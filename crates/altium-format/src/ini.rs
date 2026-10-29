//! Order-preserving INI reader/writer for `.PrjPcb`, `.OutJob` and other Altium text files.
//! Preserves entry order, line-ending style and a leading BOM. Writing normalizes spacing
//! between sections and omits comments and unrecognized lines.

use std::path::Path;

use altium_core::error::{Error, Result};
use crate::props::{decode_ansi, encode_ansi};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ini {
    pub sections: Vec<Section>,
    /// A leading UTF-8 BOM is retained when writing.
    pub bom: bool,
    /// `\r\n` if the source used it (Altium always does).
    pub crlf: bool,
    /// Empty lines after the last entry (kept so an untouched file round-trips).
    pub trailing_blank: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Section {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl Section {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
    pub fn str(&self, key: &str) -> String {
        self.get(key).unwrap_or_default().to_owned()
    }
    pub fn int(&self, key: &str) -> Option<i64> {
        self.get(key)?.trim().parse().ok()
    }
    pub fn bool(&self, key: &str) -> bool {
        matches!(self.get(key).map(str::trim), Some("1" | "True" | "TRUE" | "T"))
    }
    pub fn set(&mut self, key: &str, val: &str) {
        if let Some(e) = self.entries.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            e.1 = val.to_owned();
        } else {
            self.entries.push((key.to_owned(), val.to_owned()));
        }
    }
    pub fn remove(&mut self, key: &str) {
        self.entries.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    }
    pub fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        for (k, v) in &self.entries {
            m.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        serde_json::Value::Object(m)
    }
}

impl Ini {
    pub fn load(path: &Path) -> Result<Ini> {
        let bytes = std::fs::read(path)
            .map_err(|e| Error::from(e).hint(format!("check the path: {}", path.display())))?;
        Ok(Ini::parse(&bytes))
    }

    pub fn parse(bytes: &[u8]) -> Ini {
        let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
        let crlf = bytes.windows(2).any(|w| w == b"\r\n");
        let mut ini = Ini { sections: vec![], bom, crlf, trailing_blank: 0 };
        let lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
        // The piece after the final newline is not a line.
        let body = if lines.last().is_some_and(|l| l.is_empty()) { &lines[..lines.len() - 1] } else { &lines[..] };
        ini.trailing_blank = body.iter().rev().take_while(|l| l.strip_suffix(b"\r").unwrap_or(l).iter().all(u8::is_ascii_whitespace)).count();
        // Lines before the first header go in an unnamed section.
        let mut cur = Section::default();
        for line in bytes.split(|&b| b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let text = decode_ansi(line);
            let t = text.trim();
            if t.starts_with('[') && t.ends_with(']') {
                if !cur.name.is_empty() || !cur.entries.is_empty() {
                    ini.sections.push(std::mem::take(&mut cur));
                }
                cur.name = t[1..t.len() - 1].to_owned();
            } else if let Some((k, v)) = text.split_once('=') {
                cur.entries.push((k.to_owned(), v.to_owned()));
            }
        }
        if !cur.name.is_empty() || !cur.entries.is_empty() {
            ini.sections.push(cur);
        }
        ini
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn section_mut(&mut self, name: &str) -> Option<&mut Section> {
        self.sections.iter_mut().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Sections named `<prefix><N>` in numeric order (`Document1`, `Document2`...).
    pub fn numbered(&self, prefix: &str) -> Vec<(u32, &Section)> {
        let mut v: Vec<(u32, &Section)> = self
            .sections
            .iter()
            .filter_map(|s| {
                let rest = s.name.get(prefix.len()..)?;
                if !s.name[..prefix.len()].eq_ignore_ascii_case(prefix) {
                    return None;
                }
                rest.parse::<u32>().ok().map(|n| (n, s))
            })
            .collect();
        v.sort_by_key(|(n, _)| *n);
        v
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let nl: &[u8] = if self.crlf { b"\r\n" } else { b"\n" };
        let mut out = if self.bom { vec![0xEF, 0xBB, 0xBF] } else { Vec::new() };
        for (i, s) in self.sections.iter().enumerate() {
            if !s.name.is_empty() {
                if i > 0 {
                    out.extend(nl);
                }
                out.extend(encode_ansi(&format!("[{}]", s.name)));
                out.extend(nl);
            }
            for (k, v) in &s.entries {
                out.extend(encode_ansi(&format!("{k}={v}")));
                out.extend(nl);
            }
        }
        for _ in 0..self.trailing_blank {
            out.extend(nl);
        }
        out
    }
}

/// Parse `Key=Val|Key=Val` values embedded in INI entries (variant variations, OutJob
/// configuration items, printer options).
pub fn pipe_props(s: &str) -> indexmap::IndexMap<String, String> {
    s.split('|')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_bom_on_edit() {
        let mut ini = Ini::parse(b"\xef\xbb\xbf[Design]\r\nVersion=1.0\r\n");
        ini.section_mut("Design").unwrap().set("Version", "2.0");
        assert_eq!(ini.to_bytes(), b"\xef\xbb\xbf[Design]\r\nVersion=2.0\r\n");
    }

    #[test]
    fn parse_write() {
        let src = b"[Design]\r\nVersion=1.0\r\n\r\n[Document1]\r\nDocumentPath=a.SchDoc\r\n\r\n[Document2]\r\nDocumentPath=b.PcbDoc\r\n";
        let ini = Ini::parse(src);
        assert_eq!(ini.numbered("Document").len(), 2);
        assert_eq!(ini.section("design").unwrap().get("version"), Some("1.0"));
        assert_eq!(ini.to_bytes(), src.to_vec());
    }
}
