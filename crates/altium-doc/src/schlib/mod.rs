//! `.SchLib` schematic symbol libraries.
//!
//! Layout:
//! - `FileHeader`: one property list (`HEADER=Protel for Windows - Schematic Library Editor
//!   Binary File Version 5.0`, `COMPCOUNT`, `LIBREFn`, `COMPDESCRn`, `PARTCOUNTn`, fonts).
//! - one storage per symbol, named after its library reference (`/` → `_`, at most 31
//!   characters; `SectionKeys` maps long names `LIBREFn` → `SECTIONKEYn`). Its `Data` stream
//!   holds the symbol's records without a header: the component (record 1) first, then its
//!   primitives. Pins are *binary* records (see [`decode_binary_pin`]).
//! - optional per-symbol side streams, each a header + compressed records keyed by pin
//!   number: `PinFrac` (three i32: x, y and length fractions), and UTF-16 property lists
//!   (`PinSymbolLineWidth`, `PinPackageLength`, `PinMiscData`, `PinWideText`, `PinTextData`).
//! - `Storage`: embedded images, as in a SchDoc.

use std::io::Read;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_format::DocKind;
use altium_format::cfbx::{Bin, Compound, split_records};
use altium_format::props::Props;

use crate::sch::prim::{self, Model, Parameter, Pin, Primitive};
use crate::sch::records as rt;
use crate::sch::storage::{self, StoredFile};

pub mod edit;

pub const SCHLIB_HEADER: &str = "Protel for Windows - Schematic Library Editor Binary File Version 5.0";

/// A symbol pin plus the per-pin side-stream properties.
#[derive(Debug, Clone, Serialize)]
pub struct LibPin {
    #[serde(flatten)]
    pub pin: Pin,
    /// Properties from `PinSymbolLineWidth`, `PinPackageLength`, `PinMiscData`, ... keyed as
    /// stored (e.g. `PINPACKAGELENGTH`).
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub extra: IndexMap<String, String>,
    /// True when the pin came from a binary record (the usual SchLib encoding).
    pub binary: bool,
}

/// One symbol of the library.
#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    /// Library reference (`LIBREFERENCE`).
    pub name: String,
    /// Storage (folder) name inside the compound file.
    pub storage: String,
    pub description: String,
    /// Number of sub-parts (`PARTCOUNT - 1`).
    pub part_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_mode_count: Option<i64>,
    /// Default designator (`U?`, `R?`...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub designator: Option<String>,
    /// `Comment` parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Current footprint model name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footprint: Option<String>,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    pub pins: Vec<LibPin>,
    pub parameters: Vec<Parameter>,
    pub models: Vec<Model>,
    /// Drawing primitives (lines, rectangles, arcs, polygons, labels, images...).
    pub primitives: Vec<Primitive>,
    /// Streams of this symbol's storage.
    pub streams: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
    /// Every record as properties (binary pins decoded), for search.
    #[serde(skip)]
    pub records: Vec<(usize, i64, Props)>,
}

pub struct SchLib {
    pub path: PathBuf,
    pub header: Props,
    pub symbols: Vec<Symbol>,
    pub storage: Vec<StoredFile>,
    pub warnings: Vec<String>,
}

/// Decode a binary pin record (record 2 in a SchLib `Data` stream) into the property names
/// the text form uses. Layout: i32 record id (2), u8 ?,
/// i16 `OWNERPARTID`, u8 `OWNERPARTDISPLAYMODE`, u8 `SYMBOL_INNEREDGE`, u8 `SYMBOL_OUTEREDGE`,
/// u8 `SYMBOL_INNER`, u8 `SYMBOL_OUTER`, pascal description, u8 ?, u8 `ELECTRICAL`,
/// u8 `PINCONGLOMERATE`, i16 `PINLENGTH`, i16 `LOCATION.X`, i16 `LOCATION.Y`, i32 `COLOR`,
/// pascal `NAME`, pascal `DESIGNATOR`, pascal `SWAPIDGROUP`, pascal `part|&|seq`.
pub fn decode_binary_pin(data: &[u8]) -> Result<Props> {
    let mut b = Bin::new(data);
    let id = b.i32()?;
    if id != 2 {
        return Err(Error::parse(format!("binary record id {id}, expected 2 (pin)")));
    }
    let mut p = Props::new();
    let mut put = |k: &str, v: String| p.push(k.to_owned(), v);
    put("RECORD", "2".into());
    b.u8()?;
    put("OWNERPARTID", b.i16()?.to_string());
    put("OWNERPARTDISPLAYMODE", b.u8()?.to_string());
    put("SYMBOL_INNEREDGE", b.u8()?.to_string());
    put("SYMBOL_OUTEREDGE", b.u8()?.to_string());
    put("SYMBOL_INNER", b.u8()?.to_string());
    put("SYMBOL_OUTER", b.u8()?.to_string());
    put("DESCRIPTION", b.pascal()?);
    b.u8()?;
    put("ELECTRICAL", b.u8()?.to_string());
    put("PINCONGLOMERATE", b.u8()?.to_string());
    put("PINLENGTH", b.i16()?.to_string());
    put("LOCATION.X", b.i16()?.to_string());
    put("LOCATION.Y", b.i16()?.to_string());
    put("COLOR", b.i32()?.to_string());
    put("NAME", b.pascal()?);
    put("DESIGNATOR", b.pascal()?);
    put("SWAPIDGROUP", b.pascal()?);
    if b.remaining() > 0 {
        let seq = b.pascal()?;
        let parts: Vec<&str> = seq.split('|').collect();
        if parts.len() == 3 {
            put("SWAPIDPART", parts[0].into());
            put("SWAPIDSEQUENCE", parts[2].into());
        }
    }
    Ok(p)
}

/// One decoded side-stream entry: the pin number and either fractions or properties.
enum Side {
    Frac(i32, i32, i32),
    Props(Props),
}

fn decode_side(stream: &str, data: &[u8]) -> Option<(usize, Side)> {
    let mut b = Bin::new(data);
    if b.u8().ok()? != 0xD0 {
        return None;
    }
    let id: usize = b.pascal().ok()?.trim().parse().ok()?;
    let n = b.u32().ok()? as usize;
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(b.bytes(n).ok()?).read_to_end(&mut raw).ok()?;
    if stream.eq_ignore_ascii_case("PinFrac") {
        let mut r = Bin::new(&raw);
        return Some((id, Side::Frac(r.i32().ok()?, r.i32().ok()?, r.i32().ok()?)));
    }
    // u32 byte length, then UTF-16LE `|KEY=VALUE` text.
    let mut r = Bin::new(&raw);
    let len = r.u32().ok()? as usize;
    let body = r.bytes(len.min(r.remaining())).ok()?;
    let (pairs, _) = body.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let text = String::from_utf16_lossy(&units);
    let mut p = Props::new();
    for f in text.split('|').filter(|f| !f.is_empty()) {
        if let Some((k, v)) = f.split_once('=') {
            p.push(k.to_owned(), v.trim_end_matches('\0').to_owned());
        }
    }
    Some((id, Side::Props(p)))
}

pub fn load(path: &Path) -> Result<SchLib> {
    load_with(path, false)
}

pub fn load_with(path: &Path, keep_raw: bool) -> Result<SchLib> {
    if DocKind::of(path) != DocKind::SchLib {
        return Err(Error::new(ErrorCode::WrongFileType, format!("{} is not a .SchLib", path.display()))
            .hint("schematic sheets use `ace sch ...`"));
    }
    load_compound(path, Compound::open(path)?, keep_raw)
}

/// Parse a SchLib from bytes (`path` names it in messages); used to verify edits.
pub fn load_bytes(path: &Path, bytes: Vec<u8>, keep_raw: bool) -> Result<SchLib> {
    load_compound(path, Compound::from_bytes(path, bytes)?, keep_raw)
}

fn load_compound(path: &Path, mut cf: Compound, keep_raw: bool) -> Result<SchLib> {
    let fh = cf.read("FileHeader").map_err(|e| Error::parse(format!("{}: no FileHeader ({})", path.display(), e.message)))?;
    let mut warnings = Vec::new();
    let (recs, _) = split_records(&fh);
    let header = recs.first().map(|r| r.props()).unwrap_or_default();
    let htext = header.str("HEADER");
    if !htext.eq_ignore_ascii_case(SCHLIB_HEADER) {
        if htext.to_ascii_lowercase().contains("capture") {
            return Err(Error::new(ErrorCode::WrongFileType, format!("{} is a schematic sheet ({htext})", path.display()))
                .hint("use `ace sch ...`"));
        }
        warnings.push(format!("unexpected FileHeader header {htext:?}; parsing anyway"));
    }
    // Library references and their storage names.
    let mut keys: IndexMap<String, String> = IndexMap::new();
    if let Some(sk) = cf.read_opt("SectionKeys") {
        let (r, _) = split_records(&sk);
        if let Some(p) = r.first().map(|r| r.props()) {
            for i in 0..p.int_or("KEYCOUNT", 0).max(0) {
                keys.insert(p.str(&format!("LIBREF{i}")).to_lowercase(), p.str(&format!("SECTIONKEY{i}")));
            }
        }
    }
    let storages = cf.storages("/");
    let count = header.int_or("COMPCOUNT", 0).max(0);
    let mut order: Vec<(String, String, i64)> = Vec::new(); // (libref, storage, partcount)
    for i in 0..count {
        let libref = header.str(&format!("LIBREF{i}"));
        let storage_name = keys.get(&libref.to_lowercase()).cloned().unwrap_or_else(|| {
            let s: String = libref.replace('/', "_").chars().take(31).collect();
            storages.iter().find(|x| x.eq_ignore_ascii_case(&s)).cloned().unwrap_or(s)
        });
        order.push((libref, storage_name, header.int_or(&format!("PARTCOUNT{i}"), 0)));
    }
    for s in &storages {
        if !order.iter().any(|(_, st, _)| st.eq_ignore_ascii_case(s)) && cf.exists(&format!("{s}/Data")) {
            warnings.push(format!("storage {s:?} holds a symbol not listed in the header"));
            order.push((s.clone(), s.clone(), 0));
        }
    }
    let mut symbols = Vec::new();
    for (libref, st, _) in order {
        let Some(data) = cf.read_opt(&format!("{st}/Data")) else {
            warnings.push(format!("symbol {libref:?}: storage {st:?} has no Data stream"));
            continue;
        };
        let streams: Vec<String> = cf
            .streams()
            .into_iter()
            .filter_map(|s| s.path.strip_prefix(&format!("{st}/")).map(str::to_owned))
            .collect();
        let mut side: Vec<(String, Vec<u8>)> = Vec::new();
        for s in &streams {
            if s != "Data"
                && let Some(b) = cf.read_opt(&format!("{st}/{s}"))
            {
                side.push((s.clone(), b));
            }
        }
        match parse_symbol(&libref, &st, &data, &side, streams, keep_raw) {
            Ok(sym) => symbols.push(sym),
            Err(e) => warnings.push(format!("symbol {libref:?}: {}", e.message)),
        }
    }
    if symbols.is_empty() && count > 0 {
        return Err(Error::parse(format!("{}: none of the {count} symbols could be decoded", path.display())));
    }
    let storage = match cf.read_opt("Storage") {
        Some(b) => storage::parse(&b, &mut warnings),
        None => vec![],
    };
    Ok(SchLib { path: path.to_path_buf(), header, symbols, storage, warnings })
}

fn parse_symbol(
    libref: &str,
    storage_name: &str,
    data: &[u8],
    side: &[(String, Vec<u8>)],
    streams: Vec<String>,
    keep_raw: bool,
) -> Result<Symbol> {
    let mut warnings = Vec::new();
    let (recs, left) = split_records(data);
    if left > 0 {
        warnings.push(format!("Data: {left} trailing bytes"));
    }
    let mut records: Vec<(usize, i64, Props)> = Vec::new();
    for (i, r) in recs.iter().enumerate() {
        if r.is_text() {
            let p = r.props();
            records.push((i, p.int("RECORD").unwrap_or(-1), p));
        } else {
            match decode_binary_pin(&r.data) {
                Ok(p) => records.push((i, 2, p)),
                Err(e) => warnings.push(format!("record {i}: {}", e.message)),
            }
        }
    }
    let Some((_, 1, comp)) = records.first().cloned() else {
        return Err(Error::parse("Data does not start with a component record"));
    };
    // Side streams keyed by pin order.
    let mut fracs: IndexMap<usize, (i32, i32, i32)> = IndexMap::new();
    let mut extras: IndexMap<usize, IndexMap<String, String>> = IndexMap::new();
    for (name, buf) in side {
        let (rs, _) = split_records(buf);
        for r in rs.iter().skip(1) {
            match decode_side(name, &r.data) {
                Some((id, Side::Frac(x, y, l))) => {
                    fracs.insert(id, (x, y, l));
                }
                Some((id, Side::Props(p))) => {
                    let e = extras.entry(id).or_default();
                    for (k, v) in p.entries() {
                        e.insert(k.to_owned(), v.to_owned());
                    }
                }
                None => warnings.push(format!("{name}: undecoded record")),
            }
        }
    }
    let mut sym = Symbol {
        name: comp.get("LIBREFERENCE").filter(|s| !s.is_empty()).unwrap_or_else(|| libref.to_owned()),
        storage: storage_name.to_owned(),
        description: comp.str("COMPONENTDESCRIPTION"),
        part_count: (comp.int_or("PARTCOUNT", 2) - 1).max(1),
        display_mode_count: comp.int("DISPLAYMODECOUNT"),
        designator: None,
        comment: None,
        footprint: None,
        kind: rt::component_kind(comp.int_or("COMPONENTKIND", 0)).to_owned(),
        unique_id: comp.get("UNIQUEID").filter(|s| !s.is_empty()),
        pins: vec![],
        parameters: vec![],
        models: vec![],
        primitives: vec![],
        streams,
        warnings: vec![],
        raw: keep_raw.then(|| comp.clone()),
        records: vec![],
    };
    let rec_type = |i: usize| records.iter().find(|r| r.0 == i).map(|r| r.1).unwrap_or(-1);
    let mut pin_no = 0usize;
    for (i, record, p) in records.iter().skip(1) {
        let (i, record) = (*i, *record);
        match record {
            2 => {
                let mut props = p.clone();
                if let Some(&(x, y, l)) = fracs.get(&pin_no) {
                    props.push("LOCATION.X_FRAC".into(), x.to_string());
                    props.push("LOCATION.Y_FRAC".into(), y.to_string());
                    props.push("PINLENGTH_FRAC".into(), l.to_string());
                }
                let binary = !recs[i].is_text();
                sym.pins.push(LibPin {
                    pin: Pin::from_props(i, &props, keep_raw),
                    extra: extras.get(&pin_no).cloned().unwrap_or_default(),
                    binary,
                });
                pin_no += 1;
            }
            34 => sym.designator = p.get("TEXT"),
            41 => {
                // Parameters owned by a model's parameter list belong to the model.
                let owner = rt::owner_index(p);
                if owner.is_some_and(|o| rec_type(o) == 48) {
                    continue;
                }
                sym.parameters.push(Parameter::from_props(i, p, keep_raw));
            }
            44 | 46 | 47 | 48 => {}
            45 => {
                let mut m = Model::from_props(i, p, keep_raw);
                // Map definers (47) under this model's list (46), parameters under 48.
                for (j, rj, pj) in &records {
                    let Some(o) = rt::owner_index(pj) else { continue };
                    if *rj == 46 && o == i {
                        for (_, rk, pk) in &records {
                            if *rk == 47 && rt::owner_index(pk) == Some(*j) {
                                let n = pk.int_or("DESIMPCOUNT", 0).max(0);
                                m.pin_map.push(prim::PinMap {
                                    pin: pk.str("DESINTF"),
                                    pads: (0..n).map(|k| pk.str(&format!("DESIMP{k}"))).collect(),
                                });
                            }
                        }
                    }
                    if *rj == 48 && o == i {
                        for (_, rk, pk) in &records {
                            if *rk == 41 && rt::owner_index(pk) == Some(*j) {
                                m.parameters.insert(pk.str("NAME"), pk.str("TEXT"));
                            }
                        }
                    }
                }
                sym.models.push(m);
            }
            1 => warnings.push(format!("record {i}: second component record in one symbol")),
            n if rt::type_name(n) == "unknown" => warnings.push(format!("record {i}: unknown record type {n}")),
            n => sym.primitives.push(Primitive::from_props(i, n, p, keep_raw)),
        }
    }
    prim::resolve_params(&mut sym.parameters, &[]);
    sym.comment = sym.parameters.iter().find(|p| p.name.eq_ignore_ascii_case("Comment")).map(|p| p.effective().to_owned());
    sym.footprint = prim::current_footprint(&sym.models).map(|m| m.name.clone());
    sym.warnings = warnings;
    sym.records = records;
    Ok(sym)
}

impl SchLib {
    pub fn symbol(&self, name: &str) -> Result<&Symbol> {
        self.symbols
            .iter()
            .find(|s| s.name == name)
            .or_else(|| self.symbols.iter().find(|s| s.name.eq_ignore_ascii_case(name) || s.storage.eq_ignore_ascii_case(name)))
            .ok_or_else(|| {
                let names: Vec<&str> = self.symbols.iter().map(|s| s.name.as_str()).take(60).collect();
                Error::not_found(format!("no symbol {name:?} in {}", self.path.display()))
                    .hint(format!("symbols: {}", names.join(", ")))
            })
    }

    /// Search every text property of every symbol record.
    pub fn find(&self, needle: &str) -> Vec<crate::sch::Hit> {
        let mut out = Vec::new();
        for s in &self.symbols {
            let recs: Vec<(usize, String, i64, &Props)> = s.records.iter().map(|(i, r, p)| (*i, s.name.clone(), *r, p)).collect();
            out.extend(crate::sch::find_text(&recs, needle, |_| Some(s.name.clone())));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_pin() {
        // CAT24C256 pin 1 from BeagleBoneCape.SchLib.
        let hex = "0200000000010000000000000001043a1400140000000000000002413001310000";
        let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
        let p = decode_binary_pin(&bytes).unwrap();
        assert_eq!(p.raw("NAME"), Some("A0"));
        assert_eq!(p.raw("DESIGNATOR"), Some("1"));
        assert_eq!(p.raw("PINCONGLOMERATE"), Some("58"));
        assert_eq!(p.raw("PINLENGTH"), Some("20"));
        assert_eq!(p.raw("LOCATION.X"), Some("20"));
        assert_eq!(p.raw("ELECTRICAL"), Some("4"));
        let pin = Pin::from_props(1, &p, false);
        assert_eq!(pin.orientation, 180.0);
        assert_eq!(pin.hot_end.x.mil(), 0.0);
    }
}
