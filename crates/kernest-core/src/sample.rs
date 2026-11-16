//! A small built-in sample font ("Kernest Sample") used by the Start window's
//! "Explore a sample font" action, by tests and by the UI preview.
//!
//! Outlines are simple geometric shapes so the sample stays tiny and licence-free.

use crate::glyphdata;
use crate::model::{Codepoint, Font, Layer, Node, NodeKind, Path, Shape};

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
    // Counter-clockwise outer contour (PostScript direction).
    Shape::Path(Path {
        closed: true,
        nodes: vec![
            Node::new(x0, y0, NodeKind::Line),
            Node::new(x1, y0, NodeKind::Line),
            Node::new(x1, y1, NodeKind::Line),
            Node::new(x0, y1, NodeKind::Line),
        ],
    })
}

/// An ellipse made of four cubic segments; `clockwise` for counters.
fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64, clockwise: bool) -> Shape {
    const K: f64 = 0.5523;
    let (kx, ky) = (rx * K, ry * K);
    let c = |x, y| Node {
        x,
        y,
        kind: NodeKind::Curve,
        smooth: true,
    };
    let o = |x, y| Node::new(x, y, NodeKind::OffCurve);
    let mut nodes = vec![
        c(cx + rx, cy),
        o(cx + rx, cy + ky),
        o(cx + kx, cy + ry),
        c(cx, cy + ry),
        o(cx - kx, cy + ry),
        o(cx - rx, cy + ky),
        c(cx - rx, cy),
        o(cx - rx, cy - ky),
        o(cx - kx, cy - ry),
        c(cx, cy - ry),
        o(cx + kx, cy - ry),
        o(cx + rx, cy - ky),
    ];
    if clockwise {
        nodes.reverse();
    }
    // A closed path conventionally ends on its start point's incoming segment.
    nodes.rotate_left(1);
    Shape::Path(Path {
        closed: true,
        nodes,
    })
}

fn glyph(font: &mut Font, cp: u32, width: f64, shapes: Vec<Shape>) {
    let name = glyphdata::name_for(cp);
    let master = font.masters[0].id.clone();
    let g = match font.glyph_mut(&name) {
        Some(g) => g,
        None => {
            font.glyphs
                .push(crate::model::Glyph::new(&name, vec![Codepoint(cp)]));
            font.glyphs.last_mut().unwrap()
        }
    };
    let mut layer = Layer::new(master, width);
    layer.shapes = shapes;
    g.layers = vec![layer];
}

/// The built-in sample font: Latin Basic with a handful of drawn glyphs.
pub fn sample_font() -> Font {
    let mut font = Font::new(
        "Kernest Sample",
        glyphdata::codepoints_for(&["latin-basic"]),
    );
    let (cap, xh, st) = (700.0, 500.0, 90.0);

    glyph(
        &mut font,
        'H' as u32,
        680.0,
        vec![
            rect(80.0, 0.0, 80.0 + st, cap),
            rect(600.0 - st, 0.0, 600.0, cap),
            rect(80.0, 310.0, 600.0, 310.0 + 80.0),
        ],
    );
    glyph(
        &mut font,
        'I' as u32,
        250.0,
        vec![rect(80.0, 0.0, 80.0 + st, cap)],
    );
    glyph(
        &mut font,
        'L' as u32,
        540.0,
        vec![
            rect(80.0, 0.0, 80.0 + st, cap),
            rect(80.0, 0.0, 500.0, 80.0),
        ],
    );
    glyph(
        &mut font,
        'T' as u32,
        600.0,
        vec![
            rect(255.0, 0.0, 255.0 + st, cap - 80.0),
            rect(40.0, cap - 80.0, 560.0, cap),
        ],
    );
    glyph(
        &mut font,
        'E' as u32,
        560.0,
        vec![
            rect(80.0, 0.0, 80.0 + st, cap),
            rect(80.0, 0.0, 510.0, 80.0),
            rect(80.0, 310.0, 470.0, 390.0),
            rect(80.0, cap - 80.0, 510.0, cap),
        ],
    );
    glyph(
        &mut font,
        'O' as u32,
        760.0,
        vec![
            ellipse(380.0, 350.0, 320.0, 362.0, false),
            ellipse(380.0, 350.0, 230.0, 278.0, true),
        ],
    );
    glyph(
        &mut font,
        'o' as u32,
        580.0,
        vec![
            ellipse(290.0, 250.0, 240.0, 260.0, false),
            ellipse(290.0, 250.0, 155.0, 180.0, true),
        ],
    );
    glyph(
        &mut font,
        'l' as u32,
        240.0,
        vec![rect(75.0, 0.0, 75.0 + 85.0, 760.0)],
    );
    glyph(
        &mut font,
        'i' as u32,
        240.0,
        vec![rect(75.0, 0.0, 160.0, xh), rect(75.0, 600.0, 160.0, 700.0)],
    );
    glyph(
        &mut font,
        'n' as u32,
        600.0,
        vec![
            rect(75.0, 0.0, 160.0, xh),
            rect(440.0, 0.0, 525.0, 380.0),
            rect(75.0, 380.0, 525.0, xh),
        ],
    );
    glyph(
        &mut font,
        '.' as u32,
        240.0,
        vec![rect(75.0, 0.0, 165.0, 90.0)],
    );
    glyph(
        &mut font,
        '-' as u32,
        380.0,
        vec![rect(60.0, 250.0, 320.0, 330.0)],
    );

    font.kerning.insert(
        font.masters[0].id.clone(),
        vec![
            crate::model::KernPair {
                left: "T".into(),
                right: "o".into(),
                value: -60.0,
            },
            crate::model::KernPair {
                left: "L".into(),
                right: "T".into(),
                value: -80.0,
            },
        ],
    );
    font.features = "languagesystem DFLT dflt;\nlanguagesystem latn dflt;\n".into();
    font
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{bounds, sidebearings, svg_path};

    #[test]
    fn sample_is_valid() {
        sample_font().validate().unwrap();
    }

    #[test]
    fn metrics_of_drawn_glyphs() {
        let f = sample_font();
        let h = f.glyph("H").unwrap().layer("m01").unwrap();
        assert_eq!(sidebearings(&f, h), Some((80.0, 80.0)));
        let o = f.glyph("O").unwrap().layer("m01").unwrap();
        let b = bounds(&f, o).unwrap();
        assert!(
            (b.min_x - 60.0).abs() < 0.01 && (b.max_y - 712.0).abs() < 0.01,
            "{b:?}"
        );
        let space = f.glyph("space").unwrap().layer("m01").unwrap();
        assert_eq!(sidebearings(&f, space), None);
    }

    #[test]
    fn svg_output() {
        let f = sample_font();
        let i = f.glyph("I").unwrap().layer("m01").unwrap();
        assert_eq!(svg_path(&f, i), "M80 0L170 0L170 700L80 700L80 0Z");
        let o = f.glyph("o").unwrap().layer("m01").unwrap();
        assert_eq!(svg_path(&f, o).matches('C').count(), 8);
    }

    #[test]
    fn components_resolve() {
        let mut f = sample_font();
        let g = f.glyph_mut("bar").unwrap();
        g.layers[0].shapes = vec![Shape::component("l", [1.0, 0.0, 0.0, 1.0, 10.0, -100.0])];
        let bar = f.glyph("bar").unwrap().layer("m01").unwrap();
        let b = bounds(&f, bar).unwrap();
        assert_eq!((b.min_x, b.min_y, b.max_y), (85.0, -100.0, 660.0));
        f.validate().unwrap();
    }
}
