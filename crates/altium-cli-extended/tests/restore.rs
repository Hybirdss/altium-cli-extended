mod common;
use common::*;

#[test]
fn first_backup_survives_many_writes_and_restores_exact_bytes() {
    let Some(root) = samples() else { return };
    let dir = tempdir("restore");
    let file = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.SchDoc"), &dir);
    let f = s(&file);
    let pristine = std::fs::read(&file).unwrap();
    for v in ["one", "two", "three"] { ok(&["sch", "set-param", &f, "P8", "QA", v]); }
    assert_eq!(std::fs::read(dir.join("BeagleBoneCape.SchDoc.bak")).unwrap(), pristine);
    let edited = std::fs::read(&file).unwrap();
    let recovery = dir.join("BeagleBoneCape.SchDoc.pre-restore");
    let d = ok(&["restore", &f, "--from", "first", "--dry-run"]);
    assert_eq!(d["verified"]["reparsed"], true);
    assert_eq!(d["verified"]["read_back"], false);
    assert_eq!(d["pre_restore"], s(&recovery));
    assert_eq!(d["current"]["bytes"], edited.len());
    assert_eq!(d["restore_source"]["bytes"], pristine.len());
    assert!(d["current"]["modified_unix_ms"].is_number());
    assert!(d["restore_source"]["modified_unix_ms"].is_number());
    assert!(!recovery.exists());
    assert_eq!(std::fs::read(&file).unwrap(), edited);
    assert_eq!(ok(&["restore", &f])["verified"]["read_back"], true);
    assert_eq!(std::fs::read(&file).unwrap(), pristine);
    assert_eq!(std::fs::read(&recovery).unwrap(), edited);
    assert_eq!(std::fs::read(dir.join("BeagleBoneCape.SchDoc.bak")).unwrap(), pristine);
    assert_eq!(ok(&["restore", &f])["changed"], false);
    assert_eq!(std::fs::read(&recovery).unwrap(), edited);
    std::fs::write(dir.join("BeagleBoneCape.SchDoc.bak"), b"invalid").unwrap();
    err(&["restore", &f], "UNSAFE");
    assert_eq!(std::fs::read(&file).unwrap(), pristine);
}

#[test]
fn restore_preserves_current_bytes_even_without_backups_and_refuses_to_overwrite_recovery() {
    let dir = tempdir("restore-recovery");
    let file = dir.join("Board.PrjPcb");
    let backup = dir.join("Board.PrjPcb.bak");
    let recovery = dir.join("Board.PrjPcb.pre-restore");
    let pristine = b"[Design]\nName=Old\n";
    let edited = b"[Design]\nName=Current\n";
    std::fs::write(&file, edited).unwrap();
    std::fs::write(&backup, pristine).unwrap();
    let f = s(&file);
    let d = ok(&["restore", &f, "--no-backup"]);
    assert_eq!(d["pre_restore"], s(&recovery));
    assert_eq!(std::fs::read(&recovery).unwrap(), edited);
    assert_eq!(std::fs::read(&file).unwrap(), pristine);
    assert_eq!(std::fs::read(&backup).unwrap(), pristine);
    std::fs::write(&file, b"[Design]\nName=Later\n").unwrap();
    let e = err(&["restore", &f], "UNSAFE");
    assert!(e["error"]["hint"].as_str().unwrap().contains("pre-restore"));
    assert_eq!(std::fs::read(&file).unwrap(), b"[Design]\nName=Later\n");
    assert_eq!(std::fs::read(&recovery).unwrap(), edited);
    assert_eq!(std::fs::read(&backup).unwrap(), pristine);
}
