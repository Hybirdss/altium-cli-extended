//! Safe footprint edits in a `.PcbLib`: parameters, rename, removal, and copying a footprint
//! from another library.
//!
//! A footprint lives in its own compound-file storage (`<storage>/Data` with binary
//! primitives, `Parameters`, `Header`, `WideStrings`, optionally
//! `UniqueIDPrimitiveInformation`), and the library's footprint list is the tail of
//! `Library/Data` (`u32 count`, then `count × (u32 length + pascal name)`).
//!
//! - Only the streams an edit has to touch are rewritten: `Parameters` fields keep every
//!   other byte ([`altium_format::rawprops`]), a renamed `Data` stream only gets a new length
//!   and name in front of its unchanged primitives, and every other storage is untouched.
//! - A footprint's storage name follows its footprint name (`/` → `_`, cut at 31 characters),
//!   the way Altium names them, so a rename moves the storage and the `Data`/`Parameters`
//!   names with it.
//! - 3D models referenced by a copied footprint are copied with it unless the target library
//!   already holds the same model id.
//! - [`verify`] re-parses the result and checks that every stream outside the edit is
//!   byte-identical before the caller writes anything.

use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_core::units::Coord;
use altium_format::cfbx::Compound;
use altium_format::props::{Props, decode_ansi};
use altium_format::rawprops::{encode_ansi_strict, set_text_field};

use super::PcbLib;

/// What an edit did.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// Footprint affected: its name after the edit (its old name for a removal).
    pub target: String,
    /// Stream rewritten, created or removed.
    pub stream: String,
    /// `set`, `create`, `rename`, `remove` or `copy`.
    pub action: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// New file bytes plus what changed, for [`verify`] and the CLI report.
pub struct Edit {
    pub bytes: Vec<u8>,
    pub changes: Vec<Change>,
    pub warnings: Vec<String>,
    /// Streams rewritten or created.
    pub written: Vec<String>,
    /// Top-level storages removed.
    pub removed: Vec<String>,
}

impl Edit {
    fn unchanged(bytes: &[u8]) -> Edit {
        Edit { bytes: bytes.to_vec(), changes: vec![], warnings: vec![], written: vec![], removed: vec![] }
    }
}

fn unsafe_err(m: impl Into<String>) -> Error {
    Error::new(ErrorCode::Unsafe, m).hint("nothing was written; inspect the library in Altium Designer or report this unsupported layout")
}

fn check_text(what: &str, v: &str) -> Result<()> {
    if v.contains(['|', '\0']) {
        return Err(Error::usage(format!("{what} cannot contain | or NUL")).hint("remove the field separator or NUL from the value"));
    }
    Ok(())
}

/// Storage names Altium derives from a footprint name: `/` becomes `_`, at most 31 characters.
pub fn storage_name(name: &str) -> String {
    name.replace('/', "_").chars().take(31).collect()
}

fn check_footprint_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(Error::usage("the footprint name is empty").hint("choose a nonempty footprint name"));
    }
    check_text("footprint names", name)?;
    if name.contains('=') || name.trim() != name {
        return Err(Error::usage(format!("footprint name {name:?} cannot contain = or leading/trailing spaces")).hint("remove = and surrounding spaces"));
    }
    if decode_ansi(&encode_ansi_strict(name)) != name {
        return Err(Error::usage("name has characters the current code page cannot store")
            .hint("choose an ANSI-encodable name or pass --codepage <label> matching the library"));
    }
    if encode_ansi_strict(name).len() > 255 {
        return Err(Error::usage(format!("footprint name {name:?} is longer than 255 bytes")).hint("shorten the name to at most 255 encoded bytes"));
    }
    Ok(())
}

// ------------------------------------------------------------------------- Library/Data

/// The footprint list of `Library/Data`: the property record (unchanged bytes) plus names.
struct List {
    /// Everything before the count, starting with the property record's length word.
    head: Vec<u8>,
    names: Vec<String>,
}

impl List {
    fn parse(buf: &[u8]) -> Result<List> {
        if buf.len() < 8 {
            return Err(unsafe_err("Library/Data is too short to hold a footprint list"));
        }
        let word = u32::from_le_bytes(buf[..4].try_into().unwrap());
        let len = (word & 0x00FF_FFFF) as usize;
        if 4 + len + 4 > buf.len() {
            return Err(unsafe_err("Library/Data: the property record length is out of range"));
        }
        let head = buf[..4 + len].to_vec();
        let mut pos = 4 + len;
        let count = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        let mut names = Vec::with_capacity(count);
        for i in 0..count {
            if pos + 4 > buf.len() {
                return Err(unsafe_err(format!("Library/Data: footprint list truncated after {i} of {count} names")));
            }
            let n = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            if n < 1 || pos + n > buf.len() || buf[pos] as usize + 1 != n {
                return Err(unsafe_err(format!("Library/Data: footprint name {i} is not a length-prefixed string")));
            }
            names.push(decode_ansi(&buf[pos + 1..pos + n]));
            pos += n;
        }
        if pos != buf.len() {
            return Err(unsafe_err(format!("Library/Data: {} bytes after the footprint list", buf.len() - pos)));
        }
        Ok(List { head, names })
    }

    fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = self.head.clone();
        out.extend((self.names.len() as u32).to_le_bytes());
        for n in &self.names {
            check_footprint_name(n)?;
            let b = encode_ansi_strict(n);
            if b.is_empty() || b.len() > 255 {
                return Err(unsafe_err(format!("footprint name {n:?} does not fit the library list")));
            }
            out.extend(((b.len() + 1) as u32).to_le_bytes());
            out.push(b.len() as u8);
            out.extend(&b);
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------------- record helpers

/// Split a single-record stream (`u32 length` + `|K=V` bytes) into its length word and body.
fn split_record(stream: &str, buf: &[u8]) -> Result<(u32, Vec<u8>)> {
    if buf.len() < 4 {
        return Err(unsafe_err(format!("{stream} is shorter than its length word")));
    }
    let word = u32::from_le_bytes(buf[..4].try_into().unwrap());
    let len = (word & 0x00FF_FFFF) as usize;
    if 4 + len != buf.len() {
        return Err(unsafe_err(format!("{stream} holds more than one record; refusing to rewrite it")));
    }
    Ok((word, buf[4..].to_vec()))
}

fn join_record(word: u32, body: &[u8]) -> Result<Vec<u8>> {
    if body.len() > 0x00FF_FFFF {
        return Err(unsafe_err("the edited record is too large for a length word"));
    }
    let mut out = ((word & 0xFF00_0000) | body.len() as u32).to_le_bytes().to_vec();
    out.extend(body);
    Ok(out)
}

/// Replace the name at the front of a footprint `Data` stream (`u32 length` + pascal name).
fn set_data_name(data: &[u8], name: &str) -> Result<Vec<u8>> {
    if data.len() < 5 {
        return Err(unsafe_err("footprint Data stream is too short to hold its name"));
    }
    let n = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
    if n < 1 || 4 + n > data.len() || data[4] as usize + 1 != n {
        return Err(unsafe_err("footprint Data stream does not start with a length-prefixed name; refusing to rewrite it"));
    }
    let b = encode_ansi_strict(name);
    if b.is_empty() || b.len() > 255 {
        return Err(unsafe_err(format!("footprint name {name:?} does not fit the Data stream")));
    }
    let mut out = Vec::with_capacity(data.len() - n + b.len() + 1);
    out.extend(((b.len() + 1) as u32).to_le_bytes());
    out.push(b.len() as u8);
    out.extend(&b);
    out.extend(&data[4 + n..]);
    Ok(out)
}

/// Update `PATTERN` in a `Parameters` stream when it still holds the old footprint name.
fn set_pattern(buf: &[u8], from: &str, to: &str) -> Result<Vec<u8>> {
    let (word, mut body) = split_record("Parameters", buf)?;
    if Props::parse(&body).get("PATTERN").as_deref() != Some(from) {
        return Ok(buf.to_vec());
    }
    set_text_field(&mut body, "PATTERN", to);
    join_record(word, &body)
}

/// Every stream of one footprint storage, as `(path below the storage, bytes)`.
fn read_storage(cf: &mut Compound, storage: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let prefix = format!("{storage}/");
    let mut out = vec![];
    for s in cf.streams() {
        if let Some(rest) = s.path.strip_prefix(&prefix) {
            let data = cf.read(&s.path)?;
            out.push((rest.to_owned(), data));
        }
    }
    if out.is_empty() {
        return Err(unsafe_err(format!("storage {storage:?} has no streams")).hint("the footprint cannot be copied natively; do it in Altium"));
    }
    Ok(out)
}

/// Write a footprint storage, renaming the footprint inside `Data` and `Parameters`.
fn write_storage(
    cf: &mut Compound,
    storage: &str,
    streams: Vec<(String, Vec<u8>)>,
    name: Option<&str>,
    pattern: Option<(&str, &str)>,
    written: &mut Vec<String>,
) -> Result<()> {
    cf.create_storage(storage)?;
    for (rest, mut data) in streams {
        if rest == "Data" && let Some(n) = name {
            data = set_data_name(&data, n)?;
        } else if rest == "Parameters" && let Some((from, to)) = pattern {
            data = set_pattern(&data, from, to)?;
        }
        let dst = format!("{storage}/{rest}");
        if let Some((dir, _)) = dst.rsplit_once('/') {
            cf.create_storage(dir)?;
        }
        cf.write(&dst, &data)?;
        written.push(dst);
    }
    Ok(())
}

/// Model ids a footprint's bodies point at, in order, without repeats.
fn referenced_models(fp: &super::Footprint) -> Vec<String> {
    let mut ids: Vec<String> = vec![];
    for b in &fp.bodies {
        if b.model_embedded && !b.model_id.is_empty() && !ids.contains(&b.model_id) {
            ids.push(b.model_id.clone());
        }
    }
    ids
}

/// `Library/Models/Data` records as raw byte slices, length words included.
fn model_records(buf: &[u8]) -> Result<Vec<&[u8]>> {
    let mut out = vec![];
    let mut pos = 0;
    while pos < buf.len() {
        if pos + 4 > buf.len() {
            return Err(unsafe_err("Library/Models/Data has trailing bytes; refusing to rewrite it"));
        }
        let word = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
        let len = (word & 0x00FF_FFFF) as usize;
        if pos + 4 + len > buf.len() {
            return Err(unsafe_err("Library/Models/Data has a truncated record; refusing to rewrite it"));
        }
        out.push(&buf[pos..pos + 4 + len]);
        pos += 4 + len;
    }
    Ok(out)
}

fn read_model_stream(cf: &mut Compound, path: &str) -> Result<Vec<u8>> {
    if cf.exists(path) {
        cf.read(path).map_err(|e| e.hint("repair the model table in Altium Designer before copying; nothing was written"))
    } else {
        Ok(vec![])
    }
}

/// Copy the model `id` (record and embedded stream) from `src` into the target at `cf`.
fn add_model(cf: &mut Compound, src: &PcbLib, scf: &mut Compound, id: &str, written: &mut Vec<String>) -> Result<()> {
    let sdata = read_model_stream(scf, "Library/Models/Data")?;
    let srecs = model_records(&sdata)?;
    let pos = srecs
        .iter()
        .position(|r| Props::parse(&r[4..]).get("ID").is_some_and(|x| x == id))
        .ok_or_else(|| Error::not_found(format!("model {id} is not in the source library's Library/Models/Data")).hint("embed or repair this model in the source library using Altium Designer"))?;
    let m = src
        .models
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| Error::parse(format!("model {id} is not decoded in the source library")).hint("inspect `ace pcblib models` and repair the model table in Altium Designer"))?;
    let stream = m
        .stream
        .clone()
        .ok_or_else(|| unsafe_err(format!("model {} ({id}) has no embedded data", m.name)).hint("copy the footprint in Altium Designer"))?;
    let data = scf.read(&stream)?;
    let mut tdata = read_model_stream(cf, "Library/Models/Data")?;
    let index = {
        let trecs = model_records(&tdata)?;
        trecs.len()
    };
    tdata.extend(srecs[pos]);
    cf.create_storage("Library/Models")?;
    cf.write("Library/Models/Data", &tdata)?;
    written.push("Library/Models/Data".into());
    let mut h = read_model_stream(cf, "Library/Models/Header")?;
    if h.is_empty() && !cf.exists("Library/Models/Header") {
        h = vec![0; 4];
    }
    if h.len() < 4 {
        return Err(Error::parse("Library/Models/Header is truncated").hint("repair the model table in Altium Designer before copying"));
    }
    h[..4].copy_from_slice(&((index + 1) as u32).to_le_bytes());
    cf.write("Library/Models/Header", &h)?;
    written.push("Library/Models/Header".into());
    let dst = format!("Library/Models/{index}");
    cf.write(&dst, &data)?;
    written.push(dst);
    Ok(())
}

// ----------------------------------------------------------------------------- session

/// An edit session over one library's bytes.
struct Session {
    orig: Vec<u8>,
    cf: Compound,
    list: List,
    list_dirty: bool,
    written: Vec<String>,
    removed: Vec<String>,
    changes: Vec<Change>,
    warnings: Vec<String>,
}

impl Session {
    fn open(lib: &PcbLib, bytes: &[u8]) -> Result<Session> {
        let mut cf = Compound::from_bytes(&lib.path, bytes.to_vec())?;
        let buf = cf.read("Library/Data")?;
        let list = List::parse(&buf)?;
        if list.names.len() != lib.footprints.len() {
            return Err(unsafe_err(format!(
                "Library/Data lists {} footprints but {} were decoded; refusing to rewrite it",
                list.names.len(),
                lib.footprints.len()
            ))
            .hint("run `ace pcblib list` to see which footprint cannot be decoded"));
        }
        Ok(Session { orig: bytes.to_vec(), cf, list, list_dirty: false, written: vec![], removed: vec![], changes: vec![], warnings: vec![] })
    }

    fn finish(mut self) -> Result<Edit> {
        if self.changes.is_empty() {
            return Ok(Edit::unchanged(&self.orig));
        }
        if self.list_dirty {
            self.cf.write("Library/Data", &self.list.to_bytes()?)?;
            self.written.push("Library/Data".into());
        }
        Ok(Edit { bytes: self.cf.into_bytes()?, changes: self.changes, warnings: self.warnings, written: self.written, removed: self.removed })
    }
}

// ------------------------------------------------------------------------------- params

/// Set (or add) a footprint parameter: `DESCRIPTION`, `HEIGHT` (with a unit), or any other
/// `Parameters` key such as `PATTERN` or a user parameter.
pub fn set_param(lib: &PcbLib, bytes: &[u8], footprint: &str, name: &str, value: &str) -> Result<Edit> {
    let fp = lib.footprint(footprint)?;
    let key = name.trim();
    if key.is_empty() || key.contains('=') || key.starts_with('%') {
        return Err(Error::usage("parameter names cannot be empty or contain =").hint("keys as Altium stores them: DESCRIPTION, HEIGHT, PATTERN, ..."));
    }
    check_text("parameter names", key)?;
    check_text("parameter values", value)?;
    let stream = format!("{}/Parameters", fp.storage);
    let mut s = Session::open(lib, bytes)?;
    let Some(buf) = s.cf.read_opt(&stream) else {
        return Err(unsafe_err(format!("{} has no {stream}", fp.name)).hint("add the parameter in Altium Designer, then edit it here"));
    };
    let (word, body) = split_record(&stream, &buf)?;
    let p = Props::parse(&body);
    let (before, after, same) = if key.eq_ignore_ascii_case("HEIGHT") {
        let t = value.trim();
        if !(t.ends_with("mil") || t.ends_with("mm")) {
            return Err(Error::usage(format!("height is a length ({value:?}); give a unit, e.g. 2.5mm or 60mil")).hint("append mm or mil, for example 2.5mm"));
        }
        let c = Coord::parse_len(t).ok_or_else(|| Error::usage(format!("bad length {value:?}")).hint("use a numeric height followed by mm or mil, for example 2.5mm"))?;
        let old = p.len("HEIGHT");
        (old.map(crate::pcb::edit::mil_string), crate::pcb::edit::mil_string(c), old == Some(c))
    } else {
        let current = p.get(key);
        (current.clone(), value.to_owned(), current.as_deref() == Some(value))
    };
    if same {
        return Ok(Edit::unchanged(bytes));
    }
    let stored_key = p.entries().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(k, _)| k.to_owned()).unwrap_or_else(|| key.to_owned());
    let mut body = body;
    set_text_field(&mut body, key, &after);
    s.cf.write(&stream, &join_record(word, &body)?)?;
    s.written.push(stream.clone());
    s.changes.push(Change {
        target: fp.name.clone(),
        stream,
        action: if before.is_none() { "create" } else { "set" }.into(),
        key: stored_key,
        before,
        after: Some(after),
    });
    s.finish()
}

// ------------------------------------------------------------------------------- rename

/// Rename a footprint: the `Library/Data` entry, its storage and the names inside `Data`
/// and `Parameters`.
pub fn rename(lib: &PcbLib, bytes: &[u8], footprint: &str, new_name: &str) -> Result<Edit> {
    let fp = lib.footprint(footprint)?;
    let old_name = fp.name.clone();
    let old_storage = fp.storage.clone();
    let index = fp.index;
    check_footprint_name(new_name)?;
    if old_name == new_name {
        return Ok(Edit::unchanged(bytes));
    }
    if lib.footprints.iter().any(|f| f.index != index && f.name.eq_ignore_ascii_case(new_name)) {
        return Err(unsafe_err(format!("the library already has a footprint {new_name:?}")).hint("remove or rename that footprint first"));
    }
    let mut s = Session::open(lib, bytes)?;
    let new_storage = storage_name(new_name);
    let same_storage = new_storage.eq_ignore_ascii_case(&old_storage);
    if !same_storage {
        if s.cf.storages("").iter().any(|x| x.eq_ignore_ascii_case(&new_storage)) {
            return Err(unsafe_err(format!("storage {new_storage:?} already exists in the file")).hint("choose a name whose first 31 characters differ"));
        }
        let streams = read_storage(&mut s.cf, &old_storage)?;
        write_storage(&mut s.cf, &new_storage, streams, Some(new_name), Some((&old_name, new_name)), &mut s.written)?;
        s.cf.remove_storage(&old_storage)?;
        s.removed.push(old_storage.clone());
    } else {
        let data_path = format!("{old_storage}/Data");
        let data = s.cf.read(&data_path)?;
        let edited = set_data_name(&data, new_name)?;
        if edited != data {
            s.cf.write(&data_path, &edited)?;
            s.written.push(data_path);
        }
        let params_path = format!("{old_storage}/Parameters");
        if let Some(buf) = s.cf.read_opt(&params_path) {
            let edited = set_pattern(&buf, &old_name, new_name)?;
            if edited != buf {
                s.cf.write(&params_path, &edited)?;
                s.written.push(params_path);
            }
        }
    }
    if let Some(slot) = s.list.names.get_mut(index) {
        *slot = new_name.to_owned();
    } else {
        return Err(Error::internal(format!("footprint {old_name} has no Library/Data entry")));
    }
    s.list_dirty = true;
    s.changes.push(Change {
        target: new_name.to_owned(),
        stream: format!("{}/Data", if same_storage { &old_storage } else { &new_storage }),
        action: "rename".into(),
        key: "name".into(),
        before: Some(old_name),
        after: Some(new_name.to_owned()),
    });
    s.finish()
}

// ------------------------------------------------------------------------------- remove

/// Delete a footprint: its storage and its `Library/Data` entry. Models it referenced stay
/// in the library.
pub fn remove(lib: &PcbLib, bytes: &[u8], footprint: &str) -> Result<Edit> {
    let fp = lib.footprint(footprint)?;
    let name = fp.name.clone();
    let storage = fp.storage.clone();
    let index = fp.index;
    let mut s = Session::open(lib, bytes)?;
    s.cf.remove_storage(&storage)?;
    s.removed.push(storage.clone());
    s.list.names.remove(index);
    s.list_dirty = true;
    s.changes.push(Change {
        target: name.clone(),
        stream: storage,
        action: "remove".into(),
        key: "footprint".into(),
        before: Some(name),
        after: None,
    });
    s.finish()
}

// --------------------------------------------------------------------------------- copy

/// Copy a footprint from another `.PcbLib` (or the same one) under its own name or `new_name`.
pub fn copy_from(
    lib: &PcbLib,
    bytes: &[u8],
    source: &PcbLib,
    source_bytes: &[u8],
    footprint: &str,
    new_name: Option<&str>,
) -> Result<Edit> {
    let sfp = source.footprint(footprint)?;
    let src_name = sfp.name.clone();
    let src_storage = sfp.storage.clone();
    let name = new_name.unwrap_or(&src_name).to_owned();
    check_footprint_name(&name)?;
    if lib.footprints.iter().any(|f| f.name.eq_ignore_ascii_case(&name)) {
        return Err(unsafe_err(format!("the library already has a footprint {name:?}"))
            .hint("pass --name to copy it under another name"));
    }
    let models = referenced_models(sfp);
    let mut s = Session::open(lib, bytes)?;
    let linked = sfp.bodies.iter().filter(|b| !b.model_embedded && (!b.model_id.is_empty() || !b.model_name.is_empty())).count();
    if linked > 0 {
        s.warnings.push(format!("{linked} linked model(s) not embedded; they resolve through the target library's model search path"));
    }
    let storage = storage_name(&name);
    if s.cf.storages("").iter().any(|x| x.eq_ignore_ascii_case(&storage)) {
        return Err(unsafe_err(format!("storage {storage:?} already exists in the file")).hint("pass --name whose first 31 characters differ"));
    }
    let mut scf = Compound::from_bytes(&source.path, source_bytes.to_vec())?;
    let streams = read_storage(&mut scf, &src_storage)?;
    write_storage(&mut s.cf, &storage, streams, Some(&name), Some((&src_name, &name)), &mut s.written)?;
    s.changes.push(Change {
        target: name.clone(),
        stream: storage,
        action: "copy".into(),
        key: "footprint".into(),
        before: Some(src_name.clone()),
        after: Some(name.clone()),
    });
    for id in models {
        if lib.models.iter().any(|m| m.id == id) {
            continue;
        }
        add_model(&mut s.cf, source, &mut scf, &id, &mut s.written)?;
        s.changes.push(Change {
            target: name.clone(),
            stream: "Library/Models/Data".into(),
            action: "copy".into(),
            key: "model_id".into(),
            before: None,
            after: Some(id),
        });
    }
    s.list.names.push(name);
    s.list_dirty = true;
    s.finish()
}

// --------------------------------------------------------------------------------- verify

/// Re-parse the edited bytes and check that the edit is the only change: no new warnings,
/// every stream outside it byte-identical, every change readable back.
pub fn verify(old: &PcbLib, old_bytes: &[u8], edit: &Edit) -> Result<PcbLib> {
    let new = super::load_bytes(&old.path, edit.bytes.clone(), false)
        .map_err(|e| unsafe_err(format!("edited library does not re-parse: {}", e.message)))?;
    let count = |l: &PcbLib| l.warnings.len() + l.footprints.iter().map(|f| f.warnings.len()).sum::<usize>();
    if count(&new) > count(old) {
        return Err(unsafe_err("the edit introduced parse warnings").hint("nothing was written; report this library as unsupported"));
    }
    let removed = |p: &str| edit.removed.iter().any(|r| p.strip_prefix(r.as_str()).is_some_and(|rest| rest.starts_with('/')));
    let mut a = Compound::from_bytes(&old.path, old_bytes.to_vec())?;
    let mut b = Compound::from_bytes(&old.path, edit.bytes.clone())?;
    let mut keep_old: Vec<String> = a.streams().into_iter().map(|s| s.path).filter(|p| !edit.written.contains(p) && !removed(p)).collect();
    let mut keep_new: Vec<String> = b.streams().into_iter().map(|s| s.path).filter(|p| !edit.written.contains(p)).collect();
    keep_old.sort();
    keep_new.sort();
    if keep_old != keep_new {
        return Err(unsafe_err(format!("the stream list changed unexpectedly: {keep_old:?} -> {keep_new:?}")));
    }
    for s in &keep_old {
        if a.read(s)? != b.read(s)? {
            return Err(unsafe_err(format!("stream {s} changed unexpectedly")));
        }
    }
    for c in &edit.changes {
        check_change(&new, c)?;
    }
    Ok(new)
}

fn check_change(new: &PcbLib, c: &Change) -> Result<()> {
    match c.action.as_str() {
        "remove" => {
            if new.footprints.iter().any(|f| f.name.eq_ignore_ascii_case(&c.target)) {
                return Err(unsafe_err(format!("{} is still in the library after its removal", c.target)));
            }
        }
        "rename" => {
            let before = c.before.as_deref().unwrap_or_default();
            if before != c.target && new.footprints.iter().any(|f| f.name == before) {
                return Err(unsafe_err(format!("the old name {before:?} is still in the library")));
            }
            if !new.footprints.iter().any(|f| f.name == c.target) {
                return Err(unsafe_err(format!("the new name {:?} did not read back", c.target)));
            }
        }
        "copy" if c.key == "model_id" => {
            let id = c.after.as_deref().unwrap_or_default();
            if !new.models.iter().any(|m| m.id == id) {
                return Err(unsafe_err(format!("model {id} is not in the edited library")));
            }
        }
        "copy" => {
            new.footprint(&c.target)?;
        }
        _ => {
            let f = new.footprint(&c.target)?;
            let after = c.after.as_deref().unwrap_or_default();
            let got = if c.key.eq_ignore_ascii_case("DESCRIPTION") {
                Some(f.description.clone())
            } else if c.key.eq_ignore_ascii_case("HEIGHT") {
                f.height.map(crate::pcb::edit::mil_string)
            } else {
                f.parameters.iter().find(|(k, _)| k.eq_ignore_ascii_case(&c.key)).map(|(_, v)| v.clone())
            };
            if got.as_deref() != Some(after) {
                return Err(unsafe_err(format!("{}: {} reads back {got:?}, expected {after:?}", f.name, c.key)));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_library(corruption: Option<bool>) -> Vec<u8> {
        use std::io::{Cursor, Write};
        let mut cf = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        cf.create_storage_all("Library/Models").unwrap();
        let mut list = 0u32.to_le_bytes().to_vec();
        list.extend(0u32.to_le_bytes());
        cf.create_stream("Library/Data").unwrap().write_all(&list).unwrap();
        let props = b"|ID=one|NAME=one.step|EMBED=TRUE\0";
        let mut record = (props.len() as u32).to_le_bytes().to_vec();
        record.extend(props);
        cf.create_stream("Library/Models/Data").unwrap().write_all(&record).unwrap();
        cf.create_stream("Library/Models/0").unwrap().write_all(b"model").unwrap();
        cf.flush().unwrap();
        let mut bytes = cf.into_inner().into_inner();
        if let Some(invalid_start) = corruption {
            let name: Vec<u8> = "Data\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
            // CFB directory entries are 128 bytes; the Models/Data entry follows Library/Data.
            let offset = bytes.as_chunks::<128>().0.iter().enumerate()
                .filter(|(_, entry)| entry.starts_with(&name)).nth(1).unwrap().0 * 128;
            if invalid_start {
                bytes[offset + 116..offset + 120].copy_from_slice(&0xfffffff0u32.to_le_bytes());
            } else {
                bytes[offset + 120..offset + 128].copy_from_slice(&1000u64.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn corrupt_model_stream_does_not_get_replaced_with_empty_data() {
        let path = std::path::Path::new("test.PcbLib");
        let source_bytes = model_library(None);
        let source = super::super::load_bytes(path, source_bytes.clone(), false).unwrap();
        for invalid_start in [false, true] {
            for corrupt_source in [false, true] {
                let corrupt = model_library(Some(invalid_start));
                let mut target = Compound::from_bytes(path, if corrupt_source { source_bytes.clone() } else { corrupt.clone() }).unwrap();
                let mut scf = Compound::from_bytes(path, if corrupt_source { corrupt } else { source_bytes.clone() }).unwrap();
                let damaged = if corrupt_source { &mut scf } else { &mut target };
                assert!(damaged.exists("Library/Models/Data"));
                assert!(damaged.read("Library/Models/Data").is_err());
                let mut written = vec![];
                let result = add_model(&mut target, &source, &mut scf, "one", &mut written);
                assert!(result.is_err(), "copy unexpectedly replaced an unreadable model table");
                assert!(written.is_empty(), "failed copy modified streams: {written:?}");
            }
        }
    }

    #[test]
    fn storage_names_follow_altium() {
        assert_eq!(storage_name("SOIC-8"), "SOIC-8");
        assert_eq!(storage_name("HC-49/U3H (low profile)"), "HC-49_U3H (low profile)");
        assert_eq!(storage_name(&"x".repeat(40)), "x".repeat(31));
    }

    #[test]
    fn list_roundtrip() {
        let head = b"|KIND=x\0";
        let mut buf = ((head.len() as u32).to_le_bytes()).to_vec();
        buf.extend(head);
        buf.extend(2u32.to_le_bytes());
        for n in ["A", "SOIC-8"] {
            buf.extend(((n.len() + 1) as u32).to_le_bytes());
            buf.push(n.len() as u8);
            buf.extend(n.as_bytes());
        }
        let l = List::parse(&buf).unwrap();
        assert_eq!(l.names, ["A", "SOIC-8"]);
        assert_eq!(l.to_bytes().unwrap(), buf);
        let mut grown = l;
        grown.names.push("B".into());
        assert_eq!(List::parse(&grown.to_bytes().unwrap()).unwrap().names, ["A", "SOIC-8", "B"]);
        assert!(List::parse(&buf[..buf.len() - 1]).is_err());
    }

    #[test]
    fn data_name_and_pattern() {
        let mut data = 7u32.to_le_bytes().to_vec();
        data.push(6);
        data.extend(b"SOIC-8");
        data.extend([1, 2, 3]);
        let renamed = set_data_name(&data, "SOIC-8N").unwrap();
        assert_eq!(&renamed[..4], &8u32.to_le_bytes());
        assert_eq!(&renamed[4..], &[7, b'S', b'O', b'I', b'C', b'-', b'8', b'N', 1, 2, 3]);
        let body = b"|PATTERN=SOIC-8|HEIGHT=1mil\0";
        let mut params = ((body.len() as u32).to_le_bytes()).to_vec();
        params.extend(body);
        let p = set_pattern(&params, "SOIC-8", "SOIC-8N").unwrap();
        assert_eq!(&p[4..], b"|PATTERN=SOIC-8N|HEIGHT=1mil\0");
        assert_eq!(u32::from_le_bytes(p[..4].try_into().unwrap()) as usize, p.len() - 4);
        assert_eq!(set_pattern(&params, "OTHER", "X").unwrap(), params);
        assert!(split_record("Parameters", &params[..params.len() - 1]).is_err());
    }
}
