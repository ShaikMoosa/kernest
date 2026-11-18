//! Metrics keys: sidebearings and widths that follow other glyphs.
//!
//! A key starts with `=` and names a glyph, optionally with arithmetic:
//!
//! | Key       | Left sidebearing           | Right sidebearing          | Width        |
//! |-----------|----------------------------|----------------------------|--------------|
//! | `=n`      | `n`'s left sidebearing     | `n`'s right sidebearing    | `n`'s width  |
//! | `=\|n`    | `n`'s right sidebearing    | `n`'s left sidebearing     | —            |
//! | `=n+10`   | plus 10 (also `-`, `*`, `/`) |                          |              |
//! | `=40`     | exactly 40                 |                            |              |
//!
//! Keys are stored per layer and applied with [`apply_layer`] or, for a whole font
//! (for example before export), [`apply_all`].

use crate::edit::{with_metric, MetricEdit};
use crate::geometry::sidebearings;
use crate::model::{Font, Layer};

/// Which metric a key sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Width,
}

#[derive(Debug, Clone, PartialEq)]
enum Source {
    /// Same side of another glyph (`=n`).
    Same(String),
    /// Opposite side of another glyph (`=|n`).
    Opposite(String),
    Value(f64),
}

#[derive(Debug, Clone, PartialEq)]
struct Key {
    source: Source,
    op: Option<(char, f64)>,
}

fn parse(key: &str) -> Result<Key, String> {
    let body = key
        .trim()
        .strip_prefix('=')
        .ok_or_else(|| format!("\"{key}\": a metrics key starts with \"=\", like =n or =|H"))?
        .trim();
    // Split off a trailing arithmetic operation: the last + - * / followed by a number.
    let mut op = None;
    let mut name = body;
    if let Some(i) = body.rfind(['+', '-', '*', '/']).filter(|&i| i > 0) {
        if let Ok(v) = body[i + 1..].trim().parse::<f64>() {
            op = Some((body.as_bytes()[i] as char, v));
            name = body[..i].trim();
        }
    }
    if name.is_empty() {
        return Err(format!("\"{key}\" doesn't name a glyph"));
    }
    if let Ok(v) = name.parse::<f64>() {
        if op.is_none() {
            return Ok(Key {
                source: Source::Value(v),
                op: None,
            });
        }
    }
    let source = match name.strip_prefix('|') {
        Some(n) if !n.trim().is_empty() => Source::Opposite(n.trim().to_string()),
        Some(_) => return Err(format!("\"{key}\" doesn't name a glyph")),
        None => Source::Same(name.to_string()),
    };
    Ok(Key { source, op })
}

/// Checks that `key` is a well-formed metrics key.
pub fn validate(key: &str) -> Result<(), String> {
    parse(key).map(|_| ())
}

/// Rewrites the glyph a metrics key follows when that glyph is renamed, keeping
/// the opposite-side mark and any arithmetic. `None` when the key is a fixed value
/// or follows a glyph that is not being renamed.
pub fn rename_in_key(
    key: &str,
    renames: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let parsed = parse(key).ok()?;
    let (name, opposite) = match &parsed.source {
        Source::Same(n) => (n, false),
        Source::Opposite(n) => (n, true),
        Source::Value(_) => return None,
    };
    let new = renames.get(name)?;
    let mut out = String::from("=");
    if opposite {
        out.push('|');
    }
    out.push_str(new);
    if let Some((op, v)) = parsed.op {
        out.push(op);
        if v.fract() == 0.0 && v.abs() < 1e15 {
            out.push_str(&format!("{}", v as i64));
        } else {
            out.push_str(&format!("{v}"));
        }
    }
    Some(out)
}

/// The other glyphs a layer's keys depend on.
fn references(layer: &Layer) -> Vec<String> {
    let k = &layer.metrics_keys;
    [&k.left, &k.right, &k.width]
        .into_iter()
        .flatten()
        .filter_map(|s| parse(s).ok())
        .filter_map(|k| match k.source {
            Source::Same(n) | Source::Opposite(n) => Some(n),
            Source::Value(_) => None,
        })
        .collect()
}

/// The value `key` gives for `side`, looking at the other glyph's layer for the same
/// master (`master_id`). `None` if the other glyph is empty there.
fn value(font: &Font, side: Side, key: &Key, master_id: &str) -> Result<Option<f64>, String> {
    let (other, opposite) = match &key.source {
        Source::Value(v) => return Ok(Some(*v)),
        Source::Same(n) => (n, false),
        Source::Opposite(n) => (n, true),
    };
    let g = font
        .glyph(other)
        .ok_or_else(|| format!("there is no glyph \"{other}\""))?;
    let l = g
        .layer(master_id)
        .ok_or_else(|| format!("\"{other}\" has no layer for this master"))?;
    let base = match side {
        Side::Width => Some(l.width),
        Side::Left | Side::Right => sidebearings(font, l).map(|(lsb, rsb)| {
            let want_left = (side == Side::Left) != opposite;
            if want_left {
                lsb
            } else {
                rsb
            }
        }),
    };
    Ok(base.map(|v| match key.op {
        Some(('+', n)) => v + n,
        Some(('-', n)) => v - n,
        Some(('*', n)) => v * n,
        Some(('/', n)) if n != 0.0 => v / n,
        _ => v,
    }))
}

/// Applies the metrics keys of one layer (found by `key`) using the font as it is.
/// Returns the updated layer, or `None` if nothing changes.
pub fn apply_layer(font: &Font, glyph: &str, key: &str) -> Result<Option<Layer>, String> {
    let layer = font
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or_else(|| format!("\"{glyph}\" has no layer \"{key}\""))?;
    let keys = layer.metrics_keys.clone();
    if keys.is_empty() {
        return Ok(None);
    }
    let mut f = font.clone();
    for (side, k) in [
        (Side::Width, &keys.width),
        (Side::Left, &keys.left),
        (Side::Right, &keys.right),
    ] {
        let Some(k) = k else { continue };
        let parsed = parse(k).map_err(|e| format!("{glyph}: {e}"))?;
        let Some(v) =
            value(&f, side, &parsed, &layer.master_id).map_err(|e| format!("{glyph}: {k}: {e}"))?
        else {
            continue;
        };
        let current = f
            .glyph(glyph)
            .and_then(|g| g.layer(key))
            .expect("layer exists");
        // Sidebearings can't be set on an empty layer; widths can.
        if side != Side::Width && sidebearings(&f, current).is_none() {
            continue;
        }
        let edit = match side {
            Side::Left => MetricEdit::Lsb(v),
            Side::Right => MetricEdit::Rsb(v),
            Side::Width => MetricEdit::Width(v),
        };
        let next = with_metric(&f, glyph, key, edit).map_err(|e| format!("{glyph}: {e}"))?;
        *f.glyph_mut(glyph)
            .and_then(|g| g.layer_mut(key))
            .expect("layer exists") = next;
    }
    let out = f
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .expect("layer exists")
        .clone();
    Ok((&out != layer).then_some(out))
}

/// Applies every metrics key in the font, glyphs that others depend on first.
/// Returns the updated font and the problems found (missing glyphs, loops, bad keys);
/// keys with problems are skipped.
pub fn apply_all(font: &Font) -> (Font, Vec<String>) {
    apply_some(font, None)
}

/// Like [`apply_all`] but only the glyphs in `only` are updated (`None` means all).
/// Referenced glyphs are still visited first so a key reads up-to-date values, but
/// their own keys are left alone when they are outside the selection.
pub fn apply_some(font: &Font, only: Option<&[String]>) -> (Font, Vec<String>) {
    let mut f = font.clone();
    let mut problems = Vec::new();
    // Glyphs with keys, ordered so that references come before the glyphs using them.
    // Locked glyphs keep their metrics as they are.
    let in_scope = |name: &str| only.is_none_or(|o| o.iter().any(|n| n == name));
    let with_keys: Vec<String> = font
        .glyphs
        .iter()
        .filter(|g| {
            !g.locked && in_scope(&g.name) && g.layers.iter().any(|l| !l.metrics_keys.is_empty())
        })
        .map(|g| g.name.clone())
        .collect();
    let mut order: Vec<String> = Vec::new();
    let mut state = std::collections::HashMap::<String, u8>::new(); // 1 visiting, 2 done
    fn visit(
        font: &Font,
        name: &str,
        state: &mut std::collections::HashMap<String, u8>,
        order: &mut Vec<String>,
        problems: &mut Vec<String>,
    ) {
        match state.get(name) {
            Some(2) => return,
            Some(1) => {
                problems.push(format!("{name}: its metrics keys go round in a loop"));
                return;
            }
            _ => {}
        }
        state.insert(name.to_string(), 1);
        if let Some(g) = font.glyph(name) {
            let mut refs: Vec<String> = g.layers.iter().flat_map(references).collect();
            refs.sort();
            refs.dedup();
            for r in refs {
                if r != name {
                    visit(font, &r, state, order, problems);
                }
            }
        }
        state.insert(name.to_string(), 2);
        order.push(name.to_string());
    }
    for n in &with_keys {
        visit(font, n, &mut state, &mut order, &mut problems);
    }
    for name in order.iter().filter(|n| with_keys.contains(n)) {
        let keys: Vec<String> = f
            .glyph(name)
            .map(|g| g.layers.iter().map(|l| l.key().to_string()).collect())
            .unwrap_or_default();
        for key in keys {
            match apply_layer(&f, name, &key) {
                Ok(Some(l)) => {
                    *f.glyph_mut(name)
                        .and_then(|g| g.layer_mut(&key))
                        .expect("layer") = l
                }
                Ok(None) => {}
                Err(e) => problems.push(e),
            }
        }
    }
    problems.dedup();
    (f, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    #[test]
    fn parses_keys() {
        assert_eq!(parse("=n").unwrap().source, Source::Same("n".into()));
        assert_eq!(parse("=|H").unwrap().source, Source::Opposite("H".into()));
        let k = parse("=o+10").unwrap();
        assert_eq!(
            (k.source, k.op),
            (Source::Same("o".into()), Some(('+', 10.0)))
        );
        assert_eq!(parse("= 40").unwrap().source, Source::Value(40.0));
        assert_eq!(parse("=a.sc*1.5").unwrap().op, Some(('*', 1.5)));
        assert!(parse("n").is_err());
        assert!(parse("=").is_err());
        assert!(parse("=|").is_err());
    }

    #[test]
    fn renames_the_glyph_a_key_follows() {
        let mut m = std::collections::BTreeMap::new();
        m.insert("H".to_string(), "Hh".to_string());
        assert_eq!(rename_in_key("=H", &m).as_deref(), Some("=Hh"));
        assert_eq!(rename_in_key("=|H+10", &m).as_deref(), Some("=|Hh+10"));
        assert_eq!(rename_in_key("=H*1.5", &m).as_deref(), Some("=Hh*1.5"));
        // Fixed values and unrelated glyphs are left alone.
        assert_eq!(rename_in_key("=40", &m), None);
        assert_eq!(rename_in_key("=o", &m), None);
    }

    fn sb(f: &Font, g: &str) -> (f64, f64) {
        sidebearings(f, f.glyph(g).unwrap().layer("m01").unwrap()).unwrap()
    }

    #[test]
    fn applies_keys_in_dependency_order() {
        let mut f = sample_font();
        let (h_l, h_r) = sb(&f, "H");
        let o_r = sb(&f, "o").1;
        // n follows H on the left and o (+5) on the right; l follows n on both sides.
        let set = |f: &mut Font, g: &str, l: Option<&str>, r: Option<&str>| {
            let k = &mut f
                .glyph_mut(g)
                .unwrap()
                .layer_mut("m01")
                .unwrap()
                .metrics_keys;
            k.left = l.map(String::from);
            k.right = r.map(String::from);
        };
        set(&mut f, "l", Some("=n"), Some("=n"));
        set(&mut f, "n", Some("=H"), Some("=o+5"));
        let (f2, problems) = apply_all(&f);
        assert!(problems.is_empty(), "{problems:?}");
        let (n_l, n_r) = sb(&f2, "n");
        assert!((n_l - h_l).abs() < 1e-9);
        assert!((n_r - (o_r + 5.0)).abs() < 1e-9);
        let (m_l, m_r) = sb(&f2, "l");
        assert!(
            (m_l - n_l).abs() < 1e-9 && (m_r - n_r).abs() < 1e-9,
            "l used n's new values"
        );
        // Opposite side, and fixed values.
        set(&mut f, "n", Some("=|H"), Some("=40"));
        set(&mut f, "l", None, None);
        let (f3, _) = apply_all(&f);
        let (n_l, n_r) = sb(&f3, "n");
        assert!((n_l - h_r).abs() < 1e-9 && (n_r - 40.0).abs() < 1e-9);
    }

    #[test]
    fn applies_only_the_selection() {
        let mut f = sample_font();
        f.glyph_mut("n").unwrap().layers[0].metrics_keys.left = Some("=H".into());
        f.glyph_mut("l").unwrap().layers[0].metrics_keys.left = Some("=n".into());
        let n_before = sb(&f, "n");
        let (f2, problems) = apply_some(&f, Some(&["l".into()]));
        assert!(problems.is_empty(), "{problems:?}");
        // n is outside the selection, so its own key is not applied.
        assert_eq!(sb(&f2, "n"), n_before);
        assert!((sb(&f2, "l").0 - n_before.0).abs() < 1e-9);
    }

    #[test]
    fn reports_missing_glyphs_and_loops() {
        let mut f = sample_font();
        f.glyph_mut("n").unwrap().layers[0].metrics_keys.left = Some("=nothere".into());
        f.glyph_mut("a").unwrap().layers[0].metrics_keys.left = Some("=b".into());
        f.glyph_mut("b").unwrap().layers[0].metrics_keys.left = Some("=a".into());
        let (_, problems) = apply_all(&f);
        assert!(
            problems.iter().any(|p| p.contains("nothere")),
            "{problems:?}"
        );
        assert!(problems.iter().any(|p| p.contains("loop")), "{problems:?}");
    }
}
