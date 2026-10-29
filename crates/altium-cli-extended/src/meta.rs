//! Agent-facing command metadata.
//!
//! clap is the source of truth for command names, arguments and help text. Each module adds a
//! [`CmdMeta`] row per leaf command with what clap cannot express: which backend runs it,
//! whether it changes anything, what `data` looks like, and a worked example. `altium
//! commands` merges the two, and a unit test fails if a leaf command has no row (or a row has
//! no command), so the manifest cannot drift from the CLI.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// Pure Rust file parsing. Fast, no Altium needed, works on any OS.
    Native,
    /// Drives a real Altium Designer (local Windows, a Windows machine over SSH, or Wine)
    /// through the bridge script.
    Designer,
    /// Introspection of this tool itself.
    Meta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Reads only. Safe to call any time.
    Read,
    /// Modifies a design file (always makes a `.bak` first unless told otherwise).
    Write,
    /// Produces new output files (Gerber, PDF, BOM...) without touching the design.
    Export,
    /// Starts/stops processes or changes tool configuration.
    Control,
}

#[derive(Debug, Clone, Serialize)]
pub struct CmdMeta {
    /// Space-separated command path, e.g. `"pcb components"`.
    pub path: &'static str,
    pub backend: Backend,
    pub effect: Effect,
    /// Shape of the `data` field on success.
    pub returns: &'static str,
    /// A realistic invocation.
    pub example: &'static str,
    /// When to use it / gotchas. Empty if none.
    pub notes: &'static str,
}

/// Shorthand constructor used by the per-module tables.
pub const fn m(
    path: &'static str,
    backend: Backend,
    effect: Effect,
    returns: &'static str,
    example: &'static str,
    notes: &'static str,
) -> CmdMeta {
    CmdMeta { path, backend, effect, returns, example, notes }
}

/// Every module's table.
pub fn all() -> Vec<CmdMeta> {
    let mut v = Vec::new();
    v.extend_from_slice(crate::commands::meta::META);
    v.extend_from_slice(crate::commands::raw::META);
    v.extend_from_slice(crate::commands::outjob::META);
    v.extend_from_slice(crate::commands::project::META);
    v.extend_from_slice(crate::commands::sch::META);
    v.extend_from_slice(crate::commands::schlib::META);
    v.extend_from_slice(crate::commands::pcb::META);
    v.extend_from_slice(crate::commands::pcblib::META);
    v.extend_from_slice(crate::commands::analysis::META);
    v.extend_from_slice(crate::commands::designer::META);
    v
}

pub fn find(path: &str) -> Option<CmdMeta> {
    all().into_iter().find(|m| m.path == path)
}
