//! Masks (S29): a path marked as a mask is subtracted from every shape below it in
//! the layer's shape order. It's non-destructive: the paths stay editable, and
//! [`apply`] builds the masked outline wherever it's drawn or exported.

use kurbo::{BezPath, PathEl};
use linesweeper::{binary_op, BinaryOp, FillRule};

use crate::model::{Font, Layer, Shape};
use crate::overlap::{from_bez, to_bez};

/// Whether any path of the layer is a mask.
pub fn has_masks(layer: &Layer) -> bool {
    layer
        .shapes
        .iter()
        .any(|s| matches!(s, Shape::Path(p) if p.mask))
}

/// The layer's outline with its masks applied: every shape resolved to outlines
/// (components, strokes and corner components included), each mask subtracting
/// from everything below it. Shapes above a mask aren't affected. A layer without
/// masks comes back unchanged.
pub fn apply(font: &Font, layer: &Layer) -> Layer {
    if !has_masks(layer) {
        return layer.clone();
    }
    let mut acc = BezPath::new();
    for shape in &layer.shapes {
        let is_mask = matches!(shape, Shape::Path(p) if p.mask);
        let mut one = layer.clone();
        one.parts.clear();
        let mut s = shape.clone();
        if let Shape::Path(p) = &mut s {
            p.mask = false;
        }
        one.shapes = vec![s];
        let outline = outline_of(font, &one);
        if is_mask {
            if !acc.elements().is_empty() {
                acc = boolean(&acc, &outline, BinaryOp::Difference).unwrap_or(acc);
            }
        } else {
            acc.extend(outline.elements().iter().copied());
        }
    }
    let mut out = layer.clone();
    out.parts.clear();
    out.shapes = split(&acc)
        .into_iter()
        .filter_map(|b| from_bez(&b, true))
        .filter(|p| p.nodes.len() > 2)
        .map(Shape::Path)
        .collect();
    out
}

/// A single shape's closed outlines (transformed and resolved) as one kurbo path.
fn outline_of(font: &Font, layer: &Layer) -> BezPath {
    let mut b = BezPath::new();
    crate::geometry::for_each_path(font, layer, &crate::model::IDENTITY, 0, &mut |p, t| {
        if !p.closed {
            return;
        }
        let mut q = p.clone();
        for n in &mut q.nodes {
            let (x, y) = (n.x, n.y);
            n.x = t[0] * x + t[2] * y + t[4];
            n.y = t[1] * x + t[3] * y + t[5];
        }
        if let Some(mut bez) = to_bez(&q) {
            bez.close_path();
            b.extend(bez.elements().iter().copied());
        }
    });
    b
}

fn boolean(a: &BezPath, b: &BezPath, op: BinaryOp) -> Option<BezPath> {
    let r = binary_op(a, b, FillRule::NonZero, op).ok()?;
    let mut out = BezPath::new();
    for c in r.contours() {
        out.extend(c.path.elements().iter().copied());
    }
    Some(out)
}

fn split(b: &BezPath) -> Vec<BezPath> {
    let mut out = Vec::new();
    let mut cur = BezPath::new();
    for el in b.elements() {
        if matches!(el, PathEl::MoveTo(_)) && !cur.elements().is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(*el);
    }
    if !cur.elements().is_empty() {
        out.push(cur);
    }
    out
}

/// Sets whether the `path`-th path (counting paths only) is a mask, in the glyph's
/// masters and intermediate layers alike so they stay compatible.
pub fn set_mask(
    font: &Font,
    glyph: &str,
    key: &str,
    path: usize,
    mask: bool,
) -> Result<Font, String> {
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    if g.layer(key).is_none() {
        return Err("That layer doesn't exist.".into());
    }
    let mut found = false;
    for l in g
        .layers
        .iter_mut()
        .filter(|l| l.color.is_none() && l.backup.is_none())
    {
        if let Some(p) = l
            .shapes
            .iter_mut()
            .filter_map(|s| match s {
                Shape::Path(p) => Some(p),
                _ => None,
            })
            .nth(path)
        {
            p.mask = mask;
            found |= l.key() == key;
        }
    }
    if !found {
        return Err("No path at that position.".into());
    }
    Ok(f)
}

/// Shape Order: puts the layer's shapes in `order` (old indices, each once). With
/// `all_layers`, the same reordering is applied to every compatible layer of the
/// glyph (the masters, intermediate and alternate layers with as many shapes).
pub fn reorder_shapes(
    font: &Font,
    glyph: &str,
    key: &str,
    order: &[usize],
    all_layers: bool,
) -> Result<Font, String> {
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let count = g
        .layer(key)
        .ok_or("That layer doesn't exist.")?
        .shapes
        .len();
    let mut seen = order.to_vec();
    seen.sort_unstable();
    if seen != (0..count).collect::<Vec<_>>() {
        return Err("The new order must list every shape once.".into());
    }
    // Parts refer to paths by number: renumber them with the paths.
    for l in g.layers.iter_mut() {
        let wanted = l.key() == key || (all_layers && l.color.is_none() && l.backup.is_none());
        if !wanted || l.shapes.len() != count {
            continue;
        }
        let old_path_no: Vec<Option<usize>> = {
            let mut k = 0;
            l.shapes
                .iter()
                .map(|s| match s {
                    Shape::Path(_) => {
                        k += 1;
                        Some(k - 1)
                    }
                    _ => None,
                })
                .collect()
        };
        let shapes: Vec<Shape> = order.iter().map(|&i| l.shapes[i].clone()).collect();
        let mut renumber = std::collections::HashMap::new();
        let mut k = 0;
        for &i in order {
            if let Some(old) = old_path_no[i] {
                renumber.insert(old, k);
                k += 1;
            }
        }
        for p in &mut l.parts {
            if let Some(&n) = renumber.get(&p.path) {
                p.path = n;
            }
        }
        l.shapes = shapes;
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, NodeKind, Path};
    use crate::sample::sample_font;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
        Shape::Path(Path {
            closed: true,
            nodes: vec![
                Node::new(x0, y0, NodeKind::Line),
                Node::new(x1, y0, NodeKind::Line),
                Node::new(x1, y1, NodeKind::Line),
                Node::new(x0, y1, NodeKind::Line),
            ],
            locked: false,
            stroke: None,
            mask: false,
        })
    }

    fn area(l: &Layer) -> f64 {
        l.shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Path(p) => to_bez(p),
                _ => None,
            })
            .map(|mut b| {
                b.close_path();
                kurbo::Shape::area(&b).abs()
            })
            .sum()
    }

    #[test]
    fn a_mask_cuts_the_shapes_below_it_only() {
        let mut f = sample_font();
        let l = f.glyph_mut("H").unwrap().layer_mut("m01").unwrap();
        l.shapes = vec![
            rect(0.0, 0.0, 100.0, 100.0),
            rect(50.0, 0.0, 150.0, 100.0),
            rect(200.0, 0.0, 300.0, 100.0),
        ];
        let f = set_mask(&f, "H", "m01", 1, true).unwrap();
        let l = f.glyph("H").unwrap().layer("m01").unwrap();
        assert!(has_masks(l));
        let out = apply(&f, l);
        // The first square loses its right half; the mask itself isn't drawn; the
        // square above the mask (the third) stays whole.
        assert!(
            (area(&out) - (5000.0 + 10000.0)).abs() < 1.0,
            "{}",
            area(&out)
        );
        // Masks are applied when drawing too.
        let b = crate::geometry::bounds(&f, l).unwrap();
        assert_eq!((b.min_x, b.max_x), (0.0, 300.0));
    }

    #[test]
    fn shape_order_moves_shapes_and_their_parts() {
        let mut f = sample_font();
        let l = f.glyph_mut("H").unwrap().layer_mut("m01").unwrap();
        l.shapes = vec![rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 0.0, 30.0, 10.0)];
        l.parts.push(crate::parts::PathPart::new(
            crate::parts::PartKind::Corner,
            "x",
            0,
            1,
        ));
        let f = reorder_shapes(&f, "H", "m01", &[1, 0], false).unwrap();
        let l = f.glyph("H").unwrap().layer("m01").unwrap();
        assert!(matches!(&l.shapes[0], Shape::Path(p) if p.nodes[0].x == 20.0));
        assert_eq!(l.parts[0].path, 1, "the part follows its path");
        assert!(reorder_shapes(&f, "H", "m01", &[0, 0], false).is_err());
    }
}
