//! Integration tests for the declared-question ladder (GOAL v0.4 section A): rungs
//! record as they run, the tolerance met / refusal / budget outcomes, the local store,
//! and the `costs` redaction verb. Each test drives the real binary through
//! `Command` — the exit codes (0 / 3 / 1) are part of the contract.
//!
//! Empirical notes from the calibration run (referee, bracket.fsm, rungs 30k→150k):
//! rung-to-rung change measures −28.1 % deflection / +12.0 % von Mises, walls ≈ 0.4 s +
//! 1.9 s. The met test's 50 % box and refusal's 1 mm box sit far from those numbers, and
//! the budget test's 0.3 s sits far below spent+estimate on any box, so the tests are
//! not sensitive to a machine being 2–3× faster or slower than the referee.

use std::path::PathBuf;
use std::process::Command;

const BRACKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/bracket.fsm");
const BRACKET_FILLED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/bracket-filled.fsm"
);

fn fs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fieldsmith"))
}

/// A fresh store path in its own directory. Unique per call: the store appends and
/// rotates, so a reused directory would leak rows from previous test runs into
/// seed medians and last-run blocks.
fn store_path(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("fieldsmith-ladder-test-{}-{}-{}", tag, std::process::id(), nanos));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("costs.jsonl")
}

fn run(cmd: &mut Command) -> (String, String, Option<i32>) {
    let out = cmd.output().expect("binary runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

fn ladder(store: &PathBuf) -> std::process::Command {
    let mut c = fs();
    c.arg("solve")
        .arg(BRACKET)
        .args(["--rungs", "30000,150000"])
        .env("FIELDSMITH_COSTS_STORE", store);
    c
}

fn read_rows(store: &PathBuf) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(store).expect("store file exists");
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("row parses"))
        .collect()
}

#[test]
fn ladder_met_stops_and_records_both_rungs() {
    let store = store_path("met");
    let (out, err, code) = run(ladder(&store).arg("--tolerance").arg("deflection:50%"));
    assert_eq!(code, Some(0), "met run exits 0; stderr: {err}");
    assert!(err.is_empty(), "no stderr on a clean run: {err}");
    // The declaration line and both rungs streamed, with the honesty clause.
    assert!(out.contains("ladder: rungs 30000 → 150000, tolerance deflection:50%"));
    assert!(out.contains("rung 0: 30000 DOF aimed"));
    assert!(out.contains("rung 1: 150000 DOF aimed"));
    assert!(out.contains("change vs rung 0:"));
    assert!(out.contains("estimates, not bounds"));
    assert!(out.contains("met: deflection"), "met line in: {out}");
    assert!(out.contains("cost fit: wall ∝ DOF^"));
    assert!(out.contains("residual unmeasured"), "two rungs: {out}");

    let rows = read_rows(&store);
    assert_eq!(rows.len(), 2, "one row per rung");
    assert_eq!(rows[0]["schema"], 1);
    assert_eq!(rows[0]["platform"], "cli");
    assert_eq!(rows[0]["rung"], 0);
    assert_eq!(rows[0]["target_dofs"], 30000);
    assert_eq!(rows[1]["rung"], 1);
    assert_eq!(rows[1]["target_dofs"], 150000);
    // The engine's own numbers: refined rung has more DOF and elements, both walls
    // positive, and the change estimate only exists from rung 1 on.
    let d0 = rows[0]["dofs"].as_u64().unwrap();
    let d1 = rows[1]["dofs"].as_u64().unwrap();
    assert!(d1 > d0);
    assert!(rows[0]["wall_ms"].as_f64().unwrap() > 0.0);
    assert!(rows[1]["wall_ms"].as_f64().unwrap() > rows[0]["wall_ms"].as_f64().unwrap());
    assert!(
        rows[0].get("deflection_change_rel").is_none(),
        "rung 0 has no baseline"
    );
    let ch = rows[1]["deflection_change_rel"].as_f64().unwrap();
    assert!(ch.abs() < 0.5, "change inside the declared box: {ch}");
    assert!(rows[1]["vm_change_rel"].as_f64().unwrap().abs() < 1.0);
    // Shape-only: no path, no name, no geometry anywhere in the row.
    let row_text = serde_json::to_string(&rows[0]).unwrap();
    assert!(!row_text.contains("bracket"));
    assert!(!row_text.contains(".fsm"));
    // Engine + shape fields carried for seeding and provenance.
    assert!(rows[0]["engine"].as_str().unwrap().contains("fieldsmith"));
    assert!(rows[0]["cpu"].as_str().unwrap().len() > 0);
    assert!(rows[0]["field_leaves"].as_u64().unwrap() > 0);
    assert_eq!(rows[0]["cores"].as_u64().unwrap() > 0, true);
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn ladder_refusal_exits_3_and_still_stores() {
    let store = store_path("refusal");
    let (out, err, code) = run(ladder(&store).arg("--tolerance").arg("deflection:0.001mm"));
    assert_eq!(code, Some(3), "unmet tolerance refuses with exit 3; err: {err}");
    assert!(out.contains("refusal:"), "refusal named in: {out}");
    assert!(out.contains("the schedule ran to its cap"));
    assert!(out.contains("deflection:0.001mm"));
    // A refusal is still a run: the measurements it took are stored.
    let rows = read_rows(&store);
    assert_eq!(rows.len(), 2);
    assert!(rows[1]["deflection_change_rel"].as_f64().is_some());
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn ladder_no_store_writes_nothing() {
    let store = store_path("nostore");
    let (out, err, code) = run(ladder(&store).arg("--tolerance").arg("deflection:50%").arg("--no-store"));
    assert_eq!(code, Some(0), "err: {err}");
    assert!(out.contains("met:"));
    // The path the run would have written to does not exist.
    assert!(!store.exists(), "--no-store must not write the store");
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn ladder_budget_stops_before_overflowing() {
    let store = store_path("budget");
    // First run fills the store with real rows (rungs 30000 and 150000).
    let (_, err1, code1) = run(ladder(&store).arg("--tolerance").arg("deflection:50%"));
    assert_eq!(code1, Some(0), "first run: {err1}");
    // Second run declares a budget the rung-0 wall alone cannot fit: rung 0 always
    // runs (a budget answer needs one measurement), then rung 1's projection — seeded
    // by the first run's stored wall for the same machine class and target DOF — does
    // not fit, so the ladder stops at one rung.
    let (out2, err2, code2) = run(ladder(&store).arg("--budget").arg("0.3s"));
    assert_eq!(code2, Some(0), "budget stop is not an error: {err2}");
    assert!(out2.contains("budget:"), "budget line in: {out2}");
    assert!(out2.contains("does not fit — stopping"));
    assert!(out2.contains("median of 1 local record"), "est from store: {out2}");
    assert!(out2.contains("rung 0: 30000 DOF aimed"));
    assert!(!out2.contains("rung 1:"), "stopped before rung 1: {out2}");
    assert!(out2.contains("tolerance-free run"), "no tolerance declared: {out2}");
    // Three rows total: the first run's two plus the second run's rung-0.
    let rows = read_rows(&store);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2]["rung"], 0);
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn costs_prints_last_run_as_csv() {
    let store = store_path("costs");
    // Hand-written rows of the store schema; costs only reads shape fields.
    let row = |rung: u64, target: u64, wall: f64, dch: Option<f64>| {
        let mut m = serde_json::json!({
            "schema": 1, "month": "2026-10", "platform": "cli",
            "engine": "fieldsmith 0.2.0 / fenics-rs 0.1.0",
            "cpu": "Test CPU", "cores": 8, "ram_gb": 16,
            "rung": rung, "target_dofs": target, "dofs": target * 4 + 8822,
            "elements": 9588 + rung * 31055, "field_leaves": 3, "wall_ms": wall,
        });
        if rung > 0 {
            m["deflection_change_rel"] = serde_json::json!(dch.unwrap_or(-0.281));
            m["vm_change_rel"] = serde_json::json!(0.12);
        }
        m
    };
    let body = format!(
        "{}\n{}\n",
        serde_json::to_string(&row(0, 30000, 392.0, None)).unwrap(),
        serde_json::to_string(&row(1, 150000, 1852.0, Some(-0.281))).unwrap(),
    );
    std::fs::write(&store, body).unwrap();

    let (out, err, code) = run(fs().arg("costs").env("FIELDSMITH_COSTS_STORE", &store));
    assert_eq!(code, Some(0), "err: {err}");
    assert!(out.contains("write-access: none"), "preview banner: {out}");
    assert!(
        out.contains("schema,month,platform,engine,cpu,cores,ram_gb,rung,target_dofs,dofs,elements,field_leaves,wall_ms,deflection_change_rel,vm_change_rel")
    );
    let data: Vec<_> = out
        .lines()
        .filter(|l| l.starts_with("1,2026-10"))
        .collect();
    assert_eq!(data.len(), 2, "both rows of the last run printed: {out}");
    assert_eq!(data[0].split(',').count(), 15, "no quoted fields here");
    assert!(data[1].ends_with("-0.281,0.12"), "change carried: {}", data[1]);
    assert!(out.contains("cost fit: wall ∝ DOF^"), "fit: {out}");
    assert!(out.contains("at a 5× refined rung"));
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn costs_empty_store_and_single_rung() {
    let store = store_path("costs-empty");
    let (out, err, code) = run(fs().arg("costs").env("FIELDSMITH_COSTS_STORE", &store));
    assert_eq!(code, Some(0), "empty store is not an error: {err}");
    assert!(out.contains("no local records"), "{out}");

    // A single-rung run gives no change and no projection, said plainly.
    let single = serde_json::json!({
        "schema": 1, "month": "2026-10", "platform": "cli",
        "engine": "fieldsmith 0.2.0 / fenics-rs 0.1.0",
        "cpu": "Test CPU", "cores": 8, "ram_gb": 16,
        "rung": 0, "target_dofs": 30000, "dofs": 48822, "elements": 9588,
        "field_leaves": 3, "wall_ms": 392.0,
    });
    std::fs::write(&store, serde_json::to_string(&single).unwrap() + "\n").unwrap();
    let (out, err, code) = run(fs().arg("costs").env("FIELDSMITH_COSTS_STORE", &store));
    assert_eq!(code, Some(0), "err: {err}");
    // One rung-0 row still prints as the last run, but honestly refuses a projection.
    assert!(out.starts_with('#'), "last-run banner: {out}");
    assert!(out.contains("field_leaves"), "csv header printed: {out}");
    assert!(out.contains("too few rows for a projection"), "{out}");
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn ladder_conflicts_refuse_by_name() {
    // --modes and the ladder are separate questions.
    let (_out, err, code) = run(
        fs().args(["solve", BRACKET, "--modes", "--tolerance", "deflection:50%"]),
    );
    assert_eq!(code, Some(1));
    assert!(err.contains("--modes and the ladder are separate questions"), "{err}");
    // The ladder runs the solid path: a fill document is refused alongside the flags.
    let (_out, err, code) = run(
        fs().args(["solve", BRACKET_FILLED, "--tolerance", "deflection:50%"]),
    );
    assert_eq!(code, Some(1));
    assert!(err.contains("the ladder runs the solid path"), "{err}");
    // --dofs/--h with the ladder flags.
    let (_out, err, code) = run(
        fs().args(["solve", BRACKET, "--dofs", "50000", "--tolerance", "deflection:50%"]),
    );
    assert_eq!(code, Some(1));
    assert!(err.contains("the ladder chooses its own resolutions"), "{err}");
    // Bad tolerance value.
    let (_out, err, code) = run(
        fs().args(["solve", BRACKET, "--tolerance", "deflection:0.1"]),
    );
    assert_eq!(code, Some(1));
    assert!(err.contains("bad tolerance"), "{err}");
    // A one-rung schedule has no baseline to compare against.
    let (_out, err, code) = run(
        fs().args(["solve", BRACKET, "--rungs", "30000", "--tolerance", "deflection:50%"]),
    );
    assert_eq!(code, Some(1));
    assert!(err.contains("bad --rungs value"), "{err}");
}