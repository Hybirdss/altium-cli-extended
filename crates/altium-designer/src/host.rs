//! Where Altium Designer runs, behind one interface.
//!
//! The bridge protocol and every operation only use [`Host`]: Windows paths, file transfer,
//! launching `X2.EXE`, process status. Three implementations exist:
//!
//! - [`crate::local::LocalHost`]: Altium on this Windows machine (the default on Windows).
//! - [`crate::ssh::SshHost`]: Altium on another Windows machine reached over SSH.
//! - [`crate::wine::WineHost`]: a Wine prefix on this Linux machine (Bottles Flatpak or plain Wine).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use altium_core::Result;

/// Root of everything altium-cli-extended keeps inside Windows.
pub const ROOT: &str = r"C:\altium-cli-extended";

/// A host-side directory made visible to Altium, and one file inside it.
#[derive(Debug, Clone, Serialize)]
pub struct Staged {
    /// Local directory that was staged (the project root).
    pub local_root: PathBuf,
    /// The same directory as Altium sees it.
    pub win_root: String,
    /// Windows path of the file the caller asked about.
    pub win_file: String,
    /// True when files were copied (a VM); false when Altium reads the local files directly.
    pub copied: bool,
    /// Files sent on this staging (empty when nothing changed or nothing is copied).
    pub sent: Vec<String>,
}

pub trait Host {
    /// `local`, `ssh` or `wine`.
    fn kind(&self) -> &'static str;
    /// Facts for `status`/`doctor` (paths, ports, versions).
    fn describe(&self) -> Value;
    /// Altium folder, e.g. `C:\Program Files\Altium\AD26`.
    fn altium_dir(&self) -> &str;

    fn write(&self, win: &str, data: &[u8]) -> Result<()>;
    /// `Ok(None)` when the file does not exist.
    fn read(&self, win: &str) -> Result<Option<Vec<u8>>>;
    fn exists(&self, win: &str) -> Result<bool>;
    fn mkdirs(&self, win: &str) -> Result<()>;
    /// Write `files`, then [`Host::launch_x2`]; SSH does both writes in one round trip.
    fn write_and_launch(&self, files: &[(String, Vec<u8>)], args: &[String]) -> Result<Option<u32>> {
        self.write_many(files)?;
        self.launch_x2(args)
    }
    /// Write several files; hosts with expensive round trips (SSH) do it in one.
    fn write_many(&self, files: &[(String, Vec<u8>)]) -> Result<()> {
        for (w, d) in files {
            self.write(w, d)?;
        }
        Ok(())
    }
    /// Read several files (`None` for a missing one), in order; one round trip over SSH.
    fn read_many(&self, wins: &[String]) -> Result<Vec<Option<Vec<u8>>>> {
        wins.iter().map(|w| self.read(w)).collect()
    }
    /// Delete a file (no error when it does not exist).
    fn remove(&self, win: &str) -> Result<()>;
    /// Names (not paths) of the entries of a directory; empty if it does not exist.
    fn list(&self, win_dir: &str) -> Result<Vec<String>>;

    /// Start `X2.EXE <args>` in the interactive desktop session, without waiting. A running
    /// Altium receives the arguments (e.g. a `-R` process to run). Returns the PID of the
    /// Altium that was already running, if any.
    fn launch_x2(&self, args: &[String]) -> Result<Option<u32>>;
    fn altium_pid(&self) -> Result<Option<u32>>;
    /// Kill Altium (unsaved work is lost). Returns the number of processes signalled.
    fn kill_altium(&self) -> Result<usize>;

    /// Make `file` (inside `root`) visible to Altium. `root` must contain every document the
    /// operation touches (the project folder plus any `..\` sheets it references).
    fn stage_in(&self, root: &Path, file: &Path) -> Result<Staged>;
    /// Resolve an already staged tree without uploading local files or changing its baseline.
    fn staged_location(&self, root: &Path, file: &Path) -> Result<Staged> {
        self.stage_in(root, file)
    }
    /// Bring back files Altium created or changed under the staged root since the operation
    /// started (`since` is the value [`Host::clock`] returned before it). Returns local paths.
    /// Also returns the Altium-side "now" (the next `since`), read in the same round trip.
    fn stage_out(&self, staged: &Staged, since: &str, opts: altium_format::fsx::WriteOpts) -> Result<(Vec<PathBuf>, String)>;
    /// Opaque "now" on the Altium side, compared by [`Host::stage_out`].
    fn clock(&self) -> Result<String>;

    /// Run a PowerShell script in the desktop session Altium runs in (it can see Altium's
    /// windows). The script writes its result to the path in `$out`; that file is returned.
    fn ps_interactive(&self, script: &str, timeout: std::time::Duration) -> Result<Vec<u8>> {
        let _ = (script, timeout);
        Err(altium_core::Error::new(altium_core::ErrorCode::Usage, format!("not supported for the {} host", self.kind()))
            .hint("Altium's windows are on this desktop; look at them directly"))
    }

    /// PNG of the desktop Altium runs on (to see a dialog it is waiting on).
    fn screenshot(&self) -> Result<Vec<u8>> {
        self.ps_interactive(SCREENSHOT_PS1, std::time::Duration::from_secs(40))
    }
}

/// PowerShell that saves the whole (virtual) desktop as PNG to `$out`. Must run in the
/// interactive session ([`Host::ps_interactive`]).
pub const SCREENSHOT_PS1: &str = r"Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$b = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()";

/// A script for [`Host::ps_interactive`]: the body writes `$out` (a temporary name), which is
/// renamed to `out` when the body finishes, so a reader never sees a partial file.
pub fn ps_with_out(script: &str, out: &str) -> String {
    let q = out.replace('\'', "''");
    format!("$ErrorActionPreference = 'Stop'\r\n$out = '{q}.tmp'\r\n{script}\r\nMove-Item -LiteralPath $out -Destination '{q}' -Force\r\n")
}

/// Stable identity of a host (for local caches): kind plus its description.
pub fn host_key(h: &dyn Host) -> String {
    fnv(&format!("{}:{}", h.kind(), h.describe()))
}

pub fn x2_path(altium_dir: &str) -> String {
    format!(r"{altium_dir}\X2.EXE")
}

/// FNV-1a 64-bit, hex: stable short names for staged folders and bridge versions.
pub fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}
