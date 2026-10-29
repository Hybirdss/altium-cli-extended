mod common;
use common::*;
#[test]
fn baseline_gates_new_findings_and_rejects_partial_reports() {
    let Some(root) = samples() else { return };
    let dir = tempdir("check-baseline");
    let file = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.SchDoc"), &dir);
    let f = s(&file);
    let baseline = s(&dir.join("before.json"));
    let (_, before) = run(&["check", &f]);
    std::fs::write(&baseline, serde_json::to_vec(&before).unwrap()).unwrap();
    let d = ok(&["check", &f, "--baseline", &baseline, "--fail-on", "error"]);
    assert_eq!(d["new"], serde_json::json!([]));
    assert_eq!(d["passed_vs_baseline"], true);
    assert_eq!(d["gate"]["matched"], 0);
    ok(&["sch", "set-designator", &f, "U?", "U1"]);
    let d = ok(&["check", &f, "--baseline", &baseline, "--fail-on", "error"]);
    assert!(d["resolved"].as_array().unwrap().iter().any(|r| r["code"] == "unannotated"));
    ok(&["sch", "set-designator", &f, "P8", "P?"]);
    let (exit, d) = run(&["check", &f, "--baseline", &baseline, "--fail-on", "error"]);
    assert_eq!(exit, 9);
    assert_eq!(d["data"]["passed_vs_baseline"], false);
    std::fs::write(&baseline, b"{}").unwrap();
    err(&["check", &f, "--baseline", &baseline], "USAGE");
    let (_, partial) = run(&["check", &f, "--limit", "0"]);
    std::fs::write(&baseline, serde_json::to_vec(&partial).unwrap()).unwrap();
    err(&["check", &f, "--baseline", &baseline], "USAGE");
}
