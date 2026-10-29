//! Local host: Altium Designer on the same Windows machine as the CLI — the normal case.
//! Files are used in place. A scheduled task runs X2 and PowerShell helpers in the
//! logged-in desktop, including when the CLI itself was started through Windows OpenSSH.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use altium_core::{Error, ErrorCode, Result};

use crate::config::Config;
use crate::host::{Host, ROOT, Staged, ps_with_out};

pub struct LocalHost {
    x2: PathBuf,
    altium_dir: String,
    runner_current: Cell<Option<bool>>,
}

/// Find X2.EXE: config `altium_exe`, `ACE_X2` env var, then the newest
/// `<Program Files>\Altium\*\X2.EXE`.
pub fn find_x2(cfg: &Config) -> Option<PathBuf> {
    if let Some(p) = cfg.altium_exe.clone().filter(|p| p.is_file()) {
        return Some(p);
    }
    if let Some(p) = std::env::var_os("ACE_X2").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let mut roots = vec![];
    for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(v) = std::env::var_os(var) {
            roots.push(PathBuf::from(v).join("Altium"));
        }
    }
    roots.push(PathBuf::from(r"C:\Program Files\Altium"));
    let mut found: Vec<PathBuf> = roots
        .iter()
        .flat_map(|r| std::fs::read_dir(r).into_iter().flatten().flatten())
        .map(|e| e.path().join("X2.EXE"))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}

impl LocalHost {
    pub fn new(cfg: &Config) -> Result<LocalHost> {
        if !cfg!(windows) {
            return Err(Error::new(ErrorCode::DesignerUnavailable, "host=local needs Windows (Altium Designer is Windows-only)")
                .hint("on Linux/macOS set `ace designer config --set host=ssh --set ssh_host=<windows machine>`"));
        }
        let x2 = find_x2(cfg).ok_or_else(|| {
            Error::new(ErrorCode::DesignerUnavailable, "Altium Designer (X2.EXE) not found")
                .hint("install Altium Designer, or set its path: `ace designer config --set altium_exe=C:\\\\...\\\\X2.EXE`")
        })?;
        let altium_dir = x2.parent().map(|p| p.display().to_string()).unwrap_or_default();
        Ok(LocalHost { x2, altium_dir, runner_current: Cell::new(None) })
    }

    /// Install the same interactive runner used by the SSH host.
    pub fn install_runner(&self) -> Result<()> {
        self.security_ps(crate::security::LOCK_ROOT)?;
        self.mkdirs(&format!(r"{ROOT}\run"))?;
        self.write(&format!(r"{ROOT}\run.cmd"), crate::ssh::RUNNER_CMD.as_bytes())?;
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", crate::ssh::REGISTER_RUNNER_PS1])
            .output()?;
        if !out.status.success() {
            return Err(Error::new(ErrorCode::DesignerUnavailable, "cannot register the interactive runner")
                .details(json!({"stderr": String::from_utf8_lossy(&out.stderr)}))
                .hint("run `ace designer setup --apply runner` as the logged-in Windows desktop user (Limited task; no elevation required)"));
        }
        self.runner_current.set(Some(true));
        Ok(())
    }

    pub fn runner_installed(&self) -> bool {
        if let Some(current) = self.runner_current.get() { return current; }
        let current = Command::new("schtasks").args(["/query", "/tn", "altium-cli-extended-run", "/xml"])
            .output().is_ok_and(|o| o.status.success() && crate::ssh::runner_xml_current(&String::from_utf8_lossy(&o.stdout)))
            && std::fs::read(format!(r"{ROOT}\run.cmd")).is_ok_and(|b| b == crate::ssh::RUNNER_CMD.as_bytes());
        self.runner_current.set(Some(current));
        current
    }

    fn run_interactive(&self, body: &str) -> Result<()> {
        if Path::new(ROOT).exists() {
            crate::security::check_acl(&self.security_ps(crate::security::READ_ACL)?)?;
        }
        if !self.runner_installed() {
            self.install_runner()?;
        }
        let path = format!(r"{ROOT}\run\{}.cmd", unique_id());
        self.write(&path, format!("@echo off\r\n{body}\r\n").as_bytes())?;
        let out = Command::new("schtasks").args(["/run", "/tn", "altium-cli-extended-run"]).output()?;
        if !out.status.success() {
            self.remove(&path)?;
            return Err(Error::new(ErrorCode::DesignerUnavailable, "cannot start the interactive runner")
                .details(json!({"stderr": String::from_utf8_lossy(&out.stderr)}))
                .hint("log in to the Windows desktop and run `ace designer setup --apply runner`"));
        }
        Ok(())
    }

    fn path(win: &str) -> PathBuf {
        PathBuf::from(win)
    }

    fn security_ps(&self, script: &str) -> Result<String> {
        let out = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script]).output()?;
        if !out.status.success() {
            return Err(Error::new(ErrorCode::Unsafe, "cannot secure or inspect desktop runner permissions")
                .details(json!({"stderr": String::from_utf8_lossy(&out.stderr)}))
                .hint("run `ace designer setup --apply runner` as the Windows desktop user"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
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

impl Host for LocalHost {
    fn kind(&self) -> &'static str {
        "local"
    }

    fn describe(&self) -> Value {
        json!({"x2": self.x2})
    }

    fn altium_dir(&self) -> &str {
        &self.altium_dir
    }

    fn write(&self, win: &str, data: &[u8]) -> Result<()> {
        let p = Self::path(win);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, data)?;
        Ok(())
    }

    fn read(&self, win: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(Self::path(win)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn exists(&self, win: &str) -> Result<bool> {
        Ok(Self::path(win).exists())
    }

    fn remove(&self, win: &str) -> Result<()> {
        match std::fs::remove_file(Self::path(win)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    fn mkdirs(&self, win: &str) -> Result<()> {
        std::fs::create_dir_all(Self::path(win))?;
        Ok(())
    }

    fn list(&self, win_dir: &str) -> Result<Vec<String>> {
        Ok(std::fs::read_dir(Self::path(win_dir))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect())
    }

    fn launch_x2(&self, args: &[String]) -> Result<Option<u32>> {
        let prior = self.altium_pid()?;
        // cmd.exe must keep the RunScript metacharacters inside a quoted argument.
        if args.iter().any(|a| a.contains(['"', '\r', '\n', '%'])) {
            return Err(Error::usage("invalid character in Altium launch argument")
                .hint("use a path without double quotes, newlines or percent signs"));
        }
        let mut line = format!("start \"\" \"{}\"", self.x2.display());
        for arg in args {
            line.push_str(&format!(" \"{arg}\""));
        }
        self.run_interactive(&line)?;
        Ok(prior)
    }

    fn altium_pid(&self) -> Result<Option<u32>> {
        let out = Command::new("tasklist").args(["/FI", "IMAGENAME eq X2.EXE", "/FO", "CSV", "/NH"]).output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text
            .lines()
            .filter(|l| l.to_ascii_uppercase().starts_with("\"X2.EXE\""))
            .find_map(|l| l.split(',').nth(1).and_then(|p| p.trim_matches('"').parse().ok())))
    }

    fn kill_altium(&self) -> Result<usize> {
        let n = usize::from(self.altium_pid()?.is_some());
        let _ = Command::new("taskkill").args(["/IM", "X2.EXE", "/F"]).output()?;
        Ok(n)
    }

    fn clock(&self) -> Result<String> {
        Ok(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0).to_string())
    }

    fn stage_in(&self, root: &Path, file: &Path) -> Result<Staged> {
        let abs = std::path::absolute(file)?;
        if !abs.exists() {
            return Err(Error::new(ErrorCode::FileNotFound, format!("{} does not exist", abs.display())));
        }
        let root = std::path::absolute(root)?;
        Ok(Staged {
            win_root: root.display().to_string(),
            win_file: abs.display().to_string(),
            local_root: root,
            copied: false,
            sent: vec![],
        })
    }

    fn stage_out(&self, staged: &Staged, since: &str, _opts: altium_format::fsx::WriteOpts) -> Result<(Vec<PathBuf>, String)> {
        let t = UNIX_EPOCH + Duration::from_millis(since.parse().unwrap_or(0));
        let mut out = vec![];
        changed_since(&staged.local_root, t, 8, &mut out);
        out.sort();
        Ok((out, self.clock()?))
    }

    fn ps_interactive(&self, script: &str, timeout: Duration) -> Result<Vec<u8>> {
        let dir = PathBuf::from(ROOT).join("ps");
        std::fs::create_dir_all(&dir)?;
        let id = unique_id();
        let ps1 = dir.join(format!("{id}.ps1"));
        let out = dir.join(format!("{id}.out"));
        let err = dir.join(format!("{id}.err"));
        let body = format!("try {{\r\n{}\r\n}} catch {{ [IO.File]::WriteAllText({}, $_.ToString()) }}",
            ps_with_out(script, &out.display().to_string()), crate::ssh::psq(&err.display().to_string()));
        std::fs::write(&ps1, format!("\u{feff}{body}"))?;
        self.run_interactive(&format!("powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\"", ps1.display()))?;
        let started = Instant::now();
        let result = loop {
            if let Ok(data) = std::fs::read(&out) {
                break Ok(data);
            }
            if let Ok(message) = std::fs::read_to_string(&err) {
                break Err(Error::new(ErrorCode::DesignerFailed, "desktop PowerShell helper failed")
                    .details(json!({"stderr": message}))
                    .hint("check the desktop session and the helper error"));
            }
            if started.elapsed() >= timeout {
                break Err(Error::new(ErrorCode::Timeout, format!("no result from the desktop session after {}s", timeout.as_secs()))
                    .hint("log in to the Windows desktop; `ace designer setup --apply runner` installs the interactive task"));
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        // On timeout the desktop helper may still be reading its script.
        if !matches!(&result, Err(e) if e.code == ErrorCode::Timeout) {
            for p in [&ps1, &out, &err] {
                let _ = std::fs::remove_file(p);
            }
        }
        result
    }
}

fn unique_id() -> String {
    format!("{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_check_reuses_the_host_lifetime_cache() {
        for current in [true, false] {
            let host = LocalHost { x2: PathBuf::new(), altium_dir: String::new(), runner_current: Cell::new(Some(current)) };
            // This also runs on Linux without schtasks: a cached result needs no process.
            assert_eq!(host.runner_installed(), current);
            assert_eq!(host.runner_installed(), current);
        }
        assert!(crate::ssh::REGISTER_RUNNER_PS1.contains("-RunLevel Limited"));
    }
}
