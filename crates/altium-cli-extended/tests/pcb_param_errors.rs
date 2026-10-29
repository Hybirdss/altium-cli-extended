mod common;

use common::*;

#[test]
fn missing_component_unique_id_has_a_recovery_hint() {
    let Some(root) = samples() else { return };
    for (relative, designator) in [
        ("AirPi_PCB/AirPi_Power/AirPi_Power.PcbDoc", "Cout1"),
        ("BeagleBoneCape/BeagleBoneCape.PcbDoc", "P9"),
    ] {
        let file = root.join(relative);
        let before = std::fs::read(&file).unwrap();
        let error = err(&["pcb", "set-param", &s(&file), designator, "QA_NATIVE", "value", "--dry-run"], "UNSAFE");
        assert!(error["error"]["message"].as_str().unwrap().contains("UNIQUEID"));
        assert!(error["error"]["hint"].as_str().is_some_and(|h| h.contains("Altium")), "{error}");
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }
}
