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
    evaluate_contracts_with_uq, is_graded_assert_metric, parse_mode_metric_index,
    required_modes_for_asserts, VerificationReport,
};
use crate::declarative_system::{resolve_system_doc, sync_system_doc_compact};
use crate::fill::{self, Graded};
use crate::sensitivity::{
    apply_sens_param, compute_sensitivities_graded_with_base, compute_sensitivities_with_base,
    default_target_params, is_solve_param_target, read_param_value, CaseSensitivity,
    ParameterSensitivity,
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
    /// Minimise worst-case inter-layer peel utilization `max_c U_{peel,c}(p)` (dimensionless).
    #[serde(rename = "max_layer_peel", alias = "layer_peel", alias = "peel")]
    MaxLayerPeel,
    /// Minimise downward surface overhang area fraction `f_overhang(p)` in `[0, 1]`.
    #[serde(
        rename = "overhang_fraction",
        alias = "overhang",
        alias = "overhang_pct"
    )]
    OverhangFraction,
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
            Self::MaxLayerPeel | Self::OverhangFraction => "",
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
    graded: Option<Graded>,
    verification: VerificationReport,
    objective: f64,
    constraints_g: Vec<f64>,
    primal_solves: usize,
    solves: usize,
}

pub(crate) fn is_fill_opt_addr(doc: &Document, addr: &ParamAddr) -> bool {
    addr.id == "fill"
        && !addr.attr.is_empty()
        && doc.find("fill").is_none()
        && doc.custom_materials.iter().all(|m| m.id != "fill")
        && doc.loads.iter().all(|l| l.id != "fill")
        && doc.temperatures.iter().all(|t| t.id != "fill")
        && doc.convections.iter().all(|c| c.id != "fill")
        && doc.heat_fluxes.iter().all(|f| f.id != "fill")
}

pub(crate) fn default_fill_param_bounds(addr: &ParamAddr, initial: f64) -> (f64, f64) {
    match (addr.attr.as_str(), addr.comp) {
        ("rho_min", None) | ("rho", Some(0)) => (0.12_f64.min(initial), 0.85_f64.max(initial)),
        ("rho_max", None) | ("rho", Some(1)) => (0.18_f64.min(initial), 0.95_f64.max(initial)),
        ("skin", None) => (0.4_f64.min(initial), (3.0 * initial).max(2.5)),
        ("skin_max", None) => (0.4_f64.min(initial), (3.0 * initial).max(3.5)),
        ("cell", None) => (
            (0.45 * initial).max(2.0).min(initial),
            (2.2 * initial).max(8.0).max(initial),
        ),
        ("cell_min", None) => (
            (0.35 * initial).max(1.5).min(initial),
            (2.0 * initial).max(6.0).max(initial),
        ),
        ("curve", None) => (0.25_f64.min(initial), 3.0_f64.max(initial)),
        ("wall" | "min_wall", None) => (
            (0.5 * initial).max(0.2).min(initial),
            (2.0 * initial).max(1.6).max(initial),
        ),
        ("margin", None) => (0.2_f64.min(initial), (2.5 * initial).max(3.0)),
        ("erode", None) => (0.0, (2.5 * initial).max(3.0)),
        ("rotate", Some(_)) => (initial - 45.0, initial + 45.0),
        _ => (0.35 * initial, 2.5 * initial),
    }
}

pub(crate) fn default_solve_param_bounds(addr: &ParamAddr, initial: f64) -> (f64, f64) {
    match (addr.attr.as_str(), addr.comp) {
        ("build", Some(0)) | ("theta" | "build_theta", None) => {
            (0.0_f64.min(initial), 90.0_f64.max(initial))
        }
        ("build", Some(1)) | ("phi" | "build_phi", None) => (initial - 180.0, initial + 180.0),
        _ => (initial - 45.0, initial + 45.0),
    }
}

fn default_optimise_params(doc: &Document, requested: &[String]) -> Vec<String> {
    if !requested.is_empty() {
        return requested.to_vec();
    }
    let candidates = default_target_params(doc, &[]);
    candidates
        .into_iter()
        .filter(|name| {
            if name.starts_with("fill.") || name.starts_with("solve.") {
                return true;
            }
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
            "optimise: document has no tunable `let`, `system`, or `fill.*` design parameters (pass `params` explicitly)"
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
        } else if is_fill_opt_addr(doc, &addr) {
            default_fill_param_bounds(&addr, initial)
        } else if is_solve_param_target(doc, &addr) {
            default_solve_param_bounds(&addr, initial)
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

fn all_graded_or_solid_cases<'a>(a: &'a Analysis, g: Option<&'a Graded>) -> Vec<&'a CaseResult> {
    if let Some(gr) = g {
        gr.cases.iter().chain(gr.thermal_cases.iter()).collect()
    } else {
        a.cases.iter().collect()
    }
}

fn eval_objective_value(
    obj: OptObjective,
    a: &Analysis,
    g: Option<&Graded>,
) -> Result<f64, SetupError> {
    let cases = all_graded_or_solid_cases(a, g);
    match obj {
        OptObjective::MassG => Ok(g.map(|gr| gr.mass_g).unwrap_or(a.mass_g)),
        OptObjective::Compliance => {
            if cases.is_empty() {
                return Err(SetupError(
                    "optimise: compliance objective requires at least one load or thermal case"
                        .into(),
                ));
            }
            Ok(cases.iter().map(|c| c.compliance).sum())
        }
        OptObjective::MaxVmElem => cases
            .iter()
            .map(|c| c.max_vm_elem)
            .reduce(f64::max)
            .ok_or_else(|| {
                SetupError("optimise: max_vm_elem objective requires at least one case".into())
            }),
        OptObjective::MaxDisplacement => cases
            .iter()
            .map(|c| c.max_displacement)
            .reduce(f64::max)
            .ok_or_else(|| {
                SetupError("optimise: max_displacement objective requires at least one case".into())
            }),
        OptObjective::MaxTemp => cases
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
        OptObjective::MaxLayerPeel => cases
            .iter()
            .map(|c| c.max_layer_peel)
            .reduce(f64::max)
            .ok_or_else(|| {
                SetupError(
                    "optimise: max_layer_peel objective requires at least one load or thermal case"
                        .into(),
                )
            }),
        OptObjective::OverhangFraction => Ok(a.overhang.overhang_fraction),
    }
}

fn eval_objective_grad_p(
    obj: OptObjective,
    a: &Analysis,
    g: Option<&Graded>,
    ps: &ParameterSensitivity,
) -> Result<f64, SetupError> {
    let cases = all_graded_or_solid_cases(a, g);
    match obj {
        OptObjective::MassG => Ok(ps.d_mass_g),
        OptObjective::Compliance => Ok(ps.cases.iter().map(|c| c.d_compliance).sum()),
        OptObjective::MaxVmElem => {
            let (idx, _) = cases
                .iter()
                .enumerate()
                .max_by(|(_, c1), (_, c2)| c1.max_vm_elem.total_cmp(&c2.max_vm_elem))
                .ok_or_else(|| SetupError("optimise: no solved cases".into()))?;
            Ok(ps.cases.get(idx).map(|c| c.d_max_vm_elem).unwrap_or(0.0))
        }
        OptObjective::MaxDisplacement => {
            let (idx, _) = cases
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
            let (idx, _) = cases
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
        OptObjective::MaxLayerPeel => {
            let (idx, _) = cases
                .iter()
                .enumerate()
                .max_by(|(_, c1), (_, c2)| c1.max_layer_peel.total_cmp(&c2.max_layer_peel))
                .ok_or_else(|| SetupError("optimise: no solved cases".into()))?;
            ps.cases
                .get(idx)
                .and_then(|c| c.d_max_layer_peel)
                .ok_or_else(|| {
                    SetupError("optimise: missing layer-peel sensitivity d_max_layer_peel".into())
                })
        }
        OptObjective::OverhangFraction => ps.d_overhang_fraction.ok_or_else(|| {
            SetupError("optimise: missing overhang sensitivity d_overhang_fraction".into())
        }),
    }
}

pub(crate) fn metric_derivative_for_param(
    doc: &Document,
    metric: &str,
    a: &Analysis,
    g: Option<&Graded>,
    ps: &ParameterSensitivity,
    solid_ps: Option<&ParameterSensitivity>,
) -> Result<f64, SetupError> {
    let m = metric.trim();
    if let Some(res) =
        crate::declarative_system::system_metric_derivative_for_param(doc, m, &ps.param, ps.nominal)
    {
        return res;
    }
    let d_sigma_y = ParamAddr::parse(&ps.param)
        .ok()
        .and_then(|addr| {
            if matches!(addr.attr.as_str(), "yield" | "yield_strength") {
                let mid = crate::sensitivity::resolve_material_param_id(doc, &addr)?;
                (mid == doc.material).then_some(1.0)
            } else {
                None
            }
        })
        .unwrap_or(0.0);
    match m {
        "mass_g" | "graded_mass_g" => return Ok(ps.d_mass_g),
        "solid_mass_g" => return Ok(ps.d_solid_mass_g.unwrap_or(ps.d_mass_g)),
        "mass_saved_pct" => {
            let gr = g.ok_or_else(|| {
                SetupError("optimise: mass_saved_pct requires a graded fill solve".into())
            })?;
            if gr.solid_mass_g <= 1e-12 {
                return Ok(0.0);
            }
            let dm_graded = ps.d_mass_g;
            let dm_solid = ps.d_solid_mass_g.unwrap_or(0.0);
            return Ok(100.0 * (gr.mass_g * dm_solid - gr.solid_mass_g * dm_graded)
                / (gr.solid_mass_g * gr.solid_mass_g));
        }
        "overhang_fraction" => {
            return ps.d_overhang_fraction.ok_or_else(|| {
                SetupError(
                    "optimise: missing overhang sensitivity d_overhang_fraction on contract".into(),
                )
            });
        }
        "overhang_pct" => {
            return ps.d_overhang_fraction.map(|d| 100.0 * d).ok_or_else(|| {
                SetupError(
                    "optimise: missing overhang sensitivity d_overhang_fraction on contract".into(),
                )
            });
        }
        "overhang_area_mm2" | "overhang_area" => {
            return ps
                .d_overhang_fraction
                .map(|d| a.overhang.total_area_mm2 * d)
                .ok_or_else(|| {
                    SetupError(
                        "optimise: missing overhang sensitivity d_overhang_fraction on contract"
                            .into(),
                    )
                });
        }
        "max_overhang_deg" | "overhang_deg" => {
            return ps.d_max_overhang_deg.ok_or_else(|| {
                SetupError(
                    "optimise: missing overhang sensitivity d_max_overhang_deg on contract".into(),
                )
            });
        }
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
    let (use_graded, rest) = if let Some(r) = m.strip_prefix("graded.") {
        (true, r)
    } else if let Some(r) = m.strip_prefix("solid.") {
        (false, r)
    } else {
        (false, m)
    };
    let (field, case_sel) = match rest.split_once(':') {
        Some((f, cs)) => (f.trim(), cs.trim()),
        None => (rest.trim(), ""),
    };
    if matches!(
        field,
        "yield_utilization" | "yield_util" | "buckling_utilization" | "buckling_util"
    ) {
        let gr = g.ok_or_else(|| {
            SetupError(format!(
                "optimise: contract '{m}' requires a graded fill solve"
            ))
        })?;
        let idx = find_case_index(&gr.cases, case_sel, false).ok_or_else(|| {
            SetupError(format!(
                "optimise: contract '{m}' matched no graded mechanical case"
            ))
        })?;
        let cs = ps.cases.get(idx).ok_or_else(|| {
            SetupError(format!(
                "optimise: missing graded sensitivity for case #{idx} on contract '{m}'"
            ))
        })?;
        return match field {
            "yield_utilization" | "yield_util" => cs.d_yield_utilization.ok_or_else(|| {
                SetupError(format!(
                    "optimise: contract '{m}' requires a material with positive yield strength"
                ))
            }),
            _ => cs.d_buckling_utilization.ok_or_else(|| {
                SetupError(format!(
                    "optimise: missing buckling utilization sensitivity on contract '{m}'"
                ))
            }),
        };
    }

    let thermal_default = matches!(field, "max_temp" | "min_temp");
    if use_graded {
        let gr = g.ok_or_else(|| {
            SetupError(format!(
                "optimise: contract '{m}' requires a graded fill solve"
            ))
        })?;
        let graded_all: Vec<CaseResult> = gr
            .cases
            .iter()
            .chain(gr.thermal_cases.iter())
            .cloned()
            .collect();
        let idx = find_case_index(&graded_all, case_sel, thermal_default).ok_or_else(|| {
            SetupError(format!("optimise: contract '{m}' matched no graded case"))
        })?;
        let cs = ps.cases.get(idx).ok_or_else(|| {
            SetupError(format!(
                "optimise: missing graded sensitivity entry for case #{idx} on contract '{m}'"
            ))
        })?;
        let bc = &graded_all[idx];
        return match field {
            "compliance" => Ok(cs.d_compliance),
            "max_displacement" | "max_disp" | "deflection" => Ok(cs.d_max_displacement),
            "max_vm_elem" | "vm" | "max_von_mises" => Ok(cs.d_max_vm_elem),
            "min_safety_factor" | "safety_factor" => {
                let denom = bc.max_vm_elem.max(1e-9);
                Ok((d_sigma_y * denom - a.yield_strength * cs.d_max_vm_elem) / (denom * denom))
            }
            "max_temp" => cs.d_max_temp.ok_or_else(|| {
                SetupError(format!(
                    "optimise: contract '{m}' requires a thermal conduction case"
                ))
            }),
            "max_layer_peel" | "layer_peel" | "peel" => cs.d_max_layer_peel.ok_or_else(|| {
                SetupError(format!(
                    "optimise: missing graded layer-peel sensitivity on contract '{m}'"
                ))
            }),
            _ => Err(SetupError(format!(
                "optimise: contract metric '{m}' is not supported by 3D adjoint differentiation"
            ))),
        };
    }

    let active_ps = if g.is_some() {
        let Some(sps) = solid_ps else {
            return Ok(0.0);
        };
        sps
    } else {
        ps
    };
    let idx = find_case_index(&a.cases, case_sel, thermal_default)
        .ok_or_else(|| SetupError(format!("optimise: contract '{m}' matched no solved case")))?;
    let cs: &CaseSensitivity = active_ps.cases.get(idx).ok_or_else(|| {
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
            Ok((d_sigma_y * denom - a.yield_strength * cs.d_max_vm_elem) / (denom * denom))
        }
        "max_temp" => cs.d_max_temp.ok_or_else(|| {
            SetupError(format!(
                "optimise: contract '{m}' requires a thermal conduction case"
            ))
        }),
        "max_layer_peel" | "layer_peel" | "peel" => cs.d_max_layer_peel.ok_or_else(|| {
            SetupError(format!(
                "optimise: missing layer-peel sensitivity on contract '{m}'"
            ))
        }),
        _ => Err(SetupError(format!(
            "optimise: contract metric '{m}' is not supported by 3D adjoint differentiation \
             (supported: mass_g, graded_mass_g, solid_mass_g, mass_saved_pct, compliance, \
              max_displacement, max_vm_elem, min_safety_factor, max_temp, max_layer_peel, \
              overhang_fraction, overhang_pct, max_overhang_deg, yield_utilization, \
              buckling_utilization, mode_k, mode_split_12_pct)"
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

pub(crate) fn apply_fill_opt_param(
    doc: &mut Document,
    addr: &ParamAddr,
    val: f64,
) -> Result<(), SetupError> {
    let f = doc
        .fill
        .as_mut()
        .ok_or_else(|| SetupError(format!("param {addr}: document has no fill")))?;
    match (addr.attr.as_str(), addr.comp) {
        ("rho_min", None) | ("rho", Some(0)) => {
            if !(val > 0.0 && val <= 1.0) {
                return Err(SetupError(format!(
                    "param {addr}: rho_min={val} out of (0, 1]"
                )));
            }
            f.rho_min = val;
        }
        ("rho_max", None) | ("rho", Some(1)) => {
            if !(val > 0.0 && val <= 1.0) {
                return Err(SetupError(format!(
                    "param {addr}: rho_max={val} out of (0, 1]"
                )));
            }
            f.rho_max = val;
        }
        ("skin", None) => {
            if val < 0.0 {
                return Err(SetupError(format!("param {addr}: skin={val} must be >= 0")));
            }
            f.skin = val;
        }
        ("skin_max", None) => {
            if val < 0.0 {
                return Err(SetupError(format!(
                    "param {addr}: skin_max={val} must be >= 0"
                )));
            }
            f.skin_max = Some(val);
        }
        ("cell", None) => {
            if val <= 0.0 {
                return Err(SetupError(format!("param {addr}: cell={val} must be > 0")));
            }
            f.cell = val;
        }
        ("cell_min", None) => {
            if val <= 0.0 {
                return Err(SetupError(format!(
                    "param {addr}: cell_min={val} must be > 0"
                )));
            }
            f.cell_min = Some(val);
        }
        ("curve", None) => {
            if val <= 0.0 {
                return Err(SetupError(format!("param {addr}: curve={val} must be > 0")));
            }
            f.curve = val;
        }
        ("wall" | "min_wall", None) => {
            if val <= 0.0 {
                return Err(SetupError(format!(
                    "param {addr}: min_wall={val} must be > 0"
                )));
            }
            f.min_wall = val;
        }
        ("margin", None) => {
            if val < 0.0 {
                return Err(SetupError(format!(
                    "param {addr}: margin={val} must be >= 0"
                )));
            }
            f.margin = val;
        }
        ("erode", None) => {
            if val < 0.0 {
                return Err(SetupError(format!(
                    "param {addr}: erode={val} must be >= 0"
                )));
            }
            f.erode = Some(val);
        }
        ("rotate", Some(i)) if i < 3 => {
            f.rotate[i] = val;
        }
        _ => {
            return Err(SetupError(format!(
                "param {addr}: unsupported fill parameter"
            )));
        }
    }
    Ok(())
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
    let mut touched_fill = false;
    for (meta, &zk) in metas.iter().zip(z) {
        let p_val = meta.unscale_unit(zk);
        if is_fill_opt_addr(base_doc, &meta.addr) {
            apply_fill_opt_param(&mut doc, &meta.addr, p_val)?;
            touched_fill = true;
        } else {
            apply_sens_param(&mut doc, &meta.addr, p_val).map_err(SetupError)?;
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
    let sys_cow = resolve_system_doc(&doc)?;
    let sys_doc = sys_cow.into_owned();

    let mut modes_count = required_modes_for_asserts(sys_doc.solve.modes, asserts);
    if obj == OptObjective::MaxMode1Hz {
        modes_count = Some(modes_count.unwrap_or(0).max(1));
    }

    let analysis = analyse_full(&sys_doc, res, clock, modes_count, false)?;
    let graded = if sys_doc.fill.is_some() {
        Some(fill::graded_of(&sys_doc, &analysis, clock, false)?)
    } else {
        None
    };

    let (verification, uq_opt) =
        evaluate_contracts_with_uq(&doc, &sys_doc, asserts, &analysis, graded.as_ref(), clock)
            .map_err(SetupError)?;
    let objective = eval_objective_value(obj, &analysis, graded.as_ref())?;
    let mut constraints_g = Vec::with_capacity(asserts.len());
    for (asrt, out) in asserts.iter().zip(&verification.outcomes) {
        let (g, _) = contract_residual_and_grad(asrt, out.value, None)?;
        constraints_g.push(g);
    }
    let sdoc = crate::structural_doc(&sys_doc);
    let static_solves = crate::case_names(sdoc.as_ref()).len() + sdoc.thermal_cases().len();
    let primal_solves = if graded.is_some() {
        (2 * static_solves + usize::from(analysis.modes.is_some())).max(1)
    } else {
        (static_solves + usize::from(analysis.modes.is_some())).max(1)
    };
    let solves = uq_opt.map(|u| u.solves_run).unwrap_or(primal_solves);
    Ok(EvalState {
        doc,
        sys_doc,
        analysis,
        graded,
        verification,
        objective,
        constraints_g,
        primal_solves,
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

/// Optimise a `.fsm` [`Document`]'s design parameters (`let` variables, `system` `design`
/// levers, and/or `fill.*` TPMS grading parameters) to minimise `spec.objective` subject to all
/// `.fsm` `assert` contracts.
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
    let default_iters = if n >= 4 { 10 } else { 8 };
    let max_iters = spec.max_iters.unwrap_or(default_iters).clamp(1, 25);

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
            .map(|(m, &zk)| {
                let val =
                    read_param_value(&cur.doc, &m.addr).unwrap_or_else(|_| m.unscale_unit(zk));
                (m.name.clone(), val)
            })
            .collect(),
        step_alpha: 0.0,
    });

    let mut best = BestState {
        doc: cur.doc.clone(),
        verification: cur.verification.clone(),
        objective: cur.objective,
        constraints_g: cur.constraints_g.clone(),
    };

    let has_quantile = crate::uq::has_quantile_asserts(&asserts);
    let uq_inputs_0 = if has_quantile {
        let list = crate::uq::collect_effective_uncertainties(doc, &[]);
        if list.is_empty() {
            return Err(SetupError(
                "optimise: quantile contract (`p90.`/`p10.`) requires at least one `uncertainty` or `system` `epistemic` parameter"
                    .into(),
            ));
        }
        list
    } else {
        Vec::new()
    };
    let mut sens_param_names = param_names.clone();
    for u in &uq_inputs_0 {
        let up = u.param.trim();
        if !sens_param_names.iter().any(|x| x == up) {
            sens_param_names.push(up.to_string());
        }
    }

    let has_build_param = sens_param_names
        .iter()
        .any(|p| ParamAddr::parse(p).is_ok_and(|addr| is_solve_param_target(doc, &addr)));
    let am_active_in_opt = has_build_param
        || matches!(
            spec.objective,
            OptObjective::MaxLayerPeel | OptObjective::OverhangFraction
        )
        || asserts
            .iter()
            .any(|a| a.metric.contains("peel") || a.metric.contains("overhang"));

    let need_solid_case_sens = cur.graded.is_some()
        && asserts.iter().any(|a| {
            let (_, m) = crate::uq::strip_quantile_prefix(&a.metric);
            !is_graded_assert_metric(m)
                && !m.starts_with("mode_")
                && !matches!(
                    m,
                    "overhang_fraction"
                        | "overhang_pct"
                        | "overhang_area_mm2"
                        | "overhang_area"
                        | "max_overhang_deg"
                        | "overhang_deg"
                )
        });
    let non_fill_param_names: Vec<String> = sens_param_names
        .iter()
        .filter(|p| ParamAddr::parse(p).is_ok_and(|addr| !is_fill_opt_addr(doc, &addr)))
        .cloned()
        .collect();

    let mut trust_delta = 0.28f64;
    let mut converged = false;
    let mut iters_done = 0usize;

    for iter in 1..=max_iters {
        let sens_doc: std::borrow::Cow<'_, Document> =
            if am_active_in_opt && cur.doc.solve.build.is_none() && cur.doc.asserts.is_empty() {
                let mut d = cur.doc.clone();
                d.asserts = asserts.clone();
                if d.solve.build.is_none() {
                    d.solve.build = Some([0.0, 0.0]);
                }
                std::borrow::Cow::Owned(d)
            } else {
                std::borrow::Cow::Borrowed(&cur.doc)
            };
        let sens = if let Some(gr) = cur.graded.as_ref() {
            compute_sensitivities_graded_with_base(
                sens_doc.as_ref(),
                &cur.sys_doc,
                &cur.analysis,
                gr,
                &sens_param_names,
                clock(),
                clock,
            )?
        } else {
            compute_sensitivities_with_base(
                sens_doc.as_ref(),
                &cur.sys_doc,
                &cur.analysis,
                &sens_param_names,
                clock(),
                clock,
            )?
        };
        sensitivity_method = sens.method.clone();
        // `sens.solves_run` includes the baseline primal solve count; add only the adjoint/perturbation solves.
        total_solves += sens.solves_run.saturating_sub(cur.primal_solves);

        let solid_sens = if need_solid_case_sens && !non_fill_param_names.is_empty() {
            let mut s_doc = sens_doc.as_ref().clone();
            s_doc.fill = None;
            let mut s_sys_doc = cur.sys_doc.clone();
            s_sys_doc.fill = None;
            let sr = compute_sensitivities_with_base(
                &s_doc,
                &s_sys_doc,
                &cur.analysis,
                &non_fill_param_names,
                clock(),
                clock,
            )?;
            let solid_base_solves = {
                let sdoc = crate::structural_doc(&s_sys_doc);
                (crate::case_names(sdoc.as_ref()).len()
                    + sdoc.thermal_cases().len()
                    + usize::from(cur.analysis.modes.is_some()))
                .max(1)
            };
            total_solves += sr.solves_run.saturating_sub(solid_base_solves);
            Some(sr)
        } else {
            None
        };

        let mut g_f = vec![0.0; n];
        for (k, meta) in metas.iter().enumerate() {
            let ps = sens
                .parameters
                .iter()
                .find(|p| p.param == meta.name)
                .ok_or_else(|| {
                    SetupError(format!("optimise: missing sensitivity for '{}'", meta.name))
                })?;
            let dj_dp =
                eval_objective_grad_p(spec.objective, &cur.analysis, cur.graded.as_ref(), ps)?;
            g_f[k] = min_sign * dj_dp * meta.span() / obj_scale;
        }

        let mut jac_g = Vec::with_capacity(asserts.len());
        for (j, asrt) in asserts.iter().enumerate() {
            let val = cur.verification.outcomes[j].value;
            let mut dv_dp = Vec::with_capacity(n);
            if let (Some(qk), base_m) = crate::uq::strip_quantile_prefix(&asrt.metric) {
                let cur_uq_inputs = crate::uq::collect_effective_uncertainties(&cur.doc, &[]);
                let mut var_sum = 0.0f64;
                let mut contribs = Vec::with_capacity(cur_uq_inputs.len());
                for u in &cur_uq_inputs {
                    let up = u.param.trim();
                    let ups = sens
                        .parameters
                        .iter()
                        .find(|p| p.param == up)
                        .ok_or_else(|| {
                            SetupError(format!("optimise: missing UQ sensitivity for '{up}'"))
                        })?;
                    let usolid = solid_sens
                        .as_ref()
                        .and_then(|sr| sr.parameters.iter().find(|p| p.param == up));
                    let sig = u.effective_sigma(ups.nominal);
                    let dm_du = metric_derivative_for_param(
                        &cur.doc,
                        base_m,
                        &cur.analysis,
                        cur.graded.as_ref(),
                        ups,
                        usolid,
                    )?;
                    var_sum += (sig * dm_du) * (sig * dm_du);
                    contribs.push((up, sig, dm_du, u));
                }
                let std_m = var_sum.sqrt();
                let z_q = qk.z_score();
                for meta in &metas {
                    let ps = sens
                        .parameters
                        .iter()
                        .find(|p| p.param == meta.name)
                        .unwrap();
                    let solid_ps = solid_sens
                        .as_ref()
                        .and_then(|sr| sr.parameters.iter().find(|p| p.param == meta.name));
                    let mut dq_dp = metric_derivative_for_param(
                        &cur.doc,
                        base_m,
                        &cur.analysis,
                        cur.graded.as_ref(),
                        ps,
                        solid_ps,
                    )?;
                    if std_m > 1e-30 {
                        for &(up, sig, dm_du, u_spec) in &contribs {
                            if up == meta.name {
                                let dsig_dp = u_spec.effective_sigma_deriv(ps.nominal);
                                if dsig_dp != 0.0 {
                                    let dstd_dsig = (sig * dm_du * dm_du) / std_m;
                                    dq_dp += z_q * dstd_dsig * dsig_dp;
                                }
                            }
                        }
                    }
                    dv_dp.push(dq_dp);
                }
            } else {
                for meta in &metas {
                    let ps = sens
                        .parameters
                        .iter()
                        .find(|p| p.param == meta.name)
                        .unwrap();
                    let solid_ps = solid_sens
                        .as_ref()
                        .and_then(|sr| sr.parameters.iter().find(|p| p.param == meta.name));
                    dv_dp.push(metric_derivative_for_param(
                        &cur.doc,
                        &asrt.metric,
                        &cur.analysis,
                        cur.graded.as_ref(),
                        ps,
                        solid_ps,
                    )?);
                }
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
        let [best_th, best_ph] = cur
            .graded
            .as_ref()
            .map(|gr| gr.best_build_angles)
            .unwrap_or(cur.analysis.best_build_angles);
        let peel_violated = asserts
            .iter()
            .zip(&cur.constraints_g)
            .any(|(a, &gj)| a.metric.contains("peel") && gj > 0.0);
        let peel_active_or_violated = spec.objective == OptObjective::MaxLayerPeel || peel_violated;
        let mut z_orient_dist = 0.0_f64;
        if has_build_param && peel_active_or_violated {
            for (m, &zk) in metas.iter().zip(&z) {
                if is_solve_param_target(doc, &m.addr) {
                    let target_z = match (m.addr.attr.as_str(), m.addr.comp) {
                        ("build", Some(0)) | ("theta" | "build_theta", None) => m.to_unit(best_th),
                        ("build", Some(1)) | ("phi" | "build_phi", None) => m.to_unit(best_ph),
                        _ => zk,
                    };
                    z_orient_dist = z_orient_dist.max((target_z - zk).abs());
                }
            }
        }
        let try_hemisphere_escape =
            has_build_param && peel_active_or_violated && z_orient_dist > 0.05;

        let max_step = step_s.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
        if max_step < 5e-4 && cur.verification.all_passed && !try_hemisphere_escape {
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
        let mut accepted_merit = f64::INFINITY;
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
                accepted_merit = trial_merit;
                accepted = Some((z_trial, trial, alpha));
                break;
            }
        }

        if try_hemisphere_escape {
            for non_build_alpha in [1.0_f64, 0.0_f64] {
                let z_hemi: Vec<f64> = metas
                    .iter()
                    .zip(&z)
                    .zip(&step_s)
                    .map(|((m, &zk), &sk)| {
                        if is_solve_param_target(doc, &m.addr) {
                            match (m.addr.attr.as_str(), m.addr.comp) {
                                ("build", Some(0)) | ("theta" | "build_theta", None) => {
                                    m.to_unit(best_th)
                                }
                                ("build", Some(1)) | ("phi" | "build_phi", None) => {
                                    m.to_unit(best_ph)
                                }
                                _ => zk,
                            }
                        } else {
                            (zk + non_build_alpha * sk).clamp(0.0, 1.0)
                        }
                    })
                    .collect();
                if non_build_alpha == 0.0 && non_fill_param_names.len() == metas.len() {
                    let only_build = metas.iter().all(|m| is_solve_param_target(doc, &m.addr));
                    if only_build {
                        continue;
                    }
                }
                let Ok(trial) = evaluate_state(
                    doc,
                    &metas,
                    &z_hemi,
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
                let ceiling = accepted_merit.min(cur_merit - 1e-6);
                if trial_merit < ceiling {
                    accepted_merit = trial_merit;
                    accepted = Some((z_hemi, trial, 1.0));
                }
            }
        }

        iters_done = iter;
        if let Some((z_next, next_state, alpha)) = accepted {
            let rel_obj_move =
                (next_state.objective - cur.objective).abs() / cur.objective.abs().max(1e-6);
            z = metas
                .iter()
                .zip(&z_next)
                .map(|(m, &zk)| {
                    read_param_value(&next_state.doc, &m.addr)
                        .map(|v| m.to_unit(v))
                        .unwrap_or(zk)
                })
                .collect();
            cur = next_state;
            history.push(OptIterationRecord {
                iter,
                objective: cur.objective,
                max_violation: max_hard_violation(&cur.constraints_g),
                all_passed: cur.verification.all_passed,
                params: metas
                    .iter()
                    .zip(&z)
                    .map(|(m, &zk)| {
                        let val = read_param_value(&cur.doc, &m.addr)
                            .unwrap_or_else(|_| m.unscale_unit(zk));
                        (m.name.clone(), val)
                    })
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

    #[test]
    fn optimise_repairs_graded_tpms_yield_and_buckling_contracts_for_min_mass() {
        let src = "\
fieldsmith 1
name \"Under-graded Gyroid Cantilever\"
material ss316l
box beam size=(60, 12, 10) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -380)
fill gyroid sheet cell=6 rho=(0.14, 0.22) skin=0.6 wall=0.4 margin=1 curve=1.2
assert \"Lattice yield utilization\" metric=\"graded.yield_utilization\" target=\"<= 0.80\"
assert \"Lattice buckling utilization\" metric=\"graded.buckling_utilization\" target=\"<= 0.80\"
assert \"Mass savings vs solid\" metric=\"mass_saved_pct\" target=\">= 25\" unit=\"%\"
";
        let doc = fsm::parse(src).expect("parse graded doc");
        let mut bounds = BTreeMap::new();
        bounds.insert("fill.rho_min".to_string(), [0.12, 0.65]);
        bounds.insert("fill.rho_max".to_string(), [0.20, 0.85]);
        bounds.insert("fill.skin".to_string(), [0.5, 2.6]);
        let spec = OptimiseSpec {
            objective: OptObjective::MassG,
            params: vec![
                "fill.rho_min".to_string(),
                "fill.rho_max".to_string(),
                "fill.skin".to_string(),
            ],
            bounds,
            max_iters: Some(8),
            dofs: Some(4_000),
            h: None,
        };
        let (opt_doc, rep) =
            optimise_document(&doc, Some(src), &spec, zero_clock).expect("optimise graded tpms");
        assert_eq!(rep.sensitivity_method, "boundary_velocity_adjoint");
        assert!(
            !rep.initial_verification.all_passed,
            "expected initial rho=(0.14, 0.22), skin=0.6 to violate lattice utilization <= 0.80: {:?}",
            rep.initial_verification
        );
        assert!(
            rep.final_verification.all_passed,
            "expected graded TPMS adjoint optimiser to satisfy yield/buckling <= 0.80 & mass_saved >= 25%: {:?}",
            rep.final_verification
        );
        let roundtrip = fsm::parse(&rep.optimised_fsm).expect("round-trip optimised .fsm");
        let f = roundtrip.fill.expect("fill preserved");
        assert_eq!(f.rho_min, opt_doc.fill.as_ref().unwrap().rho_min);
        assert!(f.rho_min <= f.rho_max);
    }

    #[test]
    fn optimise_simultaneous_outer_shape_and_graded_tpms_co_design() {
        let src = "\
fieldsmith 1
name \"Simultaneous Shape + Graded TPMS Cantilever\"
material ss316l
let d = 7.5
box beam size=(60, 12, d) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -280)
fill gyroid sheet cell=6 rho=(0.16, 0.26) skin=0.65 wall=0.4 margin=1 curve=1.2
assert \"Graded tip deflection\" metric=\"graded.deflection\" target=\"<= 0.30\" unit=\"mm\"
assert \"Lattice yield utilization\" metric=\"graded.yield_utilization\" target=\"<= 0.80\"
assert \"Mass savings vs solid\" metric=\"mass_saved_pct\" target=\">= 25\" unit=\"%\"
";
        let doc = fsm::parse(src).expect("parse co-design doc");
        let mut bounds = BTreeMap::new();
        bounds.insert("d".to_string(), [6.5, 15.0]);
        bounds.insert("fill.rho_min".to_string(), [0.14, 0.55]);
        bounds.insert("fill.rho_max".to_string(), [0.22, 0.80]);
        bounds.insert("fill.skin".to_string(), [0.5, 2.4]);
        let spec = OptimiseSpec {
            objective: OptObjective::MassG,
            params: vec![
                "d".to_string(),
                "fill.rho_min".to_string(),
                "fill.rho_max".to_string(),
                "fill.skin".to_string(),
            ],
            bounds,
            max_iters: Some(8),
            dofs: Some(4_000),
            h: None,
        };
        let (opt_doc, rep) =
            optimise_document(&doc, Some(src), &spec, zero_clock).expect("co-optimise shape+fill");
        assert_eq!(rep.sensitivity_method, "boundary_velocity_adjoint");
        assert!(
            !rep.initial_verification.all_passed,
            "expected initial d=7.5 mm graded beam to fail deflection/yield contracts: {:?}",
            rep.initial_verification
        );
        assert!(
            rep.final_verification.all_passed,
            "expected co-optimised outer shape + graded TPMS beam to pass all contracts: {:?}",
            rep.final_verification
        );
        let d_final = opt_doc.lets.iter().find(|l| l.name == "d").unwrap().value;
        assert!(
            d_final > 8.0,
            "expected outer Z-depth d to grow from 7.5 mm for bending stiffness, got {d_final}"
        );
    }

    #[test]
    fn optimise_rotates_upright_fdm_beam_and_sizes_depth_to_repair_layer_peel() {
        let src = "\
fieldsmith 1
name \"Upright FDM Cantilever Synthesis\"
material petg
let h_root = 8
box beam size=(60, 14, h_root) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -240)
solve build=(90, 0)
assert \"Inter-layer peel limit\" metric=\"max_layer_peel\" target=\"<= 0.65\"
assert \"In-plane stress limit\" metric=\"max_vm_elem\" target=\"<= 32\" unit=\"MPa\"
";
        let doc = fsm::parse(src).expect("parse upright FDM cantilever");
        let mut bounds = BTreeMap::new();
        bounds.insert("h_root".to_string(), [7.0, 20.0]);
        let spec = OptimiseSpec {
            objective: OptObjective::MassG,
            params: vec!["h_root".to_string(), "solve.build.0".to_string()],
            bounds,
            max_iters: Some(8),
            dofs: Some(4_500),
            h: None,
        };
        let (opt_doc, rep) =
            optimise_document(&doc, Some(src), &spec, zero_clock).expect("optimise FDM peel+shape");
        assert_eq!(rep.sensitivity_method, "boundary_velocity_adjoint");
        assert!(
            !rep.initial_verification.all_passed,
            "expected initial upright (90°, 0°) h_root=8 mm beam to fail peel & stress contracts: {:?}",
            rep.initial_verification
        );
        assert!(
            rep.final_verification.all_passed,
            "expected rotated + sized FDM beam to pass max_layer_peel <= 0.65 & max_vm_elem <= 32 MPa: {:?}",
            rep.final_verification
        );
        let [theta_final, _] = opt_doc.solve.build.expect("build preserved");
        let h_final = opt_doc
            .lets
            .iter()
            .find(|l| l.name == "h_root")
            .unwrap()
            .value;
        assert!(
            theta_final < 25.0,
            "expected build tilt theta to rotate from 90° toward flat XY layers (< 25°), got {theta_final}"
        );
        assert!(
            h_final > 8.2 && h_final < 16.5,
            "expected h_root to grow to minimum feasible stress depth without hitting 20 mm cap, got {h_final}"
        );
    }

    #[test]
    fn optimise_three_pillar_shape_orientation_and_graded_tpms_codesign() {
        let src = "\
fieldsmith 1
name \"Three-Pillar Shape + Orientation + Graded TPMS\"
material petg
let h_root = 10
box beam size=(60, 14, h_root) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -90)
fill gyroid sheet cell=6 rho=(0.18, 0.30) skin=0.7 wall=0.4 margin=1 curve=1.2
solve build=(90, 0)
assert \"Graded inter-layer peel\" metric=\"graded.max_layer_peel\" target=\"<= 0.65\"
assert \"Lattice yield utilization\" metric=\"graded.yield_utilization\" target=\"<= 0.78\"
assert \"Mass savings vs solid\" metric=\"mass_saved_pct\" target=\">= 22\" unit=\"%\"
";
        let doc = fsm::parse(src).expect("parse 3-pillar doc");
        let mut bounds = BTreeMap::new();
        bounds.insert("h_root".to_string(), [9.0, 18.0]);
        bounds.insert("fill.rho_min".to_string(), [0.15, 0.55]);
        bounds.insert("fill.rho_max".to_string(), [0.25, 0.85]);
        bounds.insert("fill.skin".to_string(), [0.5, 2.5]);
        let spec = OptimiseSpec {
            objective: OptObjective::MassG,
            params: vec![
                "h_root".to_string(),
                "solve.build.0".to_string(),
                "fill.rho_min".to_string(),
                "fill.rho_max".to_string(),
                "fill.skin".to_string(),
            ],
            bounds,
            max_iters: Some(10),
            dofs: Some(4_000),
            h: None,
        };
        let (opt_doc, rep) =
            optimise_document(&doc, Some(src), &spec, zero_clock).expect("3-pillar co-optimise");
        assert_eq!(rep.sensitivity_method, "boundary_velocity_adjoint");
        assert!(
            !rep.initial_verification.all_passed,
            "expected initial upright graded beam to fail peel/yield contracts: {:?}",
            rep.initial_verification
        );
        assert!(
            rep.final_verification.all_passed,
            "expected 3-pillar co-optimiser (shape + orientation + graded TPMS) to pass all contracts: {:?}",
            rep.final_verification
        );
        let [theta_final, _] = opt_doc.solve.build.expect("build preserved");
        assert!(
            theta_final < 50.0,
            "expected 3-pillar co-optimiser to rotate upright build tilt from 90° down to <= 45°, got {theta_final}"
        );
    }

    #[test]
    fn optimise_robust_p90_stress_sizes_thicker_margin_than_deterministic_p50() {
        let base_src = "\
fieldsmith 1
name \"Robust Cantilever Sizing\"
material ss316l
let d = 9.0
box beam size=(80, d, 12) at=(40, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load pull tip force=(0, -600, 0)
uncertainty \"Machining tolerance\" param=d std=0.30 unit=\"mm\"
uncertainty \"Load jitter\" param=\"pull.force.1\" cv=0.12 unit=\"N\"
assert \"Stress cap\" metric=\"max_vm_elem\" target=\"<= 190\" unit=\"MPa\"
";
        let det_doc = fsm::parse(base_src).expect("parse det doc");
        let mut bounds = BTreeMap::new();
        bounds.insert("d".to_string(), [8.5, 16.0]);
        let spec = OptimiseSpec {
            objective: OptObjective::MassG,
            params: vec!["d".to_string()],
            bounds,
            max_iters: Some(8),
            dofs: Some(6_000),
            h: None,
        };
        let (det_opt, det_rep) =
            optimise_document(&det_doc, None, &spec, zero_clock).expect("det optimise");
        assert!(det_rep.final_verification.all_passed, "{det_rep:?}");
        let d_det = det_opt.lets[0].value;

        let mut rob_doc = det_doc.clone();
        rob_doc.asserts[0].metric = "p90.max_vm_elem".into();
        let (rob_opt, rob_rep) =
            optimise_document(&rob_doc, None, &spec, zero_clock).expect("robust p90 optimise");
        assert_eq!(rob_rep.sensitivity_method, "boundary_velocity_adjoint");
        assert!(
            !rob_rep.initial_verification.all_passed,
            "expected initial d=9 mm beam to violate p90.max_vm_elem <= 190: {:?}",
            rob_rep.initial_verification
        );
        assert!(
            rob_rep.final_verification.all_passed,
            "expected robust p90 optimiser to satisfy p90.max_vm_elem <= 190: {:?}",
            rob_rep.final_verification
        );
        let d_rob = rob_opt.lets[0].value;
        assert!(
            d_rob > d_det + 0.55,
            "expected robust p90 beam depth ({d_rob:.3} mm) to carry substantial safety margin over deterministic p50 depth ({d_det:.3} mm)"
        );
    }
}
