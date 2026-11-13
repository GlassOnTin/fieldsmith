//! The `.fsm` text format: a small, line-oriented, diffable description of a document.
//!
//! ```text
//! fieldsmith 1
//! name "Bracket"
//! material petg
//!
//! box base size=(60, 40, 8) at=(30, 20, 4)
//! box upright size=(60, 8, 60) at=(30, 4, 30) blend=8
//! cylinder hole1 r=3.25 h=20 at=(15, 25, 4) subtract
//! group arms rotate=(0, 0, 45) {
//!   sphere s1 r=5
//!   mirror x
//! }
//! color "#B5432A" on upright
//!
//! region holes = hole1.side + hole2.side
//! support fix1 holes fixed=xyz
//! load push1 tip force=(0, -103.5, -386.4) case="Load 1"
//! fill gyroid sheet cell=6 skin=1.2 rho=(0.2, 0.8) curve=1 wall=0.8 protect=(hole1) margin=2
//! solve size=2.5
//! ```
//!
//! [`print`] produces a canonical form; `print(parse(print(d))) == print(d)` always holds.

use crate::materials::{Material, MaterialRole};
use crate::model::*;
use std::fmt::Write as _;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    match c.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

// ---------------------------------------------------------------------------------------------
// Tokens

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Num(f64),
    Str(String),
    Sym(char),
}

fn tokenize_line(line: &str, lineno: usize) -> Result<Vec<Tok>, ParseError> {
    let err = |m: String| ParseError {
        line: lineno,
        message: m,
    };
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '#' {
            break;
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                if i >= chars.len() {
                    return Err(err("unterminated string".into()));
                }
                match chars[i] {
                    '"' => {
                        i += 1;
                        break;
                    }
                    '\\' if i + 1 < chars.len() => {
                        s.push(chars[i + 1]);
                        i += 2;
                    }
                    ch => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(s));
        } else if c.is_ascii_digit()
            || ((c == '-' || c == '+' || c == '.')
                && i + 1 < chars.len()
                && (chars[i + 1].is_ascii_digit() || (chars[i + 1] == '.' && c != '.'))
                && !matches!(out.last(), Some(Tok::Ident(_)) | Some(Tok::Num(_))))
        {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == 'e'
                    || chars[i] == 'E'
                    || ((chars[i] == '-' || chars[i] == '+') && matches!(chars[i - 1], 'e' | 'E')))
            {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            let v: f64 = s.parse().map_err(|_| err(format!("bad number '{s}'")))?;
            out.push(Tok::Num(v));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if "(){}[],=.+*/-^".contains(c) {
            out.push(Tok::Sym(c));
            i += 1;
        } else {
            return Err(err(format!("unexpected character '{c}'")));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Values and attribute lists

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Num(f64),
    Str(String),
    Ident(String),
    Tuple(Vec<Val>),
    List(Vec<Val>),
    /// Arithmetic kept as canonical text (`2 * wall_t`); the value is resolved by the
    /// attribute reader, so errors carry the attribute and target it appears on.
    Expr {
        txt: String,
    },
}

// Arithmetic expressions -------------------------------------------------------

/// A numeric expression from `.fsm`: `let w = 4`, `size=(100, 2 * w, w / 2)`,
/// `temp=80 + w`. Precedence: atoms/calls, power (^), unary minus, then * and /, then + and −.
#[derive(Debug, Clone)]
enum Expr {
    Num(f64),
    Var(String),
    Call(String, Vec<Expr>),
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
}

/// Parse an atomic operand: number, identifier, function call `f(a, ...)`, or `(e)`.
fn expr_atom(c: &mut Cursor) -> Result<Expr, ParseError> {
    match c.next() {
        Some(Tok::Num(v)) => Ok(Expr::Num(v)),
        Some(Tok::Ident(s)) => {
            if c.is_sym('(') {
                c.next();
                let mut args = Vec::new();
                if !c.is_sym(')') {
                    loop {
                        args.push(expr_sum(c)?);
                        if c.is_sym(',') {
                            c.next();
                        } else {
                            break;
                        }
                    }
                }
                c.sym(')')?;
                Ok(Expr::Call(s, args))
            } else {
                Ok(Expr::Var(s))
            }
        }
        Some(Tok::Sym('(')) => {
            let e = expr_sum(c)?;
            c.sym(')')?;
            Ok(e)
        }
        _ => c.err("expected a value"),
    }
}

/// Parse a power expression: `atom ('^' unary)?` (right-associative, binds tighter than unary `-`).
fn expr_power(c: &mut Cursor) -> Result<Expr, ParseError> {
    let a = expr_atom(c)?;
    if c.is_sym('^') {
        c.next();
        Ok(Expr::Bin('^', Box::new(a), Box::new(expr_unary(c)?)))
    } else {
        Ok(a)
    }
}

/// Parse a unary operand: `-e` or a power expression.
fn expr_unary(c: &mut Cursor) -> Result<Expr, ParseError> {
    if c.is_sym('-') {
        c.next();
        return Ok(Expr::Neg(Box::new(expr_unary(c)?)));
    }
    expr_power(c)
}

/// Fold `*` and `/` (left-associative) onto a parsed left operand.
fn expr_products(c: &mut Cursor, mut left: Expr) -> Result<Expr, ParseError> {
    while matches!(c.peek(), Some(Tok::Sym('*')) | Some(Tok::Sym('/'))) {
        let op = match c.next() {
            Some(Tok::Sym(o)) => o,
            _ => unreachable!("matched"),
        };
        left = Expr::Bin(op, Box::new(left), Box::new(expr_unary(c)?));
    }
    Ok(left)
}

/// A product-level operand, for the right side of a sum.
fn expr_product(c: &mut Cursor) -> Result<Expr, ParseError> {
    let head = expr_unary(c)?;
    expr_products(c, head)
}

/// Continue a parsed left operand through any remaining powers, products, then sums.
fn expr_tail(c: &mut Cursor, left: Expr) -> Result<Expr, ParseError> {
    let mut left = expr_products(c, left)?;
    while matches!(c.peek(), Some(Tok::Sym('+')) | Some(Tok::Sym('-'))) {
        let op = match c.next() {
            Some(Tok::Sym(o)) => o,
            _ => unreachable!("matched"),
        };
        left = Expr::Bin(op, Box::new(left), Box::new(expr_product(c)?));
    }
    Ok(left)
}

/// Parse a full expression at the cursor (the `let` statement's value).
fn expr_sum(c: &mut Cursor) -> Result<Expr, ParseError> {
    let head = expr_unary(c)?;
    expr_tail(c, head)
}

/// Binding power for the canonical printer: atoms/calls > power > unary/products > sums.
fn expr_prec(e: &Expr) -> u8 {
    match e {
        Expr::Num(_) | Expr::Var(_) | Expr::Call(_, _) => 4,
        Expr::Bin('^', _, _) => 3,
        Expr::Neg(_) => 2,
        Expr::Bin(op, _, _) => match op {
            '+' | '-' => 1,
            _ => 2,
        },
    }
}

/// Canonical text for a parsed tree, stable under re-parse: `2 * wall_t + 2`,
/// `-(wall_t * 25)`, `wall_t / 2`.
fn expr_txt(e: &Expr) -> String {
    fn go(e: &Expr, o: &mut String) {
        match e {
            Expr::Num(v) => {
                let _ = write!(o, "{}", n(*v));
            }
            Expr::Var(s) => {
                let _ = write!(o, "{s}");
            }
            Expr::Call(name, args) => {
                let _ = write!(o, "{name}(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        let _ = write!(o, ", ");
                    }
                    go(a, o);
                }
                let _ = write!(o, ")");
            }
            Expr::Neg(child) => match child.as_ref() {
                Expr::Num(_) | Expr::Var(_) | Expr::Call(_, _) => {
                    let _ = write!(o, "-");
                    go(child, o);
                }
                _ => {
                    let _ = write!(o, "-(");
                    go(child, o);
                    let _ = write!(o, ")");
                }
            },
            Expr::Bin(op, l, r) => {
                let p = match op {
                    '+' | '-' => 1,
                    '^' => 3,
                    _ => 2,
                };
                if expr_prec(l) < p || (*op == '^' && expr_prec(l) == p) {
                    let _ = write!(o, "(");
                    go(l, o);
                    let _ = write!(o, ")");
                } else {
                    go(l, o);
                }
                let _ = write!(o, " {op} ");
                // Non-associative operators parenthesise an equal-precedence right
                // side, so `a - (b - c)` and `a / (b * c)` keep their meaning.
                if expr_prec(r) < p || (expr_prec(r) == p && matches!(op, '-' | '/')) {
                    let _ = write!(o, "(");
                    go(r, o);
                    let _ = write!(o, ")");
                } else {
                    go(r, o);
                }
            }
        }
    }
    let mut o = String::new();
    go(e, &mut o);
    o
}

/// Evaluate an expression text against let values. Public so model validation can
/// re-check recorded expressions without re-parsing. Errors are the plain message
/// (`no let named 'w'`, `division by zero`); callers add their own context.
pub fn eval_expr(text: &str, lets: &[(String, f64)]) -> Result<f64, String> {
    let toks = tokenize_line(text, 0).map_err(|e| e.message)?;
    let mut c = Cursor {
        toks: &toks,
        pos: 0,
        line: 0,
    };
    let tree = expr_sum(&mut c).map_err(|e| e.message)?;
    if !c.done() {
        return Err("unexpected text after the expression".into());
    }
    fn eval_tree(e: &Expr, lets: &[(String, f64)]) -> Result<f64, String> {
        Ok(match e {
            Expr::Num(v) => *v,
            Expr::Var(s) => {
                if let Some((_, v)) = lets.iter().find(|(name, _)| name == s) {
                    *v
                } else if s == "pi" {
                    std::f64::consts::PI
                } else if s == "e" {
                    std::f64::consts::E
                } else {
                    return Err(format!("no let named '{s}'"));
                }
            }
            Expr::Call(name, args) => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(eval_tree(a, lets)?);
                }
                match (name.as_str(), vals.as_slice()) {
                    ("sin", [x]) => x.sin(),
                    ("cos", [x]) => x.cos(),
                    ("tan", [x]) => x.tan(),
                    ("sqrt", [x]) => x.sqrt(),
                    ("abs", [x]) => x.abs(),
                    ("exp", [x]) => x.exp(),
                    ("ln", [x]) => x.ln(),
                    ("min", [a, b]) => a.min(*b),
                    ("max", [a, b]) => a.max(*b),
                    ("pow", [a, b]) => a.powf(*b),
                    ("sin" | "cos" | "tan" | "sqrt" | "abs" | "exp" | "ln", _) => {
                        return Err(format!("{name} takes 1 argument, given {}", vals.len()));
                    }
                    ("min" | "max" | "pow", _) => {
                        return Err(format!("{name} takes 2 arguments, given {}", vals.len()));
                    }
                    _ => return Err(format!("unknown function '{name}'")),
                }
            }
            Expr::Neg(i) => -eval_tree(i, lets)?,
            Expr::Bin(op, l, r) => {
                let lv = eval_tree(l, lets)?;
                let rv = eval_tree(r, lets)?;
                match op {
                    '+' => lv + rv,
                    '-' => lv - rv,
                    '*' => lv * rv,
                    '/' => {
                        if rv == 0.0 {
                            return Err("division by zero".into());
                        }
                        lv / rv
                    }
                    '^' => lv.powf(rv),
                    _ => return Err(format!("unknown operator '{op}'")),
                }
            }
        })
    }
    eval_tree(&tree, lets)
}

struct Cursor<'a> {
    toks: &'a [Tok],
    pos: usize,
    line: usize,
}

impl<'a> Cursor<'a> {
    fn err<T>(&self, m: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            line: self.line,
            message: m.into(),
        })
    }
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn done(&self) -> bool {
        self.pos >= self.toks.len()
    }
    fn ident(&mut self, what: &str) -> Result<String, ParseError> {
        match self.next() {
            Some(Tok::Ident(s)) => Ok(s),
            _ => self.err(format!("expected {what}")),
        }
    }
    fn sym(&mut self, c: char) -> Result<(), ParseError> {
        match self.next() {
            Some(Tok::Sym(s)) if s == c => Ok(()),
            _ => self.err(format!("expected '{c}'")),
        }
    }
    fn is_sym(&self, c: char) -> bool {
        matches!(self.peek(), Some(Tok::Sym(s)) if *s == c)
    }
    fn value(&mut self) -> Result<Val, ParseError> {
        // An operator after a number or name (`2 * wall_t`) or a function call (`cos(0)`)
        // starts an arithmetic expression; it is kept as canonical text for the attribute
        // readers, which evaluate it against the lets and record the consumer.
        let follows_op = matches!(
            self.toks.get(self.pos + 1),
            Some(Tok::Sym(s)) if "+-*/^".contains(*s)
        );
        let follows_call = matches!(
            (self.toks.get(self.pos), self.toks.get(self.pos + 1)),
            (Some(Tok::Ident(_)), Some(Tok::Sym('(')))
        );
        match self.peek() {
            Some(Tok::Num(_) | Tok::Ident(_)) if follows_op || follows_call => {
                let e = expr_sum(self)?;
                Ok(Val::Expr { txt: expr_txt(&e) })
            }
            Some(Tok::Sym('-')) => {
                self.next();
                let inner = expr_unary(self)?;
                let e = expr_tail(self, Expr::Neg(Box::new(inner)))?;
                Ok(Val::Expr { txt: expr_txt(&e) })
            }
            Some(Tok::Sym('(')) => {
                // `(a, b, c)` is a tuple; `(2 - 1)` is a parenthesised scalar.
                if self.scan_top_level_comma() {
                    self.next();
                    Ok(Val::Tuple(self.seq(')')?))
                } else {
                    let e = expr_sum(self)?;
                    Ok(Val::Expr { txt: expr_txt(&e) })
                }
            }
            Some(Tok::Sym('[')) => {
                self.next();
                Ok(Val::List(self.seq(']')?))
            }
            _ => match self.next() {
                Some(Tok::Num(v)) => Ok(Val::Num(v)),
                Some(Tok::Str(s)) => Ok(Val::Str(s)),
                Some(Tok::Ident(s)) => Ok(Val::Ident(s)),
                _ => self.err("expected a value"),
            },
        }
    }
    /// Whether the parenthesised value starting at this '(' has a top-level comma,
    /// which makes it a tuple rather than a parenthesised expression.
    fn scan_top_level_comma(&self) -> bool {
        let mut depth = 1usize;
        for t in &self.toks[self.pos + 1..] {
            match t {
                Tok::Sym('(') | Tok::Sym('[') => depth += 1,
                Tok::Sym(')') | Tok::Sym(']') => {
                    depth -= 1;
                    if depth == 0 {
                        return false;
                    }
                }
                Tok::Sym(',') if depth == 1 => return true,
                _ => {}
            }
        }
        false
    }
    fn seq(&mut self, close: char) -> Result<Vec<Val>, ParseError> {
        let mut items = Vec::new();
        if self.is_sym(close) {
            self.next();
            return Ok(items);
        }
        loop {
            items.push(self.value()?);
            match self.next() {
                Some(Tok::Sym(',')) => continue,
                Some(Tok::Sym(c)) if c == close => break,
                _ => return self.err(format!("expected ',' or '{close}'")),
            }
        }
        Ok(items)
    }
    /// Remaining tokens as `key=value` pairs or bare flags, addressed to `target`
    /// with the lets in scope (which attribute readers use to resolve expressions).
    fn attrs(&mut self, target: &str, lets: &[(String, f64)]) -> Result<Attrs, ParseError> {
        let mut a = Attrs {
            items: Vec::new(),
            line: self.line,
            target: target.to_string(),
            lets: lets.to_vec(),
            consumed: Vec::new(),
        };
        while !self.done() {
            if self.is_sym('{') {
                break;
            }
            let key = self.ident("attribute name")?;
            if self.is_sym('=') {
                self.next();
                let v = self.value()?;
                a.items.push((key, Some(v)));
            } else {
                a.items.push((key, None));
            }
        }
        Ok(a)
    }
}

struct Attrs {
    items: Vec<(String, Option<Val>)>,
    line: usize,
    /// The statement the attrs belong to, for consumer-record targets.
    target: String,
    /// The lets in scope, for resolving `Val::Expr` and bare let names.
    lets: Vec<(String, f64)>,
    /// Expressions this statement read, to be moved into the document.
    consumed: Vec<Consumer>,
}

impl Attrs {
    fn err<T>(&self, m: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            line: self.line,
            message: m.into(),
        })
    }
    fn take(&mut self, key: &str) -> Option<Option<Val>> {
        let i = self.items.iter().position(|(k, _)| k == key)?;
        Some(self.items.remove(i).1)
    }
    fn flag(&mut self, key: &str) -> bool {
        matches!(self.take(key), Some(None))
    }
    fn num(&mut self, key: &str) -> Result<Option<f64>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::Num(v))) => Ok(Some(v)),
            Some(_) => self.err(format!("'{key}' must be a number")),
        }
    }
    fn num_or(&mut self, key: &str, d: f64) -> Result<f64, ParseError> {
        Ok(self.num(key)?.unwrap_or(d))
    }
    fn req_num(&mut self, key: &str) -> Result<f64, ParseError> {
        match self.num(key)? {
            Some(v) => Ok(v),
            None => self.err(format!("missing '{key}='")),
        }
    }
    /// Evaluate an expression text in this statement's context: let-free trees fold
    /// to plain numbers with no record; trees reading a let record a consumer and
    /// return today's value. `key` names the attribute as written for the errors;
    /// `attr` is the model vocabulary the consumer record stores.
    fn eval_for(
        &mut self,
        key: &str,
        attr: &str,
        comp: Option<usize>,
        txt: &str,
    ) -> Result<f64, ParseError> {
        let uses_let = tokenize_line(txt, 0)
            .map(|toks| {
                toks.iter().any(
                    |t| matches!(t, Tok::Ident(s) if self.lets.iter().any(|(name, _)| name == s)),
                )
            })
            .unwrap_or(false);
        if !uses_let {
            if let Ok(v) = eval_expr(txt, &[]) {
                return Ok(v);
            }
        }
        match eval_expr(txt, &self.lets) {
            Ok(v) if v.is_finite() => {
                self.consumed.push(Consumer {
                    target: self.target.clone(),
                    attr: attr.to_string(),
                    comp,
                    expr: txt.to_string(),
                });
                Ok(v)
            }
            Ok(_) => self.err(format!("'{key}' on '{}': value is not finite", self.target)),
            Err(m) => self.err(format!("'{key}' on '{}': {m}", self.target)),
        }
    }
    /// A number that may be driven by a `let` expression or bare let name.
    fn expr_num(&mut self, key: &str, attr: &str) -> Result<Option<f64>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::Num(v))) => Ok(Some(v)),
            Some(Some(Val::Ident(s))) => Ok(Some(self.eval_for(key, attr, None, &s)?)),
            Some(Some(Val::Expr { txt })) => Ok(Some(self.eval_for(key, attr, None, &txt)?)),
            Some(_) => self.err(format!("'{key}' must be a number")),
        }
    }
    fn req_expr_num(&mut self, key: &str, attr: &str) -> Result<f64, ParseError> {
        match self.expr_num(key, attr)? {
            Some(v) => Ok(v),
            None => self.err(format!("missing '{key}='")),
        }
    }
    fn expr_num_or(&mut self, key: &str, attr: &str, d: f64) -> Result<f64, ParseError> {
        Ok(self.expr_num(key, attr)?.unwrap_or(d))
    }
    /// Temperature `temp=` is the one compound-only position: `80 + wall_t` drives
    /// it, but a bare let name (`temp=w`) is refused — the temperature's sweepable
    /// address is a plain number on its own line.
    fn req_compound_num(&mut self, key: &str, attr: &str) -> Result<f64, ParseError> {
        match self.take(key) {
            None => self.err(format!("missing '{key}='")),
            Some(Some(Val::Num(v))) => Ok(v),
            Some(Some(Val::Expr { txt })) => Ok(self.eval_for(key, attr, None, &txt)?),
            Some(Some(Val::Ident(_))) | Some(_) => self.err(format!("'{key}' must be a number")),
        }
    }
    fn opt_compound_num(&mut self, key: &str, attr: &str) -> Result<Option<f64>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::Num(v))) => Ok(Some(v)),
            Some(Some(Val::Expr { txt })) => Ok(Some(self.eval_for(key, attr, None, &txt)?)),
            Some(Some(Val::Ident(_))) | Some(_) => self.err(format!("'{key}' must be a number")),
        }
    }
    fn req_string(&mut self, key: &str) -> Result<String, ParseError> {
        match self.string(key)? {
            Some(s) => Ok(s),
            None => self.err(format!("missing '{key}='")),
        }
    }
    fn vec3(&mut self, key: &str) -> Result<Option<[f64; 3]>, ParseError> {
        // Only vectors the param grammar sweeps take expressions (`size=`, `at=`,
        // `rotate=`, `force=`); `dir=`, `step=` and `local=` are fixed at declaration,
        // so an expression there reads like a non-numeric value.
        let vocab: Option<&str> = match key {
            "size" | "at" | "rotate" | "force" if !self.target.is_empty() => Some(key),
            _ => None,
        };
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::Tuple(t))) if t.len() == 3 => {
                let mut o = [0.0; 3];
                for (i, v) in t.iter().enumerate() {
                    match v {
                        Val::Num(x) => o[i] = *x,
                        // Only the vocab vectors record; the `_` arm's tuple error
                        // still names `key` for anything non-numeric.
                        Val::Ident(s) if vocab.is_some() => {
                            o[i] = self.eval_for(key, key, Some(i), s)?
                        }
                        Val::Expr { txt } if vocab.is_some() => {
                            o[i] = self.eval_for(key, key, Some(i), txt)?
                        }
                        _ => return self.err(format!("'{key}' must be (x, y, z)")),
                    }
                }
                Ok(Some(o))
            }
            Some(_) => self.err(format!("'{key}' must be (x, y, z)")),
        }
    }
    fn pair(&mut self, key: &str) -> Result<Option<(f64, f64)>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::Tuple(t))) if t.len() == 2 => match (&t[0], &t[1]) {
                (Val::Num(a), Val::Num(b)) => Ok(Some((*a, *b))),
                _ => self.err(format!("'{key}' must be (a, b)")),
            },
            Some(_) => self.err(format!("'{key}' must be (a, b)")),
        }
    }
    fn string(&mut self, key: &str) -> Result<Option<String>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::Str(s))) | Some(Some(Val::Ident(s))) => Ok(Some(s)),
            Some(_) => self.err(format!("'{key}' must be text")),
        }
    }
    /// Six numbers, e.g. a mesh node's baked bounds `(xmin, ymin, zmin, xmax, ymax, zmax)`.
    fn vec6(&mut self, key: &str) -> Result<Option<[f64; 6]>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::Tuple(t))) if t.len() == 6 => {
                let mut o = [0.0; 6];
                for (i, v) in t.iter().enumerate() {
                    match v {
                        Val::Num(x) => o[i] = *x,
                        _ => return self.err(format!("'{key}' must be six numbers")),
                    }
                }
                Ok(Some(o))
            }
            Some(_) => self.err(format!("'{key}' must be six numbers")),
        }
    }
    fn idents(&mut self, key: &str) -> Result<Vec<String>, ParseError> {
        match self.take(key) {
            None => Ok(Vec::new()),
            Some(Some(Val::Tuple(t))) | Some(Some(Val::List(t))) => t
                .into_iter()
                .map(|v| match v {
                    Val::Ident(s) => Ok(s),
                    _ => self.err(format!("'{key}' must be a list of names")),
                })
                .collect(),
            Some(Some(Val::Ident(s))) => Ok(vec![s]),
            Some(_) => self.err(format!("'{key}' must be a list of names")),
        }
    }
    fn val_to_signed_num(&self, key: &str, v: &Val) -> Result<f64, ParseError> {
        match v {
            Val::Num(x) if x.is_finite() => Ok(*x),
            Val::Expr { txt } => match eval_expr(txt, &[]) {
                Ok(x) if x.is_finite() => Ok(x),
                _ => self.err(format!("'{key}' must be a finite number")),
            },
            _ => self.err(format!("'{key}' must be a finite number")),
        }
    }
    fn signed_num(&mut self, key: &str) -> Result<Option<f64>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(v)) => Ok(Some(self.val_to_signed_num(key, &v)?)),
            Some(None) => self.err(format!("'{key}' must be a finite number")),
        }
    }
    fn req_signed_num(&mut self, key: &str) -> Result<f64, ParseError> {
        match self.signed_num(key)? {
            Some(v) => Ok(v),
            None => self.err(format!("missing '{key}='")),
        }
    }
    fn signed_num_or(&mut self, key: &str, d: f64) -> Result<f64, ParseError> {
        Ok(self.signed_num(key)?.unwrap_or(d))
    }
    fn signed_pair(&mut self, key: &str) -> Result<Option<[f64; 2]>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::Tuple(t))) if t.len() == 2 => Ok(Some([
                self.val_to_signed_num(key, &t[0])?,
                self.val_to_signed_num(key, &t[1])?,
            ])),
            Some(_) => self.err(format!("'{key}' must be (a, b)")),
        }
    }
    fn signed_nums(&mut self, key: &str) -> Result<Option<Vec<f64>>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::List(items))) | Some(Some(Val::Tuple(items))) => {
                let mut out = Vec::with_capacity(items.len());
                for item in &items {
                    out.push(self.val_to_signed_num(key, item)?);
                }
                Ok(Some(out))
            }
            Some(Some(single)) => Ok(Some(vec![self.val_to_signed_num(key, &single)?])),
            Some(None) => self.err(format!("'{key}' must be a list of numbers")),
        }
    }
    fn finish(self) -> Result<(), ParseError> {
        if let Some((k, _)) = self.items.first() {
            return self.err(format!("unknown attribute '{k}'"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Parser

/// Parse `.fsm` text into a document.
pub fn parse(text: &str) -> Result<Document, ParseError> {
    let mut doc = Document {
        name: "Untitled".into(),
        ..Document::default()
    };
    if let Some(hdr) = parse_literate_header(text) {
        doc.asserts = hdr.asserts;
        if let Some(cs) = hdr.coupled_system {
            doc.coupled_system = Some(cs);
        }
    }
    // Stack of open groups: (header node, children, modifiers).
    let mut stack: Vec<(Node, Vec<Node>, Vec<Modifier>)> = Vec::new();
    // Open `sketch <id> ... { ... }` block, if currently inside one.
    let mut open_sketch: Option<crate::sketch::ParametricSketch> = None;
    // Open `system <id> ... { ... }` block, if currently inside one.
    let mut open_system: Option<crate::system::DeclarativeSystem> = None;
    let mut saw_header = false;
    // Per-node material labels wait here until parse end: `material <id> on <shape>` may
    // run before the shape's line. A shape the document never declares fails then, by
    // name, instead of the label being dropped.
    let mut pending_labels: Vec<(String, String)> = Vec::new();
    // Same lane for `color <hex> on <shape>` (canonicalised at the statement).
    let mut pending_colours: Vec<(String, String)> = Vec::new();

    for (idx, raw) in text.lines().enumerate() {
        let lineno = idx + 1;
        let toks = tokenize_line(raw, lineno)?;
        if toks.is_empty() {
            continue;
        }
        let mut c = Cursor {
            toks: &toks,
            pos: 0,
            line: lineno,
        };
        let err = |m: String| ParseError {
            line: lineno,
            message: m,
        };

        if c.is_sym('}') {
            c.next();
            if !c.done() {
                return Err(err("nothing may follow '}' on the same line".into()));
            }
            if let Some(sk) = open_sketch.take() {
                doc.sketches.push(sk);
                continue;
            }
            if let Some(sys) = open_system.take() {
                doc.system_block = Some(sys);
                continue;
            }
            let (mut node, children, modifiers) =
                stack.pop().ok_or_else(|| err("unmatched '}'".into()))?;
            node.kind = NodeKind::Group {
                children,
                modifiers,
            };
            push_node(&mut doc, &mut stack, node);
            continue;
        }

        if let Some(ref mut sk) = open_sketch {
            parse_sketch_body_line(sk, &mut c)?;
            if !c.done() {
                return Err(err("unexpected text at end of sketch line".into()));
            }
            continue;
        }

        if let Some(ref mut sys) = open_system {
            parse_system_body_line(sys, &mut c)?;
            if !c.done() {
                return Err(err("unexpected text at end of system line".into()));
            }
            continue;
        }

        let kw = c.ident("a keyword")?;
        if !saw_header {
            if kw != "fieldsmith" {
                return Err(err("file must start with 'fieldsmith <version>'".into()));
            }
            match c.next() {
                Some(Tok::Num(v)) if v as u32 == FORMAT_VERSION => {}
                _ => {
                    return Err(err(format!(
                        "unsupported format version (expected {FORMAT_VERSION})"
                    )))
                }
            }
            saw_header = true;
            continue;
        }

        match kw.as_str() {
            "name" => match c.next() {
                Some(Tok::Str(s)) => doc.name = s,
                _ => return Err(err("expected a quoted name".into())),
            },
            "material" => {
                let id = c.ident("material id")?;
                if matches!(c.peek(), Some(Tok::Ident(t)) if t == "on") {
                    c.next();
                    let target = c.ident("the shape to label")?;
                    pending_labels.push((id, target));
                } else {
                    doc.material = id;
                }
            }
            "coupled" => {
                let sys_id = match c.next() {
                    Some(Tok::Ident(s) | Tok::Str(s)) if is_ident(&s) => s,
                    Some(Tok::Ident(s) | Tok::Str(s)) => {
                        return Err(err(format!(
                            "coupled system '{s}' is not a valid identifier"
                        )))
                    }
                    _ => {
                        return Err(err(
                            "coupled needs a system id, e.g. coupled regen_nozzle_h2_v1".into(),
                        ))
                    }
                };
                doc.coupled_system = Some(sys_id);
            }
            "assert" => {
                let label = match c.next() {
                    Some(Tok::Str(s) | Tok::Ident(s)) => s,
                    _ => return Err(err("assert needs a quoted label, e.g. assert \"Tip deflection\" metric=\"max_displacement\" target=\"0.45\"".into())),
                };
                let mut a = c.attrs("", &let_scope(&doc))?;
                let metric = a
                    .string("metric")?
                    .ok_or_else(|| err("assert needs metric=\"...\"".into()))?;
                let target = match a.take("target") {
                    Some(Some(Val::Str(s))) => s,
                    Some(Some(Val::Num(v))) => n(v),
                    Some(Some(Val::Ident(s))) => s,
                    Some(Some(Val::Expr { txt })) => txt,
                    _ => return Err(err("assert needs target=\"...\" or target=<number>".into())),
                };
                let tolerance = a
                    .string("tol")?
                    .or(a.string("tolerance")?)
                    .unwrap_or_default();
                let unit = a.string("unit")?.unwrap_or_default();
                a.finish()?;
                let item = LiterateAssert {
                    label,
                    metric,
                    target,
                    tolerance,
                    unit,
                };
                if !doc.asserts.iter().any(|x| {
                    x.label == item.label && x.metric == item.metric && x.target == item.target
                }) {
                    doc.asserts.push(item);
                }
            }
            "uncertainty" => {
                if !stack.is_empty() {
                    return Err(err("uncertainty is only allowed at the top level".into()));
                }
                let pos_label = match (c.toks.get(c.pos), c.toks.get(c.pos + 1)) {
                    (Some(Tok::Str(_)), _) => match c.next() {
                        Some(Tok::Str(s)) => Some(s),
                        _ => unreachable!(),
                    },
                    (Some(Tok::Ident(_)), Some(Tok::Sym('='))) => None,
                    (Some(Tok::Ident(_)), _) => match c.next() {
                        Some(Tok::Ident(s)) => Some(s),
                        _ => unreachable!(),
                    },
                    _ => None,
                };
                let mut a = c.attrs("", &let_scope(&doc))?;
                let opt_label = a.string("label")?;
                let param = a
                    .string("param")?
                    .ok_or_else(|| err("uncertainty needs param=\"...\"".into()))?;
                if let Err(m) = crate::param::ParamAddr::parse(param.trim()) {
                    return Err(err(m));
                }
                let std = a.num("std")?.or(a.num("sigma")?);
                let cv = a.num("cv")?;
                if let Some(s) = std {
                    if !s.is_finite() || s < 0.0 {
                        return Err(err("uncertainty 'std' must be finite and >= 0".into()));
                    }
                }
                if let Some(v) = cv {
                    if !v.is_finite() || v < 0.0 {
                        return Err(err("uncertainty 'cv' must be finite and >= 0".into()));
                    }
                }
                if std.unwrap_or(0.0) <= 0.0 && cv.unwrap_or(0.0) <= 0.0 {
                    return Err(err(
                        "uncertainty needs std=<positive> and/or cv=<positive>".into()
                    ));
                }
                let unit = a.string("unit")?.unwrap_or_default();
                a.finish()?;
                let label = pos_label
                    .or(opt_label)
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| param.clone());
                if let Some(slot) = doc
                    .uncertainties
                    .iter_mut()
                    .find(|x| x.param == param && x.label == label)
                {
                    slot.std = std;
                    slot.cv = cv;
                    slot.unit = unit;
                } else {
                    doc.uncertainties.push(UncertaintySpec {
                        label,
                        param,
                        std,
                        cv,
                        unit,
                    });
                }
            }
            "color" => {
                // `#` outside a quoted string starts a comment, so the hex travels quoted
                // in the text form; the document stores it canonical `#RRGGBB[AA]`.
                let raw = match c.next() {
                    Some(Tok::Str(s)) => s,
                    _ => return Err(err("expected a quoted colour like \"#21BB4C\"".into())),
                };
                let col = canonical_color(&raw).ok_or_else(|| {
                    err(format!(
                        "colour \"{raw}\" is not '#' plus 6 or 8 hex digits, e.g. \"#21BB4C\""
                    ))
                })?;
                if matches!(c.peek(), Some(Tok::Ident(t)) if t == "on") {
                    c.next();
                    let target = c.ident("the shape to colour")?;
                    pending_colours.push((col, target));
                } else {
                    return Err(err("name the shape: color \"<hex>\" on <shape>".into()));
                }
            }
            "let" => {
                if !stack.is_empty() {
                    return Err(err("let is only allowed at the top level".into()));
                }
                let name = c.ident("a let name")?;
                // A duplicate catches first; re-use of a statement name reads "Name",
                // matching what the other statement arms say.
                if doc.lets.iter().any(|l| l.name == name) {
                    return Err(err(format!("let '{name}' is defined more than once")));
                }
                if name_is_used(&doc, &name) {
                    return Err(err(format!("Name '{name}' is used more than once")));
                }
                c.sym('=')?;
                let tree = expr_sum(&mut c)?;
                let canon = expr_txt(&tree);
                let value = match eval_expr(&canon, &let_scope(&doc)) {
                    Ok(v) if v.is_finite() => v,
                    Ok(_) => return Err(err(format!("let '{name}': value is not finite"))),
                    Err(m) => return Err(err(format!("let '{name}': {m}"))),
                };
                doc.lets.push(Let {
                    name,
                    expr: canon,
                    value,
                });
            }
            "box" | "cylinder" | "sphere" | "capsule" | "torus" | "extrude" | "revolve"
            | "mesh" | "group" | "extend" => {
                let id = c.ident("a shape name")?;
                // Lets share the statement-name space with shapes, in either direction.
                if doc.lets.iter().any(|l| l.name == id) {
                    return Err(err(format!("Name '{id}' is used more than once")));
                }
                // `extend` takes a positional region name before its (required) dir/len.
                let region = if kw == "extend" {
                    Some(c.ident("a region name")?)
                } else {
                    None
                };
                let mut a = c.attrs(&id, &let_scope(&doc))?;
                let opens = c.is_sym('{');
                let op = if a.flag("subtract") {
                    Op::Subtract
                } else if a.flag("intersect") {
                    Op::Intersect
                } else {
                    a.flag("add");
                    Op::Add
                };
                let blend = a.num_or("blend", 0.0)?;
                let suppressed = a.flag("suppressed");
                // The kind's numbers read first so consumers record in statement order
                // (a driven `size` records before a driven `at`).
                let kind = match kw.as_str() {
                    "box" => NodeKind::Box {
                        size: a
                            .vec3("size")?
                            .ok_or_else(|| err("box needs size=(x, y, z)".into()))?,
                        round: a.expr_num_or("round", "round", 0.0)?,
                    },
                    "cylinder" => NodeKind::Cylinder {
                        radius: a.req_expr_num("r", "radius")?,
                        height: a.req_expr_num("h", "height")?,
                        round: a.expr_num_or("round", "round", 0.0)?,
                    },
                    "sphere" => NodeKind::Sphere {
                        radius: a.req_expr_num("r", "radius")?,
                    },
                    "capsule" => NodeKind::Capsule {
                        radius: a.req_expr_num("r", "radius")?,
                        length: a.req_expr_num("length", "length")?,
                    },
                    "torus" => NodeKind::Torus {
                        major: a.req_expr_num("major", "major")?,
                        minor: a.req_expr_num("minor", "minor")?,
                    },
                    "extrude" => {
                        let height = a.req_expr_num("h", "height")?;
                        let profile = match a.take("profile") {
                            Some(Some(Val::List(items))) => items
                                .into_iter()
                                .map(|v| match v {
                                    Val::Tuple(t) if t.len() == 2 || t.len() == 3 => {
                                        let n: Vec<f64> = t
                                            .iter()
                                            .filter_map(|x| {
                                                if let Val::Num(n) = x {
                                                    Some(*n)
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect();
                                        if n.len() != t.len() {
                                            return Err(err(
                                                "profile points must be numbers".into()
                                            ));
                                        }
                                        Ok(ProfileVertex {
                                            x: n[0],
                                            y: n[1],
                                            bulge: *n.get(2).unwrap_or(&0.0),
                                        })
                                    }
                                    _ => Err(err(
                                        "profile points are (x, y) or (x, y, bulge)".into()
                                    )),
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                            _ => return Err(err("extrude needs profile=[(x, y), …]".into())),
                        };
                        NodeKind::Extrude { profile, height }
                    }
                    "revolve" => {
                        let angle = a.expr_num_or("angle", "angle", 360.0)?;
                        let profile = match a.take("profile") {
                            Some(Some(Val::List(items))) => items
                                .into_iter()
                                .map(|v| match v {
                                    Val::Tuple(t) if t.len() == 2 || t.len() == 3 => {
                                        let n: Vec<f64> = t
                                            .iter()
                                            .filter_map(|x| {
                                                if let Val::Num(n) = x {
                                                    Some(*n)
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect();
                                        if n.len() != t.len() {
                                            return Err(err(
                                                "profile points must be numbers".into()
                                            ));
                                        }
                                        Ok(ProfileVertex {
                                            x: n[0],
                                            y: n[1],
                                            bulge: *n.get(2).unwrap_or(&0.0),
                                        })
                                    }
                                    _ => Err(err(
                                        "profile points are (r, z) or (r, z, bulge)".into()
                                    )),
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                            _ => return Err(err("revolve needs profile=[(r, z), …]".into())),
                        };
                        NodeKind::Revolve { profile, angle }
                    }
                    "mesh" => NodeKind::Mesh {
                        src: a
                            .string("src")?
                            .ok_or_else(|| err("mesh needs src=\"…\"".into()))?,
                        hash: a
                            .string("hash")?
                            .ok_or_else(|| err("mesh needs hash=<16 hex digits>".into()))?,
                        units: a.req_string("units")?,
                        up: a.req_string("up")?,
                        voxel: a.req_num("vox")?,
                        bbox: a.vec6("bbox")?.ok_or_else(|| {
                            err("mesh needs bbox=(xmin, ymin, zmin, xmax, ymax, zmax)".into())
                        })?,
                        tris: a.req_num("tris")?.max(0.0) as u32,
                        patches: a.req_num("patches")?.max(0.0) as u32,
                        open_edges: a.req_num("open_edges")?.max(0.0) as u32,
                    },
                    "extend" => NodeKind::Extend {
                        region: region.ok_or_else(|| err("extend needs a region name".into()))?,
                        dir: a
                            .vec3("dir")?
                            .ok_or_else(|| err("extend needs dir=(x, y, z)".into()))?,
                        len: a.req_num("len")?,
                    },
                    _ => NodeKind::Group {
                        children: Vec::new(),
                        modifiers: Vec::new(),
                    },
                };
                // Placement reads after the kind, for the same consumer order.
                let transform = Transform {
                    translate: a.vec3("at")?.unwrap_or([0.0; 3]),
                    rotate: a.vec3("rotate")?.unwrap_or([0.0; 3]),
                };
                doc.consumed.append(&mut a.consumed);
                a.finish()?;
                let node = Node {
                    id,
                    op,
                    blend,
                    suppressed,
                    transform,
                    material: None,
                    color: None,
                    kind,
                };
                if kw == "group" {
                    if !opens {
                        return Err(err("group needs '{' at the end of the line".into()));
                    }
                    c.next();
                    if !c.done() {
                        return Err(err("nothing may follow '{'".into()));
                    }
                    stack.push((node, Vec::new(), Vec::new()));
                } else {
                    if opens {
                        return Err(err("only groups can contain shapes".into()));
                    }
                    push_node(&mut doc, &mut stack, node);
                }
            }
            "shell" | "offset" | "mirror" | "linear" | "circular" => {
                let modifier = parse_modifier(&kw, &mut c)?;
                match stack.last_mut() {
                    Some((_, _, mods)) => mods.push(modifier),
                    None => return Err(err("modifiers must be inside a group".into())),
                }
            }
            "region" => {
                let id = c.ident("a region name")?;
                if doc.lets.iter().any(|l| l.name == id) {
                    return Err(err(format!("Name '{id}' is used more than once")));
                }
                c.sym('=')?;
                let mut selectors = Vec::new();
                loop {
                    let shape = c.ident("a shape name")?;
                    // `.p3` selects a surface patch of a mesh node (`face` stays All and the
                    // solver ignores it); other names after the dot must be face names.
                    let sel = if c.is_sym('.') {
                        c.next();
                        let f = c.ident("a face name")?;
                        match f.strip_prefix('p').and_then(|r| r.parse::<u32>().ok()) {
                            Some(p) => Selector {
                                shape,
                                face: Face::All,
                                patch: Some(p),
                            },
                            None => Selector {
                                shape,
                                face: Face::parse(&f)
                                    .ok_or_else(|| err(format!("unknown face '{f}'")))?,
                                patch: None,
                            },
                        }
                    } else {
                        Selector {
                            shape,
                            face: Face::All,
                            patch: None,
                        }
                    };
                    selectors.push(sel);
                    if c.is_sym('+') {
                        c.next();
                    } else {
                        break;
                    }
                }
                if !c.done() {
                    return Err(err("unexpected text after region".into()));
                }
                doc.regions.push(Region { id, selectors });
            }
            "support" => {
                let id = c.ident("a support name")?;
                if doc.lets.iter().any(|l| l.name == id) {
                    return Err(err(format!("Name '{id}' is used more than once")));
                }
                let region = c.ident("a region name")?;
                let mut a = c.attrs(&id, &let_scope(&doc))?;
                let fixed = match a.string("fixed")? {
                    None => [true; 3],
                    Some(s) => {
                        if s.is_empty() || !s.chars().all(|ch| "xyz".contains(ch)) {
                            return Err(err("fixed= takes a combination of x, y, z".into()));
                        }
                        [s.contains('x'), s.contains('y'), s.contains('z')]
                    }
                };
                a.finish()?;
                doc.supports.push(Support { id, region, fixed });
            }
            "load" => {
                let id = c.ident("a load name")?;
                if doc.lets.iter().any(|l| l.name == id) {
                    return Err(err(format!("Name '{id}' is used more than once")));
                }
                let region = c.ident("a region name")?;
                let mut a = c.attrs(&id, &let_scope(&doc))?;
                let case = a.string("case")?.unwrap_or_else(|| "Load 1".into());
                let p_num = a.num("pressure")?;
                let p_fn = a.string("pressure_fn")?;
                let kind = if let Some(v) = a.vec3("force")? {
                    if p_num.is_some() || p_fn.is_some() {
                        return Err(err(
                            "load cannot combine force= with pressure=/pressure_fn=".into(),
                        ));
                    }
                    LoadKind::Force { vector: v }
                } else if p_num.is_some() || p_fn.is_some() {
                    if let Some(ref src) = p_fn {
                        if let Err(e) =
                            crate::spatial::SpatialFn::compile_with_lets(src, &let_scope(&doc))
                        {
                            return Err(err(format!("pressure_fn: {e}")));
                        }
                    }
                    LoadKind::Pressure {
                        mpa: p_num.unwrap_or(0.0),
                        expr: p_fn,
                    }
                } else if let Some(n) = a.expr_num("preload", "newtons")? {
                    let axis = a.vec3("axis")?.unwrap_or([0.0; 3]);
                    LoadKind::Preload { newtons: n, axis }
                } else if let Some(src) = a.string("fn")? {
                    // Compiled here, so a bad function names its character, not a
                    // distant validate error.
                    if let Err(e) =
                        crate::loaddist::LoadFn::compile_with_lets(&src, &let_scope(&doc))
                    {
                        return Err(err(format!("fn: {e}")));
                    }
                    let (dir_v, local_v) = (a.vec3("dir")?, a.vec3("local")?);
                    let dir = match (dir_v, local_v) {
                        (Some(_), Some(_)) => {
                            return Err(err(
                                "load has both dir= and local= — give one direction".into()
                            ))
                        }
                        (Some(v), None) => LoadDir::Global { v },
                        (None, Some(v)) => LoadDir::Local { v },
                        (None, None) => {
                            return Err(err(
                                "a load with fn= needs dir=(x, y, z) or local=(lx, ly, ln)".into(),
                            ))
                        }
                    };
                    let total = a.expr_num("total", "total")?;
                    LoadKind::Distributed {
                        expr: src,
                        dir,
                        total,
                    }
                } else {
                    return Err(err(
                        "load needs force=(x, y, z), pressure=value, preload=value, or fn=..."
                            .into(),
                    ));
                };
                doc.consumed.append(&mut a.consumed);
                a.finish()?;
                doc.loads.push(Load {
                    id,
                    region,
                    case,
                    kind,
                });
            }
            "temperature" => {
                let id = c.ident("a temperature name")?;
                if doc.lets.iter().any(|l| l.name == id) {
                    return Err(err(format!("Name '{id}' is used more than once")));
                }
                let region = c.ident("a region name")?;
                let scope = let_scope(&doc);
                let mut a = c.attrs(&id, &scope)?;
                let expr = a.string("temp_fn")?;
                if let Some(ref src) = expr {
                    if let Err(e) = crate::spatial::SpatialFn::compile_with_lets(src, &scope) {
                        return Err(err(format!("temp_fn: {e}")));
                    }
                }
                let celsius = if expr.is_some() {
                    a.opt_compound_num("temp", "celsius")?.unwrap_or(0.0)
                } else {
                    a.req_compound_num("temp", "celsius")?
                };
                let case = a.string("case")?.unwrap_or_else(|| "Thermal 1".into());
                doc.consumed.append(&mut a.consumed);
                a.finish()?;
                doc.temperatures.push(Temperature {
                    id,
                    region,
                    celsius,
                    expr,
                    case,
                });
            }
            "convection" => {
                let id = c.ident("a convection name")?;
                if doc.lets.iter().any(|l| l.name == id) {
                    return Err(err(format!("Name '{id}' is used more than once")));
                }
                let region = c.ident("a region name")?;
                let scope = let_scope(&doc);
                let mut a = c.attrs(&id, &scope)?;
                let h_expr = a.string("h_fn")?;
                if let Some(ref src) = h_expr {
                    if let Err(e) = crate::spatial::SpatialFn::compile_with_lets(src, &scope) {
                        return Err(err(format!("h_fn: {e}")));
                    }
                }
                let t_inf_expr = a.string("tinf_fn")?;
                if let Some(ref src) = t_inf_expr {
                    if let Err(e) = crate::spatial::SpatialFn::compile_with_lets(src, &scope) {
                        return Err(err(format!("tinf_fn: {e}")));
                    }
                }
                let h = if h_expr.is_some() {
                    a.opt_compound_num("h", "h")?.unwrap_or(0.0)
                } else {
                    a.req_compound_num("h", "h")?
                };
                let t_inf = if t_inf_expr.is_some() {
                    a.opt_compound_num("tinf", "tinf")?.unwrap_or(0.0)
                } else {
                    a.req_compound_num("tinf", "tinf")?
                };
                let case = a.string("case")?.unwrap_or_else(|| "Thermal 1".into());
                doc.consumed.append(&mut a.consumed);
                a.finish()?;
                doc.convections.push(Convection {
                    id,
                    region,
                    h,
                    h_expr,
                    t_inf,
                    t_inf_expr,
                    case,
                });
            }
            "heatflux" => {
                let id = c.ident("a heat flux name")?;
                if doc.lets.iter().any(|l| l.name == id) {
                    return Err(err(format!("Name '{id}' is used more than once")));
                }
                let region = c.ident("a region name")?;
                let scope = let_scope(&doc);
                let mut a = c.attrs(&id, &scope)?;
                let expr = a.string("q_fn")?;
                if let Some(ref src) = expr {
                    if let Err(e) = crate::spatial::SpatialFn::compile_with_lets(src, &scope) {
                        return Err(err(format!("q_fn: {e}")));
                    }
                }
                let q = if expr.is_some() {
                    a.opt_compound_num("q", "q")?.unwrap_or(0.0)
                } else {
                    a.req_compound_num("q", "q")?
                };
                let case = a.string("case")?.unwrap_or_else(|| "Thermal 1".into());
                doc.consumed.append(&mut a.consumed);
                a.finish()?;
                doc.heat_fluxes.push(HeatFlux {
                    id,
                    region,
                    q,
                    expr,
                    case,
                });
            }
            "fill" => {
                let family = match c.ident("a lattice family")?.as_str() {
                    "gyroid" => Family::Gyroid,
                    "primitive" => Family::Primitive,
                    "diamond" => Family::Diamond,
                    f => return Err(err(format!("unknown lattice family '{f}'"))),
                };
                let mode = match c.ident("sheet or network")?.as_str() {
                    "sheet" => FillMode::Sheet,
                    "network" => FillMode::Network,
                    m => return Err(err(format!("unknown fill mode '{m}'"))),
                };
                let mut a = c.attrs("", &let_scope(&doc))?;
                let d = Fill::default();
                let (rho_min, rho_max) = a.pair("rho")?.unwrap_or((d.rho_min, d.rho_max));
                let morph = match a.string("morph")?.as_deref() {
                    None => None,
                    Some("gyroid") => Some(Family::Gyroid),
                    Some("primitive") | Some("schwarz_p") => Some(Family::Primitive),
                    Some("diamond") => Some(Family::Diamond),
                    Some(m) => return Err(err(format!("unknown morph lattice family '{m}'"))),
                };
                let fill = Fill {
                    family,
                    mode,
                    cell: a.num_or("cell", d.cell)?,
                    cell_min: a.num("cell_min")?,
                    morph,
                    skin: a.num_or("skin", d.skin)?,
                    skin_max: a.num("skin_max")?,
                    erode: a.num("erode")?.filter(|&er| er != 0.0),
                    rho_min,
                    rho_max,
                    curve: a.num_or("curve", d.curve)?,
                    min_wall: a.num_or("wall", d.min_wall)?,
                    protect: a.idents("protect")?,
                    margin: a.num_or("margin", d.margin)?,
                    case: a.string("case")?,
                    rotate: a.vec3("rotate")?.unwrap_or(d.rotate),
                };
                a.finish()?;
                doc.fill = Some(fill);
            }
            "solve" => {
                let mut a = c.attrs("", &let_scope(&doc))?;
                doc.solve.size = match a.take("size") {
                    None => None,
                    Some(Some(Val::Ident(s))) if s == "auto" => None,
                    Some(Some(Val::Num(v))) => Some(v),
                    Some(_) => return Err(err("size must be a number or 'auto'".into())),
                };
                doc.solve.modes = match a.take("modes") {
                    None => None,
                    Some(None) => return Err(err("modes wants a count, e.g. modes=3".into())),
                    Some(Some(Val::Num(v))) => Some(v.round().max(1.0) as usize),
                    Some(Some(_)) => return Err(err("modes must be a count, e.g. modes=3".into())),
                };
                doc.solve.t_ref = match a.take("tref") {
                    None => None,
                    Some(None) => return Err(err("tref wants a °C value, e.g. tref=20".into())),
                    Some(Some(Val::Num(v))) => Some(v),
                    Some(Some(_)) => return Err(err("tref must be a number, e.g. tref=20".into())),
                };
                doc.solve.build = a.pair("build")?.map(|(th, ph)| [th, ph]);
                doc.solve.plasticity = match a.string("plasticity")?.as_deref() {
                    None | Some("none") | Some("linear") => None,
                    Some("j2") | Some("j2_secant") => Some(PlasticityLaw::J2),
                    Some(other) => {
                        return Err(err(format!(
                            "unknown plasticity law '{other}' (expected j2 or linear)"
                        )))
                    }
                };
                doc.solve.hardening_n = a.num("hardening_n")?;
                doc.solve.t_end = a.num("t_end")?;
                doc.solve.steps = match a.take("steps") {
                    None => None,
                    Some(None) => return Err(err("steps wants a count, e.g. steps=20".into())),
                    Some(Some(Val::Num(v))) if v.is_finite() && v >= 1.0 => {
                        Some(v.round() as usize)
                    }
                    Some(Some(_)) => {
                        return Err(err("steps must be a positive integer, e.g. steps=20".into()))
                    }
                };
                doc.solve.t_init = match a.take("t_init") {
                    None => None,
                    Some(None) => {
                        return Err(err("t_init wants a °C value, e.g. t_init=300".into()))
                    }
                    Some(Some(Val::Num(v))) => Some(v),
                    Some(Some(_)) => {
                        return Err(err("t_init must be a number, e.g. t_init=300".into()))
                    }
                };
                a.finish()?;
            }
            "custommaterial" => {
                let id = c.ident("a material id")?;
                let mut a = c.attrs(&id, &let_scope(&doc))?;
                let role = match a.string("role")?.as_deref() {
                    None | Some("structural") | Some("solid") => MaterialRole::Structural,
                    Some("fluid") | Some("non_structural") => MaterialRole::Fluid,
                    Some(other) => {
                        return Err(err(format!(
                            "unknown material role '{other}' (expected structural or fluid)"
                        )))
                    }
                };
                let mat = Material {
                    name: a.string("name")?.unwrap_or_else(|| id.clone()),
                    id,
                    e: a.req_num("e")?,
                    nu: a.req_num("nu")?,
                    yield_strength: a.req_num("yield")?,
                    density: a.req_num("density")?,
                    min_wall: a.num_or("wall", 0.8)?,
                    // Thermal expansion 1/°C (e.g. 2e-5 for an aluminium class); optional,
                    // and a thermal document refuses a structural material without one.
                    alpha: a.num("a")?,
                    k: a.num("k")?,
                    cp: a.num("cp")?,
                    yield_z: a.num("yield_z")?,
                    shear_z: a.num("shear_z")?,
                    hardening_n: a.num("hardening_n")?,
                    hardening_k: a.num("hardening_k")?,
                    weibull_m: a.num("weibull_m")?,
                    role,
                    source: a
                        .string("source")?
                        .unwrap_or_else(|| "User supplied".into()),
                };
                a.finish()?;
                doc.custom_materials.push(mat);
            }
            "sketch" => {
                if !stack.is_empty() {
                    return Err(err("sketch is only allowed at the top level".into()));
                }
                let id = c.ident("a sketch name")?;
                if doc.sketches.iter().any(|s| s.id == id) {
                    return Err(err(format!("Sketch '{id}' is defined more than once")));
                }
                let mut a = c.attrs("", &[])?;
                let opens = c.is_sym('{');
                let offset = a.num_or("offset", 0.0)?;
                let plane = match a.string("on")?.as_deref() {
                    None | Some("xy") => crate::sketch::SketchPlane::Xy { offset },
                    Some("xz") => crate::sketch::SketchPlane::Xz { offset },
                    Some("yz") => crate::sketch::SketchPlane::Yz { offset },
                    Some(spec) => crate::sketch::SketchPlane::parse_spec(spec, offset)
                        .map_err(|m| err(format!("sketch '{id}': {m}")))?,
                };
                a.finish()?;
                if !opens {
                    return Err(err("sketch needs '{' at the end of the line".into()));
                }
                c.next();
                if !c.done() {
                    return Err(err("nothing may follow '{'".into()));
                }
                open_sketch = Some(crate::sketch::ParametricSketch {
                    id,
                    plane,
                    points: Vec::new(),
                    segments: Vec::new(),
                    circles: Vec::new(),
                    constraints: Vec::new(),
                    macros: Vec::new(),
                });
            }
            "sketchfeature" => {
                if !stack.is_empty() {
                    return Err(err("sketchfeature is only allowed at the top level".into()));
                }
                let binding = parse_sketch_feature(&mut c)?;
                let feat_id = binding.feature_id.clone();
                doc.sketch_features.push(binding);
                // If the `.fsm` file hand-authored `sketch` + `sketchfeature` without an
                // explicit `extrude`/`revolve` node yet, materialize it immediately so
                // downstream `region` statements can reference `<feat_id>.top` etc.
                if doc.find(&feat_id).is_none() {
                    crate::sketch::sync_sketch_features(&mut doc).map_err(err)?;
                }
            }
            "system" => {
                if !stack.is_empty() {
                    return Err(err("system is only allowed at the top level".into()));
                }
                if doc.system_block.is_some() {
                    return Err(err("at most one 'system' block may be declared".into()));
                }
                let id = c.ident("a system identifier")?;
                let mut a = c.attrs("", &[])?;
                let opens = c.is_sym('{');
                let title = a.string("title")?.unwrap_or_default();
                let sector = a.string("sector")?.unwrap_or_default();
                let nodes_f = a.num_or("nodes", 1.0)?;
                if nodes_f < 1.0 || nodes_f.fract() != 0.0 || nodes_f > 512.0 {
                    return Err(err(format!(
                        "system '{id}': nodes must be an integer in 1..=512"
                    )));
                }
                let span = a.signed_pair("span")?.unwrap_or([0.0, 1.0]);
                let axis = match a.string("axis")?.as_deref() {
                    None => crate::system::DeclAxis::Z,
                    Some(ax) => crate::system::DeclAxis::parse(ax)
                        .ok_or_else(|| err(format!("system '{id}': axis must be x, y, z, or r")))?,
                };
                a.finish()?;
                if !opens {
                    return Err(err("system needs '{' at the end of the line".into()));
                }
                c.next();
                if !c.done() {
                    return Err(err("nothing may follow '{'".into()));
                }
                open_system = Some(crate::system::DeclarativeSystem {
                    id,
                    title,
                    sector,
                    nodes: nodes_f as usize,
                    span,
                    axis,
                    designs: Vec::new(),
                    epistemics: Vec::new(),
                    states: Vec::new(),
                    aux: Vec::new(),
                    residuals: Vec::new(),
                    objectives: Vec::new(),
                    audits: Vec::new(),
                    bindings: Vec::new(),
                    scenarios: Vec::new(),
                });
            }
            other => return Err(err(format!("unknown keyword '{other}'"))),
        }
        if !c.done() && !matches!(kw.as_str(), "group" | "sketch" | "system") {
            return Err(err("unexpected text at end of line".into()));
        }
    }
    if !saw_header {
        return Err(ParseError {
            line: 1,
            message: "empty document".into(),
        });
    }
    if open_sketch.is_some() || open_system.is_some() || !stack.is_empty() {
        return Err(ParseError {
            line: text.lines().count(),
            message: "missing '}'".into(),
        });
    }
    for (mat, target) in &pending_labels {
        if !doc.set_node_material(target, mat) {
            return Err(ParseError {
                line: text.lines().count(),
                message: format!(
                    "material '{mat}' labels shape '{target}', but no shape is named '{target}'"
                ),
            });
        }
    }
    for (col, target) in &pending_colours {
        if !doc.set_node_color(target, Some(col.clone())) {
            return Err(ParseError {
                line: text.lines().count(),
                message: format!(
                    "colour '{col}' colours shape '{target}', but no shape is named '{target}'"
                ),
            });
        }
    }
    Ok(doc)
}

fn push_node(doc: &mut Document, stack: &mut [(Node, Vec<Node>, Vec<Modifier>)], node: Node) {
    match stack.last_mut() {
        Some((_, children, _)) => children.push(node),
        None => doc.root.push(node),
    }
}

/// The scope in force after the lets parsed so far: names and values in declaration
/// order, so a consumer expression sees exactly the lets preceding its statement.
fn let_scope(doc: &Document) -> Vec<(String, f64)> {
    doc.lets.iter().map(|l| (l.name.clone(), l.value)).collect()
}

fn node_uses(node: &Node, id: &str) -> bool {
    let mut any = node.id == *id;
    if let NodeKind::Group { children, .. } = &node.kind {
        any |= children.iter().any(|c| node_uses(c, id));
    }
    any
}

/// Whether `id` is already a statement name in the document (shapes recursively,
/// then regions, supports, loads, temperatures, convections, heat fluxes). Custom
/// materials sit outside this name space.
fn name_is_used(doc: &Document, id: &str) -> bool {
    doc.root.iter().any(|n| node_uses(n, id))
        || doc.regions.iter().any(|r| r.id == *id)
        || doc.supports.iter().any(|s| s.id == *id)
        || doc.loads.iter().any(|l| l.id == *id)
        || doc.temperatures.iter().any(|t| t.id == *id)
        || doc.convections.iter().any(|c| c.id == *id)
        || doc.heat_fluxes.iter().any(|f| f.id == *id)
}

fn parse_axis(c: &mut Cursor) -> Result<Axis, ParseError> {
    match c.ident("x, y or z")?.as_str() {
        "x" => Ok(Axis::X),
        "y" => Ok(Axis::Y),
        "z" => Ok(Axis::Z),
        _ => c.err("expected x, y or z"),
    }
}

fn ident_or_str(c: &mut Cursor, what: &str) -> Result<String, ParseError> {
    match c.next() {
        Some(Tok::Ident(s) | Tok::Str(s)) => Ok(s),
        _ => c.err(format!("expected {what}")),
    }
}

fn id_tok(s: &str) -> String {
    if is_ident(s) {
        s.to_string()
    } else {
        q(s)
    }
}

fn parse_sketch_body_line(
    sk: &mut crate::sketch::ParametricSketch,
    c: &mut Cursor,
) -> Result<(), ParseError> {
    use crate::sketch::{
        SketchCircle, SketchConstraint, SketchConstraintKind, SketchMacro, SketchPoint,
        SketchSegment,
    };
    let kw = c.ident("point, segment, circle, constraint, or macro")?;
    let line = c.line;
    let err = |m: String| ParseError { line, message: m };
    match kw.as_str() {
        "point" => {
            let id = ident_or_str(c, "a point id")?;
            let mut a = c.attrs("", &[])?;
            let (x, y) = a
                .pair("at")?
                .ok_or_else(|| err("sketch point needs at=(x, y)".into()))?;
            let fixed = a.flag("fixed");
            let construction = a.flag("construction");
            a.finish()?;
            sk.points.push(SketchPoint {
                id,
                x,
                y,
                fixed,
                construction,
            });
        }
        "segment" => {
            let id = ident_or_str(c, "a segment id")?;
            let mut a = c.attrs("", &[])?;
            let p1 = a.req_string("p1")?;
            let p2 = a.req_string("p2")?;
            let bulge = a.num_or("bulge", 0.0)?;
            let construction = a.flag("construction");
            a.finish()?;
            sk.segments.push(SketchSegment {
                id,
                p1,
                p2,
                bulge,
                construction,
            });
        }
        "circle" => {
            let id = ident_or_str(c, "a circle id")?;
            let mut a = c.attrs("", &[])?;
            let center = a.req_string("center")?;
            let radius = a.req_num("r")?;
            let construction = a.flag("construction");
            a.finish()?;
            sk.circles.push(SketchCircle {
                id,
                center,
                radius,
                construction,
            });
        }
        "constraint" => {
            let id = ident_or_str(c, "a constraint id")?;
            let mut a = c.attrs("", &[])?;
            let kind_str = a.req_string("kind")?;
            let kind = match kind_str.as_str() {
                "coincident" => SketchConstraintKind::Coincident {
                    p1: a.req_string("p1")?,
                    p2: a.req_string("p2")?,
                },
                "fixed" => {
                    let point = a.req_string("point")?;
                    let (x, y) = a
                        .pair("at")?
                        .ok_or_else(|| err("fixed constraint needs at=(x, y)".into()))?;
                    SketchConstraintKind::Fixed { point, x, y }
                }
                "horizontal" => SketchConstraintKind::Horizontal {
                    segment: a.string("segment")?,
                    p1: a.string("p1")?,
                    p2: a.string("p2")?,
                },
                "vertical" => SketchConstraintKind::Vertical {
                    segment: a.string("segment")?,
                    p1: a.string("p1")?,
                    p2: a.string("p2")?,
                },
                "parallel" => SketchConstraintKind::Parallel {
                    seg1: a.req_string("seg1")?,
                    seg2: a.req_string("seg2")?,
                },
                "perpendicular" => SketchConstraintKind::Perpendicular {
                    seg1: a.req_string("seg1")?,
                    seg2: a.req_string("seg2")?,
                },
                "equal_length" => SketchConstraintKind::EqualLength {
                    seg1: a.req_string("seg1")?,
                    seg2: a.req_string("seg2")?,
                },
                "tangent" => SketchConstraintKind::Tangent {
                    seg: a.req_string("seg")?,
                    target: a.req_string("target")?,
                },
                "concentric" => SketchConstraintKind::Concentric {
                    c1: a.req_string("c1")?,
                    c2: a.req_string("c2")?,
                },
                "midpoint" => SketchConstraintKind::Midpoint {
                    point: a.req_string("point")?,
                    segment: a.string("segment")?,
                    p1: a.string("p1")?,
                    p2: a.string("p2")?,
                },
                "symmetric" => SketchConstraintKind::Symmetric {
                    p1: a.req_string("p1")?,
                    p2: a.req_string("p2")?,
                    axis: a.req_string("axis")?,
                },
                "point_on_line" => SketchConstraintKind::PointOnLine {
                    point: a.req_string("point")?,
                    segment: a.req_string("segment")?,
                },
                "distance" => SketchConstraintKind::Distance {
                    segment: a.string("segment")?,
                    p1: a.string("p1")?,
                    p2: a.string("p2")?,
                    value: a.req_num("value")?,
                    expr: a.string("expr")?,
                },
                "horizontal_distance" => SketchConstraintKind::HorizontalDistance {
                    segment: a.string("segment")?,
                    p1: a.string("p1")?,
                    p2: a.string("p2")?,
                    value: a.req_num("value")?,
                    expr: a.string("expr")?,
                },
                "vertical_distance" => SketchConstraintKind::VerticalDistance {
                    segment: a.string("segment")?,
                    p1: a.string("p1")?,
                    p2: a.string("p2")?,
                    value: a.req_num("value")?,
                    expr: a.string("expr")?,
                },
                "angle" => SketchConstraintKind::Angle {
                    seg1: a.req_string("seg1")?,
                    seg2: a.string("seg2")?,
                    degrees: a.req_num("degrees")?,
                    expr: a.string("expr")?,
                },
                "radius" => SketchConstraintKind::Radius {
                    target: a.req_string("target")?,
                    value: a.req_num("value")?,
                    expr: a.string("expr")?,
                },
                "diameter" => SketchConstraintKind::Diameter {
                    target: a.req_string("target")?,
                    value: a.req_num("value")?,
                    expr: a.string("expr")?,
                },
                other => return Err(err(format!("unknown sketch constraint kind '{other}'"))),
            };
            a.finish()?;
            sk.constraints.push(SketchConstraint { id, kind });
        }
        "macro" => {
            let mkind = c.ident("a macro kind")?;
            let mut a = c.attrs("", &[])?;
            let mac = match mkind.as_str() {
                "rect" => {
                    let id = a.req_string("id")?;
                    let (x, y) = a
                        .pair("at")?
                        .ok_or_else(|| err("macro rect needs at=(x, y)".into()))?;
                    let (width, height) = a
                        .pair("size")?
                        .ok_or_else(|| err("macro rect needs size=(w, h)".into()))?;
                    let fillet = a.num_or("fillet", 0.0)?;
                    let construction = a.flag("construction");
                    SketchMacro::Rect {
                        id,
                        x,
                        y,
                        width,
                        height,
                        fillet,
                        construction,
                    }
                }
                "center_rect" => {
                    let id = a.req_string("id")?;
                    let (cx, cy) = a
                        .pair("at")?
                        .ok_or_else(|| err("macro center_rect needs at=(cx, cy)".into()))?;
                    let (width, height) = a
                        .pair("size")?
                        .ok_or_else(|| err("macro center_rect needs size=(w, h)".into()))?;
                    let fillet = a.num_or("fillet", 0.0)?;
                    let construction = a.flag("construction");
                    SketchMacro::CenterRect {
                        id,
                        cx,
                        cy,
                        width,
                        height,
                        fillet,
                        construction,
                    }
                }
                "circle" => {
                    let id = a.req_string("id")?;
                    let (cx, cy) = a
                        .pair("at")?
                        .ok_or_else(|| err("macro circle needs at=(cx, cy)".into()))?;
                    let radius = a.req_num("r")?;
                    let construction = a.flag("construction");
                    SketchMacro::Circle {
                        id,
                        cx,
                        cy,
                        radius,
                        construction,
                    }
                }
                "slot" => {
                    let id = a.req_string("id")?;
                    let (x1, y1) = a
                        .pair("p1")?
                        .ok_or_else(|| err("macro slot needs p1=(x1, y1)".into()))?;
                    let (x2, y2) = a
                        .pair("p2")?
                        .ok_or_else(|| err("macro slot needs p2=(x2, y2)".into()))?;
                    let radius = a.req_num("r")?;
                    SketchMacro::Slot {
                        id,
                        x1,
                        y1,
                        x2,
                        y2,
                        radius,
                    }
                }
                "polygon" => {
                    let id = a.req_string("id")?;
                    let (cx, cy) = a
                        .pair("at")?
                        .ok_or_else(|| err("macro polygon needs at=(cx, cy)".into()))?;
                    let radius = a.req_num("r")?;
                    let sides = a.req_num("sides")?.max(3.0) as usize;
                    let angle_deg = a.num_or("angle", 0.0)?;
                    let construction = a.flag("construction");
                    SketchMacro::Polygon {
                        id,
                        cx,
                        cy,
                        radius,
                        sides,
                        angle_deg,
                        construction,
                    }
                }
                "bolt_circle" => {
                    let id = a.req_string("id")?;
                    let (cx, cy) = a
                        .pair("at")?
                        .ok_or_else(|| err("macro bolt_circle needs at=(cx, cy)".into()))?;
                    let pcd = a.req_num("pcd")?;
                    let count = a.req_num("count")?.max(1.0) as usize;
                    let hole_diameter = a.req_num("hole_d")?;
                    let start_angle_deg = a.num_or("start_angle", 0.0)?;
                    SketchMacro::BoltCircle {
                        id,
                        cx,
                        cy,
                        pcd,
                        count,
                        hole_diameter,
                        start_angle_deg,
                    }
                }
                "fillet_corner" => SketchMacro::FilletCorner {
                    point_id: a.req_string("point")?,
                    radius: a.req_num("r")?,
                },
                other => return Err(err(format!("unknown sketch macro '{other}'"))),
            };
            a.finish()?;
            sk.macros.push(mac);
        }
        other => return Err(err(format!("unexpected keyword '{other}' inside sketch"))),
    }
    Ok(())
}

fn parse_sketch_feature(c: &mut Cursor) -> Result<crate::sketch::SketchFeatureBinding, ParseError> {
    use crate::sketch::{SketchFeatureBinding, SketchFeatureOp};
    let line = c.line;
    let err = |m: String| ParseError { line, message: m };
    let feature_id = c.ident("a feature shape id")?;
    let mut a = c.attrs("", &[])?;
    let sketch_id = a.req_string("sketch")?;
    let kind_str = a.req_string("kind")?;
    let associate_with_host = a.flag("associate_host");
    let operation = match kind_str.as_str() {
        "boss_extrude" => SketchFeatureOp::BossExtrude {
            height: a.req_num("h")?,
            blend: a.num_or("blend", 0.0)?,
            symmetric: a.flag("symmetric"),
        },
        "pocket_cut" => SketchFeatureOp::PocketCut {
            depth: a.num_or("depth", 10.0)?,
            through_all: a.flag("through_all"),
            blend: a.num_or("blend", 0.0)?,
        },
        "revolve_boss" => SketchFeatureOp::RevolveBoss {
            angle_deg: a.num_or("angle", 360.0)?,
            blend: a.num_or("blend", 0.0)?,
        },
        "revolve_cut" => SketchFeatureOp::RevolveCut {
            angle_deg: a.num_or("angle", 360.0)?,
            blend: a.num_or("blend", 0.0)?,
        },
        other => return Err(err(format!("unknown sketchfeature kind '{other}'"))),
    };
    a.finish()?;
    Ok(SketchFeatureBinding {
        feature_id,
        sketch_id,
        operation,
        associate_with_host,
    })
}

fn print_sketch(o: &mut String, sk: &crate::sketch::ParametricSketch) {
    use crate::sketch::{SketchConstraintKind, SketchMacro, SketchPlane};
    let mut hdr = format!("sketch {}", id_tok(&sk.id));
    match &sk.plane {
        SketchPlane::Xy { offset } => {
            if *offset != 0.0 {
                hdr += &format!(" offset={}", n(*offset));
            }
        }
        SketchPlane::Xz { offset } => {
            hdr += " on=xz";
            if *offset != 0.0 {
                hdr += &format!(" offset={}", n(*offset));
            }
        }
        SketchPlane::Yz { offset } => {
            hdr += " on=yz";
            if *offset != 0.0 {
                hdr += &format!(" offset={}", n(*offset));
            }
        }
        SketchPlane::OnFace {
            host_id,
            face,
            offset,
        } => {
            hdr += &format!(" on={}", q(&format!("{}.{}", host_id, face.name())));
            if *offset != 0.0 {
                hdr += &format!(" offset={}", n(*offset));
            }
        }
    }
    let _ = writeln!(o, "{hdr} {{");
    for pt in &sk.points {
        let mut line = format!("  point {} at=({}, {})", id_tok(&pt.id), n(pt.x), n(pt.y));
        if pt.fixed {
            line += " fixed";
        }
        if pt.construction {
            line += " construction";
        }
        let _ = writeln!(o, "{line}");
    }
    for seg in &sk.segments {
        let mut line = format!(
            "  segment {} p1={} p2={}",
            id_tok(&seg.id),
            id_tok(&seg.p1),
            id_tok(&seg.p2)
        );
        if seg.bulge != 0.0 {
            line += &format!(" bulge={}", n(seg.bulge));
        }
        if seg.construction {
            line += " construction";
        }
        let _ = writeln!(o, "{line}");
    }
    for cir in &sk.circles {
        let mut line = format!(
            "  circle {} center={} r={}",
            id_tok(&cir.id),
            id_tok(&cir.center),
            n(cir.radius)
        );
        if cir.construction {
            line += " construction";
        }
        let _ = writeln!(o, "{line}");
    }
    for c in &sk.constraints {
        let mut line = format!("  constraint {} ", id_tok(&c.id));
        match &c.kind {
            SketchConstraintKind::Coincident { p1, p2 } => {
                let _ = write!(line, "kind=coincident p1={} p2={}", id_tok(p1), id_tok(p2));
            }
            SketchConstraintKind::Fixed { point, x, y } => {
                let _ = write!(
                    line,
                    "kind=fixed point={} at=({}, {})",
                    id_tok(point),
                    n(*x),
                    n(*y)
                );
            }
            SketchConstraintKind::Horizontal { segment, p1, p2 } => {
                line += "kind=horizontal";
                if let Some(s) = segment {
                    line += &format!(" segment={}", id_tok(s));
                }
                if let Some(a) = p1 {
                    line += &format!(" p1={}", id_tok(a));
                }
                if let Some(b) = p2 {
                    line += &format!(" p2={}", id_tok(b));
                }
            }
            SketchConstraintKind::Vertical { segment, p1, p2 } => {
                line += "kind=vertical";
                if let Some(s) = segment {
                    line += &format!(" segment={}", id_tok(s));
                }
                if let Some(a) = p1 {
                    line += &format!(" p1={}", id_tok(a));
                }
                if let Some(b) = p2 {
                    line += &format!(" p2={}", id_tok(b));
                }
            }
            SketchConstraintKind::Parallel { seg1, seg2 } => {
                let _ = write!(
                    line,
                    "kind=parallel seg1={} seg2={}",
                    id_tok(seg1),
                    id_tok(seg2)
                );
            }
            SketchConstraintKind::Perpendicular { seg1, seg2 } => {
                let _ = write!(
                    line,
                    "kind=perpendicular seg1={} seg2={}",
                    id_tok(seg1),
                    id_tok(seg2)
                );
            }
            SketchConstraintKind::EqualLength { seg1, seg2 } => {
                let _ = write!(
                    line,
                    "kind=equal_length seg1={} seg2={}",
                    id_tok(seg1),
                    id_tok(seg2)
                );
            }
            SketchConstraintKind::Tangent { seg, target } => {
                let _ = write!(
                    line,
                    "kind=tangent seg={} target={}",
                    id_tok(seg),
                    id_tok(target)
                );
            }
            SketchConstraintKind::Concentric { c1, c2 } => {
                let _ = write!(line, "kind=concentric c1={} c2={}", id_tok(c1), id_tok(c2));
            }
            SketchConstraintKind::Midpoint {
                point,
                segment,
                p1,
                p2,
            } => {
                let _ = write!(line, "kind=midpoint point={}", id_tok(point));
                if let Some(s) = segment {
                    line += &format!(" segment={}", id_tok(s));
                }
                if let Some(a) = p1 {
                    line += &format!(" p1={}", id_tok(a));
                }
                if let Some(b) = p2 {
                    line += &format!(" p2={}", id_tok(b));
                }
            }
            SketchConstraintKind::Symmetric { p1, p2, axis } => {
                let _ = write!(
                    line,
                    "kind=symmetric p1={} p2={} axis={}",
                    id_tok(p1),
                    id_tok(p2),
                    id_tok(axis)
                );
            }
            SketchConstraintKind::PointOnLine { point, segment } => {
                let _ = write!(
                    line,
                    "kind=point_on_line point={} segment={}",
                    id_tok(point),
                    id_tok(segment)
                );
            }
            SketchConstraintKind::Distance {
                segment,
                p1,
                p2,
                value,
                expr,
            } => {
                line += "kind=distance";
                if let Some(s) = segment {
                    line += &format!(" segment={}", id_tok(s));
                }
                if let Some(a) = p1 {
                    line += &format!(" p1={}", id_tok(a));
                }
                if let Some(b) = p2 {
                    line += &format!(" p2={}", id_tok(b));
                }
                line += &format!(" value={}", n(*value));
                if let Some(ex) = expr {
                    line += &format!(" expr={}", q(ex));
                }
            }
            SketchConstraintKind::HorizontalDistance {
                segment,
                p1,
                p2,
                value,
                expr,
            } => {
                line += "kind=horizontal_distance";
                if let Some(s) = segment {
                    line += &format!(" segment={}", id_tok(s));
                }
                if let Some(a) = p1 {
                    line += &format!(" p1={}", id_tok(a));
                }
                if let Some(b) = p2 {
                    line += &format!(" p2={}", id_tok(b));
                }
                line += &format!(" value={}", n(*value));
                if let Some(ex) = expr {
                    line += &format!(" expr={}", q(ex));
                }
            }
            SketchConstraintKind::VerticalDistance {
                segment,
                p1,
                p2,
                value,
                expr,
            } => {
                line += "kind=vertical_distance";
                if let Some(s) = segment {
                    line += &format!(" segment={}", id_tok(s));
                }
                if let Some(a) = p1 {
                    line += &format!(" p1={}", id_tok(a));
                }
                if let Some(b) = p2 {
                    line += &format!(" p2={}", id_tok(b));
                }
                line += &format!(" value={}", n(*value));
                if let Some(ex) = expr {
                    line += &format!(" expr={}", q(ex));
                }
            }
            SketchConstraintKind::Angle {
                seg1,
                seg2,
                degrees,
                expr,
            } => {
                let _ = write!(line, "kind=angle seg1={}", id_tok(seg1));
                if let Some(s2) = seg2 {
                    line += &format!(" seg2={}", id_tok(s2));
                }
                line += &format!(" degrees={}", n(*degrees));
                if let Some(ex) = expr {
                    line += &format!(" expr={}", q(ex));
                }
            }
            SketchConstraintKind::Radius {
                target,
                value,
                expr,
            } => {
                let _ = write!(
                    line,
                    "kind=radius target={} value={}",
                    id_tok(target),
                    n(*value)
                );
                if let Some(ex) = expr {
                    line += &format!(" expr={}", q(ex));
                }
            }
            SketchConstraintKind::Diameter {
                target,
                value,
                expr,
            } => {
                let _ = write!(
                    line,
                    "kind=diameter target={} value={}",
                    id_tok(target),
                    n(*value)
                );
                if let Some(ex) = expr {
                    line += &format!(" expr={}", q(ex));
                }
            }
        }
        let _ = writeln!(o, "{line}");
    }
    for m in &sk.macros {
        let mut line = String::from("  macro ");
        match m {
            SketchMacro::Rect {
                id,
                x,
                y,
                width,
                height,
                fillet,
                construction,
            } => {
                let _ = write!(
                    line,
                    "rect id={} at=({}, {}) size=({}, {})",
                    id_tok(id),
                    n(*x),
                    n(*y),
                    n(*width),
                    n(*height)
                );
                if *fillet != 0.0 {
                    line += &format!(" fillet={}", n(*fillet));
                }
                if *construction {
                    line += " construction";
                }
            }
            SketchMacro::CenterRect {
                id,
                cx,
                cy,
                width,
                height,
                fillet,
                construction,
            } => {
                let _ = write!(
                    line,
                    "center_rect id={} at=({}, {}) size=({}, {})",
                    id_tok(id),
                    n(*cx),
                    n(*cy),
                    n(*width),
                    n(*height)
                );
                if *fillet != 0.0 {
                    line += &format!(" fillet={}", n(*fillet));
                }
                if *construction {
                    line += " construction";
                }
            }
            SketchMacro::Circle {
                id,
                cx,
                cy,
                radius,
                construction,
            } => {
                let _ = write!(
                    line,
                    "circle id={} at=({}, {}) r={}",
                    id_tok(id),
                    n(*cx),
                    n(*cy),
                    n(*radius)
                );
                if *construction {
                    line += " construction";
                }
            }
            SketchMacro::Slot {
                id,
                x1,
                y1,
                x2,
                y2,
                radius,
            } => {
                let _ = write!(
                    line,
                    "slot id={} p1=({}, {}) p2=({}, {}) r={}",
                    id_tok(id),
                    n(*x1),
                    n(*y1),
                    n(*x2),
                    n(*y2),
                    n(*radius)
                );
            }
            SketchMacro::Polygon {
                id,
                cx,
                cy,
                radius,
                sides,
                angle_deg,
                construction,
            } => {
                let _ = write!(
                    line,
                    "polygon id={} at=({}, {}) r={} sides={sides}",
                    id_tok(id),
                    n(*cx),
                    n(*cy),
                    n(*radius)
                );
                if *angle_deg != 0.0 {
                    line += &format!(" angle={}", n(*angle_deg));
                }
                if *construction {
                    line += " construction";
                }
            }
            SketchMacro::BoltCircle {
                id,
                cx,
                cy,
                pcd,
                count,
                hole_diameter,
                start_angle_deg,
            } => {
                let _ = write!(
                    line,
                    "bolt_circle id={} at=({}, {}) pcd={} count={count} hole_d={}",
                    id_tok(id),
                    n(*cx),
                    n(*cy),
                    n(*pcd),
                    n(*hole_diameter)
                );
                if *start_angle_deg != 0.0 {
                    line += &format!(" start_angle={}", n(*start_angle_deg));
                }
            }
            SketchMacro::FilletCorner { point_id, radius } => {
                let _ = write!(
                    line,
                    "fillet_corner point={} r={}",
                    id_tok(point_id),
                    n(*radius)
                );
            }
        }
        let _ = writeln!(o, "{line}");
    }
    let _ = writeln!(o, "}}");
}

fn print_sketch_feature(o: &mut String, sf: &crate::sketch::SketchFeatureBinding) {
    use crate::sketch::SketchFeatureOp;
    let mut line = format!(
        "sketchfeature {} sketch={} ",
        id_tok(&sf.feature_id),
        id_tok(&sf.sketch_id)
    );
    match &sf.operation {
        SketchFeatureOp::BossExtrude {
            height,
            blend,
            symmetric,
        } => {
            let _ = write!(line, "kind=boss_extrude h={}", n(*height));
            if *blend != 0.0 {
                line += &format!(" blend={}", n(*blend));
            }
            if *symmetric {
                line += " symmetric";
            }
        }
        SketchFeatureOp::PocketCut {
            depth,
            through_all,
            blend,
        } => {
            let _ = write!(line, "kind=pocket_cut depth={}", n(*depth));
            if *through_all {
                line += " through_all";
            }
            if *blend != 0.0 {
                line += &format!(" blend={}", n(*blend));
            }
        }
        SketchFeatureOp::RevolveBoss { angle_deg, blend } => {
            let _ = write!(line, "kind=revolve_boss angle={}", n(*angle_deg));
            if *blend != 0.0 {
                line += &format!(" blend={}", n(*blend));
            }
        }
        SketchFeatureOp::RevolveCut { angle_deg, blend } => {
            let _ = write!(line, "kind=revolve_cut angle={}", n(*angle_deg));
            if *blend != 0.0 {
                line += &format!(" blend={}", n(*blend));
            }
        }
    }
    if sf.associate_with_host {
        line += " associate_host";
    }
    let _ = writeln!(o, "{line}");
}

fn parse_system_body_line(
    sys: &mut crate::system::DeclarativeSystem,
    c: &mut Cursor,
) -> Result<(), ParseError> {
    use crate::system::{
        DeclAudit, DeclAuxVar, DeclBindAttr, DeclCadBinding, DeclDesignCategory, DeclDesignParam,
        DeclEpistemicParam, DeclObjKind, DeclObjective, DeclReduce, DeclResidual, DeclScenario,
        DeclStateField,
    };
    let line = c.line;
    let err = |m: String| ParseError { line, message: m };
    let kw = c.ident("a system statement keyword")?;
    match kw.as_str() {
        "design" => {
            let name = c.ident("a design parameter name")?;
            let mut a = c.attrs("", &[])?;
            let nominal = a.req_signed_num("nominal")?;
            let min = a.req_signed_num("min")?;
            let max = a.req_signed_num("max")?;
            let unit = a.string("unit")?.unwrap_or_default();
            let symbol = a.string("symbol")?.unwrap_or_else(|| name.clone());
            let category = match a.string("category")?.as_deref() {
                None => DeclDesignCategory::Process,
                Some(cat) => DeclDesignCategory::parse(cat).ok_or_else(|| {
                    err(format!(
                        "design '{name}': category must be geometry, hardware, actuator, or process"
                    ))
                })?,
            };
            let cost_rate = a.signed_num_or("cost", 0.0)?;
            let description = a.string("desc")?.unwrap_or_default();
            a.finish()?;
            sys.designs.push(DeclDesignParam {
                name,
                nominal,
                min,
                max,
                unit,
                symbol,
                category,
                cost_rate,
                description,
            });
        }
        "epistemic" => {
            let name = c.ident("an epistemic parameter name")?;
            let mut a = c.attrs("", &[])?;
            let nominal = a.req_signed_num("nominal")?;
            let sigma = a.req_signed_num("sigma")?;
            let min = a.req_signed_num("min")?;
            let max = a.req_signed_num("max")?;
            let unit = a.string("unit")?.unwrap_or_default();
            let symbol = a.string("symbol")?.unwrap_or_else(|| name.clone());
            let mechanism = a.string("mechanism")?.unwrap_or_default();
            a.finish()?;
            sys.epistemics.push(DeclEpistemicParam {
                name,
                nominal,
                sigma,
                min,
                max,
                unit,
                symbol,
                mechanism,
            });
        }
        "state" => {
            let name = c.ident("a state field name")?;
            let mut a = c.attrs("", &[])?;
            let init_expr = a.req_string("init")?;
            let scale = a.signed_num_or("scale", 1.0)?;
            let unit = a.string("unit")?.unwrap_or_default();
            let symbol = a.string("symbol")?.unwrap_or_else(|| name.clone());
            a.finish()?;
            sys.states.push(DeclStateField {
                name,
                init_expr,
                scale,
                unit,
                symbol,
            });
        }
        "aux" => {
            let name = c.ident("an aux variable name")?;
            let mut a = c.attrs("", &[])?;
            let expr = a.req_string("expr")?;
            a.finish()?;
            sys.aux.push(DeclAuxVar { name, expr });
        }
        "residual" => {
            let state = c.ident("a state field name")?;
            let mut a = c.attrs("", &[])?;
            let eq = a.req_string("eq")?;
            let bc_left = a.string("bc_left")?;
            let bc_right = a.string("bc_right")?;
            let transient = a.signed_num_or("transient", 0.0)?;
            a.finish()?;
            sys.residuals.push(DeclResidual {
                state,
                eq,
                bc_left,
                bc_right,
                transient,
            });
        }
        "objective" => {
            let name = c.ident("an objective name")?;
            let mut a = c.attrs("", &[])?;
            let expr = a.req_string("expr")?;
            let reduce = match a.string("reduce")?.as_deref() {
                None => DeclReduce::Value,
                Some(r) => DeclReduce::parse(r).ok_or_else(|| {
                    err(format!(
                        "objective '{name}': reduce must be value, entry, exit, mean, integral, max, or min"
                    ))
                })?,
            };
            let kind_word = a.req_string("kind")?;
            let kind = match kind_word.as_str() {
                "minimize" => DeclObjKind::Minimize,
                "maximize" => DeclObjKind::Maximize,
                "upper" => DeclObjKind::Upper {
                    limit: a.req_signed_num("limit")?,
                },
                "lower" => DeclObjKind::Lower {
                    limit: a.req_signed_num("limit")?,
                },
                "band" => {
                    let [min, max] = a
                        .signed_pair("band")?
                        .ok_or_else(|| err(format!("objective '{name}' kind=band needs band=(min, max)")))?;
                    DeclObjKind::Band { min, max }
                }
                other => {
                    return Err(err(format!(
                        "objective '{name}': unknown kind '{other}' (expected minimize, maximize, upper, lower, or band)"
                    )))
                }
            };
            let unit = a.string("unit")?.unwrap_or_default();
            let symbol = a.string("symbol")?.unwrap_or_else(|| name.clone());
            let description = a.string("desc")?.unwrap_or_default();
            a.finish()?;
            sys.objectives.push(DeclObjective {
                name,
                expr,
                reduce,
                kind,
                unit,
                symbol,
                description,
            });
        }
        "audit" => {
            let name = c.ident("an audit name")?;
            let mut a = c.attrs("", &[])?;
            let inflow = a.req_string("inflow")?;
            let outflow = a.req_string("outflow")?;
            let source = a.string("source")?;
            let tolerance = a.signed_num_or("tol", 0.02)?;
            let unit = a.string("unit")?.unwrap_or_default();
            a.finish()?;
            sys.audits.push(DeclAudit {
                name,
                inflow,
                outflow,
                source,
                tolerance,
                unit,
            });
        }
        "bind" => {
            let target = c.ident("a bind target identifier")?;
            let mut a = c.attrs("", &[])?;
            let attr_word = a.req_string("attr")?;
            let attr = DeclBindAttr::parse(&attr_word).ok_or_else(|| {
                err(format!(
                    "bind '{target}': unknown attr '{attr_word}' (expected let, temp, h, tinf, q, pressure, preload, force_x, force_y, or force_z)"
                ))
            })?;
            let expr = a.req_string("expr")?;
            a.finish()?;
            sys.bindings.push(DeclCadBinding { target, attr, expr });
        }
        "scenario" => {
            let sc_name = match c.next() {
                Some(Tok::Str(s)) | Some(Tok::Ident(s)) => s,
                _ => return Err(err("scenario expects a name string or identifier".into())),
            };
            let mut a = c.attrs("", &[])?;
            let design_p = a
                .signed_nums("p")?
                .ok_or_else(|| err(format!("scenario '{sc_name}' needs p=[...]")))?;
            let description = a.string("desc")?.unwrap_or_default();
            a.finish()?;
            sys.scenarios.push(DeclScenario {
                name: sc_name,
                description,
                design_p,
            });
        }
        other => return Err(err(format!("unexpected keyword '{other}' inside system"))),
    }
    Ok(())
}

fn print_system(o: &mut String, sys: &crate::system::DeclarativeSystem) {
    use crate::system::{DeclAxis, DeclDesignCategory, DeclObjKind, DeclReduce};
    let mut hdr = format!("system {}", id_tok(&sys.id));
    if !sys.title.is_empty() {
        hdr += &format!(" title={}", q(&sys.title));
    }
    if !sys.sector.is_empty() {
        hdr += &format!(" sector={}", q(&sys.sector));
    }
    if sys.nodes != 1 {
        hdr += &format!(" nodes={}", sys.nodes);
    }
    if sys.span != [0.0, 1.0] {
        hdr += &format!(" span=({}, {})", n(sys.span[0]), n(sys.span[1]));
    }
    if sys.axis != DeclAxis::Z {
        hdr += &format!(" axis={}", sys.axis.name());
    }
    let _ = writeln!(o, "{hdr} {{");

    for d in &sys.designs {
        let mut line = format!(
            "  design {} nominal={} min={} max={}",
            id_tok(&d.name),
            n(d.nominal),
            n(d.min),
            n(d.max)
        );
        if !d.unit.is_empty() {
            line += &format!(" unit={}", q(&d.unit));
        }
        if !d.symbol.is_empty() && d.symbol != d.name {
            line += &format!(" symbol={}", q(&d.symbol));
        }
        if d.category != DeclDesignCategory::Process {
            line += &format!(" category={}", d.category.name());
        }
        if d.cost_rate != 0.0 {
            line += &format!(" cost={}", n(d.cost_rate));
        }
        if !d.description.is_empty() {
            line += &format!(" desc={}", q(&d.description));
        }
        let _ = writeln!(o, "{line}");
    }

    for ep in &sys.epistemics {
        let mut line = format!(
            "  epistemic {} nominal={} sigma={} min={} max={}",
            id_tok(&ep.name),
            n(ep.nominal),
            n(ep.sigma),
            n(ep.min),
            n(ep.max)
        );
        if !ep.unit.is_empty() {
            line += &format!(" unit={}", q(&ep.unit));
        }
        if !ep.symbol.is_empty() && ep.symbol != ep.name {
            line += &format!(" symbol={}", q(&ep.symbol));
        }
        if !ep.mechanism.is_empty() {
            line += &format!(" mechanism={}", q(&ep.mechanism));
        }
        let _ = writeln!(o, "{line}");
    }

    for st in &sys.states {
        let mut line = format!("  state {} init={}", id_tok(&st.name), q(&st.init_expr));
        if (st.scale - 1.0).abs() > 1e-12 {
            line += &format!(" scale={}", n(st.scale));
        }
        if !st.unit.is_empty() {
            line += &format!(" unit={}", q(&st.unit));
        }
        if !st.symbol.is_empty() && st.symbol != st.name {
            line += &format!(" symbol={}", q(&st.symbol));
        }
        let _ = writeln!(o, "{line}");
    }

    for ax in &sys.aux {
        let _ = writeln!(o, "  aux {} expr={}", id_tok(&ax.name), q(&ax.expr));
    }

    for res in &sys.residuals {
        let mut line = format!("  residual {} eq={}", id_tok(&res.state), q(&res.eq));
        if let Some(ref bcl) = res.bc_left {
            line += &format!(" bc_left={}", q(bcl));
        }
        if let Some(ref bcr) = res.bc_right {
            line += &format!(" bc_right={}", q(bcr));
        }
        if res.transient != 0.0 {
            line += &format!(" transient={}", n(res.transient));
        }
        let _ = writeln!(o, "{line}");
    }

    for obj in &sys.objectives {
        let mut line = format!("  objective {} expr={}", id_tok(&obj.name), q(&obj.expr));
        if obj.reduce != DeclReduce::Value {
            line += &format!(" reduce={}", obj.reduce.name());
        }
        match obj.kind {
            DeclObjKind::Minimize => line += " kind=minimize",
            DeclObjKind::Maximize => line += " kind=maximize",
            DeclObjKind::Upper { limit } => line += &format!(" kind=upper limit={}", n(limit)),
            DeclObjKind::Lower { limit } => line += &format!(" kind=lower limit={}", n(limit)),
            DeclObjKind::Band { min, max } => {
                line += &format!(" kind=band band=({}, {})", n(min), n(max))
            }
        }
        if !obj.unit.is_empty() {
            line += &format!(" unit={}", q(&obj.unit));
        }
        if !obj.symbol.is_empty() && obj.symbol != obj.name {
            line += &format!(" symbol={}", q(&obj.symbol));
        }
        if !obj.description.is_empty() {
            line += &format!(" desc={}", q(&obj.description));
        }
        let _ = writeln!(o, "{line}");
    }

    for aud in &sys.audits {
        let mut line = format!(
            "  audit {} inflow={} outflow={}",
            id_tok(&aud.name),
            q(&aud.inflow),
            q(&aud.outflow)
        );
        if let Some(ref src) = aud.source {
            line += &format!(" source={}", q(src));
        }
        if (aud.tolerance - 0.02).abs() > 1e-12 {
            line += &format!(" tol={}", n(aud.tolerance));
        }
        if !aud.unit.is_empty() {
            line += &format!(" unit={}", q(&aud.unit));
        }
        let _ = writeln!(o, "{line}");
    }

    for b in &sys.bindings {
        let _ = writeln!(
            o,
            "  bind {} attr={} expr={}",
            id_tok(&b.target),
            b.attr.name(),
            q(&b.expr)
        );
    }

    for sc in &sys.scenarios {
        let p_str: Vec<String> = sc.design_p.iter().map(|v| n(*v)).collect();
        let mut line = format!("  scenario {} p=[{}]", q(&sc.name), p_str.join(", "));
        if !sc.description.is_empty() {
            line += &format!(" desc={}", q(&sc.description));
        }
        let _ = writeln!(o, "{line}");
    }

    let _ = writeln!(o, "}}");
}

fn parse_modifier(kw: &str, c: &mut Cursor) -> Result<Modifier, ParseError> {
    let num = |c: &mut Cursor| match c.next() {
        Some(Tok::Num(v)) => Ok(v),
        _ => c.err("expected a number"),
    };
    Ok(match kw {
        "shell" => {
            let thickness = num(c)?;
            let side = match c.ident("inside, outside or centre")?.as_str() {
                "inside" => ShellSide::Inside,
                "outside" => ShellSide::Outside,
                "centre" | "center" => ShellSide::Centre,
                _ => return c.err("expected inside, outside or centre"),
            };
            Modifier::Shell { thickness, side }
        }
        "offset" => Modifier::Offset { distance: num(c)? },
        "mirror" => Modifier::Mirror {
            axis: parse_axis(c)?,
        },
        "linear" => {
            // step is fixed at declaration: no lets scope and no recorded consumers.
            let count = num(c)? as u32;
            let mut a = c.attrs("", &[])?;
            let step = a.vec3("step")?.ok_or_else(|| ParseError {
                line: c.line,
                message: "linear needs step=(x, y, z)".into(),
            })?;
            a.finish()?;
            Modifier::Linear { count, step }
        }
        "circular" => {
            // axis/angle behave the same as linear's step.
            let count = num(c)? as u32;
            let mut a = c.attrs("", &[])?;
            let axis = match a.string("axis")?.as_deref() {
                None | Some("z") => Axis::Z,
                Some("x") => Axis::X,
                Some("y") => Axis::Y,
                Some(_) => return c.err("axis must be x, y or z"),
            };
            let angle = a.num_or("angle", 360.0)?;
            a.finish()?;
            Modifier::Circular { count, axis, angle }
        }
        _ => unreachable!(),
    })
}

// ---------------------------------------------------------------------------------------------
// Printer

fn n(v: f64) -> String {
    // Shortest round-trip representation; normalise negative zero.
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v}")
}

fn v3(v: [f64; 3]) -> String {
    format!("({}, {}, {})", n(v[0]), n(v[1]), n(v[2]))
}

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Prints numbers as the expressions that drove them, so a let document round-trips
/// its arithmetic instead of the frozen values of one evaluation.
struct Printer<'a> {
    consumed: &'a [Consumer],
}

impl<'a> Printer<'a> {
    /// The printed form of one number: the consumer expression when the statement
    /// recorded one, else the plain value.
    fn num_for(&self, id: &str, attr: &str, comp: Option<usize>, v: f64) -> String {
        match self
            .consumed
            .iter()
            .find(|c| c.target == id && c.attr == attr && c.comp == comp)
        {
            Some(c) => c.expr.clone(),
            None => n(v),
        }
    }

    /// The printed form of a 3-vector, per component.
    fn v3_for(&self, id: &str, attr: &str, v: [f64; 3]) -> String {
        let parts: Vec<String> = (0..3)
            .map(|i| self.num_for(id, attr, Some(i), v[i]))
            .collect();
        format!("({}, {}, {})", parts[0], parts[1], parts[2])
    }

    /// Whether any component of this attribute was driven (for the plain-number
    /// conditionals that must not drop a driven zero).
    fn driven(&self, id: &str, attr: &str) -> bool {
        self.consumed
            .iter()
            .any(|c| c.target == id && c.attr == attr)
    }
}

/// Print a document in canonical `.fsm` form.
pub fn print(doc: &Document) -> String {
    let mut o = String::new();
    let p = Printer {
        consumed: &doc.consumed,
    };
    let _ = writeln!(o, "fieldsmith {FORMAT_VERSION}");
    let _ = writeln!(o, "name {}", q(&doc.name));
    let _ = writeln!(o, "material {}", doc.material);
    if let Some(cs) = &doc.coupled_system {
        let _ = writeln!(o, "coupled {cs}");
    }
    for l in &doc.lets {
        let _ = writeln!(o, "let {} = {}", l.name, l.expr);
    }
    for m in &doc.custom_materials {
        let mut line = format!(
            "custommaterial {} name={} e={} nu={} yield={} density={} wall={}",
            m.id,
            q(&m.name),
            n(m.e),
            n(m.nu),
            n(m.yield_strength),
            n(m.density),
            n(m.min_wall)
        );
        if let Some(a) = m.alpha {
            line += &format!(" a={}", n(a));
        }
        if let Some(k) = m.k {
            line += &format!(" k={}", n(k));
        }
        if let Some(cp) = m.cp {
            line += &format!(" cp={}", n(cp));
        }
        if let Some(yz) = m.yield_z {
            line += &format!(" yield_z={}", n(yz));
        }
        if let Some(sz) = m.shear_z {
            line += &format!(" shear_z={}", n(sz));
        }
        if let Some(hn) = m.hardening_n {
            line += &format!(" hardening_n={}", n(hn));
        }
        if let Some(hk) = m.hardening_k {
            line += &format!(" hardening_k={}", n(hk));
        }
        if let Some(wm) = m.weibull_m {
            line += &format!(" weibull_m={}", n(wm));
        }
        if m.role == MaterialRole::Fluid {
            line += " role=fluid";
        }
        line += &format!(" source={}", q(&m.source));
        let _ = writeln!(o, "{line}");
    }
    if !doc.root.is_empty() {
        o.push('\n');
    }
    for node in &doc.root {
        print_node(&mut o, node, 0, &p);
    }
    if !doc.sketches.is_empty() || !doc.sketch_features.is_empty() {
        o.push('\n');
    }
    for sk in &doc.sketches {
        print_sketch(&mut o, sk);
    }
    for sf in &doc.sketch_features {
        print_sketch_feature(&mut o, sf);
    }
    if !doc.regions.is_empty()
        || !doc.supports.is_empty()
        || !doc.loads.is_empty()
        || !doc.temperatures.is_empty()
        || !doc.convections.is_empty()
        || !doc.heat_fluxes.is_empty()
    {
        o.push('\n');
    }
    for r in &doc.regions {
        let sels: Vec<String> = r
            .selectors
            .iter()
            .map(|s| match s.patch {
                Some(p) => format!("{}.p{}", s.shape, p),
                None => match s.face {
                    Face::All => s.shape.clone(),
                    f => format!("{}.{}", s.shape, f.name()),
                },
            })
            .collect();
        let _ = writeln!(o, "region {} = {}", r.id, sels.join(" + "));
    }
    for s in &doc.supports {
        let mut line = format!("support {} {}", s.id, s.region);
        if s.fixed != [true; 3] {
            let axes: String = ["x", "y", "z"]
                .iter()
                .zip(s.fixed)
                .filter(|(_, f)| *f)
                .map(|(a, _)| *a)
                .collect();
            line += &format!(" fixed={axes}");
        }
        let _ = writeln!(o, "{line}");
    }
    for l in &doc.loads {
        let kind = match &l.kind {
            LoadKind::Force { vector } => format!("force={}", p.v3_for(&l.id, "force", *vector)),
            LoadKind::Pressure { mpa, expr } => {
                let mut k = format!("pressure={}", n(*mpa));
                if let Some(src) = expr {
                    k += &format!(" pressure_fn={}", q(src));
                }
                k
            }
            LoadKind::Preload { newtons, axis } => {
                let mut k = format!("preload={}", p.num_for(&l.id, "newtons", None, *newtons));
                if *axis != [0.0; 3] {
                    k += &format!(" axis={}", v3(*axis));
                }
                k
            }
            LoadKind::Distributed { expr, dir, total } => {
                let mut k = format!("fn={}", q(expr));
                match dir {
                    LoadDir::Global { v } => k += &format!(" dir={}", v3(*v)),
                    LoadDir::Local { v } => k += &format!(" local={}", v3(*v)),
                }
                if let Some(t) = total {
                    k += &format!(" total={}", p.num_for(&l.id, "total", None, *t));
                }
                k
            }
        };
        let _ = writeln!(o, "load {} {} {} case={}", l.id, l.region, kind, q(&l.case));
    }
    for t in &doc.temperatures {
        let mut line = format!(
            "temperature {} {} temp={}",
            t.id,
            t.region,
            p.num_for(&t.id, "celsius", None, t.celsius),
        );
        if let Some(src) = &t.expr {
            line += &format!(" temp_fn={}", q(src));
        }
        line += &format!(" case={}", q(&t.case));
        let _ = writeln!(o, "{line}");
    }
    for c in &doc.convections {
        let mut line = format!(
            "convection {} {} h={}",
            c.id,
            c.region,
            p.num_for(&c.id, "h", None, c.h),
        );
        if let Some(src) = &c.h_expr {
            line += &format!(" h_fn={}", q(src));
        }
        line += &format!(" tinf={}", p.num_for(&c.id, "tinf", None, c.t_inf));
        if let Some(src) = &c.t_inf_expr {
            line += &format!(" tinf_fn={}", q(src));
        }
        line += &format!(" case={}", q(&c.case));
        let _ = writeln!(o, "{line}");
    }
    for f in &doc.heat_fluxes {
        let mut line = format!(
            "heatflux {} {} q={}",
            f.id,
            f.region,
            p.num_for(&f.id, "q", None, f.q),
        );
        if let Some(src) = &f.expr {
            line += &format!(" q_fn={}", q(src));
        }
        line += &format!(" case={}", q(&f.case));
        let _ = writeln!(o, "{line}");
    }
    if let Some(f) = &doc.fill {
        o.push('\n');
        let mut line = format!(
            "fill {} {} cell={} skin={} rho=({}, {}) curve={} wall={}",
            f.family.name(),
            match f.mode {
                FillMode::Sheet => "sheet",
                FillMode::Network => "network",
            },
            n(f.cell),
            n(f.skin),
            n(f.rho_min),
            n(f.rho_max),
            n(f.curve),
            n(f.min_wall)
        );
        if !f.protect.is_empty() {
            line += &format!(" protect=({})", f.protect.join(", "));
        }
        line += &format!(" margin={}", n(f.margin));
        if let Some(c) = &f.case {
            line += &format!(" case={}", q(c));
        }
        if f.rotate != [0.0; 3] {
            line += &format!(" rotate={}", v3(f.rotate));
        }
        if let Some(sm) = f.skin_max {
            line += &format!(" skin_max={}", n(sm));
        }
        if let Some(cm) = f.cell_min {
            line += &format!(" cell_min={}", n(cm));
        }
        if let Some(m) = f.morph {
            line += &format!(" morph={}", m.name());
        }
        if let Some(er) = f.erode.filter(|&er| er > 0.0) {
            line += &format!(" erode={}", n(er));
        }
        let _ = writeln!(o, "{line}");
    }
    if let Some(sys) = &doc.system_block {
        o.push('\n');
        print_system(&mut o, sys);
    }
    for u in &doc.uncertainties {
        let lbl = if u.label.is_empty() {
            &u.param
        } else {
            &u.label
        };
        let mut line = format!("uncertainty {} param={}", q(lbl), q(&u.param));
        if let Some(s) = u.std {
            line += &format!(" std={}", n(s));
        }
        if let Some(c) = u.cv {
            line += &format!(" cv={}", n(c));
        }
        if !u.unit.is_empty() {
            line += &format!(" unit={}", q(&u.unit));
        }
        let _ = writeln!(o, "{line}");
    }
    for a in &doc.asserts {
        let mut line = format!(
            "assert {} metric={} target={}",
            q(&a.label),
            q(&a.metric),
            q(&a.target)
        );
        if !a.tolerance.is_empty() {
            line += &format!(" tol={}", q(&a.tolerance));
        }
        if !a.unit.is_empty() {
            line += &format!(" unit={}", q(&a.unit));
        }
        let _ = writeln!(o, "{line}");
    }
    let mut solve_line = String::from("solve");
    if let Some(s) = doc.solve.size {
        solve_line += &format!(" size={}", n(s));
    }
    if let Some(m) = doc.solve.modes {
        solve_line += &format!(" modes={m}");
    }
    if let Some(t) = doc.solve.t_ref {
        solve_line += &format!(" tref={}", n(t));
    }
    if let Some([th, ph]) = doc.solve.build {
        solve_line += &format!(" build=({}, {})", n(th), n(ph));
    }
    if let Some(PlasticityLaw::J2) = doc.solve.plasticity {
        solve_line += " plasticity=j2";
    }
    if let Some(hn) = doc.solve.hardening_n {
        solve_line += &format!(" hardening_n={}", n(hn));
    }
    if let Some(te) = doc.solve.t_end {
        solve_line += &format!(" t_end={}", n(te));
    }
    if let Some(st) = doc.solve.steps {
        solve_line += &format!(" steps={st}");
    }
    if let Some(ti) = doc.solve.t_init {
        solve_line += &format!(" t_init={}", n(ti));
    }
    if solve_line.len() > "solve".len() {
        let _ = writeln!(o, "{solve_line}");
    }
    o
}

fn print_node(o: &mut String, node: &Node, depth: usize, p: &Printer) {
    let ind = "  ".repeat(depth);
    let mut line = String::new();
    let _ = write!(line, "{ind}");
    match &node.kind {
        NodeKind::Box { size, round } => {
            let _ = write!(
                line,
                "box {} size={}",
                node.id,
                p.v3_for(&node.id, "size", *size)
            );
            if *round != 0.0 || p.driven(&node.id, "round") {
                let _ = write!(
                    line,
                    " round={}",
                    p.num_for(&node.id, "round", None, *round)
                );
            }
        }
        NodeKind::Cylinder {
            radius,
            height,
            round,
        } => {
            let _ = write!(
                line,
                "cylinder {} r={} h={}",
                node.id,
                p.num_for(&node.id, "radius", None, *radius),
                p.num_for(&node.id, "height", None, *height)
            );
            if *round != 0.0 || p.driven(&node.id, "round") {
                let _ = write!(
                    line,
                    " round={}",
                    p.num_for(&node.id, "round", None, *round)
                );
            }
        }
        NodeKind::Sphere { radius } => {
            let _ = write!(
                line,
                "sphere {} r={}",
                node.id,
                p.num_for(&node.id, "radius", None, *radius)
            );
        }
        NodeKind::Capsule { radius, length } => {
            let _ = write!(
                line,
                "capsule {} r={} length={}",
                node.id,
                p.num_for(&node.id, "radius", None, *radius),
                p.num_for(&node.id, "length", None, *length)
            );
        }
        NodeKind::Torus { major, minor } => {
            let _ = write!(
                line,
                "torus {} major={} minor={}",
                node.id,
                p.num_for(&node.id, "major", None, *major),
                p.num_for(&node.id, "minor", None, *minor)
            );
        }
        NodeKind::Extrude { profile, height } => {
            let pts: Vec<String> = profile
                .iter()
                .map(|pt| {
                    if pt.bulge != 0.0 {
                        format!("({}, {}, {})", n(pt.x), n(pt.y), n(pt.bulge))
                    } else {
                        format!("({}, {})", n(pt.x), n(pt.y))
                    }
                })
                .collect();
            let _ = write!(
                line,
                "extrude {} h={} profile=[{}]",
                node.id,
                p.num_for(&node.id, "height", None, *height),
                pts.join(", ")
            );
        }
        NodeKind::Revolve { profile, angle } => {
            let pts: Vec<String> = profile
                .iter()
                .map(|pt| {
                    if pt.bulge != 0.0 {
                        format!("({}, {}, {})", n(pt.x), n(pt.y), n(pt.bulge))
                    } else {
                        format!("({}, {})", n(pt.x), n(pt.y))
                    }
                })
                .collect();
            let _ = write!(
                line,
                "revolve {} angle={} profile=[{}]",
                node.id,
                p.num_for(&node.id, "angle", None, *angle),
                pts.join(", ")
            );
        }
        NodeKind::Mesh {
            src,
            hash,
            units,
            up,
            voxel,
            bbox,
            tris,
            patches,
            open_edges,
        } => {
            let _ = write!(
                line,
                "mesh {} src={} units={} up={} vox={} hash={} bbox=({}) tris={} patches={} open_edges={}",
                node.id,
                q(src),
                units,
                up,
                n(*voxel),
                q(hash),
                [bbox[0], bbox[1], bbox[2], bbox[3], bbox[4], bbox[5]]
                    .iter()
                    .map(|v| n(*v))
                    .collect::<Vec<_>>()
                    .join(", "),
                tris,
                patches,
                open_edges
            );
        }
        NodeKind::Extend { region, dir, len } => {
            let _ = write!(
                line,
                "extend {} {} dir={} len={}",
                node.id,
                region,
                v3(*dir),
                n(*len)
            );
        }
        NodeKind::Group { .. } => {
            let _ = write!(line, "group {}", node.id);
        }
    }
    if node.transform.translate != [0.0; 3] || p.driven(&node.id, "at") {
        let _ = write!(
            line,
            " at={}",
            p.v3_for(&node.id, "at", node.transform.translate)
        );
    }
    if node.transform.rotate != [0.0; 3] || p.driven(&node.id, "rotate") {
        let _ = write!(
            line,
            " rotate={}",
            p.v3_for(&node.id, "rotate", node.transform.rotate)
        );
    }
    match node.op {
        Op::Add => {}
        Op::Subtract => line.push_str(" subtract"),
        Op::Intersect => line.push_str(" intersect"),
    }
    if node.blend != 0.0 {
        let _ = write!(line, " blend={}", n(node.blend));
    }
    if node.suppressed {
        line.push_str(" suppressed");
    }
    if let NodeKind::Group {
        children,
        modifiers,
    } = &node.kind
    {
        line.push_str(" {");
        let _ = writeln!(o, "{line}");
        if let Some(m) = &node.material {
            let _ = writeln!(o, "{ind}material {m} on {}", node.id);
        }
        if let Some(c) = &node.color {
            let _ = writeln!(o, "{ind}color \"{c}\" on {}", node.id);
        }
        for c in children {
            print_node(o, c, depth + 1, p);
        }
        let ind2 = "  ".repeat(depth + 1);
        for m in modifiers {
            let ms = match m {
                Modifier::Shell { thickness, side } => format!(
                    "shell {} {}",
                    n(*thickness),
                    match side {
                        ShellSide::Inside => "inside",
                        ShellSide::Outside => "outside",
                        ShellSide::Centre => "centre",
                    }
                ),
                Modifier::Offset { distance } => format!("offset {}", n(*distance)),
                Modifier::Mirror { axis } => format!("mirror {}", axis.name()),
                Modifier::Linear { count, step } => format!("linear {} step={}", count, v3(*step)),
                Modifier::Circular { count, axis, angle } => {
                    format!(
                        "circular {} axis={} angle={}",
                        count,
                        axis.name(),
                        n(*angle)
                    )
                }
            };
            let _ = writeln!(o, "{ind2}{ms}");
        }
        let _ = writeln!(o, "{ind}}}");
    } else {
        let _ = writeln!(o, "{line}");
        if let Some(m) = &node.material {
            let _ = writeln!(o, "{ind}material {m} on {}", node.id);
        }
        if let Some(c) = &node.color {
            let _ = writeln!(o, "{ind}color \"{c}\" on {}", node.id);
        }
    }
}

/// A closed-form analytic verification target embedded in a Literate `.fsm` header (`# @check:`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LiterateCheck {
    pub quantity: String,
    pub formula: String,
    pub analytic_ref: String,
    pub measured_fea: String,
    pub rel_error: String,
    pub tolerance: String,
}

/// An executable live-solve verification assertion embedded in a Literate `.fsm` header (`# @assert:`).
/// Syntax: `# @assert: <label> || <metric> || <target_or_op> || <tolerance> || <unit>`
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LiterateAssert {
    pub label: String,
    pub metric: String,
    pub target: String,
    #[serde(default)]
    pub tolerance: String,
    #[serde(default)]
    pub unit: String,
}

/// An executable MCP step declared in a Literate `.fsm` header (`# @mcp:`).
/// Syntax: `# @mcp: <label> || <tool_name> || <arguments_json>`
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LiterateMcpStep {
    pub label: String,
    pub tool: String,
    pub arguments_json: String,
}

/// Structured metadata and closed-form verification targets parsed from a Literate `.fsm` header
/// (`# === FIELDSMITH LITERATE CASE === ... # === END LITERATE HEADER ===`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LiterateHeader {
    pub id: String,
    pub slug: String,
    pub category: String,
    pub title: String,
    pub subtitle: String,
    pub level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coupled_system: Option<String>,
    pub checks: Vec<LiterateCheck>,
    pub asserts: Vec<LiterateAssert>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_steps: Vec<LiterateMcpStep>,
}

/// Parse the `# === FIELDSMITH LITERATE CASE ===` comment header of a `.fsm` file, if present.
pub fn parse_literate_header(text: &str) -> Option<LiterateHeader> {
    if !text.contains("# === FIELDSMITH LITERATE CASE ===") {
        return None;
    }
    let mut header = LiterateHeader::default();
    let mut in_header = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "# === FIELDSMITH LITERATE CASE ===" {
            in_header = true;
            continue;
        }
        if trimmed == "# === END LITERATE HEADER ===" {
            break;
        }
        if !in_header || !trimmed.starts_with('#') {
            continue;
        }
        let body = trimmed.trim_start_matches('#').trim();
        if let Some(v) = body.strip_prefix("@id:") {
            header.id = v.trim().to_string();
        } else if let Some(v) = body.strip_prefix("@slug:") {
            header.slug = v.trim().to_string();
        } else if let Some(v) = body.strip_prefix("@category:") {
            header.category = v.trim().to_string();
        } else if let Some(v) = body.strip_prefix("@title:") {
            header.title = v.trim().to_string();
        } else if let Some(v) = body.strip_prefix("@subtitle:") {
            header.subtitle = v.trim().to_string();
        } else if let Some(v) = body.strip_prefix("@level:") {
            header.level = v.trim().to_string();
        } else if let Some(v) = body.strip_prefix("@coupled:") {
            let cs = v.trim();
            if !cs.is_empty() {
                header.coupled_system = Some(cs.to_string());
            }
        } else if let Some(v) = body.strip_prefix("@check:") {
            let parts: Vec<&str> = v.split(" || ").map(|p| p.trim()).collect();
            header.checks.push(LiterateCheck {
                quantity: parts.first().copied().unwrap_or_default().to_string(),
                formula: parts.get(1).copied().unwrap_or_default().to_string(),
                analytic_ref: parts.get(2).copied().unwrap_or_default().to_string(),
                measured_fea: parts.get(3).copied().unwrap_or_default().to_string(),
                rel_error: parts.get(4).copied().unwrap_or_default().to_string(),
                tolerance: parts.get(5).copied().unwrap_or_default().to_string(),
            });
        } else if let Some(v) = body.strip_prefix("@assert:") {
            let parts: Vec<&str> = v.split(" || ").map(|p| p.trim()).collect();
            header.asserts.push(LiterateAssert {
                label: parts.first().copied().unwrap_or_default().to_string(),
                metric: parts.get(1).copied().unwrap_or_default().to_string(),
                target: parts.get(2).copied().unwrap_or_default().to_string(),
                tolerance: parts.get(3).copied().unwrap_or_default().to_string(),
                unit: parts.get(4).copied().unwrap_or_default().to_string(),
            });
        } else if let Some(v) = body.strip_prefix("@mcp:") {
            let parts: Vec<&str> = v.split(" || ").map(|p| p.trim()).collect();
            if parts.len() >= 2 {
                header.mcp_steps.push(LiterateMcpStep {
                    label: parts[0].to_string(),
                    tool: parts[1].to_string(),
                    arguments_json: parts.get(2).copied().unwrap_or("{}").to_string(),
                });
            }
        }
    }
    Some(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"fieldsmith 1
name "Bracket"
material petg

box base size=(60, 40, 8) at=(30, 20, 4)
box upright size=(60, 8, 60) at=(30, 4, 34) blend=8
cylinder hole1 r=3.25 h=20 at=(15, 28, 4) subtract
group arms at=(1, 2, 3) rotate=(0, 0, 45) subtract blend=0.5 {
  sphere s1 r=5
  capsule c1 r=1 length=10 intersect
  torus t1 major=10 minor=2 suppressed
  extrude e1 h=4 profile=[(0, 0), (10, 0, 0.5), (10, 10), (-0.5, 0.001)]
  shell 1 inside
  mirror x
  linear 3 step=(10, 0, 0)
  circular 6 axis=z angle=360
}

region holes = hole1.side + base.nz
region all = upright
region top = base.pz
support fix1 holes
support roll all fixed=xz
load push holes pressure=0.25 case="Load 2"
load tip all force=(0, -103.5, -386.4) case="Load 1"
load sweep top fn="0 < x < 25 -> 0.02*x/25; 25 < x < 60 -> 0.02" dir=(0, 0, -1) total=400 case="Load 1"

fill gyroid sheet cell=6 skin=1.2 rho=(0.2, 0.8) curve=1 wall=0.8 protect=(hole1, base) margin=2 case="Load 1"
solve size=2.5 modes=3
"#;

    #[test]
    fn round_trip_is_canonical() {
        let d = parse(SAMPLE).unwrap();
        let p = print(&d);
        assert_eq!(p, SAMPLE, "printer output differs:\n{p}");
        assert_eq!(parse(&p).unwrap(), d);
    }

    #[test]
    fn validates_sample() {
        let d = parse(SAMPLE).unwrap();
        assert!(d.validate().is_empty(), "{:?}", d.validate());
    }

    const LET_SAMPLE: &str = r#"fieldsmith 1
name "Bracket"
material petg
let wall_t = 4
let bolt_pitch = 2 * wall_t + 2

box beam size=(100, 2 * wall_t, 2 * wall_t) at=(50, wall_t, wall_t)
cylinder pin r=wall_t h=bolt_pitch rotate=(90, 0, 0)
sphere knob r=wall_t / 2

region root = beam.nx
region tip = beam.px
support clamp root
load push tip force=(0, 0, -(wall_t * 25)) case="Load 1"
temperature hot tip temp=80 + wall_t case="Thermal 1"
solve size=2.5
"#;

    const PLAIN_BRACKET: &str = r#"fieldsmith 1
name "Bracket"
material petg

box beam size=(100, 8, 8) at=(50, 4, 4)
cylinder pin r=4 h=10 rotate=(90, 0, 0)
sphere knob r=2
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
temperature hot tip temp=84
solve size=2.5
"#;

    #[test]
    fn let_document_round_trips_canonically() {
        let d = parse(LET_SAMPLE).unwrap();
        let p = print(&d);
        assert_eq!(p, LET_SAMPLE, "printer output differs:\n{p}");
        for _ in 0..2 {
            let d = parse(&p).unwrap();
            assert_eq!(print(&d), LET_SAMPLE, "printer output drifted");
        }
    }

    #[test]
    fn lets_substitute_and_record_consumers() {
        let d = parse(LET_SAMPLE).unwrap();
        assert_eq!(d.lets.len(), 2);
        assert_eq!(d.lets[0].name, "wall_t");
        assert_eq!(d.lets[0].value, 4.0);
        assert_eq!(d.lets[0].expr, "4");
        assert_eq!(d.lets[1].name, "bolt_pitch");
        assert_eq!(d.lets[1].value, 10.0);
        assert_eq!(d.lets[1].expr, "2 * wall_t + 2");
        let got: Vec<(String, String, Option<usize>, String)> = d
            .consumed
            .iter()
            .map(|k| (k.target.clone(), k.attr.clone(), k.comp, k.expr.clone()))
            .collect();
        let want = vec![
            ("beam", "size", Some(1), "2 * wall_t"),
            ("beam", "size", Some(2), "2 * wall_t"),
            ("beam", "at", Some(1), "wall_t"),
            ("beam", "at", Some(2), "wall_t"),
            ("pin", "radius", None, "wall_t"),
            ("pin", "height", None, "bolt_pitch"),
            ("knob", "radius", None, "wall_t / 2"),
            ("push", "force", Some(2), "-(wall_t * 25)"),
            ("hot", "celsius", None, "80 + wall_t"),
        ]
        .into_iter()
        .map(|(t, a, c, e)| (t.to_string(), a.to_string(), c, e.to_string()))
        .collect::<Vec<_>>();
        assert_eq!(got, want, "consumers in parse order");
    }

    #[test]
    fn let_document_matches_the_substituted_document() {
        let let_doc = parse(LET_SAMPLE).unwrap();
        let plain = parse(PLAIN_BRACKET).unwrap();
        // The lets/consumed records differ by design; the numbers they produce must not.
        let mut stripped = let_doc.clone();
        stripped.lets.clear();
        stripped.consumed.clear();
        assert_eq!(stripped, plain, "let substitution must be value-exact");
    }

    #[test]
    fn expression_precedence_and_scopes() {
        let value_of = |body: &str| {
            parse(&format!(
                "fieldsmith 1\nname \"t\"\nmaterial petg\n{body}\n"
            ))
            .unwrap()
            .lets
            .last()
            .expect("at least one let")
            .value
        };
        assert_eq!(value_of("let a = 2 + 3 * 4"), 14.0);
        assert_eq!(value_of("let a = (2 + 3) * 4"), 20.0);
        assert_eq!(value_of("let a = 10 - 3 - 2"), 5.0);
        assert_eq!(value_of("let a = -2 * 3"), -6.0);
        assert_eq!(value_of("let a = 20 / 2 / 5"), 2.0);
        assert_eq!(value_of("let a = 5 - 2 * (3 + 1)"), -3.0);
        // Lets are in scope only after their line.
        assert_eq!(value_of("let a = 2\nlet b = a + 1"), 3.0);
    }
    #[test]
    fn let_errors_are_named() {
        let with_lets = |body: &str| {
            parse(&format!(
                "fieldsmith 1\nname \"t\"\nmaterial petg\n{body}\n"
            ))
            .err()
            .map(|e| e.message)
            .expect("expected a parse error")
        };
        // Unknown names, duplicates, collisions, forward references.
        assert!(
            with_lets("let w = q + 1").contains("no let named 'q'"),
            "undefined variable"
        );
        assert!(
            with_lets("let w = 1\nlet w = 2").contains("let 'w' is defined more than once"),
            "duplicate let"
        );
        assert!(
            with_lets("let beam = 4\nbox beam size=(4, 4, 4)")
                .contains("Name 'beam' is used more than once"),
            "let colliding with a later shape"
        );
        assert!(
            with_lets("box beam size=(4, 4, 4)\nlet beam = 2")
                .contains("Name 'beam' is used more than once"),
            "let colliding with an earlier shape"
        );
        assert!(
            with_lets("box beam size=(w, 4, 4)\nlet w = 4").contains("no let named 'w'"),
            "forward reference"
        );
        // Bad arithmetic fails at its own line.
        assert!(
            with_lets("let z = 4 / 0").contains("let 'z': division by zero"),
            "divide by zero in a let"
        );
        assert!(
            with_lets("let big = 1e308 * 10").contains("let 'big': value is not finite"),
            "non-finite let value"
        );
        assert!(
            with_lets("let w = 4\nbox beam size=(100, w / 0, 4)")
                .contains("'size' on 'beam': division by zero"),
            "divide by zero in a consumer"
        );
        assert!(
            with_lets("let w = 1e308\nbox beam size=(w * w, 4, 4)")
                .contains("'size' on 'beam': value is not finite"),
            "non-finite consumer value"
        );
        // Only top-level lets.
        assert!(
            with_lets("group g {\n  let w = 4\n  box inner size=(4, w, 4)\n}")
                .contains("let is only allowed at the top level"),
            "let inside a group"
        );
        // Non-consumer vocabulary refuses expression values.
        assert!(
            with_lets("let w = 4\nfill gyroid sheet cell=w").contains("'cell' must be a number"),
            "fill cell"
        );
        assert!(
            with_lets("let w = 4\nbox beam size=(4, 4, 4) blend=w")
                .contains("'blend' must be a number"),
            "blend"
        );
        assert!(
            with_lets("let w = 4\nsolve size=w").contains("size must be a number or 'auto'"),
            "solve size"
        );
        assert!(
            with_lets("let w = 4\nsolve modes=w").contains("modes must be a count"),
            "solve modes"
        );
        assert!(
            with_lets("let w = 4\nload push t pressure=w").contains("'pressure' must be a number"),
            "load pressure"
        );
        assert!(
            with_lets("let w = 4\ntemperature hot t temp=w").contains("'temp' must be a number"),
            "temperature value"
        );
        // Tokenizer: unknown characters still fail before any expression work.
        assert!(
            with_lets("let w!x = 4").contains("unexpected character"),
            "bad let name"
        );
    }
    #[test]
    fn const_folded_arithmetic_needs_no_let() {
        let d =
            parse("fieldsmith 1\nname \"t\"\nmaterial petg\nbox beam size=(2 - 1, 2 + 2, 2 * 6)")
                .unwrap();
        assert!(d.lets.is_empty());
        assert!(d.consumed.is_empty());
        assert!(print(&d).contains("size=(1, 4, 12)"), "{}", print(&d));
    }

    #[test]
    fn json_round_trips_lets_and_consumers() {
        let d = parse(LET_SAMPLE).unwrap();
        let json = serde_json::to_string(&d).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
        // Let-less documents keep today's JSON shape.
        let plain_json = serde_json::to_string(&parse(PLAIN_BRACKET).unwrap()).unwrap();
        assert!(
            !plain_json.contains("consumed"),
            "empty fields stay omitted"
        );
    }

    #[test]
    fn validate_catches_stale_let_and_consumer_records() {
        // A json-built document bypasses the parse-time checks, so validate() is the
        // backstop for hand-edited lets and consumers.
        let mut d = parse(LET_SAMPLE).unwrap();
        d.lets[0].name = "wall_t2".into();
        let errors = d.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("does not evaluate") && e.contains("wall_t")),
            "{errors:?}"
        );
        let mut d = parse(LET_SAMPLE).unwrap();
        d.consumed[0].target = "ghost".into();
        let errors = d.validate();
        assert!(
            errors.iter().any(|e| e.contains("references 'ghost'")),
            "{errors:?}"
        );
        let mut d = parse(LET_SAMPLE).unwrap();
        d.lets[0].name = "push".into();
        let errors = d.validate();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("Name 'push' is used more than once")),
            "{errors:?}"
        );
    }

    #[test]
    fn material_label_round_trips() {
        let text = r#"fieldsmith 1
name "t"
material petg

box body size=(40, 10, 10)
cylinder clip r=2 h=12 at=(10, 20, 6)
group sub {
  box inner size=(4, 4, 4)
  material abs on inner
}
material abs on body
material abs
"#;
        let d = parse(text).unwrap();
        let canon = print(&d);
        assert_eq!(parse(&canon).unwrap(), d, "second parse differs:\n{canon}");
        // Statements before and after their shape all resolve; the document material is
        // the last unqualified one.
        assert_eq!(d.material, "abs");
        assert_eq!(
            d.material_labels(),
            [
                ("body".to_string(), "abs".to_string()),
                ("inner".to_string(), "abs".to_string())
            ],
            "labels in document order"
        );
    }

    #[test]
    fn material_label_forwards_and_names_its_missing_target() {
        // The label may precede the shape it names.
        let d =
            parse("fieldsmith 1\nmaterial petg\nmaterial abs on late\nbox late size=(1, 1, 1)\n")
                .unwrap();
        assert_eq!(
            d.material_labels(),
            vec![("late".to_string(), "abs".to_string())]
        );
        // A target the document never declares fails by name (the label is not dropped).
        let e = parse("fieldsmith 1\nmaterial petg\nmaterial abs on ghost\n").unwrap_err();
        assert!(e.message.contains("ghost"), "{}", e.message);
    }

    #[test]
    fn material_validate_and_warnings() {
        // An unknown label id is an error.
        let d = parse(
            "fieldsmith 1\nmaterial petg\nbox body size=(1, 1, 1)\nmaterial ghostmat on body\n",
        )
        .unwrap();
        let errs = d.validate();
        assert!(
            errs.iter()
                .any(|e| e.contains("Unknown material 'ghostmat' on 'body'")),
            "{errs:?}"
        );

        // A label the document knows but different from the document material warns;
        // identical labels and suppressed nodes do not.
        let d = parse("fieldsmith 1\nmaterial petg\nbox body size=(1, 1, 1)\nbox plain size=(1, 1, 1)\nbox gone size=(1, 1, 1) suppressed\nmaterial abs on body\nmaterial abs on gone\n").unwrap();
        assert!(d.validate().is_empty(), "{:?}", d.validate());
        let w = d.material_warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("'body' (abs)"), "{w:?}");
        assert!(w[0].contains("petg"), "{w:?}");

        // Same-material labels stay silent.
        let d =
            parse("fieldsmith 1\nmaterial petg\nbox body size=(1, 1, 1)\nmaterial petg on body\n")
                .unwrap();
        assert!(d.material_warnings().is_empty());
    }

    #[test]
    fn colour_round_trips_and_canonicalises() {
        // The hex travels quoted (a bare `#` starts a comment); input case is not kept.
        let text = r##"fieldsmith 1
name "t"
material petg

box body size=(40, 10, 10)
group sub {
  box inner size=(4, 4, 4)
  material abs on inner
  color "#21bb4c" on inner
}
color "#b5432a" on body
"##;
        let d = parse(text).unwrap();
        let canon = print(&d);
        assert!(
            canon.contains("color \"#21BB4C\" on inner")
                && canon.contains("color \"#B5432A\" on body"),
            "{canon}"
        );
        assert_eq!(parse(&canon).unwrap(), d, "second parse differs:\n{canon}");
        assert_eq!(d.find("body").unwrap().color.as_deref(), Some("#B5432A"));
        match &d.find("sub").unwrap().kind {
            NodeKind::Group { children, .. } => {
                assert_eq!(children[0].color.as_deref(), Some("#21BB4C"));
            }
            other => panic!("not a group: {other:?}"),
        }
    }

    #[test]
    fn colour_accepts_the_alpha_form() {
        let d = parse("fieldsmith 1\nname \"t\"\nbox b size=(1, 1, 1)\ncolor \"#11223344\" on b\n")
            .unwrap();
        assert_eq!(d.find("b").unwrap().color.as_deref(), Some("#11223344"));
        // The .fsmz JSON side carries the canonical string, absent on colourless nodes.
        let jsoned = serde_json::to_value(&d).unwrap();
        let b = jsoned
            .get("root")
            .and_then(|r| r.get(0))
            .and_then(|n| n.get("color"))
            .and_then(|c| c.as_str());
        assert_eq!(b, Some("#11223344"));
        let plain = parse("fieldsmith 1\nname \"t\"\nbox b size=(1, 1, 1)\n").unwrap();
        let jsoned = serde_json::to_value(&plain).unwrap();
        assert!(jsoned
            .get("root")
            .unwrap()
            .get(0)
            .unwrap()
            .get("color")
            .is_none());
    }

    #[test]
    fn colour_errors_are_named() {
        // Bad hex names itself on its own line.
        let e = parse("fieldsmith 1\nbox b size=(1, 1, 1)\ncolor \"#11GG33\" on b\n").unwrap_err();
        assert_eq!(e.line, 3, "{}", e.message);
        assert!(e.message.contains("#11GG33"), "{}", e.message);
        // A bare `#` opens a comment, so the hex must travel quoted.
        let e = parse("fieldsmith 1\nbox b size=(1, 1, 1)\ncolor on b\n").unwrap_err();
        assert!(e.message.contains("quoted"), "{}", e.message);
        // A colour without a target and an unknown target are both named.
        let e = parse("fieldsmith 1\nbox b size=(1, 1, 1)\ncolor \"#112233\"\n").unwrap_err();
        assert!(e.message.contains("color"), "{}", e.message);
        let e =
            parse("fieldsmith 1\nbox b size=(1, 1, 1)\ncolor \"#112233\" on ghost\n").unwrap_err();
        assert!(e.message.contains("ghost"), "{}", e.message);

        // validate catches a hand-edited non-hex value; a valid one passes, and clearing
        // works like the material lane does.
        let mut d = parse("fieldsmith 1\nbox b size=(1, 1, 1)\nmaterial petg\n").unwrap();
        d.set_node_color("b", Some("red".into()));
        let errs = d.validate();
        assert!(
            errs.iter().any(|e| e.contains("not a hex colour")),
            "{errs:?}"
        );
        d.set_node_color("b", Some("#21BB4C".into()));
        assert!(d.validate().is_empty(), "{:?}", d.validate());
        d.set_node_color("b", None);
        assert_eq!(d.find("b").unwrap().color, None);
    }

    #[test]
    fn reports_errors_with_lines() {
        let e = parse("fieldsmith 1\nbox b size=(1, 2)\n").unwrap_err();
        assert_eq!(e.line, 2);
        let e = parse("fieldsmith 1\nwobble\n").unwrap_err();
        assert!(e.message.contains("unknown keyword"));
        let e = parse("fieldsmith 1\ngroup g {\nsphere s r=1\n").unwrap_err();
        assert!(e.message.contains("missing '}'"));
    }

    #[test]
    fn distributed_load_round_trips() {
        let text = r#"fieldsmith 1
name "t"
material petg

box b size=(40, 10, 10)
region face = b.px
load w face fn="-0.1 < x < 0.1 -> 1; 0.1 < x < 0.9 -> -1" local=(1, 0, 0) case="Load 2"
"#;
        let d = parse(text).unwrap();
        let canon = print(&d);
        assert_eq!(parse(&canon).unwrap(), d, "second parse differs:\n{canon}");
        assert!(
            canon.contains(r#"fn="-0.1 < x < 0.1 -> 1; 0.1 < x < 0.9 -> -1""#),
            "{canon}"
        );
        match &d.loads[0].kind {
            LoadKind::Distributed {
                expr,
                dir: LoadDir::Local { v },
                total,
            } => {
                assert_eq!(expr, "-0.1 < x < 0.1 -> 1; 0.1 < x < 0.9 -> -1");
                assert_eq!(*v, [1.0, 0.0, 0.0]);
                assert_eq!(*total, None);
            }
            other => panic!("wrong kind: {other:?}"),
        }
    }

    #[test]
    fn distributed_load_errors_are_named() {
        let e = parse(
            r#"fieldsmith 1
name "t"
material petg
box b size=(40, 10, 10)
region r = b.px
load w r fn="0.1 < x < 1 -> w * 2" dir=(0, 0, -1)
"#,
        )
        .unwrap_err();
        assert!(e.message.contains("unknown name 'w'"), "{}", e.message);

        let e = parse(
            r#"fieldsmith 1
name "t"
material petg
box b size=(40, 10, 10)
region r = b.px
load w r fn="1" case="Load 2"
"#,
        )
        .unwrap_err();
        assert!(e.message.contains("needs dir="), "{}", e.message);

        // JSON-authored docs slip past the parser's checks; validate catches them.
        let d = parse(
            r#"fieldsmith 1
name "t"
material petg
box b size=(40, 10, 10)
region r = b.px
load w r fn="1 / (x - x)" dir=(0, 0, 0) total=0
"#,
        )
        .unwrap();
        let e = d.validate().join(" | ");
        assert!(e.contains("zero vector"), "{e}");
        assert!(e.contains("total=0"), "{e}");
        // A variable-dependent division is a solve-time fault, not a static one.
        assert!(!e.contains("divides"), "{e}");
    }

    #[test]
    fn temperature_round_trips() {
        let text = r#"fieldsmith 1
name "t"
material alloy

custommaterial alloy name="Test alloy" e=70000 nu=0.33 yield=230 density=2.67 wall=0.4 a=2.3e-5 source="round-trip fixture"

box b size=(40, 10, 10)
region hotface = b.px
region coldface = b.nx
temperature hot hotface temp=80 case="Thermal 2"
temperature cold coldface temp=20
solve size=2.5 tref=20
"#;
        let d = parse(text).unwrap();
        let canon = print(&d);
        assert_eq!(parse(&canon).unwrap(), d, "second parse differs:\n{canon}");
        // Cases print explicitly, like load cases do — the default is spelled out.
        assert!(
            canon.contains(r#"temperature hot hotface temp=80 case="Thermal 2""#)
                && canon.contains(r#"temperature cold coldface temp=20 case="Thermal 1""#)
                && canon.contains("solve size=2.5 tref=20")
                && canon.contains("a=0.000023"),
            "{canon}"
        );
        assert_eq!(d.thermal_cases(), ["Thermal 2", "Thermal 1"]);
        assert_eq!(d.temperatures[0].celsius, 80.0);
        assert_eq!(d.temperatures[1].case, "Thermal 1");
        assert_eq!(d.solve.t_ref, Some(20.0));
        assert_eq!(d.custom_materials[0].alpha, Some(2.3e-5));
        assert!(d.validate().is_empty(), "{:?}", d.validate());
    }

    #[test]
    fn temperatures_validate_with_named_errors() {
        let base = |extra: &str| {
            parse(&format!(
                r#"fieldsmith 1
name "t"
material petg

box b size=(40, 10, 10)
region face = b.px
{extra}
"#
            ))
            .unwrap()
        };

        // A temperature on a region the document never declares.
        let e = base("temperature hot ghost temp=80").validate().join(" | ");
        assert!(e.contains("refers to missing region 'ghost'"), "{e}");

        // Two unequal values on one region in one case fight over the same nodes.
        let e = base("temperature hot face temp=80\ntemperature cold face temp=60")
            .validate()
            .join(" | ");
        assert!(e.contains("conflicting values"), "{e}");

        // Two equal values on one region are allowed — the nodes agree, so nothing fights.
        assert!(
            base("temperature hot face temp=80\ntemperature warm face temp=80")
                .validate()
                .is_empty()
        );

        // A case name claimed by both a load and a temperature cannot be solved honestly.
        let e = base(
            r#"temperature hot face temp=80 case="Load 1"
load w face pressure=0.25 case="Load 1""#,
        )
        .validate()
        .join(" | ");
        assert!(
            e.contains("claimed by load 'w' and temperature 'hot'"),
            "{e}"
        );

        // The library PETG carries a sourced alpha, so a plain thermal doc validates.
        let d = parse(
            "fieldsmith 1\nmaterial petg\nbox b size=(1, 1, 1)\nregion f = b.px\ntemperature hot f temp=80\n",
        )
        .unwrap();
        assert!(d.validate().is_empty(), "{:?}", d.validate());

        // A custom material without a= is refused by name, not solved with an assumed one.
        let e = base(
            "custommaterial bare name=\"Bare\" e=70000 nu=0.33 yield=230 density=2.67 wall=0.4 source=\"fixture\"\nmaterial bare\ntemperature hot face temp=80",
        )
        .validate()
        .join(" | ");
        assert!(
            e.contains("thermal-expansion coefficient") && e.contains("bare"),
            "{e}"
        );

        // Non-finite values are caught by validate; the parser cannot express them, so
        // reach them the way a JSON-authored document would (fields are public).
        let mut d = base("temperature hot face temp=80");
        d.temperatures[0].celsius = f64::NAN;
        let e = d.validate().join(" | ");
        assert!(e.contains("not a finite number"), "{e}");
        d.temperatures[0].celsius = 80.0;
        d.solve.t_ref = Some(f64::NAN);
        let e = d.validate().join(" | ");
        assert!(e.contains("tref= is not a finite number"), "{e}");

        // tref= parse errors are named.
        let text = |line: &str| {
            format!(
                "fieldsmith 1\nname \"t\"\nmaterial petg\nbox b size=(40, 10, 10)\n\
                 region face = b.px\ntemperature hot face temp=80\nsolve {line}\n"
            )
        };
        let e = parse(&text("tref")).unwrap_err();
        assert!(e.message.contains("tref wants a °C value"), "{}", e.message);
        let e = parse(&text("tref=wat")).unwrap_err();
        assert!(e.message.contains("tref must be a number"), "{}", e.message);
    }

    #[test]
    fn convection_round_trips() {
        let text = r#"fieldsmith 1
name "t"
material petg

let air_h = 1e-5

box b size=(40, 10, 10)
region fin = b.py
region hotface = b.px
convection wall fin h=2 * air_h tinf=20 case="Thermal 2"
convection bath hotface h=0.1 tinf=90
solve size=2.5 tref=20
"#;
        let d = parse(text).unwrap();
        let canon = print(&d);
        assert_eq!(parse(&canon).unwrap(), d, "second parse differs:\n{canon}");
        // Cases print explicitly, and a driven h prints its expression.
        assert!(
            canon.contains(r#"convection wall fin h=2 * air_h tinf=20 case="Thermal 2""#)
                && canon.contains(r#"convection bath hotface h=0.1 tinf=90 case="Thermal 1""#),
            "{canon}"
        );
        // Convection-only case names join the thermal case list.
        assert_eq!(d.thermal_cases(), ["Thermal 2", "Thermal 1"]);
        assert_eq!(d.convections[0].h, 2e-5);
        assert_eq!(d.convections[0].t_inf, 20.0);
        assert_eq!(d.convections[1].case, "Thermal 1");
        // The driven h recorded a consumer, so the param writer routes edits through it.
        assert!(
            d.consumed.iter().any(|c| c.target == "wall"
                && c.attr == "h"
                && c.comp.is_none()
                && c.expr == "2 * air_h"),
            "{:?}",
            d.consumed
        );
        // tinf= is a plain number here, so it did not record one.
        assert!(
            !d.consumed
                .iter()
                .any(|c| c.target == "bath" && c.attr == "tinf"),
            "{:?}",
            d.consumed
        );
        assert!(d.validate().is_empty(), "{:?}", d.validate());
    }

    #[test]
    fn convections_validate_with_named_errors() {
        let base = |extra: &str| {
            parse(&format!(
                r#"fieldsmith 1
name "t"
material petg

box b size=(40, 10, 10)
region face = b.px
{extra}
"#
            ))
            .unwrap()
        };

        // A convection on a region the document never declares.
        let e = base("convection hot ghost h=0.1 tinf=20")
            .validate()
            .join(" | ");
        assert!(e.contains("refers to missing region 'ghost'"), "{e}");

        // Negative h, and a NaN the parser cannot express, are caught by name.
        let e = base("convection hot face h=-0.1 tinf=20")
            .validate()
            .join(" | ");
        assert!(e.contains("h= must be a finite number ≥ 0"), "{e}");
        let mut d = base("convection hot face h=0.1 tinf=20");
        d.convections[0].h = f64::NAN;
        let e = d.validate().join(" | ");
        assert!(e.contains("h= must be a finite number ≥ 0"), "{e}");
        let mut d = base("convection hot face h=0.1 tinf=20");
        d.convections[0].t_inf = f64::NAN;
        let e = d.validate().join(" | ");
        assert!(e.contains("tinf= is not a finite number"), "{e}");

        // Two unequal bands on one region in one case would double-assemble.
        let e = base("convection hot face h=0.1 tinf=20\nconvection cold face h=0.2 tinf=20")
            .validate()
            .join(" | ");
        assert!(e.contains("set conflicting values"), "{e}");
        // Two equal bands agree, so nothing fights.
        assert!(
            base("convection hot face h=0.1 tinf=20\nconvection same face h=0.1 tinf=20")
                .validate()
                .is_empty()
        );

        // A case name claimed by both a load and a convection cannot be solved honestly.
        let e = base(
            r#"convection hot face h=0.1 tinf=20 case="Load 1"
load w face pressure=0.25 case="Load 1""#,
        )
        .validate()
        .join(" | ");
        assert!(
            e.contains("claimed by load 'w' and convection 'hot'"),
            "{e}"
        );

        // A fixed temperature on the same region is legal — the fixed value wins there.
        assert!(
            base("convection hot face h=0.1 tinf=20\ntemperature wall face temp=80")
                .validate()
                .is_empty()
        );

        // A custom material without a= is refused even for convection-only thermal work.
        let e = base(
            "custommaterial bare name=\"Bare\" e=70000 nu=0.33 yield=230 density=2.67 wall=0.4 source=\"fixture\"\nmaterial bare\nconvection hot face h=0.1 tinf=20",
        )
        .validate()
        .join(" | ");
        assert!(
            e.contains("thermal-expansion coefficient") && e.contains("bare"),
            "{e}"
        );
    }

    #[test]
    fn convection_parse_errors_are_named() {
        let body = |line: &str| {
            format!(
                "fieldsmith 1\nname \"t\"\nmaterial petg\nbox b size=(40, 10, 10)\n\
                 region face = b.px\n{line}\n"
            )
        };
        let e = parse(&body("convection band face tinf=20")).unwrap_err();
        assert!(e.message.contains("missing 'h='"), "{}", e.message);
        let e = parse(&body("convection band face h=0.1")).unwrap_err();
        assert!(e.message.contains("missing 'tinf='"), "{}", e.message);
        // A bare let name is refused — the sweepable address is the let itself.
        assert!(
            parse(&body("let w = 4\nconvection band face h=w tinf=20"))
                .unwrap_err()
                .message
                .contains("'h' must be a number"),
            "bare let name as h"
        );
        assert!(
            parse(&body("let w = 4\nconvection w face h=1 tinf=20"))
                .unwrap_err()
                .message
                .contains("Name 'w' is used more than once"),
            "let name collision"
        );
        assert!(
            parse(&body("convection band face h=q + 1 tinf=20"))
                .unwrap_err()
                .message
                .contains("'h' on 'band': no let named 'q'"),
            "undefined name in h"
        );
    }

    #[test]
    fn heatflux_round_trips() {
        let text = r#"fieldsmith 1
name "t"
material petg

let qv = 1e-3

box b size=(40, 10, 10)
region hotface = b.px
region coldface = b.nx
heatflux lead hotface q=2 * qv case="Thermal 2"
heatflux outflow coldface q=-0.5 case="Thermal 1"
solve size=2.5 tref=20
"#;
        let d = parse(text).unwrap();
        let canon = print(&d);
        assert_eq!(parse(&canon).unwrap(), d, "second parse differs:\n{canon}");
        // Cases print explicitly, and a driven q prints its expression.
        assert!(
            canon.contains(r#"heatflux lead hotface q=2 * qv case="Thermal 2""#)
                && canon.contains(r#"heatflux outflow coldface q=-0.5 case="Thermal 1""#),
            "{canon}"
        );
        // Heat-flux-only case names join the thermal case list.
        assert_eq!(d.thermal_cases(), ["Thermal 2", "Thermal 1"]);
        assert_eq!(d.heat_fluxes[0].q, 2e-3);
        assert_eq!(d.heat_fluxes[1].q, -0.5);
        // The driven q recorded a consumer, so the param writer routes edits through it.
        assert!(
            d.consumed.iter().any(|c| c.target == "lead"
                && c.attr == "q"
                && c.comp.is_none()
                && c.expr == "2 * qv"),
            "{:?}",
            d.consumed
        );
        // The plain-number q did not record one.
        assert!(
            !d.consumed
                .iter()
                .any(|c| c.target == "outflow" && c.attr == "q"),
            "{:?}",
            d.consumed
        );
        assert!(d.validate().is_empty(), "{:?}", d.validate());
    }

    #[test]
    fn heatflux_validate_with_named_errors() {
        let base = |extra: &str| {
            parse(&format!(
                r#"fieldsmith 1
name "t"
material petg

box b size=(40, 10, 10)
region face = b.px
{extra}
"#
            ))
            .unwrap()
        };

        // A heat flux on a region the document never declares.
        let e = base("heatflux hot ghost q=0.1").validate().join(" | ");
        assert!(e.contains("refers to missing region 'ghost'"), "{e}");

        // A NaN the parser cannot express is caught by name; −q is legal (outflow).
        let mut d = base("heatflux hot face q=0.1");
        d.heat_fluxes[0].q = f64::NAN;
        let e = d.validate().join(" | ");
        assert!(e.contains("q= is not a finite number"), "{e}");
        assert!(base("heatflux out face q=-0.4").validate().is_empty());

        // Two flux statements on one region in one case superpose — nothing fights.
        assert!(base("heatflux a face q=0.1\nheatflux b face q=0.2")
            .validate()
            .is_empty());

        // A case name claimed by both a load and a heat flux cannot be solved honestly.
        let e = base(
            r#"heatflux hot face q=0.1 case="Load 1"
load w face pressure=0.25 case="Load 1""#,
        )
        .validate()
        .join(" | ");
        assert!(e.contains("claimed by load 'w' and heat flux 'hot'"), "{e}");

        // A fixed temperature or a convection on the same region is legal.
        assert!(
            base("heatflux hot face q=0.1\ntemperature wall face temp=80")
                .validate()
                .is_empty()
        );
        assert!(
            base("heatflux hot face q=0.1\nconvection bath face h=0.1 tinf=20")
                .validate()
                .is_empty()
        );

        // A custom material without a= is refused even for flux-only thermal work.
        let e = base(
            "custommaterial bare name=\"Bare\" e=70000 nu=0.33 yield=230 density=2.67 wall=0.4 source=\"fixture\"\nmaterial bare\nheatflux hot face q=0.1",
        )
        .validate()
        .join(" | ");
        assert!(
            e.contains("thermal-expansion coefficient") && e.contains("bare"),
            "{e}"
        );
    }

    #[test]
    fn heatflux_parse_errors_are_named() {
        let body = |line: &str| {
            format!(
                "fieldsmith 1\nname \"t\"\nmaterial petg\nbox b size=(40, 10, 10)\n\
                 region face = b.px\n{line}\n"
            )
        };
        let e = parse(&body("heatflux hot face")).unwrap_err();
        assert!(e.message.contains("missing 'q='"), "{}", e.message);
        // A bare let name is refused — the sweepable address is the let itself.
        assert!(
            parse(&body("let qv = 1e-3\nheatflux band face q=qv"))
                .unwrap_err()
                .message
                .contains("'q' must be a number"),
            "bare let name as q"
        );
        assert!(
            parse(&body("let w = 4\nheatflux w face q=1"))
                .unwrap_err()
                .message
                .contains("Name 'w' is used more than once"),
            "let name collision"
        );
        assert!(
            parse(&body("heatflux band face q=q + 1"))
                .unwrap_err()
                .message
                .contains("'q' on 'band': no let named 'q'"),
            "undefined name in q"
        );
    }

    #[test]
    fn json_round_trip() {
        let d = parse(SAMPLE).unwrap();
        let j = serde_json::to_string(&d).unwrap();
        let d2: Document = serde_json::from_str(&j).unwrap();
        assert_eq!(d, d2);
    }

    const MESH_SAMPLE: &str = r#"fieldsmith 1
name "Import"
material petg

mesh mesh1 src="/tmp/part.stl" units=m up=y vox=0.8 hash="0123456789abcdef" bbox=(0, 0, 0, 15.24, 17.25, 5) tris=87286 patches=6 open_edges=0

region bottom = mesh1.p3 + mesh1.px
support hold bottom
load press bottom pressure=0.5 case="Load 1"
"#;

    #[test]
    fn mesh_node_and_patch_selector_round_trip() {
        let d = parse(MESH_SAMPLE).unwrap();
        assert_eq!(
            print(&d),
            MESH_SAMPLE,
            "printer output differs:\n{}",
            print(&d)
        );
        match &d.root[0].kind {
            NodeKind::Mesh {
                src,
                hash,
                units,
                up,
                voxel,
                bbox,
                tris,
                patches,
                open_edges,
            } => {
                assert_eq!(src, "/tmp/part.stl");
                assert_eq!(hash, "0123456789abcdef");
                assert_eq!((units.as_str(), up.as_str(), *voxel), ("m", "y", 0.8));
                assert_eq!(*bbox, [0.0, 0.0, 0.0, 15.24, 17.25, 5.0]);
                assert_eq!((*tris, *patches, *open_edges), (87286, 6, 0));
            }
            _ => panic!("expected a mesh node"),
        }
        // patch selectors keep face All and the id; face selectors stay untouched
        assert_eq!(d.regions[0].selectors[0].patch, Some(3));
        assert_eq!(d.regions[0].selectors[1].face, Face::Px);
        assert!(d.validate().is_empty(), "{:?}", d.validate());
        // JSON round trip keeps the patch (skip_serializing_if must not drop it)
        let j = serde_json::to_string(&d).unwrap();
        let d2: Document = serde_json::from_str(&j).unwrap();
        assert_eq!(d, d2);
    }

    #[test]
    fn mesh_validate_reports_bad_fields() {
        let bad = |kw_line: &str| {
            let d = parse(&format!(
                "fieldsmith 1\nname \"n\"\nmaterial petg\n{kw_line}\n"
            ));
            d.map(|d| d.validate())
        };
        // non-hex hash
        let errs = bad(
            "mesh m1 src=\"s\" units=m up=y vox=0.8 hash=\"zzz\" bbox=(0, 0, 0, 1, 1, 1) tris=1 patches=0 open_edges=0",
        ).unwrap();
        assert!(
            errs.iter().any(|e| e.contains("hex fingerprint")),
            "{errs:?}"
        );
        // bad units and up-axis
        let errs = bad(
            "mesh m1 src=\"s\" units=km up=x vox=0.8 hash=\"0123456789abcdef\" bbox=(0, 0, 0, 1, 1, 1) tris=1 patches=0 open_edges=0",
        ).unwrap();
        assert!(errs.iter().any(|e| e.contains("units")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("up-axis")), "{errs:?}");
        // inverted bounds
        let errs = bad(
            "mesh m1 src=\"s\" units=m up=y vox=0.8 hash=\"0123456789abcdef\" bbox=(2, 0, 0, 1, 1, 1) tris=1 patches=0 open_edges=0",
        ).unwrap();
        assert!(errs.iter().any(|e| e.contains("inverted")), "{errs:?}");
        // patch selector on a non-mesh node, and patch 0
        let errs = bad("box b size=(1, 2, 3)\nregion r = b.p1\n").unwrap();
        assert!(
            errs.iter().any(|e| e.contains("not a mesh node")),
            "{errs:?}"
        );
        let errs = bad(
            "mesh m1 src=\"s\" units=m up=y vox=0.8 hash=\"0123456789abcdef\" bbox=(0, 0, 0, 1, 1, 1) tris=1 patches=0 open_edges=0\nregion r = m1.p0\n",
        ).unwrap();
        assert!(
            errs.iter().any(|e| e.contains("patches start at 1")),
            "{errs:?}"
        );
        // selector typo reads as an unknown face at parse time
        let e = parse("fieldsmith 1\nbox b size=(1, 2, 3)\nregion r = b.qx\n").unwrap_err();
        assert!(e.message.contains("unknown face"));
    }

    const EXTEND_SAMPLE: &str = r#"fieldsmith 1
name "Skirt"
material petg

mesh mesh1 src="/tmp/part.stl" units=m up=y vox=0.8 hash="0123456789abcdef" bbox=(0, 0, 0, 15.24, 17.25, 5) tris=87286 patches=6 open_edges=0
extend skirt bottom dir=(0, 0, -1) len=10

region bottom = mesh1.p1
"#;

    #[test]
    fn extend_node_round_trip() {
        let d = parse(EXTEND_SAMPLE).unwrap();
        assert_eq!(
            print(&d),
            EXTEND_SAMPLE,
            "printer output differs:\n{}",
            print(&d)
        );
        match &d.root[1].kind {
            NodeKind::Extend { region, dir, len } => {
                assert_eq!(region, "bottom");
                assert_eq!(*dir, [0.0, 0.0, -1.0]);
                assert_eq!(*len, 10.0);
            }
            _ => panic!("expected an extend node"),
        }
        assert!(d.validate().is_empty(), "{:?}", d.validate());
        // JSON round trip keeps the sweep
        let j = serde_json::to_string(&d).unwrap();
        let d2: Document = serde_json::from_str(&j).unwrap();
        assert_eq!(d, d2);
    }

    #[test]
    fn extend_validate_reports_bad_fields_and_budget() {
        let mesh_line = "mesh mesh1 src=\"s\" units=m up=y vox=0.8 hash=\"0123456789abcdef\" bbox=(0, 0, 0, 15.24, 17.25, 5) tris=1 patches=2 open_edges=0";
        let bad = |body: &str| {
            let d = parse(&format!(
                "fieldsmith 1\nname \"n\"\nmaterial petg\n{mesh_line}\n{body}\n"
            ))
            .unwrap();
            d.validate()
        };
        // missing region, patchless region, zero dir, non-positive len
        let errs = bad("extend e1 nope dir=(0, 0, -1) len=10\n");
        assert!(
            errs.iter().any(|e| e.contains("missing region")),
            "{errs:?}"
        );
        let errs = bad(
            "box b size=(1, 2, 3)\nregion plain = b.px\nextend e1 plain dir=(0, 0, -1) len=10\n",
        );
        assert!(
            errs.iter().any(|e| e.contains("names no mesh patch")),
            "{errs:?}"
        );
        let errs = bad("region bottom = mesh1.p1\nextend e1 bottom dir=(0, 0, 0) len=10\n");
        assert!(errs.iter().any(|e| e.contains("no direction")), "{errs:?}");
        let errs = bad("region bottom = mesh1.p1\nextend e1 bottom dir=(0, 0, -1) len=0\n");
        assert!(
            errs.iter().any(|e| e.contains("must be positive")),
            "{errs:?}"
        );
        // a fine parent voxel blows the sweep-grid budget
        let errs = bad("region bottom = mesh1.p1\nextend e1 bottom dir=(0, 0, -1) len=10\n")
            .into_iter()
            .collect::<Vec<_>>();
        assert!(errs.is_empty(), "{errs:?}");
        let voxel_line = "mesh mesh1 src=\"s\" units=m up=y vox=0.02 hash=\"0123456789abcdef\" bbox=(0, 0, 0, 15, 17, 5) tris=1 patches=2 open_edges=0";
        let d = parse(&format!(
            "fieldsmith 1\nname \"n\"\nmaterial petg\n{voxel_line}\nregion bottom = mesh1.p1\nextend e1 bottom dir=(0, 0, -1) len=10\n"
        ))
        .unwrap();
        let errs = d.validate();
        assert!(errs.iter().any(|e| e.contains("voxel cells")), "{errs:?}");
    }

    #[test]
    fn extend_accepts_a_face_region_and_rejects_bare_all_and_side() {
        let mesh_line = "mesh mesh1 src=\"s\" units=m up=y vox=0.8 hash=\"0123456789abcdef\" bbox=(0, 0, 0, 15.24, 17.25, 5) tris=1 patches=0 open_edges=0";
        // A geometric face of the mesh is a legal sweep source even with no patch grid.
        let d = parse(&format!(
            "fieldsmith 1\nname \"n\"\nmaterial petg\n{mesh_line}\nregion bottom = mesh1.bottom\nextend e1 bottom dir=(0, 0, -1) len=10\n"
        ))
        .unwrap();
        assert!(d.validate().is_empty(), "{:?}", d.validate());
        // `all` on a mesh is not a sweep target (every normal); extend refuses it.
        let d = parse(&format!(
            "fieldsmith 1\nname \"n\"\nmaterial petg\n{mesh_line}\nregion all = mesh1.all\nextend e1 all dir=(0, 0, -1) len=10\n"
        ))
        .unwrap();
        let errs = d.validate();
        assert!(
            errs.iter()
                .any(|e| e.contains("names no mesh patch or face")),
            "{errs:?}"
        );
        // `side` on a mesh is refused outright: a mesh has no side face.
        let d = parse(&format!(
            "fieldsmith 1\nname \"n\"\nmaterial petg\n{mesh_line}\nregion side = mesh1.side\n"
        ))
        .unwrap();
        let errs = d.validate();
        assert!(errs.iter().any(|e| e.contains("no side face")), "{errs:?}");
    }

    #[test]
    fn extend_face_selector_names_the_swept_face() {
        // M7: face selectors on derived solids. A region selecting a geometric face of
        // an extend node is valid (the extend's derived leaf carries the face hint), and
        // `all`/`side` on an extend are refused — the swept solid has no side wall and
        // `all` is not a face.
        let mesh_line = "mesh mesh1 src=\"s\" units=m up=y vox=0.8 hash=\"0123456789abcdef\" bbox=(0, 0, 0, 15.24, 17.25, 5) tris=1 patches=2 open_edges=0";
        let doc = |body: &str| {
            parse(&format!(
                "fieldsmith 1\nname \"n\"\nmaterial petg\n{mesh_line}\n{body}\n"
            ))
            .unwrap()
        };
        // A face selector on the extend node validates clean.
        let d = doc(
            "region bottom = mesh1.p1\nextend skirt bottom dir=(0, 0, -1) len=10\nregion wall = skirt.nz\n",
        );
        assert!(d.validate().is_empty(), "{:?}", d.validate());
        // `all` on an extend is not a face.
        let d = doc(
            "region bottom = mesh1.p1\nextend skirt bottom dir=(0, 0, -1) len=10\nregion all = skirt.all\n",
        );
        let errs = d.validate();
        assert!(
            errs.iter().any(|e| e.contains("exposes its swept face")),
            "{errs:?}"
        );
        // `side` on an extend is refused: the swept solid has no side wall.
        let d = doc(
            "region bottom = mesh1.p1\nextend skirt bottom dir=(0, 0, -1) len=10\nregion side = skirt.side\n",
        );
        let errs = d.validate();
        assert!(
            errs.iter().any(|e| e.contains("exposes its swept face")),
            "{errs:?}"
        );
    }

    #[test]
    fn mesh_modifier_validate_names_the_fallback_conditions() {
        // One mesh node per document (ids are unique); the group wraps a single mesh
        // child, so the modifier check sees exactly the group's contents.
        let mesh_line = "mesh mesh1 src=\"s\" units=m up=y vox=0.8 hash=\"0123456789abcdef\" bbox=(0, 0, 0, 15.24, 17.25, 5) tris=1 patches=2 open_edges=0";
        let doc = |body: &str| {
            parse(&format!(
                "fieldsmith 1\nname \"n\"\nmaterial petg\n{body}\n"
            ))
            .unwrap()
        };
        // A single mesh under a shell bakes: no fallback named.
        let d = doc(&format!(
            "group g {{\n  {mesh_line}\n  shell 2 inside\n}}\n"
        ));
        assert!(d.validate().is_empty(), "{:?}", d.validate());
        // A group of two (mesh + box) keeps the eval-time wrap — named.
        let d = doc(&format!(
            "group g {{\n  {mesh_line}\n  box b size=(1, 2, 3)\n  offset 1\n}}\n"
        ));
        let errs = d.validate();
        assert!(
            errs.iter().any(|e| e.contains("keeps the eval-time form")),
            "{errs:?}"
        );
        // Two meshes also keep the wrap.
        let mesh2_line = "mesh mesh2 src=\"s\" units=m up=y vox=0.8 hash=\"fedcba9876543210\" bbox=(0, 0, 0, 1, 1, 1) tris=1 patches=0 open_edges=0";
        let d = doc(&format!(
            "group g {{\n  {mesh_line}\n  {mesh2_line}\n  shell 1 outside\n}}\n"
        ));
        let errs = d.validate();
        assert!(
            errs.iter().any(|e| e.contains("keeps the eval-time form")),
            "{errs:?}"
        );
        // A suppressed mesh is not live: the single live child is the box, so no mesh
        // operand remains and nothing is named.
        let d = doc(&format!(
            "group g {{\n  {mesh_line} suppressed\n  box b size=(1, 2, 3)\n  shell 2 inside\n}}\n"
        ));
        assert!(d.validate().is_empty(), "{:?}", d.validate());
        // A zero / non-finite parameter is not usable.
        let d = doc(&format!("group g {{\n  {mesh_line}\n  offset 0\n}}\n"));
        let errs = d.validate();
        assert!(errs.iter().any(|e| e.contains("not usable")), "{errs:?}");
        // A parameter far past the derived-grid budget is named with the mesh.
        let d = doc(&format!("group g {{\n  {mesh_line}\n  offset 10000\n}}\n"));
        let errs = d.validate();
        assert!(errs.iter().any(|e| e.contains("grid cells")), "{errs:?}");
        // A purely analytic group keeps the exact wrap — nothing named.
        let d = doc("group g {\n  box b size=(1, 2, 3)\n  shell 2 inside\n}\n");
        assert!(d.validate().is_empty(), "{:?}", d.validate());
    }

    #[test]
    fn all_examples_and_literate_fsm_files_parse() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples");
        let subdirs = ["", "analytic", "tutorials", "case-studies"];
        let mut count = 0usize;
        let mut errors = Vec::new();
        for sub in subdirs {
            let dir = if sub.is_empty() {
                root.clone()
            } else {
                root.join(sub)
            };
            let mut entries: Vec<_> = std::fs::read_dir(&dir)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("fsm"))
                .collect();
            entries.sort_by_key(|e| e.path());
            for entry in entries {
                let path = entry.path();
                let raw = std::fs::read_to_string(&path).unwrap();
                count += 1;
                if !sub.is_empty() {
                    match parse_literate_header(&raw) {
                        Some(hdr) => {
                            if hdr.id.is_empty() || hdr.title.is_empty() {
                                errors.push(format!(
                                    "{}: literate header missing id/title",
                                    path.display()
                                ));
                            }
                            if sub == "analytic" && hdr.checks.is_empty() {
                                errors.push(format!(
                                    "{}: analytic case has 0 @check entries",
                                    path.display()
                                ));
                            }
                            if sub == "analytic" && hdr.asserts.is_empty() {
                                errors.push(format!(
                                    "{}: analytic case has 0 @assert entries",
                                    path.display()
                                ));
                            }
                        }
                        None => {
                            errors.push(format!("{}: missing literate header", path.display()));
                        }
                    }
                }
                match parse(&raw) {
                    Ok(doc) => {
                        let v_errs = doc.validate();
                        if !v_errs.is_empty() {
                            errors.push(format!("{}: validate {:?}", path.display(), v_errs));
                        }
                    }
                    Err(e) => {
                        errors.push(format!("{}:{}: {}", path.display(), e.line, e.message));
                    }
                }
            }
        }
        assert!(
            errors.is_empty(),
            "Failed to parse/validate {} of {} .fsm files:\n{}",
            errors.len(),
            count,
            errors.join("\n")
        );
        assert!(
            count >= 36,
            "Expected at least 36 .fsm files, found {count}"
        );
    }

    #[test]
    fn unified_expressions_and_loaddist_lets() {
        let src = "\
fieldsmith 1
name \"Unified Expr Test\"
material pla
let a = 10 * cos(0)
let b = 2 ^ 3 + sin(pi / 2) + ln(exp(1)) + sqrt(16) + abs(-5) + min(2, 4) + max(1, 3) + pow(2, 3) + tan(0)
custommaterial custom_cu name=\"Cu\" e=110000 nu=0.34 yield=250 density=8.9e-6 wall=0.8 a=1.7e-5 k=320 source=\"test\"
box body size=(a, b, 10)
region top = body.pz
region bot = body.nz
support fix bot fixed=xyz
load press top fn=\"0 < x < a -> b * 0.1\" dir=(0, 0, -1)
";
        let doc = parse(src).expect("should parse unified expressions and load fn with lets");
        assert!((doc.lets[0].value - 10.0).abs() < 1e-12);
        // b = 8 + 1 + 1 + 4 + 5 + 2 + 3 + 8 + 0 = 32
        assert!((doc.lets[1].value - 32.0).abs() < 1e-12);
        assert_eq!(doc.custom_materials[0].k, Some(320.0));
        assert!(
            doc.validate().is_empty(),
            "validate errors: {:?}",
            doc.validate()
        );
        let printed = print(&doc);
        assert!(printed.contains("k=320"), "printed: {printed}");
        let reparsed = parse(&printed).expect("reparse");
        assert_eq!(reparsed.custom_materials[0].k, Some(320.0));
    }

    #[test]
    fn typed_constitutive_fluid_role_coupled_and_assert_round_trip() {
        let src = "\
fieldsmith 1
name \"Typed Constitutive & Coupled Spec\"
material sheet_al
coupled regen_nozzle_h2_v1
custommaterial sheet_al name=\"Al 5754\" e=70000 nu=0.33 yield=65 density=2.67 wall=0.4 a=0.000023 k=150 hardening_n=0.2 hardening_k=390 source=\"EN AW-5754\"
custommaterial bath_gas name=\"Forming Gas\" e=100 nu=0.3 yield=5 density=0.15 wall=0.8 role=\"fluid\" source=\"Non-structural headspace\"

box plate size=(30, 20, 2)
box gas_pocket size=(10, 10, 4) at=(0, 0, 3)
material bath_gas on gas_pocket

region clamp = plate.nx
region tip = plate.px
support s1 clamp
load f1 tip force=(0, 0, -50) case=\"Load 1\"
temperature t1 tip temp=120 case=\"Thermal 1\"
assert \"Tip deflection positive\" metric=\"max_displacement\" target=\">= 0.001\" tol=\">= 0.001\" unit=\"mm\"
solve tref=20 plasticity=\"j2\" hardening_n=0.2
";
        let doc = parse(src).expect("parse typed constitutive fsm");
        assert_eq!(doc.coupled_system.as_deref(), Some("regen_nozzle_h2_v1"));
        assert_eq!(doc.solve.plasticity, Some(PlasticityLaw::J2));
        assert_eq!(doc.solve.hardening_n, Some(0.2));
        assert_eq!(doc.custom_materials[0].hardening_n, Some(0.2));
        assert_eq!(doc.custom_materials[0].hardening_k, Some(390.0));
        assert_eq!(doc.custom_materials[1].role, MaterialRole::Fluid);
        // Note: bath_gas has role=fluid and omits a= (CTE), yet thermal validate passes!
        assert!(doc.validate().is_empty(), "validate: {:?}", doc.validate());

        let printed = print(&doc);
        let reparsed = parse(&printed).expect("reparse printed");
        assert_eq!(reparsed, doc);
        assert_eq!(print(&reparsed), printed);
    }

    #[test]
    fn constitutive_validation_rejects_invalid_bounds() {
        let mut doc = parse("fieldsmith 1\nname \"Bad\"\nmaterial bad_m\ncustommaterial bad_m e=70000 nu=0.3 yield=100 density=2.7 hardening_n=1.4 weibull_m=-3\nsolve hardening_n=0\n").unwrap();
        doc.coupled_system = Some("123 bad".into());
        let errs = doc.validate();
        assert!(
            errs.iter()
                .any(|e| e.contains("Custom material 'bad_m' hardening_n")),
            "{errs:?}"
        );
        assert!(
            errs.iter()
                .any(|e| e.contains("Custom material 'bad_m' weibull_m")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("solve hardening_n=")),
            "{errs:?}"
        );
        assert!(
            errs.iter().any(|e| e.contains("coupled system")),
            "{errs:?}"
        );
    }

    #[test]
    fn literate_header_asserts_and_mcp_survive_print_reparse() {
        let raw = "\
fieldsmith 1
# === FIELDSMITH LITERATE CASE ===
# @id: TEST1
# @title: Literate Preservation
# @coupled: regen_nozzle_h2_v1
# @assert: Beam Deflection || max_displacement || 0.45 || 2.0% || mm
# @mcp: Run Solve || solve || {\"kind\":\"solve\",\"dofs\":5000}
# === END LITERATE HEADER ===
name \"Literate Test\"
material pla
box b size=(20, 10, 10)
";
        let hdr = parse_literate_header(raw).expect("literate header");
        assert_eq!(hdr.mcp_steps.len(), 1);
        assert_eq!(hdr.mcp_steps[0].tool, "solve");
        let doc = parse(raw).expect("parse literate fsm");
        assert_eq!(doc.coupled_system.as_deref(), Some("regen_nozzle_h2_v1"));
        assert_eq!(doc.asserts.len(), 1);
        let printed = print(&doc);
        let reparsed = parse(&printed).expect("reparse printed");
        assert_eq!(reparsed.asserts, doc.asserts);
        assert_eq!(reparsed.coupled_system, doc.coupled_system);
    }

    #[test]
    fn spatial_boundary_condition_fields_round_trip_and_validate() {
        let raw = "\
fieldsmith 1
name \"Spatial BCs\"
material cucrzr
let q_peak = 1.4
cylinder bore r=8 h=60 at=(0, 0, 30)
region wall = bore.side
region cap = bore.top
region bot = bore.bottom
support fix bot
load p_var wall pressure=0.5 pressure_fn=\"lerp(z, 0, 0.8, 30, 0.4, 60, 0.2)\" case=\"Press\"
temperature t_var cap temp=40 temp_fn=\"40 + 0.5 * r\" case=\"Hot\"
convection c_var wall h=0.003 h_fn=\"lerp(z, 0, 0.002, 30, 0.006, 60, 0.0025)\" tinf=25 tinf_fn=\"20 + 0.15 * z\" case=\"Hot\"
heatflux q_var wall q=0.8 q_fn=\"0.2 + q_peak * exp(-((z - 30) / 10)^2)\" case=\"Hot\"
";
        let doc = parse(raw).expect("parse spatial BCs");
        assert!(doc.validate().is_empty(), "{:?}", doc.validate());
        let printed = print(&doc);
        let reparsed = parse(&printed).expect("reparse spatial BCs");
        assert_eq!(reparsed, doc);
    }

    #[test]
    fn uncertainty_declarations_round_trip_and_validate() {
        let raw = "\
fieldsmith 1
name \"Epistemic Cantilever\"
material ss316l
let d = 10
box beam size=(80, d, 12) at=(40, 0, 0)
region root = beam.nx
region tip = beam.px
support fix root
load pull tip force=(0, -500, 0)
uncertainty \"Machining tolerance\" param=d std=0.15 unit=\"mm\"
uncertainty \"Tip load jitter\" param=\"pull.force.1\" cv=0.08 unit=\"N\"
assert \"Robust peak stress\" metric=\"p90.max_vm_elem\" target=\"<= 160\" unit=\"MPa\"
";
        let doc = parse(raw).expect("parse uncertainties");
        assert_eq!(doc.uncertainties.len(), 2);
        assert_eq!(doc.uncertainties[0].effective_sigma(10.0), 0.15);
        assert!((doc.uncertainties[1].effective_sigma(-500.0) - 40.0).abs() < 1e-12);
        assert!(doc.validate().is_empty(), "{:?}", doc.validate());
        let printed = print(&doc);
        let reparsed = parse(&printed).expect("reparse uncertainties");
        assert_eq!(reparsed, doc);
    }

    #[test]
    fn transient_solve_and_custom_material_cp_round_trip() {
        let raw = "\
fieldsmith 1
name \"Transient Quench Slab\"
custommaterial sic_test name=\"SiC Test\" e=410000 nu=0.14 yield=350 density=3.1 wall=1 a=0.000004 k=120 cp=675
material sic_test
solve tref=320 t_end=8.5 steps=17 t_init=320
box slab size=(10, 10, 10) at=(5, 5, 5)
region sym_x = slab.nx
region sym_y = slab.ny
region sym_z = slab.nz
region quench = slab.pz
support sx sym_x fixed=x
support sy sym_y fixed=y
support sz sym_z fixed=z
convection q quench h=0.012 tinf=20
";
        let doc = parse(raw).expect("parse transient solve");
        assert_eq!(doc.solve.t_end, Some(8.5));
        assert_eq!(doc.solve.steps, Some(17));
        assert_eq!(doc.solve.t_init, Some(320.0));
        assert_eq!(doc.custom_materials[0].cp, Some(675.0));
        let alpha_th = doc.custom_materials[0].effective_k_w_mm_k()
            / doc.custom_materials[0].volumetric_heat_capacity_j_mm3_k();
        // 0.120 / (3.1e-6 * 675) = 57.347670... mm^2/s
        assert!((alpha_th - 0.120 / (3.1e-6 * 675.0)).abs() < 1e-10);
        assert!(doc.validate().is_empty(), "{:?}", doc.validate());
        let printed = print(&doc);
        let reparsed = parse(&printed).expect("reparse transient solve");
        assert_eq!(reparsed, doc);

        let mut bad = doc.clone();
        bad.solve.steps = Some(0);
        assert!(!bad.validate().is_empty());
        let mut bad2 = doc.clone();
        bad2.solve.t_end = Some(-1.0);
        assert!(!bad2.validate().is_empty());
    }
}
