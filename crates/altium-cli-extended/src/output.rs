//! The response envelope and output shaping.
//!
//! Every command prints exactly one JSON document on stdout:
//!
//! ```json
//! {"ok":true,"command":"pcb nets","data":{...},"warnings":[],"meta":{"units":"mm",...}}
//! {"ok":false,"command":"pcb nets","error":{"code":"FILE_NOT_FOUND","message":"...","hint":"..."}}
//! ```
//!
//! List commands return `data = {"count": N, "items": [...], ...}`; the global `--fields`,
//! `--where` and `--limit` flags act on `items`.

use serde::Serialize;
use serde_json::{Map, Value, json};

use altium_core::error::{Error, Result};

/// Early consumer exit is normal for CLI pipelines on every platform.
pub fn write_stdout(text: &str) {
    use std::io::Write;
    if let Err(e) = std::io::stdout().lock().write_all(text.as_bytes()) {
        if e.kind() == std::io::ErrorKind::BrokenPipe { std::process::exit(0); }
        eprintln!("cannot write stdout: {e}");
        std::process::exit(altium_core::ErrorCode::Io.exit_code());
    }
}

pub const SCHEMA: &str = "altium-cli-extended/v1";

/// Data plus non-fatal warnings, returned by every command handler.
#[derive(Debug, Default)]
pub struct Out {
    pub data: Value,
    pub warnings: Vec<String>,
    pub exit_code: i32,
}

impl Out {
    pub fn new(data: impl Serialize) -> Result<Out> {
        Ok(Out { data: serde_json::to_value(data)?, warnings: vec![], exit_code: 0 })
    }
    pub fn gate(mut self, fail_on: &str, matched: usize) -> Self {
        self.data["gate"] = json!({"fail_on": fail_on, "matched": matched,
            "hint": "inspect findings before retrying; omit the gate flag for a report-only run"});
        if matched > 0 {
            self.exit_code = altium_core::ErrorCode::GateFailed.exit_code();
        }
        self
    }
    pub fn warn(mut self, w: impl Into<String>) -> Out {
        self.warnings.push(w.into());
        self
    }
    pub fn warnings(mut self, w: impl IntoIterator<Item = String>) -> Out {
        self.warnings.extend(w);
        self
    }
}

/// `{"count": n, "items": [...]}` with optional extra top-level fields.
pub fn list<T: Serialize>(items: &[T]) -> Result<Value> {
    Ok(json!({ "count": items.len(), "items": serde_json::to_value(items)? }))
}

pub fn list_with<T: Serialize>(items: &[T], extra: Value) -> Result<Value> {
    let mut v = list(items)?;
    if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) {
        for (k, val) in e {
            o.insert(k, val);
        }
    }
    Ok(v)
}

#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub fields: Vec<String>,
    pub filters: Vec<Filter>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Filter {
    pub key: String,
    pub op: FilterOp,
}

#[derive(Debug, Clone)]
pub enum FilterOp {
    Eq(String),
    Ne(String),
    Regex(regex::Regex),
    Contains(String),
}

impl Filter {
    /// `key=value` (exact, case-insensitive), `key!=value`, `key~regex`, `key^=substring`.
    pub fn parse(s: &str) -> Result<Filter> {
        let bad = || {
            Error::usage(format!("bad --where {s:?}"))
                .hint("use key=value, key!=value, key~regex or key^=substring; dotted keys reach nested fields (pad.net=GND)")
        };
        let (key, op) = if let Some((k, v)) = s.split_once("!=") {
            (k, FilterOp::Ne(v.to_owned()))
        } else if let Some((k, v)) = s.split_once("^=") {
            (k, FilterOp::Contains(v.to_lowercase()))
        } else if let Some((k, v)) = s.split_once('~') {
            let re = regex::RegexBuilder::new(v)
                .case_insensitive(true)
                .build()
                .map_err(|e| Error::usage(format!("bad regex in --where: {e}")))?;
            (k, FilterOp::Regex(re))
        } else if let Some((k, v)) = s.split_once('=') {
            (k, FilterOp::Eq(v.to_owned()))
        } else {
            return Err(bad());
        };
        if key.is_empty() {
            return Err(bad());
        }
        Ok(Filter { key: key.to_owned(), op })
    }

    fn matches(&self, item: &Value) -> bool {
        let v = lookup(item, &self.key);
        let text = match v {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
        };
        match &self.op {
            FilterOp::Eq(want) => text.eq_ignore_ascii_case(want),
            FilterOp::Ne(want) => !text.eq_ignore_ascii_case(want),
            FilterOp::Regex(re) => re.is_match(&text),
            FilterOp::Contains(sub) => text.to_lowercase().contains(sub),
        }
    }
}

fn lookup<'a>(v: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut cur = v;
    for part in dotted.split('.') {
        cur = match cur {
            Value::Object(m) => m
                .get(part)
                .or_else(|| m.iter().find(|(k, _)| k.eq_ignore_ascii_case(part)).map(|(_, v)| v))?,
            _ => return None,
        };
    }
    Some(cur)
}

impl Shape {
    /// Explain empty queries and unknown fields before projection destroys the evidence.
    pub fn diagnostics(&self, data: &Value) -> (Vec<String>, Value) {
        let items = data.get("items").and_then(Value::as_array).or_else(|| data.as_array());
        let mut warnings = vec![];
        let mut similar = Map::new();
        let rows: Vec<&Value> = items.map(|rows| rows.iter().collect()).unwrap_or_else(|| vec![data]);
        let unknown: Vec<_> = self.fields.iter().filter(|field| !rows.is_empty() && rows.iter().all(|r| lookup(r, field).is_none())).collect();
        if !unknown.is_empty() {
            let valid: std::collections::BTreeSet<_> = rows.iter().filter_map(|r| r.as_object()).flat_map(|r| r.keys()).collect();
            warnings.push(format!("unknown field(s): {}; valid: {}", unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), valid.into_iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
        }
        if let Some(items) = items {
            if !items.is_empty() && !self.filters.is_empty() && !items.iter().any(|it| self.filters.iter().all(|f| f.matches(it))) {
                warnings.push("no items satisfy all --where filters; relax a filter or inspect the unfiltered list".into());
                for filter in &self.filters {
                    if items.iter().any(|it| filter.matches(it)) { continue; }
                    if let FilterOp::Eq(want) = &filter.op {
                        let available: std::collections::BTreeSet<_> = items.iter().filter_map(|it| lookup(it, &filter.key).and_then(Value::as_str)).collect();
                        let names: Vec<_> = available.into_iter().take(20).collect();
                        if filter.key == "designator" {
                            warnings.push(format!("no part with designator {want} (parts: {})", names.join(", ")));
                        } else {
                            warnings.push(format!("no matches for {}={want}; available: {}", filter.key, names.join(", ")));
                        }
                        similar.insert(filter.key.clone(), json!(names.into_iter().take(5).collect::<Vec<_>>()));
                    }
                }
            }
        } else if !self.filters.is_empty() {
            warnings.push("--where applies to list items; this command returns a single object".into());
        }
        (warnings, Value::Object(similar))
    }

    /// Apply to list items, or project fields of a single-object result.
    pub fn apply(&self, data: &mut Value) {
        if !self.fields.is_empty() && data.is_object() && data.get("items").is_none() {
            let mut projected = Map::new();
            for field in &self.fields {
                if let Some(value) = lookup(data, field) { projected.insert(field.clone(), value.clone()); }
            }
            *data = Value::Object(projected);
            return;
        }
        let items = match data {
            Value::Array(a) => Some(a),
            Value::Object(o) => o.get_mut("items").and_then(Value::as_array_mut),
            _ => None,
        };
        let Some(items) = items else { return };
        if !self.filters.is_empty() {
            items.retain(|it| self.filters.iter().all(|f| f.matches(it)));
        }
        let total = items.len();
        if let Some(l) = self.limit {
            items.truncate(l);
        }
        if !self.fields.is_empty() {
            for it in items.iter_mut() {
                let mut m = Map::new();
                for f in &self.fields {
                    if let Some(v) = lookup(it, f) {
                        m.insert(f.clone(), v.clone());
                    }
                }
                *it = Value::Object(m);
            }
        }
        let shown = items.len();
        if let Value::Object(o) = data {
            o.insert("count".into(), json!(shown));
            o.insert("matched".into(), json!(total));
            if shown < total {
                o.insert("truncated".into(), json!(true));
            }
        }
    }
}

/// Plain-text rendering for humans at a terminal.
pub fn render_text(data: &Value) -> String {
    let items = match data {
        Value::Array(a) => Some(a.as_slice()),
        Value::Object(o) => o.get("items").and_then(Value::as_array).map(Vec::as_slice),
        _ => None,
    };
    let mut out = String::new();
    if let Value::Object(o) = data {
        for (k, v) in o {
            if k == "items" {
                continue;
            }
            out.push_str(&format!("{k}: {}\n", scalar(v)));
        }
    }
    if let Some(items) = items {
        out.push_str(&table(items));
    } else if !matches!(data, Value::Object(_)) {
        out.push_str(&scalar(data));
        out.push('\n');
    }
    out
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Array(a) if a.iter().all(|x| !x.is_object() && !x.is_array()) => {
            a.iter().map(scalar).collect::<Vec<_>>().join(", ")
        }
        other => other.to_string(),
    }
}

fn table(items: &[Value]) -> String {
    fn flatten(prefix: &str, value: &Value, fields: &mut Map<String, Value>) {
        match value {
            Value::Object(object) => for (key, value) in object {
                let name = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                flatten(&name, value, fields);
            },
            Value::Array(rows) if rows.iter().any(|v| v.is_object() || v.is_array()) => {},
            _ => { fields.insert(prefix.into(), value.clone()); }
        }
    }
    let rows: Vec<_> = items.iter().map(|item| {
        if !item.is_object() { return item.clone(); }
        let mut fields = Map::new(); flatten("", item, &mut fields); Value::Object(fields)
    }).collect();
    let mut out = flat_table(&rows);
    for (index, item) in items.iter().enumerate() {
        if let Some(object) = item.as_object() {
            for (key, value) in object {
                if let Some(rows) = value.as_array().filter(|rows| rows.iter().any(Value::is_object)) {
                    let label = item.get("designator").or_else(|| item.get("name")).map(scalar).unwrap_or_else(|| index.to_string());
                    out.push_str(&format!("\n{label} {key}:\n"));
                    out.push_str(&table(rows));
                }
            }
        }
    }
    out
}

fn flat_table(items: &[Value]) -> String {
    let mut cols: Vec<String> = Vec::new();
    for it in items {
        if let Value::Object(o) = it {
            for k in o.keys() {
                if !cols.contains(k) {
                    cols.push(k.clone());
                }
            }
        }
    }
    if cols.is_empty() {
        return items.iter().map(|v| scalar(v) + "\n").collect();
    }
    let cell = |it: &Value, c: &str| {
        let s = scalar(it.get(c).unwrap_or(&Value::Null));
        if s.chars().count() > 48 { s.chars().take(45).collect::<String>() + "..." } else { s }
    };
    let widths: Vec<usize> = cols
        .iter()
        .map(|c| items.iter().map(|it| cell(it, c).chars().count()).max().unwrap_or(0).max(c.len()))
        .collect();
    let mut out = String::new();
    let line = |vals: Vec<String>| {
        vals.iter()
            .zip(&widths)
            .map(|(v, w)| format!("{v:<w$}"))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_owned()
            + "\n"
    };
    out.push_str(&line(cols.clone()));
    out.push_str(&line(widths.iter().map(|w| "-".repeat(*w)).collect()));
    for it in items {
        out.push_str(&line(cols.iter().map(|c| cell(it, c)).collect()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_filters() {
        let mut d = json!({"count":3,"items":[
            {"designator":"R1","value":"10k","pad":{"net":"GND"}},
            {"designator":"R2","value":"1k","pad":{"net":"VCC"}},
            {"designator":"C1","value":"100n","pad":{"net":"GND"}}]});
        let s = Shape {
            fields: vec!["designator".into()],
            filters: vec![Filter::parse("pad.net=gnd").unwrap(), Filter::parse("designator~^R").unwrap()],
            limit: None,
        };
        s.apply(&mut d);
        assert_eq!(d["count"], 1);
        assert_eq!(d["items"][0], json!({"designator":"R1"}));
    }
}
