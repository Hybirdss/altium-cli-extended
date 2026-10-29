//! `ace project ...` — project files (.PrjPcb).

use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde_json::{Value, json};

use altium_doc::project::{Project, VariationKind, resolve_ci};
use crate::ctx::Ctx;
use altium_core::error::{Error, ErrorCode, Result};
use altium_format::DocKind;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list};

#[derive(Subcommand, Debug)]
pub enum ProjectCmd {
    /// Summary: documents by kind, variants, parameters, key compile options.
    Info { project: PathBuf },
    /// Documents in the project with resolved paths and whether they exist.
    Documents {
        project: PathBuf,
        /// Only this kind (schematic, pcb, schlib, pcblib, outjob...).
        #[arg(long)]
        kind: Option<String>,
        /// Include generated (output) documents.
        #[arg(long)]
        generated: bool,
    },
    /// Project-level parameters (usable as =Name special strings on sheets/PCB).
    Parameters { project: PathBuf },
    /// Assembly variants with their per-designator variations.
    Variants { project: PathBuf },
    /// One variant in full.
    Variant { project: PathBuf, name: String },
    /// Output groups stored in the project itself (netlists, reports, etc.).
    Outputs { project: PathBuf },
    /// Raw INI sections (every setting Altium stores: ERC matrix, comparator, annotate...).
    Settings {
        project: PathBuf,
        /// Only this section (e.g. `Design`, `Electrical Rules Check`).
        #[arg(long)]
        section: Option<String>,
        /// Only list section names.
        #[arg(long)]
        names: bool,
    },
    /// Set (or add) a project parameter.
    SetParameter { project: PathBuf, name: String, value: String },
    /// Remove a project parameter.
    RemoveParameter { project: PathBuf, name: String },
    /// Set any raw project setting `[section] key=value` (escape hatch).
    SetOption { project: PathBuf, section: String, key: String, value: String },
    /// Add an existing document to the project.
    AddDocument { project: PathBuf, document: PathBuf },
    /// Remove a document from the project (the file itself is kept).
    RemoveDocument { project: PathBuf, document: String },
    /// Add an empty assembly variant.
    AddVariant {
        project: PathBuf,
        name: String,
        /// Allow fabrication outputs for this variant.
        #[arg(long)]
        allow_fabrication: bool,
    },
    /// Remove an assembly variant.
    RemoveVariant { project: PathBuf, variant: String },
    /// Set how a variant treats one part: not fitted (default), fitted, or an alternate part.
    SetVariation {
        project: PathBuf,
        variant: String,
        designator: String,
        /// Fitted with parameter changes only (instead of not fitted).
        #[arg(long, conflicts_with = "alternate")]
        fitted: bool,
        /// Replace the part with this library reference.
        #[arg(long)]
        alternate: Option<String>,
        /// Unique-id path of the part (`\SHEETID\COMPID`); default: taken from the PCB or schematic.
        #[arg(long)]
        unique_id: Option<String>,
    },
    /// Set a parameter value of one part in a variant (e.g. Comment=22k).
    SetVariantParam { project: PathBuf, variant: String, designator: String, name: String, value: String },
    /// Remove every variation (fitting and parameters) of one part from a variant.
    ClearVariation { project: PathBuf, variant: String, designator: String },
    /// Find .PrjPcb projects under a directory.
    Find {
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Maximum directory depth.
        #[arg(long, default_value_t = 6)]
        depth: usize,
        /// Include macOS resource forks and hidden directories.
        #[arg(long)]
        include_hidden: bool,
    },
}

pub const META: &[CmdMeta] = &[
    m("project info", Native, Read,
      "{name, path, documents:{schematic:N,...}, missing_documents:[...], variants:[names], parameters:N, options:{...}}",
      "ace project info Board.PrjPcb", ""),
    m("project documents", Native, Read, "{count, items:[{index, rel, path, kind, exists, unique_id, generated?}]}",
      "ace project documents Board.PrjPcb --kind schematic",
      "Paths are resolved case-insensitively; exists=false means the project references a missing file."),
    m("project parameters", Native, Read, "{count, items:[{name, value}]}", "ace project parameters Board.PrjPcb", ""),
    m("project variants", Native, Read, "{count, items:[{index, name, unique_id, allow_fabrication, variations:[{designator, kind, alternate_part}], parameters, param_variations}]}",
      "ace project variants Board.PrjPcb", "kind: not_fitted | fitted (parameter-only) | alternate."),
    m("project variant", Native, Read, "variant object", "ace project variant Board.PrjPcb \"No LCD\"", "Name, unique id or 1-based index."),
    m("project outputs", Native, Read, "{count, items:[{group, index, type, name, document, variant, default}]}",
      "ace project outputs Board.PrjPcb", "Fabrication outputs usually live in .OutJob files instead: `ace outjob outputs`."),
    m("project settings", Native, Read, "{count, items:[{section, entries:{...}}]}",
      "ace project settings Board.PrjPcb --section Design", ""),
    m("project set-parameter", Native, Write, "{changed, before, after, backup}", "ace project set-parameter Board.PrjPcb Revision B", ""),
    m("project remove-parameter", Native, Write, "{changed, backup}", "ace project remove-parameter Board.PrjPcb Revision", ""),
    m("project set-option", Native, Write, "{changed, before, after, backup}",
      "ace project set-option Board.PrjPcb Design OutputPath Outputs", "Raw edit; prefer a dedicated command when one exists."),
    m("project add-document", Native, Write, "{added:{index, rel}, backup}", "ace project add-document Board.PrjPcb Power.SchDoc", ""),
    m("project remove-document", Native, Write, "{removed:{index, rel}, backup}", "ace project remove-document Board.PrjPcb Old.SchDoc", "Matches by file name or relative path."),
    m("project add-variant", Native, Write, "{changed, variant:{index, name, unique_id, unique_id_note?}, backup, verified, verification:{candidate_reparsed,written,read_back}}",
      "ace project add-variant Board.PrjPcb Lite", "Then mark parts with `project set-variation` / `project set-variant-param`; `ace bom --variant` applies it. A dry-run returns unique_id:null with unique_id_note because IDs are assigned on apply; verification separates candidate reparse from disk read-back."),
    m("project remove-variant", Native, Write, "{changed, removed, backup, verified}", "ace project remove-variant Board.PrjPcb Lite", ""),
    m("project set-variation", Native, Write, "{changed, variant, designator, before:variation|null, after:variation, raw:{before,after}, unique_id_source, backup, verified, verification:{candidate_reparsed,written,read_back}}",
      "ace project set-variation Board.PrjPcb Lite R12",
      "Default not fitted; --fitted or --alternate <LibRef>. The unique-id path comes from the PCB component link (SOURCEUNIQUEID) or the schematic part."),
    m("project set-variant-param", Native, Write, "{changed, variant, designator, name, before, after, backup, verified}",
      "ace project set-variant-param Board.PrjPcb Lite R1 Comment 22k", ""),
    m("project clear-variation", Native, Write, "{changed, removed, backup, verified}", "ace project clear-variation Board.PrjPcb Lite R12", "removed counts both component variations and parameter variations."),
    m("project find", Native, Read, "{count, items:[{path, documents}]}", "ace project find ~/designs", ""),
];

pub fn run(c: ProjectCmd, ctx: &Ctx) -> Result<Out> {
    match c {
        ProjectCmd::Info { project } => info(&project, ctx),
        ProjectCmd::Documents { project, kind, generated } => {
            let p = Project::load(&ctx.input(&project)?)?;
            let want = kind.map(|k| k.to_ascii_lowercase());
            let items: Vec<_> = p
                .documents
                .iter()
                .filter(|d| generated || !d.generated)
                .filter(|d| want.as_deref().is_none_or(|k| json!(d.kind).as_str() == Some(k)))
                .collect();
            let missing = items.iter().filter(|d| !d.exists).count();
            let mut out = Out::new(list(&items)?)?;
            if missing > 0 {
                out = out.warn(format!("{missing} referenced document(s) not found on disk"));
            }
            Ok(out)
        }
        ProjectCmd::Parameters { project } => {
            let p = Project::load(&ctx.input(&project)?)?;
            let items: Vec<Value> = p.parameters().into_iter().map(|(k, v)| json!({"name": k, "value": v})).collect();
            Out::new(list(&items)?)
        }
        ProjectCmd::Variants { project } => {
            let p = Project::load(&ctx.input(&project)?)?;
            Out::new(list(&p.variants())?)
        }
        ProjectCmd::Variant { project, name } => {
            let p = Project::load(&ctx.input(&project)?)?;
            Out::new(p.variant(&name)?)
        }
        ProjectCmd::Outputs { project } => {
            let p = Project::load(&ctx.input(&project)?)?;
            let mut items = vec![];
            for (_, s) in p.ini.numbered("OutputGroup") {
                for i in 1.. {
                    let Some(t) = s.get(&format!("OutputType{i}")) else { break };
                    items.push(json!({
                        "group": s.str("Name"),
                        "index": i,
                        "type": t,
                        "name": s.str(&format!("OutputName{i}")),
                        "document": s.str(&format!("OutputDocumentPath{i}")),
                        "variant": s.str(&format!("OutputVariantName{i}")),
                        "default": s.bool(&format!("OutputDefault{i}")),
                    }));
                }
            }
            Out::new(list(&items)?)
        }
        ProjectCmd::Settings { project, section, names } => {
            let p = Project::load(&ctx.input(&project)?)?;
            if names {
                let n: Vec<&str> = p.ini.sections.iter().map(|s| s.name.as_str()).collect();
                return Out::new(list(&n)?);
            }
            let items: Vec<Value> = p
                .ini
                .sections
                .iter()
                .filter(|s| section.as_deref().is_none_or(|w| s.name.eq_ignore_ascii_case(w)))
                .map(|s| json!({"section": s.name, "entries": s.to_json()}))
                .collect();
            if items.is_empty() {
                return Err(Error::not_found(format!("no section {:?}", section.unwrap_or_default()))
                    .hint("list them with `ace project settings <prj> --names`"));
            }
            Out::new(list(&items)?)
        }
        ProjectCmd::SetParameter { project, name, value } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            let existing = p
                .ini
                .numbered("Parameter")
                .into_iter()
                .find(|(_, s)| s.str("Name").eq_ignore_ascii_case(&name))
                .map(|(n, s)| (n, s.str("Value")));
            let before = existing.as_ref().map(|(_, v)| v.clone());
            match existing {
                Some((n, _)) => p.ini.section_mut(&format!("Parameter{n}")).unwrap().set("Value", &value),
                None => {
                    let n = p.ini.numbered("Parameter").last().map(|(n, _)| n + 1).unwrap_or(1);
                    let mut s = altium_format::ini::Section { name: format!("Parameter{n}"), entries: vec![] };
                    s.set("Name", &name);
                    s.set("Value", &value);
                    insert_after_numbered(&mut p.ini, "Parameter", s);
                }
            }
            let changed = before.as_deref() != Some(value.as_str());
            let backup = if changed { save(ctx, &path, &p.ini)? } else { None };
            Out::new(json!({"changed": changed, "name": name, "before": before, "after": value, "backup": backup}))
        }
        ProjectCmd::RemoveParameter { project, name } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            let Some((n, _)) =
                p.ini.numbered("Parameter").into_iter().find(|(_, s)| s.str("Name").eq_ignore_ascii_case(&name))
            else {
                return Err(Error::not_found(format!("no project parameter {name:?}")));
            };
            p.ini.sections.retain(|s| s.name != format!("Parameter{n}"));
            renumber(&mut p.ini, "Parameter");
            let backup = save(ctx, &path, &p.ini)?;
            Out::new(json!({"changed": true, "name": name, "backup": backup}))
        }
        ProjectCmd::SetOption { project, section, key, value } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            if p.ini.section(&section).is_none() {
                p.ini.sections.push(altium_format::ini::Section { name: section.clone(), entries: vec![] });
            }
            let s = p.ini.section_mut(&section).unwrap();
            let before = s.get(&key).map(str::to_owned);
            s.set(&key, &value);
            let changed = before.as_deref() != Some(value.as_str());
            let backup = if changed { save(ctx, &path, &p.ini)? } else { None };
            Out::new(json!({"changed": changed, "section": section, "key": key, "before": before, "after": value, "backup": backup}))
        }
        ProjectCmd::AddDocument { project, document } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            let rel = relative_windows(&p.dir, &document);
            if p.documents.iter().any(|d| !d.generated && d.rel.eq_ignore_ascii_case(&rel)) {
                return Err(Error::new(altium_core::error::ErrorCode::Usage, format!("{rel} is already in the project")));
            }
            if !resolve_ci(&p.dir, &rel).exists() {
                return Err(Error::new(altium_core::error::ErrorCode::FileNotFound, format!("{} does not exist", document.display()))
                    .hint("create the document first (in Altium, or copy a template)"));
            }
            let n = p.ini.numbered("Document").last().map(|(n, _)| n + 1).unwrap_or(1);
            let mut s = altium_format::ini::Section { name: format!("Document{n}"), entries: vec![] };
            s.set("DocumentPath", &rel);
            s.set("AnnotationEnabled", "1");
            s.set("DoLibraryUpdate", "1");
            s.set("DoDatabaseUpdate", "1");
            s.set("DocumentUniqueId", "");
            insert_after_numbered(&mut p.ini, "Document", s);
            let backup = save(ctx, &path, &p.ini)?;
            Out::new(json!({"added": {"index": n, "rel": rel}, "backup": backup}))
        }
        ProjectCmd::RemoveDocument { project, document } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            let want = document.replace('/', "\\").to_lowercase();
            let hit: Vec<_> = p
                .documents
                .iter()
                .filter(|d| !d.generated)
                .filter(|d| {
                    let r = d.rel.to_lowercase();
                    r == want || r.rsplit('\\').next() == Some(want.as_str())
                })
                .map(|d| (d.index, d.rel.clone()))
                .collect();
            match hit.as_slice() {
                [] => Err(Error::not_found(format!("{document} is not in the project"))
                    .hint("see `ace project documents`")),
                [(n, rel)] => {
                    p.ini.sections.retain(|s| s.name != format!("Document{n}"));
                    renumber(&mut p.ini, "Document");
                    let backup = save(ctx, &path, &p.ini)?;
                    Out::new(json!({"removed": {"index": n, "rel": rel}, "backup": backup}))
                }
                many => Err(Error::new(altium_core::error::ErrorCode::Ambiguous, format!("{document} matches several documents"))
                    .details(json!(many.iter().map(|(_, r)| r).collect::<Vec<_>>()))
                    .hint("pass the relative path exactly as `project documents` shows it")),
            }
        }
        ProjectCmd::AddVariant { project, name, allow_fabrication } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            let used: Vec<String> = p.variants().iter().map(|v| v.unique_id.clone()).collect();
            let uid = new_uid(&name, &used);
            let n = p.add_variant(&name, &uid, allow_fabrication)?;
            let backup = save(ctx, &path, &p.ini)?;
            Out::new(json!({"changed": true, "variant": {"index": n, "name": name,
                "unique_id": if ctx.dry_run { None } else { Some(uid) },
                "unique_id_note": ctx.dry_run.then_some("assigned on apply; preview IDs are not reserved")},
                "backup": backup, "verified": true,
                "verification":{"candidate_reparsed":true,"written":!ctx.dry_run,"read_back":!ctx.dry_run}}))
        }
        ProjectCmd::RemoveVariant { project, variant } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            let removed = p.remove_variant(&variant)?;
            let backup = save(ctx, &path, &p.ini)?;
            Out::new(json!({"changed": true, "removed": removed, "backup": backup, "verified": true}))
        }
        ProjectCmd::SetVariation { project, variant, designator, fitted, alternate, unique_id } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            let before_typed = p.variant(&variant)?.variations.into_iter().find(|v| v.designator.eq_ignore_ascii_case(&designator));
            let (uid, source) = match unique_id {
                Some(u) => (u, "argument"),
                None => part_uid(&p, &designator)?,
            };
            let kind = match alternate {
                Some(a) => VariationKind::Alternate(a),
                None if fitted => VariationKind::Fitted,
                None => VariationKind::NotFitted,
            };
            let (before, after) = p.set_variation(&variant, &designator, &uid, &kind)?;
            let changed = before.as_deref() != Some(after.as_str());
            let backup = if changed { save(ctx, &path, &p.ini)? } else { None };
            let after_typed = p.variant(&variant)?.variations.into_iter().find(|v| v.designator.eq_ignore_ascii_case(&designator));
            Out::new(json!({"changed": changed, "variant": variant, "designator": designator,
                            "before": before_typed, "after": after_typed, "raw":{"before":before,"after":after},
                            "unique_id_source": source, "backup": backup, "verified": true,
                            "verification":{"candidate_reparsed":changed,"written":changed && !ctx.dry_run,"read_back":changed && !ctx.dry_run}}))
        }
        ProjectCmd::SetVariantParam { project, variant, designator, name, value } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            let before = p.set_param_variation(&variant, &designator, &name, &value)?;
            let changed = before.as_deref() != Some(value.as_str());
            let backup = if changed { save(ctx, &path, &p.ini)? } else { None };
            Out::new(json!({"changed": changed, "variant": variant, "designator": designator, "name": name, "before": before, "after": value, "backup": backup, "verified": true}))
        }
        ProjectCmd::ClearVariation { project, variant, designator } => {
            let path = ctx.input(&project)?;
            let mut p = Project::load(&path)?;
            let removed = p.clear_variation(&variant, &designator)?;
            let backup = if removed > 0 { save(ctx, &path, &p.ini)? } else { None };
            Out::new(json!({"changed": removed > 0, "removed": removed, "backup": backup, "verified": true}))
        }
        ProjectCmd::Find { dir, depth, include_hidden } => {
            let mut found = vec![];
            if !dir.is_dir() { return Err(Error::new(ErrorCode::FileNotFound, format!("{} is not a directory", dir.display())).hint("pass a directory to `ace project find`")); }
            find_projects(&dir, depth, include_hidden, &mut found);
            found.sort();
            let items: Vec<Value> = found
                .iter()
                .map(|p| {
                    let docs = Project::load(p).map(|pr| pr.documents.iter().filter(|d| !d.generated).count()).ok();
                    json!({"path": p, "documents": docs, "error": docs.is_none().then_some("not a project")})
                })
                .collect();
            Out::new(list(&items)?)
        }
    }
}

/// Write the edited project: the new text must re-parse to exactly the edited sections
/// (and still load as a project) before it replaces the file.
fn save(ctx: &Ctx, path: &Path, ini: &altium_format::ini::Ini) -> Result<Option<PathBuf>> {
    let bytes = ini.to_bytes();
    if &altium_format::ini::Ini::parse(&bytes) != ini {
        return Err(Error::new(ErrorCode::Unsafe, "the edited project does not read back identically; nothing written")
            .hint("a value probably contains a line break or a character the code page cannot store"));
    }
    Project::from_bytes(path, &bytes).map_err(|e| Error::new(ErrorCode::Unsafe, format!("the edited project does not load: {}", e.message)))?;
    let backup = ctx.write_file(path, &bytes)?;
    if !ctx.dry_run && std::fs::read(path)? != bytes {
        return Err(Error::new(ErrorCode::Unsafe, "project on disk differs from the verified candidate")
            .hint("inspect concurrent writers; keep the first .bak and restore if necessary"));
    }
    Ok(backup)
}

/// A fresh 8-letter unique id not in `used`.
fn new_uid(seed: &str, used: &[String]) -> String {
    let mut x: u64 = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7) | 1;
    for b in seed.bytes() {
        x = x.wrapping_mul(0x100_0000_01B3) ^ b as u64;
    }
    loop {
        let mut s = String::new();
        for _ in 0..8 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            s.push((b'A' + (x % 26) as u8) as char);
        }
        if !used.contains(&s) {
            return s;
        }
    }
}

/// Unique-id path Altium uses in variations: the PCB component's SOURCEUNIQUEID when a PCB
/// has the part, else `\<schematic unique id>`.
fn part_uid(p: &Project, designator: &str) -> Result<(String, &'static str)> {
    for f in p.pcbs() {
        let Ok(doc) = altium_doc::pcb::load_with(&f, &altium_doc::pcb::LoadOptions::parts(0)) else { continue };
        if let Some(c) = doc.components.iter().find(|c| {
            (c.source_designator.eq_ignore_ascii_case(designator) || c.designator.eq_ignore_ascii_case(designator)) && !c.source_unique_id.is_empty()
        }) {
            return Ok((c.source_unique_id.clone(), "pcb"));
        }
    }
    if !p.schematics().is_empty() {
        let d = altium_doc::sch::project_design(p)?;
        if let Some(part) = d.parts.iter().find(|x| x.designator.eq_ignore_ascii_case(designator)) {
            if part.unique_id.is_empty() {
                return Err(Error::new(ErrorCode::Unsafe, format!("{designator} has no unique id")).hint("pass --unique-id"));
            }
            return Ok((format!("\\{}", part.unique_id), "schematic"));
        }
    }
    Err(Error::not_found(format!("no part {designator} in the project's PCB or schematics"))
        .hint("check `ace bom <project> --group-by designator`, or pass --unique-id"))
}

fn find_projects(dir: &Path, depth: usize, include_hidden: bool, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !include_hidden && (name.starts_with('.') || name == "__MACOSX") { continue; }
        if p.is_dir() {
            if depth > 0 && name != "History" && name != "__Previews" {
                find_projects(&p, depth - 1, include_hidden, out);
            }
        } else if DocKind::of(&p) == DocKind::Project {
            out.push(p);
        }
    }
}

/// Path of `doc` relative to `dir`, with Windows separators, as Altium writes it.
fn relative_windows(dir: &Path, doc: &Path) -> String {
    let abs = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (d, f) = (abs(dir), abs(doc));
    let rel = match f.strip_prefix(&d) {
        Ok(r) => r.to_path_buf(),
        Err(_) => {
            // Walk up until a common ancestor.
            let mut up = PathBuf::new();
            let mut base = d.clone();
            loop {
                if let Ok(r) = f.strip_prefix(&base) {
                    break up.join(r);
                }
                if !base.pop() {
                    break f.clone();
                }
                up.push("..");
            }
        }
    };
    rel.to_string_lossy().replace('/', "\\")
}

fn insert_after_numbered(ini: &mut altium_format::ini::Ini, prefix: &str, s: altium_format::ini::Section) {
    let pos = ini
        .sections
        .iter()
        .rposition(|x| x.name.starts_with(prefix) && x.name[prefix.len()..].parse::<u32>().is_ok());
    match pos {
        Some(i) => ini.sections.insert(i + 1, s),
        None => ini.sections.push(s),
    }
}

/// Renumber `<prefix>N` sections to 1..n after a removal.
fn renumber(ini: &mut altium_format::ini::Ini, prefix: &str) {
    let mut n = 0;
    for s in ini.sections.iter_mut() {
        if s.name.starts_with(prefix) && s.name[prefix.len()..].parse::<u32>().is_ok() {
            n += 1;
            s.name = format!("{prefix}{n}");
        }
    }
}

pub fn info(file: &Path, ctx: &Ctx) -> Result<Out> {
    let p = Project::load(&ctx.input(file)?)?;
    let mut by_kind = serde_json::Map::new();
    for d in p.documents.iter().filter(|d| !d.generated) {
        let k = json!(d.kind).as_str().unwrap_or("other").to_owned();
        let e = by_kind.entry(k).or_insert(json!(0));
        *e = json!(e.as_u64().unwrap_or(0) + 1);
    }
    let missing: Vec<&str> = p.documents.iter().filter(|d| !d.exists && !d.generated).map(|d| d.rel.as_str()).collect();
    let opt = |k: &str| p.design(k);
    let mut out = Out::new(json!({
        "name": p.name(),
        "path": p.path,
        "documents": by_kind,
        "missing_documents": missing,
        "generated_documents": p.documents.iter().filter(|d| d.generated).count(),
        "variants": p.variants().iter().map(|v| v.name.clone()).collect::<Vec<_>>(),
        "parameters": p.parameters().len(),
        "options": {
            "hierarchy_mode": opt("HierarchyMode"),
            "allow_port_net_names": opt("AllowPortNetNames").map(|v| v == "1"),
            "allow_sheet_entry_net_names": opt("AllowSheetEntryNetNames").map(|v| v == "1"),
            "append_sheet_number_to_local_nets": opt("AppendSheetNumberToLocalNets").map(|v| v == "1"),
            "power_port_names_take_priority": opt("PowerPortNamesTakePriority").map(|v| v == "1"),
            "name_nets_hierarchically": opt("NameNetsHierarchically").map(|v| v == "1"),
            "netlist_single_pin_nets": opt("NetlistSinglePinNets").map(|v| v == "1"),
            "output_path": opt("OutputPath"),
        },
    }))?;
    if !missing.is_empty() {
        out = out.warn(format!("{} referenced document(s) missing on disk", missing.len()));
    }
    Ok(out)
}
