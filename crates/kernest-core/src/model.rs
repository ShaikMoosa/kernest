//! In-memory font model.
//!
//! Units are font units (y up, baseline at 0), stored as `f64` so intermediate
//! edits and interpolation keep full precision; rounding happens at export.

use std::collections::{BTreeMap, BTreeSet};
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
    /// Free-form notes for the designer. Saved in `.kernest` but never exported
    /// into font files, so private remarks cannot leak into a shipped font.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Which generated features are switched on. Generated code is kept out of
    /// `features` so a hand-written feature always wins over the generated one.
    #[serde(default, skip_serializing_if = "FeatureSettings::is_empty")]
    pub feature_settings: FeatureSettings,
    /// Colour palettes (CPAL): every palette has the same number of colours, and
    /// colour layers refer to them by index. Palette 0 is the default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palettes: Vec<Vec<crate::color::Color>>,
    /// Per-master palette overrides, keyed by master id (like `kerning`). A master
    /// listed here uses these palettes instead of `palettes`; in a variable font the
    /// differing colours cross-fade between masters.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub master_palettes: BTreeMap<String, Vec<Vec<crate::color::Color>>>,
    /// Saved smart and list filters for the Font View sidebar.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<crate::smartfilter::Filter>,
    /// Several variable fonts cut from this one family. Empty means a single
    /// variable font covering every axis at its full range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variable_fonts: Vec<VariableFontSetting>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export_presets: Vec<crate::presets::ExportPreset>,
}

/// Toggles for the features Kernest generates from glyph names. Only the tags
/// listed here are generated; anything the designer writes by hand is left alone.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureSettings {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub automatic: BTreeSet<String>,
}

impl FeatureSettings {
    pub fn is_empty(&self) -> bool {
        self.automatic.is_empty()
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designer_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trademark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_text: Option<String>,
    /// TrueType autohinting settings; `None` means the defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hinting: Option<Hinting>,
    /// PostScript hinting values written into the CFF Private DICT; `None` means
    /// the standard defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ps_hinting: Option<PsHinting>,
    /// TTFAutohint control instructions: per-glyph point deltas applied on top of
    /// the autohinter. Empty means none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub control_instructions: String,
    /// Snap grid for the edit view; `None` (or spacing 0) means no grid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<Grid>,
    /// Sets OS/2 fsSelection bit 7 so applications use the typo metrics for line
    /// spacing ("Use Typo Metrics" in Glyphs).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub use_typo_metrics: bool,
    /// Source settings Kernest doesn't use itself (Glyphs custom parameters), kept so
    /// they survive a trip back to `.glyphs`. Not applied on export.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_parameters: Vec<CustomParameter>,
    /// Virtual masters (S39): named design-space locations that only glyphs with
    /// intermediate layers there use; every other glyph stays as it is along them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub virtual_masters: Vec<VirtualMaster>,
    /// Number values (S64) for feature code tokens such as `$padding`: by name, then
    /// by master id. A master without a value uses 0.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub numbers: BTreeMap<String, BTreeMap<String, f64>>,
}

/// A virtual master: a name and a location on the font's axes (design units).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualMaster {
    pub name: String,
    pub location: BTreeMap<String, f64>,
}

/// A custom parameter: its name and its value as Glyphs writes it (plist text, such
/// as `1`, `"text"` or `(a, b)`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomParameter {
    pub name: String,
    pub value: String,
    /// Switched off in Glyphs (kept, but not applied there either).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

/// A snap grid in font units: nodes snap to `spacing / subdivision` steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grid {
    pub spacing: u16,
    pub subdivision: u16,
}

impl Grid {
    /// The snap step, or `None` when the grid is off.
    pub fn step(&self) -> Option<f64> {
        if self.spacing == 0 || self.subdivision == 0 {
            None
        } else {
            Some(self.spacing as f64 / self.subdivision as f64)
        }
    }
}

/// Settings for TrueType autohinting on export: the ttfautohint options that matter
/// most. Kernest hints with tilvisan, a Rust port of ttfautohint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hinting {
    /// Smallest text size, in pixels per em, that gets hinting instructions.
    pub range_min: u16,
    /// Largest text size, in pixels per em, that gets hinting instructions.
    pub range_max: u16,
    /// Up to this size (pixels per em) the x-height is rounded up for legibility;
    /// 0 turns this off.
    pub increase_x_height: u16,
    /// Hint composite glyphs as a whole rather than through their components.
    #[serde(default)]
    pub hint_composites: bool,
}

impl Default for Hinting {
    fn default() -> Self {
        Self {
            range_min: 8,
            range_max: 50,
            increase_x_height: 14,
            hint_composites: false,
        }
    }
}

/// PostScript hinting values shared by every CFF export (the Private DICT's
/// blue zone controls). Defaults follow the CFF specification's recommendations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PsHinting {
    /// Point size at which blue zones stop flattening overshoots (in points).
    pub blue_scale: f64,
    /// How far a stem may reach past a blue zone and still be flattened (units).
    pub blue_shift: f64,
    /// Extra slack around blue zones so nearby edges snap together (units).
    pub blue_fuzz: f64,
}

impl Default for PsHinting {
    fn default() -> Self {
        Self {
            blue_scale: 0.039625,
            blue_shift: 7.0,
            blue_fuzz: 1.0,
        }
    }
}

/// An alignment zone: a flat edge (`position`) and its overshoot (`size`).
/// A positive size is a top zone (BlueValues); a negative size is a bottom zone
/// (OtherBlues). The baseline zone has position 0 and a negative size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Zone {
    pub position: f64,
    pub size: f64,
}

/// A standard stem width used for hinting. The first horizontal stem becomes
/// StdHW, the first vertical stem StdVW; all of them become StemSnapH/V.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stem {
    pub name: String,
    pub horizontal: bool,
    pub value: f64,
}

/// A manual PostScript stem hint on a layer. `width` 0 makes it a ghost hint,
/// which snaps a single edge; `ghost_top` then picks the edge (top or right when
/// true, bottom or left when false).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    pub horizontal: bool,
    /// The edge the hint starts at: y for horizontal hints, x for vertical.
    pub position: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ghost_top: bool,
}

impl Hint {
    pub fn is_ghost(&self) -> bool {
        self.width == 0.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Axis {
    pub name: String,
    pub tag: String,
    /// User-space coordinates, as in a designspace `<axis>` element.
    pub min: f64,
    pub default: f64,
    pub max: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// avar: piecewise-linear user → design points, sorted by user value.
    /// Empty means user and design coordinates are the same.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map: Vec<(f64, f64)>,
    /// STAT axis value labels: the names apps show for positions on this axis.
    /// Empty means the axis has no named values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<AxisValue>,
}

/// One STAT axis value label. Values are in user coordinates, as in a designspace
/// `<label>` and the STAT axis value tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisValue {
    pub name: String,
    /// The nominal user value this label names.
    pub value: f64,
    /// Optional range of user values the label covers (a format 2 axis value).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Whether apps may leave this name out of a style name (e.g. "Regular").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub elidable: bool,
    /// A linked user value (a format 3 axis value), e.g. Bold linking to Regular.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_value: Option<f64>,
}

/// One variable font cut from the family: a name, the file it exports to, the part
/// of each axis it covers, and the glyphs it keeps. Written as a designspace 5
/// `<variable-font>`; because fontc ignores those, export compiles each one from a
/// reduced designspace instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableFontSetting {
    pub name: String,
    /// Base file name (no extension) for the exported variable font.
    pub file_name: String,
    /// Per-axis user-range restrictions. An axis not listed keeps its full range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub axes: Vec<VariableAxisSubset>,
    /// Glyphs to keep; empty keeps every glyph. `.notdef` and component targets are
    /// always kept so the subset still composes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub glyphs: Vec<String>,
    /// Glyph swaps for this variable font (see [`Instance::rename_glyphs`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rename_glyphs: Vec<[String; 2]>,
    /// Glyphs left out of this variable font.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_glyphs: Vec<String>,
    /// Masters (by name) left out of this variable font ("Disable Masters"); the axis
    /// ranges shrink to the masters that remain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_masters: Vec<String>,
    /// The variable font's own family name ("Variable Font Family Name"); `None`
    /// uses the font's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    /// Name records set by hand for this variable font ("Name Table Entry").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub name_records: Vec<NameRecord>,
}

/// The part of one axis a variable font covers, in user coordinates. A missing
/// bound falls back to the axis's own minimum or maximum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableAxisSubset {
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
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
    /// Global guides: drawn in every glyph of this master.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// Alignment zones for PostScript hinting (blue values).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alignment_zones: Vec<Zone>,
    /// Standard stem widths for PostScript hinting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stems: Vec<Stem>,
    /// OpenType line spacing and underline values set by hand.
    #[serde(default, skip_serializing_if = "VerticalMetrics::is_empty")]
    pub vertical: VerticalMetrics,
}

/// OpenType vertical metrics for one master. Values left unset are worked out by the
/// compiler from the ascender and descender.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerticalMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typo_ascender: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typo_descender: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typo_line_gap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hhea_ascender: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hhea_descender: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hhea_line_gap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub win_ascent: Option<f64>,
    /// Positive below the baseline, as in the OS/2 table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub win_descent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline_thickness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline_position: Option<f64>,
    /// vhea ascender, descender and line gap for vertical typesetting (Glyphs'
    /// `vheaVertAscender`…). Unset: half the em above and below, as ufo2ft does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vhea_ascender: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vhea_descender: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vhea_line_gap: Option<f64>,
}

impl VerticalMetrics {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
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
    /// The name this glyph carries in exported fonts when production names are used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production_name: Option<String>,
    /// Writing direction set for this glyph; `None` takes it from glyph data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<WritingDirection>,
    /// A locked glyph refuses every edit until it is unlocked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    /// For a smart glyph: the axes components of it can be set on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub smart_axes: Vec<SmartAxis>,
    /// One layer per master, plus any brace (intermediate), bracket (alternate) and
    /// smart pole layers.
    pub layers: Vec<Layer>,
    /// Bitmap pictures for the sbix table (S63), one per pixel size.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bitmaps: Vec<GlyphBitmap>,
    /// A drawing for the SVG table (S63).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svg: Option<GlyphSvg>,
}

/// A PNG picture of a glyph at one pixel size (an sbix strike). Like a layer image,
/// the file is referenced by path, relative to the font file where possible.
/// A glyph's writing direction (Glyphs' `direction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WritingDirection {
    #[serde(rename = "LTR")]
    Ltr,
    #[serde(rename = "RTL")]
    Rtl,
    #[serde(rename = "BIDI")]
    Bidi,
}

impl WritingDirection {
    /// From a Glyphs value (`LTR`, `RTL`, `BIDI`), any case.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "LTR" => Some(Self::Ltr),
            "RTL" => Some(Self::Rtl),
            "BIDI" => Some(Self::Bidi),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ltr => "LTR",
            Self::Rtl => "RTL",
            Self::Bidi => "BIDI",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphBitmap {
    /// The pixel size (pixels per em) this picture is drawn for.
    pub ppem: u16,
    pub path: String,
    /// Position of the picture's bottom-left corner, in font units. Each image pixel
    /// covers units-per-em / ppem font units.
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

/// An SVG drawing of a glyph, placed like a layer image: its bottom-left corner at
/// (x, y) in font units, one SVG unit per font unit times `scale`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphSvg {
    pub path: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default = "one")]
    pub scale: f64,
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
            production_name: None,
            direction: None,
            locked: false,
            smart_axes: Vec::new(),
            layers: Vec::new(),
            bitmaps: Vec::new(),
            svg: None,
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

    /// Smart pole layers.
    pub fn smart_layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers.iter().filter(|l| l.smart.is_some())
    }

    /// Whether components of this glyph can be set on smart axes.
    pub fn is_smart(&self) -> bool {
        !self.smart_axes.is_empty()
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
    /// Smart pole layer: its position on the glyph's smart axes (by axis name). The
    /// glyph's master layer is the pole at every axis' minimum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart: Option<BTreeMap<String, f64>>,
    /// Colour layer: drawn in colour on top of the glyph's other colour layers of the
    /// same master, in layer order (COLR). Every master has the same colour layers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<crate::color::ColorInfo>,
    pub width: f64,
    /// Vertical advance for vertical typesetting (Glyphs' `vertWidth`). Unset: the
    /// master's typo ascender minus its typo descender.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vert_width: Option<f64>,
    /// How far the vertical origin sits below the typo ascender (Glyphs'
    /// `vertOrigin`). Unset: the origin is at the typo ascender.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vert_origin: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<Shape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<Anchor>,
    #[serde(default, skip_serializing_if = "MetricsKeys::is_empty")]
    pub metrics_keys: MetricsKeys,
    /// Local guides: drawn only in this layer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// Manual PostScript stem hints, exported with CFF outlines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<Hint>,
    /// A reference drawing behind the outline (a previous version or a scan).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Background>,
    /// A backup layer: a saved version of this master's drawing, by name. It isn't
    /// exported, interpolated or checked; it's kept for reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    /// Corner, cap and segment components placed on this layer's paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<crate::parts::PathPart>,
    /// Review notes on the canvas (S32): never exported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
    /// A placed image, such as a scan to trace (S33): never exported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<LayerImage>,
    /// Higher-order interpolation (S40): curves some nodes follow on their way to
    /// the next master up an axis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hoi: Vec<crate::hoi::HoiCurve>,
}

/// What an annotation draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationKind {
    Text,
    Arrow,
    Circle,
    Plus,
    Minus,
}

/// A note or correction mark on a glyph layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub kind: AnnotationKind,
    pub x: f64,
    pub y: f64,
    /// Text annotations: the note.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Arrows: the direction they point, in degrees (0 points right).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
    /// Text: box width; circles: diameter. 0 means the default size.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub size: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// An image placed on a layer. The path is kept as given (relative to the font
/// file where possible), so images in a folder next to the font travel with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerImage {
    pub path: String,
    /// Position of the image's bottom-left corner, in font units.
    pub x: f64,
    pub y: f64,
    /// Scale: 1 draws one image point per font unit.
    #[serde(default = "one")]
    pub scale_x: f64,
    #[serde(default = "one")]
    pub scale_y: f64,
    /// Rotation in degrees, counter-clockwise, around the bottom-left corner.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
    /// The part of the image shown, in image points from its bottom-left corner:
    /// [x, y, width, height]. `None` shows the whole image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
}

fn one() -> f64 {
    1.0
}

/// A straight guide line through `(x, y)` at `angle` degrees from horizontal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Guide {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
}

/// The reference drawing behind a layer's outline: shapes and anchors kept aside so
/// the designer can trace over them without them exporting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Background {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<Shape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<Anchor>,
}

impl Background {
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty() && self.anchors.is_empty()
    }
}

impl Font {
    /// Whether the font has vertical metrics: a layer with a vertical width or origin,
    /// or a master with vhea values. Export then writes vhea and vmtx.
    pub fn uses_vertical_metrics(&self) -> bool {
        self.masters.iter().any(|m| {
            m.vertical.vhea_ascender.is_some()
                || m.vertical.vhea_descender.is_some()
                || m.vertical.vhea_line_gap.is_some()
        }) || self.glyphs.iter().any(|g| {
            g.layers
                .iter()
                .any(|l| l.vert_width.is_some() || l.vert_origin.is_some())
        })
    }
}

impl Layer {
    pub fn new(master_id: impl Into<String>, width: f64) -> Self {
        Self {
            master_id: master_id.into(),
            id: None,
            location: None,
            bracket: Vec::new(),
            smart: None,
            color: None,
            width,
            vert_width: None,
            vert_origin: None,
            shapes: Vec::new(),
            anchors: Vec::new(),
            metrics_keys: MetricsKeys::default(),
            guides: Vec::new(),
            hints: Vec::new(),
            background: None,
            backup: None,
            parts: Vec::new(),
            annotations: Vec::new(),
            image: None,
            hoi: Vec::new(),
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
        if let Some(pole) = &self.smart {
            let parts: Vec<String> = pole
                .iter()
                .map(|(a, v)| format!("{a} {}", fmt_num(*v)))
                .collect();
            return Some(format!("Pole {}", parts.join(", ")));
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
            .or_else(|| {
                font.axes
                    .iter()
                    .find(|a| a.tag == r.tag)
                    .map(|a| crate::designspace::design_bounds(a).1)
            })
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
        /// For a smart component: its value on each of the base glyph's smart axes
        /// (axes left out sit at their minimum).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        smart: BTreeMap<String, f64>,
        /// Whether the component is placed by its anchors (true) or by hand (false).
        #[serde(default = "yes", skip_serializing_if = "is_true")]
        align: bool,
    },
}

impl Shape {
    /// A plain component of `base`, aligned by its anchors.
    pub fn component(base: impl Into<String>, transform: [f64; 6]) -> Shape {
        Shape::Component {
            base: base.into(),
            transform,
            smart: BTreeMap::new(),
            align: true,
        }
    }
}

/// An axis of a smart glyph (one used as a smart component), such as "Width" from
/// 0 to 100. The master layer sits at every axis' minimum; pole layers sit elsewhere.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartAxis {
    pub name: String,
    pub min: f64,
    pub max: f64,
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
    /// Locked paths can't be selected for editing; the flag travels with the file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    /// Drawn with a pen of this stroke instead of filled (expanded to outlines on
    /// export).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<crate::stroke::PathStroke>,
    /// A mask: this path is subtracted from every shape below it (non-destructive).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mask: bool,
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
            guides: Vec::new(),
            alignment_zones: Vec::new(),
            stems: Vec::new(),
            vertical: Default::default(),
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
                designer_url: None,
                manufacturer: None,
                manufacturer_url: None,
                license: None,
                license_url: None,
                description: None,
                trademark: None,
                sample_text: None,
                hinting: None,
                ps_hinting: None,
                control_instructions: String::new(),
                grid: None,
                use_typo_metrics: false,
                custom_parameters: Vec::new(),
                virtual_masters: Vec::new(),
                numbers: BTreeMap::new(),
            },
            axes: Vec::new(),
            masters: vec![master],
            glyphs,
            instances: Vec::new(),
            kerning: BTreeMap::new(),
            features: String::new(),
            notes: String::new(),
            feature_settings: FeatureSettings::default(),
            palettes: Vec::new(),
            master_palettes: BTreeMap::new(),
            filters: Vec::new(),
            variable_fonts: Vec::new(),
            export_presets: Vec::new(),
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
    /// The number of colours in each palette (0 without palettes).
    pub fn palette_size(&self) -> usize {
        self.palettes.first().map_or(0, Vec::len)
    }

    pub fn validate(&self) -> crate::Result<()> {
        use std::collections::HashSet;
        let bad = |m: String| Err(crate::Error::Invalid(m));
        crate::presets::validate(&self.export_presets).map_err(crate::Error::Invalid)?;
        if self.masters.is_empty() {
            return bad("a font needs at least one master".into());
        }
        let master_ids: HashSet<&str> = self.masters.iter().map(|m| m.id.as_str()).collect();
        if master_ids.len() != self.masters.len() {
            return bad("master ids must be unique".into());
        }
        if self.palettes.iter().any(|p| p.len() != self.palette_size()) {
            return bad("every colour palette must have the same number of colours".into());
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
                    let kinds = usize::from(l.location.is_some())
                        + usize::from(!l.bracket.is_empty())
                        + usize::from(l.smart.is_some())
                        + usize::from(l.color.is_some())
                        + usize::from(l.backup.is_some());
                    if kinds != 1 {
                        return bad(format!(
                            "layer \"{id}\" of \"{}\" must be one of a brace, bracket, smart pole, colour or backup layer",
                            g.name
                        ));
                    }
                    if let Some(c) = &l.color {
                        if let Some(i) = c
                            .paint
                            .colors()
                            .find(|i| *i as usize >= self.palette_size())
                        {
                            return bad(format!(
                                "colour layer \"{id}\" of \"{}\" uses colour {i}, which the palette doesn't have",
                                g.name
                            ));
                        }
                    }
                    if let Some(pole) = &l.smart {
                        if let Some(a) = pole
                            .keys()
                            .find(|a| !g.smart_axes.iter().any(|s| &s.name == *a))
                        {
                            return bad(format!(
                                "layer \"{id}\" of \"{}\" uses unknown smart axis \"{a}\"",
                                g.name
                            ));
                        }
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
                } else if l.location.is_some()
                    || !l.bracket.is_empty()
                    || l.smart.is_some()
                    || l.color.is_some()
                {
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
    /// OS/2 usWeightClass override; `None` lets export derive it from the axes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight_class: Option<u16>,
    /// OS/2 usWidthClass override; `None` lets export derive it from the axes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_class: Option<u16>,
    /// Style linking (bold/italic flags and the family they link to); `None`
    /// means export derives the style map from the instance name and location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_link: Option<StyleLink>,
    /// Glyph swaps at export ("Rename Glyphs"): each pair trades drawings, so the
    /// first glyph is drawn like the second and the other way round.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rename_glyphs: Vec<[String; 2]>,
    /// Glyphs left out of this export ("Remove Glyphs").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_glyphs: Vec<String>,
    /// This instance's own family name, so one source can export several families
    /// (Glyphs "familyNames" / "familyName"). `None` uses the font's family name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    /// Family names in other languages, keyed by Glyphs language tag ("DEU", "JAN"…).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub localized_family_names: BTreeMap<String, String>,
    /// Masters (by name) this instance is interpolated without ("Disable Masters").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_masters: Vec<String>,
    /// Naming overrides: PostScript and full names, style-map family, file name and
    /// name table entries (Glyphs naming parameters).
    #[serde(default, skip_serializing_if = "NameOverrides::is_empty")]
    pub names: NameOverrides,
}

/// An instance's naming overrides. Empty fields use the names export derives.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameOverrides {
    /// nameID 6 and the CFF FontName ("postscriptFontName").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postscript_font_name: Option<String>,
    /// nameID 4, the full font name ("postscriptFullName").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    /// nameID 18, the Mac compatible full name ("compatibleFullName").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatible_full_name: Option<String>,
    /// nameID 1, the family name for style linking ("styleMapFamilyName").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_map_family_name: Option<String>,
    /// Output file name without extension ("fileName").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// Name records set by hand ("Name Table Entry").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<NameRecord>,
}

impl NameOverrides {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One name table record ("Name Table Entry"): `nameID; text`, optionally with the
/// platform, encoding and language ids (Windows English Unicode by default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameRecord {
    pub name_id: u16,
    pub platform_id: u16,
    pub encoding_id: u16,
    pub language_id: u16,
    pub value: String,
}

impl NameRecord {
    /// Parses Glyphs' syntax: `nameID; text`, `nameID platformID; text` or
    /// `nameID platformID encID langID; text`. Numbers may be decimal, octal (leading
    /// 0) or hexadecimal (0x). Name IDs 1, 2, 3, 5 and 6 are set by export and can't be
    /// overridden here.
    pub fn parse(s: &str) -> Result<Self, String> {
        let (ids, value) = s.split_once(';').ok_or_else(|| {
            format!(
                "\"{}\" needs a semicolon between the name ID and the text, like \"0; © 2026 Me\".",
                s.trim()
            )
        })?;
        let value = value.trim().to_string();
        if value.is_empty() {
            return Err(format!("\"{}\" has no text after the semicolon.", s.trim()));
        }
        let num = |t: &str| -> Result<u16, String> {
            let n = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                u32::from_str_radix(h, 16)
            } else if t.len() > 1 && t.starts_with('0') {
                u32::from_str_radix(&t[1..], 8)
            } else {
                t.parse::<u32>()
            };
            n.ok()
                .and_then(|n| u16::try_from(n).ok())
                .ok_or_else(|| format!("\"{t}\" isn't a number from 0 to 65535."))
        };
        let nums = ids
            .split_whitespace()
            .map(num)
            .collect::<Result<Vec<u16>, String>>()?;
        let (name_id, platform_id, encoding_id, language_id) = match nums[..] {
            [id] => (id, 3, 1, 0x409),
            [id, 1] => (id, 1, 0, 0),
            [id, 3] => (id, 3, 1, 0x409),
            [_, p] => return Err(format!("Platform {p} isn't supported; use 1 (Mac) or 3 (Windows).")),
            [id, p, e, l] if p == 1 || p == 3 => (id, p, e, l),
            [_, p, _, _] => return Err(format!("Platform {p} isn't supported; use 1 (Mac) or 3 (Windows).")),
            _ => {
                return Err(format!(
                    "\"{}\" should start with a name ID, optionally followed by the platform, or the platform, encoding and language IDs.",
                    ids.trim()
                ))
            }
        };
        if matches!(name_id, 1 | 2 | 3 | 5 | 6) {
            return Err(format!(
                "Name ID {name_id} is set by export; use the instance's names instead."
            ));
        }
        Ok(Self {
            name_id,
            platform_id,
            encoding_id,
            language_id,
            value,
        })
    }

    /// Back to Glyphs' syntax, as short as the ids allow.
    pub fn to_glyphs(&self) -> String {
        let ids = match (self.platform_id, self.encoding_id, self.language_id) {
            (3, 1, 0x409) => self.name_id.to_string(),
            (1, 0, 0) => format!("{} 1", self.name_id),
            (p, e, l) => format!("{} {p} {e} {l}", self.name_id),
        };
        format!("{ids}; {}", self.value)
    }
}

impl Instance {
    /// The family this instance exports under: its own family name, or `font_family`.
    pub fn family<'a>(&'a self, font_family: &'a str) -> &'a str {
        match self.family_name.as_deref().map(str::trim) {
            Some(f) if !f.is_empty() => f,
            _ => font_family,
        }
    }

    /// Whether this instance names a family other than `font_family`.
    pub fn has_own_family(&self, font_family: &str) -> bool {
        self.family(font_family) != font_family
    }
}

/// How an instance participates in the four-style (RIBBI) style map. `base` is
/// the style-map family name the linked styles share (usually the family name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleLink {
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub base: String,
}

#[cfg(test)]
mod name_record_tests {
    use super::NameRecord;

    #[test]
    fn parses_glyphs_name_table_entries() {
        let r = NameRecord::parse("0; © 2026 Me").unwrap();
        assert_eq!(
            (r.name_id, r.platform_id, r.encoding_id, r.language_id),
            (0, 3, 1, 0x409)
        );
        assert_eq!(r.value, "© 2026 Me");
        let mac = NameRecord::parse("9 1; Mac").unwrap();
        assert_eq!(
            (mac.platform_id, mac.encoding_id, mac.language_id),
            (1, 0, 0)
        );
        // Hexadecimal and octal ids.
        let full = NameRecord::parse("0x10 3 1 0x407; Familie").unwrap();
        assert_eq!((full.name_id, full.language_id), (16, 0x407));
        assert_eq!(NameRecord::parse("010; x").unwrap().name_id, 8);
        // Round trip, as short as the ids allow.
        assert_eq!(r.to_glyphs(), "0; © 2026 Me");
        assert_eq!(mac.to_glyphs(), "9 1; Mac");
        assert_eq!(full.to_glyphs(), "16 3 1 1031; Familie");
        assert_eq!(NameRecord::parse(&full.to_glyphs()).unwrap(), full);
        // Mistakes.
        assert!(NameRecord::parse("0 © 2026").is_err());
        assert!(NameRecord::parse("6; MyFont-Bold").is_err());
        assert!(NameRecord::parse("0 2; x").is_err());
        assert!(NameRecord::parse("0;  ").is_err());
        assert!(NameRecord::parse("70000; x").is_err());
    }
}
