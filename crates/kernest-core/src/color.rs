//! Colour glyphs (COLRv1 / CPAL): palettes, colour layers and their paints.
//!
//! A colour glyph has one or more colour layers in every master, stacked bottom to
//! top in layer order. Each colour layer is a drawing (paths and components) filled
//! with a paint: a palette colour, or a linear or radial gradient between palette
//! colours. The glyph's own outline stays as the black-and-white fallback.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::{Font, Glyph, Layer};

type R<T> = Result<T, String>;

/// An sRGB colour with alpha, written as `#rrggbbaa` (or `#rrggbb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    pub fn parse(s: &str) -> Option<Color> {
        let h = s.trim().strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            6 => Some(Color {
                r: byte(0)?,
                g: byte(2)?,
                b: byte(4)?,
                a: 255,
            }),
            8 => Some(Color {
                r: byte(0)?,
                g: byte(2)?,
                b: byte(4)?,
                a: byte(6)?,
            }),
            _ => None,
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{:02x}{:02x}{:02x}{:02x}",
            self.r, self.g, self.b, self.a
        )
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("\"{s}\" is not a colour like #ff8800"))
        })
    }
}

/// A point on a gradient: where it is (0 at the start, 1 at the end) and its colour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorStop {
    pub offset: f64,
    /// Palette index.
    pub color: u16,
    #[serde(default = "one")]
    pub alpha: f64,
}

fn one() -> f64 {
    1.0
}

/// How a colour layer is filled. Coordinates are in font units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Paint {
    /// One palette colour.
    #[serde(rename_all = "camelCase")]
    Solid {
        color: u16,
        #[serde(default = "one")]
        alpha: f64,
    },
    /// A gradient along the line from `start` to `end`.
    #[serde(rename_all = "camelCase")]
    Linear {
        start: [f64; 2],
        end: [f64; 2],
        stops: Vec<ColorStop>,
    },
    /// A gradient between two circles (usually one centre: from a dot outwards).
    #[serde(rename_all = "camelCase")]
    Radial {
        center: [f64; 2],
        radius: f64,
        #[serde(default)]
        focus: Option<[f64; 2]>,
        stops: Vec<ColorStop>,
    },
}

impl Paint {
    /// The palette indices this paint uses.
    pub fn colors(&self) -> impl Iterator<Item = u16> + '_ {
        let (solid, stops) = match self {
            Paint::Solid { color, .. } => (Some(*color), &[][..]),
            Paint::Linear { stops, .. } | Paint::Radial { stops, .. } => (None, stops.as_slice()),
        };
        solid.into_iter().chain(stops.iter().map(|s| s.color))
    }

    fn check(&self, palette_size: usize) -> R<()> {
        let alpha = |a: f64| (0.0..=1.0).contains(&a);
        match self {
            Paint::Solid { alpha: a, .. } if !alpha(*a) => {
                return Err("Opacity must be between 0 and 1.".into())
            }
            Paint::Linear { stops, start, end } => {
                if start == end {
                    return Err("A linear gradient needs a start and an end that differ.".into());
                }
                check_stops(stops)?;
            }
            Paint::Radial { stops, radius, .. } => {
                if radius.is_nan() || *radius <= 0.0 {
                    return Err("A radial gradient needs a radius above 0.".into());
                }
                check_stops(stops)?;
            }
            _ => {}
        }
        if palette_size == 0 {
            return Err("Add a colour palette in Font Info first.".into());
        }
        if let Some(i) = self.colors().find(|i| *i as usize >= palette_size) {
            return Err(format!("The palette has no colour {}.", i + 1));
        }
        Ok(())
    }
}

fn check_stops(stops: &[ColorStop]) -> R<()> {
    if stops.len() < 2 {
        return Err("A gradient needs at least two colour stops.".into());
    }
    if stops
        .iter()
        .any(|s| !(0.0..=1.0).contains(&s.offset) || !(0.0..=1.0).contains(&s.alpha))
    {
        return Err("Stop positions and opacities must be between 0 and 1.".into());
    }
    Ok(())
}

/// What makes a layer a colour layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorInfo {
    pub paint: Paint,
}

impl Glyph {
    /// This master's colour layers, bottom to top.
    pub fn color_layers<'a>(&'a self, master_id: &'a str) -> impl Iterator<Item = &'a Layer> + 'a {
        self.layers
            .iter()
            .filter(move |l| l.color.is_some() && l.master_id == master_id)
    }

    /// Whether the glyph is drawn in colour.
    pub fn is_color(&self) -> bool {
        self.layers.iter().any(|l| l.color.is_some())
    }
}

/// Where a colour layer is: its glyph and its position in its master's stack.
fn position(g: &Glyph, key: &str) -> R<(String, usize)> {
    let l = g
        .layer(key)
        .filter(|l| l.color.is_some())
        .ok_or("That isn't a colour layer.")?;
    let i = g
        .color_layers(&l.master_id)
        .position(|x| x.key() == key)
        .expect("layer is listed");
    Ok((l.master_id.clone(), i))
}

/// The keys of the colour layer at stack position `i` in every master.
fn same_position(font: &Font, g: &Glyph, i: usize) -> Vec<String> {
    font.masters
        .iter()
        .filter_map(|m| g.color_layers(&m.id).nth(i).map(|l| l.key().to_string()))
        .collect()
}

/// Sets the colour palettes. Every palette needs the same number of colours, and
/// colours in use can't be removed.
pub fn set_palettes(font: &Font, palettes: Vec<Vec<Color>>) -> R<Font> {
    let size = palettes.first().map_or(0, Vec::len);
    if palettes.iter().any(|p| p.len() != size) {
        return Err("Every palette needs the same number of colours.".into());
    }
    if size > u16::MAX as usize {
        return Err("A palette can have at most 65535 colours.".into());
    }
    let used = font
        .glyphs
        .iter()
        .flat_map(|g| g.layers.iter())
        .filter_map(|l| l.color.as_ref())
        .flat_map(|c| c.paint.colors())
        .max();
    if let Some(i) = used.filter(|i| *i as usize >= size) {
        return Err(format!(
            "Colour {} is used by colour layers, so it can't be removed.",
            i + 1
        ));
    }
    let mut f = font.clone();
    f.palettes = palettes;
    Ok(f)
}

/// Adds a colour layer on top of the glyph's others, in every master. The first one
/// starts as a copy of the glyph's outline; later ones start empty. Returns the new
/// layer's key in `master_id`.
pub fn add_color_layer(font: &Font, glyph: &str, master_id: &str) -> R<(Font, String)> {
    if font.palette_size() == 0 {
        return Err("Add a colour palette in Font Info first.".into());
    }
    let mut f = font.clone();
    let masters: Vec<String> = f.masters.iter().map(|m| m.id.clone()).collect();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let first = !g.is_color();
    let paint = Paint::Solid {
        color: (g.color_layers(master_id).count() % font.palette_size()) as u16,
        alpha: 1.0,
    };
    let mut key = None;
    for m in &masters {
        let Some(master) = g.layer(m) else { continue };
        let mut l = Layer::new(m, master.width);
        if first {
            l.shapes = master.shapes.clone();
        }
        l.id = Some(g.new_layer_id(m));
        l.color = Some(ColorInfo {
            paint: paint.clone(),
        });
        if m == master_id {
            key = l.id.clone();
        }
        g.layers.push(l);
    }
    let key = key.ok_or_else(|| format!("\"{glyph}\" has no layer for that master."))?;
    Ok((f, key))
}

/// Sets a colour layer's paint, in every master.
pub fn set_paint(font: &Font, glyph: &str, key: &str, paint: Paint) -> R<Font> {
    paint.check(font.palette_size())?;
    let mut f = font.clone();
    let g = f
        .glyph(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let (_, i) = position(g, key)?;
    let keys = same_position(font, g, i);
    let g = f.glyph_mut(glyph).expect("glyph exists");
    for k in keys {
        g.layer_mut(&k).expect("layer exists").color = Some(ColorInfo {
            paint: paint.clone(),
        });
    }
    Ok(f)
}

/// Moves a colour layer up (`up`) or down the stack, in every master.
pub fn move_color_layer(font: &Font, glyph: &str, key: &str, up: bool) -> R<Font> {
    let mut f = font.clone();
    let g = f
        .glyph(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let (master, i) = position(g, key)?;
    let count = g.color_layers(&master).count();
    let j = if up { i + 1 } else { i.wrapping_sub(1) };
    if j >= count {
        return Ok(f);
    }
    let masters: Vec<String> = font.masters.iter().map(|m| m.id.clone()).collect();
    let g = f.glyph_mut(glyph).expect("glyph exists");
    for m in &masters {
        let idx: Vec<usize> = g
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.color.is_some() && &l.master_id == m)
            .map(|(n, _)| n)
            .collect();
        if let (Some(&a), Some(&b)) = (idx.get(i), idx.get(j)) {
            g.layers.swap(a, b);
        }
    }
    Ok(f)
}

/// Removes a colour layer from every master.
pub fn remove_color_layer(font: &Font, glyph: &str, key: &str) -> R<Font> {
    let mut f = font.clone();
    let g = f
        .glyph(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let (_, i) = position(g, key)?;
    let keys = same_position(font, g, i);
    let g = f.glyph_mut(glyph).expect("glyph exists");
    g.layers.retain(|l| !keys.iter().any(|k| k == l.key()));
    Ok(f)
}

/// The name of the glyph that holds colour layer `i` of `glyph` in exported fonts.
pub fn layer_glyph_name(glyph: &str, i: usize) -> String {
    format!("{glyph}.color{i}")
}

/// One colour glyph as exported: its colour layers' glyphs and paints, bottom to top.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorGlyph {
    pub glyph: String,
    pub layers: Vec<(String, Paint)>,
}

/// For export: turns every colour layer into an ordinary glyph (`A.color0`, …) with
/// one layer per master, and returns the font without colour layers plus the colour
/// glyphs that refer to them. Compiling then needs nothing special: interpolation,
/// static instances, OTF and hinting treat the layer glyphs like any other.
pub fn split_color_layers(font: &Font) -> (Font, Vec<ColorGlyph>) {
    let mut f = font.clone();
    let mut extra = Vec::new();
    let mut colored = Vec::new();
    let default = font
        .masters
        .first()
        .map(|m| m.id.clone())
        .unwrap_or_default();
    for g in &mut f.glyphs {
        if !g.is_color() {
            continue;
        }
        let stack: Vec<Paint> = g
            .color_layers(&default)
            .filter_map(|l| l.color.as_ref().map(|c| c.paint.clone()))
            .collect();
        let mut layers = Vec::new();
        for (i, paint) in stack.into_iter().enumerate() {
            let name = layer_glyph_name(&g.name, i);
            let mut lg = Glyph::new(&name, Vec::new());
            lg.export = g.export;
            for m in &font.masters {
                if let Some(cl) = g.color_layers(&m.id).nth(i) {
                    let mut l = cl.clone();
                    l.id = None;
                    l.color = None;
                    lg.layers.push(l);
                }
            }
            extra.push(lg);
            layers.push((name, paint));
        }
        g.layers.retain(|l| l.color.is_none());
        if g.export {
            colored.push(ColorGlyph {
                glyph: g.name.clone(),
                layers,
            });
        }
    }
    // Layer glyphs go after all the others, so glyph ids of existing glyphs don't move.
    f.glyphs
        .extend(extra.into_iter().filter(|g| font.glyph(&g.name).is_none()));
    (f, colored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    fn red_blue() -> Vec<Vec<Color>> {
        vec![vec![
            Color::parse("#ff0000").unwrap(),
            Color::parse("#0000ffcc").unwrap(),
        ]]
    }

    #[test]
    fn colors_parse_and_print() {
        assert_eq!(Color::parse("#FF8800").unwrap().to_string(), "#ff8800ff");
        assert_eq!(
            Color::parse("#11223344").unwrap(),
            Color {
                r: 0x11,
                g: 0x22,
                b: 0x33,
                a: 0x44
            }
        );
        assert!(Color::parse("ff8800").is_none() && Color::parse("#ff88").is_none());
        let json = serde_json::to_string(&red_blue()).unwrap();
        assert_eq!(json, r##"[["#ff0000ff","#0000ffcc"]]"##);
        assert_eq!(
            serde_json::from_str::<Vec<Vec<Color>>>(&json).unwrap(),
            red_blue()
        );
    }

    #[test]
    fn colour_layers_in_every_master() {
        let f = set_palettes(&sample_font(), red_blue()).unwrap();
        let (f, key) = add_color_layer(&f, "O", "m01").unwrap();
        let g = f.glyph("O").unwrap();
        assert!(g.is_color());
        let l = g.layer(&key).unwrap();
        assert_eq!(
            l.shapes,
            g.layer("m01").unwrap().shapes,
            "the first layer copies the outline"
        );
        let (f, top) = add_color_layer(&f, "O", "m01").unwrap();
        let g = f.glyph("O").unwrap();
        assert!(g.layer(&top).unwrap().shapes.is_empty());
        assert_eq!(
            g.layer(&top).unwrap().color.as_ref().unwrap().paint,
            Paint::Solid {
                color: 1,
                alpha: 1.0
            }
        );
        f.validate().unwrap();

        // Move the top one down: order changes; paints follow their layers.
        let f = move_color_layer(&f, "O", &top, false).unwrap();
        let keys: Vec<&str> = f
            .glyph("O")
            .unwrap()
            .color_layers("m01")
            .map(|l| l.key())
            .collect();
        assert_eq!(keys, [top.as_str(), key.as_str()]);

        let grad = Paint::Linear {
            start: [0.0, 0.0],
            end: [0.0, 700.0],
            stops: vec![
                ColorStop {
                    offset: 0.0,
                    color: 0,
                    alpha: 1.0,
                },
                ColorStop {
                    offset: 1.0,
                    color: 1,
                    alpha: 1.0,
                },
            ],
        };
        let f = set_paint(&f, "O", &key, grad.clone()).unwrap();
        assert_eq!(
            f.glyph("O")
                .unwrap()
                .layer(&key)
                .unwrap()
                .color
                .as_ref()
                .unwrap()
                .paint,
            grad
        );
        // Colours in use can't be removed from the palette.
        assert!(set_palettes(&f, vec![vec![Color::BLACK]])
            .unwrap_err()
            .contains("Colour 2"));
        let f = remove_color_layer(&f, "O", &top).unwrap();
        assert_eq!(f.glyph("O").unwrap().color_layers("m01").count(), 1);
    }

    #[test]
    fn bad_paints_are_refused() {
        let f = set_palettes(&sample_font(), red_blue()).unwrap();
        let (f, key) = add_color_layer(&f, "O", "m01").unwrap();
        let err = |p: Paint| set_paint(&f, "O", &key, p).unwrap_err();
        assert!(err(Paint::Solid {
            color: 5,
            alpha: 1.0
        })
        .contains("no colour 6"));
        assert!(err(Paint::Solid {
            color: 0,
            alpha: 2.0
        })
        .contains("Opacity"));
        assert!(err(Paint::Linear {
            start: [0.0, 0.0],
            end: [0.0, 0.0],
            stops: vec![]
        })
        .contains("differ"));
        assert!(err(Paint::Radial {
            center: [0.0, 0.0],
            radius: 10.0,
            focus: None,
            stops: vec![]
        })
        .contains("two colour stops"));
        assert!(add_color_layer(&sample_font(), "O", "m01")
            .unwrap_err()
            .contains("palette"));
    }

    #[test]
    fn split_for_export() {
        let f = set_palettes(&sample_font(), red_blue()).unwrap();
        let (f, _) = add_color_layer(&f, "O", "m01").unwrap();
        let (f, _) = add_color_layer(&f, "O", "m01").unwrap();
        let (out, colored) = split_color_layers(&f);
        assert_eq!(colored.len(), 1);
        assert_eq!(colored[0].glyph, "O");
        let names: Vec<&str> = colored[0].layers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["O.color0", "O.color1"]);
        assert!(!out.glyph("O").unwrap().is_color());
        assert_eq!(
            out.glyph("O.color0").unwrap().layer("m01").unwrap().shapes,
            f.glyph("O").unwrap().layer("m01").unwrap().shapes
        );
        assert_eq!(
            &out.glyphs[out.glyphs.len() - 2].name,
            "O.color0",
            "layer glyphs go last"
        );
        out.validate().unwrap();
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    use crate::sample::sample_font;
    use crate::structure;
    use std::collections::BTreeMap;

    #[test]
    fn saved_and_read_back() {
        let f = set_palettes(&sample_font(), vec![vec![Color::parse("#e63946").unwrap()]]).unwrap();
        let (f, key) = add_color_layer(&f, "O", "m01").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("C.kernest");
        crate::package::save(&f, &path).unwrap();
        let json = std::fs::read_to_string(path.join("font.json")).unwrap();
        assert!(
            json.contains("\"formatVersion\": 2") && json.contains("#e63946ff"),
            "{json}"
        );
        let back = crate::package::open(&path).unwrap();
        assert_eq!(back.palettes, f.palettes);
        assert_eq!(
            back.glyph("O").unwrap().layer(&key),
            f.glyph("O").unwrap().layer(&key)
        );
        // Without colour, files stay at format version 1 for older Kernest versions.
        let plain = dir.path().join("P.kernest");
        crate::package::save(&sample_font(), &plain).unwrap();
        assert!(std::fs::read_to_string(plain.join("font.json"))
            .unwrap()
            .contains("\"formatVersion\": 1"));
    }

    #[test]
    fn colour_layers_must_interpolate_and_survive_axis_changes() {
        let wght = crate::Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
        };
        let f = structure::set_axes(&sample_font(), vec![wght.clone()]).unwrap();
        let (f, bold) =
            structure::add_master(&f, "m01", "Bold", &BTreeMap::from([("wght".into(), 700.0)]))
                .unwrap();
        let f = set_palettes(&f, vec![vec![Color::BLACK]]).unwrap();
        let (f, _) = add_color_layer(&f, "O", "m01").unwrap();
        assert!(crate::interpolate::check_glyph(&f, "O").is_empty());
        // Changing a colour layer's points in one master breaks interpolation.
        let mut broken = f.clone();
        let l = broken
            .glyph_mut("O")
            .unwrap()
            .layers
            .iter_mut()
            .find(|l| l.color.is_some() && l.master_id == bold)
            .unwrap();
        l.shapes.pop();
        let issues = crate::interpolate::check_glyph(&broken, "O");
        assert!(
            issues.iter().any(|i| i.contains("colour layer 1")),
            "{issues:?}"
        );
        // Re-applying the axes keeps colour layers (and smart poles).
        let again = structure::set_axes(&f, vec![wght]).unwrap();
        assert_eq!(again.glyph("O").unwrap().color_layers("m01").count(), 1);
    }
}
