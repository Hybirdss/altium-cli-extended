//! Optional user configuration: `~/.config/altium-cli-extended/designer.json`.
//!
//! Everything is auto-detected when the file is absent; the file only pins choices
//! (which bottle, a plain Wine prefix instead of Bottles, timeouts).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use altium_core::{Error, ErrorCode, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Where Altium runs: `local` (this Windows machine; default on Windows), `ssh` (another
    /// Windows machine; default when `ssh_host` is set), `wine` (Linux: Bottles or a Wine prefix).
    pub host: Option<String>,
    /// Absolute path of X2.EXE when Altium is not under `C:\Program Files\Altium` (local host).
    pub altium_exe: Option<PathBuf>,
    /// SSH host: address of the Windows machine running Altium.
    pub ssh_host: Option<String>,
    /// SSH host: port (default 22).
    pub ssh_port: Option<u16>,
    /// SSH host: Windows user name.
    pub ssh_user: Option<String>,
    /// SSH host: private key file (default: ssh's own defaults/agent).
    pub ssh_key: Option<PathBuf>,
    /// Bottles bottle name (default: the one containing Altium, preferring `Altium`).
    pub bottle: Option<String>,
    /// Plain Wine: prefix directory.
    pub prefix: Option<PathBuf>,
    /// Plain Wine: wine binary (default `wine` on PATH).
    pub wine: Option<PathBuf>,
    /// Altium folder name under `Program Files\Altium` (default: newest, e.g. `AD26`).
    pub altium_dir: Option<String>,
    /// Seconds to wait for Altium to start and answer the first request.
    pub start_timeout_s: Option<u64>,
    /// Seconds to wait for an ordinary request.
    pub request_timeout_s: Option<u64>,
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// `%APPDATA%\altium-cli-extended\designer.json` on Windows, `$XDG_CONFIG_HOME/altium-cli-extended/designer.json`
/// (or `~/.config/...`) elsewhere.
pub fn path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData/Roaming"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
    };
    base.join("altium-cli-extended/designer.json")
}

/// Per-user state (locks, sync manifests): `%LOCALAPPDATA%\altium-cli-extended` or `~/.local/state/altium-cli-extended`.
pub fn state_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData/Local")).join("altium-cli-extended")
    } else {
        home().join(".local/state/altium-cli-extended")
    }
}

impl Config {
    pub fn load() -> Result<Config> {
        let p = path();
        match std::fs::read(&p) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| {
                Error::new(ErrorCode::Usage, format!("{}: {e}", p.display()))
                    .hint("fix or delete the file; `ace designer config` shows the valid keys")
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self) -> Result<PathBuf> {
        let p = path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, serde_json::to_vec_pretty(self)?)?;
        Ok(p)
    }

    pub fn start_timeout(&self) -> u64 {
        self.start_timeout_s.unwrap_or(300)
    }

    pub fn request_timeout(&self) -> u64 {
        self.request_timeout_s.unwrap_or(600)
    }
}
