//! Simple SVG preview of a board or footprint: outline, tracks, arcs, pads, vias, fills,
//! regions and texts, one `<g id="LayerName">` per layer drawn bottom-up. Geometry is exact
//! (pad shapes, rotations, arc sweeps, mirrored Y axis); clearances and pour fills are not
//! recomputed (poured copper is drawn from its stored regions).
//!
//! SVG user units are mils with the origin at the drawing's top-left corner.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use altium_core::units::{Coord, PER_MIL};

use super::geom::{BBoxAcc, Vertex, ccw_sweep, flatten, rotate};
use super::layer::{Layer, LayerKind};
use super::prim::{Arc, Fill, Pad, Region, Text, Track, Via};
use super::PcbDoc;

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Layers to draw; empty = copper, overlays, keep-out and multi-layer.
    pub layers: Vec<Layer>,
    /// Draw a frame around every text (its stored frame, else an estimated box).
    pub text_boxes: bool,
}

pub struct Rendered {
    pub svg: String,
    pub layers: Vec<Layer>,
    pub width: Coord,
    pub height: Coord,
    pub elements: usize,
}

/// Primitives to draw.
pub struct Scene<'a> {
    pub outline: &'a [Vertex],
    pub pads: Vec<&'a Pad>,
    pub vias: Vec<&'a Via>,
    pub tracks: Vec<&'a Track>,
    pub arcs: Vec<&'a Arc>,
    pub fills: Vec<&'a Fill>,
    pub regions: Vec<&'a Region>,
    pub texts: Vec<&'a Text>,
    pub copper: Vec<Layer>,
}

pub fn color(l: Layer) -> &'static str {
    match l {
        Layer::TOP => "#c83232",
        Layer::BOTTOM => "#3264c8",
        Layer::TOP_OVERLAY => "#d4b000",
        Layer::BOTTOM_OVERLAY => "#8a7000",
        Layer::TOP_PASTE | Layer::BOTTOM_PASTE => "#808080",
        Layer::TOP_SOLDER | Layer::BOTTOM_SOLDER => "#7a2a7a",
        Layer::KEEPOUT => "#d020d0",
        Layer::MULTI => "#a0a0a0",
        _ => match l.kind() {
            LayerKind::Copper => ["#c8a032", "#32a050", "#a03290", "#3296a0"][(l.0 as usize) % 4],
            LayerKind::Plane => "#50a050",
            LayerKind::Mechanical => "#b04080",
            LayerKind::Drill => "#404040",
            _ => "#606060",
        },
    }
}

struct Xf {
    minx: f64,
    maxy: f64,
}

impl Xf {
    fn x(&self, v: i64) -> f64 {
        (v as f64 - self.minx) / PER_MIL
    }
    fn y(&self, v: i64) -> f64 {
        (self.maxy - v as f64) / PER_MIL
    }
    fn xf(&self, v: f64) -> f64 {
        (v - self.minx) / PER_MIL
    }
    fn yf(&self, v: f64) -> f64 {
        (self.maxy - v) / PER_MIL
    }
}

fn f(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn path_of(xf: &Xf, vs: &[Vertex], out: &mut String) {
    let (pts, _) = flatten(vs, true);
    for (i, p) in pts.iter().enumerate() {
        let _ = write!(out, "{}{} {} ", if i == 0 { "M" } else { "L" }, f(xf.xf(p.0)), f(xf.yf(p.1)));
    }
    out.push('Z');
}

#[allow(clippy::too_many_arguments)]
fn pad_shape(xf: &Xf, cx: i64, cy: i64, shape: &str, sx: i64, sy: i64, rot: f64, cr: Option<u8>, fill: &str) -> String {
    let (x, y) = (xf.x(cx), xf.y(cy));
    let (w, h) = (sx as f64 / PER_MIL, sy as f64 / PER_MIL);
    let tr = if rot != 0.0 { format!(" transform=\"rotate({} {} {})\"", f(-rot), f(x), f(y)) } else { String::new() };
    match shape {
        "round" if (w - h).abs() < 1e-6 => format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{fill}\"/>", f(x), f(y), f(w / 2.0)),
        "octagonal" => {
            let c = w.min(h) / 4.0;
            let (l, r, t, b) = (x - w / 2.0, x + w / 2.0, y - h / 2.0, y + h / 2.0);
            let pts = [(l + c, t), (r - c, t), (r, t + c), (r, b - c), (r - c, b), (l + c, b), (l, b - c), (l, t + c)];
            let p: Vec<String> = pts.iter().map(|(a, b)| format!("{},{}", f(*a), f(*b))).collect();
            format!("<polygon points=\"{}\" fill=\"{fill}\"{tr}/>", p.join(" "))
        }
        _ => {
            let rx = match shape {
                "round" => w.min(h) / 2.0,
                "rounded_rectangle" => w.min(h) * cr.unwrap_or(0) as f64 / 200.0,
                _ => 0.0,
            };
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{} fill=\"{fill}\"{tr}/>",
                f(x - w / 2.0),
                f(y - h / 2.0),
                f(w),
                f(h),
                if rx > 0.0 { format!(" rx=\"{}\"", f(rx)) } else { String::new() }
            )
        }
    }
}

fn hole(xf: &Xf, p: &Pad) -> Option<String> {
    let d = p.hole_size?.0;
    let (cx, cy) = (p.x.0 + p.hole_offset.map(|o| o.x.0).unwrap_or(0), p.y.0 + p.hole_offset.map(|o| o.y.0).unwrap_or(0));
    if let (Some("slot"), Some(len)) = (p.hole_shape, p.slot_length) {
        let rot = p.rotation + p.slot_rotation.unwrap_or(0.0);
        return Some(pad_shape(xf, cx, cy, "round", len.0.max(d), d, rot, None, "#ffffff"));
    }
    let shape = if p.hole_shape == Some("square") { "rectangular" } else { "round" };
    Some(pad_shape(xf, cx, cy, shape, d, d, p.rotation, None, "#ffffff"))
}

/// Render a scene. `layers` empty = defaults.
pub fn render(scene: &Scene, opts: &Options) -> Rendered {
    let mut layers: Vec<Layer> = if opts.layers.is_empty() {
        let mut v = scene.copper.clone();
        v.extend([Layer::TOP_OVERLAY, Layer::BOTTOM_OVERLAY, Layer::KEEPOUT, Layer::MULTI]);
        v
    } else {
        opts.layers.clone()
    };
    let copper_selected = layers.iter().any(|l| l.is_copper());
    if copper_selected && !layers.contains(&Layer::MULTI) && opts.layers.is_empty() {
        layers.push(Layer::MULTI);
    }
    // Draw order: bottom side first, then inner, then top, then non-copper.
    let rank = |l: &Layer| -> (u8, i64) {
        match l.kind() {
            LayerKind::Copper | LayerKind::Plane => (1, -(l.order().1 as i64)),
            LayerKind::Multi => (2, 0),
            _ => {
                let bottom = l.side() == Some("bottom");
                (if bottom { 0 } else { 3 }, l.order().1 as i64)
            }
        }
    };
    layers.sort_by_key(rank);
    layers.dedup();

    let on = |l: Layer| layers.contains(&l);
    // Extent: outline, else everything drawn.
    let mut acc = BBoxAcc::default();
    let (opts_pts, _) = flatten(scene.outline, true);
    for p in &opts_pts {
        acc.add_f(p.0, p.1);
    }
    let has_outline = opts_pts.len() >= 3;
    for t in scene.tracks.iter().filter(|_| !has_outline) {
        if on(t.layer) {
            acc.add_r(t.start.x.0, t.start.y.0, t.width.0 / 2);
            acc.add_r(t.end.x.0, t.end.y.0, t.width.0 / 2);
        }
    }
    for p in scene.pads.iter().filter(|_| !has_outline) {
        acc.add_r(p.x.0, p.y.0, p.size.x.0.max(p.size.y.0) / 2);
    }
    for a in scene.arcs.iter().filter(|_| !has_outline) {
        if on(a.layer) {
            acc.add_r(a.center.x.0, a.center.y.0, a.radius.0 + a.width.0 / 2);
        }
    }
    for r in scene.regions.iter().filter(|_| !has_outline) {
        if on(r.layer)
            && let Some(b) = r.bbox {
                acc.add(b.min.x.0, b.min.y.0);
                acc.add(b.max.x.0, b.max.y.0);
            }
    }
    for t in scene.texts.iter().filter(|_| !has_outline) {
        if on(t.layer) {
            acc.add(t.x.0, t.y.0);
        }
    }
    if !acc.any {
        acc.add(0, 0);
        acc.add(10_000, 10_000);
    }
    let margin = ((acc.hi.0 - acc.lo.0).max(acc.hi.1 - acc.lo.1) / 40).max(50_000);
    let xf = Xf { minx: (acc.lo.0 - margin) as f64, maxy: (acc.hi.1 + margin) as f64 };
    let w = (acc.hi.0 - acc.lo.0 + 2 * margin) as f64 / PER_MIL;
    let h = (acc.hi.1 - acc.lo.1 + 2 * margin) as f64 / PER_MIL;

    let mut groups: BTreeMap<usize, String> = BTreeMap::new();
    let mut elements = 0usize;
    let idx = |l: Layer| layers.iter().position(|x| *x == l);
    let mut push = |l: Layer, s: String| {
        if let Some(i) = idx(l) {
            groups.entry(i).or_default().push_str(&s);
            groups.get_mut(&i).unwrap().push('\n');
            elements += 1;
        }
    };

    for r in &scene.regions {
        if r.source == "board_regions" || r.kind == "board_cutout" || r.outline.len() < 3 {
            continue;
        }
        let mut d = String::new();
        path_of(&xf, &r.outline, &mut d);
        for hole in &r.holes {
            d.push(' ');
            path_of(&xf, hole, &mut d);
        }
        let op = if r.keepout || r.kind != "copper" { " fill-opacity=\"0.35\"" } else { "" };
        push(r.layer, format!("<path d=\"{d}\" fill-rule=\"evenodd\" fill=\"{}\"{op}/>", color(r.layer)));
    }
    for fl in &scene.fills {
        let (cx, cy) = ((fl.corner1.x.0 + fl.corner2.x.0) as f64 / 2.0, (fl.corner1.y.0 + fl.corner2.y.0) as f64 / 2.0);
        let corners = [
            (fl.corner1.x.0, fl.corner1.y.0),
            (fl.corner2.x.0, fl.corner1.y.0),
            (fl.corner2.x.0, fl.corner2.y.0),
            (fl.corner1.x.0, fl.corner2.y.0),
        ];
        let p: Vec<String> = corners
            .iter()
            .map(|(x, y)| {
                let (a, b) = rotate(*x as f64, *y as f64, cx, cy, fl.rotation);
                format!("{},{}", f(xf.xf(a)), f(xf.yf(b)))
            })
            .collect();
        push(fl.layer, format!("<polygon points=\"{}\" fill=\"{}\"/>", p.join(" "), color(fl.layer)));
    }
    for t in &scene.tracks {
        push(
            t.layer,
            format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\"/>",
                f(xf.x(t.start.x.0)),
                f(xf.y(t.start.y.0)),
                f(xf.x(t.end.x.0)),
                f(xf.y(t.end.y.0)),
                color(t.layer),
                f(t.width.0 as f64 / PER_MIL)
            ),
        );
    }
    for a in &scene.arcs {
        let sw = f(a.width.0 as f64 / PER_MIL);
        let r = a.radius.0 as f64 / PER_MIL;
        let sweep = ccw_sweep(a.start_angle, a.end_angle);
        let s = if sweep >= 359.999 {
            format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{sw}\"/>",
                f(xf.x(a.center.x.0)),
                f(xf.y(a.center.y.0)),
                f(r),
                color(a.layer)
            )
        } else {
            // CCW in Altium's Y-up frame is CCW on screen too, i.e. SVG sweep-flag 0.
            format!(
                "<path d=\"M{} {} A{} {} 0 {} 0 {} {}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{sw}\" stroke-linecap=\"round\"/>",
                f(xf.x(a.start.x.0)),
                f(xf.y(a.start.y.0)),
                f(r),
                f(r),
                if sweep > 180.0 { 1 } else { 0 },
                f(xf.x(a.end.x.0)),
                f(xf.y(a.end.y.0)),
                color(a.layer)
            )
        };
        push(a.layer, s);
    }
    for p in &scene.pads {
        let draw_on: Vec<Layer> = if p.layer == Layer::MULTI {
            if on(Layer::MULTI) { vec![Layer::MULTI] } else { layers.iter().copied().filter(|l| l.is_copper()).take(1).collect() }
        } else {
            vec![p.layer]
        };
        for l in draw_on {
            let (shape, size) = match (l, &p.shape_bottom, p.size_bottom) {
                (Layer::BOTTOM, Some(s), Some(z)) => (s.as_str(), z),
                _ => (p.shape.as_str(), p.size),
            };
            let mut s = pad_shape(&xf, p.x.0, p.y.0, shape, size.x.0, size.y.0, p.rotation, p.corner_radius_percent, color(l));
            if let Some(hs) = hole(&xf, p) {
                s.push_str(&hs);
            }
            push(l, s);
        }
    }
    for v in &scene.vias {
        let l = if on(Layer::MULTI) { Layer::MULTI } else { v.start_layer };
        push(
            l,
            format!(
                "<circle cx=\"{x}\" cy=\"{y}\" r=\"{}\" fill=\"{}\"/><circle cx=\"{x}\" cy=\"{y}\" r=\"{}\" fill=\"#ffffff\"/>",
                f(v.diameter.0 as f64 / PER_MIL / 2.0),
                color(l),
                f(v.hole_size.0 as f64 / PER_MIL / 2.0),
                x = f(xf.x(v.x.0)),
                y = f(xf.y(v.y.0)),
            ),
        );
    }
    for t in &scene.texts {
        if t.text.is_empty() {
            continue;
        }
        let (x, y) = (xf.x(t.x.0), xf.y(t.y.0));
        let hgt = t.height.0 as f64 / PER_MIL;
        let mut tr = format!("rotate({} {} {})", f(-t.rotation), f(x), f(y));
        if t.mirrored {
            let _ = write!(tr, " translate({} 0) scale(-1 1)", f(2.0 * x));
        }
        let line = t.text.lines().next().unwrap_or("");
        let mut s = format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\" font-family=\"{}\" fill=\"{}\" transform=\"{tr}\">{}</text>",
            f(x),
            f(y),
            f(hgt),
            esc(if t.font_name.is_empty() { "sans-serif" } else { &t.font_name }),
            color(t.layer),
            esc(line)
        );
        if opts.text_boxes {
            let (bw, bh) = match &t.frame {
                Some(fr) => (fr.x.0 as f64 / PER_MIL, fr.y.0 as f64 / PER_MIL),
                None => (hgt * 0.6 * line.chars().count() as f64, hgt),
            };
            let _ = write!(
                s,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1\" transform=\"{tr}\"/>",
                f(x),
                f(y - bh),
                f(bw),
                f(bh),
                color(t.layer)
            );
        }
        push(t.layer, s);
    }

    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}mil\" height=\"{}mil\">",
        f(w),
        f(h),
        f(w),
        f(h)
    );
    let _ = writeln!(svg, "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>", f(w), f(h));
    for (i, g) in &groups {
        let l = layers[*i];
        let _ = writeln!(svg, "<g id=\"{}\" opacity=\"0.85\">\n{g}</g>", l.name());
    }
    if scene.outline.len() >= 3 {
        let mut d = String::new();
        path_of(&xf, scene.outline, &mut d);
        let _ = writeln!(svg, "<path id=\"BoardOutline\" d=\"{d}\" fill=\"none\" stroke=\"#000000\" stroke-width=\"5\"/>");
        elements += 1;
    }
    for r in &scene.regions {
        if r.kind == "board_cutout" && r.outline.len() >= 3 {
            let mut d = String::new();
            path_of(&xf, &r.outline, &mut d);
            let _ = writeln!(svg, "<path class=\"cutout\" d=\"{d}\" fill=\"#ffffff\" stroke=\"#000000\" stroke-width=\"3\"/>");
            elements += 1;
        }
    }
    svg.push_str("</svg>\n");
    Rendered {
        svg,
        layers,
        width: Coord(acc.hi.0 - acc.lo.0),
        height: Coord(acc.hi.1 - acc.lo.1),
        elements,
    }
}

/// Render a whole board.
pub fn render_pcb(doc: &PcbDoc, opts: &Options) -> Rendered {
    let scene = Scene {
        outline: &doc.board.outline.vertices,
        pads: doc.pads.iter().collect(),
        vias: doc.vias.iter().collect(),
        tracks: doc.tracks.iter().collect(),
        arcs: doc.arcs.iter().collect(),
        fills: doc.fills.iter().collect(),
        regions: doc.regions.iter().collect(),
        // Designator/comment texts the component hides are not drawn.
        texts: doc
            .texts
            .iter()
            .filter(|t| {
                let c = t.owner.component_index.and_then(|i| doc.components.get(i as usize));
                match c {
                    Some(c) if t.is_designator => c.designator_visible,
                    Some(c) if t.is_comment => c.comment_visible,
                    _ => true,
                }
            })
            .collect(),
        copper: doc.board.copper_layers.clone(),
    };
    render(&scene, opts)
}
