//! Unix ↔ Windows path mapping inside a Wine prefix.
//!
//! Files under the prefix's `drive_c` map to `C:\...`; everything else is reachable through
//! Wine's `Z:` drive (the Unix root).

use std::path::{Component, Path, PathBuf};

/// Windows path Altium should use for a Unix path.
pub fn to_windows(prefix: &Path, p: &Path) -> String {
    let abs = absolute(p);
    let drive_c = absolute(&prefix.join("drive_c"));
    if let Ok(rel) = abs.strip_prefix(&drive_c) {
        return format!("C:\\{}", rel.to_string_lossy().replace('/', "\\"));
    }
    format!("Z:{}", abs.to_string_lossy().replace('/', "\\"))
}

/// Unix path for a Windows path Altium reported (`C:\...`, `Z:\...`); `None` for other drives.
pub fn to_unix(prefix: &Path, w: &str) -> Option<PathBuf> {
    let w = w.trim().replace('\\', "/");
    let (drive, rest) = w.split_once(':')?;
    let rest = rest.trim_start_matches('/');
    match drive.to_ascii_uppercase().as_str() {
        "C" => Some(prefix.join("drive_c").join(rest)),
        "Z" => Some(PathBuf::from("/").join(rest)),
        _ => None,
    }
}

/// Make absolute and drop `.`/`..` without touching the file system (the path may not exist).
pub fn absolute(p: &Path) -> PathBuf {
    let base = if p.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    };
    let mut out = PathBuf::new();
    for c in base.join(p).components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let prefix = Path::new("/home/u/bottle");
        assert_eq!(
            to_windows(prefix, Path::new("/home/u/bottle/drive_c/altium-cli-extended/jobs/1")),
            "C:\\altium-cli-extended\\jobs\\1"
        );
        assert_eq!(to_windows(prefix, Path::new("/home/u/designs/A.PrjPcb")), "Z:\\home\\u\\designs\\A.PrjPcb");
        assert_eq!(to_unix(prefix, "Z:\\home\\u\\x.PcbDoc"), Some(PathBuf::from("/home/u/x.PcbDoc")));
        assert_eq!(to_unix(prefix, "c:\\Program Files\\Altium"), Some(PathBuf::from("/home/u/bottle/drive_c/Program Files/Altium")));
        assert_eq!(to_unix(prefix, "D:\\x"), None);
    }
}
