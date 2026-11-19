//! Hinting controls: alignment zones, stems, manual glyph hints and the
//! automatic PostScript stem generator.
//!
//! Each setter returns an edited copy of the font; the caller stores it through
//! `History::apply_font` (or `History::apply` for a layer) so it can be undone.

use crate::geometry::{for_each_path, segments};
use crate::model::{Font, Hint, Layer, NodeKind, PsHinting, Stem, Zone, IDENTITY};

type R<T> = Result<T, String>;

/// Replaces a master's alignment zones.
pub fn set_alignment_zones(font: &Font, master_id: &str, zones: Vec<Zone>) -> R<Font> {
    for z in &zones {
        if !z.position.is_finite() || !z.size.is_finite() {
            return Err("Zone positions and sizes must be numbers.".into());
        }
        if z.size == 0.0 {
            return Err("A zone needs a size other than 0.".into());
        }
    }
    let mut f = font.clone();
    let m = f
        .masters
        .iter_mut()
        .find(|m| m.id == master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
    m.alignment_zones = zones;
    Ok(f)
}

/// Replaces a master's standard stems.
pub fn set_stems(font: &Font, master_id: &str, stems: Vec<Stem>) -> R<Font> {
    for s in &stems {
        if s.name.trim().is_empty() {
            return Err("Every stem needs a name.".into());
        }
        if !s.value.is_finite() || s.value <= 0.0 {
            return Err("Stem widths must be greater than 0.".into());
        }
    }
    let mut f = font.clone();
    let m = f
        .masters
        .iter_mut()
        .find(|m| m.id == master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
    m.stems = stems;
    Ok(f)
}

/// Replaces one layer's manual hints. `key` is the master id or a layer id.
pub fn set_glyph_hints(font: &Font, glyph: &str, key: &str, hints: Vec<Hint>) -> R<Font> {
    for h in &hints {
        if !h.position.is_finite() || !h.width.is_finite() || h.width < 0.0 {
            return Err("Hint positions must be numbers and widths 0 or more.".into());
        }
    }
    let mut f = font.clone();
    let layer = f
        .glyph_mut(glyph)
        .and_then(|g| g.layer_mut(key))
        .ok_or_else(|| format!("No layer \"{key}\" for glyph \"{glyph}\""))?;
    layer.hints = hints;
    Ok(f)
}

/// Sets the TTFAutohint control instructions (empty clears them).
pub fn set_control_instructions(font: &Font, text: String) -> R<Font> {
    let mut f = font.clone();
    f.info.control_instructions = text.trim_end().to_string();
    Ok(f)
}

/// Sets or clears the PostScript blue zone controls (`None` restores defaults).
pub fn set_ps_hinting(font: &Font, ps: Option<PsHinting>) -> R<Font> {
    if let Some(p) = &ps {
        if !p.blue_scale.is_finite() || p.blue_scale <= 0.0 {
            return Err("Blue scale must be greater than 0.".into());
        }
        if !p.blue_shift.is_finite() || p.blue_shift < 0.0 {
            return Err("Blue shift must be 0 or more.".into());
        }
        if !p.blue_fuzz.is_finite() || p.blue_fuzz < 0.0 {
            return Err("Blue fuzz must be 0 or more.".into());
        }
    }
    let mut f = font.clone();
    f.info.ps_hinting = ps.filter(|p| *p != PsHinting::default());
    Ok(f)
}

/// BlueValues and OtherBlues from alignment zones, the way Glyphs splits them:
/// the baseline zone and every top zone go to BlueValues, bottom zones below the
/// baseline to OtherBlues. Zero-size zones are left out and duplicates merged.
pub fn blues_from_zones(zones: &[Zone]) -> (Vec<f64>, Vec<f64>) {
    let mut seen: Vec<(f64, f64)> = zones
        .iter()
        .filter(|z| z.size != 0.0)
        .map(|z| (z.position, z.size))
        .collect();
    seen.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    seen.dedup();
    let (mut blue, mut other) = (Vec::new(), Vec::new());
    for (pos, size) in seen {
        let list = if pos == 0.0 || size >= 0.0 {
            &mut blue
        } else {
            &mut other
        };
        let (a, b) = if size >= 0.0 {
            (pos, pos + size)
        } else {
            (pos + size, pos)
        };
        list.push(a);
        list.push(b);
    }
    (blue, other)
}

/// Alignment zones from blue value pairs (the inverse of [`blues_from_zones`]),
/// ordered from the top down as Glyphs lists them.
pub fn zones_from_blues(blue: &[f64], other: &[f64]) -> Vec<Zone> {
    let mut zones = Vec::new();
    for pair in blue.as_chunks::<2>().0 {
        let (y1, y2) = (pair[0], pair[1]);
        if y2 == 0.0 {
            zones.push(Zone {
                position: 0.0,
                size: -(y2 - y1),
            });
        } else {
            zones.push(Zone {
                position: y1,
                size: y2 - y1,
            });
        }
    }
    for pair in other.as_chunks::<2>().0 {
        zones.push(Zone {
            position: pair[1],
            size: pair[0] - pair[1],
        });
    }
    zones.sort_by(|a, b| b.position.total_cmp(&a.position));
    zones
}

/// The StemSnapH or StemSnapV list for a master's stems: widths in ascending
/// order, without duplicates, at most 12 (the CFF limit).
pub fn stem_snaps(stems: &[Stem], horizontal: bool) -> Vec<f64> {
    let mut v: Vec<f64> = stems
        .iter()
        .filter(|s| s.horizontal == horizontal)
        .map(|s| s.value)
        .collect();
    v.sort_by(|a, b| a.total_cmp(b));
    v.dedup();
    v.truncate(12);
    v
}

/// Two edges count as the same hint edge when they are this close (font units).
const EDGE_EPS: f64 = 0.01;

/// Automatic stem hints for a layer: pairs of parallel straight edges (or curve
/// extremes with a horizontal/vertical tangent) whose distance matches one of
/// the master's stem widths. Candidates closest to a stem width win; hints on
/// the same axis never overlap. Components are resolved first.
pub fn auto_stems(font: &Font, layer: &Layer, horizontal: &[f64], vertical: &[f64]) -> Vec<Hint> {
    let mut h_edges: Vec<f64> = Vec::new();
    let mut v_edges: Vec<f64> = Vec::new();
    for_each_path(font, layer, &IDENTITY, 0, &mut |path, t| {
        // Only axis-aligned transforms keep edges axis-aligned.
        if t[1].abs() > 1e-9 || t[2].abs() > 1e-9 {
            return;
        }
        let tx = |x: f64, y: f64| (t[0] * x + t[4], t[3] * y + t[5]);
        let mut in_flat: Vec<bool> = vec![false; path.nodes.len()];
        let mut out_flat: Vec<bool> = vec![false; path.nodes.len()];
        let mut in_vert: Vec<bool> = vec![false; path.nodes.len()];
        let mut out_vert: Vec<bool> = vec![false; path.nodes.len()];
        let index = |n: &crate::model::Node| {
            path.nodes
                .iter()
                .position(|m| m.x == n.x && m.y == n.y && m.kind == n.kind)
        };
        for (start, offs, end) in segments(path) {
            let (sx, sy) = tx(start.x, start.y);
            let (ex, ey) = tx(end.x, end.y);
            if offs.is_empty() {
                if (sy - ey).abs() < EDGE_EPS {
                    h_edges.push(sy);
                }
                if (sx - ex).abs() < EDGE_EPS {
                    v_edges.push(sx);
                }
                continue;
            }
            // A curve extreme: both handles at the node run along the axis.
            let first = tx(offs[0].x, offs[0].y);
            let last = tx(offs[offs.len() - 1].x, offs[offs.len() - 1].y);
            if let (Some(i), Some(j)) = (index(&start), index(&end)) {
                if (first.1 - sy).abs() < EDGE_EPS {
                    out_flat[i] = true;
                }
                if (last.1 - ey).abs() < EDGE_EPS {
                    in_flat[j] = true;
                }
                if (first.0 - sx).abs() < EDGE_EPS {
                    out_vert[i] = true;
                }
                if (last.0 - ex).abs() < EDGE_EPS {
                    in_vert[j] = true;
                }
            }
        }
        for (i, node) in path.nodes.iter().enumerate() {
            if !node.on_curve() || node.kind == NodeKind::Line {
                continue;
            }
            let (x, y) = tx(node.x, node.y);
            if in_flat[i] && out_flat[i] {
                h_edges.push(y);
            }
            if in_vert[i] && out_vert[i] {
                v_edges.push(x);
            }
        }
    });
    let mut hints = pick_stems(&h_edges, horizontal, true);
    hints.extend(pick_stems(&v_edges, vertical, false));
    hints
}

/// Deduplicates edges, then greedily pairs them into non-overlapping stems,
/// preferring widths closest to one of `values`.
fn pick_stems(edges: &[f64], values: &[f64], horizontal: bool) -> Vec<Hint> {
    if values.is_empty() || edges.len() < 2 {
        return Vec::new();
    }
    let mut sorted = edges.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mut uniq: Vec<f64> = Vec::new();
    for e in sorted {
        if uniq.last().is_none_or(|last| (e - last).abs() >= EDGE_EPS) {
            uniq.push(e);
        }
    }
    let (min, max) = (
        values.iter().copied().fold(f64::INFINITY, f64::min),
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    let mut candidates: Vec<(f64, Hint)> = Vec::new();
    for (i, &a) in uniq.iter().enumerate() {
        for &b in &uniq[i + 1..] {
            let width = b - a;
            if width < min - EDGE_EPS || width > max + EDGE_EPS {
                continue;
            }
            let score = values
                .iter()
                .map(|v| (width - v).abs())
                .fold(f64::INFINITY, f64::min);
            candidates.push((
                score,
                Hint {
                    horizontal,
                    position: a,
                    width,
                    ghost_top: false,
                },
            ));
        }
    }
    candidates.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.position.total_cmp(&b.1.position))
    });
    let mut picked: Vec<Hint> = Vec::new();
    for (_, hint) in candidates {
        let end = hint.position + hint.width;
        let overlaps = picked.iter().any(|p| {
            hint.position < p.position + p.width - EDGE_EPS && p.position < end - EDGE_EPS
        });
        if !overlaps {
            picked.push(hint);
        }
    }
    picked.sort_by(|a, b| a.position.total_cmp(&b.position));
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, Path, Shape};
    use crate::sample::sample_font;

    fn path(points: &[(f64, f64)]) -> Shape {
        Shape::Path(Path {
            closed: true,
            nodes: points
                .iter()
                .map(|&(x, y)| Node::new(x, y, NodeKind::Line))
                .collect(),
            locked: false,
        })
    }

    /// An H: two 100-wide bars and a 50-tall crossbar between y 330 and 380.
    fn h_layer() -> Layer {
        let mut l = sample_font().glyphs[0].layers[0].clone();
        l.shapes = vec![path(&[
            (80.0, 0.0),
            (180.0, 0.0),
            (180.0, 330.0),
            (420.0, 330.0),
            (420.0, 0.0),
            (520.0, 0.0),
            (520.0, 700.0),
            (420.0, 700.0),
            (420.0, 380.0),
            (180.0, 380.0),
            (180.0, 700.0),
            (80.0, 700.0),
        ])];
        l
    }

    /// An O: two contours of four curve nodes with axis-aligned handles,
    /// stroke 100, outer extremes at x 80/520 and y -10/710.
    fn o_layer() -> Layer {
        let contour = |rx: f64, ry: f64, cy: f64, k: f64| {
            // bottom, right, top, left; handles run along the tangent.
            let nodes = vec![
                Node::new(300.0, cy - ry, NodeKind::Curve),
                Node::new(300.0 + k * rx, cy - ry, NodeKind::OffCurve),
                Node::new(300.0 + rx, cy - k * ry, NodeKind::OffCurve),
                Node::new(300.0 + rx, cy, NodeKind::Curve),
                Node::new(300.0 + rx, cy + k * ry, NodeKind::OffCurve),
                Node::new(300.0 + k * rx, cy + ry, NodeKind::OffCurve),
                Node::new(300.0, cy + ry, NodeKind::Curve),
                Node::new(300.0 - k * rx, cy + ry, NodeKind::OffCurve),
                Node::new(300.0 - rx, cy + k * ry, NodeKind::OffCurve),
                Node::new(300.0 - rx, cy, NodeKind::Curve),
                Node::new(300.0 - rx, cy - k * ry, NodeKind::OffCurve),
                Node::new(300.0 - k * rx, cy - ry, NodeKind::OffCurve),
            ];
            Shape::Path(Path {
                closed: true,
                nodes,
                locked: false,
            })
        };
        let mut l = sample_font().glyphs[0].layers[0].clone();
        l.shapes = vec![
            contour(220.0, 360.0, 350.0, 0.55),
            contour(120.0, 260.0, 350.0, 0.55),
        ];
        l
    }

    #[test]
    fn zones_split_into_blues_like_glyphs() {
        let zones = vec![
            Zone {
                position: 800.0,
                size: 10.0,
            },
            Zone {
                position: 0.0,
                size: -10.0,
            },
            Zone {
                position: -200.0,
                size: -10.0,
            },
            Zone {
                position: 500.0,
                size: 10.0,
            },
            Zone {
                position: 500.0,
                size: 10.0,
            },
            Zone {
                position: 700.0,
                size: 0.0,
            },
        ];
        let (blue, other) = blues_from_zones(&zones);
        assert_eq!(blue, [-10.0, 0.0, 500.0, 510.0, 800.0, 810.0]);
        assert_eq!(other, [-210.0, -200.0]);
        let back = zones_from_blues(&blue, &other);
        assert_eq!(
            back[0],
            Zone {
                position: 800.0,
                size: 10.0
            }
        );
        assert!(back.contains(&Zone {
            position: 0.0,
            size: -10.0
        }));
        assert!(back.contains(&Zone {
            position: -200.0,
            size: -10.0
        }));
        assert_eq!(back.len(), 4);
    }

    #[test]
    fn stem_snaps_sort_dedup_and_cap() {
        let stem = |name: &str, horizontal: bool, value: f64| Stem {
            name: name.into(),
            horizontal,
            value,
        };
        let stems = vec![
            stem("a", true, 100.0),
            stem("b", true, 50.0),
            stem("c", true, 100.0),
            stem("d", false, 90.0),
        ];
        assert_eq!(stem_snaps(&stems, true), [50.0, 100.0]);
        assert_eq!(stem_snaps(&stems, false), [90.0]);
        let many: Vec<Stem> = (0..15)
            .map(|i| stem(&format!("s{i}"), true, f64::from(i) + 1.0))
            .collect();
        assert_eq!(stem_snaps(&many, true).len(), 12);
    }

    #[test]
    fn auto_stems_match_an_h() {
        let f = sample_font();
        let hints = auto_stems(&f, &h_layer(), &[50.0], &[100.0]);
        let h: Vec<&Hint> = hints.iter().filter(|h| h.horizontal).collect();
        let v: Vec<&Hint> = hints.iter().filter(|h| !h.horizontal).collect();
        assert_eq!(h.len(), 1, "the crossbar is the only horizontal stem");
        assert_eq!((h[0].position, h[0].width), (330.0, 50.0));
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].position, v[0].width), (80.0, 100.0));
        assert_eq!((v[1].position, v[1].width), (420.0, 100.0));
    }

    #[test]
    fn auto_stems_match_an_o() {
        let f = sample_font();
        let hints = auto_stems(&f, &o_layer(), &[100.0], &[100.0]);
        let h: Vec<&Hint> = hints.iter().filter(|h| h.horizontal).collect();
        let v: Vec<&Hint> = hints.iter().filter(|h| !h.horizontal).collect();
        assert_eq!(h.len(), 2, "top and bottom of the ring");
        assert_eq!((h[0].position, h[0].width), (-10.0, 100.0));
        assert_eq!((h[1].position, h[1].width), (610.0, 100.0));
        assert_eq!(v.len(), 2, "left and right of the ring");
        assert_eq!((v[0].position, v[0].width), (80.0, 100.0));
        assert_eq!((v[1].position, v[1].width), (420.0, 100.0));
    }

    #[test]
    fn no_stems_means_no_auto_hints() {
        let f = sample_font();
        assert!(auto_stems(&f, &h_layer(), &[], &[]).is_empty());
    }

    #[test]
    fn setters_validate_and_round_trip() {
        let f = sample_font();
        let id = f.masters[0].id.clone();
        assert!(set_alignment_zones(
            &f,
            &id,
            vec![Zone {
                position: 0.0,
                size: 0.0
            }]
        )
        .is_err());
        let zones = vec![Zone {
            position: 700.0,
            size: 10.0,
        }];
        let f = set_alignment_zones(&f, &id, zones.clone()).unwrap();
        assert_eq!(f.masters[0].alignment_zones, zones);
        assert!(set_stems(
            &f,
            &id,
            vec![Stem {
                name: " ".into(),
                horizontal: true,
                value: 80.0
            }]
        )
        .is_err());
        assert!(set_stems(
            &f,
            &id,
            vec![Stem {
                name: "h".into(),
                horizontal: true,
                value: 0.0
            }]
        )
        .is_err());
        let stems = vec![Stem {
            name: "h0".into(),
            horizontal: true,
            value: 80.0,
        }];
        let f = set_stems(&f, &id, stems.clone()).unwrap();
        assert_eq!(f.masters[0].stems, stems);
        let g = f.glyphs[0].name.clone();
        let hints = vec![Hint {
            horizontal: true,
            position: 0.0,
            width: 0.0,
            ghost_top: false,
        }];
        let f = set_glyph_hints(&f, &g, &id, hints.clone()).unwrap();
        assert_eq!(f.glyphs[0].layers[0].hints, hints);
        assert!(set_glyph_hints(
            &f,
            &g,
            &id,
            vec![Hint {
                horizontal: true,
                position: 0.0,
                width: -1.0,
                ghost_top: false
            }]
        )
        .is_err());
        let f = set_control_instructions(&f, "H 10\n".into()).unwrap();
        assert_eq!(f.info.control_instructions, "H 10");
        assert!(set_ps_hinting(
            &f,
            Some(PsHinting {
                blue_scale: 0.0,
                ..Default::default()
            })
        )
        .is_err());
        let f = set_ps_hinting(&f, Some(PsHinting::default())).unwrap();
        assert_eq!(f.info.ps_hinting, None, "defaults are stored as None");
    }
}
