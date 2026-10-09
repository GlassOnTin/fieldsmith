//! V15 — distributed (face-local fn) loads against Euler–Bernoulli and analytic resultants.
//!
//! Two distributed load cases on the V1 cantilever's top face, one document, two load cases:
//! a triangle profile q(x) growing from zero at the clamp to q₀ at the tip, and a two-band
//! piecewise profile stepping up mid-span. The density is written in the face frame's
//! coordinates (local x measured from the face centroid runs from the clamp end at −L/2 to
//! the tip at +L/2); the traction direction is the frame's −n. The document goes through
//! `fieldsmith_core::analyse` exactly as the app does.

use fieldsmith_core::{analyse, probe::{probe, Locator}, Resolution};
use fieldsmith_verify::{doc, Check, Report};
use std::time::Instant;

const DOC: &str = "\
name \"Distributed V15\"
custommaterial steel200 name=\"Steel (E 200 GPa)\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region top = beam.pz
support clamp root fixed=xyz
load tri top fn=\"-50 < x < 50 -> 0.4*(x+50)/100\" local=(0, 0, -1) case=\"Triangle\"
load pw top fn=\"-50 < x < 25 -> 0.2; 25 < x < 50 -> 0.4\" local=(0, 0, -1) case=\"Piecewise\"
";

fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// Composite Simpson over [a, b] with `n` (even, ≥ 2) panels.
fn simpson(f: &dyn Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
    let n = n.max(2);
    let n = if n % 2 == 0 { n } else { n + 1 };
    let hh = (b - a) / n as f64;
    let mut s = f(a) + f(b);
    for i in 1..n {
        s += f(a + hh * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * hh / 3.0
}

#[test]
fn v15_distributed() {
    let d = doc(DOC);
    // Beam theory constants: b = width (10), h = depth (10), L = 100, E = 200 GPa.
    let (l, e, b, hgt) = (100.0, 200_000.0, 10.0, 10.0);
    let i = b * hgt * hgt * hgt / 12.0;
    let ei = e * i;

    // --- case "Triangle": q(x) = q0·x/L rising from the clamp, applied as N/mm²
    // Densities: q0 = 0.4 N/mm² at the tip ⇒ line load w0 = q0·b = 4 N/mm.
    let q0 = 0.4;
    let w0 = q0 * b;
    // EB: δ_tip = 11·w0·L⁴ / (120·E·I) for a triangular load peaking at the free end.
    let delta_tri = 11.0 * w0 * l * l * l * l / (120.0 * ei);
    let r_tri: [f64; 3] = [0.0, 0.0, -(q0 * b * l / 2.0)];
    // Moment about the origin (clamp face centre-plane... the document origin is at x = 0):
    // M_x = −R·ȳ (ȳ = 5), M_y = R·x̄ with x̄ = 2L/3 for the ramp.
    let m_tri: [f64; 3] = [r_tri[2] * 5.0, -r_tri[2] * (2.0 * l / 3.0), 0.0];

    // --- case "Piecewise": w is 2.0 N/mm over the first 75 mm, 4.0 N/mm over the last 25.
    let wfn = |x: f64| if x <= 75.0 { 2.0 } else { 4.0 };
    let r_pw: [f64; 3] = [
        0.0,
        0.0,
        -(2.0 * 75.0 + 4.0 * 25.0),
    ];
    // Closed form (u = distance from the tip, M(u) = 2u² for u ≤ 25, 2u² − (u−25)² above;
    // δ·EI = ∫ M(u)·u du over u ∈ [0, L]).
    let m_of_u = |u: f64| 2.0 * u * u - (u - 25.0).max(0.0) * (u - 25.0).max(0.0);
    let delta_pw_ei: f64 = (0..=200000)
        .map(|k| {
            let u = l * k as f64 / 200000.0;
            let w = if k == 0 || k == 200000 { 0.5 } else { 1.0 };
            w * m_of_u(u) * u
        })
        .sum::<f64>()
        * (l / 200000.0);
    let delta_pw = delta_pw_ei / ei;
    // Same quantity by composite Simpson through the load integral — the algebra's guard.
    let moment_at = |x: f64| simpson(&(move |s| wfn(s) * (s - x)), x, l, 2048);
    let delta_pw_num = simpson(&move |x| moment_at(x) * (l - x), 0.0, l, 2048) / ei;
    assert!(
        ((delta_pw - delta_pw_num) / delta_pw).abs() < 2e-4,
        "closed form δ {delta_pw} vs Simpson {delta_pw_num}"
    );
    // M_y = ∫ x·w(x) dx: band 1 carries 150 N at x̄ = 37.5, band 2 carries 100 N at x̄ = 87.5,
    // positive (M_y = +x·F_z for a pure −z force). M_x = y·F_z = −R·5.
    let m_pw: [f64; 3] = [
        r_pw[2] * 5.0,
        150.0 * 37.5 + 100.0 * 87.5,
        0.0,
    ];

    let mut rows = String::from(
        "| Element size | Tets | DOFs | Iterations | Mesh | Solve | δ tip (Triangle) | δ tip (Piecewise) | R_z (Triangle) | unscaled dR | fit RMS | extent |\n\
         | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: |\n",
    );
    let mut last: Option<(f64, f64, [f64; 3], [f64; 3], f64, f64, f64, f64)> = None;
    for h in [5.0, 2.5, 2.0] {
        let a = analyse(&d, Resolution::Size(h), clock).expect("analysis");
        let ci_tri = a.cases.iter().position(|c| c.name == "Triangle").expect("tri case");
        let ci_pw = a.cases.iter().position(|c| c.name == "Piecewise").expect("pw case");
        let loc = Locator::new(&a);
        let tip_tri =
            probe(&a, &loc, Some(ci_tri), [l - 1e-6, 5.0, 5.0]).expect("tip inside mesh");
        let tip_pw =
            probe(&a, &loc, Some(ci_pw), [l - 1e-6, 5.0, 5.0]).expect("tip inside mesh");
        let dt = -tip_tri.displacement[2];
        let dp = -tip_pw.displacement[2];
        let rt = &a.cases[ci_tri].load_reports[0];
        let rp = &a.cases[ci_pw].load_reports[0];
        let p = &a.provenance;
        let rms = rt.fit_rms.expect("fit rms on a flat face");
        let ext = rt.fit_extent.expect("fit extent on a flat face");
        rows += &format!(
            "| {h} mm | {} | {} | {} | {:.0} ms | {:.0} ms | {:.5} mm | {:.5} mm | {:.3} N | {:.3} N | {:.2e} mm | {:.1} mm |\n",
            p.elements, p.dofs, p.iterations, p.mesh_ms, p.solve_ms, dt, dp,
            rt.resultant[2], rt.unscaled, rms, ext,
        );
        last = Some((dt, dp, rt.resultant, rt.moment, rt.unscaled, rp.resultant[2], rp.moment[1], rp.moment[0]));
    }
    let Some((dt, dp, r_t, m_t, u_t, r_pw_z, m_pw_y, m_pw_x)) = last else {
        unreachable!("the mesh loop ran at least once")
    };

    let mut r = Report::new("V15", "Distributed loads: face-local fn profiles");
    r.section(
        "Reference",
        format!(
            "Triangle profile q(x) = q0·x/L over the top face (q0 = {q0} N/mm² at the tip, zero at \
             the clamp), pushing into the face (the frame's −n): the equivalent line load is \
             w = q·b with w0 = {w0} N/mm, and Euler–Bernoulli gives δ_tip = 11·w0·L⁴/(120·E·I) = \
             {delta_tri:.5} mm. Resultant ∫q dA = q0·b·L/2 = {rtri:.1} N ({rz_tri:.1} N signed \
             along z); the ramp's pressure centroid sits at x̄ = 2L/3, so about the document \
             origin M_y = R·x̄ = {my_tri:.2} N·mm and M_x = −R·ȳ = {mx_tri:.1} N·mm (ȳ = 5; the \
             traction is pure −z, so the face's height z = 10 does not enter these moments). \
             Piecewise: w = 2.0 N/mm over [0, 75] and 4.0 N/mm over (75, 100]; measured from the \
             tip the moment is M(u) = 2u² − max(u−25, 0)², so δ = (∫ M(u)·u du)/(E·I) with \
             ∫ M(u)·u du = {pw_ei:.0} N·mm², giving {delta_pw:.5} mm; the same integral by \
             composite Simpson reads {delta_pw_num:.5} mm (agreement asserted at 2e-4 before use). \
             Resultant {rz_pw:.1} N; pressure centroid x̄ = {xbar_pw:.2} mm, so M_y = {my_pw:.2} \
             N·mm and M_x = {mx_pw:.1} N·mm.",
            rtri = -r_tri[2],
            rz_tri = r_tri[2],
            my_tri = m_tri[1],
            mx_tri = m_tri[0],
            pw_ei = delta_pw_ei,
            rz_pw = r_pw[2],
            xbar_pw = m_pw[1] / -r_pw[2],
            my_pw = m_pw[1],
            mx_pw = m_pw[0],
        ),
    );
    r.section(
        "Frame",
        format!(
            "The loads' region is the top face (a flat plane): the least-squares frame's residual is \
             mesh rounding (reported per rung below; the refusal bound is 10 % of extent), the local \
             x axis is world +x (the deterministic rule), and the fn's −50..50 coordinates are mm \
             from the face's area-weighted centroid at x = 50. `local=(0, 0, -1)` is the frame's −n."
        ),
    );
    r.section("Mesh study", rows);

    r.check(Check::relative(
        "δ tip, Triangle (finest rung)",
        delta_tri,
        dt,
        "mm",
        0.15,
    ));
    r.check(Check::relative(
        "δ tip, Piecewise (finest rung)",
        delta_pw,
        dp,
        "mm",
        0.15,
    ));
    r.check(Check::relative("R_z, Triangle (finest rung)", r_tri[2], r_t[2], "N", 0.02));
    r.check(Check::relative("M_x, Triangle", m_tri[0], m_t[0], "N·mm", 0.02));
    r.check(Check::relative("M_y, Triangle", m_tri[1], m_t[1], "N·mm", 0.02));
    r.check(Check::relative("R_z, Piecewise", r_pw[2], r_pw_z, "N", 0.02));
    r.check(Check::relative("M_x, Piecewise", m_pw[0], m_pw_x, "N·mm", 0.02));
    r.check(Check::relative("M_y, Piecewise", m_pw[1], m_pw_y, "N·mm", 0.02));
    r.check(Check::relative(
        "unscaled density resultant, Triangle",
        -r_tri[2],
        u_t,
        "N",
        0.02,
    ));
    r.finish();
}