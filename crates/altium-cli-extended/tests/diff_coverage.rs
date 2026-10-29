mod common;
use common::*;

#[test]
fn annotation_rename_keeps_simultaneous_parameter_changes() {
    let Some(root) = samples() else { return };
    let dir = tempdir("identity-diff");
    let before = root.join("BeagleBoneCape/BeagleBoneCape.SchDoc");
    let after = copy_to(&before, &dir);
    let (a, b) = (s(&before), s(&after));
    ok(&["sch", "set-designator", &b, "U?", "U1"]);
    ok(&["sch", "set-param", &b, "U1", "MPN", "CAT24C256WI-GT3"]);
    let d = ok(&["diff", &a, &b]);
    assert_eq!(d["summary"]["parts_added"], 0);
    assert_eq!(d["summary"]["parts_removed"], 0);
    assert_eq!(d["summary"]["parts_renamed"], 1);
    let items = d["items"].as_array().unwrap();
    assert!(items.iter().any(|r| r["change"] == "part_renamed" && r["ref"] == "U1" && r["before_ref"] == "U?"));
    assert!(items.iter().any(|r| r["change"] == "part_changed" && r["ref"] == "U1" && r["field"] == "param:MPN"));
    assert_eq!(d["summary"]["nets_changed"], 0, "annotation alone must not look like a rewire");
    let old = ok(&["diff", &a, &b, "--match", "designator"]);
    assert_eq!(old["summary"]["parts_added"], 1);
    assert_eq!(old["summary"]["parts_removed"], 1);
}

#[test]
fn project_variant_and_library_edits_are_visible() {
    let Some(root) = samples() else { return };
    let dir = tempdir("artifact-diff");
    let base = root.join("BeagleBoneCape");
    for name in ["BeagleBoneCape.PrjPcb", "BeagleBoneCape.SchDoc", "BeagleBoneCape.PcbDoc"] { copy_to(&base.join(name), &dir); }
    let before = s(&base.join("BeagleBoneCape.PrjPcb"));
    let after = s(&dir.join("BeagleBoneCape.PrjPcb"));
    ok(&["project", "add-variant", &after, "Lite"]);
    let d = ok(&["diff", &before, &after]);
    assert!(d["items"].as_array().unwrap().iter().any(|r| r["change"] == "variant_added" && r["ref"] == "Lite"));
    assert_eq!(d["summary"]["project_setting_changed"].as_u64().unwrap_or(0), 0);
    let variant_before = dir.join("Before.PrjPcb");
    std::fs::copy(&after, &variant_before).unwrap();
    ok(&["project", "set-variation", &after, "Lite", "P9"]);
    let full = ok(&["diff", &before, &after]);
    assert!(full["items"].as_array().unwrap().iter().any(|r| r["change"] == "variation_changed" && r["ref"] == "P9" && r["before"].is_null()));
    assert_eq!(full["summary"]["project_setting_changed"].as_u64().unwrap_or(0), 0);
    let d = ok(&["diff", &s(&variant_before), &after]);
    assert!(d["items"].as_array().unwrap().iter().any(|r| r["change"] == "variation_changed" && r["ref"] == "P9"));
    assert_eq!(d["summary"]["project_setting_changed"].as_u64().unwrap_or(0), 0);
    let lib = base.join("BeagleBoneCape.PcbLib");
    let edited = s(&copy_to(&lib, &dir));
    ok(&["pcblib", "set-param", &edited, "SOIC-8", "HEIGHT", "2.5mm"]);
    let d = ok(&["diff", &s(&lib), &edited]);
    assert_eq!(d["identical"], false);
    assert!(d["items"].as_array().unwrap().iter().any(|r| r["change"] == "param_changed" && r["ref"] == "SOIC-8" && r["field"] == "height"));
    let lib = base.join("BeagleBoneCape.SchLib");
    let edited = s(&copy_to(&lib, &dir));
    ok(&["schlib", "set-param", &edited, "CAT24C256", "MPN", "Changed"]);
    let d = ok(&["diff", &s(&lib), &edited]);
    assert_eq!(d["identical"], false);
    assert!(d["items"].as_array().unwrap().iter().any(|r| r["change"] == "param_changed" && r["ref"] == "CAT24C256"));
}

#[test]
fn diff_views_group_fields_preserve_gates_and_keep_rule_types() {
    let Some(root) = samples() else { return };
    let dir = tempdir("diff-views");
    let before = root.join("BeagleBoneCape/BeagleBoneCape.SchDoc");
    let after = copy_to(&before, &dir);
    let (a, b) = (s(&before), s(&after));
    ok(&["sch", "set-param", &b, "P8", "MPN", "new"]);
    ok(&["sch", "set-param", &b, "P8", "Tolerance", "1%"]);
    let d = ok(&["diff", &a, &b, "--by", "part"]);
    let row = d["items"].as_array().unwrap().iter().find(|r| r["ref"] == "P8" && r["change"] == "part_changed").unwrap();
    assert_eq!(row["fields"].as_array().unwrap().len(), 2);
    let (exit, v) = run(&["diff", &a, &b, "--summary", "--exit-code"]);
    assert_eq!(exit, 9);
    assert!(v["data"].get("items").is_none());
    assert_eq!(v["data"]["summary"]["by_field"]["param:MPN"], 1);
    let d = ok(&["diff", &a, &b, "--ignore-field", "^param:", "--exit-code"]);
    assert_eq!(d["identical"], true);
    let d = ok(&["diff", &a, &b, "--only", "mpn"]);
    assert_eq!(d["items"].as_array().unwrap().len(), 1);
    err(&["diff", &a, &b, "--ignore-field", "["], "USAGE");
    let before = root.join("BeagleBoneCape/BeagleBoneCape.PcbDoc");
    let after = copy_to(&before, &dir);
    ok(&["pcb", "set-rule", &s(&after), "Clearance", "gap", "0.2mm"]);
    let d = ok(&["diff", &s(&before), &s(&after)]);
    let gap = d["items"].as_array().unwrap().iter().find(|r| r["field"] == "gap").unwrap();
    assert!(gap["before"].is_number());
    assert!(gap["after"].is_number());
    assert_eq!(gap["unit"], "mm");
}
