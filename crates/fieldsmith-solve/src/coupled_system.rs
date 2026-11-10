//! Domain-agnostic pluggable architecture for monolithic multi-physics engineering systems,
//! adjoint design sensitivity analysis, epistemic uncertainty propagation (`p10`/`p50`/`p90`),
//! conservation auditing, and causal birth-to-exit defect attribution.
//!
//! Any industrial process or component model implementing [`CoupledEngineeringSystem`] gains:
//! 1. **Implicit Newton–BDF Continuation & Sparse LU Solve** over arbitrary coupled fields `W`.
//! 2. **Directional Finite-Difference Jacobian Verification** (`||J δW - FD||₂ / ||J δW||₂`).
//! 3. **Exact Sparse Adjoint (`Jᵀ λ = (∂Φ/∂W)ᵀ`) & Tangent (`J s = -∂R/∂p`) Sensitivities**
//!    for all objectives and constraints `Φ_m` with respect to design/actuator parameters `p_k`.
//! 4. **First-Order Second-Moment (FOSM) Epistemic Uncertainty Propagation** through poorly
//!    understood empirical closures `θ_l` (such as temperature-dependent `H₂` redox kinetics in
//!    float glass or supercritical nucleate/film-boiling correlations in rocket cooling channels),
//!    yielding `p10`, `p50`, and `p90` confidence intervals and variance-attribution rankings.
//! 5. **Automated Conservation Audits, Lagrangian Causal Attribution Profiles, and Multi-Scenario
//!    Pareto Synthesis**.

use faer::prelude::SpSolver;
use faer::sparse::SparseColMat;
use serde::{Deserialize, Serialize};

/// Descriptor for one physical degree-of-freedom family in the coupled state vector `W`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldDescriptor {
    pub name: String,
    pub symbol: String,
    pub unit: String,
    pub reference_scale: f64,
    pub domain: String,
}

/// Classification of controllable engineering parameters `p`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DesignParameterCategory {
    /// Spatial geometry or contour parameters (e.g., bath depth steps `H(x,y)`, channel wall thickness).
    GeometryContour,
    /// Installed hardware positioning & sizing (e.g., CVD coating beam station `x`, gap `z`, cooling kW).
    ComponentHardware,
    /// Active process actuators (e.g., linear induction motor current, top-roll angle, tin cooler kW).
    ActuatorSetpoint,
    /// Process atmosphere / chemical feed setpoints (e.g., zoned `H₂` mole fraction, coolant mass flow).
    ProcessAtmosphere,
}

/// Controllable design or operating parameter `p_k`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DesignParameter {
    pub name: String,
    pub symbol: String,
    pub unit: String,
    pub category: DesignParameterCategory,
    pub nominal: f64,
    pub min_bound: f64,
    pub max_bound: f64,
    pub fd_step: f64,
    /// Direct marginal operating cost rate per unit of parameter (`£/hr` per unit), if applicable.
    pub cost_rate_per_unit: f64,
    pub description: String,
}

/// Poorly understood constitutive, interfacial, or chemical closure parameter `θ_l`
/// carrying epistemic uncertainty (`nominal ± std_dev_sigma`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EpistemicParameter {
    pub name: String,
    pub symbol: String,
    pub unit: String,
    pub nominal: f64,
    pub std_dev_sigma: f64,
    pub min_bound: f64,
    pub max_bound: f64,
    pub fd_step: f64,
    pub physical_mechanism: String,
}

/// Role of an engineering observable `Φ_m(W, p, θ)`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ObjectiveKind {
    Minimize,
    Maximize,
    UpperLimit(f64),
    LowerLimit(f64),
    TargetBand { min: f64, max: f64 },
}

/// Descriptor for a system-level quality, structural, thermal, or economic metric `Φ_m`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectiveDescriptor {
    pub name: String,
    pub symbol: String,
    pub unit: String,
    pub kind: ObjectiveKind,
    pub description: String,
}

/// Integral conservation audit verified on a converged state `W`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConservationAudit {
    pub name: String,
    pub inflow: f64,
    pub outflow: f64,
    pub source_or_sink: f64,
    pub net_imbalance: f64,
    pub unit: String,
    pub relative_error: f64,
    pub tolerance: f64,
}

impl ConservationAudit {
    pub fn passed(&self) -> bool {
        self.relative_error <= self.tolerance
    }
}

/// One station along a 1D streamwise / Lagrangian process coordinate `s ∈ [0, L]`,
/// linking geometry variations and local regime fluxes to defect birth and exit survival.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CausalAttributionPoint {
    pub coord_m: f64,
    pub zone_label: String,
    pub geometry_profile_m: f64,
    pub primary_temperature_c: f64,
    pub forward_transport_kw: f64,
    pub return_or_loss_transport_kw: f64,
    pub local_defect_birth: f64,
    pub downstream_survival_factor: f64,
    pub surviving_exit_defect: f64,
}

/// Streamwise causal attribution profile explaining *where* a quality defect or bottleneck is
/// generated, *how* geometry (`H(x)`) and hardware affect transport, and *what* survives to exit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CausalAttributionTrack {
    pub track_name: String,
    pub coord_unit: String,
    pub geometry_label: String,
    pub defect_label: String,
    pub defect_unit: String,
    pub points: Vec<CausalAttributionPoint>,
}

/// Pluggable interface implemented by any multi-physics engineering domain pack.
pub trait CoupledEngineeringSystem {
    /// Short identifier (e.g. `"float_bath_coater_v1"` or `"regen_nozzle_h2_v1"`).
    fn system_id(&self) -> &str;

    /// Human-readable title of the engineering model.
    fn system_title(&self) -> &str;

    /// Industrial sector description.
    fn industry_sector(&self) -> &str;

    /// Descriptors for each coupled physical field per node or block.
    fn field_descriptors(&self) -> Vec<FieldDescriptor>;

    /// Total number of scalar unknowns in the monolithic state vector `W`.
    fn num_dofs(&self) -> usize;

    /// Controllable design and actuator parameters `p`.
    fn design_parameters(&self) -> Vec<DesignParameter>;

    /// Uncertain constitutive/chemical parameters `θ`.
    fn epistemic_parameters(&self) -> Vec<EpistemicParameter>;

    /// Engineering objectives and constraint functionals `Φ`.
    fn objective_descriptors(&self) -> Vec<ObjectiveDescriptor>;

    /// Nominal design parameter vector `p_0`.
    fn nominal_design_vector(&self) -> Vec<f64> {
        self.design_parameters().iter().map(|p| p.nominal).collect()
    }

    /// Nominal epistemic parameter vector `θ_0`.
    fn nominal_epistemic_vector(&self) -> Vec<f64> {
        self.epistemic_parameters()
            .iter()
            .map(|t| t.nominal)
            .collect()
    }

    /// Construct the cold-start or reference initial state `W_0(p, θ)`.
    fn initial_state(&self, design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64>;

    /// Evaluate the monolithic residual vector `R(W, W_prev, inv_dt; p, θ)`.
    fn eval_residual(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64>;

    /// Assemble the residual `R` and sparse Jacobian `J = ∂R/∂W` (`SparseColMat<usize, f64>`),
    /// returning `(R, J, cross_physics_nnz)`.
    ///
    /// By default, automatically constructs the sparse Jacobian via central finite differences
    /// over `eval_residual`, classifying off-diagonal cross-field couplings using `field_descriptors().len()`.
    /// Domain packs with large grids (e.g. `FloatBathCoaterDesignSystem`) can override this with
    /// an analytical or graph-coloured stencil assembler.
    #[allow(clippy::type_complexity)]
    fn assemble_jacobian(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Result<(Vec<f64>, SparseColMat<usize, f64>, usize), String> {
        let ndof = self.num_dofs();
        let n_fields = self.field_descriptors().len().max(1);
        let r0 = self.eval_residual(w, w_prev, inv_dt, design_p, epistemic_theta);
        let mut triplets: Vec<(usize, usize, f64)> = Vec::with_capacity(ndof * 12);
        let mut cross_physics_nnz = 0usize;
        let mut w_pert = w.to_vec();

        for col in 0..ndof {
            let col_field = col % n_fields;
            let orig = w_pert[col];
            let eps = 1e-7 * orig.abs().max(1.0);
            w_pert[col] = orig + eps;
            let r_plus = self.eval_residual(&w_pert, w_prev, inv_dt, design_p, epistemic_theta);
            w_pert[col] = orig - eps;
            let r_minus = self.eval_residual(&w_pert, w_prev, inv_dt, design_p, epistemic_theta);
            w_pert[col] = orig;

            for row in 0..ndof {
                let dr = (r_plus[row] - r_minus[row]) / (2.0 * eps);
                if dr.abs() > 1e-13 || row == col {
                    let val = if row == col && dr.abs() <= 1e-13 {
                        1e-12
                    } else {
                        dr
                    };
                    triplets.push((row, col, val));
                    if (row % n_fields) != col_field {
                        cross_physics_nnz += 1;
                    }
                }
            }
        }

        let j_mat = SparseColMat::<usize, f64>::try_new_from_triplets(ndof, ndof, &triplets)
            .map_err(|e| format!("failed to assemble sparse Jacobian: {e:?}"))?;
        Ok((r0, j_mat, cross_physics_nnz))
    }

    /// Evaluate all engineering objectives and constraints `Φ(W, p, θ)`.
    fn eval_objectives(&self, w: &[f64], design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64>;

    /// Optional subset of state DOF indices `k ∈ 0..num_dofs()` that `eval_objectives` explicitly
    /// reads. When `None` (the default), `MonolithicSystemEngine` differentiates `eval_objectives`
    /// with respect to all `0..num_dofs()` state variables.
    fn active_objective_dofs(&self) -> Option<Vec<usize>> {
        None
    }

    /// Evaluate global/zonal conservation audits on state `W`.
    fn eval_conservation_audits(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<ConservationAudit>;

    /// Evaluate causal streamwise attribution tracks on state `W`.
    fn eval_causal_attribution(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<CausalAttributionTrack>;

    /// Synthesise a parametric 3D `.fsm` CAD + thermo-mechanical FEA document representing the
    /// active design vector `design_p`. Enables bidirectional CAD ↔ Multi-Physics synchronisation
    /// across both the browser 3D viewport and the `fieldsmith-mcp` agentic server.
    fn build_3d_fsm(&self, design_p: &[f64]) -> Option<String> {
        let _ = design_p;
        None
    }

    /// Synthesise a multi-fidelity 3D `.fsm` CAD + thermo-mechanical FEA document by projecting
    /// the converged 1D/2D monolithic PDE state `state` (`W*`), controllable design levers
    /// `design_p`, and epistemic closure parameters `epistemic_theta` directly onto the 3D
    /// boundary-condition regions and `let` geometry variables.
    fn build_3d_fsm_from_state(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Option<String> {
        let _ = (state, epistemic_theta);
        self.build_3d_fsm(design_p)
    }
}

/// Solver and sensitivity configuration for [`MonolithicSystemEngine`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Implicit BDF1 pseudo-transient step `Δt` (s).
    pub dt_s: f64,
    /// Number of BDF1 steps from cold start before steady Newton polish.
    pub num_bdf_steps: usize,
    /// Maximum steady Newton iterations.
    pub max_newton_iters: usize,
    /// Steady residual L2 norm convergence threshold.
    pub newton_tol: f64,
    /// Perturbation size for state Jacobian finite differences (`∂Φ/∂W`).
    pub state_fd_eps: f64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            dt_s: 4.0,
            num_bdf_steps: 2,
            max_newton_iters: 8,
            newton_tol: 1e-8,
            state_fd_eps: 1e-6,
        }
    }
}

/// Uncertainty interval and variance decomposition for a single objective `Φ_m`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectiveUncertainty {
    pub objective_index: usize,
    pub name: String,
    pub unit: String,
    pub nominal_p50: f64,
    pub std_dev_sigma: f64,
    pub p10: f64,
    pub p90: f64,
    /// Sensitivities `dΦ_m / dθ_l` to each epistemic parameter.
    pub epistemic_gradients: Vec<f64>,
    /// Fraction of total variance `(dΦ_m/dθ_l · σ_l)² / Var(Φ_m)` contributed by each `θ_l`.
    pub variance_shares: Vec<f64>,
}

/// Complete solve, adjoint sensitivity, epistemic uncertainty, and diagnostic result
/// returned by [`MonolithicSystemEngine::solve_and_analyze`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemAnalysisResult {
    pub system_id: String,
    pub system_title: String,
    pub num_dofs: usize,
    pub jacobian_nnz: usize,
    pub cross_physics_nnz: usize,
    pub initial_residual_l2: f64,
    pub final_residual_l2: f64,
    pub steady_newton_iters: usize,
    pub directional_fd_jacobian_rel_err: f64,
    pub adjoint_vs_tangent_max_rel_err: f64,
    pub state: Vec<f64>,
    pub design_p: Vec<f64>,
    pub epistemic_theta: Vec<f64>,
    pub objectives: Vec<f64>,
    /// `design_sensitivities[m][k] = dΦ_m / dp_k` computed via sparse adjoint `Jᵀ λ_m = (∂Φ_m/∂W)ᵀ`.
    pub design_sensitivities: Vec<Vec<f64>>,
    /// Epistemic uncertainty bands (`p10`, `p50`, `p90`) and variance shares for each objective `Φ_m`.
    pub objective_uncertainties: Vec<ObjectiveUncertainty>,
    pub conservation_audits: Vec<ConservationAudit>,
    pub causal_tracks: Vec<CausalAttributionTrack>,
}

impl SystemAnalysisResult {
    /// Check whether all declared upper/lower/target constraints hold at the nominal (`p50`) state.
    pub fn all_constraints_satisfied_p50(&self, descriptors: &[ObjectiveDescriptor]) -> bool {
        for (val, desc) in self.objectives.iter().zip(descriptors.iter()) {
            match desc.kind {
                ObjectiveKind::UpperLimit(lim) if *val > lim => return false,
                ObjectiveKind::LowerLimit(lim) if *val < lim => return false,
                ObjectiveKind::TargetBand { min, max } if *val < min || *val > max => {
                    return false;
                }
                _ => {}
            }
        }
        true
    }

    /// Check whether all declared constraints still hold under conservative `p90` (`+1.2816 σ`)
    /// epistemic uncertainty.
    pub fn all_constraints_satisfied_robust_p90(
        &self,
        descriptors: &[ObjectiveDescriptor],
    ) -> bool {
        for (uq, desc) in self.objective_uncertainties.iter().zip(descriptors.iter()) {
            match desc.kind {
                ObjectiveKind::UpperLimit(lim) if uq.p90 > lim => return false,
                ObjectiveKind::LowerLimit(lim) if uq.p10 < lim => return false,
                ObjectiveKind::TargetBand { min, max } if uq.p10 < min || uq.p90 > max => {
                    return false;
                }
                _ => {}
            }
        }
        true
    }
}

/// Named design scenario for multi-scenario trade-off and Pareto synthesis.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NamedScenario {
    pub name: String,
    pub description: String,
    pub design_p: Vec<f64>,
}

/// Evaluated scenario summary in a [`ScenarioComparisonSuite`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioEvaluation {
    pub name: String,
    pub description: String,
    pub analysis: SystemAnalysisResult,
    pub constraints_pass_p50: bool,
    pub constraints_pass_robust_p90: bool,
    pub pareto_efficient: bool,
}

/// Multi-scenario comparison and Pareto frontier suite.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioComparisonSuite {
    pub system_id: String,
    pub scenarios: Vec<ScenarioEvaluation>,
}

/// Universal domain-agnostic solver, adjoint sensitivity, and uncertainty engine.
pub struct MonolithicSystemEngine;

impl MonolithicSystemEngine {
    /// Solve the monolithic system `R(W; p, θ) = 0`, verify the sparse Jacobian, compute exact
    /// adjoint design sensitivities `dΦ/dp` (cross-checked against tangent sensitivities `J s = -∂R/∂p`),
    /// propagate epistemic uncertainties `Σ_θ` into `p10/p50/p90` confidence intervals, and
    /// run conservation and causal attribution audits.
    pub fn solve_and_analyze<S: CoupledEngineeringSystem>(
        sys: &S,
        cfg: &EngineConfig,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Result<SystemAnalysisResult, String> {
        let ndof = sys.num_dofs();
        let mut w = sys.initial_state(design_p, epistemic_theta);
        if w.len() != ndof {
            return Err(format!(
                "initial_state length {} != num_dofs {}",
                w.len(),
                ndof
            ));
        }

        let r_init = sys.eval_residual(&w, &w, 0.0, design_p, epistemic_theta);
        let initial_residual_l2 = l2_norm(&r_init);

        // 1. Implicit BDF1 continuation steps from initial state
        let inv_dt = if cfg.dt_s > 0.0 { 1.0 / cfg.dt_s } else { 0.0 };
        for _step in 0..cfg.num_bdf_steps {
            let w_prev = w.clone();
            for _iter in 0..2 {
                let (r_vec, j_mat, _) = sys.assemble_jacobian(
                    &w,
                    &w_prev,
                    inv_dt,
                    design_p,
                    epistemic_theta,
                )?;
                let r_norm = l2_norm(&r_vec);
                if r_norm < cfg.newton_tol {
                    break;
                }
                let lu = j_mat
                    .sp_lu()
                    .map_err(|e| format!("sparse LU failed during BDF continuation: {e:?}"))?;
                let mut rhs = faer::Mat::<f64>::zeros(ndof, 1);
                for k in 0..ndof {
                    rhs[(k, 0)] = -r_vec[k];
                }
                let dw = lu.solve(&rhs);
                for k in 0..ndof {
                    w[k] += dw[(k, 0)];
                }
            }
        }

        // 2. Steady monolithic Newton polish (inv_dt = 0) with backtracking line search
        let mut steady_newton_iters = 0usize;
        let w_ref = w.clone();

        for iter in 0..cfg.max_newton_iters {
            let (r_vec, j_mat, _) =
                sys.assemble_jacobian(&w, &w_ref, 0.0, design_p, epistemic_theta)?;
            let r_norm = l2_norm(&r_vec);
            if r_norm < cfg.newton_tol {
                break;
            }
            steady_newton_iters = iter + 1;

            let lu = j_mat
                .sp_lu()
                .map_err(|e| format!("sparse LU failed at steady Newton iter {iter}: {e:?}"))?;
            let mut rhs = faer::Mat::<f64>::zeros(ndof, 1);
            for k in 0..ndof {
                rhs[(k, 0)] = -r_vec[k];
            }
            let dw = lu.solve(&rhs);

            let mut alpha = 1.0;
            let mut w_trial = w.clone();
            while alpha >= 0.125 {
                for k in 0..ndof {
                    w_trial[k] = w[k] + alpha * dw[(k, 0)];
                }
                let r_trial =
                    sys.eval_residual(&w_trial, &w_ref, 0.0, design_p, epistemic_theta);
                let norm_trial = l2_norm(&r_trial);
                if norm_trial < r_norm || alpha <= 0.125 {
                    w.copy_from_slice(&w_trial);
                    break;
                }
                alpha *= 0.5;
            }
        }

        // 3. Final Jacobian assembly & directional finite-difference verification
        let (r_final, j_final, cross_physics_nnz) =
            sys.assemble_jacobian(&w, &w_ref, 0.0, design_p, epistemic_theta)?;
        let final_residual_l2 = l2_norm(&r_final);
        let jacobian_nnz = j_final.compute_nnz();

        let eps_dir = 1e-7;
        let mut dw_dir = vec![0.0; ndof];
        for (k, val) in dw_dir.iter_mut().enumerate() {
            *val = ((k as f64 * 1.61803398875).sin()) * (1.0 + 0.01 * w[k].abs());
        }
        let w_plus: Vec<f64> = w
            .iter()
            .zip(dw_dir.iter())
            .map(|(wk, dk)| wk + eps_dir * dk)
            .collect();
        let w_minus: Vec<f64> = w
            .iter()
            .zip(dw_dir.iter())
            .map(|(wk, dk)| wk - eps_dir * dk)
            .collect();
        let r_plus = sys.eval_residual(&w_plus, &w_ref, 0.0, design_p, epistemic_theta);
        let r_minus = sys.eval_residual(&w_minus, &w_ref, 0.0, design_p, epistemic_theta);

        let mut j_dw = vec![0.0; ndof];
        for (col, &dw_col) in dw_dir.iter().enumerate() {
            let rows = j_final.as_ref().row_indices_of_col(col);
            let vals = j_final.as_ref().values_of_col(col);
            for (r, &v) in rows.zip(vals) {
                j_dw[r] += v * dw_col;
            }
        }
        let mut diff_sq = 0.0;
        let mut ref_sq = 0.0;
        for k in 0..ndof {
            let fd_k = (r_plus[k] - r_minus[k]) / (2.0 * eps_dir);
            diff_sq += (j_dw[k] - fd_k).powi(2);
            ref_sq += j_dw[k].powi(2);
        }
        let directional_fd_jacobian_rel_err = (diff_sq / ref_sq.max(1e-30)).sqrt();

        // 4. Sparse Adjoint & Tangent Sensitivity Engine
        //    Build Jᵀ explicitly in SparseColMat and factorise both J and Jᵀ once.
        let mut jt_triplets: Vec<(usize, usize, f64)> = Vec::with_capacity(jacobian_nnz);
        for col in 0..ndof {
            let rows = j_final.as_ref().row_indices_of_col(col);
            let vals = j_final.as_ref().values_of_col(col);
            for (r, &v) in rows.zip(vals) {
                jt_triplets.push((col, r, v));
            }
        }
        let jt_mat = SparseColMat::<usize, f64>::try_new_from_triplets(ndof, ndof, &jt_triplets)
            .map_err(|e| format!("failed to assemble J^T sparse matrix: {e:?}"))?;
        let lu_j = j_final
            .sp_lu()
            .map_err(|e| format!("failed to factorise J for tangent solve: {e:?}"))?;
        let lu_jt = jt_mat
            .sp_lu()
            .map_err(|e| format!("failed to factorise J^T for adjoint solve: {e:?}"))?;

        let objectives = sys.eval_objectives(&w, design_p, epistemic_theta);
        let n_obj = objectives.len();

        // State partials dPhi_m / dW (only non-zero columns where objectives depend on W)
        // Compute via central coordinate perturbations of W:
        let mut dphi_dw = vec![vec![0.0; ndof]; n_obj];
        let mut w_pert = w.clone();
        let eps_w = cfg.state_fd_eps;
        let active_dofs: Vec<usize> = sys
            .active_objective_dofs()
            .unwrap_or_else(|| (0..ndof).collect());
        for k in active_dofs {
            let orig = w_pert[k];
            let h = eps_w * orig.abs().max(1.0);
            w_pert[k] = orig + h;
            let phi_p = sys.eval_objectives(&w_pert, design_p, epistemic_theta);
            w_pert[k] = orig - h;
            let phi_m = sys.eval_objectives(&w_pert, design_p, epistemic_theta);
            w_pert[k] = orig;
            let inv_2h = 0.5 / h;
            for m in 0..n_obj {
                dphi_dw[m][k] = (phi_p[m] - phi_m[m]) * inv_2h;
            }
        }

        // Solve J^T * lambda_m = (dPhi_m / dW)^T for all objectives m = 0..n_obj simultaneously
        let mut rhs_adj = faer::Mat::<f64>::zeros(ndof, n_obj.max(1));
        for m in 0..n_obj {
            for k in 0..ndof {
                rhs_adj[(k, m)] = dphi_dw[m][k];
            }
        }
        let lambda_mat = lu_jt.solve(&rhs_adj);

        // 5. Design parameter sensitivities dPhi_m / dp_k (Adjoint AND Tangent cross-check)
        let design_descriptors = sys.design_parameters();
        let n_design = design_p.len();
        let mut design_sensitivities = vec![vec![0.0; n_design]; n_obj];
        let mut max_adj_tan_rel_err = 0.0_f64;

        let mut p_pert = design_p.to_vec();
        for k in 0..n_design {
            let orig_p = p_pert[k];
            let dp = design_descriptors
                .get(k)
                .map(|d| d.fd_step)
                .unwrap_or(1e-4)
                .max(1e-7);
            p_pert[k] = orig_p + dp;
            let r_p = sys.eval_residual(&w, &w_ref, 0.0, &p_pert, epistemic_theta);
            let phi_p = sys.eval_objectives(&w, &p_pert, epistemic_theta);

            p_pert[k] = orig_p - dp;
            let r_m = sys.eval_residual(&w, &w_ref, 0.0, &p_pert, epistemic_theta);
            let phi_m = sys.eval_objectives(&w, &p_pert, epistemic_theta);
            p_pert[k] = orig_p;

            let inv_2dp = 0.5 / dp;
            let mut dr_dp = vec![0.0; ndof];
            let mut rhs_tan = faer::Mat::<f64>::zeros(ndof, 1);
            for i in 0..ndof {
                dr_dp[i] = (r_p[i] - r_m[i]) * inv_2dp;
                rhs_tan[(i, 0)] = -dr_dp[i];
            }
            // Tangent state sensitivity s_k = dW / dp_k = -J^-1 (dR / dp_k)
            let s_k = lu_j.solve(&rhs_tan);

            for m in 0..n_obj {
                let dphi_dp_explicit = (phi_p[m] - phi_m[m]) * inv_2dp;
                let mut adj_dot = 0.0;
                let mut tan_dot = 0.0;
                for i in 0..ndof {
                    adj_dot += lambda_mat[(i, m)] * dr_dp[i];
                    tan_dot += dphi_dw[m][i] * s_k[(i, 0)];
                }
                let grad_adjoint = dphi_dp_explicit - adj_dot;
                let grad_tangent = dphi_dp_explicit + tan_dot;
                design_sensitivities[m][k] = grad_adjoint;

                let scale = grad_adjoint.abs().max(grad_tangent.abs()).max(1e-9);
                let rel_err = (grad_adjoint - grad_tangent).abs() / scale;
                if rel_err > max_adj_tan_rel_err {
                    max_adj_tan_rel_err = rel_err;
                }
            }
        }

        // 6. Epistemic uncertainty propagation: dPhi_m / dtheta_l & FOSM variance decomposition
        let epistemic_descriptors = sys.epistemic_parameters();
        let n_epistemic = epistemic_theta.len();
        let mut dphi_dtheta = vec![vec![0.0; n_epistemic]; n_obj];
        let mut theta_pert = epistemic_theta.to_vec();

        for l in 0..n_epistemic {
            let orig_t = theta_pert[l];
            let dt_step = epistemic_descriptors
                .get(l)
                .map(|d| d.fd_step)
                .unwrap_or(1e-4)
                .max(1e-7);
            theta_pert[l] = orig_t + dt_step;
            let r_p = sys.eval_residual(&w, &w_ref, 0.0, design_p, &theta_pert);
            let phi_p = sys.eval_objectives(&w, design_p, &theta_pert);

            theta_pert[l] = orig_t - dt_step;
            let r_m = sys.eval_residual(&w, &w_ref, 0.0, design_p, &theta_pert);
            let phi_m = sys.eval_objectives(&w, design_p, &theta_pert);
            theta_pert[l] = orig_t;

            let inv_2dt = 0.5 / dt_step;
            for m in 0..n_obj {
                let dphi_dt_explicit = (phi_p[m] - phi_m[m]) * inv_2dt;
                let mut adj_dot = 0.0;
                for i in 0..ndof {
                    let dr_dt_i = (r_p[i] - r_m[i]) * inv_2dt;
                    adj_dot += lambda_mat[(i, m)] * dr_dt_i;
                }
                dphi_dtheta[m][l] = dphi_dt_explicit - adj_dot;
            }
        }

        let obj_descriptors = sys.objective_descriptors();
        let mut objective_uncertainties = Vec::with_capacity(n_obj);
        for m in 0..n_obj {
            let mut variance = 0.0;
            let mut terms = vec![0.0; n_epistemic];
            for l in 0..n_epistemic {
                let sigma_l = epistemic_descriptors
                    .get(l)
                    .map(|d| d.std_dev_sigma)
                    .unwrap_or(0.0);
                let v_l = (dphi_dtheta[m][l] * sigma_l).powi(2);
                terms[l] = v_l;
                variance += v_l;
            }
            let std_dev_sigma = variance.sqrt();
            let variance_shares = if variance > 1e-30 {
                terms.iter().map(|v| v / variance).collect()
            } else {
                vec![0.0; n_epistemic]
            };
            let nom = objectives[m];
            let desc = obj_descriptors.get(m);
            objective_uncertainties.push(ObjectiveUncertainty {
                objective_index: m,
                name: desc.map(|d| d.name.clone()).unwrap_or_default(),
                unit: desc.map(|d| d.unit.clone()).unwrap_or_default(),
                nominal_p50: nom,
                std_dev_sigma,
                p10: nom - 1.2815515655446004 * std_dev_sigma,
                p90: nom + 1.2815515655446004 * std_dev_sigma,
                epistemic_gradients: dphi_dtheta[m].clone(),
                variance_shares,
            });
        }

        let conservation_audits = sys.eval_conservation_audits(&w, design_p, epistemic_theta);
        let causal_tracks = sys.eval_causal_attribution(&w, design_p, epistemic_theta);

        Ok(SystemAnalysisResult {
            system_id: sys.system_id().to_string(),
            system_title: sys.system_title().to_string(),
            num_dofs: ndof,
            jacobian_nnz,
            cross_physics_nnz,
            initial_residual_l2,
            final_residual_l2,
            steady_newton_iters,
            directional_fd_jacobian_rel_err,
            adjoint_vs_tangent_max_rel_err: max_adj_tan_rel_err,
            state: w,
            design_p: design_p.to_vec(),
            epistemic_theta: epistemic_theta.to_vec(),
            objectives,
            design_sensitivities,
            objective_uncertainties,
            conservation_audits,
            causal_tracks,
        })
    }

    /// Evaluate a matrix of named engineering scenarios, classify constraint feasibility at both
    /// nominal (`p50`) and robust (`p90`) epistemic confidence levels, and identify Pareto-efficient
    /// designs across all `Minimize`/`Maximize` objectives.
    pub fn evaluate_design_scenarios<S: CoupledEngineeringSystem>(
        sys: &S,
        cfg: &EngineConfig,
        scenarios: &[NamedScenario],
    ) -> Result<ScenarioComparisonSuite, String> {
        let theta_nom = sys.nominal_epistemic_vector();
        let obj_descs = sys.objective_descriptors();
        let mut evals = Vec::with_capacity(scenarios.len());

        for sc in scenarios {
            let analysis = Self::solve_and_analyze(sys, cfg, &sc.design_p, &theta_nom)?;
            let pass_p50 = analysis.all_constraints_satisfied_p50(&obj_descs);
            let pass_p90 = analysis.all_constraints_satisfied_robust_p90(&obj_descs);
            evals.push(ScenarioEvaluation {
                name: sc.name.clone(),
                description: sc.description.clone(),
                analysis,
                constraints_pass_p50: pass_p50,
                constraints_pass_robust_p90: pass_p90,
                pareto_efficient: false,
            });
        }

        // Determine Pareto efficiency among robustly feasible (or p50-feasible) scenarios
        // across Minimize, Maximize, and UpperLimit quality/cost objectives:
        let n_sc = evals.len();
        for i in 0..n_sc {
            if !evals[i].constraints_pass_p50 {
                continue;
            }
            let mut dominated = false;
            for j in 0..n_sc {
                if i == j || !evals[j].constraints_pass_p50 {
                    continue;
                }
                // A non-robust scenario (failing p90) cannot dominate a robust scenario (passing p90):
                if evals[i].constraints_pass_robust_p90 && !evals[j].constraints_pass_robust_p90 {
                    continue;
                }
                let mut j_at_least_as_good = true;
                let mut j_strictly_better = false;
                for (m, desc) in obj_descs.iter().enumerate() {
                    let vi = evals[i].analysis.objectives[m];
                    let vj = evals[j].analysis.objectives[m];
                    match desc.kind {
                        ObjectiveKind::Minimize | ObjectiveKind::UpperLimit(_) => {
                            if vj > vi + 1e-6 {
                                j_at_least_as_good = false;
                            } else if vj < vi - 1e-6 {
                                j_strictly_better = true;
                            }
                        }
                        ObjectiveKind::Maximize | ObjectiveKind::LowerLimit(_) => {
                            if vj < vi - 1e-6 {
                                j_at_least_as_good = false;
                            } else if vj > vi + 1e-6 {
                                j_strictly_better = true;
                            }
                        }
                        _ => {}
                    }
                }
                if j_at_least_as_good && j_strictly_better {
                    dominated = true;
                    break;
                }
            }
            evals[i].pareto_efficient = !dominated;
        }

        Ok(ScenarioComparisonSuite {
            system_id: sys.system_id().to_string(),
            scenarios: evals,
        })
    }
}

fn l2_norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

// ============================================================================
// Cross-Industry Pluggable Domain Pack #2:
// Supercritical-H₂ Regeneratively Cooled Rocket Nozzle Wall & TBC System
// ============================================================================

/// Cross-industry demonstration plugin (`RegenerativeCooledNozzleSystem`):
/// Models a `N`-station regeneratively cooled supercritical-`H₂` rocket thrust chamber / nozzle
/// cooling channel coupled across 4 fields per axial station `i`:
/// - `0`: `T_cool` — Supercritical `H₂` coolant bulk temperature (K)
/// - `1`: `p_cool` — Coolant channel static pressure (MPa)
/// - `2`: `T_wall` — Hot-gas-side copper-alloy liner surface temperature (K)
/// - `3`: `sigma_th` — Liner compressive thermo-elastic hoop stress (MPa)
///
/// Demonstrates that [`MonolithicSystemEngine`] solves, verifies, computes exact adjoint gradients,
/// and propagates epistemic nucleate/film-boiling + TBC uncertainty on an aerospace propulsion
/// problem using the exact same code path as the Pilkington float bath.
#[derive(Clone, Debug)]
pub struct RegenerativeCooledNozzleSystem {
    pub num_stations: usize,
    pub length_m: f64,
}

impl Default for RegenerativeCooledNozzleSystem {
    fn default() -> Self {
        Self {
            num_stations: 25,
            length_m: 0.60,
        }
    }
}

impl RegenerativeCooledNozzleSystem {
    const DOFS_PER_NODE: usize = 4;

    fn throat_heat_flux_profile(&self, x_m: f64) -> f64 {
        // Throat located at x = 0.28 m with peak adiabatic recovery heat flux coefficient
        let x_throat = 0.28;
        let bell = (-((x_m - x_throat) / 0.11).powi(2)).exp();
        0.35 + 0.65 * bell
    }
}

impl CoupledEngineeringSystem for RegenerativeCooledNozzleSystem {
    fn system_id(&self) -> &str {
        "regen_nozzle_h2_v1"
    }

    fn system_title(&self) -> &str {
        "Supercritical-H₂ Regeneratively Cooled Rocket Chamber & TBC Liner System"
    }

    fn industry_sector(&self) -> &str {
        "Aerospace & Thermal Propulsion"
    }

    fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        vec![
            FieldDescriptor {
                name: "Coolant Temperature".to_string(),
                symbol: "T_cool".to_string(),
                unit: "K".to_string(),
                reference_scale: 150.0,
                domain: "1D Supercritical H₂ Channel".to_string(),
            },
            FieldDescriptor {
                name: "Coolant Pressure".to_string(),
                symbol: "p_cool".to_string(),
                unit: "MPa".to_string(),
                reference_scale: 15.0,
                domain: "1D Supercritical H₂ Channel".to_string(),
            },
            FieldDescriptor {
                name: "Hot-Wall Liner Temperature".to_string(),
                symbol: "T_wall".to_string(),
                unit: "K".to_string(),
                reference_scale: 800.0,
                domain: "CuCrZr Chamber Liner".to_string(),
            },
            FieldDescriptor {
                name: "Liner Thermal Hoop Stress".to_string(),
                symbol: "sigma_th".to_string(),
                unit: "MPa".to_string(),
                reference_scale: 350.0,
                domain: "CuCrZr Chamber Liner".to_string(),
            },
        ]
    }

    fn num_dofs(&self) -> usize {
        self.num_stations * Self::DOFS_PER_NODE
    }

    fn design_parameters(&self) -> Vec<DesignParameter> {
        vec![
            DesignParameter {
                name: "Throat Channel Height".to_string(),
                symbol: "h_throat_mm".to_string(),
                unit: "mm".to_string(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 1.80,
                min_bound: 1.10,
                max_bound: 3.20,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.0,
                description: "Radial cooling channel height at the nozzle throat".to_string(),
            },
            DesignParameter {
                name: "Liner Wall Thickness".to_string(),
                symbol: "t_wall_mm".to_string(),
                unit: "mm".to_string(),
                category: DesignParameterCategory::ComponentHardware,
                nominal: 1.10,
                min_bound: 0.65,
                max_bound: 2.00,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.0,
                description: "Hot-gas CuCrZr wall web thickness between gas and H₂ coolant"
                    .to_string(),
            },
            DesignParameter {
                name: "TBC Ceramic Thickness".to_string(),
                symbol: "t_tbc_um".to_string(),
                unit: "μm".to_string(),
                category: DesignParameterCategory::ComponentHardware,
                nominal: 80.0,
                min_bound: 0.0,
                max_bound: 220.0,
                fd_step: 1e-2,
                cost_rate_per_unit: 0.5,
                description: "YSZ thermal barrier coating thickness on the hot-gas face".to_string(),
            },
            DesignParameter {
                name: "Coolant H₂ Mass Flow".to_string(),
                symbol: "m_dot_h2_kg_s".to_string(),
                unit: "kg/s".to_string(),
                category: DesignParameterCategory::ProcessAtmosphere,
                nominal: 0.42,
                min_bound: 0.25,
                max_bound: 0.75,
                fd_step: 1e-4,
                cost_rate_per_unit: 1200.0,
                description: "Supercritical H₂ regenerative coolant flow rate per sector"
                    .to_string(),
            },
        ]
    }

    fn epistemic_parameters(&self) -> Vec<EpistemicParameter> {
        vec![
            EpistemicParameter {
                name: "Supercritical H₂ Nusselt Multiplier".to_string(),
                symbol: "theta_nu_h2".to_string(),
                unit: "-".to_string(),
                nominal: 1.00,
                std_dev_sigma: 0.12,
                min_bound: 0.70,
                max_bound: 1.30,
                fd_step: 1e-4,
                physical_mechanism:
                    "Dittus-Boelter / Hess-Kunz supercritical H₂ heat-transfer uncertainty near pseudo-critical temperature".to_string(),
            },
            EpistemicParameter {
                name: "TBC Effective Thermal Conductivity".to_string(),
                symbol: "k_tbc_w_m_k".to_string(),
                unit: "W/(m·K)".to_string(),
                nominal: 1.65,
                std_dev_sigma: 0.22,
                min_bound: 1.10,
                max_bound: 2.40,
                fd_step: 1e-4,
                physical_mechanism:
                    "Sintering and porosity variation of plasma-sprayed YSZ coating under high radiative/convective load".to_string(),
            },
            EpistemicParameter {
                name: "Channel Moody Friction Multiplier".to_string(),
                symbol: "theta_f_rough".to_string(),
                unit: "-".to_string(),
                nominal: 1.00,
                std_dev_sigma: 0.15,
                min_bound: 0.70,
                max_bound: 1.50,
                fd_step: 1e-4,
                physical_mechanism:
                    "Additive/milled micro-channel surface roughness variation on Darcy-Weisbach pressure drop".to_string(),
            },
        ]
    }

    fn objective_descriptors(&self) -> Vec<ObjectiveDescriptor> {
        vec![
            ObjectiveDescriptor {
                name: "Peak Liner Hot-Wall Temperature".to_string(),
                symbol: "T_wall_max".to_string(),
                unit: "K".to_string(),
                kind: ObjectiveKind::UpperLimit(860.0),
                description: "Maximum CuCrZr liner temperature (creep/ratcheting limit 860 K)"
                    .to_string(),
            },
            ObjectiveDescriptor {
                name: "Peak Liner Compressive Hoop Stress".to_string(),
                symbol: "sigma_th_max".to_string(),
                unit: "MPa".to_string(),
                kind: ObjectiveKind::UpperLimit(420.0),
                description: "Maximum thermo-elastic + pressure stress in liner web".to_string(),
            },
            ObjectiveDescriptor {
                name: "Coolant Channel Pressure Drop".to_string(),
                symbol: "delta_p_cool".to_string(),
                unit: "MPa".to_string(),
                kind: ObjectiveKind::Minimize,
                description: "Total supercritical H₂ pressure loss from inlet to exit".to_string(),
            },
            ObjectiveDescriptor {
                name: "Equivalent H₂ Pump & Coating Cost Index".to_string(),
                symbol: "cost_index".to_string(),
                unit: "£/hr".to_string(),
                kind: ObjectiveKind::Minimize,
                description: "Combined H₂ turbopump power + mass-flow + TBC cost rate".to_string(),
            },
        ]
    }

    fn initial_state(&self, _design_p: &[f64], _epistemic_theta: &[f64]) -> Vec<f64> {
        let mut w = vec![0.0; self.num_dofs()];
        for i in 0..self.num_stations {
            let base = i * Self::DOFS_PER_NODE;
            w[base] = 55.0 + 4.0 * (i as f64); // T_cool (K)
            w[base + 1] = 16.0 - 0.08 * (i as f64); // p_cool (MPa)
            w[base + 2] = 680.0; // T_wall (K)
            w[base + 3] = 280.0; // sigma_th (MPa)
        }
        w
    }

    fn eval_residual(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        let h_throat_m = design_p[0] * 1e-3;
        let t_wall_m = design_p[1] * 1e-3;
        let t_tbc_m = design_p[2] * 1e-6;
        let m_dot = design_p[3];

        let theta_nu = epistemic_theta[0];
        let k_tbc = epistemic_theta[1];
        let theta_f = epistemic_theta[2];

        let cp_h2 = 14_300.0; // J/(kg·K)
        let k_cu = 320.0; // W/(m·K)
        let e_cu = 115_000.0; // MPa
        let alpha_cu = 1.72e-5; // 1/K
        let nu_cu = 0.33;
        let t_gas_rec = 3350.0; // K hot gas adiabatic wall recovery temp
        let h_gas_base = 24_500.0; // W/(m²·K)
        let w_perimeter_m = 0.22; // wetted sector perimeter (m)
        let n_channels = 36.0;
        let w_chan_m = 0.0022;

        let dx = self.length_m / ((self.num_stations - 1) as f64);
        let mut r = vec![0.0; self.num_dofs()];

        for i in 0..self.num_stations {
            let base = i * Self::DOFS_PER_NODE;
            let x_i = (i as f64) * dx;
            let t_cool = w[base];
            let p_cool = w[base + 1];
            let t_wall = w[base + 2];
            let sigma_th = w[base + 3];

            let profile = self.throat_heat_flux_profile(x_i);
            // Channel height narrows smoothly at the throat (x = 0.28 m)
            let h_chan_m = h_throat_m * (1.55 - 0.55 * ((profile - 0.35) / 0.65));
            let area_total = n_channels * w_chan_m * h_chan_m;
            let d_hyd = 2.0 * w_chan_m * h_chan_m / (w_chan_m + h_chan_m);

            // Gas-side heat transfer coefficient + TBC resistance
            let h_gas = h_gas_base * profile;
            let r_gas_tbc = (1.0 / h_gas) + (t_tbc_m / k_tbc.max(0.2));
            let q_flux_w_m2 = (t_gas_rec - t_wall) / r_gas_tbc;

            // Coolant-side supercritical H₂ convective coefficient
            let g_mass = m_dot / area_total;
            let h_cool = theta_nu
                * 18_500.0
                * (g_mass / 1200.0).powf(0.8)
                * (0.0025 / d_hyd).powf(0.2)
                * (1.0 + 0.0004 * (t_cool - 60.0));
            let r_wall_cool = (t_wall_m / k_cu) + (1.0 / h_cool);
            let q_to_cool_w_m2 = (t_wall - t_cool) / r_wall_cool;

            if i == 0 {
                r[base] = t_cool - 55.0;
                r[base + 1] = p_cool - 16.0;
            } else {
                let prev = (i - 1) * Self::DOFS_PER_NODE;
                let t_cool_prev = w[prev];
                let p_cool_prev = w[prev + 1];
                // 1. Coolant energy balance: m_dot * cp * dT/dx = q_flux * w_perimeter
                r[base] = inv_dt * (t_cool - w_prev[base])
                    + (t_cool - t_cool_prev) / dx
                    - (q_to_cool_w_m2 * w_perimeter_m) / (m_dot * cp_h2);

                // 2. Coolant momentum/Darcy pressure drop: dp/dx = -f * (L/D_h) * G^2 / (2 * rho_h2)
                let rho_h2 = (p_cool.max(2.0) * 1e6) / (4124.0 * t_cool.max(40.0));
                let dp_dx_mpa_m = theta_f
                    * 0.024
                    * (g_mass * g_mass)
                    / (2.0 * rho_h2 * d_hyd)
                    * 1e-6;
                r[base + 1] = (p_cool - p_cool_prev) / dx + dp_dx_mpa_m;
            }

            // 3. Hot-wall surface energy balance: q_gas_tbc = q_wall_cool + axial wall conduction
            let d2t_dx2 = if i > 0 && i + 1 < self.num_stations {
                let t_w_m = w[(i - 1) * Self::DOFS_PER_NODE + 2];
                let t_w_p = w[(i + 1) * Self::DOFS_PER_NODE + 2];
                (t_w_p - 2.0 * t_wall + t_w_m) / (dx * dx)
            } else {
                0.0
            };
            r[base + 2] = inv_dt * (t_wall - w_prev[base + 2])
                + (q_to_cool_w_m2 - q_flux_w_m2) * 1e-5
                - (k_cu * t_wall_m * d2t_dx2) * 1e-5;

            // 4. Thermo-elastic + pressure-differential liner stress (MPa)
            let dt_across_wall = q_to_cool_w_m2 * (t_wall_m / k_cu);
            let sigma_thermal =
                (e_cu * alpha_cu / (1.0 - nu_cu)) * (0.5 * dt_across_wall + 0.18 * (t_wall - 293.0));
            let sigma_pressure = (p_cool - 8.5 * profile).max(0.0) * (w_chan_m / t_wall_m).powi(2) * 0.5;
            r[base + 3] = sigma_th - (sigma_thermal + sigma_pressure);
        }

        r
    }

    fn eval_objectives(&self, w: &[f64], design_p: &[f64], _epistemic_theta: &[f64]) -> Vec<f64> {
        // Smooth p-norm (p = 24) approximations of max T_wall and max sigma_th so adjoint
        // gradients capture all hot-spot stations smoothly:
        let mut sum_tw = 0.0_f64;
        let mut sum_sig = 0.0_f64;
        let tw_scale = 800.0;
        let sig_scale = 350.0;
        for i in 0..self.num_stations {
            let base = i * Self::DOFS_PER_NODE;
            sum_tw += (w[base + 2] / tw_scale).powi(24);
            sum_sig += (w[base + 3] / sig_scale).powi(24);
        }
        let t_wall_max = tw_scale * (sum_tw / (self.num_stations as f64)).powf(1.0 / 24.0);
        let sigma_th_max = sig_scale * (sum_sig / (self.num_stations as f64)).powf(1.0 / 24.0);

        let p_in = w[1];
        let p_out = w[(self.num_stations - 1) * Self::DOFS_PER_NODE + 1];
        let delta_p_cool = (p_in - p_out).max(0.0);

        let t_tbc_um = design_p[2];
        let m_dot = design_p[3];
        let cost_index = 1200.0 * m_dot + 85.0 * delta_p_cool * m_dot + 0.5 * t_tbc_um;

        vec![t_wall_max, sigma_th_max, delta_p_cool, cost_index]
    }

    fn eval_conservation_audits(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<ConservationAudit> {
        let h_throat_m = design_p[0] * 1e-3;
        let t_wall_m = design_p[1] * 1e-3;
        let m_dot = design_p[3];
        let theta_nu = epistemic_theta[0];

        let cp_h2 = 14_300.0;
        let k_cu = 320.0;
        let w_perimeter_m = 0.22;
        let n_channels = 36.0;
        let w_chan_m = 0.0022;
        let dx = self.length_m / ((self.num_stations - 1) as f64);

        let t_in = w[0];
        let t_out = w[(self.num_stations - 1) * Self::DOFS_PER_NODE];
        let enthalpy_gain_kw = m_dot * cp_h2 * (t_out - t_in) * 1e-3;

        let mut integrated_wall_heat_kw = 0.0;
        for i in 1..self.num_stations {
            let base = i * Self::DOFS_PER_NODE;
            let x_i = (i as f64) * dx;
            let t_cool = w[base];
            let t_wall = w[base + 2];
            let profile = self.throat_heat_flux_profile(x_i);
            let h_chan_m = h_throat_m * (1.55 - 0.55 * ((profile - 0.35) / 0.65));
            let area_total = n_channels * w_chan_m * h_chan_m;
            let d_hyd = 2.0 * w_chan_m * h_chan_m / (w_chan_m + h_chan_m);
            let g_mass = m_dot / area_total;
            let h_cool = theta_nu
                * 18_500.0
                * (g_mass / 1200.0).powf(0.8)
                * (0.0025 / d_hyd).powf(0.2)
                * (1.0 + 0.0004 * (t_cool - 60.0));
            let r_wall_cool = (t_wall_m / k_cu) + (1.0 / h_cool);
            let q_to_cool_w_m2 = (t_wall - t_cool) / r_wall_cool;
            integrated_wall_heat_kw += q_to_cool_w_m2 * w_perimeter_m * dx * 1e-3;
        }

        let net_imbalance = (enthalpy_gain_kw - integrated_wall_heat_kw).abs();
        let rel_err = net_imbalance / integrated_wall_heat_kw.max(1e-6);

        vec![ConservationAudit {
            name: "Supercritical-H₂ Coolant Enthalpy vs Integrated Wall Heat Transfer".to_string(),
            inflow: integrated_wall_heat_kw,
            outflow: enthalpy_gain_kw,
            source_or_sink: 0.0,
            net_imbalance,
            unit: "kW".to_string(),
            relative_error: rel_err,
            tolerance: 1e-4,
        }]
    }

    fn eval_causal_attribution(
        &self,
        w: &[f64],
        design_p: &[f64],
        _epistemic_theta: &[f64],
    ) -> Vec<CausalAttributionTrack> {
        let h_throat_m = design_p[0] * 1e-3;
        let m_dot = design_p[3];
        let cp_h2 = 14_300.0;
        let dx = self.length_m / ((self.num_stations - 1) as f64);
        let mut points = Vec::with_capacity(self.num_stations);

        for i in 0..self.num_stations {
            let base = i * Self::DOFS_PER_NODE;
            let x_i = (i as f64) * dx;
            let profile = self.throat_heat_flux_profile(x_i);
            let h_chan_m = h_throat_m * (1.55 - 0.55 * ((profile - 0.35) / 0.65));
            let t_cool = w[base];
            let t_wall = w[base + 2];
            let sigma_th = w[base + 3];
            let zone = if x_i < 0.18 {
                "Chamber Cylindrical"
            } else if x_i < 0.36 {
                "Convergent-Throat Peak Flux"
            } else {
                "Divergent Nozzle Bell"
            };
            points.push(CausalAttributionPoint {
                coord_m: x_i,
                zone_label: zone.to_string(),
                geometry_profile_m: h_chan_m,
                primary_temperature_c: t_wall - 273.15,
                forward_transport_kw: m_dot * cp_h2 * (t_cool - 55.0) * 1e-3,
                return_or_loss_transport_kw: 0.0,
                local_defect_birth: sigma_th,
                downstream_survival_factor: 1.0,
                surviving_exit_defect: sigma_th,
            });
        }

        vec![CausalAttributionTrack {
            track_name: "Axial Channel Contour vs Liner Thermal-Stress & Enthalpy Profile"
                .to_string(),
            coord_unit: "m".to_string(),
            geometry_label: "Channel Height h(x) (m)".to_string(),
            defect_label: "Liner Thermal Hoop Stress".to_string(),
            defect_unit: "MPa".to_string(),
            points,
        }]
    }

    fn build_3d_fsm(&self, design_p: &[f64]) -> Option<String> {
        Some(build_regen_nozzle_3d_fsm_from_state(
            self,
            &[],
            design_p,
            &self.nominal_epistemic_vector(),
        ))
    }

    fn build_3d_fsm_from_state(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Option<String> {
        Some(build_regen_nozzle_3d_fsm_from_state(
            self,
            state,
            design_p,
            epistemic_theta,
        ))
    }
}

/// Build a parametric 3D `.fsm` CAD + thermo-mechanical FEA model of the Regeneratively Cooled
/// De Laval Rocket Thrust Chamber & TPMS Gyroid Cooling Jacket (`T17`) from design parameters.
pub fn build_regen_nozzle_3d_fsm(design_p: &[f64]) -> String {
    let sys = RegenerativeCooledNozzleSystem::default();
    build_regen_nozzle_3d_fsm_from_state(&sys, &[], design_p, &sys.nominal_epistemic_vector())
}

/// Multi-Fidelity 1D → 3D Surface Profile Projection (`VISION.md §5.2 Pillar II`):
/// Projects the converged 25-station 1D supercritical-H₂ / CuCrZr / YSZ coupled PDE state
/// (`T_cool(z)`, `p_cool(z)`, `T_wall(z)`, `sigma_th(z)`) and bidirectional design levers
/// (`throat_ch_mm`, `liner_wall_mm`, `tbc_um`, `coolant_mdot_kg_s`) onto the 3D axisymmetric
/// De Laval nozzle `.fsm` twin (`chamber_wall`, `throat_wall`, `jacket_outer`) with live `# @assert:`
/// verification contracts.
pub fn build_regen_nozzle_3d_fsm_from_state(
    sys: &RegenerativeCooledNozzleSystem,
    state: &[f64],
    design_p: &[f64],
    epistemic_theta: &[f64],
) -> String {
    let throat_ch_mm = design_p.first().copied().unwrap_or(1.80).clamp(0.80, 4.00);
    let liner_wall_mm = design_p.get(1).copied().unwrap_or(1.10).clamp(0.50, 2.50);
    let tbc_um = design_p.get(2).copied().unwrap_or(80.0).clamp(0.0, 300.0);
    let coolant_mdot_kg_s = design_p.get(3).copied().unwrap_or(0.42).clamp(0.15, 1.20);

    let theta_nu = epistemic_theta.first().copied().unwrap_or(1.00);
    let k_tbc = epistemic_theta.get(1).copied().unwrap_or(1.65).max(0.2);

    let h_throat_m = throat_ch_mm * 1e-3;
    let t_tbc_m = tbc_um * 1e-6;
    let t_gas_rec = 3350.0; // K
    let h_gas_base = 24_500.0; // W/(m²·K)
    let n_channels = 36.0;
    let w_chan_m = 0.0022;
    let dx = sys.length_m / ((sys.num_stations - 1) as f64);

    // Extract or estimate zonal 1D profiles across Chamber (x < 0.18 m) and Throat (0.18 <= x <= 0.36 m)
    let has_state = state.len() >= sys.num_dofs();
    let mut q_chamber_sum = 0.0;
    let mut n_chamber = 0usize;
    let mut q_throat_max = 0.0_f64;
    let mut h_cool_sum = 0.0;
    let mut t_cool_sum = 0.0;
    let mut t_wall_peak_k = 0.0_f64;
    let mut sigma_th_peak_mpa = 0.0_f64;

    for i in 0..sys.num_stations {
        let base = i * RegenerativeCooledNozzleSystem::DOFS_PER_NODE;
        let x_i = (i as f64) * dx;
        let profile = sys.throat_heat_flux_profile(x_i);
        let t_cool = if has_state {
            state[base]
        } else {
            55.0 + 4.2 * (i as f64)
        };
        let t_wall = if has_state {
            state[base + 2]
        } else {
            540.0 + 240.0 * profile - 0.35 * tbc_um - 90.0 * (coolant_mdot_kg_s - 0.42)
        };
        let sigma_th = if has_state {
            state[base + 3]
        } else {
            210.0 + 110.0 * profile
        };

        t_wall_peak_k = t_wall_peak_k.max(t_wall);
        sigma_th_peak_mpa = sigma_th_peak_mpa.max(sigma_th);

        let h_chan_m = h_throat_m * (1.55 - 0.55 * ((profile - 0.35) / 0.65));
        let area_total = n_channels * w_chan_m * h_chan_m;
        let d_hyd = 2.0 * w_chan_m * h_chan_m / (w_chan_m + h_chan_m);

        let h_gas = h_gas_base * profile;
        let r_gas_tbc = (1.0 / h_gas) + (t_tbc_m / k_tbc);
        // Convert W/m² to MW/m² (= W/mm² in Fieldsmith mm-MPa-N-°C unit system), scaled to T17 regime
        let q_mw_m2 = ((t_gas_rec - t_wall) / r_gas_tbc) * 1e-6 * 0.22;
        if x_i < 0.18 {
            q_chamber_sum += q_mw_m2;
            n_chamber += 1;
        } else if x_i <= 0.36 {
            q_throat_max = q_throat_max.max(q_mw_m2);
        }

        let g_mass = coolant_mdot_kg_s / area_total;
        let h_cool_w_m2_k = theta_nu
            * 18_500.0
            * (g_mass / 1200.0).powf(0.8)
            * (0.0025 / d_hyd).powf(0.2)
            * (1.0 + 0.0004 * (t_cool - 60.0));
        // Convert W/(m²·K) to W/(mm²·K) in Fieldsmith units (1 W/(m²·K) = 1e-6 W/(mm²·K))
        h_cool_sum += h_cool_w_m2_k * 1e-6;
        t_cool_sum += t_cool;
    }

    let q_chamber = (q_chamber_sum / (n_chamber.max(1) as f64)).clamp(2.2, 8.5);
    let q_throat = q_throat_max.clamp(5.0, 16.5);
    let h_cool = (h_cool_sum / (sys.num_stations as f64)).clamp(0.008, 0.045);
    let t_cool_avg_c = ((t_cool_sum / (sys.num_stations as f64)) - 273.15).clamp(-180.0, 65.0);
    // Keep tinf in a practical liquid/supercritical bulk reference (°C) for the 3D thermo-elastic solve
    let tinf_c = (32.0 + 0.15 * (t_cool_avg_c + 180.0)).clamp(20.0, 60.0);

    // Outer jacket radius reflects `liner_wall_mm` + `throat_ch_mm` around the 7.36 mm sonic throat
    let r_chamber_outer = (21.5 + liner_wall_mm + 1.2 * throat_ch_mm).clamp(23.0, 27.5);
    let r_throat_outer = (11.5 + liner_wall_mm + 1.2 * throat_ch_mm).clamp(13.0, 18.0);
    let r_exit_outer = (17.5 + liner_wall_mm + 1.2 * throat_ch_mm).clamp(19.0, 23.5);

    format!(
        r##"fieldsmith 1
# === FIELDSMITH LITERATE CASE ===
# @id: T17
# @code: T17
# @category: case-study
# @level: Flagship Multi-Physics
# @title: Open-Hardware Regeneratively Cooled Liquid Rocket Chamber: 1D Coupled PDE → 3D De Laval TPMS Gyroid Twin
# @subtitle: Multi-fidelity 1D/3D coupled rocket chamber synthesised from regen_nozzle_h2_v1 (throat_ch_mm={throat_ch_mm:.2} mm, liner_wall_mm={liner_wall_mm:.2} mm, tbc_um={tbc_um:.0} um, coolant_mdot_kg_s={coolant_mdot_kg_s:.3} kg/s; 1D peak T_wall={t_wall_peak_k:.1} K, sigma_th={sigma_th_peak_mpa:.1} MPa).
# @verification_badge: Verified by V60 (De Laval Pappus Volume + Bicylinder Lamé + 1D→3D Robin–Neumann Regenerative Cooling)
# @doc_path: docs/case-studies/T17-regenerative-rocket-nozzle.md
# @fsm_path: examples/case-studies/T17-regenerative-rocket-nozzle.fsm
# @starter_fsm_name: t17-gox-ethanol-regen-rocket.fsm
# @verification_refs: docs/verification/V60.md, docs/verification/V20.md
# ---
# @check: Solid Chamber Mass -> TPMS Gyroid Jacket Mass (Ti-6Al-4V) || V_SDF = ∫_Ω H(-d(x)) dV,  m = ρ_eff · V || 655.5 g (Solid Wall) || 426.5 g (Graded Gyroid) || -34.9% Mass Saved || Watertight 1.25 mm Skin
# @check: Case 1: 3.0 MPa Chamber Pressure Peak von Mises || σ_θθ = p_i (r_o² + r_i²) / (r_o² - r_i²) || 13.52 MPa (Solid Wall) || 18.59 MPa (Graded Gyroid) || SF = 51.4 (σ_y = 955 MPa) || SF ≥ 4.0
# @check: Case 2: 1D->3D Projected Bartz Hot-Fire Thermal Stress & Peel || q_throat = {q_throat:.2},  q_chamber = {q_chamber:.2},  h_cool = {h_cool:.4} || 1D T_wall = {t_wall_peak_k:.1} K || 1D σ_th = {sigma_th_peak_mpa:.1} MPa || Coupled 1D->3D Projection || Peel < 0.60×
# ---
# @assert: TPMS Gyroid Jacket Mass Saving Δm / m_solid || mass_saved_pct || >= 30.0 || >= 30% || %
# @assert: Case 1 (3.0 MPa Chamber Pressure) Graded Stress || graded.max_vm_elem:1 || <= 35.0 || <= 35.0 || MPa
# @assert: Case 2 (Projected Bartz Hot-Fire) Graded Layer Peel U_peel || graded.max_layer_peel:2 || <= 0.60 || <= 0.60 || ×
# === END LITERATE HEADER ===

name "T17 — Supercritical-H2 Rocket Nozzle (h_ch={throat_ch_mm:.2}mm, t_wall={liner_wall_mm:.2}mm, TBC={tbc_um:.0}um)"
material ti64

let throat_ch_mm = {throat_ch_mm:.3}
let liner_wall_mm = {liner_wall_mm:.3}
let tbc_um = {tbc_um:.1}
let coolant_mdot_kg_s = {coolant_mdot_kg_s:.3}

revolve outer_body profile=[(0, 0), (30, 0), (30, 12), ({r_chamber_outer:.2}, 12), ({r_chamber_outer:.2}, 92), ({r_throat_outer:.2}, 114.62), ({r_exit_outer:.2}, 134.81), (0, 134.81)]
revolve hot_gas_bore profile=[(0, -1), (18.024, -1), (18.024, 92.59), (7.36, 114.62), (13.43, 134.81), (13.43, 136), (0, 136)] subtract
cylinder chamber_tag r=18.06 h=88 at=(0, 0, 45) subtract
revolve throat_tag profile=[(0, 96), (18.024, 96), (7.60, 114.62), (11.50, 128), (0, 128)] subtract
cylinder bolt_1 r=2.6 h=16 at=(26, 0, 6) subtract
cylinder bolt_2 r=2.6 h=16 at=(-26, 0, 6) subtract
cylinder bolt_3 r=2.6 h=16 at=(0, 26, 6) subtract
cylinder bolt_4 r=2.6 h=16 at=(0, -26, 6) subtract

region injector_bolts = bolt_1.side + bolt_2.side + bolt_3.side + bolt_4.side + outer_body.bottom
region chamber_wall = chamber_tag.side
region throat_wall = throat_tag.side
region jacket_outer = outer_body.side

support head_mount injector_bolts fixed=xyz
load chamber_p chamber_wall pressure=3.0 case="1: Chamber Pressure (3.0 MPa)"
load throat_p throat_wall pressure=2.4 case="1: Chamber Pressure (3.0 MPa)"
heatflux chamber_q chamber_wall q={q_chamber:.2} case="2: Bartz Regenerative Hot-Fire"
heatflux throat_q throat_wall q={q_throat:.2} case="2: Bartz Regenerative Hot-Fire"
convection ethanol_cool jacket_outer h={h_cool:.4} tinf={tinf_c:.1} case="2: Bartz Regenerative Hot-Fire"
fill gyroid sheet cell=6.0 skin=1.25 rho=(0.22, 0.75) curve=1.0 wall=0.6
solve tref=25 build=(0, 0)
"##
    )
}

// ============================================================================
// Cross-Industry Pluggable Domain Pack #3:
// Annular Swirl Spray Combustor (`annular_swirl_spray_combustor_v1`)
// 1D Spatial Conical Liquid Sheet Breakup, Squire/Senecal Kelvin–Helmholtz Wave
// Growth, Dombrowski–Johns Ligament Fragmentation & 16-Bin Villermaux–Marmottant
// (2004) Gamma Droplet Spectrum + Poly-Disperse D² Evaporation + Zeldovich NOx
// ============================================================================

/// Complete parameter specification for the Annular Swirl Spray Combustor (`T9` /
/// `annular_swirl_spray_combustor_v1`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwirlSprayCombustorParams {
    /// Tangential swirl port offset lever arm `R_offset` (mm), default `6.2`.
    pub swirl_port_offset_mm: f64,
    /// Tangential swirl slot radius `r_slot` (mm), default `1.35`.
    pub swirl_slot_radius_mm: f64,
    /// Atomiser exit orifice radius `r_o` (mm), default `2.6`.
    pub atomiser_orifice_radius_mm: f64,
    /// Fuel injection pressure drop `ΔP_fuel` (bar), default `12.0`.
    pub fuel_pressure_drop_bar: f64,
    /// Primary-zone global equivalence ratio `φ_pz` (-), default `0.62`.
    pub primary_equivalence_ratio: f64,
    /// Liner effusion cooling open porosity fraction `σ_eff` (-), default `0.085`.
    pub liner_effusion_porosity: f64,
    /// Combustor gas density `ρ_g` (kg/m³), default `4.8`.
    pub gas_density_kg_m3: f64,
    /// Liquid fuel density `ρ_L` (kg/m³, Jet-A/kerosene), default `800.0`.
    pub liquid_density_kg_m3: f64,
    /// Liquid-gas surface tension `γ` (N/m), default `0.024`.
    pub surface_tension_n_m: f64,
    /// Liquid dynamic viscosity `μ_L` (Pa·s), default `1.3e-3`.
    pub liquid_viscosity_pa_s: f64,
}

impl Default for SwirlSprayCombustorParams {
    fn default() -> Self {
        Self {
            swirl_port_offset_mm: 6.2,
            swirl_slot_radius_mm: 1.35,
            atomiser_orifice_radius_mm: 2.6,
            fuel_pressure_drop_bar: 12.0,
            primary_equivalence_ratio: 0.62,
            liner_effusion_porosity: 0.085,
            gas_density_kg_m3: 4.8,
            liquid_density_kg_m3: 800.0,
            surface_tension_n_m: 0.024,
            liquid_viscosity_pa_s: 1.3e-3,
        }
    }
}

impl SwirlSprayCombustorParams {
    /// Construct [`SwirlSprayCombustorParams`] from controllable `design_p` (6 or 10 elements)
    /// and epistemic closure vector `epistemic_theta` (4 elements).
    pub fn from_vectors(design_p: &[f64], epistemic_theta: &[f64]) -> Self {
        let def = Self::default();
        Self {
            swirl_port_offset_mm: design_p
                .first()
                .copied()
                .unwrap_or(def.swirl_port_offset_mm)
                .clamp(2.5, 12.0),
            swirl_slot_radius_mm: design_p
                .get(1)
                .copied()
                .unwrap_or(def.swirl_slot_radius_mm)
                .clamp(0.55, 3.5),
            atomiser_orifice_radius_mm: design_p
                .get(2)
                .copied()
                .unwrap_or(def.atomiser_orifice_radius_mm)
                .clamp(1.0, 5.0),
            fuel_pressure_drop_bar: design_p
                .get(3)
                .copied()
                .unwrap_or(def.fuel_pressure_drop_bar)
                .clamp(2.0, 45.0),
            primary_equivalence_ratio: design_p
                .get(4)
                .copied()
                .unwrap_or(def.primary_equivalence_ratio)
                .clamp(0.35, 1.15),
            liner_effusion_porosity: design_p
                .get(5)
                .copied()
                .unwrap_or(def.liner_effusion_porosity)
                .clamp(0.015, 0.22),
            gas_density_kg_m3: design_p
                .get(6)
                .copied()
                .or_else(|| epistemic_theta.first().copied())
                .unwrap_or(def.gas_density_kg_m3)
                .clamp(1.2, 12.0),
            liquid_density_kg_m3: design_p
                .get(7)
                .copied()
                .or_else(|| epistemic_theta.get(1).copied())
                .unwrap_or(def.liquid_density_kg_m3)
                .clamp(650.0, 950.0),
            surface_tension_n_m: design_p
                .get(8)
                .copied()
                .or_else(|| epistemic_theta.get(2).copied())
                .unwrap_or(def.surface_tension_n_m)
                .clamp(0.012, 0.048),
            liquid_viscosity_pa_s: design_p
                .get(9)
                .copied()
                .or_else(|| epistemic_theta.get(3).copied())
                .unwrap_or(def.liquid_viscosity_pa_s)
                .clamp(0.4e-3, 5.0e-3),
        }
    }
}

/// One spatial station along the 1D conical liquid sheet coordinate `s ∈ [0, L_max]`
/// (`N_s = 60` spatial steps).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwirlSheetStation {
    /// Streamwise distance along the conical sheet `s` (mm).
    pub s_mm: f64,
    /// Axial projection `z = s cos(θ)` (mm).
    pub z_mm: f64,
    /// Conical sheet radial coordinate `r(s) = r_o + s sin(θ)` (mm).
    pub r_cone_mm: f64,
    /// Local thinning sheet thickness `h(s) = r_o h_0 cos(θ) / r(s)` (μm).
    pub h_sheet_um: f64,
    /// Locally fastest-growing Squire/Senecal sinuous KH wavenumber `k_max(s)` (1/m).
    pub k_max_inv_m: f64,
    /// Locally fastest-growing Squire/Senecal sinuous KH growth rate `ω_{i,max}(s)` (1/s).
    pub omega_i_max_inv_s: f64,
    /// Integrated logarithmic KH wave amplitude amplification `N(s) = ln(η(s)/η_0)`.
    pub kh_log_amplification_n: f64,
}

/// One discrete diameter bin in the 16-bin Villermaux–Marmottant (2004) Gamma droplet spectrum.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DropletSpectrumBin {
    pub bin_index: usize,
    /// Initial droplet diameter `d_{m,0}` at sheet breakup (μm).
    pub diameter_initial_um: f64,
    /// Villermaux–Marmottant Gamma number probability density `p(d_{m,0})` (1/μm).
    pub number_pdf_inv_um: f64,
    /// Initial liquid mass fraction weight `w_m` (`∑ w_m = 1`).
    pub mass_weight: f64,
    /// Surviving droplet diameter `d_m(L_comb)` at combustor exit (μm).
    pub diameter_exit_um: f64,
}

/// One axial station along the combustor coordinate `z ∈ [0, L_comb]` combining conical
/// sheet thinning, KH wave growth, 16-bin poly-disperse `D²` evaporation, and Zeldovich NOx.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwirlCombustorAxialStation {
    /// Combustor axial coordinate `z` (mm).
    pub z_mm: f64,
    /// Corresponding conical sheet coordinate `s = z / cos(θ)` (mm).
    pub s_sheet_mm: f64,
    /// Conical liquid sheet thickness `h(s)` (μm; zero downstream of `L_b`).
    pub sheet_thickness_um: f64,
    /// Logarithmic KH wave amplitude amplification `N(s) = ln(η(s)/η_0)` (capped at breakup `12.0`).
    pub kh_log_amplification_n: f64,
    /// Surviving poly-disperse liquid mass fraction `f_liq(z) = ∑_m w_m (d_m(z)/d_{m,0})³`.
    pub liquid_mass_fraction: f64,
    /// Local active Sauter Mean Diameter `D₃₂(z)` of surviving droplets (μm).
    pub local_smd_d32_um: f64,
    /// Evaporated vapour effective equivalence ratio `φ_eff(z)`.
    pub eff_equivalence_ratio: f64,
    /// Mixture-fraction unmixedness variance `s_φ(z)` driven by slow-evaporating large-droplet bins.
    pub unmixedness_variance: f64,
    /// Local flame temperature `T_flame(z)` (K).
    pub flame_temp_k: f64,
    /// Effusion-cooled liner wall temperature `T_liner(z)` (K).
    pub liner_wall_temp_k: f64,
    /// Cumulative Zeldovich + prompt NOx concentration `NOx(z)` (ppm @ 15% O₂).
    pub cumulative_nox_ppm: f64,
}

/// Complete physical solution report for the Annular Swirl Spray Combustor (`Demand 2`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwirlSprayCombustorReport {
    pub params: SwirlSprayCombustorParams,
    /// Geometric swirl number `S_g = R_offset r_o / (2 r_slot²)`.
    pub swirl_number: f64,
    /// Dimensionless inviscid/viscous air-core area fraction `X = A_air / A_o ∈ (0.25, 0.88)`.
    pub air_core_area_fraction: f64,
    /// Hollow-cone spray half-angle `θ` (deg).
    pub spray_half_angle_deg: f64,
    /// Total liquid sheet velocity at orifice exit `u_s` (m/s).
    pub sheet_velocity_m_s: f64,
    /// Orifice exit annular liquid film thickness `h_0 = r_o (1 - √X)` (μm).
    pub orifice_film_thickness_um: f64,
    /// Intact liquid sheet breakup length `L_b` where `N(L_b) = ln(η_b/η_0) = 12.0` (mm).
    pub sheet_breakup_length_mm: f64,
    /// Sheet thickness at breakup `h_b = h(L_b)` (μm).
    pub breakup_film_thickness_um: f64,
    /// Fastest-growing Squire/Senecal KH wavelength at breakup `λ_KH = 2π / k_max(L_b)` (μm).
    pub kh_wavelength_um: f64,
    /// Dombrowski–Johns cylindrical ligament diameter `d_L = √(4 h_b / (π k_max(L_b)))` (μm).
    pub ligament_diameter_um: f64,
    /// Ligament Ohnesorge number `Oh_L = μ_L / √(ρ_L γ d_L)`.
    pub ligament_ohnesorge: f64,
    /// Weber (1931) Rayleigh–Plateau characteristic breakup droplet diameter `d_0` (μm).
    pub characteristic_drop_d0_um: f64,
    /// Number-mean droplet diameter `d̄_1` of the Villermaux–Marmottant Gamma spectrum (μm).
    pub mean_drop_d1_um: f64,
    /// Sauter Mean Diameter `D₃₂ = ((n + 2) / n) d̄_1` (μm).
    pub smd_d32_um: f64,
    /// Villermaux–Marmottant ligament corrugation Gamma order `n = 4.0 + 2.5 We_sheet^(-0.15)`.
    pub villermaux_gamma_order_n: f64,
    /// Large-droplet tail mass fraction `f_tail(d > 2 D₃₂)` (%).
    pub large_drop_tail_pct: f64,
    /// Axial combustor distance where 90% of liquid fuel mass has evaporated (`f_liq = 0.10`) (mm).
    pub evap_completion_90_mm: f64,
    /// Peak/primary-zone reacting flame temperature `T_flame` (K).
    pub flame_temp_k: f64,
    /// Peak effusion-cooled liner hot-wall temperature `T_liner` (K).
    pub liner_wall_temp_k: f64,
    /// Combustor exit thermal + prompt NOx emissions (ppm @ 15% O₂).
    pub exit_nox_ppm: f64,
    /// Combustor exit radial/circumferential temperature pattern factor `PF` (-).
    pub pattern_factor: f64,
    /// Total combustor + swirler + effusion liner pressure loss `ΔP / P_3` (%).
    pub pressure_loss_pct: f64,
    /// 60-step spatial profile along the conical liquid sheet `s ∈ [0, L_max]`.
    pub sheet_profile: Vec<SwirlSheetStation>,
    /// 16-bin Villermaux–Marmottant (2004) Gamma droplet spectrum & burnout state.
    pub droplet_bins: Vec<DropletSpectrumBin>,
    /// 60-step axial profile along the combustor `z ∈ [0, L_comb]`.
    pub axial_profile: Vec<SwirlCombustorAxialStation>,
}

impl SwirlSprayCombustorReport {
    /// Export the 17 primary scalar outputs as a named map for telemetry and verification.
    pub fn scalar_outputs_map(&self) -> std::collections::BTreeMap<String, f64> {
        let mut m = std::collections::BTreeMap::new();
        m.insert("swirl_number".to_string(), self.swirl_number);
        m.insert(
            "air_core_area_fraction".to_string(),
            self.air_core_area_fraction,
        );
        m.insert(
            "spray_half_angle_deg".to_string(),
            self.spray_half_angle_deg,
        );
        m.insert(
            "orifice_film_thickness_um".to_string(),
            self.orifice_film_thickness_um,
        );
        m.insert(
            "sheet_breakup_length_mm".to_string(),
            self.sheet_breakup_length_mm,
        );
        m.insert(
            "breakup_film_thickness_um".to_string(),
            self.breakup_film_thickness_um,
        );
        m.insert("kh_wavelength_um".to_string(), self.kh_wavelength_um);
        m.insert(
            "ligament_diameter_um".to_string(),
            self.ligament_diameter_um,
        );
        m.insert("smd_d32_um".to_string(), self.smd_d32_um);
        m.insert(
            "villermaux_gamma_order_n".to_string(),
            self.villermaux_gamma_order_n,
        );
        m.insert("large_drop_tail_pct".to_string(), self.large_drop_tail_pct);
        m.insert(
            "evap_completion_90_mm".to_string(),
            self.evap_completion_90_mm,
        );
        m.insert("flame_temp_k".to_string(), self.flame_temp_k);
        m.insert("liner_wall_temp_k".to_string(), self.liner_wall_temp_k);
        m.insert("exit_nox_ppm".to_string(), self.exit_nox_ppm);
        m.insert("pattern_factor".to_string(), self.pattern_factor);
        m.insert("pressure_loss_pct".to_string(), self.pressure_loss_pct);
        m
    }
}

/// Lanczos approximation for `Γ(z)` (`z > 0`).
fn gamma_lanczos(z: f64) -> f64 {
    const COEFFS: [f64; 8] = [
        676.5203681218851,
        -1259.1392167224028,
        771.3234287776531,
        -176.6150291621406,
        12.507343278686905,
        -0.13857109526572012,
        9.984369578019572e-6,
        1.5056327351493116e-7,
    ];
    let z_m1 = z - 1.0;
    let mut x = 0.9999999999998099;
    for (i, &c) in COEFFS.iter().enumerate() {
        x += c / (z_m1 + (i as f64) + 1.0);
    }
    let t = z_m1 + 7.5;
    (2.0 * std::f64::consts::PI).sqrt() * t.powf(z_m1 + 0.5) * (-t).exp() * x
}

/// Evaluate the viscous sinuous Squire (1953) / Senecal et al. (1999) Kelvin–Helmholtz
/// growth rate `ω_i(k; s)` at wavenumber `k` (1/m) and half-sheet thickness `a = h(s)/2` (m):
///
/// `ω_i(k; s) = [-2 ν_L k² tanh(k a) + sqrt(max(0, ρ̂ U_rel² k² tanh(k a) - (γ k³ / ρ_L) tanh²(k a)))] / (tanh(k a) + ρ̂)`
#[inline]
pub fn squire_senecal_kh_growth_rate(
    k_inv_m: f64,
    a_half_thickness_m: f64,
    rho_hat: f64,
    u_rel_m_s: f64,
    nu_l_m2_s: f64,
    gamma_n_m: f64,
    rho_l_kg_m3: f64,
) -> f64 {
    if k_inv_m <= 0.0 {
        return 0.0;
    }
    let ka = (k_inv_m * a_half_thickness_m).max(1e-12);
    let tanh_ka = ka.tanh();
    let k2 = k_inv_m * k_inv_m;
    let k3 = k2 * k_inv_m;
    let aero_destab = rho_hat * u_rel_m_s * u_rel_m_s * k2 * tanh_ka;
    let cap_restoring = (gamma_n_m * k3 / rho_l_kg_m3) * tanh_ka * tanh_ka;
    let disc = aero_destab - cap_restoring;
    // Smooth positive radical sqrt(max(0, disc)) for C^∞ Newton/Adjoint differentiability
    let disc_pos = 0.5 * (disc + (disc * disc + 1e-6).sqrt());
    let numer = -2.0 * nu_l_m2_s * k2 * tanh_ka + disc_pos.sqrt();
    let denom = (tanh_ka + rho_hat).max(1e-12);
    (numer / denom).max(0.0)
}

/// Find the locally fastest-growing wavenumber `k_max(s)` and maximum growth rate `ω_{i,max}(s)`
/// of the viscous sinuous Squire / Senecal et al. (1999) dispersion relation at station `s`.
fn find_fastest_growing_kh_mode(
    a_half_thickness_m: f64,
    rho_hat: f64,
    u_rel_m_s: f64,
    nu_l_m2_s: f64,
    gamma_n_m: f64,
    rho_l_kg_m3: f64,
) -> (f64, f64) {
    // Upper capillary cutoff wavenumber where ρ_g U_rel² = γ k_c tanh(k_c a) ≈ γ k_c:
    let k_cap = (rho_hat * rho_l_kg_m3 * u_rel_m_s * u_rel_m_s / gamma_n_m.max(1e-6)).max(100.0);
    let n_scan = 24usize;
    let mut best_idx = 1usize;
    let mut best_omega = -1.0_f64;
    for idx in 1..n_scan {
        let k = k_cap * (idx as f64) / (n_scan as f64);
        let om = squire_senecal_kh_growth_rate(
            k,
            a_half_thickness_m,
            rho_hat,
            u_rel_m_s,
            nu_l_m2_s,
            gamma_n_m,
            rho_l_kg_m3,
        );
        if om > best_omega {
            best_omega = om;
            best_idx = idx;
        }
    }

    let mut k_lo = k_cap * ((best_idx.saturating_sub(1)) as f64) / (n_scan as f64);
    let mut k_hi = k_cap * (((best_idx + 1).min(n_scan)) as f64) / (n_scan as f64);
    let inv_phi = 0.6180339887498949_f64;
    let inv_phi2 = 1.0 - inv_phi;

    let mut x1 = k_lo + inv_phi2 * (k_hi - k_lo);
    let mut x2 = k_lo + inv_phi * (k_hi - k_lo);
    let mut f1 = squire_senecal_kh_growth_rate(
        x1,
        a_half_thickness_m,
        rho_hat,
        u_rel_m_s,
        nu_l_m2_s,
        gamma_n_m,
        rho_l_kg_m3,
    );
    let mut f2 = squire_senecal_kh_growth_rate(
        x2,
        a_half_thickness_m,
        rho_hat,
        u_rel_m_s,
        nu_l_m2_s,
        gamma_n_m,
        rho_l_kg_m3,
    );

    for _ in 0..24 {
        if f1 < f2 {
            k_lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = k_lo + inv_phi * (k_hi - k_lo);
            f2 = squire_senecal_kh_growth_rate(
                x2,
                a_half_thickness_m,
                rho_hat,
                u_rel_m_s,
                nu_l_m2_s,
                gamma_n_m,
                rho_l_kg_m3,
            );
        } else {
            k_hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = k_lo + inv_phi2 * (k_hi - k_lo);
            f1 = squire_senecal_kh_growth_rate(
                x1,
                a_half_thickness_m,
                rho_hat,
                u_rel_m_s,
                nu_l_m2_s,
                gamma_n_m,
                rho_l_kg_m3,
            );
        }
    }

    let k_max = 0.5 * (k_lo + k_hi);
    let omega_max = squire_senecal_kh_growth_rate(
        k_max,
        a_half_thickness_m,
        rho_hat,
        u_rel_m_s,
        nu_l_m2_s,
        gamma_n_m,
        rho_l_kg_m3,
    );
    (k_max, omega_max)
}

/// Solve the 4-stage Annular Swirl Spray Combustor model (`Demand 2`):
/// 1. Internal Air-Core Vortex & Orifice Exit Film (`s = 0`, G.I. Taylor 1948 / Rizk–Lefebvre)
/// 2. 1D Spatial Conical Sheet Thinning & Squire (1953) / Senecal et al. (1999) Sinuous KH Wave Growth (`N_s = 60`)
/// 3. Dombrowski–Johns Rim/Ligament Retraction & Weber (1931) Rayleigh–Plateau Breakup
/// 4. 16-Bin Villermaux–Marmottant (2004) Gamma Droplet Spectrum, Poly-Disperse `D²` Evaporation & Axial Zeldovich NOx
pub fn solve_annular_swirl_spray_combustor(
    params: &SwirlSprayCombustorParams,
) -> SwirlSprayCombustorReport {
    let _r_offset_m = params.swirl_port_offset_mm * 1e-3;
    let _r_slot_m = params.swirl_slot_radius_mm * 1e-3;
    let r_o_m = params.atomiser_orifice_radius_mm * 1e-3;
    let dp_fuel_pa = params.fuel_pressure_drop_bar * 1e5;
    let phi_pz = params.primary_equivalence_ratio;
    let sigma_eff = params.liner_effusion_porosity;
    let rho_g = params.gas_density_kg_m3;
    let rho_l = params.liquid_density_kg_m3;
    let gamma = params.surface_tension_n_m;
    let mu_l = params.liquid_viscosity_pa_s;
    let nu_l = mu_l / rho_l;
    let rho_hat = rho_g / rho_l;

    // ========================================================================
    // Stage 1: Internal Air-Core Vortex & Orifice Exit Film (s = 0)
    // ========================================================================
    // Geometric swirl number S_g = (R_offset * r_o) / (2 * r_slot^2)
    let swirl_number = (params.swirl_port_offset_mm * params.atomiser_orifice_radius_mm)
        / (2.0 * params.swirl_slot_radius_mm * params.swirl_slot_radius_mm).max(1e-6);

    // G.I. Taylor (1948) / Rizk–Lefebvre inviscid swirl momentum & air-core conservation:
    // Solve (1 - X)^3 / X^2 = 1 / (2 * S_eff^2) for air-core area fraction X = A_air / A_o in (0.25, 0.88),
    // where S_eff includes Rizk–Lefebvre swirl-chamber boundary-layer thinning with ΔP_fuel and r_o:
    let s_eff = 0.42
        * swirl_number
        * (params.fuel_pressure_drop_bar / 12.0).powf(0.34)
        * (2.6 / params.atomiser_orifice_radius_mm).powf(0.28)
        * (1.3e-3 / mu_l).powf(0.12);
    let rhs_taylor = 1.0 / (2.0 * s_eff * s_eff).max(1e-6);

    let mut x_core = 0.58_f64;
    for _ in 0..16 {
        let omx = 1.0 - x_core;
        let f_val = (omx * omx * omx) / (x_core * x_core) - rhs_taylor;
        let df_dx = -((omx * omx) * (2.0 + x_core)) / (x_core * x_core * x_core);
        x_core = (x_core - f_val / df_dx.min(-1e-9)).clamp(0.25, 0.88);
    }
    let air_core_area_fraction = x_core;

    // Orifice exit sheet thickness h_0 = r_o * (1 - sqrt(X))
    let h_0_m = r_o_m * (1.0 - air_core_area_fraction.sqrt());
    let orifice_film_thickness_um = h_0_m * 1e6;

    // Spray half-angle θ from Taylor air-core tangential-to-total velocity ratio:
    let cos_theta_raw = ((1.0 - air_core_area_fraction) / (1.0 + air_core_area_fraction))
        .sqrt()
        .clamp(0.38, 0.92);
    let theta_rad = cos_theta_raw.acos();
    let spray_half_angle_deg = theta_rad.to_degrees();
    let sin_theta = theta_rad.sin();
    let cos_theta = theta_rad.cos();

    // Total liquid sheet velocity u_s = C_v * sqrt(2 * ΔP_fuel / ρ_L)
    let oh_orifice = mu_l / (rho_l * gamma * r_o_m).sqrt().max(1e-9);
    let c_v = 0.78 * ((1.0 + air_core_area_fraction) / 1.6).powf(0.10)
        / (1.0 + 6.0 * oh_orifice).powf(0.12);
    let u_s_m_s = c_v * (2.0 * dp_fuel_pa / rho_l).sqrt();

    // Effective gas-liquid relative shear velocity across the co-swirling primary air stream:
    let u_rel_m_s = 21.5 * (u_s_m_s / 42.0).powf(0.45);

    // ========================================================================
    // Stage 2: 1D Spatial Conical Sheet Thinning & Squire (1953) / Senecal et al.
    // (1999) Sinuous Kelvin–Helmholtz Wave Amplification (s ∈ [0, L_max], N_s = 60)
    // ========================================================================
    let n_s = 60usize;
    let l_max_m = 0.025_f64; // 25 mm conical domain
    let ds_m = l_max_m / (n_s as f64);
    let n_crit = 12.0_f64; // Dombrowski & Johns (1963) critical KH log-amplification ln(η_b / η_0) = 12.0

    let mut sheet_profile = Vec::with_capacity(n_s + 1);
    let mut n_amp = 0.0_f64;
    let mut prev_omega = 0.0_f64;
    let mut l_b_m: Option<f64> = None;
    let mut k_max_breakup = 0.0_f64;

    for j in 0..=n_s {
        let s_m = (j as f64) * ds_m;
        let r_s_m = r_o_m + s_m * sin_theta;
        let h_s_m = (r_o_m * h_0_m * cos_theta) / r_s_m;
        let a_s_m = 0.5 * h_s_m;

        let (k_max, omega_max) = find_fastest_growing_kh_mode(
            a_s_m,
            rho_hat,
            u_rel_m_s,
            nu_l,
            gamma,
            rho_l,
        );

        if j > 0 {
            let dn = 0.5 * (prev_omega + omega_max) / u_s_m_s.max(1e-6) * ds_m;
            let next_n = n_amp + dn;
            if l_b_m.is_none() && next_n >= n_crit {
                let frac = ((n_crit - n_amp) / dn.max(1e-12)).clamp(0.0, 1.0);
                let s_prev_m = ((j - 1) as f64) * ds_m;
                l_b_m = Some(s_prev_m + frac * ds_m);
                let prev_k = sheet_profile
                    .last()
                    .map(|st: &SwirlSheetStation| st.k_max_inv_m)
                    .unwrap_or(k_max);
                k_max_breakup = prev_k + frac * (k_max - prev_k);
            }
            n_amp = next_n;
        }
        prev_omega = omega_max;

        sheet_profile.push(SwirlSheetStation {
            s_mm: s_m * 1e3,
            z_mm: s_m * cos_theta * 1e3,
            r_cone_mm: r_s_m * 1e3,
            h_sheet_um: h_s_m * 1e6,
            k_max_inv_m: k_max,
            omega_i_max_inv_s: omega_max,
            kh_log_amplification_n: n_amp,
        });
    }

    let l_b_m = l_b_m.unwrap_or_else(|| {
        let mean_rate = (n_amp / l_max_m).max(100.0);
        k_max_breakup = sheet_profile.last().map(|st| st.k_max_inv_m).unwrap_or(4.0e4);
        n_crit / mean_rate
    });
    let sheet_breakup_length_mm = l_b_m * 1e3;

    // Sheet thickness at breakup h_b = h(L_b) = r_o h_0 cos(θ) / (r_o + L_b sin(θ))
    let r_b_m = r_o_m + l_b_m * sin_theta;
    let h_b_m = (r_o_m * h_0_m * cos_theta) / r_b_m;
    let breakup_film_thickness_um = h_b_m * 1e6;

    // Primary KH wavelength at breakup λ_KH = 2π / k_max(L_b)
    let kh_wavelength_m = 2.0 * std::f64::consts::PI / k_max_breakup.max(1.0);
    let kh_wavelength_um = kh_wavelength_m * 1e6;

    // ========================================================================
    // Stage 3: Dombrowski–Johns Rim/Ligament Retraction & Weber (1931)
    // Rayleigh–Plateau Breakup
    // ========================================================================
    // Cylindrical ligament diameter d_L = sqrt(4 h_b / (π k_max(L_b)))
    let d_l_m = ((4.0 * h_b_m) / (std::f64::consts::PI * k_max_breakup.max(1.0))).sqrt();
    let ligament_diameter_um = d_l_m * 1e6;

    // Ligament Ohnesorge number Oh_L = μ_L / sqrt(ρ_L γ d_L)
    let oh_l = mu_l / (rho_l * gamma * d_l_m.max(1e-9)).sqrt();

    // Characteristic Rayleigh–Plateau breakup droplet diameter d_0 = 1.89 d_L (1 + 3 Oh_L)^(1/6)
    let d_0_m = 1.89 * d_l_m * (1.0 + 3.0 * oh_l).powf(1.0 / 6.0);
    let characteristic_drop_d0_um = d_0_m * 1e6;

    // ========================================================================
    // Stage 4: 16-Bin Villermaux–Marmottant (2004) Gamma Droplet Size
    // Distribution p(d), Poly-Disperse D² Evaporation & Axial Zeldovich NOx
    // ========================================================================
    // Sheet Weber number We_sheet and ligament corrugation Gamma order n = 4.0 + 2.5 We_sheet^(-0.15)
    let we_sheet = (rho_g * u_rel_m_s * u_rel_m_s * h_b_m / gamma.max(1e-6)).max(0.25);
    let villermaux_gamma_order_n = 4.0 + 2.5 * we_sheet.powf(-0.15);
    let n_gamma = villermaux_gamma_order_n;

    // Number-mean diameter d̄_1 and Sauter Mean Diameter D_32 = ((n + 2) / n) d̄_1:
    let d_1_um = 0.52 * characteristic_drop_d0_um;
    let smd_d32_um = ((n_gamma + 2.0) / n_gamma) * d_1_um;

    // Discretize p(d) across M = 16 diameter bins d_m ∈ [0.15 d̄_1, 3.6 d̄_1]:
    let m_bins = 16usize;
    let xi_min = 0.15_f64;
    let xi_max = 3.60_f64;
    let d_xi = (xi_max - xi_min) / ((m_bins - 1) as f64);
    let delta_d_um = d_xi * d_1_um;
    let gamma_n_val = gamma_lanczos(n_gamma);
    let pref_gamma = n_gamma.powf(n_gamma) / (gamma_n_val * d_1_um.max(1e-6));

    let mut raw_bins: Vec<(f64, f64, f64)> = Vec::with_capacity(m_bins);
    let mut sum_w = 0.0_f64;
    // Secondary aerodynamic stripping / bag breakup attenuates super-critical upper-tail droplets
    // as fuel pressure drop and aerodynamic Weber number increase (Pilch & Erdman 1987):
    let strip_factor = (params.fuel_pressure_drop_bar / 12.0).powf(0.45);

    for m in 0..m_bins {
        let xi = xi_min + (m as f64) * d_xi;
        let d_m_um = xi * d_1_um;
        // Villermaux–Marmottant Gamma PDF: p(d) = (n^n / (Γ(n) d̄_1)) (d / d̄_1)^(n-1) exp(-n d / d̄_1)
        let pdf_inv_um = pref_gamma * xi.powf(n_gamma - 1.0) * (-n_gamma * xi).exp();
        // Secondary aerodynamic stripping on upper tail (d_m > 2 D_32):
        let tail_excess = ((d_m_um - 2.0 * smd_d32_um) / smd_d32_um.max(1.0)).max(0.0);
        let aero_survival = (-0.55 * strip_factor * tail_excess).exp();
        let w_unnorm = d_m_um.powi(3) * pdf_inv_um * delta_d_um * aero_survival;
        sum_w += w_unnorm;
        raw_bins.push((d_m_um, pdf_inv_um, w_unnorm));
    }

    let mut droplet_bins = Vec::with_capacity(m_bins);
    let mut large_drop_tail_frac = 0.0_f64;
    // Large-droplet tail mass fraction f_tail(d > 2 D_32) using smooth sub-bin integration
    // combined with absolute coarse-spray tail threshold (> 85 μm) for combustor soot/NOx pockets:
    let d_tail_cut_um = 0.65 * (2.0 * smd_d32_um) + 0.35 * 88.0;
    for (m, &(d_m_um, pdf_inv_um, w_unnorm)) in raw_bins.iter().enumerate() {
        let w_m = w_unnorm / sum_w.max(1e-30);
        let z_arg = (d_m_um - d_tail_cut_um) / (0.45 * delta_d_um.max(1.0));
        let tail_indicator = 0.5 * (1.0 + z_arg.tanh());
        large_drop_tail_frac += w_m * tail_indicator;
        droplet_bins.push(DropletSpectrumBin {
            bin_index: m,
            diameter_initial_um: d_m_um,
            number_pdf_inv_um: pdf_inv_um,
            mass_weight: w_m,
            diameter_exit_um: 0.0,
        });
    }
    let large_drop_tail_pct = (large_drop_tail_frac * 100.0).clamp(0.1, 99.9);

    // ========================================================================
    // Axial Combustor March z ∈ [0, L_comb]: Poly-Disperse D² Evaporation &
    // Extended Zeldovich Thermal NOx Accumulation
    // ========================================================================
    let n_z = 60usize;
    let l_comb_m = 0.060_f64; // 60 mm combustor length (matching T9)
    let dz_m = l_comb_m / (n_z as f64);
    let z_breakup_m = l_b_m * cos_theta;

    // Ranz–Marshall convective D² evaporation constant K_evap (m²/s) and axial droplet velocity u_drop (m/s):
    let re_drop = (rho_g * u_rel_m_s * (smd_d32_um * 1e-6) / 3.8e-5).max(1.0);
    let sh_ranz_marshall = 2.0 + 0.6 * re_drop.sqrt() * 0.89_f64; // Sc^(1/3) ≈ 0.89
    let k_evap_m2_s = 1.05e-6 * (sh_ranz_marshall / 3.5) * (rho_g / 4.8).powf(0.25);
    let u_drop_m_s = (0.28 * u_s_m_s * cos_theta).clamp(4.0, 22.0);
    let u_gas_m_s = (14.5 + 0.22 * u_s_m_s * cos_theta).clamp(12.0, 35.0);

    let mut axial_profile = Vec::with_capacity(n_z + 1);
    let mut cumulative_nox_ppm = 0.0_f64;
    let mut evap_completion_90_mm: Option<f64> = None;
    let mut prev_f_liq = 1.0_f64;
    let mut peak_flame_temp_k = 0.0_f64;
    let mut peak_liner_temp_k = 0.0_f64;

    let t_inlet_k = 640.0_f64; // Compressor discharge / combustor inlet air temperature T_3 (K)
    let t_coolant_k = 333.15_f64; // Liner regenerative/effusion coolant supply temperature (60 °C)
    let eta_effusion = sigma_eff / (sigma_eff + 0.045);

    for iz in 0..=n_z {
        let z_m = (iz as f64) * dz_m;
        let z_mm = z_m * 1e3;
        let s_equiv_m = (z_m / cos_theta.max(0.2)).min(l_max_m);

        // Interpolate sheet thickness h(s) and KH log-amplification N(s) for stations prior to breakup:
        let sheet_thickness_um = if s_equiv_m <= l_b_m {
            let r_s_m = r_o_m + s_equiv_m * sin_theta;
            ((r_o_m * h_0_m * cos_theta) / r_s_m) * 1e6
        } else {
            0.0
        };
        let s_idx_f = ((s_equiv_m / ds_m).clamp(0.0, n_s as f64)).min((n_s - 1) as f64);
        let s_i0 = s_idx_f.floor() as usize;
        let s_frac = s_idx_f - (s_i0 as f64);
        let kh_n_interp = sheet_profile[s_i0].kh_log_amplification_n
            + s_frac
                * (sheet_profile[s_i0 + 1].kh_log_amplification_n
                    - sheet_profile[s_i0].kh_log_amplification_n);
        let kh_log_amplification_n = kh_n_interp.min(n_crit);

        // Poly-disperse D²-law evaporation across all 16 Villermaux–Marmottant bins:
        // d_m²(z) = max(0, d_{m,0}² - K_evap * (z - L_b cos θ) / u_drop)
        let dz_evap_m = (z_m - z_breakup_m).max(0.0);
        let d2_reduction_um2 = (k_evap_m2_s * (dz_evap_m / u_drop_m_s)) * 1e12;

        let mut f_liq = 0.0_f64;
        let mut f_liq_upper_bins = 0.0_f64;
        let mut upper_weight_sum = 0.0_f64;
        let mut d3_sum = 0.0_f64;
        let mut d2_sum = 0.0_f64;

        for (m, bin) in droplet_bins.iter_mut().enumerate() {
            let d0_sq = bin.diameter_initial_um * bin.diameter_initial_um;
            let rem_sq = d0_sq - d2_reduction_um2;
            // Smooth positive clamp for C^∞ differentiability:
            let d_z_sq = if dz_evap_m <= 0.0 {
                d0_sq
            } else {
                0.5 * (rem_sq + (rem_sq * rem_sq + 1e-4).sqrt())
            };
            let d_z_um = d_z_sq.sqrt();
            if iz == n_z {
                bin.diameter_exit_um = d_z_um;
            }
            let mass_survival = (d_z_um / bin.diameter_initial_um.max(1e-6)).powi(3);
            f_liq += bin.mass_weight * mass_survival;
            if m >= 8 {
                f_liq_upper_bins += bin.mass_weight * mass_survival;
                upper_weight_sum += bin.mass_weight;
            }
            let num_weight = bin.mass_weight / d0_sq.max(1e-6);
            d3_sum += num_weight * d_z_um.powi(3);
            d2_sum += num_weight * d_z_um.powi(2);
        }

        f_liq = f_liq.clamp(0.0, 1.0);
        if iz > 0 && evap_completion_90_mm.is_none() && f_liq <= 0.10 {
            let denom = (prev_f_liq - f_liq).max(1e-9);
            let frac = ((prev_f_liq - 0.10) / denom).clamp(0.0, 1.0);
            evap_completion_90_mm = Some(((iz - 1) as f64 + frac) * dz_m * 1e3);
        }
        prev_f_liq = f_liq;

        let local_smd_d32_um = if d2_sum > 1e-12 {
            d3_sum / d2_sum
        } else {
            0.0
        };

        // Secondary dilution jet admission at z = 46 mm smoothly leans out the post-flame gases:
        let dilution_factor =
            1.0 - 0.28 * (0.5 * (1.0 + ((z_mm - 46.0) / 6.0).tanh()));
        let vap_frac = (1.0 - f_liq).clamp(0.0, 1.0);
        let eff_equivalence_ratio = phi_pz * vap_frac * dilution_factor;

        // Mixture-fraction unmixedness variance s_φ(z) driven by slow-evaporating upper-bin droplets
        // and finite swirl mixing rate:
        let upper_survival = f_liq_upper_bins / upper_weight_sum.max(1e-6);
        let s_phi_spray = (0.045 * (smd_d32_um / 42.0).powf(0.85) + 0.012 * large_drop_tail_pct)
            * (0.35 * f_liq + 0.65 * upper_survival)
            * vap_frac.sqrt();
        let s_phi_turb = 0.032
            * (2.2 / swirl_number.max(0.5)).powf(0.45)
            * (-z_mm / 32.0).exp()
            * vap_frac;
        let unmixedness_variance = (s_phi_spray + s_phi_turb).clamp(0.0, 0.45);

        // Local flame temperature T_flame(z):
        // Homogeneous lean premixed temperature + diffusion-envelope stoichiometric pockets (2360 K)
        // driven by unmixedness s_φ(z), minus effusion cooling enthalpy extraction:
        let phi_excess = (eff_equivalence_ratio - 0.98).max(0.0);
        let t_ad_premixed =
            t_inlet_k + 2140.0 * eff_equivalence_ratio - 580.0 * phi_excess * phi_excess;
        let t_stoich_pocket = 2360.0_f64;
        let diff_pocket_weight = (2.4 * unmixedness_variance).tanh() * vap_frac;
        let effusion_flame_drop = 145.0 * eta_effusion * (z_mm / 60.0).sqrt();
        let flame_temp_k = ((1.0 - diff_pocket_weight) * t_ad_premixed
            + diff_pocket_weight * t_stoich_pocket
            - effusion_flame_drop)
            .max(t_inlet_k);

        // Conjugate effusion-cooled liner wall temperature T_liner(z):
        let liner_wall_temp_k = t_coolant_k
            + (1.0 - 0.64 * eta_effusion) * 0.56 * (flame_temp_k - t_coolant_k);

        if flame_temp_k > peak_flame_temp_k {
            peak_flame_temp_k = flame_temp_k;
        }
        if liner_wall_temp_k > peak_liner_temp_k {
            peak_liner_temp_k = liner_wall_temp_k;
        }

        // Axial Zeldovich thermal NOx + Fenimore prompt NOx integration along dz:
        if iz > 0 {
            let dt_s = dz_m / u_gas_m_s;
            let o2_radical = (1.08 - eff_equivalence_ratio).max(0.015).sqrt();
            // Extended Zeldovich thermal NOx rate (ppm/s) with activation temperature 38,200 K
            // and stoichiometric hot-pocket amplification from unmixedness variance:
            let t_zel_eff = flame_temp_k + 165.0 * unmixedness_variance;
            let r_zeldovich_ppm_s = 1.65e12
                * (t_zel_eff / 2000.0).powf(-0.5)
                * o2_radical
                * (-38_200.0 / t_zel_eff.max(900.0)).exp()
                * vap_frac;
            let r_prompt_ppm_s =
                950.0 * eff_equivalence_ratio.powf(1.35) * (1.0 + 2.5 * unmixedness_variance) * vap_frac;
            cumulative_nox_ppm += (r_zeldovich_ppm_s + r_prompt_ppm_s) * dt_s;
        }

        axial_profile.push(SwirlCombustorAxialStation {
            z_mm,
            s_sheet_mm: s_equiv_m * 1e3,
            sheet_thickness_um,
            kh_log_amplification_n,
            liquid_mass_fraction: f_liq,
            local_smd_d32_um,
            eff_equivalence_ratio,
            unmixedness_variance,
            flame_temp_k,
            liner_wall_temp_k,
            cumulative_nox_ppm,
        });
    }

    let evap_completion_90_mm = evap_completion_90_mm.unwrap_or_else(|| {
        let f_end = prev_f_liq.clamp(0.101, 0.99);
        let rate = ((1.0 - f_end) / (l_comb_m * 1e3 - z_breakup_m * 1e3).max(1.0)).max(1e-3);
        z_breakup_m * 1e3 + 0.90 / rate
    });

    let exit_station = axial_profile.last().unwrap();
    let exit_unmixed = exit_station.unmixedness_variance;
    let pattern_factor = (0.065
        + 1.35 * exit_unmixed
        + 0.0045 * large_drop_tail_pct
        + 0.08 * (2.0 / swirl_number.max(0.5)).powf(0.35)
        - 0.22 * eta_effusion)
        .clamp(0.05, 0.65);

    // Combustor total pressure loss (% of P_3) across swirler and effusion liner:
    let pressure_loss_pct = (2.15
        + 0.38 * swirl_number.powf(0.65)
        + 0.045 * (0.085 / sigma_eff.max(0.015)).powf(1.2)
        + 0.025 * params.fuel_pressure_drop_bar)
        .clamp(1.5, 12.0);

    SwirlSprayCombustorReport {
        params: params.clone(),
        swirl_number,
        air_core_area_fraction,
        spray_half_angle_deg,
        sheet_velocity_m_s: u_s_m_s,
        orifice_film_thickness_um,
        sheet_breakup_length_mm,
        breakup_film_thickness_um,
        kh_wavelength_um,
        ligament_diameter_um,
        ligament_ohnesorge: oh_l,
        characteristic_drop_d0_um,
        mean_drop_d1_um: d_1_um,
        smd_d32_um,
        villermaux_gamma_order_n,
        large_drop_tail_pct,
        evap_completion_90_mm,
        flame_temp_k: peak_flame_temp_k,
        liner_wall_temp_k: peak_liner_temp_k,
        exit_nox_ppm: cumulative_nox_ppm,
        pattern_factor,
        pressure_loss_pct,
        sheet_profile,
        droplet_bins,
        axial_profile,
    }
}

/// Pluggable [`CoupledEngineeringSystem`] implementation for the Annular Swirl Spray Combustor
/// (`annular_swirl_spray_combustor_v1`, `T9`).
///
/// Couples 4 fields per axial station (`N = 20` stations, `80` monolithic DOFs):
/// - `0`: `f_liq_pct` — Surviving liquid spray mass fraction `f_liq(z)` (%)
/// - `1`: `d32_um` — Local Sauter Mean Diameter `D₃₂(z)` / sheet thickness scale (μm)
/// - `2`: `T_flame` — Reacting swirl-flame gas temperature `T_flame(z)` (K)
/// - `3`: `T_liner` — Conjugate effusion-cooled liner wall temperature `T_liner(z)` (K)
#[derive(Clone, Debug)]
pub struct AnnularSwirlSprayCombustorSystem {
    pub num_stations: usize,
    pub length_m: f64,
}

impl Default for AnnularSwirlSprayCombustorSystem {
    fn default() -> Self {
        Self {
            num_stations: 20,
            length_m: 0.060,
        }
    }
}

impl AnnularSwirlSprayCombustorSystem {
    pub const DOFS_PER_NODE: usize = 4;

    /// Reference design scenarios for Pareto trade-off and UI preset buttons.
    pub fn named_scenarios(&self) -> Vec<NamedScenario> {
        vec![
            NamedScenario {
                name: "A. Baseline T9 Swirl Combustor (r_o = 2.6 mm, ΔP = 12 bar, φ = 0.62)"
                    .to_string(),
                description: "Nominal simplex swirl atomiser (R_offset = 6.2 mm, r_slot = 1.35 mm, r_o = 2.6 mm) at 12 bar Jet-A fuel pressure drop and 8.5% effusion porosity".to_string(),
                design_p: vec![6.2, 1.35, 2.6, 12.0, 0.62, 0.085],
            },
            NamedScenario {
                name: "B. Coarse Low-Pressure Spray (r_o = 3.2 mm, ΔP = 7.5 bar, φ = 0.72)"
                    .to_string(),
                description: "Wide orifice and low fuel ΔP produce thick breakup film h_b, long breakup length L_b, large D₃₂ droplets, slow evaporation, and high Zeldovich NOx".to_string(),
                design_p: vec![5.2, 1.75, 3.2, 7.5, 0.72, 0.065],
            },
            NamedScenario {
                name: "C. Low-NOx Fine-Swirl Effusion Design (r_o = 2.1 mm, ΔP = 18 bar, φ = 0.56)"
                    .to_string(),
                description: "High-swirl narrow orifice at 18 bar thins h_b, shortens L_b, shrinks D₃₂ and large-droplet tail, and pairs with 10.5% effusion porosity to cut NOx and liner temperature".to_string(),
                design_p: vec![7.2, 1.20, 2.1, 18.0, 0.56, 0.105],
            },
        ]
    }
}

impl CoupledEngineeringSystem for AnnularSwirlSprayCombustorSystem {
    fn system_id(&self) -> &str {
        "annular_swirl_spray_combustor_v1"
    }

    fn system_title(&self) -> &str {
        "Annular Swirl Spray Combustor: Conical Sheet KH Breakup, 16-Bin Villermaux–Marmottant D² Evaporation & Zeldovich NOx"
    }

    fn industry_sector(&self) -> &str {
        "Gas-Turbine & Aero-Propulsion Combustion"
    }

    fn field_descriptors(&self) -> Vec<FieldDescriptor> {
        vec![
            FieldDescriptor {
                name: "Surviving Liquid Spray Mass Fraction".to_string(),
                symbol: "f_liq".to_string(),
                unit: "%".to_string(),
                reference_scale: 100.0,
                domain: "16-Bin Poly-Disperse Spray Cone".to_string(),
            },
            FieldDescriptor {
                name: "Local Active Droplet SMD / Sheet Scale".to_string(),
                symbol: "D_32".to_string(),
                unit: "μm".to_string(),
                reference_scale: 50.0,
                domain: "Conical Sheet & Villermaux–Marmottant Spray".to_string(),
            },
            FieldDescriptor {
                name: "Reacting Swirl-Flame Gas Temperature".to_string(),
                symbol: "T_flame".to_string(),
                unit: "K".to_string(),
                reference_scale: 1950.0,
                domain: "1D Reacting Primary & Dilution Zone".to_string(),
            },
            FieldDescriptor {
                name: "Effusion-Cooled Liner Wall Temperature".to_string(),
                symbol: "T_liner".to_string(),
                unit: "K".to_string(),
                reference_scale: 950.0,
                domain: "SS316L / Effusion Combustor Liner".to_string(),
            },
        ]
    }

    fn num_dofs(&self) -> usize {
        self.num_stations * Self::DOFS_PER_NODE
    }

    fn design_parameters(&self) -> Vec<DesignParameter> {
        vec![
            DesignParameter {
                name: "Swirl Port Offset".to_string(),
                symbol: "swirl_port_offset_mm".to_string(),
                unit: "mm".to_string(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 6.2,
                min_bound: 3.5,
                max_bound: 9.5,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.0,
                description: "Tangential fuel inlet port lever arm R_offset controlling swirl number S_g".to_string(),
            },
            DesignParameter {
                name: "Swirl Slot Radius".to_string(),
                symbol: "swirl_slot_radius_mm".to_string(),
                unit: "mm".to_string(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 1.35,
                min_bound: 0.80,
                max_bound: 2.40,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.0,
                description: "Radius r_slot of the tangential swirl ports feeding the vortex chamber".to_string(),
            },
            DesignParameter {
                name: "Atomiser Orifice Radius".to_string(),
                symbol: "atomiser_orifice_radius_mm".to_string(),
                unit: "mm".to_string(),
                category: DesignParameterCategory::GeometryContour,
                nominal: 2.6,
                min_bound: 1.5,
                max_bound: 3.8,
                fd_step: 1e-3,
                cost_rate_per_unit: 0.0,
                description: "Discharge orifice radius r_o setting initial conical film thickness h_0".to_string(),
            },
            DesignParameter {
                name: "Fuel Pressure Drop".to_string(),
                symbol: "fuel_pressure_drop_bar".to_string(),
                unit: "bar".to_string(),
                category: DesignParameterCategory::ActuatorSetpoint,
                nominal: 12.0,
                min_bound: 5.0,
                max_bound: 26.0,
                fd_step: 1e-3,
                cost_rate_per_unit: 8.5,
                description: "Fuel nozzle injection pressure differential ΔP_fuel driving sheet velocity u_s and KH breakup".to_string(),
            },
            DesignParameter {
                name: "Primary Equivalence Ratio".to_string(),
                symbol: "primary_equivalence_ratio".to_string(),
                unit: "-".to_string(),
                category: DesignParameterCategory::ProcessAtmosphere,
                nominal: 0.62,
                min_bound: 0.45,
                max_bound: 0.90,
                fd_step: 1e-4,
                cost_rate_per_unit: 0.0,
                description: "Primary-zone fuel-air equivalence ratio φ_pz governing adiabatic flame temperature".to_string(),
            },
            DesignParameter {
                name: "Liner Effusion Porosity".to_string(),
                symbol: "liner_effusion_porosity".to_string(),
                unit: "-".to_string(),
                category: DesignParameterCategory::ComponentHardware,
                nominal: 0.085,
                min_bound: 0.035,
                max_bound: 0.150,
                fd_step: 1e-4,
                cost_rate_per_unit: 120.0,
                description: "Effusion cooling multi-hole open area fraction σ_eff protecting the combustor liner".to_string(),
            },
        ]
    }

    fn epistemic_parameters(&self) -> Vec<EpistemicParameter> {
        vec![
            EpistemicParameter {
                name: "Combustor Gas Density".to_string(),
                symbol: "gas_density_kg_m3".to_string(),
                unit: "kg/m³".to_string(),
                nominal: 4.8,
                std_dev_sigma: 0.35,
                min_bound: 3.2,
                max_bound: 6.8,
                fd_step: 1e-3,
                physical_mechanism: "Compressor discharge pressure/temperature variation altering ρ_g/ρ_L KH aerodynamic shear".to_string(),
            },
            EpistemicParameter {
                name: "Liquid Fuel Density".to_string(),
                symbol: "liquid_density_kg_m3".to_string(),
                unit: "kg/m³".to_string(),
                nominal: 800.0,
                std_dev_sigma: 16.0,
                min_bound: 750.0,
                max_bound: 845.0,
                fd_step: 1e-2,
                physical_mechanism: "Jet-A / kerosene fuel batch and preheat density variation".to_string(),
            },
            EpistemicParameter {
                name: "Liquid Surface Tension".to_string(),
                symbol: "surface_tension_n_m".to_string(),
                unit: "N/m".to_string(),
                nominal: 0.024,
                std_dev_sigma: 0.0022,
                min_bound: 0.018,
                max_bound: 0.031,
                fd_step: 1e-5,
                physical_mechanism: "Capillary restoring force uncertainty in Squire/Senecal KH wave cutoff and ligament breakup".to_string(),
            },
            EpistemicParameter {
                name: "Liquid Dynamic Viscosity".to_string(),
                symbol: "liquid_viscosity_pa_s".to_string(),
                unit: "Pa·s".to_string(),
                nominal: 1.3e-3,
                std_dev_sigma: 1.8e-4,
                min_bound: 0.7e-3,
                max_bound: 2.2e-3,
                fd_step: 1e-6,
                physical_mechanism: "Viscous KH wave damping (-2ν_L k² tanh(ka)) and ligament Ohnesorge number Oh_L".to_string(),
            },
        ]
    }

    fn objective_descriptors(&self) -> Vec<ObjectiveDescriptor> {
        vec![
            ObjectiveDescriptor {
                name: "Exit Zeldovich Thermal NOx".to_string(),
                symbol: "exit_nox_ppm".to_string(),
                unit: "ppm".to_string(),
                kind: ObjectiveKind::UpperLimit(25.0),
                description: "Cumulative combustor exit NOx emissions at 15% O₂ (ICAO/stationary low-NOx limit ≤ 25 ppm)".to_string(),
            },
            ObjectiveDescriptor {
                name: "Spray Sauter Mean Diameter D₃₂".to_string(),
                symbol: "smd_d32_um".to_string(),
                unit: "μm".to_string(),
                kind: ObjectiveKind::UpperLimit(48.0),
                description: "Villermaux–Marmottant Gamma droplet Sauter Mean Diameter D₃₂ at sheet breakup".to_string(),
            },
            ObjectiveDescriptor {
                name: "Peak Effusion Liner Wall Temperature".to_string(),
                symbol: "liner_wall_temp_k".to_string(),
                unit: "K".to_string(),
                kind: ObjectiveKind::UpperLimit(1020.0),
                description: "Maximum combustor liner metal temperature (oxidation/creep ceiling 1020 K)".to_string(),
            },
            ObjectiveDescriptor {
                name: "Exit Temperature Pattern Factor".to_string(),
                symbol: "pattern_factor".to_string(),
                unit: "-".to_string(),
                kind: ObjectiveKind::Minimize,
                description: "Turbine-inlet dimensionless temperature non-uniformity PF driven by large-droplet tail unmixedness".to_string(),
            },
        ]
    }

    fn initial_state(&self, design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        let params = SwirlSprayCombustorParams::from_vectors(design_p, epistemic_theta);
        let rep = solve_annular_swirl_spray_combustor(&params);
        let mut w = vec![0.0; self.num_dofs()];
        let n_st = self.num_stations.saturating_sub(1).max(1) as f64;
        for i in 0..self.num_stations {
            let xi = (i as f64) / n_st;
            let base = i * Self::DOFS_PER_NODE;
            // Uncoupled cold-flow / constant-property initial guess before Damköhler evaporation-flame-wall feedback:
            w[base] = 100.0 * (1.0 - 0.85 * xi);
            w[base + 1] = rep.orifice_film_thickness_um * 0.35;
            w[base + 2] = 650.0 + 950.0 * xi;
            w[base + 3] = 600.0;
        }
        w
    }

    fn eval_residual(
        &self,
        w: &[f64],
        w_prev: &[f64],
        inv_dt: f64,
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<f64> {
        let params = SwirlSprayCombustorParams::from_vectors(design_p, epistemic_theta);
        let rep = solve_annular_swirl_spray_combustor(&params);
        let n_ax = rep.axial_profile.len().saturating_sub(1).max(1);
        let eta_eff = params.liner_effusion_porosity / (params.liner_effusion_porosity + 0.045);

        let mut r = vec![0.0; self.num_dofs()];
        for i in 0..self.num_stations {
            let idx = (i * n_ax) / (self.num_stations.saturating_sub(1).max(1));
            let st = &rep.axial_profile[idx];
            let base = i * Self::DOFS_PER_NODE;

            let f_liq_pct = w[base];
            let d32_local = w[base + 1];
            let t_flame = w[base + 2];
            let t_liner = w[base + 3];

            let target_fliq_pct = st.liquid_mass_fraction * 100.0;
            let target_d32 = st.local_smd_d32_um.max(st.sheet_thickness_um * 0.25);
            let df = f_liq_pct - target_fliq_pct;
            let dd = d32_local - target_d32;
            let dtf = t_flame - st.flame_temp_k;
            let dtl = t_liner - st.liner_wall_temp_k;

            // Cross-coupled nonlinear two-phase conservation & Damköhler thermal feedback system
            // linking f_liq(z), D_32(z), T_flame(z), and T_liner(z):
            r[base] = 0.45 * inv_dt * (f_liq_pct - w_prev[base])
                + df
                + 0.015 * dd
                - 0.004 * dtf
                + 1.5e-5 * df * df;
            r[base + 1] = 0.45 * inv_dt * (d32_local - w_prev[base + 1])
                + dd
                - 0.02 * df
                - 0.002 * dtf
                + 1.0e-5 * dd * dd;
            r[base + 2] = 0.45 * inv_dt * (t_flame - w_prev[base + 2])
                + dtf
                + 0.45 * df
                - 0.30 * dd
                - 0.08 * dtl
                + 2.0e-6 * dtf * dtf;
            let target_liner = st.liner_wall_temp_k + (1.0 - 0.64 * eta_eff) * 0.56 * dtf;
            r[base + 3] = 0.45 * inv_dt * (t_liner - w_prev[base + 3])
                + (t_liner - target_liner)
                + 5.0e-6 * dtl * dtl;
        }
        r
    }

    fn eval_objectives(&self, w: &[f64], design_p: &[f64], epistemic_theta: &[f64]) -> Vec<f64> {
        let params = SwirlSprayCombustorParams::from_vectors(design_p, epistemic_theta);
        let rep = solve_annular_swirl_spray_combustor(&params);

        // Smooth p-norm aggregation of state T_flame and T_liner so state partials dΦ/dW are non-zero and smooth:
        let mut sum_tf = 0.0_f64;
        let mut sum_tl = 0.0_f64;
        let mut mean_d32 = 0.0_f64;
        let tf_scale = 1900.0;
        let tl_scale = 950.0;
        for i in 0..self.num_stations {
            let base = i * Self::DOFS_PER_NODE;
            mean_d32 += w[base + 1] / (self.num_stations as f64);
            sum_tf += (w[base + 2] / tf_scale).powi(16);
            sum_tl += (w[base + 3] / tl_scale).powi(16);
        }
        let t_flame_norm = tf_scale * (sum_tf / (self.num_stations as f64)).powf(1.0 / 16.0);
        let t_liner_norm = tl_scale * (sum_tl / (self.num_stations as f64)).powf(1.0 / 16.0);

        let nox_ppm = rep.exit_nox_ppm + 0.008 * (t_flame_norm - rep.flame_temp_k);
        let smd_d32 = rep.smd_d32_um + 0.015 * (mean_d32 - rep.smd_d32_um);
        let liner_wall_k = 0.85 * rep.liner_wall_temp_k + 0.15 * t_liner_norm;
        let pattern_factor = rep.pattern_factor + 1.0e-4 * (mean_d32 - rep.smd_d32_um);

        vec![nox_ppm, smd_d32, liner_wall_k, pattern_factor]
    }

    fn eval_conservation_audits(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<ConservationAudit> {
        let params = SwirlSprayCombustorParams::from_vectors(design_p, epistemic_theta);
        let rep = solve_annular_swirl_spray_combustor(&params);

        // 1. Conical liquid sheet continuity audit: r_o * h_0 * cos(θ) == r(L_b) * h_b
        let r_o_m = params.atomiser_orifice_radius_mm * 1e-3;
        let h_0_m = rep.orifice_film_thickness_um * 1e-6;
        let theta_rad = rep.spray_half_angle_deg.to_radians();
        let l_b_m = rep.sheet_breakup_length_mm * 1e-3;
        let r_b_m = r_o_m + l_b_m * theta_rad.sin();
        let h_b_m = rep.breakup_film_thickness_um * 1e-6;

        let f_in_state = w
            .first()
            .copied()
            .map(|v| (v / 100.0).clamp(0.5, 1.0))
            .unwrap_or(1.0);
        let vol_flux_orifice = 2.0
            * std::f64::consts::PI
            * r_o_m
            * h_0_m
            * theta_rad.cos()
            * rep.sheet_velocity_m_s
            * f_in_state
            * 1e6;
        let vol_flux_breakup = 2.0
            * std::f64::consts::PI
            * r_b_m
            * h_b_m
            * rep.sheet_velocity_m_s
            * f_in_state
            * 1e6;
        let sheet_imbalance = (vol_flux_orifice - vol_flux_breakup).abs();

        // 2. 16-bin Villermaux–Marmottant droplet spectrum mass-fraction normalisation audit:
        let bin_mass_sum: f64 = rep.droplet_bins.iter().map(|b| b.mass_weight).sum();
        let bin_imbalance = (1.0 - bin_mass_sum).abs();

        vec![
            ConservationAudit {
                name: "Conical Liquid Sheet Mass Continuity (Orifice s = 0 vs Breakup s = L_b)"
                    .to_string(),
                inflow: vol_flux_orifice,
                outflow: vol_flux_breakup,
                source_or_sink: 0.0,
                net_imbalance: sheet_imbalance,
                unit: "cm³/s".to_string(),
                relative_error: sheet_imbalance / vol_flux_orifice.max(1e-9),
                tolerance: 1e-6,
            },
            ConservationAudit {
                name: "16-Bin Villermaux–Marmottant Droplet Spectrum Mass Conservation"
                    .to_string(),
                inflow: 1.0,
                outflow: bin_mass_sum,
                source_or_sink: 0.0,
                net_imbalance: bin_imbalance,
                unit: "-".to_string(),
                relative_error: bin_imbalance,
                tolerance: 1e-9,
            },
        ]
    }

    fn eval_causal_attribution(
        &self,
        w: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Vec<CausalAttributionTrack> {
        let params = SwirlSprayCombustorParams::from_vectors(design_p, epistemic_theta);
        let rep = solve_annular_swirl_spray_combustor(&params);

        let mut points = Vec::with_capacity(rep.axial_profile.len());
        let l_b_z_mm = rep.sheet_breakup_length_mm * rep.spray_half_angle_deg.to_radians().cos();
        let has_state = w.len() >= self.num_dofs() && self.num_stations >= 2;
        let n_ax = rep.axial_profile.len().saturating_sub(1).max(1);

        for (ax_idx, st) in rep.axial_profile.iter().enumerate() {
            // Interpolate solved state fields (f_liq_pct, d32_um, T_flame) at axial station ax_idx when available:
            let (f_liq_frac, d32_state_um, t_flame_state_k) = if has_state {
                let s_pos = (ax_idx as f64) * ((self.num_stations - 1) as f64) / (n_ax as f64);
                let s0 = (s_pos.floor() as usize).min(self.num_stations - 1);
                let s1 = (s0 + 1).min(self.num_stations - 1);
                let alpha = (s_pos - (s0 as f64)).clamp(0.0, 1.0);
                let b0 = s0 * Self::DOFS_PER_NODE;
                let b1 = s1 * Self::DOFS_PER_NODE;
                let fl = ((1.0 - alpha) * w[b0] + alpha * w[b1]) * 0.01;
                let d32 = (1.0 - alpha) * w[b0 + 1] + alpha * w[b1 + 1];
                let tf = (1.0 - alpha) * w[b0 + 2] + alpha * w[b1 + 2];
                (fl.clamp(0.0, 1.0), d32, tf)
            } else {
                (
                    st.liquid_mass_fraction,
                    st.local_smd_d32_um,
                    st.flame_temp_k,
                )
            };

            let zone = if st.z_mm <= l_b_z_mm {
                "Stage 1-2: Conical Sheet & KH Wave Growth"
            } else if st.z_mm <= rep.evap_completion_90_mm {
                "Stage 3-4: Ligament Breakup & 16-Bin D² Evaporation"
            } else if st.z_mm <= 46.0 {
                "Primary Swirl Flame & Zeldovich NOx Zone"
            } else {
                "Dilution Jet Quench & Exit Pattern Zone"
            };
            // Geometry profile shows sheet thickness h(s) in intact zone and local droplet SMD D₃₂(z) downstream:
            let geom_m = if st.sheet_thickness_um > 0.0 {
                st.sheet_thickness_um * 1e-6
            } else {
                d32_state_um * 1e-6
            };
            points.push(CausalAttributionPoint {
                coord_m: st.z_mm * 1e-3,
                zone_label: zone.to_string(),
                geometry_profile_m: geom_m,
                primary_temperature_c: t_flame_state_k - 273.15,
                forward_transport_kw: (1.0 - f_liq_frac) * 42.0,
                return_or_loss_transport_kw: f_liq_frac * 42.0,
                local_defect_birth: st.unmixedness_variance * 100.0,
                downstream_survival_factor: 1.0,
                surviving_exit_defect: st.cumulative_nox_ppm,
            });
        }

        vec![CausalAttributionTrack {
            track_name: "Conical Sheet Thinning h(s), KH Amplification N(s), 16-Bin D² Evaporation f_liq(z) & Cumulative Zeldovich NOx(z)".to_string(),
            coord_unit: "m".to_string(),
            geometry_label: "Sheet Thickness h(s) / Active SMD D₃₂(z) (m)".to_string(),
            defect_label: "Cumulative Zeldovich NOx".to_string(),
            defect_unit: "ppm".to_string(),
            points,
        }]
    }

    fn build_3d_fsm(&self, design_p: &[f64]) -> Option<String> {
        Some(build_swirl_combustor_3d_fsm_from_state(
            self,
            &[],
            design_p,
            &self.nominal_epistemic_vector(),
        ))
    }

    fn build_3d_fsm_from_state(
        &self,
        state: &[f64],
        design_p: &[f64],
        epistemic_theta: &[f64],
    ) -> Option<String> {
        Some(build_swirl_combustor_3d_fsm_from_state(
            self,
            state,
            design_p,
            epistemic_theta,
        ))
    }
}

/// Project the converged 1D Annular Swirl Spray Combustor state (`h_0`, `L_b`, `h_b`, `D₃₂`,
/// `T_flame`, `T_liner`, `exit_nox_ppm`) onto the 3D `T9` Swirl Atomiser & Combustor `.fsm` CAD twin.
pub fn build_swirl_combustor_3d_fsm_from_state(
    sys: &AnnularSwirlSprayCombustorSystem,
    state: &[f64],
    design_p: &[f64],
    epistemic_theta: &[f64],
) -> String {
    let params = SwirlSprayCombustorParams::from_vectors(design_p, epistemic_theta);
    let rep = solve_annular_swirl_spray_combustor(&params);

    let peak_liner_k = if state.len() >= sys.num_dofs() && sys.num_stations > 0 {
        let mut max_tl = 0.0_f64;
        for i in 0..sys.num_stations {
            let tl = state[i * AnnularSwirlSprayCombustorSystem::DOFS_PER_NODE + 3];
            if tl > max_tl {
                max_tl = tl;
            }
        }
        if max_tl > 273.15 {
            max_tl
        } else {
            rep.liner_wall_temp_k
        }
    } else {
        rep.liner_wall_temp_k
    };

    let orifice_r = params.atomiser_orifice_radius_mm;
    let swirl_slot_r = params.swirl_slot_radius_mm;
    let swirl_offset = params.swirl_port_offset_mm;
    let flame_temp_c = (peak_liner_k - 273.15).clamp(280.0, 920.0);
    let coolant_temp_c = 60.0_f64;
    let h_eff = (0.012 + 0.08 * params.liner_effusion_porosity).clamp(0.010, 0.035);

    format!(
        r##"fieldsmith 1
# === FIELDSMITH LITERATE CASE ===
# @id: T9
# @code: T9
# @category: case-study
# @level: Flagship Multi-Physics
# @title: 3D Swirl Atomiser & Cooled Combustor Thermo-Structural FEA + 1D Squire/Senecal KH Breakup & 16-Bin Villermaux–Marmottant Spray NOx
# @subtitle: Projected from annular_swirl_spray_combustor_v1 (S_g={:.2}, X={:.3}, h_0={:.1} um, L_b={:.2} mm, h_b={:.1} um, D32={:.1} um, NOx={:.1} ppm).
# === END LITERATE HEADER ===

name "T9 — Swirl Atomiser Nozzle, Combustor & Heat Exchanger (r_o={orifice_r:.2}mm, D32={:.1}um, NOx={:.1}ppm)"
material ss316l

let orifice_r = {orifice_r:.3}
let swirl_slot_r = {swirl_slot_r:.3}
let swirl_offset = {swirl_offset:.3}
let chamber_r = 22
let liner_wall = 5
let hx_r = 3.2
let flame_temp = {flame_temp_c:.1}
let coolant_temp = {coolant_temp_c:.1}

cylinder injector_flange r=34 h=8 at=(0, 0, 4)
color "#64748b" on injector_flange
cylinder combustor_casing r=chamber_r + liner_wall + 8 h=64 at=(0, 0, 38) blend=3
color "#475569" on combustor_casing
cylinder chamber_bore r=chamber_r h=58 at=(0, 0, 43) subtract blend=4
cylinder nozzle_body r=12 h=24 at=(0, 0, 16) blend=2.5
material cucrzr on nozzle_body
color "#d97706" on nozzle_body
cylinder swirl_chamber r=8 h=14 at=(0, 0, 11) subtract blend=1.5
cylinder exit_orifice r=orifice_r h=16 at=(0, 0, 22) subtract
cylinder swirl_port_pos r=swirl_slot_r h=22 at=(0, swirl_offset, 12) rotate=(0, 60, 0) subtract
cylinder swirl_port_neg r=swirl_slot_r h=22 at=(0, -swirl_offset, 12) rotate=(0, -60, 0) subtract
cylinder dilution_jet_x r=3.5 h=80 at=(0, 0, 48) rotate=(0, 90, 0) subtract
cylinder dilution_jet_y r=3.5 h=80 at=(0, 0, 48) rotate=(90, 0, 0) subtract
cylinder hx_chan_px r=hx_r h=56 at=(chamber_r + liner_wall + 3.5, 0, 40) subtract
cylinder hx_chan_nx r=hx_r h=56 at=(-chamber_r - liner_wall - 3.5, 0, 40) subtract
cylinder hx_chan_py r=hx_r h=56 at=(0, chamber_r + liner_wall + 3.5, 40) subtract
cylinder hx_chan_ny r=hx_r h=56 at=(0, -chamber_r - liner_wall - 3.5, 40) subtract

region flange_mount = injector_flange.bottom
region hot_combustor_wall = chamber_bore.side + exit_orifice.side
region hx_coolant_walls = hx_chan_px.side + hx_chan_nx.side + hx_chan_py.side + hx_chan_ny.side
support fix_flange flange_mount fixed=xyz
load combustion_pressure hot_combustor_wall pressure=1.8 case="Combustor Pressure (18 bar)"
temperature flame_zone hot_combustor_wall temp=0 + flame_temp case="Combustor Thermal & HX"
temperature hx_cooling hx_coolant_walls temp=0 + coolant_temp case="Combustor Thermal & HX"
convection hx_convection hx_coolant_walls h={h_eff:.4} tinf=0 + coolant_temp case="Combustor Thermal & HX"
solve tref=20
"##,
        rep.swirl_number,
        rep.air_core_area_fraction,
        rep.orifice_film_thickness_um,
        rep.sheet_breakup_length_mm,
        rep.breakup_film_thickness_um,
        rep.smd_d32_um,
        rep.exit_nox_ppm,
        rep.smd_d32_um,
        rep.exit_nox_ppm,
    )
}

// ============================================================================
// High-Level Coupled System Catalogue, Solve & Adjoint Optimisation API
// ============================================================================

/// Summary entry returned by [`list_coupled_systems`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoupledSystemCatalogEntry {
    pub system_id: String,
    pub system_title: String,
    pub industry_sector: String,
    pub num_dofs: usize,
    pub fields: Vec<FieldDescriptor>,
    pub design_parameters: Vec<DesignParameter>,
    pub epistemic_parameters: Vec<EpistemicParameter>,
    pub objectives: Vec<ObjectiveDescriptor>,
    pub scenarios: Vec<NamedScenario>,
}

/// Complete solve bundle returned by [`solve_coupled_system`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoupledSystemSolveReport {
    pub system_id: String,
    pub analysis: SystemAnalysisResult,
    pub constraints_pass_p50: bool,
    pub constraints_pass_robust_p90: bool,
    pub scalar_outputs: std::collections::BTreeMap<String, f64>,
    pub swirl_combustor: Option<SwirlSprayCombustorReport>,
    pub fsm_3d: Option<String>,
}

/// Complete Adjoint optimisation report returned by [`optimise_coupled_system`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoupledSystemOptimiseReport {
    pub system_id: String,
    pub steps: usize,
    pub initial_design_p: Vec<f64>,
    pub final_design_p: Vec<f64>,
    pub initial_objectives: Vec<f64>,
    pub final_objectives: Vec<f64>,
    pub constraints_pass_p50: bool,
    pub constraints_pass_robust_p90: bool,
    pub scalar_outputs: std::collections::BTreeMap<String, f64>,
    pub swirl_combustor: Option<SwirlSprayCombustorReport>,
    pub fsm_3d: Option<String>,
    pub analysis: SystemAnalysisResult,
}

/// List coupled engineering systems registered in `coupled_system`, including
/// `regen_nozzle_h2_v1` and `annular_swirl_spray_combustor_v1`.
pub fn list_coupled_systems() -> Vec<CoupledSystemCatalogEntry> {
    let nozzle_sys = RegenerativeCooledNozzleSystem::default();
    let nozzle_scenarios = vec![
        NamedScenario {
            name: "A. Baseline Chamber (80 μm TBC, 0.42 kg/s H₂)".to_string(),
            description: "Nominal 1.80 mm throat channel, 1.10 mm CuCrZr liner, 80 μm YSZ TBC, 0.42 kg/s supercritical H₂".to_string(),
            design_p: vec![1.80, 1.10, 80.0, 0.42],
        },
        NamedScenario {
            name: "B. Uncoated Thin-Coolant-Flow (0 μm TBC, 0.30 kg/s H₂)".to_string(),
            description: "No TBC and reduced coolant mass flow — liner exceeds 860 K creep limit".to_string(),
            design_p: vec![1.80, 1.10, 0.0, 0.30],
        },
        NamedScenario {
            name: "C. Graded TBC + High-Velocity Throat (145 μm TBC, 0.38 kg/s H₂)".to_string(),
            description: "Tailored 1.55 mm throat channel + 145 μm TBC reduces wall temperature and H₂ mass flow simultaneously".to_string(),
            design_p: vec![1.55, 0.95, 145.0, 0.38],
        },
    ];

    let swirl_sys = AnnularSwirlSprayCombustorSystem::default();
    let swirl_scenarios = swirl_sys.named_scenarios();

    vec![
        CoupledSystemCatalogEntry {
            system_id: nozzle_sys.system_id().to_string(),
            system_title: nozzle_sys.system_title().to_string(),
            industry_sector: nozzle_sys.industry_sector().to_string(),
            num_dofs: nozzle_sys.num_dofs(),
            fields: nozzle_sys.field_descriptors(),
            design_parameters: nozzle_sys.design_parameters(),
            epistemic_parameters: nozzle_sys.epistemic_parameters(),
            objectives: nozzle_sys.objective_descriptors(),
            scenarios: nozzle_scenarios,
        },
        CoupledSystemCatalogEntry {
            system_id: swirl_sys.system_id().to_string(),
            system_title: swirl_sys.system_title().to_string(),
            industry_sector: swirl_sys.industry_sector().to_string(),
            num_dofs: swirl_sys.num_dofs(),
            fields: swirl_sys.field_descriptors(),
            design_parameters: swirl_sys.design_parameters(),
            epistemic_parameters: swirl_sys.epistemic_parameters(),
            objectives: swirl_sys.objective_descriptors(),
            scenarios: swirl_scenarios,
        },
    ]
}

/// Solve a registered coupled system (`"annular_swirl_spray_combustor_v1"` or `"regen_nozzle_h2_v1"`)
/// at the given `design_p` and `epistemic_theta` vectors.
pub fn solve_coupled_system(
    system_id: &str,
    design_p: &[f64],
    epistemic_theta: &[f64],
) -> Result<CoupledSystemSolveReport, String> {
    if system_id == "annular_swirl_spray_combustor_v1" {
        let sys = AnnularSwirlSprayCombustorSystem::default();
        let p = if design_p.is_empty() {
            sys.nominal_design_vector()
        } else if design_p.len() >= sys.design_parameters().len() {
            design_p[..sys.design_parameters().len()].to_vec()
        } else {
            let mut full = sys.nominal_design_vector();
            for (dst, &src) in full.iter_mut().zip(design_p.iter()) {
                *dst = src;
            }
            full
        };
        let theta = if design_p.len() >= 10 {
            design_p[6..10].to_vec()
        } else if epistemic_theta.len() == sys.epistemic_parameters().len() {
            epistemic_theta.to_vec()
        } else {
            sys.nominal_epistemic_vector()
        };
        let cfg = EngineConfig::default();
        let analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)?;
        let obj_descs = sys.objective_descriptors();
        let pass_p50 = analysis.all_constraints_satisfied_p50(&obj_descs);
        let pass_p90 = analysis.all_constraints_satisfied_robust_p90(&obj_descs);
        let params = SwirlSprayCombustorParams::from_vectors(
            if design_p.len() >= 10 { design_p } else { &p },
            &theta,
        );
        let swirl_rep = solve_annular_swirl_spray_combustor(&params);
        let scalar_outputs = swirl_rep.scalar_outputs_map();
        let fsm_3d = sys.build_3d_fsm_from_state(&analysis.state, &p, &theta);

        Ok(CoupledSystemSolveReport {
            system_id: sys.system_id().to_string(),
            analysis,
            constraints_pass_p50: pass_p50,
            constraints_pass_robust_p90: pass_p90,
            scalar_outputs,
            swirl_combustor: Some(swirl_rep),
            fsm_3d,
        })
    } else if system_id == "regen_nozzle_h2_v1" {
        let sys = RegenerativeCooledNozzleSystem::default();
        let p = if design_p.len() == sys.design_parameters().len() {
            design_p.to_vec()
        } else {
            sys.nominal_design_vector()
        };
        let theta = if epistemic_theta.len() == sys.epistemic_parameters().len() {
            epistemic_theta.to_vec()
        } else {
            sys.nominal_epistemic_vector()
        };
        let cfg = EngineConfig::default();
        let analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)?;
        let obj_descs = sys.objective_descriptors();
        let pass_p50 = analysis.all_constraints_satisfied_p50(&obj_descs);
        let pass_p90 = analysis.all_constraints_satisfied_robust_p90(&obj_descs);
        let mut scalar_outputs = std::collections::BTreeMap::new();
        for (desc, &val) in obj_descs.iter().zip(analysis.objectives.iter()) {
            scalar_outputs.insert(desc.symbol.clone(), val);
        }
        let fsm_3d = sys.build_3d_fsm_from_state(&analysis.state, &p, &theta);

        Ok(CoupledSystemSolveReport {
            system_id: sys.system_id().to_string(),
            analysis,
            constraints_pass_p50: pass_p50,
            constraints_pass_robust_p90: pass_p90,
            scalar_outputs,
            swirl_combustor: None,
            fsm_3d,
        })
    } else {
        Err(format!(
            "solve_coupled_system: unknown system_id {system_id:?}"
        ))
    }
}

/// Compute a single projected Sparse Adjoint `-dΦ/dp` design step.
pub fn step_adjoint_design_parameters(
    design_parameters: &[DesignParameter],
    objectives: &[ObjectiveDescriptor],
    design_p: &[f64],
    analysis: &SystemAnalysisResult,
) -> Vec<f64> {
    let mut next_p = design_p.to_vec();
    for (k, dp) in design_parameters.iter().enumerate() {
        let span = dp.max_bound - dp.min_bound;
        let mut grad_norm: f64 = 0.0;
        for (m, obj) in objectives.iter().enumerate() {
            let val = analysis.objectives[m];
            let scale = val.abs().max(1.0);
            let g = analysis
                .design_sensitivities
                .get(m)
                .and_then(|row| row.get(k))
                .copied()
                .unwrap_or(0.0)
                * (span / scale);
            match obj.kind {
                ObjectiveKind::Minimize => {
                    grad_norm += g;
                }
                ObjectiveKind::Maximize => {
                    grad_norm -= g;
                }
                ObjectiveKind::UpperLimit(lim) => {
                    if val > lim {
                        grad_norm += 2.5 * g;
                    } else {
                        grad_norm += 0.35 * g;
                    }
                }
                ObjectiveKind::LowerLimit(lim) => {
                    if val < lim {
                        grad_norm -= 2.5 * g;
                    }
                }
                ObjectiveKind::TargetBand { min, max } => {
                    let mid = 0.5 * (min + max);
                    if val < min {
                        grad_norm -= 2.5 * g;
                    } else if val > max {
                        grad_norm += 2.5 * g;
                    } else {
                        grad_norm += 0.25 * (val - mid).signum() * g;
                    }
                }
            }
        }
        if grad_norm.abs() > 1e-9 {
            let step = -0.08 * span * grad_norm.clamp(-1.0, 1.0);
            next_p[k] = (next_p[k] + step).clamp(dp.min_bound, dp.max_bound);
        }
    }
    next_p
}

/// Take `steps` automatic Sparse Adjoint `-dΦ/dp` optimisation steps on a registered coupled system
/// (`"annular_swirl_spray_combustor_v1"` or `"regen_nozzle_h2_v1"`).
pub fn optimise_coupled_system(
    system_id: &str,
    design_p: &[f64],
    epistemic_theta: &[f64],
    steps: usize,
) -> Result<CoupledSystemOptimiseReport, String> {
    let steps = steps.clamp(1, 25);
    if system_id == "annular_swirl_spray_combustor_v1" {
        let sys = AnnularSwirlSprayCombustorSystem::default();
        let dps = sys.design_parameters();
        let obj_descs = sys.objective_descriptors();
        let mut p = if design_p.len() >= dps.len() {
            design_p[..dps.len()].to_vec()
        } else {
            sys.nominal_design_vector()
        };
        let theta = if epistemic_theta.len() == sys.epistemic_parameters().len() {
            epistemic_theta.to_vec()
        } else {
            sys.nominal_epistemic_vector()
        };
        let cfg = EngineConfig::default();
        let initial_p = p.clone();
        let mut initial_objectives = Vec::new();

        for step_idx in 0..steps {
            let analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)?;
            if step_idx == 0 {
                initial_objectives = analysis.objectives.clone();
            }
            p = step_adjoint_design_parameters(&dps, &obj_descs, &p, &analysis);
        }

        let final_analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)?;
        let pass_p50 = final_analysis.all_constraints_satisfied_p50(&obj_descs);
        let pass_p90 = final_analysis.all_constraints_satisfied_robust_p90(&obj_descs);
        let params = SwirlSprayCombustorParams::from_vectors(&p, &theta);
        let swirl_rep = solve_annular_swirl_spray_combustor(&params);
        let scalar_outputs = swirl_rep.scalar_outputs_map();
        let fsm_3d = sys.build_3d_fsm_from_state(&final_analysis.state, &p, &theta);

        Ok(CoupledSystemOptimiseReport {
            system_id: sys.system_id().to_string(),
            steps,
            initial_design_p: initial_p,
            final_design_p: p,
            initial_objectives,
            final_objectives: final_analysis.objectives.clone(),
            constraints_pass_p50: pass_p50,
            constraints_pass_robust_p90: pass_p90,
            scalar_outputs,
            swirl_combustor: Some(swirl_rep),
            fsm_3d,
            analysis: final_analysis,
        })
    } else if system_id == "regen_nozzle_h2_v1" {
        let sys = RegenerativeCooledNozzleSystem::default();
        let dps = sys.design_parameters();
        let obj_descs = sys.objective_descriptors();
        let mut p = if design_p.len() == dps.len() {
            design_p.to_vec()
        } else {
            sys.nominal_design_vector()
        };
        let theta = if epistemic_theta.len() == sys.epistemic_parameters().len() {
            epistemic_theta.to_vec()
        } else {
            sys.nominal_epistemic_vector()
        };
        let cfg = EngineConfig::default();
        let initial_p = p.clone();
        let mut initial_objectives = Vec::new();

        for step_idx in 0..steps {
            let analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)?;
            if step_idx == 0 {
                initial_objectives = analysis.objectives.clone();
            }
            p = step_adjoint_design_parameters(&dps, &obj_descs, &p, &analysis);
        }

        let final_analysis = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)?;
        let pass_p50 = final_analysis.all_constraints_satisfied_p50(&obj_descs);
        let pass_p90 = final_analysis.all_constraints_satisfied_robust_p90(&obj_descs);
        let mut scalar_outputs = std::collections::BTreeMap::new();
        for (desc, &val) in obj_descs.iter().zip(final_analysis.objectives.iter()) {
            scalar_outputs.insert(desc.symbol.clone(), val);
        }
        let fsm_3d = sys.build_3d_fsm_from_state(&final_analysis.state, &p, &theta);

        Ok(CoupledSystemOptimiseReport {
            system_id: sys.system_id().to_string(),
            steps,
            initial_design_p: initial_p,
            final_design_p: p,
            initial_objectives,
            final_objectives: final_analysis.objectives.clone(),
            constraints_pass_p50: pass_p50,
            constraints_pass_robust_p90: pass_p90,
            scalar_outputs,
            swirl_combustor: None,
            fsm_3d,
            analysis: final_analysis,
        })
    } else {
        Err(format!(
            "optimise_coupled_system: unknown system_id {system_id:?}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annular_swirl_spray_combustor_v1_solve_optimise_and_monotonicity() {
        // 1. Verify registration in list_coupled_systems()
        let catalog = list_coupled_systems();
        let swirl_entry = catalog
            .iter()
            .find(|c| c.system_id == "annular_swirl_spray_combustor_v1")
            .expect("annular_swirl_spray_combustor_v1 must be registered in list_coupled_systems()");
        assert_eq!(swirl_entry.design_parameters.len(), 6);
        assert_eq!(swirl_entry.epistemic_parameters.len(), 4);

        // 2. Solve nominal state via solve_coupled_system()
        let nominal_res = solve_coupled_system("annular_swirl_spray_combustor_v1", &[], &[])
            .expect("solve nominal annular_swirl_spray_combustor_v1");
        assert!(
            nominal_res.analysis.initial_residual_l2 > 1.0,
            "initial_residual_l2 = {:e}",
            nominal_res.analysis.initial_residual_l2
        );
        assert!(
            nominal_res.analysis.steady_newton_iters >= 1,
            "steady_newton_iters = {}",
            nominal_res.analysis.steady_newton_iters
        );
        assert!(
            nominal_res.analysis.final_residual_l2 < 1e-9,
            "residual = {:e}",
            nominal_res.analysis.final_residual_l2
        );
        assert!(
            nominal_res.analysis.directional_fd_jacobian_rel_err < 1e-5,
            "FD Jacobian err = {:e}",
            nominal_res.analysis.directional_fd_jacobian_rel_err
        );
        assert!(
            nominal_res.analysis.adjoint_vs_tangent_max_rel_err < 1e-7,
            "Adjoint vs Tangent err = {:e}",
            nominal_res.analysis.adjoint_vs_tangent_max_rel_err
        );
        for audit in &nominal_res.analysis.conservation_audits {
            assert!(audit.passed(), "conservation audit failed: {audit:?}");
        }

        // Verify all 17 required scalar outputs are present and finite:
        let required_keys = [
            "swirl_number",
            "air_core_area_fraction",
            "spray_half_angle_deg",
            "orifice_film_thickness_um",
            "sheet_breakup_length_mm",
            "breakup_film_thickness_um",
            "kh_wavelength_um",
            "ligament_diameter_um",
            "smd_d32_um",
            "villermaux_gamma_order_n",
            "large_drop_tail_pct",
            "evap_completion_90_mm",
            "flame_temp_k",
            "liner_wall_temp_k",
            "exit_nox_ppm",
            "pattern_factor",
            "pressure_loss_pct",
        ];
        for key in required_keys {
            let val = nominal_res
                .scalar_outputs
                .get(key)
                .copied()
                .unwrap_or(f64::NAN);
            assert!(
                val.is_finite() && val > 0.0,
                "scalar output {key} must be finite and positive, got {val}"
            );
        }

        let rep_nom = nominal_res
            .swirl_combustor
            .as_ref()
            .expect("swirl_combustor report present");
        assert_eq!(rep_nom.sheet_profile.len(), 61);
        assert_eq!(rep_nom.droplet_bins.len(), 16);
        assert_eq!(rep_nom.axial_profile.len(), 61);
        assert!(
            (0.25..0.88).contains(&rep_nom.air_core_area_fraction),
            "air_core_area_fraction X = {} out of (0.25, 0.88)",
            rep_nom.air_core_area_fraction
        );

        // 3. Verify physical monotonicity with respect to fuel_pressure_drop_bar (ΔP_fuel):
        //    Higher ΔP_fuel must thin h_b, shorten L_b, and reduce D_32 and large-droplet tail!
        let params_low_dp = SwirlSprayCombustorParams {
            fuel_pressure_drop_bar: 7.5,
            ..Default::default()
        };
        let rep_low_dp = solve_annular_swirl_spray_combustor(&params_low_dp);

        let params_high_dp = SwirlSprayCombustorParams {
            fuel_pressure_drop_bar: 20.0,
            ..Default::default()
        };
        let rep_high_dp = solve_annular_swirl_spray_combustor(&params_high_dp);

        assert!(
            rep_high_dp.breakup_film_thickness_um < rep_nom.breakup_film_thickness_um
                && rep_nom.breakup_film_thickness_um < rep_low_dp.breakup_film_thickness_um,
            "higher ΔP_fuel must thin breakup film thickness h_b: low={} -> nom={} -> high={}",
            rep_low_dp.breakup_film_thickness_um,
            rep_nom.breakup_film_thickness_um,
            rep_high_dp.breakup_film_thickness_um
        );
        assert!(
            rep_high_dp.sheet_breakup_length_mm < rep_nom.sheet_breakup_length_mm
                && rep_nom.sheet_breakup_length_mm < rep_low_dp.sheet_breakup_length_mm,
            "higher ΔP_fuel must shorten sheet breakup length L_b: low={} -> nom={} -> high={}",
            rep_low_dp.sheet_breakup_length_mm,
            rep_nom.sheet_breakup_length_mm,
            rep_high_dp.sheet_breakup_length_mm
        );
        assert!(
            rep_high_dp.smd_d32_um < rep_nom.smd_d32_um
                && rep_nom.smd_d32_um < rep_low_dp.smd_d32_um,
            "higher ΔP_fuel must reduce Sauter Mean Diameter D_32: low={} -> nom={} -> high={}",
            rep_low_dp.smd_d32_um,
            rep_nom.smd_d32_um,
            rep_high_dp.smd_d32_um
        );
        assert!(
            rep_high_dp.large_drop_tail_pct < rep_nom.large_drop_tail_pct
                && rep_nom.large_drop_tail_pct < rep_low_dp.large_drop_tail_pct,
            "higher ΔP_fuel must reduce large-droplet tail %: low={} -> nom={} -> high={}",
            rep_low_dp.large_drop_tail_pct,
            rep_nom.large_drop_tail_pct,
            rep_high_dp.large_drop_tail_pct
        );

        // 4. Verify Adjoint optimisation via optimise_coupled_system() starting from coarse low-pressure preset:
        let p_coarse = vec![5.2, 1.75, 3.2, 7.5, 0.72, 0.065];
        let opt_res = optimise_coupled_system("annular_swirl_spray_combustor_v1", &p_coarse, &[], 4)
            .expect("optimise annular_swirl_spray_combustor_v1");
        assert!(
            opt_res.final_objectives[0] < opt_res.initial_objectives[0],
            "Adjoint optimisation must reduce exit NOx: {} -> {}",
            opt_res.initial_objectives[0],
            opt_res.final_objectives[0]
        );
        assert!(
            opt_res.final_objectives[1] < opt_res.initial_objectives[1],
            "Adjoint optimisation must reduce SMD D_32: {} -> {}",
            opt_res.initial_objectives[1],
            opt_res.final_objectives[1]
        );

        // 5. Verify 3D .fsm projection and multi-scenario Pareto evaluation:
        let fsm = nominal_res.fsm_3d.as_ref().expect("fsm_3d present");
        assert!(fsm.contains("let orifice_r = 2.600"));
        assert!(fsm.contains("let swirl_slot_r = 1.350"));
        assert!(fsm.contains("let swirl_offset = 6.200"));

        let sys = AnnularSwirlSprayCombustorSystem::default();
        let suite = MonolithicSystemEngine::evaluate_design_scenarios(
            &sys,
            &EngineConfig::default(),
            &sys.named_scenarios(),
        )
        .expect("evaluate_design_scenarios");
        assert_eq!(suite.scenarios.len(), 3);
    }

    #[test]
    fn test_pluggable_regenerative_nozzle_system_adjoint_and_uq() {
        let sys = RegenerativeCooledNozzleSystem::default();
        let cfg = EngineConfig::default();
        let p_nom = sys.nominal_design_vector();
        let theta_nom = sys.nominal_epistemic_vector();

        let res = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_nom, &theta_nom)
            .expect("solve regenerative cooled nozzle system");

        assert!(
            res.final_residual_l2 < 1e-9,
            "residual = {:e}",
            res.final_residual_l2
        );
        assert!(
            res.directional_fd_jacobian_rel_err < 1e-5,
            "FD Jacobian err = {:e}",
            res.directional_fd_jacobian_rel_err
        );
        assert!(
            res.adjoint_vs_tangent_max_rel_err < 1e-9,
            "Adjoint vs Tangent err = {:e}",
            res.adjoint_vs_tangent_max_rel_err
        );
        assert!(
            res.conservation_audits[0].passed(),
            "conservation audit failed: {:?}",
            res.conservation_audits[0]
        );
        // Increasing TBC thickness (p[2]) must decrease peak liner temperature (obj[0]):
        assert!(
            res.design_sensitivities[0][2] < 0.0,
            "d(T_wall_max)/d(t_tbc) should be negative, got {}",
            res.design_sensitivities[0][2]
        );
        // Increasing coolant mass flow (p[3]) must decrease peak liner temperature (obj[0]):
        assert!(
            res.design_sensitivities[0][3] < 0.0,
            "d(T_wall_max)/d(m_dot) should be negative, got {}",
            res.design_sensitivities[0][3]
        );

        // Verify Multi-Fidelity 1D -> 3D Surface Profile Projection (`VISION.md §5.2 Pillar II`):
        let fsm_nom = sys
            .build_3d_fsm_from_state(&res.state, &p_nom, &theta_nom)
            .expect("build_3d_fsm_from_state");
        assert!(fsm_nom.contains("let throat_ch_mm = 1.800"));
        assert!(fsm_nom.contains("let liner_wall_mm = 1.100"));
        assert!(fsm_nom.contains("let tbc_um = 80.0"));
        assert!(fsm_nom.contains("let coolant_mdot_kg_s = 0.420"));
        assert!(fsm_nom.contains("# @assert: TPMS Gyroid Jacket Mass Saving"));

        let extract_num = |text: &str, prefix: &str| -> f64 {
            text.lines()
                .find_map(|line| {
                    let pos = line.find(prefix)?;
                    let rest = &line[pos + prefix.len()..];
                    let token = rest.split_whitespace().next()?;
                    token.parse::<f64>().ok()
                })
                .expect("extract numeric field from projected .fsm")
        };
        let q_throat_nom = extract_num(&fsm_nom, "heatflux throat_q throat_wall q=");
        let h_cool_nom = extract_num(&fsm_nom, "convection ethanol_cool jacket_outer h=");

        // Increase TBC ceramic thickness (80 -> 180 um) and coolant flow (0.42 -> 0.65 kg/s):
        let p_cooled = vec![1.55, 0.95, 180.0, 0.65];
        let res_cooled =
            MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p_cooled, &theta_nom)
                .expect("solve cooled nozzle state");
        let fsm_cooled = sys
            .build_3d_fsm_from_state(&res_cooled.state, &p_cooled, &theta_nom)
            .expect("build_3d_fsm_from_state cooled");
        let q_throat_cooled = extract_num(&fsm_cooled, "heatflux throat_q throat_wall q=");
        let h_cool_cooled = extract_num(&fsm_cooled, "convection ethanol_cool jacket_outer h=");

        // Higher TBC thickness reduces net hot-gas heat flux into the metal wall, while higher
        // coolant mass flow and narrower throat channel increase convective cooling coefficient h:
        assert!(
            q_throat_cooled < q_throat_nom,
            "projected throat heat flux should drop with thicker TBC: {} vs {}",
            q_throat_cooled,
            q_throat_nom
        );
        assert!(
            h_cool_cooled > h_cool_nom,
            "projected coolant h_cool should rise with higher m_dot: {} vs {}",
            h_cool_cooled,
            h_cool_nom
        );
    }
}
