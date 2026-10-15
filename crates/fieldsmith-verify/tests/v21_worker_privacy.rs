//! V21 — the worker channel's privacy facts (GOAL v0.4 section C, C4): a sentinel job
//! crosses the Noise channel on real loopback TCP while `dumpcap` captures the
//! interface, and the test then answers three questions by measurement.
//!
//! 1. **The channel carries no plaintext of the job.** A unique sentinel document (never
//!    `examples/bracket.fsm`) is built into a `.fsmz`; the payload bytes of the frames
//!    that carry the host's port — the Noise channel itself — are scanned for everything
//!    a leaking channel could expose: the document's internal strings (node, region,
//!    support and load ids, the case label, source fragments), sample windows of the
//!    exact base64 the request carries, the request JSON's `docz` field name, the zip
//!    magic raw `.fsmz` bytes would start with, and little- and big-endian f32 images of
//!    every tracked solve value. Zero matches is the pass condition. The capture is
//!    first proven to have seen the channel's traffic — the SYN, the Noise handshake and
//!    the request itself, pinned by the request's exact frame size — with zero drops.
//!    The whole capture's payload bytes are scanned too but reported informationally,
//!    never asserted: this box's ambient services carry text shaped like the sentinel
//!    with no job running — the session's model proxy and an ollama-backed client echo
//!    conversation content that includes this harness's own source and prior reports
//!    (measured 2026-10-14: 258 whole-capture matches of one generic statement string,
//!    every sentinel string present, the docz base64 windows re-entering ambient
//!    traffic, while the job was fully encrypted and clean).
//! 2. **The worker writes only where the operator said.** The library worker runs with
//!    a fresh HOME and TMPDIR and the costs store inside the stated artifacts
//!    directory; every surface is walked afterwards — the artifacts hold the response
//!    envelope and the store and nothing else, HOME and TMPDIR hold nothing — and the
//!    files are byte-scanned for the same leak patterns (the response envelope's own
//!    `name` field is by-design legal and deliberately outside the pattern set; so are
//!    its decimal result digits).
//! 3. **The diverged answer is measured.** Same machine, same engine revision: the
//!    worker's tracked values against a local in-process solve of the *same request
//!    bytes* (the request string is built once and driven through both paths).
//!
//! Named gaps (stated because this test cannot measure them):
//! - Loopback only. The LAN path is not sniffed here; the channel's encryption is
//!   argued by the same Noise stack the bytes pass through, not re-proven per link.
//!   Cross-machine divergence is measured at M15 close on the second box.
//! - The census sees plaintext forms, not ciphertext interiors (`dumpcap`, not
//!   `tcpdump` — this box's tcpdump carries no file caps). It proves no job plaintext
//!   crosses the link, which is what a shared LAN leg owes on the wire.
//! - 4-byte f32 patterns carry a false-positive chance against arbitrary stream bytes
//!   (~2e-10 per position; measured 0 — the residual is named, not hidden).
//! - A compressed-then-encrypted leak shape is invisible to the zip-magic and base64
//!   patterns; nothing on this path compresses before encrypting, and a future
//!   stage that did would have to argue its own plaintext-free transport.
//! - Record headers carry lengths and timestamps; only packet payload bytes are
//!   scanned (headers hold numbers, never job plaintext).
//! - The asserted census reads the job's own sockets plus the worker's files. A worker
//!   that exfiltrated through a foreign socket would be missed by the wire scan — a
//!   scope boundary of this case, not a measured risk: the worker here is the library
//!   this repo controls, and the file census bounds what it persists. The whole-capture
//!   scan that once attempted to answer this question measured false positives from
//!   ambient traffic instead (see the wire census section).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use fieldsmith_api::b64;
use fieldsmith_core::fsmz;
use fieldsmith_verify::{doc, Check, Report};
use serde_json::Value;

/// The sentinel document: a small plate with two bores, but every internal name is
/// unique to this case — a leak census against these strings answers only about THIS
/// traffic. Geometry is proven syntax (mirrors examples/bracket.fsm's bore-side
/// regions); the rungs [30000, 150000] are the empirically established minimum set
/// where bore regions resolve to facets (rungs below ~30000 produced empty
/// region-facet sets in the slice-3 e2e work).
const SENTINEL: &str = r#"name "sentinel-v21"
material petg

box platev21 size=(40, 40, 6) at=(20, 20, 3)
cylinder holev21a r=5 h=20 at=(10, 10, 3) subtract
cylinder holev21b r=5 h=20 at=(30, 30, 3) subtract

region v21pad = holev21a.side
region v21pin = holev21b.side
support v21hold v21pad fixed=xyz
load v21pull v21pin force=(0, 0, -150) case="V21 case"
"#;

/// Document-internal strings. A response envelope carries none of these by design —
/// it holds the doc *name* (whose bare string is therefore excluded everywhere), the
/// question echo, and numeric records — so these are the census's leak patterns.
const DOC_PATTERNS: &[&str] = &[
    "holev21a",
    "holev21b",
    "platev21",
    "v21pad",
    "v21pin",
    "v21pull",
    "v21hold",
    "V21 case",
    "r=5 h=20",
    "force=(0, 0, -150)",
    "fixed=xyz",
    "at=(10, 10, 3)",
    "at=(30, 30, 3)",
];

/// A classic-pcap parse: concatenated packet payload bytes (the plaintext census
/// scans these), per-packet payload slices (the port census walks these), and the
/// link type.
struct Pcap {
    payloads: Vec<u8>,
    pkt: Vec<Vec<u8>>,
    linktype: u32,
}

/// Parse a capture written by `dumpcap -F pcap`. Magic `d4 c3 b2 a1` (little-endian
/// microseconds, what this box writes) with the other three accepted; payload bytes
/// only, and a record whose caplen runs past EOF is a parse error, not a truncation
/// to walk over silently.
fn parse_pcap(raw: &[u8]) -> Result<Pcap, String> {
    if raw.len() < 24 {
        return Err(format!("capture file is {} bytes — not a pcap", raw.len()));
    }
    let (le, _nano) = match &raw[..4] {
        [0xd4, 0xc3, 0xb2, 0xa1] => (true, false),
        [0xa1, 0xb2, 0xc3, 0xd4] => (false, false),
        [0x4d, 0x3c, 0xb2, 0xa1] => (true, true),
        [0xa1, 0xb2, 0x3c, 0x4d] => (false, true),
        other => return Err(format!("capture is not classic pcap (magic {other:02x?})")),
    };
    let u32at = |off: usize| -> u32 {
        let w: [u8; 4] = raw[off..off + 4].try_into().expect("4 bytes at offset");
        if le {
            u32::from_le_bytes(w)
        } else {
            u32::from_be_bytes(w)
        }
    };
    let linktype = u32at(20);
    let mut pc = Pcap {
        payloads: Vec::new(),
        pkt: Vec::new(),
        linktype,
    };
    let mut off = 24usize;
    while off + 16 <= raw.len() {
        let caplen = u32at(off + 8) as usize;
        off += 16;
        if off + caplen > raw.len() {
            return Err(format!(
                "packet record at byte {off} claims {caplen} payload bytes, past EOF {}",
                raw.len()
            ));
        }
        let payload = raw[off..off + caplen].to_vec();
        pc.payloads.extend_from_slice(&payload);
        pc.pkt.push(payload);
        off += caplen;
    }
    Ok(pc)
}

/// Count non-overlapping occurrences of `needle` in `blob`.
fn count_matches(blob: &[u8], needle: &[u8]) -> usize {
    if blob.len() < needle.len() {
        return 0;
    }
    blob.windows(needle.len()).filter(|w| *w == needle).count()
}

/// Every file under `root` (depth-first, sorted).
fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if d.is_dir() {
            for e in std::fs::read_dir(&d).expect("v21 read_dir").flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        } else {
            out.push(d);
        }
    }
    out.sort();
    out
}

/// The byte patterns that would expose the job, built after the local solve so the
/// f32 images match the actual tracked values. `wire` — everything a leaking channel
/// would expose on the link; `worker` — the same census against files the worker
/// wrote (the request's field name is excluded: artifacts never carry the request).
/// A census: named byte patterns.
type Census = Vec<(String, Vec<u8>)>;

fn leak_patterns(local: &Value, docz_b64: &str) -> (Census, Census) {
    let mut wire: Vec<(String, Vec<u8>)> = DOC_PATTERNS
        .iter()
        .map(|s| (format!("doc string {s:?}"), s.as_bytes().to_vec()))
        .collect();
    wire.push((
        "zip magic of raw .fsmz bytes".into(),
        b"PK\x03\x04".to_vec(),
    ));
    wire.push(("request field name \"docz\":".into(), b"\"docz\":".to_vec()));
    // Three windows of the exact base64 the request carries — long, unique, and the
    // literal bytes a plaintext request would show.
    let n = docz_b64.len();
    if n >= 40 {
        for (i, start) in [0usize, n / 3, n * 2 / 3].into_iter().enumerate() {
            let start = start.min(n - 40);
            let window = &docz_b64[start..start + 40];
            wire.push((
                format!("docz b64 window {i} ({window}…)"),
                window.as_bytes().to_vec(),
            ));
        }
    }
    // f32 images of every tracked value at every rung, both endians — the raw field
    // numbers a leaking binary path would carry. The envelope prints decimals, whose
    // ASCII text is not these images, so both scans expect zero.
    for rec in local["outcome"]["records"].as_array().expect("records") {
        let rung = rec["rung"].as_u64().unwrap_or(0);
        for (key, unit) in [("deflection_mm", "mm"), ("vm_mpa", "MPa")] {
            let v = rec["tracked"][key].as_f64().expect(key) as f32;
            wire.push((
                format!("f32 LE of rung {rung} {key} ({unit})"),
                v.to_le_bytes().to_vec(),
            ));
            wire.push((
                format!("f32 BE of rung {rung} {key} ({unit})"),
                v.to_be_bytes().to_vec(),
            ));
        }
    }
    let worker: Vec<(String, Vec<u8>)> = wire
        .iter()
        .filter(|(name, _)| !name.contains("\"docz\":"))
        .cloned()
        .collect();
    (wire, worker)
}

#[test]
fn v21_worker_privacy() {
    let mut rep = Report::new(
        "V21",
        "worker channel privacy — plaintext census on loopback, artefact confinement, divergence",
    );

    // --- fresh surfaces, one request string driven through both paths ------------
    let dir = std::env::temp_dir().join(format!("fieldsmith-v21-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let artifacts = dir.join("artifacts");
    let home = dir.join("home");
    let tmp = dir.join("tmp");
    for p in [&artifacts, &home, &tmp] {
        std::fs::create_dir_all(p).expect("v21 fresh surface");
    }
    // The worker's whole environment is fresh: HOME empty, TMPDIR empty, and the
    // costs store inside the stated artifacts directory. The API only reads the
    // store for seeding; only the worker-side persist() appends to it — so the
    // artifacts directory is where the appends must land.
    std::env::set_var("HOME", &home);
    std::env::set_var("TMPDIR", &tmp);
    std::env::set_var("FIELDSMITH_COSTS_STORE", artifacts.join("costs.jsonl"));

    let doc = doc(SENTINEL);
    let docz = fsmz::save(&doc).expect("fsmz builds");
    let docz_b64 = b64::encode(&docz);
    let request = serde_json::json!({
        "apiv": 2,
        "docz": docz_b64,
        "question": {"kind": "ladder", "rungs": [30000u64, 150000u64]}
    })
    .to_string();
    let local: Value = serde_json::from_str(&fieldsmith_api::respond(&request).body)
        .expect("local envelope parses");
    assert_eq!(local["status"], "ok", "local baseline: {local}");

    // --- the capture window -------------------------------------------------------
    let cap = dir.join("capture.pcap");
    let mut dump = Command::new("dumpcap")
        .args(["-P", "-i", "lo", "-w"])
        .arg(&cap)
        .args(["-a", "duration:20"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("dumpcap spawns (this box grants it cap_net_admin/cap_net_raw file caps)");
    let mut stderr = dump.stderr.take().expect("dumpcap stderr piped");
    // The capture must be live before any of our traffic exists: dumpcap prints the
    // interface banner immediately on start; we read until it appears (or the process
    // dies), and only then bring the host port up.
    let mut raw_head = Vec::new();
    let mut buf = [0u8; 512];
    while !String::from_utf8_lossy(&raw_head).contains("Capturing") {
        let n = stderr.read(&mut buf).expect("read dumpcap banner");
        if n == 0 {
            break;
        }
        raw_head.extend_from_slice(&buf[..n]);
    }
    let head = String::from_utf8_lossy(&raw_head).into_owned();
    assert!(
        head.contains("Capturing"),
        "dumpcap never reported capturing (and our traffic being on the wire is unproven): {head:?}"
    );
    // Measured on this box, 2026-10-14: dumpcap reports `Capturing` before its capture
    // thread is armed, and the job that followed immediately fell partly inside the
    // arm window — the request's SYN, handshake and request frames crossed before the
    // capture's first record, leaving the census to judge only the 1028-byte answer
    // frame (a 1000-byte envelope plus 28 bytes of channel framing). The settle is the
    // fix: when the host binds, the capture has been taking packets for half a second,
    // and the whole exchange must land in the file — asserted below by the request's
    // exact frame size.
    std::thread::sleep(std::time::Duration::from_millis(500));

    // --- the exchange, on the wire, inside the window ------------------------------
    let host = fieldsmith_worker::Host::bind("127.0.0.1", 0).expect("host binds");
    let port = host.local_addr().expect("addr").port();
    let token = *host.token();
    let worker_logs: Arc<Mutex<Vec<String>>> = Arc::default();
    let host_logs: Arc<Mutex<Vec<String>>> = Arc::default();
    let wl = worker_logs.clone();
    let hl = host_logs.clone();
    let artifacts_for_worker = artifacts.clone();
    let worker = std::thread::spawn(move || {
        let mut log = |l: &str| wl.lock().expect("log lock").push(l.to_string());
        let mut job = |req: &str| fieldsmith_api::respond(req).body;
        fieldsmith_worker::run(
            &token,
            "127.0.0.1",
            port,
            &fieldsmith_worker::RunOpts {
                once: true,
                artifacts: Some(artifacts_for_worker),
                reconnect_ms: 100,
                max_attempts: Some(10),
            },
            &mut log,
            &mut job,
        )
    });
    let outcome = {
        let mut log = |l: &str| hl.lock().expect("log lock").push(l.to_string());
        host.exchange(&request, &mut log)
            .expect("host exchange should succeed")
    };
    let answer: Value =
        serde_json::from_str(&outcome.response).expect("worker answer parses as JSON");
    assert_eq!(
        answer["status"], "ok",
        "worker answer: {}",
        outcome.response
    );
    worker
        .join()
        .expect("worker thread")
        .expect("worker run should succeed");

    // --- wait out the window, then measure the capture ------------------------------
    let status = dump.wait().expect("dumpcap exits");
    let tail = {
        let mut t = Vec::new();
        let _ = stderr.read_to_end(&mut t);
        String::from_utf8_lossy(&t).into_owned()
    };
    assert!(status.success(), "dumpcap exited {status}: {head}{tail}");
    let banner = head + &tail;
    let raw = std::fs::read(&cap).expect("capture readable");
    let pcap = parse_pcap(&raw).expect("classic pcap parses");
    // dumpcap's own accounting must agree with the file we parsed — a mismatch means
    // the writer's format changed under us, and the census below would be unmoored.
    let captured_n: usize = banner
        .lines()
        .find(|l| l.contains("Packets captured:"))
        .and_then(|l| {
            l.rsplit("Packets captured:")
                .next()
                .and_then(|tail| tail.trim().parse().ok())
        })
        .unwrap_or_else(|| panic!("no 'Packets captured:' line in dumpcap output: {banner:?}"));
    assert_eq!(
        captured_n,
        pcap.pkt.len(),
        "dumpcap counted {captured_n} packets, the file holds {} records — parse and writer disagree",
        pcap.pkt.len()
    );
    // Every drop line dumpcap printed must say 0. Line shape observed on this box:
    // `Packets received/dropped on interface 'Loopback: lo': <r>/<d> (pcap:<n>/
    // dumpcap:<n>/flushed:<n>/ps_ifdrop:<n>) (<p>%)` — the counters follow the colon
    // as a received/dropped pair plus a per-counter breakdown.
    let mut saw_drop_line = false;
    for l in banner.lines().filter(|l| l.contains("dropped")) {
        saw_drop_line = true;
        let after = l
            .split_once(':')
            .map(|(_, a)| a)
            .unwrap_or_else(|| panic!("drop line without counters: {l:?}"));
        let digits_at = |s: &str| {
            let d: String = s
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            d
        };
        let (recv, rest) = after
            .split_once('/')
            .unwrap_or_else(|| panic!("unexpected drop-line shape: {l:?}"));
        let recv_digits = digits_at(recv);
        let drop_digits = digits_at(rest);
        assert_eq!(
            drop_digits, "0",
            "dumpcap dropped {drop_digits:?} of {recv_digits} packets ({l:?}) — a census over a lossy capture is not a census"
        );
        if let Some((_, breakdown)) = after.split_once('(') {
            let inside = breakdown.split(')').next().unwrap_or("");
            for part in inside.split('/') {
                let d = digits_at(part);
                assert_eq!(
                    d, "0",
                    "drop breakdown counter in {l:?} was {d:?} — a census over a lossy capture is not a census"
                );
            }
        }
    }
    assert!(
        saw_drop_line,
        "no dropped-packet line in dumpcap's output — the drop count could not be verified: {banner:?}"
    );

    // Our traffic is on the wire, in this capture: EN10MB frames whose TCP header
    // carries the worker's port. In the raced run of 2026-10-14 the job crossed as
    // six frames in total — its SYN pair, Noise handshake and request had already
    // left before the capture's first record. With the settle above the whole
    // exchange must land inside: SYN pair, Noise handshake (three frames), request,
    // answer, close. The job's per-frame TCP payloads are collected here — they are
    // exactly the bytes the wire census judges below.
    assert_eq!(
        pcap.linktype, 1,
        "loopback link type changed (observed EN10MB=1 on this box); the port census needs re-deriving"
    );
    let mut our_frames = 0usize;
    let mut job_payloads: Vec<u8> = Vec::new();
    let mut job_frame_sizes: Vec<usize> = Vec::new();
    for p in &pcap.pkt {
        if p.len() < 14 + 20 || p[12] != 0x08 || p[13] != 0x00 {
            continue;
        }
        let ihl = ((p[14] & 0x0f) as usize) * 4;
        if ihl < 20 || p.len() < 14 + ihl + 4 || p[14 + 9] != 6 {
            continue;
        }
        let t = 14 + ihl;
        let sp = u16::from_be_bytes([p[t], p[t + 1]]);
        let dp = u16::from_be_bytes([p[t + 2], p[t + 3]]);
        if sp != port && dp != port {
            continue;
        }
        our_frames += 1;
        if p.len() < t + 20 {
            continue; // header-only frame shape; no payload to census
        }
        let doff = ((p[t + 12] >> 4) as usize) * 4;
        let payload = &p[t + doff..];
        job_frame_sizes.push(payload.len());
        job_payloads.extend_from_slice(payload);
    }
    assert!(
        our_frames >= 6,
        "the capture held {our_frames} frames for the job port {port} — our traffic was not seen, and a census that never saw the traffic proves nothing"
    );
    // The request must itself be in the file. The channel's unit framing is one
    // [u32 BE length] prefix, then — for a first chunk — an [u64 BE declared total],
    // then the ChaChaPoly ciphertext of the plaintext plus its 16-byte tag, so a
    // request of R bytes crosses as one frame of R + 28 (valid while R + 8 fits a
    // chunk; this request fits). If no frame has that size, the request crossed
    // outside the window and the census again judged only the answer.
    assert!(
        job_frame_sizes.contains(&(request.len() + 28)),
        "no job frame carried a request-sized payload (expected {} = request + 28 of channel framing; frames {:?}) — the request did not cross inside the capture window",
        request.len() + 28,
        job_frame_sizes
    );

    // --- the plaintext census ---------------------------------------------------------
    let (wire_patterns, worker_patterns) = leak_patterns(&local, &docz_b64);
    let mut census_rows = String::new();
    let mut wire_total = 0usize; // job-socket column — asserted zero below
    let mut ambient_total = 0usize; // whole-capture column — informational, never asserted
    for (name, pat) in &wire_patterns {
        let job = count_matches(&job_payloads, pat);
        let whole = count_matches(&pcap.payloads, pat);
        wire_total += job;
        ambient_total += whole;
        census_rows.push_str(&format!("| {name} | {job} | {whole} |\n"));
    }

    // --- the worker's own surfaces -----------------------------------------------------
    let artifact_files = walk(&artifacts);
    let artifact_names: Vec<String> = artifact_files
        .iter()
        .map(|p| {
            p.file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let n_json = artifact_names
        .iter()
        .filter(|n| n.ends_with(".json"))
        .count();
    assert_eq!(
        n_json, 1,
        "the artifacts directory held {n_json} response JSONs (expected one): {artifact_names:?}"
    );
    assert!(
        artifact_names.iter().any(|n| n == "costs.jsonl"),
        "the costs store lives inside the stated artifacts directory: {artifact_names:?}"
    );
    assert_eq!(
        artifact_files.len(),
        2,
        "artifacts held {} files (envelope + store expected): {artifact_names:?}",
        artifact_files.len()
    );
    assert!(
        walk(&tmp).is_empty(),
        "the worker left files in its fresh TMPDIR"
    );
    assert!(
        walk(&home).is_empty(),
        "the worker wrote into its fresh HOME (the library worker writes nowhere; only the CLI's enrol step puts a token file there)"
    );
    let mut artifact_bytes = Vec::new();
    for f in &artifact_files {
        artifact_bytes.extend(std::fs::read(f).expect("artifact readable"));
    }
    let mut worker_total = 0usize;
    let mut worker_rows = String::new();
    for (name, pat) in &worker_patterns {
        let n = count_matches(&artifact_bytes, pat);
        worker_total += n;
        worker_rows.push_str(&format!("| {name} | {n} |\n"));
    }
    // And the printing surface: the worker's log lines name sizes and times, never
    // geometry or fields.
    let logs = worker_logs.lock().expect("log lock").join("\n");
    for s in DOC_PATTERNS {
        assert!(
            !logs.contains(s),
            "worker log leaked document text {s:?}: {logs:?}"
        );
    }
    assert!(
        !logs.contains("sentinel-v21"),
        "worker log leaked the document name: {logs:?}"
    );
    let _ = host_logs;

    // --- divergence, measured -----------------------------------------------------------
    let local_recs = local["outcome"]["records"].as_array().expect("records");
    let worker_recs = answer["outcome"]["records"].as_array().expect("records");
    let mut diverge_rows = String::new();
    for (l, w) in local_recs.iter().zip(worker_recs.iter()) {
        let rung = l["rung"].as_u64().expect("rung");
        for (key, unit) in [("deflection_mm", "mm"), ("vm_mpa", "MPa")] {
            let a = l["tracked"][key].as_f64().expect(key);
            let b = w["tracked"][key].as_f64().expect(key);
            diverge_rows.push_str(&format!(
                "| rung {rung} {key} ({unit}) | {a} | {b} | {}\n",
                if a == b { "identical" } else { "differ" }
            ));
            rep.check(Check::relative(
                &format!("divergence rung {rung} {key} ({unit}), worker vs local"),
                a,
                b,
                unit,
                0.001,
            ));
        }
    }

    // --- the report -----------------------------------------------------------------------
    rep.section(
        "What ran",
        format!(
            "The sentinel document `sentinel-v21` was built at send time into a `.fsmz` ({} bytes, {} as base64) and travelled as the host's job request to an enrolled worker over loopback TCP and Noise_XXpsk3; the answer came back as the API response records. Host-measured wall: {:.0} ms. Rungs [30000, 150000]. The document's strings are unique to this case, but the box is not: its own services carry shaped text at idle (see the wire census), so the asserted census is scoped to the job's frames and the whole capture is reported informationally.\n\nThe capture ran `dumpcap -F pcap -i lo -a duration:20`, opened before the host's port was bound — the interface banner was read first, then a 500 ms settle, because dumpcap reports capturing before its capture thread is armed and without the settle the measured race of 2026-10-14 let the request cross before the file's first record — and closed after the answer landed. Packets captured: {} (dumpcap's own account, cross-checked against the file's records — both {}). Frames carrying the job port: {}. Capture bytes: {}. Dropped: 0 (every drop counter dumpcap printed). The request's own presence in the file is asserted by frame size: a request of {} bytes crosses the channel as one frame of {} bytes (4 length prefix + 8 declared total + payload + 16 tag).",
            docz.len(),
            docz_b64.len(),
            outcome.wall_ms,
            captured_n,
            pcap.pkt.len(),
            our_frames,
            raw.len(),
            request.len(),
            request.len() + 28
        ),
    );
    rep.section(
        "Wire census",
        format!(
            "Packet payload bytes only (record headers hold lengths and timestamps, not payloads). The census's question has one socket: the frames that carry the host's port — the Noise channel, SYN pair included — whose concatenated payloads are the job column, asserted to zero matches. The whole capture is the informational column, because this box's ambient services carry text shaped like the sentinel with no job running: the session's model proxy and an ollama-backed client echo conversation content, which on the measured run of 2026-10-14 put 258 whole-capture matches on one generic statement string, every document string present, and all three docz base64 windows re-entering ambient traffic through prior reports' tables — the `.fsmz` zip is byte-stable per document, so a document's base64 recurs verbatim in anything that quotes a report about it. The job column is what the channel owes: a leaking channel exposes the request plaintext first ({} bytes of base64 are the bulk of it), and the request itself is pinned in-frame by size above.\n\n| pattern | job-socket (asserted 0) | whole capture (informational) |\n| :-- | --: | --: |\n{census_rows}|\nJob-socket total: {wire_total}. Whole-capture total: {ambient_total}.",
            docz_b64.len()
        ),
    );
    rep.section(
        "Worker surfaces",
        format!(
            "The library worker ran with a fresh empty HOME, a fresh empty TMPDIR, and the costs store pointed inside the stated artifacts directory. After the job: artifacts hold exactly `{}` (the response envelope plus the cost-row store), TMPDIR is empty, HOME is empty — the library worker wrote nowhere else; only the CLI's `enrol` step ever puts a token file in HOME. All bytes the worker wrote, scanned:\n\n| pattern | matches in worker-written bytes |\n| :-- | --: |\n{worker_rows}|\nMeasured total: {worker_total}. The response envelope's `name` field (`sentinel-v21`) and decimal result digits are by-design legal and sit outside the pattern set. The worker's own stdout named sizes and times — scanned for every document string above, zero hits.",
            artifact_names.join(", ")
        ),
    );
    rep.section(
        "Divergence",
        format!(
            "Same machine, same engine revision, byte-for-byte the same request string through both paths: one in-process solve, one round-tripped solve.\n\n| quantity | local | worker | |\n| :-- | --: | --: | :-- |\n{diverge_rows}\nBit-identity is the same-machine result; cross-machine behaviour is measured at M15 close on a second box (GOAL section C), not here."
        ),
    );
    rep.section(
        "Named gaps",
        "Loopback only — the LAN leg is not sniffed in this case; the encryption argument travels with the same Noise stack the bytes pass through (the cross-machine leg is measured at M15 close). The census sees plaintext forms, not ciphertext interiors — `dumpcap`, not `tcpdump`, because this box's tcpdump carries no file caps. 4-byte f32 patterns carry a ~2e-10 per-position false-positive chance; measured 0. Zip-magic and base64 windows would miss a compressed-then-encrypted leak shape; nothing on this path compresses before encrypting. Record headers and IPv6 side traffic sit outside the payload scan. The dumpcap arm race is measured, not modelled: without the settle, the request crossed before the capture's first record and the only in-window job payload was the answer's frame — the settle plus the request-frame assertion are what make this census honest. Base64 windows recur across runs (the `.fsmz` zip is byte-stable per document), so an informational column match may reference earlier sessions' quoted reports, not this job. The wire census bounds the job's own sockets; a worker exfiltrating through a foreign socket would not appear in it — a scope boundary, bounded in kind by the worker-artifacts census above.",
    );
    rep.check(Check::absolute(
        "job-socket plaintext pattern matches (expected 0)",
        0.0,
        wire_total as f64,
        "matches",
        0.0,
    ));
    rep.check(Check::absolute(
        "worker-written-file pattern matches (expected 0)",
        0.0,
        worker_total as f64,
        "matches",
        0.0,
    ));
    rep.finish();

    std::env::remove_var("FIELDSMITH_COSTS_STORE");
    std::env::remove_var("TMPDIR");
    std::env::remove_var("HOME");
    let _ = std::fs::remove_dir_all(&dir);
}
