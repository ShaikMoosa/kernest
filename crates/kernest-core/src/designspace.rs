//! Design-space maths: normalising locations and the OpenType variation model.
//!
//! `VariationModel` is a port of fontTools' `varLib.models.VariationModel`, the
//! algorithm variable fonts are built with. Using the same model for previews
//! means an interpolated instance in the editor matches the exported font.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Axis, Font};

/// A location keyed by axis tag. In normalised form only non-zero axes are present.
pub type Location = BTreeMap<String, f64>;

/// Per-axis region `(lower, peak, upper)` in normalised coordinates.
pub type Region = BTreeMap<String, (f64, f64, f64)>;

/// Piecewise-linear user → design through the axis map (identity without one).
/// Values outside the mapped range stay at the nearest end, as fontTools does.
pub fn user_to_design(axis: &Axis, v: f64) -> f64 {
    map_value(&axis.map, v)
}

/// The inverse of [`user_to_design`], for showing design locations in user terms.
pub fn design_to_user(axis: &Axis, v: f64) -> f64 {
    let inverted: Vec<(f64, f64)> = axis.map.iter().map(|&(u, d)| (d, u)).collect();
    map_value(&inverted, v)
}

fn map_value(pts: &[(f64, f64)], v: f64) -> f64 {
    if pts.is_empty() {
        return v;
    }
    if v <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        let ((a, oa), (b, ob)) = (w[0], w[1]);
        if v <= b {
            return if b == a {
                ob
            } else {
                oa + (v - a) / (b - a) * (ob - oa)
            };
        }
    }
    pts[pts.len() - 1].1
}

/// The axis extremes in design coordinates: the map applied to min/default/max.
/// Masters and instances live in design space, so this is what interpolation uses.
pub fn design_bounds(axis: &Axis) -> (f64, f64, f64) {
    (
        user_to_design(axis, axis.min),
        user_to_design(axis, axis.default),
        user_to_design(axis, axis.max),
    )
}

/// Maps a design-space location to normalised coordinates in [-1, 1]
/// (default → 0, minimum → -1, maximum → 1). Missing axes use their default.
pub fn normalize(font: &Font, loc: &Location) -> Location {
    let mut out = Location::new();
    for axis in &font.axes {
        let (min, default, max) = design_bounds(axis);
        let v = loc
            .get(&axis.tag)
            .copied()
            .unwrap_or(default)
            .clamp(min, max);
        let n = if v < default && default > min {
            (v - default) / (default - min)
        } else if v > default && max > default {
            (v - default) / (max - default)
        } else {
            0.0
        };
        if n != 0.0 {
            out.insert(axis.tag.clone(), n);
        }
    }
    out
}

/// How much a region contributes at a location (fontTools `supportScalar`).
pub fn support_scalar(loc: &Location, support: &Region) -> f64 {
    let mut scalar = 1.0;
    for (axis, &(lower, peak, upper)) in support {
        if peak == 0.0 || lower > peak || peak > upper || (lower < 0.0 && upper > 0.0) {
            continue;
        }
        let v = loc.get(axis).copied().unwrap_or(0.0);
        if v == peak {
            continue;
        }
        if v <= lower || upper <= v {
            return 0.0;
        }
        scalar *= if v < peak {
            (v - lower) / (peak - lower)
        } else {
            (v - upper) / (peak - upper)
        };
    }
    scalar
}

#[derive(Debug, Clone)]
pub struct VariationModel {
    /// Master locations, sorted.
    locations: Vec<Location>,
    /// sorted index → original master index
    reverse_mapping: Vec<usize>,
    supports: Vec<Region>,
    delta_weights: Vec<Vec<(usize, f64)>>,
}

fn sign(v: f64) -> i8 {
    if v < 0.0 {
        -1
    } else if v > 0.0 {
        1
    } else {
        0
    }
}

struct SortKey {
    rank: usize,
    neg_on_point: i64,
    order: Vec<usize>,
    names: Vec<String>,
    signs: Vec<i8>,
    abs: Vec<f64>,
}

impl SortKey {
    fn cmp(&self, o: &Self) -> Ordering {
        self.rank
            .cmp(&o.rank)
            .then(self.neg_on_point.cmp(&o.neg_on_point))
            .then(self.order.cmp(&o.order))
            .then(self.names.cmp(&o.names))
            .then(self.signs.cmp(&o.signs))
            .then_with(|| {
                for (a, b) in self.abs.iter().zip(&o.abs) {
                    match a.total_cmp(b) {
                        Ordering::Equal => continue,
                        other => return other,
                    }
                }
                self.abs.len().cmp(&o.abs.len())
            })
    }
}

impl VariationModel {
    /// Builds a model from normalised master locations (in master order).
    /// One master must sit at the origin (the default master); locations must be unique.
    pub fn new(locations: &[Location], axis_order: &[String]) -> Result<Self, String> {
        let clean: Vec<Location> = locations
            .iter()
            .map(|l| {
                l.iter()
                    .filter(|(_, v)| **v != 0.0)
                    .map(|(k, v)| (k.clone(), *v))
                    .collect()
            })
            .collect();
        if !clean.iter().any(BTreeMap::is_empty) {
            return Err("No master sits at the default location of every axis.".into());
        }
        for (i, a) in clean.iter().enumerate() {
            if clean[..i].contains(a) {
                return Err("Two masters are at the same location.".into());
            }
        }

        // Points on each axis (from masters that vary only that axis), for the sort order.
        let mut axis_points: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for loc in &clean {
            if loc.len() == 1 {
                let (axis, v) = loc.iter().next().unwrap();
                let pts = axis_points.entry(axis.clone()).or_insert_with(|| vec![0.0]);
                if !pts.contains(v) {
                    pts.push(*v);
                }
            }
        }
        let key = |loc: &Location| -> SortKey {
            let on_point = loc
                .iter()
                .filter(|(a, v)| axis_points.get(*a).is_some_and(|p| p.contains(v)))
                .count();
            let mut ordered: Vec<String> = axis_order
                .iter()
                .filter(|a| loc.contains_key(*a))
                .cloned()
                .collect();
            ordered.extend(loc.keys().filter(|a| !axis_order.contains(a)).cloned());
            SortKey {
                rank: loc.len(),
                neg_on_point: -(on_point as i64),
                order: ordered
                    .iter()
                    .map(|a| axis_order.iter().position(|x| x == a).unwrap_or(0x10000))
                    .collect(),
                names: ordered.clone(),
                signs: ordered.iter().map(|a| sign(loc[a])).collect(),
                abs: ordered.iter().map(|a| loc[a].abs()).collect(),
            }
        };
        let mut order: Vec<usize> = (0..clean.len()).collect();
        order.sort_by(|&a, &b| key(&clean[a]).cmp(&key(&clean[b])));
        let sorted: Vec<Location> = order.iter().map(|&i| clean[i].clone()).collect();

        // Regions from locations.
        let mut min_v: BTreeMap<String, f64> = BTreeMap::new();
        let mut max_v: BTreeMap<String, f64> = BTreeMap::new();
        for l in &sorted {
            for (k, &v) in l {
                let mn = min_v.entry(k.clone()).or_insert(v);
                *mn = mn.min(v);
                let mx = max_v.entry(k.clone()).or_insert(v);
                *mx = mx.max(v);
            }
        }
        let mut regions: Vec<Region> = sorted
            .iter()
            .map(|loc| {
                loc.iter()
                    .map(|(axis, &v)| {
                        let r = if v > 0.0 {
                            (0.0, v, max_v[axis])
                        } else {
                            (min_v[axis], v, 0.0)
                        };
                        (axis.clone(), r)
                    })
                    .collect()
            })
            .collect();

        // Master supports: shrink each region so earlier masters don't overlap it.
        for i in 0..regions.len() {
            let loc_axes: BTreeSet<String> = regions[i].keys().cloned().collect();
            for j in 0..i {
                let prev = regions[j].clone();
                if prev.keys().cloned().collect::<BTreeSet<String>>() != loc_axes {
                    continue;
                }
                let region = &regions[i];
                let relevant = region.iter().all(|(axis, &(lower, peak, upper))| {
                    let p = prev[axis].1;
                    p == peak || (lower < p && p < upper)
                });
                if !relevant {
                    continue;
                }
                let mut best_axes: BTreeMap<String, (f64, f64, f64)> = BTreeMap::new();
                let mut best_ratio = -1.0;
                for (axis, &(_, val, _)) in &prev {
                    let (lower, loc_v, upper) = region[axis];
                    let (mut new_lower, mut new_upper) = (lower, upper);
                    let ratio = if val < loc_v {
                        new_lower = val;
                        (val - loc_v) / (lower - loc_v)
                    } else if loc_v < val {
                        new_upper = val;
                        (val - loc_v) / (upper - loc_v)
                    } else {
                        continue;
                    };
                    if ratio > best_ratio {
                        best_axes.clear();
                        best_ratio = ratio;
                    }
                    if ratio == best_ratio {
                        best_axes.insert(axis.clone(), (new_lower, loc_v, new_upper));
                    }
                }
                for (axis, triple) in best_axes {
                    regions[i].insert(axis, triple);
                }
            }
        }

        let delta_weights = sorted
            .iter()
            .enumerate()
            .map(|(i, loc)| {
                regions[..i]
                    .iter()
                    .enumerate()
                    .filter_map(|(j, s)| {
                        let w = support_scalar(loc, s);
                        (w != 0.0).then_some((j, w))
                    })
                    .collect()
            })
            .collect();

        Ok(Self {
            locations: sorted,
            reverse_mapping: order,
            supports: regions,
            delta_weights,
        })
    }

    /// Number of masters in the model.
    pub fn len(&self) -> usize {
        self.locations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.locations.is_empty()
    }

    /// The region each delta applies to, in the order [`deltas`](Self::deltas) returns
    /// them. The first is the default master's (empty: it always applies fully).
    pub fn supports(&self) -> &[Region] {
        &self.supports
    }

    /// The deltas that reproduce `master_values` (one vector per master, in master
    /// order): one vector per region of [`supports`](Self::supports). The first is the
    /// default master's values themselves. This is what variable fonts store.
    pub fn deltas(&self, master_values: &[Vec<f64>]) -> Vec<Vec<f64>> {
        let mut deltas: Vec<Vec<f64>> = Vec::with_capacity(self.locations.len());
        for (i, weights) in self.delta_weights.iter().enumerate() {
            let mut d = master_values[self.reverse_mapping[i]].clone();
            for &(j, w) in weights {
                for (dv, prev) in d.iter_mut().zip(&deltas[j]) {
                    *dv -= prev * w;
                }
            }
            deltas.push(d);
        }
        deltas
    }

    /// Interpolates value vectors (one per master, in master order, all the same length)
    /// at a normalised location.
    pub fn interpolate(&self, master_values: &[Vec<f64>], loc: &Location) -> Vec<f64> {
        let n = master_values.first().map_or(0, Vec::len);
        let deltas = self.deltas(master_values);
        let mut out = vec![0.0; n];
        for (d, s) in deltas.iter().zip(&self.supports) {
            let scalar = support_scalar(loc, s);
            if scalar != 0.0 {
                for (o, v) in out.iter_mut().zip(d) {
                    *o += v * scalar;
                }
            }
        }
        out
    }
}

/// Cuts one variable font out of the family: the axis ranges the setting asks for and
/// the glyphs it keeps. fontc ignores designspace `<variable-fonts>`, so export
/// compiles each setting from this reduced source instead. All axes are kept (a
/// dropped axis could leave two masters at the same spot); a listed axis is narrowed
/// to the requested user range. Masters outside the range are replaced by interpolated
/// masters at the new default and axis extremes, so the reduced source spans the range
/// with a master at its default.
/// The font without the masters named in `names` ("Disable Masters"), and the axis
/// ranges (user coordinates) the remaining masters span. Names that match no master
/// are reported in the warnings; disabling every master is an error.
pub fn without_masters(
    font: &Font,
    names: &[String],
) -> Result<(Font, Vec<crate::model::VariableAxisSubset>, Vec<String>), String> {
    let mut warnings = Vec::new();
    let unknown: Vec<String> = names
        .iter()
        .filter(|n| !font.masters.iter().any(|m| &m.name == *n))
        .map(|n| format!("\"{n}\""))
        .collect();
    if !unknown.is_empty() {
        warnings.push(format!(
            "Disable Masters: there is no master named {}; it was ignored.",
            unknown.join(", ")
        ));
    }
    let ids: Vec<String> = font
        .masters
        .iter()
        .filter(|m| names.contains(&m.name))
        .map(|m| m.id.clone())
        .collect();
    if ids.is_empty() {
        return Ok((font.clone(), Vec::new(), warnings));
    }
    if ids.len() == font.masters.len() {
        return Err("Disable Masters leaves no master to export.".into());
    }
    let default = crate::interpolate::default_master(font);
    let mut f = font.clone();
    f.masters.retain(|m| !ids.contains(&m.id));
    if ids.contains(&default.id) {
        // A variable font needs a master at its default: the nearest one left takes
        // over (the first in master order on a tie).
        let at = |m: &crate::model::Master, a: &Axis| {
            m.location
                .get(&a.tag)
                .copied()
                .unwrap_or_else(|| design_bounds(a).1)
        };
        let distance = |m: &crate::model::Master| -> f64 {
            font.axes
                .iter()
                .map(|a| (at(m, a) - at(default, a)).abs())
                .sum()
        };
        let next = f
            .masters
            .iter()
            .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            .cloned()
            .expect("at least one master is left");
        for a in &mut f.axes {
            a.default = design_to_user(a, at(&next, a));
        }
        warnings.push(format!(
            "Disable Masters: the default master \"{}\" is left out, so \"{}\" is the default instead.",
            default.name, next.name
        ));
    }
    for g in &mut f.glyphs {
        g.layers.retain(|l| !ids.contains(&l.master_id));
    }
    for id in &ids {
        f.kerning.remove(id);
    }
    // Each axis now spans only the masters that are left.
    let ranges = f
        .axes
        .iter()
        .map(|a| {
            let (lo, hi) = f
                .masters
                .iter()
                .map(|m| {
                    m.location
                        .get(&a.tag)
                        .copied()
                        .unwrap_or_else(|| design_bounds(a).1)
                })
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(v), hi.max(v))
                });
            let (lo, hi) = (design_to_user(a, lo), design_to_user(a, hi));
            crate::model::VariableAxisSubset {
                tag: a.tag.clone(),
                min: Some(lo.min(hi).max(a.min)),
                max: Some(hi.max(lo).min(a.max)),
            }
        })
        .collect();
    Ok((f, ranges, warnings))
}

/// Whether `loc` (design coordinates) lies inside every axis of `font`.
pub fn inside_axes(font: &Font, loc: &Location) -> bool {
    font.axes.iter().all(|a| {
        let (mn, df, mx) = design_bounds(a);
        let v = loc.get(&a.tag).copied().unwrap_or(df);
        v >= mn - 1e-6 && v <= mx + 1e-6
    })
}

pub fn reduce_for_variable(
    font: &Font,
    setting: &crate::model::VariableFontSetting,
) -> Result<Font, String> {
    reduce_for_variable_with_warnings(font, setting).map(|(f, _)| f)
}

/// [`reduce_for_variable`], with the glyph processing warnings (SMO-28: patterns
/// that match no glyph).
/// "Variable Font Origin": moves each axis default to the named master's location,
/// so that master becomes the variable font's default. A master that isn't in this
/// variable font, or sits outside its axis ranges, leaves the defaults as they are
/// with a warning.
fn set_origin(font: &Font, axes: &mut [Axis], origin: &str, warnings: &mut Vec<String>) {
    let Some(m) = font
        .masters
        .iter()
        .find(|m| m.name == origin || m.id == origin)
    else {
        warnings.push(format!(
            "Variable Font Origin \"{name}\" isn't one of this variable font's masters; the default master stays.",
            name = origin
        ));
        return;
    };
    let mut defaults = Vec::with_capacity(axes.len());
    for a in axes.iter() {
        let design = m
            .location
            .get(&a.tag)
            .copied()
            .unwrap_or_else(|| design_bounds(a).1);
        let user = design_to_user(a, design);
        if user < a.min - 1e-6 || user > a.max + 1e-6 {
            warnings.push(format!(
                "Variable Font Origin \"{name}\" sits outside the \"{axis}\" range of this variable font; the default master stays.",
                name = origin,
                axis = a.name
            ));
            return;
        }
        defaults.push(user.clamp(a.min, a.max));
    }
    for (a, d) in axes.iter_mut().zip(defaults) {
        a.default = d;
    }
}

pub fn reduce_for_variable_with_warnings(
    font: &Font,
    setting: &crate::model::VariableFontSetting,
) -> Result<(Font, Vec<String>), String> {
    let (mut f, warnings) = crate::processing::prepare(
        font,
        &setting.rename_glyphs,
        &setting.remove_glyphs,
        &setting.processing,
    )
    .map_err(|e| format!("{}: {e}", setting.name))?;
    f.variable_fonts.clear();
    let mut narrowed = f.axes.clone();
    for sub in &setting.axes {
        let Some(a) = narrowed.iter_mut().find(|a| a.tag == sub.tag) else {
            return Err(format!(
                "Variable font \"{}\" subsets unknown axis \"{}\".",
                setting.name, sub.tag
            ));
        };
        let lo = sub.min.unwrap_or(a.min);
        let hi = sub.max.unwrap_or(a.max);
        if !(lo.is_finite() && hi.is_finite()) || lo > hi {
            return Err(format!(
                "Variable font \"{}\" needs minimum ≤ maximum on \"{}\".",
                setting.name, a.name
            ));
        }
        if lo < a.min || hi > a.max {
            return Err(format!(
                "Variable font \"{}\" range on \"{}\" must sit inside the axis range.",
                setting.name, a.name
            ));
        }
        let orig = a.clone();
        a.min = lo;
        a.max = hi;
        a.default = a.default.clamp(lo, hi);
        // Clip the avar map to the range, re-anchoring both ends through the old map.
        if !orig.map.is_empty() {
            let mut pts = vec![
                (lo, user_to_design(&orig, lo)),
                (hi, user_to_design(&orig, hi)),
            ];
            pts.extend(orig.map.iter().copied().filter(|&(u, _)| u > lo && u < hi));
            pts.sort_by(|x, y| x.0.total_cmp(&y.0));
            pts.dedup_by(|x, y| x.0 == y.0);
            a.map = pts;
        }
        // Keep only value labels that still sit inside the narrowed range.
        a.labels.retain(|l| l.value >= lo && l.value <= hi);
        for l in &mut a.labels {
            if let Some(v) = l.min {
                l.min = Some(v.max(lo));
            }
            if let Some(v) = l.max {
                l.max = Some(v.min(hi));
            }
            if l.linked_value.is_some_and(|v| v < lo || v > hi) {
                l.linked_value = None;
            }
        }
    }
    let mut warnings = warnings;
    if let Some(origin) = setting.origin.as_deref().filter(|o| !o.trim().is_empty()) {
        set_origin(&f, &mut narrowed, origin, &mut warnings);
    }
    if !setting.axes.is_empty() {
        let bounds: Vec<(String, (f64, f64, f64))> = narrowed
            .iter()
            .map(|a| (a.tag.clone(), design_bounds(a)))
            .collect();
        let at = |loc: &Location, tag: &str, dflt: f64| loc.get(tag).copied().unwrap_or(dflt);
        let inside = |loc: &Location| {
            bounds.iter().all(|(tag, (mn, df, mx))| {
                let v = at(loc, tag, *df);
                v >= *mn - 1e-6 && v <= *mx + 1e-6
            })
        };
        let same_loc = |a: &Location, b: &Location| {
            bounds
                .iter()
                .all(|(tag, (_, df, _))| (at(a, tag, *df) - at(b, tag, *df)).abs() < 1e-6)
        };
        let default_loc: Location = narrowed
            .iter()
            .map(|a| (a.tag.clone(), design_bounds(a).1))
            .collect();
        // The reduced source needs a master at the default and at each axis extreme.
        let mut required: Vec<Location> = vec![default_loc.clone()];
        for a in &narrowed {
            let (mn, _, mx) = design_bounds(a);
            let mut lo = default_loc.clone();
            lo.insert(a.tag.clone(), mn);
            required.push(lo);
            let mut hi = default_loc.clone();
            hi.insert(a.tag.clone(), mx);
            required.push(hi);
        }
        // Interpolate the missing ones against the original full-range axes, so the
        // interpolation still spans every original master.
        for loc in &required {
            if !f.masters.iter().any(|m| same_loc(&m.location, loc)) {
                f = crate::interpolate::add_master_at(&f, loc, "Default")?;
            }
        }
        // Masters outside the range are now represented by the boundary masters.
        let drop: Vec<String> = f
            .masters
            .iter()
            .filter(|m| !inside(&m.location))
            .map(|m| m.id.clone())
            .collect();
        for id in &drop {
            f.masters.retain(|m| &m.id != id);
            for g in &mut f.glyphs {
                g.layers.retain(|l| &l.master_id != id);
            }
            f.kerning.remove(id);
        }
    }
    f.axes = narrowed;
    // Glyph subset: the listed glyphs plus .notdef and every component target.
    if !setting.glyphs.is_empty() {
        // Names must exist; patterns with * or ? may match any number of glyphs.
        for name in setting.glyphs.iter().filter(|n| !n.contains(['*', '?'])) {
            if f.glyph(name).is_none() {
                return Err(format!(
                    "Variable font \"{}\" keeps unknown glyph \"{}\".",
                    setting.name, name
                ));
            }
        }
        let mut keep: BTreeSet<String> = crate::processing::expand(&f, &setting.glyphs)
            .0
            .into_iter()
            .collect();
        keep.insert(".notdef".into());
        loop {
            let mut added = false;
            for g in &f.glyphs {
                if !keep.contains(&g.name) {
                    continue;
                }
                for l in &g.layers {
                    for s in &l.shapes {
                        if let crate::model::Shape::Component { base, .. } = s {
                            if keep.insert(base.clone()) {
                                added = true;
                            }
                        }
                    }
                }
            }
            if !added {
                break;
            }
        }
        let gone: std::collections::HashSet<String> = f
            .glyphs
            .iter()
            .filter(|g| !keep.contains(&g.name))
            .map(|g| g.name.clone())
            .collect();
        f.glyphs.retain(|g| keep.contains(&g.name));
        f.features = crate::featureprune::prune(&f.features, &gone);
    }
    Ok((f, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(pairs: &[(&str, f64)]) -> Location {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }
    fn axes() -> Vec<String> {
        vec!["wght".into(), "wdth".into()]
    }
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn one_axis_two_masters() {
        let m = VariationModel::new(&[loc(&[]), loc(&[("wght", 1.0)])], &axes()).unwrap();
        let v = m.interpolate(&[vec![100.0], vec![200.0]], &loc(&[("wght", 0.25)]));
        assert!(close(v[0], 125.0), "{v:?}");
    }

    #[test]
    fn one_axis_three_masters_any_order() {
        // Bold, Light, Regular given out of order.
        let m = VariationModel::new(
            &[loc(&[("wght", 1.0)]), loc(&[("wght", -1.0)]), loc(&[])],
            &axes(),
        )
        .unwrap();
        let vals = [vec![300.0], vec![50.0], vec![100.0]];
        assert!(close(
            m.interpolate(&vals, &loc(&[("wght", -0.5)]))[0],
            75.0
        ));
        assert!(close(
            m.interpolate(&vals, &loc(&[("wght", 0.5)]))[0],
            200.0
        ));
        assert!(close(m.interpolate(&vals, &loc(&[]))[0], 100.0));
        assert!(close(
            m.interpolate(&vals, &loc(&[("wght", 1.0)]))[0],
            300.0
        ));
    }

    #[test]
    fn two_axes_with_and_without_corner() {
        let base = [loc(&[]), loc(&[("wght", 1.0)]), loc(&[("wdth", 1.0)])];
        let m = VariationModel::new(&base, &axes()).unwrap();
        let mid = loc(&[("wght", 0.5), ("wdth", 0.5)]);
        assert!(close(
            m.interpolate(&[vec![0.0], vec![10.0], vec![20.0]], &mid)[0],
            15.0
        ));

        let mut with_corner = base.to_vec();
        with_corner.push(loc(&[("wght", 1.0), ("wdth", 1.0)]));
        let m = VariationModel::new(&with_corner, &axes()).unwrap();
        let vals = [vec![0.0], vec![10.0], vec![20.0], vec![40.0]];
        // Bilinear: 0.25 * (0 + 10 + 20 + 40)
        assert!(close(m.interpolate(&vals, &mid)[0], 17.5));
        assert!(close(
            m.interpolate(&vals, &loc(&[("wght", 1.0), ("wdth", 1.0)]))[0],
            40.0
        ));
    }

    #[test]
    fn intermediate_master_on_one_axis() {
        // Masters at 0, 0.5 and 1: piecewise linear.
        let m = VariationModel::new(
            &[loc(&[]), loc(&[("wght", 1.0)]), loc(&[("wght", 0.5)])],
            &axes(),
        )
        .unwrap();
        let vals = [vec![0.0], vec![100.0], vec![80.0]];
        assert!(close(
            m.interpolate(&vals, &loc(&[("wght", 0.25)]))[0],
            40.0
        ));
        assert!(close(
            m.interpolate(&vals, &loc(&[("wght", 0.75)]))[0],
            90.0
        ));
    }

    #[test]
    fn errors() {
        assert!(VariationModel::new(&[loc(&[("wght", 1.0)])], &axes()).is_err());
        assert!(VariationModel::new(&[loc(&[]), loc(&[])], &axes()).is_err());
    }

    #[test]
    fn normalization() {
        let mut f = crate::sample::sample_font();
        f.axes.push(crate::model::Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 100.0,
            default: 400.0,
            max: 900.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        });
        assert_eq!(normalize(&f, &loc(&[("wght", 400.0)])), loc(&[]));
        assert_eq!(
            normalize(&f, &loc(&[("wght", 900.0)])),
            loc(&[("wght", 1.0)])
        );
        assert_eq!(
            normalize(&f, &loc(&[("wght", 250.0)])),
            loc(&[("wght", -0.5)])
        );
        assert_eq!(
            normalize(&f, &loc(&[("wght", 2000.0)])),
            loc(&[("wght", 1.0)])
        );
        assert_eq!(normalize(&f, &loc(&[])), loc(&[]));
    }

    fn mapped_axis() -> crate::model::Axis {
        crate::model::Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 100.0,
            default: 400.0,
            max: 900.0,
            hidden: false,
            // User 100/400/900 sit at design 100/300/900: the lower half is compressed.
            map: vec![(100.0, 100.0), (400.0, 300.0), (900.0, 900.0)],
            labels: Vec::new(),
        }
    }

    #[test]
    fn axis_map_user_design_round_trip() {
        let a = mapped_axis();
        for u in [100.0, 250.0, 400.0, 650.0, 900.0] {
            let d = user_to_design(&a, u);
            assert!(close(design_to_user(&a, d), u), "user {u} -> design {d}");
        }
        // The bend: user 250 is halfway to the default, design 200 halfway to 300.
        assert!(close(user_to_design(&a, 250.0), 200.0));
        assert!(close(user_to_design(&a, 400.0), 300.0));
        // Outside the mapped range the value stays at the nearest end.
        assert!(close(user_to_design(&a, 0.0), 100.0));
        assert!(close(user_to_design(&a, 1000.0), 900.0));
    }

    #[test]
    fn normalization_uses_design_bounds() {
        let mut f = crate::sample::sample_font();
        f.axes.push(mapped_axis());
        // Masters live in design space: the default is design 300, not user 400.
        assert_eq!(normalize(&f, &loc(&[("wght", 300.0)])), loc(&[]));
        assert_eq!(
            normalize(&f, &loc(&[("wght", 900.0)])),
            loc(&[("wght", 1.0)])
        );
        assert_eq!(
            normalize(&f, &loc(&[("wght", 200.0)])),
            loc(&[("wght", -0.5)])
        );
        // A user-space value (400) is not the default once a map is present.
        assert_ne!(normalize(&f, &loc(&[("wght", 400.0)])), loc(&[]));
    }

    fn two_master_font() -> Font {
        let a = crate::model::Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 100.0,
            default: 400.0,
            max: 900.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        };
        let f = crate::structure::set_axes(&crate::sample::sample_font(), vec![a]).unwrap();
        let (mut f, bold) =
            crate::structure::add_master(&f, "m01", "Bold", &loc(&[("wght", 900.0)])).unwrap();
        f.glyph_mut("H").unwrap().layer_mut(&bold).unwrap().width += 100.0;
        f
    }

    #[test]
    fn reduce_variable_font_range_and_glyphs() {
        use crate::model::{VariableAxisSubset, VariableFontSetting};
        let f = two_master_font();
        let setting =
            |min: Option<f64>, max: Option<f64>, glyphs: Vec<String>| VariableFontSetting {
                origin: None,
                export_stat: true,
                processing: Default::default(),
                feature_parameters: Vec::new(),
                family_name: None,
                name_records: Vec::new(),
                name: "Cut".into(),
                file_name: "Cut".into(),
                axes: vec![VariableAxisSubset {
                    tag: "wght".into(),
                    min,
                    max,
                }],
                glyphs,
                rename_glyphs: Vec::new(),
                remove_glyphs: Vec::new(),
                disabled_masters: Vec::new(),
            };
        // A range that keeps the default needs no new master.
        let r = reduce_for_variable(&f, &setting(Some(400.0), Some(700.0), vec![])).unwrap();
        assert_eq!(
            (r.axes[0].min, r.axes[0].default, r.axes[0].max),
            (400.0, 400.0, 700.0)
        );
        assert_eq!(r.masters.len(), 2);
        assert!(r.variable_fonts.is_empty());

        // A range that excludes the default clamps it and interpolates a master there.
        let regular_h = f.glyph("H").unwrap().layer("m01").unwrap().width;
        let r = reduce_for_variable(&f, &setting(Some(500.0), Some(900.0), vec![])).unwrap();
        assert_eq!(
            (r.axes[0].min, r.axes[0].default, r.axes[0].max),
            (500.0, 500.0, 900.0)
        );
        assert_eq!(r.masters.len(), 2);
        let def = crate::interpolate::default_master(&r);
        assert!(normalize(&r, &def.location).is_empty());
        let h = r.glyph("H").unwrap().layer(&def.id).unwrap().width;
        assert!(close(h, regular_h + 20.0), "H at the new default: {h}");

        // A glyph subset keeps the listed glyphs and drops the rest.
        let r = reduce_for_variable(&f, &setting(None, None, vec!["H".into()])).unwrap();
        assert!(r.glyph("H").is_some());
        assert!(r.glyph("o").is_none());
        assert!(r.glyph("I").is_none());

        assert!(reduce_for_variable(&f, &setting(Some(50.0), None, vec![]))
            .unwrap_err()
            .contains("inside the axis range"));
        let mut bad = setting(None, None, vec!["nope".into()]);
        bad.axes.clear();
        assert!(reduce_for_variable(&f, &bad)
            .unwrap_err()
            .contains("unknown glyph"));
        let mut bad = setting(None, None, vec![]);
        bad.axes[0].tag = "wdth".into();
        assert!(reduce_for_variable(&f, &bad)
            .unwrap_err()
            .contains("unknown axis"));
    }
}
