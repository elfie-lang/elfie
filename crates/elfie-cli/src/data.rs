//! Compiled from `def/cli/data.lfy`: what the command line can do and how it returns.

use std::collections::BTreeMap;

/// What the command line can do.
// @lfy def/cli/data.lfy:Command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help,    // @lfy def/cli/data.lfy:Command.help
    Version, // @lfy def/cli/data.lfy:Command.version
    Init,    // @lfy def/cli/data.lfy:Command.init
    Check,   // @lfy def/cli/data.lfy:Command.check
    Format,  // @lfy def/cli/data.lfy:Command.format
    Tree,    // @lfy def/cli/data.lfy:Command.tree
    Tokens,  // @lfy def/cli/data.lfy:Command.tokens
    Compile, // @lfy def/cli/data.lfy:Command.compile
    Lsp,     // @lfy def/cli/data.lfy:Command.lsp
    Mcp,     // @lfy def/cli/data.lfy:Command.mcp
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

/// What the process ends with, through [`std::process::exit`].
// @lfy def/cli/data.lfy:ExitCode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Done, and nothing to report.
    Success = 0, // @lfy def/cli/data.lfy:ExitCode.success
    /// Done, and problems were found or outputs rejected.
    Problems = 1, // @lfy def/cli/data.lfy:ExitCode.problems
    /// The arguments could not be read.
    Usage = 2, // @lfy def/cli/data.lfy:ExitCode.usage
    /// Something the command relies on failed.
    Failure = 3, // @lfy def/cli/data.lfy:ExitCode.failure
}

impl ExitCode {
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// One parsed command line.
// @lfy def/cli/data.lfy:Invocation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// What to do.
    pub command: Command, // @lfy def/cli/data.lfy:Invocation.command
    /// The project directory.
    pub root: String, // @lfy def/cli/data.lfy:Invocation.root
    /// The positional arguments after the command, in order.
    pub arguments: Vec<String>, // @lfy def/cli/data.lfy:Invocation.arguments
    /// Named options by name without their dashes; `None` for a flag, the text for a
    /// valued option.
    pub options: BTreeMap<String, Option<String>>, // @lfy def/cli/data.lfy:Invocation.options
}

impl Invocation {
    pub fn flag(&self, name: &str) -> bool {
        self.options.contains_key(name)
    }

    pub fn option(&self, name: &str) -> Option<&str> {
        self.options.get(name).and_then(|v| v.as_deref())
    }
}

/// What a compile is doing, as progress reports it.
// @lfy def/cli/data.lfy:Step
// Progress is defined here and printed by the compile command, which is not generated yet.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Planned,       // @lfy def/cli/data.lfy:Step.planned
    Requesting,    // @lfy def/cli/data.lfy:Step.requesting
    Compiling,     // @lfy def/cli/data.lfy:Step.compiling
    Checking,      // @lfy def/cli/data.lfy:Step.checking
    Accepted,      // @lfy def/cli/data.lfy:Step.accepted
    Rejected,      // @lfy def/cli/data.lfy:Step.rejected
    Retrying,      // @lfy def/cli/data.lfy:Step.retrying
    Blocked,       // @lfy def/cli/data.lfy:Step.blocked
    Clarification, // @lfy def/cli/data.lfy:Step.clarification
    Failed,        // @lfy def/cli/data.lfy:Step.failed
    Finished,      // @lfy def/cli/data.lfy:Step.finished
}

#[allow(dead_code)]
impl Step {
    /// The name of the step, as a progress line and a JSON object spell it.
    pub fn name(self) -> &'static str {
        match self {
            Step::Planned => "planned",
            Step::Requesting => "requesting",
            Step::Compiling => "compiling",
            Step::Checking => "checking",
            Step::Accepted => "accepted",
            Step::Rejected => "rejected",
            Step::Retrying => "retrying",
            Step::Blocked => "blocked",
            Step::Clarification => "clarification",
            Step::Failed => "failed",
            Step::Finished => "finished",
        }
    }

    /// The value of the enum member: what the step means.
    pub fn value(self) -> &'static str {
        match self {
            Step::Planned => "the plan is made",
            Step::Requesting => "a batch's request is being assembled",
            Step::Compiling => "the compiler is running on a batch",
            Step::Checking => "the outputs of a unit are being checked",
            Step::Accepted => "a unit was accepted and recorded",
            Step::Rejected => "a unit was rejected",
            Step::Retrying => "the compiler is running a batch again with the problems",
            Step::Blocked => "the compiler could not proceed",
            Step::Clarification => "the compiler asked a question",
            Step::Failed => "the compiler command failed",
            Step::Finished => "the compile is over",
        }
    }
}

/// One line of what a compile is doing, for a person or a script watching.
// @lfy def/cli/data.lfy:Progress
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    /// What is happening.
    pub step: Step, // @lfy def/cli/data.lfy:Progress.step
    /// The batch it concerns; `None` for the plan and the end.
    pub batch: Option<String>, // @lfy def/cli/data.lfy:Progress.batch
    /// The unit it concerns; `None` when it concerns a whole batch.
    pub unit: Option<String>, // @lfy def/cli/data.lfy:Progress.unit
    /// Units accepted so far.
    pub done: usize, // @lfy def/cli/data.lfy:Progress.done
    /// Units planned.
    pub total: usize, // @lfy def/cli/data.lfy:Progress.total
    /// Seconds since the compile began, as [`std::time::SystemTime::elapsed`] from its
    /// start gives them.
    pub elapsed: f64, // @lfy def/cli/data.lfy:Progress.elapsed
    /// The detail: the reason a unit is planned, a problem, a question, or the count of
    /// batches.
    pub message: String, // @lfy def/cli/data.lfy:Progress.message
}
