//! Netlist views and exports.

use serde::Serialize;

use altium_doc::model::{Design, Net};

use crate::designator;

#[derive(Debug, Clone, Serialize)]
pub struct NetSummary {
    pub name: String,
    pub node_count: usize,
    /// `R1.1`, `U3.14`, natural-sorted.
    pub nodes: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_named: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name_source: String,
}

pub fn summarize(net: &Net) -> NetSummary {
    let mut nodes: Vec<String> = net.nodes.iter().map(|n| format!("{}.{}", n.designator, n.pin)).collect();
    nodes.sort_by(|a, b| designator::cmp(a, b));
    nodes.dedup();
    NetSummary {
        name: net.name.clone(),
        node_count: nodes.len(),
        nodes,
        auto_named: net.auto_named,
        name_source: net.name_source.clone(),
    }
}

/// Nets sorted by name, with sorted nodes.
pub fn nets(design: &Design) -> Vec<NetSummary> {
    let mut v: Vec<NetSummary> = design.nets.iter().map(summarize).collect();
    v.sort_by(|a, b| designator::cmp(&a.name, &b.name));
    v
}

/// One `(designator, pin) -> net` row per node.
pub fn pin_map(design: &Design) -> Vec<(String, String, String)> {
    let mut v: Vec<(String, String, String)> = design
        .nets
        .iter()
        .flat_map(|n| n.nodes.iter().map(move |x| (x.designator.clone(), x.pin.clone(), n.name.clone())))
        .collect();
    v.sort_by(|a, b| designator::cmp(&a.0, &b.0).then(designator::cmp(&a.1, &b.1)));
    v
}

/// Protel/Tango netlist (`[` part `]`, `(` net `)`), readable by most EDA tools.
pub fn to_protel(design: &Design) -> String {
    let mut s = String::new();
    let mut parts = design.parts.clone();
    parts.sort_by(|a, b| designator::cmp(&a.designator, &b.designator));
    for p in &parts {
        s.push_str(&format!("[\n{}\n{}\n{}\n\n\n\n]\n", p.designator, p.footprint, p.comment));
    }
    for n in nets(design) {
        s.push_str(&format!("(\n{}\n", n.name));
        for node in &n.nodes {
            s.push_str(&node.replacen('.', "-", 1));
            s.push('\n');
        }
        s.push_str(")\n");
    }
    s
}

pub fn to_csv(design: &Design) -> String {
    let mut s = String::from("Net,Designator,Pin\n");
    for (d, p, n) in pin_map(design) {
        let q = |x: &str| if x.contains([',', '"']) { format!("\"{}\"", x.replace('"', "\"\"")) } else { x.to_owned() };
        s.push_str(&format!("{},{},{}\n", q(&n), q(&d), q(&p)));
    }
    s
}

/// One pin of the netlist (the `--pins` view).
#[derive(Debug, Clone, Serialize)]
pub struct PinRow {
    pub designator: String,
    /// Pin / pad number.
    pub pin: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pin_name: String,
    pub net: String,
    /// `designator.pin`, the node reference used in `nodes` lists.
    pub node: String,
}

/// Every node with its net, natural-sorted by designator then pin.
pub fn pins(design: &Design) -> Vec<PinRow> {
    let mut v: Vec<PinRow> = design
        .nets
        .iter()
        .flat_map(|n| {
            n.nodes.iter().map(move |x| PinRow {
                designator: x.designator.clone(),
                pin: x.pin.clone(),
                pin_name: x.pin_name.clone(),
                net: n.name.clone(),
                node: format!("{}.{}", x.designator, x.pin),
            })
        })
        .collect();
    v.sort_by(|a, b| designator::cmp(&a.designator, &b.designator).then(designator::cmp(&a.pin, &b.pin)).then(a.net.cmp(&b.net)));
    v.dedup_by(|a, b| a.node == b.node && a.net == b.net);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_doc::model::{Node, Part};

    #[test]
    fn exports() {
        let n = |d: &str, p: &str| Node { designator: d.into(), pin: p.into(), pin_name: String::new() };
        let design = Design {
            source: "schematic".into(),
            parts: vec![Part { designator: "R1".into(), footprint: "0603".into(), comment: "10k".into(), ..Default::default() }],
            nets: vec![Net { name: "GND".into(), nodes: vec![n("R1", "2"), n("R10", "1"), n("R2", "1")], ..Default::default() }],
            ..Default::default()
        };
        assert_eq!(nets(&design)[0].nodes, ["R1.2", "R2.1", "R10.1"]);
        assert!(to_protel(&design).contains("[\nR1\n0603\n10k\n"));
        assert!(to_protel(&design).contains("(\nGND\nR1-2\nR2-1\nR10-1\n)"));
        assert_eq!(to_csv(&design).lines().nth(1), Some("GND,R1,2"));
        assert_eq!(pins(&design).len(), 3);
    }
}
