//! V13 — modal analysis (M8): the first modes of a thin clamped bar.
//!
//! Reference: Euler–Bernoulli cantilever frequencies computed from the document's own
//! parameters, `f_n = (βnL)² √(E I / (ρ A L⁴)) / 2π`. The square section makes ranks 1–4 two
//! degenerate bending pairs (one per transverse direction), so the comparisons run on pair
//! means and a split bound covers the degeneracy. Pair 2 sits ~4.5 % under the Euler–Bernoulli
//! value — the beam's shear flexibility and rotary inertia, physical at L/h = 10 and not a
//! discretization gap: the rung-to-rung change is ~0.1 %.
//!
//! Ranks 5–6 are not the β3 bending pair: rank 5 is the first torsion mode (St-Venant
//! reference √(G/ρ)/(4L), and the measured value drifts away from it with refinement — the
//! St-Venant constant over-credits a short bar's warping stiffness), rank 6 is unresolved.
//! V13 therefore checks four modes.

use fieldsmith_core::{analyse_modes, Resolution};
use fieldsmith_verify::{doc, Check, Report};
use std::time::Instant;

const DOC: &str = "\
name \"Modal V13\"
custommaterial steel200 name=\"Steel (E 200 GPa)\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
support clamp root fixed=xyz
";

fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

#[test]
fn v13_modal() {
    let d = doc(DOC);

    // Beam theory constants from the document: L = 100 mm, section 10 × 10 mm,
    // E = 200 GPa, density 7.85 g/cm³ = 7.85e-9 N·s²/mm⁴ (the modal run's units).
    let (l, e, b, hgt, rho): (f64, f64, f64, f64, f64) = (100.0, 200_000.0, 10.0, 10.0, 7.85e-9);
    let inertia = b * hgt * hgt * hgt / 12.0;
    let area = b * hgt;
    let scale = (e * inertia / (rho * area * l * l * l * l)).sqrt();
    let b1 = 1.875_104_069_f64;
    let b2 = 4.694_091_530_f64;
    let tau = std::f64::consts::TAU;
    let f1_ref = b1 * b1 * scale / tau;
    let f2_ref = b2 * b2 * scale / tau;

    let coarse = analyse_modes(&d, Resolution::Size(5.0), clock, 4).expect("coarse rung");
    let fine = analyse_modes(&d, Resolution::Size(2.5), clock, 4).expect("fine rung");

    let pair = |f: &[f64], k: usize| (f[2 * k] + f[2 * k + 1]) / 2.0;
    let split = |f: &[f64], k: usize| (f[2 * k] - f[2 * k + 1]).abs();

    for run in [&coarse, &fine] {
        assert_eq!(run.modal.frequencies.len(), 4, "four modes requested");
        for w in &run.modal.frequencies {
            assert!(w.is_finite() && *w > 0.0, "positive frequencies, got {w}");
        }
        // Ascending across the boundaries between different modes. Within a degenerate pair
        // the order is decided by roundoff (the two members are the same frequency to
        // within the split bound the report checks).
        assert!(
            run.modal.frequencies[1] <= run.modal.frequencies[2] + 1e-6,
            "pair 1 below pair 2: {:?}",
            run.modal.frequencies
        );
    }

    let (c1, c2) = (
        pair(&coarse.modal.frequencies, 0),
        pair(&coarse.modal.frequencies, 1),
    );
    let (f1, f2) = (
        pair(&fine.modal.frequencies, 0),
        pair(&fine.modal.frequencies, 1),
    );

    let mut r = Report::new(
        "V13",
        "Modal (M8) — clamped bar, first two bending pairs vs Euler–Bernoulli",
    );
    r.section(
        "Case",
        "A 100 × 10 × 10 mm steel box clamped at one end, no loads. The modal run meshes the \
         document as the static path does (the supports only) and solves the first four natural \
         frequencies on P2 tetrahedra with a consistent mass matrix. The measured values are \
         rung means of each degenerate transverse pair; the pair at ranks 1–2 is the first \
         bending mode in the two transverse directions, ranks 3–4 the second.",
    );
    r.section(
        "Reference",
        "Euler–Bernoulli cantilever, computed from the document's parameters: \
         f_n = (βnL)² √(EI/(ρAL⁴)) / 2π with β₁L = 1.8751, β₂L = 4.6941, I = bh³/12 = 833.3 mm⁴. \
         f1 = 815.4 Hz, f2 = 5109.9 Hz. The pair-2 gap is expected: Euler–Bernoulli neglects \
         shear deformation and rotary inertia; at L/h = 10 the Timoshenko effects lower the \
         second mode by about 4–5 %, and the FE answer converges to the Timoshenko value, not \
         the Euler–Bernoulli one.",
    );
    r.section(
        "Solver",
        "The eigen route is inverse power iteration with deflation (fenics-rs), its inner \
         solve the static path's two-level PCG, built once and reused across the run. The \
         Jacobi-only inner CG the route previously used flatlined at the fine rung (45,123 \
         DOF: relative residual 1.1 of 1e-9 after the 1000-iteration budget, measured before \
         the change); the static path's preconditioner converges the same system to 1e-9 in \
         about 48 iterations at this rung (V15's measurement). A second fix stopped the \
         power iteration a percent or two early: its stopping criterion compared the \
         Rayleigh quotient against a fixed anchor, and the mass rescale inside the route \
         shrinks the quotient to a fraction of the frequency, so the fixed anchor was an \
         absolute threshold in disguise; the criterion now compares consecutive quotients \
         to each other.",
    );
    r.check(Check::relative(
        "pair 1 mean, coarse rung",
        f1_ref,
        c1,
        "Hz",
        0.02,
    ));
    r.check(Check::relative(
        "pair 2 mean, coarse rung",
        f2_ref,
        c2,
        "Hz",
        0.07,
    ));
    r.check(Check::relative(
        "pair 1 mean, fine rung",
        f1_ref,
        f1,
        "Hz",
        0.02,
    ));
    r.check(Check::relative(
        "pair 2 mean, fine rung",
        f2_ref,
        f2,
        "Hz",
        0.07,
    ));
    r.check(Check::absolute(
        "pair 1 split, coarse rung",
        0.0,
        split(&coarse.modal.frequencies, 0),
        "Hz",
        5.0,
    ));
    r.check(Check::absolute(
        "pair 2 split, coarse rung",
        0.0,
        split(&coarse.modal.frequencies, 1),
        "Hz",
        5.0,
    ));
    r.check(Check::absolute(
        "pair 1 split, fine rung",
        0.0,
        split(&fine.modal.frequencies, 0),
        "Hz",
        5.0,
    ));
    r.check(Check::absolute(
        "pair 2 split, fine rung",
        0.0,
        split(&fine.modal.frequencies, 1),
        "Hz",
        5.0,
    ));
    r.check(Check::relative(
        "pair 1 rung change (5 → 2.5 mm)",
        c1,
        f1,
        "Hz",
        0.02,
    ));
    r.check(Check::relative(
        "pair 2 rung change (5 → 2.5 mm)",
        c2,
        f2,
        "Hz",
        0.02,
    ));
    r.finish();
}
