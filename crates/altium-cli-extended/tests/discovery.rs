mod common;
use common::*;
use std::io::Read;
use std::process::{Command, Stdio};

#[test]
fn brief_descriptions_keep_whole_words_and_mark_truncation() {
    let full = ok(&["commands"]);
    let brief = ok(&["commands", "--brief"]);
    for row in brief["items"].as_array().unwrap() {
        let original = full["items"].as_array().unwrap().iter().find(|r| r["path"] == row["path"]).unwrap();
        let original = original["about"].as_str().unwrap();
        let excerpt = row["about"].as_str().unwrap();
        assert!(excerpt.chars().count() <= 48, "{row}");
        if original.chars().count() <= 48 {
            assert_eq!(excerpt, original);
        } else {
            let prefix = excerpt.strip_suffix("...").unwrap_or_else(|| panic!("missing ellipsis: {row}"));
            let remainder = original.strip_prefix(prefix).unwrap_or_else(|| panic!("changed description: {row}"));
            assert!(!prefix.is_empty() && !prefix.ends_with(char::is_whitespace), "{row}");
            assert!(remainder.starts_with(char::is_whitespace), "partial word: {row}");
        }
    }
}

#[test]
fn discovery_guides_a_cold_agent() {
    let brief = Command::new(env!("CARGO_BIN_EXE_ace")).args(["commands", "--brief"]).output().unwrap();
    assert!(brief.status.success());
    assert!(brief.stdout.len() < 15_000, "{} bytes", brief.stdout.len());
    let v: serde_json::Value = serde_json::from_slice(&brief.stdout).unwrap();
    assert!(v["data"]["items"].as_array().unwrap().iter().all(|r| r.as_object().unwrap().len() == 2));
    let d = ok(&["describe", "sch", "set-param"]);
    assert!(d["notes"].is_string());
    assert!(d["args"].as_array().unwrap().iter().all(|a| !a["help"].as_str().unwrap().is_empty()));
    assert!(d["flags"].as_array().unwrap().iter().any(|f| f["long"] == "--dry-run"));
    assert_eq!(d["supports_dry_run"], true);
    let group = ok(&["describe", "pcb"]);
    assert!(group["notes"].is_string());
    assert!(group["flags"].is_array());
    assert!(err(&["skill", "sho"], "USAGE")["error"]["hint"].as_str().unwrap().contains("show"));
    for args in [vec!["skill", "show", "--raw"], vec!["skill", "--text"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_ace")).args(args).output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.starts_with(b"---\nname: altium-cli-extended\n"));
    }
    let d = ok(&["doctor", "--quick"]);
    assert_eq!(d["designer"]["state"], "unprobed");
    assert!(d["ready"]["designer"].is_null());
    assert_eq!(ok(&["--version"])["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn early_pipe_close_never_panics() {
    for _ in 0..10 {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ace")).args(["commands"])
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let mut one = [0u8; 1];
        stdout.read_exact(&mut one).unwrap();
        drop(stdout);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}
