//! Vertical metrics for vertical typesetting (Glyphs' `vertWidth` and `vertOrigin`).
//!
//! The defaults follow glyphsLib, which export also uses: the vertical advance is the
//! master's typo ascender minus its typo descender, and the vertical origin sits at the
//! typo ascender, `vertOrigin` units below it when set.

use crate::geometry;
use crate::model::{Font, Layer, Master};

/// The master's typo ascender and descender (its ascender and descender when unset).
fn typo(master: &Master) -> (f64, f64) {
    (
        master
            .vertical
            .typo_ascender
            .unwrap_or(master.metrics.ascender),
        master
            .vertical
            .typo_descender
            .unwrap_or(master.metrics.descender),
    )
}

/// The layer's vertical origin y and vertical advance.
pub fn origin_and_advance(master: &Master, layer: &Layer) -> (f64, f64) {
    let (asc, desc) = typo(master);
    (
        asc - layer.vert_origin.unwrap_or(0.0),
        layer.vert_width.unwrap_or(asc - desc),
    )
}

/// The layer's top and bottom sidebearings (TSB, BSB), or `None` for an empty layer.
pub fn sidebearings(font: &Font, master: &Master, layer: &Layer) -> Option<(f64, f64)> {
    let (origin, advance) = origin_and_advance(master, layer);
    geometry::bounds(font, layer).map(|b| (origin - b.max_y, b.min_y - (origin - advance)))
}

/// Sets the layer's top sidebearing by moving its vertical origin; the bottom
/// sidebearing stays where it was. `None` for an empty layer.
pub fn set_tsb(font: &Font, master: &Master, layer: &mut Layer, tsb: f64) -> Option<()> {
    let (old_tsb, _) = sidebearings(font, master, layer)?;
    let (asc, _) = typo(master);
    let (origin, advance) = origin_and_advance(master, layer);
    // The origin moves up by the change; the advance grows by as much, so the
    // bottom of the vertical advance stays put.
    let d = tsb - old_tsb;
    layer.vert_origin = Some(asc - (origin + d));
    layer.vert_width = Some(advance + d);
    Some(())
}

/// Sets the layer's bottom sidebearing by changing its vertical advance. `None` for
/// an empty layer.
pub fn set_bsb(font: &Font, master: &Master, layer: &mut Layer, bsb: f64) -> Option<()> {
    let (_, old_bsb) = sidebearings(font, master, layer)?;
    let (_, advance) = origin_and_advance(master, layer);
    layer.vert_width = Some(advance + (bsb - old_bsb));
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    #[test]
    fn defaults_and_edits() {
        let mut f = sample_font();
        let m = f.masters[0].clone();
        let asc = m.vertical.typo_ascender.unwrap_or(m.metrics.ascender);
        let desc = m.vertical.typo_descender.unwrap_or(m.metrics.descender);
        let l = f.glyph("H").unwrap().layers[0].clone();
        let b = geometry::bounds(&f, &l).unwrap();
        // Defaults: origin at the typo ascender, advance ascender − descender.
        assert_eq!(origin_and_advance(&m, &l), (asc, asc - desc));
        let (tsb, bsb) = sidebearings(&f, &m, &l).unwrap();
        assert_eq!(tsb, asc - b.max_y);
        assert_eq!(bsb, b.min_y - desc);
        // Moving the top keeps the bottom; moving the bottom keeps the top.
        let mut l2 = l.clone();
        set_tsb(&f, &m, &mut l2, tsb + 30.0).unwrap();
        let (t2, b2) = sidebearings(&f, &m, &l2).unwrap();
        assert!((t2 - (tsb + 30.0)).abs() < 1e-9 && (b2 - bsb).abs() < 1e-9);
        assert_eq!(l2.vert_origin, Some(-30.0));
        set_bsb(&f, &m, &mut l2, bsb + 20.0).unwrap();
        let (t3, b3) = sidebearings(&f, &m, &l2).unwrap();
        assert!((t3 - (tsb + 30.0)).abs() < 1e-9 && (b3 - (bsb + 20.0)).abs() < 1e-9);
        let g = f.glyph_mut("H").unwrap();
        g.layers[0] = l2;
        assert!(f.uses_vertical_metrics());
    }
}
