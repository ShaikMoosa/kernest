//! Commands the TypeScript UI calls through Tauri IPC.
//!
//! The open document (font, path, undo history) lives in Rust; the UI receives
//! views shaped for each screen and sends edits back as whole-layer updates.
//! Errors are returned as user-facing strings.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use kernest_core::edit::{component_creates_cycle, with_metric, Affected, History, MetricEdit};
use kernest_core::geometry::{sidebearings, svg_path};
use kernest_core::glyphdata::{codepoints_for, GLYPH_SETS};
use kernest_core::interpolate::check_glyph;
use kernest_core::{
    filters, package, pathops, sample,
    transform::{self, Selection},
    Anchor, Font, Grid, Guide, Hint, Layer, Metrics, Node, NodeKind, Path as KPath, Shape,
    SmartAxis,
};
use serde::{Deserialize, Serialize};
use tauri::State;

/// A compiled preview font with the revision and location it was built from,
/// plus the glyph order its ids map to.
pub(crate) type PreviewFont = (
    u64,
    Option<kernest_core::designspace::Location>,
    Vec<u8>,
    Vec<String>,
);

pub struct Document {
    pub font: Font,
    pub path: Option<PathBuf>,
    pub history: History,
    /// Incremented on every change; compared with `saved_revision` for the unsaved-changes state.
    pub(crate) revision: u64,
    saved_revision: u64,
    /// Compiled preview font for the text view's feature shaping, keyed by the
    /// revision and design-space location it was built from so edits rebuild it.
    pub(crate) preview: Option<PreviewFont>,
}

impl Document {
    pub(crate) fn new(font: Font, path: Option<PathBuf>) -> Self {
        Self {
            font,
            path,
            history: History::default(),
            revision: 0,
            saved_revision: 0,
            preview: None,
        }
    }
    fn dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
}

#[derive(Default)]
pub struct AppState(Mutex<Option<Document>>);

pub(crate) type CmdResult<T> = Result<T, String>;

pub(crate) fn lock(state: &AppState) -> CmdResult<MutexGuard<'_, Option<Document>>> {
    state
        .0
        .lock()
        .map_err(|_| "Internal error: state lock poisoned".to_string())
}

pub(crate) fn doc_mut<'a>(
    guard: &'a mut MutexGuard<'_, Option<Document>>,
) -> CmdResult<&'a mut Document> {
    guard.as_mut().ok_or_else(|| "No font is open.".to_string())
}

/// Editing commands call this first: a locked glyph refuses every edit.
pub(crate) fn ensure_glyph_unlocked(font: &Font, glyph: &str) -> CmdResult<()> {
    match font.glyph(glyph) {
        Some(g) if g.locked => Err("This glyph is locked. Unlock it to edit.".into()),
        Some(_) => Ok(()),
        None => Err(format!("No glyph named \"{glyph}\"")),
    }
}

// ---------- Views ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphSetInfo {
    id: &'static str,
    name: &'static str,
    count: usize,
    /// Up to 64 of the set's characters, for the New Font preview.
    sample: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterInfo {
    id: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphSummary {
    name: String,
    unicode: Option<String>,
    /// Every Unicode value, the first being `unicode`.
    unicodes: Vec<String>,
    character: Option<String>,
    category: &'static str,
    subcategory: String,
    script: String,
    /// The glyph's own category, subcategory, case, script and sort name, where set
    /// (absent: taken from glyph data). Glyphs' terms, as stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    category_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subcategory_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    case_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    script_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sort_name: Option<String>,
    /// Colour labels (SMO-52): the glyph's, and its layer's in the shown master, as
    /// the stored label (index or RGBA) plus RGBA for drawing.
    #[serde(skip_serializing_if = "Option::is_none")]
    color_label: Option<kernest_core::ColorLabel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    layer_color_label: Option<kernest_core::ColorLabel>,
    export: bool,
    width: f64,
    lsb: Option<f64>,
    rsb: Option<f64>,
    left_group: Option<String>,
    right_group: Option<String>,
    tags: Vec<String>,
    /// The name this glyph exports under when production names are used.
    #[serde(skip_serializing_if = "Option::is_none")]
    production_name: Option<String>,
    /// Writing direction set on the glyph (LTR, RTL, BIDI); absent means automatic.
    #[serde(skip_serializing_if = "Option::is_none")]
    direction: Option<&'static str>,
    /// Vertical metrics in this master: advance, origin and sidebearings.
    #[serde(skip_serializing_if = "Option::is_none")]
    vertical: Option<VerticalView>,
    /// Outline as SVG path data in font units (y up), empty when not drawn yet.
    svg: String,
    /// False when this glyph's masters can't be interpolated.
    compatible: bool,
    /// Compatible masters that would morph badly (start points, direction, order, kinks).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    interpolation_warnings: bool,
    /// A locked glyph refuses every edit until it is unlocked.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    locked: bool,
    /// Colour layers in this master, bottom to top (empty for black-and-white glyphs).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    color: Vec<ColorLayerView>,
}

/// A glyph's vertical metrics in one master (for vertical typesetting).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerticalView {
    advance: f64,
    origin: f64,
    tsb: Option<f64>,
    bsb: Option<f64>,
    /// True when the layer has its own vertical width or origin (not the defaults).
    own: bool,
}

/// A colour layer as the UI draws it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorLayerView {
    key: String,
    /// Outline as SVG path data in font units (y up).
    svg: String,
    paint: kernest_core::color::Paint,
}

/// `glyph`'s colour layers in master `master_id`, bottom to top.
pub(crate) fn color_views(
    font: &Font,
    g: &kernest_core::Glyph,
    master_id: &str,
) -> Vec<ColorLayerView> {
    g.color_layers(master_id)
        .filter_map(|l| {
            Some(ColorLayerView {
                key: l.key().to_string(),
                svg: svg_path(font, l),
                paint: l.color.as_ref()?.paint.clone(),
            })
        })
        .collect()
}

/// Palette 0 as CSS colours (`#rrggbbaa`).
pub(crate) fn css_palette(font: &Font) -> Vec<String> {
    font.palettes
        .first()
        .map(|p| p.iter().map(|c| c.to_string()).collect())
        .unwrap_or_default()
}

/// Palette 0 as CSS colours for one master, honouring its palette override.
pub(crate) fn css_palette_at(font: &Font, master_id: &str) -> Vec<String> {
    kernest_core::color::effective_palettes(font, master_id)
        .first()
        .map(|p| p.iter().map(|c| c.to_string()).collect())
        .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocStatus {
    pub(crate) dirty: bool,
    pub(crate) can_undo: bool,
    pub(crate) can_redo: bool,
    /// What Undo and Redo would do, for the Edit menu (SMO-83).
    pub(crate) undo: Option<StepView>,
    pub(crate) redo: Option<StepView>,
}

/// One undo step as the Edit menu names it: "Undo Edit Outlines in “a”".
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    action: &'static str,
    glyph: Option<String>,
    glyphs: usize,
}

impl From<kernest_core::edit::StepInfo> for StepView {
    fn from(i: kernest_core::edit::StepInfo) -> Self {
        Self {
            action: i.action,
            glyph: i.glyph,
            glyphs: i.glyphs,
        }
    }
}

pub(crate) fn status(doc: &Document) -> DocStatus {
    DocStatus {
        dirty: doc.dirty(),
        can_undo: doc.history.can_undo(),
        can_redo: doc.history.can_redo(),
        undo: doc.history.undo_info().map(StepView::from),
        redo: doc.history.redo_info().map(StepView::from),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontView {
    family_name: String,
    path: Option<String>,
    units_per_em: u16,
    masters: Vec<MasterInfo>,
    master_id: String,
    metrics: Metrics,
    glyphs: Vec<GlyphSummary>,
    /// The default colour palette, for drawing colour glyphs.
    palette: Vec<String>,
    /// Saved smart and list filters for the Font View sidebar.
    filters: Vec<kernest_core::smartfilter::Filter>,
    status: DocStatus,
}

/// A node as the editor sees it.
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct NodeDto {
    x: f64,
    y: f64,
    /// "line" | "curve" | "qcurve" | "offcurve"
    kind: NodeKindDto,
    #[serde(default)]
    smooth: bool,
    /// Super-smooth (G2): kept where the curvature matches on both sides.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    g2: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeKindDto {
    Line,
    Curve,
    Qcurve,
    Offcurve,
}

impl From<NodeKind> for NodeKindDto {
    fn from(k: NodeKind) -> Self {
        match k {
            NodeKind::Line => Self::Line,
            NodeKind::Curve => Self::Curve,
            NodeKind::QCurve => Self::Qcurve,
            NodeKind::OffCurve => Self::Offcurve,
        }
    }
}

impl From<NodeKindDto> for NodeKind {
    fn from(k: NodeKindDto) -> Self {
        match k {
            NodeKindDto::Line => Self::Line,
            NodeKindDto::Curve => Self::Curve,
            NodeKindDto::Qcurve => Self::QCurve,
            NodeKindDto::Offcurve => Self::OffCurve,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PathDto {
    closed: bool,
    nodes: Vec<NodeDto>,
    #[serde(default)]
    pub(crate) locked: bool,
    /// The path's stroke, if it is drawn with a pen (kept through edits).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stroke: Option<kernest_core::stroke::PathStroke>,
    /// Whether the path is a mask (subtracted from the shapes below it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) mask: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDto {
    base: String,
    transform: [f64; 6],
    /// Values on the base glyph's smart axes (smart components only).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    smart: BTreeMap<String, f64>,
    /// Whether anchors place this component (false once it is moved by hand).
    pub(crate) align: bool,
    /// The base glyph's smart axes, if it is a smart glyph.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    smart_axes: Vec<SmartAxis>,
    /// This component's resolved outline, SVG path data in font units (y up).
    svg: String,
    /// Smart handles: anchors named after smart axes, to drag on the canvas.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    handles: Vec<kernest_core::smart::SmartHandle>,
}

/// A component as sent back by the editor (move, add or remove).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInput {
    base: String,
    transform: [f64; 6],
    #[serde(default)]
    smart: BTreeMap<String, f64>,
    #[serde(default = "aligned")]
    align: bool,
}

fn aligned() -> bool {
    true
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerView {
    glyph: String,
    unicode: Option<String>,
    master_id: String,
    metrics: Metrics,
    pub(crate) width: f64,
    lsb: Option<f64>,
    rsb: Option<f64>,
    pub(crate) paths: Vec<PathDto>,
    pub(crate) components: Vec<ComponentDto>,
    /// All components resolved to outlines, as SVG path data in font units (y up).
    component_svg: String,
    /// The outlines the engine draws for this layer's stroked paths and paths with
    /// corner, cap or segment components, as SVG path data.
    stroke_svg: String,
    /// Which paths (counting paths only) `stroke_svg` draws; the editor doesn't
    /// fill these itself.
    engine_paths: Vec<usize>,
    /// The layer has masks: `stroke_svg` is the whole masked outline, components
    /// included, and the editor draws the cut-away parts in grey under it.
    engine_all: bool,
    /// The drawing order of the layer's shapes: which path or component comes
    /// first, second… (paths and components numbered separately).
    shape_order: Vec<ShapeRef>,
    /// Corner, cap and segment components placed on this layer's paths.
    parts: Vec<kernest_core::parts::PathPart>,
    /// Review notes on the canvas.
    annotations: Vec<kernest_core::Annotation>,
    /// The placed image, if any.
    image: Option<kernest_core::LayerImage>,
    pub(crate) anchors: Vec<Anchor>,
    /// This layer's manual PostScript hints (empty means auto stems are used).
    pub(crate) hints: Vec<Hint>,
    /// The automatic stem hints the master's stems produce for this layer.
    auto_hints: Vec<Hint>,
    /// The master's alignment zones, for highlighting nodes in them (SMO-42).
    zones: Vec<kernest_core::Zone>,
    /// Metrics keys of this layer, e.g. `=n` for the left sidebearing.
    metrics_keys: kernest_core::MetricsKeys,
    /// This glyph's smart axes, if it is a smart glyph.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    smart_axes: Vec<SmartAxis>,
    /// Why this glyph's masters can't be interpolated (empty when they can).
    issues: Vec<String>,
    /// Compatible masters that would morph badly, and whether Fix can repair them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    interpolation_warnings: Vec<String>,
    can_fix_interpolation: bool,
    /// Whether the whole glyph is locked against editing.
    pub(crate) glyph_locked: bool,
    /// This master's colour layers, bottom to top, and the default palette.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    color_layers: Vec<ColorLayerView>,
    palette: Vec<String>,
    /// The paint of this layer, when it is a colour layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    paint: Option<kernest_core::color::Paint>,
    /// Local guides of this layer.
    pub(crate) guides: Vec<Guide>,
    /// Global guides of this layer's master, drawn in every glyph.
    pub(crate) master_guides: Vec<Guide>,
    /// The reference drawing behind the outline, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) background: Option<BackgroundView>,
    /// The font's snap grid, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) grid: Option<Grid>,
    pub(crate) status: DocStatus,
}

/// The background layer, shaped like the foreground so it can be edited in place.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundView {
    pub(crate) paths: Vec<PathDto>,
    components: Vec<ComponentDto>,
    component_svg: String,
    anchors: Vec<Anchor>,
}

fn font_view_of(doc: &Document, master_id: Option<&str>) -> CmdResult<FontView> {
    let font = &doc.font;
    let master = match master_id {
        Some(id) => font
            .master(id)
            .ok_or_else(|| format!("No master with id \"{id}\""))?,
        None => &font.masters[0],
    };
    let glyphs = font
        .glyphs
        .iter()
        .map(|g| {
            let layer = g.layer(&master.id);
            let sb = layer.and_then(|l| sidebearings(font, l));
            let cp = g.unicodes.first();
            GlyphSummary {
                name: g.name.clone(),
                unicode: cp.map(|c| c.to_string()),
                unicodes: g.unicodes.iter().map(|c| c.to_string()).collect(),
                character: cp.and_then(|c| c.char()).map(String::from),
                category: g.category().label(),
                subcategory: g.subcategory(),
                script: g.script(),
                category_override: g.category_override.clone(),
                subcategory_override: g.subcategory_override.clone(),
                case_override: g.case_override.clone(),
                script_override: g.script_override.clone(),
                sort_name: g.sort_name.clone(),
                color_label: g.color_label,
                layer_color_label: layer.and_then(|l| l.color_label),
                export: g.export,
                width: layer.map_or(0.0, |l| l.width),
                lsb: sb.map(|s| s.0),
                rsb: sb.map(|s| s.1),
                left_group: g.left_group.clone(),
                right_group: g.right_group.clone(),
                tags: g.tags.clone(),
                production_name: g.production_name.clone(),
                direction: g.direction.map(|d| d.as_str()),
                vertical: layer.map(|l| {
                    let (origin, advance) = kernest_core::vertical::origin_and_advance(master, l);
                    let sb = kernest_core::vertical::sidebearings(font, master, l);
                    VerticalView {
                        advance,
                        origin,
                        tsb: sb.map(|s| s.0),
                        bsb: sb.map(|s| s.1),
                        own: l.vert_width.is_some() || l.vert_origin.is_some(),
                    }
                }),
                svg: layer.map(|l| svg_path(font, l)).unwrap_or_default(),
                compatible: check_glyph(font, &g.name).is_empty(),
                interpolation_warnings: font.masters.len() > 1
                    && kernest_core::interpolation_quality::has_warnings(font, &g.name),
                locked: g.locked,
                color: color_views(font, g, &master.id),
            }
        })
        .collect();
    Ok(FontView {
        family_name: font.info.family_name.clone(),
        path: doc.path.as_ref().map(|p| p.display().to_string()),
        units_per_em: font.info.units_per_em,
        masters: font
            .masters
            .iter()
            .map(|m| MasterInfo {
                id: m.id.clone(),
                name: m.name.clone(),
            })
            .collect(),
        master_id: master.id.clone(),
        metrics: master.metrics,
        glyphs,
        palette: css_palette(font),
        filters: font.filters.clone(),
        status: status(doc),
    })
}

pub(crate) fn layer_view_of(doc: &Document, glyph: &str, master_id: &str) -> CmdResult<LayerView> {
    layer_view_font(doc, &doc.font, glyph, master_id)
}

/// The layer view as drawn from `font`, which may be a preview clone rather than
/// the document's own font.
fn layer_view_font(
    doc: &Document,
    font: &Font,
    glyph: &str,
    master_id: &str,
) -> CmdResult<LayerView> {
    let g = font
        .glyph(glyph)
        .ok_or_else(|| format!("No glyph named \"{glyph}\""))?;
    // `master_id` is a layer key: a master id, or a brace/bracket layer's id.
    let layer = g
        .layer(master_id)
        .ok_or_else(|| format!("\"{glyph}\" has no layer \"{master_id}\""))?;
    let master = font
        .master(&layer.master_id)
        .ok_or_else(|| format!("No master with id \"{}\"", layer.master_id))?;
    let (paths, components, component_svg) = outline_views(
        font,
        &layer.master_id,
        layer.location.as_ref(),
        layer.width,
        &layer.shapes,
    );
    let background = layer.background.as_ref().map(|b| {
        let (paths, components, component_svg) = outline_views(
            font,
            &layer.master_id,
            layer.location.as_ref(),
            layer.width,
            &b.shapes,
        );
        BackgroundView {
            paths,
            components,
            component_svg,
            anchors: b.anchors.clone(),
        }
    });
    let sb = sidebearings(font, layer);
    Ok(LayerView {
        glyph: g.name.clone(),
        unicode: g.unicodes.first().map(|c| c.to_string()),
        master_id: master_id.into(),
        metrics: master.metrics,
        width: layer.width,
        lsb: sb.map(|s| s.0),
        rsb: sb.map(|s| s.1),
        paths,
        components,
        component_svg,
        stroke_svg: if kernest_core::masking::has_masks(layer) {
            kernest_core::geometry::svg_path(font, layer)
        } else {
            engine_svg(font, layer)
        },
        engine_paths: if kernest_core::masking::has_masks(layer) {
            (0..layer
                .shapes
                .iter()
                .filter(|s| matches!(s, Shape::Path(_)))
                .count())
                .collect()
        } else {
            engine_paths(layer)
        },
        engine_all: kernest_core::masking::has_masks(layer),
        shape_order: {
            let (mut p, mut c) = (0, 0);
            layer
                .shapes
                .iter()
                .map(|s| match s {
                    Shape::Path(_) => {
                        p += 1;
                        ShapeRef {
                            kind: "path",
                            index: p - 1,
                        }
                    }
                    _ => {
                        c += 1;
                        ShapeRef {
                            kind: "component",
                            index: c - 1,
                        }
                    }
                })
                .collect()
        },
        parts: layer.parts.clone(),
        annotations: layer.annotations.clone(),
        image: layer.image.clone(),
        anchors: layer.anchors.clone(),
        hints: layer.hints.clone(),
        zones: master.alignment_zones.clone(),
        auto_hints: kernest_core::hinting::auto_stems(
            font,
            layer,
            &master
                .stems
                .iter()
                .filter(|s| s.horizontal)
                .map(|s| s.value)
                .collect::<Vec<_>>(),
            &master
                .stems
                .iter()
                .filter(|s| !s.horizontal)
                .map(|s| s.value)
                .collect::<Vec<_>>(),
        ),
        metrics_keys: layer.metrics_keys.clone(),
        smart_axes: g.smart_axes.clone(),
        issues: check_glyph(font, glyph),
        interpolation_warnings: kernest_core::interpolation_quality::warnings(font, glyph),
        can_fix_interpolation: kernest_core::interpolation_quality::fixable(font, glyph),
        glyph_locked: g.locked,
        color_layers: color_views(font, g, &layer.master_id),
        palette: css_palette_at(font, &layer.master_id),
        paint: layer.color.as_ref().map(|c| c.paint.clone()),
        guides: layer.guides.clone(),
        master_guides: master.guides.clone(),
        background,
        grid: font.info.grid,
        status: status(doc),
    })
}

/// Paths, components and resolved component SVG for a set of shapes, as the UI
/// draws them. Shared by the foreground and the background layer.
fn outline_views(
    font: &Font,
    master_id: &str,
    location: Option<&BTreeMap<String, f64>>,
    width: f64,
    shapes: &[Shape],
) -> (Vec<PathDto>, Vec<ComponentDto>, String) {
    let mut paths = Vec::new();
    let mut components = Vec::new();
    let mut comp_only = Layer::new(master_id, width);
    for s in shapes {
        match s {
            Shape::Path(p) => paths.push(PathDto {
                closed: p.closed,
                locked: p.locked,
                stroke: p.stroke.clone(),
                mask: p.mask,
                nodes: p
                    .nodes
                    .iter()
                    .map(|n| NodeDto {
                        x: n.x,
                        y: n.y,
                        kind: n.kind.into(),
                        smooth: n.smooth,
                        g2: n.g2,
                    })
                    .collect(),
            }),
            Shape::Component {
                base,
                transform,
                smart,
                align,
            } => {
                let mut single = Layer::new(master_id, width);
                single.location = location.cloned();
                single.shapes.push(s.clone());
                components.push(ComponentDto {
                    base: base.clone(),
                    transform: *transform,
                    smart: smart.clone(),
                    align: *align,
                    smart_axes: font
                        .glyph(base)
                        .map(|b| b.smart_axes.clone())
                        .unwrap_or_default(),
                    svg: svg_path(font, &single),
                    handles: kernest_core::smart::handles(font, base, &single, smart, transform),
                });
                comp_only.shapes.push(s.clone());
            }
        }
    }
    (paths, components, svg_path(font, &comp_only))
}

/// Validates an edited path from the UI and converts it to the model.
pub(crate) fn to_path(p: &PathDto) -> CmdResult<KPath> {
    if p.nodes.iter().any(|n| !n.x.is_finite() || !n.y.is_finite()) {
        return Err("A point has an invalid coordinate.".into());
    }
    if !p.nodes.iter().any(|n| n.kind != NodeKindDto::Offcurve) {
        return Err("A path needs at least one on-curve point.".into());
    }
    if !p.closed
        && p.nodes
            .first()
            .is_some_and(|n| n.kind == NodeKindDto::Offcurve)
    {
        return Err("An open path must start with an on-curve point.".into());
    }
    // Cubic segments have at most two off-curve points in a row.
    let n = p.nodes.len();
    let start = p
        .nodes
        .iter()
        .position(|n| n.kind != NodeKindDto::Offcurve)
        .unwrap_or(0);
    let mut run = 0;
    for i in 0..n {
        let node = p.nodes[(start + i + 1) % n];
        if node.kind == NodeKindDto::Offcurve {
            run += 1;
        } else {
            if run > 2 && node.kind != NodeKindDto::Qcurve {
                return Err("A curve segment has more than two handles.".into());
            }
            run = 0;
        }
    }
    Ok(KPath {
        locked: p.locked,
        stroke: p.stroke.clone(),
        mask: p.mask,
        closed: p.closed,
        nodes: p
            .nodes
            .iter()
            .map(|d| Node {
                x: d.x,
                y: d.y,
                kind: d.kind.into(),
                smooth: (d.smooth || d.g2) && d.kind != NodeKindDto::Offcurve,
                g2: d.g2 && d.kind != NodeKindDto::Offcurve,
            })
            .collect(),
    })
}

/// Validates a component from the UI: the base must exist and must not lead back to `glyph`.
pub(crate) fn to_component(font: &Font, glyph: &str, c: ComponentInput) -> CmdResult<Shape> {
    if font.glyph(&c.base).is_none() {
        return Err(format!(
            "There is no glyph named \"{}\" to use as a component.",
            c.base
        ));
    }
    if component_creates_cycle(font, glyph, &c.base) {
        return Err(format!(
            "\"{}\" can't be a component of \"{glyph}\": it would contain itself.",
            c.base
        ));
    }
    if c.transform.iter().any(|v| !v.is_finite()) {
        return Err("A component has an invalid position.".into());
    }
    Ok(Shape::Component {
        base: c.base,
        transform: c.transform,
        smart: c.smart,
        align: c.align,
    })
}

// ---------- Document commands ----------

/// Opens an imported font: it has no file yet and counts as unsaved, so closing asks first.
pub(crate) fn replace_dirty(state: &AppState, font: Font) -> CmdResult<FontView> {
    let mut doc = Document::new(font, None);
    doc.revision = 1;
    replace(state, doc)
}

fn replace(state: &AppState, doc: Document) -> CmdResult<FontView> {
    let mut guard = lock(state)?;
    let v = font_view_of(&doc, None)?;
    *guard = Some(doc);
    Ok(v)
}

#[tauri::command]
pub fn glyph_sets() -> Vec<GlyphSetInfo> {
    GLYPH_SETS
        .iter()
        .map(|s| {
            let codepoints = codepoints_for(&[s.id]);
            // Letters first, then figures, then the rest, as a type specimen shows them.
            let mut chars: Vec<char> = codepoints
                .iter()
                .filter_map(|&c| char::from_u32(c))
                .filter(|c| !c.is_whitespace() && !c.is_control())
                .collect();
            chars.sort_by_key(|c| {
                if c.is_alphabetic() {
                    0
                } else if c.is_numeric() {
                    1
                } else {
                    2
                }
            });
            let sample = chars.into_iter().take(64).collect();
            GlyphSetInfo {
                id: s.id,
                name: s.name,
                count: codepoints.len(),
                sample,
            }
        })
        .collect()
}

#[tauri::command]
pub fn new_font(
    state: State<AppState>,
    setup: kernest_core::wizard::NewFontSetup,
) -> CmdResult<FontView> {
    replace_dirty(&state, kernest_core::wizard::create_font(&setup)?)
}

#[tauri::command]
pub fn open_sample(state: State<AppState>) -> CmdResult<FontView> {
    replace(&state, Document::new(sample::sample_font(), None))
}

#[tauri::command]
pub fn open_font(state: State<AppState>, path: String) -> CmdResult<FontView> {
    let mut dir = PathBuf::from(&path);
    // Accept the package folder or any file inside it (e.g. font.json picked in a file dialog).
    if dir.is_file() {
        dir = dir.parent().map(Path::to_path_buf).unwrap_or(dir);
        if dir.file_name().is_some_and(|n| n == "glyphs") {
            dir.pop();
        }
    }
    let font = package::open(&dir).map_err(|e| e.to_string())?;
    replace(&state, Document::new(font, Some(dir)))
}

/// Saves to `path` if given (Save As), otherwise to the document's current path.
/// Returns the path saved to.
#[tauri::command]
pub fn save_font(state: State<AppState>, path: Option<String>) -> CmdResult<String> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let target = match path {
        Some(p) => {
            let mut p = PathBuf::from(p);
            if p.extension().is_none_or(|e| e != package::EXTENSION) {
                p.set_extension(package::EXTENSION);
            }
            p
        }
        None => doc
            .path
            .clone()
            .ok_or("This font has not been saved yet. Use Save As.")?,
    };
    package::save(&doc.font, &target).map_err(|e| e.to_string())?;
    // The folder's Explorer icon and tooltip (S70).
    crate::explorer::decorate(&target, &doc.font);
    doc.path = Some(target.clone());
    doc.saved_revision = doc.revision;
    Ok(target.display().to_string())
}

#[tauri::command]
pub fn close_font(state: State<AppState>) -> CmdResult<()> {
    *lock(&state)? = None;
    Ok(())
}

#[tauri::command]
pub fn font_view(state: State<AppState>, master_id: Option<String>) -> CmdResult<FontView> {
    let mut guard = lock(&state)?;
    font_view_of(doc_mut(&mut guard)?, master_id.as_deref())
}

#[tauri::command]
pub fn doc_status(state: State<AppState>) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    Ok(status(doc_mut(&mut guard)?))
}

// ---------- Editing commands ----------

#[tauri::command]
pub fn glyph_layer(
    state: State<AppState>,
    glyph: String,
    master_id: String,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    layer_view_of(doc_mut(&mut guard)?, &glyph, &master_id)
}

/// Replaces the outlines and width of a layer. Components and anchors are kept.
#[tauri::command]
pub fn update_layer(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    width: f64,
    paths: Vec<PathDto>,
    components: Option<Vec<ComponentInput>>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    if !width.is_finite() || width < 0.0 {
        return Err("Width must be zero or more.".into());
    }
    let current = doc
        .font
        .glyph(&glyph)
        .and_then(|g| g.layer(&master_id))
        .ok_or_else(|| format!("No layer for \"{glyph}\""))?;
    let mut next = current.clone();
    next.width = width;
    let components: Vec<Shape> = match components {
        // Components not sent: keep the current ones.
        None => current
            .shapes
            .iter()
            .filter(|s| matches!(s, Shape::Component { .. }))
            .cloned()
            .collect(),
        Some(list) => list
            .into_iter()
            .map(|c| to_component(&doc.font, &glyph, c))
            .collect::<CmdResult<Vec<_>>>()?,
    };
    next.shapes = paths
        .iter()
        .map(|p| to_path(p).map(Shape::Path))
        .collect::<CmdResult<Vec<_>>>()?;
    next.shapes.extend(components);
    // Super-smooth nodes follow their handles (SMO-39).
    kernest_core::harmonize::apply_layer(&mut next);
    if doc
        .history
        .apply(&mut doc.font, &glyph, &master_id, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, &glyph, &master_id)
}

/// Sets the left sidebearing, right sidebearing or advance width ("lsb" | "rsb" | "width"),
/// or moves the outlines sideways inside the width by `value` ("shift").
#[tauri::command]
pub fn set_metric(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    metric: String,
    value: f64,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let edit = match metric.as_str() {
        "lsb" => MetricEdit::Lsb(value),
        "rsb" => MetricEdit::Rsb(value),
        "width" => MetricEdit::Width(value),
        "shift" => MetricEdit::Shift(value),
        other => return Err(format!("Unknown metric \"{other}\"")),
    };
    let next = with_metric(&doc.font, &glyph, &master_id, edit).map_err(|e| e.to_string())?;
    if doc
        .history
        .apply(&mut doc.font, &glyph, &master_id, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, &glyph, &master_id)
}

/// Replaces a layer's manual PostScript hints (empty restores the auto stems).
/// One undoable step.
#[tauri::command]
pub fn set_glyph_hints(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    hints: Vec<Hint>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let next = kernest_core::hinting::set_glyph_hints(&doc.font, &glyph, &master_id, hints)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, &glyph, &master_id)
}

/// Path › Correct Path Direction ("correct-direction") or Add Extremes ("add-extremes"),
/// as one undoable step. The change is worked out on `master_id`'s layer and, when the
/// glyph's masters are compatible, applied to every master the same way so they stay
/// compatible. `paths` limits Add Extremes to those paths.
#[tauri::command]
pub fn path_operation(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    operation: String,
    paths: Option<Vec<usize>>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let g = doc
        .font
        .glyph(&glyph)
        .ok_or_else(|| format!("No glyph named \"{glyph}\""))?;
    let current = g
        .layer(&master_id)
        .ok_or_else(|| format!("No layer for \"{glyph}\""))?;
    // On a master layer of a compatible glyph, every master and brace layer changes the
    // same way so they stay compatible. Bracket layers are a separate design, and are
    // only changed when edited themselves.
    let all_masters = operation != "remove-overlap"
        && current.is_master()
        && doc.font.masters.len() > 1
        && check_glyph(&doc.font, &glyph).is_empty();
    let targets: Vec<String> = if all_masters {
        g.layers
            .iter()
            .filter(|l| l.is_master() || l.location.is_some())
            .map(|l| l.key().to_string())
            .collect()
    } else {
        vec![master_id.clone()]
    };
    let mut next = doc.font.clone();
    let ng = next.glyph_mut(&glyph).expect("glyph exists");
    match operation.as_str() {
        "correct-direction" => {
            let plan = pathops::direction_plan(current);
            for id in &targets {
                if let Some(l) = ng.layer_mut(id) {
                    pathops::apply_direction(l, &plan);
                }
            }
        }
        "add-extremes" => {
            let plan = pathops::extremes_plan(current, paths.as_deref().filter(|p| !p.is_empty()));
            for id in &targets {
                if let Some(l) = ng.layer_mut(id) {
                    pathops::apply_extremes(l, &plan);
                }
            }
        }
        "remove-overlap" => {
            if let Some(l) = ng.layer_mut(&master_id) {
                kernest_core::overlap::remove_overlaps(l);
            }
        }
        other => return Err(format!("Unknown path operation \"{other}\"")),
    }
    let changed = if targets.len() == 1 {
        let layer = ng.layer(&master_id).expect("layer exists").clone();
        doc.history
            .apply(&mut doc.font, &glyph, &master_id, layer)
            .map_err(|e| e.to_string())?
    } else {
        doc.history
            .apply_font(&mut doc.font, next)
            .map_err(|e| e.to_string())?
    };
    if changed {
        doc.revision += 1;
    }
    layer_view_of(doc, &glyph, &master_id)
}

/// A path edit's outcome: the refreshed layer, plus a note when the change could
/// only be applied to the current master.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathEditResult {
    layer: LayerView,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

/// What a path edit leaves behind: per path, the closed flag and the node kinds.
/// Layers with the same structure still interpolate.
fn edit_structure(layer: &Layer) -> Vec<(bool, Vec<NodeKind>)> {
    layer
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some((p.closed, p.nodes.iter().map(|n| n.kind).collect())),
            _ => None,
        })
        .collect()
}

fn apply_path_edit(
    layer: &mut Layer,
    operation: &str,
    paths: Option<&[usize]>,
    nodes: &[[usize; 2]],
    points: &[[f64; 2]],
) -> CmdResult<bool> {
    let only = paths.filter(|p| !p.is_empty());
    match operation {
        "knife" => {
            let [p0, p1] = points else {
                return Err("The knife needs two points.".into());
            };
            Ok(pathops::cut(layer, (p0[0], p0[1]), (p1[0], p1[1])))
        }
        "open-corner" => {
            let [n] = nodes else {
                return Err("Pick a node.".into());
            };
            pathops::open_corner(layer, n[0], n[1])?;
            Ok(true)
        }
        "reconnect-nodes" => Ok(pathops::reconnect_nodes(layer, only) > 0),
        "connect-nodes" => {
            let [a, b] = nodes else {
                return Err("Pick two nodes.".into());
            };
            pathops::connect_nodes(layer, (a[0], a[1]), (b[0], b[1]))?;
            Ok(true)
        }
        "close-open-paths" => Ok(pathops::close_open_paths(layer, only) > 0),
        "make-node-first" => {
            let [n] = nodes else {
                return Err("Pick a node.".into());
            };
            pathops::make_node_first(layer, n[0], n[1])?;
            Ok(true)
        }
        "reverse-contours" => Ok(pathops::reverse_contours(layer, only) > 0),
        "tidy-up" => Ok(pathops::tidy_up(layer, only) > 0),
        "round-coordinates" => Ok(pathops::round_coordinates(layer, only)),
        // Picked nodes, or every smooth node when none are picked.
        "harmonize" => Ok(kernest_core::harmonize::harmonize(layer, nodes) > 0),
        // Boolean operations on the picked paths (SMO-41).
        "union" => kernest_core::booleans::apply(layer, kernest_core::booleans::Op::Union, only),
        "subtract" => {
            kernest_core::booleans::apply(layer, kernest_core::booleans::Op::Subtract, only)
        }
        "intersect" => {
            kernest_core::booleans::apply(layer, kernest_core::booleans::Op::Intersect, only)
        }
        "super-smooth" => {
            if nodes.is_empty() {
                return Err("Pick a node.".into());
            }
            Ok(kernest_core::harmonize::toggle_super_smooth(layer, nodes)? > 0)
        }
        other => Err(format!("Unknown path operation \"{other}\"")),
    }
}

/// The node-context-menu path edits (knife, open corner, reconnect, connect, close
/// open paths, make node first, reverse contours, tidy up, round coordinates) as one
/// undoable step. On a master layer of a compatible glyph the edit is tried on every
/// master and brace layer; if the results would no longer interpolate, only the
/// current master changes and the result says so.
fn path_edit_in(
    doc: &mut Document,
    glyph: &str,
    master_id: &str,
    operation: &str,
    paths: Option<&[usize]>,
    nodes: &[[usize; 2]],
    points: &[[f64; 2]],
) -> CmdResult<PathEditResult> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    let g = doc
        .font
        .glyph(glyph)
        .ok_or_else(|| format!("No glyph named \"{glyph}\""))?;
    let current = g
        .layer(master_id)
        .ok_or_else(|| format!("No layer for \"{glyph}\""))?;
    // Validate on the current layer first, so a bad pick errors before anything moves.
    let mut probe = current.clone();
    apply_path_edit(&mut probe, operation, paths, nodes, points)?;

    let all_masters = current.is_master()
        && doc.font.masters.len() > 1
        && check_glyph(&doc.font, glyph).is_empty();
    let targets: Vec<String> = if all_masters {
        g.layers
            .iter()
            .filter(|l| l.is_master() || l.location.is_some())
            .map(|l| l.key().to_string())
            .collect()
    } else {
        vec![master_id.to_string()]
    };
    // Every target's layer after the edit, whether it changed or not: an untouched
    // master still has to match the changed ones for the glyph to interpolate.
    let mut after: Vec<(String, Layer, bool)> = Vec::new();
    let mut broke = false;
    for id in &targets {
        let mut layer = g
            .layer(id)
            .ok_or_else(|| format!("No layer for \"{glyph}\""))?
            .clone();
        match apply_path_edit(&mut layer, operation, paths, nodes, points) {
            Ok(changed) => after.push((id.clone(), layer, changed)),
            // A pick that is valid here may not be on another master (a different
            // node type, say); keep the current master's edit and say so.
            Err(_) if id != master_id => broke = true,
            Err(e) => return Err(e),
        }
    }
    let compatible = !broke
        && after
            .iter()
            .map(|(_, l, _)| edit_structure(l))
            .all(|s| s == edit_structure(&after[0].1));
    let warning = if targets.len() > 1 && !compatible {
        Some("Applied to this master only to keep the others compatible.".to_string())
    } else if matches!(operation, "union" | "subtract" | "intersect")
        && kernest_core::booleans::open_picks(current, paths) > 0
    {
        Some("Open paths were left out.".to_string())
    } else {
        None
    };
    let kept: Vec<(String, Layer)> = after
        .into_iter()
        .filter(|(id, _, changed)| *changed && (compatible || id == master_id))
        .map(|(id, l, _)| (id, l))
        .collect();
    let changed = if kept.len() == 1 && kept[0].0 == master_id {
        doc.history
            .apply(
                &mut doc.font,
                glyph,
                master_id,
                kept.into_iter().next().expect("one").1,
            )
            .map_err(|e| e.to_string())?
    } else if !kept.is_empty() {
        let mut next = doc.font.clone();
        let ng = next.glyph_mut(glyph).expect("glyph exists");
        for (id, layer) in kept {
            *ng.layer_mut(&id).expect("layer exists") = layer;
        }
        doc.history
            .apply_font(&mut doc.font, next)
            .map_err(|e| e.to_string())?
    } else {
        false
    };
    if changed {
        doc.revision += 1;
    }
    Ok(PathEditResult {
        layer: layer_view_of(doc, glyph, master_id)?,
        warning,
    })
}

/// A name for Component from Selection to suggest (`_part.<glyph>`).
#[tauri::command]
pub fn suggest_component_name(state: State<AppState>, glyph: String) -> CmdResult<String> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    Ok(kernest_core::componentize::suggest_name(&doc.font, &glyph))
}

/// Component from Selection: the selected paths and components of `glyph` become a
/// new glyph `name`, replaced by a component of it in every master. One undoable step.
#[tauri::command]
pub fn component_from_selection(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    selection: kernest_core::transform::Selection,
    name: String,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let next = kernest_core::componentize::component_from_selection(
        &doc.font, &glyph, &master_id, &selection, &name,
    )?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, &glyph, &master_id)
}

#[tauri::command]
pub fn path_edit(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    operation: String,
    paths: Option<Vec<usize>>,
    nodes: Option<Vec<[usize; 2]>>,
    points: Option<Vec<[f64; 2]>>,
) -> CmdResult<PathEditResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    path_edit_in(
        doc,
        &glyph,
        &master_id,
        &operation,
        paths.as_deref(),
        &nodes.unwrap_or_default(),
        &points.unwrap_or_default(),
    )
}

/// Locks or unlocks one path of a layer (the path index counts paths only, not
/// components). A locked path refuses every edit until unlocked. One undoable step.
fn set_path_locked_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    path: usize,
    locked: bool,
) -> CmdResult<LayerView> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    let mut next = doc
        .font
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or_else(|| format!("No layer for \"{glyph}\""))?
        .clone();
    let p = next
        .shapes
        .iter_mut()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .nth(path)
        .ok_or_else(|| "No path at that position.".to_string())?;
    p.locked = locked;
    if doc
        .history
        .apply(&mut doc.font, glyph, key, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, glyph, key)
}

/// One entry of a layer's shape order.
#[derive(Serialize)]
pub struct ShapeRef {
    kind: &'static str,
    index: usize,
}

/// Paths the engine draws: stroked ones and ones with corner, cap or segment
/// components (counting paths only).
fn engine_paths(layer: &Layer) -> Vec<usize> {
    let mut v: Vec<usize> = layer
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p.stroke.is_some()),
            _ => None,
        })
        .enumerate()
        .filter(|(_, stroked)| *stroked)
        .map(|(i, _)| i)
        .chain(layer.parts.iter().map(|p| p.path))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The outlines of [`engine_paths`], with parts applied and strokes expanded, plus
/// the extra paths corner components bring.
fn engine_svg(font: &Font, layer: &Layer) -> String {
    let paths = engine_paths(layer);
    if paths.is_empty() {
        return String::new();
    }
    let own = layer
        .shapes
        .iter()
        .filter(|s| matches!(s, Shape::Path(_)))
        .count();
    let applied = kernest_core::parts::apply(font, layer);
    let mut only = Layer::new(layer.master_id.clone(), layer.width);
    only.shapes = applied
        .shapes
        .iter()
        .filter(|s| matches!(s, Shape::Path(_)))
        .enumerate()
        .filter(|(i, _)| paths.contains(i) || *i >= own)
        .map(|(_, s)| s.clone())
        .collect();
    kernest_core::geometry::svg_path(font, &only)
}

/// Makes the `path`-th path a mask (or not), in every master.
#[tauri::command]
pub fn set_path_mask(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    path: usize,
    mask: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    stroke_edit(doc, &glyph, &master_id, |f| {
        kernest_core::masking::set_mask(f, &glyph, &master_id, path, mask)
    })
}

/// What the HOI tool shows for one master layer and axis: where each node sits in
/// the next master down and up the axis, and the layer's interpolation curves.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoiView {
    /// Axes with a master further up or down from this one.
    axes: Vec<String>,
    axis: String,
    up_name: Option<String>,
    down_name: Option<String>,
    /// Node positions per path (paths only), when that master's layer is compatible.
    up: Option<PathNodes>,
    down: Option<PathNodes>,
    curves: Vec<kernest_core::hoi::HoiCurve>,
}

/// Node positions per path.
type PathNodes = Vec<Vec<[f64; 2]>>;

fn path_nodes(l: &kernest_core::Layer) -> PathNodes {
    l.shapes
        .iter()
        .filter_map(|s| match s {
            kernest_core::Shape::Path(p) => Some(p.nodes.iter().map(|n| [n.x, n.y]).collect()),
            _ => None,
        })
        .collect()
}

/// The HOI tool's view of a master layer along `axis` (the first axis that has a
/// neighbour when `axis` is empty or has none).
#[tauri::command]
pub fn hoi_view(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    axis: String,
) -> CmdResult<HoiView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    hoi_view_in(&doc.font, &glyph, &master_id, axis)
}

fn hoi_view_in(
    f: &kernest_core::Font,
    glyph: &str,
    master_id: &str,
    axis: String,
) -> CmdResult<HoiView> {
    use kernest_core::hoi::{next_down, next_up};
    let g = f.glyph(glyph).ok_or("That glyph doesn't exist.")?;
    let layer = g.layer(master_id).ok_or("That layer doesn't exist.")?;
    let axes: Vec<String> = f
        .axes
        .iter()
        .filter(|a| {
            next_up(f, &layer.master_id, &a.tag).is_some()
                || next_down(f, &layer.master_id, &a.tag).is_some()
        })
        .map(|a| a.tag.clone())
        .collect();
    let axis = if axes.contains(&axis) {
        axis
    } else {
        axes.first().cloned().unwrap_or_default()
    };
    let mine = path_nodes(layer);
    let shape_of = |m: Option<&kernest_core::Master>| -> (Option<String>, Option<PathNodes>) {
        let Some(m) = m else { return (None, None) };
        let nodes = g
            .layer(&m.id)
            .filter(|l| l.is_master())
            .map(path_nodes)
            .filter(|n| {
                n.len() == mine.len() && n.iter().zip(&mine).all(|(a, b)| a.len() == b.len())
            });
        (Some(m.name.clone()), nodes)
    };
    let (up_name, up) = shape_of(next_up(f, &layer.master_id, &axis));
    let (down_name, down) = shape_of(next_down(f, &layer.master_id, &axis));
    Ok(HoiView {
        curves: layer
            .hoi
            .iter()
            .filter(|c| c.axis == axis)
            .cloned()
            .collect(),
        axes,
        axis,
        up_name,
        down_name,
        up,
        down,
    })
}

/// A glyph that uses another as a component, for "Glyphs Using…" (Handbook 9.1.5).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphUse {
    name: String,
    /// It has the component itself, not only through another component.
    direct: bool,
}

/// The glyphs that use `glyph` as a component, directly or nested, in font order.
#[tauri::command]
pub fn glyphs_using(state: State<AppState>, glyph: String) -> CmdResult<Vec<GlyphUse>> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    if doc.font.glyph(&glyph).is_none() {
        return Err(format!("There is no glyph named \"{glyph}\"."));
    }
    Ok(kernest_core::edit::glyphs_using(&doc.font, &glyph)
        .into_iter()
        .map(|u| GlyphUse {
            name: u.name,
            direct: u.direct,
        })
        .collect())
}

/// Sets or (with no kind) removes a node's interpolation curve along an axis.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_hoi(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    axis: String,
    path: usize,
    node: usize,
    kind: Option<kernest_core::hoi::HoiKind>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    stroke_edit(doc, &glyph, &master_id, |f| {
        kernest_core::hoi::set_curve(f, &glyph, &master_id, &axis, path, node, kind)
    })
}

/// Shape Order: puts the layer's shapes (paths and components) in a new order.
#[tauri::command]
pub fn reorder_shapes(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    order: Vec<usize>,
    all_layers: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    stroke_edit(doc, &glyph, &master_id, |f| {
        kernest_core::masking::reorder_shapes(f, &glyph, &master_id, &order, all_layers)
    })
}

/// Places a corner, cap or segment component on a path node, in every master.
#[tauri::command]
pub fn add_path_part(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    part: kernest_core::parts::PathPart,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    stroke_edit(doc, &glyph, &master_id, |f| {
        kernest_core::parts::add_part(f, &glyph, &master_id, part)
    })
}

/// Changes or (with `None`) removes one of the layer's parts, in every master.
#[tauri::command]
pub fn set_path_part(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    index: usize,
    part: Option<kernest_core::parts::PathPart>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    stroke_edit(doc, &glyph, &master_id, |f| {
        kernest_core::parts::set_part(f, &glyph, &master_id, index, part)
    })
}

/// Decompose: the glyph's corner, cap and segment components become outline.
#[tauri::command]
pub fn decompose_parts(
    state: State<AppState>,
    glyph: String,
    master_id: String,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    stroke_edit(doc, &glyph, &master_id, |f| {
        kernest_core::parts::decompose_parts(f, &glyph)
    })
}

/// Applies a whole-glyph stroke change and returns the shown layer.
fn stroke_edit(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    f: impl FnOnce(&Font) -> Result<Font, String>,
) -> CmdResult<LayerView> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    let next = f(&doc.font)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, glyph, key)
}

/// Adds the `pixel` glyph the pixel tool draws with: a `size` square in every master.
#[tauri::command]
pub fn add_pixel_glyph(state: State<AppState>, size: f64) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let next = kernest_core::pixel::add_pixel_glyph(&doc.font, size)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// Pixel tool: turns the pixels at `cells` (bottom-left corners) on or off in one
/// layer, as one undoable step.
#[tauri::command]
pub fn set_pixels(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    cells: Vec<[f64; 2]>,
    on: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let cells: Vec<(f64, f64)> = cells.iter().map(|c| (c[0], c[1])).collect();
    let next = kernest_core::pixel::set_pixels(&doc.font, &glyph, &master_id, &cells, on)?;
    let layer = next
        .glyph(&glyph)
        .and_then(|g| g.layer(&master_id))
        .ok_or("That layer doesn't exist.")?
        .clone();
    if doc
        .history
        .apply(&mut doc.font, &glyph, &master_id, layer)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, &glyph, &master_id)
}

/// Strokes the `path`-th path (or fills it again with `None`), in every master.
#[tauri::command]
pub fn set_path_stroke(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    path: usize,
    stroke: Option<kernest_core::stroke::PathStroke>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    stroke_edit(doc, &glyph, &master_id, |f| {
        kernest_core::stroke::set_stroke(f, &glyph, &master_id, path, stroke)
    })
}

/// Sets or clears the pen width at an on-curve node of a stroked path.
#[tauri::command]
pub fn set_pen_width(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    path: usize,
    node: usize,
    width: Option<f64>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    stroke_edit(doc, &glyph, &master_id, |f| {
        kernest_core::stroke::set_pen_width(f, &glyph, &master_id, path, node, width)
    })
}

/// Expand Outline: the glyph's stroked paths become outlines, in every layer.
#[tauri::command]
pub fn expand_strokes(
    state: State<AppState>,
    glyph: String,
    master_id: String,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    stroke_edit(doc, &glyph, &master_id, |f| {
        kernest_core::stroke::expand_glyph(f, &glyph)
    })
}

#[tauri::command]
pub fn set_path_locked(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    path: usize,
    locked: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    set_path_locked_in(doc, &glyph, &master_id, path, locked)
}

/// Locks or unlocks a whole glyph: a locked glyph refuses every edit and is skipped
/// by filters and Update Metrics. One undoable step.
fn set_glyph_locked_in(doc: &mut Document, glyph: &str, locked: bool) -> CmdResult<DocStatus> {
    let mut next = doc.font.clone();
    next.glyph_mut(glyph)
        .ok_or_else(|| format!("No glyph named \"{glyph}\""))?
        .locked = locked;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

#[tauri::command]
pub fn set_glyph_locked(
    state: State<AppState>,
    glyph: String,
    locked: bool,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    set_glyph_locked_in(doc, &glyph, locked)
}

/// A filter's settings; every field is optional so each dialog only sends its own.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterParams {
    pub radius: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub make_stroke: bool,
    pub affine: [f64; 6],
    pub origin: [f64; 2],
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            radius: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            make_stroke: false,
            affine: kernest_core::IDENTITY,
            origin: [0.0, 0.0],
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterResult {
    changed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    layer: LayerView,
}

/// Everything a transform filter moves: all nodes of unlocked paths, all components
/// and all anchors. Locked paths stay exactly where they are.
fn unlocked_selection(layer: &Layer) -> Selection {
    let mut sel = Selection::default();
    let mut pi = 0usize;
    let mut ci = 0usize;
    for s in &layer.shapes {
        match s {
            Shape::Path(p) => {
                if !p.locked {
                    sel.nodes.extend((0..p.nodes.len()).map(|ni| (pi, ni)));
                }
                pi += 1;
            }
            Shape::Component { .. } => {
                sel.components.push(ci);
                ci += 1;
            }
        }
    }
    sel.anchors = layer.anchors.iter().map(|a| a.name.clone()).collect();
    sel
}

/// Checks a filter's name and numbers before any work is done.
fn validate_filter(name: &str, params: &FilterParams) -> CmdResult<()> {
    match name {
        "round-corners" => {
            if !params.radius.is_finite() || params.radius < 0.0 {
                return Err("The radius has an invalid number.".into());
            }
        }
        "offset-curve" => {
            if !params.offset_x.is_finite() || !params.offset_y.is_finite() {
                return Err("The offset has an invalid number.".into());
            }
        }
        "transformations" => {
            if params
                .affine
                .iter()
                .chain(params.origin.iter())
                .any(|v| !v.is_finite())
            {
                return Err("The transform has an invalid number.".into());
            }
        }
        other => return Err(format!("Unknown filter \"{other}\"")),
    }
    Ok(())
}

/// Applies a filter to one layer clone, the way `run_filter_in` does per layer.
fn apply_filter(layer: &mut Layer, name: &str, params: &FilterParams) -> bool {
    match name {
        "round-corners" => filters::round_corners(layer, params.radius, None) > 0,
        "offset-curve" => filters::offset_curve(
            layer,
            params.offset_x,
            params.offset_y,
            params.make_stroke,
            None,
        ),
        _ => {
            let sel = unlocked_selection(layer);
            if sel.is_empty() {
                false
            } else {
                let before = layer.clone();
                transform::apply(
                    layer,
                    &sel,
                    &params.affine,
                    (params.origin[0], params.origin[1]),
                );
                *layer != before
            }
        }
    }
}

/// The filter's result on the current layer without touching the document: the
/// dialogs' live preview. Locked glyphs preview unchanged, since filters skip them.
fn filter_preview_in(
    doc: &Document,
    glyph: &str,
    master_id: &str,
    name: &str,
    params: &FilterParams,
) -> CmdResult<LayerView> {
    validate_filter(name, params)?;
    let g = doc
        .font
        .glyph(glyph)
        .ok_or_else(|| format!("No glyph named \"{glyph}\""))?;
    if g.locked {
        return layer_view_of(doc, glyph, master_id);
    }
    let mut layer = g
        .layer(master_id)
        .ok_or_else(|| format!("\"{glyph}\" has no layer \"{master_id}\""))?
        .clone();
    apply_filter(&mut layer, name, params);
    let mut font = doc.font.clone();
    *font
        .glyph_mut(glyph)
        .expect("glyph exists")
        .layer_mut(master_id)
        .expect("layer exists") = layer;
    layer_view_font(doc, &font, glyph, master_id)
}

/// Runs a filter ("round-corners", "offset-curve", "transformations") over the chosen
/// glyphs (the current one when none are chosen) as a single undoable step. With
/// `all_masters`, compatible glyphs change on every master and brace layer; where a
/// change would break interpolation only the current master changes and a warning
/// says so. Locked glyphs are skipped.
fn run_filter_in(
    doc: &mut Document,
    current: &str,
    master_id: &str,
    glyphs: &[String],
    name: &str,
    params: &FilterParams,
    all_masters: bool,
) -> CmdResult<FilterResult> {
    validate_filter(name, params)?;
    let mut scope = if glyphs.is_empty() {
        vec![current.to_string()]
    } else {
        glyphs.to_vec()
    };
    let mut seen = std::collections::BTreeSet::new();
    scope.retain(|n| seen.insert(n.clone()));
    // The layer key the dialogs were opened on; other glyphs fall back to the same
    // master, since a brace layer belongs to one glyph only.
    let base_master = doc
        .font
        .glyph(current)
        .and_then(|g| g.layer(master_id))
        .map(|l| l.master_id.clone())
        .unwrap_or_else(|| master_id.to_string());
    let mut next = doc.font.clone();
    let mut changed: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut skipped_locked = false;
    let mut limited = false;
    for gname in &scope {
        let g = next
            .glyph(gname)
            .ok_or_else(|| format!("No glyph named \"{gname}\""))?;
        if g.locked {
            skipped_locked = true;
            continue;
        }
        let compatible =
            all_masters && next.masters.len() > 1 && check_glyph(&next, gname).is_empty();
        let keys: Vec<String> = if compatible {
            g.layers
                .iter()
                .filter(|l| l.is_master() || l.location.is_some())
                .map(|l| l.key().to_string())
                .collect()
        } else if g.layer(master_id).is_some() {
            vec![master_id.to_string()]
        } else {
            vec![base_master.clone()]
        };
        let mut after: Vec<(String, Layer, bool)> = Vec::new();
        for key in &keys {
            let mut layer = g
                .layer(key)
                .ok_or_else(|| format!("No layer for \"{gname}\""))?
                .clone();
            let did = apply_filter(&mut layer, name, params);
            after.push((key.clone(), layer, did));
        }
        // Transformations keep every node, so they never break interpolation.
        let stays_compatible = name == "transformations"
            || after
                .iter()
                .map(|(_, l, _)| edit_structure(l))
                .all(|s| s == edit_structure(&after[0].1));
        let base_key = if g.layer(master_id).is_some() {
            master_id
        } else {
            &base_master
        };
        if keys.len() > 1 && !stays_compatible {
            limited = true;
        }
        let mut any = false;
        let ng = next.glyph_mut(gname).expect("glyph exists");
        for (key, layer, did) in after {
            if did && (stays_compatible || key == base_key) {
                *ng.layer_mut(&key).expect("layer exists") = layer;
                any = true;
            }
        }
        if any {
            changed.push(gname.clone());
        }
    }
    if limited {
        warnings.push(
            "Applied to the current master only where masters would become incompatible.".into(),
        );
    }
    if skipped_locked {
        warnings.push("Locked glyphs were skipped.".into());
    }
    if !changed.is_empty()
        && doc
            .history
            .apply_font(&mut doc.font, next)
            .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(FilterResult {
        changed,
        warnings,
        layer: layer_view_of(doc, current, master_id)?,
    })
}

#[tauri::command]
pub fn run_filter(
    state: State<AppState>,
    current: String,
    master_id: String,
    glyphs: Vec<String>,
    name: String,
    params: FilterParams,
    all_masters: bool,
) -> CmdResult<FilterResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    run_filter_in(
        doc,
        &current,
        &master_id,
        &glyphs,
        &name,
        &params,
        all_masters,
    )
}

#[tauri::command]
pub fn filter_preview(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    name: String,
    params: FilterParams,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    filter_preview_in(doc, &glyph, &master_id, &name, &params)
}

/// Sets (or clears, with an empty key) a layer's metrics key for "lsb", "rsb" or
/// "width", and applies it right away. One undoable step.
#[tauri::command]
pub fn set_metrics_key(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    metric: String,
    key: Option<String>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let key = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    if let Some(k) = &key {
        kernest_core::metricskeys::validate(k)?;
    }
    let mut next = doc.font.clone();
    {
        let layer = next
            .glyph_mut(&glyph)
            .and_then(|g| g.layer_mut(&master_id))
            .ok_or_else(|| format!("No layer for \"{glyph}\""))?;
        let slot = match metric.as_str() {
            "lsb" => &mut layer.metrics_keys.left,
            "rsb" => &mut layer.metrics_keys.right,
            "width" => &mut layer.metrics_keys.width,
            other => return Err(format!("Unknown metric \"{other}\"")),
        };
        *slot = key;
    }
    if let Some(l) = kernest_core::metricskeys::apply_layer(&next, &glyph, &master_id)? {
        *next
            .glyph_mut(&glyph)
            .and_then(|g| g.layer_mut(&master_id))
            .expect("layer exists") = l;
    }
    let layer = next
        .glyph(&glyph)
        .and_then(|g| g.layer(&master_id))
        .expect("layer exists")
        .clone();
    if doc
        .history
        .apply(&mut doc.font, &glyph, &master_id, layer)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, &glyph, &master_id)
}

/// Sets the smart values of component `index` (among the layer's components).
#[tauri::command]
pub fn set_component_smart(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    index: usize,
    values: BTreeMap<String, f64>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let next =
        kernest_core::smart::set_component_values(&doc.font, &glyph, &master_id, index, &values)?;
    if doc
        .history
        .apply(&mut doc.font, &glyph, &master_id, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, &glyph, &master_id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetricsResult {
    /// Problems with keys that couldn't be applied (missing glyphs, loops).
    problems: Vec<String>,
    status: DocStatus,
}

/// Glyph › Update Metrics: applies every metrics key in the font (one undoable step).
#[tauri::command]
pub fn update_metrics(state: State<AppState>) -> CmdResult<UpdateMetricsResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let (next, problems) = kernest_core::metricskeys::apply_all(&doc.font);
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(UpdateMetricsResult {
        problems,
        status: status(doc),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    status: DocStatus,
}

/// Font View › Batch: applies a path operation to every selected glyph in one undoable
/// step, across compatible masters just like the single-glyph command.
#[tauri::command]
pub fn batch_path_operation(
    state: State<AppState>,
    glyphs: Vec<String>,
    master_id: String,
    operation: String,
) -> CmdResult<BatchResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    for name in &glyphs {
        ensure_glyph_unlocked(&doc.font, name)?;
    }
    let next = kernest_core::batch::path_operation(&doc.font, &glyphs, &master_id, &operation)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(BatchResult {
        status: status(doc),
    })
}

/// Font View › Batch › Update Metrics: applies metrics keys only to the selected glyphs
/// (one undoable step); referenced glyphs outside the selection keep their metrics.
#[tauri::command]
pub fn update_metrics_for(
    state: State<AppState>,
    glyphs: Vec<String>,
) -> CmdResult<UpdateMetricsResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let (next, problems) = kernest_core::metricskeys::apply_some(&doc.font, Some(&glyphs));
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(UpdateMetricsResult {
        problems,
        status: status(doc),
    })
}

/// Edit › Copy Glyph Names / Paste Special: the named glyphs with every master's layer,
/// ready to put on the clipboard as JSON.
#[tauri::command]
pub fn copy_glyphs(
    state: State<AppState>,
    names: Vec<String>,
) -> CmdResult<Vec<kernest_core::model::Glyph>> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let mut out = Vec::new();
    for n in &names {
        out.push(
            doc.font
                .glyph(n)
                .ok_or_else(|| format!("There is no glyph named \"{n}\"."))?
                .clone(),
        );
    }
    Ok(out)
}

/// Edit › Paste Special: pastes copied glyphs into same-named glyphs, onto the selection
/// in order, or as components of the selection — one undoable step.
#[tauri::command]
pub fn paste_special(
    state: State<AppState>,
    mode: String,
    targets: Vec<String>,
    glyphs: Vec<kernest_core::model::Glyph>,
) -> CmdResult<BatchResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let next = match mode.as_str() {
        "same-name" => kernest_core::clipboard::paste_same_named(&doc.font, &glyphs)?,
        "selection" => kernest_core::clipboard::paste_into_selection(&doc.font, &targets, &glyphs)?,
        "components" => kernest_core::clipboard::paste_as_components(&doc.font, &targets, &glyphs)?,
        other => return Err(format!("Unknown paste mode \"{other}\"")),
    };
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(BatchResult {
        status: status(doc),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    /// "layer" | "kerning" | "groups"
    kind: &'static str,
    glyph: Option<String>,
    master_id: Option<String>,
    status: DocStatus,
}

fn step(state: &AppState, redo: bool) -> CmdResult<Option<UndoResult>> {
    let mut guard = lock(state)?;
    let doc = doc_mut(&mut guard)?;
    let hit = if redo {
        doc.history.redo(&mut doc.font)
    } else {
        doc.history.undo(&mut doc.font)
    };
    Ok(hit.map(|a| {
        doc.revision += 1;
        let (kind, glyph, master_id) = match a {
            Affected::Layer { glyph, master_id } => ("layer", Some(glyph), Some(master_id)),
            Affected::Kerning { master_id } => ("kerning", None, Some(master_id)),
            Affected::Groups { glyph } => ("groups", Some(glyph), None),
            Affected::Font => ("font", None, None),
        };
        UndoResult {
            kind,
            glyph,
            master_id,
            status: status(doc),
        }
    }))
}

/// Picks the undo history Undo and Redo use (SMO-83): a glyph's own while its
/// Edit View tab is in front, the font's otherwise. Returns the new status.
#[tauri::command]
pub fn set_undo_context(state: State<AppState>, glyph: Option<String>) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    doc.history.set_context(match glyph {
        Some(g) if doc.font.glyph(&g).is_some() => kernest_core::edit::Scope::Glyph(g),
        _ => kernest_core::edit::Scope::Font,
    });
    Ok(status(doc))
}

#[tauri::command]
pub fn undo(state: State<AppState>) -> CmdResult<Option<UndoResult>> {
    step(&state, false)
}

#[tauri::command]
pub fn redo(state: State<AppState>) -> CmdResult<Option<UndoResult>> {
    step(&state, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Handbook 10.9.1 (Node Visualization): the Interpolation curves tool shows each
    /// node in the next master up and down the axis, and only where they match.
    #[test]
    fn interpolation_curves_view_shows_the_neighbouring_masters() {
        use kernest_core::hoi::HoiKind;
        use kernest_core::{structure, Axis, Shape};
        let wght = Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 100.0,
            default: 400.0,
            max: 900.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        };
        let f = structure::set_axes(&sample::sample_font(), vec![wght]).unwrap();
        let at = |w: f64| std::collections::BTreeMap::from([("wght".to_string(), w)]);
        let (f, thin) = structure::add_master(&f, "m01", "Thin", &at(100.0)).unwrap();
        let (mut f, black) = structure::add_master(&f, "m01", "Black", &at(900.0)).unwrap();
        // H's first node sits 40 units further right in Black.
        if let Some(Shape::Path(p)) = f
            .glyph_mut("H")
            .unwrap()
            .layer_mut(&black)
            .unwrap()
            .shapes
            .first_mut()
        {
            p.nodes[0].x += 40.0;
        }
        let regular = f.glyph("H").unwrap().layer("m01").unwrap().clone();
        let mine = path_nodes(&regular);

        // An unknown axis falls back to the first axis with a neighbour.
        let v = hoi_view_in(&f, "H", "m01", "XXXX".into()).unwrap();
        assert_eq!(
            (v.axes.clone(), v.axis.as_str()),
            (vec!["wght".to_string()], "wght")
        );
        assert_eq!(v.up_name.as_deref(), Some("Black"));
        assert_eq!(v.down_name.as_deref(), Some("Thin"));
        let up = v.up.unwrap();
        assert_eq!(up[0][0], [mine[0][0][0] + 40.0, mine[0][0][1]]);
        assert_eq!(up[0][1..], mine[0][1..]);
        assert_eq!(v.down.unwrap(), mine);
        assert!(v.curves.is_empty());

        // The ends of the axis have one neighbour each.
        let v = hoi_view_in(&f, "H", &black, String::new()).unwrap();
        assert_eq!((v.up_name, v.down_name.as_deref()), (None, Some("Regular")));
        let v = hoi_view_in(&f, "H", &thin, String::new()).unwrap();
        assert_eq!(v.up_name.as_deref(), Some("Regular"));
        assert!(v.down_name.is_none());

        // An incompatible neighbour is named but shows no nodes.
        let mut g = f.clone();
        g.glyph_mut("H")
            .unwrap()
            .layer_mut(&thin)
            .unwrap()
            .shapes
            .pop();
        let v = hoi_view_in(&g, "H", "m01", String::new()).unwrap();
        assert_eq!(v.down_name.as_deref(), Some("Thin"));
        assert!(v.down.is_none());
        assert!(v.up.is_some());

        // Curves on this layer and axis come with the view.
        let kind = HoiKind::Through { x: 0.0, y: 0.0 };
        let f =
            kernest_core::hoi::set_curve(&f, "H", "m01", "wght", 0, 0, Some(kind.clone())).unwrap();
        let v = hoi_view_in(&f, "H", "m01", "wght".into()).unwrap();
        assert_eq!(v.curves.len(), 1);
        assert_eq!((v.curves[0].path, v.curves[0].node), (0, 0));
        assert_eq!(v.curves[0].kind, kind);
        assert!(hoi_view_in(&f, "nope", "m01", String::new()).is_err());
    }

    #[test]
    fn status_names_the_step_of_the_history_in_use() {
        // SMO-83: Edit View tabs use the glyph's own history; the status says what
        // Undo would do there.
        use kernest_core::edit::Scope;
        let mut doc = Document::new(sample::sample_font(), None);
        doc.history.set_context(Scope::Glyph("H".into()));
        let mut l = doc.font.glyph("H").unwrap().layer("m01").unwrap().clone();
        l.width += 10.0;
        doc.history.apply(&mut doc.font, "H", "m01", l).unwrap();
        let s = status(&doc);
        assert!(s.can_undo);
        assert_eq!(
            s.undo,
            Some(StepView {
                action: "Change Metrics",
                glyph: Some("H".into()),
                glyphs: 1
            })
        );
        doc.history.set_context(Scope::Glyph("A".into()));
        let s = status(&doc);
        assert!(!s.can_undo && s.undo.is_none());
        doc.history.set_context(Scope::Font);
        assert!(!status(&doc).can_undo);
    }

    #[test]
    fn glyph_hints_replace_auto_stems_and_undo() {
        let mut doc = Document::new(sample::sample_font(), None);
        let id = doc.font.masters[0].id.clone();
        let f = kernest_core::hinting::set_stems(
            &doc.font,
            &id,
            vec![
                kernest_core::Stem {
                    name: "h".into(),
                    horizontal: true,
                    value: 80.0,
                },
                kernest_core::Stem {
                    name: "v".into(),
                    horizontal: false,
                    value: 90.0,
                },
            ],
        )
        .unwrap();
        doc.history.apply_font(&mut doc.font, f).unwrap();
        let v = layer_view_of(&doc, "H", &id).unwrap();
        assert!(v.hints.is_empty());
        assert_eq!(v.auto_hints.len(), 3, "crossbar plus two uprights");

        let manual = vec![Hint {
            horizontal: true,
            position: 0.0,
            width: 0.0,
            ghost_top: false,
        }];
        let f =
            kernest_core::hinting::set_glyph_hints(&doc.font, "H", &id, manual.clone()).unwrap();
        doc.history.apply_font(&mut doc.font, f).unwrap();
        let v = layer_view_of(&doc, "H", &id).unwrap();
        assert_eq!(v.hints, manual);
        assert!(v.status.can_undo);
        doc.history.undo(&mut doc.font).unwrap();
        assert!(layer_view_of(&doc, "H", &id).unwrap().hints.is_empty());
    }

    fn node(x: f64, y: f64, kind: NodeKindDto) -> NodeDto {
        NodeDto {
            x,
            y,
            kind,
            smooth: false,
            g2: false,
        }
    }

    #[test]
    fn path_validation() {
        use NodeKindDto::*;
        let ok = PathDto {
            locked: false,
            stroke: None,
            mask: false,
            closed: true,
            nodes: vec![
                node(0., 0., Line),
                node(10., 0., Line),
                node(10., 10., Line),
            ],
        };
        assert!(to_path(&ok).is_ok());
        let only_off = PathDto {
            locked: false,
            stroke: None,
            mask: false,
            closed: true,
            nodes: vec![node(0., 0., Offcurve), node(1., 1., Offcurve)],
        };
        assert!(to_path(&only_off).is_err());
        let open_off = PathDto {
            locked: false,
            stroke: None,
            mask: false,
            closed: false,
            nodes: vec![node(0., 0., Offcurve), node(1., 1., Line)],
        };
        assert!(to_path(&open_off).is_err());
        let three = PathDto {
            locked: false,
            stroke: None,
            mask: false,
            closed: true,
            nodes: vec![
                node(0., 0., Line),
                node(1., 1., Offcurve),
                node(2., 2., Offcurve),
                node(3., 3., Offcurve),
                node(4., 4., Curve),
            ],
        };
        assert!(to_path(&three).is_err());
        let cubic = PathDto {
            locked: false,
            stroke: None,
            mask: false,
            closed: false,
            nodes: vec![
                node(0., 0., Line),
                node(1., 1., Offcurve),
                node(2., 2., Offcurve),
                node(4., 4., Curve),
            ],
        };
        assert!(to_path(&cubic).is_ok());
        let nan = PathDto {
            locked: false,
            stroke: None,
            mask: false,
            closed: false,
            nodes: vec![node(f64::NAN, 0., Line)],
        };
        assert!(to_path(&nan).is_err());
    }

    #[test]
    fn dirty_tracking_and_undo() {
        let mut doc = Document::new(sample::sample_font(), None);
        assert!(!doc.dirty());
        let next = with_metric(&doc.font, "H", "m01", MetricEdit::Width(700.0)).unwrap();
        assert!(doc.history.apply(&mut doc.font, "H", "m01", next).unwrap());
        doc.revision += 1;
        assert!(doc.dirty());
        let v = layer_view_of(&doc, "H", "m01").unwrap();
        assert_eq!(v.width, 700.0);
        assert!(v.status.can_undo && v.status.dirty);
        assert_eq!(v.paths.len(), 3);
        doc.saved_revision = doc.revision;
        assert!(!doc.dirty());
    }
}

#[cfg(test)]
mod component_tests {
    use super::*;

    #[test]
    fn component_validation() {
        let font = sample::sample_font();
        let ok = ComponentInput {
            base: "H".into(),
            transform: kernest_core::IDENTITY,
            smart: BTreeMap::new(),
            align: true,
        };
        assert!(to_component(&font, "A", ok).is_ok());
        let missing = ComponentInput {
            base: "nope".into(),
            transform: kernest_core::IDENTITY,
            smart: BTreeMap::new(),
            align: true,
        };
        assert!(to_component(&font, "A", missing).is_err());
        let selfref = ComponentInput {
            base: "A".into(),
            transform: kernest_core::IDENTITY,
            smart: BTreeMap::new(),
            align: true,
        };
        assert!(to_component(&font, "A", selfref).is_err());
        let bad = ComponentInput {
            base: "H".into(),
            transform: [1.0, 0.0, 0.0, 1.0, f64::INFINITY, 0.0],
            smart: BTreeMap::new(),
            align: true,
        };
        assert!(to_component(&font, "A", bad).is_err());
    }

    #[test]
    fn layer_view_lists_components_with_outlines() {
        let mut doc = Document::new(sample::sample_font(), None);
        let mut next = doc.font.glyph("A").unwrap().layer("m01").unwrap().clone();
        next.shapes
            .push(Shape::component("H", kernest_core::IDENTITY));
        doc.history.apply(&mut doc.font, "A", "m01", next).unwrap();
        let v = layer_view_of(&doc, "A", "m01").unwrap();
        assert_eq!(v.components.len(), 1);
        assert!(v.components[0].svg.starts_with('M'));
        assert_eq!(v.lsb, Some(80.0));
    }
}

#[cfg(test)]
mod path_edit_tests {
    use super::*;
    use kernest_core::model::Master;

    fn two_master_doc() -> Document {
        let mut f = sample::sample_font();
        let bold = f.masters[0].clone();
        f.masters.push(Master {
            id: "m02".into(),
            name: "Bold".into(),
            location: Default::default(),
            metrics: bold.metrics,
            guides: Vec::new(),
            alignment_zones: Vec::new(),
            stems: Vec::new(),
            vertical: Default::default(),
        });
        for g in &mut f.glyphs {
            let mut l = g.layers[0].clone();
            l.master_id = "m02".into();
            g.layers.push(l);
        }
        Document::new(f, None)
    }

    fn shorten_bold(doc: &mut Document, sy: f64) {
        let mut l = doc.font.glyph("H").unwrap().layer("m02").unwrap().clone();
        transform::apply(
            &mut l,
            &Selection::default(),
            &transform::scale(1.0, sy),
            (0.0, 0.0),
        );
        *doc.font.glyph_mut("H").unwrap().layer_mut("m02").unwrap() = l;
    }

    #[test]
    fn knife_is_one_undo_step() {
        let mut doc = Document::new(sample::sample_font(), None);
        let original = doc.font.clone();
        let before = layer_view_of(&doc, "H", "m01").unwrap().paths.len();
        let r = path_edit_in(
            &mut doc,
            "H",
            "m01",
            "knife",
            None,
            &[],
            &[[-50.0, 350.0], [750.0, 350.0]],
        )
        .unwrap();
        assert!(r.warning.is_none());
        assert!(r.layer.paths.len() > before);
        assert!(r.layer.status.can_undo && r.layer.status.dirty);
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }

    #[test]
    fn super_smooth_and_harmonize_are_path_edits() {
        let mut doc = Document::new(sample::sample_font(), None);
        let original = doc.font.clone();
        let v = layer_view_of(&doc, "O", "m01").unwrap();
        // A smooth node between two curves.
        let ni = v.paths[0]
            .nodes
            .iter()
            .enumerate()
            .position(|(i, n)| {
                n.smooth && n.kind == NodeKindDto::Curve && {
                    let p = to_path(&v.paths[0]).unwrap();
                    kernest_core::harmonize::g2_position(&p, i).is_some()
                }
            })
            .expect("a smooth curve node in O");
        let r = path_edit_in(&mut doc, "O", "m01", "super-smooth", None, &[[0, ni]], &[]).unwrap();
        assert!(r.layer.paths[0].nodes[ni].g2);
        // Every master gets the flag, so the glyph still interpolates.
        for m in &doc.font.masters {
            let l = layer_view_of(&doc, "O", &m.id).unwrap();
            assert!(l.paths[0].nodes[ni].g2, "{}", m.id);
        }
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
        // A handle can't be super-smooth; Harmonize with no picks takes every smooth node.
        assert!(path_edit_in(
            &mut doc,
            "O",
            "m01",
            "super-smooth",
            None,
            &[[0, ni + 1]],
            &[]
        )
        .is_err());
        assert!(path_edit_in(&mut doc, "O", "m01", "super-smooth", None, &[], &[]).is_err());
        path_edit_in(&mut doc, "O", "m01", "harmonize", None, &[], &[]).unwrap();
    }

    #[test]
    fn locked_paths_and_glyphs_refuse_edits() {
        let mut doc = Document::new(sample::sample_font(), None);
        let original = doc.font.clone();
        let v = set_path_locked_in(&mut doc, "H", "m01", 0, true).unwrap();
        assert!(v.paths[0].locked);
        // A knife through the glyph leaves the locked path whole.
        let before = v.paths[0].nodes.len();
        let r = path_edit_in(
            &mut doc,
            "H",
            "m01",
            "knife",
            None,
            &[],
            &[[-50.0, 350.0], [750.0, 350.0]],
        )
        .unwrap();
        assert_eq!(r.layer.paths[0].nodes.len(), before);
        assert!(r.layer.paths[0].locked);
        assert!(path_edit_in(&mut doc, "H", "m01", "open-corner", None, &[[0, 0]], &[]).is_err());
        assert!(set_path_locked_in(&mut doc, "H", "m01", 9, true).is_err());
        // A locked glyph refuses every edit, including unlocking its paths.
        set_glyph_locked_in(&mut doc, "H", true).unwrap();
        assert!(layer_view_of(&doc, "H", "m01").unwrap().glyph_locked);
        let fv = font_view_of(&doc, None).unwrap();
        assert!(fv.glyphs.iter().find(|g| g.name == "H").unwrap().locked);
        assert!(path_edit_in(&mut doc, "H", "m01", "tidy-up", None, &[], &[]).is_err());
        assert!(set_path_locked_in(&mut doc, "H", "m01", 0, false).is_err());
        set_glyph_locked_in(&mut doc, "H", false).unwrap();
        set_path_locked_in(&mut doc, "H", "m01", 0, false).unwrap();
        for _ in 0..5 {
            doc.history.undo(&mut doc.font);
        }
        assert_eq!(doc.font, original);
    }

    #[test]
    fn knife_falls_back_to_current_master_with_a_warning() {
        let mut doc = two_master_doc();
        // Same structure, but the bold master is shorter: the knife line misses it.
        shorten_bold(&mut doc, 0.4);
        let original = doc.font.clone();
        let bold_before = doc.font.glyph("H").unwrap().layer("m02").unwrap().clone();
        let r = path_edit_in(
            &mut doc,
            "H",
            "m01",
            "knife",
            None,
            &[],
            &[[-50.0, 350.0], [750.0, 350.0]],
        )
        .unwrap();
        assert_eq!(
            r.warning.as_deref(),
            Some("Applied to this master only to keep the others compatible.")
        );
        assert_eq!(
            doc.font.glyph("H").unwrap().layer("m02").unwrap(),
            &bold_before
        );
        assert!(r.layer.paths.len() > 3);
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }

    #[test]
    fn path_edit_rejects_bad_picks() {
        let mut doc = Document::new(sample::sample_font(), None);
        let original = doc.font.clone();
        assert!(path_edit_in(&mut doc, "H", "m01", "nope", None, &[], &[]).is_err());
        assert!(path_edit_in(&mut doc, "H", "m01", "knife", None, &[], &[]).is_err());
        assert!(path_edit_in(&mut doc, "H", "m01", "connect-nodes", None, &[[0, 0]], &[]).is_err());
        assert!(path_edit_in(&mut doc, "nope", "m01", "tidy-up", None, &[], &[]).is_err());
        assert_eq!(doc.font, original);
    }

    /// Handbook 5.5.4 (Filters): a filter over every drawn glyph, non-exporting ones
    /// included, is one Font View step; in a glyph's tab Undo takes back only its part.
    #[test]
    fn filter_over_a_large_selection_undoes_per_glyph_and_as_one_step() {
        let mut font = sample::sample_font();
        font.glyph_mut("I").unwrap().export = false;
        let mut doc = Document::new(font, None);
        let original = doc.font.clone();
        let drawn: Vec<String> = doc
            .font
            .glyphs
            .iter()
            .filter(|g| g.layer("m01").is_some_and(|l| !l.shapes.is_empty()))
            .map(|g| g.name.clone())
            .collect();
        let params = FilterParams {
            radius: 20.0,
            ..Default::default()
        };
        let r = run_filter_in(
            &mut doc,
            "H",
            "m01",
            &drawn,
            "round-corners",
            &params,
            false,
        )
        .unwrap();
        assert!(r.changed.len() > 3, "{:?}", r.changed);
        assert!(r.changed.contains(&"I".to_string()));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);

        // In I's tab, Undo puts back I alone.
        doc.history
            .set_context(kernest_core::edit::Scope::Glyph("I".into()));
        let info = doc.history.undo_info().unwrap();
        assert_eq!(info.glyph.as_deref(), Some("I"));
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font.glyph("I"), original.glyph("I"));
        assert_ne!(doc.font.glyph("H"), original.glyph("H"));

        // In Font View, one Undo takes back the rest.
        doc.history.set_context(kernest_core::edit::Scope::Font);
        assert_eq!(doc.history.undo_info().unwrap().glyphs, r.changed.len());
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }

    #[test]
    fn round_corners_filter_over_glyphs_with_locked_skip() {
        let mut doc = Document::new(sample::sample_font(), None);
        let original = doc.font.clone();
        doc.font.glyph_mut("A").unwrap().locked = true;
        let params = FilterParams {
            radius: 20.0,
            ..Default::default()
        };
        let r = run_filter_in(
            &mut doc,
            "H",
            "m01",
            &["H".into(), "A".into()],
            "round-corners",
            &params,
            false,
        )
        .unwrap();
        assert_eq!(r.changed, vec!["H".to_string()]);
        assert_eq!(r.warnings, vec!["Locked glyphs were skipped.".to_string()]);
        assert!(r.layer.status.can_undo);
        assert!(r.layer.paths.iter().all(|p| p.nodes.len() > 4));
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font.glyph("H"), original.glyph("H"));
    }

    #[test]
    fn transformations_filter_moves_all_masters_and_skips_locked_paths() {
        let mut doc = two_master_doc();
        set_path_locked_in(&mut doc, "H", "m01", 0, true).unwrap();
        let original = doc.font.clone();
        let params = FilterParams {
            affine: transform::translate(10.0, 0.0),
            ..Default::default()
        };
        let r = run_filter_in(&mut doc, "H", "m01", &[], "transformations", &params, true).unwrap();
        assert_eq!(r.changed, vec!["H".to_string()]);
        assert!(r.warnings.is_empty());
        let o = original.glyph("H").unwrap();
        for m in ["m01", "m02"] {
            let l = doc.font.glyph("H").unwrap().layer(m).unwrap();
            let ol = o.layer(m).unwrap();
            for (i, s) in l.shapes.iter().enumerate() {
                if m == "m01" && i == 0 {
                    assert_eq!(s, &ol.shapes[0], "the locked path stays put");
                } else if let (Shape::Path(p), Shape::Path(q)) = (s, &ol.shapes[i]) {
                    assert_eq!(p.nodes[0].x, q.nodes[0].x + 10.0);
                }
            }
        }
        // One undo step restores the whole run; the lock was its own step before it.
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }

    #[test]
    fn offset_filter_limits_to_current_master_when_incompatible() {
        let mut doc = two_master_doc();
        // The bold master is tiny: shrinking erases its paths, which would break
        // interpolation, so only the current master changes.
        shorten_bold(&mut doc, 0.05);
        let mut l = doc.font.glyph("H").unwrap().layer("m02").unwrap().clone();
        transform::apply(
            &mut l,
            &Selection::default(),
            &transform::scale(0.05, 1.0),
            (0.0, 0.0),
        );
        *doc.font.glyph_mut("H").unwrap().layer_mut("m02").unwrap() = l;
        let original = doc.font.clone();
        let params = FilterParams {
            offset_x: -10.0,
            offset_y: -10.0,
            ..Default::default()
        };
        let r = run_filter_in(&mut doc, "H", "m01", &[], "offset-curve", &params, true).unwrap();
        assert_eq!(r.changed, vec!["H".to_string()]);
        assert!(r.warnings.iter().any(|w| w.contains("current master only")));
        assert_eq!(
            doc.font.glyph("H").unwrap().layer("m02").unwrap(),
            original.glyph("H").unwrap().layer("m02").unwrap()
        );
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }

    #[test]
    fn filter_rejects_bad_names_and_numbers() {
        let mut doc = Document::new(sample::sample_font(), None);
        let p = FilterParams::default();
        assert!(run_filter_in(&mut doc, "H", "m01", &[], "nope", &p, false).is_err());
        let bad = FilterParams {
            radius: -1.0,
            ..Default::default()
        };
        assert!(run_filter_in(&mut doc, "H", "m01", &[], "round-corners", &bad, false).is_err());
        let bad = FilterParams {
            offset_x: f64::NAN,
            ..Default::default()
        };
        assert!(run_filter_in(&mut doc, "H", "m01", &[], "offset-curve", &bad, false).is_err());
        let bad = FilterParams {
            affine: [1.0, 0.0, 0.0, 1.0, f64::INFINITY, 0.0],
            ..Default::default()
        };
        assert!(run_filter_in(&mut doc, "H", "m01", &[], "transformations", &bad, false).is_err());
        assert!(run_filter_in(&mut doc, "nope", "m01", &[], "round-corners", &p, false).is_err());
    }

    #[test]
    fn filter_preview_shows_the_result_without_changing_the_document() {
        let doc = Document::new(sample::sample_font(), None);
        let before = doc.font.clone();
        let rev = doc.revision;
        let p = FilterParams {
            radius: 20.0,
            ..Default::default()
        };
        let view = filter_preview_in(&doc, "H", "m01", "round-corners", &p).unwrap();
        let stored = layer_view_of(&doc, "H", "m01").unwrap();
        assert_ne!(
            serde_json::to_value(&view.paths).unwrap(),
            serde_json::to_value(&stored.paths).unwrap()
        );
        // The document itself is untouched: no edit, no undo step.
        assert_eq!(doc.font, before);
        assert_eq!(doc.revision, rev);
        assert!(!doc.history.can_undo());
        let bad = FilterParams {
            radius: -1.0,
            ..Default::default()
        };
        assert!(filter_preview_in(&doc, "H", "m01", "round-corners", &bad).is_err());
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterpolationFixed {
    /// How many paths changed.
    changed: usize,
    status: DocStatus,
}

/// Lines up contour order, direction and start points in every master and
/// intermediate layer with the default master (one undo step).
#[tauri::command]
pub fn fix_interpolation(state: State<AppState>, glyph: String) -> CmdResult<InterpolationFixed> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let (next, changed) = kernest_core::interpolation_quality::fix(&doc.font, &glyph)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(InterpolationFixed {
        changed,
        status: status(doc),
    })
}
