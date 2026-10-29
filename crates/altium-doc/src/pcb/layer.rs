//! PCB layer identities.
//!
//! Altium stores layers three ways: a one-byte "V6" id in binary primitives (1 = top copper ...
//! 82 = via holes), a 32-bit "V7/V8" id in newer records and in the Board6 stack keys
//! (`0x0100_0001` = top copper, `0x0102_0011` = Mechanical 17, ...), and names in text records
//! (`TOP`, `MID3`, `MECHANICAL17`, `MULTILAYER`). [`Layer`] normalises all three to the 32-bit
//! space and serialises as Altium's display/scripting name (`TopLayer`, `MidLayer3`,
//! `Mechanical17`, `MultiLayer`).

use serde::{Serialize, Serializer};

const COPPER: u32 = 0x0100_0000;
const PLANE: u32 = 0x0101_0000;
const MECH: u32 = 0x0102_0000;
const OTHER: u32 = 0x0103_0000;
const DIEL: u32 = 0x0104_0000;

/// A layer in Altium's 32-bit (V7/V8) id space. `Layer(0)` is "no layer".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Layer(pub u32);

/// What a layer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    /// Signal copper (top, mid, bottom).
    Copper,
    /// Internal plane (negative copper).
    Plane,
    Mechanical,
    Overlay,
    /// Solder mask.
    Solder,
    Paste,
    /// Drill guide / drill drawing.
    Drill,
    Keepout,
    /// Multi-layer (through-hole pads, vias).
    Multi,
    Dielectric,
    /// Display-only layers (connections, grids, DRC markers, selections, holes).
    System,
    Unknown,
}

/// (offset in the "other" range, legacy V6 id, display name, kind)
const OTHERS: &[(u32, u8, &str, LayerKind)] = &[
    (6, 33, "TopOverlay", LayerKind::Overlay),
    (7, 34, "BottomOverlay", LayerKind::Overlay),
    (8, 35, "TopPaste", LayerKind::Paste),
    (9, 36, "BottomPaste", LayerKind::Paste),
    (10, 37, "TopSolder", LayerKind::Solder),
    (11, 38, "BottomSolder", LayerKind::Solder),
    (12, 55, "DrillGuide", LayerKind::Drill),
    (13, 56, "KeepOutLayer", LayerKind::Keepout),
    (14, 73, "DrillDrawing", LayerKind::Drill),
    (15, 74, "MultiLayer", LayerKind::Multi),
    (16, 75, "Connections", LayerKind::System),
    (17, 76, "Background", LayerKind::System),
    (18, 77, "DRCErrorMarkers", LayerKind::System),
    (19, 78, "Selections", LayerKind::System),
    (20, 79, "VisibleGrid1", LayerKind::System),
    (21, 80, "VisibleGrid2", LayerKind::System),
    (22, 81, "PadHoles", LayerKind::System),
    (23, 82, "ViaHoles", LayerKind::System),
    (24, 0, "TopPadMaster", LayerKind::System),
    (25, 0, "BottomPadMaster", LayerKind::System),
    (26, 0, "DRCDetailMarkers", LayerKind::System),
];

impl Layer {
    pub const NONE: Layer = Layer(0);
    pub const TOP: Layer = Layer(COPPER + 1);
    pub const BOTTOM: Layer = Layer(COPPER + 0xFFFF);
    pub const TOP_OVERLAY: Layer = Layer(OTHER + 6);
    pub const BOTTOM_OVERLAY: Layer = Layer(OTHER + 7);
    pub const TOP_PASTE: Layer = Layer(OTHER + 8);
    pub const BOTTOM_PASTE: Layer = Layer(OTHER + 9);
    pub const TOP_SOLDER: Layer = Layer(OTHER + 10);
    pub const BOTTOM_SOLDER: Layer = Layer(OTHER + 11);
    pub const KEEPOUT: Layer = Layer(OTHER + 13);
    pub const MULTI: Layer = Layer(OTHER + 15);

    pub fn mid(n: u32) -> Layer {
        Layer(COPPER + 1 + n)
    }
    pub fn plane(n: u32) -> Layer {
        Layer(PLANE + n)
    }
    pub fn mechanical(n: u32) -> Layer {
        Layer(MECH + n)
    }
    pub fn dielectric(n: u32) -> Layer {
        Layer(DIEL + n)
    }

    /// From the one-byte id used by binary primitives (`0` = no layer).
    pub fn from_v6(id: u8) -> Layer {
        match id {
            0 => Layer::NONE,
            1 => Layer::TOP,
            2..=31 => Layer::mid(id as u32 - 1),
            32 => Layer::BOTTOM,
            39..=54 => Layer::plane(id as u32 - 38),
            57..=72 => Layer::mechanical(id as u32 - 56),
            _ => OTHERS
                .iter()
                .find(|o| o.1 == id)
                .map(|o| Layer(OTHER + o.0))
                .unwrap_or(Layer(id as u32)),
        }
    }

    /// From a 32-bit id. Small values (< 256) are legacy ids as stored by older `LAYERID` keys.
    pub fn from_id(id: u32) -> Layer {
        if id < 256 { Layer::from_v6(id as u8) } else { Layer(id) }
    }

    /// Pick the layer of a binary record that carries both a V6 byte and a V7 word: the V7 id
    /// is authoritative for mechanical layers beyond 16 (which have no V6 id), the V6 byte
    /// otherwise.
    pub fn versioned(v6: u8, v7: Option<u32>) -> Layer {
        match v7 {
            Some(v) if v > MECH + 16 && v <= MECH + 0xFFFF => Layer(v),
            Some(v) if v6 == 0 && v != 0 => Layer::from_id(v),
            _ => Layer::from_v6(v6),
        }
    }

    /// Parse a layer name as written in text records (`TOP`, `MID1`, `BOTTOM`, `TOPOVERLAY`,
    /// `PLANE2`, `MECHANICAL17`, `KEEPOUT`, `MULTILAYER`...) or a display name (`TopLayer`,
    /// `MidLayer1`, `Mechanical1`, `KeepOutLayer`), case-insensitively.
    pub fn from_name(name: &str) -> Option<Layer> {
        let n: String = name.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '_').collect();
        let n = n.to_ascii_uppercase();
        if n.is_empty() {
            return None;
        }
        let num = |p: &str| n.strip_prefix(p).and_then(|r| r.parse::<u32>().ok());
        let l = match n.as_str() {
            "TOP" | "TOPLAYER" => Layer::TOP,
            "BOTTOM" | "BOTTOMLAYER" => Layer::BOTTOM,
            "KEEPOUT" | "KEEPOUTLAYER" => Layer::KEEPOUT,
            "MULTILAYER" | "MULTI" => Layer::MULTI,
            "NOLAYER" => Layer::NONE,
            _ => {
                if let Some(k) = num("MIDLAYER").or_else(|| num("MID")) {
                    if (1..=30).contains(&k) { Layer::mid(k) } else { return None }
                } else if let Some(k) = num("INTERNALPLANE").or_else(|| num("PLANE")) {
                    Layer::plane(k)
                } else if let Some(k) = num("MECHANICAL").or_else(|| num("MECH")) {
                    Layer::mechanical(k)
                } else if let Some(k) = num("DIELECTRIC") {
                    Layer::dielectric(k)
                } else {
                    let o = OTHERS.iter().find(|o| o.2.eq_ignore_ascii_case(&n))?;
                    Layer(OTHER + o.0)
                }
            }
        };
        Some(l)
    }

    /// Altium display/scripting name: `TopLayer`, `MidLayer3`, `BottomLayer`, `InternalPlane1`,
    /// `Mechanical13`, `TopOverlay`, `MultiLayer`, `Dielectric1`...
    pub fn name(self) -> String {
        let v = self.0;
        if v == 0 {
            return "NoLayer".into();
        }
        if v < 256 {
            return format!("Layer{v}");
        }
        match v & 0xFFFF_0000 {
            COPPER => match v & 0xFFFF {
                1 => "TopLayer".into(),
                0xFFFF => "BottomLayer".into(),
                k => format!("MidLayer{}", k - 1),
            },
            PLANE => format!("InternalPlane{}", v & 0xFFFF),
            MECH => format!("Mechanical{}", v & 0xFFFF),
            DIEL => format!("Dielectric{}", v & 0xFFFF),
            OTHER => OTHERS
                .iter()
                .find(|o| o.0 == v & 0xFFFF)
                .map(|o| o.2.to_owned())
                .unwrap_or_else(|| format!("Layer0x{v:08X}")),
            _ => format!("Layer0x{v:08X}"),
        }
    }

    pub fn kind(self) -> LayerKind {
        let v = self.0;
        match v & 0xFFFF_0000 {
            COPPER if v & 0xFFFF != 0 => LayerKind::Copper,
            PLANE => LayerKind::Plane,
            MECH => LayerKind::Mechanical,
            DIEL => LayerKind::Dielectric,
            OTHER => OTHERS.iter().find(|o| o.0 == v & 0xFFFF).map(|o| o.3).unwrap_or(LayerKind::Unknown),
            _ => LayerKind::Unknown,
        }
    }

    pub fn is_copper(self) -> bool {
        matches!(self.kind(), LayerKind::Copper | LayerKind::Plane)
    }

    /// The legacy one-byte id, if this layer has one.
    pub fn v6(self) -> Option<u8> {
        let v = self.0;
        match v & 0xFFFF_0000 {
            COPPER => match v & 0xFFFF {
                1 => Some(1),
                0xFFFF => Some(32),
                k @ 2..=31 => Some(k as u8),
                _ => None,
            },
            PLANE if (1..=16).contains(&(v & 0xFFFF)) => Some((v & 0xFFFF) as u8 + 38),
            MECH if (1..=16).contains(&(v & 0xFFFF)) => Some((v & 0xFFFF) as u8 + 56),
            OTHER => OTHERS.iter().find(|o| o.0 == v & 0xFFFF && o.1 != 0).map(|o| o.1),
            _ => None,
        }
    }

    /// Sort key following the physical order top → bottom for copper; other layers after.
    pub fn order(self) -> (u8, u32) {
        let v = self.0;
        match v & 0xFFFF_0000 {
            COPPER => (0, if v & 0xFFFF == 0xFFFF { 0xFFFF } else { v & 0xFFFF }),
            PLANE => (1, v & 0xFFFF),
            OTHER => (2, v & 0xFFFF),
            MECH => (3, v & 0xFFFF),
            _ => (4, v),
        }
    }

    /// Which board side a layer belongs to, when it has one.
    pub fn side(self) -> Option<&'static str> {
        match self {
            Layer::TOP | Layer::TOP_OVERLAY | Layer::TOP_PASTE | Layer::TOP_SOLDER => Some("top"),
            Layer::BOTTOM | Layer::BOTTOM_OVERLAY | Layer::BOTTOM_PASTE | Layer::BOTTOM_SOLDER => Some("bottom"),
            _ => None,
        }
    }
}

impl std::fmt::Display for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

impl Serialize for Layer {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v6_v7_names() {
        assert_eq!(Layer::from_v6(1).name(), "TopLayer");
        assert_eq!(Layer::from_v6(32).0, 16842751); // as in LAYER_V8_*LAYERID of "Bottom Layer"
        assert_eq!(Layer::from_v6(33).0, 16973830); // Top Overlay
        assert_eq!(Layer::from_v6(35).0, 16973832); // Top Paste
        assert_eq!(Layer::from_v6(57).0, 16908289); // Mechanical 1
        assert_eq!(Layer::from_v6(74).0, 16973839); // Multi-Layer
        assert_eq!(Layer::from_v6(39).0, 16842753); // Internal Plane 1
        assert_eq!(Layer::from_v6(3).name(), "MidLayer2");
        assert_eq!(Layer(16908305).name(), "Mechanical17");
        assert_eq!(Layer(17039361).name(), "Dielectric1");
        for id in 1..=82u8 {
            assert_eq!(Layer::from_v6(id).v6(), Some(id), "{id}");
        }
    }

    #[test]
    fn names() {
        assert_eq!(Layer::from_name("TOP"), Some(Layer::TOP));
        assert_eq!(Layer::from_name("MID12"), Some(Layer::mid(12)));
        assert_eq!(Layer::from_name("MECHANICAL17").unwrap().name(), "Mechanical17");
        assert_eq!(Layer::from_name("TOPOVERLAY"), Some(Layer::TOP_OVERLAY));
        assert_eq!(Layer::from_name("MultiLayer"), Some(Layer::MULTI));
        assert_eq!(Layer::from_name("KeepOutLayer"), Some(Layer::KEEPOUT));
        assert_eq!(Layer::from_name("Top Layer"), Some(Layer::TOP));
        assert_eq!(Layer::from_name("DRILLDRAWING").unwrap().name(), "DrillDrawing");
        assert_eq!(Layer::from_name("PLANE3").unwrap().name(), "InternalPlane3");
        assert_eq!(Layer::from_name("bogus"), None);
    }

    #[test]
    fn versioned() {
        assert_eq!(Layer::versioned(1, Some(16777217)), Layer::TOP);
        assert_eq!(Layer::versioned(0, Some(16908305)).name(), "Mechanical17");
        assert_eq!(Layer::versioned(57, None).name(), "Mechanical1");
    }
}
