//! V63 — Unified Human ↔ AI Architecture Lock:
//! 1. 1:1 `TOOLS_JSON` ↔ `Server::tool` dispatch symmetry across all 46 MCP tools.
//! 2. Executable `# @mcp:` literate directives across every `.fsm` file in `examples/`.
//! 3. `apiv: 2` same-result envelope parity between `fieldsmith_api::respond` and
//!    `fieldsmith_mcp::Server::handle` for `QuestionSpec::Verify` (`ok` + `refusal`)
//!    and `QuestionSpec::Sensitivity`, plus `analyze_sdf_channel` accuracy & BC binding.
//! 4. Typed `.fsm` grammar round-trip (`coupled <id>`, `assert "..."`, `role=fluid`,
//!    `solve plasticity=j2 hardening_n=...`, `sketch <id> { ... }`, `sketchfeature ...`).
//! 5. HTTP `409 Conflict` optimistic concurrency (`expected_revision`) on `/api/workspace`
//!    and `/api/mcp`.
//! 6. Persistent parametric 2D `sketch` + `sketchfeature` driving 3D P2 FEA mass & bending
//!    stiffness under `DocCommand::SetLet` updates.

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
        let msg = body.get("text").and_then(Value::as_str).unwrap_or_default();
        if msg.starts_with("unknown tool:") {
            unknown_tool_count += 1;
        }
    }

    report.check(Check::absolute(
        "Advertised MCP tools in TOOLS_JSON",
        46.0,
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
                    && reparsed.uncertainties == doc.uncertainties
                    && reparsed.asserts == doc.asserts
                    && reparsed.solve.plasticity == doc.solve.plasticity
                    && reparsed.solve.hardening_n == doc.solve.hardening_n
                    && reparsed.custom_materials.len() == doc.custom_materials.len()
                    && reparsed.sketches == doc.sketches
                    && reparsed.sketch_features == doc.sketch_features
                    && reparsed.system_block == doc.system_block
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
                            let (is_err, body) =
                                mcp_tool_call(&mut srv, (step_idx + 2) as u64, &action.tool, args);
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

    let t19 =
        fieldsmith_verify::example("tutorials/T19-sheet-metal-clinching-slock-nonlinear-fem.fsm");
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
    assert_eq!(
        api_verify_ok["status"], "ok",
        "api_verify_ok={api_verify_ok}"
    );
    assert_eq!(api_verify_ok["verification"]["all_passed"], true);
    assert_eq!(api_verify_ok["verification"]["passed"], 2);

    let mut verify_srv = Server::new();
    let (open_err, _) = mcp_tool_call(
        &mut verify_srv,
        1,
        "open",
        json!({ "doc": contract_pass_fsm }),
    );
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

    let (open_err2, _) = mcp_tool_call(
        &mut verify_srv,
        3,
        "open",
        json!({ "doc": contract_fail_fsm }),
    );
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

    let (open_err3, _) = mcp_tool_call(
        &mut verify_srv,
        5,
        "open",
        json!({ "doc": contract_pass_fsm }),
    );
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
        Some("boundary_velocity_adjoint"),
        "SensitivityReport must state its boundary_velocity_adjoint provenance on linear-elastic solids"
    );
    assert_eq!(
        api_sens["sensitivity"]["solves_run"].as_u64(),
        Some(3),
        "3 parameters ['L', 'b', 'd'] must run in O(1) = 3 linear solves (1 forward + 2 adjoints), not 2k+1 = 7"
    );
    let sens_rows = api_sens["sensitivity"]["parameters"]
        .as_array()
        .expect("parameters array");
    let sens_l = sens_rows
        .iter()
        .find(|r| r["param"] == "L")
        .expect("param L row");
    let sens_b = sens_rows
        .iter()
        .find(|r| r["param"] == "b")
        .expect("param b row");
    let sens_d = sens_rows
        .iter()
        .find(|r| r["param"] == "d")
        .expect("param d row");
    let rel_comp_l = sens_l["cases"][0]["rel_compliance"]
        .as_f64()
        .expect("rel_compliance for L");
    let rel_comp_b = sens_b["cases"][0]["rel_compliance"]
        .as_f64()
        .expect("rel_compliance for b");
    let rel_comp_d = sens_d["cases"][0]["rel_compliance"]
        .as_f64()
        .expect("rel_compliance for d");
    report.check(Check::relative(
        "3D adjoint sensitivity elasticity (L/C)(∂C/∂L) vs cantilever span scaling (+2.93)",
        2.93,
        rel_comp_l,
        "elasticity",
        0.06,
    ));
    report.check(Check::relative(
        "3D adjoint sensitivity elasticity (b/C)(∂C/∂b) vs beam width scaling (-1.0)",
        -1.0,
        rel_comp_b,
        "elasticity",
        0.05,
    ));
    report.check(Check::relative(
        "3D adjoint sensitivity elasticity (d/C)(∂C/∂d) vs beam depth scaling (-2.93)",
        -2.93,
        rel_comp_d,
        "elasticity",
        0.06,
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

    // -------------------------------------------------------------------------
    // 6. Persistent `.fsm` sketch + sketchfeature + DocCommand::SetLet -> 3D P2 FEA
    // -------------------------------------------------------------------------
    let sketch_beam_fsm = "\
fieldsmith 1
name \"Parametric Sketch Cantilever\"
material ss316l
let beam_b = 12
let beam_h = 10
sketch beam_sec on=xy {
  point p0 at=(-6, 0)
  point p1 at=(6, 0)
  point p2 at=(6, 10)
  point p3 at=(-6, 10)
  segment s0 p1=p0 p2=p1
  segment s1 p1=p1 p2=p2
  segment s2 p1=p2 p2=p3
  segment s3 p1=p3 p2=p0
  constraint c_fix kind=fixed point=p0 at=(-6, 0)
  constraint c_h0 kind=horizontal segment=s0
  constraint c_h2 kind=horizontal segment=s2
  constraint c_v1 kind=vertical segment=s1
  constraint c_v3 kind=vertical segment=s3
  constraint c_w kind=distance segment=s0 value=12 expr=\"beam_b\"
  constraint c_d kind=distance segment=s1 value=10 expr=\"beam_h\"
}
sketchfeature beam_body sketch=beam_sec kind=boss_extrude h=60
region root_end = beam_body.bottom
region tip_end = beam_body.top
support fix root_end fixed=xyz
load tip_shear tip_end force=(0, -250, 0)
solve build=(0, 0)
";
    let mut sk_srv = Server::new();
    let (sk_open_err, sk_open_val) =
        mcp_tool_call(&mut sk_srv, 900, "open", json!({ "doc": sketch_beam_fsm }));
    assert!(!sk_open_err, "sketch beam open failed: {sk_open_val}");
    let (_, sk_txt_1) = mcp_tool_call(&mut sk_srv, 901, "doc_text", json!({}));
    let printed_sk = sk_txt_1["text"].as_str().expect("doc_text.text");
    let reparsed_sk = fsm::parse(printed_sk).expect("reparse printed sketch beam");
    assert_eq!(reparsed_sk.sketches.len(), 1);
    assert_eq!(reparsed_sk.sketch_features.len(), 1);
    assert_eq!(fsm::print(&reparsed_sk), printed_sk);

    let (solve10_err, solve10) = mcp_tool_call(
        &mut sk_srv,
        902,
        "solve",
        json!({ "kind": "solve", "h": 3.0 }),
    );
    assert!(!solve10_err, "solve at beam_h=10 failed: {solve10}");
    let mass_10 = solve10["single"]["peel"]["mass_g"]
        .as_f64()
        .expect("mass_10");
    let u_10 = solve10["single"]["cases"][0]["max_displacement"]
        .as_f64()
        .expect("u_10");

    let (cmd_err, cmd_out) = mcp_tool_call(
        &mut sk_srv,
        903,
        "apply_doc_command",
        json!({ "cmd": "set_let", "name": "beam_h", "value": 15.0 }),
    );
    assert!(!cmd_err, "apply_doc_command set_let failed: {cmd_out}");
    assert!(
        cmd_out["problems"].as_array().unwrap().is_empty(),
        "unexpected validation problems: {cmd_out}"
    );

    let (solve15_err, solve15) = mcp_tool_call(
        &mut sk_srv,
        904,
        "solve",
        json!({ "kind": "solve", "h": 3.0 }),
    );
    assert!(!solve15_err, "solve at beam_h=15 failed: {solve15}");
    let mass_15 = solve15["single"]["peel"]["mass_g"]
        .as_f64()
        .expect("mass_15");
    let u_15 = solve15["single"]["cases"][0]["max_displacement"]
        .as_f64()
        .expect("u_15");

    report.check(Check::relative(
        "Sketch-driven 3D mass ratio m(beam_h=15) / m(beam_h=10)",
        1.5,
        mass_15 / mass_10,
        "",
        0.04,
    ));
    // Euler-Bernoulli bending scales as (10/15)^3 = 0.2963; Timoshenko shear at L/h = 4..6 shifts ratio to ~0.315.
    report.check(Check::relative(
        "Sketch-driven 3D tip deflection ratio u(beam_h=15) / u(beam_h=10)",
        0.315,
        u_15 / u_10,
        "",
        0.12,
    ));

    // -------------------------------------------------------------------------
    // 7. Composable declarative 0D/1D `.fsm` `system` block -> Dual-AD + Adjoint + 3D FEA
    // -------------------------------------------------------------------------
    let decl_fsm = "\
fieldsmith 1
name \"Declarative Cooled Liner\"
material ss316l
let r_in = 6
let r_out = 10
let L_pipe = 40
cylinder wall r=r_out h=L_pipe at=(0, 0, L_pipe / 2)
cylinder bore r=r_in h=L_pipe + 2 at=(0, 0, L_pipe / 2) subtract
region mount = wall.bottom
region bore_wall = bore.side
region outer_wall = wall.side
support fix mount fixed=xyz
load bore_p bore_wall pressure=0.5 case=\"Operating\"
convection cool bore_wall h=0.004 tinf=25 case=\"Operating\"
heatflux ext_q outer_wall q=0.05 case=\"Operating\"
solve tref=25
system pipe_1d title=\"1D Cooled Liner\" sector=\"Thermal-Fluids\" nodes=31 span=(0, 40) axis=z {
  design mdot nominal=0.035 min=0.015 max=0.12 unit=\"kg/s\" symbol=\"m_dot\" category=process
  design p_in nominal=1.6 min=0.6 max=3.5 unit=\"MPa\" symbol=\"P_in\" category=process
  epistemic h_mult nominal=1.0 sigma=0.10 min=0.7 max=1.3 unit=\"-\" symbol=\"k_h\"
  state T_f init=\"25\" scale=50 unit=\"C\" symbol=\"T_f\"
  state P_f init=\"p_in\" scale=1 unit=\"MPa\" symbol=\"P_f\"
  aux cp expr=\"4180\"
  aux h_film expr=\"h_mult * 0.008 * pow(mdot / 0.05, 0.8)\"
  aux q_lin expr=\"25.0 * (1 + 0.25 * sin(pi * s / L_pipe))\"
  residual T_f eq=\"mdot * cp * d_ds(T_f) - q_lin\" bc_left=\"T_f - 25\"
  residual P_f eq=\"d_ds(P_f) + 0.0035 * pow(mdot / 0.05, 1.8)\" bc_left=\"P_f - p_in\"
  objective mdot_min expr=\"mdot\" reduce=value kind=minimize unit=\"kg/s\" symbol=\"m_dot\"
  objective t_wall_max expr=\"T_f + q_lin / (2 * pi * r_in * h_film)\" reduce=max kind=upper limit=95 unit=\"C\" symbol=\"T_w,max\"
  audit enthalpy inflow=\"mdot * cp * entry(T_f)\" outflow=\"mdot * cp * exit(T_f)\" source=\"integral(q_lin)\" tol=0.02 unit=\"W\"
  bind cool attr=h expr=\"h_film\"
  bind cool attr=tinf expr=\"T_f\"
  bind bore_p attr=pressure expr=\"P_f\"
}
";
    let decl_doc = fsm::parse(decl_fsm).expect("parse declarative system .fsm");
    let decl_roundtrip =
        fsm::parse(&fsm::print(&decl_doc)).expect("reparse declarative system .fsm");
    assert_eq!(decl_roundtrip.system_block, decl_doc.system_block);

    let mut decl_srv = Server::new();
    let (d_open_err, d_open) =
        mcp_tool_call(&mut decl_srv, 950, "open", json!({ "doc": decl_fsm }));
    assert!(!d_open_err, "open decl_fsm failed: {d_open}");

    // Verify standalone 3D staggered thermo-elastic adjoint sensitivity on untouched `decl_fsm`
    // (automatically solves the 1D `system` equilibrium and differentiates 3D `T_max` & `σ_vM` in O(1) solves!)
    let (d_sens_err, d_sens) = mcp_tool_call(
        &mut decl_srv,
        949,
        "sensitivity",
        json!({ "params": ["mdot", "p_in", "r_out"], "h": 4.5 }),
    );
    assert!(
        !d_sens_err,
        "standalone 1D->3D adjoint sensitivity failed: {d_sens}"
    );
    assert_eq!(d_sens["sensitivity"]["method"], "boundary_velocity_adjoint");
    // 2 baseline cases (1 mechanical + 1 thermal) + 2 mechanical adjoints + 6 thermal adjoints = 10 solves across 3 params
    assert_eq!(d_sens["sensitivity"]["solves_run"], 10);
    let dt_dmdot = d_sens["sensitivity"]["parameters"][0]["cases"][1]["d_max_temp"]
        .as_f64()
        .expect("d_max_temp wrt mdot");
    assert!(
        dt_dmdot < 0.0,
        "expected ∂T_max,3D / ∂mdot < 0 from automatic 1D->3D staggered adjoint, got {dt_dmdot}"
    );

    let (d_solve_err, d_solve) =
        mcp_tool_call(&mut decl_srv, 951, "solve_coupled_system", json!({}));
    assert!(
        !d_solve_err,
        "solve_coupled_system decl_fsm failed: {d_solve}"
    );
    let d_analysis = &d_solve["analysis"];
    let d_fd_err = d_analysis["directional_fd_jacobian_rel_err"]
        .as_f64()
        .expect("directional_fd_jacobian_rel_err");
    let d_adj_err = d_analysis["adjoint_vs_tangent_max_rel_err"]
        .as_f64()
        .expect("adjoint_vs_tangent_max_rel_err");
    let d_audit_err = d_analysis["conservation_audits"][0]["relative_error"]
        .as_f64()
        .expect("conservation relative_error");
    let tw_before = d_analysis["objectives"][1]
        .as_f64()
        .expect("T_w,max before");
    assert!(
        tw_before > 95.0,
        "expected initial T_w,max > 95 C, got {tw_before}"
    );

    report.check(Check::absolute(
        "Declarative .fsm Dual-AD Jacobian vs directional FD rel err",
        0.0,
        d_fd_err,
        "",
        1e-6,
    ));
    report.check(Check::absolute(
        "Declarative .fsm Adjoint vs Tangent sensitivity rel err",
        0.0,
        d_adj_err,
        "",
        1e-6,
    ));
    report.check(Check::absolute(
        "Declarative .fsm 1D enthalpy conservation audit rel err",
        0.0,
        d_audit_err,
        "",
        0.02,
    ));

    let (d_opt_err, d_opt) = mcp_tool_call(
        &mut decl_srv,
        952,
        "optimise_coupled_system",
        json!({ "steps": 6 }),
    );
    assert!(
        !d_opt_err,
        "optimise_coupled_system decl_fsm failed: {d_opt}"
    );
    let tw_after = d_opt["final_objectives"][1]
        .as_f64()
        .expect("T_w,max after");
    assert!(
        tw_after < tw_before - 10.0,
        "expected Adjoint optimisation to reduce peak wall temp by > 10 C: {tw_before} -> {tw_after}"
    );
    report.check(Check::absolute(
        "Adjoint-optimised declarative 1D peak wall temp reduction (> 10 °C)",
        0.0,
        (tw_after - (tw_before - 10.0)).max(0.0),
        "°C",
        0.0,
    ));

    let (_, d_insp) = mcp_tool_call(&mut decl_srv, 953, "inspect", json!({}));
    assert!(
        d_insp["convections"][0]["t_inf_expr"]
            .as_str()
            .unwrap_or_default()
            .starts_with("lerp(z,"),
        "expected synced lerp(z, ...) t_inf_expr on 3D Convection BC: {d_insp}"
    );
    let (d_fea_err, d_fea) = mcp_tool_call(
        &mut decl_srv,
        954,
        "solve",
        json!({ "kind": "solve", "h": 4.0 }),
    );
    assert!(
        !d_fea_err,
        "3D FEA solve after 1D declarative BC projection failed: {d_fea}"
    );
    let d_vm = d_fea["single"]["cases"][0]["max_von_mises"]
        .as_f64()
        .expect("max_von_mises");
    assert!(
        d_vm > 0.5 && d_vm.is_finite(),
        "expected positive 3D thermo-elastic stress, got {d_vm}"
    );

    // -------------------------------------------------------------------------
    // 8. Contract-Constrained 3D Adjoint Optimiser (`optimise`) API <-> MCP Parity
    // -------------------------------------------------------------------------
    let overstressed_fsm = "\
fieldsmith 1
name \"Overstressed V63 Beam\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let b = 10
let d = 5
let tip_f = -300
box beam size=(60, b, d) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, tip_f)
assert \"Peak stress limit\" metric=\"max_vm_elem\" target=\"<= 135\" unit=\"MPa\"
";
    let opt_api_req = json!({
        "apiv": 2,
        "doc": overstressed_fsm,
        "question": {
            "kind": "optimise",
            "objective": "mass_g",
            "params": ["d"],
            "bounds": { "d": [4.0, 16.0] },
            "max_iters": 6,
            "dofs": 4500,
        }
    });
    let opt_api_rep = fieldsmith_api::respond(&opt_api_req.to_string());
    assert!(
        !opt_api_rep.is_error,
        "API optimise returned error: {}",
        opt_api_rep.body
    );
    let opt_api_json: Value = serde_json::from_str(&opt_api_rep.body).expect("opt_api_json");
    assert_eq!(opt_api_json["status"], "ok");

    let mut opt_srv = Server::new();
    let (o_open_err, _) = mcp_tool_call(
        &mut opt_srv,
        960,
        "open",
        json!({ "doc": overstressed_fsm }),
    );
    assert!(!o_open_err);
    let (o_mcp_err, opt_mcp_json) = mcp_tool_call(
        &mut opt_srv,
        961,
        "optimise",
        json!({
            "objective": "mass_g",
            "params": ["d"],
            "bounds": { "d": [4.0, 16.0] },
            "max_iters": 6,
            "dofs": 4500,
            "apply": true,
        }),
    );
    assert!(!o_mcp_err, "MCP optimise failed: {opt_mcp_json}");
    assert_eq!(
        canon(&opt_api_json),
        canon(&opt_mcp_json),
        "API vs MCP `optimise` envelope mismatch"
    );
    assert_eq!(
        opt_mcp_json["optimisation"]["initial_verification"]["all_passed"],
        false
    );
    assert_eq!(
        opt_mcp_json["optimisation"]["final_verification"]["all_passed"],
        true
    );
    let (o_ver_err, o_ver_after) =
        mcp_tool_call(&mut opt_srv, 962, "verify", json!({ "dofs": 4500 }));
    assert!(!o_ver_err);
    assert_eq!(
        o_ver_after["status"], "ok",
        "expected open MCP session doc to satisfy contracts after `optimise`: {o_ver_after}"
    );
    report.check(Check::absolute(
        "3D adjoint contract optimiser failed assertions after repair",
        0.0,
        opt_mcp_json["optimisation"]["final_verification"]["failed"]
            .as_u64()
            .unwrap_or(99) as f64,
        "asserts",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 9. O(0)-Extra-Solve Rayleigh-Quotient Modal Sensitivities & Resonance Contract Optimiser
    // -------------------------------------------------------------------------
    let modal_cantilever_fsm = "\
fieldsmith 1
name \"V63 Modal Rayleigh Cantilever\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let L = 60
let b = 12
let d = 8
box beam size=(L, b, d) at=(L / 2, 0, 0)
region root = beam.nx
support clamp root fixed=xyz
solve modes=2
";
    let mut mod_srv = Server::new();
    let (m_open_err, _) = mcp_tool_call(
        &mut mod_srv,
        970,
        "open",
        json!({ "doc": modal_cantilever_fsm }),
    );
    assert!(!m_open_err);
    let (m_sens_err, m_sens) = mcp_tool_call(
        &mut mod_srv,
        971,
        "sensitivity",
        json!({ "params": ["L", "b", "d"], "dofs": 9000 }),
    );
    assert!(!m_sens_err, "modal sensitivity failed: {m_sens}");
    assert_eq!(m_sens["sensitivity"]["method"], "boundary_velocity_adjoint");
    // Pure modal solve: 1 baseline modal run + 0 extra linear/eigen solves across 3 geometric parameters!
    assert_eq!(m_sens["sensitivity"]["solves_run"], 1);
    let m_params = m_sens["sensitivity"]["parameters"]
        .as_array()
        .expect("modal sensitivity parameters");
    let find_rel_f1 = |pname: &str| -> f64 {
        m_params
            .iter()
            .find(|p| p["param"].as_str() == Some(pname))
            .and_then(|p| p["rel_modes"][0].as_f64())
            .unwrap_or(f64::NAN)
    };
    let rel_f1_l = find_rel_f1("L");
    let rel_f1_b = find_rel_f1("b");
    let rel_f1_d = find_rel_f1("d");
    assert!(
        rel_f1_b.abs() < 0.05,
        "expected |(b/f1)(∂f1/∂b)| < 0.05, got {rel_f1_b}"
    );
    report.check(Check::relative(
        "3D Rayleigh-quotient modal elasticity (d/f₁)(∂f₁/∂d) vs beam depth scaling (+0.97)",
        0.97,
        rel_f1_d,
        "elasticity",
        0.06,
    ));
    report.check(Check::relative(
        "3D Rayleigh-quotient modal elasticity (L/f₁)(∂f₁/∂L) vs cantilever span scaling (-1.96)",
        -1.96,
        rel_f1_l,
        "elasticity",
        0.06,
    ));

    // Verify modal-contract-constrained `optimise` without explicit `solve modes` (auto-inferred from `mode_1`!)
    let modal_opt_fsm = "\
fieldsmith 1
name \"V63 Resonance Repair Beam\"
custommaterial steel200 e=200000 nu=0.30 yield=250 density=7.85
material steel200
let b = 12
let d = 4.5
box beam size=(60, b, d) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -200)
assert \"Fundamental resonance floor\" metric=\"mode_1\" target=\">= 1650\" unit=\"Hz\"
assert \"Peak stress limit\" metric=\"max_vm_elem\" target=\"<= 140\" unit=\"MPa\"
";
    let (mo_open_err, _) =
        mcp_tool_call(&mut mod_srv, 972, "open", json!({ "doc": modal_opt_fsm }));
    assert!(!mo_open_err);
    let (mo_opt_err, mo_opt) = mcp_tool_call(
        &mut mod_srv,
        973,
        "optimise",
        json!({
            "objective": "mass_g",
            "params": ["b", "d"],
            "bounds": { "b": [5.0, 16.0], "d": [4.0, 16.0] },
            "max_iters": 8,
            "dofs": 4500,
            "apply": true,
        }),
    );
    assert!(!mo_opt_err, "modal contract optimise failed: {mo_opt}");
    assert_eq!(
        mo_opt["optimisation"]["initial_verification"]["all_passed"],
        false
    );
    assert_eq!(
        mo_opt["optimisation"]["final_verification"]["all_passed"],
        true
    );

    // -------------------------------------------------------------------------
    // 10. O(1) Two-Pass Graded TPMS Adjoint Sensitivities & Simultaneous 3D Shape + Lattice Co-Optimisation
    // -------------------------------------------------------------------------
    let graded_codesign_fsm = "\
fieldsmith 1
name \"V63 Graded Gyroid Co-Design Beam\"
material ss316l
let d = 7.5
box beam size=(60, 12, d) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -280)
fill gyroid sheet cell=6 rho=(0.16, 0.26) skin=0.65 wall=0.4 margin=1 curve=1.2
assert \"Graded tip deflection\" metric=\"graded.deflection\" target=\"<= 0.30\" unit=\"mm\"
assert \"Lattice yield utilization\" metric=\"graded.yield_utilization\" target=\"<= 0.80\"
assert \"Mass savings vs solid\" metric=\"mass_saved_pct\" target=\">= 25\" unit=\"%\"
";
    let mut gr_srv = Server::new();
    let (gr_open_err, _) = mcp_tool_call(
        &mut gr_srv,
        980,
        "open",
        json!({ "doc": graded_codesign_fsm }),
    );
    assert!(!gr_open_err);

    let (gr_sens_err, gr_sens) = mcp_tool_call(
        &mut gr_srv,
        981,
        "sensitivity",
        json!({
            "params": ["d", "fill.rho_min", "fill.rho_max", "fill.skin", "fill.cell"],
            "dofs": 4500
        }),
    );
    assert!(!gr_sens_err, "graded TPMS sensitivity failed: {gr_sens}");
    assert_eq!(
        gr_sens["sensitivity"]["method"],
        "boundary_velocity_adjoint"
    );
    let gr_solves = gr_sens["sensitivity"]["solves_run"]
        .as_u64()
        .expect("graded solves_run");
    // 2 baseline solves (solid + graded) + 4 graded adjoint RHS + 1 solid stress-pass adjoint RHS = 6 solves across 5 params!
    assert!(
        gr_solves <= 7,
        "expected O(1) <= 7 linear solves across 5 shape+fill parameters, got {gr_solves}"
    );
    report.check(Check::absolute(
        "Graded TPMS discrete adjoint solve count across 5 shape + lattice parameters (<= 7)",
        6.0,
        gr_solves as f64,
        "solves",
        1.0,
    ));

    let gr_params = gr_sens["sensitivity"]["parameters"]
        .as_array()
        .expect("graded sensitivity parameters");
    let find_gr_param = |pname: &str| -> &Value {
        gr_params
            .iter()
            .find(|p| p["param"].as_str() == Some(pname))
            .unwrap_or_else(|| panic!("missing graded param {pname}"))
    };
    let d_uy_drho_max = find_gr_param("fill.rho_max")["cases"][0]["d_yield_utilization"]
        .as_f64()
        .expect("d_yield_utilization wrt fill.rho_max");
    let d_mass_dskin = find_gr_param("fill.skin")["d_mass_g"]
        .as_f64()
        .expect("d_mass_g wrt fill.skin");
    assert!(
        d_uy_drho_max < 0.0,
        "expected ∂U_yield / ∂fill.rho_max < 0, got {d_uy_drho_max}"
    );
    assert!(
        d_mass_dskin > 0.0,
        "expected ∂m_graded / ∂fill.skin > 0, got {d_mass_dskin}"
    );

    let gr_opt_args = json!({
        "objective": "mass_g",
        "params": ["d", "fill.rho_min", "fill.rho_max", "fill.skin"],
        "bounds": {
            "d": [6.5, 15.0],
            "fill.rho_min": [0.14, 0.55],
            "fill.rho_max": [0.22, 0.80],
            "fill.skin": [0.5, 2.4]
        },
        "max_iters": 8,
        "dofs": 4000,
    });
    let gr_api_req = json!({
        "apiv": 2,
        "doc": graded_codesign_fsm,
        "question": {
            "kind": "optimise",
            "objective": "mass_g",
            "params": ["d", "fill.rho_min", "fill.rho_max", "fill.skin"],
            "bounds": {
                "d": [6.5, 15.0],
                "fill.rho_min": [0.14, 0.55],
                "fill.rho_max": [0.22, 0.80],
                "fill.skin": [0.5, 2.4]
            },
            "max_iters": 8,
            "dofs": 4000,
        }
    });
    let gr_api_rep = fieldsmith_api::respond(&gr_api_req.to_string());
    assert!(
        !gr_api_rep.is_error,
        "API graded co-optimise failed: {}",
        gr_api_rep.body
    );
    let gr_api_json: Value = serde_json::from_str(&gr_api_rep.body).expect("gr_api_json");

    let mut gr_mcp_args = gr_opt_args;
    gr_mcp_args["apply"] = json!(true);
    let (gr_opt_err, gr_mcp_json) = mcp_tool_call(&mut gr_srv, 982, "optimise", gr_mcp_args);
    assert!(!gr_opt_err, "MCP graded co-optimise failed: {gr_mcp_json}");
    assert_eq!(
        canon(&gr_api_json),
        canon(&gr_mcp_json),
        "API vs MCP graded TPMS co-optimise envelope mismatch"
    );
    assert_eq!(
        gr_mcp_json["optimisation"]["sensitivity_method"],
        "boundary_velocity_adjoint"
    );
    assert_eq!(
        gr_mcp_json["optimisation"]["initial_verification"]["all_passed"],
        false
    );
    assert_eq!(
        gr_mcp_json["optimisation"]["final_verification"]["all_passed"],
        true
    );
    let (gr_ver_err, gr_ver_after) =
        mcp_tool_call(&mut gr_srv, 983, "verify", json!({ "dofs": 4000 }));
    assert!(!gr_ver_err);
    assert_eq!(
        gr_ver_after["status"], "ok",
        "expected co-optimised graded TPMS session doc to pass `verify`: {gr_ver_after}"
    );
    report.check(Check::absolute(
        "Co-optimised 3D outer shape + graded TPMS lattice failed assertions after repair",
        0.0,
        gr_mcp_json["optimisation"]["final_verification"]["failed"]
            .as_u64()
            .unwrap_or(99) as f64,
        "asserts",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 11. Additive Build-Orientation (`solve.build.*`) O(0)-Extra-Solve Peel/Overhang Sensitivities & Shape+Orientation Co-Optimisation
    // -------------------------------------------------------------------------
    let am_fdm_fsm = "\
fieldsmith 1
name \"V63 Upright FDM Cantilever Synthesis\"
material petg
let h_root = 8
box beam size=(60, 14, h_root) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -240)
solve build=(90, 0)
assert \"Inter-layer peel limit\" metric=\"max_layer_peel\" target=\"<= 0.65\"
assert \"In-plane stress limit\" metric=\"max_vm_elem\" target=\"<= 32\" unit=\"MPa\"
";
    let mut am_srv = Server::new();
    let (am_open_err, _) = mcp_tool_call(&mut am_srv, 990, "open", json!({ "doc": am_fdm_fsm }));
    assert!(!am_open_err);

    // Pure `solve.build.*` differentiation runs in 1 primal solve + 0 adjoint solves!
    let (am_pure_err, am_pure_sens) = mcp_tool_call(
        &mut am_srv,
        991,
        "sensitivity",
        json!({
            "params": ["solve.build.0", "solve.build.1"],
            "dofs": 4500
        }),
    );
    assert!(
        !am_pure_err,
        "pure solve.build sensitivity failed: {am_pure_sens}"
    );
    assert_eq!(
        am_pure_sens["sensitivity"]["method"],
        "boundary_velocity_adjoint"
    );
    let am_pure_solves = am_pure_sens["sensitivity"]["solves_run"]
        .as_u64()
        .expect("am pure solves_run");
    assert_eq!(
        am_pure_solves, 1,
        "expected O(0)-extra-solve pure build sensitivity (solves_run == 1), got {am_pure_solves}"
    );
    report.check(Check::absolute(
        "Pure additive build-orientation (`solve.build.0`, `solve.build.1`) sensitivity FEA solve count",
        1.0,
        am_pure_solves as f64,
        "solves",
        0.0,
    ));

    let am_api_req = json!({
        "apiv": 2,
        "doc": am_fdm_fsm,
        "question": {
            "kind": "optimise",
            "objective": "mass_g",
            "params": ["h_root", "solve.build.0"],
            "bounds": { "h_root": [7.0, 20.0] },
            "max_iters": 8,
            "dofs": 4500,
        }
    });
    let am_api_rep = fieldsmith_api::respond(&am_api_req.to_string());
    assert!(
        !am_api_rep.is_error,
        "API FDM shape+orientation optimise failed: {}",
        am_api_rep.body
    );
    let am_api_json: Value = serde_json::from_str(&am_api_rep.body).expect("am_api_json");

    let (am_opt_err, am_mcp_json) = mcp_tool_call(
        &mut am_srv,
        992,
        "optimise",
        json!({
            "objective": "mass_g",
            "params": ["h_root", "solve.build.0"],
            "bounds": { "h_root": [7.0, 20.0] },
            "max_iters": 8,
            "dofs": 4500,
            "apply": true,
        }),
    );
    assert!(
        !am_opt_err,
        "MCP FDM shape+orientation optimise failed: {am_mcp_json}"
    );
    assert_eq!(
        canon(&am_api_json),
        canon(&am_mcp_json),
        "API vs MCP additive FDM shape+orientation optimise envelope mismatch"
    );
    assert_eq!(
        am_mcp_json["optimisation"]["initial_verification"]["all_passed"],
        false
    );
    assert_eq!(
        am_mcp_json["optimisation"]["final_verification"]["all_passed"],
        true
    );
    let (am_ver_err, am_ver_after) =
        mcp_tool_call(&mut am_srv, 993, "verify", json!({ "dofs": 4500 }));
    assert!(!am_ver_err);
    assert_eq!(
        am_ver_after["status"], "ok",
        "expected rotated+sized FDM session doc to pass `verify`: {am_ver_after}"
    );
    report.check(Check::absolute(
        "Co-optimised additive build orientation + 3D shape failed assertions after repair",
        0.0,
        am_mcp_json["optimisation"]["final_verification"]["failed"]
            .as_u64()
            .unwrap_or(99) as f64,
        "asserts",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 12. Epistemic UQ (`QuestionSpec::Uq` / MCP `uq`) O(1)-Solve Propagation, API<->MCP Parity & Robust p90 Sizing
    // -------------------------------------------------------------------------
    let uq_fsm = "\
fieldsmith 1
name \"V63 Epistemic Cantilever\"
material ss316l
let d = 10
box beam size=(60, 12, d) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -550)
solve build=(0, 0)
uncertainty \"Depth tolerance\" param=\"d\" cv=0.03 unit=\"mm\"
uncertainty \"Tip force scatter\" param=\"push.force.2\" cv=0.08 unit=\"N\"
uncertainty \"Modulus scatter\" param=\"ss316l.e\" cv=0.05 unit=\"MPa\"
uncertainty \"Yield scatter\" param=\"ss316l.yield\" cv=0.06 unit=\"MPa\"
assert \"Robust p90 stress\" metric=\"p90.max_vm_elem\" target=\"<= 175\" unit=\"MPa\"
";
    let uq_api_req = json!({
        "apiv": 2,
        "doc": uq_fsm,
        "question": {
            "kind": "uq",
            "dofs": 4500,
        }
    });
    let uq_api_rep = fieldsmith_api::respond(&uq_api_req.to_string());
    assert!(!uq_api_rep.is_error, "API uq failed: {}", uq_api_rep.body);
    let uq_api_json: Value = serde_json::from_str(&uq_api_rep.body).expect("uq_api_json");

    let mut uq_srv = Server::new();
    let (uq_open_err, _) = mcp_tool_call(&mut uq_srv, 1000, "open", json!({ "doc": uq_fsm }));
    assert!(!uq_open_err);
    let (uq_mcp_err, uq_mcp_json) = mcp_tool_call(&mut uq_srv, 1001, "uq", json!({ "dofs": 4500 }));
    assert!(!uq_mcp_err, "MCP uq failed: {uq_mcp_json}");
    assert_eq!(
        canon(&uq_api_json),
        canon(&uq_mcp_json),
        "API vs MCP `uq` envelope mismatch"
    );

    let uq_solves = uq_mcp_json["uq"]["solves_run"]
        .as_u64()
        .expect("uq solves_run");
    // 1 primal solve + 3 mechanical adjoints (σ_vM, ‖u‖, and additive U_peel since build=(0,0) is set) = 4 solves,
    // strictly O(1) regardless of parameter count k. Verify k=2 and k=4 execute identical solve counts:
    let (uq2_err, uq2_json) = mcp_tool_call(
        &mut uq_srv,
        1005,
        "uq",
        json!({
            "dofs": 4500,
            "uncertainties": [
                { "label": "Depth tolerance", "param": "d", "cv": 0.03, "unit": "mm" },
                { "label": "Tip force scatter", "param": "push.force.2", "cv": 0.08, "unit": "N" }
            ]
        }),
    );
    assert!(!uq2_err, "MCP 2-param uq failed: {uq2_json}");
    let uq2_solves = uq2_json["uq"]["solves_run"]
        .as_u64()
        .expect("uq2 solves_run");
    assert_eq!(
        uq_solves, uq2_solves,
        "expected O(1) linear FEA solve count invariant in k (k=4 vs k=2)"
    );
    assert_eq!(
        uq_solves, 4,
        "expected 1 primal + 3 adjoint solves = 4 for build-enabled 3D cantilever, got {uq_solves}"
    );
    let uq_metrics = uq_mcp_json["uq"]["metrics"]
        .as_array()
        .expect("uq metrics array");
    let vm_m = uq_metrics
        .iter()
        .find(|m| m["metric"] == "max_vm_elem")
        .expect("max_vm_elem summary in uq");
    let vm_p50 = vm_m["p50"].as_f64().unwrap();
    let vm_std = vm_m["std_dev"].as_f64().unwrap();
    // Closed-form Euler-Bernoulli CV(σ) = sqrt((2 * CV_d)^2 + CV_F^2) = sqrt(0.06^2 + 0.08^2) = 0.10.
    let expected_cv_vm = ((2.0_f64 * 0.03).powi(2) + 0.08_f64.powi(2)).sqrt();
    report.check(Check::absolute(
        "4-parameter 3D epistemic UQ linear FEA solve count (1 primal + 3 adjoints, matching k=2)",
        4.0,
        uq_solves as f64,
        "solves",
        0.0,
    ));
    report.check(Check::relative(
        "3D FOSM adjoint stress CV (std_dev / p50) vs closed-form Gaussian propagation",
        expected_cv_vm,
        vm_std / vm_p50,
        "",
        0.03,
    ));

    // Compare robust p90.max_vm_elem <= 175 sizing vs deterministic max_vm_elem <= 175 sizing
    let (rob_opt_err, rob_opt_json) = mcp_tool_call(
        &mut uq_srv,
        1002,
        "optimise",
        json!({
            "objective": "mass_g",
            "params": ["d"],
            "bounds": { "d": [8.5, 16.0] },
            "max_iters": 7,
            "dofs": 4000,
            "apply": true,
        }),
    );
    assert!(
        !rob_opt_err,
        "MCP robust p90 optimise failed: {rob_opt_json}"
    );
    assert_eq!(
        rob_opt_json["optimisation"]["final_verification"]["all_passed"],
        true
    );
    let d_rob = rob_opt_json["optimisation"]["parameters"][0]["final_value"]
        .as_f64()
        .expect("d_rob");

    let det_fsm = uq_fsm.replace("p90.max_vm_elem", "max_vm_elem");
    let mut det_srv = Server::new();
    let (det_open_err, _) = mcp_tool_call(&mut det_srv, 1003, "open", json!({ "doc": det_fsm }));
    assert!(!det_open_err);
    let (det_opt_err, det_opt_json) = mcp_tool_call(
        &mut det_srv,
        1004,
        "optimise",
        json!({
            "objective": "mass_g",
            "params": ["d"],
            "bounds": { "d": [8.5, 16.0] },
            "max_iters": 7,
            "dofs": 4000,
            "apply": true,
        }),
    );
    assert!(
        !det_opt_err,
        "MCP deterministic p50 optimise failed: {det_opt_json}"
    );
    let d_det = det_opt_json["optimisation"]["parameters"][0]["final_value"]
        .as_f64()
        .expect("d_det");
    // Analytical ratio: d_rob / d_det ≈ sqrt(1 + z_0.90 * CV_σ) = sqrt(1 + 1.28155 * 0.10) = 1.0621.
    report.check(Check::relative(
        "Robust p90 vs deterministic p50 optimal cantilever depth ratio d_rob / d_det",
        (1.0 + 1.281_551_565_544_600_4 * expected_cv_vm).sqrt(),
        d_rob / d_det,
        "",
        0.025,
    ));

    // -------------------------------------------------------------------------
    // 13. Unified Fastener & Preloaded-Joint Mechanics (`QuestionSpec::Fixing`,
    //     `evaluate_fixing` API<->MCP canon parity, `DocCommand::InsertFixing` /
    //     `ReconfigureFixing`, and 3D 4-Bolt Flange ISO 16047 Preload UQ)
    // -------------------------------------------------------------------------
    let fixing_fsm = "\
fieldsmith 1
name \"V63 4-Bolt Al7075 Flange\"
material al7075
box flange size=(70, 50, 12) at=(35, 0, 6)
region root = flange.nx
region tip = flange.px
support clamp root fixed=xyz
load tip_shear tip force=(0, 0, -600)
solve build=(0, 0)
";
    let fixing_input = json!({
        "fixing_kind": "hex_cap_bolt",
        "nominal_d_mm": 5.0,
        "pitch_mm": 0.8,
        "engagement_length_mm": 10.0,
        "substrate_material": "al7075",
        "fastener_material": "steel_10_9",
        "include_washer": true,
        "associated_nut": "nyloc_nut",
        "mu_thread": 0.14,
        "mu_head": 0.14,
        "vibration_g": 12.0,
        "payload_mass_kg": 1.8,
    });
    let coupon_samples = json!([
        {"sample_id": "C1", "torque_nm": 7.5, "principal_stress_mpa": 420.0},
        {"sample_id": "C2", "torque_nm": 8.4, "principal_stress_mpa": 470.0},
        {"sample_id": "C3", "torque_nm": 9.1, "principal_stress_mpa": 510.0},
        {"sample_id": "C4", "torque_nm": 9.6, "principal_stress_mpa": 540.0},
    ]);
    let api_fixing_str = fieldsmith_api::respond(
        &json!({
            "apiv": 2,
            "doc": fixing_fsm,
            "question": {
                "kind": "fixing",
                "input": fixing_input.clone(),
                "samples": coupon_samples.clone(),
                "target_pf": 1e-3,
            }
        })
        .to_string(),
    );
    let api_fixing_resp: Value =
        serde_json::from_str(&api_fixing_str.body).expect("api fixing json");
    assert_eq!(
        api_fixing_resp["status"], "ok",
        "API fixing failed: {api_fixing_resp}"
    );

    let mut fix_srv = Server::new();
    let (fix_open_err, _) = mcp_tool_call(&mut fix_srv, 1100, "open", json!({ "doc": fixing_fsm }));
    assert!(!fix_open_err);
    let (mcp_fix_err, mcp_fix_json) = mcp_tool_call(
        &mut fix_srv,
        1101,
        "evaluate_fixing",
        json!({
            "input": fixing_input.clone(),
            "samples": coupon_samples,
            "target_pf": 1e-3,
        }),
    );
    assert!(!mcp_fix_err, "MCP evaluate_fixing failed: {mcp_fix_json}");
    assert_eq!(
        api_fixing_resp["canon"], mcp_fix_json["canon"],
        "API QuestionSpec::Fixing and MCP evaluate_fixing must produce byte-identical canon hashes"
    );

    let actual_k = mcp_fix_json["fixing"]["envelope"]["nut_factor_k"]
        .as_f64()
        .expect("nut_factor_k");
    let actual_arm = mcp_fix_json["fixing"]["envelope"]["motosh_torque_arm_mm"]
        .as_f64()
        .expect("motosh_torque_arm_mm");
    report.check(Check::relative(
        "Motosh (1976) / ISO 16047 dimensionless nut factor K = K_torque / d on M5x0.8",
        actual_arm / 5.0,
        actual_k,
        "",
        1e-9,
    ));

    let mut bare_input = fixing_input.clone();
    bare_input["include_washer"] = json!(false);
    let (bare_err, bare_json) = mcp_tool_call(
        &mut fix_srv,
        1102,
        "evaluate_fixing",
        json!({ "input": bare_input }),
    );
    assert!(!bare_err, "MCP bare evaluate_fixing failed: {bare_json}");
    let a_brg_washer = mcp_fix_json["fixing"]["envelope"]["bearing_area_mm2"]
        .as_f64()
        .expect("bearing_area_mm2 washer");
    let a_brg_bare = bare_json["fixing"]["envelope"]["bearing_area_mm2"]
        .as_f64()
        .expect("bearing_area_mm2 bare");
    let crush_washer = mcp_fix_json["fixing"]["envelope"]["mode_limits_nm"]
        ["underhead_bearing_crush_nm"]
        .as_f64()
        .expect("underhead_bearing_crush_nm washer");
    let crush_bare = bare_json["fixing"]["envelope"]["mode_limits_nm"]
        ["underhead_bearing_crush_nm"]
        .as_f64()
        .expect("underhead_bearing_crush_nm bare");
    assert!(
        a_brg_washer > 1.3 * a_brg_bare && crush_washer > 1.2 * crush_bare,
        "expected ISO 7089 washer to expand bearing area & bearing-crush torque capacity: A_brg({a_brg_washer} vs {a_brg_bare}), T_crush({crush_washer} vs {crush_bare})"
    );

    let (ins_err, ins_json) = mcp_tool_call(
        &mut fix_srv,
        1103,
        "insert_fixing",
        json!({
            "kind": "hex_cap_bolt",
            "size": "M5",
            "engagement_mm": 10.0,
            "host_id": "flange",
            "host_face": "top",
            "placement_mode": "flange_4_corners",
            "flange_inset_x_mm": 12.0,
            "flange_inset_y_mm": 10.0,
            "fastener_material": "steel_10_9",
            "include_washer": true,
            "associated_nut": "nyloc_nut",
            "preload_cv": 0.15,
            "vibration_g": 12.0,
            "payload_mass_kg": 1.8
        }),
    );
    assert!(!ins_err, "MCP insert_fixing failed: {ins_json}");
    assert_eq!(
        ins_json["loads"].as_u64(),
        Some(5),
        "expected tip_shear + 4 bolt preloads: {ins_json}"
    );
    assert_eq!(
        ins_json["uncertainties"].as_u64(),
        Some(1),
        "expected 1 preload UQ entry: {ins_json}"
    );
    let first_bolt_id = ins_json["fixing"]["first_new_id"]
        .as_str()
        .expect("first_new_id")
        .to_string();

    let (reconf_err, reconf_json) = mcp_tool_call(
        &mut fix_srv,
        1104,
        "reconfigure_fixing",
        json!({
            "selected_node_id": first_bolt_id,
            "include_washer": true,
            "associated_nut": "hex_nut",
            "fastener_material": "ti64"
        }),
    );
    assert!(!reconf_err, "MCP reconfigure_fixing failed: {reconf_json}");
    assert_eq!(reconf_json["fixing_stack"]["associated_nut"], "hex_nut");

    let (add_uq_err, _) = mcp_tool_call(
        &mut fix_srv,
        1105,
        "upsert_uncertainty",
        json!({
            "label": "Service shear scatter",
            "param": "tip_shear.force.2",
            "cv": 0.08,
            "unit": "N"
        }),
    );
    assert!(!add_uq_err);

    let (bolt_uq_err, bolt_uq_json) =
        mcp_tool_call(&mut fix_srv, 1106, "uq", json!({ "dofs": 14000 }));
    assert!(!bolt_uq_err, "MCP 4-bolt flange uq failed: {bolt_uq_json}");
    assert_eq!(
        bolt_uq_json["uq"]["method"], "boundary_velocity_adjoint",
        "expected boundary_velocity_adjoint on 4-bolt flange UQ: {bolt_uq_json}"
    );
    let bolt_uq_solves = bolt_uq_json["uq"]["solves_run"]
        .as_u64()
        .expect("bolt uq solves_run");
    // Two mechanical load cases (`default` 4-bolt preload + `tip_shear` external load) × 4 linear solves/case = 8 solves total across k=2 uncertainties.
    report.check(Check::absolute(
        "3D 4-bolt flange 2-case ISO 16047 preload + shear UQ linear FEA solves (4/case, O(1) in k)",
        8.0,
        bolt_uq_solves as f64,
        "solves",
        0.0,
    ));

    // -------------------------------------------------------------------------
    // 14. Unified Sheet-Metal Clinching / S-Lock API <-> MCP parity, exact
    //     Breda-Avitzur & Neuber-Hencky invariants, and 3D J2 co-design
    // -------------------------------------------------------------------------
    let a19_path = examples_dir.join("analytic/A19-sheet-metal-clinching-lee-kim-breda.fsm");
    let a19_raw = std::fs::read_to_string(&a19_path).expect("read A19 .fsm");
    let api_clinch_req = json!({
        "apiv": 2,
        "doc": a19_raw,
        "question": {
            "kind": "clinching"
        }
    });
    let api_clinch_reply = fieldsmith_api::respond(&api_clinch_req.to_string());
    let api_clinch_resp: Value =
        serde_json::from_str(&api_clinch_reply.body).expect("parse API clinching response");
    assert_eq!(
        api_clinch_resp["status"], "ok",
        "expected ok API clinching response: {api_clinch_resp}"
    );

    let mut clinch_srv = Server::new();
    let (cl_open_err, cl_open_body) =
        mcp_tool_call(&mut clinch_srv, 1201, "open", json!({ "doc": a19_raw }));
    assert!(!cl_open_err, "open A19 failed: {cl_open_body}");
    let (cl_eval_err, mcp_clinch_json) =
        mcp_tool_call(&mut clinch_srv, 1202, "evaluate_clinching", json!({}));
    assert!(
        !cl_eval_err,
        "MCP evaluate_clinching failed: {mcp_clinch_json}"
    );

    let clinch_mismatch = if canon(&api_clinch_resp) == canon(&mcp_clinch_json) {
        0.0
    } else {
        1.0
    };
    report.check(Check::absolute(
        "apiv:2 API <-> MCP canon parity on QuestionSpec::Clinching",
        0.0,
        clinch_mismatch,
        "mismatches",
        0.0,
    ));

    let cur_clinch = &mcp_clinch_json["clinching"]["current"];
    let rp_mm = 2.6_f64;
    let rd_mm = 2.6_f64 + 0.75_f64;
    let x_mm = 0.85_f64;
    let n_hard = 0.20_f64;
    let k_holl = 390.0_f64;
    let eps_coin = (3.0_f64 / x_mm).ln();
    let sigma_bar = (k_holl / (n_hard + 1.0)) * eps_coin.powf(n_hard);
    let m_shear = (3.0_f64.sqrt() * 0.18_f64).min(0.95);
    let slab_factor = 1.0
        + (2.0 * m_shear * rp_mm) / (3.0 * 3.0_f64.sqrt() * x_mm)
        + (2.0 / 3.0_f64.sqrt()) * (rd_mm / rp_mm).ln();
    let exact_fpunch_kn = std::f64::consts::PI * rp_mm * rp_mm * sigma_bar * slab_factor * 1e-3;
    let measured_fpunch_kn = cur_clinch["forming_force_kn"]
        .as_f64()
        .expect("forming_force_kn");
    report.check(Check::relative(
        "Breda-Avitzur axisymmetric clinching forming force F_punch(0.85)",
        exact_fpunch_kn,
        measured_fpunch_kn,
        "kN",
        1e-6,
    ));

    let measured_kt = cur_clinch["elastic_kt"].as_f64().expect("elastic_kt");
    let measured_ksigma = cur_clinch["plastic_ksigma"]
        .as_f64()
        .expect("plastic_ksigma");
    let exact_ksigma = measured_kt.powf((2.0 * n_hard) / (n_hard + 1.0));
    report.check(Check::relative(
        "Neuber-Hencky J2 clinch-neck stress redistribution K_sigma",
        exact_ksigma,
        measured_ksigma,
        "-",
        1e-6,
    ));

    let dc04_seed = "\
fieldsmith 1
name \"DC04 Sheet Stack\"
material mat_dc04
box sheet size=(40, 26, 3.0) at=(0, 0, 1.5)
region clamp_edges = sheet.nx + sheet.px
support fixture clamp_edges fixed=xyz
solve build=(0, 0)
";
    let mut stamp_srv = Server::new();
    let (st_open_err, _) = mcp_tool_call(&mut stamp_srv, 1203, "open", json!({ "doc": dc04_seed }));
    assert!(!st_open_err);
    let (st_ins_err, st_ins_json) = mcp_tool_call(
        &mut stamp_srv,
        1204,
        "apply_clinching",
        json!({
            "insert_joint": true,
            "host_id": "sheet",
            "bottom_thickness_x_mm": 1.25,
            "service_load_n": 2200.0,
            "j2_plasticity": true
        }),
    );
    assert!(
        !st_ins_err,
        "MCP apply_clinching stamp failed: {st_ins_json}"
    );
    assert_eq!(st_ins_json["clinching"]["j2_active"], true);

    let (st_dial_err, st_dial_json) = mcp_tool_call(
        &mut stamp_srv,
        1205,
        "apply_clinching",
        json!({
            "dial_optimal_x": true,
            "load_angle_deg": 45.0
        }),
    );
    assert!(
        !st_dial_err,
        "MCP apply_clinching dial failed: {st_dial_json}"
    );
    let x_dialed = st_dial_json["clinching"]["applied_x_mm"]
        .as_f64()
        .expect("applied_x_mm");
    let x_opt = st_dial_json["clinching"]["optimal_x_mm"]
        .as_f64()
        .expect("optimal_x_mm");
    assert!(
        (x_dialed - x_opt).abs() < 1e-6 && (0.70..=0.90).contains(&x_dialed),
        "expected X dialed to optimal X* in [0.70, 0.90]: applied={x_dialed}, opt={x_opt}"
    );

    let (st_doc_err, st_doc_json) = mcp_tool_call(&mut stamp_srv, 1206, "doc_text", json!({}));
    assert!(!st_doc_err);
    assert!(
        st_doc_json["text"]
            .as_str()
            .unwrap_or_default()
            .contains("plasticity=j2"),
        "expected printed .fsm to carry solve plasticity=j2: {st_doc_json}"
    );

    let (st_solve_err, st_solve_json) =
        mcp_tool_call(&mut stamp_srv, 1207, "solve", json!({ "dofs": 10000 }));
    assert!(
        !st_solve_err,
        "3D J2 solve on stamped clinch joint failed: {st_solve_json}"
    );
    let clinch_vm = st_solve_json["single"]["cases"][0]["max_vm_elem"]
        .as_f64()
        .expect("clinch max_vm_elem");
    assert!(
        clinch_vm > 20.0,
        "expected positive 3D J2 stress on clinch coupon, got {clinch_vm}"
    );

    report.section(
        "Scope and measured invariants",
        format!(
            "Checked `{}` `.fsm` files across `examples/`, executing `{}` literate `# @mcp:` \
             directives in sequence through `fieldsmith_mcp::Server::handle` after `open`. \
             Verified 1:1 tool catalogue symmetry (`{}` advertised tools in `TOOLS_JSON`), \
             `apiv: 2` canon-byte parity between `fieldsmith_api::respond` and MCP `verify` / \
             `sensitivity` / `uq` / `optimise` / `evaluate_fixing` / `evaluate_clinching` (`boundary_velocity_adjoint`, `O(1)` linear solves in parameter count `k` \
             across 3D solid, thermo-elastic, two-pass homogenised TPMS `fill.*`, additive `max_layer_peel` / \
             `overhang_fraction`, `O(0)`-extra-solve Rayleigh-quotient modal & `solve.build.*` sensitivities, \
             first-order second-moment `p10`/`p50`/`p90` epistemic uncertainty propagation + robust quantile sizing, \
             unified Motosh / FED-STD-H28 / VDI 2230 / Weibull fastener & preloaded-joint mechanics + host-associative 3D insertion, \
             and unified Lee–Kim / Breda / Varis–Mucha / Neuber–Hencky sheet-metal clinching process windows + parametric 3D S-Lock stamping & J2 co-design), \
             general SDF cross-section channel hydraulics (`analyze_sdf_channel`), \
             persistent `.fsm` `sketch` + `sketchfeature` round-trip & `DocCommand::SetLet` 3D FEA \
             stiffness scaling, composable 0D/1D `.fsm` `system` blocks with Dual-AD Jacobians & \
             `lerp(...)` 3D BC projection, and optimistic concurrency (`409 Conflict`) on `/api/workspace` and `/api/mcp`.",
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
        "1. Browser `Store` undo-preserving transaction sync (`workspace-sync.ts`), WASM \
         `SketchWorkbench` -> `DocCommand` dispatch, contract & uncertainty authoring (`upsert_assert` / `delete_assert` / `upsert_uncertainty` / `delete_uncertainty`), \
         WASM declarative `system` authoring (`set_system_block`), WASM `FixingsModal` / `Inspector` `insert_fixing` & `reconfigure_fixing`, \
         WASM `ClinchingCalculatorCard` `apply_clinching` (`dial_optimal_x`, `load_angle_deg`, `j2_plasticity`), and Results `Optimise` / `Compute UQ` / modal `∂f₁/∂p` / \
         graded TPMS `∂U_y/∂p` / additive `∂U_peel/∂p` co-design are verified separately in `web/tests/v63-ui-lock.mjs`.\n\
         2. Linear-elastic mechanical, coupled steady-conduction thermo-elastic, two-pass homogenised \
         TPMS `fill` (`fill.*` + outer shape `let`s), additive `max_layer_peel` / `overhang_fraction` \
         (`solve.build.*`), AND modal vibration cases (including spatial `lerp(...)` \
         BCs and embedded `.fsm` `system` blocks) use fixed-topology quadric boundary-velocity discrete \
         adjoints / Rayleigh quotients (`method: \"boundary_velocity_adjoint\"`, `O(1)` linear solves \
         independent of parameter count `k`); epistemic UQ (`uq`) uses first-order second-moment (FOSM) \
         independent Gaussian propagation around nominal (`p10`/`p50`/`p90` via `z = ±1.28155`); \
         nonlinear `J2` elastoplastic cases evaluate on the fixed-topology `V_k`-deformed mesh \
         (`method: \"boundary_velocity_fixed_mesh\"`), and discrete topology jumps fall back to \
         locked-`h` re-meshing (`method: \"finite_difference_remesh\"`).\n\
         3. Multi-dimensional specialized continuum twins (`float_bath` 2D VMS+MHD, `candle_flame`, \
         `antenna` NEC/VNA) remain compiled Rust solvers alongside composable 0D/1D `.fsm` `system` blocks.",
    );

    report.finish();
}
