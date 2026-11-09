//! Project and OutJob writes change only the bytes of the entries they edit, whatever
//! `--codepage` says.
mod common;

use common::*;

/// `副本` in GBK, as a Chinese Windows stores it in printer names and parameter values.
const GBK: &[u8] = b"\xb8\xb1\xb1\xbe";
/// `С питание LCD от 3.3В` in windows-1251, as a Russian Windows stores a variant description.
const CP1251: &[u8] = b"\xd1 \xef\xe8\xf2\xe0\xed\xe8\xe5 LCD \xee\xf2 3.3\xc2";

fn crlf(lines: &[&[u8]]) -> Vec<u8> {
    lines.iter().flat_map(|l| [*l, b"\r\n"].concat()).collect()
}

fn project_lines(rev: &[u8], variant: &[&[u8]]) -> Vec<u8> {
    let printer = [&b"TargetPrinter=FX DocuPrint M268 dw ("[..], GBK, b" 1)"].concat();
    let description = [&b"Description="[..], CP1251].concat();
    let mut lines: Vec<&[u8]> = vec![
        b"[Design]",
        b"Version=1.0",
        b"",
        b"[Document1]",
        b"DocumentPath=Power.SchDoc",
        b"",
        b"[Parameter1]",
        b"Name=Rev",
        rev,
        b"",
        b"[OutputGroup1]",
        b"Name=Netlist Outputs",
        &printer,
        b"",
        b"[ProjectVariant1]",
        b"UniqueID=6DFAB600",
        &description,
        b"AllowFabrication=0",
        b"ParameterCount=0",
    ];
    lines.extend(variant);
    crlf(&lines)
}

/// Bytes as escaped text, so a failed comparison shows which line changed.
fn shown(bytes: &[u8]) -> String {
    bytes.escape_ascii().to_string().replace("\\n", "\\n\n")
}

fn read(path: &str) -> String {
    shown(&std::fs::read(path).unwrap())
}

fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> String {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    s(&path)
}

#[test]
fn project_edits_under_a_lossy_codepage_keep_untouched_bytes() {
    let gbk_override = [
        &b"ParamVariation2=ParameterName=Comment|VariantValue="[..],
        GBK,
    ]
    .concat();
    let variant: &[&[u8]] = &[
        b"VariationCount=2",
        b"Variation1=Designator=R1|UniqueId=\\AAAAAAAA|Kind=0|AlternatePart=",
        b"ParamDesignator1=R1",
        b"ParamVariation1=ParameterName=Comment|VariantValue=10k",
        b"Variation2=Designator=U9|UniqueId=\\WCFWLFNF|Kind=0|AlternatePart=",
        b"ParamDesignator2=U9",
        &gbk_override,
        b"ParamVariationCount=2",
    ];
    let original = project_lines(b"Value=A", variant);
    let dir = tempdir("ini-bytes-project");

    let p = write(&dir, "edit.PrjPcb", &original);
    ok(&[
        "project",
        "set-parameter",
        &p,
        "Rev",
        "B",
        "--codepage",
        "utf-8",
    ]);
    assert_eq!(read(&p), shown(&project_lines(b"Value=B", variant)));

    // Dropping R1 renumbers U9's override; its GBK value keeps its bytes.
    let p = write(&dir, "clear.PrjPcb", &original);
    ok(&[
        "project",
        "clear-variation",
        &p,
        "1",
        "R1",
        "--codepage",
        "utf-8",
    ]);
    let renumbered = [
        &b"ParamVariation1=ParameterName=Comment|VariantValue="[..],
        GBK,
    ]
    .concat();
    let expected = project_lines(
        b"Value=A",
        &[
            b"VariationCount=1",
            b"Variation1=Designator=U9|UniqueId=\\WCFWLFNF|Kind=0|AlternatePart=",
            b"ParamDesignator1=U9",
            &renumbered,
            b"ParamVariationCount=1",
        ],
    );
    assert_eq!(read(&p), shown(&expected));
}

#[test]
fn outjob_edits_under_a_lossy_codepage_keep_untouched_bytes() {
    let printer = [&b"TargetPrinter=FX DocuPrint M268 dw ("[..], GBK, b" 1)"].concat();
    // `Печатка 100х60` in windows-1251.
    let medium = b"OutputMedium1=\xcf\xe5\xf7\xe0\xf2\xea\xe0 100\xf560";
    let job = |variant: &[u8], enabled: &[u8]| {
        crlf(&[
            b"[OutputJobFile]",
            b"Version=1.0",
            b"",
            b"[OutputGroup1]",
            b"Name=Job1",
            b"Description=",
            &printer,
            variant,
            medium,
            b"OutputMedium1_Type=Printer",
            b"OutputType1=Gerber",
            b"OutputName1=Gerber Files",
            b"OutputDocumentPath1=Board.PcbDoc",
            enabled,
        ])
    };
    let dir = tempdir("ini-bytes-outjob");
    let j = write(
        &dir,
        "Job1.OutJob",
        &job(b"VariantName=Default", b"OutputEnabled1_OutputMedium1=0"),
    );
    ok(&["outjob", "set-variant", &j, "QA", "--codepage", "utf-8"]);
    assert_eq!(
        read(&j),
        shown(&job(b"VariantName=QA", b"OutputEnabled1_OutputMedium1=0"))
    );
    ok(&[
        "outjob",
        "enable",
        &j,
        "1",
        "--medium",
        "1",
        "--codepage",
        "utf-8",
    ]);
    assert_eq!(
        read(&j),
        shown(&job(b"VariantName=QA", b"OutputEnabled1_OutputMedium1=1"))
    );
}

#[test]
fn values_the_codepage_cannot_store_are_refused() {
    let original = project_lines(b"Value=A", &[b"VariationCount=0", b"ParamVariationCount=0"]);
    let dir = tempdir("ini-bytes-refuse");
    let p = write(&dir, "refuse.PrjPcb", &original);
    // U+FFFD is how undecodable bytes read; writing it back would replace those bytes.
    let printer = "FX DocuPrint M268 dw (\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD} 2)";
    for call in [
        vec!["project", "set-parameter", &p, "Rev", "한국"],
        vec![
            "project",
            "set-parameter",
            &p,
            "Rev",
            "\u{FFFD}",
            "--codepage",
            "utf-8",
        ],
        vec![
            "project",
            "set-option",
            &p,
            "OutputGroup1",
            "TargetPrinter",
            printer,
            "--codepage",
            "utf-8",
        ],
    ] {
        let error = err(&call, "UNSAFE");
        assert!(
            error["error"]["hint"]
                .as_str()
                .is_some_and(|h| h.contains("--codepage")),
            "{error}"
        );
        assert_eq!(read(&p), shown(&original));
        assert!(!dir.join("refuse.PrjPcb.bak").exists());
    }
}

#[test]
fn line_endings_bom_comments_and_spacing_survive_edits() {
    let lines: &[&[u8]] = &[
        b"[Design]",
        b"Version=1.0",
        b"",
        b"[Parameter1]",
        b"Name=Rev",
        b"Value=A",
        b"",
        b"[Document1]",
        b"DocumentPath=Power.SchDoc",
    ];
    let join = |nl: &[u8]| {
        lines
            .iter()
            .flat_map(|l| [*l, nl].concat())
            .collect::<Vec<u8>>()
    };
    let crlf_file = join(b"\r\n");
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("crlf", crlf_file.clone()),
        ("lf", join(b"\n")),
        ("bom", [&b"\xef\xbb\xbf"[..], &crlf_file].concat()),
        ("no-final-newline", crlf_file[..crlf_file.len() - 2].to_vec()),
        ("mixed", b"[Design]\r\nVersion=1.0\n\r\n[Parameter1]\nName=Rev\r\nValue=A\n\n[Document1]\r\nDocumentPath=Power.SchDoc\n".to_vec()),
        (
            "comments",
            b"; hand-written note\r\n[Design]\r\nVersion=1.0\r\n\r\n\r\n; release revision\r\n[Parameter1]\r\nName=Rev\r\n# reviewed\r\nValue=A\r\n  \r\n[Document1]\r\nDocumentPath=Power.SchDoc\r\n\r\n\r\n"
                .to_vec(),
        ),
    ];
    let dir = tempdir("ini-bytes-layout");
    for (name, original) in files {
        let p = write(&dir, &format!("{name}.PrjPcb"), &original);
        ok(&["project", "set-parameter", &p, "Rev", "B"]);
        let at = original.windows(7).position(|w| w == b"Value=A").unwrap();
        assert_eq!(
            read(&p),
            shown(&[&original[..at], b"Value=B", &original[at + 7..]].concat()),
            "{name}"
        );
        ok(&["project", "set-parameter", &p, "Rev", "A"]);
        assert_eq!(read(&p), shown(&original), "{name}: set back");
        ok(&["project", "set-parameter", &p, "Stage", "Proto"]);
        ok(&["project", "remove-parameter", &p, "Stage"]);
        assert_eq!(read(&p), shown(&original), "{name}: added and removed");
    }

    // A line added after an unterminated last line gets the line break it lacked.
    let p = write(&dir, "append.PrjPcb", &crlf_file[..crlf_file.len() - 2]);
    ok(&[
        "project",
        "set-option",
        &p,
        "Document1",
        "AnnotationEnabled",
        "1",
    ]);
    assert_eq!(
        read(&p),
        shown(&[&crlf_file[..], b"AnnotationEnabled=1\r\n"].concat())
    );
    let p = write(
        &dir,
        "append-section.PrjPcb",
        &crlf_file[..crlf_file.len() - 2],
    );
    ok(&["project", "set-option", &p, "Extra", "Key", "1"]);
    assert_eq!(
        read(&p),
        shown(&[&crlf_file[..], b"\r\n[Extra]\r\nKey=1\r\n"].concat())
    );
}

/// The sample projects saved on Chinese and Russian Windows, edited with the wrong code page.
#[test]
fn sample_projects_keep_every_original_line() {
    let Some(root) = samples() else { return };
    let dir = tempdir("ini-bytes-samples");
    for sample in [
        "AirPi_PCB/AirPi_Power/AirPi_Power.PrjPcb",
        "quasar_arm_pcb/Quasar ARM.PrjPcb",
    ] {
        let copy = copy_to(&root.join(sample), &dir);
        let original = std::fs::read(&copy).unwrap();
        ok(&[
            "project",
            "set-parameter",
            &s(&copy),
            "Rev",
            "B",
            "--codepage",
            "utf-8",
        ]);
        let edited = std::fs::read(&copy).unwrap();
        let mut out = edited.split(|&b| b == b'\n');
        for line in original.split(|&b| b == b'\n') {
            assert!(
                out.any(|l| l == line),
                "{sample}: {:?} changed or moved",
                String::from_utf8_lossy(line)
            );
        }
    }
}
