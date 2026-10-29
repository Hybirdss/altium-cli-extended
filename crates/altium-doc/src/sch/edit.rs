//! Safe schematic edits.
//!
//! Edits work on the raw FileHeader records at the byte level: only the `|KEY=VALUE` fields
//! being changed are rewritten inside the records being changed; every other byte (other
//! fields, other records, other streams) is kept as-is. This avoids re-encoding text that
//! was written in a code page we may not know (e.g. GBK bytes read as windows-1252).
//!
//! - Values are written in the ANSI code page (`--codepage`; unmappable characters become
//!   `?`, as Altium does) and, when not plain ASCII or when the record already has one, also
//!   as a `%UTF8%KEY` copy. A stale `UNICODE__KEY` copy is dropped.
//! - New records are appended at the end of FileHeader: owners always precede children, so
//!   `OWNERINDEX` links stay valid and no existing record is renumbered. Header `WEIGHT` is
//!   updated.
//! - Removing records shifts the `OWNERINDEX` of records owned by later records (reported as
//!   `renumbered`); records owning other records are never removed.
//! - [`verify`] re-parses the result and checks every change and every untouched record
//!   before the caller writes anything.

use std::collections::HashSet;

use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_format::cfbx::{Compound, Record, join_records};
use altium_format::props::Props;
use altium_format::rawprops::{fields, find_field, put_field, set_text_field};

use super::doc::{SchDoc, SchRecord};

/// What an edit did.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// Record index changed / created / removed.
    pub index: usize,
    /// `set`, `create`, `remove`.
    pub action: String,
    pub designator: Option<String>,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

fn unsafe_err(m: impl Into<String>) -> Error {
    Error::new(ErrorCode::Unsafe, m)
}

/// Build a new text record from `(key, value)` pairs (text values get `%UTF8%` copies).
fn build_record(pairs: &[(String, String)]) -> Record {
    let mut data = vec![0u8];
    for (k, v) in pairs {
        set_text_field(&mut data, k, v);
    }
    Record { kind: 0, data }
}

/// An edit session over a loaded sheet.
pub struct Editor<'a> {
    doc: &'a SchDoc,
    /// FileHeader records including the header at position 0.
    recs: Vec<Record>,
    /// Records removed (indices in the original numbering, sorted).
    removed: Vec<usize>,
    pub changes: Vec<Change>,
    uids: HashSet<String>,
}

impl<'a> Editor<'a> {
    pub fn new(doc: &'a SchDoc) -> Result<Editor<'a>> {
        if doc.records.iter().any(|r| r.stream == "FileHeader" && r.binary) {
            return Err(unsafe_err("the sheet has binary FileHeader records; refusing to edit"));
        }
        let uids = doc.records.iter().filter_map(|r| r.props.raw("UNIQUEID").map(str::to_owned)).collect();
        Ok(Editor { doc, recs: doc.fh_records.clone(), removed: vec![], changes: vec![], uids })
    }

    fn record(&self, index: usize) -> Result<&SchRecord> {
        self.doc
            .records
            .get(index)
            .filter(|r| r.stream == "FileHeader")
            .ok_or_else(|| Error::internal(format!("record {index} is not a FileHeader record")))
    }

    /// Current properties of record `index` (after earlier edits).
    fn props(&self, index: usize) -> Props {
        self.recs[index + 1].props()
    }

    /// Set text `key` of record `index` (keeps the `%UTF8%` copy in sync).
    pub fn set_prop(&mut self, index: usize, key: &str, value: &str, designator: Option<String>) -> Result<bool> {
        self.record(index)?;
        let before = self.props(index).get(key);
        if before.as_deref() == Some(value) {
            return Ok(false);
        }
        let data = &mut self.recs[index + 1].data;
        if find_field(data, key).is_none() && fields(data).is_empty() {
            return Err(unsafe_err(format!("record {index} has no |KEY=VALUE| fields")));
        }
        set_text_field(data, key, value);
        self.changes.push(Change {
            index,
            action: "set".into(),
            designator,
            key: key.into(),
            before,
            after: Some(value.into()),
        });
        Ok(true)
    }

    /// A fresh `UNIQUEID` (eight letters A–Y, unique in this document).
    pub fn new_unique_id(&mut self) -> String {
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15 ^ (self.uids.len() as u64).wrapping_mul(0x1000_0000_01B3);
        for b in self.doc.path.to_string_lossy().bytes() {
            seed = seed.wrapping_mul(0x100_0000_01B3) ^ b as u64;
        }
        loop {
            let mut s = String::new();
            for _ in 0..8 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                s.push((b'A' + (seed % 25) as u8) as char);
            }
            if self.uids.insert(s.clone()) {
                return s;
            }
        }
    }

    /// Append a new FileHeader record built from `pairs`; returns its index.
    pub fn append(&mut self, pairs: &[(String, String)], designator: Option<String>, key: &str, value: &str) -> usize {
        let index = self.recs.len() - 1;
        self.recs.push(build_record(pairs));
        self.changes.push(Change {
            index,
            action: "create".into(),
            designator,
            key: key.into(),
            before: None,
            after: Some(value.into()),
        });
        index
    }

    /// Remove a record that nothing owns.
    pub fn remove(&mut self, index: usize, designator: Option<String>, key: &str) -> Result<()> {
        let r = self.record(index)?;
        if self.doc.records.iter().any(|o| o.stream == "FileHeader" && o.owner() == Some(index)) {
            return Err(unsafe_err(format!("record {index} owns other records; refusing to remove it")));
        }
        if self.doc.records.iter().any(|o| o.stream == "Additional") {
            return Err(unsafe_err("the sheet has Additional-stream records that may reference FileHeader indices; refusing to renumber"));
        }
        if self.removed.contains(&index) {
            return Ok(());
        }
        let before = r.props.get("TEXT");
        self.removed.push(index);
        self.removed.sort_unstable();
        self.changes.push(Change { index, action: "remove".into(), designator, key: key.into(), before, after: None });
        Ok(())
    }

    /// New file bytes: the edited FileHeader written back into the original compound file.
    /// Returns `(bytes, renumbered_records, changes)`.
    pub fn finish(mut self) -> Result<(Vec<u8>, usize, Vec<Change>)> {
        let mut renumbered = 0usize;
        if !self.removed.is_empty() {
            for i in 0..self.recs.len() - 1 {
                if self.removed.contains(&i) {
                    continue;
                }
                let owner = self.recs[i + 1].props().int("OWNERINDEX");
                if let Some(o) = owner.filter(|&o| o >= 0) {
                    let shift = self.removed.iter().filter(|&&g| g < o as usize).count();
                    if shift > 0 {
                        let v = (o - shift as i64).to_string();
                        put_field(&mut self.recs[i + 1].data, "OWNERINDEX", v.as_bytes(), None);
                        renumbered += 1;
                    }
                }
            }
            for &gone in self.removed.iter().rev() {
                self.recs.remove(gone + 1);
            }
        }
        let count = (self.recs.len() - 1).to_string();
        if self.recs[0].props().raw("WEIGHT") != Some(count.as_str()) {
            put_field(&mut self.recs[0].data, "WEIGHT", count.as_bytes(), None);
        }
        let mut cf = Compound::from_bytes(&self.doc.path, self.doc.bytes.clone())?;
        cf.write("FileHeader", &join_records(&self.recs))?;
        Ok((cf.into_bytes()?, renumbered, self.changes))
    }
}

/// Fields for a new parameter: copy the visual conventions of an existing hidden sibling
/// parameter (position, colour, font), else of any parameter in the sheet.
pub fn new_parameter_fields(doc: &SchDoc, owner: Option<usize>, name: &str, value: &str, uid: &str) -> Vec<(String, String)> {
    let siblings: Vec<&SchRecord> = doc.records.iter().filter(|r| r.record == 41 && r.owner() == owner).collect();
    let template = siblings
        .iter()
        .rev()
        .find(|r| r.props.bool("ISHIDDEN"))
        .or(siblings.last())
        .copied()
        .or_else(|| doc.records.iter().find(|r| r.record == 41));
    let mut p: Vec<(String, String)> = vec![("RECORD".into(), "41".into())];
    if let Some(o) = owner {
        p.push(("OWNERINDEX".into(), o.to_string()));
    }
    if let Some(next) = siblings.iter().filter_map(|r| r.props.int("INDEXINSHEET")).filter(|&i| i >= 0).max() {
        p.push(("INDEXINSHEET".into(), (next + 1).to_string()));
    }
    p.push(("OWNERPARTID".into(), "-1".into()));
    if let Some(t) = template {
        for k in ["LOCATION.X", "LOCATION.X_FRAC", "LOCATION.Y", "LOCATION.Y_FRAC", "COLOR", "FONTID"] {
            if let Some(v) = t.props.raw(k) {
                p.push((k.into(), v.into()));
            }
        }
    } else if let Some(o) = owner.and_then(|o| doc.records.get(o)) {
        for k in ["LOCATION.X", "LOCATION.Y"] {
            if let Some(v) = o.props.raw(k) {
                p.push((k.into(), v.into()));
            }
        }
    }
    p.push(("ISHIDDEN".into(), "T".into()));
    p.push(("TEXT".into(), value.into()));
    p.push(("NAME".into(), name.into()));
    p.push(("UNIQUEID".into(), uid.into()));
    p
}

/// Set (or create) a parameter on every instance of a component. `designator` matches the
/// base designator (all sub-parts) or one sub-part (`U1A`).
pub fn set_param(doc: &SchDoc, designator: &str, name: &str, value: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    if name.trim().is_empty() || name.contains(['|', '=']) || value.contains('|') {
        return Err(Error::usage("parameter names cannot be empty or contain | or =, values cannot contain |"));
    }
    let comps = doc.find_components(designator)?;
    let mut ed = Editor::new(doc)?;
    for c in comps {
        match c.param(name) {
            Some(p) => {
                ed.set_prop(p.index, "TEXT", value, Some(c.display_designator.clone()))?;
            }
            None => {
                let uid = ed.new_unique_id();
                let fields = new_parameter_fields(doc, Some(c.index), name, value, &uid);
                ed.append(&fields, Some(c.display_designator.clone()), name, value);
            }
        }
    }
    let (bytes, _, changes) = ed.finish()?;
    Ok((bytes, changes))
}

pub fn remove_param(doc: &SchDoc, designator: &str, name: &str) -> Result<(Vec<u8>, Vec<Change>, usize)> {
    let comps = doc.find_components(designator)?;
    let hits: Vec<(usize, String)> =
        comps.iter().filter_map(|c| c.param(name).map(|p| (p.index, c.display_designator.clone()))).collect();
    if hits.is_empty() {
        let names: Vec<String> = comps[0].parameters.iter().map(|p| p.name.clone()).collect();
        return Err(Error::not_found(format!("{designator} has no parameter {name:?}")).hint(format!("parameters: {}", names.join(", "))));
    }
    if name.eq_ignore_ascii_case("Comment") {
        return Err(unsafe_err("refusing to remove the Comment parameter").hint("use `ace sch set-comment` to change it"));
    }
    let mut ed = Editor::new(doc)?;
    for (idx, des) in hits {
        ed.remove(idx, Some(des), name)?;
    }
    let (bytes, renumbered, changes) = ed.finish()?;
    Ok((bytes, changes, renumbered))
}

pub fn set_comment(doc: &SchDoc, designator: &str, value: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    set_param(doc, designator, "Comment", value)
}

/// Set the current footprint model's name (and the matching data-file entity).
pub fn set_footprint(doc: &SchDoc, designator: &str, footprint: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    if footprint.trim().is_empty() || footprint.contains('|') {
        return Err(Error::usage("footprint names cannot be empty or contain |"));
    }
    let comps = doc.find_components(designator)?;
    let mut ed = Editor::new(doc)?;
    for c in comps {
        let Some(m) = super::prim::current_footprint(&c.models) else {
            return Err(unsafe_err(format!("{} has no footprint model to change", c.display_designator))
                .hint("adding a model creates implementation records; do it in Altium (`ace designer ...`)"));
        };
        let old = m.name.clone();
        ed.set_prop(m.index, "MODELNAME", footprint, Some(c.display_designator.clone()))?;
        let p = ed.props(m.index);
        for i in 0..p.int_or("DATAFILECOUNT", 0) {
            let k = format!("MODELDATAFILEENTITY{i}");
            if p.get(&k).as_deref() == Some(old.as_str()) {
                ed.set_prop(m.index, &k, footprint, Some(c.display_designator.clone()))?;
            }
        }
    }
    let (bytes, _, changes) = ed.finish()?;
    Ok((bytes, changes))
}

pub fn set_designator(doc: &SchDoc, designator: &str, new: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    let new = new.trim();
    if new.is_empty() || new.contains('|') {
        return Err(Error::usage("the new designator is empty or contains |"));
    }
    let comps = doc.find_components(designator)?;
    if let Some(other) =
        doc.components.iter().find(|c| c.designator.eq_ignore_ascii_case(new) && !comps.iter().any(|x| x.index == c.index))
    {
        return Err(unsafe_err(format!("{new} is already used by the component at record {}", other.index))
            .hint("pick a free designator (see `ace sch components`)"));
    }
    let mut ed = Editor::new(doc)?;
    for c in comps {
        let Some(r) = c.designator_record else {
            return Err(unsafe_err(format!("{} has no designator record", c.display_designator)));
        };
        ed.set_prop(r, "TEXT", new, Some(c.display_designator.clone()))?;
    }
    let (bytes, _, changes) = ed.finish()?;
    Ok((bytes, changes))
}

/// Set (or create) a document parameter (title block field such as `Revision`).
pub fn set_sheet_param(doc: &SchDoc, name: &str, value: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    if name.trim().is_empty() || name.contains(['|', '=']) || value.contains('|') {
        return Err(Error::usage("parameter names cannot be empty or contain | or =, values cannot contain |"));
    }
    let mut ed = Editor::new(doc)?;
    match doc.doc_param(name) {
        Some(p) => {
            ed.set_prop(p.index, "TEXT", value, None)?;
        }
        None => {
            let uid = ed.new_unique_id();
            let fields = new_parameter_fields(doc, None, name, value, &uid);
            ed.append(&fields, None, name, value);
        }
    }
    let (bytes, _, changes) = ed.finish()?;
    Ok((bytes, changes))
}

/// Re-parse edited bytes and check the edit: every change reads back, the record count is
/// as expected, every untouched record decodes to the same properties (renumbered owners
/// after a removal allowed for), and every other stream is byte-identical.
pub fn verify(old: &SchDoc, bytes: &[u8], changes: &[Change]) -> Result<SchDoc> {
    let new = super::doc::load_bytes(&old.path, bytes.to_vec(), &Default::default())
        .map_err(|e| unsafe_err(format!("edited file does not re-parse: {}", e.message)))?;
    let created = changes.iter().filter(|c| c.action == "create").count();
    let removed: Vec<usize> = changes.iter().filter(|c| c.action == "remove").map(|c| c.index).collect();
    let old_fh = old.records.iter().filter(|r| r.stream == "FileHeader").count();
    let new_fh = new.records.iter().filter(|r| r.stream == "FileHeader").count();
    if new_fh != old_fh + created - removed.len() {
        return Err(unsafe_err(format!("record count {new_fh} after edit, expected {}", old_fh + created - removed.len())));
    }
    let touched: HashSet<usize> = changes.iter().filter(|c| c.action == "set").map(|c| c.index).collect();
    for r in old.records.iter().filter(|r| r.stream == "FileHeader") {
        if removed.contains(&r.index) {
            continue;
        }
        let shift = removed.iter().filter(|&&g| g < r.index).count();
        let n = &new.records[r.index - shift];
        if touched.contains(&r.index) {
            // Only the edited keys (and their %UTF8% / UNICODE__ copies) may differ.
            let edited: Vec<String> = changes
                .iter()
                .filter(|c| c.index == r.index)
                .flat_map(|c| [c.key.to_ascii_uppercase(), format!("%UTF8%{}", c.key.to_ascii_uppercase()), format!("UNICODE__{}", c.key.to_ascii_uppercase())])
                .collect();
            for (k, v) in r.props.entries() {
                if !edited.contains(&k.to_ascii_uppercase()) && n.props.raw(k) != Some(v) {
                    return Err(unsafe_err(format!("record {} field {k} changed unexpectedly", r.index)));
                }
            }
            continue;
        }
        let mut want = r.props.clone();
        if let Some(o) = r.owner() {
            let s = removed.iter().filter(|&&g| g < o).count();
            if s > 0 {
                want.set("OWNERINDEX", &(o - s).to_string());
            }
        }
        if n.props != want {
            return Err(unsafe_err(format!("record {} changed unexpectedly", r.index)));
        }
    }
    for c in changes {
        if c.action == "remove" {
            continue;
        }
        let Some(r) = new.records.get(c.index) else {
            return Err(unsafe_err(format!("record {} missing after edit", c.index)));
        };
        let key = if c.action == "create" { "TEXT" } else { c.key.as_str() };
        if r.props.get(key) != c.after {
            return Err(unsafe_err(format!("record {} {key} reads back {:?}, expected {:?}", c.index, r.props.get(key), c.after)));
        }
    }
    // Other streams untouched.
    let mut a = Compound::from_bytes(&old.path, old.bytes.clone())?;
    let mut b = Compound::from_bytes(&old.path, bytes.to_vec())?;
    for s in a.streams() {
        if s.path == "FileHeader" {
            continue;
        }
        if a.read(&s.path)? != b.read(&s.path).unwrap_or_default() {
            return Err(unsafe_err(format!("stream {} changed unexpectedly", s.path)));
        }
    }
    if a.streams().len() != b.streams().len() {
        return Err(unsafe_err("the stream list changed unexpectedly"));
    }
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_format::rawprops::{drop_field, encode_ansi_strict};

    #[test]
    fn field_edits_keep_other_bytes() {
        // A GBK value read as windows-1252 must survive untouched next to the edited field.
        let mut d = b"|RECORD=41|%UTF8%Text=\xe4\xbe\x9b|Text=\xb9\xa9\x81|Name=X\0".to_vec();
        set_text_field(&mut d, "Name", "Y");
        assert_eq!(d, b"|RECORD=41|%UTF8%Text=\xe4\xbe\x9b|Text=\xb9\xa9\x81|Name=Y\0".to_vec());
        let mut d = b"|RECORD=41|Text=a|Name=X\0".to_vec();
        set_text_field(&mut d, "Text", "\u{e9}");
        assert_eq!(d, b"|RECORD=41|%UTF8%Text=\xc3\xa9|Text=\xe9|Name=X\0".to_vec());
        put_field(&mut d, "OWNERINDEX", b"7", None);
        assert!(d.ends_with(b"|Name=X|OWNERINDEX=7\0"));
        assert!(drop_field(&mut d, "ownerindex"));
        assert!(d.ends_with(b"|Name=X\0"));
        assert_eq!(encode_ansi_strict("a\u{4e2d}"), b"a?".to_vec());
    }

    #[test]
    fn built_record_parses() {
        let r = build_record(&[("RECORD".into(), "41".into()), ("TEXT".into(), "\u{e9}t\u{e9}".into())]);
        let p = r.props();
        assert_eq!(p.get("TEXT").as_deref(), Some("\u{e9}t\u{e9}"));
        assert_eq!(p.raw("RECORD"), Some("41"));
        assert_eq!(r.data.last(), Some(&0));
    }
}
