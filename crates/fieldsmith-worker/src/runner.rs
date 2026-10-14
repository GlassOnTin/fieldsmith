//! The worker loop (design slice 3): connect outbound only, one job per connection.
//! Each connection is a fresh Noise session — a new ephemeral and worker static key —
//! so the wire never carries a re-used key. Jobs arrive as the API request (the
//! document enclosed in a `.fsmz` built at send time); answers leave as the API
//! response records. Artifacts are written only under the operator-stated directory,
//! and the stdout log says what it did — bytes in, envelope out, wall time — without
//! ever quoting geometry or fields (V21 pins that).
//!
//! Rung answers are also appended to the worker's own costs store (the shared
//! shape-only rows, `platform = "worker"`, machine identity probed on the worker), so
//! BYOC machines reach `data/costs/` the same way the CLI's own machine does: rows in
//! hand, PR for the user.

use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use fieldsmith_api::store::{self, Machine, Row};
use serde_json::Value;

use crate::chan::Chan;
use crate::token::Token;

/// Nothing is written outside this: the response envelope (verbatim) plus the
/// shape-only cost rows, both inside `artifacts`. The store path is the worker
/// machine's own default or `FIELDSMITH_COSTS_STORE` — the CLI's `--artifacts` has to
/// be an operator decision on that machine.
pub struct RunOpts {
    /// Serve one job, report, exit. Without it, the worker loops for the next job.
    pub once: bool,
    /// Where to write `artifacts/` envelope + cost rows; `None` = no artifacts, no
    /// rows, stdout only.
    pub artifacts: Option<PathBuf>,
    /// Sleep between connect failures.
    pub reconnect_ms: u64,
    /// Give up after this many connect attempts (`None` = keep trying; `Some(n) ==
    /// Some(1)` = no retry).
    pub max_attempts: Option<u32>,
}

/// Drive the worker: connect, handshake as initiator against the token, take one job,
/// run `job`, persist artifacts if asked, answer, repeat (or exit on `once`).
pub fn run(
    token: &Token,
    host: &str,
    port: u16,
    opts: &RunOpts,
    log: &mut dyn FnMut(&str),
    job: &mut dyn FnMut(&str) -> String,
) -> Result<(), String> {
    let (own_priv, _) = crate::host::worker_static_keypair();
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        log(&format!(
            "worker: connecting to {host}:{port} (attempt {attempt})"
        ));
        match once(token, &own_priv, host, port, opts, log, job) {
            Ok(()) if opts.once => return Ok(()),
            Ok(()) => {
                attempt = 0;
            }
            Err(e) => {
                if let Some(max) = opts.max_attempts {
                    if attempt >= max {
                        return Err(format!("worker: giving up after {attempt} attempt(s): {e}"));
                    }
                }
                log(&format!("worker: attempt failed: {e}"));
                std::thread::sleep(std::time::Duration::from_millis(opts.reconnect_ms));
            }
        }
    }
}

/// One connection: handshake, one job, answer. Connection-level errors are returned
/// (the loop decides retry vs fatal); a completed job means the answer was sent, so
/// this returns `Ok` even if the host then drops us.
fn once(
    token: &Token,
    own_priv: &[u8; 32],
    host: &str,
    port: u16,
    opts: &RunOpts,
    log: &mut dyn FnMut(&str),
    job: &mut dyn FnMut(&str) -> String,
) -> Result<(), String> {
    let stream = TcpStream::connect((host, port)).map_err(|e| format!("connect: {e}"))?;
    let mut chan = Chan::initiator(stream, own_priv, &token.host_static, &token.psk)
        .map_err(|e| format!("handshake: {e}"))?;
    log(&format!("worker: connected to {host}:{port}"));
    let request = chan
        .recv()
        .map_err(|e| format!("receiving the job failed: {e}"))?;
    log(&format!("worker: job received ({} bytes)", request.len()));
    let request = String::from_utf8_lossy(&request).into_owned();
    let started = Instant::now();
    let response = job(&request);
    let job_ms = started.elapsed().as_secs_f64() * 1000.0;
    log(&format!(
        "worker: job done in {job_ms:.0} ms — the answer is the API envelope ({} bytes)",
        response.len()
    ));
    if let Some(dir) = &opts.artifacts {
        persist(dir, &response, log);
    }
    chan.send(response.as_bytes())
        .map_err(|e| format!("sending the answer failed: {e}"))?;
    log("worker: answer sent");
    Ok(())
}

/// Write the response envelope verbatim under `dir`, then append the shape-only
/// rung rows to the worker machine's costs store. Geometry and field values stay out
/// of both — the envelope is the records the ladder reports (walls, DOFs, rel-changes),
/// the store rows are the redaction-preview shape. Public for the in-crate test that
/// pins the artifact + store behaviour.
pub fn persist(dir: &std::path::Path, response: &str, log: &mut dyn FnMut(&str)) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log(&format!(
            "worker: artifacts dir {dir:?} could not be created: {e}"
        ));
        return;
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("{millis}-response.json"));
    if let Err(e) = std::fs::write(&path, response) {
        log(&format!("worker: artifact {path:?} write failed: {e}"));
        return;
    }
    log(&format!("worker: artifact {}", path.display()));

    let Ok(envelope) = serde_json::from_str::<Value>(response) else {
        log("worker: the response did not parse as JSON — no cost rows recorded");
        return;
    };
    let Some(outcome) = envelope.get("outcome") else {
        return; // Not a ladder run (or a refusal without records) — nothing to record.
    };
    let Some(records) = outcome.get("records").and_then(Value::as_array) else {
        return;
    };
    if records.is_empty() {
        return;
    }
    let field_leaves = outcome
        .get("field_leaves")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let machine = Machine::probe();
    let rows: Vec<Row> = records
        .iter()
        .filter_map(|r| row_of(r, field_leaves, &machine))
        .collect();
    if rows.len() != records.len() {
        log(&format!(
            "worker: {} of {} rung records were unreadable — only the readable rows recorded",
            records.len() - rows.len(),
            records.len()
        ));
    }
    match store::store_path() {
        Some(path) => match store::append(&path, &rows) {
            Ok(()) => log(&format!(
                "worker: {} cost row(s) appended to {}",
                rows.len(),
                path.display()
            )),
            Err(e) => log(&format!("worker: cost rows not stored: {e}")),
        },
        None => log(&format!(
            "worker: {} cost row(s) computed but no store location (no HOME) — skipped",
            rows.len()
        )),
    }
}

/// Hand-map one envelope rung record into a store row: the worker's own machine
/// identity, `platform = "worker"`, `field_leaves` from the envelope. Unreadable
/// fields drop the row (`filter_map`) rather than fabricate values.
fn row_of(r: &Value, field_leaves: usize, machine: &Machine) -> Option<Row> {
    let num = |k: &str| r.get(k).and_then(Value::as_u64).map(|v| v as usize);
    Some(Row {
        schema: store::SCHEMA,
        month: store::month_now(),
        platform: "worker".into(),
        engine: store::engine_identity(),
        cpu: machine.cpu.clone(),
        cores: machine.cores,
        ram_gb: machine.ram_gb,
        rung: num("rung")?,
        target_dofs: num("target_dofs")?,
        dofs: num("dofs")?,
        elements: num("elements")?,
        field_leaves,
        wall_ms: r.get("wall_ms").and_then(Value::as_f64)?,
        deflection_change_rel: r
            .get("change")
            .and_then(|c| c.get("deflection_rel"))
            .and_then(Value::as_f64),
        vm_change_rel: r
            .get("change")
            .and_then(|c| c.get("vm_rel"))
            .and_then(Value::as_f64),
    })
}
