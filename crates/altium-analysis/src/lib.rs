//! Cross-document analysis over [`altium_doc::model::Design`]: BOM, netlist, consistency
//! checks (schematic vs PCB, annotation, footprints) and diffs. Inputs come from either the
//! schematic or the PCB parser; nothing here reads files.

pub mod bom;
pub mod check;
pub mod designator;
pub mod diff;
pub mod netlist;
