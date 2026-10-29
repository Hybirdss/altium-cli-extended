//! Native design checks: annotation, footprints, nets, and schematic ↔ PCB synchronisation.
//!
//! These are fast pre-flight checks. They do not replace Altium's ERC/DRC
//! (`ace designer erc|drc`), which apply the project's own rules.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;

use altium_doc::model::{Design, Part};

use crate::designator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn parse(s: &str) -> Option<Severity> {
        match s.to_ascii_lowercase().as_str() {
            "info" => Some(Severity::Info),
            "warning" | "warn" => Some(Severity::Warning),
            "error" => Some(Severity::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    /// Stable machine code, e.g. `duplicate-designator`.
    pub code: &'static str,
    pub message: String,
    /// Designators / nets / `designator.pin` nodes involved (usable in follow-up commands).
    pub refs: Vec<String>,
    /// `project`, `schematic`, `pcb`, `design` (either document) or `sync` (schematic vs PCB).
    pub scope: &'static str,
    /// What to do about it.
    pub hint: &'static str,
}

/// Build a finding; scope and hint come from [`CODES`].
pub fn finding(severity: Severity, code: &'static str, message: String, refs: Vec<String>) -> Finding {
    let (scope, hint) = CODES.iter().find(|c| c.0 == code).map(|c| (c.1, c.3)).unwrap_or(("project", ""));
    Finding { severity, code, message, refs, scope, hint }
}

fn f(severity: Severity, code: &'static str, message: String, refs: Vec<String>) -> Finding {
    finding(severity, code, message, refs)
}

/// Every check: `(code, scope, meaning, fix hint)` (for `ace check --list`).
pub const CODES: &[(&str, &str, &str, &str)] = &[
    ("incomplete-project", "project", "design documents are missing; results are partial and sync is skipped",
     "restore the files listed in refs; inspect `ace project documents <project>` before trusting BOM or sync"),
    ("missing-document", "project", "the project references a document that is not on disk",
     "restore the file or drop the entry with `ace project remove-document`"),
    ("unannotated", "design", "designator still contains '?'",
     "annotate in Altium (`ace designer process Sch:Annotate`) or set it with `ace sch set-designator`"),
    ("duplicate-designator", "design", "two parts share a designator",
     "rename one with `ace sch set-designator`, then update the PCB from the schematic"),
    ("missing-footprint", "design", "part has no footprint model",
     "add a footprint model in Altium; `ace sch set-footprint` can only rename an existing one"),
    ("empty-comment", "design", "part has no comment/value", "set it with `ace sch set-comment`"),
    ("missing-parameter", "design", "part lacks a parameter named in --require",
     "add it with `ace sch set-param <sheet> <designator> <name> <value>`"),
    ("single-node-net", "design", "named net with only one connection (often a typo in a net label)",
     "check the net label spelling (`ace sch net-labels`) or connect the second pin"),
    ("node-in-several-nets", "design", "the same pin appears in two nets",
     "two nets are shorted through one pin; inspect with `ace netlist <input> --pins --where designator=<ref>`"),
    ("missing-in-pcb", "sync", "schematic part has no PCB component",
     "update the PCB from the schematic in Altium (Design > Update PCB Document)"),
    ("missing-in-schematic", "sync", "PCB component has no schematic part (and is not mechanical/graphical)",
     "delete it from the PCB, add it to the schematic, or make it a Mechanical/Graphical component"),
    ("unique-id-mismatch", "sync", "schematic and PCB parts share a designator but not the unique id that links them",
     "re-link with Altium's Component Links dialog (Project > Component Links) before updating the PCB"),
    ("designator-mismatch", "sync", "a linked schematic part and PCB component have different designators",
     "annotate both sides the same way: change the schematic (`ace sch set-designator`) and update the PCB, or back-annotate from the PCB in Altium"),
    ("footprint-mismatch", "sync", "schematic footprint differs from the PCB pattern",
     "fix the schematic with `ace sch set-footprint`, then update the PCB in Altium"),
    ("comment-mismatch", "sync", "schematic comment differs from the PCB comment",
     "set the schematic comment (`ace sch set-comment`) and update the PCB, or ignore an intentional difference"),
    ("net-mismatch", "sync", "one schematic net is split across several PCB nets",
     "update the PCB from the schematic; see `ace diff <project> <pcb>`"),
    ("nets-merged-in-pcb", "sync", "several schematic nets are one PCB net",
     "update the PCB from the schematic; a PCB-only net assignment shorts them"),
    ("pin-unrouted-in-pcb", "sync", "a schematic net node's PCB pad has no net",
     "update the PCB from the schematic so the pad gets its net"),
];

fn graphical(p: &Part) -> bool {
    matches!(p.kind.to_ascii_lowercase().as_str(), "graphical" | "mechanical" | "jumper" | "net tie")
}

fn part_checks(d: &Design, out: &mut Vec<Finding>) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for p in &d.parts {
        *seen.entry(p.designator.to_ascii_uppercase()).or_default() += 1;
        if designator::unannotated(&p.designator) {
            out.push(f(Severity::Error, "unannotated", format!("{} ({}) is not annotated", p.designator, d.source), vec![p.designator.clone()]));
        }
        if p.footprint.trim().is_empty() && !graphical(p) {
            out.push(f(Severity::Warning, "missing-footprint", format!("{} has no footprint ({})", p.designator, d.source), vec![p.designator.clone()]));
        }
        if p.comment.trim().is_empty() && !graphical(p) {
            out.push(f(Severity::Info, "empty-comment", format!("{} has an empty comment ({})", p.designator, d.source), vec![p.designator.clone()]));
        }
    }
    let mut dups: Vec<String> = seen.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k).collect();
    designator::sort(&mut dups);
    for dsg in dups {
        if !designator::unannotated(&dsg) {
            out.push(f(Severity::Error, "duplicate-designator", format!("{dsg} is used more than once ({})", d.source), vec![dsg]));
        }
    }
}

fn net_checks(d: &Design, out: &mut Vec<Finding>) {
    let mut owner: HashMap<(String, String), Vec<String>> = HashMap::new();
    for n in &d.nets {
        let uniq: HashSet<(&str, &str)> = n.nodes.iter().map(|x| (x.designator.as_str(), x.pin.as_str())).collect();
        if uniq.len() == 1 && !n.auto_named {
            out.push(f(
                Severity::Warning,
                "single-node-net",
                format!("net {} has a single connection ({})", n.name, d.source),
                vec![n.name.clone()],
            ));
        }
        for (des, pin) in uniq {
            owner.entry((des.to_owned(), pin.to_owned())).or_default().push(n.name.clone());
        }
    }
    let mut multi: Vec<_> = owner.into_iter().filter(|(_, v)| v.len() > 1).collect();
    multi.sort_by(|a, b| designator::cmp(&a.0.0, &b.0.0).then(designator::cmp(&a.0.1, &b.0.1)));
    for ((des, pin), mut nets) in multi {
        nets.sort();
        out.push(f(
            Severity::Error,
            "node-in-several-nets",
            format!("{des}.{pin} is in several nets: {} ({})", nets.join(", "), d.source),
            std::iter::once(format!("{des}.{pin}")).chain(nets).collect(),
        ));
    }
}

fn pin_nets(d: &Design, rename: &HashMap<String, String>) -> HashMap<(String, String), String> {
    d.nets
        .iter()
        .flat_map(|n| {
            n.nodes.iter().map(move |x| {
                let des = x.designator.to_ascii_uppercase();
                let des = rename.get(&des).cloned().unwrap_or(des);
                ((des, x.pin.to_ascii_uppercase()), n.name.clone())
            })
        })
        .collect()
}

/// Pair PCB components with schematic parts: by the unique id Altium links them with (when
/// it is unique on both sides; repeated channels share one), else by designator.
/// Returns `(schematic part index, PCB part index)` pairs.
fn link(sch: &Design, pcb: &Design) -> Vec<(usize, usize)> {
    let uid_count = |d: &Design| {
        let mut m: HashMap<String, usize> = HashMap::new();
        for p in d.parts.iter().filter(|p| !p.unique_id.is_empty()) {
            *m.entry(p.unique_id.to_ascii_uppercase()).or_default() += 1;
        }
        m
    };
    let (su, pu) = (uid_count(sch), uid_count(pcb));
    let unique = |p: &Part, m: &HashMap<String, usize>| !p.unique_id.is_empty() && m.get(&p.unique_id.to_ascii_uppercase()) == Some(&1);
    let mut pairs = vec![];
    let mut used_pcb: HashSet<usize> = HashSet::new();
    let mut used_sch: HashSet<usize> = HashSet::new();
    for (i, p) in sch.parts.iter().enumerate() {
        if !unique(p, &su) {
            continue;
        }
        if let Some(j) = pcb.parts.iter().position(|q| unique(q, &pu) && q.unique_id.eq_ignore_ascii_case(&p.unique_id)) {
            pairs.push((i, j));
            used_pcb.insert(j);
            used_sch.insert(i);
        }
    }
    let by_des: HashMap<String, usize> =
        pcb.parts.iter().enumerate().filter(|(j, _)| !used_pcb.contains(j)).map(|(j, q)| (q.designator.to_ascii_uppercase(), j)).collect();
    for (i, p) in sch.parts.iter().enumerate() {
        if used_sch.contains(&i) {
            continue;
        }
        if let Some(&j) = by_des.get(&p.designator.to_ascii_uppercase())
            && used_pcb.insert(j)
        {
            pairs.push((i, j));
        }
    }
    pairs
}

fn sync_checks(sch: &Design, pcb: &Design, out: &mut Vec<Finding>) {
    let pairs = link(sch, pcb);
    let pcb_of: HashMap<usize, usize> = pairs.iter().copied().collect();
    let linked_pcb: HashSet<usize> = pairs.iter().map(|p| p.1).collect();
    // PCB designator -> schematic designator, for comparing nets pin by pin.
    let mut rename: HashMap<String, String> = HashMap::new();
    for &(i, j) in &pairs {
        rename.insert(pcb.parts[j].designator.to_ascii_uppercase(), sch.parts[i].designator.to_ascii_uppercase());
    }
    let mut sch_order: Vec<usize> = (0..sch.parts.len()).collect();
    sch_order.sort_by(|&a, &b| designator::cmp(&sch.parts[a].designator, &sch.parts[b].designator));
    for i in sch_order {
        let p = &sch.parts[i];
        if p.exclude_from_bom && p.footprint.is_empty() {
            continue;
        }
        match pcb_of.get(&i).map(|&j| &pcb.parts[j]) {
            None => {
                if !p.footprint.is_empty() {
                    out.push(f(Severity::Error, "missing-in-pcb", format!("{} is in the schematic but not on the PCB", p.designator), vec![p.designator.clone()]));
                }
            }
            Some(q) => {
                if !p.designator.eq_ignore_ascii_case(&q.designator) {
                    out.push(f(
                        Severity::Warning,
                        "designator-mismatch",
                        format!("schematic {} is linked to PCB component {} (designator changed on one side)", p.designator, q.designator),
                        vec![p.designator.clone(), q.designator.clone()],
                    ));
                } else if !p.unique_id.is_empty() && !q.unique_id.is_empty() && !p.unique_id.eq_ignore_ascii_case(&q.unique_id) {
                    out.push(f(
                        Severity::Warning,
                        "unique-id-mismatch",
                        format!("{}: schematic unique id {} vs PCB link {}", p.designator, p.unique_id, q.unique_id),
                        vec![p.designator.clone()],
                    ));
                }
                if !p.footprint.is_empty() && !q.footprint.is_empty() && !p.footprint.eq_ignore_ascii_case(&q.footprint) {
                    out.push(f(
                        Severity::Warning,
                        "footprint-mismatch",
                        format!("{}: schematic footprint {:?} vs PCB {:?}", p.designator, p.footprint, q.footprint),
                        vec![p.designator.clone()],
                    ));
                }
                if !p.comment.is_empty() && !q.comment.is_empty() && p.comment != q.comment {
                    out.push(f(
                        Severity::Info,
                        "comment-mismatch",
                        format!("{}: schematic comment {:?} vs PCB {:?}", p.designator, p.comment, q.comment),
                        vec![p.designator.clone()],
                    ));
                }
            }
        }
    }
    let mut pcb_order: Vec<usize> = (0..pcb.parts.len()).collect();
    pcb_order.sort_by(|&a, &b| designator::cmp(&pcb.parts[a].designator, &pcb.parts[b].designator));
    for j in pcb_order {
        let q = &pcb.parts[j];
        // `#N` = a PCB component without a designator text (logos and the like).
        let free = matches!(q.kind.to_ascii_lowercase().as_str(), "mechanical" | "graphical")
            || q.designator.is_empty()
            || q.designator.starts_with('#');
        if !linked_pcb.contains(&j) && !free {
            out.push(f(
                Severity::Warning,
                "missing-in-schematic",
                format!("{} is on the PCB but not in the schematic", q.designator),
                vec![q.designator.clone()],
            ));
        }
    }
    let linked_sch: HashSet<String> = rename.values().cloned().collect();
    // Net membership: compare which pins share a net, not the names (names may differ legally).
    let sp = pin_nets(sch, &HashMap::new());
    let pp = pin_nets(pcb, &rename);
    let mut name_map: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
    let mut unrouted: Vec<(&(String, String), &String)> = vec![];
    for (node, sn) in &sp {
        match pp.get(node) {
            Some(pn) => *name_map.entry(sn.as_str()).or_default().entry(pn.as_str()).or_default() += 1,
            None => {
                if linked_sch.contains(&node.0) {
                    unrouted.push((node, sn));
                }
            }
        }
    }
    unrouted.sort_by(|a, b| designator::cmp(&a.0.0, &b.0.0).then(designator::cmp(&a.0.1, &b.0.1)));
    for (node, sn) in unrouted {
        out.push(f(
            Severity::Warning,
            "pin-unrouted-in-pcb",
            format!("{}.{} is on net {sn} in the schematic but its PCB pad has no net", node.0, node.1),
            vec![format!("{}.{}", node.0, node.1), sn.clone()],
        ));
    }
    // One PCB net carrying pins of several schematic nets.
    let mut reverse: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (sn, m) in &name_map {
        for pn in m.keys() {
            reverse.entry(pn).or_default().insert(sn);
        }
    }
    for (pn, sns) in reverse.into_iter().filter(|(_, s)| s.len() > 1) {
        let sns: Vec<String> = sns.into_iter().map(str::to_owned).collect();
        out.push(f(
            Severity::Error,
            "nets-merged-in-pcb",
            format!("PCB net {pn} joins schematic nets {}", sns.join(", ")),
            std::iter::once(pn.to_owned()).chain(sns).collect(),
        ));
    }
    for (sn, m) in name_map.into_iter().filter(|(_, m)| m.len() > 1) {
        let pcb_nets: Vec<String> = m.keys().map(|s| s.to_string()).collect();
        out.push(f(
            Severity::Error,
            "net-mismatch",
            format!("schematic net {sn} is split across PCB nets {}", pcb_nets.join(", ")),
            std::iter::once(sn.to_owned()).chain(pcb_nets).collect(),
        ));
    }
}

/// Optional checks.
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// Parameters every BOM part must carry (`missing-parameter`), e.g. `MPN`, `Manufacturer`.
    /// `comment`, `footprint` and `description` name the built-in fields.
    pub require: Vec<String>,
}

fn required_params(d: &Design, require: &[String], out: &mut Vec<Finding>) {
    for p in d.parts.iter().filter(|p| !p.exclude_from_bom) {
        for name in require {
            let has = match name.to_ascii_lowercase().as_str() {
                "comment" => !p.comment.trim().is_empty(),
                "footprint" => !p.footprint.trim().is_empty(),
                "description" => !p.description.trim().is_empty(),
                _ => p.param(name).is_some_and(|v| !v.trim().is_empty() && v.trim() != "*"),
            };
            if !has {
                out.push(f(
                    Severity::Warning,
                    "missing-parameter",
                    format!("{} has no {name} ({})", p.designator, d.source),
                    vec![p.designator.clone(), name.clone()],
                ));
            }
        }
    }
}

/// Run every check that applies to the designs given.
pub fn run(sch: Option<&Design>, pcb: Option<&Design>) -> Vec<Finding> {
    run_with(sch, pcb, &CheckOptions::default())
}

pub fn run_with(sch: Option<&Design>, pcb: Option<&Design>, opts: &CheckOptions) -> Vec<Finding> {
    let mut out = vec![];
    // The schematic is the source of parts and nets; the PCB alone is checked only when there
    // is no schematic (with both, PCB problems surface as sync findings instead of duplicates).
    if let Some(d) = sch.or(pcb) {
        part_checks(d, &mut out);
        net_checks(d, &mut out);
    }
    if let (Some(_), Some(p)) = (sch, pcb) {
        let mut v = vec![];
        part_checks(p, &mut v);
        out.extend(v.into_iter().filter(|f| f.code == "duplicate-designator"));
    }
    // Sourcing parameters live on the schematic; check the PCB only when it is all there is.
    if let Some(d) = sch.or(pcb) {
        required_params(d, &opts.require, &mut out);
    }
    if let (Some(s), Some(p)) = (sch, pcb) {
        sync_checks(s, p, &mut out);
    }
    out.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.code.cmp(b.code)));
    out
}

/// Compare persisted findings by scope, code, severity and sorted references. Messages may
/// change between runs without inventing a new violation. A severity increase is new.
pub fn baseline_delta(before: &[serde_json::Value], after: &[serde_json::Value]) -> altium_core::Result<serde_json::Value> {
    use altium_core::Error;
    use serde_json::{Value, json};
    fn keyed(rows: &[Value]) -> altium_core::Result<BTreeMap<String, Value>> {
        let mut map = BTreeMap::new();
        for row in rows {
            let invalid = || Error::usage("baseline findings must retain code, scope, severity and refs")
                .hint("save a complete `ace check PROJECT` JSON envelope without --fields, --where or --limit");
            let mut refs: Vec<&str> = row["refs"].as_array().ok_or_else(invalid)?.iter()
                .map(|r| r.as_str().ok_or_else(invalid)).collect::<altium_core::Result<_>>()?;
            refs.sort();
            let severity = row["severity"].as_str().ok_or_else(invalid)?;
            if Severity::parse(severity).is_none() { return Err(invalid()); }
            let key = serde_json::to_string(&(row["scope"].as_str().ok_or_else(invalid)?,
                row["code"].as_str().ok_or_else(invalid)?, severity, refs))?;
            map.insert(key, row.clone());
        }
        Ok(map)
    }
    let (before, after) = (keyed(before)?, keyed(after)?);
    let new: Vec<_> = after.iter().filter(|(key, _)| !before.contains_key(*key)).map(|(_, v)| v).collect();
    let resolved: Vec<_> = before.iter().filter(|(key, _)| !after.contains_key(*key)).map(|(_, v)| v).collect();
    let unchanged = after.keys().filter(|key| before.contains_key(*key)).count();
    Ok(json!({"new":new,"resolved":resolved,"unchanged":unchanged,"passed_vs_baseline":new.is_empty()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_doc::model::{Net, Node};

    fn part(d: &str, fp: &str) -> Part {
        Part { designator: d.into(), footprint: fp.into(), comment: "x".into(), ..Default::default() }
    }
    fn net(name: &str, nodes: &[(&str, &str)]) -> Net {
        Net {
            name: name.into(),
            nodes: nodes.iter().map(|(d, p)| Node { designator: d.to_string(), pin: p.to_string(), pin_name: String::new() }).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn sync() {
        let sch = Design {
            source: "schematic".into(),
            parts: vec![part("R1", "0603"), part("R2", "0603"), part("R?", "0603"), part("R4", "0603")],
            nets: vec![
                net("VCC", &[("R1", "1"), ("R2", "1")]),
                net("LONELY", &[("R1", "2")]),
                net("A", &[("R4", "1")]),
                net("B", &[("R4", "2")]),
            ],
            ..Default::default()
        };
        let pcb = Design {
            source: "pcb".into(),
            parts: vec![part("R1", "0805"), part("R3", "0603"), part("R4", "0603"), part("#7", "LOGO")],
            nets: vec![net("VCC", &[("R1", "1")]), net("N2", &[("R2", "1")]), net("AB", &[("R4", "1"), ("R4", "2")])],
            ..Default::default()
        };
        let all = run(Some(&sch), Some(&pcb));
        let codes: Vec<&str> = all.iter().map(|f| f.code).collect();
        for c in ["unannotated", "single-node-net", "missing-in-pcb", "missing-in-schematic", "footprint-mismatch", "pin-unrouted-in-pcb", "nets-merged-in-pcb"] {
            assert!(codes.contains(&c), "{c} missing from {codes:?}");
        }
        assert!(!all.iter().any(|f| f.refs.contains(&"#7".to_owned())), "designator-less PCB parts are not reported");
        assert!(CODES.iter().all(|c| !c.0.is_empty() && !c.3.is_empty()));
        assert!(all.iter().all(|x| !x.hint.is_empty()), "every finding has a hint");
        let r = run_with(Some(&sch), None, &CheckOptions { require: vec!["MPN".into()] });
        assert!(r.iter().any(|x| x.code == "missing-parameter"));
    }
}
