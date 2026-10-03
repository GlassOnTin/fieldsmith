//! V6 — the fill engine's homogenised stiffness law against resolved unit-cell coupons.
//!
//! GOAL V6: gyroid sheet, unit cells ρ = 0.2 / 0.3 / 0.4, resolved FE vs the homogenised law
//! within 10 %.
//!
//! The law (`fieldsmith_core::fill::law_stiffness`, gyroid sheet c = 616.9, n = 1.426) was
//! least-squares fitted on log-log (ρ_mesh, E_int) pairs measured by the coupon harness
//! (`examples/tpms-coupon.rs` → `fieldsmith_verify::coupon`) at cell 2, N = 3 cells, mesh size
//! h = t (one mesh cell per wall), 1000 MPa coupon material, free-slip platens, interior-band
//! strain measurement. So the two tiers of this case mean different things and say so:
//!
//! * **N = 3 rows are the law's own fit inputs** — resolving the same coupons again checks the
//!   measurement pipeline reproduces its inputs (a regression gate after the harness moved from
//!   the example into `src/coupon.rs`; bit-identical mesh, same numbers). They have no power to
//!   confirm the law beyond the fit.
//! * **N = 4 rows are the independent check** — a different coupon size (≈ 4× the elements,
//!   same h) must reproduce the law to within 10 %. The relative ρ_mesh drift N = 3 → 4 is
//!   ~1 %, so the check really is size independence, not a repeat of the same mesh.
//!
//! Run (heavy: ≈ 2 min at full optimisation):
//! `cargo test -p fieldsmith-verify --release --test v6_tpms_law -- --ignored --nocapture`

use fieldsmith_core::fill::law_stiffness;
use fieldsmith_doc::{Family, FillMode};
use fieldsmith_verify::coupon::{self, CouponSpec};
use fieldsmith_verify::{Check, Report};

const YOUNG: f64 = 1000.0;
const TOL: f64 = 0.10;

/// Gyroid sheet coupon on the law's own settings, at a given size (cells per side).
fn gyroid_sheet(rho: f64, n: usize) -> CouponSpec {
    let mut s = CouponSpec::new(Family::Gyroid, FillMode::Sheet, rho);
    s.n = n;
    s
}

#[test]
#[ignore = "heavy (~2 min at full optimisation): cargo test -p fieldsmith-verify --release --test v6_tpms_law -- --ignored --nocapture"]
fn v6_gyroid_sheet_law() {
    let mut r = Report::new(
        "V6",
        "TPMS homogenised stiffness law vs resolved coupon (gyroid, sheet mode)",
    );

    // The fit inputs, as measured during the fitting session (regression gate below).
    let fit_inputs: [(f64, usize, f64, f64); 3] = [
        // (rho nominal, N, rho_mesh measured, E_int measured)
        (0.2, 3, 0.19734, 61.52),
        (0.3, 3, 0.29585, 106.18),
        (0.4, 3, 0.39733, 167.52),
    ];

    let mut n3_body = String::new();
    for &(rho, n, rho_fit, e_fit) in &fit_inputs {
        let row = coupon::run(&gyroid_sheet(rho, n));
        assert!(row.converged, "{rho} N={n}: solve must converge, log:\n{}", row.log.join("\n"));
        // Round-trip gate on the extraction and the identity machinery of the harness.
        r.check(Check::absolute(
            &format!("equilibrium ∫σxxdV vs ΣFx·x identity, ρ = {rho} N = {n}"),
            0.0,
            row.ident_energy_err,
            "%",
            0.5,
        ));
        r.check(Check::absolute(
            &format!("equilibrium ⟨σxx⟩ vs −Σ/ρL² identity, ρ = {rho} N = {n}"),
            0.0,
            row.ident_hill_err,
            "%",
            0.5,
        ));
        // Main law check. Reference: the law at the MEASURED density (fit convention), scaled
        // to the coupon's material modulus.
        let law = law_stiffness(Family::Gyroid, FillMode::Sheet, row.rho_mesh) * YOUNG;
        let e = row.e_int.expect("interior band holds elements");
        r.check(Check::relative(
            &format!("E* coupon ρ = {rho} N = {n} (regression gate: law's own fit input; reference row ρ_mesh = {rho_fit:.5}, E = {e_fit:.2})"),
            law,
            e,
            "MPa",
            TOL,
        ));
        n3_body.push_str(&format!(
            "ρ nominal {rho}: ρ_mesh {:.5} (fit input {rho_fit:.5}), law {:.2} MPa, measured {:.2} MPa ({:+.1} %), dofs {}, iters {}{}\n",
            row.rho_mesh,
            law,
            e,
            100.0 * e / law - 100.0,
            row.dofs,
            row.iters,
            if row.n_components > 1 {
                format!(", {} fragments dropped", row.dropped_tets)
            } else {
                String::new()
            }
        ));
        // Sanity: the re-measurement must be bit-identical to the fit inputs (same seedless
        // deterministic pipeline) — drift within printing precision is the gate itself; the
        // law check above carries the 10 % envelope.
        assert!(
            (row.rho_mesh - rho_fit).abs() < 2e-5 && (e - e_fit).abs() < 0.05,
            "regression: N = {n} ρ = {rho} moved off its fit input (ρ_mesh {:.5} vs {rho_fit:.5}, E_int {e} vs {e_fit})",
            row.rho_mesh
        );
    }

    let mut n4_body = String::new();
    for &rho in &[0.2, 0.3, 0.4] {
        let row = coupon::run(&gyroid_sheet(rho, 4));
        assert!(row.converged, "{rho} N=4: solve must converge, log:\n{}", row.log.join("\n"));
        r.check(Check::absolute(
            &format!("equilibrium ∫σxxdV vs ΣFx·x identity, ρ = {rho} N = 4"),
            0.0,
            row.ident_energy_err,
            "%",
            0.5,
        ));
        r.check(Check::absolute(
            &format!("equilibrium ⟨σxx⟩ vs −Σ/ρL² identity, ρ = {rho} N = 4"),
            0.0,
            row.ident_hill_err,
            "%",
            0.5,
        ));
        let law = law_stiffness(Family::Gyroid, FillMode::Sheet, row.rho_mesh) * YOUNG;
        let e = row.e_int.expect("interior band holds elements");
        r.check(Check::relative(
            &format!("E* coupon ρ = {rho} N = 4 (independent coupon size)"),
            law,
            e,
            "MPa",
            TOL,
        ));
        n4_body.push_str(&format!(
            "ρ nominal {rho}: ρ_mesh {:.5}, law {:.2} MPa, measured {:.2} MPa ({:+.1} %), dofs {}, iters {}\n",
            row.rho_mesh,
            law,
            e,
            100.0 * e / law - 100.0,
            row.dofs,
            row.iters,
        ));
    }

    r.section(
        "Coupon and law conventions",
        "Coupon: N³ unit cells of side c = 2 mm, gyroid sheet lattice, meshed by isosurface \
stuffing at h = t (one mesh cell per wall; t from the density calibration at the nominal ρ), \
1000 MPa coupon material, ν = 0.3. Free-slip platens: traction −σ·nₓ on the planar material \
patches of the x = L face (σ = 10 MPa), ux = 0 on the x = 0 patches, two point constraints \
remove the residual rigid modes. Apparent modulus E* = Σ / ε with Σ the macroscopic section \
stress and ε the volume-weighted least-squares slope of ux over the interior band \
(1 cell from each platen). The law: E*/E = c·ρ^n with c = 616.9 MPa, n = 1.426, least-squares \
fitted in log-log space on ρ_mesh/E_int pairs; V6 compares the law at the COUPON'S MEASURED \
density (ρ_mesh) with the measured E*. 1000 MPa scaling matches the coupon material. The \
equilibrium identities (∫σxx dV = Σ Fₓ·x and ⟨σxx⟩ = −Σ/ρL² over the material) gate each run \
at ≤ 0.5 %: a load-patch assembly regression shows there first.",
    );
    r.section(
        "N = 3 — the law's own fit inputs (regression gate)",
        &n3_body,
    );
    r.section(
        "N = 4 — independent coupon size (the check with confirmation power)",
        &n4_body,
    );
    r.section(
        "Coverage and limits",
        "This case covers gyroid sheet mode only (GOAL V6). The other five law rows \
(primitive and diamond in sheet mode, and all three families in network mode) share the same \
engine mechanism and harness, and their sweeps are in \
`crates/fieldsmith-verify/examples/tpms-coupon.rs` history, but they are not re-checked here \
as automated cases: their coupon setups needed per-family shift/band choices, which V6's \
single fixed configuration cannot carry. Law support is the measured band ρ ∈ [0.18, 0.40]; \
outside it the engine clamps (documented in `fill.rs`).",
    );

    r.finish();
}