mod common;
use common::*;

#[test]
fn annotation_rename_keeps_simultaneous_parameter_changes() {
    let Some(root) = samples() else { return };
    let dir = tempdir("identity-diff");
    let before = root.join("BeagleBoneCape/BeagleBoneCape.SchDoc");
    let after = copy_to(&before, &dir);
    let (a, b) = (s(&before), s(&after));
    ok(&["sch", "set-designator", &b, "U?", "U1"]);
    ok(&["sch", "set-param", &b, "U1", "MPN", "CAT24C256WI-GT3"]);
    let d = ok(&["diff", &a, &b]);
    assert_eq!(d["summary"]["parts_added"], 0);
    assert_eq!(d["summary"]["parts_removed"], 0);
    assert_eq!(d["summary"]["parts_renamed"], 1);
    let items = d["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|r| r["change"] == "part_renamed" && r["ref"] == "U1" && r["before_ref"] == "U?")
    );
    assert!(
        items.iter().any(|r| r["change"] == "part_changed"
            && r["ref"] == "U1"
            && r["field"] == "param:MPN")
    );
    assert_eq!(
        d["summary"]["nets_changed"], 0,
        "annotation alone must not look like a rewire"
    );
    let old = ok(&["diff", &a, &b, "--match", "designator"]);
    assert_eq!(old["summary"]["parts_added"], 1);
    assert_eq!(old["summary"]["parts_removed"], 1);
}

#[test]
fn project_variant_and_library_edits_are_visible() {
    let Some(root) = samples() else { return };
    let dir = tempdir("artifact-diff");
    let base = root.join("BeagleBoneCape");
    for name in [
        "BeagleBoneCape.PrjPcb",
        "BeagleBoneCape.SchDoc",
        "BeagleBoneCape.PcbDoc",
    ] {
        copy_to(&base.join(name), &dir);
    }
    let before = s(&base.join("BeagleBoneCape.PrjPcb"));
    let after = s(&dir.join("BeagleBoneCape.PrjPcb"));
    ok(&["project", "add-variant", &after, "Lite"]);
    let d = ok(&["diff", &before, &after]);
    assert!(
        d["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["change"] == "variant_added" && r["ref"] == "Lite")
    );
    assert_eq!(
        d["summary"]["project_setting_changed"]
            .as_u64()
            .unwrap_or(0),
        0
    );
    let variant_before = dir.join("Before.PrjPcb");
    std::fs::copy(&after, &variant_before).unwrap();
    ok(&["project", "set-variation", &after, "Lite", "P9"]);
    let full = ok(&["diff", &before, &after]);
    assert!(
        full["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["change"] == "variation_changed"
                && r["ref"] == "P9"
                && r["before"].is_null())
    );
    assert_eq!(
        full["summary"]["project_setting_changed"]
            .as_u64()
            .unwrap_or(0),
        0
    );
    let d = ok(&["diff", &s(&variant_before), &after]);
    assert!(
        d["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["change"] == "variation_changed" && r["ref"] == "P9")
    );
    assert_eq!(
        d["summary"]["project_setting_changed"]
            .as_u64()
            .unwrap_or(0),
        0
    );
    let lib = base.join("BeagleBoneCape.PcbLib");
    let edited = s(&copy_to(&lib, &dir));
    ok(&["pcblib", "set-param", &edited, "SOIC-8", "HEIGHT", "2.5mm"]);
    let d = ok(&["diff", &s(&lib), &edited]);
    assert_eq!(d["identical"], false);
    assert!(
        d["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["change"] == "param_changed"
                && r["ref"] == "SOIC-8"
                && r["field"] == "height")
    );
    let lib = base.join("BeagleBoneCape.SchLib");
    let edited = s(&copy_to(&lib, &dir));
    ok(&[
        "schlib",
        "set-param",
        &edited,
        "CAT24C256",
        "MPN",
        "Changed",
    ]);
    let d = ok(&["diff", &s(&lib), &edited]);
    assert_eq!(d["identical"], false);
    assert!(
        d["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["change"] == "param_changed" && r["ref"] == "CAT24C256")
    );
}

#[test]
fn diff_views_group_fields_preserve_gates_and_keep_rule_types() {
    let Some(root) = samples() else { return };
    let dir = tempdir("diff-views");
    let before = root.join("BeagleBoneCape/BeagleBoneCape.SchDoc");
    let after = copy_to(&before, &dir);
    let (a, b) = (s(&before), s(&after));
    ok(&["sch", "set-param", &b, "P8", "MPN", "new"]);
    ok(&["sch", "set-param", &b, "P8", "Tolerance", "1%"]);
    let d = ok(&["diff", &a, &b, "--by", "part"]);
    let row = d["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["ref"] == "P8" && r["change"] == "part_changed")
        .unwrap();
    assert_eq!(row["fields"].as_array().unwrap().len(), 2);
    let (exit, v) = run(&["diff", &a, &b, "--summary", "--exit-code"]);
    assert_eq!(exit, 9);
    assert!(v["data"].get("items").is_none());
    assert_eq!(v["data"]["summary"]["by_field"]["param:MPN"], 1);
    let d = ok(&["diff", &a, &b, "--ignore-field", "^param:", "--exit-code"]);
    assert_eq!(d["identical"], true);
    let d = ok(&["diff", &a, &b, "--only", "mpn"]);
    assert_eq!(d["items"].as_array().unwrap().len(), 1);
    err(&["diff", &a, &b, "--ignore-field", "["], "USAGE");
    let before = root.join("BeagleBoneCape/BeagleBoneCape.PcbDoc");
    let after = copy_to(&before, &dir);
    ok(&["pcb", "set-rule", &s(&after), "Clearance", "gap", "0.2mm"]);
    let d = ok(&["diff", &s(&before), &s(&after)]);
    let gap = d["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["field"] == "gap")
        .unwrap();
    assert!(gap["before"].is_number());
    assert!(gap["after"].is_number());
    assert_eq!(gap["unit"], "mm");
}

struct WriteCase {
    command: &'static str,
    files: &'static [&'static str],
    target: &'static str,
    setup: &'static [&'static [&'static str]],
    edit: &'static [&'static str],
    change: &'static str,
    /// The row's `field`, or a prefix of its `ref` for rows without one.
    names: &'static str,
}

const BBC: &[&str] = &[
    "BeagleBoneCape/BeagleBoneCape.PrjPcb",
    "BeagleBoneCape/BeagleBoneCape.SchDoc",
    "BeagleBoneCape/BeagleBoneCape.PcbDoc",
    "BeagleBoneCape/BeagleBoneCape.SchLib",
    "BeagleBoneCape/BeagleBoneCape.PcbLib",
];
const BBC_SCH: &[&str] = &["BeagleBoneCape/BeagleBoneCape.SchDoc"];
const AIRPI_PCB: &[&str] = &["AirPi_PCB/AirPi_Power/AirPi_Power.PcbDoc"];

fn case(
    command: &'static str,
    files: &'static [&'static str],
    edit: &'static [&'static str],
    change: &'static str,
    names: &'static str,
) -> WriteCase {
    let target = files[0].rsplit('/').next().unwrap();
    WriteCase {
        command,
        files,
        target,
        setup: &[],
        edit,
        change,
        names,
    }
}

fn lib(
    command: &'static str,
    edit: &'static [&'static str],
    change: &'static str,
    names: &'static str,
) -> WriteCase {
    case(
        command,
        if command.starts_with("pcblib") {
            &BBC[4..]
        } else {
            &BBC[3..4]
        },
        edit,
        change,
        names,
    )
}

fn write_cases() -> Vec<WriteCase> {
    vec![
        case(
            "sch set-param",
            BBC_SCH,
            &["sch", "set-param", "@", "P8", "MPN", "X1"],
            "part_changed",
            "param:MPN",
        ),
        WriteCase {
            setup: &[&["sch", "set-param", "@", "P8", "MPN", ""]],
            ..case(
                "sch remove-param",
                BBC_SCH,
                &["sch", "remove-param", "@", "P8", "MPN"],
                "part_changed",
                "param:MPN",
            )
        },
        case(
            "sch set-comment",
            BBC_SCH,
            &["sch", "set-comment", "@", "P8", "HDR"],
            "part_changed",
            "comment",
        ),
        case(
            "sch set-footprint",
            BBC_SCH,
            &["sch", "set-footprint", "@", "P8", "HDR2X23"],
            "part_changed",
            "footprint",
        ),
        case(
            "sch set-designator",
            BBC_SCH,
            &["sch", "set-designator", "@", "U?", "U1"],
            "part_renamed",
            "U1",
        ),
        case(
            "sch set-sheet-param",
            BBC_SCH,
            &["sch", "set-sheet-param", "@", "Revision", "C"],
            "sheet_param_changed",
            "Revision",
        ),
        case(
            "pcb set-rule",
            AIRPI_PCB,
            &["pcb", "set-rule", "@", "Clearance", "gap", "0.3mm"],
            "rule_changed",
            "gap",
        ),
        case(
            "pcb set-component",
            AIRPI_PCB,
            &[
                "pcb",
                "set-component",
                "@",
                "U2",
                "kind",
                "Standard (No BOM)",
            ],
            "part_changed",
            "kind",
        ),
        case(
            "pcb set-component",
            AIRPI_PCB,
            &[
                "pcb",
                "set-component",
                "@",
                "U2",
                "kind",
                "Standard (No BOM)",
            ],
            "part_changed",
            "exclude_from_bom",
        ),
        case(
            "pcb set-component",
            AIRPI_PCB,
            &[
                "pcb",
                "set-component",
                "@",
                "U2",
                "description",
                "regulator",
            ],
            "part_changed",
            "description",
        ),
        case(
            "pcb set-component",
            AIRPI_PCB,
            &["pcb", "set-component", "@", "U2", "locked", "false"],
            "part_changed",
            "locked",
        ),
        case(
            "pcb set-component",
            AIRPI_PCB,
            &[
                "pcb",
                "set-component",
                "@",
                "U2",
                "designator_visible",
                "true",
            ],
            "part_changed",
            "designator_visible",
        ),
        case(
            "pcb set-component",
            AIRPI_PCB,
            &["pcb", "set-component", "@", "U2", "comment_visible", "true"],
            "part_changed",
            "comment_visible",
        ),
        case(
            "pcb set-component",
            AIRPI_PCB,
            &["pcb", "set-component", "@", "U2", "height", "5mm"],
            "part_changed",
            "height",
        ),
        case(
            "pcb set-component",
            AIRPI_PCB,
            &[
                "pcb",
                "set-component",
                "@",
                "U2",
                "footprint_description",
                "TO-263",
            ],
            "part_changed",
            "footprint_description",
        ),
        case(
            "pcb set-param",
            AIRPI_PCB,
            &["pcb", "set-param", "@", "U2", "MPN", "LM2596SX-5.0"],
            "part_changed",
            "param:MPN",
        ),
        case(
            "pcb remove-param",
            AIRPI_PCB,
            &["pcb", "remove-param", "@", "U2", "Notepad"],
            "part_changed",
            "param:Notepad",
        ),
        case(
            "pcb class-add",
            AIRPI_PCB,
            &["pcb", "class-add", "@", "V_div", "U2"],
            "class_changed",
            "V_div",
        ),
        case(
            "pcb class-remove",
            AIRPI_PCB,
            &["pcb", "class-remove", "@", "PWM[15..0]", "PWM15"],
            "class_changed",
            "PWM[15..0]",
        ),
        case(
            "pcb add-class",
            AIRPI_PCB,
            &[
                "pcb",
                "add-class",
                "@",
                "PWR",
                "PWM0",
                "PWM1",
                "--kind",
                "net",
            ],
            "class_added",
            "PWR",
        ),
        case(
            "project set-parameter",
            BBC,
            &["project", "set-parameter", "@", "Revision", "B"],
            "project_setting_changed",
            "Parameter1.",
        ),
        WriteCase {
            setup: &[&["project", "set-parameter", "@", "Revision", "B"]],
            ..case(
                "project remove-parameter",
                BBC,
                &["project", "remove-parameter", "@", "Revision"],
                "project_setting_changed",
                "Parameter1.",
            )
        },
        case(
            "project set-option",
            BBC,
            &["project", "set-option", "@", "Design", "OutputPath", "Out"],
            "project_setting_changed",
            "Design.OutputPath",
        ),
        WriteCase {
            setup: &[&["project", "remove-document", "@", "BeagleBoneCape.SchLib"]],
            ..case(
                "project add-document",
                BBC,
                &["project", "add-document", "@", "BeagleBoneCape.SchLib"],
                "project_setting_changed",
                "Document",
            )
        },
        case(
            "project remove-document",
            BBC,
            &["project", "remove-document", "@", "BeagleBoneCape.SchLib"],
            "project_setting_changed",
            "Document",
        ),
        case(
            "project add-variant",
            BBC,
            &["project", "add-variant", "@", "Lite"],
            "variant_added",
            "Lite",
        ),
        WriteCase {
            setup: &[&["project", "add-variant", "@", "Lite"]],
            ..case(
                "project remove-variant",
                BBC,
                &["project", "remove-variant", "@", "Lite"],
                "variant_removed",
                "Lite",
            )
        },
        WriteCase {
            setup: &[&["project", "add-variant", "@", "Lite"]],
            ..case(
                "project set-variation",
                BBC,
                &["project", "set-variation", "@", "Lite", "P9"],
                "variation_changed",
                "P9",
            )
        },
        WriteCase {
            setup: &[&["project", "add-variant", "@", "Lite"]],
            ..case(
                "project set-variant-param",
                BBC,
                &[
                    "project",
                    "set-variant-param",
                    "@",
                    "Lite",
                    "P8",
                    "Comment",
                    "X",
                ],
                "variation_changed",
                "P8",
            )
        },
        WriteCase {
            setup: &[
                &["project", "add-variant", "@", "Lite"],
                &["project", "set-variation", "@", "Lite", "P9"],
            ],
            ..case(
                "project clear-variation",
                BBC,
                &["project", "clear-variation", "@", "Lite", "P9"],
                "variation_changed",
                "P9",
            )
        },
        lib(
            "schlib set-param",
            &["schlib", "set-param", "@", "CAT24C256", "MPN", "X"],
            "param_changed",
            "parameters",
        ),
        lib(
            "schlib set-description",
            &["schlib", "set-description", "@", "CAT24C256", "EEPROM"],
            "param_changed",
            "description",
        ),
        lib(
            "pcblib rename",
            &["pcblib", "rename", "@", "SOIC-8", "SOIC-8N"],
            "footprint_added",
            "SOIC-8N",
        ),
        lib(
            "pcblib set-param",
            &["pcblib", "set-param", "@", "SOIC-8", "HEIGHT", "2.5mm"],
            "param_changed",
            "height",
        ),
        lib(
            "pcblib remove",
            &["pcblib", "remove", "@", "SOIC-8"],
            "footprint_removed",
            "SOIC-8",
        ),
        lib(
            "pcblib copy",
            &[
                "pcblib", "copy", "@", "@orig", "SOIC-8", "--name", "SOIC-8C",
            ],
            "footprint_added",
            "SOIC-8C",
        ),
    ]
}

/// Native writes that change no design data `ace diff` reads: `restore` puts back an earlier
/// file, and OutJob files hold output settings.
const NOT_DESIGN_DATA: &[&str] = &[
    "restore",
    "outjob enable",
    "outjob set-document",
    "outjob set-variant",
];

#[test]
fn every_native_write_is_a_diff_case() {
    let cmds = ok(&["commands"]);
    let writes: std::collections::BTreeSet<String> = cmds["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["effect"] == "write" && c["backend"] == "native")
        .map(|c| c["path"].as_str().unwrap().to_owned())
        .collect();
    let covered: std::collections::BTreeSet<String> = write_cases()
        .iter()
        .map(|c| c.command.to_owned())
        .chain(NOT_DESIGN_DATA.iter().map(|c| c.to_string()))
        .collect();
    assert_eq!(writes, covered);
}

#[test]
fn diff_exit_code_reports_every_native_write() {
    let Some(root) = samples() else { return };
    let dir = tempdir("diff-every-write");
    let mut missed = vec![];
    for (i, c) in write_cases().iter().enumerate() {
        let (before, after) = (dir.join(format!("{i}/a")), dir.join(format!("{i}/b")));
        for side in [&before, &after] {
            std::fs::create_dir_all(side).unwrap();
            for f in c.files {
                copy_to(&root.join(f), side);
            }
        }
        let orig = c
            .files
            .iter()
            .map(|f| root.join(f))
            .find(|f| f.ends_with(c.target))
            .unwrap();
        let args = |step: &[&str], side: &std::path::Path| -> Vec<String> {
            step.iter()
                .map(|a| match *a {
                    "@" => s(&side.join(c.target)),
                    "@orig" => s(&orig),
                    a => a.to_owned(),
                })
                .collect()
        };
        for step in c.setup {
            for side in [&before, &after] {
                ok(&args(step, side));
            }
        }
        ok(&args(c.edit, &after));
        let (exit, v) = run(&[
            "diff",
            &s(&before.join(c.target)),
            &s(&after.join(c.target)),
            "--exit-code",
        ]);
        let named = v["data"]["items"].as_array().is_some_and(|items| {
            items.iter().any(|r| {
                r["change"] == c.change
                    && (r["field"] == c.names
                        || r["field"].is_null()
                            && r["ref"].as_str().is_some_and(|x| x.starts_with(c.names)))
            })
        });
        if exit != 9 || !named {
            missed.push(format!(
                "{} -> {} {}: exit {exit}, items {}",
                c.edit.join(" "),
                c.change,
                c.names,
                v["data"]["items"]
            ));
        }
    }
    assert!(
        missed.is_empty(),
        "diff missed {} of {} writes:\n{}",
        missed.len(),
        write_cases().len(),
        missed.join("\n")
    );
    assert!(
        ok(&[
            "diff",
            &s(&root.join(BBC_SCH[0])),
            &s(&root.join(BBC_SCH[0]))
        ])["coverage"]["compared"]
            .as_array()
            .is_some_and(|c| c.iter().any(|x| x == "sheet_parameters"))
    );
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Ace test",
            "-c",
            "user.email=ace@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn revision_diff_reports_a_five_edit_revision() {
    let Some(root) = samples() else { return };
    let repo = tempdir("diff-rev-c");
    let src = root.join("PilotPi_PCB/PilotPi_Power");
    for e in std::fs::read_dir(&src).unwrap() {
        copy_to(&e.unwrap().path(), &repo);
    }
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "rev B"]);
    let project = s(&repo.join("PilotPi_Power.PrjPcb"));
    let clean = ok(&["diff", &project, "--rev", "HEAD"]);
    assert_eq!(clean["incomplete"], false, "{clean}");
    assert_eq!(clean["identical"], true, "{clean}");
    let (sch, pcb) = (
        |f: &str| s(&repo.join(f)),
        s(&repo.join("PilotPi_Power.PcbDoc")),
    );
    ok(&["sch", "set-comment", &sch("xl4015.SchDoc"), "R2", "12K"]);
    ok(&[
        "pcb",
        "set-component",
        &pcb,
        "JP1",
        "kind",
        "Standard (No BOM)",
    ]);
    ok(&["pcb", "set-rule", &pcb, "Clearance", "gap", "0.2mm"]);
    ok(&[
        "sch",
        "set-sheet-param",
        &sch("PilotPi_Power.SchDoc"),
        "Revision",
        "C",
    ]);
    ok(&[
        "pcb",
        "add-class",
        &pcb,
        "PWM_HI",
        "PWM15",
        "PWM14",
        "PWM13",
        "PWM12",
        "--kind",
        "net",
    ]);
    git(&repo, &["commit", "-qam", "rev C"]);
    let has = |d: &serde_json::Value, change: &str, r: &str, field: Option<&str>| {
        d["items"].as_array().unwrap().iter().any(|x| {
            x["change"] == change && x["ref"] == r && field.is_none_or(|f| x["field"] == f)
        })
    };
    let (exit, v) = run(&["diff", &project, "--rev", "HEAD~1", "--exit-code"]);
    let d = &v["data"];
    assert_eq!(exit, 9, "{v}");
    assert_eq!(d["incomplete"], false, "{d}");
    let (exit, v) = run(&[
        "diff",
        &project,
        "--rev",
        "HEAD~1",
        "--source",
        "pcb",
        "--exit-code",
    ]);
    let p = &v["data"];
    assert_eq!(exit, 9, "{v}");
    let found = [
        has(d, "part_changed", "R2", Some("comment")),
        has(
            d,
            "sheet_param_changed",
            "PilotPi_Power.SchDoc",
            Some("Revision"),
        ),
        has(p, "part_changed", "JP1", Some("kind")),
        has(p, "rule_changed", "Clearance", Some("gap")),
        has(p, "class_added", "PWM_HI", None),
    ];
    assert_eq!(
        found.iter().filter(|f| **f).count(),
        5,
        "{found:?}\nsch: {}\npcb: {}",
        d["items"],
        p["items"]
    );
}
