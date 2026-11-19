//! Compact Type 2 charstrings for CFF and CFF2.
//!
//! Outlines arrive as plain operations (`rmoveto`, `rlineto`, `rrcurveto`) with
//! operands that may vary (CFF2: a default value plus one delta per region). This
//! module makes them small:
//!
//! - **Specialised operators:** horizontal and vertical lines and curves use
//!   `hlineto`/`vlineto`, `hvcurveto`/`vhcurveto`, `hhcurveto`/`vvcurveto`, and
//!   runs of the same operator share one operator byte.
//! - **Subroutines:** a contour that appears in several glyphs (common once
//!   components are decomposed, as in accented letters) is stored once in the local
//!   subroutines and called.
//!
//! The result draws exactly the same outlines; only the encoding changes.

use std::collections::HashMap;

use crate::cff::int;

/// One operand: its default value, followed by one delta per region (CFF2 only).
pub(crate) type Operand = Vec<i32>;

/// Plain operations: (operator, operands).
pub(crate) type Ops = Vec<(u8, Vec<Operand>)>;

const RMOVETO: u8 = 21;
const HMOVETO: u8 = 22;
const VMOVETO: u8 = 4;
const RLINETO: u8 = 5;
const HLINETO: u8 = 6;
const VLINETO: u8 = 7;
const RRCURVETO: u8 = 8;
const HHCURVETO: u8 = 27;
const HVCURVETO: u8 = 31;
const VHCURVETO: u8 = 30;
const VVCURVETO: u8 = 26;
const CALLSUBR: u8 = 10;
const RETURN: u8 = 11;
const ENDCHAR: u8 = 14;
const BLEND: u8 = 16;

fn zero(o: &Operand) -> bool {
    o.iter().all(|v| *v == 0)
}

fn varies(o: &Operand) -> bool {
    o.iter().skip(1).any(|v| *v != 0)
}

/// Which format the charstrings are for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Cff,
    Cff2,
}

impl Format {
    fn max_stack(self) -> usize {
        match self {
            Format::Cff => 48,
            Format::Cff2 => 513,
        }
    }
}

/// A glyph to encode: its plain operations (in drawing order) and, for CFF, the
/// width operand to put first (`None` when it equals the default width). `hints`
/// holds already-encoded stem operators, placed after the width and before the
/// first moveto (CFF only; CFF2 charstrings carry no stem hints).
pub(crate) struct GlyphOps {
    pub ops: Ops,
    pub width: Option<i32>,
    pub hints: Vec<u8>,
}

/// Stack slots an operand list needs when written (blended operands carry deltas).
fn stack_cost(args: &[Operand], regions: usize) -> usize {
    args.iter()
        .map(|a| if varies(a) { 1 + regions } else { 1 })
        .sum::<usize>()
        + 1
}

/// Writes operands, grouping neighbouring varying ones into one `blend`.
fn write_args(args: &[Operand], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < args.len() {
        if !varies(&args[i]) {
            int(args[i][0], out);
            i += 1;
            continue;
        }
        let j = (i..args.len())
            .find(|&j| !varies(&args[j]))
            .unwrap_or(args.len());
        let run = &args[i..j];
        for a in run {
            int(a[0], out);
        }
        for a in run {
            for d in &a[1..] {
                int(*d, out);
            }
        }
        int(run.len() as i32, out);
        out.push(BLEND);
        i = j;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    H,
    V,
    R,
    Hv,
    Vh,
    Hh,
    Vv,
    Rr,
}

/// A segment in its smallest form: kind and the operands that form takes.
fn classify(op: u8, a: &[Operand]) -> Vec<(Kind, Vec<Operand>)> {
    match op {
        RLINETO => {
            let mut v = Vec::new();
            if zero(&a[1]) {
                v.push((Kind::H, vec![a[0].clone()]));
            }
            if zero(&a[0]) {
                v.push((Kind::V, vec![a[1].clone()]));
            }
            v.push((Kind::R, a.to_vec()));
            v
        }
        _ => {
            // dx1 dy1 dx2 dy2 dx3 dy3
            let mut v = Vec::new();
            if zero(&a[1]) && zero(&a[4]) {
                v.push((
                    Kind::Hv,
                    vec![a[0].clone(), a[2].clone(), a[3].clone(), a[5].clone()],
                ));
            }
            if zero(&a[0]) && zero(&a[5]) {
                v.push((
                    Kind::Vh,
                    vec![a[1].clone(), a[2].clone(), a[3].clone(), a[4].clone()],
                ));
            }
            if zero(&a[1]) && zero(&a[5]) {
                v.push((
                    Kind::Hh,
                    vec![a[0].clone(), a[2].clone(), a[3].clone(), a[4].clone()],
                ));
            }
            if zero(&a[0]) && zero(&a[4]) {
                v.push((
                    Kind::Vv,
                    vec![a[1].clone(), a[2].clone(), a[3].clone(), a[5].clone()],
                ));
            }
            v.push((Kind::Rr, a.to_vec()));
            v
        }
    }
}

/// Whether a run of `run` can continue with a segment of `next`. Alternating runs
/// (h/v lines, hv/vh curves) need the orientation to flip each time.
fn continues(run_start: Kind, len: usize, next: Kind) -> bool {
    let expect_alt = |a: Kind, b: Kind| if len.is_multiple_of(2) { a } else { b };
    match run_start {
        Kind::H => next == expect_alt(Kind::H, Kind::V),
        Kind::V => next == expect_alt(Kind::V, Kind::H),
        Kind::Hv => next == expect_alt(Kind::Hv, Kind::Vh),
        Kind::Vh => next == expect_alt(Kind::Vh, Kind::Hv),
        k => next == k,
    }
}

fn opcode(start: Kind) -> u8 {
    match start {
        Kind::H => HLINETO,
        Kind::V => VLINETO,
        Kind::R => RLINETO,
        Kind::Hv => HVCURVETO,
        Kind::Vh => VHCURVETO,
        Kind::Hh => HHCURVETO,
        Kind::Vv => VVCURVETO,
        Kind::Rr => RRCURVETO,
    }
}

/// Encodes a contour's segments (everything after its moveto), specialised and
/// merged, within the stack limit.
fn encode_body(segs: &[(u8, Vec<Operand>)], format: Format, regions: usize, out: &mut Vec<u8>) {
    let limit = format.max_stack();
    // Current run: (start kind, number of segments, operands).
    let mut run: Option<(Kind, usize, Vec<Operand>)> = None;
    let flush = |run: &mut Option<(Kind, usize, Vec<Operand>)>, out: &mut Vec<u8>| {
        if let Some((k, _, args)) = run.take() {
            write_args(&args, out);
            out.push(opcode(k));
        }
    };
    for (op, a) in segs {
        let forms = classify(*op, a);
        // Prefer extending the current run; otherwise start with the most compact form.
        let extend = run.as_ref().and_then(|(start, len, args)| {
            forms.iter().find(|(k, f)| {
                continues(*start, *len, *k)
                    && stack_cost(args, regions) + stack_cost(f, regions) <= limit
            })
        });
        match extend {
            Some((_, f)) => {
                let r = run.as_mut().expect("a run");
                r.1 += 1;
                r.2.extend(f.iter().cloned());
            }
            None => {
                flush(&mut run, out);
                let (k, f) = forms[0].clone();
                run = Some((k, 1, f));
            }
        }
    }
    flush(&mut run, out);
}

fn encode_moveto(args: &[Operand], out: &mut Vec<u8>) {
    let (op, a): (u8, Vec<Operand>) = if zero(&args[0]) {
        (VMOVETO, vec![args[1].clone()])
    } else if zero(&args[1]) {
        (HMOVETO, vec![args[0].clone()])
    } else {
        (RMOVETO, args.to_vec())
    };
    write_args(&a, out);
    out.push(op);
}

/// A glyph split into what may be shared: (moveto bytes, body bytes) per contour.
struct Pieces {
    contours: Vec<(Vec<u8>, Vec<u8>)>,
    /// The width operand, emitted once before any hints or contours (CFF only).
    width: Option<i32>,
    /// Encoded stem operators, after the width and before the first moveto.
    hints: Vec<u8>,
}

fn pieces(g: &GlyphOps, format: Format, regions: usize, optimize: bool) -> Pieces {
    let mut contours = Vec::new();
    let mut i = 0;
    while i < g.ops.len() {
        let (op, args) = &g.ops[i];
        debug_assert_eq!(*op, RMOVETO);
        let end = (i + 1..g.ops.len())
            .find(|&j| g.ops[j].0 == RMOVETO)
            .unwrap_or(g.ops.len());
        let mut mv = Vec::new();
        let mut body = Vec::new();
        if optimize {
            encode_moveto(args, &mut mv);
            encode_body(&g.ops[i + 1..end], format, regions, &mut body);
        } else {
            write_args(args, &mut mv);
            mv.push(RMOVETO);
            for (op, a) in &g.ops[i + 1..end] {
                write_args(a, &mut body);
                body.push(*op);
            }
        }
        contours.push((mv, body));
        i = end;
    }
    Pieces {
        width: g.width,
        hints: g.hints.clone(),
        contours,
    }
}

/// Subroutine numbers are stored minus a bias that depends on how many there are.
fn bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

/// Encodes all glyphs. Returns their charstrings and the local subroutines.
pub(crate) fn encode(
    glyphs: &[GlyphOps],
    format: Format,
    regions: usize,
    optimize: bool,
) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let all: Vec<Pieces> = glyphs
        .iter()
        .map(|g| pieces(g, format, regions, optimize))
        .collect();

    // Contour bodies used more than once become subroutines when that saves bytes.
    let mut subrs: Vec<Vec<u8>> = Vec::new();
    let mut index_of: HashMap<Vec<u8>, usize> = HashMap::new();
    if optimize {
        let mut counts: HashMap<&[u8], usize> = HashMap::new();
        for p in &all {
            for (_, body) in &p.contours {
                if body.len() >= 4 {
                    *counts.entry(body.as_slice()).or_default() += 1;
                }
            }
        }
        // Calling costs about 3 bytes; storing costs the body, `return` and an offset.
        let mut chosen: Vec<(&[u8], usize)> = counts
            .into_iter()
            .filter(|(b, n)| *n >= 2 && n * b.len() > n * 3 + b.len() + 3)
            .collect();
        // Most used first, so they get the shortest numbers; ties by content for
        // reproducible files.
        chosen.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then(b.0.len().cmp(&a.0.len()))
                .then(a.0.cmp(b.0))
        });
        for (body, _) in chosen.iter().take(65000) {
            index_of.insert(body.to_vec(), subrs.len());
            let mut s = body.to_vec();
            if format == Format::Cff {
                s.push(RETURN);
            }
            subrs.push(s);
        }
    }
    let b = bias(subrs.len());

    let charstrings = all
        .iter()
        .map(|p| {
            let mut out = Vec::new();
            if format == Format::Cff {
                if let Some(w) = p.width {
                    int(w, &mut out);
                }
                out.extend(&p.hints);
            }
            for (mv, body) in &p.contours {
                out.extend(mv);
                match index_of.get(body) {
                    Some(&i) => {
                        int(i as i32 - b, &mut out);
                        out.push(CALLSUBR);
                    }
                    None => out.extend(body),
                }
            }
            if format == Format::Cff {
                out.push(ENDCHAR);
            }
            out
        })
        .collect();
    (charstrings, subrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(v: i32) -> Operand {
        vec![v]
    }

    #[test]
    fn specialises_lines_and_curves() {
        // A rectangle drawn with axis-aligned lines: one hlineto with alternating
        // h/v arguments instead of three rlinetos.
        let g = GlyphOps {
            ops: vec![
                (RMOVETO, vec![o(10), o(0)]),
                (RLINETO, vec![o(100), o(0)]),
                (RLINETO, vec![o(0), o(50)]),
                (RLINETO, vec![o(-100), o(0)]),
            ],
            width: None,
            hints: Vec::new(),
        };
        let (cs, _) = encode(&[g], Format::Cff, 0, true);
        // 10 hmoveto | 100 50 -100 hlineto | endchar
        assert_eq!(cs[0], vec![149, HMOVETO, 239, 189, 39, HLINETO, ENDCHAR]);
        // A curve starting horizontally and ending vertically: hvcurveto.
        let g = GlyphOps {
            ops: vec![
                (RMOVETO, vec![o(0), o(0)]),
                (RRCURVETO, vec![o(30), o(0), o(20), o(20), o(0), o(30)]),
            ],
            width: Some(0),
            hints: Vec::new(),
        };
        let (cs, _) = encode(&[g], Format::Cff, 0, true);
        assert_eq!(
            cs[0],
            vec![139, 139, VMOVETO, 169, 159, 159, 169, HVCURVETO, ENDCHAR]
        );
    }

    #[test]
    fn repeated_contours_become_subroutines() {
        let body = vec![
            (RLINETO, vec![o(37), o(41)]),
            (RLINETO, vec![o(53), o(-29)]),
            (RLINETO, vec![o(-61), o(17)]),
        ];
        let glyph = |dx: i32| GlyphOps {
            ops: std::iter::once((RMOVETO, vec![o(dx), o(5)]))
                .chain(body.clone())
                .collect(),
            width: None,
            hints: Vec::new(),
        };
        let glyphs = [glyph(10), glyph(20), glyph(30)];
        let (cs, subrs) = encode(&glyphs, Format::Cff, 0, true);
        assert_eq!(subrs.len(), 1);
        assert_eq!(*subrs[0].last().unwrap(), RETURN);
        // moveto, then "0 - 107" callsubr, then endchar.
        assert!(cs.iter().all(|c| c.ends_with(&[32, CALLSUBR, ENDCHAR])));
        let (plain, none) = encode(&glyphs, Format::Cff, 0, false);
        assert!(none.is_empty());
        let total = |v: &[Vec<u8>]| v.iter().map(Vec::len).sum::<usize>();
        assert!(total(&cs) + total(&subrs) < total(&plain));
    }

    #[test]
    fn cff2_blends_varying_operands() {
        // One region; the second operand varies by 7.
        let g = GlyphOps {
            ops: vec![(RMOVETO, vec![vec![5, 0], vec![9, 7]])],
            width: None,
            hints: Vec::new(),
        };
        let (cs, _) = encode(&[g], Format::Cff2, 1, true);
        // 5 (plain) | 9 7 1 blend | rmoveto
        assert_eq!(cs[0], vec![144, 148, 146, 140, BLEND, RMOVETO]);
    }
}
