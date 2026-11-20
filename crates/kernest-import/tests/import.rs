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
        rename_glyphs: Vec::new(),
        remove_glyphs: Vec::new(),
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
        6,
        "2 masters, a brace, 2 bracket layers and the backup"
    );
    assert_eq!(
        ga.layers.iter().filter(|l| l.backup.is_some()).count(),
        1,
        "the fixture's backup layer is kept as a backup"
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

// ---- Hinting: alignment zones and stems ------------------------------------

use kernest_core::{PsHinting, Stem, Zone};

/// Two masters with zones, stems and hinting settings on the regular master.
fn hinted_font() -> Font {
    let mut f = two_masters();
    let m = &mut f.masters[0];
    m.alignment_zones = vec![
        Zone {
            position: 690.0,
            size: 10.0,
        },
        Zone {
            position: 510.0,
            size: 12.0,
        },
        Zone {
            position: 0.0,
            size: -10.0,
        },
        Zone {
            position: -210.0,
            size: -10.0,
        },
    ];
    m.stems = vec![
        Stem {
            name: "hStem0".into(),
            horizontal: true,
            value: 80.0,
        },
        Stem {
            name: "vStem0".into(),
            horizontal: false,
            value: 90.0,
        },
    ];
    f.info.control_instructions = "0x0025 touch 0-5 y".into();
    f.info.ps_hinting = Some(PsHinting {
        blue_scale: 0.05,
        blue_shift: 7.0,
        blue_fuzz: 2.0,
    });
    f
}

const V3_HINTING: &str = r#"
.appVersion = "3260";
.formatVersion = 3;
customParameters = (
{
name = "TTFAutohint control instructions";
value = "0x0025 touch 0-5 y";
},
{
name = BlueScale;
value = 0.05;
},
{
name = BlueFuzz;
value = 2;
}
);
familyName = "Hint Test";
fontMaster = (
{
id = "M-REG";
metricValues = (
{
pos = 780;
},
{
pos = 690;
over = 10;
},
{
pos = 510;
over = 12;
},
{
over = -10;
},
{
pos = -210;
over = -10;
},
{
}
);
name = Regular;
stemValues = (
80,
90
);
}
);
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
},
{
type = "italic angle";
}
);
stems = (
{
name = hStem0;
horizontal = 1;
},
{
name = vStem0;
}
);
unitsPerEm = 1000;
"#;

const V2_HINTING: &str = r#"
.appVersion = "895";
customParameters = (
{
name = "TTFAutohint control instructions";
value = "0x0025 touch 0-5 y";
}
);
familyName = "Hint Test";
fontMaster = (
{
alignmentZones = (
"{690, 10}",
"{510, 12}",
"{0, -10}",
"{-210, -10}"
);
ascender = 780;
capHeight = 690;
descender = -210;
horizontalStems = (
80
);
id = "M-REG";
name = Regular;
verticalStems = (
90
);
xHeight = 510;
}
);
unitsPerEm = 1000;
"#;

fn hinting_glyphs_file(info: &str) -> String {
    let g = "{\nglyphname = A;\nlayers = (\n{\nlayerId = \"M-REG\";\nwidth = 500;\n}\n);\nunicode = 65;\n}";
    format!("{{\n{}\nglyphs = (\n{g}\n);\n}}\n", info.trim())
}

fn check_hinting_import(imp: &kernest_import::Imported) {
    let f = &imp.font;
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let m = &f.masters[0];
    assert_eq!(
        m.alignment_zones,
        vec![
            Zone {
                position: 690.0,
                size: 10.0
            },
            Zone {
                position: 510.0,
                size: 12.0
            },
            Zone {
                position: 0.0,
                size: -10.0
            },
            Zone {
                position: -210.0,
                size: -10.0
            },
        ]
    );
    assert_eq!(m.stems.len(), 2);
    assert!(m.stems.iter().any(|s| s.horizontal && s.value == 80.0));
    assert!(m.stems.iter().any(|s| !s.horizontal && s.value == 90.0));
    assert_eq!(f.info.control_instructions, "0x0025 touch 0-5 y");
}

#[test]
fn glyphs_v3_hinting_import() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Hint.glyphs");
    fs::write(&path, hinting_glyphs_file(V3_HINTING)).unwrap();
    let imp = import(&path).unwrap();
    check_hinting_import(&imp);
    assert_eq!(
        imp.font.info.ps_hinting,
        Some(PsHinting {
            blue_scale: 0.05,
            blue_shift: 7.0,
            blue_fuzz: 2.0
        })
    );
}

#[test]
fn glyphs_v2_hinting_import() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Hint.glyphs");
    fs::write(&path, hinting_glyphs_file(V2_HINTING)).unwrap();
    check_hinting_import(&import(&path).unwrap());
}

#[test]
fn glyphs_hinting_round_trip() {
    let src = hinted_font();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Hint.glyphs");
    kernest_import::write_glyphs(&src, &path).unwrap();
    let imp = import(&path).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let f = &imp.font;
    // Glyphs stores zones on metrics, so their order is normalised; compare as a set.
    let key = |z: &Zone| (z.position as i64, z.size as i64);
    let mut want = src.masters[0].alignment_zones.clone();
    let mut got = f.masters[0].alignment_zones.clone();
    want.sort_by_key(key);
    got.sort_by_key(key);
    assert_eq!(got, want);
    assert_eq!(f.masters[0].stems, src.masters[0].stems);
    assert_eq!(f.info.control_instructions, src.info.control_instructions);
    assert_eq!(f.info.ps_hinting, src.info.ps_hinting);
}

#[test]
fn ufo_hinting_round_trip() {
    let src = hinted_font();
    let dir = tempfile::tempdir().unwrap();
    let opts = kernest_export::UfoExportOptions {
        masters: vec![],
        decompose_smart: false,
        color_layers: false,
    };
    kernest_export::export_ufo(&src, dir.path(), &opts).unwrap();
    let imp = import(&dir.path().join("font.designspace")).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let m = &imp.font.masters[0];
    // UFO keeps zones as blue pairs; compare as a set (order is normalised on read).
    let mut want = src.masters[0].alignment_zones.clone();
    let mut got = m.alignment_zones.clone();
    let key = |z: &Zone| (z.position as i64, z.size as i64);
    want.sort_by_key(key);
    got.sort_by_key(key);
    assert_eq!(got, want);
    assert!(m.stems.iter().any(|s| s.horizontal && s.value == 80.0));
    assert!(m.stems.iter().any(|s| !s.horizontal && s.value == 90.0));
    assert_eq!(imp.font.info.ps_hinting, src.info.ps_hinting);
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
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
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
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
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

#[test]
fn glyphs_round_trip_keeps_guides_backgrounds_metrics_and_colour() {
    use kernest_core::color::{self, Color, ColorStop, Paint};
    use kernest_core::{Background, Guide};
    let mut src = two_masters();
    src.info.use_typo_metrics = true;
    let red = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    let blue = Color {
        r: 0,
        g: 0,
        b: 255,
        a: 200,
    };
    let green = Color {
        r: 0,
        g: 128,
        b: 0,
        a: 255,
    };
    src = color::set_palettes(&src, vec![vec![red, blue, green]]).unwrap();
    src.master_palettes
        .insert("m02".into(), vec![vec![green, red, blue]]);
    let stops = vec![
        ColorStop {
            offset: 0.0,
            color: 0,
            alpha: 1.0,
        },
        ColorStop {
            offset: 1.0,
            color: 2,
            alpha: 0.5,
        },
    ];
    let paints = vec![
        Paint::Solid {
            color: 0,
            alpha: 1.0,
        },
        Paint::Solid {
            color: 1,
            alpha: 0.5,
        },
        Paint::Linear {
            start: [10.0, 20.0],
            end: [300.0, 600.0],
            stops: stops.clone(),
        },
        Paint::Radial {
            center: [250.0, 350.0],
            radius: 120.0,
            focus: None,
            stops,
        },
    ];
    let outline = src.glyph("H").unwrap().layer("m01").unwrap().shapes.clone();
    for p in &paints {
        let (f, key) = color::add_color_layer(&src, "H", "m01").unwrap();
        src = color::set_paint(&f, "H", &key, p.clone()).unwrap();
        for l in src
            .glyph_mut("H")
            .unwrap()
            .layers
            .iter_mut()
            .filter(|l| l.color.is_some())
        {
            l.shapes = outline.clone();
        }
    }
    {
        let m = &mut src.masters[0];
        m.guides = vec![Guide {
            x: 0.0,
            y: 500.0,
            angle: 0.0,
            name: Some("mid".into()),
            locked: true,
        }];
        m.vertical.typo_ascender = Some(800.0);
        m.vertical.win_ascent = Some(1100.0);
        m.vertical.win_descent = Some(300.0);
        m.vertical.underline_position = Some(-100.0);
    }
    let g = src.glyph_mut("H").unwrap();
    g.tags = vec!["latin".into(), "upper case".into()];
    g.note = Some("Check the crossbar".into());
    g.locked = true;
    let l = g.layer_mut("m01").unwrap();
    l.guides = vec![Guide {
        x: 100.0,
        y: 0.0,
        angle: 90.0,
        name: None,
        locked: false,
    }];
    l.background = Some(Background {
        shapes: outline.clone(),
        anchors: Vec::new(),
    });

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Extras.glyphs");
    kernest_import::write_glyphs(&src, &path).unwrap();
    let imp = import(&path).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let f = &imp.font;

    assert!(f.info.use_typo_metrics);
    assert_eq!(f.masters[0].guides, src.masters[0].guides);
    assert_eq!(f.masters[0].vertical, src.masters[0].vertical);
    assert_eq!(f.palettes, src.palettes);
    assert_eq!(f.master_palettes, src.master_palettes);
    let (a, b) = (f.glyph("H").unwrap(), src.glyph("H").unwrap());
    assert_eq!((&a.tags, &a.note, a.locked), (&b.tags, &b.note, b.locked));
    let (la, lb) = (a.layer("m01").unwrap(), b.layer("m01").unwrap());
    assert_eq!(la.guides, lb.guides);
    assert_eq!(la.background, lb.background);
    for m in ["m01", "m02"] {
        let got: Vec<&Paint> = a
            .color_layers(m)
            .map(|l| &l.color.as_ref().unwrap().paint)
            .collect();
        assert_eq!(got.len(), paints.len(), "{m}");
        for (g, want) in got.iter().zip(&paints) {
            let close = |x: f64, y: f64| (x - y).abs() < 1e-6;
            match (g, want) {
                (
                    Paint::Linear { start, end, .. },
                    Paint::Linear {
                        start: s2, end: e2, ..
                    },
                ) => {
                    assert!(
                        close(start[0], s2[0])
                            && close(start[1], s2[1])
                            && close(end[0], e2[0])
                            && close(end[1], e2[1]),
                        "{g:?}"
                    );
                }
                (
                    Paint::Radial { center, radius, .. },
                    Paint::Radial {
                        center: c2,
                        radius: r2,
                        ..
                    },
                ) => {
                    assert!(
                        close(center[0], c2[0]) && close(center[1], c2[1]) && close(*radius, *r2),
                        "{g:?}"
                    );
                }
                (
                    Paint::Solid { color, alpha },
                    Paint::Solid {
                        color: c2,
                        alpha: a2,
                    },
                ) => {
                    assert_eq!(color, c2);
                    assert!((alpha - a2).abs() < 0.01, "{g:?}");
                }
                _ => panic!("{g:?} vs {want:?}"),
            }
        }
        assert!(a.color_layers(m).all(|l| !l.shapes.is_empty()));
    }
}

#[test]
fn glyphs_round_trip_keeps_custom_parameters_and_variable_settings() {
    use kernest_core::{CustomParameter, VariableFontSetting};
    let mut src = two_masters();
    src.info.custom_parameters = vec![
        CustomParameter {
            name: "fsType".into(),
            value: "(\n)".into(),
            disabled: false,
        },
        CustomParameter {
            name: "Write lastChange".into(),
            value: "0".into(),
            disabled: true,
        },
        CustomParameter {
            name: "panose".into(),
            value: "(\n2,\n0,\n5\n)".into(),
            disabled: false,
        },
    ];
    src.variable_fonts = vec![
        VariableFontSetting {
            name: "Upright".into(),
            file_name: "Test-VF".into(),
            axes: vec![],
            glyphs: vec![],
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
        },
        VariableFontSetting {
            name: "Text".into(),
            file_name: "TestText-VF".into(),
            axes: vec![],
            glyphs: vec!["H".into(), "T".into()],
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
        },
    ];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Params.glyphs");
    kernest_import::write_glyphs(&src, &path).unwrap();
    let imp = import(&path).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.font.info.custom_parameters, src.info.custom_parameters);
    assert_eq!(imp.font.variable_fonts, src.variable_fonts);
}

#[test]
fn glyphs_right_to_left_kerning_is_imported() {
    let text = r#"{
.formatVersion = 3;
familyName = RTL;
fontMaster = (
{
id = m1;
name = Regular;
}
);
glyphs = (
{
glyphname = beh;
kernLeft = beh;
kernRight = beh;
layers = (
{
layerId = m1;
width = 500;
}
);
unicode = 1576;
},
{
glyphname = alef;
layers = (
{
layerId = m1;
width = 300;
}
);
unicode = 1575;
}
);
kerningLTR = {
m1 = {
alef = {
beh = 5;
};
};
};
kerningRTL = {
m1 = {
"@MMK_R_beh" = {
alef = -40;
};
alef = {
beh = -12;
};
};
};
unitsPerEm = 1000;
}
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Rtl.glyphs");
    fs::write(&path, text).unwrap();
    let imp = import(&path).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let pairs = &imp.font.kerning["m01"];
    let value = |l: &str, r: &str| {
        pairs
            .iter()
            .find(|p| p.left == l && p.right == r)
            .map(|p| p.value)
    };
    assert_eq!(value("@beh", "alef"), Some(-40.0));
    // The RTL pair replaces the LTR pair with the same sides.
    assert_eq!(value("alef", "beh"), Some(-12.0));
}

#[test]
fn custom_parameter_values_must_be_glyphs_syntax() {
    use kernest_import::check_parameter_value as check;
    for ok in [
        "1",
        "\"some text\"",
        "(\n)",
        "(a, b)",
        "{key = value;}",
        "word",
    ] {
        assert!(check("p", ok).is_ok(), "{ok}");
    }
    for bad in ["(a, b", "\"open", "{key = value"] {
        assert!(check("p", bad).is_err(), "{bad}");
    }
}

#[test]
fn glyphs_round_trip_keeps_backup_layers() {
    let src = two_masters();
    let (src, key) = kernest_core::layers::add_backup(&src, "H", "m02", "legacy").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Backups.glyphs");
    kernest_import::write_glyphs(&src, &path).unwrap();
    let imp = import(&path).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let g = imp.font.glyph("H").unwrap();
    let backups: Vec<_> = g.layers.iter().filter(|l| l.backup.is_some()).collect();
    assert_eq!(backups.len(), 1);
    let want = src.glyph("H").unwrap().layer(&key).unwrap();
    assert_eq!(
        (
            backups[0].backup.as_deref(),
            &backups[0].master_id,
            &backups[0].shapes
        ),
        (Some("legacy"), &want.master_id, &want.shapes)
    );
}

#[test]
fn ufo_round_trip_keeps_guides_and_backgrounds() {
    use kernest_core::{Background, Guide};
    let mut src = two_masters();
    src.masters[1].guides = vec![Guide {
        x: 0.0,
        y: 620.0,
        angle: 0.0,
        name: Some("overshoot".into()),
        locked: false,
    }];
    let outline = src.glyph("H").unwrap().layer("m01").unwrap().shapes.clone();
    let l = src.glyph_mut("H").unwrap().layer_mut("m02").unwrap();
    l.guides = vec![
        Guide {
            x: 120.0,
            y: 0.0,
            angle: 90.0,
            name: None,
            locked: false,
        },
        Guide {
            x: 10.0,
            y: 20.0,
            angle: 30.0,
            name: Some("slant".into()),
            locked: false,
        },
    ];
    l.background = Some(Background {
        shapes: outline,
        anchors: Vec::new(),
    });
    let dir = tempfile::tempdir().unwrap();
    let opts = kernest_export::UfoExportOptions {
        masters: vec![],
        decompose_smart: false,
        color_layers: false,
    };
    kernest_export::export_ufo(&src, dir.path(), &opts).unwrap();
    let imp = import(&dir.path().join("font.designspace")).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let f = &imp.font;
    let m = f
        .masters
        .iter()
        .find(|m| m.name == src.masters[1].name)
        .unwrap();
    assert_eq!(m.guides, src.masters[1].guides);
    let (a, b) = (
        f.glyph("H").unwrap().layer(&m.id).unwrap(),
        src.glyph("H").unwrap().layer("m02").unwrap(),
    );
    assert_eq!(a.guides, b.guides);
    assert_eq!(a.background, b.background);
}

#[test]
fn glyphs_round_trip_keeps_strokes() {
    use kernest_core::stroke::{PathStroke, StrokeCap};
    let mut src = two_masters();
    for m in ["m01", "m02"] {
        if let Shape::Path(p) = &mut src.glyph_mut("H").unwrap().layer_mut(m).unwrap().shapes[0] {
            let mut s = PathStroke {
                width: if m == "m01" { 30.0 } else { 70.0 },
                end_cap: StrokeCap::Round,
                ..Default::default()
            };
            s.widths.insert(1, 50.0);
            p.stroke = Some(s);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Strokes.glyphs");
    kernest_import::write_glyphs(&src, &path).unwrap();
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("strokeWidth = 30;"));
    let imp = import(&path).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    for m in ["m01", "m02"] {
        let stroke = |f: &Font| match &f.glyph("H").unwrap().layer(m).unwrap().shapes[0] {
            Shape::Path(p) => p.stroke.clone(),
            _ => None,
        };
        assert_eq!(stroke(&imp.font), stroke(&src), "{m}");
    }
}

#[test]
fn glyphs_round_trip_keeps_corner_components_live() {
    use kernest_core::parts::{add_part, apply, PartKind, PathPart};
    let mut src = two_masters();
    // A cut corner glyph: from 20 up the y axis to 20 back along the x axis.
    let mut g = kernest_core::Glyph::new("_corner.cut", vec![]);
    g.export = false;
    for m in src.masters.clone() {
        let mut l = kernest_core::Layer::new(m.id.clone(), 0.0);
        l.shapes.push(Shape::Path(kernest_core::Path {
            closed: false,
            nodes: vec![
                kernest_core::Node::new(0.0, 20.0, NodeKind::Line),
                kernest_core::Node::new(-20.0, 0.0, NodeKind::Line),
            ],
            locked: false,
            stroke: None,
            mask: false,
        }));
        g.layers.push(l);
    }
    src.glyphs.push(g);
    let src = add_part(
        &src,
        "H",
        "m01",
        PathPart::new(PartKind::Corner, "_corner.cut", 0, 1),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Corners.glyphs");
    kernest_import::write_glyphs(&src, &path).unwrap();
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("type = Corner;"));
    let imp = import(&path).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    for m in ["m01", "m02"] {
        let (a, b) = (
            imp.font.glyph("H").unwrap().layer(m).unwrap(),
            src.glyph("H").unwrap().layer(m).unwrap(),
        );
        assert_eq!(a.parts, b.parts, "{m}: the corner is live again");
        assert_eq!(
            a.shapes, b.shapes,
            "{m}: the outline is the one drawn, without the corner"
        );
        let drawn = |f: &Font, l: &kernest_core::Layer| apply(f, l).shapes;
        assert_eq!(drawn(&imp.font, a), drawn(&src, b));
    }
}

#[test]
fn glyphs_round_trip_keeps_masks_annotations_images_virtual_masters_and_swaps() {
    use kernest_core::{Annotation, AnnotationKind, LayerImage, VirtualMaster};
    let mut src = two_masters();
    let axis_tag = src.axes[0].tag.clone();
    {
        let l = src.glyph_mut("H").unwrap().layer_mut("m01").unwrap();
        if let Some(Shape::Path(p)) = l.shapes.iter_mut().find(|s| matches!(s, Shape::Path(_))) {
            p.mask = true;
        }
        l.annotations = vec![
            Annotation {
                kind: AnnotationKind::Text,
                x: 10.0,
                y: 600.0,
                text: "Bowl too light".into(),
                angle: 0.0,
                size: 220.0,
            },
            Annotation {
                kind: AnnotationKind::Arrow,
                x: 100.0,
                y: 300.0,
                text: String::new(),
                angle: 225.0,
                size: 0.0,
            },
            Annotation {
                kind: AnnotationKind::Plus,
                x: 50.0,
                y: 50.0,
                text: String::new(),
                angle: 0.0,
                size: 0.0,
            },
        ];
        l.image = Some(LayerImage {
            path: "scans/H.png".into(),
            x: 20.0,
            y: -100.0,
            scale_x: 0.5,
            scale_y: 0.5,
            angle: 0.0,
            crop: Some([0.0, 0.0, 400.0, 900.0]),
            locked: true,
        });
        l.hoi = vec![kernest_core::hoi::HoiCurve {
            axis: axis_tag.clone(),
            path: 0,
            node: 1,
            kind: kernest_core::hoi::HoiKind::Through { x: 120.0, y: 40.0 },
        }];
    }
    let wght = src.axes[0].clone();
    src.info.virtual_masters = vec![VirtualMaster {
        name: "Virtual master 1".into(),
        location: BTreeMap::from([(wght.tag.clone(), wght.max)]),
    }];
    src.instances[0].rename_glyphs = vec![["H".into(), "T".into()]];
    src.instances[0].remove_glyphs = vec!["T".into()];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Extras29.glyphs");
    kernest_import::write_glyphs(&src, &path).unwrap();
    let imp = import(&path).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let (a, b) = (
        imp.font.glyph("H").unwrap().layer("m01").unwrap(),
        src.glyph("H").unwrap().layer("m01").unwrap(),
    );
    let masks = |l: &kernest_core::Layer| {
        l.shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Path(p) => Some(p.mask),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(masks(a), masks(b));
    assert_eq!(a.annotations, b.annotations);
    assert_eq!(a.hoi, b.hoi);
    let (ia, ib) = (a.image.clone().unwrap(), b.image.clone().unwrap());
    assert_eq!((ia.path, ia.crop, ia.locked), (ib.path, ib.crop, ib.locked));
    assert!(
        (ia.x - ib.x).abs() < 1e-6
            && (ia.scale_x - ib.scale_x).abs() < 1e-6
            && ia.angle.abs() < 1e-6
    );
    assert_eq!(imp.font.info.virtual_masters, src.info.virtual_masters);
    let inst = imp
        .font
        .instances
        .iter()
        .find(|i| i.name == src.instances[0].name)
        .unwrap();
    assert_eq!(
        (&inst.rename_glyphs, &inst.remove_glyphs),
        (
            &src.instances[0].rename_glyphs,
            &src.instances[0].remove_glyphs
        )
    );
}
