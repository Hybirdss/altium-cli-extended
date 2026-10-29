//! `.PcbDoc` PCB documents (OLE compound file, one storage per object type).
//!
//! [`load_with`] parses only the streams a caller needs ([`LoadOptions::parts`]); board,
//! nets, components and texts are always read because every other object refers to them.
//! Primitives carry their net name and owning component's designator, resolved here.
//!
//! Stream layouts: text-property streams in [`records`], binary primitives in [`prim`], the
//! board header and layer stack in [`board`]. See `ARCHITECTURE.md` for the conventions.

pub mod board;
pub mod edit;
pub mod geom;
pub mod layer;
pub mod prim;
pub mod query;
pub mod records;
pub mod svg;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_format::DocKind;
use altium_format::cfbx::{Bin, Compound};
use altium_format::props::Props;

use crate::model;

pub use board::Board;
pub use layer::{Layer, LayerKind};
pub use prim::{Arc, Body, Fill, Owner, Pad, Region, Text, Track, Via};
pub use records::{Class, Component, Dimension, Generic, Model, Net, Polygon, Room, Rule};

/// Which parts of the document to decode (bit set).
pub mod part {
    pub const PADS: u32 = 1 << 0;
    pub const VIAS: u32 = 1 << 1;
    pub const TRACKS: u32 = 1 << 2;
    pub const ARCS: u32 = 1 << 3;
    pub const FILLS: u32 = 1 << 4;
    pub const REGIONS: u32 = 1 << 5;
    pub const POLYGONS: u32 = 1 << 6;
    pub const DIMENSIONS: u32 = 1 << 7;
    pub const BODIES: u32 = 1 << 8;
    pub const MODELS: u32 = 1 << 9;
    pub const CLASSES: u32 = 1 << 10;
    pub const RULES: u32 = 1 << 11;
    pub const DIFFPAIRS: u32 = 1 << 12;
    pub const PARAMS: u32 = 1 << 13;
    pub const UNIQUE_IDS: u32 = 1 << 14;
    /// Copper primitives that carry nets.
    pub const COPPER: u32 = PADS | VIAS | TRACKS | ARCS | FILLS | REGIONS;
    pub const ALL: u32 = (1 << 15) - 1;
}

#[derive(Debug, Clone)]
pub struct LoadOptions {
    /// Bit set of [`part`] constants.
    pub parts: u32,
    /// Keep the raw property maps on text-record objects (`--raw`).
    pub keep_raw: bool,
}

impl Default for LoadOptions {
    fn default() -> Self {
        LoadOptions { parts: part::ALL, keep_raw: false }
    }
}

impl LoadOptions {
    pub fn parts(parts: u32) -> LoadOptions {
        LoadOptions { parts, keep_raw: false }
    }
    pub fn raw(mut self, keep: bool) -> LoadOptions {
        self.keep_raw = keep;
        self
    }
}

/// File-level identification.
#[derive(Debug, Clone, Serialize, Default)]
pub struct FileInfo {
    /// Header string (`PCB 6.0 Binary File`).
    pub header: String,
    /// Binary format version from the header (e.g. 5.01).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_version: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    /// Altium release that last saved the file (last `FileVersionInfo` entry).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_with: Option<String>,
    /// Every release recorded in `FileVersionInfo`, oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub release_history: Vec<String>,
}

pub struct PcbDoc {
    pub path: PathBuf,
    pub file: FileInfo,
    pub board: Board,
    pub nets: Vec<Net>,
    pub components: Vec<Component>,
    pub classes: Vec<Class>,
    pub rules: Vec<Rule>,
    pub rooms: Vec<Room>,
    pub polygons: Vec<Polygon>,
    pub dimensions: Vec<Dimension>,
    pub models: Vec<Model>,
    pub diff_pairs: Vec<Generic>,
    pub pads: Vec<Pad>,
    pub vias: Vec<Via>,
    pub tracks: Vec<Track>,
    pub arcs: Vec<Arc>,
    pub fills: Vec<Fill>,
    pub texts: Vec<Text>,
    /// Regions (see `Region::source`): `ShapeBasedRegions6` (or `Regions6` when there is no
    /// matching shape-based copy), then `BoardRegions`.
    pub regions: Vec<Region>,
    /// `Regions6` when it duplicates `ShapeBasedRegions6` (straight-edge copies).
    pub regions_flat: Vec<Region>,
    pub bodies: Vec<Body>,
    /// Record count of every storage that has a `Header` stream (`Pads6`, `Nets6`, ...).
    pub counts: IndexMap<String, u32>,
    pub warnings: Vec<String>,
    /// Which [`part`]s were decoded.
    pub loaded: u32,
    cf: Compound,
}

pub fn load(path: &Path) -> Result<PcbDoc> {
    load_with(path, &LoadOptions::default())
}

/// Read the zlib-compressed payload of an embedded model stream (`Models/N`).
pub fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| Error::parse(format!("embedded model is not zlib data: {e}")))?;
    Ok(out)
}


/// Decode `FileHeaderSix` / PcbLib `FileHeader`: u32 length + pascal header text, f64 format
/// version, u32 length + pascal unique id (each u32 repeats the pascal length byte). The older
/// PcbDoc `FileHeader` holds a u32 length and a (truncated) UTF-16 string.
pub fn file_header(buf: &[u8]) -> FileInfo {
    let mut fi = FileInfo::default();
    let mut b = Bin::new(buf);
    let Ok(n) = b.u32() else { return fi };
    if buf.get(4).copied() == Some(n as u8) {
        fi.header = b.pascal().unwrap_or_default();
        fi.format_version = b.f64().ok();
        if b.u32().is_ok() {
            fi.unique_id = b.pascal().ok().filter(|u| !u.is_empty());
        }
    } else {
        let units: Vec<u16> = buf[4..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take(n as usize).collect();
        fi.header = String::from_utf16_lossy(&units);
    }
    fi
}

fn header_count(cf: &mut Compound, dir: &str) -> Option<u32> {
    let h = cf.read_opt(&format!("{dir}/Header"))?;
    (h.len() >= 4).then(|| u32::from_le_bytes([h[0], h[1], h[2], h[3]]))
}

pub fn load_with(path: &Path, opt: &LoadOptions) -> Result<PcbDoc> {
    if DocKind::of(path) != DocKind::Pcb {
        return Err(Error::new(ErrorCode::WrongFileType, format!("{} is not a .PcbDoc", path.display()))
            .hint("use `ace pcblib ...` for .PcbLib footprint libraries"));
    }
    load_compound(path, Compound::open(path)?, opt)
}

/// Parse a PcbDoc from bytes (`path` names it in messages); used to verify edits.
pub fn load_bytes(path: &Path, bytes: Vec<u8>, opt: &LoadOptions) -> Result<PcbDoc> {
    load_compound(path, Compound::from_bytes(path, bytes)?, opt)
}

fn load_compound(path: &Path, mut cf: Compound, opt: &LoadOptions) -> Result<PcbDoc> {
    let mut warnings = vec![];
    let raw = opt.keep_raw;
    let want = |p: u32| opt.parts & p != 0;

    let Some(board_buf) = cf.read_opt("Board6/Data") else {
        return Err(Error::parse(format!("{} has no Board6/Data stream", path.display()))
            .hint("save the board as a binary PcbDoc (PCB 6.0) in Altium"));
    };
    let (recs, left) = records::prop_records(&board_buf, 0);
    if left > 0 {
        warnings.push(format!("Board6: {left} trailing bytes"));
    }
    let (board, w) = board::parse_board(recs.into_iter().next().map(|r| r.1).unwrap_or_default());
    warnings.extend(w);

    let mut file = cf
        .read_opt("FileHeaderSix")
        .or_else(|| cf.read_opt("FileHeader"))
        .map(|b| file_header(&b))
        .unwrap_or_default();
    if let Some(v) = cf.read_opt("FileVersionInfo/Data")
        && let Some((_, p)) = records::prop_records(&v, 0).0.into_iter().next() {
            file.release_history = records::file_versions(&p);
            file.saved_with = file.release_history.last().cloned();
        }

    // Header counts of every storage.
    let mut counts = IndexMap::new();
    let mut dirs: Vec<String> =
        cf.streams().into_iter().filter_map(|s| s.path.strip_suffix("/Header").map(str::to_owned)).collect();
    dirs.sort();
    for d in dirs {
        if let Some(n) = header_count(&mut cf, &d) {
            counts.insert(d, n);
        }
    }

    let props_stream = |cf: &mut Compound, name: &str, prefix: usize, warnings: &mut Vec<String>| -> Vec<(u16, Props)> {
        let Some(buf) = cf.read_opt(&format!("{name}/Data")) else { return vec![] };
        let (recs, left) = records::prop_records(&buf, prefix);
        if left > 0 {
            warnings.push(format!("{name}: {left} trailing bytes not decoded"));
        }
        recs
    };

    let nets: Vec<Net> = props_stream(&mut cf, "Nets6", 0, &mut warnings)
        .into_iter()
        .enumerate()
        .map(|(i, (_, p))| records::net(i, p, raw))
        .collect();
    let mut components: Vec<Component> = props_stream(&mut cf, "Components6", 0, &mut warnings)
        .into_iter()
        .enumerate()
        .map(|(i, (_, p))| records::component(i, p, raw))
        .collect();

    let wide = cf.read_opt("WideStrings6/Data").map(|b| altium_format::cfbx::wide_string_table(&b, &mut warnings)).unwrap_or_default();
    let mut texts = cf
        .read_opt("Texts6/Data")
        .map(|b| prim::parse_stream(&b, "Texts6", prim::rec::TEXT, &mut warnings, |b, i| prim::parse_text(b, i, &wide)))
        .unwrap_or_default();

    let bin = |cf: &mut Compound, dir: &str, ty: u8, warnings: &mut Vec<String>, f: &mut dyn FnMut(&mut Bin, usize) -> Result<()>| {
        if let Some(b) = cf.read_opt(&format!("{dir}/Data")) {
            prim::parse_stream(&b, dir, ty, warnings, |b, i| f(b, i));
        }
    };

    let mut pads = vec![];
    if want(part::PADS) {
        bin(&mut cf, "Pads6", prim::rec::PAD, &mut warnings, &mut |b, i| {
            pads.push(prim::parse_pad(b, i)?);
            Ok(())
        });
    }
    let mut vias = vec![];
    if want(part::VIAS) {
        bin(&mut cf, "Vias6", prim::rec::VIA, &mut warnings, &mut |b, i| {
            vias.push(prim::parse_via(b, i)?);
            Ok(())
        });
    }
    let mut tracks = vec![];
    if want(part::TRACKS) {
        bin(&mut cf, "Tracks6", prim::rec::TRACK, &mut warnings, &mut |b, i| {
            tracks.push(prim::parse_track(b, i)?);
            Ok(())
        });
    }
    let mut arcs = vec![];
    if want(part::ARCS) {
        bin(&mut cf, "Arcs6", prim::rec::ARC, &mut warnings, &mut |b, i| {
            arcs.push(prim::parse_arc(b, i)?);
            Ok(())
        });
    }
    let mut fills = vec![];
    if want(part::FILLS) {
        bin(&mut cf, "Fills6", prim::rec::FILL, &mut warnings, &mut |b, i| {
            fills.push(prim::parse_fill(b, i)?);
            Ok(())
        });
    }
    // Regions6 holds every region flattened to straight edges; ShapeBasedRegions6 holds the
    // same regions (same order, layer, net, polygon, kind: checked on every sample) with arc
    // edges. When both are present and agree in count, the shape-based copy is the primary
    // list and the flattened one is kept in `regions_flat`.
    let mut regions = vec![];
    let mut regions_flat = vec![];
    if want(part::REGIONS) {
        let mut parsed: Vec<Vec<Region>> = vec![];
        for (dir, src, ext) in [
            ("Regions6", "regions", false),
            ("ShapeBasedRegions6", "shape_based_regions", true),
            ("BoardRegions", "board_regions", false),
        ] {
            let mut v = vec![];
            bin(&mut cf, dir, prim::rec::REGION, &mut warnings, &mut |b, i| {
                v.push(prim::parse_region(b, i, src, ext, raw)?);
                Ok(())
            });
            parsed.push(v);
        }
        let board_regions = parsed.pop().unwrap_or_default();
        let shaped = parsed.pop().unwrap_or_default();
        let flat = parsed.pop().unwrap_or_default();
        if !shaped.is_empty() && shaped.len() == flat.len() {
            regions = shaped;
            regions_flat = flat;
        } else {
            regions = flat;
            regions.extend(shaped);
        }
        regions.extend(board_regions);
    }
    let mut bodies = vec![];
    if want(part::BODIES) {
        bin(&mut cf, "ComponentBodies6", prim::rec::BODY, &mut warnings, &mut |b, i| {
            bodies.push(prim::parse_body(b, i, raw)?);
            Ok(())
        });
    }

    let polygons: Vec<Polygon> = if want(part::POLYGONS) {
        props_stream(&mut cf, "Polygons6", 0, &mut warnings)
            .into_iter()
            .enumerate()
            .map(|(i, (_, p))| records::polygon(i, p, raw))
            .collect()
    } else {
        vec![]
    };
    let classes: Vec<Class> = if want(part::CLASSES) {
        let mut v: Vec<Class> = props_stream(&mut cf, "Classes6", 0, &mut warnings)
            .into_iter()
            .enumerate()
            .map(|(i, (_, p))| records::class(i, p, "Classes6", raw))
            .collect();
        v.extend(
            props_stream(&mut cf, "SignalClasses", 0, &mut warnings)
                .into_iter()
                .enumerate()
                .map(|(i, (_, p))| records::class(i, p, "SignalClasses", raw)),
        );
        v
    } else {
        vec![]
    };
    let mut rooms = vec![];
    let rules: Vec<Rule> = if want(part::RULES) {
        props_stream(&mut cf, "Rules6", 2, &mut warnings)
            .into_iter()
            .enumerate()
            .map(|(i, (code, p))| {
                let r = records::rule(i, code, p.clone(), raw);
                if r.kind.eq_ignore_ascii_case("RoomDefinition") {
                    rooms.push(records::room(&r, &p));
                }
                r
            })
            .collect()
    } else {
        vec![]
    };
    let dimensions: Vec<Dimension> = if want(part::DIMENSIONS) {
        props_stream(&mut cf, "Dimensions6", 2, &mut warnings)
            .into_iter()
            .enumerate()
            .map(|(i, (_, p))| records::dimension(i, p, raw))
            .collect()
    } else {
        vec![]
    };
    let mut models: Vec<Model> = if want(part::MODELS) || want(part::BODIES) {
        props_stream(&mut cf, "Models", 0, &mut warnings)
            .into_iter()
            .enumerate()
            .map(|(i, (_, p))| records::model(i, p, raw))
            .collect()
    } else {
        vec![]
    };
    let sizes: HashMap<String, u64> = cf.streams().into_iter().map(|s| (s.path, s.size)).collect();
    for m in &mut models {
        let s = format!("Models/{}", m.index);
        if let Some(sz) = sizes.get(&s) {
            m.stream = Some(s);
            m.compressed_size = Some(*sz);
        } else if m.embedded {
            warnings.push(format!("model {} ({}) is marked embedded but stream {s} is missing", m.index, m.name));
        }
    }
    let diff_pairs: Vec<Generic> = if want(part::DIFFPAIRS) {
        props_stream(&mut cf, "DifferentialPairs6", 0, &mut warnings)
            .iter()
            .enumerate()
            .map(|(i, (_, p))| records::generic(i, p))
            .collect()
    } else {
        vec![]
    };

    // Component parameters and the designator/comment texts they own.
    let params = records::primitive_parameters(&props_stream(&mut cf, "PrimitiveParameters", 0, &mut warnings));
    for c in components.iter_mut() {
        if let Some(p) = params.get(&c.unique_id) {
            c.parameters = p.clone();
        }
    }
    for t in &texts {
        let Some(ci) = t.owner.component_index else { continue };
        let Some(c) = components.get_mut(ci as usize) else { continue };
        if t.is_designator && !t.text.is_empty() {
            c.designator = t.text.clone();
        } else if t.is_comment && c.comment.is_empty() {
            c.comment = t.text.clone();
        }
    }
    for c in components.iter_mut() {
        if c.designator.is_empty() {
            c.designator = format!("#{}", c.index);
        }
    }

    let uids = if want(part::UNIQUE_IDS) {
        records::unique_ids(&props_stream(&mut cf, "UniqueIDPrimitiveInformation", 0, &mut warnings))
    } else {
        HashMap::new()
    };

    let mut doc = PcbDoc {
        path: path.to_path_buf(),
        file,
        board,
        nets,
        components,
        classes,
        rules,
        rooms,
        polygons,
        dimensions,
        models,
        diff_pairs,
        pads,
        vias,
        tracks,
        arcs,
        fills,
        texts: vec![],
        regions,
        regions_flat,
        bodies,
        counts,
        warnings,
        loaded: opt.parts | part::PARAMS,
        cf,
    };

    // Resolve owners.
    let net_names: Vec<String> = doc.nets.iter().map(|n| n.name.clone()).collect();
    let designators: Vec<String> = doc.components.iter().map(|c| c.designator.clone()).collect();
    let mut bad_refs = 0usize;
    let mut fix = |o: &mut Owner, kind: &str, index: usize| {
        if let Some(n) = o.net_index {
            match net_names.get(n as usize) {
                Some(name) => o.net = Some(name.clone()),
                None => bad_refs += 1,
            }
        }
        if let Some(c) = o.component_index {
            match designators.get(c as usize) {
                Some(d) => o.component = Some(d.clone()),
                None => bad_refs += 1,
            }
        }
        if let Some(u) = uids.get(&(kind.to_owned(), index)) {
            o.unique_id = Some(u.clone());
        }
    };
    for p in &mut doc.pads {
        fix(&mut p.owner, "pad", p.index);
    }
    for p in &mut doc.vias {
        fix(&mut p.owner, "via", p.index);
    }
    for p in &mut doc.tracks {
        fix(&mut p.owner, "track", p.index);
    }
    for p in &mut doc.arcs {
        fix(&mut p.owner, "arc", p.index);
    }
    for p in &mut doc.fills {
        fix(&mut p.owner, "fill", p.index);
    }
    for p in &mut texts {
        fix(&mut p.owner, "text", p.index);
    }
    for p in doc.regions.iter_mut().chain(doc.regions_flat.iter_mut()) {
        fix(&mut p.owner, "", p.index);
    }
    for p in &mut doc.bodies {
        fix(&mut p.owner, "componentbody", p.index);
    }
    doc.texts = texts;
    for pg in &mut doc.polygons {
        if let Some(n) = pg.net_index {
            pg.net = net_names.get(n as usize).cloned();
        }
        pg.poured_regions = doc.regions.iter().filter(|r| r.polygon == Some(pg.index as u16)).count();
    }
    for m in &mut doc.models {
        m.used_by = doc.bodies.iter().filter(|b| b.model_id == m.id).count();
    }
    if bad_refs > 0 {
        doc.warnings.push(format!("{bad_refs} primitive net/component references point past Nets6/Components6"));
    }

    // Cross-check decoded counts against the Header streams.
    let checks: [(&str, u32, usize); 9] = [
        ("Pads6", part::PADS, doc.pads.len()),
        ("Vias6", part::VIAS, doc.vias.len()),
        ("Tracks6", part::TRACKS, doc.tracks.len()),
        ("Arcs6", part::ARCS, doc.arcs.len()),
        ("Fills6", part::FILLS, doc.fills.len()),
        ("Texts6", u32::MAX, doc.texts.len()),
        ("ComponentBodies6", part::BODIES, doc.bodies.len()),
        ("Nets6", u32::MAX, doc.nets.len()),
        ("Components6", u32::MAX, doc.components.len()),
    ];
    for (dir, p, n) in checks {
        if p != u32::MAX && !want(p) {
            continue;
        }
        if let Some(&h) = doc.counts.get(dir)
            && h as usize != n {
                doc.warnings.push(format!("{dir}: Header says {h} records, decoded {n}"));
            }
    }
    if want(part::REGIONS) {
        for (dir, src) in [("Regions6", "regions"), ("ShapeBasedRegions6", "shape_based_regions"), ("BoardRegions", "board_regions")] {
            let n = doc.regions.iter().chain(&doc.regions_flat).filter(|r| r.source == src).count();
            if let Some(&h) = doc.counts.get(dir)
                && h as usize != n {
                    doc.warnings.push(format!("{dir}: Header says {h} records, decoded {n}"));
                }
        }
    }
    Ok(doc)
}

/// Load everything and convert to the document-independent [`model::Design`].
pub fn load_design(path: &Path) -> Result<model::Design> {
    let doc = load_with(path, &LoadOptions::parts(part::PADS))?;
    Ok(design(&doc))
}

/// One `Part` per component and one `Net` per net that has pads (pins are pad numbers).
pub fn design(doc: &PcbDoc) -> model::Design {
    let file = doc.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let parts = doc
        .components
        .iter()
        .map(|c| model::Part {
            designator: c.designator.clone(),
            lib_ref: c.source_lib_reference.clone(),
            comment: c.comment.clone(),
            description: c.description.clone(),
            footprint: c.footprint.clone(),
            library: c.source_library.clone(),
            parameters: c.parameters.clone(),
            documents: vec![file.clone()],
            unique_id: c.sch_unique_id.clone(),
            units: vec![],
            side: Some(c.side.to_owned()),
            x: Some(c.x),
            y: Some(c.y),
            rotation: Some(c.rotation),
            // Same kinds as the schematic side: Graphical, Net Tie (no BOM), Standard (No BOM), Jumper.
            exclude_from_bom: matches!(c.kind_code, Some(2) | Some(4) | Some(5) | Some(6)),
            kind: c.kind.unwrap_or_default().to_owned(),
        })
        .collect();
    let mut by_net: IndexMap<u16, BTreeSet<model::Node>> = IndexMap::new();
    for p in &doc.pads {
        let (Some(n), Some(d)) = (p.owner.net_index, &p.owner.component) else { continue };
        by_net.entry(n).or_default().insert(model::Node { designator: d.clone(), pin: p.number.clone(), pin_name: String::new() });
    }
    let nets = doc
        .nets
        .iter()
        .enumerate()
        .map(|(i, n)| model::Net {
            name: n.name.clone(),
            nodes: by_net.get(&(i as u16)).map(|s| s.iter().cloned().collect()).unwrap_or_default(),
            auto_named: false,
            name_source: "pcb".into(),
        })
        .collect();
    let mut warnings = doc.warnings.clone();
    if doc.loaded & part::PADS == 0 {
        warnings.push("pads were not loaded: nets have no nodes".into());
    }
    model::Design { source: "pcb".into(), parts, nets, warnings }
}

impl PcbDoc {
    /// Read a stream of the underlying compound file (e.g. an embedded model).
    pub fn read_stream(&mut self, name: &str) -> Result<Vec<u8>> {
        self.cf.read(name)
    }

    /// Decompressed STEP data of model `index`.
    pub fn model_data(&mut self, index: usize) -> Result<Vec<u8>> {
        let m = self.models.get(index).ok_or_else(|| Error::not_found(format!("no model {index}")))?;
        let s = m.stream.clone().ok_or_else(|| {
            Error::not_found(format!("model {index} ({}) has no embedded data", m.name))
                .hint("only models with embedded=true carry STEP data in the PcbDoc")
        })?;
        inflate(&self.cf.read(&s)?)
    }

    /// Find a component by designator (case-insensitive) or `#index`.
    pub fn component(&self, sel: &str) -> Result<&Component> {
        if let Some(c) = self.components.iter().find(|c| c.designator == sel) {
            return Ok(c);
        }
        if let Some(c) = self.components.iter().find(|c| c.designator.eq_ignore_ascii_case(sel)) {
            return Ok(c);
        }
        if let Some(i) = sel.strip_prefix('#').and_then(|s| s.parse::<usize>().ok())
            && let Some(c) = self.components.get(i) {
                return Ok(c);
            }
        let mut close: Vec<&str> = self
            .components
            .iter()
            .map(|c| c.designator.as_str())
            .filter(|d| d.to_ascii_lowercase().starts_with(&sel.chars().take(1).collect::<String>().to_ascii_lowercase()))
            .take(20)
            .collect();
        close.sort();
        Err(Error::not_found(format!("no component {sel:?}"))
            .hint(format!("see `ace pcb components`; similar: {close:?}")))
    }

    pub fn net(&self, name: &str) -> Result<&Net> {
        self.nets
            .iter()
            .find(|n| n.name == name)
            .or_else(|| self.nets.iter().find(|n| n.name.eq_ignore_ascii_case(name)))
            .ok_or_else(|| {
                Error::not_found(format!("no net {name:?}")).hint("see `ace pcb nets` for the net names")
            })
    }

    /// Name of every layer that has at least one decoded primitive on it.
    pub fn layer_names(&self) -> Vec<String> {
        self.board.layers.iter().map(|l| l.layer.name()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_string_table_with_empty_entry() {
        // index 0 = "AB", index 1 = empty (length 2, no bytes stored), index 2 = "C".
        let mut b = vec![];
        b.extend(0u32.to_le_bytes());
        b.extend(6u32.to_le_bytes());
        b.extend([b'A', 0, b'B', 0, 0, 0]);
        b.extend(1u32.to_le_bytes());
        b.extend(2u32.to_le_bytes());
        b.extend(2u32.to_le_bytes());
        b.extend(4u32.to_le_bytes());
        b.extend([b'C', 0, 0, 0]);
        let mut w = vec![];
        let t = altium_format::cfbx::wide_string_table(&b, &mut w);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(t[&0], "AB");
        assert_eq!(t[&1], "");
        assert_eq!(t[&2], "C");
    }

    #[test]
    fn headers() {
        // FileHeaderSix of PilotPi_Power.PcbDoc.
        let mut b = vec![0x13, 0, 0, 0, 0x13];
        b.extend(b"PCB 6.0 Binary File");
        b.extend(5.01f64.to_le_bytes());
        b.extend([4, 0, 0, 0, 4]);
        b.extend(b"ABCD");
        let f = file_header(&b);
        assert_eq!(f.header, "PCB 6.0 Binary File");
        assert_eq!(f.format_version, Some(5.01));
        assert_eq!(f.unique_id.as_deref(), Some("ABCD"));
        // Legacy FileHeader (truncated UTF-16).
        let mut b = vec![0x13, 0, 0, 0];
        for c in "PCB 5.0 Bi".encode_utf16() {
            b.extend(c.to_le_bytes());
        }
        assert_eq!(file_header(&b).header, "PCB 5.0 Bi");
    }

    #[test]
    fn inflate_roundtrip() {
        use std::io::Write;
        let mut e = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
        e.write_all(b"ISO-10303-21;").unwrap();
        assert_eq!(inflate(&e.finish().unwrap()).unwrap(), b"ISO-10303-21;");
        assert!(inflate(b"not zlib").is_err());
    }
}
