//! Contract-constrained 3D continuum & coupled 0D/1D+3D parameter optimiser (`optimise`).
//!
//! Solves general constrained sizing and process-parameter synthesis problems on arbitrary `.fsm`
//! documents:
//!   minimise (or maximise f₁) J(p)  over p ∈ [p_min, p_max]
//!   subject to all `.fsm` `assert` contracts  g_j(p) ≤ 0.
//!
//! Each outer iteration reuses the single forward 3D P2 solve inside
//! [`crate::sensitivity::compute_sensitivities_with_base`] (`O(1)` discrete mechanical and
//! staggered thermo-elastic adjoint solves plus `O(0)`-extra-solve Rayleigh-quotient modal
//! derivatives independent of parameter count `k`), assembles a dimensionless trust-region
//! Sequential Quadratic / Exact-Penalty subproblem on `[0, 1]^k`, and executes a backtracking
//! merit line search at locked element spacing `h`.

use crate::contracts::{
    evaluate_contracts, parse_mode_metric_index, required_modes_for_asserts, resolve_assert_metric,
    VerificationReport,
};
use crate::declarative_system::{resolve_system_doc, sync_system_doc_compact};
use crate::fill::{self, Graded};
use crate::sensitivity::{
    apply_sens_param, compute_sensitivities_with_base, default_target_params, read_param_value,
    CaseSensitivity, ParameterSensitivity,
};
use crate::{analyse_full, Analysis, CaseResult, Clock, Resolution, SetupError};
use fieldsmith_doc::fsm::{self, LiterateAssert};
use fieldsmith_doc::param::ParamAddr;
use fieldsmith_doc::Document;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Scalar objective minimised by [`optimise_document`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptObjective {
    /// Minimise part mass `m(p)` in grams (default structural/thermal sizing objective).
    #[default]
    MassG,
    /// Minimise total strain compliance `∑_c C_c(p)` in N·mm (maximise stiffness).
    Compliance,
    /// Minimise worst-case element-centroid von Mises stress `max_c σ_{vM,c}(p)` in MPa.
    MaxVmElem,
    /// Minimise worst-case peak nodal displacement `max_c ‖u‖_{max,c}(p)` in mm.
    MaxDisplacement,
    /// Minimise worst-case peak nodal temperature `max_c T_{max,c}(p)` in °C.
    MaxTemp,
    /// Maximise fundamental natural vibration frequency `f_1(p)` in Hz.
    #[serde(
        rename = "max_mode_1_hz",
        alias = "max_mode1_hz",
        alias = "mode_1",
        alias = "mode_1_hz"
    )]
    MaxMode1Hz,
}

impl OptObjective {
    pub fn unit(self) -> &'static str {
        match self {
            Self::MassG => "g",
            Self::Compliance => "N·mm",
            Self::MaxVmElem => "MPa",
            Self::MaxDisplacement => "mm",
            Self::MaxTemp => "°C",
            Self::MaxMode1Hz => "Hz",
        }
    }

    #[inline]
    fn minimise_sign(self) -> f64 {
        match self {
            Self::MaxMode1Hz => -1.0,
            _ => 1.0,
        }
    }
}

/// Configuration for [`optimise_document`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OptimiseSpec {
    #[serde(default)]
    pub objective: OptObjective,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bounds: BTreeMap<String, [f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iters: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dofs: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
}

/// Summary of one optimised design parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptParamSummary {
    pub param: String,
    pub initial: f64,
    pub final_value: f64,
    pub min_bound: f64,
    pub max_bound: f64,
    pub delta_pct: f64,
}

/// Trajectory snapshot recorded at each outer iteration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptIterationRecord {
    pub iter: usize,
    pub objective: f64,
    pub max_violation: f64,
    pub all_passed: bool,
    pub params: Vec<(String, f64)>,
    pub step_alpha: f64,
}

/// Full report returned by [`optimise_document`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptimisationReport {
    pub objective: OptObjective,
    pub objective_unit: String,
    pub method: String,
    pub sensitivity_method: String,
    pub iterations: usize,
    pub solves_run: usize,
    pub converged: bool,
    pub initial_objective: f64,
    pub final_objective: f64,
    pub objective_change_pct: f64,
    pub initial_verification: VerificationReport,
    pub final_verification: VerificationReport,
    pub parameters: Vec<OptParamSummary>,
    pub history: Vec<OptIterationRecord>,
    pub optimised_fsm: String,
    pub h: f64,
    pub elapsed_ms: f64,
}

struct ParamMeta {
    name: String,
    addr: ParamAddr,
    initial: f64,
    lo: f64,
    hi: f64,
}

impl ParamMeta {
    #[inline]
    fn span(&self) -> f64 {
        self.hi - self.lo
    }

    #[inline]
    fn to_unit(&self, p: f64) -> f64 {
        ((p - self.lo) / self.span()).clamp(0.0, 1.0)
    }

    #[inline]
    fn unscale_unit(&self, z: f64) -> f64 {
        self.lo + z.clamp(0.0, 1.0) * self.span()
    }
}

struct EvalState {
    doc: Document,
    sys_doc: Document,
    analysis: Analysis,
    verification: VerificationReport,
    objective: f64,
    constraints_g: Vec<f64>,
    solves: usize,
}

fn default_optimise_params(doc: &Document, requested: &[String]) -> Vec<String> {
    if !requested.is_empty() {
        return requested.to_vec();
    }
    let candidates = default_target_params(doc, &[]);
    candidates
        .into_iter()
        .filter(|name| {
            // Always include explicit `system` design levers.
            if doc
                .system_block
                .as_ref()
                .is_some_and(|sb| sb.designs.iter().any(|d| &d.name == name))
            {
                return true;
            }
            // Exclude bare `let`s whose ONLY consumers are external loads or heat fluxes,
            // preventing unconstrained load zeroing unless the caller explicitly names them.
            let readers: Vec<&fieldsmith_doc::Consumer> = doc
                .consumed
                .iter()
                .filter(|c| {
                    c.expr
                        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                        .any(|tok| tok == name)
                })
                .collect();
            if readers.is_empty() {
                return true;
            }
            let drives_non_load = readers.iter().any(|c| {
                !doc.loads.iter().any(|l| l.id == c.target)
                    && !doc.heat_fluxes.iter().any(|f| f.id == c.target)
            });
            drives_non_load
        })
        .collect()
}

fn build_param_metas(
    doc: &Document,
    names: &[String],
    explicit_bounds: &BTreeMap<String, [f64; 2]>,
) -> Result<Vec<ParamMeta>, SetupError> {
    if names.is_empty() {
        return Err(SetupError(
            "optimise: document has no tunable `let` or `system` design parameters (pass `params` explicitly)"
                .into(),
        ));
    }
    let mut metas = Vec::with_capacity(names.len());
    for name in names {
        let addr = ParamAddr::parse(name).map_err(SetupError)?;
        let initial = read_param_value(doc, &addr).map_err(SetupError)?;
        if !initial.is_finite() {
            return Err(SetupError(format!(
                "optimise: parameter '{name}' has non-finite value {initial}"
            )));
        }
        let (mut lo, mut hi) = if let Some(&[b_lo, b_hi]) = explicit_bounds.get(name) {
            if !b_lo.is_finite() || !b_hi.is_finite() || b_lo >= b_hi {
                return Err(SetupError(format!(
                    "optimise: invalid bounds [{b_lo}, {b_hi}] for '{name}' (expected finite min < max)"
                )));
            }
            (b_lo, b_hi)
        } else if let Some(dp) = doc
            .system_block
            .as_ref()
            .and_then(|sb| sb.designs.iter().find(|d| &d.name == name))
        {
            (dp.min, dp.max)
        } else if initial > 0.0 {
            (0.35 * initial, 2.5 * initial)
        } else if initial < 0.0 {
            (2.5 * initial, 0.35 * initial)
        } else {
            (-10.0, 10.0)
        };
        if initial < lo {
            lo = initial;
        }
        if initial > hi {
            hi = initial;
        }
        if (hi - lo).abs() < 1e-12 {
            hi = lo + 1.0;
        }
        metas.push(ParamMeta {
            name: name.clone(),
            addr,
            initial,
            lo,
            hi,
        });
    }
    Ok(metas)
}

fn find_case_index(cases: &[CaseResult], selector: &str, thermal_default: bool) -> Option<usize> {
    if selector.is_empty() {
        if thermal_default {
            return cases.iter().position(|c| c.temperature.is_some());
        }
        return if cases.is_empty() { None } else { Some(0) };
    }
    if let Ok(idx1) = selector.parse::<usize>() {
        if idx1 >= 1 && idx1 <= cases.len() {
            return Some(idx1 - 1);
        }
    }
    cases
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(selector) || c.name.contains(selector))
}

fn eval_objective_value(
    obj: OptObjective,
    a: &Analysis,
    g: Option<&Graded>,
) -> Result<f64, SetupError> {
    match obj {
        OptObjective::MassG => Ok(g.map(|gr| gr.mass_g).unwrap_or(a.mass_g)),
        OptObjective::Compliance => {
            if a.cases.is_empty() {
                return Err(SetupError(
                    "optimise: compliance objective requires at least one load or thermal case"
                        .into(),
                ));
            }
            Ok(a.cases.iter().map(|c| c.compliance).sum())
        }
        OptObjective::MaxVmElem => a
            .cases
            .iter()
            .map(|c| c.max_vm_elem)
            .reduce(f64::max)
            .ok_or_else(|| {
                SetupError("optimise: max_vm_elem objective requires at least one case".into())
            }),
        OptObjective::MaxDisplacement => a
            .cases
            .iter()
            .map(|c| c.max_displacement)
            .reduce(f64::max)
            .ok_or_else(|| {
                SetupError("optimise: max_displacement objective requires at least one case".into())
            }),
        OptObjective::MaxTemp => a
            .cases
            .iter()
            .filter_map(|c| c.temperature.as_deref())
            .map(|t| t.iter().copied().fold(f64::NEG_INFINITY, f64::max))
            .reduce(f64::max)
            .ok_or_else(|| {
                SetupError("optimise: max_temp objective requires at least one thermal case".into())
            }),
        OptObjective::MaxMode1Hz => a
            .modes
            .as_ref()
            .and_then(|m| m.frequencies.first().copied())
            .ok_or_else(|| {
                SetupError(
                    "optimise: max_mode_1_hz objective requires a supported structure with free DOFs"
                        .into(),
                )
            }),
    }
}

fn eval_objective_grad_p(
    obj: OptObjective,
    a: &Analysis,
    ps: &ParameterSensitivity,
) -> Result<f64, SetupError> {
    match obj {
        OptObjective::MassG => Ok(ps.d_mass_g),
        OptObjective::Compliance => Ok(ps.cases.iter().map(|c| c.d_compliance).sum()),
        OptObjective::MaxVmElem => {
            let (idx, _) = a
                .cases
                .iter()
                .enumerate()
                .max_by(|(_, c1), (_, c2)| c1.max_vm_elem.total_cmp(&c2.max_vm_elem))
                .ok_or_else(|| SetupError("optimise: no solved cases".into()))?;
            Ok(ps.cases.get(idx).map(|c| c.d_max_vm_elem).unwrap_or(0.0))
        }
        OptObjective::MaxDisplacement => {
            let (idx, _) = a
                .cases
                .iter()
                .enumerate()
                .max_by(|(_, c1), (_, c2)| c1.max_displacement.total_cmp(&c2.max_displacement))
                .ok_or_else(|| SetupError("optimise: no solved cases".into()))?;
            Ok(ps
                .cases
                .get(idx)
                .map(|c| c.d_max_displacement)
                .unwrap_or(0.0))
        }
        OptObjective::MaxTemp => {
            let (idx, _) = a
                .cases
                .iter()
                .enumerate()
                .filter_map(|(i, c)| {
                    let tmax = c
                        .temperature
                        .as_deref()?
                        .iter()
                        .copied()
                        .fold(f64::NEG_INFINITY, f64::max);
                    Some((i, tmax))
                })
                .max_by(|(_, t1), (_, t2)| t1.total_cmp(t2))
                .ok_or_else(|| SetupError("optimise: no thermal cases".into()))?;
            ps.cases.get(idx).and_then(|c| c.d_max_temp).ok_or_else(|| {
                SetupError("optimise: missing thermal sensitivity d_max_temp".into())
            })
        }
        OptObjective::MaxMode1Hz => {
            ps.d_modes_hz.first().copied().ok_or_else(|| {
                SetupError("optimise: missing modal sensitivity d_modes_hz[0]".into())
            })
        }
    }
}

fn metric_derivative_for_param(
    metric: &str,
    a: &Analysis,
    ps: &ParameterSensitivity,
) -> Result<f64, SetupError> {
    let m = metric.trim();
    match m {
        "mass_g" | "solid_mass_g" | "graded_mass_g" => return Ok(ps.d_mass_g),
        "mode_split_12_pct" => {
            let freqs = &a
                .modes
                .as_ref()
                .ok_or_else(|| {
                    SetupError("optimise: mode_split_12_pct requires modal solve".into())
                })?
                .frequencies;
            if freqs.len() < 2 || ps.d_modes_hz.len() < 2 {
                return Err(SetupError(
                    "optimise: mode_split_12_pct requires at least 2 solved modes".into(),
                ));
            }
            let (f1, f2) = (freqs[0], freqs[1]);
            let (df1, df2) = (ps.d_modes_hz[0], ps.d_modes_hz[1]);
            let sign = if f2 >= f1 { 1.0 } else { -1.0 };
            let sum = (f1 + f2).max(1e-12);
            return Ok(400.0 * sign * (f1 * df2 - f2 * df1) / (sum * sum));
        }
        _ => {}
    }
    if m.starts_with("mode_") {
        let idx1 = parse_mode_metric_index(m)
            .ok_or_else(|| SetupError(format!("optimise: invalid mode metric '{m}'")))?;
        return ps.d_modes_hz.get(idx1 - 1).copied().ok_or_else(|| {
            SetupError(format!(
                "optimise: contract '{m}' out of range ({} modal sensitivities)",
                ps.d_modes_hz.len()
            ))
        });
    }
    let rest = m
        .strip_prefix("solid.")
        .or_else(|| m.strip_prefix("graded."))
        .unwrap_or(m);
    let (field, case_sel) = match rest.split_once(':') {
        Some((f, cs)) => (f.trim(), cs.trim()),
        None => (rest.trim(), ""),
    };
    let thermal_default = matches!(field, "max_temp" | "min_temp");
    let idx = find_case_index(&a.cases, case_sel, thermal_default)
        .ok_or_else(|| SetupError(format!("optimise: contract '{m}' matched no solved case")))?;
    let cs: &CaseSensitivity = ps.cases.get(idx).ok_or_else(|| {
        SetupError(format!(
            "optimise: missing sensitivity entry for case #{idx} on contract '{m}'"
        ))
    })?;
    let bc = &a.cases[idx];
    match field {
        "compliance" => Ok(cs.d_compliance),
        "max_displacement" | "max_disp" | "deflection" => Ok(cs.d_max_displacement),
        "max_vm_elem" | "vm" | "max_von_mises" => Ok(cs.d_max_vm_elem),
        "min_safety_factor" | "safety_factor" => {
            let denom = bc.max_vm_elem.max(1e-9);
            Ok(-a.yield_strength * cs.d_max_vm_elem / (denom * denom))
        }
        "max_temp" => cs.d_max_temp.ok_or_else(|| {
            SetupError(format!(
                "optimise: contract '{m}' requires a thermal conduction case"
            ))
        }),
        _ => Err(SetupError(format!(
            "optimise: contract metric '{m}' is not supported by 3D adjoint differentiation \
             (supported: mass_g, compliance, max_displacement, max_vm_elem, min_safety_factor, max_temp, mode_k, mode_split_12_pct)"
        ))),
    }
}

/// Map a [`LiterateAssert`] value `val` (and optional parameter derivatives `dv_dp`) to normalized
/// inequality residual `g <= 0` (`g > 0` means violated) and `dg_dp`.
fn contract_residual_and_grad(
    asrt: &LiterateAssert,
    val: f64,
    dv_dp: Option<&[f64]>,
) -> Result<(f64, Vec<f64>), SetupError> {
    let tgt = asrt.target.trim();
    let tol_raw = asrt.tolerance.trim();

    if let Some(rhs) = tgt
        .strip_prefix("<=")
        .or_else(|| tgt.strip_prefix('≤'))
        .or_else(|| tgt.strip_prefix('<'))
    {
        let bound: f64 = rhs
            .trim()
            .parse()
            .map_err(|_| SetupError(format!("bad @assert bound '{tgt}'")))?;
        let scale = bound.abs().max(1.0);
        let g = (val - bound) / scale;
        let dg = dv_dp
            .map(|dv| dv.iter().map(|&d| d / scale).collect())
            .unwrap_or_default();
        return Ok((g, dg));
    }

    if let Some(rhs) = tgt
        .strip_prefix(">=")
        .or_else(|| tgt.strip_prefix('≥'))
        .or_else(|| tgt.strip_prefix('>'))
    {
        let bound: f64 = rhs
            .trim()
            .parse()
            .map_err(|_| SetupError(format!("bad @assert bound '{tgt}'")))?;
        let scale = bound.abs().max(1.0);
        let g = (bound - val) / scale;
        let dg = dv_dp
            .map(|dv| dv.iter().map(|&d| -d / scale).collect())
            .unwrap_or_default();
        return Ok((g, dg));
    }

    let expected: f64 = tgt
        .parse()
        .map_err(|_| SetupError(format!("bad @assert target '{tgt}'")))?;
    let tol_abs: f64 = if let Some(pct_str) = tol_raw.strip_suffix('%') {
        let pct: f64 = pct_str
            .trim()
            .trim_start_matches("<=")
            .trim_start_matches('≤')
            .trim()
            .parse()
            .map_err(|_| SetupError(format!("bad @assert percentage tolerance '{tol_raw}'")))?;
        expected.abs().max(1e-12) * (pct / 100.0)
    } else {
        tol_raw
            .trim_start_matches("<=")
            .trim_start_matches('≤')
            .trim_start_matches('±')
            .trim()
            .parse()
            .unwrap_or(1e-3)
    };
    let scale = expected.abs().max(tol_abs).max(1.0);
    let diff = val - expected;
    let sign = if diff >= 0.0 { 1.0 } else { -1.0 };
    let g = (diff.abs() - tol_abs) / scale;
    let dg = dv_dp
        .map(|dv| dv.iter().map(|&d| sign * d / scale).collect())
        .unwrap_or_default();
    Ok((g, dg))
}

#[inline]
fn smooth_hinge_and_grad(x: f64, delta: f64) -> (f64, f64) {
    if x <= 0.0 {
        (0.0, 0.0)
    } else if x < delta {
        (0.5 * x * x / delta, x / delta)
    } else {
        (x - 0.5 * delta, 1.0)
    }
}

/// Evaluate exact non-smoothed merit at normalized objective `f_norm` and normalized constraint
/// residuals `g` (`g_j <= 0` feasible).
fn eval_merit(f_norm: f64, g: &[f64], cushion: f64, rho: f64) -> f64 {
    let mut m = f_norm;
    for &gj in g {
        let vc = (gj + cushion).max(0.0);
        let vh = gj.max(0.0);
        m += rho * (vc + 0.5 * vc * vc) + 4.0 * rho * (vh + 0.5 * vh * vh);
    }
    m
}

/// Solve the convex trust-region SQP / exact-penalty subproblem on `s ∈ [l, u] ⊂ ℝⁿ` via FISTA.
fn solve_trust_region_subproblem(
    g_f: &[f64],
    g_0: &[f64],
    jac_g: &[Vec<f64>],
    z: &[f64],
    trust_delta: f64,
    cushion: f64,
    rho: f64,
) -> (Vec<f64>, f64) {
    let n = z.len();
    let l: Vec<f64> = z.iter().map(|&zk| (-trust_delta).max(-zk)).collect();
    let u: Vec<f64> = z.iter().map(|&zk| trust_delta.min(1.0 - zk)).collect();

    let gamma = 0.20;
    let huber_delta = 5e-3;

    let eval_sub = |s: &[f64]| -> (f64, Vec<f64>) {
        let mut val = 0.0;
        let mut grad = vec![0.0; n];
        for k in 0..n {
            val += g_f[k] * s[k] + 0.5 * gamma * s[k] * s[k];
            grad[k] = g_f[k] + gamma * s[k];
        }
        for (j, row) in jac_g.iter().enumerate() {
            let mut lin = g_0[j];
            for k in 0..n {
                lin += row[k] * s[k];
            }
            let (hc, dhc) = smooth_hinge_and_grad(lin + cushion, huber_delta);
            let (hh, dhh) = smooth_hinge_and_grad(lin, huber_delta);
            val += rho * (hc + 0.5 * hc * hc) + 4.0 * rho * (hh + 0.5 * hh * hh);
            let mult = rho * (1.0 + hc) * dhc + 4.0 * rho * (1.0 + hh) * dhh;
            if mult != 0.0 {
                for k in 0..n {
                    grad[k] += mult * row[k];
                }
            }
        }
        (val, grad)
    };

    let (val_zero, _) = eval_sub(&vec![0.0; n]);
    let mut s = vec![0.0; n];
    let mut y = vec![0.0; n];
    let mut t_fista = 1.0f64;
    let mut lip = gamma;

    for _ in 0..300 {
        let (val_y, grad_y) = eval_sub(&y);
        let mut s_next = vec![0.0; n];
        for _bt in 0..40 {
            let step = 1.0 / lip;
            let mut dot_dg = 0.0;
            let mut norm_ds_sq = 0.0;
            for k in 0..n {
                s_next[k] = (y[k] - step * grad_y[k]).clamp(l[k], u[k]);
                let ds = s_next[k] - y[k];
                dot_dg += grad_y[k] * ds;
                norm_ds_sq += ds * ds;
            }
            let (val_next, _) = eval_sub(&s_next);
            if val_next <= val_y + dot_dg + 0.5 * lip * norm_ds_sq + 1e-14 {
                break;
            }
            lip *= 2.0;
        }
        let max_diff = s_next
            .iter()
            .zip(&s)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f64, f64::max);
        let t_next = 0.5 * (1.0 + (1.0 + 4.0 * t_fista * t_fista).sqrt());
        let beta = (t_fista - 1.0) / t_next;
        for k in 0..n {
            y[k] = (s_next[k] + beta * (s_next[k] - s[k])).clamp(l[k], u[k]);
        }
        s = s_next;
        t_fista = t_next;
        lip = (0.9 * lip).max(gamma);
        if max_diff < 1e-9 {
            break;
        }
    }

    let (val_opt, _) = eval_sub(&s);
    let pred_drop = (val_zero - val_opt).max(0.0);
    (s, pred_drop)
}

fn evaluate_state(
    base_doc: &Document,
    metas: &[ParamMeta],
    z: &[f64],
    asserts: &[LiterateAssert],
    obj: OptObjective,
    res: Resolution,
    clock: Clock,
) -> Result<EvalState, SetupError> {
    let mut doc = base_doc.clone();
    for (meta, &zk) in metas.iter().zip(z) {
        let p_val = meta.unscale_unit(zk);
        apply_sens_param(&mut doc, &meta.addr, p_val).map_err(SetupError)?;
    }
    let sys_cow = resolve_system_doc(&doc)?;
    let sys_doc = sys_cow.into_owned();

    let need_graded = sys_doc.fill.is_some()
        && (obj == OptObjective::MassG
            || asserts.iter().any(|a| {
                let m = a.metric.trim();
                m.starts_with("graded.")
                    || m == "graded_mass_g"
                    || m == "mass_saved_pct"
                    || m == "mass_g"
            }));

    let mut modes_count = required_modes_for_asserts(sys_doc.solve.modes, asserts);
    if obj == OptObjective::MaxMode1Hz {
        modes_count = Some(modes_count.unwrap_or(0).max(1));
    }

    let (analysis, graded) = if need_graded {
        let (solid_a, gr) = fill::graded(&sys_doc, res, clock)?;
        if modes_count.is_some() {
            let modal_a = analyse_full(&sys_doc, res, clock, modes_count, false)?;
            (modal_a, Some(gr))
        } else {
            (solid_a, Some(gr))
        }
    } else {
        let a = analyse_full(&sys_doc, res, clock, modes_count, false)?;
        (a, None)
    };

    let verification =
        evaluate_contracts(asserts, &analysis, graded.as_ref()).map_err(SetupError)?;
    let objective = eval_objective_value(obj, &analysis, graded.as_ref())?;
    let mut constraints_g = Vec::with_capacity(asserts.len());
    for asrt in asserts {
        let val =
            resolve_assert_metric(&asrt.metric, &analysis, graded.as_ref()).map_err(SetupError)?;
        let (g, _) = contract_residual_and_grad(asrt, val, None)?;
        constraints_g.push(g);
    }
    let sdoc = crate::structural_doc(&sys_doc);
    let static_solves = crate::case_names(sdoc.as_ref()).len() + sdoc.thermal_cases().len();
    let solves = (static_solves + usize::from(analysis.modes.is_some())).max(1);
    Ok(EvalState {
        doc,
        sys_doc,
        analysis,
        verification,
        objective,
        constraints_g,
        solves,
    })
}

struct BestState {
    doc: Document,
    verification: VerificationReport,
    objective: f64,
    constraints_g: Vec<f64>,
}

fn max_hard_violation(g: &[f64]) -> f64 {
    g.iter().copied().fold(0.0f64, f64::max)
}

fn sum_hard_violation(g: &[f64]) -> f64 {
    g.iter().map(|&x| x.max(0.0)).sum()
}

fn is_better_state(cand: &EvalState, best: &BestState, min_sign: f64) -> bool {
    match (cand.verification.all_passed, best.verification.all_passed) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => min_sign * cand.objective < min_sign * best.objective,
        (false, false) => {
            let cv = sum_hard_violation(&cand.constraints_g);
            let bv = sum_hard_violation(&best.constraints_g);
            if (cv - bv).abs() > 1e-9 {
                cv < bv
            } else {
                min_sign * cand.objective < min_sign * best.objective
            }
        }
    }
}

/// Optimise a `.fsm` [`Document`]'s design parameters (`let` variables and/or `system` `design`
/// levers) to minimise `spec.objective` subject to all `.fsm` `assert` contracts.
///
/// Returns `(best_doc, OptimisationReport)`.
pub fn optimise_document(
    doc: &Document,
    raw_fsm: Option<&str>,
    spec: &OptimiseSpec,
    clock: Clock,
) -> Result<(Document, OptimisationReport), SetupError> {
    let t0 = clock();
    let asserts: Vec<LiterateAssert> = if !doc.asserts.is_empty() {
        doc.asserts.clone()
    } else if let Some(src) = raw_fsm {
        fsm::parse_literate_header(src)
            .map(|h| h.asserts)
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let param_names = default_optimise_params(doc, &spec.params);
    let metas = build_param_metas(doc, &param_names, &spec.bounds)?;
    let n = metas.len();

    let init_res = match (spec.h, spec.dofs) {
        (Some(h), _) => Resolution::Size(h),
        (None, Some(d)) => Resolution::Dofs(d),
        (None, None) => Resolution::Dofs(6_000),
    };
    let max_iters = spec.max_iters.unwrap_or(8).clamp(1, 25);

    let mut z: Vec<f64> = metas.iter().map(|m| m.to_unit(m.initial)).collect();
    let mut cur = evaluate_state(doc, &metas, &z, &asserts, spec.objective, init_res, clock)?;
    let locked_h = cur.analysis.provenance.element_size;
    let locked_res = Resolution::Size(locked_h);

    let initial_objective = cur.objective;
    let initial_verification = cur.verification.clone();
    let obj_scale = initial_objective.abs().max(1e-6);
    let min_sign = spec.objective.minimise_sign();
    let cushion = if asserts.is_empty() { 0.0 } else { 0.02 };
    let rho = 25.0;

    let mut total_solves = cur.solves;
    let mut sensitivity_method = String::from("boundary_velocity_adjoint");
    let mut history = Vec::with_capacity(max_iters + 1);
    history.push(OptIterationRecord {
        iter: 0,
        objective: cur.objective,
        max_violation: max_hard_violation(&cur.constraints_g),
        all_passed: cur.verification.all_passed,
        params: metas
            .iter()
            .zip(&z)
            .map(|(m, &zk)| (m.name.clone(), m.unscale_unit(zk)))
            .collect(),
        step_alpha: 0.0,
    });

    let mut best = BestState {
        doc: cur.doc.clone(),
        verification: cur.verification.clone(),
        objective: cur.objective,
        constraints_g: cur.constraints_g.clone(),
    };

    let mut trust_delta = 0.28f64;
    let mut converged = false;
    let mut iters_done = 0usize;

    for iter in 1..=max_iters {
        let sens = compute_sensitivities_with_base(
            &cur.doc,
            &cur.sys_doc,
            &cur.analysis,
            &param_names,
            clock(),
            clock,
        )?;
        sensitivity_method = sens.method.clone();
        // `sens.solves_run` includes the baseline solve count; add only the adjoint/perturbation solves.
        total_solves += sens.solves_run.saturating_sub(cur.solves);

        let mut g_f = vec![0.0; n];
        for (k, meta) in metas.iter().enumerate() {
            let ps = sens
                .parameters
                .iter()
                .find(|p| p.param == meta.name)
                .ok_or_else(|| {
                    SetupError(format!("optimise: missing sensitivity for '{}'", meta.name))
                })?;
            let dj_dp = eval_objective_grad_p(spec.objective, &cur.analysis, ps)?;
            g_f[k] = min_sign * dj_dp * meta.span() / obj_scale;
        }

        let mut jac_g = Vec::with_capacity(asserts.len());
        for (j, asrt) in asserts.iter().enumerate() {
            let val = cur.verification.outcomes[j].value;
            let mut dv_dp = Vec::with_capacity(n);
            for meta in &metas {
                let ps = sens
                    .parameters
                    .iter()
                    .find(|p| p.param == meta.name)
                    .unwrap();
                dv_dp.push(metric_derivative_for_param(
                    &asrt.metric,
                    &cur.analysis,
                    ps,
                )?);
            }
            let (_, dg_dp) = contract_residual_and_grad(asrt, val, Some(&dv_dp))?;
            let dg_dz: Vec<f64> = dg_dp
                .into_iter()
                .zip(&metas)
                .map(|(d, m)| d * m.span())
                .collect();
            jac_g.push(dg_dz);
        }

        let (step_s, pred_drop) = solve_trust_region_subproblem(
            &g_f,
            &cur.constraints_g,
            &jac_g,
            &z,
            trust_delta,
            cushion,
            rho,
        );
        let max_step = step_s.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
        if max_step < 5e-4 && cur.verification.all_passed {
            converged = true;
            break;
        }

        let cur_merit = eval_merit(
            min_sign * cur.objective / obj_scale,
            &cur.constraints_g,
            cushion,
            rho,
        );
        let cur_hard = sum_hard_violation(&cur.constraints_g);

        let mut accepted: Option<(Vec<f64>, EvalState, f64)> = None;
        for &alpha in &[1.0, 0.5, 0.25] {
            let z_trial: Vec<f64> = z
                .iter()
                .zip(&step_s)
                .map(|(&zk, &sk)| (zk + alpha * sk).clamp(0.0, 1.0))
                .collect();
            let Ok(trial) = evaluate_state(
                doc,
                &metas,
                &z_trial,
                &asserts,
                spec.objective,
                locked_res,
                clock,
            ) else {
                continue;
            };
            total_solves += trial.solves;
            if is_better_state(&trial, &best, min_sign) {
                best = BestState {
                    doc: trial.doc.clone(),
                    verification: trial.verification.clone(),
                    objective: trial.objective,
                    constraints_g: trial.constraints_g.clone(),
                };
            }
            let trial_merit = eval_merit(
                min_sign * trial.objective / obj_scale,
                &trial.constraints_g,
                cushion,
                rho,
            );
            let trial_hard = sum_hard_violation(&trial.constraints_g);
            let improved_merit = trial_merit <= cur_merit - 1e-4 * alpha * pred_drop;
            let improved_infeasible = cur_hard > 0.0 && trial_hard < 0.92 * cur_hard;
            if improved_merit || improved_infeasible {
                accepted = Some((z_trial, trial, alpha));
                break;
            }
        }

        iters_done = iter;
        if let Some((z_next, next_state, alpha)) = accepted {
            let rel_obj_move =
                (next_state.objective - cur.objective).abs() / cur.objective.abs().max(1e-6);
            z = z_next;
            cur = next_state;
            history.push(OptIterationRecord {
                iter,
                objective: cur.objective,
                max_violation: max_hard_violation(&cur.constraints_g),
                all_passed: cur.verification.all_passed,
                params: metas
                    .iter()
                    .zip(&z)
                    .map(|(m, &zk)| (m.name.clone(), m.unscale_unit(zk)))
                    .collect(),
                step_alpha: alpha,
            });
            if alpha >= 0.99 {
                trust_delta = (trust_delta * 1.15).min(0.35);
            } else {
                trust_delta = (trust_delta * 0.75).max(0.02);
            }
            if cur.verification.all_passed && rel_obj_move < 2e-3 && alpha * max_step < 5e-3 {
                converged = true;
                break;
            }
        } else {
            trust_delta *= 0.5;
            if trust_delta < 0.015 {
                converged = best.verification.all_passed;
                break;
            }
        }
    }

    let mut best_doc = best.doc;
    sync_system_doc_compact(&mut best_doc)?;
    let optimised_fsm = fsm::print(&best_doc);

    let mut parameters = Vec::with_capacity(n);
    for meta in &metas {
        let final_val = read_param_value(&best_doc, &meta.addr).map_err(SetupError)?;
        let delta_pct = if meta.initial.abs() > 1e-12 {
            100.0 * (final_val - meta.initial) / meta.initial.abs()
        } else {
            0.0
        };
        parameters.push(OptParamSummary {
            param: meta.name.clone(),
            initial: meta.initial,
            final_value: final_val,
            min_bound: meta.lo,
            max_bound: meta.hi,
            delta_pct,
        });
    }

    let objective_change_pct = if initial_objective.abs() > 1e-12 {
        100.0 * (best.objective - initial_objective) / initial_objective.abs()
    } else {
        0.0
    };
    let elapsed_ms = clock() - t0;

    Ok((
        best_doc,
        OptimisationReport {
            objective: spec.objective,
            objective_unit: spec.objective.unit().to_string(),
            method: "trust_region_sqp_adjoint".to_string(),
            sensitivity_method,
            iterations: iters_done,
            solves_run: total_solves,
            converged: converged || best.verification.all_passed,
            initial_objective,
            final_objective: best.objective,
            objective_change_pct,
            initial_verification,
            final_verification: best.verification,
            parameters,
            history,
            optimised_fsm,
            h: locked_h,
            elapsed_ms,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_clock() -> f64 {
        0.0
    }

    #[test]
    fn repairs_overstressed_cantilever_to_minimum_mass_contract_boundary() {
        let src = "\
fieldsmith 1
name \"Overstressed Cantilever\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let b = 10
let d = 5
let tip_f = -300
box beam size=(60, b, d) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, tip_f)
assert \"Peak stress limit\" metric=\"max_vm_elem\" target=\"<= 135\" unit=\"MPa\"
";
        let doc = fsm::parse(src).expect("parse");
        let mut bounds = BTreeMap::new();
        bounds.insert("d".to_string(), [4.0, 16.0]);
        let spec = OptimiseSpec {
            objective: OptObjective::MassG,
            params: vec!["d".to_string()],
            bounds,
            max_iters: Some(8),
            dofs: Some(5_000),
            h: None,
        };

        let (opt_doc, rep) =
            optimise_document(&doc, Some(src), &spec, zero_clock).expect("optimise");
        assert!(
            !rep.initial_verification.all_passed,
            "expected initial d=5 mm beam to violate <= 135 MPa: {:?}",
            rep.initial_verification
        );
        assert!(
            rep.final_verification.all_passed,
            "expected optimised beam to satisfy <= 135 MPa: {:?}",
            rep.final_verification
        );
        let d_final = opt_doc.lets.iter().find(|l| l.name == "d").unwrap().value;
        // Analytical Euler-Bernoulli stress σ = 6 F L / (b d^2) <= 135 => d >= sqrt(108000 / 1350) = 8.94 mm
        // (element-centroid P2 stress is slightly lower than outer-fibre skin stress, so d_final ~ 6.0..9.5 mm,
        // well below max_bound = 16.0 mm!).
        assert!(
            d_final > 5.3 && d_final < 11.0,
            "expected minimum-mass feasible depth d in (5.3, 11.0) mm, got {d_final}"
        );
    }

    #[test]
    fn multi_parameter_sizing_cuts_mass_and_excludes_load_lets_by_default() {
        let src = "\
fieldsmith 1
name \"Heavy Cantilever\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let b = 14
let d = 14
let tip_f = -120
box beam size=(60, b, d) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, tip_f)
assert \"Tip deflection cap\" metric=\"max_displacement\" target=\"<= 0.12\" unit=\"mm\"
assert \"Safety factor\" metric=\"min_safety_factor\" target=\">= 2.0\"
";
        let doc = fsm::parse(src).expect("parse");
        let mut bounds = BTreeMap::new();
        bounds.insert("b".to_string(), [5.0, 16.0]);
        bounds.insert("d".to_string(), [6.0, 18.0]);
        let spec = OptimiseSpec {
            objective: OptObjective::MassG,
            params: Vec::new(), // auto-discovers `b` and `d`, automatically excluding load `tip_f`!
            bounds,
            max_iters: Some(8),
            dofs: Some(4_500),
            h: None,
        };

        let (opt_doc, rep) =
            optimise_document(&doc, Some(src), &spec, zero_clock).expect("optimise");
        let param_names: Vec<&str> = rep.parameters.iter().map(|p| p.param.as_str()).collect();
        assert_eq!(param_names, vec!["b", "d"]);
        assert!(rep.final_verification.all_passed);
        assert!(
            rep.objective_change_pct < -25.0,
            "expected > 25% mass reduction from overbuilt 14x14 beam, got {:.2}%",
            rep.objective_change_pct
        );
        let b_final = opt_doc.lets.iter().find(|l| l.name == "b").unwrap().value;
        let d_final = opt_doc.lets.iter().find(|l| l.name == "d").unwrap().value;
        assert!(
            b_final < d_final,
            "expected 3D adjoints to slim width b ({b_final:.2}) below depth d ({d_final:.2})"
        );
    }

    #[test]
    fn coupled_1d_3d_pipe_repairs_thermal_contract_and_reduces_wall_mass() {
        let src = "\
fieldsmith 1
name \"Coupled Cooled Pipe Synthesis\"
material ss316l
let r_out = 13
cylinder wall r=r_out h=40 at=(0, 0, 20)
cylinder bore r=6 h=44 at=(0, 0, 20) subtract
region mount = wall.bottom
region bore_wall = bore.side
region outer_wall = wall.side
support fix mount fixed=xyz
load bore_p bore_wall pressure=2.0 case=\"Operating\"
convection cool bore_wall h=0.005 tinf=25 case=\"Thermal\"
heatflux q_ext outer_wall q=0.04 case=\"Thermal\"
solve tref=25
assert \"Peak 3D wall temp\" metric=\"max_temp\" target=\"<= 54\" unit=\"C\"
assert \"Safety factor\" metric=\"min_safety_factor\" target=\">= 2.0\"
system pipe_1d title=\"1D Coolant\" sector=\"Thermal\" nodes=21 span=(0, 40) axis=z {
  design mdot nominal=0.018 min=0.015 max=0.14 unit=\"kg/s\" symbol=\"m_dot\" category=process
  state T_f init=\"25\" scale=50 unit=\"C\" symbol=\"T_f\"
  aux cp expr=\"4180\"
  aux h_film expr=\"0.008 * pow(mdot / 0.05, 0.8)\"
  aux q_lin expr=\"35.0\"
  residual T_f eq=\"mdot * cp * d_ds(T_f) - q_lin\" bc_left=\"T_f - 25\"
  objective mdot_min expr=\"mdot\" reduce=value kind=minimize unit=\"kg/s\" symbol=\"m_dot\"
  bind cool attr=h expr=\"h_film\"
  bind cool attr=tinf expr=\"T_f\"
}
";
        let doc = fsm::parse(src).expect("parse");
        let mut bounds = BTreeMap::new();
        bounds.insert("r_out".to_string(), [8.0, 14.0]);
        let spec = OptimiseSpec {
            objective: OptObjective::MassG,
            params: vec!["r_out".to_string(), "mdot".to_string()],
            bounds,
            max_iters: Some(8),
            dofs: Some(3_000),
            h: None,
        };

        let (opt_doc, rep) =
            optimise_document(&doc, Some(src), &spec, zero_clock).expect("optimise coupled 1D+3D");
        assert!(
            !rep.initial_verification.all_passed,
            "expected initial mdot=0.018 kg/s to exceed 54 °C 3D wall temp: {:?}",
            rep.initial_verification
        );
        assert!(
            rep.final_verification.all_passed,
            "expected coupled 1D+3D optimiser to satisfy <= 54 °C & SF >= 2: {:?}",
            rep.final_verification
        );
        assert!(
            rep.final_objective < rep.initial_objective,
            "expected 3D pipe mass to decrease ({:.2} -> {:.2} g)",
            rep.initial_objective,
            rep.final_objective
        );
        let mdot_final = opt_doc.system_block.as_ref().unwrap().designs[0].nominal;
        assert!(
            mdot_final > 0.022,
            "expected 1D->3D thermal adjoint to raise mdot above 0.022 kg/s, got {mdot_final}"
        );
    }

    #[test]
    fn optimise_repairs_resonance_frequency_and_stress_contracts_for_minimum_mass() {
        // Notice `solve modes` is intentionally omitted: `optimise_document` auto-infers
        // `modes_count = 1` from `metric="mode_1"`!
        let src = "\
fieldsmith 1
name \"Resonance & Stress Cantilever\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let b = 12
let d = 4.5
let tip_f = -200
box beam size=(60, b, d) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, tip_f)
assert \"Fundamental resonance floor\" metric=\"mode_1\" target=\">= 1650\" unit=\"Hz\"
assert \"Peak stress limit\" metric=\"max_vm_elem\" target=\"<= 140\" unit=\"MPa\"
";
        let doc = fsm::parse(src).expect("parse");
        let mut bounds = BTreeMap::new();
        bounds.insert("b".to_string(), [5.0, 16.0]);
        bounds.insert("d".to_string(), [4.0, 16.0]);
        let spec = OptimiseSpec {
            objective: OptObjective::MassG,
            params: vec!["b".to_string(), "d".to_string()],
            bounds,
            max_iters: Some(8),
            dofs: Some(4_500),
            h: None,
        };
        let (opt_doc, rep) =
            optimise_document(&doc, Some(src), &spec, zero_clock).expect("optimise resonance");
        assert!(
            !rep.initial_verification.all_passed,
            "expected initial d=4.5 mm to fail f1 >= 1650 Hz: {:?}",
            rep.initial_verification
        );
        assert!(
            rep.final_verification.all_passed,
            "expected modal+mechanical adjoint optimiser to satisfy f1 >= 1650 Hz & σ_vM <= 140 MPa: {:?}",
            rep.final_verification
        );
        let b_final = opt_doc.lets.iter().find(|l| l.name == "b").unwrap().value;
        let d_final = opt_doc.lets.iter().find(|l| l.name == "d").unwrap().value;
        assert!(
            d_final > 6.5 && b_final < 12.0,
            "expected optimiser to thicken Z-depth d ({d_final}) and narrow Y-width b ({b_final}) for minimum mass"
        );
    }

    #[test]
    fn optimise_maximises_fundamental_frequency_under_mass_budget() {
        let src = "\
fieldsmith 1
name \"Max Resonance Pure Modal Beam\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let b = 12
let d = 5
box beam size=(60, b, d) at=(30, 0, 0)
region root = beam.nx
support clamp root fixed=xyz
assert \"Mass budget\" metric=\"mass_g\" target=\"<= 29.0\" unit=\"g\"
";
        let doc = fsm::parse(src).expect("parse");
        let mut bounds = BTreeMap::new();
        bounds.insert("b".to_string(), [6.0, 14.0]);
        bounds.insert("d".to_string(), [4.0, 11.0]);
        let spec = OptimiseSpec {
            objective: OptObjective::MaxMode1Hz,
            params: vec!["b".to_string(), "d".to_string()],
            bounds,
            max_iters: Some(8),
            dofs: Some(4_500),
            h: None,
        };
        let (_opt_doc, rep) =
            optimise_document(&doc, Some(src), &spec, zero_clock).expect("maximise mode_1");
        assert!(
            rep.final_verification.all_passed,
            "expected mass budget <= 29 g to hold: {:?}",
            rep.final_verification
        );
        assert!(
            rep.final_objective > 1.35 * rep.initial_objective,
            "expected f1 to increase by > 35% ({:.1} -> {:.1} Hz), got {:.1}%",
            rep.initial_objective,
            rep.final_objective,
            rep.objective_change_pct
        );
    }
}
