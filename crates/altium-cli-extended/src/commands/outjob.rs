//! `ace outjob ...` — output job containers (.OutJob).

use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde_json::{Value, json};

use altium_core::error::{Error, ErrorCode, Result};
use altium_doc::outjob::OutJob;

use crate::ctx::Ctx;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list, list_with};

#[derive(Subcommand, Debug)]
pub enum OutjobCmd {
    /// Summary: group settings, mediums and outputs.
    Info { outjob: PathBuf },
    /// Outputs with type, category, source document and which mediums they run in.
    Outputs { outjob: PathBuf },
    /// One output with its decoded configuration records.
    Output { outjob: PathBuf, output: String },
    /// Output mediums (folders, PDF publishers, printers) with their settings.
    Mediums { outjob: PathBuf },
    /// Enable or disable an output for a medium.
    Enable {
        outjob: PathBuf,
        /// Output index, name or type (a type selects every output of that type).
        output: String,
        /// Medium name or index.
        #[arg(long)]
        medium: String,
        /// Disable instead of enable.
        #[arg(long)]
        off: bool,
    },
    /// Point outputs at another source document (e.g. after renaming the PcbDoc).
    SetDocument {
        outjob: PathBuf,
        /// Output index, name or type; `all` for every output.
        output: String,
        document: String,
    },
    /// Set the variant used by the whole job.
    SetVariant { outjob: PathBuf, variant: String },
}


pub const META: &[CmdMeta] = &[
    m("outjob info", Native, Read, "{group:{...}, mediums:[...], outputs:N, by_type:{...}}", "ace outjob info Fab.OutJob", ""),
    m("outjob outputs", Native, Read, "{count, items:[{index, name, output_type, category, document, variant, enabled_for:[media], config_items}]}",
      "ace outjob outputs Fab.OutJob --where output_type~gerber", ""),
    m("outjob output", Native, Read, "{output:{...}, config:[{Record, ...}]}", "ace outjob output Fab.OutJob \"Gerber X2\"", ""),
    m("outjob mediums", Native, Read, "{count, items:[{index, name, kind, settings}]}", "ace outjob mediums Fab.OutJob", ""),
    m("outjob enable", Native, Write, "{changed:[{output, medium, enabled}], backup}",
      "ace outjob enable Fab.OutJob \"NC Drill\" --medium \"Gerber folder\"", ""),
    m("outjob set-document", Native, Write, "{changed:[{output, before, after}], backup}", "ace outjob set-document Fab.OutJob all Board.PcbDoc", ""),
    m("outjob set-variant", Native, Write, "{before, after, backup}", "ace outjob set-variant Fab.OutJob \"No LCD\"", "Use `[No Variations]` to clear."),
];


pub fn info(file: &Path, ctx: &Ctx) -> Result<Out> {
    let j = OutJob::load(&ctx.input(file)?)?;
    let outputs = j.outputs();
    let mut by_type = serde_json::Map::new();
    for o in &outputs {
        let e = by_type.entry(o.output_type.clone()).or_insert(json!(0));
        *e = json!(e.as_u64().unwrap_or(0) + 1);
    }
    let never: Vec<String> = outputs.iter().filter(|o| o.enabled_for.is_empty()).map(|o| o.name.clone()).collect();
    Out::new(json!({
        "path": j.path,
        "group": j.group_info(),
        "mediums": j.mediums().iter().map(|m| json!({"index": m.index, "name": m.name, "kind": m.kind})).collect::<Vec<_>>(),
        "outputs": outputs.len(),
        "by_type": by_type,
        "outputs_enabled_nowhere": never,
    }))
}

pub fn run(c: OutjobCmd, ctx: &Ctx) -> Result<Out> {
    match c {
        OutjobCmd::Info { outjob } => info(&outjob, ctx),
        OutjobCmd::Outputs { outjob } => {
            let j = OutJob::load(&ctx.input(&outjob)?)?;
            Out::new(list(&j.outputs())?)
        }
        OutjobCmd::Output { outjob, output } => {
            let j = OutJob::load(&ctx.input(&outjob)?)?;
            let hits = j.find_output(&output)?;
            let items: Vec<Value> =
                hits.iter().map(|o| json!({"output": o, "config": j.output_config(o.index)})).collect();
            if items.len() == 1 {
                Out::new(items.into_iter().next().unwrap())
            } else {
                Out::new(list(&items)?)
            }
        }
        OutjobCmd::Mediums { outjob } => {
            let j = OutJob::load(&ctx.input(&outjob)?)?;
            Out::new(list(&j.mediums())?)
        }
        OutjobCmd::Enable { outjob, output, medium, off } => {
            let path = ctx.input(&outjob)?;
            let mut j = OutJob::load(&path)?;
            let med = j.find_medium(&medium)?;
            let outs = j.find_output(&output)?;
            let mut changed = vec![];
            for o in &outs {
                let was = o.enabled_for.iter().any(|m| m == &med.name);
                if was != off {
                    continue;
                }
                j.set_enabled(o.index, med.index, !off);
                changed.push(json!({"output": o.name, "index": o.index, "medium": med.name, "enabled": !off}));
            }
            let backup = if changed.is_empty() { None } else { save(ctx, &path, &j)? };
            Out::new(list_with(&changed, json!({"backup": backup}))?)
        }
        OutjobCmd::SetDocument { outjob, output, document } => {
            let path = ctx.input(&outjob)?;
            let mut j = OutJob::load(&path)?;
            let outs = if output.eq_ignore_ascii_case("all") { j.outputs() } else { j.find_output(&output)? };
            let g = j.ini.section_mut("OutputGroup1").unwrap();
            let mut changed = vec![];
            for o in outs {
                let key = format!("OutputDocumentPath{}", o.index);
                if o.document != document {
                    g.set(&key, &document);
                    changed.push(json!({"output": o.name, "index": o.index, "before": o.document, "after": document}));
                }
            }
            let backup = if changed.is_empty() { None } else { save(ctx, &path, &j)? };
            Out::new(list_with(&changed, json!({"backup": backup}))?)
        }
        OutjobCmd::SetVariant { outjob, variant } => {
            let path = ctx.input(&outjob)?;
            let mut j = OutJob::load(&path)?;
            let g = j.ini.section_mut("OutputGroup1").unwrap();
            let before = g.str("VariantName");
            g.set("VariantName", &variant);
            let backup = if before != variant { save(ctx, &path, &j)? } else { None };
            Out::new(json!({"before": before, "after": variant, "backup": backup}))
        }
    }
}

fn save(ctx: &Ctx, path: &Path, job: &OutJob) -> Result<Option<PathBuf>> {
    let bytes = job.ini.to_bytes();
    if altium_format::ini::Ini::parse(&bytes) != job.ini {
        return Err(Error::new(ErrorCode::Unsafe, "the edited OutJob does not read back identically; nothing written")
            .hint("a value probably contains a line break or a character the code page cannot store"));
    }
    ctx.write_file(path, &bytes)
}
