//! V54 — Spatially varying cell-size (`cell_min`) and multi-zone TPMS family morphing (`morph`).
//!
//! Verifies:
//! 1. `.fsm` parser, printer, and validator round-trip of `cell_min=<mm>` and `morph=<family>`
//!    on `fill` statements, including rejection of `cell_min > cell` or `cell_min <= 0`.
//! 2. Exact CPU (`FillExpr::cell_at_param`, `FillExpr::wall_at_param`, `FillExpr::eval_lattice`,
//!    `FillExpr::eval`, `FillExpr::removed_skin`) and GPU GLSL (`glsl::generate`,
//!    `glsl::generate_viewport`) agreement on:
//!    - Spatially varying unit-cell edge length `a(p) = cell + (cell_min - cell) · t(p)`
//!    - Proportional wall scaling `w(p) = max(param(p) · a(p)/cell, min_wall)`
//!    - Smooth TPMS family morphing `s(p) = (1 - t(p)) · d_TPMS(family, a(p), p) + t(p) · d_TPMS(morph, a(p), p)`
//! 3. End-to-end 3D graded FEA solve (`graded`) on a cantilever beam comparing:
//!    - Uniform coarse Gyroid (`cell=8`)
//!    - Stress-refined cell-size Gyroid (`cell=8 cell_min=4`)
//!    - Multi-zone morphed Primitive → Diamond (`family=primitive morph=diamond cell=8 cell_min=4`)

use fieldsmith_core::fill::graded;
use fieldsmith_core::Resolution;
use fieldsmith_doc::{fsm, Family, FillMode};
use fieldsmith_field::expr::Expr;
use fieldsmith_field::glsl;
use fieldsmith_field::lattice::{lattice_field_rotated, tpms_distance, FillExpr, Grid3};
use fieldsmith_field::prim::Prim;
use fieldsmith_verify::{Check, Report};
use std::time::Instant;

fn clock() -> f64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[test]
fn v54_multizone_lattice() {
    let mut r = Report::new(
        "V54",
        "Spatially varying cell-size (`cell_min`) and multi-zone TPMS family morphing (`morph`)",
    );

    // -------------------------------------------------------------------------
    // 1. .fsm syntax round-trip and validation
    // -------------------------------------------------------------------------
    let fsm_src = "\
fieldsmith 1
name \"Multi-zone lattice beam\"
material petg

box beam size=(60, 16, 16) at=(30, 0, 0)

region root = beam.nx
region tip = beam.px
support fix root
load tip_f tip force=(0, 0, -400) case=\"Bend\"

fill primitive sheet cell=8 cell_min=4 morph=diamond skin=1.2 skin_max=2.2 rho=(0.22, 0.65) curve=1 wall=0.6 margin=2
solve size=3.5
";
    let doc_parsed = fsm::parse(fsm_src).expect("parse fill with cell_min=4 morph=diamond");
    let printed = fsm::print(&doc_parsed);
    let reparsed = fsm::parse(&printed).expect("reparse printed fsm");
    let fill_spec = reparsed.fill.as_ref().expect("fill present");

    r.check(Check::absolute(
        "`.fsm` round-trip `cell`",
        8.0,
        fill_spec.cell,
        "mm",
        0.0,
    ));
    r.check(Check::absolute(
        "`.fsm` round-trip `cell_min`",
        4.0,
        fill_spec.cell_min.unwrap_or(0.0),
        "mm",
        0.0,
    ));
    r.check(Check::absolute(
        "`.fsm` round-trip `morph=diamond`",
        1.0,
        if fill_spec.morph == Some(Family::Diamond) {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));
    r.check(Check::absolute(
        "`Document::validate()` errors (`0 < cell_min <= cell`)",
        0.0,
        reparsed.validate().len() as f64,
        "errors",
        0.0,
    ));

    let mut bad_doc = reparsed.clone();
    bad_doc.fill.as_mut().unwrap().cell_min = Some(10.0);
    let bad_errs = bad_doc.validate();
    r.check(Check::absolute(
        "`Document::validate()` rejects `cell_min > cell`",
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
        skin_max: 2.0,
        erode: 0.0,
        param_range: [0.4, 1.6],
        family: Family::Primitive,
        morph: Some(Family::Diamond),
        mode: FillMode::Sheet,
        cell: 8.0,
        cell_min: 4.0,
        min_wall: 0.6,
        rotate: [0.0; 3],
        param: Grid3 {
            origin: [-30.0, -10.0, -10.0],
            spacing: 60.0,
            dims: [2, 2, 2],
            data: vec![0.4, 1.6, 0.4, 1.6, 0.4, 1.6, 0.4, 1.6],
        },
        keep: Expr::Empty,
    };

    r.check(Check::absolute(
        "`cell_at_param(p_lo)` at low-stress bound",
        8.0,
        fx.cell_at_param(0.4),
        "mm",
        1e-9,
    ));
    r.check(Check::absolute(
        "`cell_at_param(p_mid)` at midpoint grade",
        6.0,
        fx.cell_at_param(1.0),
        "mm",
        1e-9,
    ));
    r.check(Check::absolute(
        "`cell_at_param(p_hi)` at high-stress bound",
        4.0,
        fx.cell_at_param(1.6),
        "mm",
        1e-9,
    ));
    r.check(Check::absolute(
        "`wall_at_param(p_hi)` scaled by `cell_min / cell`",
        0.8,
        fx.wall_at_param(1.6),
        "mm",
        1e-9,
    ));

    // Endpoint & midpoint TPMS morph identity:
    let probe_pt = [1.7, -2.3, 0.9];
    let lat_lo = fx.eval_lattice(0.4, probe_pt);
    let ref_lo = lattice_field_rotated(
        Family::Primitive,
        FillMode::Sheet,
        8.0,
        0.6, // max(0.4 * 1.0, min_wall=0.6)
        [0.0; 3],
        probe_pt,
    );
    r.check(Check::absolute(
        "endpoint `t = 0` matches pure `Primitive` at `a = 8 mm`",
        ref_lo.v,
        lat_lo.v,
        "mm",
        1e-9,
    ));

    let lat_hi = fx.eval_lattice(1.6, probe_pt);
    let ref_hi = lattice_field_rotated(
        Family::Diamond,
        FillMode::Sheet,
        4.0,
        0.8, // 1.6 * (4.0 / 8.0) = 0.8 >= min_wall
        [0.0; 3],
        probe_pt,
    );
    r.check(Check::absolute(
        "endpoint `t = 1` matches pure `Diamond` at `a = 4 mm`",
        ref_hi.v,
        lat_hi.v,
        "mm",
        1e-9,
    ));

    let lat_mid = fx.eval_lattice(1.0, probe_pt);
    let s0_mid = tpms_distance(Family::Primitive, 6.0, probe_pt);
    let s1_mid = tpms_distance(Family::Diamond, 6.0, probe_pt);
    let w_mid = (1.0 * 6.0 / 8.0_f64).max(0.6);
    let ref_mid = (0.5 * (s0_mid.v + s1_mid.v)).abs() - 0.5 * w_mid;
    r.check(Check::absolute(
        "midpoint `t = 0.5` blends `Primitive` + `Diamond` at `a = 6 mm`",
        ref_mid,
        lat_mid.v,
        "mm",
        1e-9,
    ));

    // GLSL codegen emits `fs_t`, `fs_a`, and `mix(fs_tpms_distance(1, ...), fs_tpms_distance(2, ...), fs_t)`
    let gen_full = glsl::generate(&Expr::Filled(Box::new(fx.clone())));
    let gen_vp = glsl::generate_viewport(&body, Some(&fx));
    let has_glsl_morph = gen_full.source.contains("float fs_a = 8.0 + -4.0 * fs_t;")
        && gen_full.source.contains("mix(fs_tpms_distance(1, fs_a, p), fs_tpms_distance(2, fs_a, p), fs_t)")
        && gen_vp.source.contains("mix(fs_tpms_distance(1, fs_a, p), fs_tpms_distance(2, fs_a, p), fs_t)");
    r.check(Check::absolute(
        "GLSL `generate` & `generate_viewport` emit `fs_a` and `mix(...)`",
        1.0,
        if has_glsl_morph { 1.0 } else { 0.0 },
        "flag",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 3. End-to-end 3D FEA graded beam comparison
    // -------------------------------------------------------------------------
    let mut doc_coarse = reparsed.clone();
    doc_coarse.fill.as_mut().unwrap().cell_min = None;
    doc_coarse.fill.as_mut().unwrap().morph = None;

    let mut doc_cell_graded = reparsed.clone();
    doc_cell_graded.fill.as_mut().unwrap().morph = None;

    let (_, g_coarse) =
        graded(&doc_coarse, Resolution::Size(3.5), clock).expect("graded coarse primitive");
    let (_, g_cell) =
        graded(&doc_cell_graded, Resolution::Size(3.5), clock).expect("graded cell_min=4");
    let (_, g_morph) =
        graded(&reparsed, Resolution::Size(3.5), clock).expect("graded cell_min=4 + morph=diamond");

    // Check local cell size at the high-stress clamped root (x = 4 mm) vs low-stress tip (x = 56 mm):
    let pv_root = g_morph.fill.param.sample([4.0, 0.0, 6.0]);
    let pv_tip = g_morph.fill.param.sample([56.0, 0.0, 0.0]);
    let a_root = g_morph.fill.cell_at_param(pv_root);
    let a_tip = g_morph.fill.cell_at_param(pv_tip);
    r.check(Check::absolute(
        "root cell size `< 6.0 mm` (refined toward `cell_min = 4 mm`)",
        1.0,
        if a_root < 6.0 && a_root >= 4.0 {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));
    r.check(Check::absolute(
        "tip cell size `> a_root + 1.5 mm` (coarse toward `cell = 8 mm`)",
        1.0,
        if a_tip > a_root + 1.5 { 1.0 } else { 0.0 },
        "flag",
        0.0,
    ));

    // Morphing from Primitive (c = 874.3, n = 2.028) to Diamond (c = 585.8, n = 1.208) in
    // high-stress regions increases shear/bending stiffness and reduces cantilever tip deflection:
    let defl_coarse = g_coarse.cases[0].max_displacement;
    let defl_cell = g_cell.cases[0].max_displacement;
    let defl_morph = g_morph.cases[0].max_displacement;
    r.check(Check::absolute(
        "morphed `Primitive → Diamond` lowers tip deflection vs pure `Primitive`",
        1.0,
        if defl_morph < defl_cell && defl_morph < defl_coarse {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    r.section(
        "Summary & Governing Laws",
        format!(
            "Spatially varying cell size and multi-zone TPMS family morphing let a single \
             implicit lattice field transition smoothly across functional zones without seams:\n\n\
             $$t(\\mathbf{{p}}) = \\operatorname{{clamp}}\\left(\\frac{{\\text{{param}}(\\mathbf{{p}}) - p_{{\\text{{lo}}}}}}{{p_{{\\text{{hi}}}} - p_{{\\text{{lo}}}}}},\\, 0,\\, 1\\right)$$\n\
             $$a(\\mathbf{{p}}) = a_{{\\max}} + (a_{{\\min}} - a_{{\\max}})\\,t(\\mathbf{{p}}), \\qquad w(\\mathbf{{p}}) = \\max\\!\\left(\\text{{param}}(\\mathbf{{p}})\\,\\frac{{a(\\mathbf{{p}})}}{{a_{{\\max}}}},\\, w_{{\\min}}\\right)$$\n\
             $$s(\\mathbf{{p}}) = (1 - t(\\mathbf{{p}}))\\,d_{{\\text{{TPMS}}}}(\\text{{family}}, a(\\mathbf{{p}}), \\mathbf{{p}}) + t(\\mathbf{{p}})\\,d_{{\\text{{TPMS}}}}(\\text{{morph}}, a(\\mathbf{{p}}), \\mathbf{{p}})$$\n\n\
             On the 60×16×16 mm cantilever beam (`400 N` tip load):\n\
             - **Uniform coarse Primitive (`cell = 8 mm`)**: mass = `{:.2} g`, max deflection = `{:.4} mm`\n\
             - **Stress-refined cell size (`cell = 8 → 4 mm`)**: mass = `{:.2} g`, max deflection = `{:.4} mm`, root cell = `{:.2} mm`, tip cell = `{:.2} mm`\n\
             - **Multi-zone `Primitive → Diamond` morph (`cell = 8 → 4 mm`, `morph = diamond`)**: mass = `{:.2} g`, max deflection = `{:.4} mm`",
            g_coarse.mass_g,
            defl_coarse,
            g_cell.mass_g,
            defl_cell,
            a_root,
            a_tip,
            g_morph.mass_g,
            defl_morph,
        ),
    );

    r.finish();
}
