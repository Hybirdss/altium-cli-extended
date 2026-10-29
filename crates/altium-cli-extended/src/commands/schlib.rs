//! `ace schlib ...` — schematic symbol libraries (.SchLib): read, plus parameter and description edits.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde_json::{Value, json};

use altium_core::error::Result;
use altium_doc::schlib::{self, SchLib};

use crate::ctx::Ctx;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list};

#[derive(Subcommand, Debug)]
pub enum SchlibCmd {
    /// Summary: symbol count, pins, fonts, embedded files.
    Info { file: PathBuf },
    /// Symbols with part count, pin count, description and footprint.
    List { file: PathBuf },
    /// One symbol in full: pins, parameters, models (footprint links), drawing primitives.
    Show { file: PathBuf, symbol: String },
    /// Pins of one symbol (binary pin records decoded).
    Pins { file: PathBuf, symbol: String },
    /// Search every text property of every symbol (case-insensitive substring).
    Find {
        file: PathBuf,
        /// Text to look for.
        #[arg(value_name = "TEXT")]
        needle: String,
    },
    /// Set (or add) a parameter of one symbol, e.g. MPN or Comment.
    SetParam { file: PathBuf, symbol: String, name: String, value: String },
    /// Set a symbol's description.
    SetDescription { file: PathBuf, symbol: String, description: String },
}

pub const META: &[CmdMeta] = &[
    m("schlib info", Native, Read,
      "{file, header, symbols, pins, models, fonts:[...], storage_files, warnings_count}",
      "ace schlib info Symbols.SchLib", ""),
    m("schlib list", Native, Read,
      "{count, items:[{name, storage, description, part_count, pins, designator?, comment?, footprint?, kind}]}",
      "ace schlib list Symbols.SchLib --where footprint~SOIC", "`name` is the reference for `schlib show/pins`."),
    m("schlib show", Native, Read,
      "{name, storage, description, part_count, pins:[...], parameters:[...], models:[{kind,name,current,datafiles,pin_map}], primitives:[{index,type,part_id,location?,corner?,points?...}], streams}",
      "ace schlib show Symbols.SchLib LM317", "Pin and primitive coordinates are symbol-local."),
    m("schlib pins", Native, Read,
      "{count, items:[{index, designator, name, electrical, part_id, location, hot_end, length, orientation, hidden, extra?, binary}]}",
      "ace schlib pins Symbols.SchLib LM317", "extra = per-pin side streams (PinPackageLength, PinSymbolLineWidth...)."),
    m("schlib find", Native, Read, "{count, items:[{index, type, owner, key, value, stream}]}",
      "ace schlib find Symbols.SchLib SOIC-8", "owner/stream = symbol name."),
    m("schlib set-param", Native, Write, "{changed, file, changes:[{symbol, stream, record, action, key, before, after}], backup, verified}",
      "ace schlib set-param Symbols.SchLib LM317 MPN LM317DCYR",
      "New parameters are hidden and copy a sibling parameter's position/font. Placed instances on sheets keep their own copy: update them with `sch set-param` (or Altium's Update From Libraries)."),
    m("schlib set-description", Native, Write, "{changed, file, changes, backup, verified}",
      "ace schlib set-description Symbols.SchLib LM317 \"1.5A adjustable regulator\"", "Updates the component record and the library header entry."),
];

fn load(file: &Path, ctx: &Ctx) -> Result<SchLib> {
    schlib::load_with(&ctx.input(file)?, ctx.raw)
}

fn lib_out(l: &SchLib, data: Value) -> Result<Out> {
    let mut w = l.warnings.clone();
    for s in &l.symbols {
        w.extend(s.warnings.iter().map(|x| format!("{}: {x}", s.name)));
    }
    Ok(Out::new(data)?.warnings(w))
}

fn open_for_edit(file: &Path, ctx: &Ctx) -> Result<(SchLib, Vec<u8>)> {
    let path = ctx.input(file)?;
    let bytes = std::fs::read(&path)?;
    Ok((schlib::load_bytes(&path, bytes.clone(), false)?, bytes))
}

fn write_edit(ctx: &Ctx, l: &SchLib, bytes: &[u8], r: Result<(Vec<u8>, Vec<schlib::edit::Change>)>) -> Result<Out> {
    let (new_bytes, changes) = r?;
    if changes.is_empty() {
        return Out::new(json!({"changed": false, "file": l.path, "changes": changes, "backup": null, "verified": true}));
    }
    schlib::edit::verify(l, bytes, &new_bytes, &changes)?;
    let backup = ctx.write_file(&l.path, &new_bytes)?;
    if !ctx.dry_run && std::fs::read(&l.path)? != new_bytes {
        return Err(altium_core::Error::new(altium_core::ErrorCode::Unsafe, "file on disk differs from what was written"));
    }
    Out::new(json!({"changed": true, "file": l.path, "changes": changes, "backup": backup, "verified": true}))
}

pub fn info(file: &Path, ctx: &Ctx) -> Result<Out> {
    let l = load(file, ctx)?;
    let h = &l.header;
    let fonts: Vec<Value> = (1..=h.int_or("FONTIDCOUNT", 0))
        .map(|i| json!({"id": i, "name": h.str(&format!("FONTNAME{i}")), "size": h.int(&format!("SIZE{i}"))}))
        .collect();
    lib_out(
        &l,
        json!({
            "file": l.path,
            "header": h.str("HEADER"),
            "unique_id": h.get("UNIQUEID"),
            "declared_symbols": h.int("COMPCOUNT"),
            "symbols": l.symbols.len(),
            "pins": l.symbols.iter().map(|s| s.pins.len()).sum::<usize>(),
            "models": l.symbols.iter().map(|s| s.models.len()).sum::<usize>(),
            "multi_part_symbols": l.symbols.iter().filter(|s| s.part_count > 1).count(),
            "fonts": fonts,
            "storage_files": l.storage,
            "warnings_count": l.warnings.len() + l.symbols.iter().map(|s| s.warnings.len()).sum::<usize>(),
        }),
    )
}

pub fn run(c: SchlibCmd, ctx: &Ctx) -> Result<Out> {
    match c {
        SchlibCmd::Info { file } => info(&file, ctx),
        SchlibCmd::List { file } => {
            let l = load(&file, ctx)?;
            let items: Vec<Value> = l
                .symbols
                .iter()
                .map(|s| {
                    let mut v = json!({
                        "name": s.name,
                        "storage": s.storage,
                        "description": s.description,
                        "part_count": s.part_count,
                        "pins": s.pins.len(),
                        "kind": s.kind,
                        "models": s.models.len(),
                    });
                    let o = v.as_object_mut().unwrap();
                    for (k, val) in [("designator", &s.designator), ("comment", &s.comment), ("footprint", &s.footprint)] {
                        if let Some(x) = val {
                            o.insert(k.into(), json!(x));
                        }
                    }
                    v
                })
                .collect();
            lib_out(&l, list(&items)?)
        }
        SchlibCmd::Show { file, symbol } => {
            let l = load(&file, ctx)?;
            let s = l.symbol(&symbol)?;
            let w: Vec<String> = s.warnings.clone();
            Ok(Out::new(s)?.warnings(w))
        }
        SchlibCmd::Pins { file, symbol } => {
            let l = load(&file, ctx)?;
            let s = l.symbol(&symbol)?;
            Ok(Out::new(list(&s.pins)?)?.warnings(s.warnings.clone()))
        }
        SchlibCmd::Find { file, needle } => {
            let l = load(&file, ctx)?;
            lib_out(&l, list(&l.find(&needle))?)
        }
        SchlibCmd::SetParam { file, symbol, name, value } => {
            let (l, bytes) = open_for_edit(&file, ctx)?;
            write_edit(ctx, &l, &bytes, schlib::edit::set_param(&l, &bytes, &symbol, &name, &value))
        }
        SchlibCmd::SetDescription { file, symbol, description } => {
            let (l, bytes) = open_for_edit(&file, ctx)?;
            write_edit(ctx, &l, &bytes, schlib::edit::set_description(&l, &bytes, &symbol, &description))
        }
    }
}
