//! Typed models of Altium documents, their parsers, and safe editors.
//!
//! Each document module exposes a `load(path)` returning a typed document, query helpers, and
//! (where supported) edit operations that produce new bytes for [`altium_format::fsx::replace`].
//! [`model`] holds the document-independent `Part`/`Net`/`Design` used for cross-document work.

pub mod model;
pub mod outjob;
pub mod pcb;
pub mod pcblib;
pub mod project;
pub mod sch;
pub mod schlib;

pub mod validate;
