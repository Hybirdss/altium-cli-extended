//! `.SchDoc` loading: compound streams → records → typed objects.
//!
//! Layout:
//! - `FileHeader`: a header property list (`HEADER=Protel for Windows - Schematic Capture
//!   Binary File Version 5.0`, `WEIGHT` = number of records), then one record per object.
//!   Records are numbered from 0 after the header; `OWNERINDEX` refers to that number, and
//!   children follow their owner (depth first). Record 0 is the sheet (31).
//! - `Additional`: same header, then extra records (harness connectors/entries/types, signal
//!   harnesses). They are numbered here after the FileHeader records.
//! - `Storage`: `HEADER=Icon storage` + `WEIGHT`, then binary records (0xD0, file name, zlib
//!   data) holding embedded images.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_core::units::{Coord, Point};
use altium_format::DocKind;
use altium_format::cfbx::{Compound, Record, split_records};
use altium_format::props::Props;

use super::prim::{self, Model, Parameter, Pin, PinMap};
use super::records as rt;
use super::storage::{self, StoredFile};

pub const SCHDOC_HEADER: &str = "Protel for Windows - Schematic Capture Binary File Version 5.0";

/// What to decode.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadOptions {
    /// Keep each object's raw property map (`raw` fields) for `--raw` output.
    pub keep_raw: bool,
}

fn skip_false(b: &bool) -> bool {
    !*b
}

/// One record of the document, as stored.
#[derive(Debug, Clone)]
pub struct SchRecord {
    /// 0-based record number (the `OWNERINDEX` space). Additional-stream records continue
    /// after the FileHeader records.
    pub index: usize,
    /// `FileHeader` or `Additional`.
    pub stream: &'static str,
    /// `RECORD=` value (-1 when missing).
    pub record: i64,
    pub props: Props,
    /// True for non-text (binary) records.
    pub binary: bool,
}

impl SchRecord {
    pub fn type_name(&self) -> &'static str {
        rt::type_name(self.record)
    }
    pub fn owner(&self) -> Option<usize> {
        rt::owner_index(&self.props)
    }
}

/// Font table entry of the sheet (`FONTIDCOUNT`, `FONTNAMEn`, `SIZEn`...).
#[derive(Debug, Clone, Serialize)]
pub struct Font {
    pub id: i64,
    pub name: String,
    /// Line spacing in points as stored (`SIZEn`).
    pub size: i64,
    #[serde(skip_serializing_if = "skip_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "skip_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "skip_false")]
    pub underline: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub rotation: i64,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

/// Sheet settings (record 31).
#[derive(Debug, Clone, Serialize)]
pub struct Sheet {
    pub index: usize,
    /// `A4`, `Letter`, ... or `Custom`.
    pub size: String,
    pub width: Coord,
    pub height: Coord,
    /// `landscape` or `portrait`.
    pub orientation: String,
    pub custom: bool,
    pub border: bool,
    pub title_block: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_grid: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_grid: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    pub fonts: Vec<Font>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_font: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

/// A placed component (one record 1 = one sub-part instance).
#[derive(Debug, Clone, Serialize)]
pub struct Component {
    /// Record index (stable reference).
    pub index: usize,
    /// Designator as stored (without the sub-part letter), e.g. `U1`.
    pub designator: String,
    /// Sub-part letter for multi-part symbols (`A`, `B`...), else omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    /// `CURRENTPARTID` (1-based).
    pub part_id: i64,
    /// Number of sub-parts in the symbol (`PARTCOUNT - 1`).
    pub part_count: i64,
    /// Designator as Altium shows it (`U1A` for sub-part A of a multi-part symbol).
    pub display_designator: String,
    pub lib_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_item_id: Option<String>,
    /// `Comment` parameter (references like `=Value` resolved).
    pub comment: String,
    pub description: String,
    /// Current PCB footprint model name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footprint: Option<String>,
    /// Source library (`SOURCELIBRARYNAME`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_table: Option<String>,
    /// `Standard`, `Mechanical`, `Graphical`, `Net Tie`, `Standard (No BOM)`...
    pub kind: String,
    pub location: Point,
    /// Rotation in degrees (0, 90, 180, 270).
    pub orientation: f64,
    pub mirrored: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub display_mode: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_mode_count: Option<i64>,
    pub unique_id: String,
    /// User and system parameters of this instance (records 41 owned by the component).
    pub parameters: Vec<Parameter>,
    /// Footprint / simulation / SI / 3D models (records 45 under the implementation list).
    pub models: Vec<Model>,
    /// Pins of the current sub-part and display mode (pins of other sub-parts are skipped).
    pub pins: Vec<Pin>,
    /// Record index of the designator (record 34), when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub designator_record: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

impl Component {
    pub fn param(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }
    /// Graphical components carry no electrical meaning (no pins in the netlist).
    pub fn is_graphical(&self) -> bool {
        self.kind == "Graphical"
    }
    pub fn exclude_from_bom(&self) -> bool {
        // Mechanical parts (heatsinks, screws) are in Altium's BOM; they only skip the netlist.
        matches!(self.kind.as_str(), "Graphical" | "Net Tie" | "Standard (No BOM)" | "Jumper")
    }
    /// Footprint pads a schematic pin maps to: the current footprint model's map definer
    /// (record 47) when it lists the pin, else the pin designator itself. Verified on the
    /// samples: `DESINTF=5V|DESIMP0=7` puts pad 7 of FAN0 on net 5V in the PCB.
    pub fn pads_for(&self, pin: &str) -> Vec<String> {
        if let Some(m) = super::prim::current_footprint(&self.models)
            && let Some(map) = m.pin_map.iter().find(|x| x.pin == pin)
            && !map.pads.is_empty()
        {
            return map.pads.clone();
        }
        vec![pin.to_owned()]
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Wire {
    pub index: usize,
    pub points: Vec<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BusEntry {
    pub index: usize,
    pub location: Point,
    pub corner: Point,
    /// Record index of the bus an end of this entry touches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bus: Option<usize>,
    /// Net of the wire touching the other end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bus {
    pub index: usize,
    pub points: Vec<Point>,
    /// Net labels sitting on this bus (bus names such as `D[0..7]`).
    pub labels: Vec<String>,
    /// Bus entries touching it (record indices).
    pub entries: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Junction {
    pub index: usize,
    pub location: Point,
    #[serde(skip_serializing_if = "skip_false")]
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetLabel {
    pub index: usize,
    pub name: String,
    pub location: Point,
    pub orientation: f64,
    /// Sits on a bus rather than a wire (a bus name, not a net name).
    #[serde(skip_serializing_if = "skip_false")]
    pub on_bus: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    /// Net it ended up naming / joining (may differ when another name takes priority).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PowerPort {
    pub index: usize,
    /// Net name (`TEXT`).
    pub name: String,
    /// `circle`, `arrow`, `bar`, `wave`, `power_ground`, `signal_ground`, `earth`, `gost_*`.
    pub style: String,
    pub show_net_name: bool,
    #[serde(skip_serializing_if = "skip_false")]
    pub cross_sheet_connector: bool,
    pub location: Point,
    pub orientation: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Port {
    pub index: usize,
    pub name: String,
    /// `unspecified`, `output`, `input`, `bidirectional`.
    pub io_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    pub location: Point,
    pub width: Coord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<Coord>,
    /// The two connection points (left/right or bottom/top end).
    pub ends: [Point; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness_type: Option<String>,
    #[serde(skip_serializing_if = "skip_false")]
    pub on_bus: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SheetEntry {
    pub index: usize,
    pub name: String,
    pub io_type: String,
    /// `left`, `right`, `top`, `bottom`.
    pub side: String,
    /// Offset along the side from the symbol's top-left corner.
    pub distance: Coord,
    /// Connection point on the symbol edge.
    pub location: Point,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness_type: Option<String>,
    #[serde(skip_serializing_if = "skip_false")]
    pub on_bus: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
}

/// `Repeat(name, first, last)` in a sheet symbol's name: a multi-channel sheet.
#[derive(Debug, Clone, Serialize)]
pub struct Repeat {
    pub name: String,
    pub first: i64,
    pub last: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SheetSymbol {
    pub index: usize,
    /// Sheet symbol designator (record 32).
    pub sheet_name: String,
    /// Child document (record 33), as written.
    pub file_name: String,
    /// Top-left corner.
    pub location: Point,
    pub width: Coord,
    pub height: Coord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    pub entries: Vec<SheetEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Repeat>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<Parameter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoErc {
    pub index: usize,
    pub location: Point,
    pub active: bool,
    pub suppress_all: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<f64>,
    /// Net at its location, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
}

/// Free text: `label` (4), `text_frame` (28), `note` (209) or `hyperlink` (226).
#[derive(Debug, Clone, Serialize)]
pub struct Text {
    pub index: usize,
    pub kind: String,
    pub text: String,
    pub location: Point,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corner: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<f64>,
    /// Designator of the owning component (text drawn as part of a symbol).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Image {
    pub index: usize,
    pub file_name: String,
    pub embedded: bool,
    pub keep_aspect: bool,
    pub location: Point,
    pub corner: Point,
    /// Matching Storage entry (embedded images).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

/// Directive-like objects: parameter sets (43, e.g. net class / differential pair / PCB rule
/// directives), compile masks (211) and blankets (225).
#[derive(Debug, Clone, Serialize)]
pub struct Directive {
    pub index: usize,
    /// `parameter_set`, `compile_mask`, `blanket`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corner: Option<Point>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<f64>,
    /// Child parameters (name → value), e.g. a directive's `ClassName` or rule.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub parameters: IndexMap<String, String>,
    /// Net the directive's location touches (parameter sets placed on a wire).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HarnessEntry {
    pub index: usize,
    pub name: String,
    pub side: String,
    pub distance: Coord,
}

#[derive(Debug, Clone, Serialize)]
pub struct HarnessConnector {
    pub index: usize,
    pub location: Point,
    pub width: Coord,
    pub height: Coord,
    pub side: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness_type: Option<String>,
    pub entries: Vec<HarnessEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignalHarness {
    pub index: usize,
    pub points: Vec<Point>,
}

/// A parsed `.SchDoc`.
pub struct SchDoc {
    pub path: PathBuf,
    /// The FileHeader header property list.
    pub header: Props,
    /// Every record (FileHeader then Additional).
    pub records: Vec<SchRecord>,
    pub sheet: Option<Sheet>,
    /// Document parameters (title block fields: `Title`, `Revision`, `DrawnBy`...).
    pub parameters: Vec<Parameter>,
    pub components: Vec<Component>,
    pub wires: Vec<Wire>,
    pub buses: Vec<Bus>,
    pub bus_entries: Vec<BusEntry>,
    pub junctions: Vec<Junction>,
    pub net_labels: Vec<NetLabel>,
    pub power_ports: Vec<PowerPort>,
    pub ports: Vec<Port>,
    pub sheet_symbols: Vec<SheetSymbol>,
    pub no_ercs: Vec<NoErc>,
    pub texts: Vec<Text>,
    pub images: Vec<Image>,
    pub directives: Vec<Directive>,
    pub harness_connectors: Vec<HarnessConnector>,
    pub signal_harnesses: Vec<SignalHarness>,
    /// Template file names (record 39).
    pub templates: Vec<String>,
    /// Embedded files from the Storage stream.
    pub storage: Vec<StoredFile>,
    pub warnings: Vec<String>,
    /// Original file bytes and FileHeader records, for byte-preserving edits.
    pub(crate) bytes: Vec<u8>,
    pub(crate) fh_records: Vec<Record>,
    pub(crate) options: LoadOptions,
}

pub fn load(path: &Path) -> Result<SchDoc> {
    load_with(path, &LoadOptions::default())
}

pub fn load_with(path: &Path, opts: &LoadOptions) -> Result<SchDoc> {
    if DocKind::of(path) != DocKind::Schematic {
        return Err(Error::new(ErrorCode::WrongFileType, format!("{} is not a .SchDoc", path.display()))
            .hint("schematic libraries use `ace schlib ...`; projects use `ace project ...`"));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| Error::from(e).hint(format!("check the path: {}", path.display())))?;
    load_bytes(path, bytes, opts)
}

/// Parse a schematic from memory (used to verify edits before writing).
pub fn load_bytes(path: &Path, bytes: Vec<u8>, opts: &LoadOptions) -> Result<SchDoc> {
    let mut cf = Compound::from_bytes(path, bytes.clone())?;
    let fh = cf.read("FileHeader").map_err(|e| {
        Error::parse(format!("{}: no FileHeader stream ({})", path.display(), e.message))
            .hint("is this a schematic saved by Altium Designer (binary format)?")
    })?;
    let mut warnings = Vec::new();
    let (fh_records, left) = split_records(&fh);
    if left > 0 {
        warnings.push(format!("FileHeader: {left} trailing bytes do not form a complete record"));
    }
    let Some(first) = fh_records.first() else {
        return Err(Error::parse(format!("{}: FileHeader is empty", path.display())));
    };
    let header = first.props();
    let htext = header.str("HEADER");
    if !htext.eq_ignore_ascii_case(SCHDOC_HEADER) {
        if htext.to_ascii_lowercase().contains("library") {
            return Err(Error::new(ErrorCode::WrongFileType, format!("{} is a schematic library ({htext})", path.display()))
                .hint("use `ace schlib ...`"));
        }
        warnings.push(format!("unexpected FileHeader header {htext:?}; parsing anyway"));
    }
    let mut records: Vec<SchRecord> = Vec::new();
    for r in &fh_records[1..] {
        let props = if r.is_text() { r.props() } else { Props::new() };
        records.push(SchRecord {
            index: records.len(),
            stream: "FileHeader",
            record: props.int("RECORD").unwrap_or(-1),
            props,
            binary: !r.is_text(),
        });
    }
    if let Some(w) = header.int("WEIGHT")
        && w as usize != records.len()
    {
        warnings.push(format!("header WEIGHT={w} but FileHeader holds {} records", records.len()));
    }
    if let Some(add) = cf.read_opt("Additional") {
        let (recs, left) = split_records(&add);
        if left > 0 {
            warnings.push(format!("Additional: {left} trailing bytes do not form a complete record"));
        }
        for r in recs.iter().skip(1) {
            let props = if r.is_text() { r.props() } else { Props::new() };
            records.push(SchRecord {
                index: records.len(),
                stream: "Additional",
                record: props.int("RECORD").unwrap_or(-1),
                props,
                binary: !r.is_text(),
            });
        }
    }
    let storage = match cf.read_opt("Storage") {
        Some(buf) => storage::parse(&buf, &mut warnings),
        None => vec![],
    };
    let mut doc = SchDoc {
        path: path.to_path_buf(),
        header,
        records,
        sheet: None,
        parameters: vec![],
        components: vec![],
        wires: vec![],
        buses: vec![],
        bus_entries: vec![],
        junctions: vec![],
        net_labels: vec![],
        power_ports: vec![],
        ports: vec![],
        sheet_symbols: vec![],
        no_ercs: vec![],
        texts: vec![],
        images: vec![],
        directives: vec![],
        harness_connectors: vec![],
        signal_harnesses: vec![],
        templates: vec![],
        storage,
        warnings,
        bytes,
        fh_records,
        options: *opts,
    };
    build(&mut doc);
    super::connect::annotate(&mut doc, &super::connect::NetOptions::default());
    Ok(doc)
}

/// Owner → children (record indices, in file order).
pub(crate) fn children_map(records: &[SchRecord]) -> HashMap<usize, Vec<usize>> {
    let mut m: HashMap<usize, Vec<usize>> = HashMap::new();
    for r in records {
        // Harness entries/types point into the Additional list (OWNERINDEXADDITIONALLIST);
        // they are attached to their connector by order instead (see `build`).
        if r.stream == "Additional" {
            continue;
        }
        if let Some(o) = r.owner() {
            m.entry(o).or_default().push(r.index);
        }
    }
    m
}

fn opt_str(p: &Props, key: &str) -> Option<String> {
    p.get(key).filter(|s| !s.is_empty() && s != "*")
}

fn parse_repeat(name: &str) -> Option<Repeat> {
    let s = name.trim();
    s.get(..7).filter(|h| h.eq_ignore_ascii_case("repeat("))?;
    let body = s[7..].strip_suffix(')')?;
    let parts: Vec<&str> = body.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }
    Some(Repeat { name: parts[0].to_owned(), first: parts[1].parse().ok()?, last: parts[2].parse().ok()? })
}

fn build(doc: &mut SchDoc) {
    let keep = doc.options.keep_raw;
    let children = children_map(&doc.records);
    let kids = |i: usize| children.get(&i).cloned().unwrap_or_default();
    let recs = &doc.records;
    let rec_type = |i: usize| recs.get(i).map(|r| r.record).unwrap_or(-1);

    // Pass 1: document parameters and the sheet.
    for r in recs {
        let p = &r.props;
        match r.record {
            31 => {
                let style = p.int_or("SHEETSTYLE", 0);
                let custom = p.bool("USECUSTOMSHEET");
                let portrait = p.int_or("WORKSPACEORIENTATION", 0) == 1;
                let (name, mut w, mut h) = rt::sheet_style(style).unwrap_or(("A4", 1150, 760));
                if portrait {
                    std::mem::swap(&mut w, &mut h);
                }
                let (size, width, height) = if custom {
                    ("Custom".to_owned(), p.sch("CUSTOMX"), p.sch("CUSTOMY"))
                } else {
                    (name.to_owned(), Coord::from_sch(w, 0), Coord::from_sch(h, 0))
                };
                let n = p.int_or("FONTIDCOUNT", 0);
                let fonts = (1..=n)
                    .map(|i| Font {
                        id: i,
                        name: p.str(&format!("FONTNAME{i}")),
                        size: p.int_or(&format!("SIZE{i}"), 0),
                        bold: p.bool(&format!("BOLD{i}")),
                        italic: p.bool(&format!("ITALIC{i}")),
                        underline: p.bool(&format!("UNDERLINE{i}")),
                        rotation: p.int_or(&format!("ROTATION{i}"), 0),
                    })
                    .collect();
                if doc.sheet.is_none() {
                    doc.sheet = Some(Sheet {
                        index: r.index,
                        size,
                        width,
                        height,
                        orientation: if portrait { "portrait" } else { "landscape" }.into(),
                        custom,
                        border: p.bool("BORDERON"),
                        title_block: p.bool("TITLEBLOCKON"),
                        snap_grid: p.bool("SNAPGRIDON").then(|| p.sch("SNAPGRIDSIZE")),
                        visible_grid: p.bool("VISIBLEGRIDON").then(|| p.sch("VISIBLEGRIDSIZE")),
                        template: opt_str(p, "TEMPLATEFILENAME"),
                        fonts,
                        system_font: p.int("SYSTEMFONT"),
                        raw: keep.then(|| p.clone()),
                    });
                }
            }
            41 => {
                let owner = r.owner();
                let doc_level = owner.is_none() || owner.is_some_and(|o| rec_type(o) == 31);
                if doc_level {
                    doc.parameters.push(Parameter::from_props(r.index, p, keep));
                }
            }
            39 => doc.templates.push(p.str("FILENAME")),
            _ => {}
        }
    }
    let doc_params_snapshot = doc.parameters.clone();
    prim::resolve_params(&mut doc.parameters, &[]);

    // Designators of components, for owner names on texts and images.
    let mut comp_designator: HashMap<usize, String> = HashMap::new();
    // Component record → (current part id, display mode), to skip other sub-parts' graphics.
    let mut comp_part: HashMap<usize, (i64, i64)> = HashMap::new();

    // Pass 2: components with their children.
    for r in recs.iter().filter(|r| r.record == 1) {
        let p = &r.props;
        let mut c = Component {
            index: r.index,
            designator: String::new(),
            part: None,
            part_id: p.int_or("CURRENTPARTID", 1),
            part_count: (p.int_or("PARTCOUNT", 2) - 1).max(1),
            display_designator: String::new(),
            lib_ref: p.str("LIBREFERENCE"),
            design_item_id: opt_str(p, "DESIGNITEMID"),
            comment: String::new(),
            description: p.str("COMPONENTDESCRIPTION"),
            footprint: None,
            library: opt_str(p, "SOURCELIBRARYNAME"),
            library_path: opt_str(p, "LIBRARYPATH"),
            database_table: opt_str(p, "DATABASETABLENAME"),
            kind: rt::component_kind(p.int_or("COMPONENTKIND", 0)).to_owned(),
            location: rt::location(p),
            orientation: rt::orientation_deg(p.int_or("ORIENTATION", 0)),
            mirrored: p.bool("ISMIRRORED"),
            display_mode: p.int_or("DISPLAYMODE", 0),
            display_mode_count: p.int("DISPLAYMODECOUNT"),
            unique_id: p.str("UNIQUEID"),
            parameters: vec![],
            models: vec![],
            pins: vec![],
            designator_record: None,
            raw: keep.then(|| p.clone()),
        };
        for k in kids(r.index) {
            let kr = &recs[k];
            match kr.record {
                34 => {
                    c.designator = kr.props.str("TEXT");
                    c.designator_record = Some(k);
                }
                41 => c.parameters.push(Parameter::from_props(k, &kr.props, keep)),
                2 => {
                    let mut pin = Pin::from_props(k, &kr.props, keep);
                    let active_part = pin.part_id <= 0 || pin.part_id == c.part_id;
                    if !active_part || pin.display_mode != c.display_mode {
                        continue;
                    }
                    for pk in kids(k) {
                        let pp = &recs[pk].props;
                        if recs[pk].record == 41 && pp.str("NAME").eq_ignore_ascii_case("PinUniqueId") {
                            pin.unique_id = opt_str(pp, "TEXT");
                        }
                    }
                    c.pins.push(pin);
                }
                44 => {
                    for mk in kids(k).into_iter().filter(|&m| rec_type(m) == 45) {
                        let mut model = Model::from_props(mk, &recs[mk].props, keep);
                        for sub in kids(mk) {
                            match rec_type(sub) {
                                46 => {
                                    for md in kids(sub).into_iter().filter(|&d| rec_type(d) == 47) {
                                        let dp = &recs[md].props;
                                        let n = dp.int_or("DESIMPCOUNT", 0).max(0);
                                        model.pin_map.push(PinMap {
                                            pin: dp.str("DESINTF"),
                                            pads: (0..n).map(|i| dp.str(&format!("DESIMP{i}"))).collect(),
                                        });
                                    }
                                }
                                48 => {
                                    for pk in kids(sub).into_iter().filter(|&d| rec_type(d) == 41) {
                                        let pp = &recs[pk].props;
                                        model.parameters.insert(pp.str("NAME"), pp.str("TEXT"));
                                    }
                                }
                                _ => {}
                            }
                        }
                        c.models.push(model);
                    }
                }
                _ => {}
            }
        }
        prim::resolve_params(&mut c.parameters, &doc_params_snapshot);
        c.comment = c.param("Comment").map(|p| p.effective().to_owned()).unwrap_or_default();
        c.footprint = prim::current_footprint(&c.models).map(|m| m.name.clone());
        if c.part_count > 1 {
            c.part = Some(rt::part_letter(c.part_id));
        }
        c.display_designator = format!("{}{}", c.designator, c.part.as_deref().unwrap_or(""));
        if c.designator.is_empty() {
            doc.warnings.push(format!("component at record {} ({}) has no designator", c.index, c.lib_ref));
        }
        comp_designator.insert(r.index, c.display_designator.clone());
        comp_part.insert(r.index, (c.part_id, c.display_mode));
        doc.components.push(c);
    }

    // Owner chain → owning component designator.
    let owner_comp = |mut i: usize| -> Option<String> {
        for _ in 0..8 {
            let o = recs.get(i)?.owner()?;
            if let Some(d) = comp_designator.get(&o) {
                return Some(d.clone());
            }
            i = o;
        }
        None
    };

    // Pass 3: everything else.
    let mut harness_current: Option<usize> = None;
    for r in recs {
        let p = &r.props;
        let i = r.index;
        match r.record {
            27 => doc.wires.push(Wire { index: i, points: rt::points(p), unique_id: opt_str(p, "UNIQUEID"), net: None }),
            26 => doc.buses.push(Bus {
                index: i,
                points: rt::points(p),
                labels: vec![],
                entries: vec![],
                unique_id: opt_str(p, "UNIQUEID"),
            }),
            37 => doc.bus_entries.push(BusEntry { index: i, location: rt::location(p), corner: rt::corner(p), bus: None, net: None }),
            29 => doc.junctions.push(Junction { index: i, location: rt::location(p), locked: p.bool("LOCKED"), net: None }),
            25 => doc.net_labels.push(NetLabel {
                index: i,
                name: p.str("TEXT"),
                location: rt::location(p),
                orientation: rt::orientation_deg(p.int_or("ORIENTATION", 0)),
                on_bus: false,
                unique_id: opt_str(p, "UNIQUEID"),
                net: None,
            }),
            17 => {
                let style = p.int_or("STYLE", 0);
                doc.power_ports.push(PowerPort {
                    index: i,
                    name: p.str("TEXT"),
                    style: rt::power_style(style).map(str::to_owned).unwrap_or_else(|| format!("unknown_{style}")),
                    show_net_name: p.bool("SHOWNETNAME"),
                    cross_sheet_connector: p.bool("ISCROSSSHEETCONNECTOR"),
                    location: rt::location(p),
                    orientation: rt::orientation_deg(p.int_or("ORIENTATION", 0)),
                    unique_id: opt_str(p, "UNIQUEID"),
                    net: None,
                });
            }
            18 => {
                // Connection points: horizontal styles extend WIDTH to
                // the right of LOCATION, vertical styles extend WIDTH upwards.
                let loc = rt::location(p);
                let style = p.int_or("STYLE", 0);
                let width = p.sch("WIDTH");
                let end = if (4..=7).contains(&style) {
                    Point::new(loc.x, loc.y + width)
                } else {
                    Point::new(loc.x + width, loc.y)
                };
                doc.ports.push(Port {
                    index: i,
                    name: p.str("NAME"),
                    io_type: rt::io_type(p.int_or("IOTYPE", 0)).into(),
                    style: rt::port_style(style).map(str::to_owned),
                    location: loc,
                    width,
                    height: p.has("HEIGHT").then(|| p.sch("HEIGHT")),
                    ends: [loc, end],
                    harness_type: opt_str(p, "HARNESSTYPE"),
                    on_bus: false,
                    unique_id: opt_str(p, "UNIQUEID"),
                    net: None,
                });
            }
            15 => {
                let loc = rt::location(p);
                let (w, h) = (p.sch("XSIZE"), p.sch("YSIZE"));
                let mut s = SheetSymbol {
                    index: i,
                    sheet_name: String::new(),
                    file_name: String::new(),
                    location: loc,
                    width: w,
                    height: h,
                    symbol_type: opt_str(p, "SYMBOLTYPE"),
                    unique_id: opt_str(p, "UNIQUEID"),
                    entries: vec![],
                    repeat: None,
                    parameters: vec![],
                    raw: keep.then(|| p.clone()),
                };
                for k in kids(i) {
                    let kp = &recs[k].props;
                    match rec_type(k) {
                        32 => s.sheet_name = kp.str("TEXT"),
                        33 => s.file_name = kp.str("TEXT"),
                        41 => s.parameters.push(Parameter::from_props(k, kp, keep)),
                        16 => {
                            // Entry position: DISTANCEFROMTOP is measured
                            // down from the top-left corner on the left/right sides and right
                            // from it on the top/bottom sides.
                            let side = kp.int_or("SIDE", 0);
                            let d = rt::distance_from_top(kp);
                            let location = match side {
                                1 => Point::new(loc.x + w, loc.y - d),
                                2 => Point::new(loc.x + d, loc.y),
                                3 => Point::new(loc.x + d, loc.y - h),
                                _ => Point::new(loc.x, loc.y - d),
                            };
                            s.entries.push(SheetEntry {
                                index: k,
                                name: kp.str("NAME"),
                                io_type: rt::io_type(kp.int_or("IOTYPE", 0)).into(),
                                side: rt::side(side).into(),
                                distance: d,
                                location,
                                style: kp.int("STYLE").and_then(rt::port_style).map(str::to_owned),
                                harness_type: opt_str(kp, "HARNESSTYPE"),
                                on_bus: false,
                                unique_id: opt_str(kp, "UNIQUEID"),
                                net: None,
                            });
                        }
                        _ => {}
                    }
                }
                s.repeat = parse_repeat(&s.sheet_name);
                prim::resolve_params(&mut s.parameters, &doc_params_snapshot);
                if s.file_name.is_empty() {
                    doc.warnings.push(format!("sheet symbol at record {i} has no file name"));
                }
                doc.sheet_symbols.push(s);
            }
            22 => doc.no_ercs.push(NoErc {
                index: i,
                location: rt::location(p),
                active: !p.has("ISACTIVE") || p.bool("ISACTIVE"),
                suppress_all: !p.has("SUPPRESSALL") || p.bool("SUPPRESSALL"),
                symbol: opt_str(p, "SYMBOL"),
                orientation: p.int("ORIENTATION").map(rt::orientation_deg),
                net: None,
            }),
            // Symbol text / images of a sub-part or display mode that is not the placed one
            // are not shown.
            4 | 28 | 209 | 226 | 30
                if r.owner().and_then(|o| comp_part.get(&o)).is_some_and(|&(part, mode)| {
                    let pp = p.int_or("OWNERPARTID", -1);
                    (pp > 0 && pp != part) || p.int_or("OWNERPARTDISPLAYMODE", 0) != mode
                }) => {}
            4 | 28 | 209 | 226 => doc.texts.push(Text {
                index: i,
                kind: rt::type_name(r.record).into(),
                text: if matches!(r.record, 28 | 209) { rt::frame_text(&p.str("TEXT")) } else { p.str("TEXT") },
                location: rt::location(p),
                corner: matches!(r.record, 28 | 209).then(|| rt::corner(p)),
                orientation: p.int("ORIENTATION").map(rt::orientation_deg),
                owner: owner_comp(i),
                author: opt_str(p, "AUTHOR"),
                url: opt_str(p, "URL"),
            }),
            30 => {
                let file_name = p.str("FILENAME");
                let embedded = p.bool("EMBEDIMAGE");
                let storage_name = embedded.then(|| storage::find(&doc.storage, &file_name).map(|s| s.name.clone())).flatten();
                if embedded && storage_name.is_none() {
                    doc.warnings.push(format!("image {i}: embedded file {file_name:?} not found in Storage"));
                }
                doc.images.push(Image {
                    index: i,
                    file_name,
                    embedded,
                    keep_aspect: p.bool("KEEPASPECT"),
                    location: rt::location(p),
                    corner: rt::corner(p),
                    storage_name,
                    owner: owner_comp(i),
                });
            }
            43 | 211 | 225 => {
                let mut parameters = IndexMap::new();
                for k in kids(i).into_iter().filter(|&k| rec_type(k) == 41) {
                    let kp = &recs[k].props;
                    parameters.insert(kp.str("NAME"), kp.str("TEXT"));
                }
                let has_loc = p.has("LOCATION.X") || p.has("LOCATION.Y");
                let has_corner = p.has("CORNER.X") || p.has("CORNER.Y");
                doc.directives.push(Directive {
                    index: i,
                    kind: rt::type_name(r.record).into(),
                    name: opt_str(p, "NAME"),
                    location: has_loc.then(|| rt::location(p)),
                    corner: has_corner.then(|| rt::corner(p)),
                    points: rt::points(p),
                    orientation: p.int("ORIENTATION").map(rt::orientation_deg),
                    parameters,
                    net: None,
                    raw: keep.then(|| p.clone()),
                });
            }
            215 => {
                doc.harness_connectors.push(HarnessConnector {
                    index: i,
                    location: rt::location(p),
                    width: p.sch("XSIZE"),
                    height: p.sch("YSIZE"),
                    side: rt::side(p.int_or("SIDE", 1)).into(),
                    harness_type: None,
                    entries: vec![],
                });
                harness_current = Some(doc.harness_connectors.len() - 1);
            }
            216 | 217 => {
                // OWNERINDEX of harness entries/types is unreliable; they belong to the
                // most recent harness connector.
                let Some(h) = harness_current else {
                    doc.warnings.push(format!("record {i} ({}) has no harness connector before it", rt::type_name(r.record)));
                    continue;
                };
                let hc = &mut doc.harness_connectors[h];
                if r.record == 216 {
                    hc.entries.push(HarnessEntry {
                        index: i,
                        name: p.str("NAME"),
                        side: rt::side(p.int_or("SIDE", 0)).into(),
                        distance: rt::distance_from_top(p),
                    });
                } else {
                    hc.harness_type = opt_str(p, "TEXT");
                }
            }
            218 => doc.signal_harnesses.push(SignalHarness { index: i, points: rt::points(p) }),
            -1 if !r.binary => doc.warnings.push(format!("record {i} in {} has no RECORD property", r.stream)),
            n if rt::type_name(n) == "unknown" => {
                doc.warnings.push(format!("record {i}: unknown record type {n} in {} (see `sch records --type {n}`)", r.stream));
            }
            _ => {}
        }
    }
    // Bus entries ↔ buses.
    for be in &mut doc.bus_entries {
        for b in &mut doc.buses {
            if super::connect::on_polyline(be.location, &b.points) || super::connect::on_polyline(be.corner, &b.points) {
                be.bus = Some(b.index);
                b.entries.push(be.index);
                break;
            }
        }
    }
    let unannotated: Vec<String> =
        doc.components.iter().filter(|c| c.designator.contains('?')).map(|c| c.designator.clone()).collect();
    if !unannotated.is_empty() {
        doc.warnings.push(format!("{} unannotated component(s): {}", unannotated.len(), unannotated.join(", ")));
    }
    if !doc.harness_connectors.is_empty() || !doc.signal_harnesses.is_empty() {
        doc.warnings.push("signal harnesses are listed but their member nets are not resolved (use `ace designer` for a compiled netlist)".into());
    }
}

impl SchDoc {
    pub fn file_name(&self) -> String {
        self.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Count of records per type name.
    pub fn record_counts(&self) -> IndexMap<String, usize> {
        let mut m: IndexMap<String, usize> = IndexMap::new();
        for r in &self.records {
            *m.entry(r.type_name().to_owned()).or_default() += 1;
        }
        m.sort_keys();
        m
    }

    pub fn doc_param(&self, name: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    /// Component instances matching a designator: `U1` (every sub-part) or `U1A` (one).
    /// Case-insensitive.
    pub fn find_components(&self, designator: &str) -> Result<Vec<&Component>> {
        let d = designator.trim();
        let exact: Vec<&Component> =
            self.components.iter().filter(|c| c.display_designator.eq_ignore_ascii_case(d)).collect();
        let base: Vec<&Component> = self.components.iter().filter(|c| c.designator.eq_ignore_ascii_case(d)).collect();
        let hits = if !base.is_empty() { base } else { exact };
        if hits.is_empty() {
            let mut names: Vec<&str> = self.components.iter().map(|c| c.designator.as_str()).collect();
            names.sort();
            names.dedup();
            let shown: Vec<&str> = names.iter().take(40).copied().collect();
            return Err(Error::not_found(format!("no component {d:?} in {}", self.file_name()))
                .hint(format!("designators: {}{}", shown.join(", "), if names.len() > 40 { ", ..." } else { "" })));
        }
        Ok(hits)
    }

    /// Owning component designator of a record (walking the owner chain).
    pub fn owner_designator(&self, index: usize) -> Option<String> {
        let mut i = index;
        for _ in 0..8 {
            if let Some(c) = self.components.iter().find(|c| c.index == i) {
                return Some(c.display_designator.clone());
            }
            i = self.records.get(i)?.owner()?;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_names() {
        let r = parse_repeat("Repeat(CH, 1, 4)").unwrap();
        assert_eq!((r.name.as_str(), r.first, r.last), ("CH", 1, 4));
        assert!(parse_repeat("Fan").is_none());
        assert!(parse_repeat("repeat(CH,1)").is_none());
    }
}
