//! Safe file replacement for every write command.

use std::path::{Path, PathBuf};

use altium_core::{Error, ErrorCode, Result};
use std::io::Write;

#[derive(Debug, Clone, Copy, Default)]
pub struct WriteOpts {
    /// Do nothing; the caller reports what would change.
    pub dry_run: bool,
    /// Skip the `<file>.bak` copy.
    pub no_backup: bool,
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

struct TempFile {
    path: PathBuf,
    renamed: bool,
}
impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.renamed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Save recovery bytes without overwriting an existing file or symlink.
pub fn preserve(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e| {
            let error = if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::new(ErrorCode::Unsafe, format!("recovery copy already exists: {}", path.display()))
            } else { Error::from(e) };
            error.hint("archive the existing .pre-restore copy or check directory permissions; the design was not changed")
        })?;
    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(Error::from(e).hint("check free space and permissions; the design was not changed"));
    }
    Ok(())
}

/// Replace `path` atomically, keeping the first `<file>.bak` unless disabled.
/// Returns the backup path, including an existing backup kept from an earlier write.
pub fn replace(path: &Path, bytes: &[u8], opts: WriteOpts) -> Result<Option<PathBuf>> {
    if opts.dry_run {
        return Ok(None);
    }
    let mut backup = None;
    if path.exists() && !opts.no_backup {
        let b = suffixed(path, ".bak");
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&b) {
            Ok(mut dest) => {
                let copied = (|| -> std::io::Result<()> {
                    let mut source = std::fs::File::open(path)?;
                    std::io::copy(&mut source, &mut dest)?;
                    dest.sync_all()
                })();
                if let Err(e) = copied {
                    drop(dest);
                    let _ = std::fs::remove_file(&b);
                    return Err(Error::from(e).hint("check free space and permissions; the design was not changed"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(Error::from(e).hint("check backup directory permissions; the design was not changed")),
        }
        backup = Some(b);
    }
    let tmp = suffixed(path, ".altium-cli-extended.tmp");
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)
        .map_err(|e| Error::from(e).hint("check permissions and remove a stale .altium-cli-extended.tmp file after checking no writer is active"))?;
    let mut temp = TempFile { path: tmp, renamed: false };
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp.path, path).map_err(|e| {
        Error::new(ErrorCode::Unsafe, format!("cannot replace {}: {e}", path.display()))
            .hint("close the document in Altium Designer and retry; check directory permissions; the original and any backup were kept")
    })?;
    temp.renamed = true;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ace-fsx-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir(&p).unwrap();
        p
    }

    #[test]
    fn keep_first_backup_and_honour_write_flags() {
        let dir = dir("backup");
        let p = dir.join("board.PcbDoc");
        std::fs::write(&p, b"original").unwrap();
        let b = replace(&p, b"first", WriteOpts::default()).unwrap().unwrap();
        assert_eq!(replace(&p, b"second", WriteOpts::default()).unwrap(), Some(b.clone()));
        assert_eq!(std::fs::read(&b).unwrap(), b"original");
        replace(&p, b"dry", WriteOpts { dry_run: true, no_backup: false }).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"second");
        let fresh = dir.join("fresh");
        replace(&fresh, b"one", WriteOpts { no_backup: true, dry_run: false }).unwrap();
        replace(&fresh, b"two", WriteOpts { no_backup: true, dry_run: false }).unwrap();
        assert!(!dir.join("fresh.bak").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_replace_cleans_temp_and_explains_lock() {
        let dir = dir("failure");
        let p = dir.join("destination");
        std::fs::create_dir(&p).unwrap();
        let e = replace(&p, b"new", WriteOpts { no_backup: true, dry_run: false }).unwrap_err();
        assert_eq!(e.code, altium_core::ErrorCode::Unsafe);
        assert!(e.hint.unwrap().contains("Altium Designer"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_temp_is_never_truncated() {
        let dir = dir("stale");
        let p = dir.join("board.PcbDoc");
        let tmp = dir.join("board.PcbDoc.altium-cli-extended.tmp");
        std::fs::write(&p, b"original").unwrap();
        std::fs::write(&tmp, b"another writer").unwrap();
        assert!(replace(&p, b"new", WriteOpts::default()).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), b"original");
        assert_eq!(std::fs::read(&tmp).unwrap(), b"another writer");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn backup_preserves_non_utf8_filename() {
        use std::os::unix::ffi::OsStringExt;
        let dir = dir("os");
        let p = dir.join(std::ffi::OsString::from_vec(b"board\xff.PcbDoc".to_vec()));
        std::fs::write(&p, b"old").unwrap();
        let b = replace(&p, b"new", WriteOpts::default()).unwrap().unwrap();
        let mut expected = p.as_os_str().to_os_string();
        expected.push(".bak");
        assert_eq!(b.as_os_str(), expected);
        assert_eq!(std::fs::read(b).unwrap(), b"old");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
