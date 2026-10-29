//! `ace bom|netlist|check|diff` — cross-document analysis over a project, sheet or PCB.
//!
//! Every command takes a `.PrjPcb`, `.SchDoc` or `.PcbDoc` and turns it into a
//! [`Design`] (parts + nets) through the doc layer; the analysis crate does the rest.

use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};

use altium_analysis::{bom, check, diff, netlist};
use altium_core::error::{Error, ErrorCode, Result};
use altium_doc::model::Design;
use altium_doc::pcb::{self, LoadOptions, Rule, part};
use altium_doc::project::{Project, Variant};
use altium_doc::sch;
use altium_format::DocKind;

use crate::ctx::Ctx;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list_with};

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Schematic sheets (project-wide connectivity for a .PrjPcb).
    Sch,
    /// The PCB document.
    Pcb,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BomFormat {
    Csv,
    Json,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetFormat {
    /// Protel/Tango netlist (`[` parts `]`, `(` nets `)`), readable by most EDA tools.
    Protel,
    /// `Net,Designator,Pin` rows.
    Csv,
    /// The JSON items of this command.
    Json,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffGroup { Part }

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchArg { Auto, Designator, UniqueId }
impl From<MatchArg> for diff::MatchBy {
    fn from(value: MatchArg) -> Self { match value {
        MatchArg::Auto => Self::Auto, MatchArg::Designator => Self::Designator, MatchArg::UniqueId => Self::UniqueId,
    } }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailOn { None, Warning, Error }

#[derive(Subcommand, Debug)]
pub enum AnalysisCmd {
    /// Bill of materials of a project, sheet or PCB: grouped lines, variants, CSV/JSON export.
    Bom {
        /// .PrjPcb (all sheets), .SchDoc or .PcbDoc.
        input: PathBuf,
        /// For a project: read parts from the schematics (default) or the PCB.
        #[arg(long, value_enum)]
        source: Option<Source>,
        /// PCB document to use with --source pcb when the project has several.
        #[arg(long)]
        pcb: Option<PathBuf>,
        /// Fields that make one BOM line: comment, footprint, lib_ref, description, value, mpn,
        /// manufacturer, supplier_pn, kind, designator (one line per part) or any parameter name.
        #[arg(long, value_delimiter = ',', default_value = "comment,footprint")]
        group_by: Vec<String>,
        /// Apply this assembly variant (name, unique id or 1-based index).
        #[arg(long)]
        variant: Option<String>,
        /// Project holding the variants when the input is a sheet or PCB.
        #[arg(long)]
        project: Option<PathBuf>,
        /// Keep the variant's not-fitted parts (marked dnp) instead of dropping them.
        #[arg(long)]
        include_dnp: bool,
        /// Keep parts excluded from the BOM by their component kind (marked excluded).
        #[arg(long)]
        include_excluded: bool,
        /// Columns of the CSV file (default: line,quantity,designator_ranges,comment,footprint,value,
        /// manufacturer,mpn,supplier_pn,description,dnp). Any parameter name works.
        #[arg(long, value_delimiter = ',')]
        columns: Vec<String>,
        /// Write the BOM to this file (.csv or .json; see --format).
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// File format for --out (default: from the extension, else csv).
        #[arg(long, value_enum)]
        format: Option<BomFormat>,
    },
    /// Nets with their nodes (or one row per pin with --pins); Protel/CSV/JSON export.
    Netlist {
        /// .PrjPcb (project-wide nets), .SchDoc (sheet nets) or .PcbDoc (PCB nets).
        input: PathBuf,
        /// For a project: nets from the schematics (default) or the PCB.
        #[arg(long, value_enum)]
        source: Option<Source>,
        /// PCB document to use with --source pcb when the project has several.
        #[arg(long)]
        pcb: Option<PathBuf>,
        /// One item per pin: {designator, pin, pin_name, net, node}.
        #[arg(long)]
        pins: bool,
        /// Leave out nets without nodes (PCB nets that no pad uses).
        #[arg(long)]
        connected: bool,
        /// Write the netlist to this file.
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// File format for --out (default: protel; .csv/.json by extension).
        #[arg(long, value_enum)]
        format: Option<NetFormat>,
    },
    /// Native design checks: annotation, duplicates, footprints, nets, schematic vs PCB sync.
    Check {
        /// .PrjPcb (schematics + PCB), .SchDoc or .PcbDoc.
        #[arg(required_unless_present = "list")]
        input: Option<PathBuf>,
        /// PCB to compare a project's schematics with (default: the project's PCB; with several,
        /// the one sharing the most designators with the schematics).
        #[arg(long)]
        pcb: Option<PathBuf>,
        /// Run only these check codes (comma-separated; see --list).
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Skip these check codes.
        #[arg(long, value_delimiter = ',')]
        skip: Vec<String>,
        /// Minimum severity to report: info, warning or error.
        #[arg(long, default_value = "info")]
        severity: String,
        /// Parameters every BOM part must have (e.g. MPN,Manufacturer) -> missing-parameter.
        #[arg(long, value_delimiter = ',')]
        require: Vec<String>,
        /// Compare with a complete saved `ace check` JSON envelope; gates count new findings.
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Exit 9 after emitting the normal report when this severity or higher is present.
        #[arg(long, value_enum, default_value = "none")]
        fail_on: FailOn,
        /// List the check codes with scope, meaning and fix hint instead of checking.
        #[arg(long)]
        list: bool,
    },
    /// Compare two designs (revisions, sheet vs PCB) or a file against a git revision.
    Diff {
        /// Before: .PrjPcb, .SchDoc or .PcbDoc.
        a: PathBuf,
        /// After (same kind, or a PCB to compare a project/sheet with). Omit with --rev.
        #[arg(required_unless_present = "rev")]
        b: Option<PathBuf>,
        /// Compare A as committed at this git revision (before) with A on disk (after).
        #[arg(long, conflicts_with = "b")]
        rev: Option<String>,
        /// Return the summary without individual change rows.
        #[arg(long)]
        summary: bool,
        /// Group changed part fields into one row per part.
        #[arg(long, value_enum)]
        by: Option<DiffGroup>,
        /// Ignore fields matching this regular expression (repeatable).
        #[arg(long)]
        ignore_field: Vec<String>,
        /// Compare only these fields (comma-separated; parameter names also work).
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Match parts by unique ID first, then designator; never guess ambiguous IDs.
        #[arg(long = "match", value_enum, default_value = "auto")]
        matching: MatchArg,
        /// Exit 9 when designs differ (the normal JSON report is preserved).
        #[arg(long)]
        exit_code: bool,
        /// Include position/rotation/side changes (PCB vs PCB only).
        #[arg(long)]
        geometry: bool,
        /// For projects: compare their schematics (default) or their PCBs.
        #[arg(long, value_enum)]
        source: Option<Source>,
    },
}

pub const META: &[CmdMeta] = &[
    m("bom", Native, Read,
      "{input, source, documents, documents_declared?, documents_found?, missing_documents?, incomplete, group_by, variant?, total_parts, unique_lines, count, items:[{line, quantity, designators, designator_ranges, comment, footprint, lib_ref, description, value?, manufacturer?, mpn?, supplier_pn?, parameters, inconsistent?, dnp?, excluded?}], dnp?, excluded?, varied?, out?}",
      "ace bom Board.PrjPcb --variant Lite --group-by comment,footprint,mpn --out bom.csv",
      "Project input uses every schematic (sub-parts merged, channels expanded). Variants: not-fitted parts are dropped (or marked with --include-dnp), parameter variations applied; alternate parts are flagged (resolve them with the designer BOM). --out writes CSV/JSON (honours --dry-run). `--group-by designator` gives one line per part. value uses Value/Val then comment (with a warning). CSV columns: line,quantity,designator_ranges,designator_list,comment,footprint,lib_ref,description,value,manufacturer,mpn,supplier_pn,dnp,excluded or any parameter; designator/designators remain aliases for designator_ranges. documents_declared/documents_found count schematic sheets; incomplete and missing_documents expose missing project dependencies."),
    m("netlist", Native, Read,
      "{input, source, documents, documents_declared?, documents_found?, missing_documents?, incomplete, count, items:[{name, node_count, nodes:[\"R1.1\"...], auto_named?, name_source?}] | --pins: items:[{designator, pin, pin_name?, net, node}], parts, out?}",
      "ace netlist Board.PrjPcb --where name=GND",
      "Schematic nets are a native approximation of Altium's compiler (`ace designer compile` is the authority); node pins are footprint pad numbers so they match the PCB. incomplete/missing_documents identify partial projects. --out writes protel (default), csv or json."),
    m("check", Native, Read,
      "{input, schematic?, pcb?, incomplete, missing_documents?, documents_declared?, documents_found?, passed, errors, warnings, infos, by_code, by_scope, check_options, gate?, baseline?, new?, resolved?, unchanged?, passed_vs_baseline?, count, items:[{severity, code, scope, message, refs, hint}]} | --list: {count, items:[{code, scope, meaning, hint}]}",
      "ace check Board.PrjPcb --severity warning --require MPN",
      "Fast native pre-flight, not ERC/DRC (use `ace designer erc|drc`). passed=false when any error-severity finding remains after --only/--skip/--severity. --fail-on error|warning preserves the report and exits 9; by_code/by_scope are tallies before --limit. --baseline compares complete check envelopes by code/scope/severity/refs and reports new/resolved/unchanged; gates then count only new findings. refs are designators, nets or designator.pin nodes."),
    m("diff", Native, Read,
      "{a, b, identical, incomplete?, coverage, gate?, comparison_fields?, summary:{parts_added, parts_removed, parts_changed, parts_renamed, by_field, nets_added, nets_removed, nets_renamed, nets_changed, rules_*?}, count, items:[{change, ref, field?, before?, after?, before_ref?, unique_id?, unit?, side?, fields?, added_nodes?, removed_nodes?}]}",
      "ace diff Board.PrjPcb --rev HEAD~1",
      "change: part_added|part_removed|part_renamed|part_changed|net_added|net_removed|net_renamed|net_changed|rule_added|rule_removed|rule_changed. A schematic/project vs a PCB compares parts and connectivity (not geometry). Project variants/settings are always compared. PcbLib/SchLib compare decoded fields by name (unknown binary streams excluded). Project inputs expose incomplete and per-side coverage; restore missing_documents before trusting a diff. --rev needs git on PATH and the files committed. --exit-code preserves the report and exits 9 on differences. --match auto uses unambiguous unique IDs before designators; --match designator reproduces add/remove behavior. --summary omits items; --by part groups fields; --ignore-field REGEX/--only FIELDS scope the comparison before --exit-code. Rule values keep JSON types and units."),
];

// ------------------------------------------------------------------------------- inputs

/// A loaded design plus what it came from.
struct Loaded {
    design: Design,
    source: &'static str,
    /// Documents read (paths).
    documents: Vec<PathBuf>,
    /// PCB rules, when the design is a PCB (for `diff`).
    rules: Option<Vec<Rule>>,
    warnings: Vec<String>,
    coverage: Value,
}

fn load_pcb(path: &Path, with_rules: bool) -> Result<Loaded> {
    let parts = if with_rules { part::PADS | part::RULES } else { part::PADS };
    let doc = pcb::load_with(path, &LoadOptions::parts(parts))?;
    let design = pcb::design(&doc);
    Ok(Loaded {
        coverage: json!({"incomplete": false}),
        warnings: design.warnings.clone(),
        design,
        source: "pcb",
        documents: vec![path.to_path_buf()],
        rules: with_rules.then(|| doc.rules.clone()),
    })
}

fn load_sheet(path: &Path) -> Result<Loaded> {
    let design = sch::sheet_design(path)?;
    Ok(Loaded { warnings: design.warnings.clone(), design, source: "schematic", documents: vec![path.to_path_buf()], rules: None, coverage: json!({"incomplete": false}) })
}

fn project_coverage(p: &Project) -> Value {
    let sheets = p.docs(DocKind::Schematic);
    let missing: Vec<_> = p.documents.iter().filter(|d| !d.generated && !d.exists).map(|d| &d.rel).collect();
    json!({"documents_declared": sheets.len(), "documents_found": sheets.iter().filter(|d| d.exists).count(),
        "missing_documents": missing, "incomplete": !missing.is_empty()})
}

fn load_project_sch(p: &Project) -> Result<Loaded> {
    if p.docs(DocKind::Schematic).is_empty() {
        return Err(Error::not_found(format!("{} has no schematic documents", p.path.display()))
            .hint("use --source pcb, or check `ace project documents`"));
    }
    let design = if p.schematics().is_empty() { Design::default() } else { sch::project_design(p)? };
    let coverage = project_coverage(p);
    let mut warnings = design.warnings.clone();
    if coverage["incomplete"] == true {
        let declared = coverage["documents_declared"].as_u64().unwrap_or(0);
        let found = coverage["documents_found"].as_u64().unwrap_or(0);
        if found < declared {
            warnings.push(format!("incomplete project: {} of {declared} sheets not found; results below are partial; inspect missing_documents", declared - found));
        } else {
            warnings.push("incomplete project: missing non-schematic dependencies listed in missing_documents; schematic counts cover all sheets".into());
        }
    }
    Ok(Loaded { warnings, design, source: "schematic", documents: p.schematics(), rules: None, coverage })
}

fn add_coverage(mut data: Value, coverage: &Value) -> Value {
    if let (Some(target), Some(fields)) = (data.as_object_mut(), coverage.as_object()) {
        target.extend(fields.clone());
    }
    data
}

/// The project's PCB: `explicit`, else the only one, else (given the schematic design) the
/// one sharing the most designators with it.
fn project_pcb(p: &Project, explicit: Option<&Path>, sch: Option<&Design>, with_rules: bool) -> Result<Loaded> {
    if let Some(e) = explicit {
        if !e.exists() {
            return Err(Error::new(ErrorCode::FileNotFound, format!("{} does not exist", e.display())));
        }
        return load_pcb(e, with_rules);
    }
    let pcbs = p.pcbs();
    match pcbs.as_slice() {
        [] => Err(Error::not_found(format!("{} has no PCB document on disk", p.path.display()))
            .hint("use --source sch, or pass --pcb <file>")),
        [one] => load_pcb(one, with_rules),
        many => {
            let Some(s) = sch else {
                return Err(Error::new(ErrorCode::Ambiguous, format!("{} has {} PCB documents", p.path.display(), many.len()))
                    .details(json!(many))
                    .hint("choose one with --pcb <file>"));
            };
            let want: std::collections::HashSet<String> = s.parts.iter().map(|x| x.designator.to_ascii_uppercase()).collect();
            let mut best: Option<(usize, Loaded)> = None;
            for f in many {
                let Ok(l) = load_pcb(f, with_rules) else { continue };
                let score = l.design.parts.iter().filter(|x| want.contains(&x.designator.to_ascii_uppercase())).count();
                if best.as_ref().is_none_or(|(b, _)| score > *b) {
                    best = Some((score, l));
                }
            }
            let (_, mut l) = best.ok_or_else(|| Error::parse("none of the project's PCB documents could be read"))?;
            l.warnings.push(format!(
                "the project has {} PCB documents; using {} (most designators in common with the schematics; override with --pcb)",
                many.len(),
                l.documents[0].display()
            ));
            Ok(l)
        }
    }
}

/// Load any supported input as a design.
fn load(input: &Path, source: Option<Source>, pcb_file: Option<&Path>, with_rules: bool, ctx: &Ctx) -> Result<Loaded> {
    let input = ctx.input(input)?;
    match DocKind::of(&input) {
        DocKind::Project => {
            let p = Project::load(&input)?;
            let mut loaded = match source {
                Some(Source::Pcb) => project_pcb(&p, pcb_file, None, with_rules),
                Some(Source::Sch) => load_project_sch(&p),
                None if p.docs(DocKind::Schematic).is_empty() && !p.pcbs().is_empty() => {
                    let mut l = project_pcb(&p, pcb_file, None, with_rules)?;
                    l.warnings.push("the project has no schematics on disk; using its PCB".into());
                    Ok(l)
                }
                None => load_project_sch(&p),
            }?;
            loaded.coverage = project_coverage(&p);
            if loaded.source == "pcb" && loaded.coverage["incomplete"] == true {
                loaded.warnings.push("incomplete project: see missing_documents; results below are partial".into());
            }
            Ok(loaded)
        }
        DocKind::Schematic => {
            if source == Some(Source::Pcb) {
                return Err(Error::usage("--source pcb needs a .PrjPcb or .PcbDoc input"));
            }
            load_sheet(&input)
        }
        DocKind::Pcb => {
            if source == Some(Source::Sch) {
                return Err(Error::usage("--source sch needs a .PrjPcb or .SchDoc input"));
            }
            load_pcb(&input, with_rules)
        }
        _ => Err(Error::new(ErrorCode::WrongFileType, format!("{} is not a project, schematic or PCB", input.display()))
            .hint("pass a .PrjPcb, .SchDoc or .PcbDoc")),
    }
}

fn variant_of(input: &Path, project: Option<&Path>, name: &str, ctx: &Ctx) -> Result<Variant> {
    let prj = match project {
        Some(p) => ctx.input(p)?,
        None if DocKind::of(input) == DocKind::Project => input.to_path_buf(),
        None => {
            return Err(Error::usage("--variant on a sheet or PCB needs --project <file.PrjPcb>")
                .hint("variants are stored in the project file"));
        }
    };
    Project::load(&prj)?.variant(name)
}

// --------------------------------------------------------------------------------- run

pub fn run(c: AnalysisCmd, ctx: &Ctx) -> Result<Out> {
    match c {
        AnalysisCmd::Bom { input, source, pcb, group_by, variant, project, include_dnp, include_excluded, columns, out, format } => {
            let l = load(&input, source, pcb.as_deref(), false, ctx)?;
            let variant = variant.map(|v| variant_of(&input, project.as_deref(), &v, ctx)).transpose()?;
            let opts = bom::BomOptions { group_by, variant, include_excluded, include_dnp };
            let b = bom::build(&l.design, &opts)?;
            let mut warnings: Vec<_> = l.warnings.into_iter().filter(|w| !w.contains("unnamed single-pin net(s) left out")).collect();
            warnings.extend(b.warnings.iter().cloned());
            let mut v = serde_json::to_value(&b)?;
            let o = v.as_object_mut().ok_or_else(|| Error::internal("bom is not an object"))?;
            let lines = o.remove("lines").unwrap_or(json!([]));
            o.remove("warnings");
            o.insert("input".into(), json!(input));
            o.insert("source".into(), json!(l.source));
            o.insert("documents".into(), json!(l.documents));
            o.insert("count".into(), json!(b.lines.len()));
            o.insert("items".into(), lines);
            if let Some(path) = out {
                let fmt = format.unwrap_or(if ext_is(&path, "json") { BomFormat::Json } else { BomFormat::Csv });
                let bytes = match fmt {
                    BomFormat::Csv => {
                        let cols: Vec<String> = if columns.is_empty() {
                            bom::DEFAULT_COLUMNS.iter().map(|s| s.to_string()).collect()
                        } else {
                            columns
                        };
                        bom::to_csv_columns(&b, &cols).into_bytes()
                    }
                    BomFormat::Json => serde_json::to_vec_pretty(&b)?,
                };
                ctx.export_file(&path, &bytes)?;
                o.insert("out".into(), json!({"path": path, "format": format!("{fmt:?}").to_lowercase(), "bytes": bytes.len()}));
            }
            Ok(Out::new(add_coverage(v, &l.coverage))?.warnings(warnings))
        }
        AnalysisCmd::Netlist { input, source, pcb, pins, connected, out, format } => {
            let mut l = load(&input, source, pcb.as_deref(), false, ctx)?;
            if connected {
                l.design.nets.retain(|n| !n.nodes.is_empty());
            }
            let extra = json!({"input": input, "source": l.source, "documents": l.documents, "parts": l.design.parts.len()});
            let mut data = if pins {
                list_with(&netlist::pins(&l.design), extra)?
            } else {
                list_with(&netlist::nets(&l.design), extra)?
            };
            if let Some(path) = out {
                let fmt = format.unwrap_or(if ext_is(&path, "csv") {
                    NetFormat::Csv
                } else if ext_is(&path, "json") {
                    NetFormat::Json
                } else {
                    NetFormat::Protel
                });
                let bytes = match fmt {
                    NetFormat::Protel => netlist::to_protel(&l.design).into_bytes(),
                    NetFormat::Csv => netlist::to_csv(&l.design).into_bytes(),
                    NetFormat::Json => serde_json::to_vec_pretty(&data["items"])?,
                };
                ctx.export_file(&path, &bytes)?;
                data["out"] = json!({"path": path, "format": format!("{fmt:?}").to_lowercase(), "bytes": bytes.len()});
            }
            Ok(Out::new(add_coverage(data, &l.coverage))?.warnings(l.warnings))
        }
        AnalysisCmd::Check { input, pcb, only, skip, severity, require, fail_on, baseline, list } => {
            if list {
                let items: Vec<Value> = check::CODES
                    .iter()
                    .map(|(code, scope, meaning, hint)| json!({"code": code, "scope": scope, "meaning": meaning, "hint": hint}))
                    .collect();
                return Out::new(list_with(&items, json!({}))?);
            }
            let input = input.ok_or_else(|| Error::usage("check needs an input file"))?;
            let mut out = run_check(&input, pcb.as_deref(), &only, &skip, &severity, require, ctx)?;
            if let Some(path) = baseline {
                let saved: Value = serde_json::from_slice(&std::fs::read(&path)?).map_err(|e| Error::usage(format!("invalid baseline JSON: {e}"))
                    .hint("save an unshaped `ace check PROJECT` envelope"))?;
                let rows = saved["data"]["items"].as_array();
                let by_code_total = saved["data"]["by_code"].as_object().map(|m| m.values().filter_map(Value::as_u64).sum::<u64>());
                if saved["schema"] != crate::output::SCHEMA || saved["ok"] != true || saved["command"] != "check"
                    || saved["data"]["incomplete"] == true || saved["data"]["truncated"] == true
                    || saved["data"]["check_options"] != out.data["check_options"] || saved["data"]["pcb"] != out.data["pcb"]
                    || rows.is_none() || by_code_total != rows.map(|r| r.len() as u64) {
                    return Err(Error::usage("baseline must be a complete, unshaped ace check envelope from a complete project")
                        .hint("rerun `ace check PROJECT > before.json` with the same check options and PCB; omit --fields, --where and --limit"));
                }
                let delta = check::baseline_delta(rows.unwrap(), out.data["items"].as_array().unwrap())?;
                out.data.as_object_mut().unwrap().extend(delta.as_object().unwrap().clone());
                out.data["baseline"] = json!(path);
            }
            let (errors, warnings) = if let Some(new) = out.data["new"].as_array() {
                (new.iter().filter(|r| r["severity"] == "error").count(), new.iter().filter(|r| r["severity"] == "warning").count())
            } else { (out.data["errors"].as_u64().unwrap_or(0) as usize, out.data["warnings"].as_u64().unwrap_or(0) as usize) };
            Ok(match fail_on {
                FailOn::None => out,
                FailOn::Error => out.gate("error", errors),
                FailOn::Warning => out.gate("warning", errors + warnings),
            })
        }
        AnalysisCmd::Diff { a, b, rev, geometry, source, exit_code, matching, summary, by, ignore_field, only } => {
            let mut out = run_diff(&a, b.as_deref(), rev.as_deref(), geometry, source, matching.into(), ctx)?;
            diff_view(&mut out.data, summary, by, &ignore_field, &only)?;
            let matched = usize::from(out.data["identical"] == false);
            Ok(if exit_code { out.gate("difference", matched) } else { out })
        },
    }
}

fn ext_is(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn run_check(input: &Path, pcb_file: Option<&Path>, only: &[String], skip: &[String], severity: &str, require: Vec<String>, ctx: &Ctx) -> Result<Out> {
    let min = check::Severity::parse(severity)
        .ok_or_else(|| Error::usage(format!("bad --severity {severity:?}")).hint("use info, warning or error"))?;
    let codes: Vec<&str> = check::CODES.iter().map(|c| c.0).collect();
    for c in only.iter().chain(skip) {
        if !codes.contains(&c.as_str()) {
            return Err(Error::usage(format!("unknown check code {c:?}")).hint(format!("codes: {}", codes.join(", "))));
        }
    }
    let input = ctx.input(input)?;
    let mut warnings = vec![];
    let mut extra = vec![];
    let mut coverage = json!({"incomplete": false});
    let mut incomplete_design = false;
    let (sch_l, pcb_l) = match DocKind::of(&input) {
        DocKind::Project => {
            let p = Project::load(&input)?;
            coverage = project_coverage(&p);
            incomplete_design = p.documents.iter().any(|d| !d.generated && !d.exists && matches!(d.kind, DocKind::Schematic | DocKind::Pcb));
            if incomplete_design {
                extra.push(check::finding(check::Severity::Error, "incomplete-project",
                    "project design documents are missing; cross-document sync checks skipped".into(),
                    p.documents.iter().filter(|d| !d.generated && !d.exists).map(|d| d.rel.clone()).collect()));
            }
            for d in p.documents.iter().filter(|d| !d.generated && !d.exists && !matches!(d.kind, DocKind::Schematic | DocKind::Pcb)) {
                extra.push(check::finding(
                    check::Severity::Error,
                    "missing-document",
                    format!("{} is referenced by the project but not on disk", d.rel),
                    vec![d.rel.clone()],
                ));
            }
            let s = if p.schematics().is_empty() { None } else { Some(load_project_sch(&p)?) };
            let pc = if p.pcbs().is_empty() && pcb_file.is_none() {
                None
            } else {
                Some(project_pcb(&p, pcb_file, s.as_ref().map(|l| &l.design), false)?)
            };
            (s, pc)
        }
        DocKind::Schematic => {
            let pc = pcb_file.map(|f| load_pcb(&ctx.input(f)?, false)).transpose()?;
            (Some(load_sheet(&input)?), pc)
        }
        DocKind::Pcb => (None, Some(load_pcb(&input, false)?)),
        _ => {
            return Err(Error::new(ErrorCode::WrongFileType, format!("{} is not a project, schematic or PCB", input.display()))
                .hint("pass a .PrjPcb, .SchDoc or .PcbDoc"));
        }
    };
    for l in [&sch_l, &pcb_l].into_iter().flatten() {
        warnings.extend(l.warnings.iter().cloned());
    }
    let opts = check::CheckOptions { require };
    let mut findings = extra;
    if incomplete_design {
        if let Some(l) = &sch_l { findings.extend(check::run_with(Some(&l.design), None, &opts)); }
        if let Some(l) = &pcb_l { findings.extend(check::run_with(None, Some(&l.design), &opts)); }
    } else {
        findings.extend(check::run_with(sch_l.as_ref().map(|l| &l.design), pcb_l.as_ref().map(|l| &l.design), &opts));
    }
    findings.retain(|f| f.severity >= min);
    findings.retain(|f| f.code == "incomplete-project" || ((only.is_empty() || only.iter().any(|o| o == f.code)) && !skip.iter().any(|s| s == f.code)));
    let n = |s: check::Severity| findings.iter().filter(|f| f.severity == s).count();
    let (errors, warns, infos) = (n(check::Severity::Error), n(check::Severity::Warning), n(check::Severity::Info));
    if sch_l.is_some() && pcb_l.is_none() {
        warnings.push("no PCB: schematic vs PCB sync checks skipped".into());
    }
    let mut by_code = std::collections::BTreeMap::new();
    let mut by_scope = std::collections::BTreeMap::new();
    for finding in &findings {
        *by_code.entry(finding.code).or_insert(0usize) += 1;
        *by_scope.entry(finding.scope).or_insert(0usize) += 1;
    }
    let data = list_with(
        &findings,
        json!({
            "input": input,
            "schematic": sch_l.as_ref().map(|l| &l.documents),
            "pcb": pcb_l.as_ref().map(|l| &l.documents[0]),
            "by_code": by_code,
            "by_scope": by_scope,
            "check_options": {"only":only,"skip":skip,"severity":severity,"require":opts.require},
            "passed": errors == 0,
            "errors": errors,
            "warnings": warns,
            "infos": infos,
        }),
    )?;
    Ok(Out::new(add_coverage(data, &coverage))?.warnings(warnings))
}

// -------------------------------------------------------------------------------- diff


fn run_diff(a: &Path, b: Option<&Path>, rev: Option<&str>, geometry: bool, source: Option<Source>, matching: diff::MatchBy, ctx: &Ctx) -> Result<Out> {
    let a = ctx.input(a)?;
    let (before_path, after_path, tmp, label_a) = match (b, rev) {
        (Some(b), None) => (a.clone(), ctx.input(b)?, None, a.display().to_string()),
        (None, Some(r)) => {
            let (tmp, copy) = crate::revision::checkout_rev(&a, r)?;
            (copy, a.clone(), Some(tmp), format!("{}@{r}", a.display()))
        }
        _ => return Err(Error::usage("give a second file or --rev <revision>")),
    };
    let result = (|| -> Result<Out> {
        if matches!(DocKind::of(&before_path), DocKind::PcbLib | DocKind::SchLib) {
            return library_diff(&before_path, &after_path, &label_a, ctx);
        }
        let la = load(&before_path, source, None, true, ctx)?;
        let lb = load(&after_path, source, None, true, ctx)?;
        let same_kind = la.source == lb.source;
        if geometry && !(same_kind && la.source == "pcb") {
            return Err(Error::usage("--geometry compares PCB placement; both sides must be PCBs")
                .hint("use --source pcb for projects"));
        }
        let d = diff::diff_with(&la.design, &lb.design, geometry, matching);
        let rd = match (&la.rules, &lb.rules) {
            (Some(x), Some(y)) => Some(diff::rules(x, y)),
            _ => None,
        };
        let mut items: Vec<Value> = vec![];
        for r in &d.parts_added {
            items.push(json!({"change": "part_added", "ref": r}));
        }
        for r in &d.parts_removed {
            items.push(json!({"change": "part_removed", "ref": r}));
        }
        for p in &d.parts_renamed {
            items.push(json!({"change": "part_renamed", "ref": p.designator, "before_ref": p.before_ref, "unique_id": p.unique_id}));
        }
        for p in &d.parts_changed {
            for c in &p.changes {
                items.push(json!({"change": "part_changed", "ref": p.designator, "field": c.field, "before": c.before, "after": c.after}));
            }
        }
        for r in &d.nets_added {
            items.push(json!({"change": "net_added", "ref": r}));
        }
        for r in &d.nets_removed {
            items.push(json!({"change": "net_removed", "ref": r}));
        }
        for r in &d.nets_renamed {
            items.push(json!({"change": "net_renamed", "ref": r.after, "before": r.before, "after": r.after}));
        }
        for n in &d.nets_changed {
            items.push(json!({"change": "net_changed", "ref": n.net, "added_nodes": n.added_nodes, "removed_nodes": n.removed_nodes}));
        }
        let mut summary = json!({
            "parts_added": d.parts_added.len(),
            "parts_removed": d.parts_removed.len(),
            "parts_changed": d.parts_changed.len(),
            "parts_renamed": d.parts_renamed.len(),
            "nets_added": d.nets_added.len(),
            "nets_removed": d.nets_removed.len(),
            "nets_renamed": d.nets_renamed.len(),
            "nets_changed": d.nets_changed.len(),
        });
        let mut identical = d.identical;
        if let Some(rd) = &rd {
            for r in &rd.added {
                items.push(json!({"change": "rule_added", "ref": r}));
            }
            for r in &rd.removed {
                items.push(json!({"change": "rule_removed", "ref": r}));
            }
            for r in &rd.changed {
                for c in &r.changes {
                    items.push(json!({"change": "rule_changed", "ref": r.rule, "kind": r.kind, "field": c.field, "before": c.before, "after": c.after, "unit":c.unit}));
                }
            }
            summary["rules_added"] = json!(rd.added.len());
            summary["rules_removed"] = json!(rd.removed.len());
            summary["rules_changed"] = json!(rd.changed.len());
            identical &= rd.added.is_empty() && rd.removed.is_empty() && rd.changed.is_empty();
        }
        if DocKind::of(&before_path) == DocKind::Project && DocKind::of(&after_path) == DocKind::Project {
            let project_changes = diff::project(&Project::load(&before_path)?, &Project::load(&after_path)?);
            identical &= project_changes.is_empty();
            for change in &project_changes {
                let key = change["change"].as_str().unwrap_or("project_changes");
                let n = summary[key].as_u64().unwrap_or(0);
                summary[key] = json!(n + 1);
            }
            items.extend(project_changes);
        }
        let mut warnings: Vec<String> = la.warnings.iter().map(|w| {
            if lb.warnings.contains(w) { format!("both: {w}") } else { format!("before: {w}") }
        }).collect();
        warnings.extend(lb.warnings.iter().filter(|w| !la.warnings.contains(w)).map(|w| format!("after: {w}")));
        if [(&la, &before_path), (&lb, &after_path)].iter().any(|(l, p)| l.source == "schematic" && DocKind::of(p) == DocKind::Project) {
            warnings.push("PCB not compared: project diff currently uses schematics; run a second diff with --source pcb to compare PCB rules and placement".into());
        }
        if [(&la, &before_path), (&lb, &after_path)].iter().any(|(l, _)| l.source == "pcb" && l.design.nets.is_empty()) {
            warnings.push("PCB has 0 nets: connectivity coverage is incomplete; net removals may reflect an unassigned PCB".into());
        }
        if !same_kind {
            warnings.push(format!(
                "comparing {} with {}: only parts and connectivity are compared; net names may differ legally (see `ace check` for sync findings)",
                la.source, lb.source
            ));
        }
        let data = list_with(
            &items,
            json!({
                "a": label_a,
                "b": after_path,
                "a_source": la.source,
                "b_source": lb.source,
                "identical": identical,
                "summary": summary,
            }),
        )?;
        let mut data = data;
        data["incomplete"] = json!(la.coverage["incomplete"] == true || lb.coverage["incomplete"] == true);
        data["coverage"] = json!({"a": la.coverage, "b": lb.coverage});
        Ok(Out::new(data)?.warnings(warnings))
    })();
    drop(tmp);
    result
}

fn library_diff(a: &Path, b: &Path, label_a: &str, _ctx: &Ctx) -> Result<Out> {
    if DocKind::of(a) != DocKind::of(b) {
        return Err(Error::new(ErrorCode::WrongFileType, "library diff needs two libraries of the same kind")
            .hint("compare two .PcbLib files or two .SchLib files"));
    }
    let (before, after, mut warnings, kind) = match DocKind::of(a) {
        DocKind::PcbLib => {
            let (a, b) = (altium_doc::pcblib::load(a)?, altium_doc::pcblib::load(b)?);
            let mut warnings = a.warnings; warnings.extend(b.warnings);
            let mut before: Vec<Value> = a.footprints.iter().map(serde_json::to_value).collect::<std::result::Result<_, _>>()?;
            let mut after: Vec<Value> = b.footprints.iter().map(serde_json::to_value).collect::<std::result::Result<_, _>>()?;
            before.push(json!({"name":"$library_models", "models":a.models}));
            after.push(json!({"name":"$library_models", "models":b.models}));
            (before, after, warnings, "footprint")
        }
        DocKind::SchLib => {
            let (a, b) = (altium_doc::schlib::load(a)?, altium_doc::schlib::load(b)?);
            let mut warnings = a.warnings; warnings.extend(b.warnings);
            (a.symbols.iter().map(serde_json::to_value).collect::<std::result::Result<_, _>>()?,
             b.symbols.iter().map(serde_json::to_value).collect::<std::result::Result<_, _>>()?, warnings, "symbol")
        }
        _ => return Err(Error::internal("library dispatch mismatch")),
    };
    let items = diff::library(&before, &after, kind);
    let mut summary = std::collections::BTreeMap::new();
    for item in &items { *summary.entry(item["change"].as_str().unwrap_or("changed")).or_insert(0usize) += 1; }
    warnings.push("library diff compares decoded fields, geometry and model metadata; unknown binary streams and embedded model payload bytes are not compared; renames appear as add/remove".into());
    Out::new(list_with(&items, json!({"a":label_a,"b":b,"identical":items.is_empty(),"summary":summary,
        "coverage":{"compared":["decoded_library_objects"],"excluded":["unknown_binary_streams","model_payload_bytes"]}}))?).map(|o| o.warnings(warnings))
}

fn diff_view(data: &mut Value, summary_only: bool, by: Option<DiffGroup>, ignored: &[String], only: &[String]) -> Result<()> {
    let patterns: Vec<_> = ignored.iter().map(|pattern| regex::Regex::new(pattern).map_err(|e| Error::usage(format!("invalid --ignore-field regex: {e}"))
        .hint("use a regular expression such as ^param:ComponentLink"))).collect::<Result<_>>()?;
    let mut items = data["items"].as_array().cloned().unwrap_or_default();
    if !patterns.is_empty() || !only.is_empty() {
        items.retain(|row| {
            let field = row["field"].as_str().unwrap_or("");
            !patterns.iter().any(|p| p.is_match(field)) && (only.is_empty() || only.iter().any(|key| field.eq_ignore_ascii_case(key)
                || field.strip_prefix("param:").is_some_and(|name| name.eq_ignore_ascii_case(key))))
        });
        let mut groups: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> = Default::default();
        for row in &items {
            let change = row["change"].as_str().unwrap_or("changed");
            let key = if let Some(suffix) = change.strip_prefix("part_") { format!("parts_{suffix}") }
                else if let Some(suffix) = change.strip_prefix("net_") { format!("nets_{suffix}") }
                else if let Some(suffix) = change.strip_prefix("rule_") { format!("rules_{suffix}") } else { change.to_owned() };
            groups.entry(key).or_default().insert(row["ref"].as_str().unwrap_or("").to_owned());
        }
        data["summary"] = json!(groups.into_iter().map(|(k,v)| (k,v.len())).collect::<std::collections::BTreeMap<_,_>>());
        data["identical"] = json!(items.is_empty());
        data["comparison_fields"] = json!({"only":only,"ignored":ignored});
    }
    let mut fields = std::collections::BTreeMap::new();
    for row in &mut items {
        if let Some(field) = row["field"].as_str() { *fields.entry(field.to_owned()).or_insert(0usize) += 1; }
        match row["change"].as_str().unwrap_or("") {
            c if c.ends_with("_added") => row["side"] = json!("only_in_b"),
            c if c.ends_with("_removed") => row["side"] = json!("only_in_a"),
            _ => {}
        }
    }
    data["summary"]["by_field"] = json!(fields);
    if by == Some(DiffGroup::Part) {
        let mut parts: std::collections::BTreeMap<String, Vec<Value>> = Default::default();
        items.retain(|row| {
            if row["change"] != "part_changed" { return true; }
            parts.entry(row["ref"].as_str().unwrap_or("").to_owned()).or_default().push(json!({"field":row["field"],"before":row["before"],"after":row["after"]}));
            false
        });
        items.extend(parts.into_iter().map(|(name, fields)| json!({"change":"part_changed","ref":name,"fields":fields})));
    }
    data["count"] = json!(items.len());
    if summary_only { data.as_object_mut().unwrap().remove("items"); }
    else { data["items"] = json!(items); }
    Ok(())
}
