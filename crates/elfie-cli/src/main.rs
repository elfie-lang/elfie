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
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::SystemTime;

use elfie_core::generation::{
    self, Batch, Marker, Move, Outcome, OutcomeKind, Output, Plan, REVIEWED, Reason, Request, Review, ReviewRecord,
    ReviewReport, ReviewRequest, ReviewStatus, SourceMap, Unit, Verdict,
};
#[cfg(test)]
use elfie_core::generation::{Submission, SubmittedOutcome};
use elfie_core::grammar::GrammarRule as _;
use elfie_core::interpret::{LoweredNode, Program, lower};
use elfie_core::lexer::{Token, lex};
use elfie_core::model::{EntityId, Operation, Problem};
use elfie_core::parser::{Child, ErrorNode, Node, Tree, parse};
use elfie_core::query::{self, Diagnostic, Severity};
use elfie_core::workspace::{self, Target, Workspace};
// @lfy def/cli/main.lfy:main#main:main:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
// @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
use elfie_mcp::{Binding, GlobalReview, Host, Role, SessionRecord};

pub use data::{Command, ExitCode, Invocation, Progress, Step};
// @lfy def/cli/main.lfy:main
use style::{
    ColorChoice, Stream, Tone, colors_on, duration, glyph_of, padded, paint, reason_tone, severity_tone, status_tone,
    tally, tone_of, width,
};

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
        // --version names the command only when neither --help nor -h is among the arguments,
        // which the branch before this one has already taken.
        // @lfy def/cli/main.lfy:parse#parse:parse:fff69e97f1f4842d2724ff711989149786a96a351c4d05ee30c6c64f1629a3d3
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
        // @lfy def/cli/main.lfy:parse#parse:parse:a10f50e751f0b52b43514389889c366b65d704b81d4894a77a28e783fcbcebfc
        if argument == "--tree" {
            // @lfy def/cli/main.lfy:parse#parse:parse:a10f50e751f0b52b43514389889c366b65d704b81d4894a77a28e783fcbcebfc
            if arguments.get(i + 1).is_some_and(|file| !file.starts_with('-')) {
                command = Some(Command::Tree);
            } else if i + 1 == arguments.len() && forced.is_none() {
                // The last argument with nothing after it to take: the option is named back.
                // @lfy def/cli/main.lfy:parse#parse:parse:3e415cc8c432995a2f20132ebe9faa1a51e783fa11b13cdc472e6a3089c09986
                return Err("--tree needs a file".to_string());
            }
            i += 1;
            continue;
        }
        // --root <dir> or --root=<dir> sets the root.
        // @lfy def/cli/main.lfy:parse#parse:parse:3645fdc7e4fa098477e08321e6ca15068b540b9c205c6dd4dddb5f68dba4d549
        if let Some(value) = argument.strip_prefix("--root=") {
            root = Some(value.to_string());
            i += 1;
            continue;
        }
        // @lfy def/cli/main.lfy:parse#parse:parse:3645fdc7e4fa098477e08321e6ca15068b540b9c205c6dd4dddb5f68dba4d549
        if argument == "--root" {
            match arguments.get(i + 1) {
                Some(value) => {
                    root = Some(value.clone());
                    i += 2;
                }
                // The last argument with no directory after it to take: the option is named
                // back, unless --help, -h, or --version has already named the command.
                // @lfy def/cli/main.lfy:parse#parse:parse:3e415cc8c432995a2f20132ebe9faa1a51e783fa11b13cdc472e6a3089c09986
                None if forced.is_none() => return Err("--root needs a directory".to_string()),
                None => i += 1,
            }
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
                // @lfy def/cli/main.lfy:parse#parse:parse:0da4c49e8c974658ca2be342ff4ac6e0e21e09807b88714b57af4c15bca1404c
                // @lfy def/cli/main.lfy:parse#parse:parse:5df10c8ed49640a49819e6470920fbd7c525c644a77c7d9cb53141a553068141
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
                // @lfy def/cli/main.lfy:parse#parse:parse:252d8235d06b32c568f2bf1a13157b65c1788281bf471f3abe705c298312086c
                if let Some(value) = arguments.get(i + 1) {
                    // @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
                    if name == JOBS && !names_a_count(value) {
                        return Err(JOBS_USAGE.to_string());
                    }
                    options.insert(name.to_string(), Some(value.clone()));
                    i += 1;
                } else if forced.is_none() {
                    // The last argument with nothing after it to take: the option is named
                    // back, unless --help, -h, or --version has already named the command.
                    // @lfy def/cli/main.lfy:parse#parse:parse:3e415cc8c432995a2f20132ebe9faa1a51e783fa11b13cdc472e6a3089c09986
                    return Err(format!("--{name} needs a value"));
                }
            } else {
                // Every other option written with no equals sign is a flag and gives true,
                // never taking the next argument, so --json check still names the check
                // command.
                // @lfy def/cli/main.lfy:parse#parse:parse:233533aea4b9472e5d4e1d27500a3950212b9616536f3b46cf00ef040f59af88
                // @lfy def/cli/main.lfy:parse#parse:parse:2e73a4f8b63da86790f160a37ed9aa761be453dad91436a10b475b11625c450b
                options.insert(name.to_string(), None);
            }
            i += 1;
            continue;
        }
        // The first argument that does not begin with a dash names the command.
        // @lfy def/cli/main.lfy:parse#parse:parse:3874a32a6a9ce574d2416d5c0171397b2a5a6f3c2857cb5d98d62c92e1a25770
        if command.is_none() && forced.is_none() {
            match Command::lookup(argument) {
                Some(found) => command = Some(found),
                // @lfy def/cli/main.lfy:parse#parse:parse:407b80d916b66360f99967d28e89ba7016a7c488369e0a70acdef6af3936646d
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
    // Nothing that does not begin with a dash names a command, no --version is given, and no
    // option that takes a value was the last argument: the command is help.
    // @lfy def/cli/main.lfy:parse#parse:parse:5f3d8a6fb3ccffa0749573417617024cf2ac816e469eea41263c10278b25db02
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
// @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
fn compile_jobs(invocation: &Invocation, root: &Path) -> usize {
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    match git_directory(root) {
        // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
        Some(_) => jobs_of(invocation),
        // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
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
    // The directory --root named is the root, made absolute against the current directory
    // when it is relative: a batch's copy lives under the root and its compiler runs inside
    // that copy, so a relative root would name one place from the CLI and another from there.
    // @lfy def/cli/main.lfy:parse#parse:parse:3645fdc7e4fa098477e08321e6ca15068b540b9c205c6dd4dddb5f68dba4d549
    if let Some(given) = given {
        let given = Path::new(given);
        // @lfy def/cli/main.lfy:parse#parse:parse:3645fdc7e4fa098477e08321e6ca15068b540b9c205c6dd4dddb5f68dba4d549
        if given.is_absolute() {
            return given.to_string_lossy().into_owned();
        }
        // @lfy def/cli/main.lfy:parse#parse:parse:3645fdc7e4fa098477e08321e6ca15068b540b9c205c6dd4dddb5f68dba4d549
        let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        return current.join(given).to_string_lossy().into_owned();
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
const OPTIONS: [(&str, &str); 15] = [
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
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
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
    // The global options, --no-verify, --strict, --jobs, and --color among them.
    // @lfy def/cli/main.lfy:main#main:main:98492f0a3370907bed7bf6933b88094ca03a27a2437e3e01ee1770b8d88256f8
    for (name, description) in OPTIONS {
        // @lfy def/cli/main.lfy:main#main:main:95aa9ed5f0f04989f4527492db6cc9507956c521b47e77a5c366d6f36a7f357e
        let name = padded(&paint(name, Tone::Active, on), column);
        out.push_str(&format!("  {name}{description}\n"));
    }
    out
}

/// Prints every command, verify among them, with one line of description and the global
/// options, --no-verify, --strict, --jobs, and --color among them.
// @lfy def/cli/main.lfy:main#main:main:6f0073ceecb972e98aae5b1618d058d12e3264a00f6faa1982ea18e0ca49ac39
fn help(invocation: &Invocation) -> ExitCode {
    print!("{}", help_text(Paint::of(invocation).out));
    // @lfy def/cli/main.lfy:main#main:main:0bb3adb0044327ff76f406b826c6de676f76bfa10744e5c496b356815b59c852
    ExitCode::Success
}

/// Prints the version of the executable.
// @lfy def/cli/main.lfy:main#main:main:c2de1ad0a3b01e4d1d349d3ff8890849a16e58365d070af5b7ffaf7e0ddaa0e6
fn version() -> ExitCode {
    println!("elfie {}", env!("CARGO_PKG_VERSION"));
    // @lfy def/cli/main.lfy:main#main:main:053c30d9cd6d0bc161a09937054718d2692697f373604978c53abdc9440983de
    ExitCode::Success
}

/// The one line .gitignore holds for a project the CLI set up: the cache is derivable and
/// nothing else under elfie-compile is, since the maps are checked in.
// @lfy def/cli/main.lfy:main#main:main:c160a3dc4d430206c372000abe8e8c0d338ea73fab5879b08956e0b6b4d9c22f
const IGNORED: &str = "/elfie-compile/cache/";

/// Where a unit's source maps are recorded, as [`generation::map_file`] spells it; init
/// creates it so a project has it before its first compile.
///
/// The maps live beside the cache rather than in it, so nothing under
/// `elfie-compile/cache` is ever the only record of anything: a command run after the cache
/// is deleted gives the same plan, requests, verdicts, and outputs as one run with it.
// @lfy def/cli/main.lfy:main#main:main:c160a3dc4d430206c372000abe8e8c0d338ea73fab5879b08956e0b6b4d9c22f
// @lfy def/cli/main.lfy:main#main:main:bc914a1f1aeef4f635de558b0f64bd7174c6f9cab74213ce228b002ba9b8dd46
const MAPS: &str = "elfie-compile/maps";

/// Creates elfie.json in the root naming the project after the argument, or the directory,
/// creates the source directory and elfie-compile/maps, and leaves .gitignore ignoring the
/// cache; fails when elfie.json already exists.
// @lfy def/cli/main.lfy:main
fn init(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let on = Paint::of(invocation).err;
    let manifest = root.join("elfie.json");
    // @lfy def/cli/main.lfy:main#main:main:c86f43106eb489f42a5bd5dbb47ccb8ba93847faf79d747170f2d9e1eb2f8398
    if manifest.exists() {
        complain(on, Some(&manifest.display().to_string()), "already exists");
        return ExitCode::Failure;
    }
    // The project is named after the argument.
    // @lfy def/cli/main.lfy:main#main:main:614da7562f23fb56cf245bc4b5e0387a1baa11c20d41140d76a802d92bf85eb4
    // With no argument it is named after the directory.
    // @lfy def/cli/main.lfy:main#main:main:b52a1037c894592b27e7c7cb717b19757e7b4f585f26b79ecee70a3f15f89a5e
    let name = invocation
        .arguments
        .first()
        .cloned()
        .or_else(|| root.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())))
        .unwrap_or_else(|| "project".to_string());
    let text = format!("{{\n  \"name\": {}\n}}\n", serde_json::Value::String(name));
    // The source directory and elfie-compile/maps, where the maps are recorded and checked
    // in; nothing ignores elfie-compile itself or the maps.
    // @lfy def/cli/main.lfy:main#main:main:614da7562f23fb56cf245bc4b5e0387a1baa11c20d41140d76a802d92bf85eb4
    // @lfy def/cli/main.lfy:main#main:main:b52a1037c894592b27e7c7cb717b19757e7b4f585f26b79ecee70a3f15f89a5e
    // @lfy def/cli/main.lfy:main#main:main:c160a3dc4d430206c372000abe8e8c0d338ea73fab5879b08956e0b6b4d9c22f
    // @lfy def/cli/main.lfy:main#main:main:510553d27282c73545949f5567bc388144418cced5a14b208181bc0172689d15
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
    // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
    println!("{}", created_line(invocation.flag("json"), &manifest.display().to_string()));
    // @lfy def/cli/main.lfy:main#main:main:4d9a04f63d9bbc71e754bf8dce1f8279e3966e5daee5a25f23c9534fbabbe0c1
    ExitCode::Success
}

/// What init prints for the project it set up: one object with --json, so nothing but JSON
/// reaches standard output, and the text otherwise.
// @lfy def/cli/main.lfy:main
fn created_line(json: bool, manifest: &str) -> String {
    // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
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
    // @lfy def/cli/main.lfy:main#main:main:5195c7d14b9b471efde014a85a9899221c2f10ef7c18a0bae7557e737b529f8e
    // @lfy def/cli/main.lfy:main#main:main:2522a3017f6b6c237cf9b516c25d92ce44fedd90b1e60a1c5bf46810edf6a8c1
    let diagnostics = collect_diagnostics(&workspace, &invocation.arguments);
    for diagnostic in &diagnostics {
        print_diagnostic(diagnostic, json, on);
    }
    // @lfy def/cli/main.lfy:main#main:main:e1c492734a8a85af39b52fb6730be86b2953604eb5ec203d3e081e8d54b386d7
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return ExitCode::Problems;
    }
    // With no files given, the plan is made with no stems requested, from the source maps of
    // the workspace and the stems the last global review found violated, and each unit that
    // is not up to date is printed as a warning of stage generation.
    // @lfy def/cli/main.lfy:main#main:main:32543293ed4af09d979dd15d096116fcfe9a8604b2530010f3228a20dd8126b4
    let mut stale = 0usize;
    let mut cycles = 0usize;
    if invocation.arguments.is_empty() {
        // A program that declares no target says so once, at elfie.json: it is neither a
        // problem nor a stale unit, since there is nothing wrong with it and nothing to
        // bring up to date.
        // @lfy def/cli/main.lfy:main#main:main:ab61edddf16362666867261f64e558c75d78f00fb2fb566bd5a55a393d93fadc
        if workspace.targets.is_empty() {
            print_no_target(&root.join("elfie.json").display().to_string(), json, on);
        }
        // The plan is made from the source maps of the workspace, with no stems requested,
        // and with the violated units of the last global review as its violated units.
        // @lfy def/cli/main.lfy:main#main:main:97a1afc5885c67e733bbc5a12d4919a97a3bf22bfdf6cd2fc8066dec4da9c2fc
        let maps = generation::source_maps_of(&workspace, None);
        let (program, plan) = planned(root, &maps, &[], false, &violated_ids(root));
        // A cycle among files that no move could resolve is an error of stage generation at
        // its node and counts among the problems.
        // @lfy def/cli/main.lfy:main#main:main:2c23167a93d05731803b314b2b6545c5ee1481978f5c546aacb5c0b7f512115a
        for problem in &plan.problems {
            cycles += 1;
            print_plan_problem(&program.workspace, problem, json, on);
        }
        // A move is information of stage generation: neither a problem nor a stale unit.
        // @lfy def/cli/main.lfy:main#main:main:d9b19641dafa296e2a23f8ccc4fcc5131549aa763ea4d727e8df11b3e07c3e18
        // @lfy def/cli/main.lfy:main#main:main:c89a1f3b912b1924d320ba6e0c1fb1de16f233edbef8b0b8182a60605107867a
        for moved in &plan.moves {
            print_move(&program.workspace, &plan, moved, json, on);
        }
        for unit in &plan.units {
            let Some(reason) = unit.reason else { continue };
            stale += 1;
            print_stale(&program.workspace, unit, reason, json, on);
        }
    }
    // The last line counts the files, the problems, and the stale units: the count of files
    // is plain, the problems are a tally in failure, and the stale units one in warning.
    // @lfy def/cli/main.lfy:main#main:main:9e7210de90827836891690ff65aa3f8c5367c71675e78701e57428954c22d633
    // A cycle no move could resolve counts among the problems, as every error or warning
    // diagnostic printed does.
    // @lfy def/cli/main.lfy:main#main:main:7ac16e9976c60d6a2af6a4ade349ff7a5c56e296265b682ecd0f59c0cf03f5d4
    let problems = diagnostics.len() + cycles;
    if json {
        // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
        println!("{}", serde_json::json!({ "files": workspace.files.len(), "problems": problems, "stale": stale }));
    } else {
        // @lfy def/cli/main.lfy:main#main:main:e4a92e90c92796a64c208f188a8f672fb3cb8c79eb89b30ec2f4c53b260c6d92
        println!(
            "{} files, {}, {}",
            workspace.files.len(),
            tally(problems, "problems", Tone::Failure, on),
            tally(stale, "stale units", Tone::Warning, on)
        );
    }
    // --strict: the code is problems when anything at all was printed as a warning or
    // error, a stale unit included.
    // @lfy def/cli/main.lfy:main#main:main:1e2154af80e63be28da09546cbb9a591d4443af66f06409d3c369826dd80b8f6
    if invocation.flag("strict") && (problems > 0 || stale > 0) {
        ExitCode::Problems
    } else if cycles > 0 {
        // A cycle was printed as an error, so the code is problems as any printed error
        // makes it.
        // @lfy def/cli/main.lfy:main#main:main:e1c492734a8a85af39b52fb6730be86b2953604eb5ec203d3e081e8d54b386d7
        ExitCode::Problems
    } else {
        // No error and no warning was printed, so --strict is success too.
        // @lfy def/cli/main.lfy:main#main:main:4c43ec7913846d2cf1040881428da0f35394bea06a61ac4e69f4cffc7d8d3d52
        // @lfy def/cli/main.lfy:main#main:main:291f143dfc5205e9d6fe730e2b2ed169dbf14ff75c662c86ce140e99714e009d
        ExitCode::Success
    }
}

/// One line of information of stage generation at `elfie.json` saying the program declares
/// no target, so nothing will be built; it is neither a problem nor a stale unit.
// @lfy def/cli/main.lfy:main#main:main:ab61edddf16362666867261f64e558c75d78f00fb2fb566bd5a55a393d93fadc
fn print_no_target(manifest: &str, json: bool, on: bool) {
    let message = "the program declares no target, so nothing will be built";
    if json {
        // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
        println!(
            "{}",
            serde_json::json!({
                "file": manifest,
                "stage": "generation",
                "severity": Severity::Information.value(),
                "message": message,
            })
        );
    } else {
        // @lfy def/cli/main.lfy:main#main:main:ab61edddf16362666867261f64e558c75d78f00fb2fb566bd5a55a393d93fadc
        println!(
            "{} {} {}: {message}",
            paint(&format!("{manifest}:"), Tone::Subject, on),
            paint("generation", Tone::Muted, on),
            paint(Severity::Information.value(), severity_tone(Severity::Information), on)
        );
    }
}

/// A unit that is not up to date, printed as a warning of stage generation at its source
/// file, naming the target, the unit, and the description of its reason; the target and the
/// unit are subject and the description is painted in the reason's own tone.
// @lfy def/cli/main.lfy:main#main:main:32543293ed4af09d979dd15d096116fcfe9a8604b2530010f3228a20dd8126b4
fn print_stale(workspace: &Workspace, unit: &Unit, reason: Reason, json: bool, on: bool) {
    let file = &workspace.files[unit.file].path;
    let target = &workspace.targets[unit.target].identifier;
    let message = format!("{target} {}: {}", unit.stem, reason.value());
    if json {
        // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
        println!(
            "{}",
            serde_json::json!({ "file": file, "stage": "generation", "severity": "warning", "message": message })
        );
    } else {
        // @lfy def/cli/main.lfy:main#main:main:8f743c1762e241dd7fd8ae0a41cb33ca28b92b6cb56c513da3310a47a926449f
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

/// One of [`Plan::problems`] printed as an error of stage generation at its node: a cycle
/// among files that no move could resolve, so there is no order to build them in.
// @lfy def/cli/main.lfy:main#main:main:2c23167a93d05731803b314b2b6545c5ee1481978f5c546aacb5c0b7f512115a
// @lfy def/cli/main.lfy:main#main:main:055ed621ccb4cfc04dafc60c4ae42b146676fbac4139bd00ac26ddae87c1ad70
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

/// One of [`Plan::moves`] printed as information of stage generation at the file of its
/// origin, naming the target, the identifiers of the entities moved, the stem of the unit
/// that compiles them, and the cause; it is neither a problem nor a stale unit.
// @lfy def/cli/main.lfy:main#main:main:2c23167a93d05731803b314b2b6545c5ee1481978f5c546aacb5c0b7f512115a
fn print_move(workspace: &Workspace, plan: &Plan, moved: &Move, json: bool, on: bool) {
    let origin = &plan.units[moved.origin];
    let file = &workspace.files[origin.file].path;
    let target = &workspace.targets[origin.target].identifier;
    let entities = moved_entities(workspace, moved);
    let message = format!("{target} [{entities}] into {} for {}", plan.units[moved.into].stem, moved.cause);
    if json {
        // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
        println!(
            "{}",
            serde_json::json!({
                "file": file,
                "stage": "generation",
                "severity": Severity::Information.value(),
                "message": message,
            })
        );
    } else {
        println!(
            "{} {} {}: {message}",
            paint(&format!("{file}:"), Tone::Subject, on),
            paint("generation", Tone::Muted, on),
            paint(Severity::Information.value(), severity_tone(Severity::Information), on)
        );
    }
}

// @lfy def/cli/main.lfy:main
fn collect_diagnostics(workspace: &Workspace, files: &[String]) -> Vec<Diagnostic> {
    // @lfy def/cli/main.lfy:main#main:main:5195c7d14b9b471efde014a85a9899221c2f10ef7c18a0bae7557e737b529f8e
    if files.is_empty() {
        query::diagnostics_of(workspace, None)
    } else {
        // @lfy def/cli/main.lfy:main#main:main:2522a3017f6b6c237cf9b516c25d92ce44fedd90b1e60a1c5bf46810edf6a8c1
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
    // @lfy def/cli/main.lfy:main#main:main:7f65284af03cc165df66dc356aafafe89594cec6fb35dac3aa4584da5aa9f747
    // @lfy def/cli/main.lfy:main#main:main:3454380ed0ab0be90c65034578648d28e79c0ec647abe81aafa5f4149ea66d16
    let files: Vec<PathBuf> = if invocation.arguments.is_empty() {
        let workspace = workspace::load(root);
        let mut found = Vec::new();
        walk(&root.join(&workspace.source_directory), &mut found);
        found.sort();
        found
    } else {
        // @lfy def/cli/main.lfy:main#main:main:6e2a2b3949ecb246a2d961d2c723330f7368260169b99e1b66bfbc0191c34fb9
        // @lfy def/cli/main.lfy:main#main:main:6e2a2b3949ecb246a2d961d2c723330f7368260169b99e1b66bfbc0191c34fb9
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
        // @lfy def/cli/main.lfy:main#main:main:ab93b927612e920e99ff39e2c52a46ea43151d6b2560a8e13248bb27550a15d2
        if !tree.errors.is_empty() {
            // @lfy def/cli/main.lfy:main#main:main:ab93b927612e920e99ff39e2c52a46ea43151d6b2560a8e13248bb27550a15d2
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
                    // @lfy def/cli/main.lfy:main#main:main:2729826dde4194a5d195a8c4990c0f22f3d75eec87f6a0246e625d5120ac05b7
                    println!(
                        "{} {} {}: expected [{expected}] but found {value:?}",
                        paint(&format!("{display}:{line}:{column}:"), Tone::Subject, style.out),
                        paint("parser", Tone::Muted, style.out),
                        paint("error", severity_tone(Severity::Error), style.out)
                    );
                }
            }
            // The file is left as it is and the code is problems.
            // @lfy def/cli/main.lfy:main#main:main:71eacf537d345adea7d105912e412bf5d0854cdf6be1dadd22f7ea8f2f226d86
            // @lfy def/cli/main.lfy:main#main:main:fca47b36164e7adbd33f1fc08dc5f5524cc5ea893ed7a94e375fbc0b28a7f5d3
            code = ExitCode::Problems;
            continue;
        }
        // The formatted text is the format of the parse of the lex of what the file holds.
        // @lfy def/cli/main.lfy:main#main:main:eee5fecd3046566d4f1d1916b27d8700e565f2f248f895a74ef7138c56e48b58
        // The file is replaced by the standard layout only where that differs from its text.
        // @lfy def/cli/main.lfy:main#main:main:b755a72c010c9d018d321559876bc2cacde3c27cb4f4ddf2a7b6751d0f881bd0
        let formatted = elfie_core::format::format(&tree);
        if formatted == source {
            continue;
        }
        // Nothing is written; each file that would change is printed.
        // @lfy def/cli/main.lfy:main#main:main:f1176fb82acbd19538241e67b518efa48f67433cec1fd4b946b49f9271cd558a
        if check_only {
            // @lfy def/cli/main.lfy:main#main:main:6e2a2b3949ecb246a2d961d2c723330f7368260169b99e1b66bfbc0191c34fb9
            // @lfy def/cli/main.lfy:main#main:main:3454380ed0ab0be90c65034578648d28e79c0ec647abe81aafa5f4149ea66d16
            println!("{}", formatted_line(json, &display, true, style.out));
            // @lfy def/cli/main.lfy:main#main:main:8f66b1f987d4182118d36748e1764b51e86e73de389e963296f6f5f78e5d8ab1
            code = ExitCode::Problems;
        } else if let Err(error) = fs::write(&path, formatted) {
            complain(style.err, Some(&display), &error.to_string());
            code = ExitCode::Failure;
        } else {
            // @lfy def/cli/main.lfy:main#main:main:bde950645f1a232e219e4c47c365187cb2799f267532edecb9f18ae2f761bc2f
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
    // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
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
    // @lfy def/cli/main.lfy:main#main:main:bde950645f1a232e219e4c47c365187cb2799f267532edecb9f18ae2f761bc2f
    format!("{} {file}", paint("formatted", Tone::Success, on))
}

/// Every `.lfy` file under a directory.
// @lfy def/cli/main.lfy:main#main:main:7f65284af03cc165df66dc356aafafe89594cec6fb35dac3aa4584da5aa9f747
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
// @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
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
            // @lfy def/cli/main.lfy:main#main:main:df8eae91a10b3ccf0886164683a4fd7b05f4460ac0ba6a67f49f9f5fbb1ed85f
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
                    "value": token.value,
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
///
/// A token is given as `Token::value`, the characters with escapes applied, never the raw
/// characters it was written as; the text it covers is only ever printed raw for an error
/// node, which stands for text no token could be made of.
// @lfy def/cli/main.lfy:main#main:main:c888bea4f4ce679a5bef8e137a01e4d58fef7062c146500ddf5476aec6fe5668
fn node_lines(tree: &Tree, node: &Node, depth: usize, on: bool, out: &mut Vec<String>) {
    let indent = " ".repeat(depth * 2);
    // One node per line indented by depth, as its rule and its token range; the rule is
    // active and the range is muted.
    // @lfy def/cli/main.lfy:main#main:main:b409d01cc636c5584a8f66bae5a3cf7e6416ba67105dfdf93bd0d6395f51e6db
    // @lfy def/cli/main.lfy:main#main:main:6592bac5b3c0185c725d378c01c62838c7fdac553537b6c317eb5d0b4a736afe
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
                // A token as its rule and its value, the characters with escapes applied
                // rather than as written; the rule is active and the value is success.
                // @lfy def/cli/main.lfy:main#main:main:e6f0f308d5eaac2370089dcf54b826785084d2268b4495637c219c758541efe3
                // @lfy def/cli/main.lfy:main#main:main:6592bac5b3c0185c725d378c01c62838c7fdac553537b6c317eb5d0b4a736afe
                out.push(format!(
                    "{indent}{} {}",
                    paint(rule_name(token), Tone::Active, on),
                    paint(&format!("{:?}", token.value), Tone::Success, on)
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
    // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
    if json {
        node_json(tree, &tree.root, 0, &mut lines);
        return lines;
    }
    node_lines(tree, &tree.root, 0, on, &mut lines);
    // Each error node is printed with what it expected, painted failure.
    // @lfy def/cli/main.lfy:main#main:main:df8eae91a10b3ccf0886164683a4fd7b05f4460ac0ba6a67f49f9f5fbb1ed85f
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
    // @lfy def/cli/main.lfy:main#main:main:c888bea4f4ce679a5bef8e137a01e4d58fef7062c146500ddf5476aec6fe5668
    for line in tree_lines(&tree, &path, invocation.flag("json"), Paint::of(invocation).out) {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
    // @lfy def/cli/main.lfy:main#main:main:e9edf88c58bc339db1427baac3c710de71d0f8ca43d95682df4cf366b4ca5477
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
            // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
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
                // @lfy def/cli/main.lfy:main#main:main:6592bac5b3c0185c725d378c01c62838c7fdac553537b6c317eb5d0b4a736afe
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
// @lfy def/cli/main.lfy:main#main:main:0e04fca1d5a1398b0203e9dfa3d58b7324f1ef742ad13c350264b5b958456075
const ANSWER_HEADING: &str = "## The answer";

/// Every step, so that the step column keeps one width for the whole compile.
// @lfy def/cli/main.lfy:main
const STEPS: [Step; 18] = [
    Step::Planned,
    Step::Requesting,
    Step::Resuming,
    Step::Rewording,
    Step::Building,
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
// @lfy def/cli/main.lfy:main#main:main:e6a992a863aebc985eeb11dd2cef62220e5e7590ea2500babb4124684929657c
fn step_column() -> usize {
    STEPS.iter().map(|step| width(glyph_of(*step)) + 1 + width(step.name())).max().unwrap_or(0)
}

/// How many columns the time column takes.
// @lfy def/cli/main.lfy:main#main:main:2ba85a50267062837687f356e2de2edec47dfcbaf012913034645e8c9586d86c
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
// @lfy def/cli/main.lfy:main#main:main:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
// @lfy def/cli/main.lfy:main#main:main:081257eb508c08a673bc5c569b3099485967e6b85b434c98d303c2de2d9958c5
fn counts_message(counts: &Counts, on: bool) -> String {
    counts.iter().map(|(count, label, tone)| tally(*count, label, *tone, on)).collect::<Vec<_>>().join(", ")
}

/// The counts of satisfied, violated, and unverifiable reviews, in that order, each with the
/// tone it is painted in, with every id the review was asked about that no review names
/// counted as unverifiable: nothing answered for it, which is what unverifiable means.
// @lfy def/cli/main.lfy:main#main:main:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
// @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
fn counts_with_missing(reviews: &[Review], missing: usize) -> Counts {
    let (satisfied, violated, unverifiable) = counts_of(reviews);
    vec![
        (satisfied, "satisfied", Tone::Success),
        (violated, "violated", Tone::Failure),
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        (unverifiable + missing, "unverifiable", Tone::Warning),
    ]
}

/// The same counts, of the reviews of one run of the verifier alone, with the verdicts its
/// review request carried counted after them and only there: a carried verdict is no review
/// of this run, so it is neither satisfied, violated, nor unverifiable here.
///
/// With nothing carried the counts end with the unverifiable, as they do for a global review,
/// which carries nothing.
// @lfy def/cli/main.lfy:main#main:main:f088a8a0ce2ac66dea4de2f7c633f70122a6f42b582ae35203ed2bea262e67f3
// @lfy def/cli/main.lfy:main#main:main:40a59e64f37d7276f9f2e1c4d1434060c145115922c8218363f3a700cb7f7beb
// @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
// @lfy def/cli/main.lfy:main#main:main:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
fn counts_with_carried(reviews: &[Review], missing: usize, carried: usize) -> Counts {
    let mut counts = counts_with_missing(reviews, missing);
    // The count of carried verdicts and the word carried, painted muted: a verdict nothing
    // had to look at again is worth noticing least of all.
    // @lfy def/cli/main.lfy:main#main:main:f088a8a0ce2ac66dea4de2f7c633f70122a6f42b582ae35203ed2bea262e67f3
    // @lfy def/cli/main.lfy:main#main:main:01ac2a8e3e988e7c07b647cd40cc38bce72044f98971d9e5e72eb5c62425cc0e
    if carried > 0 {
        counts.push((carried, "carried", Tone::Muted));
    }
    counts
}

/// One progress line, in columns separated by two spaces: the counter, the time, the step,
/// then the batch and unit when there are any, and the first line of the message. A reason or
/// a question runs to several lines, so the line carries only its first; the whole of it
/// follows, indented.
///
/// The counter and the time are muted, the step is its own tone unless one is given, the
/// batch and the unit are subject, and the message is the step's tone for a rejection, a
/// failure, a block, or a question, the counts when there are counts, and plain otherwise.
// @lfy def/cli/main.lfy:main#main:main:e644285bff17055757f9da45f329dc88d9e4ec34a19c3b5ce92b8a4a03950621
fn text_of(progress: &Progress, counts: Option<&Counts>, tone: Option<Tone>, on: bool) -> String {
    // The counter keeps one width for the whole compile.
    // @lfy def/cli/main.lfy:main#main:main:a8367555ef05292d4118872e01aece83edde2d5816e9e4c404840757608603d3
    let digits = progress.total.to_string().len();
    let counter = paint(&format!("[{:>digits$}/{}]", progress.done, progress.total), Tone::Muted, on);
    // @lfy def/cli/main.lfy:main#main:main:2ba85a50267062837687f356e2de2edec47dfcbaf012913034645e8c9586d86c
    let time = paint(&format!("{:>TIME_COLUMN$}", duration(progress.elapsed)), Tone::Muted, on);
    // @lfy def/cli/main.lfy:main#main:main:e6a992a863aebc985eeb11dd2cef62220e5e7590ea2500babb4124684929657c
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
    // @lfy def/cli/main.lfy:main#main:main:e644285bff17055757f9da45f329dc88d9e4ec34a19c3b5ce92b8a4a03950621
    let message = match counts {
        // @lfy def/cli/main.lfy:main#main:main:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
        Some(counts) => counts_message(counts, on),
        None => {
            let first = progress.message.lines().next().unwrap_or_default();
            match progress.step {
                // @lfy def/cli/main.lfy:main#main:main:c164c51236df72a93cac3a3c7bcc44ec0848586ed13d2f9add36192db3bc1999
                Step::Rejected | Step::Failed | Step::Blocked | Step::Clarification => {
                    paint(first, tone_of(progress.step), on)
                }
                // @lfy def/cli/main.lfy:main#main:main:6c53e9ea7e1afdca38608efb3da118808b542e52286d1acc162d7cdcb6d1a4de
                Step::Planned
                | Step::Requesting
                | Step::Resuming
                | Step::Rewording
                | Step::Building
                | Step::Compiling
                | Step::Checking
                | Step::Accepted
                | Step::Verifying
                | Step::GlobalVerifying
                | Step::Retrying => first.to_string(),
                // A line of counts whose counts were not given, which only a reviewed,
                // globalReviewed, or finished line carries, reads as it was written.
                // @lfy def/cli/main.lfy:main#main:main:e644285bff17055757f9da45f329dc88d9e4ec34a19c3b5ce92b8a4a03950621
                Step::Reviewed | Step::GlobalReviewed | Step::Finished => first.to_string(),
            }
        }
    };
    // The line ends with the first line of the message.
    // @lfy def/cli/main.lfy:main#main:main:72582398057ef4b458680631986c9aa55de4061eecc790f79ca14cc77a71b86d
    if !message.is_empty() {
        columns.push(message);
    }
    // The columns are separated by two spaces.
    // @lfy def/cli/main.lfy:main#main:main:fd851430001b39d2d1c79a0807242453986bf07f0fbe972e5ecd9815c3380aba
    columns.join("  ")
}

/// One progress line as one JSON object.
// @lfy def/cli/main.lfy:main#main:main:bbad3c69582995a56ff9152a6a483894f2d276f7b0d0b4805da999767796b37e
// @lfy def/cli/main.lfy:main#main:main:bbad3c69582995a56ff9152a6a483894f2d276f7b0d0b4805da999767796b37e
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
// @lfy def/cli/main.lfy:main#main:main:ce3b9a1a98759c3ae080fd41207a076d4853acdb0f87c1f511632082039e8a3e
struct Reporter {
    json: bool,
    /// Whether each stream is painted; the log is written with nothing painted.
    // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
    style: Paint,
    started: SystemTime,
    total: usize,
    done: usize,
    logs: Vec<PathBuf>,
    /// The batch the last line concerned, so the first line of a batch after the first batch
    /// has an empty line before it.
    // @lfy def/cli/main.lfy:main#main:main:ac2c0a6d7ff25b2bb1254f7a315b9d36b9825e5903e052d81271983f3d38201a
    batch: Option<String>,
}

impl Reporter {
    // @lfy def/cli/main.lfy:main
    fn new(json: bool, style: Paint, total: usize, logs: Vec<PathBuf>) -> Reporter {
        Reporter { json, style, started: SystemTime::now(), total, done: 0, logs, batch: None }
    }

    /// One line for one step, printed and logged.
    // @lfy def/cli/main.lfy:main#main:main:ce3b9a1a98759c3ae080fd41207a076d4853acdb0f87c1f511632082039e8a3e
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    fn report(&mut self, step: Step, batch: Option<&str>, unit: Option<&str>, message: &str) {
        self.emit(step, batch, unit, message.to_string(), None, None);
    }

    /// One line whose message is the counts it reports, each painted only when it is worth
    /// noticing.
    // @lfy def/cli/main.lfy:main
    fn report_counts(&mut self, step: Step, batch: Option<&str>, counts: &Counts, tone: Option<Tone>) {
        // The message with --json holds the same words the counts are printed with.
        // @lfy def/cli/main.lfy:main#main:main:081257eb508c08a673bc5c569b3099485967e6b85b434c98d303c2de2d9958c5
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
            // @lfy def/cli/main.lfy:main#main:main:2ba85a50267062837687f356e2de2edec47dfcbaf012913034645e8c9586d86c
            elapsed: elapsed(self.started),
            message,
        };
        // An empty line before the first line of a batch after the first batch, before
        // finished, and before globalVerifying, so each batch reads as one block.
        // @lfy def/cli/main.lfy:main#main:main:ac2c0a6d7ff25b2bb1254f7a315b9d36b9825e5903e052d81271983f3d38201a
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
        // @lfy def/cli/main.lfy:main#main:main:bbad3c69582995a56ff9152a6a483894f2d276f7b0d0b4805da999767796b37e
        // @lfy def/cli/main.lfy:main#main:main:bbad3c69582995a56ff9152a6a483894f2d276f7b0d0b4805da999767796b37e
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

    /// The problems of a step, following the line they belong to: one per line, each indented
    /// four spaces, beginning with `-` and a space and painted failure. With --json nothing
    /// follows, since the whole of the message is in the [`Progress`].
    // @lfy def/cli/main.lfy:main#main:main:43f2aa5048680432b93b7802c6b6a48f5fcb659a985ce06c84d0358d44a2277d
    fn problems(&mut self, problems: &[String]) {
        if self.json {
            return;
        }
        for problem in problems {
            // @lfy def/cli/main.lfy:main#main:main:7278a611f5028db0b60166d22b250e5673ae6472e74d4da368aacbd121b9843b
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
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
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
// @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
// @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
fn append_to(logs: &[PathBuf], text: &str) {
    // @lfy def/cli/main.lfy:main#main:main:3a1f341e1f488489eebade96f9e410811db493057811f2c78e1ceb8d3a8ae77d
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

// ---- compile ----------------------------------------------------------------------

/// The command elfie.json names under a key, when it names one.
// @lfy def/cli/main.lfy:main
fn manifest_command(root: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(root.join("elfie.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get(key)?.as_str().map(str::to_string)
}

/// The manifest's `compiler` command, when it names one.
// @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
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
///
/// A target whose folder of map files is there had its maps read from that folder, which is
/// where every map recorded since goes, so its source-map.json is not what the plan was made
/// from and nothing is moved out of it.
// @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
fn legacy_targets(workspace: &Workspace, target: Option<&str>) -> Vec<String> {
    let mut found = Vec::new();
    for named in &workspace.targets {
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        // @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
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
/// leaves the file in place rather than removed, so the maps it holds are not lost.
// @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
// @lfy def/cli/main.lfy:main#main:main:b9ad771de6ec1ee6e2812d5335aef919fe9cd8228cb6b2fea9731de46c6dd24e
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
            // @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
            every &= generation::record(workspace, unit, &mine);
        }
        // Every one recorded, the file is removed; one that could not be leaves it in place
        // rather than removed, so the maps it holds are not lost.
        // @lfy def/cli/main.lfy:main#main:main:b9ad771de6ec1ee6e2812d5335aef919fe9cd8228cb6b2fea9731de46c6dd24e
        if every {
            // @lfy def/cli/main.lfy:main#main:main:08ac623b539225786793a4c37eb60950b68b85acbd4d4b5306bc4fc89fd8b9fb
            let _ = fs::remove_file(&path);
        }
    }
}

/// Every map file under the folder of a planned target that belongs to no unit of the plan,
/// because its definition file was deleted, moved, or no longer builds for the target,
/// removed before any batch runs.
// @lfy def/cli/main.lfy:main#main:main:531885eb3c4021855f9d9fbc7e806cedbc5da481cdcee0ca5e70c32b6edaad0d
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
            // @lfy def/cli/main.lfy:main#main:main:531885eb3c4021855f9d9fbc7e806cedbc5da481cdcee0ca5e70c32b6edaad0d
            if path.extension().is_some_and(|e| e == "json") && !mine.contains(&path) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// Every file under `elfie-compile/cache/kept` for a planned target that names a stem that is
/// no unit of the plan, removed before any batch runs: the work it kept answers for a unit
/// that is gone, so nothing may take it.
// @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
fn remove_orphan_kept(workspace: &Workspace, plan: &Plan, target: Option<&str>) {
    // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
    let mine: BTreeSet<&str> = plan.units.iter().map(|unit| unit.stem.as_str()).collect();
    // The targets the plan holds a unit for, which are the planned ones: work kept for any
    // other target is not this compile's to remove.
    // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
    let planned_targets: BTreeSet<&str> = plan
        .units
        .iter()
        .map(|unit| workspace.targets[unit.target].identifier.as_str())
        .filter(|named| target.is_none_or(|target| *named == target))
        .collect();
    let mut found = Vec::new();
    walk_all(&workspace.root.join(KEPT), &mut found);
    for path in found {
        if !path.extension().is_some_and(|extension| extension == "json") {
            continue;
        }
        // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
        let held = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| Kept::of_json(&value));
        let Some(held) = held else { continue };
        // Only the kept work of a planned target is looked at here; another target's is for
        // the compile that plans it.
        // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
        if !planned_targets.contains(held.target.as_str()) {
            continue;
        }
        // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
        if held.stems().iter().any(|stem| !mine.contains(stem)) {
            // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
            let _ = fs::remove_file(&path);
        }
    }
}

/// Where the review records of every unit of a target are kept, under the root.
// @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
const REVIEWS: &str = "elfie-compile/reviews";

/// Every review record file under the folder of a planned target that belongs to no unit of
/// the plan, because its definition file was deleted, moved, or no longer builds for the
/// target, removed before any batch runs.
// @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
fn remove_orphan_reviews(workspace: &Workspace, plan: &Plan, target: Option<&str>) {
    let mine: BTreeSet<PathBuf> =
        plan.units.iter().map(|unit| generation::review_file(workspace, unit)).collect();
    for named in &workspace.targets {
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        let folder = workspace.root.join(REVIEWS).join(&named.identifier);
        let mut found = Vec::new();
        walk_all(&folder, &mut found);
        for path in found {
            // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
            if path.extension().is_some_and(|e| e == "json") && !mine.contains(&path) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// The files under the target's output directory, in the directory the batch ran in, split
/// into the ones that carry a marker for the unit's file and every other one of that listing.
/// Their paths are relative to that directory, which is the root or a copy of it, so an
/// output is named the same either way.
///
/// The second list is what `accept` is given as its `elsewhere`: a file of it earns no source
/// map and none of its markers is checked, and it is there so that a region this unit moved
/// out of a shared file into a file of its own is still found where it went.
// @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
// @lfy def/cli/main.lfy:main#main:main:e6df0c7548d8f79abfca3e578a974cfbbf2949927b4851d9860223b1315c6c27
fn outputs_of(
    workspace: &Workspace,
    plan: &Plan,
    unit: usize,
    directory: &Path,
) -> (Vec<Output>, Vec<Output>) {
    let unit = &plan.units[unit];
    let target = &workspace.targets[unit.target];
    let file = &workspace.files[unit.file].path;
    let mut paths = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
    walk_all(&directory.join(&target.output_directory), &mut paths);
    paths.sort();
    let mut found = Vec::new();
    let mut elsewhere = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let output = Output { path: relative(directory, &path), text };
        // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
        if generation::parse_markers(&output.text).iter().any(|m| &m.file == file) {
            found.push(output);
        } else {
            // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
            elsewhere.push(output);
        }
    }
    (found, elsewhere)
}

/// Whether a marker is one unit's own: it names the unit's file, or it names an entity moved
/// into the unit and the file that declares it.
// @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
fn own_marker(workspace: &Workspace, plan: &Plan, unit: usize, marker: &Marker) -> bool {
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    if marker.file == workspace.files[plan.units[unit].file].path {
        return true;
    }
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    plan.moves
        .iter()
        .filter(|moved| moved.into == unit)
        .filter(|moved| workspace.files[plan.units[moved.origin].file].path == marker.file)
        .flat_map(|moved| moved.entities.iter())
        .any(|&entity| {
            let Some(name) = workspace.model.entities[entity].identifier.as_deref() else {
                return false;
            };
            marker.entity.as_deref().is_some_and(|spelled| {
                spelled == name || spelled.strip_prefix(name).is_some_and(|rest| rest.starts_with('.'))
            })
        })
}

/// The unit's own region text of one output, as a source map defines it: the text of the
/// regions its own markers begin, in output order, each line with the marker comment it
/// carries and the white space before it taken off and every line that is then empty left
/// out.
// @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
fn own_region_text(workspace: &Workspace, plan: &Plan, unit: usize, text: &str) -> String {
    let comment = &workspace.targets[plan.units[unit].target].marker_comment;
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    for marker in generation::parse_markers(text) {
        if !own_marker(workspace, plan, unit, &marker) {
            continue;
        }
        let last = marker.end.min(lines.len());
        let first = marker.output_line.saturating_sub(1).min(last);
        for line in &lines[first..last] {
            // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
            let kept = without_marker(line, comment);
            if kept.is_empty() {
                continue;
            }
            out.push_str(kept);
            out.push('\n');
        }
    }
    out
}

/// One line of an output with the marker comment it carries, and the white space before it,
/// taken off; the line as it is when it carries none.
// @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
fn without_marker<'l>(line: &'l str, comment: &str) -> &'l str {
    if comment.is_empty() {
        return line;
    }
    let mut from = 0;
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    while let Some(position) = line[from..].find(MARKER) {
        let at = from + position;
        // The comment opener of this marker: the last one before it, which the marker is
        // written after as the guidance says.
        // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
        if let Some(opener) = line[..at].rfind(comment) {
            return line[..opener].trim_end();
        }
        from = at + MARKER.len();
    }
    line
}

/// What every marker begins with, after the target's comment opener.
// @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
const MARKER: &str = "@lfy ";

/// One region named the way a marker spells it: the file, the entity or the line, and the
/// requirement when it answers for one. Two markers of one region spell the same name.
// @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
fn region_named(marker: &Marker) -> String {
    let place = match &marker.entity {
        Some(entity) => entity.clone(),
        None => marker.line.to_string(),
    };
    // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
    match &marker.requirement {
        Some(requirement) => format!("{}:{place}#{requirement}", marker.file),
        None => format!("{}:{place}", marker.file),
    }
}

/// Every local criterion and test of one unit, by id and sorted: for each of its entities,
/// the criteria of its declaration and then its tests, then those of its own file, which is
/// exactly what a review request computes a hash for. A unit's review records are written for
/// these, in this order.
// @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
fn requirement_ids_of(program: &Program, unit: &Unit) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for &entity in &unit.entities {
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        let Some(node) = declared_node(program, unit.lowered, entity) else { continue };
        out.extend(node.criteria.iter().map(|criterion| criterion.id.clone()));
        out.extend(node.tests.iter().map(|test| test.id.clone()));
    }
    // A criterion or test of the unit's own file is one of the unit's own all the same.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    let file = &program.files[unit.lowered];
    out.extend(file.criteria.iter().map(|criterion| criterion.id.clone()));
    out.extend(file.tests.iter().map(|test| test.id.clone()));
    // Sorted by id, so the records of a unit are written in one order whatever the order its
    // entities were reviewed in.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    out.sort();
    out.dedup();
    out
}

/// The lowered node that declares one entity of a unit: its declaration in the unit's own
/// lowered file, or, for an entity moved into the unit from another file, its declaration in
/// the file it came from, whose criteria and tests are the unit's to answer for just the
/// same.
// @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
fn declared_node(program: &Program, lowered: usize, entity: EntityId) -> Option<&LoweredNode> {
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    declaring_node(&program.files[lowered].root, entity)
        .or_else(|| program.files.iter().find_map(|file| declaring_node(&file.root, entity)))
}

/// The node of a lowered tree that declares an entity, the node itself before its children.
// @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
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

/// One revision of a unit's file recovered from git, and whether it is only the unit's
/// hash-matched source rather than the source its outputs were accepted from.
// @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
struct Previous {
    /// The revision as git holds it, never lowered.
    // @lfy def/cli/main.lfy:main#main:main:1f3c991d456ca136f454724f3f7fd0dfcb94081ad9e4e7f5f238375155b93e8f
    text: String,
    /// Whether its lowered text hashes to the map but its criterion and test ids do not.
    // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
    hash_matched: bool,
}

/// The source at the last accepted generation, recovered from git when a recent revision of
/// the file both lowers to the hash the unit's source map holds and has the criterion and test
/// ids that map was recorded for; otherwise the unit's hash-matched source, the most recent
/// revision that lowers to that hash alone, marked as such.
///
/// What is given back is that revision as git holds it, never lowered: `request` lowers the
/// previous text it is handed the way `changes` binds one, so handing it lowered code would
/// lower it twice and strip the criteria from what the compiler reads. The map holds the hash
/// of the lowered text, so the revision the outputs were generated from is the one whose
/// lowered text hashes to it, and a revision that differs only in a comment is the same
/// revision.
///
/// A revision that lowers to the hash but whose ids hash to something else is not what the
/// outputs were accepted from: a rewording leaves the lowered code untouched and changes every
/// id it touched, so only the ids say which revision the outputs answered for.
// @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
// @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
// @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
fn previous_source(workspace: &Workspace, unit: &Unit) -> Option<Previous> {
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
    // The most recent revision that lowers to the map's hash and no more, kept in case none
    // of them has the ids the map was recorded for.
    // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
    let mut matched: Option<String> = None;
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
        // The revision is recognized by the hash of its text lowered the way `changes` binds
        // a previous text, since that is what the map holds.
        // @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
        let Some((lowered, requirements)) = lowered_of(workspace, &file, &text) else { continue };
        // @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
        if generation::source_hash(&lowered) != map.hash {
            continue;
        }
        // What the request is given for the unit is that source as git holds it, never
        // lowered.
        // @lfy def/cli/main.lfy:main#main:main:1f3c991d456ca136f454724f3f7fd0dfcb94081ad9e4e7f5f238375155b93e8f
        // @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
        if requirements == map.requirements {
            return Some(Previous { text, hash_matched: false });
        }
        // The lowered code is the map's and the ids are not: this revision only reworded what
        // the outputs answer for, so it is the hash-matched source and not the source at the
        // last accepted generation.
        // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
        if matched.is_none() {
            // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
            matched = Some(text);
        }
    }
    // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
    matched.map(|text| Previous { text, hash_matched: true })
}

/// One file read as something other than what is on disk, bound through the loader and
/// lowered: [`elfie_core::interpret::LoweredFile::text`] of it, the way `changes` binds a
/// previous text, and the SHA-256 of its local criterion and test ids as a source map
/// records them.
// Decision: the ids are read off the lowered file rather than off a plan made for that
// revision, since planning every revision of every unit to recover one text would cost more
// than the recovery is worth; a unit whose entities a plan moves is then not recognized and
// falls back to its hash-matched source, which the criteria already provide for.
// @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
// @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
fn lowered_of(workspace: &Workspace, path: &str, text: &str) -> Option<(String, String)> {
    // @lfy def/cli/main.lfy:main
    let before = lower(workspace::change(workspace, path, Some(text)));
    // @lfy def/cli/main.lfy:main
    let index = before.workspace.files.iter().position(|file| file.path == path)?;
    let file = &before.files[index];
    let mut ids: Vec<String> = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
    for node in file.root.nodes() {
        declared_ids(node, &mut ids);
    }
    // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
    ids.extend(file.criteria.iter().map(|criterion| criterion.id.clone()));
    ids.extend(file.tests.iter().map(|test| test.id.clone()));
    ids.sort();
    // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
    Some((file.text.clone(), generation::source_hash(&ids.join("\n"))))
}

/// The ids of the criteria and tests of one lowered node and of every node under it.
// @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
fn declared_ids(node: &LoweredNode, out: &mut Vec<String>) {
    out.extend(node.criteria.iter().map(|criterion| criterion.id.clone()));
    out.extend(node.tests.iter().map(|test| test.id.clone()));
    for child in node.nodes() {
        declared_ids(child, out);
    }
}

/// One unit as `elfie_units` lists it: the target, the stem, the reason or up to date,
/// and the stems of its dependencies.
///
/// Without --json the target is muted, the stem is subject, the reason is painted in its own
/// tone, and the dependencies are muted; the words and their order stay as they are, so a
/// script splitting a line on spaces reads the same fields.
// @lfy def/cli/main.lfy:main#main:main:5f8a307089850766f7edc63e08cf04c35a137c49fe5052d7ac979ef3f1e491ea
fn unit_line(workspace: &Workspace, plan: &Plan, index: usize, on: bool) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&d| plan.units[d].stem.as_str()).collect();
    // @lfy def/cli/main.lfy:main#main:main:84bb3a6e0235d5d36cc865a418d3f617e6b697d65fd85324d85cc995e44a3e89
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
    // @lfy def/cli/main.lfy:main#main:main:1a9fbb6611deaa4c81a556016aa08baa3d109ea425f6b509c01be7fe23a5ae32
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

/// One of [`Plan::moves`] as a compile prints it: the word move, the target, the stem of the
/// origin, the word into, the stem of the unit that compiles them, the identifiers of the
/// entities moved in brackets, the word for, and the cause.
///
/// Painted, the target and the words move, into, and for are muted, the two stems are
/// subject, and the identifiers and the cause are plain; the words and their order stay as
/// they are either way.
// @lfy def/cli/main.lfy:main#main:main:0f5a7a1258a11b49d77120109972e47f61867f0f8fe2d59078aa9bfa4ffb1849
// @lfy def/cli/main.lfy:main#main:main:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
fn move_text(workspace: &Workspace, plan: &Plan, moved: &Move, on: bool) -> String {
    let origin = &plan.units[moved.origin];
    // @lfy def/cli/main.lfy:main#main:main:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
    format!(
        "{} {} {} {} {} [{}] {} {}",
        paint("move", Tone::Muted, on),
        paint(&workspace.targets[origin.target].identifier, Tone::Muted, on),
        paint(&origin.stem, Tone::Subject, on),
        paint("into", Tone::Muted, on),
        paint(&plan.units[moved.into].stem, Tone::Subject, on),
        moved_entities(workspace, moved),
        paint("for", Tone::Muted, on),
        moved.cause
    )
}

/// Each unit is printed as `elfie_units` lists it, then each batch with the stems it
/// holds, and nothing is generated.
// @lfy def/cli/main.lfy:main#main:main:5f8a307089850766f7edc63e08cf04c35a137c49fe5052d7ac979ef3f1e491ea
fn dry_run(workspace: &Workspace, plan: &Plan, target: Option<&str>, json: bool, on: bool) -> ExitCode {
    // Nothing is generated: the move lines come first, then the unit lines, then the batch
    // lines.
    // @lfy def/cli/main.lfy:main#main:main:2e8045c1f86a83462e1d79b1bdf98da4979fc1d0edf4301ad3cf8b989dc12004
    // @lfy def/cli/main.lfy:main#main:main:96490b9cde09b44c6fecc26b1a57225e5a6f9c756adeff05a82c394cfcf6e886
    // Before the units, each move that resolved a cycle among files.
    // @lfy def/cli/main.lfy:main#main:main:0f5a7a1258a11b49d77120109972e47f61867f0f8fe2d59078aa9bfa4ffb1849
    for moved in &plan.moves {
        let origin = &plan.units[moved.origin];
        if target.is_some_and(|name| workspace.targets[origin.target].identifier != name) {
            continue;
        }
        if json {
            // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
            println!(
                "{}",
                serde_json::json!({
                    "move": origin.stem,
                    "target": workspace.targets[origin.target].identifier,
                    "into": plan.units[moved.into].stem,
                    "entities": moved
                        .entities
                        .iter()
                        .map(|&entity| workspace.model.entities[entity].identifier.clone())
                        .collect::<Vec<_>>(),
                    "cause": moved.cause,
                })
            );
        } else {
            // @lfy def/cli/main.lfy:main#main:main:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
            println!("{}", move_text(workspace, plan, moved, on));
        }
    }
    for index in 0..plan.units.len() {
        let unit = &plan.units[index];
        if target.is_some_and(|name| workspace.targets[unit.target].identifier != name) {
            continue;
        }
        if json {
            // @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
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
    // Then each batch, on a line reading the word batch, its identifier, and the stems it
    // holds.
    // @lfy def/cli/main.lfy:main#main:main:e5ba46ad9fe0f231f9b65946a77625a3a1049f4496f157325f08ca062c200285
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

/// What the verifier is handed when it reviews every global criterion and test once, and the
/// batch it is run for.
// @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
const GLOBAL: &str = "global";

/// Whether a requirement id is a global one: a global id begins with global.
// @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
fn is_global(id: &str) -> bool {
    id == GLOBAL || id.starts_with("global:")
}

/// The ids of every review the last global review found violated, as the agent server's
/// [`GlobalReview`] reads it from `elfie-requests/global.reviews.json` under the root; none
/// when there is no last global review, so that no unit is among its violated units.
///
/// The file it is kept in, what it holds, and which of its reviews are violated belong to the
/// agent server's data rather than here, because both this and the agent server plan with the
/// units the last global review found violated.
// @lfy def/cli/main.lfy:main#main:main:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
fn violated_ids(root: &Path) -> BTreeSet<String> {
    // @lfy def/cli/main.lfy:main#main:main:37aa77cbc42fd4dcc40b7262ac25164abbc8c899273439860ff39c8c39002a0d
    let Some(last) = GlobalReview::read(root) else {
        return BTreeSet::new();
    };
    // @lfy def/cli/main.lfy:main#main:main:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
    last.violated_requirements().into_iter().collect()
}

/// The stems of every unit whose outputs hold a marker whose requirement is one of these
/// ids.
// @lfy def/cli/main.lfy:main#main:main:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
fn stems_answering(plan: &Plan, ids: &BTreeSet<String>) -> Vec<String> {
    if ids.is_empty() {
        return Vec::new();
    }
    let mut stems: Vec<String> = plan
        .units
        .iter()
        .filter(|unit| {
            unit.outputs.iter().any(|map| {
                // @lfy def/cli/main.lfy:main#main:main:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
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
    // @lfy def/cli/main.lfy:main#main:main:37aa77cbc42fd4dcc40b7262ac25164abbc8c899273439860ff39c8c39002a0d
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
    // @lfy def/cli/main.lfy:main#main:main:7293a67c5e6d7c3cb9a59ca62501969dcb425b97ef7cc4ae74b048b61f3d56b0
    let (program, plan) = plan_with(root, maps, requested, &[]);
    // @lfy def/cli/main.lfy:main#main:main:37aa77cbc42fd4dcc40b7262ac25164abbc8c899273439860ff39c8c39002a0d
    let stems = stems_answering(&plan, violated);
    if stems.is_empty() && !all {
        return (program, plan);
    }
    // --all requests every unit.
    // @lfy def/cli/main.lfy:main#main:main:d3dedcdb2bf5414990bf22aff315b8514e496a3ede1fc23133fa128ee0b4ef9d
    let requested: Vec<String> =
        if all { plan.units.iter().map(|unit| unit.stem.clone()).collect() } else { requested.to_vec() };
    plan_with(root, maps, &requested, &stems)
}

/// `elfie-requests/compile.log` under the root, never under an output directory, so no
/// output is ever mistaken for the CLI's own files.
// @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
fn log_paths(root: &Path) -> Vec<PathBuf> {
    vec![requests_directory(root).join("compile.log")]
}

/// `elfie-requests` under the root: where the CLI's own files live, never under an output
/// directory.
// @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
fn requests_directory(root: &Path) -> PathBuf {
    root.join("elfie-requests")
}

/// The problems of an attempt appended to its instructions, so that the batch is run once
/// more knowing what was wrong.
// @lfy def/cli/main.lfy:main#main:main:17f6feee3279ee2519b458a9ca13607a4738441827bb862fc9a71d712ea28691
// @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
fn append_problems(request: &mut Request, problems: &[String]) {
    request.instructions.push_str("\n\n## Problems with the previous attempt\n\n");
    for problem in problems {
        request.instructions.push_str(&format!("- {problem}\n"));
    }
}

/// One problem per violated review: `failure at`, the file, a colon, the line, a colon, the
/// note, and the evidence in parentheses.
// @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
fn problem_of(review: &Review) -> String {
    format!("failure at {}:{}: {} ({})", review.file, review.line, review.note, review.evidence)
}

/// One problem per unplaced violation: a line the verifier wrote whose status is violated but
/// which names no criterion and no test of the program, so there is nowhere to place it.
///
/// `failure at a criterion the verifier did not name`, a colon, the note of the line, and its
/// evidence in parentheses; a line that reads as no object at all is its own note.
// @lfy def/cli/main.lfy:main#main:main:c0aa127519d48d85890761c2ad8369980444d7ae53bf2834596e4e7acc7444df
fn problem_of_unplaced(line: &str) -> String {
    let value: Option<serde_json::Value> = serde_json::from_str(line).ok();
    let field = |name: &str| {
        value.as_ref().and_then(|value| value.get(name)).and_then(|value| value.as_str()).unwrap_or_default().to_string()
    };
    let note = {
        let note = field("note");
        if note.is_empty() { line.trim().to_string() } else { note }
    };
    // @lfy def/cli/main.lfy:main#main:main:c0aa127519d48d85890761c2ad8369980444d7ae53bf2834596e4e7acc7444df
    format!("failure at a criterion the verifier did not name: {note} ({})", field("evidence"))
}

/// Every problem one report's violations make: one per violated review, then one per unplaced
/// violation. Empty when the report violated nothing.
// @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
// @lfy def/cli/main.lfy:main#main:main:c0aa127519d48d85890761c2ad8369980444d7ae53bf2834596e4e7acc7444df
fn violations_of(report: &ReviewReport) -> Vec<String> {
    // @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
    let mut problems: Vec<String> =
        report.reviews.iter().filter(|review| review.status == ReviewStatus::Violated).map(problem_of).collect();
    // @lfy def/cli/main.lfy:main#main:main:c0aa127519d48d85890761c2ad8369980444d7ae53bf2834596e4e7acc7444df
    problems.extend(report.unplaced.iter().map(|line| problem_of_unplaced(line)));
    problems
}

/// One id the review was asked about that no review names, as the line naming it reads.
// @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
fn not_reviewed(id: &str) -> String {
    format!("{id} was not reviewed")
}

/// One line per id the review was asked about that no review names, so that a person reading
/// the reviewed line sees what went unanswered.
// @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
fn missing_lines(report: &ReviewReport) -> String {
    report.missing.iter().map(|id| not_reviewed(id)).collect::<Vec<_>>().join("\n")
}

/// The counts of satisfied, violated, and unverifiable reviews, in that order.
// @lfy def/cli/main.lfy:main#main:main:a401e7319fbb4bdf617fc08adbbe3fea18e30d1c519f1fcbf538029c7dc283a2
// @lfy def/cli/main.lfy:main#main:main:41f684cc85ced11c6439219ff5858457d7c2ff822185af4d70f8ce243afc79ff
fn counts_of(reviews: &[Review]) -> (usize, usize, usize) {
    let count = |status| reviews.iter().filter(|review| review.status == status).count();
    (count(ReviewStatus::Satisfied), count(ReviewStatus::Violated), count(ReviewStatus::Unverifiable))
}

/// What a finished command left: its code, -1 when it ended by a signal or could not be
/// started, everything it wrote, held whole, and whatever it submitted through the agent
/// server before its session was closed.
// @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
struct Exit {
    code: i32,
    stdout: String,
    stderr: String,
    /// What the run recorded through the agent server — the reviews it submitted and the
    /// outcome it submitted — as closing its session gave it back.
    // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
    record: SessionRecord,
    /// The agent session id the run was given as ELFIE_SESSION, so that a resuming run
    /// after it can go on from the same one.
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    session: String,
}

/// Every process group a batch has running: the compiler, the verifier, or a command of the
/// target, so that stopping the batch stops them and everything they started.
// @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
// @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
#[derive(Default)]
struct Running {
    /// The leaders of the groups still running, by the number each was given.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    groups: Mutex<BTreeMap<u64, u32>>,
    /// The number the next group to start takes.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    next: Mutex<u64>,
}

impl Running {
    /// One group remembered while it runs, forgotten when the handle is dropped.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    fn started(&self, leader: u32) -> Group<'_> {
        let at = {
            let mut next = self.next.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let at = *next;
            *next += 1;
            at
        };
        self.groups.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(at, leader);
        Group { running: self, at }
    }

    /// Every group still running stopped, with every process of each.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    fn stop(&self) {
        let leaders: Vec<u32> = {
            let groups = self.groups.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            groups.values().copied().collect()
        };
        for leader in leaders {
            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
            stop_group(leader);
        }
    }
}

/// One process group of a batch while it runs.
// @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
struct Group<'r> {
    running: &'r Running,
    at: u64,
}

impl Drop for Group<'_> {
    /// A run that has ended is no longer a group to stop.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    fn drop(&mut self) {
        self.running
            .groups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.at);
    }
}

/// The agent session a resuming run goes on from: for the compiler and for the verifier
/// apart, the session of that one's first run on the batch in this directory.
///
/// Every resuming run of a chain carries the same id, because a resuming run passes on its
/// own ELFIE_RESUME when it had one and its ELFIE_SESSION otherwise. A batch that runs again
/// from a new copy begins a new chain, since this is made afresh for each directory.
// @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
// @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
#[derive(Default)]
struct Resuming {
    /// The session the compiler's runs on the batch here go on from.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    compiler: Option<String>,
    /// The session the verifier's runs on the batch here go on from.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    verifier: Option<String>,
}

impl Resuming {
    /// What a resuming run of one role goes on from after the run that just ended: that run's
    /// own `ELFIE_RESUME` when it had one, and its `ELFIE_SESSION` otherwise.
    ///
    /// `resumed` is what the run that ended carried, so a run that went on from a session
    /// passes that session on and a fresh run in the middle of a chain begins a new one: after
    /// a run that was killed and a fresh run after it, the next resuming run goes on from the
    /// fresh run rather than from the one before it.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    fn note(&mut self, role: Role, resumed: Option<&str>, session: &str) {
        let chain = match role {
            Role::Compiler => &mut self.compiler,
            Role::Verifier => &mut self.verifier,
        };
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        let id = resumed.or(if session.is_empty() { None } else { Some(session) });
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        if let Some(id) = id {
            *chain = Some(id.to_string());
        }
    }

    /// The id a resuming run of one role goes on from; nothing when that role has not run on
    /// the batch here.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    fn of(&self, role: Role) -> Option<&str> {
        match role {
            Role::Compiler => self.compiler.as_deref(),
            Role::Verifier => self.verifier.as_deref(),
        }
    }
}

/// Every process of a process group stopped, through the platform's own `kill`.
// @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
fn stop_group(leader: u32) {
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    let _ = Process::new("sh")
        .arg("-c")
        .arg(format!("kill -KILL -- -{leader}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// One stream of the compiler or the verifier, each line shown as it arrives prefixed by the
/// batch, appended to the log, and kept whole. With --json it is shown on standard error, so
/// that standard output stays one JSON object per line. Every line of what the command wrote
/// reaches the log, standard error among them, so a run reads after the fact as it read while
/// it ran.
// @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
// @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
fn watch<R: io::Read + Send + 'static>(
    pipe: R,
    batch: &str,
    keep: bool,
    to_stdout: bool,
    on: bool,
    stream: Stream,
    logs: Vec<PathBuf>,
) -> JoinHandle<String> {
    // Two spaces, the batch, a space, a bar, and a space.
    // @lfy def/cli/main.lfy:main#main:main:f747b7755db17f8d8a69c66bccc1af569ec45c9542e442d92a729acb82938344
    let plain = format!("  {batch} \u{2502} ");
    // The prefix of a line that came from standard output is muted, and one that came from
    // standard error is warning.
    // @lfy def/cli/main.lfy:main#main:main:714e27f2331360d1b071e2a8bdf90f345382ea99e062330f8068d835170e9d81
    // @lfy def/cli/main.lfy:main#main:main:f9dd575e7bd97b0a74ebebee78b614a642f4aefa5b071efda9788975324bccbf
    let tone = match stream {
        Stream::Output => Tone::Muted,
        Stream::Error => Tone::Warning,
    };
    let prefix = paint(&plain, tone, on);
    std::thread::spawn(move || {
        let mut kept = String::new();
        for line in io::BufReader::new(pipe).lines().map_while(Result::ok) {
            // The line itself is shown exactly as the command wrote it, as it arrives. One
            // `println!` writes the prefix and the line together under the lock on the
            // stream, so a line of another batch never mixes into it.
            // @lfy def/cli/main.lfy:main#main:main:f747b7755db17f8d8a69c66bccc1af569ec45c9542e442d92a729acb82938344
            // @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
            // @lfy def/cli/main.lfy:main#main:main:3a1f341e1f488489eebade96f9e410811db493057811f2c78e1ceb8d3a8ae77d
            if to_stdout {
                println!("{prefix}{line}");
            } else {
                eprintln!("{prefix}{line}");
            }
            // The line the log holds is the line as printed with colors off.
            // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            append_to(&logs, &format!("{plain}{line}"));
            if keep {
                kept.push_str(&line);
                kept.push('\n');
            }
        }
        kept
    })
}

/// The agent server every run of a compile or a verify works through, hosted on this machine
/// for the length of that command.
///
/// It is hosted when the first agent is about to run and never before, so a command that runs
/// neither the compiler nor the verifier hosts nothing; what hosting it came to is kept, so
/// that the host is made once and a failure to make it is the same failure for every run.
// @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
// @lfy def/cli/main.lfy:main#main:main:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
struct Server {
    /// The root, where every session's MCP config file is written.
    // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
    root: PathBuf,
    /// What hosting came to, once something needed it: the host, or why there is none.
    // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    hosted: Mutex<Option<Result<Arc<Host>, String>>>,
}

impl Server {
    // @lfy def/cli/main.lfy:main
    fn new(root: PathBuf) -> Server {
        Server { root, hosted: Mutex::new(None) }
    }

    /// The server hosted for the length of this command, made the first time a run needs it
    /// and the same one after that.
    // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    // @lfy def/cli/main.lfy:main#main:main:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
    fn host(&self) -> Result<Arc<Host>, String> {
        let mut held = self.hosted.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
        held.get_or_insert_with(|| elfie_mcp::host().map(Arc::new)).clone()
    }

    /// Every session that is still open ended and the server stopped, once the last run has
    /// returned; nothing to stop when nothing was ever hosted.
    // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    fn stop(&self) {
        let mut held = self.hosted.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
        if let Some(Ok(host)) = held.take() {
            (host.stop)();
        }
    }
}

/// The tools that run a target's command, in the order the agent server lists them, so that
/// a run is told which of them its session was offered.
// @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
const COMMAND_TOOLS: [&str; 4] = ["elfie_build", "elfie_test", "elfie_lint", "elfie_format"];

/// The command tools among the names a session is offered, in the order the server lists
/// them, joined by spaces; empty when it was offered none.
// @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
fn command_tools_of(offered: &[String]) -> String {
    // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    offered
        .iter()
        .filter(|name| COMMAND_TOOLS.contains(&name.as_str()))
        .map(String::as_str)
        .collect::<Vec<&str>>()
        .join(" ")
}

/// The word a role goes by in a file name and in the log.
// @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
fn word_of(role: Role) -> &'static str {
    match role {
        Role::Compiler => "compiler",
        Role::Verifier => "verifier",
    }
}

/// The operating system's secure random source, which an agent session id is made from.
// Decision: `/dev/urandom` is the secure random source of the one platform the target lists,
// posix; it is a device the kernel answers from, not configuration, so reading it is not
// configuration read from a fixed absolute path.
// @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
const RANDOM: &str = "/dev/urandom";

/// The name of the variable that carries the agent session id of a run.
// @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
const SESSION: &str = "ELFIE_SESSION";

/// The name of the variable that carries the agent session id a resuming run goes on from.
// @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
const RESUME: &str = "ELFIE_RESUME";

/// An agent session id new for one run: a version 4 UUID made from the operating system's
/// secure random source, spelled as 36 lowercase characters in the 8-4-4-4-12 form.
// @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
fn session_id() -> Result<String, String> {
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    let mut bytes = [0u8; 16];
    fs::File::open(RANDOM)
        .and_then(|mut file| io::Read::read_exact(&mut file, &mut bytes))
        .map_err(|error| format!("the secure random source {RANDOM} cannot be read: {error}"))?;
    Ok(uuid_of(bytes))
}

/// Sixteen random bytes spelled as a version 4 UUID: the version in the high nibble of the
/// seventh byte and the variant in the two high bits of the ninth, then 36 lowercase
/// characters in the 8-4-4-4-12 form.
// @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
fn uuid_of(mut bytes: [u8; 16]) -> String {
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// What running one agent takes, held apart from the rest of a compile so that a batch can
/// stream its compiler and its verifier while the other batches stream theirs: nothing here
/// changes once a compile has begun.
// @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
#[derive(Clone)]
struct Agent {
    json: bool,
    style: Paint,
    logs: Vec<PathBuf>,
    /// The path of the running executable, so the agent server an agent starts is this same
    /// program.
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    elfie: PathBuf,
    /// The agent server every run works through, shared by every batch.
    // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    server: Arc<Server>,
}

impl Agent {
    // @lfy def/cli/main.lfy:main
    fn new(json: bool, style: Paint, logs: Vec<PathBuf>, root: &Path) -> Agent {
        // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
        let elfie = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("elfie"));
        // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
        Agent { json, style, logs, elfie, server: Arc::new(Server::new(root.to_path_buf())) }
    }

    /// The MCP config file of one session, written under the root before the run starts:
    /// `elfie-requests/<batch>.<role>.mcp.json`, holding one object naming the session's URL
    /// and the bearer token every request to it must carry.
    // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
    // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
    fn write_config(&self, host: &Host, url: &str, label: &str, role: Role) -> PathBuf {
        let directory = requests_directory(&self.server.root);
        // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
        let path = directory.join(format!("{}.{}.mcp.json", file_name_of(label), word_of(role)));
        // One object with the key mcpServers, holding one object with the key elfie, holding
        // the type http, the session's URL, and the Authorization header.
        // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
        let value = serde_json::json!({
            "mcpServers": {
                "elfie": {
                    "type": "http",
                    "url": url,
                    "headers": { "Authorization": format!("Bearer {}", host.token) },
                }
            }
        });
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default());
        // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
        if let Err(error) = fs::create_dir_all(&directory).and_then(|()| fs::write(&path, &text)) {
            append_to(&self.logs, &format!("{} could not be written: {error}", path.display()));
        }
        path
    }

    /// Streams one agent, the compiler or the verifier, once for one batch: the instructions
    /// as its input, the batch's directory as its directory, and ELFIE_ROOT (the batch's
    /// directory), ELFIE_BATCH, ELFIE_UNITS (the stems, space separated), ELFIE,
    /// ELFIE_MCP_CONFIG, and ELFIE_COMMAND_TOOLS as its environment. Its standard output is
    /// the report. The code is -1 when the command could not be started or the agent server
    /// could not be hosted.
    ///
    /// One hosted session is opened for the run just before it starts, bound to what the run
    /// is, and closed once it has returned, before anything reads its outcome; what it
    /// submitted through the server comes back with its output. Its config file is removed
    /// once the run has returned, since the token it holds no longer opens anything.
    ///
    /// ELFIE_SESSION is an agent session id new for the run, and ELFIE_RESUME the id a
    /// resuming run goes on from, set only for the runs [`Resuming`] knows of and removed
    /// from what was inherited otherwise.
    ///
    /// These are the whole of what it adds: a batch running in a copy of the root reads
    /// the root's history through the `.git` file the copy holds, so nothing of git is set
    /// here and a repository one of its own commands makes elsewhere stays its own.
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
    // @lfy def/cli/main.lfy:main#main:main:dc6670a1659abe98c2ae0ed1502d175a57162b9aa395c843f4fdeaa3d5ce4664
    // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    #[expect(clippy::too_many_arguments, reason = "one run of an agent takes all of this")]
    fn run(
        &self,
        command: &str,
        where_: &Where,
        label: &str,
        units: &str,
        instructions: &str,
        binding: Binding,
        resume: Option<&str>,
        running: Option<&Running>,
    ) -> Exit {
        let directory = where_.directory();
        let role = binding.role;
        let what = word_of(role);
        // An agent session id new for the run, read from the secure random source; a source
        // that cannot be read is a run that could not be set up.
        // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
        let session_id = match session_id() {
            Ok(id) => id,
            // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
            Err(error) => {
                return Exit {
                    code: -1,
                    stdout: String::new(),
                    stderr: error,
                    record: SessionRecord::default(),
                    session: String::new(),
                };
            }
        };
        // The agent server is hosted the first time a run needs it; nothing runs when it
        // cannot be hosted, and its failure is what the run came to.
        // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        let host = match self.server.host() {
            Ok(host) => host,
            // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
            Err(error) => {
                return Exit {
                    code: -1,
                    stdout: String::new(),
                    stderr: format!("the agent server could not be hosted: {error}"),
                    record: SessionRecord::default(),
                    session: session_id,
                };
            }
        };
        // One hosted session for this run, bound to what it is, opened just before it starts.
        // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
        let session = (host.open)(binding);
        // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
        let config = self.write_config(&host, &session, label, role);
        // Which of the command tools this session was offered, in the order the server lists
        // them, so that a run knows what to call instead of knowing the tools.
        // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        let tools = command_tools_of(&(host.offered)(&session));
        let mut spawning = Process::new("sh");
        spawning
            .arg("-c")
            .arg(command)
            .current_dir(directory)
            // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
            .env("ELFIE_ROOT", directory)
            // ELFIE_BATCH is the identifier of the batch and ELFIE_UNITS its stems, separated
            // by spaces.
            // @lfy def/cli/main.lfy:main#main:main:d998aab5e2cced5f970810a3173862fcd78e571f2000c43dd4bdddaefa571c0b
            .env("ELFIE_BATCH", label)
            .env("ELFIE_UNITS", units)
            // ELFIE_ROOT is the directory it runs in, ELFIE the path of the running
            // executable, ELFIE_MCP_CONFIG the config file of this run's hosted session, and
            // ELFIE_COMMAND_TOOLS the command tools that session was offered; no git variable
            // is added beyond these six.
            // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            // @lfy def/cli/main.lfy:main#main:main:e17f6b936896c49f2d5ae4a8c46ccaf3f569ef4620c0e0e927b573171cbde588
            .env("ELFIE", &self.elfie)
            // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            .env("ELFIE_MCP_CONFIG", &config)
            // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            .env("ELFIE_COMMAND_TOOLS", &tools)
            // The agent session id of this run.
            // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
            .env(SESSION, &session_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // A resuming run goes on from the session of the run of the same one of them on the
        // batch before it; every other run has no ELFIE_RESUME at all, whatever the
        // environment the command was started in held.
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        match resume {
            // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
            Some(before) => spawning.env(RESUME, before),
            // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
            None => spawning.env_remove(RESUME),
        };
        // The run leads a process group of its own, so that stopping it stops everything it
        // started.
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        std::os::unix::process::CommandExt::process_group(&mut spawning, 0);
        // The command could not be started: the code is -1 and the failure is the output.
        // @lfy def/cli/main.lfy:main#main:main:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
        let mut child = match spawning.spawn() {
            Ok(child) => child,
            Err(error) => {
                // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
                let record = (host.close)(&session);
                // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
                remove_config(&config, &self.logs);
                return Exit {
                    code: -1,
                    stdout: String::new(),
                    stderr: error.to_string(),
                    record,
                    session: session_id,
                };
            }
        };
        // The group this run leads is remembered while it runs, so that stopping the batch
        // stops it and every process it started.
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        let group = running.map(|running| running.started(child.id()));
        // Each line is shown as it arrives, prefixed by the batch; with --json it goes to
        // standard error, so standard output stays one JSON object per line.
        // @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
        let logs = self.logs.clone();
        let out = child
            .stdout
            .take()
            .map(|pipe| watch(pipe, label, true, !self.json, self.style.out, Stream::Output, logs.clone()));
        // Standard error reaches the log as standard output does, so nothing the command
        // wrote is lost to a run read after the fact.
        // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
        let err = child
            .stderr
            .take()
            .map(|pipe| watch(pipe, label, true, false, self.style.err, Stream::Error, logs.clone()));
        if let Some(mut stdin) = child.stdin.take()
            && let Err(error) = stdin.write_all(instructions.as_bytes())
        {
            append_to(&logs, &format!("the {what} command did not read its input: {error}"));
        }
        let status = child.wait();
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        drop(group);
        let stdout = out.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        let stderr = err.map(|handle| handle.join().unwrap_or_default()).unwrap_or_default();
        // The run has returned: its session is closed, and what it submitted through the
        // server comes back with its output, before anything reads its outcome. Its config
        // file goes with the session, whose token no longer opens anything.
        // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
        let record = (host.close)(&session);
        // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
        remove_config(&config, &logs);
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                return Exit { code: -1, stdout, stderr: error.to_string(), record, session: session_id };
            }
        };
        if !status.success() {
            append_to(&logs, &format!("the {what} command exited with {status}"));
        }
        Exit { code: status.code().unwrap_or(-1), stdout, stderr, record, session: session_id }
    }
}

/// What every run of the compiler and the verifier for one batch in one directory shares:
/// the agent, the process groups the batch has running, and the sessions a resuming run goes
/// on from.
// @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
// @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
struct Runs {
    agent: Agent,
    /// The process groups the batch has running, shared with the compile so that a stopped
    /// compile stops them.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    running: Arc<Running>,
    /// The sessions a resuming run of each role goes on from, in this directory.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    resuming: Mutex<Resuming>,
}

impl Runs {
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    fn new(agent: Agent, running: Arc<Running>) -> Runs {
        Runs { agent, running, resuming: Mutex::new(Resuming::default()) }
    }

    /// One run of the compiler or the verifier for the batch: a resuming run carries the
    /// session of that role's run on the batch before it here, and every run is remembered
    /// while it runs so that stopping the batch stops it.
    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
    // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
    #[expect(clippy::too_many_arguments, reason = "one run of an agent takes all of this")]
    fn run(
        &self,
        command: &str,
        where_: &Where,
        label: &str,
        units: &str,
        instructions: &str,
        binding: Binding,
        resuming: bool,
    ) -> Exit {
        let role = binding.role;
        // A resuming run goes on from the session of that role's first run on the batch here;
        // every other run carries none.
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        let resume = if resuming {
            let held = self.resuming.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            held.of(role).map(str::to_string)
        } else {
            None
        };
        let exit = self.agent.run(
            command,
            where_,
            label,
            units,
            instructions,
            binding,
            resume.as_deref(),
            Some(&self.running),
        );
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        self.resuming
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .note(role, resume.as_deref(), &exit.session);
        exit
    }
}

/// The MCP config file of a run that has returned, removed: the token it holds no longer
/// opens anything, so nothing is left behind that reads as a way in.
// @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
fn remove_config(config: &Path, logs: &[PathBuf]) {
    // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
    if let Err(error) = fs::remove_file(config)
        && error.kind() != io::ErrorKind::NotFound
    {
        append_to(logs, &format!("{} could not be removed: {error}", config.display()));
    }
}

/// How long a command of the target may run before it and every process of its group are
/// stopped, as the command tools stop one.
// @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
const COMMAND_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// How often a running command of the target is looked in on while its deadline runs.
// @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
const LOOKED_IN_ON: std::time::Duration = std::time::Duration::from_millis(50);

/// The line a target runs for one operation, when it has one for it.
// @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
fn line_for(target: &Target, operation: Operation) -> Option<&str> {
    // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
    target
        .commands
        .iter()
        .find(|command| command.operation == operation)
        .and_then(|command| command.line.as_deref())
}

/// The word an operation of a target goes by in a progress line.
// @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
fn word_of_operation(operation: Operation) -> &'static str {
    match operation {
        Operation::Install => "install",
        Operation::Add => "add",
        Operation::Build => "build",
        Operation::Test => "test",
        Operation::Lint => "lint",
        Operation::Format => "format",
        Operation::Run => "run",
    }
}

/// One run of a command of a target on a batch, as `elfie_build` or `elfie_test` runs it
/// given no filter and no scope: through the platform shell, with no input, in the target's
/// output directory under the batch's directory, with RUN_SCRIPT set to the target's script
/// runner or the empty string and TESTS and SCOPE set to the empty string.
///
/// It and every process of its group are stopped thirty minutes after it started if it has
/// not ended, and it succeeds when its exit code is 0. Every line it wrote reaches the log.
// @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
fn run_target_command(
    target: &Target,
    operation: Operation,
    directory: &Path,
    logs: &[PathBuf],
    running: Option<&Running>,
) -> Option<i32> {
    // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
    let line = line_for(target, operation)?;
    // The command runs in the target's output directory under the batch's directory.
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    let within = directory.join(&target.output_directory);
    let mut spawning = Process::new("sh");
    spawning
        .arg("-c")
        .arg(line)
        .current_dir(&within)
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        .env("RUN_SCRIPT", target.script_runner.clone().unwrap_or_default())
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        .env("TESTS", "")
        .env("SCOPE", "")
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The command leads a process group of its own, so everything it starts is stopped with
    // it when it runs out of time or the batch is stopped.
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    std::os::unix::process::CommandExt::process_group(&mut spawning, 0);
    let mut child = match spawning.spawn() {
        Ok(child) => child,
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        Err(error) => {
            append_to(logs, &format!("the {} command could not be run: {error}", word_of_operation(operation)));
            return Some(-1);
        }
    };
    let leader = child.id();
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    let group = running.map(|running| running.started(leader));
    let label = format!("{} {}", target.identifier, word_of_operation(operation));
    let out = child.stdout.take().map(|pipe| logged(pipe, &label, logs.to_vec()));
    let err = child.stderr.take().map(|pipe| logged(pipe, &label, logs.to_vec()));
    let started = std::time::Instant::now();
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {}
            Err(_) => break -1,
        }
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        if started.elapsed() >= COMMAND_DEADLINE {
            stop_group(leader);
            let _ = child.kill();
            let _ = child.wait();
            append_to(logs, &format!("{label} was stopped after 30 minutes"));
            break -1;
        }
        std::thread::sleep(LOOKED_IN_ON);
    };
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    drop(group);
    for handle in [out, err].into_iter().flatten() {
        let _ = handle.join();
    }
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    Some(code)
}

/// One stream of a command of the target, every line of it appended to the log under the
/// name of the command, so that a run read after the fact holds what the command wrote.
// @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
fn logged<R: io::Read + Send + 'static>(pipe: R, label: &str, logs: Vec<PathBuf>) -> JoinHandle<()> {
    let prefix = format!("  {label} \u{2502} ");
    std::thread::spawn(move || {
        for line in io::BufReader::new(pipe).lines().map_while(Result::ok) {
            append_to(&logs, &format!("{prefix}{line}"));
        }
    })
}

/// Where elfie-compile keeps a batch's own copy of the root.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
const BATCHES: &str = "elfie-compile/cache/batches";

/// What a copy of the root leaves behind: the cache the copies live in, and the folder the
/// CLI writes its own files to under the root.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
const UNCOPIED: [&str; 2] = ["elfie-compile/cache", "elfie-requests"];

/// What a merge leaves alone: elfie-compile, whose maps are recorded in the root once the
/// merge is done, elfie-requests, whose files are written there all along, and the copy's own
/// `.git` file, which is the copy's and never the root's.
// @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
const UNMERGED: [&str; 3] = ["elfie-compile", "elfie-requests", GITFILE];

/// Whether a path is under one of these folders, which are spelled relative to the root with
/// forward slashes as the copy spells its own paths.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
fn under(path: &str, folders: &[&str]) -> bool {
    folders.iter().any(|folder| path == *folder || path.starts_with(&format!("{folder}/")))
}

/// What `git` wrote in a directory, when it ran and ended well.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output =
        Process::new("git").args(arguments).current_dir(directory).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The root's git directory, when the root is a git repository, as an absolute path, so that
/// the `.git` file of a copy names the same place from wherever the copy sits.
// @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
fn git_directory(root: &Path) -> Option<PathBuf> {
    git(root, &["rev-parse", "--absolute-git-dir"]).map(|text| PathBuf::from(text.trim()))
}

/// The one file that tells git in a copy of the root which repository it is looking at, named
/// as the copy spells its own paths.
// @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
const GITFILE: &str = ".git";

/// Whether a file is executable, as the mode the platform keeps it in; nothing where the
/// platform has no such mode, and nothing for a file that cannot be read.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
#[cfg(unix)]
fn mode_of(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::metadata(path).ok().map(|data| data.permissions().mode())
}

// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
#[cfg(not(unix))]
fn mode_of(_path: &Path) -> Option<u32> {
    None
}

/// A file left in the mode its source was in, so a script a batch copied or merged can still
/// be run.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt as _;
    if let Some(mode) = mode {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
}

// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) {}

/// One batch's own copy of the root: the directory its compiler and its verifier run in, and
/// what each copied file held when the copy was taken, which is the batch's base.
///
/// A copy exists because compilers editing one tree break each other's builds and tests. A
/// batch's work reaches the root only once its outputs are accepted and reviewed, merged file
/// by file, so the root only ever holds work that passed and the root holds none of a batch's
/// changes while it is not done.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
// @lfy def/cli/main.lfy:main#main:main:241b41c6ff82ebbd898b2d723f95b5c8d5928b57aa500ba825c546edeaccc6a1
struct Mirror {
    directory: PathBuf,
    /// What each copied file held, by its path relative to the root.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    base: BTreeMap<String, Vec<u8>>,
}

impl Mirror {
    /// Every file git lists in the root, except those under the cache and elfie-requests,
    /// copied byte for byte to the same path under `elfie-compile/cache/batches/<batch>`.
    /// Nothing is copied when the root is no git repository, since there is then no list of
    /// what belongs to the project.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    fn take(root: &Path, identifier: &str) -> Option<Mirror> {
        let git_directory = git_directory(root)?;
        // Every file of the root that git tracks or would track, as git ls-files --cached
        // --others --exclude-standard lists them.
        // @lfy def/cli/main.lfy:main#main:main:36bc370dd66b7ca611fe08e5c76eaae494bbb41af5a20c0a009308069c1ac3c5
        let listed = git(root, &["ls-files", "--cached", "--others", "--exclude-standard"])?;
        let directory = root.join(BATCHES).join(file_name_of(identifier));
        // A directory left at the copy's path by an earlier compile is removed before the
        // copy is made, so that nothing of it is taken for the batch's own work.
        // @lfy def/cli/main.lfy:main#main:main:168f11962e12b9d11505b3c6db25a6b72cc753f958945a34524ffce7ab7c2d43
        let _ = fs::remove_dir_all(&directory);
        let mut base = BTreeMap::new();
        for path in listed.lines() {
            let path = path.trim();
            // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
            if path.is_empty() || under(path, &UNCOPIED) {
                continue;
            }
            // A file git lists that is not there, because it was removed since, is nothing
            // to copy.
            // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
            let from = root.join(path);
            let Ok(held) = fs::read(&from) else { continue };
            let to = directory.join(path);
            // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
            if let Some(parent) = to.parent()
                && fs::create_dir_all(parent).is_err()
            {
                return None;
            }
            // Each file is copied to the same path in the copy, byte for byte.
            // @lfy def/cli/main.lfy:main#main:main:6d5d0a607c1fba3bb9c583e43d16675dc224689f89716341f1f894f0ca7f0586
            if fs::write(&to, &held).is_err() {
                return None;
            }
            // The copy keeps whether the file is executable, so a script the compiler is run
            // through can still be started in the copy.
            // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
            // @lfy def/cli/main.lfy:main#main:main:6d5d0a607c1fba3bb9c583e43d16675dc224689f89716341f1f894f0ca7f0586
            set_mode(&to, mode_of(&from));
            // What each copied file held when the batch started is kept as the batch's base.
            // @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
            base.insert(path.to_string(), held);
        }
        // The copy is pointed at the root's own repository by a `.git` file, so that git run
        // in it reads the root's history and its index, and nothing of git has to be put in
        // the environment of anything the batch runs.
        // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
        if fs::create_dir_all(&directory).is_err()
            || fs::write(
                directory.join(GITFILE),
                format!("gitdir: {}\n", git_directory.to_string_lossy()),
            )
            .is_err()
        {
            return None;
        }
        Some(Mirror { directory, base })
    }

    /// Every file of the copy outside elfie-compile, elfie-requests, and the copy's own
    /// `.git` file that differs from the base, was added, or was removed, with what the base
    /// held for it and what the copy holds, in path order: the batch's changes, which are
    /// what a merge into the root writes and what its accepted changes are taken from.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    fn changes(&self) -> Changes {
        let mut now: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        // What mode each file of the copy is in, so a file merged into the root keeps whether
        // it is executable.
        // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
        let mut modes: BTreeMap<String, Option<u32>> = BTreeMap::new();
        // What the copy is compared with the base over: every path the base holds, which is
        // every path the copy was made from, and every file git lists as added in the copy.
        // A walk of the copy would have to decide for itself which directories hold work and
        // which hold nothing, and a wrong guess reads a file that is there as removed.
        // @lfy def/cli/main.lfy:main#main:main:83b890265a80eabfeb07845b755aca4a5a8ce09a8e540c54a21997798a0c554a
        let mut found: BTreeSet<String> = self.base.keys().cloned().collect();
        // A file the batch added counts only when git would track it.
        // @lfy def/cli/main.lfy:main#main:main:b68cee4a0de41c4715a35c4aa1fd6d7f394b55f3833e77d16dd14f07c3893164
        let added = git(&self.directory, &["ls-files", "--others", "--exclude-standard"]);
        for path in added.unwrap_or_default().lines() {
            let path = path.trim();
            if !path.is_empty() {
                found.insert(path.to_string());
            }
        }
        // The paths of the base the copy no longer holds anything at.
        // @lfy def/cli/main.lfy:main#main:main:83b890265a80eabfeb07845b755aca4a5a8ce09a8e540c54a21997798a0c554a
        let mut gone: BTreeSet<String> = BTreeSet::new();
        for path in found {
            // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
            if under(&path, &UNMERGED) {
                continue;
            }
            let at = self.directory.join(&path);
            match fs::read(&at) {
                // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
                Ok(held) => {
                    modes.insert(path.clone(), mode_of(&at));
                    now.insert(path, held);
                }
                // A path counts as removed only when nothing is at it in the copy; one that
                // is there and cannot be read is left out of the merge altogether, so the
                // root's file stands rather than going.
                // @lfy def/cli/main.lfy:main#main:main:83b890265a80eabfeb07845b755aca4a5a8ce09a8e540c54a21997798a0c554a
                Err(_) if !at.exists() => {
                    gone.insert(path);
                }
                Err(_) => {}
            }
        }
        // What was added, what differs, and what was removed.
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        let mut paths: BTreeSet<&String> = now.keys().collect();
        paths.extend(gone.iter());
        let mut changes: Changes = BTreeMap::new();
        for path in paths {
            let base = self.base.get(path).cloned();
            let text = now.get(path).cloned();
            // The batch left it as it was: there is nothing of it to merge.
            // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
            // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
            if base.as_deref() == text.as_deref() {
                continue;
            }
            let mode = modes.get(path).copied().flatten();
            // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
            changes.insert(path.clone(), Change { base, text, mode });
        }
        changes
    }

    /// Every change of the batch merged into the root: the paths merged, or the one file
    /// that could not be merged without a conflict.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    fn merge(&self, root: &Path) -> Result<Vec<String>, String> {
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        merge_changes(&self.changes(), root, &self.scratch())
    }

    /// Where the three sides of a merge are written: beside the copy's own cache, which no
    /// merge reads.
    // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
    fn scratch(&self) -> PathBuf {
        self.directory.join("elfie-compile/cache/merge")
    }

    /// What each file of the copy holds now kept as the batch's base: what it is after the
    /// accepted changes it takes are merged in, and before any kept work is taken.
    // @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
    fn rebase(&mut self) {
        // @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
        for (path, change) in self.changes() {
            match change.text {
                Some(text) => {
                    self.base.insert(path, text);
                }
                None => {
                    self.base.remove(&path);
                }
            }
        }
    }

    /// The copy removed, once its work has reached the root.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    fn remove(&self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// What one file of a batch's copy holds and what its base held: one of the batch's changes.
// @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
#[derive(Clone, PartialEq, Eq)]
struct Change {
    /// What the base held for it; nothing when the batch added it.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    base: Option<Vec<u8>>,
    /// What the copy holds for it; nothing when the batch removed it.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    text: Option<Vec<u8>>,
    /// Whether the copy's file is executable, so the file merged keeps it.
    // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
    mode: Option<u32>,
}

/// The changes of one batch, by path, in path order.
// @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
type Changes = BTreeMap<String, Change>;

/// Every change merged into a directory, file by file: where the directory's file still holds
/// what the base holds it becomes the change's, or is removed when the change removed it, and
/// where something changed it since, the directory's file holds the changes of both, as the
/// three-way merge of the two against the base.
///
/// Nothing is written until every file has merged, so a conflict merges nothing at all, and
/// the one file that could not be merged is what it gives back instead.
// @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
// @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
// @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
fn merge_changes(changes: &Changes, into: &Path, scratch: &Path) -> Result<Vec<String>, String> {
    let mut writes: Vec<(String, Option<Vec<u8>>, Option<u32>)> = Vec::new();
    for (path, change) in changes {
        let theirs = fs::read(into.join(path)).ok();
        // The file still holds what the base holds: it is replaced by the change's text, or
        // removed when the change removed it.
        // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
        if theirs.as_deref() == change.base.as_deref() {
            // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
            writes.push((path.clone(), change.text.clone(), change.mode));
            continue;
        }
        // Something else changed the file since the base was taken: it holds the changes of
        // both, as the three-way merge of it and the change's text against the base. One of
        // the three missing is a file added or removed on both sides, which no merge settles.
        // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
        let (Some(base), Some(mine), Some(theirs)) =
            (change.base.as_deref(), change.text.as_deref(), theirs.as_deref())
        else {
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            return Err(path.clone());
        };
        // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
        match merged(theirs, base, mine, scratch) {
            Some(text) => writes.push((path.clone(), Some(text), change.mode)),
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            None => return Err(path.clone()),
        }
    }
    let mut written = Vec::new();
    for (path, text, mode) in writes {
        let to = into.join(&path);
        match text {
            // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
            Some(text) => {
                // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
                if let Some(parent) = to.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if fs::write(&to, &text).is_err() {
                    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
                    return Err(path);
                }
                // The file merged keeps whether the change's was executable.
                // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
                set_mode(&to, mode);
            }
            // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
            None => {
                let _ = fs::remove_file(&to);
            }
        }
        written.push(path);
    }
    Ok(written)
}

/// The three-way merge of two sides against their base, as `git merge-file` gives it;
/// nothing when it conflicts.
// @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
fn merged(theirs: &[u8], base: &[u8], mine: &[u8], scratch: &Path) -> Option<Vec<u8>> {
    // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
    fs::create_dir_all(scratch).ok()?;
    let sides = [("theirs", theirs), ("base", base), ("mine", mine)];
    for (name, held) in sides {
        fs::write(scratch.join(name), held).ok()?;
    }
    // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
    let output = Process::new("git")
        .args(["merge-file", "-p", "--quiet", "theirs", "base", "mine"])
        .current_dir(scratch)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let _ = fs::remove_dir_all(scratch);
    // A code of 0 is a clean merge; anything above it counts the conflicts, and anything
    // below it is git itself failing.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    (output.status.code() == Some(0)).then_some(output.stdout)
}

/// Where elfie-compile keeps the work a rejected batch's copy left behind, so the next
/// compile of the batch starts from it rather than from nothing.
///
/// It lives in the cache, and nothing else does: a command run after the cache is deleted
/// makes the same requests as one run with it, except that a batch that would have taken
/// kept work is handed the outputs the root holds instead.
// @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
// @lfy def/cli/main.lfy:main#main:main:b7b25cb5b407b29319dfe9ee6e8a2d89d37f8d764c616dcbb3ddc2e6a43c0213
const KEPT: &str = "elfie-compile/cache/kept";

/// One unit of a batch as its kept work names it: what it is and what it was asked for, so
/// that work kept for one program is never taken for another.
// @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
struct KeptUnit {
    /// [`Unit::stem`].
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    stem: String,
    /// The SHA-256 of [`Unit::text`] as lowercase hex.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    hash: String,
    /// The SHA-256 of the unit's local criterion and test ids, as
    /// [`SourceMap::requirements`] takes them.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    requirements: String,
}

/// One file of a batch's kept work: what the batch's base held for it and what its copy held.
// @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
struct KeptFile {
    path: String,
    /// What the base held for it; nothing when the batch added it.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    base: Option<String>,
    /// What the copy held for it; nothing when the batch removed it.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    text: Option<String>,
}

/// The work a batch's copy held when a run rejected it and nothing ever accepted it: what it
/// was for, what was wrong with it, and what its files held.
// @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
struct Kept {
    /// [`Target::identifier`] of the batch's target.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    target: String,
    /// Each unit of the batch, in batch order.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    units: Vec<KeptUnit>,
    /// The problems of the rejection that stopped the batch, in the order they were printed.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    problems: Vec<String>,
    /// Each file that would have been merged into the root were the batch done, in path
    /// order.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    files: Vec<KeptFile>,
}

impl Kept {
    /// The kept work of a batch as one JSON object with the keys target, units, problems, and
    /// files.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    fn to_json(&self) -> serde_json::Value {
        // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
        serde_json::json!({
            "target": self.target,
            "units": self
                .units
                .iter()
                .map(|unit| serde_json::json!({
                    "stem": unit.stem,
                    "hash": unit.hash,
                    "requirements": unit.requirements,
                }))
                .collect::<Vec<serde_json::Value>>(),
            "problems": self.problems,
            "files": self
                .files
                .iter()
                .map(|file| serde_json::json!({
                    "path": file.path,
                    "base": file.base,
                    "text": file.text,
                }))
                .collect::<Vec<serde_json::Value>>(),
        })
    }

    /// The kept work one file holds, when it reads as one.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    fn of_json(value: &serde_json::Value) -> Option<Kept> {
        let object = value.as_object()?;
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let units = object
            .get("units")?
            .as_array()?
            .iter()
            .map(|unit| {
                Some(KeptUnit {
                    stem: unit.get("stem")?.as_str()?.to_string(),
                    hash: unit.get("hash")?.as_str()?.to_string(),
                    requirements: unit.get("requirements")?.as_str()?.to_string(),
                })
            })
            .collect::<Option<Vec<KeptUnit>>>()?;
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let files = object
            .get("files")?
            .as_array()?
            .iter()
            .map(|file| {
                Some(KeptFile {
                    path: file.get("path")?.as_str()?.to_string(),
                    base: file.get("base").and_then(|held| held.as_str()).map(str::to_string),
                    text: file.get("text").and_then(|held| held.as_str()).map(str::to_string),
                })
            })
            .collect::<Option<Vec<KeptFile>>>()?;
        Some(Kept {
            target: object.get("target")?.as_str()?.to_string(),
            units,
            problems: object
                .get("problems")
                .and_then(|held| held.as_array())
                .map(|held| held.iter().filter_map(|one| one.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            files,
        })
    }

    /// Whether the kept work is the batch's own: it names the batch's target and the stems of
    /// its units in batch order, and gives each unit the hash and the requirements that are
    /// its own now.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    // @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
    fn is_for(&self, asked: &Kept) -> bool {
        // @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
        self.target == asked.target
            && self.units.len() == asked.units.len()
            && self.units.iter().zip(&asked.units).all(|(held, want)| {
                held.stem == want.stem && held.hash == want.hash && held.requirements == want.requirements
            })
    }

    /// The stems the kept work names.
    // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
    fn stems(&self) -> Vec<&str> {
        self.units.iter().map(|unit| unit.stem.as_str()).collect()
    }
}

/// The file a batch's kept work is written to, under the root.
// @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
fn kept_file(root: &Path, identifier: &str) -> PathBuf {
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    root.join(KEPT).join(format!("{}.json", file_name_of(identifier)))
}

/// Where one batch's compiler, verifier, and outputs are: the root itself, or a copy of it.
// @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
enum Where {
    /// The root itself, which is where a batch runs with --jobs 1 or outside a git
    /// repository, since nothing else is running to break.
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    Root(PathBuf),
    /// Its own copy of the root.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    Copy(Mirror),
}

impl Where {
    /// Where a batch runs: a copy of the root when more than one batch may run at once and
    /// the root is a git repository, and the root itself otherwise.
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    fn of(root: &Path, identifier: &str, copies: bool) -> Where {
        // The batch's directory is the root itself.
        // @lfy def/cli/main.lfy:main#main:main:cab5d4506292d57bf47d916ae73f8496c6abca998665c7cf57c137c631bd51e0
        if !copies {
            return Where::Root(root.to_path_buf());
        }
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        match Mirror::take(root, identifier) {
            Some(mirror) => Where::Copy(mirror),
            // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
            None => Where::Root(root.to_path_buf()),
        }
    }

    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    fn directory(&self) -> &Path {
        match self {
            Where::Root(root) => root,
            Where::Copy(mirror) => &mirror.directory,
        }
    }
}

/// How a batch ended, as the schedule reads it.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
enum Done {
    /// Its work is in the root: nothing that depends on it waits any longer.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    Merged,
    /// A file of it could not be merged without a conflict, so it runs again from a new copy
    /// once no other batch is running; this is not a rejection.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    Conflicted,
    /// It stopped without its files reaching the root, so a batch whose copy took its
    /// accepted changes stops too.
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    Stopped,
}

/// What has become of one batch of a round.
// @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// It has not started, or it is to run again.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    Waiting,
    /// It is running.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    Running,
    /// Every unit of it was accepted by `accept`, and it is neither running the compiler nor
    /// about to run it again; its files are not in the root yet.
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    Settled,
    /// Its files are in the root.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    Merged,
    /// It stopped without its files reaching the root.
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    Stopped,
}

/// Which batches of a round may start: a batch starts once every batch holding a dependency
/// of one of its units is done, at most --jobs run at once, and among those that can start
/// they start in plan order.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
struct Planned {
    batch: Batch,
    /// The places in the round of the batches holding a dependency of one of its units.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    after: Vec<usize>,
}

/// What a round of a compile has started, finished, and has left to start.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
#[derive(Default)]
struct Schedule {
    /// The places of the batches not started yet, in plan order.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    waiting: Vec<usize>,
    running: usize,
    /// What has become of each batch of the round, by its place.
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    states: Vec<State>,
    /// The accepted changes of each batch that has any: each file of its copy that differs
    /// from its base, was added, or was removed, with what the copy held for it the last time
    /// every unit of the batch was accepted.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    accepted: BTreeMap<usize, Changes>,
    /// The places of the batches that must run with nothing else, because a file of theirs
    /// could not be merged.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    alone: BTreeSet<usize>,
    /// Whether a batch that must run with nothing else is running.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    exclusive: bool,
}

/// What a round does next.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
enum Next {
    /// Start the batch at this place.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    Start(usize),
    /// Wait for a running batch to finish.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    Wait,
    /// Nothing is running and nothing can start.
    // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
    Over,
}

impl Schedule {
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    fn of(planned: &[Planned]) -> Schedule {
        Schedule {
            waiting: (0..planned.len()).collect(),
            // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
            states: vec![State::Waiting; planned.len()],
            ..Schedule::default()
        }
    }

    /// Whether a batch a waiting one depends on has come far enough to let it start: its
    /// files are in the root, or it stopped, so nothing of it will ever reach the root.
    ///
    /// With copies, a batch whose every unit was accepted and that is neither running the
    /// compiler nor about to run it again is far enough too: what it has accepted no longer
    /// changes, so a copy may take it before its review ends and its files are merged.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    fn far_enough(&self, at: usize, copies: bool) -> bool {
        match self.states.get(at).copied().unwrap_or(State::Waiting) {
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            State::Merged | State::Stopped => true,
            // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
            State::Settled => copies,
            State::Waiting | State::Running => false,
        }
    }

    /// The batch to start, in plan order among those that can: one whose every batch holding
    /// a dependency of one of its units, directly or through the units of other batches, has
    /// come far enough, while fewer than --jobs are running.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    fn next(&mut self, planned: &[Planned], jobs: usize, halted: bool, copies: bool) -> Next {
        if self.waiting.is_empty() || halted {
            // The compile stopped: no batch starts after that, and the batches already
            // running finish as they would.
            // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
            return if self.running == 0 { Next::Over } else { Next::Wait };
        }
        // At most --jobs batches are running the compiler, the verifier, or a command of the
        // target at once, since a batch runs one of them from the moment it starts until it
        // is done.
        // @lfy def/cli/main.lfy:main#main:main:dea9d85906c909cb3fc6dbc07d671a677ca474f87b64441b5bbd26a79c78c24b
        if self.running >= jobs || self.exclusive {
            return Next::Wait;
        }
        // Among the batches that can start, they start in plan order.
        // @lfy def/cli/main.lfy:main#main:main:97db05bc4c7806f4b6fdeff9a55295d8abf986349e700879098a18335620e22a
        let found = self.waiting.iter().position(|&at| {
            // A batch that must run with nothing else waits until nothing else is running.
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            if self.alone.contains(&at) && self.running > 0 {
                return false;
            }
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
            planned[at].after.iter().all(|&waited| self.far_enough(waited, copies))
        });
        match found {
            Some(place) => {
                let at = self.waiting.remove(place);
                self.running += 1;
                // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                self.states[at] = State::Running;
                // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
                self.exclusive = self.alone.contains(&at);
                Next::Start(at)
            }
            // Nothing can start: either a running batch will let one, or the batches left
            // wait on a batch that never finished.
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            None if self.running > 0 => Next::Wait,
            // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
            None => Next::Over,
        }
    }

    /// One batch's every unit accepted and nothing about to run the compiler on it again: its
    /// accepted changes are what a copy of a batch depending on it takes from here.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    fn settled(&mut self, at: usize, changes: Changes) {
        // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
        self.accepted.insert(at, changes);
        // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
        if self.states[at] == State::Running {
            self.states[at] = State::Settled;
        }
    }

    /// A batch that was done running again: it is no longer done, and nothing may take it as
    /// far enough until it settles once more.
    // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
    fn runs_again(&mut self, at: usize) {
        // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
        self.accepted.remove(&at);
        self.states[at] = State::Running;
    }

    /// One batch finished: done, stopped, or waiting to run again from a new copy with
    /// nothing else running.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    fn finish(&mut self, at: usize, done: &Done) {
        self.running -= 1;
        self.exclusive = false;
        match done {
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            Done::Merged => {
                self.states[at] = State::Merged;
            }
            // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
            Done::Stopped => {
                self.states[at] = State::Stopped;
                self.accepted.remove(&at);
            }
            // It runs again from a new copy once no other batch is running; its place goes
            // back among those waiting, in plan order.
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            Done::Conflicted => {
                self.alone.insert(at);
                self.states[at] = State::Waiting;
                self.accepted.remove(&at);
                let place = self.waiting.iter().position(|&other| other > at).unwrap_or(self.waiting.len());
                self.waiting.insert(place, at);
            }
        }
    }

    /// The batches of a round that never ran, because what they wait on never finished.
    // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
    fn unstarted(&self) -> Vec<usize> {
        self.waiting.clone()
    }
}

/// For each batch of a round, the places of the batches holding a dependency of one of its
/// units, directly or through the units of other batches.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
// @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
fn round_of(plan: &Plan, batches: &[usize]) -> Vec<Planned> {
    let mut planned: Vec<Planned> = Vec::new();
    for &index in batches {
        let batch = plan.batches[index].clone();
        // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
        let needed: BTreeSet<usize> =
            batch.units.iter().flat_map(|&unit| plan.units[unit].dependencies.iter().copied()).collect();
        // A batch holds a dependency of one of its units directly, or through the units of
        // the batches that one depends on, which are all earlier in plan order.
        // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
        let mut after: BTreeSet<usize> = BTreeSet::new();
        for (at, earlier) in planned.iter().enumerate() {
            if earlier.batch.units.iter().any(|unit| needed.contains(unit)) {
                after.insert(at);
                // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                after.extend(earlier.after.iter().copied());
            }
        }
        planned.push(Planned { batch, after: after.into_iter().collect() });
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
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    agent: Agent,
    json: bool,
    /// Whether each stream is painted.
    // @lfy def/cli/main.lfy:main
    style: Paint,
    continuing: bool,
    /// The command elfie.json names under `verifier`; nothing verifies a batch without one.
    // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
    verifier: Option<String>,
    /// --no-verify: no verifier runs and nothing is reviewed.
    // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
    no_verify: bool,
    /// --accept: no compiler is run, and no review record file changes.
    // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    accept_only: bool,
    /// --full: every criterion and test of a batch is reviewed again, carrying nothing.
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    full: bool,
    /// What the last review of each batch found, by batch identifier, so that the review
    /// records of its units are written when its source maps are recorded.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    reviewed: BTreeMap<String, Reviewed>,
    /// The process groups each batch has running, by batch identifier, so that a stopped
    /// compile or a stopped batch stops them and everything they started.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    running: BTreeMap<String, Arc<Running>>,
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

/// What the last review of one batch found: the reviews of its last run of the verifier, and
/// the hashes and carried verdicts of the review request that run was made with. The review
/// records of the batch's units are written from these when its source maps are recorded.
// @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
struct Reviewed {
    /// The reviews of the batch's last verifier run.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    reviews: Vec<Review>,
    /// The hash that run's review request gave each id.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    hashes: BTreeMap<String, String>,
    /// The verdicts that review request carried.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    carried: Vec<ReviewRecord>,
}

impl Run {
    // @lfy def/cli/main.lfy:main
    fn new(program: Program, plan: Plan, maps: Vec<SourceMap>, reporter: Reporter, invocation: &Invocation) -> Run {
        let verifier = verifier_command(&program.workspace.root);
        // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
        let agent = Agent::new(
            invocation.flag("json"),
            Paint::of(invocation),
            reporter.logs.clone(),
            &program.workspace.root,
        );
        Run {
            program,
            plan,
            maps,
            reporter,
            // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
            agent,
            json: invocation.flag("json"),
            // @lfy def/cli/main.lfy:main
            style: Paint::of(invocation),
            continuing: invocation.flag("continue"),
            // @lfy def/cli/main.lfy:main
            verifier,
            no_verify: invocation.flag("no-verify"),
            // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
            accept_only: invocation.flag("accept"),
            // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
            full: invocation.flag("full"),
            // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
            reviewed: BTreeMap::new(),
            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
            running: BTreeMap::new(),
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
        // The reviews of the round before answered for the plan it was made from, so nothing
        // of them is carried into the records of a unit compiled once more.
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        self.reviewed.clear();
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
    // @lfy def/cli/main.lfy:main#main:main:fef239376f4395cd11e23eb728a352b07322301b631d9069086add1723844c11
    fn stop(&mut self, batch: &Batch) {
        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
        self.incomplete.push(batch.identifier.clone());
        self.stopped.extend(batch.units.iter().copied());
        // @lfy def/cli/main.lfy:main#main:main:b4d24a6285bcfe6faf8fa684cd04234bc339a6acb61cf4c5597af955903b8ba1
        // With --continue only the batches that depend on one of its units are stopped, and
        // the batches that depend on none of them still run.
        // @lfy def/cli/main.lfy:main#main:main:2f8c90c5e12c7f100f1b5428e6c30011d648af8d88b9a3ce42d079de42b15071
        // @lfy def/cli/main.lfy:main#main:main:b4bf8bd2d74cf6932c3ed9afe7c76d862161f90df8e2ae4a6e2cb1728ba3e9b8
        if !self.continuing {
            self.halted = true;
            // The compile stops: every run of the compiler, the verifier, or a command of
            // the target still running for a batch is stopped, with every process of its
            // group, and what it gives is ignored, since a batch whose run was stopped
            // merges and records nothing.
            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
            self.stop_every_run();
        }
    }

    /// The process groups one batch has running, made the first time the batch runs
    /// something.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    fn running_of(&mut self, label: &str) -> Arc<Running> {
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        self.running.entry(label.to_string()).or_default().clone()
    }

    /// Every run of the compiler, the verifier, or a command of the target still running for
    /// one batch stopped, with every process of its group.
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    fn stop_running(&self, label: &str) {
        // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
        if let Some(running) = self.running.get(label) {
            running.stop();
        }
    }

    /// Every run still running for any batch stopped, with every process of its group: what
    /// a compile that stops does before it returns.
    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
    fn stop_every_run(&self) {
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        for running in self.running.values() {
            running.stop();
        }
    }

    /// The batches of the plan a compile runs, in plan order: every one, or only those of the
    /// target --target names.
    // @lfy def/cli/main.lfy:main#main:main:ab0e6c732c90fb6b7fa9232e9b5f894e7f913233bb84f30a1d08a14830314820
    fn batches_for(&self, target: Option<&str>) -> Vec<usize> {
        (0..self.plan.batches.len())
            .filter(|&index| {
                // @lfy def/cli/main.lfy:main#main:main:ab0e6c732c90fb6b7fa9232e9b5f894e7f913233bb84f30a1d08a14830314820
                target.is_none_or(|name| batch_target(self.workspace(), &self.plan, &self.plan.batches[index]) == name)
            })
            .collect()
    }

    /// Whether a batch holds a unit depending on one of a batch that did not complete.
    // @lfy def/cli/main.lfy:main#main:main:fef239376f4395cd11e23eb728a352b07322301b631d9069086add1723844c11
    fn depends_on_stopped(&self, batch: &Batch) -> bool {
        batch
            .units
            .iter()
            .any(|&unit| self.plan.units[unit].dependencies.iter().any(|d| self.stopped.contains(d)))
    }

    /// The directory a batch's requests, questions, and reasons are written to:
    /// `elfie-requests` under the root, never under an output directory.
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    fn requests_directory(&self) -> PathBuf {
        requests_directory(&self.workspace().root)
    }

    /// Writes `elfie-requests/<batch>.<suffix>` under the root.
    // @lfy def/cli/main.lfy:main#main:main:b01f4990d763a4c86c9d8bc4995bc1c02eebe03f8a9f36d24f234cfd4e6042df
    // @lfy def/cli/main.lfy:main#main:main:0e04fca1d5a1398b0203e9dfa3d58b7324f1ef742ad13c350264b5b958456075
    fn write_note(&self, batch: &Batch, suffix: &str, text: &str) -> Option<PathBuf> {
        let directory = self.requests_directory();
        let path = directory.join(format!("{}.{suffix}", file_name_of(&batch.identifier)));
        // The folder it writes into is created when it is missing.
        // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
        match fs::create_dir_all(&directory).and_then(|()| fs::write(&path, text)) {
            Ok(()) => Some(path),
            Err(error) => {
                // Any other message the CLI itself prints to standard error begins with
                // elfie: painted failure, and a message that begins with a path has the
                // path painted subject.
                // @lfy def/cli/main.lfy:main#main:main:e08f0e5222d67e3b1a8262febafcc033512185f2bbde7fbe34f189a53fb1d3f1
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
    ///
    /// `hosted` is whether the request is for a compiler run in a hosted session of this
    /// command's agent server: true for the request handed to a compiler command, false for
    /// the one written out for a compiler run by hand.
    // @lfy def/cli/main.lfy:main
    fn request_of(&self, batch: &Batch, directory: &Path, hosted: bool, kept: Option<&Kept>) -> Request {
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
        // @lfy def/cli/main.lfy:main#main:main:c01ce1f2a8e5cd3e6ca0d0a2be31e2356dcb22b0e029c02e817117316275641c
        let mut previous = BTreeMap::new();
        // The path of each unit whose previous text is only its hash-matched source: it is
        // given exactly when that text is the hash-matched source and not the source at the
        // unit's last accepted generation.
        // @lfy def/cli/main.lfy:main#main:main:44d118a88d1e7a297fd7918629fa1c6c140077672079734436b5b02679cb1207
        let mut hash_matched: Vec<String> = Vec::new();
        for &index in &batch.units {
            let unit = &self.plan.units[index];
            if let Some(found) = previous_source(self.workspace(), unit) {
                let path = self.workspace().files[unit.file].path.clone();
                // @lfy def/cli/main.lfy:main#main:main:44d118a88d1e7a297fd7918629fa1c6c140077672079734436b5b02679cb1207
                if found.hash_matched {
                    // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
                    hash_matched.push(path.clone());
                }
                previous.insert(path, found.text);
            }
        }
        // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
        // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
        // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
        let mut request =
            generation::request(&self.program, &self.plan, batch, &existing, &previous, hosted, &hash_matched);
        // The person answers by editing the definitions, or by writing the answer below
        // the question, which the next compile of the batch appends to its instructions.
        // @lfy def/cli/main.lfy:main#main:main:760ac9394baad44b62a3c96e58e305792fb43b2fe0dc7ab8510183a1ad12b070
        if let Some(answer) = self.answer_of(batch) {
            request.instructions.push_str(&format!("\n\n## The answer to the question asked before\n\n{answer}\n"));
        }
        // A batch that took its kept work is told so: the outputs it is handed hold work a run
        // rejected and that was never accepted, and the problems that rejection named follow,
        // one per line.
        // @lfy def/cli/main.lfy:main#main:main:42f4bccc722123748448fab03bfe2e9f1e2ec7530f09efd662aa8ce9f1cb26a2
        if let Some(kept) = kept {
            // @lfy def/cli/main.lfy:main#main:main:42f4bccc722123748448fab03bfe2e9f1e2ec7530f09efd662aa8ce9f1cb26a2
            request.instructions.push_str(
                "\n\nIts existing outputs hold work a run rejected and that was never accepted.\n",
            );
            // @lfy def/cli/main.lfy:main#main:main:42f4bccc722123748448fab03bfe2e9f1e2ec7530f09efd662aa8ce9f1cb26a2
            for problem in &kept.problems {
                request.instructions.push_str(&format!("{problem}\n"));
            }
        }
        request
    }

    /// The kept work the batch would ask for: its target and, for each unit in batch order,
    /// its stem, the SHA-256 of [`Unit::text`], and the SHA-256 of its local criterion and
    /// test ids. What a file on disk is compared with before anything of it is taken.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    fn asked_kept(&self, batch: &Batch) -> Kept {
        // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
        let units = batch
            .units
            .iter()
            .map(|&index| {
                let unit = &self.plan.units[index];
                KeptUnit {
                    stem: unit.stem.clone(),
                    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
                    hash: generation::source_hash(&unit.text),
                    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
                    requirements: generation::source_hash(&requirement_ids_of(&self.program, unit).join("\n")),
                }
            })
            .collect();
        Kept {
            // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
            target: batch_target(self.workspace(), &self.plan, batch).to_string(),
            units,
            problems: Vec::new(),
            files: Vec::new(),
        }
    }

    /// Whether every output of the batch's units has the unit's own region text that
    /// [`SourceMap::text`] of its map holds: the outputs on disk are exactly the ones the last
    /// acceptance recorded, so nothing of them has to be generated again.
    ///
    /// Both sides are read in the batch's directory: the output, and the unit's maps from its
    /// map file there, so maps another unit's acceptance re-derived before the copy was made,
    /// or in place before this comparison, count. A unit with no map there has nothing on disk
    /// that stands, so it never holds, and neither does a map recorded before the text hash
    /// was, whose empty hash no text equals.
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
    fn outputs_stand(&self, batch: &Batch, directory: &Path) -> bool {
        // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
        batch.units.iter().all(|&index| {
            // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
            let maps = self.maps_in(index, directory);
            !maps.is_empty()
                && maps.iter().all(|map| {
                    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
                    fs::read_to_string(directory.join(&map.output)).is_ok_and(|text| {
                        // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
                        let own = own_region_text(self.workspace(), &self.plan, index, &text);
                        generation::source_hash(&own) == map.text
                    })
                })
        })
    }

    /// The source maps of one unit as the map file in a directory holds them, as recorded:
    /// whether or not the outputs they name still exist, which is how `sourceMapsOf` reads a
    /// map file apart from that.
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    fn maps_in(&self, unit: usize, directory: &Path) -> Vec<SourceMap> {
        // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
        generation::read_source_maps(&self.map_file_in(unit, directory))
    }

    /// The map file of one unit under a directory, which is the root or a copy of it, so a
    /// unit's maps are named the same either way.
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    fn map_file_in(&self, unit: usize, directory: &Path) -> PathBuf {
        let unit = &self.plan.units[unit];
        // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
        directory
            .join(MAPS)
            .join(&self.workspace().targets[unit.target].identifier)
            .join(format!("{}.json", unit.stem))
    }

    /// The markers of each unit's reworded criteria and tests rewritten to the ids they have
    /// now, in the outputs on disk in the batch's directory, before a request is made for the
    /// batch.
    ///
    /// Nothing is rewritten for a unit whose source at its last accepted generation git no
    /// longer holds, since nothing then says which id replaces which. One rewording progress
    /// line per unit counts its reworded criteria and tests and the outputs written.
    // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
    // @lfy def/cli/main.lfy:main#main:main:fa18c20576f142bd6c6ed82dce02b8ba5a122973c56fb0aae68b0c40a74c201c
    fn reword_outputs(&mut self, batch: &Batch, directory: &Path) {
        for &index in &batch.units {
            let unit = self.plan.units[index].clone();
            // Only the source at the unit's last accepted generation says which id replaces
            // which; its hash-matched source answered for other ids altogether.
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let Some(previous) = previous_source(self.workspace(), &unit).filter(|found| !found.hash_matched) else {
                continue;
            };
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let rewordings = generation::reworded(&self.program, &unit, &previous.text);
            if rewordings.is_empty() {
                continue;
            }
            // The id each marker names now, by the id it names on disk.
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let ids: BTreeMap<String, String> = rewordings
                .iter()
                .map(|rewording| (rewording.replaces.clone(), rewording.id.clone()))
                .collect();
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let (outputs, _) = outputs_of(self.workspace(), &self.plan, index, directory);
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            let renamed = generation::rename(self.workspace(), &self.plan, index, &outputs, &ids);
            let mut written = 0;
            for output in &renamed {
                let path = directory.join(&output.path);
                // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
                match fs::write(&path, &output.text) {
                    Ok(()) => written += 1,
                    Err(error) => {
                        complain(self.style.err, Some(&output.path), &error.to_string());
                        self.worsen(ExitCode::Failure);
                    }
                }
            }
            // @lfy def/cli/main.lfy:main#main:main:fa18c20576f142bd6c6ed82dce02b8ba5a122973c56fb0aae68b0c40a74c201c
            if written > 0 {
                // @lfy def/cli/main.lfy:main#main:main:fa18c20576f142bd6c6ed82dce02b8ba5a122973c56fb0aae68b0c40a74c201c
                let message = format!("{} reworded, {written} outputs written", rewordings.len());
                self.reporter.report(Step::Rewording, Some(&batch.identifier), Some(&unit.stem), &message);
            }
        }
    }

    /// The target's build command and then its test command run on a batch in its directory,
    /// in place of the compiler, with a building progress line before them: nothing when both
    /// succeeded, and otherwise which command failed and its exit code.
    ///
    /// It takes the compile locked rather than borrowed, since the lock is free while a
    /// command runs: that is what lets the other batches run theirs at the same time.
    // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    fn build_and_test(run: &Mutex<Run>, batch: &Batch, directory: &Path, runs: &Runs) -> Result<(), String> {
        let (target, logs) = {
            let mut state = locked(run);
            let Some(target) = state.target_of(batch) else {
                return Err("the batch has no target".to_string());
            };
            // A building progress line is printed before the build command runs.
            // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
            let line = format!("{} build and then test", target.identifier);
            state.reporter.report(Step::Building, Some(&batch.identifier), None, &line);
            (target, state.reporter.logs.clone())
        };
        // The build command runs first, and the test command only once it succeeded.
        // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
        for operation in [Operation::Build, Operation::Test] {
            // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
            let Some(code) = run_target_command(&target, operation, directory, &logs, Some(&runs.running)) else {
                // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
                return Err(format!("the target has no {} command", word_of_operation(operation)));
            };
            // A command succeeds when its exit code is 0.
            // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
            if code != 0 {
                // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                return Err(format!("the {} command exited with {code}", word_of_operation(operation)));
            }
        }
        Ok(())
    }

    /// The target of a batch, as the workspace declares it.
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    fn target_of(&self, batch: &Batch) -> Option<Target> {
        let name = batch_target(self.workspace(), &self.plan, batch).to_string();
        // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
        self.workspace().targets.iter().find(|one| one.identifier == name).cloned()
    }

    /// Whether the batch's target has a command with a line for the build operation and one
    /// with a line for the test operation, which is what lets them stand in for the compiler.
    // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
    fn builds_and_tests(&self, batch: &Batch) -> bool {
        // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
        self.target_of(batch).is_some_and(|target| {
            line_for(&target, Operation::Build).is_some() && line_for(&target, Operation::Test).is_some()
        })
    }

    /// For each unit of the batch, `accept` runs on the files under the target's output
    /// directory in the batch's directory that carry a marker for the unit.
    // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
    fn verdicts_of(&mut self, batch: &Batch, request: &Request, directory: &Path) -> Vec<Verdict> {
        let mut verdicts = Vec::new();
        for &unit in &batch.units {
            let stem = self.plan.units[unit].stem.clone();
            let reason = self.plan.units[unit].reason.map_or_else(|| "up to date".to_string(), |r| r.as_str().to_string());
            self.reporter.report(Step::Checking, Some(&batch.identifier), Some(&stem), &reason);
            // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
            let (outputs, elsewhere) = outputs_of(self.workspace(), &self.plan, unit, directory);
            // The source maps recorded for the target's other units, which say which outputs
            // already belong to another unit.
            // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
            let recorded = self.recorded_elsewhere(unit, directory);
            verdicts.push(generation::accept(
                &self.program,
                &self.plan,
                request,
                unit,
                &outputs,
                &recorded,
                &elsewhere,
            ));
        }
        verdicts
    }

    /// The source maps recorded for the other units of one unit's target: what `accept` reads
    /// to see whether an output already belongs to another unit. The unit's own are left out,
    /// since writing its own output again is what it is being accepted for.
    ///
    /// They are read from the map files in the batch's directory, as recorded, so a map
    /// another unit's acceptance re-derived is what this one is checked against.
    // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
    fn recorded_elsewhere(&self, unit: usize, directory: &Path) -> Vec<SourceMap> {
        let target = self.workspace().targets[self.plan.units[unit].target].identifier.clone();
        let source = self.workspace().files[self.plan.units[unit].file].path.clone();
        let mut out: Vec<SourceMap> = Vec::new();
        // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
        for other in 0..self.plan.units.len() {
            let unit = &self.plan.units[other];
            if self.workspace().targets[unit.target].identifier != target
                || self.workspace().files[unit.file].path == source
            {
                continue;
            }
            // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
            out.extend(self.maps_in(other, directory));
        }
        out
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
            // The outputs are written back with their markers normalized.
            // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
            // @lfy def/cli/main.lfy:main#main:main:bba3021a7366935ae1491608251547392369c5d323bf75ec2cab5bbbd9981a54
            let path = directory.join(&output.path);
            // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
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
        // @lfy def/cli/main.lfy:main#main:main:081257eb508c08a673bc5c569b3099485967e6b85b434c98d303c2de2d9958c5
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
    // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
    // @lfy def/cli/main.lfy:main#main:main:547a317d986ea3f6e5aa1c3f7159cf0286efabfa3aa41ee361f5f5c8c9611f52
    // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
    fn record_maps(&mut self, batch: &Batch, held: &[SourceMap], on_build: bool) {
        // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
        self.accepted_batch = true;
        let mut unwritten: Vec<String> = Vec::new();
        // The files the batch's units' maps named before they were recorded, read off their
        // map files in the root: a unit whose map file names one of them is a co-owner too,
        // since this batch has just stopped writing a file it used to share with it.
        // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
        let mut shared: BTreeSet<String> = BTreeSet::new();
        for &index in &batch.units {
            let workspace = &self.program.workspace;
            let unit = &self.plan.units[index];
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            for map in generation::read_source_maps(&generation::map_file(workspace, unit)) {
                shared.insert(map.output);
            }
        }
        // The files the maps recorded here name, which are where another unit's marker may now
        // sit inside a region of this batch's.
        // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
        let mut recorded_outputs: BTreeSet<String> = BTreeSet::new();
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
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            recorded_outputs.extend(mine.iter().map(|map| map.output.clone()));
            // @lfy def/cli/main.lfy:main#main:main:547a317d986ea3f6e5aa1c3f7159cf0286efabfa3aa41ee361f5f5c8c9611f52
            self.maps.retain(|map| !(map.target == target && map.source == source));
            self.maps.extend(mine.iter().cloned());
            let workspace = &self.program.workspace;
            let unit = &self.plan.units[index];
            // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
            if !generation::record(workspace, unit, &mine) {
                unwritten.push(generation::map_file(workspace, unit).display().to_string());
            }
        }
        // The maps of every unit of the batch that has them recorded are recorded: each
        // co-owner's maps are re-derived from the files as they are now, so a unit that shares
        // an output with the batch is told where its regions went.
        // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
        shared.extend(recorded_outputs.iter().cloned());
        self.rederive_co_owners(batch, &shared, &recorded_outputs);
        // The source maps of the batch are recorded: its review records are written beside
        // them, so that the next compile carries the verdicts of what it did not change. A
        // batch accepted on its build and test had no verifier run on it and no review
        // request made for it, so no review record file of its units changes.
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
        if !on_build {
            // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
            self.record_reviews(batch);
        }
        // The work a rejected run of the batch kept is no longer anything to take: its
        // outputs are recorded now, whatever the batch's directory was.
        // @lfy def/cli/main.lfy:main#main:main:890ae3d7c53cdb8c1d8297b9a9fc00b295121ba77bafc958ef442ac72d599abd
        let path = kept_file(&self.workspace().root, &batch.identifier);
        // @lfy def/cli/main.lfy:main#main:main:890ae3d7c53cdb8c1d8297b9a9fc00b295121ba77bafc958ef442ac72d599abd
        if let Err(error) = fs::remove_file(&path)
            && error.kind() != io::ErrorKind::NotFound
        {
            complain(self.style.err, Some(&path.display().to_string()), &error.to_string());
        }
        for path in unwritten {
            complain(self.style.err, Some(&path), "could not be written");
            self.worsen(ExitCode::Failure);
        }
    }

    /// Each co-owner of the batch has its maps re-derived from the files as they are now and
    /// recorded, once the maps of every unit of the batch that has them recorded are recorded
    /// in the root.
    ///
    /// The batch's co-owners are the units of its target that are not of the batch and that
    /// have a marker of their own in a file the recorded maps name, or whose map file names
    /// such a file or a file a map of the batch's units named before they were recorded.
    /// `shared` is every one of those files and `recorded` the ones the maps recorded here
    /// name.
    ///
    /// Nothing else is written for a co-owner: no compiler and no verifier runs for it and no
    /// review record file of its changes, because its code did not change — only where its
    /// regions sit in files another unit rewrote.
    // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
    // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
    // @lfy def/cli/main.lfy:main#main:main:14b01e32487ce04075ecbde039b065c7ad9311e9e5e8cb3e8c5117b41e849f61
    fn rederive_co_owners(&mut self, batch: &Batch, shared: &BTreeSet<String>, recorded: &BTreeSet<String>) {
        if shared.is_empty() {
            return;
        }
        let root = self.workspace().root.clone();
        let target = batch_target(self.workspace(), &self.plan, batch).to_string();
        // The text of each file the maps recorded here name, where a co-owner's own marker may
        // now sit, read once rather than once per unit.
        // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
        let texts: Vec<String> =
            recorded.iter().filter_map(|output| fs::read_to_string(root.join(output)).ok()).collect();
        // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
        for index in 0..self.plan.units.len() {
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            if self.workspace().targets[self.plan.units[index].target].identifier != target
                || batch.units.contains(&index)
            {
                continue;
            }
            let workspace = &self.program.workspace;
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            let was = generation::read_source_maps(&generation::map_file(workspace, &self.plan.units[index]));
            // Its map file names one of the files the batch shares with it, or one of its own
            // markers sits in a file the batch just recorded.
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            let named = was.iter().any(|map| shared.contains(&map.output));
            // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
            let marked = texts.iter().any(|text| {
                generation::parse_markers(text)
                    .iter()
                    .any(|marker| own_marker(self.workspace(), &self.plan, index, marker))
            });
            if !named && !marked {
                continue;
            }
            // Each co-owner's maps are re-derived once for the batch, from the maps its map
            // file holds, whether or not the outputs they name still exist.
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            let now = generation::rederive(&self.program, &self.plan, index, &was);
            // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
            self.warn_of_wider_regions(batch, index, &was, &now);
            let workspace = &self.program.workspace;
            let unit = &self.plan.units[index];
            // Only the co-owner's map file is written for it.
            // @lfy def/cli/main.lfy:main#main:main:14b01e32487ce04075ecbde039b065c7ad9311e9e5e8cb3e8c5117b41e849f61
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            if !generation::record(workspace, unit, &now) {
                let path = generation::map_file(workspace, unit).display().to_string();
                complain(self.style.err, Some(&path), "could not be written");
                self.worsen(ExitCode::Failure);
                continue;
            }
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            let (target, source) = {
                let unit = &self.plan.units[index];
                (
                    self.workspace().targets[unit.target].identifier.clone(),
                    self.workspace().files[unit.file].path.clone(),
                )
            };
            // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
            self.maps.retain(|map| !(map.target == target && map.source == source));
            self.maps.extend(now);
        }
    }

    /// A message to standard error for each of a co-owner's own markers whose regions span
    /// more output lines in total than they did in the maps the re-derived ones replace: the
    /// re-derived maps are recorded all the same, and the message says that another unit's
    /// lines may now lie in the co-owner's region.
    // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
    fn warn_of_wider_regions(&mut self, batch: &Batch, unit: usize, was: &[SourceMap], now: &[SourceMap]) {
        // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
        let before = self.spans_of(unit, was);
        // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
        let after = self.spans_of(unit, now);
        let stem = self.plan.units[unit].stem.clone();
        for (marker, lines) in after {
            // A region the maps it replaces did not hold spanned no line in them, so any line
            // of it now is more.
            // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
            if lines <= before.get(&marker).copied().unwrap_or(0) {
                continue;
            }
            // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
            let message = format!(
                "the region of {marker} grew when {} was recorded: another unit's lines may now lie in it",
                batch.identifier
            );
            // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
            complain(self.style.err, Some(&stem), &message);
        }
    }

    /// How many output lines the regions of each of one unit's own markers span in total, by
    /// the marker's file, entity, and requirement, which is what names a region.
    // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
    fn spans_of(&self, unit: usize, maps: &[SourceMap]) -> BTreeMap<String, usize> {
        let mut out: BTreeMap<String, usize> = BTreeMap::new();
        for map in maps {
            for marker in &map.markers {
                // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
                if !own_marker(self.workspace(), &self.plan, unit, marker) {
                    continue;
                }
                // @lfy def/cli/main.lfy:main#main:main:e79adeb5a0eec8def98674cfa2f63ca863e974bbc0cef0329483f02c0d8cad9f
                let lines = marker.end.saturating_sub(marker.output_line) + 1;
                *out.entry(region_named(marker)).or_default() += lines;
            }
        }
        out
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
    // @lfy def/cli/main.lfy:main#main:main:e9c86cec438cdb4badb2de33afdcb601c2b149059793a66717645f2b181e4894
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
    // @lfy def/cli/main.lfy:main#main:main:a1f4dce1fbc7ba29b8c1fd6e837fee8a81c31ace4aefaabc340dbb6b9b1b0be2
    fn write_reviews(&mut self, batch: &Batch, reviews: &[Review]) {
        let value = serde_json::Value::Array(reviews.iter().map(Review::to_json).collect());
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default());
        self.write_note(batch, "reviews.json", &text);
    }

    /// The review request of a batch written for a verifier run by hand.
    // @lfy def/cli/main.lfy:main#main:main:3e861a06695127864ab425d08e899da99dbf6789c4c55135fca304c37a7e5e18
    fn write_review_request(&mut self, batch: &Batch) {
        let records = self.records_of(batch);
        let maps = self.maps.clone();
        let request = self.review_request_of(batch, &maps, None, &records);
        if let Some(path) = self.write_note(batch, "review.md", &request.instructions)
            && !self.json
        {
            println!("wrote {}", relative(&self.workspace().root, &path));
        }
    }

    /// What a batch's review is made with: what each of its units was last reviewed as, as the
    /// review record files in the root hold them, in batch order.
    ///
    /// With --full nothing is carried, so every criterion and test of the batch is reviewed
    /// again.
    // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
    // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    fn records_of(&self, batch: &Batch) -> Vec<ReviewRecord> {
        // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
        if self.full {
            return Vec::new();
        }
        // The records of each unit of the batch, in batch order, as the review record files
        // under the root hold them.
        // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
        // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
        batch
            .units
            .iter()
            .flat_map(|&unit| generation::reviews_of(&self.program, &self.plan.units[unit]))
            .collect()
    }

    /// What a verifier is handed for one batch: the plan, the batch, the source maps it is
    /// pointed at, the directory its outputs were written in, and the records of its units.
    // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
    // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    fn review_request_of(
        &self,
        batch: &Batch,
        maps: &[SourceMap],
        directory: Option<&Path>,
        records: &[ReviewRecord],
    ) -> ReviewRequest {
        // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
        generation::review(&self.program, &self.plan, batch, maps, directory, records)
    }

    /// The records a review made once more is made with: those the review before it was made
    /// with, each review that run gave replacing the record of its id, so that what was
    /// looked at and settled is carried rather than looked at a third time.
    ///
    /// With `settled`, only a review whose status is satisfied replaces a record: a run made
    /// again because it answered for too little carries what it did satisfy and nothing else.
    // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
    // @lfy def/cli/main.lfy:main#main:main:60e9ae0b09414c3ddab6543838621c14dbb8932101affdf4f89cd4ef669d2a0b
    fn carried_on(
        records: &[ReviewRecord],
        request: &ReviewRequest,
        reviews: &[Review],
        settled: bool,
    ) -> Vec<ReviewRecord> {
        let mut out: Vec<ReviewRecord> = records.to_vec();
        for review in reviews {
            // @lfy def/cli/main.lfy:main#main:main:60e9ae0b09414c3ddab6543838621c14dbb8932101affdf4f89cd4ef669d2a0b
            if settled && review.status != ReviewStatus::Satisfied {
                continue;
            }
            // The record a review earns holds the hash the run's review request gave its id,
            // so a later review carries it only while the code it was judged against stands.
            // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
            let record = ReviewRecord::of(review, &request.hashes);
            match out.iter_mut().find(|held| held.id == record.id) {
                Some(held) => *held = record,
                None => out.push(record),
            }
        }
        out
    }

    /// The review records of each unit of a batch written in the root, from what its last
    /// review found: for each local criterion and test of the unit, sorted by id, the record
    /// made from the review of its id in the batch's last verifier run, else its record among
    /// the verdicts that review request carried, else none.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    fn record_reviews(&mut self, batch: &Batch) {
        // Nothing reviewed the batch: there is nothing to write, so no review record file
        // changes with --no-verify, with no verifier named, or with --accept.
        // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
        // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
        if self.no_verify || self.accept_only || self.verifier.is_none() {
            return;
        }
        let Some(reviewed) = self.reviewed.get(&batch.identifier) else {
            return;
        };
        let mut unwritten: Vec<String> = Vec::new();
        for &index in &batch.units {
            let unit = &self.plan.units[index];
            // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
            let mut records: Vec<ReviewRecord> = Vec::new();
            for id in requirement_ids_of(&self.program, unit) {
                // The review of its id in the last verifier run, else the verdict that review
                // request carried for it, else none.
                // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
                if let Some(review) = reviewed.reviews.iter().find(|review| review.id == id) {
                    records.push(ReviewRecord::of(review, &reviewed.hashes));
                } else if let Some(carried) = reviewed.carried.iter().find(|record| record.id == id) {
                    records.push(carried.clone());
                }
            }
            // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
            if !generation::record_reviews(&self.program.workspace, unit, &records) {
                unwritten.push(generation::review_file(&self.program.workspace, unit).display().to_string());
            }
        }
        for path in unwritten {
            complain(self.style.err, Some(&path), "could not be written");
            self.worsen(ExitCode::Failure);
        }
    }

    /// The verifier run for one batch, its output shown as it arrives and logged, its report
    /// read, and its reviews written. A run that ended without every verdict is run once
    /// more, carrying what it did satisfy; when that run ends without them too its problems
    /// are printed as a failed progress line and the batch is verified with the reviews
    /// parsed from it as if its report had been complete.
    ///
    /// A review request that asks nothing runs no verifier: every verdict of the batch is
    /// carried, so there is nothing for a verifier to look at.
    ///
    /// It takes the compile locked rather than borrowed, since the lock is free while the
    /// verifier runs: that is what lets the other batches run theirs at the same time.
    // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
    // @lfy def/cli/main.lfy:main#main:main:dc6670a1659abe98c2ae0ed1502d175a57162b9aa395c843f4fdeaa3d5ce4664
    // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
    #[expect(clippy::too_many_arguments, reason = "one review of a batch takes all of this")]
    fn review_batch(
        run: &Mutex<Run>,
        batch: &Batch,
        command: &str,
        where_: &Where,
        progress: bool,
        maps: &[SourceMap],
        records: &[ReviewRecord],
        runs: &Runs,
        resuming: bool,
    ) -> Verification {
        // The source maps are the ones `accept` derived, held but not yet recorded, so every
        // criterion can be pointed at the region of output that claims to satisfy it, and the
        // directory is the batch's, so the regions are excerpted from the outputs the batch
        // just wrote rather than from the root's older ones.
        // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
        let (units, request) = {
            let held = locked(run);
            let request = held.review_request_of(batch, maps, Some(where_.directory()), records);
            (stems_of(&held.plan, batch, " "), request)
        };
        // A review request that asks about nothing runs no verifier at all: every verdict of
        // the batch is carried, and those are the records its units keep.
        // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
        // @lfy def/cli/main.lfy:main#main:main:f79d62c52638caa4cb00a160316bc7052bc62e29caef830e683dba3fca9581ca
        // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
        if request.asked.is_empty() {
            let verification = Verification { report: ReviewReport::default(), failed: false, request };
            locked(run).remember(batch, &verification);
            return verification;
        }
        // A verifying progress line is printed before the verifier runs on the batch.
        // @lfy def/cli/main.lfy:main#main:main:51dd1de1581a678bdebc04d9b69fa4f946f213ff7b9a93b18092a2e8b9587a97
        if progress {
            locked(run).reporter.report(Step::Verifying, Some(&batch.identifier), None, command);
        }
        // The verifier's run once more after the compiler's run once more is accepted goes on
        // from the session of its first run on the batch here; its own run once more for a
        // report that ended without every verdict does not.
        // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        let (exit, report, failed) =
            Run::ask_verifier(run, runs, command, where_, batch, &units, &request, maps, 1, resuming);
        let verification = if failed {
            // The run ended without every verdict: it is run once more, asked as it was but
            // carrying what it did satisfy.
            // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
            // @lfy def/cli/main.lfy:main#main:main:60e9ae0b09414c3ddab6543838621c14dbb8932101affdf4f89cd4ef669d2a0b
            let carried = Run::carried_on(records, &request, &report.reviews, true);
            let again = {
                let held = locked(run);
                held.review_request_of(batch, maps, Some(where_.directory()), &carried)
            };
            if again.asked.is_empty() {
                // Nothing is left to ask: what the first run reported stands as it is.
                // @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
                locked(run).note_incomplete(&batch.identifier, &report, exit.code);
                Verification { report, failed, request }
            } else {
                // The verifier's run once more after a run that ended without every verdict is
                // no resuming run.
                // @lfy def/cli/main.lfy:main#main:main:60e9ae0b09414c3ddab6543838621c14dbb8932101affdf4f89cd4ef669d2a0b
                // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
                let (exit, report, failed) =
                    Run::ask_verifier(run, runs, command, where_, batch, &units, &again, maps, 2, false);
                // A run made once more that ends without every verdict too has its problems
                // printed as a failed progress line, and what it did report is read as if it
                // had been complete.
                // @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
                // @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
                if failed {
                    locked(run).note_incomplete(&batch.identifier, &report, exit.code);
                }
                Verification { report, failed, request: again }
            }
        } else {
            Verification { report, failed, request }
        };
        let mut held = locked(run);
        // @lfy def/cli/main.lfy:main#main:main:a1f4dce1fbc7ba29b8c1fd6e837fee8a81c31ace4aefaabc340dbb6b9b1b0be2
        held.write_reviews(batch, &verification.report.reviews);
        held.remember(batch, &verification);
        drop(held);
        verification
    }

    /// What one review of a batch found, kept until its source maps are recorded, when the
    /// review records of its units are written from it.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    fn remember(&mut self, batch: &Batch, verification: &Verification) {
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        self.reviewed.insert(
            batch.identifier.clone(),
            Reviewed {
                reviews: verification.report.reviews.clone(),
                hashes: verification.request.hashes.clone(),
                carried: verification.request.carried.clone(),
            },
        );
    }

    /// One run of the verifier on a batch: the review request as its input, the batch's
    /// directory as its directory, its output shown as it arrives and logged, and its report
    /// read with the ids that review request asks about and the reviews it submitted through
    /// the agent server.
    // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
    // @lfy def/cli/main.lfy:main#main:main:dc6670a1659abe98c2ae0ed1502d175a57162b9aa395c843f4fdeaa3d5ce4664
    // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
    #[expect(clippy::too_many_arguments, reason = "one run of the verifier takes all of this")]
    fn ask_verifier(
        run: &Mutex<Run>,
        runs: &Runs,
        command: &str,
        where_: &Where,
        batch: &Batch,
        units: &str,
        request: &ReviewRequest,
        maps: &[SourceMap],
        attempt: usize,
        resuming: bool,
    ) -> (Exit, ReviewReport, bool) {
        // The run is bound to the review request it is given and the source maps that request
        // was made with, so every call it makes reads the outputs it is judging.
        // @lfy def/cli/main.lfy:main#main:main:d7fdc89e00db8f2de0886cdaaecf381963abe8e1eb6392c9dee9bc26a30911b5
        let binding = {
            let held = locked(run);
            held.binding_of(
                Role::Verifier,
                Some(batch),
                where_.directory(),
                None,
                Some(request.clone()),
                maps.to_vec(),
            )
        };
        // The lock is free here, so another batch streams its own agent meanwhile.
        // @lfy def/cli/main.lfy:main#main:main:0a2bb4066f627b6e8b99aaad5534b75619cb71fa40c84cd5ddf152a449abd6a8
        let exit =
            runs.run(command, where_, &batch.identifier, units, &request.instructions, binding, resuming);
        let held = locked(run);
        let label = &batch.identifier;
        // Every line of the verifier's output and its report are appended to the log.
        // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
        held.reporter.append(&format!("--- the review of {label} (attempt {attempt}) ---\n{}", exit.stdout));
        let (report, failed) = held.report_of(&exit, &request.asked);
        (exit, report, failed)
    }

    /// The report of one run of the verifier, and whether it ended without every verdict: the
    /// whole of its standard output read with the ids its review request asked about and the
    /// reviews it submitted through the agent server.
    // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    fn report_of(&self, exit: &Exit, asked: &[String]) -> (ReviewReport, bool) {
        // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
        let mut report = generation::review_of(&exit.stdout, &self.program, asked, &exit.record.reviews);
        if exit.code == -1 {
            report.problems.insert(0, format!("the verifier command could not be run: {}", exit.stderr.trim()));
        }
        // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
        let failed = verifier_failed(exit, &report);
        (report, failed)
    }

    /// A run that ended without every verdict: its problems are printed as a failed progress
    /// line, and what it did report is read as if it had been complete.
    // @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
    // @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
    fn note_incomplete(&mut self, label: &str, report: &ReviewReport, code: i32) {
        let problems = report.problems.clone();
        // @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
        self.reporter.report(Step::Failed, Some(label), None, &problems.join("\n"));
        self.reporter.problems(&problems);
        if code == -1 {
            self.failed += 1;
            self.worsen(ExitCode::Failure);
        }
    }

    /// The binding one hosted session is opened with: what the run is run as, the batch it is
    /// for, the directory it runs in, the plan and the program the command made, and what the
    /// compiler or the verifier was handed as its input.
    // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    fn binding_of(
        &self,
        role: Role,
        batch: Option<&Batch>,
        directory: &Path,
        request: Option<Request>,
        review: Option<ReviewRequest>,
        source_maps: Vec<SourceMap>,
    ) -> Binding {
        // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
        Binding {
            role,
            batch: batch.cloned(),
            directory: directory.to_path_buf(),
            plan: self.plan.clone(),
            program: self.program.clone(),
            // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
            request,
            review,
            source_maps,
        }
    }


    /// A batch whose units were accepted by `accept` is verified against the source maps it
    /// derived, held but not yet recorded, and the problems of its violated reviews are what
    /// it is rejected with; nothing verifies it with --no-verify or with no verifier named,
    /// and an unverifiable review is counted and never rejects.
    // @lfy def/cli/main.lfy:main#main:main:79415ab35d3d0d869ed27ece2f7e00f6466563a94f8acf3d056c77ec329cd386
    fn verify_batch(
        run: &Mutex<Run>,
        batch: &Batch,
        where_: &Where,
        held: &[SourceMap],
        records: &[ReviewRecord],
        runs: &Runs,
        resuming: bool,
    ) -> Option<Verified> {
        let command = {
            let state = locked(run);
            // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
            if state.no_verify {
                return None;
            }
            state.verifier.clone()?
        };
        let verification =
            Run::review_batch(run, batch, &command, where_, true, held, records, runs, resuming);
        let report = &verification.report;
        let carried = verification.request.carried.len();
        let mut state = locked(run);
        // One reviewed line follows, whose message is the counts of satisfied, violated, and
        // unverifiable reviews of this run and then the verdicts it carried, each painted only
        // when it is worth noticing.
        // @lfy def/cli/main.lfy:main#main:main:a69cc6b70fb7a4c11a8b4d46f13c5ce8f3c5af3483523d31b91eba6e44b45af0
        // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
        // @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
        let counts = counts_with_carried(&report.reviews, report.missing.len(), carried);
        state.reporter.report_counts(Step::Reviewed, Some(&batch.identifier), &counts, None);
        // Nothing was asked: nothing was reviewed, so nothing was violated either and the
        // batch is done.
        // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
        if verification.request.asked.is_empty() {
            return None;
        }
        // Each id no review names is named as not reviewed, on one line under the reviewed
        // line, and counts as unverifiable above.
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        if !report.missing.is_empty() {
            state.reporter.reason(&missing_lines(report));
        }
        // A run that stood had anything it wrote beside its reviews printed under its
        // reviewed line; a run that ended without every verdict had its problems printed as a
        // failed line already.
        // @lfy def/cli/main.lfy:main#main:main:7ac5c7dbdba835816dd103d3fb573c0697886fc87ae20c77a226aaa3f7b90592
        // @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
        if !verification.failed {
            state.reporter.problems(&report.problems);
        }
        // An unverifiable review is counted in the reviewed line and never rejects the batch;
        // no violated review and no unplaced violation leaves the batch accepted.
        // @lfy def/cli/main.lfy:main#main:main:41f684cc85ced11c6439219ff5858457d7c2ff822185af4d70f8ce243afc79ff
        // @lfy def/cli/main.lfy:main#main:main:547a317d986ea3f6e5aa1c3f7159cf0286efabfa3aa41ee361f5f5c8c9611f52
        let problems = violations_of(report);
        if problems.is_empty() {
            return None;
        }
        // The review made once more carries what this one settled; an unplaced violation
        // names no criterion, so nothing of this review can stand and the next one carries
        // nothing at all.
        // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
        let next = if report.unplaced.is_empty() {
            Run::carried_on(records, &verification.request, &report.reviews, false)
        } else {
            Vec::new()
        };
        Some(Verified { problems, next })
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
    // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
    // @lfy def/cli/main.lfy:main#main:main:b560d79fb66587371f4a98bc89eea6db2f1123b1c6455f3310c1a9827b77b43e
    fn global_review(run: &Mutex<Run>, root: &Path) -> Vec<String> {
        let (agent, command, requirements, units, request, maps) = {
            let mut held = locked(run);
            // No global criterion and no global test: no global review runs and no file is
            // written.
            // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
            if held.program.criteria.is_empty() && held.program.tests.is_empty() {
                return Vec::new();
            }
            // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
            if held.no_verify {
                return Vec::new();
            }
            let Some(command) = held.verifier.clone() else {
                return Vec::new();
            };
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            let requirements = GlobalReview::requirements_of(&held.program);
            let last = GlobalReview::read(root);
            // Either a batch was accepted, or what the last global review answered for
            // changed.
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            if !held.accepted_batch && last.as_ref().is_some_and(|last| last.requirements == requirements) {
                return Vec::new();
            }
            // A globalVerifying progress line is printed before the verifier runs.
            // @lfy def/cli/main.lfy:main#main:main:95ec4b53b789d932736cfefccb14f8a652da47d03aa0d31592172eebd66e9004
            held.reporter.report(Step::GlobalVerifying, None, None, &command);
            // Every source map, recorded or held.
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            let request = generation::global_review(&held.program, &held.maps);
            // The units just generated among them.
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
            let units = held.global_stems();
            (held.agent.clone(), command, requirements, units, request, held.maps.clone())
        };
        // @lfy def/cli/main.lfy:main#main:main:b560d79fb66587371f4a98bc89eea6db2f1123b1c6455f3310c1a9827b77b43e
        let where_ = Where::Root(root.to_path_buf());
        // A run for the global review is no resuming run.
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        let runs = Runs::new(agent, locked(run).running_of(GLOBAL));
        let report = Run::ask_globally(run, &runs, &command, &where_, &units, &request, &maps);
        let mut held = locked(run);
        held.write_global_reviews(root, &requirements, &report);
        // @lfy def/cli/main.lfy:main#main:main:2a36e0de7040861081f44ec78abe37bf9a365485f6034aa434d08e6f81d442a5
        let counts = counts_with_missing(&report.reviews, report.missing.len());
        held.reporter.report_counts(Step::GlobalReviewed, None, &counts, None);
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        if !report.missing.is_empty() {
            held.reporter.reason(&missing_lines(&report));
        }
        // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
        let violated: Vec<&Review> =
            report.reviews.iter().filter(|review| review.status == ReviewStatus::Violated).collect();
        if violated.is_empty() {
            return Vec::new();
        }
        // One problem per violated review: failure at, a space, global, a colon, a space, the
        // note, and the evidence in parentheses.
        // @lfy def/cli/main.lfy:main#main:main:6d40945fd3224481f1e733c96a5f8552118021276f2c7f21fda17341f1887ab9
        let problems: Vec<String> = violated
            .iter()
            .map(|review| format!("failure at {GLOBAL}: {} ({})", review.note, review.evidence))
            .collect();
        held.reporter.problems(&problems);
        // The units whose markers answered for it.
        // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
        let ids: BTreeSet<String> = violated.iter().map(|review| review.id.clone()).collect();
        held.stems_answering_now(&ids)
    }

    /// The stems of every unit whose source maps hold a marker answering for one of these
    /// ids, read from the maps as they stand: a unit compiled in this run has its markers
    /// there rather than in the outputs its plan was made from.
    // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
    fn stems_answering_now(&self, ids: &BTreeSet<String>) -> Vec<String> {
        // @lfy def/cli/main.lfy:main#main:main:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
        self.stems_answering_with(|id| ids.contains(id))
    }

    /// The stems, space separated, of every unit whose source maps hold a marker whose
    /// requirement is a global id: what `ELFIE_UNITS` holds for a global review.
    // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
    // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
    fn global_stems(&self) -> String {
        // ELFIE_BATCH is global and ELFIE_UNITS the stems, separated by spaces, of every unit
        // whose source maps hold a marker answering for a global criterion or test.
        // @lfy def/cli/main.lfy:main#main:main:3ddc917bd3a57754bf0ad2a8022e78d50863262ab86f611e2e486cdb2641266f
        self.stems_answering_with(is_global).join(" ")
    }

    /// The stems of every unit whose source maps hold a marker whose requirement the test
    /// accepts.
    ///
    /// A unit's source maps are those this compile accepted for it when it has any, and
    /// [`Unit::outputs`] otherwise, so a unit generated in this compile is named by the
    /// markers it has just earned rather than by the ones the plan was made from, and a unit
    /// nothing touched by the ones its map file holds.
    // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
    fn stems_answering_with(&self, answers: impl Fn(&str) -> bool) -> Vec<String> {
        let mut stems: Vec<String> = Vec::new();
        for unit in &self.plan.units {
            let target = &self.program.workspace.targets[unit.target].identifier;
            let source = &self.program.workspace.files[unit.file].path;
            // A unit's source maps are those this compile accepted for it when it has any,
            // and Unit.outputs otherwise; a verify has accepted none, so there they are
            // always Unit.outputs.
            // @lfy def/cli/main.lfy:main#main:main:5701e0bba8328be1eb78fd2c1451a0191daf21f77fddf9a61cd0581350901182
            // @lfy def/cli/main.lfy:main#main:main:81c50f38555ae6dfd98db609c7d367bafaf650ea3d271e864cee62280684913d
            let accepted: Vec<&SourceMap> =
                self.maps.iter().filter(|map| &map.target == target && &map.source == source).collect();
            let maps = if accepted.is_empty() { unit.outputs.iter().collect() } else { accepted };
            // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
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
    // @lfy def/cli/main.lfy:main#main:main:22a2ed809bd843b9b4106304478f1daff7eaa70e42e90d8c6d42efebc81418c7
    fn write_global_reviews(&mut self, root: &Path, requirements: &str, report: &ReviewReport) {
        // A GlobalReview holding that report and the SHA-256 of the global ids reviewed,
        // written where the agent server reads it from.
        // @lfy def/cli/main.lfy:main#main:main:22a2ed809bd843b9b4106304478f1daff7eaa70e42e90d8c6d42efebc81418c7
        let last = GlobalReview {
            requirements: requirements.to_string(),
            reviews: report.reviews.clone(),
            problems: report.problems.clone(),
            unplaced: report.unplaced.clone(),
            missing: report.missing.clone(),
        };
        let text = format!("{}\n", serde_json::to_string_pretty(&last.to_json()).unwrap_or_default());
        let directory = requests_directory(root);
        let path = GlobalReview::path(root);
        if let Err(error) = fs::create_dir_all(&directory).and_then(|()| fs::write(&path, text)) {
            complain(self.style.err, Some(&path.display().to_string()), &error.to_string());
            self.worsen(ExitCode::Failure);
        }
    }

    /// Every global criterion and test reviewed once for the verify command: a person asked
    /// for the opinion, so nothing but their existence gates it, and the reviews are given
    /// back to be printed.
    // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
    fn verify_globally(run: &Mutex<Run>, root: &Path) -> Vec<Review> {
        let (agent, command, units, request, requirements, maps) = {
            let held = locked(run);
            // No global criterion and no global test: no global review runs and no file is
            // written, here as in a compile.
            // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
            if held.program.criteria.is_empty() && held.program.tests.is_empty() {
                return Vec::new();
            }
            let Some(command) = held.verifier.clone() else {
                return Vec::new();
            };
            // The run is one stream of the verifier, with the global review request as its
            // input and the root as its directory.
            // @lfy def/cli/main.lfy:main#main:main:9092aae70550a2dbfe8b5fed13d4592a6956f3134ba180faf2fcebe40af78f0b
            let request = generation::global_review(&held.program, &held.maps);
            // @lfy def/cli/main.lfy:main#main:main:81c50f38555ae6dfd98db609c7d367bafaf650ea3d271e864cee62280684913d
            let units = held.global_stems();
            // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
            let requirements = GlobalReview::requirements_of(&held.program);
            (held.agent.clone(), command, units, request, requirements, held.maps.clone())
        };
        let where_ = Where::Root(root.to_path_buf());
        // A run for the global review is no resuming run, so it carries nothing.
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        let runs = Runs::new(agent, locked(run).running_of(GLOBAL));
        let report = Run::ask_globally(run, &runs, &command, &where_, &units, &request, &maps);
        locked(run).write_global_reviews(root, &requirements, &report);
        report.reviews
    }

    /// The verifier run for the global review, at most twice, and its report: the global
    /// review request as its input and the root as its directory, with one hosted session
    /// bound to it, no batch, and the source maps that request was made with.
    // @lfy def/cli/main.lfy:main#main:main:9092aae70550a2dbfe8b5fed13d4592a6956f3134ba180faf2fcebe40af78f0b
    // @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
    // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
    fn ask_globally(
        run: &Mutex<Run>,
        runs: &Runs,
        command: &str,
        where_: &Where,
        units: &str,
        request: &ReviewRequest,
        maps: &[SourceMap],
    ) -> ReviewReport {
        let mut last = ReviewReport::default();
        for attempt in 1..=2 {
            // The run is run as the verifier, for no batch, with the global review request it
            // is given and the source maps that request was made with.
            // @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
            let binding = {
                let held = locked(run);
                held.binding_of(Role::Verifier, None, where_.directory(), None, Some(request.clone()), maps.to_vec())
            };
            // A run for the global review is no resuming run, whatever came before it.
            // @lfy def/cli/main.lfy:main#main:main:9092aae70550a2dbfe8b5fed13d4592a6956f3134ba180faf2fcebe40af78f0b
            // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
            let exit = runs.run(command, where_, GLOBAL, units, &request.instructions, binding, false);
            let mut held = locked(run);
            // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
            held.reporter.append(&format!("--- the review of {GLOBAL} (attempt {attempt}) ---\n{}", exit.stdout));
            // @lfy def/cli/main.lfy:main#main:main:8e26beddf232ac15c3c4ca8bec25eccd1e26ac1300d74e2b9144e1839621effe
            let (report, failed) = held.report_of(&exit, &request.asked);
            last = report;
            if !failed {
                break;
            }
            if attempt == 2 {
                held.note_incomplete(GLOBAL, &last, exit.code);
            }
        }
        last
    }

    /// Runs the compiler on one batch, at most twice, and acts on the outcome; a batch
    /// whose reviews violate what was asked is run once more beyond that. Gives the source
    /// maps its units earned when it was accepted, to be recorded once its work has reached
    /// the root, and nothing when it stopped.
    ///
    /// A batch whose every unit is reworded only, or whose every unit has the reason
    /// dependency, may have its first run skip the compiler: the markers of its reworded
    /// criteria and tests are moved, or the target's build and test commands run, and its
    /// outputs are then checked as after a run of the compiler. A skipped run counts as no
    /// run of the compiler in any count that decides whether the batch is run once more or
    /// stops, and anything that goes wrong sends it to the compiler as its first run.
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    // @lfy def/cli/main.lfy:main#main:main:93bc38c041972030b9d2fabf83040193223e16bc9dc0d57762b9fdcea1eb2753
    // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
    fn compile_batch(
        run: &Mutex<Run>,
        batch: &Batch,
        command: &str,
        where_: &Where,
        runs: &Runs,
        kept: Option<&Kept>,
        round: &Round,
    ) -> Compiled {
        let directory = where_.directory().to_path_buf();
        // What may stand in for the compiler on the batch's first run, decided before any
        // marker of its outputs is rewritten: the outputs on disk must be exactly the ones
        // the last acceptance recorded, and nothing of a rejected run may have been taken.
        // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
        // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
        // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
        let stand = kept.is_none() && locked(run).outputs_stand(batch, &directory);
        let mut request = {
            let mut state = locked(run);
            let stems = stems_of(&state.plan, batch, " ");
            state.reporter.report(Step::Requesting, Some(&batch.identifier), None, &stems);
            // Before each request, the markers of each unit's reworded criteria and tests
            // name the ids they have now.
            // @lfy def/cli/main.lfy:main#main:main:09bd3d74151cf0e97221debf137fe7f322cd57bc1bae17b8ecd0ef36a1063045
            state.reword_outputs(batch, &directory);
            // The compiler of a batch runs in a hosted session of this compile's agent
            // server, so its request names its interfaces and the tools that session offers.
            // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
            state.request_of(batch, &directory, true, kept)
        };
        // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
        let mut stage = locked(run).stage_of(batch, &request, stand);
        let mut attempt = 0;
        // The batch is run once more for a rejected outcome, and once more for violated
        // reviews even when it was already run once more for a rejected outcome.
        // @lfy def/cli/main.lfy:main#main:main:17f6feee3279ee2519b458a9ca13607a4738441827bb862fc9a71d712ea28691
        let mut retried_for_rejection = false;
        let mut retried_for_reviews = false;
        let mut retried_for_failure = false;
        // Whether the next run of the compiler, and the next run of the verifier, go on from
        // the session of that role's run on the batch here before it.
        // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
        let mut resuming_compiler = false;
        let mut resuming_verifier = false;
        // What the batch's first review carries: what each of its units was last reviewed as.
        // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
        let mut records = locked(run).records_of(batch);
        loop {
            // The compile stopped while the batch was running: nothing more starts for it,
            // and nothing of it is merged into the root or recorded.
            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
            if locked(run).halted {
                // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                return Compiled::stopped();
            }
            // A first run that skips the compiler: the target's build and test commands, or
            // nothing but the markers already moved, and then the outputs checked.
            // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
            // @lfy def/cli/main.lfy:main#main:main:c9e9379aee6642c4c6fd644592fb507b9275e32d2a6daa8928f697b3e30a5b06
            let outcome = if stage == Stage::Compiler {
                attempt += 1;
                let (units, binding) = {
                    let mut state = locked(run);
                    let step = if attempt == 1 { Step::Compiling } else { Step::Retrying };
                    state.reporter.report(step, Some(&batch.identifier), None, command);
                    // The run is run as the compiler of this batch, in the batch's directory,
                    // with the request its input is made from, no review, and no source maps:
                    // a compiler is given no verdict to read.
                    // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
                    let binding = state.binding_of(
                        Role::Compiler,
                        Some(batch),
                        &directory,
                        Some(request.clone()),
                        None,
                        Vec::new(),
                    );
                    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
                    (stems_of(&state.plan, batch, " "), binding)
                };
                // The lock is free here, so another batch streams its own compiler meanwhile.
                // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
                let exit = runs.run(
                    command,
                    where_,
                    &batch.identifier,
                    &units,
                    &request.instructions,
                    binding,
                    resuming_compiler,
                );
                // The compile stopped while the run was going, so the run was stopped with
                // every process of its group: what it gives is ignored, and nothing of the
                // batch is merged into the root or recorded.
                // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                if locked(run).halted {
                    // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                    return Compiled::stopped();
                }
                let mut state = locked(run);
                // The code is -1 because the command could not be started: the outcome is
                // failed.
                // @lfy def/cli/main.lfy:main#main:main:0dff09f6c30f73ee8dcc8aa60016fb5af42b0b31adeaa63cec1108324df750ae
                if exit.code == -1 {
                    Outcome {
                        kind: OutcomeKind::Failed,
                        message: format!("the compiler command could not be run: {}", exit.stderr.trim()),
                        verdicts: Vec::new(),
                    }
                } else {
                    // Process.Exit.stdout is the report, appended to the log.
                    // @lfy def/cli/main.lfy:main#main:main:f507a20d5658ce7b158a66e802e963debf7d67da97fd22a6992b840100d837b2
                    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
                    let report = exit.stdout;
                    state
                        .reporter
                        .append(&format!("--- the report of {} (attempt {attempt}) ---\n{report}", batch.identifier));
                    // @lfy def/cli/main.lfy:main#main:main:28c3fdcd35d73bdee306c14f707035a5db9191c247458cee76933c6058aba777
                    let verdicts = state.verdicts_of(batch, &request, &directory);
                    // How the run ended is the outcome of its report, those verdicts, and
                    // what it submitted through its session before the session was closed.
                    // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
                    generation::outcome_of(&report, verdicts, exit.record.submission.as_ref())
                }
            } else {
                // The target's build command and then its test command stand in for the
                // compiler on a batch whose every unit has the reason dependency; a command
                // that failed sends the batch to the compiler as its first run.
                // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
                // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                if stage == Stage::Dependency
                    && let Err(why) = Run::build_and_test(run, batch, &directory, runs)
                {
                    // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                    // @lfy def/cli/main.lfy:main#main:main:93bc38c041972030b9d2fabf83040193223e16bc9dc0d57762b9fdcea1eb2753
                    locked(run).reporter.report(Step::Building, Some(&batch.identifier), None, &why);
                    stage = Stage::Compiler;
                    continue;
                }
                // Each unit is accepted or rejected as after a run of the compiler.
                // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
                // @lfy def/cli/main.lfy:main#main:main:c9e9379aee6642c4c6fd644592fb507b9275e32d2a6daa8928f697b3e30a5b06
                let verdicts = locked(run).verdicts_of(batch, &request, &directory);
                // A unit rejected without a run of the compiler: the problems are printed and
                // the compiler runs on the batch as its first run, counting toward no retry.
                // @lfy def/cli/main.lfy:main#main:main:b8f2ed1f4f7dd347c1fc2031d346ef69e5e80252bfc60209de5f60e9d30ea618
                // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                if verdicts.iter().any(|verdict| !verdict.accepted) {
                    let mut state = locked(run);
                    // @lfy def/cli/main.lfy:main#main:main:b8f2ed1f4f7dd347c1fc2031d346ef69e5e80252bfc60209de5f60e9d30ea618
                    state.report_rejections(batch, &verdicts);
                    if stage == Stage::Dependency {
                        // @lfy def/cli/main.lfy:main#main:main:9253f376492f06a33bd0110d5f3f29c04bffd6a3b5a3a922f56cb3f2d9a668e9
                        let stems = stems_of(&state.plan, batch, " ");
                        let why = format!("a unit of {stems} was rejected on the build and the test");
                        state.reporter.report(Step::Building, Some(&batch.identifier), None, &why);
                    }
                    drop(state);
                    // @lfy def/cli/main.lfy:main#main:main:93bc38c041972030b9d2fabf83040193223e16bc9dc0d57762b9fdcea1eb2753
                    stage = Stage::Compiler;
                    continue;
                }
                // Every unit was accepted: a batch accepted on its build and test writes its
                // outputs back and is done there, with no verifier run on it; a batch accepted
                // on its moved markers alone goes on as a compiler run whose outcome is
                // accepted does, which writes them back once.
                // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
                if stage == Stage::Dependency {
                    // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
                    let held = locked(run).hold_batch(batch, &verdicts, &directory);
                    // Every unit was accepted and nothing is to run the compiler on the
                    // batch: what it has accepted is what a copy depending on it takes.
                    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
                    round.settled(changes_of(where_));
                    // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
                    return Compiled::on_build(held);
                }
                // @lfy def/cli/main.lfy:main#main:main:64edfa7c41c1dd3ba97140f316748c707c8d82d2e2e9691e156ae3ba2749260f
                Outcome { kind: OutcomeKind::Accepted, message: String::new(), verdicts }
            };
            match outcome.kind {
                // Each unit's outputs are written back in the batch's directory and its
                // source maps are held, and the batch is verified; no review violated, or
                // nothing verifying it, leaves the held source maps to be recorded once the
                // batch's work has reached the root.
                // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
                OutcomeKind::Accepted => {
                    // Each unit's outputs are written back once, whether the compiler ran or
                    // the batch's moved markers stood in for it.
                    // @lfy def/cli/main.lfy:main#main:main:c0ff42fd47c552fedc4e3213d3ec87968ae59279faec3d132caf6866820027d7
                    // @lfy def/cli/main.lfy:main#main:main:64edfa7c41c1dd3ba97140f316748c707c8d82d2e2e9691e156ae3ba2749260f
                    let held = locked(run).hold_batch(batch, &outcome.verdicts, &directory);
                    // Every unit of the batch was accepted and nothing is about to run the
                    // compiler on it again: what it has accepted no longer changes, so a copy
                    // of a batch depending on it may take it before its review ends.
                    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
                    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                    round.settled(changes_of(where_));
                    // The batch is verified; no review violated, or nothing verifying it,
                    // and the batch is done.
                    // @lfy def/cli/main.lfy:main#main:main:79415ab35d3d0d869ed27ece2f7e00f6466563a94f8acf3d056c77ec329cd386
                    // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
                    // @lfy def/cli/main.lfy:main#main:main:991ed2ddd9c68b7df2156a615cae29e3dc3add9bd395dfb4fffaebafeb35a4c2
                    let Some(verified) =
                        Run::verify_batch(run, batch, where_, &held, &records, runs, resuming_verifier)
                    else {
                        // A compile that stopped while the verifier ran stopped that run too:
                        // nothing of the batch is merged into the root or recorded.
                        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                        if locked(run).halted {
                            // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
                            return Compiled::stopped();
                        }
                        return Compiled::accepted(held);
                    };
                    let problems = verified.problems;
                    // The review made once more carries what this one settled.
                    // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
                    records = verified.next;
                    let mut state = locked(run);
                    // A violated review or an unplaced violation is handled as a rejected
                    // outcome is: the problems are printed and the batch is run once more
                    // with them appended to the instructions, even when it was already run
                    // once more for a rejected outcome. The verifier then runs on it again.
                    // @lfy def/cli/main.lfy:main#main:main:a90dd937c3211a26fb1a136a29a7052114ef2561c7d039de63fe390de2dec33e
                    // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
                    state.reporter.report(Step::Rejected, Some(&batch.identifier), None, &problems.join("\n"));
                    state.reporter.problems(&problems);
                    // The second such report stops the batch as rejected, and the held source
                    // maps are dropped rather than recorded, so its outputs are left on disk
                    // and the unit stays planned. Its copy keeps the work for the next
                    // compile of the batch.
                    // @lfy def/cli/main.lfy:main#main:main:60e5c32018713d03196fbe6b68625288ef878c3b8e6a489007fc2126d39b9a6a
                    // @lfy def/cli/main.lfy:main#main:main:e9c86cec438cdb4badb2de33afdcb601c2b149059793a66717645f2b181e4894
                    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
                    if retried_for_reviews {
                        state.unrecord(batch);
                        state.worsen(ExitCode::Problems);
                        state.stop(batch);
                        // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
                        return Compiled::rejected(problems);
                    }
                    retried_for_reviews = true;
                    // The batch is about to run the compiler again, so it is no longer far
                    // enough along for a batch depending on it to take what it accepted.
                    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                    round.runs_again();
                    // The batch's run once more is the compiler's: its first run when the
                    // compiler has not run on it yet, and a resuming run when it has.
                    // @lfy def/cli/main.lfy:main#main:main:2db65eab2f47acc8e9484b0ddac5774576ff7e5ed00b482fb43f68869baa1c60
                    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
                    resuming_compiler = attempt > 0;
                    resuming_verifier = true;
                    // @lfy def/cli/main.lfy:main#main:main:2db65eab2f47acc8e9484b0ddac5774576ff7e5ed00b482fb43f68869baa1c60
                    stage = Stage::Compiler;
                    drop(state);
                    // @lfy def/cli/main.lfy:main#main:main:ccb77bf35fa2892badac90b1cd9147551248086a7c8b8e46447f25e714ed6198
                    append_problems(&mut request, &problems);
                }
                // The problems are printed, and the batch is run once more with them
                // appended to the instructions.
                // @lfy def/cli/main.lfy:main#main:main:17f6feee3279ee2519b458a9ca13607a4738441827bb862fc9a71d712ea28691
                OutcomeKind::Rejected => {
                    let mut state = locked(run);
                    let rejected = state.report_rejections(batch, &outcome.verdicts);
                    let problems: Vec<String> =
                        outcome.verdicts.iter().flat_map(|verdict| verdict.problems.iter().cloned()).collect();
                    // A second rejection stops the compile, and its copy keeps the work for
                    // the next compile of the batch.
                    // @lfy def/cli/main.lfy:main#main:main:b4d24a6285bcfe6faf8fa684cd04234bc339a6acb61cf4c5597af955903b8ba1
                    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
                    if retried_for_rejection {
                        state.rejected += rejected;
                        state.worsen(ExitCode::Problems);
                        state.stop(batch);
                        // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
                        return Compiled::rejected(problems);
                    }
                    retried_for_rejection = true;
                    // @lfy def/cli/main.lfy:main#main:main:9160811ba53e331e41567344fda0fefec31e39e30c8ddd5d3b93fb1dbac571bd
                    resuming_compiler = true;
                    drop(state);
                    append_problems(&mut request, &problems);
                }
                // The reason is printed and written, and the compile stops.
                // @lfy def/cli/main.lfy:main#main:main:b01f4990d763a4c86c9d8bc4995bc1c02eebe03f8a9f36d24f234cfd4e6042df
                OutcomeKind::Blocked => {
                    let mut state = locked(run);
                    state.blocked += 1;
                    state.reporter.report(Step::Blocked, Some(&batch.identifier), None, &outcome.message);
                    // The reason is printed.
                    // @lfy def/cli/main.lfy:main#main:main:7b4c327653decbf264373384e604a80eaf1dd98b7513acb659e6462dc8c7522a
                    state.reporter.reason(&outcome.message);
                    let note = format!("# The compiler is blocked on the batch {}\n\n{}\n", batch.identifier, outcome.message);
                    state.write_note(batch, "blocked.md", &note);
                    state.worsen(ExitCode::Problems);
                    // Without --continue the compile stops.
                    // @lfy def/cli/main.lfy:main#main:main:5c341e1e6a5ddabeb77300d7deadd7e5a86870008f1c24e99a5252f7b956a93f
                    state.stop(batch);
                    return Compiled::stopped();
                }
                // The question is printed and written, and the compile stops.
                // @lfy def/cli/main.lfy:main#main:main:0e04fca1d5a1398b0203e9dfa3d58b7324f1ef742ad13c350264b5b958456075
                OutcomeKind::Clarification => {
                    let mut state = locked(run);
                    state.reporter.report(Step::Clarification, Some(&batch.identifier), None, &outcome.message);
                    // The question is printed.
                    // @lfy def/cli/main.lfy:main#main:main:110f4900a8d312787d8bb7041198cb88f88e9ff546daa7bbbbd9255b40329ac1
                    state.reporter.reason(&outcome.message);
                    let note = format!(
                        "# The compiler asked about the batch {}\n\n{}\n\n{ANSWER_HEADING}\n\n<!-- Answer by editing the definitions, or write the answer below this line; the next compile of this batch appends it to the instructions. -->\n",
                        batch.identifier, outcome.message
                    );
                    state.write_note(batch, "question.md", &note);
                    state.worsen(ExitCode::Problems);
                    // Without --continue the compile stops.
                    // @lfy def/cli/main.lfy:main#main:main:6996d8037ef2ea7f994646839432d548664a2cbd82d484947f3530d9066f33bd
                    state.stop(batch);
                    return Compiled::stopped();
                }
                // The batch is run once more.
                // @lfy def/cli/main.lfy:main#main:main:0dff09f6c30f73ee8dcc8aa60016fb5af42b0b31adeaa63cec1108324df750ae
                OutcomeKind::Failed => {
                    let mut state = locked(run);
                    state.reporter.report(Step::Failed, Some(&batch.identifier), None, &outcome.message);
                    // @lfy def/cli/main.lfy:main#main:main:43f2aa5048680432b93b7802c6b6a48f5fcb659a985ce06c84d0358d44a2277d
                    state.reporter.problems(std::slice::from_ref(&outcome.message));
                    // A second failure stops the compile.
                    // @lfy def/cli/main.lfy:main#main:main:b4bf8bd2d74cf6932c3ed9afe7c76d862161f90df8e2ae4a6e2cb1728ba3e9b8
                    if retried_for_failure {
                        state.failed += 1;
                        state.worsen(ExitCode::Failure);
                        state.stop(batch);
                        return Compiled::stopped();
                    }
                    retried_for_failure = true;
                    // A run once more after a failed run, or after an exit code of -1, is no
                    // resuming run: there was nothing to go on from.
                    // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
                    resuming_compiler = false;
                }
            }
        }
    }

    /// What the batch's first run is: the compiler, unless something may stand in for it.
    ///
    /// A batch whose every unit is reworded only has the markers of those criteria and tests
    /// moved and its outputs checked, which asks no compiler, but only while a verifier may
    /// look at the result. A batch whose every unit has the reason dependency has the
    /// target's build and test commands run instead, while the target has a line for both.
    // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
    // @lfy def/cli/main.lfy:main#main:main:373a3000fa63b155721a53e3ee16c9fbd2ad88138c3fd6a1ec81ce3eeab80f12
    // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
    // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
    // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
    // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
    fn stage_of(&self, batch: &Batch, request: &Request, stand: bool) -> Stage {
        // An output that is no longer what was accepted, or work a rejected run kept, leaves
        // the compiler to do.
        // @lfy def/cli/main.lfy:main#main:main:c8973e9e2d07a79f390133eefde25b3157ae35c014f978eb58e3a63d4f7eada0
        if !stand || self.accept_only {
            return Stage::Compiler;
        }
        // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
        let reworded_only = batch.units.iter().all(|&index| {
            let path = &self.workspace().files[self.plan.units[index].file].path;
            request.reworded_only.contains(path)
        });
        // Only a batch a verifier may look at is accepted on its moved markers alone.
        // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
        // @lfy def/cli/main.lfy:main#main:main:373a3000fa63b155721a53e3ee16c9fbd2ad88138c3fd6a1ec81ce3eeab80f12
        if reworded_only && self.verifier.is_some() && !self.no_verify {
            return Stage::Reworded;
        }
        // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
        let dependency_only = batch
            .units
            .iter()
            .all(|&index| self.plan.units[index].reason == Some(Reason::Dependency));
        // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
        // @lfy def/cli/main.lfy:main#main:main:75192ed1a30affcf3c4f5a267b8bf361995b47809aca6cb8e4585e78bffd8551
        if dependency_only && self.builds_and_tests(batch) {
            return Stage::Dependency;
        }
        // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
        Stage::Compiler
    }

    /// No compiler is run; the outputs already on disk are checked, written back
    /// normalized, and recorded when accepted. A batch whose units were all accepted is
    /// verified like any other against the source maps held for it; since no compiler runs
    /// there is nothing to run again, so a violated review rejects the batch at once and
    /// those maps are never recorded.
    // @lfy def/cli/main.lfy:main#main:main:e6df0c7548d8f79abfca3e578a974cfbbf2949927b4851d9860223b1315c6c27
    fn accept_on_disk(run: &Mutex<Run>, batch: &Batch, root: &Path) {
        // No compiler is run; nothing else is writing the root, so the batch needs no copy of
        // its own.
        // @lfy def/cli/main.lfy:main#main:main:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
        // @lfy def/cli/main.lfy:main#main:main:e6df0c7548d8f79abfca3e578a974cfbbf2949927b4851d9860223b1315c6c27
        let where_ = Where::Root(root.to_path_buf());
        let held = {
            let mut state = locked(run);
            // Before the outputs are checked, the markers of each unit's reworded criteria
            // and tests name the ids they have now, written at their paths in the root.
            // @lfy def/cli/main.lfy:main#main:main:561bd7cbbedaf15b591306a1bda08fd34f7da689d466b406d196bcf906444f7d
            state.reword_outputs(batch, root);
            // Nothing reads these instructions: with --accept no compiler is run and no
            // session is hosted, so the request is for no hosted session.
            // @lfy def/cli/main.lfy:main#main:main:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
            // @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
            let request = state.request_of(batch, root, false, None);
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
            // @lfy def/cli/main.lfy:main#main:main:52e13f2436af16c72ca604f2b05b1b8b9579d7f785a46580d36ca9a5d0c7bdc9
            // @lfy def/cli/main.lfy:main#main:main:906a7b942e9fdeeb2d0a33ef27f1952dfd4538adf43ab27446a9ee3d464d2e24
            if rejected > 0 {
                state.record_maps(batch, &held, false);
                state.rejected += rejected;
                state.worsen(ExitCode::Problems);
                return;
            }
            held
        };
        // @lfy def/cli/main.lfy:main#main:main:b136b63971e67fd1f1b4e0b4a407ebe829061ebda953131f2a62dc4d2993330f
        let records = locked(run).records_of(batch);
        // With --accept no compiler ran, so the verifier's run is a first run and goes on
        // from nothing.
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        let runs = {
            let mut state = locked(run);
            let agent = state.agent.clone();
            Runs::new(agent, state.running_of(&batch.identifier))
        };
        let verified = Run::verify_batch(run, batch, &where_, &held, &records, &runs, false);
        let mut state = locked(run);
        if let Some(verified) = verified {
            let problems = verified.problems;
            state.reporter.report(Step::Rejected, Some(&batch.identifier), None, &problems.join("\n"));
            state.reporter.problems(&problems);
            state.unrecord(batch);
            state.worsen(ExitCode::Problems);
            state.stop(batch);
        } else {
            // @lfy def/cli/main.lfy:main#main:main:906a7b942e9fdeeb2d0a33ef27f1952dfd4538adf43ab27446a9ee3d464d2e24
            state.record_maps(batch, &held, false);
        }
    }

    /// The request of a batch written for a compiler run by hand, which is in no hosted
    /// session: its interfaces are quoted whole and it names none of a session's tools.
    // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
    fn write_request(&mut self, batch: &Batch) {
        let stems = stems_of(&self.plan, batch, " ");
        self.reporter.report(Step::Requesting, Some(&batch.identifier), None, &stems);
        let root = self.workspace().root.clone();
        // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
        let request = self.request_of(batch, &root, false, None);
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
    // @lfy def/cli/main.lfy:main#main:main:4da6fc0ef0b92bdd309365b39295a5cc5de6e9d3296556c8da4e7fc7e1c11920
    fn finish(&mut self) {
        // The count of units accepted and the word accepted, the count of units rejected and
        // the word rejected, the count of batches blocked and the word blocked, and the count
        // of batches failed and the word failed: each count and its own word alone, separated
        // by a space, and the four by a comma and a space.
        // @lfy def/cli/main.lfy:main#main:main:e86903be24031cd7d20e2b972c8d08d2fcbbc6ec9495571912401e5762cc2cd6
        // @lfy def/cli/main.lfy:main#main:main:081257eb508c08a673bc5c569b3099485967e6b85b434c98d303c2de2d9958c5
        let counts: Counts = vec![
            (self.accepted, "accepted", Tone::Success),
            (self.rejected, "rejected", Tone::Failure),
            (self.blocked, "blocked", Tone::Warning),
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
        // @lfy def/cli/main.lfy:main#main:main:bbad3c69582995a56ff9152a6a483894f2d276f7b0d0b4805da999767796b37e
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
        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
        if !self.incomplete.is_empty() {
            // The line is painted warning, as `reason` paints every line it prints.
            // @lfy def/cli/main.lfy:main#main:main:5e9f9e326f58c59bbc1092c42862138ad8fb59f86a69705b596007f531aa116c
            let line = format!("did not complete: {}", self.incomplete.join(", "));
            self.reporter.reason(&line);
        }
    }
}

/// What a batch's first run is: the compiler, or what may stand in for it.
// @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// The compiler runs on the batch, as on any batch.
    // @lfy def/cli/main.lfy:main#main:main:97bd4222037bff6cb9965f2bb80eda7bdf1510086acdf2043dba6c4f6118713f
    Compiler,
    /// Every unit of the batch is reworded only, so moving the markers of its reworded
    /// criteria and tests is the whole of the work and its outputs are checked without the
    /// compiler.
    // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
    Reworded,
    /// Every unit of the batch has the reason dependency, so the target's build and test
    /// commands stand in for the compiler.
    // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
    Dependency,
}

/// What compiling one batch came to.
// @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
struct Compiled {
    /// The source maps its units earned, to be recorded once its work has reached the root;
    /// nothing when the batch stopped.
    // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
    held: Option<Vec<SourceMap>>,
    /// Whether the batch was accepted on its build and test, so that no verifier ran on it
    /// and no review record file of its units changes.
    // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
    on_build: bool,
    /// The problems of the rejection that stopped it, in the order they were printed, so
    /// that a copy left behind keeps them with its work.
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    problems: Vec<String>,
    /// Whether a second rejection stopped it, which is what makes a copy keep its work.
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    keeps: bool,
}

impl Compiled {
    /// A batch whose units were accepted and reviewed, with the source maps they earned.
    // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
    fn accepted(held: Vec<SourceMap>) -> Compiled {
        Compiled { held: Some(held), on_build: false, problems: Vec::new(), keeps: false }
    }

    /// A batch accepted on its build and test, which no verifier looked at.
    // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
    fn on_build(held: Vec<SourceMap>) -> Compiled {
        Compiled { held: Some(held), on_build: true, problems: Vec::new(), keeps: false }
    }

    /// A batch a second rejection stopped: its copy keeps the work and the problems.
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    fn rejected(problems: Vec<String>) -> Compiled {
        Compiled { held: None, on_build: false, problems, keeps: true }
    }

    /// A batch that stopped for anything else: nothing of it is merged or recorded, and
    /// nothing of it is kept.
    // @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
    fn stopped() -> Compiled {
        Compiled { held: None, on_build: false, problems: Vec::new(), keeps: false }
    }
}

/// The bookkeeping of a compile, locked. A thread that panicked while holding it leaves what
/// it held, which is read as it stands rather than taking the whole compile down with it.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
fn locked(run: &Mutex<Run>) -> std::sync::MutexGuard<'_, Run> {
    run.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What asking the verifier came to: its report, whether the run that gave it ended without
/// every verdict, so that the problems of such a run are not printed twice, and the review
/// request it was given, which says what it was asked about and what it carried.
// @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
// @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
#[derive(Default)]
struct Verification {
    report: ReviewReport,
    failed: bool,
    /// The review request the run was given, or the one that asked nothing.
    // @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
    request: ReviewRequest,
}

/// What verifying one batch came to when its reviews violated what was asked: the problems
/// they make, and the records the review made once more is made with.
// @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
struct Verified {
    problems: Vec<String>,
    /// What the next review of the batch carries.
    // @lfy def/cli/main.lfy:main#main:main:84bbcb2516b2cafb1bfbde0f772cadb0875451d4f6704a126825d45438d436dc
    next: Vec<ReviewRecord>,
}

/// Whether a run of the verifier ended without every verdict rather than merely writing
/// something beside its reviews: it held no review at all, or it left an id it was asked
/// about unanswered and either wrote no line reading `ELFIE: REVIEWED` or ended by a signal
/// or could not be started.
///
/// A run that answered for every id it was asked about stands as it is, whatever prose
/// surrounds the answers and however it ended, and so does one that wrote the end line and
/// ended well with some of them left over: what it left is counted as unverifiable.
// @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
// @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
fn verifier_failed(exit: &Exit, report: &ReviewReport) -> bool {
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    if report.reviews.is_empty() {
        return true;
    }
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    let ended = exit.code != -1 && exit.stdout.lines().any(|line| line.trim() == REVIEWED);
    !report.missing.is_empty() && !ended
}

/// What a batch's thread needs of the round it runs in: the schedule, what wakes it when
/// another batch moves on, where it sits, and whether batches run in copies.
// @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
struct Round<'r> {
    schedule: &'r Mutex<Schedule>,
    /// What a batch waits on while another batch merges or stops.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    finished: &'r Condvar,
    planned: &'r [Planned],
    /// The batch's own place in the round.
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    at: usize,
    /// Whether a batch runs in a copy of the root rather than in the root itself.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    copies: bool,
}

impl Round<'_> {
    /// The schedule, locked.
    // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
    fn held(&self) -> std::sync::MutexGuard<'_, Schedule> {
        self.schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The accepted changes of each batch holding a dependency of one of this batch's units,
    /// directly or through the units of other batches, that does not have its files in the
    /// root, in plan order: what this batch's copy takes when it is made.
    // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
    fn to_take(&self) -> Vec<(usize, Changes)> {
        let held = self.held();
        // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
        self.planned[self.at]
            .after
            .iter()
            .filter(|&&waited| held.states[waited] != State::Merged)
            .filter_map(|&waited| held.accepted.get(&waited).map(|changes| (waited, changes.clone())))
            .collect()
    }

    /// Waits until every batch whose accepted changes this batch's copy took has its files in
    /// the root or has stopped; whether one of them stopped without them reaching it.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    // @lfy def/cli/main.lfy:main#main:main:f5720ad6fd831e736d050c0b20f67ced9fd4c44fe185806606e36c8d267767fc
    // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
    fn wait_for(&self, took: &[usize]) -> bool {
        let mut held = self.held();
        loop {
            // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
            if took.iter().any(|&waited| held.states[waited] == State::Stopped) {
                return true;
            }
            // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
            if took.iter().all(|&waited| held.states[waited] == State::Merged) {
                return false;
            }
            // @lfy def/cli/main.lfy:main#main:main:f5720ad6fd831e736d050c0b20f67ced9fd4c44fe185806606e36c8d267767fc
            held = self.finished.wait(held).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Waits until the batch at one place has its files in the root, or has stopped.
    // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
    fn wait_for_one(&self, waited: usize) {
        let mut held = self.held();
        // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
        while held.states[waited] != State::Merged && held.states[waited] != State::Stopped {
            held = self.finished.wait(held).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// The batch's every unit accepted, and nothing about to run the compiler on it again.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    fn settled(&self, changes: Changes) {
        // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
        self.held().settled(self.at, changes);
        self.finished.notify_all();
    }

    /// A batch that was done running again: it is no longer done.
    // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
    fn runs_again(&self) {
        // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
        self.held().runs_again(self.at);
        self.finished.notify_all();
    }
}

/// The changes a batch has made where it runs: nothing at all when it runs in the root, which
/// is also where nothing takes another batch's work.
// @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
fn changes_of(where_: &Where) -> Changes {
    match where_ {
        // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
        Where::Copy(mirror) => mirror.changes(),
        Where::Root(_) => Changes::new(),
    }
}

/// What one run of a batch in one directory came to: it is over, or the batch runs again from
/// a new copy with what this one held as its kept work.
// @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
enum Attempt {
    Over(Done),
    Again(Kept),
}

/// One batch run from beginning to end, and once more from a new copy where the work of a
/// batch it took changes from reached the root mixed with other files.
// @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
fn run_batch(run: &Mutex<Run>, batch: &Batch, command: &str, root: &Path, round: &Round) -> Done {
    // The work the last compile of this batch kept, when a file beside the root holds work of
    // the batch's own that a run rejected and nothing accepted.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    let mut kept = kept_work(run, batch, root);
    loop {
        match run_in_copy(run, batch, command, root, round, kept.as_ref()) {
            // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
            Attempt::Over(done) => return done,
            // The batch runs again from a new copy, which takes its changes as kept work is
            // taken, and its runs of the compiler and the verifier are counted afresh.
            // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
            // @lfy def/cli/main.lfy:main#main:main:107c61d983a8522a99a186d5c8b3e788d5407f1e171398cf2aa4910fa72a7a30
            Attempt::Again(again) => {
                round.runs_again();
                kept = Some(again);
            }
        }
    }
}

/// One run of a batch in one directory: its own copy of the root where there is one, seeded
/// with the accepted changes of the batches it depends on, its kept work taken, the compiler,
/// the verifier, and then the merge of its work into the root.
///
/// A batch whose work passed has every file of its copy merged into the root, its source maps
/// recorded there, and its copy removed. A batch that stopped merges nothing and leaves its
/// copy in place for a person to read.
// @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
fn run_in_copy(
    run: &Mutex<Run>,
    batch: &Batch,
    command: &str,
    root: &Path,
    round: &Round,
    kept: Option<&Kept>,
) -> Attempt {
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    let (mut where_, took) = match make_copy(run, batch, root, round) {
        Some(made) => made,
        // The batch stops as a batch that depends on a stopped batch does.
        // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
        None => {
            let mut state = locked(run);
            state.stop(batch);
            state.worsen(ExitCode::Problems);
            return Attempt::Over(Done::Stopped);
        }
    };
    // The kept work taken into the copy, after the base was kept.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    let took_kept = take_kept(run, batch, &where_, kept);
    let runs = {
        let mut state = locked(run);
        let agent = state.agent.clone();
        Runs::new(agent, state.running_of(&batch.identifier))
    };
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    let compiled = Run::compile_batch(run, batch, command, &where_, &runs, took_kept.as_ref(), round);
    let Where::Copy(mirror) = &mut where_ else {
        // The batch ran in the root: there is nothing to merge, and its source maps are
        // recorded where they already are. No kept work is written or taken for it, because
        // a rejected batch's outputs are left on disk in the root.
        // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
        // @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
        if let Some(held) = &compiled.held {
            // @lfy def/cli/main.lfy:main#main:main:5841e3d28e75b69d42ed2ba411b34aa9429ca7415424978b7b95493fed283150
            locked(run).record_maps(batch, held, compiled.on_build);
        }
        return Attempt::Over(Done::Merged);
    };
    // The batch stopped: nothing of it is merged and the copy is left in place for a person
    // to read. A second rejection leaves its work beside the root for the next compile.
    // @lfy def/cli/main.lfy:main#main:main:4b34b56a504ad5f2803d8a2f2485ed28fe60ae7ebee92b236bd9c7c42e93773e
    // @lfy def/cli/main.lfy:main#main:main:fb97a602debdf6ca506dd68f399ab346accef55afb53336cff20a92a377060a1
    let Some(held) = compiled.held else {
        // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
        if compiled.keeps {
            write_kept(run, batch, root, mirror, &compiled.problems);
        }
        return Attempt::Over(Done::Stopped);
    };
    let waited: Vec<usize> = took.keys().copied().collect();
    // Nothing of the batch is merged into the root while a batch whose accepted changes its
    // copy took has not had its own files merged into it.
    // @lfy def/cli/main.lfy:main#main:main:f5720ad6fd831e736d050c0b20f67ced9fd4c44fe185806606e36c8d267767fc
    if round.wait_for(&waited) {
        // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
        locked(run).stop_running(&batch.identifier);
        let mut state = locked(run);
        state.stop(batch);
        state.worsen(ExitCode::Problems);
        return Attempt::Over(Done::Stopped);
    }
    // What the batches it took changes from actually wrote into the root: where a merge of
    // theirs mixed in other work, the copy takes those files and is built and tested again.
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    let mut differs: Changes = BTreeMap::new();
    // The batches whose files differ, in plan order, so the retrying line can name them.
    // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
    let mut differing: Vec<&str> = Vec::new();
    for (&waited, changes) in &took {
        let mut any = false;
        for (path, change) in changes {
            // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
            let now = fs::read(root.join(path)).ok();
            if now.as_deref() == change.text.as_deref() {
                continue;
            }
            any = true;
            // The base of the merge into the copy is what the copy took for that file.
            // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
            differs.insert(path.clone(), Change { base: change.text.clone(), text: now, mode: change.mode });
        }
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        if any {
            differing.push(&round.planned[waited].batch.identifier);
        }
    }
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    if !differs.is_empty()
        && let Err(why) = remerge(run, batch, mirror, &differs, &runs)
    {
        // Nothing of the batch is merged, a retrying line names the batch whose files
        // differ, and the batch runs again from a new copy taking its changes as kept work.
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        let message = format!("the files of {} differ: {why}", differing.join(", "));
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        locked(run).reporter.report(Step::Retrying, Some(&batch.identifier), None, &message);
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        let again = kept_of(run, batch, mirror, &Vec::new());
        mirror.remove();
        return Attempt::Again(again);
    }
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    match mirror.merge(root) {
        // Every file merged: the source maps are recorded in the root and the copy is
        // removed.
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        Ok(_) => {
            // @lfy def/cli/main.lfy:main#main:main:e7d188f1e2558f6ab540ea91a682e317f0402c6c88a2e8b1868e626094e26a25
            locked(run).record_maps(batch, &held, compiled.on_build);
            // @lfy def/cli/main.lfy:main#main:main:d4df2511718d78630ac13bf572eebf1307e182b63f7ed82d414c8f8535fc1278
            mirror.remove();
            Attempt::Over(Done::Merged)
        }
        // A file could not be merged without a conflict: nothing of the batch is merged, a
        // retrying line names the file, and the batch runs again from a new copy once no
        // other batch is running. This is not a rejection, so no count and no code change.
        // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
        Err(path) => {
            // Nothing of the batch is merged, and this is not a rejection.
            // @lfy def/cli/main.lfy:main#main:main:c004e1659461201158916168dc6b9d94ea0c6c129329cb862b1796902b1d5fdb
            // @lfy def/cli/main.lfy:main#main:main:a247581eedaee59d3678954f47b4cd5e987c056104b4042f1edc50229d71c21a
            // A retrying progress line names the file.
            // @lfy def/cli/main.lfy:main#main:main:f4aa516e10c668a136c54381377ba1ca01d74e8e80dfb1d323ad816c23263d22
            let message = format!("{path} changed under the batch and could not be merged");
            locked(run).reporter.report(Step::Retrying, Some(&batch.identifier), None, &message);
            // The copy goes, since the batch runs again from a new one; what its units were
            // counted as stands, so the run anew counts them once rather than twice.
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            mirror.remove();
            Attempt::Over(Done::Conflicted)
        }
    }
}

/// The batch's directory made: the root itself, or a copy of it seeded with the accepted
/// changes of each batch it depends on whose files are not in the root yet.
///
/// Once the root's files are copied, those changes are merged in, in plan order, as a batch's
/// files are merged into the root; what the copy then holds is kept as the batch's base. A
/// change that cannot be merged without a conflict has the copy removed and the batch waiting
/// for that batch to merge before it tries again. Nothing when a batch it waits on stopped.
// @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
// @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
// @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
fn make_copy(
    run: &Mutex<Run>,
    batch: &Batch,
    root: &Path,
    round: &Round,
) -> Option<(Where, BTreeMap<usize, Changes>)> {
    loop {
        // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
        let to_take = round.to_take();
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        let mut where_ = Where::of(root, &batch.identifier, round.copies);
        let Where::Copy(mirror) = &mut where_ else {
            return Some((where_, BTreeMap::new()));
        };
        let mut took: BTreeMap<usize, Changes> = BTreeMap::new();
        let mut conflicted: Option<usize> = None;
        // The changes are merged in, in plan order, as a batch's files are merged into the
        // root.
        // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
        for (waited, changes) in to_take {
            // @lfy def/cli/main.lfy:main#main:main:f3bb66e88d43cf4578514a8d298ea474abb5cefc6ca529992953e31d189978e8
            match merge_changes(&changes, &mirror.directory, &mirror.scratch()) {
                Ok(_) => {
                    took.insert(waited, changes);
                }
                // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
                Err(path) => {
                    let message = format!(
                        "{path} of {} could not be taken into the copy",
                        round.planned[waited].batch.identifier
                    );
                    locked(run).reporter.append(&message);
                    conflicted = Some(waited);
                    break;
                }
            }
        }
        // The changes of a batch could not be merged into the copy: the copy is removed and
        // the batch starts only once that batch has its files merged into the root.
        // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
        if let Some(waited) = conflicted {
            // @lfy def/cli/main.lfy:main#main:main:49481a50bf94e617c823f0536f8d44cdb0ca79898e0c7d84e0bd7ac9070d5960
            mirror.remove();
            round.wait_for_one(waited);
            // @lfy def/cli/main.lfy:main#main:main:7abeb271cb2f0100d93f668ddc16303e063bcd2d3df7de58cd6b6d23d0f9ea21
            if round.held().states[waited] == State::Stopped {
                return None;
            }
            continue;
        }
        // What each file of the copy holds once the changes it takes are merged in, and
        // before any kept work is taken, is kept as the batch's base.
        // @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
        mirror.rebase();
        return Some((where_, took));
    }
}

/// Every batch of a round run: a batch starts once every batch holding a dependency of one of
/// its units is done, at most --jobs run at once, and among those that can start they start in
/// plan order. The batches that never started are the ones that did not complete.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
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
    // A batch runs in a copy of the root when more than one may run at once and the root is a
    // git repository; it runs in the root itself otherwise.
    // @lfy def/cli/main.lfy:main#main:main:cab5d4506292d57bf47d916ae73f8496c6abca998665c7cf57c137c631bd51e0
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    let copies = jobs > 1 && git_directory(root).is_some();
    std::thread::scope(|scope| {
        let mut running = Vec::new();
        loop {
            // The lock on the compile is taken and given back before the schedule's, so
            // that no thread ever holds both.
            // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
            let halted = locked(run).halted;
            let mut schedules = schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
            match schedules.next(planned, jobs, halted, copies) {
                // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
                Next::Start(at) => {
                    drop(schedules);
                    let batch = planned[at].batch.clone();
                    // A stopped batch stops only the batches that depend on one of its
                    // units; those never run, and count as not completed.
                    // @lfy def/cli/main.lfy:main#main:main:fef239376f4395cd11e23eb728a352b07322301b631d9069086add1723844c11
                    if locked(run).depends_on_stopped(&batch) {
                        let mut state = locked(run);
                        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
                        state.incomplete.push(batch.identifier.clone());
                        state.stopped.extend(batch.units.iter().copied());
                        drop(state);
                        schedule
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .finish(at, &Done::Stopped);
                        finished.notify_all();
                        continue;
                    }
                    let schedule = &schedule;
                    let finished = &finished;
                    running.push(scope.spawn(move || {
                        // @lfy def/cli/main.lfy:main#main:main:db1a5e729050c2e7e4c514c2b1f563a02e48f134099c96fb1af0d2a471cd6bbf
                        let round = Round { schedule, finished, planned, at, copies };
                        let done = match (accept_only, compiler) {
                            // @lfy def/cli/main.lfy:main#main:main:e6df0c7548d8f79abfca3e578a974cfbbf2949927b4851d9860223b1315c6c27
                            (true, _) => {
                                Run::accept_on_disk(run, &batch, root);
                                Done::Merged
                            }
                            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
                            (false, Some(command)) => run_batch(run, &batch, command, root, &round),
                            // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
                            // @lfy def/cli/main.lfy:main#main:main:3a07f6a4f45336a9a4e1b203912309522b7f93919e3caacfb488b3638af551bd
                            (false, None) => {
                                locked(run).write_request(&batch);
                                Done::Merged
                            }
                        };
                        schedule.lock().unwrap_or_else(std::sync::PoisonError::into_inner).finish(at, &done);
                        finished.notify_all();
                    }));
                }
                // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
                Next::Wait => {
                    drop(finished.wait(schedules));
                }
                // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
                Next::Over => {
                    let unstarted = schedules.unstarted();
                    drop(schedules);
                    let mut state = locked(run);
                    for at in unstarted {
                        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
                        state.incomplete.push(planned[at].batch.identifier.clone());
                        state.stopped.extend(planned[at].batch.units.iter().copied());
                    }
                    break;
                }
            }
        }
        // The batches already running finish as they would, and the global review waits for
        // every one of them, so it reads a root every batch has been merged into.
        // @lfy def/cli/main.lfy:main#main:main:c8d886404dc8c6b8b7e3dab97f05bfe2bebb213411910915c4fc538951c0882e
        // @lfy def/cli/main.lfy:main#main:main:b560d79fb66587371f4a98bc89eea6db2f1123b1c6455f3310c1a9827b77b43e
        for handle in running {
            let _ = handle.join();
        }
    });
}

/// The work a rejected run of this batch kept, when `elfie-compile/cache/kept/<batch>.json`
/// under the root names the batch's target and the stems of its units in batch order and
/// gives each unit the hash and the requirements that are its own now; nothing otherwise.
// @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
// @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
fn kept_work(run: &Mutex<Run>, batch: &Batch, root: &Path) -> Option<Kept> {
    let path = kept_file(root, &batch.identifier);
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let held = Kept::of_json(&value)?;
    // @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
    let asked = locked(run).asked_kept(batch);
    // @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
    held.is_for(&asked).then_some(held)
}

/// The kept work taken into a batch's copy, after the base was kept: each file it holds is
/// merged into the copy as the three-way merge of what the copy holds for it and its kept
/// text against its kept base, and a file whose merge has a conflict is left as the copy
/// holds it, which is neither a rejection nor a reason to run the batch again.
///
/// One resuming progress line counts the files taken and the files left. Nothing is taken
/// when the batch runs in the root, because a rejected batch's outputs are left on disk
/// there.
// @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
// @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
fn take_kept(run: &Mutex<Run>, batch: &Batch, where_: &Where, kept: Option<&Kept>) -> Option<Kept> {
    let held = kept?;
    // @lfy def/cli/main.lfy:main#main:main:8444ea8a2d9ff6b491b88ffdab12f7f13f9596b9dfd77f20d2a51a121c7cad69
    let Where::Copy(mirror) = where_ else {
        return None;
    };
    let scratch = mirror.scratch();
    let mut taken = 0;
    let mut left = 0;
    for file in &held.files {
        let at = mirror.directory.join(&file.path);
        let now = fs::read(&at).ok();
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let merged = match (&file.base, &file.text, &now) {
            // The copy holds what the kept work was based on: the kept text stands.
            // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
            (base, text, now) if base.as_deref().map(str::as_bytes) == now.as_deref() => text.clone(),
            // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
            (Some(base), Some(text), Some(now)) => {
                merged(now, base.as_bytes(), text.as_bytes(), &scratch)
                    .map(|held| String::from_utf8_lossy(&held).into_owned())
            }
            // One of the three missing is a file added or removed on both sides, which no
            // merge settles.
            // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
            _ => None,
        };
        // A kept file whose merge has a conflict is left as the copy holds it.
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let Some(text) = merged else {
            left += 1;
            continue;
        };
        // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
        if let Some(parent) = at.parent() {
            let _ = fs::create_dir_all(parent);
        }
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let written = if text.is_empty() && file.text.is_none() {
            fs::remove_file(&at).is_ok()
        } else {
            fs::write(&at, &text).is_ok()
        };
        if written {
            taken += 1;
        } else {
            left += 1;
        }
    }
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    let message = format!("{taken} files taken, {left} left");
    locked(run).reporter.report(Step::Resuming, Some(&batch.identifier), None, &message);
    Some(Kept {
        target: held.target.clone(),
        units: Vec::new(),
        // @lfy def/cli/main.lfy:main#main:main:42f4bccc722123748448fab03bfe2e9f1e2ec7530f09efd662aa8ce9f1cb26a2
        problems: held.problems.clone(),
        files: Vec::new(),
    })
}

/// The kept work of a batch: its target, its units, the problems of the rejection that
/// stopped it, and each file of its copy that would be merged into the root were it done, in
/// path order.
// @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
fn kept_of(run: &Mutex<Run>, batch: &Batch, mirror: &Mirror, problems: &[String]) -> Kept {
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    let mut kept = locked(run).asked_kept(batch);
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    kept.problems = problems.to_vec();
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    kept.files = mirror
        .changes()
        .into_iter()
        .map(|(path, change)| KeptFile {
            path,
            // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
            base: change.base.map(|held| String::from_utf8_lossy(&held).into_owned()),
            // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
            text: change.text.map(|held| String::from_utf8_lossy(&held).into_owned()),
        })
        .collect();
    kept
}

/// The kept work of a batch written to `elfie-compile/cache/kept/<batch>.json` under the
/// root, replacing what that file held, before its copy is left in place.
// @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
fn write_kept(run: &Mutex<Run>, batch: &Batch, root: &Path, mirror: &Mirror, problems: &[String]) {
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    let kept = kept_of(run, batch, mirror, problems);
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    let text = format!("{}\n", serde_json::to_string_pretty(&kept.to_json()).unwrap_or_default());
    let path = kept_file(root, &batch.identifier);
    // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
    let written = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&path, &text));
    if let Err(error) = written {
        let mut state = locked(run);
        complain(state.style.err, Some(&path.display().to_string()), &error.to_string());
        state.worsen(ExitCode::Failure);
    }
}

/// The files another batch's merge wrote into the root taken into this batch's copy, its base
/// set to them in place of what it took, and the target's build and test commands run there,
/// as they run for a batch whose units all have the reason dependency.
///
/// Nothing when they cannot be merged into the copy without a conflict, when either command
/// fails, or when the target has no line for build or for test: the reason the batch runs
/// again instead.
// @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
// @lfy def/cli/main.lfy:main#main:main:b4ffc629cbf563b3e2f3d8f96680345c330cce56013ce20eb8fb1217658eac0d
fn remerge(
    run: &Mutex<Run>,
    batch: &Batch,
    mirror: &mut Mirror,
    differs: &Changes,
    runs: &Runs,
) -> Result<(), String> {
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    if let Err(path) = merge_changes(differs, &mirror.directory, &mirror.scratch()) {
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        return Err(format!("{path} reached the root with other work and could not be taken"));
    }
    // The base takes the files that batch's merge wrote into the root in place of those it
    // took.
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    for (path, change) in differs {
        match &change.text {
            Some(text) => {
                mirror.base.insert(path.clone(), text.clone());
            }
            None => {
                mirror.base.remove(path);
            }
        }
    }
    // The target has no line for build or for test: the batch runs again rather than being
    // merged on nothing.
    // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
    if !locked(run).builds_and_tests(batch) {
        // @lfy def/cli/main.lfy:main#main:main:f787e163d40f9023ad638dd789bab0a8201e514791bc0d85d332f7441ec9b5e5
        return Err("the target has no build and test to check the merge with".to_string());
    }
    // @lfy def/cli/main.lfy:main#main:main:74f2ac185a24604969b28a7efc22aaf4d3a13819529ba16f1f40e7aa2179032d
    // @lfy def/cli/main.lfy:main#main:main:b4ffc629cbf563b3e2f3d8f96680345c330cce56013ce20eb8fb1217658eac0d
    let directory = mirror.directory.clone();
    Run::build_and_test(run, batch, &directory, runs)
}

/// The diagnostics a compile prints when loading found an error: all of them, so a warning
/// stands beside the error that stopped the run. Empty when none of them is an error.
// @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
fn loaded_diagnostics(workspace: &Workspace) -> Vec<Diagnostic> {
    let diagnostics = query::diagnostics_of(workspace, None);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        diagnostics
    } else {
        Vec::new()
    }
}

/// The diagnostics a verify prints: the error ones alone.
// @lfy def/cli/main.lfy:main#main:main:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
fn error_diagnostics(workspace: &Workspace) -> Vec<Diagnostic> {
    query::diagnostics_of(workspace, None).into_iter().filter(|d| d.severity == Severity::Error).collect()
}

/// The diagnostics of the loaded program printed, when one of them is an error; whether one
/// was.
// @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
fn print_loaded_diagnostics(workspace: &Workspace, json: bool, on: bool) -> bool {
    let diagnostics = loaded_diagnostics(workspace);
    for diagnostic in &diagnostics {
        print_diagnostic(diagnostic, json, on);
    }
    !diagnostics.is_empty()
}

/// Every error diagnostic of the program printed; whether there was one.
// @lfy def/cli/main.lfy:main#main:main:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
fn print_errors(workspace: &Workspace, json: bool, on: bool) -> bool {
    let errors = error_diagnostics(workspace);
    for diagnostic in &errors {
        print_diagnostic(diagnostic, json, on);
    }
    !errors.is_empty()
}

/// The target --target limits the plan to, when it names a known one.
// @lfy def/cli/main.lfy:main#main:main:ab0e6c732c90fb6b7fa9232e9b5f894e7f913233bb84f30a1d08a14830314820
// @lfy def/cli/main.lfy:main#main:main:aa8d8f7253225d4bbfb5d6995399aa58057a616f7d55984c01a6687dc0ff2ee9
fn target_named(workspace: &Workspace, invocation: &Invocation) -> Result<Option<String>, ExitCode> {
    let Some(name) = invocation.option("target") else {
        return Ok(None);
    };
    if !workspace.targets.iter().any(|known| known.identifier == name) {
        // A usage message naming the identifiers of the targets, nothing compiled or
        // reviewed, and the code is usage.
        // @lfy def/cli/main.lfy:main#main:main:022c51b85274d94f99424138e1f93c2421f285701b3ad67ff7eba4845f51626a
        // @lfy def/cli/main.lfy:main#main:main:315342ecb200e927c15546ab751e43c381772c3fa07f27e1d0f154a7c2b09afd
        let known: Vec<&str> = workspace.targets.iter().map(|t| t.identifier.as_str()).collect();
        complain(
            Paint::of(invocation).err,
            None,
            &format!("{name} is not a target; the targets are: {}", known.join(", ")),
        );
        // @lfy def/cli/main.lfy:main#main:main:305f0dfcde799b22957fd531d22657ae8b808df4f65b4301dbd9bbdf42a3e1cd
        return Err(ExitCode::Usage);
    }
    Ok(Some(name.to_string()))
}

/// Plans the units, runs the compiler on each batch in plan order, and records what it
/// produced.
// @lfy def/cli/main.lfy:main
fn compile(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    // @lfy def/cli/main.lfy:main#main:main:c0ff8dc13036c4a0c6308abadabef558ad51da34c43aeed7d60f6cc9a97a368c
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // The compiler is never handed a program with problems: no compiler runs, nothing is
    // generated, and the code is problems.
    // @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
    // @lfy def/cli/main.lfy:main#main:main:3df84098154780d5074fdbf4d1209831b3e3f8bc87b7bc577035c1b63e2f443d
    if print_loaded_diagnostics(&workspace, json, style.out) {
        // @lfy def/cli/main.lfy:main#main:main:77ae4e6a5e2396d858d173399db2122dd9965d17bc44b4f05613dfae2362f448
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // With --target the maps of that target alone are read, so no other target's map files
    // are opened.
    // @lfy def/cli/main.lfy:main#main:main:81eec0e83e4938bc3ddd792baad0cebc287a56344dcba0dfaf720cdfde05f3eb
    // @lfy def/cli/main.lfy:main#main:main:6b9feed8c0c1dd698132e42cc588b0234d6d80924e6e0ecb4b3e6d566b947e8d
    let maps = generation::source_maps_of(&workspace, target.as_deref());
    let legacy = legacy_targets(&workspace, target.as_deref());
    drop(workspace);
    // The units the last global review found violated.
    // @lfy def/cli/main.lfy:main#main:main:37aa77cbc42fd4dcc40b7262ac25164abbc8c899273439860ff39c8c39002a0d
    let (program, plan) = planned(root, &maps, &invocation.arguments, invocation.flag("all"), &violated_ids(root));
    // A cycle among files that no move resolves has no order to build in: its problem is
    // printed as an error and nothing is generated.
    // @lfy def/cli/main.lfy:main#main:main:055ed621ccb4cfc04dafc60c4ae42b146676fbac4139bd00ac26ddae87c1ad70
    for problem in &plan.problems {
        print_plan_problem(&program.workspace, problem, json, style.out);
    }
    let cycles = !plan.problems.is_empty();
    // @lfy def/cli/main.lfy:main#main:main:5f8a307089850766f7edc63e08cf04c35a137c49fe5052d7ac979ef3f1e491ea
    if invocation.flag("dry-run") {
        let code = dry_run(&program.workspace, &plan, target.as_deref(), json, style.out);
        // @lfy def/cli/main.lfy:main#main:main:055ed621ccb4cfc04dafc60c4ae42b146676fbac4139bd00ac26ddae87c1ad70
        return if cycles { ExitCode::Problems } else { code };
    }
    // Nothing is generated, and the code is problems.
    // @lfy def/cli/main.lfy:main#main:main:e0179e86c25f7960127d204e9f6499cb8cf0495adac3ba8441ff18238b46840d
    if cycles {
        // @lfy def/cli/main.lfy:main#main:main:d45b61bd01620de4f1c204c2a1a111fa2a8b3b2fff6d63bb3cf7faf41c8f0739
        return ExitCode::Problems;
    }
    // Before any batch runs, the maps a target kept in source-map.json are moved into the
    // units' map files and every map file belonging to no unit of the plan is removed.
    // @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
    // @lfy def/cli/main.lfy:main#main:main:531885eb3c4021855f9d9fbc7e806cedbc5da481cdcee0ca5e70c32b6edaad0d
    move_legacy_maps(&program.workspace, &plan, &legacy);
    remove_orphan_maps(&program.workspace, &plan, target.as_deref());
    // The work kept for a batch that named a stem the plan no longer holds goes the same way,
    // so nothing takes work answering for a unit that is gone.
    // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
    remove_orphan_kept(&program.workspace, &plan, target.as_deref());
    // A review record file belonging to no unit of the plan is removed before any batch runs,
    // the same way, so that nothing carries a verdict for a unit that is gone.
    // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
    if verifier_command(root).is_some() && !invocation.flag("no-verify") {
        remove_orphan_reviews(&program.workspace, &plan, target.as_deref());
    }

    let reporter = Reporter::new(json, style, 0, log_paths(root));
    let run = Mutex::new(Run::new(program, plan, maps.clone(), reporter, invocation));
    let compiler = compiler_command(root);
    let accept_only = invocation.flag("accept");
    // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    let jobs = compile_jobs(invocation, root);
    // The units a violated global review plans are compiled once more in the same compile,
    // and the global review runs once more after them.
    // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
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
            // @lfy def/cli/main.lfy:main#main:main:4da6fc0ef0b92bdd309365b39295a5cc5de6e9d3296556c8da4e7fc7e1c11920
            // @lfy def/cli/main.lfy:main#main:main:3b7ff1cb8c57ebc94c72273c9e46802fa9a2881815174d5e72e44b93e5ad00ea
            // The message counts the units planned, the batches, and the moves.
            // @lfy def/cli/main.lfy:main#main:main:2d74846758ea9c2aee36d43921093dbef40242c72894348bd117518c4fdcbe46
            let planned =
                format!("{total} units planned, {} batches, {} moves", batches.len(), state.plan.moves.len());
            state.reporter.report(Step::Planned, None, None, &planned);
            // The move lines follow the planned line, each indented four spaces.
            // @lfy def/cli/main.lfy:main#main:main:99a4adc185fa87df9a6aacaa34f84261addbd0756d5dc199405c5bc004f3e357
            if !json {
                for moved in &state.plan.moves {
                    let line = move_text(&state.program.workspace, &state.plan, moved, style.out);
                    println!("{INDENT}{line}");
                    let plain = move_text(&state.program.workspace, &state.plan, moved, false);
                    state.reporter.append(&format!("{INDENT}{plain}"));
                }
            }
            // @lfy def/cli/main.lfy:main#main:main:5b05d045fbdda6a51d4ac309b97ddac4c12d0a0e967117d6973df4878cc1bc96
            if total == 0 && round == 1 && !json {
                // @lfy def/cli/main.lfy:main#main:main:5b05d045fbdda6a51d4ac309b97ddac4c12d0a0e967117d6973df4878cc1bc96
                println!("{}", paint("every unit is up to date", Tone::Success, style.out));
            }
            state.accepted_batch = false;
            // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
            round_of(&state.plan, &batches)
        };
        // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
        run_batches(&run, &round_of_batches, root, compiler.as_deref(), accept_only, jobs);
        // Every batch has been handled and merged into the root: the global criteria and
        // tests are reviewed once, there.
        // @lfy def/cli/main.lfy:main#main:main:c2a64cff68c468890c2b64bb46be8472891c54f12fce3e4a2d404d4b2e5f0969
        // @lfy def/cli/main.lfy:main#main:main:b560d79fb66587371f4a98bc89eea6db2f1123b1c6455f3310c1a9827b77b43e
        let violated = Run::global_review(&run, root);
        // The second global review of a compile stops it there, and the next compile plans
        // those units with reason violated.
        // @lfy def/cli/main.lfy:main#main:main:ffc6f99569b9c0ce1130f02761a91460937988ff1af7dea237a2802d41f23926
        if violated.is_empty() || round == 2 || accept_only || compiler.is_none() {
            // The second global review of a compile stops it there with the code problems.
            // @lfy def/cli/main.lfy:main#main:main:96dd6a88ebe0003d78a67421241c801e1f0e0faf8ced5a63a7825b67a31a2729
            if !violated.is_empty() {
                locked(&run).worsen(ExitCode::Problems);
            }
            break;
        }
        // The units it planned are compiled once more in the same compile, and the global
        // review runs once more after them.
        // @lfy def/cli/main.lfy:main#main:main:37688db5376890a46f433004a7e76905a0292b3adf375e72df9a7b7ebc9ef5f5
        // @lfy def/cli/main.lfy:main#main:main:5e145e7cf95605462ea683b7b1dffa98f7e1ee252c92165b2c3cda98cc745aea
        let (program, plan) = plan_with(root, &maps, &[], &violated);
        locked(&run).replan(program, plan, 0);
        round = 2;
    }
    // Every session of this compile is ended and the server stopped, after the last run has
    // returned and before the command returns, whatever the outcome.
    // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    let server = locked(&run).agent.server.clone();
    server.stop();
    let mut state = locked(&run);
    // The last progress line of a compile is finished, with the count accepted, rejected,
    // blocked, and failed.
    // @lfy def/cli/main.lfy:main#main:main:4da6fc0ef0b92bdd309365b39295a5cc5de6e9d3296556c8da4e7fc7e1c11920
    state.finish();
    // Every batch accepted is success; a rejection, a block, or a question is problems; a
    // command that could not run is failure.
    // @lfy def/cli/main.lfy:main#main:main:ebb4c47d5bade2bf74c3b8f602d49839293626ec296c19748df60b3f7ed90985
    // @lfy def/cli/main.lfy:main#main:main:6c350b9b103d6929538d70b4ac5e18843079843cb444d71311ec205b548c08f4
    // @lfy def/cli/main.lfy:main#main:main:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
    state.code
}

// ---- verify -----------------------------------------------------------------------

/// Verify is compile without the compiler: the same plan, the same review request, the same
/// verifier, so a person can ask for an opinion on outputs already recorded. Nothing is
/// compiled and no output is written.
// @lfy def/cli/main.lfy:main#main:main:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
fn verify(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    // @lfy def/cli/main.lfy:main#main:main:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
    let workspace = workspace::load(root);
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // Every error diagnostic is printed and verify ends with the code problems.
    // @lfy def/cli/main.lfy:main#main:main:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
    if print_errors(&workspace, json, style.out) {
        // @lfy def/cli/main.lfy:main#main:main:cd777dab37a66b80b804ad0e2c1c6eb305a85f5c6f365759da5469e81ffe4dbf
        return ExitCode::Problems;
    }
    let target = match target_named(&workspace, invocation) {
        Ok(target) => target,
        Err(code) => return code,
    };
    // Nothing under elfie-compile is recorded, moved, or removed: the maps are only read.
    // @lfy def/cli/main.lfy:main#main:main:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
    let maps = generation::source_maps_of(&workspace, target.as_deref());
    drop(workspace);
    // Plan.problems is neither printed nor counted in the exit code here, and the review goes
    // on: a person asked for an opinion on what is recorded, not for a build.
    // @lfy def/cli/main.lfy:main#main:main:304ed515dfa3121d80f29c65da75d99d67be3708e1f5c3c46961a2dbbbc46322
    let (_, known) = plan_with(root, &maps, &[], &[]);
    // Every stem given as a requested unit or, with no stem, every unit whose outputs are
    // not empty.
    // @lfy def/cli/main.lfy:main#main:main:c98c75ccd7e9b5e3cb007cb35cb2628fd4d93779a93201868a8bf2df0c6ac604
    // @lfy def/cli/main.lfy:main#main:main:d665f83cc9e0222f6d5e72fcdc1506878985d1e586251c478035d704af123680
    let mut requested: Vec<String> = Vec::new();
    // --global with no stem reviews no batch.
    // @lfy def/cli/main.lfy:main#main:main:89ce3db657a93c0c652675c6692134a009e6bf11ceecea94526a6516e7768d43
    let global_only = invocation.flag("global") && invocation.arguments.is_empty();
    // @lfy def/cli/main.lfy:main#main:main:d665f83cc9e0222f6d5e72fcdc1506878985d1e586251c478035d704af123680
    if invocation.arguments.is_empty() {
        requested.extend(known.units.iter().filter(|unit| !unit.outputs.is_empty()).map(|unit| unit.stem.clone()));
    } else {
        // @lfy def/cli/main.lfy:main#main:main:c98c75ccd7e9b5e3cb007cb35cb2628fd4d93779a93201868a8bf2df0c6ac604
        for stem in &invocation.arguments {
            // A stem naming a unit whose outputs are empty is printed as having nothing to
            // review and left out, and the code does not change for it.
            // @lfy def/cli/main.lfy:main#main:main:f6ecc4facc68022e7002a7e92ab9ba501e6cdd6242d9d87adb8b05ae09c3533f
            // @lfy def/cli/main.lfy:main#main:main:62de602001e5a8cd6c08fe0b373ce392f7dfbc93fe340e17701e10e49d405d69
            match known.units.iter().find(|unit| &unit.stem == stem) {
                Some(unit) if !unit.outputs.is_empty() => requested.push(stem.clone()),
                _ if json => println!("{}", serde_json::json!({ "stem": stem, "message": "nothing to review" })),
                _ => println!("{stem}: nothing to review"),
            }
        }
    }
    // After any batches are reviewed, every global criterion and test is reviewed once with
    // no stem given, or with --global.
    // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
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
    // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
    // @lfy def/cli/main.lfy:main#main:main:aa8d8f7253225d4bbfb5d6995399aa58057a616f7d55984c01a6687dc0ff2ee9
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
    // At most --jobs batches are reviewed at once.
    // @lfy def/cli/main.lfy:main#main:main:d70597c60c888f5f421bf553ba38472c5a50057472a0e699ac64627f24b6c8c7
    let jobs = jobs_of(invocation);
    // The source maps already recorded are what the verifier is pointed at here: verify
    // compiles nothing, so there are none to hold.
    // @lfy def/cli/main.lfy:main#main:main:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
    let recorded = locked(&run).maps.clone();
    // @lfy def/cli/main.lfy:main#main:main:89ce3db657a93c0c652675c6692134a009e6bf11ceecea94526a6516e7768d43
    let wanted = if global_only { Vec::new() } else { batches };
    // Each batch of the plan holding a requested unit, up to --jobs at once and started in
    // plan order.
    // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
    let reviewing = round_of(&locked(&run).plan, &wanted);
    verify_batches(&run, &reviewing, root, verifier.as_deref(), &recorded, jobs);
    // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
    if globally && verifier.is_some() {
        // Nothing of the compile's own gating applies here: a person asked for the opinion.
        // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
        let reviews = Run::verify_globally(&run, root);
        // A global review carries nothing: every global criterion and test is reviewed once.
        // @lfy def/cli/main.lfy:main#main:main:a3182832b5dbbb14cd7eb5451929bf7d8a209809b74510a1887bf3cf542b0b30
        print_reviews(&reviews, &[], 0, json, style.out);
        // @lfy def/cli/main.lfy:main#main:main:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
        if counts_of(&reviews).1 > 0 {
            locked(&run).worsen(ExitCode::Problems);
        }
    }
    // Every session of this verify is ended and the server stopped, after the last run has
    // returned and before the command returns, whatever the outcome.
    // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    let server = locked(&run).agent.server.clone();
    server.stop();
    // No review is violated and the verifier was started, or no review request asked
    // anything: the code is success; it could not be started: the code is failure.
    // @lfy def/cli/main.lfy:main#main:main:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
    // @lfy def/cli/main.lfy:main#main:main:f4d733d1c98fcb52efb7af2bcf5a54e5df115f3454b8c0ce42aa336de6dd2857
    locked(&run).code
}

/// Every batch of a verify reviewed, up to --jobs at once and started in plan order, each in
/// the root: nothing is compiled, so every batch reads the outputs that are already there.
///
/// With no verifier named, the review request of each batch is written for a verifier run by
/// hand and the code is success.
// @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
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
            // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
            match schedules.next(planned, jobs, false, false) {
                Next::Start(at) => {
                    drop(schedules);
                    let batch = planned[at].batch.clone();
                    let schedule = &schedule;
                    let finished = &finished;
                    running.push(scope.spawn(move || {
                        match verifier {
                            // @lfy def/cli/main.lfy:main#main:main:49af607a5cd69c0852af2fe4631f8f52d787fd863023e06071d7569c7890cd42
                            Some(command) => {
                                // The run is one stream of the command, with the review
                                // request as its input and the root as its directory; a run
                                // that ended without every verdict is run once more.
                                // @lfy def/cli/main.lfy:main#main:main:dc6670a1659abe98c2ae0ed1502d175a57162b9aa395c843f4fdeaa3d5ce4664
                                // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
                                let where_ = Where::Root(root.to_path_buf());
                                // What the batch's review carries: what each of its units was
                                // last reviewed as, and nothing at all with --full.
                                // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
                                // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
                                let records = locked(run).records_of(&batch);
                                // A verify's run of the verifier is a first run, so it goes
                                // on from nothing.
                                // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
                                let runs = {
                                    let mut state = locked(run);
                                    let agent = state.agent.clone();
                                    Runs::new(agent, state.running_of(&batch.identifier))
                                };
                                let verification = Run::review_batch(
                                    run, &batch, command, &where_, false, recorded, &records, &runs, false,
                                );
                                let carried = verification.request.carried.len();
                                let report = verification.report;
                                let reviews = report.reviews;
                                let mut state = locked(run);
                                // Every review printed, then the counts, then the id of every
                                // criterion and test the review left unanswered. A review
                                // request that asked nothing ran no verifier, so it prints no
                                // review and counts none.
                                // @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
                                // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
                                // @lfy def/cli/main.lfy:main#main:main:f79d62c52638caa4cb00a160316bc7052bc62e29caef830e683dba3fca9581ca
                                // @lfy def/cli/main.lfy:main#main:main:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
                                print_reviews(&reviews, &report.missing, carried, state.json, state.style.out);
                                // @lfy def/cli/main.lfy:main#main:main:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
                                if counts_of(&reviews).1 > 0 {
                                    state.worsen(ExitCode::Problems);
                                }
                            }
                            // @lfy def/cli/main.lfy:main#main:main:3e861a06695127864ab425d08e899da99dbf6789c4c55135fca304c37a7e5e18
                            // @lfy def/cli/main.lfy:main#main:main:b6938589b155e207c8b6d2d220a8d8726be7f451d3867f2a1d037c89951d9923
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
///
/// Every id the review was asked about that no review names is counted among the unverifiable
/// and then named, one line under the counts line, as not reviewed; every verdict the review
/// request carried is counted after them, and only there.
// @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
// @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
// @lfy def/cli/main.lfy:main#main:main:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
fn print_reviews(reviews: &[Review], unanswered: &[String], carried: usize, json: bool, on: bool) {
    let missing = unanswered.len();
    for review in reviews {
        // @lfy def/cli/main.lfy:main#main:main:639501265f62b510499fe150b611345c387104c04de980656103153401de8d17
        if json {
            println!("{}", review.to_json());
        } else {
            // @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
            println!("{}", painted_review(review, on));
        }
    }
    // After each batch one line gives the counts of satisfied, violated, and unverifiable,
    // every id no review names counted among the unverifiable.
    // @lfy def/cli/main.lfy:main#main:main:a401e7319fbb4bdf617fc08adbbe3fea18e30d1c519f1fcbf538029c7dc283a2
    // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
    let counts = counts_of(reviews);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "satisfied": counts.0,
                "violated": counts.1,
                "unverifiable": counts.2 + missing,
                // @lfy def/cli/main.lfy:main#main:main:cc320fe0b40fde7c47dca674e64001b0a974e93cf2ffa2186a7c30bbf7aad8f7
                "carried": carried,
            })
        );
    } else {
        // The counts line begins with the satisfied, the violated, and the unverifiable, and
        // ends with the verdicts the review request carried when it carried any.
        // @lfy def/cli/main.lfy:main#main:main:5e715c81a6d8d3edeadda85d5a0a8cc96d3f5aa33c9009f7b213ae941ccb4eff
        // @lfy def/cli/main.lfy:main#main:main:cc320fe0b40fde7c47dca674e64001b0a974e93cf2ffa2186a7c30bbf7aad8f7
        println!("{}", counts_message(&counts_with_carried(reviews, missing, carried), on));
        // Each id no review names is named as not reviewed, on one line under the counts
        // line, and counts as unverifiable above.
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        for id in unanswered {
            println!("{INDENT}{}", paint(&not_reviewed(id), Tone::Warning, on));
        }
    }
}

/// One review as the verify command prints it, painted.
// @lfy def/cli/main.lfy:main#main:main:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
fn painted_review(review: &Review, on: bool) -> String {
    // The status is padded to the longest status of ReviewStatus.
    // @lfy def/cli/main.lfy:main#main:main:0011d65e2e66e2fa6bffcf28bb68b59c7d3425bddda6fd61a196e714724e5195
    let column = [ReviewStatus::Satisfied, ReviewStatus::Violated, ReviewStatus::Unverifiable]
        .iter()
        .map(|status| width(&status.to_string()))
        .max()
        .unwrap_or(0);
    // The status is painted in its own tone, the file and line are subject, and the entity
    // and the note are plain.
    // @lfy def/cli/main.lfy:main#main:main:0011d65e2e66e2fa6bffcf28bb68b59c7d3425bddda6fd61a196e714724e5195
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

    /// The project manifest: the standard library beside the project, and the commands
    /// given. The target itself is declared in the program rather than here, so nothing
    /// names it, its output, or its dependencies.
    // @lfy def/cli/main.lfy:main
    fn manifest(commands: &[(&str, &str)]) -> String {
        let mut value = serde_json::json!({ "name": "p", "dependencies": { "elfie": { "root": "lib" } } });
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
            .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]))
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
    /// line: it answered for everything it was asked about all the same.
    const UNENDED: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\n";

    /// A verifier writing the end line and no review at all: it answered for nothing it was
    /// asked about, so it is run once more.
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    const NOTHING_REVIEWED: &str =
        "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho 'ELFIE: REVIEWED'\n";

    /// A verifier writing one satisfied review of the criterion of `def/a.lfy`, then the end
    /// line.
    const SATISFIED: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier that satisfies the criterion, writes a line that reads as an object but
    /// names no criterion, and then ends: a report that answered for every id it was asked
    /// about stands, whatever else it holds.
    // @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
    const SATISFIED_WITH_A_STRAY_OBJECT: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\necho '{\"thought\":\"I also looked at the tests\"}'\necho 'ELFIE: REVIEWED'\n";

    /// A verifier that satisfies the criterion and writes a line of its own to standard error,
    /// as an agent writes its progress there.
    const SATISFIED_WITH_NOISE: &str = "#!/bin/sh\ncat > /dev/null\necho verify >> \"$ELFIE_ROOT/verifications.txt\"\necho 'reading the outputs' >&2\necho '{\"id\":\"<id>\",\"status\":\"satisfied\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}'\necho 'ELFIE: REVIEWED'\n";

    /// The review records of a project removed, so that a review of it carries nothing and
    /// every criterion and test is asked about again.
    // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
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

    // @lfy def/cli/main.lfy:parse#parse:parse:9950bf574b6e14800ed642db3dc3aa89927c17ef13574689ea665810703ae049
    // @lfy def/cli/main.lfy:parse#parse:parse:3874a32a6a9ce574d2416d5c0171397b2a5a6f3c2857cb5d98d62c92e1a25770
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
    // @lfy def/cli/main.lfy:parse#parse:parse:a10f50e751f0b52b43514389889c366b65d704b81d4894a77a28e783fcbcebfc
    #[test]
    fn the_tree_flag_is_the_tree_command() {
        let invocation = parse_arguments(&args(&["--tree", "def/a.lfy"])).unwrap();
        assert_eq!(invocation.command, Command::Tree);
        assert_eq!(invocation.arguments, vec!["def/a.lfy".to_string()]);
        // A command named as well still names itself.
        assert_eq!(parse_arguments(&args(&["--tree", "def/a.lfy", "--json"])).unwrap().command, Command::Tree);
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:5f3d8a6fb3ccffa0749573417617024cf2ac816e469eea41263c10278b25db02
    #[test]
    fn nothing_naming_a_command_is_help() {
        assert_eq!(parse_arguments(&args(&[])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["--json"])).unwrap().command, Command::Help);
        // --tree followed by no file names no command, so nothing that does not begin with a
        // dash is given and the command is help; --tree is not the last argument here.
        assert_eq!(parse_arguments(&args(&["--tree", "--json"])).unwrap().command, Command::Help);
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:3fd497a063b547c5ecbba95bd47044f648030f147a69b7241615811b47f2f09d
    // @lfy def/cli/main.lfy:parse#parse:parse:407b80d916b66360f99967d28e89ba7016a7c488369e0a70acdef6af3936646d
    #[test]
    fn an_unknown_command_is_a_usage_error() {
        let error = parse_arguments(&args(&["frobnicate"])).unwrap_err();
        assert!(error.contains("frobnicate"));
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:3e415cc8c432995a2f20132ebe9faa1a51e783fa11b13cdc472e6a3089c09986
    #[test]
    fn an_option_taking_a_value_as_the_last_argument_is_a_usage_error() {
        for (arguments, option) in [
            (args(&["check", "--root"]), "--root"),
            (args(&["--tree"]), "--tree"),
            (args(&["compile", "--target"]), "--target"),
            (args(&["compile", "--jobs"]), "--jobs"),
        ] {
            let error = parse_arguments(&arguments).unwrap_err();
            assert!(error.contains(option), "{error}");
        }
        // With --help, -h, or --version among the arguments the command is named instead.
        assert_eq!(parse_arguments(&args(&["--help", "--root"])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["-h", "--tree"])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["--version", "--target"])).unwrap().command, Command::Version);
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:c6ef1cd5d1ee70ed5efe7e765138382e292ba2e31c91916182971283cce5fc2e
    // @lfy def/cli/main.lfy:parse#parse:parse:0c1236254ed2dc7214279b9f60c85ce5ef12cdbea38d4a41543459b1268af732
    #[test]
    fn an_option_written_with_an_equals_sign_gives_its_text() {
        let invocation = parse_arguments(&args(&["compile", "--target=rust"])).unwrap();
        assert_eq!(invocation.option("target"), Some("rust"));
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:b8252392bdacab15d1d46035c068176794361e8c15f22b6d21cd543f751e7956
    // @lfy def/cli/main.lfy:parse#parse:parse:252d8235d06b32c568f2bf1a13157b65c1788281bf471f3abe705c298312086c
    #[test]
    fn target_takes_the_value_that_follows_it() {
        // Written before the command, the value it takes does not name the command: the
        // argument after it does.
        // @lfy def/cli/main.lfy:parse#parse:parse:b8252392bdacab15d1d46035c068176794361e8c15f22b6d21cd543f751e7956
        let invocation = parse_arguments(&args(&["--target", "rust", "compile"])).unwrap();
        assert_eq!(invocation.command, Command::Compile);
        assert_eq!(invocation.option("target"), Some("rust"));
        assert!(invocation.arguments.is_empty());
        // Written after it, the value is the same.
        let invocation = parse_arguments(&args(&["compile", "--target", "rust"])).unwrap();
        assert_eq!(invocation.command, Command::Compile);
        assert_eq!(invocation.option("target"), Some("rust"));
        // --jobs is the other option besides --root that takes a following value.
        // @lfy def/cli/main.lfy:parse#parse:parse:252d8235d06b32c568f2bf1a13157b65c1788281bf471f3abe705c298312086c
        assert_eq!(parse_arguments(&args(&["compile", "--jobs", "8"])).unwrap().option("jobs"), Some("8"));
    }

    // @lfy def/cli/main.lfy:parse#parse:parse:3645fdc7e4fa098477e08321e6ca15068b540b9c205c6dd4dddb5f68dba4d549
    #[test]
    fn the_root_option_names_the_project_directory() {
        assert_eq!(parse_arguments(&args(&["check", "--root", "/tmp"])).unwrap().root, "/tmp");
        assert_eq!(parse_arguments(&args(&["check", "--root=/tmp"])).unwrap().root, "/tmp");
        // A relative directory is made absolute against the current directory, so every path
        // and ELFIE_ROOT built from it names the same place from any directory.
        // @lfy def/cli/main.lfy:parse#parse:parse:3645fdc7e4fa098477e08321e6ca15068b540b9c205c6dd4dddb5f68dba4d549
        let here = std::env::current_dir().unwrap().join("here");
        assert_eq!(Path::new(&parse_arguments(&args(&["check", "--root", "here"])).unwrap().root), here);
        assert_eq!(Path::new(&parse_arguments(&args(&["check", "--root=here"])).unwrap().root), here);
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
    // @lfy def/cli/main.lfy:parse#parse:parse:fff69e97f1f4842d2724ff711989149786a96a351c4d05ee30c6c64f1629a3d3
    #[test]
    fn help_and_version_win_anywhere() {
        assert_eq!(parse_arguments(&args(&["check", "--help"])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["-h"])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["--version"])).unwrap().command, Command::Version);
        assert_eq!(parse_arguments(&args(&["check", "--version"])).unwrap().command, Command::Version);
        // --version names the command only when neither --help nor -h is among the arguments.
        // @lfy def/cli/main.lfy:parse#parse:parse:fff69e97f1f4842d2724ff711989149786a96a351c4d05ee30c6c64f1629a3d3
        assert_eq!(parse_arguments(&args(&["--version", "--help"])).unwrap().command, Command::Help);
        assert_eq!(parse_arguments(&args(&["-h", "--version"])).unwrap().command, Command::Help);
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

    // @lfy def/cli/main.lfy:parse#parse:parse:d965a2707f01d2eabc754383f4dfbd1753a9cb3926e0ff6735022a72b0fcdfd7
    // @lfy def/cli/main.lfy:parse#parse:parse:0da4c49e8c974658ca2be342ff4ac6e0e21e09807b88714b57af4c15bca1404c
    // @lfy def/cli/main.lfy:parse#parse:parse:5df10c8ed49640a49819e6470920fbd7c525c644a77c7d9cb53141a553068141
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

    // @lfy def/cli/main.lfy:parse#parse:parse:55cfb0320f82b73e9a6a9d2faa97a3dd41367a5d9e70a5b07f676b249b39fcc1
    // @lfy def/cli/main.lfy:parse#parse:parse:233533aea4b9472e5d4e1d27500a3950212b9616536f3b46cf00ef040f59af88
    // @lfy def/cli/main.lfy:parse#parse:parse:2e73a4f8b63da86790f160a37ed9aa761be453dad91436a10b475b11625c450b
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
    // @lfy def/cli/main.lfy:main#main:main:6f0073ceecb972e98aae5b1618d058d12e3264a00f6faa1982ea18e0ca49ac39
    #[test]
    fn help_and_version_return_success() {
        let text = help_text(false);
        // @lfy def/cli/main.lfy:main#main:main:6f0073ceecb972e98aae5b1618d058d12e3264a00f6faa1982ea18e0ca49ac39
        for command in Command::ALL {
            assert!(text.contains(command.value()), "{text}");
            assert!(text.contains(command.description()), "{text}");
        }
        assert!(text.contains(Command::Verify.value()), "{text}");
        // @lfy def/cli/main.lfy:main#main:main:6f0073ceecb972e98aae5b1618d058d12e3264a00f6faa1982ea18e0ca49ac39
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
    // @lfy def/cli/main.lfy:main#main:main:614da7562f23fb56cf245bc4b5e0387a1baa11c20d41140d76a802d92bf85eb4
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
    // @lfy def/cli/main.lfy:main#main:main:ae6e191e9794908cdd3ebc2c237fba7833551209d9b35e730e67507e49532e80
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

    /// A program declaring no target says so once, as information of stage generation at
    /// elfie.json: it is neither a problem nor a stale unit, so the code is success even
    /// with --strict.
    // @lfy def/cli/main.lfy:main#main:main:ab61edddf16362666867261f64e558c75d78f00fb2fb566bd5a55a393d93fadc
    #[test]
    fn check_says_once_that_the_program_declares_no_target() {
        let fixture = Fixture::new();
        library(&fixture).write("def/a.lfy", "const x = 1;\n");
        // Nothing adds a target to global, so there is none to build for.
        // @lfy def/cli/main.lfy:main#main:main:ab61edddf16362666867261f64e558c75d78f00fb2fb566bd5a55a393d93fadc
        let workspace = workspace::load(&fixture.root);
        assert!(workspace.targets.is_empty(), "{:?}", workspace.targets.len());
        // The line is information, which is neither a problem nor a stale unit.
        // @lfy def/cli/main.lfy:main#main:main:ab61edddf16362666867261f64e558c75d78f00fb2fb566bd5a55a393d93fadc
        let manifest = fixture.root.join("elfie.json").display().to_string();
        print_no_target(&manifest, false, false);
        print_no_target(&manifest, false, true);
        print_no_target(&manifest, true, false);
        assert_eq!(severity_tone(Severity::Information), Tone::Active);
        // @lfy def/cli/main.lfy:main#main:main:ab61edddf16362666867261f64e558c75d78f00fb2fb566bd5a55a393d93fadc
        assert_eq!(fixture.run(&["check"]), ExitCode::Success.code());
        assert_eq!(fixture.run(&["check", "--strict"]), ExitCode::Success.code());
        assert_eq!(fixture.run(&["check", "--json"]), ExitCode::Success.code());
        // A program that does declare one says nothing of the kind.
        // @lfy def/cli/main.lfy:main#main:main:ab61edddf16362666867261f64e558c75d78f00fb2fb566bd5a55a393d93fadc
        one_target(&fixture);
        assert!(!workspace::load(&fixture.root).targets.is_empty());
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

    /// Tree prints one node or token per line indented by depth, a node as its rule and its
    /// token range, and a token as its value, the characters with escapes applied, never the
    /// raw characters it was written as.
    // @lfy def/cli/main.lfy:main#main:main:c888bea4f4ce679a5bef8e137a01e4d58fef7062c146500ddf5476aec6fe5668
    #[test]
    fn tree_prints_a_node_as_its_range_and_a_token_as_its_value() {
        let tokens = tokens_of("const x = \"a\\nb\";\n");
        let (raw, value) = {
            let escaped =
                tokens.iter().find(|token| token.value != token.raw).expect("a token whose escapes are applied");
            (escaped.raw.clone(), escaped.value.clone())
        };
        assert_eq!((raw.as_str(), value.as_str()), ("a\\nb", "a\nb"));
        let tree = parse(tokens, None);
        let lines = tree_lines(&tree, "def/a.lfy", false, false);
        // The root is its rule and its token range at no indent, and every line below it is
        // indented two spaces per level of depth.
        // @lfy def/cli/main.lfy:main#main:main:c888bea4f4ce679a5bef8e137a01e4d58fef7062c146500ddf5476aec6fe5668
        assert_eq!(lines[0], format!("{} 0..{}", tree.root.rule.identifier(), tree.tokens.len()));
        for line in &lines {
            assert_eq!((line.len() - line.trim_start().len()) % 2, 0, "{line:?} is indented by depth");
        }
        assert!(lines.iter().any(|line| line.starts_with("  ")), "nothing below the root is indented");
        // A token is given as its value, never as the characters it was written as.
        // @lfy def/cli/main.lfy:main#main:main:c888bea4f4ce679a5bef8e137a01e4d58fef7062c146500ddf5476aec6fe5668
        let shown = format!("{value:?}");
        assert!(lines.iter().any(|line| line.ends_with(&shown)), "no line gives {shown}: {lines:?}");
        let written = format!("{raw:?}");
        assert!(!lines.iter().any(|line| line.ends_with(&written)), "a line gives the raw characters");
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
        assert_eq!(leaves[0]["value"], tree.tokens[0].value);
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
    // @lfy def/cli/main.lfy:main#main:main:13ac07944d31f4503a5b719ea69b91838998614508f9fc1675e8dfd9d60a0e84
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
        // A --target naming no target of the program is a usage message naming the targets
        // there are, with nothing compiled.
        // @lfy def/cli/main.lfy:main#main:main:58a5e74700d0403711a6d2886923ae95a252437bac01a26eb1bde6f59d71b6a5
        // @lfy def/cli/main.lfy:main#main:main:022c51b85274d94f99424138e1f93c2421f285701b3ad67ff7eba4845f51626a
        assert_eq!(fixture.run(&["compile", "--dry-run", "--target", "go"]), ExitCode::Usage.code());
        // @lfy def/cli/main.lfy:main#main:main:315342ecb200e927c15546ab751e43c381772c3fa07f27e1d0f154a7c2b09afd
        assert_eq!(fixture.run(&["compile", "--target", "wasm"]), ExitCode::Usage.code());
        assert!(!fixture.root.join("out").exists(), "nothing is compiled");
    }

    /// `def/a.lfy` and `def/b.lfy` reach each other through the member a trait of
    /// `def/c.lfy` added to `B`, and `def/b.lfy` is used by every other file of the cycle,
    /// so `C` moves out of `def/a.lfy`'s unit into `def/b.lfy`'s.
    // @lfy def/cli/main.lfy:main
    fn a_move(fixture: &Fixture) -> &Fixture {
        one_target(fixture)
            .write("def/b.lfy", "d B: `A b` {\n  $x = string;\n}\n")
            .write("def/a.lfy", "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nd C {\n  $x = string;\n}\n")
            .write("def/c.lfy", "use \"./a\";\nuse \"./b\";\n\ntrait holder {\n  $c = C;\n}\nholder.apply(B);\n")
    }

    /// A move is printed before the units of a dry run as the word move, the target, the
    /// stem of its origin, the word into, the stem of the unit that compiles the entities,
    /// their identifiers in brackets, the word for, and the cause; in a check it is
    /// information of stage generation and is neither a problem nor a stale unit.
    // @lfy def/cli/main.lfy:main#main:main:0f5a7a1258a11b49d77120109972e47f61867f0f8fe2d59078aa9bfa4ffb1849
    // @lfy def/cli/main.lfy:main#main:main:2c23167a93d05731803b314b2b6545c5ee1481978f5c546aacb5c0b7f512115a
    #[test]
    fn a_move_is_printed_before_the_units_and_counts_as_no_problem() {
        let fixture = Fixture::new();
        a_move(&fixture);
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        assert!(program.workspace.problems.is_empty(), "{:?}", program.workspace.problems);
        assert_eq!(plan.problems, [], "no cycle is left unresolved");
        assert_eq!(plan.moves.len(), 1, "{:?}", plan.moves);
        let moved = &plan.moves[0];
        // The words and their order stay as they are. @lfy def/cli/main.lfy:main
        assert_eq!(move_text(&program.workspace, &plan, moved, false), "move rust a into b [C] for B.c");
        // Painting leaves the words and their order as they are.
        // @lfy def/cli/main.lfy:main#main:main:d76dbb95b25d73fb984ddecdb07366d73e2c932574c9a94208ad9df30c8d8933
        assert_eq!(
            width(&move_text(&program.workspace, &plan, moved, true)),
            width("move rust a into b [C] for B.c")
        );
        // A move is information of stage generation at the file of its origin.
        // @lfy def/cli/main.lfy:main
        print_move(&program.workspace, &plan, moved, false, false);
        print_move(&program.workspace, &plan, moved, false, true);
        print_move(&program.workspace, &plan, moved, true, false);
        assert_eq!(severity_tone(Severity::Information), Tone::Active);
        // A move is neither a problem nor a stale unit, so a check of a program with one is
        // success and nothing is generated by a dry run.
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["check"]), ExitCode::Success.code());
        assert_eq!(fixture.run(&["compile", "--dry-run"]), ExitCode::Success.code());
        assert_eq!(fixture.run(&["compile", "--dry-run", "--json"]), ExitCode::Success.code());
        assert!(!fixture.root.join("out").exists(), "nothing is generated");
    }

    /// The local criteria and tests of a unit are those of every entity it compiles, an
    /// entity moved into it from another file included: its declaration is in the file it
    /// came from, so its criteria and tests are taken from there rather than left out of the
    /// unit's review records.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    #[test]
    fn the_criteria_of_an_entity_moved_into_a_unit_are_asked_about() {
        let fixture = Fixture::new();
        one_target(&fixture)
            .write("def/b.lfy", "d B: `A b` {\n  $x = string;\n}\n")
            .write(
                "def/a.lfy",
                "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nd C {\n  $x = string;\n  @acceptanceCriteria\n    .add({ behavior = `Holds a string` });\n}\n",
            )
            .write("def/c.lfy", "use \"./a\";\nuse \"./b\";\n\ntrait holder {\n  $c = C;\n}\nholder.apply(B);\n");
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        assert!(program.workspace.problems.is_empty(), "{:?}", program.workspace.problems);
        assert_eq!(plan.problems, [], "no cycle is left unresolved");
        assert_eq!(plan.moves.len(), 1, "{:?}", plan.moves);
        let moved = &plan.moves[0];
        // `C` is declared in `def/a.lfy` and compiled in `def/b.lfy`'s unit.
        // @lfy def/cli/main.lfy:main
        assert_eq!(plan.units[moved.origin].stem, "a");
        assert_eq!(plan.units[moved.into].stem, "b");
        // The criterion of the moved entity is among the unit's own, since its review records
        // are what carries the verdict on it, and the ids are sorted.
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        let ids = requirement_ids_of(&program, &plan.units[moved.into]);
        assert!(ids.iter().any(|id| id.starts_with("C:C:")), "{ids:?}");
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "the ids of a unit are sorted");
    }

    /// `def/a.lfy` and `def/b.lfy` reach each other through members the traits of `def/t.lfy`
    /// added, and neither uses the other, so no move resolves the cycle: the problem is
    /// printed as an error of stage generation, nothing is generated, and the code is
    /// problems.
    // @lfy def/cli/main.lfy:main#main:main:055ed621ccb4cfc04dafc60c4ae42b146676fbac4139bd00ac26ddae87c1ad70
    // @lfy def/cli/main.lfy:main#main:main:2c23167a93d05731803b314b2b6545c5ee1481978f5c546aacb5c0b7f512115a
    #[test]
    fn a_cycle_no_move_resolves_is_an_error_and_generates_nothing() {
        let fixture = Fixture::new();
        one_target(&fixture)
            .write("def/a.lfy", "d A {\n  $x = string;\n}\n")
            .write("def/b.lfy", "d B {\n  $x = string;\n}\n")
            .write(
                "def/t.lfy",
                "use \"./a\";\nuse \"./b\";\n\ntrait toB {\n  $b = B;\n}\n\ntrait toA {\n  $a = A;\n}\ntoB.apply(A);\ntoA.apply(B);\n",
            );
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        // The program itself binds cleanly, so the code is problems for the cycle alone.
        // @lfy def/cli/main.lfy:main
        let diagnostics = query::diagnostics_of(&program.workspace, None);
        assert!(!diagnostics.iter().any(|d| d.severity == Severity::Error), "{diagnostics:?}");
        assert_eq!(plan.moves, []);
        assert!(!plan.problems.is_empty(), "the cycle leaves a problem");
        // Printed as an error of stage generation at its node. @lfy def/cli/main.lfy:main
        print_plan_problem(&program.workspace, &plan.problems[0], false, false);
        print_plan_problem(&program.workspace, &plan.problems[0], false, true);
        print_plan_problem(&program.workspace, &plan.problems[0], true, false);
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["check"]), ExitCode::Problems.code());
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        assert!(!fixture.root.join("out").exists(), "nothing is generated");
        // A dry run prints the plan and ends with the same code.
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--dry-run"]), ExitCode::Problems.code());
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

    /// Compile prints the diagnostics of the loaded program, a warning among them; verify
    /// prints the error ones alone. The unused use of def/b.lfy is the warning and the
    /// undeclared z the error, both in the one file.
    // @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
    // @lfy def/cli/main.lfy:main#main:main:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
    #[test]
    fn compile_prints_every_loaded_diagnostic_and_verify_only_the_errors() {
        let fixture = Fixture::new();
        fixture.write("def/b.lfy", "d B: `A B` {}\n").write("def/a.lfy", "use \"./b\";\n\nconst y = z;\n");
        let workspace = workspace::load(&fixture.root);
        let mine: Vec<Diagnostic> = loaded_diagnostics(&workspace).into_iter().filter(|d| d.range.file == "def/a.lfy").collect();
        assert!(mine.iter().any(|d| d.severity == Severity::Error), "{mine:?}");
        assert!(mine.iter().any(|d| d.severity == Severity::Warning), "{mine:?}");
        let errors: Vec<Diagnostic> = error_diagnostics(&workspace).into_iter().filter(|d| d.range.file == "def/a.lfy").collect();
        assert!(errors.iter().all(|d| d.severity == Severity::Error), "{errors:?}");
        assert!(errors.len() < mine.len(), "{errors:?} against {mine:?}");
        // Nothing is printed when loading found no error, whatever warnings it found.
        // @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
        let clean = Fixture::new();
        clean.write("def/b.lfy", "d B: `A B` {}\n").write("def/a.lfy", "use \"./b\";\n\nconst y = 1;\n");
        let loaded = workspace::load(&clean.root);
        assert!(loaded_diagnostics(&loaded).is_empty());
        assert!(query::diagnostics_of(&loaded, None).iter().any(|d| d.severity == Severity::Warning));
    }

    /// With no compiler named, the request of each batch is written to elfie-requests under
    /// the root, one file per batch named by its identifier, and the code is success. It is
    /// read by a compiler run by hand, in no hosted session, so it names none of the tools a
    /// session offers.
    // @lfy def/cli/main.lfy:main
    // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
    #[test]
    fn a_project_with_no_compiler_writes_the_requests() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        let instructions = fixture.read("elfie-requests/a.md");
        assert!(instructions.contains("An A"), "{instructions}");
        assert!(instructions.contains("def/a.lfy"), "{instructions}");
        // The request was made for no hosted session: it names neither the tools only a
        // hosted session offers nor the way one submits its outcome.
        // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
        assert!(!instructions.contains("elfie_task"), "{instructions}");
        // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
        assert!(!instructions.contains("elfie_submit"), "{instructions}");
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
        // Neither the compiler nor the verifier ran, so no agent server was hosted and no
        // session's config file was written.
        // @lfy def/cli/main.lfy:main#main:main:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
        assert!(!fixture.root.join("elfie-requests/a.compiler.mcp.json").exists());
        // @lfy def/cli/main.lfy:main#main:main:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
        assert!(!fixture.root.join("elfie-requests/a.verifier.mcp.json").exists());
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
                "#!/bin/sh\ncat > \"$ELFIE_ROOT/instructions.txt\"\nprintf '%s\\n' \"$ELFIE_BATCH\" \"$ELFIE_UNITS\" \"$ELFIE\" \"[$ELFIE_COMMAND_TOOLS]\" > \"$ELFIE_ROOT/environment.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:1\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho done\n",
            )
            .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
        // @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // ELFIE is the path of the running executable, so the agent server an agent starts
        // is this same program.
        // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        let environment = fixture.read("environment.txt");
        let mut lines = environment.lines();
        assert_eq!(lines.next(), Some("a"));
        assert_eq!(lines.next(), Some("a"));
        let elfie = std::env::current_exe().unwrap();
        assert_eq!(lines.next(), Some(elfie.to_string_lossy().as_ref()));
        // The target of this project has no command, so its session is offered no command
        // tool and ELFIE_COMMAND_TOOLS is empty.
        // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        assert_eq!(lines.next(), Some("[]"));
        let instructions = fixture.read("instructions.txt");
        assert!(instructions.contains("An A"), "{instructions}");
        // The request was made for the run's hosted session, so it names the tools only such
        // a session offers and how to submit the outcome through it.
        // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
        assert!(instructions.contains("elfie_task"), "{instructions}");
        // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
        assert!(instructions.contains("elfie_submit"), "{instructions}");
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
        assert!(log.contains("1 accepted, 0 rejected, 0 blocked, 0 failed"), "{log}");
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
            .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
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
            .write("elfie.json", &manifest(&[("compiler", "exit 127")]));
        // The command runs and produces nothing, so the outcome is rejected rather than
        // failed; a command that cannot be spawned at all is the failure.
        assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
        let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
        let run = Mutex::new(Run::new(program, plan, Vec::new(), reporter, &invocation));
        let batch = locked(&run).plan.batches[0].clone();
        let nowhere = Where::Root(PathBuf::from("/no/such/directory"));
        let runs = {
            let mut state = locked(&run);
            let agent = state.agent.clone();
            Runs::new(agent, state.running_of(&batch.identifier))
        };
        let planned = vec![a_planned_batch(&batch.identifier, 0, Vec::new())];
        let schedule = Mutex::new(Schedule::of(&planned));
        let finished = Condvar::new();
        let round = Round { schedule: &schedule, finished: &finished, planned: &planned, at: 0, copies: false };
        assert!(Run::compile_batch(&run, &batch, "true", &nowhere, &runs, None, &round).held.is_none());
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
        assert_eq!(counts_message(&counts_with_missing(&every, 0), false), "1 satisfied, 1 violated, 1 unverifiable");
        // An id the review was asked about that no review names counts as unverifiable.
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        assert_eq!(counts_message(&counts_with_missing(&every, 2), false), "1 satisfied, 1 violated, 3 unverifiable");
        // And is named as not reviewed on a line of its own, which is the line a compile
        // prints under its reviewed line and a verify under its counts line.
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        let unanswered = ["A:A:one".to_string(), "A:A:two".to_string()];
        assert_eq!(not_reviewed(&unanswered[0]), "A:A:one was not reviewed");
        let report = ReviewReport { missing: unanswered.to_vec(), ..ReviewReport::default() };
        assert_eq!(missing_lines(&report), "A:A:one was not reviewed\nA:A:two was not reviewed");
        assert_eq!(
            missing_lines(&report).lines().collect::<Vec<_>>(),
            unanswered.iter().map(|id| not_reviewed(id)).collect::<Vec<_>>()
        );
        // Nothing follows when the review answered for every id it was asked about.
        // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
        assert!(missing_lines(&ReviewReport::default()).is_empty());
    }

    /// A violated review rejects the batch: the problem is printed, the batch is run once
    /// more with it appended to the instructions, the verifier runs again, and a violated
    /// review then stops the batch as rejected.
    // @lfy def/cli/main.lfy:main#main:main:a614f50ad52233df09b271e8629dcbe550c75c8f9eb349f91208cb874d9b7f6d
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
        assert!(log.contains("0 accepted, 1 rejected"), "{log}");
        // Its source maps are not recorded, so its outputs stay on disk and the unit stays
        // planned. @lfy def/cli/main.lfy:main
        assert!(fixture.root.join("out/a.rs").exists(), "the outputs stay on disk");
        assert!(!fixture.root.join("elfie-compile/maps/rust/a.json").exists(), "nothing is recorded");
        let (_, plan) = plan_with(&fixture.root, &[], &[], &[]);
        assert_eq!(plan.units.len(), 1);
        assert!(plan.units[0].reason.is_some(), "the unit stays planned");
    }

    /// A verifier that answered for nothing it was asked about is run once more; its problems
    /// are then printed as a failed line and the batch is verified with the reviews parsed
    /// from that run as if its report had been complete.
    ///
    /// A run that answered for every id it was asked about stands as it is, however it ended:
    /// the verifier is not run again for a report that merely did not end.
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    // @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
    // @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
    #[test]
    fn a_verifier_whose_report_does_not_end_is_run_once_more() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, NOTHING_REVIEWED);
        // Nothing it was asked about was answered: the run is made again, and the second one
        // answers for nothing either.
        // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
        assert_eq!(lines_of(&fixture, "verifications.txt"), 2, "the verifier ran twice");
        assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran once");
        let log = fixture.read("elfie-requests/compile.log");
        // Its problems are printed as a failed progress line, and the batch is verified with
        // the reviews parsed from that run as if its report had been complete: nothing was
        // violated, so the batch is done and the id it left is counted as unverifiable.
        // @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
        assert!(log.contains("✗ failed"), "{log}");
        // @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
        assert!(log.contains("0 satisfied, 0 violated, 1 unverifiable"), "{log}");
        assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
        // A report that answered for every id it was asked about stands as it is, whatever
        // else it did: the verifier is run once, and what it wrote beside its reviews is
        // printed under its reviewed line.
        // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
        let stood = Fixture::new();
        a_compiler_and_a_verifier(&stood, UNENDED);
        assert_eq!(stood.run(&["compile"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
        assert_eq!(lines_of(&stood, "verifications.txt"), 1, "the verifier ran once");
        let log = stood.read("elfie-requests/compile.log");
        assert!(!log.contains("✗ failed"), "{log}");
        assert!(log.contains("1 satisfied, 0 violated, 0 unverifiable"), "{log}");
        // What it wrote beside its reviews follows its reviewed line.
        // @lfy def/cli/main.lfy:main#main:main:7ac5c7dbdba835816dd103d3fb573c0697886fc87ae20c77a226aaa3f7b90592
        assert!(log.contains("the report did not end"), "{log}");
        // The reviews of that run are written all the same. @lfy def/cli/main.lfy:main
        assert!(stood.read("elfie-requests/a.reviews.json").contains("covered by a test"));
    }

    /// A run that answered for every id it was asked about stands, whatever else it wrote:
    /// the verifier is not run again, and what it wrote beside its reviews is printed under
    /// its reviewed line.
    // @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
    // @lfy def/cli/main.lfy:main#main:main:7ac5c7dbdba835816dd103d3fb573c0697886fc87ae20c77a226aaa3f7b90592
    #[test]
    fn a_review_that_ended_stands_and_what_else_it_wrote_is_printed() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED_WITH_A_STRAY_OBJECT);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
        assert_eq!(lines_of(&fixture, "verifications.txt"), 1, "the verifier ran once");
        let log = fixture.read("elfie-requests/compile.log");
        assert!(log.contains("1 satisfied, 0 violated, 0 unverifiable"), "{log}");
        // What it wrote beside its reviews follows its reviewed line, indented.
        // @lfy def/cli/main.lfy:main#main:main:7ac5c7dbdba835816dd103d3fb573c0697886fc87ae20c77a226aaa3f7b90592
        assert!(log.contains("I also looked at the tests"), "{log}");
        // @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
        assert!(!log.contains("✗ failed"), "{log}");
    }

    /// Every line of the verifier's output reaches the log, the lines it wrote to standard
    /// error among them, each with the prefix it was shown with and no escape.
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    #[test]
    fn every_line_the_verifier_wrote_reaches_the_log() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED_WITH_NOISE);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        let log = fixture.read("elfie-requests/compile.log");
        // The line it wrote to standard error, prefixed by the batch as it was shown.
        // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
        assert!(log.contains("  a \u{2502} reading the outputs"), "{log}");
        // Its standard output, and the report it is read from, are there too.
        // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
        assert!(log.contains("  a \u{2502} ELFIE: REVIEWED"), "{log}");
        assert!(log.contains("--- the review of a (attempt 1) ---"), "{log}");
        // What the compiler wrote is logged the same way.
        // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
        assert!(log.contains("  a \u{2502} ELFIE: DONE"), "{log}");
        // Every line of it is the line as printed with colors off.
        // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
        assert!(!log.contains('\u{1b}'), "the log holds an escape");
    }

    /// With --no-verify no verifier runs, nothing is reviewed, no reviews file is written,
    /// and the batch is done and its source maps recorded when its units are accepted.
    // @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
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
        a_compiler_and_a_verifier(&plain, VIOLATED).write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
        assert_eq!(plain.run(&["compile"]), ExitCode::Success.code());
        assert!(!plain.root.join("verifications.txt").exists());
        assert!(!plain.root.join("elfie-requests/a.reviews.json").exists());
    }

    /// Verify runs the verifier on the outputs already recorded and compiles nothing.
    // @lfy def/cli/main.lfy:main#main:main:37d72b3d77ba44131f58978f7f86e78a53b525f1f2ea28140bd3adeb3c4d2343
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
        // The verdict the compile recorded is forgotten, so the review asks about the
        // criterion again rather than carrying it.
        // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
        forget_reviews(&fixture);
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
    // @lfy def/cli/main.lfy:main#main:main:8127972dd45dbd924545668a1b1bcb7a18efd3820ce24def88c57212aa87d8ab
    #[test]
    fn verify_returns_problems_for_a_violated_review() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // The output for a is recorded in elfie-compile/maps/rust/a.json.
        // @lfy def/cli/main.lfy:main
        assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
        let _ = fs::remove_file(fixture.root.join("elfie-requests/a.reviews.json"));
        // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
        forget_reviews(&fixture);
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
        let report = generation::review_of(&line, &program, &[], &[]);
        let column = width(&ReviewStatus::Unverifiable.to_string());
        assert_eq!(
            painted_review(&report.reviews[0], false),
            format!("{} def/a.lfy:3 A: returns 0 for an empty list", padded("violated", column))
        );
        // @lfy def/cli/main.lfy:main
        assert_eq!(
            counts_message(&counts_with_missing(&report.reviews, 0), false),
            "0 satisfied, 1 violated, 0 unverifiable"
        );
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
        fixture.write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
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
        assert_eq!(value["requirements"], GlobalReview::requirements_of(&program));
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
    // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
    #[test]
    fn no_global_criterion_means_no_global_review() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
        assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
        let log = fixture.read("elfie-requests/compile.log");
        assert!(!log.contains("globalVerifying"), "{log}");
        // The verifier ran once, for the batch, and never for a global review.
        assert_eq!(lines_of(&fixture, "verifications.txt"), 1);
        // verify with no stem reviews the batch and asks for no global review either, because
        // there is nothing global to review. With --full the batch is reviewed again rather
        // than carrying the verdict the compile recorded.
        // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
        // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
        assert_eq!(fixture.run(&["verify", "--full"]), ExitCode::Success.code());
        assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
        assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
        // verify --global with nothing global to review runs no verifier at all.
        // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
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
        // The batch was handled twice and the global review ran after each round. The second
        // round compiled the unit again and its output was the same, so the verdict on its
        // own criterion was carried and no verifier ran on the batch.
        // @lfy def/cli/main.lfy:main
        // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
        assert_eq!(fixture.read("verifications.txt"), "a\nglobal\nglobal\n");
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

    /// A unit whose maps from source-map.json could not be recorded leaves the file in place
    /// rather than removed, holding every map it held, so nothing of it is lost.
    // @lfy def/cli/main.lfy:main#main:main:b9ad771de6ec1ee6e2812d5335aef919fe9cd8228cb6b2fea9731de46c6dd24e
    #[test]
    fn maps_that_could_not_be_recorded_leave_the_file_in_place() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        fixture.write("def/b.lfy", "d B {}\n");
        fixture.write("compiler.sh", PER_UNIT_COMPILER);
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        // The project as an earlier version left it: every map of the target in one
        // source-map.json under its output directory and no folder of map files.
        let folder = fixture.root.join("elfie-compile/maps/rust");
        let mut held = generation::read_source_maps(&folder.join("a.json"));
        held.extend(generation::read_source_maps(&folder.join("b.json")));
        assert_eq!(held.len(), 2);
        let value = serde_json::Value::Array(held.iter().map(SourceMap::to_json).collect());
        let text = serde_json::to_string_pretty(&value).unwrap();
        fixture.write("out/source-map.json", &text);
        fs::remove_dir_all(fixture.root.join("elfie-compile/maps")).unwrap();
        // No unit's maps can be recorded: a file stands where the target's folder of map
        // files goes, so no map file under it can be written.
        // @lfy def/cli/main.lfy:main#main:main:b9ad771de6ec1ee6e2812d5335aef919fe9cd8228cb6b2fea9731de46c6dd24e
        fixture.write("elfie-compile/maps/rust", "not a folder\n");
        let legacy = legacy_targets(&workspace::load(&fixture.root), None);
        assert_eq!(legacy, vec!["out".to_string()]);
        let (program, plan) = planned(&fixture.root, &[], &[], false, &BTreeSet::new());
        move_legacy_maps(&program.workspace, &plan, &legacy);
        // The file is left in place rather than removed, holding the maps it held.
        // @lfy def/cli/main.lfy:main#main:main:b9ad771de6ec1ee6e2812d5335aef919fe9cd8228cb6b2fea9731de46c6dd24e
        assert!(fixture.root.join("out/source-map.json").is_file(), "the file was removed");
        assert_eq!(fixture.read("out/source-map.json"), text, "the maps it holds were lost");
        // With the folder writable again, every unit's maps are recorded and the file goes.
        // @lfy def/cli/main.lfy:main#main:main:67eaaa8a1841a575fb7441eb3ad67ea695d923595769380ebe74e710e6251abf
        fs::remove_file(fixture.root.join("elfie-compile/maps/rust")).unwrap();
        let legacy = legacy_targets(&workspace::load(&fixture.root), None);
        let (program, plan) = planned(&fixture.root, &[], &[], false, &BTreeSet::new());
        move_legacy_maps(&program.workspace, &plan, &legacy);
        assert!(folder.join("a.json").is_file(), "a's maps were not recorded");
        assert!(folder.join("b.json").is_file(), "b's maps were not recorded");
        assert!(!fixture.root.join("out/source-map.json").exists(), "the file stayed");
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
    // @lfy def/cli/main.lfy:main#main:main:bc914a1f1aeef4f635de558b0f64bd7174c6f9cab74213ce228b002ba9b8dd46
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

    /// The previous text a request is given for a unit is the source its outputs were
    /// generated from as git holds it, never lowered, while the revision is recognized by the
    /// hash of its lowered text.
    // @lfy def/cli/main.lfy:main
    #[test]
    fn the_previous_source_is_the_revision_git_still_holds() {
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
        // The committed revision as git holds it, and its lowered text.
        // @lfy def/cli/main.lfy:main
        let workspace = workspace::load(&fixture.root);
        let raw = fixture.read("def/a.lfy");
        let (lowered, requirements) = lowered_of(&workspace, "def/a.lfy", &raw).unwrap();
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
        // And by the ids of its criteria and tests, which the map holds beside the hash: a
        // revision that lowers to the hash with other ids is only the hash-matched source.
        // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
        assert_eq!(requirements, unit.outputs[0].requirements);
        // What comes back is that revision as git holds it, never lowered, and it is the
        // source at the last accepted generation rather than only the hash-matched one.
        // @lfy def/cli/main.lfy:main#main:main:1f3c991d456ca136f454724f3f7fd0dfcb94081ad9e4e7f5f238375155b93e8f
        // @lfy def/cli/main.lfy:main#main:main:44d118a88d1e7a297fd7918629fa1c6c140077672079734436b5b02679cb1207
        let found = previous_source(&program.workspace, unit).unwrap();
        assert!(!found.hash_matched);
        let previous = found.text;
        assert_eq!(previous, raw);
        // Which the request lowers itself, once, to set beside the unit's own lowered text;
        // nothing is hash-matched, so the request is given no path among them.
        // @lfy def/cli/main.lfy:main#main:main:1f3c991d456ca136f454724f3f7fd0dfcb94081ad9e4e7f5f238375155b93e8f
        // @lfy def/cli/main.lfy:main#main:main:44d118a88d1e7a297fd7918629fa1c6c140077672079734436b5b02679cb1207
        let handed = BTreeMap::from([("def/a.lfy".to_string(), previous)]);
        let request = generation::request(&program, &plan, &plan.batches[0], &[], &handed, true, &[]);
        assert_eq!(request.previous["def/a.lfy"], lowered);
        assert!(request.sources["def/a.lfy"].contains("d A"), "{:?}", request.sources);
    }

    /// A revision that lowers to the hash a unit's map holds but answers for other ids is the
    /// unit's hash-matched source rather than the source at its last accepted generation: it
    /// comes back marked as such, and a unit whose previous text is only that is never one
    /// whose markers are the whole of the work.
    // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
    // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
    // @lfy def/cli/main.lfy:main#main:main:44d118a88d1e7a297fd7918629fa1c6c140077672079734436b5b02679cb1207
    // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
    #[test]
    fn a_revision_that_only_reworded_a_criterion_is_the_hash_matched_source() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        if !git(&fixture, &["init", "-q"]) {
            return; // No git here: nothing is recoverable, and nothing to check.
        }
        git(&fixture, &["config", "user.email", "p@example.com"]);
        git(&fixture, &["config", "user.name", "p"]);
        git(&fixture, &["add", "def/a.lfy"]);
        git(&fixture, &["commit", "-q", "-m", "a"]);
        let committed = fixture.read("def/a.lfy");
        // The criterion is reworded: lowering drops it, so the lowered code is untouched and
        // only the id changes. @lfy def/cli/main.lfy:main
        fixture.write(
            "def/a.lfy",
            "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts every item of the list` });\n}\n",
        );
        // The outputs are accepted from the reworded file, so the map holds the ids it has
        // now, which the committed revision does not. @lfy def/cli/main.lfy:main
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        let workspace = workspace::load(&fixture.root);
        let maps = generation::source_maps_of(&workspace, None);
        drop(workspace);
        let (program, plan) = planned(&fixture.root, &maps, &["a".to_string()], false, &BTreeSet::new());
        let unit = plan.units.iter().find(|unit| unit.stem == "a").unwrap();
        // The committed revision lowers to the map's hash and answers for other ids, so it is
        // the hash-matched source and not the source at the last accepted generation.
        // @lfy def/cli/main.lfy:main#main:main:df75e3de39b96735b913f4c3713135b546bfbcaac44bc8c3f0fb86c0e391fcc6
        // @lfy def/cli/main.lfy:main#main:main:f39bd9ab2495264b9736277dccebe0c77551b0d471884faae0d41398da0096fa
        let found = previous_source(&program.workspace, unit).unwrap();
        assert!(found.hash_matched);
        assert_eq!(found.text, committed);
        // Given that path among the hash-matched, the request names no unit whose markers are
        // the whole of the work.
        // @lfy def/cli/main.lfy:main#main:main:44d118a88d1e7a297fd7918629fa1c6c140077672079734436b5b02679cb1207
        // @lfy def/cli/main.lfy:main#main:main:413b71fcbd78964e80ed9718e6ab5d99b92285cc9fd61235e6a0a45a22467c55
        let handed = BTreeMap::from([("def/a.lfy".to_string(), found.text)]);
        let hash_matched = vec!["def/a.lfy".to_string()];
        let request =
            generation::request(&program, &plan, &plan.batches[0], &[], &handed, true, &hash_matched);
        assert!(request.reworded_only.is_empty(), "{:?}", request.reworded_only);
    }

    /// The own region text of an output is the lines of the regions this unit's own markers
    /// begin, without the marker comments, and nothing another unit wrote in the same file.
    // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
    #[test]
    fn the_own_region_text_of_a_shared_output_leaves_another_unit_out() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n").write("def/b.lfy", "d B: `A B` {}\n");
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        let index = plan.units.iter().position(|unit| unit.stem == "a").unwrap();
        let text = "// @lfy def/a.lfy:A\npub struct A {}\n// @lfy def/b.lfy:B\npub struct B {}\n";
        // @lfy def/cli/main.lfy:main#main:main:fbfbb673e8a8caec004774ec10f9f8eaab81df8006f5fd3efe0403deae5390ea
        let own = own_region_text(&program.workspace, &plan, index, text);
        assert_eq!(own, "pub struct A {}\n");
    }

    /// A project of two units in two directories, `def/b/one` and `def/a/one` which uses it,
    /// whose compiler writes both their regions into one shared output: the run for `b/one`
    /// writes its own region alone, and the run for `a/one` writes its own above it, so what
    /// `b/one` was accepted with is no longer where its own accept saw it.
    // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
    fn a_shared_output(fixture: &Fixture) -> &Fixture {
        let compiler = "#!/bin/sh\ncat > /dev/null\nmkdir -p \"$ELFIE_ROOT/out\"\n\
             case \"$ELFIE_UNITS\" in\n\
             *b/one*)\n\
             echo ran >> \"$ELFIE_ROOT/ran-b.txt\"\n\
             printf '// @lfy def/b/one.lfy:B\\npub struct B {}\\n' > \"$ELFIE_ROOT/out/shared.rs\"\n\
             ;;\n\
             *a/one*)\n\
             echo ran >> \"$ELFIE_ROOT/ran-a.txt\"\n\
             printf '// @lfy def/a/one.lfy:A\\npub struct A {}\\n// @lfy def/b/one.lfy:B\\npub struct B {}\\n' \
             > \"$ELFIE_ROOT/out/shared.rs\"\n\
             ;;\n\
             esac\n\
             echo 'ELFIE: DONE'\n";
        library(fixture)
            .write("def/roles.lfy", ROLES)
            .write("def/targets.lfy", TARGETS)
            .write("def/b/one.lfy", "d B: `A B` {}\n")
            .write("def/a/one.lfy", "use \"../b/one\";\n\nd A: `An A` {}\n")
            .write("compiler.sh", compiler)
            .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]))
    }

    /// A unit that shares an output with a batch is one of its co-owners: once the batch's
    /// source maps are recorded, the co-owner's are re-derived from the file as it is now and
    /// recorded for it, so its regions are where the batch's write left them; no compiler runs
    /// for the co-owner and nothing but its map file is written for it.
    // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
    // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
    // @lfy def/cli/main.lfy:main#main:main:14b01e32487ce04075ecbde039b065c7ad9311e9e5e8cb3e8c5117b41e849f61
    #[test]
    fn a_co_owner_of_a_shared_output_has_its_maps_re_derived() {
        let fixture = Fixture::new();
        a_shared_output(&fixture);
        // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
        assert_eq!(fixture.run(&["compile", "--jobs", "1", "--no-verify"]), ExitCode::Success.code());
        // The run for a/one put its own region above the one b/one was accepted with.
        // @lfy def/cli/main.lfy:main#main:main:7fe3a9efd4f314763aa0579759cefcffe953e3223a30cc3aa3e139a03b7497cf
        let shared = fixture.read("out/shared.rs");
        assert!(shared.starts_with("// @lfy def/a/one.lfy:A"), "{shared}");
        // b/one's map file names the marker where the file holds it now, the third line, and
        // not the first, where its own accept saw it.
        // @lfy def/cli/main.lfy:main#main:main:d6db27f2a0e3fd12b8c822bb2aabe61df5e5fa5cd3bb142095a0b7111b814dd1
        let maps = maps_of(&fixture, "elfie-compile/maps/rust/b/one.json");
        assert_eq!(maps[0]["output"], "out/shared.rs", "{maps}");
        let its = maps[0]["markers"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|marker| marker["file"] == "def/b/one.lfy")
            .unwrap_or_else(|| panic!("{maps}"));
        assert_eq!(its["outputLine"], 3, "{maps}");
        // No compiler ran for the co-owner beyond its own batch's one run.
        // @lfy def/cli/main.lfy:main#main:main:14b01e32487ce04075ecbde039b065c7ad9311e9e5e8cb3e8c5117b41e849f61
        assert_eq!(fixture.read("ran-b.txt").lines().count(), 1);
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
    // @lfy def/cli/main.lfy:parse#parse:parse:252d8235d06b32c568f2bf1a13157b65c1788281bf471f3abe705c298312086c
    // @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
    #[test]
    fn jobs_takes_a_whole_number_of_at_least_one() {
        // --jobs followed by a value gives that value, as --target does.
        // @lfy def/cli/main.lfy:parse#parse:parse:252d8235d06b32c568f2bf1a13157b65c1788281bf471f3abe705c298312086c
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

    /// Whether a text is a version 4 UUID spelled as 36 lowercase characters in the 8-4-4-4-12
    /// form, with the version in the thirteenth character and the variant in the seventeenth.
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    fn is_a_uuid(text: &str) -> bool {
        let groups: Vec<&str> = text.split('-').collect();
        groups.len() == 5
            && groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
            && text.chars().all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            && groups[2].starts_with('4')
            && "89ab".contains(groups[3].chars().next().unwrap_or('z'))
    }

    /// One batch of a round, named and waiting for the places given.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    fn a_planned_batch(identifier: &str, unit: usize, after: Vec<usize>) -> Planned {
        Planned { batch: Batch { units: vec![unit], identifier: identifier.to_string() }, after }
    }

    /// A batch starts once every batch holding a dependency of one of its units is done, at
    /// most --jobs run at once, and among those that can start they start in plan order.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    #[test]
    fn a_batch_starts_once_what_it_waits_for_is_done() {
        let planned =
            vec![a_planned_batch("a", 0, vec![]), a_planned_batch("b", 1, vec![]), a_planned_batch("c", 2, vec![0])];
        let mut schedule = Schedule::of(&planned);
        // Among those that can start, they start in plan order.
        // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(0)));
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(1)));
        // At most --jobs run at once.
        // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Wait));
        // c waits on a, which is still running.
        // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
        schedule.finish(1, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Wait));
        schedule.finish(0, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(2)));
        schedule.finish(2, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Over));
    }

    /// The compile stopped: no batch starts after that, and the batches already running
    /// finish as they would.
    // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
    #[test]
    fn a_stopped_compile_starts_no_batch() {
        let planned = vec![a_planned_batch("a", 0, vec![]), a_planned_batch("b", 1, vec![])];
        let mut schedule = Schedule::of(&planned);
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(0)));
        // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
        assert!(matches!(schedule.next(&planned, 2, true, false), Next::Wait));
        schedule.finish(0, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, true, false), Next::Over));
        // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
        assert_eq!(schedule.unstarted(), vec![1]);
    }

    /// A batch whose file could not be merged runs again from a new copy once no other batch
    /// is running, and nothing else starts while it does.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    #[test]
    fn a_conflicted_batch_runs_again_with_nothing_else_running() {
        let planned = vec![a_planned_batch("a", 0, vec![]), a_planned_batch("b", 1, vec![])];
        let mut schedule = Schedule::of(&planned);
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(0)));
        // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
        schedule.finish(0, &Done::Conflicted);
        // It is waiting again, in plan order, and runs with nothing else.
        // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(0)));
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Wait));
        schedule.finish(0, &Done::Merged);
        assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(1)));
    }

    /// With --jobs 1, or outside a git repository, batches run one at a time and the batch's
    /// directory is the root itself.
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    #[test]
    fn one_job_or_no_repository_runs_in_the_root() {
        let fixture = Fixture::new();
        let four = parse_arguments(&args(&["compile", "--jobs", "4"])).unwrap();
        // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
        assert_eq!(Where::of(&fixture.root, "a", false).directory(), fixture.root);
        // Outside a git repository --jobs does not hold: one batch runs at a time, in the
        // root itself, so that no two compilers share a tree.
        // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
        if git_directory(&fixture.root).is_none() {
            // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
            assert_eq!(compile_jobs(&four, &fixture.root), 1);
            // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
            assert_eq!(Where::of(&fixture.root, "a", false).directory(), fixture.root);
            // One at a time: with one running, nothing else starts.
            // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
            let planned =
                vec![a_planned_batch("a", 0, Vec::new()), a_planned_batch("b", 1, Vec::new())];
            let mut schedule = Schedule::of(&planned);
            assert!(matches!(schedule.next(&planned, 1, false, false), Next::Start(0)));
            assert!(matches!(schedule.next(&planned, 1, false, false), Next::Wait));
        }
        // In a git repository a batch has its own copy, so --jobs stands as it is given.
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        if a_repository(&fixture) {
            // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
            assert_eq!(compile_jobs(&four, &fixture.root), 4);
        }
    }

    /// A fixture root made a git repository, so that a batch gets its own copy of it; false
    /// when there is no git to make one with.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    fn a_repository(fixture: &Fixture) -> bool {
        git(fixture, &["init", "-q"])
            && git(fixture, &["config", "user.email", "p@example.com"])
            && git(fixture, &["config", "user.name", "p"])
    }

    /// Whether a file may be run; false everywhere on a platform with no such mode, so that
    /// comparing two files still holds there.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    fn executable(path: &Path) -> bool {
        mode_of(path).is_some_and(|mode| mode & 0o111 != 0)
    }

    /// Every file git lists in the root is copied byte for byte, and every file of the copy
    /// that differs from the base is merged back: replaced where the root still holds what
    /// the base holds, and three-way merged where another batch changed it.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
    // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
    #[test]
    fn a_copy_of_the_root_merges_back_into_it() {
        let fixture = Fixture::new();
        fixture
            .write("kept.txt", "one\n")
            .write("gone.txt", "away\n")
            .write("shared.txt", "a\nb\nc\n")
            .write("run.sh", "one\n")
            // A directory a walk of the copy would skip, holding a file that belongs to the
            // project, so that a merge reading it as removed shows here.
            // @lfy def/cli/main.lfy:main#main:main:83b890265a80eabfeb07845b755aca4a5a8ce09a8e540c54a21997798a0c554a
            .write("lib/target/rust.lfy", "kept\n")
            .write("elfie-requests/compile.log", "noise\n");
        // The compiler is often a script, which the copy must leave runnable.
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        set_mode(&fixture.root.join("run.sh"), Some(0o755));
        if !a_repository(&fixture) {
            return;
        }
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        let mirror = Mirror::take(&fixture.root, "x/y").expect("a copy of the root");
        // Every file git lists is there byte for byte, under a name a file may have.
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        assert_eq!(fs::read_to_string(mirror.directory.join("kept.txt")).unwrap(), "one\n");
        assert!(mirror.directory.ends_with("x-y"), "{:?}", mirror.directory);
        // Nothing under elfie-requests is copied.
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        assert!(!mirror.directory.join("elfie-requests/compile.log").exists());
        // The copy keeps whether a file is executable.
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        assert_eq!(executable(&mirror.directory.join("run.sh")), executable(&fixture.root.join("run.sh")));
        // The copy holds a .git file reading gitdir: and the absolute path of the root's git
        // directory, so git run in it reads the root's history.
        // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
        let pointed = fs::read_to_string(mirror.directory.join(".git")).expect("a .git file");
        let at = git_directory(&fixture.root).expect("the root's git directory");
        // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
        assert_eq!(pointed, format!("gitdir: {}\n", at.to_string_lossy()));
        assert!(at.is_absolute(), "{at:?}");
        // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
        assert_eq!(git_directory(&mirror.directory), Some(at), "git in the copy reads the root");
        // The batch changes one file, adds one, and removes one.
        fs::write(mirror.directory.join("kept.txt"), "two\n").unwrap();
        fs::write(mirror.directory.join("added.txt"), "new\n").unwrap();
        fs::write(mirror.directory.join("shared.txt"), "a\nb\nC\n").unwrap();
        fs::write(mirror.directory.join("run.sh"), "two\n").unwrap();
        set_mode(&mirror.directory.join("run.sh"), Some(0o755));
        fs::remove_file(mirror.directory.join("gone.txt")).unwrap();
        // Another batch changed shared.txt in the root meanwhile, elsewhere in the file.
        fixture.write("shared.txt", "A\nb\nc\n");
        let merged = mirror.merge(&fixture.root).expect("every file merges");
        // The root still held what the base holds: it is replaced by the copy's, or removed.
        // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
        assert_eq!(fixture.read("kept.txt"), "two\n");
        assert_eq!(fixture.read("added.txt"), "new\n");
        assert!(!fixture.root.join("gone.txt").exists());
        // What is merged keeps whether the copy's file is executable.
        // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
        assert_eq!(fixture.read("run.sh"), "two\n");
        assert_eq!(executable(&fixture.root.join("run.sh")), executable(&mirror.directory.join("run.sh")));
        // Another batch changed it: it is the three-way merge against the base.
        // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
        assert_eq!(fixture.read("shared.txt"), "A\nb\nC\n");
        for path in ["added.txt", "gone.txt", "kept.txt", "shared.txt"] {
            assert!(merged.contains(&path.to_string()), "{path} is not in {merged:?}");
        }
        // A file the batch left alone is left alone, wherever it sits: nothing is taken as
        // removed because of the name of the directory holding it.
        // @lfy def/cli/main.lfy:main#main:main:83b890265a80eabfeb07845b755aca4a5a8ce09a8e540c54a21997798a0c554a
        assert_eq!(fixture.read("lib/target/rust.lfy"), "kept\n");
        // @lfy def/cli/main.lfy:main#main:main:83b890265a80eabfeb07845b755aca4a5a8ce09a8e540c54a21997798a0c554a
        assert!(!merged.contains(&"lib/target/rust.lfy".to_string()), "{merged:?}");
        // The copy's own .git file is the copy's: it never reaches the root.
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        assert!(!merged.contains(&".git".to_string()), "{merged:?}");
        assert!(fixture.root.join(".git").is_dir(), "the root's own repository stands");
        // The copy goes once its work has reached the root.
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        mirror.remove();
        assert!(!mirror.directory.exists());
    }

    /// A file that cannot be merged without a conflict merges nothing of the batch.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
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
        // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
        assert_eq!(mirror.merge(&fixture.root).unwrap_err(), "shared.txt");
        // Nothing of the batch is merged, the file that did merge cleanly included.
        // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
        assert_eq!(fixture.read("other.txt"), "one\n");
        assert_eq!(fixture.read("shared.txt"), "theirs\n");
    }

    /// A batch running in a copy of the root runs the compiler and the verifier there, with
    /// git reading the root's history and nothing of git in their environment, and its work
    /// reaches the root once it is done.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
    #[test]
    fn a_batch_running_in_a_copy_merges_its_outputs_into_the_root() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        // The compiler writes its output, the git variables of its environment, and where git
        // run in the copy says the repository is. A repository of its own, made elsewhere,
        // answers for itself: nothing of the copy's reaches it.
        // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
        fixture.write(
            "compiler.sh",
            "#!/bin/sh\ncat > /dev/null\nown=\"$ELFIE_ROOT/../own\"\nprintf '%s\\n' \"$GIT_DIR\" \"$GIT_WORK_TREE\" \"$ELFIE_ROOT\" > \"$ELFIE_ROOT/where.txt\"\ngit rev-parse --absolute-git-dir >> \"$ELFIE_ROOT/where.txt\"\nmkdir -p \"$own\"\ngit init -q \"$own\"\n(cd \"$own\" && git rev-parse --absolute-git-dir) >> \"$ELFIE_ROOT/where.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:A\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho 'ELFIE: DONE'\n",
        );
        if !a_repository(&fixture) {
            return;
        }
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        assert_eq!(fixture.run(&["compile", "--jobs", "2"]), ExitCode::Success.code());
        // Everything the batch wrote in its copy reached the root.
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        assert!(fixture.read("out/a.rs").contains("pub struct A"), "the output is in the root");
        // No git variable is in the environment; ELFIE_ROOT is the copy; git run in the copy
        // reads the root's git directory; and a repository the compiler made of its own is
        // its own.
        // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
        let text = fixture.read("where.txt");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "", "{text}");
        assert_eq!(lines[1], "", "{text}");
        assert!(lines[2].contains("elfie-compile/cache/batches/a"), "{text}");
        // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
        assert_eq!(lines[3], git_directory(&fixture.root).unwrap().to_string_lossy(), "{text}");
        // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
        assert!(lines[4].ends_with("/own/.git"), "{text}");
        // The source maps are recorded in the root and the copy is removed.
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("out/a.rs"));
        assert!(!fixture.root.join("elfie-compile/cache/batches/a").exists(), "the copy is gone");
    }

    // ---- what stands in for the compiler -------------------------------------------

    /// The roles of a target carrying one command line per operation named.
    // @lfy def/cli/main.lfy:main#main:main:d39e66910a4061a95e4837b4d3f7e909ac681b4efc8559b6c018006a3d93cf19
    fn roles_with(commands: &str) -> String {
        format!(
            "trait lang extends targetLanguage: `Rust` {{\n  .fileExtension = \"rs\";\n  \
             .markerComment = \"//\";\n}}\n\
             trait flat extends targetLayout: `One file per unit` {{ }}\n\
             trait cargo extends targetEcosystem: `Crates from crates.io` {{\n  \
             .ecosystem = \"cargo\";\n  @commands{commands};\n}}\n"
        )
    }

    /// A build command line and a test command line that both exit 0.
    // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
    const PASSES: &str = "\n    .add({ operation = Operation.build, line = \"true\" })\
                          \n    .add({ operation = Operation.test, line = \"true\" })";

    /// A compiler writing the output of `def/b.lfy` and of `def/a.lfy`, the same way every
    /// run, and noting that it ran.
    // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
    const TWO_OUTPUTS: &str = "#!/bin/sh\ncat > /dev/null\n\
                               echo \"$ELFIE_BATCH\" >> \"$ELFIE_ROOT/ran.txt\"\n\
                               mkdir -p \"$ELFIE_ROOT/out\"\n\
                               printf '// @lfy def/b.lfy:B\\npub struct B {}\\n' > \"$ELFIE_ROOT/out/b.rs\"\n\
                               printf '// @lfy def/a.lfy:A\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\n\
                               echo 'ELFIE: DONE'\n";

    /// A command that notes that it ran and then fails, so that a run of it shows.
    // @lfy def/cli/main.lfy:main#main:main:c9e9379aee6642c4c6fd644592fb507b9275e32d2a6daa8928f697b3e30a5b06
    const RAN_AND_FAILED: &str =
        "#!/bin/sh\ncat > /dev/null\necho ran >> \"$ELFIE_ROOT/ran.txt\"\nexit 1\n";

    /// The maps one map file under the root holds, as JSON.
    // @lfy def/cli/main.lfy:main#main:main:4fb6763553d728eaca1fb8dff4b3ab5c3a0025db630a10cf64ccb8405b8f2e67
    fn maps_of(fixture: &Fixture, path: &str) -> serde_json::Value {
        serde_json::from_str(&fixture.read(path)).expect("a map file of JSON")
    }

    /// The hash a unit's map file records for one dependency of it.
    // @lfy def/cli/main.lfy:main#main:main:4fb6763553d728eaca1fb8dff4b3ab5c3a0025db630a10cf64ccb8405b8f2e67
    fn dependency_hash(fixture: &Fixture, path: &str, source: &str) -> String {
        maps_of(fixture, path)[0]["dependencies"][source].as_str().unwrap_or_default().to_string()
    }

    /// The signature one unit's recorded maps give its interface now.
    // @lfy def/cli/main.lfy:main#main:main:4fb6763553d728eaca1fb8dff4b3ab5c3a0025db630a10cf64ccb8405b8f2e67
    fn signature_of(fixture: &Fixture, source: &str) -> String {
        let maps = generation::source_maps_of(&workspace::load(&fixture.root), None);
        maps.iter()
            .find(|map| map.source == source)
            .map(|map| map.signature.clone())
            .unwrap_or_else(|| panic!("{source} has no recorded map"))
    }

    /// The log of a compile forgotten, so that the next one reads on its own.
    // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
    fn forget_log(fixture: &Fixture) {
        let _ = fs::remove_file(fixture.root.join("elfie-requests/compile.log"));
    }

    /// A batch whose every unit has the reason dependency has the target's build and test
    /// commands run in place of the compiler: both succeed, every unit is accepted on them,
    /// no verifier runs, the maps record the dependency's interface hash as it is now, and no
    /// review record file changes.
    // @lfy def/cli/main.lfy:main#main:main:4fb6763553d728eaca1fb8dff4b3ab5c3a0025db630a10cf64ccb8405b8f2e67
    #[test]
    fn a_dependency_only_batch_is_accepted_on_its_build_and_test() {
        let fixture = Fixture::new();
        library(&fixture)
            .write("def/roles.lfy", &roles_with(PASSES))
            .write("def/targets.lfy", TARGETS)
            .write("def/b.lfy", "d B: `A B` {}\n")
            .write(
                "def/a.lfy",
                "use \"./b\";\n\nd A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\n",
            )
            .write("compiler.sh", TWO_OUTPUTS);
        write_verifier(&fixture, SATISFIED);
        fixture.write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]));
        // Both units are recorded first, with a's review record beside them.
        // @lfy def/cli/main.lfy:main#main:main:4fb6763553d728eaca1fb8dff4b3ab5c3a0025db630a10cf64ccb8405b8f2e67
        assert_eq!(fixture.run(&["compile", "--jobs", "1"]), ExitCode::Success.code());
        let records = fixture.read("elfie-compile/reviews/rust/a.json");
        let signature = signature_of(&fixture, "def/b.lfy");
        // The hash a's map file records for b is made stale, so that a alone is planned and
        // its only reason is the dependency.
        // @lfy def/cli/main.lfy:main#main:main:4fb6763553d728eaca1fb8dff4b3ab5c3a0025db630a10cf64ccb8405b8f2e67
        let mut maps = maps_of(&fixture, "elfie-compile/maps/rust/a.json");
        maps[0]["dependencies"]["def/b.lfy"] = serde_json::Value::String("0".repeat(64));
        fixture.write("elfie-compile/maps/rust/a.json", &format!("{maps}\n"));
        assert_ne!(dependency_hash(&fixture, "elfie-compile/maps/rust/a.json", "def/b.lfy"), signature);
        // A compiler and a verifier that would each fail the test if they ran.
        // @lfy def/cli/main.lfy:main#main:main:c9e9379aee6642c4c6fd644592fb507b9275e32d2a6daa8928f697b3e30a5b06
        fixture.write("compiler.sh", RAN_AND_FAILED).write("verifier.sh", RAN_AND_FAILED);
        forget_log(&fixture);
        let _ = fs::remove_file(fixture.root.join("ran.txt"));
        // @lfy def/cli/main.lfy:main#main:main:4fb6763553d728eaca1fb8dff4b3ab5c3a0025db630a10cf64ccb8405b8f2e67
        assert_eq!(fixture.run(&["compile", "--jobs", "1"]), ExitCode::Success.code());
        // Neither the compiler nor the verifier ran.
        // @lfy def/cli/main.lfy:main#main:main:c9e9379aee6642c4c6fd644592fb507b9275e32d2a6daa8928f697b3e30a5b06
        // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
        assert!(!fixture.root.join("ran.txt").exists(), "the compiler or the verifier ran");
        // A building line for a, and then an accepted line.
        // @lfy def/cli/main.lfy:main#main:main:93eadeb6d21ac51de7347a9d6ec988a27d7b6f29d9c16646f22351ad0b537a45
        let log = fixture.read("elfie-requests/compile.log");
        assert!(log.contains("\u{25b8} building"), "{log}");
        assert!(log.lines().any(|line| line.contains("\u{2713} accepted")), "{log}");
        // The maps record b's interface hash as it is now.
        // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
        assert_eq!(dependency_hash(&fixture, "elfie-compile/maps/rust/a.json", "def/b.lfy"), signature);
        // No review record file of the batch's units changes.
        // @lfy def/cli/main.lfy:main#main:main:b82042daa1550092c2eeae599eba3e1056e1c9987779953b6b11500318102df7
        assert_eq!(fixture.read("elfie-compile/reviews/rust/a.json"), records);
    }

    /// The id of the first criterion of the definition file whose path ends in the suffix.
    // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
    fn criterion_in(fixture: &Fixture, suffix: &str) -> String {
        let (program, _) = plan_with(&fixture.root, &[], &[], &[]);
        let mut found = Vec::new();
        for lowered in &program.files {
            if program.workspace.files[lowered.file].path.ends_with(suffix) {
                ids_of(&lowered.root, &mut found);
            }
        }
        found.first().cloned().unwrap_or_else(|| panic!("{suffix} has no criterion"))
    }

    /// A project of two units in two directories, so that they are two batches: `def/b/one`,
    /// which carries one criterion, and `def/a/one`, which uses it and carries none.
    // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
    fn two_batches<'f>(fixture: &'f Fixture, note: &str) -> &'f Fixture {
        library(fixture)
            .write("def/roles.lfy", &roles_with(PASSES))
            .write("def/targets.lfy", TARGETS)
            .write(
                "def/b/one.lfy",
                &format!(
                    "d B: `A B {note}` {{\n  @acceptanceCriteria\n    .add({{ behavior = `Holds a number` }});\n}}\n"
                ),
            )
            .write("def/a/one.lfy", &format!("use \"../b/one\";\n\nd A: `An A {note}` {{}}\n"))
    }

    /// A compiler that writes each unit's output, notes that it ran for that unit, and, for
    /// `def/a/one`, keeps what its directory held of the output of `def/b/one` when it
    /// started. The body it writes for B is its first on its first run for that unit and its
    /// second after that, so a run once more rewrites one function.
    // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
    // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
    fn compiler_of_two(first: &str, again: &str) -> String {
        format!(
            "#!/bin/sh\ncat > /dev/null\nmkdir -p \"$ELFIE_ROOT/out/a\" \"$ELFIE_ROOT/out/b\"\n\
             case \"$ELFIE_UNITS\" in\n\
             *b/one*)\n\
             echo ran >> \"$ELFIE_ROOT/ran-b.txt\"\n\
             if [ \"$(wc -l < \"$ELFIE_ROOT/ran-b.txt\")\" -gt 1 ]; then body='{again}'; else body='{first}'; fi\n\
             printf '// @lfy def/b/one.lfy:B\\n%s\\n' \"$body\" > \"$ELFIE_ROOT/out/b/one.rs\"\n\
             ;;\n\
             *a/one*)\n\
             echo ran >> \"$ELFIE_ROOT/ran-a.txt\"\n\
             cp \"$ELFIE_ROOT/out/b/one.rs\" \"$ELFIE_ROOT/seen-b.txt\" || echo missing > \"$ELFIE_ROOT/seen-b.txt\"\n\
             printf '// @lfy def/a/one.lfy:A\\npub struct A {{}}\\n' > \"$ELFIE_ROOT/out/a/one.rs\"\n\
             ;;\n\
             esac\n\
             echo 'ELFIE: DONE'\n"
        )
    }

    /// A verifier that takes long enough for the batch after it to start, notes that it ran,
    /// and answers for the one criterion: violated on its first run when `first` says so, and
    /// satisfied after that.
    // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
    // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
    fn slow_verifier(id: &str, first: &str) -> String {
        format!(
            "#!/bin/sh\ncat > /dev/null\necho \"$ELFIE_BATCH\" >> \"$ELFIE_ROOT/verified.txt\"\nsleep 1\n\
             if [ \"$(wc -l < \"$ELFIE_ROOT/verified.txt\")\" -gt 1 ]; then status=satisfied; else status={first}; fi\n\
             printf '{{\"id\":\"{id}\",\"status\":\"%s\",\"evidence\":\"out/b/one.rs:1-2\",\"note\":\"looked at\"}}\\n' \"$status\"\n\
             echo 'ELFIE: REVIEWED'\n"
        )
    }

    /// Both units recorded by one compile that asks no verifier, so that the compile under
    /// test plans both of them for what changed since.
    // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
    fn recorded_twice(fixture: &Fixture, first: &str, again: &str) {
        fixture
            .write("compiler.sh", &compiler_of_two(first, again))
            .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
        assert_eq!(fixture.run(&["compile", "--jobs", "1", "--no-verify"]), ExitCode::Success.code());
    }

    /// With --jobs 2 in a git repository, the batch of `a` starts in a copy holding what the
    /// batch of `b` accepted, before b's review has ended; b is merged into the root first,
    /// a is merged after it with no build or test run, and both maps are recorded.
    // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
    #[test]
    fn a_batch_starts_against_what_the_batch_before_it_accepted() {
        let fixture = Fixture::new();
        two_batches(&fixture, "then");
        if !a_repository(&fixture) {
            return;
        }
        // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
        recorded_twice(&fixture, "pub struct B {}", "pub struct B {}");
        // Both are edited since they were recorded, so both are planned.
        // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
        two_batches(&fixture, "now");
        let id = criterion_in(&fixture, "b/one.lfy");
        fixture
            .write("compiler.sh", &compiler_of_two("pub struct B(u8);", "pub struct B(u8);"))
            .write("verifier.sh", &slow_verifier(&id, "satisfied"))
            .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]));
        let _ = fs::remove_file(fixture.root.join("ran-a.txt"));
        let _ = fs::remove_file(fixture.root.join("ran-b.txt"));
        forget_log(&fixture);
        // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
        assert_eq!(fixture.run(&["compile", "--jobs", "2"]), ExitCode::Success.code());
        let log = fixture.read("elfie-requests/compile.log");
        assert!(log.contains("2 units planned, 2 batches"), "{log}");
        // The copy of a held what b had accepted, before b's review ended.
        // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
        assert!(fixture.read("seen-b.txt").contains("pub struct B(u8);"), "{}", fixture.read("seen-b.txt"));
        // Nothing of b's merge differed from what a took, so no build or test ran.
        // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
        assert!(!log.contains("\u{25b8} building"), "{log}");
        // Both are merged into the root and both maps are recorded.
        // @lfy def/cli/main.lfy:main#main:main:b7262d48ed581a2a6d6d97961b2b16702098182ee1ba781ed507551af8dc1cff
        assert!(fixture.read("out/b/one.rs").contains("pub struct B(u8);"));
        assert!(fixture.read("out/a/one.rs").contains("pub struct A"));
        assert!(fixture.read("elfie-compile/maps/rust/b/one.json").contains("out/b/one.rs"));
        assert!(fixture.read("elfie-compile/maps/rust/a/one.json").contains("out/a/one.rs"));
    }

    /// A batch whose copy took what another batch accepted, where that batch then ran once
    /// more and reached the root with other work: the files its merge wrote are taken into
    /// the copy, the target's build and test commands run there, and the batch is merged once
    /// they pass, its compiler having run once.
    // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
    #[test]
    fn a_copy_takes_what_the_batch_before_it_merged_and_is_built_again() {
        let fixture = Fixture::new();
        two_batches(&fixture, "then");
        if !a_repository(&fixture) {
            return;
        }
        // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
        recorded_twice(&fixture, "pub struct B {}", "pub struct B {}");
        // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
        two_batches(&fixture, "now");
        let id = criterion_in(&fixture, "b/one.lfy");
        // The compiler's run once more on b rewrites the body of one function, and the
        // verifier's first review of b finds the criterion violated.
        // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
        fixture
            .write("compiler.sh", &compiler_of_two("pub struct B(u8);", "pub struct B(u16);"))
            .write("verifier.sh", &slow_verifier(&id, "violated"))
            .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]));
        let _ = fs::remove_file(fixture.root.join("ran-a.txt"));
        let _ = fs::remove_file(fixture.root.join("ran-b.txt"));
        forget_log(&fixture);
        // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
        assert_eq!(fixture.run(&["compile", "--jobs", "2"]), ExitCode::Success.code());
        let log = fixture.read("elfie-requests/compile.log");
        // a's batch started against b's first accepted output.
        // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
        assert!(fixture.read("seen-b.txt").contains("pub struct B(u8);"), "{}", fixture.read("seen-b.txt"));
        // b ran once more and what it merged is the body it wrote then.
        // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
        assert_eq!(lines_of(&fixture, "ran-b.txt"), 2, "{log}");
        assert!(fixture.read("out/b/one.rs").contains("pub struct B(u16);"));
        // A building line for a, since the files b merged differ from what a took.
        // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
        assert!(log.contains("\u{25b8} building"), "{log}");
        // a is merged after the build and the test pass, with its compiler run once.
        // @lfy def/cli/main.lfy:main#main:main:e803dbdf5ac818e716c02ce96a601f0a57ca23b3acd29c5b6698afe1148ce2f5
        assert_eq!(lines_of(&fixture, "ran-a.txt"), 1, "{log}");
        assert!(fixture.read("out/a/one.rs").contains("pub struct A"));
        assert!(fixture.read("elfie-compile/maps/rust/a/one.json").contains("out/a/one.rs"));
    }

    // ---- the agent server ----------------------------------------------------------

    /// What a host made by hand recorded: the binding of every session it opened, the URL of
    /// every session it closed, and how many times it was stopped.
    // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    struct Recorded {
        opened: Arc<Mutex<Vec<Binding>>>,
        closed: Arc<Mutex<Vec<String>>>,
        stopped: Arc<Mutex<usize>>,
    }

    /// A host made by hand, which records what it is asked, offers the tool names given, and
    /// gives back the record given when a session is closed.
    // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    fn a_host(record: SessionRecord, offers: Vec<String>) -> (Host, Recorded) {
        let opened: Arc<Mutex<Vec<Binding>>> = Arc::new(Mutex::new(Vec::new()));
        let closed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stopped: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let recorded =
            Recorded { opened: opened.clone(), closed: closed.clone(), stopped: stopped.clone() };
        let host = Host {
            url: "http://127.0.0.1:7/".to_string(),
            token: "a-token".to_string(),
            open: Box::new(move |binding| {
                let mut held = opened.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                held.push(binding);
                format!("http://127.0.0.1:7/sessions/{}", held.len())
            }),
            // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
            offered: Box::new(move |_| offers.clone()),
            close: Box::new(move |url| {
                closed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(url.to_string());
                record.clone()
            }),
            stop: Box::new(move || {
                *stopped.lock().unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
            }),
        };
        (host, recorded)
    }

    /// An agent whose agent server is the one given, or the failure given, rather than one
    /// hosted for real.
    // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
    fn an_agent(root: &Path, hosted: Result<Host, String>) -> Agent {
        Agent {
            json: false,
            style: Paint { out: false, err: false },
            logs: Vec::new(),
            elfie: PathBuf::from("elfie"),
            // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
            server: Arc::new(Server {
                root: root.to_path_buf(),
                hosted: Mutex::new(Some(hosted.map(Arc::new))),
            }),
        }
    }

    /// One compile of the fixture's one unit, locked, so that a run of an agent can be made
    /// by hand.
    // @lfy def/cli/main.lfy:main
    fn a_run(fixture: &Fixture) -> Run {
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
        let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
        Run::new(program, plan, Vec::new(), reporter, &invocation)
    }

    /// Every run of the compiler or the verifier works through one hosted session of the
    /// agent server: it is opened just before the run starts, bound to what the run is, its
    /// config file is written under the root and named by ELFIE_MCP_CONFIG, the command tools
    /// it is offered are named by ELFIE_COMMAND_TOOLS, and it is closed once the run has
    /// returned, giving back what the run submitted through it, its config file removed.
    // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
    // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
    // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
    // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
    // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
    #[test]
    fn a_run_of_an_agent_works_through_one_hosted_session() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        let run = a_run(&fixture);
        let batch = run.plan.batches[0].clone();
        let submitted = Review {
            id: "A:A:0".to_string(),
            file: "def/a.lfy".to_string(),
            line: 1,
            entity: "A".to_string(),
            status: ReviewStatus::Satisfied,
            evidence: "out/a.rs:1-2".to_string(),
            note: "covered by a test".to_string(),
        };
        let record = SessionRecord {
            reviews: vec![submitted.clone()],
            submission: Some(Submission { outcome: SubmittedOutcome::Done, message: String::new() }),
        };
        // The server lists more than the command tools, and lists them in its own order.
        // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        let offers = ["elfie_task", "elfie_test", "elfie_build", "elfie_mark", "elfie_lint"]
            .map(str::to_string)
            .to_vec();
        let (host, recorded) = a_host(record, offers);
        let agent = an_agent(&fixture.root, Ok(host));
        let where_ = Where::Root(fixture.root.clone());
        // The compiler of a batch: its binding has the role compiler, the batch, the
        // directory the run is in, the plan and the program the command made, the request its
        // input was made from, no review, and no source maps.
        // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
        let request = run.request_of(&batch, &fixture.root, true, None);
        let binding =
            run.binding_of(Role::Compiler, Some(&batch), &fixture.root, Some(request.clone()), None, Vec::new());
        let exit = agent.run(
            "printf '%s\\n' \"$ELFIE_MCP_CONFIG\" \"$ELFIE_COMMAND_TOOLS\" \"$ELFIE_SESSION\" \
             \"${ELFIE_RESUME-none}\" > config.txt\n\
             cp \"$ELFIE_MCP_CONFIG\" written.json",
            &where_,
            &batch.identifier,
            "a",
            "",
            binding,
            None,
            None,
        );
        assert_eq!(exit.code, 0, "{}", exit.stderr);
        // ELFIE_SESSION is an agent session id new for the run: a version 4 UUID from the
        // operating system's secure random source, 36 lowercase characters in the 8-4-4-4-12
        // form, and a run that is no resuming run carries no ELFIE_RESUME at all.
        // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        let written: Vec<String> = fixture.read("config.txt").lines().map(str::to_string).collect();
        // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
        assert_eq!(written[2], exit.session, "the run was told its own session id");
        assert!(is_a_uuid(&exit.session), "{} is no version 4 UUID", exit.session);
        // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
        assert_eq!(written[3], "none", "ELFIE_RESUME was in the environment");
        // One session was opened for the run, bound to what it is.
        // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
        let session = {
            let opened = recorded.opened.lock().unwrap();
            assert_eq!(opened.len(), 1);
            // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
            assert_eq!(opened[0].role, Role::Compiler);
            assert_eq!(opened[0].batch.as_ref().map(|batch| batch.identifier.clone()), Some("a".to_string()));
            // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
            assert_eq!(opened[0].directory, fixture.root);
            assert_eq!(opened[0].plan, run.plan);
            assert_eq!(opened[0].program, run.program);
            // The request the run's input was made from, and nothing a verifier is given.
            // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
            assert_eq!(
                opened[0].request.as_ref().map(|request| request.instructions.clone()),
                Some(request.instructions.clone()),
            );
            // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
            assert_eq!(opened[0].review, None);
            assert_eq!(opened[0].source_maps, []);
            "http://127.0.0.1:7/sessions/1".to_string()
        };
        // Its config file is written under the root, named by the batch and the role, and
        // ELFIE_MCP_CONFIG in its environment is the absolute path of it; ELFIE_COMMAND_TOOLS
        // is the command tools it was offered, in the order the server listed them.
        // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
        // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        let path = fixture.root.join("elfie-requests/a.compiler.mcp.json");
        let environment = fixture.read("config.txt");
        let mut lines = environment.lines();
        assert_eq!(lines.next(), Some(path.to_string_lossy().as_ref()));
        assert!(path.is_absolute(), "{path:?}");
        // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        assert_eq!(lines.next(), Some("elfie_test elfie_build elfie_lint"));
        // It held one object naming the session's URL and the bearer token, and is gone once
        // the run has returned, since its token no longer opens anything.
        // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
        let value: serde_json::Value = serde_json::from_str(&fixture.read("written.json")).unwrap();
        let server = &value["mcpServers"]["elfie"];
        // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
        assert_eq!(server["type"], "http");
        assert_eq!(server["url"], session);
        assert_eq!(server["headers"]["Authorization"], "Bearer a-token");
        // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
        assert!(!path.exists(), "the config file is still there");
        // The session was closed once the run returned, and what it submitted came back with
        // its output.
        // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
        assert_eq!(*recorded.closed.lock().unwrap(), vec![session]);
        // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
        assert_eq!(exit.record.reviews, vec![submitted]);
        // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
        assert_eq!(exit.record.submission.map(|submission| submission.outcome), Some(SubmittedOutcome::Done));
        // The server is stopped once, however often it is asked to stop.
        // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
        agent.server.stop();
        agent.server.stop();
        assert_eq!(*recorded.stopped.lock().unwrap(), 1);
    }

    /// How a compiler run ended is read from the outcome it submitted through its session,
    /// not from its report: a run that submitted `blocked` is blocked, with the submission's
    /// message as its reason, though its report says nothing at all.
    // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
    #[test]
    fn the_outcome_of_a_compiler_run_is_read_from_what_it_submitted() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        let mut run = a_run(&fixture);
        let batch = run.plan.batches[0].clone();
        // The run submits blocked through its session and writes no report of its own.
        // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
        let record = SessionRecord {
            reviews: Vec::new(),
            submission: Some(Submission {
                outcome: SubmittedOutcome::Blocked,
                message: "two criteria of A conflict".to_string(),
            }),
        };
        let (host, _recorded) = a_host(record, Vec::new());
        run.agent = an_agent(&fixture.root, Ok(host));
        let run = Mutex::new(run);
        let where_ = Where::Root(fixture.root.clone());
        let runs = {
            let mut state = locked(&run);
            let agent = state.agent.clone();
            Runs::new(agent, state.running_of(&batch.identifier))
        };
        let planned = vec![a_planned_batch(&batch.identifier, 0, Vec::new())];
        let schedule = Mutex::new(Schedule::of(&planned));
        let finished = Condvar::new();
        let round = Round { schedule: &schedule, finished: &finished, planned: &planned, at: 0, copies: false };
        // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
        let compiled = Run::compile_batch(&run, &batch, "true", &where_, &runs, None, &round);
        // The outcome is blocked: nothing is held to record, and the reason the submission
        // carried is written out.
        // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
        assert!(compiled.held.is_none(), "the batch was not blocked");
        // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
        let note = fixture.read("elfie-requests/a.blocked.md");
        assert!(note.contains("two criteria of A conflict"), "{note}");
    }

    /// The verifier of a batch is bound to the review request it was given and the source
    /// maps that request was made with; the verifier of the global review to no batch.
    // @lfy def/cli/main.lfy:main#main:main:d7fdc89e00db8f2de0886cdaaecf381963abe8e1eb6392c9dee9bc26a30911b5
    // @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
    #[test]
    fn a_verifier_is_bound_to_the_review_it_was_given() {
        let fixture = Fixture::new();
        one_target(&fixture)
            .write("def/a.lfy", "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\n");
        let run = a_run(&fixture);
        let batch = run.plan.batches[0].clone();
        let maps = generation::source_maps_of(&run.program.workspace, None);
        let request = run.review_request_of(&batch, &maps, Some(&fixture.root), &[]);
        // @lfy def/cli/main.lfy:main#main:main:d7fdc89e00db8f2de0886cdaaecf381963abe8e1eb6392c9dee9bc26a30911b5
        let binding =
            run.binding_of(Role::Verifier, Some(&batch), &fixture.root, None, Some(request.clone()), maps.clone());
        assert_eq!(binding.role, Role::Verifier);
        assert_eq!(binding.batch.as_ref().map(|batch| batch.identifier.clone()), Some("a".to_string()));
        // @lfy def/cli/main.lfy:main#main:main:d7fdc89e00db8f2de0886cdaaecf381963abe8e1eb6392c9dee9bc26a30911b5
        assert_eq!(binding.review.as_ref().map(|review| review.asked.clone()), Some(request.asked.clone()));
        assert_eq!(binding.source_maps, maps);
        // The global review is for no batch.
        // @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
        let global = generation::global_review(&run.program, &maps);
        let binding = run.binding_of(Role::Verifier, None, &fixture.root, None, Some(global), maps.clone());
        // @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
        assert_eq!(binding.batch, None);
        assert_eq!(binding.role, Role::Verifier);
        assert!(binding.review.is_some());
        assert_eq!(binding.source_maps, maps);
    }

    /// An agent server that cannot be hosted runs no compiler and no verifier: the run's code
    /// is -1 and its failure names the server, which makes the code of the command failure.
    // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    #[test]
    fn a_server_that_cannot_be_hosted_runs_nothing() {
        let fixture = Fixture::new();
        one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
        let run = a_run(&fixture);
        let batch = run.plan.batches[0].clone();
        let agent = an_agent(&fixture.root, Err("no port of 127.0.0.1 can be listened on".to_string()));
        let where_ = Where::Root(fixture.root.clone());
        let binding = run.binding_of(Role::Compiler, Some(&batch), &fixture.root, None, None, Vec::new());
        // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        let exit = agent.run("echo ran > ran.txt", &where_, &batch.identifier, "a", "", binding, None, None);
        // Nothing ran, and the failure is what the run came to; a run whose code is -1 makes
        // the code of the command failure.
        // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        assert_eq!(exit.code, -1);
        assert!(!fixture.root.join("ran.txt").exists(), "the command ran");
        assert!(exit.stderr.contains("the agent server could not be hosted"), "{}", exit.stderr);
        assert!(exit.stderr.contains("no port"), "{}", exit.stderr);
        // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
        assert!(!fixture.root.join("elfie-requests/a.compiler.mcp.json").exists(), "a session was opened");
    }

    // ---- carrying a verdict --------------------------------------------------------

    /// A reviewed line counts the reviews of its own run and then the verdicts its review
    /// request carried, which are counted there and nowhere else; with nothing carried the
    /// counts end with the unverifiable.
    // @lfy def/cli/main.lfy:main#main:main:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
    // @lfy def/cli/main.lfy:main#main:main:f088a8a0ce2ac66dea4de2f7c633f70122a6f42b582ae35203ed2bea262e67f3
    // @lfy def/cli/main.lfy:main#main:main:40a59e64f37d7276f9f2e1c4d1434060c145115922c8218363f3a700cb7f7beb
    // @lfy def/cli/main.lfy:main#main:main:01ac2a8e3e988e7c07b647cd40cc38bce72044f98971d9e5e72eb5c62425cc0e
    // @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
    // @lfy def/cli/main.lfy:main#main:main:5e715c81a6d8d3edeadda85d5a0a8cc96d3f5aa33c9009f7b213ae941ccb4eff
    // @lfy def/cli/main.lfy:main#main:main:cc320fe0b40fde7c47dca674e64001b0a974e93cf2ffa2186a7c30bbf7aad8f7
    // @lfy def/cli/main.lfy:main#main:main:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
    #[test]
    fn a_reviewed_line_counts_the_carried_verdicts_last() {
        let satisfied = Review {
            id: "A:A:0".to_string(),
            file: "def/a.lfy".to_string(),
            line: 3,
            entity: "A".to_string(),
            status: ReviewStatus::Satisfied,
            evidence: "out/a.rs:1-2".to_string(),
            note: "covered by a test".to_string(),
        };
        let reviews = [satisfied];
        // Nothing carried: the message begins with the three counts and ends with them.
        // @lfy def/cli/main.lfy:main#main:main:40a59e64f37d7276f9f2e1c4d1434060c145115922c8218363f3a700cb7f7beb
        // @lfy def/cli/main.lfy:main#main:main:5e715c81a6d8d3edeadda85d5a0a8cc96d3f5aa33c9009f7b213ae941ccb4eff
        assert_eq!(
            counts_message(&counts_with_carried(&reviews, 0, 0), false),
            "1 satisfied, 0 violated, 0 unverifiable"
        );
        // Something carried: the count and the word carried follow, after a comma and a
        // space.
        // @lfy def/cli/main.lfy:main#main:main:f088a8a0ce2ac66dea4de2f7c633f70122a6f42b582ae35203ed2bea262e67f3
        // @lfy def/cli/main.lfy:main#main:main:cc320fe0b40fde7c47dca674e64001b0a974e93cf2ffa2186a7c30bbf7aad8f7
        let counts = counts_with_carried(&reviews, 0, 2);
        assert_eq!(counts_message(&counts, false), "1 satisfied, 0 violated, 0 unverifiable, 2 carried");
        // That count and its word are painted muted, and painting leaves the words as they
        // are.
        // @lfy def/cli/main.lfy:main#main:main:01ac2a8e3e988e7c07b647cd40cc38bce72044f98971d9e5e72eb5c62425cc0e
        assert_eq!(counts.last(), Some(&(2, "carried", Tone::Muted)));
        // @lfy def/cli/main.lfy:main#main:main:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
        assert_eq!(width(&counts_message(&counts, true)), width(&counts_message(&counts, false)));
        assert!(counts_message(&counts, true).contains(&tally(2, "carried", Tone::Muted, true)));
        // A carried verdict is counted only as carried: it is neither satisfied, violated,
        // nor unverifiable.
        // @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
        // @lfy def/cli/main.lfy:main#main:main:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
        assert_eq!(
            counts_message(&counts_with_carried(&[], 0, 3), false),
            "0 satisfied, 0 violated, 0 unverifiable, 3 carried"
        );
    }

    /// A compile records the verdict on each criterion and test of its units, and a review of
    /// a batch whose verdicts all still hold asks about nothing: no verifier runs on it, and
    /// --full asks about them again.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
    // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
    // @lfy def/cli/main.lfy:main#main:main:f79d62c52638caa4cb00a160316bc7052bc62e29caef830e683dba3fca9581ca
    #[test]
    fn a_settled_verdict_is_carried_and_asks_no_verifier() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // The verdict is recorded in the unit's review record file, with the hash of the
        // regions it was judged against.
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        let records = fixture.read("elfie-compile/reviews/rust/a.json");
        assert!(records.contains(&criterion_id(&fixture)), "{records}");
        assert!(records.contains("satisfied"), "{records}");
        let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
        let read = generation::reviews_of(&program, &plan.units[0]);
        // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
        assert_eq!(read.len(), 1, "{read:?}");
        assert_eq!(read[0].status, ReviewStatus::Satisfied);
        assert!(!read[0].hash.is_empty(), "{read:?}");
        // Nothing of its code changed, so a review of its batch carries that verdict, asks
        // about nothing, and runs no verifier.
        // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
        // @lfy def/cli/main.lfy:main#main:main:f79d62c52638caa4cb00a160316bc7052bc62e29caef830e683dba3fca9581ca
        assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Success.code());
        assert_eq!(lines_of(&fixture, "verifications.txt"), 1, "the verifier ran again");
        // With --full every criterion and test is asked about again.
        // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
        assert_eq!(fixture.run(&["verify", "a", "--full"]), ExitCode::Success.code());
        assert_eq!(lines_of(&fixture, "verifications.txt"), 2, "the verifier did not run again");
        // The review request of the batch asks about nothing while the verdict holds, and
        // carries it instead.
        // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
        // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
        let run = a_run(&fixture);
        let batch = run.plan.batches[0].clone();
        let maps = generation::source_maps_of(&run.program.workspace, None);
        let kept = run.records_of(&batch);
        assert_eq!(kept.len(), 1, "{kept:?}");
        let request = run.review_request_of(&batch, &maps, None, &kept);
        // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
        assert_eq!(request.asked, [] as [String; 0], "{:?}", request.asked);
        assert_eq!(request.carried.len(), 1, "{:?}", request.carried);
        // With no records nothing is carried and the criterion is asked about.
        // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
        let full = run.review_request_of(&batch, &maps, None, &[]);
        assert_eq!(full.asked.len(), 1, "{:?}", full.asked);
        assert!(full.carried.is_empty(), "{:?}", full.carried);
    }

    /// With --no-verify, with no verifier named, or with --accept, no review record file
    /// changes.
    // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
    // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    #[test]
    fn nothing_that_runs_no_verifier_records_a_verdict() {
        // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
        let quiet = Fixture::new();
        a_compiler_and_a_verifier(&quiet, SATISFIED);
        assert_eq!(quiet.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        assert!(quiet.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
        // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
        assert!(!quiet.root.join("elfie-compile/reviews").exists(), "a review record was written");
        // A project naming no verifier is the same.
        // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
        let plain = Fixture::new();
        a_compiler_and_a_verifier(&plain, SATISFIED)
            .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
        assert_eq!(plain.run(&["compile"]), ExitCode::Success.code());
        assert!(!plain.root.join("elfie-compile/reviews").exists(), "a review record was written");
        // With --accept the verifier runs and the maps are recorded, but no review record
        // file changes.
        // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
        let accepted = Fixture::new();
        a_compiler_and_a_verifier(&accepted, SATISFIED);
        accepted.write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n");
        assert_eq!(accepted.run(&["compile", "--accept"]), ExitCode::Success.code());
        assert!(accepted.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
        // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
        assert!(!accepted.root.join("elfie-compile/reviews").exists(), "a review record was written");
    }

    /// A review record file belonging to no unit of the plan is removed before any batch
    /// runs, and the file of a unit of the plan is left as it was.
    // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
    #[test]
    fn a_review_record_file_of_no_unit_is_removed() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        let stale = fixture.read("elfie-compile/reviews/rust/a.json");
        // A unit whose definition file was deleted leaves its review record file behind.
        // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
        fixture.write("elfie-compile/reviews/rust/gone.json", &stale);
        fixture.write("def/b.lfy", "d B {}\n");
        fixture.write("compiler.sh", PER_UNIT_COMPILER);
        write_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
        assert!(
            !fixture.root.join("elfie-compile/reviews/rust/gone.json").exists(),
            "the review record file stayed"
        );
        // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
        assert_eq!(fixture.read("elfie-compile/reviews/rust/a.json"), stale, "a.json changed");
        // With --no-verify nothing of the kind is removed, since nothing reviews anything.
        // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
        fixture.write("elfie-compile/reviews/rust/gone.json", &stale);
        fixture.write("def/c.lfy", "d C {}\n");
        assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
        assert!(fixture.root.join("elfie-compile/reviews/rust/gone.json").is_file(), "the file was removed");
    }

    /// A verify whose review request asked nothing is success, as one whose verifier was
    /// started and violated nothing is.
    // @lfy def/cli/main.lfy:main#main:main:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
    #[test]
    fn a_verify_that_asked_nothing_is_success() {
        let fixture = Fixture::new();
        a_compiler_and_a_verifier(&fixture, SATISFIED);
        assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
        // Every verdict is carried, so no verifier is started and nothing is violated.
        // @lfy def/cli/main.lfy:main#main:main:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
        assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Success.code());
        // The verifier started and violated nothing: success too.
        // @lfy def/cli/main.lfy:main#main:main:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
        assert_eq!(fixture.run(&["verify", "a", "--full"]), ExitCode::Success.code());
        // A violated review is problems all the same, carried verdicts or not.
        // @lfy def/cli/main.lfy:main#main:main:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
        write_verifier(&fixture, VIOLATED);
        assert_eq!(fixture.run(&["verify", "a", "--full"]), ExitCode::Problems.code());
    }
}
