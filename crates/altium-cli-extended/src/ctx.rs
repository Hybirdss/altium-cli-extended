//! Per-invocation settings shared by every command handler.

use std::path::{Path, PathBuf};

use altium_core::error::{Error, ErrorCode, Result};
use altium_format::fsx::WriteOpts;

#[derive(Debug, Clone, Default)]
pub struct Ctx {
    /// Include raw Altium property maps in objects that support it.
    pub raw: bool,
    /// For write commands: show what would change without writing.
    pub dry_run: bool,
    /// For write commands: skip the `.bak` copy.
    pub no_backup: bool,
    /// Protect design files even if a handler bypasses command dispatch.
    pub read_only: bool,
}

impl Ctx {
    /// Resolve and check an input path.
    pub fn input(&self, p: &Path) -> Result<PathBuf> {
        if !p.exists() {
            return Err(Error::new(
                ErrorCode::FileNotFound,
                format!("{} does not exist", p.display()),
            )
            .hint("paths are relative to the current directory; quote names with spaces"));
        }
        if p.is_dir() {
            let mut candidates: Vec<PathBuf> = std::fs::read_dir(p)?.filter_map(|e| e.ok()).map(|e| e.path())
                .filter(|f| f.is_file() && altium_format::DocKind::of(f) == altium_format::DocKind::Project
                    && !f.file_name().is_some_and(|n| n.to_string_lossy().starts_with("._"))).collect();
            candidates.sort();
            return match candidates.as_slice() {
                [one] => Ok(one.clone()),
                [] => Err(Error::not_found(format!("no .PrjPcb project in {}", p.display()))
                    .hint("run `ace project find <directory>` and pass an explicit project")),
                _ => Err(Error::new(ErrorCode::Ambiguous, format!("several projects in {}", p.display()))
                    .details(serde_json::json!({"candidates": candidates}))
                    .hint("choose an explicit .PrjPcb from error.details.candidates")),
            };
        }
        Ok(p.to_path_buf())
    }

    fn check_write(&self) -> Result<()> {
        if self.read_only && !self.dry_run {
            return Err(Error::new(ErrorCode::Unsafe, "read-only session refuses a design write")
                .hint("copy the project and unset ACE_READONLY / --read-only to edit"));
        }
        Ok(())
    }

    /// Save recovery bytes without overwriting an existing copy, even with `--no-backup`.
    pub fn preserve_file(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.check_write()?;
        if !self.dry_run { altium_format::fsx::preserve(path, bytes)?; }
        Ok(())
    }

    /// Replace a design file honouring `--dry-run` / `--no-backup`. Returns the backup path.
    pub fn write_file(&self, path: &Path, bytes: &[u8]) -> Result<Option<PathBuf>> {
        self.check_write()?;
        altium_format::fsx::replace(path, bytes, WriteOpts { dry_run: self.dry_run, no_backup: self.no_backup })
    }

    /// Write a generated output file (BOM, netlist, SVG...): creates the parent directory,
    /// never keeps a backup, does nothing under `--dry-run`. Refuses to overwrite a design file.
    pub fn export_file(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        if !matches!(altium_format::DocKind::of(path), altium_format::DocKind::Other) {
            return Err(Error::new(ErrorCode::Unsafe, format!("refusing to overwrite design file {}", path.display()))
                .hint("choose an output name such as bom.csv or netlist.net"));
        }
        if self.dry_run {
            return Ok(());
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }
}
