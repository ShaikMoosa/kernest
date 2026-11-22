//! Feature code tokens and conditions (S64), in the Glyphs dialect.
//!
//! Before the feature code goes to the compiler it is expanded for one build:
//!
//! - `$name` and `${expr}`: number values (Font Info › Masters). A static font gets the
//!   value at its location; a variable font gets a variable value when masters differ.
//! - `$[predicate]`: the names of the glyphs that match, e.g. `$[case == smallCaps]`.
//! - `#ifdef VARIABLE` … `#else` … `#endif` (and `#ifndef`): code for one kind of font.
//! - `condition 600 < wght < 900;` inside a feature: the rules after it apply only in
//!   that part of the design space (a feature variation in variable fonts).
//! - `pos @A 10 (wdth:80) 20;`: a value that varies, as in Glyphs.
//! - `lookupflag;` resets the flags, and `delete x;` is `sub x by NULL;`.
//! - `standalone lookup NAME { … } NAME;` inside a feature is defined but not used by
//!   it, and a lookup may close with `} ;` without its name.
//! - `@MMK_L_T` and `@MMK_R_o`: kerning groups as classes (the glyphs whose right
//!   group is T, used on the left of a pair; whose left group is o, on the right).
//!
//! Static expansion keeps every line where it was, so compiler errors point at the
//! designer's own lines.

use std::collections::BTreeMap;

use crate::designspace::{design_to_user, normalize, Location, VariationModel};
use crate::features::Diagnostic;
use crate::glyphdata::{self, Category};
use crate::interpolate::{default_master, Interpolator};
use crate::model::{Font, Glyph, Shape};

/// What the feature code is expanded for.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// A variable font (otherwise a static font at `location`).
    pub variable: bool,
    /// Design-space location of a static font; `None` is the default master.
    pub location: Option<&'a Location>,
}

impl Context<'_> {
    pub const DEFAULT: Context<'static> = Context {
        variable: false,
        location: None,
    };
}

type R<T> = Result<T, String>;

fn err(text: &str, offset: usize, message: impl Into<String>) -> Diagnostic {
    let prefix = &text[..offset.min(text.len())];
    let line = prefix.matches('\n').count() + 1;
    let column = match prefix.rfind('\n') {
        Some(i) => prefix[i + 1..].chars().count() + 1,
        None => prefix.chars().count() + 1,
    };
    Diagnostic {
        line,
        column,
        message: message.into(),
    }
}

/// The feature code ready for the compiler.
pub fn expand(font: &Font, text: &str, ctx: &Context) -> Result<String, Diagnostic> {
    if !text.contains('$')
        && !text.contains("#if")
        && !text.contains("condition")
        && !text.contains("lookup")
        && !text.contains("delete")
        && !text.contains(':')
        && !text.contains("@MMK_")
    {
        return Ok(text.to_string());
    }
    let numbers = Numbers::new(font, ctx).map_err(|e| err(text, 0, e))?;
    let t = directives(text, ctx.variable).map_err(|(o, e)| err(text, o, e))?;
    let t = tokens(font, &t, &numbers).map_err(|(o, e)| err(text, o, e))?;
    let t = varying_values(font, &t, ctx).map_err(|(o, e)| err(text, o, e))?;
    let t = statements(&t);
    let t = lookup_syntax(&t);
    let t = kerning_classes(font, &t);
    conditions(font, &t, ctx).map_err(|(o, e)| err(text, o, e))
}

/// Defines the `@MMK_L_x` / `@MMK_R_x` classes the code uses but doesn't define, at
/// the start of the first line (so later lines keep their numbers).
fn kerning_classes(font: &Font, text: &str) -> String {
    let mut used: Vec<(bool, String)> = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("@MMK_") {
        let after = &rest[i + 5..];
        let left = after.starts_with("L_");
        if left || after.starts_with("R_") {
            let name: String = after[2..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'))
                .collect();
            if !name.is_empty() && !used.contains(&(left, name.clone())) {
                used.push((left, name));
            }
        }
        rest = &rest[i + 5..];
    }
    let mut defs = String::new();
    for (left, name) in used {
        let class = format!("@MMK_{}_{name}", if left { "L" } else { "R" });
        let defined = text.lines().any(|l| {
            let l = l.trim_start();
            l.starts_with(&class) && l[class.len()..].trim_start().starts_with('=')
        });
        if defined {
            continue;
        }
        let members: Vec<&str> = font
            .glyphs
            .iter()
            .filter(|g| {
                let group = if left { &g.right_group } else { &g.left_group };
                group.as_deref() == Some(name.as_str())
            })
            .map(|g| g.name.as_str())
            .collect();
        if !members.is_empty() {
            defs.push_str(&format!("{class} = [{}]; ", members.join(" ")));
        }
    }
    format!("{defs}{text}")
}

// --- #ifdef VARIABLE ------------------------------------------------------------------

fn directives(text: &str, variable: bool) -> Result<String, (usize, String)> {
    let mut out = String::with_capacity(text.len());
    // Each open block: (taking its first branch, in its else branch, line offset).
    let mut stack: Vec<(bool, bool, usize)> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let word = trimmed.split_whitespace().next().unwrap_or("");
        let arg = trimmed.split_whitespace().nth(1).unwrap_or("");
        let nl = if line.ends_with('\n') { "\n" } else { "" };
        match word {
            "#ifdef" | "#ifndef" => {
                let defined = arg == "VARIABLE" && variable;
                stack.push((defined == (word == "#ifdef"), false, offset));
                out.push_str(nl);
            }
            "#else" => {
                let Some(top) = stack.last_mut() else {
                    return Err((offset, "#else without #ifdef".into()));
                };
                top.0 = !top.0;
                top.1 = true;
                out.push_str(nl);
            }
            "#endif" => {
                if stack.pop().is_none() {
                    return Err((offset, "#endif without #ifdef".into()));
                }
                out.push_str(nl);
            }
            _ => {
                if stack.iter().all(|s| s.0) {
                    out.push_str(line);
                } else {
                    out.push_str(nl);
                }
            }
        }
        offset += line.len();
    }
    if let Some((_, _, o)) = stack.last() {
        return Err((*o, "#ifdef without #endif".into()));
    }
    Ok(out)
}

// --- Number values ----------------------------------------------------------------------

/// Number values for one build: per master (in master order) and how to write them.
struct Numbers<'a> {
    font: &'a Font,
    values: BTreeMap<String, Vec<f64>>,
    variable: bool,
    interp: Option<Interpolator>,
}

impl<'a> Numbers<'a> {
    fn new(font: &'a Font, ctx: &Context) -> R<Self> {
        let values = font
            .info
            .numbers
            .iter()
            .map(|(name, by_master)| {
                (
                    name.clone(),
                    font.masters
                        .iter()
                        .map(|m| by_master.get(&m.id).copied().unwrap_or(0.0))
                        .collect(),
                )
            })
            .collect();
        let interp = match (ctx.variable, ctx.location) {
            (false, Some(loc)) if font.masters.len() > 1 => Some(Interpolator::new(font, loc)?),
            _ => None,
        };
        Ok(Self {
            font,
            values,
            variable: ctx.variable,
            interp,
        })
    }

    fn get(&self, name: &str) -> R<Vec<f64>> {
        self.values.get(name).cloned().ok_or_else(|| {
            format!("there is no number value named \"{name}\" (add it in Font Info › Masters)")
        })
    }

    /// A glyph property token (`period:LSB`, `b:anchors.top.x`), per master: the
    /// advance width, sidebearings (components included) or an anchor position.
    fn glyph_property(&self, token: &str) -> R<Vec<f64>> {
        let (name, path) = token.split_once(':').unwrap_or((token, ""));
        let font = self.font;
        let g = font
            .glyph(name)
            .ok_or_else(|| format!("unknown glyph \"{name}\" in token"))?;
        let lower = path.to_ascii_lowercase();
        font.masters
            .iter()
            .map(|m| {
                let layer = g.layer(&m.id).ok_or_else(|| {
                    format!("glyph \"{name}\" has no layer for master \"{}\"", m.name)
                })?;
                let sides = || {
                    crate::geometry::sidebearings(font, layer).ok_or_else(|| {
                        format!("glyph \"{name}\" has no outline in \"{}\", so it has no sidebearings", m.name)
                    })
                };
                match lower.as_str() {
                    "width" => Ok(layer.width),
                    "lsb" => Ok(sides()?.0),
                    "rsb" => Ok(sides()?.1),
                    "tsb" | "bsb" => {
                        // The vertical origin and advance, with export's defaults.
                        let (origin, advance) = crate::vertical::origin_and_advance(m, layer);
                        let b = crate::geometry::bounds(font, layer).ok_or_else(|| {
                            format!("glyph \"{name}\" has no outline in \"{}\", so it has no sidebearings", m.name)
                        })?;
                        Ok(if lower == "tsb" {
                            origin - b.max_y
                        } else {
                            b.min_y - (origin - advance)
                        })
                    }
                    _ => {
                        let anchor = lower
                            .starts_with("anchors.")
                            .then(|| path["anchors.".len()..].rsplit_once('.'))
                            .flatten()
                            .filter(|(_, axis)| matches!(*axis, "x" | "y" | "X" | "Y"));
                        let Some((anchor, axis)) = anchor else {
                            return Err(format!(
                                "\"{path}\" isn't a glyph property; use width, LSB, RSB or anchors.<name>.x / .y"
                            ));
                        };
                        let a = layer
                            .anchors
                            .iter()
                            .find(|a| a.name == anchor)
                            .ok_or_else(|| {
                                format!("glyph \"{name}\" has no anchor \"{anchor}\" in \"{}\"", m.name)
                            })?;
                        Ok(if axis.eq_ignore_ascii_case("x") { a.x } else { a.y })
                    }
                }
            })
            .collect()
    }

    /// The text to write for per-master values.
    fn write(&self, per_master: &[f64]) -> String {
        if self.variable {
            if per_master.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-9) {
                return fmt(per_master.first().copied().unwrap_or(0.0));
            }
            let parts: Vec<String> =
                self.font
                    .masters
                    .iter()
                    .zip(per_master)
                    .map(|(m, v)| {
                        let loc: Vec<String> =
                            self.font
                                .axes
                                .iter()
                                .map(|a| {
                                    let d =
                                        m.location.get(&a.tag).copied().unwrap_or_else(|| {
                                            crate::designspace::design_bounds(a).1
                                        });
                                    format!("{}={}d", a.tag, fmt_f(d))
                                })
                                .collect();
                        format!("{}:{}", loc.join(","), fmt(*v))
                    })
                    .collect();
            return format!("({})", parts.join(" "));
        }
        let v = match &self.interp {
            Some(i) => {
                let rows: Vec<Vec<f64>> = per_master.iter().map(|v| vec![*v]).collect();
                i.values(&rows).first().copied().unwrap_or(0.0)
            }
            None => {
                let d = default_master(self.font);
                let i = self
                    .font
                    .masters
                    .iter()
                    .position(|m| m.id == d.id)
                    .unwrap_or(0);
                per_master.get(i).copied().unwrap_or(0.0)
            }
        };
        fmt(v)
    }
}

fn fmt(v: f64) -> String {
    format!("{}", v.round() as i64)
}

fn fmt_f(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// `${expr}`: + - * / and brackets over numbers and number values, per master.
fn eval_expr(expr: &str, numbers: &Numbers) -> R<Vec<f64>> {
    let toks = lex_expr(expr)?;
    let n = numbers.font.masters.len().max(1);
    let mut p = ExprParser {
        toks: &toks,
        i: 0,
        numbers,
        n,
    };
    let v = p.sum()?;
    if p.i != toks.len() {
        return Err(format!("unexpected \"{}\" in ${{{expr}}}", toks[p.i]));
    }
    Ok(v)
}

fn lex_expr(s: &str) -> R<Vec<String>> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if "+-*/()".contains(c) {
            out.push(c.to_string());
            i += 1;
        } else if c.is_ascii_digit() || c == '.' || c.is_alphabetic() || c == '_' {
            let start = i;
            // A glyph property token: a glyph name (which may hold "." and "-")
            // straight before ":", then the property path.
            let mut j = i;
            while j < cs.len() && (cs[j].is_alphanumeric() || matches!(cs[j], '.' | '_' | '-')) {
                j += 1;
            }
            if cs.get(j) == Some(&':') {
                j += 1;
                while j < cs.len() && (cs[j].is_alphanumeric() || matches!(cs[j], '.' | '_')) {
                    j += 1;
                }
                out.push(cs[start..j].iter().collect());
                i = j;
                continue;
            }
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '.' || cs[i] == '_') {
                i += 1;
            }
            out.push(cs[start..i].iter().collect());
        } else {
            return Err(format!("\"{c}\" can't be used in a number expression"));
        }
    }
    Ok(out)
}

struct ExprParser<'a> {
    toks: &'a [String],
    i: usize,
    numbers: &'a Numbers<'a>,
    n: usize,
}

impl ExprParser<'_> {
    fn peek(&self) -> Option<&str> {
        self.toks.get(self.i).map(String::as_str)
    }
    fn sum(&mut self) -> R<Vec<f64>> {
        let mut v = self.product()?;
        while let Some(op @ ("+" | "-")) = self.peek() {
            let op = op.to_string();
            self.i += 1;
            let r = self.product()?;
            for (a, b) in v.iter_mut().zip(r) {
                if op == "+" {
                    *a += b
                } else {
                    *a -= b
                }
            }
        }
        Ok(v)
    }
    fn product(&mut self) -> R<Vec<f64>> {
        let mut v = self.unary()?;
        while let Some(op @ ("*" | "/")) = self.peek() {
            let op = op.to_string();
            self.i += 1;
            let r = self.unary()?;
            for (a, b) in v.iter_mut().zip(r) {
                if op == "*" {
                    *a *= b
                } else if b == 0.0 {
                    return Err("division by zero".into());
                } else {
                    *a /= b
                }
            }
        }
        Ok(v)
    }
    fn unary(&mut self) -> R<Vec<f64>> {
        if self.peek() == Some("-") {
            self.i += 1;
            return Ok(self.unary()?.into_iter().map(|v| -v).collect());
        }
        self.atom()
    }
    fn atom(&mut self) -> R<Vec<f64>> {
        let Some(t) = self.toks.get(self.i).cloned() else {
            return Err("the expression ends too early".into());
        };
        self.i += 1;
        if t == "(" {
            let v = self.sum()?;
            if self.peek() != Some(")") {
                return Err("a bracket isn't closed".into());
            }
            self.i += 1;
            return Ok(v);
        }
        if let Ok(v) = t.parse::<f64>() {
            return Ok(vec![v; self.n]);
        }
        if t.contains(':') {
            return self.numbers.glyph_property(&t);
        }
        self.numbers.get(&t)
    }
}

// --- Glyph predicates ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Str(String),
    Num(f64),
    Bool(bool),
    List(Vec<Val>),
    Null,
}

impl Val {
    fn text(&self) -> String {
        match self {
            Val::Str(s) => s.clone(),
            Val::Num(n) => fmt_f(*n),
            Val::Bool(b) => b.to_string(),
            Val::List(l) => l.iter().map(Val::text).collect::<Vec<_>>().join(" "),
            Val::Null => String::new(),
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            Val::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Val::Str(s) => s.parse().ok(),
            _ => None,
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Val::Bool(b) => *b,
            Val::Num(n) => *n != 0.0,
            Val::Str(s) => !s.is_empty(),
            Val::List(l) => !l.is_empty(),
            Val::Null => false,
        }
    }
}

/// The glyph's case, as Glyphs names it.
pub fn case_of(g: &Glyph) -> &'static str {
    let n = &g.name;
    let suffix = n.split('.').skip(1).collect::<Vec<_>>();
    if suffix
        .iter()
        .any(|s| matches!(*s, "sc" | "smcp" | "c2sc" | "pcap" | "c2pc"))
    {
        return "smallCaps";
    }
    if suffix
        .iter()
        .any(|s| matches!(*s, "sups" | "subs" | "sinf" | "numr" | "dnom"))
        || n.contains("superior")
        || n.contains("inferior")
    {
        return "minor";
    }
    let cp = g
        .unicodes
        .first()
        .map(|c| c.0)
        .or_else(|| glyphdata::codepoint_for_name(n.split('.').next().unwrap_or(n)));
    match cp.and_then(char::from_u32) {
        Some(c) if c.is_uppercase() => "upper",
        Some(c) if c.is_lowercase() => "lower",
        _ => "noCase",
    }
}

fn attribute(font: &Font, g: &Glyph, key: &str) -> Option<Val> {
    let cat = g.category();
    let cp = g.unicodes.first().map(|c| c.0);
    let s = |v: &str| Some(Val::Str(v.to_string()));
    match key {
        "name" => s(&g.name),
        "category" => s(match cat {
            Category::Uppercase | Category::Lowercase => "Letter",
            c => c.label(),
        }),
        "subCategory" => {
            let sub = g.subcategory();
            if !sub.is_empty() {
                s(&sub)
            } else {
                s(match cat {
                    Category::Uppercase => "Uppercase",
                    Category::Lowercase => "Lowercase",
                    _ => "",
                })
            }
        }
        "case" => s(g.case_override.as_deref().unwrap_or(case_of(g))),
        "script" => s(&g.script().to_ascii_lowercase()),
        "direction" => s(match g.script().as_str() {
            "Arabic" | "Hebrew" => "RTL",
            _ => "LTR",
        }),
        "unicode" => match cp {
            Some(c) => s(&format!("{c:04X}")),
            None => Some(Val::Null),
        },
        "unicodes" => Some(Val::List(
            g.unicodes
                .iter()
                .map(|c| Val::Str(format!("{:04X}", c.0)))
                .collect(),
        )),
        "countOfUnicodes" => Some(Val::Num(g.unicodes.len() as f64)),
        "countOfLayers" => Some(Val::Num(g.layers.len() as f64)),
        "hasComponents" => Some(Val::Bool(g.layers.iter().any(|l| {
            l.shapes
                .iter()
                .any(|sh| matches!(sh, Shape::Component { .. }))
        }))),
        "isColor" | "isColorGlyph" => Some(Val::Bool(g.is_color())),
        "export" => Some(Val::Bool(g.export)),
        "leftKerningGroup" => Some(g.left_group.clone().map_or(Val::Null, Val::Str)),
        "rightKerningGroup" => Some(g.right_group.clone().map_or(Val::Null, Val::Str)),
        "tags" => Some(Val::List(g.tags.iter().cloned().map(Val::Str).collect())),
        "note" => Some(g.note.clone().map_or(Val::Null, Val::Str)),
        "productionName" => Some(Val::Str(g.production_name.clone().unwrap_or_default())),
        "locked" => Some(Val::Bool(g.locked)),
        _ => {
            // layer.width, layer.anchors.top.x / .y: the default master's layer.
            let rest = key.strip_prefix("layer.")?;
            let l = g.layer(&default_master(font).id)?;
            if rest == "width" {
                return Some(Val::Num(l.width));
            }
            if rest == "countOfPaths" {
                return Some(Val::Num(
                    l.shapes
                        .iter()
                        .filter(|s| matches!(s, Shape::Path(_)))
                        .count() as f64,
                ));
            }
            let a = rest.strip_prefix("anchors.")?;
            let (name, coord) = a.rsplit_once('.')?;
            Some(match l.anchors.iter().find(|x| x.name == name) {
                Some(x) if coord == "x" => Val::Num(x.x),
                Some(x) if coord == "y" => Val::Num(x.y),
                Some(_) => return None,
                None => Val::Null,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum PTok {
    Word(String),
    Str(String),
    Num(f64),
    Op(String),
}

fn lex_predicate(s: &str) -> R<Vec<PTok>> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' || c == '\'' {
            let start = i + 1;
            i += 1;
            while i < cs.len() && cs[i] != c {
                i += 1;
            }
            if i >= cs.len() {
                return Err("a quoted text isn't closed".into());
            }
            out.push(PTok::Str(cs[start..i].iter().collect()));
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '-' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            i += 1;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            let t: String = cs[start..i].iter().collect();
            out.push(PTok::Num(
                t.parse().map_err(|_| format!("\"{t}\" isn't a number"))?,
            ));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_' || cs[i] == '.') {
                i += 1;
            }
            out.push(PTok::Word(cs[start..i].iter().collect()));
        } else {
            let two: String = cs[i..(i + 2).min(cs.len())].iter().collect();
            if ["==", "!=", "<=", ">=", "&&", "||"].contains(&two.as_str()) {
                out.push(PTok::Op(two));
                i += 2;
            } else if "<>!(){},=".contains(c) {
                out.push(PTok::Op(if c == '=' { "==".into() } else { c.to_string() }));
                i += 1;
            } else {
                return Err(format!("\"{c}\" can't be used in a glyph predicate"));
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
enum Pred {
    And(Box<Pred>, Box<Pred>),
    Or(Box<Pred>, Box<Pred>),
    Not(Box<Pred>),
    Cmp(Operand, String, Operand),
    Bare(Operand),
}

#[derive(Debug, Clone)]
enum Operand {
    Key(String),
    Lit(Val),
    List(Vec<Operand>),
}

struct PredParser {
    toks: Vec<PTok>,
    i: usize,
}

const OPS: &[&str] = &[
    "==",
    "!=",
    "<",
    ">",
    "<=",
    ">=",
    "BEGINSWITH",
    "ENDSWITH",
    "CONTAINS",
    "LIKE",
    "IN",
];

impl PredParser {
    fn word_is(&self, w: &str) -> bool {
        matches!(self.toks.get(self.i), Some(PTok::Word(x)) if x.eq_ignore_ascii_case(w))
    }
    fn op_is(&self, o: &str) -> bool {
        matches!(self.toks.get(self.i), Some(PTok::Op(x)) if x == o)
    }
    fn or(&mut self) -> R<Pred> {
        let mut l = self.and()?;
        while self.word_is("OR") || self.op_is("||") {
            self.i += 1;
            l = Pred::Or(Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }
    fn and(&mut self) -> R<Pred> {
        let mut l = self.not()?;
        while self.word_is("AND") || self.op_is("&&") {
            self.i += 1;
            l = Pred::And(Box::new(l), Box::new(self.not()?));
        }
        Ok(l)
    }
    fn not(&mut self) -> R<Pred> {
        if self.word_is("NOT") || self.op_is("!") {
            self.i += 1;
            return Ok(Pred::Not(Box::new(self.not()?)));
        }
        if self.op_is("(") {
            self.i += 1;
            let p = self.or()?;
            if !self.op_is(")") {
                return Err("a bracket isn't closed".into());
            }
            self.i += 1;
            return Ok(p);
        }
        let left = self.operand()?;
        let op = match self.toks.get(self.i) {
            Some(PTok::Op(o)) if OPS.contains(&o.as_str()) => o.clone(),
            Some(PTok::Word(w)) if OPS.iter().any(|o| o.eq_ignore_ascii_case(w)) => {
                w.to_ascii_uppercase()
            }
            _ => return Ok(Pred::Bare(left)),
        };
        self.i += 1;
        let right = self.operand()?;
        Ok(Pred::Cmp(left, op, right))
    }
    fn operand(&mut self) -> R<Operand> {
        let t = self
            .toks
            .get(self.i)
            .cloned()
            .ok_or("the predicate ends too early")?;
        self.i += 1;
        Ok(match t {
            PTok::Str(s) => Operand::Lit(Val::Str(s)),
            PTok::Num(n) => Operand::Lit(Val::Num(n)),
            PTok::Word(w) => match w.as_str() {
                "true" | "YES" | "yes" => Operand::Lit(Val::Bool(true)),
                "false" | "NO" | "no" => Operand::Lit(Val::Bool(false)),
                "nil" | "null" => Operand::Lit(Val::Null),
                _ => Operand::Key(w),
            },
            PTok::Op(o) if o == "{" => {
                let mut items = Vec::new();
                while !self.op_is("}") {
                    items.push(self.operand()?);
                    if self.op_is(",") {
                        self.i += 1;
                    } else if !self.op_is("}") {
                        return Err("a list needs commas between items".into());
                    }
                }
                self.i += 1;
                Operand::List(items)
            }
            PTok::Op(o) => return Err(format!("unexpected \"{o}\"")),
        })
    }
}

fn operand_value(font: &Font, g: &Glyph, o: &Operand) -> Val {
    match o {
        Operand::Lit(v) => v.clone(),
        // An unknown word is a plain value (upper, smallCaps, Letter, RTL…).
        Operand::Key(k) => attribute(font, g, k).unwrap_or_else(|| Val::Str(k.clone())),
        Operand::List(l) => Val::List(l.iter().map(|x| operand_value(font, g, x)).collect()),
    }
}

fn like(text: &str, pattern: &str) -> bool {
    fn go(t: &[char], p: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => (0..=t.len()).any(|i| go(&t[i..], &p[1..])),
            Some('?') => !t.is_empty() && go(&t[1..], &p[1..]),
            Some(c) => t.first() == Some(c) && go(&t[1..], &p[1..]),
        }
    }
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    go(&t, &p)
}

/// Equality; `loose` ignores letter case (for plain words such as `smallcaps`).
fn equal_as(a: &Val, b: &Val, loose: bool) -> bool {
    match (a, b) {
        (Val::Null, Val::Null) => true,
        (Val::Null, Val::Str(s)) | (Val::Str(s), Val::Null) => s.is_empty(),
        (Val::Null, _) | (_, Val::Null) => false,
        (Val::Str(x), Val::Str(y)) if loose => x.eq_ignore_ascii_case(y),
        (Val::Str(x), Val::Str(y)) => x == y,
        (Val::List(x), Val::List(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| equal_as(a, b, loose))
        }
        _ => match (a.num(), b.num()) {
            (Some(x), Some(y)) => (x - y).abs() < 1e-9,
            _ => a.text() == b.text(),
        },
    }
}

fn equal(a: &Val, b: &Val) -> bool {
    equal_as(a, b, false)
}

/// A word that names no attribute: a plain value like `upper` or `Letter`.
fn is_plain_word(font: &Font, g: &Glyph, o: &Operand) -> bool {
    matches!(o, Operand::Key(k) if attribute(font, g, k).is_none())
}

fn holds(font: &Font, g: &Glyph, p: &Pred) -> bool {
    match p {
        Pred::And(a, b) => holds(font, g, a) && holds(font, g, b),
        Pred::Or(a, b) => holds(font, g, a) || holds(font, g, b),
        Pred::Not(a) => !holds(font, g, a),
        Pred::Bare(o) => operand_value(font, g, o).truthy(),
        Pred::Cmp(l, op, r) => {
            let (a, b) = (operand_value(font, g, l), operand_value(font, g, r));
            let lower = |v: &Val| v.text().to_lowercase();
            let loose = is_plain_word(font, g, l) || is_plain_word(font, g, r);
            match op.as_str() {
                "==" => equal_as(&a, &b, loose),
                "!=" => !equal_as(&a, &b, loose),
                "<" | ">" | "<=" | ">=" => match (a.num(), b.num()) {
                    (Some(x), Some(y)) => match op.as_str() {
                        "<" => x < y,
                        ">" => x > y,
                        "<=" => x <= y,
                        _ => x >= y,
                    },
                    _ => false,
                },
                "BEGINSWITH" => lower(&a).starts_with(&lower(&b)),
                "ENDSWITH" => lower(&a).ends_with(&lower(&b)),
                "CONTAINS" => match &a {
                    Val::List(items) => items.iter().any(|x| equal(x, &b)),
                    _ => lower(&a).contains(&lower(&b)),
                },
                "LIKE" => like(&a.text(), &b.text()),
                "IN" => match &b {
                    Val::List(items) => items.iter().any(|x| equal(x, &a)),
                    _ => lower(&b).contains(&lower(&a)),
                },
                _ => false,
            }
        }
    }
}

/// The glyphs (exported ones, in glyph order) that match a predicate such as
/// `case == smallCaps AND name endswith ".sc"`.
pub fn predicate_glyphs(font: &Font, predicate: &str) -> R<Vec<String>> {
    let toks = lex_predicate(predicate)?;
    if toks.is_empty() {
        return Err("the glyph predicate is empty".into());
    }
    let mut p = PredParser { toks, i: 0 };
    let pred = p.or()?;
    if p.i != p.toks.len() {
        return Err(format!("unexpected text in $[{predicate}]"));
    }
    Ok(font
        .glyphs
        .iter()
        .filter(|g| g.export && holds(font, g, &pred))
        .map(|g| g.name.clone())
        .collect())
}

// --- Token replacement ------------------------------------------------------------------

/// Byte offset of the bracket closing the one at `open`, outside quotes.
fn closing(text: &str, open: usize, o: u8, c: u8) -> Option<usize> {
    let b = text.as_bytes();
    let mut depth = 0;
    let mut quote: Option<u8> = None;
    let mut i = open;
    while i < b.len() {
        let ch = b[i];
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == b'"' || ch == b'\'' => quote = Some(ch),
            None if ch == o => depth += 1,
            None if ch == c => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            None if ch == b'\n' => return None,
            None => {}
        }
        i += 1;
    }
    None
}

fn tokens(font: &Font, text: &str, numbers: &Numbers) -> Result<String, (usize, String)> {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut last = 0;
    let mut in_comment = false;
    let mut in_string = false;
    while i < b.len() {
        let c = b[i];
        if in_comment {
            if c == b'\n' {
                in_comment = false;
            }
            i += 1;
            continue;
        }
        if in_string {
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'#' => in_comment = true,
            b'"' => in_string = true,
            b'$' => {
                let (end, replacement) = match b.get(i + 1) {
                    Some(b'[') => {
                        let close = closing(text, i + 1, b'[', b']')
                            .ok_or((i, "$[ isn't closed with ]".to_string()))?;
                        let names =
                            predicate_glyphs(font, &text[i + 2..close]).map_err(|e| (i, e))?;
                        (close + 1, names.join(" "))
                    }
                    Some(b'{') => {
                        let close = closing(text, i + 1, b'{', b'}')
                            .ok_or((i, "${ isn't closed with }".to_string()))?;
                        let v = eval_expr(&text[i + 2..close], numbers).map_err(|e| (i, e))?;
                        (close + 1, numbers.write(&v))
                    }
                    Some(ch) if ch.is_ascii_alphabetic() || *ch == b'_' => {
                        let mut e = i + 1;
                        while e < b.len() && (b[e].is_ascii_alphanumeric() || b[e] == b'_') {
                            e += 1;
                        }
                        let v = numbers.get(&text[i + 1..e]).map_err(|m| (i, m))?;
                        (e, numbers.write(&v))
                    }
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                out.push_str(&text[last..i]);
                out.push_str(&replacement);
                i = end;
                last = end;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&text[last..]);
    Ok(out)
}

// --- Glyphs-style varying values: `10 (wdth:80) 20` -------------------------------------

/// `(wdth:80 opsz:28)`: a Glyphs location, as (tag, design value) pairs.
fn glyphs_location(inner: &str) -> Option<Vec<(String, f64)>> {
    let mut out = Vec::new();
    for part in inner.split_whitespace() {
        let (tag, v) = part.split_once(':')?;
        if tag.is_empty() || tag.len() > 4 || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        out.push((tag.to_string(), v.parse().ok()?));
    }
    (!out.is_empty()).then_some(out)
}

fn varying_values(font: &Font, text: &str, ctx: &Context) -> Result<String, (usize, String)> {
    if !text.contains(':') {
        return Ok(text.to_string());
    }
    let mut out = String::with_capacity(text.len());
    for (line_start, line) in line_offsets(text) {
        let code_end = line.find('#').unwrap_or(line.len());
        let (code, comment) = line.split_at(code_end);
        out.push_str(&replace_varying(font, code, ctx).map_err(|e| (line_start, e))?);
        out.push_str(comment);
    }
    Ok(out)
}

fn line_offsets(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    text.split_inclusive('\n').map(move |l| {
        let s = at;
        at += l.len();
        (s, l)
    })
}

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.parse::<f64>().is_ok()
}

fn replace_varying(font: &Font, code: &str, ctx: &Context) -> R<String> {
    // Tokenise on whitespace, keeping bracketed groups whole.
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for ch in code.chars() {
        match ch {
            '(' => {
                if depth == 0 && !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                cur.push(ch);
                depth -= 1;
                if depth == 0 {
                    parts.push(std::mem::take(&mut cur));
                }
            }
            c if depth == 0 && (c.is_whitespace() || c == ';' || c == '<' || c == '>') => {
                if !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
                parts.push(c.to_string());
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    let is_loc = |p: &str| {
        p.starts_with('(') && p.ends_with(')') && glyphs_location(&p[1..p.len() - 1]).is_some()
    };
    if !parts.iter().any(|p| is_loc(p)) {
        return Ok(code.to_string());
    }
    // Indices of non-space parts.
    let real: Vec<usize> = (0..parts.len())
        .filter(|&i| !parts[i].trim().is_empty())
        .collect();
    let mut out: Vec<String> = parts.clone();
    let mut k = 0;
    while k < real.len() {
        let i = real[k];
        if is_number(&parts[i]) && k + 2 < real.len() && is_loc(&parts[real[k + 1]]) {
            // A chain: v0 (L1) v1 (L2) v2 …
            let mut values = vec![parts[i].parse::<f64>().unwrap_or(0.0)];
            let mut locs: Vec<Vec<(String, f64)>> = Vec::new();
            let mut j = k + 1;
            while j + 1 < real.len() && is_loc(&parts[real[j]]) && is_number(&parts[real[j + 1]]) {
                let p = &parts[real[j]];
                locs.push(glyphs_location(&p[1..p.len() - 1]).unwrap_or_default());
                values.push(parts[real[j + 1]].parse().unwrap_or(0.0));
                j += 2;
            }
            if locs.is_empty() {
                k += 1;
                continue;
            }
            for (tag, _) in locs.iter().flatten() {
                if !font.axes.iter().any(|a| &a.tag == tag) {
                    return Err(format!("the font has no axis \"{tag}\""));
                }
            }
            let text = varying_text(font, &values, &locs, ctx)?;
            out[i] = text;
            for &idx in &real[k + 1..j] {
                out[idx].clear();
            }
            // Drop the spaces between the removed parts.
            for idx in real[k]..real[j - 1] {
                if parts[idx].trim().is_empty() {
                    out[idx].clear();
                }
            }
            k = j;
        } else {
            k += 1;
        }
    }
    Ok(out.concat())
}

fn varying_text(
    font: &Font,
    values: &[f64],
    locs: &[Vec<(String, f64)>],
    ctx: &Context,
) -> R<String> {
    if ctx.variable {
        let mut parts = vec![fmt(values[0])];
        for (loc, v) in locs.iter().zip(&values[1..]) {
            let spec: Vec<String> = loc
                .iter()
                .map(|(t, d)| format!("{t}={}d", fmt_f(*d)))
                .collect();
            parts.push(format!("{}:{}", spec.join(","), fmt(*v)));
        }
        return Ok(format!("({})", parts.join(" ")));
    }
    // Static: interpolate at the location with a model of these points.
    let default: Location = font
        .axes
        .iter()
        .map(|a| (a.tag.clone(), crate::designspace::design_bounds(a).1))
        .collect();
    let mut points = vec![normalize(font, &default)];
    for loc in locs {
        let mut l = default.clone();
        for (t, d) in loc {
            l.insert(t.clone(), *d);
        }
        points.push(normalize(font, &l));
    }
    let order: Vec<String> = font.axes.iter().map(|a| a.tag.clone()).collect();
    let model = VariationModel::new(&points, &order)
        .map_err(|e| format!("these locations don't make a variation model: {e}"))?;
    let at = normalize(font, ctx.location.unwrap_or(&default_master(font).location));
    let rows: Vec<Vec<f64>> = values.iter().map(|v| vec![*v]).collect();
    Ok(fmt(model
        .interpolate(&rows, &at)
        .first()
        .copied()
        .unwrap_or(values[0])))
}

// --- Small statement fixes ------------------------------------------------------------

fn statements(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let code_end = line.find('#').unwrap_or(line.len());
        let (code, comment) = line.split_at(code_end);
        let mut code = code.to_string();
        // `lookupflag;` resets the flags.
        let compact: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
        if compact == "lookupflag;" || compact.ends_with(" lookupflag;") {
            code = code.replacen("lookupflag", "lookupflag 0", 1);
        }
        let trimmed = code.trim_start();
        if let Some(rest) = trimmed.strip_prefix("delete ") {
            if let Some(semi) = rest.find(';') {
                let indent = &code[..code.len() - trimmed.len()];
                code = format!(
                    "{indent}sub {} by NULL;{}",
                    rest[..semi].trim(),
                    &rest[semi + 1..]
                );
            }
        }
        out.push_str(&code);
        out.push_str(comment);
    }
    out
}

// --- Glyphs lookup syntax ---------------------------------------------------------------

/// A token of feature code for the lookup pass: a word or one of `{ } ;`, with its
/// byte range. Comments and strings are skipped.
struct FeaTok<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

fn fea_tokens(text: &str) -> Vec<FeaTok<'_>> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'#' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += 1;
                }
                i += 1;
            }
            b'{' | b'}' | b';' => {
                out.push(FeaTok {
                    text: &text[i..i + 1],
                    start: i,
                    end: i + 1,
                });
                i += 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < b.len()
                    && !b[i].is_ascii_whitespace()
                    && !matches!(b[i], b'{' | b'}' | b';' | b'#' | b'"')
                {
                    i += 1;
                }
                out.push(FeaTok {
                    text: &text[start..i],
                    start,
                    end: i,
                });
            }
        }
    }
    out
}

enum Block {
    /// A feature block: its tag and the latest `script`, `language` and `lookupflag`
    /// statements at its top level.
    Feature {
        tag: String,
        script: Option<String>,
        language: Option<String>,
        flags: Option<String>,
    },
    /// A lookup block: its name, and the tag of the feature it's standalone in.
    Lookup {
        name: String,
        standalone_in: Option<String>,
    },
    Other,
}

/// Glyphs' lookup syntax, rewritten for the compiler without moving any line:
///
/// - `} ;` closing a lookup gets the lookup's name: `} NAME;`.
/// - `standalone lookup NAME { … } NAME;` inside a feature is defined but not used
///   by the feature: the feature block is closed before it and opened again after it
///   (with its `script`, `language` and `lookupflag` statements repeated), so the lookup
///   keeps its place in lookup order. Outside a feature `standalone` changes nothing.
fn lookup_syntax(text: &str) -> String {
    let toks = fea_tokens(text);
    // (start, end, replacement), in text order.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut stack: Vec<Block> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        let next = |k: usize| toks.get(i + k).map(|t| t.text);
        let at_statement_start = i == 0 || matches!(toks[i - 1].text, ";" | "{" | "}");
        match t.text {
            "feature" if next(2) == Some("{") => {
                stack.push(Block::Feature {
                    tag: next(1).unwrap_or_default().to_string(),
                    script: None,
                    language: None,
                    flags: None,
                });
                i += 3;
                continue;
            }
            "standalone" if next(1) == Some("lookup") && next(3) == Some("{") => {
                let feature = match stack.last() {
                    Some(Block::Feature { tag, .. }) => Some(tag.clone()),
                    _ => None,
                };
                // Close the feature here (or just drop the word outside a feature).
                edits.push((
                    t.start,
                    t.end,
                    feature
                        .as_ref()
                        .map(|tag| format!("}} {tag};"))
                        .unwrap_or_default(),
                ));
                stack.push(Block::Lookup {
                    name: next(2).unwrap_or_default().to_string(),
                    standalone_in: feature,
                });
                i += 4;
                continue;
            }
            "lookup" if next(2) == Some("{") => {
                stack.push(Block::Lookup {
                    name: next(1).unwrap_or_default().to_string(),
                    standalone_in: None,
                });
                i += 3;
                continue;
            }
            "script" | "language" | "lookupflag" if at_statement_start => {
                if let Some(Block::Feature {
                    script,
                    language,
                    flags,
                    ..
                }) = stack.last_mut()
                {
                    let end = toks[i..]
                        .iter()
                        .position(|x| x.text == ";")
                        .map(|k| toks[i + k].end);
                    if let Some(end) = end {
                        let stmt = text[t.start..end]
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        match t.text {
                            "script" => {
                                *script = Some(stmt);
                                *language = None;
                            }
                            "language" => *language = Some(stmt),
                            _ => *flags = Some(stmt),
                        }
                    }
                }
            }
            "{" => stack.push(Block::Other),
            "}" => {
                if let Some(Block::Lookup {
                    name,
                    standalone_in,
                }) = stack.pop()
                {
                    // `} ;`: the name is optional in Glyphs.
                    let mut close_end = t.end;
                    if next(1) == Some(";") {
                        let at = toks[i + 1].start;
                        let space = if at > t.end { "" } else { " " };
                        edits.push((at, at, format!("{space}{name}")));
                        close_end = toks[i + 1].end;
                    } else if next(2) == Some(";") {
                        close_end = toks[i + 2].end;
                    }
                    if let Some(tag) = standalone_in {
                        // Open the feature again, as it was before the lookup.
                        let mut reopen = format!(" feature {tag} {{");
                        if let Some(Block::Feature {
                            script,
                            language,
                            flags,
                            ..
                        }) = stack.last()
                        {
                            for s in [script, language, flags].into_iter().flatten() {
                                reopen.push(' ');
                                reopen.push_str(s);
                            }
                        }
                        edits.push((close_end, close_end, reopen));
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    edits.sort_by_key(|e| (e.0, e.1));
    let mut out = text.to_string();
    for (start, end, rep) in edits.into_iter().rev() {
        out.replace_range(start..end, &rep);
    }
    out
}

// --- Conditions -------------------------------------------------------------------------

/// One range of a condition: axis tag, user-space min and max.
type Range = (String, f64, f64);

fn parse_condition(font: &Font, s: &str) -> R<Vec<Range>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let toks: Vec<&str> = part
            .split(|c: char| c.is_whitespace())
            .filter(|t| !t.is_empty())
            .flat_map(split_ops)
            .collect();
        let axis = |tag: &str| {
            font.axes
                .iter()
                .find(|a| a.tag == tag)
                .ok_or_else(|| format!("the font has no axis \"{tag}\""))
        };
        let num = |t: &str| {
            t.parse::<f64>()
                .map_err(|_| format!("\"{t}\" isn't a number"))
        };
        let (tag, min, max) = match toks.as_slice() {
            [a, op1, tag, op2, b] if is_lt(op1) && is_lt(op2) => {
                (tag.to_string(), Some(num(a)?), Some(num(b)?))
            }
            [a, op1, tag, op2, b] if is_gt(op1) && is_gt(op2) => {
                (tag.to_string(), Some(num(b)?), Some(num(a)?))
            }
            [a, op, tag] if is_number(a) => {
                if is_lt(op) {
                    (tag.to_string(), Some(num(a)?), None)
                } else {
                    (tag.to_string(), None, Some(num(a)?))
                }
            }
            [tag, op, b] => {
                if is_lt(op) {
                    (tag.to_string(), None, Some(num(b)?))
                } else if is_gt(op) {
                    (tag.to_string(), Some(num(b)?), None)
                } else {
                    let v = num(b)?;
                    (tag.to_string(), Some(v), Some(v))
                }
            }
            _ => {
                return Err(format!(
                    "\"{}\" isn't a condition like 600 < wght < 900",
                    part.trim()
                ))
            }
        };
        let a = axis(&tag)?;
        out.push((tag, min.unwrap_or(a.min), max.unwrap_or(a.max)));
    }
    Ok(out)
}

fn split_ops(t: &str) -> Vec<&str> {
    // "600<wght<900" → "600", "<", "wght", "<", "900"
    let mut out = Vec::new();
    let mut start = 0;
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' || b[i] == b'>' || b[i] == b'=' {
            if start < i {
                out.push(&t[start..i]);
            }
            let mut e = i + 1;
            if e < b.len() && b[e] == b'=' {
                e += 1;
            }
            out.push(&t[i..e]);
            start = e;
            i = e;
        } else {
            i += 1;
        }
    }
    if start < t.len() {
        out.push(&t[start..]);
    }
    out
}

fn is_lt(s: &str) -> bool {
    s == "<" || s == "<="
}
fn is_gt(s: &str) -> bool {
    s == ">" || s == ">="
}

/// The user-space location of a static build.
fn user_location(font: &Font, ctx: &Context) -> Location {
    let design = ctx
        .location
        .cloned()
        .unwrap_or_else(|| default_master(font).location.clone());
    font.axes
        .iter()
        .map(|a| {
            let d = design
                .get(&a.tag)
                .copied()
                .unwrap_or_else(|| crate::designspace::design_bounds(a).1);
            (a.tag.clone(), design_to_user(a, d))
        })
        .collect()
}

fn conditions(font: &Font, text: &str, ctx: &Context) -> Result<String, (usize, String)> {
    if !text.contains("condition") {
        return Ok(text.to_string());
    }
    let user = user_location(font, ctx);
    let mut out = String::with_capacity(text.len());
    // Inside a feature block: its tag, and (for variable fonts) the conditional parts.
    let mut feature: Option<String> = None;
    let mut active: Option<Vec<Range>> = None;
    let mut variations: Vec<(String, Vec<Range>, String)> = Vec::new();
    let mut pending: Vec<(String, Vec<Range>, String)> = Vec::new();
    for (offset, line) in line_offsets(text) {
        let code = line[..line.find('#').unwrap_or(line.len())].trim();
        let nl = if line.ends_with('\n') { "\n" } else { "" };
        let words: Vec<&str> = code.split_whitespace().collect();
        if feature.is_none() && words.first() == Some(&"feature") && code.contains('{') {
            feature = words.get(1).map(|t| t.trim_end_matches('{').to_string());
            active = None;
            out.push_str(line);
            continue;
        }
        if let Some(tag) = &feature {
            let closes = code.starts_with('}')
                && code
                    .trim_start_matches('}')
                    .trim()
                    .trim_end_matches(';')
                    .trim()
                    == tag.as_str();
            if closes {
                out.push_str(line);
                variations.append(&mut pending);
                feature = None;
                active = None;
                continue;
            }
            if words.first() == Some(&"condition") {
                let body = code
                    .strip_prefix("condition")
                    .unwrap_or("")
                    .trim()
                    .trim_end_matches(';');
                let ranges = parse_condition(font, body).map_err(|e| (offset, e))?;
                if ctx.variable {
                    pending.push((tag.clone(), ranges.clone(), String::new()));
                }
                active = Some(ranges);
                out.push_str(nl);
                continue;
            }
            if let Some(ranges) = &active {
                if ctx.variable {
                    if let Some(p) = pending.last_mut() {
                        p.2.push_str(line);
                    }
                    out.push_str(nl);
                } else {
                    let inside = ranges.iter().all(|(t, lo, hi)| {
                        user.get(t)
                            .is_some_and(|v| *v >= *lo - 1e-9 && *v <= *hi + 1e-9)
                    });
                    out.push_str(if inside { line } else { nl });
                }
                continue;
            }
        }
        out.push_str(line);
    }
    for (i, (tag, ranges, body)) in variations.iter().enumerate() {
        let name = format!("kernest_condition_{}", i + 1);
        let conds: Vec<String> = ranges
            .iter()
            .map(|(t, lo, hi)| format!("    {t} {} {};", fmt_f(*lo), fmt_f(*hi)))
            .collect();
        out.push_str(&format!(
            "\nconditionset {name} {{\n{}\n}} {name};\nvariation {tag} {name} {{\n{}}} {tag};\n",
            conds.join("\n"),
            body
        ));
    }
    Ok(out)
}

/// What a token stands for, for the feature editor (Alt+click): the glyphs a
/// predicate matches, or a number value in every master.
pub fn describe(font: &Font, token: &str) -> R<String> {
    let token = token.trim();
    if let Some(p) = token.strip_prefix("$[").and_then(|t| t.strip_suffix(']')) {
        let names = predicate_glyphs(font, p)?;
        let shown: Vec<&str> = names.iter().take(40).map(String::as_str).collect();
        return Ok(format!(
            "{} glyph{}: {}{}",
            names.len(),
            if names.len() == 1 { "" } else { "s" },
            shown.join(" "),
            if names.len() > 40 { " …" } else { "" }
        ));
    }
    let ctx = Context::DEFAULT;
    let numbers = Numbers::new(font, &ctx)?;
    let values = if let Some(e) = token.strip_prefix("${").and_then(|t| t.strip_suffix('}')) {
        eval_expr(e, &numbers)?
    } else if let Some(n) = token.strip_prefix('$') {
        numbers.get(n)?
    } else {
        return Err(
            "Alt+click a token such as $padding, ${padding*2}, ${period:LSB} or $[case == upper]."
                .into(),
        );
    };
    let parts: Vec<String> = font
        .masters
        .iter()
        .zip(&values)
        .map(|(m, v)| format!("{} ({})", fmt_f(*v), m.name))
        .collect();
    Ok(format!("{token} = {}", parts.join(" · ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    fn two_masters() -> Font {
        let mut f = sample_font();
        f.axes.push(crate::model::Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 100.0,
            default: 100.0,
            max: 900.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        });
        f.masters[0].location.insert("wght".into(), 100.0);
        let mut bold = f.masters[0].clone();
        bold.id = "m02".into();
        bold.name = "Bold".into();
        bold.location.insert("wght".into(), 900.0);
        f.masters.push(bold);
        for g in &mut f.glyphs {
            let mut l = g.layers[0].clone();
            l.master_id = "m02".into();
            g.layers.push(l);
        }
        f.info.numbers.insert(
            "padding".into(),
            BTreeMap::from([("m01".into(), 24.0), ("m02".into(), 48.0)]),
        );
        f
    }

    #[test]
    fn number_values_become_numbers_or_variable_values() {
        let f = two_masters();
        let code = "pos A <$padding 0 ${padding*2} 0>;";
        let s = expand(&f, code, &Context::DEFAULT).unwrap();
        assert_eq!(s, "pos A <24 0 48 0>;");
        let mid = Location::from([("wght".into(), 500.0)]);
        let s = expand(
            &f,
            code,
            &Context {
                variable: false,
                location: Some(&mid),
            },
        )
        .unwrap();
        assert_eq!(s, "pos A <36 0 72 0>;");
        let v = expand(
            &f,
            code,
            &Context {
                variable: true,
                location: None,
            },
        )
        .unwrap();
        assert_eq!(
            v,
            "pos A <(wght=100d:24 wght=900d:48) 0 (wght=100d:48 wght=900d:96) 0>;"
        );
        let e = expand(&f, "\n pos A $nothing;", &Context::DEFAULT).unwrap_err();
        assert_eq!((e.line, e.column), (2, 8));
    }

    #[test]
    fn optional_closing_lookup_name() {
        let code = "lookup TEST {\n  sub x by y; # } ;\n} ;\nfeature calt {\n  sub period x' lookup TEST;\n} calt;\n";
        let out = lookup_syntax(code);
        assert_eq!(
            out,
            "lookup TEST {\n  sub x by y; # } ;\n} TEST;\nfeature calt {\n  sub period x' lookup TEST;\n} calt;\n"
        );
        // Named closings and `} ;` outside a lookup are left alone.
        let named = "lookup A { sub a by b; } A;\nfeature liga { sub f i by f_i; } liga;\n} ;\n";
        assert_eq!(lookup_syntax(named), named);
    }

    #[test]
    fn standalone_lookups_leave_the_feature_and_keep_their_lines() {
        let code = "feature ccmp {\n  script latn;\n  lookupflag IgnoreMarks;\n  standalone lookup demo {\n    sub acutecomb by acutecomb.case;\n  } demo;\n  sub A acutecomb' lookup demo;\n} ccmp;\n";
        let out = lookup_syntax(code);
        assert_eq!(
            out,
            "feature ccmp {\n  script latn;\n  lookupflag IgnoreMarks;\n  } ccmp; lookup demo {\n    sub acutecomb by acutecomb.case;\n  } demo; feature ccmp { script latn; lookupflag IgnoreMarks;\n  sub A acutecomb' lookup demo;\n} ccmp;\n"
        );
        assert_eq!(out.lines().count(), code.lines().count());
        // Two standalone lookups, one with `} ;`.
        let two = "feature calt {\nstandalone lookup a { sub a by b; } ;\nstandalone lookup c { sub c by d; } c;\nsub x a' lookup a c' lookup c;\n} calt;";
        let out = lookup_syntax(two);
        assert_eq!(
            out,
            "feature calt {\n} calt; lookup a { sub a by b; } a; feature calt {\n} calt; lookup c { sub c by d; } c; feature calt {\nsub x a' lookup a c' lookup c;\n} calt;"
        );
        // Outside a feature, standalone is just dropped.
        assert_eq!(
            lookup_syntax("standalone lookup z { sub z by y; } z;"),
            " lookup z { sub z by y; } z;"
        );
    }

    /// Two masters where H's Bold layer is 40 units wider on the right and has its
    /// top anchor 30 units further right.
    fn property_font() -> Font {
        let mut f = two_masters();
        let h = f.glyph_mut("H").unwrap();
        h.layers[0].anchors = vec![crate::model::Anchor {
            name: "top".into(),
            x: 300.0,
            y: 700.0,
        }];
        let mut a = h.layers[0].anchors.clone();
        a[0].x = 330.0;
        h.layers[1].anchors = a;
        h.layers[1].width += 40.0;
        f
    }

    #[test]
    fn glyph_property_tokens_use_each_masters_values() {
        let f = property_font();
        let h = f.glyph("H").unwrap();
        let (lsb, rsb) = crate::geometry::sidebearings(&f, &h.layers[0]).unwrap();
        let w = h.layers[0].width;
        // Static, default master: plain numbers, arithmetic included.
        let code = "pos A <${H:LSB} 0 ${H:LSB*2} 0>; pos B ${H:RSB}; pos C ${H:width/2+10}; pos D ${H:anchors.top.x};";
        let s = expand(&f, code, &Context::DEFAULT).unwrap();
        assert_eq!(
            s,
            format!(
                "pos A <{} 0 {} 0>; pos B {}; pos C {}; pos D 300;",
                fmt(lsb),
                fmt(lsb * 2.0),
                fmt(rsb),
                fmt(w / 2.0 + 10.0)
            )
        );
        // An instance halfway: interpolated.
        let mid = Location::from([("wght".into(), 500.0)]);
        let s = expand(
            &f,
            "pos D ${H:anchors.top.x}; pos B ${H:RSB};",
            &Context {
                variable: false,
                location: Some(&mid),
            },
        )
        .unwrap();
        assert_eq!(s, format!("pos D 315; pos B {};", fmt(rsb + 20.0)));
        // Variable: a varying value where masters differ, a number where they don't.
        let v = expand(
            &f,
            "pos A ${H:LSB}; pos D ${H:anchors.top.x};",
            &Context {
                variable: true,
                location: None,
            },
        )
        .unwrap();
        assert_eq!(
            v,
            format!("pos A {}; pos D (wght=100d:300 wght=900d:330);", fmt(lsb))
        );
        // Glyph names with dots and hyphens.
        let mut g = f.clone();
        let mut alt = g.glyph("H").unwrap().clone();
        alt.name = "H.ss01-x".into();
        alt.unicodes.clear();
        g.glyphs.push(alt);
        let s = expand(
            &g,
            "pos A ${H.ss01-x:anchors.top.y - 100};",
            &Context::DEFAULT,
        )
        .unwrap();
        assert_eq!(s, "pos A 600;");
    }

    #[test]
    fn tsb_and_bsb_tokens_use_vertical_metrics() {
        let mut f = property_font();
        let m = f.masters[0].clone();
        let l = f.glyph("H").unwrap().layers[0].clone();
        let (tsb, bsb) = crate::vertical::sidebearings(&f, &m, &l).unwrap();
        let s = expand(&f, "pos A ${H:TSB}; pos B ${H:BSB};", &Context::DEFAULT).unwrap();
        assert_eq!(s, format!("pos A {}; pos B {};", fmt(tsb), fmt(bsb)));
        // A vertical origin 50 below the ascender: TSB shrinks by 50.
        f.glyph_mut("H").unwrap().layers[0].vert_origin = Some(50.0);
        let s = expand(&f, "pos A ${H:TSB};", &Context::DEFAULT).unwrap();
        assert_eq!(s, format!("pos A {};", fmt(tsb - 50.0)));
    }

    #[test]
    fn glyph_property_errors_point_at_the_token() {
        let f = property_font();
        let check = |code: &str, msg: &str| {
            let e = expand(&f, code, &Context::DEFAULT).unwrap_err();
            assert!(e.message.contains(msg), "{} / {msg}", e.message);
            assert_eq!(e.line, 2, "{code}");
        };
        check("\npos A ${nosuch:LSB};", "unknown glyph \"nosuch\"");
        check("\npos A ${H:anchors.nope.x};", "has no anchor \"nope\"");
        check("\npos A ${space:TSB};", "no outline");
        check("\npos A ${H:height};", "isn't a glyph property");
        check("\npos A ${space:LSB};", "no outline");
        // Alt+click shows the value per master.
        let d = describe(&f, "${H:anchors.top.x}").unwrap();
        assert_eq!(d, "${H:anchors.top.x} = 300 (Regular) · 330 (Bold)");
    }

    #[test]
    fn predicates_pick_glyphs() {
        let mut f = two_masters();
        let mut sc = Glyph::new("a.sc", Vec::new());
        sc.layers = f.glyph("a").unwrap().layers.clone();
        f.glyphs.push(sc);
        assert_eq!(
            predicate_glyphs(&f, "case == smallCaps").unwrap(),
            vec!["a.sc"]
        );
        assert_eq!(
            predicate_glyphs(&f, "name endswith \".sc\"").unwrap(),
            vec!["a.sc"]
        );
        let upper =
            predicate_glyphs(&f, "case == upper AND name IN {\"A\", \"B\", \"a\"}").unwrap();
        assert_eq!(upper, vec!["A", "B"]);
        let not_letters = predicate_glyphs(&f, "NOT category == \"Letter\"").unwrap();
        assert!(!not_letters.contains(&"A".to_string()));
        assert!(predicate_glyphs(&f, "name LIKE \"?\"")
            .unwrap()
            .contains(&"Z".to_string()));
        let s = expand(&f, "@SC = [$[case == smallCaps]];", &Context::DEFAULT).unwrap();
        assert_eq!(s, "@SC = [a.sc];");
        assert!(predicate_glyphs(&f, "case ==").is_err());
    }

    #[test]
    fn ifdef_variable_keeps_lines() {
        let f = two_masters();
        let code = "a;\n#ifdef VARIABLE\nb;\n#else\nc;\n#endif\nd;";
        assert_eq!(
            expand(&f, code, &Context::DEFAULT).unwrap(),
            "a;\n\n\n\nc;\n\nd;"
        );
        let v = expand(
            &f,
            code,
            &Context {
                variable: true,
                location: None,
            },
        )
        .unwrap();
        assert_eq!(v, "a;\n\nb;\n\n\n\nd;");
        assert!(expand(&f, "#ifdef VARIABLE\nb;", &Context::DEFAULT).is_err());
    }

    #[test]
    fn conditions_split_static_and_make_variations() {
        let f = two_masters();
        let code =
            "feature rvrn {\nsub a by b;\ncondition 600 < wght < 900;\nsub A by B;\n} rvrn;\n";
        let light = expand(&f, code, &Context::DEFAULT).unwrap();
        assert_eq!(light, "feature rvrn {\nsub a by b;\n\n\n} rvrn;\n");
        let bold = Location::from([("wght".into(), 900.0)]);
        let heavy = expand(
            &f,
            code,
            &Context {
                variable: false,
                location: Some(&bold),
            },
        )
        .unwrap();
        assert!(heavy.contains("sub A by B;"));
        let v = expand(
            &f,
            code,
            &Context {
                variable: true,
                location: None,
            },
        )
        .unwrap();
        assert!(
            v.contains(
                "conditionset kernest_condition_1 {\n    wght 600 900;\n} kernest_condition_1;"
            ),
            "{v}"
        );
        assert!(
            v.contains("variation rvrn kernest_condition_1 {\nsub A by B;\n} rvrn;"),
            "{v}"
        );
        assert!(expand(
            &f,
            "feature x {\ncondition 1 < zzzz;\n} x;",
            &Context::DEFAULT
        )
        .is_err());
    }

    #[test]
    fn glyphs_varying_values_and_statement_fixes() {
        let f = two_masters();
        let v = expand(
            &f,
            "pos A 10 (wght:900) 30;",
            &Context {
                variable: true,
                location: None,
            },
        )
        .unwrap();
        assert_eq!(v, "pos A (10 wght=900d:30);");
        let mid = Location::from([("wght".into(), 500.0)]);
        let s = expand(
            &f,
            "pos A 10 (wght:900) 30;",
            &Context {
                variable: false,
                location: Some(&mid),
            },
        )
        .unwrap();
        assert_eq!(s, "pos A 20;");
        assert_eq!(
            expand(&f, "  lookupflag;\n  delete a' b;\n", &Context::DEFAULT).unwrap(),
            "  lookupflag 0;\n  sub a' b by NULL;\n"
        );
    }

    #[test]
    fn kerning_group_classes_are_defined() {
        let mut f = two_masters();
        for n in ["T", "Tcaron"] {
            if let Some(g) = f.glyph_mut(n) {
                g.right_group = Some("T".into());
            }
        }
        f.glyph_mut("o").unwrap().left_group = Some("o".into());
        let s = expand(
            &f,
            "feature kern {\n  pos @MMK_L_T' -50 @MMK_R_o;\n} kern;",
            &Context::DEFAULT,
        )
        .unwrap();
        assert!(s.starts_with("@MMK_L_T = [T"), "{s}");
        assert!(s.contains("@MMK_R_o = [o]; feature kern {\n"), "{s}");
        assert_eq!(s.lines().count(), 3);
    }

    #[test]
    fn describe_explains_tokens() {
        let f = two_masters();
        assert_eq!(
            describe(&f, "${padding*2}").unwrap(),
            "${padding*2} = 48 (Regular) · 96 (Bold)"
        );
        assert!(describe(&f, "$[name == \"A\"]")
            .unwrap()
            .starts_with("1 glyph: A"));
    }
}
