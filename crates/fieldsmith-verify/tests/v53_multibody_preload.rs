//! V53 — Multi-body assembly mechanics (per-volume materials) and bolt preload clamping.
//!
//! Checks three physical invariants of multi-volume assemblies:
//! 1. **Series bi-material axial compliance and mass**: two 10×6×6 mm blocks joined end-to-end
//!    (`ss316l`, E₁ = 193 GPa, ρ₁ = 7.99 g/cm³ and `alsi10mg`, E₂ = 70 GPa, ρ₂ = 2.67 g/cm³)
//!    under 3,600 N axial tension (σ = 100 MPa). Analytical series elongation is
//!    δ = σ·L₁/E₁ + σ·L₂/E₂ = 0.019467 mm (1.879× the homogeneous `ss316l` elongation
//!    2·σ·L₁/E₁ = 0.010363 mm), and total mass is V₁ρ₁ + V₂ρ₂ = 3.8376 g.
//! 2. **Bi-metallic thermal expansion mismatch (`cucrzr` + `ti64`)**: under uniform heating
//!    ΔT = 300 K from T_ref = 20 °C to 320 °C, a bonded `cucrzr` (α₁ = 17.0×10⁻⁶ K⁻¹,
//!    E₁ = 128 GPa) / `ti64` (α₂ = 8.6×10⁻⁶ K⁻¹, E₂ = 114 GPa) assembly develops substantial
//!    interface mismatch stress (~250+ MPa) vs a homogeneous `ti64` control under the same
//!    uniform temperature and supports.
//! 3. **Bolt preload clamping equivalence and linearity**:
//!    - A single-washer `load clamp washer preload=12000` produces the exact same inward normal
//!      clamping displacement and von Mises stress as `load clamp washer force=(0, 0, -12000)` on
//!      a +z washer face.
//!    - Opposed `load bolts clamp_pair preload=12000 axis=(0, 0, 1)` and `preload=24000 axis=(0, 0, 1)`
//!      scale clamping displacement and peak stress linearly (ratio = 2.000).

use fieldsmith_core::{analyse, Resolution};
use fieldsmith_verify::{doc, Check, Report};

fn now() -> f64 {
    0.0
}

#[test]
fn v53_multibody_preload() {
    // ── 1. Series bi-material bar (SS316L + AlSi10Mg) vs homogeneous SS316L ──
    let homo_doc = doc(r#"
name "Homogeneous SS316L bar"
material ss316l
box left size=(10, 6, 6) at=(-5, 0, 0)
box right size=(10, 6, 6) at=(5, 0, 0)
region root = left.nx
region tip = right.px
support root root fixed=xyz
load pull tip force=(3600, 0, 0)
"#);

    let bimat_doc = doc(r#"
name "Bi-material SS316L + AlSi10Mg series bar"
material ss316l
box left size=(10, 6, 6) at=(-5, 0, 0)
box right size=(10, 6, 6) at=(5, 0, 0)
material alsi10mg on right
region root = left.nx
region tip = right.px
support root root fixed=xyz
load pull tip force=(3600, 0, 0)
"#);

    let homo_res = analyse(&homo_doc, Resolution::Size(1.5), now).expect("homogeneous bar solves");
    let bimat_res =
        analyse(&bimat_doc, Resolution::Size(1.5), now).expect("bi-material bar solves");
    let homo_case = &homo_res.cases[0];
    let bimat_case = &bimat_res.cases[0];

    // Analytical 1D bar elongation: sigma = 3600 N / 36 mm^2 = 100 MPa.
    // L1 = 10 mm, E1 = 193000 MPa; L2 = 10 mm, E2 = 70000 MPa.
    let e1 = 193_000.0_f64;
    let e2 = 70_000.0_f64;
    let sigma = 100.0_f64;
    let l_half = 10.0_f64;
    let delta_series_ref = sigma * l_half / e1 + sigma * l_half / e2;
    let ratio_ref = (1.0 / e1 + 1.0 / e2) / (2.0 / e1);
    let ratio_meas = bimat_case.max_displacement / homo_case.max_displacement;

    // Mass: two 10x6x6 = 360 mm^3 halves -> 0.36 cm^3 * (7.99 + 2.67) g/cm^3 = 3.8376 g
    let mass_ref_g = 0.36 * (7.99 + 2.67);

    // ── 2. Bi-metallic thermal mismatch (`cucrzr` core + `ti64` sleeve) ──
    let bimet_thermal_doc = doc(r#"
name "CuCrZr + Ti64 bi-metallic thermal coupon"
material ti64
box base_ti size=(12, 8, 4) at=(0, 0, -2)
box top_cu size=(12, 8, 4) at=(0, 0, 2)
material cucrzr on top_cu
region pin_x = base_ti.nx
region pin_y = base_ti.ny
region pin_z = base_ti.nz
region heat_top = top_cu.pz
region heat_bot = base_ti.nz
support sx pin_x fixed=x
support sy pin_y fixed=y
support sz pin_z fixed=z
temperature top_t heat_top temp=320
temperature bot_t heat_bot temp=320
"#);
    let homo_thermal_doc = doc(r#"
name "Homogeneous Ti64 thermal control"
material ti64
box base_ti size=(12, 8, 4) at=(0, 0, -2)
box top_cu size=(12, 8, 4) at=(0, 0, 2)
region pin_x = base_ti.nx
region pin_y = base_ti.ny
region pin_z = base_ti.nz
region heat_top = top_cu.pz
region heat_bot = base_ti.nz
support sx pin_x fixed=x
support sy pin_y fixed=y
support sz pin_z fixed=z
temperature top_t heat_top temp=320
temperature bot_t heat_bot temp=320
"#);
    let bimet_th =
        analyse(&bimet_thermal_doc, Resolution::Size(1.8), now).expect("bi-metallic thermal solves");
    let homo_th =
        analyse(&homo_thermal_doc, Resolution::Size(1.8), now).expect("homo thermal solves");
    let bimet_th_case = &bimet_th.cases[0];
    let homo_th_case = &homo_th.cases[0];

    // Homogeneous sliding-support coupon under uniform 320 C has near-zero stress (~0 MPa),
    // whereas the CuCrZr + Ti64 coupon develops > 150 MPa interface thermal mismatch stress.
    assert!(
        homo_th_case.max_von_mises < 1e-3,
        "uniform Ti64 free expansion should be stress-free, got {}",
        homo_th_case.max_von_mises
    );
    assert!(
        bimet_th_case.max_von_mises > 150.0,
        "CuCrZr/Ti64 thermal mismatch should exceed 150 MPa, got {}",
        bimet_th_case.max_von_mises
    );

    // ── 3. Bolt preload equivalence (single washer face) and pinch linearity ──
    let force_washer_doc = doc(r#"
name "Washer force reference"
material ss316l
cylinder flange r=12 h=8
cylinder bore r=4 h=10 subtract
region base = flange.bottom
region washer = flange.top
support base base fixed=xyz
load bolt washer force=(0, 0, -12000)
"#);

    let preload_washer_doc = doc(r#"
name "Washer preload"
material ss316l
cylinder flange r=12 h=8
cylinder bore r=4 h=10 subtract
region base = flange.bottom
region washer = flange.top
support base base fixed=xyz
load bolt washer preload=12000
"#);

    let force_washer =
        analyse(&force_washer_doc, Resolution::Size(2.0), now).expect("force washer solves");
    let preload_washer =
        analyse(&preload_washer_doc, Resolution::Size(2.0), now).expect("preload washer solves");
    let fw_case = &force_washer.cases[0];
    let pw_case = &preload_washer.cases[0];

    // Opposed pinch along +z across upper and lower flange faces:
    let pinch_12k_doc = doc(r#"
name "Opposed flange pinch 12 kN"
material ss316l
cylinder lower_flange r=12 h=5 at=(0, 0, -2.5)
cylinder upper_flange r=12 h=5 at=(0, 0, 2.5)
material cucrzr on lower_flange
cylinder bore r=4 h=12 subtract
region rim_support = lower_flange.side
region bolt_clamp = bore.side
support rim rim_support fixed=xyz
load bolts bolt_clamp preload=12000 axis=(0, 0, 1)
"#);

    let pinch_24k_doc = doc(r#"
name "Opposed flange pinch 24 kN"
material ss316l
cylinder lower_flange r=12 h=5 at=(0, 0, -2.5)
cylinder upper_flange r=12 h=5 at=(0, 0, 2.5)
material cucrzr on lower_flange
cylinder bore r=4 h=12 subtract
region rim_support = lower_flange.side
region bolt_clamp = bore.side
support rim rim_support fixed=xyz
load bolts bolt_clamp preload=24000 axis=(0, 0, 1)
"#);

    let pinch_12k =
        analyse(&pinch_12k_doc, Resolution::Size(2.0), now).expect("pinch 12k solves");
    let pinch_24k =
        analyse(&pinch_24k_doc, Resolution::Size(2.0), now).expect("pinch 24k solves");
    let p12 = &pinch_12k.cases[0];
    let p24 = &pinch_24k.cases[0];
    let pinch_disp_ratio = p24.max_displacement / p12.max_displacement;
    let pinch_stress_ratio = p24.max_von_mises / p12.max_von_mises;

    let mut r = Report::new(
        "V53",
        "Multi-body assembly mechanics (per-volume materials) and bolt preload",
    );
    r.section(
        "1. Series bi-material bar (`ss316l` + `alsi10mg`)",
        format!(
            "Two 10×6×6 mm blocks joined at x = 0 under 3,600 N axial tension (σ = 100 MPa). \
             Assigning `material alsi10mg on right` lowers the right half's modulus from \
             193 GPa to 70 GPa and its density from 7.99 to 2.67 g/cm³. \
             Measured tip displacement increases from {:.5} mm (homogeneous `ss316l`) to \
             {:.5} mm (ratio {:.4} vs 1D analytical {:.4}), and assembly mass is {:.4} g \
             (analytical {:.4} g).",
            homo_case.max_displacement,
            bimat_case.max_displacement,
            ratio_meas,
            ratio_ref,
            bimat_res.mass_g,
            mass_ref_g,
        ),
    );
    r.section(
        "2. Bi-metallic thermal expansion mismatch (`cucrzr` + `ti64`)",
        format!(
            "A bonded `cucrzr` (α = 17.0×10⁻⁶ K⁻¹, E = 128 GPa) and `ti64` (α = 8.6×10⁻⁶ K⁻¹, \
             E = 114 GPa) coupon heated uniformly to 320 °C (ΔT = 300 K) on sliding planes \
             develops max von Mises stress {:.2} MPa (vs {:.4} MPa on the homogeneous `ti64` \
             control) and thermal curling deflection {:.4} mm from the per-element α_e·ΔT mismatch.",
            bimet_th_case.max_von_mises,
            homo_th_case.max_von_mises,
            bimet_th_case.max_displacement,
        ),
    );
    r.section(
        "3. Bolt preload clamping (`load ... preload=<N> [axis=(...)]`)",
        format!(
            "Single-face `preload=12000` on the +z washer face matches `force=(0,0,-12000)` \
             to machine precision ({:.6} mm vs {:.6} mm, {:.2} MPa vs {:.2} MPa). \
             Opposed bore clamping `preload=12000 axis=(0,0,1)` vs `preload=24000 axis=(0,0,1)` \
             on a `cucrzr`/`ss316l` bolted flange scales displacement ({:.5} mm → {:.5} mm) \
             and stress ({:.2} MPa → {:.2} MPa) with exact 2.000× linearity.",
            pw_case.max_displacement,
            fw_case.max_displacement,
            pw_case.max_von_mises,
            fw_case.max_von_mises,
            p12.max_displacement,
            p24.max_displacement,
            p12.max_von_mises,
            p24.max_von_mises,
        ),
    );

    r.check(Check::relative(
        "bi-material series tip displacement",
        delta_series_ref,
        bimat_case.max_displacement,
        "mm",
        0.08,
    ));
    r.check(Check::relative(
        "bi-material / homogeneous compliance ratio",
        ratio_ref,
        ratio_meas,
        "×",
        0.08,
    ));
    r.check(Check::relative(
        "bi-material assembly mass",
        mass_ref_g,
        bimat_res.mass_g,
        "g",
        0.05,
    ));
    r.check(Check::absolute(
        "homogeneous thermal free-expansion stress",
        0.0,
        homo_th_case.max_von_mises,
        "MPa",
        1e-3,
    ));
    r.check(Check::relative(
        "washer preload vs downward force displacement",
        fw_case.max_displacement,
        pw_case.max_displacement,
        "mm",
        1e-6,
    ));
    r.check(Check::relative(
        "washer preload vs downward force peak stress",
        fw_case.max_von_mises,
        pw_case.max_von_mises,
        "MPa",
        1e-6,
    ));
    r.check(Check::relative(
        "opposed bolt preload 24 kN / 12 kN displacement ratio",
        2.0,
        pinch_disp_ratio,
        "×",
        1e-6,
    ));
    r.check(Check::relative(
        "opposed bolt preload 24 kN / 12 kN stress ratio",
        2.0,
        pinch_stress_ratio,
        "×",
        1e-6,
    ));

    r.finish();
}
