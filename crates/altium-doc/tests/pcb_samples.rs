//! Fixture tests over every real PcbDoc in `.fixtures/samples` (skipped when absent).
//! The expected numbers were cross-checked against each storage's `Header` record count and
//! by inspecting the decoded objects (`ace pcb ...`).

use std::path::{Path, PathBuf};

use altium_doc::pcb::{self, Layer, LoadOptions, part, query};

pub fn samples() -> Option<PathBuf> {
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

/// (file, components, nets, pads, vias, tracks, arcs, fills, texts, regions, board_regions,
///  polygons, bodies, models, rules, classes, dimensions, rooms)
const EXPECT: &[(&str, [usize; 17])] = &[
    ("BeagleBoneCape/BeagleBoneCape.PcbDoc", [2, 0, 96, 0, 8, 8, 0, 4, 0, 1, 0, 46, 1, 34, 15, 0, 0]),
    ("quasar_arm_pcb/CoilOrganiser.PcbDoc", [0, 0, 8, 0, 0, 2, 0, 8, 9, 1, 0, 0, 0, 35, 15, 0, 0]),
    ("quasar_arm_pcb/GenerationPCB.PcbDoc", [0, 99, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 35, 22, 0, 0]),
    ("quasar_arm_pcb/Integrator.PcbDoc", [6, 105, 22, 0, 74, 3, 0, 12, 0, 1, 0, 1, 0, 35, 16, 0, 0]),
    ("quasar_arm_pcb/pechatka.PcbDoc", [116, 77, 390, 0, 1245, 42, 1, 244, 8, 1, 5, 29, 0, 46, 21, 0, 6]),
    ("quasar_arm_pcb/pechatka_100x60.PcbDoc", [132, 94, 476, 0, 1450, 55, 3, 266, 9, 1, 5, 52, 0, 41, 23, 2, 0]),
    ("quasar_arm_pcb/pechatka_100x60_fabric.PcbDoc", [132, 94, 454, 0, 1470, 55, 3, 265, 9, 1, 5, 52, 0, 41, 23, 0, 0]),
    ("quasar_arm_pcb/pechatka_100x60_fabric_v2.PcbDoc", [142, 101, 463, 20, 1456, 60, 3, 285, 11, 1, 5, 51, 0, 41, 23, 0, 0]),
    ("AirPi_PCB/AirPi_Power/AirPi_Power.PcbDoc", [44, 29, 174, 33, 629, 68, 2, 98, 89, 1, 2, 106, 19, 35, 23, 0, 0]),
    ("AirPi_PCB/AirPi_Sensor/AirPi_Sensor.PcbDoc", [68, 91, 317, 62, 1420, 85, 6, 155, 319, 1, 2, 121, 27, 35, 27, 0, 0]),
    ("PilotPi_PCB/I2C_Expander/I2C_Expander.PcbDoc", [8, 4, 42, 20, 207, 18, 0, 32, 73, 1, 2, 17, 5, 35, 16, 0, 0]),
    ("PilotPi_PCB/PilotPi_Power/PilotPi_Power.PcbDoc", [44, 42, 191, 17, 550, 64, 2, 125, 162, 1, 2, 117, 20, 35, 22, 0, 0]),
    ("PilotPi_PCB/PilotPi_Sensors/PilotPi_Sensors.PcbDoc", [79, 96, 372, 63, 1585, 111, 5, 195, 464, 1, 2, 114, 25, 35, 26, 0, 0]),
];

#[test]
fn every_sample_decodes_cleanly_with_expected_counts() {
    let Some(s) = samples() else { return };
    for (rel, e) in EXPECT {
        let f = s.join(rel);
        let d = pcb::load(&f).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let got = [
            d.components.len(),
            d.nets.len(),
            d.pads.len(),
            d.vias.len(),
            d.tracks.len(),
            d.arcs.len(),
            d.fills.len(),
            d.texts.len(),
            d.regions.iter().filter(|r| r.source != "board_regions").count(),
            d.regions.iter().filter(|r| r.source == "board_regions").count(),
            d.polygons.len(),
            d.bodies.len(),
            d.models.len(),
            d.rules.len(),
            d.classes.len(),
            d.dimensions.len(),
            d.rooms.len(),
        ];
        assert_eq!(&got, e, "{rel}");
        assert!(d.warnings.is_empty(), "{rel}: {:?}", d.warnings);
        // Every sample is a 2-layer board.
        assert_eq!(d.board.copper_layers, vec![Layer::TOP, Layer::BOTTOM], "{rel}");
        // Header counts agree with what was decoded.
        for (dir, n) in [("Pads6", d.pads.len()), ("Tracks6", d.tracks.len()), ("Texts6", d.texts.len())] {
            if let Some(&h) = d.counts.get(dir) {
                assert_eq!(h as usize, n, "{rel} {dir}");
            }
        }
        // Every primitive's net/component reference resolves.
        for p in &d.pads {
            assert_eq!(p.owner.net_index.is_some(), p.owner.net.is_some(), "{rel} pad {}", p.index);
            assert_eq!(p.owner.component_index.is_some(), p.owner.component.is_some(), "{rel} pad {}", p.index);
        }
    }
}

#[test]
fn pilotpi_power_details() {
    let Some(s) = samples() else { return };
    let d = pcb::load(&s.join("PilotPi_PCB/PilotPi_Power/PilotPi_Power.PcbDoc")).unwrap();
    assert_eq!(d.file.header, "PCB 6.0 Binary File");
    assert_eq!(d.file.format_version, Some(5.01));
    assert_eq!(d.file.saved_with.as_deref(), Some("Release 17.1"));
    assert_eq!(d.board.stackup.source, "v9");
    assert_eq!(d.board.stackup.layers.len(), 9);
    // 2 × 1.4 mil copper + 12.6 mil FR-4 core.
    assert_eq!(d.board.stackup.board_thickness.mil(), 15.4);
    assert_eq!(d.board.outline.vertex_count, 9);
    let bb = d.board.outline.bbox.unwrap();
    assert!((bb.width.mil() - 3175.0).abs() < 0.01 && (bb.height.mil() - 2215.0).abs() < 0.01);
    let mech: Vec<String> = d
        .board
        .layers
        .iter()
        .filter(|l| l.kind == pcb::LayerKind::Mechanical && l.enabled)
        .map(|l| l.layer.name())
        .collect();
    assert_eq!(mech, ["Mechanical1", "Mechanical13", "Mechanical15"]);
    // Designator text differs from the schematic designator on the channel headers.
    let c = d.component("01").unwrap();
    assert_eq!(c.source_designator, "CH1");
    assert_eq!(c.footprint, "HDR2.54-LI-3P");
    assert_eq!(c.rotation, 180.0);
    // Regions6 duplicates ShapeBasedRegions6 and is kept apart.
    assert_eq!(d.regions_flat.len(), 162);
    // Embedded models decompress to STEP.
    let mut d = d;
    let step = d.model_data(0).unwrap();
    assert!(step.starts_with(b"ISO-10303-21"));
}

#[test]
fn pilotpi_sensors_nets_pads_holes() {
    let Some(s) = samples() else { return };
    let d = pcb::load(&s.join("PilotPi_PCB/PilotPi_Sensors/PilotPi_Sensors.PcbDoc")).unwrap();
    let c16 = d.component("C16").unwrap();
    assert_eq!(c16.comment, "220nF (224) 10% 16V");
    assert_eq!(c16.sch_unique_id, "OTZCGDUX");
    assert_eq!(c16.parameters.get("SuppliersPartNumber").map(String::as_str), Some("C16772"));
    let gnd = d.net("GND").unwrap();
    let nd = query::net_detail(&d, gnd.index);
    assert_eq!((nd.tracks, nd.vias, nd.node_count), (160, 13, 61));
    let holes = query::holes(&d);
    assert_eq!(holes.iter().map(|h| h.count).sum::<usize>(), 151);
    let slot = holes.iter().find(|h| h.shape == "slot").unwrap();
    assert!((slot.diameter.mil() - 78.7402).abs() < 1e-3);
    // Pad 92 is a rounded rectangle with a 50 % corner radius on TopLayer.
    let p = &d.pads[92];
    assert_eq!((p.shape.as_str(), p.corner_radius_percent, p.layer), ("rounded_rectangle", Some(50), Layer::TOP));
    // Pads carry their unique ids.
    assert!(d.pads.iter().all(|p| p.owner.unique_id.is_some()));
}

#[test]
fn beaglebone_outline_and_v8_stack() {
    let Some(s) = samples() else { return };
    let d = pcb::load(&s.join("BeagleBoneCape/BeagleBoneCape.PcbDoc")).unwrap();
    assert_eq!(d.board.stackup.source, "v8");
    assert_eq!(d.board.outline.vertex_count, 22);
    assert_eq!(d.board.outline.arcs, 8);
    let bb = d.board.outline.bbox.unwrap();
    assert!((bb.width.mil() - 3400.0).abs() < 0.01);
    let designators: Vec<&str> = d.components.iter().map(|c| c.designator.as_str()).collect();
    assert_eq!(designators, ["P9", "P8"]);
    assert_eq!(d.components[0].comment, "BBB_P9");
    assert_eq!(d.components[0].side, "bottom");
}

#[test]
fn quasar_cyrillic_and_rooms() {
    let Some(s) = samples() else { return };
    altium_format::props::set_codepage("windows-1251");
    let d = pcb::load(&s.join("quasar_arm_pcb/pechatka.PcbDoc")).unwrap();
    assert_eq!(d.rooms[0].name, "Keyboard_on_board");
    assert_eq!(d.rooms[0].confinement, "ConfineIn");
    let names: Vec<&str> = d.polygons.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["GND_5V", "GND_VDD", "GND_BUTTONS", "common", "GND_VDDA"]);
    let d = pcb::load(&s.join("quasar_arm_pcb/pechatka_100x60.PcbDoc")).unwrap();
    assert_eq!(d.dimensions[0].kind, "linear");
    assert_eq!(d.dimensions[0].references.len(), 2);
    altium_format::props::set_codepage("windows-1252");
}

#[test]
fn design_conversion() {
    let Some(s) = samples() else { return };
    let des = pcb::load_design(&s.join("PilotPi_PCB/I2C_Expander/I2C_Expander.PcbDoc")).unwrap();
    assert_eq!(des.source, "pcb");
    assert_eq!(des.parts.len(), 8);
    assert_eq!(des.nets.len(), 4);
    let nodes: usize = des.nets.iter().map(|n| n.nodes.len()).sum();
    assert!(nodes > 0);
    assert!(des.parts.iter().all(|p| p.side.is_some() && p.x.is_some()));
}

#[test]
fn partial_load_skips_primitives_but_keeps_components() {
    let Some(s) = samples() else { return };
    let d = pcb::load_with(&s.join("AirPi_PCB/AirPi_Sensor/AirPi_Sensor.PcbDoc"), &LoadOptions::parts(part::VIAS)).unwrap();
    assert!(d.pads.is_empty() && d.tracks.is_empty());
    assert_eq!(d.vias.len(), 62);
    assert_eq!(d.components.len(), 68);
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
}

#[test]
fn big_file_parses_fast() {
    let Some(s) = samples() else { return };
    let t = std::time::Instant::now();
    let d = pcb::load(&s.join("PilotPi_PCB/PilotPi_Power/PilotPi_Power.PcbDoc")).unwrap();
    let ms = t.elapsed().as_millis();
    assert_eq!(d.pads.len(), 191);
    // Generous even for debug builds; release takes ~15 ms.
    assert!(ms < 3000, "{ms} ms");
}
