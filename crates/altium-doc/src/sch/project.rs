//! Project-wide schematic view: sheet hierarchy, net identifier scope, nets across sheets and
//! parts merged across sheets.
//!
//! Net identifier scope comes from the project's `[Design] HierarchyMode`:
//! `0` automatic (default), `1` flat, `2` hierarchical, `3` global, `4` strict hierarchical.
//! (The numbering follows the order of Altium's Options for Project dialog; every sample uses
//! 0, Altium's default.) Automatic resolves to hierarchical when any sheet has sheet symbols
//! with entries, else flat when there are ports, else global.
//!
//! Joins across sheet instances, per scope:
//! - power ports are global by name (all scopes except strict hierarchical);
//! - hierarchical / strict: a port in a child sheet joins the net of the parent sheet entry
//!   with the same name on the sheet symbol that placed the child;
//! - flat: ports are global by name (sheet entries are ignored);
//! - global: ports and net labels are global by name.
//!
//! Bus ports / entries / labels (`D[0..7]`) join member nets `D0..D7` (named by net labels)
//! in the same way. A child sheet placed by several sheet symbols (or a `Repeat(...)` symbol)
//! becomes several instances whose designators are renamed with the project's
//! `ChannelDesignatorFormatString` (`$Component`, `$RoomName`, `$ChannelIndex`,
//! `$ChannelAlpha`) — a best effort, flagged with a warning.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::error::{Error, Result};

use super::connect::{self, NameCandidate, NetOptions, SheetGraph, Uf, natural_key};
use super::doc::{self, SchDoc};
use crate::model::{Design, Net, Node};
use crate::project::{Project, resolve_ci};

/// Effective net identifier scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Flat,
    Hierarchical,
    Global,
    StrictHierarchical,
}

/// Configured mode name for `HierarchyMode`.
pub fn mode_name(v: i64) -> &'static str {
    match v {
        0 => "automatic",
        1 => "flat",
        2 => "hierarchical",
        3 => "global",
        4 => "strict_hierarchical",
        _ => "unknown",
    }
}

/// Options for net naming read from a project.
pub fn net_options(p: &Project) -> NetOptions {
    let flag = |k: &str, d: bool| p.design(k).map(|v| v.trim() == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(d);
    let d = NetOptions::default();
    NetOptions {
        power_port_names_take_priority: flag("PowerPortNamesTakePriority", d.power_port_names_take_priority),
        allow_port_net_names: flag("AllowPortNetNames", d.allow_port_net_names),
        allow_sheet_entry_net_names: flag("AllowSheetEntryNetNames", d.allow_sheet_entry_net_names),
        single_pin_nets: flag("NetlistSinglePinNets", d.single_pin_nets),
        append_sheet_number_to_local_nets: flag("AppendSheetNumberToLocalNets", d.append_sheet_number_to_local_nets),
        higher_level_names_take_priority: flag("NameNetsHierarchically", d.higher_level_names_take_priority),
    }
}

/// One node of the sheet tree.
#[derive(Debug, Clone, Serialize)]
pub struct SheetNode {
    /// Sheet file name.
    pub file: String,
    pub path: PathBuf,
    /// Sheet symbol designator that placed this sheet (omitted for roots).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet_name: Option<String>,
    /// Record index of that sheet symbol in the parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_record: Option<usize>,
    pub exists: bool,
    /// Listed in the project's documents.
    pub in_project: bool,
    /// The same file is placed by more than one sheet symbol (multi-channel).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub repeated: bool,
    /// `Repeat(name, first, last)` channels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat: Option<doc::Repeat>,
    pub components: usize,
    pub children: Vec<SheetNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hierarchy {
    /// `HierarchyMode` as configured (`automatic`, `flat`, ...).
    pub configured_scope: String,
    /// Scope actually used for connectivity.
    pub scope: Scope,
    pub roots: Vec<SheetNode>,
    /// Sheets used by more than one sheet symbol.
    pub repeated_sheets: Vec<String>,
    pub warnings: Vec<String>,
}

/// The project's schematics, loaded.
pub struct ProjectSheets {
    pub docs: Vec<SchDoc>,
    /// Project-listed index (`[DocumentN]`) of each loaded doc, aligned with `docs`.
    pub in_project: Vec<bool>,
    pub warnings: Vec<String>,
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

/// Load every schematic of the project, plus children reached through sheet symbols that
/// the project does not list (warned).
pub fn load_sheets(project: &Project) -> Result<ProjectSheets> {
    let mut docs = Vec::new();
    let mut in_project = Vec::new();
    let mut warnings = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for d in project.docs(altium_format::DocKind::Schematic) {
        if !d.exists {
            warnings.push(format!("project document {} does not exist", d.rel));
            continue;
        }
        if !seen.insert(key(&d.path)) {
            continue;
        }
        docs.push(doc::load(&d.path)?);
        in_project.push(true);
    }
    // Children referenced by sheet symbols but not listed in the project.
    let mut i = 0;
    while i < docs.len() {
        let dir = docs[i].path.parent().map(Path::to_path_buf).unwrap_or_default();
        let files: Vec<String> = docs[i].sheet_symbols.iter().map(|s| s.file_name.clone()).collect();
        for f in files {
            if f.is_empty() {
                continue;
            }
            let p = resolve_child(&dir, &f, &docs);
            if let Some(p) = p
                && seen.insert(key(&p))
                && p.exists()
            {
                warnings.push(format!("{f} is placed by a sheet symbol but is not in the project; loaded anyway"));
                docs.push(doc::load(&p)?);
                in_project.push(false);
            }
        }
        i += 1;
    }
    Ok(ProjectSheets { docs, in_project, warnings })
}

/// Resolve a sheet symbol's file name: an already loaded sheet with the same file name, else
/// the path relative to the parent sheet's folder (case-insensitive).
fn resolve_child(parent_dir: &Path, file: &str, docs: &[SchDoc]) -> Option<PathBuf> {
    let base = file.rsplit(['\\', '/']).next().unwrap_or(file);
    let rel = resolve_ci(parent_dir, file);
    if rel.exists() {
        return Some(rel);
    }
    if let Some(d) = docs.iter().find(|d| d.file_name().eq_ignore_ascii_case(base)) {
        return Some(d.path.clone());
    }
    Some(rel)
}

fn child_index(sheets: &ProjectSheets, parent: usize, file: &str) -> Option<usize> {
    let dir = sheets.docs[parent].path.parent().map(Path::to_path_buf).unwrap_or_default();
    let p = resolve_child(&dir, file, &sheets.docs)?;
    let k = key(&p);
    sheets.docs.iter().position(|d| key(&d.path) == k)
}

/// Resolve the effective scope for a project.
pub fn scope(project: &Project, sheets: &ProjectSheets) -> (String, Scope) {
    let raw = project.design("HierarchyMode").and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
    let configured = mode_name(raw).to_owned();
    let s = match raw {
        1 => Scope::Flat,
        2 => Scope::Hierarchical,
        3 => Scope::Global,
        4 => Scope::StrictHierarchical,
        _ => {
            let entries = sheets.docs.iter().any(|d| d.sheet_symbols.iter().any(|s| !s.entries.is_empty()));
            let ports = sheets.docs.iter().any(|d| !d.ports.is_empty());
            if entries {
                Scope::Hierarchical
            } else if ports {
                Scope::Flat
            } else {
                Scope::Global
            }
        }
    };
    (configured, s)
}

/// Sheet tree from sheet symbols.
pub fn hierarchy(project: &Project) -> Result<Hierarchy> {
    let sheets = load_sheets(project)?;
    let (configured, sc) = scope(project, &sheets);
    let mut warnings = sheets.warnings.clone();
    let mut referenced: HashMap<usize, usize> = HashMap::new();
    for (i, d) in sheets.docs.iter().enumerate() {
        for s in &d.sheet_symbols {
            if let Some(c) = child_index(&sheets, i, &s.file_name) {
                *referenced.entry(c).or_default() += 1;
            }
        }
    }
    let repeated_sheets: Vec<String> = referenced
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(i, _)| sheets.docs[*i].file_name())
        .collect();
    fn node(
        sheets: &ProjectSheets,
        referenced: &HashMap<usize, usize>,
        i: usize,
        via: Option<&doc::SheetSymbol>,
        stack: &mut Vec<usize>,
        warnings: &mut Vec<String>,
    ) -> SheetNode {
        let d = &sheets.docs[i];
        stack.push(i);
        let mut children = Vec::new();
        for s in &d.sheet_symbols {
            match child_index(sheets, i, &s.file_name) {
                Some(c) if stack.contains(&c) => {
                    warnings.push(format!("sheet hierarchy cycle: {} places {}", d.file_name(), s.file_name));
                }
                Some(c) => children.push(node(sheets, referenced, c, Some(s), stack, warnings)),
                None => {
                    let dir = d.path.parent().map(Path::to_path_buf).unwrap_or_default();
                    warnings.push(format!("{}: sheet symbol {:?} points to missing {}", d.file_name(), s.sheet_name, s.file_name));
                    children.push(SheetNode {
                        file: s.file_name.clone(),
                        path: resolve_ci(&dir, &s.file_name),
                        sheet_name: Some(s.sheet_name.clone()),
                        symbol_record: Some(s.index),
                        exists: false,
                        in_project: false,
                        repeated: false,
                        repeat: s.repeat.clone(),
                        components: 0,
                        children: vec![],
                    });
                }
            }
        }
        stack.pop();
        SheetNode {
            file: d.file_name(),
            path: d.path.clone(),
            sheet_name: via.map(|s| s.sheet_name.clone()),
            symbol_record: via.map(|s| s.index),
            exists: true,
            in_project: sheets.in_project[i],
            repeated: referenced.get(&i).copied().unwrap_or(0) > 1,
            repeat: via.and_then(|s| s.repeat.clone()),
            components: d.components.len(),
            children,
        }
    }
    let mut roots = Vec::new();
    for i in 0..sheets.docs.len() {
        if !referenced.contains_key(&i) {
            roots.push(node(&sheets, &referenced, i, None, &mut vec![], &mut warnings));
        }
    }
    if roots.len() > 1 && matches!(sc, Scope::Hierarchical | Scope::StrictHierarchical) {
        warnings.push(format!(
            "{} root sheets in a hierarchical design: {}",
            roots.len(),
            roots.iter().map(|r| r.file.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(Hierarchy { configured_scope: configured, scope: sc, roots, repeated_sheets, warnings })
}

/// One placement of a sheet in the design.
#[derive(Debug, Clone)]
struct Instance {
    doc: usize,
    parent: Option<(usize, usize)>, // (parent instance, sheet symbol vec index in parent doc)
    depth: usize,
    /// Designator renaming for multi-channel instances.
    channel: Option<Channel>,
}

#[derive(Debug, Clone)]
struct Channel {
    room: String,
    index: usize,
}

fn channel_designator(format: &str, designator: &str, ch: &Channel) -> String {
    let fmt = if format.trim().is_empty() { "$Component_$RoomName" } else { format };
    let alpha = super::records::part_letter(ch.index as i64);
    fmt.replace("$Component", designator)
        .replace("$RoomName", &ch.room)
        .replace("$ChannelIndex", &ch.index.to_string())
        .replace("$ChannelAlpha", &alpha)
}

fn instances(sheets: &ProjectSheets, sc: Scope, warnings: &mut Vec<String>) -> Vec<Instance> {
    let mut out = Vec::new();
    if matches!(sc, Scope::Flat | Scope::Global) {
        for i in 0..sheets.docs.len() {
            out.push(Instance { doc: i, parent: None, depth: 0, channel: None });
        }
        return out;
    }
    let mut referenced: HashMap<usize, usize> = HashMap::new();
    for (i, d) in sheets.docs.iter().enumerate() {
        for s in &d.sheet_symbols {
            if let Some(c) = child_index(sheets, i, &s.file_name) {
                *referenced.entry(c).or_default() += 1;
            }
        }
    }
    let mut placed_count: HashMap<usize, usize> = HashMap::new();
    #[allow(clippy::too_many_arguments)]
    fn walk(
        sheets: &ProjectSheets,
        referenced: &HashMap<usize, usize>,
        doc: usize,
        parent: Option<(usize, usize)>,
        depth: usize,
        channel: Option<Channel>,
        stack: &mut Vec<usize>,
        out: &mut Vec<Instance>,
        placed: &mut HashMap<usize, usize>,
        warnings: &mut Vec<String>,
    ) {
        let me = out.len();
        out.push(Instance { doc, parent, depth, channel });
        stack.push(doc);
        for (si, s) in sheets.docs[doc].sheet_symbols.iter().enumerate() {
            let Some(c) = child_index(sheets, doc, &s.file_name) else { continue };
            if stack.contains(&c) {
                continue;
            }
            let channels: Vec<Option<Channel>> = if let Some(r) = &s.repeat {
                warnings.push(format!(
                    "{}: Repeat({},{},{}) multi-channel sheet; Repeat() sheet entries are not expanded",
                    sheets.docs[doc].file_name(),
                    r.name,
                    r.first,
                    r.last
                ));
                (r.first..=r.last.max(r.first))
                    .map(|i| Some(Channel { room: format!("{}{}", r.name, i), index: i.max(0) as usize }))
                    .collect()
            } else if referenced.get(&c).copied().unwrap_or(0) > 1 {
                let n = placed.entry(c).or_default();
                *n += 1;
                vec![Some(Channel { room: s.sheet_name.clone(), index: *n })]
            } else {
                vec![None]
            };
            for ch in channels {
                walk(sheets, referenced, c, Some((me, si)), depth + 1, ch, stack, out, placed, warnings);
            }
        }
        stack.pop();
    }
    for i in 0..sheets.docs.len() {
        if !referenced.contains_key(&i) {
            walk(sheets, &referenced, i, None, 0, None, &mut vec![], &mut out, &mut placed_count, warnings);
        }
    }
    // Sheets only reachable through a cycle.
    for i in 0..sheets.docs.len() {
        if !out.iter().any(|x| x.doc == i) {
            warnings.push(format!("{} is not reachable from a root sheet; treated as a root", sheets.docs[i].file_name()));
            walk(sheets, &referenced, i, None, 0, None, &mut vec![], &mut out, &mut placed_count, warnings);
        }
    }
    out
}

/// A project net with where it came from.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectNet {
    pub name: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_named: bool,
    pub name_source: String,
    pub nodes: Vec<Node>,
    /// Sheets the net appears on.
    pub sheets: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub other_names: Vec<String>,
}

/// Full project connectivity.
pub struct ProjectNetlist {
    pub scope: Scope,
    pub configured_scope: String,
    pub nets: Vec<ProjectNet>,
    pub parts: Vec<crate::model::Part>,
    pub warnings: Vec<String>,
    /// Project net of each pin: `(sheet path, pin record index)` → net name (first
    /// instance of a sheet). Pins on no net (unnamed single-pin nets) are absent.
    pub pin_nets: HashMap<(PathBuf, usize), String>,
}

impl ProjectNetlist {
    /// Project net name of a pin of `sheet`.
    pub fn pin_net(&self, sheet: &Path, pin_record: usize) -> Option<&str> {
        self.pin_nets.get(&(canonical(sheet), pin_record)).map(String::as_str)
    }
}

pub fn netlist(project: &Project) -> Result<ProjectNetlist> {
    let sheets = load_sheets(project)?;
    let (configured, sc) = scope(project, &sheets);
    let opts = net_options(project);
    let mut warnings = sheets.warnings.clone();
    for d in &sheets.docs {
        for w in &d.warnings {
            warnings.push(format!("{}: {w}", d.file_name()));
        }
    }
    let graphs: Vec<SheetGraph> = sheets.docs.iter().map(connect::graph).collect();
    let inst = instances(&sheets, sc, &mut warnings);
    let channel_fmt = project.design("ChannelDesignatorFormatString").unwrap_or_default();
    if inst.iter().any(|i| i.channel.is_some()) {
        warnings.push(format!(
            "multi-channel sheets: designators renamed with {:?}; compare with Altium's compiled names",
            if channel_fmt.is_empty() { "$Component_$RoomName" } else { channel_fmt.as_str() }
        ));
    }

    // Global ids: offset per instance.
    let mut offset = Vec::with_capacity(inst.len());
    let mut total = 0usize;
    for i in &inst {
        offset.push(total);
        total += graphs[i.doc].groups.len();
    }
    let mut uf = Uf::new(total);
    let gid = |ii: usize, g: usize| offset[ii] + g;

    // Per-instance lookup tables.
    let upper = |s: &str| s.to_uppercase();
    let label_groups = |ii: usize| -> HashMap<String, usize> {
        let d = &sheets.docs[inst[ii].doc];
        let mut m = HashMap::new();
        for (g, grp) in graphs[inst[ii].doc].groups.iter().enumerate() {
            for &l in &grp.net_labels {
                m.entry(upper(&d.net_labels[l].name)).or_insert(gid(ii, g));
            }
        }
        m
    };
    let labels: Vec<HashMap<String, usize>> = (0..inst.len()).map(label_groups).collect();

    let mut join_by: HashMap<(u8, String), usize> = HashMap::new();
    let mut join = |uf: &mut Uf, k: (u8, String), id: usize| {
        if k.1.is_empty() {
            return;
        }
        match join_by.get(&k) {
            Some(&first) => uf.union(first, id),
            None => {
                join_by.insert(k, id);
            }
        }
    };
    for (ii, x) in inst.iter().enumerate() {
        let d = &sheets.docs[x.doc];
        for (g, grp) in graphs[x.doc].groups.iter().enumerate() {
            if sc != Scope::StrictHierarchical {
                for &p in &grp.power_ports {
                    join(&mut uf, (2, upper(&d.power_ports[p].name)), gid(ii, g));
                }
            }
            if sc == Scope::Global {
                for &l in &grp.net_labels {
                    join(&mut uf, (1, upper(&d.net_labels[l].name)), gid(ii, g));
                }
            }
            if matches!(sc, Scope::Flat | Scope::Global) {
                for &p in &grp.ports {
                    join(&mut uf, (3, upper(&d.ports[p].name)), gid(ii, g));
                }
            }
        }
        // Bus ports global by name (flat/global): bind member nets by label name.
        if matches!(sc, Scope::Flat | Scope::Global) {
            for bg in &graphs[x.doc].bus_groups {
                for &p in &bg.ports {
                    for m in connect::expand_bus(&d.ports[p].name).unwrap_or_default() {
                        if let Some(&id) = labels[ii].get(&upper(&m)) {
                            join(&mut uf, (4, upper(&m)), id);
                        }
                    }
                }
            }
        }
    }
    // Hierarchical: child ports ↔ parent sheet entries.
    if matches!(sc, Scope::Hierarchical | Scope::StrictHierarchical) {
        let mut unmatched: Vec<String> = Vec::new();
        for (ii, x) in inst.iter().enumerate() {
            let Some((pi, si)) = x.parent else { continue };
            let pdoc = inst[pi].doc;
            let pd = &sheets.docs[pdoc];
            let d = &sheets.docs[x.doc];
            // Parent entry name → parent group id.
            let mut entry_group: HashMap<String, usize> = HashMap::new();
            for (g, grp) in graphs[pdoc].groups.iter().enumerate() {
                for &(s, e) in &grp.sheet_entries {
                    if s == si {
                        entry_group.insert(upper(&pd.sheet_symbols[s].entries[e].name), gid(pi, g));
                    }
                }
            }
            for (g, grp) in graphs[x.doc].groups.iter().enumerate() {
                for &p in &grp.ports {
                    match entry_group.get(&upper(&d.ports[p].name)) {
                        Some(&pg) => uf.union(pg, gid(ii, g)),
                        None => unmatched.push(format!("{}:{}", d.file_name(), d.ports[p].name)),
                    }
                }
            }
            // Buses: child bus port D[0..7] ↔ parent bus entry D[0..7] → members by label.
            let parent_bus_entries: HashSet<String> = graphs[pdoc]
                .bus_groups
                .iter()
                .flat_map(|bg| bg.sheet_entries.iter())
                .filter(|(s, _)| *s == si)
                .map(|&(s, e)| upper(&pd.sheet_symbols[s].entries[e].name))
                .collect();
            for bg in &graphs[x.doc].bus_groups {
                for &p in &bg.ports {
                    let name = upper(&d.ports[p].name);
                    if !parent_bus_entries.contains(&name) {
                        unmatched.push(format!("{}:{}", d.file_name(), d.ports[p].name));
                        continue;
                    }
                    for m in connect::expand_bus(&d.ports[p].name).unwrap_or_default() {
                        if let (Some(&a), Some(&b)) = (labels[pi].get(&upper(&m)), labels[ii].get(&upper(&m))) {
                            uf.union(a, b);
                        }
                    }
                }
            }
        }
        if !unmatched.is_empty() {
            unmatched.sort();
            unmatched.dedup();
            warnings.push(format!(
                "{} port(s) in child sheets have no sheet entry of the same name on their sheet symbol: {}",
                unmatched.len(),
                unmatched.join(", ")
            ));
        }
    }

    // Designators per instance.
    let designator = |ii: usize, d: &str| -> String {
        match &inst[ii].channel {
            Some(ch) => channel_designator(&channel_fmt, d, ch),
            None => d.to_owned(),
        }
    };

    // Collect merged nets.
    let mut members: IndexMap<usize, Vec<(usize, usize)>> = IndexMap::new();
    for (ii, x) in inst.iter().enumerate() {
        for g in 0..graphs[x.doc].groups.len() {
            let r = uf.find(gid(ii, g));
            members.entry(r).or_default().push((ii, g));
        }
    }
    let mut nets = Vec::new();
    let mut pin_nets: HashMap<(PathBuf, usize), String> = HashMap::new();
    let mut single = 0usize;
    for (_, groups) in members {
        let mut cands: Vec<NameCandidate> = Vec::new();
        let mut has_port = false;
        let mut nodes: Vec<Node> = Vec::new();
        let mut sheet_files: Vec<String> = Vec::new();
        for &(ii, g) in &groups {
            let x = &inst[ii];
            let d = &sheets.docs[x.doc];
            let grp = &graphs[x.doc].groups[g];
            cands.extend(connect::candidates(d, grp, &opts, if opts.higher_level_names_take_priority { x.depth } else { 0 }));
            has_port |= !grp.ports.is_empty();
            for &(c, p) in &grp.pins {
                let comp = &d.components[c];
                let pin = &comp.pins[p];
                // Nodes carry footprint pad numbers (pin map applied) so they match the PCB.
                for pad in comp.pads_for(&pin.designator) {
                    nodes.push(Node { designator: designator(ii, &comp.designator), pin: pad, pin_name: pin.name.clone() });
                }
            }
            let f = d.file_name();
            if !sheet_files.contains(&f) {
                sheet_files.push(f);
            }
        }
        // A sheet entry always meets the child's port of the same name, and in every sample
        // such nets keep Altium's automatic name although AllowSheetEntryNetNames=1 (6 nets
        // in 3 projects): the entry↔port pair only names the net when ports may name nets.
        if has_port && !opts.allow_port_net_names {
            cands.retain(|c| c.source != "sheet-entry");
        }
        nodes.sort_by_key(|n| (natural_key(&n.designator), natural_key(&n.pin)));
        nodes.dedup_by(|a, b| a.designator == b.designator && a.pin == b.pin);
        if nodes.is_empty() {
            continue;
        }
        let (mut name, auto, source) = match connect::pick(&cands) {
            Some(c) => (c.name.clone(), false, c.source.to_owned()),
            None => {
                let pairs: Vec<(String, String)> = nodes.iter().map(|n| (n.designator.clone(), n.pin.clone())).collect();
                (connect::auto_name(&pairs).unwrap_or_default(), true, "pin".to_owned())
            }
        };
        if opts.append_sheet_number_to_local_nets && source == "net-label" {
            let insts: HashSet<usize> = groups.iter().map(|g| g.0).collect();
            if insts.len() == 1 {
                let ii = *insts.iter().next().unwrap();
                let d = &sheets.docs[inst[ii].doc];
                if let Some(n) = d.doc_param("SheetNumber").map(|p| p.value.clone()).filter(|v| !v.is_empty() && v != "*") {
                    name = format!("{name}_{n}");
                }
            }
        }
        let mut other: Vec<String> = cands.iter().map(|c| c.name.clone()).filter(|n| !n.eq_ignore_ascii_case(&name)).collect();
        other.sort_by_key(|s| natural_key(s));
        other.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        if nodes.len() == 1 && auto {
            single += 1;
            if !opts.single_pin_nets {
                continue;
            }
        }
        for &(ii, g) in &groups {
            let x = &inst[ii];
            let d = &sheets.docs[x.doc];
            for &(c, p) in &graphs[x.doc].groups[g].pins {
                pin_nets.entry((canonical(&d.path), d.components[c].pins[p].index)).or_insert_with(|| name.clone());
            }
        }
        nets.push(ProjectNet { name, auto_named: auto, name_source: source, nodes, sheets: sheet_files, other_names: other });
    }
    if single > 0 && !opts.single_pin_nets {
        warnings.push(format!("{single} unnamed single-pin net(s) left out (project option NetlistSinglePinNets is off)"));
    }
    // Same name used by distinct nets.
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for n in &nets {
        *by_name.entry(n.name.to_uppercase()).or_default() += 1;
    }
    let dup: Vec<&String> = by_name.iter().filter(|(_, c)| **c > 1).map(|(n, _)| n).collect();
    if !dup.is_empty() {
        warnings.push(format!("{} net name(s) are used by more than one unconnected net: {}", dup.len(), dup.iter().take(10).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
    }
    nets.sort_by_key(|n| natural_key(&n.name));

    // Parts merged by final designator.
    let mut parts: Vec<crate::model::Part> = Vec::new();
    for (ii, x) in inst.iter().enumerate() {
        let d = &sheets.docs[x.doc];
        for c in &d.components {
            super::merge_part(&mut parts, c, &designator(ii, &c.designator), &d.file_name());
        }
    }
    parts.sort_by_key(|p| natural_key(&p.designator));
    Ok(ProjectNetlist { scope: sc, configured_scope: configured, nets, parts, warnings, pin_nets })
}

/// `model::Design` for a whole project (source `schematic`).
pub fn project_design(project: &Project) -> Result<Design> {
    let n = netlist(project)?;
    Ok(Design {
        source: "schematic".into(),
        parts: n.parts,
        nets: n
            .nets
            .into_iter()
            .map(|p| Net { name: p.name, nodes: p.nodes, auto_named: p.auto_named, name_source: p.name_source })
            .collect(),
        warnings: n.warnings,
    })
}

/// Stable sheet-local references for project-wide component lookup.
#[derive(Debug, Clone, Serialize)]
pub struct LocatedComponent {
    pub designator: String,
    pub sheet: PathBuf,
    pub index: usize,
    pub unique_id: String,
    pub comment: String,
    pub footprint: Option<String>,
}

pub fn locate(project: &Project, designators: &[String]) -> Result<(Vec<LocatedComponent>, Vec<String>)> {
    let mut items = vec![];
    let mut warnings: Vec<_> = project.documents.iter().filter(|d| !d.generated && !d.exists)
        .map(|d| format!("project document {} does not exist; lookup may be partial", d.rel)).collect();
    let mut available = std::collections::BTreeSet::new();
    for path in project.schematics() {
        let doc = super::load(&path)?;
        warnings.extend(doc.warnings);
        for c in doc.components {
            available.insert(c.designator.clone());
            if designators.iter().any(|q| c.designator.eq_ignore_ascii_case(q) || c.display_designator.eq_ignore_ascii_case(q)) {
                items.push(LocatedComponent { designator:c.display_designator, sheet:path.clone(), index:c.index,
                    unique_id:c.unique_id, comment:c.comment, footprint:c.footprint });
            }
        }
    }
    if items.is_empty() {
        let names: Vec<_> = available.into_iter().take(5).collect();
        return Err(Error::not_found(format!("no requested designator in {}", project.path.display()))
            .details(serde_json::json!({"similar":names}))
            .hint("read error.details.similar, or list parts with `ace bom PROJECT --group-by designator`"));
    }
    for q in designators {
        if !items.iter().any(|c| c.designator.eq_ignore_ascii_case(q) || c.designator.trim_end_matches(|c: char| c.is_ascii_alphabetic()).eq_ignore_ascii_case(q)) {
            warnings.push(format!("no component matched {q}; inspect the project's BOM"));
        }
    }
    Ok((items, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names() {
        let ch = Channel { room: "CH".into(), index: 2 };
        assert_eq!(channel_designator("$Component_$RoomName", "R1", &ch), "R1_CH");
        assert_eq!(channel_designator("$Component$ChannelAlpha", "R1", &ch), "R1B");
        assert_eq!(mode_name(0), "automatic");
    }
}
