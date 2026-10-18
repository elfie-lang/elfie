// @lfy def/cli/main.lfy:parse
//! Reading the arguments, and the commands that need nothing but the program and the source
//! files: help, version, init, check, format, tree, and tokens.

use super::*;

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
                // @lfy def/cli/main.lfy:parse#parse:parse:f02fa71e7da3224799ef529967d655bc2283a6e888528c149468713605888a18
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
                // @lfy def/cli/main.lfy:parse#parse:parse:f02fa71e7da3224799ef529967d655bc2283a6e888528c149468713605888a18
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
                // --target is the option besides --root that takes a following value, and the
                // option target gives it.
                // @lfy def/cli/main.lfy:parse#parse:parse:d430b5e6a7470398614d725eafdc05b807ff1820689bae58a91114f9b344c530
                if let Some(value) = arguments.get(i + 1) {
                    options.insert(name.to_string(), Some(value.clone()));
                    i += 1;
                } else if forced.is_none() {
                    // The last argument with nothing after it to take: the option is named
                    // back, unless --help, -h, or --version has already named the command.
                    // @lfy def/cli/main.lfy:parse#parse:parse:f02fa71e7da3224799ef529967d655bc2283a6e888528c149468713605888a18
                    return Err(format!("--{name} needs a value"));
                }
            } else {
                // Every other option written with no equals sign is a flag and gives true,
                // never taking the next argument, so --json check still names the check
                // command.
                // @lfy def/cli/main.lfy:parse#parse:parse:3b63870b5bd8c3d705b3fe36f7d8df1123db22b57fa590663e84f4f4a1e59008
                // @lfy def/cli/main.lfy:parse#parse:parse:dcb7c2a03e1a5126afb2f4ffe2edf196cc6deae05a25ee2cecbf8b4666b2c8f8
                options.insert(name.to_string(), None);
            }
            i += 1;
            continue;
        }
        // Any other argument beginning with a single dash is neither an option, nor the
        // command, nor positional, so the first argument without a dash still names the
        // command and one holding none spells help.
        // @lfy def/cli/main.lfy:parse#parse:parse:18a414d2a0d543bdb98172cdb41626091e3c89ed51f6a659e96ca784ab8a7499
        // @lfy def/cli/main.lfy:parse#parse:parse:6a504086755823febb0f5ac3bc59d3026a9663641ef622aa1db6e0c47626d459
        if argument.starts_with('-') {
            i += 1;
            continue;
        }
        // The first argument that does not begin with a dash names the command.
        // @lfy def/cli/main.lfy:parse#parse:parse:18a414d2a0d543bdb98172cdb41626091e3c89ed51f6a659e96ca784ab8a7499
        if command.is_none() && forced.is_none() {
            match Command::lookup(argument) {
                Some(found) => command = Some(found),
                // @lfy def/cli/main.lfy:parse#parse:parse:407b80d916b66360f99967d28e89ba7016a7c488369e0a70acdef6af3936646d
                None => return Err(format!("{argument} is not a command")),
            }
        } else {
            // Every other one, the value --root, --target, or --color took aside, is
            // positional.
            // @lfy def/cli/main.lfy:parse#parse:parse:dd2b0b0390de36e5311e96979f200953effcf502ba2cd611c18f6bf2ac6c6545
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
    // @lfy def/cli/main.lfy:parse#parse:parse:6a504086755823febb0f5ac3bc59d3026a9663641ef622aa1db6e0c47626d459
    Ok(Invocation {
        command: command.unwrap_or(Command::Help),
        root: find_root(root.as_deref()),
        arguments: positional,
        options,
    })
}

/// Options that take a value written as the next argument: --target is the one option
/// besides --root that takes a following value unconditionally, and --color takes one only
/// when it names a [`ColorChoice`].
// @lfy def/cli/main.lfy:parse
pub(crate) const OPTIONS_WITH_VALUES: [&str; 1] = ["target"];

/// What a --color value that is no [`ColorChoice`] is answered with.
// @lfy def/cli/main.lfy:parse
pub(crate) const COLOR_USAGE: &str = "--color must be auto, always, or never";

/// The nearest directory at or above the current directory where elfie.json exists, or
/// the current one.
// @lfy def/cli/main.lfy:parse
pub(crate) fn find_root(given: Option<&str>) -> String {
    // The directory --root named is the root, made absolute against the current directory
    // when it is relative: a change set's copy lives under the root and its compiler runs
    // inside that copy, so a relative root would name one place from the CLI and another from
    // there.
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

/// Prints every command, verify among them, with one line of description and the global
/// options, --no-verify, --continue, --strict, and --color among them.
// @lfy def/cli/main.lfy:main#main:main:6f0073ceecb972e98aae5b1618d058d12e3264a00f6faa1982ea18e0ca49ac39
pub(crate) fn help(invocation: &Invocation) -> ExitCode {
    print!("{}", help_text(Paint::of(invocation).out));
    // @lfy def/cli/main.lfy:main#main:main:0bb3adb0044327ff76f406b826c6de676f76bfa10744e5c496b356815b59c852
    ExitCode::Success
}

/// Prints the version of the executable.
// @lfy def/cli/main.lfy:main#main:main:c2de1ad0a3b01e4d1d349d3ff8890849a16e58365d070af5b7ffaf7e0ddaa0e6
pub(crate) fn version() -> ExitCode {
    println!("elfie {}", env!("CARGO_PKG_VERSION"));
    // @lfy def/cli/main.lfy:main#main:main:053c30d9cd6d0bc161a09937054718d2692697f373604978c53abdc9440983de
    ExitCode::Success
}

/// The one line .gitignore holds for a project the CLI set up: the cache is derivable and
/// nothing else under elfie-compile is, since the maps are checked in.
// @lfy def/cli/main.lfy:main#main:main:c160a3dc4d430206c372000abe8e8c0d338ea73fab5879b08956e0b6b4d9c22f
pub(crate) const IGNORED: &str = "/elfie-compile/cache/";

/// Creates elfie.json in the root naming the project after the argument, or the directory,
/// creates the source directory and elfie-compile/maps, and leaves .gitignore ignoring the
/// cache; fails when elfie.json already exists.
// @lfy def/cli/main.lfy:main
pub(crate) fn init(invocation: &Invocation) -> ExitCode {
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
pub(crate) fn created_line(json: bool, manifest: &str) -> String {
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
pub(crate) fn ignore_the_cache(root: &Path) -> io::Result<()> {
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

/// Load the root, then every diagnostic printed, or of the files given only; the code is
/// problems when any is an error and success otherwise. With no files given and no error,
/// each unit that is not up to date is also printed as a warning and counted, and --strict
/// fails on any warning, error, or stale unit.
// @lfy def/cli/main.lfy:main
pub(crate) fn check(invocation: &Invocation) -> ExitCode {
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
        let (program, plan) = planned(root, &maps, &[], false, &violated_ids(root), &[]);
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
pub(crate) fn print_no_target(manifest: &str, json: bool, on: bool) {
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
pub(crate) fn print_stale(workspace: &Workspace, unit: &Unit, reason: Reason, json: bool, on: bool) {
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


/// One of [`Plan::moves`] printed as information of stage generation at the file of its
/// origin, naming the target, the identifiers of the entities moved, the stem of the unit
/// that compiles them, and the cause; it is neither a problem nor a stale unit.
// @lfy def/cli/main.lfy:main#main:main:2c23167a93d05731803b314b2b6545c5ee1481978f5c546aacb5c0b7f512115a
pub(crate) fn print_move(workspace: &Workspace, plan: &Plan, moved: &Move, json: bool, on: bool) {
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
pub(crate) fn collect_diagnostics(workspace: &Workspace, files: &[String]) -> Vec<Diagnostic> {
    // @lfy def/cli/main.lfy:main#main:main:5195c7d14b9b471efde014a85a9899221c2f10ef7c18a0bae7557e737b529f8e
    if files.is_empty() {
        query::diagnostics_of(workspace, None)
    } else {
        // @lfy def/cli/main.lfy:main#main:main:2522a3017f6b6c237cf9b516c25d92ce44fedd90b1e60a1c5bf46810edf6a8c1
        files.iter().flat_map(|file| query::diagnostics_of(workspace, Some(file))).collect()
    }
}

/// Each file given, or every .lfy file under each source directory, is replaced by the
/// standard layout when that differs; --check only reports.
// @lfy def/cli/main.lfy:main
pub(crate) fn format_command(invocation: &Invocation) -> ExitCode {
    let root = Path::new(&invocation.root);
    let check_only = invocation.flag("check");
    let json = invocation.flag("json");
    let style = Paint::of(invocation);
    // Every .lfy file under each source directory, recursively, or each file given.
    // @lfy def/cli/main.lfy:main#main:main:c7d7291e53c36dbf36962e6e8f80c4b5660d3e85a30bf7c345d861e32d8e7fc6
    // @lfy def/cli/main.lfy:main#main:main:3454380ed0ab0be90c65034578648d28e79c0ec647abe81aafa5f4149ea66d16
    let files: Vec<PathBuf> = if invocation.arguments.is_empty() {
        let workspace = workspace::load(root);
        let mut found = Vec::new();
        for directory in &workspace.source_directories {
            walk(&root.join(directory), &mut found);
        }
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
pub(crate) fn formatted_line(json: bool, file: &str, check_only: bool, on: bool) -> String {
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
// @lfy def/cli/main.lfy:main#main:main:c7d7291e53c36dbf36962e6e8f80c4b5660d3e85a30bf7c345d861e32d8e7fc6
pub(crate) fn walk(directory: &Path, out: &mut Vec<PathBuf>) {
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
pub(crate) fn read_one_file(invocation: &Invocation) -> Result<(String, String), ExitCode> {
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
pub(crate) fn rule_name(token: &Token) -> &'static str {
    token.rule.map_or("invalid", |rule| rule.identifier())
}

/// Where an error node begins, as the line and column of its first token.
// @lfy def/cli/main.lfy:main
pub(crate) fn error_position(tree: &Tree, error: &ErrorNode) -> (usize, usize) {
    let token = tree.tokens.get(error.start).or(tree.tokens.last());
    token.map_or((0, 0), |token| (token.line, token.column))
}

/// One node, error node, or token of the tree as a JSON object, then its children: the
/// depth it sits at, what it is, and what the text form gives for it — a node its rule and
/// its token range, an error node what it expected and where, a token its rule and its
/// value.
// @lfy def/cli/main.lfy:main#main:main:506eab00d30b7bdabeedfaa49835ffb31dd39d992da62dd3f70f8d67b08034d2
pub(crate) fn node_json(tree: &Tree, node: &Node, depth: usize, out: &mut Vec<String>) {
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
pub(crate) fn node_lines(tree: &Tree, node: &Node, depth: usize, on: bool, out: &mut Vec<String>) {
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
pub(crate) fn tree_lines(tree: &Tree, path: &str, json: bool, on: bool) -> Vec<String> {
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
pub(crate) fn tree(invocation: &Invocation) -> ExitCode {
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
pub(crate) fn token_lines(tokens: &[Token], json: bool, on: bool) -> Vec<String> {
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
pub(crate) fn tokens(invocation: &Invocation) -> ExitCode {
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
