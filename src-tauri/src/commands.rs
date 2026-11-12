//! Commands the TypeScript UI calls through Tauri IPC.
//!
//! The open document (font, path, undo history) lives in Rust; the UI receives
//! views shaped for each screen and sends edits back as whole-layer updates.
//! Errors are returned as user-facing strings.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use kernest_core::edit::{component_creates_cycle, with_metric, Affected, History, MetricEdit};
use kernest_core::geometry::{sidebearings, svg_path};
use kernest_core::glyphdata::{codepoints_for, GLYPH_SETS};
use kernest_core::interpolate::check_glyph;
use kernest_core::{
    package, sample, Anchor, Font, Layer, Metrics, Node, NodeKind, Path as KPath, Shape,
};
use serde::{Deserialize, Serialize};
use tauri::State;

pub struct Document {
    pub font: Font,
    pub path: Option<PathBuf>,
    pub history: History,
    /// Incremented on every change; compared with `saved_revision` for the unsaved-changes state.
    pub(crate) revision: u64,
    saved_revision: u64,
}

impl Document {
    pub(crate) fn new(font: Font, path: Option<PathBuf>) -> Self {
        Self {
            font,
            path,
            history: History::default(),
            revision: 0,
            saved_revision: 0,
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

// ---------- Views ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphSetInfo {
    id: &'static str,
    name: &'static str,
    count: usize,
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
    character: Option<String>,
    category: &'static str,
    export: bool,
    width: f64,
    lsb: Option<f64>,
    rsb: Option<f64>,
    /// Outline as SVG path data in font units (y up), empty when not drawn yet.
    svg: String,
    /// False when this glyph's masters can't be interpolated.
    compatible: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocStatus {
    dirty: bool,
    can_undo: bool,
    can_redo: bool,
}

pub(crate) fn status(doc: &Document) -> DocStatus {
    DocStatus {
        dirty: doc.dirty(),
        can_undo: doc.history.can_undo(),
        can_redo: doc.history.can_redo(),
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDto {
    base: String,
    transform: [f64; 6],
    /// This component's resolved outline, SVG path data in font units (y up).
    svg: String,
}

/// A component as sent back by the editor (move, add or remove).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInput {
    base: String,
    transform: [f64; 6],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerView {
    glyph: String,
    unicode: Option<String>,
    master_id: String,
    metrics: Metrics,
    width: f64,
    lsb: Option<f64>,
    rsb: Option<f64>,
    paths: Vec<PathDto>,
    components: Vec<ComponentDto>,
    /// All components resolved to outlines, as SVG path data in font units (y up).
    component_svg: String,
    anchors: Vec<Anchor>,
    /// Why this glyph's masters can't be interpolated (empty when they can).
    issues: Vec<String>,
    status: DocStatus,
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
                character: cp.and_then(|c| c.char()).map(String::from),
                category: g.category().label(),
                export: g.export,
                width: layer.map_or(0.0, |l| l.width),
                lsb: sb.map(|s| s.0),
                rsb: sb.map(|s| s.1),
                svg: layer.map(|l| svg_path(font, l)).unwrap_or_default(),
                compatible: check_glyph(font, &g.name).is_empty(),
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
        status: status(doc),
    })
}

fn layer_view_of(doc: &Document, glyph: &str, master_id: &str) -> CmdResult<LayerView> {
    let font = &doc.font;
    let g = font
        .glyph(glyph)
        .ok_or_else(|| format!("No glyph named \"{glyph}\""))?;
    let master = font
        .master(master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
    let layer = g
        .layer(master_id)
        .ok_or_else(|| format!("\"{glyph}\" has no layer for {}", master.name))?;
    let mut paths = Vec::new();
    let mut components = Vec::new();
    let mut comp_only = Layer::new(master_id, layer.width);
    for s in &layer.shapes {
        match s {
            Shape::Path(p) => paths.push(PathDto {
                closed: p.closed,
                nodes: p
                    .nodes
                    .iter()
                    .map(|n| NodeDto {
                        x: n.x,
                        y: n.y,
                        kind: n.kind.into(),
                        smooth: n.smooth,
                    })
                    .collect(),
            }),
            Shape::Component { base, transform } => {
                let mut single = Layer::new(master_id, layer.width);
                single.shapes.push(s.clone());
                components.push(ComponentDto {
                    base: base.clone(),
                    transform: *transform,
                    svg: svg_path(font, &single),
                });
                comp_only.shapes.push(s.clone());
            }
        }
    }
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
        component_svg: svg_path(font, &comp_only),
        anchors: layer.anchors.clone(),
        issues: check_glyph(font, glyph),
        status: status(doc),
    })
}

/// Validates an edited path from the UI and converts it to the model.
fn to_path(p: &PathDto) -> CmdResult<KPath> {
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
        closed: p.closed,
        nodes: p
            .nodes
            .iter()
            .map(|d| Node {
                x: d.x,
                y: d.y,
                kind: d.kind.into(),
                smooth: d.smooth && d.kind != NodeKindDto::Offcurve,
            })
            .collect(),
    })
}

/// Validates a component from the UI: the base must exist and must not lead back to `glyph`.
fn to_component(font: &Font, glyph: &str, c: ComponentInput) -> CmdResult<Shape> {
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
    })
}

// ---------- Document commands ----------

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
        .map(|s| GlyphSetInfo {
            id: s.id,
            name: s.name,
            count: codepoints_for(&[s.id]).len(),
        })
        .collect()
}

#[tauri::command]
pub fn new_font(
    state: State<AppState>,
    family_name: String,
    glyph_sets: Vec<String>,
) -> CmdResult<FontView> {
    let family = family_name.trim();
    if family.is_empty() {
        return Err("Enter a family name.".into());
    }
    let ids: Vec<&str> = glyph_sets.iter().map(String::as_str).collect();
    replace(
        &state,
        Document::new(Font::new(family, codepoints_for(&ids)), None),
    )
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
    if doc
        .history
        .apply(&mut doc.font, &glyph, &master_id, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, &glyph, &master_id)
}

/// Sets the left sidebearing, right sidebearing or advance width ("lsb" | "rsb" | "width").
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
    let edit = match metric.as_str() {
        "lsb" => MetricEdit::Lsb(value),
        "rsb" => MetricEdit::Rsb(value),
        "width" => MetricEdit::Width(value),
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

    fn node(x: f64, y: f64, kind: NodeKindDto) -> NodeDto {
        NodeDto {
            x,
            y,
            kind,
            smooth: false,
        }
    }

    #[test]
    fn path_validation() {
        use NodeKindDto::*;
        let ok = PathDto {
            closed: true,
            nodes: vec![
                node(0., 0., Line),
                node(10., 0., Line),
                node(10., 10., Line),
            ],
        };
        assert!(to_path(&ok).is_ok());
        let only_off = PathDto {
            closed: true,
            nodes: vec![node(0., 0., Offcurve), node(1., 1., Offcurve)],
        };
        assert!(to_path(&only_off).is_err());
        let open_off = PathDto {
            closed: false,
            nodes: vec![node(0., 0., Offcurve), node(1., 1., Line)],
        };
        assert!(to_path(&open_off).is_err());
        let three = PathDto {
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
        };
        assert!(to_component(&font, "A", ok).is_ok());
        let missing = ComponentInput {
            base: "nope".into(),
            transform: kernest_core::IDENTITY,
        };
        assert!(to_component(&font, "A", missing).is_err());
        let selfref = ComponentInput {
            base: "A".into(),
            transform: kernest_core::IDENTITY,
        };
        assert!(to_component(&font, "A", selfref).is_err());
        let bad = ComponentInput {
            base: "H".into(),
            transform: [1.0, 0.0, 0.0, 1.0, f64::INFINITY, 0.0],
        };
        assert!(to_component(&font, "A", bad).is_err());
    }

    #[test]
    fn layer_view_lists_components_with_outlines() {
        let mut doc = Document::new(sample::sample_font(), None);
        let mut next = doc.font.glyph("A").unwrap().layer("m01").unwrap().clone();
        next.shapes.push(Shape::Component {
            base: "H".into(),
            transform: kernest_core::IDENTITY,
        });
        doc.history.apply(&mut doc.font, "A", "m01", next).unwrap();
        let v = layer_view_of(&doc, "A", "m01").unwrap();
        assert_eq!(v.components.len(), 1);
        assert!(v.components[0].svg.starts_with('M'));
        assert_eq!(v.lsb, Some(80.0));
    }
}
