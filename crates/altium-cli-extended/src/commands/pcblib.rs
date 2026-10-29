//! `ace pcblib ...` — footprint libraries (.PcbLib): read natively, plus footprint edits.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde_json::{Value, json};

use altium_core::error::{Error, ErrorCode, Result};
use altium_doc::pcb::svg;
use altium_doc::pcblib::{self, PcbLib, edit};

use crate::commands::pcb::unique_name;
use crate::ctx::Ctx;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list_with};

#[derive(Subcommand, Debug)]
pub enum PcblibCmd {
    /// Search footprint names across libraries or directories, optionally including pads.
    Find {
        /// Case-insensitive substring of the footprint name, e.g. 0603.
        pattern: String,
        /// .PcbLib files or directories to search recursively.
        #[arg(required = true, num_args = 1..)]
        paths: Vec<PathBuf>,
        /// Include the complete pad objects for follow-up analysis.
        #[arg(long)]
        pads: bool,
    },
    /// Summary of the document: footprints, models, layers, file version.
    Info { file: PathBuf },
    /// Footprints with pad count, description, height, extent and 3D models.
    List { file: PathBuf },
    /// One footprint in full: pads, primitive counts, bodies, parameters, texts.
    Show {
        file: PathBuf,
        footprint: String,
        /// Include every primitive (tracks, arcs, fills, regions) too.
        #[arg(long)]
        all: bool,
    },
    /// Pads of one footprint.
    Pads { file: PathBuf, footprint: String },
    /// Embedded 3D models; `--extract DIR` writes the decompressed STEP files.
    Models {
        file: PathBuf,
        #[arg(long)]
        extract: Option<PathBuf>,
    },
    /// Write an SVG preview of one footprint.
    Svg {
        file: PathBuf,
        footprint: String,
        #[arg(long, short)]
        out: PathBuf,
        /// Layers to draw (comma-separated); default: copper, overlays, keep-out, multi-layer.
        #[arg(long, value_delimiter = ',')]
        layers: Vec<String>,
    },
    /// Rename a footprint: the library list, its storage and the names inside it.
    Rename { file: PathBuf, footprint: String, name: String },
    /// Set a footprint parameter: DESCRIPTION, HEIGHT (with a unit) or any other Parameters key.
    SetParam { file: PathBuf, footprint: String, name: String, value: String },
    /// Delete a footprint and its storage.
    Remove { file: PathBuf, footprint: String },
    /// Copy a footprint from another .PcbLib, with the 3D models it uses.
    Copy {
        file: PathBuf,
        /// Library to copy from.
        source: PathBuf,
        footprint: String,
        /// Name in this library (default: the source name).
        #[arg(long)]
        name: Option<String>,
    },
}

pub const META: &[CmdMeta] = &[
    m("pcblib find", Native, Read, "{libraries_scanned,total_footprints,count,items:[{file,name,pads,pad_numbers,pad_number_values,pad_items?,...}],skipped,incomplete}",
      "ace pcblib find 0603 ./libraries --pads",
      "Case-insensitive substring search across .PcbLib files; skips hidden directories and symlinks. IntLib containers and parse failures appear in skipped and warnings, never as an empty successful search. pad_numbers is the legacy count; pad_number_values lists the distinct numbers."),
    m("pcblib info", Native, Read,
      "{file, format, footprints, models, embedded_models, layers:[enabled layer names], stackup:{source, total_thickness}}",
      "ace pcblib info Footprints.PcbLib", ""),
    m("pcblib list", Native, Read,
      "{count, items:[{index, name, pads, pad_numbers, description, height, primitives:{...}, bbox, mount, models}]}",
      "ace pcblib list Footprints.PcbLib --where mount=smd --fields name,pads", ""),
    m("pcblib show", Native, Read,
      "{footprint:{index, name, description, height, parameters, pads:N, ...}, pads:[...], bodies:[...], texts:[...], tracks?, arcs?, fills?, regions?}",
      "ace pcblib show Footprints.PcbLib SOIC-8", "Name match is exact, then case-insensitive; `#N` selects by index."),
    m("pcblib pads", Native, Read,
      "{footprint, count, items:[{index, number, layer, x, y, rotation, shape, size, hole_size, plated, ...}]}",
      "ace pcblib pads Footprints.PcbLib R0603", "Coordinates are relative to the footprint origin."),
    m("pcblib models", Native, Export,
      "{count, items:[{index, id, name, embedded, stream, compressed_size, used_by, extracted?:{path, size}}], extract_dir?}",
      "ace pcblib models Footprints.PcbLib --extract ./step", "Without --extract nothing is written; honours --dry-run."),
    m("pcblib svg", Native, Export, "{out, footprint, layers, width, height, elements}",
      "ace pcblib svg Footprints.PcbLib SOIC-8 --out soic8.svg", ""),
    m("pcblib rename", Native, Write, "{changed, file, changes:[{target, stream, action, key, before, after}], backup, verified, footprints}",
      "ace pcblib rename Footprints.PcbLib SOIC-8 SOIC-8N",
      "Also moves the footprint storage (name with / as _, cut at 31 characters) and the PATTERN parameter. Case-only renames keep the storage spelling. Other footprints keep their bytes."),
    m("pcblib set-param", Native, Write, "{changed, file, changes, backup, verified, footprints}",
      "ace pcblib set-param Footprints.PcbLib SOIC-8 HEIGHT 2.5mm",
      "Keys are Altium's: DESCRIPTION, HEIGHT (a length with a unit), PATTERN, or any user parameter. Only the field's bytes change."),
    m("pcblib remove", Native, Write, "{changed, file, changes, backup, verified, footprints}",
      "ace pcblib remove Footprints.PcbLib SOIC-8", "3D models the footprint used stay in the library."),
    m("pcblib copy", Native, Write, "{changed, file, changes, backup, verified, footprints}",
      "ace pcblib copy Footprints.PcbLib Other.PcbLib SOIC-8 --name SOIC-8N",
      "Copies the footprint's streams byte for byte; embedded models it references are copied when the library lacks them. Linked models keep their references and produce a warning. Use --name when the name is taken."),
];

fn open(file: &Path, ctx: &Ctx) -> Result<PcbLib> {
    pcblib::load_with(&ctx.input(file)?, ctx.raw)
}

fn open_for_edit(file: &Path, ctx: &Ctx) -> Result<(PcbLib, Vec<u8>)> {
    let path = ctx.input(file)?;
    let bytes = std::fs::read(&path)?;
    Ok((pcblib::load_bytes(&path, bytes.clone(), false)?, bytes))
}

fn write_edit(ctx: &Ctx, lib: &PcbLib, bytes: &[u8], r: Result<edit::Edit>) -> Result<Out> {
    let e = r?;
    if e.changes.is_empty() {
        return Out::new(json!({"changed": false, "file": lib.path, "changes": [], "backup": null, "verified": true, "footprints": lib.footprints.len()}));
    }
    let new = edit::verify(lib, bytes, &e)?;
    let backup = ctx.write_file(&lib.path, &e.bytes)?;
    if !ctx.dry_run && std::fs::read(&lib.path)? != e.bytes {
        return Err(altium_core::Error::new(altium_core::ErrorCode::Unsafe, "file on disk differs from what was written")
            .hint(backup.as_ref().map(|p| format!("restore the original from {}", p.display()))
                .unwrap_or_else(|| "restore the file from your own backup; retry with backups enabled".into())));
    }
    Out::new(json!({"changed": true, "file": lib.path, "changes": e.changes, "backup": backup, "verified": true, "footprints": new.footprints.len()})).map(|out| out.warnings(e.warnings))
}

pub fn run(c: PcblibCmd, ctx: &Ctx) -> Result<Out> {
    match c {
        PcblibCmd::Find { pattern, paths, pads } => find(&pattern, &paths, pads, ctx),
        PcblibCmd::Info { file } => info(&file, ctx),
        PcblibCmd::List { file } => {
            let lib = open(&file, ctx)?;
            let items: Vec<_> = lib.footprints.iter().map(|f| f.summary(&lib.models)).collect();
            Ok(Out::new(list_with(&items, json!({}))?)?.warnings(lib.warnings.clone()))
        }
        PcblibCmd::Show { file, footprint, all } => {
            let lib = open(&file, ctx)?;
            let f = lib.footprint(&footprint)?;
            let s = f.summary(&lib.models);
            let mut v = json!({
                "footprint": {
                    "index": f.index, "name": f.name, "storage": f.storage, "description": f.description,
                    "height": f.height, "parameters": f.parameters, "pads": s.pads, "pad_numbers": s.pad_numbers,
                    "mount": s.mount, "bbox": s.bbox, "primitives": s.primitives, "models": s.models,
                },
                "pads": f.pads,
                "bodies": f.bodies,
                "texts": f.texts,
            });
            if all {
                v["tracks"] = serde_json::to_value(&f.tracks)?;
                v["arcs"] = serde_json::to_value(&f.arcs)?;
                v["fills"] = serde_json::to_value(&f.fills)?;
                v["regions"] = serde_json::to_value(&f.regions)?;
                v["vias"] = serde_json::to_value(&f.vias)?;
            }
            Ok(Out::new(v)?.warnings(f.warnings.clone()))
        }
        PcblibCmd::Pads { file, footprint } => {
            let lib = open(&file, ctx)?;
            let f = lib.footprint(&footprint)?;
            Ok(Out::new(list_with(&f.pads, json!({"footprint": f.name}))?)?.warnings(f.warnings.clone()))
        }
        PcblibCmd::Models { file, extract } => {
            let mut lib = open(&file, ctx)?;
            let mut items: Vec<Value> = lib.models.iter().map(|m| serde_json::to_value(m).unwrap_or_default()).collect();
            if let Some(dir) = &extract {
                if !ctx.dry_run {
                    std::fs::create_dir_all(dir)?;
                }
                let mut used = std::collections::HashSet::new();
                for (i, item) in items.iter_mut().enumerate() {
                    if lib.models[i].stream.is_none() {
                        continue;
                    }
                    let data = lib.model_data(i)?;
                    let path = dir.join(unique_name(&lib.models[i].name, i, &mut used));
                    ctx.export_file(&path, &data)?;
                    item["extracted"] = json!({"path": path, "size": data.len()});
                }
            }
            Ok(Out::new(list_with(&items, json!({"extract_dir": extract}))?)?.warnings(lib.warnings.clone()))
        }
        PcblibCmd::Rename { file, footprint, name } => {
            let (lib, bytes) = open_for_edit(&file, ctx)?;
            write_edit(ctx, &lib, &bytes, edit::rename(&lib, &bytes, &footprint, &name))
        }
        PcblibCmd::SetParam { file, footprint, name, value } => {
            let (lib, bytes) = open_for_edit(&file, ctx)?;
            write_edit(ctx, &lib, &bytes, edit::set_param(&lib, &bytes, &footprint, &name, &value))
        }
        PcblibCmd::Remove { file, footprint } => {
            let (lib, bytes) = open_for_edit(&file, ctx)?;
            write_edit(ctx, &lib, &bytes, edit::remove(&lib, &bytes, &footprint))
        }
        PcblibCmd::Copy { file, source, footprint, name } => {
            let (lib, bytes) = open_for_edit(&file, ctx)?;
            let spath = ctx.input(&source)?;
            let sbytes = std::fs::read(&spath)?;
            let slib = pcblib::load_bytes(&spath, sbytes.clone(), false)?;
            write_edit(ctx, &lib, &bytes, edit::copy_from(&lib, &bytes, &slib, &sbytes, &footprint, name.as_deref()))
        }
        PcblibCmd::Svg { file, footprint, out, layers } => {
            let lib = open(&file, ctx)?;
            let f = lib.footprint(&footprint)?;
            let ls = layers
                .iter()
                .map(|l| {
                    altium_doc::pcb::Layer::from_name(l).ok_or_else(|| {
                        altium_core::Error::usage(format!("unknown layer {l:?}"))
                            .hint("use Altium layer names: TopLayer, BottomLayer, TopOverlay, MultiLayer, Mechanical13, ...")
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let r = svg::render(&f.scene(), &svg::Options { layers: ls, text_boxes: false });
            ctx.export_file(&out, r.svg.as_bytes())?;
            Ok(Out::new(json!({"out": out, "footprint": f.name, "layers": r.layers, "width": r.width, "height": r.height, "elements": r.elements}))?
                .warnings(f.warnings.clone()))
        }
    }
}

pub fn info(file: &Path, ctx: &Ctx) -> Result<Out> {
    let lib = open(file, ctx)?;
    let enabled: Vec<String> = lib.layers.iter().filter(|l| l.enabled).map(|l| l.layer.name()).collect();
    let pads: usize = lib.footprints.iter().map(|f| f.pads.len()).sum();
    Ok(Out::new(json!({
        "file": lib.path,
        "format": lib.file,
        "footprints": lib.footprints.len(),
        "pads": pads,
        "models": lib.models.len(),
        "embedded_models": lib.models.iter().filter(|m| m.stream.is_some()).count(),
        "layers": enabled,
        "stackup": {"source": lib.stackup.source, "total_thickness": lib.stackup.total_thickness},
        "names": lib.footprints.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
    }))?
    .warnings(lib.warnings.clone()))
}

fn find(pattern: &str, paths: &[PathBuf], pads: bool, ctx: &Ctx) -> Result<Out> {
    use altium_format::DocKind;
    let mut files = std::collections::BTreeSet::new();
    let mut skipped = vec![];
    let mut warnings = vec![];
    let mut queue = paths.to_vec();
    let mut visited = std::collections::BTreeSet::new();
    while let Some(path) = queue.pop() {
        let canonical = std::fs::canonicalize(&path).map_err(|e| Error::from(e)
            .hint("pass existing .PcbLib files or library directories"))?;
        if !visited.insert(canonical.clone()) { continue; }
        if canonical.is_dir() {
            for entry in std::fs::read_dir(&canonical)?.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') || name == "__MACOSX" || entry.file_type()?.is_symlink() { continue; }
                let path = entry.path();
                if path.is_dir() || matches!(DocKind::of(&path), DocKind::PcbLib | DocKind::IntLib) { queue.push(path); }
            }
        } else if DocKind::of(&canonical) == DocKind::PcbLib { files.insert(canonical); }
        else if DocKind::of(&canonical) == DocKind::IntLib {
            warnings.push(format!("{}: IntLib has no dedicated native decoder; inspect with ace raw streams; extract source libraries in Altium", canonical.display()));
            skipped.push(json!({"file":canonical,"reason":"intlib_not_decoded"}));
        } else { return Err(Error::new(ErrorCode::WrongFileType, format!("{} is not a .PcbLib", path.display()))
            .hint("pass .PcbLib files/directories; inspect .IntLib with `ace raw streams`")); }
    }
    let pattern = pattern.to_lowercase();
    let mut items = vec![];
    let mut libraries_scanned = 0;
    let mut total_footprints = 0;
    for file in files {
        let lib = match open(&file, ctx) {
            Ok(lib) => lib,
            Err(e) => {
                warnings.push(format!("{}: {}", file.display(), e.message));
                skipped.push(json!({"file":file,"reason":e.code,"hint":e.hint}));
                continue;
            }
        };
        libraries_scanned += 1;
        total_footprints += lib.footprints.len();
        warnings.extend(lib.warnings.iter().cloned());
        for footprint in &lib.footprints {
            if !footprint.name.to_lowercase().contains(&pattern) { continue; }
            let mut row = serde_json::to_value(footprint.summary(&lib.models))?;
            row["file"] = json!(file);
            let numbers: std::collections::BTreeSet<_> = footprint.pads.iter().map(|pad| pad.number.as_str()).collect();
            row["pad_number_values"] = json!(numbers);
            if pads { row["pad_items"] = json!(footprint.pads); }
            items.push(row);
        }
    }
    if items.is_empty() { warnings.push("no matching decoded footprints; inspect libraries_scanned, skipped and incomplete before concluding the footprint is absent".into()); }
    Out::new(list_with(&items, json!({"libraries_scanned":libraries_scanned,"total_footprints":total_footprints,"incomplete":!skipped.is_empty(),"skipped":skipped}))?).map(|o| o.warnings(warnings))
}
