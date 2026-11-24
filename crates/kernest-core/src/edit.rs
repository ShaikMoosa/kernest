//! Editing operations on layers, plus undo/redo history.
//!
//! History stores whole-layer snapshots (before/after). Layers are small, so
//! this is simple and robust: any edit, however complex, undoes exactly.

use crate::error::{Error, Result};
use crate::geometry::bounds;
use std::collections::HashMap;

use crate::model::{Font, KernPair, Layer, Shape};

/// Moves everything in a layer horizontally (outlines, components, anchors).
pub fn translate_x(layer: &mut Layer, dx: f64) {
    if dx == 0.0 {
        return;
    }
    for shape in &mut layer.shapes {
        match shape {
            Shape::Path(p) => p.nodes.iter_mut().for_each(|n| n.x += dx),
            Shape::Component { transform, .. } => transform[4] += dx,
        }
    }
    for a in &mut layer.anchors {
        a.x += dx;
    }
}

fn layer_ref<'a>(font: &'a Font, glyph: &str, master_id: &str) -> Result<&'a Layer> {
    font.glyph(glyph)
        .ok_or_else(|| Error::Invalid(format!("no glyph named \"{glyph}\"")))?
        .layer(master_id)
        .ok_or_else(|| {
            Error::Invalid(format!(
                "glyph \"{glyph}\" has no layer for master \"{master_id}\""
            ))
        })
}

/// Which metric of a layer to set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetricEdit {
    /// Left sidebearing: moves the outlines, keeps the right sidebearing.
    Lsb(f64),
    /// Right sidebearing: changes the advance width.
    Rsb(f64),
    /// Advance width: keeps outlines in place.
    Width(f64),
    /// Moves the outlines sideways by this much inside the same width, so the
    /// left sidebearing grows as the right one shrinks (SMO-35).
    Shift(f64),
}

/// Returns the edited copy of the layer; the caller stores it (and records history).
/// Sidebearing edits on an empty layer are rejected because there are no outlines to measure.
pub fn with_metric(font: &Font, glyph: &str, master_id: &str, edit: MetricEdit) -> Result<Layer> {
    let layer = layer_ref(font, glyph, master_id)?;
    let mut out = layer.clone();
    let finite = |v: f64| {
        if v.is_finite() {
            Ok(v)
        } else {
            Err(Error::Invalid("enter a number".into()))
        }
    };
    match edit {
        MetricEdit::Width(w) => {
            let w = finite(w)?;
            if w < 0.0 {
                return Err(Error::Invalid("width cannot be negative".into()));
            }
            out.width = w;
        }
        MetricEdit::Lsb(v) => {
            let v = finite(v)?;
            let b = bounds(font, layer).ok_or_else(|| {
                Error::Invalid(format!(
                    "\"{glyph}\" has no outlines to set a sidebearing on"
                ))
            })?;
            let dx = v - b.min_x;
            translate_x(&mut out, dx);
            out.width = (layer.width + dx).max(0.0);
        }
        MetricEdit::Rsb(v) => {
            let v = finite(v)?;
            let b = bounds(font, layer).ok_or_else(|| {
                Error::Invalid(format!(
                    "\"{glyph}\" has no outlines to set a sidebearing on"
                ))
            })?;
            out.width = (b.max_x + v).max(0.0);
        }
        MetricEdit::Shift(dx) => {
            let dx = finite(dx)?;
            if bounds(font, layer).is_none() {
                return Err(Error::Invalid(format!(
                    "\"{glyph}\" has no outlines to move"
                )));
            }
            translate_x(&mut out, dx);
        }
    }
    Ok(out)
}

/// True if using `base` as a component inside `glyph` would make a glyph contain
/// itself (directly or through nested components, in any master).
pub fn component_creates_cycle(font: &Font, glyph: &str, base: &str) -> bool {
    let mut stack = vec![base.to_string()];
    let mut seen = std::collections::HashSet::new();
    while let Some(name) = stack.pop() {
        if name == glyph {
            return true;
        }
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(g) = font.glyph(&name) {
            for l in &g.layers {
                for s in &l.shapes {
                    if let Shape::Component { base, .. } = s {
                        stack.push(base.clone());
                    }
                }
            }
        }
    }
    false
}

/// A glyph that uses another as a component (Handbook 9.1.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphUse {
    pub name: String,
    /// It has the component itself; otherwise it gets it through another component.
    pub direct: bool,
}

/// The glyphs that use `glyph` as a component in any layer, directly or through other
/// components (`dieresiscomb` in `dieresis` in `edieresis`), in font order.
pub fn glyphs_using(font: &Font, glyph: &str) -> Vec<GlyphUse> {
    let uses = |g: &crate::model::Glyph, base: &str| {
        g.layers.iter().any(|l| {
            l.shapes
                .iter()
                .any(|s| matches!(s, Shape::Component { base: b, .. } if b == base))
        })
    };
    let mut found: std::collections::HashMap<&str, bool> = std::collections::HashMap::new();
    let mut queue = vec![glyph.to_string()];
    while let Some(current) = queue.pop() {
        for g in &font.glyphs {
            if g.name == glyph || found.contains_key(g.name.as_str()) || !uses(g, &current) {
                continue;
            }
            found.insert(&g.name, current == glyph);
            queue.push(g.name.clone());
        }
    }
    // A glyph reached through another may also use `glyph` itself.
    font.glyphs
        .iter()
        .filter_map(|g| {
            found.get(g.name.as_str()).map(|_| GlyphUse {
                name: g.name.clone(),
                direct: uses(g, glyph),
            })
        })
        .collect()
}

/// Kerning groups of a glyph: (left group, right group).
pub type Groups = (Option<String>, Option<String>);

/// One recorded edit, with the state before and after.
#[derive(Debug, Clone)]
pub enum Change {
    Layer {
        glyph: String,
        master_id: String,
        before: Box<Layer>,
        after: Box<Layer>,
    },
    Kerning {
        master_id: String,
        before: Vec<KernPair>,
        after: Vec<KernPair>,
    },
    Groups {
        glyph: String,
        before: Groups,
        after: Groups,
    },
    /// A structural edit (axes, masters, instances, info): whole-font snapshots.
    Font { before: Box<Font>, after: Box<Font> },
}

/// What an undo or redo touched, so the UI knows what to refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Affected {
    Layer {
        glyph: String,
        master_id: String,
    },
    Kerning {
        master_id: String,
    },
    Groups {
        glyph: String,
    },
    /// Anything may have changed: reload everything.
    Font,
}

/// Which undo history Undo and Redo use (SMO-83): a glyph's own, in that glyph's
/// Edit View tab, or the font's, in Font View, Font Info and text tabs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Font,
    Glyph(String),
}

/// One undo step: the changes one action made, and what to call it in the Edit menu.
#[derive(Debug, Clone)]
struct Step {
    changes: Vec<Change>,
    action: &'static str,
}

impl Step {
    /// The distinct glyphs whose layers the step changed.
    fn glyphs(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for c in &self.changes {
            if let Change::Layer { glyph, .. } = c {
                if !out.contains(&glyph.as_str()) {
                    out.push(glyph);
                }
            }
        }
        out
    }
}

/// What the next Undo or Redo would do, for the Edit menu: an action such as
/// "Edit Outlines", the glyph it's in (glyph scope) and how many glyphs a font-level
/// group covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInfo {
    pub action: &'static str,
    pub glyph: Option<String>,
    pub glyphs: usize,
}

/// Undo/redo for a document (SMO-83). Each glyph keeps its own history of layer
/// edits; font-level edits (Font Info, kerning, features, glyph set) have the font's.
/// Layer edits made while the font-level history is in use (Font View actions,
/// filters, text tabs), or that touch several glyphs at once, are also kept as one
/// font-level group, so Font View can undo all of them in one step.
#[derive(Debug, Default)]
pub struct History {
    context: Scope,
    font_undo: Vec<Step>,
    font_redo: Vec<Step>,
    glyph_undo: HashMap<String, Vec<Step>>,
    glyph_redo: HashMap<String, Vec<Step>>,
}

/// Maximum undo steps kept in the font's history.
const MAX_UNDO: usize = 500;
/// Maximum undo steps kept per glyph; the oldest go first.
const MAX_GLYPH_UNDO: usize = 200;

fn push_capped(stack: &mut Vec<Step>, step: Step, max: usize) {
    stack.push(step);
    if stack.len() > max {
        stack.remove(0);
    }
}

/// What a layer edit changed, for the Edit menu.
fn layer_action(before: &Layer, after: &Layer) -> &'static str {
    if before.shapes != after.shapes || before.parts != after.parts {
        "Edit Outlines"
    } else if before.anchors != after.anchors {
        "Edit Anchors"
    } else if before.width != after.width
        || before.vert_width != after.vert_width
        || before.vert_origin != after.vert_origin
        || before.metrics_keys != after.metrics_keys
    {
        "Change Metrics"
    } else if before.hints != after.hints {
        "Edit Hints"
    } else if before.guides != after.guides {
        "Edit Guides"
    } else if before.background != after.background {
        "Edit Background"
    } else {
        "Edit Layer"
    }
}

/// What a font-level edit changed, for the Edit menu.
fn font_action(before: &Font, after: &Font) -> &'static str {
    let names = |f: &Font| f.glyphs.iter().map(|g| g.name.clone()).collect::<Vec<_>>();
    let (b, a) = (names(before), names(after));
    if a.len() > b.len() {
        "Add Glyphs"
    } else if a.len() < b.len() {
        "Remove Glyphs"
    } else if b != a {
        let mut bs = b.clone();
        let mut as_ = a.clone();
        bs.sort();
        as_.sort();
        if bs == as_ {
            "Reorder Glyphs"
        } else {
            "Rename Glyphs"
        }
    } else if before.info != after.info {
        "Font Info"
    } else if before.axes != after.axes {
        "Axes"
    } else if before.masters != after.masters {
        "Masters"
    } else if before.instances != after.instances || before.variable_fonts != after.variable_fonts {
        "Exports"
    } else if before.features != after.features || before.feature_settings != after.feature_settings
    {
        "Features"
    } else if before.kerning != after.kerning {
        "Kerning"
    } else if before.glyphs != after.glyphs {
        "Glyph Info"
    } else {
        "Font Change"
    }
}

/// The layer changes between two fonts that differ only in glyph layers (same
/// glyphs, same layer keys, nothing else changed). `None` when anything else
/// differs. The fonts are compared in place, without copying them.
fn layer_only_changes(cur: &mut Font, after: &mut Font) -> Option<Vec<Change>> {
    if cur.glyphs.len() != after.glyphs.len() {
        return None;
    }
    let cg = std::mem::take(&mut cur.glyphs);
    let ag = std::mem::take(&mut after.glyphs);
    let rest_same = *cur == *after;
    cur.glyphs = cg;
    after.glyphs = ag;
    if !rest_same {
        return None;
    }
    let mut out = Vec::new();
    for (c, a) in cur.glyphs.iter_mut().zip(after.glyphs.iter_mut()) {
        if c.layers.len() != a.layers.len()
            || c.layers
                .iter()
                .zip(&a.layers)
                .any(|(x, y)| x.key() != y.key())
        {
            return None;
        }
        let cl = std::mem::take(&mut c.layers);
        let al = std::mem::take(&mut a.layers);
        let meta_same = *c == *a;
        c.layers = cl;
        a.layers = al;
        if !meta_same {
            return None;
        }
        for (x, y) in c.layers.iter().zip(&a.layers) {
            if x != y {
                out.push(Change::Layer {
                    glyph: c.name.clone(),
                    master_id: x.key().to_string(),
                    before: Box::new(x.clone()),
                    after: Box::new(y.clone()),
                });
            }
        }
    }
    Some(out)
}

impl History {
    /// Picks the history Undo and Redo use: `Scope::Glyph` in a glyph's Edit View
    /// tab, `Scope::Font` everywhere else.
    pub fn set_context(&mut self, scope: Scope) {
        self.context = scope;
    }

    pub fn context(&self) -> &Scope {
        &self.context
    }

    fn undo_stack(&self) -> Option<&Vec<Step>> {
        match &self.context {
            Scope::Font => Some(&self.font_undo),
            Scope::Glyph(g) => self.glyph_undo.get(g),
        }
    }

    fn redo_stack(&self) -> Option<&Vec<Step>> {
        match &self.context {
            Scope::Font => Some(&self.font_redo),
            Scope::Glyph(g) => self.glyph_redo.get(g),
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo_stack().is_some_and(|s| !s.is_empty())
    }
    pub fn can_redo(&self) -> bool {
        self.redo_stack().is_some_and(|s| !s.is_empty())
    }

    fn info(&self, step: Option<&Step>) -> Option<StepInfo> {
        let step = step?;
        let glyphs = step.glyphs();
        Some(StepInfo {
            action: step.action,
            glyph: match &self.context {
                Scope::Glyph(g) => Some(g.clone()),
                Scope::Font if glyphs.len() == 1 => Some(glyphs[0].to_string()),
                Scope::Font => None,
            },
            glyphs: glyphs.len(),
        })
    }

    /// What Undo would do now, for the Edit menu.
    pub fn undo_info(&self) -> Option<StepInfo> {
        self.info(self.undo_stack().and_then(|s| s.last()))
    }

    /// What Redo would do now, for the Edit menu.
    pub fn redo_info(&self) -> Option<StepInfo> {
        self.info(self.redo_stack().and_then(|s| s.last()))
    }

    /// Records layer changes from one action: each glyph's part goes on its own
    /// history; the whole also goes on the font's history when the font's history is
    /// in use or the action touched more than one glyph.
    fn record_layers(&mut self, changes: Vec<Change>) {
        let Some(Change::Layer { before, after, .. }) = changes.first() else {
            return;
        };
        let action = layer_action(before, after);
        let mut by_glyph: Vec<(String, Vec<Change>)> = Vec::new();
        for c in &changes {
            if let Change::Layer { glyph, .. } = c {
                match by_glyph.iter_mut().find(|(g, _)| g == glyph) {
                    Some((_, list)) => list.push(c.clone()),
                    None => by_glyph.push((glyph.clone(), vec![c.clone()])),
                }
            }
        }
        let many = by_glyph.len() > 1;
        for (glyph, list) in by_glyph {
            self.glyph_redo.remove(&glyph);
            push_capped(
                self.glyph_undo.entry(glyph).or_default(),
                Step {
                    changes: list,
                    action,
                },
                MAX_GLYPH_UNDO,
            );
        }
        if many || self.context == Scope::Font {
            self.font_redo.clear();
            push_capped(&mut self.font_undo, Step { changes, action }, MAX_UNDO);
        }
    }

    /// Records a font-level change.
    fn record_font(&mut self, change: Change, action: &'static str) {
        self.font_redo.clear();
        push_capped(
            &mut self.font_undo,
            Step {
                changes: vec![change],
                action,
            },
            MAX_UNDO,
        );
    }

    /// Moves glyphs' histories to their new names after a rename.
    pub fn rename_glyphs(&mut self, renames: &std::collections::BTreeMap<String, String>) {
        let rename_steps = |steps: &mut Vec<Step>| {
            for s in steps {
                for c in &mut s.changes {
                    if let Change::Layer { glyph, .. } | Change::Groups { glyph, .. } = c {
                        if let Some(new) = renames.get(glyph.as_str()) {
                            *glyph = new.clone();
                        }
                    }
                }
            }
        };
        for map in [&mut self.glyph_undo, &mut self.glyph_redo] {
            let moved: Vec<(String, Vec<Step>)> = renames
                .iter()
                .filter_map(|(old, new)| map.remove(old).map(|s| (new.clone(), s)))
                .collect();
            for (new, steps) in moved {
                map.insert(new, steps);
            }
            map.values_mut().for_each(rename_steps);
        }
        rename_steps(&mut self.font_undo);
        rename_steps(&mut self.font_redo);
        if let Scope::Glyph(g) = &mut self.context {
            if let Some(new) = renames.get(g.as_str()) {
                *g = new.clone();
            }
        }
    }

    /// Replaces a layer and records the change. No-op edits are not recorded.
    /// Returns `true` if the layer changed.
    pub fn apply(
        &mut self,
        font: &mut Font,
        glyph: &str,
        master_id: &str,
        after: Layer,
    ) -> Result<bool> {
        // `master_id` is the layer's key: a master id, or a brace/bracket layer's id.
        if after.key() != master_id {
            return Err(Error::Invalid("layer master does not match".into()));
        }
        let slot = font
            .glyph_mut(glyph)
            .ok_or_else(|| Error::Invalid(format!("no glyph named \"{glyph}\"")))?
            .layer_mut(master_id)
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "glyph \"{glyph}\" has no layer for master \"{master_id}\""
                ))
            })?;
        if *slot == after {
            return Ok(false);
        }
        let before = std::mem::replace(slot, after.clone());
        self.record_layers(vec![Change::Layer {
            glyph: glyph.into(),
            master_id: master_id.into(),
            before: Box::new(before),
            after: Box::new(after),
        }]);
        Ok(true)
    }

    /// Replaces the kerning of a master and records the change.
    pub fn apply_kerning(
        &mut self,
        font: &mut Font,
        master_id: &str,
        after: Vec<KernPair>,
    ) -> Result<bool> {
        if font.master(master_id).is_none() {
            return Err(Error::Invalid(format!("no master with id \"{master_id}\"")));
        }
        let slot = font.kerning.entry(master_id.to_string()).or_default();
        if *slot == after {
            return Ok(false);
        }
        let before = std::mem::replace(slot, after.clone());
        if font.kerning.get(master_id).is_some_and(Vec::is_empty) {
            font.kerning.remove(master_id);
        }
        self.record_font(
            Change::Kerning {
                master_id: master_id.into(),
                before,
                after,
            },
            "Kerning",
        );
        Ok(true)
    }

    /// Sets a glyph's kerning groups and records the change. Empty names clear a group.
    pub fn apply_groups(&mut self, font: &mut Font, glyph: &str, after: Groups) -> Result<bool> {
        let clean = |g: Option<String>| {
            g.map(|s| s.trim().trim_start_matches('@').to_string())
                .filter(|s| !s.is_empty())
        };
        let after = (clean(after.0), clean(after.1));
        for name in [&after.0, &after.1].into_iter().flatten() {
            if name.chars().any(char::is_whitespace) {
                return Err(Error::Invalid(format!(
                    "group name \"{name}\" cannot contain spaces"
                )));
            }
        }
        let g = font
            .glyph_mut(glyph)
            .ok_or_else(|| Error::Invalid(format!("no glyph named \"{glyph}\"")))?;
        let before = (g.left_group.clone(), g.right_group.clone());
        if before == after {
            return Ok(false);
        }
        g.left_group = after.0.clone();
        g.right_group = after.1.clone();
        self.record_font(
            Change::Groups {
                glyph: glyph.into(),
                before,
                after,
            },
            "Kerning Groups",
        );
        Ok(true)
    }

    /// Replaces the whole font and records the change. When only glyph layers
    /// changed, the change is recorded per glyph (see [`History`]); otherwise it's
    /// a font-level step, and the histories of glyphs that are gone are dropped.
    pub fn apply_font(&mut self, font: &mut Font, after: Font) -> Result<bool> {
        after.validate()?;
        if *font == after {
            return Ok(false);
        }
        let mut after = after;
        if let Some(changes) = layer_only_changes(font, &mut after) {
            *font = after;
            self.record_layers(changes);
            return Ok(true);
        }
        let action = font_action(font, &after);
        let before = std::mem::replace(font, after.clone());
        let kept: std::collections::HashSet<&str> =
            font.glyphs.iter().map(|g| g.name.as_str()).collect();
        self.glyph_undo.retain(|g, _| kept.contains(g.as_str()));
        self.glyph_redo.retain(|g, _| kept.contains(g.as_str()));
        self.record_font(
            Change::Font {
                before: Box::new(before),
                after: Box::new(after),
            },
            action,
        );
        Ok(true)
    }

    /// Puts one change back (`forward` false) or forward again. A layer is only
    /// touched while it's still as the change left it, so a step can't overwrite a
    /// later edit; a whole-font step keeps the layers edited since.
    fn restore(font: &mut Font, c: &Change, forward: bool) -> Option<Affected> {
        match c {
            Change::Layer {
                glyph,
                master_id,
                before,
                after,
            } => {
                let (from, to) = if forward {
                    (before, after)
                } else {
                    (after, before)
                };
                let slot = font.glyph_mut(glyph)?.layer_mut(master_id)?;
                if *slot != **from {
                    return None;
                }
                *slot = (**to).clone();
                Some(Affected::Layer {
                    glyph: glyph.clone(),
                    master_id: master_id.clone(),
                })
            }
            Change::Kerning {
                master_id,
                before,
                after,
            } => {
                let to = if forward { after } else { before };
                if to.is_empty() {
                    font.kerning.remove(master_id);
                } else {
                    font.kerning.insert(master_id.clone(), to.clone());
                }
                Some(Affected::Kerning {
                    master_id: master_id.clone(),
                })
            }
            Change::Groups {
                glyph,
                before,
                after,
            } => {
                let g = font.glyph_mut(glyph)?;
                let to = if forward { after } else { before };
                g.left_group = to.0.clone();
                g.right_group = to.1.clone();
                Some(Affected::Groups {
                    glyph: glyph.clone(),
                })
            }
            Change::Font { before, after } => {
                let (from, to) = if forward {
                    (before, after)
                } else {
                    (after, before)
                };
                let mut target = (**to).clone();
                // Layers edited since this step (in their own glyph's history) stay.
                for g in &mut target.glyphs {
                    let (Some(cur), Some(old)) = (font.glyph(&g.name), from.glyph(&g.name)) else {
                        continue;
                    };
                    for l in &mut g.layers {
                        let key = l.key().to_string();
                        if let Some(now) = cur.layer(&key) {
                            if old.layer(&key) != Some(now) {
                                *l = now.clone();
                            }
                        }
                    }
                }
                *font = target;
                Some(Affected::Font)
            }
        }
    }

    /// Undoes or redoes one step of the history in use; returns what it touched.
    fn step(&mut self, font: &mut Font, forward: bool) -> Option<Affected> {
        match self.context.clone() {
            Scope::Font => {
                let mut undo = std::mem::take(&mut self.font_undo);
                let mut redo = std::mem::take(&mut self.font_redo);
                let hit = if forward {
                    Self::run(font, &mut redo, &mut undo, true, false)
                } else {
                    Self::run(font, &mut undo, &mut redo, false, false)
                };
                self.font_undo = undo;
                self.font_redo = redo;
                hit
            }
            Scope::Glyph(g) => {
                let mut undo = self.glyph_undo.remove(&g).unwrap_or_default();
                let mut redo = self.glyph_redo.remove(&g).unwrap_or_default();
                let hit = if forward {
                    Self::run(font, &mut redo, &mut undo, true, true)
                } else {
                    Self::run(font, &mut undo, &mut redo, false, true)
                };
                if !undo.is_empty() {
                    self.glyph_undo.insert(g.clone(), undo);
                }
                if !redo.is_empty() {
                    self.glyph_redo.insert(g, redo);
                }
                hit
            }
        }
    }

    /// Moves the top step of `from` to `to`, restoring it. With `skip_spent`, a step
    /// with nothing left to restore (already put back from the font's history) is
    /// dropped and the next one is tried.
    fn run(
        font: &mut Font,
        from: &mut Vec<Step>,
        to: &mut Vec<Step>,
        forward: bool,
        skip_spent: bool,
    ) -> Option<Affected> {
        loop {
            let s = from.pop()?;
            let hits: Vec<Affected> = if forward {
                s.changes
                    .iter()
                    .filter_map(|c| Self::restore(font, c, true))
                    .collect()
            } else {
                s.changes
                    .iter()
                    .rev()
                    .filter_map(|c| Self::restore(font, c, false))
                    .collect()
            };
            if hits.is_empty() && skip_spent {
                continue;
            }
            to.push(s);
            return Some(match hits.as_slice() {
                [one] => one.clone(),
                [Affected::Layer { glyph, master_id }, rest @ ..]
                    if rest
                        .iter()
                        .all(|h| matches!(h, Affected::Layer { glyph: g, .. } if g == glyph)) =>
                {
                    Affected::Layer {
                        glyph: glyph.clone(),
                        master_id: master_id.clone(),
                    }
                }
                _ => Affected::Font,
            });
        }
    }

    /// Undoes the last change of the history in use; returns what it touched.
    pub fn undo(&mut self, font: &mut Font) -> Option<Affected> {
        self.step(font, false)
    }

    /// Redoes the last undone change of the history in use; returns what it touched.
    pub fn redo(&mut self, font: &mut Font) -> Option<Affected> {
        self.step(font, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::sidebearings;
    use crate::sample::sample_font;

    fn sb(font: &Font, g: &str) -> (f64, f64, f64) {
        let l = font.glyph(g).unwrap().layer("m01").unwrap();
        let (a, b) = sidebearings(font, l).unwrap();
        (a, l.width, b)
    }

    #[test]
    fn set_lsb_keeps_rsb() {
        let mut f = sample_font();
        let mut h = History::default();
        let l = with_metric(&f, "H", "m01", MetricEdit::Lsb(100.0)).unwrap();
        assert!(h.apply(&mut f, "H", "m01", l).unwrap());
        assert_eq!(sb(&f, "H"), (100.0, 700.0, 80.0));
    }

    #[test]
    fn set_rsb_and_width() {
        let mut f = sample_font();
        let mut h = History::default();
        let l = with_metric(&f, "H", "m01", MetricEdit::Rsb(40.0)).unwrap();
        h.apply(&mut f, "H", "m01", l).unwrap();
        assert_eq!(sb(&f, "H"), (80.0, 640.0, 40.0));
        let l = with_metric(&f, "H", "m01", MetricEdit::Width(700.0)).unwrap();
        h.apply(&mut f, "H", "m01", l).unwrap();
        assert_eq!(sb(&f, "H"), (80.0, 700.0, 100.0));
        assert!(with_metric(&f, "H", "m01", MetricEdit::Width(-1.0)).is_err());
        assert!(with_metric(&f, "space", "m01", MetricEdit::Lsb(10.0)).is_err());
    }

    #[test]
    fn shift_moves_outlines_inside_the_width() {
        let mut f = sample_font();
        let mut h = History::default();
        let l = with_metric(&f, "H", "m01", MetricEdit::Shift(10.0)).unwrap();
        h.apply(&mut f, "H", "m01", l).unwrap();
        assert_eq!(sb(&f, "H"), (90.0, 680.0, 70.0));
        let l = with_metric(&f, "H", "m01", MetricEdit::Shift(-20.0)).unwrap();
        h.apply(&mut f, "H", "m01", l).unwrap();
        assert_eq!(sb(&f, "H"), (70.0, 680.0, 90.0));
        assert!(with_metric(&f, "space", "m01", MetricEdit::Shift(10.0)).is_err());
    }

    #[test]
    fn undo_redo() {
        let mut f = sample_font();
        let original = f.clone();
        let mut h = History::default();
        let l = with_metric(&f, "H", "m01", MetricEdit::Lsb(100.0)).unwrap();
        h.apply(&mut f, "H", "m01", l).unwrap();
        let edited = f.clone();
        assert_eq!(
            h.undo(&mut f),
            Some(Affected::Layer {
                glyph: "H".into(),
                master_id: "m01".into()
            })
        );
        assert_eq!(f, original);
        assert!(h.can_redo());
        h.redo(&mut f);
        assert_eq!(f, edited);
        // A new edit clears redo.
        h.undo(&mut f);
        let l = with_metric(&f, "I", "m01", MetricEdit::Width(300.0)).unwrap();
        h.apply(&mut f, "I", "m01", l).unwrap();
        assert!(!h.can_redo());
        assert_eq!(
            h.undo(&mut f),
            Some(Affected::Layer {
                glyph: "I".into(),
                master_id: "m01".into()
            })
        );
        assert_eq!(f, original);
        assert_eq!(h.undo(&mut f), None);
    }

    #[test]
    fn no_op_edits_are_not_recorded() {
        let mut f = sample_font();
        let mut h = History::default();
        let same = f.glyph("H").unwrap().layer("m01").unwrap().clone();
        assert!(!h.apply(&mut f, "H", "m01", same).unwrap());
        assert!(!h.can_undo());
    }

    #[test]
    fn component_cycles_are_detected() {
        let mut f = sample_font();
        assert!(component_creates_cycle(&f, "A", "A"));
        assert!(!component_creates_cycle(&f, "A", "H"));
        // bar uses l; adding bar to l would loop l -> bar -> l.
        f.glyph_mut("bar").unwrap().layers[0].shapes =
            vec![Shape::component("l", crate::model::IDENTITY)];
        assert!(component_creates_cycle(&f, "l", "bar"));
        assert!(!component_creates_cycle(&f, "bar", "l"));
    }

    #[test]
    fn translate_moves_components_and_anchors() {
        let mut l = Layer::new("m01", 500.0);
        l.shapes.push(Shape::component("a", crate::model::IDENTITY));
        l.anchors.push(crate::model::Anchor {
            name: "top".into(),
            x: 250.0,
            y: 700.0,
        });
        translate_x(&mut l, 10.0);
        assert!(matches!(&l.shapes[0], Shape::Component { transform, .. } if transform[4] == 10.0));
        assert_eq!(l.anchors[0].x, 260.0);
    }
}

#[cfg(test)]
mod glyphs_using_tests {
    use super::*;
    use crate::sample::sample_font;

    fn comp(base: &str) -> Shape {
        Shape::component(base, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
    }

    /// Handbook 9.1.5 (Moving between Base Glyphs and Composites): the glyphs that use
    /// a glyph, directly or through other components, in font order.
    #[test]
    fn lists_direct_and_nested_users_in_font_order() {
        let mut f = sample_font();
        // B uses I; L uses B (so I through B); O uses both I and B; H uses only H's
        // own outline.
        f.glyph_mut("B").unwrap().layers[0].shapes.push(comp("I"));
        f.glyph_mut("L").unwrap().layers[0].shapes = vec![comp("B")];
        f.glyph_mut("O").unwrap().layers[0]
            .shapes
            .extend([comp("B"), comp("I")]);
        let got: Vec<(String, bool)> = glyphs_using(&f, "I")
            .into_iter()
            .map(|u| (u.name, u.direct))
            .collect();
        assert_eq!(
            got,
            [
                ("B".to_string(), true),
                ("L".to_string(), false),
                ("O".to_string(), true)
            ]
        );
        assert_eq!(
            glyphs_using(&f, "B")
                .iter()
                .map(|u| u.name.as_str())
                .collect::<Vec<_>>(),
            ["L", "O"]
        );
        assert!(glyphs_using(&f, "O").is_empty());
        assert!(glyphs_using(&f, "nope").is_empty());
        // A component in any layer counts.
        let mut g = f.clone();
        let mut extra = g.glyph("A").unwrap().layers[0].clone();
        extra.master_id = "m01".into();
        extra.shapes = vec![comp("O")];
        g.glyph_mut("A").unwrap().layers[0] = extra;
        assert_eq!(glyphs_using(&g, "O")[0].name, "A");
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    use crate::sample::sample_font;

    #[test]
    fn kerning_and_groups_undo() {
        let mut f = sample_font();
        let original = f.clone();
        let mut h = History::default();
        let pairs = crate::kerning::with_pair(&f.kerning["m01"], "H", "H", -5.0);
        assert!(h.apply_kerning(&mut f, "m01", pairs).unwrap());
        assert!(h
            .apply_groups(&mut f, "T", (Some(" @T ".into()), Some("T".into())))
            .unwrap());
        assert_eq!(f.glyph("T").unwrap().left_group.as_deref(), Some("T"));
        assert!(!h
            .apply_groups(&mut f, "T", (Some("T".into()), Some("T".into())))
            .unwrap());
        assert!(h
            .apply_groups(&mut f, "T", (Some("bad name".into()), None))
            .is_err());
        assert_eq!(h.undo(&mut f), Some(Affected::Groups { glyph: "T".into() }));
        assert_eq!(
            h.undo(&mut f),
            Some(Affected::Kerning {
                master_id: "m01".into()
            })
        );
        assert_eq!(f, original);
        h.redo(&mut f);
        assert_eq!(
            crate::kerning::pair_value(&f, "m01", "H", "H").map(|v| v.0),
            Some(-5.0)
        );
        assert!(h.apply_kerning(&mut f, "nope", vec![]).is_err());
    }
}

#[cfg(test)]
mod font_history_tests {
    use super::*;
    use crate::sample::sample_font;

    #[test]
    fn structural_edits_undo() {
        let mut f = sample_font();
        let original = f.clone();
        let mut h = History::default();
        let next = crate::structure::set_info(&f, "Renamed", 1000, 2, 0).unwrap();
        assert!(h.apply_font(&mut f, next).unwrap());
        assert_eq!(f.info.family_name, "Renamed");
        assert_eq!(h.undo(&mut f), Some(Affected::Font));
        assert_eq!(f, original);
        let mut broken = f.clone();
        broken.masters.clear();
        assert!(h.apply_font(&mut f, broken).is_err());
    }
}

#[cfg(test)]
mod glyph_history_tests {
    //! SMO-83: per-glyph undo histories next to the font's.
    use super::*;
    use crate::sample::sample_font;

    fn widen(f: &Font, g: &str, by: f64) -> Layer {
        let mut l = f.glyph(g).unwrap().layer("m01").unwrap().clone();
        l.width += by;
        l
    }

    fn width(f: &Font, g: &str) -> f64 {
        f.glyph(g).unwrap().layer("m01").unwrap().width
    }

    #[test]
    fn each_glyph_undoes_its_own_edits() {
        let mut f = sample_font();
        let (a0, h0) = (width(&f, "A"), width(&f, "H"));
        let mut h = History::default();
        h.set_context(Scope::Glyph("A".into()));
        let l = widen(&f, "A", 10.0);
        h.apply(&mut f, "A", "m01", l).unwrap();
        h.set_context(Scope::Glyph("H".into()));
        let l = widen(&f, "H", 20.0);
        h.apply(&mut f, "H", "m01", l).unwrap();
        // Back in A: Undo reverts A and leaves H alone.
        h.set_context(Scope::Glyph("A".into()));
        assert_eq!(
            h.undo_info(),
            Some(StepInfo {
                action: "Change Metrics",
                glyph: Some("A".into()),
                glyphs: 1
            })
        );
        assert!(matches!(h.undo(&mut f), Some(Affected::Layer { ref glyph, .. }) if glyph == "A"));
        assert_eq!((width(&f, "A"), width(&f, "H")), (a0, h0 + 20.0));
        assert!(!h.can_undo());
        assert!(h.can_redo());
        h.redo(&mut f);
        assert_eq!(width(&f, "A"), a0 + 10.0);
        // Edits in a glyph tab stay off the font's history.
        h.set_context(Scope::Font);
        assert!(!h.can_undo());
    }

    #[test]
    fn batch_edits_undo_per_glyph_or_all_at_once() {
        let mut f = sample_font();
        let original = f.clone();
        let names = ["A", "H", "I", "O", "T"];
        let mut h = History::default();
        // A Font View action on five glyphs: one whole-font change.
        let mut next = f.clone();
        for n in names {
            *next.glyph_mut(n).unwrap().layer_mut("m01").unwrap() = widen(&f, n, 5.0);
        }
        h.apply_font(&mut f, next).unwrap();
        assert_eq!(
            h.undo_info(),
            Some(StepInfo {
                action: "Change Metrics",
                glyph: None,
                glyphs: 5
            })
        );
        // In I's tab only I reverts.
        h.set_context(Scope::Glyph("I".into()));
        h.undo(&mut f);
        assert_eq!(width(&f, "I"), width(&original, "I"));
        assert_eq!(width(&f, "A"), width(&original, "A") + 5.0);
        // Font View undoes the rest in one step; I's own undo isn't repeated.
        h.set_context(Scope::Font);
        assert_eq!(h.undo(&mut f), Some(Affected::Font));
        assert_eq!(f, original);
        // Redo from Font View brings all five back, I included.
        h.redo(&mut f);
        for n in names {
            assert_eq!(width(&f, n), width(&original, n) + 5.0, "{n}");
        }
        // After the group is undone again, H's own step has nothing left to undo.
        h.undo(&mut f);
        h.set_context(Scope::Glyph("H".into()));
        assert_eq!(h.undo(&mut f), None);
        assert_eq!(f, original);
    }

    #[test]
    fn font_steps_keep_later_glyph_edits() {
        let mut f = sample_font();
        let mut h = History::default();
        let next = crate::structure::set_info(&f, "Renamed", 1000, 2, 0).unwrap();
        h.apply_font(&mut f, next).unwrap();
        h.set_context(Scope::Glyph("H".into()));
        let l = widen(&f, "H", 30.0);
        h.apply(&mut f, "H", "m01", l).unwrap();
        h.set_context(Scope::Font);
        assert_eq!(h.undo_info().map(|i| i.action), Some("Font Info"));
        h.undo(&mut f);
        assert_ne!(f.info.family_name, "Renamed");
        assert_eq!(width(&f, "H"), width(&sample_font(), "H") + 30.0);
    }

    #[test]
    fn rename_moves_the_history_and_delete_drops_it() {
        let mut f = sample_font();
        let mut h = History::default();
        h.set_context(Scope::Glyph("A".into()));
        let l = widen(&f, "A", 10.0);
        h.apply(&mut f, "A", "m01", l).unwrap();
        let renames = std::collections::BTreeMap::from([("A".to_string(), "A.alt".to_string())]);
        let (next, _) = crate::rename::rename_glyphs(&f, &renames).unwrap();
        h.rename_glyphs(&renames);
        h.apply_font(&mut f, next).unwrap();
        assert_eq!(h.context(), &Scope::Glyph("A.alt".into()));
        assert!(h.can_undo());
        h.undo(&mut f);
        assert_eq!(width(&f, "A.alt"), width(&sample_font(), "A"));
        // Deleting the glyph drops its history.
        h.redo(&mut f);
        let mut next = f.clone();
        next.glyphs.retain(|g| g.name != "A.alt");
        h.set_context(Scope::Font);
        h.apply_font(&mut f, next).unwrap();
        h.set_context(Scope::Glyph("A.alt".into()));
        assert!(!h.can_undo());
    }

    #[test]
    fn glyph_histories_are_capped() {
        let mut f = sample_font();
        let mut h = History::default();
        h.set_context(Scope::Glyph("H".into()));
        for _ in 0..(MAX_GLYPH_UNDO + 20) {
            let l = widen(&f, "H", 1.0);
            h.apply(&mut f, "H", "m01", l).unwrap();
        }
        let mut n = 0;
        while h.undo(&mut f).is_some() {
            n += 1;
        }
        assert_eq!(n, MAX_GLYPH_UNDO);
        assert_eq!(width(&f, "H"), width(&sample_font(), "H") + 20.0);
    }
}
