//! Native PcbLib edits on copies of the sample libraries: the file re-parses, untouched
//! footprints keep their exact streams, and every change reads back.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::*;
use serde_json::json;

type Streams = BTreeMap<String, Vec<u8>>;

fn streams(path: &Path) -> Streams {
    let mut cf = altium_format::cfbx::Compound::open(path).unwrap();
    let mut m = Streams::new();
    for s in cf.streams() {
        m.insert(s.path.clone(), cf.read(&s.path).unwrap());
    }
    m
}

/// Every stream below `prefix` exists with identical bytes in `after`.
fn assert_prefix_unchanged(before: &Streams, after: &Streams, prefix: &str) {
    for (k, v) in before.iter().filter(|(k, _)| k.starts_with(prefix)) {
        assert_eq!(after.get(k), Some(v), "stream {k} changed");
    }
}

#[test]
fn case_only_rename_reads_back_and_keeps_storage_spelling() {
    let Some(root) = samples() else { return };
    let dir = tempdir("pcblib-case-rename");
    let lib = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.PcbLib"), &dir);
    let l = s(&lib);
    let before = streams(&lib);
    let d = ok(&["pcblib", "rename", &l, "SOIC-8", "soic-8"]);
    assert_eq!(d["verified"], true);
    assert_eq!(ok(&["pcblib", "info", &l])["names"], json!(["BBB_CAPE_HEADER", "soic-8", "R0603"]));
    let shown = ok(&["pcblib", "show", &l, "soic-8"]);
    assert_eq!(shown["footprint"]["pads"], 8);
    assert_eq!(shown["footprint"]["storage"], "SOIC-8");
    let after = streams(&lib);
    assert!(after.contains_key("SOIC-8/Data"));
    assert!(!after.contains_key("soic-8/Data"));
    assert_prefix_unchanged(&before, &after, "R0603/");
    assert_prefix_unchanged(&before, &after, "BBB_CAPE_HEADER/");
    assert_prefix_unchanged(&before, &after, "Library/Models/");
}

#[test]
fn pcblib_footprint_edits() {
    let Some(root) = samples() else { return };
    let dir = tempdir("pcblib-writes");
    let lib = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.PcbLib"), &dir);
    let l = s(&lib);
    let original = std::fs::read(&lib).unwrap();
    let before = streams(&lib);

    let d = ok(&["pcblib", "rename", &l, "SOIC-8", "SOIC-8N", "--dry-run"]);
    assert_eq!(d["changed"], true);
    assert_eq!(d["backup"], serde_json::Value::Null);
    assert_eq!(std::fs::read(&lib).unwrap(), original);

    let d = ok(&["pcblib", "rename", &l, "SOIC-8", "SOIC-8N"]);
    assert_eq!(d["changes"][0]["action"], "rename");
    assert_eq!(d["changes"][0]["before"], "SOIC-8");
    assert_eq!(d["changes"][0]["after"], "SOIC-8N");
    assert_eq!(d["verified"], true);
    assert!(dir.join("BeagleBoneCape.PcbLib.bak").exists());
    assert_eq!(ok(&["pcblib", "info", &l])["names"], json!(["BBB_CAPE_HEADER", "SOIC-8N", "R0603"]));
    err(&["pcblib", "show", &l, "SOIC-8"], "NOT_FOUND");
    err(&["pcblib", "rename", &l, "SOIC-8N", "R0603"], "UNSAFE");
    ok(&["pcblib", "rename", &l, "SOIC-8N", "SOIC-8n", "--dry-run"]);

    let after = streams(&lib);
    assert_prefix_unchanged(&before, &after, "R0603/");
    assert_prefix_unchanged(&before, &after, "BBB_CAPE_HEADER/");
    assert_prefix_unchanged(&before, &after, "Library/Models/");
    assert_prefix_unchanged(&before, &after, "FileVersionInfo/");
    assert!(!after.keys().any(|k| k.starts_with("SOIC-8/")), "the old storage is still there");
    let sh = ok(&["pcblib", "show", &l, "SOIC-8N"]);
    assert_eq!(sh["footprint"]["pads"], 8);
    assert_eq!(sh["footprint"]["primitives"]["tracks"], 14);
    assert_eq!(sh["footprint"]["description"], "SOIC, 8-Leads, Body 5.1x4.0mm (max), Pitch 1.27mm, IPC Medium Density");
    let (_, env) = run(&["pcblib", "show", &l, "SOIC-8N"]);
    assert_eq!(env["warnings"], json!([]));

    let d = ok(&["pcblib", "set-param", &l, "SOIC-8N", "description", "Narrow body SOIC"]);
    assert_eq!(d["changes"][0]["key"], "DESCRIPTION");
    assert_eq!(d["changes"][0]["action"], "set");
    ok(&["pcblib", "set-param", &l, "SOIC-8N", "height", "2.5mm"]);
    let sh = ok(&["pcblib", "show", &l, "SOIC-8N", "--units", "mil"]);
    assert_eq!(sh["footprint"]["description"], "Narrow body SOIC");
    let h = sh["footprint"]["height"].as_f64().unwrap();
    assert!((h - 98.4252).abs() < 0.01, "height reads back as {h} mil");
    assert_eq!(sh["footprint"]["pads"], 8);
    err(&["pcblib", "set-param", &l, "SOIC-8N", "height", "2.5"], "USAGE");
    err(&["pcblib", "set-param", &l, "NO-SUCH", "description", "x"], "NOT_FOUND");
    let same = ok(&["pcblib", "set-param", &l, "SOIC-8N", "description", "Narrow body SOIC"]);
    assert_eq!(same["changed"], false);
    let d = ok(&["pcblib", "set-param", &l, "SOIC-8N", "MFR_PART", "SOIC-8N-W"]);
    assert_eq!(d["changes"][0]["action"], "create");
    assert_eq!(ok(&["pcblib", "show", &l, "SOIC-8N"])["footprint"]["parameters"]["MFR_PART"], "SOIC-8N-W");
    let after_params = streams(&lib);
    assert_prefix_unchanged(&before, &after_params, "R0603/");
    assert_prefix_unchanged(&before, &after_params, "BBB_CAPE_HEADER/");

    let d = ok(&["pcblib", "remove", &l, "SOIC-8N"]);
    assert_eq!(d["changes"][0]["action"], "remove");
    assert_eq!(d["footprints"], 2);
    assert_eq!(ok(&["pcblib", "info", &l])["names"], json!(["BBB_CAPE_HEADER", "R0603"]));
    err(&["pcblib", "remove", &l, "SOIC-8N"], "NOT_FOUND");
    let after_remove = streams(&lib);
    assert!(!after_remove.keys().any(|k| k.starts_with("SOIC-8N/")));
    assert_prefix_unchanged(&before, &after_remove, "R0603/");
    assert_prefix_unchanged(&before, &after_remove, "BBB_CAPE_HEADER/");
    assert_prefix_unchanged(&before, &after_remove, "Library/Models/");
    let (_, env) = run(&["pcblib", "info", &l]);
    assert_eq!(env["warnings"], json!([]));
}

#[test]
fn pcblib_copy_footprint_with_models() {
    let Some(root) = samples() else { return };
    let dir = tempdir("pcblib-copy");
    let lib = copy_to(&root.join("quasar_arm_pcb/PcbLib1.PcbLib"), &dir);
    let other = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.PcbLib"), &dir);
    let (l, o) = (s(&lib), s(&other));
    let before = streams(&lib);
    let other_bytes = std::fs::read(&other).unwrap();
    let count = ok(&["pcblib", "info", &l])["footprints"].as_u64().unwrap();
    assert_eq!(ok(&["pcblib", "info", &l])["models"], 0);

    let lib_bytes = std::fs::read(&lib).unwrap();
    let d = ok(&["pcblib", "copy", &l, &o, "BBB_CAPE_HEADER", "--dry-run"]);
    assert_eq!(d["changed"], true);
    assert_eq!(d["backup"], serde_json::Value::Null);
    assert_eq!(std::fs::read(&lib).unwrap(), lib_bytes);

    let d = ok(&["pcblib", "copy", &l, &o, "BBB_CAPE_HEADER"]);
    assert_eq!(d["footprints"], count + 1);
    assert!(d["changes"].as_array().unwrap().iter().any(|c| c["key"] == "model_id"), "{d}");
    let info = ok(&["pcblib", "info", &l]);
    assert_eq!(info["models"], 1);
    assert_eq!(info["embedded_models"], 1);
    assert_eq!(info["footprints"], count + 1);

    let src = ok(&["pcblib", "show", &o, "BBB_CAPE_HEADER"]);
    let got = ok(&["pcblib", "show", &l, "BBB_CAPE_HEADER"]);
    assert_eq!(got["pads"], src["pads"]);
    assert_eq!(got["bodies"], src["bodies"]);
    assert_eq!(got["footprint"]["primitives"], src["footprint"]["primitives"]);
    assert_eq!(got["footprint"]["description"], src["footprint"]["description"]);
    assert_eq!(got["footprint"]["models"], src["footprint"]["models"]);

    let after = streams(&lib);
    for (k, v) in &before {
        if k == "Library/Data" || k.starts_with("Library/Models/") {
            continue;
        }
        assert_eq!(after.get(k), Some(v), "stream {k} changed");
    }
    assert_eq!(after["Library/Models/Header"], 1u32.to_le_bytes().to_vec());
    assert_eq!(std::fs::read(&other).unwrap(), other_bytes, "the source library changed");

    err(&["pcblib", "copy", &l, &o, "BBB_CAPE_HEADER"], "UNSAFE");
    let d = ok(&["pcblib", "copy", &l, &o, "BBB_CAPE_HEADER", "--name", "HDR46"]);
    assert_eq!(d["changes"].as_array().unwrap().len(), 1, "the model is already there: {d}");
    assert_eq!(ok(&["pcblib", "show", &l, "HDR46"])["pads"].as_array().unwrap().len(), 46);

    let step = dir.join("step");
    let m = ok(&["pcblib", "models", &l, "--extract", &s(&step)]);
    let name = "c-87224-2-l-3d.stp";
    let copied = std::fs::read(step.join(name)).unwrap();
    let step_src = dir.join("step-src");
    ok(&["pcblib", "models", &o, "--extract", &s(&step_src)]);
    assert_eq!(copied, std::fs::read(step_src.join(name)).unwrap());
    assert_eq!(m["items"][0]["embedded"], true);
    let (_, env) = run(&["pcblib", "info", &l]);
    assert_eq!(env["warnings"], json!([]));
}

#[test]
fn pcblib_rename_moves_the_storage() {
    let Some(root) = samples() else { return };
    let dir = tempdir("pcblib-rename");
    let lib = copy_to(&root.join("quasar_arm_pcb/PcbLib1.PcbLib"), &dir);
    let l = s(&lib);
    let before = streams(&lib);
    ok(&["pcblib", "rename", &l, "HC-49/U3H (low profile)", "HC-49/3H low profile"]);
    let after = streams(&lib);
    assert!(!after.keys().any(|k| k.starts_with("HC-49_U3H (low profile)/")));
    assert_eq!(
        ok(&["pcblib", "show", &l, "HC-49/3H low profile"])["footprint"]["storage"],
        "HC-49_3H low profile"
    );
    assert_prefix_unchanged(&before, &after, "A/");
    assert_prefix_unchanged(&before, &after, "0805 Resistor/");
    let (_, env) = run(&["pcblib", "info", &l]);
    assert_eq!(env["warnings"], json!([]));
    assert_eq!(ok(&["pcblib", "info", &l])["footprints"], 44);
}

#[test]
fn pcblib_copy_linked_model_preserves_reference() {
    let Some(root) = samples() else { return };
    let dir = tempdir("pcblib-linked");
    let lib = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.PcbLib"), &dir);
    let l = s(&lib);
    let before = ok(&["pcblib", "show", &l, "SOIC-8"]);
    assert_eq!(before["bodies"][0]["model_embedded"], false);
    let (exit, env) = run(&["pcblib", "copy", &l, &l, "SOIC-8", "--name", "SOIC-8B"]);
    assert_eq!(exit, 0, "{env}");
    assert!(env["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("linked model")), "{env}");
    let after = ok(&["pcblib", "show", &l, "SOIC-8B"]);
    assert_eq!(before["bodies"], after["bodies"]);
    assert_eq!(before["pads"], after["pads"]);
    std::fs::remove_dir_all(dir).unwrap();
}
