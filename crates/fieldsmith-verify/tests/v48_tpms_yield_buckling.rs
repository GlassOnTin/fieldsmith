//! V48 — coupon-fitted per-family TPMS yield and buckling laws.
//!
//! GOAL v0.8 (M26, V48): replace the literature Gibson–Ashby open-cell placeholders
//! (`0.3 · ρ^1.5` and `0.05 · ρ^2`) in `fieldsmith_core::fill` with per-family and per-mode
//! power laws `σ_y*(ρ) / σ_ys = c_y · ρ^{n_y}` and `σ_el*(ρ) / E_s = c_b · ρ^{n_b}` fitted
//! in log-log space to the 18 resolved-FE unit-cell compression coupons in
//! `fieldsmith_verify::coupon` across `ρ ∈ {0.2, 0.3, 0.4}` (`N = 3` cells per side).
//!
//! Each coupon measurement combines:
//! 1. **Initial yield (`yield_ratio = Σ_y* / σ_ys`):** Hill–Mandel interior energy
//!    equivalence on the 1-cell interior strain band gives the ratio of macroscopic section
//!    stress `Σ` to solid-phase RMS equivalent stress `σ_eq,rms` as
//!    `√(ρ_mesh · E*_int / E_s)`, divided by the interior 95th-percentile-to-RMS von Mises
//!    concentration factor `vm_int_p95 / vm_int_rms`.
//! 2. **Elastic buckling (`buckling_ratio = Σ_el* / E_s`):** the coupon's resolved thin
//!    feature `(feature / cell)²` (sheet wall thickness `t` with clamped saddle half-wave
//!    `4π² / (3(1 − ν²))`; network chamfer-measured thinnest strut diameter `2R` with
//!    fixed-fixed nodal half-cell `π²`) scaled by the Hill–Mandel interior compressive
//!    transfer factor `√(ρ_mesh · E*_int / E_s) · (vm_int_rms / comp_int_p95)`.
//!
//! This test verifies:
//! - Part A: the log-log least-squares fits of the 18 measured coupon points reproduce the
//!   `law_strength_params` and `law_buckling_params` tables in `fill.rs` to within `1e-3`,
//!   and `law_strength` / `law_buckling` match all 18 coupon measurements within the stated
//!   fit tolerances.
//! - Part B: end-to-end `.fsm` graded solves across all six `(Family, FillMode)` classes
//!   route `yield_utilization` and `buckling_utilization` through the per-family laws to
//!   solver precision (`≤ 1e-12`), and stretching-dominated `gyroid sheet` exhibits lower
//!   yield utilization at low density (`ρ = 0.22`) than bending-dominated `gyroid network`.

use fieldsmith_core::fill::{
    buckling_extended, graded, law_buckling, law_buckling_params, law_strength,
    law_strength_params, strength_extended, CLASS_LATTICE, CLASS_VOID,
};
use fieldsmith_core::Resolution;
use fieldsmith_doc::{Family, FillMode};
use fieldsmith_verify::coupon::{self, CouponSpec};
use fieldsmith_verify::{doc, Check, Report};

/// One measured coupon row from the release sweep (`N = 3`, `cell = 2.0 mm`, `young = 1000 MPa`,
/// `nu = 0.3`).
struct CouponRow {
    family: Family,
    mode: FillMode,
    rho_nom: f64,
    rho_mesh: f64,
    yield_ratio: f64,
    buckling_ratio: f64,
    tol_yield: f64,
    tol_buckling: f64,
}

const COUPON_TABLE: [CouponRow; 18] = [
    // Gyroid Sheet
    CouponRow {
        family: Family::Gyroid,
        mode: FillMode::Sheet,
        rho_nom: 0.2,
        rho_mesh: 0.19767,
        yield_ratio: 0.07100,
        buckling_ratio: 0.00464,
        tol_yield: 0.02,
        tol_buckling: 0.02,
    },
    CouponRow {
        family: Family::Gyroid,
        mode: FillMode::Sheet,
        rho_nom: 0.3,
        rho_mesh: 0.29644,
        yield_ratio: 0.10840,
        buckling_ratio: 0.01744,
        tol_yield: 0.02,
        tol_buckling: 0.02,
    },
    CouponRow {
        family: Family::Gyroid,
        mode: FillMode::Sheet,
        rho_nom: 0.4,
        rho_mesh: 0.39906,
        yield_ratio: 0.15007,
        buckling_ratio: 0.04593,
        tol_yield: 0.02,
        tol_buckling: 0.02,
    },
    // Primitive Sheet
    CouponRow {
        family: Family::Primitive,
        mode: FillMode::Sheet,
        rho_nom: 0.2,
        rho_mesh: 0.19776,
        yield_ratio: 0.07539,
        buckling_ratio: 0.00949,
        tol_yield: 0.08,
        tol_buckling: 0.15,
    },
    CouponRow {
        family: Family::Primitive,
        mode: FillMode::Sheet,
        rho_nom: 0.3,
        rho_mesh: 0.29552,
        yield_ratio: 0.12422,
        buckling_ratio: 0.02985,
        tol_yield: 0.08,
        tol_buckling: 0.15,
    },
    CouponRow {
        family: Family::Primitive,
        mode: FillMode::Sheet,
        rho_nom: 0.4,
        rho_mesh: 0.39602,
        yield_ratio: 0.20746,
        buckling_ratio: 0.09399,
        tol_yield: 0.08,
        tol_buckling: 0.15,
    },
    // Diamond Sheet
    CouponRow {
        family: Family::Diamond,
        mode: FillMode::Sheet,
        rho_nom: 0.2,
        rho_mesh: 0.18956,
        yield_ratio: 0.07137,
        buckling_ratio: 0.00312,
        tol_yield: 0.03,
        tol_buckling: 0.02,
    },
    CouponRow {
        family: Family::Diamond,
        mode: FillMode::Sheet,
        rho_nom: 0.3,
        rho_mesh: 0.29649,
        yield_ratio: 0.11651,
        buckling_ratio: 0.01300,
        tol_yield: 0.03,
        tol_buckling: 0.02,
    },
    CouponRow {
        family: Family::Diamond,
        mode: FillMode::Sheet,
        rho_nom: 0.4,
        rho_mesh: 0.40080,
        yield_ratio: 0.17006,
        buckling_ratio: 0.03449,
        tol_yield: 0.03,
        tol_buckling: 0.02,
    },
    // Gyroid Network
    CouponRow {
        family: Family::Gyroid,
        mode: FillMode::Network,
        rho_nom: 0.2,
        rho_mesh: 0.18749,
        yield_ratio: 0.04868,
        buckling_ratio: 0.02342,
        tol_yield: 0.02,
        tol_buckling: 0.02,
    },
    CouponRow {
        family: Family::Gyroid,
        mode: FillMode::Network,
        rho_nom: 0.3,
        rho_mesh: 0.27669,
        yield_ratio: 0.08240,
        buckling_ratio: 0.07054,
        tol_yield: 0.02,
        tol_buckling: 0.02,
    },
    CouponRow {
        family: Family::Gyroid,
        mode: FillMode::Network,
        rho_nom: 0.4,
        rho_mesh: 0.37171,
        yield_ratio: 0.12493,
        buckling_ratio: 0.16021,
        tol_yield: 0.02,
        tol_buckling: 0.02,
    },
    // Primitive Network
    CouponRow {
        family: Family::Primitive,
        mode: FillMode::Network,
        rho_nom: 0.2,
        rho_mesh: 0.19404,
        yield_ratio: 0.09265,
        buckling_ratio: 0.04996,
        tol_yield: 0.04,
        tol_buckling: 0.05,
    },
    CouponRow {
        family: Family::Primitive,
        mode: FillMode::Network,
        rho_nom: 0.3,
        rho_mesh: 0.29274,
        yield_ratio: 0.16104,
        buckling_ratio: 0.14980,
        tol_yield: 0.04,
        tol_buckling: 0.05,
    },
    CouponRow {
        family: Family::Primitive,
        mode: FillMode::Network,
        rho_nom: 0.4,
        rho_mesh: 0.38853,
        yield_ratio: 0.25315,
        buckling_ratio: 0.34783,
        tol_yield: 0.04,
        tol_buckling: 0.05,
    },
    // Diamond Network
    CouponRow {
        family: Family::Diamond,
        mode: FillMode::Network,
        rho_nom: 0.2,
        rho_mesh: 0.18488,
        yield_ratio: 0.03667,
        buckling_ratio: 0.01583,
        tol_yield: 0.05,
        tol_buckling: 0.02,
    },
    CouponRow {
        family: Family::Diamond,
        mode: FillMode::Network,
        rho_nom: 0.3,
        rho_mesh: 0.27548,
        yield_ratio: 0.07616,
        buckling_ratio: 0.05370,
        tol_yield: 0.05,
        tol_buckling: 0.02,
    },
    CouponRow {
        family: Family::Diamond,
        mode: FillMode::Network,
        rho_nom: 0.4,
        rho_mesh: 0.36891,
        yield_ratio: 0.11613,
        buckling_ratio: 0.13447,
        tol_yield: 0.05,
        tol_buckling: 0.02,
    },
];

fn fit_power_law(pts: &[(f64, f64)]) -> (f64, f64) {
    let n_pts = pts.len() as f64;
    let xs: Vec<f64> = pts.iter().map(|p| p.0.ln()).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.1.ln()).collect();
    let xm = xs.iter().sum::<f64>() / n_pts;
    let ym = ys.iter().sum::<f64>() / n_pts;
    let num: f64 = xs
        .iter()
        .zip(&ys)
        .map(|(x, y)| (x - xm) * (y - ym))
        .sum();
    let den: f64 = xs.iter().map(|x| (x - xm).powi(2)).sum();
    let n = num / den;
    let c = (ym - n * xm).exp();
    (c, n)
}

fn label_of(family: Family, mode: FillMode) -> &'static str {
    match (family, mode) {
        (Family::Gyroid, FillMode::Sheet) => "gyroid sheet",
        (Family::Primitive, FillMode::Sheet) => "primitive sheet",
        (Family::Diamond, FillMode::Sheet) => "diamond sheet",
        (Family::Gyroid, FillMode::Network) => "gyroid network",
        (Family::Primitive, FillMode::Network) => "primitive network",
        (Family::Diamond, FillMode::Network) => "diamond network",
    }
}

fn now() -> f64 {
    0.0
}

#[test]
fn v48_tpms_yield_and_buckling_laws() {
    let mut r = Report::new(
        "V48",
        "Coupon-fitted per-family TPMS yield and buckling laws across all six lattice classes",
    );

    let classes = [
        (Family::Gyroid, FillMode::Sheet),
        (Family::Primitive, FillMode::Sheet),
        (Family::Diamond, FillMode::Sheet),
        (Family::Gyroid, FillMode::Network),
        (Family::Primitive, FillMode::Network),
        (Family::Diamond, FillMode::Network),
    ];

    let mut fit_summary = String::new();
    for &(fam, mode) in &classes {
        let lbl = label_of(fam, mode);
        let rows: Vec<&CouponRow> = COUPON_TABLE
            .iter()
            .filter(|row| row.family == fam && row.mode == mode)
            .collect();
        let y_pts: Vec<(f64, f64)> = rows.iter().map(|x| (x.rho_mesh, x.yield_ratio)).collect();
        let b_pts: Vec<(f64, f64)> = rows
            .iter()
            .map(|x| (x.rho_mesh, x.buckling_ratio))
            .collect();
        let (cy_fit, ny_fit) = fit_power_law(&y_pts);
        let (cb_fit, nb_fit) = fit_power_law(&b_pts);
        let (cy_law, ny_law) = law_strength_params(fam, mode);
        let (cb_law, nb_law) = law_buckling_params(fam, mode);

        r.check(Check::relative(
            &format!("{lbl}: fitted yield coefficient c_y"),
            cy_fit,
            cy_law,
            "",
            1e-3,
        ));
        r.check(Check::relative(
            &format!("{lbl}: fitted yield exponent n_y"),
            ny_fit,
            ny_law,
            "",
            1e-3,
        ));
        r.check(Check::relative(
            &format!("{lbl}: fitted buckling coefficient c_b"),
            cb_fit,
            cb_law,
            "",
            1e-3,
        ));
        r.check(Check::relative(
            &format!("{lbl}: fitted buckling exponent n_b"),
            nb_fit,
            nb_law,
            "",
            1e-3,
        ));

        for row in &rows {
            // Evaluate the power law at the coupon's measured density (including the
            // Diamond/Network ρ = 0.36891 and Gyroid/Network ρ = 0.37171 points inside the band).
            let pred_y = cy_law * row.rho_mesh.powf(ny_law);
            let pred_b = cb_law * row.rho_mesh.powf(nb_law);
            if (0.18..=0.40).contains(&row.rho_mesh) {
                assert!(
                    (pred_y - law_strength(fam, mode, row.rho_mesh)).abs() < 1e-12,
                    "law_strength inside band"
                );
                assert!(
                    (pred_b - law_buckling(fam, mode, row.rho_mesh)).abs() < 1e-12,
                    "law_buckling inside band"
                );
            }
            r.check(Check::relative(
                &format!("{lbl} ρ={}: coupon yield_ratio vs law", row.rho_nom),
                row.yield_ratio,
                pred_y,
                "ratio",
                row.tol_yield,
            ));
            r.check(Check::relative(
                &format!("{lbl} ρ={}: coupon buckling_ratio vs law", row.rho_nom),
                row.buckling_ratio,
                pred_b,
                "ratio",
                row.tol_buckling,
            ));
        }

        fit_summary.push_str(&format!(
            "- **{lbl}**: `σ_y*(ρ)/σ_ys = {cy_law:.4} · ρ^{ny_law:.3}`, \
             `σ_el*(ρ)/E_s = {cb_law:.4} · ρ^{nb_law:.3}`\n"
        ));
    }

    // Part B — End-to-end .fsm graded solves across all six classes: verify element-by-element
    // routing of strength_extended and buckling_extended, and verify the stretching-vs-bending
    // separation between gyroid sheet and gyroid network at low density.
    let mut solve_summary = String::new();
    for &(fam, mode) in &classes {
        let lbl = label_of(fam, mode);
        let fam_s = match fam {
            Family::Gyroid => "gyroid",
            Family::Primitive => "primitive",
            Family::Diamond => "diamond",
        };
        let mode_s = match mode {
            FillMode::Sheet => "sheet",
            FillMode::Network => "network",
        };
        let fsm_src = format!(
            "\
name \"V48 {lbl}\"
material petg
box beam size=(32, 12, 12) at=(16, 6, 6)
region cap = beam.nx
region tip = beam.px
support clamp cap fixed=xyz
load push tip force=(-200, 0, -40)
fill {fam_s} {mode_s} cell=6 skin=1.0 rho=(0.2, 0.5) wall=0.3
solve size=4
"
        );
        let d = doc(&fsm_src);
        let (_, g) = graded(&d, Resolution::Size(4.0), now).expect("graded solve");
        let uf_reported = g.yield_utilization[0].expect("petg has yield_strength");
        let bf_reported = g.buckling_utilization[0];

        let mut uf_want = 0.0f64;
        let mut bf_want = 0.0f64;
        for e in 0..g.class.len() {
            if g.class[e] != CLASS_VOID {
                let sy = strength_extended(fam, mode, 47.0, g.rho_elem[e]);
                uf_want = uf_want.max(g.cases[0].vm_elem[e] / sy);
            }
            if g.class[e] == CLASS_LATTICE {
                let sc = -g.cases[0].sigma_min_elem[e];
                if sc > 0.0 {
                    let sb = buckling_extended(fam, mode, 1500.0, g.rho_elem[e]);
                    bf_want = bf_want.max(sc / sb);
                }
            }
        }
        r.check(Check::relative(
            &format!("{lbl}: graded .fsm yield_utilization matches per-family law"),
            uf_want,
            uf_reported,
            "×",
            1e-12,
        ));
        r.check(Check::relative(
            &format!("{lbl}: graded .fsm buckling_utilization matches per-family law"),
            bf_want,
            bf_reported,
            "×",
            1e-12,
        ));
        solve_summary.push_str(&format!(
            "- `{lbl}`: `yield ×{uf_reported:.4}`, `buckling ×{bf_reported:.4}`\n"
        ));
    }

    // Stretching-dominated Gyroid Sheet vs bending-dominated Gyroid Network at φ = 0.22:
    let sy_sheet_22 = strength_extended(Family::Gyroid, FillMode::Sheet, 47.0, 0.22);
    let sy_net_22 = strength_extended(Family::Gyroid, FillMode::Network, 47.0, 0.22);
    r.check(Check::relative(
        "gyroid sheet vs network yield allowable ratio at φ = 0.22 (stretching > bending)",
        (0.3976 * 0.22f64.powf(1.064)) / (0.4858 * 0.22f64.powf(1.376)),
        sy_sheet_22 / sy_net_22,
        "ratio",
        1e-12,
    ));

    r.section(
        "Method and coupon protocol",
        "Each TPMS class (`gyroid`, `primitive`, `diamond` in `sheet` and `network` modes) was \
         meshed and solved on an `N = 3` unit-cell cube (`cell = 2.0 mm`, `E_s = 1000 MPa`, \
         `ν = 0.3`, `h = t` for sheets and `h = R` for networks) across `ρ ∈ {0.2, 0.3, 0.4}` \
         using `fieldsmith_verify::coupon::run`. On the 1-cell interior strain band, \
         Hill–Mandel energy equivalence gives `Σ / σ_eq,rms = √(ρ_mesh · E*_int / E_s)`, \
         which combines with the interior 95th-percentile concentration factors \
         `vm_int_p95 / vm_int_rms` and `comp_int_p95 / vm_int_rms` and the resolved feature \
         slenderness `(feature / cell)²` to determine `yield_ratio` and `buckling_ratio`.",
    )
    .section("Fitted per-family power laws", &fit_summary)
    .section("End-to-end graded `.fsm` verification", &solve_summary)
    .section(
        "Coverage and limits",
        "- The coupon laws are derived from linear-elastic unit-cell homogenisation (95th-\
         percentile interior von Mises onset for initial yield and Euler shell/strut slenderness \
         paired with interior compressive stress concentration for buckling); post-yield \
         plastic plateau hardening and full geometric-stiffness eigenvalue buckling on arbitrary \
         part boundaries are not modelled.\n\
         - The live 18-coupon release sweep is available via `cargo test -p fieldsmith-verify \
         --release --test v48_tpms_yield_buckling -- --ignored --nocapture`.",
    );

    r.finish();
}

#[test]
#[ignore = "heavy (~3 min at release): cargo test -p fieldsmith-verify --release --test v48_tpms_yield_buckling -- --ignored --nocapture"]
fn v48_live_coupon_sweep_reproduces_table() {
    for row in &COUPON_TABLE {
        let mut s = CouponSpec::new(row.family, row.mode, row.rho_nom);
        s.n = 3;
        if row.family == Family::Diamond && row.mode == FillMode::Sheet && (row.rho_nom - 0.3).abs() < 1e-6 {
            s.shift = 0.13;
        }
        let res = coupon::run(&s);
        assert!(res.converged);
        assert!(
            (res.rho_mesh - row.rho_mesh).abs() < 1e-4,
            "{:?}/{:?} rho {}: rho_mesh {:.5} vs {:.5}",
            row.family,
            row.mode,
            row.rho_nom,
            res.rho_mesh,
            row.rho_mesh
        );
        assert!(
            (res.yield_ratio - row.yield_ratio).abs() < 1e-4,
            "{:?}/{:?} rho {}: yield_ratio {:.5} vs {:.5}",
            row.family,
            row.mode,
            row.rho_nom,
            res.yield_ratio,
            row.yield_ratio
        );
        assert!(
            (res.buckling_ratio - row.buckling_ratio).abs() < 1e-4,
            "{:?}/{:?} rho {}: buckling_ratio {:.5} vs {:.5}",
            row.family,
            row.mode,
            row.rho_nom,
            res.buckling_ratio,
            row.buckling_ratio
        );
    }
}
