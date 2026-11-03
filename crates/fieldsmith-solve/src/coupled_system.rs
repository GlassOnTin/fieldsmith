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
        Some(build_regen_nozzle_3d_fsm(design_p))
    }
}

/// Build a parametric 3D `.fsm` CAD + thermo-mechanical FEA model of the Supercritical-H₂
/// Regeneratively Cooled Rocket Thrust Chamber, Throat Venturi Liner, and Cryogenic Feed Manifold.
pub fn build_regen_nozzle_3d_fsm(design_p: &[f64]) -> String {
    let h_throat_mm = design_p.first().copied().unwrap_or(1.80);
    let t_wall_mm = design_p.get(1).copied().unwrap_or(1.10);
    let t_tbc_um = design_p.get(2).copied().unwrap_or(80.0);
    let mdot_h2 = design_p.get(3).copied().unwrap_or(2.60);
    let p_inlet_mpa = design_p.get(4).copied().unwrap_or(16.5);

    let outer_r_mm = (34.0 + h_throat_mm * 1.5).round().clamp(35.0, 40.0) as i32;
    let liner_r_mm = (24.0 + t_wall_mm * 2.0).round().clamp(25.0, 29.0) as i32;
    let throat_hot_c = (610.0 - t_tbc_um * 0.9 - (mdot_h2 - 2.0) * 35.0)
        .round()
        .clamp(380.0, 680.0) as i32;
    let thrust_n = (-2200.0 * p_inlet_mpa).round() as i32;

    format!(
        r##"fieldsmith 1
# Supercritical-H2 Regeneratively Cooled Rocket Thrust Chamber & CuCrZr TBC Liner (3D Twin)
# Synthesised from Coupled Systems Workbench:
#   h_throat = {h_throat_mm:.2} mm | t_wall = {t_wall_mm:.2} mm | t_tbc = {t_tbc_um:.0} um | mdot_H2 = {mdot_h2:.2} kg/s | p_in = {p_inlet_mpa:.1} MPa
name "Supercritical-H2 Rocket Nozzle (h_throat={h_throat_mm:.2}mm, t_tbc={t_tbc_um:.0}um)"
material ss316l

let jacket_r = {outer_r_mm}
let liner_r = {liner_r_mm}

# Injector-end structural gimbal mounting plate (z = 0..14 mm, isolated from fillets)
box mounting_flange size=(92, 92, 14) at=(0, 0, 7)
color "#475569" on mounting_flange

# Inconel/SS316L structural regenerative cooling jacket
cylinder cooling_jacket r=jacket_r h=86 at=(0, 0, 55)
color "#64748b" on cooling_jacket

# Toroidal Supercritical-H2 Throat Cooling Manifold Ring (h_throat = {h_throat_mm:.2} mm)
torus throat_manifold major=jacket_r minor=6 at=(0, 0, 54) blend=2
color "#38bdf8" on throat_manifold

# Cryogenic H2 Inlet Feed Boss (+X) & Collector Boss (-X)
box h2_inlet_boss size=(26, 22, 20) at=(jacket_r + 6, 0, 82)
box h2_outlet_boss size=(26, 22, 20) at=(-jacket_r - 6, 0, 26)
color "#0ea5e9" on h2_inlet_boss
color "#f59e0b" on h2_outlet_boss

# Nozzle Exit Thrust & Thermal Load Carrier Flange (z = 94..106 mm)
box exit_collar size=(76, 76, 12) at=(0, 0, 100)
color "#d97706" on exit_collar

# CuCrZr Hot-Gas Inner Liner & Blind Convergent-Divergent Bore (z = 16..110 mm)
cylinder cucrzr_liner r=liner_r h=88 at=(0, 0, 56)
color "#d97706" on cucrzr_liner
cylinder chamber_bore r=18 h=94 at=(0, 0, 63) subtract

# Sonic Throat Venturi Constriction Ring (x_throat = 0.28 m)
torus throat_venturi major=21 minor=6 at=(0, 0, 54) blend=3
color "#ef4444" on throat_venturi

region flange_base = mounting_flange.nz
region nozzle_exit_rim = exit_collar.pz
region h2_inlet_face = h2_inlet_boss.px

support fix_flange flange_base fixed=xyz
load chamber_thrust nozzle_exit_rim force=(0, 0, {thrust_n}) case="Chamber Thrust & Pressure"
temperature throat_heat nozzle_exit_rim temp={throat_hot_c}
temperature cryo_h2 h2_inlet_face temp=40
solve tref=20
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
