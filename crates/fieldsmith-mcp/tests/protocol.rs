//! Protocol and tool round-trip tests for the MCP stdio server (M15 slice 2). Each test
//! drives the real binary over real stdio so framing (newline-delimited JSON-RPC, one
//! reply line per request, nothing for notifications) is exercised, not just the
//! message shapes. The same-result claims against the API envelope are pinned in the
//! verify crate's V22 golden; a light structural tie lives here too so a local failure
//! reads locally.

use fieldsmith_api::b64;
use fieldsmith_core::fsmz;
use fieldsmith_doc::fsm;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

const BRACKET: &str = include_str!("../../../examples/bracket.fsm");

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Client {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fieldsmith-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fieldsmith-mcp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        Client {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        }
    }

    /// One request or notification line in.
    fn send(&mut self, msg: &Value) {
        self.stdin
            .write_all((msg.to_string() + "\n").as_bytes())
            .expect("write request");
        self.stdin.flush().expect("flush request");
    }

    /// One JSON-RPC request that expects a reply, and the reply read back.
    fn ask(&mut self, id: u32, method: &str, params: Value) -> Value {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        self.next_reply()
    }

    fn next_reply(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read reply line");
        serde_json::from_str(line.trim()).expect("reply is one JSON object")
    }

    /// Call a tool by name and return the parsed payload of its first text content.
    /// Panics when the result carries `isError` — the error-path tests read their
    /// results directly.
    fn tool_ok(&mut self, id: u32, name: &str, args: Value) -> Value {
        self.send(&json!({
            "jsonrpc": "2.0", "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": args},
        }));
        let reply = self.next_reply();
        let result = &reply["result"];
        assert_ne!(
            result["isError"].as_bool(),
            Some(true),
            "tool {name} failed: {result}"
        );
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap())
            .unwrap_or_else(|_| result["content"][0]["text"].as_str().expect("text").into())
    }

    fn initialize(&mut self) -> Value {
        self.ask(1, "initialize", json!({"capabilities": {}}))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The canonical handshake: protocol version, tool capability, server info.
#[test]
fn initialize_replies_with_the_protocol_version_and_server_info() {
    let mut c = Client::spawn();
    let reply = c.initialize();
    assert_eq!(reply["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(reply["result"]["serverInfo"]["name"], "fieldsmith-mcp");
    assert!(reply["result"]["serverInfo"]["version"].is_string());
    assert!(reply["result"]["capabilities"]["tools"].is_object());
}

/// The catalogue is a real array of the ten names (MCP tools/list — a list of tool
/// objects, not a string of JSON).
#[test]
fn tools_list_is_an_array_of_the_ten_tools() {
    let mut c = Client::spawn();
    c.initialize();
    let reply = c.ask(2, "tools/list", json!({}));
    let tools = reply["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec![
            "open",
            "new_document",
            "inspect",
            "list_shapes",
            "upsert_shape",
            "insert_fixing",
            "reconfigure_fixing",
            "evaluate_fixing",
            "solve_sketch",
            "upsert_sketch",
            "apply_sketch_feature",
            "apply_doc_command",
            "delete_entity",
            "add_region",
            "add_support",
            "add_load",
            "add_temperature",
            "add_convection",
            "add_heatflux",
            "set_material",
            "upsert_custom_material",
            "set_let",
            "set_fill",
            "set_build",
            "upsert_assert",
            "delete_assert",
            "upsert_uncertainty",
            "delete_uncertainty",
            "doc_text",
            "solve",
            "verify",
            "sensitivity",
            "uq",
            "optimise",
            "analyze_sdf_channel",
            "export",
            "list_coupled_systems",
            "solve_coupled_system",
            "optimise_coupled_system",
            "solve_drone_antenna",
            "optimise_drone_antenna",
            "discover_compute_peers",
            "connect_compute_peer",
            "disconnect_compute_peer",
        ]
    );
    for t in tools {
        assert!(t["inputSchema"].is_object());
        assert!(t["description"].is_string());
    }
}

/// Unparseable bytes: -32700 with a null id.
#[test]
fn malformed_json_is_a_parse_error_with_null_id() {
    let mut c = Client::spawn();
    c.stdin.write_all(b"definitely not json\n").unwrap();
    c.stdin.flush().unwrap();
    let reply = c.next_reply();
    assert_eq!(reply["error"]["code"], -32700);
    assert!(reply["id"].is_null());
}

/// A method the server does not speak: -32601.
#[test]
fn unknown_method_is_method_not_found() {
    let mut c = Client::spawn();
    c.initialize();
    let reply = c.ask(9, "no/such/method", json!({}));
    assert_eq!(reply["error"]["code"], -32601);
    assert_eq!(reply["id"], 9);
}

/// A notification gets NO reply: after the silence there is exactly one line pending,
/// and it belongs to the following ping (both already sent) — framing in order.
#[test]
fn notification_produces_no_reply_and_the_next_request_still_works() {
    let mut c = Client::spawn();
    c.initialize();
    c.send(&json!({"jsonrpc": "2.0", "method": "notifications/foo"}));
    c.send(&json!({"jsonrpc": "2.0", "method": "notifications/bar"}));
    let reply = c.ask(7, "ping", json!({}));
    assert!(reply["result"].is_object());
    assert_eq!(reply["id"], 7);
}

/// tools/call for a name outside the catalogue: a tool answer with isError, not a
/// protocol error.
#[test]
fn unknown_tool_is_an_is_error_result() {
    let mut c = Client::spawn();
    c.initialize();
    c.send(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                   "params": {"name": "no_such"} }));
    let reply = c.next_reply();
    assert_eq!(reply["result"]["isError"], true);
    assert!(reply["error"].is_null());
    assert!(reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("no_such"));
}

/// Bad params on a method the server speaks: -32602.
#[test]
fn tools_call_without_a_name_is_invalid_params() {
    let mut c = Client::spawn();
    c.initialize();
    let reply = c.ask(4, "tools/call", json!({}));
    assert_eq!(reply["error"]["code"], -32602);
}

#[test]
fn tool_on_a_closed_session_is_an_is_error_answer() {
    let mut c = Client::spawn();
    c.initialize();
    c.send(&json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
                   "params": {"name": "solve", "arguments": {"kind": "solve", "dofs": 15000}} }));
    let reply = c.next_reply();
    assert_eq!(reply["result"]["isError"], true);
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("no document open"), "got: {text}");
    assert!(text.contains("open"));
}

/// The editor loop: open, add a region from a real face name, hang a support and a load
/// on it (fresh ids assigned server-side), read a .fsm print back that contains them,
/// and answer a solve question whose result ties the one-shot API's (element counts are
/// mesh-deterministic; only timing leaves move).
#[test]
fn open_edit_doc_text_solve_round_trip() {
    let mut c = Client::spawn();
    c.initialize();
    let opened = c.tool_ok(10, "open", json!({"doc": BRACKET}));
    assert_eq!(opened["name"], "Bracket");
    assert_eq!(opened["shapes"], 5);
    assert_eq!(opened["problems"], json!([]));

    // Box faces are named by axis (px..nz); `top`/`bottom`/`side` are the cylinder cap /
    // wall carriers, and a box-face support must sit on an axis plane (measured: base.nz
    // matches, base.bottom matches nothing and the engine refuses it — regions.rs's
    // carrier contract).
    let added = c.tool_ok(
        11,
        "add_region",
        json!({"selectors": [{"shape": "base", "face": "nz"}]}),
    );
    let region_id = added["added"].as_str().unwrap().to_string();
    assert!(region_id.starts_with("region"));

    c.tool_ok(
        12,
        "add_support",
        json!({"region": region_id.clone(), "fixed": [true, true, true]}),
    );
    c.tool_ok(
        13,
        "add_load",
        json!({"region": region_id, "kind": {"type": "force", "vector": [0, 0, -100]}}),
    );

    let text = c.tool_ok(14, "doc_text", json!({}))["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        text.contains(&region_id),
        "print must carry the added region"
    );
    assert!(
        text.contains("force=(0, 0, 100") || text.contains("force=(0, 0, -100"),
        "print must carry the added load"
    );

    let envelope: Value = c.tool_ok(15, "solve", json!({"kind": "solve", "dofs": 15000}));
    assert_eq!(envelope["status"], "ok");
    assert_eq!(envelope["name"], "Bracket");
    let prov = &envelope["single"]["provenance"];
    assert_eq!(prov["elements"], json!(3729));
    assert_eq!(prov["nodes"], json!(6744));
    assert_eq!(prov["dofs"], json!(20232));
}

/// `isError` mirrors the one-shot's exit code: a processing error (status error — here
/// a support region the coarse mesh cannot host) is a failed tool call whose message IS
/// the engine's error envelope; a refusal (the declared tolerance no rung can meet) is
/// an answer — exit 0 at the one-shot — and must NOT be marked isError.
#[test]
fn is_error_mirrors_the_cli_exit_code_and_refusals_are_answers() {
    let mut c = Client::spawn();
    c.initialize();
    c.tool_ok(20, "open", json!({"doc": BRACKET}));

    c.send(&json!({"jsonrpc": "2.0", "id": 21, "method": "tools/call",
                   "params": {"name": "solve", "arguments": {"kind": "solve", "dofs": 1500}} }));
    let reply = c.next_reply();
    assert_eq!(reply["result"]["isError"], true);
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let envelope: Value = serde_json::from_str(text).expect("the envelope IS the message");
    assert_eq!(envelope["status"], "error");
    assert!(
        envelope["error"].as_str().unwrap().contains("mount"),
        "got: {text}"
    );

    c.send(&json!({"jsonrpc": "2.0", "id": 22, "method": "tools/call",
                   "params": {"name": "solve",
                              "arguments": {"kind": "ladder", "rungs": [30000, 120000],
                                            "tolerance": "deflection:0.000001mm"}} }));
    let reply = c.next_reply();
    assert_ne!(
        reply["result"].get("isError").and_then(Value::as_bool),
        Some(true),
        "refusals are answers: {:?}",
        reply["result"]
    );
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let envelope: Value = serde_json::from_str(text).expect("the refusal envelope");
    assert_eq!(envelope["status"], "refusal");
    assert!(
        envelope
            .get("outcome")
            .map(|o| o["refusal"].is_string())
            .unwrap_or(false),
        "got: {text}"
    );
}

/// open over base64 .fsmz — the form a worker job carries — loads the same document.
#[test]
fn open_accepts_a_docz_state_and_sees_the_same_document() {
    let doc = fsm::parse(BRACKET).expect("fixture parses");
    let bytes = fsmz::save(&doc).expect("save fsmz");
    let docz = b64::encode(&bytes);

    let mut c = Client::spawn();
    c.initialize();
    let opened = c.tool_ok(30, "open", json!({"docz": docz}));
    assert_eq!(opened["name"], "Bracket");
    let envelope: Value = c.tool_ok(31, "solve", json!({"kind": "solve", "dofs": 15000}));
    assert_eq!(envelope["status"], "ok");
}

/// The export core through the tool: facts plus STL bytes whose triangle count matches
/// the facts, no file written anywhere.
#[test]
fn export_returns_facts_and_stl_bytes_that_agree() {
    let mut c = Client::spawn();
    c.initialize();
    c.tool_ok(40, "open", json!({"doc": BRACKET}));
    let result: Value = c.tool_ok(41, "export", json!({"h": 0.5, "stl": true}));
    let facts = &result["facts"];
    assert_eq!(facts["name"], "Bracket");
    assert_eq!(facts["h"], json!(0.5));
    assert!(facts["verts"].as_u64().unwrap() > 0);
    let tris = facts["tris"].as_u64().unwrap();
    let defects = &facts["defects"];
    // h 0.5 uniform dc over this solid has closed, manifold, oriented triangles.
    assert_eq!(defects["open_edges"], json!(0));
    assert_eq!(defects["orientation_defects"], json!(0));

    let stl = b64::decode(result["stl_b64"].as_str().unwrap()).expect("stl b64");
    let count = u32::from_le_bytes([stl[80], stl[81], stl[82], stl[83]]) as u64;
    assert_eq!(count, tris, "STL triangle count ties the facts");
    assert_eq!(
        stl.len(),
        (84 + 50 * tris) as usize,
        "binary STL is exactly 84 + 50/tri bytes"
    );
}

/// inspect reports the whole setup — including the dangling-edit problem list that
/// `validate` names and the loaded-document's grid state.
#[test]
fn inspect_reports_setup_validate_problems_and_missing_grids() {
    let mut c = Client::spawn();
    c.initialize();
    c.tool_ok(50, "open", json!({"doc": BRACKET}));
    let info = c.tool_ok(51, "inspect", json!({}));
    assert_eq!(info["material"], "petg");
    assert_eq!(info["regions"].as_array().unwrap().len(), 2);
    assert_eq!(info["supports"].as_array().unwrap().len(), 1);
    assert_eq!(info["missing_grids"], json!([]));
    assert_eq!(info["problems"], json!([]));

    // A support referencing no region makes validate speak through inspect.
    c.tool_ok(
        52,
        "add_support",
        json!({"region": "nowhere", "fixed": [true, false, false]}),
    );
    let info = c.tool_ok(53, "inspect", json!({}));
    let problems: Vec<String> = info["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().into())
        .collect();
    assert!(
        problems.iter().any(|p| p.contains("nowhere")),
        "got: {problems:?}"
    );
}

/// set_material reports unknown nodes as a tool error; known nodes land and show in the
/// print.
#[test]
fn set_material_errors_on_unknown_nodes_and_names_known_ones() {
    let mut c = Client::spawn();
    c.initialize();
    c.tool_ok(60, "open", json!({"doc": BRACKET}));

    c.send(&json!({"jsonrpc": "2.0", "id": 61, "method": "tools/call",
                   "params": {"name": "set_material", "arguments": {"node": "nope", "material": "pla"}} }));
    let reply = c.next_reply();
    assert_eq!(reply["result"]["isError"], true);

    let _ = c.tool_ok(
        62,
        "set_material",
        json!({"node": "base", "material": "pla"}),
    );
    let printed = c.tool_ok(63, "doc_text", json!({}));
    let text = printed["text"].as_str().unwrap();
    assert!(text.contains("material pla"), "got: {text}");
}

/// CAD primitive authoring (`list_shapes`, `upsert_shape`, `delete_entity`) and coupled
/// multi-physics synthesis (`list_coupled_systems`, `solve_coupled_system`,
/// `optimise_coupled_system`) round-trip through the MCP server.
#[test]
fn cad_authoring_and_coupled_synthesis_round_trip() {
    let mut c = Client::spawn();
    c.initialize();
    c.tool_ok(70, "open", json!({"doc": BRACKET}));

    let shapes = c.tool_ok(71, "list_shapes", json!({}));
    assert_eq!(shapes["shapes"].as_array().unwrap().len(), 5);

    let upserted = c.tool_ok(
        72,
        "upsert_shape",
        json!({
            "id": "rib_gusset",
            "kind": "box",
            "op": "add",
            "blend": 1.5,
            "size": [12.0, 4.0, 10.0],
            "at": [10.0, 0.0, 5.0],
            "color": "#38BDF8"
        }),
    );
    assert_eq!(upserted["action"], "created");
    assert_eq!(upserted["shapes"], 6);
    assert_eq!(upserted["problems"], json!([]));

    let deleted = c.tool_ok(73, "delete_entity", json!({"id": "rib_gusset"}));
    assert_eq!(deleted["kind"], "shape");

    let catalog = c.tool_ok(74, "list_coupled_systems", json!({}));
    let systems = catalog["systems"].as_array().unwrap();
    assert_eq!(systems.len(), 6);
    assert_eq!(
        systems[0]["system_id"],
        "pilkington_float_bath_cvd_coater_v1"
    );
    assert_eq!(systems[1]["system_id"], "regen_nozzle_h2_v1");
    assert_eq!(systems[2]["system_id"], "solar_candle_ignition_v1");
    assert_eq!(systems[3]["system_id"], "jwst_fastener_fixing_envelope_v1");
    assert_eq!(systems[4]["system_id"], "deadcat_4in_gemini_antenna_v1");
    assert_eq!(systems[5]["system_id"], "annular_swirl_spray_combustor_v1");

    // Optimise the Regenerative Cooled Rocket Nozzle system via Sparse Adjoint steps
    // and verify that its 3D `.fsm` twin is automatically synced into the open CAD document.
    let opt = c.tool_ok(
        75,
        "optimise_coupled_system",
        json!({
            "system_id": "regen_nozzle_h2_v1",
            "steps": 2,
            "sync_cad": true
        }),
    );
    assert_eq!(opt["synced_to_cad_doc"], true);
    assert!(opt["fsm_3d"]
        .as_str()
        .unwrap()
        .contains("Supercritical-H2 Rocket Nozzle"));

    let shapes_after = c.tool_ok(76, "list_shapes", json!({}));
    assert!(shapes_after["name"]
        .as_str()
        .unwrap()
        .contains("Supercritical-H2 Rocket Nozzle"));
    assert_eq!(shapes_after["shapes"].as_array().unwrap().len(), 8);

    // Solve the 1D->3D projected rocket nozzle `.fsm` twin in 3D thermo-mechanical FEA
    let solved_nozzle = c.tool_ok(
        77,
        "solve",
        json!({
            "kind": "solve",
            "dofs": 8000
        }),
    );
    assert_eq!(solved_nozzle["status"], "ok");
    let cases = solved_nozzle["single"]["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2);
    assert!(cases[0]["max_vm_elem"].as_f64().unwrap() > 1.0);
    let graded = &solved_nozzle["single"]["graded"];
    assert!(graded["mass_g"].as_f64().unwrap() < graded["solid_mass_g"].as_f64().unwrap() * 0.70);
    assert!(
        graded["max_layer_peel"].as_array().unwrap()[1]
            .as_f64()
            .unwrap()
            < 0.60
    );
}

#[test]
fn spatial_boundary_conditions_round_trip_and_solve_over_mcp() {
    let mut c = Client::spawn();
    let _ = c.ask(
        1,
        "initialize",
        json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}),
    );
    c.tool_ok(
        2,
        "new_document",
        json!({"name": "MCP Spatial BC Wall", "material": "ss316l"}),
    );
    c.tool_ok(
        3,
        "upsert_shape",
        json!({"id": "wall", "kind": "box", "size": [4.0, 10.0, 20.0], "at": [2.0, 5.0, 10.0]}),
    );
    c.tool_ok(
        4,
        "add_region",
        json!({"id": "base", "selectors": [{"shape": "wall", "face": "nz"}]}),
    );
    c.tool_ok(
        5,
        "add_region",
        json!({"id": "wet", "selectors": [{"shape": "wall", "face": "nx"}]}),
    );
    c.tool_ok(
        6,
        "add_region",
        json!({"id": "dry", "selectors": [{"shape": "wall", "face": "px"}]}),
    );
    c.tool_ok(
        7,
        "add_support",
        json!({"id": "fix_base", "region": "base", "fixed": [true, true, true]}),
    );
    let r_load = c.tool_ok(
        8,
        "add_load",
        json!({"id": "hydro", "region": "wet", "pressure": 5.0, "pressure_fn": "0.5 * (20 - z)", "case": "Hydro"}),
    );
    assert!(
        r_load["problems"].as_array().unwrap().is_empty(),
        "{r_load}"
    );
    let r_temp = c.tool_ok(
        9,
        "add_temperature",
        json!({"id": "t_grad", "region": "wet", "temp_fn": "20 + 3 * z", "case": "Thermal"}),
    );
    assert!(
        r_temp["problems"].as_array().unwrap().is_empty(),
        "{r_temp}"
    );
    let r_conv = c.tool_ok(
        10,
        "add_convection",
        json!({"id": "c_grad", "region": "dry", "h_fn": "0.02 + 0.002 * z", "tinf_fn": "20 + 0.5 * z", "case": "Thermal"}),
    );
    assert!(
        r_conv["problems"].as_array().unwrap().is_empty(),
        "{r_conv}"
    );
    let r_hf = c.tool_ok(
        11,
        "add_heatflux",
        json!({"id": "hf_grad", "region": "dry", "q_fn": "0.1 * sin(pi * z / 20)", "case": "Thermal"}),
    );
    assert!(r_hf["problems"].as_array().unwrap().is_empty(), "{r_hf}");

    let txt = c.tool_ok(12, "doc_text", json!({}));
    let fsm_str = txt["text"].as_str().expect("doc_text.text");
    assert!(fsm_str.contains("pressure_fn=\"0.5 * (20 - z)\""));
    assert!(fsm_str.contains("temp_fn=\"20 + 3 * z\""));
    assert!(fsm_str.contains("h_fn=\"0.02 + 0.002 * z\""));
    assert!(fsm_str.contains("tinf_fn=\"20 + 0.5 * z\""));
    assert!(fsm_str.contains("q_fn=\"0.1 * sin(pi * z / 20)\""));

    let solved = c.tool_ok(13, "solve", json!({"kind": "solve", "h": 3.0}));
    assert_eq!(solved["status"], "ok");
    assert_eq!(solved["single"]["cases"].as_array().unwrap().len(), 2);

    let del = c.tool_ok(14, "delete_entity", json!({"id": "hf_grad"}));
    assert_eq!(del["kind"], "heatflux");
}

#[test]
fn parametric_sketch_persists_in_fsm_and_reacts_to_set_let_over_mcp() {
    let mut c = Client::spawn();
    c.initialize();
    c.tool_ok(
        2,
        "new_document",
        json!({"name": "MCP Persistent Sketch", "material": "ss316l"}),
    );
    c.tool_ok(3, "set_let", json!({"name": "boss_w", "value": 20.0}));
    c.tool_ok(4, "set_let", json!({"name": "boss_d", "value": 16.0}));

    let feat = c.tool_ok(
        5,
        "apply_sketch_feature",
        json!({
            "feature_id": "boss1",
            "sketch_id": "sk_boss",
            "operation": {"kind": "boss_extrude", "height": 12.0},
            "sketch": {
                "id": "sk_boss",
                "plane": {"kind": "xy"},
                "points": [
                    {"id": "p0", "x": -10.0, "y": -8.0},
                    {"id": "p1", "x": 10.0, "y": -8.0},
                    {"id": "p2", "x": 10.0, "y": 8.0},
                    {"id": "p3", "x": -10.0, "y": 8.0}
                ],
                "segments": [
                    {"id": "s0", "p1": "p0", "p2": "p1"},
                    {"id": "s1", "p1": "p1", "p2": "p2"},
                    {"id": "s2", "p1": "p2", "p2": "p3"},
                    {"id": "s3", "p1": "p3", "p2": "p0"}
                ],
                "constraints": [
                    {"id": "c_fix", "kind": "fixed", "point": "p0", "x": -10.0, "y": -8.0},
                    {"id": "c_h0", "kind": "horizontal", "segment": "s0"},
                    {"id": "c_h2", "kind": "horizontal", "segment": "s2"},
                    {"id": "c_v1", "kind": "vertical", "segment": "s1"},
                    {"id": "c_v3", "kind": "vertical", "segment": "s3"},
                    {"id": "c_w", "kind": "distance", "segment": "s0", "value": 20.0, "expr": "boss_w"},
                    {"id": "c_d", "kind": "distance", "segment": "s1", "value": 16.0, "expr": "boss_d"}
                ]
            }
        }),
    );
    assert!(feat["problems"].as_array().unwrap().is_empty(), "{feat}");
    assert_eq!(feat["sketches"], 1);
    assert_eq!(feat["sketch_features"], 1);

    let insp = c.tool_ok(6, "inspect", json!({}));
    assert_eq!(insp["sketches"].as_array().unwrap().len(), 1);
    assert_eq!(insp["sketch_features"].as_array().unwrap().len(), 1);

    let txt = c.tool_ok(7, "doc_text", json!({}));
    let fsm_str = txt["text"].as_str().expect("doc_text.text");
    assert!(fsm_str.contains("sketch sk_boss"), "{fsm_str}");
    assert!(
        fsm_str.contains("sketchfeature boss1 sketch=sk_boss"),
        "{fsm_str}"
    );

    // Re-open the printed `.fsm` text and verify the sketch binding survives round-trip.
    c.tool_ok(8, "open", json!({"doc": fsm_str}));
    c.tool_ok(9, "set_let", json!({"name": "boss_w", "value": 34.0}));

    let shapes = c.tool_ok(10, "list_shapes", json!({}));
    let profile = shapes["shapes"][0]["kind"]["profile"]
        .as_array()
        .expect("extrude profile");
    let xs: Vec<f64> = profile.iter().map(|v| v["x"].as_f64().unwrap()).collect();
    let span_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - xs.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(
        (span_x - 34.0).abs() < 1e-4,
        "expected boss_w=34 to update 3D extrude profile span to 34 mm, got {span_x} ({profile:?})"
    );
}

#[test]
fn declarative_fsm_system_block_solves_optimises_and_projects_1d_bcs_onto_3d_fea_over_mcp() {
    let fsm_doc = "\
fieldsmith 1
name \"User-Authored 1D Cooled Duct\"
material ss316l
let wall_t = 2.5
cylinder outer_wall r=8 + wall_t h=80 at=(0, 0, 40)
cylinder inner_bore r=8 h=84 at=(0, 0, 40) subtract
region bore_surf = inner_bore.side
region outer_surf = outer_wall.side
region base_cap = outer_wall.bottom
support fix_base base_cap fixed=xyz
convection bore_cool bore_surf h=0.01 tinf=25 case=\"Thermal\"
heatflux outer_heat outer_surf q=0.25 case=\"Thermal\"

system user_duct_cooler title=\"1D Convective Duct Cooler\" sector=\"Thermal-Fluids\" nodes=17 span=(0, 80) axis=z {
  design mdot nominal=0.03 min=0.015 max=0.20 unit=\"kg/s\" symbol=\"m_dot\"
  design wall_t nominal=2.5 min=1.5 max=5.0 unit=\"mm\" symbol=\"t_w\" category=geometry
  epistemic h_mult nominal=1.0 sigma=0.1 min=0.7 max=1.3 unit=\"-\" symbol=\"c_h\"
  state T_f init=\"25\" scale=100 unit=\"C\" symbol=\"T_f\"
  state T_w init=\"95\" scale=100 unit=\"C\" symbol=\"T_w\"
  aux h_eff expr=\"0.022 * h_mult * (mdot / 0.05)^0.8\"
  aux q_in expr=\"1.25 * (1 + 0.25 * sin(pi * s / L))\"
  residual T_f eq=\"4180 * mdot * d_ds(T_f) - 2 * pi * 8 * h_eff * (T_w - T_f)\" bc_left=\"T_f - 25\"
  residual T_w eq=\"-16 * wall_t * d2_ds2(T_w) + h_eff * (T_w - T_f) - q_in\" bc_left=\"d_ds(T_w)\" bc_right=\"d_ds(T_w)\"
  objective peak_wall expr=\"T_w\" reduce=max kind=upper limit=95 unit=\"C\" symbol=\"T_max\"
  objective flow_cost expr=\"mdot\" reduce=value kind=minimize unit=\"kg/s\" symbol=\"m_dot\"
  bind bore_cool attr=h expr=\"h_eff\"
  bind bore_cool attr=tinf expr=\"T_f\"
  bind outer_heat attr=q expr=\"q_in\"
}
solve size=6 tref=20 build=(0, 0)
";

    let mut c = Client::spawn();
    c.initialize();
    c.tool_ok(70, "open", json!({"doc": fsm_doc}));

    let cat = c.tool_ok(71, "list_coupled_systems", json!({}));
    let first = &cat["systems"][0];
    assert_eq!(first["system_id"], "user_duct_cooler");
    assert_eq!(first["is_declarative_doc_system"], true);
    assert_eq!(first["num_dofs"], 34);

    // Solve without even passing system_id — defaults to open document's declarative `system`!
    let solved = c.tool_ok(72, "solve_coupled_system", json!({}));
    assert_eq!(solved["analysis"]["system_id"], "user_duct_cooler");
    assert!(solved["analysis"]["final_residual_l2"].as_f64().unwrap() < 1e-9);
    assert!(
        solved["analysis"]["adjoint_vs_tangent_max_rel_err"]
            .as_f64()
            .unwrap()
            < 1e-5
    );
    let t_peak_0 = solved["analysis"]["objectives"][0].as_f64().unwrap();
    assert!(
        t_peak_0 > 95.0,
        "low-flow nominal should exceed 95 C limit, got {t_peak_0}"
    );

    // Take 3 Sparse Adjoint optimisation steps — increases `mdot` to reduce `peak_wall`!
    let opt = c.tool_ok(73, "optimise_coupled_system", json!({"steps": 3}));
    let t_peak_opt = opt["final_objectives"][0].as_f64().unwrap();
    assert!(
        t_peak_opt < t_peak_0,
        "Adjoint optimisation must reduce peak wall temp: {t_peak_opt} >= {t_peak_0}"
    );

    // Verify 1D `lerp(z, ...)` profiles landed on the session `.fsm` document AND 3D FEA solves cleanly!
    let insp = c.tool_ok(74, "inspect", json!({}));
    let tinf_fn = insp["convections"][0]["t_inf_expr"]
        .as_str()
        .expect("projected t_inf_expr");
    assert!(tinf_fn.starts_with("lerp(z, 0, 25"), "got {tinf_fn}");

    let fea = c.tool_ok(75, "solve", json!({"kind": "solve", "h": 6.0}));
    assert_eq!(fea["status"], "ok", "{fea}");
    assert!(
        fea["single"]["cases"][0]["max_von_mises"].as_f64().unwrap() > 0.0,
        "{fea}"
    );
}

#[test]
fn epistemic_uq_and_robust_p90_contract_optimisation_over_mcp() {
    let mut c = Client::spawn();
    c.initialize();
    let fsm_src = "\
fieldsmith 1
name \"MCP Epistemic Cantilever\"
material ss316l
let d = 9.0
box beam size=(80, d, 12) at=(40, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load pull tip force=(0, -600, 0)
";
    c.tool_ok(2, "open", json!({"doc": fsm_src}));
    c.tool_ok(
        3,
        "upsert_uncertainty",
        json!({"label": "Depth tolerance", "param": "d", "std": 0.30, "unit": "mm"}),
    );
    c.tool_ok(
        4,
        "upsert_uncertainty",
        json!({"label": "Load jitter", "param": "pull.force.1", "cv": 0.12, "unit": "N"}),
    );
    let uq_env = c.tool_ok(5, "uq", json!({"dofs": 6000}));
    assert_eq!(uq_env["status"], "ok");
    assert!(uq_env["uq"]["solves_run"].as_u64().unwrap() <= 3);
    let vm_row = uq_env["uq"]["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["metric"] == "max_vm_elem")
        .expect("max_vm_elem row");
    assert!(vm_row["p90"].as_f64().unwrap() > vm_row["p50"].as_f64().unwrap() + 15.0);

    c.tool_ok(
        6,
        "upsert_assert",
        json!({"label": "Robust p90 stress", "metric": "p90.max_vm_elem", "target": "<= 190", "unit": "MPa"}),
    );
    let ver_before = c.tool_ok(7, "verify", json!({"dofs": 6000}));
    assert_eq!(ver_before["status"], "refusal");

    let opt = c.tool_ok(
        8,
        "optimise",
        json!({"objective": "mass_g", "params": ["d"], "bounds": {"d": [8.5, 16.0]}, "dofs": 6000}),
    );
    assert_eq!(opt["status"], "ok", "{opt}");
    let ver_after = c.tool_ok(9, "verify", json!({"dofs": 6000}));
    assert_eq!(ver_after["status"], "ok", "{ver_after}");
}

#[test]
fn bolted_flange_insert_reconfigure_evaluate_and_preload_uq_over_mcp() {
    let mut c = Client::spawn();
    c.initialize();
    let fsm_src = "\
fieldsmith 1
name \"MCP Bolted Flange\"
material al7075
box flange size=(70, 50, 12) at=(0, 0, 6)
region base_root = flange.nx
support base_fix base_root fixed=xyz
";
    c.tool_ok(10, "open", json!({"doc": fsm_src}));
    let ins = c.tool_ok(
        11,
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
            "include_washer": true,
            "associated_nut": "nyloc_nut",
            "preload_cv": 0.15,
            "fastener_material": "steel_10_9"
        }),
    );
    assert_eq!(ins["problems"].as_array().unwrap().len(), 0, "{ins}");
    assert_eq!(ins["loads"].as_u64(), Some(4));
    assert_eq!(ins["uncertainties"].as_u64(), Some(1));
    let first_id = ins["fixing"]["first_new_id"]
        .as_str()
        .expect("first_new_id")
        .to_string();

    let reconf = c.tool_ok(
        12,
        "reconfigure_fixing",
        json!({
            "selected_node_id": first_id,
            "include_washer": true,
            "associated_nut": "hex_nut",
            "fastener_material": "ti64"
        }),
    );
    assert_eq!(reconf["problems"].as_array().unwrap().len(), 0, "{reconf}");
    assert_eq!(reconf["fixing_stack"]["associated_nut"], "hex_nut");
    assert_eq!(reconf["fixing_stack"]["fastener_material"], "ti64");

    let eval = c.tool_ok(
        13,
        "evaluate_fixing",
        json!({
            "fixing_kind": "hex_cap_bolt",
            "nominal_d_mm": 5.0,
            "pitch_mm": 0.8,
            "engagement_length_mm": 10.0,
            "substrate_material": "al7075",
            "fastener_material": "ti64",
            "include_washer": true,
            "associated_nut": "hex_nut"
        }),
    );
    assert_eq!(eval["status"], "ok", "{eval}");
    assert!(
        eval["fixing"]["envelope"]["holdable_forces"]["safe_working_axial_load_n"]
            .as_f64()
            .unwrap()
            > 100.0
    );
    assert_eq!(eval["fixing"]["benchmarks"].as_array().unwrap().len(), 4);

    let uq_env = c.tool_ok(14, "uq", json!({"dofs": 14000}));
    assert_eq!(uq_env["status"], "ok", "{uq_env}");
    assert!(uq_env["uq"]["solves_run"].as_u64().unwrap() <= 3);
}
