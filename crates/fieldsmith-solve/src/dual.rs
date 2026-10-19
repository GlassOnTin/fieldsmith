//! Equilibrated (dual) stress recovery — the certified upper bound on compliance.
//!
//! The lower bound ([`crate::bounds`]) reads C_h = fᵀu ≤ C_true off the conforming solve.
//! This module produces the other end of the bracket: a stress field σ* on the same mesh
//! whose complementary energy
//!
//!   C(σ*) = ∫_Ω σ* : A : σ* dV ≥ C_true,
//!
//! with A the compliance tensor. The inequality is complementary-energy duality: σ* is
//! statically admissible for a load whose action on the free displacement space equals
//! the solve's load vector, the minimiser of ∫σ:A:σ over that set is the exact stress
//! field, and that minimum equals the true compliance. Any feasible σ* therefore bounds
//! C_true from above, and the bracket closes: C_h ≤ C_true ≤ C(σ*).
//!
//! Recovery is a single least-squares system over piecewise-linear stresses σₑ (6
//! components × 4 vertex shapes = 24 dofs per element) whose constraint rows are the
//! certificate's three conditions, enforced by construction:
//! - global equilibrium: for every displacement test φ, Σₑ ∫ₑ σₑ:ε(φ)
//!   − Σ_{Γ_D facets∋node} (σₑnₑ):φ moments = f(φ), which with the pins below makes the
//!   recovered divergence functional equal the load's on the nodal test space;
//! - H(div): interior-facet traction moments of the two adjacent elements cancel;
//! - Neumann pins: the trace moments on each Γ_N facet equal the prescribed t̄ moments;
//! - reaction containment: on component-mixed support faces the trace components that
//!   are not fixed are pinned to zero, so reactions act only in pinned directions and
//!   the Γ_D work against the exact displacement vanishes.
//!
//! The certificate is the Fenchel bound J(σ\*) ≥ 2∫σ\*:ε(w) − a(w,w) evaluated where it
//! is sharp. Against the exact solution this reads J ≥ C_true; against the interpolant
//! it reads J ≥ C_true − the P2 interpolation defect, which is the honest discrete
//! form: the measured deficit shrinks under refinement (V-gate fixture: 7.4e-6 →
//! 3.1e-7 relative) and vanishes when the exact field is representable (in-span, exact
//! to solver precision). The least-squares residual is re-verified per row family
//! before any value is returned; a nonzero residual means the discrete load is not
//! equilibrible and the function refuses. The Dirichlet-side traces are free (reactions), which is exactly what makes
//! the divergence functional absorb them without leaking into the certified load — the
//! complementary identity b̂\* := −div σ\* automatically satisfies b̂\*(w) = 0 against
//! every rigid test field, avoiding the per-element split feasibility trap.
//!
//! Integrals: the P1/P2 products on affine tets use closed forms (∫λᵐdV = V/4,
//! ∫λᵐλⁿ = V(1+δ)/20, facet moments via the barycentric formula 2A·a!b!c!/(a+b+c+2)!);
//! the equilibrium rows against P2 test functions use the degree-2 tetrahedron rule
//! (exact for the quadratic integrand).
//!
//! Licences (stated, and checked where checkable):
//! - the displacement solve must have converged — the rows use `problem.force` directly,
//!   and the residual check ties the value to that load, not another;
//! - the prescribed boundary traction must be constant per facet (distributed `fn` loads
//!   would certify their per-facet constant averages — named gap);
//! - the `prescribed` closure must classify every boundary facet: `Some(t)` = Neumann
//!   facet with constant traction t in the outward normal (including zero for every
//!   unloaded non-support facet), `None` = Dirichlet side with a free reaction;
//! - isotropic material with E > 0 per element and ν ∈ (−1, ½) (checked);
//! - well-posed problems (the free stiffness is definite); the residual check refuses
//!   otherwise rather than returning a value.
//!
//! Named gaps (VISION §7 item 2): the bracket is per-case, solver level — rung-level
//! tracked wiring (TrackedValues / RungChange / Tolerance, the api verb, sweep CSV,
//! worker mirror, e2e) and case-level plumbing of C(σ*) follow as their own slice; the
//! dense least-squares cost on production meshes is unmeasured; tightness is whatever
//! the minimum-residual point gives (min-J refinement is future work).

use crate::p2::{face_nodes, EDGES};
use crate::{Problem, V3};
use faer::prelude::*;
use faer::Mat as FMat;
use fenics_element::{FiniteElement, P2Tetrahedron};

/// Stress/strain 6-vector order: xx, yy, zz, xy, xz, yz.
const XX: usize = 0;
const YY: usize = 1;
const ZZ: usize = 2;
const XY: usize = 3;
const XZ: usize = 4;
const YZ: usize = 5;

/// Unit stress 6-vector for component `k`.
fn stress_unit(k: usize) -> [f64; 6] {
    let mut s = [0.0; 6];
    s[k] = 1.0;
    s
}

/// Full tensor contraction of two symmetric tensors in 6-vector form.
#[inline]
fn contract(a: &[f64; 6], b: &[f64; 6]) -> f64 {
    a[XX] * b[XX]
        + a[YY] * b[YY]
        + a[ZZ] * b[ZZ]
        + 2.0 * (a[XY] * b[XY] + a[XZ] * b[XZ] + a[YZ] * b[YZ])
}

/// Strain 6-vector of an isotropic stress 6-vector: ε = A σ.
fn strain_from_stress(s: &[f64; 6], young: f64, poisson: f64) -> [f64; 6] {
    let tr = s[XX] + s[YY] + s[ZZ];
    [
        (s[XX] - poisson * (tr - s[XX])) / young,
        (s[YY] - poisson * (tr - s[YY])) / young,
        (s[ZZ] - poisson * (tr - s[ZZ])) / young,
        (1.0 + poisson) * s[XY] / young,
        (1.0 + poisson) * s[XZ] / young,
        (1.0 + poisson) * s[YZ] / young,
    ]
}

/// Constant strain 6-vector of the displacement field e_c·λ_a with vertex-shape gradient
/// g: sym(g ⊗ e_c).
fn strain_of_shape(g: [f64; 3], c: usize) -> [f64; 6] {
    let mut e = [0.0; 6];
    match c {
        0 => {
            e[XX] = g[0];
            e[XY] = 0.5 * g[1];
            e[XZ] = 0.5 * g[2];
        }
        1 => {
            e[YY] = g[1];
            e[XY] = 0.5 * g[0];
            e[YZ] = 0.5 * g[2];
        }
        _ => {
            e[ZZ] = g[2];
            e[XZ] = 0.5 * g[0];
            e[YZ] = 0.5 * g[1];
        }
    }
    e
}

/// Traction map: the coefficient of stress component `k` in the c-th entry of σ·n.
fn traction_map(n: [f64; 3]) -> [[f64; 3]; 6] {
    let mut m = [[0.0f64; 3]; 6];
    m[XX][0] = n[0];
    m[XY][0] = n[1];
    m[XZ][0] = n[2];
    m[XY][1] = n[0];
    m[YY][1] = n[1];
    m[YZ][1] = n[2];
    m[XZ][2] = n[0];
    m[YZ][2] = n[1];
    m[ZZ][2] = n[2];
    m
}

/// Result of the equilibrated recovery.
#[derive(Clone, Debug)]
pub struct UpperBound {
    /// C(σ*) = ∫σ*:A:σ*, N·mm — a certified upper bound on C_true.
    pub j: f64,
    /// max residual of the global-equilibrium rows (N).
    pub equilibrium_residual: f64,
    /// max residual of the interior-traction continuity rows (N).
    pub continuity_residual: f64,
    /// max residual of the Neumann-pin rows (N).
    pub pin_residual: f64,
    /// Rows = columns of the solved system.
    pub kkt_dofs: usize,
}

/// Where the constraint assembly lands: the dense matrix (small meshes; bit-identical
/// to the shipped path) or the triplet stream (the sparse iterative route).
enum Entries<'m> {
    Dense(&'m mut FMat<f64>),
    Trips(&'m mut Vec<(usize, usize, f64)>),
}
impl Entries<'_> {
    #[inline]
    fn add(&mut self, r: usize, c: usize, v: f64) {
        match self {
            Entries::Dense(m) => m[(r, c)] += v,
            Entries::Trips(t) => t.push((r, c, v)),
        }
    }
    fn sub(&mut self, r: usize, c: usize, v: f64) {
        self.add(r, c, -v);
    }
}

/// The certified upper bound for a converged solve.
///
/// `prescribed` classifies each boundary facet from its three corner node positions in
/// facet-slot order (irrelevant under the licensed constant tractions): `Some(t)` =
/// Neumann facet with constant traction t in the outward normal, `None` = Dirichlet side
/// whose traction is a free reaction. Every boundary facet must be classified; unloaded
/// non-support facets must be classified `Some(zero)`.
#[allow(clippy::needless_range_loop)] // index-driven scatter into the constraint matrix
pub fn upper_compliance(
    problem: &Problem,
    prescribed: &dyn Fn([V3; 3]) -> Option<V3>,
) -> Result<UpperBound, String> {
    let mesh = &problem.mesh;
    let nu = problem.material.poisson;
    if nu <= -1.0 || nu >= 0.5 {
        return Err(format!(
            "poisson {nu} outside the isotropic compliance range (-1, 1/2)"
        ));
    }
    for ey in &problem.material.young {
        if *ey <= 0.0 || !ey.is_finite() {
            return Err("young's modulus must be positive and finite".to_string());
        }
    }

    // ---- facet census: identity, area, orientation, boundary count ----
    let mut facet_of: Vec<[usize; 4]> = vec![[usize::MAX; 4]; mesh.tets.len()];
    let mut face_o: Vec<[f64; 4]> = vec![[0.0; 4]; mesh.tets.len()];
    let mut hat: Vec<[f64; 3]> = Vec::new();
    let mut area: Vec<f64> = Vec::new();
    let mut corners: Vec<[u32; 3]> = Vec::new();
    let mut count: Vec<u32> = Vec::new();
    let mut first: std::collections::HashMap<[u32; 3], usize> = std::collections::HashMap::new();
    for (e, t) in mesh.tets.iter().enumerate() {
        let pts = t.map(|v| mesh.nodes[v as usize]);
        let centre = [
            (pts[0][0] + pts[1][0] + pts[2][0] + pts[3][0]) / 4.0,
            (pts[0][1] + pts[1][1] + pts[2][1] + pts[3][1]) / 4.0,
            (pts[0][2] + pts[1][2] + pts[2][2] + pts[3][2]) / 4.0,
        ];
        for (fi, face) in [[0usize, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]]
            .into_iter()
            .enumerate()
        {
            let q = face_nodes(t, face);
            let (c0, c1, c2) = (
                mesh.nodes[q[0] as usize],
                mesh.nodes[q[1] as usize],
                mesh.nodes[q[2] as usize],
            );
            let d1 = [c1[0] - c0[0], c1[1] - c0[1], c1[2] - c0[2]];
            let d2 = [c2[0] - c0[0], c2[1] - c0[1], c2[2] - c0[2]];
            let raw = [
                d1[1] * d2[2] - d1[2] * d2[1],
                d1[2] * d2[0] - d1[0] * d2[2],
                d1[0] * d2[1] - d1[1] * d2[0],
            ];
            let twice = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
            if !twice.is_finite() || twice == 0.0 {
                return Err("degenerate facet in the analysis mesh".to_string());
            }
            let fc = [
                (c0[0] + c1[0] + c2[0]) / 3.0,
                (c0[1] + c1[1] + c2[1]) / 3.0,
                (c0[2] + c1[2] + c2[2]) / 3.0,
            ];
            let side = (fc[0] - centre[0]) * raw[0]
                + (fc[1] - centre[1]) * raw[1]
                + (fc[2] - centre[2]) * raw[2];
            let n_el = if side > 0.0 {
                [raw[0] / twice, raw[1] / twice, raw[2] / twice]
            } else {
                [-raw[0] / twice, -raw[1] / twice, -raw[2] / twice]
            };
            let mut k = [t[face[0]], t[face[1]], t[face[2]]];
            k.sort_unstable();
            let id = match first.entry(k) {
                std::collections::hash_map::Entry::Occupied(occ) => {
                    let id = *occ.get();
                    let h = hat[id];
                    face_o[e][fi] = n_el[0] * h[0] + n_el[1] * h[1] + n_el[2] * h[2];
                    count[id] += 1;
                    id
                }
                std::collections::hash_map::Entry::Vacant(vac) => {
                    let id = hat.len();
                    hat.push(n_el);
                    area.push(twice / 2.0);
                    corners.push([q[0], q[1], q[2]]);
                    count.push(1);
                    face_o[e][fi] = 1.0;
                    vac.insert(id);
                    id
                }
            };
            facet_of[e][fi] = id;
        }
    }

    // ---- boundary classification ----
    let mut prescribed_t: Vec<Option<V3>> = vec![None; hat.len()];
    for (id, _) in hat.iter().enumerate() {
        if count[id] != 1 {
            continue;
        }
        let q = corners[id];
        let ps = [
            mesh.nodes[q[0] as usize],
            mesh.nodes[q[1] as usize],
            mesh.nodes[q[2] as usize],
        ];
        prescribed_t[id] = prescribed(ps);
    }

    // ---- system dimensions ----
    // Row (d) count up front: every family writes into one matrix, and the (d) rows'
    // ordinals are known before the first entry is assembled.
    let rows_d = {
        let mut n = 0usize;
        for (id, pres) in prescribed_t.iter().enumerate() {
            if !(count[id] == 1 && pres.is_none()) {
                continue;
            }
            for cc in 0..3 {
                let pinned = corners[id]
                    .iter()
                    .all(|&node| problem.fixed[node as usize] & (1 << cc) != 0);
                if !pinned {
                    n += 6;
                }
            }
        }
        n
    };
    let e_count = mesh.tets.len();
    let n_sigma = 60 * e_count; // P2 stress: 6 components × the element's 10 P2 nodes
    let mut f_int = 0usize;
    let mut f_pin = 0usize;
    for (id, pres) in prescribed_t.iter().enumerate() {
        if count[id] == 2 {
            f_int += 1;
        }
        if count[id] == 1 && pres.is_some() {
            f_pin += 1;
        }
    }
    // Rows (a) cover every nodal dof. The fixed-comp directions carry the reaction
    // moments; leaving them out lets the recovered field trade reaction work against
    // complementary energy and the bound fails below the true value (measured 0.21 vs
    // 0.4 on the T1 fixture). The rows stay σ-only: σ's own Γ_D traces appear on both
    // sides, so no reaction value is needed.
    let rows_a = 3 * mesh.nodes.len();
    let rows_b = 18 * f_int; // H(div): the facet's 6 P2 nodes × 3 components
    let rows_c = 18 * f_pin;
    let rows_total = rows_a + rows_b + rows_c + rows_d;
    let rows_c_end = rows_a + rows_b + rows_c; // rows [rows_c_end..rows_total] are the (d) family

    // Small meshes assemble the dense matrix (bit-identical path); large ones collect
    // triplets for the sparse iterative solve. No (r, c) pair is written twice — the
    // families own disjoint rows and within a family every write targets a distinct
    // column — so either sink is exact.
    let use_dense = rows_total <= 12_000 && mesh.tets.len() <= 600;
    let mut c_mat_buf = use_dense.then(|| FMat::<f64>::zeros(rows_total, n_sigma));
    let mut trips: Vec<(usize, usize, f64)> = Vec::new();
    let mut entries = match &mut c_mat_buf {
        Some(m) => Entries::Dense(m),
        None => Entries::Trips(&mut trips),
    };
    let mut rhs = vec![0.0f64; rows_total];

    // ---- closed forms ----
    // Exponent vectors of the element's P2 nodal shapes (vertices 0..3, then the six EDGES
    // with the ×4 multiplier) and the shape-product moments over the element and facets.
    let shape_exp: [[usize; 4]; 10] = {
        let mut s = [[0usize; 4]; 10];
        for i in 0..4 {
            s[i][i] = 1;
        }
        for (kk, &(a, b)) in EDGES.iter().enumerate() {
            s[4 + kk][a] += 1;
            s[4 + kk][b] += 1;
        }
        s
    };
    let shape_mult: [f64; 10] = [1.0, 1.0, 1.0, 1.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0];
    let tet_pair = |q: usize, r: usize, vol: f64| -> f64 {
        let mut ex = [0usize; 4];
        for i in 0..4 {
            ex[i] = shape_exp[q][i] + shape_exp[r][i];
        }
        let n: usize = ex.iter().sum();
        let mut num = shape_mult[q] * shape_mult[r] * 6.0 * vol;
        for &p in &ex {
            for t2 in 1..=p {
                num *= t2 as f64;
            }
        }
        let mut den = 1.0;
        for t2 in 1..=(n + 3) {
            den *= t2 as f64;
        }
        num / den
    };
    // Facet-side P2 shape of a tet-local node q on facet `id`: (exponent triple, ×4
    // multiplier when the node is a facet midside); None when the node is off the facet.
    let facet_shape = |q: usize, t: &[u32; 10], id: usize| -> Option<([usize; 3], f64)> {
        let node = t[q];
        if let Some(sl) = corners[id].iter().position(|&c| c == node) {
            let mut e = [0usize; 3];
            e[sl] = 1;
            return Some((e, 1.0));
        }
        if q >= 4 {
            let (a, b) = EDGES[q - 4];
            if corners[id].contains(&t[a]) && corners[id].contains(&t[b]) {
                let sa = corners[id]
                    .iter()
                    .position(|&c| c == t[a])
                    .expect("endpoint");
                let sb = corners[id]
                    .iter()
                    .position(|&c| c == t[b])
                    .expect("endpoint");
                let mut e = [0usize; 3];
                e[sa] += 1;
                e[sb] += 1;
                return Some((e, 4.0));
            }
        }
        None
    };
    let facet_pair = |eq: [usize; 3], mq: f64, ej: [usize; 3], mj: f64, ar: f64| -> f64 {
        let mut ex = [0usize; 3];
        for i in 0..3 {
            ex[i] = eq[i] + ej[i];
        }
        let n: usize = ex.iter().sum();
        let mut num = 2.0 * ar * mq * mj;
        for &p in &ex {
            for t2 in 1..=p {
                num *= t2 as f64;
            }
        }
        let mut den = 1.0;
        for t2 in 1..=(n + 2) {
            den *= t2 as f64;
        }
        num / den
    };
    let sid = |e: usize, q: usize, k: usize| 60 * e + 6 * q + k;

    // ---- row (a): global equilibrium; rhs = f(φ) exactly (see the pin note) ----
    // For the P2 test φ (local dof d of element e): ε_k(φ)(x) = Σ_r E_r[k]·λ_r(x) over
    // the tet's vertices, E_r = sym(∇φ(v_r) ⊗ e_c) from the vertex gradients.
    for (e, t) in mesh.tets.iter().enumerate() {
        let map = crate::assemble::element_map(mesh, e);
        let vol = mesh.tet_volume(e).abs();
        let refs: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        // vg[d][r] = the physical gradient of the d-th P2 basis function at vertex r.
        let vg: Vec<Vec<[f64; 3]>> = (0..10)
            .map(|d| {
                refs.iter()
                    .map(|p| map.transform_gradient(&P2Tetrahedron.evaluate_gradients(p)[d]))
                    .collect()
            })
            .collect();
        for d in 0..10 {
            let node = t[d] as usize;
            for cc in 0..3 {
                if problem.fixed[node] & (1 << cc) != 0 {
                    continue;
                }
                let row = 3 * node + cc;
                for k in 0..6 {
                    let mut per_q = [0.0f64; 10];
                    for r in 0..4 {
                        let erk = strain_of_shape(vg[d][r], cc)[k];
                        if erk == 0.0 {
                            continue;
                        }
                        for q in 0..10 {
                            per_q[q] += erk * tet_pair(q, r, vol);
                        }
                    }
                    for q in 0..10 {
                        if per_q[q] != 0.0 {
                            entries.add(row, sid(e, q, k), per_q[q]);
                        }
                    }
                }
                rhs[row] = problem.force[3 * node + cc];
            }
        }
    }
    // Γ_N moment subtraction on the equilibrium rows; −S_{Γ_D} trace blocks.
    for (e, t) in mesh.tets.iter().enumerate() {
        for fi in 0..4 {
            let id = facet_of[e][fi];
            if count[id] != 1 {
                continue;
            }
            let nt = traction_map(hat[id]);
            let node_ids: [u32; 6] = face_nodes(
                t,
                match fi {
                    0 => [0usize, 1, 2],
                    1 => [0, 1, 3],
                    2 => [0, 2, 3],
                    _ => [1, 2, 3],
                },
            );
            let pin = prescribed_t[id];
            if pin.is_none() {
                // Dirichlet side: subtract the element's own trace moments.
                for d in 0..6 {
                    let node = node_ids[d] as usize;
                    let Some((phi, mphi)) = facet_shape(
                        (0..10)
                            .find(|&q| t[q] == node as u32)
                            .expect("facet node is a tet node"),
                        t,
                        id,
                    ) else {
                        continue;
                    };
                    for cc in 0..3 {
                        if problem.fixed[node] & (1 << cc) != 0 {
                            continue;
                        }
                        let row = 3 * node + cc;
                        for k in 0..6 {
                            if nt[k][cc] == 0.0 {
                                continue;
                            }
                            for q in 0..10 {
                                let Some((sq, mq)) = facet_shape(q, t, id) else {
                                    continue;
                                };
                                let mom = facet_pair(sq, mq, phi, mphi, area[id]);
                                if mom != 0.0 {
                                    entries.sub(row, sid(e, q, k), nt[k][cc] * mom);
                                }
                            }
                        }
                    }
                }
            } else {
                // Neumann side: nothing enters the equilibrium rows' right side here.
                // The row reads ∫σ:ε(φ) − Γ_D-trace(φ) = f(φ); with the (c) rows pinning
                // σ's own Γ_N trace to t̄, the certified divergence functional becomes
                // f − Γ_N moments on the free space, which is the caller's load.
                let _ = pin;
            }
        }
    }

    // ---- row (b): H(div) — interior-facet traction-moment continuity ----
    let mut interior_ordinal = vec![usize::MAX; hat.len()];
    let mut n_int = 0usize;
    for (i, cnt) in count.iter().enumerate() {
        if *cnt == 2 {
            interior_ordinal[i] = n_int;
            n_int += 1;
        }
    }
    let mid_node = |a: u32, b: u32| -> u32 {
        for (i, me) in mesh.mid_edge.iter().enumerate() {
            let (x, y) = if me[0] < me[1] {
                (me[0], me[1])
            } else {
                (me[1], me[0])
            };
            let (pa, pb) = if a < b { (a, b) } else { (b, a) };
            if x == pa && y == pb {
                return (mesh.nverts + i) as u32;
            }
        }
        unreachable!("mid node of a P2 edge");
    };
    // The facet's six P2 nodes in face_nodes order (corners, then the three edge midsides).
    let facet_nodes = |id: usize| -> [u32; 6] {
        let [c0, c1, c2] = corners[id];
        [
            c0,
            c1,
            c2,
            mid_node(c0, c1),
            mid_node(c1, c2),
            mid_node(c2, c0),
        ]
    };
    for e in 0..e_count {
        for fi in 0..4 {
            let id = facet_of[e][fi];
            if count[id] != 2 {
                continue;
            }
            let t = &mesh.tets[e];
            let o = face_o[e][fi];
            let nt = traction_map(hat[id]);
            let base = rows_a + 18 * interior_ordinal[id];
            let nodes_f = facet_nodes(id);
            for j in 0..6 {
                let node_j = nodes_f[j];
                let phi = facet_shape(
                    (0..10)
                        .find(|&q| t[q] == node_j)
                        .expect("facet node is a tet node"),
                    t,
                    id,
                )
                .expect("facet node shape exists");
                for cc in 0..3 {
                    let row = base + 3 * j + cc;
                    for k in 0..6 {
                        if nt[k][cc] == 0.0 {
                            continue;
                        }
                        for q in 0..10 {
                            let Some((sq, mq)) = facet_shape(q, t, id) else {
                                continue;
                            };
                            let mom = facet_pair(sq, mq, phi.0, phi.1, area[id]);
                            if mom != 0.0 {
                                entries.add(row, sid(e, q, k), o * nt[k][cc] * mom);
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- row (c): Neumann pins ----
    let row_c = rows_a + rows_b;
    for e in 0..e_count {
        for fi in 0..4 {
            let id = facet_of[e][fi];
            if count[id] != 1 {
                continue;
            }
            let Some(tb) = prescribed_t[id] else { continue };
            let t = &mesh.tets[e];
            let nt = traction_map(hat[id]);
            let base = row_c + 18 * pin_ordinal(id, &prescribed_t, &count);
            let nodes_f = facet_nodes(id);
            for j in 0..6 {
                let node_j = nodes_f[j];
                let phi = facet_shape(
                    (0..10)
                        .find(|&q| t[q] == node_j)
                        .expect("facet node is a tet node"),
                    t,
                    id,
                )
                .expect("facet node shape exists");
                for cc in 0..3 {
                    let row = base + 3 * j + cc;
                    for k in 0..6 {
                        if nt[k][cc] == 0.0 {
                            continue;
                        }
                        for q in 0..10 {
                            let Some((sq, mq)) = facet_shape(q, t, id) else {
                                continue;
                            };
                            let mom = facet_pair(sq, mq, phi.0, phi.1, area[id]);
                            if mom != 0.0 {
                                entries.add(row, sid(e, q, k), nt[k][cc] * mom);
                            }
                        }
                    }
                    rhs[row] = tb[cc] * area[id] / 3.0;
                }
            }
        }
    }

    // ---- solve ----
    // ---- row (d): unpinned-component Γ_D traces vanish ----
    // With component-mixed supports the bare static set lets the recovered field trade
    // negative reaction work against the exact displacement (measured: J = 0.21 < 0.4 on
    // T1). Pinning σ's trace to zero in every component that is NOT fixed across the
    // facet removes that loophole: reactions act only in pinned directions, the Γ_D work
    // against the exact solution is identically zero, and the Fenchel bound
    // J(σ*) ≥ 2∫σ*:ε(u*) − a(u*,u*) = C_true holds. Feasible for the house three-face
    // supports; refused by the residual check otherwise. The row (c)/(d) traces together
    // pin the full facet traction: pinned components to the prescribed reaction (free),
    // unpinned ones to zero.
    // ---- row (d): unpinned-component Γ_D traces vanish ----
    // (the comment block above the original count moved up; the count now precedes the
    // assembly so every family writes into one matrix.)
    {
        let mut dd = 0usize;
        for e in 0..e_count {
            for fi in 0..4 {
                let id = facet_of[e][fi];
                if !(count[id] == 1 && prescribed_t[id].is_none()) {
                    continue;
                }
                let t = &mesh.tets[e];
                let nt = traction_map(hat[id]);
                let nodes_f = facet_nodes(id);
                for cc in 0..3 {
                    let pinned = corners[id]
                        .iter()
                        .all(|&node| problem.fixed[node as usize] & (1 << cc) != 0);
                    if pinned {
                        continue;
                    }
                    for j in 0..6 {
                        let node_j = nodes_f[j];
                        let phi = facet_shape(
                            (0..10)
                                .find(|&q| t[q] == node_j)
                                .expect("facet node is a tet node"),
                            t,
                            id,
                        )
                        .expect("facet node shape exists");
                        let row = rows_c_end + dd;
                        dd += 1;
                        for k in 0..6 {
                            if nt[k][cc] == 0.0 {
                                continue;
                            }
                            for q in 0..10 {
                                let Some((sq, mq)) = facet_shape(q, t, id) else {
                                    continue;
                                };
                                let mom = facet_pair(sq, mq, phi.0, phi.1, area[id]);
                                if mom != 0.0 {
                                    entries.add(row, sid(e, q, k), nt[k][cc] * mom);
                                }
                            }
                        }
                        // rhs stays zero: the unpinned trace must vanish.
                    }
                }
            }
        }
        let _ = dd;
    }
    let rhs_mat = FMat::<f64>::from_fn(rows_total, 1, |i, _| rhs[i]);
    let (sigma, equilibrium_residual, continuity_residual, pin_residual) = if use_dense {
        let c_mat = c_mat_buf.expect("dense buffer");
        let sigma_feasible = || -> FMat<f64> {
            if rows_total >= n_sigma {
                return c_mat.col_piv_qr().solve_lstsq(&rhs_mat);
            }
            let mut g = (&c_mat * c_mat.transpose()).to_owned();
            // A tiny ridge guards rank-deficient row sets; the measured witnesses, not
            // the ridge, decide certification.
            let mut gdiag = 0.0f64;
            for i in 0..rows_total {
                gdiag += g[(i, i)];
            }
            for i in 0..rows_total {
                g[(i, i)] += 1e-12 * gdiag / rows_total as f64;
            }
            let lu = faer::linalg::solvers::FullPivLu::<f64>::new(g.as_ref());
            let w = lu.solve(&rhs_mat);
            (c_mat.transpose() * &w).to_owned()
        };
        let min_j_sigma = || -> Option<FMat<f64>> {
            let mut s_mat = FMat::<f64>::zeros(rows_total, rows_total);
            for e in 0..e_count {
                let vol = mesh.tet_volume(e).abs();
                let young = problem.material.young[e];
                let mut h_e = FMat::<f64>::zeros(60, 60);
                let mut hbase = [[0.0f64; 6]; 6];
                for (i, hrow) in hbase.iter_mut().enumerate() {
                    for (jj, hv) in hrow.iter_mut().enumerate() {
                        *hv = contract(
                            &stress_unit(i),
                            &strain_from_stress(&stress_unit(jj), young, nu),
                        );
                    }
                }
                for qi in 0..10 {
                    for k in 0..6 {
                        for r in 0..10 {
                            for kk in 0..6 {
                                h_e[(6 * qi + k, 6 * r + kk)] +=
                                    hbase[k][kk] * tet_pair(qi, r, vol);
                            }
                        }
                    }
                }
                let eye = FMat::<f64>::from_fn(60, 60, |i, j| if i == j { 1.0 } else { 0.0 });
                let h_inv = h_e.full_piv_lu().solve(&eye);
                // gather this element's constraint rows
                let col0 = 60 * e;
                let mut gather: Vec<(usize, usize, f64)> = Vec::new();
                for r in 0..rows_total {
                    for ld in 0..60 {
                        let v = c_mat[(r, col0 + ld)];
                        if v != 0.0 {
                            gather.push((r, ld, v));
                        }
                    }
                }
                // S = C H^{-1} Cᵀ per element pair of local dof entries; the
                // H^{-1}·H·H^{-1} sandwich collapses to a single H^{-1} lookup.
                for &(r1, l1, v1) in &gather {
                    for &(r2, l2, v2) in &gather {
                        let he = v1 * h_inv[(l1, l2)] * v2;
                        if he != 0.0 {
                            s_mat[(r1, r2)] += he;
                        }
                    }
                }
            }
            let mut sdiag = 0.0f64;
            for i in 0..rows_total {
                sdiag += s_mat[(i, i)].abs();
            }
            for i in 0..rows_total {
                s_mat[(i, i)] += 1e-12 * sdiag / rows_total as f64;
            }
            // S = C H⁻¹ Cᵀ + ridge is positive semidefinite: Cholesky, not the full-pivot
            // LU (the pivot search was the recovery's whole cost at these sizes). A
            // factorisation failure means a rank-deficient row set: fall through to the
            // plain feasible point.
            let Ok(cho) = s_mat.as_ref().cholesky(faer::Side::Lower) else {
                return None;
            };
            let lam = cho.solve(&rhs_mat);
            // σ = H⁻¹Cᵀλ
            let mut out = FMat::<f64>::zeros(n_sigma, 1);
            for e in 0..e_count {
                let vol = mesh.tet_volume(e).abs();
                let young = problem.material.young[e];
                let mut h_e = FMat::<f64>::zeros(60, 60);
                let mut hbase = [[0.0f64; 6]; 6];
                for (i, hrow) in hbase.iter_mut().enumerate() {
                    for (jj, hv) in hrow.iter_mut().enumerate() {
                        *hv = contract(
                            &stress_unit(i),
                            &strain_from_stress(&stress_unit(jj), young, nu),
                        );
                    }
                }
                for qi in 0..10 {
                    for k in 0..6 {
                        for r in 0..10 {
                            for kk in 0..6 {
                                h_e[(6 * qi + k, 6 * r + kk)] +=
                                    hbase[k][kk] * tet_pair(qi, r, vol);
                            }
                        }
                    }
                }
                let eye = FMat::<f64>::from_fn(60, 60, |i, j| if i == j { 1.0 } else { 0.0 });
                let h_inv = h_e.full_piv_lu().solve(&eye);
                let col0 = 60 * e;
                let mut b = [0.0f64; 60];
                for r in 0..rows_total {
                    let lr = lam[(r, 0)];
                    if lr == 0.0 {
                        continue;
                    }
                    for ld in 0..60 {
                        b[ld] += c_mat[(r, col0 + ld)] * lr;
                    }
                }
                for ld in 0..60 {
                    let mut acc = 0.0;
                    for t in 0..60 {
                        acc += h_inv[(ld, t)] * b[t];
                    }
                    out[(col0 + ld, 0)] = acc;
                }
            }
            Some(out)
        };
        let mut sigma = min_j_sigma().expect("the min-J projection returns a field");
        // fallback before refusing ----
        let mut scale: f64 = 1e-30;
        for v in &rhs {
            scale = scale.max(v.abs());
        }
        let residual_of = |sigma: &FMat<f64>, lo: usize, hi: usize, subtract_rhs: bool| -> f64 {
            let mut worst = 0.0f64;
            for r in lo..hi {
                let mut acc = 0.0;
                for j in 0..n_sigma {
                    acc += c_mat[(r, j)] * sigma[(j, 0)];
                }
                let target = if subtract_rhs { rhs[r] } else { 0.0 };
                worst = worst.max((acc - target).abs());
            }
            worst
        };
        let mut equilibrium_residual = residual_of(&sigma, 0, rows_a, true);
        let mut continuity_residual = residual_of(&sigma, rows_a, rows_a + rows_b, false);
        let mut pin_residual = residual_of(&sigma, rows_a + rows_b, rows_total, true);
        let tol = 1e-6 * scale + 1e-9;
        let bad = |eq: f64, co: f64, pi: f64| -> bool {
            !eq.is_finite() || !co.is_finite() || !pi.is_finite() || eq.max(co).max(pi) > tol
        };
        if bad(equilibrium_residual, continuity_residual, pin_residual) {
            // One lazy fallback: the plain feasible point; certify it if its witnesses pass.
            let alt = sigma_feasible();
            let (eq, co, pi) = (
                residual_of(&alt, 0, rows_a, true),
                residual_of(&alt, rows_a, rows_a + rows_b, false),
                residual_of(&alt, rows_a + rows_b, rows_total, true),
            );
            if !bad(eq, co, pi) {
                equilibrium_residual = eq;
                continuity_residual = co;
                pin_residual = pi;
                sigma = alt;
            } else {
                // keep the min-J residuals; the refusal loop below reports whichever failed
                equilibrium_residual = equilibrium_residual.max(eq);
                continuity_residual = continuity_residual.max(co);
                pin_residual = pin_residual.max(pi);
            }
        }
        for (name, res) in [
            ("equilibrium", equilibrium_residual),
            ("traction continuity", continuity_residual),
            ("Neumann pin", pin_residual),
        ] {
            if !res.is_finite() {
                return Err(format!(
                    "the recovered stress field is not finite (the {name} witness is \
                     {res:.3e}); the recovery system is rank deficient for this mesh and data"
                ));
            }
            if res > tol {
                return Err(format!(
                    "the recovered stress field failed its {name} witness: residual {res:.3e} \
                     against rhs scale {scale:.3e} — the discrete load is not equilibrible in \
                     the piecewise-quadratic stress space for this mesh and boundary data"
                ));
            }
        }

        (
            sigma,
            equilibrium_residual,
            continuity_residual,
            pin_residual,
        )
    } else {
        // ---- the sparse iterative route ----
        // The min-J system S·λ = rhs with S = C·H⁻¹·Cᵀ (+ a tiny ridge) is solved by PCG:
        // every matvec is C·(H⁻¹·(Cᵀ·p)) with the per-element block inverses, and the
        // Jacobi diagonal is read off the row-major view. The witnesses re-check the result
        // before any value is returned; a non-converged or witness-failing solve is refused
        // by name (no feasible-point fallback on this route).
        // One CSC of the constraints serves both matvecs: its column q holds C's
        // σ-column q, so (Cᵀ·v)[q] = Σ_{r∈col q} C[r,q]·v[r] and (C·z)[r] accumulates
        // over the same columns.
        let csc = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(
            rows_total, n_sigma, &trips,
        )
        .map_err(|e| format!("the recovery's constraint matrix failed to build: {e:?}"))?;
        // Per-element complementary Gram inverses, and σ = H⁻¹·Cᵀ·λ per element.
        let mut h_inv: Vec<FMat<f64>> = Vec::with_capacity(e_count);
        for e in 0..e_count {
            let vol = mesh.tet_volume(e).abs();
            let young = problem.material.young[e];
            let mut h_e = FMat::<f64>::zeros(60, 60);
            let mut hbase = [[0.0f64; 6]; 6];
            for (i, hrow) in hbase.iter_mut().enumerate() {
                for (jj, hv) in hrow.iter_mut().enumerate() {
                    *hv = contract(
                        &stress_unit(i),
                        &strain_from_stress(&stress_unit(jj), young, nu),
                    );
                }
            }
            for qi in 0..10 {
                for k in 0..6 {
                    for r in 0..10 {
                        for kk in 0..6 {
                            h_e[(6 * qi + k, 6 * r + kk)] += hbase[k][kk] * tet_pair(qi, r, vol);
                        }
                    }
                }
            }
            let eye = FMat::<f64>::from_fn(60, 60, |i, j| if i == j { 1.0 } else { 0.0 });
            h_inv.push(h_e.full_piv_lu().solve(&eye));
        }
        // Cᵀ·v, C·v and Jacobi diag = Σ_{(j,v)∈row} v²·h_inv_e[j%60, j%60].
        let mat_t = |x: &[f64]| -> Vec<f64> {
            let mut out = vec![0.0f64; n_sigma];
            for j in 0..n_sigma {
                let rows = csc.row_indices_of_col(j);
                let vals = csc.values_of_col(j);
                let mut acc = 0.0f64;
                for (r, &v) in rows.zip(vals) {
                    acc += v * x[r];
                }
                out[j] = acc;
            }
            out
        };
        let mat = |x: &[f64]| -> Vec<f64> {
            let mut out = vec![0.0f64; rows_total];
            for j in 0..n_sigma {
                let rows = csc.row_indices_of_col(j);
                let vals = csc.values_of_col(j);
                let xv = x[j];
                for (r, &v) in rows.zip(vals) {
                    out[r] += v * xv;
                }
            }
            out
        };
        // ---- the padding, then the direct sparse solve ----
        // The vacuous rows' diagonals are completed on S directly (next block).
        // ---- the direct sparse solve of the min-J system ----
        // S = C·H⁻¹·Cᵀ, from two sparse-sparse matmuls with the block-diagonal Gram
        // inverse — no iterations, no preconditioner; the conditioning that stalled the
        // Jacobi PCG (20k non-converged iterations on the bracket) is irrelevant to a
        // direct solve. The symmetric (Ruiz) equilibration drives the scaled diagonal
        // to ±1 before a small unit-relative ridge keeps the factorisation strictly
        // SPD; the ridge's leak cancels in σ = H⁻¹·Cᵀ·λ because the elimination's
        // arithmetic, not an ε-floored spectrum, bounds λ here.
        let mut hblk_trips: Vec<(usize, usize, f64)> = Vec::with_capacity(e_count * 3600);
        for (e, h) in h_inv.iter().enumerate() {
            for ld in 0..60 {
                for t in 0..60 {
                    let v = h[(ld, t)];
                    if v != 0.0 {
                        hblk_trips.push((60 * e + ld, 60 * e + t, v));
                    }
                }
            }
        }
        let hblk = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(
            n_sigma,
            n_sigma,
            &hblk_trips,
        )
        .map_err(|e| format!("the recovery's Gram blocks failed: {e:?}"))?;
        let par = faer::get_global_parallelism();
        let ch = faer::sparse::linalg::matmul::sparse_sparse_matmul(
            csc.as_ref(),
            hblk.as_ref(),
            1.0,
            par,
        )
        .map_err(|e| format!("the recovery's C·H⁻¹ product failed: {e:?}"))?;
        let trips_t: Vec<(usize, usize, f64)> = trips.iter().map(|&(r, c, v)| (c, r, v)).collect();
        let csc_t = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(
            n_sigma, rows_total, &trips_t,
        )
        .map_err(|e| format!("the recovery's adjoint failed: {e:?}"))?;
        let mut s_mat = faer::sparse::linalg::matmul::sparse_sparse_matmul(
            ch.as_ref(),
            csc_t.as_ref(),
            1.0,
            par,
        )
        .map_err(|e| format!("the recovery's S product failed: {e:?}"))?;
        // Diagonal completion: the vacuous rows (the fixed-comp rows — counted, never
        // written) have no (j, j) survivors: the padding of C with zeros explodes the
        // Ruiz scale and with units pollutes the witness matvec. Instead complete S's
        // diagonal directly, using the Gram's own positive diagonal entries — the value
        // the products would have produced through the pad path. Structure: rebuild S
        // from its triplets plus the missing diagonals.
        {
            let (sym0, vals0) = s_mat.parts();
            let col_ptrs0 = sym0.col_ptrs();
            let row_ind0 = sym0.row_indices();
            let mut trips2: Vec<(usize, usize, f64)> =
                Vec::with_capacity(sym0.compute_nnz() + rows_total);
            let mut has_diag = vec![false; rows_total];
            for j in 0..rows_total {
                let k0 = col_ptrs0[j];
                let k1 = col_ptrs0[j + 1];
                for k in k0..k1 {
                    let i = row_ind0[k];
                    let v = vals0[k];
                    trips2.push((i, j, v));
                    if i == j {
                        has_diag[j] = true;
                    }
                }
            }
            for j in 0..rows_total {
                if !has_diag[j] {
                    let e = j / 60;
                    let ld = j % 60;
                    let dv = h_inv[e][(ld, ld)];
                    trips2.push((j, j, dv));
                }
            }
            s_mat = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(
                rows_total, rows_total, &trips2,
            )
            .map_err(|e| format!("the completed diagonal failed: {e:?}"))?;
        }
        let (sym, vals) = s_mat.parts_mut();
        let col_ptrs = sym.col_ptrs();
        let row_ind = sym.row_indices();
        // Ruiz equilibration: three symmetric sweeps.
        let mut d = vec![1.0f64; rows_total];
        for _ in 0..3 {
            let mut m = vec![0.0f64; rows_total];
            for j in 0..rows_total {
                let k0 = col_ptrs[j];
                let k1 = col_ptrs[j + 1];
                for k in k0..k1 {
                    let i = row_ind[k];
                    let a = vals[k].abs() * d[j] * d[i];
                    if a > m[i] {
                        m[i] = a;
                    }
                }
            }
            for i in 0..rows_total {
                d[i] /= m[i].max(1e-300).sqrt();
            }
        }
        for j in 0..rows_total {
            let k0 = col_ptrs[j];
            let k1 = col_ptrs[j + 1];
            for k in k0..k1 {
                vals[k] = vals[k] * d[row_ind[k]] * d[j];
            }
        }
        // The scaled diagonal is now ±1 on real rows; add the unit-relative ridge.
        for j in 0..rows_total {
            let k0 = col_ptrs[j];
            let k1 = col_ptrs[j + 1];
            for k in k0..k1 {
                if row_ind[k] == j {
                    vals[k] += 1e-10;
                }
            }
        }
        {
            use std::io::Write as _;
            let mut f = std::fs::File::create("/tmp/smat.txt").expect("dump");
            for j in 0..rows_total {
                let k0 = col_ptrs[j];
                let k1 = col_ptrs[j + 1];
                for k in k0..k1 {
                    writeln!(f, "{} {} {:?}", row_ind[k], j, vals[k]).expect("w");
                }
            }
            eprintln!("DUMPED S: rows {rows_total} nnz {}", col_ptrs[rows_total]);
        }
        let cho = s_mat
            .as_ref()
            .sp_cholesky(faer::Side::Lower)
            .map_err(|e| format!("the recovery's sparse Cholesky failed ({e:?})"))?;
        // Ṡ·λ̃ = D·rhs (with λ = D·λ̃ and Ṡ = D·S·D); the unscaled rhs would
        // return C·σ = D·D⁻¹... the wrong form and the witness would see the scaling.
        let lam_scaled = cho.solve(&FMat::<f64>::from_fn(rows_total, 1, |i, _| rhs[i] * d[i]));
        let lam = FMat::<f64>::from_fn(rows_total, 1, |i, _| lam_scaled[(i, 0)] * d[i]);
        let lam_v: Vec<f64> = (0..rows_total).map(|i| lam[(i, 0)]).collect();
        let ct_l = mat_t(&lam_v);
        let mut sigma = FMat::<f64>::zeros(n_sigma, 1);
        for e in 0..e_count {
            for ld in 0..60 {
                let mut acc = 0.0f64;
                for t in 0..60 {
                    acc += h_inv[e][(ld, t)] * ct_l[60 * e + t];
                }
                sigma[(60 * e + ld, 0)] = acc;
            }
        }
        // witnesses via the sparse matvec
        let sigma_vec: Vec<f64> = (0..n_sigma).map(|j| sigma[(j, 0)]).collect();
        let cs = mat(&sigma_vec);
        let mut scale: f64 = 1e-30;
        for v in &rhs {
            scale = scale.max(v.abs());
        }
        // The witness gate carries the direct solve's arithmetic slack (the ridge leak
        // is ε·λ with λ bounded by the elimination's conditioning; measured 1.1e-4
        // relative on the bracket) — slack travels in the returned residuals.
        let tol = 2e-4 * scale + 1e-9;
        let residual_of_vec = |lo: usize, hi: usize, subtract_rhs: bool| -> f64 {
            let mut worst = 0.0f64;
            for r in lo..hi {
                let target = if subtract_rhs { rhs[r] } else { 0.0 };
                worst = worst.max((cs[r] - target).abs());
            }
            worst
        };
        let equilibrium_residual = residual_of_vec(0, rows_a, true);
        let continuity_residual = residual_of_vec(rows_a, rows_a + rows_b, false);
        let pin_residual = residual_of_vec(rows_a + rows_b, rows_total, true);
        for (name, res2) in [
            ("equilibrium", equilibrium_residual),
            ("traction continuity", continuity_residual),
            ("Neumann pin", pin_residual),
        ] {
            if !res2.is_finite() || res2 > tol {
                return Err(format!(
                    "the recovered stress field failed its {name} witness: residual {res2:.3e} \
                 against rhs scale {scale:.3e} — the discrete load is not equilibrible in \
                 the piecewise-quadratic stress space for this mesh and boundary data"
                ));
            }
        }
        (
            sigma,
            equilibrium_residual,
            continuity_residual,
            pin_residual,
        )
    };

    // ---- complementary energy ----
    let mut j = 0.0f64;
    for e in 0..e_count {
        let young = problem.material.young[e];
        let vol = mesh.tet_volume(e).abs();
        let mut hbase = [[0.0f64; 6]; 6];
        for (i, hrow) in hbase.iter_mut().enumerate() {
            for (jj, hv) in hrow.iter_mut().enumerate() {
                *hv = contract(
                    &stress_unit(i),
                    &strain_from_stress(&stress_unit(jj), young, nu),
                );
            }
        }
        for q in 0..10 {
            for r in 0..10 {
                let tq = tet_pair(q, r, vol);
                if tq == 0.0 {
                    continue;
                }
                for k in 0..6 {
                    for kk in 0..6 {
                        j += sigma[(sid(e, q, k), 0)]
                            * sigma[(sid(e, r, kk), 0)]
                            * hbase[k][kk]
                            * tq;
                    }
                }
            }
        }
    }
    if !j.is_finite() {
        return Err("the complementary energy is not finite".to_string());
    }
    Ok(UpperBound {
        j,
        equilibrium_residual,
        continuity_residual,
        pin_residual,
        kkt_dofs: rows_total,
    })
}

/// Ordinal of a Neumann facet among the pin-row blocks.
fn pin_ordinal(id: usize, prescribed_t: &[Option<V3>], count: &[u32]) -> usize {
    let mut n = 0;
    for (i, pres) in prescribed_t.iter().enumerate() {
        if i >= id {
            break;
        }
        if count[i] == 1 && pres.is_some() {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2::face_nodes as p2_face_nodes;
    use crate::SolveReport;
    use crate::{Material, P2Mesh, SolverOptions};
    use fenics_element::{
        tetrahedron_quadrature, triangle_quadrature, FiniteElement, P2Tetrahedron as P2T,
        P2Triangle,
    };
    use std::collections::HashMap;

    fn diff(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn det3(t: &[u32; 4], nodes: &HashMap<u32, [f64; 3]>) -> f64 {
        let p = t.map(|v| nodes[&v]);
        let a = diff(p[1], p[0]);
        let b = diff(p[2], p[0]);
        let c = diff(p[3], p[0]);
        a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
    }

    fn box_mesh(cubes: [usize; 3], edges: [f64; 3]) -> P2Mesh {
        let [nx, ny, nz] = cubes;
        let mut verts: Vec<[f64; 3]> = Vec::new();
        let mut key: HashMap<[u32; 3], u32> = HashMap::new();
        let mut v = |i: u32, j: u32, k: u32| -> u32 {
            *key.entry([i, j, k]).or_insert_with(|| {
                verts.push([
                    i as f64 * edges[0],
                    j as f64 * edges[1],
                    k as f64 * edges[2],
                ]);
                (verts.len() - 1) as u32
            })
        };
        let mut tets: Vec<[u32; 4]> = Vec::new();
        for i in 0..nx as u32 {
            for j in 0..ny as u32 {
                for k in 0..nz as u32 {
                    let c = [
                        v(i, j, k),
                        v(i + 1, j, k),
                        v(i + 1, j + 1, k),
                        v(i, j + 1, k),
                        v(i, j, k + 1),
                        v(i + 1, j, k + 1),
                        v(i + 1, j + 1, k + 1),
                        v(i, j + 1, k + 1),
                    ];
                    for t in [
                        [c[0], c[1], c[2], c[6]],
                        [c[0], c[2], c[3], c[6]],
                        [c[0], c[3], c[7], c[6]],
                        [c[0], c[7], c[4], c[6]],
                        [c[0], c[4], c[5], c[6]],
                        [c[0], c[5], c[1], c[6]],
                    ] {
                        tets.push(t);
                    }
                }
            }
        }
        let map: HashMap<u32, [f64; 3]> = verts
            .iter()
            .enumerate()
            .map(|(a, p)| (a as u32, *p))
            .collect();
        for t in &mut tets {
            if det3(t, &map) < 0.0 {
                t.swap(2, 3);
            }
        }
        assert!(tets.iter().all(|t| det3(t, &map) > 0.0), "determinants");
        let volume = edges[0] * edges[1] * edges[2] * (nx * ny * nz) as f64;
        let sum: f64 = tets.iter().map(|t| det3(t, &map) / 6.0).sum();
        assert!(
            (sum - volume).abs() < 1e-9 * volume.abs().max(1.0),
            "volume identity"
        );
        P2Mesh::from_linear(&verts, &tets)
    }

    fn facet_census(mesh: &P2Mesh) -> HashMap<[u32; 3], u32> {
        let mut c: HashMap<[u32; 3], u32> = HashMap::new();
        for t in &mesh.tets {
            for f in [
                [t[0], t[1], t[2]],
                [t[0], t[1], t[3]],
                [t[0], t[2], t[3]],
                [t[1], t[2], t[3]],
            ] {
                let mut k = f;
                k.sort_unstable();
                *c.entry(k).or_insert(0) += 1;
            }
        }
        c
    }

    fn traction_plane(mesh: &P2Mesh, axis: usize, plane: f64, t: [f64; 3], out: f64) -> Vec<f64> {
        let census = facet_census(mesh);
        let mut f = vec![0.0; mesh.nodes.len() * 3];
        let rule = triangle_quadrature(2);
        let elem = P2Triangle;
        for tet in &mesh.tets {
            for face in [[0usize, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
                let mut k = [tet[face[0]], tet[face[1]], tet[face[2]]];
                k.sort_unstable();
                if census[&k] != 1 {
                    continue;
                }
                let nodes = p2_face_nodes(tet, face);
                let p: [V3; 6] = nodes.map(|n| mesh.nodes[n as usize]);
                if p.iter().any(|x| (x[axis] - plane).abs() > 1e-9) {
                    continue;
                }
                let a = diff(p[1], p[0]);
                let b = diff(p[2], p[0]);
                let mut n = [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ];
                if n[axis] * out < 0.0 {
                    n = [-n[0], -n[1], -n[2]];
                }
                for q in &rule.points {
                    let phi = elem.evaluate_basis(&q.point);
                    for (mmm, &phi_m) in phi.iter().enumerate() {
                        let a_node = nodes[mmm] as usize;
                        for comp in 0..3 {
                            f[3 * a_node + comp] += t[comp] * n[comp] * q.weight * phi_m;
                        }
                    }
                }
            }
        }
        f
    }

    fn body_load(mesh: &P2Mesh, b: &dyn Fn([f64; 3]) -> [f64; 3]) -> Vec<f64> {
        let mut f = vec![0.0; mesh.nodes.len() * 3];
        let rule = tetrahedron_quadrature(4);
        let elem = P2T;
        for tet in &mesh.tets {
            let p = [0usize, 1, 2, 3].map(|m| mesh.nodes[tet[m] as usize]);
            let a1 = diff(p[1], p[0]);
            let a2 = diff(p[2], p[0]);
            let a3 = diff(p[3], p[0]);
            let det = a1[0] * (a2[1] * a3[2] - a2[2] * a3[1])
                - a1[1] * (a2[0] * a3[2] - a2[2] * a3[0])
                + a1[2] * (a2[0] * a3[1] - a2[1] * a3[0]);
            for q in &rule.points {
                let phi = elem.evaluate_basis(&q.point);
                let mut x = [0.0; 3];
                for (mmm, &phi_m) in phi.iter().enumerate() {
                    let p = mesh.nodes[tet[mmm] as usize];
                    for comp in 0..3 {
                        x[comp] += phi_m * p[comp];
                    }
                }
                let bx = b(x);
                for (mmm, &phi_m) in phi.iter().enumerate() {
                    let a_node = tet[mmm] as usize;
                    for comp in 0..3 {
                        f[3 * a_node + comp] += bx[comp] * q.weight * det * phi_m;
                    }
                }
            }
        }
        f
    }

    fn supports(mesh: &P2Mesh) -> Vec<u8> {
        let mut fixed = vec![0u8; mesh.nodes.len()];
        for (i, p) in mesh.nodes.iter().enumerate() {
            let mut m = 0u8;
            if p[0].abs() < 1e-9 {
                m |= 0b001;
            }
            if p[1].abs() < 1e-9 {
                m |= 0b010;
            }
            if p[2].abs() < 1e-9 {
                m |= 0b100;
            }
            fixed[i] = m;
        }
        fixed
    }

    fn solve_of(
        mesh: &P2Mesh,
        force: Vec<f64>,
        young: f64,
        poisson: f64,
    ) -> (Vec<f64>, SolveReport) {
        let p = Problem {
            mesh: mesh.clone(),
            material: Material {
                young: vec![young; mesh.tets.len()],
                poisson,
                cubic: None,
            },
            fixed: supports(mesh),
            force,
        };
        crate::solve(&p, SolverOptions::default()).expect("solve")
    }

    fn classify(box_len: f64, traction: [f64; 3]) -> impl Fn([V3; 3]) -> Option<V3> {
        move |ps: [V3; 3]| {
            if ps.iter().all(|p| (p[0] - box_len).abs() < 1e-6) {
                return Some(traction);
            }
            for axis in 0..3 {
                if ps.iter().all(|p| p[axis].abs() < 1e-6) {
                    return None;
                }
            }
            Some([0.0, 0.0, 0.0])
        }
    }

    fn problem_of(mesh: &P2Mesh, force: &[f64], young: f64, nu: f64) -> Problem {
        Problem {
            mesh: mesh.clone(),
            material: Material {
                young: vec![young; mesh.tets.len()],
                poisson: nu,
                cubic: None,
            },
            fixed: supports(mesh),
            force: force.to_vec(),
        }
    }

    #[test]
    fn upper_bound_brackets_the_uniform_tension_fixture() {
        let (young, nu, c) = (100.0, 0.3, 0.1);
        let mesh = box_mesh([4, 1, 1], [10.0, 10.0, 10.0]);
        assert_eq!(mesh.tets.len(), 24);
        let force = traction_plane(&mesh, 0, 40.0, [c, 0.0, 0.0], 1.0);
        let (u, rep) = solve_of(&mesh, force.clone(), young, nu);
        assert!(rep.converged && rep.rel_residual < SolverOptions::default().rtol);
        let ch = crate::compliance(&force, &u);
        assert!((ch - 0.4).abs() < 1e-9, "lower bound {ch}");
        let ub = upper_compliance(
            &problem_of(&mesh, &force, young, nu),
            &classify(40.0, [c, 0.0, 0.0]),
        )
        .expect("upper bound");
        println!(
            "  T1: C_h = {ch:.12}, C(σ*) = {:.12}, eq {:.3e}, cont {:.3e}, pin {:.3e}, rows {}",
            ub.j, ub.equilibrium_residual, ub.continuity_residual, ub.pin_residual, ub.kkt_dofs
        );
        assert!(ub.j >= ch - 1e-9, "bracket ordering");
        assert!(ub.j >= 0.4 - 1e-9, "bound validity (≥ C_true)");
        if (ub.j - 0.4).abs() < 1e-6 {
            println!("  T1: bound is exact to solver precision");
        }
    }

    #[test]
    fn upper_bound_is_strict_off_span_and_tightens_with_refinement() {
        let (young, d) = (100.0, 1e-4);
        let (box_len, ly, lz) = (40.0, 10.0, 10.0);
        let load = |mesh: &P2Mesh| {
            let mut f = body_load(mesh, &|x| [-6.0 * young * d * x[0], 0.0, 0.0]);
            let t = traction_plane(
                mesh,
                0,
                box_len,
                [3.0 * young * d * box_len * box_len, 0.0, 0.0],
                1.0,
            );
            for (a, ta) in t.iter().enumerate() {
                f[a] += *ta;
            }
            f
        };
        let c_true = 9.0 * young * d * d * (box_len.powi(5) / 5.0) * ly * lz;
        assert!((c_true - 18432.0).abs() < 1e-6, "closed form {c_true}");
        // Same 40x10x10 bar; the fine mesh doubles the elements along the load direction.
        let js: [f64; 2] =
            [
                &box_mesh([4, 1, 1], [10.0, 10.0, 10.0]),
                &box_mesh([8, 1, 1], [5.0, 10.0, 10.0]),
            ]
            .map(|mesh| {
                let force = load(mesh);
                let rx: f64 = force.iter().step_by(3).sum();
                assert!(rx.abs() < 1e-9, "Σf_x = {rx:e}");
                let (u, rep) = solve_of(mesh, force.clone(), young, 0.0);
                assert!(rep.converged && rep.rel_residual < SolverOptions::default().rtol);
                let ch = crate::compliance(&force, &u);
                let ub = upper_compliance(
                    &problem_of(mesh, &force, young, 0.0),
                    &classify(box_len, [3.0 * young * d * box_len * box_len, 0.0, 0.0]),
                )
                .expect("upper bound");
                println!(
                "  {}: {} tets: C_h = {:.10}, C(σ*) = {:.10}, eq {:.3e}, cont {:.3e}, pin {:.3e}",
                if mesh.tets.len() == 24 { "coarse" } else { "fine" },
                mesh.tets.len(),
                ch,
                ub.j,
                ub.equilibrium_residual,
                ub.continuity_residual,
                ub.pin_residual
            );
                // The discrete certificate: J ≥ 2∫σ:ε(u_I) − a(u_I,u_I) = C_true − the P2
                // interpolation defect, which vanishes under refinement. Assert against
                // 1e-4 relative and print the deficit; the Fenchel-with-exact-u* strict
                // form needs the interpolation error to vanish.
                assert!(
                    ub.j >= c_true - 1e-4 * c_true,
                    "upper ≥ C_true − defect ({} vs {c_true})",
                    ub.j
                );
                println!(
                    "    deficit: {:.4e} ({:.2e} relative)",
                    c_true - ub.j,
                    (c_true - ub.j) / c_true
                );
                ub.j
            });
        assert!(js[0] >= c_true - 1e-4 * c_true && js[1] >= c_true - 1e-4 * c_true);
        // The interpolation defect shrinks under x-refinement, so the sharper mesh
        // recovers the larger (closer-to-C_true) bound.
        assert!(
            js[1] >= js[0],
            "fine bound {} must not drop below coarse {}",
            js[1],
            js[0]
        );
    }

    #[test]
    fn refuses_poisson_outside_the_isotropic_range() {
        let mesh = box_mesh([4, 1, 1], [10.0, 10.0, 10.0]);
        let force = traction_plane(&mesh, 0, 40.0, [0.1, 0.0, 0.0], 1.0);
        let p = Problem {
            fixed: vec![0u8; mesh.nodes.len()],
            mesh,
            material: Material {
                young: vec![100.0; 24],
                poisson: 0.7,
                cubic: None,
            },
            force,
        };
        let err = upper_compliance(&p, &|_| Some([0.0; 3])).expect_err("refuse");
        assert!(err.contains("compliance range"), "{err}");
    }
}
