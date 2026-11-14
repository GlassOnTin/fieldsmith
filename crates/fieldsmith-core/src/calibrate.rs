//! Adjoint Gauss–Newton / Levenberg–Marquardt Bayesian Maximum-A-Posteriori (MAP) calibration
//! and Laplace posterior uncertainty synthesis (`QuestionSpec::Calibrate`, `VISION.md §5.2 V.1`).
//!
//! Fuses empirical bench / test-stand readings (`doc.observations`, `observe "..." metric="..."
//! measured=<y> std=<σ_y>`) with Gaussian parameter priors (`doc.uncertainties`, `system`
//! `epistemic` closures, or explicit `CalibrateSpec::priors`) by solving:
//!
//! `min_θ  ½ ∑_i ((M_i(θ) − y_i) / σ_obs,i)²  +  ½ ∑_j ((θ_j − θ_{0,j}) / σ_prior,j)²`
//!
//! in dimensionless prior-sigma coordinates `z_j = (θ_j − θ_{0,j}) / σ_prior,j` using `O(1)`-solve
//! 3D discrete adjoint sensitivities `H_{i,j} = ∂M_i / ∂θ_j` on a locked P2 tetrahedral mesh
//! spacing `h`. At convergence, inverts the standardized posterior precision matrix
//! `Λ_z = I_n + Aᵀ A` (`A_{i,j} = (σ_prior,j / σ_obs,i) H_{i,j}`) to obtain the Laplace posterior
//! covariance `Σ_post = diag(σ_prior) Λ_z⁻¹ diag(σ_prior)` and writes calibrated parameter means
//! plus narrowed marginal posterior standard deviations back into a canonical `.fsm` document.

use crate::contracts::{
    evaluate_contracts_with_uq, is_graded_assert_metric, required_modes_for_asserts,
    resolve_assert_metric, VerificationReport,
};
use crate::declarative_system::resolve_system_doc;
use crate::fill::{self, Graded};
use crate::optimise::{
    apply_fill_opt_param, default_fill_param_bounds, default_solve_param_bounds, is_fill_opt_addr,
    metric_derivative_for_param,
};
use crate::sensitivity::{
    apply_sens_param, compute_sensitivities_graded_with_base, compute_sensitivities_with_base,
    default_target_params, is_solve_param_target, read_param_value, resolve_material_param_id,
    SensitivityReport,
};
use crate::uq::{
    collect_effective_uncertainties, default_metric_unit, strip_quantile_prefix, Z_90,
};
use crate::{analyse_full, Analysis, Clock, Resolution, SetupError};
use fieldsmith_doc::fsm;
use fieldsmith_doc::param::ParamAddr;
use fieldsmith_doc::{Document, ObserveSpec, UncertaintySpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Specification for [`calibrate_document`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CalibrateSpec {
    /// Parameters to calibrate; when empty, defaults to parameters declared in
    /// `doc.uncertainties` + `doc.system_block` `epistemic` closures (or `default_target_params`
    /// with a weakly-informative `cv = 0.25` prior if no uncertainties are declared).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    /// Optional explicit bench observations; when empty, uses `doc.observations`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<ObserveSpec>,
    /// Optional explicit Gaussian priors overriding `doc.uncertainties`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub priors: Vec<UncertaintySpec>,
    /// Optional `[min, max]` physical box bounds per calibrated parameter.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bounds: BTreeMap<String, [f64; 2]>,
    /// Maximum Gauss–Newton / Levenberg–Marquardt outer iterations (`1..=25`, default `6`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iters: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dofs: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
}

/// Prior vs Laplace-posterior summary for one calibrated parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibratedParamSummary {
    pub param: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    pub prior_mean: f64,
    pub prior_sigma: f64,
    pub posterior_mean: f64,
    pub posterior_sigma: f64,
    pub posterior_p10: f64,
    pub posterior_p90: f64,
    /// Percentage reduction in 1-σ uncertainty `100 · (1 − σ_post / σ_prior)`.
    pub sigma_reduction_pct: f64,
    /// Standardized MAP shift `(θ_MAP − θ_prior) / σ_prior`.
    pub shift_in_prior_sigmas: f64,
    pub lower_bound: f64,
    pub upper_bound: f64,
}

/// Prior vs MAP-posterior fit summary for one empirical bench sensor observation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservationFitSummary {
    pub label: String,
    pub metric: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    pub measured: f64,
    pub obs_sigma: f64,
    pub prior_prediction: f64,
    pub posterior_prediction: f64,
    /// Propagated 1-σ predictive uncertainty under the prior `sqrt(h_iᵀ Σ_prior h_i)`.
    pub prior_pred_sigma: f64,
    /// Propagated 1-σ predictive uncertainty under the full correlated Laplace posterior
    /// `sqrt(h_iᵀ Σ_post h_i)`.
    pub posterior_pred_sigma: f64,
    /// Standardized MAP observation residual `(M_i(θ_MAP) − y_i) / σ_obs,i`.
    pub normalized_residual_z: f64,
}

/// Per-iteration Gauss–Newton / Levenberg–Marquardt trace entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibrationIteration {
    pub iter: usize,
    pub map_cost: f64,
    pub chi2_obs: f64,
    pub chi2_prior: f64,
    pub damping_lambda: f64,
    pub step_norm_z: f64,
    pub params: BTreeMap<String, f64>,
}

/// Complete Bayesian MAP calibration & Laplace posterior report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibrationReport {
    pub method: String,
    pub sensitivity_method: String,
    pub iterations: usize,
    pub solves_run: usize,
    pub converged: bool,
    pub h: f64,
    pub initial_cost: f64,
    pub final_cost: f64,
    pub chi2_obs: f64,
    pub chi2_prior: f64,
    /// Bayesian joint reduced chi-square `(χ²_obs + χ²_prior) / m`, which has exact expectation
    /// `1.0` under a well-specified linear-Gaussian prior + likelihood model.
    pub reduced_chi2: f64,
    pub parameters: Vec<CalibratedParamSummary>,
    pub observations: Vec<ObservationFitSummary>,
    /// Full `n × n` Laplace posterior correlation matrix `ρ_{j,k} ∈ [-1, 1]`.
    pub posterior_correlation: Vec<Vec<f64>>,
    pub history: Vec<CalibrationIteration>,
    pub initial_verification: VerificationReport,
    pub final_verification: VerificationReport,
    pub calibrated_fsm: String,
    pub elapsed_ms: f64,
}

#[derive(Clone, Debug)]
struct CalParamMeta {
    name: String,
    label: String,
    unit: String,
    addr: ParamAddr,
    prior_mean: f64,
    prior_sigma: f64,
    lo: f64,
    hi: f64,
}

impl CalParamMeta {
    #[inline]
    fn theta_from_z(&self, z: f64) -> f64 {
        (self.prior_mean + self.prior_sigma * z).clamp(self.lo, self.hi)
    }

    #[inline]
    fn z_lo(&self) -> f64 {
        (self.lo - self.prior_mean) / self.prior_sigma
    }

    #[inline]
    fn z_hi(&self) -> f64 {
        (self.hi - self.prior_mean) / self.prior_sigma
    }
}

struct CalState {
    doc: Document,
    sys_doc: Document,
    analysis: Analysis,
    graded: Option<Graded>,
    predictions: Vec<f64>,
    residuals_r: Vec<f64>,
    chi2_obs: f64,
    chi2_prior: f64,
    map_cost: f64,
    primal_solves: usize,
}

fn clean_sig12(v: f64) -> f64 {
    if !v.is_finite() || v == 0.0 {
        return 0.0;
    }
    format!("{v:.11e}").parse::<f64>().unwrap_or(v)
}

fn is_strictly_positive_param(doc: &Document, addr: &ParamAddr) -> bool {
    if resolve_material_param_id(doc, addr).is_some() {
        return matches!(
            addr.attr.as_str(),
            "e" | "young"
                | "yield"
                | "yield_strength"
                | "yield_z"
                | "shear_z"
                | "density"
                | "rho"
                | "k"
                | "conductivity"
        );
    }
    if doc.convections.iter().any(|c| c.id == addr.id) && addr.attr == "h" {
        return true;
    }
    if doc.contacts.iter().any(|c| c.id == addr.id) && addr.attr == "k" {
        return true;
    }
    if doc.find(&addr.id).is_some() {
        return matches!(
            addr.attr.as_str(),
            "size" | "radius" | "r" | "height" | "h" | "length" | "l" | "major" | "minor"
        );
    }
    if addr.id == "hx" && doc.hx.is_some() && matches!(addr.attr.as_str(), "mdot" | "cold_mdot") {
        return true;
    }
    false
}

fn build_cal_param_metas(
    doc: &Document,
    spec: &CalibrateSpec,
) -> Result<Vec<CalParamMeta>, SetupError> {
    let eff_priors = collect_effective_uncertainties(doc, &spec.priors);

    let mut names: Vec<String> = if !spec.params.is_empty() {
        spec.params.iter().map(|s| s.trim().to_string()).collect()
    } else if !eff_priors.is_empty() {
        let mut out = Vec::new();
        for u in &eff_priors {
            let p = u.param.trim();
            if !p.is_empty() && !out.iter().any(|x| x == p) {
                out.push(p.to_string());
            }
        }
        out
    } else {
        default_target_params(doc, &[])
    };
    names.retain(|s| !s.is_empty());

    if names.is_empty() {
        return Err(SetupError(
            "calibrate: document declares no uncertain or tunable parameters \
             (add `uncertainty \"...\" param=\"...\" std=...` or pass `params`)"
                .into(),
        ));
    }

    let mut metas = Vec::with_capacity(names.len());
    for name in &names {
        let addr = ParamAddr::parse(name).map_err(SetupError)?;
        let prior_mean = read_param_value(doc, &addr).map_err(SetupError)?;
        if !prior_mean.is_finite() {
            return Err(SetupError(format!(
                "calibrate: parameter '{name}' has non-finite prior value {prior_mean}"
            )));
        }

        let matched_u = eff_priors.iter().find(|u| u.param.trim() == name.as_str());
        let prior_sigma = match matched_u {
            Some(u) => {
                let s = u.effective_sigma(prior_mean);
                if s <= 0.0 || !s.is_finite() {
                    return Err(SetupError(format!(
                        "calibrate: prior uncertainty on '{name}' resolved to non-positive σ = {s}"
                    )));
                }
                s
            }
            None => (0.25 * prior_mean.abs()).max(1e-3),
        };

        let label = matched_u
            .map(|u| u.label.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| name.clone());
        let unit = matched_u.map(|u| u.unit.clone()).unwrap_or_default();

        let (mut lo, mut hi) = if let Some(&[b_lo, b_hi]) = spec.bounds.get(name) {
            if !b_lo.is_finite() || !b_hi.is_finite() || b_lo >= b_hi {
                return Err(SetupError(format!(
                    "calibrate: invalid bounds [{b_lo}, {b_hi}] for '{name}' (expected finite min < max)"
                )));
            }
            (b_lo, b_hi)
        } else if let Some(ep) = doc
            .system_block
            .as_ref()
            .and_then(|sb| sb.epistemics.iter().find(|e| &e.name == name))
        {
            (ep.min, ep.max)
        } else if let Some(dp) = doc
            .system_block
            .as_ref()
            .and_then(|sb| sb.designs.iter().find(|d| &d.name == name))
        {
            (dp.min, dp.max)
        } else if is_fill_opt_addr(doc, &addr) {
            default_fill_param_bounds(&addr, prior_mean)
        } else if is_solve_param_target(doc, &addr) {
            default_solve_param_bounds(&addr, prior_mean)
        } else if resolve_material_param_id(doc, &addr).is_some()
            && matches!(addr.attr.as_str(), "nu" | "poisson")
        {
            (0.01_f64.min(prior_mean), 0.495_f64.max(prior_mean))
        } else if is_strictly_positive_param(doc, &addr) {
            let floor = (0.02 * prior_mean.abs()).max(1e-9);
            let lo_cand = (prior_mean - 6.0 * prior_sigma)
                .min(0.1 * prior_mean)
                .max(floor);
            let hi_cand = (prior_mean + 6.0 * prior_sigma).max(6.0 * prior_mean);
            (lo_cand, hi_cand)
        } else if prior_mean > 0.0 && addr.attr.is_empty() {
            // For positive `let`s, test whether `0` or negative values are valid on the document:
            // geometry lets fail validation <= 0, whereas signed lets (temperature/load) succeed.
            let mut probe_doc = doc.clone();
            let zero_ok = apply_sens_param(&mut probe_doc, &addr, 0.0).is_ok()
                && probe_doc.validate().is_empty();
            if zero_ok {
                (
                    prior_mean - 6.0 * prior_sigma,
                    prior_mean + 6.0 * prior_sigma,
                )
            } else {
                let floor = (0.05 * prior_mean).max(1e-6);
                let lo_cand = (prior_mean - 6.0 * prior_sigma)
                    .min(0.15 * prior_mean)
                    .max(floor);
                let hi_cand = (prior_mean + 6.0 * prior_sigma).max(5.0 * prior_mean);
                (lo_cand, hi_cand)
            }
        } else {
            (
                prior_mean - 6.0 * prior_sigma,
                prior_mean + 6.0 * prior_sigma,
            )
        };

        if prior_mean < lo {
            lo = prior_mean;
        }
        if prior_mean > hi {
            hi = prior_mean;
        }
        if (hi - lo).abs() < 1e-15 {
            hi = lo + prior_sigma.max(1e-6);
        }

        metas.push(CalParamMeta {
            name: name.clone(),
            label,
            unit,
            addr,
            prior_mean,
            prior_sigma,
            lo,
            hi,
        });
    }
    Ok(metas)
}

fn apply_parameter_vector(
    base_doc: &Document,
    metas: &[CalParamMeta],
    z: &[f64],
) -> Result<Document, SetupError> {
    let mut doc = base_doc.clone();
    let mut touched_fill = false;
    for (meta, &zj) in metas.iter().zip(z) {
        let val = meta.theta_from_z(zj);
        if is_fill_opt_addr(base_doc, &meta.addr) {
            apply_fill_opt_param(&mut doc, &meta.addr, val)?;
            touched_fill = true;
        } else {
            apply_sens_param(&mut doc, &meta.addr, val).map_err(SetupError)?;
        }
    }
    if touched_fill {
        if let Some(f) = doc.fill.as_mut() {
            f.rho_max = f.rho_max.max(f.rho_min);
            if let Some(sm) = f.skin_max.as_mut() {
                *sm = sm.max(f.skin);
            }
            if let Some(cm) = f.cell_min.as_mut() {
                *cm = cm.min(f.cell);
            }
        }
    }
    let errs = doc.validate();
    if !errs.is_empty() {
        return Err(SetupError(format!(
            "calibrate: candidate parameters produced invalid document: {}",
            errs.join("; ")
        )));
    }
    Ok(doc)
}

fn required_modes_for_calibration(doc: &Document, observations: &[ObserveSpec]) -> Option<usize> {
    let mut modes = required_modes_for_asserts(doc.solve.modes, &doc.asserts);
    for obs in observations {
        let (_, bm) = strip_quantile_prefix(&obs.metric);
        if bm == "mode_split_12_pct" {
            modes = Some(modes.unwrap_or(0).max(2));
        } else if let Some(idx1) = crate::contracts::parse_mode_metric_index(bm) {
            modes = Some(modes.unwrap_or(0).max(idx1));
        }
    }
    modes
}

fn evaluate_cal_state(
    base_doc: &Document,
    metas: &[CalParamMeta],
    z: &[f64],
    observations: &[ObserveSpec],
    res: Resolution,
    clock: Clock,
) -> Result<CalState, SetupError> {
    let doc = apply_parameter_vector(base_doc, metas, z)?;
    let sys_doc = resolve_system_doc(&doc)?.into_owned();
    let modes_count = required_modes_for_calibration(&sys_doc, observations);

    let analysis = analyse_full(&sys_doc, res, clock, modes_count, false)?;
    let graded = if sys_doc.fill.is_some() {
        Some(fill::graded_of(&sys_doc, &analysis, clock, false)?)
    } else {
        None
    };

    let mut predictions = Vec::with_capacity(observations.len());
    let mut residuals_r = Vec::with_capacity(observations.len());
    let mut chi2_obs = 0.0;
    for obs in observations {
        let (_, base_m) = strip_quantile_prefix(&obs.metric);
        let pred = resolve_assert_metric(base_m, &analysis, graded.as_ref()).map_err(SetupError)?;
        let r = (pred - obs.measured) / obs.std;
        chi2_obs += r * r;
        predictions.push(pred);
        residuals_r.push(r);
    }

    let mut chi2_prior = 0.0;
    for (meta, &zj) in metas.iter().zip(z) {
        let z_eff = (meta.theta_from_z(zj) - meta.prior_mean) / meta.prior_sigma;
        chi2_prior += z_eff * z_eff;
    }
    let map_cost = 0.5 * (chi2_obs + chi2_prior);

    let sdoc = crate::structural_doc(&sys_doc);
    let static_solves = crate::case_names(sdoc.as_ref()).len() + sdoc.thermal_cases().len();
    let primal_solves = if graded.is_some() {
        (2 * static_solves + usize::from(analysis.modes.is_some())).max(1)
    } else {
        (static_solves + usize::from(analysis.modes.is_some())).max(1)
    };

    Ok(CalState {
        doc,
        sys_doc,
        analysis,
        graded,
        predictions,
        residuals_r,
        chi2_obs,
        chi2_prior,
        map_cost,
        primal_solves,
    })
}

/// Compute `m × n` physical sensitivity matrix `H_{i,j} = ∂M_i / ∂θ_j` at `state`.
/// Uses `O(1)` 3D discrete adjoint sensitivities first and seamlessly falls back to central
/// finite differences at locked `h` only if an observation queries a non-adjoint metric (e.g. `hx_*`).
fn compute_observation_jacobian(
    state: &CalState,
    metas: &[CalParamMeta],
    observations: &[ObserveSpec],
    locked_h: f64,
    clock: Clock,
) -> Result<(Vec<Vec<f64>>, usize, String), SetupError> {
    let param_names: Vec<String> = metas.iter().map(|m| m.name.clone()).collect();
    let am_needed = state.doc.solve.build.is_some()
        || observations
            .iter()
            .any(|o| o.metric.contains("peel") || o.metric.contains("overhang"))
        || metas.iter().any(|m| {
            is_solve_param_target(&state.doc, &m.addr)
                || matches!(m.addr.attr.as_str(), "yield_z" | "shear_z")
        });

    let sens_doc: std::borrow::Cow<'_, Document> = if am_needed && state.doc.solve.build.is_none() {
        let mut d = state.doc.clone();
        d.solve.build = Some([0.0, 0.0]);
        std::borrow::Cow::Owned(d)
    } else {
        std::borrow::Cow::Borrowed(&state.doc)
    };

    let sens: SensitivityReport = if let Some(ref gr) = state.graded {
        compute_sensitivities_graded_with_base(
            sens_doc.as_ref(),
            &state.sys_doc,
            &state.analysis,
            gr,
            &param_names,
            clock(),
            clock,
        )?
    } else {
        compute_sensitivities_with_base(
            sens_doc.as_ref(),
            &state.sys_doc,
            &state.analysis,
            &param_names,
            clock(),
            clock,
        )?
    };
    let mut extra_solves = sens.solves_run.saturating_sub(state.primal_solves);

    let need_solid_sens = state.graded.is_some()
        && observations.iter().any(|o| {
            let (_, bm) = strip_quantile_prefix(&o.metric);
            !is_graded_assert_metric(bm)
                && !bm.starts_with("mode_")
                && !matches!(
                    bm,
                    "overhang_fraction"
                        | "overhang_pct"
                        | "overhang_area_mm2"
                        | "overhang_area"
                        | "max_overhang_deg"
                        | "overhang_deg"
                )
        });
    let non_fill_params: Vec<String> = metas
        .iter()
        .filter(|m| !is_fill_opt_addr(&state.doc, &m.addr))
        .map(|m| m.name.clone())
        .collect();
    let solid_sens = if need_solid_sens && !non_fill_params.is_empty() {
        let mut s_doc = sens_doc.as_ref().clone();
        s_doc.fill = None;
        let mut s_sys_doc = state.sys_doc.clone();
        s_sys_doc.fill = None;
        let sr = compute_sensitivities_with_base(
            &s_doc,
            &s_sys_doc,
            &state.analysis,
            &non_fill_params,
            clock(),
            clock,
        )?;
        let sdoc = crate::structural_doc(&s_sys_doc);
        let solid_base = (crate::case_names(sdoc.as_ref()).len()
            + sdoc.thermal_cases().len()
            + usize::from(state.analysis.modes.is_some()))
        .max(1);
        extra_solves += sr.solves_run.saturating_sub(solid_base);
        Some(sr)
    } else {
        None
    };

    let m_obs = observations.len();
    let n_par = metas.len();
    let mut jac_h = vec![vec![0.0; n_par]; m_obs];
    let mut need_fd_cols = vec![false; n_par];

    for (j, meta) in metas.iter().enumerate() {
        let ps = sens
            .parameters
            .iter()
            .find(|p| p.param == meta.name)
            .ok_or_else(|| {
                SetupError(format!(
                    "calibrate: missing sensitivity entry for parameter '{}'",
                    meta.name
                ))
            })?;
        let sps = solid_sens
            .as_ref()
            .and_then(|sr| sr.parameters.iter().find(|p| p.param == meta.name));
        for (i, obs) in observations.iter().enumerate() {
            let (_, base_m) = strip_quantile_prefix(&obs.metric);
            match metric_derivative_for_param(
                &state.doc,
                base_m,
                &state.analysis,
                state.graded.as_ref(),
                ps,
                sps,
            ) {
                Ok(dm_dtheta) => jac_h[i][j] = dm_dtheta,
                Err(_) => need_fd_cols[j] = true,
            }
        }
    }

    let mut used_fd = false;
    for (j, meta) in metas.iter().enumerate() {
        if !need_fd_cols[j] {
            continue;
        }
        used_fd = true;
        let theta_curr = read_param_value(&state.doc, &meta.addr).map_err(SetupError)?;
        let delta = (1e-3 * meta.prior_sigma)
            .max(1e-4 * theta_curr.abs())
            .max(1e-6);
        let theta_plus = (theta_curr + delta).min(meta.hi);
        let theta_minus = (theta_curr - delta).max(meta.lo);
        let span = theta_plus - theta_minus;
        if span <= 1e-14 {
            continue;
        }
        let mut doc_p = state.doc.clone();
        let mut doc_m = state.doc.clone();
        if is_fill_opt_addr(&state.doc, &meta.addr) {
            apply_fill_opt_param(&mut doc_p, &meta.addr, theta_plus)?;
            apply_fill_opt_param(&mut doc_m, &meta.addr, theta_minus)?;
        } else {
            apply_sens_param(&mut doc_p, &meta.addr, theta_plus).map_err(SetupError)?;
            apply_sens_param(&mut doc_m, &meta.addr, theta_minus).map_err(SetupError)?;
        }
        let modes_count = required_modes_for_calibration(&state.sys_doc, observations);
        let sys_p = resolve_system_doc(&doc_p)?.into_owned();
        let sys_m = resolve_system_doc(&doc_m)?.into_owned();
        let a_p = analyse_full(
            &sys_p,
            Resolution::Size(locked_h),
            clock,
            modes_count,
            false,
        )?;
        let g_p = if sys_p.fill.is_some() {
            Some(fill::graded_of(&sys_p, &a_p, clock, false)?)
        } else {
            None
        };
        let a_m = analyse_full(
            &sys_m,
            Resolution::Size(locked_h),
            clock,
            modes_count,
            false,
        )?;
        let g_m = if sys_m.fill.is_some() {
            Some(fill::graded_of(&sys_m, &a_m, clock, false)?)
        } else {
            None
        };
        extra_solves += 2 * state.primal_solves;
        for (i, obs) in observations.iter().enumerate() {
            let (_, base_m) = strip_quantile_prefix(&obs.metric);
            let vp = resolve_assert_metric(base_m, &a_p, g_p.as_ref()).map_err(SetupError)?;
            let vm = resolve_assert_metric(base_m, &a_m, g_m.as_ref()).map_err(SetupError)?;
            jac_h[i][j] = (vp - vm) / span;
        }
    }

    let method_tag = if used_fd {
        format!("{}+fd_fallback", sens.method)
    } else {
        sens.method
    };
    Ok((jac_h, extra_solves, method_tag))
}

/// Dense Gauss–Jordan matrix inverse + linear solver for symmetric positive-definite `n × n`
/// matrices (`n` is typically `1..25`).
fn invert_spd_matrix(mat: &[Vec<f64>]) -> Result<Vec<Vec<f64>>, SetupError> {
    let n = mat.len();
    let mut aug = vec![vec![0.0; 2 * n]; n];
    for (r, row) in mat.iter().enumerate() {
        aug[r][..n].copy_from_slice(row);
        aug[r][n + r] = 1.0;
    }
    for col in 0..n {
        let mut pivot_row = col;
        let mut max_abs = aug[col][col].abs();
        for (r, aug_row) in aug.iter().enumerate().skip(col + 1) {
            if aug_row[col].abs() > max_abs {
                max_abs = aug_row[col].abs();
                pivot_row = r;
            }
        }
        if max_abs < 1e-14 {
            return Err(SetupError(
                "calibrate: posterior precision matrix is singular".into(),
            ));
        }
        if pivot_row != col {
            aug.swap(col, pivot_row);
        }
        let piv = aug[col][col];
        for val in &mut aug[col] {
            *val /= piv;
        }
        let piv_row_vals = aug[col].clone();
        for (r, aug_row) in aug.iter_mut().enumerate() {
            if r == col {
                continue;
            }
            let factor = aug_row[col];
            if factor != 0.0 {
                for (dst, &src) in aug_row.iter_mut().zip(&piv_row_vals) {
                    *dst -= factor * src;
                }
            }
        }
    }
    let mut inv = vec![vec![0.0; n]; n];
    for (r, aug_row) in aug.iter().enumerate() {
        inv[r].copy_from_slice(&aug_row[n..]);
    }
    Ok(inv)
}

/// Solve the box-constrained quadratic Gauss–Newton / LM subproblem:
/// `min_{dz ∈ [l, u]} ½ dzᵀ H_damped dz + gᵀ dz`.
/// Uses the exact unconstrained matrix inverse when `dz_unc ∈ [l, u]`, and projected FISTA
/// refinement when any parameter hits a physical box bound.
fn solve_box_gn_step(
    h_damped: &[Vec<f64>],
    grad: &[f64],
    l: &[f64],
    u: &[f64],
) -> Result<Vec<f64>, SetupError> {
    let n = grad.len();
    let inv = invert_spd_matrix(h_damped)?;
    let mut dz = vec![0.0; n];
    let mut inside = true;
    for j in 0..n {
        let mut acc = 0.0;
        for k in 0..n {
            acc -= inv[j][k] * grad[k];
        }
        if acc < l[j] || acc > u[j] {
            inside = false;
        }
        dz[j] = acc.clamp(l[j], u[j]);
    }
    if inside {
        return Ok(dz);
    }

    // Estimate Lipschitz constant `‖H_damped‖_1` for exact box-projected FISTA refinement.
    let mut lip = 1.0_f64;
    for row in h_damped {
        let row_sum: f64 = row.iter().map(|v| v.abs()).sum();
        lip = lip.max(row_sum);
    }
    let mut x = dz;
    let mut y = x.clone();
    let mut t = 1.0_f64;
    for _ in 0..120 {
        let mut x_next = vec![0.0; n];
        let mut max_diff = 0.0_f64;
        for j in 0..n {
            let mut gj = grad[j];
            for (k, &yk) in y.iter().enumerate() {
                gj += h_damped[j][k] * yk;
            }
            let cand = (y[j] - gj / lip).clamp(l[j], u[j]);
            max_diff = max_diff.max((cand - x[j]).abs());
            x_next[j] = cand;
        }
        let t_next = 0.5 * (1.0 + (1.0 + 4.0 * t * t).sqrt());
        let beta = (t - 1.0) / t_next;
        for j in 0..n {
            y[j] = (x_next[j] + beta * (x_next[j] - x[j])).clamp(l[j], u[j]);
        }
        x = x_next;
        t = t_next;
        if max_diff < 1e-10 {
            break;
        }
    }
    Ok(x)
}

fn write_posterior_uncertainties(
    doc: &mut Document,
    metas: &[CalParamMeta],
    theta_map: &[f64],
    sigma_post: &[f64],
) {
    for ((meta, &th), &sig) in metas.iter().zip(theta_map).zip(sigma_post) {
        let clean_th = clean_sig12(th);
        let clean_sig = clean_sig12(sig.max(1e-12));

        // Clean up `let` expression formatting so printed `.fsm` carries concise decimals.
        if meta.addr.attr.is_empty() {
            if let Some(l) = doc.lets.iter_mut().find(|l| l.name == meta.name) {
                l.value = clean_th;
                l.expr = format!("{clean_th}");
            }
        }

        let mut updated_system_epistemic = false;
        if let Some(sb) = &mut doc.system_block {
            if let Some(ep) = sb.epistemics.iter_mut().find(|e| e.name == meta.name) {
                ep.nominal = clean_th;
                ep.sigma = clean_sig;
                if clean_th < ep.min {
                    ep.min = clean_th;
                }
                if clean_th > ep.max {
                    ep.max = clean_th;
                }
                updated_system_epistemic = true;
            }
            if let Some(dp) = sb.designs.iter_mut().find(|d| d.name == meta.name) {
                dp.nominal = clean_th;
            }
        }

        if let Some(u) = doc
            .uncertainties
            .iter_mut()
            .find(|u| u.param.trim() == meta.name)
        {
            u.std = Some(clean_sig);
            u.cv = None;
        } else if !updated_system_epistemic {
            doc.uncertainties.push(UncertaintySpec {
                label: meta.label.clone(),
                param: meta.name.clone(),
                std: Some(clean_sig),
                cv: None,
                unit: meta.unit.clone(),
            });
        }
    }
}

/// Run adjoint Gauss–Newton / Levenberg–Marquardt Bayesian MAP calibration on `doc`, returning
/// both the structured [`CalibrationReport`] and the updated posterior [`Document`].
pub fn calibrate_document(
    doc: &Document,
    spec: &CalibrateSpec,
    clock: Clock,
) -> Result<(CalibrationReport, Document), SetupError> {
    let t0 = clock();
    let observations: Vec<ObserveSpec> = if !spec.observations.is_empty() {
        spec.observations.clone()
    } else {
        doc.observations.clone()
    };
    if observations.is_empty() {
        return Err(SetupError(
            "calibrate: document declares no bench observations \
             (add `observe \"...\" metric=\"...\" measured=... std=...` or pass `observations`)"
                .into(),
        ));
    }
    for obs in &observations {
        if obs.metric.trim().is_empty() {
            return Err(SetupError(format!(
                "calibrate: observation '{}' has empty metric",
                obs.label
            )));
        }
        if !obs.measured.is_finite() {
            return Err(SetupError(format!(
                "calibrate: observation '{}' has non-finite measured value",
                obs.label
            )));
        }
        if !obs.std.is_finite() || obs.std <= 0.0 {
            return Err(SetupError(format!(
                "calibrate: observation '{}' must have positive std (got {})",
                obs.label, obs.std
            )));
        }
    }

    let metas = build_cal_param_metas(doc, spec)?;
    let n_par = metas.len();
    let m_obs = observations.len();
    let max_iters = spec.max_iters.unwrap_or(6).clamp(1, 25);

    let init_res = if let Some(h) = spec.h.or(doc.solve.size) {
        Resolution::Size(h)
    } else {
        Resolution::Dofs(spec.dofs.unwrap_or(8_000))
    };

    let mut z = vec![0.0_f64; n_par];
    let init_state = evaluate_cal_state(doc, &metas, &z, &observations, init_res, clock)?;
    let locked_h = init_state.analysis.provenance.element_size;
    let locked_res = Resolution::Size(locked_h);

    let (initial_verification, init_uq) = evaluate_contracts_with_uq(
        &init_state.doc,
        &init_state.sys_doc,
        &doc.asserts,
        &init_state.analysis,
        init_state.graded.as_ref(),
        clock,
    )
    .map_err(SetupError)?;

    let mut solves_run = init_uq
        .map(|u| u.solves_run)
        .unwrap_or(init_state.primal_solves);

    let initial_cost = init_state.map_cost;
    let prior_predictions = init_state.predictions.clone();

    let (mut cur_jac_h, init_jac_solves, mut sens_method) =
        compute_observation_jacobian(&init_state, &metas, &observations, locked_h, clock)?;
    solves_run += init_jac_solves;

    // Prior predictive sigmas `sqrt(∑_j (H_{i,j} σ_prior,j)²)`.
    let prior_pred_sigmas: Vec<f64> = (0..m_obs)
        .map(|i| {
            cur_jac_h[i]
                .iter()
                .zip(&metas)
                .map(|(&hij, meta)| {
                    let s = hij * meta.prior_sigma;
                    s * s
                })
                .sum::<f64>()
                .sqrt()
        })
        .collect();

    let mut cur_state = init_state;
    let mut lambda = 1e-6_f64;
    let mut converged = false;
    let mut iterations_done = 0usize;
    let mut history = Vec::with_capacity(max_iters + 1);

    let snapshot_params = |zv: &[f64]| -> BTreeMap<String, f64> {
        metas
            .iter()
            .zip(zv)
            .map(|(m, &zj)| (m.name.clone(), clean_sig12(m.theta_from_z(zj))))
            .collect()
    };

    history.push(CalibrationIteration {
        iter: 0,
        map_cost: cur_state.map_cost,
        chi2_obs: cur_state.chi2_obs,
        chi2_prior: cur_state.chi2_prior,
        damping_lambda: 0.0,
        step_norm_z: 0.0,
        params: snapshot_params(&z),
    });

    for iter_idx in 1..=max_iters {
        // Assemble dimensionless Jacobian `A_{i,j} = (σ_prior,j / σ_obs,i) H_{i,j}`,
        // gradient `g = Aᵀ r + z`, and Gauss–Newton matrix `G = I_n + Aᵀ A`.
        let mut a_mat = vec![vec![0.0; n_par]; m_obs];
        for i in 0..m_obs {
            let inv_obs_sig = 1.0 / observations[i].std;
            for j in 0..n_par {
                a_mat[i][j] = cur_jac_h[i][j] * metas[j].prior_sigma * inv_obs_sig;
            }
        }

        let mut grad = vec![0.0; n_par];
        for (j, grad_j) in grad.iter_mut().enumerate() {
            let z_eff = (metas[j].theta_from_z(z[j]) - metas[j].prior_mean) / metas[j].prior_sigma;
            let mut acc = z_eff;
            for (row_a, &res_i) in a_mat.iter().zip(&cur_state.residuals_r) {
                acc += row_a[j] * res_i;
            }
            *grad_j = acc;
        }

        // First-order projected KKT stationarity check in `z`-space.
        let mut proj_grad_inf = 0.0_f64;
        for (j, meta) in metas.iter().enumerate() {
            let z_curr = (meta.theta_from_z(z[j]) - meta.prior_mean) / meta.prior_sigma;
            let z_proj = (z_curr - grad[j]).clamp(meta.z_lo(), meta.z_hi());
            proj_grad_inf = proj_grad_inf.max((z_proj - z_curr).abs());
        }
        if proj_grad_inf < 1e-6 {
            converged = true;
            break;
        }

        let mut g_base = vec![vec![0.0; n_par]; n_par];
        for j in 0..n_par {
            g_base[j][j] = 1.0;
            for k in 0..n_par {
                let mut dot = 0.0;
                for row_a in &a_mat {
                    dot += row_a[j] * row_a[k];
                }
                g_base[j][k] += dot;
            }
        }

        let l_box: Vec<f64> = metas.iter().zip(&z).map(|(m, &zj)| m.z_lo() - zj).collect();
        let u_box: Vec<f64> = metas.iter().zip(&z).map(|(m, &zj)| m.z_hi() - zj).collect();

        let mut accepted = None;
        let mut trial_lambda = lambda;
        for _lm_try in 0..10 {
            let mut g_damped = g_base.clone();
            for (j, row_g) in g_damped.iter_mut().enumerate() {
                row_g[j] += trial_lambda * row_g[j].max(1.0);
            }
            let dz = solve_box_gn_step(&g_damped, &grad, &l_box, &u_box)?;
            let step_norm = dz.iter().map(|v| v.abs()).fold(0.0_f64, f64::max);
            if step_norm < 1e-8 {
                converged = true;
                break;
            }
            let z_cand: Vec<f64> = z
                .iter()
                .zip(&dz)
                .zip(&metas)
                .map(|((&zj, &dzj), m)| (zj + dzj).clamp(m.z_lo(), m.z_hi()))
                .collect();

            match evaluate_cal_state(doc, &metas, &z_cand, &observations, locked_res, clock) {
                Ok(cand_state) => {
                    solves_run += cand_state.primal_solves;
                    if cand_state.map_cost <= cur_state.map_cost + 1e-12 {
                        accepted = Some((z_cand, cand_state, step_norm, trial_lambda));
                        break;
                    }
                }
                Err(_) => {
                    // Candidate crossed a nonlinear domain constraint; increase LM damping.
                }
            }
            trial_lambda = (trial_lambda * 8.0).max(0.125);
        }

        if converged {
            break;
        }

        let Some((z_next, next_state, step_norm, used_lambda)) = accepted else {
            break;
        };

        let cost_drop = (cur_state.map_cost - next_state.map_cost).max(0.0);
        z = z_next;
        cur_state = next_state;
        iterations_done = iter_idx;
        lambda = (used_lambda * 0.2).max(1e-8);

        history.push(CalibrationIteration {
            iter: iter_idx,
            map_cost: cur_state.map_cost,
            chi2_obs: cur_state.chi2_obs,
            chi2_prior: cur_state.chi2_prior,
            damping_lambda: used_lambda,
            step_norm_z: step_norm,
            params: snapshot_params(&z),
        });

        let (next_jac_h, jac_solves, next_method) =
            compute_observation_jacobian(&cur_state, &metas, &observations, locked_h, clock)?;
        solves_run += jac_solves;
        cur_jac_h = next_jac_h;
        sens_method = next_method;

        if step_norm < 1e-4 || cost_drop < 1e-7 * (1.0 + cur_state.map_cost) {
            converged = true;
            break;
        }
    }

    // Exact Laplace posterior covariance in standardized `z`-space:
    // `C_z = (I_n + A_MAPᵀ A_MAP)⁻¹`, `Σ_post = diag(σ_prior) C_z diag(σ_prior)`.
    let mut a_final = vec![vec![0.0; n_par]; m_obs];
    for i in 0..m_obs {
        let inv_obs_sig = 1.0 / observations[i].std;
        for j in 0..n_par {
            a_final[i][j] = cur_jac_h[i][j] * metas[j].prior_sigma * inv_obs_sig;
        }
    }
    let mut precision_z = vec![vec![0.0; n_par]; n_par];
    for j in 0..n_par {
        precision_z[j][j] = 1.0;
        for k in 0..n_par {
            let mut dot = 0.0;
            for row_a in &a_final {
                dot += row_a[j] * row_a[k];
            }
            precision_z[j][k] += dot;
        }
    }
    let cov_z = invert_spd_matrix(&precision_z)?;

    let mut cov_phys = vec![vec![0.0; n_par]; n_par];
    let mut sigma_post = vec![0.0; n_par];
    for j in 0..n_par {
        for k in 0..n_par {
            cov_phys[j][k] = metas[j].prior_sigma * cov_z[j][k] * metas[k].prior_sigma;
        }
        sigma_post[j] = cov_phys[j][j].max(0.0).sqrt();
    }

    let mut posterior_correlation = vec![vec![0.0; n_par]; n_par];
    for j in 0..n_par {
        posterior_correlation[j][j] = 1.0;
        for k in (j + 1)..n_par {
            let denom = (sigma_post[j] * sigma_post[k]).max(1e-30);
            let rho = (cov_phys[j][k] / denom).clamp(-1.0, 1.0);
            posterior_correlation[j][k] = rho;
            posterior_correlation[k][j] = rho;
        }
    }

    let theta_map: Vec<f64> = metas
        .iter()
        .zip(&z)
        .map(|(m, &zj)| m.theta_from_z(zj))
        .collect();

    let mut posterior_doc = cur_state.doc.clone();
    write_posterior_uncertainties(&mut posterior_doc, &metas, &theta_map, &sigma_post);
    let posterior_sys_doc = resolve_system_doc(&posterior_doc)?.into_owned();

    let (final_verification, final_uq) = evaluate_contracts_with_uq(
        &posterior_doc,
        &posterior_sys_doc,
        &posterior_doc.asserts,
        &cur_state.analysis,
        cur_state.graded.as_ref(),
        clock,
    )
    .map_err(SetupError)?;
    if let Some(u) = final_uq {
        solves_run += u.solves_run.saturating_sub(cur_state.primal_solves);
    }

    let mut param_summaries = Vec::with_capacity(n_par);
    for (j, meta) in metas.iter().enumerate() {
        let post_m = theta_map[j];
        let post_s = sigma_post[j];
        let red_pct = 100.0 * (1.0 - post_s / meta.prior_sigma);
        let shift_z = (post_m - meta.prior_mean) / meta.prior_sigma;
        param_summaries.push(CalibratedParamSummary {
            param: meta.name.clone(),
            label: meta.label.clone(),
            unit: meta.unit.clone(),
            prior_mean: meta.prior_mean,
            prior_sigma: meta.prior_sigma,
            posterior_mean: post_m,
            posterior_sigma: post_s,
            posterior_p10: post_m - Z_90 * post_s,
            posterior_p90: post_m + Z_90 * post_s,
            sigma_reduction_pct: red_pct,
            shift_in_prior_sigmas: shift_z,
            lower_bound: meta.lo,
            upper_bound: meta.hi,
        });
    }

    let mut obs_summaries = Vec::with_capacity(m_obs);
    for (i, obs) in observations.iter().enumerate() {
        let (_, base_m) = strip_quantile_prefix(&obs.metric);
        let mut post_var = 0.0_f64;
        for j in 0..n_par {
            for k in 0..n_par {
                post_var += cur_jac_h[i][j] * cov_phys[j][k] * cur_jac_h[i][k];
            }
        }
        let unit = if obs.unit.trim().is_empty() {
            default_metric_unit(base_m).to_string()
        } else {
            obs.unit.clone()
        };
        obs_summaries.push(ObservationFitSummary {
            label: obs.label.clone(),
            metric: obs.metric.clone(),
            unit,
            measured: obs.measured,
            obs_sigma: obs.std,
            prior_prediction: prior_predictions[i],
            posterior_prediction: cur_state.predictions[i],
            prior_pred_sigma: prior_pred_sigmas[i],
            posterior_pred_sigma: post_var.max(0.0).sqrt(),
            normalized_residual_z: cur_state.residuals_r[i],
        });
    }

    let calibrated_fsm = fsm::print(&posterior_doc);
    let reduced_chi2 = (cur_state.chi2_obs + cur_state.chi2_prior) / (m_obs as f64);

    Ok((
        CalibrationReport {
            method: "adjoint_gauss_newton_laplace_map".into(),
            sensitivity_method: sens_method,
            iterations: iterations_done,
            solves_run,
            converged,
            h: locked_h,
            initial_cost,
            final_cost: cur_state.map_cost,
            chi2_obs: cur_state.chi2_obs,
            chi2_prior: cur_state.chi2_prior,
            reduced_chi2,
            parameters: param_summaries,
            observations: obs_summaries,
            posterior_correlation,
            history,
            initial_verification,
            final_verification,
            calibrated_fsm,
            elapsed_ms: clock() - t0,
        },
        posterior_doc,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::now_ms;

    #[test]
    fn bayesian_linear_and_nonlinear_coupon_match_closed_form_gaussian_posterior() {
        // Part A: Exact linear-Gaussian conjugate check (`max_displacement(f_tip) = α · f_tip`).
        let linear_fsm = "\
fieldsmith 1
name \"Linear Conjugate Coupon\"
material alsi10mg
let f_tip = 100.0
box beam size=(60, 12, 6) at=(30, 6, 3)
region root = beam.nx
region tip = beam.px
support fix root fixed=xyz
load push tip force=(0, 0, -f_tip)
uncertainty \"Actuator load cell prior\" param=\"f_tip\" std=25.0 unit=\"N\"
observe \"Laser vibrometer deflection\" metric=\"max_displacement\" measured=0.260 std=0.012 unit=\"mm\"
solve size=4.0
";
        let doc = fsm::parse(linear_fsm).expect("parse linear coupon");
        let (rep, post_doc) =
            calibrate_document(&doc, &CalibrateSpec::default(), now_ms).expect("calibrate linear");
        assert!(rep.converged, "expected convergence: {rep:?}");
        assert_eq!(rep.parameters.len(), 1);

        let p = &rep.parameters[0];
        let o = &rep.observations[0];
        // Since w(f_tip) = α · f_tip is strictly linear on the locked P2 mesh, α = prior_pred / 100.0.
        let alpha = o.prior_prediction / 100.0;
        let tau_prior = 1.0 / (25.0_f64 * 25.0);
        let tau_like = (alpha * alpha) / (0.012_f64 * 0.012);
        let exact_post_var = 1.0 / (tau_prior + tau_like);
        let exact_post_sigma = exact_post_var.sqrt();
        let exact_post_mean =
            exact_post_var * (100.0 * tau_prior + alpha * 0.260 / (0.012 * 0.012));

        let rel_mean_err = ((p.posterior_mean - exact_post_mean) / exact_post_mean).abs();
        let rel_sig_err = ((p.posterior_sigma - exact_post_sigma) / exact_post_sigma).abs();
        assert!(
            rel_mean_err < 1e-4,
            "MAP mean {} vs exact conjugate {} (rel_err={rel_mean_err})",
            p.posterior_mean,
            exact_post_mean
        );
        assert!(
            rel_sig_err < 1e-4,
            "Laplace sigma {} vs exact conjugate {} (rel_err={rel_sig_err})",
            p.posterior_sigma,
            exact_post_sigma
        );
        assert!(p.sigma_reduction_pct > 70.0);

        // Verify round-trip of `calibrated_fsm`.
        let reparsed = fsm::parse(&rep.calibrated_fsm).expect("reparse calibrated_fsm");
        assert_eq!(reparsed.uncertainties.len(), 1);
        assert!(
            (reparsed.lets[0].value - post_doc.lets[0].value).abs() < 1e-9,
            "calibrated_fsm let mismatch"
        );

        // Part B: Nonlinear inverse Young's modulus calibration (`w(E) = C / E`).
        let e_true = 2150.0_f64;
        let true_doc = fsm::parse(&format!(
            "fieldsmith 1\n\
             name \"Printed Coupon E\"\n\
             custommaterial coupon name=\"Coupon\" e={e_true} nu=0.36 yield=50 density=1.25 wall=0.8\n\
             material coupon\n\
             box beam size=(60, 10, 5) at=(30, 5, 2.5)\n\
             region root = beam.nx\n\
             region tip = beam.px\n\
             support fix root fixed=xyz\n\
             load push tip force=(0, 0, -40)\n\
             solve size=4.0\n"
        ))
        .expect("parse true coupon");
        let a_true = analyse_full(&true_doc, Resolution::Size(4.0), now_ms, None, false)
            .expect("solve true");
        let w_meas = a_true.cases[0].max_displacement;

        let prior_doc = fsm::parse(&format!(
            "fieldsmith 1\n\
             name \"Printed Coupon E\"\n\
             custommaterial coupon name=\"Coupon\" e=1650 nu=0.36 yield=50 density=1.25 wall=0.8\n\
             material coupon\n\
             box beam size=(60, 10, 5) at=(30, 5, 2.5)\n\
             region root = beam.nx\n\
             region tip = beam.px\n\
             support fix root fixed=xyz\n\
             load push tip force=(0, 0, -40)\n\
             uncertainty \"Printed modulus prior\" param=\"coupon.e\" std=600 unit=\"MPa\"\n\
             observe \"Bench dial gauge\" metric=\"max_displacement\" measured={w_meas} std=0.015 unit=\"mm\"\n\
             solve size=4.0\n"
        ))
        .expect("parse prior coupon");
        let (rep_e, _) =
            calibrate_document(&prior_doc, &CalibrateSpec::default(), now_ms).expect("calibrate E");
        assert!(rep_e.converged);
        assert!(rep_e.iterations <= 6);
        let pe = &rep_e.parameters[0];
        // With tight bench noise (0.015 mm) and wide prior (σ=600 MPa), posterior pulls strongly toward E_true=2150 MPa.
        assert!(
            (pe.posterior_mean - e_true).abs() / e_true < 0.02,
            "expected posterior E near {e_true}, got {} (prior 1650)",
            pe.posterior_mean
        );
        assert!(pe.posterior_sigma < 80.0);
    }

    #[test]
    fn multi_sensor_thermo_structural_calibration_narrows_p90_contracts() {
        // Ground-truth hardware part (`h_true = 0.011 W/(mm²·K)`, `E_true = 72000 MPa`).
        let truth_raw = "\
fieldsmith 1
name \"Cooled Bracket Hardware Truth\"
custommaterial al_am name=\"LPBF Al\" e=72000 nu=0.33 yield=230 density=2.67 wall=0.8 a=0.000020 k=140
material al_am
box plate size=(50, 20, 6) at=(25, 10, 3)
region root = plate.nx
region tip = plate.px
region top = plate.pz
support fix root fixed=xyz
load push tip force=(0, 0, -600)
heatflux q_hot tip q=1.50
convection cool top h=0.011 tinf=25
solve size=4.5 tref=25
";
        let truth_doc = fsm::parse(truth_raw).expect("parse truth");
        let a_truth =
            analyse_full(&truth_doc, Resolution::Size(4.5), now_ms, None, false).expect("truth");
        let t_true = resolve_assert_metric("max_temp", &a_truth, None).expect("true max_temp");
        let w_true = resolve_assert_metric("max_displacement:Load 1", &a_truth, None)
            .expect("true load disp");

        // Conservative datasheet prior (`h = 0.0085 ± 0.0030`, `E = 63000 ± 9000`) violates
        // tight `p90` certification contracts until bench thermocouple + dial-gauge calibration.
        let t_limit = t_true + 3.5;
        let w_limit = w_true * 1.06;
        let raw = format!(
            "\
fieldsmith 1
name \"Cooled Bracket Bench Calibration\"
custommaterial al_am name=\"LPBF Al\" e=63000 nu=0.33 yield=230 density=2.67 wall=0.8 a=0.000020 k=140
material al_am
box plate size=(50, 20, 6) at=(25, 10, 3)
region root = plate.nx
region tip = plate.px
region top = plate.pz
support fix root fixed=xyz
load push tip force=(0, 0, -600)
heatflux q_hot tip q=1.50
convection cool top h=0.0085 tinf=25
uncertainty \"Convection film prior\" param=\"cool.h\" std=0.0030 unit=\"W/(mm²·K)\"
uncertainty \"LPBF modulus prior\" param=\"al_am.e\" std=9000 unit=\"MPa\"
observe \"IR thermocouple T_max\" metric=\"max_temp\" measured={t_true:.4} std=0.35 unit=\"°C\"
observe \"Tip dial indicator\" metric=\"max_displacement:Load 1\" measured={w_true:.5} std=0.008 unit=\"mm\"
assert \"Posterior p90 peak temp\" metric=\"p90.max_temp\" target=\"<= {t_limit:.3}\" unit=\"°C\"
assert \"Posterior p90 tip disp\" metric=\"p90.max_displacement:Load 1\" target=\"<= {w_limit:.4}\" unit=\"mm\"
solve size=4.5 tref=25
"
        );
        let doc = fsm::parse(&raw).expect("parse thermo-structural calibration");
        let (rep, post_doc) =
            calibrate_document(&doc, &CalibrateSpec::default(), now_ms).expect("calibrate");
        assert!(
            !rep.initial_verification.all_passed,
            "wide prior should violate tight p90 contracts before bench calibration: {:?}",
            rep.initial_verification
        );
        assert!(
            rep.final_verification.all_passed,
            "narrowed posterior must satisfy p90 contracts: {:?}",
            rep.final_verification
        );
        assert_eq!(rep.posterior_correlation.len(), 2);
        assert!(rep.parameters.iter().all(|p| p.sigma_reduction_pct > 55.0));
        assert!(post_doc.validate().is_empty());
    }

    #[test]
    fn library_material_modulus_calibration_propagates_nonzero_uq() {
        let raw = "\
fieldsmith 1
name \"Library Material Modulus Calibration\"
material al7075
box beam size=(80, 12, 8) at=(40, 6, 4)
region root = beam.nx
region tip = beam.px
support fix root fixed=xyz
load pull tip force=(0, 0, -400)
uncertainty \"Modulus prior\" param=\"al7075.e\" cv=0.10 unit=\"MPa\"
observe \"Tip dial gauge\" metric=\"max_displacement:Load 1\" measured=0.72 std=0.02 unit=\"mm\"
solve size=4.5
";
        let doc = fsm::parse(raw).expect("parse");
        let (rep, post_doc) =
            calibrate_document(&doc, &CalibrateSpec::default(), now_ms).expect("calibrate");
        assert_eq!(rep.parameters.len(), 1);
        assert_eq!(rep.parameters[0].param, "al7075.e");
        assert!(rep.parameters[0].posterior_sigma > 0.0);

        // compute_uq solves internally; empty `uncertainties` falls back to the
        // document's own specs, whose std was rewritten by the posterior pass.
        let uq = crate::uq::compute_uq(
            &post_doc,
            &crate::uq::UqSpec {
                uncertainties: Vec::new(),
                metrics: vec!["max_displacement:Load 1".into()],
                dofs: None,
                h: Some(4.5),
            },
            now_ms,
        )
        .expect("compute_uq");
        assert_eq!(uq.metrics.len(), 1);
        assert!(
            uq.metrics[0].std_dev > 1e-4,
            "expected non-zero UQ std_dev on calibrated library material al7075.e, got {}",
            uq.metrics[0].std_dev
        );
    }
}
