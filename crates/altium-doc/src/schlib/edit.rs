//! Safe symbol edits in a `.SchLib`: parameter values (set or add) and the description.
//!
//! Like schematic edits, only the `|KEY=VALUE` fields being changed are rewritten; binary
//! pin records, other records, other symbols and every other stream keep their bytes. A new
//! parameter is appended at the end of the symbol's `Data` stream (owners precede children,
//! so no record is renumbered). [`verify`] re-parses the result before anything is written.

use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_format::cfbx::{Compound, Record, join_records, split_records};
use altium_format::rawprops::set_text_field;

use super::SchLib;

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub symbol: String,
    /// Stream edited (`<storage>/Data` or `FileHeader`).
    pub stream: String,
    pub record: usize,
    /// `set` or `create`.
    pub action: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    pub after: String,
}

fn unsafe_err(m: impl Into<String>) -> Error {
    Error::new(ErrorCode::Unsafe, m)
}

fn text_records(cf: &mut Compound, stream: &str) -> Result<Vec<Record>> {
    let buf = cf.read(stream)?;
    let (recs, left) = split_records(&buf);
    if left > 0 {
        return Err(unsafe_err(format!("{stream} has {left} trailing bytes; refusing to rewrite it")));
    }
    Ok(recs)
}

/// Set (or add) a parameter of one symbol (`Comment` included).
pub fn set_param(lib: &SchLib, bytes: &[u8], symbol: &str, name: &str, value: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    if name.trim().is_empty() || name.contains(['|', '=']) || value.contains('|') {
        return Err(Error::usage("parameter names cannot be empty or contain | or =, values cannot contain |"));
    }
    let sym = lib.symbol(symbol)?;
    let stream = format!("{}/Data", sym.storage);
    let mut cf = Compound::from_bytes(&lib.path, bytes.to_vec())?;
    let mut recs = text_records(&mut cf, &stream)?;
    let mut changes = vec![];
    match sym.parameters.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
        Some(p) => {
            let r = recs.get_mut(p.index).filter(|r| r.is_text()).ok_or_else(|| Error::internal("parameter record is not a text record"))?;
            let before = r.props().get("TEXT");
            if before.as_deref() == Some(value) {
                return Ok((bytes.to_vec(), vec![]));
            }
            set_text_field(&mut r.data, "TEXT", value);
            changes.push(Change { symbol: sym.name.clone(), stream: stream.clone(), record: p.index, action: "set".into(), key: p.name.clone(), before, after: value.into() });
        }
        None => {
            // Copy the visual conventions of a hidden sibling parameter, as Altium places
            // new parameters.
            let tmpl = sym
                .parameters
                .iter()
                .rev()
                .find(|p| p.hidden)
                .or(sym.parameters.last())
                .and_then(|p| recs.get(p.index))
                .map(|r| r.props());
            // Fields go in front of the record's terminating NUL.
            let mut data = vec![0u8];
            let mut put = |k: &str, v: &str| set_text_field(&mut data, k, v);
            put("RECORD", "41");
            if let Some(t) = &tmpl {
                for k in ["OWNERINDEX", "OWNERPARTID", "LOCATION.X", "LOCATION.X_FRAC", "LOCATION.Y", "LOCATION.Y_FRAC", "COLOR", "FONTID"] {
                    if let Some(v) = t.raw(k) {
                        put(k, v);
                    }
                }
            } else {
                put("OWNERPARTID", "-1");
            }
            put("ISHIDDEN", "T");
            put("TEXT", value);
            put("NAME", name);
            recs.push(Record { kind: 0, data });
            changes.push(Change { symbol: sym.name.clone(), stream: stream.clone(), record: recs.len() - 1, action: "create".into(), key: name.into(), before: None, after: value.into() });
        }
    }
    cf.write(&stream, &join_records(&recs))?;
    Ok((cf.into_bytes()?, changes))
}

/// Set a symbol's description (component record and the library header entry).
pub fn set_description(lib: &SchLib, bytes: &[u8], symbol: &str, value: &str) -> Result<(Vec<u8>, Vec<Change>)> {
    if value.contains('|') {
        return Err(Error::usage("the description cannot contain |"));
    }
    let sym = lib.symbol(symbol)?;
    if sym.description == value {
        return Ok((bytes.to_vec(), vec![]));
    }
    let mut cf = Compound::from_bytes(&lib.path, bytes.to_vec())?;
    let stream = format!("{}/Data", sym.storage);
    let mut recs = text_records(&mut cf, &stream)?;
    let r = recs.first_mut().filter(|r| r.is_text()).ok_or_else(|| unsafe_err("the symbol has no component record"))?;
    let before = r.props().get("COMPONENTDESCRIPTION");
    set_text_field(&mut r.data, "COMPONENTDESCRIPTION", value);
    let mut changes = vec![Change { symbol: sym.name.clone(), stream: stream.clone(), record: 0, action: "set".into(), key: "COMPONENTDESCRIPTION".into(), before, after: value.into() }];
    cf.write(&stream, &join_records(&recs))?;
    let mut fh = text_records(&mut cf, "FileHeader")?;
    let h = fh.first_mut().ok_or_else(|| unsafe_err("empty FileHeader"))?;
    let hp = h.props();
    if let Some(i) = (0..hp.int_or("COMPCOUNT", 0)).find(|i| hp.get(&format!("LIBREF{i}")).is_some_and(|l| l.eq_ignore_ascii_case(&sym.name))) {
        let k = format!("COMPDESCR{i}");
        let before = hp.get(&k);
        set_text_field(&mut h.data, &k, value);
        changes.push(Change { symbol: sym.name.clone(), stream: "FileHeader".into(), record: 0, action: "set".into(), key: k, before, after: value.into() });
        cf.write("FileHeader", &join_records(&fh))?;
    }
    Ok((cf.into_bytes()?, changes))
}

/// Re-parse: no new warnings, the change reads back, every stream not edited is identical.
pub fn verify(old: &SchLib, old_bytes: &[u8], new_bytes: &[u8], changes: &[Change]) -> Result<SchLib> {
    let new = super::load_bytes(&old.path, new_bytes.to_vec(), false).map_err(|e| unsafe_err(format!("edited library does not re-parse: {}", e.message)))?;
    let count = |l: &SchLib| l.warnings.len() + l.symbols.iter().map(|s| s.warnings.len()).sum::<usize>();
    if count(&new) > count(old) {
        return Err(unsafe_err("the edit introduced parse warnings"));
    }
    if new.symbols.len() != old.symbols.len() {
        return Err(unsafe_err("the symbol count changed"));
    }
    for c in changes {
        let s = new.symbol(&c.symbol)?;
        let got = if c.key == "COMPONENTDESCRIPTION" || c.key.starts_with("COMPDESCR") {
            Some(s.description.clone())
        } else {
            s.parameters.iter().find(|p| p.name.eq_ignore_ascii_case(&c.key)).map(|p| p.value.clone())
        };
        if got.as_deref() != Some(c.after.as_str()) {
            return Err(unsafe_err(format!("{}: {} reads back {got:?}", c.symbol, c.key)));
        }
    }
    let mut a = Compound::from_bytes(&old.path, old_bytes.to_vec())?;
    let mut b = Compound::from_bytes(&old.path, new_bytes.to_vec())?;
    if a.streams().len() != b.streams().len() {
        return Err(unsafe_err("the stream list changed"));
    }
    for s in a.streams() {
        if changes.iter().any(|c| c.stream == s.path) {
            continue;
        }
        if a.read(&s.path)? != b.read(&s.path).unwrap_or_default() {
            return Err(unsafe_err(format!("stream {} changed unexpectedly", s.path)));
        }
    }
    Ok(new)
}
