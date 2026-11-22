//! Component from Selection (Glyphs Handbook 9.1.2): the selected paths and components
//! of a glyph become a new glyph, and a component of it takes their place — in every
//! master, intermediate and alternate layer, so the glyph still interpolates.

use crate::glyphset::valid_name;
use crate::model::{Font, Glyph, Layer, Shape};
use crate::transform::Selection;

/// A name for the new glyph: `_part.<glyph>`, numbered if that is taken.
pub fn suggest_name(font: &Font, glyph: &str) -> String {
    let base = format!("_part.{}", glyph.trim_start_matches('_'));
    if font.glyph(&base).is_none() {
        return base;
    }
    (2..)
        .map(|n| format!("{base}.{n}"))
        .find(|n| font.glyph(n).is_none())
        .expect("an unused name")
}

/// Shape indices (into `layer.shapes`) the selection picks: paths with a selected
/// node, then selected components, in drawing order.
fn picked_shapes(layer: &Layer, sel: &Selection) -> Vec<usize> {
    let (mut pi, mut ci) = (0usize, 0usize);
    let mut out = Vec::new();
    for (si, s) in layer.shapes.iter().enumerate() {
        match s {
            Shape::Path(_) => {
                if sel.nodes.iter().any(|&(p, _)| p == pi) {
                    out.push(si);
                }
                pi += 1;
            }
            Shape::Component { .. } => {
                if sel.components.contains(&ci) {
                    out.push(si);
                }
                ci += 1;
            }
        }
    }
    out
}

/// The structure that has to match across layers: for each shape, its node count (or
/// the component's base).
fn shape_key(s: &Shape) -> String {
    match s {
        Shape::Path(p) => format!("p{}{}", p.nodes.len(), if p.closed { "c" } else { "o" }),
        Shape::Component { base, .. } => format!("c{base}"),
    }
}

/// `font` with the selected shapes of `glyph`'s layer `key` moved to a new glyph
/// `name`, replaced by a component of it, in every master, brace and bracket layer.
pub fn component_from_selection(
    font: &Font,
    glyph: &str,
    key: &str,
    sel: &Selection,
    name: &str,
) -> Result<Font, String> {
    let name = name.trim();
    if !valid_name(name) {
        return Err(format!(
            "\"{name}\" can't be used as a glyph name: use letters, digits, . _ and -, starting with a letter or _."
        ));
    }
    if font.glyph(name).is_some() {
        return Err(format!("There is already a glyph named \"{name}\"."));
    }
    let g = font
        .glyph(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    if g.locked {
        return Err(format!("\"{glyph}\" is locked. Unlock it to edit."));
    }
    let current = g
        .layer(key)
        .ok_or_else(|| format!("\"{glyph}\" has no layer \"{key}\"."))?;
    let picked = picked_shapes(current, sel);
    if picked.is_empty() {
        return Err("Select the paths or components to turn into a component.".into());
    }
    if picked
        .iter()
        .any(|&i| matches!(&current.shapes[i], Shape::Path(p) if p.locked))
    {
        return Err("A selected path is locked. Unlock it first.".into());
    }
    // The layers that interpolate together: masters, braces and brackets (backups,
    // colour and smart layers keep their own drawing).
    let structure = |l: &Layer| l.shapes.iter().map(shape_key).collect::<Vec<_>>();
    let want = structure(current);
    let layers: Vec<usize> = g
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            (l.is_master() || l.location.is_some() || !l.bracket.is_empty())
                && l.backup.is_none()
                && l.color.is_none()
                && l.smart.is_none()
        })
        .map(|(i, _)| i)
        .collect();
    for &i in &layers {
        if structure(&g.layers[i]) != want {
            return Err(format!(
                "\"{glyph}\" doesn't have the same paths in every master, so its selection can't become one component. Make the masters compatible first."
            ));
        }
    }

    let mut new_glyph = Glyph::new(name, Vec::new());
    new_glyph.export = !name.starts_with('_');
    let mut next = font.clone();
    let og = next.glyph_mut(glyph).expect("checked above");
    for &i in &layers {
        let l = &mut og.layers[i];
        let mut part = Layer::new(l.master_id.clone(), l.width);
        part.id = l.id.clone();
        part.location = l.location.clone();
        part.bracket = l.bracket.clone();
        part.shapes = picked.iter().map(|&s| l.shapes[s].clone()).collect();
        new_glyph.layers.push(part);
        // The component goes where the first picked shape was.
        let first = picked[0];
        let mut shapes = Vec::with_capacity(l.shapes.len());
        for (si, s) in l.shapes.drain(..).enumerate() {
            if si == first {
                let mut c = Shape::component(name, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                // Placed exactly where the shapes were, not moved by anchors.
                if let Shape::Component { align, .. } = &mut c {
                    *align = false;
                }
                shapes.push(c);
            }
            if !picked.contains(&si) {
                shapes.push(s);
            }
        }
        l.shapes = shapes;
    }
    let at = next
        .glyphs
        .iter()
        .position(|x| x.name == glyph)
        .expect("glyph exists");
    next.glyphs.insert(at + 1, new_glyph);
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, NodeKind, Path};

    fn rect(x: f64, w: f64) -> Shape {
        Shape::Path(Path {
            closed: true,
            nodes: vec![
                Node::new(x, 0.0, NodeKind::Line),
                Node::new(x + w, 0.0, NodeKind::Line),
                Node::new(x + w, 100.0, NodeKind::Line),
                Node::new(x, 100.0, NodeKind::Line),
            ],
            locked: false,
            stroke: None,
            mask: false,
        })
    }

    /// "H" with two masters: stem, bar, stem (the bar is wider in the bold).
    fn font() -> Font {
        let mut f = Font::new("T", []);
        let mut bold = f.masters[0].clone();
        bold.id = "m02".into();
        bold.name = "Bold".into();
        f.masters.push(bold);
        let mut h = Glyph::new("H", Vec::new());
        for (id, bar) in [("m01", 300.0), ("m02", 340.0)] {
            let mut l = Layer::new(id, 600.0);
            l.shapes = vec![rect(0.0, 80.0), rect(80.0, bar), rect(500.0, 80.0)];
            h.layers.push(l);
        }
        f.glyphs = vec![h, Glyph::new("I", Vec::new())];
        f
    }

    fn sel_paths(paths: &[usize]) -> Selection {
        Selection {
            nodes: paths.iter().map(|&p| (p, 0)).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn selected_paths_become_a_component_in_every_master() {
        let f = font();
        assert_eq!(suggest_name(&f, "H"), "_part.H");
        let next = component_from_selection(&f, "H", "m01", &sel_paths(&[1]), "_bar").unwrap();
        // The new glyph sits after H, isn't exported and has the bar in both masters.
        assert_eq!(next.glyphs[1].name, "_bar");
        let bar = next.glyph("_bar").unwrap();
        assert!(!bar.export);
        assert_eq!(bar.layers.len(), 2);
        for (l, w) in bar.layers.iter().zip([300.0, 340.0]) {
            let Shape::Path(p) = &l.shapes[0] else {
                panic!()
            };
            assert_eq!(p.nodes[1].x - p.nodes[0].x, w);
        }
        // H keeps its stems, with the component in the bar's place.
        for l in &next.glyph("H").unwrap().layers {
            assert_eq!(l.shapes.len(), 3);
            assert!(
                matches!(&l.shapes[1], Shape::Component { base, align: false, .. } if base == "_bar")
            );
            assert!(matches!(l.shapes[0], Shape::Path(_)) && matches!(l.shapes[2], Shape::Path(_)));
        }
        // The name is now taken; a numbered one is suggested.
        assert!(component_from_selection(&next, "I", "m01", &sel_paths(&[0]), "_bar").is_err());
        assert_eq!(suggest_name(&next.clone(), "H"), "_part.H");
    }

    #[test]
    fn mistakes_are_refused() {
        let f = font();
        assert!(component_from_selection(&f, "H", "m01", &sel_paths(&[]), "_x").is_err());
        assert!(component_from_selection(&f, "H", "m01", &sel_paths(&[1]), "2bad").is_err());
        assert!(component_from_selection(&f, "H", "m01", &sel_paths(&[1]), "I").is_err());
        // Masters that don't match.
        let mut odd = font();
        odd.glyph_mut("H").unwrap().layers[1].shapes.pop();
        let e = component_from_selection(&odd, "H", "m01", &sel_paths(&[1]), "_bar").unwrap_err();
        assert!(e.contains("same paths in every master"), "{e}");
        // An exported name stays exported.
        let next = component_from_selection(&f, "H", "m01", &sel_paths(&[0, 2]), "stems").unwrap();
        assert!(next.glyph("stems").unwrap().export);
        assert_eq!(next.glyph("stems").unwrap().layers[0].shapes.len(), 2);
        assert_eq!(
            next.glyph("H").unwrap().layers[0].shapes.len(),
            2,
            "component plus the bar"
        );
    }
}
