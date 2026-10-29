//! Typed objects shared by schematic sheets and symbol libraries: pins, parameters, models
//! (implementations) and drawing primitives. Built from one record's [`Props`].

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::units::{Coord, Point};
use altium_format::props::Props;

use super::records as rt;

fn skip_false(b: &bool) -> bool {
    !*b
}

/// One component pin.
#[derive(Debug, Clone, Serialize)]
pub struct Pin {
    /// Record index (stable reference within the document / symbol).
    pub index: usize,
    /// Pin designator ("number"): `1`, `A3`, `GND`...
    pub designator: String,
    /// Pin name / function shown inside the body. A `\` after a character means an overbar.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `input`, `io`, `output`, `open_collector`, `passive`, `hiz`, `open_emitter`, `power`.
    pub electrical: String,
    /// Owning sub-part id (`OWNERPARTID`); 0 or negative = shared by every part.
    pub part_id: i64,
    /// `OWNERPARTDISPLAYMODE` (alternate symbol index, 0 = normal).
    #[serde(skip_serializing_if = "is_zero")]
    pub display_mode: i64,
    /// Body end of the pin line (`LOCATION`). Absolute sheet coordinates in a SchDoc,
    /// symbol-local in a SchLib.
    pub location: Point,
    /// Electrical connection point: `location + length` in the pin's direction.
    pub hot_end: Point,
    pub length: Coord,
    /// Direction the pin points from the body towards its hot end, degrees (0 right, 90 up).
    pub orientation: f64,
    #[serde(skip_serializing_if = "skip_false")]
    pub hidden: bool,
    pub show_name: bool,
    pub show_designator: bool,
    #[serde(skip_serializing_if = "skip_false")]
    pub locked: bool,
    /// IEEE symbols decorating the pin, when any (`inner`, `outer`, `inner_edge`, `outer_edge`).
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub symbols: IndexMap<String, String>,
    /// Pin swap group (`SWAPIDPIN`) / part swap (`SWAPIDPART`), when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap_id_pin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap_id_part: Option<String>,
    /// `PinUniqueId` parameter attached to the pin (SchDoc only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    /// Net the hot end connects to (filled by connectivity; SchDoc only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

impl Pin {
    pub fn from_props(index: usize, p: &Props, keep_raw: bool) -> Pin {
        let conglomerate = p.int_or("PINCONGLOMERATE", 0);
        let orient = conglomerate & 0x03;
        let location = rt::location(p);
        let length = p.sch("PINLENGTH");
        let (dx, dy) = rt::orientation_dir(orient);
        let hot_end = Point::new(Coord(location.x.0 + dx * length.0), Coord(location.y.0 + dy * length.0));
        let mut symbols = IndexMap::new();
        for (key, label) in [
            ("SYMBOL_INNER", "inner"),
            ("SYMBOL_OUTER", "outer"),
            ("SYMBOL_INNEREDGE", "inner_edge"),
            ("SYMBOL_OUTEREDGE", "outer_edge"),
        ] {
            let v = p.int_or(key, 0);
            if v != 0 {
                symbols.insert(label.to_owned(), rt::pin_symbol(v).map(str::to_owned).unwrap_or_else(|| v.to_string()));
            }
        }
        let opt = |k: &str| p.get(k).filter(|s| !s.is_empty());
        let electrical = p.int_or("ELECTRICAL", 0);
        Pin {
            index,
            designator: p.str("DESIGNATOR"),
            name: p.str("NAME"),
            description: opt("DESCRIPTION"),
            electrical: rt::pin_electrical(electrical).map(str::to_owned).unwrap_or_else(|| format!("unknown_{electrical}")),
            part_id: p.int_or("OWNERPARTID", 0),
            display_mode: p.int_or("OWNERPARTDISPLAYMODE", 0),
            location,
            hot_end,
            length,
            orientation: rt::orientation_deg(orient),
            hidden: conglomerate & 0x04 != 0,
            show_name: conglomerate & 0x08 != 0,
            show_designator: conglomerate & 0x10 != 0,
            locked: conglomerate & 0x40 != 0,
            symbols,
            swap_id_pin: opt("SWAPIDPIN"),
            swap_id_part: opt("SWAPIDPART"),
            unique_id: None,
            net: None,
            raw: keep_raw.then(|| p.clone()),
        }
    }
}

/// A parameter (record 41): component, document, sheet symbol, pin or model parameter.
#[derive(Debug, Clone, Serialize)]
pub struct Parameter {
    pub index: usize,
    pub name: String,
    /// Text exactly as stored (`*` = not set; `=Other` = reference to another parameter).
    pub value: String,
    /// Value of the referenced parameter when `value` starts with `=` and the target exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    /// Not shown on the sheet.
    pub hidden: bool,
    #[serde(skip_serializing_if = "skip_false")]
    pub show_name: bool,
    /// `READONLYSTATE` (1 = name read-only, 3 = name and value read-only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_state: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

impl Parameter {
    pub fn from_props(index: usize, p: &Props, keep_raw: bool) -> Parameter {
        Parameter {
            index,
            name: p.str("NAME"),
            value: p.str("TEXT"),
            resolved: None,
            hidden: p.bool("ISHIDDEN"),
            show_name: p.bool("SHOWNAME"),
            read_only_state: p.int("READONLYSTATE"),
            location: rt::location_opt(p),
            orientation: p.int("ORIENTATION").map(rt::orientation_deg),
            unique_id: p.get("UNIQUEID").filter(|s| !s.is_empty()),
            raw: keep_raw.then(|| p.clone()),
        }
    }

    /// The value to use: the resolved reference when there is one, else the stored text.
    pub fn effective(&self) -> &str {
        self.resolved.as_deref().unwrap_or(&self.value)
    }
}

/// Resolve `=Name` references among a parameter list (then `fallback`, e.g. document
/// parameters). Altium ignores spaces after `=` and matches names case-insensitively.
pub fn resolve_params(params: &mut [Parameter], fallback: &[Parameter]) {
    let lookup: Vec<(String, String)> = params
        .iter()
        .chain(fallback.iter())
        .map(|p| (p.name.to_ascii_lowercase(), p.value.clone()))
        .collect();
    for p in params.iter_mut() {
        let Some(target) = p.value.strip_prefix('=') else { continue };
        let target = target.trim().to_ascii_lowercase();
        if target.is_empty() {
            continue;
        }
        // One level of indirection is what Altium shows; follow up to 4 to be safe.
        let mut cur = target;
        for _ in 0..4 {
            let Some((_, v)) = lookup.iter().find(|(n, _)| *n == cur) else { break };
            match v.strip_prefix('=') {
                Some(next) => cur = next.trim().to_ascii_lowercase(),
                None => {
                    p.resolved = Some(v.clone());
                    break;
                }
            }
        }
    }
}

/// One data file of a model (`MODELDATAFILEKINDn`, `MODELDATAFILEENTITYn`, `MODELDATAFILEn`).
#[derive(Debug, Clone, Serialize)]
pub struct ModelDataFile {
    /// `PCBLib`, `SIMLib`, ...
    pub kind: String,
    /// Entity (model) name inside the file.
    pub entity: String,
    /// Library file, when recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Schematic pin → model pin mapping (map definer, record 47).
#[derive(Debug, Clone, Serialize)]
pub struct PinMap {
    /// Schematic pin designator (`DESINTF`).
    pub pin: String,
    /// Model pins / pads it maps to (`DESIMPn`).
    pub pads: Vec<String>,
}

/// An implementation (record 45): footprint, simulation, signal integrity, 3D or IBIS model.
#[derive(Debug, Clone, Serialize)]
pub struct Model {
    pub index: usize,
    /// `footprint`, `simulation`, `signal_integrity`, `3d`, `ibis`, `other`.
    pub kind: String,
    /// `MODELTYPE` as stored (`PCBLIB`, `SIM`, `SI`, `PCB3DLib`...).
    pub model_type: String,
    /// Model (footprint) name.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The model the component currently uses for its kind (`ISCURRENT`).
    pub current: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub datafiles: Vec<ModelDataFile>,
    #[serde(skip_serializing_if = "skip_false")]
    pub use_component_library: bool,
    #[serde(skip_serializing_if = "skip_false")]
    pub integrated_model: bool,
    #[serde(skip_serializing_if = "skip_false")]
    pub database_model: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
    /// Pin mapping overrides (only when the symbol's pins do not map 1:1).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pin_map: Vec<PinMap>,
    /// Model parameters (records 41 under the model's parameter list).
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub parameters: IndexMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

impl Model {
    pub fn from_props(index: usize, p: &Props, keep_raw: bool) -> Model {
        let model_type = p.str("MODELTYPE");
        let n = p.int_or("DATAFILECOUNT", 0).max(0);
        let datafiles = (0..n)
            .map(|i| ModelDataFile {
                kind: p.str(&format!("MODELDATAFILEKIND{i}")),
                entity: p.str(&format!("MODELDATAFILEENTITY{i}")),
                file: p.get(&format!("MODELDATAFILE{i}")).filter(|s| !s.is_empty()),
            })
            .collect();
        Model {
            index,
            kind: rt::model_kind(&model_type).to_owned(),
            model_type,
            name: p.str("MODELNAME"),
            description: p.get("DESCRIPTION").filter(|s| !s.is_empty()),
            current: p.bool("ISCURRENT"),
            datafiles,
            use_component_library: p.bool("USECOMPONENTLIBRARY"),
            integrated_model: p.bool("INTEGRATEDMODEL"),
            database_model: p.bool("DATABASEMODEL"),
            unique_id: p.get("UNIQUEID").filter(|s| !s.is_empty()),
            pin_map: vec![],
            parameters: IndexMap::new(),
            raw: keep_raw.then(|| p.clone()),
        }
    }
}

/// The footprint a component uses: the current `PCBLIB` model, else the first one.
pub fn current_footprint(models: &[Model]) -> Option<&Model> {
    let fps: Vec<&Model> = models.iter().filter(|m| m.kind == "footprint").collect();
    fps.iter().find(|m| m.current).or(fps.first()).copied()
}

/// A drawing primitive of a symbol (line, rectangle, arc, polygon, label...).
#[derive(Debug, Clone, Serialize)]
pub struct Primitive {
    pub index: usize,
    pub record: i64,
    /// Record type name (`rectangle`, `polyline`, `arc`...).
    #[serde(rename = "type")]
    pub kind: String,
    /// Owning sub-part (`OWNERPARTID`); 0/negative = all parts.
    pub part_id: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub display_mode: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corner: Option<Point>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_radius: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_angle: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_angle: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "skip_false")]
    pub filled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

impl Primitive {
    pub fn from_props(index: usize, record: i64, p: &Props, keep_raw: bool) -> Primitive {
        let has_loc = p.has("LOCATION.X") || p.has("LOCATION.Y");
        let has_corner = p.has("CORNER.X") || p.has("CORNER.Y");
        let has_r = p.has("RADIUS") || p.has("RADIUS_FRAC");
        let is_arc = matches!(record, 9 | 11 | 12);
        Primitive {
            index,
            record,
            kind: rt::type_name(record).to_owned(),
            part_id: p.int_or("OWNERPARTID", 0),
            display_mode: p.int_or("OWNERPARTDISPLAYMODE", 0),
            location: has_loc.then(|| rt::location(p)),
            corner: has_corner.then(|| rt::corner(p)),
            points: rt::points(p),
            radius: has_r.then(|| p.sch("RADIUS")),
            secondary_radius: (p.has("SECONDARYRADIUS") || p.has("SECONDARYRADIUS_FRAC")).then(|| p.sch("SECONDARYRADIUS")),
            start_angle: if is_arc { Some(p.f64("STARTANGLE").unwrap_or(0.0)) } else { None },
            end_angle: if is_arc { Some(p.f64("ENDANGLE").unwrap_or(0.0)) } else { None },
            text: p.get("TEXT").filter(|s| !s.is_empty()).map(|s| rt::frame_text(&s)),
            filled: p.bool("ISSOLID"),
            raw: keep_raw.then(|| p.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_hot_end() {
        // From AirPi Fan.SchDoc: pin at (740,400), length 30, PINCONGLOMERATE 58 (leftwards);
        // the wire ends at x=710.
        let p = Props::parse(b"|RECORD=2|OWNERINDEX=30|OWNERPARTID=1|ELECTRICAL=4|PINCONGLOMERATE=58|PINLENGTH=30|LOCATION.X=740|LOCATION.Y=400|NAME=5V|DESIGNATOR=5V");
        let pin = Pin::from_props(31, &p, false);
        assert_eq!(pin.hot_end, Point::new(Coord::from_sch(710, 0), Coord::from_sch(400, 0)));
        assert_eq!(pin.orientation, 180.0);
        assert_eq!(pin.electrical, "passive");
        assert!(pin.show_name && pin.show_designator && !pin.hidden);
    }

    #[test]
    fn param_refs() {
        let mk = |i, n: &str, v: &str| Parameter::from_props(i, &Props::parse(format!("|RECORD=41|NAME={n}|TEXT={v}").as_bytes()), false);
        let mut ps = vec![mk(1, "Comment", "= Value"), mk(2, "Value", "10k"), mk(3, "X", "=Missing")];
        resolve_params(&mut ps, &[]);
        assert_eq!(ps[0].effective(), "10k");
        assert_eq!(ps[2].resolved, None);
    }
}
