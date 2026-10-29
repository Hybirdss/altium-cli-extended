//! Fixture tests over the real PcbLib files in `.fixtures/samples` (skipped when absent).

use std::path::{Path, PathBuf};

use altium_doc::pcb::Layer;
use altium_doc::pcblib;

fn samples() -> Option<PathBuf> {
    let mut d = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    loop {
        let s = d.join(".fixtures/samples");
        if s.is_dir() {
            return Some(s);
        }
        if !d.pop() {
            return None;
        }
    }
}

#[test]
fn beaglebone_library() {
    let Some(s) = samples() else { return };
    let mut lib = pcblib::load(&s.join("BeagleBoneCape/BeagleBoneCape.PcbLib")).unwrap();
    assert!(lib.warnings.is_empty(), "{:?}", lib.warnings);
    assert_eq!(lib.file.header, "PCB 6.0 Binary Library File");
    let names: Vec<&str> = lib.footprints.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["BBB_CAPE_HEADER", "SOIC-8", "R0603"]);
    let soic = lib.footprint("soic-8").unwrap();
    assert_eq!(soic.pads.len(), 8);
    assert_eq!((soic.tracks.len(), soic.arcs.len(), soic.bodies.len()), (14, 3, 1));
    assert!(soic.description.starts_with("SOIC, 8-Leads"));
    assert!((soic.height.unwrap().mil() - 68.8976).abs() < 1e-3);
    assert!(soic.pads.iter().all(|p| p.layer == Layer::TOP && p.hole_size.is_none()));
    let r = lib.footprint("R0603").unwrap();
    assert_eq!(r.pads.len(), 2);
    let pads: usize = lib.footprints.iter().map(|f| f.pads.len()).sum();
    assert_eq!(pads, 56);
    assert_eq!(lib.models.len(), 2);
    assert_eq!(lib.models[1].name, "c-87224-2-l-3d.stp");
    assert_eq!(lib.models[1].used_by, 23);
    let step = lib.model_data(0).unwrap();
    assert!(step.starts_with(b"ISO-10303-21"));
}

#[test]
fn quasar_library() {
    let Some(s) = samples() else { return };
    altium_format::props::set_codepage("windows-1251");
    let lib = pcblib::load(&s.join("quasar_arm_pcb/PcbLib1.PcbLib")).unwrap();
    assert!(lib.warnings.is_empty(), "{:?}", lib.warnings);
    assert_eq!(lib.footprints.len(), 44);
    assert_eq!(lib.file.unique_id.as_deref(), Some("QKWPILJA"));
    // '/' in a footprint name is stored as '_' in its storage name.
    let hc = lib.footprint("HC-49/U3H (low profile)").unwrap();
    assert_eq!(hc.storage, "HC-49_U3H (low profile)");
    // Footprint WideStrings (ENCODEDTEXTn code points) supply the text.
    assert_eq!(lib.footprint("HCM").unwrap().texts[0].text, "+");
    assert_eq!(lib.footprint("DC-044").unwrap().description, "Разъем питания штыревой на плату, 5.5 x 2.5 мм");
    let lq = lib.footprint("LQFP-48_L").unwrap();
    assert_eq!(lq.pads.len(), 48);
    let pads: usize = lib.footprints.iter().map(|f| f.pads.len()).sum();
    assert_eq!(pads, 254);
    assert!(lib.footprints.iter().all(|f| f.warnings.is_empty()));
    altium_format::props::set_codepage("windows-1252");
}
