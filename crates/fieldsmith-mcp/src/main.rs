//! `fieldsmith-mcp` — the MCP stdio server (GOAL v0.4 section C, slice 2). Line-based
//! JSON-RPC 2.0 on stdin/stdout: one request line in, at most one response line out
//! ([`fieldsmith_mcp::Server::handle`]); notifications produce no bytes. All the logic
//! lives in the lib so the verify tests drive the same code path this loop runs.

use std::io::{BufRead, Write};

fn main() -> std::io::Result<()> {
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