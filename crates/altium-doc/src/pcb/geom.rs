//! Geometry shared by PCB and footprint primitives: outlines with arc edges, bounding boxes,
//! areas and lengths. All computation is in Altium internal units.

use serde::{Serialize, Serializer};

use altium_core::units::{Coord, PER_MIL, PER_MM, Point, Unit, output_unit, round_to};
use altium_format::props::Props;

/// One outline vertex. When `arc` is present the edge from this vertex to the next one is a
/// circular arc (Altium `KINDn=1`); otherwise it is a straight line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Vertex {
    pub x: Coord,
    pub y: Coord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arc: Option<VertexArc>,
}

/// Arc edge description. Altium arcs run counter-clockwise from `start_angle` to `end_angle`
/// (degrees, 0 = +X); the edge may traverse it either way.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VertexArc {
    pub center: Point,
    pub radius: Coord,
    pub start_angle: f64,
    pub end_angle: f64,
}

impl Vertex {
    pub fn pt(x: Coord, y: Coord) -> Vertex {
        Vertex { x, y, arc: None }
    }
}

/// An area. Serialises in the square of the output unit (`mm²`, `mil²` or raw²).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Area(pub f64);

impl Serialize for Area {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match output_unit() {
            Unit::Mm => s.serialize_f64(round_to(self.0 / (PER_MM * PER_MM), 6)),
            Unit::Mil => s.serialize_f64(round_to(self.0 / (PER_MIL * PER_MIL), 3)),
            Unit::Raw => s.serialize_f64(self.0.round()),
        }
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BBox {
    pub min: Point,
    pub max: Point,
    pub width: Coord,
    pub height: Coord,
}

/// Incremental bounding-box builder.
#[derive(Debug, Clone, Copy, Default)]
pub struct BBoxAcc {
    pub lo: (i64, i64),
    pub hi: (i64, i64),
    pub any: bool,
}

impl BBoxAcc {
    pub fn add(&mut self, x: i64, y: i64) {
        if !self.any {
            self.lo = (x, y);
            self.hi = (x, y);
            self.any = true;
        } else {
            self.lo = (self.lo.0.min(x), self.lo.1.min(y));
            self.hi = (self.hi.0.max(x), self.hi.1.max(y));
        }
    }
    pub fn add_f(&mut self, x: f64, y: f64) {
        self.add(x.round() as i64, y.round() as i64);
    }
    /// Add a disc (point with radius), e.g. a round track end.
    pub fn add_r(&mut self, x: i64, y: i64, r: i64) {
        self.add(x - r, y - r);
        self.add(x + r, y + r);
    }
    pub fn merge(&mut self, o: &BBoxAcc) {
        if o.any {
            self.add(o.lo.0, o.lo.1);
            self.add(o.hi.0, o.hi.1);
        }
    }
    pub fn get(&self) -> Option<BBox> {
        self.any.then(|| BBox {
            min: Point::new(Coord(self.lo.0), Coord(self.lo.1)),
            max: Point::new(Coord(self.hi.0), Coord(self.hi.1)),
            width: Coord(self.hi.0 - self.lo.0),
            height: Coord(self.hi.1 - self.lo.1),
        })
    }
}

/// Read the `KINDn/VXn/VYn/CXn/CYn/Rn/SAn/EAn` vertex series used by Board6 outlines,
/// Polygons6 and room rules.
pub fn vertices_from_props(p: &Props) -> Vec<Vertex> {
    let mut out = vec![];
    for i in 0.. {
        let (Some(x), Some(y)) = (p.len(&format!("VX{i}")), p.len(&format!("VY{i}"))) else { break };
        let arc = (p.int(&format!("KIND{i}")).unwrap_or(0) != 0).then(|| VertexArc {
            center: Point::new(
                p.len(&format!("CX{i}")).unwrap_or_default(),
                p.len(&format!("CY{i}")).unwrap_or_default(),
            ),
            radius: p.len(&format!("R{i}")).unwrap_or_default(),
            start_angle: p.f64(&format!("SA{i}")).unwrap_or(0.0),
            end_angle: p.f64(&format!("EA{i}")).unwrap_or(0.0),
        });
        out.push(Vertex { x, y, arc });
    }
    out
}

/// Counter-clockwise sweep from `sa` to `ea` in degrees, in (0, 360].
pub fn ccw_sweep(sa: f64, ea: f64) -> f64 {
    let mut d = (ea - sa) % 360.0;
    if d <= 1e-9 {
        d += 360.0;
    }
    d
}

fn polar(c: (f64, f64), r: f64, deg: f64) -> (f64, f64) {
    let a = deg.to_radians();
    (c.0 + r * a.cos(), c.1 + r * a.sin())
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Points along an arc edge from `from` to `to` (exclusive of `from`, inclusive of `to`), and
/// the edge length.
fn arc_edge(from: (f64, f64), to: (f64, f64), a: &VertexArc, out: &mut Vec<(f64, f64)>) -> f64 {
    let c = (a.center.x.0 as f64, a.center.y.0 as f64);
    let r = a.radius.0 as f64;
    let sweep = ccw_sweep(a.start_angle, a.end_angle);
    if r <= 0.0 {
        out.push(to);
        return dist(from, to);
    }
    // Traverse CCW from SA when `from` sits at the start angle, otherwise CW from EA.
    let at_start = dist(from, polar(c, r, a.start_angle)) <= dist(from, polar(c, r, a.end_angle));
    let (a0, dir) = if at_start { (a.start_angle, 1.0) } else { (a.end_angle, -1.0) };
    let steps = ((sweep / 5.0).ceil() as usize).max(2);
    for k in 1..steps {
        out.push(polar(c, r, a0 + dir * sweep * k as f64 / steps as f64));
    }
    out.push(to);
    r * sweep.to_radians()
}

/// Flatten a vertex chain (arcs discretised every ≤5°) and return the points and the exact
/// perimeter (arcs measured as arcs). A closed chain gets its closing edge.
pub fn flatten(vs: &[Vertex], closed: bool) -> (Vec<(f64, f64)>, f64) {
    let mut pts = vec![];
    let mut len = 0.0;
    if vs.is_empty() {
        return (pts, 0.0);
    }
    let p = |v: &Vertex| (v.x.0 as f64, v.y.0 as f64);
    pts.push(p(&vs[0]));
    let n = vs.len();
    let edges = if closed { n } else { n - 1 };
    for i in 0..edges {
        let a = &vs[i];
        let b = &vs[(i + 1) % n];
        match &a.arc {
            Some(arc) => len += arc_edge(p(a), p(b), arc, &mut pts),
            None => {
                len += dist(p(a), p(b));
                pts.push(p(b));
            }
        }
    }
    if closed && pts.len() > 1 && dist(pts[0], *pts.last().unwrap()) < 1.0 {
        pts.pop();
    }
    (pts, len)
}

/// Absolute shoelace area of a closed polygon.
pub fn polygon_area(pts: &[(f64, f64)]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        s += a.0 * b.1 - b.0 * a.1;
    }
    (s / 2.0).abs()
}

pub fn bbox_of(pts: &[(f64, f64)]) -> Option<BBox> {
    let mut acc = BBoxAcc::default();
    for p in pts {
        acc.add_f(p.0, p.1);
    }
    acc.get()
}

/// Summary of a closed outline: vertex list, flattened area, perimeter, bbox.
#[derive(Debug, Clone, Serialize)]
pub struct Outline {
    pub vertices: Vec<Vertex>,
    pub vertex_count: usize,
    /// Number of arc edges.
    pub arcs: usize,
    pub area: Area,
    pub perimeter: Coord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

impl Outline {
    pub fn new(vertices: Vec<Vertex>) -> Outline {
        let (pts, len) = flatten(&vertices, true);
        Outline {
            vertex_count: vertices.len(),
            arcs: vertices.iter().filter(|v| v.arc.is_some()).count(),
            area: Area(polygon_area(&pts)),
            perimeter: Coord(len.round() as i64),
            bbox: bbox_of(&pts),
            vertices,
        }
    }
}

/// Rotate `(x, y)` about `(cx, cy)` by `deg` degrees counter-clockwise.
pub fn rotate(x: f64, y: f64, cx: f64, cy: f64, deg: f64) -> (f64, f64) {
    if deg == 0.0 {
        return (x, y);
    }
    let (s, c) = deg.to_radians().sin_cos();
    let (dx, dy) = (x - cx, y - cy);
    (cx + dx * c - dy * s, cy + dx * s + dy * c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep() {
        assert_eq!(ccw_sweep(90.0, 180.0), 90.0);
        assert_eq!(ccw_sweep(270.0, 0.0), 90.0);
        assert_eq!(ccw_sweep(0.0, 360.0), 360.0);
        assert_eq!(ccw_sweep(10.0, 10.0), 360.0);
    }

    #[test]
    fn square_with_rounded_corner() {
        // 100x100 mil square whose first edge is a quarter arc of r=10mil (Board6 style: the
        // vertex sits at the arc's end angle, so the edge runs clockwise).
        let m = |v: f64| Coord::from_mil(v);
        let vs = vec![
            Vertex {
                x: m(0.0),
                y: m(90.0),
                arc: Some(VertexArc {
                    center: Point::new(m(10.0), m(90.0)),
                    radius: m(10.0),
                    start_angle: 90.0,
                    end_angle: 180.0,
                }),
            },
            Vertex::pt(m(10.0), m(100.0)),
            Vertex::pt(m(100.0), m(100.0)),
            Vertex::pt(m(100.0), m(0.0)),
            Vertex::pt(m(0.0), m(0.0)),
        ];
        let o = Outline::new(vs);
        let expect_area = 100.0 * 100.0 - (100.0 - std::f64::consts::PI * 100.0 / 4.0);
        let got = o.area.0 / (PER_MIL * PER_MIL);
        assert!((got - expect_area).abs() < 1.0, "{got} vs {expect_area}");
        let expect_len = 90.0 + 90.0 + 100.0 + 100.0 + std::f64::consts::PI * 10.0 / 2.0;
        assert!((o.perimeter.mil() - expect_len).abs() < 0.01);
        assert_eq!(o.bbox.unwrap().width, m(100.0));
    }
}
