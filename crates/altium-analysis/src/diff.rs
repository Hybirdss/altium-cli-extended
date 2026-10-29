//! Compare two designs (two revisions, or schematic vs PCB): parts and nets.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use altium_doc::model::{Design, Part};
use altium_doc::pcb::Rule;

use crate::designator;

#[derive(Debug, Clone, Serialize)]
pub struct FieldChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PartChange {
    pub designator: String,
    pub changes: Vec<FieldChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetChange {
    pub net: String,
    pub added_nodes: Vec<String>,
    pub removed_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetRename {
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MatchBy { #[default] Auto, Designator, UniqueId }

#[derive(Debug, Clone, Serialize)]
pub struct PartRename {
    pub before_ref: String,
    pub designator: String,
    pub unique_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diff {
    pub parts_added: Vec<String>,
    pub parts_removed: Vec<String>,
    pub parts_changed: Vec<PartChange>,
    pub parts_renamed: Vec<PartRename>,
    pub nets_added: Vec<String>,
    pub nets_removed: Vec<String>,
    /// Same nodes, new name.
    pub nets_renamed: Vec<NetRename>,
    pub nets_changed: Vec<NetChange>,
    pub identical: bool,
}

fn part_fields(p: &Part) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("comment".to_owned(), p.comment.clone());
    m.insert("footprint".to_owned(), p.footprint.clone());
    m.insert("lib_ref".to_owned(), p.lib_ref.clone());
    m.insert("description".to_owned(), p.description.clone());
    if let Some(s) = &p.side {
        m.insert("side".to_owned(), s.clone());
    }
    if let (Some(x), Some(y)) = (p.x, p.y) {
        // In the output unit (`--units`); compared on that rounding.
        m.insert("position".to_owned(), format!("{},{}", x.out(), y.out()));
    }
    if let Some(r) = p.rotation {
        m.insert("rotation".to_owned(), format!("{r:.2}"));
    }
    for (k, v) in &p.parameters {
        m.insert(format!("param:{k}"), v.clone());
    }
    m
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    designator::sort(&mut v);
    v
}

/// `geometry` = include position/rotation/side changes (off for sch-vs-pcb comparisons).
pub fn diff(a: &Design, b: &Design, geometry: bool) -> Diff {
    diff_with(a, b, geometry, MatchBy::Auto)
}

/// Stable nonempty IDs are matched only when unique on both sides. Auto then falls back
/// to designators, so old files without IDs remain comparable without guessed renames.
pub fn diff_with(a: &Design, b: &Design, geometry: bool, matching: MatchBy) -> Diff {
    let pa: BTreeMap<String, &Part> = a.parts.iter().map(|p| (p.designator.clone(), p)).collect();
    let pb: BTreeMap<String, &Part> = b.parts.iter().map(|p| (p.designator.clone(), p)).collect();
    let unique_ids = |parts: &BTreeMap<String, &Part>| {
        let mut ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, part) in parts {
            if !part.unique_id.is_empty() { ids.entry(part.unique_id.clone()).or_default().push(name.clone()); }
        }
        ids.into_iter().filter_map(|(id, names)| (names.len() == 1).then(|| (id, names[0].clone()))).collect::<BTreeMap<_, _>>()
    };
    let mut pairs: BTreeMap<String, String> = BTreeMap::new();
    let mut used = BTreeSet::new();
    if matching != MatchBy::Designator {
        let (ia, ib) = (unique_ids(&pa), unique_ids(&pb));
        for (id, name) in ia {
            if let Some(other) = ib.get(&id) { pairs.insert(name, other.clone()); used.insert(other.clone()); }
        }
    }
    if matching != MatchBy::UniqueId {
        for name in pa.keys() {
            if !pairs.contains_key(name) && pb.contains_key(name) && !used.contains(name) {
                pairs.insert(name.clone(), name.clone()); used.insert(name.clone());
            }
        }
    }
    let parts_added = sorted(pb.keys().filter(|k| !used.contains(*k)).cloned().collect());
    let parts_removed = sorted(pa.keys().filter(|k| !pairs.contains_key(*k)).cloned().collect());
    let parts_renamed: Vec<_> = pairs.iter().filter(|(old, new)| old != new).map(|(old, new)| PartRename {
        before_ref: old.clone(), designator: new.clone(), unique_id: pb[new].unique_id.clone(),
    }).collect();
    let mut parts_changed = vec![];
    for (old, k) in &pairs {
        let (x, y) = (pa[old], pb[k]);
        let (fx, fy) = (part_fields(x), part_fields(y));
        let keys: BTreeSet<&String> = fx.keys().chain(fy.keys()).collect();
        let changes: Vec<FieldChange> = keys
            .into_iter()
            .filter(|f| geometry || !matches!(f.as_str(), "position" | "rotation" | "side"))
            .filter_map(|f| {
                let (bv, av) = (fx.get(f).cloned().unwrap_or_default(), fy.get(f).cloned().unwrap_or_default());
                (bv != av).then(|| FieldChange { field: f.clone(), before: bv, after: av })
            })
            .collect();
        if !changes.is_empty() {
            parts_changed.push(PartChange { designator: k.clone(), changes });
        }
    }
    parts_changed.sort_by(|a, b| designator::cmp(&a.designator, &b.designator));

    let nodes = |d: &Design, before: bool| -> BTreeMap<String, BTreeSet<String>> {
        d.nets
            .iter()
            .map(|n| (n.name.clone(), n.nodes.iter().map(|x| {
                let name = if before { pairs.get(&x.designator).unwrap_or(&x.designator) } else { &x.designator };
                format!("{name}.{}", x.pin)
            }).collect()))
            .collect()
    };
    let (na, nb) = (nodes(a, true), nodes(b, false));
    let mut nets_added = sorted(nb.keys().filter(|k| !na.contains_key(*k)).cloned().collect());
    let mut nets_removed = sorted(na.keys().filter(|k| !nb.contains_key(*k)).cloned().collect());
    // A removed and an added net with the same (non-empty) node set is a rename.
    let mut nets_renamed = vec![];
    nets_removed.retain(|old| {
        let set = &na[old];
        match nets_added.iter().position(|new| !set.is_empty() && &nb[new] == set) {
            Some(i) => {
                nets_renamed.push(NetRename { before: old.clone(), after: nets_added.remove(i) });
                false
            }
            None => true,
        }
    });
    let mut nets_changed = vec![];
    for (k, x) in &na {
        let Some(y) = nb.get(k) else { continue };
        let added = sorted(y.difference(x).cloned().collect());
        let removed = sorted(x.difference(y).cloned().collect());
        if !added.is_empty() || !removed.is_empty() {
            nets_changed.push(NetChange { net: k.clone(), added_nodes: added, removed_nodes: removed });
        }
    }
    let identical = parts_added.is_empty()
        && parts_removed.is_empty()
        && parts_changed.is_empty()
        && parts_renamed.is_empty()
        && nets_added.is_empty()
        && nets_removed.is_empty()
        && nets_renamed.is_empty()
        && nets_changed.is_empty();
    Diff { parts_added, parts_removed, parts_changed, parts_renamed, nets_added, nets_removed, nets_renamed, nets_changed, identical }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleChange {
    pub rule: String,
    pub kind: String,
    pub changes: Vec<RuleFieldChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleFieldChange {
    pub field: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RuleDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<RuleChange>,
}

fn rule_fields(r: &Rule) -> BTreeMap<String, serde_json::Value> {
    use serde_json::json;
    let mut m = BTreeMap::new();
    for (key, value) in [("kind", json!(r.kind)), ("comment", json!(r.comment)), ("net_scope", json!(r.net_scope)),
        ("layer_kind", json!(r.layer_kind)), ("enabled", json!(r.enabled)), ("priority", json!(r.priority)),
        ("scope1", json!(r.scope1)), ("scope2", json!(r.scope2))] { m.insert(key.into(), value); }
    for (k, v) in &r.values { m.insert(k.clone(), json!(v)); }
    m
}

/// PCB design rules matched by name: added, removed, and changed settings.
pub fn rules(a: &[Rule], b: &[Rule]) -> RuleDiff {
    let ra: BTreeMap<&str, &Rule> = a.iter().map(|r| (r.name.as_str(), r)).collect();
    let rb: BTreeMap<&str, &Rule> = b.iter().map(|r| (r.name.as_str(), r)).collect();
    let mut d = RuleDiff {
        added: rb.keys().filter(|k| !ra.contains_key(*k)).map(|k| k.to_string()).collect(),
        removed: ra.keys().filter(|k| !rb.contains_key(*k)).map(|k| k.to_string()).collect(),
        changed: vec![],
    };
    for (name, x) in &ra {
        let Some(y) = rb.get(name) else { continue };
        let (fx, fy) = (rule_fields(x), rule_fields(y));
        let keys: BTreeSet<&String> = fx.keys().chain(fy.keys()).collect();
        let changes: Vec<RuleFieldChange> = keys
            .into_iter()
            .filter_map(|f| {
                let (bv, av) = (fx.get(f).cloned().unwrap_or_default(), fy.get(f).cloned().unwrap_or_default());
                (bv != av).then(|| RuleFieldChange { field: f.clone(), before: bv, after: av,
                    unit: x.values.get(f).or_else(|| y.values.get(f)).and_then(|v| matches!(v, altium_doc::pcb::records::Value::Len(_))
                        .then(|| altium_core::units::output_unit().name())) })
            })
            .collect();
        if !changes.is_empty() {
            d.changed.push(RuleChange { rule: name.to_string(), kind: y.kind.clone(), changes });
        }
    }
    d
}

/// Project-level changes, independent of the chosen schematic/PCB comparison source.
pub fn project(a: &altium_doc::project::Project, b: &altium_doc::project::Project) -> Vec<serde_json::Value> {
    use serde_json::json;
    let variants = |p: &altium_doc::project::Project| p.variants().into_iter().map(|v| (v.name.clone(), v)).collect::<BTreeMap<_, _>>();
    let (av, bv) = (variants(a), variants(b));
    let variations = |v: &altium_doc::project::Variant| {
        let mut result = BTreeMap::new();
        for r in &v.variations { result.insert(r.designator.clone(), json!(r)); }
        for r in &v.param_variations {
            let name = r.iter().find(|(k, _)| k.eq_ignore_ascii_case("Designator")).map(|(_, v)| v.clone()).unwrap_or_default();
            result.insert(format!("{name}:parameters"), json!(r));
        }
        result
    };
    let mut changes = vec![];
    for name in av.keys().chain(bv.keys()).collect::<BTreeSet<_>>() {
        match (av.get(name), bv.get(name)) {
            (None, Some(v)) => {
                changes.push(json!({"change":"variant_added", "ref": name, "after": v}));
                for (key, value) in variations(v) {
                    changes.push(json!({"change":"variation_changed","ref":key.split(':').next().unwrap_or(&key),
                        "variant":name,"before":null,"after":value}));
                }
            },
            (Some(v), None) => changes.push(json!({"change":"variant_removed", "ref": name, "before": v})),
            (Some(x), Some(y)) => {
                let (ax, by) = (variations(x), variations(y));
                for key in ax.keys().chain(by.keys()).collect::<BTreeSet<_>>() {
                    if ax.get(key) != by.get(key) { changes.push(json!({"change":"variation_changed", "ref": key.split(':').next().unwrap_or(key), "variant":name, "before":ax.get(key), "after":by.get(key)})); }
                }
                if x.parameters != y.parameters || x.allow_fabrication != y.allow_fabrication {
                    changes.push(json!({"change":"variant_changed", "ref": name,
                        "before":{"parameters":x.parameters,"allow_fabrication":x.allow_fabrication},
                        "after":{"parameters":y.parameters,"allow_fabrication":y.allow_fabrication}}));
                }
            }
            _ => {}
        }
    }
    let fields = |p: &altium_doc::project::Project| {
        let mut fields = BTreeMap::new();
        for section in &p.ini.sections {
            if section.name.starts_with("ProjectVariant") || section.name.starts_with("GeneratedDocument") { continue; }
            for (key, value) in &section.entries { fields.insert(format!("{}.{}", section.name, key), value.clone()); }
        }
        fields
    };
    let (ax, by) = (fields(a), fields(b));
    for key in ax.keys().chain(by.keys()).collect::<BTreeSet<_>>() {
        if ax.get(key) != by.get(key) { changes.push(json!({"change":"project_setting_changed", "ref":key, "before":ax.get(key), "after":by.get(key)})); }
    }
    changes
}

/// Compare decoded library objects by name. Unknown binary streams are outside this scope.
pub fn library(a: &[serde_json::Value], b: &[serde_json::Value], kind: &str) -> Vec<serde_json::Value> {
    use serde_json::{Value, json};
    let named = |objects: &[Value]| objects.iter().filter_map(|v| v["name"].as_str().map(|n| (n.to_owned(), v.clone()))).collect::<BTreeMap<_, _>>();
    let (a, b) = (named(a), named(b));
    let mut items = vec![];
    for name in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
        match (a.get(name), b.get(name)) {
            (None, Some(v)) => items.push(json!({"change":format!("{kind}_added"),"ref":name,"after":v})),
            (Some(v), None) => items.push(json!({"change":format!("{kind}_removed"),"ref":name,"before":v})),
            (Some(x), Some(y)) => {
                let (Some(x), Some(y)) = (x.as_object(), y.as_object()) else { continue };
                for field in x.keys().chain(y.keys()).collect::<BTreeSet<_>>() {
                    if ["index", "storage", "streams", "warnings", "raw"].contains(&field.as_str()) || x.get(field) == y.get(field) { continue; }
                    let change = match field.as_str() {
                        "pads" => "pads_changed", "models" | "bodies" => "model_changed",
                        "height" | "parameters" | "description" => "param_changed", _ => "library_field_changed",
                    };
                    items.push(json!({"change":change,"ref":name,"field":field,"before":x.get(field),"after":y.get(field)}));
                }
            }
            _ => {}
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_doc::model::{Net, Node};

    #[test]
    fn project_settings_exclude_typed_variants_and_generated_documents() {
        use altium_doc::project::Project;
        use std::path::Path;

        let before = Project::from_bytes(Path::new("before.PrjPcb"), b"[Design]\nName=Before\n").unwrap();
        let after = Project::from_bytes(Path::new("after.PrjPcb"), b"[Design]\nName=After\n\
            [ProjectVariant1]\nDescription=Lite\nUniqueID=ABCDEF\nAllowFabrication=1\n\
            [GeneratedDocument1]\nDocumentPath=generated.OutJob\n").unwrap();
        let changes = project(&before, &after);
        assert!(changes.iter().any(|r| r["change"] == "variant_added" && r["ref"] == "Lite"));
        let settings: Vec<_> = changes.iter().filter(|r| r["change"] == "project_setting_changed").collect();
        assert_eq!(settings.len(), 1, "{changes:?}");
        assert_eq!(settings[0]["ref"], "Design.Name");
        assert_eq!(settings[0]["before"], "Before");
        assert_eq!(settings[0]["after"], "After");
    }

    #[test]
    fn rule_metadata_changes_are_reported() {
        use altium_doc::pcb::records::rule;
        use altium_format::props::Props;

        let before = rule(0, 0, Props::parse(b"|NAME=Clearance|RULEKIND=Clearance"), false);
        for (key, field, value) in [
            ("COMMENT", "comment", "QA comment"),
            ("NETSCOPE", "net_scope", "DifferentNetsOnly"),
            ("LAYERKIND", "layer_kind", "SameLayer"),
            ("RULEKIND", "kind", "Width"),
        ] {
            let mut props = Props::parse(b"|NAME=Clearance|RULEKIND=Clearance");
            props.set(key, value);
            let after = rule(0, 0, props, false);
            let d = rules(std::slice::from_ref(&before), &[after]);
            assert_eq!(d.changed.len(), 1, "missing {field} change");
            assert_eq!(d.changed[0].changes.len(), 1);
            assert_eq!(d.changed[0].changes[0].field, field);
            assert_eq!(d.changed[0].changes[0].after, value);
        }
        assert!(rules(std::slice::from_ref(&before), std::slice::from_ref(&before)).changed.is_empty());
    }

    #[test]
    fn parts_and_nets() {
        let p = |d: &str, c: &str| Part { designator: d.into(), comment: c.into(), ..Default::default() };
        let n = |name: &str, nodes: &[&str]| Net {
            name: name.into(),
            nodes: nodes.iter().map(|x| Node { designator: x.to_string(), pin: "1".into(), pin_name: String::new() }).collect(),
            ..Default::default()
        };
        let a = Design { parts: vec![p("R1", "10k"), p("R2", "1k")], nets: vec![n("GND", &["R1"])], ..Default::default() };
        let b = Design { parts: vec![p("R1", "22k"), p("C1", "1u")], nets: vec![n("GND", &["R1", "C1"])], ..Default::default() };
        let d = diff(&a, &b, false);
        assert_eq!(d.parts_added, ["C1"]);
        assert_eq!(d.parts_removed, ["R2"]);
        assert_eq!(d.parts_changed[0].changes[0].after, "22k");
        assert_eq!(d.nets_changed[0].added_nodes, ["C1.1"]);
        assert!(!d.identical);
        assert!(diff(&a, &a, true).identical);
    }
}
