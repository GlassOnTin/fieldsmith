//! Embedded HTTP/1.1 compute, MCP JSON-RPC, and static Web/PWA asset server.
//!
//! Used in two places:
//! 1. **Standalone remote compute & headless workstation host** (`fieldsmith-mcp --serve 0.0.0.0:8080`
//!    or `fieldsmith serve --bind 0.0.0.0:8080 [--web-dir web/dist]`): exposes `/api/health`,
//!    `/api/workspace`, `/api/mcp`, and `/api/solve` with CORS headers so a mobile phone or
//!    remote browser can offload multi-core native FEA and coupled PDE solves over LAN,
//!    Tailscale, or WireGuard.
//! 2. **Android F-Droid local host (`sh.fieldsmith.app`)**: binds to `127.0.0.1:0` inside the
//!    Android app process and serves the bundled `web/dist` assets with
//!    `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`
//!    headers (unlocking `SharedArrayBuffer` multi-threaded WASM as well as native `arm64-v8a`
//!    `/api/mcp` and `/api/solve` execution).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use fieldsmith_api::respond;
use serde_json::{json, Value};

use crate::{Server, SharedWorkspaceState};

#[derive(Clone, Debug)]
pub struct ServeConfig {
    pub bind_addr: String,
    pub workspace_path: PathBuf,
    pub web_dir: Option<PathBuf>,
}

pub struct RunningServer {
    local_addr: SocketAddr,
}

impl RunningServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

/// Bind `cfg.bind_addr` and spawn a background accept loop. Returns the bound [`RunningServer`]
/// (useful when binding to port `0` on Android or in tests).
pub fn spawn_http_server(cfg: ServeConfig) -> std::io::Result<RunningServer> {
    let listener = TcpListener::bind(&cfg.bind_addr)?;
    let local_addr = listener.local_addr()?;
    let shared_cfg = Arc::new(cfg);
    let mcp_lock = Arc::new(Mutex::new(()));

    std::thread::Builder::new()
        .name("fieldsmith-http".to_string())
        .spawn(move || {
            for incoming in listener.incoming() {
                let Ok(stream) = incoming else {
                    continue;
                };
                let c = Arc::clone(&shared_cfg);
                let l = Arc::clone(&mcp_lock);
                std::thread::spawn(move || {
                    let _ = handle_connection(stream, &c, &l);
                });
            }
        })?;

    Ok(RunningServer { local_addr })
}

/// Run the HTTP server on the current thread (blocking).
pub fn run_http_server_blocking(cfg: ServeConfig) -> std::io::Result<()> {
    let listener = TcpListener::bind(&cfg.bind_addr)?;
    let local_addr = listener.local_addr()?;
    eprintln!(
        "fieldsmith compute & MCP server listening on http://{local_addr} (arch={}, threads={})",
        std::env::consts::ARCH,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    );
    let shared_cfg = Arc::new(cfg);
    let mcp_lock = Arc::new(Mutex::new(()));
    for incoming in listener.incoming() {
        let Ok(stream) = incoming else {
            continue;
        };
        let c = Arc::clone(&shared_cfg);
        let l = Arc::clone(&mcp_lock);
        std::thread::spawn(move || {
            let _ = handle_connection(stream, &c, &l);
        });
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_workspace_file(path: &Path) -> SharedWorkspaceState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<SharedWorkspaceState>(&s).ok())
        .unwrap_or_else(|| SharedWorkspaceState {
            revision: 0,
            updated_at_ms: now_ms(),
            actor: "init".to_string(),
            last_tool: "none".to_string(),
            doc_fsm: None,
            coupled_system_id: None,
            design_p: None,
            epistemic_theta: None,
            activity_log: Vec::new(),
        })
}

fn write_workspace_file(path: &Path, ws: &SharedWorkspaceState) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(ws) {
        let _ = std::fs::write(path, text);
    }
}

fn handle_connection(
    mut stream: TcpStream,
    cfg: &ServeConfig,
    mcp_lock: &Mutex<()>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return write_json(&mut stream, 400, &json!({ "error": "bad request line" }));
    }
    let method = parts[0];
    let raw_target = parts[1];
    let path_only = raw_target.split('?').next().unwrap_or("/");

    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    if method == "OPTIONS" {
        return write_empty_cors(&mut stream, 204);
    }

    let mut body_bytes = vec![0u8; content_length.min(64 * 1024 * 1024)];
    if content_length > 0 {
        reader.read_exact(&mut body_bytes)?;
    }
    let body_str = String::from_utf8_lossy(&body_bytes);

    match (method, path_only) {
        ("GET", "/api/health") => {
            let threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            let ws = read_workspace_file(&cfg.workspace_path);
            let payload = json!({
                "status": "ok",
                "server": "fieldsmith-compute",
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "threads": threads,
                "workspace_revision": ws.revision,
                "capabilities": ["mcp", "workspace", "solve", "coupled_systems"],
            });
            write_json(&mut stream, 200, &payload)
        }
        ("GET", "/api/workspace") => {
            let ws = read_workspace_file(&cfg.workspace_path);
            let val = serde_json::to_value(ws).unwrap_or_else(|_| json!({}));
            write_json(&mut stream, 200, &val)
        }
        ("POST", "/api/workspace") => {
            let _guard = mcp_lock.lock().unwrap_or_else(|e| e.into_inner());
            let incoming: Value = match serde_json::from_str(&body_str) {
                Ok(v) => v,
                Err(e) => return write_json(&mut stream, 400, &json!({ "error": e.to_string() })),
            };
            let mut ws = read_workspace_file(&cfg.workspace_path);
            ws.revision += 1;
            ws.updated_at_ms = now_ms();
            ws.actor = incoming
                .get("actor")
                .and_then(Value::as_str)
                .unwrap_or("web-ui")
                .to_string();
            ws.last_tool = incoming
                .get("last_tool")
                .and_then(Value::as_str)
                .unwrap_or("ui-sync")
                .to_string();
            if let Some(doc_fsm) = incoming.get("doc_fsm").and_then(Value::as_str) {
                ws.doc_fsm = Some(doc_fsm.to_string());
            }
            if let Some(cid) = incoming.get("coupled_system_id").and_then(Value::as_str) {
                ws.coupled_system_id = Some(cid.to_string());
            }
            if let Some(arr) = incoming.get("design_p").and_then(Value::as_array) {
                ws.design_p = Some(arr.iter().filter_map(Value::as_f64).collect());
            }
            if let Some(arr) = incoming.get("epistemic_theta").and_then(Value::as_array) {
                ws.epistemic_theta = Some(arr.iter().filter_map(Value::as_f64).collect());
            }
            let summary = incoming
                .get("summary")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("Updated from {}", ws.actor));
            ws.activity_log.push(json!({
                "revision": ws.revision,
                "timestamp_ms": ws.updated_at_ms,
                "actor": ws.actor,
                "tool": ws.last_tool,
                "summary": summary,
            }));
            if ws.activity_log.len() > 25 {
                let drain = ws.activity_log.len() - 25;
                ws.activity_log.drain(0..drain);
            }
            write_workspace_file(&cfg.workspace_path, &ws);
            let val = serde_json::to_value(ws).unwrap_or_else(|_| json!({}));
            write_json(&mut stream, 200, &val)
        }
        ("POST", "/api/mcp") => {
            let _guard = mcp_lock.lock().unwrap_or_else(|e| e.into_inner());
            let parsed: Value = match serde_json::from_str(&body_str) {
                Ok(v) => v,
                Err(e) => return write_json(&mut stream, 400, &json!({ "error": e.to_string() })),
            };
            let rpc_req = if parsed.get("jsonrpc").and_then(Value::as_str) == Some("2.0") {
                parsed
            } else {
                let id = parsed.get("id").cloned().unwrap_or_else(|| json!(1));
                let method = parsed
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("tools/call");
                let params = parsed.get("params").cloned().unwrap_or_else(|| {
                    let tool_name = parsed
                        .get("tool")
                        .or_else(|| parsed.get("name"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let args = parsed
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    json!({
                        "name": tool_name,
                        "arguments": args,
                    })
                });
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params,
                })
            };

            let mut srv = Server::with_workspace(&cfg.workspace_path);
            let reply_str = srv
                .handle(&rpc_req.to_string())
                .unwrap_or_else(|| "{}".to_string());
            let rpc_val: Value =
                serde_json::from_str(&reply_str).unwrap_or_else(|_| json!({ "raw": reply_str }));
            let ws = read_workspace_file(&cfg.workspace_path);
            write_json(
                &mut stream,
                200,
                &json!({
                    "rpc": rpc_val,
                    "workspace": ws,
                }),
            )
        }
        ("POST", "/mcp") => {
            let _guard = mcp_lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut srv = Server::with_workspace(&cfg.workspace_path);
            let reply_str = srv.handle(&body_str).unwrap_or_default();
            write_bytes_with_isolation(
                &mut stream,
                200,
                "application/json; charset=utf-8",
                reply_str.as_bytes(),
            )
        }
        ("GET", "/mcp") => write_bytes_with_isolation(
            &mut stream,
            405,
            "text/plain; charset=utf-8",
            b"Method Not Allowed",
        ),
        ("POST", "/api/solve") => {
            let answer = respond(&body_str);
            write_bytes_with_isolation(
                &mut stream,
                if answer.is_error { 400 } else { 200 },
                "application/json; charset=utf-8",
                answer.body.as_bytes(),
            )
        }
        ("GET", p) => {
            if let Some(ref web_dir) = cfg.web_dir {
                serve_static_file(&mut stream, web_dir, p)
            } else {
                write_json(
                    &mut stream,
                    404,
                    &json!({ "error": "not found (no --web-dir configured)" }),
                )
            }
        }
        _ => write_json(&mut stream, 404, &json!({ "error": "not found" })),
    }
}

fn serve_static_file(stream: &mut TcpStream, web_dir: &Path, req_path: &str) -> std::io::Result<()> {
    let rel = req_path.trim_start_matches('/');
    let rel_clean = if rel.is_empty() { "index.html" } else { rel };
    let mut candidate = web_dir.to_path_buf();
    for comp in Path::new(rel_clean).components() {
        if let Component::Normal(seg) = comp {
            candidate.push(seg);
        }
    }
    if candidate.is_dir() {
        candidate.push("index.html");
    }
    if !candidate.exists() {
        let fallback = web_dir.join("index.html");
        if fallback.exists() && !rel_clean.contains('.') {
            candidate = fallback;
        } else {
            return write_json(stream, 404, &json!({ "error": "static asset not found" }));
        }
    }
    let bytes = std::fs::read(&candidate)?;
    let mime = mime_for_path(&candidate);
    write_bytes_with_isolation(stream, 200, mime, &bytes)
}

fn mime_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    }
}

fn write_empty_cors(stream: &mut TcpStream, code: u16) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, Mcp-Session-Id, Mcp-Protocol-Version\r\n\
         Content-Length: 0\r\n\
         Connection: close\r\n\r\n",
        status_text(code)
    );
    stream.write_all(head.as_bytes())?;
    stream.flush()
}

fn write_json(stream: &mut TcpStream, code: u16, body: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    write_bytes_with_isolation(stream, code, "application/json; charset=utf-8", &bytes)
}

fn write_bytes_with_isolation(
    stream: &mut TcpStream,
    code: u16,
    content_type: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} {}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cross-Origin-Opener-Policy: same-origin\r\n\
         Cross-Origin-Embedder-Policy: require-corp\r\n\
         Cross-Origin-Resource-Policy: cross-origin\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization, Mcp-Session-Id, Mcp-Protocol-Version\r\n\
         Connection: close\r\n\r\n",
        status_text(code),
        bytes.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_server_health_and_mcp_roundtrip() {
        let tmp_ws = std::env::temp_dir().join(format!(
            "fieldsmith-http-test-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp_ws);

        let srv = spawn_http_server(ServeConfig {
            bind_addr: "127.0.0.1:0".to_string(),
            workspace_path: tmp_ws.clone(),
            web_dir: None,
        })
        .expect("spawn http server");
        let addr = srv.local_addr();

        // 1. GET /api/health
        let mut s1 = TcpStream::connect(addr).expect("connect health");
        s1.write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("write health");
        let mut resp1 = String::new();
        s1.read_to_string(&mut resp1).expect("read health");
        assert!(resp1.contains("200 OK"), "resp1={resp1}");
        assert!(resp1.contains("fieldsmith-compute"), "resp1={resp1}");

        // 2. POST /api/mcp (new_document)
        let body = r#"{"tool":"new_document","arguments":{"name":"mobile_test","material":"pla"}}"#;
        let req2 = format!(
            "POST /api/mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let mut s2 = TcpStream::connect(addr).expect("connect mcp");
        s2.write_all(req2.as_bytes()).expect("write mcp");
        let mut resp2 = String::new();
        s2.read_to_string(&mut resp2).expect("read mcp");
        assert!(resp2.contains("200 OK"), "resp2={resp2}");
        assert!(resp2.contains("mobile_test"), "resp2={resp2}");

        // 3. POST /mcp (Streamable HTTP JSON-RPC 2.0)
        let rpc_init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1.0"}}}"#;
        let req3 = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            rpc_init.len(),
            rpc_init
        );
        let mut s3 = TcpStream::connect(addr).expect("connect /mcp");
        s3.write_all(req3.as_bytes()).expect("write /mcp");
        let mut resp3 = String::new();
        s3.read_to_string(&mut resp3).expect("read /mcp");
        assert!(resp3.contains("200 OK"), "resp3={resp3}");
        assert!(resp3.contains("fieldsmith-mcp"), "resp3={resp3}");

        let _ = std::fs::remove_file(&tmp_ws);
    }
}
