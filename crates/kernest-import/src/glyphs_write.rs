//! Writing a Kernest font as a Glyphs 3 `.glyphs` source (OpenStep plist text).
//!
//! glyphs-reader only reads, so the plist is written by hand. The output is shaped
//! to read back through [`crate::glyphs::import`]: masters and axes, glyph layers
//! (master, brace, bracket and smart poles), components with `piece` values, anchors,
//! kerning groups and pairs, metrics keys, names, features and hinting.
//!
//! Glyphs normalises some things on the way back in (feature code is re-wrapped, axis
//! ranges are rebuilt from the master positions), so those round-trip in meaning rather
//! than byte for byte. Guides, backgrounds, glyph tags, notes and locks, line spacing
//! metrics, colour palettes and colour layers are written as Glyphs stores them. A
//! radial gradient's separate focus point has no Glyphs equivalent and is left out.

use std::fmt::Write as _;
use std::path::Path;

use kernest_core::color::Paint;
use kernest_core::{Font, Glyph, Guide, Layer, NodeKind, Shape};

/// Writes `font` as a Glyphs 3 `.glyphs` file at `path`.
pub fn write_glyphs(font: &Font, path: &Path) -> Result<(), String> {
    std::fs::write(path, glyphs_source(font))
        .map_err(|e| format!("could not write \"{}\": {e}", path.display()))
}

/// A number as Glyphs writes it: no trailing `.0` on whole values.
fn num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// A plist string atom, quoted and escaped when it isn't a bare word.
fn q(s: &str) -> String {
    let bare = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_$/:.-".contains(c));
    if bare {
        return s.to_string();
    }
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn node_type(kind: NodeKind, smooth: bool) -> &'static str {
    match (kind, smooth) {
        (NodeKind::Line, false) => "l",
        (NodeKind::Line, true) => "ls",
        (NodeKind::Curve, false) => "c",
        (NodeKind::Curve, true) => "cs",
        (NodeKind::QCurve, false) => "q",
        (NodeKind::QCurve, true) => "qs",
        (NodeKind::OffCurve, _) => "o",
    }
}

/// One path or component as a Glyphs 3 `shapes` entry.
fn shape(s: &Shape) -> String {
    match s {
        Shape::Path(p) => {
            let nodes: Vec<String> = p
                .nodes
                .iter()
                .map(|n| {
                    format!(
                        "({},{},{})",
                        num(n.x),
                        num(n.y),
                        node_type(n.kind, n.smooth)
                    )
                })
                .collect();
            format!(
                "{{\nclosed = {};\nnodes = (\n{}\n);\n}}",
                if p.closed { 1 } else { 0 },
                nodes.join(",\n")
            )
        }
        Shape::Component {
            base,
            transform,
            smart,
            ..
        } => {
            let mut s = format!("{{\nref = {};\n", q(base));
            let [xx, xy, yx, yy, dx, dy] = *transform;
            if (dx, dy) != (0.0, 0.0) {
                let _ = writeln!(s, "pos = ({},{});", num(dx), num(dy));
            }
            // Glyphs stores a component as position, scale and rotation; shear can't be
            // carried, so the matrix is split into a rotation and a (signed) scale.
            if [xx, xy, yx, yy] != [1.0, 0.0, 0.0, 1.0] {
                let sx = (xx * xx + xy * xy).sqrt();
                let (c, sn) = if sx > 0.0 {
                    (xx / sx, xy / sx)
                } else {
                    (1.0, 0.0)
                };
                let sy = c * yy - sn * yx;
                let angle = sn.atan2(c).to_degrees();
                if (sx, sy) != (1.0, 1.0) {
                    let _ = writeln!(s, "scale = ({},{});", num(sx), num(sy));
                }
                if angle != 0.0 {
                    let _ = writeln!(s, "angle = {};", num(angle));
                }
            }
            if !smart.is_empty() {
                let pieces: Vec<String> = smart
                    .iter()
                    .map(|(k, v)| format!("{} = {};", q(k), num(*v)))
                    .collect();
                let _ = writeln!(s, "piece = {{\n{}\n}};", pieces.join("\n"));
            }
            s.push('}');
            s
        }
    }
}

fn anchors(l: &Layer) -> String {
    if l.anchors.is_empty() {
        return String::new();
    }
    let items: Vec<String> = l
        .anchors
        .iter()
        .map(|a| {
            format!(
                "{{\nname = {};\npos = ({},{});\n}}",
                q(&a.name),
                num(a.x),
                num(a.y)
            )
        })
        .collect();
    format!("anchors = (\n{}\n);\n", items.join(",\n"))
}

/// A layer's metrics keys, written on the layer so each master keeps its own.
fn metrics_keys(l: &Layer) -> String {
    let mut s = String::new();
    for (key, val) in [
        ("metricLeft", &l.metrics_keys.left),
        ("metricRight", &l.metrics_keys.right),
        ("metricWidth", &l.metrics_keys.width),
    ] {
        if let Some(v) = val {
            let _ = writeln!(s, "{key} = {};", q(v));
        }
    }
    s
}

/// A `shapes = (...)` list; `attr`, when given, is added to every shape (a colour
/// layer's fill).
fn shapes_list(list: &[Shape], attr: Option<&str>) -> String {
    if list.is_empty() {
        return String::new();
    }
    let items: Vec<String> = list
        .iter()
        .map(|sh| {
            let mut text = shape(sh);
            // A stroked path shows its pen width in Glyphs too.
            let stroke = match sh {
                Shape::Path(p) => p
                    .stroke
                    .as_ref()
                    .map(|s| format!("strokeWidth = {};", num(s.width))),
                _ => None,
            };
            let mask = matches!(sh, Shape::Path(p) if p.mask).then_some("mask = 1;");
            let all: Vec<&str> = attr
                .into_iter()
                .chain(stroke.as_deref())
                .chain(mask)
                .collect();
            if !all.is_empty() {
                text.pop();
                let _ = write!(text, "attr = {{\n{}\n}};\n}}", all.join("\n"));
            }
            text
        })
        .collect();
    format!("shapes = (\n{}\n);\n", items.join(",\n"))
}

/// Guides as a Glyphs `guides` list (on a layer or a master).
pub(crate) fn guides(list: &[Guide]) -> String {
    if list.is_empty() {
        return String::new();
    }
    let items: Vec<String> = list
        .iter()
        .map(|g| {
            let mut e = String::from("{\n");
            if g.angle != 0.0 {
                let _ = writeln!(e, "angle = {};", num(g.angle));
            }
            if g.locked {
                e.push_str("locked = 1;\n");
            }
            if let Some(n) = &g.name {
                let _ = writeln!(e, "name = {};", q(n));
            }
            let _ = writeln!(e, "pos = ({},{});", num(g.x), num(g.y));
            if g.show_measurement {
                e.push_str(
                    "showMeasurement = 1;
",
                );
            }
            e.push('}');
            e
        })
        .collect();
    format!("guides = (\n{}\n);\n", items.join(",\n"))
}

fn background(l: &Layer) -> String {
    let Some(bg) = l
        .background
        .as_ref()
        .filter(|b| !b.shapes.is_empty() || !b.anchors.is_empty())
    else {
        return String::new();
    };
    let mut probe = Layer::new("", 0.0);
    probe.anchors = bg.anchors.clone();
    format!(
        "background = {{\n{}{}}};\n",
        anchors(&probe),
        shapes_list(&bg.shapes, None)
    )
}

/// A layer's body with its corner, cap and segment components: corners become
/// Glyphs corner hints (live in Glyphs too); caps and segments, which Glyphs has
/// no matching form for, are written into the outline, along with any corner on
/// the same path so node numbers stay right.
fn layer_body_parts(font: &Font, l: &Layer) -> String {
    if l.parts.is_empty() {
        return layer_body(l);
    }
    use kernest_core::parts::PartKind;
    let baked_paths: Vec<usize> = l
        .parts
        .iter()
        .filter(|p| p.kind != PartKind::Corner)
        .map(|p| p.path)
        .collect();
    let (live, baked): (Vec<_>, Vec<_>) = l
        .parts
        .iter()
        .cloned()
        .partition(|p| p.kind == PartKind::Corner && !baked_paths.contains(&p.path));
    let mut bake = l.clone();
    bake.parts = baked;
    let mut out = kernest_core::parts::apply(font, &bake);
    out.parts = Vec::new();
    // Path number → shape number, as Glyphs counts them.
    let shape_of: Vec<usize> = out
        .shapes
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, Shape::Path(_)))
        .map(|(i, _)| i)
        .collect();
    let hints: Vec<String> = live
        .iter()
        .filter_map(|p| {
            let si = shape_of.get(p.path)?;
            let options = match p.align {
                kernest_core::parts::CornerAlign::Out => 0,
                kernest_core::parts::CornerAlign::In => 1,
                kernest_core::parts::CornerAlign::Middle => 2,
                kernest_core::parts::CornerAlign::Unaligned => 4,
            };
            Some(format!(
                "{{\nname = {};\noptions = {options};\norigin = ({si},{});\nscale = ({},{});\ntype = Corner;\n}}",
                q(&p.glyph),
                p.node,
                num(p.scale[0]),
                num(p.scale[1])
            ))
        })
        .collect();
    let body = layer_body(&out);
    if hints.is_empty() {
        body
    } else {
        format!("hints = (\n{}\n);\n{body}", hints.join(",\n"))
    }
}

/// The body shared by every kind of layer: width, keys, guides, background, anchors
/// and shapes.
fn layer_body(l: &Layer) -> String {
    layer_body_with(l, None)
}

fn layer_body_with(l: &Layer, attr: Option<&str>) -> String {
    format!(
        "{}{}{}{}{}{}{}{}{}{}width = {};",
        color_label(l.color_label),
        annotations(l),
        background_image(l),
        metrics_keys(l),
        anchors(l),
        background(l),
        guides(&l.guides),
        shapes_list(&l.shapes, attr),
        stroke_data(l),
        vertical(l),
        num(l.width)
    )
}

/// A colour label as Glyphs writes it: `color = 4;` or `color = (r,g,b,a);`.
fn color_label(c: Option<kernest_core::ColorLabel>) -> String {
    match c {
        None => String::new(),
        Some(kernest_core::ColorLabel::Index(i)) => format!(
            "color = {i};
"
        ),
        Some(kernest_core::ColorLabel::Rgba([r, g, b, a])) => {
            format!(
                "color = ({r},{g},{b},{a});
"
            )
        }
    }
}

/// The layer's vertical origin and width (Glyphs' `vertOrigin`, `vertWidth`).
fn vertical(l: &Layer) -> String {
    let mut s = String::new();
    if let Some(v) = l.vert_origin {
        let _ = writeln!(s, "vertOrigin = {};", num(v));
    }
    if let Some(v) = l.vert_width {
        let _ = writeln!(s, "vertWidth = {};", num(v));
    }
    s
}

/// A layer's annotations as Glyphs writes them.
fn annotations(l: &Layer) -> String {
    use kernest_core::AnnotationKind as K;
    if l.annotations.is_empty() {
        return String::new();
    }
    let items: Vec<String> = l
        .annotations
        .iter()
        .map(|a| {
            let kind = match a.kind {
                K::Text => "Text",
                K::Arrow => "Arrow",
                K::Circle => "Circle",
                K::Plus => "Plus",
                K::Minus => "Minus",
            };
            let mut e = String::from("{\n");
            if a.angle != 0.0 {
                let _ = writeln!(e, "angle = {};", num(a.angle));
            }
            let _ = writeln!(e, "pos = ({},{});", num(a.x), num(a.y));
            if !a.text.is_empty() {
                let _ = writeln!(e, "text = {};", q(&a.text));
            }
            let _ = writeln!(e, "type = {kind};");
            if a.size != 0.0 {
                let _ = writeln!(e, "width = {};", num(a.size));
            }
            e.push('}');
            e
        })
        .collect();
    format!("annotations = (\n{}\n);\n", items.join(",\n"))
}

/// A layer's placed image as a Glyphs `backgroundImage`.
fn background_image(l: &Layer) -> String {
    let Some(im) = &l.image else {
        return String::new();
    };
    let r = im.angle.to_radians();
    let t = [
        im.scale_x * r.cos(),
        im.scale_x * r.sin(),
        -im.scale_y * r.sin(),
        im.scale_y * r.cos(),
        im.x,
        im.y,
    ];
    let mut e = String::from("backgroundImage = {\n");
    if let Some(c) = im.crop {
        let _ = writeln!(
            e,
            "crop = \"{{{{{}, {}}}, {{{}, {}}}}}\";",
            num(c[0]),
            num(c[1]),
            num(c[2]),
            num(c[3])
        );
    }
    let _ = writeln!(e, "imagePath = {};", q(&im.path));
    if im.locked {
        e.push_str("locked = 1;\n");
    }
    let _ = writeln!(
        e,
        "transform = \"{{{}, {}, {}, {}, {}, {}}}\";",
        num(t[0]),
        num(t[1]),
        num(t[2]),
        num(t[3]),
        num(t[4]),
        num(t[5])
    );
    e.push_str("};\n");
    e
}

/// Kernest's full stroke settings (caps, joins, pen points, brush) by path number,
/// kept in the layer's userData so they come back exactly.
fn stroke_data(l: &Layer) -> String {
    let strokes: Vec<(usize, &kernest_core::stroke::PathStroke)> = l
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .enumerate()
        .filter_map(|(i, p)| p.stroke.as_ref().map(|s| (i, s)))
        .collect();
    let mut body = String::new();
    if !strokes.is_empty() {
        let json = serde_json::to_string(&strokes).unwrap_or_default();
        body.push_str(&format!("\"{STROKE_KEY}\" = {};\n", q(&json)));
    }
    if !l.hoi.is_empty() {
        let json = serde_json::to_string(&l.hoi).unwrap_or_default();
        body.push_str(&format!("\"{HOI_KEY}\" = {};\n", q(&json)));
    }
    if body.is_empty() {
        return body;
    }
    format!("userData = {{\n{body}}};\n")
}

/// The userData key that holds Kernest's interpolation curves.
pub(crate) const HOI_KEY: &str = "com.kernest.hoi";

/// The userData key that holds Kernest's stroke settings.
pub(crate) const STROKE_KEY: &str = "com.kernest.strokes";

/// The box around a layer's path points (on- and off-curve), which Glyphs gradient
/// positions are relative to. The reader uses the same box, so positions round-trip.
pub(crate) fn point_box(shapes: &[Shape]) -> Option<(f64, f64, f64, f64)> {
    let mut b: Option<(f64, f64, f64, f64)> = None;
    for sh in shapes {
        let Shape::Path(p) = sh else { continue };
        for n in &p.nodes {
            b = Some(match b {
                None => (n.x, n.y, n.x, n.y),
                Some((x0, y0, x1, y1)) => (x0.min(n.x), y0.min(n.y), x1.max(n.x), y1.max(n.y)),
            });
        }
    }
    b
}

/// A palette colour as Glyphs writes it, `(r,g,b,a)`, with `alpha` applied.
fn rgba(font: &Font, index: u16, alpha: f64) -> String {
    let c = font
        .palettes
        .first()
        .and_then(|p| p.get(index as usize))
        .copied()
        .unwrap_or(kernest_core::color::Color::BLACK);
    let a = (f64::from(c.a) * alpha).round().clamp(0.0, 255.0) as u8;
    format!("({},{},{},{a})", c.r, c.g, c.b)
}

/// The shape attributes that fill a full-colour layer with `paint`.
fn paint_attr(font: &Font, l: &Layer, paint: &Paint) -> String {
    let (x0, y0, x1, y1) = point_box(&l.shapes).unwrap_or((0.0, 0.0, 1.0, 1.0));
    let (w, h) = ((x1 - x0).max(1e-9), (y1 - y0).max(1e-9));
    let rel = |p: [f64; 2]| format!("({},{})", num((p[0] - x0) / w), num((p[1] - y0) / h));
    let stops = |list: &[kernest_core::color::ColorStop]| -> String {
        list.iter()
            .map(|st| format!("({},{})", rgba(font, st.color, st.alpha), num(st.offset)))
            .collect::<Vec<_>>()
            .join(",\n")
    };
    match paint {
        Paint::Solid { color, alpha } => format!("fillColor = {};", rgba(font, *color, *alpha)),
        Paint::Linear {
            start,
            end,
            stops: st,
        } => format!(
            "gradient = {{\ncolors = (\n{}\n);\nend = {};\nstart = {};\n}};",
            stops(st),
            rel(*end),
            rel(*start)
        ),
        Paint::Radial {
            center,
            radius,
            stops: st,
            ..
        } => format!(
            "gradient = {{\ncolors = (\n{}\n);\nend = {};\nstart = {};\ntype = circle;\n}};",
            stops(st),
            rel([center[0] + radius, center[1]]),
            rel(*center)
        ),
    }
}

/// A colour layer: a palette layer (`colorPalette`) for an opaque solid colour,
/// otherwise a full-colour layer whose shapes carry the fill or gradient.
fn color_layer(font: &Font, l: &Layer) -> Option<String> {
    let paint = &l.color.as_ref()?.paint;
    let gid = q(&l.master_id);
    let lid = q(l.id.as_deref().unwrap_or(&l.master_id));
    if let Paint::Solid { color, alpha } = paint {
        if *alpha >= 1.0 {
            return Some(format!(
                "{{\nassociatedMasterId = {gid};\nattr = {{\ncolorPalette = {color};\n}};\nlayerId = {lid};\nname = {};\n{}\n}}",
                q(&format!("Color {color}")),
                layer_body(l)
            ));
        }
    }
    let attr = paint_attr(font, l, paint);
    Some(format!(
        "{{\nassociatedMasterId = {gid};\nattr = {{\ncolor = 1;\n}};\nlayerId = {lid};\nname = Color;\n{}\n}}",
        layer_body_with(l, Some(&attr))
    ))
}

/// Palettes as a "Color Palettes" value: a list of palettes of `(r,g,b,a)` colours.
fn palettes_value(list: &[Vec<kernest_core::color::Color>]) -> String {
    let pals: Vec<String> = list
        .iter()
        .map(|p| {
            let cs: Vec<String> = p
                .iter()
                .map(|c| format!("({},{},{},{})", c.r, c.g, c.b, c.a))
                .collect();
            format!("(\n{}\n)", cs.join(",\n"))
        })
        .collect();
    format!("(\n{}\n)", pals.join(",\n"))
}

/// The Glyphs id of a master: Kernest's own stable id.
fn master_gid(m: &kernest_core::Master) -> &str {
    &m.id
}

/// One glyph's layers, in Glyphs form: a layer per master, then smart poles, brace
/// (intermediate) and bracket (alternate) layers. Colour layers are left out.
fn glyph_layers(font: &Font, g: &Glyph) -> String {
    let mut out: Vec<String> = Vec::new();
    for m in &font.masters {
        let Some(l) = g.layer(&m.id).filter(|l| l.is_master()) else {
            continue;
        };
        // A smart glyph's master layer is the minimum pole. Glyphs reads an
        // omitted pole as the maximum, so mark it explicitly or the min and max
        // poles collapse to the same location and fail to interpolate.
        let pole = if g.smart_axes.is_empty() {
            String::new()
        } else {
            let sel: Vec<String> = g
                .smart_axes
                .iter()
                .map(|a| format!("{} = 1;", q(&a.name)))
                .collect();
            format!("partSelection = {{\n{}\n}};\n", sel.join("\n"))
        };
        out.push(format!(
            "{{\nlayerId = {};\n{}{}\n}}",
            q(master_gid(m)),
            pole,
            layer_body_parts(font, l)
        ));
    }
    for l in g.layers.iter().filter(|l| l.color.is_none()) {
        if l.is_master() {
            continue;
        }
        let gid = q(&l.master_id);
        let lid = q(l.id.as_deref().unwrap_or(&l.master_id));
        if let Some(pos) = &l.smart {
            // A smart pole: 1 is the axis minimum, 2 the maximum.
            let sel: Vec<String> = g
                .smart_axes
                .iter()
                .map(|a| {
                    let at_max = pos.get(&a.name).is_some_and(|v| *v >= a.max);
                    format!("{} = {};", q(&a.name), if at_max { 2 } else { 1 })
                })
                .collect();
            out.push(format!(
                "{{\nassociatedMasterId = {gid};\nlayerId = {lid};\npartSelection = {{\n{}\n}};\n{}\n}}",
                sel.join("\n"),
                layer_body_parts(font, l)
            ));
        } else if let Some(loc) = &l.location {
            let coords: Vec<String> = font
                .axes
                .iter()
                .map(|a| {
                    num(loc
                        .get(&a.tag)
                        .copied()
                        .unwrap_or_else(|| kernest_core::designspace::design_bounds(a).1))
                })
                .collect();
            out.push(format!(
                "{{\nassociatedMasterId = {gid};\nattr = {{\ncoordinates = (\n{}\n);\n}};\nlayerId = {lid};\n{}\n}}",
                coords.join(",\n"),
                layer_body_parts(font, l)
            ));
        } else if let Some(name) = &l.backup {
            // A backup: a named layer of its master with no special attributes.
            out.push(format!(
                "{{
associatedMasterId = {gid};
layerId = {lid};
name = {};
{}
}}",
                q(name),
                layer_body_parts(font, l)
            ));
        } else if !l.bracket.is_empty() {
            let rules: Vec<String> = font
                .axes
                .iter()
                .map(|a| {
                    let r = l.bracket.iter().find(|r| r.tag == a.tag);
                    let mut s = String::from("{");
                    if let Some(r) = r {
                        if let Some(v) = r.min {
                            let _ = write!(s, "min = {};", num(v));
                        }
                        if let Some(v) = r.max {
                            let _ = write!(s, "max = {};", num(v));
                        }
                    }
                    s.push('}');
                    s
                })
                .collect();
            out.push(format!(
                "{{\nassociatedMasterId = {gid};\nattr = {{\naxisRules = (\n{}\n);\n}};\nlayerId = {lid};\n{}\n}}",
                rules.join(",\n"),
                layer_body_parts(font, l)
            ));
        }
    }
    // Colour layers, bottom to top, after the others.
    for l in g.layers.iter().filter(|l| l.color.is_some()) {
        if let Some(text) = color_layer(font, l) {
            out.push(text);
        }
    }
    // sbix ("iColor") and SVG layers: the first master's, each holding its picture.
    if let Some(m) = font.masters.first() {
        let gid = q(master_gid(m));
        let width = g.layer(&m.id).map_or(0.0, |l| l.width);
        let picture = |name: &str, attr: &str, path: &str, x: f64, y: f64, scale: f64| {
            format!(
                "{{\nassociatedMasterId = {gid};\n{attr}backgroundImage = {{\nimagePath = {};\ntransform = \"{{{}, 0, 0, {}, {}, {}}}\";\n}};\nlayerId = {};\nname = {};\nwidth = {};\n}}",
                q(path),
                num(scale),
                num(scale),
                num(x),
                num(y),
                q(&format!("{}-{name}", m.id)),
                q(name),
                num(width)
            )
        };
        for b in &g.bitmaps {
            out.push(picture(
                &format!("iColor {}", b.ppem),
                &format!("attr = {{\nsbixSize = {};\n}};\n", b.ppem),
                &b.path,
                b.x,
                b.y,
                font.info.units_per_em as f64 / b.ppem.max(1) as f64,
            ));
        }
        if let Some(svg) = &g.svg {
            out.push(picture("svg", "", &svg.path, svg.x, svg.y, svg.scale));
        }
    }
    out.join(",\n")
}

fn glyph(font: &Font, g: &Glyph) -> String {
    let mut s = String::from("{\n");
    if !g.export {
        s.push_str("export = 0;\n");
    }
    let _ = writeln!(s, "glyphname = {};", q(&g.name));
    match g.unicodes.len() {
        0 => {}
        1 => {
            let _ = writeln!(s, "unicode = {};", g.unicodes[0].0);
        }
        _ => {
            let cps: Vec<String> = g.unicodes.iter().map(|c| c.0.to_string()).collect();
            let _ = writeln!(s, "unicode = (\n{}\n);", cps.join(",\n"));
        }
    }
    if let Some(k) = &g.left_group {
        let _ = writeln!(s, "kernLeft = {};", q(k));
    }
    if let Some(k) = &g.right_group {
        let _ = writeln!(s, "kernRight = {};", q(k));
    }
    if let Some(p) = &g.production_name {
        let _ = writeln!(s, "production = {};", q(p));
    }
    for (key, v) in [
        ("category", &g.category_override),
        ("subCategory", &g.subcategory_override),
        ("case", &g.case_override),
        ("script", &g.script_override),
        ("sortName", &g.sort_name),
    ] {
        if let Some(v) = v {
            let _ = writeln!(s, "{key} = {};", q(v));
        }
    }
    if let Some(d) = g.direction {
        let _ = writeln!(s, "direction = {};", d.as_str());
    }
    s.push_str(&color_label(g.color_label));
    if g.locked {
        s.push_str("locked = 1;\n");
    }
    if let Some(n) = g.note.as_ref().filter(|n| !n.is_empty()) {
        let _ = writeln!(s, "note = {};", q(n));
    }
    if !g.tags.is_empty() {
        let tags: Vec<String> = g.tags.iter().map(|t| q(t)).collect();
        let _ = writeln!(s, "tags = (\n{}\n);", tags.join(",\n"));
    }
    if !g.smart_axes.is_empty() {
        let parts: Vec<String> = g
            .smart_axes
            .iter()
            .map(|a| {
                format!(
                    "{{\nbottomValue = {};\nname = {};\ntopValue = {};\n}}",
                    num(a.min),
                    q(&a.name),
                    num(a.max)
                )
            })
            .collect();
        let _ = writeln!(s, "partsSettings = (\n{}\n);", parts.join(",\n"));
    }
    let _ = writeln!(s, "layers = (\n{}\n);", glyph_layers(font, g));
    s.push('}');
    s
}

/// Splits feature code into the `feature tag { … } tag;` blocks and everything else
/// (languagesystems, classes, lookups), which Glyphs keeps as a prefix.
fn split_features(code: &str) -> (String, Vec<(String, String)>) {
    let mut prefix = String::new();
    let mut blocks = Vec::new();
    let b = code.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if let Some((tag, body, end)) = feature_block(code, i) {
            blocks.push((tag, body));
            i = end;
        } else {
            prefix.push(code[i..].chars().next().unwrap());
            i += code[i..].chars().next().unwrap().len_utf8();
        }
    }
    (prefix, blocks)
}

/// If a `feature <tag> { … } <tag>;` block starts at `i` (on a statement boundary),
/// returns its tag, inner code and the index just past it.
fn feature_block(code: &str, i: usize) -> Option<(String, String, usize)> {
    let rest = &code[i..];
    let boundary = i == 0
        || code[..i]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_whitespace() || c == ';');
    if !boundary || !rest.starts_with("feature") {
        return None;
    }
    let after = &rest["feature".len()..];
    if !after.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let after = after.trim_start();
    let tag_len = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    if tag_len == 0 {
        return None;
    }
    let tag = after[..tag_len].to_string();
    let after = after[tag_len..].trim_start();
    if !after.starts_with('{') {
        return None;
    }
    // Brace-match to the closing brace (feature bodies hold no unbalanced braces).
    let bytes = after.as_bytes();
    let mut depth = 0usize;
    let mut close = None;
    for (j, c) in bytes.iter().enumerate() {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(j);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let body = after[1..close].trim().to_string();
    let mut end = i + (rest.len() - after.len()) + close + 1;
    // Skip the trailing ` tag;`.
    let tail = code[end..].trim_start();
    if let Some(stripped) = tail.strip_prefix(&tag) {
        let stripped = stripped.trim_start();
        if let Some(stripped) = stripped.strip_prefix(';') {
            end = code.len() - stripped.len();
        }
    }
    Some((tag, body, end))
}

fn features(code: &str) -> String {
    let (prefix, blocks) = split_features(code);
    let prefix = prefix.trim();
    let mut s = String::new();
    if !prefix.is_empty() {
        let _ = writeln!(
            s,
            "featurePrefixes = (\n{{\ncode = {};\nname = Prefix;\n}}\n);",
            q(prefix)
        );
    }
    if !blocks.is_empty() {
        let feats: Vec<String> = blocks
            .iter()
            .map(|(tag, body)| format!("{{\ncode = {};\ntag = {};\n}}", q(body), q(tag)))
            .collect();
        let _ = writeln!(s, "features = (\n{}\n);", feats.join(",\n"));
    }
    s
}

/// Glyph processing (SMO-28) as Glyphs custom parameters.
fn processing_parameters(p: &kernest_core::GlyphProcessing) -> Vec<String> {
    let list = |name: &str, items: &[String]| {
        let v: Vec<String> = items.iter().map(|x| q(x)).collect();
        format!(
            "{{\nname = {};\nvalue = (\n{}\n);\n}}",
            q(name),
            v.join(",\n")
        )
    };
    let mut out = Vec::new();
    if !p.keep_glyphs.is_empty() {
        out.push(list("Keep Glyphs", &p.keep_glyphs));
    }
    if !p.decompose_glyphs.is_empty() {
        out.push(list("Decompose Glyphs", &p.decompose_glyphs));
    }
    if p.decompose_components {
        out.push("{\nname = \"Decompose Components in Variable Font\";\nvalue = 1;\n}".into());
    }
    for f in &p.pre_filters {
        out.push(format!("{{\nname = PreFilter;\nvalue = {};\n}}", q(f)));
    }
    for f in &p.filters {
        out.push(format!("{{\nname = Filter;\nvalue = {};\n}}", q(f)));
    }
    if p.keep_overlaps {
        out.push("{\nname = \"Remove Overlap\";\nvalue = 0;\n}".into());
    }
    out
}

/// Feature code parameters (SMO-23) as Glyphs custom parameters: the list ones
/// (Remove Features…) as arrays, the replace ones as `name;` and code.
fn feature_parameters(params: &[kernest_core::CustomParameter]) -> Vec<String> {
    params
        .iter()
        .map(|p| {
            let value = if p.name.starts_with("Remove") {
                let items: Vec<String> = kernest_core::featureedits::list_items(&p.value)
                    .iter()
                    .map(|x| q(x))
                    .collect();
                format!("(\n{}\n)", items.join(",\n"))
            } else {
                q(&p.value)
            };
            format!(
                "{{\n{}name = {};\nvalue = {};\n}}",
                if p.disabled { "disabled = 1;\n" } else { "" },
                q(&p.name),
                value
            )
        })
        .collect()
}

/// The autohinting custom parameter value, from Kernest's hinting settings.
fn hinting_options(h: &kernest_core::Hinting) -> String {
    let mut s = format!(
        "-l {} -r {} -x {}",
        h.range_min, h.range_max, h.increase_x_height
    );
    if h.hint_composites {
        s.push_str(" -c");
    }
    if let Some(n) = h.hinting_limit {
        s.push_str(&format!(" -G {n}"));
    }
    if let Some(n) = h.fallback_stem_width {
        s.push_str(&format!(" -H {n}"));
    }
    for (flag, v) in [
        ("-a", &h.stem_width_mode),
        ("-D", &h.default_script),
        ("-f", &h.fallback_script),
    ] {
        if let Some(v) = v {
            s.push_str(&format!(" {flag} {v}"));
        }
    }
    if let Some(x) = &h.x_height_snapping_exceptions {
        s.push_str(&format!(" -X \"{x}\""));
    }
    for (flag, on) in [
        ("-W", h.windows_compatibility),
        ("-s", h.symbol),
        ("-p", h.adjust_subglyphs),
        ("-S", h.fallback_scaling),
        ("-t", h.ttfa_table),
        ("-n", h.no_info),
    ] {
        if on {
            s.push(' ');
            s.push_str(flag);
        }
    }
    s
}

/// The full `.glyphs` plist text for a font.
pub fn glyphs_source(font: &Font) -> String {
    let info = &font.info;
    let mut s = String::from("{\n.appVersion = \"3260\";\n.formatVersion = 3;\n");

    // Axes: Glyphs keeps only name, tag and hidden; ranges come back from the masters.
    if !font.axes.is_empty() {
        let axes: Vec<String> = font
            .axes
            .iter()
            .map(|a| {
                let mut e = format!("{{\nname = {};\ntag = {};\n", q(&a.name), q(&a.tag));
                if a.hidden {
                    e.push_str("hidden = 1;\n");
                }
                e.push('}');
                e
            })
            .collect();
        let _ = writeln!(s, "axes = (\n{}\n);", axes.join(",\n"));
    }

    // Names: familyName is always top-level; the rest live in properties.
    let _ = writeln!(s, "familyName = {};", q(&info.family_name));
    let mut props: Vec<String> = Vec::new();
    let mut prop = |key: &str, val: &Option<String>| {
        if let Some(v) = val {
            props.push(format!(
                "{{\nkey = {};\nvalues = (\n{{\nlanguage = dflt;\nvalue = {};\n}}\n);\n}}",
                q(key),
                q(v)
            ));
        }
    };
    prop("designers", &info.designer);
    prop("copyrights", &info.copyright);
    prop("designerURL", &info.designer_url);
    prop("manufacturers", &info.manufacturer);
    prop("manufacturerURL", &info.manufacturer_url);
    prop("licenses", &info.license);
    prop("licenseURL", &info.license_url);
    prop("descriptions", &info.description);
    prop("trademarks", &info.trademark);
    prop("sampleTexts", &info.sample_text);
    prop("vendorID", &info.vendor_id);
    if !props.is_empty() {
        let _ = writeln!(s, "properties = (\n{}\n);", props.join(",\n"));
    }

    // Custom parameters: glyph order, the default master, vendor ID and hinting.
    let mut params: Vec<String> = Vec::new();
    let order: Vec<String> = font.glyphs.iter().map(|g| q(&g.name)).collect();
    params.push(format!(
        "{{\nname = glyphOrder;\nvalue = (\n{}\n);\n}}",
        order.join(",\n")
    ));
    let default = kernest_core::interpolate::default_master(font);
    params.push(format!(
        "{{\nname = \"Variable Font Origin\";\nvalue = {};\n}}",
        q(&default.id)
    ));
    if let Some(v) = &info.vendor_id {
        params.push(format!("{{\nname = vendorID;\nvalue = {};\n}}", q(v)));
    }
    if let Some(h) = &info.hinting {
        params.push(format!(
            "{{\nname = \"TTFAutohint options\";\nvalue = {};\n}}",
            q(&hinting_options(h))
        ));
    }
    if !info.control_instructions.is_empty() {
        params.push(format!(
            "{{\nname = \"TTFAutohint control instructions\";\nvalue = {};\n}}",
            q(&info.control_instructions)
        ));
    }
    if let Some(ps) = &info.ps_hinting {
        for (name, v) in [
            ("BlueScale", ps.blue_scale),
            ("BlueShift", ps.blue_shift),
            ("BlueFuzz", ps.blue_fuzz),
        ] {
            params.push(format!("{{\nname = {name};\nvalue = {};\n}}", num(v)));
        }
        for (name, list) in [
            ("familyBlues", &ps.family_blues),
            ("familyOtherBlues", &ps.family_other_blues),
            ("stemSnapH", &ps.stem_snap_h),
            ("stemSnapV", &ps.stem_snap_v),
        ] {
            if !list.is_empty() {
                let v: Vec<String> = list.iter().map(|x| num(*x)).collect();
                params.push(format!(
                    "{{\nname = {name};\nvalue = (\n{}\n);\n}}",
                    v.join(",\n")
                ));
            }
        }
    }
    if info.export_kern_table {
        params.push("{\nname = \"Export kern Table\";\nvalue = 1;\n}".into());
    }
    if !info.gasp.is_empty() {
        let v: Vec<String> = info
            .gasp
            .iter()
            .map(|r| format!("\"{}\" = {};", r.max_ppem, r.flags))
            .collect();
        params.push(format!(
            "{{\nname = \"gasp Table\";\nvalue = {{\n{}\n}};\n}}",
            v.join("\n")
        ));
    }
    let mapped: Vec<&kernest_core::Axis> = font.axes.iter().filter(|a| !a.map.is_empty()).collect();
    if !mapped.is_empty() {
        let entries: Vec<String> = mapped
            .iter()
            .map(|a| {
                let pts: Vec<String> = a
                    .map
                    .iter()
                    .map(|(u, d)| format!("{} = {};", num(*u), num(*d)))
                    .collect();
                format!("{} = {{\n{}\n}};", q(&a.tag), pts.join("\n"))
            })
            .collect();
        params.push(format!(
            "{{\nname = \"Axis Mappings\";\nvalue = {{\n{}\n}};\n}}",
            entries.join("\n")
        ));
    }
    for vm in &info.virtual_masters {
        let entries: Vec<String> = vm
            .location
            .iter()
            .filter_map(|(tag, v)| {
                let axis = font.axes.iter().find(|a| &a.tag == tag)?;
                Some(format!(
                    "{{\nAxis = {};\nLocation = {};\n}}",
                    q(&axis.name),
                    num(*v)
                ))
            })
            .collect();
        params.push(format!(
            "{{\nname = \"Virtual Master\";\nvalue = (\n{}\n);\n}}",
            entries.join(",\n")
        ));
    }
    if info.use_typo_metrics {
        params.push("{\nname = \"Use Typo Metrics\";\nvalue = 1;\n}".to_string());
    }
    if !font.palettes.is_empty() {
        params.push(format!(
            "{{\nname = \"Color Palettes\";\nvalue = {};\n}}",
            palettes_value(&font.palettes)
        ));
    }
    // Parameters kept from the source, unless Kernest writes that one itself above.
    for p in &info.custom_parameters {
        let ours = params
            .iter()
            .any(|e| e.contains(&format!("name = {};", q(&p.name))));
        if ours || p.name.trim().is_empty() {
            continue;
        }
        let value = if p.value.trim().is_empty() {
            "\"\""
        } else {
            p.value.as_str()
        };
        let off = if p.disabled { "disabled = 1;\n" } else { "" };
        params.push(format!(
            "{{\n{off}name = {};\nvalue = {value};\n}}",
            q(&p.name)
        ));
    }
    let _ = writeln!(s, "customParameters = (\n{}\n);", params.join(",\n"));

    let _ = writeln!(s, "unitsPerEm = {};", info.units_per_em);
    let _ = writeln!(s, "versionMajor = {};", info.version_major);
    let _ = writeln!(s, "versionMinor = {};", info.version_minor);

    // Metrics: the six standard types, then a custom entry for every alignment-zone
    // position that isn't a standard metric. Each master's values run in the same order;
    // a zone becomes the overshoot (`over`) on its metric, as Glyphs 3 stores them.
    let std_pos = |m: &kernest_core::Master| -> [f64; 5] {
        let mv = &m.metrics;
        [mv.ascender, mv.cap_height, mv.x_height, 0.0, mv.descender]
    };
    let zone_at = |m: &kernest_core::Master, pos: f64| -> Option<f64> {
        m.alignment_zones
            .iter()
            .find(|z| z.position == pos)
            .map(|z| z.size)
    };
    let mut custom: Vec<f64> = Vec::new();
    for m in &font.masters {
        let std = std_pos(m);
        for z in &m.alignment_zones {
            if !std.contains(&z.position) && !custom.contains(&z.position) {
                custom.push(z.position);
            }
        }
    }
    custom.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let mut metric_defs = vec![
        "{type = ascender;}".to_string(),
        "{type = \"cap height\";}".to_string(),
        "{type = \"x-height\";}".to_string(),
        "{type = baseline;}".to_string(),
        "{type = descender;}".to_string(),
        "{type = \"italic angle\";}".to_string(),
    ];
    for p in &custom {
        metric_defs.push(format!("{{name = {};}}", q(&format!("zone {}", num(*p)))));
    }
    let _ = writeln!(s, "metrics = (\n{}\n);", metric_defs.join(",\n"));

    // Stems: a font-wide name list, then each master's widths in the same order.
    let mut stem_defs: Vec<(String, bool)> = Vec::new();
    for m in &font.masters {
        for st in &m.stems {
            if !stem_defs
                .iter()
                .any(|(n, h)| *n == st.name && *h == st.horizontal)
            {
                stem_defs.push((st.name.clone(), st.horizontal));
            }
        }
    }
    if !stem_defs.is_empty() {
        let defs: Vec<String> = stem_defs
            .iter()
            .map(|(n, h)| {
                if *h {
                    format!("{{name = {}; horizontal = 1;}}", q(n))
                } else {
                    format!("{{name = {};}}", q(n))
                }
            })
            .collect();
        let _ = writeln!(s, "stems = (\n{}\n);", defs.join(",\n"));
    }

    let masters: Vec<String> = font
        .masters
        .iter()
        .map(|m| {
            let vals: Vec<String> = font
                .axes
                .iter()
                .map(|a| {
                    num(m
                        .location
                        .get(&a.tag)
                        .copied()
                        .unwrap_or_else(|| kernest_core::designspace::design_bounds(a).1))
                })
                .collect();
            let mv = &m.metrics;
            let mut mvals: Vec<String> = Vec::new();
            for p in std_pos(m) {
                let pos_part = if p == 0.0 {
                    String::new()
                } else {
                    format!("pos = {};", num(p))
                };
                let over_part = zone_at(m, p)
                    .map(|o| format!("over = {};", num(o)))
                    .unwrap_or_default();
                let body = [pos_part, over_part]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                mvals.push(format!("{{{body}}}"));
            }
            mvals.push(format!("{{pos = {};}}", num(mv.italic_angle)));
            for p in &custom {
                let over_part = zone_at(m, *p)
                    .map(|o| format!(" over = {};", num(o)))
                    .unwrap_or_default();
                mvals.push(format!("{{pos = {};{over_part}}}", num(*p)));
            }
            let mut e = format!(
                "{{\naxesValues = (\n{}\n);\nid = {};\nmetricValues = (\n{}\n);\n",
                vals.join(",\n"),
                q(master_gid(m)),
                mvals.join(",\n")
            );
            if !stem_defs.is_empty() {
                let sv: Vec<String> = stem_defs
                    .iter()
                    .map(|(n, h)| {
                        num(m
                            .stems
                            .iter()
                            .find(|s| s.name == *n && s.horizontal == *h)
                            .map(|s| s.value)
                            .unwrap_or(0.0))
                    })
                    .collect();
                let _ = writeln!(e, "stemValues = (\n{}\n);", sv.join(",\n"));
            }
            // Number values for feature tokens, in the font's number order.
            if !font.info.numbers.is_empty() {
                let nv: Vec<String> = font
                    .info
                    .numbers
                    .values()
                    .map(|by| num(by.get(&m.id).copied().unwrap_or(0.0)))
                    .collect();
                let _ = writeln!(e, "numberValues = (\n{}\n);", nv.join(",\n"));
            }
            // Line spacing and underline, and this master's own palettes.
            let v = &m.vertical;
            let mut mp: Vec<String> = [
                ("typoAscender", v.typo_ascender),
                ("typoDescender", v.typo_descender),
                ("typoLineGap", v.typo_line_gap),
                ("hheaAscender", v.hhea_ascender),
                ("hheaDescender", v.hhea_descender),
                ("hheaLineGap", v.hhea_line_gap),
                ("winAscent", v.win_ascent),
                ("winDescent", v.win_descent),
                ("underlineThickness", v.underline_thickness),
                ("underlinePosition", v.underline_position),
                ("vheaVertAscender", v.vhea_ascender),
                ("vheaVertDescender", v.vhea_descender),
                ("vheaVertLineGap", v.vhea_line_gap),
            ]
            .into_iter()
            .filter_map(|(k, val)| Some(format!("{{\nname = {k};\nvalue = {};\n}}", num(val?))))
            .collect();
            if let Some(p) = font.master_palettes.get(&m.id) {
                mp.push(format!(
                    "{{\nname = \"Color Palettes\";\nvalue = {};\n}}",
                    palettes_value(p)
                ));
            }
            if !mp.is_empty() {
                let _ = writeln!(e, "customParameters = (\n{}\n);", mp.join(",\n"));
            }
            e.push_str(&guides(&m.guides));
            let _ = writeln!(e, "name = {};\n}}", q(&m.name));
            e
        })
        .collect();
    let _ = writeln!(s, "fontMaster = (\n{}\n);", masters.join(",\n"));
    if !font.info.numbers.is_empty() {
        let names: Vec<String> = font
            .info
            .numbers
            .keys()
            .map(|n| format!("{{\nname = {};\n}}", q(n)))
            .collect();
        let _ = writeln!(s, "numbers = (\n{}\n);", names.join(",\n"));
    }

    // Instances: the static instances with their class and style-link overrides, then
    // one "Variable Font Setting" per variable font (its file name and glyph list; axis
    // ranges have no Glyphs equivalent and are left out).
    if !font.instances.is_empty() || !font.variable_fonts.is_empty() {
        let mut insts: Vec<String> =
            font.instances
                .iter()
                .map(|i| {
                    let vals: Vec<String> =
                        font.axes
                            .iter()
                            .map(|a| {
                                num(i.location.get(&a.tag).copied().unwrap_or_else(|| {
                                    kernest_core::designspace::design_bounds(a).1
                                }))
                            })
                            .collect();
                    let mut e = format!("{{\naxesValues = (\n{}\n);\n", vals.join(",\n"));
                    if !i.export {
                        e.push_str("exports = 0;\n");
                    }
                    if let Some(w) = i.weight_class {
                        let _ = writeln!(e, "weightClass = {w};");
                    }
                    if let Some(w) = i.width_class {
                        let _ = writeln!(e, "widthClass = {w};");
                    }
                    if let Some(link) = &i.style_link {
                        if link.bold {
                            e.push_str("isBold = 1;\n");
                        }
                        if link.italic {
                            e.push_str("isItalic = 1;\n");
                        }
                        if !link.base.is_empty() {
                            let _ = writeln!(e, "linkStyle = {};", q(&link.base));
                        }
                    }
                    let mut ip: Vec<String> = Vec::new();
                    if !i.rename_glyphs.is_empty() {
                        let v: Vec<String> = i
                            .rename_glyphs
                            .iter()
                            .map(|[a, b]| q(&format!("{a}={b}")))
                            .collect();
                        ip.push(format!(
                            "{{\nname = \"Rename Glyphs\";\nvalue = (\n{}\n);\n}}",
                            v.join(",\n")
                        ));
                    }
                    if !i.remove_glyphs.is_empty() {
                        let v: Vec<String> = i.remove_glyphs.iter().map(|g| q(g)).collect();
                        ip.push(format!(
                            "{{\nname = \"Remove Glyphs\";\nvalue = (\n{}\n);\n}}",
                            v.join(",\n")
                        ));
                    }
                    if !i.disabled_masters.is_empty() {
                        let v: Vec<String> = i.disabled_masters.iter().map(|m| q(m)).collect();
                        ip.push(format!(
                            "{{\nname = \"Disable Masters\";\nvalue = (\n{}\n);\n}}",
                            v.join(",\n")
                        ));
                    }
                    ip.extend(feature_parameters(&i.feature_parameters));
                    ip.extend(processing_parameters(&i.processing));
                    if !i.autohint {
                        ip.push("{\nname = Autohint;\nvalue = 0;\n}".into());
                    }
                    for tag in &i.names.elidable_stat_axes {
                        ip.push(format!(
                            "{{
name = \"Elidable STAT Axis Value Name\";
value = {};
}}",
                            q(tag)
                        ));
                    }
                    if let Some(f) = &i.names.file_name {
                        ip.push(format!("{{\nname = fileName;\nvalue = {};\n}}", q(f)));
                    }
                    for r in &i.names.records {
                        ip.push(format!(
                            "{{\nname = \"Name Table Entry\";\nvalue = {};\n}}",
                            q(&r.to_glyphs())
                        ));
                    }
                    if !ip.is_empty() {
                        let _ = writeln!(e, "customParameters = (\n{}\n);", ip.join(",\n"));
                    }
                    let _ = writeln!(e, "name = {};", q(&i.name));
                    // The instance's own family name, and its other languages.
                    let mut names: Vec<String> = Vec::new();
                    if let Some(f) = i.family_name.as_deref().filter(|f| !f.trim().is_empty()) {
                        names.push(format!(
                            "{{\nlanguage = dflt;\nvalue = {};\n}}",
                            q(f.trim())
                        ));
                    }
                    for (lang, f) in &i.localized_family_names {
                        names.push(format!(
                            "{{\nlanguage = {};\nvalue = {};\n}}",
                            q(lang),
                            q(f)
                        ));
                    }
                    // Naming properties, in Glyphs' key order.
                    let mut props: Vec<String> = Vec::new();
                    let localized = |key: &str, v: &str| {
                        format!(
                            "{{\nkey = {key};\nvalues = (\n{{\nlanguage = dflt;\nvalue = {};\n}}\n);\n}}",
                            q(v)
                        )
                    };
                    if let Some(v) = &i.names.compatible_full_name {
                        props.push(localized("compatibleFullNames", v));
                    }
                    if !names.is_empty() {
                        props.push(format!(
                            "{{\nkey = familyNames;\nvalues = (\n{}\n);\n}}",
                            names.join(",\n")
                        ));
                    }
                    if let Some(v) = &i.names.postscript_font_name {
                        props.push(format!("{{\nkey = postscriptFontName;\nvalue = {};\n}}", q(v)));
                    }
                    if let Some(v) = &i.names.full_name {
                        props.push(localized("postscriptFullNames", v));
                    }
                    if let Some(v) = &i.names.style_map_family_name {
                        props.push(localized("styleMapFamilyNames", v));
                    }
                    if !props.is_empty() {
                        let _ = writeln!(e, "properties = (\n{}\n);", props.join(",\n"));
                    }
                    e.push('}');
                    e
                })
                .collect();
        for vf in &font.variable_fonts {
            let mut vp = vec![format!(
                "{{\nname = fileName;\nvalue = {};\n}}",
                q(&vf.file_name)
            )];
            if !vf.rename_glyphs.is_empty() {
                let v: Vec<String> = vf
                    .rename_glyphs
                    .iter()
                    .map(|[a, b]| q(&format!("{a}={b}")))
                    .collect();
                vp.push(format!(
                    "{{\nname = \"Rename Glyphs\";\nvalue = (\n{}\n);\n}}",
                    v.join(",\n")
                ));
            }
            if !vf.disabled_masters.is_empty() {
                let v: Vec<String> = vf.disabled_masters.iter().map(|m| q(m)).collect();
                vp.push(format!(
                    "{{\nname = \"Disable Masters\";\nvalue = (\n{}\n);\n}}",
                    v.join(",\n")
                ));
            }
            vp.extend(feature_parameters(&vf.feature_parameters));
            vp.extend(processing_parameters(&vf.processing));
            // Glyphs names the origin by master id.
            if let Some(o) = vf.origin.as_deref().filter(|o| !o.trim().is_empty()) {
                let id = font
                    .masters
                    .iter()
                    .find(|m| m.name == o || m.id == o)
                    .map_or(o, |m| m.id.as_str());
                vp.push(format!(
                    "{{
name = \"Variable Font Origin\";
value = {};
}}",
                    q(id)
                ));
            }
            if !vf.export_stat {
                vp.push(
                    "{
name = \"Export STAT Table\";
value = 0;
}"
                    .into(),
                );
            }
            if !vf.glyphs.is_empty() {
                let names: Vec<String> = vf.glyphs.iter().map(|g| q(g)).collect();
                vp.push(format!(
                    "{{\nname = \"Keep Glyphs\";\nvalue = (\n{}\n);\n}}",
                    names.join(",\n")
                ));
            }
            for r in &vf.name_records {
                vp.push(format!(
                    "{{\nname = \"Name Table Entry\";\nvalue = {};\n}}",
                    q(&r.to_glyphs())
                ));
            }
            let props = vf
                .family_name
                .as_ref()
                .map(|f| {
                    format!(
                        "properties = (\n{{\nkey = familyNames;\nvalues = (\n{{\nlanguage = dflt;\nvalue = {};\n}}\n);\n}}\n);\n",
                        q(f)
                    )
                })
                .unwrap_or_default();
            insts.push(format!(
                "{{\ncustomParameters = (\n{}\n);\nname = {};\n{props}type = variable;\n}}",
                vp.join(",\n"),
                q(&vf.name)
            ));
        }
        let _ = writeln!(s, "instances = (\n{}\n);", insts.join(",\n"));
    }

    // Kerning: master id → left side → right side → value, groups as @MMK_L_/@MMK_R_.
    if !font.kerning.is_empty() {
        let side = |s: &str, left: bool| -> String {
            match s.strip_prefix('@') {
                Some(g) => format!("@MMK_{}_{g}", if left { "L" } else { "R" }),
                None => s.to_string(),
            }
        };
        let mut masters_k: Vec<String> = Vec::new();
        for m in &font.masters {
            let Some(pairs) = font.kerning.get(&m.id) else {
                continue;
            };
            if pairs.is_empty() {
                continue;
            }
            let mut lefts: Vec<(String, Vec<(String, f64)>)> = Vec::new();
            for p in pairs {
                let l = side(&p.left, true);
                let r = side(&p.right, false);
                match lefts.iter_mut().find(|(k, _)| *k == l) {
                    Some((_, rs)) => rs.push((r, p.value)),
                    None => lefts.push((l, vec![(r, p.value)])),
                }
            }
            let left_entries: Vec<String> = lefts
                .iter()
                .map(|(l, rs)| {
                    let rights: Vec<String> = rs
                        .iter()
                        .map(|(r, v)| format!("{} = {};", q(r), num(*v)))
                        .collect();
                    format!("{} = {{\n{}\n}};", q(l), rights.join("\n"))
                })
                .collect();
            masters_k.push(format!(
                "{} = {{\n{}\n}};",
                q(master_gid(m)),
                left_entries.join("\n")
            ));
        }
        if !masters_k.is_empty() {
            let _ = writeln!(s, "kerningLTR = {{\n{}\n}};", masters_k.join("\n"));
        }
    }

    let feat = features(&font.features);
    s.push_str(&feat);

    let glyphs: Vec<String> = font.glyphs.iter().map(|g| glyph(font, g)).collect();
    let _ = writeln!(s, "glyphs = (\n{}\n);", glyphs.join(",\n"));
    s.push_str("}\n");
    s
}
