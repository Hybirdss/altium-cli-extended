//! Corrupt or mislabelled input files still get one JSON envelope with a stable error code.

mod common;

use common::*;

#[test]
fn corrupt_region_coordinate_in_pcblib_returns_an_envelope() {
    let Some(root) = samples() else { return };
    let dir = tempdir("corrupt-pcblib");
    let lib = copy_to(&root.join("BeagleBoneCape/BeagleBoneCape.PcbLib"), &dir);
    let mut bytes = std::fs::read(&lib).unwrap();
    assert_eq!(bytes[99149], 0xC1, "fixture changed; pick another region vertex byte");
    bytes[99149] = 0xE5;
    std::fs::write(&lib, bytes).unwrap();
    let (exit, v) = run(&["pcblib", "list", &s(&lib)]);
    assert_eq!(v["error"]["code"], "INTERNAL", "{v}");
    assert!(v["error"]["details"]["location"].as_str().unwrap().contains("geom.rs"), "{v}");
    assert_eq!(exit, 70);
    std::fs::remove_dir_all(dir).unwrap();
}
