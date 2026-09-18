//! Compiled from `def/mcp/data.lfy`: the data of the agent server.
//!
//! A [`Tool`] is one fn of `def/mcp/main.lfy` as the server lists it, with one
//! [`ToolArgument`] per parameter; a [`ToolResult`] is what a call returns; a [`Session`]
//! is one agent's connection, holding the program as of the last call and the stamp of
//! every file it was read from.

use std::collections::BTreeMap;
use std::time::SystemTime;

use elfie_core::workspace::Workspace;

/// One argument of a tool.
// @lfy def/mcp/data.lfy:3
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolArgument {
    /// As the parameter is named.
    pub name: String, // @lfy def/mcp/data.lfy:4
    /// What it is for.
    pub description: String, // @lfy def/mcp/data.lfy:5
    /// `string`, `number`, or `boolean`.
    pub ty: String, // @lfy def/mcp/data.lfy:6
    /// Whether a call must give it.
    pub required: bool, // @lfy def/mcp/data.lfy:7
}

impl ToolArgument {
    /// An argument of the type given, required unless `optional`.
    pub fn new(name: &str, description: &str, ty: &str, required: bool) -> ToolArgument {
        ToolArgument {
            name: name.to_string(),
            description: description.to_string(),
            ty: ty.to_string(),
            required,
        }
    }
}

/// One tool as the server lists it.
// @lfy def/mcp/data.lfy:10
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// `elfie_` followed by the fn's identifier.
    pub name: String, // @lfy def/mcp/data.lfy:11
    /// The fn's definition.
    pub description: String, // @lfy def/mcp/data.lfy:12
    /// In parameter order.
    pub arguments: Vec<ToolArgument>, // @lfy def/mcp/data.lfy:13
}

/// What a call returns.
// @lfy def/mcp/data.lfy:16
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// Plain text for the agent to read.
    pub text: String, // @lfy def/mcp/data.lfy:17
    /// Whether the call failed.
    pub is_error: bool, // @lfy def/mcp/data.lfy:18
}

impl ToolResult {
    /// A result that succeeded with the text.
    pub fn text(text: impl Into<String>) -> ToolResult {
        ToolResult {
            text: text.into(),
            is_error: false,
        }
    }

    /// A result that failed with the text.
    pub fn error(text: impl Into<String>) -> ToolResult {
        ToolResult {
            text: text.into(),
            is_error: true,
        }
    }
}

impl From<Result<String, String>> for ToolResult {
    /// What a tool fn returns, as a result: `Ok` is the text, `Err` the failure.
    fn from(result: Result<String, String>) -> ToolResult {
        match result {
            Ok(text) => ToolResult::text(text),
            Err(text) => ToolResult::error(text),
        }
    }
}

/// The modification time and size of one file when it was last read.
// Decision: the definition types `Session.seen` as an object; its entries are kept as
// this pair so that a changed file is noticed by either its time or its size.
pub type Stamp = (Option<SystemTime>, u64);

/// One agent's connection.
// @lfy def/mcp/data.lfy:21
#[derive(Debug)]
pub struct Session {
    /// The program as of the last call.
    pub workspace: Workspace, // @lfy def/mcp/data.lfy:22
    /// The modification time and size of every file of the program when it was last
    /// read, by path relative to the workspace root.
    pub seen: BTreeMap<String, Stamp>, // @lfy def/mcp/data.lfy:23
}
