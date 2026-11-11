//! Compiled world-space scalar field expressions `f(x, y, z, r)` for spatially varying
//! boundary conditions (`pressure_fn`, `temp_fn`, `h_fn`, `tinf_fn`, `q_fn`).
//!
//! Coordinates are evaluated in world millimetres (`x`, `y`, `z`, and cylindrical radius
//! `r = hypot(x, y)`), with document `let` variables in scope. Supports:
//! - arithmetic (`+`, `-`, `*`, `/`, `^`), scientific notation (`1.5e-4`), `pi`, `e`;
//! - standard math functions (`sin`, `cos`, `tan`, `exp`, `ln`, `sqrt`, `abs`, `min`, `max`,
//!   `pow`, `clamp(val, lo, hi)`);
//! - clamped piecewise-linear 1D table lookup `lerp(coord, s0, v0, s1, v1, ...)` with
//!   strictly ascending constant knot stations `s0 < s1 < ... < sm`;
//! - non-overlapping piecewise bands `lo < coord < hi -> body` separated by `;` (all bands
//!   in one expression share one coordinate `x`, `y`, `z`, or `r`).

use std::f64::consts::{E as EULER, PI};

/// World-space coordinate referenced by a [`SpatialExpr`] or band header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialCoord {
    X,
    Y,
    Z,
    R,
}

impl SpatialCoord {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "x" => Some(Self::X),
            "y" => Some(Self::Y),
            "z" => Some(Self::Z),
            "r" => Some(Self::R),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
            Self::R => "r",
        }
    }

    #[inline]
    pub fn select(self, p: [f64; 3]) -> f64 {
        match self {
            Self::X => p[0],
            Self::Y => p[1],
            Self::Z => p[2],
            Self::R => p[0].hypot(p[1]),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialFnName {
    Sin,
    Cos,
    Tan,
    Exp,
    Ln,
    Sqrt,
    Abs,
    Min,
    Max,
    Pow,
    Clamp,
    Lerp,
}

impl SpatialFnName {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "tan" => Self::Tan,
            "exp" => Self::Exp,
            "ln" => Self::Ln,
            "sqrt" => Self::Sqrt,
            "abs" => Self::Abs,
            "min" => Self::Min,
            "max" => Self::Max,
            "pow" => Self::Pow,
            "clamp" => Self::Clamp,
            "lerp" => Self::Lerp,
            _ => return None,
        })
    }

    fn fixed_arity(self) -> Option<usize> {
        match self {
            Self::Sin | Self::Cos | Self::Tan | Self::Exp | Self::Ln | Self::Sqrt | Self::Abs => {
                Some(1)
            }
            Self::Min | Self::Max | Self::Pow => Some(2),
            Self::Clamp => Some(3),
            Self::Lerp => None,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Exp => "exp",
            Self::Ln => "ln",
            Self::Sqrt => "sqrt",
            Self::Abs => "abs",
            Self::Min => "min",
            Self::Max => "max",
            Self::Pow => "pow",
            Self::Clamp => "clamp",
            Self::Lerp => "lerp",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpatialExpr {
    Const(f64),
    Var(SpatialCoord),
    Pi,
    E,
    Neg(Box<SpatialExpr>),
    Sum(Box<SpatialExpr>, Box<SpatialExpr>),
    Difference(Box<SpatialExpr>, Box<SpatialExpr>),
    Product(Box<SpatialExpr>, Box<SpatialExpr>),
    Quotient(Box<SpatialExpr>, Box<SpatialExpr>),
    Power(Box<SpatialExpr>, Box<SpatialExpr>),
    Call(SpatialFnName, Vec<SpatialExpr>),
    Lerp {
        coord: Box<SpatialExpr>,
        /// Strictly ascending constant stations `s_i` paired with value expressions `v_i`.
        knots: Vec<(f64, SpatialExpr)>,
    },
}

impl SpatialExpr {
    pub fn eval(&self, p: [f64; 3]) -> f64 {
        match self {
            Self::Const(v) => *v,
            Self::Var(c) => c.select(p),
            Self::Pi => PI,
            Self::E => EULER,
            Self::Neg(a) => -a.eval(p),
            Self::Sum(a, b) => a.eval(p) + b.eval(p),
            Self::Difference(a, b) => a.eval(p) - b.eval(p),
            Self::Product(a, b) => a.eval(p) * b.eval(p),
            Self::Quotient(a, b) => a.eval(p) / b.eval(p),
            Self::Power(a, b) => a.eval(p).powf(b.eval(p)),
            Self::Call(name, args) => {
                let a = |i: usize| args[i].eval(p);
                match name {
                    SpatialFnName::Sin => a(0).sin(),
                    SpatialFnName::Cos => a(0).cos(),
                    SpatialFnName::Tan => a(0).tan(),
                    SpatialFnName::Exp => a(0).exp(),
                    SpatialFnName::Ln => a(0).ln(),
                    SpatialFnName::Sqrt => a(0).sqrt(),
                    SpatialFnName::Abs => a(0).abs(),
                    SpatialFnName::Min => a(0).min(a(1)),
                    SpatialFnName::Max => a(0).max(a(1)),
                    SpatialFnName::Pow => a(0).powf(a(1)),
                    SpatialFnName::Clamp => a(0).clamp(a(1), a(2)),
                    SpatialFnName::Lerp => unreachable!("Lerp is lowered to SpatialExpr::Lerp"),
                }
            }
            Self::Lerp { coord, knots } => {
                let u = coord.eval(p);
                if !u.is_finite() {
                    return f64::NAN;
                }
                let first = &knots[0];
                if u <= first.0 {
                    return first.1.eval(p);
                }
                let last = &knots[knots.len() - 1];
                if u >= last.0 {
                    return last.1.eval(p);
                }
                let idx = knots
                    .partition_point(|(s, _)| *s <= u)
                    .saturating_sub(1)
                    .min(knots.len() - 2);
                let (s0, ref e0) = knots[idx];
                let (s1, ref e1) = knots[idx + 1];
                let v0 = e0.eval(p);
                let v1 = e1.eval(p);
                let t = (u - s0) / (s1 - s0);
                v0 + t * (v1 - v0)
            }
        }
    }

    pub fn fold(&self) -> Option<f64> {
        match self {
            Self::Const(v) => Some(*v),
            Self::Var(_) => None,
            Self::Pi => Some(PI),
            Self::E => Some(EULER),
            Self::Neg(a) => Some(-a.fold()?),
            Self::Sum(a, b) => Some(a.fold()? + b.fold()?),
            Self::Difference(a, b) => Some(a.fold()? - b.fold()?),
            Self::Product(a, b) => Some(a.fold()? * b.fold()?),
            Self::Quotient(a, b) => Some(a.fold()? / b.fold()?),
            Self::Power(a, b) => Some(a.fold()?.powf(b.fold()?)),
            Self::Call(name, args) => {
                let g = args.iter().map(Self::fold).collect::<Option<Vec<f64>>>()?;
                Some(match name {
                    SpatialFnName::Sin => g[0].sin(),
                    SpatialFnName::Cos => g[0].cos(),
                    SpatialFnName::Tan => g[0].tan(),
                    SpatialFnName::Exp => g[0].exp(),
                    SpatialFnName::Ln => g[0].ln(),
                    SpatialFnName::Sqrt => g[0].sqrt(),
                    SpatialFnName::Abs => g[0].abs(),
                    SpatialFnName::Min => g[0].min(g[1]),
                    SpatialFnName::Max => g[0].max(g[1]),
                    SpatialFnName::Pow => g[0].powf(g[1]),
                    SpatialFnName::Clamp => g[0].clamp(g[1], g[2]),
                    SpatialFnName::Lerp => unreachable!("Lerp is lowered to SpatialExpr::Lerp"),
                })
            }
            Self::Lerp { coord, knots } => {
                let u = coord.fold()?;
                let vals = knots
                    .iter()
                    .map(|(s, e)| Some((*s, e.fold()?)))
                    .collect::<Option<Vec<(f64, f64)>>>()?;
                if u <= vals[0].0 {
                    return Some(vals[0].1);
                }
                let last = vals[vals.len() - 1];
                if u >= last.0 {
                    return Some(last.1);
                }
                let idx = vals
                    .partition_point(|(s, _)| *s <= u)
                    .saturating_sub(1)
                    .min(vals.len() - 2);
                let (s0, v0) = vals[idx];
                let (s1, v1) = vals[idx + 1];
                Some(v0 + (u - s0) / (s1 - s0) * (v1 - v0))
            }
        }
    }

    fn first_coord(&self) -> Option<SpatialCoord> {
        match self {
            Self::Var(c) => Some(*c),
            Self::Const(_) | Self::Pi | Self::E => None,
            Self::Neg(a) => a.first_coord(),
            Self::Sum(a, b)
            | Self::Difference(a, b)
            | Self::Product(a, b)
            | Self::Quotient(a, b)
            | Self::Power(a, b) => a.first_coord().or_else(|| b.first_coord()),
            Self::Call(_, args) => args.iter().find_map(Self::first_coord),
            Self::Lerp { coord, knots } => coord
                .first_coord()
                .or_else(|| knots.iter().find_map(|(_, e)| e.first_coord())),
        }
    }

    fn lerp_span(&self) -> Option<(f64, f64)> {
        match self {
            Self::Lerp { knots, .. } => Some((knots[0].0, knots[knots.len() - 1].0)),
            Self::Neg(a) => a.lerp_span(),
            Self::Sum(a, b)
            | Self::Difference(a, b)
            | Self::Product(a, b)
            | Self::Quotient(a, b)
            | Self::Power(a, b) => a.lerp_span().or_else(|| b.lerp_span()),
            Self::Call(_, args) => args.iter().find_map(Self::lerp_span),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpatialBand {
    pub coord: Option<SpatialCoord>,
    pub lo: f64,
    pub hi: f64,
    pub body: SpatialExpr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpatialFn {
    pub source: String,
    bands: Vec<SpatialBand>,
}

impl SpatialFn {
    pub fn compile(src: &str) -> Result<Self, String> {
        Self::compile_with_lets(src, &[])
    }

    pub fn compile_with_lets(src: &str, lets: &[(String, f64)]) -> Result<Self, String> {
        let toks = tokenize(src, lets)?;
        let mut bands = Vec::new();
        let mut start = 0usize;
        for cut in 0..=toks.len() {
            if cut == toks.len() || toks[cut].tok == Tok::Semi {
                bands.push(parse_piece(&toks[start..cut], src.chars().count())?);
                start = cut + 1;
            }
        }
        if bands.len() > 1 {
            let first_coord = bands[0].coord.ok_or_else(|| {
                "multiple ';' bands require 'lo < coord < hi -> expr' headers".to_string()
            })?;
            for b in &bands[1..] {
                let Some(c) = b.coord else {
                    return Err(
                        "multiple ';' bands require 'lo < coord < hi -> expr' headers".to_string(),
                    );
                };
                if c != first_coord {
                    return Err(format!(
                        "all bands in one expression must slice the same coordinate ('{}' vs '{}')",
                        first_coord.name(),
                        c.name()
                    ));
                }
            }
        }
        for w in bands.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if b.lo < a.hi {
                return Err(format!(
                    "bands overlap: [{}, {}] runs into [{}, {}]",
                    a.lo, a.hi, b.lo, b.hi
                ));
            }
        }
        Ok(Self {
            source: src.to_string(),
            bands,
        })
    }

    pub fn bands(&self) -> &[SpatialBand] {
        &self.bands
    }

    /// Primary coordinate axis used by this spatial expression (band axis if banded, else
    /// first referenced coordinate, defaulting to `Z`).
    pub fn primary_coord(&self) -> SpatialCoord {
        if let Some(c) = self.bands.first().and_then(|b| b.coord) {
            return c;
        }
        self.bands
            .iter()
            .find_map(|b| b.body.first_coord())
            .unwrap_or(SpatialCoord::Z)
    }

    /// Natural preview domain `(lo, hi)` when the expression declares explicit bands or a
    /// `lerp(...)` knot table.
    pub fn domain(&self) -> Option<(f64, f64)> {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for b in &self.bands {
            lo = lo.min(b.lo);
            hi = hi.max(b.hi);
        }
        if lo.is_finite() && hi.is_finite() {
            return Some((lo, hi));
        }
        self.bands.iter().find_map(|b| b.body.lerp_span())
    }

    #[inline]
    pub fn eval(&self, p: [f64; 3]) -> f64 {
        for b in &self.bands {
            match b.coord {
                None => return b.body.eval(p),
                Some(c) => {
                    let u = c.select(p);
                    if b.lo <= u && u <= b.hi {
                        return b.body.eval(p);
                    }
                }
            }
        }
        0.0
    }

    /// Build a compact, human-readable `lerp(axis, s0, v0, s1, v1, ...)` expression from
    /// strictly increasing `(station_mm, value)` pairs.
    pub fn format_lerp(coord: SpatialCoord, samples: &[(f64, f64)]) -> Option<String> {
        Self::format_lerp_with(coord, samples, fmt_num)
    }

    /// Build a full IEEE-754 precision `lerp(axis, s0, v0, s1, v1, ...)` expression for
    /// in-memory solver and adjoint/sensitivity evaluation without decimal rounding noise.
    pub fn format_lerp_exact(coord: SpatialCoord, samples: &[(f64, f64)]) -> Option<String> {
        Self::format_lerp_with(coord, samples, |v| {
            if v == 0.0 {
                "0".to_string()
            } else {
                format!("{v}")
            }
        })
    }

    fn format_lerp_with(
        coord: SpatialCoord,
        samples: &[(f64, f64)],
        fmt: fn(f64) -> String,
    ) -> Option<String> {
        if samples.len() < 2 {
            return None;
        }
        let mut parts = Vec::with_capacity(1 + 2 * samples.len());
        parts.push(coord.name().to_string());
        let mut prev_s = f64::NEG_INFINITY;
        for &(s, v) in samples {
            if !(s.is_finite() && v.is_finite()) {
                return None;
            }
            if s <= prev_s + 1e-9 {
                continue;
            }
            prev_s = s;
            parts.push(fmt(s));
            parts.push(fmt(v));
        }
        if parts.len() < 5 {
            return None;
        }
        Some(format!("lerp({})", parts.join(", ")))
    }

    /// Static finiteness and optional non-negativity checks (`non_negative = true` for
    /// convective film coefficient `h_fn`).
    pub fn static_checks(&self, what: &str, non_negative: bool) -> Vec<String> {
        let mut errs = Vec::new();
        for (i, b) in self.bands.iter().enumerate() {
            if let Some(v) = b.body.fold() {
                if !v.is_finite() {
                    errs.push(format!(
                        "{what}: band {i} is a constant and is not finite ({v}); write a finite value"
                    ));
                } else if non_negative && v < 0.0 {
                    errs.push(format!(
                        "{what}: band {i} folds to negative value ({v}); must be ≥ 0"
                    ));
                }
            }
            check_expr(&b.body, what, i, non_negative, &mut errs);
        }
        errs
    }
}

fn fmt_num(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let av = v.abs();
    if (1e-3..1e5).contains(&av) {
        let s = format!("{v:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s.is_empty() || s == "-0" {
            "0".to_string()
        } else {
            s.to_string()
        }
    } else {
        format!("{v:.5e}")
    }
}

fn check_expr(
    e: &SpatialExpr,
    what: &str,
    band: usize,
    non_negative: bool,
    errs: &mut Vec<String>,
) {
    match e {
        SpatialExpr::Const(_) | SpatialExpr::Var(_) | SpatialExpr::Pi | SpatialExpr::E => {}
        SpatialExpr::Neg(a) => check_expr(a, what, band, false, errs),
        SpatialExpr::Sum(a, b) | SpatialExpr::Difference(a, b) | SpatialExpr::Product(a, b) => {
            check_expr(a, what, band, false, errs);
            check_expr(b, what, band, false, errs);
        }
        SpatialExpr::Quotient(a, b) => {
            if b.fold() == Some(0.0) {
                errs.push(format!("{what}: band {band} divides by a constant zero"));
            }
            check_expr(a, what, band, false, errs);
            check_expr(b, what, band, false, errs);
        }
        SpatialExpr::Power(a, b) => {
            if let (Some(base), Some(p)) = (a.fold(), b.fold()) {
                if base < 0.0 && p.fract() != 0.0 {
                    errs.push(format!(
                        "{what}: band {band} raises a negative constant to a non-integer power"
                    ));
                }
                if base == 0.0 && p < 0.0 {
                    errs.push(format!(
                        "{what}: band {band} raises zero to a negative power"
                    ));
                }
            }
            check_expr(a, what, band, false, errs);
            check_expr(b, what, band, false, errs);
        }
        SpatialExpr::Call(name, args) => {
            if *name == SpatialFnName::Sqrt && args[0].fold().is_some_and(|v| v < 0.0) {
                errs.push(format!(
                    "{what}: band {band} takes the square root of a negative constant"
                ));
            }
            if *name == SpatialFnName::Pow && args.len() == 2 {
                if let (Some(base), Some(p)) = (args[0].fold(), args[1].fold()) {
                    if base < 0.0 && p.fract() != 0.0 {
                        errs.push(format!(
                            "{what}: band {band} raises a negative constant to a non-integer power"
                        ));
                    }
                    if base == 0.0 && p < 0.0 {
                        errs.push(format!(
                            "{what}: band {band} raises zero to a negative power"
                        ));
                    }
                }
            }
            for a in args {
                check_expr(a, what, band, false, errs);
            }
        }
        SpatialExpr::Lerp { coord, knots } => {
            check_expr(coord, what, band, false, errs);
            for (k_idx, (_, ve)) in knots.iter().enumerate() {
                if non_negative {
                    if let Some(v) = ve.fold() {
                        if v < 0.0 {
                            errs.push(format!("{what}: lerp knot {k_idx} value ({v}) must be ≥ 0"));
                        }
                    }
                }
                check_expr(ve, what, band, false, errs);
            }
        }
    }
}

// ---------------------------------------------------------------- tokenizer --

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Var(SpatialCoord),
    Fn(SpatialFnName),
    Pi,
    E,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    Comma,
    Lt,
    Arrow,
    Semi,
}

#[derive(Clone, Copy, Debug)]
struct TokPos {
    tok: Tok,
    pos: usize,
}

fn tokenize(src: &str, lets: &[(String, f64)]) -> Result<Vec<TokPos>, String> {
    let cs: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0usize;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let pos = i;
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            let name: String = cs[start..i].iter().collect();
            let tok = if let Some(coord) = SpatialCoord::parse(&name) {
                Tok::Var(coord)
            } else if let Some(f) = SpatialFnName::parse(&name) {
                Tok::Fn(f)
            } else if let Some((_, v)) = lets.iter().find(|(k, _)| k == &name) {
                Tok::Num(*v)
            } else if name == "pi" {
                Tok::Pi
            } else if name == "e" {
                Tok::E
            } else {
                return Err(format!(
                    "at character {start}: unknown name '{name}' (allowed: x, y, z, r, pi, e, let variables, sin, cos, tan, exp, ln, sqrt, abs, min, max, pow, clamp, lerp)"
                ));
            };
            toks.push(TokPos { tok, pos: start });
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') {
                let mut exp_i = i + 1;
                if exp_i < cs.len() && (cs[exp_i] == '+' || cs[exp_i] == '-') {
                    exp_i += 1;
                }
                if exp_i < cs.len() && cs[exp_i].is_ascii_digit() {
                    i = exp_i;
                    while i < cs.len() && cs[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = cs[start..i].iter().collect();
            let v: f64 = text
                .parse()
                .map_err(|_| format!("at character {start}: '{text}' is not a number"))?;
            toks.push(TokPos {
                tok: Tok::Num(v),
                pos: start,
            });
            continue;
        }
        let (tok, len) = match c {
            '+' => (Tok::Plus, 1),
            '-' => {
                if cs.get(i + 1) == Some(&'>') {
                    (Tok::Arrow, 2)
                } else {
                    (Tok::Minus, 1)
                }
            }
            '*' => (Tok::Star, 1),
            '/' => (Tok::Slash, 1),
            '^' => (Tok::Caret, 1),
            '(' => (Tok::LParen, 1),
            ')' => (Tok::RParen, 1),
            ',' => (Tok::Comma, 1),
            '<' => (Tok::Lt, 1),
            ';' => (Tok::Semi, 1),
            other => {
                return Err(format!(
                    "at character {pos}: unexpected character '{other}'"
                ))
            }
        };
        toks.push(TokPos { tok, pos });
        i += len;
    }
    Ok(toks)
}

fn describe(t: Tok) -> &'static str {
    match t {
        Tok::Num(_) => "a number",
        Tok::Var(_) => "a coordinate (x, y, z, or r)",
        Tok::Fn(_) => "a function name",
        Tok::Pi => "pi",
        Tok::E => "e",
        Tok::Plus => "'+'",
        Tok::Minus => "'-'",
        Tok::Star => "'*'",
        Tok::Slash => "'/'",
        Tok::Caret => "'^'",
        Tok::LParen => "'('",
        Tok::RParen => "')'",
        Tok::Comma => "','",
        Tok::Lt => "'<'",
        Tok::Arrow => "'->'",
        Tok::Semi => "';'",
    }
}

struct P {
    toks: Vec<TokPos>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<Tok> {
        self.toks.get(self.i).map(|t| t.tok)
    }

    fn eat(&mut self, t: Tok) -> bool {
        if self.peek() == Some(t) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: Tok, what: &str) -> Result<(), String> {
        match self.peek() {
            Some(got) if got == t => {
                self.i += 1;
                Ok(())
            }
            Some(got) => Err(self.err(&format!("expected {what}, found {}", describe(got)))),
            None => Err(self.err(&format!("expected {what}"))),
        }
    }

    fn err(&self, msg: &str) -> String {
        match self.toks.get(self.i) {
            Some(t) => format!("at character {}: {msg}", t.pos),
            None => format!("at end of input: {msg}"),
        }
    }

    fn expr(&mut self) -> Result<SpatialExpr, String> {
        let mut a = self.term()?;
        loop {
            let minus = self.eat(Tok::Minus);
            if !minus && !self.eat(Tok::Plus) {
                break;
            }
            let b = self.term()?;
            a = if minus {
                SpatialExpr::Difference(Box::new(a), Box::new(b))
            } else {
                SpatialExpr::Sum(Box::new(a), Box::new(b))
            };
        }
        Ok(a)
    }

    fn term(&mut self) -> Result<SpatialExpr, String> {
        let mut a = self.unary()?;
        loop {
            let slash = self.eat(Tok::Slash);
            if !slash && !self.eat(Tok::Star) {
                break;
            }
            let b = self.unary()?;
            a = if slash {
                SpatialExpr::Quotient(Box::new(a), Box::new(b))
            } else {
                SpatialExpr::Product(Box::new(a), Box::new(b))
            };
        }
        Ok(a)
    }

    fn unary(&mut self) -> Result<SpatialExpr, String> {
        if self.eat(Tok::Minus) {
            Ok(SpatialExpr::Neg(Box::new(self.unary()?)))
        } else {
            self.power()
        }
    }

    fn power(&mut self) -> Result<SpatialExpr, String> {
        let a = self.atom()?;
        if self.eat(Tok::Caret) {
            let b = self.unary()?;
            Ok(SpatialExpr::Power(Box::new(a), Box::new(b)))
        } else {
            Ok(a)
        }
    }

    fn atom(&mut self) -> Result<SpatialExpr, String> {
        let (tok, pos) = match self.toks.get(self.i) {
            Some(t) => (t.tok, t.pos),
            None => {
                return Err("at end of input: expected a number, x/y/z/r, or '('.".into());
            }
        };
        match tok {
            Tok::Num(v) => {
                self.i += 1;
                Ok(SpatialExpr::Const(v))
            }
            Tok::Var(c) => {
                self.i += 1;
                Ok(SpatialExpr::Var(c))
            }
            Tok::Pi => {
                self.i += 1;
                Ok(SpatialExpr::Pi)
            }
            Tok::E => {
                self.i += 1;
                Ok(SpatialExpr::E)
            }
            Tok::LParen => {
                self.i += 1;
                let a = self.expr()?;
                self.expect(Tok::RParen, "')'")?;
                Ok(a)
            }
            Tok::Fn(f) => {
                self.i += 1;
                self.expect(Tok::LParen, "a '(' after a function name")?;
                let mut args = vec![self.expr()?];
                while self.eat(Tok::Comma) {
                    args.push(self.expr()?);
                }
                self.expect(Tok::RParen, "')' to close the argument list")?;
                if f == SpatialFnName::Lerp {
                    if args.len() < 5 || args.len() % 2 == 0 {
                        return Err(format!(
                            "at character {pos}: lerp takes (coord, s0, v0, s1, v1, ...) — odd argument count ≥ 5, given {}",
                            args.len()
                        ));
                    }
                    let mut it = args.into_iter();
                    let coord = Box::new(it.next().unwrap());
                    let mut knots = Vec::new();
                    while let (Some(se), Some(ve)) = (it.next(), it.next()) {
                        let s = se.fold().filter(|v| v.is_finite()).ok_or_else(|| {
                            format!(
                                "at character {pos}: lerp station positions s_i must be finite constants"
                            )
                        })?;
                        if let Some((prev_s, _)) = knots.last() {
                            if s <= *prev_s {
                                return Err(format!(
                                    "at character {pos}: lerp stations must strictly ascend ({prev_s} >= {s})"
                                ));
                            }
                        }
                        knots.push((s, ve));
                    }
                    return Ok(SpatialExpr::Lerp { coord, knots });
                }
                if let Some(arity) = f.fixed_arity() {
                    if args.len() != arity {
                        let s = if arity == 1 { "" } else { "s" };
                        return Err(format!(
                            "at character {pos}: {} takes {arity} argument{s}, given {}",
                            f.word(),
                            args.len()
                        ));
                    }
                }
                Ok(SpatialExpr::Call(f, args))
            }
            other => Err(self.err(&format!(
                "expected a number, x/y/z/r, or '(', found {}",
                describe(other)
            ))),
        }
    }
}

fn parse_piece(piece: &[TokPos], end_char: usize) -> Result<SpatialBand, String> {
    if piece.is_empty() {
        return Err(format!(
            "at character {end_char}: nothing here (bands are separated by ';')"
        ));
    }
    let arrow = piece.iter().position(|t| t.tok == Tok::Arrow);
    let (header, body_toks) = match arrow {
        Some(at) => (&piece[..at], &piece[at + 1..]),
        None => (&piece[..0], piece),
    };
    let mut p = P {
        toks: body_toks.to_vec(),
        i: 0,
    };
    let body = p.expr()?;
    if p.i != p.toks.len() {
        return Err(p.err("unexpected text after the expression"));
    }
    if arrow.is_none() {
        return Ok(SpatialBand {
            coord: None,
            lo: f64::NEG_INFINITY,
            hi: f64::INFINITY,
            body,
        });
    }
    let mut h = P {
        toks: header.to_vec(),
        i: 0,
    };
    let lo_e = h.expr()?;
    h.expect(Tok::Lt, "'<' in the band header")?;
    let coord = match h.peek() {
        Some(Tok::Var(c)) => {
            h.i += 1;
            c
        }
        _ => return Err(h.err("expected x, y, z, or r in the band header")),
    };
    h.expect(Tok::Lt, "'<' in the band header")?;
    let hi_e = h.expr()?;
    if h.i != h.toks.len() {
        return Err(h.err("trailing text in the band header"));
    }
    let lo = lo_e.fold().filter(|v| v.is_finite()).ok_or_else(|| {
        "the band's lower bound must be a constant number (no coordinates)".to_string()
    })?;
    let hi = hi_e.fold().filter(|v| v.is_finite()).ok_or_else(|| {
        "the band's upper bound must be a constant number (no coordinates)".to_string()
    })?;
    if lo > hi {
        return Err(format!(
            "the band's bounds are reversed ([{lo}, {hi}]: the lower bound is above the upper)"
        ));
    }
    Ok(SpatialBand {
        coord: Some(coord),
        lo,
        hi,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_world_xyzr_scientific_notation_and_lets() {
        let lets = vec![("q_peak".to_string(), 1.5), ("z_th".to_string(), 30.0)];
        let f = SpatialFn::compile_with_lets(
            "2.5e-3 + q_peak * exp(-((z - z_th) / 10)^2) + 0.01 * r",
            &lets,
        )
        .expect("compile");
        let at_throat = f.eval([3.0, 4.0, 30.0]); // r = 5
        assert!((at_throat - (2.5e-3 + 1.5 + 0.05)).abs() < 1e-12);
        assert_eq!(f.primary_coord(), SpatialCoord::Z);
    }

    #[test]
    fn lerp_interpolates_linearly_clamps_tails_and_reports_domain() {
        let src = SpatialFn::format_lerp(
            SpatialCoord::Z,
            &[(4.0, 2.0e-3), (30.0, 6.0e-3), (56.0, 3.0e-3)],
        )
        .expect("format lerp");
        let f = SpatialFn::compile(&src).expect("compile lerp");
        assert_eq!(f.primary_coord(), SpatialCoord::Z);
        assert_eq!(f.domain(), Some((4.0, 56.0)));
        assert!((f.eval([0.0, 0.0, 0.0]) - 2.0e-3).abs() < 1e-12);
        assert!((f.eval([0.0, 0.0, 17.0]) - 4.0e-3).abs() < 1e-12);
        assert!((f.eval([0.0, 0.0, 30.0]) - 6.0e-3).abs() < 1e-12);
        assert!((f.eval([0.0, 0.0, 60.0]) - 3.0e-3).abs() < 1e-12);
    }

    #[test]
    fn piecewise_z_bands_and_non_negative_validation() {
        let f = SpatialFn::compile("0 < z < 30 -> 0.5 + 0.01 * z; 30 < z < 60 -> 0.8").unwrap();
        assert_eq!(f.domain(), Some((0.0, 60.0)));
        assert!((f.eval([0.0, 0.0, 20.0]) - 0.7).abs() < 1e-12);
        assert!((f.eval([0.0, 0.0, 45.0]) - 0.8).abs() < 1e-12);

        let neg_h = SpatialFn::compile("lerp(z, 0, 0.002, 20, -0.001, 40, 0.002)").unwrap();
        let errs = neg_h.static_checks("convection 'c1' h_fn", true);
        assert!(errs.iter().any(|e| e.contains("must be ≥ 0")), "{errs:?}");
    }
}
