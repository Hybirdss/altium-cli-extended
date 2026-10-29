//! `Board6/Data` (and the PcbLib `Library/Data` header, which uses the same keys): origin,
//! sheet, outline, every layer definition and the physical layer stack.
//!
//! Layer definitions come in up to four generations inside one property record:
//! legacy `LAYER<n>*` (n = V6 id 1..82, linked list via `PREV`/`NEXT`), `LAYERV7_<i>*`
//! (mechanical 17+), `LAYER_V8_<i>*` and `V9_STACK_LAYER<i>_*` / `V9_CACHE_LAYER<i>_*`
//! (32-bit `LAYERID`, physical order, `USEDBYPRIMS`). The newest present generation decides the
//! stack order; older ones fill in what it lacks.

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::units::{Coord, Point};
use altium_format::props::Props;

use super::geom::{Outline, vertices_from_props};
use super::layer::{Layer, LayerKind};

#[derive(Debug, Clone, Serialize)]
pub struct Board {
    /// User origin (`ORIGINX/ORIGINY`); every coordinate in the file is absolute, this is the
    /// point Altium displays as 0,0.
    pub origin: Point,
    pub sheet: Sheet,
    /// Board shape from `Board6` (`KIND/VX/VY/...` vertices).
    pub outline: Outline,
    /// Number of copper layers (signal + plane) in the stack.
    pub copper_layer_count: usize,
    /// Copper layers top to bottom.
    pub copper_layers: Vec<Layer>,
    pub stackup: Stackup,
    /// Every layer the file defines, with names, enable state and stack data.
    pub layers: Vec<LayerDef>,
    /// Drill pairs (`LAYERPAIRn`).
    pub layer_pairs: Vec<LayerPair>,
    /// Net assigned to each internal plane (`PLANEnNETNAME`), when not "(No Net)".
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub plane_nets: IndexMap<String, String>,
    /// `KIND` / `VERSION` / `DATE` / `TIME` / `FILENAME` of the board record.
    pub saved: IndexMap<String, String>,
    #[serde(skip)]
    pub props: Props,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sheet {
    pub x: Coord,
    pub y: Coord,
    pub width: Coord,
    pub height: Coord,
    pub shown: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerPair {
    pub low: Option<Layer>,
    pub high: Option<Layer>,
    pub drill_guide: bool,
    pub drill_drawing: bool,
}

/// One layer as the file defines it.
#[derive(Debug, Clone, Serialize)]
pub struct LayerDef {
    /// Display / scripting name (`TopLayer`, `MidLayer1`, `Mechanical13`, `TopOverlay`...).
    pub layer: Layer,
    /// 32-bit layer id.
    pub id: u32,
    /// One-byte legacy id (1..82), when the layer has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacy_id: Option<u8>,
    /// Name the user gave the layer (`Top Layer`, `Assembly Top`, ...).
    pub user_name: String,
    pub kind: LayerKind,
    /// Copper: part of the stack; mechanical: enabled; others: always available.
    pub enabled: bool,
    /// Altium's own "has primitives" flag (newer files).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_by_prims: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copper_thickness: Option<Coord>,
    /// Mechanical layer kind (`MECHKIND`, e.g. `AssemblyTop`), when assigned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mech_kind: Option<String>,
    /// Position in the physical stack (see `stackup`), for stack layers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_index: Option<usize>,
    /// Dielectric data for dielectric and solder-mask layers of the stack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dielectric: Option<Dielectric>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dielectric {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constant: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loss_tangent: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stackup {
    /// Which keys described it: `v9`, `v8` or `legacy`.
    pub source: &'static str,
    pub layers: Vec<StackLayer>,
    /// Sum of all thicknesses (copper, dielectrics, solder mask).
    pub total_thickness: Coord,
    /// Copper + dielectric only (no solder mask).
    pub board_thickness: Coord,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackLayer {
    pub index: usize,
    pub name: String,
    pub layer: Layer,
    /// `copper`, `plane`, `dielectric`, `solder`, `overlay`, `paste`.
    pub kind: LayerKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dielectric_constant: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loss_tangent: Option<f64>,
    /// `DIELTYPE` code as stored (3 = surface coating such as solder resist).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dielectric_type: Option<i64>,
    /// Component placement side for copper layers (`top` / `bottom`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_side: Option<&'static str>,
}

/// Props of one layer description under some key prefix.
struct Keys<'a> {
    p: &'a Props,
    prefix: String,
}

impl Keys<'_> {
    fn s(&self, k: &str) -> Option<String> {
        self.p.get(&format!("{}{k}", self.prefix))
    }
    fn len(&self, k: &str) -> Option<Coord> {
        self.p.len(&format!("{}{k}", self.prefix))
    }
    fn f(&self, k: &str) -> Option<f64> {
        self.p.f64(&format!("{}{k}", self.prefix))
    }
    fn i(&self, k: &str) -> Option<i64> {
        self.p.int(&format!("{}{k}", self.prefix))
    }
    fn b(&self, k: &str) -> Option<bool> {
        self.p.raw(&format!("{}{k}", self.prefix)).map(|v| v.eq_ignore_ascii_case("TRUE") || v == "T")
    }
}

fn stack_entry(k: &Keys, layer: Layer, index: usize) -> StackLayer {
    let kind = layer.kind();
    let copper = matches!(kind, LayerKind::Copper | LayerKind::Plane);
    let diel = k.s("DIELMATERIAL").is_some() || kind == LayerKind::Dielectric;
    StackLayer {
        index,
        name: k.s("NAME").unwrap_or_default(),
        layer,
        kind,
        thickness: if copper { k.len("COPTHICK") } else if diel { k.len("DIELHEIGHT") } else { None },
        material: if diel { k.s("DIELMATERIAL") } else { None },
        dielectric_constant: if diel { k.f("DIELCONST") } else { None },
        loss_tangent: if diel { k.f("DIELLOSSTANGENT") } else { None },
        dielectric_type: if diel { k.i("DIELTYPE") } else { None },
        component_side: match k.i("COMPONENTPLACEMENT") {
            Some(1) => Some("top"),
            Some(2) => Some("bottom"),
            _ => None,
        },
    }
}

fn physical(l: Layer) -> bool {
    matches!(
        l.kind(),
        LayerKind::Copper | LayerKind::Plane | LayerKind::Dielectric | LayerKind::Solder | LayerKind::Overlay | LayerKind::Paste
    )
}

/// Collect `<family><i><sep>...` layer descriptions that carry a `LAYERID`.
fn family<'a>(p: &'a Props, family: &str, sep: &str) -> Vec<(Layer, Keys<'a>)> {
    let mut out = vec![];
    for i in 0.. {
        let prefix = format!("{family}{i}{sep}");
        if p.raw(&format!("{prefix}NAME")).is_none() {
            break;
        }
        let k = Keys { p, prefix };
        let Some(id) = k.i("LAYERID") else { continue };
        out.push((Layer::from_id(id as u32), k));
    }
    out
}

pub fn parse_layers(p: &Props) -> (Vec<LayerDef>, Stackup, Vec<Layer>, Vec<String>) {
    let mut warnings = vec![];
    let mut defs: IndexMap<Layer, LayerDef> = IndexMap::new();
    let put = |layer: Layer, k: &Keys, defs: &mut IndexMap<Layer, LayerDef>| {
        let kind = layer.kind();
        let e = defs.entry(layer).or_insert_with(|| LayerDef {
            layer,
            id: layer.0,
            legacy_id: layer.v6(),
            user_name: String::new(),
            kind,
            enabled: !matches!(kind, LayerKind::Mechanical | LayerKind::Copper | LayerKind::Plane | LayerKind::System),
            used_by_prims: None,
            copper_thickness: None,
            mech_kind: None,
            stack_index: None,
            dielectric: None,
        });
        if let Some(n) = k.s("NAME").filter(|n| !n.is_empty()) {
            e.user_name = n;
        }
        if let Some(u) = k.b("USEDBYPRIMS") {
            e.used_by_prims = Some(u);
        }
        if kind == LayerKind::Mechanical {
            if let Some(m) = k.b("MECHENABLED") {
                e.enabled = m;
            }
            if let Some(m) = k.s("MECHKIND").filter(|m| !m.is_empty()) {
                e.mech_kind = Some(m);
            }
        }
        if matches!(kind, LayerKind::Copper | LayerKind::Plane)
            && let Some(c) = k.len("COPTHICK") {
                e.copper_thickness = Some(c);
            }
    };

    // Legacy generation: LAYER1..LAYER82 and LAYERV7_i.
    let mut legacy: Vec<(Layer, Keys)> = vec![];
    for n in 1..=82u8 {
        let k = Keys { p, prefix: format!("LAYER{n}") };
        if k.s("NAME").is_some() {
            legacy.push((Layer::from_v6(n), k));
        }
    }
    legacy.extend(family(p, "LAYERV7_", ""));
    for (l, k) in &legacy {
        put(*l, k, &mut defs);
    }
    let v8 = family(p, "LAYER_V8_", "");
    for (l, k) in &v8 {
        put(*l, k, &mut defs);
    }
    let v9_stack = family(p, "V9_STACK_LAYER", "_");
    let v9_cache = family(p, "V9_CACHE_LAYER", "_");
    for (l, k) in v9_cache.iter().chain(&v9_stack) {
        put(*l, k, &mut defs);
    }

    // Legacy copper chain from the top layer through NEXT links.
    let mut chain = vec![];
    let mut cur: i64 = 1;
    let mut guard = 0;
    while cur > 0 && guard < 100 {
        let k = Keys { p, prefix: format!("LAYER{cur}") };
        if k.s("NAME").is_none() {
            break;
        }
        chain.push((Layer::from_v6(cur as u8), cur));
        if cur == 32 {
            break;
        }
        cur = k.i("NEXT").unwrap_or(0);
        guard += 1;
    }

    let mut layers = vec![];
    let source;
    let newest: Vec<&(Layer, Keys)> = if !v9_stack.is_empty() {
        source = "v9";
        v9_stack.iter().filter(|(l, _)| physical(*l)).collect()
    } else if !v8.is_empty() {
        source = "v8";
        v8.iter().filter(|(l, _)| physical(*l)).collect()
    } else {
        source = "legacy";
        vec![]
    };
    if source != "legacy" {
        for (l, k) in newest {
            let i = layers.len();
            layers.push(stack_entry(k, *l, i));
        }
    } else {
        let side = |pre: &str| -> Option<StackLayer> {
            let k = Keys { p, prefix: pre.to_owned() };
            k.len("HEIGHT")?;
            Some(StackLayer {
                index: 0,
                name: if pre == "TOP" { "Top Solder".into() } else { "Bottom Solder".into() },
                layer: if pre == "TOP" { Layer::TOP_SOLDER } else { Layer::BOTTOM_SOLDER },
                kind: LayerKind::Solder,
                thickness: k.len("HEIGHT"),
                material: k.s("MATERIAL"),
                dielectric_constant: k.f("CONST"),
                loss_tangent: None,
                dielectric_type: k.i("TYPE"),
                component_side: None,
            })
        };
        if let Some(s) = side("TOP") {
            layers.push(s);
        }
        let mut dn = 0;
        for (i, (l, n)) in chain.iter().enumerate() {
            let k = Keys { p, prefix: format!("LAYER{n}") };
            let mut e = stack_entry(&k, *l, 0);
            e.material = None;
            e.dielectric_constant = None;
            e.loss_tangent = None;
            e.dielectric_type = None;
            e.thickness = k.len("COPTHICK");
            layers.push(e);
            if i + 1 < chain.len() {
                dn += 1;
                layers.push(StackLayer {
                    index: 0,
                    name: format!("Dielectric {dn}"),
                    layer: Layer::dielectric(dn),
                    kind: LayerKind::Dielectric,
                    thickness: k.len("DIELHEIGHT"),
                    material: k.s("DIELMATERIAL"),
                    dielectric_constant: k.f("DIELCONST"),
                    loss_tangent: None,
                    dielectric_type: k.i("DIELTYPE"),
                    component_side: None,
                });
            }
        }
        if let Some(s) = side("BOTTOM") {
            layers.push(s);
        }
        for (i, l) in layers.iter_mut().enumerate() {
            l.index = i;
        }
    }

    let copper: Vec<Layer> = layers.iter().filter(|l| l.layer.is_copper()).map(|l| l.layer).collect();
    let chain_layers: Vec<Layer> = chain.iter().map(|c| c.0).collect();
    if source != "legacy" && !chain_layers.is_empty() && chain_layers != copper {
        warnings.push(format!(
            "layer stack: legacy LAYERn chain lists {} copper layers, {source} stack lists {}; using {source}",
            chain_layers.len(),
            copper.len()
        ));
    }
    for st in &layers {
        let d = defs.entry(st.layer).or_insert_with(|| LayerDef {
            layer: st.layer,
            id: st.layer.0,
            legacy_id: st.layer.v6(),
            user_name: st.name.clone(),
            kind: st.kind,
            enabled: true,
            used_by_prims: None,
            copper_thickness: None,
            mech_kind: None,
            stack_index: None,
            dielectric: None,
        });
        d.stack_index = Some(st.index);
        if st.material.is_some() || st.dielectric_constant.is_some() {
            d.dielectric = Some(Dielectric {
                thickness: st.thickness,
                material: st.material.clone(),
                constant: st.dielectric_constant,
                loss_tangent: st.loss_tangent,
            });
        }
        if matches!(st.kind, LayerKind::Copper | LayerKind::Plane | LayerKind::Dielectric) {
            d.enabled = true;
        }
        if d.user_name.is_empty() {
            d.user_name = st.name.clone();
        }
    }
    let total: i64 = layers.iter().filter_map(|l| l.thickness).map(|c| c.0).sum();
    let board: i64 = layers
        .iter()
        .filter(|l| matches!(l.kind, LayerKind::Copper | LayerKind::Plane | LayerKind::Dielectric))
        .filter_map(|l| l.thickness)
        .map(|c| c.0)
        .sum();
    let mut defs: Vec<LayerDef> = defs.into_values().collect();
    defs.sort_by_key(|d| (d.stack_index.unwrap_or(usize::MAX), d.layer.order()));
    (defs, Stackup { source, layers, total_thickness: Coord(total), board_thickness: Coord(board) }, copper, warnings)
}

pub fn parse_board(p: Props) -> (Board, Vec<String>) {
    let l = |k: &str| p.len(k).unwrap_or_default();
    let (layers, stackup, copper, warnings) = parse_layers(&p);
    let mut layer_pairs = vec![];
    for i in 0.. {
        let Some(low) = p.raw(&format!("LAYERPAIR{i}LOW")) else { break };
        layer_pairs.push(LayerPair {
            low: Layer::from_name(low),
            high: p.raw(&format!("LAYERPAIR{i}HIGH")).and_then(Layer::from_name),
            drill_guide: p.bool(&format!("LAYERPAIR{i}DRILLGUIDE")),
            drill_drawing: p.bool(&format!("LAYERPAIR{i}DRILLDRAWING")),
        });
    }
    let mut plane_nets = IndexMap::new();
    for n in 1..=16 {
        if let Some(v) = p.get(&format!("PLANE{n}NETNAME")).filter(|v| v != "(No Net)" && !v.is_empty()) {
            plane_nets.insert(Layer::plane(n).name(), v);
        }
    }
    let saved = ["KIND", "VERSION", "DATE", "TIME", "FILENAME"]
        .iter()
        .filter_map(|k| p.get(k).map(|v| (k.to_ascii_lowercase(), v)))
        .collect();
    let b = Board {
        origin: Point::new(l("ORIGINX"), l("ORIGINY")),
        sheet: Sheet {
            x: l("SHEETX"),
            y: l("SHEETY"),
            width: l("SHEETWIDTH"),
            height: l("SHEETHEIGHT"),
            shown: p.bool("SHOWSHEET"),
        },
        outline: Outline::new(vertices_from_props(&p)),
        copper_layer_count: copper.len(),
        copper_layers: copper,
        stackup,
        layers,
        layer_pairs,
        plane_nets,
        saved,
        props: p,
    };
    (b, warnings)
}
