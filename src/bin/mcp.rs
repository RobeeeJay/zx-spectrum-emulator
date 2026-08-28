//! The emulator as an MCP server: JSON-RPC 2.0 over stdin and stdout.
//!
//! `cargo run --release --bin mcp`, or point an MCP client at the built
//! binary. See `docs/mcp.md`.

fn main() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    zx_rustrum::mcp::serve(stdin.lock(), stdout.lock())
}
