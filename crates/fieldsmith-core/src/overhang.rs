//! Exact-gradient surface overhang analysis (`M23`, `V45`).
//!
//! For any extracted surface triangle `t = [i, j, k]` with vertices `(a, b, c)`:
//! - its area is `A_t = ½ ‖(b − a) × (c − a)‖` (degenerate triangles with `A_t ≤ 1e-12`
//!   are skipped);
//! - its centroid is `p = (a + b + c) / 3`;
//! - its outward unit normal `n(p)` is evaluated from the exact implicit field gradient
//!   `∇f(p)` at `p` (`n = ∇f(p) / ‖∇f(p)‖` when `‖∇f(p)‖ > 1e-12`, falling back to the
//!   geometric triangle normal when the gradient vanishes).
//!
//! Relative to the unit build direction `b̂` (default `+Z = [0, 0, 1]`), the downward
//! alignment of the outward normal is `d = n(p) · (−b̂)`. The local overhang angle from
//! build vertical is `α(p) = arcsin(max(0, d))` in `[0°, 90°]` (`90°` on a horizontal
//! downward-facing ceiling where `n = −b̂`, `0°` on a vertical wall where `n · b̂ = 0`).
//! A facet is in overhang when `d > sin(α_max)`.

use fieldsmith_doc::Document;
use fieldsmith_field::expr::{self, Expr};
use fieldsmith_mesh::{
    adaptive::{surface_adaptive, AdaptiveOpts, Sample},
    export::TriMesh,
};
use serde::{Deserialize, Serialize};

/// Default build axis (`+Z`).
pub const DEFAULT_BUILD_AXIS: [f64; 3] = [0.0, 0.0, 1.0];

/// Default overhang threshold angle from build vertical (degrees).
pub const DEFAULT_OVERHANG_DEG: f64 = 45.0;

/// Summary of surface overhang relative to a build direction `b̂` and threshold `α_max`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverhangReport {
    /// Unit build direction vector `b̂`.
    pub build_axis: [f64; 3],
    /// Overhang threshold angle `α_max` from build vertical (degrees).
    pub threshold_deg: f64,
    /// Total surface area of the mesh (mm²).
    pub total_area_mm2: f64,
    /// Surface area of facets where `n(p) · (−b̂) > sin(α_max)` (mm²).
    pub overhang_area_mm2: f64,
    /// Fraction of total surface area in overhang (`overhang_area_mm2 / total_area_mm2`,
    /// or `0.0` when `total_area_mm2 == 0.0`).
    pub overhang_fraction: f64,
    /// Worst (maximum) overhang angle from build vertical across all downward-facing
    /// facets (degrees in `[0, 90]`).
    pub max_overhang_deg: f64,
    /// Number of surface triangles exceeding the overhang threshold.
    pub overhang_tris: usize,
}

/// Unit build direction from spherical build orientation angles `(theta_deg, phi_deg)`:
/// `theta` is the tilt from `+Z` toward the XY plane (degrees), and `phi` is the azimuth
/// in the XY plane from `+X` toward `+Y` (degrees). `(0, 0)` is `+Z = [0, 0, 1]`.
pub fn build_axis_from_angles(theta_deg: f64, phi_deg: f64) -> [f64; 3] {
    let th = theta_deg.to_radians();
    let ph = phi_deg.to_radians();
    let (st, ct) = th.sin_cos();
    let (sp, cp) = ph.sin_cos();
    [st * cp, st * sp, ct]
}

fn unit3(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l <= 1e-15 {
        DEFAULT_BUILD_AXIS
    } else {
        [v[0] / l, v[1] / l, v[2] / l]
    }
}

/// Evaluate surface overhang on `mesh` using the exact field gradient `∇f(p)` of `field`
/// at each triangle centroid.
pub fn evaluate_overhang(
    mesh: &TriMesh,
    field: &Expr,
    build_axis: [f64; 3],
    threshold_deg: f64,
) -> OverhangReport {
    let b_hat = unit3(build_axis);
    let sin_alpha = threshold_deg.to_radians().sin();
    // Small guard above sin(α_max) so vertical/boundary float noise (~1e-16) does not
    // classify a tangent facet as overhanging.
    let thresh = sin_alpha + 1e-9;

    let mut total_area = 0.0;
    let mut overhang_area = 0.0;
    let mut max_down = 0.0_f64;
    let mut overhang_tris = 0usize;

    for &t in &mesh.tris {
        let a = mesh.verts[t[0] as usize];
        let b = mesh.verts[t[1] as usize];
        let c = mesh.verts[t[2] as usize];
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cr = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let cr_len = (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt();
        let area = 0.5 * cr_len;
        if area <= 1e-12 {
            continue;
        }
        total_area += area;

        let p = [
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
            (a[2] + b[2] + c[2]) / 3.0,
        ];
        let g = expr::eval(field, p).g;
        let g_len = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
        let n = if g_len > 1e-12 {
            [g[0] / g_len, g[1] / g_len, g[2] / g_len]
        } else {
            [cr[0] / cr_len, cr[1] / cr_len, cr[2] / cr_len]
        };

        // Outward normal alignment with −b̂.
        let down = -(n[0] * b_hat[0] + n[1] * b_hat[1] + n[2] * b_hat[2]).clamp(-1.0, 1.0);
        if down > max_down {
            max_down = down;
        }
        if down > thresh {
            overhang_area += area;
            overhang_tris += 1;
        }
    }

    let overhang_fraction = if total_area > 0.0 {
        overhang_area / total_area
    } else {
        0.0
    };
    let max_overhang_deg = max_down.clamp(0.0, 1.0).asin().to_degrees();

    OverhangReport {
        build_axis: b_hat,
        threshold_deg,
        total_area_mm2: total_area,
        overhang_area_mm2: overhang_area,
        overhang_fraction,
        max_overhang_deg,
        overhang_tris,
    }
}

/// Compile the solid body of `doc`, extract its adaptive dual-contouring surface at
/// spacing `h` (default `0.25 mm`), and evaluate [`evaluate_overhang`].
pub fn check_overhang(
    doc: &Document,
    h: Option<f64>,
    build_axis: [f64; 3],
    threshold_deg: f64,
) -> Result<OverhangReport, String> {
    let h = h.unwrap_or(0.25);
    if !h.is_finite() || h <= 0.0 {
        return Err("overhang mesh spacing h must be positive and finite".into());
    }
    if !threshold_deg.is_finite() || !(0.0..=90.0).contains(&threshold_deg) {
        return Err("overhang threshold angle must be between 0° and 90°".into());
    }
    let field = expr::compile_body(doc);
    let b = expr::bounds(&field);
    if b.is_empty() {
        return Err("document has no solid".into());
    }
    let mesh = surface_adaptive(
        |p| {
            let d = expr::eval(&field, p);
            Sample { v: d.v, g: d.g }
        },
        [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h],
        [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h],
        h,
        &AdaptiveOpts::default(),
    );
    Ok(evaluate_overhang(&mesh, &field, build_axis, threshold_deg))
}
