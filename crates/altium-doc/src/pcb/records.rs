//! Text-property PCB streams: `Nets6`, `Components6`, `Classes6`, `SignalClasses`, `Rules6`,
//! `Polygons6`, `Dimensions6`, `Models`, `DifferentialPairs6`, `PrimitiveParameters`,
//! `UniqueIDPrimitiveInformation`, `ExtendedPrimitiveInformation`, `FileVersionInfo`.
//!
//! Each stream is a sequence of `u32 length + |K=V|` records; `Rules6` and `Dimensions6`
//! prefix every record with a u16 (the rule/dimension object id).

use indexmap::IndexMap;
use serde::{Serialize, Serializer};

use altium_core::units::{Coord, Point};
use altium_format::cfbx::Bin;
use altium_format::props::Props;

use super::geom::{Outline, Vertex, vertices_from_props};
use super::layer::Layer;
use super::prim::codepoints;

fn is_false(b: &bool) -> bool {
    !*b
}

/// Split a property stream into records. `prefix` bytes precede each record (Rules6,
/// Dimensions6: 2). Returns the records (with their prefix as u16) and the undecoded tail.
pub fn prop_records(buf: &[u8], prefix: usize) -> (Vec<(u16, Props)>, usize) {
    let mut b = Bin::new(buf);
    let mut out = vec![];
    while b.remaining() >= 4 + prefix {
        let save = b.pos;
        let pre = if prefix == 2 { b.u16().unwrap_or(0) } else { 0 };
        if prefix > 2 && b.skip(prefix).is_err() {
            break;
        }
        match b.props() {
            Ok(p) => out.push((pre, p)),
            Err(_) => {
                b.pos = save;
                break;
            }
        }
    }
    (out, b.remaining())
}

// ----------------------------------------------------------------------------------- nets

#[derive(Debug, Clone, Serialize)]
pub struct Net {
    /// 0-based index (primitives refer to nets by this index).
    pub index: usize,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unique_id: String,
    /// Net colour (Altium BGR integer).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<i64>,
    pub visible: bool,
    /// Target length for length tuning, when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_length: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

pub fn net(index: usize, p: Props, keep_raw: bool) -> Net {
    Net {
        index,
        name: p.str("NAME"),
        unique_id: p.str("UNIQUEID"),
        color: p.int("COLOR"),
        visible: !p.has("VISIBLE") || p.bool("VISIBLE"),
        target_length: p.len("TARGETLENGTH"),
        raw: keep_raw.then_some(p),
    }
}

// ----------------------------------------------------------------------------- components

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    /// 0-based index (primitives refer to components by this index).
    pub index: usize,
    /// Designator as shown on the board (designator text, else `SOURCEDESIGNATOR`).
    pub designator: String,
    /// Footprint name (`PATTERN`).
    pub footprint: String,
    /// Comment text shown on the board (usually the value / part number).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Schematic description (`SOURCEDESCRIPTION`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub footprint_description: String,
    /// `top` or `bottom`.
    pub side: &'static str,
    pub x: Coord,
    pub y: Coord,
    /// Degrees.
    pub rotation: f64,
    pub locked: bool,
    pub designator_visible: bool,
    pub comment_visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<Coord>,
    /// Altium component kind code (`COMPONENTKIND`) and its name, when stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_code: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_designator: String,
    /// Schematic link as stored (`\SHEETID\COMPID` path of unique ids).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_unique_id: String,
    /// The schematic component's own `UNIQUEID` (last segment of `source_unique_id`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sch_unique_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_hierarchical_path: String,
    /// Schematic library the part came from.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_library: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_lib_reference: String,
    /// Footprint library (`SOURCEFOOTPRINTLIBRARY`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub footprint_library: String,
    /// The PCB component's own unique id (key of its `PrimitiveParameters`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unique_id: String,
    /// Parameters stored on the PCB component (`PrimitiveParameters`, newer files only).
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub parameters: IndexMap<String, String>,
    #[serde(skip_serializing_if = "is_false")]
    pub flipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

/// Altium `TComponentKind`.
pub fn component_kind(code: i64) -> Option<&'static str> {
    Some(match code {
        0 => "Standard",
        1 => "Mechanical",
        2 => "Graphical",
        3 => "Net Tie (In BOM)",
        4 => "Net Tie",
        5 => "Standard (No BOM)",
        6 => "Jumper",
        _ => return None,
    })
}

pub fn component(index: usize, p: Props, keep_raw: bool) -> Component {
    let suid = p.str("SOURCEUNIQUEID");
    let sch_uid = suid.rsplit('\\').next().unwrap_or("").to_owned();
    let kind_code = p.int("COMPONENTKIND");
    Component {
        index,
        designator: p.str("SOURCEDESIGNATOR"),
        footprint: p.str("PATTERN"),
        comment: String::new(),
        description: p.str("SOURCEDESCRIPTION"),
        footprint_description: p.str("FOOTPRINTDESCRIPTION"),
        side: if Layer::from_name(&p.str("LAYER")) == Some(Layer::BOTTOM) { "bottom" } else { "top" },
        x: p.len("X").unwrap_or_default(),
        y: p.len("Y").unwrap_or_default(),
        rotation: p.f64("ROTATION").unwrap_or(0.0),
        locked: p.bool("LOCKED"),
        designator_visible: !p.has("NAMEON") || p.bool("NAMEON"),
        comment_visible: p.bool("COMMENTON"),
        height: p.len("HEIGHT"),
        kind: kind_code.and_then(component_kind),
        kind_code,
        source_designator: p.str("SOURCEDESIGNATOR"),
        source_unique_id: suid,
        sch_unique_id: sch_uid,
        source_hierarchical_path: p.str("SOURCEHIERARCHICALPATH"),
        source_library: p.str("SOURCECOMPONENTLIBRARY"),
        source_lib_reference: p.str("SOURCELIBREFERENCE"),
        footprint_library: p.str("SOURCEFOOTPRINTLIBRARY"),
        unique_id: p.str("UNIQUEID"),
        parameters: IndexMap::new(),
        flipped: p.bool("FLIPPED"),
        raw: keep_raw.then_some(p),
    }
}

/// `PrimitiveParameters`: a header record (`PRIMITIVEID`, `COUNT`) followed by `COUNT`
/// `NAME`/`VALUE` records. Returns parameters keyed by primitive (component) unique id.
pub fn primitive_parameters(recs: &[(u16, Props)]) -> IndexMap<String, IndexMap<String, String>> {
    let mut out: IndexMap<String, IndexMap<String, String>> = IndexMap::new();
    let mut i = 0;
    while i < recs.len() {
        let p = &recs[i].1;
        if let Some(id) = p.get("PRIMITIVEID") {
            let n = p.int("COUNT").unwrap_or(0).max(0) as usize;
            let entry = out.entry(id).or_default();
            for r in recs.iter().skip(i + 1).take(n) {
                if let Some(name) = r.1.get("NAME") {
                    entry.insert(name, r.1.str("VALUE"));
                }
            }
            i += n + 1;
        } else {
            i += 1;
        }
    }
    out
}

// -------------------------------------------------------------------------------- classes

#[derive(Debug, Clone, Serialize)]
pub struct Class {
    pub index: usize,
    pub name: String,
    /// `net`, `component`, `from_to`, `pad`, `layer`, `design_channel`, `diff_pair`,
    /// `polygon`, `xsignal`.
    pub kind: String,
    pub kind_code: i64,
    pub members: Vec<String>,
    /// Built-in "All ..." class (members are implicit).
    #[serde(skip_serializing_if = "is_false")]
    pub superclass: bool,
    /// Generated by Altium (e.g. "Top Side Components").
    #[serde(skip_serializing_if = "is_false")]
    pub auto_generated: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unique_id: String,
    /// `Classes6` or `SignalClasses`.
    pub stream: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

pub fn class_kind(code: i64) -> String {
    match code {
        0 => "net".into(),
        1 => "component".into(),
        2 => "from_to".into(),
        3 => "pad".into(),
        4 => "layer".into(),
        5 => "design_channel".into(),
        6 => "diff_pair".into(),
        7 => "polygon".into(),
        10 => "xsignal".into(),
        n => format!("kind_{n}"),
    }
}

pub fn class(index: usize, p: Props, stream: &'static str, keep_raw: bool) -> Class {
    let code = p.int("KIND").unwrap_or(-1);
    let members = (0..).map_while(|i| p.get(&format!("M{i}"))).collect();
    Class {
        index,
        name: p.str("NAME"),
        kind: class_kind(code),
        kind_code: code,
        members,
        superclass: p.bool("SUPERCLASS"),
        auto_generated: p.bool("AUTOGENERATEDCLASS"),
        unique_id: p.str("UNIQUEID"),
        stream,
        raw: keep_raw.then_some(p),
    }
}

// ---------------------------------------------------------------------------------- rules

/// A rule setting value: lengths become [`Coord`], `TRUE`/`FALSE` booleans, numbers numbers.
#[derive(Debug, Clone)]
pub enum Value {
    Len(Coord),
    Bool(bool),
    Num(f64),
    Text(String),
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Value::Len(c) => c.serialize(s),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Num(n) => s.serialize_f64(*n),
            Value::Text(t) => s.serialize_str(t),
        }
    }
}

pub fn value(v: &str) -> Value {
    let t = v.trim();
    if (t.ends_with("mil") || t.ends_with("mm"))
        && let Some(c) = Coord::parse_len(t) {
            return Value::Len(c);
        }
    match t {
        "TRUE" | "True" | "T" => return Value::Bool(true),
        "FALSE" | "False" | "F" => return Value::Bool(false),
        _ => {}
    }
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'E' | 'e'))
        && let Ok(n) = t.parse::<f64>() {
            return Value::Num(n);
        }
    Value::Text(v.to_owned())
}

const RULE_COMMON: &[&str] = &[
    "SELECTION", "LAYER", "LOCKED", "POLYGONOUTLINE", "USERROUTED", "UNIONINDEX", "RULEKIND", "NETSCOPE",
    "LAYERKIND", "SCOPE1EXPRESSION", "SCOPE2EXPRESSION", "NAME", "ENABLED", "PRIORITY", "COMMENT", "UNIQUEID",
    "DEFINEDBYLOGICALDOCUMENT", "KEEPOUT",
];

fn is_vertex_key(k: &str) -> bool {
    let up = k.to_ascii_uppercase();
    ["KIND", "VX", "VY", "CX", "CY", "SA", "EA", "R"].iter().any(|p| {
        up.strip_prefix(p).is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Rule {
    pub index: usize,
    pub name: String,
    /// Altium rule kind (`Clearance`, `Width`, `RoutingVias`, `HoleSize`, `PolygonConnect`,
    /// `RoomDefinition`, ...).
    pub kind: String,
    /// The u16 object id stored before each record.
    pub kind_code: u16,
    pub enabled: bool,
    /// 1 = highest priority within its kind.
    pub priority: i64,
    /// First object scope (query language).
    pub scope1: String,
    /// Second scope for binary rules (clearances), empty for unary rules.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scope2: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub net_scope: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub layer_kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub comment: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unique_id: String,
    /// Every other setting, keys lower-cased (`gap`, `minlimit`, `maxlimit`, `preferedwidth`,
    /// `connectstyle`...). Lengths are converted to the output unit.
    pub values: IndexMap<String, Value>,
    /// Room outline (RoomDefinition rules only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Outline>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

pub fn rule(index: usize, code: u16, p: Props, keep_raw: bool) -> Rule {
    let mut values = IndexMap::new();
    for (k, v) in p.entries() {
        let up = k.to_ascii_uppercase();
        if RULE_COMMON.contains(&up.as_str()) || is_vertex_key(k) || up.starts_with("%UTF8%") {
            continue;
        }
        values.insert(k.to_ascii_lowercase(), value(v));
    }
    let verts = vertices_from_props(&p);
    Rule {
        index,
        name: p.str("NAME"),
        kind: p.str("RULEKIND"),
        kind_code: code,
        enabled: !p.has("ENABLED") || p.bool("ENABLED"),
        priority: p.int("PRIORITY").unwrap_or(1),
        scope1: p.str("SCOPE1EXPRESSION"),
        scope2: p.str("SCOPE2EXPRESSION"),
        net_scope: p.str("NETSCOPE"),
        layer_kind: p.str("LAYERKIND"),
        comment: p.str("COMMENT"),
        unique_id: p.str("UNIQUEID"),
        values,
        outline: (!verts.is_empty()).then(|| Outline::new(verts)),
        raw: keep_raw.then_some(p),
    }
}

/// A room: a `RoomDefinition` rule seen as a placement region.
#[derive(Debug, Clone, Serialize)]
pub struct Room {
    pub name: String,
    pub enabled: bool,
    pub layer: Option<Layer>,
    /// Components the room applies to (scope expression).
    pub scope: String,
    /// `ConfineIn` / `ConfineOut`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub confinement: String,
    pub lock_components: bool,
    pub priority: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Outline>,
    pub rule_index: usize,
}

pub fn room(r: &Rule, p: &Props) -> Room {
    Room {
        name: r.name.clone(),
        enabled: r.enabled,
        layer: Layer::from_name(&p.str("LAYER")),
        scope: r.scope1.clone(),
        confinement: p.str("CONFINEMENTSTYLE"),
        lock_components: p.bool("LOCKCOMPONENTS"),
        priority: r.priority,
        outline: r.outline.clone(),
        rule_index: r.index,
    }
}

// ------------------------------------------------------------------------------- polygons

#[derive(Debug, Clone, Serialize)]
pub struct Polygon {
    pub index: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub layer: Layer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_index: Option<u16>,
    /// `POLYGONTYPE` (`Polygon`, `Split Plane`, `Signal Layer Polygon`...).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub polygon_type: String,
    /// `Solid`, `45Degree`, `90Degree`, `Horizontal`, `Vertical`, `None` (outline only).
    pub hatch_style: String,
    /// Pour-over-same-net setting (`POUROVERSTYLE` code) and the legacy flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pour_over_style: Option<i64>,
    pub pour_over: bool,
    pub remove_dead: bool,
    pub remove_islands_by_area: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub island_area_threshold: Option<f64>,
    pub remove_necks: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neck_width_threshold: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid_size: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_width: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_prim_length: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arc_resolution: Option<Coord>,
    pub use_octagons: bool,
    /// Pour order (lower pours first).
    pub pour_index: i64,
    pub locked: bool,
    /// Shelved (not poured).
    pub shelved: bool,
    pub ignore_violations: bool,
    pub outline: Outline,
    /// Poured copper regions belonging to this polygon (filled by the document).
    pub poured_regions: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

/// Altium writes some names (polygon `NAME`, body `IDENTIFIER`) as comma-separated code
/// points (`71,78,68` = `GND`). Decode when the value has that exact shape.
pub fn maybe_codepoints(s: &str) -> String {
    let parts: Vec<&str> = s.split(',').collect();
    let codes: Option<Vec<u32>> = parts.iter().map(|c| c.trim().parse::<u32>().ok()).collect();
    match codes {
        Some(c) if !s.is_empty() && c.iter().all(|&u| u >= 32) => {
            c.into_iter().filter_map(char::from_u32).collect()
        }
        _ => s.to_owned(),
    }
}

pub fn polygon(index: usize, p: Props, keep_raw: bool) -> Polygon {
    let layer = Layer::from_name(&p.str("LAYER")).unwrap_or_default();
    let net_index = p.int("NET").and_then(|n| u16::try_from(n).ok()).filter(|&n| n != 0xFFFF);
    Polygon {
        index,
        name: maybe_codepoints(&p.str("NAME")),
        layer,
        net: None,
        net_index,
        polygon_type: p.str("POLYGONTYPE"),
        hatch_style: p.str("HATCHSTYLE"),
        pour_over_style: p.int("POUROVERSTYLE"),
        pour_over: p.bool("POUROVER"),
        remove_dead: p.bool("REMOVEDEAD"),
        remove_islands_by_area: p.bool("REMOVEISLANDSBYAREA"),
        island_area_threshold: p.f64("AREATHRESHOLD"),
        remove_necks: p.bool("REMOVENECKS"),
        neck_width_threshold: p.len("NECKWIDTHTHRESHOLD"),
        grid_size: p.len("GRIDSIZE"),
        track_width: p.len("TRACKWIDTH"),
        min_prim_length: p.len("MINPRIMLENGTH"),
        arc_resolution: p.len("ARCRESOLUTION"),
        use_octagons: p.bool("USEOCTAGONS"),
        pour_index: p.int("POURINDEX").unwrap_or(0),
        locked: p.bool("LOCKED"),
        shelved: p.bool("SHELVED"),
        ignore_violations: p.bool("IGNOREVIOLATIONS"),
        outline: Outline::new(vertices_from_props(&p)),
        poured_regions: 0,
        raw: keep_raw.then_some(p),
    }
}

// ----------------------------------------------------------------------------- dimensions

#[derive(Debug, Clone, Serialize)]
pub struct Dimension {
    pub index: usize,
    pub layer: Layer,
    /// `linear`, `angular`, `radial`, `leader`, `datum`, `baseline`, `center`,
    /// `linear_diameter`, `radial_diameter`.
    pub kind: String,
    pub kind_code: i64,
    /// Primary point (`X1`,`Y1`).
    pub start: Point,
    /// Measured reference points (`REFERENCEnPOINTX/Y`).
    pub references: Vec<DimRef>,
    /// Text anchor points (`TEXTnX/Y`).
    pub text_points: Vec<Point>,
    pub height: Coord,
    pub line_width: Coord,
    pub text_height: Coord,
    pub text_line_width: Coord,
    /// Degrees.
    pub angle: f64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precision: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text_format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrow_size: Option<Coord>,
    /// Bounding box stored by Altium (`LX,LY,HX,HY`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[Point; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DimRef {
    pub point: Point,
    /// What the reference snaps to (`BoardOutline`, `Track`, ...).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub object: String,
}

fn dim_kind(code: i64) -> String {
    match code {
        1 => "linear".into(),
        2 => "angular".into(),
        3 => "radial".into(),
        4 => "leader".into(),
        5 => "datum".into(),
        6 => "baseline".into(),
        7 => "center".into(),
        9 => "linear_diameter".into(),
        10 => "radial_diameter".into(),
        n => format!("kind_{n}"),
    }
}

pub fn dimension(index: usize, p: Props, keep_raw: bool) -> Dimension {
    let l = |k: &str| p.len(k).unwrap_or_default();
    let code = p.int("DIMENSIONKIND").unwrap_or(0);
    let layer = p
        .raw("LAYER_V7")
        .and_then(Layer::from_name)
        .filter(|l| l.0 & 0xFFFF > 16)
        .or_else(|| Layer::from_name(&p.str("LAYER")))
        .unwrap_or_default();
    let refs = (0..p.int("REFERENCES_COUNT").unwrap_or(0))
        .map(|i| DimRef {
            point: Point::new(l(&format!("REFERENCE{i}POINTX")), l(&format!("REFERENCE{i}POINTY"))),
            object: p.str(&format!("REFERENCE{i}OBJECTSTRING")),
        })
        .collect();
    let mut text_points = vec![];
    for i in 1.. {
        let (Some(x), Some(y)) = (p.len(&format!("TEXT{i}X")), p.len(&format!("TEXT{i}Y"))) else { break };
        text_points.push(Point::new(x, y));
    }
    let bbox = match (p.len("LX"), p.len("LY"), p.len("HX"), p.len("HY")) {
        (Some(a), Some(b), Some(c), Some(d)) => Some([Point::new(a, b), Point::new(c, d)]),
        _ => None,
    };
    Dimension {
        index,
        layer,
        kind: dim_kind(code),
        kind_code: code,
        start: Point::new(l("X1"), l("Y1")),
        references: refs,
        text_points,
        height: l("HEIGHT"),
        line_width: l("LINEWIDTH"),
        text_height: l("TEXTHEIGHT"),
        text_line_width: l("TEXTLINEWIDTH"),
        angle: p.f64("ANGLE").unwrap_or(0.0),
        unit: p.str("TEXTDIMENSIONUNIT"),
        precision: p.int("TEXTPRECISION"),
        prefix: p.str("TEXTPREFIX"),
        suffix: p.str("TEXTSUFFIX"),
        text_format: p.str("TEXTFORMAT"),
        arrow_size: p.len("ARROWSIZE"),
        bbox,
        raw: keep_raw.then_some(p),
    }
}

// --------------------------------------------------------------------------------- models

/// An entry of `Models/Data`: a 3D model referenced by component bodies; embedded models
/// have their zlib-compressed STEP data in the sibling stream `Models/<index>`.
#[derive(Debug, Clone, Serialize)]
pub struct Model {
    pub index: usize,
    /// GUID that bodies reference as `model_id`.
    pub id: String,
    pub name: String,
    pub embedded: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
    /// `DZ` exactly as stored (integer; its unit is not documented).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dz_raw: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<i64>,
    /// Stream holding the compressed data, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    /// Compressed size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressed_size: Option<u64>,
    /// Number of bodies using this model (filled by the document).
    pub used_by: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

pub fn model(index: usize, p: Props, keep_raw: bool) -> Model {
    Model {
        index,
        id: p.str("ID"),
        name: p.str("NAME"),
        embedded: p.bool("EMBED"),
        source: p.str("MODELSOURCE"),
        rotation_x: p.f64("ROTX").unwrap_or(0.0),
        rotation_y: p.f64("ROTY").unwrap_or(0.0),
        rotation_z: p.f64("ROTZ").unwrap_or(0.0),
        dz_raw: p.int("DZ"),
        checksum: p.int("CHECKSUM"),
        stream: None,
        compressed_size: None,
        used_by: 0,
        raw: keep_raw.then_some(p),
    }
}

// -------------------------------------------------------------------------- misc streams

/// One record of a stream this tool does not model field by field (e.g. `DifferentialPairs6`,
/// whose layout no sample exercises): name plus every property, keys lower-cased.
#[derive(Debug, Clone, Serialize)]
pub struct Generic {
    pub index: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub properties: IndexMap<String, String>,
}

pub fn generic(index: usize, p: &Props) -> Generic {
    Generic {
        index,
        name: p.str("NAME"),
        properties: p.entries().map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned())).collect(),
    }
}

/// `FileVersionInfo`: `VERn` entries are code-point lists naming the Altium releases that
/// touched the file; the last one is the version that saved it.
pub fn file_versions(p: &Props) -> Vec<String> {
    let n = p.int("COUNT").unwrap_or(0);
    (0..n).filter_map(|i| p.raw(&format!("VER{i}")).map(codepoints)).collect()
}

/// `UniqueIDPrimitiveInformation`: (object type name, primitive index) → unique id.
pub fn unique_ids(recs: &[(u16, Props)]) -> std::collections::HashMap<(String, usize), String> {
    recs.iter()
        .filter_map(|(_, p)| {
            let i = p.int("PRIMITIVEINDEX")?;
            Some(((p.str("PRIMITIVEOBJECTID").to_ascii_lowercase(), i as usize), p.str("UNIQUEID")))
        })
        .collect()
}

/// Board outline vertex series helper re-exported for callers holding raw props.
pub fn outline_of(p: &Props) -> Vec<Vertex> {
    vertices_from_props(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(prefix: Option<u16>, s: &str) -> Vec<u8> {
        let mut out = vec![];
        if let Some(p) = prefix {
            out.extend(p.to_le_bytes());
        }
        let mut b = s.as_bytes().to_vec();
        b.push(0);
        out.extend((b.len() as u32).to_le_bytes());
        out.extend(b);
        out
    }

    #[test]
    fn prefixed_records_and_rules() {
        let mut buf = rec(Some(2), "|RULEKIND=Width|NAME=W|ENABLED=TRUE|PRIORITY=3|SCOPE1EXPRESSION=All|MINLIMIT=6mil|MAXLIMIT=0.5mm|FOO=TRUE|N=12");
        buf.extend(rec(Some(40), "|RULEKIND=RoomDefinition|NAME=R1|LAYER=TOP|CONFINEMENTSTYLE=ConfineIn|KIND0=0|VX0=0mil|VY0=0mil|KIND1=0|VX1=100mil|VY1=0mil|KIND2=0|VX2=100mil|VY2=100mil"));
        let (recs, left) = prop_records(&buf, 2);
        assert_eq!(left, 0);
        assert_eq!(recs.len(), 2);
        let r = rule(0, recs[0].0, recs[0].1.clone(), false);
        assert_eq!((r.kind.as_str(), r.kind_code, r.priority, r.enabled), ("Width", 2, 3, true));
        assert!(matches!(r.values["minlimit"], Value::Len(Coord(60_000))));
        assert!(matches!(r.values["maxlimit"], Value::Len(c) if c == Coord::from_mm(0.5)));
        assert!(matches!(r.values["foo"], Value::Bool(true)));
        assert!(matches!(r.values["n"], Value::Num(n) if n == 12.0));
        assert!(!r.values.contains_key("name") && !r.values.contains_key("scope1expression"));
        let r2 = rule(1, recs[1].0, recs[1].1.clone(), false);
        let room = room(&r2, &recs[1].1);
        assert_eq!(room.layer, Some(Layer::TOP));
        assert_eq!(room.outline.as_ref().unwrap().vertex_count, 3);
        assert!(!r2.values.contains_key("vx0"));
    }

    #[test]
    fn parameters_follow_their_header() {
        let mut buf = rec(None, "|PRIMITIVEID=AAA|VARIANTGUID=|COUNT=2");
        buf.extend(rec(None, "|NAME=Value|VALUE=10k"));
        buf.extend(rec(None, "|NAME=MPN|VALUE=RC0603"));
        buf.extend(rec(None, "|PRIMITIVEID=BBB|COUNT=0"));
        let (recs, _) = prop_records(&buf, 0);
        let p = primitive_parameters(&recs);
        assert_eq!(p["AAA"]["Value"], "10k");
        assert_eq!(p["AAA"]["MPN"], "RC0603");
        assert!(p["BBB"].is_empty());
    }

    #[test]
    fn components_and_classes() {
        let (recs, _) = prop_records(
            &rec(None, "|LAYER=BOTTOM|X=100mil|Y=200mil|ROTATION= 9.00000000000000E+0001|PATTERN=R0603|SOURCEDESIGNATOR=R1|SOURCEUNIQUEID=\\AB\\CD|COMPONENTKIND=4"),
            0,
        );
        let c = component(0, recs[0].1.clone(), false);
        assert_eq!((c.side, c.rotation, c.sch_unique_id.as_str()), ("bottom", 90.0, "CD"));
        assert_eq!(c.kind, Some("Net Tie"));
        let (recs, _) = prop_records(&rec(None, "|NAME=PWR|KIND=0|M0=VCC|M1=GND"), 0);
        let k = class(0, recs[0].1.clone(), "Classes6", false);
        assert_eq!((k.kind.as_str(), k.members.len()), ("net", 2));
    }

    #[test]
    fn codepoint_names() {
        assert_eq!(maybe_codepoints("71,78,68,95,53,86"), "GND_5V");
        assert_eq!(maybe_codepoints("Top Pour"), "Top Pour");
        assert_eq!(maybe_codepoints("5"), "5", "control characters are not decoded");
        assert_eq!(maybe_codepoints(""), "");
    }
}
