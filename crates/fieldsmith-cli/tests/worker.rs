//! The M15 slice-3 wire, end to end through the two binaries: a `fieldsmith session`
//! host and an enrolled `fieldsmith worker` exchange one real job over loopback TCP
//! and Noise. Pins the contract the slice-3 design states — outbound-only workers, a
//! token stored once at 0600 and never echoed, the worker's stdout naming sizes and
//! times but never geometry or fields, artifacts only under the stated directory, and
//! worker answers that solve identically to the local engine (same records, walls
//! apart).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_fieldsmith");
const BRACKET: &str = include_str!("../../../examples/bracket.fsm");
const BRACKET_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/bracket.fsm");

/// A scratch root, fresh per test (process id in the name).
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fs-worker-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The env shape children get: a private HOME (token files), a private TMPDIR, and a
/// fresh costs store. FIELDSMITH_TOKEN_DIR and the XDG defaults are removed so the
/// `$HOME/.config` token path and the store override are exactly what runs.
fn child_env<'a>(cmd: &'a mut Command, home: &'a Path, store: &'a Path) -> &'a mut Command {
    cmd.env("HOME", home)
        .env("TMPDIR", home)
        .env("FIELDSMITH_COSTS_STORE", store.join("costs.jsonl"))
        .env_remove("FIELDSMITH_TOKEN_DIR")
        .env_remove("XDG_STATE_HOME")
        .env_remove("XDG_CONFIG_HOME")
}

/// Everything captured from a spawned child with piped stdio: one reader thread per
/// stream, both lines mirrored into shared vecs (and the test log under --nocapture).
struct Captured {
    child: Child,
    out: Arc<Mutex<Vec<String>>>,
    err: Arc<Mutex<Vec<String>>>,
}

fn spawn_captured(cmd: &mut Command) -> Captured {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("child spawns");
    let out: Arc<Mutex<Vec<String>>> = Arc::default();
    let err: Arc<Mutex<Vec<String>>> = Arc::default();
    // One reader thread per piped stream; both mirrors end in shared vecs. Boxed so
    // stdout and stderr share a loop type.
    for (stream, sink, tag) in [
        (
            Box::new(child.stdout.take().expect("stdout piped")) as Box<dyn std::io::Read + Send>,
            out.clone(),
            "out",
        ),
        (
            Box::new(child.stderr.take().expect("stderr piped")) as Box<dyn std::io::Read + Send>,
            err.clone(),
            "err",
        ),
    ] {
        std::thread::spawn(move || {
            let r = std::io::BufReader::new(stream);
            for l in r.lines().map_while(Result::ok) {
                println!("[{tag}] {l}");
                sink.lock().expect("captured lock").push(l);
            }
        });
    }
    Captured { child, out, err }
}

fn lines_of(v: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    v.lock().expect("captured lock").clone()
}

/// Spawn the session host and wait (bounded) for it to print the token and the
/// address. Rust's stdout is line-buffered even into a pipe, so these lines arrive
/// while the host is blocked accepting.
fn spawn_session(dir: &Path, runs: &str) -> (Captured, String, String) {
    let mut cmd = Command::new(BIN);
    child_env(&mut cmd, dir, dir)
        .args(["session", BRACKET_PATH, "--rungs", runs, "--once"]);
    let c = spawn_captured(&mut cmd);
    let deadline = Instant::now() + Duration::from_secs(180);
    let (mut token, mut addr) = (None, None);
    while token.is_none() || addr.is_none() {
        let lines = lines_of(&c.out);
        token = lines
            .iter()
            .find(|l| l.starts_with("session token: "))
            .map(|l| l["session token: ".len()..].trim().to_string());
        addr = lines
            .iter()
            .find(|l| l.starts_with("session: serving on "))
            .map(|l| l["session: serving on ".len()..].trim().to_string());
        if token.is_none() || addr.is_none() {
            if Instant::now() > deadline {
                panic!("the session never printed its token/addr lines");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    (c, token.expect("token"), addr.expect("addr"))
}

/// The local engine's envelope for the same question — the session's answer records
/// must match it run for run (deterministic mesh + solve; walls are the only
/// difference).
fn local_envelope(home: &Path, question: Value) -> Value {
    let mut child = child_env(&mut Command::new(BIN), home, home)
        .arg("api")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("api binary spawns");
    let body = json!({"apiv": 1, "doc": BRACKET, "question": question}).to_string();
    child
        .stdin
        .as_mut()
        .expect("stdin piped")
        .write_all(body.as_bytes())
        .expect("stdin write");
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("api runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("local api envelope: {e}; got {stdout:?}"))
}

/// Records with wall times stripped — the comparable part across run places.
fn comparable(outcome: &Value) -> Vec<Value> {
    let mut rows = outcome["records"]
        .as_array()
        .expect("outcome has records")
        .iter()
        .map(|r| {
            let mut c = r.clone();
            c.as_object_mut().expect("record object").remove("wall_ms");
            c
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|r| r["rung"].as_u64().expect("rung index"));
    rows
}

#[test]
fn session_and_worker_exchange_one_real_job() {
    let dir = scratch("e2e");
    let worker_home = dir.join("worker-home");
    std::fs::create_dir_all(&worker_home).expect("worker home");

    let (mut session, token, addr) = spawn_session(&dir, "30000,150000");

    // Enrol: the token string the host printed, stored 0600 under the worker's HOME,
    // and never echoed back to the terminal that typed it.
    let enrol = Command::new(BIN)
        .args(["worker", "enrol", &token, &addr])
        .env("HOME", &worker_home)
        .env_remove("FIELDSMITH_TOKEN_DIR")
        .output()
        .expect("enrol runs");
    let enrol_out = String::from_utf8_lossy(&enrol.stdout).into_owned();
    assert!(
        enrol.status.success(),
        "enrol failed: {}",
        String::from_utf8_lossy(&enrol.stderr)
    );
    assert!(
        !enrol_out.contains(&token),
        "enrol must not echo the credential back: {enrol_out:?}"
    );
    let token_file = worker_home.join(".config/fieldsmith/worker-token.json");
    std::fs::metadata(&token_file).expect("token file stored");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(&token_file).expect("token file mode");
        assert_eq!(m.permissions().mode() & 0o777, 0o600, "token file is 0600");
    }

    // Serve: one job through the Noise channel; artifacts land only under --artifacts.
    let artifacts = dir.join("artifacts");
    let mut cmd = Command::new(BIN);
    child_env(&mut cmd, &worker_home, &dir)
        .args(["worker", "--once"])
        .arg("--artifacts")
        .arg(&artifacts);
    let mut worker = spawn_captured(&mut cmd);
    let status = worker.child.wait().expect("worker waits");
    let w_out = lines_of(&worker.out).join("\n");
    let w_err = lines_of(&worker.err).join("\n");
    assert!(status.success(), "worker exited {status}: {w_out}");
    assert!(w_err.is_empty(), "worker stderr on a clean run: {w_err}");
    for needle in [
        "worker: connected",
        "worker: job received (",
        "the answer is the API envelope (",
        "worker: answer sent",
    ] {
        assert!(
            w_out.contains(needle),
            "worker stdout lacks {needle:?}\nworker stdout: {w_out:?}"
        );
    }
    // The privacy line: stdout names sizes and times, never geometry or fields. The
    // document's own names — doc name, material, region and load ids — stay inside
    // the channel and the artifacts, off the printing surface.
    for forbidden in ["Bracket", "petg", "Load 1", "hole", "pull", "mount"] {
        assert!(
            !w_out.contains(forbidden),
            "worker stdout leaked document text {forbidden:?}: {w_out:?}"
        );
    }

    // The session finishes by itself (--once) and has printed the answer verbatim.
    let status = session.child.wait().expect("session waits");
    let s_out = lines_of(&session.out);
    assert!(status.success(), "session exited {status}: {s_out:?}");
    let envelope_line = s_out
        .iter()
        .find(|l| l.starts_with("{\"apiv\":"))
        .expect("session printed the answer envelope verbatim");
    let answer: Value = serde_json::from_str(envelope_line).expect("session answer parses");
    assert_eq!(answer["status"], "ok", "session answer: {envelope_line}");
    assert_eq!(answer["name"], "Bracket");

    // Same job locally: the record shapes and tracked values agree (the .fsmz and the
    // text path feed the same doc); walls are the only honest difference.
    let local = local_envelope(&dir, json!({"kind": "ladder", "rungs": [30000, 150000]}));
    assert_eq!(local["status"], "ok");
    assert_eq!(
        comparable(&answer["outcome"]),
        comparable(&local["outcome"]),
        "the worker's solve diverged from the local engine"
    );
    assert_eq!(
        answer["outcome"]["field_leaves"], local["outcome"]["field_leaves"],
        "field export leaves diverged"
    );

    // Artifacts: one response envelope written verbatim under the stated dir, plus
    // two worker cost rows in the store, shaped from the worker's own records.
    let artifacts = std::fs::read_dir(&artifacts).expect("artifacts dir");
    let n_json = artifacts
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .count();
    assert_eq!(n_json, 1, "one response envelope artifact");
    let rows = fieldsmith_api::store::read_all(&dir.join("costs.jsonl"));
    assert_eq!(rows.len(), 2, "one cost row per rung record");
    assert!(rows.iter().all(|r| r.platform == "worker"));
    assert_eq!(rows.iter().map(|r| r.rung).collect::<Vec<_>>(), vec![0, 1]);
}

#[test]
fn enrol_rejects_a_bad_token_without_storing_anything() {
    let dir = scratch("enrol-bad");
    let out = Command::new(BIN)
        .args(["worker", "enrol", "not-a-token", "127.0.0.1:12345"])
        .env("HOME", &dir)
        .env_remove("FIELDSMITH_TOKEN_DIR")
        .output()
        .expect("enrol runs");
    assert!(!out.status.success(), "a bad token must fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("token"),
        "the error names the token: {err:?}"
    );
    assert!(
        !dir.join(".config/fieldsmith/worker-token.json").exists(),
        "nothing is stored for a refused token"
    );
}

#[test]
fn worker_without_a_stored_token_names_the_enrol_step() {
    let dir = scratch("enrol-missing");
    let out = Command::new(BIN)
        .args(["worker", "--once"])
        .env("HOME", &dir)
        .env_remove("FIELDSMITH_TOKEN_DIR")
        .output()
        .expect("worker runs");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("enrol"),
        "the error points at enrol: {err:?}"
    );
}