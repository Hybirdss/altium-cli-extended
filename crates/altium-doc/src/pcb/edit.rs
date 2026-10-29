//! Safe PCB edits on the text-property streams: design rules (`Rules6`), classes
//! (`Classes6`), component fields (`Components6`) and component parameters
//! (`PrimitiveParameters`).
//!
//! - Only the `|KEY=VALUE` fields being changed are rewritten inside the records being
//!   changed ([`altium_format::rawprops`]); every other record keeps its exact bytes and
//!   every other stream is left untouched.
//! - New records (a class, a parameter) are appended where Altium keeps them and the
//!   storage's `Header` record count is updated.
//! - [`verify`] re-parses the result and checks that every other stream is byte-identical
//!   before the caller writes anything.
//!
//! Geometry (moving, adding primitives) and the designator/comment texts (binary `Texts6`
//! records plus `WideStrings6`) are not edited natively: use `ace designer`.

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_core::units::Coord;
use altium_format::cfbx::Compound;
use altium_format::props::Props;
use altium_format::rawprops::{drop_field, fields, find_field, key_of, put_field, set_text_field};

use super::PcbDoc;

/// What an edit did.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// Storage edited (`Rules6`, `Classes6`, `Components6`, `PrimitiveParameters`).
    pub stream: String,
    /// Record index inside the storage (after the edit for created records).
    pub record: usize,
    /// `set`, `create`, `remove`, `add_member`, `remove_member`.
    pub action: String,
    /// Rule / class name, or component designator.
    pub target: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

fn unsafe_err(m: impl Into<String>) -> Error {
    Error::new(ErrorCode::Unsafe, m)
}

fn check_text(what: &str, v: &str) -> Result<()> {
    if v.contains(['|', '\0']) {
        return Err(Error::usage(format!("{what} cannot contain | or NUL")));
    }
    Ok(())
}

/// One length-prefixed property record (`prefix` bytes, u32 length, `|K=V|...` bytes).
#[derive(Debug, Clone)]
struct Rec {
    prefix: Vec<u8>,
    data: Vec<u8>,
}

impl Rec {
    fn props(&self) -> Props {
        Props::parse(&self.data)
    }
}

fn split(name: &str, buf: &[u8], prefix: usize) -> Result<Vec<Rec>> {
    let mut out = vec![];
    let mut pos = 0;
    while pos < buf.len() {
        if pos + prefix + 4 > buf.len() {
            return Err(unsafe_err(format!("{name}: {} trailing bytes; refusing to rewrite the stream", buf.len() - pos)));
        }
        let pre = buf[pos..pos + prefix].to_vec();
        pos += prefix;
        let word = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
        if word >> 24 != 0 {
            return Err(unsafe_err(format!("{name}: record {} is not a text record; refusing to rewrite the stream", out.len())));
        }
        let len = word as usize;
        pos += 4;
        if pos + len > buf.len() {
            return Err(unsafe_err(format!("{name}: record {} is truncated; refusing to rewrite the stream", out.len())));
        }
        out.push(Rec { prefix: pre, data: buf[pos..pos + len].to_vec() });
        pos += len;
    }
    Ok(out)
}

fn join(recs: &[Rec]) -> Vec<u8> {
    let mut out = vec![];
    for r in recs {
        out.extend(&r.prefix);
        out.extend((r.data.len() as u32).to_le_bytes());
        out.extend(&r.data);
    }
    out
}

struct Stream {
    prefix: usize,
    recs: Vec<Rec>,
    /// Records added (+) or removed (-) that the storage `Header` counts.
    header_delta: i64,
}

/// An edit session over one PcbDoc's bytes.
pub struct Editor<'a> {
    doc: &'a PcbDoc,
    bytes: &'a [u8],
    streams: IndexMap<String, Stream>,
    pub changes: Vec<Change>,
}

impl<'a> Editor<'a> {
    /// `bytes` are the file the document was loaded from.
    pub fn new(doc: &'a PcbDoc, bytes: &'a [u8]) -> Editor<'a> {
        Editor { doc, bytes, streams: IndexMap::new(), changes: vec![] }
    }

    fn stream(&mut self, name: &str, prefix: usize) -> Result<&mut Stream> {
        if !self.streams.contains_key(name) {
            let mut cf = Compound::from_bytes(&self.doc.path, self.bytes.to_vec())?;
            let data_path = format!("{name}/Data");
            if !cf.exists(&data_path) {
                return Err(unsafe_err(format!("the board has no {name} storage; creating one is not supported natively"))
                    .hint("make the change in Altium (`ace designer ...`)"));
            }
            let recs = split(name, &cf.read(&data_path)?, prefix)?;
            self.streams.insert(name.to_owned(), Stream { prefix, recs, header_delta: 0 });
        }
        Ok(self.streams.get_mut(name).unwrap())
    }

    fn set_field(&mut self, stream: &str, prefix: usize, record: usize, target: &str, key: &str, value: &str) -> Result<bool> {
        let s = self.stream(stream, prefix)?;
        let r = s.recs.get_mut(record).ok_or_else(|| Error::internal(format!("{stream} has no record {record}")))?;
        let before = r.props().get(key);
        if before.as_deref() == Some(value) {
            return Ok(false);
        }
        set_text_field(&mut r.data, key, value);
        self.changes.push(Change {
            stream: stream.into(),
            record,
            action: "set".into(),
            target: target.into(),
            key: key.into(),
            before,
            after: Some(value.into()),
        });
        Ok(true)
    }

    /// New file bytes with every edited stream (and header count) written back.
    pub fn finish(self) -> Result<(Vec<u8>, Vec<Change>)> {
        let mut cf = Compound::from_bytes(&self.doc.path, self.bytes.to_vec())?;
        for (name, s) in &self.streams {
            cf.write(&format!("{name}/Data"), &join(&s.recs))?;
            if s.header_delta != 0 {
                let hp = format!("{name}/Header");
                let mut h = cf.read(&hp)?;
                if h.len() < 4 {
                    return Err(unsafe_err(format!("{hp} is shorter than 4 bytes")));
                }
                let n = u32::from_le_bytes(h[..4].try_into().unwrap()) as i64 + s.header_delta;
                h[..4].copy_from_slice(&(n.max(0) as u32).to_le_bytes());
                cf.write(&hp, &h)?;
            }
            let _ = s.prefix;
        }
        Ok((cf.into_bytes()?, self.changes))
    }

    /// Storages this session rewrote.
    pub fn edited(&self) -> Vec<String> {
        self.streams.keys().cloned().collect()
    }
}

/// A fresh Altium-style unique id (eight letters A–Z) not in `used`.
fn new_unique_id(seed_from: &str, used: &HashSet<String>) -> String {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15
        ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    for b in seed_from.bytes() {
        seed = seed.wrapping_mul(0x100_0000_01B3) ^ b as u64;
    }
    loop {
        let mut s = String::new();
        for _ in 0..8 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            s.push((b'A' + (seed % 26) as u8) as char);
        }
        if !used.contains(&s) {
            return s;
        }
    }
}

// ---------------------------------------------------------------------------------- rules

/// Friendly rule field names → Altium keys.
fn rule_key(k: &str) -> String {
    match k.to_ascii_lowercase().as_str() {
        "enabled" => "ENABLED".into(),
        "priority" => "PRIORITY".into(),
        "scope1" | "scope" => "SCOPE1EXPRESSION".into(),
        "scope2" => "SCOPE2EXPRESSION".into(),
        "comment" => "COMMENT".into(),
        "name" => "NAME".into(),
        other => other.to_ascii_uppercase(),
    }
}

/// A length as Altium writes rule values: mils, up to four decimals (lossless).
pub fn mil_string(c: Coord) -> String {
    let s = format!("{:.4}", c.0 as f64 / 10_000.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}mil")
}

fn bool_word(v: &str) -> Option<&'static str> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "t" | "1" | "yes" | "on" => Some("TRUE"),
        "false" | "f" | "0" | "no" | "off" => Some("FALSE"),
        _ => None,
    }
}

/// Normalise a new value against the existing one: lengths need a unit and are written in
/// mil, booleans become TRUE/FALSE, numbers must be numbers.
fn normalise(key: &str, old: &str, new: &str) -> Result<String> {
    let o = old.trim();
    if (o.ends_with("mil") || o.ends_with("mm")) && Coord::parse_len(o).is_some() {
        let n = new.trim();
        if !(n.ends_with("mil") || n.ends_with("mm")) {
            return Err(Error::usage(format!("{key} is a length ({old}); give a unit, e.g. 0.2mm or 8mil")));
        }
        let c = Coord::parse_len(n).ok_or_else(|| Error::usage(format!("bad length {new:?}")))?;
        return Ok(mil_string(c));
    }
    if matches!(o, "TRUE" | "FALSE") {
        return bool_word(new).map(str::to_owned).ok_or_else(|| Error::usage(format!("{key} is a boolean; use true or false")));
    }
    if o.parse::<i64>().is_ok() && new.trim().parse::<i64>().is_err() {
        return Err(Error::usage(format!("{key} is an integer ({old})")));
    }
    if o.parse::<f64>().is_ok() && new.trim().parse::<f64>().is_err() {
        return Err(Error::usage(format!("{key} is a number ({old})")));
    }
    Ok(new.to_owned())
}

/// Set one field of a design rule (`gap`, `minlimit`, `enabled`, `priority`, `scope1`...).
/// Only fields the rule already has can be set. `doc` must be loaded with rules.
pub fn set_rule(doc: &PcbDoc, bytes: &[u8], rule: &str, key: &str, value: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    check_text("rule values", value)?;
    let r = doc
        .rules
        .iter()
        .find(|r| r.name == rule)
        .or_else(|| doc.rules.iter().find(|r| r.name.eq_ignore_ascii_case(rule)))
        .ok_or_else(|| Error::not_found(format!("no rule {rule:?}")).hint("see `ace pcb rules`"))?;
    let k = rule_key(key);
    let mut ed = Editor::new(doc, bytes);
    let props = ed.stream("Rules6", 2)?.recs[r.index].props();
    let Some(old) = props.get(&k) else {
        let keys: Vec<String> = props.entries().map(|(k, _)| k.to_ascii_lowercase()).filter(|k| !k.starts_with("%utf8%")).collect();
        return Err(Error::not_found(format!("rule {} has no field {key:?}", r.name)).hint(format!("fields: {}", keys.join(", "))));
    };
    let v = normalise(&k, &old, value)?;
    if k == "NAME" && doc.rules.iter().any(|x| x.index != r.index && x.name.eq_ignore_ascii_case(&v)) {
        return Err(unsafe_err(format!("another rule is already named {v:?}")));
    }
    if k == "PRIORITY"
        && let Some(o) = doc.rules.iter().find(|x| x.index != r.index && x.kind == r.kind && x.priority.to_string() == v)
    {
        return Err(unsafe_err(format!("rule {:?} already has priority {v} among {} rules", o.name, r.kind))
            .hint("Altium keeps priorities unique per rule kind; change the other rule first"));
    }
    ed.set_field("Rules6", 2, r.index, &r.name, &k, &v)?;
    ed.finish()
}

// -------------------------------------------------------------------------------- classes

fn class_by_name<'d>(doc: &'d PcbDoc, name: &str, kind: Option<&str>) -> Result<&'d super::Class> {
    let hits: Vec<_> = doc
        .classes
        .iter()
        .filter(|c| c.name.eq_ignore_ascii_case(name) && kind.is_none_or(|k| c.kind.eq_ignore_ascii_case(k)))
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(Error::not_found(format!("no class {name:?}")).hint("see `ace pcb classes`")),
        _ => Err(Error::new(ErrorCode::Ambiguous, format!("several classes are named {name:?}")).hint("add --kind net|component|...")),
    }
}

/// Check that each member names an object of the class kind.
fn check_members(doc: &PcbDoc, kind: &str, members: &[String]) -> Result<()> {
    let known: Option<Vec<&str>> = match kind {
        "net" => Some(doc.nets.iter().map(|n| n.name.as_str()).collect()),
        "component" => Some(doc.components.iter().map(|c| c.designator.as_str()).collect()),
        _ => None,
    };
    if let Some(k) = known {
        for m in members {
            if !k.iter().any(|x| x.eq_ignore_ascii_case(m)) {
                return Err(Error::not_found(format!("no {kind} {m:?} on the board"))
                    .hint(if kind == "net" { "see `ace pcb nets`" } else { "see `ace pcb components`" }));
            }
        }
    }
    for m in members {
        check_text("member names", m)?;
    }
    Ok(())
}

/// Rewrite the contiguous `M0..Mn` block of a class record with `members`.
fn write_members(data: &mut Vec<u8>, members: &[String]) -> Result<()> {
    let is_m = |k: &[u8]| k.len() > 1 && (k[0] == b'M' || k[0] == b'm') && k[1..].iter().all(u8::is_ascii_digit);
    let fs = fields(data);
    let ms: Vec<usize> = (0..fs.len()).filter(|&i| is_m(key_of(data, fs[i]))).collect();
    let block: Vec<u8> = members.iter().enumerate().flat_map(|(i, m)| {
        let mut f = format!("|M{i}=").into_bytes();
        f.extend(altium_format::rawprops::encode_ansi_strict(m));
        f
    }).collect();
    match (ms.first(), ms.last()) {
        (Some(&a), Some(&b)) => {
            if b - a + 1 != ms.len() {
                return Err(unsafe_err("class members are not stored contiguously; refusing to rewrite them"));
            }
            let start = fs[a].0 - 1; // the '|' before the first member
            let end = fs[b].2;
            data.splice(start..end, block);
        }
        _ => {
            // No members yet: Altium writes them before SELECTED (else at the end).
            let at = find_field(data, "SELECTED")
                .map(|f| f.0 - 1)
                .unwrap_or(if data.last() == Some(&0) { data.len() - 1 } else { data.len() });
            data.splice(at..at, block);
        }
    }
    Ok(())
}

/// Add and/or remove members of a class (net, component, ... class). `doc` needs classes.
pub fn class_members(doc: &PcbDoc, bytes: &[u8], class: &str, kind: Option<&str>, add: &[String], remove: &[String]) -> Result<(Vec<u8>, Vec<Change>)> {
    let c = class_by_name(doc, class, kind)?;
    if c.superclass {
        return Err(unsafe_err(format!("{} is a built-in class with implicit members", c.name)));
    }
    check_members(doc, &c.kind, add)?;
    let mut members = c.members.clone();
    let mut changes = vec![];
    for m in remove {
        let Some(i) = members.iter().position(|x| x.eq_ignore_ascii_case(m)) else {
            return Err(Error::not_found(format!("{m} is not a member of {}", c.name)).hint(format!("members: {}", c.members.join(", "))));
        };
        let gone = members.remove(i);
        changes.push(("remove_member", gone));
    }
    for m in add {
        if members.iter().any(|x| x.eq_ignore_ascii_case(m)) {
            continue;
        }
        let canonical = match c.kind.as_str() {
            "net" => doc.nets.iter().find(|n| n.name.eq_ignore_ascii_case(m)).map(|n| n.name.clone()),
            "component" => doc.components.iter().find(|x| x.designator.eq_ignore_ascii_case(m)).map(|x| x.designator.clone()),
            _ => None,
        }
        .unwrap_or_else(|| m.clone());
        members.push(canonical.clone());
        changes.push(("add_member", canonical));
    }
    let mut ed = Editor::new(doc, bytes);
    if !changes.is_empty() {
        let s = ed.stream(c.stream, 0)?;
        let r = s.recs.get_mut(c.index).ok_or_else(|| Error::internal("class record out of range"))?;
        write_members(&mut r.data, &members)?;
        for (action, m) in changes {
            ed.changes.push(Change {
                stream: c.stream.into(),
                record: c.index,
                action: action.into(),
                target: c.name.clone(),
                key: "members".into(),
                before: (action == "remove_member").then(|| m.clone()),
                after: (action == "add_member").then_some(m),
            });
        }
    }
    ed.finish()
}

/// Altium class kind code for a kind name.
pub fn class_kind_code(kind: &str) -> Option<i64> {
    Some(match kind.to_ascii_lowercase().as_str() {
        "net" => 0,
        "component" => 1,
        "from_to" => 2,
        "pad" => 3,
        "layer" => 4,
        "diff_pair" => 6,
        "polygon" => 7,
        _ => return None,
    })
}

/// Create a class (`Classes6`) by copying the layout of an existing user class.
pub fn add_class(doc: &PcbDoc, bytes: &[u8], name: &str, kind: &str, members: &[String]) -> Result<(Vec<u8>, Vec<Change>)> {
    check_text("class names", name)?;
    if name.trim().is_empty() {
        return Err(Error::usage("the class name is empty"));
    }
    let code = class_kind_code(kind).ok_or_else(|| Error::usage(format!("unknown class kind {kind:?}")).hint("net, component, pad, layer, from_to, diff_pair, polygon"))?;
    let kind_name = super::records::class_kind(code);
    if doc.classes.iter().any(|c| c.kind == kind_name && c.name.eq_ignore_ascii_case(name)) {
        return Err(unsafe_err(format!("a {kind_name} class named {name:?} already exists"))
            .hint("add members with `ace pcb class-add`"));
    }
    check_members(doc, &kind_name, members)?;
    let in6: Vec<_> = doc.classes.iter().filter(|c| c.stream == "Classes6").collect();
    let template = in6
        .iter()
        .find(|c| c.kind_code == code && !c.superclass)
        .or_else(|| in6.iter().find(|c| !c.superclass))
        .or_else(|| in6.first())
        .ok_or_else(|| unsafe_err("the board has no class to copy the record layout from").hint("create the first class in Altium"))?;
    let used: HashSet<String> = doc.classes.iter().map(|c| c.unique_id.clone()).collect();
    let uid = new_unique_id(name, &used);
    let mut ed = Editor::new(doc, bytes);
    let s = ed.stream("Classes6", 0)?;
    let mut data = s.recs[template.index].data.clone();
    let prefix = s.recs[template.index].prefix.clone();
    write_members(&mut data, &[])?;
    set_text_field(&mut data, "NAME", name);
    put_field(&mut data, "KIND", code.to_string().as_bytes(), None);
    for (k, v) in [("SUPERCLASS", "FALSE"), ("SELECTED", "FALSE"), ("SCHAUTOGENERATEDCLUSTER", "FALSE"), ("AUTOGENERATEDCLASS", "FALSE")] {
        if find_field(&data, k).is_some() {
            put_field(&mut data, k, v.as_bytes(), None);
        }
    }
    drop_field(&mut data, "UNIQUEID");
    put_field(&mut data, "UNIQUEID", uid.as_bytes(), None);
    let canon: Vec<String> = members
        .iter()
        .map(|m| match kind_name.as_str() {
            "net" => doc.nets.iter().find(|n| n.name.eq_ignore_ascii_case(m)).map(|n| n.name.clone()).unwrap_or(m.clone()),
            "component" => doc.components.iter().find(|c| c.designator.eq_ignore_ascii_case(m)).map(|c| c.designator.clone()).unwrap_or(m.clone()),
            _ => m.clone(),
        })
        .collect();
    write_members(&mut data, &canon)?;
    s.recs.push(Rec { prefix, data });
    s.header_delta += 1;
    let record = s.recs.len() - 1;
    ed.changes.push(Change {
        stream: "Classes6".into(),
        record,
        action: "create".into(),
        target: name.into(),
        key: "class".into(),
        before: None,
        after: Some(format!("{kind_name}: {}", canon.join(", "))),
    });
    ed.finish()
}

// ----------------------------------------------------------------------------- components

/// Component fields that can be changed without touching geometry.
pub const COMPONENT_FIELDS: &[(&str, &str)] = &[
    ("description", "SOURCEDESCRIPTION"),
    ("kind", "COMPONENTKIND"),
    ("locked", "LOCKED"),
    ("designator_visible", "NAMEON"),
    ("comment_visible", "COMMENTON"),
    ("height", "HEIGHT"),
    ("footprint_description", "FOOTPRINTDESCRIPTION"),
];

/// Set a non-geometric component field (see [`COMPONENT_FIELDS`]).
pub fn set_component(doc: &PcbDoc, bytes: &[u8], designator: &str, field: &str, value: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    check_text("values", value)?;
    let c = doc.component(designator)?;
    let key = COMPONENT_FIELDS
        .iter()
        .find(|(f, k)| f.eq_ignore_ascii_case(field) || k.eq_ignore_ascii_case(field))
        .map(|(_, k)| *k)
        .ok_or_else(|| {
            let names: Vec<&str> = COMPONENT_FIELDS.iter().map(|f| f.0).collect();
            let code = if ["position", "rotation", "side", "footprint", "designator", "comment"].iter().any(|f| f.eq_ignore_ascii_case(field)) {
                ErrorCode::UnsupportedNative
            } else { ErrorCode::Usage };
            Error::new(code, format!("field {field:?} cannot be set natively"))
                .hint(format!("fields: {}; position, rotation, side, footprint, designator and comment texts need `ace designer`", names.join(", ")))
        })?;
    let v = match key {
        "COMPONENTKIND" => {
            let code = (0..=6)
                .find(|&i| {
                    super::records::component_kind(i).is_some_and(|n| n.eq_ignore_ascii_case(value)) || value.trim() == i.to_string()
                })
                .ok_or_else(|| Error::usage(format!("unknown component kind {value:?}")).hint("Standard, Mechanical, Graphical, Net Tie (In BOM), Net Tie, Standard (No BOM), Jumper"))?;
            code.to_string()
        }
        "LOCKED" | "NAMEON" | "COMMENTON" => bool_word(value).ok_or_else(|| Error::usage(format!("{field} is a boolean; use true or false")))?.to_owned(),
        "HEIGHT" => {
            let n = value.trim();
            if !(n.ends_with("mil") || n.ends_with("mm")) {
                return Err(Error::usage("height is a length; give a unit, e.g. 2.5mm"));
            }
            mil_string(Coord::parse_len(n).ok_or_else(|| Error::usage(format!("bad length {value:?}")))?)
        }
        _ => value.to_owned(),
    };
    let mut ed = Editor::new(doc, bytes);
    if fields(&ed.stream("Components6", 0)?.recs[c.index].data).is_empty() {
        return Err(unsafe_err("the component record has no fields"));
    }
    ed.set_field("Components6", 0, c.index, &c.designator, key, &v)?;
    ed.finish()
}

// ----------------------------------------------------------------------------- parameters

/// Records of `PrimitiveParameters`: `(header index, count)` of the group for `uid`.
fn param_group(recs: &[Rec], uid: &str) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < recs.len() {
        let p = recs[i].props();
        if let Some(id) = p.get("PRIMITIVEID") {
            let n = p.int("COUNT").unwrap_or(0).max(0) as usize;
            if id.eq_ignore_ascii_case(uid) && p.str("VARIANTGUID").is_empty() {
                return Some((i, n));
            }
            i += n + 1;
        } else {
            i += 1;
        }
    }
    None
}

/// Set (or add) a component parameter. `doc` needs the component's `unique_id`.
pub fn set_param(doc: &PcbDoc, bytes: &[u8], designator: &str, name: &str, value: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    check_text("parameter names", name)?;
    check_text("parameter values", value)?;
    if name.trim().is_empty() || name.contains('=') {
        return Err(Error::usage("parameter names cannot be empty or contain ="));
    }
    let c = doc.component(designator)?;
    if c.unique_id.is_empty() {
        return Err(unsafe_err(format!("{} has no UNIQUEID to attach parameters to", c.designator))
            .hint("set the parameter in Altium, then save the board with component UNIQUEID fields"));
    }
    let mut ed = Editor::new(doc, bytes);
    let s = ed.stream("PrimitiveParameters", 0)?;
    match param_group(&s.recs, &c.unique_id) {
        Some((h, n)) => {
            if let Some(i) = (h + 1..=h + n).find(|&i| s.recs[i].props().get("NAME").is_some_and(|x| x.eq_ignore_ascii_case(name))) {
                ed.set_field("PrimitiveParameters", 0, i, &c.designator, "VALUE", value)?;
                // Report the parameter name, not the VALUE key.
                if let Some(ch) = ed.changes.last_mut() {
                    ch.key = name.to_owned();
                }
                return ed.finish();
            }
            let mut data = vec![];
            set_text_field(&mut data, "NAME", name);
            set_text_field(&mut data, "VALUE", value);
            data.push(0);
            let prefix = s.recs[h].prefix.clone();
            s.recs.insert(h + n + 1, Rec { prefix, data });
            put_field(&mut s.recs[h].data, "COUNT", (n + 1).to_string().as_bytes(), None);
            ed.changes.push(Change {
                stream: "PrimitiveParameters".into(),
                record: h + n + 1,
                action: "create".into(),
                target: c.designator.clone(),
                key: name.into(),
                before: None,
                after: Some(value.into()),
            });
        }
        None => {
            let mut head = format!("|PRIMITIVEID={}|VARIANTGUID=|COUNT=1", c.unique_id).into_bytes();
            head.push(0);
            let mut data = vec![];
            set_text_field(&mut data, "NAME", name);
            set_text_field(&mut data, "VALUE", value);
            data.push(0);
            s.recs.push(Rec { prefix: vec![], data: head });
            s.recs.push(Rec { prefix: vec![], data });
            s.header_delta += 1;
            let record = s.recs.len() - 1;
            ed.changes.push(Change {
                stream: "PrimitiveParameters".into(),
                record,
                action: "create".into(),
                target: c.designator.clone(),
                key: name.into(),
                before: None,
                after: Some(value.into()),
            });
        }
    }
    ed.finish()
}

/// Remove a component parameter.
pub fn remove_param(doc: &PcbDoc, bytes: &[u8], designator: &str, name: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    let c = doc.component(designator)?;
    let mut ed = Editor::new(doc, bytes);
    let s = ed.stream("PrimitiveParameters", 0)?;
    let hit = param_group(&s.recs, &c.unique_id).and_then(|(h, n)| {
        (h + 1..=h + n).find(|&i| s.recs[i].props().get("NAME").is_some_and(|x| x.eq_ignore_ascii_case(name))).map(|i| (h, n, i))
    });
    let Some((h, n, i)) = hit else {
        let names: Vec<&String> = c.parameters.keys().collect();
        return Err(Error::not_found(format!("{} has no parameter {name:?}", c.designator)).hint(format!("parameters: {names:?}")));
    };
    let before = s.recs[i].props().get("VALUE");
    s.recs.remove(i);
    if n == 1 {
        s.recs.remove(h);
        s.header_delta -= 1;
    } else {
        put_field(&mut s.recs[h].data, "COUNT", (n - 1).to_string().as_bytes(), None);
    }
    ed.changes.push(Change {
        stream: "PrimitiveParameters".into(),
        record: i,
        action: "remove".into(),
        target: c.designator.clone(),
        key: name.into(),
        before,
        after: None,
    });
    ed.finish()
}

// --------------------------------------------------------------------------------- verify

/// Re-parse edited bytes: the document must load with no new warnings, every stream other
/// than the edited storages must be byte-identical, and the stream list unchanged.
pub fn verify(old: &PcbDoc, old_bytes: &[u8], new_bytes: &[u8], edited: &[String]) -> Result<PcbDoc> {
    let opts = super::LoadOptions::parts(super::part::RULES | super::part::CLASSES);
    let new = super::load_bytes(&old.path, new_bytes.to_vec(), &opts).map_err(|e| unsafe_err(format!("edited file does not re-parse: {}", e.message)))?;
    let before = super::load_bytes(&old.path, old_bytes.to_vec(), &opts)?;
    let fresh: Vec<&String> = new.warnings.iter().filter(|w| !before.warnings.contains(w)).collect();
    if !fresh.is_empty() {
        return Err(unsafe_err(format!("the edit introduced parse warnings: {fresh:?}")));
    }
    let mut a = Compound::from_bytes(&old.path, old_bytes.to_vec())?;
    let mut b = Compound::from_bytes(&old.path, new_bytes.to_vec())?;
    let (sa, sb) = (a.streams(), b.streams());
    if sa.len() != sb.len() {
        return Err(unsafe_err("the stream list changed unexpectedly"));
    }
    for s in sa {
        if edited.iter().any(|e| s.path == format!("{e}/Data") || s.path == format!("{e}/Header")) {
            continue;
        }
        if a.read(&s.path)? != b.read(&s.path).unwrap_or_default() {
            return Err(unsafe_err(format!("stream {} changed unexpectedly", s.path)));
        }
    }
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_roundtrip_and_members() {
        let mut buf = vec![];
        for d in [&b"|NAME=A|KIND=0|M0=GND|M1=VCC|SELECTED=FALSE\0"[..], b"|NAME=B|KIND=0|SELECTED=FALSE\0"] {
            buf.extend((d.len() as u32).to_le_bytes());
            buf.extend(d);
        }
        let mut recs = split("Classes6", &buf, 0).unwrap();
        assert_eq!(join(&recs), buf);
        write_members(&mut recs[0].data, &["VCC".into()]).unwrap();
        assert_eq!(recs[0].data, b"|NAME=A|KIND=0|M0=VCC|SELECTED=FALSE\0".to_vec());
        write_members(&mut recs[1].data, &["X".into(), "Y".into()]).unwrap();
        assert_eq!(recs[1].data, b"|NAME=B|KIND=0|M0=X|M1=Y|SELECTED=FALSE\0".to_vec());
        assert!(split("x", &buf[..buf.len() - 3], 0).is_err());
    }

    #[test]
    fn values() {
        assert_eq!(mil_string(Coord::from_mil(6.0)), "6mil");
        assert_eq!(mil_string(Coord::from_mm(0.2)), "7.874mil");
        assert_eq!(normalise("GAP", "6mil", "0.2mm").unwrap(), "7.874mil");
        assert!(normalise("GAP", "6mil", "8").is_err());
        assert_eq!(normalise("ENABLED", "TRUE", "no").unwrap(), "FALSE");
        assert!(normalise("PRIORITY", "1", "x").is_err());
    }
}
