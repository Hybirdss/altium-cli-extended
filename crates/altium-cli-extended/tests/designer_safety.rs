use std::process::Command;
use serde_json::Value;

fn invoke(args: &[&str]) -> (bool, Value) {
    // No valid designer configuration: a dry run must finish before connecting or staging.
    let dir = std::env::temp_dir().join(format!("ace-designer-safety-{}", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_ace")).args(args)
        .env("XDG_CONFIG_HOME", &dir).env("APPDATA", &dir).output().unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stdout)));
    (out.status.success(), json)
}

#[test]
fn designer_dry_run_never_connects_or_writes() {
    for args in [
        vec!["designer", "save-all"],
        vec!["designer", "process", "WorkspaceManager:SaveObject"],
        vec!["designer", "script", "missing.pas"],
        vec!["designer", "pull", "missing.PrjPcb"],
        vec!["designer", "call", "save_all"],
        vec!["designer", "export", "missing.PrjPcb", "--kind", "gerber"],
        vec!["designer", "reset"],
    ] {
        let mut args = args;
        args.push("--dry-run");
        let (ok, json) = invoke(&args);
        assert!(ok, "{json}");
        assert_eq!(json["data"]["dry_run"], true, "{json}");
    }
}

#[test]
fn opaque_writes_require_explicit_backup_opt_out() {
    for args in [
        vec!["designer", "process", "WorkspaceManager:SaveObject"],
        vec!["designer", "script", "missing.pas"],
        vec!["designer", "call", "save_all"],
    ] {
        let (ok, json) = invoke(&args);
        assert!(!ok);
        assert_eq!(json["error"]["code"], "UNSAFE");
        assert!(json["error"]["hint"].as_str().unwrap().contains("--no-backup"));
    }
}
