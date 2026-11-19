//! Importing font sources from other editors into Kernest.
//!
//! - Glyphs: `.glyphs` (Glyphs 2 and 3) and `.glyphspackage`, read with glyphs-reader.
//! - UFO: a single `.ufo` (one master) or a `.designspace` with its UFO sources, read with norad.
//!
//! Anything Kernest can't represent yet (brace and bracket layers, sparse sources,
//! non-kerning groups…) is left out and listed in [`Imported::warnings`].

mod compiled;
mod glyphs;
mod glyphs_write;
mod metrics;
mod svg;
mod ufo;
mod woff2;

pub use glyphs_write::{glyphs_source, write_glyphs};
pub use metrics::{apply_metrics, from_font, parse_afm, KeepSide, SourceMetrics};
pub use svg::{svg_shapes, Placement};
pub use woff2::decode as decode_woff2;

use std::collections::BTreeMap;
use std::path::Path;

use kernest_core::{Font, Glyph, KernPair, Layer};
use serde::Serialize;

/// The result of an import: the converted font plus what was left out or changed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub font: Font,
    pub warnings: Vec<String>,
}

/// File types that can be imported, for file dialogs.
pub const EXTENSIONS: [&str; 7] = [
    "glyphs",
    "glyphspackage",
    "ufo",
    "designspace",
    "otf",
    "ttf",
    "woff2",
];

/// Imports a font source (`.glyphs`, `.glyphspackage`, `.ufo`, `.designspace`) or a
/// compiled font (`.otf`, `.ttf`, `.woff2`).
pub fn import(path: &Path) -> Result<Imported, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if EXTENSIONS.contains(&ext.as_str()) && !path.exists() {
        return Err(format!("\"{}\" doesn't exist.", path.display()));
    }
    let mut imported = match ext.as_str() {
        "glyphs" | "glyphspackage" => glyphs::import(path)?,
        "ufo" => ufo::import_ufo(path)?,
        "designspace" => ufo::import_designspace(path)?,
        "otf" | "ttf" | "woff2" => compiled::import_compiled(path)?,
        _ => {
            let kinds = ".glyphs, .glyphspackage, .ufo, .designspace, .otf, .ttf and .woff2";
            return Err(format!(
                "Kernest can import {kinds} files, not \"{}\".",
                path.display()
            ));
        }
    };
    finish(&mut imported);
    imported
        .font
        .validate()
        .map_err(|e| format!("the imported font is not valid: {e}"))?;
    Ok(imported)
}

/// Master ids in Kernest are short and stable: m01, m02, …
fn master_id(i: usize) -> String {
    format!("m{:02}", i + 1)
}

/// Fixes up what every importer needs: a layer for every master (empty if the
/// source had none), components that point at real glyphs, and kerning that only
/// uses glyphs and groups that exist.
fn finish(imp: &mut Imported) {
    let font = &mut imp.font;
    let names: std::collections::HashSet<String> =
        font.glyphs.iter().map(|g| g.name.clone()).collect();
    let mut missing_layers = Vec::new();
    let mut missing_components = BTreeMap::<String, usize>::new();
    let default_id = font.masters[0].id.clone();
    let master_ids: Vec<String> = font.masters.iter().map(|m| m.id.clone()).collect();
    for g in &mut font.glyphs {
        let fallback = g
            .layer(&default_id)
            .or_else(|| g.layers.first())
            .map(|l| l.width)
            .unwrap_or(600.0);
        for id in &master_ids {
            if g.layer(id).is_none() {
                g.layers.push(Layer::new(id, fallback));
                missing_layers.push(g.name.clone());
            }
        }
        g.layers.retain(|l| master_ids.contains(&l.master_id));
        // Master layers first, in master order; brace and bracket layers after them.
        g.layers.sort_by_key(|l| {
            let master = l.is_master();
            (
                !master,
                master.then(|| master_ids.iter().position(|m| *m == l.master_id)),
            )
        });
        for l in &mut g.layers {
            l.shapes.retain(|s| match s {
                kernest_core::Shape::Component { base, .. } if !names.contains(base) => {
                    *missing_components.entry(base.clone()).or_default() += 1;
                    false
                }
                _ => true,
            });
        }
    }
    missing_layers.dedup();
    if !missing_layers.is_empty() {
        imp.warnings.push(format!(
            "{} glyph{} had no outline for some masters and got empty layers there: {}",
            missing_layers.len(),
            plural(missing_layers.len()),
            list(&missing_layers)
        ));
    }
    if !missing_components.is_empty() {
        let names: Vec<String> = missing_components.into_keys().collect();
        imp.warnings.push(format!(
            "Components referring to missing glyphs were removed: {}",
            list(&names)
        ));
    }

    // Kerning: drop pairs whose sides don't exist.
    let lefts: std::collections::HashSet<String> = font
        .glyphs
        .iter()
        .filter_map(|g| g.right_group.as_ref().map(|r| format!("@{r}")))
        .chain(names.iter().cloned())
        .collect();
    let rights: std::collections::HashSet<String> = font
        .glyphs
        .iter()
        .filter_map(|g| g.left_group.as_ref().map(|r| format!("@{r}")))
        .chain(names.iter().cloned())
        .collect();
    let mut dropped = 0;
    for pairs in font.kerning.values_mut() {
        let before = pairs.len();
        pairs.retain(|p| lefts.contains(&p.left) && rights.contains(&p.right) && p.value != 0.0);
        dropped += before - pairs.len();
    }
    font.kerning.retain(|_, v| !v.is_empty());
    if dropped > 0 {
        imp.warnings.push(format!(
            "{dropped} kerning pair{} referred to missing glyphs or groups (or were zero) and were left out.",
            plural(dropped)
        ));
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// A short, readable list: the first eight names and "…" if there are more.
fn list(names: &[String]) -> String {
    let mut s = names.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 8 {
        s.push_str(&format!(" and {} more", names.len() - 8));
    }
    s
}

/// Sets a glyph's kerning group for one side, warning when it already has a different one.
fn set_group(glyph: &mut Glyph, left_side_of_pair: bool, group: &str, warnings: &mut Vec<String>) {
    let slot = if left_side_of_pair {
        &mut glyph.right_group
    } else {
        &mut glyph.left_group
    };
    match slot {
        Some(existing) if existing != group => warnings.push(format!(
            "\"{}\" is in two kerning groups for the same side (\"{existing}\" and \"{group}\"); kept \"{existing}\".",
            glyph.name
        )),
        _ => *slot = Some(group.to_string()),
    }
}

/// Collects kerning pairs, sorted for stable files.
fn sorted_pairs(mut pairs: Vec<KernPair>) -> Vec<KernPair> {
    pairs.sort_by(|a, b| (&a.left, &a.right).cmp(&(&b.left, &b.right)));
    pairs
}
