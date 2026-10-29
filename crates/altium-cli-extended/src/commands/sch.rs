//! `ace sch ...` — schematic sheets (.SchDoc): objects, connectivity, hierarchy, edits.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde_json::{Value, json};

use altium_core::error::{Error, Result};
use altium_doc::project::Project;
use altium_doc::sch::{self, SchDoc, connect, edit, project as sproj, records as rt};

use crate::ctx::Ctx;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list, list_with};

#[derive(Subcommand, Debug)]
pub enum SchCmd {
    /// Summary: sheet size/style, document parameters, record counts, components, warnings.
    Info { file: PathBuf },
    /// Placed components (one row per sub-part instance) with footprint and parameters.
    Components { file: PathBuf },
    /// One component in full: pins with nets, every model (footprint/sim/SI/3D), parameters.
    Component {
        file: PathBuf,
        /// Designator (`U1` = every sub-part, `U1A` = one sub-part).
        designator: String,
        /// Name pin nets project-wide using this .PrjPcb (default: sheet scope).
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Pins with electrical type, hot-end location and net.
    Pins {
        file: PathBuf,
        /// Only pins of this component.
        #[arg(long)]
        component: Option<String>,
        /// Name pin nets project-wide using this .PrjPcb (default: sheet scope).
        #[arg(long)]
        project: Option<PathBuf>,
        /// Only pins without a net in this scope (native approximation, not ERC).
        #[arg(long)]
        unconnected: bool,
    },
    /// Sheet-local nets (this sheet only; use `sch project-nets` across a project).
    Nets { file: PathBuf },
    /// Project-wide nets across all schematics, following the project's net identifier scope.
    ProjectNets { project: PathBuf },
    /// Net labels.
    NetLabels { file: PathBuf },
    /// Power ports (net name and symbol style).
    PowerPorts { file: PathBuf },
    /// Ports (name, IO type, harness type, net).
    Ports { file: PathBuf },
    /// Sheet symbols (child file, sheet name) with their entries.
    SheetSymbols { file: PathBuf },
    /// Wires with their vertices and net.
    Wires { file: PathBuf },
    /// Buses with their labels and bus entries.
    Buses { file: PathBuf },
    /// Junctions.
    Junctions { file: PathBuf },
    /// No-ERC markers.
    NoErc { file: PathBuf },
    /// Free text: labels, notes, text frames, hyperlinks.
    Texts { file: PathBuf },
    /// Document parameters (title block fields).
    Parameters { file: PathBuf },
    /// Directives: parameter sets (net class / diff pair / rules), no-ERC, compile masks, blankets.
    Directives { file: PathBuf },
    /// Images, with the embedded files from the Storage stream.
    Images {
        file: PathBuf,
        /// Write the embedded files into this directory.
        #[arg(long)]
        extract: Option<PathBuf>,
    },
    /// Every record with its type name, owner and raw properties.
    Records {
        file: PathBuf,
        /// Only this record type (name such as `wire` or number such as `27`).
        #[arg(long = "type")]
        record_type: Option<String>,
    },
    /// Search every text property (case-insensitive substring).
    Find {
        file: PathBuf,
        /// Text to look for.
        #[arg(value_name = "TEXT")]
        needle: String,
    },
    /// Locate components across all project sheets; use each sheet/index in follow-up edits.
    Locate {
        /// Project containing the schematic sheets.
        project: PathBuf,
        /// Component designators, comma-separated or separate arguments.
        #[arg(required = true, num_args = 1.., value_delimiter = ',')]
        designators: Vec<String>,
    },
    /// Sheet tree of a project from its sheet symbols (roots, repeated sheets, scope).
    Hierarchy { project: PathBuf },
    /// Set (or create) a component parameter.
    SetParam { file: PathBuf, designator: String, name: String, value: String,
        /// Explicitly replace a special-string reference such as =Value with literal text.
        #[arg(long)]
        literal: bool,
    },
    /// Remove a component parameter.
    RemoveParam { file: PathBuf, designator: String, name: String },
    /// Set the Comment parameter.
    SetComment { file: PathBuf, designator: String, value: String,
        /// Explicitly replace a special-string reference such as =Value with literal text.
        #[arg(long)]
        literal: bool,
    },
    /// Rename the current footprint model.
    SetFootprint { file: PathBuf, designator: String, footprint: String },
    /// Change a designator.
    SetDesignator { file: PathBuf, designator: String, new: String },
    /// Set (or create) a document parameter, e.g. `Revision`.
    SetSheetParam { file: PathBuf, name: String, value: String },
}

pub const META: &[CmdMeta] = &[
    m("sch info", Native, Read,
      "{file, header, sheet:{size,width,height,orientation,fonts...}, parameters:{name:value}, record_counts:{type:n}, components, nets, sheet_symbols, ports, power_ports, net_labels, storage_files, warnings_count}",
      "ace sch info Power.SchDoc", "Start here for a sheet. `parameters` are the title-block fields."),
    m("sch components", Native, Read,
      "{count, items:[{index, designator, part?, display_designator, lib_ref, comment, footprint?, library?, description, kind, parameters:{name:value}, location, orientation, mirrored, unique_id, pins}]}",
      "ace sch components Power.SchDoc --where footprint~0603",
      "One row per placed sub-part (U1A, U1B share designator U1). `index` is the component record."),
    m("sch component", Native, Read,
      "{count, items:[component with pins:[{designator,name,electrical,hot_end,net...}], models:[{kind,name,current,datafiles,pin_map}], parameters:[{name,value,resolved?,hidden}]}]}",
      "ace sch component Power.SchDoc U3 --project Board.PrjPcb", "`U1` returns every sub-part instance, `U1A` only that one. Pin nets are sheet-scope names unless --project is given (then project-wide names; pins on no net have none)."),
    m("sch pins", Native, Read,
      "{count, items:[{component, index, pin, designator, connected, name, electrical, hot_end, location, orientation, length, hidden, net}]}",
      "ace sch pins Power.SchDoc --component U3 --project Board.PrjPcb", "net = sheet-scope name, or the project-wide name with --project. Native approximation of Altium connectivity (rules in altium-doc sch/connect.rs); `ace designer` compile is the authority. pin is the pin number (designator is a deprecated alias); net is always present (null if unconnected), connected is boolean. --unconnected filters within the selected scope."),
    m("sch nets", Native, Read,
      "{count, single_node, dangling_wires, items:[{name, auto_named?, name_source, nodes:[{designator,pin,pin_name,pin_record}], net_labels, power_ports, ports, sheet_entries, other_names, wires}]}",
      "ace sch nets Power.SchDoc --where name=GND",
      "Sheet scope with Altium default naming options. Cross-sheet joins: `sch project-nets`. Native approximation; `ace designer` compile is the authority."),
    m("sch project-nets", Native, Read,
      "{scope, configured_scope, count, single_node, items:[{name, auto_named?, name_source, nodes:[{designator,pin,pin_name}], sheets, other_names}]}",
      "ace sch project-nets Board.PrjPcb --where name~^Net",
      "Follows [Design] HierarchyMode and naming options. Node pins are footprint pad numbers (pin maps applied), pin_name the schematic pin name. Unnamed single-pin nets are left out unless NetlistSinglePinNets. Matches the sample PCBs' net names; still a native approximation, `ace designer` compile is the authority."),
    m("sch net-labels", Native, Read, "{count, items:[{index, name, location, orientation, on_bus?, net}]}",
      "ace sch net-labels Power.SchDoc", "on_bus=true: the label names a bus, not a net."),
    m("sch power-ports", Native, Read, "{count, items:[{index, name, style, show_net_name, location, orientation, net}]}",
      "ace sch power-ports Power.SchDoc", ""),
    m("sch ports", Native, Read, "{count, items:[{index, name, io_type, style, location, width, ends, harness_type?, net}]}",
      "ace sch ports Sensor.SchDoc", ""),
    m("sch sheet-symbols", Native, Read,
      "{count, items:[{index, sheet_name, file_name, location, width, height, repeat?, entries:[{index, name, io_type, side, location, net}]}]}",
      "ace sch sheet-symbols Top.SchDoc", "file_name is the child sheet; `sch hierarchy` resolves the tree."),
    m("sch wires", Native, Read, "{count, items:[{index, points, net}]}", "ace sch wires Power.SchDoc", ""),
    m("sch buses", Native, Read, "{count, entries:[...], items:[{index, points, labels, entries}]}", "ace sch buses Top.SchDoc",
      "Bus names like D[0..7] are expanded to member nets D0..D7 across sheets."),
    m("sch junctions", Native, Read, "{count, items:[{index, location, net}]}", "ace sch junctions Power.SchDoc", ""),
    m("sch no-erc", Native, Read, "{count, items:[{index, location, active, suppress_all, net?}]}", "ace sch no-erc Power.SchDoc", ""),
    m("sch texts", Native, Read, "{count, items:[{index, kind, text, location, owner?}]}", "ace sch texts Power.SchDoc --where kind=note",
      "kind: label | text_frame | note | hyperlink. owner = component designator for symbol text."),
    m("sch parameters", Native, Read, "{count, items:[{index, name, value, resolved?, hidden}]}", "ace sch parameters Power.SchDoc",
      "value `*` = not set. Change with `sch set-sheet-param`."),
    m("sch directives", Native, Read,
      "{count, items:[{index, kind, name?, location?, parameters:{...}, net?}]}",
      "ace sch directives Power.SchDoc", "kind: parameter_set | no_erc | compile_mask | blanket."),
    m("sch images", Native, Export,
      "{count, storage:[{index, name, format, size}], extracted?:[paths], items:[{index, file_name, embedded, storage_name?}]}",
      "ace sch images Logo.SchDoc --extract ./img", "Writes files only with --extract (honours --dry-run)."),
    m("sch records", Native, Read,
      "{count, items:[{index, stream, record, type, owner?, owner_designator?, props}]}",
      "ace sch records Power.SchDoc --type wire", "Escape hatch: every record, raw. --type accepts a name or number."),
    m("sch find", Native, Read, "{count, items:[{index, type, owner?, key, value, stream}]}",
      "ace sch find Power.SchDoc 100nF", ""),
    m("sch locate", Native, Read, "{count, items:[{designator,sheet,index,unique_id,comment,footprint}]}",
      "ace sch locate Board.PrjPcb R20,BF1", "Searches every existing schematic; warnings identify missing project files. Use the returned sheet path with sch set-param. Multiple rows can represent sub-parts or repeated designators."),
    m("sch hierarchy", Native, Read,
      "{configured_scope, scope, roots:[{file, sheet_name?, exists, in_project, repeated?, repeat?, components, children:[...]}], repeated_sheets, warnings}",
      "ace sch hierarchy Board.PrjPcb", "Roots = project sheets no sheet symbol places."),
    m("sch set-param", Native, Write, "{changed, changes:[{index, action, designator, key, before, after}], backup, verified}",
      "ace sch set-param Power.SchDoc R5 MPN RC0603FR-0710KL",
      "Creates a hidden parameter (copying a sibling's style) when missing. Applies to every sub-part instance. Replacing a =Param reference needs --literal; --dry-run warns and marks special_string."),
    m("sch remove-param", Native, Write, "{changed, changes, renumbered, backup, verified}",
      "ace sch remove-param Power.SchDoc R5 MPN", "Later records' OWNERINDEX are renumbered (reported)."),
    m("sch set-comment", Native, Write, "{changed, changes, backup, verified}", "ace sch set-comment Power.SchDoc R5 10k", "Special strings such as =Value are protected: set the Value parameter instead, or explicitly use --literal. Dry runs report special_string without writing."),
    m("sch set-footprint", Native, Write, "{changed, changes, backup, verified}",
      "ace sch set-footprint Power.SchDoc R5 0603", "Renames the current footprint model; refuses (UNSAFE) when there is none."),
    m("sch set-designator", Native, Write, "{changed, changes, backup, verified}",
      "ace sch set-designator Power.SchDoc R5 R50", "Refuses when the new designator is already used on the sheet."),
    m("sch set-sheet-param", Native, Write, "{changed, changes, backup, verified}",
      "ace sch set-sheet-param Power.SchDoc Revision B", ""),
];

fn load(file: &Path, ctx: &Ctx) -> Result<SchDoc> {
    sch::load_with(&ctx.input(file)?, &sch::LoadOptions { keep_raw: ctx.raw })
}

fn doc_out(d: &SchDoc, data: Value) -> Result<Out> {
    Ok(Out::new(data)?.warnings(d.warnings.iter().cloned()))
}

pub fn info(file: &Path, ctx: &Ctx) -> Result<Out> {
    let d = load(file, ctx)?;
    let g = connect::graph(&d);
    let nets = connect::sheet_nets(&d, &g, &connect::NetOptions::default());
    let params: serde_json::Map<String, Value> =
        d.parameters.iter().map(|p| (p.name.clone(), json!(p.effective()))).collect();
    let designators: std::collections::BTreeSet<&str> = d.components.iter().map(|c| c.designator.as_str()).collect();
    doc_out(
        &d,
        json!({
            "file": d.path,
            "header": d.header.str("HEADER"),
            "unique_id": d.header.get("UNIQUEID"),
            "sheet": d.sheet,
            "parameters": params,
            "record_counts": d.record_counts(),
            "records": d.records.len(),
            "components": designators.len(),
            "component_instances": d.components.len(),
            "pins": d.components.iter().map(|c| c.pins.len()).sum::<usize>(),
            "nets": nets.len(),
            "net_labels": d.net_labels.len(),
            "power_ports": d.power_ports.len(),
            "ports": d.ports.len(),
            "sheet_symbols": d.sheet_symbols.len(),
            "wires": d.wires.len(),
            "buses": d.buses.len(),
            "images": d.images.len(),
            "storage_files": d.storage.len(),
            "templates": d.templates,
            "warnings_count": d.warnings.len(),
        }),
    )
}

/// Replace sheet-scope pin nets with the project's net names (pins on no project net get
/// `net: null`).
fn project_pin_nets(d: &mut SchDoc, project: &Path, ctx: &Ctx) -> Result<()> {
    let p = Project::load(&ctx.input(project)?)?;
    let n = sproj::netlist(&p)?;
    let path = d.path.clone();
    let canon = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    if !n.pin_nets.keys().any(|(f, _)| f == &canon) {
        return Err(Error::not_found(format!("{} is not a schematic of {}", path.display(), project.display()))
            .hint("`ace project documents <project> --kind schematic` lists them"));
    }
    for c in &mut d.components {
        for pin in &mut c.pins {
            pin.net = n.pin_net(&path, pin.index).map(str::to_owned);
        }
    }
    Ok(())
}

fn component_row(c: &sch::Component) -> Value {
    let params: serde_json::Map<String, Value> =
        c.parameters.iter().map(|p| (p.name.clone(), json!(p.effective()))).collect();
    let mut v = json!({
        "index": c.index,
        "designator": c.designator,
        "display_designator": c.display_designator,
        "lib_ref": c.lib_ref,
        "comment": c.comment,
        "description": c.description,
        "kind": c.kind,
        "parameters": params,
        "location": c.location,
        "orientation": c.orientation,
        "mirrored": c.mirrored,
        "unique_id": c.unique_id,
        "pins": c.pins.len(),
    });
    let o = v.as_object_mut().unwrap();
    if let Some(p) = &c.part {
        o.insert("part".into(), json!(p));
        o.insert("part_id".into(), json!(c.part_id));
        o.insert("part_count".into(), json!(c.part_count));
    }
    for (k, val) in [("footprint", &c.footprint), ("library", &c.library), ("design_item_id", &c.design_item_id)] {
        if let Some(x) = val {
            o.insert(k.into(), json!(x));
        }
    }
    if let Some(r) = &c.raw {
        o.insert("raw".into(), json!(r));
    }
    v
}

fn write_edit(ctx: &Ctx, d: &SchDoc, result: Result<(Vec<u8>, Vec<edit::Change>)>, extra: Value) -> Result<Out> {
    let (bytes, changes) = result?;
    let changed = !changes.iter().all(|c| c.action == "set" && c.before == c.after) && !changes.is_empty();
    if !changed {
        return Out::new(json!({"changed": false, "file": d.path, "changes": changes, "backup": null, "verified": true}));
    }
    edit::verify(d, &bytes, &changes)?;
    let backup = ctx.write_file(&d.path, &bytes)?;
    if !ctx.dry_run {
        // Read back what landed on disk.
        let on_disk = std::fs::read(&d.path)?;
        if on_disk != bytes {
            return Err(Error::new(altium_core::ErrorCode::Unsafe, "file on disk differs from what was written")
                .hint(format!("restore from the backup: {}", backup.as_ref().map(|b| b.display().to_string()).unwrap_or_default())));
        }
    }
    let mut v = json!({"changed": true, "file": d.path, "changes": changes, "backup": backup, "verified": true});
    if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) {
        o.extend(e);
    }
    Out::new(v)
}

fn write_parameter(ctx: &Ctx, d: &SchDoc, result: Result<(Vec<u8>, Vec<edit::Change>)>, literal: bool, designator: &str) -> Result<Out> {
    let (bytes, changes) = result?;
    let special = changes.iter().find_map(|c| c.before.as_deref().filter(|s| s.starts_with('=')));
    let hint = special.map(|reference| format!(
        "special string {reference:?}: set its parameter with `ace sch set-param \"{}\" {designator} {} <value>`; use --literal to replace the reference",
        d.path.display(), reference.trim_start_matches('=')));
    if !literal && !ctx.dry_run && let Some(hint) = &hint {
        return Err(Error::new(altium_core::ErrorCode::Unsafe, "refusing to replace a special-string reference").hint(hint));
    }
    let mut out = write_edit(ctx, d, Ok((bytes, changes)), json!({}))?;
    if let Some(items) = out.data["changes"].as_array_mut() {
        for c in items {
            c["special_string"] = json!(c["before"].as_str().is_some_and(|s| s.starts_with('=')));
        }
    }
    if let Some(hint) = hint { out.warnings.push(hint); }
    Ok(out)
}

pub fn run(c: SchCmd, ctx: &Ctx) -> Result<Out> {
    match c {
        SchCmd::Info { file } => info(&file, ctx),
        SchCmd::Components { file } => {
            let d = load(&file, ctx)?;
            let items: Vec<Value> = d.components.iter().map(component_row).collect();
            doc_out(&d, list(&items)?)
        }
        SchCmd::Component { file, designator, project } => {
            let mut d = load(&file, ctx)?;
            if let Some(p) = project {
                project_pin_nets(&mut d, &p, ctx)?;
            }
            let hits = d.find_components(&designator)?;
            doc_out(&d, list(&hits)?)
        }
        SchCmd::Pins { file, component, project, unconnected } => {
            let mut d = load(&file, ctx)?;
            if let Some(p) = project {
                project_pin_nets(&mut d, &p, ctx)?;
            }
            let comps: Vec<&sch::Component> = match &component {
                Some(des) => d.find_components(des)?,
                None => d.components.iter().collect(),
            };
            let mut items = Vec::new();
            for c in comps {
                for p in &c.pins {
                    if unconnected && p.net.is_some() { continue; }
                    let mut v = serde_json::to_value(p)?;
                    v["pin"] = json!(p.designator);
                    v["net"] = json!(p.net);
                    v["connected"] = json!(p.net.is_some());
                    v.as_object_mut().unwrap().insert("component".into(), json!(c.display_designator));
                    items.push(v);
                }
            }
            doc_out(&d, list(&items)?)
        }
        SchCmd::Nets { file } => {
            let d = load(&file, ctx)?;
            let g = connect::graph(&d);
            let nets = connect::sheet_nets(&d, &g, &connect::NetOptions::default());
            let single = nets.iter().filter(|n| n.nodes.len() == 1).count();
            doc_out(&d, list_with(&nets, json!({"single_node": single, "dangling_wires": g.dangling_wires}))?)
        }
        SchCmd::ProjectNets { project } => {
            let p = Project::load(&ctx.input(&project)?)?;
            let n = sproj::netlist(&p)?;
            let single = n.nets.iter().filter(|x| x.nodes.len() == 1).count();
            Ok(Out::new(list_with(
                &n.nets,
                json!({"scope": n.scope, "configured_scope": n.configured_scope, "single_node": single, "parts": n.parts.len()}),
            )?)?
            .warnings(n.warnings))
        }
        SchCmd::NetLabels { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.net_labels)?)
        }
        SchCmd::PowerPorts { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.power_ports)?)
        }
        SchCmd::Ports { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.ports)?)
        }
        SchCmd::SheetSymbols { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.sheet_symbols)?)
        }
        SchCmd::Wires { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.wires)?)
        }
        SchCmd::Buses { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list_with(&d.buses, json!({"entries": d.bus_entries}))?)
        }
        SchCmd::Junctions { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.junctions)?)
        }
        SchCmd::NoErc { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.no_ercs)?)
        }
        SchCmd::Texts { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.texts)?)
        }
        SchCmd::Parameters { file } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.parameters)?)
        }
        SchCmd::Directives { file } => {
            let d = load(&file, ctx)?;
            let mut items: Vec<Value> = d.directives.iter().map(serde_json::to_value).collect::<std::result::Result<_, _>>()?;
            for n in &d.no_ercs {
                let mut v = serde_json::to_value(n)?;
                v.as_object_mut().unwrap().insert("kind".into(), json!("no_erc"));
                items.push(v);
            }
            items.sort_by_key(|v| v["index"].as_u64().unwrap_or(0));
            doc_out(&d, list(&items)?)
        }
        SchCmd::Images { file, extract } => {
            let d = load(&file, ctx)?;
            let mut extra = json!({"storage": d.storage});
            if let Some(dir) = extract {
                let mut written = Vec::new();
                if !ctx.dry_run {
                    std::fs::create_dir_all(&dir)?;
                }
                let mut used = std::collections::HashSet::new();
                for f in &d.storage {
                    let name = crate::commands::pcb::unique_name(&f.base_name(), f.index, &mut used);
                    let path = dir.join(&name);
                    ctx.export_file(&path, &f.data)?;
                    written.push(path);
                }
                extra["extracted"] = json!(written);
            }
            doc_out(&d, list_with(&d.images, extra)?)
        }
        SchCmd::Records { file, record_type } => {
            let d = load(&file, ctx)?;
            let want = match &record_type {
                Some(t) => Some(rt::type_number(t).ok_or_else(|| {
                    let names: Vec<String> = rt::known_types().iter().map(|(n, s)| format!("{s}={n}")).collect();
                    Error::usage(format!("unknown record type {t:?}")).hint(format!("types: {}", names.join(", ")))
                })?),
                None => None,
            };
            let items: Vec<Value> = d
                .records
                .iter()
                .filter(|r| want.is_none_or(|w| r.record == w))
                .map(|r| {
                    let mut v = json!({"index": r.index, "stream": r.stream, "record": r.record, "type": r.type_name(), "props": r.props});
                    let o = v.as_object_mut().unwrap();
                    if let Some(own) = r.owner() {
                        o.insert("owner".into(), json!(own));
                        if let Some(des) = d.owner_designator(own) {
                            o.insert("owner_designator".into(), json!(des));
                        }
                    }
                    if r.binary {
                        o.insert("binary".into(), json!(true));
                    }
                    v
                })
                .collect();
            doc_out(&d, list(&items)?)
        }
        SchCmd::Find { file, needle } => {
            let d = load(&file, ctx)?;
            doc_out(&d, list(&d.find(&needle))?)
        }
        SchCmd::Locate { project, designators } => {
            let p = Project::load(&ctx.input(&project)?)?;
            let (items, warnings) = sproj::locate(&p, &designators)?;
            Ok(Out::new(list(&items)?)?.warnings(warnings))
        }
        SchCmd::Hierarchy { project } => {
            let p = Project::load(&ctx.input(&project)?)?;
            let h = sproj::hierarchy(&p)?;
            let w = h.warnings.clone();
            Ok(Out::new(&h)?.warnings(w))
        }
        SchCmd::SetParam { file, designator, name, value, literal } => {
            let d = load(&file, ctx)?;
            write_parameter(ctx, &d, edit::set_param(&d, &designator, &name, &value), literal, &designator)
        }
        SchCmd::RemoveParam { file, designator, name } => {
            let d = load(&file, ctx)?;
            let r = edit::remove_param(&d, &designator, &name);
            let renumbered = r.as_ref().map(|x| x.2).unwrap_or(0);
            write_edit(ctx, &d, r.map(|(b, c, _)| (b, c)), json!({"renumbered": renumbered}))
        }
        SchCmd::SetComment { file, designator, value, literal } => {
            let d = load(&file, ctx)?;
            write_parameter(ctx, &d, edit::set_comment(&d, &designator, &value), literal, &designator)
        }
        SchCmd::SetFootprint { file, designator, footprint } => {
            let d = load(&file, ctx)?;
            write_edit(ctx, &d, edit::set_footprint(&d, &designator, &footprint), json!({}))
        }
        SchCmd::SetDesignator { file, designator, new } => {
            let d = load(&file, ctx)?;
            write_edit(ctx, &d, edit::set_designator(&d, &designator, &new), json!({}))
        }
        SchCmd::SetSheetParam { file, name, value } => {
            let d = load(&file, ctx)?;
            write_edit(ctx, &d, edit::set_sheet_param(&d, &name, &value), json!({}))
        }
    }
}
