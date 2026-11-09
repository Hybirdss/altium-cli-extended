//! The installation path for Altium Designer on Linux, as a list of detectable steps.
//!
//! Follows <https://github.com/FaulknerMassimo/altium-on-linux> (Bottles + soda runner + its
//! fixes). Each step knows whether it is done, whether a human must do it (sudo, Altium
//! account sign-in, GUI installer) and, if not, the exact commands `--apply` runs.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Serialize, Serializer};

use altium_core::{Error, ErrorCode, Result};

use crate::config::{Config, home};
use crate::install::{BOTTLES_APP, bottles_data, discover, find_ci};

pub const FIXES_REPO: &str = "https://github.com/FaulknerMassimo/altium-on-linux.git";

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub id: &'static str,
    pub title: &'static str,
    pub done: bool,
    /// `auto` (altium can run it with --apply) or `user` (needs you: sudo, sign-in, GUI).
    pub who: &'static str,
    pub detail: String,
    /// Commands for this step. For `user` steps they are what you type yourself.
    pub commands: Vec<Cmd>,
}

/// One command of a step: a program and its arguments that `--apply` runs without a shell, or
/// an instruction (another `ace` command, a placeholder) that it never runs.
#[derive(Debug, Clone)]
pub enum Cmd {
    Run(Vec<OsString>),
    Say(String),
}

impl Cmd {
    /// What to type in a POSIX shell.
    pub fn display(&self) -> String {
        match self {
            Cmd::Run(argv) => argv.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" "),
            Cmd::Say(s) => s.clone(),
        }
    }
}

impl Serialize for Cmd {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.display())
    }
}

fn run(argv: &[&dyn AsRef<OsStr>]) -> Cmd {
    Cmd::Run(argv.iter().map(|a| a.as_ref().to_owned()).collect())
}

fn say(s: impl Into<String>) -> Cmd {
    Cmd::Say(s.into())
}

fn sh_quote(a: &OsStr) -> String {
    let s = a.to_string_lossy();
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"@%+=:,./_-".contains(&b)) {
        s.into_owned()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

fn concat(a: &str, b: &Path) -> OsString {
    let mut s = OsString::from(a);
    s.push(b);
    s
}

fn unzip_cmd(zip: &Path) -> Cmd {
    run(&[&"unzip", &"-q", &"-o", &zip, &"-d", &zip.with_extension("")])
}

fn bottles_cli(sub: &str, bottle: &str, rest: &[&dyn AsRef<OsStr>]) -> Cmd {
    let mut argv: Vec<&dyn AsRef<OsStr>> = vec![&"flatpak", &"run", &"--command=bottles-cli", &BOTTLES_APP, &sub, &"-b", &bottle];
    argv.extend_from_slice(rest);
    run(&argv)
}

pub fn fixes_dir() -> PathBuf {
    home().join(".local/share/altium-cli-extended/altium-on-linux")
}

fn has(bin: &str) -> bool {
    Command::new("sh").args(["-c", &format!("command -v {bin} >/dev/null 2>&1")]).status().is_ok_and(|s| s.success())
}

fn bottles_installed() -> bool {
    has("flatpak")
        && Command::new("flatpak")
            .args(["info", "--user", BOTTLES_APP])
            .output()
            .is_ok_and(|o| o.status.success())
}

/// Extracted offline installer folders (`OfflineSetup_Altium_Designer_Public_*`) in ~/Downloads.
pub fn offline_installers() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(home().join("Downloads"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("OfflineSetup_Altium_Designer"))
                && find_ci(p, "Installer.exe").is_some()
        })
        .collect();
    v.sort();
    v
}

fn offline_zips() -> Vec<PathBuf> {
    std::fs::read_dir(home().join("Downloads"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))
                && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("OfflineSetup_Altium_Designer"))
        })
        .collect()
}

fn bottle_env_has(bottle_dir: &Path, key: &str) -> bool {
    std::fs::read_to_string(bottle_dir.join("bottle.yml")).is_ok_and(|s| s.contains(&format!("  {key}: ")))
}

/// Steps for the configured host (see [`crate::ops::host_choice`]).
pub fn steps(cfg: &Config) -> Vec<Step> {
    match crate::ops::host_choice(cfg) {
        "local" => local_steps(cfg),
        "ssh" => ssh_steps(cfg),
        _ => wine_steps(cfg),
    }
}

const DOWNLOAD: &str = "https://www.altium.com/products/downloads (sign in; Altium Designer > Online Installer)";

fn bridge_step(done: bool) -> Step {
    Step {
        id: "bridge",
        title: "Install the altium-cli-extended bridge script",
        done,
        who: "auto",
        detail: r"C:\altium-cli-extended\bridge (also installed automatically on first use)".into(),
        commands: vec![say("ace designer install-bridge")],
    }
}

fn license_step() -> Step {
    Step {
        id: "license",
        title: "Sign in inside Altium and activate a license",
        done: false,
        who: "user",
        detail: "start Altium, License Management > Sign In. Not detectable; `ace designer ping` succeeds once Altium starts without dialogs.".into(),
        commands: vec![say("ace designer start")],
    }
}

/// This Windows machine.
pub fn local_steps(cfg: &Config) -> Vec<Step> {
    let x2 = crate::local::find_x2(cfg);
    let bridge = x2.is_some() && Path::new(r"C:\altium-cli-extended\bridge\VERSION").exists();
    vec![
        Step {
            id: "altium",
            title: "Install Altium Designer",
            done: x2.is_some(),
            who: "user",
            detail: match &x2 {
                Some(p) => p.display().to_string(),
                None => format!("download from {DOWNLOAD}; or point to an existing install: ace designer config --set altium_exe=<X2.EXE>"),
            },
            commands: vec![],
        },
        Step {
            id: "runner",
            title: "Interactive desktop runner for local Windows (including OpenSSH sessions)",
            done: crate::local::LocalHost::new(cfg).is_ok_and(|h| h.runner_installed()),
            who: "auto",
            detail: "registers altium-cli-extended-run for the logged-in Windows user".into(),
            commands: vec![say("ace designer setup --apply runner")],
        },
        license_step(),
        bridge_step(bridge),
    ]
}

/// Another Windows machine over SSH.
pub fn ssh_steps(cfg: &Config) -> Vec<Step> {
    let sc = crate::ssh::SshConfig::from(cfg);
    let host = sc.clone().and_then(|c| crate::ssh::SshHost::connect(c).ok());
    let runner = host.as_ref().is_some_and(|h| h.runner_installed());
    let has_altium = host.as_ref().is_some_and(|h| h.has_altium());
    let bridge = host.as_ref().is_some_and(|h| crate::bridge::Bridge::new(h).installed());
    vec![
        Step {
            id: "ssh-config",
            title: "Tell altium-cli-extended which Windows machine runs Altium",
            done: sc.is_some(),
            who: "user",
            detail: "address, user and key of a Windows machine with OpenSSH Server (Settings > Optional features > OpenSSH Server)".into(),
            commands: vec![say("ace designer config --set ssh_host=<address> --set ssh_user=<user> --set ssh_key=<private key>")],
        },
        Step {
            id: "ssh",
            title: "SSH login works (pinned host key, key-based login, PowerShell available)",
            done: host.is_some(),
            who: "user",
            detail: "verify the SSH host fingerprint through a trusted channel and pin it in ~/.ssh/known_hosts_altium (unknown keys are refused). On Windows: Start-Service sshd; put your public key in C:\\ProgramData\\ssh\\administrators_authorized_keys (admins) or ~/.ssh/authorized_keys".into(),
            commands: vec![say("ace designer status")],
        },
        Step {
            id: "runner",
            title: "Interactive runner task (Altium must start in the logged-in desktop, not the SSH session)",
            done: runner,
            who: "auto",
            detail: "registers scheduled task altium-cli-extended-run for the SSH user; that user must be logged in to the desktop".into(),
            commands: vec![say("(altium-cli-extended registers it over SSH)")],
        },
        Step {
            id: "altium",
            title: "Install Altium Designer on that machine",
            done: has_altium,
            who: "user",
            detail: format!("download from {DOWNLOAD} and install it there"),
            commands: vec![],
        },
        license_step(),
        bridge_step(bridge),
    ]
}

/// Wine path (Bottles + altium-on-linux fixes).
pub fn wine_steps(cfg: &Config) -> Vec<Step> {
    let bottle = cfg.bottle.clone().unwrap_or_else(|| "Altium".into());
    let bdir = bottles_data().join("bottles").join(&bottle);
    let disc = discover(cfg);
    let inst = disc.installation.as_ref();
    let installers = offline_installers();
    let mut v = vec![];

    v.push(Step {
        id: "flatpak",
        title: "Install Flatpak",
        done: has("flatpak"),
        who: "user",
        detail: "needs sudo once".into(),
        commands: vec![run(&[&"sudo", &"pacman", &"-S", &"--needed", &"flatpak"])],
    });
    v.push(Step {
        id: "bottles",
        title: "Install Bottles (Flatpak, user scope)",
        done: bottles_installed(),
        who: "auto",
        detail: BOTTLES_APP.into(),
        commands: vec![
            run(&[&"flatpak", &"remote-add", &"--user", &"--if-not-exists", &"flathub", &"https://dl.flathub.org/repo/flathub.flatpakrepo"]),
            run(&[&"flatpak", &"install", &"--user", &"-y", &"flathub", &BOTTLES_APP]),
        ],
    });
    v.push(Step {
        id: "build-tools",
        title: "Install clang, lld, llvm (to build the Wine shims)",
        done: has("clang") && has("ld.lld") && has("llvm-dlltool"),
        who: "user",
        detail: "needs sudo once".into(),
        commands: vec![run(&[&"sudo", &"pacman", &"-S", &"--needed", &"clang", &"lld", &"llvm"])],
    });
    v.push(Step {
        id: "bottle",
        title: "Create the bottle (Gaming environment, soda runner)",
        done: bdir.join("bottle.yml").is_file(),
        who: "user",
        detail: format!("open Bottles, Create new bottle: name {bottle}, environment Gaming, runner soda (soda-11.0-7 tested). Bottles installs DXVK, vkd3d-proton, fonts, d3dx9, Wine Mono."),
        commands: vec![run(&[&"flatpak", &"run", &BOTTLES_APP])],
    });
    v.push(Step {
        id: "bottle-env",
        title: "Set the bottle environment variables",
        done: bottle_env_has(&bdir, "DOTNET_SYSTEM_GLOBALIZATION_USENLS") && bottle_env_has(&bdir, "SteamAppId"),
        who: "auto",
        detail: "installer ICU fix + window class for Hyprland rules".into(),
        commands: vec![
            bottles_cli("edit", &bottle, &[&"--env-var", &"DOTNET_SYSTEM_GLOBALIZATION_USENLS=1"]),
            bottles_cli("edit", &bottle, &[&"--env-var", &"SteamAppId=altium"]),
        ],
    });
    v.push(Step {
        id: "installer",
        title: "Download the Altium Designer offline installer",
        done: !installers.is_empty(),
        who: "user",
        detail: if installers.is_empty() && !offline_zips().is_empty() {
            "a zip is in ~/Downloads: extract it (`ace designer setup --apply installer`)".into()
        } else {
            "sign in at altium.com > Downloads, get OfflineSetup_Altium_Designer_Public_<version>.zip into ~/Downloads and extract it there".into()
        },
        commands: offline_zips().iter().map(|z| unzip_cmd(z)).collect(),
    });
    let inst_dir = installers.last().cloned();
    v.push(Step {
        id: "installer-access",
        title: "Let Bottles read the installer folder",
        done: inst_dir.as_ref().is_some_and(|d| crate::wine::flatpak_filesystems().iter().any(|f| d.starts_with(f.trim_end_matches(":ro")))),
        who: "auto",
        detail: inst_dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "(installer not found yet)".into()),
        commands: inst_dir
            .iter()
            .map(|d| run(&[&"flatpak", &"override", &"--user", &concat("--filesystem=", d), &BOTTLES_APP]))
            .collect(),
    });
    v.push(Step {
        id: "install-altium",
        title: "Run the Altium installer in the bottle",
        done: inst.is_some(),
        who: "user",
        detail: "GUI installer: install as usual. If it asks to sign in and no browser opens, cancel and run it again (the second run skips sign-in).".into(),
        commands: inst_dir
            .iter()
            .map(|d| bottles_cli("run", &bottle, &[&"-e", &find_ci(d, "Installer.exe").unwrap_or_default()]))
            .collect(),
    });
    v.push(Step {
        id: "first-run",
        title: "Start Altium once, then close it",
        done: inst.is_some_and(|i| {
            std::fs::read_to_string(i.prefix.join("user.reg")).is_ok_and(|r| r.contains("Altium"))
        }),
        who: "user",
        detail: "creates the registry keys the fixes write into; misbehaviour on this run is expected".into(),
        commands: vec![say("ace designer start")],
    });
    let fixes = fixes_dir();
    v.push(Step {
        id: "fixes",
        title: "Apply the altium-on-linux fixes (Altium closed)",
        done: inst.is_some_and(|i| find_ci(&i.altium_dir, "SHFolder.dll").is_some()),
        who: "auto",
        detail: format!("clones {FIXES_REPO} into {} and runs scripts/install.sh", fixes.display()),
        commands: vec![
            if fixes.is_dir() {
                run(&[&"git", &"-C", &fixes, &"pull", &"--ff-only"])
            } else {
                run(&[&"git", &"clone", &"--depth", &"1", &FIXES_REPO, &fixes])
            },
            run(&[&fixes.join("scripts/install.sh"), &"--bottle", &bottle]),
        ],
    });
    v.push(Step {
        id: "hyprland",
        title: "Hyprland window rules (dialogs float, no focus-follows-mouse for Altium)",
        done: std::fs::read_to_string(home().join(".config/hypr/hyprland.lua")).is_ok_and(|s| s.contains("steam_app_altium"))
            || std::fs::read_to_string(home().join(".config/hypr/hyprland.conf")).is_ok_and(|s| s.contains("steam_app_altium")),
        who: "user",
        detail: format!("add {}/config/hyprland/altium.lua to ~/.config/hypr/hyprland.lua (set altium_on_linux to that repo path)", fixes_dir().display()),
        commands: vec![],
    });
    v.push(Step {
        id: "sign-in",
        title: "Sign in and get a license",
        done: false,
        who: "user",
        detail: "start Altium, open License Management, click Sign In; finish in the browser. ace cannot detect this; `ace designer ping` fails while a license dialog is open.".into(),
        commands: vec![say("ace designer start")],
    });
    v.push(Step {
        id: "bridge",
        title: "Install the altium-cli-extended bridge script",
        done: inst.is_some_and(|i| crate::bridge::Bridge::new(&crate::wine::WineHost::new(i.clone())).installed()),
        who: "auto",
        detail: "C:\\altium-cli-extended\\bridge".into(),
        commands: vec![say("ace designer install-bridge")],
    });
    v
}

fn joined(cmds: &[Cmd]) -> String {
    cmds.iter().map(Cmd::display).collect::<Vec<_>>().join(" && ")
}

/// Run an `auto` step's commands. Returns each command with its exit status.
pub fn apply(cfg: &Config, id: &str) -> Result<Vec<(String, i32)>> {
    if id == "runner" && crate::ops::host_choice(cfg) == "local" {
        crate::local::LocalHost::new(cfg)?.install_runner()?;
        return Ok(vec![("register scheduled task altium-cli-extended-run locally".into(), 0)]);
    }
    if id == "runner" && crate::ops::host_choice(cfg) == "ssh" {
        let sc = crate::ssh::SshConfig::from(cfg).ok_or_else(|| Error::usage("no ssh_host configured"))?;
        crate::ssh::SshHost::connect(sc)?.install_runner()?;
        return Ok(vec![("register scheduled task altium-cli-extended-run over SSH".into(), 0)]);
    }
    let all = steps(cfg);
    let Some(step) = all.iter().find(|s| s.id == id) else {
        return Err(Error::not_found(format!("no setup step {id:?}"))
            .hint(format!("steps: {}", all.iter().map(|s| s.id).collect::<Vec<_>>().join(", "))));
    };
    if step.who != "auto" && (step.id != "installer" || step.commands.is_empty()) {
        return Err(Error::new(ErrorCode::Unsafe, format!("step {id} needs you: {}", step.detail))
            .hint(if step.commands.is_empty() { "follow the detail text".to_owned() } else { format!("run: {}", joined(&step.commands)) }));
    }
    let mut argvs = vec![];
    for c in &step.commands {
        match c {
            Cmd::Run(argv) => argvs.push((c.display(), argv)),
            Cmd::Say(_) => {
                return Err(Error::usage(format!("`designer setup --apply` does not run step {id}")).hint(format!("run: {}", joined(&step.commands))));
            }
        }
    }
    let mut out = vec![];
    for (c, argv) in argvs {
        let st = Command::new(&argv[0]).args(&argv[1..]).status()?;
        let code = st.code().unwrap_or(-1);
        out.push((c.clone(), code));
        if code != 0 {
            return Err(Error::new(ErrorCode::Io, format!("`{c}` failed with exit code {code}"))
                .details(serde_json::json!({"ran": out})));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unzip_gets_the_zip_name_as_one_argument() {
        let zip = Path::new("/home/u/Downloads/OfflineSetup_Altium_Designer_Public_25';touch ${HOME}PWNED;'.zip");
        let Cmd::Run(argv) = unzip_cmd(zip) else { panic!("unzip must be runnable") };
        let dir = "/home/u/Downloads/OfflineSetup_Altium_Designer_Public_25';touch ${HOME}PWNED;'";
        assert_eq!(argv, ["unzip", "-q", "-o", zip.to_str().unwrap(), "-d", dir].map(OsString::from));
    }

    #[test]
    fn bottle_names_stay_one_argument() {
        let Cmd::Run(argv) = bottles_cli("edit", "My Bottle;id", &[&"--env-var", &"SteamAppId=altium"]) else { panic!() };
        assert_eq!(argv, ["flatpak", "run", "--command=bottles-cli", BOTTLES_APP, "edit", "-b", "My Bottle;id", "--env-var", "SteamAppId=altium"].map(OsString::from));
    }

    #[cfg(unix)]
    #[test]
    fn shown_commands_paste_into_a_shell_unchanged() {
        let hostile = ["25';echo INJECTED;'", "a b", "$(echo INJECTED)", "`echo INJECTED`", "line\nbreak", "-rf", "100%", "it's", "", "${HOME}", "back\\slash", "plain-arg_1.zip"];
        let mut argv: Vec<OsString> = vec!["printf".into(), "%s\\0".into()];
        argv.extend(hostile.iter().map(OsString::from));
        let shown = Cmd::Run(argv).display();
        let out = Command::new("sh").arg("-c").arg(&shown).output().unwrap();
        let expected: Vec<u8> = hostile.iter().flat_map(|a| [a.as_bytes(), b"\0"].concat()).collect();
        assert_eq!(out.stdout, expected, "{shown}");
        assert!(shown.ends_with(" plain-arg_1.zip"));
    }

    #[test]
    fn apply_refuses_a_step_it_cannot_run() {
        let cfg = Config { host: Some("ssh".into()), ..Config::default() };
        let e = apply(&cfg, "bridge").unwrap_err();
        assert_eq!(e.code, ErrorCode::Usage);
        assert_eq!(e.hint.as_deref(), Some("run: ace designer install-bridge"));
    }
}
