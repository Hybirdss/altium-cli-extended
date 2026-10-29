//! `.PcbLib` footprint libraries.
//!
//! Layout (checked on every sample): `FileHeader`; `Library/Data` = one property record (layer
//! definitions, same keys as `Board6`) followed by `u32 count` and `count × (u32 length +
//! pascal name)` — the footprint list; one storage per footprint holding `Data` (`u32 length +
//! pascal name`, then a mixed stream of binary primitives, same record layouts as the PcbDoc
//! streams), `Parameters` (`PATTERN`, `HEIGHT`, `DESCRIPTION`, ...), `WideStrings` and
//! optionally `UniqueIDPrimitiveInformation`; `Library/Models/Data` + `Library/Models/<n>`
//! (zlib STEP) for embedded 3D models.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Serialize;

use altium_core::error::{Error, ErrorCode, Result};
use altium_core::units::Coord;
use altium_format::DocKind;
use altium_format::cfbx::{Bin, Compound};
use altium_format::props::Props;

use crate::pcb::board::{self, LayerDef, Stackup};
use crate::pcb::geom::{BBox, BBoxAcc};
use crate::pcb::prim::{self, Arc, Body, Fill, Pad, Region, Text, Track, Via};
use crate::pcb::records::{self, Model};
use crate::pcb::{FileInfo, file_header, inflate, svg};

pub mod edit;

pub struct PcbLib {
    pub path: PathBuf,
    pub file: FileInfo,
    pub layers: Vec<LayerDef>,
    pub stackup: Stackup,
    pub footprints: Vec<Footprint>,
    pub models: Vec<Model>,
    pub warnings: Vec<String>,
    /// `Library/Data` properties (for `--raw`).
    pub props: Props,
    cf: Compound,
}

#[derive(Debug, Clone, Serialize)]
pub struct Footprint {
    /// 0-based position in the library's footprint list.
    pub index: usize,
    pub name: String,
    /// Compound-file storage holding it (names longer than 31 characters are truncated).
    pub storage: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<Coord>,
    /// Every `Parameters` property other than the ones above, as stored.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub parameters: IndexMap<String, String>,
    pub pads: Vec<Pad>,
    pub vias: Vec<Via>,
    pub tracks: Vec<Track>,
    pub arcs: Vec<Arc>,
    pub fills: Vec<Fill>,
    pub texts: Vec<Text>,
    pub regions: Vec<Region>,
    pub bodies: Vec<Body>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// One row of `pcblib list`.
#[derive(Debug, Clone, Serialize)]
pub struct FootprintSummary {
    pub index: usize,
    pub name: String,
    pub pads: usize,
    /// Distinct pad numbers (a footprint may repeat a number on several pads).
    pub pad_numbers: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<Coord>,
    pub primitives: IndexMap<&'static str, usize>,
    /// Extent of pads, tracks, arcs, fills, regions (copper and overlay).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
    /// Through-hole (has drilled pads), SMD, or none.
    pub mount: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

impl Footprint {
    pub fn counts(&self) -> IndexMap<&'static str, usize> {
        let mut m = IndexMap::new();
        m.insert("pads", self.pads.len());
        m.insert("vias", self.vias.len());
        m.insert("tracks", self.tracks.len());
        m.insert("arcs", self.arcs.len());
        m.insert("fills", self.fills.len());
        m.insert("texts", self.texts.len());
        m.insert("regions", self.regions.len());
        m.insert("bodies", self.bodies.len());
        m
    }

    pub fn bbox(&self) -> Option<BBox> {
        let mut acc = BBoxAcc::default();
        for p in &self.pads {
            let r = p.size.x.0.max(p.size.y.0) / 2;
            acc.add_r(p.x.0, p.y.0, r);
        }
        for t in &self.tracks {
            acc.add_r(t.start.x.0, t.start.y.0, t.width.0 / 2);
            acc.add_r(t.end.x.0, t.end.y.0, t.width.0 / 2);
        }
        for a in &self.arcs {
            acc.add_r(a.center.x.0, a.center.y.0, a.radius.0 + a.width.0 / 2);
        }
        for f in &self.fills {
            acc.add(f.corner1.x.0, f.corner1.y.0);
            acc.add(f.corner2.x.0, f.corner2.y.0);
        }
        for r in &self.regions {
            if let Some(b) = r.bbox {
                acc.add(b.min.x.0, b.min.y.0);
                acc.add(b.max.x.0, b.max.y.0);
            }
        }
        acc.get()
    }

    pub fn summary(&self, models: &[Model]) -> FootprintSummary {
        let mut nums: Vec<&str> = self.pads.iter().map(|p| p.number.as_str()).filter(|n| !n.is_empty()).collect();
        nums.sort();
        nums.dedup();
        let th = self.pads.iter().any(|p| p.hole_size.is_some());
        let mut ms: Vec<String> = self
            .bodies
            .iter()
            .filter(|b| !b.model_id.is_empty())
            .map(|b| {
                models
                    .iter()
                    .find(|m| m.id == b.model_id)
                    .map(|m| m.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| if b.model_name.is_empty() { b.model_id.clone() } else { b.model_name.clone() })
            })
            .collect();
        ms.dedup();
        FootprintSummary {
            index: self.index,
            name: self.name.clone(),
            pads: self.pads.len(),
            pad_numbers: nums.len(),
            description: self.description.clone(),
            height: self.height,
            primitives: self.counts(),
            bbox: self.bbox(),
            mount: if th {
                "through_hole"
            } else if self.pads.is_empty() {
                "none"
            } else {
                "smd"
            },
            models: ms,
        }
    }

    pub fn scene(&self) -> svg::Scene<'_> {
        let mut copper: Vec<_> = vec![crate::pcb::Layer::TOP, crate::pcb::Layer::BOTTOM];
        for p in &self.pads {
            if p.layer.is_copper() && !copper.contains(&p.layer) {
                copper.push(p.layer);
            }
        }
        svg::Scene {
            outline: &[],
            pads: self.pads.iter().collect(),
            vias: self.vias.iter().collect(),
            tracks: self.tracks.iter().collect(),
            arcs: self.arcs.iter().collect(),
            fills: self.fills.iter().collect(),
            regions: self.regions.iter().collect(),
            texts: self.texts.iter().collect(),
            copper,
        }
    }
}

/// Decode a footprint's `Data` stream: name subrecord, then mixed primitives.
pub fn parse_footprint_data(
    buf: &[u8],
    wide: &HashMap<u32, String>,
    keep_raw: bool,
    warnings: &mut Vec<String>,
) -> (String, Footprint) {
    let mut fp = Footprint {
        index: 0,
        name: String::new(),
        storage: String::new(),
        description: String::new(),
        height: None,
        parameters: IndexMap::new(),
        pads: vec![],
        vias: vec![],
        tracks: vec![],
        arcs: vec![],
        fills: vec![],
        texts: vec![],
        regions: vec![],
        bodies: vec![],
        warnings: vec![],
    };
    let mut b = Bin::new(buf);
    let name = match b.sub() {
        Ok(mut s) => s.pascal().unwrap_or_default(),
        Err(_) => {
            warnings.push("footprint Data stream is empty".into());
            return (String::new(), fp);
        }
    };
    while b.remaining() > 0 {
        let at = b.pos;
        let Ok(t) = b.u8() else { break };
        let r: Result<()> = match t {
            prim::rec::ARC => prim::parse_arc(&mut b, fp.arcs.len()).map(|v| fp.arcs.push(v)),
            prim::rec::PAD => prim::parse_pad(&mut b, fp.pads.len()).map(|v| fp.pads.push(v)),
            prim::rec::VIA => prim::parse_via(&mut b, fp.vias.len()).map(|v| fp.vias.push(v)),
            prim::rec::TRACK => prim::parse_track(&mut b, fp.tracks.len()).map(|v| fp.tracks.push(v)),
            prim::rec::TEXT => prim::parse_text(&mut b, fp.texts.len(), wide).map(|v| fp.texts.push(v)),
            prim::rec::FILL => prim::parse_fill(&mut b, fp.fills.len()).map(|v| fp.fills.push(v)),
            prim::rec::REGION => {
                prim::parse_region(&mut b, fp.regions.len(), "footprint", false, keep_raw).map(|v| fp.regions.push(v))
            }
            prim::rec::BODY => prim::parse_body(&mut b, fp.bodies.len(), keep_raw).map(|v| fp.bodies.push(v)),
            other => Err(Error::parse(format!("unknown object type {other}"))),
        };
        if let Err(e) = r {
            warnings.push(format!("{name}: primitive at byte {at}: {}; {} bytes not decoded", e.message, buf.len() - at));
            break;
        }
    }
    (name, fp)
}

pub fn load(path: &Path) -> Result<PcbLib> {
    load_with(path, false)
}

pub fn load_with(path: &Path, keep_raw: bool) -> Result<PcbLib> {
    if DocKind::of(path) != DocKind::PcbLib {
        return Err(Error::new(ErrorCode::WrongFileType, format!("{} is not a .PcbLib", path.display()))
            .hint("use `ace pcb ...` for .PcbDoc boards"));
    }
    load_compound(path, Compound::open(path)?, keep_raw)
}

/// Parse a PcbLib from bytes (`path` names it in messages); used to verify edits.
pub fn load_bytes(path: &Path, bytes: Vec<u8>, keep_raw: bool) -> Result<PcbLib> {
    load_compound(path, Compound::from_bytes(path, bytes)?, keep_raw)
}

fn load_compound(path: &Path, mut cf: Compound, keep_raw: bool) -> Result<PcbLib> {
    let mut warnings = vec![];
    let lib = cf
        .read_opt("Library/Data")
        .ok_or_else(|| Error::parse(format!("{} has no Library/Data stream", path.display())))?;
    let mut b = Bin::new(&lib);
    let props = b.props().unwrap_or_default();
    let mut names = vec![];
    if let Ok(n) = b.u32() {
        for _ in 0..n {
            match b.sub().and_then(|mut s| s.pascal()) {
                Ok(name) => names.push(name),
                Err(_) => {
                    warnings.push(format!("Library/Data: footprint list truncated after {} of {n} names", names.len()));
                    break;
                }
            }
        }
    }
    if b.remaining() > 0 {
        warnings.push(format!("Library/Data: {} trailing bytes", b.remaining()));
    }
    let (layers, stackup, _, w) = board::parse_layers(&props);
    warnings.extend(w);
    let file = cf.read_opt("FileHeader").map(|h| file_header(&h)).unwrap_or_default();

    let storages: Vec<String> = cf.storages("");
    let mut footprints = vec![];
    let mut seen = std::collections::HashSet::new();
    for (i, name) in names.iter().enumerate() {
        let storage = storages
            .iter()
            .find(|s| *s == name)
            .or_else(|| storages.iter().find(|s| s.eq_ignore_ascii_case(name)))
            .or_else(|| {
                // Storage names cannot contain '/' (stored as '_') and are cut at 31 characters.
                let short: String = name.replace('/', "_").chars().take(31).collect();
                storages.iter().find(|s| s.eq_ignore_ascii_case(&short))
            })
            .cloned();
        let Some(storage) = storage else {
            warnings.push(format!("footprint {name:?} is listed but has no storage"));
            continue;
        };
        seen.insert(storage.clone());
        let Some(data) = cf.read_opt(&format!("{storage}/Data")) else {
            warnings.push(format!("footprint {name:?}: {storage}/Data missing"));
            continue;
        };
        let mut fw = vec![];
        let wide = cf.read_opt(&format!("{storage}/WideStrings")).and_then(|w| Bin::new(&w).props().ok()).map(|p| altium_format::cfbx::encoded_text_table(&p)).unwrap_or_default();
        let (inner, mut fp) = parse_footprint_data(&data, &wide, keep_raw, &mut fw);
        fp.index = i;
        fp.name = name.clone();
        fp.storage = storage.clone();
        if !inner.is_empty() && inner != *name {
            fw.push(format!("Data names the footprint {inner:?}"));
        }
        if let Some(pb) = cf.read_opt(&format!("{storage}/Parameters"))
            && let Ok(p) = Bin::new(&pb).props() {
                fp.description = p.str("DESCRIPTION");
                fp.height = p.len("HEIGHT");
                for (k, v) in p.entries() {
                    if !["PATTERN", "DESCRIPTION", "HEIGHT"].iter().any(|x| x.eq_ignore_ascii_case(k)) && !v.is_empty() {
                        fp.parameters.insert(k.to_owned(), v.to_owned());
                    }
                }
            }
        if let Some(u) = cf.read_opt(&format!("{storage}/UniqueIDPrimitiveInformation/Data")) {
            let ids = records::unique_ids(&records::prop_records(&u, 0).0);
            for p in &mut fp.pads {
                p.owner.unique_id = ids.get(&("pad".to_owned(), p.index)).cloned();
            }
        }
        warnings.extend(fw.iter().map(|w| format!("{name}: {w}")));
        fp.warnings = fw;
        footprints.push(fp);
    }
    for s in &storages {
        if s != "Library" && s != "FileVersionInfo" && !seen.contains(s) && cf.exists(&format!("{s}/Data")) {
            warnings.push(format!("storage {s:?} looks like a footprint but is not in the Library/Data list"));
        }
    }

    let mut models: Vec<Model> = cf
        .read_opt("Library/Models/Data")
        .map(|b| records::prop_records(&b, 0).0)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, (_, p))| records::model(i, p, keep_raw))
        .collect();
    let sizes: HashMap<String, u64> = cf.streams().into_iter().map(|s| (s.path, s.size)).collect();
    for m in &mut models {
        let s = format!("Library/Models/{}", m.index);
        if let Some(sz) = sizes.get(&s) {
            m.stream = Some(s);
            m.compressed_size = Some(*sz);
        }
        m.used_by = footprints.iter().flat_map(|f| &f.bodies).filter(|b| b.model_id == m.id).count();
    }
    Ok(PcbLib { path: path.to_path_buf(), file, layers, stackup, footprints, models, warnings, props, cf })
}

impl PcbLib {
    /// Find a footprint by exact name, then case-insensitively, then by index (`#N`).
    pub fn footprint(&self, name: &str) -> Result<&Footprint> {
        if let Some(f) = self.footprints.iter().find(|f| f.name == name) {
            return Ok(f);
        }
        if let Some(f) = self.footprints.iter().find(|f| f.name.eq_ignore_ascii_case(name)) {
            return Ok(f);
        }
        if let Some(i) = name.strip_prefix('#').and_then(|s| s.parse::<usize>().ok())
            && let Some(f) = self.footprints.get(i) {
                return Ok(f);
            }
        let names: Vec<&str> = self.footprints.iter().map(|f| f.name.as_str()).take(40).collect();
        Err(Error::not_found(format!("no footprint {name:?}")).hint(format!("footprints: {names:?}")))
    }

    /// Decompressed STEP data of model `index`.
    pub fn model_data(&mut self, index: usize) -> Result<Vec<u8>> {
        let m = self.models.get(index).ok_or_else(|| Error::not_found(format!("no model {index}")))?;
        let s = m
            .stream
            .clone()
            .ok_or_else(|| Error::not_found(format!("model {index} ({}) has no embedded data", m.name)))?;
        inflate(&self.cf.read(&s)?)
    }
}
