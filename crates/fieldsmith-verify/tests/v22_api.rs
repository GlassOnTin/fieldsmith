//! V22 — the API + MCP contract (GOAL v0.4 section C): golden request/response,
//! headless, for the reference document at a pinned engine version.
//!
//! Legs:
//! 1. Eleven golden cases through `fieldsmith_api::respond` — the ladder's ok / met /
//!    refusal / budget answers, one solve, one modes run, four request-error
//!    envelopes (apiv 1 refusal and the apiv 99 mismatch among them, M16's version
//!    bump), and one sweep. Their canon forms are held in
//!    `tests/fixtures/v22-golden.jsonl` and byte-compared. One case additionally
//!    pins its EXACT wire bytes (field order).
//! 2. The canon transform is justified by measurement, not theory: two same-machine
//!    process runs of the same request were byte-identical apart from the timing
//!    leaves (probed during M15 slice 1: 17-digit element sizes, changes and tracked
//!    values reproduced exactly). The canon drops precisely those leaves and their
//!    direct products, quantizes every remaining float to 6 significant digits, and
//!    lets serde_json sort keys (BTreeMap, alphabetical).
//! 3. Version-bump policy: the engine identity travels in every envelope (`fieldsmith
//!    0.2.0 / fenics-rs 0.1.0` at pin time), so a version bump breaks the golden — the
//!    named, never silent, change. Re-bless with `FIELDSMITH_BLESS=1` and record the
//!    bump; the report below then names the old and new identity.
//! 4. The MCP column (same tool, same result, same document) is measured in
//!    `v22_mcp_same_result_column` against THESE golden lines: the tool-reachable cases
//!    re-run through `fieldsmith_mcp::Server::handle` and canon-byte-compare, the
//!    bad-tolerance route also reproduces the pinned wire bytes, an edited document
//!    solves to the same envelope through the tools and the one-shot, and the export
//!    tool's facts agree with the export core field-by-field. Report:
//!    `docs/verification/V22-MCP.md`.
//!
//! Named gaps (stated because this test cannot measure them):
//! - Cross-machine float bit-identity is NOT promised (GOAL section E): the 6-digit
//!   quantization bounds what a different machine's last-ulp differences can move.
//! - `refusal` / notes text leaves embed change floats at `{:+.1}` precision — a
//!   rounding-boundary flip across machines is near-zero but nonzero.
//! - The budget case's refusal sentence embeds timed stop text, so it is redacted from
//!   the canon; the cap refusal's sentence is time-free and pinned in full.
//! - serde limitation (schema.rs states it too): `deny_unknown_fields` cannot reach
//!   inside the internally tagged question enum — unknown keys INSIDE a question are
//!   ignored.

use fieldsmith_api::store;
use fieldsmith_verify::{Check, Report};
use serde_json::{json, Map, Value};

/// The reference document, as a request would carry it — a `doc` (text) request: the
/// bracket builds its mesh from a primitive, no external grid registry needed.
const DOC: &str = include_str!("../../../examples/bracket.fsm");

/// The rung-1 change the 2026-10-13 static-ladder CLI rows measured (V18's ship
/// constants, referee CPU, same document and targets): the API ladder must reproduce
/// the same engine numbers to well inside the tolerance.
const SHIP_D: f64 = -0.2807483011665701;
const SHIP_V: f64 = 0.12031574159861531;

/// Every leaf the determinism probe showed varies run to run, plus their direct
/// composites (`cost_fit` is all wall-derived; `stop_reason` prose embeds measured
/// times; the projection leaves are wall or store-derived) — dropped wherever they
/// appear, before the recursive walk.
const BASE_DROPS: &[&str] = &[
    "wall_ms",
    "total_ms",
    "mesh_ms",
    "solve_ms",
    "thermal_ms",
    "cost_fit",
    "stop_reason",
    "fit_ms",
    "seed_ms",
    "seed_n",
];

const BUDGET_DROPS: &[&str] = &["refusal"];

fn canon(value: &Value, drops: &[&str]) -> Value {
    match value {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                if drops.contains(&k.as_str()) {
                    continue;
                }
                out.insert(k.clone(), canon(v, drops));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|v| canon(v, drops)).collect()),
        // 6 significant digits: `{:.5e}` keeps 6, the parse round-trip lands on the
        // representable double the 6-digit form names, and serde_json's shortest
        // float formatting is deterministic. Integer leaves stay integers.
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => {
                let q: f64 = format!("{f:.5e}").parse().expect("quantized float parses");
                Value::Number(serde_json::Number::from_f64(q).expect("quantized float is finite"))
            }
            _ => value.clone(),
        },
        _ => value.clone(),
    }
}

struct Case {
    id: &'static str,
    request: Value,
    extra_drops: &'static [&'static str],
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            id: "ladder-ok",
            request: json!({"apiv": 2, "doc": DOC,
                "question": {"kind": "ladder", "rungs": [30000, 150000]}}),
            extra_drops: &[],
        },
        Case {
            id: "ladder-met",
            request: json!({"apiv": 2, "doc": DOC,
                "question": {"kind": "ladder", "rungs": [30000, 150000],
                             "tolerance": "deflection:100%"}}),
            extra_drops: &[],
        },
        Case {
            id: "ladder-refusal",
            request: json!({"apiv": 2, "doc": DOC,
                "question": {"kind": "ladder", "rungs": [30000, 150000],
                             "tolerance": "deflection:0.00001mm"}}),
            extra_drops: &[],
        },
        Case {
            id: "ladder-budget",
            request: json!({"apiv": 2, "doc": DOC,
                "question": {"kind": "ladder", "rungs": [30000, 150000, 750000],
                             "budget": "0s"}}),
            extra_drops: BUDGET_DROPS,
        },
        Case {
            id: "solve-30k",
            request: json!({"apiv": 2, "doc": DOC,
                "question": {"kind": "solve", "dofs": 30000}}),
            extra_drops: &[],
        },
        Case {
            id: "modes-30k",
            request: json!({"apiv": 2, "doc": DOC,
                "question": {"kind": "modes", "n": 3, "dofs": 30000}}),
            extra_drops: &[],
        },
        Case {
            id: "err-bad-tolerance",
            request: json!({"apiv": 2, "doc": DOC,
                "question": {"kind": "ladder", "rungs": [30000, 150000],
                             "tolerance": "deflection:0.1"}}),
            extra_drops: &[],
        },
        // apiv is checked before doc presence, so this carries no document on purpose:
        // the error names the version, not the document.
        Case {
            id: "err-apiv-99",
            request: json!({"apiv": 99, "question": {"kind": "ladder"}}),
            extra_drops: &[],
        },
        // The M16 version bump: a v1 request is refused, not translated, with a line
        // naming the change (schema.rs's bump rule, tested at the golden level too).
        Case {
            id: "err-apiv-1",
            request: json!({"apiv": 1, "question": {"kind": "ladder"}}),
            extra_drops: &[],
        },
        // The sweep (M16): the envelope answer carries one point per value, each point
        // its own ladder outcome. The bracket's hole-side regions only resolve at the
        // reference rungs (a 2000-DOF sweep point errors naming the region that lost
        // contact — the request errors name points), so this runs the same [30000,
        // 150000] the ladder legs run; the canon drops every wall-derived leaf
        // wherever it sits, points included.
        Case {
            id: "sweep-ok",
            request: json!({"apiv": 2, "doc": DOC,
                "question": {"kind": "sweep", "param": "base.size.2",
                             "values": [6.0, 8.0], "rungs": [30000, 150000]}}),
            extra_drops: &[],
        },
        Case {
            id: "err-both-docs",
            request: json!({"apiv": 2, "doc": DOC, "docz": "Zg==",
                "question": {"kind": "ladder", "rungs": [30000, 150000]}}),
            extra_drops: &[],
        },
    ]
}

fn fixture_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v22-golden.jsonl")
}

/// The contract each canon line must keep, independent of its bytes: statuses, record
/// counts, the ship-tie constants (V18's CLI_ROWS, same engine, same document), and the
/// error texts the envelope promises.
fn assert_semantics(id: &str, r: &Value) {
    let status = r["status"].as_str().unwrap_or("missing");
    let o = &r["outcome"];
    let single = &r["single"];
    match id {
        "ladder-ok" => {
            assert_eq!(status, "ok");
            let recs = o["records"].as_array().expect("records array");
            assert_eq!(recs.len(), 2, "the reference ladder buys both rungs");
            assert_eq!(recs[0]["dofs"], 48822, "coarse DOF ties to the ship record");
            assert_eq!(
                recs[1]["dofs"], 190440,
                "refined DOF ties to the ship record"
            );
            assert!(o["met"].is_null() && o["unmet"].is_null());
            assert!(o["projections"].as_array().unwrap().is_empty());
        }
        "ladder-met" => {
            assert_eq!(status, "ok");
            let m = &o["met"];
            assert_eq!(m["rung"], 1);
            assert_eq!(m["dofs"], 190440);
            assert_eq!(m["rungs_run"], 2);
            assert_eq!(
                m["tolerance"], "deflection:100%",
                "the met answer quotes the user's string back"
            );
            let ch = &m["change"];
            let drel = ch["deflection_rel"].as_f64().expect("deflection_rel");
            let vrel = ch["vm_rel"].as_f64().expect("vm_rel");
            assert!(
                (drel - SHIP_D).abs() < 1e-6,
                "rung-1 deflection change ties to the 2026-10-13 ship row: {drel}"
            );
            assert!(
                (vrel - SHIP_V).abs() < 1e-6,
                "rung-1 von Mises change ties to the ship row: {vrel}"
            );
            assert!(o["unmet"].is_null());
        }
        "ladder-refusal" => {
            assert_eq!(
                status, "refusal",
                "an unmet tolerance is an answer, not an error"
            );
            assert!(o["met"].is_null());
            let u = &o["unmet"];
            assert_eq!(u["cap_dofs"], 150000);
            let text = o["refusal"].as_str().expect("refusal sentence present");
            assert!(
                text.starts_with("the schedule ran to its cap (150000 DOF);"),
                "cap refusals are time-free and quoted whole: {text}"
            );
        }
        "ladder-budget" => {
            assert_eq!(
                status, "ok",
                "the budget cut is in stop_reason, not an error"
            );
            assert_eq!(o["records"].as_array().unwrap().len(), 2);
            let proj = o["projections"].as_array().unwrap();
            assert_eq!(proj.len(), 1, "one schedule entry was not bought");
            assert_eq!(proj[0]["target_dofs"], 750000);
            let notes = o["notes"].as_array().unwrap();
            assert_eq!(notes.len(), 1, "rung 1 had no fit and no seed");
            assert!(
                notes[0]
                    .as_str()
                    .unwrap()
                    .starts_with("budget: no estimate for rung 1"),
                "the note names why it bought the rung anyway"
            );
            assert!(o["met"].is_null() && o["unmet"].is_null());
        }
        "solve-30k" => {
            assert_eq!(status, "ok");
            assert_eq!(single["provenance"]["dofs"], 48822);
            assert!(!single["cases"].as_array().unwrap().is_empty());
            assert!(single["modes"].is_null());
        }
        "modes-30k" => {
            assert_eq!(status, "ok");
            let modes = &single["modes"];
            assert_eq!(modes["n_requested"], 3);
            assert_eq!(modes["frequencies_hz"].as_array().unwrap().len(), 3);
            assert!(
                single["cases"].as_array().unwrap().is_empty(),
                "modal runs solve no loads"
            );
            assert!(
                r["warnings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|w| w.as_str().unwrap().contains("loads ignored")),
                "the envelope carries the same loads note the CLI prints"
            );
        }
        "err-bad-tolerance" => {
            assert_eq!(status, "error");
            let e = r["error"].as_str().unwrap();
            assert!(
                e.starts_with("bad tolerance \"deflection:0.1\""),
                "the tolerance error quotes the user's string back: {e}"
            );
        }
        "err-apiv-99" => {
            assert_eq!(status, "error");
            assert_eq!(
                r["error"].as_str().unwrap(),
                "apiv: request declares 99, this engine speaks 2 — apiv 2 added the \
                 sweep question (v1 requests are not translated; see schema.rs)"
            );
            assert!(
                r["name"].is_null(),
                "the version check precedes any document read"
            );
        }
        "err-apiv-1" => {
            assert_eq!(status, "error");
            let e = r["error"].as_str().unwrap();
            assert_eq!(r["apiv"], 1, "the error echoes the request's version");
            assert!(
                e.contains("apiv 2 added the sweep question"),
                "the v1 refusal names the version's change: {e}"
            );
            assert!(
                e.contains("speaks 2"),
                "the error names the engine's version, not just the request's: {e}"
            );
        }
        "sweep-ok" => {
            assert_eq!(status, "ok");
            let s = &r["sweep"];
            assert_eq!(s["param"], "base.size.2");
            assert_eq!(s["values"].as_array().unwrap().len(), 2);
            let pts = s["points"].as_array().expect("one point per value");
            assert_eq!(pts.len(), 2);
            assert_eq!(pts[0]["value"], 6.0);
            assert_eq!(pts[1]["value"], 8.0);
            // Each point ran its own ladder — two rungs each — and no sweep-level
            // tolerance was declared, so the met/unmet keys stay absent everywhere.
            for p in pts {
                let o = &p["outcome"];
                let recs = o["records"].as_array().expect("point records");
                assert_eq!(recs.len(), 2);
                assert!(recs[1]["tracked"]["deflection_mm"].is_number());
                assert!(o["met"].is_null() && o["unmet"].is_null());
            }
            assert!(
                r["outcome"].is_null(),
                "the sweep question answers in sweep, not outcome"
            );
            // Thicker base is stiffer — the rungs measured it, not the grammar.
            let d = |i: usize| {
                pts[i]["outcome"]["records"][1]["tracked"]["deflection_mm"]
                    .as_f64()
                    .unwrap()
            };
            assert!(d(1) < d(0), "size.2 6→8: deflection {} → {}", d(0), d(1));
        }
        "err-both-docs" => {
            assert_eq!(status, "error");
            let e = r["error"].as_str().unwrap();
            assert!(
                e.contains("exactly one of doc (.fsm text) or docz"),
                "the two-document error names the rule: {e}"
            );
        }
        _ => panic!("unknown golden case {id}"),
    }
}

#[test]
fn v22_api_golden_contract() {
    // The golden must not depend on whose store seeds the estimators: point the read
    // only look at a fresh empty path (created, emptied, never appended to).
    let seed_path = std::env::temp_dir().join("fsm-v22-golden-seed.jsonl");
    let _ = std::fs::remove_file(&seed_path);
    std::env::set_var("FIELDSMITH_COSTS_STORE", &seed_path);
    let fixture = fixture_path();
    let bless = std::env::var("FIELDSMITH_BLESS")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false);

    // Run every case once, keep canon text plus the contract data the report quotes.
    struct Run {
        id: &'static str,
        canon: String,
        raw: String,
        status: &'static str,
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut met_drel: Option<f64> = None;
    let mut met_vrel: Option<f64> = None;
    for c in cases() {
        let body = c.request.to_string();
        let reply = fieldsmith_api::respond(&body);
        let r: Value = serde_json::from_str(&reply.body).expect("envelope parses");
        if c.id == "ladder-met" {
            let ch = &r["outcome"]["met"]["change"];
            met_drel = ch["deflection_rel"].as_f64();
            met_vrel = ch["vm_rel"].as_f64();
        }
        assert_semantics(c.id, &r);
        assert_eq!(
            r["engine"],
            store::engine_identity(),
            "every envelope carries the pinned engine identity"
        );
        assert_eq!(
            reply.is_error,
            r["status"] == "error",
            "is_error and status agree — the one-shot's exit-code contract"
        );
        let drops: Vec<&str> = BASE_DROPS
            .iter()
            .chain(c.extra_drops.iter())
            .copied()
            .collect();
        runs.push(Run {
            id: c.id,
            canon: serde_json::to_string(&canon(&r, &drops)).unwrap(),
            raw: reply.body,
            status: status_of(&r),
        });
    }

    // The wire line: one response pinned BYTE-for-BYTE (field order included), which
    // the sorted-key canon cannot show on its own. An error envelope cut at the
    // request-validation stage carries no timing leaves, so its bytes are fully
    // deterministic — the second respond() call must agree with the loop's, and the
    // equality holds below.
    let wire_id = "err-bad-tolerance-wire";
    let wire_raw = fieldsmith_api::respond(
        &cases()
            .iter()
            .find(|c| c.id == "err-bad-tolerance")
            .unwrap()
            .request
            .to_string(),
    )
    .body;
    let wire_golden = json!({"id": wire_id, "wire": wire_raw});

    // Checks the report can show regardless of mode.
    let mut lines_ok = 0usize;
    let wire_ok: usize;

    if bless {
        std::fs::create_dir_all(fixture.parent().unwrap()).expect("create fixtures dir");
        let mut text = String::new();
        for run in &runs {
            let line = serde_json::json!({"id": run.id, "canon": serde_json::from_str::<Value>(&run.canon).unwrap()});
            text.push_str(&line.to_string());
            text.push('\n');
        }
        text.push_str(&wire_golden.to_string());
        text.push('\n');
        std::fs::write(&fixture, text).expect("write golden fixture");
        lines_ok = runs.len();
        wire_ok = 1;
        println!(
            "blessed {} canon lines + 1 wire line into {} — re-run WITHOUT FIELDSMITH_BLESS to byte-compare",
            runs.len(),
            fixture.display()
        );
    } else {
        let text = std::fs::read_to_string(&fixture).unwrap_or_else(|e| {
            panic!(
                "read {}: {e} — bless first with FIELDSMITH_BLESS=1 cargo test -p fieldsmith-verify --test v22_api",
                fixture.display()
            )
        });
        let mut golden: std::collections::BTreeMap<String, (Option<Value>, Option<Value>)> =
            std::collections::BTreeMap::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line).expect("golden line parses");
            golden.insert(
                v["id"].as_str().expect("golden id").to_string(),
                (v.get("canon").cloned(), v.get("wire").cloned()),
            );
        }
        for run in &runs {
            let (want_canon, _) = golden
                .get(run.id)
                .unwrap_or_else(|| panic!("golden fixture lacks a canon line for {}", run.id));
            let want = serde_json::to_string(
                want_canon
                    .as_ref()
                    .expect("canon member present on golden line"),
            )
            .unwrap();
            assert_eq!(
                run.canon, want,
                "case {}: canon envelope drifted from the pinned golden — if the engine \
                 version moved, re-bless with FIELDSMITH_BLESS=1 and record the bump",
                run.id
            );
            lines_ok += 1;
        }
        let (_, want_wire) = golden
            .get(wire_id)
            .unwrap_or_else(|| panic!("golden fixture lacks the wire line"));
        let want = want_wire
            .as_ref()
            .expect("wire member present")
            .as_str()
            .expect("wire member is a string");
        assert_eq!(
            wire_golden["wire"].as_str().unwrap(),
            want,
            "err-bad-tolerance wire bytes drifted — field order is part of the contract"
        );
        wire_ok = 1;
    }

    // Wire bytes equal the err-bad-tolerance canon case's raw body (same question, so
    // the pinned line stays the honest witness of one response's exact bytes).
    assert_eq!(
        wire_raw,
        runs.iter()
            .find(|r| r.id == "err-bad-tolerance")
            .unwrap()
            .raw
    );

    let statuses: Vec<&str> = runs.iter().map(|r| r.status).collect();
    let mut rep = Report::new(
        "V22",
        "API + MCP contract — golden envelopes at the pinned engine version",
    );
    rep.section(
        "The nine golden cases",
        format!(
            "Through `fieldsmith_api::respond` on `examples/bracket.fsm`, with an empty \
             costs store (seeding is a read-only look; here it reads nothing):\n\n\
             | Case | Status | Contract the bytes pin |\n| :-- | :-- | :-- |\n\
             | ladder-ok | ok | 2 records, DOF 48822 / 190440 tie to the ship rungs (V18 rows) |\n\
             | ladder-met | ok | met at rung 1 (`100 %` tolerance met by the first change) |\n\
             | ladder-refusal | refusal | cap refusal's sentence pinned whole (time-free) |\n\
             | ladder-budget | ok | 2 rungs bought, note for rung 1, projection for 750000 |\n\
             | solve-30k | ok | provenance DOF 48822, one static case |\n\
             | modes-30k | ok | 3 frequencies, cases empty, the loads-ignored warning rides |\n\
             | err-bad-tolerance | error | the tolerance error quotes the user's string |\n\
             | err-apiv-99 | error | version check precedes the document check |\n\
             | err-both-docs | error | names the exactly-one-document rule |\n\n\
             Statuses measured this run: {statuses:?}. The canon lines byte-compare \
             against `crates/fieldsmith-verify/tests/fixtures/v22-golden.jsonl` ({lines_ok} \
             of 9 canon lines) plus one exact wire line ({wire_ok} of 1) that pins the raw \
             response bytes — field order included, which sorted-key canon cannot show. \
             `ok` and `refusal` are both non-error envelopes (the one-shot exits 0 on \
             both); `error` maps to exit 2.",
            statuses = statuses,
            lines_ok = lines_ok,
            wire_ok = wire_ok,
        ),
    );
    rep.section(
        "The canon transform and why it is lawful",
        "Two same-machine process runs of a request are byte-identical apart from the \
         timing leaves — probed during M15 slice 1 on this engine: 17-digit element \
         sizes (3.8076468739412803), changes (-0.2807483011665701) and tracked values \
         reproduced exactly across processes, while wall_ms and its derivates moved. \
         The canon therefore drops exactly `wall_ms total_ms mesh_ms solve_ms thermal_ms \
         cost_fit stop_reason fit_ms seed_ms seed_n` wherever they appear (`cost_fit` is \
         entirely wall-derived; `stop_reason` prose embeds measured times; the three \
         projection leaves are wall- or store-derived), and the budget case additionally \
         drops `refusal`, whose sentence embeds the timed stop text. Remaining floats \
         quantize to 6 significant digits, object keys serialize sorted (serde_json's \
         BTreeMap map). Consequences: wall-time regressions visible ONLY in the timing \
         leaves are out of the golden's sight (V17/V18 own walls); any change in the \
         engine's numbers, labels, structure or field VALUES bites.",
    );
    rep.section(
        "Version-bump policy",
        format!(
            "Every envelope carries the engine identity `{}` (`store::engine_identity`). A \
             version bump changes every golden line — the compare fails naming the \
             drift, and the fix is a recorded re-bless (`FIELDSMITH_BLESS=1 cargo test \
             -p fieldsmith-verify --test v22_api`), never a silent refresh. Re-blessed \
             2026-10-14 (M16) for the `apiv` move 1 → 2: apiv 2 adds the sweep question \
             and apiv 1 requests are refused, not translated — the version check replies \
             `apiv: request declares 1, this engine speaks 2 — apiv 2 added the sweep \
             question (v1 requests are not translated; see schema.rs)`. The move also \
             added two golden cases: `err-apiv-1` pins that refusal, `sweep-ok` pins the \
             sweep envelope (one point per value, each point the same ladder records as \
             a rung ladder, no sweep-level outcome).",
            store::engine_identity(),
        ),
    );
    rep.section(
        "MCP same-result — measured in V22-MCP",
        "The second column exists as its own report (`docs/verification/V22-MCP.md`, test \
         `v22_mcp_same_result_column`): the tool-reachable golden cases re-run through \
         `fieldsmith_mcp::Server::handle` canon-compare against THIS fixture; the \
         bad-tolerance route also reproduces the exact wire bytes pinned below; an \
         edited document (region, support and load added through the tools) solves to \
         the same envelope through the tool and the one-shot; the export tool's facts \
         agree with the export core field-by-field.",
    );
    rep.section(
        "Named gaps",
        "Cross-machine float bit-identity is NOT promised (GOAL section E): the 6-digit \
         quantization bounds what another machine's last-ulp differences can move. Text \
         leaves embedding change floats at `{:+.1}` precision carry a near-zero but \
         nonzero cross-machine rounding-boundary risk. serde cannot apply \
         `deny_unknown_fields` inside the internally tagged question enum — unknown keys \
         INSIDE a question silently pass (schema.rs states the same). The engine-version \
         drift in mesh DOF counts would break the golden together with the ship-tie \
         semantic asserts — one recording, then one re-bless.",
    );
    rep.check(Check::absolute(
        "Canon lines byte-equal against fixtures/v22-golden.jsonl",
        11.0,
        lines_ok as f64,
        "lines",
        0.0,
    ));
    rep.check(Check::absolute(
        "Wire line byte-equal (err-bad-tolerance raw body)",
        1.0,
        wire_ok as f64,
        "line",
        0.0,
    ));
    rep.check(Check::absolute(
        "Ladder-met rung-1 deflection change vs the 2026-10-13 ship row (referee, \
         same document and targets)",
        SHIP_D,
        met_drel.expect("ladder-met produced a change"),
        "rel",
        1e-6,
    ));
    rep.check(Check::absolute(
        "Ladder-met rung-1 von Mises change vs the 2026-10-13 ship row (referee)",
        SHIP_V,
        met_vrel.expect("ladder-met produced a vm change"),
        "rel",
        1e-6,
    ));
    rep.finish();
}

/// The MCP column: drive `fieldsmith_mcp::Server::handle` — the exact code path the
/// stdio binary runs, framing aside (framing is the server crate's 14 protocol tests) —
/// and hold the tools to the golden lines above: the tool-reachable golden cases
/// canon-byte-compare, the bad-tolerance route also reproduces the pinned wire bytes,
/// an edited document solves to the same envelope through the tool and the one-shot,
/// and the export tool's facts equal the export core's numbers field-by-field.
#[test]
fn v22_mcp_same_result_column() {
    use fieldsmith_api::{export as api_export, question};
    use fieldsmith_doc::fsm;
    use fieldsmith_mcp::Server;

    // Same store discipline as the golden test: a fresh, never-written path, so the
    // estimators read nothing (seeding is a read-only look).
    let seed_path = std::env::temp_dir().join("fsm-v22-mcp-seed.jsonl");
    let _ = std::fs::remove_file(&seed_path);
    std::env::set_var("FIELDSMITH_COSTS_STORE", &seed_path);

    // One tool call through the real handler: Ok carries the parsed payload, Err the
    // isError message text (which, for solve failures, IS the engine envelope).
    fn tool(srv: &mut Server, id: u32, name: &str, args: Value) -> Result<Value, String> {
        let line = serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": name, "arguments": args},
        })
        .to_string();
        let reply: Value =
            serde_json::from_str(&srv.handle(&line).expect("a tools/call always gets a reply"))
                .expect("reply line parses");
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .expect("result text")
            .to_string();
        if reply["result"]["isError"].as_bool().unwrap_or(false) {
            Err(text)
        } else {
            Ok(serde_json::from_str(&text).expect("tool payload parses"))
        }
    }

    // The golden lines this column compares against (the same fixture the golden test
    // blesses; the wire line lives under its own id, `err-bad-tolerance-wire`).
    let fixture = fixture_path();
    let text = std::fs::read_to_string(&fixture).unwrap_or_else(|e| {
        panic!(
            "read {}: {e} — bless first with FIELDSMITH_BLESS=1 cargo test -p fieldsmith-verify --test v22_api",
            fixture.display()
        )
    });
    let mut golden: std::collections::BTreeMap<String, (Option<Value>, Option<Value>)> =
        std::collections::BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).expect("golden line parses");
        golden.insert(
            v["id"].as_str().expect("golden id").to_string(),
            (v.get("canon").cloned(), v.get("wire").cloned()),
        );
    }

    // Leg A — the golden cases, re-run through the tool surface of one session.
    let mut srv = Server::new();
    let opened = tool(&mut srv, 1, "open", json!({"doc": DOC})).expect("open through the tool");
    assert_eq!(opened["name"], "Bracket");

    let mut ties = 0.0f64;
    let mut wire_ties = 0.0f64;
    for (i, c) in cases().iter().enumerate() {
        // err-apiv-99, err-apiv-1 and err-both-docs are request-shape errors the
        // server cannot emit: it builds every request with the constant APIV and
        // `open` enforces exactly one document form. Pinned on the API surface by
        // the golden; named in the report gap list here.
        if c.id == "err-apiv-99" || c.id == "err-apiv-1" || c.id == "err-both-docs" {
            continue;
        }
        let spec = c.request["question"].clone();
        let drops: Vec<&str> = BASE_DROPS
            .iter()
            .chain(c.extra_drops.iter())
            .copied()
            .collect();
        match tool(&mut srv, 100 + i as u32, "solve", spec) {
            Ok(env) => {
                assert!(
                    !c.id.starts_with("err-"),
                    "{} came back as a tool Ok — the isError contract moved",
                    c.id
                );
                assert_semantics(c.id, &env);
                ties += canon_tie(&golden, c.id, &env, &drops);
            }
            Err(text) => {
                let env: Value =
                    serde_json::from_str(&text).expect("the error message IS the envelope");
                assert_semantics(c.id, &env);
                ties += canon_tie(&golden, c.id, &env, &drops);
                // For the validation error, the tool message passes the body through
                // unchanged — the pinned wire bytes must reproduce exactly.
                let (wire_id, want) = ("err-bad-tolerance-wire", c.id == "err-bad-tolerance");
                if want {
                    let (_, wire) = golden.get(wire_id).expect("golden wire line");
                    let wire = wire
                        .as_ref()
                        .expect("wire member")
                        .as_str()
                        .expect("wire string");
                    assert_eq!(
                        text, wire,
                        "the MCP route must reproduce the pinned wire bytes for {}",
                        c.id
                    );
                    wire_ties += 1.0;
                }
            }
        }
    }

    // Leg B — the same edited document through both routes: the tool's `doc_text`
    // output feeds the one-shot request, so the two envelopes differ only in timings.
    let mut srv2 = Server::new();
    tool(&mut srv2, 200, "open", json!({"doc": DOC})).expect("open");
    let added = tool(
        &mut srv2,
        201,
        "add_region",
        json!({"selectors": [{"shape": "base", "face": "nz"}]}),
    )
    .expect("add_region");
    let rid = added["added"].as_str().unwrap().to_string();
    tool(
        &mut srv2,
        202,
        "add_support",
        json!({"region": rid.clone(), "fixed": [true, true, true]}),
    )
    .expect("add_support");
    tool(
        &mut srv2,
        203,
        "add_load",
        json!({"region": rid, "kind": {"type": "force", "vector": [0, 0, -100]}}),
    )
    .expect("add_load");
    let printed = tool(&mut srv2, 204, "doc_text", json!({})).expect("doc_text");
    let edited = printed["text"].as_str().expect("doc text").to_string();
    let spec = json!({"kind": "solve", "dofs": 30000});
    let via_tool =
        tool(&mut srv2, 205, "solve", spec.clone()).expect("edited solve through the tool");
    assert_eq!(via_tool["status"], "ok");
    assert_eq!(
        via_tool["single"]["provenance"]["dofs"], 48822,
        "supports and loads do not change the mesh — the provenance ties solve-30k"
    );
    let reply =
        fieldsmith_api::respond(&json!({"apiv": 2, "doc": edited, "question": spec}).to_string());
    assert!(
        !reply.is_error,
        "the one-shot accepts the tool's own doc_text output: {}",
        reply.body
    );
    let via_api: Value = serde_json::from_str(&reply.body).expect("envelope parses");
    assert_eq!(via_api["status"], "ok");
    let drops: Vec<&str> = BASE_DROPS.to_vec();
    assert_eq!(
        serde_json::to_string(&canon(&via_tool, &drops)).unwrap(),
        serde_json::to_string(&canon(&via_api, &drops)).unwrap(),
        "tool and one-shot answers over the same edited document must canon-tie"
    );
    let edited_ties = 1.0f64;

    // Leg C — the export tool's facts equal the export core's numbers field-by-field
    // (timing leaves excluded; exact same-process floats, so no quantization needed).
    let mut srv3 = Server::new();
    tool(&mut srv3, 300, "open", json!({"doc": DOC})).expect("open");
    let facts = tool(&mut srv3, 301, "export", json!({"h": 0.5})).expect("export through the tool");
    assert!(
        facts.get("stl_b64").is_none() && facts.get("mf_b64").is_none(),
        "no artifacts requested, no artifact members returned"
    );
    let doc = fsm::parse(DOC).expect("fixture parses");
    let opts = api_export::ExportOpts {
        h: 0.5,
        adaptive: false,
        solid_only: false,
        stl: false,
        mf: false,
    };
    let ex = api_export::export(&doc, &opts, question::clock).expect("the export core");
    assert_eq!(facts["name"], "Bracket");
    assert_eq!(facts["h"].as_f64().unwrap(), ex.h);
    assert_eq!(facts["adaptive"].as_bool().unwrap(), ex.adaptive);
    assert_eq!(facts["verts"].as_f64().unwrap(), ex.verts as f64);
    assert_eq!(facts["tris"].as_f64().unwrap(), ex.tris as f64);
    assert_eq!(facts["mesh_volume"].as_f64().unwrap(), ex.mesh_volume);
    assert_eq!(facts["field_volume"].as_f64().unwrap(), ex.reference.volume);
    assert_eq!(
        facts["field_volume_error"].as_f64().unwrap(),
        ex.reference.error
    );
    assert_eq!(facts["delta_percent"].as_f64().unwrap(), ex.delta_percent);
    for (k, v) in [
        ("open_edges", ex.defects.open_edges),
        ("non_manifold_edges", ex.defects.non_manifold_edges),
        ("orientation_defects", ex.defects.orientation_defects),
        ("degenerate_triangles", ex.defects.degenerate_triangles),
        ("self_intersections", ex.defects.self_intersections),
    ] {
        assert_eq!(
            facts["defects"][k].as_f64().unwrap(),
            v as f64,
            "defects.{k}"
        );
    }
    let export_ties = 1.0f64;

    let mut rep = Report::new(
        "V22-MCP",
        "MCP same-result column — the tools held to the V22 golden lines",
    );
    rep.section(
        "How the column is measured",
        "`fieldsmith_mcp::Server::handle` — the exact code path the stdio binary runs — \
         is driven in-process (framing is the server crate's protocol-suite business). \
         The same golden fixture `v22_api_golden_contract` blesses is the reference. \
         Store discipline is identical: a fresh, never-written costs path, so the \
         estimators read nothing.",
    );
    rep.section(
        "Measured ties",
        format!(
            "| Leg | Claim |\n| :-- | :-- |\n\
             | A — golden canon | {ties} of 7 tool-reachable cases re-run through the \
             tools (open once, then solve) canon-byte-compare against their golden \
             lines: ladder-ok, ladder-met, ladder-refusal, ladder-budget, solve-30k, \
             modes-30k, err-bad-tolerance |\n\
             | A — wire bytes | {wire_ties} of 1: the bad-tolerance tool message passes \
             the envelope body through unchanged and byte-equals the pinned wire line |\n\
             | B — edited document | {edited_ties} of 1: region + support + load added \
             through the tools; the tool's `doc_text` output feeds a one-shot request; \
             tool answer and one-shot answer canon-tie; provenance DOF pins 48822 |\n\
             | C — export | {export_ties} of 1: tool facts equal `fieldsmith_api::export` \
             field-by-field (h, verts, tris, volumes, delta_percent, five defect \
             counts), timing leaves excluded |\n\n\
             The 48822 provenance DOF is the same mesh solve-30k pins: extra supports \
             and loads do not change the mesh.",
            ties = ties,
            wire_ties = wire_ties,
            edited_ties = edited_ties,
            export_ties = export_ties,
        ),
    );
    rep.section(
        "Construction vs measurement",
        "By construction: the solve tool builds the same `{apiv, doc|docz, question}` \
         request and calls `fieldsmith_api::respond`; the export tool calls \
         `fieldsmith_api::export` with the parsed ExportOpts. Measured anyway, every \
         run: the canon and wire ties above, which would fail if either surface's \
         question parsing, document printing, or envelope handling diverged. The \
         doc_text output being accepted unchanged by the one-shot pins the print's \
         round-trip fidelity on the edited document.",
    );
    rep.section(
        "Named gaps",
        "err-apiv-99 and err-both-docs are request-shape errors the server cannot emit \
         (it writes the constant APIV into every request and `open` enforces exactly one \
         document form) — they are pinned on the API surface by the golden test and on \
         the tool surface by the server crate's protocol tests. Cross-machine float \
         caveats carry over from the golden test's canon reasoning. The two tests set \
         the costs-store env var concurrently (both to fresh empty paths); seeding is \
         read-only, so neither run can see the other's writes.",
    );
    rep.check(Check::absolute(
        "Golden cases canon-tie through the MCP tool surface",
        8.0,
        ties,
        "cases",
        0.0,
    ));
    rep.check(Check::absolute(
        "Wire bytes reproduce through the MCP route (err-bad-tolerance)",
        1.0,
        wire_ties,
        "line",
        0.0,
    ));
    rep.check(Check::absolute(
        "Edited-document tie (tool solve vs one-shot respond on the tool's doc_text)",
        1.0,
        edited_ties,
        "tie",
        0.0,
    ));
    rep.check(Check::absolute(
        "Export facts tie the tool to the export core (field-by-field)",
        1.0,
        export_ties,
        "tie",
        0.0,
    ));
    rep.finish();
}

/// Canon-compare one envelope against its golden line; returns 1.0 on a tie (the call
/// panics otherwise, so a return value is always 1.0).
fn canon_tie(
    golden: &std::collections::BTreeMap<String, (Option<Value>, Option<Value>)>,
    id: &str,
    env: &Value,
    drops: &[&str],
) -> f64 {
    let got = serde_json::to_string(&canon(env, drops)).unwrap();
    let (want, _) = golden
        .get(id)
        .unwrap_or_else(|| panic!("golden fixture lacks a canon line for {id}"));
    let want = serde_json::to_string(want.as_ref().expect("canon member present")).unwrap();
    assert_eq!(
        got, want,
        "case {id}: the MCP route's canon drifted from the pinned golden"
    );
    1.0
}

fn status_of(r: &Value) -> &'static str {
    match r["status"].as_str() {
        Some("ok") => "ok",
        Some("refusal") => "refusal",
        Some("error") => "error",
        _ => "missing",
    }
}
