mod common;
use common::*;

#[test]
fn gate_results_survive_shaping_and_default_stays_report_only() {
    let Some(root) = samples() else { return };
    let bad = s(&root.join("BeagleBoneCape/BeagleBoneCape.PrjPcb"));
    let good = s(&root.join("PilotPi_PCB/I2C_Expander/I2C_Expander.PrjPcb"));
    let (exit, v) = run(&["check", &bad, "--fail-on", "error", "--limit", "0"]);
    assert_eq!(exit, 9);
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"]["passed"], false);
    assert!(v["data"]["items"].as_array().unwrap().is_empty());
    assert_eq!(v["data"]["gate"]["matched"], v["data"]["errors"]);
    let counts: u64 = v["data"]["by_code"].as_object().unwrap().values().map(|n| n.as_u64().unwrap()).sum();
    assert_eq!(counts, v["data"]["matched"].as_u64().unwrap());
    assert!(!v["data"]["gate"]["hint"].as_str().unwrap().is_empty());
    ok(&["check", &bad]);
    assert_eq!(ok(&["check", &good, "--fail-on", "error"])["gate"]["matched"], 0);
    let pcb = s(&root.join("BeagleBoneCape/BeagleBoneCape.PcbDoc"));
    let (exit, v) = run(&["diff", &bad, &pcb, "--exit-code", "--limit", "0"]);
    assert_eq!(exit, 9);
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"]["gate"]["matched"], 1);
    assert_eq!(ok(&["diff", &bad, &bad, "--exit-code"])["gate"]["matched"], 0);
}

#[test]
fn error_categories_and_native_fallback_are_discoverable() {
    let catalog = ok(&["errors"]);
    let rows = catalog["items"].as_array().unwrap();
    assert!(rows.iter().all(|r| r["category"].is_string()));
    assert!(rows.iter().any(|r| r["code"] == "GATE_FAILED" && r["exit_code"] == 9));
    assert_eq!(err(&["inspect", "/nonexistent/ace-test.PrjPcb"], "FILE_NOT_FOUND")["error"]["category"], "input");
    let Some(root) = samples() else { return };
    let pcb = s(&root.join("BeagleBoneCape/BeagleBoneCape.PcbDoc"));
    let v = err(&["pcb", "set-component", &pcb, "P8", "footprint", "X", "--dry-run"], "UNSUPPORTED_NATIVE");
    assert!(v["error"]["hint"].as_str().unwrap().contains("ace designer"));
}
