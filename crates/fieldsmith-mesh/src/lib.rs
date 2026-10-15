//! Quality tetrahedral meshing of an implicit solid by isosurface stuffing.
//!
//! Follows Labelle & Shewchuk, "Isosurface Stuffing: Fast Tetrahedral Meshes with Good Dihedral
//! Angles" (SIGGRAPH 2007), uniform variant:
//!
//! 1. Sample the signed distance field on a body-centred cubic (BCC) lattice.
//! 2. Where a lattice edge crosses the surface close to one of its ends, *warp* that lattice
//!    vertex onto the crossing (its value becomes exactly zero).
//! 3. Cut the remaining crossing edges at the zero level and fill each lattice tetrahedron's
//!    inside part with one of a few stencils. Quadrilateral faces are split along the diagonal
//!    that touches the vertex with the smallest global index, so neighbouring tetrahedra agree.
//!
//! The lattice is aligned to the solid's bounding box with per-axis spacing chosen so that the box
//! is a whole number of cells. Planar faces that coincide with lattice planes (the outer faces of
//! most mechanical parts, and inner faces at grid multiples) are then reproduced exactly.

pub mod adaptive;
pub mod export;
pub mod gltf;
pub mod obj;
pub mod quality;
pub mod threemf;
pub mod weld;
pub mod zip;
pub use quality::{quality, Quality};

use std::collections::HashMap;

pub type V3 = [f64; 3];

/// One named mesh read from a scene file (GLB, OBJ): all of that mesh's triangle
/// primitives, with every node instance's transform baked into the vertices. The empty
/// name means the file did not name the object; the import layer names it then.
///
/// `material` is the file's own material label when the format records one (3MF
/// `basematerials` via the object's `pid`/`pindex`); `None` elsewhere. Provenance only:
/// names from another tool's library are not the document's material ids, so import
/// reports them but does not adopt them.
#[derive(Clone, Debug)]
pub struct NamedPart {
    pub name: String,
    pub mesh: export::TriMesh,
    pub material: Option<String>,
}

/// A linear tetrahedral mesh. Tetrahedra are positively oriented.
#[derive(Clone, Debug, Default)]
pub struct TetMesh {
    pub verts: Vec<V3>,
    pub tets: Vec<[u32; 4]>,
}

/// Lattice parameters actually used (per-axis spacing).
#[derive(Clone, Copy, Debug)]
pub struct Lattice {
    pub origin: V3,
    pub h: V3,
    pub n: [usize; 3],
}

// Warp thresholds from the paper (fractions of the edge length).
const ALPHA_LONG: f64 = 0.24999;
const ALPHA_SHORT: f64 = 0.41189;

// Noise floor for the claim bisection's predicate. Composite fields put a fixed ±ulp value
// (of the probe coordinates, a few evaluation ops deep) on the zero patch next to anything
// the caller's own quantisation calls a max face: measured +5.6e-17 on the mouth fixture's
// top plane and ±3.6e-15 on the skirt model's cap. Below that the patch can read "positive"
// the whole way to the bracket's far end and the bisection collapses onto the window end —
// the junction node is lost outright (the mouth apex at 0.013 of a pitch) instead of landing
// NOISE/slope off the surface (~1e-14, a planar sliver the emit floor drops). Parts evaluated
// at much larger coordinate scales carry proportionally larger probe noise; unmeasured here.
const NOISE: f64 = 1e-14;

impl Lattice {
    /// A lattice covering `[lo, hi]` with spacing close to `h`, padded by one cell, aligned so
    /// that `lo` and `hi` fall on lattice planes.
    pub fn fit(lo: V3, hi: V3, h: f64) -> Lattice {
        let mut origin = [0.0; 3];
        let mut hh = [0.0; 3];
        let mut n = [0usize; 3];
        for a in 0..3 {
            let ext = (hi[a] - lo[a]).max(1e-9);
            let cells = (ext / h).round().max(1.0);
            hh[a] = ext / cells;
            origin[a] = lo[a] - hh[a];
            n[a] = cells as usize + 2;
        }
        Lattice { origin, h: hh, n }
    }
    #[inline]
    fn corner(&self, i: usize, j: usize, k: usize) -> usize {
        i + (self.n[0] + 1) * (j + (self.n[1] + 1) * k)
    }
    #[inline]
    fn ncorners(&self) -> usize {
        (self.n[0] + 1) * (self.n[1] + 1) * (self.n[2] + 1)
    }
    #[inline]
    fn centre(&self, i: usize, j: usize, k: usize) -> usize {
        self.ncorners() + i + self.n[0] * (j + self.n[1] * k)
    }
    pub fn nverts(&self) -> usize {
        self.ncorners() + self.n[0] * self.n[1] * self.n[2]
    }
    fn position(&self, id: usize) -> V3 {
        let nc = self.ncorners();
        if id < nc {
            let i = id % (self.n[0] + 1);
            let j = (id / (self.n[0] + 1)) % (self.n[1] + 1);
            let k = id / ((self.n[0] + 1) * (self.n[1] + 1));
            [
                self.origin[0] + i as f64 * self.h[0],
                self.origin[1] + j as f64 * self.h[1],
                self.origin[2] + k as f64 * self.h[2],
            ]
        } else {
            let c = id - nc;
            let i = c % self.n[0];
            let j = (c / self.n[0]) % self.n[1];
            let k = c / (self.n[0] * self.n[1]);
            [
                self.origin[0] + (i as f64 + 0.5) * self.h[0],
                self.origin[1] + (j as f64 + 0.5) * self.h[1],
                self.origin[2] + (k as f64 + 0.5) * self.h[2],
            ]
        }
    }

    /// Every lattice edge once, flagged long (axis-aligned) or short (centre to corner).
    fn edges(&self, mut f: impl FnMut(usize, usize, bool)) {
        let [nx, ny, nz] = self.n;
        // Long corner-corner edges.
        for k in 0..=nz {
            for j in 0..=ny {
                for i in 0..=nx {
                    let a = self.corner(i, j, k);
                    if i < nx {
                        f(a, self.corner(i + 1, j, k), true);
                    }
                    if j < ny {
                        f(a, self.corner(i, j + 1, k), true);
                    }
                    if k < nz {
                        f(a, self.corner(i, j, k + 1), true);
                    }
                }
            }
        }
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let c = self.centre(i, j, k);
                    // Long centre-centre edges.
                    if i + 1 < nx {
                        f(c, self.centre(i + 1, j, k), true);
                    }
                    if j + 1 < ny {
                        f(c, self.centre(i, j + 1, k), true);
                    }
                    if k + 1 < nz {
                        f(c, self.centre(i, j, k + 1), true);
                    }
                    // Short centre-corner edges.
                    for dk in 0..2 {
                        for dj in 0..2 {
                            for di in 0..2 {
                                f(c, self.corner(i + di, j + dj, k + dk), false);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Every BCC tetrahedron: two adjacent centres and an edge of their shared square face.
    fn tets(&self, mut f: impl FnMut([usize; 4])) {
        let [nx, ny, nz] = self.n;
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let c = self.centre(i, j, k);
                    // Neighbour along +x: shared face at corner-plane i+1.
                    if i + 1 < nx {
                        let d = self.centre(i + 1, j, k);
                        let q = [
                            self.corner(i + 1, j, k),
                            self.corner(i + 1, j + 1, k),
                            self.corner(i + 1, j + 1, k + 1),
                            self.corner(i + 1, j, k + 1),
                        ];
                        for e in 0..4 {
                            f([c, d, q[e], q[(e + 1) % 4]]);
                        }
                    }
                    if j + 1 < ny {
                        let d = self.centre(i, j + 1, k);
                        let q = [
                            self.corner(i, j + 1, k),
                            self.corner(i + 1, j + 1, k),
                            self.corner(i + 1, j + 1, k + 1),
                            self.corner(i, j + 1, k + 1),
                        ];
                        for e in 0..4 {
                            f([c, d, q[e], q[(e + 1) % 4]]);
                        }
                    }
                    if k + 1 < nz {
                        let d = self.centre(i, j, k + 1);
                        let q = [
                            self.corner(i, j, k + 1),
                            self.corner(i + 1, j, k + 1),
                            self.corner(i + 1, j + 1, k + 1),
                            self.corner(i, j + 1, k + 1),
                        ];
                        for e in 0..4 {
                            f([c, d, q[e], q[(e + 1) % 4]]);
                        }
                    }
                }
            }
        }
    }
}

#[inline]
fn lerp(a: V3, b: V3, t: f64) -> V3 {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Zero crossing of `f` on the segment `a`–`b` (with `fa < 0 < fb` or the reverse), as a
/// parameter in (0, 1). Illinois-modified regula falsi; converges to ~1e-9 of the edge.
fn crossing(f: &impl Fn(V3) -> f64, a: V3, b: V3, fa: f64, fb: f64) -> f64 {
    let (mut t0, mut t1, mut f0, mut f1) = (0.0f64, 1.0f64, fa, fb);
    let mut side = 0;
    let mut t = f0 / (f0 - f1);
    for _ in 0..40 {
        t = (t0 * f1 - t1 * f0) / (f1 - f0);
        if !(t > t0 && t < t1) {
            t = 0.5 * (t0 + t1);
        }
        let ft = f(lerp(a, b, t));
        if ft == 0.0 || (t1 - t0) < 1e-10 {
            break;
        }
        if (ft < 0.0) == (f0 < 0.0) {
            t0 = t;
            f0 = ft;
            if side == -1 {
                f1 *= 0.5;
            }
            side = -1;
        } else {
            t1 = t;
            f1 = ft;
            if side == 1 {
                f0 *= 0.5;
            }
            side = 1;
        }
    }
    t
}

/// Parameter (in the a→b sense) of the boundary of the strictly-positive region of the raw
/// field on the segment a–b, given a probe inside it (`f > 0`) and one beyond on `other`
/// (`f ≤ 0`). [`crossing`] cannot be reused: the `≤0` side of a max-face edge lies on a zero
/// patch where every probe reads exactly the same fixed value and the `ft == 0` break stops at
/// the first one — potentially the whole bracket away from the crossing. Bisect instead on
/// `raw(p) > NOISE`. With a zero-floor predicate the answer pins to the caller's own
/// quantisation — the tiny band's edge, tiny/slope off the surface, far enough that a rim claim
/// and a half-pitch cut on the same line carry distinct residuals and the consuming stencil
/// emits a planar sliver; with a floor that is itself below the quantisation the patch's
/// constant ±ulp probe values (a lattice coordinate one ulp outside a face) can instead read
/// positive along the whole patch and the answer collapses onto the window end, losing the
/// junction node. At ~1e-14 the answer lands NOISE/slope off the raw surface and a claim and
/// a cut on the same line disagree by less than the emit floor drops. Tracks the region
/// adjoining `pos` (skips interior gaps), under the same one-crossing-per-edge assumption
/// [`crossing`] carries. 60 iterations with the 1e-14 break land at ~2⁻⁶⁰ of the edge.
fn positive_boundary(
    raw: &impl Fn(V3) -> f64,
    a: V3,
    b: V3,
    pos: (f64, V3),
    other: (f64, V3),
) -> f64 {
    let mut tp = pos.0;
    let mut tn = other.0;
    for _ in 0..60 {
        if tn - tp < 1e-14 {
            break;
        }
        let tm = 0.5 * (tp + tn);
        if raw(lerp(a, b, tm)) > NOISE {
            tp = tm;
        } else {
            tn = tm;
        }
    }
    0.5 * (tp + tn)
}

/// Mesh the solid `{p : f(p) < 0}` inside the box `[lo, hi]` with target spacing `h`.
///
/// Lattice positions are computed on demand ([`Lattice::position`] is O(1) arithmetic): at
/// h = 0.25 mm across a 150 mm bracket the full-position table alone would be 1.6 GiB, which
/// does not fit a browser's wasm linear memory alongside `val` and the remap table.
pub fn stuff(f: impl Fn(V3) -> f64, lo: V3, hi: V3, h: f64) -> (TetMesh, Lattice) {
    let lat = Lattice::fit(lo, hi, h);
    let nv = lat.nverts();
    let pos = |i: usize| -> V3 { lat.position(i) };
    let mut val: Vec<f64> = (0..nv).map(|i| f(pos(i))).collect();
    // Points on lattice planes that coincide with faces evaluate to ±rounding noise; snap them.
    let tiny = 1e-9 * lat.h[0].max(lat.h[1]).max(lat.h[2]);
    for v in val.iter_mut() {
        if v.abs() < tiny {
            *v = 0.0;
        }
    }

    // ---- drop coincident-surface vertices ------------------------------------------------------
    // Where two primitive surfaces coincide on a flat region — a subtracted cylinder whose cap
    // disc sits in the parent's cap plane — the composite evaluates max(0, -0) = 0 across the
    // whole shared plane, so lattice vertices there read as surface even though the true
    // surface is only a rim or part of the annulus. The composite's sign is exact off those
    // planes, so probe the eight diagonals a small offset away: a real surface vertex has
    // material in at least one inward direction; the rest are treated as outside. Vertices
    // within the offset of true material are kept — their position error is the offset.
    let max_h = lat.h[0].max(lat.h[1]).max(lat.h[2]);
    let eps = 0.05 * max_h;
    let e = eps / 3.0f64.sqrt();
    let spur: Vec<usize> = (0..nv)
        .filter(|&i| {
            if val[i] != 0.0 {
                return false;
            }
            let p = pos(i);
            ![-1.0f64, 1.0].iter().any(|&sx| {
                [-1.0f64, 1.0].iter().any(|&sy| {
                    [-1.0f64, 1.0]
                        .iter()
                        .any(|&sz| f([p[0] + sx * e, p[1] + sy * e, p[2] + sz * e]) < 0.0)
                })
            })
        })
        .collect();
    for &i in &spur {
        val[i] = eps;
    }

    // ---- warp vertices that lie too close to a crossing --------------------------------------
    // For each violated vertex keep the nearest violating cut point (as an edge fraction); the
    // cut point becomes the vertex's emitted position (a handful of vertices, so a map, not a
    // whole-lattice table). Max-face edges — exactly one end strictly positive, the other
    // exactly 0 — are scanned too: on a symmetry plane the field is exactly 0 across the whole
    // face, so their hole side never reads negative and the strict-sign branch never fires;
    // without this, junction nodes where a wall meets such a face are missed entirely. The
    // positive end claims the node when the crossing sits inside its claim window; a deeper
    // crossing claims the zero end itself, and pairs whose own crossing the claimed end
    // no longer covers store the crossing in `zero_cuts` for cut-node emission (second pass
    // below).
    let mut warp: HashMap<usize, (f64, V3)> = HashMap::new();
    // Mid-edge crossings stored for cut-node emission: fraction from the smaller id to the
    // larger. Every stored crossing is consumed between the pair's raw lattice positions —
    // the bisection ran on that segment, and a claimed end's claim point belongs to its
    // claiming edge, which may exit through another wall entirely.
    let mut zero_cuts: HashMap<(usize, usize), f64> = HashMap::new();
    // Claim runs per vertex: how many edges claimed it in this pass. The second pass below
    // stores raw crossings only through single-run claims (see the note after the pass).
    let mut claim_runs: HashMap<usize, usize> = HashMap::new();
    lat.edges(|a, b, long| {
        let (fa, fb) = (val[a], val[b]);
        let alpha = if long { ALPHA_LONG } else { ALPHA_SHORT };
        if !((fa < 0.0 && fb > 0.0) || (fa > 0.0 && fb < 0.0)) {
            if !((fa == 0.0 && fb > 0.0) || (fb == 0.0 && fa > 0.0)) {
                return;
            }
            // u = the positive end, z = the zero end; s measured u→z. The ≤0 side of the edge
            // lies on a zero patch, so this is `positive_boundary`'s job, not `crossing`'s.
            let (u, z) = if fb > 0.0 { (b, a) } else { (a, b) };
            let pu = pos(u);
            let pz = pos(z);
            let rim = |ts: f64| lerp(pu, pz, ts);
            // Probe sign at the val snapshot's own noise floor. A lattice plane coinciding with
            // a face carries up to an ulp of coordinate error from the `origin + k·h`
            // accumulation (−0.11 + 5·0.11 rounds to 0.44 + 1ulp), and the box distance turns a
            // coordinate one ulp outside a face into a strictly positive value; at interior
            // planes the same error folds back against the face and is invisible. `tiny` already
            // declares such points zero in `val`; the probes must agree or the windows
            // misclassify (the top-plane edge read skipfar and its junction node was lost).
            let sign = |p: V3| if f(p) > tiny { 1.0 } else { -1.0 };
            // The positive end must be positive in the field, not only in `val`. A spur vertex
            // — a coincident-surface corner (two faces' caps on one lattice plane read a zero
            // patch across the whole disc) flipped to `eps` so no cell keeps it — has
            // `val > 0` while `f(pu)` is exactly 0. Claiming such an end feeds `positive_boundary`
            // an unsigned bracket (both probes ≤ 0), the walk degenerates to ~0, and the warp
            // pins the vertex at its own inside-the-void position as a kept corner — material
            // tets reaching ~h/3 into the bore at every cap face (the V20 regression). Such an
            // edge rides the zero patch with no interior crossing on its ≤0 side: skip it, as
            // the strict-sign branch did before the max-face scan existed.
            if sign(pu) < 0.0 {
                return;
            }
            // Window order matters. Probe the far window first: an edge from the padded outer
            // lattice ring to a face corner is a max-face pair by construction (the padded
            // corner reads +h, the face corner exactly 0) but carries no interior crossing —
            // its surface is the zero end itself, and the lattice vertex already lies on the
            // surface. Bisecting it anyway breaks `positive_boundary`'s contract (both probes
            // positive, the walk degenerates to alpha) and warps the padded corner onto phantom
            // positions just outside the box, growing the volume. Settling these in one eval
            // also guarantees the two bisections below only ever launch from a correctly signed
            // bracket.
            if sign(rim(1.0 - alpha)) > 0.0 {
                return;
            }
            // Crossing beyond the claim window: a mid-edge crossing here would mint a new
            // vertex, and the added dofs re-tessellate the mesh around the bore crown — on V3
            // they pulled the Kt answer off band on three of five rungs while claims alone
            // move it the other way. The mouth contract needs a node ON the rim, not a new
            // dof, and the zero end is a kept corner on the max face (`val == 0`): claim it
            // instead — nearest-wins with run bookkeeping, exactly like the claim window
            // below — presenting the crossing at that vertex to every consuming stencil with
            // no new dofs. Reach runs to 1 − alpha, held green by the fixture outcomes
            // (tests/mouth_junction.rs: apex, seal and volume through 0.7 of a pitch). Pairs
            // whose own crossing the claimed corner no longer covers ride the second pass
            // below as raw tails.
            if sign(rim(alpha)) > 0.0 {
                let s = positive_boundary(
                    &f,
                    pu,
                    pz,
                    (alpha, rim(alpha)),
                    (1.0 - alpha, rim(1.0 - alpha)),
                );
                let c = rim(s);
                let reach = 1.0 - s;
                let e = warp.entry(z).or_insert((f64::INFINITY, c));
                if reach < e.0 {
                    *e = (reach, c);
                }
                *claim_runs.entry(z).or_insert(0) += 1;
                return;
            }
            // Claim window, capped: the claim drags the void corner onto the rim, so its reach
            // is capped at the long-edge alpha. On short edges a deeper crossing — a void
            // centre nearly the whole half-diagonal from the rim (V3's mid-layer centres, s up
            // to 0.41) — would pull a full 30%-of-h displacement into interior-void geometry
            // whose real material neighbours claim nothing there. Long edges keep the full
            // window (`aclaim == alpha`), the mouth apexes all cross inside it.
            let aclaim = if long { alpha } else { ALPHA_LONG };
            if sign(rim(aclaim)) > 0.0 {
                return;
            }
            // Claim window: warp the positive corner onto the first zero. The bracket ends are
            // correctly classified: the guards above read the `tiny`-quantised sign, and the
            // bisection below floors the raw patch at NOISE. The ulp-valued lattice-plane
            // probes that `tiny` treats as zero are also below NOISE (a plane one ulp past a
            // face evaluates its probes at +ulp too), so they read as the zero patch to both
            // and the claim lands on the rim.
            let s = positive_boundary(&f, pu, pz, (0.0, pu), (aclaim, rim(aclaim)));
            let c = rim(s);
            let e = warp.entry(u).or_insert((f64::INFINITY, c));
            if s < e.0 {
                *e = (s, c);
            }
            *claim_runs.entry(u).or_insert(0) += 1;
        }
        let t = crossing(&f, pos(a), pos(b), fa, fb);
        let c = lerp(pos(a), pos(b), t);
        // Both endpoint kinds claim. A claim on the material side zeroes the only negative
        // vertex of the mouth strip's tets (the quarter model's h = 0.07 tangent): without a
        // record of the raw crossing those tets' cut nodes collapse onto the claimed corner's
        // own claim point and the plane wedges are lost — the apexes print as orphan vertices
        // and the V3 row peaks 10+% high (tests/mouth_junction.rs). The second edges pass
        // below stores those crossings, so the cut-node lookup hands the consuming stencils a
        // real surface node on the pair's own segment: the claim moves the vertex onto the
        // surface rather than deleting its sliver.
        if t < alpha {
            let e = warp.entry(a).or_insert((f64::INFINITY, c));
            if t < e.0 {
                *e = (t, c);
            }
            *claim_runs.entry(a).or_insert(0) += 1;
        } else if 1.0 - t < alpha {
            let e = warp.entry(b).or_insert((f64::INFINITY, c));
            if 1.0 - t < e.0 {
                *e = (1.0 - t, c);
            }
            *claim_runs.entry(b).or_insert(0) += 1;
        }
    });
    // A vertex claimed by several edges has crossings riding different edges inside its one
    // claim window — a corner graze, e.g. a curved wall sweeping past a lattice corner
    // (V21's bores graze theirs). The claimed end keeps its own claim point; the other
    // crossings arrive here as second-pass tails with the claimed end already zeroed, and a
    // tail a fraction of the claim window from its claimed end emits a sub-degree sliver
    // wedge (an edge fraction ~0.03·h from the claimed vertex; the two-level
    // preconditioner's coarse LDLT went not-positive-definite on them at the V21
    // sentinel's coarse rung). So the pass below stores a raw crossing only when every
    // claimed end it touches is single-run — one crossing to snap, whose node really is on
    // that pair's own segment.
    // Second pass over the same edges: every pair with one claimed end and a strict sign change
    // pre-reset (claimed material ends read `val < 0` here — the reset below has not run
    // — claimed void ends `val > 0`) is a pair whose cut the stencils touch with the claimed
    // end zeroed. `cut`'s zero-end branch needs the crossing to emit a real surface node on
    // the pair's own segment instead of the claimed corner's point.
    lat.edges(|a, b, _| {
        let (fa, fb) = (val[a], val[b]);
        if !(warp.contains_key(&a) || warp.contains_key(&b)) {
            return;
        }
        if !((fa < 0.0 && fb > tiny) || (fa > tiny && fb < 0.0)) {
            return;
        }
        let t = crossing(&f, pos(a), pos(b), fa, fb);
        // An end may have been claimed by this very pair: the record's claim point is then
        // this same crossing, bit for bit (the claiming bisection ran on identical arguments).
        // Skip the entry — the consuming cut falls through to the zero-end branch and gets the
        // claimed vertex itself; storing the crossing would emit a second node at the claim
        // point. A record from a different edge matches nothing here, and that pair keeps its
        // raw crossing.
        let cp = lerp(pos(a), pos(b), t);
        if [a, b]
            .iter()
            .any(|&v| warp.contains_key(&v) && claim_runs.get(&v) != Some(&1))
        {
            return;
        }
        if [a, b]
            .iter()
            .any(|&v| warp.get(&v).is_some_and(|&(_, c)| c == cp))
        {
            return;
        }
        // Fractions run smaller-id → larger-id (the key `cut` looks up and lerps from); `t`
        // runs a→b. Consumed between the pair's raw lattice positions: the bisection ran
        // there, and a claimed end's own claim point belongs to its claiming edge, whose
        // direction may exit through another wall entirely.
        zero_cuts
            .entry(if a < b { (a, b) } else { (b, a) })
            .or_insert(if a < b { t } else { 1.0 - t });
    });
    for &v in warp.keys() {
        val[v] = 0.0;
    }

    // ---- stencils ----------------------------------------------------------------------------
    // Emitted position of a lattice vertex: its warped cut point when it was warped, else the
    // lattice position.
    let warp_pos = |v: usize| -> V3 {
        match warp.get(&v) {
            Some(&(_, c)) => c,
            None => pos(v),
        }
    };
    let mut out = TetMesh::default();
    let mut remap: Vec<u32> = vec![u32::MAX; nv];
    let mut cuts: HashMap<(usize, usize), u32> = HashMap::new();
    // Global order used for diagonal choice: lattice vertices by lattice id, cut points after
    // all lattice vertices in creation order (stable for a given lattice and field).
    let mut gid: Vec<usize> = Vec::new();
    let mut lattice_vertex = |v: usize, out: &mut TetMesh, gid: &mut Vec<usize>| -> u32 {
        if remap[v] == u32::MAX {
            remap[v] = out.verts.len() as u32;
            out.verts.push(warp_pos(v));
            gid.push(v);
        }
        remap[v]
    };

    lat.tets(|t| {
        let s = t.map(|v| {
            if val[v] < 0.0 {
                -1i8
            } else if val[v] > 0.0 {
                1
            } else {
                0
            }
        });
        let npos = s.iter().filter(|&&x| x > 0).count();
        let nneg = s.iter().filter(|&&x| x < 0).count();
        // Sorted lattice pair of the tet edge (i, j) — the key of `cuts` and `zero_cuts`.
        let ek = |i: usize, j: usize| -> (usize, usize) {
            if t[i] < t[j] {
                (t[i], t[j])
            } else {
                (t[j], t[i])
            }
        };
        if nneg == 0 {
            if npos == 0 {
                // All four vertices on the surface: keep if the tet lies inside.
                let c = [0, 1, 2].map(|a| {
                    (warp_pos(t[0])[a] + warp_pos(t[1])[a] + warp_pos(t[2])[a] + warp_pos(t[3])[a])
                        * 0.25
                });
                if f(c) < 0.0 {
                    let v = t.map(|x| lattice_vertex(x, &mut out, &mut gid));
                    emit(&mut out, v);
                }
            }
            // A hole-side tet with no material corner drops: nothing on it mints a cut node.
            return;
        }
        let vids: [u32; 4] = t.map(|x| {
            if val[x] <= 0.0 {
                lattice_vertex(x, &mut out, &mut gid)
            } else {
                u32::MAX
            }
        });
        // Surface node on the tet edge (a, b). Strict sign changes cross as before; a pair
        // with a zero end — a zero-patch vertex or a claimed one (claims zero their target,
        // so a still-positive end never warps) — takes its stored mid-edge crossing when
        // there is one, else the zero end's own lattice vertex as the boundary rides out
        // past the pair. Stored crossings are consumed between the pair's raw lattice
        // positions: the bisection ran on that segment, and a claimed end's own claim point
        // would drag the node off its own wall.
        let mut cut = |a: usize, b: usize, out: &mut TetMesh, gid: &mut Vec<usize>| -> u32 {
            let key = ek(a, b);
            *cuts.entry(key).or_insert_with(|| {
                let (p, q) = (warp_pos(key.0), warp_pos(key.1));
                if val[key.0] == 0.0 || val[key.1] == 0.0 {
                    if let Some(&w) = zero_cuts.get(&key) {
                        let c = lerp(pos(key.0), pos(key.1), w);
                        out.verts.push(c);
                        gid.push(nv + out.verts.len());
                        return (out.verts.len() - 1) as u32;
                    }
                    let zv = if val[key.0] == 0.0 { key.0 } else { key.1 };
                    return lattice_vertex(zv, out, gid);
                }
                let u = crossing(&f, p, q, val[key.0], val[key.1]);
                out.verts.push(lerp(p, q, u));
                gid.push(nv + out.verts.len());
                (out.verts.len() - 1) as u32
            })
        };
        if npos == 0 {
            emit(&mut out, vids);
            return;
        }
        let negs: Vec<usize> = (0..4).filter(|&i| s[i] < 0).collect();
        let zeros: Vec<usize> = (0..4).filter(|&i| s[i] == 0).collect();
        let poss: Vec<usize> = (0..4).filter(|&i| s[i] > 0).collect();
        match (nneg, zeros.len(), npos) {
            (1, _, _) => {
                // One inside vertex: a single tet, outside vertices replaced by cut points.
                let a = negs[0];
                let mut v = vids;
                for &p in &poss {
                    v[p] = cut(a, p, &mut out, &mut gid);
                }
                emit(&mut out, v);
            }
            (2, 0, 2) => {
                // Prism: (a, ac, ad) – (b, bc, bd).
                let (a, b, c, d) = (negs[0], negs[1], poss[0], poss[1]);
                let p = [
                    vids[a],
                    cut(a, c, &mut out, &mut gid),
                    cut(a, d, &mut out, &mut gid),
                ];
                let q = [
                    vids[b],
                    cut(b, c, &mut out, &mut gid),
                    cut(b, d, &mut out, &mut gid),
                ];
                prism(&mut out, &gid, p, q);
            }
            (2, 1, _) => {
                // Pyramid with apex z over quad (a, b, bd, ad).
                let (a, b, z, d) = (negs[0], negs[1], zeros[0], poss[0]);
                let ad = cut(a, d, &mut out, &mut gid);
                let bd = cut(b, d, &mut out, &mut gid);
                pyramid(&mut out, &gid, [vids[a], vids[b], bd, ad], vids[z]);
            }
            (3, 0, 1) => {
                // Prism: (a, b, c) – (ad, bd, cd).
                let (a, b, c, d) = (negs[0], negs[1], negs[2], poss[0]);
                let p = [vids[a], vids[b], vids[c]];
                let q = [
                    cut(a, d, &mut out, &mut gid),
                    cut(b, d, &mut out, &mut gid),
                    cut(c, d, &mut out, &mut gid),
                ];
                prism(&mut out, &gid, p, q);
            }
            _ => unreachable!("sign pattern {s:?}"),
        }
    });

    (out, lat)
}

fn signed_volume(m: &TetMesh, t: [u32; 4]) -> f64 {
    let [a, b, c, d] = t.map(|i| m.verts[i as usize]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
    (u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0])
        + u[2] * (v[0] * w[1] - v[1] * w[0]))
        / 6.0
}

/// Append a tetrahedron, oriented positively. Degenerate (zero-volume) pieces are dropped: they
/// arise only where a cut point coincides with a vertex and carry no volume.
fn emit(m: &mut TetMesh, mut t: [u32; 4]) {
    let v = signed_volume(m, t);
    if v.abs() < 1e-14 {
        return;
    }
    // Scale-relative degeneracy bound: a piece this far below the cube of its longest edge
    // represents nothing — structurally flat truncation pieces whose planarity is exact by
    // construction, with only roundoff separating their points (vol/e³ ~ 1e-16), or cut
    // points coinciding with vertices. Legit material, even the slimmest slivers the lattice
    // produces, sits two-plus orders above.
    let pts = t.map(|i| m.verts[i as usize]);
    let mut e2 = 0.0f64;
    for (i, a) in pts.iter().enumerate() {
        for b in &pts[i + 1..] {
            let d = (a[0] - b[0]) * (a[0] - b[0])
                + (a[1] - b[1]) * (a[1] - b[1])
                + (a[2] - b[2]) * (a[2] - b[2]);
            e2 = e2.max(d);
        }
    }
    if v.abs() < 1e-14 * e2 * e2.sqrt() {
        return;
    }
    if v < 0.0 {
        t.swap(2, 3);
    }
    m.tets.push(t);
}

/// Split a pyramid (quad base `q` in cyclic order, apex `z`) along the base diagonal touching
/// the base vertex with the smallest global index.
fn pyramid(m: &mut TetMesh, gid: &[usize], q: [u32; 4], z: u32) {
    let g = |v: u32| gid[v as usize];
    let k = (0..4).min_by_key(|&i| g(q[i])).unwrap();
    let (a, b, c, d) = (q[k], q[(k + 1) % 4], q[(k + 2) % 4], q[(k + 3) % 4]);
    emit(m, [a, b, c, z]);
    emit(m, [a, c, d, z]);
}

/// Split a triangular prism (bottom `p`, top `q`, `p[i]`–`q[i]` vertical edges) into three
/// tetrahedra whose quad-face diagonals all touch each face's smallest-index vertex
/// (Dompierre et al. 1999), so that neighbouring cells agree.
fn prism(m: &mut TetMesh, gid: &[usize], p: [u32; 3], q: [u32; 3]) {
    let g = |v: u32| gid[v as usize];
    let all = [p[0], p[1], p[2], q[0], q[1], q[2]];
    let k = (0..6).min_by_key(|&i| g(all[i])).unwrap();
    // Rotate/flip so that the minimum is at bottom vertex 0.
    let (r, flip) = (k % 3, k >= 3);
    let (bot, top) = if flip { (q, p) } else { (p, q) };
    let v0 = bot[r];
    let v1 = bot[(r + 1) % 3];
    let v2 = bot[(r + 2) % 3];
    let v3 = top[r];
    let v4 = top[(r + 1) % 3];
    let v5 = top[(r + 2) % 3];
    // Quad opposite v0: (v1, v2, v5, v4). Its diagonal is v1–v5 or v2–v4.
    if g(v1).min(g(v5)) < g(v2).min(g(v4)) {
        emit(m, [v0, v1, v2, v5]);
        emit(m, [v0, v1, v5, v4]);
        emit(m, [v0, v4, v5, v3]);
    } else {
        emit(m, [v0, v1, v2, v4]);
        emit(m, [v0, v4, v2, v5]);
        emit(m, [v0, v4, v5, v3]);
    }
}

impl TetMesh {
    pub fn volume(&self) -> f64 {
        self.tets.iter().map(|&t| signed_volume(self, t)).sum()
    }

    /// Boundary triangles (outward oriented) with the index of the owning tetrahedron.
    ///
    /// The face-usage table is split from the boundary-faces table deliberately: ~2·T distinct
    /// faces across T tets means a combined `HashMap<[u32; 3], (count, face, owner)>` entry
    /// costs 32 B, and at T ≈ 16 M that needs 2^26 buckets ≈ 2.15 GB — past the 32-bit linear
    /// address space (wasm32 calls this "hash table capacity overflow", 64-bit platforms
    /// never see it). A bare count per face fits in ~14 B per bucket, and the boundary faces
    /// (count 1, ≤ T entries) go into their own small map on a second pass.
    pub fn boundary(&self) -> Vec<([u32; 3], u32)> {
        let mut uses: HashMap<[u32; 3], u16> = HashMap::new();
        for t in &self.tets {
            // Outward faces for a positively oriented tet (a, b, c, d).
            let fs = [
                [t[1], t[2], t[3]],
                [t[0], t[3], t[2]],
                [t[0], t[1], t[3]],
                [t[0], t[2], t[1]],
            ];
            for f in fs {
                let mut key = f;
                key.sort_unstable();
                uses.entry(key)
                    .and_modify(|c| *c = c.saturating_add(1))
                    .or_insert(1);
            }
        }
        // Boundary faces: collect their outward orientation (the single occurrence's winding)
        // and owning tet.
        let mut faces: HashMap<[u32; 3], ([u32; 3], u32)> =
            HashMap::with_capacity(uses.len() / 16 + 1024);
        for (ti, t) in self.tets.iter().enumerate() {
            let fs = [
                [t[1], t[2], t[3]],
                [t[0], t[3], t[2]],
                [t[0], t[1], t[3]],
                [t[0], t[2], t[1]],
            ];
            for f in fs {
                let mut key = f;
                key.sort_unstable();
                if uses.get(&key).is_some_and(|&c| c == 1) {
                    faces.insert(key, (f, ti as u32));
                }
            }
        }
        let mut out: Vec<([u32; 3], u32)> = faces.into_values().collect();
        out.sort_unstable_by_key(|e| e.1);
        out
    }

    /// Remove vertices not referenced by any tetrahedron.
    pub fn compact(&mut self) {
        let mut used = vec![u32::MAX; self.verts.len()];
        let mut verts = Vec::new();
        for t in self.tets.iter_mut() {
            for v in t.iter_mut() {
                if used[*v as usize] == u32::MAX {
                    used[*v as usize] = verts.len() as u32;
                    verts.push(self.verts[*v as usize]);
                }
                *v = used[*v as usize];
            }
        }
        self.verts = verts;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere(p: V3) -> f64 {
        (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 10.0
    }

    #[test]
    fn sphere_volume_and_quality() {
        let (mut m, _) = stuff(sphere, [-10.0; 3], [10.0; 3], 1.0);
        m.compact();
        let exact = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
        let v = m.volume();
        assert!((v - exact).abs() / exact < 0.01, "volume {v} vs {exact}");
        let q = quality(&m);
        assert!(
            q.min_dihedral_deg > 8.0,
            "min dihedral {}",
            q.min_dihedral_deg
        );
        assert!(
            q.max_dihedral_deg < 168.0,
            "max dihedral {}",
            q.max_dihedral_deg
        );
    }

    #[test]
    fn box_is_exact_and_watertight() {
        let f = |p: V3| {
            let q = [
                p[0].abs() - 50.0,
                (p[1] - 5.0).abs() - 5.0,
                (p[2] - 5.0).abs() - 5.0,
            ];
            let o = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)];
            (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt() + q[0].max(q[1]).max(q[2]).min(0.0)
        };
        let (mut m, _) = stuff(f, [-50.0, 0.0, 0.0], [50.0, 10.0, 10.0], 2.5);
        m.compact();
        assert!((m.volume() - 10000.0).abs() < 1e-6, "volume {}", m.volume());
        // Every boundary edge is shared by exactly two boundary triangles.
        let b = m.boundary();
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for (f, _) in &b {
            for i in 0..3 {
                let (a, c) = (f[i], f[(i + 1) % 3]);
                *edges.entry((a, c)).or_default() += 1;
                *edges.entry((c, a)).or_default() -= 1;
            }
        }
        assert!(
            edges.values().all(|&c| c == 0),
            "boundary is not closed and consistently oriented"
        );
    }
}
