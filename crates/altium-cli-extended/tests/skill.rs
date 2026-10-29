//! The generated skill: committed copy is current, install/status/uninstall work.

mod common;

use std::path::Path;
use std::process::Command;

use common::*;

fn generated(file: &str) -> String {
    let d = ok(&["skill", "--file", file]);
    d["markdown"].as_str().unwrap().to_owned()
}

/// `skills/altium-cli-extended/*.md` in the repo must match the binary. Regenerate with
/// `ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill`.
#[test]
fn committed_skill_is_current() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/altium-cli-extended");
    for (file, name) in [("skill", "SKILL.md"), ("commands", "COMMANDS.md")] {
        let want = generated(file);
        let path = dir.join(name);
        if std::env::var_os("ACE_UPDATE_SKILL").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &want).unwrap();
            continue;
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        assert!(have == want, "{} is stale: run ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill", path.display());
    }
}

#[test]
fn skill_is_concise_and_english() {
    let md = generated("skill");
    assert!(md.starts_with("---\nname: altium-cli-extended\ndescription: "));
    assert!(md.len() < 26_000, "SKILL.md should stay short ({} bytes)", md.len());
    assert!(md.is_ascii() || !md.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c)), "English only");
    let cmds = generated("commands");
    for c in ["### `ace bom`", "### `ace pcb set-rule`", "### `ace skill install`", "## Errors"] {
        assert!(cmds.contains(c), "COMMANDS.md lacks {c}");
    }
}

fn run_home(home: &Path, args: &[&str]) -> serde_json::Value {
    let o = Command::new(env!("CARGO_BIN_EXE_ace"))
        .args(args)
        .arg("--json")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["ok"], true, "{args:?}: {v}");
    v
}

#[test]
fn install_status_uninstall() {
    let home = tempdir("skill-home");
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    // Default targets: .agents plus agents whose home exists (.claude here, not .codex).
    let v = run_home(&home, &["skill", "install", "--dry-run"]);
    assert_eq!(v["data"]["count"], 2);
    assert!(!home.join(".agents/skills/altium-cli-extended/SKILL.md").exists());
    let v = run_home(&home, &["skill", "install"]);
    let targets: Vec<&str> = v["data"]["items"].as_array().unwrap().iter().map(|i| i["target"].as_str().unwrap()).collect();
    assert_eq!(targets, ["agents", "claude"]);
    for d in [".agents", ".claude"] {
        assert_eq!(std::fs::read_to_string(home.join(d).join("skills/altium-cli-extended/SKILL.md")).unwrap(), generated("skill"));
        assert!(home.join(d).join("skills/altium-cli-extended/COMMANDS.md").is_file());
    }
    let v = run_home(&home, &["skill", "install", "--target", "codex"]);
    assert_eq!(v["data"]["items"][0]["written"].as_array().unwrap().len(), 2);
    let v = run_home(&home, &["skill", "install", "--target", "codex"]);
    assert_eq!(v["data"]["items"][0]["unchanged"].as_array().unwrap().len(), 2);

    // Status sees a stale copy.
    std::fs::write(home.join(".claude/skills/altium-cli-extended/SKILL.md"), "old").unwrap();
    let v = run_home(&home, &["skill", "status"]);
    let claude = v["data"]["items"].as_array().unwrap().iter().find(|i| i["target"] == "claude").unwrap().clone();
    assert_eq!(claude["installed"], true);
    assert_eq!(claude["up_to_date"], false);
    assert!(!v["warnings"].as_array().unwrap().is_empty());

    // --dir installs into DIR/altium-cli-extended.
    let proj = home.join("proj/.claude/skills");
    run_home(&home, &["skill", "install", "--dir", &s(&proj)]);
    assert!(proj.join("altium-cli-extended/SKILL.md").is_file());

    // Uninstall keeps foreign files.
    std::fs::write(home.join(".codex/skills/altium-cli-extended/notes.txt"), "mine").unwrap();
    run_home(&home, &["skill", "uninstall", "--target", "all"]);
    assert!(!home.join(".agents/skills/altium-cli-extended").exists());
    assert!(!home.join(".claude/skills/altium-cli-extended").exists());
    assert!(home.join(".codex/skills/altium-cli-extended/notes.txt").exists());
    assert!(!home.join(".codex/skills/altium-cli-extended/SKILL.md").exists());
}

#[test]
fn migration_intake_names_the_target_and_requires_readback() {
    for md in [generated("skill"), include_str!("../../../README.md").to_owned()] {
        let import = md.lines().find(|line| line.contains(" pcb import ")).unwrap();
        assert!(import.contains("--output /tmp/target/target.kicad_pcb"));
        assert!(md.contains("kce --json --project /tmp/target pcb info"));
        assert!(md.contains("file additions and deletions"));
        assert!(md.contains("not a\nverified migration"));
    }
}

#[test]
fn scenario_recipes_teach_supported_chains_and_limits() {
    let md = generated("skill");
    assert_eq!(md.lines().filter(|line| line.starts_with("### Recipe ")).count(), 12);
    for recipe in md.split("### Recipe ").skip(1) {
        let title = recipe.lines().next().unwrap();
        for part in ["Trigger:", "```bash", "Verify:"] {
            assert!(recipe.contains(part), "recipe {title} lacks {part}");
        }
    }
    for text in ["ACE_READONLY=1", "absolute paths", "--source pcb", "--baseline", "--codepage", "not verified:", "=Value", "destination first", "migration blocked", "no project checkpoint"] {
        assert!(md.contains(text), "skill lacks {text}");
    }
}
