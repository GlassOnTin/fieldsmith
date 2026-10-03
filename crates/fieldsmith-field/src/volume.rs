//! Volume of a field's solid region by voxel integration with a sub-voxel occupancy estimate.

use crate::dual::*;
use crate::expr::{bounds, eval, Aabb, Expr};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeEstimate {
    /// Volume (mm³) at the finest spacing.
    pub volume: f64,
    /// Estimated discretisation error (mm³) from comparing spacings `h` and `2h`.
    pub error: f64,
    /// Voxel spacing used (mm).
    pub spacing: f64,
}

/// Occupancy of a cube of edge `h` whose centre has field sample `d` (with gradient):
/// a linear ramp over the cube's extent along the surface normal, `h·‖n‖₁`.
#[inline]
pub fn occupancy(d: D, h: f64) -> f64 {
    let gl = len(d.g);
    if gl < 1e-12 {
        return if d.v < 0.0 { 1.0 } else { 0.0 };
    }
    let n = scale(d.g, 1.0 / gl);
    let width = h * (n[0].abs() + n[1].abs() + n[2].abs());
    (0.5 - (d.v / gl) / width).clamp(0.0, 1.0)
}

/// Integrate the solid volume of `e` on a grid of spacing `h`, restricted to `region`.
/// Blocks of `B³` voxels whose centre lies further than `1.5 ×` the block half-diagonal from the
/// surface are classified wholesale (the factor allows for fields that are not exact distances).
pub fn integrate(e: &Expr, h: f64, region: Option<Aabb>) -> f64 {
    integrate_with(&|p| eval(e, p), bounds(e), h, region)
}

pub fn integrate_with(f: &dyn Fn(V3) -> D, b: Aabb, h: f64, region: Option<Aabb>) -> f64 {
    let b = match region {
        Some(r) => b.intersect(&r),
        None => b,
    };
    if b.is_empty() {
        return 0.0;
    }
    const B: usize = 8;
    let lo = sub(b.lo, [h; 3]);
    let n = [
        (((b.hi[0] - lo[0]) / h).ceil() as usize + 1).div_ceil(B) * B,
        (((b.hi[1] - lo[1]) / h).ceil() as usize + 1).div_ceil(B) * B,
        (((b.hi[2] - lo[2]) / h).ceil() as usize + 1).div_ceil(B) * B,
    ];
    let bs = B as f64 * h;
    let half_diag = 0.5 * bs * 3f64.sqrt();
    let voxel = h * h * h;
    // Voxels are counted only when their centre lies inside `b`; blocks are padded past it.
    let inside = |p: V3| (0..3).all(|a| p[a] >= b.lo[a] && p[a] < b.hi[a]);
    let mut total = 0.0;
    for bk in 0..n[2] / B {
        for bj in 0..n[1] / B {
            for bi in 0..n[0] / B {
                let c = [
                    lo[0] + (bi as f64 + 0.5) * bs,
                    lo[1] + (bj as f64 + 0.5) * bs,
                    lo[2] + (bk as f64 + 0.5) * bs,
                ];
                let dc = f(c).v;
                if dc > 1.5 * half_diag {
                    continue;
                }
                let solid = dc < -1.5 * half_diag;
                let mut s = 0.0;
                for k in 0..B {
                    for j in 0..B {
                        for i in 0..B {
                            let p = [
                                lo[0] + ((bi * B + i) as f64 + 0.5) * h,
                                lo[1] + ((bj * B + j) as f64 + 0.5) * h,
                                lo[2] + ((bk * B + k) as f64 + 0.5) * h,
                            ];
                            if !inside(p) {
                                continue;
                            }
                            s += if solid { 1.0 } else { occupancy(f(p), h) };
                        }
                    }
                }
                total += s * voxel;
            }
        }
    }
    total
}

/// Volume with a Richardson-style error estimate from spacings `h` and `2h` (second order).
pub fn volume(e: &Expr, h: f64) -> VolumeEstimate {
    volume_with(&|p| eval(e, p), bounds(e), h)
}

/// Default grid spacing for reported volumes: 1/160 of the largest bounding dimension.
pub fn default_spacing(b: &Aabb) -> f64 {
    (b.size().iter().cloned().fold(0.0, f64::max) / 160.0).max(0.02)
}

/// The volume shown to the user (app, CLI and verification case V4 all call this).
pub fn volume_auto(e: &Expr) -> VolumeEstimate {
    let b = bounds(e);
    if b.is_empty() {
        return VolumeEstimate {
            volume: 0.0,
            error: 0.0,
            spacing: 0.0,
        };
    }
    volume(e, default_spacing(&b))
}

pub fn volume_with(f: &dyn Fn(V3) -> D, b: Aabb, h: f64) -> VolumeEstimate {
    let fine = integrate_with(f, b, h, None);
    let coarse = integrate_with(f, b, 2.0 * h, None);
    VolumeEstimate {
        volume: fine,
        error: (fine - coarse).abs() / 3.0,
        spacing: h,
    }
}
