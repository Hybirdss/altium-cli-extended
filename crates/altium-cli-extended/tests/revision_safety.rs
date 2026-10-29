mod common;
mod support;
use common::*;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn revision_failures_leave_no_temporary_checkout() {
    let dir = tempdir("rev-cleanup");
    let repo = dir.join("repo"); let tmp = dir.join("tmp");
    std::fs::create_dir(&repo).unwrap(); std::fs::create_dir(&tmp).unwrap();
    git(&repo, &["init", "-q"]);
    let file = repo.join("L.PcbLib"); support::pcblib(&file);
    for rev in ["no-such-revision", "--output=unwanted"] {
        let o = Command::new(env!("CARGO_BIN_EXE_ace")).args(["diff", &s(&file), &format!("--rev={rev}"), "--json"])
            .env("TMPDIR", &tmp).env("TMP", &tmp).env("TEMP", &tmp).output().unwrap();
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        assert_eq!(v["ok"], false, "{v}");
        assert!(v["error"]["hint"].is_string());
        assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "checkout leaked: {v}");
        if rev.starts_with('-') { assert_eq!(v["error"]["code"], "USAGE"); }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn revision_shared_document_paths_stay_in_scratch_tree() {
    let dir = tempdir("rev-paths");
    let repo = dir.join("repo"); let project_dir = repo.join("board"); let tmp = dir.join("tmp");
    std::fs::create_dir_all(project_dir.join("Sub")).unwrap(); std::fs::create_dir(&tmp).unwrap();
    git(&repo, &["init", "-q"]);
    support::sch(&repo.join("shared.SchDoc"), &[]);
    let project = project_dir.join("Board.PrjPcb");
    std::fs::write(&project, b"[Design]\nVersion=1.0\n[Document1]\nDocumentPath=Sub\\..\\..\\shared.SchDoc\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["-c", "user.name=Ace test", "-c", "user.email=ace@example.invalid", "commit", "-qm", "fixture"]);
    let o = Command::new(env!("CARGO_BIN_EXE_ace")).args(["diff", &s(&project), "--rev=HEAD", "--json"])
        .env("TMPDIR", &tmp).env("TMP", &tmp).env("TEMP", &tmp).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["data"]["identical"], true, "{v}");
    assert_eq!(v["data"]["incomplete"], false, "{v}");
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0);
    assert!(!dir.join("shared.SchDoc").exists());
    std::fs::remove_dir_all(dir).unwrap();
}
