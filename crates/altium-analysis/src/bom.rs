//! Bill of materials from a [`Design`], optionally with an assembly variant applied.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::{Error, Result};
use altium_doc::model::{Design, Part};
use altium_doc::project::Variant;

use crate::designator;

/// Parameter names that commonly carry sourcing data, in lookup order.
const MPN_KEYS: &[&str] = &["Manufacturer Part Number", "MPN", "Manufacturer Part Number 1", "Part Number", "PartNumber", "MfrPN"];
const MFR_KEYS: &[&str] = &["Manufacturer", "Manufacturer 1", "MFR", "Mfr"];
const VALUE_KEYS: &[&str] = &["Value", "Val"];
const SUPPLIER_PN_KEYS: &[&str] = &["Supplier Part Number 1", "Supplier Part Number", "LCSC", "LCSC Part", "Digikey", "DigiKey Part Number", "Mouser Part Number"];

#[derive(Debug, Clone)]
pub struct BomOptions {
    /// Fields that define one BOM line. Built-ins: comment, footprint, lib_ref, description,
    /// value, mpn, manufacturer; anything else is a part parameter name.
    pub group_by: Vec<String>,
    pub variant: Option<Variant>,
    /// Keep parts flagged "exclude from BOM" (marked `excluded`).
    pub include_excluded: bool,
    /// Keep not-fitted parts of the variant (marked `dnp`) instead of dropping them.
    pub include_dnp: bool,
}

impl Default for BomOptions {
    fn default() -> Self {
        BomOptions {
            group_by: vec!["comment".into(), "footprint".into()],
            variant: None,
            include_excluded: false,
            include_dnp: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BomLine {
    pub line: usize,
    pub quantity: usize,
    /// Natural-sorted designators.
    pub designators: Vec<String>,
    /// Compressed form, e.g. `R1-R4, R9`.
    pub designator_ranges: String,
    pub comment: String,
    pub footprint: String,
    pub lib_ref: String,
    pub description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub value: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub manufacturer: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mpn: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub supplier_pn: String,
    /// Parameters whose value is identical across every part of the line.
    pub parameters: IndexMap<String, String>,
    /// Parameters that differ inside the line (a grouping smell worth reporting).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inconsistent: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dnp: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub excluded: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bom {
    pub group_by: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub lines: Vec<BomLine>,
    pub total_parts: usize,
    pub unique_lines: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dnp: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<String>,
    /// Parts whose parameters the variant changed (`ParamVariation`) or swapped (`alternate`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub varied: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Apply a variant's parameter variations and alternate parts to a copy of the parts.
fn apply_variant(parts: &[Part], v: Option<&Variant>, varied: &mut Vec<String>, warnings: &mut Vec<String>) -> Vec<Part> {
    let mut out = parts.to_vec();
    let Some(v) = v else { return out };
    let find = |out: &[Part], d: &str| out.iter().position(|p| p.designator.eq_ignore_ascii_case(d.trim_end_matches('*')));
    for pv in &v.param_variations {
        let get = |k: &str| pv.iter().find(|(x, _)| x.eq_ignore_ascii_case(k)).map(|(_, y)| y.as_str());
        let (Some(d), Some(name)) = (get("Designator"), get("ParameterName")) else {
            warnings.push(format!("variant {}: parameter variation without designator/name: {pv:?}", v.name));
            continue;
        };
        let value = get("VariantValue").unwrap_or("").to_owned();
        let Some(i) = find(&out, d) else { continue };
        let p = &mut out[i];
        if name.eq_ignore_ascii_case("Comment") {
            p.comment = value;
        } else if let Some(k) = p.parameters.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned() {
            p.parameters.insert(k, value);
        } else {
            p.parameters.insert(name.to_owned(), value);
        }
        varied.push(p.designator.clone());
    }
    for x in v.variations.iter().filter(|x| x.kind == "alternate") {
        let Some(i) = find(&out, &x.designator) else { continue };
        if !x.alternate_part.is_empty() {
            out[i].lib_ref = x.alternate_part.clone();
        }
        varied.push(out[i].designator.clone());
        warnings.push(format!(
            "{} uses alternate part {:?}: its comment/footprint come from the library and are not resolved natively (`ace designer` BOM output is authoritative)",
            out[i].designator, x.alternate_part
        ));
    }
    designator::sort(varied);
    varied.dedup();
    out
}

fn lookup(p: &Part, keys: &[&str]) -> String {
    keys.iter().find_map(|k| p.param(k)).unwrap_or("").to_owned()
}

fn value(p: &Part) -> String {
    let explicit = lookup(p, VALUE_KEYS);
    if explicit.trim().is_empty() { p.comment.clone() } else { explicit }
}

/// Value of one grouping field for a part.
pub fn field(p: &Part, name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "comment" => p.comment.clone(),
        "footprint" => p.footprint.clone(),
        "lib_ref" | "libref" | "library_reference" => p.lib_ref.clone(),
        "description" => p.description.clone(),
        "value" => value(p),
        "mpn" => lookup(p, MPN_KEYS),
        "manufacturer" => lookup(p, MFR_KEYS),
        "kind" => p.kind.clone(),
        "designator" => p.designator.clone(),
        "supplier_pn" => lookup(p, SUPPLIER_PN_KEYS),
        _ => p.param(name).unwrap_or("").to_owned(),
    }
}

pub fn build(design: &Design, opts: &BomOptions) -> Result<Bom> {
    if opts.group_by.is_empty() {
        return Err(Error::usage("--group-by needs at least one field"));
    }
    let not_fitted: Vec<String> = opts
        .variant
        .as_ref()
        .map(|v| v.variations.iter().filter(|x| x.kind == "not_fitted").map(|x| x.designator.trim_end_matches('*').to_owned()).collect())
        .unwrap_or_default();
    let is_dnp = |d: &str| not_fitted.iter().any(|n| n.eq_ignore_ascii_case(d));

    let mut groups: BTreeMap<(bool, bool, Vec<String>), Vec<&Part>> = BTreeMap::new();
    let mut dnp = vec![];
    let mut excluded = vec![];
    let mut warnings = vec![];
    let mut varied = vec![];
    let parts = apply_variant(&design.parts, opts.variant.as_ref(), &mut varied, &mut warnings);
    for p in &parts {
        let d = is_dnp(&p.designator);
        if d {
            dnp.push(p.designator.clone());
            if !opts.include_dnp {
                continue;
            }
        }
        if p.exclude_from_bom {
            excluded.push(p.designator.clone());
            if !opts.include_excluded {
                continue;
            }
        }
        let key: Vec<String> = opts.group_by.iter().map(|f| field(p, f)).collect();
        groups.entry((d, p.exclude_from_bom, key)).or_default().push(p);
    }
    for v in opts.variant.iter().flat_map(|v| &v.variations) {
        let d = v.designator.trim_end_matches('*');
        if !design.parts.iter().any(|p| p.designator.eq_ignore_ascii_case(d)) {
            warnings.push(format!("variant names {d} but no such part exists"));
        }
    }

    let mut lines: Vec<BomLine> = groups
        .into_iter()
        .map(|((d, ex, _), parts)| {
            let mut des: Vec<String> = parts.iter().map(|p| p.designator.clone()).collect();
            designator::sort(&mut des);
            let first = parts[0];
            let mut common = IndexMap::new();
            let mut inconsistent = vec![];
            for (k, v) in &first.parameters {
                if parts.iter().all(|p| p.param(k) == Some(v.as_str())) {
                    common.insert(k.clone(), v.clone());
                } else {
                    inconsistent.push(k.clone());
                }
            }
            let uniform = |f: fn(&Part) -> String| {
                let v = f(first);
                if parts.iter().all(|p| f(p) == v) { v } else { "<varies>".to_owned() }
            };
            BomLine {
                line: 0,
                quantity: parts.len(),
                designator_ranges: designator::compress(&des),
                designators: des,
                comment: uniform(|p| p.comment.clone()),
                footprint: uniform(|p| p.footprint.clone()),
                lib_ref: uniform(|p| p.lib_ref.clone()),
                description: uniform(|p| p.description.clone()),
                value: uniform(value),
                manufacturer: uniform(|p| lookup(p, MFR_KEYS)),
                mpn: uniform(|p| lookup(p, MPN_KEYS)),
                supplier_pn: uniform(|p| lookup(p, SUPPLIER_PN_KEYS)),
                parameters: common,
                inconsistent,
                dnp: d,
                excluded: ex,
            }
        })
        .collect();
    lines.sort_by(|a, b| designator::cmp(&a.designators[0], &b.designators[0]));
    for (i, l) in lines.iter_mut().enumerate() {
        l.line = i + 1;
    }
    let fallback_count: usize = lines.iter().filter(|l| !l.value.is_empty())
        .flat_map(|l| &l.designators)
        .filter(|d| parts.iter().any(|p| &p.designator == *d && lookup(p, VALUE_KEYS).trim().is_empty())).count();
    if fallback_count > 0 { warnings.push(format!("value parameter empty for {fallback_count} BOM part(s); using comment as value")); }
    designator::sort(&mut dnp);
    designator::sort(&mut excluded);
    Ok(Bom {
        group_by: opts.group_by.clone(),
        variant: opts.variant.as_ref().map(|v| v.name.clone()),
        total_parts: lines.iter().filter(|l| !l.dnp && !l.excluded).map(|l| l.quantity).sum(),
        unique_lines: lines.len(),
        lines,
        dnp,
        excluded,
        varied,
        warnings,
    })
}

/// Default CSV columns (see [`column`]).
pub const DEFAULT_COLUMNS: &[&str] =
    &["line", "quantity", "designator_ranges", "comment", "footprint", "value", "manufacturer", "mpn", "supplier_pn", "description", "dnp"];

/// One CSV cell of a line. Built-ins: line, quantity, designators (ranges), designator_list,
/// comment, footprint, lib_ref, description, value, manufacturer, mpn, supplier_pn, dnp,
/// excluded; anything else is a parameter common to the line.
pub fn column(l: &BomLine, name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "line" => l.line.to_string(),
        "quantity" | "qty" => l.quantity.to_string(),
        "designator_ranges" | "designators" | "designator" => l.designator_ranges.clone(),
        "designator_list" => l.designators.join(", "),
        "comment" => l.comment.clone(),
        "footprint" => l.footprint.clone(),
        "lib_ref" | "libref" => l.lib_ref.clone(),
        "description" => l.description.clone(),
        "value" => l.value.clone(),
        "manufacturer" => l.manufacturer.clone(),
        "mpn" => l.mpn.clone(),
        "supplier_pn" => l.supplier_pn.clone(),
        "dnp" => if l.dnp { "DNP".into() } else { String::new() },
        "excluded" => if l.excluded { "excluded".into() } else { String::new() },
        _ => l.parameters.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone()).unwrap_or_default(),
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

/// CSV with the given columns (header = column names as given).
pub fn to_csv_columns(bom: &Bom, columns: &[String]) -> String {
    let mut out = columns.iter().map(|c| csv_escape(c)).collect::<Vec<_>>().join(",");
    out.push('\n');
    for l in &bom.lines {
        out.push_str(&columns.iter().map(|c| csv_escape(&column(l, c))).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

/// CSV with a fixed, spreadsheet-friendly column set.
pub fn to_csv(bom: &Bom) -> String {
    let esc = |s: &str| csv_escape(s);
    let mut out = String::from("Line,Quantity,Designators,Comment,Footprint,Value,Manufacturer,MPN,Supplier PN,Description,DNP\n");
    for l in &bom.lines {
        let row = [
            l.line.to_string(),
            l.quantity.to_string(),
            l.designator_ranges.clone(),
            l.comment.clone(),
            l.footprint.clone(),
            l.value.clone(),
            l.manufacturer.clone(),
            l.mpn.clone(),
            l.supplier_pn.clone(),
            l.description.clone(),
            if l.dnp { "DNP".into() } else { String::new() },
        ];
        out.push_str(&row.iter().map(|c| esc(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_doc::project::Variation;

    fn part(d: &str, c: &str, fp: &str) -> Part {
        Part { designator: d.into(), comment: c.into(), footprint: fp.into(), ..Default::default() }
    }

    #[test]
    fn grouping_and_variant() {
        let design = Design {
            source: "schematic".into(),
            parts: vec![part("R2", "10k", "0603"), part("R1", "10k", "0603"), part("C1", "100n", "0402"), part("R3", "1k", "0603")],
            ..Default::default()
        };
        let v = Variant {
            index: 1,
            name: "Lite".into(),
            unique_id: String::new(),
            allow_fabrication: true,
            variations: vec![Variation {
                designator: "R3".into(),
                unique_id: String::new(),
                kind: "not_fitted".into(),
                kind_code: 1,
                alternate_part: String::new(),
                raw: Default::default(),
            }],
            parameters: Default::default(),
            param_variations: vec![[("Designator", "R2"), ("ParameterName", "Comment"), ("VariantValue", "22k")]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()],
        };
        let bom = build(&design, &BomOptions { variant: Some(v.clone()), ..Default::default() }).unwrap();
        assert_eq!(bom.unique_lines, 3, "R2 became 22k");
        assert_eq!(bom.varied, ["R2"]);
        let mut v = v;
        v.param_variations.clear();
        let bom = build(&design, &BomOptions { variant: Some(v), ..Default::default() }).unwrap();
        assert_eq!(bom.unique_lines, 2);
        assert_eq!(bom.lines[1].designator_ranges, "R1, R2");
        assert_eq!(bom.dnp, ["R3"]);
        assert_eq!(bom.total_parts, 3);
        assert!(to_csv(&bom).contains("2,2,\"R1, R2\",10k,0603"));
        let cols: Vec<String> = ["designator_list", "qty"].iter().map(|s| s.to_string()).collect();
        assert!(to_csv_columns(&bom, &cols).contains("\"R1, R2\",2"));
    }
}
