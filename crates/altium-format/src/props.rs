//! `|KEY=VALUE|KEY=VALUE|` property lists, the text record format used by schematic
//! documents and by many PCB streams (Board6, Nets6, Classes6, Rules6, Components6...).
//!
//! Order and original key spelling are preserved so a parsed list can be written back
//! byte-for-byte (except for edited entries).

use std::cell::Cell;

use encoding_rs::{Encoding, WINDOWS_1252};
use indexmap::IndexMap;
use serde::Serialize;

use altium_core::units::Coord;

/// A parsed property record. Duplicate keys keep only the last value, so serialising a parsed
/// record is not always byte-identical to the input; use [`crate::rawprops`] for file edits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Props {
    /// Keys exactly as written; lookups are case-insensitive through `index`.
    entries: Vec<(String, String)>,
    index: IndexMap<String, usize>,
}

thread_local! {
    static CODEPAGE: Cell<&'static Encoding> = const { Cell::new(WINDOWS_1252) };
    /// Code page that reproduces the ANSI bytes of a `%UTF8%` copy when the current one
    /// does not (see [`codepage_mismatch`]).
    static MISMATCH: Cell<Option<&'static Encoding>> = const { Cell::new(None) };
}

/// ANSI code pages Altium files are commonly saved in, tried when the text does not match.
const CANDIDATES: &[&[u8]] = &[
    b"gbk", b"big5", b"shift_jis", b"euc-kr", b"windows-1251", b"windows-1250", b"windows-1253",
    b"windows-1254", b"windows-1255", b"windows-1256", b"windows-1257", b"windows-1258", b"windows-874",
];

/// A field was stored in a different code page than `--codepage`: returns the one that
/// matches the file's own UTF-8 copies (e.g. `GBK`), once one has been seen.
pub fn codepage_mismatch() -> Option<&'static str> {
    MISMATCH.with(|m| m.get()).map(|e| e.name())
}

/// Compare an ANSI field with its `%UTF8%` copy; remember a code page that fits better.
fn note_pair(ansi: &[u8], utf8: &str) {
    if ansi.is_ascii() || utf8.is_ascii() || ansi.contains(&b'?') || MISMATCH.with(|m| m.get()).is_some() {
        return;
    }
    let (enc, _, bad) = codepage().encode(utf8);
    if !bad && &*enc == ansi {
        return;
    }
    let fit = CANDIDATES.iter().filter_map(|l| Encoding::for_label(l)).find(|e| {
        let (b, _, bad) = e.encode(utf8);
        !bad && &*b == ansi
    });
    if let Some(e) = fit {
        MISMATCH.with(|m| m.set(Some(e)));
    }
}

/// Set the ANSI code page used for non-UTF-8 text (`--codepage`, e.g. `windows-1251`,
/// `euc-kr`, `gbk`, `shift_jis`). Altium writes whatever the author's Windows used.
pub fn set_codepage(label: &str) -> Option<&'static str> {
    let enc = Encoding::for_label(label.as_bytes())?;
    CODEPAGE.with(|c| c.set(enc));
    Some(enc.name())
}

pub fn codepage() -> &'static Encoding {
    CODEPAGE.with(|c| c.get())
}

/// Decode raw bytes. Altium writes the system ANSI code page; UTF-8 copies of
/// non-ASCII values are stored separately under `%UTF8%KEY`.
pub fn decode_ansi(b: &[u8]) -> String {
    if b.is_ascii() {
        return String::from_utf8_lossy(b).into_owned();
    }
    codepage().decode_without_bom_handling(b).0.into_owned()
}

pub fn encode_ansi(s: &str) -> Vec<u8> {
    if s.is_ascii() {
        s.as_bytes().to_vec()
    } else {
        codepage().encode(s).0.into_owned()
    }
}

/// Decode a comma-separated list of Unicode code points (`71,78,68` → `GND`), the form Altium
/// uses for `UNICODE__KEY` values, polygon names and `ENCODEDTEXT` entries.
pub fn codepoints(s: &str) -> String {
    s.split(',').filter_map(|c| c.trim().parse::<u32>().ok()).filter_map(char::from_u32).collect()
}

impl Props {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a property list. Accepts a missing leading `|` (old schematics) and a
    /// trailing NUL.
    pub fn parse(raw: &[u8]) -> Props {
        let raw = match raw.last() {
            Some(0) => &raw[..raw.len() - 1],
            _ => raw,
        };
        let mut p = Props::new();
        let mut ansi: Vec<(&[u8], &[u8])> = vec![];
        for field in raw.split(|&b| b == b'|') {
            if field.is_empty() {
                continue;
            }
            let Some(eq) = field.iter().position(|&b| b == b'=') else { continue };
            if !field[eq + 1..].is_ascii() {
                ansi.push((&field[..eq], &field[eq + 1..]));
            }
            let key = decode_ansi(&field[..eq]);
            let val = if key.starts_with("%UTF8%") {
                String::from_utf8_lossy(&field[eq + 1..]).into_owned()
            } else {
                decode_ansi(&field[eq + 1..])
            };
            p.push(key, val);
        }
        for (k, v) in &ansi {
            if let Some(plain) = k.strip_prefix(b"%UTF8%")
                && let Some((_, a)) = ansi.iter().find(|(x, _)| x.eq_ignore_ascii_case(plain))
            {
                note_pair(a, &String::from_utf8_lossy(v));
            }
        }
        p
    }

    pub fn push(&mut self, key: String, val: String) {
        let k = key.to_ascii_uppercase();
        if let Some(&i) = self.index.get(&k) {
            self.entries[i].1 = val;
        } else {
            self.index.insert(k, self.entries.len());
            self.entries.push((key, val));
        }
    }

    /// Set or insert a value (key match is case-insensitive). Keeps the `%UTF8%` copy in
    /// sync when one exists.
    pub fn set(&mut self, key: &str, val: &str) {
        let utf8_key = format!("%UTF8%{key}");
        if self.raw(&utf8_key).is_some() || !val.is_ascii() {
            self.push(utf8_key, val.to_owned());
        }
        self.push(key.to_owned(), val.to_owned());
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let k = key.to_ascii_uppercase();
        let Some(i) = self.index.shift_remove(&k) else { return false };
        self.entries.remove(i);
        for v in self.index.values_mut() {
            if *v > i {
                *v -= 1;
            }
        }
        true
    }

    /// Exact lookup, no UTF-8/Unicode fallbacks.
    pub fn raw(&self, key: &str) -> Option<&str> {
        self.index
            .get(&key.to_ascii_uppercase())
            .map(|&i| self.entries[i].1.as_str())
    }

    /// String lookup preferring the `%UTF8%` copy, then the `UNICODE__` code-point list.
    pub fn get(&self, key: &str) -> Option<String> {
        if let Some(v) = self.raw(&format!("%UTF8%{key}")) {
            return Some(v.to_owned());
        }
        if self.raw("UNICODE").is_some_and(|u| u.contains("EXISTS"))
            && let Some(list) = self.raw(&format!("UNICODE__{key}")) {
                return Some(codepoints(list));
            }
        self.raw(key).map(str::to_owned)
    }

    pub fn str(&self, key: &str) -> String {
        self.get(key).unwrap_or_default()
    }

    pub fn has(&self, key: &str) -> bool {
        self.raw(key).is_some()
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        let v = self.raw(key)?.trim();
        v.parse::<i64>().ok().or_else(|| v.parse::<f64>().ok().map(|f| f as i64))
    }

    pub fn int_or(&self, key: &str, d: i64) -> i64 {
        self.int(key).unwrap_or(d)
    }

    pub fn f64(&self, key: &str) -> Option<f64> {
        self.raw(key)?.trim().parse().ok()
    }

    pub fn bool(&self, key: &str) -> bool {
        matches!(self.raw(key).map(|v| v.to_ascii_uppercase()).as_deref(), Some("T" | "TRUE"))
    }

    /// PCB length such as `"10mil"`.
    pub fn len(&self, key: &str) -> Option<Coord> {
        Coord::parse_len(self.raw(key)?)
    }

    /// Schematic coordinate `KEY` + `KEY_FRAC`.
    pub fn sch(&self, key: &str) -> Coord {
        Coord::from_sch(self.int_or(key, 0), self.int_or(&format!("{key}_FRAC"), 0))
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn len_entries(&self) -> usize {
        self.entries.len()
    }

    /// Serialise back to `|K=V|K=V` (no trailing NUL; the record writer adds it).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (k, v) in &self.entries {
            out.push(b'|');
            out.extend(encode_ansi(k));
            out.push(b'=');
            if k.starts_with("%UTF8%") {
                out.extend(v.as_bytes());
            } else {
                out.extend(encode_ansi(v));
            }
        }
        out
    }

    /// All entries as an ordered JSON object (for `--raw` output).
    pub fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        for (k, v) in &self.entries {
            m.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        serde_json::Value::Object(m)
    }
}

impl Serialize for Props {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let raw = b"|RECORD=1|LibReference=RES|%UTF8%Text=\xc3\xa9|Text=\xe9\0";
        let p = Props::parse(raw);
        assert_eq!(p.int("record"), Some(1));
        assert_eq!(p.get("TEXT").as_deref(), Some("é"));
        assert_eq!(p.to_bytes(), raw[..raw.len() - 1].to_vec());
    }

    #[test]
    fn missing_leading_bar() {
        let p = Props::parse(b"RECORD=28|TEXT=x");
        assert_eq!(p.int("RECORD"), Some(28));
    }

    #[test]
    fn set_remove() {
        let mut p = Props::parse(b"|A=1|B=2|C=3");
        p.set("b", "20");
        assert!(p.remove("A"));
        assert_eq!(p.to_bytes(), b"|B=20|C=3".to_vec());
        assert_eq!(p.raw("C"), Some("3"));
    }
}
