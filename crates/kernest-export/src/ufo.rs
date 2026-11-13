//! Converting one master of a Kernest font to a UFO (via norad), and writing a designspace.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use kernest_core::{Font, KernPair, Layer, Metrics, NodeKind, Shape};
use norad::{
    AffineTransform, Anchor, Codepoints, Component, Contour, ContourPoint, Glyph, Name, PointType,
};

fn name(s: &str) -> Result<Name, String> {
    Name::new(s).map_err(|e| format!("\"{s}\" can't be used as a glyph or group name: {e}"))
}

fn point_type(kind: NodeKind) -> PointType {
    match kind {
        NodeKind::Line => PointType::Line,
        NodeKind::Curve => PointType::Curve,
        NodeKind::QCurve => PointType::QCurve,
        NodeKind::OffCurve => PointType::OffCurve,
    }
}

/// A UFO glyph for one Kernest layer.
fn glyph(font: &Font, gname: &str, layer: &Layer) -> Result<Glyph, String> {
    let g = font
        .glyph(gname)
        .ok_or_else(|| format!("no glyph named \"{gname}\""))?;
    let mut out = Glyph::new(gname);
    out.width = layer.width;
    out.codepoints = Codepoints::new(g.unicodes.iter().filter_map(|c| c.char()));
    for s in &layer.shapes {
        match s {
            Shape::Path(p) => {
                let mut points: Vec<ContourPoint> = p
                    .nodes
                    .iter()
                    .map(|n| ContourPoint::new(n.x, n.y, point_type(n.kind), n.smooth, None, None))
                    .collect();
                if !p.closed {
                    // In a UFO an open contour starts with a "move" point.
                    if let Some(first) = points.first_mut() {
                        first.typ = PointType::Move;
                    }
                }
                out.contours.push(Contour::new(points, None));
            }
            Shape::Component { base, transform } => {
                let t = AffineTransform {
                    x_scale: transform[0],
                    xy_scale: transform[1],
                    yx_scale: transform[2],
                    y_scale: transform[3],
                    x_offset: transform[4],
                    y_offset: transform[5],
                };
                out.components.push(Component::new(name(base)?, t, None));
            }
        }
    }
    for a in &layer.anchors {
        out.anchors
            .push(Anchor::new(a.x, a.y, Some(name(&a.name)?), None, None));
    }
    Ok(out)
}

/// UFO kerning group names: a glyph's right group is used on the left of a pair (kern1),
/// its left group on the right of a pair (kern2).
fn kern_side(side: &str, first: bool) -> Result<Name, String> {
    match side.strip_prefix('@') {
        Some(g) => name(&format!("public.kern{}.{g}", if first { 1 } else { 2 })),
        None => name(side),
    }
}

/// A box with a counter, used when the font has no `.notdef` glyph of its own.
fn notdef(metrics: &Metrics) -> Glyph {
    let mut g = Glyph::new(".notdef");
    g.width = 500.0;
    let top = metrics.cap_height.max(200.0);
    let p = |x, y| ContourPoint::new(x, y, PointType::Line, false, None, None);
    g.contours.push(Contour::new(
        vec![p(50.0, 0.0), p(450.0, 0.0), p(450.0, top), p(50.0, top)],
        None,
    ));
    g.contours.push(Contour::new(
        vec![
            p(100.0, 50.0),
            p(100.0, top - 50.0),
            p(400.0, top - 50.0),
            p(400.0, 50.0),
        ],
        None,
    ));
    g
}

/// Everything needed to write one master (or one interpolated instance) as a UFO.
pub struct UfoSource<'a> {
    pub font: &'a Font,
    pub style_name: &'a str,
    pub metrics: Metrics,
    /// Layer per glyph name, in glyph order.
    pub layers: Vec<(&'a str, Layer)>,
    pub kerning: Vec<KernPair>,
}

pub fn to_ufo(src: &UfoSource) -> Result<norad::Font, String> {
    let font = src.font;
    let mut ufo = norad::Font::new();
    let info = &mut ufo.font_info;
    info.family_name = Some(font.info.family_name.clone());
    info.style_name = Some(src.style_name.to_string());
    info.units_per_em = Some(u32::from(font.info.units_per_em).into());
    info.version_major = Some(i32::from(font.info.version_major));
    info.version_minor = Some(u32::from(font.info.version_minor));
    info.ascender = Some(src.metrics.ascender);
    info.descender = Some(src.metrics.descender);
    info.cap_height = Some(src.metrics.cap_height);
    info.x_height = Some(src.metrics.x_height);
    info.italic_angle = Some(-src.metrics.italic_angle);
    info.copyright = font.info.copyright.clone();
    info.open_type_name_designer = font.info.designer.clone();
    info.open_type_os2_vendor_id = font.info.vendor_id.clone();

    let has_notdef = src.layers.iter().any(|(n, _)| *n == ".notdef");
    let mut order: Vec<String> = Vec::with_capacity(src.layers.len() + 1);
    if !has_notdef {
        ufo.default_layer_mut().insert_glyph(notdef(&src.metrics));
        order.push(".notdef".into());
    }
    for (gname, layer) in &src.layers {
        ufo.default_layer_mut()
            .insert_glyph(glyph(font, gname, layer)?);
        order.push((*gname).to_string());
    }
    ufo.lib.insert(
        "public.glyphOrder".into(),
        plist::Value::Array(order.into_iter().map(plist::Value::String).collect()),
    );
    let skip: Vec<plist::Value> = font
        .glyphs
        .iter()
        .filter(|g| !g.export)
        .map(|g| plist::Value::String(g.name.clone()))
        .collect();
    if !skip.is_empty() {
        ufo.lib
            .insert("public.skipExportGlyphs".into(), plist::Value::Array(skip));
    }

    // Kerning groups from the glyphs' left/right groups.
    let mut groups: BTreeMap<Name, Vec<Name>> = BTreeMap::new();
    for g in &font.glyphs {
        if let Some(r) = &g.right_group {
            groups
                .entry(kern_side(&format!("@{r}"), true)?)
                .or_default()
                .push(name(&g.name)?);
        }
        if let Some(l) = &g.left_group {
            groups
                .entry(kern_side(&format!("@{l}"), false)?)
                .or_default()
                .push(name(&g.name)?);
        }
    }
    for p in &src.kerning {
        let (l, r) = (kern_side(&p.left, true)?, kern_side(&p.right, false)?);
        // Pairs that refer to groups nobody belongs to would be rejected by the compiler.
        if (p.left.starts_with('@') && !groups.contains_key(&l))
            || (p.right.starts_with('@') && !groups.contains_key(&r))
        {
            continue;
        }
        if !p.left.starts_with('@') && font.glyph(&p.left).is_none()
            || !p.right.starts_with('@') && font.glyph(&p.right).is_none()
        {
            continue;
        }
        ufo.kerning.entry(l).or_default().insert(r, p.value);
    }
    ufo.groups = groups;
    ufo.features = font.features.clone();
    Ok(ufo)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A designspace (format 5) listing the axes, the master UFOs and their locations.
pub fn designspace(font: &Font, sources: &[(String, String, BTreeMap<String, f64>)]) -> String {
    let mut x = String::from(
        "<?xml version='1.0' encoding='UTF-8'?>\n<designspace format=\"5.0\">\n  <axes>\n",
    );
    for a in &font.axes {
        let _ = writeln!(
            x,
            "    <axis tag=\"{}\" name=\"{}\" minimum=\"{}\" default=\"{}\" maximum=\"{}\"{}/>",
            xml_escape(&a.tag),
            xml_escape(&a.name),
            a.min,
            a.default,
            a.max,
            if a.hidden { " hidden=\"1\"" } else { "" }
        );
    }
    x.push_str("  </axes>\n  <sources>\n");
    for (file, style, loc) in sources {
        let _ = write!(
            x,
            "    <source filename=\"{}\" name=\"{}\" stylename=\"{}\"><location>",
            xml_escape(file),
            xml_escape(style),
            xml_escape(style)
        );
        for a in &font.axes {
            let _ = write!(
                x,
                "<dimension name=\"{}\" xvalue=\"{}\"/>",
                xml_escape(&a.name),
                loc.get(&a.tag).copied().unwrap_or(a.default)
            );
        }
        x.push_str("</location></source>\n");
    }
    x.push_str("  </sources>\n  <instances>\n");
    for i in font.instances.iter().filter(|i| i.export) {
        let _ = write!(
            x,
            "    <instance familyname=\"{}\" stylename=\"{}\"><location>",
            xml_escape(&font.info.family_name),
            xml_escape(&i.name)
        );
        for a in &font.axes {
            let _ = write!(
                x,
                "<dimension name=\"{}\" xvalue=\"{}\"/>",
                xml_escape(&a.name),
                i.location.get(&a.tag).copied().unwrap_or(a.default)
            );
        }
        x.push_str("</location></instance>\n");
    }
    x.push_str("  </instances>\n</designspace>\n");
    x
}
