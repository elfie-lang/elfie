//! The Elfie language server; compiled from `def/lsp`.

use std::path::Path;

/// Serve one editor over standard input and output until it shuts the server down.
pub fn serve(_root: Option<&Path>) -> i32 {
    eprintln!("the language server is not compiled yet");
    3
}
