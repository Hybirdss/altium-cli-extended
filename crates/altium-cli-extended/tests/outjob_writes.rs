//! OutJob writes must survive the same INI decoding used by the read commands.
mod common;

use common::*;

#[test]
fn outjob_rejects_values_that_do_not_roundtrip() {
    let dir = tempdir("outjob-roundtrip");
    let file = dir.join("test.OutJob");
    let original = b"[OutputGroup1]\r\nOutputMedium1=Files\r\nOutputMedium1_Type=GeneratedFiles\r\nOutputType1=Gerber\r\nOutputName1=Gerbers\r\nOutputDocumentPath1=Board.PcbDoc\r\nVariantName=Default\r\n";
    std::fs::write(&file, original).unwrap();
    let f = s(&file);
    for dry in [false, true] {
        for (command, args) in [
            ("set-document", vec!["all", "QA.PcbDoc\nInjected=value"]),
            ("set-variant", vec!["QA\r\n[Injected]\r\nKey=value"]),
            ("set-variant", vec!["한국어"]),
        ] {
            let mut call = vec!["outjob", command, &f];
            call.extend(args);
            if dry {
                call.push("--dry-run");
            }
            let error = err(&call, "UNSAFE");
            assert!(error["error"]["hint"].as_str().is_some_and(|h| !h.is_empty()));
            assert_eq!(std::fs::read(&file).unwrap(), original);
            assert!(!dir.join("test.OutJob.bak").exists());
        }
    }
    ok(&["outjob", "set-document", &f, "all", "QA.PcbDoc"]);
    assert_eq!(ok(&["outjob", "outputs", &f])["items"][0]["document"], "QA.PcbDoc");
    ok(&["outjob", "set-variant", &f, "QA"]);
    assert_eq!(ok(&["outjob", "info", &f])["group"]["VariantName"], "QA");
    ok(&["outjob", "enable", &f, "1", "--medium", "1"]);
    assert_eq!(ok(&["outjob", "outputs", &f])["items"][0]["enabled_for"], serde_json::json!(["Files"]));
}
