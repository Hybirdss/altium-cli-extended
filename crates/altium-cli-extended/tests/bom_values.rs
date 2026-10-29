mod common;
use common::*;
#[test]
fn bom_values_and_csv_reference_names_are_useful_by_default() {
    let Some(root) = samples() else { return };
    let dir = tempdir("bom-values");
    let prj = s(&root.join("BeagleBoneCape/BeagleBoneCape.PrjPcb"));
    let csv = s(&dir.join("parts.csv"));
    let (exit, v) = run(&["bom", &prj, "--group-by", "designator", "--columns", "designator,value,footprint", "--out", &csv]);
    assert_eq!(exit, 0);
    let text = std::fs::read_to_string(&csv).unwrap();
    for value in ["BBB_P8", "BBB_P9", "CAT24C256"] { assert!(text.contains(value), "{text}"); }
    assert!(v["data"]["items"].as_array().unwrap().iter().all(|r| !r["value"].as_str().unwrap().is_empty()));
    assert!(v["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("using comment")));
    assert_eq!(ok(&["bom", &prj, "--group-by", "designator", "--out", &csv])["total_parts"], 3);
    assert!(std::fs::read_to_string(csv).unwrap().starts_with("line,quantity,designator_ranges,"));
}
