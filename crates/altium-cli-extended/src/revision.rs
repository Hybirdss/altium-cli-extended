//! A private, temporary checkout for `diff --rev`.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use altium_core::{Error, ErrorCode, Result};
use altium_doc::project::Project;
use altium_format::DocKind;

pub struct CheckoutDir(PathBuf);
impl Drop for CheckoutDir {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}
impl CheckoutDir {
    fn create() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        loop {
            let path = std::env::temp_dir().join(format!("ace-diff-{}-{stamp}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(Error::from(e).hint("check the temporary directory permissions and free space")),
            }
        }
    }
}

// Normalize Windows separators even on Unix. Keep leading parents so shared project
// documents can be checked out beneath a sufficiently nested scratch project directory.
fn relative(path: &str) -> Result<Vec<String>> {
    let path = path.replace('\\', "/");
    if path.starts_with('/') || path.contains([':', '\0']) {
        return Err(Error::new(ErrorCode::Unsafe, format!("project document path {path:?} is not relative"))
            .hint("use relative document paths inside the repository before comparing a revision"));
    }
    let mut parts: Vec<String> = vec![];
    for part in path.split('/') {
        match part {
            "" | "." => {},
            ".." if parts.last().is_some_and(|p| p != "..") => { parts.pop(); },
            p if p != ".." && p.ends_with(['.', ' ']) => return Err(Error::new(ErrorCode::Unsafe, "project document path has a Windows-ambiguous component")
                .hint("remove trailing dots and spaces from DocumentPath components")),
            p => parts.push(p.to_owned()),
        }
    }
    if parts.is_empty() {
        return Err(Error::usage("project document path is empty").hint("repair DocumentPath in the project"));
    }
    Ok(parts)
}

fn within(root: &Path, base: &Path, parts: &[String]) -> Result<PathBuf> {
    let mut path = base.to_path_buf();
    for part in parts {
        if part == ".." { path.pop(); } else { path.push(part); }
        if !path.starts_with(root) {
            return Err(Error::new(ErrorCode::Unsafe, "project document path escapes the temporary checkout")
                .hint("repair DocumentPath in the project before comparing a revision"));
        }
    }
    Ok(path)
}

pub fn checkout_rev(file: &Path, rev: &str) -> Result<(CheckoutDir, PathBuf)> {
    if rev.starts_with('-') || rev.is_empty() {
        return Err(Error::usage("revision must be nonempty and cannot start with '-'").hint("use a commit hash, HEAD, or a branch or tag name"));
    }
    let file = std::path::absolute(file)?;
    let dir = file.parent().unwrap_or_else(|| Path::new("."));
    let name = file.file_name().and_then(|s| s.to_str())
        .ok_or_else(|| Error::usage("git revision paths must be Unicode").hint("rename the file to a Unicode name before using --rev"))?;
    let tmp = CheckoutDir::create()?;
    let show = |rel: &str| -> Result<Vec<u8>> {
        let spec = format!("{rev}:./{rel}");
        let o = std::process::Command::new("git").arg("-C").arg(dir).args(["show", "--end-of-options", &spec]).output()
            .map_err(|e| Error::new(ErrorCode::Io, format!("cannot run git: {e}")).hint("install git and make sure it is on PATH"))?;
        if !o.status.success() {
            return Err(Error::not_found(format!("git show {spec}: {}", String::from_utf8_lossy(&o.stderr).trim()))
                .hint("the file must be committed at that revision; check `git log -- <file>`"));
        }
        Ok(o.stdout)
    };
    let main = show(name)?;
    let probe = tmp.0.join(name);
    std::fs::write(&probe, &main)?;
    if DocKind::of(&file) != DocKind::Project { return Ok((tmp, probe)); }
    let docs = Project::load(&probe)?.documents.iter()
        .filter(|d| !d.generated && matches!(d.kind, DocKind::Schematic | DocKind::Pcb))
        .map(|d| relative(&d.rel)).collect::<Result<Vec<_>>>()?;
    std::fs::remove_file(&probe)?;
    let depth = docs.iter().map(|p| p.iter().take_while(|s| *s == "..").count()).max().unwrap_or(0);
    let mut project_dir = tmp.0.clone();
    for i in 0..depth { project_dir.push(format!("d{i}")); }
    std::fs::create_dir_all(&project_dir)?;
    let copy = project_dir.join(name);
    std::fs::write(&copy, main)?;
    for parts in docs {
        let target = within(&tmp.0, &project_dir, &parts)?;
        if let Ok(bytes) = show(&parts.join("/")) {
            if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
            std::fs::write(target, bytes)?;
        }
    }
    Ok((tmp, copy))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize_embedded_parents_and_reject_windows_prefixes() {
        assert_eq!(relative(r"Sub\..\..\shared.SchDoc").unwrap(), ["..", "shared.SchDoc"]);
        for p in [r"C:evil.SchDoc", r"C:\evil.SchDoc", r"\\host\file.SchDoc", "/tmp/evil.SchDoc"] { assert!(relative(p).is_err()); }
        assert!(within(Path::new("scratch"), Path::new("scratch"), &["..".into(), "evil".into()]).is_err());
    }
}
