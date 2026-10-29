//! SSH host: Altium on another Windows machine (a workstation, a build box, a VM) reached over
//! OpenSSH. Files and folders go through sftp batches and simple commands (tasklist, schtasks)
//! run directly: neither starts PowerShell, which is slow on a busy machine. The rest runs as
//! PowerShell via `-EncodedCommand`.
//!
//! Requirements on the Windows side (`ace designer setup` checks and installs what it can):
//! OpenSSH Server with key login, a logged-in desktop session, and the `altium-cli-extended-run`
//! scheduled task that runs `C:\altium-cli-extended\run.cmd` in that session — GUI apps such as Altium
//! cannot start from an SSH session.
//!
//! Projects are synced, not shared: [`SshHost::stage_in`] copies only files whose size or
//! modification time changed since the last sync (a manifest per project root), so documents
//! Altium already has open are not rewritten under it. [`SshHost::stage_out`] brings back what
//! Altium wrote.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use altium_core::{Error, ErrorCode, Result};

use crate::config::{Config, home};
use crate::host::{Host, ROOT, Staged, fnv, ps_with_out, x2_path};

/// Directories never synced to the remote machine.
const SKIP_DIRS: &[&str] = &[".git", "History", "__Previews", ".fixtures", "target", "node_modules"];

#[derive(Debug, Clone, Serialize)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key: Option<PathBuf>,
}

impl SshConfig {
    /// `None` when no SSH host is configured.
    pub fn from(cfg: &Config) -> Option<SshConfig> {
        Some(SshConfig {
            host: cfg.ssh_host.clone()?,
            port: cfg.ssh_port.unwrap_or(22),
            user: cfg.ssh_user.clone().unwrap_or_else(|| std::env::var("USER").unwrap_or_default()),
            key: cfg.ssh_key.clone(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// SSH plumbing

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// PowerShell single-quoted literal.
pub fn psq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub struct SshHost {
    pub sc: SshConfig,
    altium_dir: String,
}

/// The batch file the `altium-cli-extended-run` task executes in the desktop session. It runs every
/// queued `run\<id>.cmd` once. Queued files (instead of one rewritten script) keep concurrent
/// callers apart, and the task itself redirects nothing: a program started from it (Altium)
/// would otherwise inherit and lock the log file, so every later run would fail.
pub const RUNNER_CMD: &str = "@echo off\r
chcp 65001 >nul\r
if not exist C:\\altium-cli-extended\\run\\active mkdir C:\\altium-cli-extended\\run\\active\r
for %%f in (\"C:\\altium-cli-extended\\run\\*.cmd\") do (\r
  move /y \"%%~f\" \"C:\\altium-cli-extended\\run\\active\\%%~nxf\" >nul 2>&1 && (\r
    call \"C:\\altium-cli-extended\\run\\active\\%%~nxf\" > \"C:\\altium-cli-extended\\run\\active\\%%~nf.log\" 2>&1\r
    del \"C:\\altium-cli-extended\\run\\active\\%%~nxf\" >nul 2>&1\r
  )\r
)\r
";

/// Run over SSH to start the runner: reports Altium's PID, then kicks the task. A file, so it
/// works whatever the SSH server's default shell is (cmd or PowerShell).
pub const KICK_CMD: &str = "@echo off\r
tasklist /fi \"imagename eq X2.EXE\" /fo csv /nh\r
schtasks /run /tn altium-cli-extended-run\r
";

/// Shared task definition for local Windows and SSH hosts.
pub(crate) const REGISTER_RUNNER_PS1: &str = r#"$ErrorActionPreference = 'Stop'
              # conhost --headless: no console window pops up over Altium (Windows 10 1809+).
              $a = New-ScheduledTaskAction -Execute 'conhost.exe' -Argument '--headless cmd.exe /c C:\altium-cli-extended\run.cmd'
              $p = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
              $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances Parallel
              Register-ScheduledTask -TaskName 'altium-cli-extended-run' -Action $a -Principal $p -Settings $s -Force | Out-Null"#;

/// Bumped when [`RUNNER_CMD`], [`KICK_CMD`] or the task definition changes; stored in the
/// host cache.
const RUNNER_VERSION: u32 = 5;

/// Facts about an SSH host that are slow to discover and rarely change.
#[derive(Debug, Default, Serialize, Deserialize)]
struct HostCache {
    altium_dir: String,
    runner_version: u32,
}

impl SshHost {
    fn cache_path(sc: &SshConfig) -> PathBuf {
        crate::config::state_dir().join(format!("ssh-{}-{}.json", sc.host.replace([':', '/', '\\'], "_"), sc.port))
    }

    fn load_cache(sc: &SshConfig) -> HostCache {
        std::fs::read(Self::cache_path(sc)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save_cache(&self, c: &HostCache) {
        let p = Self::cache_path(&self.sc);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, serde_json::to_vec(c).unwrap_or_default());
    }

    /// Connect and locate Altium on the Windows machine. The Altium folder is cached (checked
    /// with one cheap sftp call); PowerShell only runs on first contact or after a reinstall.
    pub fn connect(sc: SshConfig) -> Result<SshHost> {
        let cache = Self::load_cache(&sc);
        let mut h = SshHost { sc, altium_dir: cache.altium_dir.clone() };
        if !h.altium_dir.is_empty() && h.exists(&x2_path(&h.altium_dir))? {
            return Ok(h);
        }
        let dir = h.ps(
            r"$d = Get-ChildItem 'C:\Program Files\Altium' -Directory -ErrorAction SilentlyContinue |
                 Where-Object { Test-Path (Join-Path $_.FullName 'X2.EXE') } | Sort-Object Name | Select-Object -Last 1
              if ($d) { $d.FullName }",
        )?;
        h.altium_dir = dir.trim().to_owned();
        h.save_cache(&HostCache { altium_dir: h.altium_dir.clone(), ..cache });
        Ok(h)
    }

    pub fn has_altium(&self) -> bool {
        !self.altium_dir.is_empty()
    }

    /// Is the `altium-cli-extended-run` task there, in its current form?
    pub fn runner_installed(&self) -> bool {
        self.exec(&["schtasks", "/query", "/tn", "altium-cli-extended-run", "/xml"])
            .is_ok_and(|o| o.status.success() && runner_xml_current(&String::from_utf8_lossy(&o.stdout)))
    }

    /// Register (or upgrade) the `altium-cli-extended-run` task for the SSH user's interactive session.
    pub fn install_runner(&self) -> Result<()> {
        self.ps(crate::security::LOCK_ROOT)?;
        self.mkdirs(&format!(r"{ROOT}\run"))?;
        self.write_many(&[
            (format!(r"{ROOT}\run.cmd"), RUNNER_CMD.as_bytes().to_vec()),
            (format!(r"{ROOT}\kick.cmd"), KICK_CMD.as_bytes().to_vec()),
        ])?;
        self.ps(
            REGISTER_RUNNER_PS1,
        )?;
        let mut c = Self::load_cache(&self.sc);
        c.runner_version = RUNNER_VERSION;
        self.save_cache(&c);
        Ok(())
    }

    /// Upgrade a runner installed by an older version (once; remembered in the host cache).
    fn ensure_runner(&self) -> Result<()> {
        if Self::load_cache(&self.sc).runner_version >= RUNNER_VERSION {
            return Ok(());
        }
        if self.runner_installed() && self.exists(&format!(r"{ROOT}\run"))? && self.read(&format!(r"{ROOT}\kick.cmd"))?.as_deref() == Some(KICK_CMD.as_bytes()) {
            let mut c = Self::load_cache(&self.sc);
            c.runner_version = RUNNER_VERSION;
            self.save_cache(&c);
            return Ok(());
        }
        self.install_runner()
    }

    /// Run a .cmd body in the interactive desktop session through the runner task. Returns
    /// the PID of Altium as it was just before (one round trip for both).
    fn run_interactive(&self, cmd_body: &str) -> Result<Option<u32>> {
        let (path, body) = self.payload(cmd_body)?;
        self.write(&path, &body)?;
        self.kick()
    }

    /// A queued runner file for `cmd_body` (path, contents).
    fn payload(&self, cmd_body: &str) -> Result<(String, Vec<u8>)> {
        if self.exists(ROOT)? {
            crate::security::check_acl(&self.ps(crate::security::READ_ACL)?)?;
        }
        self.ensure_runner()?;
        let id = format!("{}-{}", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"), std::process::id());
        Ok((format!(r"{ROOT}\run\{id}.cmd"), format!("@echo off\r\n{cmd_body}\r\n").into_bytes()))
    }

    /// The runner file that starts X2.EXE with `args`.
    fn launch_payload(&self, args: &[String]) -> Result<(String, Vec<u8>)> {
        if !self.has_altium() {
            return Err(Error::new(ErrorCode::DesignerUnavailable, "Altium Designer is not installed on the SSH host")
                .hint("install Altium Designer on that machine (`ace designer setup`)"));
        }
        // Each argument is quoted, so cmd.exe leaves | > ( ) inside it alone.
        let mut line = format!("start \"\" \"{}\"", x2_path(&self.altium_dir));
        for a in args {
            line.push_str(&format!(" \"{a}\""));
        }
        self.payload(&line)
    }

    /// Start the runner task (it runs every queued file); returns Altium's PID before that.
    fn kick(&self) -> Result<Option<u32>> {
        let out = self.exec(&[r"C:\altium-cli-extended\kick.cmd"])?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        if !text.contains("SUCCESS") {
            return Err(Error::new(ErrorCode::DesignerUnavailable, "the altium-cli-extended-run task is missing on the SSH host")
                .hint("`ace designer setup --apply runner` installs it (a user must be logged in to the desktop)")
                .details(json!({"output": text, "stderr": String::from_utf8_lossy(&out.stderr)})));
        }
        Ok(parse_tasklist_pid(&text))
    }

    /// Options shared by ssh, scp and sftp: batch mode, our known_hosts, connection reuse.
    /// A busy Windows machine can take well over 10 s to send its SSH banner, hence the
    /// generous connect timeout; connection reuse keeps that cost to the first command.
    fn common_opts(&self, c: &mut Command) {
        if let Some(k) = &self.sc.key {
            c.arg("-i").arg(k);
        }
        let cm = home().join(".ssh").join(format!("cm-altium-{}-{}", self.sc.host, self.sc.port));
        c.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=60", "-o", "StrictHostKeyChecking=yes"]);
        c.arg("-o").arg(format!("UserKnownHostsFile={}", home().join(".ssh/known_hosts_altium").display()));
        if cfg!(unix) {
            c.args(["-o", "ControlMaster=auto", "-o", "ControlPersist=600"]);
            c.arg("-o").arg(format!("ControlPath={}", cm.display()));
        }
    }

    fn ssh_base(&self) -> Command {
        let mut c = Command::new("ssh");
        c.arg("-p").arg(self.sc.port.to_string());
        self.common_opts(&mut c);
        c.arg(format!("{}@{}", self.sc.user, self.sc.host));
        c
    }

    /// Run a PowerShell script in the Windows machine; returns stdout. Non-zero exit → error.
    pub fn ps(&self, script: &str) -> Result<String> {
        // Real failures throw (ErrorActionPreference=Stop) and exit non-zero; a silenced error must not.
        let full = format!("$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'\n{script}\nexit 0");
        let utf16: Vec<u8> = full.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let out: Output = self
            .ssh_base()
            .args(["powershell", "-NoProfile", "-NonInteractive", "-EncodedCommand", &b64(&utf16)])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| Error::new(ErrorCode::DesignerUnavailable, format!("ssh failed to start: {e}")))?;
        if out.status.code() == Some(255) {
            return Err(unreachable_error(&out));
        }
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(Error::new(ErrorCode::DesignerFailed, format!("remote command failed: {}", clean_ps_error(&err)))
                .hint("see error.details")
                .details(json!({"stderr": err, "stdout": String::from_utf8_lossy(&out.stdout)})));
        }
        Ok(String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"))
    }

    /// Run one plain command (no PowerShell) in the Windows machine.
    fn exec(&self, args: &[&str]) -> Result<Output> {
        let out = self
            .ssh_base()
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| Error::new(ErrorCode::DesignerUnavailable, format!("ssh failed to start: {e}")))?;
        if out.status.code() == Some(255) {
            return Err(unreachable_error(&out));
        }
        Ok(out)
    }

    /// Run an sftp batch. sftp starts neither a shell nor PowerShell on the Windows side, so it
    /// is cheap enough to poll with and does not depend on the server's default shell.
    /// Returns stdout when every line succeeded, None when one failed (a line starting with
    /// `-` may fail without failing the batch).
    fn sftp(&self, batch: &str) -> Result<Option<String>> {
        let mut c = Command::new("sftp");
        c.arg("-q").arg("-P").arg(self.sc.port.to_string());
        self.common_opts(&mut c);
        c.args(["-b", "-"]).arg(format!("{}@{}", self.sc.user, self.sc.host));
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::new(ErrorCode::DesignerUnavailable, format!("sftp failed to start: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(batch.as_bytes())?;
        }
        let out = child.wait_with_output()?;
        match out.status.code() {
            Some(0) => Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned())),
            Some(1) => Ok(None),
            _ => Err(unreachable_error(&out)),
        }
    }

    /// `-mkdir` lines for every folder from the drive down to `win_dir`.
    fn mkdir_lines(win_dir: &str) -> String {
        let parts: Vec<&str> = win_dir.split('\\').filter(|p| !p.is_empty()).collect();
        (2..=parts.len()).map(|n| format!("-mkdir \"{}\"\n", sftp_path(&parts[..n].join("\\")))).collect()
    }

    fn local_tmp() -> Result<TransferDir> {
        let state = crate::config::state_dir();
        std::fs::create_dir_all(&state)?;
        TransferDir::new_in(&state.join("ssh-transfers"))
    }

    fn state_dir() -> Result<PathBuf> {
        let d = crate::config::state_dir().join("sync");
        std::fs::create_dir_all(&d)?;
        Ok(d)
    }
}

/// A private, exclusive workspace for one transfer; tar and sftp never share filenames
/// across operations, and failed transfers leave no local design payload behind.
struct TransferDir(PathBuf);

impl TransferDir {
    fn new_in(parent: &Path) -> Result<Self> {
        fn create(path: &Path) -> std::io::Result<()> {
            let builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            builder.create(path)
        }
        match create(parent) {
            Ok(()) => {},
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {},
            Err(e) => return Err(e.into()),
        }
        let md = std::fs::symlink_metadata(parent)?;
        let private = md.is_dir() && !md.file_type().is_symlink();
        #[cfg(unix)]
        let private = {
            use std::os::unix::fs::PermissionsExt;
            private && md.permissions().mode() & 0o077 == 0
        };
        if !private {
            return Err(Error::new(ErrorCode::Unsafe, format!("SSH transfer directory is not private: {}", parent.display()))
                .hint("remove symlinks and restrict the SSH transfer directory to its owner (chmod 700 on Unix), then retry"));
        }
        for i in 0..100 {
            let stamp = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let path = parent.join(format!("{}-{stamp}-{i}", std::process::id()));
            match create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::new(ErrorCode::Io, "cannot allocate a private SSH transfer directory")
            .hint("check free space and permissions in the per-user state directory"))
    }
}

impl std::ops::Deref for TransferDir {
    type Target = Path;
    fn deref(&self) -> &Path { &self.0 }
}

impl Drop for TransferDir {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

/// The task runs `run.cmd` in a hidden console without redirecting its output (see
/// [`RUNNER_CMD`]).
pub(crate) fn runner_xml_current(xml: &str) -> bool {
    // schtasks can emit UTF-16 even when its stdout is captured as bytes.
    let x = xml.replace('\0', "").to_ascii_lowercase();
    x.contains("run.cmd") && !x.contains("run.log") && x.contains("--headless") && x.contains("<runlevel>leastprivilege</runlevel>")
}

/// PID from `tasklist /fo csv /nh` output (`"X2.EXE","7556","Console","1","525,276 K"`).
fn parse_tasklist_pid(out: &str) -> Option<u32> {
    out.lines()
        .find(|l| l.to_ascii_uppercase().starts_with("\"X2.EXE\""))
        .and_then(|l| l.split("\",\"").nth(1))
        .and_then(|pid| pid.trim_matches('"').parse().ok())
}

/// `C:\a\b` as an sftp path (`/C:/a/b`).
fn sftp_path(win: &str) -> String {
    format!("/{}", win.replace('\\', "/"))
}

fn unreachable_error(out: &Output) -> Error {
    let err = String::from_utf8_lossy(&out.stderr);
    Error::new(ErrorCode::DesignerUnavailable, format!("cannot reach the Windows machine over SSH: {}", err.trim()))
        .hint("verify the host key fingerprint through a trusted channel and pin it in ~/.ssh/known_hosts_altium; also check ssh_host/ssh_port/ssh_user/ssh_key and OpenSSH Server")
        .details(json!({"stderr": err}))
}

fn clean_ps_error(s: &str) -> String {
    // CLIXML or multi-line errors: keep the first meaningful line.
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("#<") && !l.starts_with("At line"))
        .unwrap_or("unknown error")
        .chars()
        .take(400)
        .collect()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    files: BTreeMap<String, (u64, u128)>,
}

fn safe_pull_path(root: &Path, relative: &str) -> Result<PathBuf> {
    if relative.is_empty() || relative.contains([':', '\\']) || Path::new(relative).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(Error::new(ErrorCode::Unsafe, format!("unsafe remote output path: {relative}"))
            .hint("keep output files inside the staged project tree"));
    }
    let mut path = root.to_path_buf();
    for part in Path::new(relative).components() {
        path.push(part);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::new(ErrorCode::Unsafe, format!("output path is a symlink: {}", path.display()))
                .hint("pull into a project tree without symlinks"));
        }
    }
    Ok(path)
}

fn verify_pull_target(path: &Path, relative: &str, baseline: &Manifest, incoming: &[u8]) -> Result<()> {
    let Ok(md) = std::fs::metadata(path) else { return Ok(()) };
    if std::fs::read(path).ok().as_deref() == Some(incoming) { return Ok(()) }
    let stamp = (md.len(), md.modified()?.duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
    if baseline.files.get(relative) != Some(&stamp) {
        return Err(Error::new(ErrorCode::Unsafe, format!("local file changed since staging: {}", path.display()))
            .hint("keep your local edit, or move it aside before `ace designer pull`; no local files were overwritten"));
    }
    Ok(())
}

fn write_pulled_files(files: Vec<(PathBuf, Vec<u8>)>, opts: altium_format::fsx::WriteOpts) -> Result<()> {
    for (path, data) in files {
        if let Some(parent) = path.parent() && !opts.dry_run {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::read(&path).ok().as_deref() != Some(data.as_slice()) {
            altium_format::fsx::replace(&path, &data, opts)?;
        }
    }
    Ok(())
}

fn scan(root: &Path, dir: &Path, out: &mut BTreeMap<String, (u64, u128)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(md) = e.metadata() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if md.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with("Project Outputs") {
                scan(root, &p, out);
            }
        } else if md.is_file() && !name.to_ascii_lowercase().ends_with(".bak") {
            let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().into_owned();
            let mt = md.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
            out.insert(rel, (md.len(), mt));
        }
    }
}

impl Host for SshHost {
    fn kind(&self) -> &'static str {
        "ssh"
    }

    fn describe(&self) -> Value {
        json!({
            "ssh": format!("{}@{}:{}", self.sc.user, self.sc.host, self.sc.port),
            "altium_dir": self.altium_dir,
        })
    }

    fn altium_dir(&self) -> &str {
        &self.altium_dir
    }

    // Files and folders go through one sftp batch each: a single round trip, no remote shell.

    fn write(&self, win: &str, data: &[u8]) -> Result<()> {
        let parent = win.rsplit_once('\\').map(|(p, _)| p).unwrap_or("");
        let transfer = Self::local_tmp()?;
        let tmp = transfer.join(format!("w-{}-{}", std::process::id(), fnv(win)));
        std::fs::write(&tmp, data)?;
        let r = self.sftp(&format!("{}put \"{}\" \"{}\"\n", Self::mkdir_lines(parent), tmp.display(), sftp_path(win)));
        let _ = std::fs::remove_file(&tmp);
        match r? {
            Some(_) => Ok(()),
            None => Err(Error::new(ErrorCode::DesignerFailed, format!("could not write {win} on the SSH host"))),
        }
    }

    fn read(&self, win: &str) -> Result<Option<Vec<u8>>> {
        let transfer = Self::local_tmp()?;
        let tmp = transfer.join(format!("r-{}-{}", std::process::id(), fnv(win)));
        if self.sftp(&format!("get \"{}\" \"{}\"\n", sftp_path(win), tmp.display()))?.is_none() {
            return Ok(None);
        }
        let b = std::fs::read(&tmp)?;
        let _ = std::fs::remove_file(&tmp);
        Ok(Some(b))
    }

    fn write_many(&self, files: &[(String, Vec<u8>)]) -> Result<()> {
        let tmp = Self::local_tmp()?;
        let mut batch = String::new();
        let mut made = std::collections::BTreeSet::new();
        let mut temps = vec![];
        for (i, (win, data)) in files.iter().enumerate() {
            let parent = win.rsplit_once('\\').map(|(p, _)| p).unwrap_or("");
            if made.insert(parent.to_owned()) {
                batch.push_str(&Self::mkdir_lines(parent));
            }
            let t = tmp.join(format!("w-{}-{i}-{}", std::process::id(), fnv(win)));
            std::fs::write(&t, data)?;
            batch.push_str(&format!("put \"{}\" \"{}\"\n", t.display(), sftp_path(win)));
            temps.push(t);
        }
        let r = self.sftp(&batch);
        for t in temps {
            let _ = std::fs::remove_file(t);
        }
        match r? {
            Some(_) => Ok(()),
            None => Err(Error::new(ErrorCode::DesignerFailed, "could not write files on the SSH host").details(json!({"files": files.iter().map(|f| &f.0).collect::<Vec<_>>()}))),
        }
    }

    fn read_many(&self, wins: &[String]) -> Result<Vec<Option<Vec<u8>>>> {
        let tmp = Self::local_tmp()?;
        let locals: Vec<PathBuf> = wins.iter().enumerate().map(|(i, w)| tmp.join(format!("r-{}-{i}-{}", std::process::id(), fnv(w)))).collect();
        for l in &locals {
            let _ = std::fs::remove_file(l);
        }
        let batch: String = wins.iter().zip(&locals).map(|(w, l)| format!("-get \"{}\" \"{}\"\n", sftp_path(w), l.display())).collect();
        self.sftp(&batch)?;
        Ok(locals
            .iter()
            .map(|l| {
                let b = std::fs::read(l).ok();
                let _ = std::fs::remove_file(l);
                b
            })
            .collect())
    }

    fn exists(&self, win: &str) -> Result<bool> {
        Ok(self.sftp(&format!("ls -1 \"{}\"\n", sftp_path(win)))?.is_some())
    }

    fn remove(&self, win: &str) -> Result<()> {
        self.sftp(&format!("-rm \"{}\"\n", sftp_path(win)))?;
        Ok(())
    }

    fn mkdirs(&self, win: &str) -> Result<()> {
        match self.sftp(&format!("{}ls -1 \"{}\"\n", Self::mkdir_lines(win), sftp_path(win)))? {
            Some(_) => Ok(()),
            None => Err(Error::new(ErrorCode::DesignerFailed, format!("could not create {win} on the SSH host"))),
        }
    }

    fn list(&self, win_dir: &str) -> Result<Vec<String>> {
        let Some(out) = self.sftp(&format!("ls -1 \"{}\"\n", sftp_path(win_dir)))? else { return Ok(vec![]) };
        Ok(out
            .lines()
            .filter(|l| !l.starts_with("sftp>"))
            .filter_map(|l| l.trim().rsplit('/').next().map(str::to_owned))
            .filter(|n| !n.is_empty() && n != "." && n != "..")
            .collect())
    }

    fn launch_x2(&self, args: &[String]) -> Result<Option<u32>> {
        let (path, body) = self.launch_payload(args)?;
        self.write(&path, &body)?;
        self.kick()
    }

    fn altium_pid(&self) -> Result<Option<u32>> {
        let out = self.exec(&["tasklist", "/fi", "\"imagename eq X2.EXE\"", "/fo", "csv", "/nh"])?;
        Ok(parse_tasklist_pid(&String::from_utf8_lossy(&out.stdout)))
    }

    fn kill_altium(&self) -> Result<usize> {
        let out = self.ps("$p = @(Get-Process X2 -ErrorAction SilentlyContinue); $p | Stop-Process -Force; $p.Count")?;
        Ok(out.trim().parse().unwrap_or(0))
    }

    fn clock(&self) -> Result<String> {
        Ok(self.ps("(Get-Date).ToUniversalTime().ToString('o')")?.trim().to_owned())
    }

    fn stage_in(&self, root: &Path, file: &Path) -> Result<Staged> {
        let root = crate::paths::absolute(root);
        let file = crate::paths::absolute(file);
        let rel_file = file.strip_prefix(&root).map_err(|_| Error::internal(format!("{} is not under {}", file.display(), root.display())))?;
        let key = fnv(&root.to_string_lossy());
        let base = root.file_name().map(|n| n.to_string_lossy().replace(' ', "_")).unwrap_or_else(|| "root".into());
        let win_root = format!(r"{ROOT}\work\{base}-{}", &key[..8]);

        let mpath = Self::state_dir()?.join(format!("{key}.json"));
        let old: Manifest = std::fs::read(&mpath).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let mut now = Manifest::default();
        scan(&root, &root, &mut now.files);
        let changed: Vec<String> = now.files.iter().filter(|(k, v)| old.files.get(*k) != Some(v)).map(|(k, _)| k.clone()).collect();
        let remote_missing = !self.exists(&win_root)?;
        let send: Vec<String> = if remote_missing { now.files.keys().cloned().collect() } else { changed };

        if !send.is_empty() {
            let tmp = Self::local_tmp()?;
            let list = tmp.join(format!("files-{key}.txt"));
            let tarf = tmp.join(format!("in-{key}.tar"));
            std::fs::write(&list, send.join("\n") + "\n")?;
            let st = Command::new("tar").arg("-cf").arg(&tarf).arg("-C").arg(&root).arg("-T").arg(&list).status()?;
            if !st.success() {
                return Err(Error::new(ErrorCode::Io, "tar failed while packing the project"));
            }
            let win_tar = format!(r"{ROOT}\work\in-{key}.tar");
            self.write(&win_tar, &std::fs::read(&tarf)?)?;
            self.ps(&format!(
                "New-Item -ItemType Directory -Force -Path {0} | Out-Null; tar -xf {1} -C {0}; if ($LASTEXITCODE) {{ throw 'tar failed' }}; Remove-Item {1}",
                psq(&win_root),
                psq(&win_tar)
            ))?;
            let _ = std::fs::remove_file(&tarf);
            let _ = std::fs::remove_file(&list);
        }
        std::fs::write(&mpath, serde_json::to_vec(&now)?)?;
        Ok(Staged {
            win_file: format!(r"{win_root}\{}", rel_file.to_string_lossy().replace('/', "\\")),
            win_root,
            local_root: root,
            copied: true,
            sent: send,
        })
    }

    fn staged_location(&self, root: &Path, file: &Path) -> Result<Staged> {
        let root = crate::paths::absolute(root);
        let file = crate::paths::absolute(file);
        let rel = file.strip_prefix(&root).map_err(|_| Error::usage("file is outside the staged root"))?;
        let key = fnv(&root.to_string_lossy());
        let base = root.file_name().map(|n| n.to_string_lossy().replace(' ', "_")).unwrap_or_else(|| "root".into());
        let win_root = format!(r"{ROOT}\work\{base}-{}", &key[..8]);
        if !Self::state_dir()?.join(format!("{key}.json")).exists() || !self.exists(&win_root)? {
            return Err(Error::not_found("this project has no staged copy").hint("run `ace designer push <project>` first"));
        }
        Ok(Staged { win_file: format!(r"{win_root}\{}", rel.to_string_lossy().replace('/', "\\")), win_root, local_root: root, copied: true, sent: vec![] })
    }

    fn stage_out(&self, staged: &Staged, since: &str, opts: altium_format::fsx::WriteOpts) -> Result<(Vec<PathBuf>, String)> {
        let listing = self.ps(&format!(
            "$t = [DateTime]::Parse({1}).ToUniversalTime(); $r = (Resolve-Path -LiteralPath {0}).Path
             'NOW ' + (Get-Date).ToUniversalTime().ToString('o')
             Get-ChildItem -LiteralPath $r -Recurse -File | Where-Object {{ $_.LastWriteTimeUtc -gt $t }} |
               ForEach-Object {{ $_.FullName.Substring($r.Length).TrimStart('\\') }}",
            psq(&staged.win_root),
            psq(since)
        ))?;
        let now = listing.lines().find_map(|l| l.trim().strip_prefix("NOW ")).unwrap_or_default().to_owned();
        let files: Vec<String> =
            listing.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("NOW ") && !l.to_ascii_lowercase().ends_with(".bak")).map(|l| l.replace('\\', "/")).collect();
        if files.is_empty() {
            return Ok((vec![], now));
        }
        let key = fnv(&staged.local_root.to_string_lossy());
        let mpath = Self::state_dir()?.join(format!("{key}.json"));
        let baseline: Manifest = serde_json::from_slice(&std::fs::read(&mpath)?).map_err(|e| {
            Error::new(ErrorCode::Unsafe, format!("invalid sync manifest: {e}")).hint("stage the project again before pulling")
        })?;
        let mut pending = vec![];
        for f in &files {
            let path = safe_pull_path(&staged.local_root, f)?;
            let data = self.read(&format!(r"{}\{}", staged.win_root, f.replace('/', "\\")))?.ok_or_else(|| {
                Error::not_found(format!("remote output disappeared: {f}")).hint("retry `ace designer pull`")
            })?;
            verify_pull_target(&path, f, &baseline, &data)?;
            pending.push((path, data));
        }
        write_pulled_files(pending, opts)?;
        if opts.dry_run { return Ok((files.iter().map(|f| staged.local_root.join(f)).collect(), String::new())); }
        // Record the new state so these files are not sent back on the next sync.
        let mpath = Self::state_dir()?.join(format!("{key}.json"));
        let mut m: Manifest = std::fs::read(&mpath).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let mut fresh = BTreeMap::new();
        scan(&staged.local_root, &staged.local_root, &mut fresh);
        for f in &files {
            if let Some(v) = fresh.get(f) {
                m.files.insert(f.clone(), *v);
            }
        }
        std::fs::write(&mpath, serde_json::to_vec(&m)?)?;
        Ok((files.iter().map(|f| staged.local_root.join(f)).collect(), now))
    }

    fn write_and_launch(&self, files: &[(String, Vec<u8>)], args: &[String]) -> Result<Option<u32>> {
        let mut all = files.to_vec();
        all.push(self.launch_payload(args)?);
        self.write_many(&all)?;
        self.kick()
    }

    fn ps_interactive(&self, script: &str, timeout: Duration) -> Result<Vec<u8>> {
        let id = format!("{}-{}", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"), std::process::id());
        let ps1 = format!(r"{ROOT}\ps\{id}.ps1");
        let out = format!(r"{ROOT}\ps\{id}.out");
        let mut body = vec![0xef, 0xbb, 0xbf];
        body.extend_from_slice(ps_with_out(script, &out).as_bytes());
        self.write(&ps1, &body)?;
        self.run_interactive(&format!("powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{ps1}\""))?;
        let t0 = Instant::now();
        while t0.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(700));
            if let Some(b) = self.read(&out)? {
                let _ = self.sftp(&format!("-rm \"{}\"\n-rm \"{}\"\n", sftp_path(&ps1), sftp_path(&out)));
                return Ok(b);
            }
        }
        Err(Error::new(ErrorCode::Timeout, format!("no result from the desktop session after {}s", timeout.as_secs()))
            .hint("is a user logged in to the Windows desktop? (`ace designer setup`)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_transfers_are_private_exclusive_and_cleaned() {
        let parent = std::env::temp_dir().join(format!("ace-transfer-test-{}", std::process::id()));
        let a = TransferDir::new_in(&parent).unwrap();
        let b = TransferDir::new_in(&parent).unwrap();
        assert_ne!(a.0, b.0);
        let payload = a.join("in-project.tar");
        std::fs::write(&payload, b"private design").unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            for path in [&parent, &a.0, &b.0] {
                assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o700);
            }
        }
        let path = a.0.clone();
        drop(a);
        assert!(!path.exists());
        assert!(b.0.exists());
        drop(b);
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(TransferDir::new_in(&parent).err().unwrap().code, ErrorCode::Unsafe);
            std::fs::remove_dir(&parent).unwrap();
            std::os::unix::fs::symlink(std::env::temp_dir(), &parent).unwrap();
            assert_eq!(TransferDir::new_in(&parent).err().unwrap().code, ErrorCode::Unsafe);
            std::fs::remove_file(&parent).unwrap();
        }
        #[cfg(not(unix))]
        std::fs::remove_dir(&parent).unwrap();
    }

    #[test]
    fn base64() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        assert_eq!(psq("it's"), "'it''s'");
    }

    #[test]
    fn ssh_requires_a_pinned_host_key() {
        let host = SshHost { sc: SshConfig { host: "example.invalid".into(), port: 22, user: "test".into(), key: None }, altium_dir: String::new() };
        let mut command = Command::new("ssh");
        host.common_opts(&mut command);
        let args: Vec<_> = command.get_args().map(|v| v.to_string_lossy().into_owned()).collect();
        assert!(args.iter().any(|v| v == "StrictHostKeyChecking=yes"));
        assert!(!args.iter().any(|v| v.contains("accept-new") || v == "StrictHostKeyChecking=no"));
        assert!(args.iter().any(|v| v.contains("known_hosts_altium")));
    }

    #[test]
    fn pull_refuses_conflicts_and_escaping_paths() {
        let dir = std::env::temp_dir().join(format!("ace-pull-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.PcbDoc");
        std::fs::write(&path, b"staged").unwrap();
        let mut baseline = Manifest::default();
        scan(&dir, &dir, &mut baseline.files);
        assert!(verify_pull_target(&path, "board.PcbDoc", &baseline, b"remote").is_ok());
        std::fs::write(&path, b"local edit").unwrap();
        assert_eq!(verify_pull_target(&path, "board.PcbDoc", &baseline, b"remote").unwrap_err().code, ErrorCode::Unsafe);
        assert_eq!(std::fs::read(&path).unwrap(), b"local edit");
        assert!(verify_pull_target(&path, "board.PcbDoc", &baseline, b"local edit").is_ok());
        assert!(verify_pull_target(&path, "board.PcbDoc", &Manifest::default(), b"remote").is_err());
        for path in ["../board", "/etc/passwd", "C:/board", "sub/../../board", "sub\\board", ""] {
            assert_eq!(safe_pull_path(&dir, path).unwrap_err().code, ErrorCode::Unsafe);
        }
        assert_eq!(safe_pull_path(&dir, "outputs/gerber.GTL").unwrap(), dir.join("outputs/gerber.GTL"));
        #[cfg(unix)] {
            std::os::unix::fs::symlink(std::env::temp_dir(), dir.join("link")).unwrap();
            assert!(safe_pull_path(&dir, "link/other").is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pulled_files_keep_first_backup_and_honour_write_flags() {
        use altium_format::fsx::WriteOpts;
        let dir = std::env::temp_dir().join(format!("ace-pull-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("board.PcbDoc");
        std::fs::write(&path, b"original").unwrap();
        let files = |data: &[u8]| vec![(path.clone(), data.to_vec())];
        write_pulled_files(files(b"preview"), WriteOpts { dry_run: true, no_backup: false }).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert!(!dir.join("board.PcbDoc.bak").exists());
        write_pulled_files(files(b"first"), WriteOpts::default()).unwrap();
        write_pulled_files(files(b"second"), WriteOpts::default()).unwrap();
        assert_eq!(std::fs::read(dir.join("board.PcbDoc.bak")).unwrap(), b"original");
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        std::fs::remove_file(dir.join("board.PcbDoc.bak")).unwrap();
        write_pulled_files(files(b"opt out"), WriteOpts { dry_run: false, no_backup: true }).unwrap();
        assert!(!dir.join("board.PcbDoc.bak").exists());
        let nested = dir.join("uncreated/board.PcbDoc");
        write_pulled_files(vec![(nested, b"preview".to_vec())], WriteOpts { dry_run: true, no_backup: false }).unwrap();
        assert!(!dir.join("uncreated").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn runner_definition() {
        assert!(!runner_xml_current("<Arguments>/c C:\\altium-cli-extended\\run.cmd >> C:\\altium-cli-extended\\run.log 2>&1</Arguments>"));
        assert!(!runner_xml_current("<Arguments>/c C:\\altium-cli-extended\\run.cmd</Arguments>"));
        assert!(!runner_xml_current("<Command>conhost.exe</Command><Arguments>--headless cmd.exe /c C:\\altium-cli-extended\\run.cmd</Arguments><RunLevel>HighestAvailable</RunLevel>"));
        assert!(runner_xml_current("<Command>conhost.exe</Command><Arguments>--headless cmd.exe /c C:\\altium-cli-extended\\run.cmd</Arguments><RunLevel>LeastPrivilege</RunLevel>"));
        let wide = "<Command>conhost.exe</Command><Arguments>--headless cmd.exe /c C:\\altium-cli-extended\\run.cmd</Arguments><RunLevel>LeastPrivilege</RunLevel>".encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
        assert!(runner_xml_current(&String::from_utf8_lossy(&wide)));
        assert!(RUNNER_CMD.contains("C:\\altium-cli-extended\\run\\*.cmd") && !RUNNER_CMD.contains("run.log"));
    }

    #[test]
    fn tasklist_pid() {
        let out = "\r\n\"X2.EXE\",\"7556\",\"Console\",\"1\",\"525,276 K\"\r\n";
        assert_eq!(parse_tasklist_pid(out), Some(7556));
        assert_eq!(parse_tasklist_pid("INFO: No tasks are running which match the specified criteria.\r\n"), None);
    }

    #[test]
    fn sftp_paths() {
        assert_eq!(sftp_path(r"C:\altium-cli-extended\jobs"), "/C:/altium-cli-extended/jobs");
        assert_eq!(
            SshHost::mkdir_lines(r"C:\altium-cli-extended\jobs\1"),
            "-mkdir \"/C:/altium-cli-extended\"\n-mkdir \"/C:/altium-cli-extended/jobs\"\n-mkdir \"/C:/altium-cli-extended/jobs/1\"\n"
        );
        assert_eq!(SshHost::mkdir_lines(""), "");
    }
}
