//! Compiled from `def/cli/data.lfy`: what the command line can do and how it returns.

use std::collections::BTreeMap;

/// What the command line can do.
// @lfy def/cli/data.lfy:1
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help,    // @lfy def/cli/data.lfy:2
    Version, // @lfy def/cli/data.lfy:3
    Init,    // @lfy def/cli/data.lfy:4
    Check,   // @lfy def/cli/data.lfy:5
    Format,  // @lfy def/cli/data.lfy:6
    Tree,    // @lfy def/cli/data.lfy:7
    Tokens,  // @lfy def/cli/data.lfy:8
    Compile, // @lfy def/cli/data.lfy:9
    Lsp,     // @lfy def/cli/data.lfy:10
    Mcp,     // @lfy def/cli/data.lfy:11
}

impl Command {
    pub const ALL: [Command; 10] = [
        Command::Help,
        Command::Version,
        Command::Init,
        Command::Check,
        Command::Format,
        Command::Tree,
        Command::Tokens,
        Command::Compile,
        Command::Lsp,
        Command::Mcp,
    ];

    /// The value of the enum member: what is typed.
    pub fn value(self) -> &'static str {
        match self {
            Command::Help => "help",
            Command::Version => "version",
            Command::Init => "init",
            Command::Check => "check",
            Command::Format => "format",
            Command::Tree => "tree",
            Command::Tokens => "tokens",
            Command::Compile => "compile",
            Command::Lsp => "lsp",
            Command::Mcp => "mcp",
        }
    }

    pub fn lookup(text: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.value() == text)
    }

    /// One line of description for help.
    pub fn description(self) -> &'static str {
        match self {
            Command::Help => "Print this help",
            Command::Version => "Print the version of the executable",
            Command::Init => "Create elfie.json and the source directory in the root",
            Command::Check => "Load the project and print every problem",
            Command::Format => "Rewrite source files in the standard layout (--check only reports)",
            Command::Tree => "Print the parse tree of one file",
            Command::Tokens => "Print the tokens of one file",
            Command::Compile => "Plan the units and run the compiler on each (--dry-run, --all, --target)",
            Command::Lsp => "Serve an editor over standard input and output",
            Command::Mcp => "Serve agents over standard input and output",
        }
    }
}

/// What the process returns.
// @lfy def/cli/data.lfy:14
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Done, and nothing to report.
    Success = 0, // @lfy def/cli/data.lfy:15
    /// Done, and problems were found or outputs rejected.
    Problems = 1, // @lfy def/cli/data.lfy:16
    /// The arguments could not be read.
    Usage = 2, // @lfy def/cli/data.lfy:17
    /// Something the command relies on failed.
    Failure = 3, // @lfy def/cli/data.lfy:18
}

impl ExitCode {
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// One parsed command line.
// @lfy def/cli/data.lfy:21
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// What to do.
    pub command: Command, // @lfy def/cli/data.lfy:22
    /// The project directory.
    pub root: String, // @lfy def/cli/data.lfy:23
    /// The positional arguments after the command, in order.
    pub arguments: Vec<String>, // @lfy def/cli/data.lfy:24
    /// Named options by name without their dashes; `None` for a flag, the text for a
    /// valued option.
    pub options: BTreeMap<String, Option<String>>, // @lfy def/cli/data.lfy:25
}

impl Invocation {
    pub fn flag(&self, name: &str) -> bool {
        self.options.contains_key(name)
    }

    pub fn option(&self, name: &str) -> Option<&str> {
        self.options.get(name).and_then(|v| v.as_deref())
    }
}
