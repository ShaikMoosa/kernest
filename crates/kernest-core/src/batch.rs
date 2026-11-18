//! Batch path operations over a Font View selection.
//!
//! The single-glyph commands edit one layer at a time; this applies the same edits to
//! many glyphs at once so a whole selection can be cleaned up without opening each
//! glyph. On a compatible master layer the change reaches every master and brace layer
//! together, exactly as the single-glyph command does, so the masters stay compatible.

use crate::interpolate::check_glyph;
use crate::model::Font;
use crate::{overlap, pathops, smart};

type R<T> = Result<T, String>;

/// Applies `operation` to every glyph in `glyphs`. `master_id` is the active master;
/// it decides which layer seeds a direction plan and which layer remove-overlap edits.
pub fn path_operation(font: &Font, glyphs: &[String], master_id: &str, operation: &str) -> R<Font> {
    match operation {
        "correct-direction" | "remove-overlap" | "tidy-up" | "round-coordinates" | "decompose" => {}
        other => return Err(format!("Unknown path operation \"{other}\"")),
    }
    let mut next = font.clone();
    for name in glyphs {
        let g = font
            .glyph(name)
            .ok_or_else(|| format!("There is no glyph named \"{name}\"."))?;
        let ng = next.glyph_mut(name).expect("glyph exists");
        if operation == "decompose" {
            for layer in &mut ng.layers {
                *layer = smart::decompose_layer(font, layer);
            }
            continue;
        }
        let current = g
            .layer(master_id)
            .ok_or_else(|| format!("\"{name}\" has no layer for that master."))?;
        // Remove-overlap can change topology differently in each master, so it stays on
        // the active layer; the other edits keep compatible masters in step.
        let all_masters = operation != "remove-overlap"
            && current.is_master()
            && font.masters.len() > 1
            && check_glyph(font, name).is_empty();
        let targets: Vec<String> = if all_masters {
            g.layers
                .iter()
                .filter(|l| l.is_master() || l.location.is_some())
                .map(|l| l.key().to_string())
                .collect()
        } else {
            vec![master_id.to_string()]
        };
        if operation == "correct-direction" {
            let plan = pathops::direction_plan(current);
            for id in &targets {
                if let Some(l) = ng.layer_mut(id) {
                    pathops::apply_direction(l, &plan);
                }
            }
        } else {
            for id in &targets {
                let Some(l) = ng.layer_mut(id) else { continue };
                match operation {
                    "remove-overlap" => {
                        overlap::remove_overlaps(l);
                    }
                    "tidy-up" => {
                        pathops::tidy_up(l, None);
                    }
                    "round-coordinates" => {
                        pathops::round_coordinates(l, None);
                    }
                    _ => unreachable!("validated above"),
                }
            }
        }
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Shape;
    use crate::sample::sample_font;

    fn two_masters() -> Font {
        let mut f = sample_font();
        let mut m2 = f.masters[0].clone();
        m2.id = "m02".into();
        m2.name = "Bold".into();
        f.masters.push(m2);
        for g in &mut f.glyphs {
            let masters: Vec<_> = g.layers.iter().filter(|l| l.is_master()).cloned().collect();
            for mut l in masters {
                l.master_id = "m02".into();
                g.layers.push(l);
            }
        }
        f
    }

    #[test]
    fn decomposes_components_into_outlines() {
        let mut f = sample_font();
        let m = f.masters[0].id.clone();
        let h_paths = f
            .glyph("H")
            .unwrap()
            .layer(&m)
            .unwrap()
            .shapes
            .iter()
            .filter(|s| matches!(s, Shape::Path(_)))
            .count();
        f.glyph_mut("A")
            .unwrap()
            .layer_mut(&m)
            .unwrap()
            .shapes
            .push(Shape::component("H", [1.0, 0.0, 0.0, 1.0, 10.0, 20.0]));
        let out = path_operation(&f, &["A".into()], &m, "decompose").unwrap();
        let l = out.glyph("A").unwrap().layer(&m).unwrap();
        assert!(l.shapes.iter().all(|s| matches!(s, Shape::Path(_))));
        let added = l
            .shapes
            .iter()
            .filter(|s| matches!(s, Shape::Path(_)))
            .count();
        assert!(added >= h_paths);
        out.validate().unwrap();
    }

    #[test]
    fn tidy_and_round_reach_every_compatible_master() {
        let f = two_masters();
        // Give H fractional coordinates and a stray node so tidy/round have work to do.
        let mut f = f;
        for g in ["H", "o"] {
            for l in &mut f.glyph_mut(g).unwrap().layers {
                if let Some(Shape::Path(p)) = l.shapes.first_mut() {
                    for n in &mut p.nodes {
                        n.x += 0.4;
                        n.y += 0.6;
                    }
                }
            }
        }
        let out =
            path_operation(&f, &["H".into(), "o".into()], "m01", "round-coordinates").unwrap();
        for g in ["H", "o"] {
            for id in ["m01", "m02"] {
                let l = out.glyph(g).unwrap().layer(id).unwrap();
                if let Some(Shape::Path(p)) = l.shapes.first() {
                    assert!(p
                        .nodes
                        .iter()
                        .all(|n| n.x.fract() == 0.0 && n.y.fract() == 0.0));
                }
            }
        }
        let _ = path_operation(&f, &["H".into()], "m01", "tidy-up").unwrap();
    }

    #[test]
    fn remove_overlap_stays_on_the_active_master() {
        let f = two_masters();
        let before = f.glyph("H").unwrap().layer("m02").unwrap().clone();
        let out = path_operation(&f, &["H".into()], "m01", "remove-overlap").unwrap();
        assert_eq!(out.glyph("H").unwrap().layer("m02").unwrap(), &before);
    }

    #[test]
    fn unknown_operation_and_missing_glyph_error() {
        let f = sample_font();
        let m = f.masters[0].id.clone();
        assert!(path_operation(&f, &["H".into()], &m, "nope").is_err());
        assert!(path_operation(&f, &["nothere".into()], &m, "tidy-up").is_err());
    }
}
