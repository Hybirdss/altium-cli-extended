mod common;
mod support;
use common::*;

#[test]
fn case_only_rename_and_name_encoding() {
    let dir = tempdir("pcblib-case");
    let file = dir.join("L.PcbLib");
    support::pcblib(&file);
    let f = s(&file);
    let original = std::fs::read(&file).unwrap();
    let e = err(&["pcblib", "rename", &f, "A", "Ω-8", "--codepage", "windows-1252"], "USAGE");
    assert!(e["error"]["hint"].as_str().unwrap().contains("--codepage"));
    assert_eq!(std::fs::read(&file).unwrap(), original);
    ok(&["pcblib", "rename", &f, "A", "a"]);
    assert_eq!(ok(&["pcblib", "info", &f])["names"], serde_json::json!(["a"]));
    let show = ok(&["pcblib", "show", &f, "a"]);
    assert_eq!(show["footprint"]["storage"], "A");
    let no_op = ok(&["pcblib", "rename", &f, "a", "a"]);
    assert_eq!(no_op["changed"], false);
    assert_eq!(no_op["footprints"], 1);
    assert_eq!(std::fs::read(dir.join("L.PcbLib.bak")).unwrap(), original);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn remove_and_parameter_rejections_without_fixtures() {
    let dir = tempdir("pcblib-synthetic");
    let file = dir.join("L.PcbLib");
    support::pcblib(&file);
    let f = s(&file);
    let original = std::fs::read(&file).unwrap();
    for (name, val) in [("HEIGHT", "2"), ("HEIGHT", "nonsensemm"), ("BAD=KEY", "x"), ("DESCRIPTION", "a|b")] {
        let e = err(&["pcblib", "set-param", &f, "A", name, val], "USAGE");
        assert!(e["error"]["hint"].is_string(), "{e}");
    }
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert!(!dir.join("L.PcbLib.bak").exists());
    ok(&["pcblib", "set-param", &f, "A", "DESCRIPTION", "New", "--no-backup"]);
    let no_op = ok(&["pcblib", "set-param", &f, "A", "DESCRIPTION", "New"]);
    assert_eq!(no_op["footprints"], 1);
    let d = ok(&["pcblib", "remove", &f, "A", "--no-backup"]);
    assert_eq!(d["footprints"], 0);
    assert_eq!(ok(&["pcblib", "info", &f])["footprints"], 0);
    assert!(!dir.join("L.PcbLib.bak").exists());
    let mut cf = altium_format::cfbx::Compound::open(&file).unwrap();
    assert!(!cf.exists("A/Data"));
    assert!(cf.read("Library/Models/Data").is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}
