//! Schematic record types and the small enumerations shared by SchDoc and SchLib records.
//!
//! Every value decoded here has been seen in real files (`.fixtures/samples`) or in files
//! saved by Altium Designer. Unknown codes are reported as `None` (callers keep the raw
//! number).

use altium_core::units::{Coord, Point};
use altium_format::props::Props;

/// Record type names by `RECORD=` number (snake_case, stable).
pub fn type_name(record: i64) -> &'static str {
    match record {
        0 => "header",
        1 => "component",
        2 => "pin",
        3 => "ieee_symbol",
        4 => "label",
        5 => "bezier",
        6 => "polyline",
        7 => "polygon",
        8 => "ellipse",
        9 => "piechart",
        10 => "round_rectangle",
        11 => "elliptical_arc",
        12 => "arc",
        13 => "line",
        14 => "rectangle",
        15 => "sheet_symbol",
        16 => "sheet_entry",
        17 => "power_port",
        18 => "port",
        22 => "no_erc",
        25 => "net_label",
        26 => "bus",
        27 => "wire",
        28 => "text_frame",
        29 => "junction",
        30 => "image",
        31 => "sheet",
        32 => "sheet_name",
        33 => "file_name",
        34 => "designator",
        37 => "bus_entry",
        39 => "template",
        41 => "parameter",
        43 => "parameter_set",
        44 => "implementation_list",
        45 => "implementation",
        46 => "map_definer_list",
        47 => "map_definer",
        48 => "impl_params",
        209 => "note",
        211 => "compile_mask",
        215 => "harness_connector",
        216 => "harness_entry",
        217 => "harness_type",
        218 => "signal_harness",
        225 => "blanket",
        226 => "hyperlink",
        _ => "unknown",
    }
}

/// Inverse of [`type_name`]; also accepts the number itself.
pub fn type_number(name: &str) -> Option<i64> {
    if let Ok(n) = name.trim().parse::<i64>() {
        return Some(n);
    }
    let want = name.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    (0..=300).find(|&n| type_name(n) == want)
}

/// Every known record type as `(number, name)`, for hints.
pub fn known_types() -> Vec<(i64, &'static str)> {
    (0..=300).filter(|&n| type_name(n) != "unknown").map(|n| (n, type_name(n))).collect()
}

/// `TRotateBy90` orientation (0..3) → degrees.
pub fn orientation_deg(v: i64) -> f64 {
    (v.rem_euclid(4) * 90) as f64
}

/// Unit vector of a `TRotateBy90` orientation (0 right, 1 up, 2 left, 3 down; y grows upwards).
pub fn orientation_dir(v: i64) -> (i64, i64) {
    match v.rem_euclid(4) {
        0 => (1, 0),
        1 => (0, 1),
        2 => (-1, 0),
        _ => (0, -1),
    }
}

/// Pin electrical type (`ELECTRICAL`, default 0 = input).
pub fn pin_electrical(v: i64) -> Option<&'static str> {
    Some(match v {
        0 => "input",
        1 => "io",
        2 => "output",
        3 => "open_collector",
        4 => "passive",
        5 => "hiz",
        6 => "open_emitter",
        7 => "power",
        _ => return None,
    })
}

/// IEEE pin symbol codes (`SYMBOL_INNER`, `SYMBOL_OUTER`, `SYMBOL_INNEREDGE`, `SYMBOL_OUTEREDGE`).
pub fn pin_symbol(v: i64) -> Option<&'static str> {
    Some(match v {
        1 => "dot",
        2 => "right_left_signal_flow",
        3 => "clock",
        4 => "active_low_input",
        5 => "analog_signal_in",
        6 => "not_logic_connection",
        8 => "postponed_output",
        9 => "open_collector",
        10 => "hiz",
        11 => "high_current",
        12 => "pulse",
        13 => "schmitt",
        17 => "active_low_output",
        22 => "open_collector_pull_up",
        23 => "open_emitter",
        24 => "open_emitter_pull_up",
        25 => "digital_signal_in",
        30 => "shift_left",
        32 => "open_output",
        33 => "left_right_signal_flow",
        34 => "bidirectional_signal_flow",
        _ => return None,
    })
}

/// Port / sheet entry `IOTYPE`.
pub fn io_type(v: i64) -> &'static str {
    match v {
        1 => "output",
        2 => "input",
        3 => "bidirectional",
        _ => "unspecified",
    }
}

/// Port / sheet entry `STYLE`.
pub fn port_style(v: i64) -> Option<&'static str> {
    Some(match v {
        0 => "none_horizontal",
        1 => "left",
        2 => "right",
        3 => "left_right",
        4 => "none_vertical",
        5 => "top",
        6 => "bottom",
        7 => "top_bottom",
        _ => return None,
    })
}

/// Sheet entry / harness `SIDE`.
pub fn side(v: i64) -> &'static str {
    match v {
        1 => "right",
        2 => "top",
        3 => "bottom",
        _ => "left",
    }
}

/// Power port `STYLE`.
pub fn power_style(v: i64) -> Option<&'static str> {
    Some(match v {
        0 => "circle",
        1 => "arrow",
        2 => "bar",
        3 => "wave",
        4 => "power_ground",
        5 => "signal_ground",
        6 => "earth",
        7 => "gost_arrow",
        8 => "gost_power_ground",
        9 => "gost_earth",
        10 => "gost_bar",
        _ => return None,
    })
}

/// Component `COMPONENTKIND` (Altium `TComponentKind`; missing = standard).
pub fn component_kind(v: i64) -> &'static str {
    match v {
        0 => "Standard",
        1 => "Mechanical",
        2 => "Graphical",
        3 => "Net Tie (In BOM)",
        4 => "Net Tie",
        5 => "Standard (No BOM)",
        6 => "Jumper",
        _ => "Unknown",
    }
}

/// Implementation `MODELTYPE` → stable kind.
pub fn model_kind(model_type: &str) -> &'static str {
    match model_type.to_ascii_uppercase().as_str() {
        "PCBLIB" => "footprint",
        "SIM" => "simulation",
        "SI" => "signal_integrity",
        "PCB3DLIB" => "3d",
        "IBIS" => "ibis",
        _ => "other",
    }
}

/// Predefined sheet sizes (`SHEETSTYLE`) in schematic units (10 mil), landscape.
pub fn sheet_style(v: i64) -> Option<(&'static str, i64, i64)> {
    Some(match v {
        0 => ("A4", 1150, 760),
        1 => ("A3", 1550, 1110),
        2 => ("A2", 2230, 1570),
        3 => ("A1", 3150, 2230),
        4 => ("A0", 4460, 3150),
        5 => ("A", 950, 750),
        6 => ("B", 1500, 950),
        7 => ("C", 2000, 1500),
        8 => ("D", 3200, 2000),
        9 => ("E", 4200, 3200),
        10 => ("Letter", 1100, 850),
        11 => ("Legal", 1400, 850),
        12 => ("Tabloid", 1700, 1100),
        13 => ("OrCAD A", 990, 790),
        14 => ("OrCAD B", 1540, 990),
        15 => ("OrCAD C", 2060, 1560),
        16 => ("OrCAD D", 3260, 2060),
        17 => ("OrCAD E", 4280, 3280),
        _ => return None,
    })
}

/// `LOCATION.X/Y` (+ `_FRAC`).
pub fn location(p: &Props) -> Point {
    Point::new(p.sch("LOCATION.X"), p.sch("LOCATION.Y"))
}

/// `LOCATION` when present at all (some parameters have no position).
pub fn location_opt(p: &Props) -> Option<Point> {
    (p.has("LOCATION.X") || p.has("LOCATION.Y")).then(|| location(p))
}

/// `CORNER.X/Y` (+ `_FRAC`).
pub fn corner(p: &Props) -> Point {
    Point::new(p.sch("CORNER.X"), p.sch("CORNER.Y"))
}

/// `LOCATIONCOUNT` + `Xn/Yn` (+ `_FRAC`), plus `EXTRALOCATIONCOUNT` + `EXn/EYn` (numbered after
/// the regular ones) used by long polygons.
pub fn points(p: &Props) -> Vec<Point> {
    let n = p.int_or("LOCATIONCOUNT", 0).max(0);
    let mut v: Vec<Point> =
        (1..=n).map(|i| Point::new(p.sch(&format!("X{i}")), p.sch(&format!("Y{i}")))).collect();
    let extra = p.int_or("EXTRALOCATIONCOUNT", 0).max(0);
    for i in n + 1..=n + extra {
        v.push(Point::new(p.sch(&format!("EX{i}")), p.sch(&format!("EY{i}"))));
    }
    v
}

/// `DISTANCEFROMTOP` of sheet / harness entries: integer part in units of 100 mil (ten
/// schematic units) plus `DISTANCEFROMTOP_FRAC1` in 1/100000 of 10 mil.
pub fn distance_from_top(p: &Props) -> Coord {
    Coord::from_sch(p.int_or("DISTANCEFROMTOP", 0) * 10, p.int_or("DISTANCEFROMTOP_FRAC1", 0))
}

/// Text frame / note text: `~1` encodes a line break.
pub fn frame_text(s: &str) -> String {
    s.replace("~1", "\n")
}

/// `OWNERINDEX` as a record index, when the record has one.
pub fn owner_index(p: &Props) -> Option<usize> {
    p.int("OWNERINDEX").filter(|&i| i >= 0).map(|i| i as usize)
}

/// Sub-part letter for a 1-based part id (`1` → `A`, `27` → `AA`).
pub fn part_letter(id: i64) -> String {
    if id <= 0 {
        return String::new();
    }
    let mut n = id;
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_roundtrip() {
        assert_eq!(type_number("net_label"), Some(25));
        assert_eq!(type_number("Net-Label"), Some(25));
        assert_eq!(type_number("209"), Some(209));
        assert_eq!(type_name(218), "signal_harness");
        assert!(known_types().len() > 40);
    }

    #[test]
    fn letters() {
        assert_eq!(part_letter(1), "A");
        assert_eq!(part_letter(2), "B");
        assert_eq!(part_letter(26), "Z");
        assert_eq!(part_letter(27), "AA");
        assert_eq!(part_letter(0), "");
    }

    #[test]
    fn coords() {
        let p = Props::parse(b"|LOCATIONCOUNT=2|X1=10|Y1=20|X2=30|X2_FRAC=50000|Y2=20|DISTANCEFROMTOP=2|DISTANCEFROMTOP_FRAC1=50000");
        let pts = points(&p);
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0].x.mil(), 100.0);
        assert_eq!(pts[1].x.mil(), 305.0);
        assert_eq!(distance_from_top(&p).mil(), 205.0);
        assert_eq!(orientation_dir(2), (-1, 0));
        assert_eq!(orientation_deg(3), 270.0);
    }
}
