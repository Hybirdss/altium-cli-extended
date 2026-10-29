//! Finding the Altium installation: a Bottles (Flatpak) bottle or a plain Wine prefix.
//!
//! Discovery never fails outright; it returns a list of [`Check`]s (what was looked at, what
//! was found, what to do next) plus an [`Installation`] when enough was found to run Altium.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

use crate::config::{Config, home};

pub const BOTTLES_APP: &str = "com.usebottles.bottles";

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Check {
    fn pass(name: &'static str, detail: impl Into<String>) -> Check {
        Check { name, ok: true, detail: detail.into(), hint: None }
    }
    fn fail(name: &'static str, detail: impl Into<String>, hint: impl Into<String>) -> Check {
        Check { name, ok: false, detail: detail.into(), hint: Some(hint.into()) }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Runtime {
    /// Bottles Flatpak: commands run inside the Bottles sandbox with the bottle's environment.
    Bottles {
        bottle: String,
        runner: String,
        runner_dir: PathBuf,
        /// Bottle environment variables from bottle.yml (SteamAppId, LD_PRELOAD, ...).
        env: IndexMap<String, String>,
        sync: String,
    },
    /// Plain Wine prefix on the host.
    Wine { wine: PathBuf },
}

#[derive(Debug, Clone, Serialize)]
pub struct Installation {
    pub runtime: Runtime,
    /// WINEPREFIX.
    pub prefix: PathBuf,
    /// Folder name under `Program Files\Altium`, e.g. `AD26`.
    pub altium_name: String,
    /// Unix path of that folder.
    pub altium_dir: PathBuf,
    pub x2: PathBuf,
}

impl Installation {
    /// Windows path of X2.EXE.
    pub fn x2_windows(&self) -> String {
        format!("C:\\Program Files\\Altium\\{}\\X2.EXE", self.altium_name)
    }
    pub fn altium_dir_windows(&self) -> String {
        format!("C:\\Program Files\\Altium\\{}", self.altium_name)
    }
    /// Directory the bridge and its jobs live in (`C:\altium-cli-extended`).
    pub fn bridge_root(&self) -> PathBuf {
        self.prefix.join("drive_c/altium-cli-extended")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Discovery {
    pub checks: Vec<Check>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installation: Option<Installation>,
}

pub fn bottles_data() -> PathBuf {
    home().join(".var/app").join(BOTTLES_APP).join("data/bottles")
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_str()?
        .split(':')
        .map(|d| Path::new(d).join(bin))
        .find(|p| p.is_file())
}

/// Folder names under `<prefix>/drive_c/Program Files/Altium` that contain X2.EXE, newest last.
fn altium_dirs(prefix: &Path) -> Vec<String> {
    let root = prefix.join("drive_c/Program Files/Altium");
    let mut v: Vec<String> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| find_ci(&e.path(), "X2.EXE").is_some())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort_by_key(|a| natural_key(a));
    v
}

fn natural_key(s: &str) -> (String, u64) {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    (s.chars().filter(|c| !c.is_ascii_digit()).collect(), digits.parse().unwrap_or(0))
}

/// Case-insensitive file lookup in one directory.
pub fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(name)))
}

/// Minimal reader for the parts of Bottles' bottle.yml we need.
pub struct BottleYml {
    pub runner: String,
    pub sync: String,
    pub env: IndexMap<String, String>,
}

pub fn read_bottle_yml(path: &Path) -> Option<BottleYml> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut runner = String::new();
    let mut sync = String::new();
    let mut env = IndexMap::new();
    let mut in_env = false;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Runner: ") {
            runner = v.trim().to_owned();
        }
        if let Some(v) = line.strip_prefix("    sync: ") {
            sync = v.trim().to_owned();
        }
        if line.starts_with("Environment_Variables:") {
            in_env = !line.trim_end().ends_with("{}");
            continue;
        }
        if in_env {
            if let Some(kv) = line.strip_prefix("  ")
                && let Some((k, v)) = kv.trim_start().split_once(": ") {
                    let v = v.trim().trim_matches('\'').trim_matches('"');
                    env.insert(k.trim().to_owned(), v.to_owned());
                    continue;
                }
            in_env = false;
        }
    }
    Some(BottleYml { runner, sync, env })
}

pub fn discover(cfg: &Config) -> Discovery {
    let mut checks = vec![];
    let want_wine = cfg.prefix.is_some() || cfg.wine.is_some();

    let (runtime, prefix) = if want_wine {
        let prefix = cfg.prefix.clone().unwrap_or_else(|| home().join(".wine"));
        let wine = cfg.wine.clone().or_else(|| which("wine"));
        match &wine {
            Some(w) => checks.push(Check::pass("wine", w.display().to_string())),
            None => {
                checks.push(Check::fail("wine", "wine not on PATH", "install wine or set \"wine\" in the config"));
                return Discovery { checks, installation: None };
            }
        }
        if !prefix.join("drive_c").is_dir() {
            checks.push(Check::fail("prefix", format!("{} is not a Wine prefix", prefix.display()), "set \"prefix\" in the config"));
            return Discovery { checks, installation: None };
        }
        checks.push(Check::pass("prefix", prefix.display().to_string()));
        (Runtime::Wine { wine: wine.unwrap() }, prefix)
    } else {
        match which("flatpak") {
            Some(p) => checks.push(Check::pass("flatpak", p.display().to_string())),
            None => {
                checks.push(Check::fail(
                    "flatpak",
                    "flatpak is not installed",
                    "run `! sudo pacman -S flatpak` (Arch) then `ace designer setup`",
                ));
                return Discovery { checks, installation: None };
            }
        }
        let data = bottles_data();
        if !data.join("bottles").is_dir() {
            checks.push(Check::fail(
                "bottles",
                format!("no Bottles data at {}", data.display()),
                format!("run `flatpak install --user -y flathub {BOTTLES_APP}` then create a bottle (see `ace designer setup`)"),
            ));
            return Discovery { checks, installation: None };
        }
        checks.push(Check::pass("bottles", data.display().to_string()));

        let bottles: Vec<String> = std::fs::read_dir(data.join("bottles"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("bottle.yml").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let chosen = match &cfg.bottle {
            Some(b) => bottles.iter().find(|x| *x == b).cloned(),
            None => bottles
                .iter()
                .find(|b| b.eq_ignore_ascii_case("Altium"))
                .or_else(|| bottles.iter().find(|b| !altium_dirs(&data.join("bottles").join(b)).is_empty()))
                .cloned(),
        };
        let Some(bottle) = chosen else {
            checks.push(Check::fail(
                "bottle",
                format!("no bottle with Altium (bottles: {bottles:?})"),
                "create a bottle named Altium (Gaming environment, soda runner) and install Altium into it; see `ace designer setup`",
            ));
            return Discovery { checks, installation: None };
        };
        let bdir = data.join("bottles").join(&bottle);
        let Some(yml) = read_bottle_yml(&bdir.join("bottle.yml")) else {
            checks.push(Check::fail("bottle", format!("cannot read {}/bottle.yml", bdir.display()), "recreate the bottle"));
            return Discovery { checks, installation: None };
        };
        checks.push(Check::pass("bottle", format!("{bottle} ({})", bdir.display())));
        let runner_dir = data.join("runners").join(&yml.runner);
        if !runner_dir.join("bin/wine").is_file() {
            checks.push(Check::fail(
                "runner",
                format!("runner {:?} not found in {}", yml.runner, data.join("runners").display()),
                "install the soda runner in Bottles (Preferences > Runners)",
            ));
            return Discovery { checks, installation: None };
        }
        checks.push(Check::pass("runner", yml.runner.clone()));
        (
            Runtime::Bottles { bottle, runner: yml.runner, runner_dir, env: yml.env, sync: yml.sync },
            bdir,
        )
    };

    let dirs = altium_dirs(&prefix);
    let name = match &cfg.altium_dir {
        Some(n) => dirs.iter().find(|d| d.eq_ignore_ascii_case(n)).cloned(),
        None => dirs.last().cloned(),
    };
    let Some(name) = name else {
        checks.push(Check::fail(
            "altium",
            format!("no X2.EXE under {}/drive_c/Program Files/Altium", prefix.display()),
            "install Altium Designer into the bottle with the offline installer; see `ace designer setup`",
        ));
        return Discovery { checks, installation: None };
    };
    let altium_dir = prefix.join("drive_c/Program Files/Altium").join(&name);
    let x2 = find_ci(&altium_dir, "X2.EXE").expect("checked by altium_dirs");
    checks.push(Check::pass("altium", format!("{name} ({})", x2.display())));

    if matches!(runtime, Runtime::Bottles { .. }) {
        let fixes = ["SHFolder.dll", "dxvk.conf"].iter().all(|f| find_ci(&altium_dir, f).is_some());
        checks.push(if fixes {
            Check::pass("wine-fixes", "altium-on-linux fixes present (SHFolder.dll proxy, dxvk.conf)")
        } else {
            Check::fail(
                "wine-fixes",
                "altium-on-linux fixes not applied",
                "run altium-on-linux's scripts/install.sh with Altium closed (see `ace designer setup`)",
            )
        });
    }

    let installation = Installation { runtime, prefix, altium_name: name, altium_dir, x2 };
    Discovery { checks, installation: Some(installation) }
}
