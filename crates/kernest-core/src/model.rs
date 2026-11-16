//! In-memory font model.
//!
//! Units are font units (y up, baseline at 0), stored as `f64` so intermediate
//! edits and interpolation keep full precision; rounding happens at export.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::glyphdata::{self, Category};

/// A whole font family source: info, axes, masters, glyphs, kerning and features.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Font {
    pub info: FontInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub axes: Vec<Axis>,
    pub masters: Vec<Master>,
    /// Named locations in the design space that are exported as fonts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<Instance>,
    /// Glyphs in glyph order.
    pub glyphs: Vec<Glyph>,
    /// Kerning per master id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kerning: BTreeMap<String, Vec<KernPair>>,
    /// OpenType feature code (AFDKO syntax).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub features: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontInfo {
    pub family_name: String,
    pub units_per_em: u16,
    pub version_major: u16,
    pub version_minor: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Axis {
    pub name: String,
    pub tag: String,
    pub min: f64,
    pub default: f64,
    pub max: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Master {
    /// Stable identifier referenced by layers and kerning. Never shown to users.
    pub id: String,
    pub name: String,
    /// Design-space location keyed by axis tag.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub location: BTreeMap<String, f64>,
    pub metrics: Metrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub ascender: f64,
    pub cap_height: f64,
    pub x_height: f64,
    pub descender: f64,
    #[serde(default)]
    pub italic_angle: f64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            ascender: 800.0,
            cap_height: 700.0,
            x_height: 500.0,
            descender: -200.0,
            italic_angle: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Glyph {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unicodes: Vec<Codepoint>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub export: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_group: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// One layer per master, plus any brace (intermediate) and bracket (alternate) layers.
    pub layers: Vec<Layer>,
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}

impl Glyph {
    pub fn new(name: impl Into<String>, unicodes: Vec<Codepoint>) -> Self {
        Self {
            name: name.into(),
            unicodes,
            export: true,
            left_group: None,
            right_group: None,
            tags: Vec::new(),
            note: None,
            layers: Vec::new(),
        }
    }

    /// A layer by key: a master id gives that master's own layer; the id of a brace or
    /// bracket layer (such as `m01~2`) gives that layer.
    pub fn layer(&self, key: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.matches(key))
    }

    pub fn layer_mut(&mut self, key: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.matches(key))
    }

    /// Brace (intermediate) layers.
    pub fn brace_layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers.iter().filter(|l| l.location.is_some())
    }

    /// Bracket (alternate) layers.
    pub fn bracket_layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers.iter().filter(|l| !l.bracket.is_empty())
    }

    /// The distinct bracket ranges this glyph has alternates for, in first-seen order.
    pub fn bracket_groups(&self) -> Vec<Vec<AxisRange>> {
        let mut groups: Vec<Vec<AxisRange>> = Vec::new();
        for l in self.bracket_layers() {
            if !groups.contains(&l.bracket) {
                groups.push(l.bracket.clone());
            }
        }
        groups
    }

    /// A fresh id for a new brace or bracket layer belonging to `master_id`.
    pub fn new_layer_id(&self, master_id: &str) -> String {
        (1..)
            .map(|n| format!("{master_id}~{n}"))
            .find(|id| !self.layers.iter().any(|l| l.id.as_deref() == Some(id)))
            .expect("an unused id")
    }

    /// Category derived from the first code point (or the name suffix for unencoded glyphs).
    pub fn category(&self) -> Category {
        match self.unicodes.first() {
            Some(cp) => glyphdata::category(cp.0),
            None => glyphdata::category_for_name(&self.name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    /// The master this layer belongs to. For brace and bracket layers it is the master
    /// they were made from (and, for bracket layers, the master they replace).
    pub master_id: String,
    /// Set for brace and bracket layers only, e.g. `m01~1`; master layers are found by
    /// their master id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Brace (intermediate) layer: its own design-space location (by axis tag), which
    /// adds a source to the glyph's interpolation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<BTreeMap<String, f64>>,
    /// Bracket (alternate) layer: replaces `master_id`'s layer wherever the location is
    /// inside every one of these ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bracket: Vec<AxisRange>,
    pub width: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<Shape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<Anchor>,
    #[serde(default, skip_serializing_if = "MetricsKeys::is_empty")]
    pub metrics_keys: MetricsKeys,
}

impl Layer {
    pub fn new(master_id: impl Into<String>, width: f64) -> Self {
        Self {
            master_id: master_id.into(),
            id: None,
            location: None,
            bracket: Vec::new(),
            width,
            shapes: Vec::new(),
            anchors: Vec::new(),
            metrics_keys: MetricsKeys::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// A master's own layer (not a brace or bracket layer).
    pub fn is_master(&self) -> bool {
        self.id.is_none()
    }

    /// The key that finds this layer with [`Glyph::layer`].
    pub fn key(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.master_id)
    }

    fn matches(&self, key: &str) -> bool {
        match &self.id {
            Some(id) => id == key,
            None => self.master_id == key,
        }
    }

    /// A short label: `{wght 550}` for a brace layer, `[wght 600–]` for a bracket layer.
    pub fn label(&self) -> Option<String> {
        if let Some(loc) = &self.location {
            let parts: Vec<String> = loc
                .iter()
                .map(|(t, v)| format!("{t} {}", fmt_num(*v)))
                .collect();
            return Some(format!("{{{}}}", parts.join(", ")));
        }
        if !self.bracket.is_empty() {
            let parts: Vec<String> = self.bracket.iter().map(AxisRange::to_string).collect();
            return Some(format!("[{}]", parts.join(", ")));
        }
        None
    }
}

/// An axis range for bracket layers, in design units: `min <= value <= max`, where a
/// missing bound is open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxisRange {
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

impl AxisRange {
    pub fn contains(&self, v: f64) -> bool {
        self.min.is_none_or(|m| v >= m) && self.max.is_none_or(|m| v <= m)
    }
}

impl fmt::Display for AxisRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.min, self.max) {
            (Some(a), Some(b)) => write!(f, "{} {}–{}", self.tag, fmt_num(a), fmt_num(b)),
            (Some(a), None) => write!(f, "{} {}–", self.tag, fmt_num(a)),
            (None, Some(b)) => write!(f, "{} –{}", self.tag, fmt_num(b)),
            (None, None) => write!(f, "{} any", self.tag),
        }
    }
}

/// Whether `loc` (design units by tag; missing axes at their default) is inside every range.
pub fn in_bracket(font: &Font, ranges: &[AxisRange], loc: &BTreeMap<String, f64>) -> bool {
    ranges.iter().all(|r| {
        let v = loc
            .get(&r.tag)
            .copied()
            .or_else(|| font.axes.iter().find(|a| a.tag == r.tag).map(|a| a.default))
            .unwrap_or(0.0);
        r.contains(v)
    })
}

/// Metrics keys such as `=n` or `=|H` that link sidebearings to other glyphs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsKeys {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<String>,
}

impl MetricsKeys {
    pub fn is_empty(&self) -> bool {
        self.left.is_none() && self.right.is_none() && self.width.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Shape {
    Path(Path),
    #[serde(rename_all = "camelCase")]
    Component {
        base: String,
        /// Affine transform `[xx, xy, yx, yy, dx, dy]`.
        #[serde(default = "identity", skip_serializing_if = "is_identity")]
        transform: [f64; 6],
    },
}

pub const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
fn identity() -> [f64; 6] {
    IDENTITY
}
fn is_identity(t: &[f64; 6]) -> bool {
    *t == IDENTITY
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Path {
    #[serde(default = "yes")]
    pub closed: bool,
    pub nodes: Vec<Node>,
}

/// A point on an outline. Serialised compactly as `"x y kind [smooth]"`,
/// e.g. `"120 0 line"` or `"300 700 curve smooth"`, to keep files readable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub x: f64,
    pub y: f64,
    pub kind: NodeKind,
    pub smooth: bool,
}

impl Node {
    pub fn new(x: f64, y: f64, kind: NodeKind) -> Self {
        Self {
            x,
            y,
            kind,
            smooth: false,
        }
    }
    pub fn on_curve(&self) -> bool {
        self.kind != NodeKind::OffCurve
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Line,
    Curve,
    QCurve,
    OffCurve,
}

impl NodeKind {
    fn as_str(self) -> &'static str {
        match self {
            NodeKind::Line => "line",
            NodeKind::Curve => "curve",
            NodeKind::QCurve => "qcurve",
            NodeKind::OffCurve => "offcurve",
        }
    }
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        // Shortest representation that parses back to exactly the same value.
        format!("{v}")
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            fmt_num(self.x),
            fmt_num(self.y),
            self.kind.as_str()
        )?;
        if self.smooth {
            f.write_str(" smooth")?;
        }
        Ok(())
    }
}

impl FromStr for Node {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut it = s.split_whitespace();
        let mut num = |what: &str| -> Result<f64, String> {
            it.next()
                .ok_or_else(|| format!("node \"{s}\" is missing {what}"))?
                .parse::<f64>()
                .map_err(|_| format!("node \"{s}\" has an invalid {what}"))
        };
        let x = num("x")?;
        let y = num("y")?;
        let mut rest = s.split_whitespace().skip(2);
        let kind = match rest.next() {
            Some("line") => NodeKind::Line,
            Some("curve") => NodeKind::Curve,
            Some("qcurve") => NodeKind::QCurve,
            Some("offcurve") => NodeKind::OffCurve,
            other => return Err(format!("node \"{s}\" has unknown kind {other:?}")),
        };
        let smooth = match rest.next() {
            None => false,
            Some("smooth") => true,
            Some(other) => return Err(format!("node \"{s}\" has unknown flag \"{other}\"")),
        };
        Ok(Node { x, y, kind, smooth })
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernPair {
    /// Glyph name, or `@group` for a kerning group.
    pub left: String,
    pub right: String,
    pub value: f64,
}

/// A Unicode code point, serialised as an uppercase hex string such as `"0041"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Codepoint(pub u32);

impl Codepoint {
    pub fn char(self) -> Option<char> {
        char::from_u32(self.0)
    }
}

impl fmt::Display for Codepoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04X}", self.0)
    }
}

impl Serialize for Codepoint {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Codepoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        u32::from_str_radix(s.trim_start_matches("U+"), 16)
            .ok()
            .filter(|v| char::from_u32(*v).is_some())
            .map(Codepoint)
            .ok_or_else(|| {
                serde::de::Error::custom(format!("\"{s}\" is not a valid Unicode code point"))
            })
    }
}

impl Font {
    /// The layer a static font at `master_id`'s location uses: the master's bracket
    /// layer if that location is inside its ranges, otherwise the master layer.
    pub fn static_layer(&self, glyph: &str, master_id: &str) -> Option<&Layer> {
        let g = self.glyph(glyph)?;
        let loc = &self.master(master_id)?.location;
        g.bracket_layers()
            .find(|l| l.master_id == master_id && in_bracket(self, &l.bracket, loc))
            .or_else(|| g.layer(master_id))
    }

    /// A new, empty single-master font with the given glyph set.
    pub fn new(family_name: impl Into<String>, codepoints: impl IntoIterator<Item = u32>) -> Self {
        let master = Master {
            id: "m01".into(),
            name: "Regular".into(),
            location: BTreeMap::new(),
            metrics: Metrics::default(),
        };
        let glyphs = codepoints
            .into_iter()
            .map(|cp| {
                let mut g = Glyph::new(glyphdata::name_for(cp), vec![Codepoint(cp)]);
                let width = if cp == 0x20 { 250.0 } else { 600.0 };
                g.layers.push(Layer::new(&master.id, width));
                g
            })
            .collect();
        Font {
            info: FontInfo {
                family_name: family_name.into(),
                units_per_em: 1000,
                version_major: 1,
                version_minor: 0,
                designer: None,
                copyright: None,
                vendor_id: None,
            },
            axes: Vec::new(),
            masters: vec![master],
            glyphs,
            instances: Vec::new(),
            kerning: BTreeMap::new(),
            features: String::new(),
        }
    }

    pub fn glyph(&self, name: &str) -> Option<&Glyph> {
        self.glyphs.iter().find(|g| g.name == name)
    }

    pub fn glyph_mut(&mut self, name: &str) -> Option<&mut Glyph> {
        self.glyphs.iter_mut().find(|g| g.name == name)
    }

    pub fn master(&self, id: &str) -> Option<&Master> {
        self.masters.iter().find(|m| m.id == id)
    }

    /// Checks structural invariants: unique names, layers reference real masters,
    /// every glyph has a layer for every master, components point at existing glyphs.
    pub fn validate(&self) -> crate::Result<()> {
        use std::collections::HashSet;
        let bad = |m: String| Err(crate::Error::Invalid(m));
        if self.masters.is_empty() {
            return bad("a font needs at least one master".into());
        }
        let master_ids: HashSet<&str> = self.masters.iter().map(|m| m.id.as_str()).collect();
        if master_ids.len() != self.masters.len() {
            return bad("master ids must be unique".into());
        }
        let mut names = HashSet::new();
        for g in &self.glyphs {
            if g.name.is_empty() {
                return bad("glyph names cannot be empty".into());
            }
            if !names.insert(g.name.as_str()) {
                return bad(format!("glyph name \"{}\" is used twice", g.name));
            }
        }
        let tags: HashSet<&str> = self.axes.iter().map(|a| a.tag.as_str()).collect();
        for g in &self.glyphs {
            let mut layer_ids = HashSet::new();
            for l in &g.layers {
                if let Some(id) = &l.id {
                    if !id.contains('~') || !layer_ids.insert(id.as_str()) {
                        return bad(format!(
                            "glyph \"{}\" has a bad or repeated layer id \"{id}\"",
                            g.name
                        ));
                    }
                    if l.location.is_none() == l.bracket.is_empty() {
                        return bad(format!(
                            "layer \"{id}\" of \"{}\" must be either a brace or a bracket layer",
                            g.name
                        ));
                    }
                    let used: Vec<&str> = match &l.location {
                        Some(loc) => loc.keys().map(String::as_str).collect(),
                        None => l.bracket.iter().map(|r| r.tag.as_str()).collect(),
                    };
                    if let Some(t) = used.iter().find(|t| !tags.contains(**t)) {
                        return bad(format!(
                            "layer \"{id}\" of \"{}\" uses unknown axis \"{t}\"",
                            g.name
                        ));
                    }
                } else if l.location.is_some() || !l.bracket.is_empty() {
                    return bad(format!(
                        "a brace or bracket layer of \"{}\" has no id",
                        g.name
                    ));
                }
                if !master_ids.contains(l.master_id.as_str()) {
                    return bad(format!(
                        "glyph \"{}\" has a layer for unknown master \"{}\"",
                        g.name, l.master_id
                    ));
                }
                for s in &l.shapes {
                    if let Shape::Component { base, .. } = s {
                        if !names.contains(base.as_str()) {
                            return bad(format!(
                                "glyph \"{}\" uses missing component \"{base}\"",
                                g.name
                            ));
                        }
                    }
                }
            }
            for m in &self.masters {
                if g.layer(&m.id).is_none() {
                    return bad(format!(
                        "glyph \"{}\" has no layer for master \"{}\"",
                        g.name, m.name
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A named location in the design space, exported as its own font (e.g. "SemiBold").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub name: String,
    /// Design-space location keyed by axis tag.
    #[serde(default)]
    pub location: BTreeMap<String, f64>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub export: bool,
}
