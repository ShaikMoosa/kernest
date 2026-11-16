//! Smart components: components with their own axes.
//!
//! A *smart glyph* has smart axes (such as "Width" from 0 to 100). Its master layer
//! is the shape at every axis' minimum, and *pole layers* give the shape at other
//! positions (such as Width 100). A component of a smart glyph stores a value per
//! axis, and is drawn as the glyph interpolated between its poles at those values,
//! so one design can serve many glyphs at different sizes.
//!
//! Export turns smart components into outlines first ([`flatten_font`]), so the font
//! compiler only ever sees plain outlines and components.

use std::collections::BTreeMap;

use crate::designspace::{Location, VariationModel};
use crate::interpolate::{compare, rebuild, values, Interpolator};
use crate::model::{Font, Layer, Node, Path, Shape, SmartAxis};

type R<T> = Result<T, String>;

/// Smart-axis values as a normalised location: each axis' minimum is 0, maximum 1.
fn normalise(axes: &[SmartAxis], values: &BTreeMap<String, f64>) -> Location {
    axes.iter()
        .filter_map(|a| {
            let span = a.max - a.min;
            if span <= 0.0 {
                return None;
            }
            let v = values
                .get(&a.name)
                .copied()
                .unwrap_or(a.min)
                .clamp(a.min, a.max);
            let n = (v - a.min) / span;
            (n != 0.0).then(|| (a.name.clone(), n))
        })
        .collect()
}

/// A plain copy of a layer for drawing (no layer id or position).
fn plain(l: &Layer, master_id: &str) -> Layer {
    let mut out = l.clone();
    out.master_id = master_id.to_string();
    out.id = None;
    out.location = None;
    out.bracket.clear();
    out.smart = None;
    out
}

/// The smart glyph `base` for master `master_id` at these axis values, interpolated
/// between the master layer and that master's pole layers. A glyph that isn't smart
/// (or whose poles don't match its master layer) gives its master layer.
pub fn instance(
    font: &Font,
    base: &str,
    master_id: &str,
    smart: &BTreeMap<String, f64>,
) -> Option<Layer> {
    let g = font.glyph(base)?;
    let master = g.layer(master_id)?;
    let poles: Vec<&Layer> = std::iter::once(master)
        .chain(g.smart_layers().filter(|l| l.master_id == master_id))
        .collect();
    if !g.is_smart() || poles.len() == 1 {
        return Some(plain(master, master_id));
    }
    if poles[1..]
        .iter()
        .any(|p| !compare("pole", master, "master", p).is_empty())
    {
        return Some(plain(master, master_id));
    }
    let empty = BTreeMap::new();
    let locs: Vec<Location> = poles
        .iter()
        .map(|l| normalise(&g.smart_axes, l.smart.as_ref().unwrap_or(&empty)))
        .collect();
    let order: Vec<String> = g.smart_axes.iter().map(|a| a.name.clone()).collect();
    let model = VariationModel::new(&locs, &order).ok()?;
    let vals: Vec<Vec<f64>> = poles.iter().map(|l| values(l)).collect();
    let at = model.interpolate(&vals, &normalise(&g.smart_axes, smart));
    Some(plain(&rebuild(master, &at), master_id))
}

/// The smart glyph `base` as used inside `owner` (a layer of another glyph): for a
/// master or alternate layer, that master's instance; for an intermediate layer, the
/// instances of all masters interpolated at its location.
pub fn instance_for(
    font: &Font,
    base: &str,
    owner: &Layer,
    smart: &BTreeMap<String, f64>,
) -> Option<Layer> {
    let Some(loc) = &owner.location else {
        return instance(font, base, &owner.master_id, smart);
    };
    let per_master: Vec<Layer> = font
        .masters
        .iter()
        .map(|m| instance(font, base, &m.id, smart))
        .collect::<Option<_>>()?;
    let template = &per_master[0];
    if per_master[1..]
        .iter()
        .any(|l| !compare("master", template, "master", l).is_empty())
    {
        return Some(template.clone());
    }
    let interp = Interpolator::new(font, loc).ok()?;
    let vals: Vec<Vec<f64>> = per_master.iter().map(values).collect();
    Some(plain(
        &rebuild(template, &interp.values(&vals)),
        &owner.master_id,
    ))
}

fn compose(o: &[f64; 6], i: &[f64; 6]) -> [f64; 6] {
    [
        o[0] * i[0] + o[2] * i[1],
        o[1] * i[0] + o[3] * i[1],
        o[0] * i[2] + o[2] * i[3],
        o[1] * i[2] + o[3] * i[3],
        o[0] * i[4] + o[2] * i[5] + o[4],
        o[1] * i[4] + o[3] * i[5] + o[5],
    ]
}

fn push_flat(
    font: &Font,
    owner: &Layer,
    shapes: &[Shape],
    t: [f64; 6],
    depth: usize,
    out: &mut Vec<Shape>,
) {
    for s in shapes {
        match s {
            Shape::Path(p) => out.push(Shape::Path(Path {
                closed: p.closed,
                nodes: p
                    .nodes
                    .iter()
                    .map(|n| Node {
                        x: t[0] * n.x + t[2] * n.y + t[4],
                        y: t[1] * n.x + t[3] * n.y + t[5],
                        ..*n
                    })
                    .collect(),
            })),
            Shape::Component {
                base,
                transform,
                smart,
            } => {
                let ct = compose(&t, transform);
                let is_smart = font.glyph(base).is_some_and(|g| g.is_smart());
                match (is_smart && depth < 16)
                    .then(|| instance_for(font, base, owner, smart))
                    .flatten()
                {
                    Some(inst) => push_flat(font, owner, &inst.shapes, ct, depth + 1, out),
                    None => out.push(Shape::component(base.clone(), ct)),
                }
            }
        }
    }
}

/// The layer with every smart component replaced by its outlines (other components
/// are kept).
pub fn flatten_layer(font: &Font, layer: &Layer) -> Layer {
    let mut out = layer.clone();
    let mut shapes = Vec::with_capacity(layer.shapes.len());
    push_flat(
        font,
        layer,
        &layer.shapes,
        crate::model::IDENTITY,
        0,
        &mut shapes,
    );
    out.shapes = shapes;
    out
}

/// The font with every smart component turned into outlines, as exported. Smart
/// glyphs themselves keep their master layers; their pole layers are dropped.
pub fn flatten_font(font: &Font) -> Font {
    let uses_smart = |l: &Layer| {
        l.shapes.iter().any(|s| {
            matches!(s, Shape::Component { base, .. } if font.glyph(base).is_some_and(|g| g.is_smart()))
        })
    };
    let mut f = font.clone();
    for g in &mut f.glyphs {
        for l in &mut g.layers {
            if uses_smart(l) {
                *l = flatten_layer(font, l);
            }
        }
        g.layers.retain(|l| l.smart.is_none());
        g.smart_axes.clear();
    }
    f
}

/// Gives `glyph` smart axes (replacing any). Pole positions and component values on
/// axes that no longer exist are dropped.
pub fn set_smart_axes(font: &Font, glyph: &str, axes: Vec<SmartAxis>) -> R<Font> {
    let mut seen = std::collections::HashSet::new();
    for a in &axes {
        if a.name.trim().is_empty() {
            return Err("Every smart axis needs a name.".into());
        }
        if !seen.insert(a.name.as_str()) {
            return Err(format!("The smart axis \"{}\" is used twice.", a.name));
        }
        if !(a.min.is_finite() && a.max.is_finite()) || a.min >= a.max {
            return Err(format!(
                "{}: the minimum must be below the maximum.",
                a.name
            ));
        }
    }
    let mut f = font.clone();
    let names: Vec<String> = axes.iter().map(|a| a.name.clone()).collect();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    g.smart_axes = axes;
    for l in &mut g.layers {
        if let Some(p) = &mut l.smart {
            p.retain(|k, _| names.contains(k));
        }
    }
    // Poles left with no position (or all axes gone) are removed.
    g.layers.retain(|l| {
        l.smart
            .as_ref()
            .is_none_or(|p| !p.is_empty() && !names.is_empty())
    });
    for other in &mut f.glyphs {
        for l in &mut other.layers {
            for s in &mut l.shapes {
                if let Shape::Component { base, smart, .. } = s {
                    if base == glyph {
                        smart.retain(|k, _| names.contains(k));
                    }
                }
            }
        }
    }
    Ok(f)
}

/// Adds a pole layer to a smart glyph for `master_id` at `pole` (axis name → value),
/// starting as a copy of the master layer. Returns the font and the layer's key.
pub fn add_pole(
    font: &Font,
    glyph: &str,
    master_id: &str,
    pole: &BTreeMap<String, f64>,
) -> R<(Font, String)> {
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    if !g.is_smart() {
        return Err("Give the glyph smart axes first.".into());
    }
    let mut pos = BTreeMap::new();
    for a in &g.smart_axes {
        let v = pole.get(&a.name).copied().unwrap_or(a.min);
        if !v.is_finite() || v < a.min || v > a.max {
            return Err(format!(
                "{} must be between {} and {}.",
                a.name, a.min, a.max
            ));
        }
        pos.insert(a.name.clone(), v);
    }
    if g.smart_axes.iter().all(|a| pos[&a.name] == a.min) {
        return Err("The master layer is already the pole at every axis' minimum.".into());
    }
    if g.smart_layers()
        .any(|l| l.master_id == master_id && l.smart.as_ref() == Some(&pos))
    {
        return Err("This glyph already has a pole there.".into());
    }
    let mut l = g
        .layer(master_id)
        .ok_or_else(|| format!("\"{glyph}\" has no layer for that master."))?
        .clone();
    let id = g.new_layer_id(master_id);
    l.id = Some(id.clone());
    l.smart = Some(pos);
    l.location = None;
    l.bracket.clear();
    g.layers.push(l);
    Ok((f, id))
}

/// Sets a component's smart values (component `index` among the layer's components).
pub fn set_component_values(
    font: &Font,
    glyph: &str,
    key: &str,
    index: usize,
    smart: &BTreeMap<String, f64>,
) -> R<Layer> {
    let layer = font
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or_else(|| format!("\"{glyph}\" has no layer \"{key}\"."))?;
    let mut out = layer.clone();
    let comp = out
        .shapes
        .iter_mut()
        .filter(|s| matches!(s, Shape::Component { .. }))
        .nth(index)
        .ok_or("There is no such component.")?;
    let Shape::Component {
        base,
        smart: values,
        ..
    } = comp
    else {
        unreachable!()
    };
    let g = font
        .glyph(base)
        .ok_or_else(|| format!("There is no glyph named \"{base}\"."))?;
    let mut clean = BTreeMap::new();
    for a in &g.smart_axes {
        if let Some(v) = smart.get(&a.name) {
            if !v.is_finite() {
                return Err(format!("{}: enter a number.", a.name));
            }
            clean.insert(a.name.clone(), v.clamp(a.min, a.max));
        }
    }
    *values = clean;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::bounds;
    use crate::model::NodeKind;
    use crate::sample::sample_font;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
        Shape::Path(Path {
            closed: true,
            nodes: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                .into_iter()
                .map(|(x, y)| Node::new(x, y, NodeKind::Line))
                .collect(),
        })
    }

    /// A smart "_stem": 80 wide, Height 0..100 → 300..700 units tall.
    fn with_stem() -> Font {
        use crate::glyphset::{add_glyphs, NewGlyph};
        let f = sample_font();
        let f = add_glyphs(
            &f,
            &[NewGlyph {
                name: "_stem".into(),
                codepoint: None,
            }],
        )
        .unwrap()
        .font;
        let axes = vec![SmartAxis {
            name: "Height".into(),
            min: 0.0,
            max: 100.0,
        }];
        let mut f = set_smart_axes(&f, "_stem", axes).unwrap();
        f.glyph_mut("_stem")
            .unwrap()
            .layer_mut("m01")
            .unwrap()
            .shapes = vec![rect(0.0, 0.0, 80.0, 300.0)];
        let (mut f, pole) = add_pole(
            &f,
            "_stem",
            "m01",
            &BTreeMap::from([("Height".into(), 100.0)]),
        )
        .unwrap();
        f.glyph_mut("_stem")
            .unwrap()
            .layer_mut(&pole)
            .unwrap()
            .shapes = vec![rect(0.0, 0.0, 80.0, 700.0)];
        f.validate().unwrap();
        f
    }

    #[test]
    fn components_take_their_shape_from_the_poles() {
        let mut f = with_stem();
        let i = f.glyph_mut("I").unwrap().layer_mut("m01").unwrap();
        i.shapes = vec![Shape::Component {
            base: "_stem".into(),
            transform: [1.0, 0.0, 0.0, 1.0, 85.0, 0.0],
            smart: BTreeMap::from([("Height".into(), 50.0)]),
        }];
        // Height 50 → halfway between 300 and 700 tall.
        let b = bounds(&f, f.glyph("I").unwrap().layer("m01").unwrap()).unwrap();
        assert_eq!(
            (b.min_x, b.min_y, b.max_x, b.max_y),
            (85.0, 0.0, 165.0, 500.0)
        );
        // Changing the value through the edit function.
        let l = set_component_values(
            &f,
            "I",
            "m01",
            0,
            &BTreeMap::from([("Height".into(), 200.0)]),
        )
        .unwrap();
        f.glyph_mut("I").unwrap().layers[0] = l;
        let b = bounds(&f, f.glyph("I").unwrap().layer("m01").unwrap()).unwrap();
        assert_eq!(b.max_y, 700.0, "values are clamped to the axis");
        // Flattening for export gives the same outline as a plain path.
        let flat = flatten_font(&f);
        let l = flat.glyph("I").unwrap().layer("m01").unwrap();
        let Shape::Path(p) = &l.shapes[0] else {
            panic!("expected an outline")
        };
        let top = p.nodes.iter().map(|n| n.y).fold(f64::MIN, f64::max);
        assert_eq!(top, 700.0);
        assert!(flat.glyph("_stem").unwrap().smart_layers().next().is_none());
        flat.validate().unwrap();
    }

    #[test]
    fn axes_and_poles_are_checked() {
        let f = with_stem();
        assert!(
            add_pole(
                &f,
                "_stem",
                "m01",
                &BTreeMap::from([("Height".into(), 100.0)])
            )
            .is_err(),
            "twice"
        );
        assert!(
            add_pole(&f, "_stem", "m01", &BTreeMap::new()).is_err(),
            "the master is the minimum pole"
        );
        assert!(
            add_pole(&f, "H", "m01", &BTreeMap::new()).is_err(),
            "not smart"
        );
        let bad = vec![SmartAxis {
            name: "W".into(),
            min: 5.0,
            max: 5.0,
        }];
        assert!(set_smart_axes(&f, "_stem", bad).is_err());
        // Removing the axis removes the pole.
        let f2 = set_smart_axes(&f, "_stem", vec![]).unwrap();
        assert_eq!(f2.glyph("_stem").unwrap().smart_layers().count(), 0);
        f2.validate().unwrap();
    }
}
