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
        assert_ne!(result["isError"].as_bool(), Some(true));
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
            "inspect",
            "add_region",
            "add_support",
            "add_load",
            "add_temperature",
            "set_material",
            "doc_text",
            "solve",
            "export",
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
