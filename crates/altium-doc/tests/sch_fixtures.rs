//! Fixture tests over the real sample projects in `.fixtures/samples` (skipped when absent).
//! Every number asserted here was checked by hand against the files and, for nets, against
//! the net names / pad nets of the matching PcbDoc.

use std::path::{Path, PathBuf};

use altium_core::units::Coord;
use altium_doc::project::Project;
use altium_doc::sch::{self, connect, edit, project as sproj};
use altium_doc::schlib;

/// `.fixtures/samples` of the workspace, or of an enclosing checkout (git worktrees live
/// inside the main checkout), or `$ACE_FIXTURES`.
fn samples() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ACE_FIXTURES").map(PathBuf::from).filter(|p| p.is_dir()) {
        return Some(p);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().map(|a| a.join(".fixtures/samples")).find(|p| p.is_dir())
}

macro_rules! need {
    () => {
        match samples() {
            Some(s) => s,
            None => {
                eprintln!("skipping: .fixtures/samples not present (scripts/fetch-fixtures.sh)");
                return;
            }
        }
    };
}

fn all_with_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != ".git") {
                all_with_ext(&p, ext, out);
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
            out.push(p);
        }
    }
    out.sort();
}

fn with_codepage<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    let cp = if path.to_string_lossy().contains("quasar") { "windows-1251" } else { "windows-1252" };
    altium_format::props::set_codepage(cp);
    let r = f();
    altium_format::props::set_codepage("windows-1252");
    r
}

#[test]
fn every_schdoc_parses() {
    let s = need!();
    let mut files = vec![];
    all_with_ext(&s, "SchDoc", &mut files);
    assert_eq!(files.len(), 43);
    let mut components = 0;
    for f in &files {
        let d = with_codepage(f, || sch::load(f)).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert!(d.sheet.is_some(), "{}: no sheet record", f.display());
        for w in &d.warnings {
            assert!(
                w.contains("unannotated") || w.contains("hidden pin"),
                "{}: unexpected warning {w}",
                f.display()
            );
        }
        components += d.components.len();
    }
    assert_eq!(components, 450);
}

#[test]
fn fan_sheet_objects_and_connectivity() {
    let s = need!();
    let d = sch::load(&s.join("AirPi_PCB/Utils/Fan.SchDoc")).unwrap();
    assert_eq!(d.records.len(), 46);
    assert_eq!(d.header.int("WEIGHT"), Some(46));
    assert_eq!(d.parameters.len(), 27);
    assert_eq!(d.sheet.as_ref().unwrap().size, "A4");
    let c = &d.components[0];
    assert_eq!(c.designator, "FAN0");
    assert_eq!(c.lib_ref, "20mmFAN");
    assert_eq!(c.footprint.as_deref(), Some("20mmFAN"));
    assert_eq!(c.description, "20mm_5V_风扇");
    assert_eq!(c.models[0].pin_map.len(), 2);
    assert_eq!(c.models[0].pin_map[0].pin, "5V");
    assert_eq!(c.models[0].pin_map[0].pads, vec!["7"]);
    assert_eq!(c.pads_for("5V"), vec!["7"]);
    let pin = c.pins.iter().find(|p| p.designator == "5V").unwrap();
    // Body end (740,400), 30 long, pointing left: the wire ends at x=710.
    assert_eq!(pin.hot_end.x, Coord::from_sch(710, 0));
    assert_eq!(pin.unique_id.as_deref(), Some("SRPCVJJM"));
    assert_eq!(d.ports.len(), 2);
    assert_eq!(d.ports[0].ends[1].x, Coord::from_sch(680, 0));
    let g = connect::graph(&d);
    let nets = connect::sheet_nets(&d, &g, &connect::NetOptions::default());
    assert_eq!(nets.len(), 2);
    assert_eq!(nets[0].name, "NetFAN0_5V");
    assert!(nets[0].auto_named);
    assert_eq!(nets[0].ports, vec!["5V"]);
    assert_eq!(nets[0].nodes[0].pads, vec!["7"]);
    assert_eq!(pin.net.as_deref(), Some("NetFAN0_5V"));
}

#[test]
fn multipart_components() {
    let s = need!();
    let f = s.join("quasar_arm_pcb/Integrator.SchDoc");
    let d = with_codepage(&f, || sch::load(&f)).unwrap();
    let u = d.find_components("U101").unwrap();
    assert_eq!(u.len(), 2);
    assert_eq!(u[0].display_designator, "U101A");
    assert_eq!(u[1].display_designator, "U101B");
    assert_eq!(u[0].part_count, 2);
    // Part A pins (1,2,3) plus the shared power pins (4,8): five of the eight pin records.
    let mut pins: Vec<&str> = u[0].pins.iter().map(|p| p.designator.as_str()).collect();
    pins.sort();
    assert_eq!(pins, vec!["1", "2", "3", "4", "8"]);
    assert_eq!(d.find_components("U101B").unwrap().len(), 1);
    assert!(d.find_components("U999").is_err());
    let design = sch::design_of(&d, &connect::NetOptions::default());
    let part = design.parts.iter().find(|p| p.designator == "U101").unwrap();
    assert_eq!(part.units, vec!["A", "B"]);
}

#[test]
fn hierarchy_and_scope() {
    let s = need!();
    let p = Project::load(&s.join("AirPi_PCB/AirPi_Sensor/AirPi_Sensor.PrjPcb")).unwrap();
    let h = sproj::hierarchy(&p).unwrap();
    assert_eq!(h.configured_scope, "automatic");
    assert_eq!(h.scope, sproj::Scope::Hierarchical);
    assert_eq!(h.roots.len(), 1);
    assert_eq!(h.roots[0].file, "PiHAT_Sensor.SchDoc");
    assert_eq!(h.roots[0].children.len(), 10);
    assert!(h.repeated_sheets.is_empty());
    let q = Project::load(&s.join("quasar_arm_pcb/Quasar ARM.PrjPcb")).unwrap();
    let hq = with_codepage(&q.path, || sproj::hierarchy(&q)).unwrap();
    assert_eq!(hq.scope, sproj::Scope::Flat);
    assert_eq!(hq.roots.len(), 7);
}

/// (project, scope, nets, auto-named, parts) — every net name also exists in the PcbDoc.
#[test]
fn project_nets_match_pcb() {
    let s = need!();
    let cases = [
        ("AirPi_PCB/AirPi_Power/AirPi_Power.PrjPcb", sproj::Scope::Hierarchical, 29, 8, 44),
        ("AirPi_PCB/AirPi_Sensor/AirPi_Sensor.PrjPcb", sproj::Scope::Hierarchical, 91, 31, 68),
        ("PilotPi_PCB/PilotPi_Power/PilotPi_Power.PrjPcb", sproj::Scope::Hierarchical, 42, 10, 44),
        ("PilotPi_PCB/PilotPi_Sensors/PilotPi_Sensors.PrjPcb", sproj::Scope::Hierarchical, 96, 35, 79),
        ("PilotPi_PCB/I2C_Expander/I2C_Expander.PrjPcb", sproj::Scope::Global, 4, 0, 8),
        ("quasar_arm_pcb/Quasar ARM.PrjPcb", sproj::Scope::Flat, 105, 83, 157),
        ("BeagleBoneCape/BeagleBoneCape.PrjPcb", sproj::Scope::Global, 2, 0, 3),
    ];
    for (rel, scope, nets, auto, parts) in cases {
        let path = s.join(rel);
        let p = Project::load(&path).unwrap();
        let n = with_codepage(&path, || sproj::netlist(&p)).unwrap();
        assert_eq!(n.scope, scope, "{rel}");
        assert_eq!(n.nets.len(), nets, "{rel}");
        assert_eq!(n.nets.iter().filter(|x| x.auto_named).count(), auto, "{rel}");
        assert_eq!(n.parts.len(), parts, "{rel}");
    }
}

#[test]
fn cross_sheet_naming() {
    let s = need!();
    let p = Project::load(&s.join("PilotPi_PCB/PilotPi_Sensors/PilotPi_Sensors.PrjPcb")).unwrap();
    let n = sproj::netlist(&p).unwrap();
    let get = |name: &str| n.nets.iter().find(|x| x.name == name).unwrap_or_else(|| panic!("no net {name}"));
    // Labelled SPI1CE0_BCM18 on the top sheet and CSN in the child: the PCB calls it CSN.
    let csn = get("CSN");
    let nodes: Vec<(&str, &str)> = csn.nodes.iter().map(|x| (x.designator.as_str(), x.pin.as_str())).collect();
    assert_eq!(nodes, vec![("JP_RPi", "12"), ("U7", "10")]);
    assert_eq!(csn.sheets.len(), 2);
    assert!(csn.other_names.iter().any(|x| x == "SPI1CE0_BCM18"));
    assert_eq!(get("GND").nodes.len(), 55);
    // Named single-pin nets stay (the PCB has them); unnamed ones are left out.
    assert_eq!(get("GPCLK0_BCM4").nodes.len(), 1);
    assert!(!n.nets.iter().any(|x| x.auto_named && x.nodes.len() == 1));
    // Pin map applied: FAN1 pin "5V" is pad 7 on the PCB.
    let p2 = Project::load(&s.join("AirPi_PCB/AirPi_Power/AirPi_Power.PrjPcb")).unwrap();
    let d = sproj::project_design(&p2).unwrap();
    assert_eq!(d.source, "schematic");
    let v5 = d.nets.iter().find(|x| x.name == "5V").unwrap();
    assert!(v5.nodes.iter().any(|x| x.designator == "FAN0" && x.pin == "7" && x.pin_name == "5V"));
    // Sheet-entry-only nets keep automatic names (NetC2_2 in the PCB, not VIN).
    assert!(d.nets.iter().any(|x| x.name == "NetC2_2" && x.auto_named));
    assert!(!d.nets.iter().any(|x| x.name == "VIN"));
}

#[test]
fn sheet_design_api() {
    let s = need!();
    let d = sch::sheet_design(&s.join("PilotPi_PCB/I2C_Expander/I2C_Expander.SchDoc")).unwrap();
    assert_eq!(d.parts.len(), 8);
    assert_eq!(d.nets.len(), 4);
    assert!(d.nets.iter().all(|n| n.nodes.len() == 8));
}

#[test]
fn bus_ports_join_members() {
    let s = need!();
    let d = sch::load(&s.join("AirPi_PCB/Utils/PCA9685.SchDoc")).unwrap();
    let port = d.ports.iter().find(|p| p.name == "PWM[0..15]").unwrap();
    assert!(port.on_bus);
    assert!(port.net.is_none());
    assert_eq!(d.buses.len(), 1);
    let top = sch::load(&s.join("AirPi_PCB/AirPi_Sensor/PiHAT_Sensor.SchDoc")).unwrap();
    assert_eq!(top.buses[0].labels, vec!["PWM[0..15]"]);
    assert_eq!(top.buses[0].entries.len(), 16);
    assert_eq!(top.bus_entries[0].net.as_deref(), Some("PWM0"));
    let p = Project::load(&s.join("AirPi_PCB/AirPi_Sensor/AirPi_Sensor.PrjPcb")).unwrap();
    let n = sproj::netlist(&p).unwrap();
    assert!(!n.warnings.iter().any(|w| w.contains("no sheet entry")), "{:?}", n.warnings);
    let pwm0 = n.nets.iter().find(|x| x.name == "PWM0").unwrap();
    assert_eq!(pwm0.sheets.len(), 2, "{pwm0:?}");
}

#[test]
fn find_and_records() {
    let s = need!();
    let d = sch::load(&s.join("AirPi_PCB/Utils/Fan.SchDoc")).unwrap();
    let hits = d.find("srpcvjjm");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].owner.as_deref(), Some("FAN0"));
    assert_eq!(hits[0].record_type, "parameter");
    let counts = d.record_counts();
    assert_eq!(counts["parameter"], 30);
    assert_eq!(counts["map_definer"], 2);
}

#[test]
fn schlib_symbols() {
    let s = need!();
    let l = schlib::load(&s.join("BeagleBoneCape/BeagleBoneCape.SchLib")).unwrap();
    assert_eq!(l.symbols.len(), 3);
    let cat = l.symbol("CAT24C256").unwrap();
    assert_eq!(cat.pins.len(), 8);
    assert!(cat.pins.iter().all(|p| p.binary));
    assert_eq!(cat.pins[0].pin.name, "A0");
    assert_eq!(cat.pins[0].pin.designator, "1");
    assert_eq!(cat.pins[0].pin.electrical, "passive");
    assert_eq!(cat.footprint.as_deref(), Some("SOIC-8"));
    assert_eq!(cat.designator.as_deref(), Some("U?"));
    assert_eq!(cat.pins[0].extra.get("SYMBOL_LINEWIDTH").map(String::as_str), Some("0"));
    assert_eq!(cat.primitives.iter().filter(|p| p.kind == "rectangle").count(), 1);

    let q = s.join("quasar_arm_pcb/Quasar ARM.SCHLIB");
    let ql = with_codepage(&q, || schlib::load(&q)).unwrap();
    assert_eq!(ql.symbols.len(), 36);
    assert_eq!(ql.symbols.iter().map(|s| s.pins.len()).sum::<usize>(), 227);
    assert!(ql.warnings.is_empty(), "{:?}", ql.warnings);
    // Long name through SectionKeys; '/' replaced in the storage name.
    assert_eq!(ql.symbol("Elrctro-Magnetic Sound Generator").unwrap().storage, "Elrctro-Magnetic Sound Generato");
    let op = ql.symbol("MCP6022-I/SN").unwrap();
    assert_eq!(op.storage, "MCP6022-I_SN");
    assert_eq!(op.part_count, 2);
    assert_eq!(op.pins.len(), 8);
    assert!(!ql.find("SOIC").is_empty());
}

#[test]
fn edits_roundtrip() {
    let s = need!();
    let tmp = std::env::temp_dir().join(format!("altium-sch-edit-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let f = tmp.join("LM2596.SchDoc");
    std::fs::copy(s.join("AirPi_PCB/Utils/LM2596.SchDoc"), &f).unwrap();
    let d = sch::load(&f).unwrap();

    // Existing parameter (record holds GBK text decoded as windows-1252 next to it).
    let (b, ch) = edit::set_param(&d, "U2", "Supplier", "Digi-Key").unwrap();
    assert_eq!(ch.len(), 1);
    let n = edit::verify(&d, &b, &ch).unwrap();
    let u2 = &n.find_components("U2").unwrap()[0];
    assert_eq!(u2.param("Supplier").unwrap().value, "Digi-Key");
    assert_eq!(u2.param("ComponentLink1Description").unwrap().value, "供应商链接");

    // New parameter, non-ASCII value: appended, WEIGHT updated, %UTF8% copy written.
    let (b, ch) = edit::set_param(&d, "U2", "Note", "é ok").unwrap();
    assert_eq!(ch[0].action, "create");
    let n = edit::verify(&d, &b, &ch).unwrap();
    assert_eq!(n.records.len(), d.records.len() + 1);
    assert_eq!(n.header.int("WEIGHT"), Some(d.records.len() as i64 + 1));
    assert_eq!(n.find_components("U2").unwrap()[0].param("Note").unwrap().value, "é ok");

    // Footprint, designator, comment, sheet parameter.
    let (b, ch) = edit::set_footprint(&d, "L2", "CDRH127-NEW").unwrap();
    assert_eq!(ch.len(), 2);
    let n = edit::verify(&d, &b, &ch).unwrap();
    assert_eq!(n.find_components("L2").unwrap()[0].footprint.as_deref(), Some("CDRH127-NEW"));
    assert!(edit::set_designator(&d, "L2", "D2").is_err());
    let (b, ch) = edit::set_designator(&d, "L2", "L20").unwrap();
    edit::verify(&d, &b, &ch).unwrap();
    let (b, ch) = edit::set_comment(&d, "D2", "SS54B").unwrap();
    edit::verify(&d, &b, &ch).unwrap();
    let (b, ch) = edit::set_sheet_param(&d, "Revision", "B").unwrap();
    let n = edit::verify(&d, &b, &ch).unwrap();
    assert_eq!(n.doc_param("Revision").unwrap().value, "B");

    // Removal renumbers later owners and keeps connectivity identical.
    let (b, ch, renumbered) = edit::remove_param(&d, "U2", "Notepad").unwrap();
    assert!(renumbered > 400);
    let n = edit::verify(&d, &b, &ch).unwrap();
    assert!(n.find_components("U2").unwrap()[0].param("Notepad").is_none());
    let before = connect::sheet_nets(&d, &connect::graph(&d), &Default::default());
    let after = connect::sheet_nets(&n, &connect::graph(&n), &Default::default());
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(&after) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.nodes.len(), b.nodes.len());
    }
    assert!(edit::remove_param(&d, "U2", "Comment").is_err());
    std::fs::remove_dir_all(&tmp).ok();
}
