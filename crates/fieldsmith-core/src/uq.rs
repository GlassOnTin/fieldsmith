//! `O(1)`-solve 3D continuum epistemic uncertainty quantification (`QuestionSpec::Uq`),
//! variance attribution, and Gaussian quantile (`p10`, `p50`, `p90`) propagation.
//!
//! Propagates arbitrary numbers of independent epistemic or manufacturing tolerances
//! (`doc.uncertainties` plus any `sigma > 0` `epistemic` closures declared inside a `.fsm`
//! `system` block) through the 3D discrete adjoint (`boundary_velocity_adjoint`) in `O(1)` linear
//! FEA solves: the 2–4 adjoint right-hand sides are solved once on the baseline P2 mesh, and each
//! uncertain parameter contributes via cheap element/surface residual inner products `λᵀ r_i`.

use crate::contracts::{
    is_graded_assert_metric, required_modes_for_asserts, resolve_assert_metric,
};
use crate::declarative_system::resolve_system_doc;
use crate::fill::{self, Graded};
use crate::optimise::metric_derivative_for_param;
use crate::sensitivity::{
    compute_sensitivities_graded_with_base, compute_sensitivities_with_base, is_solve_param_target,
    SensitivityReport,
};
use crate::{analyse_full, Analysis, Clock, Resolution, SetupError};
use fieldsmith_doc::fsm::LiterateAssert;
use fieldsmith_doc::param::ParamAddr;
use fieldsmith_doc::{Document, UncertaintySpec};
use serde::{Deserialize, Serialize};

/// Standard normal 90th-percentile quantile `z_0.90 = Φ⁻¹(0.90) = -Φ⁻¹(0.10)`.
pub const Z_90: f64 = 1.281_551_565_544_600_4;

/// Supported quantile contract prefixes (`p10.<metric>` and `p90.<metric>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuantileKind {
    P10,
    P90,
}

impl QuantileKind {
    #[inline]
    pub fn z_score(self) -> f64 {
        match self {
            Self::P10 => -Z_90,
            Self::P90 => Z_90,
        }
    }
}

/// Strip an optional `p90.` or `p10.` prefix from a contract metric name.
pub fn strip_quantile_prefix(metric: &str) -> (Option<QuantileKind>, &str) {
    let m = metric.trim();
    if let Some(rest) = m.strip_prefix("p90.") {
        (Some(QuantileKind::P90), rest.trim())
    } else if let Some(rest) = m.strip_prefix("p10.") {
        (Some(QuantileKind::P10), rest.trim())
    } else {
        (None, m)
    }
}

/// Whether any contract in `asserts` requests a `p90.<metric>` or `p10.<metric>` quantile.
pub fn has_quantile_asserts(asserts: &[LiterateAssert]) -> bool {
    asserts
        .iter()
        .any(|a| strip_quantile_prefix(&a.metric).0.is_some())
}

/// Specification for [`compute_uq`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UqSpec {
    /// Optional explicit uncertainty list; when empty, merges `doc.uncertainties` with any
    /// `sigma > 0` `epistemic` closures declared in `doc.system_block`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncertainties: Vec<UncertaintySpec>,
    /// Optional metric names (`max_vm_elem`, `max_displacement`, `compliance`,
    /// `min_safety_factor`, `mass_g`, `max_temp`, `max_layer_peel`, `overhang_fraction`,
    /// `mode_1`, `graded.yield_utilization`, etc.). When empty, selects active metrics for the
    /// solved model plus any base metrics referenced in `doc.asserts`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dofs: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
}

/// Variance attribution entry for one uncertain input parameter on one output metric.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UqContributor {
    pub param: String,
    pub label: String,
    pub nominal: f64,
    pub sigma: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    /// Local sensitivity `∂M / ∂p_i` evaluated at nominal.
    pub sensitivity: f64,
    /// Absolute 1-σ contribution `|σ_i · (∂M / ∂p_i)|` in the metric's units.
    pub std_contribution: f64,
    /// Fractional variance share `(σ_i ∂M/∂p_i)² / ∑_j (σ_j ∂M/∂p_j)² ∈ [0, 1]`.
    pub variance_share: f64,
}

/// First-order second-moment (FOSM) Gaussian uncertainty summary for one 3D metric.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UqMetricSummary {
    pub metric: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    pub p50: f64,
    pub std_dev: f64,
    pub p10: f64,
    pub p90: f64,
    /// Ranked descending by `variance_share`.
    pub contributors: Vec<UqContributor>,
}

/// Complete 3D epistemic uncertainty report returned by [`compute_uq`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UqReport {
    pub method: String,
    pub solves_run: usize,
    pub h: f64,
    pub nodes: usize,
    pub elements: usize,
    pub inputs: Vec<UncertaintySpec>,
    pub metrics: Vec<UqMetricSummary>,
    pub elapsed_ms: f64,
}

/// Merge `explicit` uncertainties (if non-empty) or `doc.uncertainties` + `doc.system_block`
/// `epistemic` parameters (`sigma > 0`).
pub fn collect_effective_uncertainties(
    doc: &Document,
    explicit: &[UncertaintySpec],
) -> Vec<UncertaintySpec> {
    if !explicit.is_empty() {
        return explicit.to_vec();
    }
    let mut out = doc.uncertainties.clone();
    if let Some(sb) = &doc.system_block {
        for ep in &sb.epistemics {
            if ep.sigma.is_finite()
                && ep.sigma > 0.0
                && !out.iter().any(|u| u.param.trim() == ep.name)
            {
                let label = if ep.mechanism.trim().is_empty() {
                    ep.name.clone()
                } else {
                    ep.mechanism.trim().to_string()
                };
                out.push(UncertaintySpec {
                    label,
                    param: ep.name.clone(),
                    std: Some(ep.sigma),
                    cv: None,
                    unit: ep.unit.clone(),
                });
            }
        }
    }
    out
}

/// Canonical engineering unit string for a supported 3D metric.
pub fn default_metric_unit(metric: &str) -> &'static str {
    let (_, base) = strip_quantile_prefix(metric);
    if base.starts_with("mode_") && base != "mode_split_12_pct" {
        return "Hz";
    }
    let rest = base
        .strip_prefix("graded.")
        .or_else(|| base.strip_prefix("solid."))
        .unwrap_or(base);
    let field = rest.split_once(':').map(|(f, _)| f.trim()).unwrap_or(rest);
    match field {
        "mass_g" | "graded_mass_g" | "solid_mass_g" => "g",
        "mass_saved_pct" | "overhang_pct" | "mode_split_12_pct" => "%",
        "overhang_area_mm2" | "overhang_area" => "mm²",
        "max_overhang_deg" | "overhang_deg" => "deg",
        "compliance" => "N·mm",
        "max_displacement" | "max_disp" | "deflection" => "mm",
        "max_vm_elem" | "vm" | "max_von_mises" => "MPa",
        "max_temp"
        | "min_temp"
        | "hot_outlet_temp_c"
        | "hx_hot_outlet_temp_c"
        | "cold_outlet_temp_c"
        | "hx_cold_outlet_temp_c" => "°C",
        "hx_pressure_drop_kpa" | "pressure_drop_kpa" | "hx_cold_pressure_drop_kpa" => "kPa",
        "hx_pumping_power_w" | "pumping_power_w" | "hx_hot_pumping_power_w" | "hx_heat_duty_w" => {
            "W"
        }
        "hx_ua_w_k" => "W/K",
        "hx_power_density_kw_l" => "kW/L",
        "hx_permeability_mm2" => "mm²",
        "hx_area_density_per_m" => "1/m",
        "hx_wetted_area_cm2" => "cm²",
        "hx_hydraulic_diam_mm" => "mm",
        "hx_darcy_share_pct" => "%",
        _ => "",
    }
}

fn push_unique_metric(list: &mut Vec<String>, m: &str) {
    let trimmed = m.trim();
    if !trimmed.is_empty() && !list.iter().any(|x| x == trimmed) {
        list.push(trimmed.to_string());
    }
}

/// Select active default metrics for `doc` when `UqSpec::metrics` is omitted.
pub fn default_uq_metrics(doc: &Document, a: &Analysis, g: Option<&Graded>) -> Vec<String> {
    let mut list = Vec::new();
    push_unique_metric(&mut list, "mass_g");
    if let Some(gr) = g {
        if !gr.cases.is_empty() {
            if gr.yield_utilization.iter().any(|u| u.is_some()) {
                push_unique_metric(&mut list, "graded.yield_utilization");
            }
            push_unique_metric(&mut list, "graded.buckling_utilization");
            push_unique_metric(&mut list, "graded.deflection");
            push_unique_metric(&mut list, "graded.max_vm_elem");
            if doc.solve.build.is_some() {
                push_unique_metric(&mut list, "graded.max_layer_peel");
            }
        }
        if !gr.thermal_cases.is_empty() {
            push_unique_metric(&mut list, "graded.max_temp");
        }
        if gr.hx.is_some() {
            push_unique_metric(&mut list, "hx_heat_duty_w");
            push_unique_metric(&mut list, "hx_pressure_drop_kpa");
            push_unique_metric(&mut list, "hx_effectiveness");
        }
    } else {
        let has_mech_or_thermo_disp = a
            .cases
            .iter()
            .any(|c| c.temperature.is_none() || c.max_vm_elem > 0.0 || c.max_displacement > 0.0);
        if has_mech_or_thermo_disp {
            push_unique_metric(&mut list, "max_vm_elem");
            push_unique_metric(&mut list, "max_displacement");
            if a.cases.iter().any(|c| c.compliance.abs() > 0.0) {
                push_unique_metric(&mut list, "compliance");
            }
            if a.yield_strength > 0.0 {
                push_unique_metric(&mut list, "min_safety_factor");
            }
        }
        if a.cases.iter().any(|c| c.temperature.is_some()) {
            push_unique_metric(&mut list, "max_temp");
        }
        if doc.solve.build.is_some() {
            if !a.cases.is_empty() {
                push_unique_metric(&mut list, "max_layer_peel");
            }
            push_unique_metric(&mut list, "overhang_fraction");
        }
    }
    if let Some(modes) = &a.modes {
        if !modes.frequencies.is_empty() {
            push_unique_metric(&mut list, "mode_1");
        }
    }
    for asrt in &doc.asserts {
        let (_, base_m) = strip_quantile_prefix(&asrt.metric);
        if base_m != "resultant_mag" && base_m != "max_plastic_strain" && base_m != "plastic_strain"
        {
            push_unique_metric(&mut list, base_m);
        }
    }
    list
}

fn is_fill_uq_addr(doc: &Document, addr: &ParamAddr) -> bool {
    addr.id == "fill"
        && !addr.attr.is_empty()
        && doc.find("fill").is_none()
        && doc.custom_materials.iter().all(|m| m.id != "fill")
        && doc.loads.iter().all(|l| l.id != "fill")
        && doc.temperatures.iter().all(|t| t.id != "fill")
        && doc.convections.iter().all(|c| c.id != "fill")
        && doc.heat_fluxes.iter().all(|f| f.id != "fill")
}

/// Pure FOSM algebra over already-computed adjoint sensitivities (`0` FEA solves).
#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_uq_from_sensitivities(
    doc: &Document,
    a: &Analysis,
    g: Option<&Graded>,
    sens: &SensitivityReport,
    solid_sens: Option<&SensitivityReport>,
    inputs: &[UncertaintySpec],
    metrics: &[String],
    solves_run: usize,
    elapsed_ms: f64,
) -> Result<UqReport, SetupError> {
    let mut summaries = Vec::with_capacity(metrics.len());
    for raw_metric in metrics {
        let (_, base_m) = strip_quantile_prefix(raw_metric);
        let p50 = resolve_assert_metric(base_m, a, g).map_err(SetupError)?;
        let mut contributors = Vec::with_capacity(inputs.len());
        let mut var_sum = 0.0f64;
        for u in inputs {
            let pname = u.param.trim();
            let ps = sens
                .parameters
                .iter()
                .find(|p| p.param == pname)
                .ok_or_else(|| {
                    SetupError(format!(
                        "uq: missing sensitivity entry for uncertain parameter '{pname}'"
                    ))
                })?;
            let solid_ps =
                solid_sens.and_then(|sr| sr.parameters.iter().find(|p| p.param == pname));
            let nominal = ps.nominal;
            let sigma = u.effective_sigma(nominal);
            let dm_dp = metric_derivative_for_param(doc, base_m, a, g, ps, solid_ps)?;
            let std_c = (sigma * dm_dp).abs();
            var_sum += std_c * std_c;
            contributors.push(UqContributor {
                param: pname.to_string(),
                label: if u.label.trim().is_empty() {
                    pname.to_string()
                } else {
                    u.label.clone()
                },
                nominal,
                sigma,
                unit: u.unit.clone(),
                sensitivity: dm_dp,
                std_contribution: std_c,
                variance_share: 0.0,
            });
        }
        let std_dev = var_sum.sqrt();
        for c in &mut contributors {
            c.variance_share = if var_sum > 1e-30 {
                (c.std_contribution * c.std_contribution) / var_sum
            } else {
                0.0
            };
        }
        contributors.sort_by(|a, b| {
            b.variance_share
                .total_cmp(&a.variance_share)
                .then_with(|| a.param.cmp(&b.param))
        });
        let mut unit_str = default_metric_unit(base_m).to_string();
        if unit_str.is_empty() {
            if let Some(ref vis) = a.system {
                if let Some(oname) = base_m
                    .strip_prefix("sys.")
                    .or_else(|| base_m.strip_prefix("sys_"))
                {
                    if let Some(o) = vis.objectives.iter().find(|o| o.name == oname.trim()) {
                        unit_str = o.unit.clone();
                    }
                } else if let Some(rest) = base_m.strip_prefix("section.") {
                    let sname = rest.split_once('.').map(|(n, _)| n).unwrap_or(rest).trim();
                    if let Some(s) = vis.sections.iter().find(|s| s.name == sname) {
                        unit_str = s.unit.clone();
                    }
                }
            }
        }
        summaries.push(UqMetricSummary {
            metric: base_m.to_string(),
            unit: unit_str,
            p50,
            std_dev,
            p10: p50 - Z_90 * std_dev,
            p90: p50 + Z_90 * std_dev,
            contributors,
        });
    }

    Ok(UqReport {
        method: sens.method.clone(),
        solves_run,
        h: sens.h,
        nodes: sens.nodes,
        elements: sens.elements,
        inputs: inputs.to_vec(),
        metrics: summaries,
        elapsed_ms,
    })
}

/// Compute FOSM uncertainty propagation reusing an already-solved primal `(a, g)`.
#[allow(clippy::too_many_arguments)]
pub fn compute_uq_with_base(
    doc: &Document,
    sys_doc: &Document,
    a: &Analysis,
    g: Option<&Graded>,
    explicit_uncertainties: &[UncertaintySpec],
    explicit_metrics: &[String],
    base_solves: usize,
    t0: f64,
    clock: Clock,
) -> Result<UqReport, SetupError> {
    let inputs = collect_effective_uncertainties(doc, explicit_uncertainties);
    if inputs.is_empty() {
        return Err(SetupError(
            "uq: document declares no epistemic uncertainties \
             (add `uncertainty \"...\" param=\"...\" std=...` or `system { epistemic ... }`)"
                .into(),
        ));
    }
    let mut param_names: Vec<String> = Vec::with_capacity(inputs.len());
    for u in &inputs {
        let p = u.param.trim();
        if p.is_empty() {
            return Err(SetupError("uq: uncertainty param cannot be empty".into()));
        }
        ParamAddr::parse(p).map_err(SetupError)?;
        if !param_names.iter().any(|x| x == p) {
            param_names.push(p.to_string());
        }
    }

    let metrics: Vec<String> = if explicit_metrics.is_empty() {
        default_uq_metrics(doc, a, g)
    } else {
        explicit_metrics
            .iter()
            .map(|m| strip_quantile_prefix(m).1.to_string())
            .collect()
    };

    let am_needed = doc.solve.build.is_some()
        || metrics
            .iter()
            .any(|m| m.contains("peel") || m.contains("overhang"))
        || param_names.iter().any(|p| {
            ParamAddr::parse(p).is_ok_and(|addr| {
                is_solve_param_target(doc, &addr)
                    || matches!(addr.attr.as_str(), "yield_z" | "shear_z")
            })
        });

    let sens_doc: std::borrow::Cow<'_, Document> = if am_needed && doc.solve.build.is_none() {
        let mut d = doc.clone();
        d.solve.build = Some([0.0, 0.0]);
        std::borrow::Cow::Owned(d)
    } else {
        std::borrow::Cow::Borrowed(doc)
    };

    let sens = if let Some(gr) = g {
        compute_sensitivities_graded_with_base(
            sens_doc.as_ref(),
            sys_doc,
            a,
            gr,
            &param_names,
            clock(),
            clock,
        )?
    } else {
        compute_sensitivities_with_base(
            sens_doc.as_ref(),
            sys_doc,
            a,
            &param_names,
            clock(),
            clock,
        )?
    };
    let mut total_solves = base_solves + sens.solves_run.saturating_sub(base_solves);

    let need_solid_sens = g.is_some()
        && metrics.iter().any(|m| {
            let (_, bm) = strip_quantile_prefix(m);
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
    let non_fill_params: Vec<String> = param_names
        .iter()
        .filter(|p| ParamAddr::parse(p).is_ok_and(|addr| !is_fill_uq_addr(doc, &addr)))
        .cloned()
        .collect();
    let solid_sens = if need_solid_sens && !non_fill_params.is_empty() {
        let mut s_doc = sens_doc.as_ref().clone();
        s_doc.fill = None;
        let mut s_sys_doc = sys_doc.clone();
        s_sys_doc.fill = None;
        let sr = compute_sensitivities_with_base(
            &s_doc,
            &s_sys_doc,
            a,
            &non_fill_params,
            clock(),
            clock,
        )?;
        let solid_base_solves = {
            let sdoc = crate::structural_doc(&s_sys_doc);
            (crate::case_names(sdoc.as_ref()).len()
                + sdoc.thermal_cases().len()
                + usize::from(a.modes.is_some()))
            .max(1)
        };
        total_solves += sr.solves_run.saturating_sub(solid_base_solves);
        Some(sr)
    } else {
        None
    };

    evaluate_uq_from_sensitivities(
        doc,
        a,
        g,
        &sens,
        solid_sens.as_ref(),
        &inputs,
        &metrics,
        total_solves,
        clock() - t0,
    )
}

/// Run the full 3D epistemic uncertainty pipeline (`O(1)` linear FEA solves across any number of
/// uncertain parameters).
pub fn compute_uq(doc: &Document, spec: &UqSpec, clock: Clock) -> Result<UqReport, SetupError> {
    let t0 = clock();
    let inputs = collect_effective_uncertainties(doc, &spec.uncertainties);
    if inputs.is_empty() {
        return Err(SetupError(
            "uq: document declares no epistemic uncertainties \
             (add `uncertainty \"...\" param=\"...\" std=...` or `system { epistemic ... }`)"
                .into(),
        ));
    }

    let res = match (spec.h, spec.dofs) {
        (Some(h), _) => Resolution::Size(h),
        (None, Some(d)) => Resolution::Dofs(d),
        (None, None) => Resolution::Dofs(25_000),
    };

    let mut synthetic_asserts = doc.asserts.clone();
    for m in &spec.metrics {
        synthetic_asserts.push(LiterateAssert {
            label: m.clone(),
            metric: m.clone(),
            target: "0".into(),
            tolerance: String::new(),
            unit: String::new(),
        });
    }
    let modes_count = required_modes_for_asserts(doc.solve.modes, &synthetic_asserts);

    let am_in_req = spec
        .metrics
        .iter()
        .any(|m| m.contains("peel") || m.contains("overhang"))
        || inputs.iter().any(|u| {
            ParamAddr::parse(u.param.trim()).is_ok_and(|addr| {
                is_solve_param_target(doc, &addr)
                    || matches!(addr.attr.as_str(), "yield_z" | "shear_z")
            })
        });

    let solve_doc: std::borrow::Cow<'_, Document> = if am_in_req && doc.solve.build.is_none() {
        let mut d = doc.clone();
        d.solve.build = Some([0.0, 0.0]);
        std::borrow::Cow::Owned(d)
    } else {
        std::borrow::Cow::Borrowed(doc)
    };

    let sys_cow = resolve_system_doc(solve_doc.as_ref())?;
    let a = analyse_full(sys_cow.as_ref(), res, clock, modes_count, false)?;
    let g = if sys_cow.fill.is_some() {
        Some(fill::graded_of(sys_cow.as_ref(), &a, clock, false)?)
    } else {
        None
    };

    let sdoc = crate::structural_doc(sys_cow.as_ref());
    let static_solves = crate::case_names(sdoc.as_ref()).len() + sdoc.thermal_cases().len();
    let base_solves = if g.is_some() {
        (2 * static_solves + usize::from(a.modes.is_some())).max(1)
    } else {
        (static_solves + usize::from(a.modes.is_some())).max(1)
    };

    compute_uq_with_base(
        solve_doc.as_ref(),
        sys_cow.as_ref(),
        &a,
        g.as_ref(),
        &spec.uncertainties,
        &spec.metrics,
        base_solves,
        t0,
        clock,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldsmith_doc::fsm;

    fn zero_clock() -> f64 {
        0.0
    }

    #[test]
    fn cantilever_fosm_uq_matches_closed_form_gaussian_propagation_in_o1_solves() {
        // Euler–Bernoulli rectangular cantilever:
        //   compliance C = F² L³ / (3 E I) ∝ F² / (E · d³)
        //   bending stress σ ∝ F / d²
        // Relative 1-σ elasticities:
        //   ∂ln(C)/∂ln(F) = +2,  ∂ln(C)/∂ln(E) = -1,  ∂ln(C)/∂ln(d) = -3
        //   σ_C / C = √((2 cv_F)² + (1 cv_E)² + (3 σ_d / d)²)
        let src = "\
fieldsmith 1
name \"Analytical Epistemic Cantilever\"
material ss316l
let d = 10
box beam size=(80, d, 12) at=(40, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load pull tip force=(0, -600, 0)
uncertainty \"Machining depth\" param=d std=0.20 unit=\"mm\"
uncertainty \"Tip load jitter\" param=\"pull.force.1\" cv=0.05 unit=\"N\"
uncertainty \"Modulus scatter\" param=\"ss316l.e\" cv=0.04 unit=\"MPa\"
uncertainty \"Yield scatter\" param=\"ss316l.yield\" cv=0.06 unit=\"MPa\"
";
        let doc = fsm::parse(src).expect("parse cantilever uq");
        let rep = compute_uq(
            &doc,
            &UqSpec {
                uncertainties: Vec::new(),
                metrics: vec![
                    "compliance".into(),
                    "max_vm_elem".into(),
                    "min_safety_factor".into(),
                ],
                dofs: Some(12_000),
                h: None,
            },
            zero_clock,
        )
        .expect("compute_uq");

        assert_eq!(rep.method, "boundary_velocity_adjoint");
        assert!(
            rep.solves_run <= 3,
            "expected O(1) linear solves across 4 uncertain parameters, got {}",
            rep.solves_run
        );
        assert_eq!(rep.inputs.len(), 4);

        let comp = rep
            .metrics
            .iter()
            .find(|m| m.metric == "compliance")
            .expect("compliance summary");
        let cv_f = 0.05_f64;
        let cv_e = 0.04_f64;
        let rel_d = 0.20_f64 / 10.0;
        let expected_rel_std_comp =
            ((2.0 * cv_f).powi(2) + (1.0 * cv_e).powi(2) + (3.0 * rel_d).powi(2)).sqrt();
        let measured_rel_std_comp = comp.std_dev / comp.p50;
        let rel_err = (measured_rel_std_comp - expected_rel_std_comp).abs() / expected_rel_std_comp;
        assert!(
            rel_err < 0.025,
            "compliance relative std {measured_rel_std_comp:.5} vs closed-form {expected_rel_std_comp:.5} (rel_err={rel_err:.4})"
        );
        let share_sum: f64 = comp.contributors.iter().map(|c| c.variance_share).sum();
        assert!((share_sum - 1.0).abs() < 1e-9);
        assert!((comp.p90 - (comp.p50 + Z_90 * comp.std_dev)).abs() < 1e-9);
        assert!((comp.p10 - (comp.p50 - Z_90 * comp.std_dev)).abs() < 1e-9);

        // Verify yield strength scatter contributes directly to min_safety_factor variance!
        let sf = rep
            .metrics
            .iter()
            .find(|m| m.metric == "min_safety_factor")
            .expect("safety factor summary");
        let yield_contrib = sf
            .contributors
            .iter()
            .find(|c| c.param == "ss316l.yield")
            .expect("yield contributor");
        assert!(
            yield_contrib.variance_share > 0.15,
            "expected yield scatter to carry substantive safety-factor variance share, got {}",
            yield_contrib.variance_share
        );
    }

    #[test]
    fn coupled_1d_system_epistemic_and_3d_shape_uncertainty_propagate_jointly() {
        let src = "\
fieldsmith 1
name \"Epistemic Cooled Pipe\"
material cucrzr
let r_out = 12
cylinder outer r=r_out h=40 at=(0, 0, 20)
cylinder bore subtract r=7 h=44 at=(0, 0, 20)
region inner_wall = bore.side
region outer_wall = outer.side
region bot = outer.bottom
support fix bot fixed=xyz
convection cool inner_wall h=0.008 tinf=30 case=\"Hot\"
heatflux hot_load outer_wall q=0.9 case=\"Hot\"
system channel_1d nodes=5 span=(0, 40) axis=z {
  design mdot nominal=0.20 min=0.05 max=0.60 unit=\"kg/s\"
  epistemic fouling nominal=1.0 sigma=0.15 min=0.5 max=1.6 unit=\"-\" mechanism=\"Coolant film factor\"
  state t_bulk init=\"30\" scale=100 unit=\"C\"
  aux h_film expr=\"0.008 * fouling * (mdot / 0.20)^0.8\"
  residual t_bulk eq=\"d_ds_upwind(t_bulk) - 0.35 / mdot\" bc_left=\"t_bulk - 30\"
  bind cool attr=h expr=\"h_film\"
  bind cool attr=tinf expr=\"t_bulk\"
}
solve tref=20
uncertainty \"Outer radius tolerance\" param=r_out std=0.25 unit=\"mm\"
";
        let doc = fsm::parse(src).expect("parse coupled epistemic pipe");
        let rep = compute_uq(
            &doc,
            &UqSpec {
                uncertainties: Vec::new(),
                metrics: vec!["max_temp".into(), "max_vm_elem".into()],
                dofs: Some(8_000),
                h: None,
            },
            zero_clock,
        )
        .expect("coupled uq");

        assert_eq!(
            rep.inputs.len(),
            2,
            "expected merged 3D uncertainty + 1D system epistemic"
        );
        assert!(
            rep.solves_run <= 7,
            "expected O(1) thermo-elastic adjoint solves (1 primal + 2 mech + 4 thermal = 7), got {}",
            rep.solves_run
        );
        let mut doc3 = doc.clone();
        doc3.uncertainties.push(UncertaintySpec {
            label: "External heat flux jitter".into(),
            param: "hot_load.q".into(),
            std: None,
            cv: Some(0.08),
            unit: "W/mm²".into(),
        });
        let rep3 = compute_uq(
            &doc3,
            &UqSpec {
                uncertainties: Vec::new(),
                metrics: vec!["max_temp".into(), "max_vm_elem".into()],
                dofs: Some(8_000),
                h: None,
            },
            zero_clock,
        )
        .expect("coupled uq with 3 uncertainties");
        assert_eq!(
            rep3.solves_run, rep.solves_run,
            "adding more uncertain parameters must add 0 extra FEA solves"
        );
        let mt = rep
            .metrics
            .iter()
            .find(|m| m.metric == "max_temp")
            .expect("max_temp summary");
        assert!(mt.p90 > mt.p50 && mt.p50 > mt.p10);
        let foul_c = mt
            .contributors
            .iter()
            .find(|c| c.param == "fouling")
            .expect("fouling contributor");
        assert!(
            foul_c.sensitivity < 0.0,
            "higher fouling multiplier increases h_film and must lower wall temperature, got {}",
            foul_c.sensitivity
        );
        assert!(foul_c.variance_share > 0.05);
    }
}
