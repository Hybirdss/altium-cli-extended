//! Document type by file extension.

use std::path::Path;

use serde::Serialize;

/// Altium document kinds recognised by file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DocKind {
    Project,
    Schematic,
    SchLib,
    Pcb,
    PcbLib,
    IntLib,
    OutJob,
    Harness,
    Other,
}

impl DocKind {
    pub fn of(path: &Path) -> DocKind {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "prjpcb" => DocKind::Project,
            "schdoc" => DocKind::Schematic,
            "schlib" => DocKind::SchLib,
            "pcbdoc" => DocKind::Pcb,
            "pcblib" => DocKind::PcbLib,
            "intlib" => DocKind::IntLib,
            "outjob" => DocKind::OutJob,
            "harness" => DocKind::Harness,
            _ => DocKind::Other,
        }
    }
}
