mod common;
use common::*;
use std::path::Path;

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let target = dst.join(e.file_name());
        if e.path().is_dir() { copy_dir(&e.path(), &target); }
        else { std::fs::copy(e.path(), target).unwrap(); }
    }
}

#[test]
fn bare_relative_project_keeps_sibling_sheets_and_reports_missing_coverage() {
    let Some(root) = samples() else { return };
    let source = root.join("AirPi_PCB/AirPi_Sensor/AirPi_Sensor.PrjPcb");
    let p = altium_doc::project::Project::load(&source).unwrap();
    let dir = tempdir("project-coverage");
    let project_dir = dir.join("Sensor");
    std::fs::create_dir_all(&project_dir).unwrap();
    let file = copy_to(&source, &project_dir);
    for d in &p.documents {
        if d.exists && !d.generated {
            let target = project_dir.join(d.rel.replace('\\', "/"));
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(&d.path, target).unwrap();
        }
    }
    let name = file.file_name().unwrap().to_str().unwrap();
    let (exit, relative) = run_in(Some(&project_dir), &["bom", name, "--limit", "1"]);
    assert_eq!(exit, 0, "{relative}");
    let absolute = ok(&["bom", &s(&file), "--limit", "1"]);
    assert_eq!(relative["data"]["total_parts"], 68);
    assert_eq!(relative["data"]["total_parts"], absolute["total_parts"]);
    assert_eq!(relative["data"]["documents"].as_array().unwrap().len(), 11);
    assert_eq!(absolute["documents_declared"], 11);
    assert_eq!(absolute["documents_found"], 11);
    let utils = dir.join("Utils");
    std::fs::rename(&utils, dir.join("Utils.off")).unwrap();
    let (_, partial) = run(&["bom", &s(&file)]);
    assert_eq!(partial["data"]["incomplete"], true);
    assert_eq!(partial["data"]["documents_found"], 1);
    assert!(partial["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("10 of 11")));
    let d = ok(&["check", &s(&file)]);
    assert_eq!(d["by_code"]["incomplete-project"], 1);
    assert!(d["by_code"].get("missing-in-schematic").is_none());
    assert!(d["by_code"].get("missing-in-pcb").is_none());
    assert_eq!(d["passed"], false);
    assert_eq!(run(&["check", &s(&file), "--only", "unannotated", "--fail-on", "error"]).0, 9);
    let (_, diff) = run(&["diff", &s(&file), &s(&file)]);
    assert_eq!(diff["data"]["incomplete"], true);
}

#[test]
fn directory_inputs_and_macos_junk_are_unambiguous() {
    let dir = tempdir("directory-input");
    std::fs::write(dir.join("A.PrjPcb"), b"[Design]\n").unwrap();
    std::fs::create_dir_all(dir.join("__MACOSX")).unwrap();
    std::fs::write(dir.join("__MACOSX/._A.PrjPcb"), b"junk").unwrap();
    assert_eq!(ok(&["inspect", &s(&dir)])["kind"], "project");
    assert_eq!(ok(&["project", "find", &s(&dir)])["count"], 1);
    assert_eq!(ok(&["project", "find", &s(&dir), "--include-hidden"])["count"], 2);
    std::fs::write(dir.join("B.PrjPcb"), b"[Design]\n").unwrap();
    for cmd in ["inspect", "bom", "netlist", "check"] {
        let v = err(&[cmd, &s(&dir)], "AMBIGUOUS");
        assert_eq!(v["error"]["details"]["candidates"].as_array().unwrap().len(), 2);
    }
}

#[test]
fn complete_project_still_checks_sync() {
    let Some(root) = samples() else { return };
    let dir = tempdir("complete-sync");
    copy_dir(&root.join("BeagleBoneCape"), &dir);
    let d = ok(&["check", &s(&dir.join("BeagleBoneCape.PrjPcb"))]);
    assert!(d["by_code"].get("missing-in-pcb").is_some());
}
