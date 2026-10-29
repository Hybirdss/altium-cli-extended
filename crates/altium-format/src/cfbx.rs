//! OLE compound file access plus the length-prefixed record framing shared by all
//! Altium binary documents.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};

use crate::props::{Props, codepoints};

/// An Altium compound document fully loaded into memory.
pub struct Compound {
    pub path: PathBuf,
    cf: cfb::CompoundFile<Cursor<Vec<u8>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamInfo {
    pub path: String,
    pub size: u64,
}

impl Compound {
    pub fn open(path: &Path) -> Result<Compound> {
        let bytes = std::fs::read(path).map_err(|e| {
            Error::from(e).hint(format!("check the path: {}", path.display()))
        })?;
        Self::from_bytes(path, bytes)
    }

    pub fn from_bytes(path: &Path, bytes: Vec<u8>) -> Result<Compound> {
        if bytes.len() < 8 || bytes[..8] != [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
            return Err(Error::new(
                ErrorCode::WrongFileType,
                format!("{} is not an OLE compound file (binary Altium document)", path.display()),
            )
            .hint("ASCII-format .SchDoc/.PcbDoc files are not supported; save as binary in Altium"));
        }
        let cf = cfb::CompoundFile::open(Cursor::new(bytes))
            .map_err(|e| Error::parse(format!("{}: corrupt compound file: {e}", path.display())))?;
        Ok(Compound { path: path.to_owned(), cf })
    }

    /// Every stream in the file, depth first.
    pub fn streams(&self) -> Vec<StreamInfo> {
        self.cf
            .walk()
            .filter(|e| e.is_stream())
            .map(|e| StreamInfo {
                // `/`-joined on every OS: cfb hands back a std Path, whose separator is `\` on Windows.
                path: e
                    .path()
                    .components()
                    .filter_map(|c| match c {
                        std::path::Component::Normal(s) => Some(s.to_string_lossy()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("/"),
                size: e.len(),
            })
            .collect()
    }

    /// Top-level storage names (e.g. `Pads6`, or one per footprint in a PcbLib).
    pub fn storages(&self, parent: &str) -> Vec<String> {
        let p = format!("/{}", parent.trim_matches('/'));
        match self.cf.read_storage(&p) {
            Ok(it) => it.filter(|e| e.is_storage()).map(|e| e.name().to_owned()).collect(),
            Err(_) => vec![],
        }
    }

    pub fn exists(&self, stream: &str) -> bool {
        self.cf.is_stream(format!("/{}", stream.trim_start_matches('/')))
    }

    /// Read a whole stream. `stream` is a `/`-separated path like `Pads6/Data`.
    pub fn read(&mut self, stream: &str) -> Result<Vec<u8>> {
        let p = format!("/{}", stream.trim_start_matches('/'));
        let mut s = self.cf.open_stream(&p).map_err(|_| {
            Error::not_found(format!("stream {stream} not found in {}", self.path.display()))
        })?;
        let mut buf = Vec::new();
        s.read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// Read a stream if it exists.
    pub fn read_opt(&mut self, stream: &str) -> Option<Vec<u8>> {
        if self.exists(stream) { self.read(stream).ok() } else { None }
    }

    /// Replace a stream's contents (creating it if needed).
    pub fn write(&mut self, stream: &str, data: &[u8]) -> Result<()> {
        let p = format!("/{}", stream.trim_start_matches('/'));
        let mut s = self
            .cf
            .create_stream(&p)
            .map_err(|e| Error::new(ErrorCode::Io, format!("cannot write stream {stream}: {e}")))?;
        s.write_all(data)?;
        Ok(())
    }

    /// Create a storage, and any missing parent storages. Storages that already exist are
    /// left alone; a stream on the path is an error.
    pub fn create_storage(&mut self, storage: &str) -> Result<()> {
        let p = format!("/{}", storage.trim_matches('/'));
        self.cf
            .create_storage_all(&p)
            .map_err(|e| Error::new(ErrorCode::Io, format!("cannot create storage {storage}: {e}")))
    }

    /// Remove a storage and everything inside it (streams and nested storages).
    pub fn remove_storage(&mut self, storage: &str) -> Result<()> {
        let p = format!("/{}", storage.trim_matches('/'));
        self.cf
            .remove_storage_all(&p)
            .map_err(|e| Error::new(ErrorCode::Io, format!("cannot remove storage {storage}: {e}")))
    }

    /// Serialise the whole compound file.
    pub fn into_bytes(self) -> Result<Vec<u8>> {
        let mut cf = self.cf;
        cf.flush()?;
        Ok(cf.into_inner().into_inner())
    }
}

/// One length-prefixed record: a u32 whose low 24 bits are the length and whose top byte
/// flags binary (non-zero) vs text properties.
#[derive(Debug, Clone)]
pub struct Record {
    /// Top byte of the length word. 0 = text properties, 1 = binary (e.g. SchLib pins).
    pub kind: u8,
    pub data: Vec<u8>,
}

impl Record {
    pub fn is_text(&self) -> bool {
        self.kind == 0
    }
    pub fn props(&self) -> Props {
        Props::parse(&self.data)
    }
    pub fn text(props: &Props) -> Record {
        let mut data = props.to_bytes();
        data.push(0);
        Record { kind: 0, data }
    }
}

/// Split a stream into length-prefixed records. Stops (without error) at a truncated tail
/// and reports how many trailing bytes were left over.
pub fn split_records(buf: &[u8]) -> (Vec<Record>, usize) {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= buf.len() {
        let word = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
        let kind = (word >> 24) as u8;
        let len = (word & 0x00FF_FFFF) as usize;
        pos += 4;
        if pos + len > buf.len() {
            return (out, buf.len() - (pos - 4));
        }
        out.push(Record { kind, data: buf[pos..pos + len].to_vec() });
        pos += len;
    }
    (out, buf.len() - pos)
}

pub fn join_records(records: &[Record]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in records {
        let word = ((r.kind as u32) << 24) | (r.data.len() as u32 & 0x00FF_FFFF);
        out.extend(word.to_le_bytes());
        out.extend(&r.data);
    }
    out
}

/// Little-endian cursor over a binary record, with bounds-checked reads.
pub struct Bin<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
}

impl<'a> Bin<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Bin { buf, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::parse(format!(
                "binary record truncated: wanted {n} bytes at offset {}, {} left",
                self.pos,
                self.remaining()
            )));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.take(n)
    }
    /// u8 length + ANSI bytes.
    pub fn pascal(&mut self) -> Result<String> {
        let n = self.u8()? as usize;
        Ok(crate::props::decode_ansi(self.take(n)?))
    }
    /// u32 length + ANSI bytes.
    pub fn pascal32(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        Ok(crate::props::decode_ansi(self.take(n)?))
    }
    /// A sub-record: u32 length, then that many bytes. Returns a cursor over it.
    pub fn sub(&mut self) -> Result<Bin<'a>> {
        let n = self.u32()? as usize;
        Ok(Bin::new(self.take(n)?))
    }
    /// A text property block (u32 length with flags, then `|K=V|`).
    pub fn props(&mut self) -> Result<Props> {
        let word = self.u32()?;
        let n = (word & 0x00FF_FFFF) as usize;
        Ok(Props::parse(self.take(n)?))
    }
}

/// Decode UTF-16LE bytes (an odd trailing byte is ignored).
pub fn utf16le(b: &[u8]) -> String {
    let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    String::from_utf16_lossy(&units)
}

/// The binary `WideStrings6/Data` table of a PcbDoc: repeated (u32 index, u32 byte length,
/// UTF-16LE with NUL). Entries of length <= 2 are empty strings that store no bytes at all.
/// Problems are pushed to `warnings`; whatever decoded before them is returned.
pub fn wide_string_table(buf: &[u8], warnings: &mut Vec<String>) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let mut b = Bin::new(buf);
    while b.remaining() >= 8 {
        let (Ok(idx), Ok(len)) = (b.u32(), b.u32()) else { break };
        let len = len as usize;
        if len <= 2 {
            out.insert(idx, String::new());
            continue;
        }
        let Ok(raw) = b.bytes(len) else {
            warnings.push(format!("WideStrings6: entry {idx} claims {len} bytes, only {} left", b.remaining()));
            return out;
        };
        out.insert(idx, utf16le(&raw[..len - 2]));
    }
    if b.remaining() != 0 {
        warnings.push(format!("WideStrings6: {} trailing bytes", b.remaining()));
    }
    out
}

/// `ENCODEDTEXT<n>=<code points>` entries of one property record — the form PcbLib footprint
/// `<footprint>/WideStrings` streams use.
pub fn encoded_text_table(props: &Props) -> HashMap<u32, String> {
    props
        .entries()
        .filter_map(|(k, v)| {
            let i = k.to_ascii_uppercase().strip_prefix("ENCODEDTEXT")?.parse::<u32>().ok()?;
            Some((i, codepoints(v)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_roundtrip() {
        let recs = vec![
            Record { kind: 0, data: b"|A=1\0".to_vec() },
            Record { kind: 1, data: vec![1, 2, 3] },
        ];
        let buf = join_records(&recs);
        let (back, left) = split_records(&buf);
        assert_eq!(left, 0);
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].kind, 1);
        assert_eq!(back[0].props().raw("A"), Some("1"));
    }

    #[test]
    fn stream_paths_use_forward_slashes_on_every_os() {
        let mut cf = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        cf.create_storage("/Rules6").unwrap();
        cf.create_stream("/Rules6/Data").unwrap().write_all(b"x").unwrap();
        cf.flush().unwrap();
        let c = Compound::from_bytes(Path::new("t.PcbDoc"), cf.into_inner().into_inner()).unwrap();
        let paths: Vec<String> = c.streams().into_iter().map(|s| s.path).collect();
        assert_eq!(paths, vec!["Rules6/Data".to_owned()]);
    }
}
