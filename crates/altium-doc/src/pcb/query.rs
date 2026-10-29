//! Derived views over a loaded [`PcbDoc`]: per-net detail and routed length, drill table,
//! per-layer statistics, layer usage.

use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::units::Coord;

use super::geom::BBoxAcc;
use super::layer::{Layer, LayerKind};
use super::{PcbDoc, part};

/// A pad seen from a net or component.
#[derive(Debug, Clone, Serialize)]
pub struct PadRef {
    /// `U1.3` style node name (`.3` for a free pad).
    pub node: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    pub number: String,
    pub layer: Layer,
    pub x: Coord,
    pub y: Coord,
    pub index: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetDetail {
    pub index: usize,
    pub name: String,
    pub node_count: usize,
    pub pads: Vec<PadRef>,
    pub tracks: usize,
    pub arcs: usize,
    pub vias: usize,
    pub fills: usize,
    pub regions: usize,
    /// Polygon pours on this net (`index:name@layer`).
    pub polygons: Vec<String>,
    /// Tracks + arcs on copper, excluding polygon-pour fill primitives.
    pub routed_length: Coord,
    pub track_length: Coord,
    pub arc_length: Coord,
    /// Copper layers carrying routing or pads of this net.
    pub layers: Vec<Layer>,
    /// Net classes (`Classes6`) listing the net.
    pub classes: Vec<String>,
    /// Differential-pair classes listing the net.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diff_pair_classes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_length: Option<Coord>,
}

/// Routed length of one net.
#[derive(Debug, Clone, Serialize)]
pub struct NetLength {
    pub net: String,
    pub routed_length: Coord,
    pub track_length: Coord,
    pub arc_length: Coord,
    pub tracks: usize,
    pub arcs: usize,
    pub vias: usize,
    pub layers: Vec<Layer>,
    /// Length per copper layer.
    pub by_layer: IndexMap<String, Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_length: Option<Coord>,
}

fn routing(doc: &PcbDoc, net: u16) -> NetLength {
    let mut track = 0i64;
    let mut arc = 0i64;
    let (mut nt, mut na) = (0, 0);
    let mut by_layer: BTreeMap<Layer, i64> = BTreeMap::new();
    for t in &doc.tracks {
        if t.owner.net_index == Some(net) && t.layer.is_copper() && t.polygon.is_none() && !t.keepout {
            track += t.length.0;
            nt += 1;
            *by_layer.entry(t.layer).or_default() += t.length.0;
        }
    }
    for a in &doc.arcs {
        if a.owner.net_index == Some(net) && a.layer.is_copper() && a.polygon.is_none() && !a.keepout {
            arc += a.length.0;
            na += 1;
            *by_layer.entry(a.layer).or_default() += a.length.0;
        }
    }
    let vias = doc.vias.iter().filter(|v| v.owner.net_index == Some(net)).count();
    let mut layers: Vec<Layer> = by_layer.keys().copied().collect();
    layers.sort_by_key(|l| l.order());
    let mut bl: Vec<(Layer, i64)> = by_layer.into_iter().collect();
    bl.sort_by_key(|(l, _)| l.order());
    let n = doc.nets.get(net as usize);
    NetLength {
        net: n.map(|n| n.name.clone()).unwrap_or_default(),
        routed_length: Coord(track + arc),
        track_length: Coord(track),
        arc_length: Coord(arc),
        tracks: nt,
        arcs: na,
        vias,
        layers,
        by_layer: bl.into_iter().map(|(l, v)| (l.name(), Coord(v))).collect(),
        target_length: n.and_then(|n| n.target_length),
    }
}

/// Routed length of every net (optionally only members of one net class).
pub fn lengths(doc: &PcbDoc, class: Option<&str>) -> Vec<NetLength> {
    let members: Option<BTreeSet<String>> = class.map(|c| {
        doc.classes
            .iter()
            .filter(|k| k.kind == "net" && k.name.eq_ignore_ascii_case(c))
            .flat_map(|k| k.members.iter().cloned())
            .collect()
    });
    doc.nets
        .iter()
        .filter(|n| members.as_ref().is_none_or(|m| m.contains(&n.name)))
        .map(|n| routing(doc, n.index as u16))
        .collect()
}

pub fn pad_ref(p: &super::Pad) -> PadRef {
    PadRef {
        node: format!("{}.{}", p.owner.component.clone().unwrap_or_default(), p.number),
        component: p.owner.component.clone(),
        number: p.number.clone(),
        layer: p.layer,
        x: p.x,
        y: p.y,
        index: p.index,
    }
}

pub fn net_detail(doc: &PcbDoc, net_index: usize) -> NetDetail {
    let ni = net_index as u16;
    let n = &doc.nets[net_index];
    let pads: Vec<PadRef> = doc.pads.iter().filter(|p| p.owner.net_index == Some(ni)).map(pad_ref).collect();
    let r = routing(doc, ni);
    let mut layers: BTreeSet<Layer> = r.layers.iter().copied().collect();
    for p in doc.pads.iter().filter(|p| p.owner.net_index == Some(ni)) {
        if p.layer == Layer::MULTI {
            layers.insert(Layer::TOP);
            layers.insert(Layer::BOTTOM);
        } else if p.layer.is_copper() {
            layers.insert(p.layer);
        }
    }
    for v in doc.vias.iter().filter(|v| v.owner.net_index == Some(ni)) {
        layers.insert(v.start_layer);
        layers.insert(v.end_layer);
    }
    let mut layers: Vec<Layer> = layers.into_iter().filter(|l| l.is_copper()).collect();
    layers.sort_by_key(|l| l.order());
    let class_of = |kind: &str| -> Vec<String> {
        doc.classes
            .iter()
            .filter(|c| c.kind == kind && !c.superclass && c.members.iter().any(|m| m == &n.name))
            .map(|c| c.name.clone())
            .collect()
    };
    let nodes: BTreeSet<&str> = pads.iter().filter(|p| p.component.is_some()).map(|p| p.node.as_str()).collect();
    NetDetail {
        index: net_index,
        name: n.name.clone(),
        node_count: nodes.len(),
        tracks: r.tracks,
        arcs: r.arcs,
        vias: r.vias,
        fills: doc.fills.iter().filter(|f| f.owner.net_index == Some(ni)).count(),
        regions: doc.regions.iter().filter(|f| f.owner.net_index == Some(ni)).count(),
        polygons: doc
            .polygons
            .iter()
            .filter(|p| p.net_index == Some(ni))
            .map(|p| format!("{}:{}@{}", p.index, p.name, p.layer))
            .collect(),
        routed_length: r.routed_length,
        track_length: r.track_length,
        arc_length: r.arc_length,
        layers,
        classes: class_of("net"),
        diff_pair_classes: class_of("diff_pair"),
        target_length: n.target_length,
        pads,
    }
}

/// One row of the drill table.
#[derive(Debug, Clone, Serialize)]
pub struct Hole {
    pub diameter: Coord,
    /// `round`, `square` or `slot`.
    pub shape: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_length: Option<Coord>,
    pub plated: bool,
    pub count: usize,
    pub pads: usize,
    pub vias: usize,
    /// Via layer spans / `MultiLayer` for through-hole pads.
    pub spans: Vec<String>,
}

pub fn holes(doc: &PcbDoc) -> Vec<Hole> {
    let mut map: BTreeMap<(i64, &'static str, i64, bool), Hole> = BTreeMap::new();
    for p in &doc.pads {
        let Some(d) = p.hole_size else { continue };
        let shape = p.hole_shape.unwrap_or("round");
        let slot = p.slot_length.filter(|_| shape == "slot");
        let span = p.layer.name();
        let h = map.entry((d.0, shape, slot.map(|s| s.0).unwrap_or(0), p.plated)).or_insert(Hole {
            diameter: d,
            shape,
            slot_length: slot,
            plated: p.plated,
            count: 0,
            pads: 0,
            vias: 0,
            spans: vec![],
        });
        h.count += 1;
        h.pads += 1;
        if !h.spans.contains(&span) {
            h.spans.push(span);
        }
    }
    for v in &doc.vias {
        if v.hole_size.0 == 0 {
            continue;
        }
        let span = format!("{}-{}", v.start_layer, v.end_layer);
        let h = map.entry((v.hole_size.0, "round", 0, true)).or_insert(Hole {
            diameter: v.hole_size,
            shape: "round",
            slot_length: None,
            plated: true,
            count: 0,
            pads: 0,
            vias: 0,
            spans: vec![],
        });
        h.count += 1;
        h.vias += 1;
        if !h.spans.contains(&span) {
            h.spans.push(span);
        }
    }
    map.into_values().collect()
}

/// Object counts on one layer.
#[derive(Debug, Clone, Serialize, Default)]
pub struct LayerStats {
    pub layer: Layer,
    pub kind: Option<LayerKind>,
    pub pads: usize,
    pub tracks: usize,
    pub arcs: usize,
    pub fills: usize,
    pub regions: usize,
    pub texts: usize,
    pub polygons: usize,
    pub bodies: usize,
    pub dimensions: usize,
    /// Vias whose span starts or ends on this layer.
    pub vias: usize,
    pub total: usize,
}

pub fn layer_stats(doc: &PcbDoc) -> Vec<LayerStats> {
    let mut m: BTreeMap<Layer, LayerStats> = BTreeMap::new();
    fn get(m: &mut BTreeMap<Layer, LayerStats>, l: Layer) -> &mut LayerStats {
        m.entry(l).or_insert_with(|| LayerStats { layer: l, kind: Some(l.kind()), ..Default::default() })
    }
    for p in &doc.pads {
        get(&mut m, p.layer).pads += 1;
    }
    for p in &doc.tracks {
        get(&mut m, p.layer).tracks += 1;
    }
    for p in &doc.arcs {
        get(&mut m, p.layer).arcs += 1;
    }
    for p in &doc.fills {
        get(&mut m, p.layer).fills += 1;
    }
    for p in &doc.regions {
        get(&mut m, p.layer).regions += 1;
    }
    for p in &doc.texts {
        get(&mut m, p.layer).texts += 1;
    }
    for p in &doc.polygons {
        get(&mut m, p.layer).polygons += 1;
    }
    for p in &doc.bodies {
        get(&mut m, p.layer).bodies += 1;
    }
    for p in &doc.dimensions {
        get(&mut m, p.layer).dimensions += 1;
    }
    for v in &doc.vias {
        get(&mut m, v.start_layer).vias += 1;
        if v.end_layer != v.start_layer {
            get(&mut m, v.end_layer).vias += 1;
        }
    }
    let mut v: Vec<LayerStats> = m
        .into_values()
        .map(|mut s| {
            s.total = s.pads + s.tracks + s.arcs + s.fills + s.regions + s.texts + s.polygons + s.bodies + s.dimensions + s.vias;
            s
        })
        .collect();
    v.sort_by_key(|s| s.layer.order());
    v
}

/// Number of primitives per layer (vias counted on their start and end layers).
pub fn layer_usage(doc: &PcbDoc) -> IndexMap<Layer, usize> {
    layer_stats(doc).into_iter().map(|s| (s.layer, s.total)).collect()
}

/// Bounding box of every copper/overlay primitive (pads use their size, tracks their width).
pub fn extent(doc: &PcbDoc) -> BBoxAcc {
    let mut acc = BBoxAcc::default();
    for t in &doc.tracks {
        acc.add_r(t.start.x.0, t.start.y.0, t.width.0 / 2);
        acc.add_r(t.end.x.0, t.end.y.0, t.width.0 / 2);
    }
    for p in &doc.pads {
        let r = p.size.x.0.max(p.size.y.0) / 2;
        acc.add_r(p.x.0, p.y.0, r);
    }
    for v in &doc.vias {
        acc.add_r(v.x.0, v.y.0, v.diameter.0 / 2);
    }
    acc
}

/// Counts of every object kind actually decoded (keys are command names).
pub fn object_counts(doc: &PcbDoc) -> IndexMap<&'static str, usize> {
    let mut m = IndexMap::new();
    m.insert("components", doc.components.len());
    m.insert("nets", doc.nets.len());
    let w = |p: u32| doc.loaded & p != 0;
    if w(part::PADS) {
        m.insert("pads", doc.pads.len());
    }
    if w(part::VIAS) {
        m.insert("vias", doc.vias.len());
    }
    if w(part::TRACKS) {
        m.insert("tracks", doc.tracks.len());
    }
    if w(part::ARCS) {
        m.insert("arcs", doc.arcs.len());
    }
    if w(part::FILLS) {
        m.insert("fills", doc.fills.len());
    }
    m.insert("texts", doc.texts.len());
    if w(part::REGIONS) {
        m.insert("regions", doc.regions.iter().filter(|r| r.source != "board_regions").count());
        m.insert("board_regions", doc.regions.iter().filter(|r| r.source == "board_regions").count());
    }
    if w(part::POLYGONS) {
        m.insert("polygons", doc.polygons.len());
    }
    if w(part::DIMENSIONS) {
        m.insert("dimensions", doc.dimensions.len());
    }
    if w(part::BODIES) {
        m.insert("bodies", doc.bodies.len());
    }
    if w(part::MODELS) {
        m.insert("models", doc.models.len());
    }
    if w(part::CLASSES) {
        m.insert("classes", doc.classes.len());
    }
    if w(part::RULES) {
        m.insert("rules", doc.rules.len());
        m.insert("rooms", doc.rooms.len());
    }
    if w(part::DIFFPAIRS) {
        m.insert("diff_pairs", doc.diff_pairs.len());
    }
    m
}
