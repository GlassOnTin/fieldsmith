//! 3D P2 unilateral Signorini–Winkler boundary contact (`VISION.md §5.2 Pillar IV.2`).
//!
//! Solves the variational inequality / complementarity boundary value problem on 6-node
//! quadratic (`P2Triangle`) surface facets against a rigid foundation or mating seat:
//!
//! - Normal penetration into obstacle: `δ_n(x) = u_h(x) · n_hat − g_0`
//! - Unilateral pressure law: `p_n(x) = k_n max(0, δ_n(x)) ≥ 0` (`MPa`)
//! - Complementarity: `p_n ≥ 0`, `δ_n ≤ 0` in the `k_n → ∞` limit; zero tensile traction
//!   (`p_n = 0`) wherever the boundary separates / lifts off (`δ_n ≤ 0`).
//!
//! Discretisation uses the 9-point positive-weight Duffy Gauss–Legendre rule (`degree 4`
//! exact on affine `P2Triangle` facets) inside a Semi-Smooth Newton (SSN) active-set loop.
//! Because every quadrature weight `w_q > 0` and normal stiffness `k_n > 0`, every active
//! rank-1 normal tangent contribution `w_q k_n (φ(x_q) ⊗ n_hat)(φ(x_q) ⊗ n_hat)ᵀ` is
//! symmetric positive semi-definite (free of P2 vertex lumped-weight sign inversions) and
//! sits strictly within the owning 10-node tetrahedron's existing 3×3 [`crate::Bsr`] pattern.

use crate::assemble::assemble;
use crate::bsr::Bsr;
use crate::p2::P2Mesh;
use crate::pcg::{
    apply_dirichlet, configure_host_parallelism, pcg_solve, SolveReport, SolverOptions, TwoLevel,
};
use crate::{Material, V3};
use fenics_element::{interval_quadrature, FiniteElement, P2Triangle};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Maximum Semi-Smooth Newton active-set iterations before returning the damped iterate.
pub const MAX_CONTACT_SSN_ITERS: usize = 16;

/// Hysteresis threshold (mm) on quadrature penetration `δ_n` to prevent floating-point
/// chattering at the exact zero-pressure grazing boundary.
const ACTIVE_PENETRATION_TOL_MM: f64 = 1.0e-13;

/// One 6-node quadratic boundary triangle belonging to a [`ContactPatch`].
#[derive(Clone, Debug)]
pub struct ContactFacet {
    /// Global node IDs `[c0, c1, c2, m01, m12, m20]` in [`P2Triangle`] order.
    pub nodes: [u32; 6],
    /// Unit obstacle normal `n_hat` pointing from the body toward the rigid seat/obstacle.
    pub normal: V3,
}

/// One `.fsm` `contact` boundary patch resolved onto the P2 surface mesh.
#[derive(Clone, Debug)]
pub struct ContactPatch {
    pub id: String,
    pub region: String,
    /// Normal penalty foundation stiffness `k_n > 0` (`MPa/mm`).
    pub k_mpa_mm: f64,
    /// Initial clearance `g_0 ≥ 0` (`mm`) before compressive contact engages.
    pub gap_mm: f64,
    pub facets: Vec<ContactFacet>,
}

/// Certified summary for one unilateral [`ContactPatch`] on a solved load or thermal case.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContactReport {
    pub id: String,
    pub region: String,
    /// Peak normal contact pressure `max p_n` across the patch (`MPa`, `≥ 0`).
    pub max_contact_pressure_mpa: f64,
    /// Integrated normal contact reaction force `∫_{Γ_c} p_n dA` (`N`, `≥ 0`).
    pub contact_force_n: f64,
    /// Total geometric area of the contact region (`mm²`).
    pub total_area_mm2: f64,
    /// Active compressed area `∫_{δ_n > 0} dA` (`mm²`).
    pub active_area_mm2: f64,
    /// Fraction of contact region area in active compression (`0..1`).
    pub contact_area_ratio: f64,
    /// Peak elastic foundation penetration `max(0, u · n_hat − gap)` (`mm`, `≥ 0`).
    pub max_penetration_mm: f64,
    /// Peak normal separation / heel liftoff `max(0, gap − u · n_hat)` (`mm`, `≥ 0`).
    pub max_liftoff_mm: f64,
    /// Number of Semi-Smooth Newton active-set iterations executed.
    pub active_set_iters: usize,
}

/// Result of [`solve_unilateral_contact`].
#[derive(Clone, Debug)]
pub struct ContactSolveOutput {
    /// Nodal displacement vector (`3 * mesh.nodes.len()`, `mm`).
    pub u: Vec<f64>,
    /// Aggregated linear-solver provenance (`iterations` = sum across SSN solves).
    pub solve_report: SolveReport,
    /// Per-patch certified contact summaries in `patches` order.
    pub reports: Vec<ContactReport>,
    /// Nodal contact pressure field (`mesh.nodes.len()`, `MPa`, `0.0` outside active contact).
    pub nodal_pressure_mpa: Vec<f64>,
}

#[derive(Clone, Copy)]
struct QuadSample {
    patch_idx: usize,
    nodes: [u32; 6],
    normal: V3,
    phi: [f64; 6],
    da_mm2: f64,
    k_mpa_mm: f64,
    gap_mm: f64,
}

#[inline]
fn is_fixed(fixed: &[u8], node: usize, c: usize) -> bool {
    fixed[node] & (1 << c) != 0
}

#[inline]
fn normalize_unit(v: V3) -> Result<V3, String> {
    let n2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if !n2.is_finite() || n2 <= 1.0e-24 {
        return Err("contact normal vector must be finite and non-zero".into());
    }
    let inv = 1.0 / n2.sqrt();
    Ok([v[0] * inv, v[1] * inv, v[2] * inv])
}

/// Build precomputed 9-point degree-4 Duffy Gauss–Legendre quadrature samples across all
/// [`ContactPatch`]es, plus each patch's exact geometric surface area (`mm²`).
fn build_quad_samples(
    mesh: &P2Mesh,
    patches: &[ContactPatch],
) -> Result<(Vec<QuadSample>, Vec<f64>), String> {
    let gl = interval_quadrature(5);
    let elem = P2Triangle;
    let mut ref_pts = Vec::with_capacity(9);
    for wu in &gl.points {
        let u = wu.point[0];
        for wv in &gl.points {
            let v = wv.point[0];
            let xi = [u, v * (1.0 - u)];
            let s = wu.weight * wv.weight * (1.0 - u);
            let phi_vec = elem.evaluate_basis(&xi);
            let dn_vec = elem.evaluate_gradients(&xi);
            let mut phi = [0.0f64; 6];
            let mut dn = [[0.0f64; 2]; 6];
            phi.copy_from_slice(&phi_vec[..6]);
            dn.copy_from_slice(&dn_vec[..6]);
            ref_pts.push((s, phi, dn));
        }
    }

    let total_facets: usize = patches.iter().map(|p| p.facets.len()).sum();
    let mut samples = Vec::with_capacity(total_facets * 9);
    let mut patch_areas = vec![0.0f64; patches.len()];

    for (pi, patch) in patches.iter().enumerate() {
        if !(patch.k_mpa_mm.is_finite() && patch.k_mpa_mm > 0.0) {
            return Err(format!(
                "contact '{}': k must be positive and finite (got {})",
                patch.id, patch.k_mpa_mm
            ));
        }
        if !(patch.gap_mm.is_finite() && patch.gap_mm >= 0.0) {
            return Err(format!(
                "contact '{}': gap must be finite and ≥ 0 (got {})",
                patch.id, patch.gap_mm
            ));
        }
        for facet in &patch.facets {
            let normal = normalize_unit(facet.normal)?;
            let p: [V3; 6] = facet.nodes.map(|idx| mesh.nodes[idx as usize]);
            for &(s, phi, dn) in &ref_pts {
                let mut t1 = [0.0f64; 3];
                let mut t2 = [0.0f64; 3];
                for k in 0..6 {
                    for c in 0..3 {
                        t1[c] += dn[k][0] * p[k][c];
                        t2[c] += dn[k][1] * p[k][c];
                    }
                }
                let cx = t1[1] * t2[2] - t1[2] * t2[1];
                let cy = t1[2] * t2[0] - t1[0] * t2[2];
                let cz = t1[0] * t2[1] - t1[1] * t2[0];
                let jac = (cx * cx + cy * cy + cz * cz).sqrt();
                let da_mm2 = s * jac;
                patch_areas[pi] += da_mm2;
                samples.push(QuadSample {
                    patch_idx: pi,
                    nodes: facet.nodes,
                    normal,
                    phi,
                    da_mm2,
                    k_mpa_mm: patch.k_mpa_mm,
                    gap_mm: patch.gap_mm,
                });
            }
        }
    }
    Ok((samples, patch_areas))
}

#[inline]
fn sample_penetration_mm(qs: &QuadSample, u: &[f64]) -> f64 {
    let mut un = 0.0f64;
    for a in 0..6 {
        let base = 3 * (qs.nodes[a] as usize);
        let dot_n =
            u[base] * qs.normal[0] + u[base + 1] * qs.normal[1] + u[base + 2] * qs.normal[2];
        un += qs.phi[a] * dot_n;
    }
    un - qs.gap_mm
}

fn total_potential_energy(
    k_el: &Bsr,
    force: &[f64],
    samples: &[QuadSample],
    u: &[f64],
    ku_buf: &mut [f64],
) -> f64 {
    k_el.mul(u, ku_buf);
    let mut u_ku = 0.0f64;
    let mut f_u = 0.0f64;
    for i in 0..u.len() {
        u_ku += u[i] * ku_buf[i];
        f_u += force[i] * u[i];
    }
    let mut e_c = 0.0f64;
    for qs in samples {
        let pen = sample_penetration_mm(qs, u);
        if pen > 0.0 {
            e_c += 0.5 * qs.da_mm2 * qs.k_mpa_mm * pen * pen;
        }
    }
    0.5 * u_ku - f_u + e_c
}

/// Add the exact Newton normal contact tangent `∑_{q ∈ 𝒜} w_q k_q (φ ⊗ n)(φ ⊗ n)ᵀ` and
/// clearance load `∑_{q ∈ 𝒜} w_q k_q g_0 (φ ⊗ n)` onto `(a, b)` prior to Dirichlet elimination.
fn assemble_active_contact(a: &mut Bsr, b: &mut [f64], samples: &[QuadSample], active: &[bool]) {
    for (qs, &is_act) in samples.iter().zip(active.iter()) {
        if !is_act {
            continue;
        }
        let wk = qs.da_mm2 * qs.k_mpa_mm;
        let [nx, ny, nz] = qs.normal;
        let nn_outer = [
            nx * nx,
            nx * ny,
            nx * nz,
            ny * nx,
            ny * ny,
            ny * nz,
            nz * nx,
            nz * ny,
            nz * nz,
        ];
        if qs.gap_mm != 0.0 {
            let wkg = wk * qs.gap_mm;
            for ia in 0..6 {
                let s = wkg * qs.phi[ia];
                let base = 3 * (qs.nodes[ia] as usize);
                b[base] += s * nx;
                b[base + 1] += s * ny;
                b[base + 2] += s * nz;
            }
        }
        for ia in 0..6 {
            let row = qs.nodes[ia] as usize;
            let pa = qs.phi[ia];
            for ib in 0..6 {
                let w_ab = wk * pa * qs.phi[ib];
                let blk = nn_outer.map(|v| w_ab * v);
                a.add(row, qs.nodes[ib], &blk);
            }
        }
    }
}

/// Solve a 3D P2 linear/thermo-elastic problem subject to unilateral Signorini contact
/// patches via Semi-Smooth Newton (SSN) active-set iteration.
pub fn solve_unilateral_contact(
    mesh: &P2Mesh,
    material: &Material,
    fixed: &[u8],
    force: &[f64],
    patches: &[ContactPatch],
    opts: SolverOptions,
) -> Result<ContactSolveOutput, String> {
    configure_host_parallelism();
    let nn = mesh.nodes.len();
    let ndofs = mesh.ndofs();
    if force.len() != ndofs {
        return Err(format!(
            "contact force length {} does not match mesh DOFs {}",
            force.len(),
            ndofs
        ));
    }
    if fixed.len() != nn {
        return Err(format!(
            "contact fixed mask length {} does not match mesh nodes {}",
            fixed.len(),
            nn
        ));
    }

    let (samples, patch_areas) = build_quad_samples(mesh, patches)?;
    let k_el = assemble(mesh, material);

    let free_dofs: usize = (0..nn).map(|i| 3 - fixed[i].count_ones() as usize).sum();

    // Initial active set `𝒜_0`: engage flush (`gap == 0`) seats first so bodies whose normal
    // rigid-body mode rests purely on a unilateral seat start with an SPD tangent operator.
    // If every contact has a positive initial clearance `gap > 0` AND Dirichlet supports leave
    // an unrestrained rigid-body mode, fall back to engaging all contact quadrature points on
    // iteration 0 so Trial 0 stays well-posed.
    let mut active: Vec<bool> = samples.iter().map(|s| s.gap_mm <= 1.0e-12).collect();
    let has_full_dirichlet_anchor = {
        let mut mask_or = 0u8;
        for &m in fixed {
            mask_or |= m;
        }
        mask_or == 0b111
    };
    if !active.iter().any(|&a| a) && !has_full_dirichlet_anchor {
        active.fill(true);
    }

    let mut u = vec![0.0f64; ndofs];
    let mut ku_buf = vec![0.0f64; ndofs];
    let mut total_pcg_iters = 0usize;
    let mut last_rel_residual = 0.0f64;
    let mut last_gpu_device: Option<String> = None;
    let mut active_set_iters = 0usize;
    let mut seen_hashes: HashSet<u64> = HashSet::new();

    for ssn_iter in 1..=MAX_CONTACT_SSN_ITERS {
        active_set_iters = ssn_iter;

        let hash = {
            let mut h = 0xcbf29ce484222325u64;
            for &bit in &active {
                h ^= u64::from(bit);
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        };
        let revisited = !seen_hashes.insert(hash);

        let mut a_tan = k_el.clone();
        let mut b_rhs = force.to_vec();
        assemble_active_contact(&mut a_tan, &mut b_rhs, &samples, &active);
        apply_dirichlet(&mut a_tan, fixed);
        for i in 0..nn {
            for c in 0..3 {
                if is_fixed(fixed, i, c) {
                    b_rhs[3 * i + c] = 0.0;
                    u[3 * i + c] = 0.0;
                }
            }
        }

        let pre = TwoLevel::new(&a_tan, fixed, mesh.nverts, &mesh.tets, &mesh.mid_edge).map_err(
            |e| {
                if !active.iter().any(|&act| act) {
                    format!(
                        "unilateral contact completely separated leaving an unrestrained rigid-body mode ({e}); add a clamping preload or support in the liftoff direction"
                    )
                } else {
                    e
                }
            },
        )?;
        let (u_trial, run) = pcg_solve(&a_tan, &pre, &b_rhs, &u, opts).map_err(|e| {
            if !active.iter().any(|&act| act) {
                format!(
                    "unilateral contact completely separated leaving an unrestrained rigid-body mode ({e}); add a clamping preload or support in the liftoff direction"
                )
            } else {
                e
            }
        })?;
        total_pcg_iters += run.iterations;
        last_rel_residual = run.rel_residual;
        if run.gpu_device.is_some() {
            last_gpu_device = run.gpu_device;
        }

        // If a near-grazing active set ever cycles, perform an Armijo backtracking step on
        // the convex C¹ total potential `Π(u)` so monotone descent is guaranteed.
        let u_next = if revisited && ssn_iter > 2 {
            let pi_prev = total_potential_energy(&k_el, force, &samples, &u, &mut ku_buf);
            let mut alpha = 0.5f64;
            let mut cand = u_trial.clone();
            for _ in 0..8 {
                for k in 0..ndofs {
                    cand[k] = (1.0 - alpha) * u[k] + alpha * u_trial[k];
                }
                let pi_cand = total_potential_energy(&k_el, force, &samples, &cand, &mut ku_buf);
                if pi_cand <= pi_prev + 1.0e-12 * pi_prev.abs().max(1.0) {
                    break;
                }
                alpha *= 0.5;
            }
            cand
        } else {
            u_trial
        };

        let mut du_norm2 = 0.0f64;
        let mut u_norm2 = 0.0f64;
        for k in 0..ndofs {
            let d = u_next[k] - u[k];
            du_norm2 += d * d;
            u_norm2 += u_next[k] * u_next[k];
        }
        u = u_next;

        let next_active: Vec<bool> = samples
            .iter()
            .map(|qs| sample_penetration_mm(qs, &u) > ACTIVE_PENETRATION_TOL_MM)
            .collect();

        if next_active == active {
            break;
        }
        active = next_active;
        if ssn_iter > 1 && du_norm2.sqrt() <= 1.0e-10 * u_norm2.sqrt().max(1.0e-12) {
            break;
        }
    }

    // Recover per-patch ContactReport metrics and nodal contact pressure (MPa).
    let mut reports: Vec<ContactReport> = patches
        .iter()
        .enumerate()
        .map(|(pi, p)| ContactReport {
            id: p.id.clone(),
            region: p.region.clone(),
            total_area_mm2: patch_areas[pi],
            active_set_iters,
            ..Default::default()
        })
        .collect();

    for qs in &samples {
        let rep = &mut reports[qs.patch_idx];
        let pen = sample_penetration_mm(qs, &u);
        if pen > ACTIVE_PENETRATION_TOL_MM {
            let p_mpa = qs.k_mpa_mm * pen;
            rep.active_area_mm2 += qs.da_mm2;
            rep.contact_force_n += qs.da_mm2 * p_mpa;
            if p_mpa > rep.max_contact_pressure_mpa {
                rep.max_contact_pressure_mpa = p_mpa;
            }
            if pen > rep.max_penetration_mm {
                rep.max_penetration_mm = pen;
            }
        } else {
            let liftoff = (-pen).max(0.0);
            if liftoff > rep.max_liftoff_mm {
                rep.max_liftoff_mm = liftoff;
            }
        }
    }

    // Evaluate Lagrange P2 facet nodes directly as well so outer heel/toe edge maxima are
    // captured without interior Gauss-point under-sampling, and populate `nodal_pressure_mpa`.
    let mut nodal_pressure_mpa = vec![0.0f64; nn];
    for (pi, patch) in patches.iter().enumerate() {
        let rep = &mut reports[pi];
        let mut node_n_sum: std::collections::HashMap<usize, ([f64; 3], f64)> =
            std::collections::HashMap::new();
        for facet in &patch.facets {
            let n_hat = normalize_unit(facet.normal)?;
            let p0 = mesh.nodes[facet.nodes[0] as usize];
            let p1 = mesh.nodes[facet.nodes[1] as usize];
            let p2 = mesh.nodes[facet.nodes[2] as usize];
            let ux = p1[0] - p0[0];
            let uy = p1[1] - p0[1];
            let uz = p1[2] - p0[2];
            let vx = p2[0] - p0[0];
            let vy = p2[1] - p0[1];
            let vz = p2[2] - p0[2];
            let cx = uy * vz - uz * vy;
            let cy = uz * vx - ux * vz;
            let cz = ux * vy - uy * vx;
            let w_area = 0.5 * (cx * cx + cy * cy + cz * cz).sqrt().max(1.0e-18);
            for &nid in &facet.nodes {
                let entry = node_n_sum.entry(nid as usize).or_insert(([0.0; 3], 0.0));
                entry.0[0] += w_area * n_hat[0];
                entry.0[1] += w_area * n_hat[1];
                entry.0[2] += w_area * n_hat[2];
                entry.1 += w_area;
            }
        }
        for (nid, (nsum, wsum)) in node_n_sum {
            if wsum <= 0.0 {
                continue;
            }
            let n_avg = normalize_unit([nsum[0] / wsum, nsum[1] / wsum, nsum[2] / wsum])?;
            let un = u[3 * nid] * n_avg[0] + u[3 * nid + 1] * n_avg[1] + u[3 * nid + 2] * n_avg[2];
            let pen = un - patch.gap_mm;
            if pen > ACTIVE_PENETRATION_TOL_MM {
                let p_mpa = patch.k_mpa_mm * pen;
                if p_mpa > nodal_pressure_mpa[nid] {
                    nodal_pressure_mpa[nid] = p_mpa;
                }
                if p_mpa > rep.max_contact_pressure_mpa {
                    rep.max_contact_pressure_mpa = p_mpa;
                }
                if pen > rep.max_penetration_mm {
                    rep.max_penetration_mm = pen;
                }
            } else {
                let liftoff = (-pen).max(0.0);
                if liftoff > rep.max_liftoff_mm {
                    rep.max_liftoff_mm = liftoff;
                }
            }
        }
        rep.contact_area_ratio = if rep.total_area_mm2 > 0.0 {
            (rep.active_area_mm2 / rep.total_area_mm2).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    let solve_report = SolveReport {
        dofs: ndofs,
        free_dofs,
        coarse_dofs: mesh.nverts * 3,
        iterations: total_pcg_iters,
        rel_residual: last_rel_residual,
        converged: true,
        gpu_device: last_gpu_device,
        gpu_gflops: None,
        gpu_bw_gbps: None,
    };

    Ok(ContactSolveOutput {
        u,
        solve_report,
        reports,
        nodal_pressure_mpa,
    })
}

/// Assemble the converged active-set normal contact stiffness onto an existing [`Bsr`]
/// matrix `a` at displacement state `u`, so discrete adjoint solves `(K_el + K_c(𝒜*)) λ = rhs`
/// differentiate cleanly through smooth active contact patches (`VISION.md §5.2 Pillar III/IV`).
pub fn add_converged_contact_tangent(
    mesh: &P2Mesh,
    patches: &[ContactPatch],
    u: &[f64],
    a: &mut Bsr,
) -> Result<(), String> {
    if patches.is_empty() {
        return Ok(());
    }
    let (samples, _) = build_quad_samples(mesh, patches)?;
    let active: Vec<bool> = samples
        .iter()
        .map(|qs| sample_penetration_mm(qs, u) > ACTIVE_PENETRATION_TOL_MM)
        .collect();
    let mut dummy_rhs = vec![0.0f64; mesh.ndofs()];
    assemble_active_contact(a, &mut dummy_rhs, &samples, &active);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2::face_nodes;

    /// Structured brick subdivided into `nx × ny × nz × 6` Kuhn tetrahedra on `[0, lx] × [0, ly] × [0, lz]`.
    fn structured_box_p2(
        lx: f64,
        ly: f64,
        lz: f64,
        nx: usize,
        ny: usize,
        nz: usize,
    ) -> (P2Mesh, Vec<[u32; 6]>, Vec<[u32; 6]>) {
        let mut verts = Vec::new();
        let vid =
            |i: usize, j: usize, k: usize| -> u32 { ((k * (ny + 1) + j) * (nx + 1) + i) as u32 };
        for k in 0..=nz {
            for j in 0..=ny {
                for i in 0..=nx {
                    verts.push([
                        lx * i as f64 / nx as f64,
                        ly * j as f64 / ny as f64,
                        lz * k as f64 / nz as f64,
                    ]);
                }
            }
        }
        let mut tets = Vec::new();
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let v000 = vid(i, j, k);
                    let v100 = vid(i + 1, j, k);
                    let v010 = vid(i, j + 1, k);
                    let v110 = vid(i + 1, j + 1, k);
                    let v001 = vid(i, j, k + 1);
                    let v101 = vid(i + 1, j, k + 1);
                    let v011 = vid(i, j + 1, k + 1);
                    let v111 = vid(i + 1, j + 1, k + 1);
                    tets.push([v000, v100, v110, v111]);
                    tets.push([v000, v110, v010, v111]);
                    tets.push([v000, v010, v011, v111]);
                    tets.push([v000, v011, v001, v111]);
                    tets.push([v000, v001, v101, v111]);
                    tets.push([v000, v101, v100, v111]);
                }
            }
        }
        let mesh = P2Mesh::from_linear(&verts, &tets);
        let mut bottom_facets = Vec::new();
        let mut top_facets = Vec::new();
        const TET_FACES: [[usize; 3]; 4] = [[1, 2, 3], [0, 3, 2], [0, 1, 3], [0, 2, 1]];
        for tet in &mesh.tets {
            for lf in TET_FACES {
                let c0 = mesh.nodes[tet[lf[0]] as usize];
                let c1 = mesh.nodes[tet[lf[1]] as usize];
                let c2 = mesh.nodes[tet[lf[2]] as usize];
                if c0[2].abs() < 1e-12 && c1[2].abs() < 1e-12 && c2[2].abs() < 1e-12 {
                    bottom_facets.push(face_nodes(tet, lf));
                }
                if (c0[2] - lz).abs() < 1e-12
                    && (c1[2] - lz).abs() < 1e-12
                    && (c2[2] - lz).abs() < 1e-12
                {
                    top_facets.push(face_nodes(tet, lf));
                }
            }
        }
        (mesh, bottom_facets, top_facets)
    }

    /// Assemble exact uniform surface traction `[tx, ty, tz]` (MPa) across `facets`.
    fn apply_uniform_surface_traction(
        mesh: &P2Mesh,
        facets: &[[u32; 6]],
        traction_mpa: V3,
        force: &mut [f64],
    ) {
        for f in facets {
            let (_m, l) = crate::thermal::element_robin(mesh, f);
            for a in 0..6 {
                let base = 3 * (f[a] as usize);
                force[base] += l[a] * traction_mpa[0];
                force[base + 1] += l[a] * traction_mpa[1];
                force[base + 2] += l[a] * traction_mpa[2];
            }
        }
    }

    #[test]
    fn uniform_compression_and_clearance_gap_match_analytical_winkler_equilibrium() {
        // 10 × 10 × 10 mm block (`A = 100 mm²`), E = 200,000 MPa, ν = 0.0 (uniaxial exact!).
        // Restrained ONLY tangentially (`u_x = 0` on x = 0, `u_y = 0` on y = 0) with ZERO
        // vertical Dirichlet support — `z` equilibrium is carried 100% by unilateral contact!
        let (mesh, bottom_facets, top_facets) = structured_box_p2(10.0, 10.0, 10.0, 2, 2, 2);
        let mat = Material::iso(vec![200_000.0; mesh.tets.len()], 0.0);
        let mut fixed = vec![0u8; mesh.nodes.len()];
        for (i, p) in mesh.nodes.iter().enumerate() {
            if p[0].abs() < 1e-12 {
                fixed[i] |= 1;
            }
            if p[1].abs() < 1e-12 {
                fixed[i] |= 2;
            }
        }
        // Apply uniform downward traction `-10 MPa` on top (`F_z = -1000 N`).
        let mut force = vec![0.0f64; mesh.ndofs()];
        apply_uniform_surface_traction(&mesh, &top_facets, [0.0, 0.0, -10.0], &mut force);

        // 1. Flush seat (`gap = 0`, `k = 5000 MPa/mm`): exact `p_n = 10 MPa`, `δ_n = 0.002 mm`.
        let patch_flush = ContactPatch {
            id: "seat".into(),
            region: "bottom".into(),
            k_mpa_mm: 5_000.0,
            gap_mm: 0.0,
            facets: bottom_facets
                .iter()
                .map(|&nodes| ContactFacet {
                    nodes,
                    normal: [0.0, 0.0, -1.0],
                })
                .collect(),
        };
        let out0 = solve_unilateral_contact(
            &mesh,
            &mat,
            &fixed,
            &force,
            &[patch_flush],
            SolverOptions::default(),
        )
        .expect("flush contact solve");
        let r0 = &out0.reports[0];
        assert!((r0.total_area_mm2 - 100.0).abs() < 1e-9);
        assert!((r0.contact_area_ratio - 1.0).abs() < 1e-9);
        assert!(
            (r0.contact_force_n - 1_000.0).abs() < 1e-5,
            "force={}",
            r0.contact_force_n
        );
        assert!(
            (r0.max_contact_pressure_mpa - 10.0).abs() < 1e-5,
            "p_max={}",
            r0.max_contact_pressure_mpa
        );
        assert!(
            (r0.max_penetration_mm - 0.002).abs() < 1e-7,
            "pen={}",
            r0.max_penetration_mm
        );
        assert!(r0.max_liftoff_mm < 1e-12);
        assert_eq!(r0.active_set_iters, 1);

        // 2. Initial clearance `gap = 0.05 mm`: block travels `-0.052 mm` downward at `z = 0`,
        //    and contact pressure / force remain `10.0 MPa` and `1000.0 N`.
        let patch_gap = ContactPatch {
            id: "seat_gap".into(),
            region: "bottom".into(),
            k_mpa_mm: 5_000.0,
            gap_mm: 0.05,
            facets: bottom_facets
                .iter()
                .map(|&nodes| ContactFacet {
                    nodes,
                    normal: [0.0, 0.0, -1.0],
                })
                .collect(),
        };
        let out_gap = solve_unilateral_contact(
            &mesh,
            &mat,
            &fixed,
            &force,
            &[patch_gap],
            SolverOptions::default(),
        )
        .expect("gapped contact solve");
        let rg = &out_gap.reports[0];
        assert!((rg.contact_force_n - 1_000.0).abs() < 1e-5);
        assert!((rg.max_contact_pressure_mpa - 10.0).abs() < 1e-5);
        assert!((rg.max_penetration_mm - 0.002).abs() < 1e-7);
        for (i, p) in mesh.nodes.iter().enumerate() {
            if p[2].abs() < 1e-12 {
                assert!(
                    (out_gap.u[3 * i + 2] - (-0.052)).abs() < 1e-6,
                    "bottom uz={} expected -0.052",
                    out_gap.u[3 * i + 2]
                );
            }
        }
    }

    #[test]
    fn bolted_flange_prying_produces_heel_liftoff_and_prying_force_amplification() {
        // Flange bar `40 × 10 × 6 mm` resting on unilateral seat `z = 0` (`normal = [0, 0, -1]`).
        // Bolt line at `x = 12 mm` (`z = 0` held in `x, y, z`, representing stiff bolt clamping
        // the joint at distance `a = 12 mm` from the toe `x = 0` and `b = 28 mm` from the
        // external pull at `x = 40 mm`).
        // Upward external pull `F_ext = +300 N` on `x ∈ [35, 40], z = 6` levers the flange
        // over the toe (`x ≈ 0`):
        // - The flange lifts off cleanly over the heel/tip (`x > 12`, `p_n = 0`, `max_liftoff > 0`).
        // - The toe (`x < 12`) compresses into the rigid bed with prying reaction `Q = contact_force_n > 0`!
        // - By static lever equilibrium about the bolt line (`x = 12`), `Q ≈ F_ext * (b / a_eff) > 0`,
        //   amplifying total bolt tension to `F_bolt = F_ext + Q > F_ext`!
        let (mesh, bottom_facets, top_facets) = structured_box_p2(40.0, 10.0, 6.0, 8, 2, 2);
        let mat = Material::iso(vec![200_000.0; mesh.tets.len()], 0.3);
        let mut fixed = vec![0u8; mesh.nodes.len()];
        for (i, p) in mesh.nodes.iter().enumerate() {
            // Bolt head clamping line across top face at `x = 10 mm`, `z = 6 mm`:
            if (p[0] - 10.0).abs() < 1e-9 && (p[2] - 6.0).abs() < 1e-9 {
                fixed[i] = 0b111;
            }
        }
        // Upward prying traction on top tip strip `x ∈ [35, 40]` (`area = 5 * 10 = 50 mm²`,
        // `+6 MPa` => `F_ext = +300 N` centered at `x = 37.5 mm`).
        let tip_facets: Vec<[u32; 6]> = top_facets
            .iter()
            .copied()
            .filter(|f| {
                f[..3]
                    .iter()
                    .all(|&nid| mesh.nodes[nid as usize][0] >= 35.0 - 1e-9)
            })
            .collect();
        let mut force = vec![0.0f64; mesh.ndofs()];
        apply_uniform_surface_traction(&mesh, &tip_facets, [0.0, 0.0, 6.0], &mut force);

        let patch = ContactPatch {
            id: "mating_bed".into(),
            region: "bottom".into(),
            k_mpa_mm: 20_000.0,
            gap_mm: 0.0,
            facets: bottom_facets
                .iter()
                .map(|&nodes| ContactFacet {
                    nodes,
                    normal: [0.0, 0.0, -1.0],
                })
                .collect(),
        };

        let out = solve_unilateral_contact(
            &mesh,
            &mat,
            &fixed,
            &force,
            &[patch],
            SolverOptions::default(),
        )
        .expect("flange prying contact solve");
        let rep = &out.reports[0];

        // Partial contact (`0 < contact_area_ratio < 0.30`): only the toe `x ∈ [0, 10)` compresses!
        assert!(
            rep.contact_area_ratio > 0.05 && rep.contact_area_ratio < 0.30,
            "expected partial toe contact, got ratio={}",
            rep.contact_area_ratio
        );
        // Lever prying reaction: `F_ext = 300 N` at `x = 37.5 mm` (`b = 27.5 mm` from bolt at `x = 10 mm`,
        // `M = 8250 N·mm`) levers against the compressed toe centroid `x_c ∈ (0, 10)` (`a_eff = 10 - x_c ≈ 6.11 mm`),
        // producing `Q = M / a_eff ≈ 1350 N >> F_ext`!
        assert!(
            rep.contact_force_n > 1_000.0 && rep.contact_force_n < 1_700.0,
            "expected lever prying reaction Q in [1000, 1700] N, got {}",
            rep.contact_force_n
        );
        // Exact vertical force balance: bolt vertical reaction `R_bolt_z` + `F_ext` (300 N) - `Q_contact` == 0!
        // Note `normal = [0, 0, -1]` pushes upward (`+z`) on the flange with force `+rep.contact_force_n`,
        // while upward external pull is `+300 N`, so downward bolt reaction is `-(300 + rep.contact_force_n)`.
        let k_el = assemble(&mesh, &mat);
        let mut ku = vec![0.0f64; mesh.ndofs()];
        k_el.mul(&out.u, &mut ku);
        let mut bolt_rz = 0.0f64;
        for (i, &m) in fixed.iter().enumerate() {
            if m != 0 {
                // Note: if any contact quadrature shares bolt-line nodes `x = 10`, those nodes
                // have `u_z = 0` (`pen = 0`), so zero contact force lands directly on fixed bolt nodes!
                bolt_rz += ku[3 * i + 2] - force[3 * i + 2];
            }
        }
        let vertical_imbalance = bolt_rz + 300.0 + rep.contact_force_n;
        assert!(
            vertical_imbalance.abs() < 1e-5,
            "exact vertical equilibrium violated: R_bolt_z ({bolt_rz}) + F_ext (300) + Q_contact ({}) = {vertical_imbalance}",
            rep.contact_force_n
        );
        // Euler–Bernoulli bending + bolt-line slope rotation on E=200 GPa, t=6 mm, b=27.5 mm gives ~0.095 mm tip liftoff:
        assert!(
            rep.max_liftoff_mm > 0.06 && rep.max_liftoff_mm < 0.16,
            "expected measurable flange tip liftoff in [0.06, 0.16] mm, got {}",
            rep.max_liftoff_mm
        );
        // Pointwise Signorini complementarity: zero contact pressure on lifted-off nodes `x > 12`.
        for (i, p) in mesh.nodes.iter().enumerate() {
            if p[2].abs() < 1e-9 && p[0] > 12.0 {
                assert_eq!(
                    out.nodal_pressure_mpa[i], 0.0,
                    "lifted-off node at x={} must have zero contact pressure",
                    p[0]
                );
                assert!(
                    out.u[3 * i + 2] > 0.0,
                    "node at x={} should lift upward (+z), got uz={}",
                    p[0],
                    out.u[3 * i + 2]
                );
            }
        }
        assert!(
            rep.active_set_iters >= 2 && rep.active_set_iters <= 8,
            "SSN should converge in 2..8 steps, took {}",
            rep.active_set_iters
        );
    }
}
