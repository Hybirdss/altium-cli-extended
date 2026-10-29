//! Document-independent design model.
//!
//! The schematic and PCB parsers each produce these types, so cross-document analysis
//! (BOM, netlist, schematic-vs-PCB checks, diffs) works the same whatever the source.

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::units::Coord;

/// One placed part (one designator). Multi-part schematic symbols (U1A, U1B) are merged into
/// one `Part` whose `units` lists the sub-parts.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Part {
    pub designator: String,
    /// Library symbol or footprint name it was placed from.
    pub lib_ref: String,
    /// The `Comment` field (usually the value or part number).
    pub comment: String,
    pub description: String,
    /// Footprint name (schematic: current PCB footprint model; PCB: pattern).
    pub footprint: String,
    /// Library file the part came from, when recorded.
    pub library: String,
    /// User parameters (`Value`, `Manufacturer`, `MPN`, ...). Keys as written in Altium.
    pub parameters: IndexMap<String, String>,
    /// Document(s) the part appears on (file name only).
    pub documents: Vec<String>,
    /// Altium unique id (schematic `UNIQUEID` / PCB `SOURCEUNIQUEID`) — links sch↔pcb.
    pub unique_id: String,
    /// Sub-part labels for multi-part symbols (e.g. `["A","B"]`). Empty if single part.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<String>,
    /// PCB only: `top` / `bottom`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<Coord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    /// Excluded from BOM (`Standard (No BOM)`, graphical, ...).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub exclude_from_bom: bool,
    /// Component kind as Altium names it (`Standard`, `Mechanical`, `Graphical`, `NetTie`, ...).
    pub kind: String,
}

impl Part {
    /// Case-insensitive parameter lookup.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A connection point: one pin of one part.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Node {
    pub designator: String,
    /// Pin designator / pad number (`"1"`, `"A3"`, `"GND"`...).
    pub pin: String,
    /// Pin name / function when known.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pin_name: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Net {
    pub name: String,
    pub nodes: Vec<Node>,
    /// True when the name was generated (no net label / power port / port named it).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_named: bool,
    /// How the name was decided (`net-label`, `power-port`, `port`, `sheet-entry`, `pin`, `pcb`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name_source: String,
}

/// Source the analysis layer pulls parts and nets from.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Design {
    /// `schematic` or `pcb`.
    pub source: String,
    pub parts: Vec<Part>,
    pub nets: Vec<Net>,
    pub warnings: Vec<String>,
}
