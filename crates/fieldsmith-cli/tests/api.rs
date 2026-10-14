//! The one-shot `fieldsmith api` verb (GOAL v0.4 section C): one JSON request on
//! stdin, one JSON envelope on stdout, and the exit codes that make it a usable
//! contract — `ok` and `refusal` exit 0 (a refusal is an answer), a request or
//! processing error is an `error` envelope with exit 2, and argument misuse is the
//! plain CLI usage error (exit 1, no envelope). The API only ever reads the costs
//! store, so every run points at a fresh empty path: answers stay machine-independent.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_fieldsmith");
const BRACKET: &str = include_str!("../../../examples/bracket.fsm");

/// A fresh empty store path. The API reads it for seeding (a fresh path reads
/// nothing) and never writes it, so reusing one path across runs in a test is safe.
fn empty_store(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fs-api-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("store dir");
    dir.join("costs.jsonl")
}

/// Run the binary with `body` on stdin, return (envelope, stderr, exit code). The
/// envelope parsing IS a test of the contract: stdout must be one compact JSON line.
fn api(store: &Path, body: &str) -> (Value, String, Option<i32>, String) {
    let mut child = Command::new(BIN)
        .arg("api")
        .env("FIELDSMITH_COSTS_STORE", store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all(body.as_bytes())
        .expect("stdin write");
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("binary runs");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let v: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be one JSON envelope ({e}); got: {stdout:?}"));
    (v, stderr, out.status.code(), stdout)
}

fn req(question: Value) -> String {
    json!({"apiv": 1, "doc": BRACKET, "question": question}).to_string()
}

#[test]
fn api_ok_solves_and_exits_zero() {
    let store = empty_store("ok");
    let (v, err, code, _) = api(&store, &req(json!({"kind": "solve", "dofs": 30000})));
    assert_eq!(code, Some(0), "stderr: {err}");
    assert!(err.is_empty(), "clean run prints nothing on stderr: {err}");
    assert_eq!(v["status"], "ok");
    assert_eq!(v["apiv"], 1);
    assert!(v["engine"].as_str().unwrap().contains("fieldsmith"));
    assert_eq!(v["single"]["provenance"]["dofs"], 48822);
    assert_eq!(v["name"], "Bracket");
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn api_refusal_is_an_answer_and_exits_zero() {
    let store = empty_store("refusal");
    let (v, err, code, _) = api(
        &store,
        &req(json!({
            "kind": "ladder",
            "rungs": [30000, 150000],
            "tolerance": "deflection:0.00001mm"
        })),
    );
    assert_eq!(
        code,
        Some(0),
        "a refusal is an answer, not a failure; stderr: {err}"
    );
    assert_eq!(v["status"], "refusal");
    let text = v["outcome"]["refusal"].as_str().expect("refusal sentence");
    assert!(text.contains("the schedule ran to its cap (150000 DOF);"));
    assert!(
        text.contains("deflection:0.00001mm"),
        "quotes the tolerance: {text}"
    );
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn api_error_envelope_exits_two() {
    let store = empty_store("error");
    // Bad tolerance: the ladder's tolerance parse fails before any rung runs.
    let (v, err, code, stdout) = api(
        &store,
        &req(json!({
            "kind": "ladder",
            "rungs": [30000, 150000],
            "tolerance": "deflection:0.1"
        })),
    );
    assert_eq!(
        code,
        Some(2),
        "processing error maps to exit 2; stderr: {err}"
    );
    assert!(err.is_empty(), "the envelope carries everything: {err}");
    assert_eq!(stdout.lines().count(), 1, "envelope is one compact line");
    assert_eq!(v["status"], "error");
    let e = v["error"].as_str().unwrap();
    assert!(
        e.starts_with("bad tolerance "),
        "error names the field: {e}"
    );
    // Both fields on the wire (the request parsed; the document loaded).
    assert_eq!(v["name"], "Bracket");

    // Garbled stdin: a parse error envelope, same exit code.
    let (v, err, code, stdout) = api(&store, "not json at all");
    assert_eq!(code, Some(2), "garbled stdin: {err}");
    assert_eq!(v["status"], "error");
    assert!(
        v["error"].as_str().unwrap().starts_with("request:"),
        "parse errors name the stage: {}",
        v["error"]
    );
    assert_eq!(stdout.lines().count(), 1);

    // A version the engine does not speak: the error replies with the same apiv the
    // request declared, so a caller can tell which half was speaking.
    let (v, err, code, _) = api(&store, "{\"apiv\":99,\"question\":{\"kind\":\"ladder\"}}");
    assert_eq!(code, Some(2), "garbled stdin: {err}");
    assert_eq!(v["apiv"], 99, "the error echoes the request's apiv");
    assert_eq!(v["status"], "error");
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn api_takes_no_arguments_and_exits_one() {
    let store = empty_store("usage");
    let out = Command::new(BIN)
        .args(["api", "request.json"])
        .env("FIELDSMITH_COSTS_STORE", &store)
        .output()
        .expect("binary runs");
    assert_eq!(
        out.status.code(),
        Some(1),
        "argument misuse is the plain CLI usage error, not an envelope"
    );
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("api takes no arguments"), "{stderr}");
    assert!(
        String::from_utf8_lossy(&out.stdout).is_empty(),
        "no stdout for a usage error"
    );
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}

#[test]
fn api_two_runs_with_an_empty_store_agree() {
    let store = empty_store("det");
    let question = json!({"kind": "ladder", "rungs": [30000, 150000]});
    let first = api(&store, &req(question.clone()));
    let second = api(&store, &req(question));
    assert_eq!(first.2, Some(0), "first: {}", first.1);
    assert_eq!(second.2, Some(0), "second: {}", second.1);
    assert_eq!(first.0["status"], "ok");
    assert_eq!(second.0["status"], "ok");
    let (a, b) = (&first.0["outcome"], &second.0["outcome"]);
    assert_eq!(a["records"].as_array().unwrap().len(), 2);
    assert_eq!(b["records"].as_array().unwrap().len(), 2);
    // The engine's own numbers are the run-invariant part of the answer; only the
    // timing leaves move from run to run.
    assert_eq!(a["records"][0]["dofs"], b["records"][0]["dofs"]);
    assert_eq!(a["records"][1]["dofs"], b["records"][1]["dofs"]);
    assert_eq!(a["records"][1]["dofs"], 190440);
    let rec = serde_json::to_string(&a["records"][1]).unwrap();
    assert!(
        !rec.contains("bracket") && !rec.contains(".fsm"),
        "shape only: {rec}"
    );
    // An API run never writes the store — the path stays empty between the runs.
    assert!(
        !store.exists(),
        "the api reads the store and never writes it"
    );
    std::fs::remove_dir_all(store.parent().unwrap()).ok();
}
