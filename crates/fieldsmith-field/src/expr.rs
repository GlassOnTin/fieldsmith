//! The field expression tree compiled from a document, and its CPU evaluator.

use crate::dual::*;
use crate::prim::*;
use fieldsmith_doc::{Axis, Document, Face, Modifier, Node, NodeKind, Op, ShellSide};

/// Rigid transform `world = r · local + t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rigid {
    pub r: M3,
    pub t: V3,
}

impl Rigid {
    pub fn from_doc(t: &fieldsmith_doc::Transform) -> Self {
        Self {
            r: rotation_xyz_deg(t.rotate),
            t: t.translate,
        }
    }
    #[inline]
    pub fn to_local(&self, p: V3) -> V3 {
        mat_t_vec(&self.r, sub(p, self.t))
    }
    #[inline]
    pub fn to_world(&self, p: V3) -> V3 {
        add(mat_vec(&self.r, p), self.t)
    }
    #[inline]
    pub fn grad_to_world(&self, g: V3) -> V3 {
        mat_vec(&self.r, g)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Empty,
    Prim(Prim),
    Xform {
        m: Rigid,
        e: Box<Expr>,
    },
    Combine {
        op: Op,
        blend: f64,
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Shell {
        side: ShellSide,
        t: f64,
        e: Box<Expr>,
    },
    Offset {
        d: f64,
        e: Box<Expr>,
    },
    Mirror {
        axis: usize,
        e: Box<Expr>,
    },
    Linear {
        count: u32,
        step: V3,
        e: Box<Expr>,
    },
    Circular {
        count: u32,
        axis: usize,
        step_deg: f64,
        e: Box<Expr>,
    },
    /// Lattice-filled part: `max(body, min(−(body + skin), lattice, keep))`.
    Filled(Box<crate::lattice::FillExpr>),
}

pub fn circular_step(count: u32, angle: f64) -> f64 {
    if count <= 1 {
        0.0
    } else if (angle.abs() - 360.0).abs() < 1e-9 {
        angle / count as f64
    } else {
        angle / (count - 1) as f64
    }
}

/// Compile one node (with its own transform) to an expression.
pub fn compile_node(node: &Node) -> Expr {
    let local = match &node.kind {
        NodeKind::Box { size, round } => {
            let half = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
            let round = round.clamp(0.0, half[0].min(half[1]).min(half[2]));
            Expr::Prim(Prim::Box { half, round })
        }
        NodeKind::Cylinder {
            radius,
            height,
            round,
        } => {
            let round = round.clamp(0.0, radius.min(height * 0.5));
            Expr::Prim(Prim::Cylinder {
                radius: *radius,
                half_h: height * 0.5,
                round,
            })
        }
        NodeKind::Sphere { radius } => Expr::Prim(Prim::Sphere { radius: *radius }),
        NodeKind::Capsule { radius, length } => Expr::Prim(Prim::Capsule {
            radius: *radius,
            half_len: length * 0.5,
        }),
        NodeKind::Torus { major, minor } => Expr::Prim(Prim::Torus {
            major: *major,
            minor: *minor,
        }),
        NodeKind::Extrude { profile, height } => {
            if profile.len() < 3 {
                Expr::Empty
            } else {
                let segs = (0..profile.len())
                    .map(|i| {
                        let a = profile[i];
                        let b = profile[(i + 1) % profile.len()];
                        ArcSeg {
                            a: [a.x, a.y],
                            b: [b.x, b.y],
                            bulge: a.bulge,
                        }
                    })
                    .filter(|s| s.a != s.b)
                    .collect();
                Expr::Prim(Prim::Extrude {
                    segs,
                    half_h: height * 0.5,
                })
            }
        }
        NodeKind::Group {
            children,
            modifiers,
        } => {
            let mut e = compile_list(children);
            for m in modifiers {
                e = apply_modifier(e, m);
            }
            e
        }
    };
    if matches!(local, Expr::Empty) || node.transform.is_identity() {
        local
    } else {
        Expr::Xform {
            m: Rigid::from_doc(&node.transform),
            e: Box::new(local),
        }
    }
}

fn apply_modifier(e: Expr, m: &Modifier) -> Expr {
    if matches!(e, Expr::Empty) {
        return e;
    }
    match m {
        Modifier::Shell { thickness, side } => Expr::Shell {
            side: *side,
            t: *thickness,
            e: Box::new(e),
        },
        Modifier::Offset { distance } => Expr::Offset {
            d: *distance,
            e: Box::new(e),
        },
        Modifier::Mirror { axis } => Expr::Mirror {
            axis: axis.index(),
            e: Box::new(e),
        },
        Modifier::Linear { count, step } => Expr::Linear {
            count: (*count).max(1),
            step: *step,
            e: Box::new(e),
        },
        Modifier::Circular { count, axis, angle } => Expr::Circular {
            count: (*count).max(1),
            axis: axis.index(),
            step_deg: circular_step(*count, *angle),
            e: Box::new(e),
        },
    }
}

/// Fold an ordered list of nodes with their Add / Subtract / Intersect ops.
pub fn compile_list(nodes: &[Node]) -> Expr {
    let mut acc = Expr::Empty;
    for n in nodes.iter().filter(|n| !n.suppressed) {
        let e = compile_node(n);
        if matches!(e, Expr::Empty) {
            if n.op == Op::Intersect {
                acc = Expr::Empty;
            }
            continue;
        }
        acc = match (acc, n.op) {
            (Expr::Empty, Op::Add) => e,
            (Expr::Empty, _) => Expr::Empty,
            (a, op) => Expr::Combine {
                op,
                blend: n.blend.max(0.0),
                a: Box::new(a),
                b: Box::new(e),
            },
        };
    }
    acc
}

/// The part's solid field.
pub fn compile_body(doc: &Document) -> Expr {
    compile_list(&doc.root)
}

#[inline]
fn reflect(p: V3, axis: usize) -> V3 {
    let mut q = p;
    q[axis] = -q[axis];
    q
}

pub fn eval(e: &Expr, p: V3) -> D {
    match e {
        Expr::Empty => D::EMPTY,
        Expr::Prim(prim) => eval_prim(prim, p),
        Expr::Xform { m, e } => {
            let d = eval(e, m.to_local(p));
            D::new(d.v, m.grad_to_world(d.g))
        }
        Expr::Combine { op, blend, a, b } => {
            let da = eval(a, p);
            let db = eval(b, p);
            if *blend <= 0.0 {
                return match op {
                    Op::Add => dmin(da, db),
                    Op::Subtract => dmax(da, -db),
                    Op::Intersect => dmax(da, db),
                };
            }
            // Every blended op is a round union of (possibly negated) operands.
            let (x, y, neg) = match op {
                Op::Add => (da, db, false),
                Op::Subtract => (-da, db, true),
                Op::Intersect => (-da, -db, true),
            };
            let (sx, sy) = match op {
                Op::Add => (1.0, 1.0),
                Op::Subtract => (-1.0, 1.0),
                Op::Intersect => (-1.0, -1.0),
            };
            let mut k = blend_radius(x, y, *blend);
            if k > 0.0 && x.v < k && y.v < k {
                let (tx, ty, cos) = blend_contacts(p, x, y, k);
                k *= blend_gate(sx * eval(a, tx).v, sy * eval(b, ty).v, x.v, y.v, k, cos);
            }
            let r = round_min(x, y, k);
            if neg {
                -r
            } else {
                r
            }
        }
        Expr::Shell { side, t, e } => {
            let d = eval(e, p);
            match side {
                ShellSide::Inside => dmax(d, D::new(-(d.v + t), scale(d.g, -1.0))),
                ShellSide::Outside => dmax(D::new(d.v - t, d.g), -d),
                ShellSide::Centre => {
                    let s = if d.v < 0.0 { -1.0 } else { 1.0 };
                    D::new(d.v.abs() - 0.5 * t, scale(d.g, s))
                }
            }
        }
        Expr::Offset { d: o, e } => {
            let d = eval(e, p);
            D::new(d.v - o, d.g)
        }
        Expr::Mirror { axis, e } => {
            let a = eval(e, p);
            let b = eval(e, reflect(p, *axis));
            dmin(a, D::new(b.v, reflect(b.g, *axis)))
        }
        Expr::Linear { count, step, e } => {
            let mut best = D::EMPTY;
            for i in 0..*count {
                let o = scale(*step, i as f64);
                best = dmin(best, eval(e, sub(p, o)));
            }
            best
        }
        Expr::Circular {
            count,
            axis,
            step_deg,
            e,
        } => {
            let mut best = D::EMPTY;
            for i in 0..*count {
                let r = rotation_axis_deg(*axis, step_deg * i as f64);
                let d = eval(e, mat_t_vec(&r, p));
                best = dmin(best, D::new(d.v, mat_vec(&r, d.g)));
            }
            best
        }
        Expr::Filled(f) => f.eval(p),
    }
}

// ---------------------------------------------------------------------------------------------
// Bounds

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub lo: V3,
    pub hi: V3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        lo: [f64::INFINITY; 3],
        hi: [f64::NEG_INFINITY; 3],
    };
    pub fn is_empty(&self) -> bool {
        (0..3).any(|i| self.lo[i] > self.hi[i])
    }
    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb {
            lo: [
                self.lo[0].min(o.lo[0]),
                self.lo[1].min(o.lo[1]),
                self.lo[2].min(o.lo[2]),
            ],
            hi: [
                self.hi[0].max(o.hi[0]),
                self.hi[1].max(o.hi[1]),
                self.hi[2].max(o.hi[2]),
            ],
        }
    }
    pub fn intersect(&self, o: &Aabb) -> Aabb {
        Aabb {
            lo: [
                self.lo[0].max(o.lo[0]),
                self.lo[1].max(o.lo[1]),
                self.lo[2].max(o.lo[2]),
            ],
            hi: [
                self.hi[0].min(o.hi[0]),
                self.hi[1].min(o.hi[1]),
                self.hi[2].min(o.hi[2]),
            ],
        }
    }
    pub fn expand(&self, m: f64) -> Aabb {
        if self.is_empty() {
            return *self;
        }
        Aabb {
            lo: sub(self.lo, [m; 3]),
            hi: add(self.hi, [m; 3]),
        }
    }
    pub fn corners(&self) -> [V3; 8] {
        let mut c = [[0.0; 3]; 8];
        for (i, ci) in c.iter_mut().enumerate() {
            *ci = [
                if i & 1 == 0 { self.lo[0] } else { self.hi[0] },
                if i & 2 == 0 { self.lo[1] } else { self.hi[1] },
                if i & 4 == 0 { self.lo[2] } else { self.hi[2] },
            ];
        }
        c
    }
    pub fn from_points(pts: &[V3]) -> Aabb {
        let mut b = Aabb::EMPTY;
        for p in pts {
            b = b.union(&Aabb { lo: *p, hi: *p });
        }
        b
    }
    pub fn size(&self) -> V3 {
        sub(self.hi, self.lo)
    }
    pub fn centre(&self) -> V3 {
        scale(add(self.lo, self.hi), 0.5)
    }
}

fn map_box(b: &Aabb, f: impl Fn(V3) -> V3) -> Aabb {
    if b.is_empty() {
        return *b;
    }
    let pts: Vec<V3> = b.corners().iter().map(|&c| f(c)).collect();
    Aabb::from_points(&pts)
}

pub fn bounds(e: &Expr) -> Aabb {
    match e {
        Expr::Empty => Aabb::EMPTY,
        Expr::Prim(p) => {
            let (lo, hi) = prim_bounds(p);
            Aabb { lo, hi }
        }
        Expr::Xform { m, e } => map_box(&bounds(e), |c| m.to_world(c)),
        Expr::Combine { op, blend, a, b } => {
            let ba = bounds(a);
            match op {
                Op::Add => ba.union(&bounds(b)).expand(blend * 0.25),
                Op::Subtract => ba,
                Op::Intersect => ba.intersect(&bounds(b)),
            }
        }
        Expr::Shell { side, t, e } => match side {
            ShellSide::Inside => bounds(e),
            ShellSide::Outside => bounds(e).expand(*t),
            ShellSide::Centre => bounds(e).expand(t * 0.5),
        },
        Expr::Offset { d, e } => bounds(e).expand(d.max(0.0)),
        Expr::Mirror { axis, e } => {
            let b = bounds(e);
            b.union(&map_box(&b, |c| reflect(c, *axis)))
        }
        Expr::Linear { count, step, e } => {
            let b = bounds(e);
            let last = scale(*step, (*count - 1) as f64);
            b.union(&map_box(&b, |c| add(c, last)))
        }
        Expr::Circular {
            count,
            axis,
            step_deg,
            e,
        } => {
            let b = bounds(e);
            let mut acc = b;
            for i in 1..*count {
                let r = rotation_axis_deg(*axis, step_deg * i as f64);
                acc = acc.union(&map_box(&b, |c| mat_vec(&r, c)));
            }
            acc
        }
        Expr::Filled(f) => bounds(&f.body),
    }
}

// ---------------------------------------------------------------------------------------------
// Shapes as placed in the world (for regions, picking and protected zones)

/// Expression for one node *as placed in the world* — its own field wrapped in every ancestor's
/// transform and replicating modifier (mirror / patterns), but not combined with siblings.
pub fn placed_shape(doc: &Document, id: &str) -> Option<Expr> {
    fn rec(nodes: &[Node], id: &str) -> Option<Expr> {
        for n in nodes {
            if n.id == id {
                return Some(compile_node(&Node {
                    suppressed: false,
                    ..n.clone()
                }));
            }
            if let NodeKind::Group {
                children,
                modifiers,
            } = &n.kind
            {
                if let Some(mut inner) = rec(children, id) {
                    for m in modifiers {
                        if matches!(
                            m,
                            Modifier::Mirror { .. }
                                | Modifier::Linear { .. }
                                | Modifier::Circular { .. }
                        ) {
                            inner = apply_modifier(inner, m);
                        }
                    }
                    if !n.transform.is_identity() {
                        inner = Expr::Xform {
                            m: Rigid::from_doc(&n.transform),
                            e: Box::new(inner),
                        };
                    }
                    return Some(inner);
                }
            }
        }
        None
    }
    rec(&doc.root, id)
}

/// Evaluate a placed shape and report which of its primitive faces is nearest.
/// Returns `(distance, face)`; the face is `All` for groups and face-less primitives.
pub fn eval_face(e: &Expr, p: V3) -> (f64, Face) {
    match e {
        Expr::Prim(prim) => (eval_prim(prim, p).v, classify_face(prim, p)),
        Expr::Xform { m, e } => eval_face(e, m.to_local(p)),
        Expr::Mirror { axis, e } => {
            let a = eval_face(e, p);
            let b = eval_face(e, reflect(p, *axis));
            if a.0.abs() <= b.0.abs() {
                a
            } else {
                b
            }
        }
        Expr::Linear { count, step, e } => (0..*count)
            .map(|i| eval_face(e, sub(p, scale(*step, i as f64))))
            .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
            .unwrap_or((1e9, Face::All)),
        Expr::Circular {
            count,
            axis,
            step_deg,
            e,
        } => (0..*count)
            .map(|i| {
                eval_face(
                    e,
                    mat_t_vec(&rotation_axis_deg(*axis, step_deg * i as f64), p),
                )
            })
            .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
            .unwrap_or((1e9, Face::All)),
        other => (eval(other, p).v, Face::All),
    }
}

/// The local primitive of a leaf node (for listing selectable faces).
pub fn node_prim(node: &Node) -> Option<Prim> {
    match compile_node(&Node {
        transform: Default::default(),
        ..node.clone()
    }) {
        Expr::Prim(p) => Some(p),
        _ => None,
    }
}

/// Leaf (non-group) node ids, in tree order, that are not suppressed (including ancestors).
pub fn active_leaves(doc: &Document) -> Vec<String> {
    fn rec(nodes: &[Node], out: &mut Vec<String>) {
        for n in nodes.iter().filter(|n| !n.suppressed) {
            match &n.kind {
                NodeKind::Group { children, .. } => rec(children, out),
                _ => out.push(n.id.clone()),
            }
        }
    }
    let mut out = Vec::new();
    rec(&doc.root, &mut out);
    out
}

/// Identify the shape surface a world point lies on: the active leaf whose placed surface
/// is nearest. Returns `(shape id, face, |distance|)`.
pub fn identify_surface(doc: &Document, p: V3) -> Option<(String, Face, f64)> {
    active_leaves(doc)
        .into_iter()
        .filter_map(|id| {
            let e = placed_shape(doc, &id)?;
            let (d, f) = eval_face(&e, p);
            Some((id, f, d.abs()))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// Axis helper for callers that hold a doc `Axis`.
pub fn axis_index(a: Axis) -> usize {
    a.index()
}

/// March a ray against the body. Returns the hit distance along the (normalised) direction.
pub fn raymarch(e: &Expr, origin: V3, dir: V3, max_t: f64) -> Option<f64> {
    let dir = normalize(dir);
    let b = bounds(e);
    if b.is_empty() {
        return None;
    }
    // Clip to the bounding box.
    let mut t0: f64 = 0.0;
    let mut t1 = max_t;
    for i in 0..3 {
        if dir[i].abs() < 1e-12 {
            if origin[i] < b.lo[i] - 1e-6 || origin[i] > b.hi[i] + 1e-6 {
                return None;
            }
        } else {
            let a = (b.lo[i] - 1e-3 - origin[i]) / dir[i];
            let c = (b.hi[i] + 1e-3 - origin[i]) / dir[i];
            t0 = t0.max(a.min(c));
            t1 = t1.min(a.max(c));
        }
    }
    if t0 > t1 {
        return None;
    }
    let mut t = t0;
    for _ in 0..512 {
        let p = add(origin, scale(dir, t));
        let d = eval(e, p).v;
        if d < 1e-4 {
            return Some(t);
        }
        t += d * 0.9;
        if t > t1 {
            return None;
        }
    }
    None
}
