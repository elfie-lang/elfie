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
use std::sync::{Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::SystemTime;

use elfie_core::generation::{
    self, Batch, Outcome, OutcomeKind, Output, Plan, REVIEWED, Reason, Request, Review, ReviewReport, ReviewStatus,
    SourceMap, Unit, Verdict,
};
use elfie_core::grammar::GrammarRule as _;
use elfie_core::interpret::{Program, lower};
use elfie_core::lexer::{Token, lex};
use elfie_core::parser::{Child, ErrorNode, Node, Tree, parse};
use elfie_core::query::{self, Diagnostic, Severity};
use elfie_core::workspace::{self, Workspace};

pub use data::{Command, ExitCode, Invocation, Progress, Step};
// @lfy def/cli/main.lfy:main
use style::{ColorChoice, Tone, colors_on, duration, glyph_of, padded, paint, reason_tone, severity_tone, status_tone, tally, tone_of, width};

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    std::process::ExitCode::from(run(&arguments))
}

/// The invocation the arguments spell, or the usage error they make.
// @lfy def/cli/main.lfy:parse
pub fn parse_arguments(arguments: &[String]) -> Result<Invocation, String> {
    // --help or -h anywhere means help; --version means version. Neither leaves an
    // argument that does not begin with a dash naming the command, so the arguments are
    // read only for the options and the root.
    // @lfy def/cli/main.lfy:parse#parse:parse:7bcf6480b22f39361f8cf17177eeb14d00016bfcdcae4c115113d275a3436eaf
    let forced = if arguments.iter().any(|a| a == "--help" || a == "-h") {
        Some(Command::Help)
    } else if arguments.iter().any(|a| a == "--version") {
        // @lfy def/cli/main.lfy:parse#parse:parse:30bfe1626040570c321d5980500d0c1db321cc4fbc6de0760fbd15fcb9d741df
        Some(Command::Version)
    } else {
        None
    };
    let mut command = None;
    let mut root = None;
    let mut positional = Vec::new();
    let mut options: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut i = 0;
    while i < arguments.len() {
        let argument = &arguments[i];
        // Neither --help, -h, nor --version is an option.
        // @lfy def/cli/main.lfy:parse#parse:parse:0c1236254ed2dc7214279b9f60c85ce5ef12cdbea38d4a41543459b1268af732
        if argument == "--help" || argument == "-h" || argument == "--version" {
            i += 1;
            continue;
        }
        // --tree followed by a file keeps working as the tree command; followed by no file
        // it names no command, so arguments holding nothing that does not begin with a dash
        // spell help as they would without it.
        // @lfy def/cli/main.lfy:parse#parse:parse:afa30017cb06f5ac342c5f674b5eeb1cce4fc47b8d28911799eee3c809dff1c0
        if argument == "--tree" {
            // @lfy def/cli/main.lfy:parse#parse:parse:afa30017cb06f5ac342c5f674b5eeb1cce4fc47b8d28911799eee3c809dff1c0
            if arguments.get(i + 1).is_some_and(|file| !file.starts_with('-')) {
                command = Some(Command::Tree);
            }
            i += 1;
            continue;
        }
        // --root <dir> or --root=<dir> sets the root.
        // @lfy def/cli/main.lfy:parse#parse:parse:c13f6c1685e6037f390557f71f93bf09b652b2e862a8aecc750e49f518a644d3
        if let Some(value) = argument.strip_prefix("--root=") {
            root = Some(value.to_string());
            i += 1;
            continue;
        }
        // @lfy def/cli/main.lfy:parse#parse:parse:c13f6c1685e6037f390557f71f93bf09b652b2e862a8aecc750e49f518a644d3
        if argument == "--root" {
            // @lfy def/cli/main.lfy:parse#parse:parse:1875232948f1e78b7879e17a6142dbc7d23a119e9a6954e465036b47195bed69
            let Some(value) = arguments.get(i + 1) else {
                return Err("--root needs a directory".to_string());
            };
            root = Some(value.clone());
            i += 2;
            continue;
        }
        // Every argument beginning with two dashes other than --root, --tree, --help, and
        // --version is an option.
        // @lfy def/cli/main.lfy:parse#parse:parse:0c1236254ed2dc7214279b9f60c85ce5ef12cdbea38d4a41543459b1268af732
        if let Some(name) = argument.strip_prefix("--") {
            // An option written --name=value gives the text value.
            // @lfy def/cli/main.lfy:parse#parse:parse:c6ef1cd5d1ee70ed5efe7e765138382e292ba2e31c91916182971283cce5fc2e
            if let Some((name, value)) = name.split_once('=') {
                // --color=value gives the text, and a value that is no ColorChoice is a
                // usage error.
                // @lfy def/cli/main.lfy:parse#parse:parse:5f6a4cec2476302e1f65b3f63fd1793dbdea9475e1d5856222fcb8f1396bc841
                if name == COLOR && ColorChoice::lookup(value).is_none() {
                    return Err(COLOR_USAGE.to_string());
                }
                // --jobs=value gives the text too, and a value that is no whole number of at
                // least 1 is a usage error.
                // @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
                if name == JOBS && !names_a_count(value) {
                    return Err(JOBS_USAGE.to_string());
                }
                options.insert(name.to_string(), Some(value.to_string()));
            } else if name == COLOR {
                // --color followed by auto, always, or never gives that value.
                // @lfy def/cli/main.lfy:parse#parse:parse:372de213ff4328da7d560b94bc55aaa40b317fbfb13ed63c19833a9b840bb5f9
                // Written with no equals sign and not followed by one, it is a flag and gives
                // true, read as always, and the next argument is left for what follows.
                // @lfy def/cli/main.lfy:parse#parse:parse:3494bf75f2354dcf6b331282875ea7d823eba517ad9b03427013e5bbd35f1fe2
                match arguments.get(i + 1).and_then(|value| ColorChoice::lookup(value)) {
                    Some(choice) => {
                        options.insert(COLOR.to_string(), Some(choice.value().to_string()));
                        i += 1;
                    }
                    None => {
                        options.insert(COLOR.to_string(), None);
                    }
                }
                i += 1;
                continue;
            } else if OPTIONS_WITH_VALUES.contains(&name) {
                // --target and --jobs are the options besides --root that take a following
                // value, and the option of that name gives it.
                // @lfy def/cli/main.lfy:parse#parse:parse:31d286a6e27d0b3b1aee3510e0c91b34c50892ae2bb9d66fe5da061ccb6d6d01
                // @lfy def/cli/main.lfy:parse#parse:parse:1875232948f1e78b7879e17a6142dbc7d23a119e9a6954e465036b47195bed69
                let Some(value) = arguments.get(i + 1) else {
                    return Err(format!("--{name} needs a value"));
                };
                // @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
                if name == JOBS && !names_a_count(value) {
                    return Err(JOBS_USAGE.to_string());
                }
                options.insert(name.to_string(), Some(value.clone()));
                i += 1;
            } else {
                // Every other option written with no equals sign is a flag and gives true,
                // never taking the next argument, so --json check still names the check
                // command.
                // @lfy def/cli/main.lfy:parse#parse:parse:d19a41c8634f25c25ba5bfe17c06df12102331e68a6fe8764d48bf0936fbcb24
                options.insert(name.to_string(), None);
            }
            i += 1;
            continue;
        }
        // The first argument that does not begin with a dash names the command.
        // @lfy def/cli/main.lfy:parse#parse:parse:06b91a7a77a0c2c5fe6d322ae3821056895ceb8a780ca7d3353fc4224244a7fa
        if command.is_none() && forced.is_none() {
            match Command::lookup(argument) {
                Some(found) => command = Some(found),
                // @lfy def/cli/main.lfy:parse#parse:parse:1875232948f1e78b7879e17a6142dbc7d23a119e9a6954e465036b47195bed69
                None => return Err(format!("{argument} is not a command")),
            }
        } else {
            // Every other one, the value --root, --target, --jobs, or --color took aside, is
            // positional.
            // @lfy def/cli/main.lfy:parse#parse:parse:7473c896dd710a7b0dcccc8450958126fa8e9c70c04172f551e9a551ff75afa3
            positional.push(argument.clone());
        }
        i += 1;
    }
    // With --help, -h, or --version among the arguments the invocation has no positional
    // arguments.
    // @lfy def/cli/main.lfy:parse#parse:parse:414b6a3814d9b33eb9ee85b7db8bcea349d0753972f675579d3a2645c4e54547
    if let Some(forced) = forced {
        return Ok(Invocation { command: forced, root: find_root(root.as_deref()), arguments: Vec::new(), options });
    }
    // Nothing that does not begin with a dash names a command, and no --version is given:
    // the command is help.
    // @lfy def/cli/main.lfy:parse#parse:parse:92c545c12e3310c439130278d8e5daad4eb1ad4428545e1880ccbfa2de0f549b
    Ok(Invocation {
        command: command.unwrap_or(Command::Help),
        root: find_root(root.as_deref()),
        arguments: positional,
        options,
    })
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
        // @lfy def/cli/main.lfy:main#main:main:ee8bd5829ee7fe5920ce7e7756f9c3871ae10116d21c76739cfd2e3e83afc97d
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
        // @lfy def/cli/main.lfy:main#main:main:a1b9e02d08f2535a6201db8032108d51a84e84fbbd9cdfcd5a0180242e66d82e
        if json {
            return Paint { out: false, err: false };
        }
        // @lfy def/cli/main.lfy:main#main:main:ee8bd5829ee7fe5920ce7e7756f9c3871ae10116d21c76739cfd2e3e83afc97d
        Paint { out: colors_on(choice, false), err: colors_on(choice, true) }
    }
}

/// What --color asked for: the value it names, always for the bare flag, and auto when it is
/// not given.
// @lfy def/cli/main.lfy:main
fn color_choice(invocation: &Invocation) -> ColorChoice {
    match invocation.options.get(COLOR) {
        // @lfy def/cli/main.lfy:main#main:main:b1010681b4061d13ca1450d333ca09ece4bc89d41df8d2952fafd2764e99997a
        None => ColorChoice::Auto,
        // @lfy def/cli/main.lfy:main
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
                // The bare flag is read as always. @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main#main:main:e4117eb242e540729b17f5396d7c22ad3c1767218252db7809b623a935bcaee9
    let mut line = paint("elfie:", Tone::Failure, on);
    // @lfy def/cli/main.lfy:main#main:main:3bc24d1134581e926c331b962023badb11e4b07515ab0a41fd3086faba51bab6
    if let Some(path) = path {
        line.push(' ');
        line.push_str(&paint(path, Tone::Subject, on));
        line.push(':');
    }
    line.push(' ');
    line.push_str(message);
    line
}

/// Options that take a value written as the next argument: --target is the one option
/// besides --root that takes a following value unconditionally, and --color takes one only
/// when it names a [`ColorChoice`].
// @lfy def/cli/main.lfy:parse
const OPTIONS_WITH_VALUES: [&str; 2] = ["target", "jobs"];

/// The option that says when the CLI colors what it prints.
// @lfy def/cli/main.lfy:parse
const COLOR: &str = "color";

/// The option that says how many batches run at once.
// @lfy def/cli/main.lfy:parse
const JOBS: &str = "jobs";

/// What a --jobs value that is no whole number of at least 1 is answered with.
// @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
const JOBS_USAGE: &str = "--jobs must be a whole number of at least 1";

/// How many batches run at once with no --jobs given.
// @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
const JOBS_BY_DEFAULT: usize = 4;

/// Whether text names a whole number of at least 1: what --jobs takes.
// @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
fn names_a_count(value: &str) -> bool {
    value.parse::<usize>().is_ok_and(|count| count >= 1)
}

/// How many batches a compile or a verify runs at once: what --jobs says, or 4.
// @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
fn jobs_of(invocation: &Invocation) -> usize {
    // Nothing but a whole number of at least 1 reaches here, since parse answers anything
    // else with a usage message.
    // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
    invocation
        .option(JOBS)
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&count| count >= 1)
        .unwrap_or(JOBS_BY_DEFAULT)
}

/// How many batches a compile runs at once: what --jobs says where a batch can have its own
/// copy of the root, and one where it cannot.
///
/// Outside a git repository nothing says what belongs to the project, so no batch gets a
/// copy and every one of them would run in the root itself, breaking each other's builds and
/// tests. They run one at a time there, in plan order, however --jobs is given.
// @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
fn compile_jobs(invocation: &Invocation, root: &Path) -> usize {
    // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
    match git_directory(root) {
        // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
        Some(_) => jobs_of(invocation),
        // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
        None => 1,
    }
}

/// How that option is written among the arguments.
// @lfy def/cli/main.lfy:parse
const COLOR_FLAG: &str = "--color";

/// What a --color value that is no [`ColorChoice`] is answered with.
// @lfy def/cli/main.lfy:parse
const COLOR_USAGE: &str = "--color must be auto, always, or never";

/// The nearest directory at or above the current directory where elfie.json exists, or
/// the current one.
// @lfy def/cli/main.lfy:parse
fn find_root(given: Option<&str>) -> String {
    // The directory --root named is the root.
    // @lfy def/cli/main.lfy:parse#parse:parse:c13f6c1685e6037f390557f71f93bf09b652b2e862a8aecc750e49f518a644d3
    if let Some(given) = given {
        return given.to_string();
    }
    let mut current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let start = current.clone();
    loop {
        // @lfy def/cli/main.lfy:parse#parse:parse:92a69bd92ba69e35f172e62ee820fff5befe5e882a02dec0f3244e94f3e80d88
        if current.join("elfie.json").exists() {
            return current.to_string_lossy().into_owned();
        }
        // @lfy def/cli/main.lfy:parse#parse:parse:6527e686f555f618a19c2de3e492ca76ae143f4dfb6ca3080cc02a8ced1e2374
        if !current.pop() {
            return start.to_string_lossy().into_owned();
        }
    }
}

/// Run one command and return its exit code.
// @lfy def/cli/main.lfy:main
pub fn run(arguments: &[String]) -> u8 {
    let invocation = match parse_arguments(arguments) {
        Ok(invocation) => invocation,
        Err(message) => {
            // A usage message is printed to standard error with the help text and the code
            // is usage. The arguments spell no invocation, so --json and --color are read
            // from them as they are written.
            // @lfy def/cli/main.lfy:main#main:main:4735f9367c38c85e1d376224e3d41d069d86e48f4693749e103b593c55ad7150
            let style = Paint::of_arguments(arguments);
            complain(style.err, None, &format!("{message}\n"));
            eprintln!("{}", help_text(style.err));
            return ExitCode::Usage.code();
        }
    };
    let code = match invocation.command {
        Command::Help => help(&invocation),
        Command::Version => version(),
        Command::Init => init(&invocation),
        Command::Check => check(&invocation),
        Command::Format => format_command(&invocation),
        Command::Tree => tree(&invocation),
        Command::Tokens => tokens(&invocation),
        Command::Compile => compile(&invocation),
        // @lfy def/cli/main.lfy:main
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

/// The global options help lists: how each is written and one line of what it does.
// @lfy def/cli/main.lfy:main
const OPTIONS: [(&str, &str); 14] = [
    ("--root <dir>", "The project directory (default: the nearest elfie.json above the current directory)"),
    ("--json", "Print results as JSON lines"),
    ("--color <choice>", "When to color what is printed: auto, always, or never (default: auto)"),
    ("--strict", "check: fail on any warning, error, or stale unit"),
    ("--all", "compile: request every unit, up to date or not"),
    ("--target <name>", "compile, verify: only the target named"),
    ("--dry-run", "compile: print the plan and generate nothing"),
    ("--accept", "compile: check the outputs already on disk instead of running the compiler"),
    ("--continue", "compile: keep running the batches that do not depend on a stopped one"),
    ("--no-verify", "compile: do not run the verifier on a batch whose units were accepted"),
    ("--jobs <count>", "compile, verify: how many batches to run at once (default: 4)"),
    ("--global", "verify: review only the global criteria and tests"),
    ("--help, -h", "Print this help"),
    ("--version", "Print the version"),
];

/// Every command and every option, with one line of description each.
///
/// The usage line and the headings are subject; each command name and each option is active
/// and padded so every description starts in the same column; descriptions are plain.
// @lfy def/cli/main.lfy:main
fn help_text(on: bool) -> String {
    // @lfy def/cli/main.lfy:main#main:main:6ee407c6393a2c3323984dfc984860009bcc5b7e05fb79ca89988c26ed377004
    let column = Command::ALL
        .iter()
        .map(|command| width(command.value()))
        .chain(OPTIONS.iter().map(|(name, _)| width(name)))
        .max()
        .unwrap_or(0)
        + 2;
    let mut out = String::new();
    // The usage line and the headings are subject.
    // @lfy def/cli/main.lfy:main#main:main:6ee407c6393a2c3323984dfc984860009bcc5b7e05fb79ca89988c26ed377004
    out.push_str(&paint("usage: elfie <command> [arguments] [--root <dir>] [--json]", Tone::Subject, on));
    out.push_str("\n\n");
    out.push_str(&paint("commands:", Tone::Subject, on));
    out.push('\n');
    // Every command of Command, verify among them, with one line of description.
    // @lfy def/cli/main.lfy:main#main:main:1209be9acca3fe8503b9fbe8c9175d7c0272512b07cdcfd341cac2ca4696dfaa
    for command in Command::ALL {
        let name = padded(&paint(command.value(), Tone::Active, on), column);
        out.push_str(&format!("  {name}{}\n", command.description()));
    }
    out.push('\n');
    // @lfy def/cli/main.lfy:main#main:main:6ee407c6393a2c3323984dfc984860009bcc5b7e05fb79ca89988c26ed377004
    out.push_str(&paint("options:", Tone::Subject, on));
    out.push('\n');
    // The global options, --no-verify, --strict, --jobs, and --color among them.
    // @lfy def/cli/main.lfy:main#main:main:1209be9acca3fe8503b9fbe8c9175d7c0272512b07cdcfd341cac2ca4696dfaa
    for (name, description) in OPTIONS {
        let name = padded(&paint(name, Tone::Active, on), column);
        out.push_str(&format!("  {name}{description}\n"));
    }
    out
}

/// Prints every command, verify among them, with one line of description and the global
/// options, --no-verify, --strict, --jobs, and --color among them.
// @lfy def/cli/main.lfy:main#main:main:1209be9acca3fe8503b9fbe8c9175d7c0272512b07cdcfd341cac2ca4696dfaa
fn help(invocation: &Invocation) -> ExitCode {
    print!("{}", help_text(Paint::of(invocation).out));
    ExitCode::Success
}

/// Prints the version of the executable.
// @lfy def/cli/main.lfy:main#main:main:8003da025a3516fee0798c7c05b78202d7e3a9121630d51bbcc38d167cb91d77
fn version() -> ExitCode {
    println!("elfie {}", env!("CARGO_PKG_VERSION"));
    ExitCode::Success
}

/// The one line .gitignore holds for a project the CLI set up: the cache is derivable and
/// nothing else under elfie-compile is, since the maps are checked in.
// @lfy def/cli/main.lfy:main#main:main:2e29257126f4e16946ac795078a4f2268a21bd84f8b9f7355db8d24fd183e954
const IGNORED: &str = "/elfie-compile/cache/";

/// Where a unit's source maps are recorded, as [`generation::map_file`] spells it; init
/// creates it so a project has it before its first compile.
///
/// The maps live beside the cache rather than in it, so nothing under
/// `elfie-compile/cache` is ever the only record of anything: a command run after the cache
/// is deleted gives the same plan, requests, verdicts, and outputs as one run with it.
// @lfy def/cli/main.lfy:main#main:main:2e29257126f4e16946ac795078a4f2268a21bd84f8b9f7355db8d24fd183e954
// @lfy def/cli/main.lfy:main#main:main:771e545a60806e91b890eb4af7eb959bf8e8df680ca9fcf262ca16ac3546d3c6
const MAPS: &str = "elfie-compile/maps";

/// Creates elfie.json in the root naming the project after the argument, or the directory,
/// creates the source directory and elfie-compile/maps, and leaves .gitignore ignoring the
/// cache; fails when elfie.json already exists.
// @lfy def/cli/main.lfy:main
fn init(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let on = Paint::of(invocation).err;
    let manifest = root.join("elfie.json");
    // @lfy def/cli/main.lfy:main#main:main:e57bc55bcf30f1c92f97fb48fdfcd31bcee9df8ef258ff2894b5f583d2198ad6
    if manifest.exists() {
        complain(on, Some(&manifest.display().to_string()), "already exists");
        return ExitCode::Failure;
    }
    // The project is named after the argument.
    // @lfy def/cli/main.lfy:main#main:main:bf7b614b3f32c18097d431254884622107c5689cc4d3fced921b99dc3e3d5bf9
    // With no argument it is named after the directory.
    // @lfy def/cli/main.lfy:main#main:main:a3eeaabe800e22ed6e55aa4ec9dd596314099d5af29c955306e53f7f52b1d76d
    let name = invocation
        .arguments
        .first()
        .cloned()
        .or_else(|| root.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())))
        .unwrap_or_else(|| "project".to_string());
    let text = format!("{{\n  \"name\": {}\n}}\n", serde_json::Value::String(name));
    // The source directory and elfie-compile/maps, where the maps are recorded and checked
    // in; nothing ignores elfie-compile itself or the maps.
    // @lfy def/cli/main.lfy:main#main:main:bf7b614b3f32c18097d431254884622107c5689cc4d3fced921b99dc3e3d5bf9
    // @lfy def/cli/main.lfy:main#main:main:a3eeaabe800e22ed6e55aa4ec9dd596314099d5af29c955306e53f7f52b1d76d
    // @lfy def/cli/main.lfy:main#main:main:2e29257126f4e16946ac795078a4f2268a21bd84f8b9f7355db8d24fd183e954
    let written = fs::create_dir_all(root.join("def"))
        .and_then(|()| fs::create_dir_all(root.join(MAPS)))
        .and_then(|()| fs::write(&manifest, text));
    if let Err(error) = written {
        complain(on, Some(&manifest.display().to_string()), &error.to_string());
        return ExitCode::Failure;
    }
    // @lfy def/cli/main.lfy:main#main:main:b0f8f64480e8e96f28bea6c62581aa0aa2ae0b170dc064b189c34b72bac42fb8
    // @lfy def/cli/main.lfy:main#main:main:8bee067797e211fa966a09e5847cda634635e838280f400f6d3e6e8fa068bda1
    if let Err(error) = ignore_the_cache(root) {
        complain(on, Some(".gitignore"), &error.to_string());
        return ExitCode::Failure;
    }
    // @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
    println!("{}", created_line(invocation.flag("json"), &manifest.display().to_string()));
    ExitCode::Success
}

/// What init prints for the project it set up: one object with --json, so nothing but JSON
/// reaches standard output, and the text otherwise.
// @lfy def/cli/main.lfy:main
fn created_line(json: bool, manifest: &str) -> String {
    // @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
    if json {
        return serde_json::json!({ "file": manifest, "created": true }).to_string();
    }
    // @lfy def/cli/main.lfy:main
    format!("created {manifest}")
}

/// `.gitignore` in the root holding one line reading `/elfie-compile/cache/`: the file is
/// written when there is none, and the line is appended on a line of its own when the file
/// has no such line, so nothing else in it changes.
// @lfy def/cli/main.lfy:main
fn ignore_the_cache(root: &Path) -> io::Result<()> {
    let path = root.join(".gitignore");
    // @lfy def/cli/main.lfy:main#main:main:b0f8f64480e8e96f28bea6c62581aa0aa2ae0b170dc064b189c34b72bac42fb8
    let Ok(text) = fs::read_to_string(&path) else {
        return fs::write(&path, format!("{IGNORED}\n"));
    };
    // @lfy def/cli/main.lfy:main#main:main:8bee067797e211fa966a09e5847cda634635e838280f400f6d3e6e8fa068bda1
    if text.lines().any(|line| line.trim() == IGNORED) {
        return Ok(());
    }
    // @lfy def/cli/main.lfy:main#main:main:8bee067797e211fa966a09e5847cda634635e838280f400f6d3e6e8fa068bda1
    let mut file = fs::OpenOptions::new().append(true).open(&path)?;
    let lead = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
    writeln!(file, "{lead}{IGNORED}")
}

/// A diagnostic is printed as the file, a colon, the line, a colon, the column, a colon, the
/// stage, the severity, and the message.
///
/// Without --json the file, line, and column with their colons are subject, the stage is
/// muted, the severity is painted in its own tone, and the message is plain.
// @lfy def/cli/main.lfy:main
fn diagnostic_text(diagnostic: &Diagnostic, on: bool) -> String {
    // The file, a colon, the line, a colon, the column, a colon, the stage, the severity,
    // and the message; the place is subject and the stage is muted.
    // @lfy def/cli/main.lfy:main#main:main:60777ca25c879530c47c4447a595c062d50d0b8ad38da215e4a451c7bbb12c7c
    // @lfy def/cli/main.lfy:main#main:main:f4e8b8ebc4edea65a2b01a5da76c4387508643d7954a9d4572c9953776f011d6
    let place = format!("{}:{}:{}:", diagnostic.range.file, diagnostic.range.start.line, diagnostic.range.start.column);
    format!(
        "{} {} {}: {}",
        paint(&place, Tone::Subject, on),
        paint(diagnostic.stage.value(), Tone::Muted, on),
        // @lfy def/cli/main.lfy:main#main:main:f4e8b8ebc4edea65a2b01a5da76c4387508643d7954a9d4572c9953776f011d6
        paint(diagnostic.severity.value(), severity_tone(diagnostic.severity), on),
        diagnostic.message
    )
}

// @lfy def/cli/main.lfy:main#main:main:60777ca25c879530c47c4447a595c062d50d0b8ad38da215e4a451c7bbb12c7c
fn print_diagnostic(diagnostic: &Diagnostic, json: bool, on: bool) {
    if json {
        // @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
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

/// Load the root, then every diagnostic printed, or of the files given only; the code is
/// problems when any is an error and success otherwise. With no files given and no error,
/// each unit that is not up to date is also printed as a warning and counted, and --strict
/// fails on any warning, error, or stale unit.
// @lfy def/cli/main.lfy:main
fn check(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let on = Paint::of(invocation).out;
    // Every diagnostic of the program, or of the files given as arguments only, printed.
    // @lfy def/cli/main.lfy:main#main:main:5a7e92eb846986baee2d5e9805aa6c83a681a5112f647d5f8a251a185c24267e
    // @lfy def/cli/main.lfy:main#main:main:ed5ff32e9f44921b7929dc27a6538bdd1dd00bddf2e1de100f059571c4c43923
    let diagnostics = collect_diagnostics(&workspace, &invocation.arguments);
    for diagnostic in &diagnostics {
        print_diagnostic(diagnostic, json, on);
    }
    // @lfy def/cli/main.lfy:main#main:main:62048c8ca7167afb8dccb82b17611d4f4a1fff7804fe730afa69f230b6c8024e
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return ExitCode::Problems;
    }
    // With no files given, the plan is made with no stems requested, from the source maps of
    // the workspace and the stems the last global review found violated, and each unit that
    // is not up to date is printed as a warning of stage generation.
    // @lfy def/cli/main.lfy:main#main:main:f10c226ebebfe4eb9abf0d5eae513727904e676d3833fcffe51e2f58db885722
    let mut stale = 0usize;
    if invocation.arguments.is_empty() {
        // @lfy def/cli/main.lfy:main#main:main:f10c226ebebfe4eb9abf0d5eae513727904e676d3833fcffe51e2f58db885722
        let maps = generation::source_maps_of(&workspace, None);
        let (program, plan) = planned(root, &maps, &[], false, &violated_ids(root));
        for unit in &plan.units {
            let Some(reason) = unit.reason else { continue };
            stale += 1;
            print_stale(&program.workspace, unit, reason, json, on);
        }
    }
    // The last line counts the files, the problems, and the stale units: the count of files
    // is plain, the problems are a tally in failure, and the stale units one in warning.
    // @lfy def/cli/main.lfy:main#main:main:f10c226ebebfe4eb9abf0d5eae513727904e676d3833fcffe51e2f58db885722
    if json {
        // @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
        println!(
            "{}",
            serde_json::json!({ "files": workspace.files.len(), "problems": diagnostics.len(), "stale": stale })
        );
    } else {
        // @lfy def/cli/main.lfy:main#main:main:e4a92e90c92796a64c208f188a8f672fb3cb8c79eb89b30ec2f4c53b260c6d92
        println!(
            "{} files, {}, {}",
            workspace.files.len(),
            tally(diagnostics.len(), "problems", Tone::Failure, on),
            tally(stale, "stale units", Tone::Warning, on)
        );
    }
    // --strict: the code is problems when anything at all was printed as a warning or
    // error, a stale unit included.
    // @lfy def/cli/main.lfy:main#main:main:03dc28129d1c4a684c116c8cbfb1847cfcb0d7db07da121f8c5ff234afe74ed8
    if invocation.flag("strict") && (!diagnostics.is_empty() || stale > 0) {
        ExitCode::Problems
    } else {
        // @lfy def/cli/main.lfy:main#main:main:e31edb81ec3ee9b8748e2d4b9d4c9fdb554fad63e6baa85970c87f5cf7ac7075
        ExitCode::Success
    }
}

/// A unit that is not up to date, printed as a warning of stage generation at its source
/// file, naming the target, the unit, and the description of its reason; the target and the
/// unit are subject and the description is painted in the reason's own tone.
// @lfy def/cli/main.lfy:main#main:main:f10c226ebebfe4eb9abf0d5eae513727904e676d3833fcffe51e2f58db885722
fn print_stale(workspace: &Workspace, unit: &Unit, reason: Reason, json: bool, on: bool) {
    let file = &workspace.files[unit.file].path;
    let target = &workspace.targets[unit.target].identifier;
    let message = format!("{target} {}: {}", unit.stem, reason.value());
    if json {
        // @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
        println!(
            "{}",
            serde_json::json!({ "file": file, "stage": "generation", "severity": "warning", "message": message })
        );
    } else {
        // @lfy def/cli/main.lfy:main#main:main:109b71f38f376efeb23ef4a095d165ce23c6590d9e83b3a81a420ad3d544a9e6
        let message = format!(
            "{} {}: {}",
            paint(target, Tone::Subject, on),
            paint(&unit.stem, Tone::Subject, on),
            paint(reason.value(), reason_tone(Some(reason)), on)
        );
        println!(
            "{} {} {}: {message}",
            paint(&format!("{file}:"), Tone::Subject, on),
            paint("generation", Tone::Muted, on),
            paint("warning", severity_tone(Severity::Warning), on)
        );
    }
}

// @lfy def/cli/main.lfy:main
fn collect_diagnostics(workspace: &Workspace, files: &[String]) -> Vec<Diagnostic> {
    // @lfy def/cli/main.lfy:main#main:main:5a7e92eb846986baee2d5e9805aa6c83a681a5112f647d5f8a251a185c24267e
    if files.is_empty() {
        query::diagnostics_of(workspace, None)
    } else {
        // @lfy def/cli/main.lfy:main#main:main:ed5ff32e9f44921b7929dc27a6538bdd1dd00bddf2e1de100f059571c4c43923
        files.iter().flat_map(|file| query::diagnostics_of(workspace, Some(file))).collect()
    }
}

/// Each file given, or every .lfy file under the source directory, is replaced by the
/// standard layout when that differs; --check only reports.
// @lfy def/cli/main.lfy:main
fn format_command(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let check_only = invocation.flag("check");
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // Every .lfy file under the source directory, recursively, or each file given.
    // @lfy def/cli/main.lfy:main#main:main:1d0b1ab4401129accdd1a29fac4c5eb81857f849bbd63b314da9099ee0890a23
    // @lfy def/cli/main.lfy:main#main:main:3f404fcc81409252aac8e0779630c8170311dabe0e1bbcff51d5d1adc65a373e
    let files: Vec<PathBuf> = if invocation.arguments.is_empty() {
        let workspace = workspace::load(root);
        let mut found = Vec::new();
        walk(&root.join(&workspace.source_directory), &mut found);
        found.sort();
        found
    } else {
        // @lfy def/cli/main.lfy:main#main:main:7493114767bce2e3c09c9e660478cecb7d012c725695832fe9388a7a8fa526ec
        // @lfy def/cli/main.lfy:main#main:main:5e8bfaa9a386a337f7407c6ad7fb67ff40a18b598ffd1cd2aad693a3a68aaf5d
        invocation.arguments.iter().map(PathBuf::from).collect()
    };
    let mut code = ExitCode::Success;
    for path in files {
        let display = relative(root, &path);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                complain(style.err, Some(&display), &error.to_string());
                code = ExitCode::Failure;
                continue;
            }
        };
        let tokens = match lex(&source, Some(&display)) {
            Ok(tokens) => tokens,
            Err(error) => {
                complain(style.err, Some(&display), &error.to_string());
                code = ExitCode::Problems;
                continue;
            }
        };
        let tree = parse(tokens, None);
        // A file whose tree has errors has its errors printed, is left as it is, and the code
        // is problems.
        // @lfy def/cli/main.lfy:main#main:main:1b0e233552018abfd88cbf75ed64ac2dbce293e0c766bb9469107628a5fbce2c
        if !tree.errors.is_empty() {
            // @lfy def/cli/main.lfy:main#main:main:1b0e233552018abfd88cbf75ed64ac2dbce293e0c766bb9469107628a5fbce2c
            for error in &tree.errors {
                let token = tree.tokens.get(error.start).or(tree.tokens.last());
                let (line, column) = token.map_or((0, 0), |t| (t.line, t.column));
                let expected = error.expected.join(", ");
                let value = tree.raw(error.start, error.end);
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "file": display,
                            "line": line,
                            "column": column,
                            "stage": "parser",
                            "severity": "error",
                            "message": format!("expected [{expected}] but found {value:?}"),
                        })
                    );
                } else {
                    // @lfy def/cli/main.lfy:main#main:main:f4e8b8ebc4edea65a2b01a5da76c4387508643d7954a9d4572c9953776f011d6
                    println!(
                        "{} {} {}: expected [{expected}] but found {value:?}",
                        paint(&format!("{display}:{line}:{column}:"), Tone::Subject, style.out),
                        paint("parser", Tone::Muted, style.out),
                        paint("error", severity_tone(Severity::Error), style.out)
                    );
                }
            }
            code = ExitCode::Problems;
            continue;
        }
        // The file is replaced by the standard layout only where that differs from its text.
        // @lfy def/cli/main.lfy:main#main:main:26a10f2e65c863503fbb8e74864b9fbd5270ec5b736cb84e003e332c151facc8
        let formatted = elfie_core::format::format(&tree);
        if formatted == source {
            continue;
        }
        // Nothing is written; each file that would change is printed.
        // @lfy def/cli/main.lfy:main#main:main:f1176fb82acbd19538241e67b518efa48f67433cec1fd4b946b49f9271cd558a
        if check_only {
            // @lfy def/cli/main.lfy:main#main:main:5e8bfaa9a386a337f7407c6ad7fb67ff40a18b598ffd1cd2aad693a3a68aaf5d
            // @lfy def/cli/main.lfy:main#main:main:3f404fcc81409252aac8e0779630c8170311dabe0e1bbcff51d5d1adc65a373e
            println!("{}", formatted_line(json, &display, true, style.out));
            // @lfy def/cli/main.lfy:main#main:main:73ff7ea1548e739b29dba5c6ac46975dcd8b1bb63a36def282e586586f28782c
            code = ExitCode::Problems;
        } else if let Err(error) = fs::write(&path, formatted) {
            complain(style.err, Some(&display), &error.to_string());
            code = ExitCode::Failure;
        } else {
            // @lfy def/cli/main.lfy:main#main:main:d753a07acf7cd2e3524dbffd665b892b3ae9864de414a7f4b9c8f0d14544ce61
            println!("{}", formatted_line(json, &display, false, style.out));
        }
    }
    code
}

/// What format prints for one file it did something about: one object with --json, so every
/// result is a JSON line and nothing else reaches standard output, and the text otherwise —
/// the file alone painted warning for one --check found would change, and formatted painted
/// success, a space, and the file for one that was written.
// @lfy def/cli/main.lfy:main
fn formatted_line(json: bool, file: &str, check_only: bool, on: bool) -> String {
    // @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
    if json {
        return if check_only {
            serde_json::json!({ "file": file, "changed": true }).to_string()
        } else {
            serde_json::json!({ "file": file, "formatted": true }).to_string()
        };
    }
    // @lfy def/cli/main.lfy:main#main:main:d504cb8ad2e387fe99beb3a519324beeacca4def6dedd849adabc59474e1e0fe
    if check_only {
        return paint(file, Tone::Warning, on);
    }
    // @lfy def/cli/main.lfy:main#main:main:d753a07acf7cd2e3524dbffd665b892b3ae9864de414a7f4b9c8f0d14544ce61
    format!("{} {file}", paint("formatted", Tone::Success, on))
}

/// Every `.lfy` file under a directory.
// @lfy def/cli/main.lfy:main#main:main:1d0b1ab4401129accdd1a29fac4c5eb81857f849bbd63b314da9099ee0890a23
fn walk(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "lfy") {
            out.push(path);
        }
    }
}

/// The one file the command takes, read.
// @lfy def/cli/main.lfy:main
fn read_one_file(invocation: &Invocation) -> Result<(String, String), ExitCode> {
    let on = Paint::of(invocation).err;
    let [path] = invocation.arguments.as_slice() else {
        complain(on, None, &format!("usage: elfie {} <file>", invocation.command.value()));
        return Err(ExitCode::Usage);
    };
    match fs::read_to_string(path) {
        Ok(source) => Ok((path.clone(), source)),
        Err(error) => {
            complain(on, Some(path), &error.to_string());
            Err(ExitCode::Failure)
        }
    }
}

/// The identifier of the terminal that matched a token, or `invalid` for text none did.
// @lfy def/cli/main.lfy:main
fn rule_name(token: &Token) -> &'static str {
    token.rule.map_or("invalid", |rule| rule.identifier())
}

/// Where an error node begins, as the line and column of its first token.
// @lfy def/cli/main.lfy:main
fn error_position(tree: &Tree, error: &ErrorNode) -> (usize, usize) {
    let token = tree.tokens.get(error.start).or(tree.tokens.last());
    token.map_or((0, 0), |token| (token.line, token.column))
}

/// One node, error node, or token of the tree as a JSON object, then its children: the
/// depth it sits at, what it is, and what the text form gives for it — a node its rule and
/// its token range, an error node what it expected and where, a token its rule and its
/// value.
// @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
fn node_json(tree: &Tree, node: &Node, depth: usize, out: &mut Vec<String>) {
    let value = serde_json::json!({
        "depth": depth,
        "kind": "node",
        "rule": node.rule.identifier(),
        "start": node.start,
        "end": node.end,
    });
    out.push(value.to_string());
    for child in &node.children {
        match child {
            Child::Node(child) => node_json(tree, child, depth + 1, out),
            // An error node is given with what it expected, as the text form gives it.
            // @lfy def/cli/main.lfy:main#main:main:bc59fdd7d54a31dbb815a312311c8b74b14c270d75c54a6e78462dd5d5d85011
            Child::Error(error) => {
                let (line, column) = error_position(tree, error);
                let value = serde_json::json!({
                    "depth": depth + 1,
                    "kind": "error",
                    "start": error.start,
                    "end": error.end,
                    "line": line,
                    "column": column,
                    "expected": error.expected,
                    "value": tree.raw(error.start, error.end),
                });
                out.push(value.to_string());
            }
            Child::Token(index) => {
                let token = tree.token(*index);
                let value = serde_json::json!({
                    "depth": depth + 1,
                    "kind": "token",
                    "rule": rule_name(token),
                    "value": token.raw,
                });
                out.push(value.to_string());
            }
        }
    }
}

/// One node of the tree as the tree command prints it, then its children: one node or token
/// per line indented by depth, a node as its rule and its token range, a token as its rule
/// and its value, an error node as what it expected and the text it covers. A rule is active,
/// a token's value is success, a token range is muted, and an error node and what it expected
/// are failure.
// @lfy def/cli/main.lfy:main#main:main:2ab08b38cf714f57be25010631a0f3c194133753bf7d0adb4dafa2bfe312c8d8
fn node_lines(tree: &Tree, node: &Node, depth: usize, on: bool, out: &mut Vec<String>) {
    let indent = " ".repeat(depth * 2);
    // One node per line indented by depth, as its rule and its token range; the rule is
    // active and the range is muted.
    // @lfy def/cli/main.lfy:main#main:main:2ab08b38cf714f57be25010631a0f3c194133753bf7d0adb4dafa2bfe312c8d8
    // @lfy def/cli/main.lfy:main#main:main:de832217801af86c7592cc4721c38a98c42fa4f39679a023293e5124a250b778
    out.push(format!(
        "{indent}{} {}",
        paint(node.rule.identifier(), Tone::Active, on),
        paint(&format!("{}..{}", node.start, node.end), Tone::Muted, on)
    ));
    let indent = " ".repeat((depth + 1) * 2);
    for child in &node.children {
        match child {
            Child::Node(child) => node_lines(tree, child, depth + 1, on, out),
            // @lfy def/cli/main.lfy:main#main:main:b5834d8c0825c5c8edbdc8bd66074a5a35f59f1c15630f4416a73142537d7f00
            Child::Error(error) => out.push(format!(
                "{indent}{} {} {} {}",
                paint("Error", Tone::Failure, on),
                paint(&format!("{}..{}", error.start, error.end), Tone::Muted, on),
                paint(&format!("expected [{}]", error.expected.join(", ")), Tone::Failure, on),
                paint(&format!("{:?}", tree.raw(error.start, error.end)), Tone::Failure, on)
            )),
            Child::Token(index) => {
                let token = tree.token(*index);
                // A token as its rule and its value; the rule is active and the value is
                // success.
                // @lfy def/cli/main.lfy:main#main:main:2ab08b38cf714f57be25010631a0f3c194133753bf7d0adb4dafa2bfe312c8d8
                // @lfy def/cli/main.lfy:main#main:main:de832217801af86c7592cc4721c38a98c42fa4f39679a023293e5124a250b778
                out.push(format!(
                    "{indent}{} {}",
                    paint(rule_name(token), Tone::Active, on),
                    paint(&format!("{:?}", token.raw), Tone::Success, on)
                ));
            }
        }
    }
}

/// The lines the tree command prints: one node or token per line indented by depth, a
/// node as its rule and its token range, a token as its rule and its value, then every
/// error with what it expected. With --json each result is one JSON object on one line
/// instead of the text.
// @lfy def/cli/main.lfy:main
fn tree_lines(tree: &Tree, path: &str, json: bool, on: bool) -> Vec<String> {
    let mut lines = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
    if json {
        node_json(tree, &tree.root, 0, &mut lines);
        return lines;
    }
    node_lines(tree, &tree.root, 0, on, &mut lines);
    // Each error node is printed with what it expected, painted failure.
    // @lfy def/cli/main.lfy:main#main:main:bc59fdd7d54a31dbb815a312311c8b74b14c270d75c54a6e78462dd5d5d85011
    // @lfy def/cli/main.lfy:main#main:main:b5834d8c0825c5c8edbdc8bd66074a5a35f59f1c15630f4416a73142537d7f00
    for error in &tree.errors {
        let (line, column) = error_position(tree, error);
        lines.push(paint(
            &format!(
                "error at {path}:{line}:{column}: expected [{}] but found {:?}",
                error.expected.join(", "),
                tree.raw(error.start, error.end)
            ),
            Tone::Failure,
            on,
        ));
    }
    lines
}

/// Prints the parse of the one file given, one node or token per line, and the errors;
/// the code is problems when there is one.
// @lfy def/cli/main.lfy:main
fn tree(invocation: &Invocation) -> ExitCode {
    let (path, source) = match read_one_file(invocation) {
        Ok(read) => read,
        Err(code) => return code,
    };
    let tokens = match lex(&source, Some(&path)) {
        Ok(tokens) => tokens,
        Err(error) => {
            complain(Paint::of(invocation).err, Some(&path), &error.to_string());
            return ExitCode::Problems;
        }
    };
    let tree = parse(tokens, None);
    let mut out = io::BufWriter::new(io::stdout().lock());
    // @lfy def/cli/main.lfy:main#main:main:2ab08b38cf714f57be25010631a0f3c194133753bf7d0adb4dafa2bfe312c8d8
    for line in tree_lines(&tree, &path, invocation.flag("json"), Paint::of(invocation).out) {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    // @lfy def/cli/main.lfy:main#main:main:bc59fdd7d54a31dbb815a312311c8b74b14c270d75c54a6e78462dd5d5d85011
    if tree.errors.is_empty() { ExitCode::Success } else { ExitCode::Problems }
}

/// The lines the tokens command prints: one token per line as its line, column, rule, and
/// value. With --json each token is one JSON object on one line instead of the text. The line
/// and column are muted, the rule is active, and the value is success.
// @lfy def/cli/main.lfy:main#main:main:a39ab7b95d5bde21d9c6cac0a61d6ae97cb16498162052240c5f466c7d066355
fn token_lines(tokens: &[Token], json: bool, on: bool) -> Vec<String> {
    tokens
        .iter()
        .map(|token| {
            // @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
            if json {
                let value = serde_json::json!({
                    "line": token.line,
                    "column": token.column,
                    "rule": rule_name(token),
                    "value": token.value,
                });
                value.to_string()
            } else {
                // One token per line as its line, column, rule, and value; the line and
                // column are muted, the rule is active, and the value is success.
                // @lfy def/cli/main.lfy:main#main:main:a39ab7b95d5bde21d9c6cac0a61d6ae97cb16498162052240c5f466c7d066355
                // @lfy def/cli/main.lfy:main#main:main:de832217801af86c7592cc4721c38a98c42fa4f39679a023293e5124a250b778
                format!(
                    "{}\t{}\t{}",
                    paint(&format!("{}:{}", token.line, token.column), Tone::Muted, on),
                    paint(rule_name(token), Tone::Active, on),
                    paint(&format!("{:?}", token.value), Tone::Success, on)
                )
            }
        })
        .collect()
}

/// Prints the tokens of the one file given, one per line as line, column, rule, and
/// value.
// @lfy def/cli/main.lfy:main
fn tokens(invocation: &Invocation) -> ExitCode {
    let (path, source) = match read_one_file(invocation) {
        Ok(read) => read,
        Err(code) => return code,
    };
    let tokens = match lex(&source, Some(&path)) {
        Ok(tokens) => tokens,
        Err(error) => {
            complain(Paint::of(invocation).err, Some(&path), &error.to_string());
            return ExitCode::Problems;
        }
    };
    let mut out = io::BufWriter::new(io::stdout().lock());
    // @lfy def/cli/main.lfy:main#main:main:a39ab7b95d5bde21d9c6cac0a61d6ae97cb16498162052240c5f466c7d066355
    for line in token_lines(&tokens, invocation.flag("json"), Paint::of(invocation).out) {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    ExitCode::Success
}

// ---- progress ---------------------------------------------------------------------

/// The heading below which a person writes the answer to a question the compiler asked.
// @lfy def/cli/main.lfy:main#main:main:7362a1e1444ebeacc62b98e8cda328a14cd00d2aa703aacf5356c7356afd1059
const ANSWER_HEADING: &str = "## The answer";

/// Every step, so that the step column keeps one width for the whole compile.
// @lfy def/cli/main.lfy:main
const STEPS: [Step; 15] = [
    Step::Planned,
    Step::Requesting,
    Step::Compiling,
    Step::Checking,
    Step::Accepted,
    Step::Rejected,
    Step::Verifying,
    Step::Reviewed,
    Step::GlobalVerifying,
    Step::GlobalReviewed,
    Step::Retrying,
    Step::Blocked,
    Step::Clarification,
    Step::Failed,
    Step::Finished,
];

/// The width of the step column: the longest name of [`Step`] with its glyph, so every batch
/// starts in the same column.
// @lfy def/cli/main.lfy:main#main:main:b2460d0f754feeb9fcedecf6f6fa30aba02005172827b625b41b85b5fe03134c
fn step_column() -> usize {
    STEPS.iter().map(|step| width(glyph_of(*step)) + 1 + width(step.name())).max().unwrap_or(0)
}

/// How many columns the time column takes.
// @lfy def/cli/main.lfy:main#main:main:28892d8bcd2032ab9cd7e480196c8afab0b8e27e1799281d6c97c5bd7b5d2132
const TIME_COLUMN: usize = 6;

/// How many spaces a problem, a reason, or a question that follows a progress line is
/// indented by.
// @lfy def/cli/main.lfy:main#main:main:43f2aa5048680432b93b7802c6b6a48f5fcb659a985ce06c84d0358d44a2277d
const INDENT: &str = "    ";

/// The counts a reviewed, globalReviewed, or finished line reports: how many, what they
/// count, and the tone each is painted in when it is not 0.
// @lfy def/cli/main.lfy:main
type Counts = Vec<(usize, &'static str, Tone)>;

/// The counts as one message: each a [`tally`], separated by a comma and a space. Painted or
/// not, it holds the same words, so the message a script reads with --json is the one a
/// person reads.
// @lfy def/cli/main.lfy:main#main:main:378cfbd1a04722e2ce7778c7ad6ee7b2e5099f71f80618926ceb2f7b2302005d
// @lfy def/cli/main.lfy:main#main:main:5323f65cd6095d90ba92d44febf6910a6322aa6e7f5d1dd2a355bc374b55d7ee
fn counts_message(counts: &Counts, on: bool) -> String {
    counts.iter().map(|(count, label, tone)| tally(*count, label, *tone, on)).collect::<Vec<_>>().join(", ")
}

/// The counts of satisfied, violated, and unverifiable reviews, in that order, each with the
/// tone it is painted in.
// @lfy def/cli/main.lfy:main#main:main:378cfbd1a04722e2ce7778c7ad6ee7b2e5099f71f80618926ceb2f7b2302005d
fn review_counts(reviews: &[Review]) -> Counts {
    let (satisfied, violated, unverifiable) = counts_of(reviews);
    vec![
        (satisfied, "satisfied", Tone::Success),
        (violated, "violated", Tone::Failure),
        (unverifiable, "unverifiable", Tone::Warning),
    ]
}

/// One progress line, in columns separated by two spaces: the counter, the time, the step,
/// then the batch and unit when there are any, and the first line of the message. A reason or
/// a question runs to several lines, so the line carries only its first; the whole of it
/// follows, indented.
///
/// The counter and the time are muted, the step is its own tone unless one is given, the
/// batch and the unit are subject, and the message is the step's tone for a rejection, a
/// failure, a block, or a question, the counts when there are counts, and plain otherwise.
// @lfy def/cli/main.lfy:main#main:main:75d4f78bbe5ecb3c44fcc22c50a9c1442ecd4e8e7a09d6d85cb7e680052b81a4
fn text_of(progress: &Progress, counts: Option<&Counts>, tone: Option<Tone>, on: bool) -> String {
    // The counter keeps one width for the whole compile.
    // @lfy def/cli/main.lfy:main#main:main:f601506e044162ca9098825bc405de4953d676bd147ee6ea418ae3e2f2538737
    let digits = progress.total.to_string().len();
    let counter = paint(&format!("[{:>digits$}/{}]", progress.done, progress.total), Tone::Muted, on);
    // @lfy def/cli/main.lfy:main#main:main:28892d8bcd2032ab9cd7e480196c8afab0b8e27e1799281d6c97c5bd7b5d2132
    let time = paint(&format!("{:>TIME_COLUMN$}", duration(progress.elapsed)), Tone::Muted, on);
    // @lfy def/cli/main.lfy:main#main:main:b2460d0f754feeb9fcedecf6f6fa30aba02005172827b625b41b85b5fe03134c
    let named = format!("{} {}", glyph_of(progress.step), progress.step.name());
    let step = padded(&paint(&named, tone.unwrap_or_else(|| tone_of(progress.step)), on), step_column());
    let mut columns = vec![counter, time, step];
    // @lfy def/cli/main.lfy:main#main:main:333d22596bb1c95ec4fb99dc88ec17ccafcf20936594ac8c4a33e75623454079
    if let Some(batch) = &progress.batch {
        columns.push(paint(batch, Tone::Subject, on));
    }
    // The unit follows the batch.
    // @lfy def/cli/main.lfy:main#main:main:0d1ff3a013f40b12d868ee8a8671025d13e0700f5915bc9f9d4b0d9198e0206f
    // It is not repeated when it is the batch.
    // @lfy def/cli/main.lfy:main#main:main:d522b94cfc9d9f498700381b4c7513e7268b507feb946ad583475dc374bb91ba
    if let Some(unit) = &progress.unit
        && progress.batch.as_deref() != Some(unit.as_str())
    {
        columns.push(paint(unit, Tone::Subject, on));
    }
    // @lfy def/cli/main.lfy:main#main:main:75d4f78bbe5ecb3c44fcc22c50a9c1442ecd4e8e7a09d6d85cb7e680052b81a4
    let message = match counts {
        // @lfy def/cli/main.lfy:main#main:main:378cfbd1a04722e2ce7778c7ad6ee7b2e5099f71f80618926ceb2f7b2302005d
        Some(counts) => counts_message(counts, on),
        None => {
            let first = progress.message.lines().next().unwrap_or_default();
            match progress.step {
                // @lfy def/cli/main.lfy:main#main:main:c164c51236df72a93cac3a3c7bcc44ec0848586ed13d2f9add36192db3bc1999
                Step::Rejected | Step::Failed | Step::Blocked | Step::Clarification => {
                    paint(first, tone_of(progress.step), on)
                }
                // @lfy def/cli/main.lfy:main#main:main:2f7e303cf370420301ad1a9e978953cc546d4c7cf01b4737ae6ab3348f51447d
                _ => first.to_string(),
            }
        }
    };
    if !message.is_empty() {
        columns.push(message);
    }
    columns.join("  ")
}

/// One progress line as one JSON object.
// @lfy def/cli/main.lfy:main#main:main:5700505ffc866c1665e0871f60ef239b80f5cfa788e6ae0110e7e1ab3980d09f
// @lfy def/cli/main.lfy:main#main:main:19c38996ed044a2d3acb8961872c1c1b1925af0159ad008b6e7dab510c4d9d69
fn json_of(progress: &Progress) -> serde_json::Value {
    serde_json::json!({
        "step": progress.step.name(),
        "batch": progress.batch,
        "unit": progress.unit,
        "done": progress.done,
        "total": progress.total,
        "elapsed": progress.elapsed,
        "message": progress.message,
    })
}

/// Seconds since an instant, milliseconds kept as a fraction and never negative.
// @lfy def/cli/main.lfy:main
fn elapsed(start: SystemTime) -> f64 {
    SystemTime::now().duration_since(start).map_or(0.0, |since| since.as_secs_f64())
}

/// Prints one [`Progress`] line per step as it happens and appends it to the log. Every line
/// the log holds is the line as printed with colors off, so the log never holds an escape and
/// its layout is the printed layout.
// @lfy def/cli/main.lfy:main#main:main:b4823bca5f7b3b0ff98fb599d53625c19d59bd6bba538f5742bbc579e7082f9d
struct Reporter {
    json: bool,
    /// Whether each stream is painted; the log is written with nothing painted.
    // @lfy def/cli/main.lfy:main#main:main:6d1962bd388ee5c3b97353bbd3d76bfb559b2e1a57b72b2916622465fb1de177
    style: Paint,
    started: SystemTime,
    total: usize,
    done: usize,
    logs: Vec<PathBuf>,
    /// The batch the last line concerned, so the first line of a batch after the first batch
    /// has an empty line before it.
    // @lfy def/cli/main.lfy:main#main:main:04c60673b95c6861755ece09c1327c7fa907508c55c516f0b44d33ca5d52de46
    batch: Option<String>,
}

impl Reporter {
    // @lfy def/cli/main.lfy:main
    fn new(json: bool, style: Paint, total: usize, logs: Vec<PathBuf>) -> Reporter {
        Reporter { json, style, started: SystemTime::now(), total, done: 0, logs, batch: None }
    }

    /// One line for one step, printed and logged.
    // @lfy def/cli/main.lfy:main#main:main:b4823bca5f7b3b0ff98fb599d53625c19d59bd6bba538f5742bbc579e7082f9d
    // @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
    fn report(&mut self, step: Step, batch: Option<&str>, unit: Option<&str>, message: &str) {
        self.emit(step, batch, unit, message.to_string(), None, None);
    }

    /// One line whose message is the counts it reports, each painted only when it is worth
    /// noticing.
    // @lfy def/cli/main.lfy:main
    fn report_counts(&mut self, step: Step, batch: Option<&str>, counts: &Counts, tone: Option<Tone>) {
        // The message with --json holds the same words the counts are printed with.
        // @lfy def/cli/main.lfy:main#main:main:5323f65cd6095d90ba92d44febf6910a6322aa6e7f5d1dd2a355bc374b55d7ee
        let message = counts_message(counts, false);
        self.emit(step, batch, None, message, Some(counts), tone);
    }

    // @lfy def/cli/main.lfy:main
    fn emit(
        &mut self,
        step: Step,
        batch: Option<&str>,
        unit: Option<&str>,
        message: String,
        counts: Option<&Counts>,
        tone: Option<Tone>,
    ) {
        let progress = Progress {
            step,
            batch: batch.map(str::to_string),
            unit: unit.map(str::to_string),
            done: self.done,
            total: self.total,
            // @lfy def/cli/main.lfy:main#main:main:28892d8bcd2032ab9cd7e480196c8afab0b8e27e1799281d6c97c5bd7b5d2132
            elapsed: elapsed(self.started),
            message,
        };
        // An empty line before the first line of a batch after the first batch, before
        // finished, and before globalVerifying, so each batch reads as one block.
        // @lfy def/cli/main.lfy:main#main:main:04c60673b95c6861755ece09c1327c7fa907508c55c516f0b44d33ca5d52de46
        let fresh = batch.is_some() && batch != self.batch.as_deref();
        if !self.json
            && (self.batch.is_some() && fresh || step == Step::Finished || step == Step::GlobalVerifying)
        {
            println!();
            self.append("");
        }
        if fresh {
            self.batch = batch.map(str::to_string);
        }
        // @lfy def/cli/main.lfy:main#main:main:5700505ffc866c1665e0871f60ef239b80f5cfa788e6ae0110e7e1ab3980d09f
        // @lfy def/cli/main.lfy:main#main:main:19c38996ed044a2d3acb8961872c1c1b1925af0159ad008b6e7dab510c4d9d69
        if self.json {
            let line = json_of(&progress).to_string();
            println!("{line}");
            self.append(&line);
        } else {
            println!("{}", text_of(&progress, counts, tone, self.style.out));
            // Every line the log holds is the line as printed with colors off.
            // @lfy def/cli/main.lfy:main#main:main:6d1962bd388ee5c3b97353bbd3d76bfb559b2e1a57b72b2916622465fb1de177
            self.append(&text_of(&progress, counts, tone, false));
        }
    }

    /// The problems of a step, following the line they belong to: one per line, each indented
    /// four spaces, beginning with `-` and a space and painted failure. With --json nothing
    /// follows, since the whole of the message is in the [`Progress`].
    // @lfy def/cli/main.lfy:main#main:main:43f2aa5048680432b93b7802c6b6a48f5fcb659a985ce06c84d0358d44a2277d
    fn problems(&mut self, problems: &[String]) {
        if self.json {
            return;
        }
        for problem in problems {
            // @lfy def/cli/main.lfy:main#main:main:571d0aa15d4a9f6e843367171141143d8e9cc535fafa960cba337ad8ea554554
            let body = format!("- {problem}");
            println!("{INDENT}{}", paint(&body, Tone::Failure, self.style.out));
            self.append(&format!("{INDENT}{body}"));
        }
    }

    /// A reason or a question, following the line it belongs to: one line of text per line,
    /// each indented four spaces and painted warning.
    // @lfy def/cli/main.lfy:main#main:main:43f2aa5048680432b93b7802c6b6a48f5fcb659a985ce06c84d0358d44a2277d
    fn reason(&mut self, text: &str) {
        if self.json {
            return;
        }
        for line in text.lines() {
            // @lfy def/cli/main.lfy:main#main:main:f74f012b6abed1f12784b91dd8349a19d7a6b3e65f13f043f7bf8dc07929bc01
            println!("{INDENT}{}", paint(line, Tone::Warning, self.style.out));
            self.append(&format!("{INDENT}{line}"));
        }
    }

    /// Appends text to `elfie-requests/compile.log` under the root, so a run can be read
    /// after the fact.
    // @lfy def/cli/main.lfy:main#main:main:fd2e103cd45ada13659acc347b24014460bea62429f97ff35843a450d9ebde3b
    // @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
    fn append(&self, text: &str) {
        append_to(&self.logs, text);
    }
}

/// The lock every line of a log is written under, so that lines of two batches running at
/// once never mix: a line reaches the log whole or not at all, as it reaches the terminal
/// whole through one `println!`.
// @lfy def/cli/main.lfy:main#main:main:3a1f341e1f488489eebade96f9e410811db493057811f2c78e1ceb8d3a8ae77d
static LOGGING: Mutex<()> = Mutex::new(());

/// One line appended to each log, so a run can be read after the fact. Nothing that writes a
/// line to a person is without this, so the log holds everything the run showed.
// @lfy def/cli/main.lfy:main#main:main:fd2e103cd45ada13659acc347b24014460bea62429f97ff35843a450d9ebde3b
// @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
fn append_to(logs: &[PathBuf], text: &str) {
    // @lfy def/cli/main.lfy:main#main:main:3a1f341e1f488489eebade96f9e410811db493057811f2c78e1ceb8d3a8ae77d
    let _held = LOGGING.lock();
    for path in logs {
        let Some(parent) = path.parent() else { continue };
        // The folder it writes into is created when it is missing.
        // @lfy def/cli/main.lfy:main#main:main:d879c4ed7720d6b2f10f7658bdebae88cfb2ee0f415bb4e65471cec46f12d71d
        if fs::create_dir_all(parent).is_err() {
            continue;
        }
        let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) else { continue };
        let _ = writeln!(file, "{text}");
    }
}

// ---- compile ----------------------------------------------------------------------

/// The command elfie.json names under a key, when it names one.
// @lfy def/cli/main.lfy:main
fn manifest_command(root: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(root.join("elfie.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get(key)?.as_str().map(str::to_string)
}

/// The manifest's `compiler` command, when it names one.
// @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
fn compiler_command(root: &Path) -> Option<String> {
    manifest_command(root, "compiler")
}

/// The manifest's `verifier` command, when it names one: the second agent, which reads the
/// outputs and never writes.
// @lfy def/cli/main.lfy:main
fn verifier_command(root: &Path) -> Option<String> {
    manifest_command(root, "verifier")
}

/// What earlier versions recorded every map of a target in, under its output directory.
// @lfy def/cli/main.lfy:main
const LEGACY_MAPS: &str = "source-map.json";

/// The output directory of each planned target whose maps were read from source-map.json
/// rather than from a folder of map files, so that a compile moves them before any batch
/// runs.
// @lfy def/cli/main.lfy:main#main:main:4ede95f73677e1e4b2200b8d8acb50fc3d6ee537b3859a6c26714538dede2a4b
fn legacy_targets(workspace: &Workspace, target: Option<&str>) -> Vec<String> {
    let mut found = Vec::new();
    for named in &workspace.targets {
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        // @lfy def/cli/main.lfy:main
        if workspace.root.join(MAPS).join(&named.identifier).is_dir() {
            continue;
        }
        if workspace.root.join(&named.output_directory).join(LEGACY_MAPS).is_file() {
            found.push(named.output_directory.clone());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The maps a planned target kept in source-map.json recorded in each unit's map file, and
/// the file removed once every one is recorded; a unit whose maps could not be recorded
/// leaves the file in place, to be read again by the next compile.
// @lfy def/cli/main.lfy:main#main:main:4ede95f73677e1e4b2200b8d8acb50fc3d6ee537b3859a6c26714538dede2a4b
// @lfy def/cli/main.lfy:main#main:main:dceb0166c2f576d51b88a3750b57fa1a9753ed11934b200ac800febfebcd7d76
fn move_legacy_maps(workspace: &Workspace, plan: &Plan, directories: &[String]) {
    for directory in directories {
        let path = workspace.root.join(directory).join(LEGACY_MAPS);
        let held = generation::read_source_maps(&path);
        let mut every = true;
        for unit in &plan.units {
            let target = &workspace.targets[unit.target].identifier;
            let source = &workspace.files[unit.file].path;
            // @lfy def/cli/main.lfy:main
            let mine: Vec<SourceMap> =
                held.iter().filter(|map| &map.target == target && &map.source == source).cloned().collect();
            if mine.is_empty() {
                continue;
            }
            // @lfy def/cli/main.lfy:main#main:main:4ede95f73677e1e4b2200b8d8acb50fc3d6ee537b3859a6c26714538dede2a4b
            every &= generation::record(workspace, unit, &mine);
        }
        // Every one recorded, the file is removed; one that could not be is left in place.
        // @lfy def/cli/main.lfy:main#main:main:dceb0166c2f576d51b88a3750b57fa1a9753ed11934b200ac800febfebcd7d76
        if every {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Every map file under the folder of a planned target that belongs to no unit of the plan,
/// because its definition file was deleted, moved, or no longer builds for the target,
/// removed before any batch runs.
// @lfy def/cli/main.lfy:main#main:main:c0a6f173443a9a61ce5bf7b0bac002bc6df2efcba347f2a5d3ac185638db26c5
fn remove_orphan_maps(workspace: &Workspace, plan: &Plan, target: Option<&str>) {
    let mine: BTreeSet<PathBuf> =
        plan.units.iter().map(|unit| generation::map_file(workspace, unit)).collect();
    for named in &workspace.targets {
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        let folder = workspace.root.join(MAPS).join(&named.identifier);
        let mut found = Vec::new();
        walk_all(&folder, &mut found);
        for path in found {
            // @lfy def/cli/main.lfy:main#main:main:c0a6f173443a9a61ce5bf7b0bac002bc6df2efcba347f2a5d3ac185638db26c5
            if path.extension().is_some_and(|e| e == "json") && !mine.contains(&path) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// The files under the target's output directory, in the directory the batch ran in, that
/// carry a marker for the unit's file. Their paths are relative to that directory, which is
/// the root or a copy of it, so an output is named the same either way.
// @lfy def/cli/main.lfy:main#main:main:d0b78b7ebc4d561d8ef46a4e9de06ef0f94dd107b2ceaf69593d09297bbbde51
// @lfy def/cli/main.lfy:main#main:main:a4f5fbdf4d4599c991622bd9b47997c8c03fd47ec8dedfc46405fa17cd22947b
fn outputs_of(workspace: &Workspace, plan: &Plan, unit: usize, directory: &Path) -> Vec<Output> {
    let unit = &plan.units[unit];
    let target = &workspace.targets[unit.target];
    let file = &workspace.files[unit.file].path;
    let mut paths = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:d0b78b7ebc4d561d8ef46a4e9de06ef0f94dd107b2ceaf69593d09297bbbde51
    walk_all(&directory.join(&target.output_directory), &mut paths);
    paths.sort();
    let mut found = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if generation::parse_markers(&text).iter().any(|m| &m.file == file) {
            found.push(Output { path: relative(directory, &path), text });
        }
    }
    found
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

/// The source at the last accepted generation as generation reads it, recovered from git
/// when the hash of a recent revision of the file matches the unit's source map.
///
/// What is given back is [`elfie_core::interpret::LoweredFile::text`] of that revision,
/// bound through the loader and lowered the way `changes` binds a previous text, so that the
/// compiler is handed lowered code to set beside the lowered sources of its request rather
/// than the raw definition file. The map holds the hash of the lowered text, so the revision
/// the outputs were generated from is the one whose lowered text hashes to it, and a revision
/// that differs only in a comment is the same revision.
// @lfy def/cli/main.lfy:main#main:main:2be10bfb832c05f43391a87187f6acc8a21402fea8b3e547ebbc7f5738dd7bf8
fn previous_source(workspace: &Workspace, unit: &Unit) -> Option<String> {
    let map = unit.outputs.first()?;
    let file = workspace.files[unit.file].path.clone();
    let log = Process::new("git")
        .args(["log", "--format=%H", "-n", "50", "--", &file])
        .current_dir(&workspace.root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !log.status.success() {
        return None;
    }
    for revision in String::from_utf8_lossy(&log.stdout).lines() {
        let show = Process::new("git")
            .args(["show", &format!("{revision}:{file}")])
            .current_dir(&workspace.root)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !show.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&show.stdout).into_owned();
        // @lfy def/cli/main.lfy:main#main:main:2be10bfb832c05f43391a87187f6acc8a21402fea8b3e547ebbc7f5738dd7bf8
        let Some(lowered) = lowered_text(workspace, &file, &text) else { continue };
        // @lfy def/cli/main.lfy:main#main:main:2be10bfb832c05f43391a87187f6acc8a21402fea8b3e547ebbc7f5738dd7bf8
        if generation::source_hash(&lowered) == map.hash {
            return Some(lowered);
        }
    }
    None
}

/// One file read as something other than what is on disk, bound through the loader and
/// lowered: [`elfie_core::interpret::LoweredFile::text`] of it, the way `changes` binds a
/// previous text.
// @lfy def/cli/main.lfy:main#main:main:2be10bfb832c05f43391a87187f6acc8a21402fea8b3e547ebbc7f5738dd7bf8
fn lowered_text(workspace: &Workspace, path: &str, text: &str) -> Option<String> {
    // @lfy def/cli/main.lfy:main
    let before = lower(workspace::change(workspace, path, Some(text)));
    // @lfy def/cli/main.lfy:main
    let index = before.workspace.files.iter().position(|file| file.path == path)?;
    // @lfy def/cli/main.lfy:main
    Some(before.files[index].text.clone())
}

/// One unit as `elfie_units` lists it: the target, the stem, the reason or up to date,
/// and the stems of its dependencies.
///
/// Without --json the target is muted, the stem is subject, the reason is painted in its own
/// tone, and the dependencies are muted; the words and their order stay as they are, so a
/// script splitting a line on spaces reads the same fields.
// @lfy def/cli/main.lfy:main#main:main:1b5c2f2cb6fa24c09e7c95e0fdfcf07271d9861aaa40628a6344f360e5a4f3c0
fn unit_line(workspace: &Workspace, plan: &Plan, index: usize, on: bool) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&d| plan.units[d].stem.as_str()).collect();
    // @lfy def/cli/main.lfy:main#main:main:ab18a024b20ed11e7fe0d13e5a48ccf7598d626da4fec7f06cad31078f3a7a3d
    format!(
        "{} {} {} {}",
        paint(&workspace.targets[unit.target].identifier, Tone::Muted, on),
        paint(&unit.stem, Tone::Subject, on),
        paint(&reason, reason_tone(unit.reason), on),
        paint(&format!("[{}]", dependencies.join(", ")), Tone::Muted, on)
    )
}

/// One batch as `elfie_units` lists it: its identifier and the stems it holds. Without --json
/// the word batch is muted, the identifier is subject, and the stems are muted.
// @lfy def/cli/main.lfy:main
fn batch_line(plan: &Plan, batch: &Batch, on: bool) -> String {
    // @lfy def/cli/main.lfy:main#main:main:ee90074dd18f9c9628750926b72177aa313baf425665cfa10cf4c1d24135414d
    format!(
        "{} {} {}",
        paint("batch", Tone::Muted, on),
        paint(&batch.identifier, Tone::Subject, on),
        paint(&format!("[{}]", stems_of(plan, batch, ", ")), Tone::Muted, on)
    )
}

/// The identifier of the target a batch's units are of; a batch holds units of one
/// target.
// @lfy def/cli/main.lfy:main
fn batch_target<'w>(workspace: &'w Workspace, plan: &Plan, batch: &Batch) -> &'w str {
    match batch.units.first() {
        Some(&unit) => workspace.targets[plan.units[unit].target].identifier.as_str(),
        None => "",
    }
}

/// The stems a batch holds, joined.
// @lfy def/cli/main.lfy:main
fn stems_of(plan: &Plan, batch: &Batch, separator: &str) -> String {
    batch.units.iter().map(|&unit| plan.units[unit].stem.as_str()).collect::<Vec<_>>().join(separator)
}

/// A batch identifier as a file name: a stem holds slashes and a file name may not.
// @lfy def/cli/main.lfy:main
fn file_name_of(identifier: &str) -> String {
    identifier.replace('/', "-")
}

/// Each unit is printed as `elfie_units` lists it, then each batch with the stems it
/// holds, and nothing is generated.
// @lfy def/cli/main.lfy:main#main:main:1b5c2f2cb6fa24c09e7c95e0fdfcf07271d9861aaa40628a6344f360e5a4f3c0
fn dry_run(workspace: &Workspace, plan: &Plan, target: Option<&str>, json: bool, on: bool) -> ExitCode {
    for index in 0..plan.units.len() {
        let unit = &plan.units[index];
        if target.is_some_and(|name| workspace.targets[unit.target].identifier != name) {
            continue;
        }
        if json {
            // @lfy def/cli/main.lfy:main#main:main:941658dd3141e49fe1beca5cd4d12bb2f08da13cb77388c22d9ea9e25a44d6da
            println!(
                "{}",
                serde_json::json!({
                    "target": workspace.targets[unit.target].identifier,
                    "stem": unit.stem,
                    "file": workspace.files[unit.file].path,
                    "reason": unit.reason.map(|r| r.as_str()),
                    "dependencies": unit.dependencies.iter().map(|&d| plan.units[d].stem.clone()).collect::<Vec<_>>(),
                })
            );
        } else {
            println!("{}", unit_line(workspace, plan, index, on));
        }
    }
    for batch in &plan.batches {
        if target.is_some_and(|name| batch_target(workspace, plan, batch) != name) {
            continue;
        }
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "batch": batch.identifier,
                    "stems": batch.units.iter().map(|&unit| plan.units[unit].stem.clone()).collect::<Vec<_>>(),
                })
            );
        } else {
            println!("{}", batch_line(plan, batch, on));
        }
    }
    ExitCode::Success
}

/// Where the last global review is kept: the SHA-256 of the ids of every global criterion and
/// test it reviewed, and its report.
// @lfy def/cli/main.lfy:main#main:main:3c17358634114d8071a6f6dc88eaffdf201f2e93cbcf1b2521af485b92332931
const GLOBAL_REVIEWS: &str = "global.reviews.json";

/// What the verifier is handed when it reviews every global criterion and test once, and the
/// batch it is run for.
// @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
const GLOBAL: &str = "global";

/// Whether a requirement id is a global one: a global id begins with global.
// @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
fn is_global(id: &str) -> bool {
    id == GLOBAL || id.starts_with("global:")
}

/// The last global review: the SHA-256 of the ids of every global criterion and test it
/// reviewed, and its reviews, read from elfie-requests/global.reviews.json under the root.
/// There is no last global review when the file is missing.
// @lfy def/cli/main.lfy:main#main:main:3c17358634114d8071a6f6dc88eaffdf201f2e93cbcf1b2521af485b92332931
struct LastReview {
    requirements: String,
    reviews: Vec<Review>,
}

// There is no last global review when the file is missing.
// @lfy def/cli/main.lfy:main#main:main:ef8ef4391e1d110a280ad67e284f8a6fb14ac4c9eedd19b77ee2f9b14d520fd8
fn last_global_review(root: &Path) -> Option<LastReview> {
    let text = fs::read_to_string(requests_directory(root).join(GLOBAL_REVIEWS)).ok()?;
    // @lfy def/cli/main.lfy:main#main:main:3c17358634114d8071a6f6dc88eaffdf201f2e93cbcf1b2521af485b92332931
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let requirements = value.get("requirements").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let reviews = value
        .get("reviews")
        .and_then(|v| v.as_array())
        .map(|array| array.iter().filter_map(Review::from_json).collect())
        .unwrap_or_default();
    Some(LastReview { requirements, reviews })
}

/// The ids of every review the last global review found violated.
// @lfy def/cli/main.lfy:main#main:main:3c17358634114d8071a6f6dc88eaffdf201f2e93cbcf1b2521af485b92332931
fn violated_ids(root: &Path) -> BTreeSet<String> {
    let Some(last) = last_global_review(root) else {
        // @lfy def/cli/main.lfy:main#main:main:ef8ef4391e1d110a280ad67e284f8a6fb14ac4c9eedd19b77ee2f9b14d520fd8
        return BTreeSet::new();
    };
    last.reviews
        .iter()
        .filter(|review| review.status == ReviewStatus::Violated)
        .map(|review| review.id.clone())
        .collect()
}

/// The SHA-256 of the ids of every global criterion and test of the program, sorted and
/// joined by line breaks, so a compile can tell whether what a global review answered for
/// changed.
// @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
fn global_requirements(program: &Program) -> String {
    let mut ids: Vec<&str> = program
        .criteria
        .iter()
        .map(|criterion| criterion.id.as_str())
        .chain(program.tests.iter().map(|test| test.id.as_str()))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    // @lfy def/cli/main.lfy:main
    generation::source_hash(&ids.join("\n"))
}

/// The stems of every unit whose outputs hold a marker whose requirement is one of these
/// ids.
// @lfy def/cli/main.lfy:main#main:main:3c17358634114d8071a6f6dc88eaffdf201f2e93cbcf1b2521af485b92332931
fn stems_answering(plan: &Plan, ids: &BTreeSet<String>) -> Vec<String> {
    if ids.is_empty() {
        return Vec::new();
    }
    let mut stems: Vec<String> = plan
        .units
        .iter()
        .filter(|unit| {
            unit.outputs.iter().any(|map| {
                // @lfy def/cli/main.lfy:main#main:main:3c17358634114d8071a6f6dc88eaffdf201f2e93cbcf1b2521af485b92332931
                map.markers.iter().any(|marker| marker.requirement.as_deref().is_some_and(|id| ids.contains(id)))
            })
        })
        .map(|unit| unit.stem.clone())
        .collect();
    stems.sort();
    stems.dedup();
    stems
}

/// The workspace at the root loaded and planned once, with the stems requested and the stems
/// whose outputs a global review found violated.
// @lfy def/cli/main.lfy:main
fn plan_with(root: &Path, maps: &[SourceMap], requested: &[String], violated: &[String]) -> (Program, Plan) {
    // @lfy def/cli/main.lfy:main#main:main:d616972fdf1695d2ca0f4f9d8c1d0ab6f76bbe5af87f18daccc1f1c300092e65
    generation::plan(workspace::load(root), maps, requested, violated)
}

/// The plan of a command: the units planned, from the source maps, the stems requested, and
/// the stems whose outputs answer for a global criterion or test the last global review found
/// violated.
///
/// A stem is only known from a plan, so the plan is made once to learn which units answer for
/// a violated global criterion, and which units there are at all for --all, and once more
/// with those requested.
// @lfy def/cli/main.lfy:main
fn planned(
    root: &Path,
    maps: &[SourceMap],
    requested: &[String],
    all: bool,
    violated: &BTreeSet<String>,
) -> (Program, Plan) {
    // The stems given as arguments are the requested units.
    // @lfy def/cli/main.lfy:main#main:main:05db1e7bb995407566ee55e98fb6bd703f8b95b1d1a397ccb024cfef046666ae
    let (program, plan) = plan_with(root, maps, requested, &[]);
    // @lfy def/cli/main.lfy:main#main:main:d616972fdf1695d2ca0f4f9d8c1d0ab6f76bbe5af87f18daccc1f1c300092e65
    let stems = stems_answering(&plan, violated);
    if stems.is_empty() && !all {
        return (program, plan);
    }
    // --all requests every unit.
    // @lfy def/cli/main.lfy:main#main:main:4fbb0107e57ed38f5dcf0c4e543204d9a12d4897f4fcaa8506a16674fae6ac95
    let requested: Vec<String> =
        if all { plan.units.iter().map(|unit| unit.stem.clone()).collect() } else { requested.to_vec() };
    plan_with(root, maps, &requested, &stems)
}

/// `elfie-requests/compile.log` under the root, never under an output directory, so no
/// output is ever mistaken for the CLI's own files.
// @lfy def/cli/main.lfy:main#main:main:fd2e103cd45ada13659acc347b24014460bea62429f97ff35843a450d9ebde3b
fn log_paths(root: &Path) -> Vec<PathBuf> {
    vec![requests_directory(root).join("compile.log")]
}

/// `elfie-requests` under the root: where the CLI's own files live, never under an output
/// directory.
// @lfy def/cli/main.lfy:main#main:main:fd2e103cd45ada13659acc347b24014460bea62429f97ff35843a450d9ebde3b
fn requests_directory(root: &Path) -> PathBuf {
    root.join("elfie-requests")
}

/// The problems of an attempt appended to its instructions, so that the batch is run once
/// more knowing what was wrong.
// @lfy def/cli/main.lfy:main#main:main:17f6feee3279ee2519b458a9ca13607a4738441827bb862fc9a71d712ea28691
// @lfy def/cli/main.lfy:main#main:main:cfbd77013d5de5003e04314cb55ac7a6f8c091ff6c8a1c889716c385e8ffd907
fn append_problems(request: &mut Request, problems: &[String]) {
    request.instructions.push_str("\n\n## Problems with the previous attempt\n\n");
    for problem in problems {
        request.instructions.push_str(&format!("- {problem}\n"));
    }
}

/// One problem per violated review: `failure at`, the file, a colon, the line, a colon, the
/// note, and the evidence in parentheses.
// @lfy def/cli/main.lfy:main#main:main:cfbd77013d5de5003e04314cb55ac7a6f8c091ff6c8a1c889716c385e8ffd907
fn problem_of(review: &Review) -> String {
    format!("failure at {}:{}: {} ({})", review.file, review.line, review.note, review.evidence)
}

/// The counts of satisfied, violated, and unverifiable reviews, in that order.
// @lfy def/cli/main.lfy:main#main:main:0602c17d6b9aa65bc3f97a3834871de59d07641bcd9918c26de4764319637bab
// @lfy def/cli/main.lfy:main#main:main:e69fa07a0972795ed49b519edb1e91afc1d89a242334ffb96dea80538ff3e12b
fn counts_of(reviews: &[Review]) -> (usize, usize, usize) {
    let count = |status| reviews.iter().filter(|review| review.status == status).count();
    (count(ReviewStatus::Satisfied), count(ReviewStatus::Violated), count(ReviewStatus::Unverifiable))
}

/// What a finished command left: its code, -1 when it ended by a signal or could not be
/// started, and everything it wrote, held whole.
// @lfy def/cli/main.lfy:main#main:main:51f0754df779215b3f09f993a1826cd830a284f574fa4e2ccdf8ff100cdb30e8
struct Exit {
    code: i32,
    stdout: String,
    stderr: String,
}

/// One stream of the compiler or the verifier, each line shown as it arrives prefixed by the
/// batch, appended to the log, and kept whole. With --json it is shown on standard error, so
/// that standard output stays one JSON object per line. Every line of what the command wrote
/// reaches the log, standard error among them, so a run reads after the fact as it read while
/// it ran.
// @lfy def/cli/main.lfy:main#main:main:51f0754df779215b3f09f993a1826cd830a284f574fa4e2ccdf8ff100cdb30e8
// @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
fn watch<R: io::Read + Send + 'static>(
    pipe: R,
    batch: &str,
    keep: bool,
    to_stdout: bool,
    on: bool,
    is_error: bool,
    logs: Vec<PathBuf>,
) -> JoinHandle<String> {
    // Two spaces, the batch, a space, a bar, and a space.
    // @lfy def/cli/main.lfy:main#main:main:fb4120a58e1734e9c28f23a737dba9ae4e5f8193f283ceee38366f2f755d0604
    let plain = format!("  {batch} \u{2502} ");
    // The prefix of a line from standard output is muted.
    // @lfy def/cli/main.lfy:main#main:main:6ad3451822c64ea891b558c21bdcc31f8cb6f81e3454617bac2b1f5e5cc07250
    // The prefix of a line from standard error is warning.
    // @lfy def/cli/main.lfy:main#main:main:a3aaa5f304f90c834a7a590a643db67dc6784f65ff25f12deb878fc59a8b2044
    let prefix = paint(&plain, if is_error { Tone::Warning } else { Tone::Muted }, on);
    std::thread::spawn(move || {
        let mut kept = String::new();
        for line in io::BufReader::new(pipe).lines().map_while(Result::ok) {
            // The line itself is shown exactly as the command wrote it, as it arrives. One
            // `println!` writes the prefix and the line together under the lock on the
            // stream, so a line of another batch never mixes into it.
            // @lfy def/cli/main.lfy:main#main:main:fb4120a58e1734e9c28f23a737dba9ae4e5f8193f283ceee38366f2f755d0604
            // @lfy def/cli/main.lfy:main#main:main:51f0754df779215b3f09f993a1826cd830a284f574fa4e2ccdf8ff100cdb30e8
            // @lfy def/cli/main.lfy:main#main:main:3a1f341e1f488489eebade96f9e410811db493057811f2c78e1ceb8d3a8ae77d
            if to_stdout {
                println!("{prefix}{line}");
            } else {
                eprintln!("{prefix}{line}");
            }
            // The line the log holds is the line as printed with colors off.
            // @lfy def/cli/main.lfy:main#main:main:6d1962bd388ee5c3b97353bbd3d76bfb559b2e1a57b72b2916622465fb1de177
            // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
            // @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
            append_to(&logs, &format!("{plain}{line}"));
            if keep {
                kept.push_str(&line);
                kept.push('\n');
            }
        }
        kept
    })
}

/// What running one agent takes, held apart from the rest of a compile so that a batch can
/// stream its compiler and its verifier while the other batches stream theirs: nothing here
/// changes once a compile has begun.
// @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
#[derive(Clone)]
struct Agent {
    json: bool,
    style: Paint,
    logs: Vec<PathBuf>,
    /// The path of the running executable, so the agent server an agent starts is this same
    /// program.
    // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
    elfie: PathBuf,
}

impl Agent {
    // @lfy def/cli/main.lfy:main
    fn new(json: bool, style: Paint, logs: Vec<PathBuf>) -> Agent {
        // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
        let elfie = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("elfie"));
        Agent { json, style, logs, elfie }
    }

    /// Streams one agent, the compiler or the verifier, once for one batch: the instructions
    /// as its input, the batch's directory as its directory, and ELFIE_ROOT (the batch's
    /// directory), ELFIE_BATCH, ELFIE_UNITS (the stems, space separated), and ELFIE as its
    /// environment. Its standard output is the report. The code is -1 when the command could
    /// not be started.
    ///
    /// A batch running in a copy of the root gets GIT_DIR and GIT_WORK_TREE as well, so that
    /// git in the copy reads the root's history.
    // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
    // @lfy def/cli/main.lfy:main#main:main:48b94869143ec017c1f51605c536962c1e448cced5680bde78d799aa96824461
    fn run(&self, command: &str, where_: &Where, label: &str, units: &str, instructions: &str, what: &str) -> Exit {
        let directory = where_.directory();
        let mut spawning = Process::new("sh");
        spawning
            .arg("-c")
            .arg(command)
            .current_dir(directory)
            // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
            .env("ELFIE_ROOT", directory)
            .env("ELFIE_BATCH", label)
            .env("ELFIE_UNITS", units)
            .env("ELFIE", &self.elfie)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // The compiler and the verifier run in the copy with GIT_DIR naming the root's git
        // directory and GIT_WORK_TREE the copy.
        // @lfy def/cli/main.lfy:main#main:main:256d3040e99b1dbe822f8eb7b21d3d34f15faf77af0dc09bc5f7724644b6453b
        if let Where::Copy(mirror) = where_ {
            // @lfy def/cli/main.lfy:main#main:main:256d3040e99b1dbe822f8eb7b21d3d34f15faf77af0dc09bc5f7724644b6453b
            spawning.env("GIT_DIR", &mirror.git_directory).env("GIT_WORK_TREE", &mirror.directory);
        }
        // The command could not be started: the code is -1 and the failure is the output.
        // @lfy def/cli/main.lfy:main#main:main:fd1abf25846b2c26daa7f8517bfc55161489793755d16066f188e885d2c7dd29
        let mut child = match spawning.spawn() {
            Ok(child) => child,
            Err(error) => return Exit { code: -1, stdout: String::new(), stderr: error.to_string() },
        };
        // Each line is shown as it arrives, prefixed by the batch; with --json it goes to
        // standard error, so standard output stays one JSON object per line.
        // @lfy def/cli/main.lfy:main#main:main:51f0754df779215b3f09f993a1826cd830a284f574fa4e2ccdf8ff100cdb30e8
        let logs = self.logs.clone();
        let out = child.stdout.take().map(|pipe| watch(pipe, label, true, !self.json, self.style.out, false, logs.clone()));
        // Standard error reaches the log as standard output does, so nothing the command
        // wrote is lost to a run read after the fact.
        // @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
        let err = child.stderr.take().map(|pipe| watch(pipe, label, true, false, self.style.err, true, logs.clone()));
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(instructions.as_bytes())
        {
            append_to(&logs, &format!("the {what} command did not read its input: {error}"));
        }
        let status = child.wait();
        let stdout = out.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        let stderr = err.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        let status = match status {
            Ok(status) => status,
            Err(error) => return Exit { code: -1, stdout, stderr: error.to_string() },
        };
        if !status.success() {
            append_to(&logs, &format!("the {what} command exited with {status}"));
        }
        Exit { code: status.code().unwrap_or(-1), stdout, stderr }
    }
}

/// Where elfie-compile keeps a batch's own copy of the root.
// @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
const BATCHES: &str = "elfie-compile/cache/batches";

/// What a copy of the root leaves behind: the cache the copies live in, and the folder the
/// CLI writes its own files to under the root.
// @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
const UNCOPIED: [&str; 2] = ["elfie-compile/cache", "elfie-requests"];

/// What a merge leaves alone: elfie-compile, whose maps are recorded in the root once the
/// merge is done, and elfie-requests, whose files are written there all along.
// @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
const UNMERGED: [&str; 2] = ["elfie-compile", "elfie-requests"];

/// Whether a path is under one of these folders, which are spelled relative to the root with
/// forward slashes as the copy spells its own paths.
// @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
fn under(path: &str, folders: &[&str]) -> bool {
    folders.iter().any(|folder| path == *folder || path.starts_with(&format!("{folder}/")))
}

/// What `git` wrote in a directory, when it ran and ended well.
// @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output =
        Process::new("git").args(arguments).current_dir(directory).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The root's git directory, when the root is a git repository.
// @lfy def/cli/main.lfy:main#main:main:256d3040e99b1dbe822f8eb7b21d3d34f15faf77af0dc09bc5f7724644b6453b
fn git_directory(root: &Path) -> Option<PathBuf> {
    git(root, &["rev-parse", "--absolute-git-dir"]).map(|text| PathBuf::from(text.trim()))
}

/// One batch's own copy of the root: the directory its compiler and its verifier run in, and
/// what each copied file held when the copy was taken, which is the batch's base.
///
/// A copy exists because compilers editing one tree break each other's builds and tests. A
/// batch's work reaches the root only once its outputs are accepted and reviewed, merged file
/// by file, so the root only ever holds work that passed.
// @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
struct Mirror {
    directory: PathBuf,
    /// What each copied file held, by its path relative to the root.
    // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
    base: BTreeMap<String, Vec<u8>>,
    /// The root's git directory, so that git in the copy reads the root's history.
    // @lfy def/cli/main.lfy:main#main:main:256d3040e99b1dbe822f8eb7b21d3d34f15faf77af0dc09bc5f7724644b6453b
    git_directory: PathBuf,
}

impl Mirror {
    /// Every file git lists in the root, except those under the cache and elfie-requests,
    /// copied byte for byte to the same path under `elfie-compile/cache/batches/<batch>`.
    /// Nothing is copied when the root is no git repository, since there is then no list of
    /// what belongs to the project.
    // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
    fn take(root: &Path, identifier: &str) -> Option<Mirror> {
        let git_directory = git_directory(root)?;
        // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
        let listed = git(root, &["ls-files", "--cached", "--others", "--exclude-standard"])?;
        let directory = root.join(BATCHES).join(file_name_of(identifier));
        let _ = fs::remove_dir_all(&directory);
        let mut base = BTreeMap::new();
        for path in listed.lines() {
            let path = path.trim();
            // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
            if path.is_empty() || under(path, &UNCOPIED) {
                continue;
            }
            // A file git lists that is not there, because it was removed since, is nothing
            // to copy.
            // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
            let Ok(held) = fs::read(root.join(path)) else { continue };
            let to = directory.join(path);
            // @lfy def/cli/main.lfy:main#main:main:d879c4ed7720d6b2f10f7658bdebae88cfb2ee0f415bb4e65471cec46f12d71d
            if let Some(parent) = to.parent()
                && fs::create_dir_all(parent).is_err()
            {
                return None;
            }
            // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
            if fs::write(&to, &held).is_err() {
                return None;
            }
            base.insert(path.to_string(), held);
        }
        Some(Mirror { directory, base, git_directory })
    }

    /// Every file of the copy outside elfie-compile and elfie-requests that differs from the
    /// base, was added, or was removed, merged into the root; the paths merged, or the one
    /// file that could not be merged without a conflict.
    ///
    /// Nothing is written until every file has merged, so a conflict merges nothing of the
    /// batch.
    // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
    fn merge(&self, root: &Path) -> Result<Vec<String>, String> {
        let mut now: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut found = Vec::new();
        walk_all(&self.directory, &mut found);
        for path in found {
            let at = relative(&self.directory, &path);
            // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
            if under(&at, &UNMERGED) {
                continue;
            }
            if let Ok(held) = fs::read(&path) {
                now.insert(at, held);
            }
        }
        // Every path the copy holds, and every path the base held and the copy no longer
        // does: what was added, what differs, and what was removed.
        // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
        let mut paths: BTreeSet<&String> = now.keys().collect();
        paths.extend(self.base.keys().filter(|path| !under(path, &UNMERGED)));
        let mut writes: Vec<(String, Option<Vec<u8>>)> = Vec::new();
        for path in paths {
            let base = self.base.get(path);
            let mine = now.get(path);
            // The batch left it as it was: there is nothing of it to merge.
            // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
            if base.map(Vec::as_slice) == mine.map(Vec::as_slice) {
                continue;
            }
            let theirs = fs::read(root.join(path)).ok();
            // The root's file still holds what the base holds: it is replaced by the copy's,
            // or removed when the copy removed it.
            // @lfy def/cli/main.lfy:main#main:main:17684978e617aff4e72859ceb7f5ff7d85e0c699112fcb5e5aa6d8fb18f2a390
            if theirs.as_deref() == base.map(Vec::as_slice) {
                // @lfy def/cli/main.lfy:main#main:main:17684978e617aff4e72859ceb7f5ff7d85e0c699112fcb5e5aa6d8fb18f2a390
                writes.push((path.clone(), mine.cloned()));
                continue;
            }
            // Another batch changed the root's file since the base was taken: the root's file
            // is the three-way merge of the root's and the copy's against the base. One of
            // the three missing is a file added or removed on both sides, which no merge
            // settles.
            // @lfy def/cli/main.lfy:main#main:main:909b9177e97f545e3adf99e7e4b6c8bbf1cadc9f0982c0a89695dc00cc474e8d
            let (Some(base), Some(mine), Some(theirs)) = (base, mine, theirs.as_deref()) else {
                // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
                return Err(path.clone());
            };
            // @lfy def/cli/main.lfy:main#main:main:909b9177e97f545e3adf99e7e4b6c8bbf1cadc9f0982c0a89695dc00cc474e8d
            match self.merged(theirs, base, mine) {
                Some(text) => writes.push((path.clone(), Some(text))),
                // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
                None => return Err(path.clone()),
            }
        }
        let mut merged = Vec::new();
        for (path, text) in writes {
            let to = root.join(&path);
            match text {
                // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
                Some(text) => {
                    // @lfy def/cli/main.lfy:main#main:main:d879c4ed7720d6b2f10f7658bdebae88cfb2ee0f415bb4e65471cec46f12d71d
                    if let Some(parent) = to.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if fs::write(&to, &text).is_err() {
                        // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
                        return Err(path);
                    }
                }
                // @lfy def/cli/main.lfy:main#main:main:17684978e617aff4e72859ceb7f5ff7d85e0c699112fcb5e5aa6d8fb18f2a390
                None => {
                    let _ = fs::remove_file(&to);
                }
            }
            merged.push(path);
        }
        Ok(merged)
    }

    /// The three-way merge of the root's file and the copy's against the base, as
    /// `git merge-file` gives it; nothing when it conflicts.
    // @lfy def/cli/main.lfy:main#main:main:909b9177e97f545e3adf99e7e4b6c8bbf1cadc9f0982c0a89695dc00cc474e8d
    fn merged(&self, theirs: &[u8], base: &[u8], mine: &[u8]) -> Option<Vec<u8>> {
        // The three sides go beside the copy's own cache, which no merge reads.
        // @lfy def/cli/main.lfy:main#main:main:909b9177e97f545e3adf99e7e4b6c8bbf1cadc9f0982c0a89695dc00cc474e8d
        let scratch = self.directory.join("elfie-compile/cache/merge");
        fs::create_dir_all(&scratch).ok()?;
        let sides = [("theirs", theirs), ("base", base), ("mine", mine)];
        for (name, held) in sides {
            fs::write(scratch.join(name), held).ok()?;
        }
        // @lfy def/cli/main.lfy:main#main:main:909b9177e97f545e3adf99e7e4b6c8bbf1cadc9f0982c0a89695dc00cc474e8d
        let output = Process::new("git")
            .args(["merge-file", "-p", "--quiet", "theirs", "base", "mine"])
            .current_dir(&scratch)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let _ = fs::remove_dir_all(&scratch);
        // A code of 0 is a clean merge; anything above it counts the conflicts, and anything
        // below it is git itself failing.
        // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
        (output.status.code() == Some(0)).then_some(output.stdout)
    }

    /// The copy removed, once its work has reached the root.
    // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
    fn remove(&self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// Where one batch's compiler, verifier, and outputs are: the root itself, or a copy of it.
// @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
enum Where {
    /// The root itself, which is where a batch runs with --jobs 1 or outside a git
    /// repository, since nothing else is running to break.
    // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
    Root(PathBuf),
    /// Its own copy of the root.
    // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
    Copy(Mirror),
}

impl Where {
    /// Where a batch runs: a copy of the root when more than one batch may run at once and
    /// the root is a git repository, and the root itself otherwise.
    // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
    // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
    fn of(root: &Path, identifier: &str, jobs: usize) -> Where {
        // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
        if jobs <= 1 {
            return Where::Root(root.to_path_buf());
        }
        // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
        match Mirror::take(root, identifier) {
            Some(mirror) => Where::Copy(mirror),
            // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
            None => Where::Root(root.to_path_buf()),
        }
    }

    // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
    fn directory(&self) -> &Path {
        match self {
            Where::Root(root) => root,
            Where::Copy(mirror) => &mirror.directory,
        }
    }
}

/// How a batch ended, as the schedule reads it.
// @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
enum Done {
    /// Its work is in the root: nothing that depends on it waits any longer.
    // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
    Merged,
    /// A file of it could not be merged without a conflict, so it runs again from a new copy
    /// once no other batch is running; this is not a rejection.
    // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
    Conflicted,
}

/// Which batches of a round may start: a batch starts once every batch holding a dependency
/// of one of its units is done, at most --jobs run at once, and among those that can start
/// they start in plan order.
// @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
struct Planned {
    batch: Batch,
    /// The places in the round of the batches holding a dependency of one of its units.
    // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
    after: Vec<usize>,
}

/// What a round of a compile has started, finished, and has left to start.
// @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
#[derive(Default)]
struct Schedule {
    /// The places of the batches not started yet, in plan order.
    // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
    waiting: Vec<usize>,
    running: usize,
    done: BTreeSet<usize>,
    /// The places of the batches that must run with nothing else, because a file of theirs
    /// could not be merged.
    // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
    alone: BTreeSet<usize>,
    /// Whether a batch that must run with nothing else is running.
    // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
    exclusive: bool,
}

/// What a round does next.
// @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
enum Next {
    /// Start the batch at this place.
    // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
    Start(usize),
    /// Wait for a running batch to finish.
    // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
    Wait,
    /// Nothing is running and nothing can start.
    // @lfy def/cli/main.lfy:main#main:main:3c23880977d6965ce85ac281ad2e12af55f114b91a87851d4a296f402ee2550b
    Over,
}

impl Schedule {
    // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
    fn of(planned: &[Planned]) -> Schedule {
        Schedule { waiting: (0..planned.len()).collect(), ..Schedule::default() }
    }

    /// The batch to start, in plan order among those that can: one whose every batch holding
    /// a dependency is done, while fewer than --jobs are running.
    // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
    fn next(&mut self, planned: &[Planned], jobs: usize, halted: bool) -> Next {
        if self.waiting.is_empty() || halted {
            // The compile stopped: no batch starts after that, and the batches already
            // running finish as they would.
            // @lfy def/cli/main.lfy:main#main:main:3c23880977d6965ce85ac281ad2e12af55f114b91a87851d4a296f402ee2550b
            return if self.running == 0 { Next::Over } else { Next::Wait };
        }
        // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
        if self.running >= jobs || self.exclusive {
            return Next::Wait;
        }
        // Among the batches that can start, they start in plan order.
        // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
        let found = self.waiting.iter().position(|&at| {
            // A batch that must run with nothing else waits until nothing else is running.
            // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
            if self.alone.contains(&at) && self.running > 0 {
                return false;
            }
            // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
            planned[at].after.iter().all(|waited| self.done.contains(waited))
        });
        match found {
            Some(place) => {
                let at = self.waiting.remove(place);
                self.running += 1;
                // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
                self.exclusive = self.alone.contains(&at);
                Next::Start(at)
            }
            // Nothing can start: either a running batch will let one, or the batches left
            // wait on a batch that never finished.
            // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
            None if self.running > 0 => Next::Wait,
            // @lfy def/cli/main.lfy:main#main:main:3c23880977d6965ce85ac281ad2e12af55f114b91a87851d4a296f402ee2550b
            None => Next::Over,
        }
    }

    /// One batch finished: done, or waiting to run again from a new copy with nothing else
    /// running.
    // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
    fn finish(&mut self, at: usize, done: &Done) {
        self.running -= 1;
        self.exclusive = false;
        match done {
            // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
            Done::Merged => {
                self.done.insert(at);
            }
            // It runs again from a new copy once no other batch is running; its place goes
            // back among those waiting, in plan order.
            // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
            Done::Conflicted => {
                self.alone.insert(at);
                let place = self.waiting.iter().position(|&other| other > at).unwrap_or(self.waiting.len());
                self.waiting.insert(place, at);
            }
        }
    }

    /// The batches of a round that never ran, because what they wait on never finished.
    // @lfy def/cli/main.lfy:main#main:main:3c23880977d6965ce85ac281ad2e12af55f114b91a87851d4a296f402ee2550b
    fn unstarted(&self) -> Vec<usize> {
        self.waiting.clone()
    }
}

/// For each batch of a round, the places of the batches holding a dependency of one of its
/// units.
// @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
fn round_of(plan: &Plan, batches: &[usize]) -> Vec<Planned> {
    let mut planned: Vec<Planned> = Vec::new();
    for &index in batches {
        let batch = plan.batches[index].clone();
        // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
        let needed: BTreeSet<usize> =
            batch.units.iter().flat_map(|&unit| plan.units[unit].dependencies.iter().copied()).collect();
        // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
        let after = planned
            .iter()
            .enumerate()
            .filter(|(_, earlier)| earlier.batch.units.iter().any(|unit| needed.contains(unit)))
            .map(|(at, _)| at)
            .collect();
        planned.push(Planned { batch, after });
    }
    planned
}

/// What one compile is doing: the plan, the source maps as they are recorded, the counts
/// the finished line reports, and the units a stopped batch took down with it. The maps a
/// batch earns are held beside these until nothing has violated what was asked; only then
/// do they replace the units' here and reach the disk.
// @lfy def/cli/main.lfy:main
struct Run {
    /// The workspace lowered: what the plan, the requests, and the reviews are read from. The
    /// compile owns it, because a violated global review plans its units again and the plan is
    /// made from a workspace loaded afresh.
    // @lfy def/cli/main.lfy:main
    program: Program,
    plan: Plan,
    maps: Vec<SourceMap>,
    reporter: Reporter,
    /// What running the compiler and the verifier takes, held apart so that a batch streams
    /// its agents while the lock on the rest of this is free for the other batches.
    // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
    agent: Agent,
    json: bool,
    /// Whether each stream is painted.
    // @lfy def/cli/main.lfy:main
    style: Paint,
    continuing: bool,
    /// The command elfie.json names under `verifier`; nothing verifies a batch without one.
    // @lfy def/cli/main.lfy:main#main:main:4b47cdc2bc493291e54e816634dc3544ac06e3aaa5172b00447fc5b0ed5f60e6
    verifier: Option<String>,
    /// --no-verify: no verifier runs and nothing is reviewed.
    // @lfy def/cli/main.lfy:main#main:main:4b47cdc2bc493291e54e816634dc3544ac06e3aaa5172b00447fc5b0ed5f60e6
    no_verify: bool,
    halted: bool,
    /// Whether a batch of the round under way was accepted and its source maps recorded, so
    /// that the global review runs even when every unit was counted by an earlier round.
    // @lfy def/cli/main.lfy:main
    accepted_batch: bool,
    accepted: usize,
    rejected: usize,
    blocked: usize,
    failed: usize,
    stopped: BTreeSet<usize>,
    /// The stems accepted and written back, so that a unit accepted again after a review
    /// sent its batch back to the compiler is counted once, and a unit compiled again in a
    /// later round of the same compile is too.
    // @lfy def/cli/main.lfy:main
    recorded: BTreeSet<String>,
    incomplete: Vec<String>,
    code: ExitCode,
}

impl Run {
    // @lfy def/cli/main.lfy:main
    fn new(program: Program, plan: Plan, maps: Vec<SourceMap>, reporter: Reporter, invocation: &Invocation) -> Run {
        let verifier = verifier_command(&program.workspace.root);
        // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
        let agent = Agent::new(invocation.flag("json"), Paint::of(invocation), reporter.logs.clone());
        Run {
            program,
            plan,
            maps,
            reporter,
            // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
            agent,
            json: invocation.flag("json"),
            // @lfy def/cli/main.lfy:main
            style: Paint::of(invocation),
            continuing: invocation.flag("continue"),
            // @lfy def/cli/main.lfy:main
            verifier,
            no_verify: invocation.flag("no-verify"),
            halted: false,
            accepted_batch: false,
            accepted: 0,
            rejected: 0,
            blocked: 0,
            failed: 0,
            stopped: BTreeSet::new(),
            recorded: BTreeSet::new(),
            incomplete: Vec::new(),
            code: ExitCode::Success,
        }
    }

    /// The workspace the plan was made from.
    // @lfy def/cli/main.lfy:main
    fn workspace(&self) -> &Workspace {
        &self.program.workspace
    }

    /// The units a violated global review planned, compiled once more in the same compile:
    /// the program and the plan are replaced and the batches stopped by the round before are
    /// forgotten, while every count the finished line reports is kept.
    // @lfy def/cli/main.lfy:main
    fn replan(&mut self, program: Program, plan: Plan, total: usize) {
        self.program = program;
        self.plan = plan;
        self.reporter.total = total;
        self.halted = false;
        self.accepted_batch = false;
        self.stopped.clear();
        self.incomplete.clear();
    }

    /// The code so far, or a new one when it is more severe.
    // @lfy def/cli/main.lfy:main
    fn worsen(&mut self, code: ExitCode) {
        if code.rank() > self.code.rank() {
            self.code = code;
        }
    }

    /// A stopped batch stops only the batches that depend on one of its units; without
    /// --continue it stops the compile.
    // @lfy def/cli/main.lfy:main#main:main:6c99a3fd756dde171135b4dbd26c28b4aee7e003acc1b178a2f2d54429eeea4a
    fn stop(&mut self, batch: &Batch) {
        // @lfy def/cli/main.lfy:main#main:main:974ee309cbfc12d85b60f2bbb0b8cd1361aa959c94969a5cf6c482e0ec137ba7
        self.incomplete.push(batch.identifier.clone());
        self.stopped.extend(batch.units.iter().copied());
        // @lfy def/cli/main.lfy:main#main:main:b4d24a6285bcfe6faf8fa684cd04234bc339a6acb61cf4c5597af955903b8ba1
        // @lfy def/cli/main.lfy:main#main:main:a2115c940f2947290bc6ebc3b6560b08cc0b512909ff66d8413cfc366f78bcd9
        if !self.continuing {
            self.halted = true;
        }
    }

    /// The batches of the plan a compile runs, in plan order: every one, or only those of the
    /// target --target names.
    // @lfy def/cli/main.lfy:main#main:main:f8b290e1fac928822f6a90f053f270ccf4317dbe0ad7db90d725e4b9882322dc
    fn batches_for(&self, target: Option<&str>) -> Vec<usize> {
        (0..self.plan.batches.len())
            .filter(|&index| {
                // @lfy def/cli/main.lfy:main#main:main:f8b290e1fac928822f6a90f053f270ccf4317dbe0ad7db90d725e4b9882322dc
                target.is_none_or(|name| batch_target(self.workspace(), &self.plan, &self.plan.batches[index]) == name)
            })
            .collect()
    }

    /// Whether a batch holds a unit depending on one of a batch that did not complete.
    // @lfy def/cli/main.lfy:main#main:main:6c99a3fd756dde171135b4dbd26c28b4aee7e003acc1b178a2f2d54429eeea4a
    fn depends_on_stopped(&self, batch: &Batch) -> bool {
        batch
            .units
            .iter()
            .any(|&unit| self.plan.units[unit].dependencies.iter().any(|d| self.stopped.contains(d)))
    }

    /// The directory a batch's requests, questions, and reasons are written to:
    /// `elfie-requests` under the root, never under an output directory.
    // @lfy def/cli/main.lfy:main#main:main:fd2e103cd45ada13659acc347b24014460bea62429f97ff35843a450d9ebde3b
    fn requests_directory(&self) -> PathBuf {
        requests_directory(&self.workspace().root)
    }

    /// Writes `elfie-requests/<batch>.<suffix>` under the root.
    // @lfy def/cli/main.lfy:main#main:main:983348c7345facb1e1d5c4b06cffcad12cbe2adb4a8f41febe797ad372d4992e
    // @lfy def/cli/main.lfy:main#main:main:7362a1e1444ebeacc62b98e8cda328a14cd00d2aa703aacf5356c7356afd1059
    fn write_note(&self, batch: &Batch, suffix: &str, text: &str) -> Option<PathBuf> {
        let directory = self.requests_directory();
        let path = directory.join(format!("{}.{suffix}", file_name_of(&batch.identifier)));
        // The folder it writes into is created when it is missing.
        // @lfy def/cli/main.lfy:main#main:main:d879c4ed7720d6b2f10f7658bdebae88cfb2ee0f415bb4e65471cec46f12d71d
        match fs::create_dir_all(&directory).and_then(|()| fs::write(&path, text)) {
            Ok(()) => Some(path),
            Err(error) => {
                // Any other message the CLI itself prints to standard error begins with
                // elfie: painted failure, and a message that begins with a path has the
                // path painted subject.
                // @lfy def/cli/main.lfy:main#main:main:e4117eb242e540729b17f5396d7c22ad3c1767218252db7809b623a935bcaee9
                complain(self.style.err, Some(&path.to_string_lossy()), &error.to_string());
                None
            }
        }
    }

    /// The answer a person wrote below the question in
    /// `elfie-requests/<batch>.question.md`, when there is one.
    // @lfy def/cli/main.lfy:main#main:main:760ac9394baad44b62a3c96e58e305792fb43b2fe0dc7ab8510183a1ad12b070
    fn answer_of(&self, batch: &Batch) -> Option<String> {
        let directory = self.requests_directory();
        let path = directory.join(format!("{}.question.md", file_name_of(&batch.identifier)));
        let text = fs::read_to_string(path).ok()?;
        let (_, below) = text.split_once(ANSWER_HEADING)?;
        let answer = below
            .lines()
            .filter(|line| !line.trim_start().starts_with("<!--"))
            .collect::<Vec<_>>()
            .join("\n");
        let answer = answer.trim().to_string();
        (!answer.is_empty()).then_some(answer)
    }

    /// The request of one batch, with its existing outputs read from the batch's directory,
    /// the source each unit's outputs were generated from where git still holds it, and the
    /// answer to a question asked before appended.
    // @lfy def/cli/main.lfy:main
    fn request_of(&self, batch: &Batch, directory: &Path) -> Request {
        let existing: Vec<Output> = batch
            .units
            .iter()
            .flat_map(|&unit| self.plan.units[unit].outputs.iter())
            .filter_map(|map| {
                fs::read_to_string(directory.join(&map.output))
                    .ok()
                    .map(|text| Output { path: map.output.clone(), text })
            })
            .collect();
        // @lfy def/cli/main.lfy:main#main:main:2be10bfb832c05f43391a87187f6acc8a21402fea8b3e547ebbc7f5738dd7bf8
        let mut previous = BTreeMap::new();
        for &index in &batch.units {
            let unit = &self.plan.units[index];
            if let Some(text) = previous_source(self.workspace(), unit) {
                previous.insert(self.workspace().files[unit.file].path.clone(), text);
            }
        }
        // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
        let mut request = generation::request(&self.program, &self.plan, batch, &existing, &previous);
        // The person answers by editing the definitions, or by writing the answer below
        // the question, which the next compile of the batch appends to its instructions.
        // @lfy def/cli/main.lfy:main#main:main:760ac9394baad44b62a3c96e58e305792fb43b2fe0dc7ab8510183a1ad12b070
        if let Some(answer) = self.answer_of(batch) {
            request.instructions.push_str(&format!("\n\n## The answer to the question asked before\n\n{answer}\n"));
        }
        request
    }

    /// For each unit of the batch, `accept` runs on the files under the target's output
    /// directory in the batch's directory that carry a marker for the unit.
    // @lfy def/cli/main.lfy:main#main:main:d0b78b7ebc4d561d8ef46a4e9de06ef0f94dd107b2ceaf69593d09297bbbde51
    fn verdicts_of(&mut self, batch: &Batch, request: &Request, directory: &Path) -> Vec<Verdict> {
        let mut verdicts = Vec::new();
        for &unit in &batch.units {
            let stem = self.plan.units[unit].stem.clone();
            let reason = self.plan.units[unit].reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
            self.reporter.report(Step::Checking, Some(&batch.identifier), Some(&stem), &reason);
            // @lfy def/cli/main.lfy:main#main:main:d0b78b7ebc4d561d8ef46a4e9de06ef0f94dd107b2ceaf69593d09297bbbde51
            let outputs = outputs_of(self.workspace(), &self.plan, unit, directory);
            verdicts.push(generation::accept(&self.program, &self.plan, request, unit, &outputs));
        }
        verdicts
    }

    /// One unit's outputs written back in the batch's directory and the unit counted as
    /// done. The source maps [`generation::accept`] derived replace the unit's in the list
    /// held for the batch; nothing reaches a map file until that list is recorded.
    // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
    fn write_back(
        &mut self,
        unit: usize,
        verdict: &Verdict,
        held: &mut Vec<SourceMap>,
        directory: &Path,
    ) -> io::Result<()> {
        for output in &verdict.outputs {
            // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
            let path = directory.join(&output.path);
            // @lfy def/cli/main.lfy:main#main:main:d879c4ed7720d6b2f10f7658bdebae88cfb2ee0f415bb4e65471cec46f12d71d
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, &output.text)?;
        }
        let target = self.workspace().targets[self.plan.units[unit].target].identifier.clone();
        let source = self.workspace().files[self.plan.units[unit].file].path.clone();
        held.retain(|map| !(map.target == target && map.source == source));
        held.extend(verdict.source_maps.iter().cloned());
        // A unit accepted again, because its reviews sent its batch back to the compiler,
        // is counted once.
        // @lfy def/cli/main.lfy:main#main:main:5323f65cd6095d90ba92d44febf6910a6322aa6e7f5d1dd2a355bc374b55d7ee
        if self.recorded.insert(self.plan.units[unit].stem.clone()) {
            self.accepted += 1;
            self.reporter.done = self.accepted;
        }
        Ok(())
    }

    /// Every unit of an accepted batch written back in the batch's directory, and the source
    /// maps its verdicts earned held beside the recorded ones: the verifier is pointed at
    /// them, and they are recorded only once nothing has violated what was asked.
    // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
    fn hold_batch(&mut self, batch: &Batch, verdicts: &[Verdict], directory: &Path) -> Vec<SourceMap> {
        let mut held = self.maps.clone();
        for (&unit, verdict) in batch.units.iter().zip(verdicts) {
            let stem = self.plan.units[unit].stem.clone();
            if let Err(error) = self.write_back(unit, verdict, &mut held, directory) {
                complain(self.style.err, Some(&stem), &error.to_string());
                self.worsen(ExitCode::Failure);
                continue;
            }
            let written = format!("{} outputs", verdict.outputs.len());
            self.reporter.report(Step::Accepted, Some(&batch.identifier), Some(&stem), &written);
        }
        held
    }

    /// The source maps one batch held recorded: each of its units' are recorded in its own
    /// map file, so recording one unit leaves every other map file byte for byte as it was.
    ///
    /// Only the batch's own units are taken from what it held, and only they are replaced
    /// here, because a batch running beside it earned its own maps from a list taken before
    /// these and would otherwise lose them.
    // @lfy def/cli/main.lfy:main#main:main:5f94d631456802b856fe0bcc8391128da066ebd319c2f4ea466011a8739c6534
    // @lfy def/cli/main.lfy:main#main:main:3d60f0333a4eade80f24de8bdd5d6d7793e6939d89d318938e182a1c4deaf7f1
    // @lfy def/cli/main.lfy:main#main:main:4b47cdc2bc493291e54e816634dc3544ac06e3aaa5172b00447fc5b0ed5f60e6
    fn record_maps(&mut self, batch: &Batch, held: &[SourceMap]) {
        // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
        self.accepted_batch = true;
        let mut unwritten: Vec<String> = Vec::new();
        for &index in &batch.units {
            let unit = &self.plan.units[index];
            // Only a unit written back in this compile has anything new to record.
            // @lfy def/cli/main.lfy:main#main:main:a64dbb7ffadb4cffc5916067dd80d88bf16cb8db4d6aadf2d4406ffe3d0553e4
            if !self.recorded.contains(&unit.stem) {
                continue;
            }
            let workspace = &self.program.workspace;
            let target = workspace.targets[unit.target].identifier.clone();
            let source = workspace.files[unit.file].path.clone();
            let mine: Vec<SourceMap> =
                held.iter().filter(|map| map.target == target && map.source == source).cloned().collect();
            // @lfy def/cli/main.lfy:main#main:main:3d60f0333a4eade80f24de8bdd5d6d7793e6939d89d318938e182a1c4deaf7f1
            self.maps.retain(|map| !(map.target == target && map.source == source));
            self.maps.extend(mine.iter().cloned());
            let workspace = &self.program.workspace;
            let unit = &self.plan.units[index];
            // @lfy def/cli/main.lfy:main#main:main:5f94d631456802b856fe0bcc8391128da066ebd319c2f4ea466011a8739c6534
            if !generation::record(workspace, unit, &mine) {
                unwritten.push(generation::map_file(workspace, unit).display().to_string());
            }
        }
        for path in unwritten {
            complain(self.style.err, Some(&path), "could not be written");
            self.worsen(ExitCode::Failure);
        }
    }

    /// The problems of every rejected unit: one rejected line per unit, then its problems
    /// indented below it.
    // @lfy def/cli/main.lfy:main#main:main:edc0f227f5209e85d4be95c8b95210cd16f09cf786498eb2bc58d9ea7f7ea385
    fn report_rejections(&mut self, batch: &Batch, verdicts: &[Verdict]) -> usize {
        let mut rejected = 0;
        for (&unit, verdict) in batch.units.iter().zip(verdicts) {
            if verdict.accepted {
                continue;
            }
            rejected += 1;
            let stem = self.plan.units[unit].stem.clone();
            // @lfy def/cli/main.lfy:main#main:main:edc0f227f5209e85d4be95c8b95210cd16f09cf786498eb2bc58d9ea7f7ea385
            let message = verdict.problems.join("\n");
            self.reporter.report(Step::Rejected, Some(&batch.identifier), Some(&stem), &message);
            // @lfy def/cli/main.lfy:main#main:main:edc0f227f5209e85d4be95c8b95210cd16f09cf786498eb2bc58d9ea7f7ea385
            self.reporter.problems(&verdict.problems);
        }
        rejected
    }

    /// A batch whose units were accepted and written back, and whose reviews then violated
    /// what was asked: the acceptance was structural, and the review retracts it, so the
    /// finished line and the code count the units as rejected rather than accepted. Its
    /// held source maps are dropped rather than recorded, so its outputs stay on disk as
    /// they are and the unit stays planned.
    // @lfy def/cli/main.lfy:main#main:main:a4b92ae9375a7fc02cbba55aef2deb52de20c605973349f6fbf4dd6af16fa924
    fn unrecord(&mut self, batch: &Batch) {
        for &unit in &batch.units {
            if self.recorded.remove(&self.plan.units[unit].stem) {
                self.accepted = self.accepted.saturating_sub(1);
                self.rejected += 1;
            }
        }
        self.reporter.done = self.accepted;
    }

    /// The reviews of one batch as one JSON array, written to
    /// `elfie-requests/<batch>.reviews.json` under the root, replacing an earlier file.
    // @lfy def/cli/main.lfy:main#main:main:3f548e42a8495de4daff89eead0a9e15b4a7d683e508070b4b766e72d305e9fa
    // @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
    fn write_reviews(&mut self, batch: &Batch, reviews: &[Review]) {
        let value = serde_json::Value::Array(reviews.iter().map(Review::to_json).collect());
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default());
        self.write_note(batch, "reviews.json", &text);
    }

    /// The review request of a batch written for a verifier run by hand.
    // @lfy def/cli/main.lfy:main#main:main:bd9654344c4cef40114b45abde3e92ca30e92c685747378ea3e269fcf1b63fe4
    fn write_review_request(&mut self, batch: &Batch) {
        let request = generation::review(&self.program, &self.plan, batch, &self.maps);
        if let Some(path) = self.write_note(batch, "review.md", &request.instructions)
            && !self.json
        {
            println!("wrote {}", relative(&self.workspace().root, &path));
        }
    }

    /// The verifier run once for one batch, its output shown as it arrives and logged, its
    /// report read, and its reviews written. A failed run is run once more; when that run
    /// fails too its problems are printed as a failed progress line and the batch is verified
    /// with the reviews parsed from it as if its report had been complete.
    ///
    /// It takes the compile locked rather than borrowed, since the lock is free while the
    /// verifier runs: that is what lets the other batches run theirs at the same time.
    // @lfy def/cli/main.lfy:main#main:main:48b94869143ec017c1f51605c536962c1e448cced5680bde78d799aa96824461
    // @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
    fn review_batch(
        run: &Mutex<Run>,
        batch: &Batch,
        command: &str,
        where_: &Where,
        progress: bool,
        maps: &[SourceMap],
    ) -> Verification {
        let (agent, units, instructions) = {
            let mut held = locked(run);
            // A verifying progress line is printed before the verifier runs.
            // @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
            if progress {
                held.reporter.report(Step::Verifying, Some(&batch.identifier), None, command);
            }
            // The source maps are the ones `accept` derived, held but not yet recorded, so
            // every criterion can be pointed at the region of output that claims to satisfy
            // it.
            // @lfy def/cli/main.lfy:main#main:main:48b94869143ec017c1f51605c536962c1e448cced5680bde78d799aa96824461
            let request = generation::review(&held.program, &held.plan, batch, maps);
            (held.agent.clone(), stems_of(&held.plan, batch, " "), request.instructions)
        };
        let verification = Run::ask_verifier(run, &agent, command, where_, &batch.identifier, &units, &instructions);
        // @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
        locked(run).write_reviews(batch, &verification.report.reviews);
        verification
    }

    /// The verifier run for one batch, or once for the global criteria and tests, its output
    /// shown as it arrives and logged and its report read. A failed run — no line of its
    /// output read `ELFIE: REVIEWED`, it held no review at all, or the command could not be
    /// started — is run once more; when that run fails too its problems are printed as a
    /// failed progress line and the reviews parsed from it are read as if its report had been
    /// complete.
    // @lfy def/cli/main.lfy:main#main:main:e13334a45d8bb1815a5cb6ea0c40120b7e2ee52e9ccc44d15376b76b3fa724e8
    // @lfy def/cli/main.lfy:main#main:main:9a00d5ab9bc962d57d3c9e59f64a8dbd0034f2260b51fc176602c9b103475660
    fn ask_verifier(
        run: &Mutex<Run>,
        agent: &Agent,
        command: &str,
        where_: &Where,
        label: &str,
        units: &str,
        instructions: &str,
    ) -> Verification {
        let mut verification = Verification::default();
        for attempt in 1..=2 {
            // The lock is free here, so another batch streams its own agent meanwhile.
            // @lfy def/cli/main.lfy:main#main:main:48b94869143ec017c1f51605c536962c1e448cced5680bde78d799aa96824461
            let exit = agent.run(command, where_, label, units, instructions, "verifier");
            let mut held = locked(run);
            // Every line of the verifier's output and its report are appended to the log.
            // @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
            held.reporter.append(&format!("--- the review of {label} (attempt {attempt}) ---\n{}", exit.stdout));
            // @lfy def/cli/main.lfy:main#main:main:48b94869143ec017c1f51605c536962c1e448cced5680bde78d799aa96824461
            let mut report = generation::review_of(&exit.stdout, &held.program);
            if exit.code == -1 {
                report
                    .problems
                    .insert(0, format!("the verifier command could not be run: {}", exit.stderr.trim()));
            }
            // @lfy def/cli/main.lfy:main#main:main:e13334a45d8bb1815a5cb6ea0c40120b7e2ee52e9ccc44d15376b76b3fa724e8
            // @lfy def/cli/main.lfy:main#main:main:9a00d5ab9bc962d57d3c9e59f64a8dbd0034f2260b51fc176602c9b103475660
            let failed = verifier_failed(&exit, &report);
            verification = Verification { report, failed };
            // A run that ended with the end line and at least one review stands as it is,
            // whatever else it wrote; the verifier is not run again for that.
            // @lfy def/cli/main.lfy:main#main:main:7b33e4abb60c999fd74ba8a003a59f6de17412062b9767f21eda5ffd7eaf963c
            if !failed {
                break;
            }
            // A run made once more that fails too has its problems printed as a failed
            // progress line, and what it did report is read as if it had been complete.
            // @lfy def/cli/main.lfy:main#main:main:2526667ae85d62b2f4e46dcd53587a64569672ad4127a4c8c4cea6ebf71881d8
            if attempt == 2 {
                let problems = verification.report.problems.clone();
                held.reporter.report(Step::Failed, Some(label), None, &problems.join("\n"));
                held.reporter.problems(&problems);
                if exit.code == -1 {
                    held.failed += 1;
                    held.worsen(ExitCode::Failure);
                }
            }
        }
        verification
    }

    /// A batch whose units were accepted by `accept` is verified against the source maps it
    /// derived, held but not yet recorded, and the problems of its violated reviews are what
    /// it is rejected with; nothing verifies it with --no-verify or with no verifier named,
    /// and an unverifiable review is counted and never rejects.
    // @lfy def/cli/main.lfy:main#main:main:758b13daa5e7c7b626f2729da688fabd57ede1ef0528f05f4dfcbea26ec5509a
    fn verify_batch(run: &Mutex<Run>, batch: &Batch, where_: &Where, held: &[SourceMap]) -> Option<Vec<String>> {
        let command = {
            let state = locked(run);
            // @lfy def/cli/main.lfy:main#main:main:4b47cdc2bc493291e54e816634dc3544ac06e3aaa5172b00447fc5b0ed5f60e6
            if state.no_verify {
                return None;
            }
            state.verifier.clone()?
        };
        let verification = Run::review_batch(run, batch, &command, where_, true, held);
        let report = &verification.report;
        let mut state = locked(run);
        // One reviewed line follows, whose message is the counts of satisfied, violated,
        // and unverifiable reviews, each painted only when it is worth noticing.
        // @lfy def/cli/main.lfy:main#main:main:3f548e42a8495de4daff89eead0a9e15b4a7d683e508070b4b766e72d305e9fa
        let counts = review_counts(&report.reviews);
        state.reporter.report_counts(Step::Reviewed, Some(&batch.identifier), &counts, None);
        // A run that stood had anything it wrote beside its reviews printed under its
        // reviewed line; a failed run had its problems printed as a failed line already.
        // @lfy def/cli/main.lfy:main#main:main:7b33e4abb60c999fd74ba8a003a59f6de17412062b9767f21eda5ffd7eaf963c
        if !verification.failed {
            state.reporter.problems(&report.problems);
        }
        // An unverifiable review is counted in the reviewed line and never rejects the batch;
        // no violated review leaves the batch accepted.
        // @lfy def/cli/main.lfy:main#main:main:e69fa07a0972795ed49b519edb1e91afc1d89a242334ffb96dea80538ff3e12b
        // @lfy def/cli/main.lfy:main#main:main:3d60f0333a4eade80f24de8bdd5d6d7793e6939d89d318938e182a1c4deaf7f1
        if counts_of(&report.reviews).1 == 0 {
            return None;
        }
        // @lfy def/cli/main.lfy:main#main:main:cfbd77013d5de5003e04314cb55ac7a6f8c091ff6c8a1c889716c385e8ffd907
        Some(
            report
                .reviews
                .iter()
                .filter(|review| review.status == ReviewStatus::Violated)
                .map(problem_of)
                .collect(),
        )
    }

    /// Every global criterion and test reviewed once for the whole program, after every batch
    /// has been handled, when a verifier is named, --no-verify is not given, and either a
    /// batch was accepted or what the last global review answered for has changed. Nothing is
    /// reviewed when the program has no global criterion and no global test, and no file is
    /// written.
    ///
    /// Gives the stems whose markers answered for a violated review, so that a compile can
    /// plan them and run them once more.
    ///
    /// It runs in the root, after every batch has been merged into it, so that what it reads
    /// is the program's whole output rather than one batch's copy.
    // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
    // @lfy def/cli/main.lfy:main#main:main:2dcfe6cabe7e1aa71bf93e6eb934b1da6606315fb78fc3dd418d2cc4878dc2e5
    fn global_review(run: &Mutex<Run>, root: &Path) -> Vec<String> {
        let (agent, command, requirements, units, instructions) = {
            let mut held = locked(run);
            // No global criterion and no global test: no global review runs and no file is
            // written.
            // @lfy def/cli/main.lfy:main#main:main:be3d004d2bbdb0016741f66318c37fc38045672858eb8ac8e802ac6fee6024f0
            if held.program.criteria.is_empty() && held.program.tests.is_empty() {
                return Vec::new();
            }
            // @lfy def/cli/main.lfy:main#main:main:4b47cdc2bc493291e54e816634dc3544ac06e3aaa5172b00447fc5b0ed5f60e6
            if held.no_verify {
                return Vec::new();
            }
            let Some(command) = held.verifier.clone() else {
                return Vec::new();
            };
            // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
            let requirements = global_requirements(&held.program);
            let last = last_global_review(root);
            // Either a batch was accepted, or what the last global review answered for
            // changed.
            // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
            if !held.accepted_batch && last.as_ref().is_some_and(|last| last.requirements == requirements) {
                return Vec::new();
            }
            // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
            held.reporter.report(Step::GlobalVerifying, None, None, &command);
            // Every source map, recorded or held.
            // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
            let request = generation::global_review(&held.program, &held.maps);
            // The units just generated among them.
            // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
            let units = held.global_stems();
            (held.agent.clone(), command, requirements, units, request.instructions)
        };
        // @lfy def/cli/main.lfy:main#main:main:2dcfe6cabe7e1aa71bf93e6eb934b1da6606315fb78fc3dd418d2cc4878dc2e5
        let where_ = Where::Root(root.to_path_buf());
        let verification = Run::ask_verifier(run, &agent, &command, &where_, GLOBAL, &units, &instructions);
        let report = verification.report;
        let mut held = locked(run);
        held.write_global_reviews(root, &requirements, &report);
        // @lfy def/cli/main.lfy:main#main:main:959368e3b0059d177c0bc07849a385193d9f17f351c8c61412cca4591380b4c1
        let counts = review_counts(&report.reviews);
        held.reporter.report_counts(Step::GlobalReviewed, None, &counts, None);
        // @lfy def/cli/main.lfy:main#main:main:f3b3525a24fda209b102430025ef51ebb6eab2bd9b4e278ac2b7d72a8e46fd96
        let violated: Vec<&Review> =
            report.reviews.iter().filter(|review| review.status == ReviewStatus::Violated).collect();
        if violated.is_empty() {
            return Vec::new();
        }
        // One problem per violated review: failure at, a space, global, a colon, a space, the
        // note, and the evidence in parentheses.
        // @lfy def/cli/main.lfy:main#main:main:f3b3525a24fda209b102430025ef51ebb6eab2bd9b4e278ac2b7d72a8e46fd96
        let problems: Vec<String> = violated
            .iter()
            .map(|review| format!("failure at {GLOBAL}: {} ({})", review.note, review.evidence))
            .collect();
        held.reporter.problems(&problems);
        // The units whose markers answered for it.
        // @lfy def/cli/main.lfy:main#main:main:f3b3525a24fda209b102430025ef51ebb6eab2bd9b4e278ac2b7d72a8e46fd96
        let ids: BTreeSet<String> = violated.iter().map(|review| review.id.clone()).collect();
        held.stems_answering_now(&ids)
    }

    /// The stems of every unit whose source maps hold a marker answering for one of these
    /// ids, read from the maps as they stand: a unit compiled in this run has its markers
    /// there rather than in the outputs its plan was made from.
    // @lfy def/cli/main.lfy:main#main:main:f3b3525a24fda209b102430025ef51ebb6eab2bd9b4e278ac2b7d72a8e46fd96
    fn stems_answering_now(&self, ids: &BTreeSet<String>) -> Vec<String> {
        // @lfy def/cli/main.lfy:main#main:main:3c17358634114d8071a6f6dc88eaffdf201f2e93cbcf1b2521af485b92332931
        self.stems_answering_with(|id| ids.contains(id))
    }

    /// The stems, space separated, of every unit whose source maps hold a marker whose
    /// requirement is a global id: what `ELFIE_UNITS` holds for a global review.
    // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
    // @lfy def/cli/main.lfy:main#main:main:f71aa5a984a022b464e4eabdb0b20fd093322e8041b14b8ac9747246929ebeb9
    fn global_stems(&self) -> String {
        // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
        self.stems_answering_with(is_global).join(" ")
    }

    /// The stems of every unit whose source maps hold a marker whose requirement the test
    /// accepts.
    ///
    /// A unit's source maps are those this compile accepted for it when it has any, and
    /// [`Unit::outputs`] otherwise, so a unit generated in this compile is named by the
    /// markers it has just earned rather than by the ones the plan was made from, and a unit
    /// nothing touched by the ones its map file holds.
    // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
    fn stems_answering_with(&self, answers: impl Fn(&str) -> bool) -> Vec<String> {
        let mut stems: Vec<String> = Vec::new();
        for unit in &self.plan.units {
            let target = &self.program.workspace.targets[unit.target].identifier;
            let source = &self.program.workspace.files[unit.file].path;
            // A unit's source maps are those this compile accepted for it when it has any.
            // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
            let accepted: Vec<&SourceMap> =
                self.maps.iter().filter(|map| &map.target == target && &map.source == source).collect();
            // Unit.outputs otherwise.
            // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
            let maps = if accepted.is_empty() { unit.outputs.iter().collect() } else { accepted };
            // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
            let answered = maps
                .iter()
                .any(|map| map.markers.iter().any(|marker| marker.requirement.as_deref().is_some_and(&answers)));
            if answered {
                stems.push(unit.stem.clone());
            }
        }
        stems.sort();
        stems.dedup();
        stems
    }

    /// `elfie-requests/global.reviews.json` under the root written with the SHA-256 of the
    /// global ids reviewed and the report, replacing an earlier file.
    // @lfy def/cli/main.lfy:main#main:main:959368e3b0059d177c0bc07849a385193d9f17f351c8c61412cca4591380b4c1
    // @lfy def/cli/main.lfy:main#main:main:f71aa5a984a022b464e4eabdb0b20fd093322e8041b14b8ac9747246929ebeb9
    fn write_global_reviews(&mut self, root: &Path, requirements: &str, report: &ReviewReport) {
        // @lfy def/cli/main.lfy:main#main:main:959368e3b0059d177c0bc07849a385193d9f17f351c8c61412cca4591380b4c1
        let value = serde_json::json!({
            "requirements": requirements,
            "reviews": serde_json::Value::Array(report.reviews.iter().map(Review::to_json).collect()),
            "problems": report.problems,
        });
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default());
        let directory = requests_directory(root);
        let path = directory.join(GLOBAL_REVIEWS);
        if let Err(error) = fs::create_dir_all(&directory).and_then(|()| fs::write(&path, text)) {
            complain(self.style.err, Some(&path.display().to_string()), &error.to_string());
            self.worsen(ExitCode::Failure);
        }
    }

    /// Every global criterion and test reviewed once for the verify command: a person asked
    /// for the opinion, so nothing but their existence gates it, and the reviews are given
    /// back to be printed.
    // @lfy def/cli/main.lfy:main#main:main:f71aa5a984a022b464e4eabdb0b20fd093322e8041b14b8ac9747246929ebeb9
    fn verify_globally(run: &Mutex<Run>, root: &Path) -> Vec<Review> {
        let (agent, command, units, instructions, requirements) = {
            let held = locked(run);
            // No global criterion and no global test: no global review runs and no file is
            // written, here as in a compile.
            // @lfy def/cli/main.lfy:main#main:main:be3d004d2bbdb0016741f66318c37fc38045672858eb8ac8e802ac6fee6024f0
            if held.program.criteria.is_empty() && held.program.tests.is_empty() {
                return Vec::new();
            }
            let Some(command) = held.verifier.clone() else {
                return Vec::new();
            };
            // @lfy def/cli/main.lfy:main#main:main:f71aa5a984a022b464e4eabdb0b20fd093322e8041b14b8ac9747246929ebeb9
            let request = generation::global_review(&held.program, &held.maps);
            // @lfy def/cli/main.lfy:main#main:main:f71aa5a984a022b464e4eabdb0b20fd093322e8041b14b8ac9747246929ebeb9
            let units = held.global_stems();
            // @lfy def/cli/main.lfy:main#main:main:f71aa5a984a022b464e4eabdb0b20fd093322e8041b14b8ac9747246929ebeb9
            let requirements = global_requirements(&held.program);
            (held.agent.clone(), command, units, request.instructions, requirements)
        };
        let where_ = Where::Root(root.to_path_buf());
        let verification = Run::ask_verifier(run, &agent, &command, &where_, GLOBAL, &units, &instructions);
        locked(run).write_global_reviews(root, &requirements, &verification.report);
        verification.report.reviews
    }

    /// Runs the compiler on one batch, at most twice, and acts on the outcome; a batch
    /// whose reviews violate what was asked is run once more beyond that. Gives the source
    /// maps its units earned when it was accepted, to be recorded once its work has reached
    /// the root, and nothing when it stopped.
    // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
    fn compile_batch(run: &Mutex<Run>, batch: &Batch, command: &str, where_: &Where) -> Option<Vec<SourceMap>> {
        let directory = where_.directory().to_path_buf();
        let (agent, mut request) = {
            let mut state = locked(run);
            let stems = stems_of(&state.plan, batch, " ");
            state.reporter.report(Step::Requesting, Some(&batch.identifier), None, &stems);
            let request = state.request_of(batch, &directory);
            (state.agent.clone(), request)
        };
        let mut attempt = 0;
        // The batch is run once more for a rejected outcome, and once more for violated
        // reviews even when it was already run once more for a rejected outcome.
        // @lfy def/cli/main.lfy:main#main:main:17f6feee3279ee2519b458a9ca13607a4738441827bb862fc9a71d712ea28691
        let mut retried_for_rejection = false;
        let mut retried_for_reviews = false;
        let mut retried_for_failure = false;
        loop {
            attempt += 1;
            let units = {
                let mut state = locked(run);
                let step = if attempt == 1 { Step::Compiling } else { Step::Retrying };
                state.reporter.report(step, Some(&batch.identifier), None, command);
                // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
                stems_of(&state.plan, batch, " ")
            };
            // The lock is free here, so another batch streams its own compiler meanwhile.
            // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
            let exit = agent.run(command, where_, &batch.identifier, &units, &request.instructions, "compiler");
            let outcome = {
                let mut state = locked(run);
                // The code is -1 because the command could not be started: the outcome is
                // failed.
                // @lfy def/cli/main.lfy:main#main:main:8a37c623bc2cba7e08ed04d5a76ecf0545b959457f78fe3968759a2306298411
                if exit.code == -1 {
                    Outcome {
                        kind: OutcomeKind::Failed,
                        message: format!("the compiler command could not be run: {}", exit.stderr.trim()),
                        verdicts: Vec::new(),
                    }
                } else {
                    // Process.Exit.stdout is the report, appended to the log.
                    // @lfy def/cli/main.lfy:main#main:main:51f0754df779215b3f09f993a1826cd830a284f574fa4e2ccdf8ff100cdb30e8
                    // @lfy def/cli/main.lfy:main#main:main:fd2e103cd45ada13659acc347b24014460bea62429f97ff35843a450d9ebde3b
                    let report = exit.stdout;
                    state
                        .reporter
                        .append(&format!("--- the report of {} (attempt {attempt}) ---\n{report}", batch.identifier));
                    // @lfy def/cli/main.lfy:main#main:main:d0b78b7ebc4d561d8ef46a4e9de06ef0f94dd107b2ceaf69593d09297bbbde51
                    let verdicts = state.verdicts_of(batch, &request, &directory);
                    generation::outcome_of(&report, verdicts)
                }
            };
            match outcome.kind {
                // Each unit's outputs are written back in the batch's directory and its
                // source maps are held, and the batch is verified; no review violated, or
                // nothing verifying it, leaves the held source maps to be recorded once the
                // batch's work has reached the root.
                // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
                OutcomeKind::Accepted => {
                    // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
                    let held = locked(run).hold_batch(batch, &outcome.verdicts, &directory);
                    // The batch is verified; no review violated, or nothing verifying it,
                    // and the batch is done.
                    // @lfy def/cli/main.lfy:main#main:main:758b13daa5e7c7b626f2729da688fabd57ede1ef0528f05f4dfcbea26ec5509a
                    // @lfy def/cli/main.lfy:main#main:main:5f94d631456802b856fe0bcc8391128da066ebd319c2f4ea466011a8739c6534
                    // @lfy def/cli/main.lfy:main#main:main:fa4ae831ea45c283917b32ed93efbfcc7f4638b8d516acfff72f3d2a470888e6
                    let Some(problems) = Run::verify_batch(run, batch, where_, &held) else {
                        return Some(held);
                    };
                    let mut state = locked(run);
                    // A violated review is handled as a rejected outcome is: the problems
                    // are printed and the batch is run once more with them appended to the
                    // instructions.
                    // @lfy def/cli/main.lfy:main#main:main:cfbd77013d5de5003e04314cb55ac7a6f8c091ff6c8a1c889716c385e8ffd907
                    state.reporter.report(Step::Rejected, Some(&batch.identifier), None, &problems.join("\n"));
                    state.reporter.problems(&problems);
                    // A violated review then stops the batch as rejected, and the held source
                    // maps are dropped rather than recorded, so the unit stays planned.
                    // @lfy def/cli/main.lfy:main#main:main:a4b92ae9375a7fc02cbba55aef2deb52de20c605973349f6fbf4dd6af16fa924
                    if retried_for_reviews {
                        state.unrecord(batch);
                        state.worsen(ExitCode::Problems);
                        state.stop(batch);
                        return None;
                    }
                    retried_for_reviews = true;
                    drop(state);
                    append_problems(&mut request, &problems);
                }
                // The problems are printed, and the batch is run once more with them
                // appended to the instructions.
                // @lfy def/cli/main.lfy:main#main:main:17f6feee3279ee2519b458a9ca13607a4738441827bb862fc9a71d712ea28691
                OutcomeKind::Rejected => {
                    let mut state = locked(run);
                    let rejected = state.report_rejections(batch, &outcome.verdicts);
                    // A second rejection stops the compile.
                    // @lfy def/cli/main.lfy:main#main:main:b4d24a6285bcfe6faf8fa684cd04234bc339a6acb61cf4c5597af955903b8ba1
                    if retried_for_rejection {
                        state.rejected += rejected;
                        state.worsen(ExitCode::Problems);
                        state.stop(batch);
                        return None;
                    }
                    retried_for_rejection = true;
                    drop(state);
                    let problems: Vec<String> =
                        outcome.verdicts.iter().flat_map(|verdict| verdict.problems.iter().cloned()).collect();
                    append_problems(&mut request, &problems);
                }
                // The reason is printed and written, and the compile stops.
                // @lfy def/cli/main.lfy:main#main:main:983348c7345facb1e1d5c4b06cffcad12cbe2adb4a8f41febe797ad372d4992e
                OutcomeKind::Blocked => {
                    let mut state = locked(run);
                    state.blocked += 1;
                    state.reporter.report(Step::Blocked, Some(&batch.identifier), None, &outcome.message);
                    // @lfy def/cli/main.lfy:main#main:main:983348c7345facb1e1d5c4b06cffcad12cbe2adb4a8f41febe797ad372d4992e
                    state.reporter.reason(&outcome.message);
                    let note = format!("# The compiler is blocked on the batch {}\n\n{}\n", batch.identifier, outcome.message);
                    state.write_note(batch, "blocked.md", &note);
                    state.worsen(ExitCode::Problems);
                    state.stop(batch);
                    return None;
                }
                // The question is printed and written, and the compile stops.
                // @lfy def/cli/main.lfy:main#main:main:7362a1e1444ebeacc62b98e8cda328a14cd00d2aa703aacf5356c7356afd1059
                OutcomeKind::Clarification => {
                    let mut state = locked(run);
                    state.reporter.report(Step::Clarification, Some(&batch.identifier), None, &outcome.message);
                    // @lfy def/cli/main.lfy:main#main:main:7362a1e1444ebeacc62b98e8cda328a14cd00d2aa703aacf5356c7356afd1059
                    state.reporter.reason(&outcome.message);
                    let note = format!(
                        "# The compiler asked about the batch {}\n\n{}\n\n{ANSWER_HEADING}\n\n<!-- Answer by editing the definitions, or write the answer below this line; the next compile of this batch appends it to the instructions. -->\n",
                        batch.identifier, outcome.message
                    );
                    state.write_note(batch, "question.md", &note);
                    state.worsen(ExitCode::Problems);
                    state.stop(batch);
                    return None;
                }
                // The batch is run once more.
                // @lfy def/cli/main.lfy:main#main:main:8a37c623bc2cba7e08ed04d5a76ecf0545b959457f78fe3968759a2306298411
                OutcomeKind::Failed => {
                    let mut state = locked(run);
                    state.reporter.report(Step::Failed, Some(&batch.identifier), None, &outcome.message);
                    // @lfy def/cli/main.lfy:main#main:main:43f2aa5048680432b93b7802c6b6a48f5fcb659a985ce06c84d0358d44a2277d
                    state.reporter.problems(std::slice::from_ref(&outcome.message));
                    // A second failure stops the compile.
                    // @lfy def/cli/main.lfy:main#main:main:a2115c940f2947290bc6ebc3b6560b08cc0b512909ff66d8413cfc366f78bcd9
                    if retried_for_failure {
                        state.failed += 1;
                        state.worsen(ExitCode::Failure);
                        state.stop(batch);
                        return None;
                    }
                    retried_for_failure = true;
                }
            }
        }
    }

    /// No compiler is run; the outputs already on disk are checked, written back
    /// normalized, and recorded when accepted. A batch whose units were all accepted is
    /// verified like any other against the source maps held for it; since no compiler runs
    /// there is nothing to run again, so a violated review rejects the batch at once and
    /// those maps are never recorded.
    // @lfy def/cli/main.lfy:main#main:main:a4f5fbdf4d4599c991622bd9b47997c8c03fd47ec8dedfc46405fa17cd22947b
    fn accept_on_disk(run: &Mutex<Run>, batch: &Batch, root: &Path) {
        // Nothing runs a compiler here, so nothing else is writing the root and the batch
        // needs no copy of its own.
        // @lfy def/cli/main.lfy:main#main:main:a4f5fbdf4d4599c991622bd9b47997c8c03fd47ec8dedfc46405fa17cd22947b
        let where_ = Where::Root(root.to_path_buf());
        let held = {
            let mut state = locked(run);
            let request = state.request_of(batch, root);
            let verdicts = state.verdicts_of(batch, &request, root);
            let accepted: Vec<Verdict> = verdicts.iter().filter(|v| v.accepted).cloned().collect();
            let mut held = state.maps.clone();
            if !accepted.is_empty() {
                let only = Batch {
                    units: batch.units.iter().copied().zip(&verdicts).filter(|(_, v)| v.accepted).map(|(unit, _)| unit).collect(),
                    identifier: batch.identifier.clone(),
                };
                held = state.hold_batch(&only, &accepted, root);
            }
            let rejected = state.report_rejections(batch, &verdicts);
            // A rejected unit makes the code problems; what was accepted is recorded all the
            // same, so a compiler that worked through the agent server has its work recorded.
            // @lfy def/cli/main.lfy:main#main:main:e8df67af0bfbe8da3b5385f4331e5af61c8c556720739d356bf3ecd8ffcb35cd
            // @lfy def/cli/main.lfy:main#main:main:43faad21948267940be8ff5b048b7787640a1b18bbf51a40dd24d462dd7aeb43
            if rejected > 0 {
                state.record_maps(batch, &held);
                state.rejected += rejected;
                state.worsen(ExitCode::Problems);
                return;
            }
            held
        };
        // @lfy def/cli/main.lfy:main#main:main:cfbd77013d5de5003e04314cb55ac7a6f8c091ff6c8a1c889716c385e8ffd907
        let problems = Run::verify_batch(run, batch, &where_, &held);
        let mut state = locked(run);
        if let Some(problems) = problems {
            state.reporter.report(Step::Rejected, Some(&batch.identifier), None, &problems.join("\n"));
            state.reporter.problems(&problems);
            state.unrecord(batch);
            state.worsen(ExitCode::Problems);
            state.stop(batch);
        } else {
            // @lfy def/cli/main.lfy:main#main:main:43faad21948267940be8ff5b048b7787640a1b18bbf51a40dd24d462dd7aeb43
            state.record_maps(batch, &held);
        }
    }

    /// The request of a batch written for a compiler run by hand.
    // @lfy def/cli/main.lfy:main#main:main:3d2332fc257e54c75412463666b0f80d03f3f2eaa2fe2cf9847d6c9e5da2401c
    fn write_request(&mut self, batch: &Batch) {
        let stems = stems_of(&self.plan, batch, " ");
        self.reporter.report(Step::Requesting, Some(&batch.identifier), None, &stems);
        let root = self.workspace().root.clone();
        let request = self.request_of(batch, &root);
        if let Some(path) = self.write_note(batch, "md", &request.instructions)
            && !self.json
        {
            println!("wrote {}", relative(&self.workspace().root, &path));
        }
    }

    /// The last line: the count accepted, rejected, blocked, and failed, each a tally, and
    /// every batch that did not complete on its own line, indented, painted warning. Its glyph
    /// and name are success when nothing was rejected, blocked, or failed and failure when
    /// something was.
    // @lfy def/cli/main.lfy:main#main:main:3489747b8b8c37349030e2e6b46ecf0fd1112f662f7cff22846b43836dd0a62d
    fn finish(&mut self) {
        // @lfy def/cli/main.lfy:main#main:main:5323f65cd6095d90ba92d44febf6910a6322aa6e7f5d1dd2a355bc374b55d7ee
        let counts: Counts = vec![
            (self.accepted, "units accepted", Tone::Success),
            (self.rejected, "rejected", Tone::Failure),
            (self.blocked, "batches blocked", Tone::Warning),
            (self.failed, "failed", Tone::Failure),
        ];
        // Nothing rejected, blocked, or failed: the glyph and name are success; something was:
        // they are failure.
        // @lfy def/cli/main.lfy:main#main:main:039d3bfd25c50ba9d34b15d77ecda97ff4ce12b3f3040faa8fadda1b972a2375
        // @lfy def/cli/main.lfy:main#main:main:8ad6cc67e5917e94bdf860ca278ad8a72e97989115f12275cd1d47dd6d7499d0
        let went_wrong = self.rejected > 0 || self.blocked > 0 || self.failed > 0;
        let tone = if went_wrong { Tone::Failure } else { Tone::Success };
        // With --json the one object carries every batch that did not complete too, since
        // nothing follows a JSON line.
        // @lfy def/cli/main.lfy:main#main:main:5700505ffc866c1665e0871f60ef239b80f5cfa788e6ae0110e7e1ab3980d09f
        if self.json {
            let mut message = counts_message(&counts, false);
            if !self.incomplete.is_empty() {
                message.push_str(&format!("; did not complete: {}", self.incomplete.join(", ")));
            }
            self.reporter.report(Step::Finished, None, None, &message);
            return;
        }
        self.reporter.report_counts(Step::Finished, None, &counts, Some(tone));
        // The batches that did not complete follow on their own line, indented four spaces.
        // @lfy def/cli/main.lfy:main#main:main:afc241e23c3c357ff76bfb3971894263507614876b21b87028fb863ffc9f9926
        // @lfy def/cli/main.lfy:main#main:main:974ee309cbfc12d85b60f2bbb0b8cd1361aa959c94969a5cf6c482e0ec137ba7
        if !self.incomplete.is_empty() {
            let line = format!("did not complete: {}", self.incomplete.join(", "));
            self.reporter.reason(&line);
        }
    }
}

/// The bookkeeping of a compile, locked. A thread that panicked while holding it leaves what
/// it held, which is read as it stands rather than taking the whole compile down with it.
// @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
fn locked(run: &Mutex<Run>) -> std::sync::MutexGuard<'_, Run> {
    run.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What asking the verifier came to: its report, and whether the run that gave it failed, so
/// that the problems of a failed run are not printed twice.
// @lfy def/cli/main.lfy:main#main:main:7b33e4abb60c999fd74ba8a003a59f6de17412062b9767f21eda5ffd7eaf963c
#[derive(Default)]
struct Verification {
    report: ReviewReport,
    failed: bool,
}

/// Whether a run of the verifier failed rather than merely writing something beside its
/// reviews: no line of its output read `ELFIE: REVIEWED`, it held no review at all, or the
/// command could not be started. A run that ended with the end line and at least one review
/// stands as it is, whatever prose surrounds them.
// @lfy def/cli/main.lfy:main#main:main:e13334a45d8bb1815a5cb6ea0c40120b7e2ee52e9ccc44d15376b76b3fa724e8
// @lfy def/cli/main.lfy:main#main:main:7b33e4abb60c999fd74ba8a003a59f6de17412062b9767f21eda5ffd7eaf963c
// @lfy def/cli/main.lfy:main#main:main:9a00d5ab9bc962d57d3c9e59f64a8dbd0034f2260b51fc176602c9b103475660
fn verifier_failed(exit: &Exit, report: &ReviewReport) -> bool {
    // @lfy def/cli/main.lfy:main#main:main:e13334a45d8bb1815a5cb6ea0c40120b7e2ee52e9ccc44d15376b76b3fa724e8
    exit.code == -1 || report.reviews.is_empty() || !exit.stdout.lines().any(|line| line.trim() == REVIEWED)
}

/// One batch run from beginning to end: its own copy of the root where there is one, the
/// compiler, the verifier, and then the merge of its work into the root.
///
/// A batch whose work passed has every file of its copy merged into the root, its source maps
/// recorded there, and its copy removed. A batch that stopped merges nothing and leaves its
/// copy in place for a person to read.
// @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
fn run_batch(run: &Mutex<Run>, batch: &Batch, command: &str, root: &Path, jobs: usize) -> Done {
    // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
    let where_ = Where::of(root, &batch.identifier, jobs);
    // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
    let held = Run::compile_batch(run, batch, command, &where_);
    let Where::Copy(mirror) = &where_ else {
        // The batch ran in the root: there is nothing to merge, and its source maps are
        // recorded where they already are.
        // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
        if let Some(held) = held {
            // @lfy def/cli/main.lfy:main#main:main:5f94d631456802b856fe0bcc8391128da066ebd319c2f4ea466011a8739c6534
            locked(run).record_maps(batch, &held);
        }
        return Done::Merged;
    };
    // The batch stopped: nothing of it is merged and the copy is left in place for a person
    // to read.
    // @lfy def/cli/main.lfy:main#main:main:1c335d58e9fffee616b1561e508393d0f678fd2d3598933dee54aa2a082d5003
    let Some(held) = held else {
        return Done::Merged;
    };
    // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
    match mirror.merge(root) {
        // Every file merged: the source maps are recorded in the root and the copy is
        // removed.
        // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
        Ok(_) => {
            // @lfy def/cli/main.lfy:main#main:main:5f94d631456802b856fe0bcc8391128da066ebd319c2f4ea466011a8739c6534
            locked(run).record_maps(batch, &held);
            // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
            mirror.remove();
            Done::Merged
        }
        // A file could not be merged without a conflict: nothing of the batch is merged, a
        // retrying line names the file, and the batch runs again from a new copy once no
        // other batch is running. This is not a rejection, so no count and no code change.
        // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
        Err(path) => {
            // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
            let message = format!("{path} changed under the batch and could not be merged");
            locked(run).reporter.report(Step::Retrying, Some(&batch.identifier), None, &message);
            // The copy goes, since the batch runs again from a new one; what its units were
            // counted as stands, so the run anew counts them once rather than twice.
            // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
            mirror.remove();
            Done::Conflicted
        }
    }
}

/// Every batch of a round run: a batch starts once every batch holding a dependency of one of
/// its units is done, at most --jobs run at once, and among those that can start they start in
/// plan order. The batches that never started are the ones that did not complete.
// @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
fn run_batches(
    run: &Mutex<Run>,
    planned: &[Planned],
    root: &Path,
    compiler: Option<&str>,
    accept_only: bool,
    jobs: usize,
) {
    let schedule = Mutex::new(Schedule::of(planned));
    let finished = Condvar::new();
    std::thread::scope(|scope| {
        let mut running = Vec::new();
        loop {
            // The lock on the compile is taken and given back before the schedule's, so
            // that no thread ever holds both.
            // @lfy def/cli/main.lfy:main#main:main:3c23880977d6965ce85ac281ad2e12af55f114b91a87851d4a296f402ee2550b
            let halted = locked(run).halted;
            let mut schedules = schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match schedules.next(planned, jobs, halted) {
                // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
                Next::Start(at) => {
                    drop(schedules);
                    let batch = planned[at].batch.clone();
                    // A stopped batch stops only the batches that depend on one of its
                    // units; those never run, and count as not completed.
                    // @lfy def/cli/main.lfy:main#main:main:6c99a3fd756dde171135b4dbd26c28b4aee7e003acc1b178a2f2d54429eeea4a
                    if locked(run).depends_on_stopped(&batch) {
                        let mut state = locked(run);
                        // @lfy def/cli/main.lfy:main#main:main:974ee309cbfc12d85b60f2bbb0b8cd1361aa959c94969a5cf6c482e0ec137ba7
                        state.incomplete.push(batch.identifier.clone());
                        state.stopped.extend(batch.units.iter().copied());
                        drop(state);
                        schedule
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .finish(at, &Done::Merged);
                        continue;
                    }
                    let schedule = &schedule;
                    let finished = &finished;
                    running.push(scope.spawn(move || {
                        let done = match (accept_only, compiler) {
                            // @lfy def/cli/main.lfy:main#main:main:a4f5fbdf4d4599c991622bd9b47997c8c03fd47ec8dedfc46405fa17cd22947b
                            (true, _) => {
                                Run::accept_on_disk(run, &batch, root);
                                Done::Merged
                            }
                            // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
                            (false, Some(command)) => run_batch(run, &batch, command, root, jobs),
                            // @lfy def/cli/main.lfy:main#main:main:3d2332fc257e54c75412463666b0f80d03f3f2eaa2fe2cf9847d6c9e5da2401c
                            (false, None) => {
                                locked(run).write_request(&batch);
                                Done::Merged
                            }
                        };
                        schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner).finish(at, &done);
                        finished.notify_all();
                    }));
                }
                // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
                Next::Wait => {
                    drop(finished.wait(schedules));
                }
                // @lfy def/cli/main.lfy:main#main:main:3c23880977d6965ce85ac281ad2e12af55f114b91a87851d4a296f402ee2550b
                Next::Over => {
                    let unstarted = schedules.unstarted();
                    drop(schedules);
                    let mut state = locked(run);
                    for at in unstarted {
                        // @lfy def/cli/main.lfy:main#main:main:974ee309cbfc12d85b60f2bbb0b8cd1361aa959c94969a5cf6c482e0ec137ba7
                        state.incomplete.push(planned[at].batch.identifier.clone());
                        state.stopped.extend(planned[at].batch.units.iter().copied());
                    }
                    break;
                }
            }
        }
        // The batches already running finish as they would.
        // @lfy def/cli/main.lfy:main#main:main:3c23880977d6965ce85ac281ad2e12af55f114b91a87851d4a296f402ee2550b
        for handle in running {
            let _ = handle.join();
        }
    });
}

/// Every error diagnostic of the program printed; whether there was one.
// @lfy def/cli/main.lfy:main#main:main:10c848648939cefe26a4f80656f058562ad20921dd50559912076440f5fe2dc7
// @lfy def/cli/main.lfy:main#main:main:bdc2e5aa37dd312d79165bc49913ba74aee8f67e048816135ebe62d23c5e3810
fn print_errors(workspace: &Workspace, json: bool, on: bool) -> bool {
    let diagnostics = query::diagnostics_of(workspace, None);
    let errors: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    for diagnostic in &errors {
        print_diagnostic(diagnostic, json, on);
    }
    !errors.is_empty()
}

/// The target --target limits the plan to, when it names a known one.
// @lfy def/cli/main.lfy:main#main:main:f8b290e1fac928822f6a90f053f270ccf4317dbe0ad7db90d725e4b9882322dc
// @lfy def/cli/main.lfy:main#main:main:a9fac2a320d06cf381052df6e84319fa6bd0f84d6a12f610bcda1e8150d5ead2
fn target_named(workspace: &Workspace, invocation: &Invocation) -> Result<Option<String>, ExitCode> {
    let Some(name) = invocation.option("target") else {
        return Ok(None);
    };
    if !workspace.targets.iter().any(|known| known.identifier == name) {
        let known: Vec<&str> = workspace.targets.iter().map(|t| t.identifier.as_str()).collect();
        complain(
            Paint::of(invocation).err,
            None,
            &format!("{name} is not a target; the targets are: {}", known.join(", ")),
        );
        return Err(ExitCode::Usage);
    }
    Ok(Some(name.to_string()))
}

/// Plans the units, runs the compiler on each batch in plan order, and records what it
/// produced.
// @lfy def/cli/main.lfy:main
fn compile(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    // @lfy def/cli/main.lfy:main#main:main:49ba302a459842539a965494f1c08247f6f18cac706517a1c2e944fe3da88f5d
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // The compiler is never handed a program with problems.
    // @lfy def/cli/main.lfy:main#main:main:10c848648939cefe26a4f80656f058562ad20921dd50559912076440f5fe2dc7
    if print_errors(&workspace, json, style.out) {
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // With --target the maps of that target alone are read, so no other target's map files
    // are opened.
    // @lfy def/cli/main.lfy:main#main:main:d4afb45a7911a8a6cccbb1cf66299e4e53ea60b18cc125cd7f3df901a16446c7
    let maps = generation::source_maps_of(&workspace, target.as_deref());
    let legacy = legacy_targets(&workspace, target.as_deref());
    drop(workspace);
    // The units the last global review found violated.
    // @lfy def/cli/main.lfy:main#main:main:d616972fdf1695d2ca0f4f9d8c1d0ab6f76bbe5af87f18daccc1f1c300092e65
    let (program, plan) = planned(root, &maps, &invocation.arguments, invocation.flag("all"), &violated_ids(root));
    // @lfy def/cli/main.lfy:main#main:main:1b5c2f2cb6fa24c09e7c95e0fdfcf07271d9861aaa40628a6344f360e5a4f3c0
    if invocation.flag("dry-run") {
        return dry_run(&program.workspace, &plan, target.as_deref(), json, style.out);
    }
    // Before any batch runs, the maps a target kept in source-map.json are moved into the
    // units' map files and every map file belonging to no unit of the plan is removed.
    // @lfy def/cli/main.lfy:main#main:main:4ede95f73677e1e4b2200b8d8acb50fc3d6ee537b3859a6c26714538dede2a4b
    // @lfy def/cli/main.lfy:main#main:main:c0a6f173443a9a61ce5bf7b0bac002bc6df2efcba347f2a5d3ac185638db26c5
    move_legacy_maps(&program.workspace, &plan, &legacy);
    remove_orphan_maps(&program.workspace, &plan, target.as_deref());

    let reporter = Reporter::new(json, style, 0, log_paths(root));
    let run = Mutex::new(Run::new(program, plan, maps.clone(), reporter, invocation));
    let compiler = compiler_command(root);
    let accept_only = invocation.flag("accept");
    // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
    // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
    let jobs = compile_jobs(invocation, root);
    // The units a violated global review plans are compiled once more in the same compile,
    // and the global review runs once more after them.
    // @lfy def/cli/main.lfy:main#main:main:f3b3525a24fda209b102430025ef51ebb6eab2bd9b4e278ac2b7d72a8e46fd96
    let mut round = 1;
    loop {
        let round_of_batches = {
            let mut state = locked(&run);
            let batches = state.batches_for(target.as_deref());
            let total: usize = batches.iter().map(|&index| state.plan.batches[index].units.len()).sum();
            state.reporter.total = total;
            // The first progress line of a compile is planned, with the count of units
            // planned and of batches, whether anything was planned or not, so that a compile
            // with nothing to do still reads as one run.
            // @lfy def/cli/main.lfy:main#main:main:3489747b8b8c37349030e2e6b46ecf0fd1112f662f7cff22846b43836dd0a62d
            let planned = format!("{total} units planned, {} batches", batches.len());
            state.reporter.report(Step::Planned, None, None, &planned);
            // @lfy def/cli/main.lfy:main#main:main:1c2654d00988fd20e79daf0fdd4ede6f6462a99761112243093d0993e588a150
            if total == 0 && round == 1 && !json {
                // @lfy def/cli/main.lfy:main#main:main:1c2654d00988fd20e79daf0fdd4ede6f6462a99761112243093d0993e588a150
                println!("{}", paint("every unit is up to date", Tone::Success, style.out));
            }
            state.accepted_batch = false;
            // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
            round_of(&state.plan, &batches)
        };
        // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
        run_batches(&run, &round_of_batches, root, compiler.as_deref(), accept_only, jobs);
        // Every batch has been handled and merged into the root: the global criteria and
        // tests are reviewed once, there.
        // @lfy def/cli/main.lfy:main#main:main:4f8dbdb8fc28bbacb10eb19592784f31634f2e8201f9598ee5917ece46cf13af
        // @lfy def/cli/main.lfy:main#main:main:2dcfe6cabe7e1aa71bf93e6eb934b1da6606315fb78fc3dd418d2cc4878dc2e5
        let violated = Run::global_review(&run, root);
        // The second global review of a compile stops it there, and the next compile plans
        // those units with reason violated.
        // @lfy def/cli/main.lfy:main#main:main:bc15bc779dc3c825012b7c066b70d8256fb0fa6cf44002c9343835413372d7c0
        if violated.is_empty() || round == 2 || accept_only || compiler.is_none() {
            if !violated.is_empty() {
                locked(&run).worsen(ExitCode::Problems);
            }
            break;
        }
        // @lfy def/cli/main.lfy:main#main:main:f3b3525a24fda209b102430025ef51ebb6eab2bd9b4e278ac2b7d72a8e46fd96
        let (program, plan) = plan_with(root, &maps, &[], &violated);
        locked(&run).replan(program, plan, 0);
        round = 2;
    }
    let mut state = locked(&run);
    // The last progress line of a compile is finished, with the count accepted, rejected,
    // blocked, and failed.
    // @lfy def/cli/main.lfy:main#main:main:3489747b8b8c37349030e2e6b46ecf0fd1112f662f7cff22846b43836dd0a62d
    state.finish();
    // Every batch accepted is success; a rejection, a block, or a question is problems; a
    // command that could not run is failure.
    // @lfy def/cli/main.lfy:main#main:main:ec6dc86241d03ea455096a6fee081b00ee9db78faea2ab5f36b95b1d38e4b536
    // @lfy def/cli/main.lfy:main#main:main:8b6e3fa8c2b970ba9830679ab3f2fa763239f6bdb74b35ccee02b82f3f49cb19
    // @lfy def/cli/main.lfy:main#main:main:fd1abf25846b2c26daa7f8517bfc55161489793755d16066f188e885d2c7dd29
    state.code
}

// ---- verify -----------------------------------------------------------------------

/// Verify is compile without the compiler: the same plan, the same review request, the same
/// verifier, so a person can ask for an opinion on outputs already recorded. Nothing is
/// compiled and no output is written.
// @lfy def/cli/main.lfy:main#main:main:3bdfedbcd1b4048a4c9e1327dfe40e4fb739577df3cfa5179384ac518798f07d
fn verify(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    // @lfy def/cli/main.lfy:main#main:main:3bdfedbcd1b4048a4c9e1327dfe40e4fb739577df3cfa5179384ac518798f07d
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // Every error diagnostic is printed and verify ends with the code problems.
    // @lfy def/cli/main.lfy:main#main:main:bdc2e5aa37dd312d79165bc49913ba74aee8f67e048816135ebe62d23c5e3810
    if print_errors(&workspace, json, style.out) {
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // Nothing under elfie-compile is recorded, moved, or removed: the maps are only read.
    // @lfy def/cli/main.lfy:main#main:main:3bdfedbcd1b4048a4c9e1327dfe40e4fb739577df3cfa5179384ac518798f07d
    let maps = generation::source_maps_of(&workspace, target.as_deref());
    drop(workspace);
    let (_, known) = plan_with(root, &maps, &[], &[]);
    // Every stem given as a requested unit or, with no stem, every unit whose outputs are
    // not empty.
    // @lfy def/cli/main.lfy:main#main:main:bc465532d7a33b0a2df4db5f2abc60d85a5f06168cb989a911707583a598ed0e
    // @lfy def/cli/main.lfy:main#main:main:41e5715f680f9dea73affa0004e4961f8db17aec7af09083078fa908dc642769
    let mut requested: Vec<String> = Vec::new();
    // --global with no stem reviews no batch.
    // @lfy def/cli/main.lfy:main#main:main:4e1e5413d9d9a90de9ddb43e0d7498c9c7dae9038e4e9f81de821bbc0c135a2a
    let global_only = invocation.flag("global") && invocation.arguments.is_empty();
    // @lfy def/cli/main.lfy:main#main:main:41e5715f680f9dea73affa0004e4961f8db17aec7af09083078fa908dc642769
    if invocation.arguments.is_empty() {
        requested.extend(known.units.iter().filter(|unit| !unit.outputs.is_empty()).map(|unit| unit.stem.clone()));
    } else {
        // @lfy def/cli/main.lfy:main#main:main:bc465532d7a33b0a2df4db5f2abc60d85a5f06168cb989a911707583a598ed0e
        for stem in &invocation.arguments {
            // A stem naming a unit whose outputs are empty is printed as having nothing to
            // review and left out, and the code does not change for it.
            // @lfy def/cli/main.lfy:main#main:main:ae746365d561701b4ed076160e2627f354cb5dfbc74bd103cb41f98c28d77693
            match known.units.iter().find(|unit| &unit.stem == stem) {
                Some(unit) if !unit.outputs.is_empty() => requested.push(stem.clone()),
                _ if json => println!("{}", serde_json::json!({ "stem": stem, "message": "nothing to review" })),
                _ => println!("{stem}: nothing to review"),
            }
        }
    }
    // After any batches are reviewed, every global criterion and test is reviewed once with
    // no stem given, or with --global.
    // @lfy def/cli/main.lfy:main#main:main:f71aa5a984a022b464e4eabdb0b20fd093322e8041b14b8ac9747246929ebeb9
    let globally = invocation.flag("global") || invocation.arguments.is_empty();
    if requested.is_empty() && !globally {
        if !json {
            println!("nothing to review");
        }
        return ExitCode::Success;
    }
    let (program, plan) = plan_with(root, &maps, &requested, &[]);
    let wanted: BTreeSet<&str> = requested.iter().map(String::as_str).collect();
    // Each batch of the plan holding a requested unit, in plan order.
    // @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
    // @lfy def/cli/main.lfy:main#main:main:a9fac2a320d06cf381052df6e84319fa6bd0f84d6a12f610bcda1e8150d5ead2
    let batches: Vec<usize> = (0..plan.batches.len())
        .filter(|&index| {
            let batch = &plan.batches[index];
            target.as_deref().is_none_or(|name| batch_target(&program.workspace, &plan, batch) == name)
                && batch.units.iter().any(|&unit| wanted.contains(plan.units[unit].stem.as_str()))
        })
        .collect();
    let total: usize = batches.iter().map(|&index| plan.batches[index].units.len()).sum();
    let reporter = Reporter::new(json, style, total, log_paths(root));
    let run = Mutex::new(Run::new(program, plan, maps, reporter, invocation));
    let verifier = verifier_command(root);
    // @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
    let jobs = jobs_of(invocation);
    // The source maps already recorded are what the verifier is pointed at here: verify
    // compiles nothing, so there are none to hold.
    // @lfy def/cli/main.lfy:main#main:main:3bdfedbcd1b4048a4c9e1327dfe40e4fb739577df3cfa5179384ac518798f07d
    let recorded = locked(&run).maps.clone();
    // @lfy def/cli/main.lfy:main#main:main:4e1e5413d9d9a90de9ddb43e0d7498c9c7dae9038e4e9f81de821bbc0c135a2a
    let wanted = if global_only { Vec::new() } else { batches };
    // Each batch of the plan holding a requested unit, up to --jobs at once and started in
    // plan order.
    // @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
    let reviewing = round_of(&locked(&run).plan, &wanted);
    verify_batches(&run, &reviewing, root, verifier.as_deref(), &recorded, jobs);
    // @lfy def/cli/main.lfy:main#main:main:f71aa5a984a022b464e4eabdb0b20fd093322e8041b14b8ac9747246929ebeb9
    if globally && verifier.is_some() {
        // Nothing of the compile's own gating applies here: a person asked for the opinion.
        // @lfy def/cli/main.lfy:main#main:main:f71aa5a984a022b464e4eabdb0b20fd093322e8041b14b8ac9747246929ebeb9
        let reviews = Run::verify_globally(&run, root);
        print_reviews(&reviews, json, style.out);
        // @lfy def/cli/main.lfy:main#main:main:c23934c293f91e4d0fd7e7b377cabf5d969bb4fb55719a43b8fc70b6d61f9d54
        if counts_of(&reviews).1 > 0 {
            locked(&run).worsen(ExitCode::Problems);
        }
    }
    // The verifier was started and no review is violated: the code is success; it could not
    // be started: the code is failure.
    // @lfy def/cli/main.lfy:main#main:main:d84f0a9bcbfdcf127934491b8390962cb32846f6fe4b291314cd473d8c2fabbe
    // @lfy def/cli/main.lfy:main#main:main:fd6a921a3570f69a0c04ca8b2b478aca795777e982fe5fe4557e7537db73fe14
    locked(&run).code
}

/// Every batch of a verify reviewed, up to --jobs at once and started in plan order, each in
/// the root: nothing is compiled, so every batch reads the outputs that are already there.
///
/// With no verifier named, the review request of each batch is written for a verifier run by
/// hand and the code is success.
// @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
fn verify_batches(
    run: &Mutex<Run>,
    planned: &[Planned],
    root: &Path,
    verifier: Option<&str>,
    recorded: &[SourceMap],
    jobs: usize,
) {
    let schedule = Mutex::new(Schedule::of(planned));
    let finished = Condvar::new();
    std::thread::scope(|scope| {
        let mut running = Vec::new();
        loop {
            let mut schedules = schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
            match schedules.next(planned, jobs, false) {
                Next::Start(at) => {
                    drop(schedules);
                    let batch = planned[at].batch.clone();
                    let schedule = &schedule;
                    let finished = &finished;
                    running.push(scope.spawn(move || {
                        match verifier {
                            // @lfy def/cli/main.lfy:main#main:main:4c19bc9880ea93ba9993abbd0c2c10e8797eb913aecd8e30a30223d39a488474
                            Some(command) => {
                                // The verifier is run exactly as compile runs it after
                                // acceptance, a failed run being run once more the same way.
                                // @lfy def/cli/main.lfy:main#main:main:9a00d5ab9bc962d57d3c9e59f64a8dbd0034f2260b51fc176602c9b103475660
                                let where_ = Where::Root(root.to_path_buf());
                                let verification =
                                    Run::review_batch(run, &batch, command, &where_, false, recorded);
                                let reviews = verification.report.reviews;
                                let mut state = locked(run);
                                // @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
                                print_reviews(&reviews, state.json, state.style.out);
                                // @lfy def/cli/main.lfy:main#main:main:c23934c293f91e4d0fd7e7b377cabf5d969bb4fb55719a43b8fc70b6d61f9d54
                                if counts_of(&reviews).1 > 0 {
                                    state.worsen(ExitCode::Problems);
                                }
                            }
                            // @lfy def/cli/main.lfy:main#main:main:bd9654344c4cef40114b45abde3e92ca30e92c685747378ea3e269fcf1b63fe4
                            None => locked(run).write_review_request(&batch),
                        }
                        schedule
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .finish(at, &Done::Merged);
                        finished.notify_all();
                    }));
                }
                Next::Wait => {
                    drop(finished.wait(schedules));
                }
                Next::Over => break,
            }
        }
        for handle in running {
            let _ = handle.join();
        }
    });
}

/// Every review printed as its status, a space, the file, a colon, the line, a space, the
/// entity, a colon, a space, and the note, then one line giving the counts of satisfied,
/// violated, and unverifiable. Without --json the status is painted in its own tone and padded
/// to the longest status of [`ReviewStatus`], the file and line are subject, and the entity and
/// the note are plain; with --json each review is one JSON object on one line.
// @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
fn print_reviews(reviews: &[Review], json: bool, on: bool) {
    for review in reviews {
        // @lfy def/cli/main.lfy:main#main:main:9fef7e351a8ffb5c083e5af6dc58271922ba2e3d6fd92b9226ef2f6d0b627065
        if json {
            println!("{}", review.to_json());
        } else {
            // @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
            println!("{}", painted_review(review, on));
        }
    }
    // After each batch one line gives the counts of satisfied, violated, and unverifiable.
    // @lfy def/cli/main.lfy:main#main:main:0602c17d6b9aa65bc3f97a3834871de59d07641bcd9918c26de4764319637bab
    let counts = counts_of(reviews);
    if json {
        println!("{}", serde_json::json!({ "satisfied": counts.0, "violated": counts.1, "unverifiable": counts.2 }));
    } else {
        // @lfy def/cli/main.lfy:main#main:main:b6e482bf8db0a13e8b221786995f0fb5af1792d75b825d64ca1a8f4483c0b6e8
        println!("{}", counts_message(&review_counts(reviews), on));
    }
}

/// One review as the verify command prints it, painted.
// @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
fn painted_review(review: &Review, on: bool) -> String {
    // The status is padded to the longest status of ReviewStatus.
    // @lfy def/cli/main.lfy:main#main:main:ddc5b4d1fde32eff5586a15b9755423789f35def4793871cf38191f1bb20433f
    let column = [ReviewStatus::Satisfied, ReviewStatus::Violated, ReviewStatus::Unverifiable]
        .iter()
        .map(|status| width(&status.to_string()))
        .max()
        .unwrap_or(0);
    // The status is painted in its own tone, the file and line are subject, and the entity
    // and the note are plain.
    // @lfy def/cli/main.lfy:main#main:main:ddc5b4d1fde32eff5586a15b9755423789f35def4793871cf38191f1bb20433f
    let status = padded(&paint(&review.status.to_string(), status_tone(review.status), on), column);
    format!(
        "{status} {} {}: {}",
        paint(&format!("{}:{}", review.file, review.line), Tone::Subject, on),
        review.entity,
        review.note
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

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
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// A project with one target whose marker `global` carries and one file.
    // @lfy def/cli/main.lfy:main
    fn one_target(fixture: &Fixture) -> &Fixture {
        fixture
            .write(
                "elfie.json",
                r#"{
                    "name": "p",
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust", "output": "out" } }
                }"#,
            )
            .write(
                "targets/rust/main.lfy",
                "use \"elfie/target/target\";\n\ntrait rust extends target: `Built as Rust` {\n  .outputDirectory = \"out\";\n  .markerComment = \"//\";\n}\nrust.apply(global);\n",
            )
    }

    /// A project with one target, `def/a.lfy` declaring A with a criterion on line 3, a
    /// compiler that writes an output with a marker for A and ends with `ELFIE: DONE`, and
    /// the verifier given.
    // @lfy def/cli/main.lfy:main
    fn a_compiler_and_a_verifier<'f>(fixture: &'f Fixture, verifier: &str) -> &'f Fixture {
        one_target(fixture)
            .write("def/a.lfy", "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\n")
            .write(
                "compiler.sh",
                "#!/bin/sh\ncat >> \"$ELFIE_ROOT/instructions.txt\"\necho compile >> \"$ELFIE_ROOT/attempts.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:A\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho 'ELFIE: DONE'\n",
            )
            .write("verifier.sh", &verifier_script(fixture, verifier))
            .write(
                "elfie.json",
                r#"{
                    "name": "p",
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust", "output": "out" } },
                    "compiler": "sh compiler.sh",
                    "verifier": "sh verifier.sh"
                }"#,
            )
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
        let (program, _) = plan_with(&fixture.root, &[], &[], &[]);
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
        script.replace("<id>", &criterion_id(fixture))
    }

    /// `verifier.sh` replaced by another script, its `<id>` filled in.
    fn write_verifier(fixture: &Fixture, script: &str) {
        let script = verifier_script(fixture, script);
        fixture.write("verifier.sh", &script);
    }

    /// A verifier writing one violated review of the criterion of `def/a.lfy`, then the end
    /// line.
    const VIOLATED: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho '{\"id\":\"<id>\",\"status\":\"violated\",\"evidence\":\"crates/a/src/a.rs:4-6\",\"note\":\"returns 0 for an empty list\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier writing one violated review of the criterion of `def/a.lfy` whose evidence
    /// names the output under src, then the end line.
    const VIOLATED_IN_SRC: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho '{\"id\":\"<id>\",\"status\":\"violated\",\"evidence\":\"src/a.rs:4-6\",\"note\":\"returns 0 for an empty list\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier writing one satisfied review of the criterion of `def/a.lfy` and no end
    /// line.
    const UNENDED: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\n";

    /// A verifier writing one satisfied review of the criterion of `def/a.lfy`, then the end
    /// line.
    const SATISFIED: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier that satisfies the criterion, writes a line that reads as an object but
    /// names no criterion, and then ends: a report that ended with a review in it stands,
    /// whatever else it holds.
    // @lfy def/cli/main.lfy:main#main:main:7b33e4abb60c999fd74ba8a003a59f6de17412062b9767f21eda5ffd7eaf963c
    const SATISFIED_WITH_A_STRAY_OBJECT: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\necho '{\"thought\":\"I also looked at the tests\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier that satisfies the criterion and writes a line of its own to standard error,
    /// as an agent writes its progress there.
    const SATISFIED_WITH_NOISE: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho 'reading the outputs' >&2\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\necho 'ELFIE: REVIEWED'\n";

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

    // @lfy def/cli/main.lfy:parse#parse:parse:9950bf574b6e14800ed642db3dc3aa89927c17ef13574689ea665810703ae049
    // @lfy def/cli/main.lfy:parse#parse:parse:06b91a7a77a0c2c5fe6d322ae3821056895ceb8a780ca7d3353fc4224244a7fa
    #[test]
    fn check_with_no_arguments_parses() {
        let invocation = parse_arguments(&args(&["check"])).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert!(invocation.arguments.is_empty());
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:92a69bd92ba69e35f172e62ee820fff5befe5e882a02dec0f3244e94f3e80d88
    #[test]
    fn the_root_is_the_nearest_directory_holding_the_manifest() {
        let invocation = parse_arguments(&args(&["check"])).unwrap();
        assert!(Path::new(&invocation.root).join("elfie.json").exists(), "{}", invocation.root);
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:e3c290f038d675be7f56fc738628a4aa9ffa92e3b07e8784a6357fa8b2395385
    // @lfy def/cli/main.lfy:parse#parse:parse:afa30017cb06f5ac342c5f674b5eeb1cce4fc47b8d28911799eee3c809dff1c0
    #[test]
    fn the_tree_flag_is_the_tree_command() {
        let invocation = parse_arguments(&args(&["--tree", "def/a.lfy"])).unwrap();
        assert_eq!(invocation.command, Command::Tree);
        assert_eq!(invocation.arguments, vec!["def/a.lfy".to_string()]);
        // A command named as well still names itself.
        assert_eq!(parse_arguments(&args(&["--tree", "def/a.lfy", "--json"])).unwrap().command, Command::Tree);
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:92c545c12e3310c439130278d8e5daad4eb1ad4428545e1880ccbfa2de0f549b
    #[test]
    fn nothing_naming_a_command_is_help() {
        assert_eq!(parse_arguments(&args(&[])).unwrap().command, Command::Help);
        // --tree followed by no file is followed by no file, so nothing that does not begin
        // with a dash is given and the command is help.
        assert_eq!(parse_arguments(&args(&["--tree"])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["--tree", "--json"])).unwrap().command, Command::Help);
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:3fd497a063b547c5ecbba95bd47044f648030f147a69b7241615811b47f2f09d
    // @lfy def/cli/main.lfy:parse#parse:parse:1875232948f1e78b7879e17a6142dbc7d23a119e9a6954e465036b47195bed69
    #[test]
    fn an_unknown_command_is_a_usage_error() {
        let error = parse_arguments(&args(&["frobnicate"])).unwrap_err();
        assert!(error.contains("frobnicate"));
        // An option's value is missing: a usage message naming the argument.
        let error = parse_arguments(&args(&["compile", "--target"])).unwrap_err();
        assert!(error.contains("target"), "{error}");
        let error = parse_arguments(&args(&["compile", "--root"])).unwrap_err();
        assert!(error.contains("--root"), "{error}");
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:c6ef1cd5d1ee70ed5efe7e765138382e292ba2e31c91916182971283cce5fc2e
    // @lfy def/cli/main.lfy:parse#parse:parse:0c1236254ed2dc7214279b9f60c85ce5ef12cdbea38d4a41543459b1268af732
    #[test]
    fn an_option_written_with_an_equals_sign_gives_its_text() {
        let invocation = parse_arguments(&args(&["compile", "--target=rust"])).unwrap();
        assert_eq!(invocation.option("target"), Some("rust"));
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:31d286a6e27d0b3b1aee3510e0c91b34c50892ae2bb9d66fe5da061ccb6d6d01
    #[test]
    fn target_takes_the_value_that_follows_it() {
        let invocation = parse_arguments(&args(&["compile", "--target", "rust"])).unwrap();
        assert_eq!(invocation.option("target"), Some("rust"));
        // --jobs is the other option besides --root that takes a following value.
        // @lfy def/cli/main.lfy:parse#parse:parse:31d286a6e27d0b3b1aee3510e0c91b34c50892ae2bb9d66fe5da061ccb6d6d01
        assert_eq!(parse_arguments(&args(&["compile", "--jobs", "8"])).unwrap().option("jobs"), Some("8"));
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:c13f6c1685e6037f390557f71f93bf09b652b2e862a8aecc750e49f518a644d3
    #[test]
    fn the_root_option_names_the_project_directory() {
        assert_eq!(parse_arguments(&args(&["check", "--root", "/tmp"])).unwrap().root, "/tmp");
        assert_eq!(parse_arguments(&args(&["check", "--root=/tmp"])).unwrap().root, "/tmp");
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:7473c896dd710a7b0dcccc8450958126fa8e9c70c04172f551e9a551ff75afa3
    #[test]
    fn what_names_neither_the_command_nor_an_option_value_is_positional() {
        let invocation =
            parse_arguments(&args(&["compile", "--dry-run", "--target", "rust", "--root", "/tmp", "lexer/main"]))
                .unwrap();
        assert!(invocation.flag("dry-run"));
        assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
        // The value --color takes is no positional argument either.
        let invocation = parse_arguments(&args(&["compile", "--color", "never", "lexer/main"])).unwrap();
        assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:7bcf6480b22f39361f8cf17177eeb14d00016bfcdcae4c115113d275a3436eaf
    // @lfy def/cli/main.lfy:parse#parse:parse:30bfe1626040570c321d5980500d0c1db321cc4fbc6de0760fbd15fcb9d741df
    #[test]
    fn help_and_version_win_anywhere() {
        assert_eq!(parse_arguments(&args(&["check", "--help"])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["-h"])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["--version"])).unwrap().command, Command::Version);
        assert_eq!(parse_arguments(&args(&["check", "--version"])).unwrap().command, Command::Version);
        // An argument that does not begin with a dash names no command when --help is among
        // them.
        assert_eq!(parse_arguments(&args(&["--help", "frobnicate"])).unwrap().command, Command::Help);
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:414b6a3814d9b33eb9ee85b7db8bcea349d0753972f675579d3a2645c4e54547
    #[test]
    fn help_and_version_take_no_positional_arguments() {
        for arguments in [args(&["--help", "frobnicate"]), args(&["-h", "check", "x"]), args(&["--version", "x"])] {
            let invocation = parse_arguments(&arguments).unwrap();
            assert!(invocation.arguments.is_empty(), "{:?}", invocation.arguments);
        }
        // The root and the options are still read as they are written.
        let invocation = parse_arguments(&args(&["--help", "--root", "/tmp", "--json"])).unwrap();
        assert_eq!(invocation.root, "/tmp");
        assert!(invocation.flag("json"));
        assert!(invocation.arguments.is_empty());
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:95e98fdf72dec65667d15defc92aa98f02e4a2d19663a4159b088872839bb8d3
    // @lfy def/cli/main.lfy:parse#parse:parse:372de213ff4328da7d560b94bc55aaa40b317fbfb13ed63c19833a9b840bb5f9
    #[test]
    fn color_followed_by_a_choice_takes_it_as_its_value() {
        let invocation = parse_arguments(&args(&["--color", "never", "check"])).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert_eq!(invocation.option("color"), Some("never"));
        let invocation = parse_arguments(&args(&["check", "--color=always"])).unwrap();
        assert_eq!(invocation.option("color"), Some("always"));
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:3494bf75f2354dcf6b331282875ea7d823eba517ad9b03427013e5bbd35f1fe2
    #[test]
    fn color_not_followed_by_a_choice_is_a_flag() {
        // The next argument is left for what follows, so it still names the command.
        let invocation = parse_arguments(&args(&["--color", "check"])).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert!(invocation.flag("color"));
        assert_eq!(invocation.option("color"), None);
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:0be5993fbfc9cfaa0fa07c774c7656d48b82dddb25cffe651e98bfddf6211d41
    // @lfy def/cli/main.lfy:parse#parse:parse:5f6a4cec2476302e1f65b3f63fd1793dbdea9475e1d5856222fcb8f1396bc841
    #[test]
    fn a_color_value_that_names_no_choice_is_a_usage_message() {
        let error = parse_arguments(&args(&["check", "--color=sometimes"])).unwrap_err();
        assert_eq!(error, "--color must be auto, always, or never");
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:d19a41c8634f25c25ba5bfe17c06df12102331e68a6fe8764d48bf0936fbcb24
    #[test]
    fn every_other_flag_never_takes_the_next_argument() {
        let invocation = parse_arguments(&args(&["--json", "check"])).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert!(invocation.flag("json"));
        let invocation = parse_arguments(&args(&["--strict", "--no-verify", "check"])).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert!(invocation.flag("strict") && invocation.flag("no-verify"));
    }

    /// What --color asked for: the value it names, always for the bare flag, and auto when it
    /// is not given.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn the_color_choice_is_what_was_asked_for() {
        // @lfy def/cli/main.lfy:main
        assert_eq!(color_choice(&parse_arguments(&args(&["--color", "never", "check"])).unwrap()), ColorChoice::Never);
        // @lfy def/cli/main.lfy:main
        assert_eq!(color_choice(&parse_arguments(&args(&["check", "--color=always"])).unwrap()), ColorChoice::Always);
        // The bare flag is read as always. @lfy def/cli/main.lfy:main
        assert_eq!(color_choice(&parse_arguments(&args(&["--color", "check"])).unwrap()), ColorChoice::Always);
        // Not given at all, the choice is auto. @lfy def/cli/main.lfy:main
        assert_eq!(color_choice(&parse_arguments(&args(&["check"])).unwrap()), ColorChoice::Auto);
    }

    /// With --json nothing is painted on either stream, whatever --color says.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn json_paints_nothing_and_always_paints_both_streams() {
        let invocation = parse_arguments(&args(&["check", "--json", "--color=always"])).unwrap();
        // @lfy def/cli/main.lfy:main
        let style = Paint::of(&invocation);
        assert!(!style.out && !style.err);
        // Each stream is decided apart, and always wins over the environment.
        // @lfy def/cli/main.lfy:main
        let invocation = parse_arguments(&args(&["check", "--color=always"])).unwrap();
        let style = Paint::of(&invocation);
        assert!(style.out && style.err);
        // @lfy def/cli/main.lfy:main
        let invocation = parse_arguments(&args(&["check", "--color=never"])).unwrap();
        let style = Paint::of(&invocation);
        assert!(!style.out && !style.err);
        // Arguments that spell no invocation are read the same way, so a usage message obeys
        // the --color choice the person wrote and --json. @lfy def/cli/main.lfy:main
        assert_eq!(choice_in(&args(&["--color=never", "frobnicate"])), ColorChoice::Never);
        assert_eq!(choice_in(&args(&["--color", "always", "frobnicate"])), ColorChoice::Always);
        // The bare flag is read as always, and nothing given at all is auto.
        // @lfy def/cli/main.lfy:main
        assert_eq!(choice_in(&args(&["--color", "frobnicate"])), ColorChoice::Always);
        assert_eq!(choice_in(&args(&["frobnicate"])), ColorChoice::Auto);
        // @lfy def/cli/main.lfy:main
        let style = Paint::of_arguments(&args(&["--color=never", "frobnicate"]));
        assert!(!style.out && !style.err);
        // @lfy def/cli/main.lfy:main
        let style = Paint::of_arguments(&args(&["--color", "always", "frobnicate"]));
        assert!(style.out && style.err);
        // With --json nothing is painted on either stream, whatever --color says, even when
        // the arguments are a usage error. @lfy def/cli/main.lfy:main
        let style = Paint::of_arguments(&args(&["--json", "check", "--color=sometimes"]));
        assert!(!style.out && !style.err);
        // @lfy def/cli/main.lfy:main
        let style = Paint::of_arguments(&args(&["--json", "--color", "always", "frobnicate"]));
        assert!(!style.out && !style.err);
    }

    /// A message the CLI prints to standard error begins with elfie: painted failure, and a
    /// path that begins it is subject.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_message_to_standard_error_names_itself() {
        // The words are the same painted or not, so a script reads what it read before.
        // @lfy def/cli/main.lfy:main
        complain(false, None, "frobnicate is not a command");
        // @lfy def/cli/main.lfy:main
        assert_eq!(complaint(false, None, "frobnicate is not a command"), "elfie: frobnicate is not a command");
        // @lfy def/cli/main.lfy:main
        complain(true, Some("def/a.lfy"), "no such file");
        // @lfy def/cli/main.lfy:main
        assert_eq!(complaint(false, Some("def/a.lfy"), "no such file"), "elfie: def/a.lfy: no such file");
        // elfie: is painted failure and a path that begins the message is subject.
        // @lfy def/cli/main.lfy:main
        let painted = complaint(true, Some("def/a.lfy"), "no such file");
        assert!(painted.starts_with(&paint("elfie:", Tone::Failure, true)), "{painted:?}");
        assert!(painted.contains(&paint("def/a.lfy", Tone::Subject, true)), "{painted:?}");
        // Painting leaves the width of the prefix as it was.
        // @lfy def/cli/main.lfy:main
        assert_eq!(width(&paint("elfie:", Tone::Failure, true)), width("elfie:"));
        assert_eq!(width(&painted), width(&complaint(false, Some("def/a.lfy"), "no such file")));
    }

    /// A usage message is printed with the help text and the code is usage.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_usage_error_returns_the_usage_code() {
        assert_eq!(run(&["frobnicate".to_string()]), ExitCode::Usage.code());
    }

    /// Help prints every command of [`Command`], verify among them, with one line of
    /// description and the global options, --no-verify, --strict, --jobs, and --color among
    /// them, and version prints the version; both return success.
    // @lfy def/cli/main.lfy:main#main:main:1209be9acca3fe8503b9fbe8c9175d7c0272512b07cdcfd341cac2ca4696dfaa
    #[test]
    fn help_and_version_return_success() {
        let text = help_text(false);
        // @lfy def/cli/main.lfy:main#main:main:1209be9acca3fe8503b9fbe8c9175d7c0272512b07cdcfd341cac2ca4696dfaa
        for command in Command::ALL {
            assert!(text.contains(command.value()), "{text}");
            assert!(text.contains(command.description()), "{text}");
        }
        assert!(text.contains(Command::Verify.value()), "{text}");
        // @lfy def/cli/main.lfy:main#main:main:1209be9acca3fe8503b9fbe8c9175d7c0272512b07cdcfd341cac2ca4696dfaa
        for option in ["--root", "--json", "--no-verify", "--strict", "--jobs", "--color", "--help, -h", "--version"] {
            assert!(text.contains(option), "{option} is missing from {text}");
        }
        // Every description starts in the same column, painted or not.
        // @lfy def/cli/main.lfy:main
        let painted = help_text(true);
        let starts: BTreeSet<usize> = Command::ALL
            .iter()
            .map(|command| width(command.value()))
            .chain(OPTIONS.iter().map(|(name, _)| width(name)))
            .collect();
        let column = starts.iter().copied().max().unwrap_or(0) + 2;
        for (name, description) in OPTIONS {
            let line = format!("  {}{description}", padded(name, column));
            assert!(text.contains(&line), "{line:?} is not in {text}");
        }
        // The painted text holds the same words. @lfy def/cli/main.lfy:main
        for command in Command::ALL {
            assert!(painted.contains(command.description()), "{painted}");
        }
        let invocation = parse_arguments(&args(&["help", "--color=never"])).unwrap();
        assert_eq!(help(&invocation), ExitCode::Success);
        // @lfy def/cli/main.lfy:main
        assert_eq!(version(), ExitCode::Success);
    }

    /// Init writes elfie.json naming the project after the argument, creates the source
    /// directory and elfie-compile/maps, writes .gitignore holding one line reading
    /// /elfie-compile/cache/, and fails when elfie.json already exists.
    // @lfy def/cli/main.lfy:main#main:main:62e086f2f773784e6e958bc91430daf9aa0450b53dc61dbcad867069e4182d7e
    #[test]
    fn init_creates_the_manifest_once() {
        let fixture = Fixture::new();
        assert_eq!(fixture.run(&["init", "demo"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main
        assert!(fixture.read("elfie.json").contains("\"demo\""));
        assert!(fixture.root.join("def").is_dir());
        // @lfy def/cli/main.lfy:main
        assert!(fixture.root.join("elfie-compile/maps").is_dir());
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.read(".gitignore"), "/elfie-compile/cache/\n");
        // Nothing ignores elfie-compile itself or the maps, because the maps are checked in.
        // @lfy def/cli/main.lfy:main
        let ignored = fixture.read(".gitignore");
        assert!(!ignored.lines().any(|line| line.trim() == "/elfie-compile/" || line.trim() == "elfie-compile"));
        assert!(!ignored.contains("maps"));
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["init", "demo"]), ExitCode::Failure.code());
        // The project is named after the directory when no argument is given.
        // @lfy def/cli/main.lfy:main
        let plain = Fixture::new();
        assert_eq!(plain.run(&["init"]), ExitCode::Success.code());
        let name = plain.root.file_name().unwrap().to_string_lossy().into_owned();
        assert!(plain.read("elfie.json").contains(&name), "{}", plain.read("elfie.json"));
        // A .gitignore that has no such line keeps what it had and gains the line on a line
        // of its own. @lfy def/cli/main.lfy:main
        let kept = Fixture::new();
        kept.write(".gitignore", "/target");
        assert_eq!(kept.run(&["init", "k"]), ExitCode::Success.code());
        assert_eq!(kept.read(".gitignore"), "/target\n/elfie-compile/cache/\n");
        // A .gitignore that already has it is left as it is. @lfy def/cli/main.lfy:main
        let had = Fixture::new();
        had.write(".gitignore", "/elfie-compile/cache/\n/target\n");
        assert_eq!(had.run(&["init", "h"]), ExitCode::Success.code());
        assert_eq!(had.read(".gitignore"), "/elfie-compile/cache/\n/target\n");
    }

    /// Check prints the binder error of the one file and the code is problems.
    // @lfy def/cli/main.lfy:main#main:main:aee3d48625bec29154f75f242791ed5f8eec9cb699656380cbd0294a4f8ecbf7
    #[test]
    fn check_reports_the_binder_error_of_a_file() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = z;\n");
        assert_eq!(fixture.run(&["check"]), ExitCode::Problems.code());
        let diagnostics = query::diagnostics_of(&workspace::load(&fixture.root), None);
        // The standard library is part of the program, so only the project's own file is
        // this file's to report on.
        let mine: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.range.file == "def/a.lfy").collect();
        assert_eq!(mine.len(), 1);
        let printed = format!(
            "{}:{}:{}: {} {}: {}",
            mine[0].range.file, mine[0].range.start.line, mine[0].range.start.column, mine[0].stage, mine[0].severity, mine[0].message
        );
        // @lfy def/cli/main.lfy:main
        assert_eq!(printed, "def/a.lfy:1:10: binder error: z is not declared in scope");
        // A file given as an argument narrows the diagnostics. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["check", "def/a.lfy"]), ExitCode::Problems.code());
    }

    /// Check returns success when there is nothing to report.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn check_returns_success_when_there_are_no_problems() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = 1;\n");
        assert_eq!(fixture.run(&["check"]), ExitCode::Success.code());
    }

    /// With no files given and no error, a unit that is not up to date is printed as a
    /// warning of stage generation naming the target, the unit, and the description of its
    /// reason, and the last line counts the files, the problems, and the stale units.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn check_prints_stale_units_as_warnings() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        assert_eq!(fixture.run(&["check"]), ExitCode::Success.code());
        // A file given as an argument skips the stale-unit report. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["check", "def/a.lfy"]), ExitCode::Success.code());
    }

    /// The count of stale units matches the plan, and the message names the target, the
    /// unit, and the description of the reason.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn print_stale_names_the_target_the_unit_and_the_reason() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        assert_eq!(plan.units.len(), 1);
        assert_eq!(plan.units[0].reason, Some(Reason::Fresh));
        print_stale(&program.workspace, &plan.units[0], Reason::Fresh, false, false);
        // The description of the reason is painted in the reason's own tone.
        // @lfy def/cli/main.lfy:main
        assert_eq!(reason_tone(plan.units[0].reason), Tone::Success);
        print_stale(&program.workspace, &plan.units[0], Reason::Fresh, false, true);
    }

    /// With --strict the code is problems when anything at all was printed as a warning or
    /// error, a stale unit included, even though the same project without --strict is
    /// success.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn check_strict_fails_on_a_stale_unit() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        assert_eq!(fixture.run(&["check"]), ExitCode::Success.code());
        assert_eq!(fixture.run(&["check", "--strict"]), ExitCode::Problems.code());
        // With every unit up to date, --strict is success too. @lfy def/cli/main.lfy:main
        fixture.write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n");
        assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
        assert_eq!(fixture.run(&["check", "--strict"]), ExitCode::Success.code());
    }

    /// Format with --check writes nothing, prints the file that would change, and the code
    /// is problems.
    // @lfy def/cli/main.lfy:main#main:main:3fbe8b3c0216de28ee03acabf6f8094637f58313fd15ab4b1a7ed940933d4151
    #[test]
    fn format_check_prints_the_file_and_leaves_it_alone() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const   x=1;\n");
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["format", "--check"]), ExitCode::Problems.code());
        assert_eq!(fixture.read("def/a.lfy"), "const   x=1;\n");
        // Without --check the file is replaced by the standard layout.
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["format"]), ExitCode::Success.code());
        assert_eq!(fixture.read("def/a.lfy"), "const x = 1;\n");
        // And formatting it again changes nothing.
        assert_eq!(fixture.run(&["format", "--check"]), ExitCode::Success.code());
    }

    /// A file whose tree has errors is left as it is and the code is problems.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn format_leaves_a_file_with_errors_alone() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const x = ;\n");
        assert_eq!(fixture.run(&["format"]), ExitCode::Problems.code());
        assert_eq!(fixture.read("def/a.lfy"), "const x = ;\n");
    }

    /// Tree prints the parse of the one file given, and tokens its tokens.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn tree_and_tokens_take_one_file() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const x = 1;\n");
        let path = relative(Path::new(""), &fixture.root.join("def/a.lfy"));
        assert_eq!(run(&["tree".to_string(), path.clone()]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main
        assert_eq!(run(&["tokens".to_string(), path]), ExitCode::Success.code());
        // Neither takes more than one file. @lfy def/cli/main.lfy:main
        assert_eq!(run(&["tree".to_string()]), ExitCode::Usage.code());
    }

    /// With --json tree and tokens print their results as JSON lines instead of text, one
    /// object per result.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn json_prints_one_object_per_result() {
        let source = "const x = 1;\n";
        let tokens = lex(source, Some("def/a.lfy")).unwrap();
        // One object per token, each on one line. @lfy def/cli/main.lfy:main
        let text = token_lines(&tokens, false, false);
        let objects = token_lines(&tokens, true, false);
        assert_eq!(objects.len(), tokens.len());
        assert_eq!(objects.len(), text.len());
        assert_ne!(objects, text);
        let first: serde_json::Value = serde_json::from_str(&objects[0]).unwrap();
        assert_eq!(first["line"], tokens[0].line);
        assert_eq!(first["column"], tokens[0].column);
        assert_eq!(first["rule"], rule_name(&tokens[0]));
        assert_eq!(first["value"], tokens[0].value);
        // One object per node or token of the tree. @lfy def/cli/main.lfy:main
        let tree = parse(tokens, None);
        let text = tree_lines(&tree, "def/a.lfy", false, false);
        let objects = tree_lines(&tree, "def/a.lfy", true, false);
        assert_ne!(objects, text);
        let parsed: Vec<serde_json::Value> =
            objects.iter().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(parsed[0]["kind"], "node");
        assert_eq!(parsed[0]["depth"], 0);
        assert_eq!(parsed[0]["rule"], tree.root.rule.identifier());
        assert_eq!(parsed[0]["start"], 0);
        assert_eq!(parsed[0]["end"], tree.tokens.len());
        let leaves: Vec<&serde_json::Value> = parsed.iter().filter(|value| value["kind"] == "token").collect();
        assert_eq!(leaves.len(), tree.tokens.len(), "every token is one object");
        assert_eq!(leaves[0]["value"], tree.tokens[0].raw);
        assert_eq!(leaves[0]["rule"], rule_name(&tree.tokens[0]));
        // An error node is given with what it expected, as the text form gives it.
        // @lfy def/cli/main.lfy:main
        let broken = parse(lex("const x = ;\n", Some("def/a.lfy")).unwrap(), None);
        assert!(
            tree_lines(&broken, "def/a.lfy", false, false).iter().any(|line| line.starts_with("error at def/a.lfy:"))
        );
        let errors: Vec<serde_json::Value> = tree_lines(&broken, "def/a.lfy", true, false)
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|value: &serde_json::Value| value["kind"] == "error")
            .collect();
        assert_eq!(errors.len(), broken.errors.len());
        assert_eq!(errors[0]["expected"], serde_json::json!(broken.errors[0].expected));
        assert_eq!(errors[0]["line"], error_position(&broken, &broken.errors[0]).0);
        // A rule is active, a token's value is success, and a token range, line, or column is
        // muted; an error node and what it expected are failure. The words stay the same.
        // @lfy def/cli/main.lfy:main
        let plain = tree_lines(&broken, "def/a.lfy", false, false);
        let painted = tree_lines(&broken, "def/a.lfy", false, true);
        assert_eq!(painted.len(), plain.len());
        for (painted, plain) in painted.iter().zip(&plain) {
            assert_eq!(width(painted), width(plain), "{painted:?}");
            assert_ne!(painted, plain, "{plain:?} is painted");
        }
        // @lfy def/cli/main.lfy:main
        let painted = token_lines(&tokens_of("const x = 1;\n"), false, true);
        let plain = token_lines(&tokens_of("const x = 1;\n"), false, false);
        for (painted, plain) in painted.iter().zip(&plain) {
            assert_eq!(width(painted), width(plain));
        }
        // The commands themselves take the flag. @lfy def/cli/main.lfy:main
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", source);
        let path = relative(Path::new(""), &fixture.root.join("def/a.lfy"));
        assert_eq!(run(&["tokens".to_string(), path.clone(), "--json".to_string()]), ExitCode::Success.code());
        assert_eq!(run(&["tree".to_string(), path, "--json".to_string()]), ExitCode::Success.code());
        // Every command that prints a result of its own prints it as one object with --json
        // and as text otherwise, so no line of standard output is anything but JSON.
        // @lfy def/cli/main.lfy:main
        let lines = [
            ("p/elfie.json", created_line(true, "p/elfie.json")),
            ("def/a.lfy", formatted_line(true, "def/a.lfy", false, false)),
            ("def/a.lfy", formatted_line(true, "def/a.lfy", true, false)),
        ];
        for (file, line) in &lines {
            let value: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|_| panic!("{line} is no object"));
            assert_eq!(value["file"], *file, "{line}");
            assert_eq!(line.lines().count(), 1, "{line}");
        }
        assert_eq!(created_line(false, "p/elfie.json"), "created p/elfie.json");
        // @lfy def/cli/main.lfy:main
        assert_eq!(formatted_line(false, "def/a.lfy", true, false), "def/a.lfy");
        assert_eq!(formatted_line(false, "def/a.lfy", false, false), "formatted def/a.lfy");
        // A file written and a file --check found would change are told apart.
        // @lfy def/cli/main.lfy:main
        assert_ne!(formatted_line(true, "def/a.lfy", false, false), formatted_line(true, "def/a.lfy", true, false));
    }

    /// Compile with --dry-run prints one unit with reason fresh and one batch holding it,
    /// generates nothing, and returns success.
    // @lfy def/cli/main.lfy:main#main:main:429f48f7dff6be9a0e438f5d769234845188d197ea4f7f520f3518d707132956
    #[test]
    fn dry_run_prints_the_units_then_the_batches() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        assert!(program.workspace.problems.is_empty(), "{:?}", program.workspace.problems);
        assert_eq!(plan.units.len(), 1);
        // The words and their order stay as they are, so a script splitting a line on spaces
        // reads the same fields. @lfy def/cli/main.lfy:main
        assert_eq!(unit_line(&program.workspace, &plan, 0, false), "rust a fresh []");
        assert_eq!(width(&unit_line(&program.workspace, &plan, 0, true)), width("rust a fresh []"));
        // @lfy def/cli/main.lfy:main
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(batch_line(&plan, &plan.batches[0], false), "batch a [a]");
        assert_eq!(width(&batch_line(&plan, &plan.batches[0], true)), width("batch a [a]"));
        assert_eq!(fixture.run(&["compile", "--dry-run"]), ExitCode::Success.code());
        assert!(!fixture.root.join("out").exists(), "nothing is generated");
        // --target limits the plan to one target. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--dry-run", "--target", "rust"]), ExitCode::Success.code());
        assert_eq!(fixture.run(&["compile", "--dry-run", "--target", "go"]), ExitCode::Usage.code());
    }

    /// Compile prints the errors and returns problems rather than handing the compiler a
    /// program with problems.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn compile_refuses_a_program_with_problems() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "const y = z;\n");
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        assert!(!fixture.root.join("out").exists());
    }

    /// With no compiler named, the request of each batch is written to elfie-requests under
    /// the root, one file per batch named by its identifier, and the code is success.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_project_with_no_compiler_writes_the_requests() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        let instructions = fixture.read("elfie-requests/a.md");
        assert!(instructions.contains("An A"), "{instructions}");
        assert!(instructions.contains("def/a.lfy"), "{instructions}");
        // Every progress line is appended to the log, as printed with colors off.
        // @lfy def/cli/main.lfy:main
        let log = fixture.read("elfie-requests/compile.log");
        assert!(log.contains("[0/1]"), "{log}");
        assert!(log.contains("◆ planned"), "{log}");
        assert!(log.contains("· requesting"), "{log}");
        assert!(log.contains("◆ finished"), "{log}");
        // @lfy def/cli/main.lfy:main
        assert!(!log.contains('\u{1b}'), "the log holds an escape");
        // Nothing of the CLI's own is left under the output directory.
        // @lfy def/cli/main.lfy:main
        assert!(!fixture.root.join("out/elfie-requests").exists());
    }

    /// Each progress line reads, in columns separated by two spaces: the counter, the time,
    /// the step with its glyph, then the batch and unit when there are any, and the first line
    /// of the message. The first line of a compile is planned with the counts, and the last is
    /// finished with the count accepted, rejected, blocked, and failed.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_progress_line_carries_the_step_the_counts_and_the_elapsed_seconds() {
        let progress = Progress {
            step: Step::Accepted,
            batch: Some("a+1".to_string()),
            unit: Some("cli/main".to_string()),
            done: 2,
            total: 7,
            elapsed: 12.34,
            message: "3 outputs\nand more".to_string(),
        };
        // The counter, the time right aligned to six columns, the step padded to the longest
        // name of Step with its glyph, the batch, the unit, and the first line of the message.
        // @lfy def/cli/main.lfy:main
        let line = text_of(&progress, None, None, false);
        assert_eq!(
            line,
            format!("[2/7]   12.3s  {}  a+1  cli/main  3 outputs", padded("✓ accepted", step_column()))
        );
        // Every column is separated by two spaces, in order. @lfy def/cli/main.lfy:main
        let columns: Vec<&str> =
            line.split("  ").map(str::trim).filter(|column| !column.is_empty()).collect();
        assert_eq!(columns, vec!["[2/7]", "12.3s", "✓ accepted", "a+1", "cli/main", "3 outputs"], "{line:?}");
        let value = json_of(&progress);
        assert_eq!(value["step"], "accepted");
        assert_eq!(value["batch"], "a+1");
        assert_eq!(value["unit"], "cli/main");
        assert_eq!(value["done"], 2);
        assert_eq!(value["total"], 7);
        assert_eq!(value["message"], "3 outputs\nand more");
        // Painting leaves every column the width it had. @lfy def/cli/main.lfy:main
        assert_eq!(width(&text_of(&progress, None, None, true)), width(&line));
        // The counter keeps one width for the whole compile: done is right aligned to the
        // width of total. @lfy def/cli/main.lfy:main
        let wide = Progress { done: 9, total: 100, ..progress.clone() };
        assert!(text_of(&wide, None, None, false).starts_with("[  9/100]"));
        // The unit is not repeated after the batch. @lfy def/cli/main.lfy:main
        let same = Progress { unit: Some("a+1".to_string()), ..progress.clone() };
        assert_eq!(text_of(&same, None, None, false).matches("a+1").count(), 1);
        // @lfy def/cli/main.lfy:main
        let planned = Progress {
            step: Step::Planned,
            batch: None,
            unit: None,
            done: 0,
            total: 7,
            elapsed: 0.0,
            message: "7 units planned, 3 batches".to_string(),
        };
        assert_eq!(
            text_of(&planned, None, None, false),
            format!("[0/7]    0.0s  {}  7 units planned, 3 batches", padded("◆ planned", step_column()))
        );
        assert_eq!(json_of(&planned)["batch"], serde_json::Value::Null);
        // A reviewed line's message is the counts, each a tally; painted or not it holds the
        // same words. @lfy def/cli/main.lfy:main
        let counts: Counts = vec![
            (1, "satisfied", Tone::Success),
            (0, "violated", Tone::Failure),
            (0, "unverifiable", Tone::Warning),
        ];
        assert_eq!(counts_message(&counts, false), "1 satisfied, 0 violated, 0 unverifiable");
        assert_eq!(width(&counts_message(&counts, true)), width(&counts_message(&counts, false)));
        // A rejection's message is painted in the step's tone, and a step that is none of
        // rejected, failed, blocked, or clarification is plain. @lfy def/cli/main.lfy:main
        let rejected = Progress { step: Step::Rejected, ..progress.clone() };
        assert!(text_of(&rejected, None, None, true).ends_with(&paint("3 outputs", Tone::Failure, true)));
        assert!(text_of(&progress, None, None, true).ends_with("3 outputs"));
    }

    /// A batch identifier holds the stem's slashes, which a file name may not.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_batch_names_its_note_files() {
        assert_eq!(file_name_of("cli/main+1"), "cli-main+1");
        assert_eq!(file_name_of("a"), "a");
    }

    /// The answer a person wrote below the question is read back; a question with none
    /// gives nothing.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn the_answer_below_a_question_is_read_back() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
        let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
        let run = Run::new(program, plan, Vec::new(), reporter, &invocation);
        let batch = run.plan.batches[0].clone();
        let unanswered = format!("# Asked\n\nWhich one?\n\n{ANSWER_HEADING}\n\n<!-- Write the answer below this line. -->\n");
        fixture.write("elfie-requests/a.question.md", &unanswered);
        assert_eq!(run.answer_of(&batch), None);
        fixture.write("elfie-requests/a.question.md", &format!("{unanswered}\nThe second one.\n"));
        assert_eq!(run.answer_of(&batch).as_deref(), Some("The second one."));
    }

    /// A stopped batch stops only the batches that depend on one of its units, and the
    /// finished line lists every batch that did not complete.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_stopped_batch_stops_what_depends_on_it() {
        let fixture = Fixture::new();
        one_target(&fixture)
            .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
            .write("def/b.lfy", "d B {}\n")
            .write("def/c.lfy", "d C {}\n");
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        assert!(program.workspace.problems.is_empty(), "{:?}", program.workspace.problems);
        let invocation = parse_arguments(&args(&["compile", "--continue", "--color=never"])).unwrap();
        let reporter = Reporter::new(false, Paint::of(&invocation), plan.units.len(), Vec::new());
        let mut run = Run::new(program, plan, Vec::new(), reporter, &invocation);
        assert!(run.continuing);
        // The batch holding b is stopped; a depends on b, c does not.
        let of = |run: &Run, stem: &str| run.plan.units.iter().position(|unit| unit.stem == stem).unwrap();
        let (a, b, c) = (of(&run, "a"), of(&run, "b"), of(&run, "c"));
        run.stop(&Batch { units: vec![b], identifier: "b".to_string() });
        assert!(!run.halted, "--continue keeps the independent batches running");
        assert!(run.depends_on_stopped(&Batch { units: vec![a], identifier: "a".to_string() }));
        assert!(!run.depends_on_stopped(&Batch { units: vec![c], identifier: "c".to_string() }));
        // Without --continue a stopped batch stops the compile.
        let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
        let mut run = Run::new(program, plan, Vec::new(), reporter, &invocation);
        run.stop(&Batch { units: vec![b], identifier: "b".to_string() });
        assert!(run.halted);
        assert_eq!(run.incomplete, vec!["b".to_string()]);
    }

    /// The code is success when every batch was accepted, problems when any was rejected,
    /// blocked, or asked a question, and failure when a command could not run.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn the_code_is_the_worst_thing_the_compile_met() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
        let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
        let mut run = Run::new(program, plan, Vec::new(), reporter, &invocation);
        assert_eq!(run.code, ExitCode::Success);
        run.worsen(ExitCode::Problems);
        assert_eq!(run.code, ExitCode::Problems);
        run.worsen(ExitCode::Success);
        assert_eq!(run.code, ExitCode::Problems, "a milder code does not win");
        run.worsen(ExitCode::Failure);
        assert_eq!(run.code, ExitCode::Failure);
    }

    /// With --accept no compiler is run; outputs that carry no marker for the unit are
    /// rejected and the code is problems.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn accept_checks_what_is_already_on_disk() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        // Nothing on disk: the unit is rejected. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Problems.code());
        // An output carrying a marker for the unit is accepted and recorded.
        // @lfy def/cli/main.lfy:main
        fixture.write("out/a.rs", "// @lfy def/a.lfy:1\npub struct A {}\n");
        assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
        // The source maps of its outputs are recorded in the unit's own map file.
        // @lfy def/cli/main.lfy:main
        let maps = fixture.read("elfie-compile/maps/rust/a.json");
        assert!(maps.contains("\"out/a.rs\""), "{maps}");
        assert!(maps.contains("\"def/a.lfy\""), "{maps}");
        // The marker is written back naming the entity rather than the line.
        assert_eq!(fixture.read("out/a.rs"), "// @lfy def/a.lfy:A\npub struct A {}\n");
        // And the unit is then up to date. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        assert!(!fixture.root.join("elfie-requests/a.md").exists(), "nothing is requested");
    }

    /// The compiler is run once per batch with the request on its standard input and
    /// ELFIE_ROOT, ELFIE_BATCH, and ELFIE_UNITS in its environment; its standard output is
    /// kept whole as the report.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn the_compiler_is_run_per_batch_and_its_report_is_read() {
        let fixture = Fixture::new();
        one_target(&fixture)
            .write("def/a.lfy", "d A: `An A` {}\n")
            .write(
                "compiler.sh",
                "#!/bin/sh\ncat > \"$ELFIE_ROOT/instructions.txt\"\nprintf '%s\\n' \"$ELFIE_BATCH\" \"$ELFIE_UNITS\" \"$ELFIE\" > \"$ELFIE_ROOT/environment.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:1\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho done\n",
            )
            .write(
                "elfie.json",
                r#"{
                    "name": "p",
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust", "output": "out" } },
                    "compiler": "sh compiler.sh"
                }"#,
            );
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // ELFIE is the path of the running executable, so the agent server an agent starts
        // is this same program.
        // @lfy def/cli/main.lfy:main#main:main:197d3757fe5da79799c8bd1c6f147465e8333e0f5f9819d87bad4139ec70c195
        let environment = fixture.read("environment.txt");
        let mut lines = environment.lines();
        assert_eq!(lines.next(), Some("a"));
        assert_eq!(lines.next(), Some("a"));
        let elfie = std::env::current_exe().unwrap();
        assert_eq!(lines.next(), Some(elfie.to_string_lossy().as_ref()));
        assert!(fixture.read("instructions.txt").contains("An A"));
        // @lfy def/cli/main.lfy:main
        assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
        // The report is appended to the log with every progress line.
        // @lfy def/cli/main.lfy:main
        let log = fixture.read("elfie-requests/compile.log");
        assert!(log.contains("done"), "{log}");
        assert!(log.contains("▸ compiling"), "{log}");
        // The unit is not repeated after the batch when it is the batch.
        // @lfy def/cli/main.lfy:main
        assert!(log.contains("[1/1]"), "{log}");
        assert!(log.lines().any(|line| line.contains("✓ accepted") && line.matches(" a ").count() == 1), "{log}");
        assert!(log.contains("1 units accepted, 0 rejected, 0 batches blocked, 0 failed"), "{log}");
    }

    /// A compiler that produces nothing is rejected, run once more, and then stops the
    /// compile; a blocked one writes its reason.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_rejected_batch_is_run_once_more_and_a_blocked_one_writes_its_reason() {
        let fixture = Fixture::new();
        one_target(&fixture)
            .write("def/a.lfy", "d A: `An A` {}\n")
            .write("compiler.sh", "#!/bin/sh\ncat > /dev/null\necho 'attempt' >> \"$ELFIE_ROOT/attempts.txt\"\n")
            .write(
                "elfie.json",
                r#"{
                    "name": "p",
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust", "output": "out" } },
                    "compiler": "sh compiler.sh"
                }"#,
            );
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        assert_eq!(fixture.read("attempts.txt").lines().count(), 2, "the batch is run once more");
        // @lfy def/cli/main.lfy:main
        fixture.write("compiler.sh", "#!/bin/sh\ncat > /dev/null\necho 'ELFIE: BLOCKED: def/a.lfy says nothing'\n");
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        let blocked = fixture.read("elfie-requests/a.blocked.md");
        assert!(blocked.contains("def/a.lfy says nothing"), "{blocked}");
        // @lfy def/cli/main.lfy:main
        fixture.write("compiler.sh", "#!/bin/sh\ncat > /dev/null\necho 'ELFIE: CLARIFY: which A?'\n");
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        let question = fixture.read("elfie-requests/a.question.md");
        assert!(question.contains("which A?") && question.contains(ANSWER_HEADING), "{question}");
        // The answer written below the question is appended to the next request.
        fixture.write("elfie-requests/a.question.md", &format!("{question}\nThe only one.\n"));
        fixture.write("compiler.sh", "#!/bin/sh\ncat > \"$ELFIE_ROOT/instructions.txt\"\n");
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        assert!(fixture.read("instructions.txt").contains("The only one."));
    }

    /// A command that cannot be run is a failure, after being run once more.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_command_that_cannot_run_is_a_failure() {
        let fixture = Fixture::new();
        one_target(&fixture)
            .write("def/a.lfy", "d A: `An A` {}\n")
            .write(
                "elfie.json",
                r#"{
                    "name": "p",
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust", "output": "out" } },
                    "compiler": "exit 127"
                }"#,
            );
        // The command runs and produces nothing, so the outcome is rejected rather than
        // failed; a command that cannot be spawned at all is the failure.
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
        let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
        let run = Mutex::new(Run::new(program, plan, Vec::new(), reporter, &invocation));
        let batch = locked(&run).plan.batches[0].clone();
        let nowhere = Where::Root(PathBuf::from("/no/such/directory"));
        assert!(Run::compile_batch(&run, &batch, "true", &nowhere).is_none());
        let state = locked(&run);
        assert_eq!(state.code, ExitCode::Failure);
        assert_eq!(state.failed, 1);
        assert!(state.halted);
    }

    /// A review is printed as its status, a space, the file, a colon, the line, a space, the
    /// entity, a colon, a space, and the note; a violated one becomes one problem reading
    /// failure at, the file, a colon, the line, a colon, the note, and the evidence in
    /// parentheses.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_review_is_printed_and_a_violated_one_becomes_a_problem() {
        let review = Review {
            id: "A:A:0".to_string(),
            file: "def/a.lfy".to_string(),
            line: 3,
            entity: "A".to_string(),
            status: ReviewStatus::Satisfied,
            evidence: "out/a.rs:1-2".to_string(),
            note: "covered by a test".to_string(),
        };
        // The status is padded to the longest status of ReviewStatus, so every entity starts
        // in the same column. @lfy def/cli/main.lfy:main
        let column = width(&ReviewStatus::Unverifiable.to_string());
        assert_eq!(
            painted_review(&review, false),
            format!("{} def/a.lfy:3 A: covered by a test", padded("satisfied", column))
        );
        // The status is painted in its own tone and the file and line are subject, which
        // leaves the width as it was. @lfy def/cli/main.lfy:main
        assert_eq!(width(&painted_review(&review, true)), width(&painted_review(&review, false)));
        assert_eq!(status_tone(review.status), Tone::Success);
        let violated = Review {
            status: ReviewStatus::Violated,
            evidence: "crates/a/src/a.rs:4-6".to_string(),
            note: "returns 0 for an empty list".to_string(),
            ..review.clone()
        };
        // @lfy def/cli/main.lfy:main
        assert_eq!(problem_of(&violated), "failure at def/a.lfy:3: returns 0 for an empty list (crates/a/src/a.rs:4-6)");
        // An unverifiable review is counted and never rejects. @lfy def/cli/main.lfy:main
        let unverifiable = Review { status: ReviewStatus::Unverifiable, ..review.clone() };
        let every = [review, violated, unverifiable];
        let counts = counts_of(&every);
        assert_eq!(counts, (1, 1, 1));
        // @lfy def/cli/main.lfy:main
        assert_eq!(counts_message(&review_counts(&every), false), "1 satisfied, 1 violated, 1 unverifiable");
    }

    /// A violated review rejects the batch: the problem is printed, the batch is run once
    /// more with it appended to the instructions, the verifier runs again, and a violated
    /// review then stops the batch as rejected.
    // @lfy def/cli/main.lfy:main#main:main:09db375382682bd356021bc01b133a6cc8d64d7a8fd0e8abd7deb5253b8d6384
    #[test]
    fn a_violated_review_sends_the_batch_back_and_then_rejects_it() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, VIOLATED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        // The compiler and the verifier each ran twice. @lfy def/cli/main.lfy:main
        assert_eq!(lines_of(&fixture, "attempts.txt"), 2);
        assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
        let log = fixture.read("elfie-requests/compile.log");
        // A reviewed line counting 0 satisfied, 1 violated, 0 unverifiable.
        // @lfy def/cli/main.lfy:main
        assert!(log.contains("✓ reviewed"), "{log}");
        assert!(log.contains("0 satisfied, 1 violated, 0 unverifiable"), "{log}");
        // The problem follows the line it belongs to, indented four spaces and beginning with
        // a dash and a space, then a retrying line. @lfy def/cli/main.lfy:main
        assert!(
            log.contains("    - failure at def/a.lfy:3: returns 0 for an empty list (crates/a/src/a.rs:4-6)"),
            "{log}"
        );
        assert!(log.contains("↻ retrying"), "{log}");
        // The log never holds an escape. @lfy def/cli/main.lfy:main
        assert!(!log.contains('\u{1b}'), "the log holds an escape");
        // The compiler was run a second time with that problem appended to its
        // instructions. @lfy def/cli/main.lfy:main
        assert!(fixture.read("instructions.txt").contains("failure at def/a.lfy:3"));
        // The reviews are written, and the batch counts as a rejection.
        // @lfy def/cli/main.lfy:main
        assert!(fixture.read("elfie-requests/a.reviews.json").contains("violated"));
        assert!(log.contains("0 units accepted, 1 rejected"), "{log}");
        // Its source maps are not recorded, so its outputs stay on disk and the unit stays
        // planned. @lfy def/cli/main.lfy:main
        assert!(fixture.root.join("out/a.rs").exists(), "the outputs stay on disk");
        assert!(!fixture.root.join("elfie-compile/maps/rust/a.json").exists(), "nothing is recorded");
        let (_, plan) = plan_with(&fixture.root, &[], &[], &[]);
        assert_eq!(plan.units.len(), 1);
        assert!(plan.units[0].reason.is_some(), "the unit stays planned");
    }

    /// A verifier whose report does not end is run once more; its problems are then printed
    /// as a failed line and the batch is verified with the reviews parsed from that run as
    /// if its report had been complete.
    // @lfy def/cli/main.lfy:main#main:main:cb2dacfb1c6b799d86c8db6d7609b3be690848043b299f9d073c26a67afc11e0
    // @lfy def/cli/main.lfy:main#main:main:e13334a45d8bb1815a5cb6ea0c40120b7e2ee52e9ccc44d15376b76b3fa724e8
    // @lfy def/cli/main.lfy:main#main:main:2526667ae85d62b2f4e46dcd53587a64569672ad4127a4c8c4cea6ebf71881d8
    #[test]
    fn a_verifier_whose_report_does_not_end_is_run_once_more() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, UNENDED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main#main:main:e13334a45d8bb1815a5cb6ea0c40120b7e2ee52e9ccc44d15376b76b3fa724e8
        assert_eq!(lines_of(&fixture, "verifications.txt"), 2, "the verifier ran twice");
        assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran once");
        let log = fixture.read("elfie-requests/compile.log");
        // Its problems are printed as a failed progress line, and the batch is verified with
        // the reviews parsed from that run as if its report had been complete.
        // @lfy def/cli/main.lfy:main#main:main:2526667ae85d62b2f4e46dcd53587a64569672ad4127a4c8c4cea6ebf71881d8
        assert!(log.contains("✗ failed"), "{log}");
        assert!(log.contains("1 satisfied, 0 violated, 0 unverifiable"), "{log}");
        // The reviews of that run are written all the same. @lfy def/cli/main.lfy:main
        assert!(fixture.read("elfie-requests/a.reviews.json").contains("covered by a test"));
    }

    /// A run that ended with the end line and at least one review stands, whatever else it
    /// wrote: the verifier is not run again, and what it wrote beside its reviews is printed
    /// under its reviewed line.
    // @lfy def/cli/main.lfy:main#main:main:7b33e4abb60c999fd74ba8a003a59f6de17412062b9767f21eda5ffd7eaf963c
    #[test]
    fn a_review_that_ended_stands_and_what_else_it_wrote_is_printed() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED_WITH_A_STRAY_OBJECT);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main#main:main:7b33e4abb60c999fd74ba8a003a59f6de17412062b9767f21eda5ffd7eaf963c
        assert_eq!(lines_of(&fixture, "verifications.txt"), 1, "the verifier ran once");
        let log = fixture.read("elfie-requests/compile.log");
        assert!(log.contains("1 satisfied, 0 violated, 0 unverifiable"), "{log}");
        // What it wrote beside its reviews follows its reviewed line, indented.
        // @lfy def/cli/main.lfy:main#main:main:7b33e4abb60c999fd74ba8a003a59f6de17412062b9767f21eda5ffd7eaf963c
        assert!(log.contains("I also looked at the tests"), "{log}");
        assert!(!log.contains("✗ failed"), "{log}");
    }

    /// Every line of the verifier's output reaches the log, the lines it wrote to standard
    /// error among them, each with the prefix it was shown with and no escape.
    // @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
    #[test]
    fn every_line_the_verifier_wrote_reaches_the_log() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED_WITH_NOISE);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        let log = fixture.read("elfie-requests/compile.log");
        // The line it wrote to standard error, prefixed by the batch as it was shown.
        // @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
        assert!(log.contains("  a \u{2502} reading the outputs"), "{log}");
        // Its standard output, and the report it is read from, are there too.
        // @lfy def/cli/main.lfy:main#main:main:f26bf3b8653ec38619ec8021b98b5bf3f3f9251405593fadddafbf7e2c75caf3
        assert!(log.contains("  a \u{2502} ELFIE: REVIEWED"), "{log}");
        assert!(log.contains("--- the review of a (attempt 1) ---"), "{log}");
        // What the compiler wrote is logged the same way.
        // @lfy def/cli/main.lfy:main#main:main:fd2e103cd45ada13659acc347b24014460bea62429f97ff35843a450d9ebde3b
        assert!(log.contains("  a \u{2502} ELFIE: DONE"), "{log}");
        // Every line of it is the line as printed with colors off.
        // @lfy def/cli/main.lfy:main#main:main:6d1962bd388ee5c3b97353bbd3d76bfb559b2e1a57b72b2916622465fb1de177
        assert!(!log.contains('\u{1b}'), "the log holds an escape");
    }

    /// With --no-verify no verifier runs, nothing is reviewed, no reviews file is written,
    /// and the batch is done and its source maps recorded when its units are accepted.
    // @lfy def/cli/main.lfy:main#main:main:c39eb2ac088e0940ca15c25cd3ff8a3faef375a1be5b7e99e7a6c7a5f078b66e
    #[test]
    fn no_verify_runs_no_verifier() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, VIOLATED);
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran once");
        assert!(!fixture.root.join("verifications.txt").exists(), "no verifier ran");
        assert!(!fixture.root.join("elfie-requests/a.reviews.json").exists(), "nothing is reviewed");
        // The source maps are recorded in the unit's own map file all the same.
        // @lfy def/cli/main.lfy:main
        assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
        // A project naming no verifier is the same. @lfy def/cli/main.lfy:main
        let plain = Fixture::new();
        a_compiler_and_a_verifier(&plain, VIOLATED).write(
            "elfie.json",
            r#"{
                "name": "p",
                "dependencies": { "rust": { "root": "targets/rust" } },
                "targets": { "rust": { "package": "rust", "marker": "rust", "output": "out" } },
                "compiler": "sh compiler.sh"
            }"#,
        );
        assert_eq!(plain.run(&["compile"]), ExitCode::Success.code());
        assert!(!plain.root.join("verifications.txt").exists());
        assert!(!plain.root.join("elfie-requests/a.reviews.json").exists());
    }

    /// Verify runs the verifier on the outputs already recorded and compiles nothing.
    // @lfy def/cli/main.lfy:main#main:main:15d1c2f8c98a4118bbe33674e45a72fe0fd1f66e993c301722513fea2e9f7482
    #[test]
    fn verify_reviews_what_is_recorded_without_compiling() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        assert_eq!(lines_of(&fixture, "attempts.txt"), 1);
        // No review was violated, so the held source maps were recorded and the batch was
        // done. @lfy def/cli/main.lfy:main
        assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
        let _ = fs::remove_file(fixture.root.join("elfie-requests/a.reviews.json"));
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Success.code());
        assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "no compiler is run");
        let reviews = fixture.read("elfie-requests/a.reviews.json");
        assert!(reviews.contains("covered by a test"), "{reviews}");
        // A violated review makes the code problems. @lfy def/cli/main.lfy:main
        write_verifier(&fixture, VIOLATED);
        assert_eq!(fixture.run(&["verify"]), ExitCode::Problems.code());
        // A stem naming a unit whose outputs are empty is left out, and the code does not
        // change for it. @lfy def/cli/main.lfy:main
        fixture.write("def/b.lfy", "d B {}\n");
        write_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["verify", "b"]), ExitCode::Success.code());
        assert!(!fixture.root.join("elfie-requests/b.reviews.json").exists(), "nothing was reviewed for b");
    }

    /// Verify on a unit already recorded, whose review is violated, prints that review and
    /// the counts, writes the reviews, runs no compiler, and returns problems.
    // @lfy def/cli/main.lfy:main#main:main:27855aa05b4fe32ee428e7ec864838ffc63fe7de8951112ca50eb119a8345666
    #[test]
    fn verify_returns_problems_for_a_violated_review() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // The output for a is recorded in elfie-compile/maps/rust/a.json.
        // @lfy def/cli/main.lfy:main
        assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
        let _ = fs::remove_file(fixture.root.join("elfie-requests/a.reviews.json"));
        write_verifier(&fixture, VIOLATED_IN_SRC);
        // The code is problems when any review is violated. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Problems.code());
        assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "no compiler is run");
        // elfie-requests/a.reviews.json holds the review. @lfy def/cli/main.lfy:main
        let reviews = fixture.read("elfie-requests/a.reviews.json");
        assert!(reviews.contains("returns 0 for an empty list"), "{reviews}");
        assert!(reviews.contains("src/a.rs:4-6"), "{reviews}");
        // It is printed as violated, the file, a colon, the line, the entity, a colon, and
        // the note, then the counts. @lfy def/cli/main.lfy:main
        let (program, _) = plan_with(&fixture.root, &[], &[], &[]);
        let line = format!(
            "{{\"id\":\"{}\",\"status\":\"violated\",\"evidence\":\"src/a.rs:4-6\",\"note\":\"returns 0 for an empty list\"}}\nELFIE: REVIEWED\n",
            criterion_id(&fixture)
        );
        let report = generation::review_of(&line, &program);
        let column = width(&ReviewStatus::Unverifiable.to_string());
        assert_eq!(
            painted_review(&report.reviews[0], false),
            format!("{} def/a.lfy:3 A: returns 0 for an empty list", padded("violated", column))
        );
        // @lfy def/cli/main.lfy:main
        assert_eq!(counts_message(&review_counts(&report.reviews), false), "0 satisfied, 1 violated, 0 unverifiable");
    }

    /// With no verifier named, the review request of each batch is written to
    /// elfie-requests named by the batch identifier with the extension .review.md, and the
    /// code is success.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn verify_with_no_verifier_writes_the_review_requests() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        fixture.write(
            "elfie.json",
            r#"{
                "name": "p",
                "dependencies": { "rust": { "root": "targets/rust" } },
                "targets": { "rust": { "package": "rust", "marker": "rust", "output": "out" } },
                "compiler": "sh compiler.sh"
            }"#,
        );
        assert_eq!(fixture.run(&["verify"]), ExitCode::Success.code());
        let request = fixture.read("elfie-requests/a.review.md");
        assert!(request.contains("def/a.lfy"), "{request}");
        assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "nothing is compiled");
    }

    /// The id of the one global criterion of the project.
    fn global_id(fixture: &Fixture) -> String {
        let (program, _) = plan_with(&fixture.root, &[], &[], &[]);
        program.criteria.first().map(|criterion| criterion.id.clone()).unwrap_or_else(|| panic!("no global criterion"))
    }

    /// The same project with one global criterion, so a global review has something to review,
    /// and a compiler whose output answers for it. The verifier's `<global>` is the id of that
    /// criterion and its `<id>` the id of the criterion of A.
    // @lfy def/cli/main.lfy:main
    fn with_a_global_criterion<'f>(fixture: &'f Fixture, verifier: &str) -> &'f Fixture {
        a_compiler_and_a_verifier(fixture, SATISFIED);
        fixture.write(
            "def/a.lfy",
            "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\nglobal@acceptanceCriteria.add({ behavior = `Every output builds` });\n",
        );
        let global = global_id(fixture);
        // The marker answers for the global criterion, so the unit is one the global review
        // can send back. @lfy def/cli/main.lfy:main
        fixture.write(
            "compiler.sh",
            &format!(
                "#!/bin/sh\ncat >> \"$ELFIE_ROOT/instructions.txt\"\necho compile >> \"$ELFIE_ROOT/attempts.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:A#{global}\\npub struct A {{}}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho 'ELFIE: DONE'\n"
            ),
        );
        let script = verifier.replace("<id>", &criterion_id(fixture)).replace("<global>", &global);
        fixture.write("verifier.sh", &script)
    }

    /// A verifier that answers for the global criterion when it is run for the global review
    /// and for the criterion of A otherwise, with the status each is given.
    fn branching_verifier(global: &str, local: &str) -> String {
        format!(
            "#!/bin/sh\ncat > /dev/null\necho \"$ELFIE_BATCH\" >> \"$ELFIE_ROOT/verifications.txt\"\necho \"$ELFIE_BATCH=$ELFIE_UNITS\" >> \"$ELFIE_ROOT/units.txt\"\nif [ \"$ELFIE_BATCH\" = global ]; then\n  echo '{{\"id\":\"<global>\",\"status\":\"{global}\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"every output builds\"}}'\nelse\n  echo '{{\"id\":\"<id>\",\"status\":\"{local}\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}}'\nfi\necho 'ELFIE: REVIEWED'\n"
        )
    }

    /// Every global criterion is reviewed once for the whole program after every batch has
    /// been handled, and the review is written with the hash of the ids it answered for.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_global_criterion_is_reviewed_once_after_every_batch() {
        let fixture = Fixture::new();
        with_a_global_criterion(&fixture, &branching_verifier("satisfied", "satisfied"));
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // The verifier ran once for the batch and once for the global review, with
        // ELFIE_BATCH set to global for the second. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.read("verifications.txt"), "a\nglobal\n");
        // ELFIE_UNITS names every unit whose source maps answer for a global id, the unit
        // this compile just generated among them: it has no outputs from an earlier
        // generation, so only the maps this compile accepted name it.
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.read("units.txt"), "a=a\nglobal=a\n");
        let log = fixture.read("elfie-requests/compile.log");
        // @lfy def/cli/main.lfy:main
        assert!(log.contains("▸ globalVerifying"), "{log}");
        assert!(log.contains("✓ globalReviewed"), "{log}");
        // elfie-requests/global.reviews.json holds the hash of the global ids reviewed and
        // the report. @lfy def/cli/main.lfy:main
        let written = fixture.read("elfie-requests/global.reviews.json");
        let value: serde_json::Value = serde_json::from_str(&written).unwrap();
        let (program, _) = plan_with(&fixture.root, &[], &[], &[]);
        assert_eq!(value["requirements"], global_requirements(&program));
        assert_eq!(value["reviews"][0]["id"], global_id(&fixture));
        assert_eq!(value["reviews"][0]["status"], "satisfied");
        // Nothing was violated, so nothing is planned with reason violated.
        // @lfy def/cli/main.lfy:main
        assert!(violated_ids(&fixture.root).is_empty());
        // The second compile has nothing to accept and the ids it answered for did not
        // change, so no global review runs again. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        assert_eq!(fixture.read("verifications.txt"), "a\nglobal\n");
    }

    /// With no global criterion and no global test, no global review runs and no file is
    /// written, by a compile or by a verify.
    // @lfy def/cli/main.lfy:main#main:main:be3d004d2bbdb0016741f66318c37fc38045672858eb8ac8e802ac6fee6024f0
    #[test]
    fn no_global_criterion_means_no_global_review() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main#main:main:be3d004d2bbdb0016741f66318c37fc38045672858eb8ac8e802ac6fee6024f0
        assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
        let log = fixture.read("elfie-requests/compile.log");
        assert!(!log.contains("globalVerifying"), "{log}");
        // The verifier ran once, for the batch, and never for a global review.
        assert_eq!(lines_of(&fixture, "verifications.txt"), 1);
        // verify with no stem reviews the batch and asks for no global review either, because
        // there is nothing global to review.
        // @lfy def/cli/main.lfy:main#main:main:be3d004d2bbdb0016741f66318c37fc38045672858eb8ac8e802ac6fee6024f0
        assert_eq!(fixture.run(&["verify"]), ExitCode::Success.code());
        assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
        assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
        // verify --global with nothing global to review runs no verifier at all.
        // @lfy def/cli/main.lfy:main#main:main:be3d004d2bbdb0016741f66318c37fc38045672858eb8ac8e802ac6fee6024f0
        assert_eq!(fixture.run(&["verify", "--global"]), ExitCode::Success.code());
        assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
        assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
    }

    /// A violated global review plans the units whose markers answered for it with reason
    /// violated and compiles them once more in the same compile, and the global review runs
    /// once more after them; a second violated review stops the compile with the code
    /// problems, and the next compile plans those units with reason violated.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_violated_global_review_plans_its_units_once_more() {
        let fixture = Fixture::new();
        with_a_global_criterion(&fixture, &branching_verifier("violated", "satisfied"));
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        // The batch was handled twice and the global review ran after each round.
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.read("verifications.txt"), "a\nglobal\na\nglobal\n");
        assert_eq!(lines_of(&fixture, "attempts.txt"), 2, "the unit was compiled once more");
        let log = fixture.read("elfie-requests/compile.log");
        // One problem per violated review, as failure at, a space, global, a colon, a space,
        // the note, and the evidence in parentheses. @lfy def/cli/main.lfy:main
        assert!(log.contains("    - failure at global: every output builds (out/a.rs:1-2)"), "{log}");
        // The next compile plans those units with reason violated. @lfy def/cli/main.lfy:main
        let ids = violated_ids(&fixture.root);
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&global_id(&fixture)));
        let (_, plan) = planned(&fixture.root, &generation::source_maps_of(&workspace::load(&fixture.root), None), &[], false, &ids);
        let unit = plan.units.iter().find(|unit| unit.stem == "a").unwrap();
        // @lfy def/cli/main.lfy:main
        assert_eq!(unit.reason, Some(Reason::Violated));
    }

    /// A target whose maps were kept in source-map.json has each unit's recorded in its map
    /// file before any batch runs, and the file is removed once every one is recorded.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn the_maps_of_an_older_project_are_moved_into_the_map_files() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        let recorded = fixture.read("elfie-compile/maps/rust/a.json");
        // The project as an earlier version left it: one source-map.json under the output
        // directory and no folder of map files.
        fixture.write("out/source-map.json", &recorded);
        fs::remove_dir_all(fixture.root.join("elfie-compile/maps")).unwrap();
        // The unit is up to date all the same, since the maps are still read.
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--dry-run"]), ExitCode::Success.code());
        assert!(fixture.root.join("out/source-map.json").exists(), "a dry run moves nothing");
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        assert_eq!(fixture.read("elfie-compile/maps/rust/a.json"), recorded);
        // @lfy def/cli/main.lfy:main
        assert!(!fixture.root.join("out/source-map.json").exists(), "the file is removed");
    }

    /// A map file belonging to no unit of the plan is removed before any batch runs.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_map_file_of_no_unit_is_removed() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        // A unit whose definition file was deleted leaves its map file behind.
        let stale = fixture.read("elfie-compile/maps/rust/a.json");
        fixture.write("elfie-compile/maps/rust/gone.json", &stale);
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        assert!(!fixture.root.join("elfie-compile/maps/rust/gone.json").exists(), "the map file stayed");
        // The map file of a unit of the plan is left as it was, byte for byte.
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.read("elfie-compile/maps/rust/a.json"), stale);
    }

    /// A compiler that writes one output per unit of the batch, so either unit of a project of
    /// two is accepted, and names each unit it was run for in `attempts.txt`.
    // @lfy def/cli/main.lfy:main
    const PER_UNIT_COMPILER: &str = "#!/bin/sh\ncat >> \"$ELFIE_ROOT/instructions.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nfor stem in $ELFIE_UNITS; do\n  echo \"$stem\" >> \"$ELFIE_ROOT/attempts.txt\"\n  name=$(echo \"$stem\" | tr 'a-z' 'A-Z')\n  printf '// @lfy def/%s.lfy:%s\\npub struct %s {}\\n' \"$stem\" \"$name\" \"$name\" > \"$ELFIE_ROOT/out/$stem.rs\"\ndone\necho 'ELFIE: DONE'\n";

    /// Two units both recorded, one of them edited since: the edited one alone is compiled, its
    /// map file is rewritten, and the other's is left byte for byte as it was.
    // @lfy def/cli/main.lfy:main#main:main:a64dbb7ffadb4cffc5916067dd80d88bf16cb8db4d6aadf2d4406ffe3d0553e4
    #[test]
    fn a_compile_of_one_unit_leaves_every_other_map_file_alone() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        fixture.write("def/b.lfy", "d B {}\n");
        fixture.write("compiler.sh", PER_UNIT_COMPILER);
        // Both units recorded in elfie-compile/maps/rust. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        let (was_a, was_b) =
            (fixture.read("elfie-compile/maps/rust/a.json"), fixture.read("elfie-compile/maps/rust/b.json"));
        // a.lfy edited since, so a alone is planned. @lfy def/cli/main.lfy:main
        fixture.write("def/a.lfy", "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list once` });\n}\n");
        write_verifier(&fixture, SATISFIED);
        fixture.write("attempts.txt", "");
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // Only a's batch was run. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.read("attempts.txt"), "a\n");
        // @lfy def/cli/main.lfy:main
        assert_ne!(fixture.read("elfie-compile/maps/rust/a.json"), was_a, "a.json was not rewritten");
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.read("elfie-compile/maps/rust/b.json"), was_b, "b.json changed");
    }

    /// Every map recorded and elfie-compile/cache deleted: every unit is still up to date, since
    /// nothing under the cache is ever the only record of anything.
    // @lfy def/cli/main.lfy:main#main:main:340adcae1ad95cbe8bc4b606664bc8d048b41ab12e13403411e12ae1c743c7c8
    // @lfy def/cli/main.lfy:main#main:main:771e545a60806e91b890eb4af7eb959bf8e8df680ca9fcf262ca16ac3546d3c6
    #[test]
    fn a_deleted_cache_gives_the_same_plan() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main
        let _ = fs::remove_dir_all(fixture.root.join("elfie-compile/cache"));
        let maps = generation::source_maps_of(&workspace::load(&fixture.root), None);
        let (program, plan) = plan_with(&fixture.root, &maps, &[], &[]);
        assert_eq!(plan.units.len(), 1);
        // @lfy def/cli/main.lfy:main
        assert_eq!(unit_line(&program.workspace, &plan, 0, false), "rust a up to date []");
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--dry-run"]), ExitCode::Success.code());
    }

    /// `Request.previous` holds the lowered text of the source the unit's outputs were
    /// generated from, recovered from git, never the raw definition file.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn the_previous_source_is_the_lowered_text_git_still_holds() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        // The unit is compiled and recorded, so its map holds the hash of the lowered text of
        // def/a.lfy as it is now. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        if !git(&fixture, &["init", "-q"]) {
            return; // No git here: nothing is recoverable, and nothing to check.
        }
        git(&fixture, &["config", "user.email", "p@example.com"]);
        git(&fixture, &["config", "user.name", "p"]);
        git(&fixture, &["add", "def/a.lfy"]);
        git(&fixture, &["commit", "-q", "-m", "a"]);
        // The lowered text of the committed revision, as generation reads it.
        // @lfy def/cli/main.lfy:main
        let workspace = workspace::load(&fixture.root);
        let raw = fixture.read("def/a.lfy");
        let lowered = lowered_text(&workspace, "def/a.lfy", &raw).unwrap();
        // Lowering drops the criteria, so the lowered text is not the raw source and a test
        // cannot pass with either one in place of the other. @lfy def/cli/main.lfy:main
        assert!(raw.contains("@acceptanceCriteria"), "{raw}");
        assert!(!lowered.contains("@acceptanceCriteria"), "{lowered}");
        // The revision is found by the hash of its lowered text, which is what the map holds.
        // @lfy def/cli/main.lfy:main
        let maps = generation::source_maps_of(&workspace, None);
        drop(workspace);
        let (program, plan) = planned(&fixture.root, &maps, &["a".to_string()], false, &BTreeSet::new());
        let unit = plan.units.iter().find(|unit| unit.stem == "a").unwrap();
        assert_eq!(generation::source_hash(&lowered), unit.outputs[0].hash);
        // @lfy def/cli/main.lfy:main
        assert_eq!(previous_source(&program.workspace, unit).as_deref(), Some(lowered.as_str()));
        // Which is what the request hands the compiler. @lfy def/cli/main.lfy:main
        let request = generation::request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new());
        assert!(request.sources["def/a.lfy"].contains("d A"), "{:?}", request.sources);
        assert_eq!(lowered, request.sources["def/a.lfy"]);
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

    /// A compile with nothing to do still begins with planned and ends with finished, even
    /// when a global review runs between them because no global review was ever recorded.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_compile_with_nothing_to_do_still_begins_planned_and_ends_finished() {
        let fixture = Fixture::new();
        with_a_global_criterion(&fixture, &branching_verifier("satisfied", "satisfied"));
        // The unit is compiled and recorded with no verifier, so nothing is planned next time
        // and no global review was ever recorded. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        fs::remove_file(fixture.root.join("elfie-requests/compile.log")).unwrap();
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        let log = fixture.read("elfie-requests/compile.log");
        // Nothing was planned, so the counter keeps the width of no units at all.
        // @lfy def/cli/main.lfy:main
        assert!(log.contains("[0/0]"), "{log}");
        // The global review still ran, and the first progress line is planned all the same.
        // @lfy def/cli/main.lfy:main
        assert!(log.contains("▸ globalVerifying"), "{log}");
        let mut lines = log.lines().filter(|line| !line.trim().is_empty());
        // @lfy def/cli/main.lfy:main
        assert!(lines.next().is_some_and(|first| first.contains("◆ planned")), "{log}");
        // @lfy def/cli/main.lfy:main
        assert!(lines.next_back().is_some_and(|last| last.contains("◆ finished")), "{log}");
    }

    /// Verify with --global and no stem reviews no batch, runs the verifier once for the
    /// global criteria and tests, and writes the global review.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn verify_global_reviews_no_batch() {
        let fixture = Fixture::new();
        with_a_global_criterion(&fixture, &branching_verifier("satisfied", "satisfied"));
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        assert!(!fixture.root.join("verifications.txt").exists(), "no verifier ran");
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["verify", "--global"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.read("verifications.txt"), "global\n");
        assert!(!fixture.root.join("elfie-requests/a.reviews.json").exists(), "a batch was reviewed");
        // @lfy def/cli/main.lfy:main
        let written = fixture.read("elfie-requests/global.reviews.json");
        assert!(written.contains(&global_id(&fixture)), "{written}");
        // Nothing under elfie-compile is recorded, moved, or removed by verify.
        // @lfy def/cli/main.lfy:main
        let before = fixture.read("elfie-compile/maps/rust/a.json");
        assert_eq!(fixture.run(&["verify"]), ExitCode::Success.code());
        assert_eq!(fixture.read("elfie-compile/maps/rust/a.json"), before);
        // With no stem and no --global every batch is reviewed and the global review follows.
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.read("verifications.txt"), "global\na\nglobal\n");
        // A violated review makes the code problems. @lfy def/cli/main.lfy:main
        let script = branching_verifier("violated", "satisfied")
            .replace("<id>", &criterion_id(&fixture))
            .replace("<global>", &global_id(&fixture));
        fixture.write("verifier.sh", &script);
        assert_eq!(fixture.run(&["verify", "--global"]), ExitCode::Problems.code());
    }

    // ---- running batches together --------------------------------------------------

    /// --jobs takes the value that follows it or the one after its equals sign, and anything
    /// but a whole number of at least 1 is a usage message naming it.
    // @lfy def/cli/main.lfy:parse#parse:parse:8a7c0ab826525b9fea70ea111a51d4e84e153e8e61475a1c4494148b8243fc71
    // @lfy def/cli/main.lfy:parse#parse:parse:31d286a6e27d0b3b1aee3510e0c91b34c50892ae2bb9d66fe5da061ccb6d6d01
    // @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
    #[test]
    fn jobs_takes_a_whole_number_of_at_least_one() {
        // --jobs followed by a value gives that value, as --target does.
        // @lfy def/cli/main.lfy:parse#parse:parse:31d286a6e27d0b3b1aee3510e0c91b34c50892ae2bb9d66fe5da061ccb6d6d01
        let invocation = parse_arguments(&args(&["compile", "--jobs", "2", "lexer/main"])).unwrap();
        assert_eq!(invocation.option("jobs"), Some("2"));
        assert_eq!(jobs_of(&invocation), 2);
        // The value it took is no positional argument.
        // @lfy def/cli/main.lfy:parse#parse:parse:7473c896dd710a7b0dcccc8450958126fa8e9c70c04172f551e9a551ff75afa3
        assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
        assert_eq!(jobs_of(&parse_arguments(&args(&["compile", "--jobs=3"])).unwrap()), 3);
        // 0 is no whole number of at least 1, and neither is anything that is no number.
        // @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
        // @lfy def/cli/main.lfy:parse#parse:parse:8a7c0ab826525b9fea70ea111a51d4e84e153e8e61475a1c4494148b8243fc71
        let error = parse_arguments(&args(&["compile", "--jobs", "0"])).unwrap_err();
        assert_eq!(error, JOBS_USAGE);
        assert!(error.contains("--jobs"), "{error}");
        for arguments in [
            args(&["compile", "--jobs=0"]),
            args(&["compile", "--jobs=some"]),
            args(&["compile", "--jobs", "-1"]),
            args(&["compile", "--jobs", "1.5"]),
        ] {
            assert_eq!(parse_arguments(&arguments).unwrap_err(), JOBS_USAGE, "{arguments:?}");
        }
    }

    /// With no --jobs given, four batches run at once.
    // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
    #[test]
    fn without_jobs_four_batches_run_at_once() {
        // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
        assert_eq!(JOBS_BY_DEFAULT, 4);
        assert_eq!(jobs_of(&parse_arguments(&args(&["compile"])).unwrap()), 4);
    }

    /// One batch of a round, named and waiting for the places given.
    // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
    fn a_planned_batch(identifier: &str, unit: usize, after: Vec<usize>) -> Planned {
        Planned { batch: Batch { units: vec![unit], identifier: identifier.to_string() }, after }
    }

    /// A batch starts once every batch holding a dependency of one of its units is done, at
    /// most --jobs run at once, and among those that can start they start in plan order.
    // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
    #[test]
    fn a_batch_starts_once_what_it_waits_for_is_done() {
        let planned =
            vec![a_planned_batch("a", 0, vec![]), a_planned_batch("b", 1, vec![]), a_planned_batch("c", 2, vec![0])];
        let mut schedule = Schedule::of(&planned);
        // Among those that can start, they start in plan order.
        // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
        assert!(matches!(schedule.next(&planned, 2, false), Next::Start(0)));
        assert!(matches!(schedule.next(&planned, 2, false), Next::Start(1)));
        // At most --jobs run at once.
        // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
        assert!(matches!(schedule.next(&planned, 2, false), Next::Wait));
        // c waits on a, which is still running.
        // @lfy def/cli/main.lfy:main#main:main:6ce3a95e70162dcd0e108bfe5ed94dfd56b3736d592fb8b222a5094127b5f845
        schedule.finish(1, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, false), Next::Wait));
        schedule.finish(0, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, false), Next::Start(2)));
        schedule.finish(2, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, false), Next::Over));
    }

    /// The compile stopped: no batch starts after that, and the batches already running
    /// finish as they would.
    // @lfy def/cli/main.lfy:main#main:main:3c23880977d6965ce85ac281ad2e12af55f114b91a87851d4a296f402ee2550b
    #[test]
    fn a_stopped_compile_starts_no_batch() {
        let planned = vec![a_planned_batch("a", 0, vec![]), a_planned_batch("b", 1, vec![])];
        let mut schedule = Schedule::of(&planned);
        assert!(matches!(schedule.next(&planned, 2, false), Next::Start(0)));
        // @lfy def/cli/main.lfy:main#main:main:3c23880977d6965ce85ac281ad2e12af55f114b91a87851d4a296f402ee2550b
        assert!(matches!(schedule.next(&planned, 2, true), Next::Wait));
        schedule.finish(0, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, true), Next::Over));
        // @lfy def/cli/main.lfy:main#main:main:974ee309cbfc12d85b60f2bbb0b8cd1361aa959c94969a5cf6c482e0ec137ba7
        assert_eq!(schedule.unstarted(), vec![1]);
    }

    /// A batch whose file could not be merged runs again from a new copy once no other batch
    /// is running, and nothing else starts while it does.
    // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
    #[test]
    fn a_conflicted_batch_runs_again_with_nothing_else_running() {
        let planned = vec![a_planned_batch("a", 0, vec![]), a_planned_batch("b", 1, vec![])];
        let mut schedule = Schedule::of(&planned);
        assert!(matches!(schedule.next(&planned, 2, false), Next::Start(0)));
        // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
        schedule.finish(0, &Done::Conflicted);
        // It is waiting again, in plan order, and runs with nothing else.
        // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
        assert!(matches!(schedule.next(&planned, 2, false), Next::Start(0)));
        assert!(matches!(schedule.next(&planned, 2, false), Next::Wait));
        schedule.finish(0, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, false), Next::Start(1)));
    }

    /// With --jobs 1, or outside a git repository, batches run one at a time and the batch's
    /// directory is the root itself.
    // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
    #[test]
    fn one_job_or_no_repository_runs_in_the_root() {
        let fixture = Fixture::new();
        let four = parse_arguments(&args(&["compile", "--jobs", "4"])).unwrap();
        // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
        assert_eq!(Where::of(&fixture.root, "a", 1).directory(), fixture.root);
        // Outside a git repository --jobs does not hold: one batch runs at a time, in the
        // root itself, so that no two compilers share a tree.
        // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
        if git_directory(&fixture.root).is_none() {
            // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
            assert_eq!(compile_jobs(&four, &fixture.root), 1);
            // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
            assert_eq!(Where::of(&fixture.root, "a", 4).directory(), fixture.root);
            // One at a time: with one running, nothing else starts.
            // @lfy def/cli/main.lfy:main#main:main:499ee7a3765c8bd298923979a12ca327e03f51d363ef956cfd843593ac4f1130
            let planned =
                vec![a_planned_batch("a", 0, Vec::new()), a_planned_batch("b", 1, Vec::new())];
            let mut schedule = Schedule::of(&planned);
            assert!(matches!(schedule.next(&planned, 1, false), Next::Start(0)));
            assert!(matches!(schedule.next(&planned, 1, false), Next::Wait));
        }
        // In a git repository a batch has its own copy, so --jobs stands as it is given.
        // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
        if a_repository(&fixture) {
            // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
            assert_eq!(compile_jobs(&four, &fixture.root), 4);
        }
    }

    /// A fixture root made a git repository, so that a batch gets its own copy of it; false
    /// when there is no git to make one with.
    // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
    fn a_repository(fixture: &Fixture) -> bool {
        git(fixture, &["init", "-q"])
            && git(fixture, &["config", "user.email", "p@example.com"])
            && git(fixture, &["config", "user.name", "p"])
    }

    /// Every file git lists in the root is copied byte for byte, and every file of the copy
    /// that differs from the base is merged back: replaced where the root still holds what
    /// the base holds, and three-way merged where another batch changed it.
    // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
    // @lfy def/cli/main.lfy:main#main:main:17684978e617aff4e72859ceb7f5ff7d85e0c699112fcb5e5aa6d8fb18f2a390
    // @lfy def/cli/main.lfy:main#main:main:909b9177e97f545e3adf99e7e4b6c8bbf1cadc9f0982c0a89695dc00cc474e8d
    #[test]
    fn a_copy_of_the_root_merges_back_into_it() {
        let fixture = Fixture::new();
        fixture
            .write("kept.txt", "one\n")
            .write("gone.txt", "away\n")
            .write("shared.txt", "a\nb\nc\n")
            .write("elfie-requests/compile.log", "noise\n");
        if !a_repository(&fixture) {
            return;
        }
        // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
        let mirror = Mirror::take(&fixture.root, "x/y").expect("a copy of the root");
        // Every file git lists is there byte for byte, under a name a file may have.
        // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
        assert_eq!(fs::read_to_string(mirror.directory.join("kept.txt")).unwrap(), "one\n");
        assert!(mirror.directory.ends_with("x-y"), "{:?}", mirror.directory);
        // Nothing under elfie-requests is copied.
        // @lfy def/cli/main.lfy:main#main:main:7663634e78e29f09bc9bfa3ec791667abf7a230354c0f31313bbe8cc7b4ecb9b
        assert!(!mirror.directory.join("elfie-requests/compile.log").exists());
        // The batch changes one file, adds one, and removes one.
        fs::write(mirror.directory.join("kept.txt"), "two\n").unwrap();
        fs::write(mirror.directory.join("added.txt"), "new\n").unwrap();
        fs::write(mirror.directory.join("shared.txt"), "a\nb\nC\n").unwrap();
        fs::remove_file(mirror.directory.join("gone.txt")).unwrap();
        // Another batch changed shared.txt in the root meanwhile, elsewhere in the file.
        fixture.write("shared.txt", "A\nb\nc\n");
        let merged = mirror.merge(&fixture.root).expect("every file merges");
        // The root still held what the base holds: it is replaced by the copy's, or removed.
        // @lfy def/cli/main.lfy:main#main:main:17684978e617aff4e72859ceb7f5ff7d85e0c699112fcb5e5aa6d8fb18f2a390
        assert_eq!(fixture.read("kept.txt"), "two\n");
        assert_eq!(fixture.read("added.txt"), "new\n");
        assert!(!fixture.root.join("gone.txt").exists());
        // Another batch changed it: it is the three-way merge against the base.
        // @lfy def/cli/main.lfy:main#main:main:909b9177e97f545e3adf99e7e4b6c8bbf1cadc9f0982c0a89695dc00cc474e8d
        assert_eq!(fixture.read("shared.txt"), "A\nb\nC\n");
        for path in ["added.txt", "gone.txt", "kept.txt", "shared.txt"] {
            assert!(merged.contains(&path.to_string()), "{path} is not in {merged:?}");
        }
        // The copy goes once its work has reached the root.
        // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
        mirror.remove();
        assert!(!mirror.directory.exists());
    }

    /// A file that cannot be merged without a conflict merges nothing of the batch.
    // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
    #[test]
    fn a_file_that_cannot_be_merged_merges_nothing_of_the_batch() {
        let fixture = Fixture::new();
        fixture.write("shared.txt", "a\n").write("other.txt", "one\n");
        if !a_repository(&fixture) {
            return;
        }
        let mirror = Mirror::take(&fixture.root, "x").expect("a copy of the root");
        fs::write(mirror.directory.join("shared.txt"), "mine\n").unwrap();
        fs::write(mirror.directory.join("other.txt"), "two\n").unwrap();
        // Another batch changed the same line of the same file.
        fixture.write("shared.txt", "theirs\n");
        // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
        assert_eq!(mirror.merge(&fixture.root).unwrap_err(), "shared.txt");
        // Nothing of the batch is merged, the file that did merge cleanly included.
        // @lfy def/cli/main.lfy:main#main:main:2bc1b8e178c29a51b68a1d7757ec51effe3f4ee46c1f06a71ffe285c65d04366
        assert_eq!(fixture.read("other.txt"), "one\n");
        assert_eq!(fixture.read("shared.txt"), "theirs\n");
    }

    /// A batch running in a copy of the root runs the compiler and the verifier there, with
    /// git reading the root's history, and its work reaches the root once it is done.
    // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
    // @lfy def/cli/main.lfy:main#main:main:256d3040e99b1dbe822f8eb7b21d3d34f15faf77af0dc09bc5f7724644b6453b
    #[test]
    fn a_batch_running_in_a_copy_merges_its_outputs_into_the_root() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        // The compiler writes its output and what git in the copy was pointed at.
        // @lfy def/cli/main.lfy:main#main:main:256d3040e99b1dbe822f8eb7b21d3d34f15faf77af0dc09bc5f7724644b6453b
        fixture.write(
            "compiler.sh",
            "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' \"$GIT_DIR\" \"$GIT_WORK_TREE\" \"$ELFIE_ROOT\" > \"$ELFIE_ROOT/where.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:A\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho 'ELFIE: DONE'\n",
        );
        if !a_repository(&fixture) {
            return;
        }
        // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
        assert_eq!(fixture.run(&["compile", "--jobs", "2"]), ExitCode::Success.code());
        // Everything the batch wrote in its copy reached the root.
        // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
        assert!(fixture.read("out/a.rs").contains("pub struct A"), "the output is in the root");
        // GIT_DIR names the root's git directory and GIT_WORK_TREE the copy, which is also
        // ELFIE_ROOT.
        // @lfy def/cli/main.lfy:main#main:main:256d3040e99b1dbe822f8eb7b21d3d34f15faf77af0dc09bc5f7724644b6453b
        let text = fixture.read("where.txt");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], git_directory(&fixture.root).unwrap().to_string_lossy(), "{text}");
        assert_eq!(lines[1], lines[2], "{text}");
        assert!(lines[1].contains("elfie-compile/cache/batches/a"), "{text}");
        // The source maps are recorded in the root and the copy is removed.
        // @lfy def/cli/main.lfy:main#main:main:32ec9ca39bc2ed83e058c8f7cf792dac4595e470dc4c4ccb73d2412d430942dc
        assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("out/a.rs"));
        assert!(!fixture.root.join("elfie-compile/cache/batches/a").exists(), "the copy is gone");
    }
}
