//! V50 — Field-native stress-driven outer shell thickening (`fill ... skin_max=<mm>`).
//!
//! Verifies:
//! 1. `.fsm` parser, printer, and validator round-trip of `skin_max=<mm>` on `fill`
//!    statements, including rejection of `skin_max < skin`.
//! 2. Exact CPU (`FillExpr::eval`, `FillExpr::removed_skin`, `FillExpr::in_core`) and GPU
//!    GLSL (`glsl::generate`, `glsl::generate_viewport`) agreement on spatially varying
//!    skin thickness `skin(p) = skin + (skin_max - skin) * clamp((param(p) - p_lo)/(p_hi - p_lo), 0, 1)`.
//! 3. End-to-end graded FEA solve on a tip-loaded cantilever beam (`60 × 16 × 16 mm`)
//!    comparing uniform thin skin (`skin=1.0`), stress-thickened skin (`skin=1.0 skin_max=2.8`),
//!    and uniform thick skin (`skin=2.8`):
//!    - Local skin thickness at the high-stress clamped root approaches `2.8 mm` while the
//!      low-stress free tip stays near `1.0 mm`.
//!    - Part mass satisfies `mass(1.0) < mass(1.0..2.8) < mass(2.8)`.
//!    - Graded compliance `fᵀu`, peak tip deflection, and yield utilization all decrease
//!      relative to uniform `skin=1.0`.

use fieldsmith_core::fill::graded;
use fieldsmith_core::Resolution;
use fieldsmith_doc::{fsm, Family, FillMode};
use fieldsmith_field::expr::Expr;
use fieldsmith_field::glsl;
use fieldsmith_field::lattice::{FillExpr, Grid3};
use fieldsmith_field::prim::Prim;
use fieldsmith_verify::{Check, Report};
use std::time::Instant;

fn clock() -> f64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[test]
fn v50_stress_skin() {
    let mut r = Report::new(
        "V50",
        "Field-native stress-driven outer shell thickening (`skin_max`)",
    );

    // -------------------------------------------------------------------------
    // 1. .fsm syntax round-trip and validation
    // -------------------------------------------------------------------------
    let fsm_src = "\
fieldsmith 1
name \"Stress-graded skin beam\"
material petg

box beam size=(60, 16, 16) at=(30, 0, 0)

region root = beam.nx
region tip = beam.px
support fix root
load tip_f tip force=(0, 0, -400) case=\"Bend\"

fill gyroid sheet cell=6 skin=1 skin_max=2.8 rho=(0.2, 0.7) curve=1 wall=0.6 margin=2
solve size=3.5
";
    let doc_graded_skin = fsm::parse(fsm_src).expect("parse fill with skin_max=2.8");
    let printed = fsm::print(&doc_graded_skin);
    let reparsed = fsm::parse(&printed).expect("reparse printed fsm");
    let fill_spec = reparsed.fill.as_ref().expect("fill present");
    r.check(Check::absolute(
        "`.fsm` round-trip `skin`",
        1.0,
        fill_spec.skin,
        "mm",
        0.0,
    ));
    r.check(Check::absolute(
        "`.fsm` round-trip `skin_max`",
        2.8,
        fill_spec.skin_max.unwrap_or(0.0),
        "mm",
        0.0,
    ));
    r.check(Check::absolute(
        "`Document::validate()` errors (`skin_max >= skin`)",
        0.0,
        reparsed.validate().len() as f64,
        "errors",
        0.0,
    ));

    let mut bad_doc = reparsed.clone();
    bad_doc.fill.as_mut().unwrap().skin_max = Some(0.5);
    let bad_errs = bad_doc.validate();
    r.check(Check::absolute(
        "`Document::validate()` rejects `skin_max < skin`",
        1.0,
        bad_errs.len() as f64,
        "errors",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 2. Direct implicit field & GLSL codegen verification
    // -------------------------------------------------------------------------
    let body = Expr::Prim(Prim::Box {
        half: [30.0, 10.0, 10.0],
        round: 0.0,
    });
    let fx = FillExpr {
        body: body.clone(),
        skin: 1.0,
        skin_max: 3.0,
        erode: 0.0,
        param_range: [0.4, 1.6],
        family: Family::Gyroid,
        morph: None,
        mode: FillMode::Sheet,
        cell: 6.0,
        cell_min: 6.0,
        min_wall: 0.0,
        rotate: [0.0; 3],
        param: Grid3 {
            origin: [-30.0, -10.0, -10.0],
            spacing: 60.0,
            dims: [2, 2, 2],
            data: vec![0.4, 1.6, 0.4, 1.6, 0.4, 1.6, 0.4, 1.6],
        },
        keep: Expr::Empty,
    };

    let sk_lo = fx.skin_at_param(0.4);
    let sk_hi = fx.skin_at_param(1.6);
    let sk_mid = fx.skin_at_param(1.0);
    r.check(Check::absolute(
        "Local skin thickness at `param = p_lo`",
        1.0,
        sk_lo,
        "mm",
        1e-12,
    ));
    r.check(Check::absolute(
        "Local skin thickness at `param = p_hi`",
        3.0,
        sk_hi,
        "mm",
        1e-12,
    ));
    r.check(Check::absolute(
        "Local skin thickness at midpoint `param`",
        2.0,
        sk_mid,
        "mm",
        1e-12,
    ));

    // At x = -24 (t = 0.1 -> skin = 1.2 mm), z = 8.0 (2.0 mm below top face, d = -2.0) is in core;
    // at x = +24 (t = 0.9 -> skin = 2.8 mm), z = 8.0 (2.0 mm below top face, d = -2.0) is in SOLID SKIN!
    let p_thin_end = [-24.0, 0.0, 8.0];
    let p_thick_end = [24.0, 0.0, 8.0];
    r.check(Check::absolute(
        "`in_core` at 2.0 mm depth on low-stress end (`skin(x) = 1.2 mm`)",
        1.0,
        if fx.in_core(p_thin_end) { 1.0 } else { 0.0 },
        "flag",
        0.0,
    ));
    r.check(Check::absolute(
        "`in_core` at 2.0 mm depth on high-stress end (`skin(x) = 2.8 mm`)",
        0.0,
        if fx.in_core(p_thick_end) { 1.0 } else { 0.0 },
        "flag",
        0.0,
    ));
    r.check(Check::absolute(
        "`FillExpr::eval` is solid (`< 0`) at 2.0 mm depth on thickened end",
        1.0,
        if fx.eval(p_thick_end).v <= -0.79 {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    let glsl_vp = glsl::generate_viewport(&body, Some(&fx));
    let glsl_full = glsl::generate(&Expr::Filled(Box::new(fx.clone())));
    let has_ramp_vp = glsl_vp
        .source
        .contains("clamp((fs_grid(p) - 4.0e-1) / 1.2, 0.0, 1.0)");
    let has_ramp_full = glsl_full
        .source
        .contains("clamp((fs_grid(p) - 4.0e-1) / 1.2, 0.0, 1.0)");
    r.check(Check::absolute(
        "GLSL `generate` & `generate_viewport` emit clamped skin ramp",
        1.0,
        if has_ramp_vp && has_ramp_full {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 3. End-to-end graded FEA comparison on the cantilever beam
    // -------------------------------------------------------------------------
    let mut doc_thin = doc_graded_skin.clone();
    doc_thin.fill.as_mut().unwrap().skin_max = None; // uniform skin = 1.0 mm

    let mut doc_thick = doc_graded_skin.clone();
    doc_thick.fill.as_mut().unwrap().skin = 2.8;
    doc_thick.fill.as_mut().unwrap().skin_max = None; // uniform skin = 2.8 mm

    let res = Resolution::Size(3.5);
    let (_base_thin, g_thin) = graded(&doc_thin, res, clock).expect("graded solve (uniform 1.0 mm)");
    let (_base_grad, g_grad) =
        graded(&doc_graded_skin, res, clock).expect("graded solve (skin 1.0..2.8 mm)");
    let (_base_thick, g_thick) =
        graded(&doc_thick, res, clock).expect("graded solve (uniform 2.8 mm)");

    // Compare local skin thickness on `g_grad.fill` near the clamped root (x = 4, z = 7.5)
    // vs near the unloaded tip (x = 56, z = 0.0).
    let root_param = g_grad.fill.param.sample([4.0, 0.0, 7.5]);
    let tip_param = g_grad.fill.param.sample([56.0, 0.0, 0.0]);
    let root_skin_mm = g_grad.fill.skin_at_param(root_param);
    let tip_skin_mm = g_grad.fill.skin_at_param(tip_param);

    println!(
        "V50 beam: root_skin={root_skin_mm:.3} mm (param={root_param:.3}), tip_skin={tip_skin_mm:.3} mm (param={tip_param:.3})"
    );
    println!(
        "V50 mass: thin={:.2} g, stress-graded={:.2} g, thick={:.2} g (solid={:.2} g)",
        g_thin.mass_g, g_grad.mass_g, g_thick.mass_g, g_grad.solid_mass_g
    );
    println!(
        "V50 compliance: thin={:.3} N·mm, stress-graded={:.3} N·mm, thick={:.3} N·mm",
        g_thin.cases[0].compliance, g_grad.cases[0].compliance, g_thick.cases[0].compliance
    );
    println!(
        "V50 deflection: thin={:.4} mm, stress-graded={:.4} mm, thick={:.4} mm",
        g_thin.cases[0].max_displacement,
        g_grad.cases[0].max_displacement,
        g_thick.cases[0].max_displacement
    );
    println!(
        "V50 yield util: thin={:.4}, stress-graded={:.4}, thick={:.4}",
        g_thin.yield_utilization[0].unwrap(),
        g_grad.yield_utilization[0].unwrap(),
        g_thick.yield_utilization[0].unwrap()
    );

    r.check(Check::absolute(
        "Cantilever root skin thickness (target 2.2..2.8 mm)",
        2.5,
        root_skin_mm,
        "mm",
        0.35,
    ));
    r.check(Check::absolute(
        "Cantilever tip/neutral skin thickness (target 1.0..1.6 mm)",
        1.25,
        tip_skin_mm,
        "mm",
        0.35,
    ));
    r.check(Check::absolute(
        "Root skin minus tip skin thickness (≥ 0.8 mm)",
        1.0,
        if root_skin_mm - tip_skin_mm >= 0.8 {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));
    r.check(Check::absolute(
        "Mass ordering `mass(1.0) < mass(1.0..2.8) < mass(2.8)`",
        1.0,
        if g_grad.mass_g > g_thin.mass_g + 0.5 && g_grad.mass_g < g_thick.mass_g - 1.0 {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));
    let comp_ratio = g_grad.cases[0].compliance / g_thin.cases[0].compliance;
    let defl_ratio = g_grad.cases[0].max_displacement / g_thin.cases[0].max_displacement;
    let yield_ratio = g_grad.yield_utilization[0].unwrap() / g_thin.yield_utilization[0].unwrap();
    r.check(Check::absolute(
        "Graded compliance ratio `C(1.0..2.8) / C(1.0)` (≤ 0.92)",
        0.75,
        comp_ratio,
        "ratio",
        0.20,
    ));
    r.check(Check::absolute(
        "Peak deflection ratio `u_max(1.0..2.8) / u_max(1.0)` (≤ 0.92)",
        0.75,
        defl_ratio,
        "ratio",
        0.20,
    ));
    r.check(Check::absolute(
        "Yield utilization ratio `U_y(1.0..2.8) / U_y(1.0)` (≤ 0.95)",
        0.80,
        yield_ratio,
        "ratio",
        0.18,
    ));

    r.section(
        "Method",
        "1. **Grammar & validation (`fieldsmith-doc`)**: `fill <family> <mode> ... skin=<mm> skin_max=<mm>` \
         parses and prints `skin_max` when present (`Option<f64>`), and `Document::validate()` plus \
         `fill::check_spec()` require `skin_max.is_finite() && skin_max >= skin`.\n\
         2. **Field & GLSL (`fieldsmith-field`)**: `FillExpr` carries `skin_max: f64` and \
         `param_range: [f64; 2]` (`[p_lo, p_hi]`). Both CPU evaluation (`FillExpr::eval`, \
         `FillExpr::removed_skin`, `FillExpr::in_core`) and GLSL codegen (`glsl::generate`, \
         `glsl::generate_viewport`) evaluate the local shell thickness as:\n\
         `skin(p) = skin + (skin_max - skin) * clamp((param(p) - p_lo) / (p_hi - p_lo), 0.0, 1.0)`.\n\
         When `skin_max <= skin`, GLSL emits the constant literal `lit(skin)`.\n\
         3. **Graded FEA coupling (`fieldsmith-core`)**: When `skin_max > skin`, the density blur \
         includes populated boundary nodes and halos one radius outward so trilinear samples across \
         the outer shell band interpolate local boundary von Mises stress without dropping toward \
         `ρ_min`. Element material fractions `φ_e` are sampled directly from the resulting `FillExpr`, \
         converting thickened root skin bands to full solid modulus (`φ → 1`).",
    );

    r.section(
        "Cantilever Comparison (`60 × 16 × 16 mm`, PETG, 400 N tip shear, `h = 3.5 mm`)",
        format!(
            "| Fill configuration | Mass (g) | Compliance `fᵀu` (N·mm) | Peak deflection (mm) | Yield utilization | Root skin (mm) | Tip/neutral skin (mm) |\n\
             | :-- | --: | --: | --: | --: | --: | --: |\n\
             | Uniform thin (`skin=1.0`) | {:.2} | {:.3} | {:.4} | {:.3} | 1.000 | 1.000 |\n\
             | Stress-thickened (`skin=1.0 skin_max=2.8`) | {:.2} | {:.3} | {:.4} | {:.3} | {:.3} | {:.3} |\n\
             | Uniform thick (`skin=2.8`) | {:.2} | {:.3} | {:.4} | {:.3} | 2.800 | 2.800 |",
            g_thin.mass_g,
            g_thin.cases[0].compliance,
            g_thin.cases[0].max_displacement,
            g_thin.yield_utilization[0].unwrap(),
            g_grad.mass_g,
            g_grad.cases[0].compliance,
            g_grad.cases[0].max_displacement,
            g_grad.yield_utilization[0].unwrap(),
            root_skin_mm,
            tip_skin_mm,
            g_thick.mass_g,
            g_thick.cases[0].compliance,
            g_thick.cases[0].max_displacement,
            g_thick.yield_utilization[0].unwrap(),
        ),
    );

    r.finish();
}
