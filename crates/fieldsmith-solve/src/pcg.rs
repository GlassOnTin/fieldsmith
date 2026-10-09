//! Preconditioned conjugate gradients with a two-level (P2 → P1) preconditioner.

use crate::bsr::{inv3, mv3, Bsr};
use crate::{assemble, Problem};
use faer::prelude::*;
use faer::sparse::SparseColMat;
use faer::Side;

#[derive(Clone, Copy, Debug)]
pub struct SolverOptions {
    /// Relative residual ‖b − Au‖ / ‖b‖ at which to stop.
    pub rtol: f64,
    pub max_iter: usize,
}

impl Default for SolverOptions {
    fn default() -> Self {
        SolverOptions {
            rtol: 1e-9,
            max_iter: 2000,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SolveReport {
    pub dofs: usize,
    pub free_dofs: usize,
    pub coarse_dofs: usize,
    pub iterations: usize,
    pub rel_residual: f64,
    pub converged: bool,
}

#[inline]
fn is_fixed(fixed: &[u8], node: usize, c: usize) -> bool {
    fixed[node] & (1 << c) != 0
}

/// Zero fixed rows/columns and put 1 on their diagonal, so fixed DOFs decouple with value 0.
pub(crate) fn apply_dirichlet(a: &mut Bsr, fixed: &[u8]) {
    for i in 0..a.n {
        let (s, e) = (a.row_ptr[i] as usize, a.row_ptr[i + 1] as usize);
        for k in s..e {
            let j = a.cols[k] as usize;
            if fixed[i] == 0 && fixed[j] == 0 {
                continue;
            }
            let b = &mut a.vals[k];
            for r in 0..3 {
                for c in 0..3 {
                    if is_fixed(fixed, i, r) || is_fixed(fixed, j, c) {
                        b[r * 3 + c] = if i == j && r == c { 1.0 } else { 0.0 };
                    }
                }
            }
        }
    }
}

pub(crate) struct TwoLevel<'a> {
    a: &'a Bsr,
    dinv: Vec<[f64; 9]>,
    fixed: &'a [u8],
    nverts: usize,
    mid_edge: &'a [[u32; 2]],
    coarse: faer::sparse::linalg::solvers::Cholesky<usize, f64>,
}

impl<'a> TwoLevel<'a> {
    /// Coarse operator Ac = Pᵀ M A M P on the P1 (vertex) pattern plus the block-SGS
    /// smoothing, built once and reused by every PCG iteration (and by the modal route's
    /// inner solves, which run many of them against one stiffened system).
    pub(crate) fn new(
        a: &'a Bsr,
        fixed: &'a [u8],
        nverts: usize,
        tets: &[[u32; 10]],
        mid_edge: &'a [[u32; 2]],
    ) -> Result<TwoLevel<'a>, String> {
        let dinv = (0..a.n)
            .map(|i| inv3(&a.vals[a.find(i, i as u32)]))
            .collect();
        let nv = nverts;
        // Coarse operator Ac = Pᵀ M A M P on the P1 (vertex) pattern.
        let mut ac = Bsr::from_elements(nv, tets.iter().map(|t| [t[0], t[1], t[2], t[3]]));
        let pmap = |i: usize| -> [(u32, f64); 2] {
            if i < nv {
                [(i as u32, 1.0), (u32::MAX, 0.0)]
            } else {
                let [x, y] = mid_edge[i - nv];
                [(x, 0.5), (y, 0.5)]
            }
        };
        for i in 0..a.n {
            let (s, e) = (a.row_ptr[i] as usize, a.row_ptr[i + 1] as usize);
            let pi = pmap(i);
            for k in s..e {
                let j = a.cols[k] as usize;
                let mut b = a.vals[k];
                for r in 0..3 {
                    for c in 0..3 {
                        if is_fixed(fixed, i, r) || is_fixed(fixed, j, c) {
                            b[r * 3 + c] = 0.0;
                        }
                    }
                }
                let pj = pmap(j);
                for &(ci, wi) in &pi {
                    if ci == u32::MAX {
                        continue;
                    }
                    for &(cj, wj) in &pj {
                        if cj == u32::MAX {
                            continue;
                        }
                        let w = wi * wj;
                        let scaled = b.map(|x| x * w);
                        ac.add(ci as usize, cj, &scaled);
                    }
                }
            }
        }
        // Scalar lower-triangle triplets; empty rows get a unit diagonal.
        let n = nv * 3;
        let mut trip: Vec<(usize, usize, f64)> = Vec::with_capacity(ac.nnz_blocks() * 5);
        let mut row_has = vec![false; n];
        for i in 0..nv {
            let (s, e) = (ac.row_ptr[i] as usize, ac.row_ptr[i + 1] as usize);
            for k in s..e {
                let j = ac.cols[k] as usize;
                let b = &ac.vals[k];
                for r in 0..3 {
                    for c in 0..3 {
                        let (gi, gj) = (3 * i + r, 3 * j + c);
                        let v = b[r * 3 + c];
                        if gi >= gj && v != 0.0 {
                            trip.push((gi, gj, v));
                            if gi == gj {
                                row_has[gi] = true;
                            }
                        }
                    }
                }
            }
        }
        for (g, has) in row_has.iter().enumerate() {
            if !has {
                trip.push((g, g, 1.0));
            }
        }
        let m = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &trip)
            .map_err(|e| format!("coarse matrix: {e:?}"))?;
        let coarse = m.sp_cholesky(Side::Lower).map_err(|e| {
            format!("coarse factorisation failed ({e:?}); the part may not be held by its supports")
        })?;
        Ok(TwoLevel {
            a,
            dinv,
            fixed,
            nverts: nv,
            mid_edge,
            coarse,
        })
    }

    fn gs_sweep(&self, r: &[f64], x: &mut [f64], forward: bool) {
        let a = self.a;
        let mut sweep = |i: usize| {
            let (s, e) = (a.row_ptr[i] as usize, a.row_ptr[i + 1] as usize);
            let mut acc = [r[3 * i], r[3 * i + 1], r[3 * i + 2]];
            for k in s..e {
                let j = a.cols[k] as usize;
                if j == i {
                    continue;
                }
                let b = &a.vals[k];
                let (x0, x1, x2) = (x[3 * j], x[3 * j + 1], x[3 * j + 2]);
                acc[0] -= b[0] * x0 + b[1] * x1 + b[2] * x2;
                acc[1] -= b[3] * x0 + b[4] * x1 + b[5] * x2;
                acc[2] -= b[6] * x0 + b[7] * x1 + b[8] * x2;
            }
            let v = mv3(&self.dinv[i], acc);
            x[3 * i..3 * i + 3].copy_from_slice(&v);
        };
        if forward {
            (0..a.n).for_each(&mut sweep);
        } else {
            (0..a.n).rev().for_each(&mut sweep);
        }
    }

    /// z = M⁻¹ r (symmetric V-cycle: forward GS, coarse correction, backward GS).
    pub(crate) fn apply(&self, r: &[f64], z: &mut [f64], tmp: &mut [f64]) {
        z.iter_mut().for_each(|v| *v = 0.0);
        self.gs_sweep(r, z, true);
        // Residual.
        self.a.mul(z, tmp);
        for (t, rv) in tmp.iter_mut().zip(r) {
            *t = rv - *t;
        }
        // Restrict: rc = Pᵀ M res.
        let nv = self.nverts;
        let mut rc = faer::Col::<f64>::zeros(nv * 3);
        for i in 0..self.a.n {
            for c in 0..3 {
                if is_fixed(self.fixed, i, c) {
                    continue;
                }
                let v = tmp[3 * i + c];
                if i < nv {
                    rc[3 * i + c] += v;
                } else {
                    let [x, y] = self.mid_edge[i - nv];
                    rc[3 * x as usize + c] += 0.5 * v;
                    rc[3 * y as usize + c] += 0.5 * v;
                }
            }
        }
        self.coarse.solve_in_place(rc.as_mut());
        // Prolong: z += M P xc.
        for i in 0..self.a.n {
            for c in 0..3 {
                if is_fixed(self.fixed, i, c) {
                    continue;
                }
                let v = if i < nv {
                    rc[3 * i + c]
                } else {
                    let [x, y] = self.mid_edge[i - nv];
                    0.5 * (rc[3 * x as usize + c] + rc[3 * y as usize + c])
                };
                z[3 * i + c] += v;
            }
        }
        self.gs_sweep(r, z, false);
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The PCG loop against an already built two-level preconditioner. `b` and `x0` must already
/// be zero on the fixed rows (the fixed entries of `x0` never move); the report's `dofs`
/// fields stay zero — the caller fills what it reports. `faer::set_global_parallelism` is the
/// caller's concern.
pub(crate) fn pcg_solve(
    a: &Bsr,
    pre: &TwoLevel,
    b: &[f64],
    x0: &[f64],
    opts: SolverOptions,
) -> Result<(Vec<f64>, SolveReport), String> {
    let n = 3 * a.n; // Bsr rows are node blocks; the PCG runs on the 3N DOF vectors.
    debug_assert_eq!(b.len(), n);
    debug_assert_eq!(x0.len(), n);
    let mut x = x0.to_vec();
    let mut report = SolveReport::default();

    let bnorm = dot(b, b).sqrt();
    if bnorm == 0.0 {
        report.converged = true;
        return Ok((x, report));
    }
    let mut ax = vec![0.0; n];
    a.mul(&x, &mut ax);
    let mut r: Vec<f64> = b.iter().zip(&ax).map(|(&bi, &ai)| bi - ai).collect();
    let mut z = vec![0.0; n];
    let mut tmp = vec![0.0; n];
    pre.apply(&r, &mut z, &mut tmp);
    let mut d = z.clone();
    let mut rz = dot(&r, &z);
    let mut ad = vec![0.0; n];
    for it in 1..=opts.max_iter {
        a.mul(&d, &mut ad);
        let dad = dot(&d, &ad);
        if dad <= 0.0 {
            return Err(
                "stiffness matrix is not positive definite; check that supports hold the part"
                    .into(),
            );
        }
        let alpha = rz / dad;
        for k in 0..n {
            x[k] += alpha * d[k];
            r[k] -= alpha * ad[k];
        }
        let rel = dot(&r, &r).sqrt() / bnorm;
        report.iterations = it;
        report.rel_residual = rel;
        if rel < opts.rtol {
            report.converged = true;
            break;
        }
        pre.apply(&r, &mut z, &mut tmp);
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for k in 0..n {
            d[k] = z[k] + beta * d[k];
        }
    }
    if !report.converged {
        return Err(format!(
            "solver did not converge in {} iterations (residual {:.2e})",
            report.iterations, report.rel_residual
        ));
    }
    Ok((x, report))
}

/// Solve the problem. Returns nodal displacements (3 per node, mm).
pub fn solve(p: &Problem, opts: SolverOptions) -> Result<(Vec<f64>, SolveReport), String> {
    faer::set_global_parallelism(faer::Parallelism::None);
    let n = p.mesh.ndofs();
    let mut a = assemble(&p.mesh, &p.material);
    apply_dirichlet(&mut a, &p.fixed);
    let mut b = p.force.clone();
    for i in 0..p.mesh.nodes.len() {
        for c in 0..3 {
            if is_fixed(&p.fixed, i, c) {
                b[3 * i + c] = 0.0;
            }
        }
    }
    let free = (0..p.mesh.nodes.len())
        .map(|i| 3 - p.fixed[i].count_ones() as usize)
        .sum();
    let pre = TwoLevel::new(&a, &p.fixed, p.mesh.nverts, &p.mesh.tets, &p.mesh.mid_edge)?;
    let mut report = SolveReport {
        dofs: n,
        free_dofs: free,
        coarse_dofs: p.mesh.nverts * 3,
        ..Default::default()
    };
    let (x, run) = pcg_solve(&a, &pre, &b, &vec![0.0; n], opts)?;
    report.iterations = run.iterations;
    report.rel_residual = run.rel_residual;
    report.converged = run.converged;
    Ok((x, report))
}
