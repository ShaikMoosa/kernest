use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use kernest_core::sample::sample_font;
use kernest_core::{
    geometry, layers, structure, Axis, AxisRange, AxisValue, Font, Instance, KernPair, NodeKind,
    Shape, VariableAxisSubset, VariableFontSetting,
};
use kernest_import::import;

/// The sample font with a Bold master, kerning groups and an instance.
fn two_masters() -> Font {
    let wght = Axis {
        name: "Weight".into(),
        tag: "wght".into(),
        min: 400.0,
        default: 400.0,
        max: 700.0,
        hidden: false,
        map: Vec::new(),
        labels: Vec::new(),
    };
    let f = structure::set_axes(&sample_font(), vec![wght]).unwrap();
    let (mut f, bold) =
        structure::add_master(&f, "m01", "Bold", &BTreeMap::from([("wght".into(), 700.0)]))
            .unwrap();
    f.glyph_mut("H").unwrap().layer_mut(&bold).unwrap().width += 100.0;
    for n in ["O", "o"] {
        let g = f.glyph_mut(n).unwrap();
        g.left_group = Some("round".into());
        g.right_group = Some("round".into());
    }
    f.glyph_mut("A").unwrap().export = false;
    f.kerning.insert(
        bold.clone(),
        vec![KernPair {
            left: "T".into(),
            right: "@round".into(),
            value: -90.0,
        }],
    );
    f.instances.push(Instance {
        name: "Medium".into(),
        location: BTreeMap::from([("wght".into(), 550.0)]),
        export: true,
        weight_class: None,
        width_class: None,
        style_link: None,
    });
    // A brace layer for H and an alternate for o.
    let (mut f, brace) =
        layers::add_brace_layer(&f, "H", "m01", &BTreeMap::from([("wght".into(), 550.0)])).unwrap();
    f.glyph_mut("H").unwrap().layer_mut(&brace).unwrap().width += 20.0;
    let ranges = [AxisRange {
        tag: "wght".into(),
        min: Some(600.0),
        max: None,
    }];
    let (mut f, _) = layers::add_bracket_layers(&f, "o", "m02", &ranges).unwrap();
    for l in f
        .glyph_mut("o")
        .unwrap()
        .layers
        .iter_mut()
        .filter(|l| !l.bracket.is_empty())
    {
        l.width += 50.0;
    }
    f
}

#[test]
fn designspace_round_trip() {
    let src = two_masters();
    let dir = tempfile::tempdir().unwrap();
    let ds = kernest_export::write_sources(&src, dir.path()).unwrap();
    let imp = import(&ds).unwrap();
    let f = &imp.font;
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);

    assert_eq!(f.info.family_name, src.info.family_name);
    assert_eq!(f.axes, src.axes);
    assert_eq!(f.masters.len(), 2);
    for (a, b) in f.masters.iter().zip(&src.masters) {
        assert_eq!((&a.id, &a.name, &a.location), (&b.id, &b.name, &b.location));
        assert_eq!(a.metrics, b.metrics);
    }
    assert_eq!(f.instances, src.instances);
    assert_eq!(f.features, src.features);

    // The exporter adds a .notdef; everything else comes back in the same order.
    assert_eq!(f.glyphs[0].name, ".notdef");
    let names: Vec<&str> = f.glyphs[1..].iter().map(|g| g.name.as_str()).collect();
    let expected: Vec<&str> = src.glyphs.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, expected);
    for g in &src.glyphs {
        let got = f.glyph(&g.name).unwrap();
        assert_eq!(got.unicodes, g.unicodes, "{}", g.name);
        assert_eq!(got.layers, g.layers, "{}", g.name);
        assert_eq!(got.export, g.export, "{}", g.name);
        assert_eq!(
            (&got.left_group, &got.right_group),
            (&g.left_group, &g.right_group),
            "{}",
            g.name
        );
    }
    let mut want = src.kerning.clone();
    for v in want.values_mut() {
        v.sort_by(|a, b| (&a.left, &a.right).cmp(&(&b.left, &b.right)));
    }
    assert_eq!(f.kerning, want);
}

#[test]
fn designspace_axis_map_round_trip() {
    let mut src = two_masters();
    // User 400..700 maps to design 400..900, with a bend at user 550 → design 600.
    src.axes[0].map = vec![(400.0, 400.0), (550.0, 600.0), (700.0, 900.0)];
    let dir = tempfile::tempdir().unwrap();
    let ds = kernest_export::write_sources(&src, dir.path()).unwrap();
    let xml = fs::read_to_string(&ds).unwrap();
    assert!(xml.contains("<map input=\"550\" output=\"600\"/>"), "{xml}");
    let imp = import(&ds).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let f = &imp.font;
    assert_eq!(f.axes, src.axes);
    for (a, b) in f.masters.iter().zip(&src.masters) {
        assert_eq!(a.location, b.location, "{}", a.name);
    }
    assert_eq!(f.instances, src.instances);
    for g in &src.glyphs {
        assert_eq!(f.glyph(&g.name).unwrap().layers, g.layers, "{}", g.name);
    }
}

#[test]
fn glyphs_axis_map_round_trip() {
    let mut src = two_masters();
    src.axes[0].map = vec![(400.0, 400.0), (550.0, 600.0), (700.0, 900.0)];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Mapped.glyphs");
    kernest_import::write_glyphs(&src, &path).unwrap();
    let imp = import(&path).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let f = &imp.font;
    assert_eq!(f.axes, src.axes);
    assert_eq!(f.masters, src.masters);
    assert_eq!(f.instances, src.instances);
    for g in &src.glyphs {
        assert_eq!(f.glyph(&g.name).unwrap().layers, g.layers, "{}", g.name);
    }
}

#[test]
fn glyphs_axis_location_import() {
    // Masters carry user locations ("Axis Location") instead of a font-wide mapping.
    let master = |id: &str, name: &str, design: f64, user: f64| {
        format!(
            "{{\naxesValues = (\n{design}\n);\ncustomParameters = (\n{{\nname = \"Axis Location\";\nvalue = (\n{{\nAxis = Weight;\nLocation = {user};\n}}\n);\n}}\n);\nid = {id};\nname = {name};\n}}"
        )
    };
    let file = format!(
        "{{\n.appVersion = \"3260\";\n.formatVersion = 3;\naxes = (\n{{\nname = Weight;\ntag = wght;\n}}\n);\nfamilyName = LocTest;\nfontMaster = (\n{},\n{}\n);\nunitsPerEm = 1000;\nglyphs = (\n{{\nglyphname = A;\nlayers = (\n{{\nlayerId = m01;\nwidth = 500;\n}},\n{{\nlayerId = m02;\nwidth = 600;\n}}\n);\n}}\n);\n}}",
        master("m01", "Regular", 400.0, 400.0),
        master("m02", "Bold", 900.0, 700.0),
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Loc.glyphs");
    fs::write(&path, file).unwrap();
    let f = import(&path).unwrap().font;
    assert_eq!(f.axes.len(), 1);
    let a = &f.axes[0];
    assert_eq!((a.min, a.default, a.max), (400.0, 400.0, 700.0));
    assert_eq!(a.map, vec![(400.0, 400.0), (700.0, 900.0)]);
    // Master locations stay in design units.
    assert_eq!(f.masters[1].location.get("wght"), Some(&900.0));
}

#[test]
fn ufo_export_round_trip() {
    let mut src = two_masters();
    src.glyph_mut("H").unwrap().production_name = Some("Hprod".into());
    let dir = tempfile::tempdir().unwrap();
    let opts = kernest_export::UfoExportOptions {
        masters: vec![],
        decompose_smart: false,
        color_layers: false,
    };
    let report = kernest_export::export_ufo(&src, dir.path(), &opts).unwrap();
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(report.files.len(), 3);
    let imp = import(&dir.path().join("font.designspace")).unwrap();
    let f = &imp.font;
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(f.axes, src.axes);
    assert_eq!(f.masters.len(), 2);
    assert_eq!(f.instances, src.instances);
    assert_eq!(f.features, src.features);
    for g in &src.glyphs {
        let got = f.glyph(&g.name).unwrap();
        assert_eq!(got.layers, g.layers, "{}", g.name);
        assert_eq!(got.unicodes, g.unicodes, "{}", g.name);
        assert_eq!(got.production_name, g.production_name, "{}", g.name);
    }
    let mut want = src.kerning.clone();
    for v in want.values_mut() {
        v.sort_by(|a, b| (&a.left, &a.right).cmp(&(&b.left, &b.right)));
    }
    assert_eq!(f.kerning, want);
}

#[test]
fn single_ufo() {
    let src = sample_font();
    let dir = tempfile::tempdir().unwrap();
    kernest_export::write_sources(&src, dir.path()).unwrap();
    let imp = import(&dir.path().join("master0.ufo")).unwrap();
    assert_eq!(imp.font.masters.len(), 1);
    assert_eq!(imp.font.masters[0].name, "Regular");
    assert!(imp.font.axes.is_empty());
    assert_eq!(imp.font.glyphs.len(), src.glyphs.len() + 1);
    assert_eq!(
        imp.font.glyph("H").unwrap().layers,
        src.glyph("H").unwrap().layers
    );
}

#[test]
fn glyphs_round_trip() {
    let mut src = two_masters();
    src.info.designer = Some("Test Designer".into());
    src.info.copyright = Some("Copyright 2026".into());
    src.info.vendor_id = Some("KRNS".into());
    src.info.license = Some("Open Font License".into());
    src.info.description = Some("A test font.".into());
    src.info.hinting = Some(kernest_core::Hinting {
        range_min: 9,
        range_max: 45,
        increase_x_height: 14,
        hint_composites: true,
    });
    src.features = "languagesystem DFLT dflt;\nfeature salt {\nsub A by A.alt;\n} salt;\n".into();
    src.glyph_mut("H")
        .unwrap()
        .layer_mut("m01")
        .unwrap()
        .metrics_keys
        .left = Some("=T".into());
    src.glyph_mut("H")
        .unwrap()
        .layer_mut("m02")
        .unwrap()
        .metrics_keys
        .right = Some("=|T+10".into());
    src.glyph_mut("H").unwrap().production_name = Some("Hprod".into());

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("RoundTrip.glyphs");
    kernest_import::write_glyphs(&src, &path).unwrap();
    let imp = import(&path).unwrap();
    let f = &imp.font;
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);

    assert_eq!(f.info.family_name, src.info.family_name);
    assert_eq!(
        (f.info.version_major, f.info.version_minor),
        (src.info.version_major, src.info.version_minor)
    );
    assert_eq!(f.info.designer, src.info.designer);
    assert_eq!(f.info.copyright, src.info.copyright);
    assert_eq!(f.info.vendor_id, src.info.vendor_id);
    assert_eq!(f.info.license, src.info.license);
    assert_eq!(f.info.description, src.info.description);
    assert_eq!(f.info.hinting, src.info.hinting);
    assert_eq!(f.axes, src.axes);
    assert_eq!(f.masters, src.masters);
    assert_eq!(f.instances, src.instances);
    assert!(f.features.contains("sub A by A.alt;"), "{}", f.features);
    assert!(
        f.features.contains("languagesystem DFLT dflt;"),
        "{}",
        f.features
    );

    let names: Vec<&str> = f.glyphs.iter().map(|g| g.name.as_str()).collect();
    let expected: Vec<&str> = src.glyphs.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, expected);
    for g in &src.glyphs {
        let got = f.glyph(&g.name).unwrap();
        assert_eq!(got.unicodes, g.unicodes, "{}", g.name);
        assert_eq!(got.export, g.export, "{}", g.name);
        assert_eq!(
            (&got.left_group, &got.right_group),
            (&g.left_group, &g.right_group),
            "{}",
            g.name
        );
        assert_eq!(got.production_name, g.production_name, "{}", g.name);
        assert_eq!(got.layers, g.layers, "{}", g.name);
    }
    let mut want = src.kerning.clone();
    for v in want.values_mut() {
        v.sort_by(|a, b| (&a.left, &a.right).cmp(&(&b.left, &b.right)));
    }
    assert_eq!(f.kerning, want);
    assert_eq!(
        f.glyph("H")
            .unwrap()
            .layer("m01")
            .unwrap()
            .metrics_keys
            .left
            .as_deref(),
        Some("=T")
    );
    assert_eq!(
        f.glyph("H")
            .unwrap()
            .layer("m02")
            .unwrap()
            .metrics_keys
            .right
            .as_deref(),
        Some("=|T+10")
    );
}

#[test]
fn glyphs_smart_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("Src.glyphs");
    fs::write(&p1, glyphs_file()).unwrap();
    let src = import(&p1).unwrap().font;
    let p2 = dir.path().join("Out.glyphs");
    kernest_import::write_glyphs(&src, &p2).unwrap();
    let f = &import(&p2).unwrap().font;

    let stem = f.glyph("_stem").unwrap();
    assert_eq!(stem.smart_axes.len(), 1);
    assert_eq!(
        (
            stem.smart_axes[0].name.as_str(),
            stem.smart_axes[0].min,
            stem.smart_axes[0].max
        ),
        ("Height", 0.0, 100.0)
    );
    assert_eq!(stem.smart_layers().count(), 2, "one tall pole per master");
    for pole in stem.smart_layers() {
        assert_eq!(pole.smart.as_ref().unwrap()["Height"], 100.0);
        let Shape::Path(p) = &pole.shapes[0] else {
            panic!("pole is a path")
        };
        assert_eq!(p.nodes[2].y, 700.0, "the tall pole");
    }
    let il = f.glyph("I").unwrap().layer("m01").unwrap();
    let Shape::Component {
        base,
        transform,
        smart,
        ..
    } = &il.shapes[0]
    else {
        panic!(
            "I should use the smart stem as a component: {:?}",
            il.shapes
        )
    };
    assert_eq!(
        (base.as_str(), transform[4], smart.get("Height").copied()),
        ("_stem", 85.0, Some(50.0))
    );
}

#[test]
fn unknown_extension() {
    let e = import(Path::new("font.xyz")).unwrap_err();
    assert!(e.contains(".glyphs"), "{e}");
}

// ---- Glyphs ----------------------------------------------------------------

const FONTINFO: &str = r#"
.appVersion = "3260";
.formatVersion = 3;
axes = (
{
name = Weight;
tag = wght;
}
);
customParameters = (
{
name = vendorID;
value = KRNS;
},
{
name = "TTFAutohint options";
value = "-l 9 --hinting-range-max=45 -x0 -c -D latn";
}
);
familyName = "Import Test";
features = (
{
code = "sub A by A.alt;";
tag = salt;
}
);
fontMaster = (
{
axesValues = (
400
);
id = "M-REG";
metricValues = (
{
pos = 780;
},
{
pos = 690;
},
{
pos = 510;
},
{
},
{
pos = -210;
}
);
name = Regular;
},
{
axesValues = (
800
);
id = "M-BLD";
metricValues = (
{
pos = 790;
},
{
pos = 700;
},
{
pos = 520;
},
{
},
{
pos = -220;
}
);
name = Black;
}
);
instances = (
{
axesValues = (
600
);
name = SemiBold;
},
{
axesValues = (
800
);
exports = 0;
name = Black;
}
);
kerningLTR = {
"M-REG" = {
"@MMK_L_A" = {
V = -80;
};
};
"M-BLD" = {
"@MMK_L_A" = {
V = -100;
};
};
};
metrics = (
{
type = ascender;
},
{
type = "cap height";
},
{
type = "x-height";
},
{
type = baseline;
},
{
type = descender;
}
);
unitsPerEm = 1000;
versionMajor = 2;
versionMinor = 3;
"#;

fn a_layer(id: &str, w: u32, top: u32) -> String {
    format!(
        r#"{{
anchors = (
{{
name = top;
pos = ({top},700);
}}
);
layerId = "{id}";
shapes = (
{{
closed = 1;
nodes = (
(0,0,l),
({top},700,l),
({w},0,l)
);
}},
{{
closed = 0;
nodes = (
(100,100,l),
(150,200,o),
(250,200,o),
(300,100,cs)
);
}}
);
width = {w};
}}"#
    )
}

fn glyph_a() -> String {
    format!(
        r#"{{
glyphname = A;
kernRight = A;
metricLeft = "=V";
metricRight = V;
layers = (
{},
{},
{},
{{
associatedMasterId = "M-REG";
layerId = "BACKUP-1";
name = "Jan 1 26, 10:00";
width = 600;
}},
{{
associatedMasterId = "M-REG";
attr = {{
axisRules = (
{{
min = 600;
}}
);
}};
layerId = "BRACKET-R";
name = "[600]";
width = 610;
}},
{{
associatedMasterId = "M-BLD";
attr = {{
axisRules = (
{{
min = 600;
}}
);
}};
layerId = "BRACKET-B";
name = "[600]";
width = 710;
}}
);
unicode = 65;
}}"#,
        a_layer("M-REG", 600, 300),
        a_layer("M-BLD", 700, 350).replacen("{\n", "{\nmetricRight = \"=|V+10\";\n", 1),
        a_layer("BRACE-1", 650, 320).replacen(
            "{
",
            "{
associatedMasterId = \"M-REG\";
attr = {
coordinates = (
600
);
};
name = \"{600}\";
",
            1
        )
    )
}

fn glyph_aacute() -> String {
    let layer = |id: &str, w: u32| {
        format!(
            r#"{{
layerId = "{id}";
shapes = (
{{
ref = A;
}},
{{
pos = (200,120);
ref = acutecomb;
}}
);
width = {w};
}}"#
        )
    };
    format!(
        "{{\nglyphname = Aacute;\nlayers = (\n{},\n{}\n);\nunicode = 193;\n}}",
        layer("M-REG", 600),
        layer("M-BLD", 700)
    )
}

fn simple_glyph(name: &str, unicode: Option<u32>, export: bool, kern_left: Option<&str>) -> String {
    let layer = |id: &str| format!("{{\nlayerId = \"{id}\";\nwidth = 500;\n}}");
    let mut s = String::from("{\n");
    if !export {
        s.push_str("export = 0;\n");
    }
    s.push_str(&format!("glyphname = {name};\n"));
    if let Some(k) = kern_left {
        s.push_str(&format!("kernLeft = {k};\n"));
    }
    s.push_str(&format!(
        "layers = (\n{},\n{}\n);\n",
        layer("M-REG"),
        layer("M-BLD")
    ));
    if let Some(u) = unicode {
        s.push_str(&format!("unicode = {u};\n"));
    }
    s.push('}');
    s
}

/// A smart glyph: an 80-unit stem, Height 0 → 300 tall, Height 100 → 700 tall.
fn smart_stem() -> String {
    let rect = |h: u32| {
        format!("{{\nclosed = 1;\nnodes = (\n(0,0,l),\n(80,0,l),\n(80,{h},l),\n(0,{h},l)\n);\n}}")
    };
    let layer = |id: &str, master: Option<&str>, pole: u32, h: u32| {
        let assoc = master
            .map(|m| format!("associatedMasterId = \"{m}\";\n"))
            .unwrap_or_default();
        format!(
            "{{\n{assoc}layerId = \"{id}\";\nname = \"pole\";\npartSelection = {{\nHeight = {pole};\n}};\nshapes = (\n{}\n);\nwidth = 80;\n}}",
            rect(h)
        )
    };
    format!(
        "{{\nexport = 0;\nglyphname = _stem;\nlayers = (\n{},\n{},\n{},\n{}\n);\npartsSettings = (\n{{\nbottomValue = 0;\nname = Height;\ntopValue = 100;\n}}\n);\n}}",
        layer("M-REG", None, 1, 300),
        layer("POLE-R", Some("M-REG"), 2, 700),
        layer("M-BLD", None, 1, 300),
        layer("POLE-B", Some("M-BLD"), 2, 700),
    )
}

/// I: the smart stem at Height 50.
fn glyph_i() -> String {
    let layer = |id: &str| {
        format!("{{\nlayerId = \"{id}\";\nshapes = (\n{{\npiece = {{\nHeight = 50;\n}};\npos = (85,0);\nref = _stem;\n}}\n);\nwidth = 250;\n}}")
    };
    format!(
        "{{\nglyphname = I;\nlayers = (\n{},\n{}\n);\nunicode = 73;\n}}",
        layer("M-REG"),
        layer("M-BLD")
    )
}

fn glyphs() -> Vec<(&'static str, String)> {
    vec![
        ("_stem", smart_stem()),
        ("I", glyph_i()),
        ("A", glyph_a()),
        ("Aacute", glyph_aacute()),
        ("V", simple_glyph("V", Some(86), true, Some("V"))),
        (
            "acutecomb",
            simple_glyph("acutecomb", Some(769), true, None),
        ),
        ("A.alt", simple_glyph("A.alt", None, true, None)),
        ("_part", simple_glyph("_part", None, false, None)),
    ]
}

fn glyphs_file() -> String {
    let body = FONTINFO.trim_start().trim_end();
    let gl: Vec<String> = glyphs().into_iter().map(|(_, g)| g).collect();
    format!("{{\n{body}\nglyphs = (\n{}\n);\n}}\n", gl.join(",\n"))
}

fn check_glyphs_font(imp: &kernest_import::Imported) {
    let f = &imp.font;
    assert_eq!(f.info.family_name, "Import Test");
    assert_eq!((f.info.version_major, f.info.version_minor), (2, 3));
    assert_eq!(f.info.vendor_id.as_deref(), Some("KRNS"));
    assert_eq!(
        f.info.hinting,
        Some(kernest_core::Hinting {
            range_min: 9,
            range_max: 45,
            increase_x_height: 0,
            hint_composites: true,
        }),
        "from the TTFAutohint options custom parameter"
    );
    assert_eq!(f.axes.len(), 1);
    let a = &f.axes[0];
    assert_eq!(
        (a.tag.as_str(), a.min, a.default, a.max),
        ("wght", 400.0, 400.0, 800.0)
    );

    assert_eq!(f.masters.len(), 2);
    assert_eq!(f.masters[0].id, "m01");
    assert_eq!(f.masters[1].name, "Black");
    assert_eq!(f.masters[1].location["wght"], 800.0);
    let m = f.masters[0].metrics;
    assert_eq!(
        (m.ascender, m.cap_height, m.x_height, m.descender),
        (780.0, 690.0, 510.0, -210.0)
    );

    let names: Vec<&str> = f.glyphs.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "_stem",
            "I",
            "A",
            "Aacute",
            "V",
            "acutecomb",
            "A.alt",
            "_part"
        ]
    );
    // Smart components stay smart: the stem's axis and poles, and I's value on it.
    let stem = f.glyph("_stem").unwrap();
    assert_eq!(stem.smart_axes.len(), 1);
    assert_eq!(
        (
            stem.smart_axes[0].name.as_str(),
            stem.smart_axes[0].min,
            stem.smart_axes[0].max
        ),
        ("Height", 0.0, 100.0)
    );
    assert_eq!(stem.smart_layers().count(), 2, "one tall pole per master");
    let il = f.glyph("I").unwrap().layer("m01").unwrap();
    let Shape::Component {
        base,
        transform,
        smart,
        ..
    } = &il.shapes[0]
    else {
        panic!(
            "I should use the smart stem as a component: {:?}",
            il.shapes
        )
    };
    assert_eq!(
        (base.as_str(), transform[4], smart.get("Height").copied()),
        ("_stem", 85.0, Some(50.0))
    );
    let b = kernest_core::geometry::bounds(f, il).unwrap();
    assert_eq!(
        (b.min_x, b.max_y),
        (85.0, 500.0),
        "drawn halfway between the poles"
    );
    let ga = f.glyph("A").unwrap();
    assert_eq!(ga.unicodes[0].0, 65);
    assert_eq!(ga.right_group.as_deref(), Some("A"));
    assert_eq!(
        ga.layers.len(),
        5,
        "2 masters, a brace and 2 bracket layers; no backup"
    );
    let brace = ga.brace_layers().next().unwrap();
    assert_eq!((brace.master_id.as_str(), brace.width), ("m01", 650.0));
    assert_eq!(brace.location.as_ref().unwrap()["wght"], 600.0);
    let brackets: Vec<_> = ga.bracket_layers().collect();
    assert_eq!(brackets.len(), 2);
    assert_eq!(
        (brackets[1].master_id.as_str(), brackets[1].width),
        ("m02", 710.0)
    );
    assert_eq!(
        brackets[0].bracket,
        [AxisRange {
            tag: "wght".into(),
            min: Some(600.0),
            max: None
        }]
    );
    let bold = ga.layer("m02").unwrap();
    assert_eq!(bold.width, 700.0);
    assert_eq!(bold.anchors[0].x, 350.0);
    let Shape::Path(open) = &bold.shapes[1] else {
        panic!("expected a path")
    };
    assert!(!open.closed);
    assert_eq!(open.nodes.len(), 4);
    assert_eq!(open.nodes[1].kind, NodeKind::OffCurve);
    assert_eq!(open.nodes[3].kind, NodeKind::Curve);
    assert!(open.nodes[3].smooth);

    let Shape::Component {
        base, transform, ..
    } = &f.glyph("Aacute").unwrap().layers[0].shapes[1]
    else {
        panic!("expected a component")
    };
    assert_eq!(base, "acutecomb");
    assert_eq!(*transform, [1.0, 0.0, 0.0, 1.0, 200.0, 120.0]);

    assert_eq!(f.glyph("V").unwrap().left_group.as_deref(), Some("V"));
    assert!(f.glyph("A.alt").unwrap().export);
    assert!(!f.glyph("_part").unwrap().export);

    assert_eq!(
        f.kerning["m02"],
        vec![KernPair {
            left: "@A".into(),
            right: "V".into(),
            value: -100.0
        }]
    );
    assert_eq!(f.instances.len(), 2);
    assert_eq!(f.instances[0].name, "SemiBold");
    assert_eq!(f.instances[0].location["wght"], 600.0);
    assert!(f.instances[0].export);
    assert!(!f.instances[1].export);
    assert!(f.features.contains("sub A by A.alt;"), "{}", f.features);
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    // Metrics keys: glyph-wide on every master layer (with "=" added where the
    // file leaves it out), and a layer's own key overriding it.
    let ka = &f.glyph("A").unwrap().layer("m01").unwrap().metrics_keys;
    assert_eq!(
        (ka.left.as_deref(), ka.right.as_deref()),
        (Some("=V"), Some("=V"))
    );
    let kb = &f.glyph("A").unwrap().layer("m02").unwrap().metrics_keys;
    assert_eq!(kb.right.as_deref(), Some("=|V+10"));
}

#[test]
fn glyphs_file_import() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ImportTest.glyphs");
    fs::write(&path, glyphs_file()).unwrap();
    check_glyphs_font(&import(&path).unwrap());
}

#[test]
fn glyphspackage_import() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("ImportTest.glyphspackage");
    fs::create_dir_all(pkg.join("glyphs")).unwrap();
    fs::write(
        pkg.join("fontinfo.plist"),
        format!("{{\n{}\n}}\n", FONTINFO.trim()),
    )
    .unwrap();
    let mut order = Vec::new();
    for (name, g) in glyphs() {
        fs::write(pkg.join("glyphs").join(format!("{name}.glyph")), g).unwrap();
        order.push(format!("\"{name}\""));
    }
    fs::write(
        pkg.join("order.plist"),
        format!("(\n{}\n)\n", order.join(",\n")),
    )
    .unwrap();
    check_glyphs_font(&import(&pkg).unwrap());
}

#[test]
fn imported_glyphs_font_exports() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ImportTest.glyphs");
    fs::write(&path, glyphs_file()).unwrap();
    let font = import(&path).unwrap().font;
    let out = dir.path().join("out");
    let r = kernest_export::export(&font, &kernest_export::ExportOptions::ttf(false, true, out))
        .unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.files.len(), 1, "one exported instance: SemiBold");
}

// ---- Compiled fonts --------------------------------------------------------

/// Exports the font as one static file in the given format ("ttf", "otf", "woff2").
fn export_static(src: &Font, dir: &Path, fmt: &str) -> std::path::PathBuf {
    let opts = kernest_export::ExportOptions {
        variable: false,
        statics: true,
        ttf: fmt == "ttf",
        otf: fmt == "otf",
        woff2: fmt == "woff2",
        remove_overlaps: false,
        apply_metrics_keys: true,
        autohint: false,
        production_names: false,
        out_dir: dir.to_path_buf(),
    };
    let r = kernest_export::export(src, &opts).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.files.len(), 1, "{fmt}");
    std::path::PathBuf::from(&r.files[0].path)
}

/// The effective kerning for a glyph pair, resolving any kerning groups.
fn kern_value(f: &Font, left: &str, right: &str) -> f64 {
    let lg = f.glyph(left).and_then(|g| g.right_group.clone());
    let rg = f.glyph(right).and_then(|g| g.left_group.clone());
    for pairs in f.kerning.values() {
        for p in pairs {
            let l = p.left == left || lg.as_ref().is_some_and(|g| p.left == format!("@{g}"));
            let r = p.right == right || rg.as_ref().is_some_and(|g| p.right == format!("@{g}"));
            if l && r {
                return p.value;
            }
        }
    }
    0.0
}

#[test]
fn compiled_ttf_round_trip() {
    let src = sample_font();
    let dir = tempfile::tempdir().unwrap();
    let path = export_static(&src, dir.path(), "ttf");
    let imp = import(&path).unwrap();
    let f = &imp.font;
    assert_eq!(f.info.family_name, "Kernest Sample");
    assert_eq!(f.info.units_per_em, 1000);
    assert_eq!(f.masters.len(), 1);
    assert!(f.axes.is_empty());
    let m = f.masters[0].metrics;
    assert_eq!(
        (m.ascender, m.cap_height, m.x_height, m.descender),
        (800.0, 700.0, 500.0, -200.0)
    );
    let mid = f.masters[0].id.clone();
    for g in &src.glyphs {
        let got = f
            .glyph(&g.name)
            .unwrap_or_else(|| panic!("missing {}", g.name));
        assert_eq!(got.unicodes, g.unicodes, "{}", g.name);
        assert_eq!(
            got.layer(&mid).unwrap().width,
            g.layer("m01").unwrap().width,
            "{} width",
            g.name
        );
    }
    // H is three rectangles: the outline comes back as twelve line nodes.
    let h = f.glyph("H").unwrap().layer(&mid).unwrap();
    let hb = geometry::bounds(f, h).unwrap();
    assert_eq!(
        (hb.min_x, hb.min_y, hb.max_x, hb.max_y),
        (80.0, 0.0, 600.0, 700.0)
    );
    let nodes: Vec<&kernest_core::Node> = h
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .flat_map(|p| &p.nodes)
        .collect();
    assert_eq!(nodes.len(), 12);
    assert!(nodes.iter().all(|n| n.kind == NodeKind::Line));
    // O keeps its size (the cubic→quadratic approximation stays within a unit).
    let o = f.glyph("O").unwrap().layer(&mid).unwrap();
    let ob = geometry::bounds(f, o).unwrap();
    assert!(
        (ob.min_x - 60.0).abs() < 2.0 && (ob.max_y - 712.0).abs() < 2.0,
        "{ob:?}"
    );
    assert_eq!(kern_value(f, "T", "o"), -60.0);
    assert_eq!(kern_value(f, "L", "T"), -80.0);
}

#[test]
fn compiled_variable_avar_round_trip() {
    let mut src = two_masters();
    src.axes[0].map = vec![(400.0, 400.0), (550.0, 600.0), (700.0, 900.0)];
    let dir = tempfile::tempdir().unwrap();
    let opts = kernest_export::ExportOptions {
        variable: true,
        statics: false,
        ttf: true,
        otf: false,
        woff2: false,
        remove_overlaps: false,
        apply_metrics_keys: true,
        autohint: false,
        production_names: false,
        out_dir: dir.path().to_path_buf(),
    };
    let r = kernest_export::export(&src, &opts).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let path = std::path::PathBuf::from(&r.files[0].path);
    let imp = import(&path).unwrap();
    let f = &imp.font;
    assert_eq!(f.axes.len(), 1);
    let a = &f.axes[0];
    assert_eq!((a.min, a.default, a.max), (400.0, 400.0, 700.0));
    assert!(!a.map.is_empty(), "avar was lost");

    // A compiled font keeps only the normalized avar, not the original design scale,
    // so the map comes back on the user scale. What must survive is the normalized
    // user -> design shape: compare it at several points across the axis.
    let norm_shape = |ax: &Axis, t: f64| -> f64 {
        let user = if t < 0.0 {
            ax.default + t * (ax.default - ax.min)
        } else {
            ax.default + t * (ax.max - ax.default)
        };
        let d = kernest_core::designspace::user_to_design(ax, user);
        let (dmin, dflt, dmax) = kernest_core::designspace::design_bounds(ax);
        if d < dflt {
            (d - dflt) / (dflt - dmin)
        } else if d > dflt {
            (d - dflt) / (dmax - dflt)
        } else {
            0.0
        }
    };
    for t in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let want = norm_shape(&src.axes[0], t);
        let got = norm_shape(a, t);
        assert!((want - got).abs() < 0.01, "t={t}: want {want}, got {got}");
    }
    // The bend is preserved: normalized user 0.5 lands at normalized design 0.4.
    assert!((norm_shape(a, 0.5) - 0.4).abs() < 0.01);
    // Masters sit at the reimported design extremes (the user scale here).
    let bold = f
        .masters
        .iter()
        .find(|m| m.name.contains("max"))
        .expect("a max master");
    assert!(
        (bold.location["wght"] - 700.0).abs() < 0.05,
        "{:?}",
        bold.location
    );
}

#[test]
fn designspace_stat_labels_round_trip() {
    let mut src = two_masters();
    src.axes[0].labels = vec![
        AxisValue {
            name: "Regular".into(),
            value: 400.0,
            min: None,
            max: None,
            elidable: true,
            linked_value: Some(700.0),
        },
        AxisValue {
            name: "Bold".into(),
            value: 700.0,
            min: None,
            max: None,
            elidable: false,
            linked_value: None,
        },
        AxisValue {
            name: "Semibold".into(),
            value: 600.0,
            min: Some(550.0),
            max: Some(650.0),
            elidable: false,
            linked_value: None,
        },
    ];
    let dir = tempfile::tempdir().unwrap();
    let ds = kernest_export::write_sources(&src, dir.path()).unwrap();
    let xml = fs::read_to_string(&ds).unwrap();
    assert!(xml.contains("<labels>"), "{xml}");
    assert!(xml.contains("uservalue=\"400\""), "{xml}");
    assert!(xml.contains("linkeduservalue=\"700\""), "{xml}");
    assert!(xml.contains("elidable=\"true\""), "{xml}");
    let imp = import(&ds).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.font.axes, src.axes);
}

#[test]
fn designspace_variable_fonts_round_trip() {
    let mut src = two_masters();
    src.variable_fonts = vec![
        VariableFontSetting {
            name: "Light".into(),
            file_name: "Sample-Light".into(),
            axes: vec![VariableAxisSubset {
                tag: "wght".into(),
                min: Some(400.0),
                max: Some(500.0),
            }],
            glyphs: Vec::new(),
        },
        VariableFontSetting {
            name: "Bold".into(),
            file_name: "Sample-Bold".into(),
            axes: vec![VariableAxisSubset {
                tag: "wght".into(),
                min: Some(500.0),
                max: Some(700.0),
            }],
            glyphs: Vec::new(),
        },
    ];
    let dir = tempfile::tempdir().unwrap();
    let ds = kernest_export::write_sources(&src, dir.path()).unwrap();
    let xml = fs::read_to_string(&ds).unwrap();
    assert!(xml.contains("<variable-fonts>"), "{xml}");
    assert!(xml.contains("filename=\"Sample-Light\""), "{xml}");
    assert!(xml.contains("userminimum=\"400\""), "{xml}");
    assert!(xml.contains("usermaximum=\"500\""), "{xml}");
    let imp = import(&ds).unwrap();
    assert_eq!(imp.font.variable_fonts, src.variable_fonts);
}

#[test]
fn compiled_variable_stat_round_trip() {
    let mut src = two_masters();
    src.axes[0].labels = vec![
        AxisValue {
            name: "Regular".into(),
            value: 400.0,
            min: None,
            max: None,
            elidable: true,
            linked_value: Some(700.0),
        },
        AxisValue {
            name: "Bold".into(),
            value: 700.0,
            min: None,
            max: None,
            elidable: false,
            linked_value: None,
        },
        AxisValue {
            name: "Semibold".into(),
            value: 600.0,
            min: Some(550.0),
            max: Some(650.0),
            elidable: false,
            linked_value: None,
        },
    ];
    let dir = tempfile::tempdir().unwrap();
    let opts = kernest_export::ExportOptions {
        variable: true,
        statics: false,
        ttf: true,
        otf: false,
        woff2: false,
        remove_overlaps: false,
        apply_metrics_keys: true,
        autohint: false,
        production_names: false,
        out_dir: dir.path().to_path_buf(),
    };
    let r = kernest_export::export(&src, &opts).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let path = std::path::PathBuf::from(&r.files[0].path);
    let imp = import(&path).unwrap();
    let f = &imp.font;
    assert_eq!(f.axes.len(), 1);
    assert_eq!(f.axes[0].labels, src.axes[0].labels);
}

#[test]
fn compiled_otf_and_woff2_round_trip() {
    let src = sample_font();
    for fmt in ["otf", "woff2"] {
        let dir = tempfile::tempdir().unwrap();
        let path = export_static(&src, dir.path(), fmt);
        let imp = import(&path).unwrap();
        let f = &imp.font;
        assert_eq!(f.info.family_name, "Kernest Sample", "{fmt}");
        assert_eq!(f.info.units_per_em, 1000, "{fmt}");
        let mid = f.masters[0].id.clone();
        for g in &src.glyphs {
            let got = f
                .glyph(&g.name)
                .unwrap_or_else(|| panic!("{fmt} missing {}", g.name));
            assert_eq!(got.unicodes, g.unicodes, "{fmt} {}", g.name);
            assert_eq!(
                got.layer(&mid).unwrap().width,
                g.layer("m01").unwrap().width,
                "{fmt} {} width",
                g.name
            );
        }
        let h = f.glyph("H").unwrap().layer(&mid).unwrap();
        let hb = geometry::bounds(f, h).unwrap();
        assert_eq!(
            (hb.min_x, hb.min_y, hb.max_x, hb.max_y),
            (80.0, 0.0, 600.0, 700.0),
            "{fmt}"
        );
        assert_eq!(kern_value(f, "T", "o"), -60.0, "{fmt}");
        assert_eq!(kern_value(f, "L", "T"), -80.0, "{fmt}");
    }
}

#[test]
fn woff2_transformed_fixture() {
    // A real web font with the standard glyf transform (not Kernest's null transform).
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/lato.woff2");
    let imp = import(&path).unwrap();
    let f = &imp.font;
    assert!(
        f.info.family_name.contains("Lato"),
        "{}",
        f.info.family_name
    );
    assert!(f.glyphs.len() > 100, "{}", f.glyphs.len());
    let a = f.glyph("A").unwrap();
    assert!(a.unicodes.iter().any(|c| c.0 == 65));
    let l = a.layer(&f.masters[0].id).unwrap();
    assert!(l.width > 0.0);
    assert!(!l.shapes.is_empty(), "A should have a drawn outline");
}
