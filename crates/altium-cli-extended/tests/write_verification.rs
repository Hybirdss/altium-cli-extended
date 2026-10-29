mod common;
use common::*;
#[test]
fn rule_previews_report_typed_units_and_candidate_verification() {
    let Some(root) = samples() else { return };
    let dir = tempdir("write-verification");
    let file = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.PcbDoc"), &dir);
    let f = s(&file);
    let original = std::fs::read(&file).unwrap();
    let d = ok(&["pcb", "set-rule", &f, "Clearance", "gap", "0.2mm", "--dry-run"]);
    assert_eq!(d["changes"][0]["after_value"], 0.2);
    assert_eq!(d["changes"][0]["unit"], "mm");
    assert_eq!(d["verification"]["candidate_reparsed"], true);
    assert_eq!(d["verification"]["read_back"], false);
    assert_eq!(d["verification"]["written"], false);
    assert_eq!(std::fs::read(&file).unwrap(), original);
    let d = ok(&["pcb", "set-rule", &f, "Clearance", "gap", "0.2mm", "--units", "mil"]);
    assert_eq!(d["changes"][0]["unit"], "mil");
    assert_eq!(d["verification"]["read_back"], true);
    assert!(d["changes"][0]["after_value"].as_f64().unwrap() > 7.8);
}

#[test]
fn variant_previews_do_not_promise_unreserved_ids_and_variations_are_typed() {
    let Some(root) = samples() else { return };
    let dir = tempdir("variant-verification");
    for ext in ["PrjPcb", "SchDoc", "PcbDoc"] { copy_to(&root.join(format!("BeagleBoneCape/BeagleBoneCape.{ext}")), &dir); }
    let file = dir.join("BeagleBoneCape.PrjPcb");
    let f = s(&file);
    let original = std::fs::read(&file).unwrap();
    let d = ok(&["project", "add-variant", &f, "Preview", "--dry-run"]);
    assert!(d["variant"]["unique_id"].is_null());
    assert!(d["variant"]["unique_id_note"].as_str().unwrap().contains("assigned on apply"));
    assert_eq!(d["verification"]["read_back"], false);
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert!(ok(&["project", "add-variant", &f, "Preview"])["variant"]["unique_id"].is_string());
    let d = ok(&["project", "set-variation", &f, "Preview", "P9"]);
    assert!(d["before"].is_null());
    assert_eq!(d["after"]["kind"], "not_fitted");
    assert!(d["raw"]["after"].is_string());
    assert_eq!(d["verification"]["read_back"], true);
}
