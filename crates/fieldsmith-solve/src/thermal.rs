//! Steady linear conduction on the P2 mesh, and the thermal quantities the one-way
//! coupling needs (M14).
//!
//! Steady conduction on one isotropic material with constant conductivity is
//! ∇·(k∇T) = 0 → ∇²T = 0: the Laplace problem, conductivity-independent. Temperature
//! regions are Dirichlet boundaries (`Option<f64>` per node, `None` = insulated by the
//! natural boundary condition); the solve assembles the full Laplace matrix, lifts the
//! known values into the right-hand side, and solves the free system with the same
//! two-level P2→P1 structure as the elastic solve, block size 1.
//!
//! Convection (Robin) bands break the conductivity-independence: their boundary
//! condition `k ∂T/∂n = h(T_inf − T)` scales with the ratio `h/k`, and the unit
//! conductivity keeps `k = 1` W/(mm·K) (the materials sheet carries no conductivity
//! today) — so a band's `h` is read per that unit conductivity and mixed
//! Dirichlet/Robin fields carry the unit-solid's Biot number, not the real part's.
//! Pure-Dirichlet cases are unaffected. [`solve_conduction`] carries the per-element
//! `k` explicitly (one conductivity per element, relative to that unit solid):
//! all-ones is the pinned solve bit for bit, and a graded TPMS core passes its fill's
//! material map while the boundary statements keep their unit-solid reading.
//!
//! Thermal quantities: the element Laplacian, the thermal load
//! f[a·3+i] = (3λ+2μ)·α·∫ ΔT·∂φa/∂x_i dV (degree-3-exact quadrature — the integrand is
//! degree 3, one higher than the P2 stiffness), and nothing else; stress recovery with
//! the thermal term lives next to the elastic one in [`crate::stress`].

use crate::assemble::{element_map, RefData};
use crate::p2::P2Mesh;
use crate::pcg::{SolveReport, SolverOptions};
use crate::V3;
use faer::prelude::*;
use faer::Side;
use fenics_element::{interval_quadrature, FiniteElement, P2Tetrahedron, P2Triangle};

// ---------------------------------------------------------------------------
// Scalar CSR (the conduction system is one DOF per node).
// ---------------------------------------------------------------------------

/// Symmetric-pattern scalar CSR matrix (the 3×3 block [`crate::Bsr`] at block size 1).
#[derive(Clone, Debug, Default)]
pub struct Csr {
    pub n: usize,
    pub row_ptr: Vec<u32>,
    pub cols: Vec<u32>,
    pub vals: Vec<f64>,
}

impl Csr {
    /// Pattern from per-row neighbour lists (each must include the diagonal).
    pub fn from_adjacency(mut adj: Vec<Vec<u32>>) -> Csr {
        let n = adj.len();
        let mut row_ptr = Vec::with_capacity(n + 1);
        row_ptr.push(0u32);
        let mut cols = Vec::new();
        for row in adj.iter_mut() {
            row.sort_unstable();
            row.dedup();
            cols.extend_from_slice(row);
            row_ptr.push(cols.len() as u32);
        }
        let nnz = cols.len();
        Csr {
            n,
            row_ptr,
            cols,
            vals: vec![0.0; nnz],
        }
    }

    /// Pattern of the node adjacency of a set of 10-node elements.
    pub fn from_elements(n: usize, elems: impl Iterator<Item = [u32; 10]>) -> Csr {
        let mut adj: Vec<Vec<u32>> = (0..n as u32).map(|i| vec![i]).collect();
        for e in elems {
            for &a in &e {
                let row = &mut adj[a as usize];
                row.extend_from_slice(&e);
            }
        }
        Csr::from_adjacency(adj)
    }

    #[inline]
    pub fn find(&self, i: usize, j: u32) -> usize {
        let (s, e) = (self.row_ptr[i] as usize, self.row_ptr[i + 1] as usize);
        match self.cols[s..e].binary_search(&j) {
            Ok(k) => s + k,
            Err(_) => panic!("entry ({i}, {j}) not in pattern"),
        }
    }

    #[inline]
    pub fn add(&mut self, i: usize, j: u32, v: f64) {
        let k = self.find(i, j);
        self.vals[k] += v;
    }

    /// y = A x.
    pub fn mul(&self, x: &[f64], y: &mut [f64]) {
        for (i, yi) in y.iter_mut().enumerate() {
            let (s, e) = (self.row_ptr[i] as usize, self.row_ptr[i + 1] as usize);
            let mut acc = 0.0;
            for k in s..e {
                acc += self.vals[k] * x[self.cols[k] as usize];
            }
            *yi = acc;
        }
    }
}

/// Zero fixed rows/columns and put 1 on their diagonal (P2 value 0 there — the Dirichlet
/// values travel through the right-hand side lift instead, see [`solve_conduction`]).
fn apply_dirichlet(a: &mut Csr, fixed: &[bool]) {
    for i in 0..a.n {
        let (s, e) = (a.row_ptr[i] as usize, a.row_ptr[i + 1] as usize);
        for k in s..e {
            let j = a.cols[k] as usize;
            if fixed[i] || fixed[j] {
                a.vals[k] = if i == j { 1.0 } else { 0.0 };
            }
        }
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

// ---------------------------------------------------------------------------
// Scalar two-level preconditioner on the P2→P1 embedding (block size 1).
// ---------------------------------------------------------------------------

pub(crate) struct ScalarTwoLevel<'a> {
    a: &'a Csr,
    dinv: Vec<f64>,
    fixed: &'a [bool],
    nverts: usize,
    mid_edge: &'a [[u32; 2]],
    coarse: faer::sparse::linalg::solvers::Cholesky<usize, f64>,
}

impl<'a> ScalarTwoLevel<'a> {
    /// Same construction as the 3×3 [`crate::pcg::TwoLevel`], at block size 1: the vertex
    /// prolongation is the identity on vertices and the mean of the edge ends on midsides.
    pub(crate) fn new(
        a: &'a Csr,
        fixed: &'a [bool],
        nverts: usize,
        mid_edge: &'a [[u32; 2]],
    ) -> Result<ScalarTwoLevel<'a>, String> {
        let dinv = (0..a.n)
            .map(|i| {
                let d = a.vals[a.find(i, i as u32)];
                if d == 0.0 {
                    return 0.0; // empty/zero row: unit diagonal was inserted below
                }
                1.0 / d
            })
            .collect();
        // Coarse operator Ac = Pᵀ A P on the P1 (vertex) pattern — the vertex graph of the
        // P2 edges (same pattern the 3×3 two-level builds from the tets' vertex corners).
        let mut adj: Vec<Vec<u32>> = (0..nverts as u32).map(|i| vec![i]).collect();
        for &[x, y] in mid_edge {
            adj[x as usize].push(y);
            adj[y as usize].push(x);
        }
        let mut ac = Csr::from_adjacency(adj);
        let pmap = |i: usize| -> [(u32, f64); 2] {
            if i < nverts {
                [(i as u32, 1.0), (u32::MAX, 0.0)]
            } else {
                let [x, y] = mid_edge[i - nverts];
                [(x, 0.5), (y, 0.5)]
            }
        };
        for i in 0..a.n {
            let (s, e) = (a.row_ptr[i] as usize, a.row_ptr[i + 1] as usize);
            let pi = pmap(i);
            for k in s..e {
                let j = a.cols[k] as usize;
                let v = if fixed[i] || fixed[j] { 0.0 } else { a.vals[k] };
                let pj = pmap(j);
                for &(ci, wi) in &pi {
                    if ci == u32::MAX {
                        continue;
                    }
                    for &(cj, wj) in &pj {
                        if cj == u32::MAX {
                            continue;
                        }
                        ac.add(ci as usize, cj, v * wi * wj);
                    }
                }
            }
        }
        let mut trip: Vec<(usize, usize, f64)> = Vec::with_capacity(ac.cols.len());
        let mut row_has = vec![false; nverts];
        for (i, has) in row_has.iter_mut().enumerate() {
            let (s, e) = (ac.row_ptr[i] as usize, ac.row_ptr[i + 1] as usize);
            for k in s..e {
                let j = ac.cols[k] as usize;
                let v = ac.vals[k];
                if i >= j && v != 0.0 {
                    trip.push((i, j, v));
                    if i == j {
                        *has = true;
                    }
                }
            }
        }
        for (g, has) in row_has.iter().enumerate() {
            if !has {
                trip.push((g, g, 1.0));
            }
        }
        let m =
            faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(nverts, nverts, &trip)
                .map_err(|e| format!("coarse matrix: {e:?}"))?;
        let coarse = m.sp_cholesky(Side::Lower).map_err(|e| {
            format!("coarse factorisation failed ({e:?}); the thermal system may be singular — is at least one temperature region placed?")
        })?;
        Ok(ScalarTwoLevel {
            a,
            dinv,
            fixed,
            nverts,
            mid_edge,
            coarse,
        })
    }

    /// z = M⁻¹ r (symmetric V-cycle: forward GS, coarse correction, backward GS), skip
    /// fixed nodes.
    fn apply(&self, r: &[f64], z: &mut [f64], tmp: &mut [f64]) {
        z.iter_mut().for_each(|v| *v = 0.0);
        self.gs_sweep(r, z, true);
        self.a.mul(z, tmp);
        for (t, rv) in tmp.iter_mut().zip(r) {
            *t = rv - *t;
        }
        let nv = self.nverts;
        let mut rc = faer::Col::<f64>::zeros(nv);
        for i in 0..self.a.n {
            if self.fixed[i] {
                continue;
            }
            let v = tmp[i];
            if i < nv {
                rc[i] += v;
            } else {
                let [x, y] = self.mid_edge[i - nv];
                rc[x as usize] += 0.5 * v;
                rc[y as usize] += 0.5 * v;
            }
        }
        self.coarse.solve_in_place(rc.as_mut());
        for i in 0..self.a.n {
            if self.fixed[i] {
                continue;
            }
            let v = if i < nv {
                rc[i]
            } else {
                let [x, y] = self.mid_edge[i - nv];
                0.5 * (rc[x as usize] + rc[y as usize])
            };
            z[i] += v;
        }
        self.gs_sweep(r, z, false);
    }

    /// GS sweep writing 0 on fixed rows (the lifted system's residual is 0 there and
    /// fixed temperatures must not drift; the PCG's x0 carries 0 until the solve writes
    /// the Dirichlet values back).
    fn gs_sweep(&self, r: &[f64], x: &mut [f64], forward: bool) {
        let a = self.a;
        let mut sweep = |i: usize| {
            if self.fixed[i] {
                x[i] = 0.0;
                return;
            }
            let (s, e) = (a.row_ptr[i] as usize, a.row_ptr[i + 1] as usize);
            let mut acc = r[i];
            for k in s..e {
                let j = a.cols[k] as usize;
                if j == i {
                    continue;
                }
                acc -= a.vals[k] * x[j];
            }
            x[i] = self.dinv[i] * acc;
        };
        if forward {
            (0..a.n).for_each(&mut sweep);
        } else {
            (0..a.n).rev().for_each(&mut sweep);
        }
    }
}

/// PCG on the lifted conduction system (`b` is already 0 on fixed rows; `x0` = 0).
fn pcg_scalar(
    a: &Csr,
    pre: &ScalarTwoLevel,
    b: &[f64],
    opts: SolverOptions,
) -> Result<(Vec<f64>, SolveReport), String> {
    let n = a.n;
    let mut x = vec![0.0; n];
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
                "conduction matrix is not positive definite; check that a temperature region \
                 holds the field"
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

// ---------------------------------------------------------------------------
// Conduction solve.
// ---------------------------------------------------------------------------

/// A convection (Robin) band: `h·(T − t_inf)` per unit area across the listed boundary
/// facets, added to the conduction system as `K += h·MΓ`, `b += h·t_inf·∫φ dΓ` — the weak
/// form of `k ∂T/∂n = h(T_inf − T)` at the pinned conductivity `k = 1` W/(mm·K) (the
/// module comment names the consequence for mixed Dirichlet/Robin fields). `h` in
/// W/(mm²·K), `t_inf` in °C. Facet nodes follow [`crate::regions`]'s P2 facet order
/// `[c0, c1, c2, m01, m12, m20]`; only the surface metric enters the term, so facet
/// winding is irrelevant.
#[derive(Clone, Debug)]
pub struct RobinBc {
    pub h: f64,
    pub t_inf: f64,
    pub facets: Vec<[u32; 6]>,
}

/// A prescribed surface heat flux (Neumann) band: `q` per unit area across the listed
/// boundary facets, added to the conduction system as `b += q·∫φ dΓ` — `q` positive
/// heats the solid through that facet, negative draws heat out, and only the surface
/// metric enters, so facet winding is irrelevant (as with [`RobinBc`]). `q` in W/mm².
/// A flux band carries no matrix term, so it does not lift the Laplacian's constant
/// mode: a body whose only boundary data is heat flux has no steady solution (the
/// solvability gate still requires a fixed temperature or an `h > 0` convection).
#[derive(Clone, Debug)]
pub struct HeatFluxBc {
    pub q: f64,
    pub facets: Vec<[u32; 6]>,
}

/// World-space evaluator `(h(x), t_inf(x))` for [`SpatialRobinBc`].
pub type SpatialRobinEval<'a> = Box<dyn Fn(V3) -> Result<(f64, f64), String> + Sync + 'a>;

/// World-space evaluator `q(x)` for [`SpatialHeatFluxBc`].
pub type SpatialFluxEval<'a> = Box<dyn Fn(V3) -> Result<f64, String> + Sync + 'a>;

/// A spatially varying convection (Robin) band: `(h(x), t_inf(x))` evaluated in world
/// coordinates `[x, y, z]` (mm) at each of the 9 Gauss–Legendre points of every 6-node
/// boundary facet. Adds `m_h[a][b] = ∫ h(x) φ_a φ_b dΓ` to the conduction matrix and
/// `l_ht[a] = ∫ h(x) t_inf(x) φ_a dΓ` to the load vector.
pub struct SpatialRobinBc<'a> {
    pub facets: Vec<[u32; 6]>,
    pub eval: SpatialRobinEval<'a>,
}

/// A spatially varying prescribed surface heat flux (Neumann) band: `q(x)` (W/mm²)
/// evaluated in world coordinates `[x, y, z]` (mm) at each of the 9 Gauss–Legendre points
/// of every 6-node boundary facet, adding `l_q[a] = ∫ q(x) φ_a dΓ` to the load vector.
pub struct SpatialHeatFluxBc<'a> {
    pub facets: Vec<[u32; 6]>,
    pub eval: SpatialFluxEval<'a>,
}

/// Steady conduction on the P2 mesh with Dirichlet temperatures per node (`Some(t)` =
/// fixed °C, `None` = insulated), optional convection (Robin) bands, and optional
/// prescribed heat-flux bands. Returns nodal temperature (°C). Solvable iff at least
/// one node is fixed or one band has h > 0 — a wholly insulated body is unsolvable (the
/// Laplacian's constant mode), and heat flux alone does not fix the level either.
///
/// `k` is one conductivity per element (mesh order), relative to the pinned unit solid:
/// each element's Laplace block is scaled by its k, while the Robin and flux terms are
/// not — at the unit-solid reading the boundary statements carry their `h`/`q` against
/// k = 1 (module header). All-ones reproduces the pinned solve bit for bit; a graded
/// TPMS core passes its fill's material map, and the interface condition (flux
/// continuous) falls out of the shared weak form.
pub fn solve_conduction(
    mesh: &P2Mesh,
    d: &[Option<f64>],
    k: &[f64],
    robin: &[RobinBc],
    flux: &[HeatFluxBc],
    opts: SolverOptions,
) -> Result<(Vec<f64>, SolveReport), String> {
    solve_conduction_spatial(mesh, d, k, robin, &[], flux, &[], opts)
}

/// Assemble the unconstrained scalar P2 conduction matrix `A` (`k`-weighted Laplace + Robin
/// mass terms), unconstrained thermal load vector `q` (Robin `h · T_inf` + surface heat flux),
/// and Dirichlet node mask `fixed` (`d[i].is_some()`), validating all boundary inputs.
#[allow(clippy::too_many_arguments)]
pub fn assemble_conduction_system_spatial(
    mesh: &P2Mesh,
    d: &[Option<f64>],
    k: &[f64],
    robin: &[RobinBc],
    spatial_robin: &[SpatialRobinBc<'_>],
    flux: &[HeatFluxBc],
    spatial_flux: &[SpatialHeatFluxBc<'_>],
) -> Result<(Csr, Vec<f64>, Vec<bool>), String> {
    assemble_conduction_operators_spatial(
        mesh,
        d,
        k,
        robin,
        spatial_robin,
        flux,
        spatial_flux,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn assemble_conduction_operators_spatial(
    mesh: &P2Mesh,
    d: &[Option<f64>],
    k: &[f64],
    robin: &[RobinBc],
    spatial_robin: &[SpatialRobinBc<'_>],
    flux: &[HeatFluxBc],
    spatial_flux: &[SpatialHeatFluxBc<'_>],
    require_steady_anchor: bool,
) -> Result<(Csr, Vec<f64>, Vec<bool>), String> {
    let n = mesh.nodes.len();
    assert_eq!(d.len(), n, "one temperature statement (or None) per node");
    for (bi, bc) in robin.iter().enumerate() {
        if !(bc.h.is_finite() && bc.h >= 0.0) {
            return Err(format!(
                "convection band {bi}: h must be finite and ≥ 0 (got {})",
                bc.h
            ));
        }
        if !bc.t_inf.is_finite() {
            return Err(format!(
                "convection band {bi}: t_inf must be finite (got {})",
                bc.t_inf
            ));
        }
    }
    for (bi, bc) in flux.iter().enumerate() {
        if !bc.q.is_finite() {
            return Err(format!(
                "heat flux band {bi}: q must be finite (got {})",
                bc.q
            ));
        }
    }
    let fixed: Vec<bool> = d.iter().map(|x| x.is_some()).collect();
    assert_eq!(k.len(), mesh.tets.len(), "one conductivity per element");
    for (e, &ke) in k.iter().enumerate() {
        if !(ke.is_finite() && ke > 0.0) {
            return Err(format!(
                "conduction conductivity: element {e} carries k = {ke} — k must be finite \
                 and > 0 (the value scales the element's Laplace block)"
            ));
        }
    }
    let no_fixed_or_uniform_h = !fixed.iter().any(|&f| f) && robin.iter().all(|bc| bc.h <= 0.0);
    if require_steady_anchor && spatial_robin.is_empty() && no_fixed_or_uniform_h {
        return Err(if flux.is_empty() && spatial_flux.is_empty() {
            "conduction has no fixed temperature and no convection band with h > 0 — every \
             face would be insulated; place at least one temperature region or a convection"
                .into()
        } else {
            "conduction has no fixed temperature and no convection band with h > 0 — a heat \
             flux q alone does not fix the field's level; place at least one temperature \
             region or a convection"
                .into()
        });
    }
    let mut a = assemble_laplace_k(mesh, k);
    // Assemble the Robin terms (before the Dirichlet lift, so a shared Dirichlet/Robin
    // node's row is driven by the prescribed value — Dirichlet wins).
    let mut load = vec![0.0; n];
    for bc in robin {
        for f in &bc.facets {
            let (m, l) = element_robin(mesh, f);
            for i in 0..6 {
                let ni = f[i] as usize;
                load[ni] += bc.h * bc.t_inf * l[i];
                for j in 0..6 {
                    a.add(ni, f[j], bc.h * m[i][j]);
                }
            }
        }
    }
    let mut any_positive_spatial_h = false;
    for (bi, bc) in spatial_robin.iter().enumerate() {
        for f in &bc.facets {
            let (m_h, l_ht, max_h) = element_robin_spatial(mesh, f, &*bc.eval)
                .map_err(|e| format!("convection band {bi}: {e}"))?;
            if max_h > 0.0 {
                any_positive_spatial_h = true;
            }
            for i in 0..6 {
                let ni = f[i] as usize;
                load[ni] += l_ht[i];
                for j in 0..6 {
                    a.add(ni, f[j], m_h[i][j]);
                }
            }
        }
    }
    // Heat-flux bands join the same load vector — the facet integral is the flux's
    // whole contribution; no matrix term. Facets on fixed rows are lifted to zero with
    // the rest of the right-hand side.
    for bc in flux {
        for f in &bc.facets {
            let l = element_flux(mesh, f);
            for i in 0..6 {
                load[f[i] as usize] += bc.q * l[i];
            }
        }
    }
    for (bi, bc) in spatial_flux.iter().enumerate() {
        for f in &bc.facets {
            let l_q = element_flux_spatial(mesh, f, &*bc.eval)
                .map_err(|e| format!("heat flux band {bi}: {e}"))?;
            for i in 0..6 {
                load[f[i] as usize] += l_q[i];
            }
        }
    }
    if require_steady_anchor && no_fixed_or_uniform_h && !any_positive_spatial_h {
        return Err(if flux.is_empty() && spatial_flux.is_empty() {
            "conduction has no fixed temperature and no convection band with h > 0 — every \
             face would be insulated; place at least one temperature region or a convection"
                .into()
        } else {
            "conduction has no fixed temperature and no convection band with h > 0 — a heat \
             flux q alone does not fix the field's level; place at least one temperature \
             region or a convection"
                .into()
        });
    }
    Ok((a, load, fixed))
}

/// Steady conduction on the P2 mesh accepting both uniform [`RobinBc`]/[`HeatFluxBc`] and
/// world-space [`SpatialRobinBc`]/[`SpatialHeatFluxBc`] boundary terms. When
/// `spatial_robin` and `spatial_flux` are empty, arithmetic is bit-for-bit identical to
/// [`solve_conduction`].
#[allow(clippy::too_many_arguments)]
pub fn solve_conduction_spatial(
    mesh: &P2Mesh,
    d: &[Option<f64>],
    k: &[f64],
    robin: &[RobinBc],
    spatial_robin: &[SpatialRobinBc<'_>],
    flux: &[HeatFluxBc],
    spatial_flux: &[SpatialHeatFluxBc<'_>],
    opts: SolverOptions,
) -> Result<(Vec<f64>, SolveReport), String> {
    faer::set_global_parallelism(faer::Parallelism::None);
    let n = mesh.nodes.len();
    let (mut a, load, fixed) =
        assemble_conduction_system_spatial(mesh, d, k, robin, spatial_robin, flux, spatial_flux)?;
    // Lift the Dirichlet values into the right-hand side: solve A_ff x = −A_fd·s_d, then
    // write the fixed values back. Symmetric one-sided lift (full matrix times the
    // Dirichlet vector, fixed rows of the rhs zeroed) keeps A symmetric for PCG; the
    // convection load joins the free rows here.
    let s: Vec<f64> = d.iter().map(|x| x.unwrap_or(0.0)).collect();
    let mut b = vec![0.0; n];
    a.mul(&s, &mut b);
    for (bi, (li, &f)) in b.iter_mut().zip(load.iter().zip(&fixed)) {
        *bi = if f { 0.0 } else { -*bi + *li };
    }
    apply_dirichlet(&mut a, &fixed);
    let pre = ScalarTwoLevel::new(&a, &fixed, mesh.nverts, &mesh.mid_edge)?;
    let mut report = SolveReport {
        dofs: n,
        free_dofs: n - fixed.iter().filter(|&&f| f).count(),
        coarse_dofs: mesh.nverts,
        ..Default::default()
    };
    let (mut x, run) = pcg_scalar(&a, &pre, &b, opts)?;
    report.iterations = run.iterations;
    report.rel_residual = run.rel_residual;
    report.converged = run.converged;
    for (xi, (di, &f)) in x.iter_mut().zip(d.iter().zip(&fixed)) {
        if f {
            *xi = di.unwrap();
        }
    }
    Ok((x, report))
}

/// Solve multiple homogeneous-Dirichlet conduction right-hand sides `A_ff λ_m = rhs_m`
/// reusing a single assembled scalar CSR operator and a single `ScalarTwoLevel` Cholesky
/// preconditioner. Used by staggered discrete thermo-elastic adjoints (`K_T λ_T = B_th^T λ_u`).
#[allow(clippy::too_many_arguments)]
pub fn solve_conduction_multi_rhs(
    mesh: &P2Mesh,
    d: &[Option<f64>],
    k: &[f64],
    robin: &[RobinBc],
    spatial_robin: &[SpatialRobinBc<'_>],
    flux: &[HeatFluxBc],
    spatial_flux: &[SpatialHeatFluxBc<'_>],
    rhs_list: &[Vec<f64>],
    opts: SolverOptions,
) -> Result<Vec<(Vec<f64>, SolveReport)>, String> {
    if rhs_list.is_empty() {
        return Ok(Vec::new());
    }
    faer::set_global_parallelism(faer::Parallelism::None);
    let n = mesh.nodes.len();
    let (mut a, _load, fixed) =
        assemble_conduction_system_spatial(mesh, d, k, robin, spatial_robin, flux, spatial_flux)?;
    apply_dirichlet(&mut a, &fixed);
    let pre = ScalarTwoLevel::new(&a, &fixed, mesh.nverts, &mesh.mid_edge)?;
    let free_dofs = n - fixed.iter().filter(|&&f| f).count();
    let mut out = Vec::with_capacity(rhs_list.len());
    for rhs in rhs_list {
        if rhs.len() != n {
            return Err(format!(
                "conduction adjoint RHS length {} does not match mesh nodes {}",
                rhs.len(),
                n
            ));
        }
        let mut b = rhs.clone();
        for (bi, &f) in b.iter_mut().zip(&fixed) {
            if f {
                *bi = 0.0;
            }
        }
        let (x, run) = pcg_scalar(&a, &pre, &b, opts)?;
        out.push((
            x,
            SolveReport {
                dofs: n,
                free_dofs,
                coarse_dofs: mesh.nverts,
                iterations: run.iterations,
                rel_residual: run.rel_residual,
                converged: run.converged,
                ..Default::default()
            },
        ));
    }
    Ok(out)
}

/// One time-step snapshot from [`solve_transient_conduction_spatial`].
#[derive(Clone, Debug)]
pub struct TransientConductionFrame {
    pub step: usize,
    pub time_s: f64,
    pub temperature: Vec<f64>,
    pub report: SolveReport,
}

/// Unconditionally A-stable 3D P2 transient conduction (`ρ c_p ∂T/∂t − ∇·(k ∇T) = 0`)
/// with uniform Robin/Neumann boundary terms, delegating to
/// [`solve_transient_conduction_spatial`].
#[allow(clippy::too_many_arguments)]
pub fn solve_transient_conduction(
    mesh: &P2Mesh,
    d: &[Option<f64>],
    k_phys: &[f64],
    c_vol: &[f64],
    t_init: f64,
    t_end: f64,
    steps: usize,
    robin: &[RobinBc],
    flux: &[HeatFluxBc],
    opts: SolverOptions,
) -> Result<Vec<TransientConductionFrame>, String> {
    solve_transient_conduction_spatial(
        mesh,
        d,
        k_phys,
        c_vol,
        t_init,
        t_end,
        steps,
        robin,
        &[],
        flux,
        &[],
        opts,
    )
}

/// Unconditionally A-stable 3D P2 transient conduction (`c_vol ∂T/∂t − ∇·(k_phys ∇T) = 0`)
/// using BDF1 on step 1 (`t_1 = Δt`) and second-order constant-operator **BDF2** on steps
/// `2..=steps`, reusing a single `ScalarTwoLevel` Cholesky coarse preconditioner across all
/// BDF2 time steps.
///
/// Units are self-consistent `mm · W · s · K`:
/// - `k_phys[e]`: element conductivity in `W/(mm·K)` (`> 0`)
/// - `c_vol[e]`: element volumetric heat capacity `ρ · c_p` in `J/(mm³·K)` (`> 0`)
/// - `t_init`: initial solid temperature `T(x, 0⁻)` in `°C`
/// - `t_end`: end time in `s` (`> 0`)
/// - `steps`: uniform step count (`Δt = t_end / steps`, `>= 1`)
///
/// Returns `steps + 1` frames (`step = 0` at `t = 0` through `step = steps` at `t = t_end`).
#[allow(clippy::too_many_arguments)]
pub fn solve_transient_conduction_spatial(
    mesh: &P2Mesh,
    d: &[Option<f64>],
    k_phys: &[f64],
    c_vol: &[f64],
    t_init: f64,
    t_end: f64,
    steps: usize,
    robin: &[RobinBc],
    spatial_robin: &[SpatialRobinBc<'_>],
    flux: &[HeatFluxBc],
    spatial_flux: &[SpatialHeatFluxBc<'_>],
    opts: SolverOptions,
) -> Result<Vec<TransientConductionFrame>, String> {
    faer::set_global_parallelism(faer::Parallelism::None);
    if !t_init.is_finite() {
        return Err(format!("transient t_init must be finite (got {t_init})"));
    }
    if !(t_end.is_finite() && t_end > 0.0) {
        return Err(format!("transient t_end must be > 0 (got {t_end})"));
    }
    if steps == 0 {
        return Err("transient steps must be >= 1".into());
    }
    let n = mesh.nodes.len();
    assert_eq!(
        c_vol.len(),
        mesh.tets.len(),
        "one volumetric heat capacity per element"
    );
    for (e, &ce) in c_vol.iter().enumerate() {
        if !(ce.is_finite() && ce > 0.0) {
            return Err(format!(
                "transient volumetric heat capacity: element {e} carries c_vol = {ce} — must be finite and > 0"
            ));
        }
    }
    let (a, load, fixed) = assemble_conduction_operators_spatial(
        mesh,
        d,
        k_phys,
        robin,
        spatial_robin,
        flux,
        spatial_flux,
        false,
    )?;
    let m_mat = assemble_thermal_mass(mesh, c_vol);
    debug_assert_eq!(m_mat.row_ptr, a.row_ptr);
    debug_assert_eq!(m_mat.cols, a.cols);

    // Method-of-Lines static forcing on free DOFs: `rhs_static_f = load_f - A_fd s_d`
    // (`d/dt(s_d) = 0` for `t > 0` on time-independent Dirichlet boundaries).
    let s: Vec<f64> = d.iter().map(|x| x.unwrap_or(0.0)).collect();
    let mut a_s = vec![0.0; n];
    a.mul(&s, &mut a_s);
    let rhs_static: Vec<f64> = (0..n)
        .map(|i| if fixed[i] { 0.0 } else { load[i] - a_s[i] })
        .collect();
    let free_dofs = n - fixed.iter().filter(|&&f| f).count();
    let dt = t_end / (steps as f64);

    let mut frames = Vec::with_capacity(steps + 1);
    frames.push(TransientConductionFrame {
        step: 0,
        time_s: 0.0,
        temperature: vec![t_init; n],
        report: SolveReport {
            dofs: n,
            free_dofs,
            coarse_dofs: mesh.nverts,
            iterations: 0,
            rel_residual: 0.0,
            converged: true,
            ..Default::default()
        },
    });

    // Step 1: BDF1 `( (1/dt) M_ff + A_ff ) T^1_f = rhs_static_f + (1/dt) M_ff T^0_f`.
    let gamma1 = 1.0 / dt;
    let mut s1 = Csr {
        n: a.n,
        row_ptr: a.row_ptr.clone(),
        cols: a.cols.clone(),
        vals: m_mat
            .vals
            .iter()
            .zip(&a.vals)
            .map(|(&mv, &av)| gamma1 * mv + av)
            .collect(),
    };
    apply_dirichlet(&mut s1, &fixed);
    let pre1 = ScalarTwoLevel::new(&s1, &fixed, mesh.nverts, &mesh.mid_edge)?;

    let mut h_free: Vec<f64> = (0..n)
        .map(|j| {
            if fixed[j] {
                0.0
            } else {
                gamma1 * frames[0].temperature[j]
            }
        })
        .collect();
    let mut m_h = vec![0.0; n];
    m_mat.mul(&h_free, &mut m_h);
    let mut b: Vec<f64> = (0..n)
        .map(|i| {
            if fixed[i] {
                0.0
            } else {
                rhs_static[i] + m_h[i]
            }
        })
        .collect();
    let (mut t1, run1) = pcg_scalar(&s1, &pre1, &b, opts)?;
    for (xi, (di, &f)) in t1.iter_mut().zip(d.iter().zip(&fixed)) {
        if f {
            *xi = di.unwrap();
        }
    }
    frames.push(TransientConductionFrame {
        step: 1,
        time_s: if steps == 1 { t_end } else { dt },
        temperature: t1,
        report: SolveReport {
            dofs: n,
            free_dofs,
            coarse_dofs: mesh.nverts,
            iterations: run1.iterations,
            rel_residual: run1.rel_residual,
            converged: run1.converged,
            ..Default::default()
        },
    });

    if steps >= 2 {
        // Steps 2..=steps: BDF2 `( (1.5/dt) M_ff + A_ff ) T^m_f = rhs_static_f + M_ff ( (2/dt) T^{m-1}_f - (0.5/dt) T^{m-2}_f )`.
        // Factor `ScalarTwoLevel` ONCE and reuse across all remaining steps.
        let gamma2 = 1.5 / dt;
        let c_prev1 = 2.0 / dt;
        let c_prev2 = -0.5 / dt;
        let mut s2 = Csr {
            n: a.n,
            row_ptr: a.row_ptr.clone(),
            cols: a.cols.clone(),
            vals: m_mat
                .vals
                .iter()
                .zip(&a.vals)
                .map(|(&mv, &av)| gamma2 * mv + av)
                .collect(),
        };
        apply_dirichlet(&mut s2, &fixed);
        let pre2 = ScalarTwoLevel::new(&s2, &fixed, mesh.nverts, &mesh.mid_edge)?;

        for m in 2..=steps {
            let t_prev1 = &frames[m - 1].temperature;
            let t_prev2 = &frames[m - 2].temperature;
            for j in 0..n {
                h_free[j] = if fixed[j] {
                    0.0
                } else {
                    c_prev1 * t_prev1[j] + c_prev2 * t_prev2[j]
                };
            }
            m_mat.mul(&h_free, &mut m_h);
            for i in 0..n {
                b[i] = if fixed[i] {
                    0.0
                } else {
                    rhs_static[i] + m_h[i]
                };
            }
            let (mut tm, runm) = pcg_scalar(&s2, &pre2, &b, opts)?;
            for (xi, (di, &f)) in tm.iter_mut().zip(d.iter().zip(&fixed)) {
                if f {
                    *xi = di.unwrap();
                }
            }
            let time_s = if m == steps { t_end } else { (m as f64) * dt };
            frames.push(TransientConductionFrame {
                step: m,
                time_s,
                temperature: tm,
                report: SolveReport {
                    dofs: n,
                    free_dofs,
                    coarse_dofs: mesh.nverts,
                    iterations: runm.iterations,
                    rel_residual: runm.rel_residual,
                    converged: runm.converged,
                    ..Default::default()
                },
            });
        }
    }

    Ok(frames)
}

// ---------------------------------------------------------------------------
// Element quantities.
// ---------------------------------------------------------------------------

/// Exact reference-tetrahedron products `∫ φ_a φ_b dV_ref` for the 10-node P2 basis
/// (`[f64; 100]` indexed `a * 10 + b`), computed once from exact barycentric monomial
/// moments `m0! m1! m2! m3! / (m0 + m1 + m2 + m3 + 3)!` on [`P2Tetrahedron`].
fn reference_mass_products_p2() -> &'static [f64; 100] {
    static PRODUCTS: std::sync::OnceLock<[f64; 100]> = std::sync::OnceLock::new();
    PRODUCTS.get_or_init(|| {
        const FACTORIALS: [f64; 12] = [
            1.0, 1.0, 2.0, 6.0, 24.0, 120.0, 720.0, 5040.0, 40320.0, 362880.0, 3628800.0,
            39916800.0,
        ];
        let nodes = P2Tetrahedron.nodal_coordinates();
        let terms: Vec<Vec<([u8; 4], f64)>> = nodes
            .iter()
            .map(|&c| {
                let lam = [1.0 - c[0] - c[1] - c[2], c[0], c[1], c[2]];
                let unit: Vec<usize> = (0..4).filter(|&i| (lam[i] - 1.0).abs() < 1e-9).collect();
                let half: Vec<usize> = (0..4).filter(|&i| (lam[i] - 0.5).abs() < 1e-9).collect();
                let mono = |i: usize, k: u8| {
                    let mut m = [0u8; 4];
                    m[i] = k;
                    m
                };
                match (unit.len(), half.len()) {
                    (1, 0) => vec![(mono(unit[0], 2), 2.0), (mono(unit[0], 1), -1.0)],
                    (0, 2) => {
                        let mut m = [0u8; 4];
                        m[half[0]] = 1;
                        m[half[1]] = 1;
                        vec![(m, 4.0)]
                    }
                    _ => panic!("P2Tetrahedron nodes must be corner or edge-midpoint positions"),
                }
            })
            .collect();
        let mut products = [0.0f64; 100];
        for (a, ta) in terms.iter().enumerate() {
            for (b, tb) in terms.iter().enumerate() {
                let mut acc = 0.0f64;
                for &(ma, ca) in ta {
                    for &(mb, cb) in tb {
                        let m = [ma[0] + mb[0], ma[1] + mb[1], ma[2] + mb[2], ma[3] + mb[3]];
                        let deg = (m[0] + m[1] + m[2] + m[3]) as usize;
                        acc += ca
                            * cb
                            * FACTORIALS[m[0] as usize]
                            * FACTORIALS[m[1] as usize]
                            * FACTORIALS[m[2] as usize]
                            * FACTORIALS[m[3] as usize]
                            / FACTORIALS[deg + 3];
                    }
                }
                products[a * 10 + b] = acc;
            }
        }
        products
    })
}

/// Assemble the exact degree-4 scalar P2 thermal capacity matrix
/// `M[a, b] = ∑_e c_vol[e] ∫_{Ω_e} φ_a φ_b dV` (`J/K`), sharing the exact CSR sparsity
/// pattern of [`assemble_laplace_k`].
pub fn assemble_thermal_mass(mesh: &P2Mesh, c_vol: &[f64]) -> Csr {
    assert_eq!(
        c_vol.len(),
        mesh.tets.len(),
        "one volumetric heat capacity per element"
    );
    let products = reference_mass_products_p2();
    let mut m = Csr::from_elements(mesh.nodes.len(), mesh.tets.iter().copied());
    for (e, &ce) in c_vol.iter().enumerate() {
        let det = element_map(mesh, e).det_jacobian.abs();
        let scale = ce * det;
        let t = &mesh.tets[e];
        for i in 0..10 {
            let ni = t[i] as usize;
            for j in 0..10 {
                m.add(ni, t[j], scale * products[i * 10 + j]);
            }
        }
    }
    m
}

/// Element Laplacian L[a][b] = ∫ ∇φa·∇φb dV, degree-2-exact on affine elements.
pub fn element_laplace(mesh: &P2Mesh, e: usize, rd: &RefData) -> [[f64; 10]; 10] {
    let map = element_map(mesh, e);
    let det = map.det_jacobian.abs();
    let mut l = [[0.0f64; 10]; 10];
    let mut g = [[0.0f64; 3]; 10];
    for (q, w) in rd.weights.iter().enumerate() {
        for (ga, grad) in g.iter_mut().zip(rd.grads[q].iter()) {
            *ga = map.transform_gradient(grad);
        }
        let s = w * det;
        for a in 0..10 {
            for b in 0..10 {
                l[a][b] += s * (g[a][0] * g[b][0] + g[a][1] * g[b][1] + g[a][2] * g[b][2]);
            }
        }
    }
    l
}

/// Global Laplace matrix at unit conductivity, no boundary conditions applied —
/// [`assemble_laplace_k`] with k ≡ 1. Multiplication by 1.0 is exact (IEEE), so the
/// entries match an unscaled assembly bit for bit.
pub fn assemble_laplace(mesh: &P2Mesh) -> Csr {
    assemble_laplace_k(mesh, &vec![1.0; mesh.tets.len()])
}

/// Global Laplace matrix (no boundary conditions applied) scaled elementwise by `k`,
/// one conductivity per element in mesh order — the graded cores' per-element material
/// map. All-ones is bit-identical to the unscaled assembly.
pub fn assemble_laplace_k(mesh: &P2Mesh, k: &[f64]) -> Csr {
    assert_eq!(k.len(), mesh.tets.len(), "one conductivity per element");
    let rd = RefData::new();
    let mut a = Csr::from_elements(mesh.nodes.len(), mesh.tets.iter().copied());
    for (e, &ke) in k.iter().enumerate() {
        let le = element_laplace(mesh, e, &rd);
        let t = &mesh.tets[e];
        for i in 0..10 {
            for j in 0..10 {
                a.add(t[i] as usize, t[j], ke * le[i][j]);
            }
        }
    }
    a
}

/// Local Robin (convection) integrals for one boundary facet, on the facet's own
/// isoparametric map: `(m, l)` with `m[a][b] = ∫φa φb dΓ` and `l[a] = ∫φa dΓ`. The mass
/// integrand is degree 4 on an affine facet, one past [`fenics_element::triangle_quadrature`]'s
/// degree-3 cap, so each direction carries a 3-point Gauss–Legendre rule through the Duffy
/// map ξ = (u, v·(1−u)) — exact to degree 4, 9 points per facet. The rule is exact on any
/// affine facet regardless of winding (only |t1×t2| enters); a curved facet's jacobian
/// would need a higher rule, and no supported geometry produces one.
pub fn element_robin(mesh: &P2Mesh, nodes: &[u32; 6]) -> ([[f64; 6]; 6], [f64; 6]) {
    let p: [V3; 6] = nodes.map(|f| mesh.nodes[f as usize]);
    let gl = interval_quadrature(5);
    let elem = P2Triangle;
    let mut m = [[0.0f64; 6]; 6];
    let mut l = [0.0f64; 6];
    for wu in &gl.points {
        let u = wu.point[0];
        for wv in &gl.points {
            let v = wv.point[0];
            // Duffy: the (ξ, η) triangle swept as v·(1−u) for fixed u, jacobian 1−u.
            let xi = [u, v * (1.0 - u)];
            let s = wu.weight * wv.weight * (1.0 - u);
            let n = elem.evaluate_basis(&xi);
            let dn = elem.evaluate_gradients(&xi);
            let mut t1 = [0.0f64; 3];
            let mut t2 = [0.0f64; 3];
            for (k, &dnk) in dn.iter().enumerate() {
                for c in 0..3 {
                    t1[c] += dnk[0] * p[k][c];
                    t2[c] += dnk[1] * p[k][c];
                }
            }
            let cx = t1[1] * t2[2] - t1[2] * t2[1];
            let cy = t1[2] * t2[0] - t1[0] * t2[2];
            let cz = t1[0] * t2[1] - t1[1] * t2[0];
            let j = (cx * cx + cy * cy + cz * cz).sqrt();
            for a in 0..6 {
                l[a] += s * j * n[a];
                for b in 0..6 {
                    m[a][b] += s * j * n[a] * n[b];
                }
            }
        }
    }
    (m, l)
}

/// Nodal surface-flux integrals for one boundary facet: `flux[a] = ∫φa dΓ` — the facet
/// mass row [`element_robin`] returns, the prescribed surface heat flux's whole weak
/// contribution (no matrix term). Same quadrature, same winding irrelevance.
pub fn element_flux(mesh: &P2Mesh, nodes: &[u32; 6]) -> [f64; 6] {
    element_robin(mesh, nodes).1
}

/// 4-point Gauss–Legendre quadrature on `[0, 1]` (exact up to polynomial degree 7).
/// Through the Duffy map `ξ = (u, v·(1−u))` (16 points per facet), this integrates
/// `h(x)·φa·φb·(1−u)` up to quadratic `h(x)` (total degree `2 + 2 + 2 + 1 = 7`) and
/// `q(x)·φa·(1−u)` up to quartic `q(x)` (total degree `4 + 2 + 1 = 7`) to machine precision
/// on affine P2 facets.
fn gl4_unit_interval() -> [(f64, f64); 4] {
    let s6_5 = (6.0_f64 / 5.0).sqrt();
    let r1 = (3.0 / 7.0 - (2.0 / 7.0) * s6_5).sqrt();
    let r2 = (3.0 / 7.0 + (2.0 / 7.0) * s6_5).sqrt();
    let s30 = 30.0_f64.sqrt();
    let w1 = (18.0 + s30) / 72.0;
    let w2 = (18.0 - s30) / 72.0;
    [
        (0.5 * (1.0 - r2), w2),
        (0.5 * (1.0 - r1), w1),
        (0.5 * (1.0 + r1), w1),
        (0.5 * (1.0 + r2), w2),
    ]
}

/// Facet Robin quadrature result `(m_h, l_ht, max_h)`.
pub type RobinFacetQuad = ([[f64; 6]; 6], [f64; 6], f64);

/// Local spatially varying Robin integrals on one 6-node P2 boundary facet using the
/// 16-point (4×4 degree-7) Duffy Gauss–Legendre rule: returns `(m_h, l_ht, max_h)` where
/// `m_h[a][b] = ∫ h(x) φa φb dΓ` and `l_ht[a] = ∫ h(x) t_inf(x) φa dΓ`.
pub fn element_robin_spatial<F>(
    mesh: &P2Mesh,
    nodes: &[u32; 6],
    eval: F,
) -> Result<RobinFacetQuad, String>
where
    F: Fn(V3) -> Result<(f64, f64), String>,
{
    let p: [V3; 6] = nodes.map(|f| mesh.nodes[f as usize]);
    let gl = gl4_unit_interval();
    let elem = P2Triangle;
    let mut m = [[0.0f64; 6]; 6];
    let mut l = [0.0f64; 6];
    let mut max_h = 0.0f64;
    for &(u, wu) in &gl {
        for &(v, wv) in &gl {
            let xi = [u, v * (1.0 - u)];
            let s = wu * wv * (1.0 - u);
            let n = elem.evaluate_basis(&xi);
            let dn = elem.evaluate_gradients(&xi);
            let mut x_gp = [0.0f64; 3];
            let mut t1 = [0.0f64; 3];
            let mut t2 = [0.0f64; 3];
            for (k, (&nk, &dnk)) in n.iter().zip(dn.iter()).enumerate() {
                for c in 0..3 {
                    x_gp[c] += nk * p[k][c];
                    t1[c] += dnk[0] * p[k][c];
                    t2[c] += dnk[1] * p[k][c];
                }
            }
            let (h, t_inf) = eval(x_gp)?;
            if !(h.is_finite() && h >= 0.0) {
                return Err(format!(
                    "h({:.4}, {:.4}, {:.4}) must be finite and ≥ 0 (got {h})",
                    x_gp[0], x_gp[1], x_gp[2]
                ));
            }
            if !t_inf.is_finite() {
                return Err(format!(
                    "t_inf({:.4}, {:.4}, {:.4}) must be finite (got {t_inf})",
                    x_gp[0], x_gp[1], x_gp[2]
                ));
            }
            if h > max_h {
                max_h = h;
            }
            let cx = t1[1] * t2[2] - t1[2] * t2[1];
            let cy = t1[2] * t2[0] - t1[0] * t2[2];
            let cz = t1[0] * t2[1] - t1[1] * t2[0];
            let dgamma = s * (cx * cx + cy * cy + cz * cz).sqrt();
            for a in 0..6 {
                l[a] += dgamma * h * t_inf * n[a];
                for b in 0..6 {
                    m[a][b] += dgamma * h * n[a] * n[b];
                }
            }
        }
    }
    Ok((m, l, max_h))
}

/// Nodal spatially varying surface-flux integrals on one 6-node P2 boundary facet:
/// `flux[a] = ∫ q(x) φa dΓ` via the 16-point (4×4 degree-7) Duffy Gauss–Legendre rule.
pub fn element_flux_spatial<F>(mesh: &P2Mesh, nodes: &[u32; 6], eval: F) -> Result<[f64; 6], String>
where
    F: Fn(V3) -> Result<f64, String>,
{
    let p: [V3; 6] = nodes.map(|f| mesh.nodes[f as usize]);
    let gl = gl4_unit_interval();
    let elem = P2Triangle;
    let mut l = [0.0f64; 6];
    for &(u, wu) in &gl {
        for &(v, wv) in &gl {
            let xi = [u, v * (1.0 - u)];
            let s = wu * wv * (1.0 - u);
            let n = elem.evaluate_basis(&xi);
            let dn = elem.evaluate_gradients(&xi);
            let mut x_gp = [0.0f64; 3];
            let mut t1 = [0.0f64; 3];
            let mut t2 = [0.0f64; 3];
            for (k, (&nk, &dnk)) in n.iter().zip(dn.iter()).enumerate() {
                for c in 0..3 {
                    x_gp[c] += nk * p[k][c];
                    t1[c] += dnk[0] * p[k][c];
                    t2[c] += dnk[1] * p[k][c];
                }
            }
            let q = eval(x_gp)?;
            if !q.is_finite() {
                return Err(format!(
                    "q({:.4}, {:.4}, {:.4}) must be finite (got {q})",
                    x_gp[0], x_gp[1], x_gp[2]
                ));
            }
            let cx = t1[1] * t2[2] - t1[2] * t2[1];
            let cy = t1[2] * t2[0] - t1[0] * t2[2];
            let cz = t1[0] * t2[1] - t1[1] * t2[0];
            let dgamma = s * (cx * cx + cy * cy + cz * cz).sqrt();
            for a in 0..6 {
                l[a] += dgamma * q * n[a];
            }
        }
    }
    Ok(l)
}

/// Consistent P2 nodal traction forces, resultant force, and origin moment for a spatially
/// varying normal pressure `p(x)` (MPa, positive inward `-p(x)·n_out`) on one 6-node facet,
/// integrated via the 16-point (4×4 degree-7) Duffy Gauss–Legendre rule.
pub fn element_pressure_spatial<F>(
    mesh: &P2Mesh,
    nodes: &[u32; 6],
    n_out: V3,
    eval: F,
) -> Result<([V3; 6], V3, V3), String>
where
    F: Fn(V3) -> Result<f64, String>,
{
    let p: [V3; 6] = nodes.map(|f| mesh.nodes[f as usize]);
    let gl = gl4_unit_interval();
    let elem = P2Triangle;
    let mut nodal_f = [[0.0f64; 3]; 6];
    let mut resultant = [0.0f64; 3];
    let mut moment = [0.0f64; 3];
    for &(u, wu) in &gl {
        for &(v, wv) in &gl {
            let xi = [u, v * (1.0 - u)];
            let s = wu * wv * (1.0 - u);
            let n = elem.evaluate_basis(&xi);
            let dn = elem.evaluate_gradients(&xi);
            let mut x_gp = [0.0f64; 3];
            let mut t1 = [0.0f64; 3];
            let mut t2 = [0.0f64; 3];
            for (k, (&nk, &dnk)) in n.iter().zip(dn.iter()).enumerate() {
                for c in 0..3 {
                    x_gp[c] += nk * p[k][c];
                    t1[c] += dnk[0] * p[k][c];
                    t2[c] += dnk[1] * p[k][c];
                }
            }
            let mpa = eval(x_gp)?;
            if !mpa.is_finite() {
                return Err(format!(
                    "pressure({:.4}, {:.4}, {:.4}) must be finite (got {mpa})",
                    x_gp[0], x_gp[1], x_gp[2]
                ));
            }
            let cx = t1[1] * t2[2] - t1[2] * t2[1];
            let cy = t1[2] * t2[0] - t1[0] * t2[2];
            let cz = t1[0] * t2[1] - t1[1] * t2[0];
            let dgamma = s * (cx * cx + cy * cy + cz * cz).sqrt();
            let df = n_out.map(|c| -mpa * dgamma * c);
            for a in 0..6 {
                for c in 0..3 {
                    nodal_f[a][c] += df[c] * n[a];
                }
            }
            for c in 0..3 {
                resultant[c] += df[c];
            }
            moment[0] += x_gp[1] * df[2] - x_gp[2] * df[1];
            moment[1] += x_gp[2] * df[0] - x_gp[0] * df[2];
            moment[2] += x_gp[0] * df[1] - x_gp[1] * df[0];
        }
    }
    Ok((nodal_f, resultant, moment))
}

/// Reference data for the degree-3 integrand of the thermal load
/// (ΔT is P2: degree 2, ∂φa/∂x: degree 1). Keast's 5-point rule.
pub struct ThermalRefData {
    pub weights: Vec<f64>,
    /// `basis[q][b]` = φ_b at point q.
    pub basis: Vec<Vec<f64>>,
    /// `grads[q][a]` = ∇ξ φ_a at point q.
    pub grads: Vec<Vec<[f64; 3]>>,
}

impl ThermalRefData {
    pub fn new() -> ThermalRefData {
        let rule = fenics_element::tetrahedron_quadrature(3);
        let el = fenics_element::P2Tetrahedron;
        ThermalRefData {
            weights: rule.points.iter().map(|q| q.weight).collect(),
            basis: rule
                .points
                .iter()
                .map(|q| el.evaluate_basis(&q.point))
                .collect(),
            grads: rule
                .points
                .iter()
                .map(|q| el.evaluate_gradients(&q.point))
                .collect(),
        }
    }
}

impl Default for ThermalRefData {
    fn default() -> Self {
        Self::new()
    }
}

/// Element thermal-load integrals: `out[a][i]` = ∫ ΔT·∂φa/∂x_i dV, with the node
/// temperatures `dt` indexed like the element's own 10 nodes.
pub fn element_thermal_integrals(
    mesh: &P2Mesh,
    e: usize,
    dt: &[f64],
    rd: &ThermalRefData,
) -> Box<[[f64; 3]; 10]> {
    let map = element_map(mesh, e);
    let det = map.det_jacobian.abs();
    let t = &mesh.tets[e];
    let mut dt_local = [0.0f64; 10];
    for (k, &node) in t.iter().enumerate() {
        dt_local[k] = dt[node as usize];
    }
    let mut out_box = Box::new([[0.0f64; 3]; 10]);
    for (q, w) in rd.weights.iter().enumerate() {
        // ΔT(x) at the quadrature point from the P2 field.
        let t_at: f64 = rd.basis[q]
            .iter()
            .zip(dt_local.iter())
            .map(|(b, dt)| b * dt)
            .sum();
        let s = w * det * t_at;
        for a in 0..10 {
            let gp = map.transform_gradient(&rd.grads[q][a]);
            out_box[a][0] += s * gp[0];
            out_box[a][1] += s * gp[1];
            out_box[a][2] += s * gp[2];
        }
    }
    out_box
}

/// Nodal thermal load f[3a+i] = (3λ+2μ)·α·∫ ΔT·∂φa/∂x_i dV, nodal ΔT in hand.
/// (3λ+2μ) is per element from that element's modulus, matching the stiffness
/// parameterisation exactly.
pub fn thermal_load(
    mesh: &P2Mesh,
    dt: &[f64],
    young: &[f64],
    poisson: f64,
    alpha: f64,
    rd: &ThermalRefData,
) -> Vec<f64> {
    assert_eq!(young.len(), mesh.tets.len(), "one modulus per element");
    assert_eq!(dt.len(), mesh.nodes.len(), "one ΔT per node");
    let mut f = vec![0.0; mesh.ndofs()];
    for (e, y) in young.iter().enumerate() {
        let m = fenics_assembly::elasticity::ElasticMaterial::new(*y, poisson);
        // 3λ+2μ = E/(1−2ν); through lame_* to stay the exact same numbers the
        // stiffness and stress recovery use.
        let beta = (3.0 * m.lame_lambda() + 2.0 * m.lame_mu()) * alpha;
        let t = &mesh.tets[e];
        let le = element_thermal_integrals(mesh, e, dt, rd);
        for a in 0..10 {
            let row = t[a] as usize * 3;
            for i in 0..3 {
                f[row + i] += beta * le[a][i];
            }
        }
    }
    f
}

/// Multi-material nodal thermal load: one thermal expansion coefficient `alpha_elem[e]`
/// and one modulus `young[e]` per element (V53). When all `alpha_elem[e]` equal a single
/// scalar `alpha`, the arithmetic is bit-identical to [`thermal_load`].
pub fn thermal_load_multi(
    mesh: &P2Mesh,
    dt: &[f64],
    young: &[f64],
    poisson: f64,
    alpha_elem: &[f64],
    rd: &ThermalRefData,
) -> Vec<f64> {
    assert_eq!(young.len(), mesh.tets.len(), "one modulus per element");
    assert_eq!(alpha_elem.len(), mesh.tets.len(), "one alpha per element");
    assert_eq!(dt.len(), mesh.nodes.len(), "one ΔT per node");
    let mut f = vec![0.0; mesh.ndofs()];
    for (e, (&y, &alpha)) in young.iter().zip(alpha_elem.iter()).enumerate() {
        let m = fenics_assembly::elasticity::ElasticMaterial::new(y, poisson);
        let beta = (3.0 * m.lame_lambda() + 2.0 * m.lame_mu()) * alpha;
        let t = &mesh.tets[e];
        let le = element_thermal_integrals(mesh, e, dt, rd);
        for a in 0..10 {
            let row = t[a] as usize * 3;
            for i in 0..3 {
                f[row + i] += beta * le[a][i];
            }
        }
    }
    f
}

/// Exact discrete transpose `(B_th^T w)[b] = ∑_e β_e ∫_{Ω_e} (∇ · w) φ_b dV` of
/// [`thermal_load_multi`], satisfying `<thermal_load_multi(dt), w> == <dt, B_th^T w>`
/// to IEEE-754 floating-point precision. Used by staggered discrete thermo-elastic adjoints.
pub fn thermal_load_transpose_multi(
    mesh: &P2Mesh,
    w: &[f64],
    young: &[f64],
    poisson: f64,
    alpha_elem: &[f64],
    rd: &ThermalRefData,
) -> Vec<f64> {
    assert_eq!(young.len(), mesh.tets.len(), "one modulus per element");
    assert_eq!(alpha_elem.len(), mesh.tets.len(), "one alpha per element");
    assert_eq!(w.len(), mesh.ndofs(), "3 displacement DOFs per node");
    let mut out_t = vec![0.0; mesh.nodes.len()];
    for (e, (&y, &alpha)) in young.iter().zip(alpha_elem.iter()).enumerate() {
        let m = fenics_assembly::elasticity::ElasticMaterial::new(y, poisson);
        let beta = (3.0 * m.lame_lambda() + 2.0 * m.lame_mu()) * alpha;
        if beta == 0.0 {
            continue;
        }
        let map = element_map(mesh, e);
        let det = map.det_jacobian.abs();
        let t = &mesh.tets[e];
        for (q, wq) in rd.weights.iter().enumerate() {
            let mut div_w = 0.0f64;
            for (a, &ta) in t.iter().enumerate() {
                let gp = map.transform_gradient(&rd.grads[q][a]);
                let base = 3 * ta as usize;
                div_w += gp[0] * w[base] + gp[1] * w[base + 1] + gp[2] * w[base + 2];
            }
            let scale = beta * wq * det * div_w;
            for (b, &phi_b) in rd.basis[q].iter().enumerate() {
                out_t[t[b] as usize] += scale * phi_b;
            }
        }
    }
    out_t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thermal_load_transpose_satisfies_exact_bilinear_duality() {
        let verts = vec![
            [0.0, 0.0, 0.0],
            [12.0, 1.0, 0.0],
            [2.0, 10.0, 0.5],
            [1.0, 2.0, 9.0],
            [14.0, 11.0, 8.0],
        ];
        let tets = vec![[0, 1, 2, 3], [1, 2, 3, 4]];
        let mesh = P2Mesh::from_linear(&verts, &tets);
        let young = vec![71_000.0, 193_000.0];
        let alpha = vec![23.1e-6, 16.0e-6];
        let dt: Vec<f64> = (0..mesh.nodes.len())
            .map(|i| 15.0 + 3.7 * (i as f64).sin() - 1.9 * ((2 * i) as f64).cos())
            .collect();
        let w: Vec<f64> = (0..mesh.ndofs())
            .map(|k| 0.42 * ((k + 3) as f64).cos() - 0.17 * ((3 * k) as f64).sin())
            .collect();
        let rd = ThermalRefData::new();
        let f_th = thermal_load_multi(&mesh, &dt, &young, 0.31, &alpha, &rd);
        let bt_w = thermal_load_transpose_multi(&mesh, &w, &young, 0.31, &alpha, &rd);
        let lhs: f64 = f_th.iter().zip(&w).map(|(a, b)| a * b).sum();
        let rhs: f64 = dt.iter().zip(&bt_w).map(|(a, b)| a * b).sum();
        let rel = (lhs - rhs).abs() / lhs.abs().max(1e-12);
        assert!(
            rel < 1e-14,
            "expected <B_th dt, w> == <dt, B_th^T w> to machine precision, lhs={lhs}, rhs={rhs}, rel={rel:e}"
        );
    }

    #[test]
    fn p2_thermal_mass_matches_exact_volume_capacity() {
        let verts = vec![
            [0.0, 0.0, 0.0],
            [8.0, 0.5, 0.0],
            [1.0, 9.0, 0.2],
            [0.5, 1.0, 7.0],
            [9.0, 8.5, 6.5],
        ];
        let tets = vec![[0, 1, 2, 3], [1, 2, 3, 4]];
        let mesh = P2Mesh::from_linear(&verts, &tets);
        let c_vol = vec![0.00399, 0.00209];
        let m = assemble_thermal_mass(&mesh, &c_vol);
        let ones = vec![1.0; mesh.nodes.len()];
        let mut m1 = vec![0.0; mesh.nodes.len()];
        m.mul(&ones, &mut m1);
        let total: f64 = ones.iter().zip(&m1).map(|(a, b)| a * b).sum();
        let expected: f64 = c_vol
            .iter()
            .enumerate()
            .map(|(e, &c)| c * mesh.tet_volume(e).abs())
            .sum();
        let rel = (total - expected).abs() / expected.abs();
        assert!(
            rel < 1e-13,
            "expected exact P2 scalar capacity integral {expected}, got {total} (rel={rel:e})"
        );
    }

    /// Exact 1D Fourier–Biot eigenfunction series on `[0, L]` with insulated `z = 0`
    /// and convective quench `-k ∂T/∂z = h (T - T_inf)` on `z = L`, uniform `T(z, 0) = T_init`.
    fn exact_fourier_biot_slab(
        z: f64,
        t: f64,
        l_mm: f64,
        alpha_mm2_s: f64,
        bi: f64,
        t_init: f64,
        t_inf: f64,
    ) -> f64 {
        let fo = alpha_mm2_s * t / (l_mm * l_mm);
        let mut sum = 0.0;
        for n in 0..40 {
            let n_pi = (n as f64) * std::f64::consts::PI;
            let mut lo = n_pi + 1e-12;
            let mut hi = n_pi + 0.5 * std::f64::consts::PI - 1e-12;
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if mid * mid.tan() < bi {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let mu = 0.5 * (lo + hi);
            let c_n = 2.0 * mu.sin() / (mu + mu.sin() * mu.cos());
            sum += c_n * (mu * z / l_mm).cos() * (-mu * mu * fo).exp();
        }
        t_inf + (t_init - t_inf) * sum
    }

    #[test]
    fn transient_bdf2_matches_exact_fourier_biot_column_quench() {
        // Structured Kuhn-triangulated column [0, 2] x [0, 2] x [0, 10] mm (nz = 8 layers = 48 P2 tets).
        let nz = 8usize;
        let w = 2.0;
        let l_mm = 10.0;
        let mut verts = Vec::new();
        for iz in 0..=nz {
            let z = l_mm * (iz as f64) / (nz as f64);
            for iy in 0..=1 {
                for ix in 0..=1 {
                    verts.push([(ix as f64) * w, (iy as f64) * w, z]);
                }
            }
        }
        let vid = |ix: usize, iy: usize, iz: usize| -> u32 { (iz * 4 + iy * 2 + ix) as u32 };
        let kuhn = [
            [0, 1, 3, 7],
            [0, 1, 7, 5],
            [0, 2, 3, 7],
            [0, 2, 7, 6],
            [0, 4, 5, 7],
            [0, 4, 7, 6],
        ];
        let mut tets = Vec::with_capacity(nz * 6);
        for iz in 0..nz {
            let cube = [
                vid(0, 0, iz),
                vid(1, 0, iz),
                vid(0, 1, iz),
                vid(1, 1, iz),
                vid(0, 0, iz + 1),
                vid(1, 0, iz + 1),
                vid(0, 1, iz + 1),
                vid(1, 1, iz + 1),
            ];
            for k in &kuhn {
                tets.push([cube[k[0]], cube[k[1]], cube[k[2]], cube[k[3]]]);
            }
        }
        let mesh = P2Mesh::from_linear(&verts, &tets);
        let mut quench_facets = Vec::new();
        for t in &mesh.tets {
            for f in [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
                if f.iter()
                    .all(|&li| (mesh.nodes[t[li] as usize][2] - l_mm).abs() < 1e-9)
                {
                    quench_facets.push(crate::p2::face_nodes(t, f));
                }
            }
        }
        assert_eq!(quench_facets.len(), 2);

        let k_val = 0.016; // W/(mm·K)
        let c_val = 0.004; // J/(mm³·K) -> alpha = 4.0 mm²/s
        let h_val = 0.012; // W/(mm²·K) -> Bi = h*L/k = 7.5
        let t_init = 320.0;
        let t_inf = 20.0;
        let t_end = 6.0;
        let steps = 48;
        let frames = solve_transient_conduction(
            &mesh,
            &vec![None; mesh.nodes.len()],
            &vec![k_val; mesh.tets.len()],
            &vec![c_val; mesh.tets.len()],
            t_init,
            t_end,
            steps,
            &[RobinBc {
                h: h_val,
                t_inf,
                facets: quench_facets,
            }],
            &[],
            SolverOptions::default(),
        )
        .expect("transient BDF2 solve");
        assert_eq!(frames.len(), steps + 1);

        let alpha_th = k_val / c_val;
        let bi = h_val * l_mm / k_val;
        for &check_step in &[steps / 2, steps] {
            let fr = &frames[check_step];
            let t_s = fr.time_s;
            let exact_back = exact_fourier_biot_slab(0.0, t_s, l_mm, alpha_th, bi, t_init, t_inf);
            let exact_skin = exact_fourier_biot_slab(l_mm, t_s, l_mm, alpha_th, bi, t_init, t_inf);
            let fem_back = fr.temperature[vid(0, 0, 0) as usize];
            let fem_skin = fr.temperature[vid(0, 0, nz) as usize];
            let err_back = (fem_back - exact_back).abs() / (t_init - t_inf);
            let err_skin = (fem_skin - exact_skin).abs() / (t_init - t_inf);
            assert!(
                err_back < 0.005 && err_skin < 0.005,
                "step {check_step} (t={t_s:.2}s): back={fem_back:.3} vs {exact_back:.3} (err={err_back:.4}), skin={fem_skin:.3} vs {exact_skin:.3} (err={err_skin:.4})"
            );
        }
    }
}
