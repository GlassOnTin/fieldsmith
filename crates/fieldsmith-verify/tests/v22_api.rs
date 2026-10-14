//! V22 — the API + MCP contract (GOAL v0.4 section C): golden request/response,
//! headless, for the reference document at a pinned engine version.
//!
//! Legs:
//! 1. Nine golden cases through `fieldsmith_api::respond` — the ladder's ok / met /
//!    refusal / budget answers, one solve, one modes run, and three request-error
//!    envelopes. Their canon forms are held in `tests/fixtures/v22-golden.jsonl` and
//!    byte-compared. One case additionally pins its EXACT wire bytes (field order).
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
//! 4. The MCP column (same tool, same result, same document) is the second half of the
//!    V22 case: the stdio server is M15 slice 2 and its same-result assertion lands
//!    with it. Named as pending here so the gap is contract, not accident.
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
            request: json!({"apiv": 1, "doc": DOC,
                "question": {"kind": "ladder", "rungs": [30000, 150000]}}),
            extra_drops: &[],
        },
        Case {
            id: "ladder-met",
            request: json!({"apiv": 1, "doc": DOC,
                "question": {"kind": "ladder", "rungs": [30000, 150000],
                             "tolerance": "deflection:100%"}}),
            extra_drops: &[],
        },
        Case {
            id: "ladder-refusal",
            request: json!({"apiv": 1, "doc": DOC,
                "question": {"kind": "ladder", "rungs": [30000, 150000],
                             "tolerance": "deflection:0.00001mm"}}),
            extra_drops: &[],
        },
        Case {
            id: "ladder-budget",
            request: json!({"apiv": 1, "doc": DOC,
                "question": {"kind": "ladder", "rungs": [30000, 150000, 750000],
                             "budget": "0s"}}),
            extra_drops: BUDGET_DROPS,
        },
        Case {
            id: "solve-30k",
            request: json!({"apiv": 1, "doc": DOC,
                "question": {"kind": "solve", "dofs": 30000}}),
            extra_drops: &[],
        },
        Case {
            id: "modes-30k",
            request: json!({"apiv": 1, "doc": DOC,
                "question": {"kind": "modes", "n": 3, "dofs": 30000}}),
            extra_drops: &[],
        },
        Case {
            id: "err-bad-tolerance",
            request: json!({"apiv": 1, "doc": DOC,
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
        Case {
            id: "err-both-docs",
            request: json!({"apiv": 1, "doc": DOC, "docz": "Zg==",
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
                "apiv: request declares 99, this engine speaks 1"
            );
            assert!(
                r["name"].is_null(),
                "the version check precedes any document read"
            );
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
             -p fieldsmith-verify --test v22_api`), never a silent refresh.",
            store::engine_identity(),
        ),
    );
    rep.section(
        "MCP same-result — pending slice 2",
        "The V22 case's second column (the MCP tools return the same result for the same \
         document) is named OPEN: the stdio server is M15 slice 2. This report and the \
         golden envelope are its reference; when the server lands, its tools' answers \
         are compared against these canon lines and this section is replaced with the \
         measured claim.",
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
        9.0,
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

fn status_of(r: &Value) -> &'static str {
    match r["status"].as_str() {
        Some("ok") => "ok",
        Some("refusal") => "refusal",
        Some("error") => "error",
        _ => "missing",
    }
}
