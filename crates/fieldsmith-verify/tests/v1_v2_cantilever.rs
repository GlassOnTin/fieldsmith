//! V1 & V2 — cantilever beam against Euler–Bernoulli theory.
//!
//! A 100 × 10 × 10 mm steel beam (E = 200 GPa, ν = 0.3) is clamped on its x = 0 face and carries
//! a 100 N downward force spread over its x = 100 face. The document goes through exactly the
//! pipeline the app uses (`fieldsmith_core::analyse`: field → isosurface-stuffed tets → P2 solve →
//! nodal stress recovery) and results are read with the same probe the app uses.

use fieldsmith_core::{
    analyse,
    probe::{probe, Locator},
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

fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

#[test]
fn v1_v2_cantilever() {
    let d = doc(DOC);
    // Euler–Bernoulli references.
    let (f, l, e, b, hgt) = (100.0, 100.0, 200_000.0, 10.0, 10.0);
    let i = b * hgt * hgt * hgt / 12.0;
    let defl_ref = f * l * l * l / (3.0 * e * i);
    let m_mid = f * (l - 50.0);
    let sigma_ref = m_mid * (hgt / 2.0) / i;

    let mut rows = String::from(
        "| Element size | Tets | DOFs | Iterations | Mesh | Solve | Tip deflection | σxx at mid-span top |\n\
         | --: | --: | --: | --: | --: | --: | --: | --: |\n",
    );
    let mut last = (0.0, 0.0);
    for h in [5.0, 2.5, 2.0] {
        let a = analyse(&d, Resolution::Size(h), clock).expect("analysis");
        let loc = Locator::new(&a);
        let tip = probe(&a, &loc, Some(0), [100.0 - 1e-6, 5.0, 5.0]).expect("tip inside mesh");
        let top =
            probe(&a, &loc, Some(0), [50.0, 5.0, 10.0 - 1e-6]).expect("top fibre inside mesh");
        let p = &a.provenance;
        rows += &format!(
            "| {h} mm | {} | {} | {} | {:.0} ms | {:.0} ms | {:.5} mm | {:.3} MPa |\n",
            p.elements,
            p.dofs,
            p.iterations,
            p.mesh_ms,
            p.solve_ms,
            -tip.displacement[2],
            top.stress[0]
        );
        last = (-tip.displacement[2], top.stress[0]);
    }

    let mut r1 = Report::new("V1", "Cantilever tip deflection");
    r1.check(Check::relative(
        "Tip deflection (finest mesh)",
        defl_ref,
        last.0,
        "mm",
        0.02,
    ));
    r1.section(
        "Reference",
        format!(
            "Euler–Bernoulli: δ = F·L³ / (3·E·I), with I = b·h³/12 = {i:.3} mm⁴, giving {defl_ref:.4} mm. \
             Beam theory neglects shear deformation (Timoshenko adds ≈ 0.8 % here) and the 3D clamp \
             also restrains Poisson contraction at the root, so a few tenths of a percent difference \
             in either direction is physical, not numerical."
        ),
    );
    r1.section("Mesh study", rows.clone());
    r1.section(
        "Method",
        "Document → signed-distance field → isosurface-stuffed tetrahedra (BCC lattice) → quadratic \
         (P2) elements → conjugate gradients with a two-level P2/P1 preconditioner. The tip value is \
         the probed z-displacement at the centre of the loaded face.",
    );

    let mut r2 = Report::new("V2", "Cantilever bending stress");
    r2.check(Check::relative(
        "σxx at x = 50, top fibre (finest mesh)",
        sigma_ref,
        last.1,
        "MPa",
        0.03,
    ));
    r2.section(
        "Reference",
        format!(
            "σ = M·c / I with M = F·(L − x) = {m_mid} N·mm at x = 50 mm, c = 5 mm, I = {i:.3} mm⁴, \
             giving {sigma_ref:.2} MPa (tension on the top fibre for a downward tip load)."
        ),
    );
    r2.section("Mesh study", rows);
    r2.section(
        "Method",
        "Same analysis as V1. Stress is recovered per element at the nodes and volume-averaged \
         (the field the app colours), then interpolated with the P2 basis at the probe point.",
    );
    r1.write();
    r2.write();
    r1.finish();
    r2.finish();
}
