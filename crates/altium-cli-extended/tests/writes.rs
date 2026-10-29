//! Native write commands on copies of the sample files: dry run, backup, read-back.

mod common;

use common::*;

#[test]
fn pcb_rule_class_param_component_edits() {
    let Some(root) = samples() else { return };
    let dir = tempdir("pcb-writes");
    let pcb = copy_to(&root.join("AirPi_PCB/AirPi_Power/AirPi_Power.PcbDoc"), &dir);
    let f = s(&pcb);
    let original = std::fs::read(&pcb).unwrap();

    // Dry run changes nothing.
    let d = ok(&["pcb", "set-rule", &f, "Clearance", "gap", "0.2mm", "--dry-run"]);
    assert_eq!(d["changed"], true);
    assert_eq!(std::fs::read(&pcb).unwrap(), original);

    let d = ok(&["pcb", "set-rule", &f, "Clearance", "gap", "8mil"]);
    assert_eq!(d["changes"][0]["before"], "6mil");
    assert_eq!(d["changes"][0]["after"], "8mil");
    assert!(dir.join("AirPi_Power.PcbDoc.bak").exists());
    let r = ok(&["pcb", "rule", &f, "Clearance", "--units", "mil"]);
    assert_eq!(r["values"]["gap"], 8.0);
    err(&["pcb", "set-rule", &f, "Clearance", "gap", "8"], "USAGE");
    err(&["pcb", "set-rule", &f, "Clearance", "nosuchfield", "1"], "NOT_FOUND");
    ok(&["pcb", "set-rule", &f, "Clearance", "enabled", "false"]);
    assert_eq!(ok(&["pcb", "rule", &f, "Clearance"])["enabled"], false);

    ok(&["pcb", "add-class", &f, "PWR", "5V", "GND"]);
    ok(&["pcb", "class-add", &f, "PWR", "5V_ALT"]);
    ok(&["pcb", "class-remove", &f, "PWR", "GND"]);
    let c = ok(&["pcb", "classes", &f, "--where", "name=PWR"]);
    assert_eq!(c["items"][0]["members"], serde_json::json!(["5V", "5V_ALT"]));
    err(&["pcb", "class-add", &f, "PWR", "NoSuchNet"], "NOT_FOUND");
    err(&["pcb", "class-add", &f, "All Nets", "5V"], "UNSAFE");

    ok(&["pcb", "set-param", &f, "U2", "MPN", "LM2596SX-5.0"]);
    ok(&["pcb", "set-param", &f, "U2", "Supplier", "Mouser"]);
    ok(&["pcb", "remove-param", &f, "U2", "Notepad"]);
    let u2 = ok(&["pcb", "component", &f, "U2"]);
    assert_eq!(u2["component"]["parameters"]["MPN"], "LM2596SX-5.0");
    assert_eq!(u2["component"]["parameters"]["Supplier"], "Mouser");
    assert!(u2["component"]["parameters"].get("Notepad").is_none());

    ok(&["pcb", "set-component", &f, "U2", "kind", "Standard (No BOM)"]);
    ok(&["pcb", "set-component", &f, "U2", "description", "buck regulator"]);
    let u2 = ok(&["pcb", "component", &f, "U2"]);
    assert_eq!(u2["component"]["kind"], "Standard (No BOM)");
    assert_eq!(u2["component"]["description"], "buck regulator");
    err(&["pcb", "set-component", &f, "U2", "x", "3mm"], "USAGE");

    // Everything else on the board is as it was.
    let a = ok(&["pcb", "stats", &s(&root.join("AirPi_PCB/AirPi_Power/AirPi_Power.PcbDoc"))]);
    let b = ok(&["pcb", "stats", &f]);
    assert_eq!(a["totals"], b["totals"]);
    let (_, v) = run(&["pcb", "info", &f]);
    assert_eq!(v["warnings"], serde_json::json!([]));
}

#[test]
fn project_variants_and_bom() {
    let Some(root) = samples() else { return };
    let dir = tempdir("variants");
    for e in ["PrjPcb", "SchDoc", "PcbDoc"] {
        copy_to(&root.join(format!("BeagleBoneCape/BeagleBoneCape.{e}")), &dir);
    }
    let prj = dir.join("BeagleBoneCape.PrjPcb");
    let p = s(&prj);
    let original = std::fs::read(&prj).unwrap();

    // Untouched sections keep their bytes: a set + remove round trip restores the file.
    ok(&["project", "set-parameter", &p, "Revision", "B"]);
    ok(&["project", "remove-parameter", &p, "Revision"]);
    assert_eq!(std::fs::read(&prj).unwrap(), original);

    ok(&["project", "add-variant", &p, "Lite"]);
    err(&["project", "add-variant", &p, "lite"], "UNSAFE");
    let d = ok(&["project", "set-variation", &p, "Lite", "P9"]);
    assert_eq!(d["unique_id_source"], "pcb");
    ok(&["project", "set-variant-param", &p, "Lite", "P8", "Comment", "HDR"]);
    let v = ok(&["project", "variant", &p, "Lite"]);
    assert_eq!(v["variations"][0]["designator"], "P9");
    assert_eq!(v["variations"][0]["kind"], "not_fitted");
    assert_eq!(v["param_variations"][0]["Designator"], "P8");

    let b = ok(&["bom", &p, "--variant", "Lite"]);
    assert_eq!(b["dnp"], serde_json::json!(["P9"]));
    assert_eq!(b["varied"], serde_json::json!(["P8"]));
    assert!(b["items"].as_array().unwrap().iter().any(|l| l["comment"] == "HDR"));
    assert!(!b["items"].as_array().unwrap().iter().any(|l| l["designator_ranges"] == "P9"));
    let b = ok(&["bom", &s(&dir.join("BeagleBoneCape.PcbDoc")), "--project", &p, "--variant", "1", "--include-dnp"]);
    assert!(b["items"].as_array().unwrap().iter().any(|l| l["designator_ranges"] == "P9" && l["dnp"] == true));

    ok(&["project", "set-variation", &p, "Lite", "P9", "--alternate", "BBB_P9_ALT"]);
    let b = ok(&["bom", &p, "--variant", "Lite"]);
    assert!(b["items"].as_array().unwrap().iter().any(|l| l["lib_ref"] == "BBB_P9_ALT"));
    let (_, env) = run(&["bom", &p, "--variant", "Lite"]);
    assert!(env["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("alternate")));

    let d = ok(&["project", "clear-variation", &p, "Lite", "P8"]);
    assert_eq!(d["removed"], 2);
    err(&["project", "set-variation", &p, "Lite", "Q99"], "NOT_FOUND");
    ok(&["project", "remove-variant", &p, "Lite"]);
    assert_eq!(ok(&["project", "variants", &p])["count"], 0);
}

#[test]
fn schlib_edits() {
    let Some(root) = samples() else { return };
    let dir = tempdir("schlib");
    let lib = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.SchLib"), &dir);
    let l = s(&lib);
    let d = ok(&["schlib", "set-param", &l, "CAT24C256", "MPN", "CAT24C256WI-GT3"]);
    assert_eq!(d["changes"][0]["action"], "create");
    ok(&["schlib", "set-param", &l, "CAT24C256", "Comment", "EEPROM"]);
    ok(&["schlib", "set-description", &l, "CAT24C256", "256 kbit I2C EEPROM"]);
    let s1 = ok(&["schlib", "show", &l, "CAT24C256"]);
    assert_eq!(s1["description"], "256 kbit I2C EEPROM");
    let params = s1["parameters"].as_array().unwrap();
    assert!(params.iter().any(|p| p["name"] == "MPN" && p["value"] == "CAT24C256WI-GT3" && p["hidden"] == true));
    assert!(params.iter().any(|p| p["name"] == "Comment" && p["value"] == "EEPROM"));
    let d = ok(&["schlib", "list", &l, "--where", "name=CAT24C256"]);
    assert_eq!(d["items"][0]["description"], "256 kbit I2C EEPROM");
    // The other symbols are untouched.
    let a = ok(&["schlib", "show", &s(&root.join("BeagleBoneCape/BeagleBoneCape.SchLib")), "BBB_P9"]);
    let b = ok(&["schlib", "show", &l, "BBB_P9"]);
    assert_eq!(a, b);
    let same = ok(&["schlib", "set-param", &l, "CAT24C256", "MPN", "CAT24C256WI-GT3"]);
    assert_eq!(same["changed"], false);
}

#[test]
fn codepage_hint() {
    let Some(root) = samples() else { return };
    let f = s(&root.join("PilotPi_PCB/I2C_Expander/I2C_Expander.SchDoc"));
    let (_, v) = run(&["sch", "components", &f]);
    assert!(v["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("--codepage gbk")), "{v}");
    let (_, v) = run(&["sch", "components", &f, "--codepage", "gbk"]);
    assert_eq!(v["warnings"], serde_json::json!([]));
}

/// Project/OutJob text edits rely on untouched lines surviving a parse + write unchanged.
#[test]
fn ini_files_roundtrip() {
    let Some(root) = samples() else { return };
    let mut n = 0;
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if !p.ends_with(".git") {
                    stack.push(p);
                }
                continue;
            }
            let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if ext == "prjpcb" || ext == "outjob" {
                let b = std::fs::read(&p).unwrap();
                assert!(altium_format::ini::Ini::parse(&b).to_bytes() == b, "{} does not round-trip", p.display());
                n += 1;
            }
        }
    }
    assert!(n >= 7);
}
