//! Compiled load-density expressions over a region's face-local coordinates (M11).
//!
//! The face-local frame is chosen per labelled region at solve time: a least-squares
//! plane through the region's facet centroids gives the normal n; the world axis least
//! aligned with n projects onto the local x axis; y = n × x, so (x, y, n) is
//! right-handed and deterministic except for exact diagonals, where the tie breaks in
//! x, y, z order. Coordinates are millimetres. The fit residual is reported with the
//! load's provenance, and a region too curved for the frame is refused by name (the
//! fit and its bound live in `fieldsmith-core`).
//!
//! A load expression evaluates to force per unit area (N/mm²). Its text form, inside
//! `fn="..."`: either a plain expression body over `x` and `y`, or a chain of bands
//! `lo < x < hi -> body` separated by `;`. A point matches the first band with
//! lo ≤ x ≤ hi, so touching bands are allowed, the earlier band wins on the shared
//! edge, and overlapping bands are a named compile error; outside every band the
//! density is zero. Division by an expression that folds to a constant zero and other
//! constant non-finite results are caught here by `static_checks`; a density that is
//! still not finite at a facet's centroid is caught at solve time, naming the point.

use std::f64::consts::{E as EULER, PI};

/// Functions the expression body may call. Names are recognised in lowercase only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FnName {
    Sin,
    Cos,
    Exp,
    Sqrt,
    Abs,
    Min,
    Max,
    Pow,
}

impl FnName {
    fn parse(name: &str) -> Option<FnName> {
        Some(match name {
            "sin" => FnName::Sin,
            "cos" => FnName::Cos,
            "exp" => FnName::Exp,
            "sqrt" => FnName::Sqrt,
            "abs" => FnName::Abs,
            "min" => FnName::Min,
            "max" => FnName::Max,
            "pow" => FnName::Pow,
            _ => return None,
        })
    }

    fn arity(self) -> usize {
        match self {
            FnName::Sin | FnName::Cos | FnName::Exp | FnName::Sqrt | FnName::Abs => 1,
            FnName::Min | FnName::Max | FnName::Pow => 2,
        }
    }

    fn word(self) -> &'static str {
        match self {
            FnName::Sin => "sin",
            FnName::Cos => "cos",
            FnName::Exp => "exp",
            FnName::Sqrt => "sqrt",
            FnName::Abs => "abs",
            FnName::Min => "min",
            FnName::Max => "max",
            FnName::Pow => "pow",
        }
    }
}

/// Arithmetic over the local coordinates. `Var(true)` is x, `Var(false)` is y.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadExpr {
    Const(f64),
    Var(bool),
    Pi,
    E,
    Neg(Box<LoadExpr>),
    Sum(Box<LoadExpr>, Box<LoadExpr>),
    Difference(Box<LoadExpr>, Box<LoadExpr>),
    Product(Box<LoadExpr>, Box<LoadExpr>),
    Quotient(Box<LoadExpr>, Box<LoadExpr>),
    Power(Box<LoadExpr>, Box<LoadExpr>),
    Call(FnName, Vec<LoadExpr>),
}

impl LoadExpr {
    pub fn eval(&self, x: f64, y: f64) -> f64 {
        match self {
            LoadExpr::Const(v) => *v,
            LoadExpr::Var(is_x) => {
                if *is_x {
                    x
                } else {
                    y
                }
            }
            LoadExpr::Pi => PI,
            LoadExpr::E => EULER,
            LoadExpr::Neg(a) => -a.eval(x, y),
            LoadExpr::Sum(a, b) => a.eval(x, y) + b.eval(x, y),
            LoadExpr::Difference(a, b) => a.eval(x, y) - b.eval(x, y),
            LoadExpr::Product(a, b) => a.eval(x, y) * b.eval(x, y),
            LoadExpr::Quotient(a, b) => a.eval(x, y) / b.eval(x, y),
            LoadExpr::Power(a, b) => a.eval(x, y).powf(b.eval(x, y)),
            LoadExpr::Call(name, args) => {
                let a = |i: usize| args[i].eval(x, y);
                match name {
                    FnName::Sin => a(0).sin(),
                    FnName::Cos => a(0).cos(),
                    FnName::Exp => a(0).exp(),
                    FnName::Sqrt => a(0).sqrt(),
                    FnName::Abs => a(0).abs(),
                    FnName::Min => a(0).min(a(1)),
                    FnName::Max => a(0).max(a(1)),
                    FnName::Pow => a(0).powf(a(1)),
                }
            }
        }
    }

    /// The subtree's value if it holds no variables, else `None`.
    pub fn fold(&self) -> Option<f64> {
        match self {
            LoadExpr::Const(v) => Some(*v),
            LoadExpr::Var(_) => None,
            LoadExpr::Pi => Some(PI),
            LoadExpr::E => Some(EULER),
            LoadExpr::Neg(a) => Some(-a.fold()?),
            LoadExpr::Sum(a, b) => Some(a.fold()? + b.fold()?),
            LoadExpr::Difference(a, b) => Some(a.fold()? - b.fold()?),
            LoadExpr::Product(a, b) => Some(a.fold()? * b.fold()?),
            LoadExpr::Quotient(a, b) => Some(a.fold()? / b.fold()?),
            LoadExpr::Power(a, b) => Some(a.fold()?.powf(b.fold()?)),
            LoadExpr::Call(name, args) => {
                let g = args
                    .iter()
                    .map(|a| a.fold())
                    .collect::<Option<Vec<f64>>>()?;
                Some(match name {
                    FnName::Sin => g[0].sin(),
                    FnName::Cos => g[0].cos(),
                    FnName::Exp => g[0].exp(),
                    FnName::Sqrt => g[0].sqrt(),
                    FnName::Abs => g[0].abs(),
                    FnName::Min => g[0].min(g[1]),
                    FnName::Max => g[0].max(g[1]),
                    FnName::Pow => g[0].powf(g[1]),
                })
            }
        }
    }
}

/// One band: the body applies where `lo ≤ x ≤ hi`. A plain (bandedless) body is a
/// single band over all x.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    pub lo: f64,
    pub hi: f64,
    pub body: LoadExpr,
}

/// A compiled load function: its source text (kept verbatim for lossless round-trip)
/// and its bands in document order.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadFn {
    pub source: String,
    bands: Vec<Band>,
}

impl LoadFn {
    /// Compile the text form. Errors name the character position inside `fn="..."`
    /// and the fault, and surface identically from the parser, `validate` and the
    /// Setup-lens editor.
    pub fn compile(src: &str) -> Result<LoadFn, String> {
        let toks = tokenize(src)?;
        let mut bands = Vec::new();
        let mut start = 0usize;
        for cut in 0..=toks.len() {
            if cut == toks.len() || toks[cut].tok == Tok::Semi {
                bands.push(parse_piece(&toks[start..cut], src.chars().count())?);
                start = cut + 1;
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
        Ok(LoadFn {
            source: src.to_string(),
            bands,
        })
    }

    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    /// The closed domain the bands cover, for previews. `None` for a plain body.
    pub fn domain(&self) -> Option<(f64, f64)> {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for b in &self.bands {
            lo = lo.min(b.lo);
            hi = hi.max(b.hi);
        }
        lo.is_finite().then_some((lo, hi))
    }

    /// The density at a face-local point; first band wins on shared edges, zero
    /// outside every band.
    pub fn eval(&self, x: f64, y: f64) -> f64 {
        for b in &self.bands {
            if b.lo <= x && x <= b.hi {
                return b.body.eval(x, y);
            }
        }
        0.0
    }

    /// Constant-folding checks beyond parsing: non-finite folded values, division by
    /// a constant zero, negative constants raised to non-integer powers, square roots
    /// of negative constants. `what` names the load, as in "load 'tip'".
    pub fn static_checks(&self, what: &str) -> Vec<String> {
        let mut errs = Vec::new();
        for (i, b) in self.bands.iter().enumerate() {
            if let Some(v) = b.body.fold() {
                if !v.is_finite() {
                    errs.push(format!(
                        "{what}: band {i} is a constant and is not finite ({v}); write a finite value"
                    ));
                }
            }
            check_finite(&b.body, what, i, &mut errs);
        }
        errs
    }
}

/// Flags sub-expressions that fold to a non-finite value although the whole body
/// still carries a variable (so the body's own fold() saw nothing).
fn check_finite(e: &LoadExpr, what: &str, band: usize, errs: &mut Vec<String>) {
    match e {
        LoadExpr::Const(_) | LoadExpr::Var(_) | LoadExpr::Pi | LoadExpr::E => {}
        LoadExpr::Neg(a) => check_finite(a, what, band, errs),
        LoadExpr::Sum(a, b) | LoadExpr::Difference(a, b) | LoadExpr::Product(a, b) => {
            check_finite(a, what, band, errs);
            check_finite(b, what, band, errs);
        }
        LoadExpr::Quotient(a, b) => {
            if b.fold() == Some(0.0) {
                errs.push(format!("{what}: band {band} divides by a constant zero"));
            }
            check_finite(a, what, band, errs);
            check_finite(b, what, band, errs);
        }
        LoadExpr::Power(a, b) => {
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
            check_finite(a, what, band, errs);
            check_finite(b, what, band, errs);
        }
        LoadExpr::Call(name, args) => {
            if *name == FnName::Sqrt && args[0].fold().is_some_and(|v| v < 0.0) {
                errs.push(format!(
                    "{what}: band {band} takes the square root of a negative constant"
                ));
            }
            if *name == FnName::Pow && args.len() == 2 {
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
                check_finite(a, what, band, errs);
            }
        }
    }
}

// ---------------------------------------------------------------- tokenizer --

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Var(bool),
    Fn(FnName),
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
    /// Character offset within the whole `fn="..."` string.
    pos: usize,
}

fn tokenize(src: &str) -> Result<Vec<TokPos>, String> {
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
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < cs.len() && cs[i].is_ascii_alphabetic() {
                i += 1;
            }
            let name: String = cs[start..i].iter().collect();
            let tok = match name.as_str() {
                "x" => Tok::Var(true),
                "y" => Tok::Var(false),
                "pi" => Tok::Pi,
                "e" => Tok::E,
                _ => FnName::parse(&name)
                    .map(Tok::Fn)
                    .ok_or_else(|| {
                        format!(
                            "at character {start}: unknown name '{name}' (allowed: x, y, pi, e, sin, cos, exp, sqrt, abs, min, max, pow)"
                        )
                    })?,
            };
            toks.push(TokPos { tok, pos: start });
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
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

/// Readable token names for parse errors.
fn describe(t: Tok) -> &'static str {
    match t {
        Tok::Num(_) => "a number",
        Tok::Var(_) => "a coordinate",
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

// ------------------------------------------------------------------- parser --

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
    /// The error text for "stopped here": names the character position (or end of input).
    fn err(&self, msg: &str) -> String {
        match self.toks.get(self.i) {
            Some(t) => format!("at character {}: {msg}", t.pos),
            None => format!("at end of input: {msg}"),
        }
    }

    /// sum := term (('+' | '-') term)* — lowest precedence, left-associative.
    fn expr(&mut self) -> Result<LoadExpr, String> {
        let mut a = self.term()?;
        loop {
            let minus = self.eat(Tok::Minus);
            if !minus && !self.eat(Tok::Plus) {
                break;
            }
            let b = self.term()?;
            a = if minus {
                LoadExpr::Difference(Box::new(a), Box::new(b))
            } else {
                LoadExpr::Sum(Box::new(a), Box::new(b))
            };
        }
        Ok(a)
    }

    /// term := unary (('*' | '/') unary)*
    fn term(&mut self) -> Result<LoadExpr, String> {
        let mut a = self.unary()?;
        loop {
            let slash = self.eat(Tok::Slash);
            if !slash && !self.eat(Tok::Star) {
                break;
            }
            let b = self.unary()?;
            a = if slash {
                LoadExpr::Quotient(Box::new(a), Box::new(b))
            } else {
                LoadExpr::Product(Box::new(a), Box::new(b))
            };
        }
        Ok(a)
    }

    /// unary := '-' unary | power   (so -x^2 is -(x^2) and 2^-3 parses)
    fn unary(&mut self) -> Result<LoadExpr, String> {
        if self.eat(Tok::Minus) {
            Ok(LoadExpr::Neg(Box::new(self.unary()?)))
        } else {
            self.power()
        }
    }

    /// power := atom ('^' unary)? — right-associative, binds tighter than unary minus.
    fn power(&mut self) -> Result<LoadExpr, String> {
        let a = self.atom()?;
        if self.eat(Tok::Caret) {
            let b = self.unary()?;
            Ok(LoadExpr::Power(Box::new(a), Box::new(b)))
        } else {
            Ok(a)
        }
    }

    /// atom := num | x | y | pi | e | '(' expr ')' | fn '(' expr [, expr] ')'
    fn atom(&mut self) -> Result<LoadExpr, String> {
        let (tok, pos) = match self.toks.get(self.i) {
            Some(t) => (t.tok, t.pos),
            None => {
                return Err("at end of input: expected a number, x or y, or '('.".into());
            }
        };
        match tok {
            Tok::Num(v) => {
                self.i += 1;
                Ok(LoadExpr::Const(v))
            }
            Tok::Var(is_x) => {
                self.i += 1;
                Ok(LoadExpr::Var(is_x))
            }
            Tok::Pi => {
                self.i += 1;
                Ok(LoadExpr::Pi)
            }
            Tok::E => {
                self.i += 1;
                Ok(LoadExpr::E)
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
                let arity = f.arity();
                if args.len() != arity {
                    let s = if arity == 1 { "" } else { "s" };
                    return Err(format!(
                        "at character {pos}: {} takes {arity} argument{s}, given {}",
                        f.word(),
                        args.len()
                    ));
                }
                Ok(LoadExpr::Call(f, args))
            }
            other => Err(self.err(&format!(
                "expected a number, x or y, or '(', found {}",
                describe(other)
            ))),
        }
    }
}

/// One band from its tokens: `lo < x < hi -> body`, or a bare `body` spanning all x.
/// `end_char` is where an empty piece would sit, for the empty-band error.
fn parse_piece(piece: &[TokPos], end_char: usize) -> Result<Band, String> {
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
        return Ok(Band {
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
    h.expect(Tok::Var(true), "x in the band header")?;
    h.expect(Tok::Lt, "'<' in the band header")?;
    let hi_e = h.expr()?;
    if h.i != h.toks.len() {
        return Err(h.err("trailing text in the band header"));
    }
    let lo = lo_e.fold().filter(|v| v.is_finite()).ok_or_else(|| {
        "the band's lower bound must be a constant number (no x or y)".to_string()
    })?;
    let hi = hi_e.fold().filter(|v| v.is_finite()).ok_or_else(|| {
        "the band's upper bound must be a constant number (no x or y)".to_string()
    })?;
    if lo > hi {
        return Err(format!(
            "the band's bounds are reversed ([{lo}, {hi}]: the lower bound is above the upper)"
        ));
    }
    Ok(Band { lo, hi, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(src: &str) -> String {
        LoadFn::compile(src).expect_err("compile should fail")
    }

    #[test]
    fn user_example_bands() {
        let f = LoadFn::compile("-0.1 < x < 0.1 -> 1; 0.1 < x < 0.9 -> -1").unwrap();
        assert_eq!(f.eval(0.0, 7.0), 1.0);
        assert_eq!(f.eval(0.5, 0.0), -1.0);
        assert_eq!(f.eval(2.0, 0.0), 0.0);
        // Touching bands: the shared edge belongs to the first band.
        assert_eq!(f.eval(0.1, 0.0), 1.0);
        assert_eq!(f.source, "-0.1 < x < 0.1 -> 1; 0.1 < x < 0.9 -> -1");
        assert_eq!(f.domain(), Some((-0.1, 0.9)));
    }

    #[test]
    fn plain_body_has_one_open_band() {
        let f = LoadFn::compile("0.2 * x + 0.05").unwrap();
        assert_eq!(f.bands().len(), 1);
        assert_eq!(f.domain(), None);
        assert_eq!(f.eval(-3.0, 1.0), 0.2 * -3.0 + 0.05);
    }

    #[test]
    fn expression_eval() {
        let f = LoadFn::compile("sin(x*pi/25) * 0.5 + y*y").unwrap();
        let want = (25.0f64 * PI / 25.0).sin() * 0.5 + 4.0;
        assert!((f.eval(25.0, 2.0) - want).abs() < 1e-12);
        assert_eq!(f.eval(0.0, 0.0), 0.0);
    }

    #[test]
    fn unknown_variable_is_named() {
        let e = err("w * 2");
        assert!(e.contains("unknown name 'w'"), "{e}");
        let e = err("-0.1 < x < 0.1 -> w");
        assert!(e.contains("unknown name 'w'"), "{e}");
    }

    #[test]
    fn overlaps_reversed_empty_are_named() {
        let e = err("0 < x < 1 -> 1; 0.5 < x < 2 -> 2");
        assert!(e.contains("bands overlap"), "{e}");
        let e = err("1 < x < 0 -> 1");
        assert!(e.contains("reversed"), "{e}");
        let e = err("0 < x < 1 -> 1; ; 1 < x < 2 -> 2");
        assert!(e.contains("bands are separated"), "{e}");
        let e = err("0 < x < 1 -> 1;");
        assert!(e.contains("bands are separated"), "{e}");
    }

    #[test]
    fn header_syntax_is_named() {
        let e = err("0 < y < 1 -> 1");
        assert!(e.contains("x in the band header"), "{e}");
        let e = err("0 < x < x -> 1");
        assert!(e.contains("constant number"), "{e}");
        let e = err("0 < x < 1 -> 1 -> 2");
        assert!(e.contains("unexpected"), "{e}");
        let e = err("x*2 -> 3");
        assert!(e.contains("'<' in the band header"), "{e}");
    }

    #[test]
    fn static_checks_catch_constant_faults() {
        let f = LoadFn::compile("1 / 0").unwrap();
        let e = f.static_checks("load 'a'").join(" | ");
        assert!(e.contains("divides by a constant zero"), "{e}");
        assert!(e.contains("not finite"), "{e}");

        let f = LoadFn::compile("0.1 < x < 0.9 -> 1 / (0.5 - 0.5); 0.9 < x < 2 -> 0").unwrap();
        let e = f.static_checks("load 'a'").join(" | ");
        assert!(e.contains("divides by a constant zero"), "{e}");
        assert!(e.contains("not finite"), "{e}");

        let f = LoadFn::compile("x + sqrt(-1)").unwrap();
        let e = f.static_checks("load 'a'").join(" | ");
        assert!(e.contains("square root of a negative constant"), "{e}");

        let f = LoadFn::compile("pow(-1, 0.5)").unwrap();
        let e = f.static_checks("load 'a'").join(" | ");
        assert!(e.contains("non-integer power"), "{e}");

        // A variable-dependent fault with no constant fault stays silent here; the
        // solver catches it at the facet, naming the coordinates.
        let f = LoadFn::compile("1 / (x - x)").unwrap();
        assert!(f.static_checks("load 'a'").is_empty());
        let f = LoadFn::compile("0.1 < x < 0.9 -> 1 / (x - 0.5); 0.9 < x < 2 -> 0").unwrap();
        assert!(f.static_checks("load 'a'").is_empty());
    }

    #[test]
    fn precedence_and_unary_minus() {
        let f = LoadFn::compile("-0.2 * x ^ 2 + 1").unwrap();
        assert_eq!(f.eval(3.0, 0.0), -0.2 * 9.0 + 1.0);
        let f = LoadFn::compile("2 ^ -2").unwrap();
        assert_eq!(f.eval(0.0, 0.0), 0.25);
        // Right-associative power.
        let f = LoadFn::compile("2 ^ 3 ^ 0").unwrap();
        assert_eq!(f.eval(0.0, 0.0), 2.0);
    }

    #[test]
    fn functions_two_args_and_parentheses() {
        let f = LoadFn::compile("max(x, 0) * min(y, 2) * (pi + e)").unwrap();
        assert_eq!(f.eval(1.0, 3.0), 1.0 * 2.0 * (PI + EULER));
        assert_eq!(f.eval(-1.0, 3.0), 0.0);
        let f = LoadFn::compile("abs(-2 * x)").unwrap();
        assert_eq!(f.eval(3.0, 0.0), 6.0);
    }

    #[test]
    fn arity_errors() {
        let e = err("sin(x, y)");
        assert!(e.contains("sin takes 1 argument"), "{e}");
        let e = err("max(x)");
        assert!(e.contains("max takes 2 arguments"), "{e}");
        let e = err("sin x");
        assert!(e.contains("'('"), "{e}");
    }

    #[test]
    fn positions_land_in_the_whole_string() {
        // 'w' sits late in the string; the position must be the whole-string offset.
        let e = err("0 < x < 1 -> x/2; 1 < x < 2 -> w");
        assert!(e.contains("at character 31"), "{e}");
        let e = err("1 +");
        assert!(e.contains("at end of input"), "{e}");
        let e = err("q / 0");
        assert!(e.contains("character 0"), "{e}");
    }
}
