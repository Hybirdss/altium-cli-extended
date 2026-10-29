//! The `Storage` stream: embedded files (images) of a schematic or library.
//!
//! Layout: a text header
//! `|HEADER=Icon storage|WEIGHT=n`, then `n` binary records, each `0xD0`, a u8-length file
//! name, a u32 byte count and that many bytes of zlib-compressed file data.

use std::io::Read;

use serde::Serialize;

use altium_format::cfbx::{Bin, split_records};

/// One embedded file.
#[derive(Debug, Clone, Serialize)]
pub struct StoredFile {
    /// 0-based position in the Storage stream.
    pub index: usize,
    /// File name as stored (often a full Windows path).
    pub name: String,
    pub compressed_size: usize,
    /// Decompressed size, when the data inflates cleanly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<usize>,
    /// `png`, `jpeg`, `bmp`, `gif`, `tiff`, `emf`, `wmf` or `unknown` (from the content).
    pub format: String,
    #[serde(skip)]
    pub data: Vec<u8>,
}

impl StoredFile {
    /// Base file name without the Windows directory.
    pub fn base_name(&self) -> String {
        self.name.rsplit(['\\', '/']).next().unwrap_or(&self.name).to_owned()
    }
}

/// Sniff the image format from its first bytes.
pub fn sniff(b: &[u8]) -> &'static str {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "jpeg"
    } else if b.starts_with(b"BM") {
        "bmp"
    } else if b.starts_with(b"GIF8") {
        "gif"
    } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        "tiff"
    } else if b.len() > 44 && b[0..4] == [1, 0, 0, 0] && &b[40..44] == b" EMF" {
        "emf"
    } else if b.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) {
        "wmf"
    } else {
        "unknown"
    }
}

pub fn parse(buf: &[u8], warnings: &mut Vec<String>) -> Vec<StoredFile> {
    let (recs, left) = split_records(buf);
    if left > 0 {
        warnings.push(format!("Storage: {left} trailing bytes do not form a complete record"));
    }
    let mut out = Vec::new();
    for r in recs.iter().skip(1) {
        let mut b = Bin::new(&r.data);
        let parsed = (|| -> altium_core::Result<(String, Vec<u8>)> {
            let tag = b.u8()?;
            if tag != 0xD0 {
                return Err(altium_core::Error::parse(format!("storage record tag {tag:#x}, expected 0xd0")));
            }
            let name = b.pascal()?;
            let n = b.u32()? as usize;
            Ok((name, b.bytes(n)?.to_vec()))
        })();
        match parsed {
            Ok((name, compressed)) => {
                let mut data = Vec::new();
                let ok = flate2::read::ZlibDecoder::new(compressed.as_slice()).read_to_end(&mut data).is_ok();
                if !ok {
                    warnings.push(format!("Storage: {name:?} does not inflate as zlib; kept compressed"));
                    data = compressed.clone();
                }
                out.push(StoredFile {
                    index: out.len(),
                    format: if ok { sniff(&data).into() } else { "unknown".into() },
                    size: ok.then_some(data.len()),
                    compressed_size: compressed.len(),
                    name,
                    data,
                });
            }
            Err(e) => warnings.push(format!("Storage record {}: {}", out.len(), e.message)),
        }
    }
    out
}

/// Find a stored file by the image record's `FILENAME`: exact match, else the stored name
/// ending with it (images keep the full path they were placed from), else matching base names.
pub fn find<'a>(files: &'a [StoredFile], name: &str) -> Option<&'a StoredFile> {
    files
        .iter()
        .find(|f| f.name == name)
        .or_else(|| files.iter().find(|f| f.name.ends_with(name)))
        .or_else(|| {
            let base = name.rsplit(['\\', '/']).next().unwrap_or(name);
            files.iter().find(|f| f.base_name().eq_ignore_ascii_case(base))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_format::cfbx::{Record, join_records};
    use std::io::Write;

    #[test]
    fn roundtrip_storage() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(png).unwrap();
        let comp = z.finish().unwrap();
        let name = b"C:\\img\\logo.png";
        let mut data = vec![0xD0, name.len() as u8];
        data.extend(name);
        data.extend((comp.len() as u32).to_le_bytes());
        data.extend(&comp);
        let buf = join_records(&[Record { kind: 0, data: b"|HEADER=Icon storage|WEIGHT=1\0".to_vec() }, Record { kind: 1, data }]);
        let mut w = vec![];
        let files = parse(&buf, &mut w);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].format, "png");
        assert_eq!(files[0].base_name(), "logo.png");
        assert!(find(&files, "logo.png").is_some());
    }
}
