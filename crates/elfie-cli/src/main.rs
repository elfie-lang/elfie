//! Compiled from `def/cli/main.lfy`: the `elfie` command line.
//!
//! Every command is a thin driver over the stage it belongs to, and the process exits
//! with a code from [`ExitCode`] so agents and scripts can read the outcome without
//! parsing text. Progress is a stream of one-line events rather than a redrawn screen, so
//! it reads the same in a terminal, in a log, and as JSON lines for a script.

mod data;
mod style;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::SystemTime;

use elfie_core::generation::{
    self, ForeignEdit, ForeignEditKind, Marker, Move, Outcome, OutcomeKind, Output, Plan, Previous, REVIEWED, Reason,
    Relocated, Relocation, Request, Review, ReviewRecord, ReviewReport, ReviewRequest, ReviewStatus, SourceMap, Unit,
    Verdict,
};
#[cfg(test)]
use elfie_core::generation::{Submission, SubmittedOutcome};
use elfie_core::grammar::GrammarRule as _;
use elfie_core::interpret::{LoweredNode, Program};
use elfie_core::lexer::{Token, lex};
use elfie_core::model::{EntityId, Operation, Problem};
use elfie_core::parser::{Child, ErrorNode, Node, Tree, parse};
use elfie_core::query::{self, Diagnostic, Severity};
use elfie_core::workspace::{self, Target, Workspace};
// @lfy def/cli/compile.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
// @lfy def/cli/compile.lfy:compile#compile:compile:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
use elfie_mcp::{Binding, GlobalReview, Host, Role, SessionRecord};

pub use data::{ChangeSet, ChangeSetUnit, Command, ExitCode, Invocation, Progress, Step, UnitState};
// @lfy def/cli/main.lfy:main
use style::{
    ColorChoice, Stream, Tone, colors_on, duration, glyph_of, padded, paint, reason_tone, severity_tone, status_tone,
    tally, tone_of, width,
};

// ---- the submodules of this unit ----------------------------------------------------

mod commands;
mod compiling;
mod copies;
mod processes;
mod reviewing;
mod running;
mod schedule;

pub use commands::parse_arguments;
use commands::{check, format_command, help, init, tokens, tree, version};
use compiling::compile;
use reviewing::verify;

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    std::process::ExitCode::from(run(&arguments))
}

/// Whether each stream is painted, decided once per run: standard output and standard error
/// apart, as [`colors_on`] decides for the --color choice, and neither with --json, so every
/// JSON line is plain JSON.
// @lfy def/cli/main.lfy:main
#[derive(Debug, Clone, Copy)]
struct Paint {
    /// Whether text for standard output is painted.
    out: bool,
    /// Whether text for standard error is painted.
    err: bool,
}

impl Paint {
    // @lfy def/cli/main.lfy:main
    fn of(invocation: &Invocation) -> Paint {
        // @lfy def/cli/main.lfy:main#main:main:c165c71867417a5b57f2005b84dcc6e393c60ebc0691cfff1ea1c560a49f898e
        Paint::decided(invocation.flag("json"), color_choice(invocation))
    }

    /// Whether each stream is painted when the arguments spell no invocation: --json and
    /// --color are read from the arguments as they are written, so a usage message is painted
    /// by what was asked for and is plain with --json, exactly as every other line is.
    // @lfy def/cli/main.lfy:main
    fn of_arguments(arguments: &[String]) -> Paint {
        // @lfy def/cli/main.lfy:main
        Paint::decided(arguments.iter().any(|argument| argument == "--json"), choice_in(arguments))
    }

    /// Each stream decided once per run: neither is painted with --json, whatever --color
    /// says, so every JSON line is plain JSON; otherwise each is what [`colors_on`] decides
    /// for the choice, standard output and standard error apart.
    // @lfy def/cli/main.lfy:main
    fn decided(json: bool, choice: ColorChoice) -> Paint {
        // @lfy def/cli/main.lfy:main#main:main:971bbab0a61205433b9047ee83a0043ee8ea76fac6f7051a613391b800e2c9f5
        if json {
            return Paint { out: false, err: false };
        }
        // Each stream is decided once per run, standard output and standard error apart.
        // @lfy def/cli/main.lfy:main#main:main:c165c71867417a5b57f2005b84dcc6e393c60ebc0691cfff1ea1c560a49f898e
        // @lfy def/cli/main.lfy:main#main:main:1d8a2834a9d639972a324652c9bc690a8a4223d2fe11c108a95c80c0434e92f3
        // @lfy def/cli/main.lfy:main#main:main:8e109cc37a5231f8c7bd42963bc6f31c2b57bec12d2f7d6024c975d9500eca77
        Paint { out: colors_on(choice, Stream::Output), err: colors_on(choice, Stream::Error) }
    }
}

/// What --color asked for: the value it names, always for the bare flag, and auto when it is
/// not given.
// @lfy def/cli/main.lfy:main
fn color_choice(invocation: &Invocation) -> ColorChoice {
    match invocation.options.get(COLOR) {
        // @lfy def/cli/main.lfy:main#main:main:b1010681b4061d13ca1450d333ca09ece4bc89d41df8d2952fafd2764e99997a
        None => ColorChoice::Auto,
        // The option color gives true: the choice is always.
        // @lfy def/cli/main.lfy:main#main:main:5c2269ebdbb59660619fff88fbd00c044da579723fabecc9550aa33e7921b58e
        Some(None) => ColorChoice::Always,
        Some(Some(value)) => ColorChoice::lookup(value).unwrap_or(ColorChoice::Auto),
    }
}

/// What --color asks for in the arguments as they are written, read the way [`parse_arguments`]
/// reads it but without parsing them, so the choice is known even when they spell no
/// invocation: the value it names, always for the bare flag, and auto when it is not given or
/// names no [`ColorChoice`].
// @lfy def/cli/main.lfy:main
fn choice_in(arguments: &[String]) -> ColorChoice {
    let mut choice = ColorChoice::Auto;
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        // @lfy def/cli/main.lfy:main
        if let Some(value) = argument.strip_prefix("--color=") {
            choice = ColorChoice::lookup(value).unwrap_or(ColorChoice::Auto);
        } else if argument == COLOR_FLAG {
            // @lfy def/cli/main.lfy:main
            choice = match arguments.get(index + 1).and_then(|value| ColorChoice::lookup(value)) {
                Some(found) => {
                    index += 1;
                    found
                }
                // The bare flag is read as always.
                // @lfy def/cli/main.lfy:main#main:main:5c2269ebdbb59660619fff88fbd00c044da579723fabecc9550aa33e7921b58e
                None => ColorChoice::Always,
            };
        }
        index += 1;
    }
    choice
}

/// A message the CLI itself prints to standard error: `elfie:` painted failure, a space, and
/// the message; a message that begins with a path has the path painted subject.
// @lfy def/cli/main.lfy:main
fn complain(on: bool, path: Option<&str>, message: &str) {
    eprintln!("{}", complaint(on, path, message));
}

/// That message as one line, so that every message the CLI prints to standard error is
/// spelled in one place.
// @lfy def/cli/main.lfy:main
fn complaint(on: bool, path: Option<&str>, message: &str) -> String {
    // @lfy def/cli/main.lfy:main#main:main:e08f0e5222d67e3b1a8262febafcc033512185f2bbde7fbe34f189a53fb1d3f1
    let mut line = paint("elfie:", Tone::Failure, on);
    // @lfy def/cli/main.lfy:main#main:main:2d82f2a77899a24a26d34ed9821fe2df83ca505e7f09f5a94ea83b4d0c538b71
    if let Some(path) = path {
        line.push(' ');
        line.push_str(&paint(path, Tone::Subject, on));
        line.push(':');
    }
    line.push(' ');
    line.push_str(message);
    line
}

/// The option that says when the CLI colors what it prints.
// @lfy def/cli/main.lfy:parse
const COLOR: &str = "color";

/// How that option is written among the arguments.
// @lfy def/cli/main.lfy:parse
const COLOR_FLAG: &str = "--color";

/// Run one command and return its exit code.
// @lfy def/cli/main.lfy:main
pub fn run(arguments: &[String]) -> u8 {
    let invocation = match parse_arguments(arguments) {
        Ok(invocation) => invocation,
        Err(message) => {
            // A usage message is printed to standard error with the help text and the code
            // is usage. The arguments spell no invocation, so --json and --color are read
            // from them as they are written.
            // @lfy def/cli/main.lfy:main#main:main:eaaaef39a0b62a0c5ccac3410a16d4fbc681b39fa02be772dc521c3a39081677
            let style = Paint::of_arguments(arguments);
            complain(style.err, None, &format!("{message}\n"));
            // The help text follows the message on standard error.
            // @lfy def/cli/main.lfy:main#main:main:b57702ffb9dc80d9af999db3b3f5989e6ef2f8a2ab2a08dcaf045a7252b81818
            eprintln!("{}", help_text(style.err));
            // @lfy def/cli/main.lfy:main#main:main:889d51d4438d413f53162a7f06a683bc40691c55541b30580a1929caed0c2ded
            return ExitCode::Usage.code();
        }
    };
    let code = match invocation.command {
        Command::Help => help(&invocation),
        Command::Version => version(),
        Command::Init => init(&invocation),
        // @lfy def/cli/main.lfy:main#main:main:481d4218ce7077ec2927351bce1faee54e0b38c651589302ff8954f5de21bcb5
        Command::Check => check(&invocation),
        // @lfy def/cli/main.lfy:main#main:main:63e041dd0eb2785e056ef2f4dfa286f3e9180c74abc863fa5ba8d0c5509c19a0
        Command::Format => format_command(&invocation),
        // @lfy def/cli/main.lfy:main#main:main:d2dfa82545147fcdacc310d86cd6148f4032d0be2a04a009c446cf8c66d14029
        Command::Tree => tree(&invocation),
        // @lfy def/cli/main.lfy:main#main:main:c14eb6d4e898170d9311ec97860a68360e1ebd33a5ca414cdc8b0ccd690ff140
        Command::Tokens => tokens(&invocation),
        // @lfy def/cli/main.lfy:main#main:main:b7b3bc0be22c23f68f958477429b9cec7660b706949318684e87f53812c4435f
        Command::Compile => compile(&invocation),
        // @lfy def/cli/main.lfy:main#main:main:58577360b51377dc9f947bd36c2146edf4e724f26454444cf5f6845da104fe5f
        Command::Verify => verify(&invocation),
        // @lfy def/cli/main.lfy:main#main:main:36f48d78f9141f2f56383741b2ba370ebe00b73a100f6cf7b27dc59c508be4ac
        Command::Lsp => ExitCode::from_code(elfie_lsp::serve(Some(Path::new(&invocation.root)))),
        // @lfy def/cli/main.lfy:main#main:main:f86d558cfd1d8662edda18f5898bbeb20a43f0eec46029343009bdbb8215ebd1
        Command::Mcp => ExitCode::from_code(elfie_mcp::serve(Some(Path::new(&invocation.root)))),
    };
    code.code()
}

impl ExitCode {
    /// The code a served protocol returned, as an exit code.
    // @lfy def/cli/main.lfy:main
    fn from_code(code: i32) -> ExitCode {
        match code {
            0 => ExitCode::Success,
            1 => ExitCode::Problems,
            2 => ExitCode::Usage,
            _ => ExitCode::Failure,
        }
    }

    /// How severe a code is: failure over usage over problems over success, so that the
    /// worst thing a compile met is the code it returns.
    // @lfy def/cli/main.lfy:main
    fn rank(self) -> u8 {
        match self {
            ExitCode::Success => 0,
            ExitCode::Problems => 1,
            ExitCode::Usage => 2,
            ExitCode::Failure => 3,
        }
    }
}

/// A path relative to the root, with forward slashes.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// How many units may run at once when --jobs is not given.
// @lfy def/cli/compile.lfy:compile#compile:compile:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
const DEFAULT_JOBS: usize = 4;

/// The global options help lists: how each is written and one line of what it does.
// @lfy def/cli/main.lfy:main
const OPTIONS: [(&str, &str); 15] = [
    ("--root <dir>", "The project directory (default: the nearest elfie.json above the current directory)"),
    ("--json", "Print results as JSON lines"),
    ("--color <choice>", "When to color what is printed: auto, always, or never (default: auto)"),
    ("--strict", "check: fail on any warning, error, or stale unit"),
    ("--all", "compile: request every unit, up to date or not"),
    ("--target <name>", "compile, verify: only the target named"),
    // @lfy def/cli/main.lfy:main#main:main:027934d626ca3e1baa3508e59ebe0c56b3748c5ce68a885ef8cb2c8c5f6efa53
    ("--jobs <count>", "compile: how many units may run at once (default: 4)"),
    ("--dry-run", "compile: print the plan and generate nothing"),
    ("--accept", "compile: check the outputs already on disk instead of running the compiler"),
    ("--continue", "compile: keep running the units that do not depend on a stopped one"),
    ("--no-verify", "compile: do not run the verifier on a unit that was accepted"),
    ("--global", "verify: review only the global criteria and tests"),
    // @lfy def/cli/compile.lfy:verify#verify:verify:9fc0da16b4fb4241e8c2e859851d9e3fb85f014aa0a2f7a997decf40c5da9d86
    ("--full", "verify: review every criterion and test again, carrying no earlier verdict"),
    ("--help, -h", "Print this help"),
    ("--version", "Print the version"),
];

/// Every command and every option, with one line of description each.
///
/// The usage line and the headings are subject; each command name and each option is active
/// and padded so every description starts in the same column; descriptions are plain.
// @lfy def/cli/main.lfy:main
fn help_text(on: bool) -> String {
    // @lfy def/cli/main.lfy:main#main:main:b29d10711513010880a538943ed621a82080cb0005389669e1c52e6f00e8082d
    let column = Command::ALL
        .iter()
        .map(|command| width(command.value()))
        .chain(OPTIONS.iter().map(|(name, _)| width(name)))
        .max()
        .unwrap_or(0)
        + 2;
    let mut out = String::new();
    // The usage line and the headings are subject.
    // A usage line, painted subject.
    // @lfy def/cli/main.lfy:main#main:main:81836c1a29440f101693cf6125ad1293ff375ba4c8c5ebf6e15215c72a7cf747
    // @lfy def/cli/main.lfy:main#main:main:b29d10711513010880a538943ed621a82080cb0005389669e1c52e6f00e8082d
    out.push_str(&paint("usage: elfie <command> [arguments] [--root <dir>] [--json]", Tone::Subject, on));
    out.push_str("\n\n");
    out.push_str(&paint("commands:", Tone::Subject, on));
    out.push('\n');
    // Every command of Command, verify among them, with one line of description.
    // @lfy def/cli/main.lfy:main#main:main:6f0073ceecb972e98aae5b1618d058d12e3264a00f6faa1982ea18e0ca49ac39
    for command in Command::ALL {
        // Each name is padded so every description starts in the same column.
        // @lfy def/cli/main.lfy:main#main:main:95aa9ed5f0f04989f4527492db6cc9507956c521b47e77a5c366d6f36a7f357e
        let name = padded(&paint(command.value(), Tone::Active, on), column);
        out.push_str(&format!("  {name}{}\n", command.description()));
    }
    out.push('\n');
    // @lfy def/cli/main.lfy:main#main:main:b29d10711513010880a538943ed621a82080cb0005389669e1c52e6f00e8082d
    out.push_str(&paint("options:", Tone::Subject, on));
    out.push('\n');
    // The global options, --jobs, --no-verify, --continue, --strict, and --color among them.
    // @lfy def/cli/main.lfy:main#main:main:027934d626ca3e1baa3508e59ebe0c56b3748c5ce68a885ef8cb2c8c5f6efa53
    for (name, description) in OPTIONS {
        // @lfy def/cli/main.lfy:main#main:main:95aa9ed5f0f04989f4527492db6cc9507956c521b47e77a5c366d6f36a7f357e
        let name = padded(&paint(name, Tone::Active, on), column);
        out.push_str(&format!("  {name}{description}\n"));
    }
    out
}

/// Where a unit's source maps are recorded, as [`generation::map_file`] spells it; init
/// creates it so a project has it before its first compile.
///
/// The maps live beside the cache rather than in it, so nothing under
/// `elfie-compile/cache` is ever the only record of anything: a command run after the cache
/// is deleted gives the same plan, requests, verdicts, and outputs as one run with it.
// @lfy def/cli/main.lfy:main#main:main:c160a3dc4d430206c372000abe8e8c0d338ea73fab5879b08956e0b6b4d9c22f
// @lfy def/cli/main.lfy:main#main:main:ce142a9ba3b804ff57be3fcdcb014e73336642491908017d455d8ff42740abee
const MAPS: &str = "elfie-compile/maps";

/// Where the review records of every unit of a target are kept, under the root.
// @lfy def/cli/compile.lfy:compile#compile:compile:ea7ff5171f6e9c135b3736f46e4d6878cfe8097bd5ca7b45686d9b57a0e0cfb7
const REVIEWS: &str = "elfie-compile/reviews";

/// A diagnostic is printed as the file, a colon, the line, a colon, the column, a colon, the
/// stage, the severity, and the message.
///
/// Without --json the file, line, and column with their colons are subject, the stage is
/// muted, the severity is painted in its own tone, and the message is plain.
// @lfy def/cli/main.lfy:main
fn diagnostic_text(diagnostic: &Diagnostic, on: bool) -> String {
    // The file, a colon, the line, a colon, the column, a colon, the stage, the severity,
    // and the message; the place is subject and the stage is muted.
    // @lfy def/cli/main.lfy:main#main:main:22fc3d70b2c14966225f4598112d8f2dbef65184de8982f10df4538cbe785e25
    // @lfy def/cli/main.lfy:main#main:main:2729826dde4194a5d195a8c4990c0f22f3d75eec87f6a0246e625d5120ac05b7
    let place = format!("{}:{}:{}:", diagnostic.range.file, diagnostic.range.start.line, diagnostic.range.start.column);
    format!(
        "{} {} {}: {}",
        paint(&place, Tone::Subject, on),
        paint(diagnostic.stage.value(), Tone::Muted, on),
        // @lfy def/cli/main.lfy:main#main:main:2729826dde4194a5d195a8c4990c0f22f3d75eec87f6a0246e625d5120ac05b7
        paint(diagnostic.severity.value(), severity_tone(diagnostic.severity), on),
        diagnostic.message
    )
}

// @lfy def/cli/main.lfy:main#main:main:22fc3d70b2c14966225f4598112d8f2dbef65184de8982f10df4538cbe785e25
fn print_diagnostic(diagnostic: &Diagnostic, json: bool, on: bool) {
    if json {
        // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
        let value = serde_json::json!({
            "file": diagnostic.range.file,
            "line": diagnostic.range.start.line,
            "column": diagnostic.range.start.column,
            "stage": diagnostic.stage.value(),
            "severity": diagnostic.severity.value(),
            "message": diagnostic.message,
        });
        println!("{value}");
    } else {
        println!("{}", diagnostic_text(diagnostic, on));
    }
}

/// One of [`Plan::problems`] printed as an error of stage generation at its node: a cycle
/// among files that no move could resolve, so there is no order to build them in.
// @lfy def/cli/inspect.lfy:check#check:check:2c23167a93d05731803b314b2b6545c5ee1481978f5c546aacb5c0b7f512115a
// @lfy def/cli/compile.lfy:compile#compile:compile:055ed621ccb4cfc04dafc60c4ae42b146676fbac4139bd00ac26ddae87c1ad70
fn print_plan_problem(workspace: &Workspace, problem: &Problem, json: bool, on: bool) {
    let range = query::range_of_node(workspace, problem.node);
    if json {
        // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
        println!(
            "{}",
            serde_json::json!({
                "file": range.file,
                "line": range.start.line,
                "column": range.start.column,
                "stage": "generation",
                "severity": Severity::Error.value(),
                "message": problem.message,
            })
        );
    } else {
        let place = format!("{}:{}:{}:", range.file, range.start.line, range.start.column);
        println!(
            "{} {} {}: {}",
            paint(&place, Tone::Subject, on),
            paint("generation", Tone::Muted, on),
            paint(Severity::Error.value(), severity_tone(Severity::Error), on),
            problem.message
        );
    }
}

/// The identifiers of the entities one of [`Plan::moves`] moved, in the file order of the
/// file that declares them.
// @lfy def/cli/main.lfy:main
fn moved_entities(workspace: &Workspace, moved: &Move) -> String {
    moved
        .entities
        .iter()
        .map(|&entity| workspace.model.entities[entity].identifier.as_deref().unwrap_or("anonymous"))
        .collect::<Vec<_>>()
        .join(", ")
}
/// The heading below which a person writes the answer to a question the compiler asked.
// @lfy def/cli/compile.lfy:compile#compile:compile:a3ff4cc29185e0a608fd124eb6e98d73490a313012922426815ffdaaf23e2e2a
const ANSWER_HEADING: &str = "## The answer";

/// Every step, so that the step column keeps one width for the whole compile.
// @lfy def/cli/compile.lfy:compile
const STEPS: [Step; 26] = [
    Step::Planned,
    Step::Requesting,
    Step::Resuming,
    Step::Rewording,
    Step::Relocated,
    Step::Started,
    Step::Building,
    Step::Compiling,
    Step::Checking,
    Step::Accepted,
    Step::Merged,
    Step::Undone,
    Step::Rejected,
    Step::Verifying,
    Step::Reviewed,
    Step::GlobalVerifying,
    Step::GlobalReviewed,
    Step::Retrying,
    Step::Blocked,
    Step::Clarification,
    Step::Failed,
    Step::Touched,
    Step::Stubbed,
    Step::FinalGate,
    Step::Merging,
    Step::Finished,
];

/// The width of the step column: the longest name of [`Step`] with its glyph, so every unit
/// starts in the same column.
// @lfy def/cli/compile.lfy:compile#compile:compile:e6a992a863aebc985eeb11dd2cef62220e5e7590ea2500babb4124684929657c
fn step_column() -> usize {
    STEPS.iter().map(|step| width(glyph_of(*step)) + 1 + width(step.name())).max().unwrap_or(0)
}

/// How many columns the time column takes.
// @lfy def/cli/compile.lfy:compile#compile:compile:2ba85a50267062837687f356e2de2edec47dfcbaf012913034645e8c9586d86c
const TIME_COLUMN: usize = 6;

/// How many spaces a problem, a reason, or a question that follows a progress line is
/// indented by.
// @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
const INDENT: &str = "    ";

/// The counts a reviewed, globalReviewed, or finished line reports: how many, what they
/// count, and the tone each is painted in when it is not 0.
// @lfy def/cli/main.lfy:main
type Counts = Vec<(usize, &'static str, Tone)>;

/// The counts as one message: each a [`tally`], separated by a comma and a space. Painted or
/// not, it holds the same words, so the message a script reads with --json is the one a
/// person reads.
// @lfy def/cli/compile.lfy:compile#compile:compile:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
// @lfy def/cli/compile.lfy:compile#compile:compile:081257eb508c08a673bc5c569b3099485967e6b85b434c98d303c2de2d9958c5
fn counts_message(counts: &Counts, on: bool) -> String {
    counts.iter().map(|(count, label, tone)| tally(*count, label, *tone, on)).collect::<Vec<_>>().join(", ")
}

/// The counts of satisfied, violated, and unverifiable reviews, in that order, each with the
/// tone it is painted in, with every id the review was asked about that no review names
/// counted as unverifiable: nothing answered for it, which is what unverifiable means.
// @lfy def/cli/compile.lfy:compile#compile:compile:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
// @lfy def/cli/compile.lfy:compile#compile:compile:8533b0690caaa0d5a3703e6633b44fd6034ce112c1a2c32f151e384e973b8bdf
fn counts_with_missing(reviews: &[Review], missing: usize) -> Counts {
    let (satisfied, violated, unverifiable) = counts_of(reviews);
    vec![
        (satisfied, "satisfied", Tone::Success),
        (violated, "violated", Tone::Failure),
        // @lfy def/cli/compile.lfy:compile#compile:compile:8533b0690caaa0d5a3703e6633b44fd6034ce112c1a2c32f151e384e973b8bdf
        (unverifiable + missing, "unverifiable", Tone::Warning),
    ]
}

/// The same counts, of the reviews of one run of the verifier alone, with the verdicts its
/// review request carried counted after them and only there: a carried verdict is no review
/// of this run, so it is neither satisfied, violated, nor unverifiable here.
///
/// With nothing carried the counts end with the unverifiable, as they do for a global review,
/// which carries nothing.
// @lfy def/cli/compile.lfy:compile#compile:compile:79aeca787363a11f87e229a581c29dfcab7548d25cd5e7e3018983a54dbcb14d
// @lfy def/cli/compile.lfy:compile#compile:compile:42cb7931abf1b1cbc69474e94e4981f4280630610c6622305d2f9ce3dab316d1
// @lfy def/cli/compile.lfy:compile#compile:compile:0ae375c1084691369dda163c81e2e1f335de4780dfb63290fe31e727ca79fec3
// @lfy def/cli/compile.lfy:verify#verify:verify:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
fn counts_with_carried(reviews: &[Review], missing: usize, carried: usize) -> Counts {
    let mut counts = counts_with_missing(reviews, missing);
    // The count of carried verdicts and the word carried, painted muted: a verdict nothing
    // had to look at again is worth noticing least of all.
    // @lfy def/cli/compile.lfy:compile#compile:compile:79aeca787363a11f87e229a581c29dfcab7548d25cd5e7e3018983a54dbcb14d
    // @lfy def/cli/compile.lfy:compile#compile:compile:01ac2a8e3e988e7c07b647cd40cc38bce72044f98971d9e5e72eb5c62425cc0e
    if carried > 0 {
        counts.push((carried, "carried", Tone::Muted));
    }
    counts
}

/// One progress line, in columns separated by two spaces: the counter, the time, the step,
/// then the unit when there is one, and the first line of the message. A reason or a question
/// runs to several lines, so the line carries only its first; the whole of it follows,
/// indented.
///
/// The counter and the time are muted, the step is its own tone unless one is given, the
/// unit is subject, and the message is the step's tone for a rejection, a failure, a block,
/// or a question, the counts when there are counts, and plain otherwise.
// @lfy def/cli/compile.lfy:compile#compile:compile:e644285bff17055757f9da45f329dc88d9e4ec34a19c3b5ce92b8a4a03950621
fn text_of(progress: &Progress, counts: Option<&Counts>, tone: Option<Tone>, on: bool) -> String {
    // The counter keeps one width for the whole compile.
    // @lfy def/cli/compile.lfy:compile#compile:compile:a8367555ef05292d4118872e01aece83edde2d5816e9e4c404840757608603d3
    let digits = progress.total.to_string().len();
    let counter = paint(&format!("[{:>digits$}/{}]", progress.done, progress.total), Tone::Muted, on);
    // @lfy def/cli/compile.lfy:compile#compile:compile:2ba85a50267062837687f356e2de2edec47dfcbaf012913034645e8c9586d86c
    let time = paint(&format!("{:>TIME_COLUMN$}", duration(progress.elapsed)), Tone::Muted, on);
    // @lfy def/cli/compile.lfy:compile#compile:compile:e6a992a863aebc985eeb11dd2cef62220e5e7590ea2500babb4124684929657c
    let named = format!("{} {}", glyph_of(progress.step), progress.step.name());
    let step = padded(&paint(&named, tone.unwrap_or_else(|| tone_of(progress.step)), on), step_column());
    let mut columns = vec![counter, time, step];
    // The unit a step concerns follows the step, painted subject.
    // @lfy def/cli/compile.lfy:compile#compile:compile:679737b9c65e8c1627af4566b3a9f5d490c8790929b3f394353619a617003c6e
    if let Some(unit) = &progress.unit {
        columns.push(paint(unit, Tone::Subject, on));
    }
    // @lfy def/cli/compile.lfy:compile#compile:compile:e644285bff17055757f9da45f329dc88d9e4ec34a19c3b5ce92b8a4a03950621
    let message = match counts {
        // @lfy def/cli/compile.lfy:compile#compile:compile:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
        Some(counts) => counts_message(counts, on),
        None => {
            let first = progress.message.lines().next().unwrap_or_default();
            match progress.step {
                // @lfy def/cli/compile.lfy:compile#compile:compile:c164c51236df72a93cac3a3c7bcc44ec0848586ed13d2f9add36192db3bc1999
                Step::Rejected | Step::Failed | Step::Blocked | Step::Clarification => {
                    paint(first, tone_of(progress.step), on)
                }
                // @lfy def/cli/compile.lfy:compile#compile:compile:fa43f8b6da9b3c369ebca761ab832f180a9bc8eb02b280dd2435250f4884c825
                Step::Planned
                | Step::Requesting
                | Step::Resuming
                | Step::Rewording
                | Step::Relocated
                | Step::Started
                | Step::Building
                | Step::Compiling
                | Step::Checking
                | Step::Accepted
                | Step::Merged
                | Step::Undone
                | Step::Touched
                | Step::Stubbed
                | Step::Verifying
                | Step::GlobalVerifying
                | Step::FinalGate
                | Step::Merging
                | Step::Retrying => first.to_string(),
                // A line of counts whose counts were not given, which only a reviewed,
                // globalReviewed, or finished line carries, reads as it was written.
                // @lfy def/cli/compile.lfy:compile#compile:compile:e644285bff17055757f9da45f329dc88d9e4ec34a19c3b5ce92b8a4a03950621
                Step::Reviewed | Step::GlobalReviewed | Step::Finished => first.to_string(),
            }
        }
    };
    // The line ends with the first line of the message.
    // @lfy def/cli/compile.lfy:compile#compile:compile:72582398057ef4b458680631986c9aa55de4061eecc790f79ca14cc77a71b86d
    if !message.is_empty() {
        columns.push(message);
    }
    // The columns are separated by two spaces.
    // @lfy def/cli/compile.lfy:compile#compile:compile:fd851430001b39d2d1c79a0807242453986bf07f0fbe972e5ecd9815c3380aba
    columns.join("  ")
}

/// One progress line as one JSON object.
// @lfy def/cli/compile.lfy:compile#compile:compile:bbad3c69582995a56ff9152a6a483894f2d276f7b0d0b4805da999767796b37e
fn json_of(progress: &Progress) -> serde_json::Value {
    serde_json::json!({
        "step": progress.step.name(),
        "unit": progress.unit,
        "done": progress.done,
        "total": progress.total,
        "elapsed": progress.elapsed,
        "message": progress.message,
    })
}

/// Seconds since an instant, milliseconds kept as a fraction and never negative.
// @lfy def/cli/compile.lfy:compile
fn elapsed(start: SystemTime) -> f64 {
    SystemTime::now().duration_since(start).map_or(0.0, |since| since.as_secs_f64())
}

/// Prints one [`Progress`] line per step as it happens and appends it to the log. Every line
/// the log holds is the line as printed with colors off, so the log never holds an escape and
/// its layout is the printed layout.
///
/// Every unit that runs at once prints through a clone of the one reporter of its compile, so
/// the counts and whether a started line was printed yet are shared between them.
// @lfy def/cli/compile.lfy:compile#compile:compile:ce3b9a1a98759c3ae080fd41207a076d4853acdb0f87c1f511632082039e8a3e
#[derive(Clone)]
struct Reporter {
    json: bool,
    /// Whether each stream is painted; the log is written with nothing painted.
    // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
    style: Paint,
    started: SystemTime,
    logs: Vec<PathBuf>,
    /// What every clone of the reporter shares: the counter and whether a started line was
    /// printed.
    counter: Arc<Mutex<Counter>>,
}

/// The counter every progress line shows, and whether the compile printed a started line yet,
/// so that every started line after the first one has an empty line before it.
// @lfy def/cli/compile.lfy:compile#compile:compile:4630171e9007878ed9f3e54a7018754522e39bd151c749a09d46ba54f64b0a3e
#[derive(Default)]
struct Counter {
    total: usize,
    done: usize,
    // @lfy def/cli/compile.lfy:compile#compile:compile:4630171e9007878ed9f3e54a7018754522e39bd151c749a09d46ba54f64b0a3e
    begun: bool,
}

impl Reporter {
    // @lfy def/cli/main.lfy:main
    fn new(json: bool, style: Paint, total: usize, logs: Vec<PathBuf>) -> Reporter {
        let counter = Counter { total, ..Counter::default() };
        Reporter { json, style, started: SystemTime::now(), logs, counter: Arc::new(Mutex::new(counter)) }
    }

    /// The units merged so far and the units planned, as the counter shows them from the next
    /// line on.
    // @lfy def/cli/compile.lfy:compile#compile:compile:a8367555ef05292d4118872e01aece83edde2d5816e9e4c404840757608603d3
    fn set_counts(&self, done: usize, total: usize) {
        let mut counter = self.counter.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        counter.done = done;
        counter.total = total;
    }

    /// The units planned, as the counter shows them.
    fn total(&self) -> usize {
        self.counter.lock().unwrap_or_else(std::sync::PoisonError::into_inner).total
    }

    /// The turn to print, held while a line and what follows it are printed together.
    // @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
    fn hold(&self) -> Turn {
        turn()
    }

    /// One line for one step, printed and logged.
    // @lfy def/cli/compile.lfy:compile#compile:compile:ce3b9a1a98759c3ae080fd41207a076d4853acdb0f87c1f511632082039e8a3e
    // @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    fn report(&self, step: Step, unit: Option<&str>, message: &str) {
        self.emit(step, unit, message.to_string(), None, None);
    }

    /// One line whose message is the counts it reports, each painted only when it is worth
    /// noticing, then the text given after them.
    // @lfy def/cli/main.lfy:main
    fn report_counts(&self, step: Step, unit: Option<&str>, counts: &Counts, tone: Option<Tone>) {
        // The message with --json holds the same words the counts are printed with.
        // @lfy def/cli/compile.lfy:compile#compile:compile:081257eb508c08a673bc5c569b3099485967e6b85b434c98d303c2de2d9958c5
        let message = counts_message(counts, false);
        self.emit(step, unit, message, Some(counts), tone);
    }

    // @lfy def/cli/main.lfy:main
    fn emit(&self, step: Step, unit: Option<&str>, message: String, counts: Option<&Counts>, tone: Option<Tone>) {
        let _turn = self.hold();
        let (done, total, apart) = {
            let mut counter = self.counter.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // An empty line before every started line after the first one, and before
            // relocated, finalGate, globalVerifying, merging, and finished, so each reads as
            // one block.
            // @lfy def/cli/compile.lfy:compile#compile:compile:4630171e9007878ed9f3e54a7018754522e39bd151c749a09d46ba54f64b0a3e
            let apart = match step {
                Step::Started => std::mem::replace(&mut counter.begun, true),
                Step::Relocated | Step::FinalGate | Step::GlobalVerifying | Step::Merging | Step::Finished => true,
                _ => false,
            };
            (counter.done, counter.total, apart)
        };
        let progress = Progress {
            step,
            unit: unit.map(str::to_string),
            done,
            total,
            // @lfy def/cli/compile.lfy:compile#compile:compile:2ba85a50267062837687f356e2de2edec47dfcbaf012913034645e8c9586d86c
            elapsed: elapsed(self.started),
            message,
        };
        // The empty line is printed and logged.
        // @lfy def/cli/compile.lfy:compile#compile:compile:4630171e9007878ed9f3e54a7018754522e39bd151c749a09d46ba54f64b0a3e
        if !self.json && apart {
            println!();
            self.append("");
        }
        // @lfy def/cli/compile.lfy:compile#compile:compile:bbad3c69582995a56ff9152a6a483894f2d276f7b0d0b4805da999767796b37e
        if self.json {
            let line = json_of(&progress).to_string();
            println!("{line}");
            self.append(&line);
        } else {
            println!("{}", text_of(&progress, counts, tone, self.style.out));
            // Every line the log holds is the line as printed with colors off.
            // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
            self.append(&text_of(&progress, counts, tone, false));
        }
    }

    /// One line for one step with the problems that belong to it after it, printed in one
    /// turn, so no line of another unit comes between them.
    // @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
    fn report_problems(&self, step: Step, unit: Option<&str>, message: &str, problems: &[String]) {
        let _turn = self.hold();
        self.report(step, unit, message);
        self.problems(problems);
    }

    /// One line for one step with the reason or the question that belongs to it after it,
    /// printed in one turn.
    // @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
    fn report_reason(&self, step: Step, unit: Option<&str>, message: &str, reason: &str) {
        let _turn = self.hold();
        self.report(step, unit, message);
        self.reason(reason);
    }

    /// The problems of a step, following the line they belong to: one per line, each indented
    /// four spaces, beginning with `-` and a space and painted failure. With --json nothing
    /// follows, since the whole of the message is in the [`Progress`].
    // @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
    fn problems(&self, problems: &[String]) {
        if self.json {
            return;
        }
        let _turn = self.hold();
        for problem in problems {
            // A problem that runs to several lines, as one followed by the lines of a
            // command's output does, has the dash before its first line alone.
            // @lfy def/cli/compile.lfy:compile#compile:compile:7278a611f5028db0b60166d22b250e5673ae6472e74d4da368aacbd121b9843b
            for (at, line) in problem.lines().enumerate() {
                let body = if at == 0 { format!("- {line}") } else { format!("  {line}") };
                println!("{INDENT}{}", paint(&body, Tone::Failure, self.style.out));
                self.append(&format!("{INDENT}{body}"));
            }
        }
    }

    /// A reason or a question, following the line it belongs to: one line of text per line,
    /// each indented four spaces and painted warning.
    // @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
    fn reason(&self, text: &str) {
        if self.json {
            return;
        }
        let _turn = self.hold();
        for line in text.lines() {
            // @lfy def/cli/compile.lfy:compile#compile:compile:f74f012b6abed1f12784b91dd8349a19d7a6b3e65f13f043f7bf8dc07929bc01
            println!("{INDENT}{}", paint(line, Tone::Warning, self.style.out));
            self.append(&format!("{INDENT}{line}"));
        }
    }

    /// Appends text to `elfie-requests/compile.log` under the root, so a run can be read
    /// after the fact.
    // @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    // @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    fn append(&self, text: &str) {
        append_to(&self.logs, text);
    }
}

/// Which thread holds the turn to print, and how many times over it took it, so that a line
/// and what follows it are printed in one turn and the same thread may take it again inside.
// @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
static TURN: Mutex<Option<(std::thread::ThreadId, usize)>> = Mutex::new(None);

/// What a thread waiting for the turn is woken by.
// @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
static FREED: std::sync::Condvar = std::sync::Condvar::new();

/// The turn to print, held until it is dropped.
// @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
struct Turn;

/// The turn to print taken, once every other thread has given it up; a thread that holds it
/// already takes it once more.
// @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
fn turn() -> Turn {
    let me = std::thread::current().id();
    let mut held = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        match &mut *held {
            None => {
                *held = Some((me, 1));
                return Turn;
            }
            Some((owner, depth)) if *owner == me => {
                *depth += 1;
                return Turn;
            }
            Some(_) => held = FREED.wait(held).unwrap_or_else(std::sync::PoisonError::into_inner),
        }
    }
}

impl Drop for Turn {
    // @lfy def/cli/compile.lfy:compile#compile:compile:a7d36c6314ecefbcbce7f5f0d72190c18e20d5d6358618dd7d1063060f1ae77f
    fn drop(&mut self) {
        let mut held = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, depth)) = &mut *held {
            *depth -= 1;
            if *depth == 0 {
                *held = None;
                FREED.notify_all();
            }
        }
    }
}

/// The lock every line of a log is written under, so that the lines a command writes to its
/// standard output and to its standard error never mix: a line reaches the log whole or not
/// at all, as it reaches the terminal whole through one `println!`.
// @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
static LOGGING: Mutex<()> = Mutex::new(());

/// One line appended to each log, so a run can be read after the fact. Nothing that writes a
/// line to a person is without this, so the log holds everything the run showed.
// @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
// @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
fn append_to(logs: &[PathBuf], text: &str) {
    // @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    let _held = LOGGING.lock();
    for path in logs {
        let Some(parent) = path.parent() else { continue };
        // The folder it writes into is created when it is missing.
        // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
        if fs::create_dir_all(parent).is_err() {
            continue;
        }
        let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) else { continue };
        let _ = writeln!(file, "{text}");
    }
}

/// The command elfie.json names under a key, when it names one.
// @lfy def/cli/main.lfy:main
fn manifest_command(root: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(root.join("elfie.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get(key)?.as_str().map(str::to_string)
}

/// The manifest's `compiler` command, when it names one.
// @lfy def/cli/compile.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
fn compiler_command(root: &Path) -> Option<String> {
    manifest_command(root, "compiler")
}

/// The manifest's `verifier` command, when it names one: the second agent, which reads the
/// outputs and never writes.
// @lfy def/cli/main.lfy:main
fn verifier_command(root: &Path) -> Option<String> {
    manifest_command(root, "verifier")
}

/// Every local criterion and test of one unit, by id and sorted: for each of its entities,
/// the criteria of its declaration and then its tests, then those of its own file, which is
/// exactly what a review request computes a hash for. A unit's review records are written for
/// these, in this order.
// @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
fn requirement_ids_of(program: &Program, unit: &Unit) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for &entity in &unit.entities {
        // @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
        let Some(node) = declared_node(program, unit.lowered, entity) else { continue };
        out.extend(node.criteria.iter().map(|criterion| criterion.id.clone()));
        out.extend(node.tests.iter().map(|test| test.id.clone()));
    }
    // A criterion or test of the unit's own file is one of the unit's own all the same.
    // @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
    let file = &program.files[unit.lowered];
    out.extend(file.criteria.iter().map(|criterion| criterion.id.clone()));
    out.extend(file.tests.iter().map(|test| test.id.clone()));
    // Sorted by id, so the records of a unit are written in one order whatever the order its
    // entities were reviewed in.
    // @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
    out.sort();
    out.dedup();
    out
}

/// The lowered node that declares one entity of a unit: its declaration in the unit's own
/// lowered file, or, for an entity moved into the unit from another file, its declaration in
/// the file it came from, whose criteria and tests are the unit's to answer for just the
/// same.
// @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
fn declared_node(program: &Program, lowered: usize, entity: EntityId) -> Option<&LoweredNode> {
    // @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
    declaring_node(&program.files[lowered].root, entity)
        .or_else(|| program.files.iter().find_map(|file| declaring_node(&file.root, entity)))
}

/// The node of a lowered tree that declares an entity, the node itself before its children.
// @lfy def/cli/compile.lfy:compile#compile:compile:ce733e8816d1638bf89269929d621f9cc341aaee184834fbd66203ed30bfb6b1
fn declaring_node(node: &LoweredNode, entity: EntityId) -> Option<&LoweredNode> {
    if node.entity == Some(entity) {
        return Some(node);
    }
    node.nodes().find_map(|child| declaring_node(child, entity))
}

/// Every file under a directory, skipping build and version control directories.
// @lfy def/cli/main.lfy:main
fn walk_all(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target" || n == ".git" || n == "node_modules") {
                continue;
            }
            walk_all(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// A unit's request name: its stem with each `/` replaced by `-`, such as `cli-main` for
/// `def/cli/main.lfy`, since a stem holds slashes and a file name may not. `global` names the
/// global review the same way.
// @lfy def/cli/compile.lfy:compile#compile:compile:471b5ff5941e414fe0f5f5350c3d0f8c3560fdbfd6326589991ff388f7367356
fn request_name(stem: &str) -> String {
    stem.replace('/', "-")
}

/// The identifier of a unit's target.
// @lfy def/cli/main.lfy:main
fn target_of<'w>(workspace: &'w Workspace, unit: &Unit) -> &'w str {
    workspace.targets[unit.target].identifier.as_str()
}

/// The ids of every review the last global review found violated, as the agent server's
/// [`GlobalReview`] reads it from `elfie-requests/global.reviews.json` under the root; none
/// when there is no last global review, so that no unit is among its violated units.
///
/// The file it is kept in, what it holds, and which of its reviews are violated belong to the
/// agent server's data rather than here, because both this and the agent server plan with the
/// units the last global review found violated.
// @lfy def/cli/compile.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
fn violated_ids(root: &Path) -> BTreeSet<String> {
    // @lfy def/cli/compile.lfy:compile#compile:compile:db9055e3c9db5499ef9db628c8352be669b3c2eb83fc5533541757772f6fae75
    let Some(last) = GlobalReview::read(root) else {
        return BTreeSet::new();
    };
    // @lfy def/cli/compile.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
    last.violated_requirements().into_iter().collect()
}

/// The stems of every unit whose outputs, or whose maps among those given beside them, hold a
/// marker whose requirement is one of these ids. The maps given are those a kept change set's
/// copy holds, where a unit's markers are when the root never took its change set.
// @lfy def/cli/compile.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
fn stems_answering(workspace: &Workspace, plan: &Plan, ids: &BTreeSet<String>, beside: &[SourceMap]) -> Vec<String> {
    if ids.is_empty() {
        return Vec::new();
    }
    let answers = |map: &SourceMap| {
        // @lfy def/cli/compile.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
        map.markers.iter().any(|marker| marker.requirement.as_deref().is_some_and(|id| ids.contains(id)))
    };
    let mut stems: Vec<String> = plan
        .units
        .iter()
        .filter(|unit| {
            let target = target_of(workspace, unit);
            let source = &workspace.files[unit.file].path;
            unit.outputs.iter().any(answers)
                || beside.iter().filter(|map| map.target == target && &map.source == source).any(answers)
        })
        .map(|unit| unit.stem.clone())
        .collect();
    stems.sort();
    stems.dedup();
    stems
}

/// The workspace at the root loaded and planned once, with the stems requested, the stems
/// whose outputs a global review found violated, and the foreign edits kept change sets
/// recorded.
// @lfy def/cli/main.lfy:main
fn plan_with(
    root: &Path,
    maps: &[SourceMap],
    requested: &[String],
    violated: &[String],
    foreign_edits: &[ForeignEdit],
) -> (Program, Plan) {
    // @lfy def/cli/compile.lfy:compile#compile:compile:db9055e3c9db5499ef9db628c8352be669b3c2eb83fc5533541757772f6fae75
    generation::plan(workspace::load(root), maps, requested, violated, foreign_edits)
}

/// What a command plans with: the source maps, the stems requested, and the foreign edits,
/// so that a compile can make its plan again with the same inputs and other foreign edits.
// @lfy def/cli/compile.lfy:compile#compile:compile:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
#[derive(Clone, Default)]
struct Planning {
    maps: Vec<SourceMap>,
    requested: Vec<String>,
    violated: Vec<String>,
}

impl Planning {
    /// The plan made from these inputs and the foreign edits given; its order is the same
    /// whatever the foreign edits are, since only the definitions decide it.
    // @lfy def/cli/compile.lfy:compile#compile:compile:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
    fn plan(&self, root: &Path, foreign_edits: &[ForeignEdit]) -> (Program, Plan) {
        plan_with(root, &self.maps, &self.requested, &self.violated, foreign_edits)
    }
}

/// The inputs of a command's plan: the stems given as arguments, or every stem with --all,
/// requested, and the stems whose outputs answer for a global criterion or test the last
/// global review found violated as its violated units.
///
/// A stem is only known from a plan, so the plan is made once to learn which units answer for
/// a violated global criterion, and which units there are at all for --all.
// @lfy def/cli/main.lfy:main
fn planning_of(
    root: &Path,
    maps: &[SourceMap],
    requested: &[String],
    all: bool,
    violated: &BTreeSet<String>,
    beside: &[SourceMap],
) -> Planning {
    // The stems given as arguments are the requested units.
    // @lfy def/cli/compile.lfy:compile#compile:compile:7293a67c5e6d7c3cb9a59ca62501969dcb425b97ef7cc4ae74b048b61f3d56b0
    let mut planning = Planning { maps: maps.to_vec(), requested: requested.to_vec(), violated: Vec::new() };
    if violated.is_empty() && !all {
        return planning;
    }
    let (program, plan) = planning.plan(root, &[]);
    // @lfy def/cli/compile.lfy:compile#compile:compile:db9055e3c9db5499ef9db628c8352be669b3c2eb83fc5533541757772f6fae75
    planning.violated = stems_answering(&program.workspace, &plan, violated, beside);
    // --all requests every unit.
    // @lfy def/cli/compile.lfy:compile#compile:compile:d3dedcdb2bf5414990bf22aff315b8514e496a3ede1fc23133fa128ee0b4ef9d
    if all {
        planning.requested = plan.units.iter().map(|unit| unit.stem.clone()).collect();
    }
    planning
}

/// The plan of a command: the units planned, from the source maps, the stems requested, the
/// stems whose outputs answer for a global criterion or test the last global review found
/// violated, and the foreign edits given.
// @lfy def/cli/main.lfy:main
fn planned(
    root: &Path,
    maps: &[SourceMap],
    requested: &[String],
    all: bool,
    violated: &BTreeSet<String>,
    foreign_edits: &[ForeignEdit],
) -> (Program, Plan) {
    planning_of(root, maps, requested, all, violated, &[]).plan(root, foreign_edits)
}

/// `elfie-requests/compile.log` under the root, never under an output directory, so no
/// output is ever mistaken for the CLI's own files.
// @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
fn log_paths(root: &Path) -> Vec<PathBuf> {
    vec![requests_directory(root).join("compile.log")]
}

/// `elfie-requests` under the root: where the CLI's own files live, never under an output
/// directory.
// @lfy def/cli/compile.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
fn requests_directory(root: &Path) -> PathBuf {
    root.join("elfie-requests")
}

/// The counts of satisfied, violated, and unverifiable reviews, in that order.
// @lfy def/cli/compile.lfy:verify#verify:verify:aefab83034ace05b16e3a0289b1a6d708bca8805648197925ce1c4cb013db13d
fn counts_of(reviews: &[Review]) -> (usize, usize, usize) {
    let count = |status| reviews.iter().filter(|review| review.status == status).count();
    (count(ReviewStatus::Satisfied), count(ReviewStatus::Violated), count(ReviewStatus::Unverifiable))
}

/// What `git` wrote in a directory, when it ran and ended well.
// @lfy def/cli/compile.lfy:compile
fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output =
        Process::new("git").args(arguments).current_dir(directory).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The root's git directory, when the root is a git repository, as an absolute path, so that
/// the `.git` file of a copy names the same place from wherever the copy sits.
// @lfy def/cli/compile.lfy:compile#compile:compile:8a305990c26ec5a21e609fca17bc010a769c49511853e849d7b5b29db5b1b906
fn git_directory(root: &Path) -> Option<PathBuf> {
    git(root, &["rev-parse", "--absolute-git-dir"]).map(|text| PathBuf::from(text.trim()))
}

/// The target --target limits the plan to, when it names a known one.
// @lfy def/cli/compile.lfy:compile#compile:compile:ab0e6c732c90fb6b7fa9232e9b5f894e7f913233bb84f30a1d08a14830314820
// @lfy def/cli/compile.lfy:verify#verify:verify:aa8d8f7253225d4bbfb5d6995399aa58057a616f7d55984c01a6687dc0ff2ee9
fn target_named(workspace: &Workspace, invocation: &Invocation) -> Result<Option<String>, ExitCode> {
    let Some(name) = invocation.option("target") else {
        return Ok(None);
    };
    if !workspace.targets.iter().any(|known| known.identifier == name) {
        // A usage message naming the identifiers of the targets, nothing compiled or
        // reviewed, and the code is usage.
        // @lfy def/cli/compile.lfy:compile#compile:compile:022c51b85274d94f99424138e1f93c2421f285701b3ad67ff7eba4845f51626a
        // @lfy def/cli/compile.lfy:compile#compile:compile:315342ecb200e927c15546ab751e43c381772c3fa07f27e1d0f154a7c2b09afd
        let known: Vec<&str> = workspace.targets.iter().map(|t| t.identifier.as_str()).collect();
        complain(
            Paint::of(invocation).err,
            None,
            &format!("{name} is not a target; the targets are: {}", known.join(", ")),
        );
        // @lfy def/cli/compile.lfy:compile#compile:compile:305f0dfcde799b22957fd531d22657ae8b808df4f65b4301dbd9bbdf42a3e1cd
        return Err(ExitCode::Usage);
    }
    Ok(Some(name.to_string()))
}

// ---- what the tests of this unit are built from -------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::running::Run;

    mod arguments_tests;
    mod commands_tests;
    mod compiling_tests;
    mod copies_tests;
    mod maps_tests;
    mod printing_tests;
    mod processes_tests;
    mod schedule_tests;

    /// A project directory under the system's temporary directory, removed when dropped.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("elfie-cli-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("def")).unwrap();
            Fixture { root }
        }

        fn write(&self, path: &str, text: &str) -> &Fixture {
            let disk = self.root.join(path);
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(disk, text).unwrap();
            self
        }

        fn read(&self, path: &str) -> String {
            fs::read_to_string(self.root.join(path)).unwrap()
        }

        /// The root as an argument to `--root`.
        fn path(&self) -> String {
            self.root.to_string_lossy().into_owned()
        }

        /// One invocation, as the process would make it.
        fn run(&self, arguments: &[&str]) -> u8 {
            let mut all: Vec<String> = arguments.iter().map(|a| (*a).to_string()).collect();
            all.push("--root".to_string());
            all.push(self.path());
            run(&all)
        }

        /// One invocation run in a child process of this test binary, so that what it prints
        /// can be read: its code, its standard output, and its standard error. The child runs
        /// [`a_child_invocation`] alone, which runs the invocation and exits with its code.
        fn printed(&self, arguments: &[&str]) -> (u8, String, String) {
            let mut all: Vec<String> = arguments.iter().map(|a| (*a).to_string()).collect();
            all.push("--root".to_string());
            all.push(self.path());
            let output = Process::new(std::env::current_exe().unwrap())
                .args(["--exact", "tests::a_child_invocation", "--nocapture", "--test-threads=1"])
                .env(CHILD, all.join("\u{1f}"))
                .stdin(Stdio::null())
                .output()
                .unwrap();
            let code = output.status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(u8::MAX);
            (
                code,
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )
        }
    }

    /// The variable that hands a child process of this test binary the arguments of the one
    /// invocation it runs, separated by the unit separator.
    const CHILD: &str = "ELFIE_CLI_TEST_INVOCATION";

    /// The invocation a parent test handed this process, run as the executable runs it, and
    /// the process ended with its code; nothing when this process is no such child.
    #[test]
    fn a_child_invocation() {
        let Ok(arguments) = std::env::var(CHILD) else { return };
        let arguments: Vec<String> = arguments.split('\u{1f}').map(str::to_string).collect();
        // The test harness has begun a line of its own, which the invocation's first line is
        // not to share.
        println!();
        eprintln!();
        let code = run(&arguments);
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
        std::process::exit(i32::from(code));
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// The project manifest: the standard library beside the project, and the commands
    /// given. The target itself is declared in the program rather than here, so nothing
    /// names it, its output, or its dependencies.
    // @lfy def/cli/main.lfy:main
    fn manifest(commands: &[(&str, &str)]) -> String {
        let mut value = serde_json::json!({ "name": "p", "dependencies": { "elfie": "./lib" } });
        for (key, command) in commands {
            value[*key] = serde_json::Value::String((*command).to_string());
        }
        format!("{value}\n")
    }

    /// A project with one target `rust` that `global`'s targets hold, so that every entity
    /// of every project file is built for it, and one file. The package `elfie` beside it is
    /// the standard library every fixture is loaded against: its main file is the prelude,
    /// so `Target`, the roles its slots take, and `targetOf` are in scope everywhere.
    /// `def/roles.lfy` gives the language, the layout, and the ecosystem the target needs,
    /// and `def/targets.lfy` declares the `ace const` that holds it; neither file declares
    /// anything with a lowered node, so neither adds a unit.
    // @lfy def/cli/main.lfy:main
    fn one_target(fixture: &Fixture) -> &Fixture {
        library(fixture).write("def/roles.lfy", ROLES).write("def/targets.lfy", TARGETS)
    }

    /// The same project with the standard library beside it but no target declared, so that
    /// nothing is built for anything.
    // @lfy def/cli/main.lfy:main
    fn library(fixture: &Fixture) -> &Fixture {
        fixture
            .write("elfie.json", &manifest(&[]))
            .write("lib/main.lfy", LIBRARY)
            .write("lib/prelude/builtin.lfy", LIBRARY_BUILTIN)
            .write("lib/prelude/entity.lfy", LIBRARY_ENTITY)
            .write("lib/prelude/trait.lfy", LIBRARY_TRAIT)
            .write("lib/criteria/main.lfy", LIBRARY_CRITERIA)
            .write("lib/target/main.lfy", LIBRARY_TARGET)
    }

    /// The main file of the package `elfie`: the prelude, which brings the vocabulary a
    /// target is declared with into the scope of every file of the project.
    // @lfy def/cli/main.lfy:main
    const LIBRARY: &str = "use \"./prelude/builtin\";\nuse \"./prelude/entity\";\n\
                           use \"./prelude/trait\";\nuse \"./criteria/main\";\n\
                           use \"./target/main\";\n";
    /// The trait a builtin carries, which the library's own declarations need.
    // @lfy def/cli/main.lfy:main
    const LIBRARY_BUILTIN: &str = "trait builtin: `Performed by the compiler` { }\n";
    /// The context layer every entity is seen through: the lists a body adds to live there,
    /// and `apply` is a member of a trait's kind rather than a name nothing declares.
    // @lfy def/cli/main.lfy:main
    const LIBRARY_ENTITY: &str = "\
d Entity: `A declared thing seen through its context layer` {
  $knowledge: `What the compiler is given to read for it` = object[];
  $commands: `Shell commands for its operations` = object[];
  $targets: `The targets it is built for` = object[];
}
d Trait extends Entity: `A trait seen through its context layer` {
  $apply: `Applies the trait to a target and returns the trait` = (target: Entity) => Trait;
  $layer: `The trait as a layer of a target, with arguments for its parameters` = (...arguments: string[]) => object;
}
";
    /// `layer`, which chooses a trait as a layer with arguments.
    // @lfy def/cli/main.lfy:main
    const LIBRARY_TRAIT: &str = "use \"./builtin\";\n\
                                 fn layer(subject: trait, ...arguments: string[]) is builtin: \
                                 `A trait chosen as a layer of a target` => object;\n";
    /// The enums a knowledge item and a command name.
    // @lfy def/cli/main.lfy:main
    const LIBRARY_CRITERIA: &str = "\
enum KnowledgeKind: `What a piece of knowledge is` {
  reference = `documentation to read`,
  example = `working code to imitate`,
  definition = `Elfie source that defines it`,
  tool = `a tool of the agent server`,
}
enum Operation: `What a command is for` {
  install = `install`,
  add = `add`,
  build = `build`,
  test = `test`,
  lint = `lint`,
  format = `format`,
  run = `run`,
}
";
    /// `Target`, the roles its slots take, and the function that gives a record written in
    /// braces its type, as the package `elfie` declares them.
    // @lfy def/cli/main.lfy:main
    const LIBRARY_TARGET: &str = "\
use \"../prelude/builtin\";
trait target: `What every layer of a target is` {
  $markerComment: `How a line comment begins` = string;
  $provides: `Capabilities this layer gives its target` = string[];
  $requires: `Capabilities some layer of its target must provide` = string[];
  $outputExtensions: `Extensions besides the language's own` = string[];
  .provides = [];
  .requires = [];
  .outputExtensions = [];
}
trait targetLanguage extends target: `What its code is written in` {
  $fileExtension: `The extension of its files` = string;
  $stubLine: `The statement that replaces a body to stub it` = string | undefined;
  $stubMarker: `The text every stub line holds` = string | undefined;
}
trait targetRuntime extends target: `What runs its code` { }
trait targetPlatform extends target: `Where it runs` { }
trait targetEcosystem extends target: `Where its dependencies come from` {
  $ecosystem: `What that ecosystem is called` = string;
  $scriptRunner: `What runs a script its manifest names` = string;
}
trait targetFramework extends target: `What its code is built on` { }
trait targetInterface extends target: `What it offers` { }
trait targetLayout extends target: `Where its files go` { }
d Target is builtin: `One artifact the project is built into` {
  $output: `Where everything it writes goes` = string;
  $language: `What its code is written in` = trait;
  $runtime: `What runs its code` = trait | undefined;
  $platforms: `Where it runs` = trait[] | undefined;
  $ecosystem: `Where its dependencies come from` = trait | undefined;
  $frameworks: `What its code is built on` = trait[] | undefined;
  $interfaces: `What it offers` = trait[] | undefined;
  $layout: `Where its files go` = trait;
  $layers: `Anything else the compiler is told` = trait[] | undefined;
  $dependencies: `What its generated code needs` = object[] | undefined;
}
function targetOf(target: Target): `Gives a record written in braces its type` -> Target {
  return target;
}
";

    /// The language, the layout, and the ecosystem the fixture's target is built from: only
    /// traits, so this file declares nothing with a lowered node and adds no unit.
    // @lfy def/cli/main.lfy:main
    const ROLES: &str = "\
trait lang extends targetLanguage: `Rust` {
  .fileExtension = \"rs\";
  .markerComment = \"//\";
}
trait flat extends targetLayout: `One file per unit` { }
trait cargo extends targetEcosystem: `Crates from crates.io` {
  .ecosystem = \"cargo\";
}
";

    /// The `ace const` holding the target `rust`, writing to `out`, added to `global` so
    /// that every entity of every project file is built for it.
    // @lfy def/cli/main.lfy:main
    const TARGETS: &str = "\
use \"elfie/target\";
use \"./roles\";

ace const rust = targetOf({
  output = \"out\",
  language = lang,
  layout = flat,
  ecosystem = cargo,
});
global@targets.add(rust);
";

    /// A project with one target, `def/a.lfy` declaring A with a criterion on line 3, a
    /// compiler that writes an output with a marker for A and ends with `ELFIE: DONE`, and
    /// the verifier given. The compiler and the verifier run in the change set's copy, so what
    /// they note is written under the fixture's root, which `<notes>` names.
    // @lfy def/cli/main.lfy:main
    fn a_compiler_and_a_verifier<'f>(fixture: &'f Fixture, verifier: &str) -> &'f Fixture {
        one_target(fixture)
            .write("def/a.lfy", "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\n")
            .write("compiler.sh", &noted(fixture, A_COMPILER))
            .write("verifier.sh", &verifier_script(fixture, verifier))
            .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]))
    }

    /// A compiler that notes its instructions and that it ran, and writes an output with a
    /// marker for A.
    // @lfy def/cli/main.lfy:main
    const A_COMPILER: &str = "#!/bin/sh\ncat >> '<notes>/instructions.txt'\necho compile >> '<notes>/attempts.txt'\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:A\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho 'ELFIE: DONE'\n";

    /// A script with `<notes>` replaced by the fixture's root, where what it notes is read.
    fn noted(fixture: &Fixture, script: &str) -> String {
        script.replace("<notes>", &fixture.path())
    }

    /// Every criterion and test id of one lowered node and the nodes below it, in order.
    fn ids_of(node: &elfie_core::interpret::LoweredNode, out: &mut Vec<String>) {
        out.extend(node.criteria.iter().map(|criterion| criterion.id.clone()));
        out.extend(node.tests.iter().map(|test| test.id.clone()));
        for child in &node.children {
            if let Some(child) = child.as_node() {
                ids_of(child, out);
            }
        }
    }

    /// The id of the one criterion of `def/a.lfy`, as the program spells it: a review whose id
    /// names no criterion and no test of the program is no review, so a verifier script has to
    /// answer for the real one.
    fn criterion_id(fixture: &Fixture) -> String {
        let (program, _) = plan_with(&fixture.root, &[], &[], &[], &[]);
        let mut found = Vec::new();
        for lowered in &program.files {
            if program.workspace.files[lowered.file].path.ends_with("a.lfy") {
                ids_of(&lowered.root, &mut found);
            }
        }
        found.first().cloned().unwrap_or_else(|| panic!("def/a.lfy has no criterion"))
    }

    /// A verifier script with `<id>` replaced by the id of the criterion of `def/a.lfy`.
    fn verifier_script(fixture: &Fixture, script: &str) -> String {
        noted(fixture, script).replace("<id>", &criterion_id(fixture))
    }

    /// `verifier.sh` replaced by another script, its `<id>` filled in.
    fn write_verifier(fixture: &Fixture, script: &str) {
        let script = verifier_script(fixture, script);
        fixture.write("verifier.sh", &script);
    }

    /// A verifier writing one violated review of the criterion of `def/a.lfy`, then the end
    /// line.
    const VIOLATED: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> '<notes>/verifications.txt'\necho '{\"id\":\"<id>\",\"status\":\"violated\",\"evidence\":\"crates/a/src/a.rs:4-6\",\"note\":\"returns 0 for an empty list\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier writing one violated review of the criterion of `def/a.lfy` whose evidence
    /// names the output under src, then the end line.
    const VIOLATED_IN_SRC: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> '<notes>/verifications.txt'\necho '{\"id\":\"<id>\",\"status\":\"violated\",\"evidence\":\"src/a.rs:4-6\",\"note\":\"returns 0 for an empty list\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier writing one satisfied review of the criterion of `def/a.lfy` and no end
    /// line: it answered for everything it was asked about all the same.
    const UNENDED: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> '<notes>/verifications.txt'\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\n";

    /// A verifier writing the end line and no review at all: it answered for nothing it was
    /// asked about, so it is run once more.
    // @lfy def/cli/compile.lfy:compile#compile:compile:6c97f065ec22808dd6a71afa10fbb1b9f2c63c5fe6ab869aeeb129eeb68a59d1
    const NOTHING_REVIEWED: &str =
        "#!/bin/sh\ncat > /dev/null\necho verify >> '<notes>/verifications.txt'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier writing one satisfied review of the criterion of `def/a.lfy`, then the end
    /// line.
    const SATISFIED: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> '<notes>/verifications.txt'\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier that satisfies the criterion, writes a line that reads as an object but
    /// names no criterion, and then ends: a report that answered for every id it was asked
    /// about stands, whatever else it holds.
    // @lfy def/cli/compile.lfy:compile#compile:compile:c3711c547c11411e55264eed71019f21008660209ec5348f9c940bbad6ddeb23
    const SATISFIED_WITH_A_STRAY_OBJECT: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> '<notes>/verifications.txt'\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\necho '{\"thought\":\"I also looked at the tests\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier that satisfies the criterion and writes a line of its own to standard error,
    /// as an agent writes its progress there.
    const SATISFIED_WITH_NOISE: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> '<notes>/verifications.txt'\necho 'reading the outputs' >&2\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\necho 'ELFIE: REVIEWED'\n";

    /// The review records of a project removed, so that a review of it carries nothing and
    /// every criterion and test is asked about again.
    // @lfy def/cli/compile.lfy:verify#verify:verify:62ee8e26855d189d744062b262cfed91d039f0cd61f31e843738d7a60ec73d44
    fn forget_reviews(fixture: &Fixture) {
        let _ = fs::remove_dir_all(fixture.root.join("elfie-compile/reviews"));
    }

    /// How many lines a file the fixture wrote holds; none when it was never written.
    fn lines_of(fixture: &Fixture, path: &str) -> usize {
        fs::read_to_string(fixture.root.join(path)).map_or(0, |text| text.lines().count())
    }

    /// The tokens of a source text, which always lexes in these tests.
    fn tokens_of(source: &str) -> Vec<Token> {
        lex(source, Some("def/a.lfy")).unwrap()
    }

    /// Words, then a short list of them, as the arguments of one invocation.
    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_string()).collect()
    }

    /// A compiler that writes the output of the unit it is run for, so any unit of a project of
    /// several is accepted, and names each unit it was run for in `attempts.txt` under the
    /// fixture's root.
    // @lfy def/cli/main.lfy:main
    fn per_unit_compiler(fixture: &Fixture) -> String {
        noted(
            fixture,
            "#!/bin/sh\ncat >> '<notes>/instructions.txt'\nmkdir -p \"$ELFIE_ROOT/out\"\nstem=\"$ELFIE_UNIT\"\necho \"$stem\" >> '<notes>/attempts.txt'\nname=$(echo \"$stem\" | tr 'a-z' 'A-Z')\nprintf '// @lfy def/%s.lfy:%s\\npub struct %s {}\\n' \"$stem\" \"$name\" \"$name\" > \"$ELFIE_ROOT/out/$stem.rs\"\necho 'ELFIE: DONE'\n",
        )
    }

    /// One git command in the fixture's root; whether it ran and succeeded.
    fn git(fixture: &Fixture, arguments: &[&str]) -> bool {
        Process::new("git")
            .args(arguments)
            .current_dir(&fixture.root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// A fixture root made a git repository, so that a copy of it holds what git lists; false
    /// when there is no git to make one with.
    // @lfy def/cli/compile.lfy:compile#compile:compile:670dca2fbcb84c029ce58dc80e12e107f388904bf0cf79378ec5b4136e8b6097
    fn a_repository(fixture: &Fixture) -> bool {
        git(fixture, &["init", "-q"])
            && git(fixture, &["config", "user.email", "p@example.com"])
            && git(fixture, &["config", "user.name", "p"])
    }

    /// Whether a text is a version 4 UUID spelled as 36 lowercase characters in the 8-4-4-4-12
    /// form, with the version in the thirteenth character and the variant in the seventeenth.
    // @lfy def/cli/compile.lfy:compile#compile:compile:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    fn is_a_uuid(text: &str) -> bool {
        let groups: Vec<&str> = text.split('-').collect();
        groups.len() == 5
            && groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
            && text.chars().all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            && groups[2].starts_with('4')
            && "89ab".contains(groups[3].chars().next().unwrap_or('z'))
    }

    /// One compile of the fixture, made by hand so that a test can drive it, with the
    /// arguments given.
    // @lfy def/cli/main.lfy:main
    fn a_run(fixture: &Fixture, arguments: &[&str]) -> Run {
        let invocation = parse_arguments(&args(arguments)).unwrap();
        let maps = generation::source_maps_of(&workspace::load(&fixture.root), None);
        let planning = Planning { maps, ..Planning::default() };
        let (program, plan) = planning.plan(&fixture.root, &[]);
        let reporter = Reporter::new(invocation.flag("json"), Paint::of(&invocation), 1, Vec::new());
        Run::new(program, plan, planning, reporter, &invocation)
    }

    /// The maps one map file under the root holds, as JSON.
    // @lfy def/cli/compile.lfy:compile#compile:compile:3d902ee1bcf6d6cc1e1820560d8d0cb6595db90f02696457e1c34eeceb299755
    fn maps_of(fixture: &Fixture, path: &str) -> serde_json::Value {
        serde_json::from_str(&fixture.read(path)).expect("a map file of JSON")
    }
}
