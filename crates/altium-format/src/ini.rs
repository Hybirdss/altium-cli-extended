//! Order-preserving INI reader/writer for `.PrjPcb`, `.OutJob` and other Altium text files.
//! Every header and entry keeps the bytes it was read from, together with the comment, blank
//! and unrecognized lines just above it. Writing re-encodes only what an edit changed or added,
//! so every other line, its line break and a leading BOM stay byte-identical whatever the code
//! page.

use std::collections::HashMap;
use std::path::Path;

use altium_core::error::{Error, ErrorCode, Result};
use crate::props::{check_mixing, codepage, decode_ansi};

const BOM: &[u8] = b"\xef\xbb\xbf";

#[derive(Debug, Clone, Default)]
pub struct Ini {
    pub sections: Vec<Section>,
    /// A leading UTF-8 BOM is retained when writing.
    pub bom: bool,
    /// `\r\n` if the source used it (Altium always does); added lines use it.
    pub crlf: bool,
    /// Comment and blank lines after the last header or entry.
    tail: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct Section {
    pub name: String,
    pub entries: Vec<Entry>,
    /// The header as read; `None` for a section an edit added (or the unnamed first one).
    src: Option<Src>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub key: String,
    pub value: String,
    /// The line as read; `None` for an entry an edit added.
    src: Option<Src>,
}

/// Where a header or entry was read from.
#[derive(Debug, Clone)]
struct Src {
    /// Offset of the line in the file (after the BOM).
    at: usize,
    /// Comment, blank and unrecognized lines just above it, with their line breaks.
    lead: Vec<u8>,
    line: Vec<u8>,
    /// `\r\n`, `\n`, or empty on a last line without one.
    nl: &'static [u8],
}

impl Entry {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Entry {
        Entry { key: key.into(), value: value.into(), src: None }
    }

    /// The line to write: the bytes read for a key or value that still says the same thing,
    /// the code page's bytes for one that was edited. A renamed key keeps its value's bytes.
    fn line(&self) -> Result<Vec<u8>> {
        let raw = self.src.as_ref().map_or(&[][..], |s| &s.line[..]);
        let (raw_key, raw_value) = match raw.iter().position(|&b| b == b'=') {
            Some(eq) => (&raw[..eq], &raw[eq + 1..]),
            None => (raw, &[][..]),
        };
        let mut out = reuse_or_encode(raw_key, &self.key)?;
        out.push(b'=');
        out.extend(reuse_or_encode(raw_value, &self.value)?);
        Ok(out)
    }
}

impl Section {
    pub fn new(name: impl Into<String>) -> Section {
        Section { name: name.into(), ..Section::default() }
    }
    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key.eq_ignore_ascii_case(key))
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entry(key).map(|e| e.value.as_str())
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
        if let Some(e) = self.entries.iter_mut().find(|e| e.key.eq_ignore_ascii_case(key)) {
            e.value = val.to_owned();
        } else {
            self.entries.push(Entry::new(key, val));
        }
    }
    pub fn remove(&mut self, key: &str) {
        self.entries.retain(|e| !e.key.eq_ignore_ascii_case(key));
    }
    pub fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        for e in &self.entries {
            m.insert(e.key.clone(), serde_json::Value::String(e.value.clone()));
        }
        serde_json::Value::Object(m)
    }
}

/// The section name of a `[Name]` line.
fn header(text: &str) -> Option<&str> {
    let t = text.trim();
    (t.starts_with('[') && t.ends_with(']')).then(|| &t[1..t.len() - 1])
}

/// `raw` if it still reads as `text`, else `text` encoded.
fn reuse_or_encode(raw: &[u8], text: &str) -> Result<Vec<u8>> {
    if decode_ansi(raw) == text {
        return Ok(raw.to_vec());
    }
    encode(text)
}

/// `text` in the code page. U+FFFD is refused too: it stands for bytes that did not decode,
/// and writing it would replace them.
fn encode(text: &str) -> Result<Vec<u8>> {
    if text.is_ascii() {
        return Ok(text.as_bytes().to_vec());
    }
    check_mixing(text)?;
    let (bytes, _, unmappable) = codepage().encode(text);
    if unmappable || text.contains('\u{FFFD}') {
        return Err(Error::new(ErrorCode::Unsafe, format!("{text:?} cannot be stored in {}; nothing written", codepage().name()))
            .hint("pass --codepage with the file's code page (e.g. gbk, windows-1251), or use only characters it can store"));
    }
    Ok(bytes.into_owned())
}

/// Output that remembers whether the last line written lacked its line break.
struct Writer {
    out: Vec<u8>,
    nl: &'static [u8],
    open: bool,
}

impl Writer {
    fn put(&mut self, lead: &[u8], line: &[u8], nl: &'static [u8]) {
        if self.open {
            self.out.extend(self.nl);
        }
        self.out.extend(lead);
        self.out.extend(line);
        self.out.extend(nl);
        self.open = nl.is_empty();
    }
}

impl Ini {
    pub fn load(path: &Path) -> Result<Ini> {
        let bytes = std::fs::read(path)
            .map_err(|e| Error::from(e).hint(format!("check the path: {}", path.display())))?;
        Ok(Ini::parse(&bytes))
    }

    pub fn parse(bytes: &[u8]) -> Ini {
        let bom = bytes.starts_with(BOM);
        let mut rest = bytes.strip_prefix(BOM).unwrap_or(bytes);
        let crlf = rest.windows(2).any(|w| w == b"\r\n");
        let mut ini = Ini { sections: vec![], bom, crlf, tail: vec![] };
        // Lines before the first header go in an unnamed section.
        let mut cur = Section::default();
        let mut lead = Vec::new();
        let len = rest.len();
        while !rest.is_empty() {
            let at = len - rest.len();
            let (line, nl, next): (&[u8], &'static [u8], &[u8]) = match rest.iter().position(|&b| b == b'\n') {
                Some(i) if i > 0 && rest[i - 1] == b'\r' => (&rest[..i - 1], b"\r\n", &rest[i + 1..]),
                Some(i) => (&rest[..i], b"\n", &rest[i + 1..]),
                None => (rest, b"", &[]),
            };
            rest = next;
            let text = decode_ansi(line);
            let src = |lead: &mut Vec<u8>| Some(Src { at, lead: std::mem::take(lead), line: line.to_vec(), nl });
            if let Some(name) = header(&text) {
                if !cur.name.is_empty() || !cur.entries.is_empty() || cur.src.is_some() {
                    ini.sections.push(std::mem::take(&mut cur));
                }
                cur.name = name.to_owned();
                cur.src = src(&mut lead);
            } else if let Some((k, v)) = text.split_once('=') {
                cur.entries.push(Entry { key: k.to_owned(), value: v.to_owned(), src: src(&mut lead) });
            } else {
                lead.extend(line);
                lead.extend(nl);
            }
        }
        if !cur.name.is_empty() || !cur.entries.is_empty() || cur.src.is_some() {
            ini.sections.push(cur);
        }
        ini.tail = lead;
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

    /// The file bytes. Fails (UNSAFE) when an edited or added text cannot be stored in the code
    /// page.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let nl: &'static [u8] = if self.crlf { b"\r\n" } else { b"\n" };
        let mut w = Writer { out: if self.bom { BOM.to_vec() } else { Vec::new() }, nl, open: false };
        for (i, s) in self.sections.iter().enumerate() {
            match &s.src {
                Some(src) if header(&decode_ansi(&src.line)) == Some(s.name.as_str()) => w.put(&src.lead, &src.line, src.nl),
                Some(src) => w.put(&src.lead, &encode(&format!("[{}]", s.name))?, src.nl),
                None if !s.name.is_empty() => {
                    if i > 0 {
                        w.put(b"", b"", nl);
                    }
                    w.put(b"", &encode(&format!("[{}]", s.name))?, nl);
                }
                None => {}
            }
            for e in &s.entries {
                match &e.src {
                    Some(src) => w.put(&src.lead, &e.line()?, src.nl),
                    None => w.put(b"", &e.line()?, nl),
                }
            }
        }
        if !self.tail.is_empty() {
            w.put(&self.tail, b"", b"");
        }
        Ok(w.out)
    }

    /// Section names, keys and values: what a reader of the file sees.
    fn text(&self) -> Vec<(&str, Vec<(&str, &str)>)> {
        self.sections
            .iter()
            .map(|s| (s.name.as_str(), s.entries.iter().map(|e| (e.key.as_str(), e.value.as_str())).collect()))
            .collect()
    }

    /// Bytes that replace `original`, the file this INI was read from before it was edited.
    /// Refused (UNSAFE) unless they read back as this INI and every header and entry the edit
    /// kept, with the lines above it, is byte-identical to `original`.
    pub fn write_over(&self, original: &[u8]) -> Result<Vec<u8>> {
        let bytes = self.to_bytes()?;
        let back = Ini::parse(&bytes);
        if back.text() != self.text() {
            return Err(Error::new(ErrorCode::Unsafe, "the edited file does not read back as edited; nothing written")
                .hint("a value probably contains a line break, or a character that reads back differently; pass --codepage with the file's code page"));
        }
        // `back` lines up with `self`, and `self` with `original` through each item's offset.
        let mut kept: HashMap<usize, &Src> = HashMap::new();
        for (s, b) in self.sections.iter().zip(&back.sections) {
            if let (Some(src), Some(new)) = (&s.src, &b.src)
                && header(&decode_ansi(&src.line)) == Some(s.name.as_str())
            {
                kept.insert(src.at, new);
            }
            for (e, f) in s.entries.iter().zip(&b.entries) {
                if let (Some(src), Some(new)) = (&e.src, &f.src)
                    && decode_ansi(&src.line) == format!("{}={}", e.key, e.value)
                {
                    kept.insert(src.at, new);
                }
            }
        }
        let before = Ini::parse(original);
        for s in &before.sections {
            let items = s.src.iter().map(|src| ("the header", src)).chain(s.entries.iter().filter_map(|e| Some((e.key.as_str(), e.src.as_ref()?))));
            for (what, src) in items {
                if let Some(new) = kept.remove(&src.at)
                    && (new.lead != src.lead || new.line != src.line)
                {
                    return Err(Error::new(ErrorCode::Unsafe, format!("the bytes of {what} in [{}] would change, though the edit did not touch it; nothing written", s.name))
                        .hint("the file may have changed after it was read; re-run the command"));
                }
            }
        }
        if !kept.is_empty() || back.tail != before.tail {
            return Err(Error::new(ErrorCode::Unsafe, "the file changed after it was read; nothing written").hint("re-run the command"));
        }
        Ok(bytes)
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
        assert_eq!(ini.to_bytes().unwrap(), b"\xef\xbb\xbf[Design]\r\nVersion=2.0\r\n");
    }

    #[test]
    fn parse_write() {
        let src = b"[Design]\r\nVersion=1.0\r\n\r\n[Document1]\r\nDocumentPath=a.SchDoc\r\n\r\n[Document2]\r\nDocumentPath=b.PcbDoc\r\n";
        let ini = Ini::parse(src);
        assert_eq!(ini.numbered("Document").len(), 2);
        assert_eq!(ini.section("design").unwrap().get("version"), Some("1.0"));
        assert_eq!(ini.to_bytes().unwrap(), src.to_vec());
    }

    #[test]
    fn write_over_refuses_a_file_that_changed_after_it_was_read() {
        let read = b"[Design]\r\nVersion=1.0\r\nTitle=A\r\n";
        let mut ini = Ini::parse(read);
        ini.section_mut("Design").unwrap().set("Version", "2.0");
        assert_eq!(ini.write_over(read).unwrap(), b"[Design]\r\nVersion=2.0\r\nTitle=A\r\n");
        let error = ini.write_over(b"[Design]\r\nVersion=1.0\r\nTitle=B\r\n").unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsafe);
    }
}
