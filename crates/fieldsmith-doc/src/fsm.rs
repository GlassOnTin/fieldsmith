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

use crate::materials::Material;
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
        } else if "(){}[],=.+".contains(c) {
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
        match self.next() {
            Some(Tok::Num(v)) => Ok(Val::Num(v)),
            Some(Tok::Str(s)) => Ok(Val::Str(s)),
            Some(Tok::Ident(s)) => Ok(Val::Ident(s)),
            Some(Tok::Sym('(')) => Ok(Val::Tuple(self.seq(')')?)),
            Some(Tok::Sym('[')) => Ok(Val::List(self.seq(']')?)),
            _ => self.err("expected a value"),
        }
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
    /// Remaining tokens as `key=value` pairs or bare flags.
    fn attrs(&mut self) -> Result<Attrs, ParseError> {
        let mut a = Attrs {
            items: Vec::new(),
            line: self.line,
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
    fn req_string(&mut self, key: &str) -> Result<String, ParseError> {
        match self.string(key)? {
            Some(s) => Ok(s),
            None => self.err(format!("missing '{key}='")),
        }
    }
    fn vec3(&mut self, key: &str) -> Result<Option<[f64; 3]>, ParseError> {
        match self.take(key) {
            None => Ok(None),
            Some(Some(Val::Tuple(t))) if t.len() == 3 => {
                let mut o = [0.0; 3];
                for (i, v) in t.iter().enumerate() {
                    match v {
                        Val::Num(x) => o[i] = *x,
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
    // Stack of open groups: (header node, children, modifiers).
    let mut stack: Vec<(Node, Vec<Node>, Vec<Modifier>)> = Vec::new();
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
            let (mut node, children, modifiers) =
                stack.pop().ok_or_else(|| err("unmatched '}'".into()))?;
            node.kind = NodeKind::Group {
                children,
                modifiers,
            };
            push_node(&mut doc, &mut stack, node);
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
            "box" | "cylinder" | "sphere" | "capsule" | "torus" | "extrude" | "mesh" | "group"
            | "extend" => {
                let id = c.ident("a shape name")?;
                // `extend` takes a positional region name before its (required) dir/len.
                let region = if kw == "extend" {
                    Some(c.ident("a region name")?)
                } else {
                    None
                };
                let mut a = c.attrs()?;
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
                let transform = Transform {
                    translate: a.vec3("at")?.unwrap_or([0.0; 3]),
                    rotate: a.vec3("rotate")?.unwrap_or([0.0; 3]),
                };
                let kind = match kw.as_str() {
                    "box" => NodeKind::Box {
                        size: a
                            .vec3("size")?
                            .ok_or_else(|| err("box needs size=(x, y, z)".into()))?,
                        round: a.num_or("round", 0.0)?,
                    },
                    "cylinder" => NodeKind::Cylinder {
                        radius: a.req_num("r")?,
                        height: a.req_num("h")?,
                        round: a.num_or("round", 0.0)?,
                    },
                    "sphere" => NodeKind::Sphere {
                        radius: a.req_num("r")?,
                    },
                    "capsule" => NodeKind::Capsule {
                        radius: a.req_num("r")?,
                        length: a.req_num("length")?,
                    },
                    "torus" => NodeKind::Torus {
                        major: a.req_num("major")?,
                        minor: a.req_num("minor")?,
                    },
                    "extrude" => {
                        let height = a.req_num("h")?;
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
                let region = c.ident("a region name")?;
                let mut a = c.attrs()?;
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
                let region = c.ident("a region name")?;
                let mut a = c.attrs()?;
                let case = a.string("case")?.unwrap_or_else(|| "Load 1".into());
                let kind = if let Some(v) = a.vec3("force")? {
                    LoadKind::Force { vector: v }
                } else if let Some(p) = a.num("pressure")? {
                    LoadKind::Pressure { mpa: p }
                } else if let Some(src) = a.string("fn")? {
                    // Compiled here, so a bad function names its character, not a
                    // distant validate error.
                    if let Err(e) = crate::loaddist::LoadFn::compile(&src) {
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
                    let total = a.num("total")?;
                    LoadKind::Distributed {
                        expr: src,
                        dir,
                        total,
                    }
                } else {
                    return Err(err(
                        "load needs force=(x, y, z), pressure=value, or fn=...".into()
                    ));
                };
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
                let region = c.ident("a region name")?;
                let mut a = c.attrs()?;
                let celsius = a.req_num("temp")?;
                let case = a.string("case")?.unwrap_or_else(|| "Thermal 1".into());
                a.finish()?;
                doc.temperatures.push(Temperature {
                    id,
                    region,
                    celsius,
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
                let mut a = c.attrs()?;
                let d = Fill::default();
                let (rho_min, rho_max) = a.pair("rho")?.unwrap_or((d.rho_min, d.rho_max));
                let fill = Fill {
                    family,
                    mode,
                    cell: a.num_or("cell", d.cell)?,
                    skin: a.num_or("skin", d.skin)?,
                    rho_min,
                    rho_max,
                    curve: a.num_or("curve", d.curve)?,
                    min_wall: a.num_or("wall", d.min_wall)?,
                    protect: a.idents("protect")?,
                    margin: a.num_or("margin", d.margin)?,
                    case: a.string("case")?,
                };
                a.finish()?;
                doc.fill = Some(fill);
            }
            "solve" => {
                let mut a = c.attrs()?;
                doc.solve.size = match a.take("size") {
                    None | Some(Some(Val::Ident(_))) => None,
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
                a.finish()?;
            }
            "custommaterial" => {
                let id = c.ident("a material id")?;
                let mut a = c.attrs()?;
                let mat = Material {
                    name: a.string("name")?.unwrap_or_else(|| id.clone()),
                    id,
                    e: a.req_num("e")?,
                    nu: a.req_num("nu")?,
                    yield_strength: a.req_num("yield")?,
                    density: a.req_num("density")?,
                    min_wall: a.num_or("wall", 0.8)?,
                    // Thermal expansion 1/°C (e.g. 2e-5 for an aluminium class); optional,
                    // and a thermal document refuses a material without one.
                    alpha: a.num("a")?,
                    source: a
                        .string("source")?
                        .unwrap_or_else(|| "User supplied".into()),
                };
                a.finish()?;
                doc.custom_materials.push(mat);
            }
            other => return Err(err(format!("unknown keyword '{other}'"))),
        }
        if !c.done() && !matches!(kw.as_str(), "group") {
            return Err(err("unexpected text at end of line".into()));
        }
    }
    if !saw_header {
        return Err(ParseError {
            line: 1,
            message: "empty document".into(),
        });
    }
    if !stack.is_empty() {
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

fn parse_axis(c: &mut Cursor) -> Result<Axis, ParseError> {
    match c.ident("x, y or z")?.as_str() {
        "x" => Ok(Axis::X),
        "y" => Ok(Axis::Y),
        "z" => Ok(Axis::Z),
        _ => c.err("expected x, y or z"),
    }
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
            let count = num(c)? as u32;
            let mut a = c.attrs()?;
            let step = a.vec3("step")?.ok_or_else(|| ParseError {
                line: c.line,
                message: "linear needs step=(x, y, z)".into(),
            })?;
            a.finish()?;
            Modifier::Linear { count, step }
        }
        "circular" => {
            let count = num(c)? as u32;
            let mut a = c.attrs()?;
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

/// Print a document in canonical `.fsm` form.
pub fn print(doc: &Document) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "fieldsmith {FORMAT_VERSION}");
    let _ = writeln!(o, "name {}", q(&doc.name));
    let _ = writeln!(o, "material {}", doc.material);
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
        line += &format!(" source={}", q(&m.source));
        let _ = writeln!(o, "{line}");
    }
    if !doc.root.is_empty() {
        o.push('\n');
    }
    for node in &doc.root {
        print_node(&mut o, node, 0);
    }
    if !doc.regions.is_empty()
        || !doc.supports.is_empty()
        || !doc.loads.is_empty()
        || !doc.temperatures.is_empty()
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
            LoadKind::Force { vector } => format!("force={}", v3(*vector)),
            LoadKind::Pressure { mpa } => format!("pressure={}", n(*mpa)),
            LoadKind::Distributed { expr, dir, total } => {
                let mut k = format!("fn={}", q(expr));
                match dir {
                    LoadDir::Global { v } => k += &format!(" dir={}", v3(*v)),
                    LoadDir::Local { v } => k += &format!(" local={}", v3(*v)),
                }
                if let Some(t) = total {
                    k += &format!(" total={}", n(*t));
                }
                k
            }
        };
        let _ = writeln!(o, "load {} {} {} case={}", l.id, l.region, kind, q(&l.case));
    }
    for t in &doc.temperatures {
        let _ = writeln!(
            o,
            "temperature {} {} temp={} case={}",
            t.id,
            t.region,
            n(t.celsius),
            q(&t.case)
        );
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
    if solve_line.len() > "solve".len() {
        let _ = writeln!(o, "{solve_line}");
    }
    o
}

fn print_node(o: &mut String, node: &Node, depth: usize) {
    let ind = "  ".repeat(depth);
    let mut line = String::new();
    let _ = write!(line, "{ind}");
    match &node.kind {
        NodeKind::Box { size, round } => {
            let _ = write!(line, "box {} size={}", node.id, v3(*size));
            if *round != 0.0 {
                let _ = write!(line, " round={}", n(*round));
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
                n(*radius),
                n(*height)
            );
            if *round != 0.0 {
                let _ = write!(line, " round={}", n(*round));
            }
        }
        NodeKind::Sphere { radius } => {
            let _ = write!(line, "sphere {} r={}", node.id, n(*radius));
        }
        NodeKind::Capsule { radius, length } => {
            let _ = write!(
                line,
                "capsule {} r={} length={}",
                node.id,
                n(*radius),
                n(*length)
            );
        }
        NodeKind::Torus { major, minor } => {
            let _ = write!(
                line,
                "torus {} major={} minor={}",
                node.id,
                n(*major),
                n(*minor)
            );
        }
        NodeKind::Extrude { profile, height } => {
            let pts: Vec<String> = profile
                .iter()
                .map(|p| {
                    if p.bulge != 0.0 {
                        format!("({}, {}, {})", n(p.x), n(p.y), n(p.bulge))
                    } else {
                        format!("({}, {})", n(p.x), n(p.y))
                    }
                })
                .collect();
            let _ = write!(
                line,
                "extrude {} h={} profile=[{}]",
                node.id,
                n(*height),
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
    if node.transform.translate != [0.0; 3] {
        let _ = write!(line, " at={}", v3(node.transform.translate));
    }
    if node.transform.rotate != [0.0; 3] {
        let _ = write!(line, " rotate={}", v3(node.transform.rotate));
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
            print_node(o, c, depth + 1);
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
}
