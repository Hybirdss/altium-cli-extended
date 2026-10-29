mod common;
use common::*;

#[test]
fn linked_comments_require_explicit_literal_and_preview_preserves_bytes() {
    let Some(root) = samples() else { return };
    let dir = tempdir("special-strings");
    let file = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.SchDoc"), &dir);
    let f = s(&file);
    ok(&["sch", "set-comment", &f, "P8", "=Value"]);
    ok(&["sch", "set-param", &f, "P8", "Value", "10k"]);
    let before = std::fs::read(&file).unwrap();
    let v = err(&["sch", "set-comment", &f, "P8", "22k"], "UNSAFE");
    assert!(v["error"]["hint"].as_str().unwrap().contains("P8 Value"));
    err(&["sch", "set-param", &f, "P8", "Comment", "22k"], "UNSAFE");
    let (exit, v) = run(&["sch", "set-comment", &f, "P8", "22k", "--dry-run"]);
    assert_eq!(exit, 0);
    assert_eq!(v["data"]["changes"][0]["special_string"], true);
    assert!(!v["warnings"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read(&file).unwrap(), before);
    ok(&["sch", "set-param", &f, "P8", "Value", "22k"]);
    let linked = ok(&["sch", "component", &f, "P8"]);
    assert_eq!(linked["items"][0]["comment"], "22k");
    assert!(linked["items"][0]["parameters"].as_array().unwrap().iter().any(|p| p["name"] == "Comment" && p["value"] == "=Value"));
    ok(&["sch", "set-comment", &f, "P8", "22k", "--literal"]);
    assert_eq!(ok(&["sch", "component", &f, "P8"])["items"][0]["comment"], "22k");
}
