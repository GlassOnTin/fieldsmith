//! STEP tessellation engine (V14): analytic B-rep surfaces → triangle meshes.
//!
//! The reader returns the analytic B-rep; flattening it to a mesh for
//! voxelisation is fieldsmith's job. Surfaces with stored analytic forms
//! (plane, cylinder, cone, sphere, torus) are evaluated from those forms
//! exactly; other faces go through a rational B-spline view and triangulate in
//! parameter space. Faces this engine cannot produce are refused by name —
//! never silently dropped.
//!
//! Watertightness: each `EDGE_CURVE` is sampled once (per entity) and the same
//! polyline is reused in both faces sharing it, through a shared quantised
//! vertex pool. Refinement rounds split interior edges at their midpoints;
//! boundary edges are locked (never split), so no T-junctions form across
//! faces. Volume and area references are independent quadratures (boundary
//! flux by Green's theorem, parameter-space area), not derivations of the
//! output triangulation.

use std::collections::{HashMap, HashSet};

use step_io::scene::NurbsCurve;
use step_io::scene::NurbsSurface;

pub type P3 = [f64; 3];
pub type P2 = [f64; 2];

// ---------------------------------------------------------------------------
// small vector helpers
// ---------------------------------------------------------------------------

pub(crate) fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn dot3(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn cross3(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn len3(a: P3) -> f64 {
    dot3(a, a).sqrt()
}
pub(crate) fn dist3(a: P3, b: P3) -> f64 {
    len3(sub(a, b))
}
pub(crate) fn scale3(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(crate) fn add3(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn unit_or(a: P3, fallback: P3) -> P3 {
    let n = len3(a);
    if n > 1e-12 {
        scale3(a, 1.0 / n)
    } else {
        fallback
    }
}
/// Length of the projection of `a` onto the plane ⊥ `z`.
fn proj_len_perp(a: P3, z: P3) -> f64 {
    len3(sub(a, scale3(z, dot3(a, z))))
}

// ---------------------------------------------------------------------------
// frames
// ---------------------------------------------------------------------------

/// Right-handed orthonormal frame for a surface placement: `o + x·u + y·v`,
/// with `z` the surface axis (u/v follow the ISO 10303-42 conventions below).
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub o: P3,
    pub x: P3,
    pub y: P3,
    pub z: P3,
}

impl Frame {
    /// From a placement origin, axis, and optional reference direction: `z` =
    /// axis; `x` = the reference direction with its axis component removed, or
    /// the world direction least aligned with `z` (+X, +Y, +Z in that order)
    /// when there is no usable reference; `y` = `z × x`.
    pub fn build(origin: P3, axis: P3, ref_dir: Option<P3>) -> Result<Frame, String> {
        let z = unit_or(axis, [0.0, 0.0, 0.0]);
        if len3(z) < 0.9999 {
            return Err("surface placement axis is degenerate".to_owned());
        }
        let x = match ref_dir {
            Some(rd) if proj_len_perp(rd, z) > 1e-12 => unit_or(sub(rd, scale3(z, dot3(rd, z))), z),
            _ => stepio_fallback_x(z),
        };
        Ok(Frame {
            o: origin,
            x,
            y: cross3(z, x),
            z,
        })
    }
}

/// Deterministic fallback rule for the u reference direction (documented).
fn stepio_fallback_x(z: P3) -> P3 {
    for cand in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        if dot3(cand, z).abs() < 0.99 {
            return cand;
        }
    }
    unreachable!("unit axis always has a world direction within 0.99")
}

// ---------------------------------------------------------------------------
// surfaces
// ---------------------------------------------------------------------------

/// A surface in the five analytic forms the B-rep reader stores, or as the
/// rational B-spline view every other convertible form reduces to.
#[derive(Clone, Debug)]
pub enum Surf {
    /// `o + u·x + v·y`.
    Plane(Frame),
    /// `o + r·(cos u·x + sin u·y) + v·z`; u has period 2π.
    Cyl { f: Frame, r: f64 },
    /// `o + (r + k·v)·(cos u·x + sin u·y) + v·z`, `k = tan(α)`; u has period 2π.
    Cone { f: Frame, r: f64, k: f64 },
    /// `o + r·(cos v·(cos u·x + sin u·y) + sin v·z)`, v ∈ [−π/2, π/2]; u has
    /// period 2π.
    Sphere { f: Frame, r: f64 },
    /// `o + (R + r·cos v)·(cos u·x + sin u·y) + r·sin v·z`; both u and v have
    /// period 2π.
    Torus { f: Frame, big_r: f64, r: f64 },
    /// Rational B-spline view (world coordinates, millimetres).
    Nurbs(Box<NurbsSurface>),
}

impl Surf {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Surf::Plane(_) => "plane",
            Surf::Cyl { .. } => "cylinder",
            Surf::Cone { .. } => "cone",
            Surf::Sphere { .. } => "sphere",
            Surf::Torus { .. } => "torus",
            Surf::Nurbs(_) => "b-spline",
        }
    }

    /// Surface point. For NURBS the parameters are clamped to the domain.
    pub fn eval(&self, u: f64, v: f64) -> P3 {
        let f = |x: P3, s: f64| scale3(x, s);
        match self {
            Surf::Plane(fr) => add3(add3(fr.o, f(fr.x, u)), f(fr.y, v)),
            Surf::Cyl { f: fr, r } => add3(add3(fr.o, f(rhat(fr, u), *r)), f(fr.z, v)),
            Surf::Sphere { f: fr, r } => add3(
                add3(fr.o, f(rhat(fr, u), r * v.cos())),
                f(fr.z, r * v.sin()),
            ),
            Surf::Cone { f: fr, r, k } => {
                let rho = r + k * v;
                add3(add3(fr.o, f(rhat(fr, u), rho)), f(fr.z, v))
            }
            Surf::Torus { f: fr, big_r, r } => {
                let w = big_r + r * v.cos();
                add3(add3(fr.o, f(rhat(fr, u), w)), f(fr.z, r * v.sin()))
            }
            Surf::Nurbs(ns) => {
                let (u, v) = clamp_nurbs_domain(ns, u, v);
                nurbs_eval(ns, u, v)
            }
        }
    }

    /// Natural (parameterisation) normal, unit length, pointing the way
    /// `x_u × x_v` does. `same_sense` flips it for the material side.
    pub fn n_nat(&self, u: f64, v: f64) -> P3 {
        match self {
            Surf::Plane(fr) => fr.z,
            Surf::Cyl { f: fr, .. } => rhat(fr, u),
            Surf::Cone { f: fr, k: kk, .. } => {
                let kk = *kk;
                let d = (1.0 + kk * kk).sqrt();
                scale3(sub(rhat(fr, u), f_scale(fr.z, kk)), 1.0 / d)
            }
            Surf::Sphere { f: fr, .. } | Surf::Torus { f: fr, .. } => {
                add3(f_scale(rhat(fr, u), v.cos()), f_scale(fr.z, v.sin()))
            }
            Surf::Nurbs(ns) => {
                let (u, v) = clamp_nurbs_domain(ns, u, v);
                let h = nurbs_step(ns);
                let p = |du: f64, dv: f64| nurbs_eval(ns, u + du, v + dv);
                unit_or(
                    cross3(sub(p(h, 0.0), p(-h, 0.0)), sub(p(0.0, h), p(0.0, -h))),
                    [0.0, 0.0, 1.0],
                )
            }
        }
    }

    /// Area element √g = |x_u × x_v| (mm²).
    pub fn sqrt_g(&self, u: f64, v: f64) -> f64 {
        match self {
            Surf::Plane(_) => 1.0,
            Surf::Cyl { r, .. } => *r,
            Surf::Cone { r, k, .. } => (r + k * v) * (1.0 + k * k).sqrt(),
            Surf::Sphere { r, .. } => r * r * v.cos(),
            Surf::Torus { big_r, r, .. } => (*big_r + *r * v.cos()).abs() * *r,
            Surf::Nurbs(ns) => {
                let (u, v) = clamp_nurbs_domain(ns, u, v);
                let h = nurbs_step(ns);
                // fd differences ÷ (2h) each: true partials, so the cross
                // magnitude is |x_u × x_v| and not 4h² times it
                let x_u = scale3(
                    sub(nurbs_eval(ns, u + h, v), nurbs_eval(ns, u - h, v)),
                    0.5 / h,
                );
                let x_v = scale3(
                    sub(nurbs_eval(ns, u, v + h), nurbs_eval(ns, u, v - h)),
                    0.5 / h,
                );
                len3(cross3(x_u, x_v))
            }
        }
    }

    /// u period (radians for the surfaces of revolution; 0 = not periodic).
    pub fn period_u(&self) -> f64 {
        match self {
            Surf::Cyl { .. } | Surf::Sphere { .. } | Surf::Torus { .. } | Surf::Cone { .. } => {
                std::f64::consts::TAU
            }
            _ => 0.0,
        }
    }

    /// v period (the torus tube direction; 0 = not periodic).
    pub fn period_v(&self) -> f64 {
        match self {
            Surf::Torus { .. } => std::f64::consts::TAU,
            _ => 0.0,
        }
    }

    /// Does (u, v) sit at a singular point — where the u-rate collapses to
    /// zero (a cone's apex, a sphere's poles, a horn torus)? There every u
    /// maps to the same 3D point, so the surface's branch at that parameter
    /// is arbitrary and only the v is meaningful.
    pub fn singular_pt(&self, uv: P2) -> bool {
        match self {
            Surf::Cone { r, k, .. } => (r + k * uv[1]).abs() <= 1e-7,
            Surf::Sphere { r, .. } => *r * uv[1].cos().abs() <= 1e-7,
            Surf::Torus { big_r, r, .. } => (*big_r + *r * uv[1].cos()).abs() <= 1e-7,
            _ => false,
        }
    }

    /// Antiderivative `Q(u,v) = ∫ f du` of `f = x·(x_u × x_v)`: the boundary
    /// flux ∮ Q·dv on the (u,v) ring integrates `f` over the trimmed region
    /// (P = 0). `None` where no closed form is available (B-spline faces).
    pub fn flux_q(&self, u: f64, v: f64) -> Option<f64> {
        let (su, cu) = (u.sin(), u.cos());
        match self {
            Surf::Plane(fr) => Some(dot3(fr.o, fr.z) * u),
            Surf::Cyl { f: fr, r } => {
                let (ax, ay) = (dot3(fr.o, fr.x), dot3(fr.o, fr.y));
                Some(*r * (ax * su - ay * cu) + r * r * u)
            }
            Surf::Cone { f: fr, r, k } => {
                let rho = *r + *k * v;
                let (ax, ay, az) = (dot3(fr.o, fr.x), dot3(fr.o, fr.y), dot3(fr.o, fr.z));
                Some(rho * (ax * su - ay * cu) - rho * *k * az * u + rho * *r * u)
            }
            Surf::Sphere { f: fr, r } => {
                let (cv, sv) = (v.cos(), v.sin());
                let (ax, ay, az) = (dot3(fr.o, fr.x), dot3(fr.o, fr.y), dot3(fr.o, fr.z));
                Some(r * r * (cv * cv * (ax * su - ay * cu) + u * az * sv * cv + u * r * cv))
            }
            Surf::Torus { f: fr, big_r, r } => {
                let w = *big_r + *r * v.cos();
                let (cv, sv) = (v.cos(), v.sin());
                let (ax, ay, az) = (dot3(fr.o, fr.x), dot3(fr.o, fr.y), dot3(fr.o, fr.z));
                Some(
                    *r * w * (cv * (ax * su - ay * cu) + az * u * sv)
                        + u * (w * w * *r * cv + *r * w * *r * sv * sv),
                )
            }
            Surf::Nurbs(_) => None,
        }
    }

    /// Largest chord step in u that keeps the sagitta within `chord` on the
    /// arcs spanned by the v-range (mm); ∞ where the u-direction is straight.
    pub fn u_step(&self, v_lo: f64, v_hi: f64, chord: f64) -> f64 {
        let ang = |radius: f64| 2.0 * (1.0 - (chord / radius).clamp(-1.0, 1.0)).acos();
        match self {
            Surf::Plane(_) | Surf::Nurbs(_) => f64::INFINITY,
            Surf::Cyl { r, .. } => ang(*r),
            Surf::Cone { r, k, .. } => {
                let mut rho_max = 0.0f64;
                for i in 0..=64 {
                    let v = v_lo + (v_hi - v_lo) * (i as f64) / 64.0;
                    rho_max = rho_max.max((r + k * v).abs());
                }
                if rho_max <= 0.0 {
                    f64::INFINITY
                } else {
                    ang(rho_max)
                }
            }
            Surf::Sphere { r, .. } => ang(*r),
            Surf::Torus { big_r, r, .. } => ang(*big_r + *r),
        }
    }

    /// Largest chord step in v (mm); ∞ where the rulings are straight.
    pub fn v_step(&self, chord: f64) -> f64 {
        let ang = |radius: f64| 2.0 * (1.0 - (chord / radius).clamp(-1.0, 1.0)).acos();
        match self {
            Surf::Plane(_) | Surf::Cyl { .. } | Surf::Cone { .. } | Surf::Nurbs(_) => f64::INFINITY,
            Surf::Sphere { r, .. } => ang(*r),
            Surf::Torus { r, .. } => ang(*r),
        }
    }

    /// Invert a point (mm) to (u, v). Exact for the analytic forms; Newton
    /// with a seed grid for B-splines. `None` when the point is not on the
    /// surface within `tol`.
    pub fn invert(&self, p: P3, tol: f64) -> Option<P2> {
        match self {
            Surf::Plane(fr) => {
                let d = sub(p, fr.o);
                let (u, v) = (dot3(d, fr.x), dot3(d, fr.y));
                let resid = len3(sub(d, add3(scale3(fr.x, u), scale3(fr.y, v))));
                (resid <= tol).then_some([u, v])
            }
            Surf::Cyl { f: fr, r } => {
                let d = sub(p, fr.o);
                let rho = (dot3(d, fr.x) * dot3(d, fr.x) + dot3(d, fr.y) * dot3(d, fr.y)).sqrt();
                if (rho - r).abs() > tol {
                    return None;
                }
                Some([dot3(d, fr.y).atan2(dot3(d, fr.x)), dot3(d, fr.z)])
            }
            Surf::Cone { f: fr, r, k } => {
                let d = sub(p, fr.o);
                let v = dot3(d, fr.z);
                let rho = (dot3(d, fr.x) * dot3(d, fr.x) + dot3(d, fr.y) * dot3(d, fr.y)).sqrt();
                if (rho - (r + k * v)).abs() > tol {
                    return None;
                }
                Some([dot3(d, fr.y).atan2(dot3(d, fr.x)), v])
            }
            Surf::Sphere { f: fr, r } => {
                let d = sub(p, fr.o);
                let n = len3(d);
                if (n - r).abs() > tol {
                    return None;
                }
                Some([
                    dot3(d, fr.y).atan2(dot3(d, fr.x)),
                    (dot3(d, fr.z) / n).clamp(-1.0, 1.0).asin(),
                ])
            }
            Surf::Torus { f: fr, big_r, r } => {
                let d = sub(p, fr.o);
                let (dx, dy) = (dot3(d, fr.x), dot3(d, fr.y));
                let w = (dx * dx + dy * dy).sqrt() - big_r;
                let n = (w * w + dot3(d, fr.z) * dot3(d, fr.z)).sqrt();
                if (n - r).abs() > tol {
                    return None;
                }
                Some([dy.atan2(dx), dot3(d, fr.z).atan2(w)])
            }
            Surf::Nurbs(ns) => nurbs_invert(ns, p, tol),
        }
    }
}

fn rhat(fr: &Frame, u: f64) -> P3 {
    add3(scale3(fr.x, u.cos()), scale3(fr.y, u.sin()))
}
fn f_scale(a: P3, s: f64) -> P3 {
    scale3(a, s)
}

fn clamp_nurbs_domain(ns: &NurbsSurface, u: f64, v: f64) -> (f64, f64) {
    let (ul, uh) = nurbs_domain(&ns.knots_u, ns.degree_u);
    let (vl, vh) = nurbs_domain(&ns.knots_v, ns.degree_v);
    (u.clamp(ul, uh), v.clamp(vl, vh))
}

fn nurbs_domain(knots: &[f64], degree: usize) -> (f64, f64) {
    let n = knots.len();
    (knots[degree], knots[n - degree - 1])
}

/// Step size for NURBS finite-difference partials.
fn nurbs_step(ns: &NurbsSurface) -> f64 {
    let (ul, uh) = nurbs_domain(&ns.knots_u, ns.degree_u);
    let (vl, vh) = nurbs_domain(&ns.knots_v, ns.degree_v);
    ((uh - ul).abs().max((vh - vl).abs()) * 1e-6).clamp(1e-12, 1e-3)
}

// ---------------------------------------------------------------------------
// NURBS evaluation (deBoor over homogeneous 4-vectors)
// ---------------------------------------------------------------------------

/// 4-vectors (x, y, z, w).
type P4 = [f64; 4];

fn deboor_span(knots: &[f64], degree: usize, n: usize, t: f64) -> usize {
    if t >= knots[n] {
        return n - 1;
    }
    if t <= knots[degree] {
        return degree;
    }
    let mut i = degree;
    while i + 1 < n && t >= knots[i + 1] {
        i += 1;
    }
    i
}

/// deBoor over (point, weight) pairs; with unit weights this is the plain
/// (non-rational) recursion.
fn deboor_4(pts: &[P4], knots: &[f64], degree: usize, t: f64) -> P4 {
    let n = pts.len();
    let span = deboor_span(knots, degree, n, t);
    let mut p: Vec<P4> = (0..=degree).map(|j| pts[span - degree + j]).collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = span - degree + j;
            let denom = knots[i + degree - r + 1] - knots[i];
            let alpha = (t - knots[i]) / denom;
            p[j] = [
                p[j][0] * alpha + p[j - 1][0] * (1.0 - alpha),
                p[j][1] * alpha + p[j - 1][1] * (1.0 - alpha),
                p[j][2] * alpha + p[j - 1][2] * (1.0 - alpha),
                p[j][3] * alpha + p[j - 1][3] * (1.0 - alpha),
            ];
        }
    }
    p[degree]
}

/// One-sided deBoor at the domain end (the recursion above is exact there for
/// clamped knots; kept for symmetry/readability of callers).
fn deboor_3(pts: &[[f64; 3]], knots: &[f64], degree: usize, t: f64) -> [f64; 3] {
    let span = deboor_span(knots, degree, pts.len(), t);
    let mut p: Vec<[f64; 3]> = (0..=degree).map(|j| pts[span - degree + j]).collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = span - degree + j;
            let alpha = (t - knots[i]) / (knots[i + degree - r + 1] - knots[i]);
            p[j] = [
                p[j][0] * alpha + p[j - 1][0] * (1.0 - alpha),
                p[j][1] * alpha + p[j - 1][1] * (1.0 - alpha),
                p[j][2] * alpha + p[j - 1][2] * (1.0 - alpha),
            ];
        }
    }
    p[degree]
}

/// Evaluate a surface at (u, v): a v-pass over the weighted rows' homogeneous
/// points, then a u-pass over the resulting 4D points (weights are carried
/// through the recursion), divide by w at the end. Dividing per pass instead
/// drops the row's weight factor and is wrong wherever weights vary.
pub fn nurbs_eval(ns: &NurbsSurface, u: f64, v: f64) -> P3 {
    let rows: Vec<Vec<P4>> = ns
        .control_points
        .iter()
        .zip(ns.weights.iter())
        .map(|(cps, ws)| {
            cps.iter()
                .zip(ws.iter())
                .map(|(c, w)| [c[0] * w, c[1] * w, c[2] * w, *w])
                .collect()
        })
        .collect();
    let row_pts: Vec<P4> = rows
        .iter()
        .map(|row| deboor_4(row, &ns.knots_v, ns.degree_v, v))
        .collect();
    let h = deboor_4(&row_pts, &ns.knots_u, ns.degree_u, u);
    [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
}

/// NURBS inversion by Newton with a seed grid (central-difference partials,
/// damped 2×2 normal-equation steps).
fn nurbs_invert(ns: &NurbsSurface, p: P3, tol: f64) -> Option<P2> {
    let (ul, uh) = nurbs_domain(&ns.knots_u, ns.degree_u);
    let (vl, vh) = nurbs_domain(&ns.knots_v, ns.degree_v);
    let seeds = 6;
    let mut best: Option<(f64, P2)> = None;
    for i in 0..seeds {
        for j in 0..seeds {
            let u = ul + (uh - ul) * (i as f64 + 0.5) / seeds as f64;
            let v = vl + (vh - vl) * (j as f64 + 0.5) / seeds as f64;
            if let Some((res, uv)) = newton(ns, p, u, v, ul, uh, vl, vh) {
                if best.is_none_or(|(b, _)| res < b) {
                    best = Some((res, uv));
                }
            }
        }
    }
    best.filter(|(res, _)| *res <= tol).map(|(_, uv)| uv)
}

#[allow(clippy::too_many_arguments)]
fn newton(
    ns: &NurbsSurface,
    p: P3,
    mut u: f64,
    mut v: f64,
    ul: f64,
    uh: f64,
    vl: f64,
    vh: f64,
) -> Option<(f64, P2)> {
    let h = nurbs_step(ns);
    let span_u = (uh - ul).abs();
    let span_v = (vh - vl).abs();
    for _ in 0..12 {
        // central differences ÷ (2h): true partials, so Δ below is a
        // parameter-space step and an affine patch solves in one iteration
        let x_u = scale3(
            sub(nurbs_eval(ns, u + h, v), nurbs_eval(ns, u - h, v)),
            0.5 / h,
        );
        let x_v = scale3(
            sub(nurbs_eval(ns, u, v + h), nurbs_eval(ns, u, v - h)),
            0.5 / h,
        );
        let fvec = sub(nurbs_eval(ns, u, v), p);
        let (a, b, c, d) = (
            dot3(x_u, x_u) + 1e-30,
            dot3(x_u, x_v),
            dot3(x_u, x_v),
            dot3(x_v, x_v) + 1e-30,
        );
        let det = a * d - b * c;
        if !det.is_finite() || det.abs() < 1e-18 {
            return None;
        }
        let gu = dot3(fvec, x_u);
        let gv = dot3(fvec, x_v);
        // Solve [a b; b d]·Δ = [gu gv]; damp by clamping the step.
        let du = (d * gu - b * gv) / det;
        let dv = (a * gv - b * gu) / det;
        let step_u = du.clamp(-span_u * 0.2, span_u * 0.2);
        let step_v = dv.clamp(-span_v * 0.2, span_v * 0.2);
        u = (u - step_u).clamp(ul, uh);
        v = (v - step_v).clamp(vl, vh);
    }
    Some((dist3(nurbs_eval(ns, u, v), p), [u, v]))
}

// ---------------------------------------------------------------------------
// edge sampling
// ---------------------------------------------------------------------------

/// A boundary polyline in the edge entity's own direction (start → end).
#[derive(Clone, Debug)]
pub struct EdgePoly {
    pub pts: Vec<P3>,
    /// The edge's start vertex is its end vertex (a closed entity).
    pub entity_closed: bool,
}

/// Sample a boundary curve to a polyline whose every sub-edge deviates from
/// the curve by at most the chord (sagitta), with an angle guard against
/// shallow S-shapes.
pub fn sample_edge(curve: &NurbsCurve, tgt: f64) -> Result<EdgePoly, String> {
    let deg = curve.degree;
    if curve.control_points.len() < deg + 1
        || curve.knots.len() != curve.control_points.len() + deg + 1
    {
        return Err(format!(
            "edge curve inconsistent ({} control points, degree {}, {} knots)",
            curve.control_points.len(),
            deg,
            curve.knots.len()
        ));
    }
    if deg > 20 {
        return Err(format!(
            "edge curve degree {} is beyond support (max 20)",
            deg
        ));
    }
    if curve.knots.windows(2).any(|w| w[1] < w[0] - 1e-12) {
        return Err("edge curve knots are not non-decreasing".to_owned());
    }
    let t0 = curve.knots[deg];
    let t1 = curve.knots[curve.knots.len() - deg - 1];
    if t1 <= t0 {
        return Err("edge curve has zero-length parameter domain".to_owned());
    }
    let pts_w: Vec<P4> = curve
        .control_points
        .iter()
        .zip(curve.weights.iter())
        .map(|(c, w)| [c[0] * w, c[1] * w, c[2] * w, *w])
        .collect();
    let eval = |t: f64| {
        let h = deboor_4(&pts_w, &curve.knots, deg, t);
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    };
    let p0 = eval(t0);
    let p1 = eval(t1);
    let mut out = vec![p0];
    let sink = |out: &mut Vec<P3>, t: f64| {
        let pm = eval(t);
        if out
            .last()
            .is_none_or(|last| dist3(*last, pm) > 1e-12 * (1.0 + len3(pm)))
        {
            out.push(pm);
        }
    };
    subdivide(t0, t1, p0, p1, 0, &eval, tgt, &mut out, &sink, 20_000)?;
    out.push(p1);
    let close_tol = 1e-9 * (1.0 + len3(p0).max(len3(p1)));
    Ok(EdgePoly {
        entity_closed: dist3(p0, p1) <= close_tol,
        pts: out,
    })
}

#[allow(clippy::too_many_arguments)]
fn subdivide(
    a: f64,
    b: f64,
    pa: P3,
    pb: P3,
    depth: u32,
    eval: &dyn Fn(f64) -> P3,
    tgt: f64,
    out: &mut Vec<P3>,
    sink: &dyn Fn(&mut Vec<P3>, f64),
    cap: usize,
) -> Result<(), String> {
    if out.len() > cap {
        return Err("edge curve needs more than 20000 samples at this chord".to_owned());
    }
    if depth >= 24 {
        return Ok(());
    }
    let m = 0.5 * (a + b);
    let pm = eval(m);
    let dev = dist3(pm, scale3(add3(pa, pb), 0.5));
    let d1 = sub(pm, pa);
    let d2 = sub(pb, pm);
    let turn = (dot3(d1, d2) / (len3(d1) * len3(d2) + 1e-300))
        .clamp(-1.0, 1.0)
        .acos();
    if dev <= tgt && turn <= 0.35 {
        return Ok(());
    }
    subdivide(a, m, pa, pm, depth + 1, eval, tgt, out, sink, cap)?;
    sink(out, m);
    subdivide(m, b, pm, pb, depth + 1, eval, tgt, out, sink, cap)?;
    Ok(())
}

/// Straight-through deBoor on an already-expanded control polygon (used by
/// tests on analytic conversions).
pub fn curve_eval(curve: &NurbsCurve, t: f64) -> [f64; 3] {
    let n = curve.control_points.len();
    let deg = curve.degree;
    deboor_3(
        &curve.control_points,
        &curve.knots,
        deg.min(n.saturating_sub(1)),
        t,
    )
}

// ---------------------------------------------------------------------------
// rings
// ---------------------------------------------------------------------------

/// A boundary loop in inverted and unrolled surface parameters.
#[derive(Clone, Debug)]
pub struct Ring {
    /// 3D points, one per parameter point, open in uv (the polygon closes
    /// implicitly; a 3D-closed ring keeps its last entry when the parameter
    /// closure still carries the window's far edge).
    pub pts: Vec<P3>,
    /// (u, v) of the same points; periodic directions are unrolled so the
    /// polygon is continuous across any seam.
    pub uv: Vec<P2>,
    pub is_outer: bool,
}

/// One oriented edge occurrence in a bound's loop: the sampled polyline (in
/// the edge entity's own start→end direction) plus the traversal flag, and
/// how many occurrences of the same edge entity this loop has already seen
/// (0 = first).
#[derive(Clone, Debug)]
pub struct RingPart {
    pub poly: EdgePoly,
    pub fwd: bool,
    pub occ: u32,
}

/// Assemble one boundary loop: walk the oriented edge polylines with a
/// continuity check, invert each point to (u, v), and unroll periodic
/// parameters by shortest-path continuation. A loop made of exactly two
/// occurrences of one closed-direction seam edge gets its second occurrence
/// offset by a full period in that direction, which turns the loop into the
/// trimmed window of a full-period face (whole tube / whole v-slab).
///
/// Two tolerances do separate jobs. `join_tol` gates the 3D continuity walk:
/// consecutive edge samples come from topological vertices, so a gap there
/// means a mis-walked bound, and it is kept tight. `inv_tol` gates the surface
/// inversion of each sample: points are vertices of a polyline that lies on
/// the *file's* curve, and when that curve approximates a 3D intersection
/// (cylinder × cylinder, say) its interior sits off the analytic surface by
/// the authoring system's own curve tolerance. Accepting up to that distance
/// is not a weakening — it is the same chord fidelity the tessellation names,
/// and the inversion returns the nearest surface point, whose flux differs
/// from the off-surface vertex by O(δ/Area) — far below the 1 % volume gate.
///
/// Periodic continuation is 3D-neutral: two candidate placements one period
/// apart evaluate identically, so a delta landing within noise of exactly half
/// a period cannot be disambiguated against the surface (and the two outcomes
/// differ only in window position). This is deliberate — it is not attempted.
pub fn assemble_ring(
    surf: &Surf,
    parts: &[RingPart],
    is_outer: bool,
    join_tol: f64,
    inv_tol: f64,
) -> Result<Ring, String> {
    // 3D walk with continuity checks; per-part ranges into the merged list.
    let mut pts: Vec<P3> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        let seg: Vec<P3> = if part.fwd {
            part.poly.pts.clone()
        } else {
            part.poly.pts.iter().rev().copied().collect()
        };
        if seg.is_empty() {
            return Err(format!("edge {} of the bound contributes no points", i));
        }
        let skip = if pts.is_empty() {
            0
        } else {
            let d = dist3(*pts.last().unwrap(), seg[0]);
            if d > join_tol {
                return Err(format!(
                    "boundary loop is not continuous between edges {} and {} (gap {})",
                    i - 1,
                    i,
                    d
                ));
            }
            1
        };
        let start = pts.len();
        pts.extend(seg.iter().skip(skip).copied());
        ranges.push((start, pts.len()));
    }
    if pts.len() < 3 {
        return Err("boundary loop has fewer than 3 points".to_owned());
    }
    // invert everything (parameters, not yet unwrapped)
    let mut raw: Vec<P2> = Vec::with_capacity(pts.len());
    for (i, p) in pts.iter().enumerate() {
        raw.push(surf.invert(*p, inv_tol).ok_or_else(|| {
            format!(
                "boundary point {i} does not lie on the {} surface within {inv_tol} (point {p:?})",
                surf.kind_name()
            )
        })?);
    }
    let pu = surf.period_u();
    let pv = surf.period_v();
    // unroll: shortest-path continuation per periodic direction
    let mut uv: Vec<P2> = Vec::with_capacity(pts.len());
    let short = |prev: f64, x: f64, period: f64| -> f64 {
        let half = period * 0.5;
        let mut delta = x - prev;
        while delta > half {
            delta -= period;
        }
        while delta < -half {
            delta += period;
        }
        prev + delta
    };
    // A singular sample (a sphere pole) carries no u branch: every u maps to
    // the same 3D point, so the inversion's u there is arbitrary. Unwrapping
    // the samples after it from that arbitrary branch drags the whole segment
    // onto the window's far side — CTC-02's quarter-sphere caps unrolled
    // across the pole into a full-period window that also covered the band
    // of longitudes opposite the face, and the refinement then bisected the
    // wrap-crossing chords to the wrong end of the sphere, round after round.
    // Continue from the last non-singular u instead; the singularity pass
    // below rebranches the singular itself and bridges the missing branch.
    let mut u_anchor: Option<f64> = None;
    for (i, r) in raw.iter().enumerate() {
        let q = if i == 0 {
            *r
        } else {
            let prev = uv[i - 1];
            [
                if pu > 0.0 {
                    match u_anchor {
                        Some(a) => short(a, r[0], pu),
                        None => short(prev[0], r[0], pu),
                    }
                } else {
                    r[0]
                },
                if pv > 0.0 {
                    short(prev[1], r[1], pv)
                } else {
                    r[1]
                },
            ]
        };
        if !surf.singular_pt(*r) {
            u_anchor = Some(q[0]);
        }
        uv.push(q);
    }
    // seam rule: exactly two occurrences of one edge entity, both flat in one
    // periodic direction (raw and placed spans of the second occurrence ≈ 0):
    // push the second occurrence one period over, so the loop spans the
    // trimmed window (full-period face).
    if parts.len() == 2 && parts[0].occ == 0 && parts[1].occ == 1 {
        let (s1, e1) = ranges[1];
        if e1 > s1 {
            let flat = |k: usize, period: f64| -> bool {
                if period <= 0.0 {
                    return false;
                }
                let mut lo = f64::INFINITY;
                let mut hi = f64::NEG_INFINITY;
                let mut plo = lo;
                let mut phi = hi;
                for i in s1..e1 {
                    lo = lo.min(raw[i][k]);
                    hi = hi.max(raw[i][k]);
                    plo = plo.min(uv[i][k]);
                    phi = phi.max(uv[i][k]);
                }
                (hi - lo) < 1e-6 && (phi - plo) < 1e-6
            };
            if flat(0, pu) {
                for uv_i in uv[s1..e1].iter_mut() {
                    uv_i[0] += pu;
                }
                // The walk dropped occ1's first sample (same 3D point as
                // occ0's tail), so the u-jump between the unshifted and the
                // shifted column would be a diagonal. Re-insert the bridge:
                // occ0's tail carried one period over.
                uv.insert(s1, [uv[s1 - 1][0] + pu, uv[s1 - 1][1]]);
                pts.insert(s1, pts[s1 - 1]);
            } else if flat(1, pv) {
                for uv_i in uv[s1..e1].iter_mut() {
                    uv_i[1] += pv;
                }
                uv.insert(s1, [uv[s1 - 1][0], uv[s1 - 1][1] + pv]);
                pts.insert(s1, pts[s1 - 1]);
            }
        }
    }
    // closure alignment: a 3D-closed ring whose uv closure misses by an exact
    // number of periods means the walk started on the far branch of a seam
    // point (a boundary point shared with an edge that continues across it,
    // whose atan2 inversion picked the other representation). Shift uv[0]
    // into the branch the walk actually used — the alternative copies are
    // 3D-identical, so no geometry changes. Windows spanning a full period
    // (the seam-rule tubes above) are excluded: their wrap entry is the
    // window itself, kept on purpose, and must not be closed.
    {
        // span over uv[1..len-1] — the walk's interior. Excluding the closing
        // last entry keeps tubes (whose span comes from interior seam
        // bridges) distinguishable from a seam-ray start whose -period and
        // +period copies are only in the endpoints.
        let span_full = |k: usize, period: f64| -> bool {
            if period <= 0.0 || uv.len() < 3 {
                return false;
            }
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for q in uv[1..uv.len() - 1].iter() {
                lo = lo.min(q[k]);
                hi = hi.max(q[k]);
            }
            hi - lo >= period - 1e-6
        };
        let full_u = span_full(0, surf.period_u());
        let full_v = span_full(1, surf.period_v());
        let periods = [
            (0usize, surf.period_u(), full_u),
            (1usize, surf.period_v(), full_v),
        ];
        for (k, p, full) in periods {
            if p <= 0.0 || full {
                continue;
            }
            let delta = uv[uv.len() - 1][k] - uv[0][k];
            let n = (delta / p).round();
            if n != 0.0
                && (delta - n * p).abs() <= 1e-6
                && pts.len() >= 2
                && dist3(*pts.last().unwrap(), pts[0]) <= join_tol
            {
                // Two fixes fit: hoist uv[0] one period onto the branch the
                // walk actually used, or pull the closing sample one period
                // back onto uv[0]'s branch. The interior samples decide. The
                // unwrap ties consecutive seam samples at ±half-period: when
                // the closing samples flipped past the branch the interior
                // lives on (NIST CTC-02's half-drill-cone rims: bbox span
                // 5.906 of 2π), hoisting uv[0] drags the polygon's opening
                // across the seam and ears the mirrored half. Pulling the
                // tail keeps every interior sample put. Hoist only wins when
                // uv[0] itself is the far-branch outlier (the walk's end
                // branch holds the interior) — the wider pulled window says
                // so.
                let mut blo = f64::INFINITY;
                let mut bhi = f64::NEG_INFINITY;
                for q in uv[1..uv.len() - 1].iter() {
                    if surf.singular_pt(*q) {
                        continue;
                    }
                    blo = blo.min(q[k]);
                    bhi = bhi.max(q[k]);
                }
                if !blo.is_finite() {
                    uv[0][k] += n * p;
                    continue;
                }
                let w_hoist = (bhi.max(uv[0][k] + n * p)) - (blo.min(uv[0][k] + n * p));
                let w_pull = (bhi.max(uv[0][k])) - (blo.min(uv[0][k]));
                if w_pull <= w_hoist + 1e-12 {
                    let last = uv.len() - 1;
                    uv[last][k] -= n * p;
                } else {
                    uv[0][k] += n * p;
                }
            }
        }
    }
    // singular points: where the surface's u-rate collapses to zero (a cone's
    // apex, a sphere's poles, a horn torus) every u maps to one 3D point, so
    // the inversion's branch there is arbitrary and the uv polygon needs the
    // point's in-branch AND out-branch entries to represent the window's
    // degenerate parameter edge. If the placed uv sits on the in-branch (same
    // u as the previous unrolled point), the out entry is missing — insert it
    // after; if it sits on the out-branch, insert the in entry before.
    {
        let eps = 1e-6;
        let sing = |uv_j: P2| -> bool { surf.singular_pt(uv_j) };
        let short_dist = |a: f64, b: f64, period: f64| -> f64 {
            let mut d = a - b;
            if period > 0.0 {
                let half = period * 0.5;
                while d > half {
                    d -= period;
                }
                while d < -half {
                    d += period;
                }
            }
            d.abs()
        };
        let mut bridge: Vec<(usize, usize, P2)> = Vec::new(); // (insert_at, singular_idx, uv)
        let len = uv.len();
        let pu = surf.period_u();
        // rebranch: at a singularity the inversion's u is arbitrary. When it
        // matches neither the previous nor the next unrolled sample's u, the uv
        // polygon loses its true window there and the meshed face opens along
        // the wedge. Re-point the singularity's u at the nearest non-singular
        // neighbour's branch (previous preferred, then next; walk order so
        // consecutive singular entries cascade); the bridge pass below then
        // adds the missing branch entry.
        for j in 0..len {
            if !sing(uv[j]) {
                continue;
            }
            let u_in = uv[(j + len - 1) % len][0];
            let u_out = uv[(j + 1) % len][0];
            let (d_in, d_out) = if pu > 0.0 {
                (
                    short_dist(uv[j][0], u_in, pu),
                    short_dist(uv[j][0], u_out, pu),
                )
            } else {
                ((uv[j][0] - u_in).abs(), (uv[j][0] - u_out).abs())
            };
            if d_in <= eps || d_out <= eps {
                // mod-period the entry already matches a neighbour's branch —
                // but when the unwrap tied the closing seam samples onto the
                // walk's exit branch, the stored u sits a whole period off
                // that neighbour (CTC-02's half-drill-cone apexes): the uv
                // polygon's closing edge then sweeps the far half. Repoint to
                // the neighbour's branch — same 3D point either way.
                if d_in <= eps && eps < (uv[j][0] - u_in).abs() {
                    uv[j][0] = u_in;
                } else if d_out <= eps && eps < (uv[j][0] - u_out).abs() {
                    uv[j][0] = u_out;
                }
                continue;
            }
            if !sing(uv[(j + len - 1) % len]) {
                uv[j][0] = u_in;
            } else if !sing(uv[(j + 1) % len]) {
                uv[j][0] = u_out;
            }
            // both neighbours singular as well: rare, leave as-is
        }
        // a closed loop through the singularity can already hold BOTH branches
        // (the walk carries the singular point at its two loop ends). Inserting
        // the branch entry again would re-walk the polygon's (u,v) edge and
        // double-cover it, so the insertion below skips when the entry's (u,v)
        // is already present.
        let _ = pu;
        for j in 0..len {
            if !sing(uv[j]) {
                continue;
            }
            let u_in = uv[(j + len - 1) % len][0];
            let u_out = uv[(j + 1) % len][0];
            let (d_in, d_out) = if pu > 0.0 {
                (
                    short_dist(uv[j][0], u_in, pu),
                    short_dist(uv[j][0], u_out, pu),
                )
            } else {
                ((uv[j][0] - u_in).abs(), (uv[j][0] - u_out).abs())
            };
            let same_pt = |k: usize, q: P2| {
                dist3(pts[k], pts[j]) <= join_tol * 1e-3
                    && short_dist(uv[k][0], q[0], pu) <= eps
                    && (uv[k][1] - q[1]).abs() <= eps
            };
            if d_in <= eps && d_out > eps {
                let other = |k: usize| k != j && same_pt(k, [u_out, uv[j][1]]);
                if !(0..len).any(other) {
                    bridge.push((j + 1, j, [u_out, uv[j][1]]));
                }
            } else if d_out <= eps && d_in > eps {
                let other = |k: usize| k != j && same_pt(k, [u_in, uv[j][1]]);
                if !(0..len).any(other) {
                    bridge.push((j, j, [u_in, uv[j][1]]));
                }
            }
        }
        for (j, src, q) in bridge.iter().rev() {
            uv.insert(*j, *q);
            pts.insert(*j, pts[*src]);
        }
    }
    // consecutive identical (3D and parameter) points out; a trailing closure
    // that is identical in BOTH 3D and parameters is the loop's last point
    // repeated and goes too. A 3D-closed ring whose parameter closure spans
    // the window (u-periodic tubes) keeps its last entry.
    let same3 = |a: P3, b: P3| dist3(a, b) <= join_tol * 1e-3;
    let same2 = |a: P2, b: P2| {
        (a[0] - b[0]).abs() <= 1e-9_f64.max((a[0] + b[0]).abs() * 1e-9)
            && (a[1] - b[1]).abs() <= 1e-9_f64.max((a[1] + b[1]).abs() * 1e-9)
    };
    if uv.len() >= 2 && same3(*pts.last().unwrap(), pts[0]) && same2(*uv.last().unwrap(), uv[0]) {
        pts.pop();
        uv.pop();
    }
    let mut kept_i: Vec<usize> = Vec::with_capacity(uv.len());
    for i in 0..uv.len() {
        if let Some(&j) = kept_i.last() {
            if same3(pts[i], pts[j]) && same2(uv[i], uv[j]) {
                continue;
            }
        }
        kept_i.push(i);
    }
    if kept_i.len() < 3 {
        return Err("boundary loop degenerates to fewer than 3 distinct points".to_owned());
    }
    let fpts: Vec<P3> = kept_i.iter().map(|&i| pts[i]).collect();
    let fuv: Vec<P2> = kept_i.iter().map(|&i| uv[i]).collect();
    Ok(Ring {
        pts: fpts,
        uv: fuv,
        is_outer,
    })
}

// ---------------------------------------------------------------------------
// vertex pool
// ---------------------------------------------------------------------------

/// Shared vertex pool with quantised deduplication (seam wraps and poles
/// produce near-identical 3D points from different parameters).
pub struct VertexPool {
    q: f64,
    map: HashMap<[i64; 3], u32>,
    pub verts: Vec<P3>,
}

impl VertexPool {
    pub fn new(diag: f64) -> Self {
        Self {
            q: (diag.abs() * 1e-9).clamp(1e-9, 1e-4),
            map: HashMap::new(),
            verts: Vec::new(),
        }
    }

    pub fn insert(&mut self, p: P3) -> u32 {
        let key = [
            (p[0] / self.q).round() as i64,
            (p[1] / self.q).round() as i64,
            (p[2] / self.q).round() as i64,
        ];
        if let Some(&i) = self.map.get(&key) {
            return i;
        }
        let i = self.verts.len() as u32;
        self.verts.push(p);
        self.map.insert(key, i);
        i
    }
}

// ---------------------------------------------------------------------------
// face tessellation
// ---------------------------------------------------------------------------

/// One face's contribution: triangles into the shared pool, plus the
/// reference quadratures.
pub struct FaceTess {
    pub tris: Vec<[u32; 3]>,
    /// ∫∫_R √g du dv over the trimmed parameter region (an independent area
    /// reference; analytic for whole spheres / tori).
    pub quad_area: f64,
    /// Σ |A| of the output triangles.
    pub mesh_area: f64,
    /// −(1/3)·s_f·∮ Q dv with the material side `same_sense`; None when the
    /// surface is a B-spline (no flux potential).
    pub volume_face: Option<f64>,
    /// which triangulation path ran (for the report)
    pub path: &'static str,
}

/// Drop a ring's consecutive coincident samples: identical in both 3D
/// (`tol · 1e−3`, the same margin assemble_ring keeps) and parameter
/// (relative 1e−9), plus a trailing closure that repeats the loop's first
/// point in both. assemble_ring dedupes the rings it builds; a ring from
/// any other source can keep both halves of a seam corner whose two
/// entries disagree only at roundoff (0.2·6 vs 0.6 differ by 2e−16 in
/// (u,v)) — and a duplicated corner forks the refinement into a
/// ping-pong: the row-fan branch fans the corner chord through the twin
/// copy, drops the zero-area child it spans, and re-marks the same chord
/// on the surviving child, flipping between the two copies round after
/// round with no progress.
///
/// `tol` is tess_face's own tolerance parameter (the weld/join tolerance in
/// the production caller).
fn dedupe_ring(r: &Ring, tol: f64) -> Option<Ring> {
    let same2 = |a: P2, b: P2| {
        (a[0] - b[0]).abs() <= 1e-9_f64.max((a[0] + b[0]).abs() * 1e-9)
            && (a[1] - b[1]).abs() <= 1e-9_f64.max((a[1] + b[1]).abs() * 1e-9)
    };
    let same3 = |a: P3, b: P3| dist3(a, b) <= tol * 1e-3;
    let mut uv: Vec<P2> = r.uv.clone();
    let mut pts: Vec<P3> = r.pts.clone();
    if uv.len() >= 2 && same3(pts[pts.len() - 1], pts[0]) && same2(uv[uv.len() - 1], uv[0]) {
        uv.pop();
        pts.pop();
    }
    let mut kept_uv: Vec<P2> = Vec::with_capacity(uv.len());
    let mut kept_pts: Vec<P3> = Vec::with_capacity(pts.len());
    for (&q, &p) in uv.iter().zip(pts.iter()) {
        if let Some((&lq, &lp)) = kept_uv.last().zip(kept_pts.last()) {
            if same2(lq, q) && same3(lp, p) {
                continue;
            }
        }
        kept_uv.push(q);
        kept_pts.push(p);
    }
    if kept_uv.len() == uv.len() {
        return None;
    }
    Some(Ring {
        pts: kept_pts,
        uv: kept_uv,
        is_outer: r.is_outer,
    })
}

/// Tessellate one face.
///
/// Triage: no trim curves → whole-sphere / whole-torus grids; a full-period
/// u (or v) span → a structured rectangular grid; otherwise an earcut
/// triangulation of the parameter polygon, refined to the chord.
pub fn tess_face(
    surf: &Surf,
    same_sense: bool,
    rings: &[Ring],
    pool: &mut VertexPool,
    chord: f64,
    tol: f64,
) -> Result<FaceTess, String> {
    // Deduped views of the caller's rings; the borrow is swapped for the
    // deduped owned copy when any dropped a sample.
    let mut changed = false;
    let mut owned: Vec<Ring> = Vec::with_capacity(rings.len());
    for r in rings {
        match dedupe_ring(r, tol) {
            Some(d) => {
                changed = true;
                owned.push(d);
            }
            None => owned.push(r.clone()),
        }
    }
    if !changed {
        owned.clear();
    }
    let rings: &[Ring] = if changed { &owned } else { rings };
    // A ring that collapses below three distinct points has no boundary the
    // triangulator can consume.
    for r in rings {
        if r.uv.len() < 3 {
            return Err("boundary loop degenerates to fewer than 3 distinct points".to_owned());
        }
    }
    let outers: Vec<&Ring> = rings.iter().filter(|r| r.is_outer).collect();
    let holes: Vec<&Ring> = rings.iter().filter(|r| !r.is_outer).collect();
    if rings.is_empty() {
        return tess_unbounded(surf, same_sense, pool, chord, tol);
    }
    if outers.len() != 1 {
        return Err(format!(
            "face has {} outer bounds; exactly one is supported",
            outers.len()
        ));
    }
    let outer = outers[0];
    let w_u = {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for q in outer.uv.iter() {
            lo = lo.min(q[0]);
            hi = hi.max(q[0]);
        }
        hi - lo
    };
    let w_v = {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for q in outer.uv.iter() {
            lo = lo.min(q[1]);
            hi = hi.max(q[1]);
        }
        hi - lo
    };
    let t_u = surf.period_u();
    let t_v = surf.period_v();
    let eps_u = if t_u > 0.0 { t_u * 1e-6 } else { 0.0 };
    let eps_v = if t_v > 0.0 { t_v * 1e-6 } else { 0.0 };
    // Full-period on a periodic axis takes either of two complementary
    // signals. The bbox span under-reads a window the walk covers by the wrap
    // arc (a stored rim can start on a far branch: NIST CTC-01's apex-cone
    // rims record 5.906 of 2π) — the closed-walk advance catches those. The
    // advance under-reads in its turn when the ring traverses the full width
    // and comes back — a two-rim band rectangle walks +period along the
    // bottom and −period along the top and nets zero — and there the bbox
    // span is exact. Either signal alone would refuse a legitimate full
    // window; a partial arc cannot satisfy the width test without being a
    // face's whole outer bound, and steps without 3D extent (NIST CTC-04's
    // apex-to-apex degenerate edge on half-window cones) are excluded from
    // the advance, so the OR has no false positives.
    let adv_u = if t_u > 0.0 {
        ring_axis_advance(&outer.uv, &outer.pts, 0, t_u, tol * 1e-3).abs()
    } else {
        w_u
    };
    let adv_v = if t_v > 0.0 {
        ring_axis_advance(&outer.uv, &outer.pts, 1, t_v, tol * 1e-3).abs()
    } else {
        w_v
    };
    let full_u = t_u > 0.0 && (w_u >= t_u - eps_u || adv_u >= t_u - eps_u);
    let full_v = t_v > 0.0 && (w_v >= t_v - eps_v || adv_v >= t_v - eps_v);
    if full_u || full_v {
        if !holes.is_empty() {
            return Err("full-period face with inner bounds; unsupported".to_owned());
        }
        // A ring that is rectangular in the window's own axes (every trim
        // segment constant-u or constant-v) is cheapest as a structured grid.
        // Any other shape — a planar cut, a swept profile — triangulates as an
        // earcut polygon over the seam-shifted unrolled window; the ring's uv
        // already carries that unroll (closure alignment pushes uv[0] into the
        // walk's branch), so the polygon is a plain 2D region (NIST CTC-02).
        if ring_axis_aligned(&outer.uv) {
            return tess_structured(surf, outer, pool, chord, tol, same_sense, full_u, full_v);
        }
    }
    tess_earcut(surf, outer, &holes, pool, chord, tol, same_sense)
}

/// Signed advance of the ring's closed walk along parameter axis `k`
/// (wrapped per step, the implicit closure included). Steps whose 3D
/// endpoints coincide (`dist <= zero`) contribute nothing: a degenerate
/// edge can traverse the whole parameter width without moving in 3D —
/// NIST CTC-04's half-window cones carry an apex-to-apex edge whose
/// parameter jump is π, and counting it flipped the walk's net advance to
/// a full period on a face whose real window is half of one.
fn ring_axis_advance(uv: &[P2], pts: &[P3], k: usize, period: f64, zero: f64) -> f64 {
    let half = period * 0.5;
    let step = |d: f64| -> f64 {
        let mut d = d;
        while d > half {
            d -= period;
        }
        while d < -half {
            d += period;
        }
        d
    };
    let mut acc = 0.0f64;
    let mut prev = uv[0][k];
    for (i, q) in uv.iter().enumerate().skip(1) {
        if dist3(pts[i - 1], pts[i]) <= zero {
            prev = q[k];
            continue;
        }
        acc += step(q[k] - prev);
        prev = q[k];
    }
    if dist3(pts[pts.len() - 1], pts[0]) > zero {
        acc += step(uv[0][k] - prev);
    }
    acc
}

/// every segment of the ring is constant-u or constant-v
fn ring_axis_aligned(uv: &[P2]) -> bool {
    ring_segments(uv)
        .iter()
        .all(|(a, b)| !((a[0] - b[0]).abs() > 1e-6 && (a[1] - b[1]).abs() > 1e-6))
}

fn ring_segments(uv: &[P2]) -> Vec<(P2, P2)> {
    let n = uv.len();
    (0..n).map(|i| (uv[i], uv[(i + 1) % n])).collect()
}

// --- whole-sphere / whole-torus (no trim curves) ---

fn tess_unbounded(
    surf: &Surf,
    same_sense: bool,
    pool: &mut VertexPool,
    chord: f64,
    _tol: f64,
) -> Result<FaceTess, String> {
    match surf {
        Surf::Sphere { f, r } => {
            let r = *r;
            let theta_v = surf.v_step(chord);
            if !theta_v.is_finite() {
                return Err("sphere without trims has no meridian step".to_owned());
            }
            let rows = (std::f64::consts::PI / theta_v).ceil() as usize;
            let theta_u = surf.u_step(-1.0, 1.0, chord);
            let cols = (std::f64::consts::TAU / theta_u).ceil() as usize;
            bounded_checks(rows, cols)?;
            let north = pool.insert(add3(f.o, scale3(f.z, r)));
            let south = pool.insert(add3(f.o, scale3(f.z, -r)));
            let quad_area = 4.0 * std::f64::consts::PI * r * r;
            let volume_face =
                Some(vol_sign(same_sense) * (4.0 / 3.0) * std::f64::consts::PI * r * r * r);
            let mut row_idx: Vec<Vec<u32>> = Vec::new();
            for i in 0..=rows {
                let v =
                    -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * (i as f64) / rows as f64;
                if i == 0 {
                    row_idx.push(vec![north]);
                    continue;
                }
                if i == rows {
                    row_idx.push(vec![south]);
                    continue;
                }
                let mut row = Vec::with_capacity(cols);
                for j in 0..cols {
                    let u = std::f64::consts::TAU * (j as f64) / cols as f64;
                    row.push(pool.insert(surf.eval(u, v)));
                }
                row_idx.push(row);
            }
            let mut tris: Vec<[u32; 3]> = Vec::new();
            for j in 0..cols {
                tris.push([north, row_idx[1][j], row_idx[1][(j + 1) % cols]]);
                tris.push([
                    south,
                    row_idx[rows - 1][(j + 1) % cols],
                    row_idx[rows - 1][j],
                ]);
            }
            let mut mesh_area = 0.0f64;
            for i in 1..rows - 1 {
                for j in 0..cols {
                    let a = row_idx[i][j];
                    let b = row_idx[i + 1][j];
                    let c = row_idx[i + 1][(j + 1) % cols];
                    let d = row_idx[i][(j + 1) % cols];
                    emit_nonzero(&mut tris, &mut mesh_area, &pool.verts, [a, b, c]);
                    emit_nonzero(&mut tris, &mut mesh_area, &pool.verts, [a, c, d]);
                }
            }
            // pole-fan triangles were never area-accounted; sum from scratch
            mesh_area = tris
                .iter()
                .map(|t| {
                    let (a, b, c) = (
                        pool.verts[t[0] as usize],
                        pool.verts[t[1] as usize],
                        pool.verts[t[2] as usize],
                    );
                    0.5 * len3(cross3(sub(b, a), sub(c, a)))
                })
                .sum();
            Ok(FaceTess {
                tris,
                quad_area,
                mesh_area,
                volume_face,
                path: "sphere grid",
            })
        }
        Surf::Torus { f: _, big_r, r } => {
            let (big_r, r) = (*big_r, *r);
            let theta_v = surf.v_step(chord);
            if !theta_v.is_finite() {
                return Err("torus without trims has no v step".to_owned());
            }
            let rows = (std::f64::consts::TAU / theta_v).ceil() as usize;
            let theta_u = surf.u_step(-1.0, 1.0, chord);
            let cols = (std::f64::consts::TAU / theta_u).ceil() as usize;
            bounded_checks(rows, cols)?;
            let mut tris = Vec::new();
            let quad_area = 4.0 * std::f64::consts::PI * std::f64::consts::PI * big_r * r;
            let volume_face = Some(
                vol_sign(same_sense)
                    * 2.0
                    * std::f64::consts::PI
                    * std::f64::consts::PI
                    * big_r
                    * r
                    * r,
            );
            let mut row_idx: Vec<Vec<u32>> = Vec::new();
            for i in 0..=rows {
                let v = -std::f64::consts::PI + std::f64::consts::TAU * (i as f64) / rows as f64;
                let mut row = Vec::with_capacity(cols);
                for j in 0..cols {
                    let u = std::f64::consts::TAU * (j as f64) / cols as f64;
                    row.push(pool.insert(surf.eval(u, v)));
                }
                row_idx.push(row);
            }
            let mut mesh_area = 0.0f64;
            for i in 0..rows {
                for j in 0..cols {
                    let a = row_idx[i][j];
                    let b = row_idx[i + 1][j];
                    let c = row_idx[i + 1][(j + 1) % cols];
                    let d = row_idx[i][(j + 1) % cols];
                    emit_nonzero(&mut tris, &mut mesh_area, &pool.verts, [a, b, c]);
                    emit_nonzero(&mut tris, &mut mesh_area, &pool.verts, [a, c, d]);
                }
            }
            Ok(FaceTess {
                tris,
                quad_area,
                mesh_area,
                volume_face,
                path: "torus grid",
            })
        }
        _ => Err(format!(
            "face has no trim curves and its {} surface has no whole-surface form",
            surf.kind_name()
        )),
    }
}

fn bounded_checks(rows: usize, cols: usize) -> Result<(), String> {
    if rows > 8192 || cols > 8192 {
        return Err(format!(
            "whole-surface grid needs {}×{} rows×cols; the chord is finer than supported",
            rows, cols
        ));
    }
    Ok(())
}

fn vol_sign(same_sense: bool) -> f64 {
    if same_sense {
        1.0
    } else {
        -1.0
    }
}

// --- structured grids (full-period spans) ---

#[allow(clippy::too_many_arguments)]
fn tess_structured(
    surf: &Surf,
    ring: &Ring,
    pool: &mut VertexPool,
    chord: f64,
    tol: f64,
    same_sense: bool,
    full_u: bool,
    full_v: bool,
) -> Result<FaceTess, String> {
    // every segment axis-aligned
    for (a, b) in ring_segments(&ring.uv) {
        if (a[0] - b[0]).abs() > 1e-6 && (a[1] - b[1]).abs() > 1e-6 {
            return Err(
                "full-period face with a non-axis-aligned trim curve; unsupported".to_owned(),
            );
        }
    }
    let t_u = surf.period_u();
    let t_v = surf.period_v();
    // parameter windows: full directions wrap by one period; partial hulls
    let (u0, u1) = window(ring.uv.iter().map(|q| q[0]), full_u, t_u);
    let (mut v0, mut v1) = window(ring.uv.iter().map(|q| q[1]), full_v, t_v);
    // A cone face whose ring is a single closed constant-v rim (full_u: the
    // wrap entry spans the period) has no v extent in the ring: the face runs
    // from that rim to the apex, the surface's other parameter edge, which is
    // degenerate (one 3D point for every u). One real bound suffices to
    // establish this — a face closing at a second rim would carry that rim as
    // a second bound. Widen the window so the grid spans rim → apex; the apex
    // row collapses into one pooled vertex (NIST CTC-05's drill points).
    let mut extended_v = false;
    if (v1 - v0).abs() <= 1e-9 {
        if let Surf::Cone { r, k, .. } = surf {
            if *k != 0.0 {
                let apex_v = -r / k;
                v0 = v0.min(apex_v);
                v1 = v1.max(apex_v);
                extended_v = true;
            }
        }
    }
    if (u1 - u0) <= 1e-12 || (v1 - v0) <= 1e-12 {
        return Err("full-period face has a degenerate parameter window".to_owned());
    }
    // columns: ring sample u values (normalised into the window per ring) +
    // constant-u boundary values + interior fills by the u chord rule
    let mut cols: Vec<f64> = vec![u0, u1];
    let mut rows: Vec<f64> = vec![v0, v1];
    for (a, b) in ring_segments(&ring.uv) {
        let du = (a[0] - b[0]).abs();
        let dv = (a[1] - b[1]).abs();
        if dv > 1e-6 {
            // constant-u segment: its u is a column, its v samples are rows
            cols.push(a[0]);
            rows.push(a[1]);
            rows.push(b[1]);
        } else {
            rows.push(a[1]);
            // constant-v segment: every sample u is a column
            // (its endpoints' u values)
            cols.push(a[0]);
            cols.push(b[0]);
        }
        let _ = du;
    }
    // normalise all collected values into the windows
    for c in cols.iter_mut() {
        *c = normalise(*c, u0, u1, t_u);
    }
    for r in rows.iter_mut() {
        *r = normalise(*r, v0, v1, t_v);
    }
    densify(&mut cols, surf.u_step(v0, v1, chord));
    densify(&mut rows, surf.v_step(chord));
    dedup_sorted(&mut cols, 1e-9 * (u1 - u0));
    dedup_sorted(&mut rows, 1e-9 * (v1 - v0));
    if cols.len() > 8192 || rows.len() > 8192 {
        return Err(format!(
            "structured face grid needs {}×{}; the chord is finer than supported",
            cols.len(),
            rows.len()
        ));
    }
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut mesh_area = 0.0f64;
    let mut quad_area = 0.0f64;
    let mut row_pts: Vec<P2> = Vec::new();
    let mut row_idx: Vec<Vec<u32>> = Vec::new();
    for v in rows.iter().copied() {
        let mut row = Vec::with_capacity(cols.len());
        for u in cols.iter().copied() {
            row_pts.push([u, v]);
            row.push(pool.insert(surf.eval(u, v)));
        }
        row_idx.push(row);
    }
    let (n_u, n_v) = (cols.len(), rows.len());
    for j in 0..n_v - 1 {
        for i in 0..n_u - 1 {
            let a = row_idx[j][i];
            let b = row_idx[j + 1][i];
            let c = row_idx[j + 1][i + 1];
            let d = row_idx[j][i + 1];
            // quad area by the two parameter triangles with √g at midpoints
            let p00 = row_pts[j * n_u + i];
            let p10 = row_pts[j * n_u + i + 1];
            let p01 = row_pts[(j + 1) * n_u + i];
            let p11 = row_pts[(j + 1) * n_u + i + 1];
            quad_area += param_tri_area(surf, p00, p01, p11);
            quad_area += param_tri_area(surf, p00, p11, p10);
            emit_nonzero(&mut tris, &mut mesh_area, &pool.verts, [a, b, c]);
            emit_nonzero(&mut tris, &mut mesh_area, &pool.verts, [a, c, d]);
        }
    }
    // volume: Green's quadrature over the boundary ring
    let volume_face = if full_u && full_v && matches!(surf, Surf::Torus { .. }) {
        // the single seam line cannot serve as a planar boundary; closed form
        if let Surf::Torus { big_r, r, .. } = surf {
            Some(
                vol_sign(same_sense)
                    * 2.0
                    * std::f64::consts::PI
                    * std::f64::consts::PI
                    * big_r
                    * r
                    * r,
            )
        } else {
            unreachable!()
        }
    } else if full_u && extended_v {
        // the boundary ring does not enclose the window in uv (a rim-only
        // apex cone: the ring is one collinear segment; the window spans rim →
        // apex) — the flux integral goes over the window's two constant-u
        // sides instead, where ∮ Q dv = ∫∫ f du dv still holds exactly
        Some(vol_sign(same_sense) * green_window(surf, u0, u1, v0, v1) / 3.0)
    } else if ring.uv.len() >= 3 {
        let g = green_ring(surf, ring, true);
        Some(vol_sign(same_sense) * g / 3.0)
    } else {
        None
    };
    let _ = tol;
    Ok(FaceTess {
        tris,
        quad_area,
        mesh_area,
        volume_face,
        path: "structured grid",
    })
}

fn window(it: impl Iterator<Item = f64>, full: bool, period: f64) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for x in it {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    if full {
        (lo, lo + period)
    } else {
        (lo, hi)
    }
}

/// Map a value into `[w0, w1]` by whole periods (for the full direction).
fn normalise(x: f64, w0: f64, w1: f64, period: f64) -> f64 {
    if period <= 0.0 {
        return x;
    }
    let span = w1 - w0;
    let mut y = x;
    while y < w0 {
        y += period;
    }
    while y > w0 + span {
        y -= period;
    }
    y
}

fn densify(set: &mut Vec<f64>, step: f64) {
    if !step.is_finite() || step <= 0.0 {
        return;
    }
    set.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut out = Vec::with_capacity(set.len());
    for i in 0..set.len() - 1 {
        let (a, b) = (set[i], set[i + 1]);
        out.push(a);
        let gap = b - a;
        if gap > step {
            let n = (gap / step).floor() as usize;
            for k in 1..=n {
                out.push(a + gap * (k as f64) / (n as f64 + 1.0));
            }
        }
    }
    out.push(*set.last().unwrap());
    *set = out;
}

fn dedup_sorted(set: &mut Vec<f64>, eps: f64) {
    set.sort_by(|a, b| a.partial_cmp(b).unwrap());
    set.dedup_by(|b, a| (*a - *b).abs() <= eps);
}

// --- general (earcut) path ---

/// ring samples (of `lim_lo..lim_hi` in `uv`) lying strictly inside the
/// side's parameter line, in order along it: what a long chord skips. The
/// across tolerance is noise-level: a row's own samples come from as many
/// pcurve representations as the row has boundary edges and disagree across
/// the row by ~1e-10 of the chord (three classes measured on CTC-04), so
/// exact-row tolerance only ever sees one class; a real sample crossing a
/// diagonal chord sits orders above it.
fn ring_skips_between(
    uv: &[P2],
    lim_lo: usize,
    lim_hi: usize,
    x: usize,
    y: usize,
) -> Vec<(f64, u32)> {
    let (a, b) = (uv[x], uv[y]);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let llen = (dx * dx + dy * dy).sqrt();
    if llen == 0.0 {
        return Vec::new();
    }
    let eps = llen * 2e-9;
    let mut pts: Vec<(f64, u32)> = Vec::new();
    for (i, s) in uv.iter().enumerate().take(lim_hi).skip(lim_lo) {
        if i == x || i == y {
            continue;
        }
        let across = (s[0] - a[0]) * dy - (s[1] - a[1]) * dx;
        if (across / llen).abs() > eps {
            continue;
        }
        let along = (s[0] - a[0]) * dx + (s[1] - a[1]) * dy;
        if along <= 0.0 || along >= llen * llen {
            continue;
        }
        pts.push((along, i as u32));
    }
    pts.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap());
    pts
}

/// the ring samples a chord (a, b) skips over, from whichever ring's samples
/// lie on the chord's own parameter line
fn row_chord_skips(uv: &[P2], ranges: &[(usize, usize)], a: usize, b: usize) -> Vec<u32> {
    for &(lim_lo, lim_hi) in ranges {
        let sk = ring_skips_between(uv, lim_lo, lim_hi, a, b);
        if !sk.is_empty() {
            return sk.into_iter().map(|(_, i)| i).collect();
        }
    }
    Vec::new()
}

fn tess_earcut(
    surf: &Surf,
    outer: &Ring,
    holes: &[&Ring],
    pool: &mut VertexPool,
    chord: f64,
    tol: f64,
    same_sense: bool,
) -> Result<FaceTess, String> {
    let mut data: Vec<P2> = outer.uv.clone();
    let mut data_pts: Vec<P3> = outer.pts.clone();
    let mut hole_indices: Vec<u32> = Vec::new();
    for h in holes {
        hole_indices.push(data.len() as u32);
        data.extend(h.uv.iter().copied());
        data_pts.extend(h.pts.iter().copied());
    }
    let mut out: Vec<u32> = Vec::new();
    earcut::Earcut::<f64>::new().earcut(data.iter().copied(), &hole_indices, &mut out);
    // earcut keeps zero-(u,v)-area ears to hold the triangulation together
    // when the boundary has collinear runs — several of its ring samples on
    // one parameter line with roundoff-level spread. On a curved surface
    // those ears are real in 3D: slivers between a long chord and the arc
    // samples it skips, and two ears over the same sample run refine into
    // overlapping slivers — set-identical triangles after the pool weld,
    // four-user edges. Two replacements instead: drop the zero-area ears
    // (they cover no parameter region), and split every kept ear whose side
    // skips ring samples lying on that side's parameter line, fanning the
    // samples to the opposite corner. Boundary samples then all stay mesh
    // vertices, and the refinement sees only short ring-gap chords.
    let mut ring_bases: Vec<usize> = Vec::with_capacity(holes.len() + 1);
    let mut ring_limits: Vec<(usize, usize)> = Vec::with_capacity(ring_bases.capacity());
    {
        let mut base = 0usize;
        for ring in std::iter::once(outer).chain(holes.iter().copied()) {
            ring_bases.push(base);
            ring_limits.push((base, base + ring.uv.len()));
            base += ring.uv.len();
        }
    }
    // ring samples of ANY ring lying strictly inside the side's parameter
    // line, in order along it: what a long chord skips. The check has to be
    // ring-agnostic, not scoped to one endpoint's ring: a chord can run
    // along a parameter line that carries several rings' edges — NIST
    // CTC-04's rib-root plane has hole1's closure edge, the open gap between
    // the ribs, and hole2's bottom edge on one line, and earcut bridges the
    // hole wedges with chords from one hole's corner to the other's, skipping
    // first the near ring's samples, then the far ring's — and the two
    // endpoint-scoped measurements of one chord disagree, so the expansion
    // split the near ring's samples and left the far ring's inside a chord
    // that nothing pairs with.
    let ring_skips = |uv: &[P2], x: usize, y: usize| -> Vec<u32> {
        let mut pts: Vec<(f64, u32)> = Vec::new();
        for &(lim_lo, lim_hi) in &ring_limits {
            pts.extend(ring_skips_between(uv, lim_lo, lim_hi, x, y));
        }
        pts.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap());
        pts.into_iter().map(|(_, i)| i).collect()
    };
    // drop ears that are collinear-degenerate in (u,v) — their cross product
    // is roundoff relative to their own edge lengths, not the face's
    let mut seed: Vec<u32> = Vec::with_capacity(out.len());
    for t in out.chunks_exact(3) {
        let (p0, p1, p2) = (
            data[t[0] as usize],
            data[t[1] as usize],
            data[t[2] as usize],
        );
        let a = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
        let e01 = (p1[0] - p0[0]).hypot(p1[1] - p0[1]);
        let e12 = (p2[0] - p1[0]).hypot(p2[1] - p1[1]);
        let e20 = (p0[0] - p2[0]).hypot(p0[1] - p2[1]);
        // collinear runs of an 18-sample arc row measured ~8e-12 (u,v) spread
        // across a 1.4-long chord; a threshold four orders under the triangle
        // scale still catches that while leaving any real sliver (or a
        // pool-weld hairline) alone
        if a.abs() > 1e-9 * e01.max(e12).max(e20) {
            seed.extend_from_slice(t);
        } else {
            // (u,v)-collinear ear. Keep it when none of its sides skips ring
            // samples: then it is one of the tiling's own bridge triangles
            // and its three sides already have their other users — dropping
            // it is what opens them. Drop only the duplicate-sliver shape (a
            // side over skipped ring samples), which the expansion below
            // re-covers.
            let skippy = [
                (t[0] as usize, t[1] as usize),
                (t[1] as usize, t[2] as usize),
                (t[2] as usize, t[0] as usize),
            ]
            .iter()
            .any(|&(x, y)| !ring_skips(&data, x, y).is_empty());
            if !skippy {
                seed.extend_from_slice(t);
            }
        }
    }
    // expand any remaining seed ear whose side skips ring samples
    let mut out = seed;
    for _ in 0..8 {
        let mut next: Vec<u32> = Vec::with_capacity(out.len());
        let mut expanded = false;
        for t in out.chunks_exact(3) {
            let sides = [
                (t[0] as usize, t[1] as usize, t[2] as usize),
                (t[1] as usize, t[2] as usize, t[0] as usize),
                (t[2] as usize, t[0] as usize, t[1] as usize),
            ];
            let skipping: Vec<(usize, usize, usize, Vec<u32>)> = sides
                .iter()
                .filter_map(|&(x, y, z)| {
                    let skips = ring_skips(&data, x, y);
                    (!skips.is_empty()).then_some((x, y, z, skips))
                })
                .collect();
            if skipping.is_empty() {
                next.extend_from_slice(t);
                continue;
            }
            expanded = true;
            let best = skipping.iter().max_by_key(|s| s.3.len()).unwrap();
            let (x, y, z) = (best.0, best.1, best.2);
            let skips = &best.3;
            next.push(z as u32);
            next.push(x as u32);
            next.push(skips[0]);
            for w in skips.windows(2) {
                next.push(z as u32);
                next.push(w[0]);
                next.push(w[1]);
            }
            next.push(z as u32);
            next.push(*skips.last().unwrap());
            next.push(y as u32);
        }
        out = next;
        if !expanded {
            break;
        }
    }
    if out.is_empty() {
        return Err(format!(
            "earcut produced no triangles for this {} face (degenerate or self-intersecting boundary?)",
            surf.kind_name()
        ));
    }
    // locked boundary edges (local ring indices), never split; boundary
    // points land in the pool so neighbouring faces weld to them. Ring
    // samples pool from their true 3D (the shared edge polyline), not a
    // re-eval of this face's (u,v): two faces' own (u,v) of the same sample
    // can disagree at ~1e-6 mm (pcurve vs curve consistency), straddling a
    // pool bucket, which is exactly an open boundary edge.
    let mut locked: HashSet<(u32, u32)> = HashSet::new();
    for p in data_pts.iter() {
        let _ = pool.insert(*p);
    }
    let all_rings: Vec<&Ring> = std::iter::once(outer)
        .chain(holes.iter().copied())
        .collect();
    // No boundary-deviation check here: every ring segment IS the mesh's
    // boundary edge (locked, never split), so its ends and interior follow the
    // caller's samples of the true edge exactly. An exact straight edge across
    // a curved window (a planar cut on a cone) can bow far from the surface at
    // the uv midpoint while touching the surface at every sample — measuring
    // that bow rejected valid faces (NIST CTC-04). Boundary fidelity to the
    // chord is sample_edge's contract, caller side.
    for (ri, ring) in all_rings.iter().enumerate() {
        let base = ring_bases[ri];
        let n = ring.uv.len();
        for k in 0..n {
            let a = (base + k) as u32;
            let b = (base + (k + 1) % n) as u32;
            locked.insert(sort2(a, b));
        }
    }
    // parameter-space polygon triangulation with refinement
    let (tris_p, quad_area) = refine_eearcut(
        surf,
        &data,
        &data_pts,
        &out,
        &locked,
        &ring_limits,
        chord,
        pool,
    )?;
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut mesh_area = 0.0f64;
    for t in tris_p.iter() {
        let (a, b, c) = (
            pool.verts[t[0] as usize],
            pool.verts[t[1] as usize],
            pool.verts[t[2] as usize],
        );
        let area = 0.5 * len3(cross3(sub(b, a), sub(c, a)));
        if area <= 0.0 {
            continue;
        }
        tris.push(*t);
        mesh_area += area;
    }
    let volume_face = if surf.flux_q(0.0, 0.0).is_some() {
        // green_ring delivers the ring's own algebraic flux with the requested
        // orientation forced: the outer comes back positive (c·A_outer
        // whatever its winding), every hole comes back negative (−c·A_hole,
        // CCW or CW) — so the holes are ADDED, not subtracted; subtracting
        // them double-counts each hole region and inflated every holed
        // face's volume (NIST CTC-01's plate faces by 2·c·A_holes/3 each).
        let g = green_ring(surf, outer, true)
            + holes
                .iter()
                .map(|h| green_ring(surf, h, false))
                .sum::<f64>();
        Some(vol_sign(same_sense) * g / 3.0)
    } else {
        None
    };
    let _ = tol;
    Ok(FaceTess {
        tris,
        quad_area,
        mesh_area,
        volume_face,
        path: "earcut",
    })
}

fn sort2(a: u32, b: u32) -> (u32, u32) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// The pool's own bucket key (the same quantisation `VertexPool::insert`
/// uses): two points sharing a bucket weld into one pool vertex, so any two
/// locally distinct refinement points sharing a key must be unified locally
/// as well, or triangles that differ only in those local copies come out
/// set-identical after mapping.
fn pool_key(p: P3, q: f64) -> [i64; 3] {
    [
        (p[0] / q).round() as i64,
        (p[1] / q).round() as i64,
        (p[2] / q).round() as i64,
    ]
}

/// A candidate split coordinate, confined to the face's unrolled window
/// `[lo, hi]`: accepted at the candidate itself or at any whole-period shift
/// of it that sits in the window; `None` when no representative fits, which
/// keeps refinement splits inside the parameter region the ring actually
/// spans (a short-walk midpoint can sit on the far side of a written seam
/// that the face's region does not cover, and must be refused there).
fn wrap_in(c: f64, period: f64, w: (f64, f64)) -> Option<f64> {
    if period <= 0.0 {
        return (c >= w.0 - 1e-9 && c <= w.1 + 1e-9).then_some(c);
    }
    for shift in [0.0, -period, period] {
        let x = c + shift;
        if x >= w.0 - 1e-9 && x <= w.1 + 1e-9 {
            return Some(x);
        }
    }
    None
}

/// Split midpoint in (u, v) for a refinement edge, as the best of candidate
/// parameter splits by distance of the candidates' surface point to the 3D
/// chord's midpoint `chord_mid`, returning the winner and its deviation.
/// Per-axis candidate coordinates: the plain average; over a periodic axis
/// also the walk resolved the short way across a written seam (with |Δ·| over
/// half the period the endpoints' true arc runs the short way across it, and
/// earcut slivers over a full-period window have exactly such edges); and for
/// an edge with exactly one singular endpoint (a cone apex, a sphere pole —
/// the singular u is arbitrary, both branches carry the same 3D point, so an
/// average leaves the true 3D chord: that chord lies on the regular end's
/// generator, whose parameter line is u = const) the regular end's u. Every
/// candidate is wrapped into the face's unrolled window, so only split
/// coordinates that name a surface point of this face's region survive. In a
/// clean case the single average candidate wins, so the extra evals only
/// cost straddling or apical edges.
fn refine_mid(
    surf: &Surf,
    wu: (f64, f64),
    wv: (f64, f64),
    ua: P2,
    ub: P2,
    chord_mid: P3,
) -> (P2, f64) {
    let period_u = surf.period_u();
    let period_v = surf.period_v();
    let mut us: Vec<f64> = Vec::new();
    let mut vs: Vec<f64> = Vec::new();
    let push = |axis: char, cs: &mut Vec<f64>, c: f64| {
        if let Some(x) = wrap_in(
            c,
            if axis == 'u' { period_u } else { period_v },
            if axis == 'u' { wu } else { wv },
        ) {
            cs.push(x);
        }
    };
    push('u', &mut us, (ua[0] + ub[0]) * 0.5);
    push('v', &mut vs, (ua[1] + ub[1]) * 0.5);
    if period_u > 0.0 {
        let d = ub[0] - ua[0];
        if d.abs() > period_u * 0.5 {
            push(
                'u',
                &mut us,
                ua[0] + (d - (d / period_u).round() * period_u) * 0.5,
            );
        }
    }
    if period_v > 0.0 {
        let d = ub[1] - ua[1];
        if d.abs() > period_v * 0.5 {
            push(
                'v',
                &mut vs,
                ua[1] + (d - (d / period_v).round() * period_v) * 0.5,
            );
        }
    }
    let (sing_a, sing_b) = (surf.singular_pt(ua), surf.singular_pt(ub));
    if sing_a != sing_b {
        push('u', &mut us, if sing_a { ub[0] } else { ua[0] });
    }
    let (mut best, mut bestdev): (P2, f64) = if us.is_empty() || vs.is_empty() {
        // No confined candidate survived (cannot happen with in-window
        // endpoints: the plain average is always inside the window). Fall
        // back to the raw average and report its true deviation so at worst
        // the edge is split honestly rather than silently accepted.
        let b = [(ua[0] + ub[0]) * 0.5, (ua[1] + ub[1]) * 0.5];
        let d = dist3(surf.eval(b[0], b[1]), chord_mid);
        (b, d)
    } else {
        ([us[0], vs[0]], f64::INFINITY)
    };
    for x in &us {
        for y in &vs {
            let pm = surf.eval(*x, *y);
            let dev = dist3(pm, chord_mid);
            if dev < bestdev {
                bestdev = dev;
                best = [*x, *y];
            }
        }
    }
    (best, bestdev)
}

/// Refine the earcut triangulation in parameter space: rounds of simultaneous
/// midpoint insertion on edges deviating more than `chord`, fan
/// re-triangulation of each affected triangle, boundary edges locked. All
/// indices are local to `data`; output vertices are mapped into the pool.
/// Seed vertices weld through the pool on their true 3D (`seed_pts`, the
/// shared edge polyline) rather than a re-eval of `data` — two faces' own
/// (u,v) of the same sample disagree at ~1e-6 mm, enough to straddle a pool
/// bucket and open the shared boundary edge.
#[allow(clippy::too_many_arguments)]
fn refine_eearcut(
    surf: &Surf,
    data: &[P2],
    seed_pts: &[P3],
    seed: &[u32],
    locked: &HashSet<(u32, u32)>,
    ranges: &[(usize, usize)],
    chord: f64,
    pool: &mut VertexPool,
) -> Result<(Vec<[u32; 3]>, f64), String> {
    const MAX_ROUNDS: usize = 16;
    const MAX_TRIS: usize = 250_000;
    let mut params: Vec<P2> = data.to_vec();
    let mut verts: Vec<P3> = seed_pts.to_vec();
    // the face's unrolled parameter window: the span of the ring's own
    // (possibly whole-period-shifted) coordinates. Refinement splits are
    // confined to it so the mesh never grows params outside the region the
    // ring covers, even when a short-walk midpoint resolves across a seam.
    let (mut wu_lo, mut wu_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut wv_lo, mut wv_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in data {
        wu_lo = wu_lo.min(p[0]);
        wu_hi = wu_hi.max(p[0]);
        wv_lo = wv_lo.min(p[1]);
        wv_hi = wv_hi.max(p[1]);
    }
    let windows = ((wu_lo, wu_hi), (wv_lo, wv_hi));
    let mut tris: Vec<[u32; 3]> = seed.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    let mut rounds = 0;
    loop {
        rounds += 1;
        // mark long edges. A marked chord whose own ring samples lie on its
        // parameter line (a row chord: a boundary row or any collinear sample
        // run) is re-tiled as a chain fan, not a midpoint fan — its skipped
        // samples already tile the strip between the chord and the row
        // polyline, so the split midpoint class and the parameter-area row
        // slivers it spawns are exactly the double-cover shape.
        let mut marked: HashSet<(u32, u32)> = HashSet::new();
        let mut row_marked: HashSet<(u32, u32)> = HashSet::new();
        let mut midpoint: HashMap<(u32, u32), usize> = HashMap::new();
        for t in tris.iter() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = sort2(a, b);
                if locked.contains(&key) || marked.contains(&key) || row_marked.contains(&key) {
                    continue;
                }
                let (pa, pb) = (verts[a as usize], verts[b as usize]);
                let (pua, pub_) = (params[a as usize], params[b as usize]);
                let (_, dev) = refine_mid(
                    surf,
                    windows.0,
                    windows.1,
                    pua,
                    pub_,
                    scale3(add3(pa, pb), 0.5),
                );
                if dev > chord {
                    if !row_chord_skips(&params, ranges, a as usize, b as usize).is_empty() {
                        row_marked.insert(key);
                    } else {
                        marked.insert(key);
                        midpoint.insert(key, 0usize);
                    }
                }
            }
        }
        if marked.is_empty() && row_marked.is_empty() {
            break;
        }
        if rounds > MAX_ROUNDS {
            return Err(format!(
                "face needs more than {} refinement rounds at chord {}",
                MAX_ROUNDS, chord
            ));
        }
        if tris.len() * 4 > MAX_TRIS {
            return Err(format!(
                "face tessellation would exceed {} triangles at this chord",
                MAX_TRIS
            ));
        }
        // insert midpoints (params + pool)
        let mut new_params: HashMap<(u32, u32), usize> = midpoint;
        // pool bucket -> local index: an evaluated midpoint whose surface
        // point rounds into an existing local vertex's pool bucket reuses that
        // local index instead of forking a second copy whose weld resolves
        // into T-junctions.
        let mut pmap: HashMap<[i64; 3], u32> = HashMap::with_capacity(params.len());
        for (i, v) in verts.iter().enumerate() {
            pmap.insert(pool_key(*v, pool.q), i as u32);
        }
        for key in new_params.keys().copied().collect::<Vec<_>>() {
            let (a, b) = key;
            // Row chords — ring samples on their own parameter line — never
            // reach this loop; they are re-tiled as chain fans, whose skipped
            // samples are the strip's own tiling. No midpoint class.
            let (pua, pub_) = (params[a as usize], params[b as usize]);
            let (pa, pb) = (verts[a as usize], verts[b as usize]);
            let (midp, dev) = refine_mid(
                surf,
                windows.0,
                windows.1,
                pua,
                pub_,
                scale3(add3(pa, pb), 0.5),
            );
            let _ = dev;
            let midvert = surf.eval(midp[0], midp[1]);
            // The evaluated midpoint often re-derives a point the boundary
            // already carries as a pooled ring sample — on arcs, the
            // parameter average is offset from the skipped sample by the
            // row's non-uniform spacing (0.02–0.08 mm here, a fraction of
            // the chord), and fold chords between inserted midpoints and
            // samples resolve to the sample the same way. Reusing the
            // sample's local index tiles both users of the marked edge
            // through the shared pooled curve point instead of forking a
            // second copy whose weld resolves into T-junctions. The
            // contaminated guard below still rejects a reuse into a parent
            // that would re-emit a triangle with a duplicated corner.
            let (pi, _reused) = match pmap.get(&pool_key(midvert, pool.q)).copied() {
                Some(existing) => {
                    let contaminated = tris
                        .iter()
                        .any(|t| t.contains(&existing) && t.contains(&a) && t.contains(&b));
                    if contaminated {
                        let pi = params.len();
                        params.push(midp);
                        verts.push(midvert);
                        // registered too: a later chord of the same round at
                        // the same surface point gets the same guard treatment
                        pmap.insert(pool_key(midvert, pool.q), pi as u32);
                        (pi, false)
                    } else {
                        (existing as usize, true)
                    }
                }
                None => {
                    let pi = params.len();
                    params.push(midp);
                    verts.push(midvert);
                    // register the fork so a later chord of the same round
                    // whose eval lands on the same surface point reuses it
                    // (subject to the same contaminated guard) instead of
                    // forking its own near-duplicate copy
                    pmap.insert(pool_key(midvert, pool.q), pi as u32);
                    (pi, false)
                }
            };
            new_params.insert(key, pi);
        }
        // rebuild triangles. Sub-patterns are chosen so a marked edge never
        // reappears whole as a side — a corner-to-corner fan would keep the
        // long edge alive and the rounds would never converge.
        let mut next: Vec<[u32; 3]> = Vec::with_capacity(tris.len());
        // A fan output with no area in parameter space covers no region of
        // the face: row-fold slivers and corner-duplicated side tris fold
        // across a boundary row instead of tiling it (their 3D wedge is the
        // same sub-tolerance chord approximation every row boundary carries).
        // They cannot be retained: the fold's duplicated corners weld into
        // the open-edge census's degenerate classes. The region they claim is
        // genuinely empty — the row's neighbouring faces tile the row curve
        // from their own sides, so dropping keeps the shell closed.
        let push_tri = |next: &mut Vec<[u32; 3]>, t: [u32; 3]| {
            let (pa, pb, pc) = (
                params[t[0] as usize],
                params[t[1] as usize],
                params[t[2] as usize],
            );
            let area = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);
            let e0 = (pb[0] - pa[0]) * (pb[0] - pa[0]) + (pb[1] - pa[1]) * (pb[1] - pa[1]);
            let e1 = (pc[0] - pa[0]) * (pc[0] - pa[0]) + (pc[1] - pa[1]) * (pc[1] - pa[1]);
            let e2 = (pc[0] - pb[0]) * (pc[0] - pb[0]) + (pc[1] - pb[1]) * (pc[1] - pb[1]);
            // area/edge² approximates the sine of the flattest corner: legit
            // thin tris sit at ~1e-9 and above, row folds and duplicate
            // corners at fp noise 1e-15 and below. The axis-flat row-fold
            // class is eliminated structurally — row chords are chain-fanned
            // along their skipped ring samples, so no skip chord survives to
            // spawn the lens strip. One class the floor alone cannot decide:
            // an inserted midpoint's invert can land exactly ON the face's
            // boundary parameter line, so the fan's boundary sub-tri is
            // exactly degenerate in (u,v) while its 3D wedge — the lens
            // between the boundary curve and the straight chords — carries
            // real area the shell needs closed (the 3π/8 mid on CTC-04's
            // sphere quadrant, 0.17 mm² on an r=5 arc). Retain such a tri on
            // its 3D area: the (u,v)-noise classes have noise-level 3D
            // wedges, ~five orders below.
            let (va, vb, vc) = (
                verts[t[0] as usize],
                verts[t[1] as usize],
                verts[t[2] as usize],
            );
            let (u1, v1, w1) = (vb[0] - va[0], vb[1] - va[1], vb[2] - va[2]);
            let (u2, v2, w2) = (vc[0] - va[0], vc[1] - va[1], vc[2] - va[2]);
            let area3abs = ((v1 * w2 - w1 * v2).powi(2)
                + (w1 * u2 - u1 * w2).powi(2)
                + (u1 * v2 - v1 * u2).powi(2))
            .sqrt();
            let e3 = u1 * u1 + v1 * v1 + w1 * w1;
            let e3max = e3
                .max(u2 * u2 + v2 * v2 + w2 * w2)
                .max((u2 - u1).powi(2) + (v2 - v1).powi(2) + (w2 - w1).powi(2));
            if area.abs() > e0.max(e1).max(e2) * 1e-12 || area3abs > e3max * 1e-6 {
                next.push(t);
            }
        };
        for t in tris.iter() {
            let (c0, c1, c2) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let m01 = new_params.get(&sort2(t[0], t[1])).copied();
            let m12 = new_params.get(&sort2(t[1], t[2])).copied();
            let m20 = new_params.get(&sort2(t[2], t[0])).copied();
            // A row chord (ring samples on its own parameter line) is not
            // split at a midpoint: its skipped samples ARE the strip's
            // tiling, so it fans from the opposite corner along them. The
            // straight chord and the noise lens between it and the row
            // polyline both disappear — no sliver strip, no double cover.
            // Recomputed fresh per parent: the opposite-direction user's fan
            // needs the samples sorted along ITS direction, and params grows
            // between mark and dispatch, so a stale-skip cache would drift.
            let mut rowfan: Option<(usize, Vec<u32>)> = None;
            for k in 0..3 {
                let key = sort2(t[k], t[(k + 1) % 3]);
                if !row_marked.contains(&key) {
                    continue;
                }
                let sk = row_chord_skips(&params, ranges, t[k] as usize, t[(k + 1) % 3] as usize);
                if !sk.is_empty() {
                    rowfan = Some((k, sk));
                    break;
                }
            }
            if let Some((k, sk)) = rowfan {
                let z = t[(k + 2) % 3];
                push_tri(&mut next, [z, t[k], sk[0]]);
                for w in sk.windows(2) {
                    push_tri(&mut next, [z, w[0], w[1]]);
                }
                push_tri(&mut next, [z, *sk.last().unwrap(), t[(k + 1) % 3]]);
                // the parent's other sides survive whole and re-mark next
                // round; skipped ring-sample pairs stay locked (one in-face
                // user each), and the chord pair drops to zero users
                continue;
            }
            match (m01, m12, m20) {
                (None, None, None) => next.push(*t),
                (Some(a), None, None) => {
                    push_tri(&mut next, [c0 as u32, a as u32, c2 as u32]);
                    push_tri(&mut next, [a as u32, c1 as u32, c2 as u32]);
                }
                (None, Some(a), None) => {
                    push_tri(&mut next, [c0 as u32, c1 as u32, a as u32]);
                    push_tri(&mut next, [c0 as u32, a as u32, c2 as u32]);
                }
                (None, None, Some(a)) => {
                    push_tri(&mut next, [c1 as u32, c2 as u32, a as u32]);
                    push_tri(&mut next, [c1 as u32, a as u32, c0 as u32]);
                }
                (Some(x), Some(y), None) => {
                    push_tri(&mut next, [c0 as u32, x as u32, y as u32]);
                    push_tri(&mut next, [c0 as u32, y as u32, c2 as u32]);
                    push_tri(&mut next, [x as u32, c1 as u32, y as u32]);
                }
                (None, Some(x), Some(y)) => {
                    push_tri(&mut next, [c1 as u32, x as u32, y as u32]);
                    push_tri(&mut next, [x as u32, c2 as u32, y as u32]);
                    push_tri(&mut next, [c1 as u32, y as u32, c0 as u32]);
                }
                (Some(x), None, Some(y)) => {
                    // marked edges 01 and 20 share corner c0: corner fan,
                    // then the opposite quad split on the x–c2 diagonal
                    push_tri(&mut next, [x as u32, c0 as u32, y as u32]);
                    push_tri(&mut next, [x as u32, c1 as u32, c2 as u32]);
                    push_tri(&mut next, [x as u32, c2 as u32, y as u32]);
                }
                (Some(x), Some(y), Some(z)) => {
                    push_tri(&mut next, [x as u32, y as u32, z as u32]);
                    push_tri(&mut next, [c0 as u32, x as u32, z as u32]);
                    push_tri(&mut next, [x as u32, c1 as u32, y as u32]);
                    push_tri(&mut next, [y as u32, c2 as u32, z as u32]);
                }
            }
        }
        tris = next;
    }
    // map param vertices to pool indices
    let mapped: Vec<[u32; 3]> = tris
        .iter()
        .map(|t| {
            let (a, b, c) = (
                pool.insert(verts[t[0] as usize]),
                pool.insert(verts[t[1] as usize]),
                pool.insert(verts[t[2] as usize]),
            );
            [a, b, c]
        })
        .filter(|t| {
            // pole/cone-apex wedge triangles weld to a two-same-vertex chordal
            // loop: zero 3D area, and their doubled edges exhaust the
            // adjacency key so the real neighbour opens. The wedge's true
            // chordal cover comes from the tris on both pole branches; the
            // region between them and the singularity lies within the chord
            // tolerance of the single segment they share.
            !(t[0] == t[1] || t[1] == t[2] || t[2] == t[0])
        })
        .collect();
    // independent area reference over the ORIGINAL earcut triangles (the
    // 3-midpoint √g rule; the seed's corners are `data` points)
    let mut quad_area = 0.0f64;
    for t in seed.chunks_exact(3) {
        let (p0, p1, p2) = (
            data[t[0] as usize],
            data[t[1] as usize],
            data[t[2] as usize],
        );
        quad_area += param_tri_area(surf, p0, p1, p2);
    }
    Ok((mapped, quad_area))
}

/// Param triangle area with √g evaluated at the three edge midpoints
/// (3-point Gauss), an area reference independent of the output mesh.
fn param_tri_area(surf: &Surf, a: P2, b: P2, c: P2) -> f64 {
    let area2 = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
    let m = |p: P2, q: P2| [(p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5];
    let g = (surf.sqrt_g(m(a, b)[0], m(a, b)[1])
        + surf.sqrt_g(m(b, c)[0], m(b, c)[1])
        + surf.sqrt_g(m(c, a)[0], m(c, a)[1]))
        / 3.0;
    area2 * g * 0.5
}

/// Emitted triangle with a positive area (degenerates dropped).
fn emit_nonzero(tris: &mut Vec<[u32; 3]>, mesh_area: &mut f64, verts: &[P3], t: [u32; 3]) {
    let (a, b, c) = (
        verts[t[0] as usize],
        verts[t[1] as usize],
        verts[t[2] as usize],
    );
    let area = 0.5 * len3(cross3(sub(b, a), sub(c, a)));
    if area > 0.0 {
        tris.push(t);
        *mesh_area += area;
    }
}

// ---------------------------------------------------------------------------
// quadrature
// ---------------------------------------------------------------------------

/// Green's integral ∮ Q(u,v)·dv over a closed (u, v) ring (P = 0), sampled
/// with 2-point Gauss–Legendre per segment. `ccw` forces the sign: outer
/// rings count positive (counter-clockwise), holes negative.
fn green_ring(surf: &Surf, ring: &Ring, ccw: bool) -> f64 {
    let uv = &ring.uv;
    let n = uv.len();
    let mut s = 0.0f64;
    for i in 0..n {
        let (a, b) = (uv[i], uv[(i + 1) % n]);
        let (du, dv) = (b[0] - a[0], b[1] - a[1]);
        if dv == 0.0 {
            continue;
        }
        // split long segments (a full-period window edge is one segment) so
        // the 2-point Gauss nodes see a trig polynomial piecewise; the
        // quadric integrands are trig polynomials of degree ≤ 3 in u and v
        let k = (((du.abs()).max(dv.abs()) / 0.1) as usize).clamp(1, 256);
        let seg = |t: f64| {
            let p = [a[0] + du * t, a[1] + dv * t];
            surf.flux_q(p[0], p[1]).unwrap_or(f64::NAN)
        };
        for j in 0..k {
            let t0 = j as f64 / k as f64;
            let q = |t: f64| seg(t0 + t / k as f64);
            let g1 = q(0.5 - 0.5 / 3.0f64.sqrt());
            let g2 = q(0.5 + 0.5 / 3.0f64.sqrt());
            s += 0.5 * (g1 + g2) * dv / k as f64;
        }
    }
    // orientation: force CCW for outer loops, CW for holes
    let area2 = uv
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = uv[(i + 1) % n];
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f64>();
    let want = if ccw { area2 > 0.0 } else { area2 < 0.0 };
    if want {
        s
    } else {
        -s
    }
}

/// ∮ Q dv over the rectangular parameter window [u0, u1] × [v0, v1], as
/// ∫_v0^v1 [Q(u1, v) − Q(u0, v)] dv — the constant-v sides contribute nothing.
/// 32 slices of 2-point Gauss–Legendre: the analytic surfaces' Q is at most
/// quadratic in v across a slice, so this is exact to machine precision.
fn green_window(surf: &Surf, u0: f64, u1: f64, v0: f64, v1: f64) -> f64 {
    const N: usize = 32;
    let dv = (v1 - v0) / N as f64;
    let (g1, g2) = (0.5 - 0.5 / 3.0f64.sqrt(), 0.5 + 0.5 / 3.0f64.sqrt());
    let mut s = 0.0f64;
    for k in 0..N {
        let (a, b) = (v0 + dv * k as f64, v0 + dv * (k + 1) as f64);
        let q = |t: f64| {
            let v = a + (b - a) * t;
            surf.flux_q(u1, v).unwrap_or(0.0) - surf.flux_q(u0, v).unwrap_or(0.0)
        };
        s += 0.5 * (q(g1) + q(g2)) * dv;
    }
    s
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_at_origin() -> Frame {
        Frame {
            o: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        }
    }

    fn lin_ring(surf: &Surf, samples: &[(f64, f64)]) -> Ring {
        let pts: Vec<P3> = samples.iter().map(|(u, v)| surf.eval(*u, *v)).collect();
        let uv: Vec<P2> = samples.iter().map(|(u, v)| [*u, *v]).collect();
        Ring {
            pts,
            uv,
            is_outer: true,
        }
    }

    fn quad_frame() -> Frame {
        // a square in z = 1 plane: vertices (±1, ±1, 1), CCW seen from +z
        Frame {
            o: [0.0, 0.0, 1.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        }
    }

    #[test]
    fn frame_build_orthonormalises() {
        let fr = Frame::build([1.0, 2.0, 3.0], [0.0, 2.0, 0.0], Some([3.0, 1.0, 0.0])).unwrap();
        assert!((dot3(fr.z, fr.x)).abs() < 1e-12);
        assert!((dot3(fr.z, fr.y)).abs() < 1e-12);
        assert!((dot3(fr.x, fr.y)).abs() < 1e-12);
        assert!((len3(fr.x) - 1.0).abs() < 1e-12);
        // x was projected: ref (3,1,0) minus its y-part
        let x = fr.x;
        assert!(x[1].abs() < 1e-12 && x[0] > 0.9);
        assert!(Frame::build([0.0; 3], [0.0; 3], None).is_err());
    }

    #[test]
    fn cyl_eval_invert_roundtrip() {
        let s = Surf::Cyl {
            f: frame_at_origin(),
            r: 2.0,
        };
        for u in [-1.5, 0.3, 1.2, 2.5, 3.3] {
            for v in [-1.0, 0.5, 4.0] {
                let p = s.eval(u, v);
                let uv = s.invert(p, 1e-6).unwrap();
                let du = (uv[0] - u).abs() % std::f64::consts::TAU;
                assert!(du.min(std::f64::consts::TAU - du) < 1e-12, "u round-trip");
                assert!((uv[1] - v).abs() < 1e-12, "v round-trip");
            }
        }
        assert!(s.invert([2.0, 2.0, 0.0], 1e-6).is_none());
    }

    #[test]
    fn torus_eval_invert_roundtrip() {
        let s = Surf::Torus {
            f: frame_at_origin(),
            big_r: 5.0,
            r: 1.0,
        };
        for u in [0.2, 1.4, 3.0, 4.7] {
            for v in [-2.5, -0.3, 0.0, 1.0, 2.9] {
                let p = s.eval(u, v);
                let uv = s.invert(p, 1e-6).unwrap();
                let du = (uv[0] - u).abs() % std::f64::consts::TAU;
                assert!(du.min(std::f64::consts::TAU - du) < 1e-12);
                assert!((uv[1] - v).abs() < 1e-12);
            }
        }
        // spindle refused at the caller; invert just answers honestly
        assert!(s.invert([2.0, 0.0, 0.0], 1e-6).is_none());
    }

    #[test]
    fn deboor_degree1_and_degree2_boundaries() {
        // degree 1: the curve is the control polygon
        let c = NurbsCurve {
            degree: 1,
            control_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            weights: vec![1.0; 3],
            knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        };
        assert_eq!(curve_eval(&c, 0.5), [0.5, 0.0, 0.0]);
        // t = 1 is the knot between segment 0 and 1: the shared control point
        assert_eq!(curve_eval(&c, 1.0), [1.0, 0.0, 0.0]);
        // degree 2 Bézier piece A→B→C: the midpoint is ¼A + ½B + ¼C
        let c2 = NurbsCurve {
            degree: 2,
            control_points: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]],
            weights: vec![1.0; 3],
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        };
        let p_half = curve_eval(&c2, 0.5);
        assert!((p_half[0] - 1.0).abs() < 1e-12 && (p_half[1] - 0.5).abs() < 1e-12);
        // the domain end evaluates to the last control point
        let p_end = curve_eval(&c2, 1.0);
        assert_eq!(p_end, [2.0, 0.0, 0.0]);
    }

    #[test]
    fn nurbs_eval_tensor_weighted_matches_reference() {
        // bilinear: weights all 1 → the bilinear interpolation
        let ns = bilinear_surface();
        let p = nurbs_eval(&ns, 0.5, 0.25);
        // rows are u-indexed: row 0 spans y=0.5, row 1 y=1.0; x blends with v
        let expect = [0.875, 0.75, 0.0];
        assert!((p[0] - expect[0]).abs() < 1e-12);
        assert!((p[1] - expect[1]).abs() < 1e-12);
        // rational check: weight 2 on one corner
        let mut ns2 = bilinear_surface();
        ns2.weights[0][0] = 2.0;
        // denominator at (u, v) = (0, 0): B(0,0) = (cp00·2) / 2 = cp00 ✓
        let p = nurbs_eval(&ns2, 0.0, 0.0);
        assert!((p[0] - 0.5).abs() < 1e-12 && (p[1] - 0.5).abs() < 1e-12);
    }

    fn bilinear_surface() -> NurbsSurface {
        NurbsSurface {
            degree_u: 1,
            degree_v: 1,
            control_points: vec![
                vec![[0.5, 0.5, 0.0], [2.0, 0.5, 0.0]],
                vec![[0.5, 1.0, 0.0], [2.0, 1.0, 0.0]],
            ],
            weights: vec![vec![1.0, 1.0], vec![1.0, 1.0]],
            knots_u: vec![0.0, 0.0, 1.0, 1.0],
            knots_v: vec![0.0, 0.0, 1.0, 1.0],
        }
    }

    #[test]
    fn green_ring_full_cylinder_lateral() {
        // a ring wrapping u fully (the seam staircase) must still give
        // 2π·r²·Δv for the cylinder lateral flux
        let s = Surf::Cyl {
            f: frame_at_origin(),
            r: 3.0,
        };
        // the ring: wrap u from 0.37 to 0.37+2π at v=1, seam up/down
        let mut samples: Vec<(f64, f64)> = Vec::new();
        let (u0, v0, v1) = (0.37, 1.0, 5.0);
        // bottom: u0 → u0+2π (the endpoint IS the seam occurrence, walked up)
        for i in 0..64 {
            samples.push((u0 + std::f64::consts::TAU * i as f64 / 64.0, v0));
        }
        samples.push((u0 + std::f64::consts::TAU, v0));
        // seam up
        samples.push((u0 + std::f64::consts::TAU, v1));
        // top back
        for i in (0..64).rev() {
            samples.push((u0 + std::f64::consts::TAU * i as f64 / 64.0, v1));
        }
        // seam down (the raw u of both seam edges is the same entity: the
        // unrolled walk keeps continuity)
        samples.push((u0, v0));
        let ring = Ring {
            pts: samples.iter().map(|(u, v)| s.eval(*u, *v)).collect(),
            uv: samples.iter().map(|(u, v)| [*u, *v]).collect(),
            is_outer: true,
        };
        // the walk above is the staircase the seam encoding produces: assert
        // quadrature equals 2π r² Δv
        let g = green_ring(&s, &ring, true);
        let expect = std::f64::consts::TAU * 9.0 * (v1 - v0);
        let d = (g - expect).abs() / expect.abs().max(1.0);
        assert!(d < 1e-3, "flux {g} vs expected {expect} (rel {d})");
    }

    #[test]
    fn green_ring_rect_plane_matches_area() {
        // plane at o·z = 5: flux = c · area; ring CCW
        let s = Surf::Plane(quad_frame());
        let ring = lin_ring(&s, &[(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]);
        assert!(green_ring(&s, &ring, true) > 0.0);
        let a = green_ring(&s, &ring, true);
        // ∮ (o·z)·u dv = (o·z)·A = 4 with o·z = 1
        assert!((a - 4.0).abs() < 1e-9, "flux {a}");
    }

    #[test]
    fn holed_plane_flux_subtracts_the_hole_once() {
        // NIST CTC-01 regression: the earcut path's hole rings come back from
        // green_ring with the hole's own algebraic flux — negative whatever
        // their winding, because the orientation is forced CW — so the trim's
        // flux is outer + holes. Subtraction added each hole's area back and
        // inflated the part volume by 2·c·A_holes/3 per holed face.
        let s = Surf::Plane(quad_frame());
        let outer = lin_ring(&s, &[(-2.0, -2.0), (2.0, -2.0), (2.0, 2.0), (-2.0, 2.0)]);
        let hole_cw = Ring {
            pts: [(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)]
                .iter()
                .map(|&(u, v)| s.eval(u, v))
                .collect(),
            uv: vec![[-1.0, -1.0], [-1.0, 1.0], [1.0, 1.0], [1.0, -1.0]],
            is_outer: false,
        };
        let hole_ccw = Ring {
            pts: [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
                .iter()
                .map(|&q| s.eval(q[0], q[1]))
                .collect(),
            uv: vec![[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]],
            is_outer: false,
        };
        // (A_outer − A_hole)/3 with c = 1: 12/3 = 4, vs 20/3 for the old bug
        for (_tag, hole) in [("cw", &hole_cw), ("ccw", &hole_ccw)] {
            let mut pool = VertexPool::new(5.0);
            let tess = tess_face(
                &s,
                true,
                &[outer.clone(), hole.clone()],
                &mut pool,
                0.5,
                1e-6,
            )
            .unwrap();
            let vf = tess.volume_face.expect("plane face has a flux potential");
            assert!(
                (vf * 3.0 - 12.0).abs() < 1e-9,
                "flux {vf}·3 vs trimmed 12 (hole winding {hole:?})"
            );
            assert!(
                (tess.quad_area - 12.0).abs() < 1e-9,
                "earcut fill {} vs trimmed 12",
                tess.quad_area
            );
        }
    }
    /// earcut's raw winding convention, read off the output directly: CCW and
    /// CW square rings on the z=1 plane, tessellated at same_sense=true. The
    /// global orientation pass (oriented once per solid, in step.rs) leans on
    /// this: earcut normalises its output, so every face's raw winding is
    /// already n_nat-agreeing whatever the ring wrote — no per-triangle
    /// flipping can be needed, and the earlier flip_tris through
    /// surf.invert(centroid) only corrupted half-window faces whose invert
    /// unwrapped onto a full-period-away branch.
    #[test]
    fn earcut_raw_winding_convention() {
        let s = Surf::Plane(quad_frame());
        for (tag, samples) in [
            (
                "ccw",
                [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].as_slice(),
            ),
            (
                "cw",
                [(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)].as_slice(),
            ),
        ] {
            let ring = lin_ring(&s, samples);
            let mut pool = VertexPool::new(3.0);
            let tess = tess_face(&s, true, &[ring], &mut pool, 0.5, 1e-6).unwrap();
            let mut pos = 0usize;
            let mut neg = 0usize;
            for t in tess.tris.iter() {
                let (a, b, c) = (
                    pool.verts[t[0] as usize],
                    pool.verts[t[1] as usize],
                    pool.verts[t[2] as usize],
                );
                let x = cross3(sub(b, a), sub(c, a));
                if x[2] > 0.0 {
                    pos += 1;
                } else {
                    neg += 1;
                }
            }
            println!(
                "ring {tag} (same_sense true): pos {pos} neg {neg} of {} tris",
                tess.tris.len()
            );
            assert!(pos * neg == 0, "windings mixed within the face");
        }
    }

    #[test]
    fn quad_face_via_earcut() {
        let s = Surf::Plane(quad_frame());
        let ring = lin_ring(&s, &[(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]);
        let mut pool = VertexPool::new(1.0);
        let tess = tess_face(&s, true, &[ring], &mut pool, 0.2, 1e-6).unwrap();
        assert_eq!(tess.tris.len(), 2);
        // the face's flux ∫∫f = (o·z)·A = 4, delivered as volume_face·3
        assert!((tess.volume_face.unwrap() * 3.0 - 4.0).abs() < 1e-9);
        assert!((tess.quad_area - 4.0).abs() < 1e-9);
        assert!((tess.mesh_area - 4.0).abs() < 1e-9);
    }

    #[test]
    fn earcut_interior_refinement_reaches_chord() {
        // a sphere patch with a chord-fine ring: the boundary sits within the
        // chord, but the earcut seed tris span the patch and deviate — rounds
        // must bring every interior edge inside the chord
        let s = Surf::Sphere {
            f: frame_at_origin(),
            r: 10.0,
        };
        let samples: Vec<(f64, f64)> = {
            let mut v = Vec::new();
            for i in 0..=6 {
                v.push((-0.6 + 0.2 * i as f64, -0.6));
            }
            for i in 1..=6 {
                v.push((0.6, -0.6 + 0.2 * i as f64));
            }
            for i in (0..=6).rev() {
                v.push((-0.6 + 0.2 * i as f64, 0.6));
            }
            for i in (1..=5).rev() {
                v.push((-0.6, -0.6 + 0.2 * i as f64));
            }
            v
        };
        let ring = lin_ring(&s, &samples);
        let mut pool = VertexPool::new(10.0);
        let chord = 0.05f64;
        let tess = tess_face(&s, true, &[ring], &mut pool, chord, 1e-6).unwrap();
        assert_eq!(tess.path, "earcut");
        assert!(
            tess.tris.len() > 64,
            "no real refinement: {} tris",
            tess.tris.len()
        );
        // flux: f = r³·cos v (u-independent) → r³·Δu·(sin v1 − sin v0)
        let expect = 1000.0 * 1.2 * (0.6f64.sin() - (-0.6f64).sin());
        let flux = tess.volume_face.unwrap() * 3.0;
        assert!(
            (flux - expect).abs() / expect < 1e-4,
            "flux {flux} vs {expect}"
        );
        // area reference: √g = r²·cos v → area = r²·Δu·(sin v1 − sin v0)
        let expect_area = 100.0 * 1.2 * (0.6f64.sin() - (-0.6f64).sin());
        assert!((tess.quad_area - expect_area).abs() / expect_area < 1e-3);
        let coverage = tess.mesh_area / tess.quad_area;
        assert!(coverage > 0.985 && coverage < 1.005, "coverage {coverage}");
    }

    #[test]
    fn structured_grid_full_cylinder() {
        // rings: bottom circle (full u wrap), seam pairs, top circle — build
        // the realistic trimmed-tube ring for the triangulator
        let s = Surf::Cyl {
            f: frame_at_origin(),
            r: 3.0,
        };
        let mut samples: Vec<(f64, f64)> = Vec::new();
        let (v0, v1) = (2.0, 9.0);
        for i in 0..=32 {
            let u = std::f64::consts::TAU * i as f64 / 32.0;
            samples.push((u, v0));
        }
        for i in 1..=12 {
            samples.push((std::f64::consts::TAU, v0 + 7.0 * i as f64 / 12.0));
        }
        for i in (0..=32).rev() {
            let u = std::f64::consts::TAU * i as f64 / 32.0;
            samples.push((u, v1));
        }
        for i in (1..=12).rev() {
            samples.push((0.0, v0 + 7.0 * i as f64 / 12.0));
        }
        let ring = lin_ring(&s, &samples);
        let mut pool = VertexPool::new(10.0);
        let tess = tess_face(&s, true, &[ring], &mut pool, 0.2, 1e-6).unwrap();
        assert_eq!(tess.path, "structured grid");
        let flux = tess.volume_face.unwrap() * 3.0;
        let expect = 9.0 * std::f64::consts::TAU * 7.0;
        assert!(
            (flux - expect).abs() / expect < 1e-6,
            "flux {flux} vs {expect}"
        );
        let coverage = tess.mesh_area / tess.quad_area;
        assert!(coverage > 0.98, "coverage {coverage}");
    }

    #[test]
    fn whole_sphere_grid_volume() {
        let s = Surf::Sphere {
            f: frame_at_origin(),
            r: 4.0,
        };
        let mut pool = VertexPool::new(5.0);
        let tess = tess_face(&s, true, &[], &mut pool, 0.2, 1e-6).unwrap();
        let v = tess.volume_face.unwrap();
        let expect = (4.0 / 3.0) * std::f64::consts::PI * 64.0;
        assert!((v - expect).abs() / expect < 1e-12);
        let coverage = tess.mesh_area / tess.quad_area;
        assert!(coverage > 0.99, "coverage {coverage}");
        assert!(!tess.tris.is_empty());
    }

    #[test]
    fn whole_torus_grid_volume() {
        let s = Surf::Torus {
            f: frame_at_origin(),
            big_r: 5.0,
            r: 1.0,
        };
        let mut pool = VertexPool::new(10.0);
        // a radius-1 tube needs a fine chord for the flat-mesh area to sit
        // within 2 % of the analytic area: coverage sin(dθ/2)/(dθ/2) at
        // dθ = 2·acos(1 − chord/r) reaches 0.98 only below chord ≈ 0.04
        let tess = tess_face(&s, true, &[], &mut pool, 0.02, 1e-6).unwrap();
        let v = tess.volume_face.unwrap();
        let expect = 2.0 * std::f64::consts::PI * std::f64::consts::PI * 5.0;
        assert!((v - expect).abs() / expect < 1e-12);
        let coverage = tess.mesh_area / tess.quad_area;
        assert!(coverage > 0.98, "coverage {coverage}");
    }

    // --- assemble_ring ---

    /// An open seam edge of a full cylinder: constant u, v from `a` to `b`.
    fn seam_poly(s: &Surf, alpha: f64, v: f64, w: f64) -> EdgePoly {
        let pts: Vec<P3> = (0..=32)
            .map(|i| s.eval(alpha, v + (w - v) * i as f64 / 32.0))
            .collect();
        EdgePoly {
            pts,
            entity_closed: false,
        }
    }

    #[test]
    fn assemble_ring_two_seam_occurrences_give_full_tube() {
        let s = Surf::Cyl {
            f: frame_at_origin(),
            r: 3.0,
        };
        let (v_b, v_t, alpha) = (2.0, 9.0, 0.6);
        let p = seam_poly(&s, alpha, v_b, v_t);
        let parts = vec![
            RingPart {
                poly: p.clone(),
                fwd: true,
                occ: 0,
            },
            RingPart {
                poly: p,
                fwd: false,
                occ: 1,
            },
        ];
        let ring = assemble_ring(&s, &parts, true, 1e-6, 1e-6).unwrap();
        let w_u = ring.uv.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min)
            - ring
                .uv
                .iter()
                .map(|q| -q[0])
                .fold(f64::NEG_INFINITY, f64::max);
        // span = max − min
        let lo = ring.uv.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min);
        let hi = ring
            .uv
            .iter()
            .map(|q| q[0])
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (hi - lo - std::f64::consts::TAU).abs() < 1e-5,
            "u span {hi}−{lo}"
        );
        let _ = w_u;
        // 3D continuity: the ring walks up one copy and down the other
        let mut pool = VertexPool::new(10.0);
        let tess = tess_face(&s, true, &[ring], &mut pool, 0.2, 1e-6).unwrap();
        assert_eq!(tess.path, "structured grid");
        let flux = tess.volume_face.unwrap() * 3.0;
        let expect = 9.0 * std::f64::consts::TAU * (v_t - v_b);
        assert!(
            (flux - expect).abs() / expect < 1e-6,
            "flux {flux} vs {expect}"
        );
    }

    #[test]
    fn assemble_ring_four_part_full_torus() {
        let s = Surf::Torus {
            f: frame_at_origin(),
            big_r: 5.0,
            r: 1.0,
        };
        let t = std::f64::consts::TAU;
        let (alpha, gamma) = (0.7, -1.9);
        // entity A: constant-u seam circle (wraps v), closed; entity B:
        // constant-v seam circle (wraps u), closed. Both share the vertex at
        // (alpha, gamma).
        let a_pts: Vec<P3> = (0..=90)
            .map(|i| s.eval(alpha, gamma + t * i as f64 / 90.0))
            .collect();
        let b_pts: Vec<P3> = (0..=90)
            .map(|i| s.eval(alpha + t * i as f64 / 90.0, gamma))
            .collect();
        let a = EdgePoly {
            pts: a_pts,
            entity_closed: true,
        };
        let b = EdgePoly {
            pts: b_pts,
            entity_closed: true,
        };
        let parts = vec![
            RingPart {
                poly: a.clone(),
                fwd: true,
                occ: 0,
            },
            RingPart {
                poly: b.clone(),
                fwd: true,
                occ: 0,
            },
            RingPart {
                poly: a,
                fwd: false,
                occ: 1,
            },
            RingPart {
                poly: b,
                fwd: false,
                occ: 1,
            },
        ];
        let ring = assemble_ring(&s, &parts, true, 1e-2, 1e-2).unwrap();
        let mut pool = VertexPool::new(10.0);
        let tess = tess_face(&s, true, &[ring], &mut pool, 0.2, 1e-6).unwrap();
        assert_eq!(tess.path, "structured grid");
        let v = tess.volume_face.unwrap();
        let expect = 2.0 * std::f64::consts::PI * std::f64::consts::PI * 5.0;
        assert!((v - expect).abs() / expect < 1e-9, "volume {v} vs {expect}");
    }

    #[test]
    fn assemble_ring_two_seam_occurrences_give_full_v_wedge() {
        let s = Surf::Torus {
            f: frame_at_origin(),
            big_r: 5.0,
            r: 1.0,
        };
        // constant-v ruled edge from u=0.2 to u=1.7, used twice: the trimmed
        // region is the full-v wedge
        let (u_a, u_b, gamma) = (0.2, 1.7, -0.4);
        let pts: Vec<P3> = (0..=24)
            .map(|i| s.eval(u_a + (u_b - u_a) * i as f64 / 24.0, gamma))
            .collect();
        let poly = EdgePoly {
            pts,
            entity_closed: false,
        };
        let parts = vec![
            RingPart {
                poly: poly.clone(),
                fwd: true,
                occ: 0,
            },
            RingPart {
                poly,
                fwd: false,
                occ: 1,
            },
        ];
        let ring = assemble_ring(&s, &parts, true, 1e-6, 1e-6).unwrap();
        let lo = ring.uv.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min);
        let hi = ring
            .uv
            .iter()
            .map(|q| q[1])
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (hi - lo - std::f64::consts::TAU).abs() < 1e-5,
            "v span {hi}−{lo}"
        );
        let mut pool = VertexPool::new(10.0);
        let tess = tess_face(&s, true, &[ring], &mut pool, 0.2, 1e-6).unwrap();
        assert_eq!(tess.path, "structured grid");
        let flux = tess.volume_face.unwrap() * 3.0;
        // with the frame at the origin the closed form over the wedge is
        // 3·R·r²·π·(u_b − u_a)
        let expect = 3.0 * 5.0 * 1.0 * std::f64::consts::PI * (u_b - u_a);
        assert!(
            (flux - expect).abs() / expect < 1e-6,
            "flux {flux} vs {expect}"
        );
    }

    #[test]
    fn assemble_ring_discontinuous_rejected() {
        let s = Surf::Cyl {
            f: frame_at_origin(),
            r: 3.0,
        };
        let p = seam_poly(&s, 0.0, 0.0, 1.0);
        // second part starts 4 mm away from the first part's end
        let shifted = EdgePoly {
            pts: p.pts.iter().map(|x| [x[0] + 4.0, x[1], x[2]]).collect(),
            entity_closed: false,
        };
        let parts = vec![
            RingPart {
                poly: p,
                fwd: true,
                occ: 0,
            },
            RingPart {
                poly: shifted,
                fwd: true,
                occ: 0,
            },
        ];
        assert!(assemble_ring(&s, &parts, true, 1e-6, 1e-6).is_err());
    }

    /// Regression (V14 groundwork): an earcut-path face that needs multiple
    /// refinement rounds must come out edge-manifold. The two-marked-edge
    /// sub-patterns must tile the marked triangle exactly — the 01+20 arm
    /// used to emit its quad halves on opposite sides of the c1–y line and
    /// drop the triangle (x, c1, y), leaving three dangling edges.
    #[test]
    fn earcut_refined_cyl_window_comes_out_manifold() {
        let f = Frame {
            o: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        let s = Surf::Cyl { f, r: 10.0 };
        // partial window u in [-2.5, -1.0], v in [0, 60]: arcs sampled so each
        // boundary segment's sagitta stays under the chord; the seed's long
        // diagonals force several refinement rounds
        let n = 16usize;
        let (u0, u1, v0, v1, chord) = (-2.5f64, -1.0, 0.0, 60.0, 0.05);
        let mut samples: Vec<(f64, f64)> = (0..=n)
            .map(|i| (u0 + (u1 - u0) * i as f64 / n as f64, v0))
            .collect();
        samples.push((u1, v1));
        for i in (0..n).rev() {
            samples.push((u0 + (u1 - u0) * i as f64 / n as f64, v1));
        }
        let ring = lin_ring(&s, &samples);
        assert_eq!(ring.uv.len(), 34);
        let mut pool = VertexPool::new(100.0);
        let tess = tess_earcut(&s, &ring, &[], &mut pool, chord, 1e-6, true).unwrap();
        let mesh = fieldsmith_mesh::export::TriMesh {
            verts: pool.verts.clone(),
            tris: tess.tris,
        };
        let adj = fieldsmith_mesh::weld::edge_adjacency(&mesh);
        // the face's own ring edges have their partners on the neighbouring
        // faces of the solid; any open edge NOT on the ring is a tiling hole
        const UNSET: u32 = u32::MAX;
        let boundary: HashSet<(u32, u32)> = (0..34u32).map(|k| sort2(k, (k + 1) % 34)).collect();
        let mut interior_open = 0usize;
        for ti in 0..mesh.tris.len() {
            for k in 0..3 {
                let (a, b) = (mesh.tris[ti][k], mesh.tris[ti][(k + 1) % 3]);
                if a == b {
                    continue;
                }
                if adj.nbr[ti][k] == UNSET && !boundary.contains(&sort2(a, b)) {
                    interior_open += 1;
                }
            }
        }
        assert_eq!(
            interior_open, 0,
            "refined window has {interior_open} interior open edge sides"
        );
        let expect = 10.0 * (u1 - u0) * (v1 - v0);
        assert!(
            (tess.mesh_area - expect).abs() / expect < 0.01,
            "mesh area {:.3} vs analytic {:.3}",
            tess.mesh_area,
            expect
        );
    }
}
