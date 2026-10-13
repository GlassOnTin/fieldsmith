//! V17 — rung records: one host clock per rung, records that are the engine's own
//! arithmetic, and the change estimate measured against a known reference.
//!
//! Three legs:
//! 1. Replay: a ladder driven straight through `analyse` (one `Instant` per rung, the
//!    CLI's contract) produces records that recompute bit-exactly from the engine's own
//!    outputs — wall time, DOF, elements, element size, floored envelope maxima, and the
//!    change estimate equal to the two-solve difference. Nothing extra is timed and
//!    nothing is invented.
//! 2. Cantilever: the ladder on the V1 beam; each pair's remaining error against the
//!    Euler–Bernoulli tip deflection sits next to the rung-to-rung change estimate.
//!    What the ship-time data shows is sharper than the assumed frame: at the coarse
//!    rung the change moved the solution AWAY from the theory (the small coarse offset
//!    was cancellation, not accuracy), and in the tail the remaining offset is bias the
//!    ladder cannot see at any refinement — both directions are why the estimate is
//!    labelled "estimate, not bound". Bands are recorded at ship and re-asserted.
//! 3. The web rung table labels its change an estimate — asserted by the scripted
//!    bracket e2e (`web/tests/bracket.mjs`, step "Results lens", `(estimate):` check),
//!    recorded in `docs/verification/bracket-e2e.md` by `web/tests/run-e2e.mjs`.

use fieldsmith_core::{
    analyse,
    probe::{probe, Locator},
    rung::{CostFit, RungRecord},
    Resolution,
};
use fieldsmith_verify::{doc, Check, Report};
use std::time::Instant;

const DOC: &str = "\
name \"Cantilever V1\"
custommaterial steel200 name=\"Steel (E 200 GPa)\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
";

/// The 30k / 150k schedule targets the web's coarse/refined constants run at.
const TARGETS: [usize; 2] = [30_000, 150_000];

/// The analytic leg starts coarse (1000-aimed, h≈8 mm — barely past one element through
/// the 10 mm section) so the ladder crosses two regimes: a pair where change and
/// remaining error are comparable, then the bias tail. The 30k→150k pair is the UI
/// schedule.
const CANTILEVER_TARGETS: [usize; 3] = [1_000, 30_000, 150_000];

/// Recorded at ship (referee, 2026-10-13). Two pair ratios, measured on the V1 beam —
/// |remaining error vs Euler–Bernoulli| / |change estimate| per pair (see
/// `v17_cantilever_ladder`):
/// - PAIR01: the beam's coarse→fine pair. Change +0.383 %, remaining offset +0.341 %
///   — comparable magnitudes. Note the coarse rung sat at only −0.042 %: THAT was
///   cancellation (−0.35 % discretisation under ~+0.31 % bias), not accuracy.
/// - TAIL: once converged, the change is fp-level movement while the remaining offset
///   is model-form bias no rung ladder can see. The ratio there is large and stays
///   where it is at ship.
const SHIP_PAIR01_LO: f64 = 0.6;
const SHIP_PAIR01_HI: f64 = 1.2;
const SHIP_TAIL_LO: f64 = 8.0;
const SHIP_TAIL_HI: f64 = 20.0;

fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// One rung exactly as the CLI drives it: one `Instant` around the analyse, wall snapshotted
/// before anything else reads it.
fn run_rung(
    d: &fieldsmith_doc::Document,
    target_dofs: usize,
    rung_i: usize,
    prev: Option<&RungRecord>,
) -> (RungRecord, fieldsmith_core::Analysis) {
    let t0 = Instant::now();
    let a = analyse(d, Resolution::Dofs(target_dofs), clock).expect("analysis runs");
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    (RungRecord::of(rung_i, target_dofs, wall_ms, prev, &a), a)
}

/// Hand-rolled floored envelope over cases — independent recomputation of
/// `TrackedValues::of_cases` for the replay leg.
fn envelope(cases: &[fieldsmith_core::CaseResult]) -> (f64, f64) {
    let mut d = 0.0f64;
    let mut v = 0.0f64;
    for c in cases {
        if c.max_displacement > d {
            d = c.max_displacement;
        }
        if c.max_vm_elem > v {
            v = c.max_vm_elem;
        }
    }
    (d.max(1e-9), v.max(1e-9))
}

#[test]
fn v17_rung_replay() {
    let d = doc(DOC);
    let (rec0, a0) = run_rung(&d, TARGETS[0], 0, None);
    let (rec1, a1) = run_rung(&d, TARGETS[1], 1, Some(&rec0));

    // Wall: record construction stores the driver's snapshot verbatim — feed a sentinel
    // and it must come back unchanged, proving the record cannot re-clock or clamp.
    let sentinel = RungRecord::of(1, TARGETS[1], 1234.5, Some(&rec0), &a1);
    assert_eq!(sentinel.wall_ms, 1234.5, "record stores the driver's wall verbatim");

    // Shape fields are the engine's own provenance, bit-equal.
    assert_eq!(rec0.dofs, a0.provenance.dofs);
    assert_eq!(rec0.elements, a0.provenance.elements);
    assert_eq!(rec0.element_size_mm, a0.provenance.element_size);
    assert_eq!(rec1.dofs, a1.provenance.dofs);
    assert_eq!(rec1.elements, a1.provenance.elements);
    assert_eq!(rec1.element_size_mm, a1.provenance.element_size);
    assert!(rec1.dofs > rec0.dofs && rec1.wall_ms > rec0.wall_ms, "refinement moves both");

    // Tracked maxima are the floored envelope over cases, recomputed by hand and bit-equal.
    let (d0, v0) = envelope(&a0.cases);
    let (d1, v1) = envelope(&a1.cases);
    assert_eq!(rec0.tracked.deflection_mm, d0, "rung 0 deflection envelope");
    assert_eq!(rec0.tracked.vm_mpa, v0, "rung 0 von Mises envelope");
    assert_eq!(rec1.tracked.deflection_mm, d1, "rung 1 deflection envelope");
    assert_eq!(rec1.tracked.vm_mpa, v1, "rung 1 von Mises envelope");

    // The change estimate is exactly the two-solve difference over the floored envelopes.
    let ch = rec1.change.as_ref().expect("rung 1 has a change");
    assert_eq!(ch.deflection_rel, (d1 - d0) / d0, "deflection change = two-solve difference");
    assert_eq!(ch.vm_rel, (v1 - v0) / v0, "von Mises change = two-solve difference");
    assert_eq!(ch.deflection_mm, d1 - d0);
    assert_eq!(ch.vm_mpa, v1 - v0);
    assert!(rec0.change.is_none(), "rung 0 has no baseline");
}

#[test]
fn v17_cantilever_ladder() {
    let d = doc(DOC);
    // Euler–Bernoulli tip deflection (same reference as V1): F = 100 N, L = 100 mm,
    // E = 200 GPa, I = b·h³/12 over the 10×10 section.
    let (f, l, e, b, hgt) = (100.0, 100.0, 200_000.0, 10.0, 10.0);
    let i = b * hgt * hgt * hgt / 12.0;
    let defl_ref = f * l * l * l / (3.0 * e * i);

    let rungs: Vec<(RungRecord, fieldsmith_core::Analysis)> = CANTILEVER_TARGETS
        .iter()
        .enumerate()
        .scan(None::<RungRecord>, |prev, (ri, &t)| {
            let (r, a) = run_rung(&d, t, ri, prev.as_ref());
            *prev = Some(r.clone());
            Some((r, a))
        })
        .collect();

    // Probe the tip through the same path V1 uses, so the table's tip values are what a
    // user would read off the model.
    let tips: Vec<f64> = rungs
        .iter()
        .map(|(_, a)| {
            let loc = Locator::new(a);
            probe(a, &loc, Some(0), [100.0 - 1e-6, 5.0, 5.0]).expect("tip inside mesh").displacement[2]
        })
        .collect();

    // Remaining analytic error per rung (tracked envelope vs the EB reference) and the
    // honesty ratios: |remaining error| / |change estimate| per pair.
    let errs: Vec<f64> = rungs
        .iter()
        .map(|(r, _)| (r.tracked.deflection_mm - defl_ref) / defl_ref)
        .collect();
    let mut ratios = Vec::new();
    for ri in 1..rungs.len() {
        let ch = rungs[ri].0.change.as_ref().expect("rung > 0 carries a change");
        ratios.push(errs[ri].abs() / ch.deflection_rel.abs());
    }

    // Pair 0→1: change and remaining offset are comparable (and the first move went
    // AWAY from the theory — the coarse offset was cancellation). Pair 1→2: the ladder
    // has converged onto model-form bias the change cannot see, so the ratio flips large.
    // Print the whole ladder first so a band failure shows its numbers.
    println!("V17 cantilever ladder (ref {:.4} mm):", defl_ref);
    for (ri, (r, _)) in rungs.iter().enumerate() {
        let chl = match r.change.as_ref() {
            Some(c) => format!("{:+.3} %", c.deflection_rel * 100.0),
            None => "—".to_string(),
        };
        println!(
            "  rung {ri}: aim {}, dofs {}, h {:.3}, tracked {:.6}, tip {:.4}, vs EB {:+.3} %, change {chl}",
            r.target_dofs,
            r.dofs,
            r.element_size_mm,
            r.tracked.deflection_mm,
            -tips[ri],
            errs[ri] * 100.0,
        );
    }
    println!(
        "  ratios (|remaining offset| / |change|): pair01 {:.3}, tail {:.1}",
        ratios[0], ratios[1],
    );
    let (ch1, ch2) = (
        rungs[1].0.change.as_ref().expect("rung 1 change"),
        rungs[2].0.change.as_ref().expect("rung 2 change"),
    );
    assert!(
        ratios[0] >= SHIP_PAIR01_LO && ratios[0] <= SHIP_PAIR01_HI,
        "pair-01 ratio {:.4} outside the ship band [{}, {}] — \
         re-measure and update SHIP_PAIR01_*",
        ratios[0],
        SHIP_PAIR01_LO,
        SHIP_PAIR01_HI,
    );
    assert!(
        ratios[1] >= SHIP_TAIL_LO && ratios[1] <= SHIP_TAIL_HI,
        "tail-regime ratio {:.4} outside the ship band [{}, {}] — \
         re-measure and update SHIP_TAIL_*",
        ratios[1],
        SHIP_TAIL_LO,
        SHIP_TAIL_HI,
    );
    // The ladder moved the beam away from the theory on the first pair: the coarse rung
    // sat under the reference, the refined one over it. Fp arithmetic both machines agree
    // on (bracket deltas re-measured to ~1e-11 relative), so the sign is stable.
    assert!(errs[0] < 0.0 && errs[1] > 0.0, "sign flip coarse→fine: {:.5} → {:.5}", errs[0], errs[1]);
    assert!(ch1.deflection_rel > 0.0, "softening direction: {:+.5}", ch1.deflection_rel);
    // The tail movement is fp-level and the remaining offset holds at bias: the ladder
    // stalls; it will never close the last third of a percent.
    assert!(ch2.deflection_rel.abs() < 1e-3, "tail movement is small: {:+.5}", ch2.deflection_rel);
    assert!(
        (errs[2] - errs[1]).abs() <= 0.15 * errs[1].abs(),
        "tail offset stable at bias level: {:.5} → {:.5}",
        errs[1],
        errs[2],
    );

    let mut r = Report::new("V17", "Rung records — one clock per rung, estimates not bounds");
    r.check(Check::relative(
        "Tip deflection (refined rung)",
        defl_ref,
        -tips[2],
        "mm",
        0.02,
    ));
    r.section(
        "Replay (bit-exact)",
        "`v17_rung_replay` drives the two-rung ladder straight through `analyse` — one \
         `Instant` per rung, wall snapshotted before anything else reads it — and asserts \
         the records recompute **bit-exactly** from the engine's own outputs: DOF, elements, \
         element size from `Provenance`; tracked maxima from a hand-rolled floored envelope \
         over cases; the change estimate equal to the two-solve difference. Nothing extra \
         is clocked and nothing is invented: record construction gets a sentinel wall and \
         must store it verbatim, so the record provably cannot re-clock or clamp.",
    );
    r.section(
        "Cantilever ladder vs the known reference",
        format!(
            "The V1 beam through a three-rung ladder (aimed DOF {ta0} → {ta1} → {ta2}; the \
             30k→150k pair is the web schedule). The change estimate measures how much the \
             solution MOVED between rungs — it is not a bound on how far the solution is \
             from the truth, and the two regimes below say so from opposite directions.\n\n\
             | Rung | Aim | DOF | h (mm) | Envelope defl. (mm) | Tip probe (mm) | vs Euler–Bernoulli | Change estimate | Remaining error / change |\n\
             | --: | --: | --: | --: | --: | --: | --: | --: | --: |\n\
             | 0 | {ta0} | {dd0} | {h0:.3} | {dd0v:.4} | {tp0:.4} | {e0p} | — | — |\n\
             | 1 | {ta1} | {dd1} | {h1:.3} | {dd1v:.4} | {tp1:.4} | {e1p} | {c1p} | 1 : {r0:.3} (comparable) |\n\
             | 2 | {ta2} | {dd2} | {h2:.3} | {dd2v:.4} | {tp2:.4} | {e2p} | {c2p} | {r1:.1} : 1 (tail) |\n\n\
             Reference: δ = F·L³/(3·E·I) = {ref_mm:.4} mm; the envelope deflection is the \
             tracked value the rung table uses (max displacement magnitude over the case); \
             the tip probe is the Z deflection at the load face, what a user reads off.\n\n\
             **Pair 0→1 — the move can go AWAY from the theory.** At h≈{h0:.1} mm the \
             envelope sat at {e0p} of beam theory; one refinement later (change {c1}) it \
             sits at {e1p}. The coarse rung's closeness was cancellation — roughly \
             −0.35 % of discretisation under ~+0.31 % of model-form offset — not \
             accuracy. A reader who took \"within 0.1 % at the coarse rung\" as convergence \
             would be wrong, and a reader taking the change as the error's fate would get \
             the wrong sign. At ship (referee, 2026-10-13) remaining offset / change = \
             {r0:.3} (band [{p01_lo}, {p01_hi}]; fp arithmetic, stable across machines).\n\n\
             **Pair 1→2 — converged onto bias.** The next rung moves the envelope {c2} — \
             nearly nothing — while the offset to theory holds at {e2}. That offset is \
             model form (the 3D clamp restrains Poisson contraction at the root; \
             Euler–Bernoulli neglects shear — V1's discussion), which NO rung ladder can \
             see or fix at any refinement. At ship remaining offset / change = {r1:.1} \
             (band [{tl_lo}, {tl_hi}]).\n\n\
             So the change estimates how much the solution MOVED, and both failure modes \
             of reading it as a bound show up in this one beam: the small coarse offset \
             was luck, and the tiny tail change understates the remaining offset by \
             {r1:.0}×. \"Estimates, not bounds\" is the only honest label.",
            ta0 = CANTILEVER_TARGETS[0],
            ta1 = CANTILEVER_TARGETS[1],
            ta2 = CANTILEVER_TARGETS[2],
            dd0 = rungs[0].0.dofs,
            dd1 = rungs[1].0.dofs,
            dd2 = rungs[2].0.dofs,
            h0 = rungs[0].0.element_size_mm,
            h1 = rungs[1].0.element_size_mm,
            h2 = rungs[2].0.element_size_mm,
            dd0v = rungs[0].0.tracked.deflection_mm,
            dd1v = rungs[1].0.tracked.deflection_mm,
            dd2v = rungs[2].0.tracked.deflection_mm,
            tp0 = -tips[0],
            tp1 = -tips[1],
            tp2 = -tips[2],
            e0p = fmt_pct(errs[0]),
            e1p = fmt_pct(errs[1]),
            e2p = fmt_pct(errs[2]),
            c1p = fmt_pct(ch1.deflection_rel),
            c2p = fmt_pct(ch2.deflection_rel),
            c1 = fmt_pct(ch1.deflection_rel),
            c2 = fmt_pct(ch2.deflection_rel),
            e2 = fmt_pct(errs[2]),
            r0 = ratios[0],
            r1 = ratios[1],
            ref_mm = defl_ref,
            p01_lo = SHIP_PAIR01_LO,
            p01_hi = SHIP_PAIR01_HI,
            tl_lo = SHIP_TAIL_LO,
            tl_hi = SHIP_TAIL_HI,
        ),
    );
    r.section(
        "Web rung table labels estimates",
        "The results panel's rung table labels its change `(estimate):` — asserted by the \
         scripted bracket e2e (`web/tests/bracket.mjs`, step \"Results lens\", the \
         \"rung table lists both levels and labels the change an estimate\" check), \
         recorded in `docs/verification/bracket-e2e.md`. The change math is the same as the \
         badge's — both read from one extracted helper (`changePct`, `results.tsx`) and the \
         CLI's `RungChange::of` uses the same formula over the same floored envelopes.",
    );
    r.section(
        "Cost fit sanity on this ladder",
        format!(
            "Two rungs give the exact-by-construction power law with residual unmeasured. \
             Measured walls across the ladder: {} — fitted exponent {:.2} over the first \
             pair (the three-rung referee measurement on the bracket — 250.9/1164.9/9792.7 ms \
             at 30k/150k/750k — carries the residual row; see V18).",
            rungs
                .iter()
                .map(|(rr, _)| format!("{:.0} ms", rr.wall_ms))
                .collect::<Vec<_>>()
                .join(" → "),
            CostFit::of(&[
                (rungs[0].0.dofs as f64, rungs[0].0.wall_ms),
                (rungs[1].0.dofs as f64, rungs[1].0.wall_ms),
            ])
            .expect("two valid points")
            .exponent,
        ),
    );
    r.write();
}

/// Signed relative fraction as a readable percent string.
fn fmt_pct(v: f64) -> String {
    format!("{:+.3} %", v * 100.0)
}