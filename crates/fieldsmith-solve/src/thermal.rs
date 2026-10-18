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
//! condition `k ∂T/∂n = h(T_inf − T)` scales with the ratio `h/k`, and this solve pins
//! `k = 1` W/(mm·K) (the materials sheet carries no conductivity today) — so a band's
//! `h` is read per that unit conductivity and mixed Dirichlet/Robin fields carry the
//! unit-solid's Biot number, not the real part's. Pure-Dirichlet cases are unaffected.
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
use fenics_element::{interval_quadrature, FiniteElement, P2Triangle};

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

/// Steady conduction on the P2 mesh with Dirichlet temperatures per node (`Some(t)` =
/// fixed °C, `None` = insulated), optional convection (Robin) bands, and optional
/// prescribed heat-flux bands. Returns nodal temperature (°C). Solvable iff at least
/// one node is fixed or one band has h > 0 — a wholly insulated body is unsolvable (the
/// Laplacian's constant mode), and heat flux alone does not fix the level either.
pub fn solve_conduction(
    mesh: &P2Mesh,
    d: &[Option<f64>],
    robin: &[RobinBc],
    flux: &[HeatFluxBc],
    opts: SolverOptions,
) -> Result<(Vec<f64>, SolveReport), String> {
    faer::set_global_parallelism(faer::Parallelism::None);
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
    if !fixed.iter().any(|&f| f) && robin.iter().all(|bc| bc.h <= 0.0) {
        return Err(if flux.is_empty() {
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
    let mut a = assemble_laplace(mesh);
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

// ---------------------------------------------------------------------------
// Element quantities.
// ---------------------------------------------------------------------------

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

/// Global Laplace matrix (no boundary conditions applied).
pub fn assemble_laplace(mesh: &P2Mesh) -> Csr {
    let rd = RefData::new();
    let mut a = Csr::from_elements(mesh.nodes.len(), mesh.tets.iter().copied());
    for e in 0..mesh.tets.len() {
        let le = element_laplace(mesh, e, &rd);
        let t = &mesh.tets[e];
        for i in 0..10 {
            for j in 0..10 {
                a.add(t[i] as usize, t[j], le[i][j]);
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
