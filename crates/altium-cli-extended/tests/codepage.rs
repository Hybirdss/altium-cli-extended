//! Text stored in another code page than `--codepage` is flagged, and no write mixes two code
//! pages in one file.
mod common;

use common::*;
use serde_json::Value;

fn warns_about(v: &Value, codepage: &str) -> bool {
    let hint = format!("--codepage {codepage}");
    v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap().contains(&hint))
}

fn crlf(lines: &[&[u8]]) -> Vec<u8> {
    lines.iter().flat_map(|l| [*l, b"\r\n"].concat()).collect()
}

fn project(dir: &std::path::Path, description: &[u8]) -> std::path::PathBuf {
    let description = [&b"Description="[..], description].concat();
    let path = dir.join("Board.PrjPcb");
    let lines: &[&[u8]] = &[
        b"[Design]",
        b"Version=1.0",
        b"",
        b"[ProjectVariant1]",
        b"UniqueID=6DFAB600",
        &description,
        b"AllowFabrication=0",
        b"ParameterCount=0",
    ];
    std::fs::write(&path, crlf(lines)).unwrap();
    path
}

#[test]
fn schematic_write_that_would_mix_code_pages_is_refused() {
    let Some(root) = samples() else { return };
    let dir = tempdir("codepage-sch");
    let sheet = copy_to(&root.join("AirPi_PCB/Utils/LM2596.SchDoc"), &dir);
    let f = s(&sheet);
    let original = std::fs::read(&sheet).unwrap();

    let v = err(
        &[
            "sch",
            "set-param",
            &f,
            "R1",
            "Comment",
            "22KΩ ±1% 한국",
            "--literal",
        ],
        "UNSAFE",
    );
    assert!(
        v["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("--codepage gbk"),
        "{v}"
    );
    assert_eq!(std::fs::read(&sheet).unwrap(), original);

    ok(&[
        "sch",
        "set-param",
        &f,
        "R1",
        "Comment",
        "22KΩ ±1% 한국",
        "--literal",
        "--codepage",
        "gbk",
    ]);
    let written = std::fs::read(&sheet).unwrap();
    // Ω and ± in GB2312; GBK has no Hangul.
    let fallback = b"|TEXT=22K\xa6\xb8 \xa1\xc01% ??";
    assert!(
        written
            .windows(fallback.len())
            .any(|w| w.eq_ignore_ascii_case(fallback)),
        "ANSI fallback is not GBK"
    );
}

#[test]
fn pcb_parameters_without_utf8_copies_suggest_gbk() {
    let Some(root) = samples() else { return };
    let pcb = root.join("AirPi_PCB/AirPi_Power/AirPi_Power.PcbDoc");
    let (_, v) = run(&["pcb", "components", &s(&pcb), "--where", "designator=U2"]);
    assert!(warns_about(&v, "gbk"), "{v}");
    assert_eq!(v["meta"]["suggest"]["codepage"], "gbk");

    let (_, v) = run(&[
        "pcb",
        "components",
        &s(&pcb),
        "--where",
        "designator=U2",
        "--codepage",
        "gbk",
    ]);
    assert_eq!(
        v["data"]["items"][0]["parameters"]["ComponentLink1Description"],
        "供应商链接"
    );
    assert_eq!(v["warnings"], serde_json::json!([]));

    let dir = tempdir("codepage-pcb");
    let copy = copy_to(&pcb, &dir);
    let original = std::fs::read(&copy).unwrap();
    let v = err(
        &["pcb", "set-param", &s(&copy), "U2", "Note", "±5%"],
        "UNSAFE",
    );
    assert!(
        v["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("--codepage windows-1252"),
        "{v}"
    );
    assert_eq!(std::fs::read(&copy).unwrap(), original);
}

#[test]
fn naming_the_code_page_overrides_a_guess() {
    let Some(root) = samples() else { return };
    let dir = tempdir("codepage-explicit");
    let copy = copy_to(&root.join("AirPi_PCB/AirPi_Power/AirPi_Power.PcbDoc"), &dir);
    let (_, v) = run(&[
        "pcb",
        "components",
        &s(&copy),
        "--where",
        "designator=U2",
        "--codepage",
        "windows-1252",
    ]);
    assert!(!warns_about(&v, "gbk"), "{v}");
    assert!(v["meta"]["suggest"].is_null(), "{v}");
    ok(&[
        "pcb",
        "set-param",
        &s(&copy),
        "U2",
        "Note",
        "±5%",
        "--codepage",
        "windows-1252",
    ]);
}

#[test]
fn outjob_names_in_cp1251_suggest_windows_1251() {
    let Some(root) = samples() else { return };
    let job = s(&root.join("quasar_arm_pcb/Job1.OutJob"));
    let (_, v) = run(&["outjob", "mediums", &job]);
    assert!(warns_about(&v, "windows-1251"), "{v}");
    assert_eq!(v["meta"]["suggest"]["codepage"], "windows-1251");

    let d = ok(&["outjob", "mediums", &job, "--codepage", "windows-1251"]);
    assert!(
        d["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "Печатка 100х60"),
        "{d}"
    );
}

#[test]
fn accented_latin_text_is_not_flagged() {
    let dir = tempdir("codepage-latin");
    // windows-1252: "Résistance über Kühlkörper, Größe ±5% 25°C"
    let p = s(&project(
        &dir,
        b"R\xe9sistance \xfcber K\xfchlk\xf6rper, Gr\xf6\xdfe \xb15% 25\xb0C",
    ));
    let (_, v) = run(&["project", "variants", &p]);
    assert_eq!(v["ok"], true, "{v}");
    assert!(!v["warnings"].to_string().contains("--codepage"), "{v}");
    assert_eq!(v["meta"]["suggest"], Value::Null);
    ok(&["project", "set-parameter", &p, "Owner", "Müller"]);
}

#[test]
fn project_write_that_would_mix_code_pages_is_refused() {
    let dir = tempdir("codepage-project");
    // windows-1251: "Печатка 100х60"
    let path = project(&dir, b"\xcf\xe5\xf7\xe0\xf2\xea\xe0 100\xf560");
    let p = s(&path);
    let original = std::fs::read(&path).unwrap();
    let v = err(
        &["project", "set-parameter", &p, "Owner", "Müller"],
        "UNSAFE",
    );
    assert!(
        v["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("--codepage windows-1251"),
        "{v}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    ok(&["project", "set-parameter", &p, "Revision", "B"]);
}
