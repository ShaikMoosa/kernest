//! Design-space maths: normalising locations and the OpenType variation model.
//!
//! `VariationModel` is a port of fontTools' `varLib.models.VariationModel`, the
//! algorithm variable fonts are built with. Using the same model for previews
//! means an interpolated instance in the editor matches the exported font.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::model::Font;

/// A location keyed by axis tag. In normalised form only non-zero axes are present.
pub type Location = BTreeMap<String, f64>;

/// Per-axis region `(lower, peak, upper)` in normalised coordinates.
pub type Region = BTreeMap<String, (f64, f64, f64)>;

/// Maps a design-space location to normalised coordinates in [-1, 1]
/// (default → 0, minimum → -1, maximum → 1). Missing axes use their default.
pub fn normalize(font: &Font, loc: &Location) -> Location {
    let mut out = Location::new();
    for axis in &font.axes {
        let v = loc
            .get(&axis.tag)
            .copied()
            .unwrap_or(axis.default)
            .clamp(axis.min, axis.max);
        let n = if v < axis.default && axis.default > axis.min {
            (v - axis.default) / (axis.default - axis.min)
        } else if v > axis.default && axis.max > axis.default {
            (v - axis.default) / (axis.max - axis.default)
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

    /// Interpolates value vectors (one per master, in master order, all the same length)
    /// at a normalised location.
    pub fn interpolate(&self, master_values: &[Vec<f64>], loc: &Location) -> Vec<f64> {
        let n = master_values.first().map_or(0, Vec::len);
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
}
