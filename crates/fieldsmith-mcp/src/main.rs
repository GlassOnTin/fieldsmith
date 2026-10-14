//! `fieldsmith-mcp` — the MCP stdio server (GOAL v0.4 section C, slice 2). It will
//! speak hand-rolled JSON-RPC against one open document, calling the same
//! `fieldsmith_api` functions the one-shot verb uses, so V22's second column (MCP) and
//! its first (API golden) share one code path.

fn main() {
    // Slice 2 implements initialize / tools-list / tools-call here. Anything else
    // would be a fake server, so this builds as a placeholder and refuses honestly.
    eprintln!("fieldsmith-mcp: the stdio MCP server is not implemented yet (M15 slice 2)");
    std::process::exit(1);
}
