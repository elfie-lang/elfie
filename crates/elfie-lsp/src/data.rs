//! Compiled from `def/lsp/data.lfy`: the data of one editor's connection.

use elfie_core::workspace::Workspace;
use tower_lsp::lsp_types::PositionEncodingKind;

/// A file the editor holds open, whose text the editor owns until it closes it.
// @lfy def/lsp/data.lfy:3
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Relative to `Workspace.root`.
    pub path: String, // @lfy def/lsp/data.lfy:4
    /// The text as the editor last sent it.
    pub text: String, // @lfy def/lsp/data.lfy:5
    /// The editor's version of the text, increasing with each change.
    // Decision: the definition types the version as a number; it is kept as the `i32` the
    // protocol carries so it can be handed back with the diagnostics of the document.
    pub version: i32, // @lfy def/lsp/data.lfy:6
}

/// How the client counts columns.
// @lfy def/lsp/data.lfy:9
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Encoding {
    Utf8, // @lfy def/lsp/data.lfy:10
    #[default]
    Utf16, // @lfy def/lsp/data.lfy:11
    Utf32, // @lfy def/lsp/data.lfy:12
}

impl Encoding {
    /// The value of the enum key: the name the protocol uses.
    pub fn value(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf16 => "utf-16",
            Encoding::Utf32 => "utf-32",
        }
    }

    /// The encoding as the protocol spells it.
    pub fn protocol(self) -> PositionEncodingKind {
        match self {
            Encoding::Utf8 => PositionEncodingKind::UTF8,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
            Encoding::Utf32 => PositionEncodingKind::UTF32,
        }
    }

    /// The code units one character takes in this encoding.
    pub fn units_of(self, c: char) -> usize {
        match self {
            Encoding::Utf8 => c.len_utf8(),
            Encoding::Utf16 => c.len_utf16(),
            Encoding::Utf32 => 1,
        }
    }
}

/// One editor's connection.
// @lfy def/lsp/data.lfy:15
#[derive(Debug, Clone)]
pub struct Session {
    /// The program as the editor sees it, open documents included.
    pub workspace: Workspace, // @lfy def/lsp/data.lfy:16
    /// Every open document.
    pub documents: Vec<Document>, // @lfy def/lsp/data.lfy:17
    /// The position encoding agreed at initialization.
    pub encoding: Encoding, // @lfy def/lsp/data.lfy:18
}

impl Session {
    /// The open document at a path, when the editor holds it open.
    pub fn document(&self, path: &str) -> Option<&Document> {
        self.documents.iter().find(|document| document.path == path)
    }
}
