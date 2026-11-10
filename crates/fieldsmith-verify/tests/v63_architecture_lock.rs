//! V63 — Unified Human ↔ AI Architecture Lock:
//! 1. 1:1 `TOOLS_JSON` ↔ `Server::tool` dispatch symmetry across all 34 MCP tools.
//! 2. Executable `# @mcp:` literate directives across every `.fsm` file in `examples/`.
//! 3. `apiv: 2` same-result envelope parity between `fieldsmith_api::respond` and
//!    `fieldsmith_mcp::Server::handle` for `QuestionSpec::Verify` (`ok` + `refusal`)
//!    and `QuestionSpec::Sensitivity`, plus `analyze_sdf_channel` accuracy & BC binding.
//! 4. Typed `.fsm` grammar round-trip (`coupled <id>`, `assert "..."`, `role=fluid`,
//!    `solve plasticity=j2 hardening_n=...`).
//! 5. HTTP `409 Conflict` optimistic concurrency (`expected_revision`) on `/api/workspace`
//!    and `/api/mcp`.

use std::path::{Path, PathBuf};

use fieldsmith_doc::{fsm, MaterialRole, PlasticityLaw};
use fieldsmith_mcp::http::{http_request_json, spawn_http_server, ServeConfig};
use fieldsmith_mcp::Server;
use fieldsmith_verify::{Check, Report};
use serde_json::{json, Map, Value};

const TIMING_DROPS: &[&str] = &[
    "wall_ms",
    "total_ms",
    "mesh_ms",
    "solve_ms",
    "thermal_ms",
    "elapsed_ms",
    "cost_fit",
    "stop_reason",
    "gpu_device",
    "gpu_gflops",
    "gpu_bw_gbps",
    "fit_ms",
    "seed_ms",
    "seed_n",
];

fn canon(value: &Value) -> Value {
    match value {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                if TIMING_DROPS.contains(&k.as_str()) {
                    continue;
                }
                out.insert(k.clone(), canon(v));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
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

fn collect_fsm_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|d| d.path()))
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_fsm_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("fsm") {
            out.push(path);
        }
    }
}

fn rpc_call(srv: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string();
    let raw = srv
        .handle(&req)
        .unwrap_or_else(|| panic!("expected JSON-RPC reply for {method}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("invalid JSON reply ({e}): {raw}"))
}

fn mcp_tool_call(srv: &mut Server, id: u64, name: &str, args: Value) -> (bool, Value) {
    let reply = rpc_call(
        srv,
        id,
        "tools/call",
        json!({
            "name": name,
            "arguments": args,
        }),
    );
    assert!(
        reply.get("error").is_none(),
        "JSON-RPC protocol error calling tool {name}: {reply}"
    );
    let result = &reply["result"];
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("missing text content in reply for {name}: {reply}"));
    let payload = serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({ "text": text }));
    (is_error, payload)
}

#[test]
fn v63_architecture_lock() {
    let mut report = Report::new(
        "V63",
        "Human-AI state symmetry, executable @mcp contracts, API-MCP parity, and optimistic concurrency",
    );

    // -------------------------------------------------------------------------
    // 1. TOOLS_JSON <-> Server::tool 1:1 dispatch symmetry
    // -------------------------------------------------------------------------
    let mut probe_srv = Server::new();
    let list_reply = rpc_call(&mut probe_srv, 1, "tools/list", json!({}));
    let tools = list_reply["result"]["tools"]
        .as_array()
        .expect("tools/list array");
    let tool_count = tools.len();
    let mut unknown_tool_count = 0usize;
    for (idx, t) in tools.iter().enumerate() {
        let name = t["name"].as_str().expect("tool name");
        assert!(
            t["inputSchema"].is_object(),
            "tool {name} missing inputSchema"
        );
        let (_, body) = mcp_tool_call(&mut probe_srv, (idx + 10) as u64, name, json!({}));
        let msg = body
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if msg.starts_with("unknown tool:") {
            unknown_tool_count += 1;
        }
    }

    report.check(Check::absolute(
        "Advertised MCP tools in TOOLS_JSON",
        34.0,
        tool_count as f64,
        "tools",
        0.0,
    ));
    report.check(Check::absolute(
        "Advertised MCP tools failing dispatch with 'unknown tool'",
        0.0,
        unknown_tool_count as f64,
        "tools",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 2. Parse, round-trip, and execute every `# @mcp:` directive across examples/
    // -------------------------------------------------------------------------
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut fsm_files = Vec::new();
    collect_fsm_files(&examples_dir, &mut fsm_files);
    assert!(
        !fsm_files.is_empty(),
        "expected .fsm files under {}",
        examples_dir.display()
    );

    let mut parsed_ok = 0usize;
    let mut roundtrip_ok = 0usize;
    let mut mcp_directives_total = 0usize;
    let mut mcp_directives_failed = 0usize;
    let mut failure_notes: Vec<String> = Vec::new();

    #[derive(Default)]
    struct FileRunOutcome {
        parsed: bool,
        roundtripped: bool,
        directives_total: usize,
        directives_failed: usize,
        notes: Vec<String>,
    }

    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 6))
        .unwrap_or(4);
    let next_idx = std::sync::atomic::AtomicUsize::new(0);
    let outcomes: std::sync::Mutex<Vec<FileRunOutcome>> =
        std::sync::Mutex::new(Vec::with_capacity(fsm_files.len()));

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| loop {
                let idx = next_idx.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if idx >= fsm_files.len() {
                    break;
                }
                let path = &fsm_files[idx];
                let rel = path
                    .strip_prefix(&examples_dir)
                    .unwrap_or(path)
                    .display()
                    .to_string();
                let raw = std::fs::read_to_string(path)
                    .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
                let doc = fsm::parse(&raw).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
                let mut out = FileRunOutcome {
                    parsed: true,
                    ..Default::default()
                };

                let printed = fsm::print(&doc);
                let reparsed =
                    fsm::parse(&printed).unwrap_or_else(|e| panic!("reparse printed {rel}: {e}"));
                if reparsed.coupled_system == doc.coupled_system
                    && reparsed.asserts == doc.asserts
                    && reparsed.solve.plasticity == doc.solve.plasticity
                    && reparsed.solve.hardening_n == doc.solve.hardening_n
                    && reparsed.custom_materials.len() == doc.custom_materials.len()
                {
                    out.roundtripped = true;
                } else {
                    out.notes.push(format!("round-trip mismatch on {rel}"));
                }

                if let Some(header) = fsm::parse_literate_header(&raw) {
                    if !header.mcp_steps.is_empty() {
                        let mut srv = Server::new();
                        let (open_err, open_body) =
                            mcp_tool_call(&mut srv, 1, "open", json!({ "doc": raw.as_str() }));
                        assert!(!open_err, "open failed for {rel}: {open_body}");

                        for (step_idx, action) in header.mcp_steps.iter().enumerate() {
                            out.directives_total += 1;
                            let args: Value = match serde_json::from_str(&action.arguments_json) {
                                Ok(v) => v,
                                Err(e) => {
                                    out.directives_failed += 1;
                                    out.notes.push(format!(
                                        "{rel} step {} ({}) invalid JSON args: {e}",
                                        step_idx + 1,
                                        action.tool
                                    ));
                                    continue;
                                }
                            };
                            let (is_err, body) = mcp_tool_call(
                                &mut srv,
                                (step_idx + 2) as u64,
                                &action.tool,
                                args,
                            );
                            if is_err {
                                out.directives_failed += 1;
                                out.notes.push(format!(
                                    "{rel} step {} (`{}` — {}): {body}",
                                    step_idx + 1,
                                    action.tool,
                                    action.label
                                ));
                            }
                        }
                    }
                }
                outcomes.lock().expect("outcomes mutex").push(out);
            });
        }
    });

    for out in outcomes.into_inner().expect("outcomes into_inner") {
        if out.parsed {
            parsed_ok += 1;
        }
        if out.roundtripped {
            roundtrip_ok += 1;
        }
        mcp_directives_total += out.directives_total;
        mcp_directives_failed += out.directives_failed;
        failure_notes.extend(out.notes);
    }

    report.check(Check::absolute(
        "Example .fsm files parsed and round-tripped with typed fields intact",
        fsm_files.len() as f64,
        roundtrip_ok.min(parsed_ok) as f64,
        "files",
        0.0,
    ));
    report.check(Check::absolute(
        "Literate # @mcp: directives failing across examples/",
        0.0,
        mcp_directives_failed as f64,
        "directives",
        0.0,
    ));

    // Verify T8 fluid role & T19 J2 plasticity explicitly
    let t8 = fieldsmith_verify::example("case-studies/T8-float-bath-cvd-coater.fsm");
    assert_eq!(
        t8.coupled_system.as_deref(),
        Some("pilkington_float_bath_cvd_coater_v1")
    );
    let tin = t8
        .custom_materials
        .iter()
        .find(|m| m.id == "molten_tin")
        .expect("molten_tin in T8");
    assert_eq!(tin.role, MaterialRole::Fluid);
    let t8_structural = fieldsmith_core::structural_doc(&t8);
    let tin_node = t8_structural
        .root
        .iter()
        .find(|n| n.id == "tin_bath_pool")
        .expect("tin_bath_pool node in T8");
    assert!(
        tin_node.suppressed,
        "fluid volume tin_bath_pool should be suppressed in 3D solid FEA assembly in T8"
    );

    let t19 = fieldsmith_verify::example("tutorials/T19-sheet-metal-clinching-slock-nonlinear-fem.fsm");
    assert_eq!(t19.solve.plasticity, Some(PlasticityLaw::J2));
    assert_eq!(t19.solve.hardening_n, Some(0.20));

    // -------------------------------------------------------------------------
    // 3. API <-> MCP same-result parity for Verify (ok + refusal) & Sensitivity
    // -------------------------------------------------------------------------
    let contract_pass_fsm = "\
fieldsmith 1
name \"Contract Beam\"
material al7075
let L = 60
let b = 8
let d = 10
box beam size=(L, b, d) at=(L / 2, 0, 0)
region root = beam.nx
region tip = beam.px
support fix root fixed=xyz
load push tip force=(0, 0, -120)
assert \"Tip deflection\" metric=\"max_displacement\" target=\"<= 5.0\" tol=\"<= 5.0\" unit=\"mm\"
assert \"Safety factor\" metric=\"min_safety_factor\" target=\">= 1.2\" tol=\">= 1.2\" unit=\"\"
solve
";

    let api_verify_ok: Value = serde_json::from_str(
        &fieldsmith_api::respond(
            &json!({
                "apiv": 2,
                "doc": contract_pass_fsm,
                "question": { "kind": "verify", "dofs": 4000 }
            })
            .to_string(),
        )
        .body,
    )
    .expect("api verify ok json");
    assert_eq!(api_verify_ok["status"], "ok", "api_verify_ok={api_verify_ok}");
    assert_eq!(api_verify_ok["verification"]["all_passed"], true);
    assert_eq!(api_verify_ok["verification"]["passed"], 2);

    let mut verify_srv = Server::new();
    let (open_err, _) = mcp_tool_call(&mut verify_srv, 1, "open", json!({ "doc": contract_pass_fsm }));
    assert!(!open_err);
    let (mcp_verify_err, mcp_verify_ok) =
        mcp_tool_call(&mut verify_srv, 2, "verify", json!({ "dofs": 4000 }));
    assert!(!mcp_verify_err);
    assert_eq!(canon(&api_verify_ok), canon(&mcp_verify_ok));

    // Failing contract -> Status::Refusal, MCP isError == false
    let contract_fail_fsm = contract_pass_fsm.replace("<= 5.0", "<= 0.0001");
    let api_verify_refusal: Value = serde_json::from_str(
        &fieldsmith_api::respond(
            &json!({
                "apiv": 2,
                "doc": contract_fail_fsm,
                "question": { "kind": "verify", "dofs": 4000 }
            })
            .to_string(),
        )
        .body,
    )
    .expect("api verify refusal json");
    assert_eq!(api_verify_refusal["status"], "refusal");
    assert_eq!(api_verify_refusal["verification"]["all_passed"], false);
    assert_eq!(api_verify_refusal["verification"]["failed"], 1);

    let (open_err2, _) =
        mcp_tool_call(&mut verify_srv, 3, "open", json!({ "doc": contract_fail_fsm }));
    assert!(!open_err2);
    let (mcp_refusal_is_err, mcp_verify_refusal) =
        mcp_tool_call(&mut verify_srv, 4, "verify", json!({ "dofs": 4000 }));
    assert!(
        !mcp_refusal_is_err,
        "contract refusal is a structured answer, not an MCP tool error"
    );
    assert_eq!(canon(&api_verify_refusal), canon(&mcp_verify_refusal));

    // Sensitivity parity + beam scaling check
    let api_sens: Value = serde_json::from_str(
        &fieldsmith_api::respond(
            &json!({
                "apiv": 2,
                "doc": contract_pass_fsm,
                "question": { "kind": "sensitivity", "params": ["L", "b", "d"], "dofs": 5000 }
            })
            .to_string(),
        )
        .body,
    )
    .expect("api sensitivity json");
    assert_eq!(api_sens["status"], "ok");

    let (open_err3, _) =
        mcp_tool_call(&mut verify_srv, 5, "open", json!({ "doc": contract_pass_fsm }));
    assert!(!open_err3);
    let (mcp_sens_err, mcp_sens) = mcp_tool_call(
        &mut verify_srv,
        6,
        "sensitivity",
        json!({ "params": ["L", "b", "d"], "dofs": 5000 }),
    );
    assert!(!mcp_sens_err);
    assert_eq!(canon(&api_sens), canon(&mcp_sens));

    assert_eq!(
        api_sens["sensitivity"]["method"].as_str(),
        Some("finite_difference_remesh"),
        "SensitivityReport must state its finite_difference_remesh provenance"
    );
    let sens_rows = api_sens["sensitivity"]["parameters"]
        .as_array()
        .expect("parameters array");
    let sens_b = sens_rows
        .iter()
        .find(|r| r["param"] == "b")
        .expect("param b row");
    let rel_comp_b = sens_b["cases"][0]["rel_compliance"]
        .as_f64()
        .expect("rel_compliance for b");
    report.check(Check::relative(
        "3D sensitivity elasticity (b/C)(∂C/∂b) vs beam width scaling (-1.0)",
        -1.0,
        rel_comp_b,
        "elasticity",
        0.08,
    ));

    // -------------------------------------------------------------------------
    // 4. General 1D SDF Channel operator (`analyze_sdf_channel`) + BC binding
    // -------------------------------------------------------------------------
    let pipe_fsm = "\
fieldsmith 1
name \"Cooling Bore\"
material al7075
box block size=(28, 28, 80) at=(0, 0, 40)
cylinder bore r=5 h=84 at=(0, 0, 40) subtract
region bore_wall = bore.side
region mount = block.nz
support fix mount fixed=xyz
solve tref=20
";
    let (open_pipe_err, _) = mcp_tool_call(&mut verify_srv, 7, "open", json!({ "doc": pipe_fsm }));
    assert!(!open_pipe_err);
    // Laminar oil flow: Re = 4 * 0.05 / (pi * 0.01 * 0.02) = 318.3 (< 2300)
    // Hagen-Poiseuille Δp = 128 * μ * L * Q / (π * D^4) over span [5, 75] mm (L = 70 mm)
    let (chan_err, chan_val) = mcp_tool_call(
        &mut verify_srv,
        8,
        "analyze_sdf_channel",
        json!({
            "axis": "z",
            "stations": 9,
            "span_mm": [5.0, 75.0],
            "slice_h_mm": 0.35,
            "mass_flow_kg_s": 0.05,
            "density_kg_m3": 850.0,
            "viscosity_pa_s": 0.020,
            "conductivity_w_m_k": 0.14,
            "cp_j_kg_k": 2000.0,
            "inlet_pressure_mpa": 0.50,
            "inlet_temp_c": 30.0,
            "wall_temp_c": 90.0,
            "bind_region": "bore_wall",
        }),
    );
    assert!(!chan_err, "analyze_sdf_channel failed: {chan_val}");
    assert_eq!(chan_val["bound_region"], "bore_wall");
    let dh_mean = chan_val["report"]["mean_hydraulic_diameter_mm"]
        .as_f64()
        .expect("mean_hydraulic_diameter_mm");
    let dp_pa = chan_val["report"]["viscous_pressure_drop_mpa"]
        .as_f64()
        .expect("viscous_pressure_drop_mpa")
        * 1e6;
    let dp_exact_pa =
        128.0 * 0.020 * 0.070 * (0.05 / 850.0) / (std::f64::consts::PI * 0.010_f64.powi(4));

    report.check(Check::relative(
        "SDF sliced hydraulic diameter D_h on ⌀ 10 mm bore",
        10.0,
        dh_mean,
        "mm",
        0.02,
    ));
    report.check(Check::relative(
        "1D SDF channel laminar Δp vs Hagen-Poiseuille",
        dp_exact_pa,
        dp_pa,
        "Pa",
        0.03,
    ));

    let (_, inspected_pipe) = mcp_tool_call(&mut verify_srv, 9, "inspect", json!({}));
    assert_eq!(
        inspected_pipe["convections"].as_array().map(Vec::len),
        Some(1),
        "expected bound Convection BC on bore_wall"
    );

    // -------------------------------------------------------------------------
    // 5. HTTP 409 Conflict optimistic concurrency on /api/workspace & /api/mcp
    // -------------------------------------------------------------------------
    let tmp_ws = std::env::temp_dir().join(format!(
        "fieldsmith-v63-concurrency-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp_ws);
    let http_srv = spawn_http_server(ServeConfig {
        bind_addr: "127.0.0.1:0".to_string(),
        workspace_path: tmp_ws.clone(),
        web_dir: None,
    })
    .expect("spawn http server");
    let base_url = format!("http://127.0.0.1:{}", http_srv.local_addr().port());

    // Revision 0 -> 1 via human UI save
    let (c1, v1, _) = http_request_json(
        &base_url,
        "POST",
        "/api/workspace",
        Some(
            &json!({
                "expected_revision": 0,
                "actor": "web-ui",
                "summary": "Initial bracket load",
                "doc_fsm": contract_pass_fsm,
            })
            .to_string(),
        ),
        2000,
    )
    .expect("post rev 1");
    assert_eq!(c1, 200, "v1={v1}");
    assert_eq!(v1["revision"], 1);

    // Revision 1 -> 2 via AI MCP mutation (`set_let`)
    let (c2, v2, _) = http_request_json(
        &base_url,
        "POST",
        "/api/mcp",
        Some(
            &json!({
                "expected_revision": 1,
                "tool": "set_let",
                "arguments": { "name": "d", "value": 14.0 }
            })
            .to_string(),
        ),
        2000,
    )
    .expect("post rev 2");
    assert_eq!(c2, 200, "v2={v2}");
    assert_eq!(v2["workspace"]["revision"], 2);

    // Stale human UI write with expected_revision: 1 MUST be refused with 409 Conflict
    let (c_stale_ws, v_stale_ws, _) = http_request_json(
        &base_url,
        "POST",
        "/api/workspace",
        Some(
            &json!({
                "expected_revision": 1,
                "actor": "web-ui-stale",
                "doc_fsm": pipe_fsm,
            })
            .to_string(),
        ),
        2000,
    )
    .expect("post stale workspace");
    assert_eq!(v_stale_ws["error"], "workspace revision conflict");
    assert_eq!(v_stale_ws["current_revision"], 2);

    // Stale MCP write with expected_revision: 1 MUST also be refused with 409 Conflict
    let (c_stale_mcp, v_stale_mcp, _) = http_request_json(
        &base_url,
        "POST",
        "/api/mcp",
        Some(
            &json!({
                "expected_revision": 1,
                "tool": "set_let",
                "arguments": { "name": "d", "value": 6.0 }
            })
            .to_string(),
        ),
        2000,
    )
    .expect("post stale mcp");
    assert_eq!(v_stale_mcp["error"], "workspace revision conflict");

    // Verify workspace still preserves revision 2 (`let d = 14`)
    let (c_get, v_get, _) =
        http_request_json(&base_url, "GET", "/api/workspace", None, 2000).expect("get workspace");
    assert_eq!(c_get, 200);
    assert_eq!(v_get["revision"], 2);
    assert!(
        v_get["doc_fsm"]
            .as_str()
            .unwrap_or_default()
            .contains("let d = 14"),
        "expected revision 2 doc_fsm to retain `let d = 14`, got: {}",
        v_get["doc_fsm"]
    );

    let _ = std::fs::remove_file(&tmp_ws);

    report.check(Check::absolute(
        "HTTP status on stale POST /api/workspace (expected_revision=1 vs current=2)",
        409.0,
        c_stale_ws as f64,
        "HTTP",
        0.0,
    ));
    report.check(Check::absolute(
        "HTTP status on stale POST /api/mcp (expected_revision=1 vs current=2)",
        409.0,
        c_stale_mcp as f64,
        "HTTP",
        0.0,
    ));

    report.section(
        "Scope and measured invariants",
        format!(
            "Checked `{}` `.fsm` files across `examples/`, executing `{}` literate `# @mcp:` \
             directives in sequence through `fieldsmith_mcp::Server::handle` after `open`. \
             Verified 1:1 tool catalogue symmetry (`{}` advertised tools in `TOOLS_JSON`), \
             `apiv: 2` canon-byte parity between `fieldsmith_api::respond` and MCP `verify` / \
             `sensitivity`, general SDF cross-section channel hydraulics (`analyze_sdf_channel`), \
             and optimistic concurrency (`409 Conflict`) on `/api/workspace` and `/api/mcp`.",
            fsm_files.len(),
            mcp_directives_total,
            tool_count,
        ),
    );

    if !failure_notes.is_empty() {
        report.section("Failures observed", failure_notes.join("\n"));
    }

    report.section(
        "Named gaps",
        "1. Browser `Store` undo-preserving transaction sync (`workspace-sync.ts`) is verified \
         separately in `web/tests/v63-ui-lock.mjs`.\n\
         2. `QuestionSpec::Sensitivity` uses locked-`h` finite differences with re-stuffed \
         meshes (`method: \"finite_difference_remesh\"`, `2k + 1` solves), not fixed-topology \
         boundary-velocity adjoints.\n\
         3. `bind_channel_bcs_to_region` currently writes spatial-mean `h`, `T_∞`, and `p` onto \
         scalar `Convection` / `Pressure` BCs rather than station-varying spatial BC fields.\n\
         4. Parametric 2D sketches (`ParametricSketch`) still compile down to `ProfileVertex` \
         lists on `Extrude` / `Revolve` rather than persisting constraint graphs inside `.fsm`.",
    );

    report.finish();
}
