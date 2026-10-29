//! `ace pcb ...` — PCB documents (.PcbDoc): read natively, plus byte-safe edits of rules, classes, component fields and parameters.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use serde::Serialize;
use serde_json::{Value, json};

use altium_core::error::{Error, Result};
use altium_doc::pcb::{self, Layer, LoadOptions, PcbDoc, part, query, svg};

use crate::ctx::Ctx;
use crate::meta::{Backend::*, CmdMeta, Effect::*, m};
use crate::output::{Out, list_with};

#[derive(Subcommand, Debug)]
pub enum PcbCmd {
    /// Board summary: size, outline, layers, stackup thickness, object counts, file version.
    Info { file: PathBuf },
    /// Every enabled or used layer: id, Altium name, user name, kind, copper thickness, objects.
    Layers {
        file: PathBuf,
        /// Include disabled and display-only layers too.
        #[arg(long)]
        all: bool,
    },
    /// Physical layer stack top to bottom: thickness, material, dielectric constant, total.
    Stackup { file: PathBuf },
    /// Placed components with footprint, comment, side, position, rotation, schematic link.
    Components {
        file: PathBuf,
        /// Only `top` or `bottom`.
        #[arg(long)]
        side: Option<String>,
    },
    /// One component in full: its pads (with nets), 3D bodies, texts and parameters.
    Component { file: PathBuf, designator: String },
    /// Nets with pad/node counts and routed length.
    Nets { file: PathBuf },
    /// One net: pads/nodes, track/via/arc counts, routed length, layers, classes, pours.
    Net { file: PathBuf, name: String },
    /// Net, component, pad, layer, diff-pair, polygon and xSignal classes with members.
    Classes {
        file: PathBuf,
        /// Only this kind (net, component, pad, layer, diff_pair, polygon, from_to, xsignal).
        #[arg(long)]
        kind: Option<String>,
    },
    /// Every design rule: kind, name, enabled, priority, scopes and decoded settings.
    Rules {
        file: PathBuf,
        /// Only this rule kind (Clearance, Width, RoutingVias, HoleSize, ...).
        #[arg(long)]
        kind: Option<String>,
    },
    /// One design rule by name.
    Rule { file: PathBuf, name: String },
    /// Pads: number, shape, size, hole, plating, layer, net, rotation, position, mask expansion.
    Pads {
        file: PathBuf,
        #[arg(long)]
        component: Option<String>,
        #[arg(long)]
        net: Option<String>,
        #[arg(long)]
        layer: Option<String>,
    },
    /// Vias: position, diameter, hole, layer span, tenting, net.
    Vias {
        file: PathBuf,
        #[arg(long)]
        net: Option<String>,
    },
    /// Track segments: endpoints, width, length, layer, net.
    Tracks {
        file: PathBuf,
        #[arg(long)]
        net: Option<String>,
        #[arg(long)]
        layer: Option<String>,
    },
    /// Arcs: centre, radius, angles, width, length, layer, net.
    Arcs {
        file: PathBuf,
        #[arg(long)]
        net: Option<String>,
        #[arg(long)]
        layer: Option<String>,
    },
    /// Rectangular fills.
    Fills {
        file: PathBuf,
        #[arg(long)]
        layer: Option<String>,
    },
    /// Regions: copper regions, poured polygon pieces, cutouts, keepouts, board cutouts,
    /// shape-based regions and board regions.
    Regions {
        file: PathBuf,
        #[arg(long)]
        layer: Option<String>,
        /// Only this kind (copper, polygon_cutout, board_cutout, dashed_outline, cavity_definition).
        #[arg(long)]
        kind: Option<String>,
        /// Only this source: shape_based_regions (default list, arc-exact), board_regions, or
        /// regions (the straight-edge Regions6 copies of the shape-based regions).
        #[arg(long)]
        source: Option<String>,
        /// Omit outline vertices (counts, area and bbox only).
        #[arg(long)]
        brief: bool,
    },
    /// Polygon pours: net, layer, hatch and pour settings, outline.
    Polygons { file: PathBuf },
    /// Free and component texts with resolved text, font, height, position, rotation.
    Texts {
        file: PathBuf,
        #[arg(long)]
        layer: Option<String>,
    },
    /// Dimension objects.
    Dimensions { file: PathBuf },
    /// 3D component bodies: component, model, layer, heights, placement.
    Bodies {
        file: PathBuf,
        #[arg(long)]
        component: Option<String>,
    },
    /// Embedded 3D models; `--extract DIR` writes the decompressed STEP files.
    Models {
        file: PathBuf,
        /// Write every embedded model into this directory.
        #[arg(long)]
        extract: Option<PathBuf>,
    },
    /// Drill table from pads and vias: size, shape, plating, count.
    Holes { file: PathBuf },
    /// Differential pairs (DifferentialPairs6 records and diff-pair classes).
    Diffpairs { file: PathBuf },
    /// Rooms (RoomDefinition rules) with their outlines.
    Rooms { file: PathBuf },
    /// Board outline vertices (with arc edges), area, perimeter, bounding box.
    Outline { file: PathBuf },
    /// Object counts per type per layer.
    Stats { file: PathBuf },
    /// Routed length per net from tracks and arcs.
    Lengths {
        file: PathBuf,
        /// Only nets of this net class.
        #[arg(long)]
        class: Option<String>,
    },
    /// Set one field of a design rule (gap, minlimit, maxlimit, enabled, priority, scope1...).
    SetRule {
        file: PathBuf,
        /// Rule name (see `pcb rules`).
        rule: String,
        /// Field as `pcb rules` shows it under values (gap, minlimit...) or enabled, priority,
        /// scope1, scope2, comment, name.
        key: String,
        /// New value; lengths need a unit (0.2mm, 8mil), booleans true/false.
        value: String,
    },
    /// Set a non-geometric component field: description, kind, locked, designator_visible,
    /// comment_visible, height, footprint_description.
    SetComponent { file: PathBuf, designator: String, field: String, value: String },
    /// Set (or add) a component parameter.
    SetParam { file: PathBuf, designator: String, name: String, value: String },
    /// Remove a component parameter.
    RemoveParam { file: PathBuf, designator: String, name: String },
    /// Add members (nets, designators...) to an existing class.
    ClassAdd {
        file: PathBuf,
        class: String,
        #[arg(required = true, num_args = 1..)]
        members: Vec<String>,
        /// Class kind when several classes share the name (net, component, ...).
        #[arg(long)]
        kind: Option<String>,
    },
    /// Remove members from a class.
    ClassRemove {
        file: PathBuf,
        class: String,
        #[arg(required = true, num_args = 1..)]
        members: Vec<String>,
        #[arg(long)]
        kind: Option<String>,
    },
    /// Create a class (net by default) with optional initial members.
    AddClass {
        file: PathBuf,
        name: String,
        members: Vec<String>,
        /// net, component, pad, layer, from_to, diff_pair, polygon.
        #[arg(long, default_value = "net")]
        kind: String,
    },
    /// Write an SVG preview of chosen layers.
    Svg {
        file: PathBuf,
        /// Output file.
        #[arg(long, short)]
        out: PathBuf,
        /// Layers to draw (comma-separated names); default: copper, overlays, keep-out,
        /// multi-layer and the board outline.
        #[arg(long, value_delimiter = ',')]
        layers: Vec<String>,
        /// Draw texts as their bounding boxes.
        #[arg(long)]
        text_boxes: bool,
    },
}

pub const META: &[CmdMeta] = &[
    m("pcb info", Native, Read,
      "{file, format, board:{origin, width, height, bbox, area, outline_vertices}, copper_layers, stackup:{source, total_thickness}, components:{total, top, bottom}, nets, counts:{pads, vias, tracks, ...}, streams:{Pads6:N, ...}}",
      "ace pcb info Board.PcbDoc", "Counts come from each storage's Header stream; every other command decodes the records."),
    m("pcb layers", Native, Read,
      "{count, items:[{layer, id, legacy_id, user_name, kind, enabled, used_by_prims, copper_thickness, mech_kind, stack_index, objects}]}",
      "ace pcb layers Board.PcbDoc --where kind=mechanical",
      "layer is the Altium name used everywhere else (TopLayer, MidLayer1, Mechanical13, TopOverlay, MultiLayer); --all adds disabled/system layers."),
    m("pcb stackup", Native, Read,
      "{source, total_thickness, board_thickness, count, items:[{index, name, layer, kind, thickness, material, dielectric_constant, loss_tangent, dielectric_type, component_side}]}",
      "ace pcb stackup Board.PcbDoc --units mil", ""),
    m("pcb components", Native, Read,
      "{count, items:[{index, designator, footprint, comment, description, side, x, y, rotation, locked, height, kind, source_unique_id, sch_unique_id, source_library, footprint_library, parameters, pads}]}",
      "ace pcb components Board.PcbDoc --side bottom --fields designator,footprint,x,y",
      "sch_unique_id matches the schematic component UNIQUEID (sch↔pcb link)."),
    m("pcb component", Native, Read, "{component:{...}, pads:[{number, net, shape, size, hole_size, ...}], bodies:[...], texts:[...], primitives:{tracks, arcs, ...}}",
      "ace pcb component Board.PcbDoc U1", "Designator match is case-insensitive; `#N` selects by index."),
    m("pcb nets", Native, Read, "{count, items:[{index, name, pads, nodes, tracks, vias, arcs, routed_length, unique_id}]}",
      "ace pcb nets Board.PcbDoc --where name~^GND", ""),
    m("pcb net", Native, Read,
      "{index, name, node_count, pads:[{node, component, number, layer, x, y}], tracks, arcs, vias, fills, regions, polygons, routed_length, layers, classes}",
      "ace pcb net Board.PcbDoc GND", ""),
    m("pcb classes", Native, Read, "{count, items:[{index, name, kind, kind_code, members, superclass, auto_generated, stream}]}",
      "ace pcb classes Board.PcbDoc --kind net", ""),
    m("pcb rules", Native, Read,
      "{count, items:[{index, name, kind, kind_code, enabled, priority, scope1, scope2, values:{gap|minlimit|...}}]}",
      "ace pcb rules Board.PcbDoc --kind Clearance", "Lengths in values are converted to --units; keys are Altium's, lower-cased."),
    m("pcb rule", Native, Read, "rule object", "ace pcb rule Board.PcbDoc Clearance", "Exact name, then case-insensitive."),
    m("pcb pads", Native, Read,
      "{count, items:[{index, number, component, net, layer, x, y, rotation, shape, size:{x,y}, mode, hole_size, hole_shape, slot_length, plated, paste_mask_expansion, solder_mask_expansion, ...}]}",
      "ace pcb pads Board.PcbDoc --component U1 --fields number,net,shape,size", ""),
    m("pcb vias", Native, Read,
      "{count, items:[{index, net, x, y, diameter, hole_size, start_layer, end_layer, mode, tented_top, tented_bottom, solder_mask_expansion?}]}",
      "ace pcb vias Board.PcbDoc --net GND", ""),
    m("pcb tracks", Native, Read, "{count, total_length, items:[{index, layer, net, component, start, end, width, length, polygon?, keepout?}]}",
      "ace pcb tracks Board.PcbDoc --net CLK --layer TopLayer", "polygon set = part of a hatched pour, not routing."),
    m("pcb arcs", Native, Read, "{count, total_length, items:[{index, layer, net, center, radius, start_angle, end_angle, width, start, end, length}]}",
      "ace pcb arcs Board.PcbDoc --layer TopOverlay", ""),
    m("pcb fills", Native, Read, "{count, items:[{index, layer, net, corner1, corner2, rotation, keepout?}]}",
      "ace pcb fills Board.PcbDoc", ""),
    m("pcb regions", Native, Read,
      "{count, items:[{index, source, layer, net, component, kind, polygon, keepout, keepout_restrictions, outline:[{x,y,arc?}], holes, area, bbox}]}",
      "ace pcb regions Board.PcbDoc --kind board_cutout",
      "Poured polygon copper is regions with polygon set; --brief drops vertices."),
    m("pcb polygons", Native, Read,
      "{count, items:[{index, name, layer, net, polygon_type, hatch_style, pour_over, remove_dead, grid_size, track_width, pour_index, shelved, outline:{vertices, area, bbox}, poured_regions}]}",
      "ace pcb polygons Board.PcbDoc", ""),
    m("pcb texts", Native, Read,
      "{count, items:[{index, layer, component, text, x, y, height, rotation, mirrored, font_type, font_name, is_designator, is_comment, frame?}]}",
      "ace pcb texts Board.PcbDoc --layer TopOverlay", "Text comes from WideStrings6 when present (Unicode), else the legacy 8-bit copy."),
    m("pcb dimensions", Native, Read, "{count, items:[{index, layer, kind, start, references, text_points, height, unit, precision, ...}]}",
      "ace pcb dimensions Board.PcbDoc", ""),
    m("pcb bodies", Native, Read,
      "{count, items:[{index, component, layer, model_id, model_name, model_embedded, standoff_height, overall_height, height, model_rotation_z, outline}]}",
      "ace pcb bodies Board.PcbDoc --component U1", ""),
    m("pcb models", Native, Export,
      "{count, items:[{index, id, name, embedded, stream, compressed_size, used_by, extracted?:{path, size}}], extract_dir?}",
      "ace pcb models Board.PcbDoc --extract ./step", "Without --extract nothing is written; honours --dry-run."),
    m("pcb holes", Native, Read, "{count, total, plated, non_plated, items:[{diameter, shape, slot_length, plated, count, pads, vias, spans}]}",
      "ace pcb holes Board.PcbDoc --units mm", ""),
    m("pcb diffpairs", Native, Read, "{count, items:[{index, name, properties}], classes:[{name, members}]}",
      "ace pcb diffpairs Board.PcbDoc", "DifferentialPairs6 records are shown as raw properties (no sample has any)."),
    m("pcb rooms", Native, Read, "{count, items:[{name, enabled, layer, scope, confinement, lock_components, outline}]}",
      "ace pcb rooms Board.PcbDoc", ""),
    m("pcb outline", Native, Read, "{vertices:[{x,y,arc?:{center,radius,start_angle,end_angle}}], vertex_count, arcs, area, perimeter, bbox, board_cutouts}",
      "ace pcb outline Board.PcbDoc --units mm", "An arc on vertex i shapes the edge from vertex i to i+1."),
    m("pcb stats", Native, Read, "{count, items:[{layer, kind, pads, tracks, arcs, fills, regions, texts, polygons, bodies, dimensions, vias, total}], totals}",
      "ace pcb stats Board.PcbDoc", ""),
    m("pcb lengths", Native, Read, "{count, items:[{net, routed_length, track_length, arc_length, tracks, arcs, vias, layers, by_layer}]}",
      "ace pcb lengths Board.PcbDoc --class DDR --where routed_length!=0",
      "Excludes polygon pour tracks and keepouts; via barrels are not counted."),
    m("pcb svg", Native, Export, "{out, layers:[...], width, height, elements}",
      "ace pcb svg Board.PcbDoc --out board.svg --layers TopLayer,TopOverlay",
      "Preview only (no clearances, polygons drawn from their poured regions)."),
    m("pcb set-rule", Native, Write, "{changed, file, changes:[{stream, record, action, target, key, before, after}], backup, verified}",
      "ace pcb set-rule Board.PcbDoc Clearance gap 0.2mm",
      "Changes retain raw before/after strings and add typed before_value/after_value with unit for lengths. verification distinguishes candidate reparse from disk read-back. Only fields the rule already has. Lengths are written in mil like Altium does; priorities stay unique per kind. New rules and rule kinds need `ace designer`."),
    m("pcb set-component", Native, Write, "{changed, file, changes, backup, verified}",
      "ace pcb set-component Board.PcbDoc U1 kind \"Standard (No BOM)\"",
      "Position, rotation, side, footprint and the designator/comment texts are geometry/binary records: use `ace designer` (or change the schematic and update the PCB)."),
    m("pcb set-param", Native, Write, "{changed, file, changes, backup, verified}",
      "ace pcb set-param Board.PcbDoc U1 MPN LM2596SX-5.0", "Parameters live in PrimitiveParameters keyed by the component's unique id."),
    m("pcb remove-param", Native, Write, "{changed, file, changes, backup, verified}", "ace pcb remove-param Board.PcbDoc U1 Notepad", ""),
    m("pcb class-add", Native, Write, "{changed, file, changes:[{action:add_member, target, after}], backup, verified}",
      "ace pcb class-add Board.PcbDoc PWR 5V 3V3", "Members must exist (nets for net classes, designators for component classes)."),
    m("pcb class-remove", Native, Write, "{changed, file, changes:[{action:remove_member, target, before}], backup, verified}",
      "ace pcb class-remove Board.PcbDoc PWR 3V3", ""),
    m("pcb add-class", Native, Write, "{changed, file, changes:[{action:create, record, target}], backup, verified}",
      "ace pcb add-class Board.PcbDoc DDR DQ0 DQ1 DQ2 --kind net",
      "Copies the record layout of an existing user class; Classes6/Header count is updated."),
];

fn open(file: &Path, ctx: &Ctx, parts: u32) -> Result<PcbDoc> {
    pcb::load_with(&ctx.input(file)?, &LoadOptions::parts(parts).raw(ctx.raw))
}

fn out_list<T: Serialize>(doc: &PcbDoc, items: &[T], extra: Value) -> Result<Out> {
    Ok(Out::new(list_with(items, extra)?)?.warnings(doc.warnings.clone()))
}

/// Resolve a `--layer` argument against Altium names and the board's user layer names.
pub fn resolve_layer(doc: &PcbDoc, name: &str) -> Result<Layer> {
    if let Some(l) = Layer::from_name(name) {
        return Ok(l);
    }
    if let Some(d) = doc.board.layers.iter().find(|d| d.user_name.eq_ignore_ascii_case(name)) {
        return Ok(d.layer);
    }
    let names: Vec<String> = doc.board.layers.iter().filter(|d| d.enabled).map(|d| d.layer.name()).collect();
    Err(Error::usage(format!("unknown layer {name:?}")).hint(format!("use an Altium layer name or user name: {names:?}")))
}

fn net_filter(doc: &PcbDoc, net: &Option<String>) -> Result<Option<u16>> {
    net.as_ref().map(|n| doc.net(n).map(|n| n.index as u16)).transpose()
}

fn comp_filter(doc: &PcbDoc, c: &Option<String>) -> Result<Option<u16>> {
    c.as_ref().map(|c| doc.component(c).map(|c| c.index as u16)).transpose()
}

fn layer_filter(doc: &PcbDoc, l: &Option<String>) -> Result<Option<Layer>> {
    l.as_ref().map(|l| resolve_layer(doc, l)).transpose()
}

pub fn run(c: PcbCmd, ctx: &Ctx) -> Result<Out> {
    match c {
        PcbCmd::Info { file } => info(&file, ctx),
        PcbCmd::Layers { file, all } => {
            let doc = open(&file, ctx, part::ALL & !part::MODELS & !part::RULES & !part::CLASSES)?;
            let usage = query::layer_usage(&doc);
            let items: Vec<Value> = doc
                .board
                .layers
                .iter()
                .filter_map(|d| {
                    let objects = usage.get(&d.layer).copied().unwrap_or(0);
                    if !all && !d.enabled && objects == 0 {
                        return None;
                    }
                    if !all && d.kind == pcb::LayerKind::System && objects == 0 {
                        return None;
                    }
                    let mut v = serde_json::to_value(d).ok()?;
                    v["objects"] = json!(objects);
                    Some(v)
                })
                .collect();
            let mut out = Out::new(list_with(&items, json!({"copper_layers": doc.board.copper_layers}))?)?.warnings(doc.warnings.clone());
            for (l, n) in &usage {
                if *n > 0 && !doc.board.layers.iter().any(|d| d.layer == *l) {
                    out = out.warn(format!("{n} objects on layer {l} which Board6 does not define"));
                }
            }
            Ok(out)
        }
        PcbCmd::Stackup { file } => {
            let doc = open(&file, ctx, 0)?;
            let s = &doc.board.stackup;
            out_list(
                &doc,
                &s.layers,
                json!({"source": s.source, "total_thickness": s.total_thickness, "board_thickness": s.board_thickness,
                       "copper_layers": doc.board.copper_layer_count}),
            )
        }
        PcbCmd::Components { file, side } => {
            let doc = open(&file, ctx, part::PADS)?;
            let want = side.map(|s| s.to_ascii_lowercase());
            if let Some(s) = &want
                && s != "top" && s != "bottom" {
                    return Err(Error::usage(format!("--side {s:?}")).hint("use --side top or --side bottom"));
                }
            let items: Vec<Value> = doc
                .components
                .iter()
                .filter(|c| want.as_deref().is_none_or(|s| c.side == s))
                .map(|c| {
                    let mut v = serde_json::to_value(c).unwrap_or_default();
                    v["pads"] = json!(doc.pads.iter().filter(|p| p.owner.component_index == Some(c.index as u16)).count());
                    v
                })
                .collect();
            out_list(&doc, &items, json!({}))
        }
        PcbCmd::Component { file, designator } => {
            let doc = open(&file, ctx, part::ALL & !part::RULES & !part::CLASSES)?;
            let c = doc.component(&designator)?;
            let ci = Some(c.index as u16);
            let pads: Vec<_> = doc.pads.iter().filter(|p| p.owner.component_index == ci).collect();
            let bodies: Vec<_> = doc.bodies.iter().filter(|p| p.owner.component_index == ci).collect();
            let texts: Vec<_> = doc.texts.iter().filter(|p| p.owner.component_index == ci).collect();
            let nets: std::collections::BTreeSet<&str> = pads.iter().filter_map(|p| p.owner.net.as_deref()).collect();
            let models: Vec<_> = doc.models.iter().filter(|m| bodies.iter().any(|b| b.model_id == m.id)).collect();
            Ok(Out::new(json!({
                "component": c,
                "pads": pads,
                "nets": nets,
                "bodies": bodies,
                "models": models,
                "texts": texts,
                "primitives": {
                    "pads": pads.len(),
                    "tracks": doc.tracks.iter().filter(|p| p.owner.component_index == ci).count(),
                    "arcs": doc.arcs.iter().filter(|p| p.owner.component_index == ci).count(),
                    "fills": doc.fills.iter().filter(|p| p.owner.component_index == ci).count(),
                    "regions": doc.regions.iter().filter(|p| p.owner.component_index == ci).count(),
                    "texts": texts.len(),
                    "vias": doc.vias.iter().filter(|p| p.owner.component_index == ci).count(),
                    "bodies": bodies.len(),
                },
            }))?
            .warnings(doc.warnings.clone()))
        }
        PcbCmd::Nets { file } => {
            let doc = open(&file, ctx, part::PADS | part::VIAS | part::TRACKS | part::ARCS)?;
            let lens = query::lengths(&doc, None);
            let items: Vec<Value> = doc
                .nets
                .iter()
                .zip(&lens)
                .map(|(n, l)| {
                    let ni = Some(n.index as u16);
                    let pads: Vec<_> = doc.pads.iter().filter(|p| p.owner.net_index == ni).collect();
                    let nodes: std::collections::BTreeSet<(Option<&str>, &str)> =
                        pads.iter().filter(|p| p.owner.component.is_some()).map(|p| (p.owner.component.as_deref(), p.number.as_str())).collect();
                    json!({"index": n.index, "name": n.name, "pads": pads.len(), "nodes": nodes.len(),
                           "tracks": l.tracks, "arcs": l.arcs, "vias": l.vias, "routed_length": l.routed_length,
                           "unique_id": n.unique_id})
                })
                .collect();
            out_list(&doc, &items, json!({}))
        }
        PcbCmd::Net { file, name } => {
            let doc = open(&file, ctx, part::COPPER | part::POLYGONS | part::CLASSES)?;
            let n = doc.net(&name)?.index;
            Ok(Out::new(query::net_detail(&doc, n))?.warnings(doc.warnings.clone()))
        }
        PcbCmd::Classes { file, kind } => {
            let doc = open(&file, ctx, part::CLASSES)?;
            let items: Vec<_> = doc.classes.iter().filter(|c| kind.as_deref().is_none_or(|k| c.kind.eq_ignore_ascii_case(k))).collect();
            out_list(&doc, &items, json!({}))
        }
        PcbCmd::Rules { file, kind } => {
            let doc = open(&file, ctx, part::RULES)?;
            let items: Vec<_> = doc.rules.iter().filter(|r| kind.as_deref().is_none_or(|k| r.kind.eq_ignore_ascii_case(k))).collect();
            let mut kinds: Vec<&str> = doc.rules.iter().map(|r| r.kind.as_str()).collect();
            kinds.sort();
            kinds.dedup();
            let mut out = out_list(&doc, &items, json!({"kinds": kinds}))?;
            for rule in items.iter().filter(|r| r.values.is_empty()) {
                out.warnings.push(format!("rule {}: settings not decoded (no typed values); use --raw to inspect Altium properties", rule.name));
            }
            Ok(out)
        }
        PcbCmd::Rule { file, name } => {
            let doc = open(&file, ctx, part::RULES)?;
            let r = doc
                .rules
                .iter()
                .find(|r| r.name == name)
                .or_else(|| doc.rules.iter().find(|r| r.name.eq_ignore_ascii_case(&name)))
                .ok_or_else(|| {
                    let names: Vec<&str> = doc.rules.iter().map(|r| r.name.as_str()).collect();
                    Error::not_found(format!("no rule {name:?}")).hint(format!("rules: {names:?}"))
                })?;
            Ok(Out::new(r)?.warnings(doc.warnings.clone()))
        }
        PcbCmd::Pads { file, component, net, layer } => {
            let doc = open(&file, ctx, part::PADS | part::UNIQUE_IDS)?;
            let (c, n, l) = (comp_filter(&doc, &component)?, net_filter(&doc, &net)?, layer_filter(&doc, &layer)?);
            let items: Vec<_> = doc
                .pads
                .iter()
                .filter(|p| c.is_none_or(|c| p.owner.component_index == Some(c)))
                .filter(|p| n.is_none_or(|n| p.owner.net_index == Some(n)))
                .filter(|p| l.is_none_or(|l| p.layer == l))
                .collect();
            out_list(&doc, &items, json!({}))
        }
        PcbCmd::Vias { file, net } => {
            let doc = open(&file, ctx, part::VIAS)?;
            let n = net_filter(&doc, &net)?;
            let items: Vec<_> = doc.vias.iter().filter(|p| n.is_none_or(|n| p.owner.net_index == Some(n))).collect();
            out_list(&doc, &items, json!({}))
        }
        PcbCmd::Tracks { file, net, layer } => {
            let doc = open(&file, ctx, part::TRACKS)?;
            let (n, l) = (net_filter(&doc, &net)?, layer_filter(&doc, &layer)?);
            let items: Vec<_> = doc
                .tracks
                .iter()
                .filter(|p| n.is_none_or(|n| p.owner.net_index == Some(n)))
                .filter(|p| l.is_none_or(|l| p.layer == l))
                .collect();
            let total: i64 = items.iter().map(|t| t.length.0).sum();
            out_list(&doc, &items, json!({"total_length": altium_core::Coord(total)}))
        }
        PcbCmd::Arcs { file, net, layer } => {
            let doc = open(&file, ctx, part::ARCS)?;
            let (n, l) = (net_filter(&doc, &net)?, layer_filter(&doc, &layer)?);
            let items: Vec<_> = doc
                .arcs
                .iter()
                .filter(|p| n.is_none_or(|n| p.owner.net_index == Some(n)))
                .filter(|p| l.is_none_or(|l| p.layer == l))
                .collect();
            let total: i64 = items.iter().map(|t| t.length.0).sum();
            out_list(&doc, &items, json!({"total_length": altium_core::Coord(total)}))
        }
        PcbCmd::Fills { file, layer } => {
            let doc = open(&file, ctx, part::FILLS)?;
            let l = layer_filter(&doc, &layer)?;
            let items: Vec<_> = doc.fills.iter().filter(|p| l.is_none_or(|l| p.layer == l)).collect();
            out_list(&doc, &items, json!({}))
        }
        PcbCmd::Regions { file, layer, kind, source, brief } => {
            let doc = open(&file, ctx, part::REGIONS)?;
            let l = layer_filter(&doc, &layer)?;
            let flat = source.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("regions")) && !doc.regions_flat.is_empty();
            let items: Vec<Value> = if flat { &doc.regions_flat } else { &doc.regions }
                .iter()
                .filter(|p| l.is_none_or(|l| p.layer == l))
                .filter(|p| kind.as_deref().is_none_or(|k| p.kind.eq_ignore_ascii_case(k)))
                .filter(|p| source.as_deref().is_none_or(|s| p.source.eq_ignore_ascii_case(s)))
                .map(|r| {
                    let mut v = serde_json::to_value(r).unwrap_or_default();
                    v["vertex_count"] = json!(r.outline.len());
                    if brief {
                        if let Some(o) = v.as_object_mut() {
                            o.remove("outline");
                            o.remove("holes");
                        }
                        v["hole_count"] = json!(r.holes.len());
                    }
                    v
                })
                .collect();
            out_list(&doc, &items, json!({}))
        }
        PcbCmd::Polygons { file } => {
            let doc = open(&file, ctx, part::POLYGONS | part::REGIONS)?;
            out_list(&doc, &doc.polygons, json!({}))
        }
        PcbCmd::Texts { file, layer } => {
            let doc = open(&file, ctx, 0)?;
            let l = layer_filter(&doc, &layer)?;
            let items: Vec<_> = doc.texts.iter().filter(|p| l.is_none_or(|l| p.layer == l)).collect();
            out_list(&doc, &items, json!({}))
        }
        PcbCmd::Dimensions { file } => {
            let doc = open(&file, ctx, part::DIMENSIONS)?;
            out_list(&doc, &doc.dimensions, json!({}))
        }
        PcbCmd::Bodies { file, component } => {
            let doc = open(&file, ctx, part::BODIES)?;
            let c = comp_filter(&doc, &component)?;
            let items: Vec<_> = doc.bodies.iter().filter(|p| c.is_none_or(|c| p.owner.component_index == Some(c))).collect();
            out_list(&doc, &items, json!({}))
        }
        PcbCmd::Models { file, extract } => {
            let mut doc = open(&file, ctx, part::MODELS | part::BODIES)?;
            let mut items: Vec<Value> = doc.models.iter().map(|m| serde_json::to_value(m).unwrap_or_default()).collect();
            if let Some(dir) = &extract {
                let written = extract_models(&mut doc, dir, ctx)?;
                for (i, w) in written {
                    items[i]["extracted"] = w;
                }
            }
            out_list(&doc, &items, json!({"extract_dir": extract}))
        }
        PcbCmd::Holes { file } => {
            let doc = open(&file, ctx, part::PADS | part::VIAS)?;
            let h = query::holes(&doc);
            let total: usize = h.iter().map(|h| h.count).sum();
            let plated: usize = h.iter().filter(|h| h.plated).map(|h| h.count).sum();
            out_list(&doc, &h, json!({"total": total, "plated": plated, "non_plated": total - plated}))
        }
        PcbCmd::Diffpairs { file } => {
            let doc = open(&file, ctx, part::DIFFPAIRS | part::CLASSES)?;
            let classes: Vec<Value> = doc
                .classes
                .iter()
                .filter(|c| c.kind == "diff_pair")
                .map(|c| json!({"name": c.name, "members": c.members, "superclass": c.superclass}))
                .collect();
            out_list(&doc, &doc.diff_pairs, json!({"classes": classes}))
        }
        PcbCmd::Rooms { file } => {
            let doc = open(&file, ctx, part::RULES)?;
            out_list(&doc, &doc.rooms, json!({}))
        }
        PcbCmd::Outline { file } => {
            let doc = open(&file, ctx, part::REGIONS)?;
            let cutouts: Vec<Value> = doc
                .regions
                .iter()
                .filter(|r| r.kind == "board_cutout")
                .map(|r| json!({"index": r.index, "source": r.source, "vertices": r.outline, "area": r.area, "bbox": r.bbox}))
                .collect();
            let mut v = serde_json::to_value(&doc.board.outline)?;
            v["board_cutouts"] = json!(cutouts);
            v["origin"] = serde_json::to_value(doc.board.origin)?;
            let mut out = Out::new(v)?.warnings(doc.warnings.clone());
            if doc.board.outline.vertex_count < 3 {
                out = out.warn("Board6 has fewer than 3 outline vertices; the board shape may be defined on a mechanical layer instead");
            }
            Ok(out)
        }
        PcbCmd::Stats { file } => {
            let doc = open(&file, ctx, part::ALL & !part::MODELS & !part::RULES & !part::CLASSES)?;
            let s = query::layer_stats(&doc);
            out_list(&doc, &s, json!({"totals": query::object_counts(&doc)}))
        }
        PcbCmd::Lengths { file, class } => {
            let doc = open(&file, ctx, part::TRACKS | part::ARCS | part::VIAS | part::CLASSES)?;
            if let Some(c) = &class
                && !doc.classes.iter().any(|k| k.kind == "net" && k.name.eq_ignore_ascii_case(c)) {
                    let names: Vec<&str> = doc.classes.iter().filter(|k| k.kind == "net").map(|k| k.name.as_str()).collect();
                    return Err(Error::not_found(format!("no net class {c:?}")).hint(format!("net classes: {names:?}")));
                }
            out_list(&doc, &query::lengths(&doc, class.as_deref()), json!({}))
        }
        PcbCmd::SetRule { file, rule, key, value } => {
            let (doc, bytes) = open_for_edit(&file, ctx, part::RULES)?;
            let k = key.clone();
            write_edit(ctx, &doc, &bytes, pcb::edit::set_rule(&doc, &bytes, &rule, &key, &value), move |n, ch| {
                let r = n.rules.get(ch[0].record).ok_or_else(|| Error::internal("rule vanished"))?;
                let raw = r.raw.as_ref().and_then(|p| p.get(&ch[0].key));
                if raw != ch[0].after {
                    return Err(Error::new(altium_core::ErrorCode::Unsafe, format!("rule field {k} reads back {raw:?}")));
                }
                Ok(())
            })
        }
        PcbCmd::SetComponent { file, designator, field, value } => {
            let (doc, bytes) = open_for_edit(&file, ctx, 0)?;
            write_edit(ctx, &doc, &bytes, pcb::edit::set_component(&doc, &bytes, &designator, &field, &value), |n, ch| {
                let c = n.components.get(ch[0].record).ok_or_else(|| Error::internal("component vanished"))?;
                let raw = c.raw.as_ref().and_then(|p| p.get(&ch[0].key));
                if raw != ch[0].after {
                    return Err(Error::new(altium_core::ErrorCode::Unsafe, format!("component field reads back {raw:?}")));
                }
                Ok(())
            })
        }
        PcbCmd::SetParam { file, designator, name, value } => {
            let (doc, bytes) = open_for_edit(&file, ctx, 0)?;
            let d = doc.component(&designator)?.index;
            write_edit(ctx, &doc, &bytes, pcb::edit::set_param(&doc, &bytes, &designator, &name, &value), move |n, _| {
                let got = n.components[d].parameters.iter().find(|(k, _)| k.eq_ignore_ascii_case(&name)).map(|(_, v)| v.as_str());
                if got != Some(value.as_str()) {
                    return Err(Error::new(altium_core::ErrorCode::Unsafe, format!("parameter {name} reads back {got:?}")));
                }
                Ok(())
            })
        }
        PcbCmd::RemoveParam { file, designator, name } => {
            let (doc, bytes) = open_for_edit(&file, ctx, 0)?;
            let d = doc.component(&designator)?.index;
            write_edit(ctx, &doc, &bytes, pcb::edit::remove_param(&doc, &bytes, &designator, &name), move |n, _| {
                if n.components[d].parameters.keys().any(|k| k.eq_ignore_ascii_case(&name)) {
                    return Err(Error::new(altium_core::ErrorCode::Unsafe, format!("parameter {name} is still there")));
                }
                Ok(())
            })
        }
        PcbCmd::ClassAdd { file, class, members, kind } => class_edit(&file, ctx, &class, kind.as_deref(), &members, &[]),
        PcbCmd::ClassRemove { file, class, members, kind } => class_edit(&file, ctx, &class, kind.as_deref(), &[], &members),
        PcbCmd::AddClass { file, name, members, kind } => {
            let (doc, bytes) = open_for_edit(&file, ctx, part::CLASSES)?;
            let before = doc.classes.len();
            write_edit(ctx, &doc, &bytes, pcb::edit::add_class(&doc, &bytes, &name, &kind, &members), move |n, _| {
                let ok = n.classes.len() == before + 1
                    && n.classes.iter().any(|c| c.name == name && c.members.len() == members.len());
                if !ok {
                    return Err(Error::new(altium_core::ErrorCode::Unsafe, "the new class does not read back"));
                }
                Ok(())
            })
        }
        PcbCmd::Svg { file, out, layers, text_boxes } => {
            let doc = open(&file, ctx, part::ALL & !part::RULES & !part::CLASSES & !part::MODELS)?;
            let ls = layers.iter().map(|l| resolve_layer(&doc, l)).collect::<Result<Vec<_>>>()?;
            let opts = svg::Options { layers: ls, text_boxes };
            let r = svg::render_pcb(&doc, &opts);
            ctx.export_file(&out, r.svg.as_bytes())?;
            Ok(Out::new(json!({"out": out, "layers": r.layers, "width": r.width, "height": r.height, "elements": r.elements}))?
                .warnings(doc.warnings.clone()))
        }
    }
}

fn open_for_edit(file: &Path, ctx: &Ctx, parts: u32) -> Result<(PcbDoc, Vec<u8>)> {
    let path = ctx.input(file)?;
    let bytes = std::fs::read(&path)?;
    let doc = pcb::load_bytes(&path, bytes.clone(), &LoadOptions::parts(parts).raw(true))?;
    Ok((doc, bytes))
}

fn class_edit(file: &Path, ctx: &Ctx, class: &str, kind: Option<&str>, add: &[String], remove: &[String]) -> Result<Out> {
    let (doc, bytes) = open_for_edit(file, ctx, part::CLASSES)?;
    let r = pcb::edit::class_members(&doc, &bytes, class, kind, add, remove);
    let (add, remove) = (add.to_vec(), remove.to_vec());
    write_edit(ctx, &doc, &bytes, r, move |n, ch| {
        let Some(c) = ch.first().and_then(|c| n.classes.iter().find(|k| k.name == c.target)) else { return Ok(()) };
        let has = |m: &String| c.members.iter().any(|x| x.eq_ignore_ascii_case(m));
        if !add.iter().all(has) || remove.iter().any(has) {
            return Err(Error::new(altium_core::ErrorCode::Unsafe, format!("class {} members read back as {:?}", c.name, c.members)));
        }
        Ok(())
    })
}

/// Verify an edit (re-parse, untouched streams identical, `check` on the new document),
/// then write it with `.bak` / `--dry-run` and read it back.
fn write_edit(
    ctx: &Ctx,
    doc: &PcbDoc,
    bytes: &[u8],
    r: Result<(Vec<u8>, Vec<pcb::edit::Change>)>,
    check: impl Fn(&PcbDoc, &[pcb::edit::Change]) -> Result<()>,
) -> Result<Out> {
    let (new_bytes, changes) = r?;
    if changes.is_empty() {
        return Out::new(json!({"changed": false, "file": doc.path, "changes": changes, "backup": null, "verified": true,
            "verification":{"candidate_reparsed":false,"only_expected_changes":true,"bytes_changed":0,
                "streams_touched":[],"written":false,"read_back":false}}));
    }
    let mut edited: Vec<String> = changes.iter().map(|c| c.stream.clone()).collect();
    edited.dedup();
    let new = pcb::edit::verify(doc, bytes, &new_bytes, &edited)?;
    let new = pcb::load_bytes(&doc.path, new_bytes.clone(), &LoadOptions::parts(new.loaded).raw(true))?;
    check(&new, &changes)?;
    let backup = ctx.write_file(&doc.path, &new_bytes)?;
    if !ctx.dry_run && std::fs::read(&doc.path)? != new_bytes {
        return Err(Error::new(altium_core::ErrorCode::Unsafe, "file on disk differs from what was written")
            .hint(format!("restore from the backup: {}", backup.as_ref().map(|b| b.display().to_string()).unwrap_or_default())));
    }
    let mut rows = serde_json::to_value(&changes)?;
    for row in rows.as_array_mut().unwrap() {
        if row["stream"] != "Rules6" { continue; }
        let before = row["before"].as_str().map(pcb::records::value);
        let after = row["after"].as_str().map(pcb::records::value);
        let length = before.iter().chain(after.iter()).any(|v| matches!(v, pcb::records::Value::Len(_)));
        row["before_value"] = json!(before);
        row["after_value"] = json!(after);
        if length { row["unit"] = json!(altium_core::units::output_unit().name()); }
    }
    let bytes_changed = bytes.iter().zip(&new_bytes).filter(|(a,b)| a != b).count() + bytes.len().abs_diff(new_bytes.len());
    Out::new(json!({"changed": true, "file": doc.path, "changes": rows, "backup": backup, "verified": true,
        "verification":{"candidate_reparsed":true,"only_expected_changes":true,"bytes_changed":bytes_changed,
            "streams_touched":edited,"written":!ctx.dry_run,"read_back":!ctx.dry_run}}))
}

/// Write each embedded model to `dir`; returns (model index, {path, size}) for each.
fn extract_models(doc: &mut PcbDoc, dir: &Path, ctx: &Ctx) -> Result<Vec<(usize, Value)>> {
    if !ctx.dry_run {
        std::fs::create_dir_all(dir)?;
    }
    let mut used = std::collections::HashSet::new();
    let mut out = vec![];
    for i in 0..doc.models.len() {
        if doc.models[i].stream.is_none() {
            continue;
        }
        let data = doc.model_data(i)?;
        let path = dir.join(unique_name(&doc.models[i].name, i, &mut used));
        ctx.export_file(&path, &data)?;
        out.push((i, json!({"path": path, "size": data.len()})));
    }
    Ok(out)
}

/// File-system-safe, unique file name for a model.
pub fn unique_name(name: &str, i: usize, used: &mut std::collections::HashSet<String>) -> String {
    let clean: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let clean = clean.trim().trim_end_matches(['.', ' ']).to_owned();
    let stem = clean.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ["COM", "LPT"].iter().any(|p| stem.strip_prefix(p).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")));
    let clean = if reserved { format!("_{clean}") } else { clean };
    let base = if clean.is_empty() { format!("model_{i}.step") } else { clean };
    let mut cand = base.clone();
    let mut k = 1;
    while !used.insert(cand.to_ascii_lowercase()) {
        cand = match base.rsplit_once('.') {
            Some((b, e)) => format!("{b}_{k}.{e}"),
            None => format!("{base}_{k}"),
        };
        k += 1;
    }
    cand
}

pub fn info(file: &Path, ctx: &Ctx) -> Result<Out> {
    let doc = open(file, ctx, 0)?;
    let b = &doc.board;
    let c = |k: &str| doc.counts.get(k).copied().unwrap_or(0);
    let top = doc.components.iter().filter(|c| c.side == "top").count();
    let mech: Vec<String> = b
        .layers
        .iter()
        .filter(|l| l.kind == pcb::LayerKind::Mechanical && l.enabled)
        .map(|l| format!("{} ({})", l.layer, l.user_name))
        .collect();
    let bbox = b.outline.bbox;
    let mut out = Out::new(json!({
        "file": doc.path,
        "format": doc.file,
        "saved": b.saved,
        "board": {
            "origin": b.origin,
            "width": bbox.map(|b| b.width),
            "height": bbox.map(|b| b.height),
            "bbox": bbox,
            "area": b.outline.area,
            "outline_vertices": b.outline.vertex_count,
            "outline_arcs": b.outline.arcs,
            "sheet": b.sheet,
        },
        "copper_layers": b.copper_layer_count,
        "copper_layer_names": b.copper_layers,
        "mechanical_layers": mech,
        "stackup": {"source": b.stackup.source, "total_thickness": b.stackup.total_thickness, "board_thickness": b.stackup.board_thickness, "layers": b.stackup.layers.len()},
        "components": {"total": doc.components.len(), "top": top, "bottom": doc.components.len() - top},
        "nets": doc.nets.len(),
        "counts": {
            "components": c("Components6"),
            "nets": c("Nets6"),
            "pads": c("Pads6"),
            "vias": c("Vias6"),
            "tracks": c("Tracks6"),
            "arcs": c("Arcs6"),
            "fills": c("Fills6"),
            "texts": c("Texts6"),
            "regions": c("Regions6"),
            "shape_based_regions": c("ShapeBasedRegions6"),
            "board_regions": c("BoardRegions"),
            "polygons": c("Polygons6"),
            "dimensions": c("Dimensions6"),
            "bodies": c("ComponentBodies6"),
            "models": c("Models"),
            "classes": c("Classes6"),
            "rules": c("Rules6"),
            "diff_pairs": c("DifferentialPairs6"),
            "embedded_fonts": c("EmbeddedFonts6"),
        },
        "plane_nets": b.plane_nets,
        "streams": doc.counts,
    }))?;
    if ctx.raw {
        out.data["raw"] = b.props.to_json();
    }
    out = out.warnings(doc.warnings.clone());
    if b.outline.vertex_count < 3 {
        out = out.warn("Board6 has no board outline (fewer than 3 vertices)");
    }
    Ok(out)
}
