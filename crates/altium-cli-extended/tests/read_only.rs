mod common;
use common::*;
use std::process::Command;

#[test]
fn empty_read_only_env_is_unset_and_other_values_stay_strict() {
    for (value, expected) in [("", false), ("0", false), ("false", false), ("1", true), ("yes", true)] {
        let output = Command::new(env!("CARGO_BIN_EXE_ace"))
            .args(["commands", "--brief", "--json"]).env("ACE_READONLY", value).output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(0), "ACE_READONLY={value:?}: {v}");
        assert_eq!(v["meta"]["read_only"], expected);
    }
    let output = Command::new(env!("CARGO_BIN_EXE_ace"))
        .args(["--read-only", "commands", "--brief", "--json"]).env("ACE_READONLY", "").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{v}");
    assert_eq!(v["meta"]["read_only"], true);
    let output = Command::new(env!("CARGO_BIN_EXE_ace"))
        .args(["commands", "--brief", "--json"]).env("ACE_READONLY", "banana").output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(2), "{v}");
    assert_eq!(v["error"]["code"], "USAGE");
}

#[test]
fn read_only_blocks_before_io_and_dry_runs_are_safe() {
    // Nonexistent files are intentional: UNSAFE proves dispatch precedes all input/host IO.
    for args in [
        vec!["sch", "set-param", "missing.SchDoc", "R1", "Value", "1"],
        vec!["designer", "annotate", "missing.PrjPcb"],
        vec!["designer", "process", "PCB:DeleteObjects"],
        vec!["designer", "script", "missing.pas"],
        vec!["skill", "install"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_ace")).args(&args).env("ACE_READONLY", "1").output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(8), "{args:?}: {v}");
        assert_eq!(v["error"]["code"], "UNSAFE");
    }
    let d = ok(&["--read-only", "--dry-run", "designer", "annotate", "missing.PrjPcb"]);
    assert_eq!(d["executed"], false);
    let Some(root) = samples() else { return };
    let dir = tempdir("read-only");
    let file = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.SchDoc"), &dir);
    let f = s(&file);
    let before = std::fs::read(&file).unwrap();
    err(&["--read-only", "sch", "set-param", &f, "P8", "Value", "1"], "UNSAFE");
    let (exit, v) = run(&["--read-only", "--dry-run", "sch", "set-param", &f, "P8", "Value", "1"]);
    assert_eq!(exit, 0);
    assert_eq!(v["meta"]["read_only"], true);
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert!(!dir.join("BeagleBoneCape.SchDoc.bak").exists());
}
