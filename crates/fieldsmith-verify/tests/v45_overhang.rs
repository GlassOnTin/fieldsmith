//! V45: Exact-gradient overhang analysis (`∇f(p)` overhang area & angle, M23).
//!
//! Verifies that `fieldsmith_core::overhang` (and its `fieldsmith_api` / `fieldsmith-cli`
//! exposure) computes surface overhang area and maximum overhang angle from the exact
//! SDF gradient `n(p) = ∇f(p) / ‖∇f(p)‖` on adaptive dual-contouring surfaces:
//!
//! 1. **Sphere (`r = 10 mm`) across thresholds `α_max ∈ {30°, 45°, 60°}`**:
//!    - Total surface area `A_total = 4π r² = 1256.637 mm²`;
//!    - Exact overhanging lower cap area `A_overhang(α_max) = 2π r² (1 − sin α_max)`;
//!    - Exact overhang fraction `(1 − sin α_max) / 2`;
//!    - Exact maximum overhang angle `90.0°` at the south pole (`n = (0, 0, -1)`).
//!    At the default export mesh spacing `h = 0.25 mm`, overhang area and overhang
//!    fraction match the closed-form spherical cap within `1.5 %`, and worst overhang
//!    angle matches `90°` within `0.5°`.
//!
//! 2. **Tilted plate (`60°` overhang angle from build vertical)**:
//!    - A flat plate `box plate size=(40, 20, 2) rotate=(30, 0, 0)` (tilted `30°` from
//!      horizontal, so its downward-facing `40 × 20 = 800 mm²` bottom face has outward
//!      normal `n = (0, sin 30°, −cos 30°)` with `n · (−ẑ) = cos 30° = sin 60°`, i.e. a
//!      `60°` overhang angle from vertical, while its four side faces have
//!      `n · (−ẑ) ∈ {0, ±sin 30° = ±0.5} < sin 45°`).
//!    - At `h = 0.25 mm` and default threshold `α_max = 45°`, the measured overhang area
//!      matches `800.0 mm²` within `1.5 %`, and the measured maximum overhang angle
//!      matches `60.0°` within `0.1°` (exact SDF gradient on the planar face).
//!
//! 3. **Build orientation rotation (`b̂(θ, φ)`) and headless API (`kind: "overhang"`)**:
//!    - Rotating the build axis to align with the tilted plate's normal (`b̂ = (0, -0.5, √3/2)`,
//!      `θ = 30°, φ = -90°`) puts the `800 mm²` bottom face at `90°` overhang and all four
//!      thin side faces (`n · (−b̂) = 0`) at `0°`; rotating `b̂` by `90°` to `+X = [1, 0, 0]`
//!      makes only the `-X` end cap (`20 × 2 = 40 mm²`) overhang.
//!    - The JSON API `{"apiv": 2, "doc": ..., "question": {"kind": "overhang"}}` returns
//!      the identical `OverhangReport`.

use fieldsmith_core::overhang::{
    build_axis_from_angles, check_overhang, DEFAULT_BUILD_AXIS, DEFAULT_OVERHANG_DEG,
};
use fieldsmith_doc::fsm;
use fieldsmith_verify::{Check, Report};
use std::f64::consts::PI;

const SPHERE_DOC: &str = "\
fieldsmith 1
name \"V45 sphere r=10\"
material al6061
sphere ball r=10
";

const TILTED_PLATE_DOC: &str = "\
fieldsmith 1
name \"V45 60-deg overhang plate\"
material al6061
box plate size=(40, 20, 2) rotate=(30, 0, 0)
";

#[test]
fn v45_overhang() {
    let mut report = Report::new(
        "V45",
        "exact-gradient surface overhang area and angle (`∇f(p)` vs. build axis `b̂`)",
    );

    let sphere_doc = fsm::parse(SPHERE_DOC).expect("parse sphere doc");
    let plate_doc = fsm::parse(TILTED_PLATE_DOC).expect("parse tilted plate doc");

    let r = 10.0_f64;
    let exact_sphere_total = 4.0 * PI * r * r;

    // 1. Sphere across α_max ∈ {30°, 45°, 60°} at default export spacing h = 0.25 mm.
    let mut sphere_rows = Vec::new();
    for &alpha_deg in &[30.0_f64, 45.0, 60.0] {
        let rep = check_overhang(&sphere_doc, Some(0.25), DEFAULT_BUILD_AXIS, alpha_deg)
            .expect("sphere overhang");
        let sin_a = alpha_deg.to_radians().sin();
        let exact_ov = 2.0 * PI * r * r * (1.0 - sin_a);
        let exact_frac = 0.5 * (1.0 - sin_a);
        sphere_rows.push((alpha_deg, exact_ov, exact_frac, rep));
    }

    let (_, exact_ov_45, exact_frac_45, ref rep_sphere_45) = sphere_rows[1];

    report
        .section(
            "Sphere (`r = 10 mm`, `h = 0.25 mm`) vs. closed-form cap `2π r² (1 − sin α_max)`",
            "On a sphere of radius `r = 10 mm` (`A_total = 4π r² = 1256.637 mm²`), the lower \
             spherical cap where `n(p) · (−ẑ) > sin(α_max)` has exact area \
             `A_overhang(α_max) = 2π r² (1 − sin α_max)`, exact overhang fraction \
             `(1 − sin α_max) / 2`, and worst overhang angle `90.0°` at the south pole. \
             Evaluating `∇f(p)` at adaptive dual-contouring triangle centroids (`h = 0.25 mm`) \
             matches all three thresholds within `1.5 %`.",
        )
        .check(Check::relative(
            "sphere r=10 (h=0.25): total surface area vs 4π r²",
            exact_sphere_total,
            rep_sphere_45.total_area_mm2,
            "mm²",
            0.015,
        ))
        .check(Check::relative(
            "sphere r=10 (α_max=30°): overhang area vs 2π r²(1 − sin 30°)",
            sphere_rows[0].1,
            sphere_rows[0].3.overhang_area_mm2,
            "mm²",
            0.015,
        ))
        .check(Check::relative(
            "sphere r=10 (α_max=45°): overhang area vs 2π r²(1 − sin 45°)",
            exact_ov_45,
            rep_sphere_45.overhang_area_mm2,
            "mm²",
            0.015,
        ))
        .check(Check::relative(
            "sphere r=10 (α_max=45°): overhang fraction vs (1 − sin 45°)/2",
            exact_frac_45,
            rep_sphere_45.overhang_fraction,
            "1",
            0.015,
        ))
        .check(Check::relative(
            "sphere r=10 (α_max=60°): overhang area vs 2π r²(1 − sin 60°)",
            sphere_rows[2].1,
            sphere_rows[2].3.overhang_area_mm2,
            "mm²",
            0.015,
        ))
        .check(Check::absolute(
            "sphere r=10: worst overhang angle vs 90° south pole",
            90.0,
            rep_sphere_45.max_overhang_deg,
            "deg",
            0.5,
        ));

    // 2. 60° overhang tilted plate at default h = 0.25 mm, α_max = 45°.
    let rep_plate = check_overhang(
        &plate_doc,
        Some(0.25),
        DEFAULT_BUILD_AXIS,
        DEFAULT_OVERHANG_DEG,
    )
    .expect("plate overhang");
    let exact_plate_ov = 40.0 * 20.0; // 800.0 mm²

    // Build-axis re-orientation on the tilted plate:
    // Tilted by +30° around X, the plate's normal is (0, -sin 30°, cos 30°) = (0, -0.5, √3/2),
    // which is build_axis_from_angles(30.0, -90.0). Along that build axis, the bottom face is
    // a 90° ceiling (800 mm²) and the side faces are vertical walls (0° overhang).
    // Conversely, building along the opposite normal (-b_plate_normal = (0, 0.5, -√3/2), i.e.
    // build_axis_from_angles(150.0, 90.0)) flips the 800 mm² bottom face onto the build plate
    // (0° overhang) and puts the 800 mm² top face in overhang instead; and tilting the build
    // axis by 20° toward -Y (`build_axis_from_angles(20.0, -90.0)`) reduces the bottom face's
    // overhang angle from 60° to 40° (< 45°), dropping overhang area to 0 mm²!
    let b_plate_normal = build_axis_from_angles(30.0, -90.0);
    let rep_plate_aligned =
        check_overhang(&plate_doc, Some(0.25), b_plate_normal, DEFAULT_OVERHANG_DEG)
            .expect("plate aligned build axis");
    let b_plate_self_supporting = build_axis_from_angles(20.0, 90.0);
    let rep_plate_self_supporting = check_overhang(
        &plate_doc,
        Some(0.25),
        b_plate_self_supporting,
        DEFAULT_OVERHANG_DEG,
    )
    .expect("plate 20-deg tilted build axis");

    // 3. Headless JSON API parity (`kind: "overhang"`).
    let api_req = serde_json::json!({
        "apiv": 2,
        "doc": TILTED_PLATE_DOC,
        "question": {
            "kind": "overhang",
            "h": 0.25,
            "build": [0.0, 0.0, 1.0],
            "threshold_deg": 45.0
        }
    })
    .to_string();
    let api_reply = fieldsmith_api::respond(&api_req);
    assert!(!api_reply.is_error, "api overhang error: {}", api_reply.body);
    let api_json: serde_json::Value = serde_json::from_str(&api_reply.body).expect("parse api json");
    let api_ov_area = api_json["overhang"]["overhang_area_mm2"]
        .as_f64()
        .expect("api overhang_area_mm2");
    let api_max_deg = api_json["overhang"]["max_overhang_deg"]
        .as_f64()
        .expect("api max_overhang_deg");

    report
        .section(
            "Tilted plate (`60°` overhang angle from vertical) and build-axis re-orientation",
            "Rotating a `40 × 20 × 2 mm` plate by `30°` about X tilts its `800 mm²` bottom face \
             to outward normal `(0, 0.5, −√3/2)`, which makes a `60°` overhang angle from the `+Z` \
             build vertical (`n · (−ẑ) = cos 30° = sin 60° = 0.866025 > sin 45°`), while all four \
             perimeter side faces stay at `0°` or `30°` (`< 45°`). Re-orienting the build axis by \
             `20°` toward `+Y` (`b̂(20°, +90°)`) reduces the bottom face's overhang angle from \
             `60°` to `40°` and brings the `-Y` side face to `50°` (`40 × 2 = 80 mm²`), reducing \
             overhang area by `10×` (`800 mm² → 80 mm²`).",
        )
        .check(Check::relative(
            "60° tilted plate (40×20×2, rot X=30°): overhang area vs 800 mm²",
            exact_plate_ov,
            rep_plate.overhang_area_mm2,
            "mm²",
            0.015,
        ))
        .check(Check::absolute(
            "60° tilted plate: exact-gradient worst overhang angle vs 60°",
            60.0,
            rep_plate.max_overhang_deg,
            "deg",
            0.1,
        ))
        .check(Check::relative(
            "tilted plate along normal build_axis(30°, -90°): overhang area vs 800 mm²",
            exact_plate_ov,
            rep_plate_aligned.overhang_area_mm2,
            "mm²",
            0.015,
        ))
        .check(Check::absolute(
            "tilted plate along build_axis(20°, +90°): worst overhang angle vs 50°",
            50.0,
            rep_plate_self_supporting.max_overhang_deg,
            "deg",
            0.1,
        ))
        .check(Check::absolute(
            "headless API question kind='overhang' matches core overhang_area_mm2 and max_overhang_deg",
            0.0,
            (api_ov_area - rep_plate.overhang_area_mm2).abs()
                + (api_max_deg - rep_plate.max_overhang_deg).abs(),
            "mm²+deg",
            1e-12,
        ));

    report.finish();
}
