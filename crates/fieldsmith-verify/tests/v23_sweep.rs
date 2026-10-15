//! V23 — the sweep question at apiv 2 (M16): five points on the Cantilever V1, one
//! envelope, one CSV.
//!
//! Four legs:
//! 1. Envelope shape: a `sweep` answer whose `values` echo the request in order and
//!    whose `points` are one per value; the top level carries no outcome — the verdict
//!    lives in each point, and the envelope stays `ok` even where a point would
//!    refuse. Each point runs the same declared-question runner the ladder uses: a
//!    two-rung ladder with its own records, cost fit and rung walls, no verdict when
//!    no tolerance is declared.
//! 2. Scaling: consecutive-pair deflection ratios against the beam-theory relation
//!    δ ∝ 1/h³. The band is pinned from the first measured run (2026-10-14, referee):
//!    measured max |error| 0.226 %, band 0.35 %. The FE solve is deterministic — the
//!    tracked literals are stable run to run (asserted for the first point below) —
//!    so the band re-asserts the same measured numbers on every run; only the walls
//!    move. vm follows no power law (its pair ratios sit between 1/h and 1/h³);
//!    recorded without a scaling claim.
//! 3. The CSV (`fieldsmith_api::sweep_csv`): one row per point, the last rung's
//!    numbers, cells equal to the typed records — and through sweep_csv's own
//!    wire-walk test, to the wire envelope's digits. `--csv` is the CLI route; the
//!    Results lens (slice 3) writes the same file from the same numbers.
//! 4. Not here: what the grammar does not name — a per-point tolerance (one
//!    `tolerance` applies to every point), params outside the address grammar
//!    (`<id>.<attr>[.<k>]`: region selectors, solve settings, profile vertices), and
//!    any warm-start sharing between points (each point re-meshes and bills its own
//!    walls).

use fieldsmith_api::{
    respond,
    schema::{Status, SweepJson},
    store,
    sweep_csv::{sweep_csv, CSV_HEADER},
};
use fieldsmith_verify::{Check, Report};
use serde_json::json;

const DOC: &str = "\
fieldsmith 1
name \"Cantilever V1\"
custommaterial steel200 name=\"Steel (E 200 GPa)\" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
";

/// The web's coarse/refined constants: the same schedule the ladder runs at apiv 2's
/// sweep question.
const RUNGS: [usize; 2] = [30_000, 150_000];

/// Pinned from the first measured run (2026-10-14, referee): the four consecutive-pair
/// deflection ratios sat at −0.168/−0.188/−0.206/−0.226 % against δ ∝ 1/h³; the band
/// gives the float-determinism headroom, not model-form slack.
const BAND: f64 = 0.0035;

/// The first point's tracked deflection, recorded at ship (referee, 2026-10-14). The
/// FE solve is deterministic, so this literal is stable run to run; the walls are not.
/// Cross-machine bit-stability is not asserted here — V18 measured it for the ladder's
/// change estimates, and the M16 close re-ran the same five points on the N100 (with a
/// declared tolerance) and held bit-identity on every tracked field; those rows are
/// pinned in data/costs/costs.csv. These literals stay referee-pinned.
const POINT0_DEFLECTION_MM: f64 = 0.39066861588109364;

fn sweep_shape(resp: &fieldsmith_api::schema::Response) -> &SweepJson {
    resp.sweep
        .as_ref()
        .expect("a sweep answer carries the sweep")
}

#[test]
fn v23_sweep_five_points_on_the_cantilever() {
    let req = json!({
        "apiv": 2,
        "doc": DOC,
        "question": {
            "kind": "sweep",
            "param": "beam.size.2",
            "values": [8.0, 9.0, 10.0, 11.0, 12.0],
            "rungs": RUNGS,
        },
    })
    .to_string();
    let reply = respond(&req);
    assert!(!reply.is_error, "the sweep answers ok: {}", reply.body);
    let resp = reply
        .response
        .as_ref()
        .expect("a parsed request answers with a typed envelope");
    assert!(matches!(resp.status, Status::Ok), "{}", reply.body);
    assert_eq!(resp.apiv, 2);
    assert_eq!(resp.engine, store::engine_identity());
    assert_eq!(resp.name.as_deref(), Some("Cantilever V1"));
    assert!(resp.error.is_none(), "{:?}", resp.error);
    assert!(resp.warnings.is_empty(), "{:?}", resp.warnings);
    // No sweep-level outcome: the top level is only the question echo and the points.
    assert!(resp.outcome.is_none() && resp.single.is_none());

    let sweep = sweep_shape(resp);

    // ---- leg 1: per-point ladders ----------------------------------------
    assert_eq!(sweep.points.len(), 5, "one point per value");
    struct Point {
        value: f64,
        dofs: usize,
        elements: usize,
        wall_ms: [f64; 2],
        deflection_mm: f64,
        vm_mpa: f64,
    }
    let mut points = Vec::new();
    for (i, p) in sweep.points.iter().enumerate() {
        assert_eq!(p.value, sweep.values[i], "points are in request order");
        let o = &p.outcome;
        assert_eq!(o.records.len(), 2, "point {} runs both rungs", p.value);
        for (j, rec) in o.records.iter().enumerate() {
            assert_eq!(rec.rung, j);
            assert_eq!(rec.target_dofs, RUNGS[j]);
            assert!(rec.dofs > 0 && rec.elements > 0);
            assert!(rec.wall_ms > 0.0, "each rung bills its own clock");
        }
        assert_eq!(o.field_leaves, 1);
        assert!(
            o.cost_fit.is_some(),
            "a two-rung ladder fits its own cost model"
        );
        assert!(
            o.met.is_none() && o.unmet.is_none() && o.refusal.is_none(),
            "no tolerance declared: point {} has no verdict",
            p.value
        );
        // total_ms is the sum of this point's own rung walls, nothing else billed.
        let sum = o.records[0].wall_ms + o.records[1].wall_ms;
        assert!(
            (o.total_ms - sum).abs() < 1e-9,
            "wall_ms {} vs total {}",
            sum,
            o.total_ms
        );
        let last = o.records.last().unwrap();
        points.push(Point {
            value: p.value,
            dofs: last.dofs,
            elements: last.elements,
            wall_ms: [o.records[0].wall_ms, o.records[1].wall_ms],
            deflection_mm: last.tracked.deflection_mm,
            vm_mpa: last.tracked.vm_mpa,
        });
    }
    // The determinism anchor: the first point's deflection is bit-stable run to run.
    assert_eq!(
        points[0].deflection_mm.to_bits(),
        POINT0_DEFLECTION_MM.to_bits(),
        "point 0 deflection {} moved off the ship literal",
        points[0].deflection_mm
    );

    let r = &mut Report::new(
        "V23",
        "The sweep question at apiv 2 — five points on the Cantilever V1",
    );

    r.section(
        "What was driven",
        format!(
            "One `sweep` question at apiv 2 against the V1 cantilever: param \
             `beam.size.2` over the values [8, 9, 10, 11, 12] mm, rungs {}, no \
             tolerance declared. One envelope comes back: status ok, values echoed in \
             order, five points, no top-level outcome — the verdict lives in each \
             point, and here is none because no tolerance was declared. Each point \
             re-meshes the document copy at its value and runs the two-rung ladder.",
            RUNGS.map(|x| x.to_string()).join("/")
        ),
    );

    let mut table = String::from(
        "| point (mm) | dofs | elements | wall r0 (ms) | wall r1 (ms) | deflection (mm) | vm (MPa) |\n\
         |---|---|---|---|---|---|---|\n",
    );
    for p in &points {
        table.push_str(&format!(
            "| {} | {} | {} | {:.3} | {:.3} | {} | {} |\n",
            p.value, p.dofs, p.elements, p.wall_ms[0], p.wall_ms[1], p.deflection_mm, p.vm_mpa,
        ));
    }
    r.section("Per-point ladders (last rung's numbers)", table);

    // ---- leg 2: deflection scaling ---------------------------------------
    let mut scaling = String::from(
        "Between consecutive points the thinning beam's deflection should follow \
         δ ∝ 1/h³. Measured ratios against the theory ratio (h_a/h_b)³:\n\n",
    );
    for (i, pair) in sweep.values.windows(2).enumerate() {
        let theory = (pair[0] / pair[1]).powi(3);
        let measured = points[i + 1].deflection_mm / points[i].deflection_mm;
        let err = (measured - theory) / theory;
        scaling.push_str(&format!(
            "- {:.0}→{:.0} mm: measured {:.6} vs theory {:.6} ({:+.3} %)\n",
            pair[0],
            pair[1],
            measured,
            theory,
            err * 100.0,
        ));
        r.check(Check::relative(
            &format!(
                "deflection ratio {:.0}→{:.0} mm against δ ∝ 1/h³",
                pair[0], pair[1]
            ),
            theory,
            measured,
            "ratio",
            BAND,
        ));
    }
    scaling.push_str(
        "\nThe band is pinned from the first measured run (2026-10-14, referee): max \
         |error| 0.226 %, band 0.35 %. The FE solve is deterministic — the tracked \
         literals are the same run to run (asserted for point 0 in the test) — so \
         these numbers re-assert themselves; only the walls move.",
    );
    r.section("Deflection scaling between consecutive points", scaling);

    // vm has no power law: its pair ratios sit between 1/h and 1/h³. Recorded, not
    // claimed.
    let mut vm = String::from(
        "The von Mises maximum across the same pairs follows no beam-theory power \
         law — the ratios sit between δ ∝ 1/h (the section modulus scales with h²) \
         and δ ∝ 1/h³. The number of sampled pairs here is four; no scaling law is \
         claimed, the ratios are recorded for the next user of the sweep:\n\n",
    );
    for (i, pair) in sweep.values.windows(2).enumerate() {
        let vm_ratio = points[i].vm_mpa / points[i + 1].vm_mpa;
        let lin = pair[1] / pair[0];
        let cub = (pair[1] / pair[0]).powi(3);
        vm.push_str(&format!(
            "- {:.0}→{:.0} mm: vm ratio {:.4}, between 1/h {:.4} and 1/h³ {:.4}\n",
            pair[0], pair[1], vm_ratio, lin, cub,
        ));
    }
    r.section("vm across the same pairs — no scaling claim", vm);

    // ---- leg 3: the CSV ---------------------------------------------------
    let csv = sweep_csv(resp).expect("a sweep answer builds its csv");
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], CSV_HEADER);
    assert_eq!(lines.len(), 6, "header plus one row per point");
    for (i, line) in lines.iter().skip(1).enumerate() {
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), CSV_HEADER.split(',').count(), "{line}");
        let p = &points[i];
        let last = sweep.points[i].outcome.records.last().unwrap();
        assert_eq!(cols[0], p.value.to_string());
        assert_eq!(cols[4], last.tracked.deflection_mm.to_string(), "{line}");
        assert_eq!(cols[5], last.tracked.vm_mpa.to_string(), "{line}");
        // Two rungs ran: both change columns present, verdict ok (no tolerance).
        assert!(cols[6].parse::<f64>().is_ok(), "{line}");
        assert!(cols[7].parse::<f64>().is_ok(), "{line}");
        assert_eq!(cols[8], "ok", "{line}");
    }
    r.section(
        "The CSV",
        format!(
            "`sweep_csv` writes one row per point — the last rung's numbers, both \
             change columns present at a two-rung point, status `ok` where no \
             tolerance is declared. The cells are the typed records' own digits, \
             which the wire envelope carries too (sweep_csv's wire-walk test holds \
             the cell-for-cell identity; serde_json's float parse is not correctly \
             rounded and the rows are built from the typed response for that reason).\n\n\
             ```csv\n{}```",
            csv.trim_end()
        ),
    );

    // ---- leg 4: not here ---------------------------------------------------
    r.section(
        "Not here",
        "- A per-point tolerance: one `tolerance` field applies to every point \
         (defaulting like the ladder's); there is no grammar to give point k a \
         different band than point k+1.\n\
         - Params outside the address grammar `<id>.<attr>[.<k>]`: the sweep addresses \
         a named numeric attribute on the parsed document — region selectors, solve \
         settings, and extruded-profile vertices are not names in that grammar, and \
         the sweep cannot add or remove nodes.\n\
         - Any warm-start sharing between points: each point clones the document, \
         re-meshes at its value, and bills its own rung walls; nothing is carried \
         across points.\n\
         - A per-point budget: the points share the declared schedule or tolerance; \
         an outcome gate evaluated per value is the lens's job, not the grammar's.",
    );

    r.finish();

    // The determinism note's "cross-machine" caveat is the one gap: this run is the
    // referee's. The N100 leg is slice 4's throughput row, not this case.
}
