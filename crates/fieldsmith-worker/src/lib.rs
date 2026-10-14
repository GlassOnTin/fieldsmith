//! fieldsmith-worker: the M15 slice-3 pieces — enrolment [`token`], the Noise-framed
//! [`chan`], the session [`host`], and the worker [`runner`] loop. Design (GOAL
//! section C): `fieldsmith session` binds where the user says and prints an enrolment
//! token once (host Noise static key + pre-shared key, base64); `fieldsmith worker
//! enrol <token> <host:port>` stores it 0600; `fieldsmith worker` connects outbound
//! only, `Noise_XXpsk3_25519_ChaChaPoly_SHA256` through the `snow` crate (hand-rolled
//! crypto is off the table). Jobs are the API request — the document enclosed in a
//! `.fsmz` built at send time; answers are the API response records. The worker writes
//! artifacts only under the operator-stated directory and never prints geometry or
//! fields; the host prints per-rung walls from the worker's own records and cost =
//! user-stated rate × measured time.

pub mod chan;
pub mod host;
pub mod runner;
pub mod token;

pub use chan::{Chan, ChanError, PARAMS};
pub use host::{Host, HostOutcome};
pub use runner::{run, RunOpts};
pub use token::Token;

#[cfg(test)]
mod tests {
    use super::*;

    /// Host and worker, both in-process, over real loopback TCP: the full slice-3 wire.
    #[test]
    fn host_and_worker_roundtrip() {
        let host = Host::bind("127.0.0.1", 0).expect("bind");
        let addr = host.local_addr().expect("addr");
        let token = *host.token();
        let port = addr.port();
        // The job handler stands in for `fieldsmith_api::respond`: an identity on the
        // request, so the test pins transport, not the engine.
        let worker = std::thread::spawn(move || {
            let mut logs: Vec<String> = Vec::new();
            let r = run(
                &token,
                "127.0.0.1",
                port,
                &RunOpts {
                    once: true,
                    artifacts: None,
                    reconnect_ms: 100,
                    max_attempts: Some(10),
                },
                &mut |l| logs.push(l.to_string()),
                &mut |req| format!("echo:{req}"),
            );
            (r, logs)
        });
        let host = std::thread::spawn(move || {
            let mut logs: Vec<String> = Vec::new();
            host.exchange("hello job", &mut |l| logs.push(l.to_string()))
        });
        let outcome = host
            .join()
            .expect("host thread")
            .expect("host exchange should succeed");
        let (run_result, _worker_logs) = worker.join().expect("worker thread");
        run_result.expect("worker run should succeed");
        assert_eq!(outcome.response, "echo:hello job");
        assert!(outcome.wall_ms > 0.0, "the host wall is measured");
    }

    /// A worker holding a different token cannot pull a job. Empirical shape (the
    /// first version of this test hung: against a listener nobody accepts, a blocked
    /// handshake read waits forever): the host rejects the wrong-psk handshake at msg 3
    /// (see chan::tests), the worker's first job read ends at EOF when the host drops
    /// the socket, and a one-attempt budget gives up without retrying. The same
    /// exchange then serves a real token — the host logged the rejection, moved on,
    /// and answers.
    #[test]
    fn wrong_token_worker_cannot_pull_a_job() {
        let host = Host::bind("127.0.0.1", 0).expect("bind");
        let addr = host.local_addr().expect("addr");
        let port = addr.port();
        let real_token = *host.token();
        let host_thread = std::thread::spawn(move || {
            let mut logs: Vec<String> = Vec::new();
            let r = host.exchange("pull the job", &mut |l| logs.push(l.to_string()));
            (r, logs)
        });

        let rogue = Token::generate().0; // A real encoding of the wrong credential.
        let r = run(
            &rogue,
            "127.0.0.1",
            port,
            &RunOpts {
                once: true,
                artifacts: None,
                reconnect_ms: 50,
                max_attempts: Some(1),
            },
            &mut |_l| {},
            &mut |req| req.to_string(),
        );
        assert!(r.is_err(), "a rogue worker must not complete a job");

        let real = run(
            &real_token,
            "127.0.0.1",
            port,
            &RunOpts {
                once: true,
                artifacts: None,
                reconnect_ms: 100,
                max_attempts: Some(10),
            },
            &mut |_l| {},
            &mut |req| format!("echo of {req}"),
        );
        real.expect("the real worker should complete the same exchange");

        let (host_result, logs) = host_thread.join().expect("host thread");
        let outcome = host_result.expect("the host should answer the real worker");
        assert_eq!(outcome.response, "echo of pull the job");
        assert!(
            logs.iter().any(|l| l.contains("rejected")),
            "the host logged the handshake refusal: {logs:?}"
        );
    }

    /// persist(): the envelope lands verbatim, and the shape-only rung rows reach the
    /// costs store with the worker machine identity and the envelope's field_leaves.
    /// This test is the crate's only store-path user, so its env override is private.
    #[test]
    fn persist_writes_envelope_and_store_rows() {
        let dir =
            std::env::temp_dir().join(format!("fieldsmith-worker-persist-{}", std::process::id()));
        let store = dir.join("costs.jsonl");
        std::env::set_var("FIELDSMITH_COSTS_STORE", &store);
        let envelope = serde_json::json!({
            "apiv": 1, "engine": "fieldsmith-test", "status": "ok", "name": "t",
            "question": {"kind": "ladder"},
            "outcome": {
                "records": [
                    {"rung": 0, "target_dofs": 1000, "dofs": 1012, "elements": 490,
                     "wall_ms": 7.5, "change": null, "tracked": {"deflection_mm": 1.0, "vm_mpa": 10.0}},
                    {"rung": 1, "target_dofs": 4000, "dofs": 3988, "elements": 1970,
                     "wall_ms": 21.25, "change": {"deflection_rel": 0.01, "vm_rel": 0.02, "deflection_mm": 1.01, "vm_mpa": 10.2},
                     "tracked": {"deflection_mm": 1.01, "vm_mpa": 10.2}}
                ],
                "field_leaves": 11, "total_ms": 28.75
            }
        });
        let mut logs: Vec<String> = Vec::new();
        runner::persist(&dir, &envelope.to_string(), &mut |l| {
            logs.push(l.to_string())
        });
        let written = std::fs::read_dir(&dir)
            .expect("dir created")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
            .count();
        assert_eq!(written, 1, "one response envelope artifact");
        let rows = fieldsmith_api::store::read_all(&store);
        assert_eq!(rows.len(), 2, "one row per rung record");
        assert_eq!(rows[0].platform, "worker");
        assert_eq!(rows[0].rung, 0);
        assert_eq!(rows[1].rung, 1);
        assert_eq!(rows[1].target_dofs, 4000);
        assert_eq!(rows[1].field_leaves, 11);
        assert_eq!(rows[1].deflection_change_rel, Some(0.01));
        assert_eq!(rows[0].deflection_change_rel, None);
        assert!(logs.iter().any(|l| l.contains("cost row")));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("FIELDSMITH_COSTS_STORE");
    }
}
