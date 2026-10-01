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
use std::thread::JoinHandle;
use std::time::SystemTime;

use elfie_core::generation::{
    self, Batch, Outcome, OutcomeKind, Output, Plan, Reason, Request, Review, ReviewReport, ReviewStatus, SourceMap,
    Unit, Verdict,
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
    // @lfy def/cli/main.lfy:parse
    let forced = if arguments.iter().any(|a| a == "--help" || a == "-h") {
        Some(Command::Help)
    } else if arguments.iter().any(|a| a == "--version") {
        Some(Command::Version) // @lfy def/cli/main.lfy:parse
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
        // @lfy def/cli/main.lfy:parse
        if argument == "--help" || argument == "-h" || argument == "--version" {
            i += 1;
            continue;
        }
        // --tree followed by a file keeps working as the tree command; followed by no file
        // it names no command, so arguments holding nothing that does not begin with a dash
        // spell help as they would without it.
        // @lfy def/cli/main.lfy:parse
        if argument == "--tree" {
            // @lfy def/cli/main.lfy:parse
            if arguments.get(i + 1).is_some_and(|file| !file.starts_with('-')) {
                command = Some(Command::Tree);
            }
            i += 1;
            continue;
        }
        // --root <dir> or --root=<dir> sets the root.
        // @lfy def/cli/main.lfy:parse
        if let Some(value) = argument.strip_prefix("--root=") {
            root = Some(value.to_string());
            i += 1;
            continue;
        }
        if argument == "--root" {
            let Some(value) = arguments.get(i + 1) else {
                return Err("--root needs a directory".to_string());
            };
            root = Some(value.clone());
            i += 2;
            continue;
        }
        // Every argument beginning with two dashes other than --root, --tree, --help, and
        // --version is an option.
        // @lfy def/cli/main.lfy:parse
        if let Some(name) = argument.strip_prefix("--") {
            if let Some((name, value)) = name.split_once('=') {
                // --color=value gives the text, and a value that is no ColorChoice is a
                // usage error. @lfy def/cli/main.lfy:parse
                if name == COLOR && ColorChoice::lookup(value).is_none() {
                    return Err(COLOR_USAGE.to_string());
                }
                options.insert(name.to_string(), Some(value.to_string()));
            } else if name == COLOR {
                // --color followed by auto, always, or never gives that value; written with
                // no equals sign and not followed by one, it is a flag and gives true, read
                // as always, and the next argument is left for what follows.
                // @lfy def/cli/main.lfy:parse
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
                let Some(value) = arguments.get(i + 1) else {
                    return Err(format!("--{name} needs a value"));
                };
                options.insert(name.to_string(), Some(value.clone()));
                i += 1;
            } else {
                options.insert(name.to_string(), None);
            }
            i += 1;
            continue;
        }
        // The first argument that does not begin with a dash names the command; every other
        // one, and every one at all when --help or --version is among them, is positional.
        // @lfy def/cli/main.lfy:parse
        if command.is_none() && forced.is_none() {
            match Command::lookup(argument) {
                Some(found) => command = Some(found),
                None => return Err(format!("{argument} is not a command")), // @lfy def/cli/main.lfy:parse
            }
        } else {
            positional.push(argument.clone());
        }
        i += 1;
    }
    // @lfy def/cli/main.lfy:parse
    if let Some(forced) = forced {
        return Ok(Invocation { command: forced, root: find_root(root.as_deref()), arguments: Vec::new(), options });
    }
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
        // @lfy def/cli/main.lfy:main
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
        // @lfy def/cli/main.lfy:main
        if json {
            return Paint { out: false, err: false };
        }
        // @lfy def/cli/main.lfy:main
        Paint { out: colors_on(choice, false), err: colors_on(choice, true) }
    }
}

/// What --color asked for: the value it names, always for the bare flag, and auto when it is
/// not given.
// @lfy def/cli/main.lfy:main
fn color_choice(invocation: &Invocation) -> ColorChoice {
    match invocation.options.get(COLOR) {
        // @lfy def/cli/main.lfy:main
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
    let mut line = paint("elfie:", Tone::Failure, on);
    // @lfy def/cli/main.lfy:main
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
const OPTIONS_WITH_VALUES: [&str; 1] = ["target"];

/// The option that says when the CLI colors what it prints.
// @lfy def/cli/main.lfy:parse
const COLOR: &str = "color";

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
    if let Some(given) = given {
        return given.to_string();
    }
    let mut current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let start = current.clone();
    loop {
        if current.join("elfie.json").exists() {
            return current.to_string_lossy().into_owned();
        }
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
            // from them as they are written. @lfy def/cli/main.lfy:main
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
        // @lfy def/cli/main.lfy:main
        Command::Lsp => ExitCode::from_code(elfie_lsp::serve(Some(Path::new(&invocation.root)))),
        // @lfy def/cli/main.lfy:main
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
const OPTIONS: [(&str, &str); 13] = [
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
    // @lfy def/cli/main.lfy:main
    let column = Command::ALL
        .iter()
        .map(|command| width(command.value()))
        .chain(OPTIONS.iter().map(|(name, _)| width(name)))
        .max()
        .unwrap_or(0)
        + 2;
    let mut out = String::new();
    // @lfy def/cli/main.lfy:main
    out.push_str(&paint("usage: elfie <command> [arguments] [--root <dir>] [--json]", Tone::Subject, on));
    out.push_str("\n\n");
    out.push_str(&paint("commands:", Tone::Subject, on));
    out.push('\n');
    // Every command of Command, verify among them. @lfy def/cli/main.lfy:main
    for command in Command::ALL {
        let name = padded(&paint(command.value(), Tone::Active, on), column);
        out.push_str(&format!("  {name}{}\n", command.description()));
    }
    out.push('\n');
    // @lfy def/cli/main.lfy:main
    out.push_str(&paint("options:", Tone::Subject, on));
    out.push('\n');
    // The global options, --no-verify, --strict, and --color among them.
    // @lfy def/cli/main.lfy:main
    for (name, description) in OPTIONS {
        let name = padded(&paint(name, Tone::Active, on), column);
        out.push_str(&format!("  {name}{description}\n"));
    }
    out
}

/// Prints every command, verify among them, with one line of description and the global
/// options, --no-verify, --strict, and --color among them.
// @lfy def/cli/main.lfy:main
fn help(invocation: &Invocation) -> ExitCode {
    print!("{}", help_text(Paint::of(invocation).out));
    ExitCode::Success
}

/// Prints the version of the executable.
// @lfy def/cli/main.lfy:main
fn version() -> ExitCode {
    println!("elfie {}", env!("CARGO_PKG_VERSION"));
    ExitCode::Success
}

/// The one line .gitignore holds for a project the CLI set up: the cache is derivable and
/// nothing else under elfie-compile is, since the maps are checked in.
// @lfy def/cli/main.lfy:main
const IGNORED: &str = "/elfie-compile/cache/";

/// Where a unit's source maps are recorded, as [`generation::map_file`] spells it; init
/// creates it so a project has it before its first compile.
// @lfy def/cli/main.lfy:main
const MAPS: &str = "elfie-compile/maps";

/// Creates elfie.json in the root naming the project after the argument, or the directory,
/// creates the source directory and elfie-compile/maps, and leaves .gitignore ignoring the
/// cache; fails when elfie.json already exists.
// @lfy def/cli/main.lfy:main
fn init(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let on = Paint::of(invocation).err;
    let manifest = root.join("elfie.json");
    // @lfy def/cli/main.lfy:main
    if manifest.exists() {
        complain(on, Some(&manifest.display().to_string()), "already exists");
        return ExitCode::Failure;
    }
    // The project is named after the argument, or after the directory.
    // @lfy def/cli/main.lfy:main
    let name = invocation
        .arguments
        .first()
        .cloned()
        .or_else(|| root.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())))
        .unwrap_or_else(|| "project".to_string());
    let text = format!("{{\n  \"name\": {}\n}}\n", serde_json::Value::String(name));
    // The source directory and elfie-compile/maps, where the maps are recorded and checked
    // in; nothing ignores elfie-compile itself or the maps.
    // @lfy def/cli/main.lfy:main
    let written = fs::create_dir_all(root.join("def"))
        .and_then(|()| fs::create_dir_all(root.join(MAPS)))
        .and_then(|()| fs::write(&manifest, text));
    if let Err(error) = written {
        complain(on, Some(&manifest.display().to_string()), &error.to_string());
        return ExitCode::Failure;
    }
    // @lfy def/cli/main.lfy:main
    if let Err(error) = ignore_the_cache(root) {
        complain(on, Some(".gitignore"), &error.to_string());
        return ExitCode::Failure;
    }
    // @lfy def/cli/main.lfy:main
    println!("{}", created_line(invocation.flag("json"), &manifest.display().to_string()));
    ExitCode::Success
}

/// What init prints for the project it set up: one object with --json, so nothing but JSON
/// reaches standard output, and the text otherwise.
// @lfy def/cli/main.lfy:main
fn created_line(json: bool, manifest: &str) -> String {
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    let Ok(text) = fs::read_to_string(&path) else {
        return fs::write(&path, format!("{IGNORED}\n"));
    };
    // @lfy def/cli/main.lfy:main
    if text.lines().any(|line| line.trim() == IGNORED) {
        return Ok(());
    }
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    let place = format!("{}:{}:{}:", diagnostic.range.file, diagnostic.range.start.line, diagnostic.range.start.column);
    format!(
        "{} {} {}: {}",
        paint(&place, Tone::Subject, on),
        paint(diagnostic.stage.value(), Tone::Muted, on),
        // @lfy def/cli/main.lfy:main
        paint(diagnostic.severity.value(), severity_tone(diagnostic.severity), on),
        diagnostic.message
    )
}

// @lfy def/cli/main.lfy:main
fn print_diagnostic(diagnostic: &Diagnostic, json: bool, on: bool) {
    if json {
        // @lfy def/cli/main.lfy:main
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
    let diagnostics = collect_diagnostics(&workspace, &invocation.arguments);
    for diagnostic in &diagnostics {
        print_diagnostic(diagnostic, json, on);
    }
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return ExitCode::Problems;
    }
    // With no files given, the plan is made with no stems requested, from the source maps of
    // the workspace and the stems the last global review found violated, and each unit that
    // is not up to date is printed as a warning of stage generation.
    // @lfy def/cli/main.lfy:main
    let mut stale = 0usize;
    if invocation.arguments.is_empty() {
        // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    if json {
        println!(
            "{}",
            serde_json::json!({ "files": workspace.files.len(), "problems": diagnostics.len(), "stale": stale })
        );
    } else {
        // @lfy def/cli/main.lfy:main
        println!(
            "{} files, {}, {}",
            workspace.files.len(),
            tally(diagnostics.len(), "problems", Tone::Failure, on),
            tally(stale, "stale units", Tone::Warning, on)
        );
    }
    // --strict: the code is problems when anything at all was printed as a warning or
    // error, a stale unit included.
    // @lfy def/cli/main.lfy:main
    if invocation.flag("strict") && (!diagnostics.is_empty() || stale > 0) {
        ExitCode::Problems
    } else {
        ExitCode::Success
    }
}

/// A unit that is not up to date, printed as a warning of stage generation at its source
/// file, naming the target, the unit, and the description of its reason; the target and the
/// unit are subject and the description is painted in the reason's own tone.
// @lfy def/cli/main.lfy:main
fn print_stale(workspace: &Workspace, unit: &Unit, reason: Reason, json: bool, on: bool) {
    let file = &workspace.files[unit.file].path;
    let target = &workspace.targets[unit.target].identifier;
    let message = format!("{target} {}: {}", unit.stem, reason.value());
    if json {
        println!(
            "{}",
            serde_json::json!({ "file": file, "stage": "generation", "severity": "warning", "message": message })
        );
    } else {
        // @lfy def/cli/main.lfy:main
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
    if files.is_empty() {
        query::diagnostics_of(workspace, None)
    } else {
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
    let files: Vec<PathBuf> = if invocation.arguments.is_empty() {
        let workspace = workspace::load(root);
        let mut found = Vec::new();
        walk(&root.join(&workspace.source_directory), &mut found);
        found.sort();
        found
    } else {
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
        if !tree.errors.is_empty() {
            // A file whose tree has errors is left as it is.
            // @lfy def/cli/main.lfy:main
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
                    // @lfy def/cli/main.lfy:main
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
        let formatted = elfie_core::format::format(&tree);
        if formatted == source {
            continue;
        }
        // Nothing is written; each file that would change is printed.
        // @lfy def/cli/main.lfy:main
        if check_only {
            // @lfy def/cli/main.lfy:main
            println!("{}", formatted_line(json, &display, true, style.out));
            code = ExitCode::Problems;
        } else if let Err(error) = fs::write(&path, formatted) {
            complain(style.err, Some(&display), &error.to_string());
            code = ExitCode::Failure;
        } else {
            // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    if json {
        return if check_only {
            serde_json::json!({ "file": file, "changed": true }).to_string()
        } else {
            serde_json::json!({ "file": file, "formatted": true }).to_string()
        };
    }
    // @lfy def/cli/main.lfy:main
    if check_only {
        return paint(file, Tone::Warning, on);
    }
    // @lfy def/cli/main.lfy:main
    format!("{} {file}", paint("formatted", Tone::Success, on))
}

/// Every `.lfy` file under a directory.
// @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
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
            // @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
fn node_lines(tree: &Tree, node: &Node, depth: usize, on: bool, out: &mut Vec<String>) {
    let indent = " ".repeat(depth * 2);
    // @lfy def/cli/main.lfy:main
    out.push(format!(
        "{indent}{} {}",
        paint(node.rule.identifier(), Tone::Active, on),
        paint(&format!("{}..{}", node.start, node.end), Tone::Muted, on)
    ));
    let indent = " ".repeat((depth + 1) * 2);
    for child in &node.children {
        match child {
            Child::Node(child) => node_lines(tree, child, depth + 1, on, out),
            // @lfy def/cli/main.lfy:main
            Child::Error(error) => out.push(format!(
                "{indent}{} {} {} {}",
                paint("Error", Tone::Failure, on),
                paint(&format!("{}..{}", error.start, error.end), Tone::Muted, on),
                paint(&format!("expected [{}]", error.expected.join(", ")), Tone::Failure, on),
                paint(&format!("{:?}", tree.raw(error.start, error.end)), Tone::Failure, on)
            )),
            Child::Token(index) => {
                let token = tree.token(*index);
                // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    if json {
        node_json(tree, &tree.root, 0, &mut lines);
        return lines;
    }
    node_lines(tree, &tree.root, 0, on, &mut lines);
    // Each error node is printed with what it expected, painted failure.
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    for line in tree_lines(&tree, &path, invocation.flag("json"), Paint::of(invocation).out) {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    if tree.errors.is_empty() { ExitCode::Success } else { ExitCode::Problems }
}

/// The lines the tokens command prints: one token per line as its line, column, rule, and
/// value. With --json each token is one JSON object on one line instead of the text. The line
/// and column are muted, the rule is active, and the value is success.
// @lfy def/cli/main.lfy:main
fn token_lines(tokens: &[Token], json: bool, on: bool) -> Vec<String> {
    tokens
        .iter()
        .map(|token| {
            // @lfy def/cli/main.lfy:main
            if json {
                let value = serde_json::json!({
                    "line": token.line,
                    "column": token.column,
                    "rule": rule_name(token),
                    "value": token.value,
                });
                value.to_string()
            } else {
                // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    for line in token_lines(&tokens, invocation.flag("json"), Paint::of(invocation).out) {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    ExitCode::Success
}

// ---- progress ---------------------------------------------------------------------

/// The heading below which a person writes the answer to a question the compiler asked.
// @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
fn step_column() -> usize {
    STEPS.iter().map(|step| width(glyph_of(*step)) + 1 + width(step.name())).max().unwrap_or(0)
}

/// How many columns the time column takes.
// @lfy def/cli/main.lfy:main
const TIME_COLUMN: usize = 6;

/// How many spaces a problem, a reason, or a question that follows a progress line is
/// indented by.
// @lfy def/cli/main.lfy:main
const INDENT: &str = "    ";

/// The counts a reviewed, globalReviewed, or finished line reports: how many, what they
/// count, and the tone each is painted in when it is not 0.
// @lfy def/cli/main.lfy:main
type Counts = Vec<(usize, &'static str, Tone)>;

/// The counts as one message: each a [`tally`], separated by a comma and a space. Painted or
/// not, it holds the same words, so the message a script reads with --json is the one a
/// person reads.
// @lfy def/cli/main.lfy:main
fn counts_message(counts: &Counts, on: bool) -> String {
    counts.iter().map(|(count, label, tone)| tally(*count, label, *tone, on)).collect::<Vec<_>>().join(", ")
}

/// The counts of satisfied, violated, and unverifiable reviews, in that order, each with the
/// tone it is painted in.
// @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
fn text_of(progress: &Progress, counts: Option<&Counts>, tone: Option<Tone>, on: bool) -> String {
    // The counter keeps one width for the whole compile. @lfy def/cli/main.lfy:main
    let digits = progress.total.to_string().len();
    let counter = paint(&format!("[{:>digits$}/{}]", progress.done, progress.total), Tone::Muted, on);
    // @lfy def/cli/main.lfy:main
    let time = paint(&format!("{:>TIME_COLUMN$}", duration(progress.elapsed)), Tone::Muted, on);
    // @lfy def/cli/main.lfy:main
    let named = format!("{} {}", glyph_of(progress.step), progress.step.name());
    let step = padded(&paint(&named, tone.unwrap_or_else(|| tone_of(progress.step)), on), step_column());
    let mut columns = vec![counter, time, step];
    // @lfy def/cli/main.lfy:main
    if let Some(batch) = &progress.batch {
        columns.push(paint(batch, Tone::Subject, on));
    }
    // The unit follows the batch, and is not repeated when it is the batch.
    // @lfy def/cli/main.lfy:main
    if let Some(unit) = &progress.unit
        && progress.batch.as_deref() != Some(unit.as_str())
    {
        columns.push(paint(unit, Tone::Subject, on));
    }
    // @lfy def/cli/main.lfy:main
    let message = match counts {
        // @lfy def/cli/main.lfy:main
        Some(counts) => counts_message(counts, on),
        None => {
            let first = progress.message.lines().next().unwrap_or_default();
            match progress.step {
                // @lfy def/cli/main.lfy:main
                Step::Rejected | Step::Failed | Step::Blocked | Step::Clarification => {
                    paint(first, tone_of(progress.step), on)
                }
                // @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
struct Reporter {
    json: bool,
    /// Whether each stream is painted; the log is written with nothing painted.
    // @lfy def/cli/main.lfy:main
    style: Paint,
    started: SystemTime,
    total: usize,
    done: usize,
    logs: Vec<PathBuf>,
    /// The batch the last line concerned, so the first line of a batch after the first batch
    /// has an empty line before it.
    // @lfy def/cli/main.lfy:main
    batch: Option<String>,
}

impl Reporter {
    // @lfy def/cli/main.lfy:main
    fn new(json: bool, style: Paint, total: usize, logs: Vec<PathBuf>) -> Reporter {
        Reporter { json, style, started: SystemTime::now(), total, done: 0, logs, batch: None }
    }

    /// One line for one step, printed and logged.
    // @lfy def/cli/main.lfy:main
    fn report(&mut self, step: Step, batch: Option<&str>, unit: Option<&str>, message: &str) {
        self.emit(step, batch, unit, message.to_string(), None, None);
    }

    /// One line whose message is the counts it reports, each painted only when it is worth
    /// noticing.
    // @lfy def/cli/main.lfy:main
    fn report_counts(&mut self, step: Step, batch: Option<&str>, counts: &Counts, tone: Option<Tone>) {
        // The message with --json holds the same words the counts are printed with.
        // @lfy def/cli/main.lfy:main
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
            // @lfy def/cli/main.lfy:main
            elapsed: elapsed(self.started),
            message,
        };
        // An empty line before the first line of a batch after the first batch, before
        // finished, and before globalVerifying, so each batch reads as one block.
        // @lfy def/cli/main.lfy:main
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
        // @lfy def/cli/main.lfy:main
        if self.json {
            let line = json_of(&progress).to_string();
            println!("{line}");
            self.append(&line);
        } else {
            println!("{}", text_of(&progress, counts, tone, self.style.out));
            // @lfy def/cli/main.lfy:main
            self.append(&text_of(&progress, counts, tone, false));
        }
    }

    /// The problems of a step, following the line they belong to: one per line, each indented
    /// four spaces, beginning with `-` and a space and painted failure. With --json nothing
    /// follows, since the whole of the message is in the [`Progress`].
    // @lfy def/cli/main.lfy:main
    fn problems(&mut self, problems: &[String]) {
        if self.json {
            return;
        }
        for problem in problems {
            // @lfy def/cli/main.lfy:main
            let body = format!("- {problem}");
            println!("{INDENT}{}", paint(&body, Tone::Failure, self.style.out));
            self.append(&format!("{INDENT}{body}"));
        }
    }

    /// A reason or a question, following the line it belongs to: one line of text per line,
    /// each indented four spaces and painted warning.
    // @lfy def/cli/main.lfy:main
    fn reason(&mut self, text: &str) {
        if self.json {
            return;
        }
        for line in text.lines() {
            // @lfy def/cli/main.lfy:main
            println!("{INDENT}{}", paint(line, Tone::Warning, self.style.out));
            self.append(&format!("{INDENT}{line}"));
        }
    }

    /// Appends text to `elfie-requests/compile.log` under the root, so a run can be read
    /// after the fact.
    // @lfy def/cli/main.lfy:main
    fn append(&self, text: &str) {
        for path in &self.logs {
            let Some(parent) = path.parent() else { continue };
            if fs::create_dir_all(parent).is_err() {
                continue;
            }
            let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) else { continue };
            let _ = writeln!(file, "{text}");
        }
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
// @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
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
            // @lfy def/cli/main.lfy:main
            every &= generation::record(workspace, unit, &mine);
        }
        // @lfy def/cli/main.lfy:main
        if every {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Every map file under the folder of a planned target that belongs to no unit of the plan,
/// because its definition file was deleted, moved, or no longer builds for the target,
/// removed before any batch runs.
// @lfy def/cli/main.lfy:main
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
            // @lfy def/cli/main.lfy:main
            if path.extension().is_some_and(|e| e == "json") && !mine.contains(&path) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// The files under the target's output directory that carry a marker for the unit's file.
// @lfy def/cli/main.lfy:main
fn outputs_of(workspace: &Workspace, plan: &Plan, unit: usize) -> Vec<Output> {
    let unit = &plan.units[unit];
    let target = &workspace.targets[unit.target];
    let file = &workspace.files[unit.file].path;
    let mut paths = Vec::new();
    walk_all(&workspace.root.join(&target.output_directory), &mut paths);
    paths.sort();
    let mut found = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if generation::parse_markers(&text).iter().any(|m| &m.file == file) {
            found.push(Output { path: relative(&workspace.root, &path), text });
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
// @lfy def/cli/main.lfy:main
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
        // @lfy def/cli/main.lfy:main
        let Some(lowered) = lowered_text(workspace, &file, &text) else { continue };
        // @lfy def/cli/main.lfy:main
        if generation::source_hash(&lowered) == map.hash {
            return Some(lowered);
        }
    }
    None
}

/// One file read as something other than what is on disk, bound through the loader and
/// lowered: [`elfie_core::interpret::LoweredFile::text`] of it, the way `changes` binds a
/// previous text.
// @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
fn unit_line(workspace: &Workspace, plan: &Plan, index: usize, on: bool) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&d| plan.units[d].stem.as_str()).collect();
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
fn dry_run(workspace: &Workspace, plan: &Plan, target: Option<&str>, json: bool, on: bool) -> ExitCode {
    for index in 0..plan.units.len() {
        let unit = &plan.units[index];
        if target.is_some_and(|name| workspace.targets[unit.target].identifier != name) {
            continue;
        }
        if json {
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
// @lfy def/cli/main.lfy:main
const GLOBAL_REVIEWS: &str = "global.reviews.json";

/// What the verifier is handed when it reviews every global criterion and test once, and the
/// batch it is run for.
// @lfy def/cli/main.lfy:main
const GLOBAL: &str = "global";

/// Whether a requirement id is a global one: a global id begins with global.
// @lfy def/cli/main.lfy:main
fn is_global(id: &str) -> bool {
    id == GLOBAL || id.starts_with("global:")
}

/// The last global review: the SHA-256 of the ids of every global criterion and test it
/// reviewed, and its reviews, read from elfie-requests/global.reviews.json under the root.
/// There is no last global review when the file is missing.
// @lfy def/cli/main.lfy:main
struct LastReview {
    requirements: String,
    reviews: Vec<Review>,
}

// @lfy def/cli/main.lfy:main
fn last_global_review(root: &Path) -> Option<LastReview> {
    let text = fs::read_to_string(requests_directory(root).join(GLOBAL_REVIEWS)).ok()?;
    // @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
fn violated_ids(root: &Path) -> BTreeSet<String> {
    let Some(last) = last_global_review(root) else {
        // @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
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
// @lfy def/cli/main.lfy:main
fn stems_answering(plan: &Plan, ids: &BTreeSet<String>) -> Vec<String> {
    if ids.is_empty() {
        return Vec::new();
    }
    let mut stems: Vec<String> = plan
        .units
        .iter()
        .filter(|unit| {
            unit.outputs.iter().any(|map| {
                // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
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
    let (program, plan) = plan_with(root, maps, requested, &[]);
    // @lfy def/cli/main.lfy:main
    let stems = stems_answering(&plan, violated);
    if stems.is_empty() && !all {
        return (program, plan);
    }
    // --all requests every unit. @lfy def/cli/main.lfy:main
    let requested: Vec<String> =
        if all { plan.units.iter().map(|unit| unit.stem.clone()).collect() } else { requested.to_vec() };
    plan_with(root, maps, &requested, &stems)
}

/// `elfie-requests/compile.log` under the root, never under an output directory, so no
/// output is ever mistaken for the CLI's own files.
// @lfy def/cli/main.lfy:main
fn log_paths(root: &Path) -> Vec<PathBuf> {
    vec![requests_directory(root).join("compile.log")]
}

/// `elfie-requests` under the root: where the CLI's own files live, never under an output
/// directory.
// @lfy def/cli/main.lfy:main
fn requests_directory(root: &Path) -> PathBuf {
    root.join("elfie-requests")
}

/// The problems of an attempt appended to its instructions, so that the batch is run once
/// more knowing what was wrong.
// @lfy def/cli/main.lfy:main
fn append_problems(request: &mut Request, problems: &[String]) {
    request.instructions.push_str("\n\n## Problems with the previous attempt\n\n");
    for problem in problems {
        request.instructions.push_str(&format!("- {problem}\n"));
    }
}

/// One problem per violated review: `failure at`, the file, a colon, the line, a colon, the
/// note, and the evidence in parentheses.
// @lfy def/cli/main.lfy:main
fn problem_of(review: &Review) -> String {
    format!("failure at {}:{}: {} ({})", review.file, review.line, review.note, review.evidence)
}

/// The counts of satisfied, violated, and unverifiable reviews, in that order.
// @lfy def/cli/main.lfy:main
fn counts_of(reviews: &[Review]) -> (usize, usize, usize) {
    let count = |status| reviews.iter().filter(|review| review.status == status).count();
    (count(ReviewStatus::Satisfied), count(ReviewStatus::Violated), count(ReviewStatus::Unverifiable))
}

/// What a finished command left: its code, -1 when it ended by a signal or could not be
/// started, and everything it wrote, held whole.
// @lfy def/cli/main.lfy:main
struct Exit {
    code: i32,
    stdout: String,
    stderr: String,
}

/// The compiler's output, each line shown as it arrives prefixed by the batch and kept
/// whole. With --json it is shown on standard error, so that standard output stays one
/// JSON object per line.
// @lfy def/cli/main.lfy:main
fn watch<R: io::Read + Send + 'static>(
    pipe: R,
    batch: &str,
    keep: bool,
    to_stdout: bool,
    on: bool,
    is_error: bool,
) -> JoinHandle<String> {
    // Two spaces, the batch, a space, a bar, and a space; the prefix of a line from standard
    // output is muted and of one from standard error warning.
    // @lfy def/cli/main.lfy:main
    let prefix = paint(
        &format!("  {batch} \u{2502} "),
        if is_error { Tone::Warning } else { Tone::Muted },
        on,
    );
    std::thread::spawn(move || {
        let mut kept = String::new();
        for line in io::BufReader::new(pipe).lines().map_while(Result::ok) {
            // The line itself is shown exactly as the command wrote it.
            // @lfy def/cli/main.lfy:main
            if to_stdout {
                println!("{prefix}{line}");
            } else {
                eprintln!("{prefix}{line}");
            }
            if keep {
                kept.push_str(&line);
                kept.push('\n');
            }
        }
        kept
    })
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
    json: bool,
    /// Whether each stream is painted.
    // @lfy def/cli/main.lfy:main
    style: Paint,
    continuing: bool,
    /// The command elfie.json names under `verifier`; nothing verifies a batch without one.
    // @lfy def/cli/main.lfy:main
    verifier: Option<String>,
    /// --no-verify: no verifier runs and nothing is reviewed.
    // @lfy def/cli/main.lfy:main
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
        Run {
            program,
            plan,
            maps,
            reporter,
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
    // @lfy def/cli/main.lfy:main
    fn stop(&mut self, batch: &Batch) {
        self.incomplete.push(batch.identifier.clone());
        self.stopped.extend(batch.units.iter().copied());
        if !self.continuing {
            self.halted = true;
        }
    }

    /// The batches of the plan a compile runs, in plan order: every one, or only those of the
    /// target --target names.
    // @lfy def/cli/main.lfy:main
    fn batches_for(&self, target: Option<&str>) -> Vec<usize> {
        (0..self.plan.batches.len())
            .filter(|&index| {
                // @lfy def/cli/main.lfy:main
                target.is_none_or(|name| batch_target(self.workspace(), &self.plan, &self.plan.batches[index]) == name)
            })
            .collect()
    }

    /// Whether a batch holds a unit depending on one of a batch that did not complete.
    // @lfy def/cli/main.lfy:main
    fn depends_on_stopped(&self, batch: &Batch) -> bool {
        batch
            .units
            .iter()
            .any(|&unit| self.plan.units[unit].dependencies.iter().any(|d| self.stopped.contains(d)))
    }

    /// The directory a batch's requests, questions, and reasons are written to:
    /// `elfie-requests` under the root, never under an output directory.
    // @lfy def/cli/main.lfy:main
    fn requests_directory(&self) -> PathBuf {
        requests_directory(&self.workspace().root)
    }

    /// Writes `elfie-requests/<batch>.<suffix>` under the root.
    // @lfy def/cli/main.lfy:main
    fn write_note(&self, batch: &Batch, suffix: &str, text: &str) -> Option<PathBuf> {
        let directory = self.requests_directory();
        let path = directory.join(format!("{}.{suffix}", file_name_of(&batch.identifier)));
        match fs::create_dir_all(&directory).and_then(|()| fs::write(&path, text)) {
            Ok(()) => Some(path),
            Err(error) => {
                // Any other message the CLI itself prints to standard error begins with
                // elfie: painted failure, and a message that begins with a path has the
                // path painted subject. @lfy def/cli/main.lfy:main
                complain(self.style.err, Some(&path.to_string_lossy()), &error.to_string());
                None
            }
        }
    }

    /// The answer a person wrote below the question in
    /// `elfie-requests/<batch>.question.md`, when there is one.
    // @lfy def/cli/main.lfy:main
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

    /// The request of one batch, with its existing outputs read from disk, the source each
    /// unit's outputs were generated from where git still holds it, and the answer to a
    /// question asked before appended.
    // @lfy def/cli/main.lfy:main
    fn request_of(&self, batch: &Batch) -> Request {
        let existing: Vec<Output> = batch
            .units
            .iter()
            .flat_map(|&unit| self.plan.units[unit].outputs.iter())
            .filter_map(|map| {
                fs::read_to_string(self.workspace().root.join(&map.output))
                    .ok()
                    .map(|text| Output { path: map.output.clone(), text })
            })
            .collect();
        // @lfy def/cli/main.lfy:main
        let mut previous = BTreeMap::new();
        for &index in &batch.units {
            let unit = &self.plan.units[index];
            if let Some(text) = previous_source(self.workspace(), unit) {
                previous.insert(self.workspace().files[unit.file].path.clone(), text);
            }
        }
        // @lfy def/cli/main.lfy:main
        let mut request = generation::request(&self.program, &self.plan, batch, &existing, &previous);
        // The person answers by editing the definitions, or by writing the answer below
        // the question, which the next compile of the batch appends to its instructions.
        // @lfy def/cli/main.lfy:main
        if let Some(answer) = self.answer_of(batch) {
            request.instructions.push_str(&format!("\n\n## The answer to the question asked before\n\n{answer}\n"));
        }
        request
    }

    /// Streams one agent, the compiler or the verifier, once for one batch: the
    /// instructions as its input, the root as its directory, and ELFIE_ROOT, ELFIE_BATCH,
    /// and ELFIE_UNITS (the stems, space separated) as its environment. Its standard output
    /// is the report. The code is -1 when the command could not be started.
    // @lfy def/cli/main.lfy:main
    fn run_agent(&self, command: &str, root: &Path, label: &str, units: &str, instructions: &str, what: &str) -> Exit {
        let spawned = Process::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(root)
            .env("ELFIE_ROOT", root)
            .env("ELFIE_BATCH", label)
            .env("ELFIE_UNITS", units)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        // The command could not be started: the code is -1 and the failure is the output.
        // @lfy def/cli/main.lfy:main
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => return Exit { code: -1, stdout: String::new(), stderr: error.to_string() },
        };
        // Each line is shown as it arrives, prefixed by the batch; with --json it goes to
        // standard error, so standard output stays one JSON object per line.
        // @lfy def/cli/main.lfy:main
        let out = child.stdout.take().map(|pipe| watch(pipe, label, true, !self.json, self.style.out, false));
        let err = child.stderr.take().map(|pipe| watch(pipe, label, true, false, self.style.err, true));
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(instructions.as_bytes())
        {
            self.reporter.append(&format!("the {what} command did not read its input: {error}"));
        }
        let status = child.wait();
        let stdout = out.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        let stderr = err.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        let status = match status {
            Ok(status) => status,
            Err(error) => return Exit { code: -1, stdout, stderr: error.to_string() },
        };
        if !status.success() {
            self.reporter.append(&format!("the {what} command exited with {status}"));
        }
        Exit { code: status.code().unwrap_or(-1), stdout, stderr }
    }

    /// For each unit of the batch, `accept` runs on the files under the target's output
    /// directory that carry a marker for the unit.
    // @lfy def/cli/main.lfy:main
    fn verdicts_of(&mut self, batch: &Batch, request: &Request) -> Vec<Verdict> {
        let mut verdicts = Vec::new();
        for &unit in &batch.units {
            let stem = self.plan.units[unit].stem.clone();
            let reason = self.plan.units[unit].reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
            self.reporter.report(Step::Checking, Some(&batch.identifier), Some(&stem), &reason);
            let outputs = outputs_of(self.workspace(), &self.plan, unit);
            verdicts.push(generation::accept(&self.program, &self.plan, request, unit, &outputs));
        }
        verdicts
    }

    /// One unit's outputs written back and the unit counted as done. The source maps
    /// [`generation::accept`] derived replace the unit's in the list held for the batch;
    /// nothing reaches a map file until that list is recorded.
    // @lfy def/cli/main.lfy:main
    fn write_back(&mut self, unit: usize, verdict: &Verdict, held: &mut Vec<SourceMap>) -> io::Result<()> {
        for output in &verdict.outputs {
            let path = self.workspace().root.join(&output.path);
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
        // is counted once. @lfy def/cli/main.lfy:main
        if self.recorded.insert(self.plan.units[unit].stem.clone()) {
            self.accepted += 1;
            self.reporter.done = self.accepted;
        }
        Ok(())
    }

    /// Every unit of an accepted batch written back, and the source maps its verdicts
    /// earned held beside the recorded ones: the verifier is pointed at them, and they are
    /// recorded only once nothing has violated what was asked.
    // @lfy def/cli/main.lfy:main
    fn hold_batch(&mut self, batch: &Batch, verdicts: &[Verdict]) -> Vec<SourceMap> {
        let mut held = self.maps.clone();
        for (&unit, verdict) in batch.units.iter().zip(verdicts) {
            let stem = self.plan.units[unit].stem.clone();
            if let Err(error) = self.write_back(unit, verdict, &mut held) {
                complain(self.style.err, Some(&stem), &error.to_string());
                self.worsen(ExitCode::Failure);
                continue;
            }
            let written = format!("{} outputs", verdict.outputs.len());
            self.reporter.report(Step::Accepted, Some(&batch.identifier), Some(&stem), &written);
        }
        held
    }

    /// The held source maps recorded: each unit's are recorded in its own map file, so
    /// recording one unit leaves every other map file byte for byte as it was.
    // @lfy def/cli/main.lfy:main
    fn record_maps(&mut self, held: Vec<SourceMap>) {
        self.maps = held;
        // @lfy def/cli/main.lfy:main
        self.accepted_batch = true;
        let mut unwritten: Vec<String> = Vec::new();
        for unit in &self.plan.units {
            // Only a unit written back in this compile has anything new to record.
            // @lfy def/cli/main.lfy:main
            if !self.recorded.contains(&unit.stem) {
                continue;
            }
            let workspace = &self.program.workspace;
            let target = &workspace.targets[unit.target].identifier;
            let source = &workspace.files[unit.file].path;
            let mine: Vec<SourceMap> = self
                .maps
                .iter()
                .filter(|map| &map.target == target && &map.source == source)
                .cloned()
                .collect();
            // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    fn report_rejections(&mut self, batch: &Batch, verdicts: &[Verdict]) -> usize {
        let mut rejected = 0;
        for (&unit, verdict) in batch.units.iter().zip(verdicts) {
            if verdict.accepted {
                continue;
            }
            rejected += 1;
            let stem = self.plan.units[unit].stem.clone();
            // @lfy def/cli/main.lfy:main
            let message = verdict.problems.join("\n");
            self.reporter.report(Step::Rejected, Some(&batch.identifier), Some(&stem), &message);
            // @lfy def/cli/main.lfy:main
            self.reporter.problems(&verdict.problems);
        }
        rejected
    }

    /// A batch whose units were accepted and written back, and whose reviews then violated
    /// what was asked: the acceptance was structural, and the review retracts it, so the
    /// finished line and the code count the units as rejected rather than accepted. Its
    /// held source maps are dropped rather than recorded, so its outputs stay on disk as
    /// they are and the unit stays planned.
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    fn write_reviews(&mut self, batch: &Batch, reviews: &[Review]) {
        let value = serde_json::Value::Array(reviews.iter().map(Review::to_json).collect());
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default());
        self.write_note(batch, "reviews.json", &text);
    }

    /// The review request of a batch written for a verifier run by hand.
    // @lfy def/cli/main.lfy:main
    fn write_review_request(&mut self, batch: &Batch) {
        let request = generation::review(&self.program, &self.plan, batch, &self.maps);
        if let Some(path) = self.write_note(batch, "review.md", &request.instructions)
            && !self.json
        {
            println!("wrote {}", relative(&self.workspace().root, &path));
        }
    }

    /// The verifier run once for one batch, its output shown as it arrives and logged, its
    /// report read, and its reviews written. A run that failed — its problems are not
    /// empty, so no line of its output read `ELFIE: REVIEWED` or some line was neither a
    /// review nor a heading, or the command could not be started — is run once more; when
    /// that run fails too its problems are printed as a failed progress line and the batch
    /// is verified with the reviews parsed from it as if its report had been complete.
    // @lfy def/cli/main.lfy:main
    fn review_batch(&mut self, batch: &Batch, command: &str, root: &Path, progress: bool, maps: &[SourceMap]) -> ReviewReport {
        if progress {
            self.reporter.report(Step::Verifying, Some(&batch.identifier), None, command);
        }
        // The source maps are the ones `accept` derived, held but not yet recorded, so
        // every criterion can be pointed at the region of output that claims to satisfy it.
        // @lfy def/cli/main.lfy:main
        let request = generation::review(&self.program, &self.plan, batch, maps);
        let units = stems_of(&self.plan, batch, " ");
        let report = self.ask_verifier(command, root, &batch.identifier, &units, &request.instructions);
        self.write_reviews(batch, &report.reviews);
        report
    }

    /// The verifier run for one batch, or once for the global criteria and tests, its output
    /// shown as it arrives and logged and its report read. A run that failed — its problems
    /// are not empty, so no line of its output read `ELFIE: REVIEWED` or some line was neither
    /// a review nor a heading, or the command could not be started — is run once more; when
    /// that run fails too its problems are printed as a failed progress line and the reviews
    /// parsed from it are read as if its report had been complete.
    // @lfy def/cli/main.lfy:main
    fn ask_verifier(&mut self, command: &str, root: &Path, label: &str, units: &str, instructions: &str) -> ReviewReport {
        let mut report = ReviewReport::default();
        for attempt in 1..=2 {
            // @lfy def/cli/main.lfy:main
            let exit = self.run_agent(command, root, label, units, instructions, "verifier");
            self.reporter.append(&format!("--- the review of {label} (attempt {attempt}) ---\n{}", exit.stdout));
            // @lfy def/cli/main.lfy:main
            report = generation::review_of(&exit.stdout, &self.program);
            if exit.code == -1 {
                report
                    .problems
                    .insert(0, format!("the verifier command could not be run: {}", exit.stderr.trim()));
            }
            // @lfy def/cli/main.lfy:main
            if report.problems.is_empty() {
                break;
            }
            // @lfy def/cli/main.lfy:main
            if attempt == 2 {
                let problems = report.problems.clone();
                self.reporter.report(Step::Failed, Some(label), None, &problems.join("\n"));
                self.reporter.problems(&problems);
                if exit.code == -1 {
                    self.failed += 1;
                    self.worsen(ExitCode::Failure);
                }
            }
        }
        report
    }

    /// A batch whose units were accepted by `accept` is verified against the source maps it
    /// derived, held but not yet recorded, and the problems of its violated reviews are what
    /// it is rejected with; nothing verifies it with --no-verify or with no verifier named,
    /// and an unverifiable review is counted and never rejects.
    // @lfy def/cli/main.lfy:main
    fn verify_batch(&mut self, batch: &Batch, root: &Path, held: &[SourceMap]) -> Option<Vec<String>> {
        // @lfy def/cli/main.lfy:main
        if self.no_verify {
            return None;
        }
        let command = self.verifier.clone()?;
        let report = self.review_batch(batch, &command, root, true, held);
        // One reviewed line follows, whose message is the counts of satisfied, violated,
        // and unverifiable reviews, each painted only when it is worth noticing.
        // @lfy def/cli/main.lfy:main
        let counts = review_counts(&report.reviews);
        self.reporter.report_counts(Step::Reviewed, Some(&batch.identifier), &counts, None);
        // An unverifiable review is counted in the reviewed line and never rejects the batch.
        // @lfy def/cli/main.lfy:main
        if counts_of(&report.reviews).1 == 0 {
            return None;
        }
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
    // @lfy def/cli/main.lfy:main
    fn global_review(&mut self, root: &Path) -> Vec<String> {
        // @lfy def/cli/main.lfy:main
        if self.program.criteria.is_empty() && self.program.tests.is_empty() {
            return Vec::new();
        }
        // @lfy def/cli/main.lfy:main
        if self.no_verify {
            return Vec::new();
        }
        let Some(command) = self.verifier.clone() else {
            return Vec::new();
        };
        // @lfy def/cli/main.lfy:main
        let requirements = global_requirements(&self.program);
        let last = last_global_review(root);
        // @lfy def/cli/main.lfy:main
        if !self.accepted_batch && last.as_ref().is_some_and(|last| last.requirements == requirements) {
            return Vec::new();
        }
        // @lfy def/cli/main.lfy:main
        self.reporter.report(Step::GlobalVerifying, None, None, &command);
        // Every source map, recorded or held. @lfy def/cli/main.lfy:main
        let request = generation::global_review(&self.program, &self.maps);
        // The units just generated among them. @lfy def/cli/main.lfy:main
        let units = self.global_stems();
        let report = self.ask_verifier(&command, root, GLOBAL, &units, &request.instructions);
        self.write_global_reviews(root, &requirements, &report);
        // @lfy def/cli/main.lfy:main
        let counts = review_counts(&report.reviews);
        self.reporter.report_counts(Step::GlobalReviewed, None, &counts, None);
        // @lfy def/cli/main.lfy:main
        let violated: Vec<&Review> =
            report.reviews.iter().filter(|review| review.status == ReviewStatus::Violated).collect();
        if violated.is_empty() {
            return Vec::new();
        }
        // One problem per violated review: failure at, a space, global, a colon, a space, the
        // note, and the evidence in parentheses. @lfy def/cli/main.lfy:main
        let problems: Vec<String> = violated
            .iter()
            .map(|review| format!("failure at {GLOBAL}: {} ({})", review.note, review.evidence))
            .collect();
        self.reporter.problems(&problems);
        // The units whose markers answered for it. @lfy def/cli/main.lfy:main
        let ids: BTreeSet<String> = violated.iter().map(|review| review.id.clone()).collect();
        self.stems_answering_now(&ids)
    }

    /// The stems of every unit whose source maps hold a marker answering for one of these
    /// ids, read from the maps as they stand: a unit compiled in this run has its markers
    /// there rather than in the outputs its plan was made from.
    // @lfy def/cli/main.lfy:main
    fn stems_answering_now(&self, ids: &BTreeSet<String>) -> Vec<String> {
        // @lfy def/cli/main.lfy:main
        self.stems_answering_with(|id| ids.contains(id))
    }

    /// The stems, space separated, of every unit whose source maps hold a marker whose
    /// requirement is a global id: what `ELFIE_UNITS` holds for a global review.
    // @lfy def/cli/main.lfy:main
    fn global_stems(&self) -> String {
        // @lfy def/cli/main.lfy:main
        self.stems_answering_with(is_global).join(" ")
    }

    /// The stems of every unit whose source maps hold a marker whose requirement the test
    /// accepts.
    ///
    /// A unit's source maps are those this compile accepted for it when it has any, and
    /// [`Unit::outputs`] otherwise, so a unit generated in this compile is named by the
    /// markers it has just earned rather than by the ones the plan was made from, and a unit
    /// nothing touched by the ones its map file holds.
    // @lfy def/cli/main.lfy:main
    fn stems_answering_with(&self, answers: impl Fn(&str) -> bool) -> Vec<String> {
        let mut stems: Vec<String> = Vec::new();
        for unit in &self.plan.units {
            let target = &self.program.workspace.targets[unit.target].identifier;
            let source = &self.program.workspace.files[unit.file].path;
            // @lfy def/cli/main.lfy:main
            let accepted: Vec<&SourceMap> =
                self.maps.iter().filter(|map| &map.target == target && &map.source == source).collect();
            // @lfy def/cli/main.lfy:main
            let maps = if accepted.is_empty() { unit.outputs.iter().collect() } else { accepted };
            // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    fn write_global_reviews(&mut self, root: &Path, requirements: &str, report: &ReviewReport) {
        // @lfy def/cli/main.lfy:main
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
    /// for the opinion, so nothing gates it, and the reviews are given back to be printed.
    // @lfy def/cli/main.lfy:main
    fn verify_globally(&mut self, root: &Path) -> Vec<Review> {
        let Some(command) = self.verifier.clone() else {
            return Vec::new();
        };
        // @lfy def/cli/main.lfy:main
        let request = generation::global_review(&self.program, &self.maps);
        // @lfy def/cli/main.lfy:main
        let units = self.global_stems();
        let report = self.ask_verifier(&command, root, GLOBAL, &units, &request.instructions);
        // @lfy def/cli/main.lfy:main
        let requirements = global_requirements(&self.program);
        self.write_global_reviews(root, &requirements, &report);
        report.reviews
    }

    /// Runs the compiler on one batch, at most twice, and acts on the outcome; a batch
    /// whose reviews violate what was asked is run once more beyond that.
    // @lfy def/cli/main.lfy:main
    fn compile_batch(&mut self, batch: &Batch, command: &str, root: &Path) {
        let stems = stems_of(&self.plan, batch, " ");
        self.reporter.report(Step::Requesting, Some(&batch.identifier), None, &stems);
        let mut request = self.request_of(batch);
        let mut attempt = 0;
        // The batch is run once more for a rejected outcome, and once more for violated
        // reviews even when it was already run once more for a rejected outcome.
        // @lfy def/cli/main.lfy:main
        let mut retried_for_rejection = false;
        let mut retried_for_reviews = false;
        let mut retried_for_failure = false;
        loop {
            attempt += 1;
            let step = if attempt == 1 { Step::Compiling } else { Step::Retrying };
            self.reporter.report(step, Some(&batch.identifier), None, command);
            // @lfy def/cli/main.lfy:main
            let units = stems_of(&self.plan, batch, " ");
            let exit = self.run_agent(command, root, &batch.identifier, &units, &request.instructions, "compiler");
            // The code is -1 because the command could not be started: the outcome is failed.
            // @lfy def/cli/main.lfy:main
            let outcome = if exit.code == -1 {
                Outcome {
                    kind: OutcomeKind::Failed,
                    message: format!("the compiler command could not be run: {}", exit.stderr.trim()),
                    verdicts: Vec::new(),
                }
            } else {
                let report = exit.stdout;
                self.reporter.append(&format!("--- the report of {} (attempt {attempt}) ---\n{report}", batch.identifier));
                // @lfy def/cli/main.lfy:main
                let verdicts = self.verdicts_of(batch, &request);
                generation::outcome_of(&report, verdicts)
            };
            match outcome.kind {
                // Each unit's outputs are written back and its source maps are held, and the
                // batch is verified; no review violated, or nothing verifying it, records
                // the held source maps and the batch is done.
                // @lfy def/cli/main.lfy:main
                OutcomeKind::Accepted => {
                    let held = self.hold_batch(batch, &outcome.verdicts);
                    // @lfy def/cli/main.lfy:main
                    let Some(problems) = self.verify_batch(batch, root, &held) else {
                        self.record_maps(held);
                        break;
                    };
                    // A violated review is handled as a rejected outcome is: the problems
                    // are printed and the batch is run once more with them appended to the
                    // instructions; a violated review then stops the batch as rejected, and
                    // the held source maps are dropped rather than recorded, so the unit
                    // stays planned. @lfy def/cli/main.lfy:main
                    self.reporter.report(Step::Rejected, Some(&batch.identifier), None, &problems.join("\n"));
                    self.reporter.problems(&problems);
                    if retried_for_reviews {
                        self.unrecord(batch);
                        self.worsen(ExitCode::Problems);
                        self.stop(batch);
                        break;
                    }
                    retried_for_reviews = true;
                    append_problems(&mut request, &problems);
                }
                // The problems are printed, and the batch is run once more with them
                // appended to the instructions; a second rejection stops the compile.
                // @lfy def/cli/main.lfy:main
                OutcomeKind::Rejected => {
                    let rejected = self.report_rejections(batch, &outcome.verdicts);
                    if retried_for_rejection {
                        self.rejected += rejected;
                        self.worsen(ExitCode::Problems);
                        self.stop(batch);
                        break;
                    }
                    retried_for_rejection = true;
                    let problems: Vec<String> =
                        outcome.verdicts.iter().flat_map(|verdict| verdict.problems.iter().cloned()).collect();
                    append_problems(&mut request, &problems);
                }
                // The reason is printed and written, and the compile stops.
                // @lfy def/cli/main.lfy:main
                OutcomeKind::Blocked => {
                    self.blocked += 1;
                    self.reporter.report(Step::Blocked, Some(&batch.identifier), None, &outcome.message);
                    // @lfy def/cli/main.lfy:main
                    self.reporter.reason(&outcome.message);
                    let note = format!("# The compiler is blocked on the batch {}\n\n{}\n", batch.identifier, outcome.message);
                    self.write_note(batch, "blocked.md", &note);
                    self.worsen(ExitCode::Problems);
                    self.stop(batch);
                    break;
                }
                // The question is printed and written, and the compile stops.
                // @lfy def/cli/main.lfy:main
                OutcomeKind::Clarification => {
                    self.reporter.report(Step::Clarification, Some(&batch.identifier), None, &outcome.message);
                    // @lfy def/cli/main.lfy:main
                    self.reporter.reason(&outcome.message);
                    let note = format!(
                        "# The compiler asked about the batch {}\n\n{}\n\n{ANSWER_HEADING}\n\n<!-- Answer by editing the definitions, or write the answer below this line; the next compile of this batch appends it to the instructions. -->\n",
                        batch.identifier, outcome.message
                    );
                    self.write_note(batch, "question.md", &note);
                    self.worsen(ExitCode::Problems);
                    self.stop(batch);
                    break;
                }
                // The batch is run once more; a second failure stops the compile.
                // @lfy def/cli/main.lfy:main
                OutcomeKind::Failed => {
                    self.reporter.report(Step::Failed, Some(&batch.identifier), None, &outcome.message);
                    // @lfy def/cli/main.lfy:main
                    self.reporter.problems(std::slice::from_ref(&outcome.message));
                    if retried_for_failure {
                        self.failed += 1;
                        self.worsen(ExitCode::Failure);
                        self.stop(batch);
                        break;
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
    // @lfy def/cli/main.lfy:main
    fn accept_on_disk(&mut self, batch: &Batch, root: &Path) {
        let request = self.request_of(batch);
        let verdicts = self.verdicts_of(batch, &request);
        let accepted: Vec<Verdict> = verdicts.iter().filter(|v| v.accepted).cloned().collect();
        let mut held = self.maps.clone();
        if !accepted.is_empty() {
            let only = Batch {
                units: batch.units.iter().copied().zip(&verdicts).filter(|(_, v)| v.accepted).map(|(unit, _)| unit).collect(),
                identifier: batch.identifier.clone(),
            };
            held = self.hold_batch(&only, &accepted);
        }
        let rejected = self.report_rejections(batch, &verdicts);
        if rejected > 0 {
            // What was accepted is recorded all the same, so a compiler that worked through
            // the agent server has its work recorded. @lfy def/cli/main.lfy:main
            self.record_maps(held);
            self.rejected += rejected;
            self.worsen(ExitCode::Problems);
            return;
        }
        // @lfy def/cli/main.lfy:main
        if let Some(problems) = self.verify_batch(batch, root, &held) {
            self.reporter.report(Step::Rejected, Some(&batch.identifier), None, &problems.join("\n"));
            self.reporter.problems(&problems);
            self.unrecord(batch);
            self.worsen(ExitCode::Problems);
            self.stop(batch);
        } else {
            // @lfy def/cli/main.lfy:main
            self.record_maps(held);
        }
    }

    /// The request of a batch written for a compiler run by hand.
    // @lfy def/cli/main.lfy:main
    fn write_request(&mut self, batch: &Batch) {
        let stems = stems_of(&self.plan, batch, " ");
        self.reporter.report(Step::Requesting, Some(&batch.identifier), None, &stems);
        let request = self.request_of(batch);
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
    // @lfy def/cli/main.lfy:main
    fn finish(&mut self) {
        // @lfy def/cli/main.lfy:main
        let counts: Counts = vec![
            (self.accepted, "units accepted", Tone::Success),
            (self.rejected, "rejected", Tone::Failure),
            (self.blocked, "batches blocked", Tone::Warning),
            (self.failed, "failed", Tone::Failure),
        ];
        // @lfy def/cli/main.lfy:main
        let went_wrong = self.rejected > 0 || self.blocked > 0 || self.failed > 0;
        let tone = if went_wrong { Tone::Failure } else { Tone::Success };
        // With --json the one object carries every batch that did not complete too, since
        // nothing follows a JSON line. @lfy def/cli/main.lfy:main
        if self.json {
            let mut message = counts_message(&counts, false);
            if !self.incomplete.is_empty() {
                message.push_str(&format!("; did not complete: {}", self.incomplete.join(", ")));
            }
            self.reporter.report(Step::Finished, None, None, &message);
            return;
        }
        self.reporter.report_counts(Step::Finished, None, &counts, Some(tone));
        // @lfy def/cli/main.lfy:main
        if !self.incomplete.is_empty() {
            let line = format!("did not complete: {}", self.incomplete.join(", "));
            self.reporter.reason(&line);
        }
    }
}

/// Every error diagnostic of the program printed; whether there was one.
// @lfy def/cli/main.lfy:main
fn print_errors(workspace: &Workspace, json: bool, on: bool) -> bool {
    let diagnostics = query::diagnostics_of(workspace, None);
    let errors: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    for diagnostic in &errors {
        print_diagnostic(diagnostic, json, on);
    }
    !errors.is_empty()
}

/// The target --target limits the plan to, when it names a known one.
// @lfy def/cli/main.lfy:main
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
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // The compiler is never handed a program with problems.
    // @lfy def/cli/main.lfy:main
    if print_errors(&workspace, json, style.out) {
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // With --target the maps of that target alone are read, so no other target's map files
    // are opened. @lfy def/cli/main.lfy:main
    let maps = generation::source_maps_of(&workspace, target.as_deref());
    let legacy = legacy_targets(&workspace, target.as_deref());
    drop(workspace);
    // The units the last global review found violated. @lfy def/cli/main.lfy:main
    let (program, plan) = planned(root, &maps, &invocation.arguments, invocation.flag("all"), &violated_ids(root));
    // @lfy def/cli/main.lfy:main
    if invocation.flag("dry-run") {
        return dry_run(&program.workspace, &plan, target.as_deref(), json, style.out);
    }
    // Before any batch runs, the maps a target kept in source-map.json are moved into the
    // units' map files and every map file belonging to no unit of the plan is removed.
    // @lfy def/cli/main.lfy:main
    move_legacy_maps(&program.workspace, &plan, &legacy);
    remove_orphan_maps(&program.workspace, &plan, target.as_deref());

    let reporter = Reporter::new(json, style, 0, log_paths(root));
    let mut run = Run::new(program, plan, maps.clone(), reporter, invocation);
    let compiler = compiler_command(root);
    let accept_only = invocation.flag("accept");
    // The units a violated global review plans are compiled once more in the same compile,
    // and the global review runs once more after them; a second violated global review stops
    // the compile there. @lfy def/cli/main.lfy:main
    let mut round = 1;
    loop {
        let batches = run.batches_for(target.as_deref());
        let total: usize = batches.iter().map(|&index| run.plan.batches[index].units.len()).sum();
        run.reporter.total = total;
        // The first progress line of a compile is planned, with the count of units planned
        // and of batches, whether anything was planned or not, so that a compile with
        // nothing to do still reads as one run. @lfy def/cli/main.lfy:main
        let planned = format!("{total} units planned, {} batches", batches.len());
        run.reporter.report(Step::Planned, None, None, &planned);
        // @lfy def/cli/main.lfy:main
        if total == 0 && round == 1 && !json {
            // @lfy def/cli/main.lfy:main
            println!("{}", paint("every unit is up to date", Tone::Success, style.out));
        }
        run.accepted_batch = false;
        for index in batches {
            let batch = run.plan.batches[index].clone();
            // A stopped batch stops only the batches that depend on one of its units.
            // @lfy def/cli/main.lfy:main
            if run.halted || run.depends_on_stopped(&batch) {
                run.incomplete.push(batch.identifier.clone());
                run.stopped.extend(batch.units.iter().copied());
                continue;
            }
            if accept_only {
                run.accept_on_disk(&batch, root); // @lfy def/cli/main.lfy:main
            } else if let Some(command) = &compiler {
                run.compile_batch(&batch, command, root); // @lfy def/cli/main.lfy:main
            } else {
                run.write_request(&batch); // @lfy def/cli/main.lfy:main
            }
        }
        // Every batch has been handled: the global criteria and tests are reviewed once.
        // @lfy def/cli/main.lfy:main
        let violated = run.global_review(root);
        // The second global review of a compile stops it there, and the next compile plans
        // those units with reason violated. @lfy def/cli/main.lfy:main
        if violated.is_empty() || round == 2 || accept_only || compiler.is_none() {
            if !violated.is_empty() {
                run.worsen(ExitCode::Problems);
            }
            break;
        }
        // @lfy def/cli/main.lfy:main
        let (program, plan) = plan_with(root, &maps, &[], &violated);
        run.replan(program, plan, 0);
        round = 2;
    }
    // The last progress line of a compile is finished, with the count accepted, rejected,
    // blocked, and failed. @lfy def/cli/main.lfy:main
    run.finish();
    run.code
}

// ---- verify -----------------------------------------------------------------------

/// Verify is compile without the compiler: the same plan, the same review request, the same
/// verifier, so a person can ask for an opinion on outputs already recorded. Nothing is
/// compiled and no output is written.
// @lfy def/cli/main.lfy:main
fn verify(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // Every error diagnostic is printed and verify ends with the code problems.
    // @lfy def/cli/main.lfy:main
    if print_errors(&workspace, json, style.out) {
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // Nothing under elfie-compile is recorded, moved, or removed: the maps are only read.
    // @lfy def/cli/main.lfy:main
    let maps = generation::source_maps_of(&workspace, target.as_deref());
    drop(workspace);
    let (_, known) = plan_with(root, &maps, &[], &[]);
    // Every stem given as a requested unit or, with no stem, every unit whose outputs are
    // not empty. @lfy def/cli/main.lfy:main
    let mut requested: Vec<String> = Vec::new();
    // --global with no stem reviews no batch. @lfy def/cli/main.lfy:main
    let global_only = invocation.flag("global") && invocation.arguments.is_empty();
    if invocation.arguments.is_empty() {
        requested.extend(known.units.iter().filter(|unit| !unit.outputs.is_empty()).map(|unit| unit.stem.clone()));
    } else {
        for stem in &invocation.arguments {
            // A stem naming a unit whose outputs are empty is printed as having nothing to
            // review and left out, and the code does not change for it.
            // @lfy def/cli/main.lfy:main
            match known.units.iter().find(|unit| &unit.stem == stem) {
                Some(unit) if !unit.outputs.is_empty() => requested.push(stem.clone()),
                _ if json => println!("{}", serde_json::json!({ "stem": stem, "message": "nothing to review" })),
                _ => println!("{stem}: nothing to review"),
            }
        }
    }
    // After any batches are reviewed, every global criterion and test is reviewed once with
    // no stem given, or with --global. @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    let batches: Vec<usize> = (0..plan.batches.len())
        .filter(|&index| {
            let batch = &plan.batches[index];
            target.as_deref().is_none_or(|name| batch_target(&program.workspace, &plan, batch) == name)
                && batch.units.iter().any(|&unit| wanted.contains(plan.units[unit].stem.as_str()))
        })
        .collect();
    let total: usize = batches.iter().map(|&index| plan.batches[index].units.len()).sum();
    let reporter = Reporter::new(json, style, total, log_paths(root));
    let mut run = Run::new(program, plan, maps, reporter, invocation);
    let verifier = verifier_command(root);
    // The source maps already recorded are what the verifier is pointed at here: verify
    // compiles nothing, so there are none to hold. @lfy def/cli/main.lfy:main
    let recorded = run.maps.clone();
    // @lfy def/cli/main.lfy:main
    for index in if global_only { Vec::new() } else { batches } {
        let batch = run.plan.batches[index].clone();
        // With no verifier named, the review request of each batch is written for a
        // verifier run by hand and the code is success. @lfy def/cli/main.lfy:main
        let Some(command) = verifier.clone() else {
            run.write_review_request(&batch);
            continue;
        };
        // The verifier is run exactly as compile runs it after acceptance, a failed run
        // being run once more the same way. @lfy def/cli/main.lfy:main
        let report = run.review_batch(&batch, &command, root, false, &recorded);
        print_reviews(&report.reviews, json, style.out);
        // @lfy def/cli/main.lfy:main
        if counts_of(&report.reviews).1 > 0 {
            run.worsen(ExitCode::Problems);
        }
    }
    // @lfy def/cli/main.lfy:main
    if globally && verifier.is_some() {
        // Nothing of the compile's own gating applies here: a person asked for the opinion.
        // @lfy def/cli/main.lfy:main
        let reviews = run.verify_globally(root);
        print_reviews(&reviews, json, style.out);
        // @lfy def/cli/main.lfy:main
        if counts_of(&reviews).1 > 0 {
            run.worsen(ExitCode::Problems);
        }
    }
    run.code
}

/// Every review printed as its status, a space, the file, a colon, the line, a space, the
/// entity, a colon, a space, and the note, then one line giving the counts of satisfied,
/// violated, and unverifiable. Without --json the status is painted in its own tone and padded
/// to the longest status of [`ReviewStatus`], the file and line are subject, and the entity and
/// the note are plain; with --json each review is one JSON object on one line.
// @lfy def/cli/main.lfy:main
fn print_reviews(reviews: &[Review], json: bool, on: bool) {
    for review in reviews {
        // @lfy def/cli/main.lfy:main
        if json {
            println!("{}", review.to_json());
        } else {
            println!("{}", painted_review(review, on));
        }
    }
    // After each batch one line gives the counts of satisfied, violated, and unverifiable.
    // @lfy def/cli/main.lfy:main
    let counts = counts_of(reviews);
    if json {
        println!("{}", serde_json::json!({ "satisfied": counts.0, "violated": counts.1, "unverifiable": counts.2 }));
    } else {
        // @lfy def/cli/main.lfy:main
        println!("{}", counts_message(&review_counts(reviews), on));
    }
}

/// One review as the verify command prints it, painted.
// @lfy def/cli/main.lfy:main
fn painted_review(review: &Review, on: bool) -> String {
    // @lfy def/cli/main.lfy:main
    let column = [ReviewStatus::Satisfied, ReviewStatus::Violated, ReviewStatus::Unverifiable]
        .iter()
        .map(|status| width(&status.to_string()))
        .max()
        .unwrap_or(0);
    // @lfy def/cli/main.lfy:main
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

    /// How many lines a file the fixture wrote holds; none when it was never written.
    fn lines_of(fixture: &Fixture, path: &str) -> usize {
        fs::read_to_string(fixture.root.join(path)).map_or(0, |text| text.lines().count())
    }

    /// The tokens of a source text, which always lexes in these tests.
    fn tokens_of(source: &str) -> Vec<Token> {
        lex(source, Some("def/a.lfy")).unwrap()
    }

    // @lfy def/cli/main.lfy:parse
    #[test]
    fn check_with_no_arguments_parses() {
        let invocation = parse_arguments(&["check".to_string()]).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert!(invocation.arguments.is_empty());
    }

    // @lfy def/cli/main.lfy:parse
    #[test]
    fn the_tree_flag_is_the_tree_command() {
        let invocation = parse_arguments(&["--tree".to_string(), "def/a.lfy".to_string()]).unwrap();
        assert_eq!(invocation.command, Command::Tree);
        assert_eq!(invocation.arguments, vec!["def/a.lfy".to_string()]);
        // --tree followed by no file is followed by no file, so nothing that does not begin
        // with a dash is given and the command is help. @lfy def/cli/main.lfy:parse
        assert_eq!(parse_arguments(&args(&["--tree"])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["--tree", "--json"])).unwrap().command, Command::Help);
        // A command named as well still names itself. @lfy def/cli/main.lfy:parse
        assert_eq!(parse_arguments(&args(&["--tree", "def/a.lfy", "--json"])).unwrap().command, Command::Tree);
    }

    // @lfy def/cli/main.lfy:parse
    #[test]
    fn an_unknown_command_is_a_usage_error() {
        let error = parse_arguments(&["frobnicate".to_string()]).unwrap_err();
        assert!(error.contains("frobnicate"));
    }

    // @lfy def/cli/main.lfy:parse
    #[test]
    fn options_take_flags_and_values() {
        let invocation = parse_arguments(&["compile".to_string(), "--dry-run".to_string(), "--target=rust".to_string(), "--root".to_string(), "/tmp".to_string(), "lexer/main".to_string()]).unwrap();
        assert!(invocation.flag("dry-run"));
        assert_eq!(invocation.option("target"), Some("rust"));
        assert_eq!(invocation.root, "/tmp");
        assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
        let invocation = parse_arguments(&["compile".to_string(), "--target".to_string(), "rust".to_string()]).unwrap();
        assert_eq!(invocation.option("target"), Some("rust"));
        // An option's value is missing: a usage message naming the argument.
        // @lfy def/cli/main.lfy:parse
        let error = parse_arguments(&["compile".to_string(), "--target".to_string()]).unwrap_err();
        assert!(error.contains("target"), "{error}");
    }

    // @lfy def/cli/main.lfy:parse
    #[test]
    fn help_and_version_win_anywhere() {
        assert_eq!(parse_arguments(&["check".to_string(), "--help".to_string()]).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&["-h".to_string()]).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&["--version".to_string()]).unwrap().command, Command::Version);
        assert_eq!(parse_arguments(&[]).unwrap().command, Command::Help);
        // An argument that does not begin with a dash names no command when --help is among
        // them. @lfy def/cli/main.lfy:parse
        assert_eq!(parse_arguments(&["--help".to_string(), "frobnicate".to_string()]).unwrap().command, Command::Help);
    }

    /// Words, then a short list of them, as the arguments of one invocation.
    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_string()).collect()
    }

    /// --color followed by one of [`ColorChoice`] takes it as its value; written with no
    /// equals sign and not followed by one it is a flag, read as always, and the next argument
    /// is left for what follows; --color=value naming no choice is a usage message.
    // @lfy def/cli/main.lfy:parse
    #[test]
    fn color_takes_a_choice_or_is_a_flag() {
        // @lfy def/cli/main.lfy:parse
        let invocation = parse_arguments(&args(&["--color", "never", "check"])).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert_eq!(invocation.option("color"), Some("never"));
        // @lfy def/cli/main.lfy:parse
        assert_eq!(color_choice(&invocation), ColorChoice::Never);
        // @lfy def/cli/main.lfy:parse
        let invocation = parse_arguments(&args(&["check", "--color=always"])).unwrap();
        assert_eq!(color_choice(&invocation), ColorChoice::Always);
        // Written with no equals sign and not followed by a choice, it is a flag read as
        // always, and the next argument still names the command. @lfy def/cli/main.lfy:parse
        let invocation = parse_arguments(&args(&["--color", "check"])).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert!(invocation.flag("color"));
        assert_eq!(invocation.option("color"), None);
        // @lfy def/cli/main.lfy:parse
        assert_eq!(color_choice(&invocation), ColorChoice::Always);
        // A value naming no choice is a usage message reading --color must be auto, always,
        // or never. @lfy def/cli/main.lfy:parse
        let error = parse_arguments(&args(&["check", "--color=sometimes"])).unwrap_err();
        assert_eq!(error, "--color must be auto, always, or never");
        // Not given at all, the choice is auto. @lfy def/cli/main.lfy:parse
        let invocation = parse_arguments(&args(&["check"])).unwrap();
        assert_eq!(color_choice(&invocation), ColorChoice::Auto);
        // Every flag other than --color, --root, and --target never takes the next argument,
        // so --json check still names the check command. @lfy def/cli/main.lfy:parse
        let invocation = parse_arguments(&args(&["--json", "check"])).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert!(invocation.flag("json"));
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
    /// description and the global options, --no-verify, --strict, and --color among them, and
    /// version prints the version; both return success.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn help_and_version_return_success() {
        let text = help_text(false);
        for command in Command::ALL {
            assert!(text.contains(command.value()), "{text}");
            assert!(text.contains(command.description()), "{text}");
        }
        assert!(text.contains(Command::Verify.value()), "{text}");
        // @lfy def/cli/main.lfy:main
        for option in ["--root", "--json", "--no-verify", "--strict", "--color", "--help, -h", "--version"] {
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
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
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
                "#!/bin/sh\ncat > \"$ELFIE_ROOT/instructions.txt\"\nprintf '%s\\n' \"$ELFIE_BATCH\" \"$ELFIE_UNITS\" > \"$ELFIE_ROOT/environment.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:1\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho done\n",
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
        assert_eq!(fixture.read("environment.txt"), "a\na\n");
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
        let mut run = Run::new(program, plan, Vec::new(), reporter, &invocation);
        let batch = run.plan.batches[0].clone();
        run.compile_batch(&batch, "true", Path::new("/no/such/directory"));
        assert_eq!(run.code, ExitCode::Failure);
        assert_eq!(run.failed, 1);
        assert!(run.halted);
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
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_verifier_whose_report_does_not_end_is_run_once_more() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, UNENDED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        assert_eq!(lines_of(&fixture, "verifications.txt"), 2, "the verifier ran twice");
        assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran once");
        let log = fixture.read("elfie-requests/compile.log");
        // @lfy def/cli/main.lfy:main
        assert!(log.contains("✗ failed"), "{log}");
        assert!(log.contains("1 satisfied, 0 violated, 0 unverifiable"), "{log}");
        // The reviews of that run are written all the same. @lfy def/cli/main.lfy:main
        assert!(fixture.read("elfie-requests/a.reviews.json").contains("covered by a test"));
    }

    /// With --no-verify no verifier runs, nothing is reviewed, no reviews file is written,
    /// and the batch is done and its source maps recorded when its units are accepted.
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
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
    /// written.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn no_global_criterion_means_no_global_review() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main
        assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
        let log = fixture.read("elfie-requests/compile.log");
        assert!(!log.contains("globalVerifying"), "{log}");
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
    // @lfy def/cli/main.lfy:main
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
    // @lfy def/cli/main.lfy:main
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
}
