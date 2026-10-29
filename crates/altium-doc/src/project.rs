//! `.PrjPcb` projects and `.OutJob` output job files (both INI text).

use std::path::{Component, Path, PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_format::DocKind;
use altium_format::ini::{Ini, pipe_props};

pub struct Project {
    pub path: PathBuf,
    pub dir: PathBuf,
    pub ini: Ini,
    pub documents: Vec<ProjectDoc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDoc {
    /// 1-based `[DocumentN]` index.
    pub index: u32,
    /// Path as written in the project (Windows separators).
    pub rel: String,
    /// Resolved local path (case-insensitive match on Linux).
    pub path: PathBuf,
    pub kind: DocKind,
    pub exists: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unique_id: String,
    /// True for `[GeneratedDocumentN]` entries (outputs Altium added to the project).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub generated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Variant {
    pub index: u32,
    /// Altium shows the description as the variant name.
    pub name: String,
    pub unique_id: String,
    pub allow_fabrication: bool,
    pub variations: Vec<Variation>,
    /// Variant-level parameters.
    pub parameters: IndexMap<String, String>,
    pub param_variations: Vec<IndexMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Variation {
    pub designator: String,
    pub unique_id: String,
    /// `not_fitted`, `fitted` (parameter changes only) or `alternate`.
    pub kind: String,
    pub kind_code: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub alternate_part: String,
    pub raw: IndexMap<String, String>,
}

/// Resolve a project-relative Windows path against `dir`, matching each component
/// case-insensitively (Altium paths are case-insensitive; Linux file systems are not).
pub fn resolve_ci(dir: &Path, rel: &str) -> PathBuf {
    let norm = rel.replace('\\', "/");
    let rel_path = Path::new(&norm);
    let mut cur = if rel_path.is_absolute() { PathBuf::from("/") } else { dir.to_path_buf() };
    // Drive-letter paths (C:/...) cannot be resolved; keep them verbatim.
    if norm.len() > 1 && norm.as_bytes()[1] == b':' {
        return PathBuf::from(norm);
    }
    for comp in rel_path.components() {
        match comp {
            Component::ParentDir => {
                cur.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            Component::Normal(name) => {
                let direct = cur.join(name);
                if direct.exists() {
                    cur = direct;
                    continue;
                }
                let want = name.to_string_lossy().to_lowercase();
                let found = std::fs::read_dir(&cur).ok().and_then(|rd| {
                    rd.flatten().map(|e| e.path()).find(|p| {
                        p.file_name().is_some_and(|f| f.to_string_lossy().to_lowercase() == want)
                    })
                });
                cur = found.unwrap_or(direct);
            }
        }
    }
    cur
}

impl Project {
    pub fn load(path: &Path) -> Result<Project> {
        if DocKind::of(path) != DocKind::Project {
            return Err(Error::new(
                ErrorCode::WrongFileType,
                format!("{} is not a .PrjPcb project", path.display()),
            ));
        }
        Self::from_ini(path, Ini::load(path)?)
    }

    /// Parse project text (`path` locates its documents); used to verify edits.
    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Project> {
        Self::from_ini(path, Ini::parse(bytes))
    }

    fn from_ini(path: &Path, ini: Ini) -> Result<Project> {
        if ini.section("Design").is_none() && ini.numbered("Document").is_empty() {
            return Err(Error::parse(format!("{} has no [Design] or [DocumentN] sections", path.display())));
        }
        // Resolve the entry before consuming `..` components in Windows document paths.
        // Verification may parse bytes for an output path that has not been created yet.
        let path = if path.exists() { std::fs::canonicalize(path)? } else { std::path::absolute(path)? };
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut documents = Vec::new();
        for (prefix, generated) in [("Document", false), ("GeneratedDocument", true)] {
            for (n, s) in ini.numbered(prefix) {
                let rel = s.str("DocumentPath");
                if rel.is_empty() {
                    continue;
                }
                let p = resolve_ci(&dir, &rel);
                documents.push(ProjectDoc {
                    index: n,
                    kind: DocKind::of(Path::new(&rel.replace('\\', "/"))),
                    exists: p.exists(),
                    path: p,
                    rel,
                    unique_id: s.str("DocumentUniqueId"),
                    generated,
                });
            }
        }
        Ok(Project { path: path.to_path_buf(), dir, ini, documents })
    }

    pub fn name(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub fn docs(&self, kind: DocKind) -> Vec<&ProjectDoc> {
        self.documents.iter().filter(|d| d.kind == kind && !d.generated).collect()
    }

    /// Existing schematic sheets in project order.
    pub fn schematics(&self) -> Vec<PathBuf> {
        self.docs(DocKind::Schematic).into_iter().filter(|d| d.exists).map(|d| d.path.clone()).collect()
    }

    pub fn pcbs(&self) -> Vec<PathBuf> {
        self.docs(DocKind::Pcb).into_iter().filter(|d| d.exists).map(|d| d.path.clone()).collect()
    }

    pub fn design(&self, key: &str) -> Option<String> {
        self.ini.section("Design").and_then(|s| s.get(key)).map(str::to_owned)
    }

    pub fn design_bool(&self, key: &str) -> bool {
        self.ini.section("Design").is_some_and(|s| s.bool(key))
    }

    /// Project parameters (`[ParameterN]` Name/Value).
    pub fn parameters(&self) -> IndexMap<String, String> {
        self.ini
            .numbered("Parameter")
            .into_iter()
            .map(|(_, s)| (s.str("Name"), s.str("Value")))
            .filter(|(k, _)| !k.is_empty())
            .collect()
    }

    pub fn variants(&self) -> Vec<Variant> {
        self.ini
            .numbered("ProjectVariant")
            .into_iter()
            .map(|(n, s)| {
                let count = |key: &str| s.int(key).unwrap_or(0).max(0) as usize;
                let variations = (1..=count("VariationCount"))
                    .filter_map(|i| s.get(&format!("Variation{i}")))
                    .map(|v| {
                        let raw = pipe_props(v);
                        let code = raw.get("Kind").and_then(|k| k.trim().parse().ok()).unwrap_or(-1);
                        Variation {
                            designator: raw.get("Designator").cloned().unwrap_or_default(),
                            unique_id: raw.get("UniqueId").cloned().unwrap_or_default(),
                            kind: match code {
                                0 => "fitted",
                                1 => "not_fitted",
                                2 => "alternate",
                                _ => "unknown",
                            }
                            .into(),
                            kind_code: code,
                            alternate_part: raw.get("AlternatePart").map(|a| a.trim().to_owned()).unwrap_or_default(),
                            raw,
                        }
                    })
                    .collect();
                let parameters = (1..=count("ParameterCount"))
                    .filter_map(|i| {
                        let p = pipe_props(s.get(&format!("Parameter{i}"))?);
                        Some((p.get("Name")?.clone(), p.get("Value").cloned().unwrap_or_default()))
                    })
                    .collect();
                // `ParamDesignatorN=R1` + `ParamVariationN=ParameterName=Value|VariantValue=22k`;
                // the designator is folded into the map as `Designator`.
                let param_variations = (1..=count("ParamVariationCount"))
                    .filter_map(|i| {
                        let mut m = pipe_props(s.get(&format!("ParamVariation{i}"))?);
                        if let Some(d) = s.get(&format!("ParamDesignator{i}"))
                            && !m.keys().any(|k| k.eq_ignore_ascii_case("Designator"))
                        {
                            m.shift_insert(0, "Designator".to_owned(), d.to_owned());
                        }
                        Some(m)
                    })
                    .collect();
                Variant {
                    index: n,
                    name: s.str("Description"),
                    unique_id: s.str("UniqueID"),
                    allow_fabrication: s.bool("AllowFabrication"),
                    variations,
                    parameters,
                    param_variations,
                }
            })
            .collect()
    }

    pub fn variant(&self, name: &str) -> Result<Variant> {
        let vs = self.variants();
        let names: Vec<String> = vs.iter().map(|v| v.name.clone()).collect();
        vs.into_iter()
            .find(|v| v.name.eq_ignore_ascii_case(name) || v.unique_id.eq_ignore_ascii_case(name) || v.index.to_string() == name)
            .ok_or_else(|| {
                Error::not_found(format!("no variant {name:?}"))
                    .hint(format!("variants: {names:?} (name, unique id or 1-based index)"))
            })
    }
}

/// What a variant gets for one part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariationKind {
    /// Kind=1: not placed in this variant.
    NotFitted,
    /// Kind=0: fitted, parameter changes only.
    Fitted,
    /// Kind=2: replaced by another library part.
    Alternate(String),
}

impl VariationKind {
    fn code(&self) -> i64 {
        match self {
            VariationKind::Fitted => 0,
            VariationKind::NotFitted => 1,
            VariationKind::Alternate(_) => 2,
        }
    }
}

/// Variation lines of a `[ProjectVariantN]` section, in Altium's order.
#[derive(Debug, Clone, Default)]
struct VariantLines {
    variations: Vec<String>,
    /// `(designator, ParameterName=..|VariantValue=..)`.
    params: Vec<(String, String)>,
}

fn is_numbered(key: &str, prefix: &str) -> bool {
    key.len() > prefix.len() && key[..prefix.len()].eq_ignore_ascii_case(prefix) && key[prefix.len()..].bytes().all(|b| b.is_ascii_digit())
}

impl VariantLines {
    fn take(s: &mut altium_format::ini::Section) -> VariantLines {
        let count = |k: &str| s.int(k).unwrap_or(0).max(0) as usize;
        let mut v = VariantLines::default();
        for i in 1..=count("VariationCount") {
            if let Some(x) = s.get(&format!("Variation{i}")) {
                v.variations.push(x.to_owned());
            }
        }
        for i in 1..=count("ParamVariationCount") {
            if let Some(x) = s.get(&format!("ParamVariation{i}")) {
                v.params.push((s.str(&format!("ParamDesignator{i}")), x.to_owned()));
            }
        }
        s.entries.retain(|(k, _)| {
            !(k.eq_ignore_ascii_case("VariationCount")
                || k.eq_ignore_ascii_case("ParamVariationCount")
                || is_numbered(k, "Variation")
                || is_numbered(k, "ParamVariation")
                || is_numbered(k, "ParamDesignator"))
        });
        v
    }

    /// Put the lines back after the variant's own fields and parameters.
    fn put(self, s: &mut altium_format::ini::Section) {
        let parameter_count = self.params.len();
        let mut components: Vec<_> = self.variations.into_iter().map(|line| (line, Vec::new())).collect();
        let mut orphans = Vec::new();
        for (designator, line) in self.params {
            if let Some((_, params)) = components.iter_mut().find(|(line, _)| designator_of(line).eq_ignore_ascii_case(&designator)) {
                params.push((designator, line));
            } else {
                orphans.push((designator, line));
            }
        }
        let mut block = vec![("VariationCount".to_owned(), components.len().to_string())];
        let mut parameter_index = 0;
        let mut append_params = |block: &mut Vec<(String, String)>, params: Vec<(String, String)>| {
            for (designator, line) in params {
                parameter_index += 1;
                block.push((format!("ParamDesignator{parameter_index}"), designator));
                block.push((format!("ParamVariation{parameter_index}"), line));
            }
        };
        // Preserve legacy orphan rows before any component, where they cannot change
        // another part. Altium attaches parameter rows to the preceding VariationN.
        append_params(&mut block, orphans);
        for (i, (line, params)) in components.into_iter().enumerate() {
            block.push((format!("Variation{}", i + 1), line));
            append_params(&mut block, params);
        }
        // Altium ignores the overrides if their count appears before the rows.
        block.push(("ParamVariationCount".to_owned(), parameter_count.to_string()));
        let at = s
            .entries
            .iter()
            .rposition(|(k, _)| {
                ["UniqueID", "Description", "AllowFabrication", "ParameterCount"].iter().any(|x| k.eq_ignore_ascii_case(x)) || is_numbered(k, "Parameter")
            })
            .map(|i| i + 1)
            .unwrap_or(s.entries.len());
        s.entries.splice(at..at, block);
    }
}

fn designator_of(line: &str) -> String {
    pipe_props(line).get("Designator").cloned().unwrap_or_default()
}

impl Project {
    fn variant_section_name(&self, name: &str) -> Result<String> {
        let v = self.variant(name)?;
        Ok(format!("ProjectVariant{}", v.index))
    }

    /// Add an empty assembly variant; returns its `[ProjectVariantN]` index.
    pub fn add_variant(&mut self, name: &str, unique_id: &str, allow_fabrication: bool) -> Result<u32> {
        if name.trim().is_empty() || name.contains(['\r', '\n']) {
            return Err(Error::usage("the variant name is empty or contains a line break"));
        }
        if self.variants().iter().any(|v| v.name.eq_ignore_ascii_case(name)) {
            return Err(Error::new(ErrorCode::Unsafe, format!("a variant named {name:?} already exists")));
        }
        let n = self.ini.numbered("ProjectVariant").last().map(|(n, _)| n + 1).unwrap_or(1);
        let mut s = altium_format::ini::Section { name: format!("ProjectVariant{n}"), entries: vec![] };
        s.set("UniqueID", unique_id);
        s.set("Description", name);
        s.set("AllowFabrication", if allow_fabrication { "1" } else { "0" });
        s.set("ParameterCount", "0");
        VariantLines::default().put(&mut s);
        let pos = self
            .ini
            .sections
            .iter()
            .rposition(|x| is_numbered(&x.name, "ProjectVariant"))
            .or_else(|| self.ini.sections.iter().rposition(|x| is_numbered(&x.name, "Document") || is_numbered(&x.name, "GeneratedDocument")));
        match pos {
            Some(i) => self.ini.sections.insert(i + 1, s),
            None => self.ini.sections.push(s),
        }
        Ok(n)
    }

    /// Remove a variant (later `[ProjectVariantN]` sections are renumbered).
    pub fn remove_variant(&mut self, name: &str) -> Result<String> {
        let sec = self.variant_section_name(name)?;
        self.ini.sections.retain(|s| s.name != sec);
        let mut n = 0;
        for s in self.ini.sections.iter_mut().filter(|s| is_numbered(&s.name, "ProjectVariant")) {
            n += 1;
            s.name = format!("ProjectVariant{n}");
        }
        Ok(sec)
    }

    /// Set how a variant treats one part. `unique_id` is the part's unique-id path
    /// (`\SHEETSYMBOLID\COMPONENTID`, or `\COMPONENTID` on a flat design).
    /// Returns (previous line, new line).
    pub fn set_variation(&mut self, variant: &str, designator: &str, unique_id: &str, kind: &VariationKind) -> Result<(Option<String>, String)> {
        if designator.contains(['|', '=']) || unique_id.contains(['|', '=']) {
            return Err(Error::usage("designators and unique ids cannot contain | or ="));
        }
        let sec = self.variant_section_name(variant)?;
        let s = self.ini.section_mut(&sec).ok_or_else(|| Error::internal("variant section vanished"))?;
        let mut lines = VariantLines::take(s);
        let alt = match kind {
            VariationKind::Alternate(a) => a.clone(),
            _ => String::new(),
        };
        if alt.contains('|') {
            return Err(Error::usage("the alternate part cannot contain |"));
        }
        let line = format!("Designator={designator}|UniqueId={unique_id}|Kind={}|AlternatePart={alt}", kind.code());
        let before = match lines.variations.iter().position(|x| designator_of(x).eq_ignore_ascii_case(designator)) {
            Some(i) => Some(std::mem::replace(&mut lines.variations[i], line.clone())),
            None => {
                lines.variations.push(line.clone());
                None
            }
        };
        lines.put(s);
        Ok((before, line))
    }

    /// Set a parameter value of one part in a variant (`ParamVariation`). Creates a fitted
    /// component variation when absent: Altium needs it to attach overrides to this part.
    /// Returns the previous effective variant value (or None for an orphan override).
    pub fn set_param_variation(&mut self, variant: &str, designator: &str, parameter: &str, value: &str) -> Result<Option<String>> {
        if [designator, parameter, value].iter().any(|x| x.contains(['|', '\r', '\n'])) || parameter.contains('=') {
            return Err(Error::usage("designator, parameter and value cannot contain | or line breaks (nor = in the name)"));
        }
        let sec = self.variant_section_name(variant)?;
        let missing_component = !self.variant(variant)?.variations.iter().any(|v| v.designator.eq_ignore_ascii_case(designator));
        let unique_id = if missing_component { Some(self.variation_part_id(designator)?) } else { None };
        let s = self.ini.section_mut(&sec).ok_or_else(|| Error::internal("variant section vanished"))?;
        let mut lines = VariantLines::take(s);
        if let Some(unique_id) = unique_id {
            lines.variations.push(format!("Designator={designator}|UniqueId={unique_id}|Kind=0|AlternatePart="));
        }
        let line = format!("ParameterName={parameter}|VariantValue={value}");
        let hit = lines.params.iter().position(|(d, x)| {
            d.eq_ignore_ascii_case(designator) && pipe_props(x).get("ParameterName").is_some_and(|p| p.eq_ignore_ascii_case(parameter))
        });
        let before = match hit {
            Some(i) => pipe_props(&std::mem::replace(&mut lines.params[i].1, line)).get("VariantValue").cloned(),
            None => {
                lines.params.push((designator.to_owned(), line));
                None
            }
        };
        lines.put(s);
        // An orphan override did not apply to its named part. Report a change even when
        // its stored value is identical, so callers save the new component variation.
        Ok(if missing_component { None } else { before })
    }

    fn variation_part_id(&self, designator: &str) -> Result<String> {
        for path in self.pcbs() {
            let Ok(pcb) = crate::pcb::load_with(&path, &crate::pcb::LoadOptions::parts(0)) else { continue };
            if let Some(part) = pcb.components.iter().find(|part| {
                (part.source_designator.eq_ignore_ascii_case(designator) || part.designator.eq_ignore_ascii_case(designator))
                    && !part.source_unique_id.is_empty()
            }) {
                return Ok(part.source_unique_id.clone());
            }
        }
        if !self.schematics().is_empty() {
            let design = crate::sch::project_design(self)?;
            if let Some(part) = design.parts.iter().find(|part| part.designator.eq_ignore_ascii_case(designator)) {
                if part.unique_id.is_empty() {
                    return Err(Error::new(ErrorCode::Unsafe, format!("{designator} has no unique id"))
                        .hint("first use `ace project set-variation <project> <variant> <part> --fitted --unique-id <id>`, then set the parameter"));
                }
                return Ok(format!("\\{}", part.unique_id.trim_start_matches('\\')));
            }
        }
        Err(Error::not_found(format!("no part {designator} in the project's PCB or schematics"))
            .hint("check `ace bom <project> --group-by designator`, or create its variation with `ace project set-variation` and --unique-id"))
    }

    /// Drop every variation (fitting and parameters) of one part from a variant. Returns the
    /// number of lines removed.
    pub fn clear_variation(&mut self, variant: &str, designator: &str) -> Result<usize> {
        let sec = self.variant_section_name(variant)?;
        let s = self.ini.section_mut(&sec).ok_or_else(|| Error::internal("variant section vanished"))?;
        let mut lines = VariantLines::take(s);
        let n = lines.variations.len() + lines.params.len();
        lines.variations.retain(|x| !designator_of(x).eq_ignore_ascii_case(designator));
        lines.params.retain(|(d, _)| !d.eq_ignore_ascii_case(designator));
        let removed = n - lines.variations.len() - lines.params.len();
        lines.put(s);
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_edits_roundtrip() {
        let src = b"[Design]\r\nVersion=1.0\r\n\r\n[Document1]\r\nDocumentPath=a.SchDoc\r\n";
        let mut p = Project::from_bytes(Path::new("x.PrjPcb"), src).unwrap();
        assert_eq!(p.add_variant("Lite", "ABCDEFGH", false).unwrap(), 1);
        p.set_variation("Lite", "R3", "\\QWERTYUI", &VariationKind::NotFitted).unwrap();
        p.set_variation("Lite", "R4", "\\ASDFGHJK", &VariationKind::Alternate("RES_ALT".into())).unwrap();
        p.set_param_variation("Lite", "R4", "Comment", "22k").unwrap();
        let p = Project::from_bytes(Path::new("x.PrjPcb"), &p.ini.to_bytes()).unwrap();
        let v = p.variant("Lite").unwrap();
        assert_eq!(v.variations.len(), 2);
        assert_eq!(v.variations[0].kind, "not_fitted");
        assert_eq!(v.variations[1].alternate_part, "RES_ALT");
        assert_eq!(v.param_variations[0]["Designator"], "R4");
        assert_eq!(v.param_variations[0]["VariantValue"], "22k");
        let mut p = p;
        assert!(p.set_variation("Lite", "R3", "\\QWERTYUI", &VariationKind::Fitted).unwrap().0.is_some());
        assert_eq!(p.clear_variation("Lite", "R4").unwrap(), 2);
        assert_eq!(p.variant("Lite").unwrap().param_variations.len(), 0);
        assert_eq!(p.remove_variant("Lite").unwrap(), "ProjectVariant1");
        assert!(p.variants().is_empty());
    }
}
