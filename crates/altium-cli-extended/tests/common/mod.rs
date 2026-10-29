//! Helpers for CLI tests: run the built `ace` binary and parse its JSON envelope.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// `.fixtures/samples` of the workspace or of an enclosing checkout (git worktrees live
/// inside the main checkout). `None` (test skipped) when absent.
pub fn samples() -> Option<PathBuf> {
    let found = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().map(|a| a.join(".fixtures/samples")).find(|p| p.is_dir());
    if found.is_none() {
        assert!(std::env::var_os("CI").is_none(), "CI requires .fixtures/samples; run scripts/fetch-fixtures.sh");
        eprintln!("SKIPPED fixture test: .fixtures/samples missing; run scripts/fetch-fixtures.sh (synthetic tests still run)");
    }
    found
}

/// Run `altium <args> --json`; returns (exit code, envelope).
pub fn run<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> (i32, Value) {
    run_in(None, args)
}

pub fn run_in<S: AsRef<std::ffi::OsStr>>(dir: Option<&Path>, args: &[S]) -> (i32, Value) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ace"));
    c.args(args).arg("--json");
    if let Some(d) = dir {
        c.current_dir(d);
    }
    let o = c.output().expect("run altium");
    let text = String::from_utf8_lossy(&o.stdout);
    let v: Value = serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("not one JSON envelope ({e}): {text}\nstderr: {}", String::from_utf8_lossy(&o.stderr)));
    (o.status.code().unwrap_or(-1), v)
}

/// Run and require `ok: true`; returns `data`.
pub fn ok<S: AsRef<std::ffi::OsStr> + std::fmt::Debug>(args: &[S]) -> Value {
    let (code, v) = run(args);
    assert_eq!(v["ok"], true, "{args:?} failed: {v}");
    assert_eq!(code, 0);
    v["data"].clone()
}

/// Run and require a failure with this error code; returns the envelope.
pub fn err<S: AsRef<std::ffi::OsStr> + std::fmt::Debug>(args: &[S], code: &str) -> Value {
    let (exit, v) = run(args);
    assert_eq!(v["ok"], false, "{args:?} should fail: {v}");
    assert_eq!(v["error"]["code"], code, "{args:?}: {v}");
    assert_ne!(exit, 0);
    v
}

/// A fresh, empty temporary directory unique to this test.
pub fn tempdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("altium-cli-extended-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Copy a sample file into `dir` (same file name); returns the copy.
pub fn copy_to(src: &Path, dir: &Path) -> PathBuf {
    let dst = dir.join(src.file_name().unwrap());
    std::fs::copy(src, &dst).unwrap();
    dst
}

pub fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
