//! Sheet-level connectivity: which pins, labels, ports and wires form one net on a sheet.
//!
//! This is a native approximation of Altium's compiler; `ace designer compile` is the
//! authority. Rules implemented (all coordinates exact, in internal units):
//!
//! 1. A wire is a polyline; all its segments are one conductor.
//! 2. Two wires connect when a vertex of one lies anywhere on the other (end-to-end or
//!    T connection — Altium auto-junctions those). Wires that merely cross connect only when a
//!    junction (record 29) sits on the crossing.
//! 3. A connection point — pin hot end, net label, power port, either end of a port, sheet
//!    entry, junction — joins every wire it lies on, and every other connection point at the
//!    same location (a power port placed directly on a pin end, two pins touching...).
//!    Pin hot end = `LOCATION + PINLENGTH` along the pin orientation (`PINCONGLOMERATE & 3`);
//!    SchDoc pin locations and orientations are already absolute (verified on samples: wire
//!    ends coincide with the computed hot ends).
//! 4. The two ends of a port belong to the same port, so they are joined.
//! 5. Within a sheet, net labels with the same name are one net, and so are power ports and
//!    ports with the same name (names compared case-insensitively).
//! 6. Pins of graphical components, and pins of sub-parts / display modes that are not the
//!    placed one, are ignored (filtered in `doc.rs`).
//! 7. Buses are separate conductors: net labels, ports and sheet entries sitting on a bus
//!    (and on no wire) name the bus, not a net. Bus members are bound by name: a bus name
//!    `D[0..7]` stands for nets `D0`..`D7` named by net labels on the wires (see project.rs).
//!
//! Naming priority (lowest wins; Altium "Net identifier" rules):
//! net label (1) < power port (2) < port (3, only if `AllowPortNetNames`) < sheet entry (4, only
//! if `AllowSheetEntryNetNames`) — with `PowerPortNamesTakePriority` swapping 1 and 2. Ties are
//! broken alphabetically; with `NameNetsHierarchically` (the project file's key for "Higher
//! Level Names Take Priority") the shallower sheet wins first. Verified on the samples: a net
//! labelled `SPI1CE0_BCM18` on the top sheet and `CSN` in the child is `CSN` in the PCB.
//! Unnamed nets get Altium's automatic name `Net<designator>_<pin>` from the first node in
//! natural designator/pin order (`auto_named=true`, `name_source=pin`); this matches all 83
//! automatic names of the Quasar ARM PCB.
//!
//! Single-node nets: nets named by a label/port/power port are kept even with one node (they
//! exist in the samples' PCBs); unnamed single-node nets are dropped unless
//! `NetlistSinglePinNets` is on.

use std::collections::HashMap;

use serde::Serialize;

use altium_core::units::Point;

use super::doc::SchDoc;

/// Project options that affect net naming (`[Design]` section of the .PrjPcb).
#[derive(Debug, Clone, Copy)]
pub struct NetOptions {
    pub power_port_names_take_priority: bool,
    pub allow_port_net_names: bool,
    pub allow_sheet_entry_net_names: bool,
    /// Keep nets with a single node in the design (`NetlistSinglePinNets`).
    pub single_pin_nets: bool,
    pub append_sheet_number_to_local_nets: bool,
    /// `NameNetsHierarchically` ("Higher Level Names Take Priority").
    pub higher_level_names_take_priority: bool,
}

impl Default for NetOptions {
    /// Altium's defaults for a new project.
    fn default() -> Self {
        NetOptions {
            power_port_names_take_priority: false,
            allow_port_net_names: false,
            allow_sheet_entry_net_names: true,
            single_pin_nets: false,
            append_sheet_number_to_local_nets: false,
            higher_level_names_take_priority: false,
        }
    }
}

/// Exact test: does `p` lie on segment `a`–`b` (endpoints included)?
pub fn on_segment(p: Point, a: Point, b: Point) -> bool {
    let (px, py) = (p.x.0 as i128, p.y.0 as i128);
    let (ax, ay) = (a.x.0 as i128, a.y.0 as i128);
    let (bx, by) = (b.x.0 as i128, b.y.0 as i128);
    let cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
    if cross != 0 {
        return false;
    }
    px >= ax.min(bx) && px <= ax.max(bx) && py >= ay.min(by) && py <= ay.max(by)
}

pub fn on_polyline(p: Point, pts: &[Point]) -> bool {
    match pts.len() {
        0 => false,
        1 => pts[0] == p,
        _ => pts.windows(2).any(|w| on_segment(p, w[0], w[1])),
    }
}

/// Natural ordering key: `R10` after `R9`, `A10` after `A2`.
pub fn natural_key(s: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut num = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
        } else {
            if !num.is_empty() {
                out.push((std::mem::take(&mut text), num.parse().unwrap_or(u64::MAX)));
                num.clear();
            }
            text.push(ch.to_ascii_uppercase());
        }
    }
    out.push((text, if num.is_empty() { 0 } else { num.parse().unwrap_or(u64::MAX) }));
    out
}

/// Expand a bus name: `D[0..7]`, `A[3..0]`, comma lists `A[0..1],CLK` → member names.
/// `None` when the name is not bus syntax.
pub fn expand_bus(name: &str) -> Option<Vec<String>> {
    if !name.contains('[') {
        return None;
    }
    let mut out = Vec::new();
    for part in name.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(open) = part.find('[') {
            let prefix = &part[..open];
            let body = part[open + 1..].strip_suffix(']')?;
            let (a, b) = body.split_once("..")?;
            let (a, b): (i64, i64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
            if (a - b).abs() > 4096 {
                return None;
            }
            if a <= b {
                out.extend((a..=b).map(|i| format!("{prefix}{i}")));
            } else {
                out.extend((b..=a).rev().map(|i| format!("{prefix}{i}")));
            }
        } else {
            out.push(part.to_owned());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Union-find with path halving.
pub(crate) struct Uf {
    p: Vec<usize>,
}

impl Uf {
    pub fn new(n: usize) -> Uf {
        Uf { p: (0..n).collect() }
    }
    pub fn find(&mut self, mut x: usize) -> usize {
        while self.p[x] != x {
            self.p[x] = self.p[self.p[x]];
            x = self.p[x];
        }
        x
    }
    pub fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.p[ra.max(rb)] = ra.min(rb);
        }
    }
}

/// The members of one sheet-level net (indices into the SchDoc vectors).
#[derive(Debug, Clone, Default)]
pub struct Group {
    /// (component vec index, pin vec index).
    pub pins: Vec<(usize, usize)>,
    pub net_labels: Vec<usize>,
    pub power_ports: Vec<usize>,
    pub ports: Vec<usize>,
    /// (sheet symbol vec index, entry vec index).
    pub sheet_entries: Vec<(usize, usize)>,
    pub wires: Vec<usize>,
    pub junctions: Vec<usize>,
}

impl Group {
    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
            && self.net_labels.is_empty()
            && self.power_ports.is_empty()
            && self.ports.is_empty()
            && self.sheet_entries.is_empty()
    }
}

/// Bus conductors of a sheet and the names attached to them.
#[derive(Debug, Clone, Default)]
pub struct BusGroup {
    pub buses: Vec<usize>,
    pub net_labels: Vec<usize>,
    pub ports: Vec<usize>,
    pub sheet_entries: Vec<(usize, usize)>,
}

impl BusGroup {
    /// Member net names from every bus-syntax name attached to it.
    pub fn members(&self, doc: &SchDoc) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let all = self
            .net_labels
            .iter()
            .map(|&i| doc.net_labels[i].name.clone())
            .chain(self.ports.iter().map(|&i| doc.ports[i].name.clone()))
            .chain(self.sheet_entries.iter().map(|&(s, e)| doc.sheet_symbols[s].entries[e].name.clone()));
        for n in all {
            for m in expand_bus(&n).unwrap_or_default() {
                if !names.iter().any(|x| x.eq_ignore_ascii_case(&m)) {
                    names.push(m);
                }
            }
        }
        names
    }
}

pub struct SheetGraph {
    pub groups: Vec<Group>,
    pub bus_groups: Vec<BusGroup>,
    /// Wires connected to nothing named and no pin.
    pub dangling_wires: usize,
}

#[derive(Clone, Copy)]
enum Item {
    Pin(usize, usize),
    Label(usize),
    Power(usize),
    Port(usize),
    Entry(usize, usize),
    Junction(usize),
}

/// Build the sheet's connectivity graph.
pub fn graph(doc: &SchDoc) -> SheetGraph {
    let nw = doc.wires.len();
    let mut items: Vec<(Item, Point)> = Vec::new();
    for (ci, c) in doc.components.iter().enumerate() {
        if c.is_graphical() {
            continue;
        }
        for (pi, p) in c.pins.iter().enumerate() {
            items.push((Item::Pin(ci, pi), p.hot_end));
        }
    }
    for (i, l) in doc.net_labels.iter().enumerate() {
        items.push((Item::Label(i), l.location));
    }
    for (i, p) in doc.power_ports.iter().enumerate() {
        items.push((Item::Power(i), p.location));
    }
    for (i, p) in doc.ports.iter().enumerate() {
        items.push((Item::Port(i), p.ends[0]));
        items.push((Item::Port(i), p.ends[1]));
    }
    for (si, s) in doc.sheet_symbols.iter().enumerate() {
        for (ei, e) in s.entries.iter().enumerate() {
            items.push((Item::Entry(si, ei), e.location));
        }
    }
    for (i, j) in doc.junctions.iter().enumerate() {
        items.push((Item::Junction(i), j.location));
    }

    let mut uf = Uf::new(nw + items.len());
    // Items on wires.
    let mut on_wire = vec![false; items.len()];
    for (ii, (_, pt)) in items.iter().enumerate() {
        for (wi, w) in doc.wires.iter().enumerate() {
            if on_polyline(*pt, &w.points) {
                uf.union(nw + ii, wi);
                on_wire[ii] = true;
            }
        }
    }
    // Wire to wire: a vertex of one on the other.
    for a in 0..nw {
        for b in a + 1..nw {
            let (wa, wb) = (&doc.wires[a].points, &doc.wires[b].points);
            if wa.iter().any(|p| on_polyline(*p, wb)) || wb.iter().any(|p| on_polyline(*p, wa)) {
                uf.union(a, b);
            }
        }
    }
    // Bus-side items: labels / port ends / sheet entries on a bus and on no wire.
    let on_bus = |pt: Point| doc.buses.iter().position(|b| on_polyline(pt, &b.points));
    let mut bus_item = vec![None; items.len()];
    for (ii, (it, pt)) in items.iter().enumerate() {
        if on_wire[ii] || !matches!(it, Item::Label(_) | Item::Port(_) | Item::Entry(..)) {
            continue;
        }
        bus_item[ii] = on_bus(*pt);
    }
    // A port is one object: when one end sits on a bus and no end on a wire, the whole port
    // belongs to the bus; when any end is on a wire it is a wire port.
    let mut port_bus: HashMap<usize, (bool, Option<usize>)> = HashMap::new();
    for (ii, (it, _)) in items.iter().enumerate() {
        if let Item::Port(p) = it {
            let e = port_bus.entry(*p).or_insert((false, None));
            e.0 |= on_wire[ii];
            e.1 = e.1.or(bus_item[ii]);
        }
    }
    for (ii, (it, _)) in items.iter().enumerate() {
        if let Item::Port(p) = it {
            let (wired, bus) = port_bus[p];
            bus_item[ii] = if wired { None } else { bus };
        }
    }
    // Coincident connection points (bus-side items excluded).
    let mut at: HashMap<(i64, i64), usize> = HashMap::new();
    for (ii, (_, pt)) in items.iter().enumerate() {
        if bus_item[ii].is_some() {
            continue;
        }
        match at.get(&(pt.x.0, pt.y.0)) {
            Some(&first) => uf.union(nw + ii, nw + first),
            None => {
                at.insert((pt.x.0, pt.y.0), ii);
            }
        }
    }
    // Same names within the sheet (labels, power ports, ports; unnamed ones never join).
    let mut by_key: HashMap<(u8, String), usize> = HashMap::new();
    for (ii, (it, _)) in items.iter().enumerate() {
        if bus_item[ii].is_some() {
            continue;
        }
        let key = match it {
            Item::Port(i) => (3u8, doc.ports[*i].name.trim().to_uppercase()),
            Item::Label(i) => (1, doc.net_labels[*i].name.trim().to_uppercase()),
            Item::Power(i) => (2, doc.power_ports[*i].name.trim().to_uppercase()),
            _ => continue,
        };
        if key.1.is_empty() {
            continue;
        }
        match by_key.get(&key) {
            Some(&first) => uf.union(nw + ii, nw + first),
            None => {
                by_key.insert(key, ii);
            }
        }
    }
    // The two ends of one port are one object.
    let mut port_first: HashMap<usize, usize> = HashMap::new();
    for (ii, (it, _)) in items.iter().enumerate() {
        if let Item::Port(i) = it {
            if bus_item[ii].is_some() {
                continue;
            }
            match port_first.get(i) {
                Some(&f) => uf.union(nw + ii, nw + f),
                None => {
                    port_first.insert(*i, ii);
                }
            }
        }
    }

    // Collect groups.
    let mut root_group: HashMap<usize, usize> = HashMap::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut group_of = |uf: &mut Uf, x: usize, groups: &mut Vec<Group>| -> usize {
        let r = uf.find(x);
        *root_group.entry(r).or_insert_with(|| {
            groups.push(Group::default());
            groups.len() - 1
        })
    };
    for (ii, (it, _)) in items.iter().enumerate() {
        if bus_item[ii].is_some() {
            continue;
        }
        let g = group_of(&mut uf, nw + ii, &mut groups);
        let gr = &mut groups[g];
        match *it {
            Item::Pin(c, p) => gr.pins.push((c, p)),
            Item::Label(i) => gr.net_labels.push(i),
            Item::Power(i) => gr.power_ports.push(i),
            Item::Port(i) => {
                if !gr.ports.contains(&i) {
                    gr.ports.push(i)
                }
            }
            Item::Entry(s, e) => gr.sheet_entries.push((s, e)),
            Item::Junction(i) => gr.junctions.push(i),
        }
    }
    for wi in 0..nw {
        let g = group_of(&mut uf, wi, &mut groups);
        groups[g].wires.push(wi);
    }
    let dangling_wires = groups.iter().filter(|g| g.is_empty()).map(|g| g.wires.len()).sum();
    groups.retain(|g| !g.is_empty());

    // Bus groups.
    let nb = doc.buses.len();
    let mut buf = Uf::new(nb);
    for a in 0..nb {
        for b in a + 1..nb {
            let (pa, pb) = (&doc.buses[a].points, &doc.buses[b].points);
            if pa.iter().any(|p| on_polyline(*p, pb)) || pb.iter().any(|p| on_polyline(*p, pa)) {
                buf.union(a, b);
            }
        }
    }
    let mut bus_root: HashMap<usize, usize> = HashMap::new();
    let mut bus_groups: Vec<BusGroup> = Vec::new();
    for bi in 0..nb {
        let r = buf.find(bi);
        let g = *bus_root.entry(r).or_insert_with(|| {
            bus_groups.push(BusGroup::default());
            bus_groups.len() - 1
        });
        bus_groups[g].buses.push(bi);
    }
    for (ii, (it, _)) in items.iter().enumerate() {
        let Some(bi) = bus_item[ii] else { continue };
        let g = bus_root[&buf.find(bi)];
        let bg = &mut bus_groups[g];
        match *it {
            Item::Label(i) => bg.net_labels.push(i),
            Item::Port(i) => {
                if !bg.ports.contains(&i) {
                    bg.ports.push(i)
                }
            }
            Item::Entry(s, e) => bg.sheet_entries.push((s, e)),
            _ => {}
        }
    }
    SheetGraph { groups, bus_groups, dangling_wires }
}

/// One candidate name for a net.
#[derive(Debug, Clone)]
pub struct NameCandidate {
    pub priority: u8,
    /// Sheet depth (0 = root); only meaningful at project scope.
    pub depth: usize,
    pub name: String,
    pub source: &'static str,
}

/// Name candidates contributed by a group, given the options.
pub fn candidates(doc: &SchDoc, g: &Group, opts: &NetOptions, depth: usize) -> Vec<NameCandidate> {
    let (label_p, power_p) = if opts.power_port_names_take_priority { (2, 1) } else { (1, 2) };
    let mut v = Vec::new();
    for &i in &g.net_labels {
        v.push(NameCandidate { priority: label_p, depth, name: doc.net_labels[i].name.clone(), source: "net-label" });
    }
    for &i in &g.power_ports {
        v.push(NameCandidate { priority: power_p, depth, name: doc.power_ports[i].name.clone(), source: "power-port" });
    }
    if opts.allow_port_net_names {
        for &i in &g.ports {
            v.push(NameCandidate { priority: 3, depth, name: doc.ports[i].name.clone(), source: "port" });
        }
    }
    if opts.allow_sheet_entry_net_names {
        for &(s, e) in &g.sheet_entries {
            v.push(NameCandidate {
                priority: 4,
                depth,
                name: doc.sheet_symbols[s].entries[e].name.clone(),
                source: "sheet-entry",
            });
        }
    }
    v.retain(|c| !c.name.trim().is_empty());
    v
}

/// Pick the winning candidate: lowest priority, then shallowest sheet (depth is 0 unless
/// higher-level names take priority), then alphabetical.
pub fn pick(cands: &[NameCandidate]) -> Option<&NameCandidate> {
    cands.iter().min_by(|a, b| {
        (a.priority, a.depth, natural_key(&a.name)).cmp(&(b.priority, b.depth, natural_key(&b.name)))
    })
}

/// Altium's automatic net name from the first node in natural order.
pub fn auto_name(nodes: &[(String, String)]) -> Option<String> {
    nodes
        .iter()
        .min_by(|a, b| (natural_key(&a.0), natural_key(&a.1)).cmp(&(natural_key(&b.0), natural_key(&b.1))))
        .map(|(d, p)| format!("Net{d}_{p}"))
}

/// A connection point of a sheet-level net.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SheetNode {
    pub designator: String,
    pub pin: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pin_name: String,
    /// Record index of the pin.
    pub pin_record: usize,
    /// Footprint pads the pin maps to, when a pin map renames it (PCB pad numbers).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pads: Vec<String>,
}

/// A net on one sheet.
#[derive(Debug, Clone, Serialize)]
pub struct SheetNet {
    pub name: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_named: bool,
    /// `net-label`, `power-port`, `port`, `sheet-entry` or `pin` (automatic).
    pub name_source: String,
    pub nodes: Vec<SheetNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub net_labels: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub power_ports: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
    /// `<sheet symbol>/<entry>`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sheet_entries: Vec<String>,
    /// Other names attached to the same net (Altium reports these as multiple-name errors).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub other_names: Vec<String>,
    /// Wire record indices.
    pub wires: Vec<usize>,
}

pub fn group_nodes(doc: &SchDoc, g: &Group) -> Vec<SheetNode> {
    let mut nodes: Vec<SheetNode> = g
        .pins
        .iter()
        .map(|&(c, p)| {
            let comp = &doc.components[c];
            let pin = &comp.pins[p];
            SheetNode {
                designator: comp.designator.clone(),
                pin: pin.designator.clone(),
                pin_name: pin.name.clone(),
                pin_record: pin.index,
                pads: Some(comp.pads_for(&pin.designator)).filter(|p| p.len() != 1 || p[0] != pin.designator).unwrap_or_default(),
            }
        })
        .collect();
    nodes.sort_by_key(|n| (natural_key(&n.designator), natural_key(&n.pin)));
    nodes.dedup_by(|a, b| a.designator == b.designator && a.pin == b.pin);
    nodes
}

fn uniq(mut v: Vec<String>) -> Vec<String> {
    v.sort_by_key(|s| natural_key(s));
    v.dedup();
    v
}

/// Sheet-scope name of a group: `(name, auto_named, name_source, candidates)`.
pub fn name_of(doc: &SchDoc, g: &Group, opts: &NetOptions) -> (String, bool, String, Vec<NameCandidate>) {
    let cands = candidates(doc, g, opts, 0);
    if let Some(c) = pick(&cands) {
        return (c.name.clone(), false, c.source.to_owned(), cands);
    }
    let pairs: Vec<(String, String)> =
        group_nodes(doc, g).iter().map(|n| (n.designator.clone(), n.pin.clone())).collect();
    match auto_name(&pairs) {
        Some(n) => (n, true, "pin".to_owned(), cands),
        // No pins: the group only links connectors (e.g. a port wired to a sheet entry).
        // Name it after them so it can be referred to; it never becomes a design net.
        None => {
            if let Some(&p) = g.ports.first() {
                (doc.ports[p].name.clone(), true, "port".to_owned(), cands)
            } else if let Some(&(s, e)) = g.sheet_entries.first() {
                (doc.sheet_symbols[s].entries[e].name.clone(), true, "sheet-entry".to_owned(), cands)
            } else {
                let w = g.wires.first().map(|&w| doc.wires[w].index).unwrap_or(0);
                (format!("Net_W{w}"), true, "wire".to_owned(), cands)
            }
        }
    }
}

/// Name every group of the sheet (sheet scope, no hierarchy), sorted by name.
pub fn sheet_nets(doc: &SchDoc, graph: &SheetGraph, opts: &NetOptions) -> Vec<SheetNet> {
    let mut out = Vec::new();
    for g in &graph.groups {
        let nodes = group_nodes(doc, g);
        let (name, auto, source, cands) = name_of(doc, g, opts);
        let other_names = uniq(cands.iter().map(|c| c.name.clone()).filter(|n| !n.eq_ignore_ascii_case(&name)).collect());
        out.push(SheetNet {
            name,
            auto_named: auto,
            name_source: source,
            nodes,
            net_labels: uniq(g.net_labels.iter().map(|&i| doc.net_labels[i].name.clone()).collect()),
            power_ports: uniq(g.power_ports.iter().map(|&i| doc.power_ports[i].name.clone()).collect()),
            ports: uniq(g.ports.iter().map(|&i| doc.ports[i].name.clone()).collect()),
            sheet_entries: uniq(
                g.sheet_entries
                    .iter()
                    .map(|&(s, e)| format!("{}/{}", doc.sheet_symbols[s].sheet_name, doc.sheet_symbols[s].entries[e].name))
                    .collect(),
            ),
            other_names,
            wires: g.wires.iter().map(|&w| doc.wires[w].index).collect(),
        });
    }
    out.sort_by_key(|n| natural_key(&n.name));
    out
}

/// Fill the `net` fields of the document's objects with sheet-scope net names.
pub fn annotate(doc: &mut SchDoc, opts: &NetOptions) {
    let g = graph(doc);
    let names: Vec<String> = g.groups.iter().map(|grp| name_of(doc, grp, opts).0).collect();
    for (gi, grp) in g.groups.iter().enumerate() {
        let n = Some(names[gi].clone());
        for &(c, p) in &grp.pins {
            doc.components[c].pins[p].net = n.clone();
        }
        for &i in &grp.net_labels {
            doc.net_labels[i].net = n.clone();
        }
        for &i in &grp.power_ports {
            doc.power_ports[i].net = n.clone();
        }
        for &i in &grp.ports {
            doc.ports[i].net = n.clone();
        }
        for &(s, e) in &grp.sheet_entries {
            doc.sheet_symbols[s].entries[e].net = n.clone();
        }
        for &w in &grp.wires {
            doc.wires[w].net = n.clone();
        }
        for &j in &grp.junctions {
            doc.junctions[j].net = n.clone();
        }
    }
    for bg in &g.bus_groups {
        let labels: Vec<String> = bg.net_labels.iter().map(|&i| doc.net_labels[i].name.clone()).collect();
        for &i in &bg.net_labels {
            doc.net_labels[i].on_bus = true;
        }
        for &i in &bg.ports {
            doc.ports[i].on_bus = true;
        }
        for &(s, e) in &bg.sheet_entries {
            doc.sheet_symbols[s].entries[e].on_bus = true;
        }
        for &b in &bg.buses {
            doc.buses[b].labels = labels.clone();
        }
    }
    // Point objects that are not conductors: look up the net under them.
    let wire_net = |pt: Point, doc: &SchDoc| -> Option<String> {
        doc.wires.iter().find(|w| on_polyline(pt, &w.points)).and_then(|w| w.net.clone()).or_else(|| {
            doc.components
                .iter()
                .flat_map(|c| c.pins.iter())
                .find(|p| p.hot_end == pt)
                .and_then(|p| p.net.clone())
        })
    };
    for i in 0..doc.no_ercs.len() {
        doc.no_ercs[i].net = wire_net(doc.no_ercs[i].location, doc);
    }
    for i in 0..doc.directives.len() {
        if let Some(l) = doc.directives[i].location {
            doc.directives[i].net = wire_net(l, doc);
        }
    }
    for i in 0..doc.bus_entries.len() {
        let (a, b) = (doc.bus_entries[i].location, doc.bus_entries[i].corner);
        let n = doc
            .wires
            .iter()
            .find(|w| on_polyline(a, &w.points) || on_polyline(b, &w.points))
            .and_then(|w| w.net.clone());
        doc.bus_entries[i].net = n;
    }
    let unresolved: Vec<String> = g
        .bus_groups
        .iter()
        .flat_map(|bg| bg.net_labels.iter().map(|&i| doc.net_labels[i].name.clone()))
        .filter(|n| expand_bus(n).is_none())
        .collect();
    if !unresolved.is_empty() {
        doc.warnings.push(format!("net labels on buses that are not bus names (left unresolved): {}", unresolved.join(", ")));
    }
    let hidden = doc.components.iter().flat_map(|c| c.pins.iter()).filter(|p| p.hidden).count();
    if hidden > 0 {
        doc.warnings.push(format!(
            "{hidden} hidden pin(s): Altium may tie hidden pins to a named net; native connectivity only uses their hot end"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use altium_core::units::Coord;

    fn pt(x: i64, y: i64) -> Point {
        Point::new(Coord::from_sch(x, 0), Coord::from_sch(y, 0))
    }

    #[test]
    fn segments() {
        assert!(on_segment(pt(5, 0), pt(0, 0), pt(10, 0)));
        assert!(on_segment(pt(10, 0), pt(0, 0), pt(10, 0)));
        assert!(!on_segment(pt(11, 0), pt(0, 0), pt(10, 0)));
        assert!(!on_segment(pt(5, 1), pt(0, 0), pt(10, 0)));
        assert!(on_segment(pt(5, 5), pt(0, 0), pt(10, 10)));
        assert!(on_polyline(pt(10, 5), &[pt(0, 0), pt(10, 0), pt(10, 10)]));
    }

    #[test]
    fn bus_names() {
        assert_eq!(expand_bus("D[0..3]").unwrap(), vec!["D0", "D1", "D2", "D3"]);
        assert_eq!(expand_bus("A[2..1],CLK").unwrap(), vec!["A2", "A1", "CLK"]);
        assert!(expand_bus("GND").is_none());
        assert!(expand_bus("D[x..3]").is_none());
    }

    #[test]
    fn naming() {
        let c = |p, d, n: &str, s| NameCandidate { priority: p, depth: d, name: n.into(), source: s };
        let cands = vec![c(4, 0, "ENTRY", "sheet-entry"), c(1, 1, "LBL_B", "net-label"), c(1, 1, "LBL_A", "net-label")];
        assert_eq!(pick(&cands).unwrap().name, "LBL_A");
        let nodes = vec![("R10".to_string(), "1".to_string()), ("R9".to_string(), "2".to_string())];
        assert_eq!(auto_name(&nodes).unwrap(), "NetR9_2");
        assert!(natural_key("R10") > natural_key("R9"));
    }

    #[test]
    fn union_find() {
        let mut u = Uf::new(4);
        u.union(0, 3);
        u.union(3, 2);
        assert_eq!(u.find(2), u.find(0));
        assert_ne!(u.find(1), u.find(0));
    }
}
