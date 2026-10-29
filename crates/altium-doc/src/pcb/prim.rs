//! Binary PCB primitives (`Arcs6`, `Pads6`, `Vias6`, `Tracks6`, `Texts6`, `Fills6`,
//! `Regions6`, `ShapeBasedRegions6`, `BoardRegions`, `ComponentBodies6`, and the mixed
//! primitive stream of a PcbLib footprint).
//!
//! Every record is `u8 object type` followed by u32-length-prefixed subrecords. The layouts
//! below were checked against every sample in `.fixtures/samples`: record types, subrecord
//! lengths and the stream `Header` counts.
//! Coordinates are kept in Altium's frame (Y up) and units.

use std::collections::HashMap;

use serde::Serialize;

use altium_core::error::{Error, Result};
use altium_core::units::{Coord, Point};
use altium_format::cfbx::Bin;
use altium_format::props::Props;

use super::geom::{Area, BBox, Vertex, VertexArc, bbox_of, ccw_sweep, flatten, polygon_area};
use super::layer::Layer;

/// Object type byte that starts every binary primitive record.
pub mod rec {
    pub const ARC: u8 = 1;
    pub const PAD: u8 = 2;
    pub const VIA: u8 = 3;
    pub const TRACK: u8 = 4;
    pub const TEXT: u8 = 5;
    pub const FILL: u8 = 6;
    pub const REGION: u8 = 11;
    pub const BODY: u8 = 12;
}

/// `0xFFFF` in a net/component/polygon slot means "none".
pub const NONE16: u16 = 0xFFFF;

fn opt16(v: u16) -> Option<u16> {
    (v != NONE16).then_some(v)
}

fn nz(c: Coord) -> Option<Coord> {
    (c.0 != 0).then_some(c)
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn coord(b: &mut Bin) -> Result<Coord> {
    Ok(Coord(b.i32()? as i64))
}

fn point(b: &mut Bin) -> Result<Point> {
    Ok(Point::new(coord(b)?, coord(b)?))
}

/// UTF-16LE fixed-size buffer up to the first NUL.
fn utf16z(raw: &[u8]) -> String {
    let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
    String::from_utf16_lossy(&units)
}

/// Header shared by most primitives: layer, two flag bytes, net, polygon, component.
#[derive(Debug, Clone, Copy)]
struct Head {
    layer: u8,
    flags1: u8,
    flags2: u8,
    net: u16,
    polygon: u16,
    component: u16,
}

fn head(b: &mut Bin) -> Result<Head> {
    let h = Head {
        layer: b.u8()?,
        flags1: b.u8()?,
        flags2: b.u8()?,
        net: b.u16()?,
        polygon: b.u16()?,
        component: b.u16()?,
    };
    b.skip(4)?;
    Ok(h)
}

impl Head {
    /// Altium stores "unlocked" as bit 2 of the first flag byte.
    fn locked(&self) -> bool {
        self.flags1 & 0x04 == 0
    }
    fn keepout(&self) -> bool {
        self.flags2 == 2
    }
}

/// Decode the keepout restriction bit mask (tracks, arcs, fills, regions).
pub fn keepout_names(mask: u8) -> Vec<&'static str> {
    [(0x01, "via"), (0x02, "track"), (0x04, "copper"), (0x08, "smd_pad"), (0x10, "th_pad")]
        .iter()
        .filter(|(b, _)| mask & b != 0)
        .map(|(_, n)| *n)
        .collect()
}

/// References every primitive carries: net and owning component, by index and (once the
/// document resolves them) by name.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Owner {
    /// Net name (resolved from `Nets6`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<String>,
    /// Owning component's designator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// 0-based index into `Nets6` (absent = no net).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_index: Option<u16>,
    /// 0-based index into `Components6` (absent = free primitive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_index: Option<u16>,
    /// Primitive unique id (`UniqueIDPrimitiveInformation`), when stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<String>,
}

impl Owner {
    fn new(net: u16, component: u16) -> Owner {
        Owner { net_index: opt16(net), component_index: opt16(component), ..Default::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Size {
    pub x: Coord,
    pub y: Coord,
}

// ---------------------------------------------------------------------------------------- arcs

#[derive(Debug, Clone, Serialize)]
pub struct Arc {
    /// 0-based record index in its stream (stable reference).
    pub index: usize,
    pub layer: Layer,
    #[serde(flatten)]
    pub owner: Owner,
    pub center: Point,
    pub radius: Coord,
    /// Degrees, counter-clockwise from +X.
    pub start_angle: f64,
    pub end_angle: f64,
    pub width: Coord,
    pub start: Point,
    pub end: Point,
    /// Arc length along the centre line.
    pub length: Coord,
    pub locked: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub keepout: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keepout_restrictions: Vec<&'static str>,
    /// Part of a polygon pour's track fill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polygon: Option<u16>,
    #[serde(skip_serializing_if = "is_false")]
    pub polygon_outline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub union_index: Option<u32>,
}

fn arc_point(c: Point, r: Coord, deg: f64) -> Point {
    let a = deg.to_radians();
    Point::new(
        Coord(c.x.0 + (r.0 as f64 * a.cos()).round() as i64),
        Coord(c.y.0 + (r.0 as f64 * a.sin()).round() as i64),
    )
}

pub fn parse_arc(b: &mut Bin, index: usize) -> Result<Arc> {
    let mut s = b.sub()?;
    let h = head(&mut s)?;
    let center = point(&mut s)?;
    let radius = coord(&mut s)?;
    let start_angle = s.f64()?;
    let end_angle = s.f64()?;
    let width = coord(&mut s)?;
    let _subpoly = s.u16()?;
    let rem = s.remaining();
    let (mut union_index, mut v7) = (None, None);
    if rem >= 9 {
        s.skip(1)?;
        union_index = Some(s.u32()?);
        v7 = Some(s.u32()?);
    }
    let kr = if rem >= 10 { s.u8()? } else if h.keepout() { 0x1F } else { 0 };
    let sweep = ccw_sweep(start_angle, end_angle);
    Ok(Arc {
        index,
        layer: Layer::versioned(h.layer, v7),
        owner: Owner::new(h.net, h.component),
        center,
        radius,
        start_angle,
        end_angle,
        width,
        start: arc_point(center, radius, start_angle),
        end: arc_point(center, radius, end_angle),
        length: Coord((radius.0 as f64 * sweep.to_radians()).round() as i64),
        locked: h.locked(),
        keepout: h.keepout(),
        keepout_restrictions: if h.keepout() { keepout_names(kr) } else { vec![] },
        polygon: opt16(h.polygon),
        polygon_outline: h.flags1 & 0x02 != 0,
        union_index: union_index.filter(|&u| u != 0),
    })
}

// -------------------------------------------------------------------------------------- tracks

#[derive(Debug, Clone, Serialize)]
pub struct Track {
    pub index: usize,
    pub layer: Layer,
    #[serde(flatten)]
    pub owner: Owner,
    pub start: Point,
    pub end: Point,
    pub width: Coord,
    pub length: Coord,
    pub locked: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub keepout: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keepout_restrictions: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polygon: Option<u16>,
    #[serde(skip_serializing_if = "is_false")]
    pub polygon_outline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub union_index: Option<u32>,
}

pub fn parse_track(b: &mut Bin, index: usize) -> Result<Track> {
    let mut s = b.sub()?;
    let h = head(&mut s)?;
    let start = point(&mut s)?;
    let end = point(&mut s)?;
    let width = coord(&mut s)?;
    let _subpoly = s.u16()?;
    s.skip(1)?;
    let rem = s.remaining();
    let (mut union_index, mut v7) = (None, None);
    if rem >= 9 {
        union_index = Some(s.u32()?);
        s.skip(1)?;
        v7 = Some(s.u32()?);
    }
    let kr = if rem >= 10 { s.u8()? } else if h.keepout() { 0x1F } else { 0 };
    let dx = (end.x.0 - start.x.0) as f64;
    let dy = (end.y.0 - start.y.0) as f64;
    Ok(Track {
        index,
        layer: Layer::versioned(h.layer, v7),
        owner: Owner::new(h.net, h.component),
        start,
        end,
        width,
        length: Coord((dx * dx + dy * dy).sqrt().round() as i64),
        locked: h.locked(),
        keepout: h.keepout(),
        keepout_restrictions: if h.keepout() { keepout_names(kr) } else { vec![] },
        polygon: opt16(h.polygon),
        polygon_outline: h.flags1 & 0x02 != 0,
        union_index: union_index.filter(|&u| u != 0),
    })
}

// --------------------------------------------------------------------------------------- fills

#[derive(Debug, Clone, Serialize)]
pub struct Fill {
    pub index: usize,
    pub layer: Layer,
    #[serde(flatten)]
    pub owner: Owner,
    /// Corners before rotation.
    pub corner1: Point,
    pub corner2: Point,
    /// Degrees about the rectangle centre.
    pub rotation: f64,
    pub locked: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub keepout: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keepout_restrictions: Vec<&'static str>,
}

pub fn parse_fill(b: &mut Bin, index: usize) -> Result<Fill> {
    let mut s = b.sub()?;
    let h = head(&mut s)?;
    let corner1 = point(&mut s)?;
    let corner2 = point(&mut s)?;
    let rotation = s.f64()?;
    let rem = s.remaining();
    let mut v7 = None;
    if rem >= 9 {
        s.skip(5)?;
        v7 = Some(s.u32()?);
    }
    let kr = if rem >= 10 { s.u8()? } else if h.keepout() { 0x1F } else { 0 };
    Ok(Fill {
        index,
        layer: Layer::versioned(h.layer, v7),
        owner: Owner::new(h.net, h.component),
        corner1,
        corner2,
        rotation,
        locked: h.locked(),
        keepout: h.keepout(),
        keepout_restrictions: if h.keepout() { keepout_names(kr) } else { vec![] },
    })
}

// ---------------------------------------------------------------------------------------- vias

#[derive(Debug, Clone, Serialize)]
pub struct Via {
    pub index: usize,
    #[serde(flatten)]
    pub owner: Owner,
    pub x: Coord,
    pub y: Coord,
    pub diameter: Coord,
    pub hole_size: Coord,
    pub start_layer: Layer,
    pub end_layer: Layer,
    /// `simple`, `top_middle_bottom` or `full_stack`.
    pub mode: &'static str,
    /// Land diameter per copper layer (`TopLayer`, `MidLayerN`, `BottomLayer`) when the via is
    /// not `simple`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diameters: Vec<LayerSize>,
    pub tented_top: bool,
    pub tented_bottom: bool,
    pub locked: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub test_fab_top: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub test_fab_bottom: bool,
    /// Manual solder mask expansion (front/back); absent when the rule applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solder_mask_expansion: Option<ViaMask>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thermal_relief: Option<ThermalRelief>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerSize {
    pub layer: Layer,
    pub diameter: Coord,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViaMask {
    pub front: Coord,
    pub back: Coord,
    /// True when the expansion is measured from the drill hole instead of the via land.
    pub from_hole: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThermalRelief {
    pub air_gap: Coord,
    pub conductors: u8,
    pub conductor_width: Coord,
}

fn pad_mode(m: u8) -> &'static str {
    match m {
        0 => "simple",
        1 => "top_middle_bottom",
        2 => "full_stack",
        _ => "unknown",
    }
}

/// Copper layer for slot `i` of a 32-entry per-layer table (0 = top, 1..30 = mid, 31 = bottom).
fn stack_layer(i: usize) -> Layer {
    match i {
        0 => Layer::TOP,
        31 => Layer::BOTTOM,
        k => Layer::mid(k as u32),
    }
}

pub fn parse_via(b: &mut Bin, index: usize) -> Result<Via> {
    let mut s = b.sub()?;
    let len = s.buf.len();
    let h = head(&mut s)?;
    let pos = point(&mut s)?;
    let diameter = coord(&mut s)?;
    let hole_size = coord(&mut s)?;
    let start_layer = Layer::from_v6(s.u8()?);
    let end_layer = Layer::from_v6(s.u8()?);
    let mut v = Via {
        index,
        owner: Owner::new(h.net, h.component),
        x: pos.x,
        y: pos.y,
        diameter,
        hole_size,
        start_layer,
        end_layer,
        mode: "simple",
        diameters: vec![],
        tented_top: h.flags1 & 0x20 != 0,
        tented_bottom: h.flags1 & 0x40 != 0,
        locked: h.locked(),
        test_fab_top: h.flags1 & 0x80 != 0,
        test_fab_bottom: h.flags2 & 0x01 != 0,
        solder_mask_expansion: None,
        thermal_relief: None,
    };
    if len <= 74 {
        return Ok(v);
    }
    s.skip(1)?;
    let air_gap = coord(&mut s)?;
    let conductors = s.u8()?;
    s.skip(1)?;
    let conductor_width = coord(&mut s)?;
    v.thermal_relief = Some(ThermalRelief { air_gap, conductors, conductor_width });
    s.skip(8)?;
    s.skip(4)?;
    let front = coord(&mut s)?;
    s.skip(8)?;
    let manual = s.u8()? & 0x02 != 0;
    let from_hole = s.u8()? & 0x01 != 0;
    s.skip(6)?;
    let mode = s.u8()?;
    v.mode = pad_mode(mode);
    let mut dia = [Coord(0); 32];
    for d in dia.iter_mut() {
        *d = coord(&mut s)?;
    }
    if mode == 1 {
        v.diameters = vec![
            LayerSize { layer: Layer::TOP, diameter: dia[0] },
            LayerSize { layer: Layer::mid(1), diameter: dia[1] },
            LayerSize { layer: Layer::BOTTOM, diameter: dia[31] },
        ];
    } else if mode == 2 {
        v.diameters = dia.iter().enumerate().map(|(i, &d)| LayerSize { layer: stack_layer(i), diameter: d }).collect();
    }
    let mut back = front;
    if len >= 246 {
        s.skip(38)?;
        let linked = s.u8()? & 0x01 != 0;
        let b2 = coord(&mut s)?;
        if !linked {
            back = b2;
        }
    }
    if manual {
        v.solder_mask_expansion = Some(ViaMask { front, back, from_hole });
    }
    Ok(v)
}

// ---------------------------------------------------------------------------------------- pads

#[derive(Debug, Clone, Serialize)]
pub struct Pad {
    pub index: usize,
    /// Pad designator (`"1"`, `"A3"`, `"GND"`); empty for unnumbered pads.
    pub number: String,
    #[serde(flatten)]
    pub owner: Owner,
    pub layer: Layer,
    pub x: Coord,
    pub y: Coord,
    /// Degrees.
    pub rotation: f64,
    /// Top (or only) layer shape: `round`, `rectangular`, `octagonal`, `rounded_rectangle`.
    pub shape: String,
    pub size: Size,
    /// Rounded-rectangle corner radius as a percentage of half the smaller side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corner_radius_percent: Option<u8>,
    /// `simple`, `top_middle_bottom` or `full_stack`.
    pub mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape_mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_mid: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape_bottom: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bottom: Option<Size>,
    /// Per-copper-layer shapes for `full_stack` pads.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<PadLayer>,
    /// Drill diameter (slot width for slots). Absent for SMD pads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hole_size: Option<Coord>,
    /// `round`, `square` or `slot`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hole_shape: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_length: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_rotation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hole_offset: Option<Point>,
    pub plated: bool,
    pub paste_mask_expansion: MaskExpansion,
    pub solder_mask_expansion: MaskExpansion,
    pub tented_top: bool,
    pub tented_bottom: bool,
    pub locked: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub test_fab_top: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub test_fab_bottom: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PadLayer {
    pub layer: Layer,
    pub shape: String,
    pub size: Size,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corner_radius_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hole_offset: Option<Point>,
}

/// Pad mask expansion: `rule` (from design rules), `manual` (with `value`) or `none`.
#[derive(Debug, Clone, Serialize)]
pub struct MaskExpansion {
    pub mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Coord>,
}

fn mask(mode: u8, value: Coord) -> MaskExpansion {
    let m = match mode {
        0 => "none",
        1 => "rule",
        2 => "manual",
        _ => "unknown",
    };
    MaskExpansion { mode: m, value: (mode == 2).then_some(value) }
}

/// Pad shape name from the base shape byte and the alternate shape byte (9 = rounded rect).
pub fn pad_shape(shape: u8, alt: Option<u8>) -> String {
    if alt == Some(9) {
        return "rounded_rectangle".into();
    }
    match shape {
        1 => "round".into(),
        2 => "rectangular".into(),
        3 => "octagonal".into(),
        n => format!("unknown_{n}"),
    }
}

fn hole_shape(h: u8) -> Option<&'static str> {
    match h {
        0 => Some("round"),
        1 => Some("square"),
        2 => Some("slot"),
        _ => None,
    }
}

pub fn parse_pad(b: &mut Bin, index: usize) -> Result<Pad> {
    let mut s1 = b.sub()?;
    let number = if s1.remaining() > 0 { s1.pascal()? } else { String::new() };
    for _ in 0..3 {
        b.sub()?;
    }
    let mut s = b.sub()?;
    let len5 = s.buf.len();
    if len5 < 110 {
        return Err(Error::parse(format!("Pads6 subrecord 5 is {len5} bytes, expected at least 110")));
    }
    let h = head(&mut s)?;
    let pos = point(&mut s)?;
    let top = Size { x: coord(&mut s)?, y: coord(&mut s)? };
    let mid = Size { x: coord(&mut s)?, y: coord(&mut s)? };
    let bot = Size { x: coord(&mut s)?, y: coord(&mut s)? };
    let hole = coord(&mut s)?;
    let (ts, ms, bs) = (s.u8()?, s.u8()?, s.u8()?);
    let rotation = s.f64()?;
    let plated = s.u8()? != 0;
    s.skip(1)?;
    let mode = s.u8()?;
    s.skip(23)?;
    let paste_v = coord(&mut s)?;
    let solder_v = coord(&mut s)?;
    s.skip(7)?;
    let paste_m = s.u8()?;
    let solder_m = s.u8()?;
    s.skip(3)?;
    // Bytes 106..114 are not a hole rotation double in any sample (real files hold small
    // integers there), so they are left undecoded. From 118 bytes on, offset
    // 114 holds the pad's 32-bit layer id (verified: TopLayer, BottomLayer, MultiLayer,
    // TopPaste, KeepOutLayer pads all match their V6 byte).
    let mut v7 = None;
    if len5 >= 118 {
        s.skip(8)?;
        v7 = Some(s.u32()?);
    }

    let mut p = Pad {
        index,
        number,
        owner: Owner::new(h.net, h.component),
        layer: Layer::versioned(h.layer, v7),
        x: pos.x,
        y: pos.y,
        rotation,
        shape: pad_shape(ts, None),
        size: top,
        corner_radius_percent: None,
        mode: pad_mode(mode),
        shape_mid: None,
        size_mid: None,
        shape_bottom: None,
        size_bottom: None,
        stack: vec![],
        hole_size: nz(hole),
        hole_shape: None,
        slot_length: None,
        slot_rotation: None,
        hole_offset: None,
        plated,
        paste_mask_expansion: mask(paste_m, paste_v),
        solder_mask_expansion: mask(solder_m, solder_v),
        tented_top: h.flags1 & 0x20 != 0,
        tented_bottom: h.flags1 & 0x40 != 0,
        locked: h.locked(),
        test_fab_top: h.flags1 & 0x80 != 0,
        test_fab_bottom: h.flags2 & 0x01 != 0,
    };

    // Subrecord 6: per-layer size and shape table (absent or empty on simple pads).
    let mut s6 = b.sub()?;
    let mut alt = [0u8; 32];
    let mut corner = [0u8; 32];
    let mut offs = [Point::default(); 32];
    let mut inner = [(Size { x: Coord(0), y: Coord(0) }, 0u8); 29];
    let has6 = s6.buf.len() >= 596;
    if has6 {
        for i in inner.iter_mut() {
            i.0.x = coord(&mut s6)?;
        }
        for i in inner.iter_mut() {
            i.0.y = coord(&mut s6)?;
        }
        for i in inner.iter_mut() {
            i.1 = s6.u8()?;
        }
        s6.skip(1)?;
        let hs = s6.u8()?;
        let slot = coord(&mut s6)?;
        let slot_rot = s6.f64()?;
        for o in offs.iter_mut() {
            o.x = coord(&mut s6)?;
        }
        for o in offs.iter_mut() {
            o.y = coord(&mut s6)?;
        }
        s6.skip(1)?;
        for a in alt.iter_mut() {
            *a = s6.u8()?;
        }
        for c in corner.iter_mut() {
            *c = s6.u8()?;
        }
        if p.hole_size.is_some() {
            p.hole_shape = hole_shape(hs);
            if hs == 2 {
                p.slot_length = nz(slot);
                p.slot_rotation = Some(slot_rot);
            }
            p.hole_offset = (offs[0] != Point::default()).then_some(offs[0]);
        }
        p.shape = pad_shape(ts, Some(alt[0]));
        if alt[0] == 9 {
            p.corner_radius_percent = Some(corner[0]);
        }
    } else if p.hole_size.is_some() {
        p.hole_shape = Some("round");
    }
    if mode != 0 {
        p.shape_mid = Some(pad_shape(ms, has6.then_some(alt[1])));
        p.size_mid = Some(mid);
        p.shape_bottom = Some(pad_shape(bs, has6.then_some(alt[31])));
        p.size_bottom = Some(bot);
    }
    if mode == 2 && has6 {
        for i in 0..32 {
            let (shape, size) = match i {
                0 => (ts, top),
                31 => (bs, bot),
                k if k <= 29 => (inner[k - 1].1, inner[k - 1].0),
                _ => (ms, mid),
            };
            p.stack.push(PadLayer {
                layer: stack_layer(i),
                shape: pad_shape(shape, Some(alt[i])),
                size,
                corner_radius_percent: (alt[i] == 9).then_some(corner[i]),
                hole_offset: (offs[i] != Point::default()).then_some(offs[i]),
            });
        }
    }
    Ok(p)
}

// --------------------------------------------------------------------------------------- texts

#[derive(Debug, Clone, Serialize)]
pub struct Text {
    pub index: usize,
    pub layer: Layer,
    #[serde(flatten)]
    pub owner: Owner,
    /// Text as Altium stores it (special strings like `.Designator` unexpanded), CRLF → LF.
    pub text: String,
    pub x: Coord,
    pub y: Coord,
    pub height: Coord,
    /// Degrees.
    pub rotation: f64,
    pub mirrored: bool,
    /// `stroke`, `truetype` or `barcode`.
    pub font_type: &'static str,
    /// Stroke font: `default`, `sans_serif`, `serif`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_font: Option<&'static str>,
    pub stroke_width: Coord,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub font_name: String,
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub inverted: bool,
    /// The component's designator text.
    #[serde(skip_serializing_if = "is_false")]
    pub is_designator: bool,
    /// The component's comment text.
    #[serde(skip_serializing_if = "is_false")]
    pub is_comment: bool,
    /// Text frame (box) size, when the text is framed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<Size>,
    /// Justification inside the frame (`left_top` ... `right_bottom`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub justification: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub barcode: Option<Barcode>,
    /// Key into `WideStrings6` (text came from it when present there).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widestring_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Barcode {
    /// `code39` or `code128`.
    pub kind: &'static str,
    pub margin: Size,
    pub inverted: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub font_name: String,
}

fn text_position(v: u8) -> Option<&'static str> {
    Some(match v {
        1 => "left_top",
        2 => "left_center",
        3 => "left_bottom",
        4 => "center_top",
        5 => "center_center",
        6 => "center_bottom",
        7 => "right_top",
        8 => "right_center",
        9 => "right_bottom",
        _ => return None,
    })
}

pub fn parse_text(b: &mut Bin, index: usize, wide: &HashMap<u32, String>) -> Result<Text> {
    let mut s = b.sub()?;
    let len = s.buf.len();
    let layer = s.u8()?;
    s.skip(6)?;
    let component = s.u16()?;
    s.skip(4)?;
    let pos = point(&mut s)?;
    let height = coord(&mut s)?;
    let stroke_font = s.u16()?;
    let rotation = s.f64()?;
    let mirrored = s.u8()? != 0;
    let stroke_width = coord(&mut s)?;
    let mut t = Text {
        index,
        layer: Layer::from_v6(layer),
        owner: Owner::new(NONE16, component),
        text: String::new(),
        x: pos.x,
        y: pos.y,
        height,
        rotation,
        mirrored,
        font_type: "stroke",
        stroke_font: None,
        stroke_width,
        font_name: String::new(),
        bold: false,
        italic: false,
        inverted: false,
        is_designator: false,
        is_comment: false,
        frame: None,
        justification: None,
        barcode: None,
        widestring_index: None,
    };
    let mut font_type = 0u8;
    let mut v7 = None;
    if len >= 123 {
        t.is_comment = s.u8()? != 0;
        t.is_designator = s.u8()? != 0;
        s.skip(1)?;
        font_type = s.u8()?;
        t.bold = s.u8()? != 0;
        t.italic = s.u8()? != 0;
        t.font_name = utf16z(s.bytes(64)?);
        let inverted = s.u8()? != 0;
        let _margin = coord(&mut s)?;
        let wi = s.u32()?;
        s.skip(4)?;
        let _inverted_rect = s.u8()? != 0;
        let fw = coord(&mut s)?;
        let fh = coord(&mut s)?;
        let just = s.u8()?;
        let _offset = coord(&mut s)?;
        let rem = s.remaining();
        let mut barcode = None;
        if rem >= 93 {
            s.skip(8)?;
            let margin = Size { x: coord(&mut s)?, y: coord(&mut s)? };
            s.skip(4)?;
            let kind = if s.u8()? == 1 { "code128" } else { "code39" };
            s.skip(1)?;
            let binv = s.u8()? != 0;
            font_type = s.u8()?;
            let bfont = utf16z(s.bytes(64)?);
            s.skip(1)?;
            v7 = Some(s.u32()?);
            barcode = Some(Barcode { kind, margin, inverted: binv, font_name: bfont });
        }
        let framed = if rem >= 103 {
            let f = s.u8()? != 0;
            s.skip(1 + 8)?;
            f
        } else {
            fw.0 != 0 && fh.0 != 0
        };
        let just_valid = if rem >= 115 { s.u8()? != 0 } else { false };
        if framed {
            t.frame = Some(Size { x: fw, y: fh });
            t.justification = if just_valid { text_position(just) } else { Some("left_bottom") };
        }
        t.inverted = inverted;
        t.widestring_index = Some(wi);
        if font_type == 2 {
            t.barcode = barcode;
        }
    }
    t.font_type = match font_type {
        1 => "truetype",
        2 => "barcode",
        _ => "stroke",
    };
    if t.font_type == "stroke" {
        t.stroke_font = Some(match stroke_font {
            2 => "sans_serif",
            3 => "serif",
            _ => "default",
        });
    }
    t.layer = Layer::versioned(layer, v7);
    // Subrecord 2: legacy 8-bit text, superseded by the wide string table when present.
    let mut s2 = b.sub()?;
    let legacy = if s2.remaining() > 0 { s2.pascal()? } else { String::new() };
    t.text = match t.widestring_index.and_then(|i| wide.get(&i)) {
        Some(w) => w.clone(),
        None => {
            t.widestring_index = None;
            legacy
        }
    }
    .replace("\r\n", "\n");
    Ok(t)
}

// ------------------------------------------------------------------------------------- regions

#[derive(Debug, Clone, Serialize)]
pub struct Region {
    pub index: usize,
    /// Stream it came from: `regions`, `shape_based_regions`, `board_regions`, `footprint`.
    pub source: &'static str,
    pub layer: Layer,
    #[serde(flatten)]
    pub owner: Owner,
    /// `copper`, `polygon_cutout`, `dashed_outline`, `kind_3`, `cavity_definition`,
    /// `board_cutout`, `board_region` (a `BoardRegions` layer-stack region).
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Board regions (rigid-flex layer-stack regions): the layer stack they use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_stack: Option<String>,
    /// Owning polygon pour (index into `Polygons6`) for poured copper.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polygon: Option<u16>,
    pub locked: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub keepout: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keepout_restrictions: Vec<&'static str>,
    #[serde(skip_serializing_if = "is_false")]
    pub teardrop: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub shape_based: bool,
    pub outline: Vec<Vertex>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<Vertex>>,
    /// Outline area minus holes.
    pub area: Area,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

/// Layer of a record that carries a V6 byte plus a `V7_LAYER` name property.
fn text_layer(v6: u8, v7: Option<&str>) -> Layer {
    match v7.and_then(Layer::from_name) {
        Some(l) if l.kind() == super::layer::LayerKind::Mechanical && l.0 & 0xFFFF > 16 => l,
        Some(l) if v6 == 0 => l,
        _ => Layer::from_v6(v6),
    }
}

fn fpt(b: &mut Bin) -> Result<Vertex> {
    let x = b.f64()?;
    let y = b.f64()?;
    Ok(Vertex::pt(Coord(x.round() as i64), Coord(y.round() as i64)))
}

/// `extended`: ShapeBasedRegions6 layout (n+1 vertices with arc data) instead of plain f64
/// points.
pub fn parse_region(b: &mut Bin, index: usize, source: &'static str, extended: bool, keep_raw: bool) -> Result<Region> {
    let mut s = b.sub()?;
    let h = head(&mut s)?; // includes 4 of the 5 skipped bytes
    s.skip(1)?;
    let holecount = s.u16()?;
    s.skip(2)?;
    let props = s.props()?;
    let n = s.u32()? as usize;
    let mut outline = Vec::with_capacity(n + 1);
    if extended {
        for _ in 0..n + 1 {
            let round = s.u8()? != 0;
            let p = point(&mut s)?;
            let c = point(&mut s)?;
            let r = coord(&mut s)?;
            let a1 = s.f64()?;
            let a2 = s.f64()?;
            outline.push(Vertex {
                x: p.x,
                y: p.y,
                arc: round.then_some(VertexArc { center: c, radius: r, start_angle: a1, end_angle: a2 }),
            });
        }
        // The last vertex repeats the first to close the shape.
        if outline.len() > 1 && outline.first().map(|v| (v.x, v.y)) == outline.last().map(|v| (v.x, v.y)) {
            outline.pop();
        }
    } else {
        for _ in 0..n {
            outline.push(fpt(&mut s)?);
        }
    }
    let mut holes = vec![];
    for _ in 0..holecount {
        let k = s.u32()? as usize;
        let mut hole = Vec::with_capacity(k);
        for _ in 0..k {
            hole.push(fpt(&mut s)?);
        }
        holes.push(hole);
    }
    let pkind = props.int("KIND").unwrap_or(0);
    let cutout = props.bool("ISBOARDCUTOUT");
    let board_region = props.str("OBJECTKIND").eq_ignore_ascii_case("BoardRegion");
    let kind = match pkind {
        _ if board_region => "board_region".to_owned(),
        0 if cutout => "board_cutout".to_owned(),
        0 => "copper".to_owned(),
        1 => "polygon_cutout".to_owned(),
        2 => "dashed_outline".to_owned(),
        3 => "kind_3".to_owned(),
        4 => "cavity_definition".to_owned(),
        k => format!("kind_{k}"),
    };
    let layer = text_layer(h.layer, props.raw("V7_LAYER"));
    let (pts, _) = flatten(&outline, true);
    let mut area = polygon_area(&pts);
    for hole in &holes {
        area -= polygon_area(&flatten(hole, true).0);
    }
    let kr = props.int("KEEPOUTRESTRICTIONS").map(|v| v as u8).unwrap_or(0x1F);
    Ok(Region {
        index,
        source,
        layer,
        owner: Owner::new(h.net, h.component),
        kind,
        name: props.str("NAME").trim().to_owned(),
        layer_stack: props.get("LAYERSTACKID").filter(|_| board_region),
        polygon: opt16(h.polygon),
        locked: h.locked(),
        keepout: h.keepout(),
        keepout_restrictions: if h.keepout() { keepout_names(kr) } else { vec![] },
        teardrop: h.flags1 & 0x10 != 0,
        shape_based: props.bool("ISSHAPEBASED"),
        bbox: bbox_of(&pts),
        area: Area(area.max(0.0)),
        outline,
        holes,
        raw: keep_raw.then_some(props),
    })
}

// -------------------------------------------------------------------------------------- bodies

/// A 3D component body (extruded outline and/or a STEP model reference).
#[derive(Debug, Clone, Serialize)]
pub struct Body {
    pub index: usize,
    pub layer: Layer,
    #[serde(flatten)]
    pub owner: Owner,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub identifier: String,
    /// Model GUID (key into `Models/Data`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model_name: String,
    pub model_embedded: bool,
    /// Altium model type code (`MODEL.MODELTYPE`: 0 extruded, 1 STEP, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_type: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_checksum: Option<i64>,
    pub standoff_height: Coord,
    pub overall_height: Coord,
    /// Body height above its standoff.
    pub height: Coord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cavity_height: Option<Coord>,
    /// `BODYPROJECTION` code (0 = same side as the component, 1 = opposite side).
    pub projection: i64,
    /// Model placement: 2D position, 2D rotation, 3D rotation and Z offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_x: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_y: Option<Coord>,
    pub model_rotation: f64,
    pub model_rotation_x: f64,
    pub model_rotation_y: f64,
    pub model_rotation_z: f64,
    pub model_dz: Coord,
    /// Colour as Altium's BGR integer and opacity 0..1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    pub outline: Vec<Vertex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Props>,
}

/// Code-point list (`72,68,82`) to string, as used by `IDENTIFIER`.
pub use altium_format::props::codepoints;

pub fn parse_body(b: &mut Bin, index: usize, keep_raw: bool) -> Result<Body> {
    let mut s = b.sub()?;
    let h = head(&mut s)?; // 7 bytes + component + 4
    s.skip(5)?;
    let props = s.props()?;
    let mut outline = vec![];
    if s.remaining() >= 4 {
        let n = s.u32()? as usize;
        if s.remaining() == n * 16 {
            for _ in 0..n {
                outline.push(fpt(&mut s)?);
            }
        }
    }
    let layer = text_layer(h.layer, props.raw("V7_LAYER"));
    let standoff = props.len("STANDOFFHEIGHT").unwrap_or_default();
    let overall = props.len("OVERALLHEIGHT").unwrap_or_default();
    let (pts, _) = flatten(&outline, true);
    Ok(Body {
        index,
        layer,
        owner: Owner::new(NONE16, h.component),
        name: props.str("NAME").trim().to_owned(),
        identifier: codepoints(&props.str("IDENTIFIER")),
        model_id: props.str("MODELID"),
        model_name: props.str("MODEL.NAME"),
        model_embedded: props.bool("MODEL.EMBED"),
        model_type: props.int("MODEL.MODELTYPE"),
        model_source: props.str("MODEL.MODELSOURCE"),
        model_checksum: props.int("MODEL.CHECKSUM"),
        standoff_height: standoff,
        overall_height: overall,
        height: overall - standoff,
        cavity_height: props.len("CAVITYHEIGHT").filter(|c| c.0 != 0),
        projection: props.int("BODYPROJECTION").unwrap_or(0),
        model_x: props.len("MODEL.2D.X"),
        model_y: props.len("MODEL.2D.Y"),
        model_rotation: props.f64("MODEL.2D.ROTATION").unwrap_or(0.0),
        model_rotation_x: props.f64("MODEL.3D.ROTX").unwrap_or(0.0),
        model_rotation_y: props.f64("MODEL.3D.ROTY").unwrap_or(0.0),
        model_rotation_z: props.f64("MODEL.3D.ROTZ").unwrap_or(0.0),
        model_dz: props.len("MODEL.3D.DZ").unwrap_or_default(),
        color: props.int("BODYCOLOR3D"),
        opacity: props.f64("BODYOPACITY3D"),
        bbox: bbox_of(&pts),
        outline,
        raw: keep_raw.then_some(props),
    })
}

// ----------------------------------------------------------------------------- stream framing

/// Decode a single-type primitive stream. Stops at the first undecodable record with a warning
/// (records are not self-delimiting across subrecords, so resynchronising would be a guess).
pub fn parse_stream<T>(
    buf: &[u8],
    stream: &str,
    want: u8,
    warnings: &mut Vec<String>,
    mut f: impl FnMut(&mut Bin, usize) -> Result<T>,
) -> Vec<T> {
    let mut out = vec![];
    let mut b = Bin::new(buf);
    while b.remaining() > 0 {
        let at = b.pos;
        let t = match b.u8() {
            Ok(t) => t,
            Err(_) => break,
        };
        if t != want {
            warnings.push(format!(
                "{stream}: record {} at byte {at} has object type {t}, expected {want}; {} bytes not decoded",
                out.len(),
                buf.len() - at
            ));
            break;
        }
        match f(&mut b, out.len()) {
            Ok(v) => out.push(v),
            Err(e) => {
                warnings.push(format!("{stream}: record {} at byte {at}: {}; rest of stream skipped", out.len(), e.message));
                break;
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Little-endian record builder for synthetic primitives.
    #[derive(Default)]
    pub struct W(pub Vec<u8>);

    impl W {
        pub fn u8(mut self, v: u8) -> Self {
            self.0.push(v);
            self
        }
        pub fn u16(mut self, v: u16) -> Self {
            self.0.extend(v.to_le_bytes());
            self
        }
        pub fn u32(mut self, v: u32) -> Self {
            self.0.extend(v.to_le_bytes());
            self
        }
        pub fn i32(mut self, v: i32) -> Self {
            self.0.extend(v.to_le_bytes());
            self
        }
        pub fn f64(mut self, v: f64) -> Self {
            self.0.extend(v.to_le_bytes());
            self
        }
        pub fn zeros(mut self, n: usize) -> Self {
            self.0.extend(std::iter::repeat_n(0u8, n));
            self
        }
        pub fn bytes(mut self, b: &[u8]) -> Self {
            self.0.extend(b);
            self
        }
        /// u32 length + body.
        pub fn sub(self, body: W) -> Self {
            self.u32(body.0.len() as u32).bytes(&body.0)
        }
        pub fn pascal(self, s: &str) -> Self {
            self.u8(s.len() as u8).bytes(s.as_bytes())
        }
        pub fn props(self, s: &str) -> Self {
            let mut b = s.as_bytes().to_vec();
            b.push(0);
            self.u32(b.len() as u32).bytes(&b)
        }
        /// layer, flags1, flags2, net, polygon, component, 4 unknown bytes.
        pub fn head(self, layer: u8, flags1: u8, net: u16, polygon: u16, component: u16) -> Self {
            self.u8(layer).u8(flags1).u8(0).u16(net).u16(polygon).u16(component).zeros(4)
        }
    }

    fn one<T>(rec: W, f: impl FnOnce(&mut Bin) -> Result<T>) -> T {
        let mut b = Bin::new(&rec.0);
        let v = f(&mut b).unwrap();
        assert_eq!(b.remaining(), 0, "record not fully consumed");
        v
    }

    #[test]
    fn track_with_v7_tail() {
        let body = W::default()
            .head(1, 0x04, 3, NONE16, NONE16)
            .i32(100)
            .i32(200)
            .i32(400)
            .i32(600)
            .i32(50)
            .u16(0)
            .u8(0)
            .u32(7)
            .u8(0)
            .u32(0x0100_0001)
            .u8(0);
        let t = one(W::default().sub(body), |b| parse_track(b, 4));
        assert_eq!(t.index, 4);
        assert_eq!(t.layer, Layer::TOP);
        assert_eq!(t.length, Coord(500));
        assert_eq!(t.owner.net_index, Some(3));
        assert_eq!(t.owner.component_index, None);
        assert!(!t.locked);
        assert_eq!(t.union_index, Some(7));
        assert_eq!(t.polygon, None);
    }

    #[test]
    fn short_track_is_old_format() {
        let body = W::default().head(57, 0, NONE16, NONE16, 2).i32(0).i32(0).i32(0).i32(10).i32(5).u16(0).u8(0);
        let t = one(W::default().sub(body), |b| parse_track(b, 0));
        assert_eq!(t.layer.name(), "Mechanical1");
        assert!(t.locked);
        assert_eq!(t.owner.component_index, Some(2));
    }

    #[test]
    fn arc_geometry() {
        let body = W::default()
            .head(33, 0x04, NONE16, NONE16, NONE16)
            .i32(1000)
            .i32(2000)
            .i32(10_000)
            .f64(0.0)
            .f64(90.0)
            .i32(100)
            .u16(0);
        let a = one(W::default().sub(body), |b| parse_arc(b, 0));
        assert_eq!(a.layer, Layer::TOP_OVERLAY);
        assert_eq!(a.start, Point::new(Coord(11_000), Coord(2000)));
        assert_eq!(a.end, Point::new(Coord(1000), Coord(12_000)));
        assert_eq!(a.length, Coord((10_000.0 * std::f64::consts::FRAC_PI_2).round() as i64));
    }

    /// Subrecord 5 of a pad, 180 bytes, layer byte `l6`, 32-bit layer at 114.
    fn pad5(l6: u8, v7: u32, hole: i32) -> W {
        W::default()
            .head(l6, 0x24, 9, NONE16, 1)
            .i32(5000)
            .i32(-5000)
            .i32(600)
            .i32(2200)
            .i32(600)
            .i32(2200)
            .i32(600)
            .i32(2200)
            .i32(hole)
            .u8(1)
            .u8(1)
            .u8(1)
            .f64(90.0)
            .u8(1)
            .u8(0)
            .u8(0)
            .zeros(23)
            .i32(0)
            .i32(20_000)
            .zeros(7)
            .u8(1)
            .u8(2)
            .zeros(3)
            .zeros(8)
            .u32(v7)
            .zeros(180 - 118)
    }

    #[test]
    fn pad_180() {
        let rec = W::default()
            .sub(W::default().pascal("A1"))
            .sub(W::default().u8(0))
            .sub(W::default().bytes(b"|&|0"))
            .sub(W::default().u8(0))
            .sub(pad5(74, 0x0103_000F, 9000))
            .sub(W::default());
        let p = one(rec, |b| parse_pad(b, 2));
        assert_eq!(p.number, "A1");
        assert_eq!(p.layer, Layer::MULTI);
        assert_eq!(p.shape, "round");
        assert_eq!(p.size, Size { x: Coord(600), y: Coord(2200) });
        assert_eq!(p.rotation, 90.0);
        assert_eq!(p.hole_size, Some(Coord(9000)));
        assert_eq!(p.hole_shape, Some("round"));
        assert!(p.plated && p.tented_top && !p.tented_bottom && !p.locked);
        assert_eq!(p.paste_mask_expansion.mode, "rule");
        assert_eq!(p.solder_mask_expansion.mode, "manual");
        assert_eq!(p.solder_mask_expansion.value, Some(Coord(20_000)));
        assert_eq!(p.owner.net_index, Some(9));
        assert_eq!(p.owner.component_index, Some(1));
        assert_eq!(p.mode, "simple");
    }

    #[test]
    fn pad_size_and_shape_table() {
        let mut s6 = W::default().zeros(29 * 4 * 2 + 29 + 1).u8(2).i32(40_000).f64(0.0).zeros(32 * 4 * 2 + 1);
        let mut alt = [0u8; 32];
        alt[0] = 9;
        let mut cr = [0u8; 32];
        cr[0] = 50;
        s6 = s6.bytes(&alt).bytes(&cr);
        assert_eq!(s6.0.len(), 596);
        let rec = W::default()
            .sub(W::default().pascal("1"))
            .sub(W::default())
            .sub(W::default())
            .sub(W::default())
            .sub(pad5(74, 0x0103_000F, 9000))
            .sub(s6);
        let p = one(rec, |b| parse_pad(b, 0));
        assert_eq!(p.shape, "rounded_rectangle");
        assert_eq!(p.corner_radius_percent, Some(50));
        assert_eq!(p.hole_shape, Some("slot"));
        assert_eq!(p.slot_length, Some(Coord(40_000)));
    }

    #[test]
    fn via_simple_and_extended() {
        let base = || W::default().head(74, 0x60 | 0x04, 5, NONE16, NONE16).i32(10).i32(20).i32(6000).i32(3000).u8(1).u8(32);
        let v = one(W::default().sub(base()), |b| parse_via(b, 0));
        assert_eq!((v.diameter, v.hole_size), (Coord(6000), Coord(3000)));
        assert_eq!((v.start_layer, v.end_layer), (Layer::TOP, Layer::BOTTOM));
        assert!(v.tented_top && v.tented_bottom && !v.locked);
        assert_eq!(v.mode, "simple");
        // Extended: thermal relief, manual mask 4 mil from the hole edge, full stack mode.
        let ext = base()
            .u8(0)
            .i32(100_000)
            .u8(4)
            .u8(0)
            .i32(100_000)
            .zeros(8)
            .zeros(4)
            .i32(40_000)
            .zeros(8)
            .u8(0x02)
            .u8(0x01)
            .zeros(6)
            .u8(2)
            .bytes(&(0..32).flat_map(|i: i32| (6000 + i).to_le_bytes()).collect::<Vec<u8>>());
        let v = one(W::default().sub(ext), |b| parse_via(b, 1));
        assert_eq!(v.mode, "full_stack");
        assert_eq!(v.diameters.len(), 32);
        assert_eq!(v.diameters[31].layer, Layer::BOTTOM);
        assert_eq!(v.diameters[31].diameter, Coord(6031));
        let m = v.solder_mask_expansion.unwrap();
        assert_eq!((m.front, m.back, m.from_hole), (Coord(40_000), Coord(40_000), true));
        assert_eq!(v.thermal_relief.unwrap().conductors, 4);
    }

    fn text_rec(long: bool, wi: u32, legacy: &str) -> W {
        let mut s1 = W::default()
            .u8(33)
            .zeros(6)
            .u16(4)
            .zeros(4)
            .i32(1000)
            .i32(2000)
            .i32(600_000)
            .u16(2)
            .f64(45.0)
            .u8(1)
            .i32(100_000);
        if long {
            let mut font = "Arial".encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>();
            font.resize(64, 0);
            s1 = s1
                .u8(0)
                .u8(1)
                .u8(0)
                .u8(1)
                .u8(1)
                .u8(0)
                .bytes(&font)
                .u8(0)
                .i32(0)
                .u32(wi)
                .zeros(4)
                .u8(0)
                .i32(0)
                .i32(0)
                .u8(5)
                .i32(0);
            assert_eq!(s1.0.len(), 137, "fixed part of the extended (>= 123 byte) text layout");
        }
        W::default().sub(s1).sub(W::default().pascal(legacy))
    }

    #[test]
    fn texts_old_and_new() {
        let t = one(text_rec(false, 0, "OLD"), |b| parse_text(b, 0, &HashMap::new()));
        assert_eq!(t.text, "OLD");
        assert_eq!(t.layer, Layer::TOP_OVERLAY);
        assert_eq!(t.owner.component_index, Some(4));
        assert_eq!(t.rotation, 45.0);
        assert!(t.mirrored);
        assert_eq!(t.font_type, "stroke");
        assert_eq!(t.stroke_font, Some("sans_serif"));
        let wide: HashMap<u32, String> = [(7, "Ünïcode\r\nline".to_owned())].into();
        let t = one(text_rec(true, 7, "?"), |b| parse_text(b, 1, &wide));
        assert_eq!(t.text, "Ünïcode\nline");
        assert!(t.is_designator && !t.is_comment && t.bold);
        assert_eq!(t.font_type, "truetype");
        assert_eq!(t.font_name, "Arial");
        assert_eq!(t.widestring_index, Some(7));
        let t = one(text_rec(true, 9, "fallback"), |b| parse_text(b, 1, &wide));
        assert_eq!(t.text, "fallback");
        assert_eq!(t.widestring_index, None);
    }

    fn region_head(layer: u8, holes: u16) -> W {
        W::default().head(layer, 0x04, NONE16, 0, NONE16).u8(0).u16(holes).zeros(2)
    }

    #[test]
    fn flat_region_with_hole() {
        let sq = |w: W, s: f64| w.f64(0.0).f64(0.0).f64(s).f64(0.0).f64(s).f64(s).f64(0.0).f64(s);
        let body = region_head(1, 1).props("|V7_LAYER=TOP|KIND=0|ISSHAPEBASED=FALSE").u32(4);
        let body = sq(body, 100.0).u32(4);
        let body = sq(body, 10.0);
        let r = one(W::default().sub(body), |b| parse_region(b, 0, "regions", false, false));
        assert_eq!(r.kind, "copper");
        assert_eq!(r.layer, Layer::TOP);
        assert_eq!(r.polygon, Some(0));
        assert_eq!(r.outline.len(), 4);
        assert_eq!(r.holes.len(), 1);
        assert!((r.area.0 - (10_000.0 - 100.0)).abs() < 1e-6);
    }

    #[allow(clippy::too_many_arguments)]
    fn xv(w: W, round: u8, x: i32, y: i32, cx: i32, cy: i32, r: i32, a1: f64, a2: f64) -> W {
        w.u8(round).i32(x).i32(y).i32(cx).i32(cy).i32(r).f64(a1).f64(a2)
    }

    #[test]
    fn shape_based_region_with_arc() {
        // Half disc: (0,0) → (200,0) straight, then an arc back to (0,0) around (100,0).
        let body = region_head(1, 0).props("|KIND=1|ISSHAPEBASED=TRUE").u32(2);
        let body = xv(body, 0, 0, 0, 0, 0, 0, 0.0, 0.0);
        let body = xv(body, 1, 200, 0, 100, 0, 100, 0.0, 180.0);
        let body = xv(body, 0, 0, 0, 0, 0, 0, 0.0, 0.0);
        let r = one(W::default().sub(body), |b| parse_region(b, 0, "shape_based_regions", true, false));
        assert_eq!(r.kind, "polygon_cutout");
        assert!(r.shape_based);
        assert_eq!(r.outline.len(), 2, "closing vertex dropped");
        let half = std::f64::consts::PI * 100.0 * 100.0 / 2.0;
        assert!((r.area.0 - half).abs() / half < 0.01, "{}", r.area.0);
    }

    #[test]
    fn board_region_kind() {
        let body = region_head(56, 0).props("|KIND=0|ISBOARDCUTOUT=TRUE|OBJECTKIND=BoardRegion|LAYERSTACKID={S}").u32(0);
        let r = one(W::default().sub(body), |b| parse_region(b, 0, "board_regions", false, false));
        assert_eq!(r.kind, "board_region");
        assert_eq!(r.layer_stack.as_deref(), Some("{S}"));
        let body = region_head(56, 0).props("|KIND=0|ISBOARDCUTOUT=TRUE").u32(0);
        let r = one(W::default().sub(body), |b| parse_region(b, 0, "regions", false, false));
        assert_eq!(r.kind, "board_cutout");
    }

    #[test]
    fn body_outline() {
        let body = W::default()
            .head(69, 0x0c, NONE16, NONE16, 3)
            .zeros(5)
            .props("|V7_LAYER=MECHANICAL13|STANDOFFHEIGHT=1mil|OVERALLHEIGHT=11mil|MODELID={X}|IDENTIFIER=72,73|MODEL.EMBED=TRUE")
            .u32(2)
            .f64(1.0)
            .f64(2.0)
            .f64(3.0)
            .f64(4.0);
        let b = one(W::default().sub(body), |b| parse_body(b, 0, false));
        assert_eq!(b.layer.name(), "Mechanical13");
        assert_eq!(b.owner.component_index, Some(3));
        assert_eq!(b.height, Coord(100_000));
        assert_eq!(b.identifier, "HI");
        assert!(b.model_embedded);
        assert_eq!(b.outline.len(), 2);
    }

    #[test]
    fn stream_stops_on_wrong_type() {
        let rec = W::default().u8(rec::TRACK).sub(W::default().head(1, 0, 0, 0, 0).zeros(23)).u8(rec::ARC).u32(0);
        let mut w = vec![];
        let v = parse_stream(&rec.0, "Tracks6", rec::TRACK, &mut w, parse_track);
        assert_eq!(v.len(), 1);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("object type 1"), "{w:?}");
    }

    #[test]
    fn keepouts() {
        assert_eq!(keepout_names(0x1F), vec!["via", "track", "copper", "smd_pad", "th_pad"]);
        assert_eq!(codepoints("71,78,68"), "GND");
    }
}
