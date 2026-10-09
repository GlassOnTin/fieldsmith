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
    /// Direction (not position) mapped from world into the node's local frame — the inverse
    /// of [`Rigid::grad_to_world`]. A rotation's transpose is its own inverse, so this is the
    /// plain transpose apply; the translation never touches directions.
    #[inline]
    pub fn to_local_dir(&self, g: V3) -> V3 {
        mat_t_vec(&self.r, g)
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
    /// Baked voxel field from an imported mesh (`fieldsmith-field::meshfield`). `key` names
    /// the session registry entry; unhydrated keys evaluate as vacuum. `bbox` is the baked
    /// world bounds (bounds() needs no registry) and `spec` the grid placement taken at
    /// compile time for shader generation. `face_hint` carries the axis of a geometric
    /// face selector naming this leaf (`Face::axis`, mapped into the leaf's own frame) so
    /// `eval_face` can label the derived surface; `None` for an import, an extend or any
    /// other leaf whose surface carries no face claim.
    Mesh {
        key: u64,
        bbox: crate::expr::Aabb,
        spec: Option<crate::meshfield::GridSpec>,
        face_hint: Option<usize>,
    },
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

/// Compile one node (with its own transform) to an expression. The document threads through
/// so region-driven extensions can resolve their named region's patch selectors; every leaf
/// else compiles from its node alone.
pub fn compile_node(doc: &Document, node: &Node) -> Expr {
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
            let mut e = compile_list(doc, children);
            for m in modifiers {
                e = apply_modifier(e, m, &node.id);
            }
            e
        }
        NodeKind::Mesh { hash, bbox, .. } => {
            let key = crate::meshfield::key_from_hex(hash);
            Expr::Mesh {
                key,
                bbox: Aabb {
                    lo: [bbox[0], bbox[1], bbox[2]],
                    hi: [bbox[3], bbox[4], bbox[5]],
                },
                spec: crate::meshfield::lookup(key).map(|f| f.spec()),
                face_hint: None,
            }
        }
        NodeKind::Extend { region, dir, len } => compile_extend(doc, region, *dir, *len),
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

/// Compile a region-driven extension into synthetic mesh fields, one per distinct parent
/// mesh the region's patch selectors name, unioned. Extends degrade gracefully rather than
/// erroring compile-side: a region that names no mesh patch, an unhydrated parent (vacuum
/// placeholder over the swept hull of the node's bounds, the same grace an unhydrated mesh
/// gets) or a sweep that claims no cells all compile to `Empty` / a vacuum `Expr::Mesh`,
/// with `validate` naming the real conditions before anything solves.
/// One parent mesh node feeding an extension: (hash, patch-id set, doc bbox, node id,
/// node placement).
/// What a swept parent's selectors name: a set of patch ids, or a face direction. The face
/// axis is held in the node's LOCAL frame — the frame the hydrated grid (and its SDF
/// gradient) lives in — mapped from the document frame at selector-collection time.
#[derive(Debug)]
enum SweptFrom {
    Patches(Vec<u16>),
    /// Unit face-direction axis, node-local.
    Face([f64; 3]),
}

type SweptParent<'a> = (
    &'a str,
    SweptFrom,
    [f64; 6],
    &'a str,
    &'a fieldsmith_doc::Transform,
);

fn compile_extend(doc: &Document, region: &str, dir: [f64; 3], len: f64) -> Expr {
    let Some(r) = doc.regions.iter().find(|r| r.id == region) else {
        return Expr::Empty;
    };
    // Distinct (parent node, swept source) pairs in selector order, grouped per mesh NODE
    // rather than per hash (two nodes sharing a hash sit at different placements, and each
    // keeps its own swept arm). Patch selectors on one node union their ids; face selectors
    // group by axis. A selector contributes only when it names a mesh NODE directly —
    // modifiers' copies of a mesh do not carry their own patch grid.
    let mut parents: Vec<SweptParent> = Vec::new();
    for s in &r.selectors {
        let Some(node) = doc.find(&s.shape) else {
            continue;
        };
        let NodeKind::Mesh { hash, bbox, .. } = &node.kind else {
            continue;
        };
        if let Some(p) = s.patch {
            if p == 0 || p >= u16::MAX as u32 {
                continue;
            }
            match parents.iter_mut().find(|(_, from, _, id, _)| {
                *id == node.id.as_str() && matches!(from, SweptFrom::Patches(_))
            }) {
                Some((_, SweptFrom::Patches(ids), _, _, _)) => {
                    if !ids.contains(&(p as u16)) {
                        ids.push(p as u16);
                    }
                }
                _ => parents.push((
                    hash.as_str(),
                    SweptFrom::Patches(vec![p as u16]),
                    *bbox,
                    &node.id,
                    &node.transform,
                )),
            }
        } else if let Some(ax) = s.face.axis() {
            // Face-direction rule: normals within a 45° cone of the face axis. Mapped into
            // the node's local frame so the sweep and the gradient cone read the same grid.
            let ax_leaf = Rigid::from_doc(&node.transform).to_local_dir(ax);
            let duplicate = parents.iter().any(|(_, from, _, id, _)| {
                *id == node.id.as_str()
                    && match from {
                        SweptFrom::Face(a2) => a2 == &ax_leaf,
                        SweptFrom::Patches(_) => false,
                    }
            });
            if !duplicate {
                parents.push((
                    hash.as_str(),
                    SweptFrom::Face(ax_leaf),
                    *bbox,
                    &node.id,
                    &node.transform,
                ))
            }
        }
    }
    if parents.is_empty() {
        return Expr::Empty;
    }

    let mut acc = Expr::Empty;
    for (hash, from, bbox, _node_id, xform) in parents {
        // The doc-frame dir (and face axis) live in a different frame than the hydrated
        // grid: map them through the node transform. Identity transforms are exact no-ops.
        let rig = Rigid::from_doc(xform);
        let dir_leaf = rig.to_local_dir(dir);
        // A face-direction selector names the swept surface by its face: the derived
        // leaf carries the axis as a face hint (in the leaf's own frame) so a region
        // selecting `ext1.px` labels the skirt wall — closing V10's gap that a Face on
        // an Extend node fell through to the primitive branch. The hint rides on the
        // leaf only; the node-frame wrapper maps it back to world with the surface.
        let face_hint = match &from {
            SweptFrom::Face(axis) => {
                let ax = [axis[0].abs(), axis[1].abs(), axis[2].abs()];
                let i = if ax[1] > ax[0] { 1 } else { 0 };
                let i = if ax[2] > ax[i] { 2 } else { i };
                Some(i)
            }
            SweptFrom::Patches(_) => None,
        };
        let e = match crate::meshfield::lookup(crate::meshfield::key_from_hex(hash)) {
            // Hydrated parent: derive the swept field, register it under its input hash and
            // compile exactly like a Mesh (same slots, meshing, picking, persistence).
            Some(f) => match &from {
                SweptFrom::Patches(ids) => {
                    match crate::meshfield::swept_patch_field(&f, dir_leaf, len, ids) {
                        Ok(derived) => {
                            let key = crate::meshfield::extend_key(
                                hash,
                                ids,
                                dir_leaf,
                                len,
                                derived.dist.spacing,
                            );
                            if crate::meshfield::lookup(key).is_none() {
                                crate::meshfield::hydrate(key, derived);
                            }
                            let f2 = crate::meshfield::lookup(key).expect("just hydrated");
                            in_node_frame(
                                Expr::Mesh {
                                    key,
                                    bbox: f2.meta.bbox,
                                    spec: Some(f2.spec()),
                                    face_hint: None,
                                },
                                xform,
                            )
                        }
                        Err(_) => Expr::Empty,
                    }
                }
                SweptFrom::Face(axis) => {
                    match crate::meshfield::swept_face_field(&f, dir_leaf, len, *axis) {
                        Ok(derived) => {
                            let key = crate::meshfield::extend_key_face(
                                hash,
                                *axis,
                                dir_leaf,
                                len,
                                derived.dist.spacing,
                            );
                            if crate::meshfield::lookup(key).is_none() {
                                crate::meshfield::hydrate(key, derived);
                            }
                            let f2 = crate::meshfield::lookup(key).expect("just hydrated");
                            in_node_frame(
                                Expr::Mesh {
                                    key,
                                    bbox: f2.meta.bbox,
                                    spec: Some(f2.spec()),
                                    face_hint,
                                },
                                xform,
                            )
                        }
                        Err(_) => Expr::Empty,
                    }
                }
            },
            // Unhydrated parent: vacuum over the swept hull of the node's bounds (no slots —
            // `spec` stays None — and meshing finds nothing there). The hull displacement
            // must be leaf-frame too: the placeholder box is, and the node frame wraps it.
            None => in_node_frame(
                Expr::Mesh {
                    key: 0,
                    bbox: swept_placeholder_bbox(bbox, dir_leaf, len),
                    spec: None,
                    face_hint,
                },
                xform,
            ),
        };
        if matches!(e, Expr::Empty) {
            continue;
        }
        acc = if matches!(acc, Expr::Empty) {
            e
        } else {
            Expr::Combine {
                op: Op::Add,
                blend: 0.0,
                a: Box::new(acc),
                b: Box::new(e),
            }
        };
    }
    acc
}

/// Conservative bounds of a sweep placeholder: the parent's box union its copy displaced by
/// `len·dir`.
fn swept_placeholder_bbox(bbox: [f64; 6], dir: [f64; 3], len: f64) -> Aabb {
    let mut lo = [bbox[0], bbox[1], bbox[2]];
    let mut hi = [bbox[3], bbox[4], bbox[5]];
    for a in 0..3 {
        lo[a] = lo[a].min(bbox[a] + dir[a] * len);
        hi[a] = hi[a].max(hi[a] + dir[a] * len);
    }
    Aabb { lo, hi }
}

/// Place an arm derived from a mesh node's LOCAL frame (the hydrated grid the sweep reads,
/// and the node bounds the placeholder inflates) into that node's placement, matching how
/// the mesh leaf itself wraps in `compile_node`.
fn in_node_frame(e: Expr, xform: &fieldsmith_doc::Transform) -> Expr {
    if xform.is_identity() {
        e
    } else {
        Expr::Xform {
            m: Rigid::from_doc(xform),
            e: Box::new(e),
        }
    }
}

/// Shell-side ordinal for the mesh bakes, in [`ShellSide`]'s variant order.
fn shell_ordinal(side: &ShellSide) -> u8 {
    match side {
        ShellSide::Inside => crate::meshfield::SHELL_INSIDE,
        ShellSide::Outside => crate::meshfield::SHELL_OUTSIDE,
        ShellSide::Centre => crate::meshfield::SHELL_CENTRE,
    }
}

/// Mesh-operand shell/offset bakes a derived grid (goal M7, step 2): the eval-time wrap
/// below is only sound within the ±3·spacing band, so a hydrated single-mesh operand
/// compiles to a keyed derived `Expr::Mesh` leaf instead, and placement wrappers re-wrap
/// the derived leaf unchanged (placement changes placement, not distance content).
/// Groups of several operands, unhydrated meshes and refused bakes return `None` —
/// the caller keeps today's eval-time wrap, which validate names conditions on.
fn derive_modifier(e: &Expr, m: &Modifier, node: &str) -> Option<Expr> {
    match e {
        Expr::Mesh { key, .. } => {
            let hash = format!("{key:016x}");
            let f = crate::meshfield::lookup(*key)?;
            let (new_key, derived) = match m {
                Modifier::Offset { distance } => {
                    let k = crate::meshfield::offset_key(&hash, *distance);
                    match crate::meshfield::offset_field(&f, *distance, node) {
                        Ok(g) => (k, g),
                        Err(_) => return None,
                    }
                }
                Modifier::Shell { thickness, side } => {
                    let k = crate::meshfield::shell_key(&hash, shell_ordinal(side), *thickness);
                    match crate::meshfield::shell_field(&f, shell_ordinal(side), *thickness, node) {
                        Ok(g) => (k, g),
                        Err(_) => return None,
                    }
                }
                // Placement modifiers never bake; their call falls through to the wrap.
                _ => return None,
            };
            if crate::meshfield::lookup(new_key).is_none() {
                crate::meshfield::hydrate(new_key, derived);
            }
            let f2 = crate::meshfield::lookup(new_key).expect("just hydrated");
            Some(Expr::Mesh {
                key: new_key,
                bbox: f2.meta.bbox,
                spec: Some(f2.spec()),
                face_hint: None,
            })
        }
        Expr::Xform { m: rig, e } => derive_modifier(e, m, node).map(|inner| Expr::Xform {
            m: *rig,
            e: Box::new(inner),
        }),
        Expr::Mirror { axis, e } => derive_modifier(e, m, node).map(|inner| Expr::Mirror {
            axis: *axis,
            e: Box::new(inner),
        }),
        Expr::Linear { count, step, e } => derive_modifier(e, m, node).map(|inner| Expr::Linear {
            count: *count,
            step: *step,
            e: Box::new(inner),
        }),
        Expr::Circular {
            count,
            axis,
            step_deg,
            e,
        } => derive_modifier(e, m, node).map(|inner| Expr::Circular {
            count: *count,
            axis: *axis,
            step_deg: *step_deg,
            e: Box::new(inner),
        }),
        _ => None,
    }
}

fn apply_modifier(e: Expr, m: &Modifier, node: &str) -> Expr {
    if matches!(e, Expr::Empty) {
        return e;
    }
    if let Some(derived) = derive_modifier(&e, m, node) {
        return derived;
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
pub fn compile_list(doc: &Document, nodes: &[Node]) -> Expr {
    let mut acc = Expr::Empty;
    for n in nodes.iter().filter(|n| !n.suppressed) {
        let e = compile_node(doc, n);
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
    compile_list(doc, &doc.root)
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
        Expr::Mesh { key, .. } => match crate::meshfield::lookup(*key) {
            Some(f) => f.sample(p),
            None => D::EMPTY,
        },
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
        Expr::Mesh { bbox, .. } => *bbox,
    }
}

// ---------------------------------------------------------------------------------------------
// Shapes as placed in the world (for regions, picking and protected zones)

/// Expression for one node *as placed in the world* — its own field wrapped in every ancestor's
/// transform and replicating modifier (mirror / patterns), but not combined with siblings.
pub fn placed_shape(doc: &Document, id: &str) -> Option<Expr> {
    fn rec(doc: &Document, nodes: &[Node], id: &str) -> Option<Expr> {
        for n in nodes {
            if n.id == id {
                return Some(compile_node(
                    doc,
                    &Node {
                        suppressed: false,
                        ..n.clone()
                    },
                ));
            }
            if let NodeKind::Group {
                children,
                modifiers,
            } = &n.kind
            {
                if let Some(mut inner) = rec(doc, children, id) {
                    for m in modifiers {
                        if matches!(
                            m,
                            Modifier::Mirror { .. }
                                | Modifier::Linear { .. }
                                | Modifier::Circular { .. }
                        ) {
                            inner = apply_modifier(inner, m, &n.id);
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
    rec(doc, &doc.root, id)
}

/// Evaluate a placed shape and report which of its primitive faces is nearest.
/// Returns `(distance, face)`; the face is `All` for groups and face-less primitives.
pub fn eval_face(e: &Expr, p: V3) -> (f64, Face) {
    match e {
        Expr::Prim(prim) => (eval_prim(prim, p).v, classify_face(prim, p)),
        // A baked mesh leaf with a face hint (a face-rule extend's swept surface, goal
        // M7): the derived surface's own face is the hint's axis, signed by the SDF
        // gradient — the outward normal at `p` points along +axis for a +axis face, so
        // the face is Px/Py/Pz when the gradient agrees with the axis and Nx/Ny/Nz when
        // it opposes it. This is the same outward-normal rule `region_facets` applies to
        // imported meshes (the 45° cone there is a facet-matching tolerance; here the
        // hint is exact, so the sign of the gradient decides). Without a hint the leaf
        // labels `All`, as before.
        Expr::Mesh {
            face_hint: Some(axis),
            ..
        } => {
            let d = eval(e, p);
            let f = match (*axis, d.g[*axis] >= 0.0) {
                (0, true) => Face::Px,
                (0, false) => Face::Nx,
                (1, true) => Face::Py,
                (1, false) => Face::Ny,
                (2, true) => Face::Pz,
                _ => Face::Nz,
            };
            (d.v, f)
        }
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
        // Composite surfaces: the nearer operand's surface owns the face — argmin |d| with
        // ties to `a`, the per-leaf rule [`identify_surface`] already uses. A blend fillet
        // band (blend > 0 with both operands inside the radius) belongs to neither operand
        // and labels `All`; label claims near fillets have no independent reference —
        // `validate` caps blends against mesh operands instead (M7). The distance is the
        // composite's own (the value the old fall-through arm reported).
        Expr::Combine { op: _, blend, a, b } => {
            let da = eval_face(a, p);
            let db = eval_face(b, p);
            let f = if *blend > 0.0 && da.0.abs() < *blend && db.0.abs() < *blend {
                Face::All
            } else if da.0.abs() <= db.0.abs() {
                da.1
            } else {
                db.1
            };
            (eval(e, p).v, f)
        }
        other => (eval(other, p).v, Face::All),
    }
}

/// Evaluate which surface patch of a placed shape's mesh covers `p` — the patch of whichever
/// per-copy leaf surface [`eval_face`] would pick. `None` unless the nearest surface belongs
/// to a mesh leaf's claimed patches (primitive faces carry no patches).
pub fn eval_mesh_patch(e: &Expr, p: V3) -> Option<u16> {
    match e {
        Expr::Mesh { key, .. } => crate::meshfield::lookup(*key).and_then(|f| f.patch_at(p)),
        Expr::Xform { m, e } => eval_mesh_patch(e, m.to_local(p)),
        Expr::Mirror { axis, e } => {
            let (da, _) = eval_face(e, p);
            let (db, _) = eval_face(e, reflect(p, *axis));
            if da.abs() <= db.abs() {
                eval_mesh_patch(e, p)
            } else {
                eval_mesh_patch(e, reflect(p, *axis))
            }
        }
        Expr::Linear { count, step, e } => {
            let pick = (0..*count)
                .map(|i| {
                    let q = sub(p, scale(*step, i as f64));
                    (i, eval_face(e, q).0.abs())
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            pick.and_then(|i| eval_mesh_patch(e, sub(p, scale(*step, i as f64))))
        }
        Expr::Circular {
            count,
            axis,
            step_deg,
            e,
        } => {
            let pick = (0..*count)
                .map(|i| {
                    let q = mat_t_vec(&rotation_axis_deg(*axis, step_deg * i as f64), p);
                    (i, eval_face(e, q).0.abs())
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            pick.and_then(|i| {
                eval_mesh_patch(
                    e,
                    mat_t_vec(&rotation_axis_deg(*axis, step_deg * i as f64), p),
                )
            })
        }
        // Composites: the same winner rule `eval_face` uses (argmin |d| through the same
        // face evaluations, ties to `a`, blend band → no patch), so a face claim and its
        // patch claim always agree on which operand's surface they sit on.
        Expr::Combine { op: _, blend, a, b } => {
            let (da, _) = eval_face(a, p);
            let (db, _) = eval_face(b, p);
            if *blend > 0.0 && da.abs() < *blend && db.abs() < *blend {
                None
            } else if db.abs() < da.abs() {
                eval_mesh_patch(b, p)
            } else {
                eval_mesh_patch(a, p)
            }
        }
        _ => None,
    }
}

/// The local primitive of a leaf node (for listing selectable faces).
pub fn node_prim(node: &Node) -> Option<Prim> {
    let doc = Document::default();
    match compile_node(
        &doc,
        &Node {
            transform: Default::default(),
            ..node.clone()
        },
    ) {
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
