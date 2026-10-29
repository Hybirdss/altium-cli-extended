//! Altium file containers, independent of what the documents mean:
//!
//! - [`cfbx`]: OLE compound files (`.SchDoc`, `.SchLib`, `.PcbDoc`, `.PcbLib`, `.IntLib`),
//!   length-prefixed record framing and a bounds-checked binary cursor.
//! - [`props`]: `|KEY=VALUE|` property lists with order-preserving write-back.
//! - [`rawprops`]: byte-exact edits of a single field inside a record (for safe writes).
//! - [`ini`]: order-preserving INI for `.PrjPcb` and `.OutJob`.
//! - [`fsx`]: safe writes (dry run, `.bak`, atomic rename).
//! - [`DocKind`]: document type by extension.

pub mod cfbx;
pub mod fsx;
pub mod ini;
mod kind;
pub mod props;
pub mod rawprops;

pub use kind::DocKind;
