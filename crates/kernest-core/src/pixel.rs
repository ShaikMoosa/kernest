//! The pixel tool (S59): glyphs drawn from components of a `pixel` glyph placed on
//! the grid. Changing the `pixel` glyph's shape (a square, a dot, a cross) restyles
//! every pixel glyph at once.

use crate::model::{Font, Glyph, Layer, Node, NodeKind, Path, Shape};

type R<T> = Result<T, String>;

/// The glyph the pixel tool places.
pub const PIXEL_GLYPH: &str = "pixel";

/// Adds a non-exported `pixel` glyph: a `size` × `size` square in every master.
pub fn add_pixel_glyph(font: &Font, size: f64) -> R<Font> {
    if font.glyph(PIXEL_GLYPH).is_some() {
        return Err("The font already has a glyph named \"pixel\".".into());
    }
    if !size.is_finite() || size < 1.0 {
        return Err("The pixel size must be at least 1.".into());
    }
    let mut f = font.clone();
    let mut g = Glyph::new(PIXEL_GLYPH, Vec::new());
    g.export = false;
    for m in &f.masters {
        let mut l = Layer::new(m.id.clone(), size);
        l.shapes.push(Shape::Path(Path {
            closed: true,
            nodes: vec![
                Node::new(0.0, 0.0, NodeKind::Line),
                Node::new(size, 0.0, NodeKind::Line),
                Node::new(size, size, NodeKind::Line),
                Node::new(0.0, size, NodeKind::Line),
            ],
            locked: false,
            stroke: None,
        }));
        g.layers.push(l);
    }
    f.glyphs.push(g);
    Ok(f)
}

/// The grid cell (its bottom-left corner) that the point `(x, y)` falls in.
pub fn cell(x: f64, y: f64, size: f64) -> (f64, f64) {
    ((x / size).floor() * size, (y / size).floor() * size)
}

fn pixel_at(s: &Shape, (x, y): (f64, f64)) -> bool {
    matches!(s, Shape::Component { base, transform, .. }
        if base == PIXEL_GLYPH
            && (transform[4] - x).abs() < 1e-6
            && (transform[5] - y).abs() < 1e-6
            && transform[..4] == [1.0, 0.0, 0.0, 1.0])
}

/// Turns pixels on (adds a `pixel` component at each cell that has none) or off
/// (removes them) in one layer. Cells are bottom-left corners.
pub fn set_pixels(font: &Font, glyph: &str, key: &str, cells: &[(f64, f64)], on: bool) -> R<Font> {
    if font.glyph(PIXEL_GLYPH).is_none() {
        return Err("There is no glyph named \"pixel\" to draw with.".into());
    }
    if glyph == PIXEL_GLYPH {
        return Err("The pixel glyph can't be drawn with itself.".into());
    }
    let mut f = font.clone();
    let layer = f
        .glyph_mut(glyph)
        .and_then(|g| g.layer_mut(key))
        .ok_or_else(|| format!("\"{glyph}\" has no layer \"{key}\"."))?;
    for &c in cells {
        let present = layer.shapes.iter().any(|s| pixel_at(s, c));
        if on && !present {
            layer.shapes.push(Shape::component(
                PIXEL_GLYPH,
                [1.0, 0.0, 0.0, 1.0, c.0, c.1],
            ));
        } else if !on && present {
            layer.shapes.retain(|s| !pixel_at(s, c));
        }
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    #[test]
    fn pixels_go_on_and_off_at_grid_cells() {
        let f = sample_font();
        assert!(
            set_pixels(&f, "H", "m01", &[(0.0, 0.0)], true).is_err(),
            "needs the pixel glyph"
        );
        let f = add_pixel_glyph(&f, 50.0).unwrap();
        assert!(!f.glyph(PIXEL_GLYPH).unwrap().export);
        assert_eq!(cell(74.0, -1.0, 50.0), (50.0, -50.0));
        let before = f.glyph("H").unwrap().layer("m01").unwrap().shapes.len();
        let f = set_pixels(
            &f,
            "H",
            "m01",
            &[(50.0, 0.0), (100.0, 0.0), (50.0, 0.0)],
            true,
        )
        .unwrap();
        assert_eq!(
            f.glyph("H").unwrap().layer("m01").unwrap().shapes.len(),
            before + 2
        );
        let f = set_pixels(&f, "H", "m01", &[(50.0, 0.0)], false).unwrap();
        assert_eq!(
            f.glyph("H").unwrap().layer("m01").unwrap().shapes.len(),
            before + 1
        );
        assert!(f.validate().is_ok());
        assert!(add_pixel_glyph(&f, 50.0).is_err());
    }
}
