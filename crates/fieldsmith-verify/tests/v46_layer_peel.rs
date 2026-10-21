//! V46: Orthotropic layer-peel criterion and build orientation (`yield_z`, `shear_z`, `build=(θ, φ)`, M24).
//!
//! Verifies that `fieldsmith_solve::{layer_tractions, layer_peel_quadratic, layer_peel_utilization}`
//! and `fieldsmith_core::analyse` evaluate the Hoffman / Puck tension-only quadratic
//! inter-layer peel criterion on the build-axis layer plane `b̂(θ, φ)` and identify the
//! optimal build orientation:
//!
//! 1. **Closed-form traction and quadratic peel verification**:
//!    - Under uniaxial tension `σ = diag(0, 0, σ₀)` (`σ₀ = 30 MPa`) with layer normal
//!      `b̂(θ, 0) = [sin θ, 0, cos θ]`, the exact normal and shear tractions on the layer
//!      plane are `σ_nn(θ) = σ₀ cos²θ` and `τ_nt(θ) = σ₀ sin θ cos θ`.
//!    - With `S_z = 30 MPa` and `S_s = 20 MPa`, the quadratic peel index
//!      `I_peel(θ) = (max(0, σ_nn) / S_z)² + (τ_nt / S_s)²` and utilization `U_peel = √I_peel`
//!      match the closed form across `θ ∈ {0°, 30°, 45°, 60°, 90°}` to `< 1e-12`.
//!    - Under uniaxial compression (`σ₀ = -30 MPa`), `max(0, σ_nn) = 0` suppresses normal
//!      delamination at `θ = 0°` (`U_peel = 0`) while retaining inter-layer shear at
//!      `θ = 45°` (`U_peel = |τ_nt| / S_s = 15 / 20 = 0.75`).
//!
//! 2. **3D P2 finite-element uniaxial bar across build angles `build=(θ, 0)`**:
//!    - A `10 × 10 × 40 mm` bar with roller supports on `x = -5`, `y = -5`, `z = -20`
//!      (free Poisson contraction, exact uniform stress state `σ_zz = 30 MPa`) and
//!      `pressure = -30 MPa` on `z = +20`, solved with custom orthotropic strengths
//!      `yield_z = 30 MPa`, `shear_z = 20 MPa`.
//!    - Across `build=(θ, 0)` for `θ ∈ {0°, 30°, 45°, 60°, 90°}`, the 3D P2 finite-element
//!      `max_layer_peel` and element-centroid `peel_elem` match the closed-form
//!      `√((σ₀ cos²θ / S_z)² + (σ₀ sin θ cos θ / S_s)²)` within `1e-4`.
//!    - The hemisphere build-orientation sweep (`best_build_angles`, `min_peak_peel`)
//!      identifies `θ = 90°` (build axis perpendicular to the pull axis, so layer planes
//!      contain the pull axis) with `min_peak_peel < 1e-5`, and `worst_build_angles`
//!      identifies `θ = 0°` (`max_peak_peel = 1.0000×`).

use fieldsmith_core::overhang::build_axis_from_angles;
use fieldsmith_core::{analyse, Resolution};
use fieldsmith_doc::fsm;
use fieldsmith_solve::{layer_peel_quadratic, layer_peel_utilization, layer_tractions, Stress};
use fieldsmith_verify::{Check, Report};

fn now() -> f64 {
    0.0
}

fn bar_doc_with_build(theta_deg: f64, phi_deg: f64) -> String {
    format!(
        "\
fieldsmith 1
name \"V46 uniaxial bar build=({theta_deg}, {phi_deg})\"
custommaterial fdm_petg name=\"FDM PETG\" e=2100 nu=0.36 yield=50 density=1270 wall=0.8 yield_z=30 shear_z=20 source=\"V46 test\"
material fdm_petg
solve build=({theta_deg}, {phi_deg})

box bar size=(10, 10, 40)

region x_neg = bar.nx
region y_neg = bar.ny
region z_bot = bar.nz
region z_top = bar.pz

support rx x_neg fixed=x
support ry y_neg fixed=y
support rz z_bot fixed=z
load pull z_top pressure=-30 case=\"tension\"
"
    )
}

#[test]
fn v46_layer_peel() {
    let mut report = Report::new(
        "V46",
        "orthotropic layer-peel criterion (`yield_z`, `shear_z`) and build orientation `b̂(θ, φ)`",
    );

    let sigma0 = 30.0_f64;
    let sz = 30.0_f64;
    let ss = 20.0_f64;
    let angles = [0.0_f64, 30.0, 45.0, 60.0, 90.0];

    // 1. Closed-form traction & utilization checks on Stress = [0, 0, σ₀, 0, 0, 0].
    let s_tens: Stress = [0.0, 0.0, sigma0, 0.0, 0.0, 0.0];
    let s_comp: Stress = [0.0, 0.0, -sigma0, 0.0, 0.0, 0.0];

    let mut closed_rows = Vec::new();
    for &theta_deg in &angles {
        let b_hat = build_axis_from_angles(theta_deg, 0.0);
        let th = theta_deg.to_radians();
        let exact_snn = sigma0 * th.cos() * th.cos();
        let exact_tnt = sigma0 * th.sin() * th.cos();
        let exact_q = (exact_snn.max(0.0) / sz).powi(2) + (exact_tnt / ss).powi(2);
        let exact_u = exact_q.sqrt();

        let (snn, tnt) = layer_tractions(&s_tens, b_hat);
        let q = layer_peel_quadratic(&s_tens, b_hat, sz, ss);
        let u = layer_peel_utilization(&s_tens, b_hat, sz, ss);
        closed_rows.push((theta_deg, exact_snn, exact_tnt, exact_u, snn, tnt, q, u));
    }

    let u_comp_0 = layer_peel_utilization(&s_comp, build_axis_from_angles(0.0, 0.0), sz, ss);
    let u_comp_45 = layer_peel_utilization(&s_comp, build_axis_from_angles(45.0, 0.0), sz, ss);

    report
        .section(
            "Closed-form traction decomposition and quadratic peel criterion (`θ ∈ 0°..90°`)",
            "For uniaxial stress `σ_zz = 30 MPa` and layer normal `b̂(θ, 0) = [sin θ, 0, cos θ]`, \
             the exact layer-plane normal traction is `σ_nn = σ₀ cos²θ`, the exact in-layer \
             shear traction is `τ_nt = σ₀ sin θ cos θ`, and the Hoffman/Puck tension-only \
             utilization with `S_z = 30 MPa, S_s = 20 MPa` is \
             `U_peel(θ) = √((max(0, σ_nn)/S_z)² + (τ_nt/S_s)²)`. Under compression (`σ_zz = -30 MPa`), \
             normal peel is zeroed at `θ = 0°` while inter-layer shear at `θ = 45°` remains `0.75`.",
        )
        .check(Check::absolute(
            "tension θ=0°: U_peel vs σ₀ / S_z = 1.0",
            closed_rows[0].3,
            closed_rows[0].7,
            "×",
            1e-12,
        ))
        .check(Check::absolute(
            "tension θ=30°: U_peel vs √((22.5/30)² + (12.9904/20)²)",
            closed_rows[1].3,
            closed_rows[1].7,
            "×",
            1e-12,
        ))
        .check(Check::absolute(
            "tension θ=45°: U_peel vs √((15/30)² + (15/20)²) = 0.90139",
            closed_rows[2].3,
            closed_rows[2].7,
            "×",
            1e-12,
        ))
        .check(Check::absolute(
            "tension θ=60°: U_peel vs √((7.5/30)² + (12.9904/20)²)",
            closed_rows[3].3,
            closed_rows[3].7,
            "×",
            1e-12,
        ))
        .check(Check::absolute(
            "tension θ=90°: U_peel vs 0.0 (pull parallel to layers)",
            0.0,
            closed_rows[4].7,
            "×",
            1e-12,
        ))
        .check(Check::absolute(
            "compression θ=0°: normal compression does not trigger peel",
            0.0,
            u_comp_0,
            "×",
            1e-12,
        ))
        .check(Check::absolute(
            "compression θ=45°: inter-layer shear utilization |τ_nt| / S_s = 0.75",
            0.75,
            u_comp_45,
            "×",
            1e-12,
        ));

    // 2. Full 3D P2 finite-element uniaxial bar solved across build=(θ, 0).
    let mut fe_rows = Vec::new();
    let mut first_analysis = None;
    for &(theta_deg, _, _, exact_u, _, _, _, _) in &closed_rows {
        let src = bar_doc_with_build(theta_deg, 0.0);
        let doc = fsm::parse(&src).expect("parse bar doc");
        let a = analyse(&doc, Resolution::Size(5.0), now).expect("solve 3D bar");
        let c = &a.cases[0];
        let min_elem = c
            .peel_elem
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        let max_elem = c.max_layer_peel;
        fe_rows.push((
            theta_deg,
            exact_u,
            min_elem,
            max_elem,
            c.max_vm_elem,
            a.build_angles,
        ));
        if first_analysis.is_none() {
            first_analysis = Some(a);
        }
    }
    let a0 = first_analysis.expect("at least one analysis");

    let th15 = 15.0_f64.to_radians();
    let exact_u_15 = ((sigma0 * th15.cos().powi(2) / sz).powi(2)
        + (sigma0 * th15.sin() * th15.cos() / ss).powi(2))
    .sqrt();

    report
        .section(
            "3D P2 finite-element uniaxial bar across `solve build=(θ, 0)`",
            "Solving the `10 × 10 × 40 mm` uniaxial tension bar (`σ_zz = 30 MPa`) with P2 \
             tetrahedra (`h = 5.0 mm`) across `solve build=(θ, 0)` reproduces the exact \
             layer-peel utilization at every element centroid and in `CaseResult::max_layer_peel`. \
             Because `S_s = 20 MPa < S_z / √2 = 21.21 MPa`, the quadratic normal+shear interaction \
             exceeds the pure-normal `θ = 0°` value (`1.0000×`) at oblique tilt (`θ = 15°`, \
             `U_peel(15°) = 1.005553×`), while the hemisphere orientation sweep (`15°` grid) \
             finds `best_build_angles = [90°, 0°]` (`min_peak_peel = 0`, where layers run \
             parallel to the tensile axis) and `worst_build_angles = [15°, 0°]` \
             (`max_peak_peel = 1.005553×`).",
        )
        .check(Check::absolute(
            "3D FE θ=0°: max_layer_peel vs closed form 1.0000",
            fe_rows[0].1,
            fe_rows[0].3,
            "×",
            1e-4,
        ))
        .check(Check::absolute(
            "3D FE θ=30°: max_layer_peel vs closed form 0.9922",
            fe_rows[1].1,
            fe_rows[1].3,
            "×",
            1e-4,
        ))
        .check(Check::absolute(
            "3D FE θ=45°: max_layer_peel vs closed form 0.9014",
            fe_rows[2].1,
            fe_rows[2].3,
            "×",
            1e-4,
        ))
        .check(Check::absolute(
            "3D FE θ=60°: max_layer_peel vs closed form 0.6960",
            fe_rows[3].1,
            fe_rows[3].3,
            "×",
            1e-4,
        ))
        .check(Check::absolute(
            "3D FE θ=90°: max_layer_peel vs closed form 0.0000",
            fe_rows[4].1,
            fe_rows[4].3,
            "×",
            1e-4,
        ))
        .check(Check::absolute(
            "orientation sweep: best build tilt θ = 90° (pull in layer plane)",
            90.0,
            a0.best_build_angles[0],
            "deg",
            1e-9,
        ))
        .check(Check::absolute(
            "orientation sweep: min_peak_peel at θ = 90°",
            0.0,
            a0.min_peak_peel,
            "×",
            1e-4,
        ))
        .check(Check::absolute(
            "orientation sweep: worst build tilt θ = 15° (combined normal+shear peak)",
            15.0,
            a0.worst_build_angles[0],
            "deg",
            1e-9,
        ))
        .check(Check::absolute(
            "orientation sweep: max_peak_peel at θ = 15° vs closed form 1.005553×",
            exact_u_15,
            a0.max_peak_peel,
            "×",
            1e-4,
        ));

    report.finish();
}
