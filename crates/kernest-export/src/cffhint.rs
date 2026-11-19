//! PostScript hinting for CFF: the Private DICT's zone and stem entries, and the
//! stem hints placed at the start of each charstring. The values travel in the UFO
//! (`font_info` for the DICT, the `com.kernest.hints` lib key for per-glyph stems),
//! so the CFF builder needs nothing beyond the source it already draws from.

use std::collections::HashMap;

use crate::cff::{int, op};

/// A stem hint as stored in the UFO lib. `width == 0` is a ghost hint.
pub(crate) struct StemHint {
    pub horizontal: bool,
    pub position: f64,
    pub width: f64,
    pub ghost_top: bool,
}

/// Reads per-glyph stem hints from the UFO lib.
pub(crate) fn glyph_hints(ufo: &norad::Font) -> HashMap<String, Vec<StemHint>> {
    let mut out = HashMap::new();
    let Some(dict) = ufo
        .lib
        .get("com.kernest.hints")
        .and_then(|v| v.as_dictionary())
    else {
        return out;
    };
    let num = |d: &plist::Dictionary, k: &str| -> Option<f64> {
        d.get(k).and_then(|v| v.as_real()).or_else(|| {
            d.get(k)
                .and_then(|v| v.as_signed_integer())
                .map(|i| i as f64)
        })
    };
    for (name, value) in dict {
        let Some(arr) = value.as_array() else {
            continue;
        };
        let hints: Vec<StemHint> = arr
            .iter()
            .filter_map(|h| {
                let d = h.as_dictionary()?;
                Some(StemHint {
                    horizontal: d
                        .get("horizontal")
                        .and_then(|v| v.as_boolean())
                        .unwrap_or(false),
                    position: num(d, "position")?,
                    width: num(d, "width").unwrap_or(0.0),
                    ghost_top: d
                        .get("ghostTop")
                        .and_then(|v| v.as_boolean())
                        .unwrap_or(false),
                })
            })
            .collect();
        if !hints.is_empty() {
            out.insert(name.clone(), hints);
        }
    }
    out
}

/// Encodes stem hints as `hstem`/`vstem` operators (relative edge deltas). Ghost
/// hints use the special widths -20 (top/right) and -21 (bottom/left). Horizontal
/// stems come first, as the charstring convention expects.
pub(crate) fn encode_hints(hints: &[StemHint]) -> Vec<u8> {
    let mut out = Vec::new();
    for (horizontal, code) in [(true, 1u8), (false, 3u8)] {
        let mut axis: Vec<&StemHint> = hints
            .iter()
            .filter(|h| h.horizontal == horizontal)
            .collect();
        if axis.is_empty() {
            continue;
        }
        axis.sort_by(|a, b| {
            a.position
                .partial_cmp(&b.position)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut args: Vec<i32> = Vec::new();
        let mut prev_end = 0i32;
        for h in axis {
            let pos = h.position.round() as i32;
            let w = if h.width == 0.0 {
                if h.ghost_top {
                    -20
                } else {
                    -21
                }
            } else {
                h.width.round() as i32
            };
            args.push(pos - prev_end);
            args.push(w);
            prev_end = pos + w.max(0);
        }
        // The Type 2 stack holds 48 operands; long lists span several operators.
        for chunk in args.chunks(48) {
            for a in chunk {
                int(*a, &mut out);
            }
            op(code, &mut out);
        }
    }
    out
}

/// A DICT real number in binary coded decimal (opcode 30): digits, `.` and `-`
/// as nibbles, terminated by 0xf.
fn real(x: f64, out: &mut Vec<u8>) {
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    let mut nibbles: Vec<u8> = s
        .chars()
        .map(|c| match c {
            '0'..='9' => c as u8 - b'0',
            '.' => 0xa,
            '-' => 0xe,
            _ => unreachable!("a formatted number"),
        })
        .collect();
    nibbles.push(0xf);
    if nibbles.len() % 2 == 1 {
        nibbles.push(0xf);
    }
    out.push(30);
    for pair in nibbles.chunks(2) {
        out.push((pair[0] << 4) | pair[1]);
    }
}

/// Appends the zone and stem entries (and the blue controls) to a Private DICT.
/// Blues and stem snaps are delta-encoded; BlueScale is a real, the other blue
/// controls integers.
pub(crate) fn private_hinting(info: &norad::FontInfo, out: &mut Vec<u8>) {
    let deltas = |vals: &[f64], out: &mut Vec<u8>| {
        let mut prev = 0i32;
        for v in vals {
            let v = v.round() as i32;
            int(v - prev, out);
            prev = v;
        }
    };
    if let Some(b) = &info.postscript_blue_values {
        if !b.is_empty() {
            deltas(b, out);
            op(6, out); // BlueValues
        }
    }
    if let Some(b) = &info.postscript_other_blues {
        if !b.is_empty() {
            deltas(b, out);
            op(7, out); // OtherBlues
        }
    }
    let h = info.postscript_stem_snap_h.clone().unwrap_or_default();
    let v = info.postscript_stem_snap_v.clone().unwrap_or_default();
    if let Some(&first) = h.first() {
        int(first.round() as i32, out);
        op(10, out); // StdHW
    }
    if let Some(&first) = v.first() {
        int(first.round() as i32, out);
        op(11, out); // StdVW
    }
    if !h.is_empty() {
        deltas(&h, out);
        op(12, out);
        op(12, out); // StemSnapH
    }
    if !v.is_empty() {
        deltas(&v, out);
        op(12, out);
        op(13, out); // StemSnapV
    }
    if let Some(x) = info.postscript_blue_scale {
        real(x, out);
        op(12, out);
        op(9, out); // BlueScale
    }
    if let Some(x) = info.postscript_blue_shift {
        int(x.round() as i32, out);
        op(12, out);
        op(10, out); // BlueShift
    }
    if let Some(x) = info.postscript_blue_fuzz {
        int(x.round() as i32, out);
        op(12, out);
        op(11, out); // BlueFuzz
    }
}

/// The same entries straight from the font model, for CFF2 (which is built without
/// a UFO). The default master supplies the zones and stems.
pub(crate) fn font_private_hinting(font: &kernest_core::Font, out: &mut Vec<u8>) {
    let master = kernest_core::interpolate::default_master(font);
    let (blue, other) = kernest_core::hinting::blues_from_zones(&master.alignment_zones);
    let ps = font.info.ps_hinting.as_ref();
    let info = norad::FontInfo {
        postscript_blue_values: Some(blue),
        postscript_other_blues: Some(other),
        postscript_stem_snap_h: Some(kernest_core::hinting::stem_snaps(&master.stems, true)),
        postscript_stem_snap_v: Some(kernest_core::hinting::stem_snaps(&master.stems, false)),
        postscript_blue_scale: ps.map(|p| p.blue_scale),
        postscript_blue_shift: ps.map(|p| p.blue_shift),
        postscript_blue_fuzz: ps.map(|p| p.blue_fuzz),
        ..Default::default()
    };
    private_hinting(&info, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stem(horizontal: bool, position: f64, width: f64) -> StemHint {
        StemHint {
            horizontal,
            position,
            width,
            ghost_top: false,
        }
    }

    #[test]
    fn stem_hints_encode_as_relative_edges() {
        // An H's crossbar and bars: hstem 330 50, vstem 80 100 240 100.
        let bytes = encode_hints(&[
            stem(true, 330.0, 50.0),
            stem(false, 80.0, 100.0),
            stem(false, 420.0, 100.0),
        ]);
        assert_eq!(bytes, vec![247, 222, 189, 1, 219, 239, 247, 132, 239, 3]);
    }

    #[test]
    fn ghost_hints_use_special_widths() {
        let top = StemHint {
            horizontal: true,
            position: 700.0,
            width: 0.0,
            ghost_top: true,
        };
        let bottom = stem(true, 0.0, 0.0);
        // Sorted by position, one hstem: 0 -21 700 -20.
        assert_eq!(
            encode_hints(&[top, bottom]),
            vec![139, 118, 249, 80, 119, 1]
        );
    }

    #[test]
    fn long_stem_lists_span_operators() {
        // 25 stems need 50 operands, over the 48-slot stack: two hstem operators.
        let hints: Vec<StemHint> = (0..25)
            .map(|i| stem(true, f64::from(i) * 200.0, 100.0))
            .collect();
        let bytes = encode_hints(&hints);
        assert_eq!(bytes.iter().filter(|b| **b == 1).count(), 2);
    }

    #[test]
    fn private_dict_entries_round_trip() {
        use write_fonts::read::ps::cff::dict::{self, Entry};
        use write_fonts::types::Fixed;
        let info = norad::FontInfo {
            postscript_blue_values: Some(vec![-10.0, 0.0, 700.0, 710.0]),
            postscript_other_blues: Some(vec![-210.0, -200.0]),
            postscript_stem_snap_h: Some(vec![50.0, 100.0]),
            postscript_stem_snap_v: Some(vec![100.0]),
            postscript_blue_scale: Some(0.05),
            postscript_blue_shift: Some(7.0),
            postscript_blue_fuzz: Some(2.0),
            ..Default::default()
        };
        let mut out = Vec::new();
        private_hinting(&info, &mut out);
        let entries: Vec<Entry> = dict::entries(&out, None).filter_map(|e| e.ok()).collect();
        let find = |pick: fn(&Entry) -> bool| entries.iter().find(|e| pick(e)).unwrap();
        let Entry::BlueValues(b) = find(|e| matches!(e, Entry::BlueValues(_))) else {
            unreachable!()
        };
        assert_eq!(
            b.values(),
            [
                (Fixed::from_i32(-10), Fixed::ZERO),
                (Fixed::from_i32(700), Fixed::from_i32(710))
            ]
        );
        let Entry::OtherBlues(b) = find(|e| matches!(e, Entry::OtherBlues(_))) else {
            unreachable!()
        };
        assert_eq!(b.values(), [(Fixed::from_i32(-210), Fixed::from_i32(-200))]);
        assert!(entries.contains(&Entry::StdHw(Fixed::from_i32(50))));
        assert!(entries.contains(&Entry::StdVw(Fixed::from_i32(100))));
        let Entry::StemSnapH(s) = find(|e| matches!(e, Entry::StemSnapH(_))) else {
            unreachable!()
        };
        assert_eq!(s.values(), [Fixed::from_i32(50), Fixed::from_i32(100)]);
        let Entry::StemSnapV(s) = find(|e| matches!(e, Entry::StemSnapV(_))) else {
            unreachable!()
        };
        assert_eq!(s.values(), [Fixed::from_i32(100)]);
        // Readers hold blue values at 1/65536 precision.
        let Entry::BlueScale(x) = find(|e| matches!(e, Entry::BlueScale(_))) else {
            unreachable!()
        };
        assert!((x.to_f64() - 0.05).abs() < 1.0 / 65536.0);
        assert!(entries.contains(&Entry::BlueShift(Fixed::from_i32(7))));
        assert!(entries.contains(&Entry::BlueFuzz(Fixed::from_i32(2))));
    }
}
