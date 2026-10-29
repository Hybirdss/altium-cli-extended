mod common;
use common::*;
#[test]
fn empty_queries_unknown_fields_and_single_objects_explain_their_results() {
    let Some(root) = samples() else { return };
    let prj = s(&root.join("BeagleBoneCape/BeagleBoneCape.PrjPcb"));
    let sch = s(&root.join("BeagleBoneCape/BeagleBoneCape.SchDoc"));
    let pcb = s(&root.join("BeagleBoneCape/BeagleBoneCape.PcbDoc"));
    let (exit, v) = run(&["netlist", &prj, "--pins", "--where", "designator=U1"]);
    assert_eq!(exit, 0);
    assert_eq!(v["data"]["count"], 0);
    assert!(v["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("no part with designator U1")));
    assert!(v["meta"]["similar"]["designator"].is_array());
    let (_, v) = run(&["sch", "component", &sch, "P8", "--fields", "count"]);
    assert!(v["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("unknown field(s): count")));
    let d = ok(&["pcb", "info", &pcb, "--fields", "file"]);
    assert_eq!(d.as_object().unwrap().len(), 1);
    assert!(d.get("file").is_some());
    let d = ok(&["sch", "components", &sch]);
    assert_eq!(d["count"], d["matched"]);
}

#[test]
fn text_component_shows_pin_tables_and_nested_dimensions() {
    let Some(root) = samples() else { return };
    let sch = s(&root.join("BeagleBoneCape/BeagleBoneCape.SchDoc"));
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_ace"))
        .args(["sch", "component", &sch, "U?", "--text"]).output().unwrap();
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("U? pins:"), "{text}");
    assert!(text.contains("hot_end.x"), "{text}");
    assert!(!text.contains("\"electrical\":"));
}
