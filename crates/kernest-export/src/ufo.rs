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

/// A UFO glyph for one Kernest layer. `gname` may also be a generated glyph (such as a
/// bracket alternate), which has no code points.
pub fn glyph(font: &Font, gname: &str, layer: &Layer) -> Result<Glyph, String> {
    let mut out = Glyph::new(gname);
    out.width = layer.width;
    if let Some(g) = font.glyph(gname) {
        out.codepoints = Codepoints::new(g.unicodes.iter().filter_map(|c| c.char()));
    }
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
    pub layers: Vec<(String, Layer)>,
    pub kerning: Vec<KernPair>,
    /// Generated glyphs (bracket alternates) and the glyph whose kerning groups they share.
    pub alternates: Vec<(String, String)>,
    /// Where this source sits in the design space (for the OS/2 weight and width class).
    pub location: BTreeMap<String, f64>,
}

/// OS/2 weight class (1–1000) for a style: the `wght` location if the font has that
/// axis, otherwise from the style name (Thin 100 … Black 900), otherwise 400.
pub fn weight_class(font: &Font, location: &BTreeMap<String, f64>, style: &str) -> u32 {
    if let Some(a) = font.axes.iter().find(|a| a.tag == "wght") {
        let v = location.get("wght").copied().unwrap_or(a.default);
        return v.round().clamp(1.0, 1000.0) as u32;
    }
    let s: String = style
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    const NAMES: [(&str, u32); 14] = [
        ("hairline", 100),
        ("thin", 100),
        ("extralight", 200),
        ("ultralight", 200),
        ("semibold", 600),
        ("demibold", 600),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("light", 300),
        ("medium", 500),
        ("bold", 700),
        ("black", 900),
        ("heavy", 900),
        ("regular", 400),
    ];
    NAMES
        .iter()
        .find(|(n, _)| s.contains(n))
        .map(|(_, w)| *w)
        .unwrap_or(400)
}

/// OS/2 width class (1–9) from the `wdth` location (percent of normal), if the font
/// has that axis.
pub fn width_class(font: &Font, location: &BTreeMap<String, f64>) -> Option<u8> {
    let a = font.axes.iter().find(|a| a.tag == "wdth")?;
    let v = location.get("wdth").copied().unwrap_or(a.default);
    const STEPS: [(f64, u8); 9] = [
        (50.0, 1),
        (62.5, 2),
        (75.0, 3),
        (87.5, 4),
        (100.0, 5),
        (112.5, 6),
        (125.0, 7),
        (150.0, 8),
        (200.0, 9),
    ];
    STEPS
        .iter()
        .min_by(|a, b| (a.0 - v).abs().total_cmp(&(b.0 - v).abs()))
        .map(|s| s.1)
}

fn os2_width(class: u8) -> norad::fontinfo::Os2WidthClass {
    use norad::fontinfo::Os2WidthClass::*;
    match class {
        1 => UltraCondensed,
        2 => ExtraCondensed,
        3 => Condensed,
        4 => SemiCondensed,
        6 => SemiExpanded,
        7 => Expanded,
        8 => ExtraExpanded,
        9 => UltraExpanded,
        _ => Normal,
    }
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
    info.open_type_os2_weight_class = Some(weight_class(font, &src.location, src.style_name));
    info.open_type_os2_width_class = width_class(font, &src.location).map(os2_width);

    let has_notdef = src.layers.iter().any(|(n, _)| n == ".notdef");
    let mut order: Vec<String> = Vec::with_capacity(src.layers.len() + 1);
    if !has_notdef {
        ufo.default_layer_mut().insert_glyph(notdef(&src.metrics));
        order.push(".notdef".into());
    }
    for (gname, layer) in &src.layers {
        ufo.default_layer_mut()
            .insert_glyph(glyph(font, gname, layer)?);
        order.push(gname.clone());
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
    // Alternates kern like the glyph they replace.
    for (alt, base) in &src.alternates {
        if let Some(g) = font.glyph(base) {
            if let Some(r) = &g.right_group {
                groups
                    .entry(kern_side(&format!("@{r}"), true)?)
                    .or_default()
                    .push(name(alt)?);
            }
            if let Some(l) = &g.left_group {
                groups
                    .entry(kern_side(&format!("@{l}"), false)?)
                    .or_default()
                    .push(name(alt)?);
            }
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

/// A source in a designspace: a UFO (or one layer of it, for sparse brace sources).
pub struct DsSource {
    pub file: String,
    pub style: String,
    pub location: BTreeMap<String, f64>,
    pub layer: Option<String>,
}

/// A designspace rule: use `with` instead of `glyph` inside all of `ranges`.
pub struct DsRule {
    pub glyph: String,
    pub with: String,
    pub ranges: Vec<kernest_core::AxisRange>,
}

/// A designspace (format 5) listing the axes, rules, sources and instances.
pub fn designspace(font: &Font, sources: &[DsSource], rules: &[DsRule]) -> String {
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
    x.push_str("  </axes>\n");
    if !rules.is_empty() {
        x.push_str("  <rules>\n");
        for r in rules {
            let _ = write!(
                x,
                "    <rule name=\"{}\"><conditionset>",
                xml_escape(&r.with)
            );
            for range in &r.ranges {
                let Some(a) = font.axes.iter().find(|a| a.tag == range.tag) else {
                    continue;
                };
                // Open ends are left out (an open range keeps going past the axis).
                let bound = |attr: &str, v: Option<f64>| {
                    v.map(|v| format!(" {attr}=\"{v}\"")).unwrap_or_default()
                };
                let _ = write!(
                    x,
                    "<condition name=\"{}\"{}{}/>",
                    xml_escape(&a.name),
                    bound("minimum", range.min),
                    bound("maximum", range.max)
                );
            }
            let _ = writeln!(
                x,
                "</conditionset><sub name=\"{}\" with=\"{}\"/></rule>",
                xml_escape(&r.glyph),
                xml_escape(&r.with)
            );
        }
        x.push_str("  </rules>\n");
    }
    x.push_str("  <sources>\n");
    for (n, s) in sources.iter().enumerate() {
        let (file, style, loc) = (&s.file, &s.style, &s.location);
        let layer = s
            .layer
            .as_ref()
            .map(|l| format!(" layer=\"{}\"", xml_escape(l)))
            .unwrap_or_default();
        let _ = write!(
            x,
            "    <source filename=\"{}\" name=\"{}\" stylename=\"{}\"{layer}><location>",
            xml_escape(file),
            xml_escape(&format!("{style} {n}")),
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
