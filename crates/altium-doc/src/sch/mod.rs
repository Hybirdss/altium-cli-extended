//! Schematic documents (`.SchDoc`): typed records, connectivity, project-wide nets, safe edits.
//!
//! - [`doc`]: load a sheet into typed objects ([`SchDoc`]).
//! - [`connect`]: sheet-level connectivity rules (documented there).
//! - [`project`]: hierarchy, net identifier scope, nets and parts across a project's sheets.
//! - [`edit`]: parameter / comment / footprint / designator edits that keep every other
//!   record byte-for-byte.
//! - [`storage`]: embedded files (images).
//!
//! Library API for the analysis crate: [`sheet_design`] and [`project_design`].

pub mod connect;
pub mod doc;
pub mod edit;
pub mod prim;
pub mod project;
pub mod records;
pub mod storage;

use std::path::Path;

use serde::Serialize;

use altium_core::error::Result;

pub use doc::{Component, LoadOptions, SchDoc, SchRecord, load, load_bytes, load_with};
pub use project::{project_design, Hierarchy, ProjectNetlist};

use crate::model::{Design, Net, Node, Part};

/// Add a component instance to a part list, merging sub-parts (`U1A`, `U1B`) into one part.
pub(crate) fn merge_part(parts: &mut Vec<Part>, c: &Component, designator: &str, file: &str) {
    if let Some(p) = parts.iter_mut().find(|p| p.designator == designator) {
        if let Some(letter) = &c.part
            && !p.units.contains(letter)
        {
            p.units.push(letter.clone());
            p.units.sort();
        }
        if !p.documents.iter().any(|d| d == file) {
            p.documents.push(file.to_owned());
        }
        for prm in &c.parameters {
            if !prm.name.eq_ignore_ascii_case("Comment") && !p.parameters.contains_key(&prm.name) {
                p.parameters.insert(prm.name.clone(), prm.effective().to_owned());
            }
        }
        if p.footprint.is_empty() {
            p.footprint = c.footprint.clone().unwrap_or_default();
        }
        return;
    }
    parts.push(Part {
        designator: designator.to_owned(),
        lib_ref: c.lib_ref.clone(),
        comment: c.comment.clone(),
        description: c.description.clone(),
        footprint: c.footprint.clone().unwrap_or_default(),
        library: c.library.clone().unwrap_or_default(),
        parameters: c
            .parameters
            .iter()
            .filter(|p| !p.name.eq_ignore_ascii_case("Comment"))
            .map(|p| (p.name.clone(), p.effective().to_owned()))
            .collect(),
        documents: vec![file.to_owned()],
        unique_id: c.unique_id.clone(),
        units: c.part.iter().cloned().collect(),
        side: None,
        x: Some(c.location.x),
        y: Some(c.location.y),
        rotation: Some(c.orientation),
        exclude_from_bom: c.exclude_from_bom(),
        kind: c.kind.clone(),
    });
}

/// `model::Design` of one sheet (source `schematic`), with Altium's default naming options.
/// Unnamed single-pin nets are left out as Altium does by default (`NetlistSinglePinNets` off).
pub fn sheet_design(path: &Path) -> Result<Design> {
    let d = load(path)?;
    Ok(design_of(&d, &connect::NetOptions::default()))
}

pub fn design_of(d: &SchDoc, opts: &connect::NetOptions) -> Design {
    let g = connect::graph(d);
    let mut warnings = d.warnings.clone();
    let mut nets = Vec::new();
    let mut single = 0;
    for n in connect::sheet_nets(d, &g, opts) {
        if n.nodes.is_empty() {
            continue;
        }
        if n.nodes.len() == 1 && n.auto_named {
            single += 1;
            if !opts.single_pin_nets {
                continue;
            }
        }
        nets.push(Net {
            name: n.name,
            // Footprint pad numbers (pin map applied), so nodes match the PCB.
            nodes: n
                .nodes
                .into_iter()
                .flat_map(|x| {
                    let pads = if x.pads.is_empty() { vec![x.pin.clone()] } else { x.pads.clone() };
                    pads.into_iter().map(move |pad| Node { designator: x.designator.clone(), pin: pad, pin_name: x.pin_name.clone() })
                })
                .collect(),
            auto_named: n.auto_named,
            name_source: n.name_source,
        });
    }
    if single > 0 && !opts.single_pin_nets {
        warnings.push(format!("{single} unnamed single-pin net(s) left out (NetlistSinglePinNets is off by default)"));
    }
    let mut parts = Vec::new();
    let file = d.file_name();
    for c in &d.components {
        merge_part(&mut parts, c, &c.designator, &file);
    }
    parts.sort_by_key(|p| connect::natural_key(&p.designator));
    Design { source: "schematic".into(), parts, nets, warnings }
}

/// One hit of a text search.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub index: usize,
    #[serde(rename = "type")]
    pub record_type: String,
    /// Owning component designator, when the record belongs to a component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub key: String,
    pub value: String,
    /// Stream (`FileHeader`, `Additional`, or a SchLib symbol name).
    pub stream: String,
}

/// Search every text property of every record (case-insensitive substring). Values are
/// compared as decoded (`%UTF8%` copies preferred over ANSI duplicates).
pub fn find_text(records: &[(usize, String, i64, &altium_format::props::Props)], needle: &str, owner: impl Fn(usize) -> Option<String>) -> Vec<Hit> {
    let n = needle.to_lowercase();
    let mut out = Vec::new();
    for (index, stream, record, props) in records {
        for (k, _) in props.entries() {
            if k.starts_with("%UTF8%") || k.to_ascii_uppercase().starts_with("UNICODE__") {
                continue;
            }
            let v = props.get(k).unwrap_or_default();
            if v.to_lowercase().contains(&n) {
                out.push(Hit {
                    index: *index,
                    record_type: records::type_name(*record).to_owned(),
                    owner: owner(*index),
                    key: k.to_owned(),
                    value: v,
                    stream: stream.clone(),
                });
            }
        }
    }
    out
}

impl SchDoc {
    pub fn find(&self, needle: &str) -> Vec<Hit> {
        let recs: Vec<(usize, String, i64, &altium_format::props::Props)> =
            self.records.iter().map(|r| (r.index, r.stream.to_owned(), r.record, &r.props)).collect();
        find_text(&recs, needle, |i| self.owner_designator(i))
    }
}
