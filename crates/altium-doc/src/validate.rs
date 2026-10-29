//! Re-parse a proposed whole-document replacement without writing it.
use std::path::Path;
use altium_core::{Error, ErrorCode, Result};
use altium_format::DocKind;

pub fn bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    match DocKind::of(path) {
        DocKind::Project => { super::project::Project::from_bytes(path, bytes)?; }
        DocKind::OutJob => {
            if altium_format::ini::Ini::parse(bytes).section("OutputGroup1").is_none() {
                return Err(Error::parse("backup has no [OutputGroup1]")
                    .hint("keep the current file; recover a valid .OutJob backup"));
            }
        }
        DocKind::Schematic => { super::sch::load_bytes(path, bytes.to_vec(), &Default::default())?; }
        DocKind::SchLib => { super::schlib::load_bytes(path, bytes.to_vec(), false)?; }
        DocKind::Pcb => { super::pcb::load_bytes(path, bytes.to_vec(), &Default::default())?; }
        DocKind::PcbLib => { super::pcblib::load_bytes(path, bytes.to_vec(), false)?; }
        _ => return Err(Error::new(ErrorCode::WrongFileType, "no native validator for this backup type")
            .hint("restore a .PrjPcb, .OutJob, .SchDoc, .SchLib, .PcbDoc or .PcbLib")),
    }
    Ok(())
}
