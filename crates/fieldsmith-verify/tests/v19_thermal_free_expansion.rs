//! V19 — thermal free expansion legs: the whole M14 coupling chain probed where it can
//! be held to roundoff.
//!
//! Both legs drive the full pipeline the user sees (`analyse`)
//! with temperature regions, no loads, `solve tref=20`, and land in one report:
//!
//! 1. Free expansion: the bar's whole surface holds one temperature, so Laplace with
//!    constant boundary data returns T ≡ 60 °C and ΔT = 40 K is uniform. The three
//!    single-component supports on the faces through the origin (x=0 face fixed=x,
//!    y=0 face fixed=y, z=0 face fixed=z) kill all six rigid modes — each rotation
//!    violates at least one face constraint on a surface of positive measure — without
//!    constraining the exact solution u = α·ΔT·(x, y, z), which satisfies every
//!    constraint exactly. That solution lies in the P2 space and its recovered stress
//!    vanishes identically, so every free row of the discrete system is satisfied
//!    exactly: the discrete stress must sit at roundoff and the displacement must be
//!    linear in the coordinates to the PCG tolerance.
//! 2. End-fixed expansion: the same bar with u_x = 0 on BOTH end faces plus the y and z
//!    single-component supports (all satisfied exactly by the exact solution, all six
//!    rigid modes still killed). The exact solution u = (0, g·y, g·z) with
//!    g = (1+ν)·α·ΔT carries the constant uniaxial stress σxx = −E·α·ΔT — its
//!    divergence-free reaction lives entirely on the Dirichlet ends, so the interior
//!    rows are still satisfied exactly. Probed mid-length (the plan's Saint-Venant
//!    wording — here the exact discrete field is the constant-stress field itself, so
//!    the probe position is nominal); the plan declared 5 % on E·α·ΔT.

use fieldsmith_core::{
    analyse,
    probe::{probe, Locator},
    Resolution,
};
use fieldsmith_verify::{doc, Check, Report};
use std::time::Instant;

const E: f64 = 70_000.0; // MPa
const NU: f64 = 0.33;
const ALPHA: f64 = 2.3e-5; // 1/K
const T_HOT: f64 = 60.0; // °C on the whole surface
const T_REF: f64 = 20.0; // stress-free reference
const DT: f64 = T_HOT - T_REF; // 40 K
/// Displacement scale of the free leg: α·ΔT·L along the length.
const DISP_SCALE: f64 = ALPHA * DT * 40.0;
const STRESS_SCALE: f64 = E * ALPHA * DT; // E·α·ΔT = 64.4 MPa

/// Geometry: 40×10×10 bar from the origin (all of nx/ny/nz are planes through it).
const BAR: &str = "box bar size=(40, 10, 10) at=(20, 5, 5)";

/// Measured on ship (referee, 2026-10-13). The exact-solution argument says the
/// discrete residuals are zero to the PCG tolerance, so the measured errors sit at
/// floating-point level, not discretisation level; the bands are set orders above the
/// measured values so a solver refactor cannot push a real regression through quietly.
const FREE_DISP_HI: f64 = 1e-4; // mm, vs a displacement scale of 0.037 mm
const FREE_T_HI: f64 = 1e-2; // °C, a uniform Dirichlet field reproduced exactly
const FREE_STRESS_HI: f64 = 0.1; // MPa, vs E·α·ΔT = 64.4 MPa

/// End-fixed leg: the plan's declared 5 %; σyy/σzz must vanish at roundoff per the
/// same argument (recovered stress is exactly σxx êx⊗êx at the probe).
const FIXED_SIGXX_TOL: f64 = 0.05;
const FIXED_TRANSVERSE_HI: f64 = 0.1; // MPa
const FIXED_UX_HI: f64 = 1e-4; // mm, vs the free-expansion displacement scale

fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

fn fixture(temperature_line: &str, supports: &str) -> String {
    format!(
        "name \"V19 thermal\"\n\
         custommaterial heat e={E} nu={NU} a={ALPHA} yield=250 density=2.7\n\
         material heat\n\
         {BAR}\n\
         region skin = bar.all\n\
         {supports}\n\
         {temperature_line}\n\
         solve tref={T_REF}\n"
    )
}

fn run(doc_text: &str, name: &str) -> (fieldsmith_core::Analysis, usize) {
    let d = doc(doc_text);
    let a = analyse(&d, Resolution::Size(2.0), clock).expect("analysis runs");
    let case = a
        .cases
        .iter()
        .position(|c| c.name == name)
        .unwrap_or_else(|| {
            panic!(
                "case {name} ran (cases: {:?})",
                a.cases.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
            )
        });
    (a, case)
}

/// Max |component| over nodal stresses of a case.
fn max_stress_component(a: &fieldsmith_core::Analysis, case: usize) -> f64 {
    a.cases[case]
        .stress
        .iter()
        .flat_map(|s| s.iter())
        .fold(0.0f64, |m, v| m.max(v.abs()))
}

/// Max |u(p) − u_exact(p)| over the mesh nodes against a closed-form field.
fn displacement_error(
    a: &fieldsmith_core::Analysis,
    case: usize,
    u_exact: impl Fn([f64; 3]) -> [f64; 3],
) -> f64 {
    let u = &a.cases[case].u;
    a.mesh
        .nodes
        .iter()
        .enumerate()
        .map(|(n, p)| {
            (0..3)
                .map(|i| (u[3 * n + i] - u_exact(*p)[i]).abs())
                .fold(0.0f64, f64::max)
        })
        .fold(0.0f64, f64::max)
}

#[test]
fn v19_thermal_expansion_legs() {
    let mut r = Report::new("V19", "Thermal free expansion — the discrete coupling legs");

    // --- Leg (a): free expansion ------------------------------------------------------
    let (a, case) = run(
        &fixture(
            "temperature warmed skin temp=60 case=\"Free\"",
            "\
             region gx = bar.nx\n\
             region gy = bar.ny\n\
             region gz = bar.nz\n\
             support sx gx fixed=x\n\
             support sy gy fixed=y\n\
             support sz gz fixed=z\n",
        ),
        "Free",
    );
    let t = a.cases[case].temperature.as_ref().expect("thermal field");
    let t_dev = t.iter().fold(0.0f64, |m, ti| m.max((ti - T_HOT).abs()));
    let disp_err = displacement_error(&a, case, |p| {
        [ALPHA * DT * p[0], ALPHA * DT * p[1], ALPHA * DT * p[2]]
    });
    let stress_max = max_stress_component(&a, case);

    println!(
        "V19(a): T deviation {t_dev:.3e} °C, displacement error {disp_err:.3e} mm \
         (scale {DISP_SCALE:.3e}), max stress component {stress_max:.3e} MPa \
         (E·α·ΔT = {STRESS_SCALE:.1}), max element von Mises {:.3e}, \
         conduction {} iters (rel {:.2e})",
        a.cases[case].max_vm_elem, a.cases[case].solve.iterations, a.cases[case].solve.rel_residual
    );
    assert!(
        t_dev <= FREE_T_HI && disp_err <= FREE_DISP_HI && stress_max <= FREE_STRESS_HI,
        "V19(a) free expansion outside the ship bands (T ≤ {FREE_T_HI} °C, \
         disp ≤ {FREE_DISP_HI} mm, stress ≤ {FREE_STRESS_HI} MPa): T {t_dev:.3e}, \
         disp {disp_err:.3e}, stress {stress_max:.3e} — re-measure and update FREE_* \
         with the regression explained"
    );
    r.section(
        "Leg (a) — free expansion",
        format!(
            "Box bar 40×10×10 from the origin, whole surface at {T_HOT} °C (`solve tref={T_REF}`, \
             ΔT = {DT} K), no loads. Supports: three single-component supports — the x=0 face \
             fixed=x, the y=0 face fixed=y, the z=0 face fixed=z — which kill all six rigid \
             modes without touching the exact solution. Element size 2.0 mm. \
             Conduction: {} iterations, rel residual {:.1e} — T ≡ 60 to solver tolerance \
             (max nodal deviation {t_dev:.1e} °C).",
            a.cases[case].solve.iterations, a.cases[case].solve.rel_residual
        ),
    );
    r.check(Check::absolute(
        "free: temperature uniformity, max |T − 60| over nodes",
        0.0,
        t_dev,
        "°C",
        FREE_T_HI,
    ));
    r.check(Check::absolute(
        "free: displacement vs u = α·ΔT·(x, y, z), max nodal error",
        0.0,
        disp_err,
        "mm",
        FREE_DISP_HI,
    ));
    r.check(Check::absolute(
        "free: stress-free, max |stress component| over nodes",
        0.0,
        stress_max,
        "MPa",
        FREE_STRESS_HI,
    ));

    // --- Leg (b): end-fixed expansion --------------------------------------------------
    let (a, case) = run(
        &fixture(
            "temperature warmed skin temp=60 case=\"EndFixed\"",
            "\
             region lx = bar.nx\n\
             region rx = bar.px\n\
             region gy = bar.ny\n\
             region gz = bar.nz\n\
             support lxh lx fixed=x\n\
             support rxh rx fixed=x\n\
             support sy gy fixed=y\n\
             support sz gz fixed=z\n",
        ),
        "EndFixed",
    );
    // u = (0, g·y, g·z), g = (1+ν)·α·ΔT.
    let g = (1.0 + NU) * ALPHA * DT;
    let ux_max = a.cases[case]
        .u
        .iter()
        .step_by(3)
        .fold(0.0f64, |m, v| m.max(v.abs()));
    let loc = Locator::new(&a);
    let mid = probe(&a, &loc, Some(case), [20.0, 5.0, 5.0]).expect("probe mid-length");

    println!(
        "V19(b): σxx {:+.3} MPa (want {:+.1}), σyy {:+.3e}, σzz {:+.3e}, \
         |u_x| max {ux_max:.3e} mm, u_y(mid) {:+.4e} vs g·y {:+.4e}",
        mid.stress[0],
        -STRESS_SCALE,
        mid.stress[1],
        mid.stress[2],
        mid.displacement[1],
        g * 5.0
    );
    assert!(
        mid.stress[1].abs() <= FIXED_TRANSVERSE_HI
            && mid.stress[2].abs() <= FIXED_TRANSVERSE_HI
            && ux_max <= FIXED_UX_HI,
        "V19(b) transverse bands (σyy/σzz ≤ {FIXED_TRANSVERSE_HI} MPa, |u_x| ≤ \
         {FIXED_UX_HI} mm): σyy {:+.3e}, σzz {:+.3e}, u_x {ux_max:.3e} — re-measure and \
         update FIXED_* with the regression explained",
        mid.stress[1],
        mid.stress[2]
    );
    r.section(
        "Leg (b) — end-fixed expansion",
        format!(
            "Same bar and ΔT = {DT} K; u_x = 0 on BOTH end faces plus the y and z \
             single-component supports on the through-origin faces — all exactly satisfied \
             by the exact solution u = (0, (1+ν)·α·ΔT·y, (1+ν)·α·ΔT·z), all six rigid modes \
             still killed. Probed mid-length at (20, 5, 5): the constant uniaxial recovered \
             stress σxx = −E·α·ΔT is divergence-free with zero lateral traction, so its \
             residual reaches only the reaction ends (whose x-rows are Dirichlet-eliminated) \
             and the interior rows stay exact — the probe position is nominal here, the exact \
             discrete field is the constant-stress field itself. The plan declared 5 % on \
             E·α·ΔT."
        ),
    );
    r.section(
        "Measured",
        format!(
            "σxx probe {:+.4} MPa vs −E·α·ΔT = {:+.1} MPa; transverse components \
             {:+.2e} / {:+.2e} MPa (vanish in the exact field); max nodal |u_x| \
             {:.1e} mm; max nodal |u_y| error vs g·y: {:.1e} mm.",
            mid.stress[0],
            -STRESS_SCALE,
            mid.stress[1],
            mid.stress[2],
            ux_max,
            displacement_error(&a, case, |p| [0.0, g * p[1], g * p[2]])
        ),
    );
    r.check(Check::relative(
        "end-fixed: σxx at mid-length (thermal stress)",
        -STRESS_SCALE,
        mid.stress[0],
        "MPa",
        FIXED_SIGXX_TOL,
    ));
    r.check(Check::absolute(
        "end-fixed: transverse σyy at mid-length",
        0.0,
        mid.stress[1],
        "MPa",
        FIXED_TRANSVERSE_HI,
    ));
    r.check(Check::absolute(
        "end-fixed: transverse σzz at mid-length",
        0.0,
        mid.stress[2],
        "MPa",
        FIXED_TRANSVERSE_HI,
    ));
    r.check(Check::absolute(
        "end-fixed: u_x nodal max",
        0.0,
        ux_max,
        "mm",
        FIXED_UX_HI,
    ));

    r.finish();
}
