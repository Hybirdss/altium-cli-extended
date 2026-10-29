mod common;
use common::*;
#[test]
fn pin_identity_and_unconnected_are_explicit() {
    let Some(root) = samples() else { return };
    let prj = s(&root.join("BeagleBoneCape/BeagleBoneCape.PrjPcb"));
    let sch = s(&root.join("BeagleBoneCape/BeagleBoneCape.SchDoc"));
    let d = ok(&["sch", "pins", &sch, "--component", "U?", "--project", &prj, "--unconnected", "--fields", "pin,name,net,connected"]);
    assert_eq!(d["count"], 6);
    for row in d["items"].as_array().unwrap() {
        assert!(row.get("net").unwrap().is_null());
        assert_eq!(row["connected"], false);
        assert!(row["pin"].is_string());
    }
    let d = ok(&["sch", "pins", &sch, "--component", "U?", "--project", &prj, "--where", "pin=4", "--fields", "pin,net"]);
    assert_eq!(d["items"][0]["net"], "GND");
}
