//! Modal analysis (M8): natural frequencies of the clamped part.
//!
//! The consistent P2 mass matrix comes from `fenics_assembly::assemble_elasticity_mass_p2_3d`
//! (same element family as the stiffness), and the eigen route is
//! `fenics_solver::solve_vibration_modes_with_inner` — consumed, not reimplemented. The inner
//! solve behind each power iteration is the static path's two-level PCG (see `pcg`), built
//! once and reused across the run: Jacobi-only CG stalls on the P2 stiffness at mesh sizes
//! the static path handles routinely (the V13 fine rung, 45k DOF, flattened at a relative
//! residual of 1.1 after its 1000-iteration budget, against ~50 iterations for the two
//! level). There is no load input: the supports define the constrained DOFs exactly as in
//! the static path.
//!
//! Units: mm, N, s. Density enters the mass matrix as N·s²/mm⁴ — a document's g/cm³ value
//! times 1e-9 (checked in the `modal.rs` tests: 1 g = 1e-6 N·s²/mm, 1 cm³ = 1e3 mm³).

use crate::{assemble, pcg, Material, P2Mesh};
use faer::sparse::SparseColMat;
use serde::Serialize;

/// Modal run options.
#[derive(Clone, Debug)]
pub struct ModeOptions {
    pub count: usize,
    pub max_power_iters: usize,
    pub tol: f64,
}

impl Default for ModeOptions {
    fn default() -> Self {
        ModeOptions {
            count: 3,
            max_power_iters: 50,
            tol: 1e-7,
        }
    }
}

/// Frequencies of the first modes. Mode shapes are kept out of the serialisable result: the
/// readouts (CLI, Results lens) show frequencies, and the shapes are not visualised in v0.3.
#[derive(Clone, Debug, Serialize)]
pub struct ModalResult {
    /// Natural frequencies in Hz, ascending.
    pub frequencies: Vec<f64>,
    /// ω² eigenvalues (rad/s)², same order.
    pub eigenvalues: Vec<f64>,
    /// Set by the caller: `solve_modes` runs clock-free because std's clocks panic on
    /// wasm32 (core measures the call with its `Clock`; tests leave it 0).
    pub solve_ms: f64,
}

/// Solve the first `opts.count` modes of a clamped P2 mesh.
///
/// `fixed` is the per-node bit mask the static path builds (bit 0/1/2 = x/y/z held). The
/// stiffness matrix is the path `assemble` already builds, Dirichlet-folded the same way the
/// static solver folds it before factoring, expanded to scalar triplets.
pub fn solve_modes(
    mesh: &P2Mesh,
    young: &[f64],
    poisson: f64,
    density_modal: f64,
    fixed: &[u8],
    opts: &ModeOptions,
) -> Result<ModalResult, String> {
    faer::set_global_parallelism(faer::Parallelism::None);
    let mut bsr = assemble(mesh, &Material {
        young: young.to_vec(),
        poisson,
    });
    pcg::apply_dirichlet(&mut bsr, fixed);
    let n_total = mesh.nodes.len() * 3;
    let mut triplets = Vec::with_capacity(bsr.nnz_blocks() * 9);
    for i in 0..bsr.n {
        for k in bsr.row_ptr[i] as usize..bsr.row_ptr[i + 1] as usize {
            let blk = &bsr.vals[k];
            let row = 3 * i;
            let col = 3 * bsr.cols[k] as usize;
            for a in 0..3 {
                for b in 0..3 {
                    triplets.push((row + a, col + b, blk[a * 3 + b]));
                }
            }
        }
    }
    let k = SparseColMat::<usize, f64>::try_new_from_triplets(
        n_total,
        n_total,
        &triplets,
    )
    .map_err(|e| format!("stiffness matrix: {e}"))?;

    let cells: Vec<[usize; 10]> = mesh.tets.iter().map(|t| t.map(|n| n as usize)).collect();
    let m = fenics_assembly::assemble_elasticity_mass_p2_3d(&mesh.nodes, &cells, density_modal);

    // The constrained DOF list, from the same bit masks the static path applies.
    let fixed_dofs: Vec<usize> = fixed
        .iter()
        .enumerate()
        .flat_map(|(n, &bits)| {
            (0..3)
                .filter(move |&c| bits & (1 << c) != 0)
                .map(move |c| 3 * n + c)
        })
        .collect();

    // One two-level preconditioner serves every inner solve of the run. `pcg_solve` starts
    // from the previous iterate, which inverse iteration has already moved near the mode.
    let pre = pcg::TwoLevel::new(&bsr, fixed, mesh.nverts, &mesh.tets, &mesh.mid_edge)
        .map_err(|e| format!("modal preconditioner: {e}"))?;
    let inner = |b: &[f64], x0: &[f64]| -> Result<Vec<f64>, String> {
        let (w, rep) = pcg::pcg_solve(&bsr, &pre, b, x0, pcg::SolverOptions::default())?;
        if !rep.converged {
            return Err(format!(
                "modal inner solve stalled at a relative residual of {:.2e} after {} iterations",
                rep.rel_residual, rep.iterations
            ));
        }
        Ok(w)
    };

    let modes = fenics_solver::solve_vibration_modes_with_inner(
        &k,
        &m,
        &fixed_dofs,
        opts.count,
        opts.max_power_iters,
        opts.tol,
        &inner,
    )?;
    Ok(ModalResult {
        frequencies: modes.iter().map(|m| m.frequency_hz).collect(),
        eigenvalues: modes.iter().map(|m| m.eigenvalue).collect(),
        solve_ms: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::P2Mesh;

    /// One tetrahedron (corners (0,0,0), (1,0,0), (0,1,0), (0,0,1)) with the four corners
    /// clamped — only the six midside nodes stay free. The absolute frequency is not the
    /// anchor; the scaling laws are: f ∝ √E and f ∝ 1/√ρ.
    fn one_tet() -> P2Mesh {
        let verts: Vec<[f64; 3]> = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        let tets: Vec<[u32; 4]> = vec![[0, 1, 2, 3]];
        P2Mesh::from_linear(&verts, &tets)
    }

    fn fixed_corners(mesh: &P2Mesh) -> Vec<u8> {
        let mut fixed = vec![0u8; mesh.nodes.len()];
        for i in 0..mesh.nverts {
            fixed[i] = 0b111;
        }
        fixed
    }

    fn first_frequency(e: f64, rho: f64) -> f64 {
        let mesh = one_tet();
        let fixed = fixed_corners(&mesh);
        let young = vec![e; mesh.tets.len()];
        let opts = ModeOptions {
            count: 2,
            ..ModeOptions::default()
        };
        let r = solve_modes(&mesh, &young, 0.3, rho, &fixed, &opts);
        r.expect("modal run").frequencies[0]
    }

    #[test]
    fn frequency_scales_with_sqrt_density_and_stiffness() {
        let base = first_frequency(200_000.0, 7.85e-9);
        assert!(base > 0.0 && base.is_finite());
        // Four times the stiffness: √4 = 2.
        let stiffer = first_frequency(800_000.0, 7.85e-9);
        assert!(
            (stiffer / base - 2.0).abs() < 1e-6,
            "f(4E)/f(E) = {}, expected 2",
            stiffer / base
        );
        // Sixteen times the density: 1/√16 = 1/4.
        let denser = first_frequency(200_000.0, 16.0 * 7.85e-9);
        assert!(
            (base / denser - 4.0).abs() < 1e-6,
            "f(16ρ)/f(ρ) = {}, expected 1/4",
            denser / base
        );
    }

    #[test]
    fn fully_clamped_mesh_has_no_modes() {
        let mesh = one_tet();
        let fixed = vec![0b111u8; mesh.nodes.len()];
        let young = vec![200_000.0; mesh.tets.len()];
        let opts = ModeOptions::default();
        let err = solve_modes(&mesh, &young, 0.3, 7.85e-9, &fixed, &opts)
            .expect_err("no free DOFs");
        assert!(err.contains("No free degrees of freedom"), "{err}");
    }
}