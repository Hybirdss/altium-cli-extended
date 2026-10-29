//! `ace bom|netlist|check|diff` against the sample projects (skipped when absent).

mod common;

use common::*;

#[test]
fn bom_project_and_exports() {
    let Some(root) = samples() else { return };
    let prj = s(&root.join("BeagleBoneCape/BeagleBoneCape.PrjPcb"));
    let d = ok(&["bom", &prj]);
    assert_eq!(d["source"], "schematic");
    assert_eq!(d["unique_lines"], 3);
    assert_eq!(d["count"], 3);
    let items = d["items"].as_array().unwrap();
    assert!(items.iter().any(|l| l["comment"] == "CAT24C256" && l["footprint"] == "SOIC-8"));

    // One line per part, shaped with the global flags.
    let pcb = s(&root.join("quasar_arm_pcb/pechatka.PcbDoc"));
    let d = ok(&["bom", &pcb, "--group-by", "designator", "--where", "designator_ranges=C1", "--fields", "designators,quantity"]);
    assert_eq!(d["source"], "pcb");
    assert_eq!(d["count"], 1);
    assert_eq!(d["items"][0]["quantity"], 1);

    let dir = tempdir("bom");
    let csv = dir.join("out/bom.csv");
    let d = ok(&["bom", &prj, "--out", &s(&csv), "--columns", "designators,qty,comment,footprint"]);
    assert_eq!(d["out"]["format"], "csv");
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.starts_with("designators,qty,comment,footprint\n"), "{text}");
    assert_eq!(text.lines().count(), 4);
    let json = dir.join("bom.json");
    ok(&["bom", &prj, "--out", &s(&json)]);
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
    assert_eq!(v["lines"].as_array().unwrap().len(), 3);
    // --dry-run writes nothing; design files are never overwritten by an export.
    let none = dir.join("none.csv");
    ok(&["bom", &prj, "--out", &s(&none), "--dry-run"]);
    assert!(!none.exists());
    err(&["bom", &prj, "--out", &s(&dir.join("x.SchDoc"))], "UNSAFE");
    // Variants need a project.
    err(&["bom", &pcb, "--variant", "Lite"], "USAGE");
    err(&["bom", &prj, "--variant", "Nope"], "NOT_FOUND");
}

#[test]
fn netlist_views_and_exports() {
    let Some(root) = samples() else { return };
    let prj = s(&root.join("PilotPi_PCB/I2C_Expander/I2C_Expander.PrjPcb"));
    let d = ok(&["netlist", &prj, "--where", "name=5V"]);
    assert_eq!(d["count"], 1);
    assert_eq!(d["items"][0]["node_count"], 8);
    assert!(d["items"][0]["nodes"].as_array().unwrap().iter().any(|n| n == "JP_IN.1"));

    // Schematic and PCB agree pin by pin on this project.
    let pcb = s(&root.join("PilotPi_PCB/I2C_Expander/I2C_Expander.PcbDoc"));
    let a = ok(&["netlist", &prj, "--pins"]);
    let b = ok(&["netlist", &pcb, "--pins", "--connected"]);
    let pins = |v: &serde_json::Value| {
        let mut x: Vec<(String, String)> = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["node"].as_str().unwrap().to_owned(), r["net"].as_str().unwrap().to_owned()))
            .collect();
        x.sort();
        x
    };
    // The PCB connectors also have a shell pad 0 on GND that no schematic pin maps to.
    let pcb_pins: Vec<_> = pins(&b).into_iter().filter(|(n, _)| !n.ends_with(".0")).collect();
    assert_eq!(pins(&a), pcb_pins);

    let dir = tempdir("netlist");
    let net = dir.join("board.net");
    let d = ok(&["netlist", &prj, "--out", &s(&net)]);
    assert_eq!(d["out"]["format"], "protel");
    let text = std::fs::read_to_string(&net).unwrap();
    assert!(text.contains("(\n5V\n"), "{text}");
    ok(&["netlist", &pcb, "--out", &s(&dir.join("n.csv"))]);
    assert!(std::fs::read_to_string(dir.join("n.csv")).unwrap().starts_with("Net,Designator,Pin\n"));
    err(&["netlist", &pcb, "--source", "sch"], "USAGE");
}

#[test]
fn check_findings() {
    let Some(root) = samples() else { return };
    let d = ok(&["check", &s(&root.join("PilotPi_PCB/I2C_Expander/I2C_Expander.PrjPcb"))]);
    assert_eq!(d["passed"], true);
    assert_eq!(d["count"], 0);

    let d = ok(&["check", &s(&root.join("BeagleBoneCape/BeagleBoneCape.PrjPcb"))]);
    assert_eq!(d["passed"], false);
    let items = d["items"].as_array().unwrap();
    for code in ["unannotated", "missing-document", "missing-in-pcb"] {
        let f = items.iter().find(|f| f["code"] == code).unwrap_or_else(|| panic!("{code} missing: {d}"));
        assert_eq!(f["severity"], "error");
        assert!(!f["hint"].as_str().unwrap().is_empty());
    }
    assert_eq!(items.iter().find(|f| f["code"] == "unannotated").unwrap()["refs"][0], "U?");

    // Channel designators edited on the PCB are linked by unique id, not reported as missing.
    let d = ok(&["check", &s(&root.join("PilotPi_PCB/PilotPi_Power/PilotPi_Power.PrjPcb"))]);
    let codes: Vec<&str> = d["items"].as_array().unwrap().iter().map(|f| f["code"].as_str().unwrap()).collect();
    assert!(codes.contains(&"designator-mismatch"), "{codes:?}");
    assert!(!codes.contains(&"missing-in-pcb") && !codes.contains(&"missing-in-schematic"), "{codes:?}");

    let d = ok(&["check", &s(&root.join("BeagleBoneCape/BeagleBoneCape.PrjPcb")), "--only", "unannotated", "--require", "MPN"]);
    assert_eq!(d["count"], 1);
    let d = ok(&["check", &s(&root.join("BeagleBoneCape/BeagleBoneCape.PrjPcb")), "--severity", "error", "--skip", "missing-document"]);
    assert!(d["items"].as_array().unwrap().iter().all(|f| f["severity"] == "error" && f["code"] != "missing-document"));
    let d = ok(&["check", &s(&root.join("BeagleBoneCape/BeagleBoneCape.SchDoc")), "--require", "MPN"]);
    assert!(d["items"].as_array().unwrap().iter().any(|f| f["code"] == "missing-parameter"));
    let d = ok(&["check", "--list"]);
    assert!(d["count"].as_u64().unwrap() >= 15);
    err(&["check", "--only", "nonsense", &s(&root.join("BeagleBoneCape/BeagleBoneCape.SchDoc"))], "USAGE");
}

#[test]
fn diff_designs() {
    let Some(root) = samples() else { return };
    let a = s(&root.join("quasar_arm_pcb/pechatka_100x60.PcbDoc"));
    let b = s(&root.join("quasar_arm_pcb/pechatka_100x60_fabric.PcbDoc"));
    let d = ok(&["diff", &a, &a]);
    assert_eq!(d["identical"], true);
    assert_eq!(d["count"], 0);
    let d = ok(&["diff", &a, &b, "--geometry"]);
    assert_eq!(d["identical"], false);
    let items = d["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["change"] == "part_removed" && i["ref"] == "R2-1"));
    assert!(items.iter().any(|i| i["change"] == "part_changed" && i["field"] == "position"));
    // Without --geometry, moves are not changes.
    let d = ok(&["diff", &a, &b, "--where", "field=position"]);
    assert_eq!(d["count"], 0);

    let prj = s(&root.join("PilotPi_PCB/I2C_Expander/I2C_Expander.PrjPcb"));
    let pcb = s(&root.join("PilotPi_PCB/I2C_Expander/I2C_Expander.PcbDoc"));
    let d = ok(&["diff", &prj, &pcb]);
    assert_eq!(d["a_source"], "schematic");
    assert_eq!(d["b_source"], "pcb");
    assert_eq!(d["summary"]["parts_added"], 0);
    assert_eq!(d["summary"]["nets_added"], 0);
    err(&["diff", &prj, &pcb, "--geometry"], "USAGE");

    // Against git: the samples are git clones.
    let has_git = std::process::Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success());
    if has_git && root.join("AirPi_PCB/.git").exists() {
        let d = ok(&["diff", &s(&root.join("AirPi_PCB/AirPi_Sensor/AirPi_Sensor.PrjPcb")), "--rev", "HEAD"]);
        assert_eq!(d["identical"], true, "{d}");
        err(&["diff", &a, "--rev", "no-such-rev-xyz"], "NOT_FOUND");
    }
}
