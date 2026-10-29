//! Wine host: Altium in a local Wine prefix (Bottles Flatpak or plain Wine). Files are not
//! copied; Altium reads the project in place through Wine's `Z:` drive (Bottles needs a
//! Flatpak filesystem grant for directories outside the bottle).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use altium_core::{Error, ErrorCode, Result};

use crate::config::home;
use crate::host::{Host, Staged};
use crate::install::{BOTTLES_APP, Installation, Runtime};
use crate::{paths, runtime};

pub struct WineHost {
    pub inst: Installation,
    altium_dir: String,
}

impl WineHost {
    pub fn new(inst: Installation) -> WineHost {
        let altium_dir = inst.altium_dir_windows();
        WineHost { inst, altium_dir }
    }

    fn unix(&self, win: &str) -> Result<PathBuf> {
        paths::to_unix(&self.inst.prefix, win).ok_or_else(|| Error::internal(format!("cannot map {win} to the host")))
    }

    /// For Flatpak Bottles: the directory that needs a filesystem override if `p` is invisible
    /// inside the sandbox.
    pub fn sandbox_blocks(&self, p: &Path) -> Option<PathBuf> {
        if !matches!(self.inst.runtime, Runtime::Bottles { .. }) || p.starts_with(&self.inst.prefix) {
            return None;
        }
        let h = home();
        let visible = flatpak_filesystems().iter().any(|g| match g.as_str() {
            "host" | "host-os" => true,
            "home" | "~" => p.starts_with(&h),
            other => {
                let other = other.trim_end_matches(":rw").trim_end_matches(":ro").trim_end_matches(":create");
                let gp = if let Some(r) = other.strip_prefix("~/") { h.join(r) } else { PathBuf::from(other) };
                p.starts_with(gp)
            }
        });
        if visible { None } else { Some(p.parent().map(Path::to_path_buf).unwrap_or_else(|| p.to_path_buf())) }
    }

    /// Give the Bottles sandbox access to a directory.
    pub fn grant(&self, dir: &Path) -> Result<Value> {
        if !matches!(self.inst.runtime, Runtime::Bottles { .. }) {
            return Ok(json!({"changed": false, "reason": "plain Wine has no sandbox"}));
        }
        let abs = paths::absolute(dir);
        let st = std::process::Command::new("flatpak")
            .args(["override", "--user", &format!("--filesystem={}", abs.display()), BOTTLES_APP])
            .status()?;
        if !st.success() {
            return Err(Error::new(ErrorCode::Io, "flatpak override failed"));
        }
        Ok(json!({"changed": true, "granted": abs, "note": "restart Altium for the change to apply"}))
    }
}

/// `filesystems=` entries of the Flatpak overrides for Bottles.
pub fn flatpak_filesystems() -> Vec<String> {
    let mut v = vec![];
    for p in [home().join(".local/share/flatpak/overrides").join(BOTTLES_APP), PathBuf::from("/var/lib/flatpak/overrides").join(BOTTLES_APP)] {
        if let Ok(s) = std::fs::read_to_string(p) {
            for line in s.lines() {
                if let Some(list) = line.strip_prefix("filesystems=") {
                    v.extend(list.split(';').filter(|x| !x.is_empty() && !x.starts_with('!')).map(str::to_owned));
                }
            }
        }
    }
    v
}

fn changed_since(dir: &Path, t: SystemTime, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            let n = e.file_name().to_string_lossy().into_owned();
            if depth > 0 && n != "History" && n != "__Previews" && !n.starts_with('.') {
                changed_since(&p, t, depth - 1, out);
            }
        } else if md.modified().is_ok_and(|m| m > t) {
            out.push(p);
        }
    }
}

impl Host for WineHost {
    fn kind(&self) -> &'static str {
        "wine"
    }

    fn describe(&self) -> Value {
        json!({"prefix": self.inst.prefix, "altium": self.inst.altium_name, "runtime": self.inst.runtime})
    }

    fn altium_dir(&self) -> &str {
        &self.altium_dir
    }

    fn write(&self, win: &str, data: &[u8]) -> Result<()> {
        let p = self.unix(win)?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, data)?;
        Ok(())
    }

    fn read(&self, win: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.unix(win)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn exists(&self, win: &str) -> Result<bool> {
        Ok(self.unix(win)?.exists())
    }

    fn remove(&self, win: &str) -> Result<()> {
        match std::fs::remove_file(self.unix(win)?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    fn mkdirs(&self, win: &str) -> Result<()> {
        std::fs::create_dir_all(self.unix(win)?)?;
        Ok(())
    }

    fn list(&self, win_dir: &str) -> Result<Vec<String>> {
        Ok(std::fs::read_dir(self.unix(win_dir)?)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect())
    }

    fn launch_x2(&self, args: &[String]) -> Result<Option<u32>> {
        let prior = runtime::altium_pid(&self.inst);
        let log = self.inst.bridge_root().join("wine.log");
        std::fs::create_dir_all(self.inst.bridge_root())?;
        runtime::spawn_x2(&self.inst, args, &log)?;
        Ok(prior)
    }

    fn altium_pid(&self) -> Result<Option<u32>> {
        Ok(runtime::altium_pid(&self.inst))
    }

    fn kill_altium(&self) -> Result<usize> {
        Ok(runtime::kill_prefix(&self.inst))
    }

    fn clock(&self) -> Result<String> {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        Ok(ms.to_string())
    }

    fn stage_in(&self, root: &Path, file: &Path) -> Result<Staged> {
        let abs = paths::absolute(file);
        if !abs.exists() {
            return Err(Error::new(ErrorCode::FileNotFound, format!("{} does not exist", abs.display())));
        }
        if let Some(dir) = self.sandbox_blocks(&abs) {
            return Err(Error::new(ErrorCode::DesignerUnavailable, format!("Altium (inside the Bottles sandbox) cannot see {}", abs.display()))
                .hint(format!("grant access once: `ace designer grant {}`", dir.display())));
        }
        let root = paths::absolute(root);
        Ok(Staged {
            win_root: paths::to_windows(&self.inst.prefix, &root),
            win_file: paths::to_windows(&self.inst.prefix, &abs),
            local_root: root,
            copied: false,
            sent: vec![],
        })
    }

    fn stage_out(&self, staged: &Staged, since: &str, _opts: altium_format::fsx::WriteOpts) -> Result<(Vec<PathBuf>, String)> {
        let ms: u64 = since.parse().unwrap_or(0);
        let t = UNIX_EPOCH + std::time::Duration::from_millis(ms);
        let mut out = vec![];
        changed_since(&staged.local_root, t, 8, &mut out);
        out.sort();
        Ok((out, self.clock()?))
    }

}
