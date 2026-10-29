//! Tool introspection: `commands`, `describe`, `skill`, `errors`, `doctor`, `inspect`.

use std::path::{Path, PathBuf};

use clap::{CommandFactory, Subcommand};
use serde_json::{Value, json};

use crate::ctx::Ctx;
use altium_core::error::{Error, ErrorCode, Result};
use altium_format::DocKind;
use altium_format::cfbx::Compound;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list, list_with};

#[derive(Subcommand, Debug)]
pub enum MetaCmd {
    /// List every command with backend, effect, return shape, arguments and an example.
    Commands {
        /// Only commands under this group (e.g. `pcb`, `designer`).
        group: Option<String>,
        /// Only path and up to 48 characters of whole words, with ... when shortened.
        #[arg(long)]
        brief: bool,
    },
    /// Show one command in full: arguments, return shape, example, notes.
    Describe {
        /// Command path, e.g. `pcb nets` or `designer drc`.
        #[arg(required = true, num_args = 1..)]
        path: Vec<String>,
    },
    /// Print the agent skill (SKILL.md, or COMMANDS.md with --file commands), generated from
    /// the live command table; `skill install|status|uninstall` manage installed copies.
    #[command(args_conflicts_with_subcommands = true)]
    Skill {
        #[command(subcommand)]
        action: Option<crate::commands::skill::SkillCmd>,
        /// Which file to print.
        #[arg(long, value_enum, default_value = "skill")]
        file: crate::commands::skill::SkillFile,
    },
    /// Restore a design file from its preserved first .bak, after parsing the backup.
    Restore {
        /// Current design file; its original backup is <file>.bak.
        file: PathBuf,
        /// Oldest preserved backup, not the last edit.
        #[arg(long, default_value = "first", value_parser = ["first"])]
        from: String,
    },
    /// List error codes and exit codes.
    Errors,
    /// Check what works on this machine: native parsing, Altium Designer host (local Windows, SSH or Wine), bridge.
    Doctor {
        /// Native/configuration summary only: never contact a Designer host.
        #[arg(long)]
        quick: bool,
    },
    /// Identify any Altium file and summarise it (dispatches to the right `info` command).
    Inspect {
        file: PathBuf,
    },
}

pub const META: &[CmdMeta] = &[
    m("restore", Native, Write, "{file, from, backup, pre_restore, current:{bytes,modified_unix_ms}, restore_source:{bytes,modified_unix_ms}, changed, verified:{reparsed,read_back}, bytes}",
      "ace restore Board.SchDoc --from first --dry-run",
      "first means the oldest preserved .bak, not the last edit. Re-parses it before replacing one design file. Saves current bytes to <file>.pre-restore even with --no-backup; an existing recovery copy blocks replacement. --dry-run reports both sizes and modification times without writing. The .bak is kept; recovery covers one file."),
    m("commands", Meta, Read,
      "{count, items:[{path, about, backend, effect, returns, example, notes, args:[...]}]}",
      "ace commands pcb --brief",
      "Call this first. backend=native needs nothing installed; backend=designer needs Altium Designer on this Windows machine, a Windows machine over SSH, or Wine (see `ace doctor`)."),
    m("describe", Meta, Read, "{path, about, backend, effect, returns, example, notes, args, flags, global_flags, supports_dry_run}",
      "ace describe designer drc", ""),
    m("skill", Meta, Read, "{name, file, markdown, files}", "ace skill --file commands",
      "SKILL.md is the short guide (when to use, rules, workflows); COMMANDS.md is the full reference. Both are generated from this binary."),
    m("skill show", Meta, Read, "{name, file, markdown, files}", "ace skill show --raw", "--raw or --text prints Markdown directly; default remains a JSON envelope."),
    m("skill install", Meta, Control, "{skill, version, count, items:[{target, path, written, unchanged, replaced_symlink}]}",
      "ace skill install --target claude --target codex",
      "Copies SKILL.md + COMMANDS.md to <home>/.agents|.claude|.codex|.gemini/skills/altium-cli-extended (default: .agents plus every agent home that exists), or DIR/altium-cli-extended with --dir. Works the same on Windows, macOS and Linux; honours --dry-run."),
    m("skill status", Meta, Read, "{skill, version, count, items:[{target, path, installed, files, up_to_date, symlink}]}",
      "ace skill status", "up_to_date=false: the copy was written by another version; rerun `skill install`."),
    m("skill uninstall", Meta, Control, "{skill, count, items:[{target, path, removed}]}",
      "ace skill uninstall --target all", "Removes only SKILL.md/COMMANDS.md (and the directory when it is then empty)."),
    m("errors", Meta, Read, "{count, items:[{code, exit_code, category, meaning}]}", "ace errors", ""),
    m("doctor", Meta, Read, "{native, designer:{host, altium, bridge, ...}, ready:{native, designer}}",
      "ace doctor --quick", "Use --quick for offline/native tasks (no host contact). Full doctor probes Designer; run when a designer command fails with DESIGNER_UNAVAILABLE. Native commands never need anything installed."),
    m("inspect", Native, Read, "same as the matching `<kind> info` command plus {kind}",
      "ace inspect Board.PcbDoc", "Use when you do not know what a file is."),
];

/// Space-joined paths of every leaf command. A command that has subcommands but also runs on
/// its own (`skill`) counts as a leaf too.
pub fn leaf_paths(cmd: &clap::Command) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(c: &clap::Command, prefix: &str, out: &mut Vec<String>) {
        for sub in c.get_subcommands() {
            if sub.get_name() == "help" {
                continue;
            }
            let p = if prefix.is_empty() { sub.get_name().to_owned() } else { format!("{prefix} {}", sub.get_name()) };
            if sub.has_subcommands() {
                if sub.is_args_conflicts_with_subcommands_set() {
                    out.push(p.clone());
                }
                walk(sub, &p, out);
            } else {
                out.push(p);
            }
        }
    }
    walk(cmd, "", &mut out);
    out
}

pub fn find_cmd<'a>(root: &'a clap::Command, path: &str) -> Option<&'a clap::Command> {
    let mut cur = root;
    for part in path.split_whitespace() {
        cur = cur
            .get_subcommands()
            .find(|s| s.get_name() == part || s.get_all_aliases().any(|a| a == part))?;
    }
    Some(cur)
}

fn argument_help(name: &str) -> String {
    match name {
        "file" => "Input Altium document path; quote spaces".into(),
        "project" => "Project (.PrjPcb) path; resolves all referenced documents".into(),
        "pcb" => "PCB document (.PcbDoc) path".into(),
        "sch" => "Schematic document (.SchDoc) path".into(),
        "outjob" => "Output job (.OutJob) path".into(),
        "designator" | "component" => "Component designator from the component list, e.g. R5; quote 'U?'".into(),
        "name" => "Exact object name from the corresponding list command".into(),
        "value" => "New field/parameter value; include units for lengths (e.g. 0.2mm)".into(),
        "field" => "Field name to change; see notes for supported fields".into(),
        "key" => "Altium property key".into(),
        "dir" => "Directory path".into(),
        "footprint" => "Footprint name from `ace pcblib list`".into(),
        "symbol" => "Symbol name from `ace schlib list`".into(),
        "net" => "Net name from a nets/netlist command".into(),
        "stream" => "Compound stream path from `ace raw streams`".into(),
        "section" => "INI section name from settings".into(),
        "variant" => "Assembly variant name, unique ID or 1-based index".into(),
        "new" => "New object name/designator".into(),
        _ => format!("{} for this operation; see the example and notes", name.replace('_', " ")),
    }
}

fn args_json(c: &clap::Command) -> Vec<Value> {
    c.get_arguments()
        .filter(|a| !a.is_global_set() && !matches!(a.get_id().as_str(), "help" | "version"))
        .map(|a| {
            let takes_value = a.get_num_args().map(|n| n.takes_values()).unwrap_or(true)
                && !matches!(a.get_action(), clap::ArgAction::SetTrue | clap::ArgAction::SetFalse | clap::ArgAction::Count);
            let kind = if a.is_positional() { "positional" } else if takes_value { "option" } else { "flag" };
            let choices: Vec<String> = a.get_possible_values().iter().map(|p| p.get_name().to_owned()).collect();
            let defaults: Vec<String> = a.get_default_values().iter().map(|d| d.to_string_lossy().into_owned()).collect();
            let mut o = json!({
                "name": a.get_id().as_str(),
                "kind": kind,
                "required": a.is_required_set(),
                "help": a.get_help().map(|h| h.to_string()).unwrap_or_else(|| argument_help(a.get_id().as_str())),
            });
            if let Some(l) = a.get_long() {
                o["long"] = json!(format!("--{l}"));
            }
            if let Some(s) = a.get_short() {
                o["short"] = json!(format!("-{s}"));
            }
            if matches!(a.get_action(), clap::ArgAction::Append) || a.get_num_args().is_some_and(|n| n.max_values() > 1) {
                o["multiple"] = json!(true);
            }
            if !choices.is_empty() && kind != "flag" {
                o["choices"] = json!(choices);
            }
            if !defaults.is_empty() && kind != "flag" {
                o["default"] = json!(defaults.join(","));
            }
            o
        })
        .collect()
}

pub fn describe_json(root: &clap::Command, path: &str, with_args: bool) -> Option<Value> {
    let c = find_cmd(root, path)?;
    let meta = crate::meta::find(path);
    let mut v = json!({
        "path": path,
        "about": c.get_about().map(|a| a.to_string()).unwrap_or_default(),
    });
    v["notes"] = json!("");
    if let Some(m) = &meta {
        v["backend"] = json!(m.backend);
        v["effect"] = json!(m.effect);
        v["returns"] = json!(m.returns);
        v["example"] = json!(m.example);
        v["notes"] = json!(m.notes);
    }
    if with_args {
        v["args"] = json!(args_json(c));
        v["flags"] = json!(global_flags(root));
        v["global_flags"] = json!(global_flags(root));
        v["supports_dry_run"] = json!(meta.as_ref().is_some_and(|m| matches!(m.effect, Write | Export | Control) || matches!(m.path, "bom" | "netlist")));
    }
    Some(v)
}

pub fn global_flags(root: &clap::Command) -> Vec<Value> {
    args_json(&root.clone().mut_args(|a| a.global(false)))
}

pub fn run(c: MetaCmd, ctx: &Ctx) -> Result<Out> {
    let root = crate::Cli::command();
    match c {
        MetaCmd::Commands { group, brief } => {
            let items: Vec<Value> = leaf_paths(&root)
                .into_iter()
                .filter(|p| group.as_deref().is_none_or(|g| p == g || p.starts_with(&format!("{g} "))))
                .filter_map(|p| {
                    if brief {
                        let c = find_cmd(&root, &p)?;
                        let about = c.get_about().map(|a| a.to_string()).unwrap_or_default();
                        let short = if about.chars().count() > 48 {
                            let prefix: String = about.chars().take(46).collect();
                            let end = prefix.rfind(char::is_whitespace).unwrap_or(0);
                            format!("{}...", prefix[..end].trim_end())
                        } else { about };
                        Some(json!({"path":p,"about":short}))
                    } else {
                        let mut full = describe_json(&root, &p, true)?;
                        // The manifest carries inherited flags once at the top level.
                        full.as_object_mut()?.remove("flags");
                        full.as_object_mut()?.remove("global_flags");
                        Some(full)
                    }
                })
                .collect();
            if items.is_empty() {
                return Err(Error::not_found(format!("no commands under {:?}", group.unwrap_or_default()))
                    .hint("groups: project outjob sch schlib pcb pcblib bom netlist check diff designer raw"));
            }
            Out::new(if brief { list(&items)? } else { list_with(&items, json!({"global_flags": global_flags(&root)}))? })
        }
        MetaCmd::Describe { path } => {
            let p = path.join(" ");
            let mut v = describe_json(&root, &p, true).ok_or_else(|| {
                Error::not_found(format!("no command {p:?}")).hint("run `ace commands --brief`")
            })?;
            if find_cmd(&root, &p).is_some_and(|c| c.has_subcommands()) {
                let subs: Vec<String> =
                    leaf_paths(&root).into_iter().filter(|l| l.starts_with(&format!("{p} "))).collect();
                v["group"] = json!(true);
                v["subcommands"] = json!(subs);
                return Out::new(v);
            }
            Out::new(v)
        }
        MetaCmd::Skill { action: None, file } => crate::commands::skill::run_print(file, &root),
        MetaCmd::Skill { action: Some(a), .. } => crate::commands::skill::run(a, &root, ctx),
        MetaCmd::Restore { file, from } => {
            let path = ctx.input(&file)?;
            let mut name = path.as_os_str().to_os_string(); name.push(".bak");
            let backup = PathBuf::from(name);
            let bytes = std::fs::read(&backup).map_err(|e| Error::from(e)
                .hint(format!("no usable backup at {}; use version control or a project copy", backup.display())))?;
            altium_doc::validate::bytes(&path, &bytes).map_err(|e| Error::new(ErrorCode::Unsafe, format!("backup validation failed: {}", e.message))
                .hint("current file was kept; recover a valid backup or use version control"))?;
            let current = std::fs::read(&path)?;
            let state = |file: &Path, len: usize| -> Result<Value> {
                let modified = std::fs::metadata(file)?.modified()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis());
                Ok(json!({"bytes":len,"modified_unix_ms":modified}))
            };
            let current_state = state(&path, current.len())?;
            let backup_state = state(&backup, bytes.len())?;
            let changed = current != bytes;
            let pre_restore = if changed {
                let mut name = path.as_os_str().to_os_string(); name.push(".pre-restore");
                let recovery = PathBuf::from(name);
                ctx.preserve_file(&recovery, &current)?;
                ctx.write_file(&path, &bytes)?;
                Some(recovery)
            } else { None };
            let read_back = !ctx.dry_run && std::fs::read(&path)? == bytes;
            if !ctx.dry_run && !read_back {
                return Err(Error::new(ErrorCode::Unsafe, "restored file differs from validated backup")
                    .hint("keep the .bak and inspect disk permissions or concurrent writers"));
            }
            Out::new(json!({"file":path,"from":from,"backup":backup,"pre_restore":pre_restore,
                "current":current_state,"restore_source":backup_state,"changed":changed,"bytes":bytes.len(),
                "verified":{"reparsed":true,"read_back":read_back}}))
        }
        MetaCmd::Errors => {
            let items: Vec<Value> = ErrorCode::all()
                .iter()
                .map(|c| json!({"code": c.name(), "exit_code": c.exit_code(), "category": c.category(), "meaning": error_meaning(*c)}))
                .collect();
            Out::new(list(&items)?)
        }
        MetaCmd::Doctor { quick: true } => Out::new(json!({
            "native":{"ok":true,"formats":["PrjPcb","OutJob","SchDoc","SchLib","PcbDoc","PcbLib"]},
            "designer":{"state":"unprobed","hint":"run ace designer status or ace designer ping when Designer is needed"},
            "ready":{"native":true,"designer":null}})),
        MetaCmd::Doctor { quick: false } => {
            let designer = altium_designer::doctor();
            let ready_designer = designer.get("ready").and_then(Value::as_bool).unwrap_or(false);
            Out::new(json!({
                "native": {"ok": true, "formats": ["PrjPcb", "OutJob", "SchDoc", "SchLib", "PcbDoc", "PcbLib", "IntLib (raw)"]},
                "designer": designer,
                "ready": {"native": true, "designer": ready_designer},
            }))
        }
        MetaCmd::Inspect { file } => inspect(&file, ctx),
    }
}

pub fn error_meaning(c: ErrorCode) -> &'static str {
    match c {
        ErrorCode::Usage => "bad arguments or unknown command; read `ace describe <cmd>`",
        ErrorCode::FileNotFound => "input path does not exist",
        ErrorCode::WrongFileType => "file is not the Altium document type this command expects",
        ErrorCode::Parse => "file is the right type but could not be decoded",
        ErrorCode::NotFound => "named object (designator, net, stream, symbol...) not in the file",
        ErrorCode::Ambiguous => "query matched several objects; narrow it",
        ErrorCode::Unsafe => "write refused to protect the design (locked, open in Altium, unverifiable)",
        ErrorCode::GateFailed => "command succeeded but an explicit gate matched; see data.gate",
        ErrorCode::UnsupportedNative => "operation requires Altium Designer; follow the hint",
        ErrorCode::DesignerUnavailable => "Altium Designer host (local, SSH or Wine) or its bridge not set up; run `ace doctor`",
        ErrorCode::DesignerFailed => "Altium ran but the operation failed; see error.details",
        ErrorCode::Timeout => "Altium did not answer in time; it may be busy or stuck on a dialog",
        ErrorCode::Io => "file system error",
        ErrorCode::Internal => "bug in altium-cli-extended; please report with the command line",
    }
}

pub fn inspect(file: &Path, ctx: &Ctx) -> Result<Out> {
    let file = ctx.input(file)?;
    let kind = DocKind::of(&file);
    let mut out = match kind {
        DocKind::Project => crate::commands::project::info(&file, ctx)?,
        DocKind::OutJob => crate::commands::outjob::info(&file, ctx)?,
        DocKind::Schematic => crate::commands::sch::info(&file, ctx)?,
        DocKind::SchLib => crate::commands::schlib::info(&file, ctx)?,
        DocKind::Pcb => crate::commands::pcb::info(&file, ctx)?,
        DocKind::PcbLib => crate::commands::pcblib::info(&file, ctx)?,
        DocKind::IntLib | DocKind::Harness | DocKind::Other => {
            let cf = Compound::open(&file).map_err(|e| {
                if e.code == ErrorCode::WrongFileType {
                    Error::new(ErrorCode::WrongFileType, format!("{} is not a recognised Altium file", file.display()))
                        .hint("supported: .PrjPcb .OutJob .SchDoc .SchLib .PcbDoc .PcbLib (.IntLib via `raw`)")
                } else {
                    e
                }
            })?;
            Out::new(json!({"streams": cf.streams().len(), "note": "no dedicated decoder; use `ace raw streams`"}))?
        }
    };
    if let Value::Object(o) = &mut out.data {
        o.insert("kind".into(), json!(kind));
    }
    Ok(out)
}

