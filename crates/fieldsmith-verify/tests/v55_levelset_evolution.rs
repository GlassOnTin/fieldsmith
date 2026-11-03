//! V55 — Topology optimization / level-set outer boundary evolution (`erode`) on the implicit field grid.
//!
//! Verifies:
//! 1. `.fsm` parser, printer, and validator round-trip of `erode=<mm>` on `fill` statements,
//!    including `erode=0` normalization to `None` and rejection of `erode < 0`.
//! 2. Analytical `FillExpr` level-set boundary evolution and CPU/GLSL parity:
//!    - Inward outer boundary shift `δ(p) = erode · (1 − t(p))` driven by the local normalized
//!      stress/strain-energy grade `t(p) ∈ [0, 1]`
//!    - Full outer envelope preservation along peak load paths (`t = 1 → δ = 0`) and inside
//!      protected zones (`keep`, `d_outer = min(d + δ, max(d, d_keep))`)
//!    - Conformal outer skin (`skin` → `skin_max`) and multi-zone TPMS core following the
//!      evolved outer boundary `d_outer(p) = 0`
//! 3. End-to-end two-pass homogenised FEA (`fieldsmith_core::fill::graded`) on a 3D cantilever
//!    bracket comparing un-eroded graded lattice infill (`erode=0`) against level-set
//!    boundary-evolved topology optimization (`erode=2.5 mm`).

use std::time::Instant;

use fieldsmith_core::fill::graded;
use fieldsmith_core::Resolution;
use fieldsmith_doc::{fsm, Family, FillMode};
use fieldsmith_field::dual::{dmax, dmin, scale, D};
use fieldsmith_field::expr::{self, Expr, Rigid};
use fieldsmith_field::glsl;
use fieldsmith_field::lattice::{FillExpr, Grid3};
use fieldsmith_field::prim::Prim;
use fieldsmith_verify::{Check, Report};

fn clock() -> f64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[test]
fn v55_levelset_evolution() {
    let mut r = Report::new(
        "V55",
        "Topology optimization & level-set outer boundary evolution (`erode`) on the implicit field grid",
    );

    // -------------------------------------------------------------------------
    // 1. .fsm Parser, Printer, and Validator Round-Trip
    // -------------------------------------------------------------------------
    let fsm_src = "\
fieldsmith 1
name \"V55 Level-Set Topology Bracket\"
material petg

box beam size=(60, 18, 18) at=(30, 0, 0)
box root_pad size=(8, 18, 18) at=(4, 0, 0)
box tip_pad size=(8, 18, 18) at=(56, 0, 0)

region clamp = beam.nx
region tip = beam.px

support s1 clamp fixed=xyz
load l1 tip force=(0, 0, -400) case=\"1. Tip Shear\"

fill primitive sheet cell=8 cell_min=4 morph=diamond skin=1.2 skin_max=2.0 erode=2.5 rho=(0.20, 0.65) curve=1 wall=0.6 protect=(root_pad, tip_pad) margin=1.5
solve size=3.5
";

    let doc_eroded = fsm::parse(fsm_src).expect("parse fill with erode=2.5");
    let printed = fsm::print(&doc_eroded);
    let reparsed = fsm::parse(&printed).expect("reparse printed .fsm");
    let fill_spec = reparsed.fill.as_ref().expect("fill present");

    r.check(Check::absolute(
        "`.fsm` round-trip `erode`",
        2.5,
        fill_spec.erode.unwrap_or(0.0),
        "mm",
        1e-9,
    ));
    r.check(Check::absolute(
        "`Document::validate()` errors (`erode >= 0`)",
        0.0,
        reparsed.validate().len() as f64,
        "count",
        0.0,
    ));

    // Verify erode=0 normalizes to None, and negative erode is rejected by validate()
    let doc_zero = fsm::parse(&fsm_src.replace("erode=2.5", "erode=0")).expect("parse erode=0");
    r.check(Check::absolute(
        "`.fsm` normalizes `erode=0` to `None`",
        1.0,
        if doc_zero.fill.as_ref().unwrap().erode.is_none() {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    let mut bad_doc = reparsed.clone();
    bad_doc.fill.as_mut().unwrap().erode = Some(-1.5);
    r.check(Check::absolute(
        "`Document::validate()` rejects negative `erode < 0`",
        1.0,
        bad_doc.validate().len() as f64,
        "count",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 2. Analytical FillExpr Level-Set Boundary Evolution & CPU / GLSL Parity
    // -------------------------------------------------------------------------
    let body = Expr::Prim(Prim::Box {
        half: [30.0, 10.0, 10.0],
        round: 0.0,
    });
    // Protect a sphere around (-25, 0, 0) of radius 6 mm
    let keep = Expr::Xform {
        m: Rigid {
            r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            t: [-25.0, 0.0, 0.0],
        },
        e: Box::new(Expr::Prim(Prim::Sphere { radius: 6.0 })),
    };
    let fx = FillExpr {
        body: body.clone(),
        skin: 1.0,
        skin_max: 2.0,
        erode: 3.0,
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
        keep: keep.clone(),
    };

    r.check(Check::absolute(
        "`erode_at_param(p_lo)` at low-stress bound (`t = 0`)",
        3.0,
        fx.erode_at_param(0.4),
        "mm",
        1e-9,
    ));
    r.check(Check::absolute(
        "`erode_at_param(p_mid)` at midpoint (`t = 0.5`)",
        1.5,
        fx.erode_at_param(1.0),
        "mm",
        1e-9,
    ));
    r.check(Check::absolute(
        "`erode_at_param(p_hi)` at high-stress bound (`t = 1`)",
        0.0,
        fx.erode_at_param(1.6),
        "mm",
        1e-9,
    ));

    // At x = 0, y = 10, z = 0 (outside keep, on top face d_body = 0, t = 0.5 -> delta = 1.5 mm):
    // outer_body([0, 10, 0]) must equal +1.5 mm (eroded 1.5 mm inward, so y = 8.5 is the new surface)
    r.check(Check::absolute(
        "`outer_body([0, 10, 0])` shifts low-stress surface inward by `1.5 mm`",
        1.5,
        fx.outer_body([0.0, 10.0, 0.0]).v,
        "mm",
        1e-6,
    ));
    r.check(Check::absolute(
        "`outer_body([0, 8.5, 0])` zero-level-set lies `1.5 mm` inside original face",
        0.0,
        fx.outer_body([0.0, 8.5, 0.0]).v,
        "mm",
        1e-6,
    ));

    // Inside protected region `keep` at [-28, 0, 0] (where d_body = -2, d_keep = -3 < 0):
    let p_prot = [-28.0, 0.0, 0.0];
    let d_body_prot = expr::eval(&body, p_prot).v;
    let d_outer_prot = fx.outer_body(p_prot).v;
    r.check(Check::absolute(
        "protected region `keep` suppresses level-set erosion (`d_outer == d_body`)",
        d_body_prot,
        d_outer_prot,
        "mm",
        1e-9,
    ));

    // Verify GLSL codegen emits `fs_outer`, `fs_core`, and routes `fs_field` through `fs_outer`
    let vp_shader = glsl::generate_viewport(&body, Some(&fx));
    let full_shader = glsl::generate(&Expr::Filled(Box::new(fx.clone())));
    let has_glsl_erode = vp_shader.source.contains("vec4 fs_outer(vec3 p)")
        && vp_shader
            .source
            .contains("vec4 fs_field(vec3 p) { return fs_outer(p); }")
        && vp_shader.source.contains("vec4 d = fs_outer(p);")
        && vp_shader
            .source
            .matches("uniform highp sampler3D u_param;")
            .count()
            == 1
        && full_shader.source.contains("fs_min(vec4(d0.x +");
    r.check(Check::absolute(
        "GLSL emits `fs_outer`, single `u_param`, and `fs_field -> fs_outer`",
        1.0,
        if has_glsl_erode { 1.0 } else { 0.0 },
        "flag",
        0.0,
    ));

    // Verify exact parity between FillExpr::eval / removed_skin and reference formula across 128 points
    let mut max_eval_diff = 0.0_f64;
    let mut max_core_diff = 0.0_f64;
    for ix in 0..8 {
        for iy in 0..4 {
            for iz in 0..4 {
                let p = [
                    -26.0 + ix as f64 * 7.4,
                    -9.0 + iy as f64 * 6.0,
                    -9.0 + iz as f64 * 6.0,
                ];
                let d0 = expr::eval(&body, p);
                let k0 = expr::eval(&keep, p);
                let pv = fx.param.sample(p);
                let t = ((pv - 0.4) / 1.2).clamp(0.0, 1.0);
                let delta = 3.0 * (1.0 - t);
                let d_ref = dmin(D::new(d0.v + delta, d0.g), dmax(d0, k0));
                let sk = 1.0 + 1.0 * t;
                let core_ref = D::new(-(d_ref.v + sk), scale(d_ref.g, -1.0));
                let lat_ref = fx.eval_lattice(pv, p);
                let inner_ref = dmin(dmin(core_ref, lat_ref), k0);
                let eval_ref = dmax(d_ref, inner_ref);
                let rem_ref = dmax(D::new(d_ref.v + sk, d_ref.g), dmin(lat_ref, k0));

                let eval_got = fx.eval(p);
                let rem_got = fx.removed_skin(p);
                max_eval_diff = max_eval_diff.max((eval_got.v - eval_ref.v).abs());
                max_core_diff = max_core_diff.max((rem_got.v - rem_ref.v).abs());
            }
        }
    }
    r.check(Check::absolute(
        "128-point `FillExpr::eval` vs reference `fs_outer` parity",
        0.0,
        max_eval_diff,
        "mm",
        1e-12,
    ));
    r.check(Check::absolute(
        "128-point `FillExpr::removed_skin` vs reference `fs_core` parity",
        0.0,
        max_core_diff,
        "mm",
        1e-12,
    ));

    // -------------------------------------------------------------------------
    // 3. End-to-End Two-Pass FEA Comparison
    // -------------------------------------------------------------------------
    let mut doc_uneroded = reparsed.clone();
    doc_uneroded.fill.as_mut().unwrap().erode = None;

    let (_base_u, graded_uneroded) =
        graded(&doc_uneroded, Resolution::Size(3.5), clock).expect("graded erode=None");
    let (_base_e, graded_eroded) =
        graded(&reparsed, Resolution::Size(3.5), clock).expect("graded erode=2.5");

    let voids_uneroded = graded_uneroded
        .class
        .iter()
        .filter(|&&c| c == fieldsmith_core::fill::CLASS_VOID)
        .count();
    let voids_eroded = graded_eroded
        .class
        .iter()
        .filter(|&&c| c == fieldsmith_core::fill::CLASS_VOID)
        .count();

    r.check(Check::absolute(
        "level-set evolved mass `< 0.90 ×` un-eroded graded mass",
        1.0,
        if graded_eroded.mass_g < graded_uneroded.mass_g * 0.90 {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));
    r.check(Check::absolute(
        "level-set evolved void element count `>` un-eroded void element count",
        1.0,
        if voids_eroded > voids_uneroded {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));
    r.check(Check::absolute(
        "level-set evolved tip displacement finite and positive",
        1.0,
        if graded_eroded.cases[0].max_displacement > 0.01
            && graded_eroded.cases[0].max_displacement.is_finite()
        {
            1.0
        } else {
            0.0
        },
        "flag",
        0.0,
    ));

    r.section(
        "Summary & Governing Level-Set Evolution Law",
        format!(
            "Fieldsmith's `fill` statement supports stress- and compliance-driven outer boundary \
             level-set evolution via `erode=<mm>`. Given the 3D parameter field `param(p)` derived \
             from the Pass 1 FEA von Mises / strain-energy field and its normalized grade \
             `t(p) = clamp((param(p) - p_lo) / (p_hi - p_lo), 0, 1)`, the local inward boundary \
             shift is:\n\n\
             $$\\delta(\\mathbf{{p}}) = \\text{{erode}} \\cdot (1 - t(\\mathbf{{p}}))$$\n\
             $$d_{{\\text{{outer}}}}(\\mathbf{{p}}) = \\min\\!\\big(d_{{\\text{{body}}}}(\\mathbf{{p}}) + \\delta(\\mathbf{{p}}),\\; \\max(d_{{\\text{{body}}}}(\\mathbf{{p}}),\\, d_{{\\text{{keep}}}}(\\mathbf{{p}}))\\big)$$\n\n\
             On the 60×18×18 mm cantilever bracket (`400 N` tip shear, protected root & tip pads):\n\
             - **Solid envelope mass**: `{:.2} g`\n\
             - **Un-eroded multi-zone lattice (`erode = 0 mm`)**: mass = `{:.2} g` (`{:.1}%` of solid), `{}` void tets, max deflection = `{:.4} mm`, peak von Mises = `{:.1} MPa`\n\
             - **Level-set boundary-evolved (`erode = 2.5 mm`)**: mass = `{:.2} g` (`{:.1}%` of solid), `{}` void tets, max deflection = `{:.4} mm`, peak von Mises = `{:.1} MPa`",
            graded_eroded.solid_mass_g,
            graded_uneroded.mass_g,
            100.0 * graded_uneroded.mass_g / graded_uneroded.solid_mass_g,
            voids_uneroded,
            graded_uneroded.cases[0].max_displacement,
            graded_uneroded.cases[0].max_von_mises,
            graded_eroded.mass_g,
            100.0 * graded_eroded.mass_g / graded_eroded.solid_mass_g,
            voids_eroded,
            graded_eroded.cases[0].max_displacement,
            graded_eroded.cases[0].max_von_mises,
        ),
    );

    r.finish();
}
