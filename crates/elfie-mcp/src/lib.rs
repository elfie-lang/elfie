//! The Elfie agent server; compiled from `def/mcp`.

use std::path::Path;

/// Serve agents over standard input and output until the connection closes.
pub fn serve(_root: Option<&Path>) -> i32 {
    eprintln!("the agent server is not compiled yet");
    3
}
