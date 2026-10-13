//! Refinement rungs as first-class records (GOAL v0.4 section A): what a rung is, the
//! rung-to-rung change estimate on the tracked quantities, and a power-law fit of wall
//! time against DOF count with its own residual. Every field here is arithmetic over the
//! engine's own outputs or a host-measured wall time — the solver reports nothing of its
//! own beyond its existing `Provenance`, and timing is the host's single measurement per
//! rung.

use serde::Serialize;

use crate::{Analysis, CaseResult};

/// The tracked quantities, enveloped over load cases. Same convention as the UI badge
/// (`results.tsx`): max over cases of element-centroid von Mises and of nodal displacement
/// magnitude, with the same 1e-9 floor so relative change stays defined when an unloaded
/// document solves to zero.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TrackedValues {
    /// Max over cases of nodal displacement magnitude (mm).
    pub deflection_mm: f64,
    /// Max over cases of element-centroid von Mises (MPa) — the V3.md stress readout.
    pub vm_mpa: f64,
}

impl TrackedValues {
    /// Envelope over cases, floored at 1e-9 exactly as the badge computes it.
    pub fn of_cases(cases: &[CaseResult]) -> Self {
        TrackedValues {
            deflection_mm: 1e-9f64
                .max(cases.iter().map(|c| c.max_displacement).fold(0.0, f64::max)),
            vm_mpa: 1e-9f64.max(cases.iter().map(|c| c.max_vm_elem).fold(0.0, f64::max)),
        }
    }
}

/// The rung-to-rung change estimate on the tracked quantities — an estimate from change,
/// not a bound on the true error (README says so; V17 measures the ratio against a known
/// reference). Same arithmetic as the badge (`results.tsx`): relative change is
/// `(cur − prev) / prev` over the floored envelope, signed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RungChange {
    /// Deflection change as a fraction of the previous rung's value (0.012 = 1.2 %).
    pub deflection_rel: f64,
    /// Von Mises change as a fraction of the previous rung's value.
    pub vm_rel: f64,
    /// Deflection change in absolute mm.
    pub deflection_mm: f64,
    /// Von Mises change in absolute MPa.
    pub vm_mpa: f64,
}

impl RungChange {
    /// The badge's math: relative and absolute change between two floored envelopes.
    pub fn of(prev: TrackedValues, cur: TrackedValues) -> Self {
        RungChange {
            deflection_rel: (cur.deflection_mm - prev.deflection_mm) / prev.deflection_mm,
            vm_rel: (cur.vm_mpa - prev.vm_mpa) / prev.vm_mpa,
            deflection_mm: cur.deflection_mm - prev.deflection_mm,
            vm_mpa: cur.vm_mpa - prev.vm_mpa,
        }
    }
}

/// A declared tolerance on one tracked quantity: `deflection:0.1mm`, `deflection:1%`,
/// `vm:2.5mpa` — a fraction (%) is relative to the current rung's value, as the badge's
/// change math defines it. Parsing lives here so the CLI, and later the API (M15), share
/// one definition of what "to within a stated tolerance" means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tolerance {
    /// Absolute deflection box (mm).
    DeflectionMm(f64),
    /// Deflection change as a fraction of the current value.
    DeflectionRel(f64),
    /// Absolute von Mises box (MPa).
    VonMisesMpa(f64),
    /// Von Mises change as a fraction of the current value.
    VonMisesRel(f64),
}

impl Tolerance {
    /// `quantity:unit` — quantity `deflection|defl|vm|vonmises`, value a plain decimal,
    /// unit `mm|mpa|%` (`%` on a number is part of the unit token).
    pub fn parse(s: &str) -> Result<Self, String> {
        let bad = || {
            format!(
                "bad tolerance {s:?}: expect quantity:value with quantity deflection|vm \
                 and unit mm|mpa|% — e.g. deflection:0.1mm, deflection:1%, vm:2.5mpa"
            )
        };
        let (qty, rest) = s.split_once(':').ok_or_else(bad)?;
        let qty = qty.trim().to_ascii_lowercase();
        let rest = rest.trim();
        let split = rest
            .find(|c: char| c.is_ascii_alphabetic() || c == '%')
            .unwrap_or(rest.len());
        let (num, unit) = rest.split_at(split);
        let value: f64 = num.trim().parse().map_err(|_| bad())?;
        if !(value.is_finite() && value > 0.0) {
            return Err(bad());
        }
        let deflection = matches!(qty.as_str(), "deflection" | "defl");
        let vm = matches!(qty.as_str(), "vm" | "vonmises");
        match (unit.trim().to_ascii_lowercase().as_str(), deflection, vm) {
            ("mm", true, _) => Ok(Tolerance::DeflectionMm(value)),
            ("%", true, _) => Ok(Tolerance::DeflectionRel(value / 100.0)),
            ("mpa", _, true) => Ok(Tolerance::VonMisesMpa(value)),
            ("%", false, true) => Ok(Tolerance::VonMisesRel(value / 100.0)),
            _ => Err(bad()),
        }
    }

    /// Did the rung-to-rung change land inside this box?
    pub fn met(&self, change: &RungChange) -> bool {
        match self {
            Tolerance::DeflectionMm(v) => change.deflection_mm.abs() <= *v,
            Tolerance::DeflectionRel(v) => change.deflection_rel.abs() <= *v,
            Tolerance::VonMisesMpa(v) => change.vm_mpa.abs() <= *v,
            Tolerance::VonMisesRel(v) => change.vm_rel.abs() <= *v,
        }
    }
}

/// One refinement rung, measured. `wall_ms` is the host's single measurement for the whole
/// rung (mesh + every solve inside it); the engine's own `Provenance` breakdown stays as
/// it is, so records add no second clock.
#[derive(Clone, Debug, Serialize)]
pub struct RungRecord {
    /// Index in the caller's schedule (0 = coarsest).
    pub rung: usize,
    /// The DOF count the rung aimed for (the schedule entry, not what meshing achieved).
    pub target_dofs: usize,
    pub dofs: usize,
    pub elements: usize,
    pub element_size_mm: f64,
    /// Host-measured wall time (ms).
    pub wall_ms: f64,
    /// Change vs the previous rung; `None` for the first rung (no baseline).
    pub change: Option<RungChange>,
    pub tracked: TrackedValues,
}

impl RungRecord {
    /// Build the record for one finished rung from the engine's own analysis plus the
    /// host-measured wall time. `prev` carries the previous rung's recorded values.
    pub fn of(
        rung: usize,
        target_dofs: usize,
        wall_ms: f64,
        prev: Option<&RungRecord>,
        analysis: &Analysis,
    ) -> Self {
        let p = &analysis.provenance;
        let tracked = TrackedValues::of_cases(&analysis.cases);
        let change = prev.map(|prev| RungChange::of(prev.tracked, tracked));
        RungRecord {
            rung,
            target_dofs,
            dofs: p.dofs,
            elements: p.elements,
            element_size_mm: p.element_size,
            wall_ms,
            change,
            tracked,
        }
    }
}

/// Log–log power-law fit of wall time against DOF count over the rungs so far — the cost
/// estimate GOAL section A calls for: project rung k+1 from the fit over rungs ≤ k, and
/// show the fit residual next to the projection. House rule: the estimate carries its own
/// error, so the residual travels with every projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CostFit {
    /// Slope in log–log space (wall ∝ DOF^`exponent`).
    pub exponent: f64,
    /// Wall time (ms) at 1 DOF — the fit's scale.
    pub ms_at_unit: f64,
    /// Max relative deviation |fit − actual| / actual over the fitted points. Meaningful
    /// from three points on; with exactly two the fit is exact by construction, the field
    /// is `None`, and an honest report says "residual unmeasured" rather than 0 %.
    pub residual: Option<f64>,
}

impl CostFit {
    /// Fit over `(dofs, wall_ms)` points. Returns `None` unless at least two points with
    /// positive, distinct DOF counts and positive wall times remain after filtering.
    pub fn of(points: &[(f64, f64)]) -> Option<Self> {
        let pts: Vec<(f64, f64)> = points
            .iter()
            .filter(|&&(d, w)| d > 0.0 && w > 0.0)
            .map(|&(d, w)| (d.ln(), w.ln()))
            .collect();
        if pts.len() < 2 {
            return None;
        }
        let n = pts.len() as f64;
        let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
        let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
        let num: f64 = pts.iter().map(|&(x, y)| (x - mx) * (y - my)).sum();
        let den: f64 = pts.iter().map(|&(x, _)| (x - mx) * (x - mx)).sum();
        if !(den > 1e-12) {
            return None; // identical DOF counts — no slope knowable
        }
        let exponent = num / den;
        if !exponent.is_finite() {
            return None;
        }
        let c = my - exponent * mx; // ln(dofs)=0 → ln(wall) = c
        let mut worst = 0.0f64;
        for &(x, y) in &pts {
            let fitted = (c + exponent * x).exp();
            worst = worst.max((fitted - y.exp()).abs() / y.exp());
        }
        Some(CostFit {
            exponent,
            ms_at_unit: c.exp(),
            residual: if pts.len() == 2 { None } else { Some(worst) },
        })
    }

    /// Project the wall time (ms) at `dofs`, extrapolating the power law.
    pub fn project(&self, dofs: f64) -> f64 {
        self.ms_at_unit * dofs.powf(self.exponent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analyse, Resolution};
    use fieldsmith_doc::fsm;
    use std::time::Instant;

    fn parse(src: &str) -> fieldsmith_doc::Document {
        fsm::parse(&format!("fieldsmith 1\n{src}")).expect("valid test document")
    }
    fn clock() -> f64 {
        thread_local!(static T0: Instant = Instant::now());
        T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
    }

    const DOC: &str = "\
custommaterial steel200 name=\"Steel (E 200 GPa)\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
";

    #[test]
    fn tracked_envelope_takes_the_case_maximum_over_floored_values() {
        let d = parse(DOC);
        let a = analyse(&d, Resolution::Size(5.0), clock).expect("analysis");
        let t = TrackedValues::of_cases(&a.cases);
        assert_eq!(a.cases.len(), 1);
        // The case fields the badge reads are exactly what the envelope maxes over.
        assert_eq!(t.deflection_mm, a.cases[0].max_displacement.max(1e-9));
        assert_eq!(t.vm_mpa, a.cases[0].max_vm_elem.max(1e-9));
        // A non-trivial solve, not a degenerate zero: the beam deflects and carries stress.
        assert!(t.deflection_mm > 0.1);
        assert!(t.vm_mpa > 1.0);
    }

    #[test]
    fn change_is_the_badge_math_on_floored_values() {
        let prev = TrackedValues { deflection_mm: 2.0, vm_mpa: 100.0 };
        let cur = TrackedValues { deflection_mm: 2.02, vm_mpa: 98.0 };
        let ch = RungChange::of(prev, cur);
        assert!((ch.deflection_rel - 0.01).abs() < 1e-9);
        assert!((ch.vm_rel + 0.02).abs() < 1e-9);
        assert!((ch.deflection_mm - 0.02).abs() < 1e-9);
        assert!((ch.vm_mpa + 2.0).abs() < 1e-9);
        // Zero floor: the badge's 1e-9 keeps % defined where a raw 0 would divide by zero.
        let zero = RungChange::of(TrackedValues { deflection_mm: 1e-9, vm_mpa: 1e-9 }, cur);
        assert!(zero.deflection_rel > 2_000_000.0);
    }

    #[test]
    fn record_of_wires_provenance_and_change_together() {
        let d = parse(DOC);
        let a0 = analyse(&d, Resolution::Size(5.0), clock).expect("analysis");
        let t0 = clock();
        let r0 = RungRecord::of(0, 50_000, 12.0, None, &a0);
        let a1 = analyse(&d, Resolution::Size(2.5), clock).expect("analysis");
        let wall1 = clock() - t0;
        let r1 = RungRecord::of(1, 150_000, wall1, Some(&r0), &a1);
        // Recorded fields are the engine's own numbers, not recomputed lookalikes.
        assert_eq!(r1.dofs, a1.provenance.dofs);
        assert_eq!(r1.elements, a1.provenance.elements);
        assert_eq!(r1.element_size_mm, a1.provenance.element_size);
        assert_eq!(r1.wall_ms, wall1);
        assert!(r0.change.is_none());
        let ch = r1.change.expect("second rung has a baseline");
        let expect = RungChange::of(r0.tracked, r1.tracked);
        assert_eq!(ch, expect);
        // Rung 1 actually refined: more DOF than rung 0. (Deflection change direction is
        // not a hard property at this mesh coarseness — V1 measures it; see V17.)
    }

    #[test]
    fn tolerance_parse_and_met() {
        assert_eq!(Tolerance::parse("deflection:0.1mm"), Ok(Tolerance::DeflectionMm(0.1)));
        assert_eq!(
            Tolerance::parse("deflection:1%"),
            Ok(Tolerance::DeflectionRel(0.01))
        );
        assert_eq!(Tolerance::parse("vm:2.5mpa"), Ok(Tolerance::VonMisesMpa(2.5)));
        assert_eq!(Tolerance::parse("VM:2 %"), Ok(Tolerance::VonMisesRel(0.02)));
        assert!(Tolerance::parse("deflection:1").is_err()); // unit required
        assert!(Tolerance::parse("vm:0.5mm").is_err()); // mm is deflection-only
        assert!(Tolerance::parse("bogus:1%").is_err());
        assert!(Tolerance::parse("deflection:-1mm").is_err());
        let ch = RungChange::of(
            TrackedValues { deflection_mm: 2.0, vm_mpa: 100.0 },
            TrackedValues { deflection_mm: 2.04, vm_mpa: 101.5 },
        );
        assert!(Tolerance::DeflectionRel(0.05).met(&ch)); // |2 %| inside 5 %
        assert!(!Tolerance::DeflectionRel(0.01).met(&ch));
        assert!(Tolerance::DeflectionMm(0.05).met(&ch));
        assert!(!Tolerance::DeflectionMm(0.03).met(&ch));
        assert!(Tolerance::VonMisesMpa(2.0).met(&ch));
        assert!(!Tolerance::VonMisesMpa(1.0).met(&ch));
    }

    #[test]
    fn cost_fit_recovers_a_known_power_law() {
        // wall = 20 ms · dofs^1.35
        let exact: Vec<(f64, f64)> = [30_000.0, 150_000.0, 750_000.0]
            .iter()
            .map(|&d| (d, 20.0 * (d as f64).powf(1.35)))
            .collect();
        let f = CostFit::of(&exact).expect("three clean points fit");
        assert!((f.exponent - 1.35).abs() < 1e-9);
        assert!((f.ms_at_unit - 20.0).abs() < 1e-6);
        assert!((f.residual.expect("≥3 points measure the residual") - 0.0).abs() < 1e-6);
        assert!((f.project(750_000.0) - 20.0 * 750_000f64.powf(1.35)).abs() < 1e-3);

        // An off-law point sets the residual, and the projection still follows the fit.
        let noisy = vec![
            (30_000.0, 20.0 * 30_000f64.powf(1.35)),
            (150_000.0, 20.0 * 150_000f64.powf(1.35)),
            (750_000.0, 40.0 * 750_000f64.powf(1.35)),
        ];
        let f2 = CostFit::of(&noisy).expect("fit over noisy points");
        let r = f2.residual.expect("three points");
        // By hand: even x-spacing (×5 per rung), the doubled last point pulls the least-
        // squares slope to ≈ 1.565; the worst deviation is the middle point,
        // e^(ln 2 − 2·(1.565−1.35)·ln 5 …) → ≈ e^0.231 − 1 ≈ 0.26.
        assert!(r > 0.24 && r < 0.28, "worst relative deviation = {r}");
        // Two points fit exactly and honestly refuse a residual claim.
        let two = &exact[..2];
        let f3 = CostFit::of(two).expect("two distinct points fit");
        assert!(f3.residual.is_none());
        assert!((f3.exponent - 1.35).abs() < 1e-9);
        // Degenerate inputs.
        assert!(CostFit::of(&[(30_000.0, 100.0), (30_000.0, 120.0)]).is_none());
        assert!(CostFit::of(&[(30_000.0, 100.0)]).is_none());
        assert!(CostFit::of(&[(30_000.0, 0.0), (150_000.0, 100.0)]).is_none());
        assert!(CostFit::of(&[]).is_none());
    }
}