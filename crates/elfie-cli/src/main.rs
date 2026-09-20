//! Compiled from `def/cli/main.lfy`: the `elfie` command line.
//!
//! Every command is a thin driver over the stage it belongs to, and the process exits
//! with a code from [`ExitCode`] so agents and scripts can read the outcome without
//! parsing text. Progress is a stream of one-line events rather than a redrawn screen, so
//! it reads the same in a terminal, in a log, and as JSON lines for a script.

mod data;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::thread::JoinHandle;
use std::time::SystemTime;

use elfie_core::generation::{
    self, Batch, Outcome, OutcomeKind, Output, Plan, Request, SourceMap, Unit, Verdict,
};
use elfie_core::lexer::lex;
use elfie_core::parser::parse;
use elfie_core::query::{self, Diagnostic, Severity};
use elfie_core::workspace::{self, Workspace};

pub use data::{Command, ExitCode, Invocation, Progress, Step};

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    std::process::ExitCode::from(run(&arguments))
}

/// The invocation the arguments spell, or the usage error they make.
// @lfy def/cli/main.lfy:parse
pub fn parse_arguments(arguments: &[String]) -> Result<Invocation, String> {
    let mut command = None;
    let mut root = None;
    let mut positional = Vec::new();
    let mut options: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut i = 0;
    while i < arguments.len() {
        let argument = &arguments[i];
        // --help or -h anywhere means help; --version means version.
        // @lfy def/cli/main.lfy:parse
        if argument == "--help" || argument == "-h" {
            return Ok(Invocation { command: Command::Help, root: find_root(None), arguments: Vec::new(), options });
        }
        if argument == "--version" {
            return Ok(Invocation { command: Command::Version, root: find_root(None), arguments: Vec::new(), options });
        }
        // --tree <file> keeps working as the tree command.
        // @lfy def/cli/main.lfy:parse
        if argument == "--tree" {
            command = Some(Command::Tree);
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
        // Every other argument beginning with two dashes is an option.
        // @lfy def/cli/main.lfy:parse
        if let Some(name) = argument.strip_prefix("--") {
            if let Some((name, value)) = name.split_once('=') {
                options.insert(name.to_string(), Some(value.to_string()));
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
        // The first argument that does not begin with a dash names the command.
        // @lfy def/cli/main.lfy:parse
        if command.is_none() {
            match Command::lookup(argument) {
                Some(found) => command = Some(found),
                None => return Err(format!("{argument} is not a command")), // @lfy def/cli/main.lfy:parse
            }
        } else {
            positional.push(argument.clone());
        }
        i += 1;
    }
    Ok(Invocation {
        command: command.unwrap_or(Command::Help),
        root: find_root(root.as_deref()),
        arguments: positional,
        options,
    })
}

/// Options that take a value written as the next argument.
// @lfy def/cli/main.lfy:parse
const OPTIONS_WITH_VALUES: [&str; 1] = ["target"];

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
            // A usage message is printed with the help text and the code is usage.
            // @lfy def/cli/main.lfy:main
            eprintln!("elfie: {message}\n");
            eprintln!("{}", help_text());
            return ExitCode::Usage.code();
        }
    };
    let code = match invocation.command {
        Command::Help => help(),
        Command::Version => version(),
        Command::Init => init(&invocation),
        Command::Check => check(&invocation),
        Command::Format => format_command(&invocation),
        Command::Tree => tree(&invocation),
        Command::Tokens => tokens(&invocation),
        Command::Compile => compile(&invocation),
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

// @lfy def/cli/main.lfy:main
fn help_text() -> String {
    let mut out = String::from("usage: elfie <command> [arguments] [--root <dir>] [--json]\n\ncommands:\n");
    for command in Command::ALL {
        out.push_str(&format!("  {:<8} {}\n", command.value(), command.description()));
    }
    out.push_str("\noptions:\n  --root <dir>   The project directory (default: the nearest elfie.json above the current directory)\n  --json         Print results as JSON lines\n  --help, -h     Print this help\n  --version      Print the version\n");
    out
}

/// Prints every command with one line of description and the global options.
// @lfy def/cli/main.lfy:main
fn help() -> ExitCode {
    print!("{}", help_text());
    ExitCode::Success
}

/// Prints the version of the executable.
// @lfy def/cli/main.lfy:main
fn version() -> ExitCode {
    println!("elfie {}", env!("CARGO_PKG_VERSION"));
    ExitCode::Success
}

/// Creates elfie.json in the root naming the project after the argument, or the
/// directory, and creates the source directory; fails when elfie.json already exists.
// @lfy def/cli/main.lfy:main
fn init(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let manifest = root.join("elfie.json");
    if manifest.exists() {
        eprintln!("{}: already exists", manifest.display());
        return ExitCode::Failure;
    }
    let name = invocation
        .arguments
        .first()
        .cloned()
        .or_else(|| root.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())))
        .unwrap_or_else(|| "project".to_string());
    let text = format!("{{\n  \"name\": {}\n}}\n", serde_json::Value::String(name));
    if let Err(error) = fs::create_dir_all(root.join("def")).and_then(|()| fs::write(&manifest, text)) {
        eprintln!("{}: {error}", manifest.display());
        return ExitCode::Failure;
    }
    println!("created {}", manifest.display());
    ExitCode::Success
}

/// A diagnostic is printed as the file, line, column, stage, severity, and message.
// @lfy def/cli/main.lfy:main
fn print_diagnostic(diagnostic: &Diagnostic, json: bool) {
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
        println!(
            "{}:{}:{}: {} {}: {}",
            diagnostic.range.file,
            diagnostic.range.start.line,
            diagnostic.range.start.column,
            diagnostic.stage,
            diagnostic.severity,
            diagnostic.message
        );
    }
}

/// Load the root, then every diagnostic printed, or of the files given only; the code is
/// problems when any is an error and success otherwise.
// @lfy def/cli/main.lfy:main
fn check(invocation: &Invocation) -> ExitCode {
    let workspace = workspace::load(Path::new(&invocation.root));
    let json = invocation.flag("json");
    let diagnostics = collect_diagnostics(&workspace, &invocation.arguments);
    for diagnostic in &diagnostics {
        print_diagnostic(diagnostic, json);
    }
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        ExitCode::Problems
    } else {
        if !json {
            println!("{} files, no problems", workspace.files.len());
        }
        ExitCode::Success
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
                eprintln!("{display}: {error}");
                code = ExitCode::Failure;
                continue;
            }
        };
        let tokens = match lex(&source, Some(&display)) {
            Ok(tokens) => tokens,
            Err(error) => {
                eprintln!("{display}: {error}");
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
                println!("{display}:{line}:{column}: parser error: expected [{}] but found {:?}", error.expected.join(", "), tree.raw(error.start, error.end));
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
            if json {
                println!("{}", serde_json::json!({ "file": display, "changed": true }));
            } else {
                println!("{display}");
            }
            code = ExitCode::Problems;
        } else if let Err(error) = fs::write(&path, formatted) {
            eprintln!("{display}: {error}");
            code = ExitCode::Failure;
        } else if !json {
            println!("formatted {display}");
        }
    }
    code
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
    let [path] = invocation.arguments.as_slice() else {
        eprintln!("usage: elfie {} <file>", invocation.command.value());
        return Err(ExitCode::Usage);
    };
    match fs::read_to_string(path) {
        Ok(source) => Ok((path.clone(), source)),
        Err(error) => {
            eprintln!("{path}: {error}");
            Err(ExitCode::Failure)
        }
    }
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
            eprintln!("{error}");
            return ExitCode::Problems;
        }
    };
    let tree = parse(tokens, None);
    let mut out = io::BufWriter::new(io::stdout().lock());
    let _ = out.write_all(tree.render().as_bytes());
    for error in &tree.errors {
        let token = tree.tokens.get(error.start).or(tree.tokens.last());
        let (line, column) = token.map_or((0, 0), |t| (t.line, t.column));
        let _ = writeln!(out, "error at {path}:{line}:{column}: expected [{}] but found {:?}", error.expected.join(", "), tree.raw(error.start, error.end));
    }
    let _ = out.flush();
    if tree.errors.is_empty() { ExitCode::Success } else { ExitCode::Problems }
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
            eprintln!("{error}");
            return ExitCode::Problems;
        }
    };
    let mut out = io::BufWriter::new(io::stdout().lock());
    for token in &tokens {
        let rule = token.rule.map_or("invalid", |rule| {
            use elfie_core::grammar::GrammarRule;
            rule.identifier()
        });
        let _ = writeln!(out, "{}:{}\t{rule}\t{:?}", token.line, token.column, token.value);
    }
    let _ = out.flush();
    ExitCode::Success
}

// ---- progress ---------------------------------------------------------------------

/// The heading below which a person writes the answer to a question the compiler asked.
// @lfy def/cli/main.lfy:main
const ANSWER_HEADING: &str = "## The answer";

/// One progress line: the step, the batch and unit when there are any, done over total,
/// the elapsed seconds, and the message. A reason or a question runs to several lines, so
/// the line carries only its first; the whole of it is printed and written where it
/// belongs.
// @lfy def/cli/main.lfy:main
fn text_of(progress: &Progress) -> String {
    let mut text = progress.step.name().to_string();
    if let Some(batch) = &progress.batch {
        text.push_str(&format!(" {batch}"));
    }
    if let Some(unit) = &progress.unit {
        text.push_str(&format!(" {unit}"));
    }
    text.push_str(&format!(" {}/{} {:.1}s", progress.done, progress.total, progress.elapsed));
    if let Some(first) = progress.message.lines().next() {
        text.push_str(&format!(" {first}"));
    }
    text
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

/// Prints one [`Progress`] line per step as it happens and appends it to the log.
// @lfy def/cli/main.lfy:main
struct Reporter {
    json: bool,
    started: SystemTime,
    total: usize,
    done: usize,
    logs: Vec<PathBuf>,
}

impl Reporter {
    // @lfy def/cli/main.lfy:main
    fn new(json: bool, total: usize, logs: Vec<PathBuf>) -> Reporter {
        Reporter { json, started: SystemTime::now(), total, done: 0, logs }
    }

    /// One line for one step, printed and logged.
    // @lfy def/cli/main.lfy:main
    fn report(&mut self, step: Step, batch: Option<&str>, unit: Option<&str>, message: &str) {
        let progress = Progress {
            step,
            batch: batch.map(str::to_string),
            unit: unit.map(str::to_string),
            done: self.done,
            total: self.total,
            // @lfy def/cli/main.lfy:main
            elapsed: elapsed(self.started),
            message: message.to_string(),
        };
        let line = if self.json { json_of(&progress).to_string() } else { text_of(&progress) };
        println!("{line}");
        self.append(&line);
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

/// The manifest's `compiler` command, when it names one.
// @lfy def/cli/main.lfy:main
fn compiler_command(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join("elfie.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("compiler")?.as_str().map(str::to_string)
}

/// The source maps of each target, read from source-map.json joined to its output
/// directory, none when the file is missing, dropping any whose output no longer exists.
// @lfy def/cli/main.lfy:main
fn load_source_maps(workspace: &Workspace) -> Vec<SourceMap> {
    let mut maps = Vec::new();
    let mut seen = BTreeSet::new();
    for target in &workspace.targets {
        if !seen.insert(target.output_directory.clone()) {
            continue;
        }
        let path = workspace.root.join(&target.output_directory).join("source-map.json");
        for map in generation::read_source_maps(&path) {
            if workspace.root.join(&map.output).exists() {
                maps.push(map);
            }
        }
    }
    maps
}

/// The source maps written back, grouped by the output directory of the target each is
/// for.
// @lfy def/cli/main.lfy:main
fn save_source_maps(workspace: &Workspace, maps: &[SourceMap]) -> io::Result<()> {
    let mut by_directory: BTreeMap<String, Vec<SourceMap>> = BTreeMap::new();
    for map in maps {
        let directory = workspace
            .targets
            .iter()
            .find(|t| t.identifier == map.target)
            .map(|t| t.output_directory.clone())
            .unwrap_or_else(|| workspace.output_directory.clone());
        by_directory.entry(directory).or_default().push(map.clone());
    }
    for (directory, maps) in by_directory {
        let path = workspace.root.join(&directory).join("source-map.json");
        fs::create_dir_all(workspace.root.join(&directory))?;
        generation::write_source_maps(&path, &maps)?;
    }
    Ok(())
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

/// The source at the last accepted generation, recovered from git when the hash of a
/// recent revision of the file matches the unit's source map.
// @lfy def/cli/main.lfy:main
fn previous_source(workspace: &Workspace, unit: &Unit) -> Option<String> {
    let map = unit.outputs.first()?;
    let file = &workspace.files[unit.file].path;
    let log = Process::new("git")
        .args(["log", "--format=%H", "-n", "50", "--", file])
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
        if generation::source_hash(&text) == map.hash {
            return Some(text);
        }
    }
    None
}

/// One unit as `elfie_units` lists it: the target, the stem, the reason or up to date,
/// and the stems of its dependencies.
// @lfy def/cli/main.lfy:main
fn unit_line(workspace: &Workspace, plan: &Plan, index: usize) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&d| plan.units[d].stem.as_str()).collect();
    format!("{} {} {reason} [{}]", workspace.targets[unit.target].identifier, unit.stem, dependencies.join(", "))
}

/// One batch as `elfie_units` lists it: its identifier and the stems it holds.
// @lfy def/cli/main.lfy:main
fn batch_line(plan: &Plan, batch: &Batch) -> String {
    format!("batch {} [{}]", batch.identifier, stems_of(plan, batch, ", "))
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
fn dry_run(workspace: &Workspace, plan: &Plan, target: Option<&str>, json: bool) -> ExitCode {
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
            println!("{}", unit_line(workspace, plan, index));
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
            println!("{}", batch_line(plan, batch));
        }
    }
    ExitCode::Success
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
fn watch<R: io::Read + Send + 'static>(pipe: R, batch: &str, keep: bool, to_stdout: bool) -> JoinHandle<String> {
    let prefix = batch.to_string();
    std::thread::spawn(move || {
        let mut kept = String::new();
        for line in io::BufReader::new(pipe).lines().map_while(Result::ok) {
            if to_stdout {
                println!("{prefix}| {line}");
            } else {
                eprintln!("{prefix}| {line}");
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
/// the finished line reports, and the units a stopped batch took down with it.
// @lfy def/cli/main.lfy:main
struct Run<'w> {
    workspace: &'w Workspace,
    plan: Plan,
    maps: Vec<SourceMap>,
    reporter: Reporter,
    json: bool,
    continuing: bool,
    halted: bool,
    accepted: usize,
    rejected: usize,
    blocked: usize,
    failed: usize,
    stopped: BTreeSet<usize>,
    incomplete: Vec<String>,
    code: ExitCode,
}

impl<'w> Run<'w> {
    // @lfy def/cli/main.lfy:main
    fn new(workspace: &'w Workspace, plan: Plan, maps: Vec<SourceMap>, reporter: Reporter, invocation: &Invocation) -> Run<'w> {
        Run {
            workspace,
            plan,
            maps,
            reporter,
            json: invocation.flag("json"),
            continuing: invocation.flag("continue"),
            halted: false,
            accepted: 0,
            rejected: 0,
            blocked: 0,
            failed: 0,
            stopped: BTreeSet::new(),
            incomplete: Vec::new(),
            code: ExitCode::Success,
        }
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
        requests_directory(&self.workspace.root)
    }

    /// Writes `elfie-requests/<batch>.<suffix>` under the root.
    // @lfy def/cli/main.lfy:main
    fn write_note(&self, batch: &Batch, suffix: &str, text: &str) -> Option<PathBuf> {
        let directory = self.requests_directory();
        let path = directory.join(format!("{}.{suffix}", file_name_of(&batch.identifier)));
        match fs::create_dir_all(&directory).and_then(|()| fs::write(&path, text)) {
            Ok(()) => Some(path),
            Err(error) => {
                eprintln!("{}: {error}", path.display());
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
                fs::read_to_string(self.workspace.root.join(&map.output))
                    .ok()
                    .map(|text| Output { path: map.output.clone(), text })
            })
            .collect();
        // @lfy def/cli/main.lfy:main
        let mut previous = BTreeMap::new();
        for &index in &batch.units {
            let unit = &self.plan.units[index];
            if let Some(text) = previous_source(self.workspace, unit) {
                previous.insert(self.workspace.files[unit.file].path.clone(), text);
            }
        }
        let mut request = generation::request(self.workspace, &self.plan, batch, &existing, &previous);
        // The person answers by editing the definitions, or by writing the answer below
        // the question, which the next compile of the batch appends to its instructions.
        // @lfy def/cli/main.lfy:main
        if let Some(answer) = self.answer_of(batch) {
            request.instructions.push_str(&format!("\n\n## The answer to the question asked before\n\n{answer}\n"));
        }
        request
    }

    /// Streams the compiler once for one batch: the instructions as its input, the root as
    /// its directory, and ELFIE_ROOT, ELFIE_BATCH, and ELFIE_UNITS as its environment. Its
    /// standard output is the report. The code is -1 when the command could not be started.
    // @lfy def/cli/main.lfy:main
    fn run_compiler(&self, command: &str, root: &Path, batch: &Batch, instructions: &str) -> Exit {
        let spawned = Process::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(root)
            .env("ELFIE_ROOT", root)
            .env("ELFIE_BATCH", &batch.identifier)
            .env("ELFIE_UNITS", stems_of(&self.plan, batch, " "))
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
        // @lfy def/cli/main.lfy:main
        let out = child.stdout.take().map(|pipe| watch(pipe, &batch.identifier, true, !self.json));
        let err = child.stderr.take().map(|pipe| watch(pipe, &batch.identifier, true, false));
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(instructions.as_bytes())
        {
            self.reporter.append(&format!("the compiler command did not read its input: {error}"));
        }
        let status = child.wait();
        let stdout = out.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        let stderr = err.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        let status = match status {
            Ok(status) => status,
            Err(error) => return Exit { code: -1, stdout, stderr: error.to_string() },
        };
        if !status.success() {
            self.reporter.append(&format!("the compiler command exited with {status}"));
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
            let outputs = outputs_of(self.workspace, &self.plan, unit);
            verdicts.push(generation::accept(self.workspace, &self.plan, request, unit, &outputs));
        }
        verdicts
    }

    /// One unit's outputs written back, its source maps replacing the unit's, and the unit
    /// counted as done.
    // @lfy def/cli/main.lfy:main
    fn record(&mut self, unit: usize, verdict: &Verdict) -> io::Result<()> {
        for output in &verdict.outputs {
            let path = self.workspace.root.join(&output.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, &output.text)?;
        }
        let target = self.workspace.targets[self.plan.units[unit].target].identifier.clone();
        let source = self.workspace.files[self.plan.units[unit].file].path.clone();
        self.maps.retain(|map| !(map.target == target && map.source == source));
        self.maps.extend(verdict.source_maps.iter().cloned());
        self.accepted += 1;
        self.reporter.done = self.accepted;
        Ok(())
    }

    /// Every unit of an accepted batch recorded, and the source maps saved.
    // @lfy def/cli/main.lfy:main
    fn record_batch(&mut self, batch: &Batch, verdicts: &[Verdict]) {
        for (&unit, verdict) in batch.units.iter().zip(verdicts) {
            let stem = self.plan.units[unit].stem.clone();
            if let Err(error) = self.record(unit, verdict) {
                eprintln!("{stem}: {error}");
                self.worsen(ExitCode::Failure);
                continue;
            }
            let written = format!("{} outputs", verdict.outputs.len());
            self.reporter.report(Step::Accepted, Some(&batch.identifier), Some(&stem), &written);
        }
        if let Err(error) = save_source_maps(self.workspace, &self.maps) {
            eprintln!("source-map.json: {error}");
            self.worsen(ExitCode::Failure);
        }
    }

    /// The problems of every rejected unit, printed as one line each.
    // @lfy def/cli/main.lfy:main
    fn report_rejections(&mut self, batch: &Batch, verdicts: &[Verdict]) -> usize {
        let mut rejected = 0;
        for (&unit, verdict) in batch.units.iter().zip(verdicts) {
            if verdict.accepted {
                continue;
            }
            rejected += 1;
            let stem = self.plan.units[unit].stem.clone();
            for problem in &verdict.problems {
                self.reporter.report(Step::Rejected, Some(&batch.identifier), Some(&stem), problem);
            }
        }
        rejected
    }

    /// Runs the compiler on one batch, at most twice, and acts on the outcome.
    // @lfy def/cli/main.lfy:main
    fn compile_batch(&mut self, batch: &Batch, command: &str, root: &Path) {
        let stems = stems_of(&self.plan, batch, " ");
        self.reporter.report(Step::Requesting, Some(&batch.identifier), None, &stems);
        let mut request = self.request_of(batch);
        let mut attempt = 0;
        loop {
            attempt += 1;
            let step = if attempt == 1 { Step::Compiling } else { Step::Retrying };
            self.reporter.report(step, Some(&batch.identifier), None, command);
            let exit = self.run_compiler(command, root, batch, &request.instructions);
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
                // Each unit's outputs are written back, its source maps replace the unit's,
                // and the batch counts as done.
                // @lfy def/cli/main.lfy:main
                OutcomeKind::Accepted => {
                    self.record_batch(batch, &outcome.verdicts);
                    break;
                }
                // The problems are printed, and the batch is run once more with them
                // appended to the instructions; a second rejection stops the compile.
                // @lfy def/cli/main.lfy:main
                OutcomeKind::Rejected => {
                    let rejected = self.report_rejections(batch, &outcome.verdicts);
                    if attempt >= 2 {
                        self.rejected += rejected;
                        self.worsen(ExitCode::Problems);
                        self.stop(batch);
                        break;
                    }
                    request.instructions.push_str("\n\n## Problems with the previous attempt\n\n");
                    for verdict in &outcome.verdicts {
                        for problem in &verdict.problems {
                            request.instructions.push_str(&format!("- {problem}\n"));
                        }
                    }
                }
                // The reason is printed and written, and the compile stops.
                // @lfy def/cli/main.lfy:main
                OutcomeKind::Blocked => {
                    self.blocked += 1;
                    self.reporter.report(Step::Blocked, Some(&batch.identifier), None, &outcome.message);
                    eprintln!("{}", outcome.message);
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
                    eprintln!("{}", outcome.message);
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
                    eprintln!("{}", outcome.message);
                    if attempt >= 2 {
                        self.failed += 1;
                        self.worsen(ExitCode::Failure);
                        self.stop(batch);
                        break;
                    }
                }
            }
        }
    }

    /// No compiler is run; the outputs already on disk are checked, written back
    /// normalized, and recorded when accepted.
    // @lfy def/cli/main.lfy:main
    fn accept_on_disk(&mut self, batch: &Batch) {
        let request = self.request_of(batch);
        let verdicts = self.verdicts_of(batch, &request);
        let accepted: Vec<Verdict> = verdicts.iter().filter(|v| v.accepted).cloned().collect();
        if !accepted.is_empty() {
            let only = Batch {
                units: batch.units.iter().copied().zip(&verdicts).filter(|(_, v)| v.accepted).map(|(unit, _)| unit).collect(),
                identifier: batch.identifier.clone(),
            };
            self.record_batch(&only, &accepted);
        }
        let rejected = self.report_rejections(batch, &verdicts);
        if rejected > 0 {
            self.rejected += rejected;
            self.worsen(ExitCode::Problems);
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
            println!("wrote {}", relative(&self.workspace.root, &path));
        }
    }

    /// The last line: the count accepted, rejected, blocked, and failed, and every batch
    /// that did not complete.
    // @lfy def/cli/main.lfy:main
    fn finish(&mut self) {
        let mut message = format!(
            "{} units accepted, {} rejected, {} batches blocked, {} failed",
            self.accepted, self.rejected, self.blocked, self.failed
        );
        if !self.incomplete.is_empty() {
            message.push_str(&format!("; did not complete: {}", self.incomplete.join(", ")));
        }
        self.reporter.report(Step::Finished, None, None, &message);
    }
}

/// Plans the units, runs the compiler on each batch in plan order, and records what it
/// produced.
// @lfy def/cli/main.lfy:main
fn compile(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    // The compiler is never handed a program with problems.
    // @lfy def/cli/main.lfy:main
    let diagnostics = query::diagnostics_of(&workspace, None);
    let errors: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    if !errors.is_empty() {
        for diagnostic in errors {
            print_diagnostic(diagnostic, json);
        }
        return ExitCode::Problems;
    }
    // @lfy def/cli/main.lfy:main
    let maps = load_source_maps(&workspace);
    let mut plan = generation::plan(&workspace, &maps, &invocation.arguments);
    if invocation.flag("all") {
        let every: Vec<String> = plan.units.iter().map(|unit| unit.stem.clone()).collect();
        plan = generation::plan(&workspace, &maps, &every);
    }
    let target = invocation.option("target").map(str::to_string);
    if let Some(name) = &target
        && !workspace.targets.iter().any(|known| &known.identifier == name)
    {
        let known: Vec<&str> = workspace.targets.iter().map(|t| t.identifier.as_str()).collect();
        eprintln!("{name} is not a target; the targets are: {}", known.join(", "));
        return ExitCode::Usage;
    }
    // @lfy def/cli/main.lfy:main
    if invocation.flag("dry-run") {
        return dry_run(&workspace, &plan, target.as_deref(), json);
    }
    let batches: Vec<usize> = (0..plan.batches.len())
        .filter(|&index| target.as_deref().is_none_or(|name| batch_target(&workspace, &plan, &plan.batches[index]) == name))
        .collect();
    let total: usize = batches.iter().map(|&index| plan.batches[index].units.len()).sum();
    if total == 0 {
        if !json {
            println!("every unit is up to date");
        }
        return ExitCode::Success;
    }
    let reporter = Reporter::new(json, total, log_paths(root));
    let mut run = Run::new(&workspace, plan, maps, reporter, invocation);
    // The first line is planned, with the count of units planned and of batches.
    // @lfy def/cli/main.lfy:main
    let planned = format!("{total} units planned, {} batches", batches.len());
    run.reporter.report(Step::Planned, None, None, &planned);

    let compiler = compiler_command(root);
    let accept_only = invocation.flag("accept");
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
            run.accept_on_disk(&batch); // @lfy def/cli/main.lfy:main
        } else if let Some(command) = &compiler {
            run.compile_batch(&batch, command, root); // @lfy def/cli/main.lfy:main
        } else {
            run.write_request(&batch); // @lfy def/cli/main.lfy:main
        }
    }
    run.finish();
    run.code
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
        assert_eq!(parse_arguments(&["--version".to_string()]).unwrap().command, Command::Version);
        assert_eq!(parse_arguments(&[]).unwrap().command, Command::Help);
    }

    /// A usage message is printed with the help text and the code is usage.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn a_usage_error_returns_the_usage_code() {
        assert_eq!(run(&["frobnicate".to_string()]), ExitCode::Usage.code());
    }

    /// Help prints every command with one line of description and the global options, and
    /// version prints the version; both return success.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn help_and_version_return_success() {
        let text = help_text();
        for command in Command::ALL {
            assert!(text.contains(command.value()), "{text}");
            assert!(text.contains(command.description()), "{text}");
        }
        assert!(text.contains("--root") && text.contains("--json"), "{text}");
        assert_eq!(help(), ExitCode::Success);
        // @lfy def/cli/main.lfy:main
        assert_eq!(version(), ExitCode::Success);
    }

    /// Init creates elfie.json and the source directory, and fails when elfie.json
    /// already exists.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn init_creates_the_manifest_once() {
        let fixture = Fixture::new();
        assert_eq!(fixture.run(&["init", "p"]), ExitCode::Success.code());
        assert!(fixture.read("elfie.json").contains("\"p\""));
        assert!(fixture.root.join("def").is_dir());
        assert_eq!(fixture.run(&["init", "p"]), ExitCode::Failure.code());
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
        let printed =
            format!("{}:{}:{}: {} {}", mine[0].range.file, mine[0].range.start.line, mine[0].range.start.column, mine[0].stage, mine[0].severity);
        // @lfy def/cli/main.lfy:main
        assert_eq!(printed, "def/a.lfy:1:10: binder error");
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

    /// Compile with --dry-run prints one unit with reason fresh and one batch holding it,
    /// generates nothing, and returns success.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn dry_run_prints_the_units_then_the_batches() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        let workspace = workspace::load(&fixture.root);
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let plan = generation::plan(&workspace, &load_source_maps(&workspace), &[]);
        assert_eq!(plan.units.len(), 1);
        assert_eq!(unit_line(&workspace, &plan, 0), "rust a fresh []");
        // @lfy def/cli/main.lfy:main
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(batch_line(&plan, &plan.batches[0]), "batch a [a]");
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
        // Every progress line is appended to the log. @lfy def/cli/main.lfy:main
        let log = fixture.read("elfie-requests/compile.log");
        assert!(log.contains("planned 0/1"), "{log}");
        assert!(log.contains("requesting a 0/1"), "{log}");
        assert!(log.contains("finished 0/1"), "{log}");
        // Nothing of the CLI's own is left under the output directory.
        // @lfy def/cli/main.lfy:main
        assert!(!fixture.root.join("out/elfie-requests").exists());
    }

    /// The first line is planned with the counts, and the last is finished with the count
    /// accepted, rejected, blocked, and failed.
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
        // @lfy def/cli/main.lfy:main
        assert_eq!(text_of(&progress), "accepted a+1 cli/main 2/7 12.3s 3 outputs");
        let value = json_of(&progress);
        assert_eq!(value["step"], "accepted");
        assert_eq!(value["batch"], "a+1");
        assert_eq!(value["unit"], "cli/main");
        assert_eq!(value["done"], 2);
        assert_eq!(value["total"], 7);
        assert_eq!(value["message"], "3 outputs\nand more");
        let planned = Progress {
            step: Step::Planned,
            batch: None,
            unit: None,
            done: 0,
            total: 7,
            elapsed: 0.0,
            message: "7 units planned, 3 batches".to_string(),
        };
        assert_eq!(text_of(&planned), "planned 0/7 0.0s 7 units planned, 3 batches");
        assert_eq!(json_of(&planned)["batch"], serde_json::Value::Null);
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
        let workspace = workspace::load(&fixture.root);
        let plan = generation::plan(&workspace, &load_source_maps(&workspace), &[]);
        let reporter = Reporter::new(false, 1, Vec::new());
        let invocation = parse_arguments(&["compile".to_string()]).unwrap();
        let run = Run::new(&workspace, plan, Vec::new(), reporter, &invocation);
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
        let workspace = workspace::load(&fixture.root);
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let plan = generation::plan(&workspace, &load_source_maps(&workspace), &[]);
        let reporter = Reporter::new(false, plan.units.len(), Vec::new());
        let invocation = parse_arguments(&["compile".to_string(), "--continue".to_string()]).unwrap();
        let mut run = Run::new(&workspace, plan, Vec::new(), reporter, &invocation);
        assert!(run.continuing);
        // The batch holding b is stopped; a depends on b, c does not.
        let of = |run: &Run, stem: &str| run.plan.units.iter().position(|unit| unit.stem == stem).unwrap();
        let (a, b, c) = (of(&run, "a"), of(&run, "b"), of(&run, "c"));
        run.stop(&Batch { units: vec![b], identifier: "b".to_string() });
        assert!(!run.halted, "--continue keeps the independent batches running");
        assert!(run.depends_on_stopped(&Batch { units: vec![a], identifier: "a".to_string() }));
        assert!(!run.depends_on_stopped(&Batch { units: vec![c], identifier: "c".to_string() }));
        // Without --continue a stopped batch stops the compile.
        let invocation = parse_arguments(&["compile".to_string()]).unwrap();
        let mut run = Run::new(&workspace, run.plan.clone(), Vec::new(), Reporter::new(false, 1, Vec::new()), &invocation);
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
        let workspace = workspace::load(&fixture.root);
        let plan = generation::plan(&workspace, &load_source_maps(&workspace), &[]);
        let invocation = parse_arguments(&["compile".to_string()]).unwrap();
        let mut run = Run::new(&workspace, plan, Vec::new(), Reporter::new(false, 1, Vec::new()), &invocation);
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
        let maps = fixture.read("out/source-map.json");
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
        assert!(fixture.read("out/source-map.json").contains("\"out/a.rs\""));
        // The report is appended to the log with every progress line.
        // @lfy def/cli/main.lfy:main
        let log = fixture.read("elfie-requests/compile.log");
        assert!(log.contains("done"), "{log}");
        assert!(log.contains("compiling a 0/1"), "{log}");
        assert!(log.contains("accepted a a 1/1"), "{log}");
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
        let workspace = workspace::load(&fixture.root);
        let plan = generation::plan(&workspace, &load_source_maps(&workspace), &[]);
        let invocation = parse_arguments(&["compile".to_string()]).unwrap();
        let mut run = Run::new(&workspace, plan, Vec::new(), Reporter::new(false, 1, Vec::new()), &invocation);
        let batch = run.plan.batches[0].clone();
        run.compile_batch(&batch, "true", Path::new("/no/such/directory"));
        assert_eq!(run.code, ExitCode::Failure);
        assert_eq!(run.failed, 1);
        assert!(run.halted);
    }
}
