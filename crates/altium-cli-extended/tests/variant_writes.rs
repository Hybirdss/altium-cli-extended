//! Altium attaches parameter overrides to component variations, including fitted parts.

mod common;

use common::*;

#[test]
fn parameter_rows_follow_their_own_component() {
    let dir = tempdir("variant-param-owner");
    let project = dir.join("Owner.PrjPcb");
    std::fs::write(&project, b"[Design]\r\nVersion=1.0\r\n").unwrap();
    let p = s(&project);
    ok(&["project", "add-variant", &p, "Owner"]);
    for (part, uid) in [("R1", "\\FIRSTAAA"), ("R2", "\\SECONDAA")] {
        ok(&["project", "set-variation", &p, "Owner", part, "--fitted", "--unique-id", uid]);
    }
    ok(&["project", "set-variant-param", &p, "Owner", "R2", "Comment", "SECOND"]);
    ok(&["project", "set-variant-param", &p, "Owner", "R1", "Comment", "FIRST"]);
    let text = std::fs::read_to_string(project).unwrap();
    let expected = "Variation1=Designator=R1|UniqueId=\\FIRSTAAA|Kind=0|AlternatePart=\r\nParamDesignator1=R1\r\nParamVariation1=ParameterName=Comment|VariantValue=FIRST\r\nVariation2=";
    assert!(text.contains(expected), "expected {expected:?} in:\n{text}");
}

#[test]
fn parameter_count_follows_the_overrides() {
    let dir = tempdir("variant-param-count");
    let project = dir.join("Order.PrjPcb");
    std::fs::write(&project, b"[Design]\r\nVersion=1.0\r\n").unwrap();
    let p = s(&project);
    ok(&["project", "add-variant", &p, "Order"]);
    ok(&["project", "set-variation", &p, "Order", "P8", "--fitted", "--unique-id", "\\ROHJKUCQ"]);
    ok(&["project", "set-variant-param", &p, "Order", "P8", "Comment", "ROUNDTRIP_HDR"]);
    let text = std::fs::read_to_string(project).unwrap();
    // AD 26 reads no overrides when the count precedes the parameter rows.
    assert_eq!(text.lines().rfind(|line| !line.is_empty()), Some("ParamVariationCount=1"), "{text}");
}

#[test]
fn parameter_override_creates_the_fitted_component_variation() {
    let Some(root) = samples() else { return };
    for with_pcb in [true, false] {
        let dir = tempdir(if with_pcb { "variant-param-pcb" } else { "variant-param-sch" });
        for ext in ["PrjPcb", "SchDoc"].into_iter().chain(with_pcb.then_some("PcbDoc")) {
            copy_to(&root.join(format!("BeagleBoneCape/BeagleBoneCape.{ext}")), &dir);
        }
        let project = dir.join("BeagleBoneCape.PrjPcb");
        let p = s(&project);
        ok(&["project", "add-variant", &p, "NativeRoundtrip"]);
        ok(&["project", "set-variation", &p, "NativeRoundtrip", "P9"]);
        ok(&["project", "set-variant-param", &p, "NativeRoundtrip", "P8", "Comment", "ROUNDTRIP_HDR"]);

        // AD 26 needs a component variation for the target; otherwise an override
        // can attach to another part.
        let v = ok(&["project", "variant", &p, "NativeRoundtrip"]);
        assert_eq!(v["variations"].as_array().unwrap().len(), 2, "{v}");
        assert_eq!(v["variations"][0]["designator"], "P9");
        assert_eq!(v["variations"][0]["kind"], "not_fitted");
        assert_eq!(v["variations"][1]["designator"], "P8");
        assert_eq!(v["variations"][1]["kind"], "fitted");
        assert_eq!(v["variations"][1]["unique_id"], "\\ROHJKUCQ");
        assert_eq!(v["param_variations"][0]["VariantValue"], "ROUNDTRIP_HDR");

        ok(&["project", "set-variant-param", &p, "NativeRoundtrip", "p8", "Comment", "HDR2"]);
        ok(&["project", "set-variant-param", &p, "NativeRoundtrip", "P9", "Comment", "DNP_HDR"]);
        let updated = ok(&["project", "variant", &p, "NativeRoundtrip"]);
        assert_eq!(updated["variations"], v["variations"]);
        let p8 = updated["param_variations"].as_array().unwrap().iter().find(|p| p["Designator"] == "P8").unwrap();
        assert_eq!(p8["VariantValue"], "HDR2");
        assert_eq!(updated["param_variations"].as_array().unwrap().len(), 2);

        // Repair a file emitted by the old writer even if the requested value is unchanged.
        ok(&["project", "set-option", &p, "ProjectVariant1", "VariationCount", "1"]);
        let repaired = ok(&["project", "set-variant-param", &p, "NativeRoundtrip", "P8", "Comment", "HDR2"]);
        assert_eq!(repaired["changed"], true);
        assert_eq!(repaired["before"], serde_json::Value::Null);
        assert_eq!(ok(&["project", "variant", &p, "NativeRoundtrip"])["variations"], v["variations"]);

        let before = std::fs::read(&project).unwrap();
        err(&["project", "set-variant-param", &p, "NativeRoundtrip", "Q99", "Comment", "bad"], "NOT_FOUND");
        assert_eq!(std::fs::read(&project).unwrap(), before);
    }
}
