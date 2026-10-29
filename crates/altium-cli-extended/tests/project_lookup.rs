mod common;
use common::*;
#[test]
fn locate_returns_followup_sheet_references_without_a_sheet_loop() {
    let Some(root) = samples() else { return };
    let p = s(&root.join("quasar_arm_pcb/Quasar ARM.PrjPcb"));
    if !std::path::Path::new(&p).exists() { return; }
    let d = ok(&["sch", "locate", &p, "R20"]);
    assert_eq!(d["count"], 1);
    let item = &d["items"][0];
    assert!(item["sheet"].as_str().unwrap().ends_with("Power block.SchDoc"));
    assert!(item["index"].is_number());
    let sheet = item["sheet"].as_str().unwrap();
    ok(&["sch", "set-param", sheet, "R20", "Value", "22K", "--dry-run"]);
    let e = err(&["sch", "locate", &p, "NOSUCH999"], "NOT_FOUND");
    assert!(!e["error"]["details"]["similar"].as_array().unwrap().is_empty());
}
