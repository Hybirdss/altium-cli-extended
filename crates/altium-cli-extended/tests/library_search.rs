mod common;
use common::*;
#[test]
fn search_across_libraries_returns_pad_data_and_reports_skipped_inputs() {
    let Some(root) = samples() else { return };
    let lib = root.join("BeagleBoneCape/BeagleBoneCape.PcbLib");
    let d = ok(&["pcblib", "find", "soic", &s(&lib), "--pads"]);
    assert_eq!(d["libraries_scanned"], 1);
    assert!(d["items"].as_array().unwrap().iter().any(|r| r["name"] == "SOIC-8" && r["pad_items"].as_array().unwrap().len() == 8));
    let dir = tempdir("find-libraries");
    copy_to(&lib, &dir);
    std::fs::write(dir.join("Unopened.IntLib"), b"no native decoder").unwrap();
    let d = ok(&["pcblib", "find", "SOIC", &s(&dir)]);
    assert_eq!(d["incomplete"], true);
    assert_eq!(d["skipped"][0]["reason"], "intlib_not_decoded");
    assert_eq!(d["items"][0]["pad_number_values"].as_array().unwrap().len(), 8);
}
