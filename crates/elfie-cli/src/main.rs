//! Compiled from `def/cli/main.lfy`: the `elfie` command line.
//!
//! Every command is a thin driver over the stage it belongs to, and the process exits
//! with a code from [`ExitCode`] so agents and scripts can read the outcome without
//! parsing text.

mod data;

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};

use elfie_core::generation::{self, Output, SourceMap};
use elfie_core::lexer::lex;
use elfie_core::parser::parse;
use elfie_core::query::{self, Diagnostic, Severity};
use elfie_core::workspace::{self, Workspace};

pub use data::{Command, ExitCode, Invocation};

fn main() -> std::process::ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    std::process::ExitCode::from(run(&arguments))
}

/// The invocation the arguments spell, or the usage error they make.
// @lfy def/cli/main.lfy:16
pub fn parse_arguments(arguments: &[String]) -> Result<Invocation, String> {
    let mut command = None;
    let mut root = None;
    let mut positional = Vec::new();
    let mut options: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut i = 0;
    while i < arguments.len() {
        let argument = &arguments[i];
        // --help or -h anywhere means help; --version means version.
        // @lfy def/cli/main.lfy:18
        if argument == "--help" || argument == "-h" {
            return Ok(Invocation { command: Command::Help, root: find_root(None), arguments: Vec::new(), options });
        }
        if argument == "--version" {
            return Ok(Invocation { command: Command::Version, root: find_root(None), arguments: Vec::new(), options });
        }
        // --tree <file> keeps working as the tree command.
        // @lfy def/cli/main.lfy:20
        if argument == "--tree" {
            command = Some(Command::Tree);
            i += 1;
            continue;
        }
        // --root <dir> or --root=<dir> sets the root.
        // @lfy def/cli/main.lfy:19
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
        // @lfy def/cli/main.lfy:21
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
        // @lfy def/cli/main.lfy:18
        if command.is_none() {
            match Command::lookup(argument) {
                Some(found) => command = Some(found),
                None => return Err(format!("{argument} is not a command")), // @lfy def/cli/main.lfy:22
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
const OPTIONS_WITH_VALUES: [&str; 1] = ["target"];

/// The nearest directory at or above the current one holding elfie.json, or the current
/// one.
// @lfy def/cli/main.lfy:19
fn find_root(given: Option<&str>) -> String {
    if let Some(given) = given {
        return given.to_string();
    }
    let mut current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let start = current.clone();
    loop {
        if current.join("elfie.json").is_file() {
            return current.to_string_lossy().into_owned();
        }
        if !current.pop() {
            return start.to_string_lossy().into_owned();
        }
    }
}

/// Run one command and return its exit code.
// @lfy def/cli/main.lfy:29
pub fn run(arguments: &[String]) -> u8 {
    let invocation = match parse_arguments(arguments) {
        Ok(invocation) => invocation,
        Err(message) => {
            // A usage message is printed with the help text and the code is usage.
            // @lfy def/cli/main.lfy:33
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
        Command::Lsp => ExitCode::from_code(elfie_lsp::serve(Some(Path::new(&invocation.root)))),
        Command::Mcp => ExitCode::from_code(elfie_mcp::serve(Some(Path::new(&invocation.root)))),
    };
    code.code()
}

impl ExitCode {
    fn from_code(code: i32) -> ExitCode {
        match code {
            0 => ExitCode::Success,
            1 => ExitCode::Problems,
            2 => ExitCode::Usage,
            _ => ExitCode::Failure,
        }
    }
}

fn help_text() -> String {
    let mut out = String::from("usage: elfie <command> [arguments] [--root <dir>] [--json]\n\ncommands:\n");
    for command in Command::ALL {
        out.push_str(&format!("  {:<8} {}\n", command.value(), command.description()));
    }
    out.push_str("\noptions:\n  --root <dir>   The project directory (default: the nearest elfie.json above the current directory)\n  --json         Print results as JSON lines\n  --help, -h     Print this help\n  --version      Print the version\n");
    out
}

/// Prints every command with one line of description and the global options.
// @lfy def/cli/main.lfy:38
fn help() -> ExitCode {
    print!("{}", help_text());
    ExitCode::Success
}

// @lfy def/cli/main.lfy:39
fn version() -> ExitCode {
    println!("elfie {}", env!("CARGO_PKG_VERSION"));
    ExitCode::Success
}

/// Creates elfie.json in the root naming the project after the argument, or the
/// directory, and creates the source directory; fails when elfie.json already exists.
// @lfy def/cli/main.lfy:43
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
// @lfy def/cli/main.lfy:35
fn print_diagnostic(diagnostic: &Diagnostic, json: bool) {
    if json {
        let value = serde_json::json!({
            "file": diagnostic.range.file,
            "line": diagnostic.range.start.line,
            "column": diagnostic.range.start.column,
            "stage": format!("{:?}", diagnostic.stage).to_lowercase(),
            "severity": format!("{:?}", diagnostic.severity).to_lowercase(),
            "message": diagnostic.message,
        });
        println!("{value}");
    } else {
        println!(
            "{}:{}:{}: {} {}: {}",
            diagnostic.range.file,
            diagnostic.range.start.line,
            diagnostic.range.start.column,
            format!("{:?}", diagnostic.stage).to_lowercase(),
            format!("{:?}", diagnostic.severity).to_lowercase(),
            diagnostic.message
        );
    }
}

/// Load the root, then every diagnostic printed, or of the files given only; the code
/// is problems when any is an error.
// @lfy def/cli/main.lfy:47
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

fn collect_diagnostics(workspace: &Workspace, files: &[String]) -> Vec<Diagnostic> {
    if files.is_empty() {
        query::diagnostics_of(workspace, None)
    } else {
        files.iter().flat_map(|file| query::diagnostics_of(workspace, Some(file))).collect()
    }
}

/// Each file given, or every .lfy file under the source directory, is replaced by the
/// standard layout when that differs; --check only reports.
// @lfy def/cli/main.lfy:51
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
        let display = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
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
            // @lfy def/cli/main.lfy:53
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
// @lfy def/cli/main.lfy:57
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
// @lfy def/cli/main.lfy:58
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

// ---- compile ----------------------------------------------------------------------

/// The manifest's `compiler` command, when it names one.
fn compiler_command(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join("elfie.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("compiler")?.as_str().map(str::to_string)
}

/// The source maps of each target, read from source-map.json in its output directory,
/// dropping any whose output no longer exists.
// @lfy def/cli/main.lfy:63
fn load_source_maps(workspace: &Workspace) -> Vec<SourceMap> {
    let mut maps = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for target in &workspace.targets {
        if !seen.insert(target.output_directory.clone()) {
            continue;
        }
        let path = workspace.root.join(&target.output_directory).join("source-map.json");
        for map in generation::read_source_maps(&path) {
            if workspace.root.join(&map.output).is_file() {
                maps.push(map);
            }
        }
    }
    maps
}

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

/// The outputs under the target's output directory that carry a marker for the unit's
/// file.
fn outputs_of(workspace: &Workspace, plan: &generation::Plan, unit: usize) -> Vec<Output> {
    let unit = &plan.units[unit];
    let target = &workspace.targets[unit.target];
    let file = &workspace.files[unit.file].path;
    let mut found = Vec::new();
    let mut paths = Vec::new();
    walk_all(&workspace.root.join(&target.output_directory), &mut paths);
    paths.sort();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if generation::parse_markers(&text).iter().any(|m| &m.file == file) {
            let relative = path.strip_prefix(&workspace.root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            found.push(Output { path: relative, text });
        }
    }
    found
}

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
// @lfy def/cli/main.lfy:66
fn previous_source(workspace: &Workspace, unit: &generation::Unit) -> Option<String> {
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

fn unit_line(workspace: &Workspace, plan: &generation::Plan, index: usize) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |r| format!("{r:?}").to_lowercase());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&d| plan.units[d].stem.as_str()).collect();
    format!("{} {} {reason} [{}]", workspace.targets[unit.target].identifier, unit.stem, dependencies.join(", "))
}

/// Plans the units and runs the compiler command on each planned one in order.
// @lfy def/cli/main.lfy:62
fn compile(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    // The compiler is never handed a program with problems.
    // @lfy def/cli/main.lfy:62
    let diagnostics = query::diagnostics_of(&workspace, None);
    let errors: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    if !errors.is_empty() {
        for diagnostic in errors {
            print_diagnostic(diagnostic, json);
        }
        return ExitCode::Problems;
    }
    let maps = load_source_maps(&workspace);
    let mut requested: Vec<String> = invocation.arguments.clone();
    let mut plan = generation::plan(&workspace, &maps, &requested);
    if invocation.flag("all") {
        requested = plan.units.iter().map(|u| u.stem.clone()).collect();
        plan = generation::plan(&workspace, &maps, &requested);
    }
    let target_filter = invocation.option("target").map(str::to_string);
    if let Some(target) = &target_filter {
        if !workspace.targets.iter().any(|t| &t.identifier == target) {
            eprintln!("{target} is not a target; known: {}", workspace.targets.iter().map(|t| t.identifier.as_str()).collect::<Vec<_>>().join(", "));
            return ExitCode::Usage;
        }
    }
    let selected: Vec<usize> = (0..plan.units.len())
        .filter(|&i| target_filter.as_ref().is_none_or(|t| &workspace.targets[plan.units[i].target].identifier == t))
        .collect();
    // --dry-run prints each unit and generates nothing.
    // @lfy def/cli/main.lfy:64
    if invocation.flag("dry-run") {
        for &index in &selected {
            if json {
                let unit = &plan.units[index];
                println!(
                    "{}",
                    serde_json::json!({
                        "target": workspace.targets[unit.target].identifier,
                        "stem": unit.stem,
                        "file": workspace.files[unit.file].path,
                        "reason": unit.reason.map(|r| format!("{r:?}").to_lowercase()),
                        "dependencies": unit.dependencies.iter().map(|&d| plan.units[d].stem.clone()).collect::<Vec<_>>(),
                        "entities": unit.entities.iter().map(|&e| workspace.model.entities[e].identifier.clone()).collect::<Vec<_>>(),
                    })
                );
            } else {
                println!("{}", unit_line(&workspace, &plan, index));
            }
        }
        return ExitCode::Success;
    }
    let planned: Vec<usize> = selected.into_iter().filter(|&i| plan.units[i].reason.is_some()).collect();
    if planned.is_empty() {
        if !json {
            println!("every unit is up to date");
        }
        return ExitCode::Success;
    }
    let compiler = compiler_command(root);
    let mut maps = maps;
    for index in planned {
        let unit = plan.units[index].clone();
        let existing: Vec<Output> = unit
            .outputs
            .iter()
            .filter_map(|map| fs::read_to_string(root.join(&map.output)).ok().map(|text| Output { path: map.output.clone(), text }))
            .collect();
        let previous = previous_source(&workspace, &unit);
        let mut request = generation::request(&workspace, &plan, index, &existing, previous.as_deref());
        let Some(command) = &compiler else {
            // No compiler: the request of each planned unit is written for a compiler run
            // by hand.
            // @lfy def/cli/main.lfy:69
            let directory = root.join(&workspace.targets[unit.target].output_directory).join("elfie-requests");
            let path = directory.join(format!("{}.md", unit.stem.replace('/', "-")));
            if let Err(error) = fs::create_dir_all(&directory).and_then(|()| fs::write(&path, &request.instructions)) {
                eprintln!("{}: {error}", path.display());
                return ExitCode::Failure;
            }
            if !json {
                println!("wrote {}", path.strip_prefix(root).unwrap_or(&path).display());
            }
            continue;
        };
        if !json {
            println!("compiling {}", unit_line(&workspace, &plan, index));
        }
        let mut attempts = 0;
        loop {
            attempts += 1;
            if let Err(error) = run_compiler(command, root, &unit, &request.instructions) {
                eprintln!("the compiler command could not be run: {error}");
                return ExitCode::Failure;
            }
            let outputs = outputs_of(&workspace, &plan, index);
            let verdict = generation::accept(&workspace, &plan, &request, &outputs);
            if verdict.accepted {
                // Its source maps replace the unit's.
                // @lfy def/cli/main.lfy:67
                let target = workspace.targets[unit.target].identifier.clone();
                let file = workspace.files[unit.file].path.clone();
                maps.retain(|m| !(m.target == target && m.source == file));
                maps.extend(verdict.source_maps.iter().cloned());
                if let Err(error) = save_source_maps(&workspace, &maps) {
                    eprintln!("source-map.json: {error}");
                    return ExitCode::Failure;
                }
                if json {
                    println!("{}", serde_json::json!({ "stem": unit.stem, "accepted": true, "outputs": outputs.iter().map(|o| o.path.clone()).collect::<Vec<_>>() }));
                } else {
                    println!("accepted {} ({} outputs)", unit.stem, outputs.len());
                }
                break;
            }
            for problem in &verdict.problems {
                if json {
                    println!("{}", serde_json::json!({ "stem": unit.stem, "accepted": false, "problem": problem }));
                } else {
                    println!("rejected {}: {problem}", unit.stem);
                }
            }
            if attempts >= 2 {
                // The second run is rejected too: the remaining units are not run.
                // @lfy def/cli/main.lfy:68
                return ExitCode::Problems;
            }
            request.instructions.push_str("\n\n## Problems with the previous attempt\n\n");
            for problem in &verdict.problems {
                request.instructions.push_str(&format!("- {problem}\n"));
            }
        }
    }
    ExitCode::Success
}

/// Runs the compiler command with the request on its standard input, the root as its
/// working directory, and ELFIE_ROOT and ELFIE_UNIT in its environment.
// @lfy def/cli/main.lfy:65
fn run_compiler(command: &str, root: &Path, unit: &generation::Unit, instructions: &str) -> io::Result<()> {
    let mut child = Process::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(root)
        .env("ELFIE_ROOT", root)
        .env("ELFIE_UNIT", &unit.stem)
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(instructions.as_bytes())?;
    }
    let status = child.wait()?;
    if !status.success() {
        eprintln!("the compiler command exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/cli/main.lfy:24
    #[test]
    fn check_with_no_arguments_parses() {
        let invocation = parse_arguments(&["check".to_string()]).unwrap();
        assert_eq!(invocation.command, Command::Check);
        assert!(invocation.arguments.is_empty());
    }

    // @lfy def/cli/main.lfy:25
    #[test]
    fn the_tree_flag_is_the_tree_command() {
        let invocation = parse_arguments(&["--tree".to_string(), "def/a.lfy".to_string()]).unwrap();
        assert_eq!(invocation.command, Command::Tree);
        assert_eq!(invocation.arguments, vec!["def/a.lfy".to_string()]);
    }

    // @lfy def/cli/main.lfy:26
    #[test]
    fn an_unknown_command_is_a_usage_error() {
        let error = parse_arguments(&["frobnicate".to_string()]).unwrap_err();
        assert!(error.contains("frobnicate"));
    }

    // @lfy def/cli/main.lfy:21
    #[test]
    fn options_take_flags_and_values() {
        let invocation = parse_arguments(&["compile".to_string(), "--dry-run".to_string(), "--target=rust".to_string(), "--root".to_string(), "/tmp".to_string(), "lexer/main".to_string()]).unwrap();
        assert!(invocation.flag("dry-run"));
        assert_eq!(invocation.option("target"), Some("rust"));
        assert_eq!(invocation.root, "/tmp");
        assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
        let invocation = parse_arguments(&["compile".to_string(), "--target".to_string(), "rust".to_string()]).unwrap();
        assert_eq!(invocation.option("target"), Some("rust"));
        assert!(parse_arguments(&["compile".to_string(), "--target".to_string()]).is_err());
    }

    // @lfy def/cli/main.lfy:18
    #[test]
    fn help_and_version_win_anywhere() {
        assert_eq!(parse_arguments(&["check".to_string(), "--help".to_string()]).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&["--version".to_string()]).unwrap().command, Command::Version);
        assert_eq!(parse_arguments(&[]).unwrap().command, Command::Help);
    }
}
