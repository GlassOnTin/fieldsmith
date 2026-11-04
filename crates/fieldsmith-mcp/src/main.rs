//! `fieldsmith-mcp` — the MCP stdio server (GOAL v0.4 section C, slice 2). Line-based
//! JSON-RPC 2.0 on stdin/stdout: one request line in, at most one response line out
//! ([`fieldsmith_mcp::Server::handle`]); notifications produce no bytes. All the logic
//! lives in the lib so the verify tests drive the same code path this loop runs.

use std::io::{BufRead, Write};
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(idx) = args.iter().position(|a| a == "--serve") {
        let bind_addr = args
            .get(idx + 1)
            .cloned()
            .unwrap_or_else(|| "0.0.0.0:8080".to_string());
        let web_dir = args
            .iter()
            .position(|a| a == "--web-dir")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("FIELDSMITH_WEB_DIR")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .map(PathBuf::from)
            });
        let workspace_path = std::env::var("FIELDSMITH_WORKSPACE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".fieldsmith-workspace.json"));
        return fieldsmith_mcp::http::run_http_server_blocking(fieldsmith_mcp::http::ServeConfig {
            bind_addr,
            workspace_path,
            web_dir,
        });
    }

    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut server = fieldsmith_mcp::Server::new();
    for line in stdin.lock().lines() {
        // EOF (a closed pipe) is a clean exit; a read error is not.
        let line = line?;
        if let Some(reply) = server.handle(&line) {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}
