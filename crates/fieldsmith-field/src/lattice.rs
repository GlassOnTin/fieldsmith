//! Triply periodic minimal surface (TPMS) lattices, their density calibration, and the
//! filled-part field.

use crate::dual::*;
use crate::expr::{eval, Expr};
use fieldsmith_doc::{Family, FillMode};
use std::f64::consts::PI;

/// Raw TPMS level function `g` and its gradient at `p` for cell size `a`.
#[inline]
pub fn tpms(family: Family, a: f64, p: V3) -> D {
    let k = 2.0 * PI / a;
    let (sx, cx) = (k * p[0]).sin_cos();
    let (sy, cy) = (k * p[1]).sin_cos();
    let (sz, cz) = (k * p[2]).sin_cos();
    match family {
        Family::Gyroid => D::new(
            sx * cy + sy * cz + sz * cx,
            [
                k * (cx * cy - sz * sx),
                k * (cy * cz - sx * sy),
                k * (cz * cx - sy * sz),
            ],
        ),
        Family::Primitive => D::new(cx + cy + cz, [-k * sx, -k * sy, -k * sz]),
        Family::Diamond => D::new(
            sx * sy * sz + sx * cy * cz + cx * sy * cz + cx * cy * sz,
            [
                k * (cx * sy * sz + cx * cy * cz - sx * sy * cz - sx * cy * sz),
                k * (sx * cy * sz - sx * sy * cz + cx * cy * cz - cx * sy * sz),
                k * (sx * sy * cz - sx * cy * sz - cx * sy * sz + cx * cy * cz),
            ],
        ),
    }
}

/// Approximate signed distance (mm) to the TPMS mid-surface: `g / |∇g|` with a floored gradient.
#[inline]
pub fn tpms_distance(family: Family, a: f64, p: V3) -> D {
    let t = tpms(family, a, p);
    let k = 2.0 * PI / a;
    let gl = len(t.g).max(0.3 * k);
    D::new(t.v / gl, scale(t.g, 1.0 / gl))
}

/// Lattice solid field for parameter `param` (sheet: wall thickness in mm; network: level offset in mm).
#[inline]
pub fn lattice_field(family: Family, mode: FillMode, a: f64, param: f64, p: V3) -> D {
    let s = tpms_distance(family, a, p);
    match mode {
        FillMode::Sheet => {
            let sg = if s.v < 0.0 { -1.0 } else { 1.0 };
            D::new(s.v.abs() - 0.5 * param, scale(s.g, sg))
        }
        FillMode::Network => D::new(s.v - param, s.g),
    }
}

/// Monotone table mapping relative density ρ ↔ lattice parameter for one family and mode,
/// measured by sampling one periodic unit cell. Parameters are stored per unit cell size
/// (i.e. for a = 1) and scale linearly with the cell size.
#[derive(Clone, Debug)]
pub struct Calibration {
    pub family: Family,
    pub mode: FillMode,
    /// (param_per_cell, rho) pairs with strictly increasing rho.
    pub table: Vec<(f64, f64)>,
}

impl Calibration {
    /// Sample a unit cell on an `n³` midpoint grid.
    pub fn measure(family: Family, mode: FillMode, n: usize) -> Self {
        // Pre-compute distances once; density for any parameter is then a count.
        let mut dists = Vec::with_capacity(n * n * n);
        for i in 0..n {
            for j in 0..n {
                for k in 0..n {
                    let p = [
                        (i as f64 + 0.5) / n as f64,
                        (j as f64 + 0.5) / n as f64,
                        (k as f64 + 0.5) / n as f64,
                    ];
                    dists.push(tpms_distance(family, 1.0, p).v);
                }
            }
        }
        let total = dists.len() as f64;
        let params: Vec<f64> = match mode {
            FillMode::Sheet => (0..=60).map(|i| i as f64 * 0.01).collect(),
            FillMode::Network => (0..=80).map(|i| -0.4 + i as f64 * 0.01).collect(),
        };
        let mut table: Vec<(f64, f64)> = Vec::new();
        for &q in &params {
            let solid = dists
                .iter()
                .filter(|&&d| match mode {
                    FillMode::Sheet => d.abs() - 0.5 * q < 0.0,
                    FillMode::Network => d - q < 0.0,
                })
                .count() as f64;
            let rho = solid / total;
            if table.last().is_none_or(|&(_, r)| rho > r + 1e-9) {
                table.push((q, rho));
            }
        }
        Self {
            family,
            mode,
            table,
        }
    }

    /// Lattice parameter (mm) for density `rho` at cell size `a` (linear interpolation, clamped).
    pub fn param_for(&self, rho: f64, a: f64) -> f64 {
        let t = &self.table;
        if rho <= t[0].1 {
            return t[0].0 * a;
        }
        for w in t.windows(2) {
            if rho <= w[1].1 {
                let f = (rho - w[0].1) / (w[1].1 - w[0].1);
                return (w[0].0 + f * (w[1].0 - w[0].0)) * a;
            }
        }
        t[t.len() - 1].0 * a
    }

    /// Density for a lattice parameter (mm) at cell size `a`.
    pub fn rho_for(&self, param: f64, a: f64) -> f64 {
        let q = param / a;
        let t = &self.table;
        if q <= t[0].0 {
            return t[0].1;
        }
        for w in t.windows(2) {
            if q <= w[1].0 {
                let f = (q - w[0].0) / (w[1].0 - w[0].0);
                return w[0].1 + f * (w[1].1 - w[0].1);
            }
        }
        t[t.len() - 1].1
    }

    pub fn max_rho(&self) -> f64 {
        self.table.last().map(|x| x.1).unwrap_or(1.0)
    }
}

/// Cached calibrations (computed once per process at 64³ resolution).
pub fn calibration(family: Family, mode: FillMode) -> &'static Calibration {
    use std::sync::OnceLock;
    static CACHE: [OnceLock<Calibration>; 6] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    let fi = match family {
        Family::Gyroid => 0,
        Family::Primitive => 1,
        Family::Diamond => 2,
    };
    let mi = match mode {
        FillMode::Sheet => 0,
        FillMode::Network => 1,
    };
    CACHE[fi * 2 + mi].get_or_init(|| Calibration::measure(family, mode, 64))
}

/// Regular 3D grid of scalar samples with trilinear interpolation (node-centred).
#[derive(Clone, Debug, PartialEq)]
pub struct Grid3 {
    pub origin: V3,
    pub spacing: f64,
    pub dims: [usize; 3],
    pub data: Vec<f32>,
}

impl Grid3 {
    pub fn constant(value: f32) -> Self {
        Self {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            data: vec![value],
        }
    }
    #[inline]
    pub fn at(&self, i: usize, j: usize, k: usize) -> f32 {
        self.data[(k * self.dims[1] + j) * self.dims[0] + i]
    }
    /// Trilinear interpolation; coordinates outside the grid are clamped to its boundary.
    /// GLSL twin: `fs_grid` in [`crate::glsl`] (identical f32 arithmetic order is not required;
    /// both sides use the same formula).
    pub fn sample(&self, p: V3) -> f64 {
        let mut idx = [0usize; 3];
        let mut frac = [0.0f64; 3];
        for a in 0..3 {
            let n = self.dims[a];
            if n == 1 {
                idx[a] = 0;
                frac[a] = 0.0;
                continue;
            }
            let u = ((p[a] - self.origin[a]) / self.spacing).clamp(0.0, (n - 1) as f64);
            let i0 = (u.floor() as usize).min(n - 2);
            idx[a] = i0;
            frac[a] = u - i0 as f64;
        }
        let step = |a: usize| if self.dims[a] == 1 { 0 } else { 1 };
        let (i, j, k) = (idx[0], idx[1], idx[2]);
        let (di, dj, dk) = (step(0), step(1), step(2));
        let c = |a, b, c| self.at(a, b, c) as f64;
        let c00 = c(i, j, k) * (1.0 - frac[0]) + c(i + di, j, k) * frac[0];
        let c10 = c(i, j + dj, k) * (1.0 - frac[0]) + c(i + di, j + dj, k) * frac[0];
        let c01 = c(i, j, k + dk) * (1.0 - frac[0]) + c(i + di, j, k + dk) * frac[0];
        let c11 = c(i, j + dj, k + dk) * (1.0 - frac[0]) + c(i + di, j + dj, k + dk) * frac[0];
        let c0 = c00 * (1.0 - frac[1]) + c10 * frac[1];
        let c1 = c01 * (1.0 - frac[1]) + c11 * frac[1];
        c0 * (1.0 - frac[2]) + c1 * frac[2]
    }
}

/// The filled part: `max(body, min(−(body + skin), lattice(param(p)), keep))`.
#[derive(Clone, Debug, PartialEq)]
pub struct FillExpr {
    pub body: Expr,
    pub skin: f64,
    pub family: Family,
    pub mode: FillMode,
    pub cell: f64,
    /// Lattice parameter field (mm), already including density grading and min-wall limits.
    pub param: Grid3,
    /// Union of protected zones (shapes offset by the margin); `Empty` for none.
    pub keep: Expr,
}

impl FillExpr {
    pub fn eval(&self, p: V3) -> D {
        let d = eval(&self.body, p);
        let core = D::new(-(d.v + self.skin), scale(d.g, -1.0));
        let lat = lattice_field(self.family, self.mode, self.cell, self.param.sample(p), p);
        let mut inner = dmin(core, lat);
        if !matches!(self.keep, Expr::Empty) {
            inner = dmin(inner, eval(&self.keep, p));
        }
        dmax(d, inner)
    }

    /// Classify a point inside the body: 0 = skin or protected (solid), 1 = lattice core.
    pub fn in_core(&self, p: V3) -> bool {
        let d = eval(&self.body, p).v;
        if d >= -self.skin {
            return false;
        }
        if !matches!(self.keep, Expr::Empty) && eval(&self.keep, p).v < 0.0 {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tpms_gradients_match_finite_differences() {
        for fam in [Family::Gyroid, Family::Primitive, Family::Diamond] {
            for p in [[0.3, 1.7, 2.2], [4.1, -0.6, 0.9]] {
                let d = tpms(fam, 5.0, p);
                for i in 0..3 {
                    let h = 1e-6;
                    let mut a = p;
                    let mut b = p;
                    a[i] += h;
                    b[i] -= h;
                    let fd = (tpms(fam, 5.0, a).v - tpms(fam, 5.0, b).v) / (2.0 * h);
                    assert!(
                        (fd - d.g[i]).abs() < 1e-5,
                        "{fam:?} grad {i}: {fd} vs {}",
                        d.g[i]
                    );
                }
            }
        }
    }

    #[test]
    fn calibration_is_monotone_and_plausible() {
        let c = Calibration::measure(Family::Gyroid, FillMode::Sheet, 48);
        assert!(c
            .table
            .windows(2)
            .all(|w| w[1].1 > w[0].1 && w[1].0 > w[0].0));
        // Gyroid surface-to-volume ratio ≈ 3.09 / a, so a thin sheet of thickness t has ρ ≈ 3.09 t / a.
        let rho = c.rho_for(0.05, 1.0);
        assert!((rho - 3.09 * 0.05).abs() < 0.02, "rho(0.05) = {rho}");
        let n = Calibration::measure(Family::Gyroid, FillMode::Network, 48);
        // The gyroid splits space into two equal labyrinths: zero offset gives ρ = 0.5.
        assert!((n.rho_for(0.0, 1.0) - 0.5).abs() < 0.01);
    }

    #[test]
    fn grid_trilinear_reproduces_linear_functions() {
        let mut g = Grid3 {
            origin: [1.0, 2.0, 3.0],
            spacing: 0.5,
            dims: [4, 5, 6],
            data: Vec::new(),
        };
        for k in 0..6 {
            for j in 0..5 {
                for i in 0..4 {
                    let p = [
                        1.0 + 0.5 * i as f64,
                        2.0 + 0.5 * j as f64,
                        3.0 + 0.5 * k as f64,
                    ];
                    g.data.push((p[0] + 2.0 * p[1] - p[2]) as f32);
                }
            }
        }
        let p = [1.7, 3.1, 4.2];
        assert!((g.sample(p) - (p[0] + 2.0 * p[1] - p[2])).abs() < 1e-5);
    }
}
