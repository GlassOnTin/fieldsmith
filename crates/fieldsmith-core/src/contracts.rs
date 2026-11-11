//! Unified `@assert` / `assert` contract verification shared across CLI, REST API, MCP, and WASM.
//!
//! Eliminates duplicate evaluator logic between `fieldsmith-cli` and browser/MCP callers so
//! every `.fsm` document's engineering assertions produce identical pass/fail verdicts.

use crate::fill::{self, Graded};
use crate::{analyse_full, Analysis, CaseResult, Clock, Resolution};
use fieldsmith_doc::fsm::{self, LiterateAssert};
use fieldsmith_doc::Document;
use serde::{Deserialize, Serialize};

/// Evaluated result of a single `@assert` / `assert` engineering contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssertOutcome {
    pub label: String,
    pub metric: String,
    pub target: String,
    pub tolerance: String,
    pub unit: String,
    pub value: f64,
    pub error_or_margin: String,
    pub passed: bool,
}

/// Summary of evaluating all contracts on a `.fsm` document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerificationReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub all_passed: bool,
    pub outcomes: Vec<AssertOutcome>,
}

fn find_case<'a>(cases: &'a [CaseResult], selector: &str) -> Option<&'a CaseResult> {
    if selector.is_empty() {
        return cases.first();
    }
    if let Ok(idx1) = selector.parse::<usize>() {
        if idx1 >= 1 && idx1 <= cases.len() {
            return cases.get(idx1 - 1);
        }
    }
    cases
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(selector) || c.name.contains(selector))
}

/// Parse a 1-based modal index from `mode_<k>` or `mode_<k>_hz` (`k >= 1`).
pub fn parse_mode_metric_index(metric: &str) -> Option<usize> {
    let m = metric.trim();
    let rest = m.strip_prefix("mode_")?;
    let num_str = rest.strip_suffix("_hz").unwrap_or(rest);
    let idx1: usize = num_str.parse().ok()?;
    (idx1 >= 1).then_some(idx1)
}

/// Minimum modal eigenpair count required to evaluate `doc_modes` and `asserts`.
pub fn required_modes_for_asserts(
    doc_modes: Option<usize>,
    asserts: &[LiterateAssert],
) -> Option<usize> {
    let mut needed = doc_modes.unwrap_or(0);
    for a in asserts {
        let m = a.metric.trim();
        if m == "mode_split_12_pct" {
            needed = needed.max(2);
        } else if let Some(idx1) = parse_mode_metric_index(m) {
            needed = needed.max(idx1);
        }
    }
    (needed > 0).then_some(needed)
}

/// Whether an `@assert` / `assert` metric requires a [`Graded`] lattice re-solve when `doc.fill`
/// is present.
pub fn is_graded_assert_metric(metric: &str) -> bool {
    let m = metric.trim();
    if m.starts_with("graded.")
        || matches!(
            m,
            "graded_mass_g"
                | "mass_saved_pct"
                | "mass_g"
                | "solid_mass_g"
                | "yield_utilization"
                | "yield_util"
                | "buckling_utilization"
                | "buckling_util"
        )
    {
        return true;
    }
    if let Some((f, _)) = m.split_once(':') {
        return matches!(
            f.trim(),
            "yield_utilization" | "yield_util" | "buckling_utilization" | "buckling_util"
        );
    }
    false
}

/// Resolve a named contract metric (`compliance`, `max_vm_elem:2`, `graded.deflection`,
/// `graded.yield_utilization`, `graded.buckling_utilization`, `mode_1`, `mass_saved_pct`,
/// `max_plastic_strain`, etc.) from a solved `Analysis`.
pub fn resolve_assert_metric(
    metric: &str,
    a: &Analysis,
    g: Option<&Graded>,
) -> Result<f64, String> {
    let m = metric.trim();
    if m == "mode_split_12_pct" {
        let freqs = &a
            .modes
            .as_ref()
            .ok_or_else(|| format!("metric '{m}' requires a modal solve (`solve modes=...`)"))?
            .frequencies;
        if freqs.len() < 2 {
            return Err("mode_split_12_pct requires at least 2 modes".into());
        }
        let mean = 0.5 * (freqs[0] + freqs[1]).max(1e-12);
        return Ok(100.0 * (freqs[1] - freqs[0]).abs() / mean);
    }
    if m.starts_with("mode_") {
        let idx1 =
            parse_mode_metric_index(m).ok_or_else(|| format!("invalid mode metric '{m}'"))?;
        let freqs = &a
            .modes
            .as_ref()
            .ok_or_else(|| format!("metric '{m}' requires a modal solve (`solve modes=...`)"))?
            .frequencies;
        return freqs
            .get(idx1 - 1)
            .copied()
            .ok_or_else(|| format!("metric '{m}' out of range ({} modes solved)", freqs.len()));
    }
    match m {
        "mass_g" => return Ok(g.map(|gr| gr.mass_g).unwrap_or(a.mass_g)),
        "solid_mass_g" => return Ok(g.map(|gr| gr.solid_mass_g).unwrap_or(a.mass_g)),
        "graded_mass_g" => {
            return g
                .map(|gr| gr.mass_g)
                .ok_or_else(|| "graded_mass_g requires a fill statement".into())
        }
        "mass_saved_pct" => {
            let gr = g.ok_or_else(|| "mass_saved_pct requires a fill statement".to_string())?;
            if gr.solid_mass_g <= 0.0 {
                return Ok(0.0);
            }
            return Ok(100.0 * (1.0 - gr.mass_g / gr.solid_mass_g));
        }
        "min_peak_peel" => return Ok(a.min_peak_peel),
        "max_peak_peel" => return Ok(a.max_peak_peel),
        _ => {}
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
        let gr = g.ok_or_else(|| format!("metric '{m}' requires a graded fill solve"))?;
        if gr.cases.is_empty() {
            return Err(format!(
                "metric '{m}' requires at least one mechanical case"
            ));
        }
        let ci = if case_sel.is_empty() {
            Some(0)
        } else if let Ok(idx1) = case_sel.parse::<usize>() {
            (idx1 >= 1 && idx1 <= gr.cases.len()).then_some(idx1 - 1)
        } else {
            gr.cases
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(case_sel) || c.name.contains(case_sel))
        }
        .ok_or_else(|| format!("metric '{m}': no graded mechanical case matching '{case_sel}'"))?;

        return match field {
            "yield_utilization" | "yield_util" => gr
                .yield_utilization
                .get(ci)
                .and_then(|v| *v)
                .ok_or_else(|| {
                    format!("metric '{m}' requires a material with positive yield strength")
                }),
            _ => gr
                .buckling_utilization
                .get(ci)
                .copied()
                .ok_or_else(|| format!("metric '{m}': case index {ci} out of range")),
        };
    }

    let graded_cases_vec: Vec<CaseResult> = g
        .map(|gr| {
            gr.cases
                .iter()
                .chain(gr.thermal_cases.iter())
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    let pool: &[CaseResult] = if use_graded {
        if graded_cases_vec.is_empty() {
            return Err(format!("metric '{m}' requires a graded fill solve"));
        }
        &graded_cases_vec
    } else {
        &a.cases
    };

    let case = if case_sel.is_empty() && matches!(field, "max_temp" | "min_temp") {
        pool.iter().find(|c| c.temperature.is_some())
    } else {
        find_case(pool, case_sel)
    }
    .ok_or_else(|| format!("metric '{m}': no solved case matching '{case_sel}'"))?;

    match field {
        "max_displacement" | "max_disp" | "deflection" => Ok(case.max_displacement),
        "max_vm_elem" | "vm" => Ok(case.max_vm_elem),
        "max_von_mises" => Ok(case.max_von_mises),
        "min_safety_factor" | "safety_factor" => Ok(a.yield_strength / case.max_vm_elem.max(1e-12)),
        "max_layer_peel" | "peel" => Ok(case.max_layer_peel),
        "compliance" => Ok(case.compliance),
        "max_plastic_strain" | "plastic_strain" => {
            Ok(case.plastic_strain_elem.iter().copied().fold(0.0, f64::max))
        }
        "max_temp" => {
            let t = case
                .temperature
                .as_deref()
                .ok_or_else(|| format!("case '{}' has no thermal field", case.name))?;
            Ok(t.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        }
        "min_temp" => {
            let t = case
                .temperature
                .as_deref()
                .ok_or_else(|| format!("case '{}' has no thermal field", case.name))?;
            Ok(t.iter().copied().fold(f64::INFINITY, f64::min))
        }
        "resultant_mag" => {
            let r = case
                .load_reports
                .first()
                .ok_or_else(|| format!("case '{}' has no load report", case.name))?
                .resultant;
            Ok((r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt())
        }
        _ => Err(format!("unknown @assert metric '{m}'")),
    }
}

/// Evaluate a single [`LiterateAssert`] against solved solid/modal/graded results.
pub fn evaluate_assert(
    asrt: &LiterateAssert,
    a: &Analysis,
    g: Option<&Graded>,
) -> Result<(f64, String, bool), String> {
    let val = resolve_assert_metric(&asrt.metric, a, g)?;
    let tgt = asrt.target.trim();
    let tol_raw = asrt.tolerance.trim();

    if let Some(rhs) = tgt.strip_prefix("<=").or_else(|| tgt.strip_prefix('≤')) {
        let bound: f64 = rhs
            .trim()
            .parse()
            .map_err(|_| format!("bad @assert bound '{tgt}'"))?;
        let ok = val <= bound;
        let margin = format!("{:+.4}", val - bound);
        return Ok((val, margin, ok));
    }
    if let Some(rhs) = tgt.strip_prefix(">=").or_else(|| tgt.strip_prefix('≥')) {
        let bound: f64 = rhs
            .trim()
            .parse()
            .map_err(|_| format!("bad @assert bound '{tgt}'"))?;
        let ok = val >= bound;
        let margin = format!("{:+.4}", val - bound);
        return Ok((val, margin, ok));
    }
    if let Some(rhs) = tgt.strip_prefix('<') {
        let bound: f64 = rhs
            .trim()
            .parse()
            .map_err(|_| format!("bad @assert bound '{tgt}'"))?;
        let ok = val < bound;
        let margin = format!("{:+.4}", val - bound);
        return Ok((val, margin, ok));
    }
    if let Some(rhs) = tgt.strip_prefix('>') {
        let bound: f64 = rhs
            .trim()
            .parse()
            .map_err(|_| format!("bad @assert bound '{tgt}'"))?;
        let ok = val > bound;
        let margin = format!("{:+.4}", val - bound);
        return Ok((val, margin, ok));
    }

    let expected: f64 = tgt
        .parse()
        .map_err(|_| format!("bad @assert target '{tgt}'"))?;
    let abs_err = (val - expected).abs();
    if let Some(pct_str) = tol_raw.strip_suffix('%') {
        let tol_pct: f64 = pct_str
            .trim()
            .trim_start_matches("<=")
            .trim_start_matches('≤')
            .trim()
            .parse()
            .map_err(|_| format!("bad @assert percentage tolerance '{tol_raw}'"))?;
        let denom = expected.abs().max(1e-12);
        let rel_pct = 100.0 * abs_err / denom;
        let ok = rel_pct <= tol_pct;
        return Ok((val, format!("{rel_pct:.2}%"), ok));
    }
    let tol_abs: f64 = tol_raw
        .trim_start_matches("<=")
        .trim_start_matches('≤')
        .trim_start_matches('±')
        .trim()
        .parse()
        .unwrap_or(1e-3);
    let ok = abs_err <= tol_abs;
    Ok((val, format!("±{abs_err:.4}"), ok))
}

/// Evaluate a slice of [`LiterateAssert`] contracts against an already-computed [`Analysis`].
pub fn evaluate_contracts(
    asserts: &[LiterateAssert],
    a: &Analysis,
    g: Option<&Graded>,
) -> Result<VerificationReport, String> {
    let mut outcomes = Vec::with_capacity(asserts.len());
    let mut passed = 0usize;
    for asrt in asserts {
        let (value, error_or_margin, ok) = evaluate_assert(asrt, a, g)?;
        if ok {
            passed += 1;
        }
        outcomes.push(AssertOutcome {
            label: asrt.label.clone(),
            metric: asrt.metric.clone(),
            target: asrt.target.clone(),
            tolerance: asrt.tolerance.clone(),
            unit: asrt.unit.clone(),
            value,
            error_or_margin,
            passed: ok,
        });
    }
    let total = outcomes.len();
    let failed = total - passed;
    Ok(VerificationReport {
        total,
        passed,
        failed,
        all_passed: failed == 0,
        outcomes,
    })
}

/// Run the document's required solver pass(es) (solid, modal if `solve modes=...`, graded TPMS if
/// `fill ...`) and evaluate all `doc.asserts` (plus any header `@assert:` lines from `raw_fsm` if
/// `doc.asserts` is empty).
pub fn verify_document(
    doc: &Document,
    raw_fsm: Option<&str>,
    res: Resolution,
    clock: Clock,
) -> Result<(Analysis, Option<Graded>, VerificationReport), String> {
    let asserts: Vec<LiterateAssert> = if !doc.asserts.is_empty() {
        doc.asserts.clone()
    } else if let Some(src) = raw_fsm {
        fsm::parse_literate_header(src)
            .map(|h| h.asserts)
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let need_graded =
        doc.fill.is_some() && asserts.iter().any(|a| is_graded_assert_metric(&a.metric));

    let modes_count = required_modes_for_asserts(doc.solve.modes, &asserts);

    let a = analyse_full(doc, res, clock, modes_count, false).map_err(|e| e.0)?;
    let graded_opt = if need_graded {
        Some(fill::graded_of(doc, &a, clock, false).map_err(|e| e.0)?)
    } else {
        None
    };
    let analysis = a;

    let report = evaluate_contracts(&asserts, &analysis, graded_opt.as_ref())?;
    Ok((analysis, graded_opt, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_clock() -> f64 {
        0.0
    }

    #[test]
    fn verify_document_checks_exact_percentage_and_inequality_contracts() {
        let src = "\
fieldsmith 1
name \"Contract Test Cantilever\"
material al7075
box beam size=(40, 10, 10) at=(20, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -500)
assert \"Tip load resultant\" metric=\"resultant_mag\" target=\"500\" tol=\"0.5%\" unit=\"N\"
assert \"Deflection positive\" metric=\"max_displacement\" target=\">= 0.01\" tol=\">= 0.01\" unit=\"mm\"
assert \"Peak stress bounded\" metric=\"max_vm_elem\" target=\"<= 250\" tol=\"<= 250\" unit=\"MPa\"
";
        let doc = fsm::parse(src).expect("parse");
        let (_a, _g, rep) =
            verify_document(&doc, Some(src), Resolution::Dofs(8_000), zero_clock).expect("verify");
        assert_eq!(rep.total, 3);
        assert!(rep.all_passed, "expected all 3 contracts to pass: {rep:?}");

        let mut failing_doc = doc.clone();
        failing_doc.asserts.push(LiterateAssert {
            label: "Impossible stress cap".into(),
            metric: "max_vm_elem".into(),
            target: "<= 0.001".into(),
            tolerance: "<= 0.001".into(),
            unit: "MPa".into(),
        });
        let (_a2, _g2, rep2) =
            verify_document(&failing_doc, None, Resolution::Dofs(8_000), zero_clock)
                .expect("verify with failing contract");
        assert!(!rep2.all_passed);
        assert_eq!(rep2.failed, 1);
    }
}
