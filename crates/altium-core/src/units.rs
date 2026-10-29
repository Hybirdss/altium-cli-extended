//! Length units.
//!
//! PCB documents store lengths as i32 in Altium internal units (1 unit = 0.0001 mil = 2.54 nm).
//! Schematic documents store integer coordinates in units of 10 mil plus an optional `_FRAC`
//! part in 1/100000 of that unit. Everything is normalised to [`Coord`] (internal units) and
//! converted to the caller's chosen output unit only when serialising.

use serde::{Serialize, Serializer};
use std::cell::Cell;

/// Internal units per mil.
pub const PER_MIL: f64 = 10_000.0;
/// Internal units per mm.
pub const PER_MM: f64 = PER_MIL * 1000.0 / 25.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    Mm,
    Mil,
    /// Raw Altium internal units (1e-4 mil). Lossless.
    Raw,
}

impl Unit {
    pub fn name(self) -> &'static str {
        match self {
            Unit::Mm => "mm",
            Unit::Mil => "mil",
            Unit::Raw => "raw",
        }
    }
}

thread_local! {
    static OUTPUT_UNIT: Cell<Unit> = const { Cell::new(Unit::Mm) };
}

/// Set the unit every [`Coord`] serialises to. Called once from `main` based on `--units`.
pub fn set_output_unit(u: Unit) {
    OUTPUT_UNIT.with(|c| c.set(u));
}

pub fn output_unit() -> Unit {
    OUTPUT_UNIT.with(|c| c.get())
}

/// A length or coordinate in Altium internal units. Serialises as a number in the output unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Coord(pub i64);

impl Coord {
    pub fn from_mil(mil: f64) -> Coord {
        Coord((mil * PER_MIL).round() as i64)
    }
    pub fn from_mm(mm: f64) -> Coord {
        Coord((mm * PER_MM).round() as i64)
    }
    pub fn mil(self) -> f64 {
        self.0 as f64 / PER_MIL
    }
    pub fn mm(self) -> f64 {
        self.0 as f64 / PER_MM
    }
    /// Value in the current output unit, rounded to a sensible precision.
    pub fn out(self) -> f64 {
        match output_unit() {
            Unit::Mm => round_to(self.mm(), 6),
            Unit::Mil => round_to(self.mil(), 4),
            Unit::Raw => self.0 as f64,
        }
    }

    /// Schematic coordinate: integer part in 10-mil units plus `_FRAC` in 1/100000 of that.
    pub fn from_sch(int: i64, frac: i64) -> Coord {
        // 10 mil = 100_000 internal units; frac is 1/100000 of 10 mil = 1 internal unit.
        Coord(int * 100_000 + frac)
    }

    /// Parse a PCB property length such as `"12.5mil"`, `"+0mil"`, `"0.3mm"`.
    pub fn parse_len(s: &str) -> Option<Coord> {
        let s = s.trim();
        let (num, unit) = if let Some(p) = s.strip_suffix("mil") {
            (p, "mil")
        } else if let Some(p) = s.strip_suffix("mm") {
            (p, "mm")
        } else {
            (s, "mil")
        };
        let v: f64 = num.trim().trim_start_matches('+').parse().ok()?;
        Some(if unit == "mm" { Coord::from_mm(v) } else { Coord::from_mil(v) })
    }
}

impl std::ops::Add for Coord {
    type Output = Coord;
    fn add(self, o: Coord) -> Coord {
        Coord(self.0 + o.0)
    }
}
impl std::ops::Sub for Coord {
    type Output = Coord;
    fn sub(self, o: Coord) -> Coord {
        Coord(self.0 - o.0)
    }
}

pub fn round_to(v: f64, places: i32) -> f64 {
    let p = 10f64.powi(places);
    let r = (v * p).round() / p;
    if r == 0.0 { 0.0 } else { r }
}

impl Serialize for Coord {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let v = self.out();
        if output_unit() == Unit::Raw {
            s.serialize_i64(self.0)
        } else {
            s.serialize_f64(v)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub struct Point {
    pub x: Coord,
    pub y: Coord,
}

impl Point {
    pub fn new(x: Coord, y: Coord) -> Point {
        Point { x, y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(Coord::from_mil(1.0).0, 10_000);
        assert!((Coord::from_mm(25.4).mil() - 1000.0).abs() < 1e-6);
        assert_eq!(Coord::parse_len("+12.5mil"), Some(Coord(125_000)));
        assert_eq!(Coord::from_sch(1, 0).mil(), 10.0);
    }
}
