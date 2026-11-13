use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use kernest_core::sample::sample_font;
use kernest_core::{layers, structure, Axis, AxisRange, Font, Instance, KernPair, NodeKind, Shape};
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
fn unknown_extension() {
    let e = import(Path::new("font.otf")).unwrap_err();
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
        a_layer("M-BLD", 700, 350),
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

fn glyphs() -> Vec<(&'static str, String)> {
    vec![
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
    assert_eq!(names, ["A", "Aacute", "V", "acutecomb", "A.alt", "_part"]);
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

    let Shape::Component { base, transform } = &f.glyph("Aacute").unwrap().layers[0].shapes[1]
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
