//! Editing operations on layers, plus undo/redo history.
//!
//! History stores whole-layer snapshots (before/after). Layers are small, so
//! this is simple and robust: any edit, however complex, undoes exactly.

use crate::error::{Error, Result};
use crate::geometry::bounds;
use crate::model::{Font, Layer, Shape};

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

/// One recorded edit: the layer before and after.
#[derive(Debug, Clone)]
pub struct Change {
    pub glyph: String,
    pub master_id: String,
    pub before: Layer,
    pub after: Layer,
}

/// Undo/redo stacks for a document.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Change>,
    redo: Vec<Change>,
}

/// Maximum undo steps kept per document.
const MAX_UNDO: usize = 500;

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
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
        if after.master_id != master_id {
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
        self.undo.push(Change {
            glyph: glyph.into(),
            master_id: master_id.into(),
            before,
            after,
        });
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(true)
    }

    fn swap(font: &mut Font, c: &Change, to: &Layer) -> Option<(String, String)> {
        let slot = font.glyph_mut(&c.glyph)?.layer_mut(&c.master_id)?;
        *slot = to.clone();
        Some((c.glyph.clone(), c.master_id.clone()))
    }

    /// Undoes the last change; returns the affected (glyph, master id).
    pub fn undo(&mut self, font: &mut Font) -> Option<(String, String)> {
        let c = self.undo.pop()?;
        let hit = Self::swap(font, &c, &c.before);
        self.redo.push(c);
        hit
    }

    /// Redoes the last undone change; returns the affected (glyph, master id).
    pub fn redo(&mut self, font: &mut Font) -> Option<(String, String)> {
        let c = self.redo.pop()?;
        let hit = Self::swap(font, &c, &c.after);
        self.undo.push(c);
        hit
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
    fn undo_redo() {
        let mut f = sample_font();
        let original = f.clone();
        let mut h = History::default();
        let l = with_metric(&f, "H", "m01", MetricEdit::Lsb(100.0)).unwrap();
        h.apply(&mut f, "H", "m01", l).unwrap();
        let edited = f.clone();
        assert_eq!(h.undo(&mut f), Some(("H".into(), "m01".into())));
        assert_eq!(f, original);
        assert!(h.can_redo());
        h.redo(&mut f);
        assert_eq!(f, edited);
        // A new edit clears redo.
        h.undo(&mut f);
        let l = with_metric(&f, "I", "m01", MetricEdit::Width(300.0)).unwrap();
        h.apply(&mut f, "I", "m01", l).unwrap();
        assert!(!h.can_redo());
        assert_eq!(h.undo(&mut f), Some(("I".into(), "m01".into())));
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
        f.glyph_mut("bar").unwrap().layers[0].shapes = vec![Shape::Component {
            base: "l".into(),
            transform: crate::model::IDENTITY,
        }];
        assert!(component_creates_cycle(&f, "l", "bar"));
        assert!(!component_creates_cycle(&f, "bar", "l"));
    }

    #[test]
    fn translate_moves_components_and_anchors() {
        let mut l = Layer::new("m01", 500.0);
        l.shapes.push(Shape::Component {
            base: "a".into(),
            transform: crate::model::IDENTITY,
        });
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
