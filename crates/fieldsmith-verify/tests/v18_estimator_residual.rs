//! V18 — the rung cost estimator against its own ship-time data.
//!
//! Four legs, all on measured rows (2026-10-13, `examples/bracket.fsm`,
//! rungs 30000→150000→750000 — the same rows `data/costs/costs.csv` holds):
//! 1. Fit behaviour: the two-point power-law fit is exact by construction, and the
//!    test proves it refuses a residual claim (`residual: None`). Its projection of
//!    the extension rung UNDERPREDICTS — by a third on the referee, 42 % on the N100 —
//!    recorded as ship bands. The three-point fit's exponent and residual sit in
//!    recorded bands.
//! 2. Cross-machine shape: the two small rungs run at one shared
//!    N100/referee wall factor (to 0.01 %); the extension rung's factor diverges +15 %.
//!    Mechanism not verified; the consequence (per-machine-class rows and bands) is.
//! 3. `data/costs/costs.csv` ties to these constants: every CLI wall and change value
//!    parses back bit-equal, wasm rows tie to the budgets-doc constants (probe-harness
//!    rows with empty field cells) and to the OnePlus 13 in-page walls (rows carrying
//!    achieved DOF/element cells). Twenty rows: the six 2026-10-13 static-ladder CLI
//!    rows, the four M14 thermal CLI rows (2026-10-14, `bracket-thermal.fsm` — the
//!    wall is the mesh+solve+thermal superset, change cells empty by measurement), the
//!    eight probe-harness wasm rows and the two OnePlus 13 wasm rows.
//! 4. The report carries the honest label: projections carry residuals, and the
//!    first-pair fit is on the cheap side of the curve.

use fieldsmith_core::rung::CostFit;
use fieldsmith_verify::{Check, Report};

/// DOF counts achieved on the bracket ladder (shared by both machines; from
/// `Provenance`, identical because both binaries are the referee build).
const DOFS: [f64; 3] = [48822.0, 190440.0, 876558.0];

/// Ship walls in ms, verbatim the CLI rows in `data/costs/costs.csv`.
const REF_WALLS: [f64; 3] = [250.913718, 1164.928266, 9792.657955];
const N100_WALLS: [f64; 3] = [569.213065, 2642.8623829999997, 25573.328251];

/// Recorded at ship (2026-10-13). The first-pair fit underpredicts the extension rung:
/// measured / projected = 1.502 (referee), 1.729 (N100).
const SHIP_PRED_RATIO_REF: (f64, f64) = (1.3, 1.7);
const SHIP_PRED_RATIO_N100: (f64, f64) = (1.5, 2.0);
/// Three-point fit measured on ship data: exponent 1.2714 / 1.3210, residual 13.6 % / 18.8 %.
const SHIP_EXPONENT_REF: (f64, f64) = (1.22, 1.32);
const SHIP_EXPONENT_N100: (f64, f64) = (1.27, 1.38);
const SHIP_RESIDUAL_REF: (f64, f64) = (0.10, 0.20);
const SHIP_RESIDUAL_N100: (f64, f64) = (0.14, 0.26);

/// Machine identity strings, as `data/costs/costs.csv` holds them.
const CPU_REF: &str = "AMD Ryzen 9 9950X 16-Core Processor";
const CPU_N100: &str = "Intel(R) N100";
const CPU_ONEPLUS: &str = "SM8750";

/// One CLI ladder row from the seed. `chd`/`chv` are the change-estimate columns
/// (None on rung 0).
struct ShipRow {
    cpu: &'static str,
    target: usize,
    rung: usize,
    wall: f64,
    chd: Option<f64>,
    chv: Option<f64>,
}

/// The ten CLI rows of `data/costs/costs.csv`, verbatim. The first six are the
/// 2026-10-13 static ladder; change values are the envelope-to-envelope estimates the
/// V17 replay proves are two-solve differences. The last four are the 2026-10-14 M14
/// thermal rows (`examples/bracket-thermal.fsm`): wall_ms is the mesh+solve+thermal
/// superset (the banner components sum to it), the change columns are empty because no
/// envelope deltas were measured that way, and rungs 0/1 of both machine classes share
/// (cpu, target, rung) with the static rows — the matcher ties by wall, as the wasm
/// matcher does.
const CLI_ROWS: [ShipRow; 10] = [
    ShipRow {
        cpu: CPU_REF,
        target: 30_000,
        rung: 0,
        wall: 250.913718,
        chd: None,
        chv: None,
    },
    ShipRow {
        cpu: CPU_REF,
        target: 150_000,
        rung: 1,
        wall: 1164.928266,
        chd: Some(-0.2807483011665701),
        chv: Some(0.12031574159861531),
    },
    ShipRow {
        cpu: CPU_REF,
        target: 750_000,
        rung: 2,
        wall: 9792.657955,
        chd: Some(-0.04126717503663446),
        chv: Some(0.0993936436864128),
    },
    ShipRow {
        cpu: CPU_N100,
        target: 30_000,
        rung: 0,
        wall: 569.213065,
        chd: None,
        chv: None,
    },
    ShipRow {
        cpu: CPU_N100,
        target: 150_000,
        rung: 1,
        wall: 2642.8623829999997,
        chd: Some(-0.2807483011665701),
        chv: Some(0.12031574159861531),
    },
    ShipRow {
        cpu: CPU_N100,
        target: 750_000,
        rung: 2,
        wall: 25573.328251,
        chd: Some(-0.041267175037406054),
        chv: Some(0.09939364368535455),
    },
    // 2026-10-14 M14 thermal block — wall_ms is the mesh+solve+thermal superset
    // (referee coarse 23+212+216, refined 41+1186+1245; N100 coarse 49+515+528,
    // refined 86+2547+2723). Change columns empty: no envelope deltas measured.
    ShipRow {
        cpu: CPU_REF,
        target: 30_000,
        rung: 0,
        wall: 451.0,
        chd: None,
        chv: None,
    },
    ShipRow {
        cpu: CPU_REF,
        target: 150_000,
        rung: 1,
        wall: 2472.0,
        chd: None,
        chv: None,
    },
    ShipRow {
        cpu: CPU_N100,
        target: 30_000,
        rung: 0,
        wall: 1092.0,
        chd: None,
        chv: None,
    },
    ShipRow {
        cpu: CPU_N100,
        target: 150_000,
        rung: 1,
        wall: 5356.0,
        chd: None,
        chv: None,
    },
];

/// The ten wasm rows of `data/costs/costs.csv`, verbatim: (cpu, target, rung, wall,
/// achieved dofs, achieved elements). The eight 2026-10-13 budgets-document rows come
/// from the probe harness (edit→level wall including document-UI overhead) and carry no
/// achieved cells; the two OnePlus 13 rows are walls read in-page from the running app's
/// analysis state (the worker's own per-level measurement, no document-UI overhead) and
/// carry the achieved DOF/element three runs agree on.
const WASM_ROWS: [(&str, usize, usize, f64, Option<usize>, Option<usize>); 10] = [
    (CPU_REF, 30_000, 0, 1352.0, None, None),
    (CPU_REF, 150_000, 1, 6025.0, None, None),
    (CPU_REF, 30_000, 0, 1678.0, None, None),
    (CPU_REF, 150_000, 1, 6494.0, None, None),
    (CPU_N100, 30_000, 0, 9679.0, None, None),
    (CPU_N100, 150_000, 1, 24967.0, None, None),
    (CPU_N100, 30_000, 0, 2963.0, None, None),
    (CPU_N100, 150_000, 1, 12670.0, None, None),
    (CPU_ONEPLUS, 30_000, 0, 1270.0, Some(48822), Some(9588)),
    (CPU_ONEPLUS, 150_000, 1, 7523.0, Some(190440), Some(40643)),
];

/// The CSV fixture holds no commas inside fields, so a naive split is exact here;
/// the production parse path is `crates/fieldsmith-cli/src/store.rs`.
fn parse_cost_csv(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split(',').map(str::to_string).collect())
        .collect()
}

#[test]
fn v18_estimator_residual() {
    let fit2_ref = CostFit::of(&[(DOFS[0], REF_WALLS[0]), (DOFS[1], REF_WALLS[1])])
        .expect("two distinct positive points");
    let fit2_n100 = CostFit::of(&[(DOFS[0], N100_WALLS[0]), (DOFS[1], N100_WALLS[1])])
        .expect("two distinct positive points");
    // Two points fit exactly; the house rule forbids claiming a residual there.
    assert!(
        fit2_ref.residual.is_none() && fit2_n100.residual.is_none(),
        "two-point fits refuse a residual claim"
    );

    // The first-pair projection of the extension rung: underprediction, banded.
    let pred_ref = fit2_ref.project(DOFS[2]);
    let pred_n100 = fit2_n100.project(DOFS[2]);
    let ratio_ref = REF_WALLS[2] / pred_ref;
    let ratio_n100 = N100_WALLS[2] / pred_n100;

    // The three-point fit carries its own error honestly.
    let fit3_ref = CostFit::of(&[
        (DOFS[0], REF_WALLS[0]),
        (DOFS[1], REF_WALLS[1]),
        (DOFS[2], REF_WALLS[2]),
    ])
    .expect("three distinct positive points");
    let fit3_n100 = CostFit::of(&[
        (DOFS[0], N100_WALLS[0]),
        (DOFS[1], N100_WALLS[1]),
        (DOFS[2], N100_WALLS[2]),
    ])
    .expect("three distinct positive points");
    let res_ref = fit3_ref
        .residual
        .expect("three points measure the residual");
    let res_n100 = fit3_n100
        .residual
        .expect("three points measure the residual");

    // Cross-machine wall factors per rung: small rungs share one factor, the extension
    // rung diverges.
    let f0 = N100_WALLS[0] / REF_WALLS[0];
    let f1 = N100_WALLS[1] / REF_WALLS[1];
    let f2 = N100_WALLS[2] / REF_WALLS[2];

    // Print everything before the asserts so a band failure shows its numbers.
    println!("V18 ship fit (DOF {:?}):", DOFS);
    println!(
        "  referee: 2pt exp {:.4}, rung-2 projection {:.1} vs {:.1} (ratio {:.3}); \
         3pt exp {:.4}, residual {:.1} %",
        fit2_ref.exponent,
        pred_ref,
        REF_WALLS[2],
        ratio_ref,
        fit3_ref.exponent,
        res_ref * 100.0,
    );
    println!(
        "  N100:    2pt exp {:.4}, rung-2 projection {:.1} vs {:.1} (ratio {:.3}); \
         3pt exp {:.4}, residual {:.1} %",
        fit2_n100.exponent,
        pred_n100,
        N100_WALLS[2],
        ratio_n100,
        fit3_n100.exponent,
        res_n100 * 100.0,
    );
    println!(
        "  cross-machine wall factors: rung0 {:.4}, rung1 {:.4}, rung2 {:.4}",
        f0, f1, f2
    );

    assert!(
        ratio_ref >= SHIP_PRED_RATIO_REF.0 && ratio_ref <= SHIP_PRED_RATIO_REF.1,
        "referee rung-2 ratio {:.3} outside ship band {:?} — re-measure and update \
         SHIP_PRED_RATIO_REF",
        ratio_ref,
        SHIP_PRED_RATIO_REF,
    );
    assert!(
        ratio_n100 >= SHIP_PRED_RATIO_N100.0 && ratio_n100 <= SHIP_PRED_RATIO_N100.1,
        "N100 rung-2 ratio {:.3} outside ship band {:?} — re-measure and update \
         SHIP_PRED_RATIO_N100",
        ratio_n100,
        SHIP_PRED_RATIO_N100,
    );
    assert!(
        fit3_ref.exponent >= SHIP_EXPONENT_REF.0 && fit3_ref.exponent <= SHIP_EXPONENT_REF.1,
        "referee exponent {:.4} outside ship band {:?} — re-measure and update \
         SHIP_EXPONENT_REF",
        fit3_ref.exponent,
        SHIP_EXPONENT_REF,
    );
    assert!(
        fit3_n100.exponent >= SHIP_EXPONENT_N100.0 && fit3_n100.exponent <= SHIP_EXPONENT_N100.1,
        "N100 exponent {:.4} outside ship band {:?} — re-measure and update \
         SHIP_EXPONENT_N100",
        fit3_n100.exponent,
        SHIP_EXPONENT_N100,
    );
    assert!(
        res_ref >= SHIP_RESIDUAL_REF.0 && res_ref <= SHIP_RESIDUAL_REF.1,
        "referee residual {:.3} outside ship band {:?} — re-measure and update \
         SHIP_RESIDUAL_REF",
        res_ref,
        SHIP_RESIDUAL_REF,
    );
    assert!(
        res_n100 >= SHIP_RESIDUAL_N100.0 && res_n100 <= SHIP_RESIDUAL_N100.1,
        "N100 residual {:.3} outside ship band {:?} — re-measure and update \
         SHIP_RESIDUAL_N100",
        res_n100,
        SHIP_RESIDUAL_N100,
    );
    // Small rungs share one cross-machine factor; the extension rung does not.
    assert!(
        (f1 / f0 - 1.0).abs() < 0.005,
        "small-rung wall factors disagree by {:.2} % (rung0 {:.4}, rung1 {:.4}) — \
         re-measure the cross-machine band",
        (f1 / f0 - 1.0).abs() * 100.0,
        f0,
        f1,
    );
    assert!(
        f2 / f0 - 1.0 > 0.10,
        "extension-rung factor {:.4} only +{:.1} % over the small-rung factor {:.4} — \
         re-measure the cross-machine band",
        f2,
        (f2 / f0 - 1.0) * 100.0,
        f0,
    );

    let mut r = Report::new(
        "V18",
        "Cost estimator residual — projections carry their own error",
    );
    r.check(Check::relative(
        "Rung-2 projection from the first-pair fit (referee)",
        REF_WALLS[2],
        pred_ref,
        "ms",
        0.5,
    ));
    r.check(Check::relative(
        "Rung-2 projection from the first-pair fit (N100)",
        N100_WALLS[2],
        pred_n100,
        "ms",
        0.5,
    ));
    r.section(
        "The ship rows and the recorded bands",
        format!(
            "The bracket ladder (`examples/bracket.fsm`, rungs 30000→150000→750000) measured \
             on two machine classes, 2026-10-13, walls in ms — the same rows \
             `data/costs/costs.csv` holds:\n\n\
             | Machine | Walls (30k / 150k / 750k DOF) | 2-pt exponent | 750k projection | 750k measured | measured/projection | 3-pt exponent | Residual |\n\
             | :-- | :-- | --: | --: | --: | --: | --: | --: |\n\
             | Referee (9950X) | {rw0} / {rw1} / {rw2} | {re2:.4} | {rp:.1} | {rm:.1} | {rr:.3} | {re3:.4} | {rres:.1} % |\n\
             | N100 | {nw0} / {nw1} / {nw2} | {ne2:.4} | {np:.1} | {nm:.1} | {nr:.3} | {ne3:.4} | {nres:.1} % |\n\n\
             Ship bands: projection ratio [{prl}, {prh}] (referee), [{pnl}, {pnh}] (N100); \
             exponent [{el}, {eh}] / [{enl}, {enh}]; residual [{rl}, {rh}] / [{rnl}, {rnh}]. \
             Each is re-asserted on re-run against these embedded constants; the band check \
             fails rather than silently re-baselines if the data changes shape.",
            rw0 = REF_WALLS[0], rw1 = REF_WALLS[1], rw2 = REF_WALLS[2],
            re2 = fit2_ref.exponent, re3 = fit3_ref.exponent,
            rp = pred_ref, rm = REF_WALLS[2], rr = ratio_ref,
            rres = res_ref * 100.0,
            nw0 = N100_WALLS[0], nw1 = N100_WALLS[1], nw2 = N100_WALLS[2],
            ne2 = fit2_n100.exponent, ne3 = fit3_n100.exponent,
            np = pred_n100, nm = N100_WALLS[2], nr = ratio_n100,
            nres = res_n100 * 100.0,
            prl = SHIP_PRED_RATIO_REF.0, prh = SHIP_PRED_RATIO_REF.1,
            pnl = SHIP_PRED_RATIO_N100.0, pnh = SHIP_PRED_RATIO_N100.1,
            el = SHIP_EXPONENT_REF.0, eh = SHIP_EXPONENT_REF.1,
            enl = SHIP_EXPONENT_N100.0, enh = SHIP_EXPONENT_N100.1,
            rl = SHIP_RESIDUAL_REF.0, rh = SHIP_RESIDUAL_REF.1,
            rnl = SHIP_RESIDUAL_N100.0, rnh = SHIP_RESIDUAL_N100.1,
        ),
    );
    r.section(
        "What the first-pair fit gets wrong",
        format!(
            "The two-point fit is exact over its own points and still wrong beyond them: it \
             projects the 750k-DOF rung at {rp0:.0} ms (referee) / {np0:.0} ms (N100) against \
             measured {rm:.0} / {nm:.0} — under by {rur:.0} % / {nun:.0} %. The first pair sits \
             on the cheap side of the curve; extrapolating it far is the estimator's least \
             reliable act. That is why (a) every projection travels with its residual, and a \
             two-point fit reports \"residual unmeasured\" rather than a fabricated 0 %; and \
             (b) when a rung has fewer than two local points, the CLI's budget/tolerance \
             projections fall back to `seed_median` — the median wall of same machine-class \
             rows — a level, never a claim about slope (`crates/fieldsmith-cli/src/main.rs`).",
            rp0 = pred_ref,
            np0 = pred_n100,
            rm = REF_WALLS[2],
            nm = N100_WALLS[2],
            rur = (1.0 - pred_ref / REF_WALLS[2]) * 100.0,
            nun = (1.0 - pred_n100 / N100_WALLS[2]) * 100.0,
        ),
    );
    r.section(
        "Cross-machine shape",
        format!(
            "Wall ratios N100/referee per rung: {f0:.4}, {f1:.4}, {f2:.4}. The two small \
             rungs share ONE slowdown factor (to {fd:.2} %) — a machine class is a coherent \
             thing for cost purposes at those sizes. The extension rung's factor is {fdp:.1} % \
             above that shared factor: the biggest rung does not scale with the class the way \
             the small ones do. The mechanism is not verified here (a memory-system size \
             effect is the plausible reading; nothing in this test measures it) — the recorded \
             consequence is: `data/costs/costs.csv` and every seeded projection key rows by \
             machine class, and cost bands are recorded per class, not assumed transferable.",
            f0 = f0,
            f1 = f1,
            f2 = f2,
            fd = (f1 / f0 - 1.0).abs() * 100.0,
            fdp = (f2 / f0 - 1.0) * 100.0,
        ),
    );
    r.section(
        "costs.csv ties to these numbers",
        format!(
            "`v18_costs_csv_ties_to_ship_data` parses `data/costs/costs.csv` and checks: \
             {nrows} rows, every row schema 1, month 2026-10; the ten CLI rows tie wall and \
             change-estimate cells EXACTLY to the constants this test embeds — the six \
             2026-10-13 static-ladder rows carry the change estimates, the four 2026-10-14 \
             M14 thermal rows carry the mesh+solve+thermal superset wall with empty change \
             columns (two machine classes' rungs 0/1 share keys with the static rows, so \
             the matcher ties by wall); the \
             eight wasm probe-harness rows tie to the budgets-document walls ({w0}, {w1}, \
             {w2}, {w3}, {w4}, {w5}, {w6}, {w7}) with empty DOF/element/leaf/change cells; \
             the two OnePlus 13 rows tie to the in-page walls ({w8}, {w9}) and carry achieved \
             DOF/element cells (the worker reads them from its own analysis state) with empty \
             leaf/change cells; engine \"fieldsmith wasm\". The CSV is the single seed \
             source; this test fails if the file and the ship data drift apart.",
            nrows = CLI_ROWS.len() + WASM_ROWS.len(),
            w0 = WASM_ROWS[0].3,
            w1 = WASM_ROWS[1].3,
            w2 = WASM_ROWS[2].3,
            w3 = WASM_ROWS[3].3,
            w4 = WASM_ROWS[4].3,
            w5 = WASM_ROWS[5].3,
            w6 = WASM_ROWS[6].3,
            w7 = WASM_ROWS[7].3,
            w8 = WASM_ROWS[8].3,
            w9 = WASM_ROWS[9].3,
        ),
    );
    r.write();
}

#[test]
fn v18_costs_csv_ties_to_ship_data() {
    let csv = include_str!("../../../data/costs/costs.csv");
    let rows = parse_cost_csv(csv);
    assert_eq!(
        rows.len(),
        CLI_ROWS.len() + WASM_ROWS.len(),
        "row count in data/costs/costs.csv must match its provenance blocks",
    );
    for row in &rows {
        assert_eq!(row[0], "1", "schema field");
        assert_eq!(row[1], "2026-10", "month field");
        match row[2].as_str() {
            "cli" => {
                let rung: usize = row[7].parse().expect("cli rung index");
                let target: usize = row[8].parse().expect("cli target_dofs");
                let wall: f64 = row[12].parse().expect("cli wall_ms");
                // The M14 thermal block shares (cpu, target, rung) with the static
                // ladder rows — its wall is the mesh+solve+thermal superset. Tie to
                // the ship row whose constant equals the row's wall, as the wasm
                // matcher ties two same-key blocks by wall.
                let ship = CLI_ROWS
                    .iter()
                    .find(|s| {
                        s.cpu == row[4] && s.target == target && s.rung == rung && s.wall == wall
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "no ship CLI row matches cpu={:?} rung={rung} target={target} \
                             wall={wall} — data/costs/costs.csv drifted from this test",
                            row[4]
                        )
                    });
                assert_eq!(row[3], "fieldsmith 0.2.0 / fenics-rs 0.1.0", "cli engine");
                assert_eq!(
                    row[9].parse::<usize>().expect("achieved dofs"),
                    DOFS[rung] as usize,
                    "achieved dofs per rung",
                );
                for (ci, want) in [(13usize, ship.chd), (14, ship.chv)] {
                    match want {
                        Some(w) => assert_eq!(
                            row[ci].parse::<f64>().expect("change cell parse"),
                            w,
                            "change cell cpu={:?} rung={rung} column {ci}",
                            row[4],
                        ),
                        None => assert!(
                            row[ci].is_empty(),
                            "row must carry an empty change cell (rung 0, or the M14 \
                             thermal rows) cpu={:?} column {ci}",
                            row[4],
                        ),
                    }
                }
                assert!(!row[10].is_empty(), "cli rows carry an element count");
                assert!(!row[11].is_empty(), "cli rows carry a field-leaf count");
            }
            "wasm" => {
                let rung: usize = row[7].parse().expect("wasm rung index");
                let target: usize = row[8].parse().expect("wasm target_dofs");
                let wall: f64 = row[12].parse().expect("wasm wall_ms");
                assert_eq!(row[3], "fieldsmith wasm", "wasm engine");
                let ships: Vec<_> = WASM_ROWS
                    .iter()
                    .filter(|&&(cpu, t, g, ..)| cpu == row[4] && t == target && g == rung)
                    .collect();
                // Browser and harness block live only in the CSV's provenance comments, so
                // two ship rows may share (cpu, target, rung); the row must tie wall and
                // achieved cells to the SAME constant, whichever of the tied block it is.
                let ship = ships
                    .iter()
                    .find(|&&(_, _, _, w, _, _)| *w == wall)
                    .unwrap_or_else(|| {
                        panic!(
                            "wasm wall for cpu={:?} rung={rung} must tie to a ship measurement \
                             (have {:?})",
                            row[4],
                            ships.iter().map(|s| s.3).collect::<Vec<_>>(),
                        )
                    });
                for ci in [11usize, 13, 14] {
                    assert!(
                        row[ci].is_empty(),
                        "wasm rows carry no field-leaf/change cells (column {ci})",
                    );
                }
                match (ship.4, ship.5) {
                    // Probe-harness rows: the harness records only the wall.
                    (None, None) => {
                        for ci in [9usize, 10] {
                            assert!(
                                row[ci].is_empty(),
                                "probe-harness wasm rows carry no DOF/element cells (column \
                                 {ci})",
                            );
                        }
                    }
                    // In-page rows: the worker reads its own achieved sizes.
                    (Some(d), Some(e)) => {
                        assert_eq!(
                            row[9].parse::<usize>().expect("wasm achieved dofs"),
                            d,
                            "in-page wasm rows tie achieved DOF to the ship measurement",
                        );
                        assert_eq!(
                            row[10].parse::<usize>().expect("wasm achieved elements"),
                            e,
                            "in-page wasm rows tie achieved elements to the ship measurement",
                        );
                    }
                    _ => unreachable!("ship wasm rows carry both achieved cells or neither"),
                }
            }
            other => panic!("unexpected platform {other:?} in the seed CSV"),
        }
    }
}
