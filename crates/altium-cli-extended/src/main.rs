//! `ace` — an agent-native command line for Altium Designer projects.

mod commands;
mod ctx;
mod revision;
mod meta;
mod output;

use std::io::IsTerminal;
use std::time::Instant;

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use serde_json::{Value, json};

use crate::ctx::Ctx;
use altium_core::error::Error;
use crate::output::{Filter, Out, Shape};
use altium_core::units::{self, Unit};

use crate::commands as cmds;

const ABOUT: &str = "Agent-native CLI for Altium Designer: read, query, check and edit \
.PrjPcb/.SchDoc/.SchLib/.PcbDoc/.PcbLib/.OutJob natively, and drive the real Altium Designer \
(compile, ERC, DRC, Gerber/ODB++/PDF/BOM/STEP outputs, any process or script).";

const LONG_ABOUT: &str = "Agent-native CLI for Altium Designer.

Start here:
  ace commands               every command with backend, effect, arguments and an example
  ace describe pcb nets      one command in full
  ace skill show --raw       read Markdown (skill install: copy it to agents)
  ace doctor --quick         offline native readiness (full doctor probes Altium host/bridge)

Output: one JSON envelope on stdout when piped (or with --json):
  {\"ok\":true,\"schema\":\"altium-cli-extended/v1\",\"command\":\"...\",\"data\":{...},\"warnings\":[],\"meta\":{...}}
  {\"ok\":false,\"schema\":\"altium-cli-extended/v1\",\"command\":\"...\",\"error\":{\"code\":\"...\",\"message\":\"...\",\"hint\":\"...\"}}
Errors have code, category, message, optional hint/details; warnings/meta appear only on success.
Exit 9 with ok:true means an explicit design gate matched (data.gate).
List commands return data.items; shape them with --fields, --where and --limit.
Exit code 0 on success; see `ace errors` for the others.";

#[derive(Parser, Debug)]
#[command(name = "ace", version, about = ABOUT, long_about = LONG_ABOUT)]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Args, Debug, Clone)]
pub struct Global {
    /// Force JSON output (default when stdout is not a terminal).
    #[arg(long, global = true)]
    pub json: bool,
    /// Force human-readable output (tables).
    #[arg(long, global = true, conflicts_with = "json")]
    pub text: bool,
    /// Pretty-print JSON.
    #[arg(long, global = true)]
    pub pretty: bool,
    /// Unit for every length/coordinate in the output.
    #[arg(long, global = true, value_enum, default_value_t = UnitArg::Mm, env = "ACE_UNITS")]
    pub units: UnitArg,
    /// ANSI code page of the design files (e.g. windows-1251, euc-kr, gbk, shift_jis).
    #[arg(long, global = true, env = "ACE_CODEPAGE", default_value = "windows-1252")]
    pub codepage: String,
    /// Keep only these fields of each list item (comma-separated, dotted paths allowed).
    #[arg(long, global = true, value_delimiter = ',')]
    pub fields: Vec<String>,
    /// Filter list items: key=value, key!=value, key~regex, key^=substring. Repeatable (AND).
    #[arg(long = "where", global = true, value_name = "FILTER")]
    pub filters: Vec<String>,
    /// Keep at most N list items.
    #[arg(long, global = true)]
    pub limit: Option<usize>,
    /// Include raw Altium properties; with skill/skill show, print Markdown directly.
    #[arg(long, global = true)]
    pub raw: bool,
    /// Refuse design writes and control operations before any file or host access.
    #[arg(long, global = true, env = "ACE_READONLY", value_parser = ReadOnlyValueParser)]
    pub read_only: bool,
    /// Write commands: report the change without writing anything.
    #[arg(long, global = true)]
    pub dry_run: bool,
    /// Write commands: do not keep a <file>.bak copy.
    #[arg(long, global = true)]
    pub no_backup: bool,
}

#[derive(Clone, Debug)]
struct ReadOnlyValueParser;

impl clap::builder::TypedValueParser for ReadOnlyValueParser {
    type Value = bool;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<bool, clap::Error> {
        if value.is_empty() {
            Ok(false)
        } else {
            clap::builder::BoolishValueParser::new().parse_ref(cmd, arg, value)
        }
    }
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
pub enum UnitArg {
    Mm,
    Mil,
    /// Raw Altium internal units (1e-4 mil). Lossless.
    Raw,
}

impl From<UnitArg> for Unit {
    fn from(u: UnitArg) -> Unit {
        match u {
            UnitArg::Mm => Unit::Mm,
            UnitArg::Mil => Unit::Mil,
            UnitArg::Raw => Unit::Raw,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    #[command(flatten)]
    Meta(cmds::meta::MetaCmd),
    /// Low-level access to any compound document: streams and records.
    #[command(subcommand)]
    Raw(cmds::raw::RawCmd),
    /// Project files (.PrjPcb): documents, parameters, variants, output settings.
    #[command(subcommand)]
    Project(cmds::project::ProjectCmd),
    /// Output job files (.OutJob): inspect and edit outputs and medium settings.
    #[command(subcommand)]
    Outjob(cmds::outjob::OutjobCmd),
    /// Schematic documents (.SchDoc).
    #[command(subcommand)]
    Sch(cmds::sch::SchCmd),
    /// Schematic symbol libraries (.SchLib).
    #[command(subcommand)]
    Schlib(cmds::schlib::SchlibCmd),
    /// PCB documents (.PcbDoc).
    #[command(subcommand)]
    Pcb(cmds::pcb::PcbCmd),
    /// Footprint libraries (.PcbLib).
    #[command(subcommand)]
    Pcblib(cmds::pcblib::PcblibCmd),
    #[command(flatten)]
    Analysis(cmds::analysis::AnalysisCmd),
    /// Drive a real Altium Designer (local Windows, Windows over SSH, or Wine): compile, ERC, DRC, outputs, processes, live edits.
    #[command(subcommand, visible_alias = "ad")]
    Designer(cmds::designer::DesignerCmd),
}

fn command_path(m: &clap::ArgMatches) -> String {
    let mut parts = Vec::new();
    let mut cur = m;
    while let Some((name, sub)) = cur.subcommand() {
        parts.push(name.to_owned());
        cur = sub;
    }
    parts.join(" ")
}

fn want_json(args: &[std::ffi::OsString]) -> bool {
    if args.iter().any(|a| a == "--json") {
        return true;
    }
    if args.iter().any(|a| a == "--text") {
        return false;
    }
    !std::io::stdout().is_terminal()
}

fn emit(v: &Value, pretty: bool) {
    let s = if pretty { serde_json::to_string_pretty(v) } else { serde_json::to_string(v) };
    output::write_stdout(&format!("{}\n", s.unwrap_or_else(|_| "{\"ok\":false}".into())));
}

fn fail(command: &str, e: &Error, json_mode: bool, pretty: bool) -> ! {
    if json_mode {
        emit(&json!({"ok": false, "schema": output::SCHEMA, "command": command, "error": e}), pretty);
    } else {
        eprintln!("error: {}", e.message);
        if let Some(h) = &e.hint {
            eprintln!("hint: {h}");
        }
        if let Some(d) = &e.details {
            eprintln!("{}", serde_json::to_string_pretty(d).unwrap_or_default());
        }
    }
    std::process::exit(e.code.exit_code());
}

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let json_mode = want_json(&args);
    let pretty = args.iter().any(|a| a == "--pretty");

    let matches = match Cli::command().try_get_matches_from(&args) {
        Ok(m) => m,
        Err(e) => {
            use clap::error::ErrorKind::*;
            if e.kind() == DisplayVersion && json_mode {
                emit(&json!({"ok":true,"schema":output::SCHEMA,"command":"version",
                    "data":{"tool":"ace","version":env!("CARGO_PKG_VERSION")},"warnings":[],
                    "meta":{"version":env!("CARGO_PKG_VERSION")}}), pretty);
                return;
            }
            if matches!(e.kind(), DisplayHelp | DisplayVersion | DisplayHelpOnMissingArgumentOrSubcommand) {
                e.exit();
            }
            let msg = e.render().to_string();
            let recovery = "run `ace commands` for the full command list or `ace <command> --help`";
            let hint = msg.lines().find_map(|line| line.trim().strip_prefix("tip: "))
                .map(|tip| format!("{tip}; {recovery}")).unwrap_or_else(|| recovery.into());
            let err = Error::usage(msg.lines().next().unwrap_or("bad arguments").trim_start_matches("error: "))
                .hint(hint)
                .details(json!({"clap": msg}));
            fail("", &err, json_mode, pretty);
        }
    };
    let path = command_path(&matches);
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(c) => c,
        Err(e) => fail(&path, &Error::usage(e.to_string()), json_mode, pretty),
    };
    let g = cli.global.clone();

    let command_meta = meta::find(&path);
    let mutating = command_meta.as_ref().is_some_and(|m| matches!(m.effect, meta::Effect::Write | meta::Effect::Control));
    if g.read_only && mutating && !g.dry_run {
        fail(&path, &Error::new(altium_core::ErrorCode::Unsafe, "read-only session refuses a write/control operation")
            .hint("copy the project and unset ACE_READONLY / --read-only to edit; use --dry-run to preview"), json_mode, pretty);
    }

    let unit: Unit = g.units.into();
    units::set_output_unit(unit);
    let Some(cp) = altium_format::props::set_codepage(&g.codepage) else {
        fail(
            &path,
            &Error::usage(format!("unknown code page {:?}", g.codepage))
                .hint("use a WHATWG label such as windows-1252, windows-1251, euc-kr, gbk, shift_jis, utf-8"),
            json_mode,
            pretty,
        );
    };

    let mut shape = Shape { fields: g.fields.clone(), filters: vec![], limit: g.limit };
    for f in &g.filters {
        match Filter::parse(f) {
            Ok(f) => shape.filters.push(f),
            Err(e) => fail(&path, &e, json_mode, pretty),
        }
    }

    let ctx = Ctx { raw: g.raw, dry_run: g.dry_run, no_backup: g.no_backup, read_only: g.read_only };
    let started = Instant::now();
    let result: altium_core::Result<Out> = if g.read_only && mutating && g.dry_run
        && command_meta.as_ref().is_some_and(|m| m.backend != meta::Backend::Native) {
        // A dry-run escape hatch must never contact a host that could execute arbitrary code.
        Out::new(json!({"dry_run": true, "executed": false, "command": path,
            "hint": "local preview only: read-only prevents contacting a mutating backend"}))
    } else { match cli.cmd {
        Cmd::Meta(c) => cmds::meta::run(c, &ctx),
        Cmd::Raw(c) => cmds::raw::run(c, &ctx),
        Cmd::Project(c) => cmds::project::run(c, &ctx),
        Cmd::Outjob(c) => cmds::outjob::run(c, &ctx),
        Cmd::Sch(c) => cmds::sch::run(c, &ctx),
        Cmd::Schlib(c) => cmds::schlib::run(c, &ctx),
        Cmd::Pcb(c) => cmds::pcb::run(c, &ctx),
        Cmd::Pcblib(c) => cmds::pcblib::run(c, &ctx),
        Cmd::Analysis(c) => cmds::analysis::run(c, &ctx),
        Cmd::Designer(c) => cmds::designer::run(c, &ctx),
    } };

    match result {
        Ok(mut out) => {
            let exit_code = out.exit_code;
            if !json_mode && path == "bom" && shape.fields.is_empty() {
                shape.fields = ["line", "quantity", "designator_ranges", "comment", "footprint"].map(str::to_owned).to_vec();
            }
            let (shape_warnings, similar) = shape.diagnostics(&out.data);
            out.warnings.extend(shape_warnings);
            shape.apply(&mut out.data);
            if let Some(cp) = altium_format::props::codepage_mismatch() {
                out.warnings.push(format!(
                    "non-ASCII text in this file is stored in {cp}, not {}: rerun with --codepage {} (or set ACE_CODEPAGE) to decode names and parameters correctly",
                    altium_format::props::codepage().name(),
                    cp.to_ascii_lowercase()
                ));
            }
            let mut seen = std::collections::HashSet::new();
            out.warnings.retain(|warning| seen.insert(warning.clone()));
            if matches!(path.as_str(), "skill" | "skill show") && (g.raw || g.text) {
                output::write_stdout(out.data["markdown"].as_str().unwrap_or_default());
            } else if json_mode {
                let m = meta::find(&path);
                emit(
                    &json!({
                        "ok": true,
                        "schema": output::SCHEMA,
                        "command": path,
                        "data": out.data,
                        "warnings": out.warnings,
                        "meta": {
                            "units": unit.name(),
                            "codepage": cp,
                            "backend": m.as_ref().map(|m| m.backend),
                            "effect": m.as_ref().map(|m| m.effect),
                            "dry_run": g.dry_run,
                            "read_only": g.read_only,
                            "elapsed_ms": started.elapsed().as_millis() as u64,
                            "version": env!("CARGO_PKG_VERSION"),
                            "similar": similar,
                            "suggest": altium_format::props::codepage_mismatch().map(|cp| json!({"codepage":cp.to_ascii_lowercase()})),
                        }
                    }),
                    pretty,
                );
            } else {
                output::write_stdout(&output::render_text(&out.data));
                for w in &out.warnings {
                    eprintln!("warning: {w}");
                }
            }
            if exit_code != 0 { std::process::exit(exit_code); }
        }
        Err(e) => fail(&path, &e, json_mode, pretty),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every leaf command must have a metadata row, and every row must name a real command.
    #[test]
    fn manifest_complete() {
        let leaves = cmds::meta::leaf_paths(&Cli::command());
        let metas = meta::all();
        let missing: Vec<_> = leaves.iter().filter(|p| !metas.iter().any(|m| &m.path == p)).collect();
        assert!(missing.is_empty(), "commands without META rows: {missing:?}");
        let orphan: Vec<_> = metas.iter().filter(|m| !leaves.contains(&m.path.to_owned())).map(|m| m.path).collect();
        assert!(orphan.is_empty(), "META rows without commands: {orphan:?}");
        let mut seen = std::collections::HashSet::new();
        for m in &metas {
            assert!(seen.insert(m.path), "duplicate META row {}", m.path);
            assert!(m.example.starts_with("ace "), "example for {} must start with `ace `", m.path);
        }
    }

    #[test]
    fn clap_debug_assert() {
        Cli::command().debug_assert();
    }

    #[test]
    fn error_names_unique() {
        use altium_core::ErrorCode;
        let names: std::collections::HashSet<_> = ErrorCode::all().iter().map(|c| c.name()).collect();
        assert_eq!(names.len(), ErrorCode::all().len());
    }
}
