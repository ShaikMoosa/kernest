//! Editing operations on layers, plus undo/redo history.
//!
//! History stores whole-layer snapshots (before/after). Layers are small, so
//! this is simple and robust: any edit, however complex, undoes exactly.

use crate::error::{Error, Result};
use crate::geometry::bounds;
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
        self.record(Change::Layer {
            glyph: glyph.into(),
            master_id: master_id.into(),
            before: Box::new(before),
            after: Box::new(after),
        });
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
        self.record(Change::Kerning {
            master_id: master_id.into(),
            before,
            after,
        });
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
        self.record(Change::Groups {
            glyph: glyph.into(),
            before,
            after,
        });
        Ok(true)
    }

    fn record(&mut self, c: Change) {
        self.undo.push(c);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(font: &mut Font, c: &Change, forward: bool) -> Option<Affected> {
        match c {
            Change::Layer {
                glyph,
                master_id,
                before,
                after,
            } => {
                let slot = font.glyph_mut(glyph)?.layer_mut(master_id)?;
                *slot = (**if forward { after } else { before }).clone();
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
                *font = (**if forward { after } else { before }).clone();
                Some(Affected::Font)
            }
        }
    }

    /// Replaces the whole font (structural edits: axes, masters, instances, info)
    /// and records the change.
    pub fn apply_font(&mut self, font: &mut Font, after: Font) -> Result<bool> {
        after.validate()?;
        if *font == after {
            return Ok(false);
        }
        let before = std::mem::replace(font, after.clone());
        self.record(Change::Font {
            before: Box::new(before),
            after: Box::new(after),
        });
        Ok(true)
    }

    /// Undoes the last change; returns what it touched.
    pub fn undo(&mut self, font: &mut Font) -> Option<Affected> {
        let c = self.undo.pop()?;
        let hit = Self::restore(font, &c, false);
        self.redo.push(c);
        hit
    }

    /// Redoes the last undone change; returns what it touched.
    pub fn redo(&mut self, font: &mut Font) -> Option<Affected> {
        let c = self.redo.pop()?;
        let hit = Self::restore(font, &c, true);
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
