// @lfy def/cli/main.lfy:main
//! Tests of the commands that need nothing but the program and the source files: help,
//! version, init, check, format, tree, and tokens.

use super::*;
use crate::commands::{print_no_target, print_stale, tree_lines};

/// A usage message is printed with the help text and the code is usage.
// @lfy def/cli/main.lfy:main
#[test]
fn a_usage_error_returns_the_usage_code() {
    assert_eq!(run(&["frobnicate".to_string()]), ExitCode::Usage.code());
}

/// Help prints every command of [`Command`], verify among them, with one line of
/// description and the global options, --json, --root, --target, --jobs, --no-verify,
/// --continue, --strict, and --color among them, and version prints the version; both return
/// success.
// @lfy def/cli/main.lfy:main#main:main:6f0073ceecb972e98aae5b1618d058d12e3264a00f6faa1982ea18e0ca49ac39
// @lfy def/cli/main.lfy:main#main:main:027934d626ca3e1baa3508e59ebe0c56b3748c5ce68a885ef8cb2c8c5f6efa53
#[test]
fn help_and_version_return_success() {
    let text = help_text(false);
    // @lfy def/cli/main.lfy:main#main:main:6f0073ceecb972e98aae5b1618d058d12e3264a00f6faa1982ea18e0ca49ac39
    for command in Command::ALL {
        assert!(text.contains(command.value()), "{text}");
        assert!(text.contains(command.description()), "{text}");
    }
    assert!(text.contains(Command::Verify.value()), "{text}");
    // @lfy def/cli/main.lfy:main#main:main:027934d626ca3e1baa3508e59ebe0c56b3748c5ce68a885ef8cb2c8c5f6efa53
    for option in [
        "--root",
        "--json",
        "--target",
        "--jobs",
        "--no-verify",
        "--continue",
        "--strict",
        "--color",
        "--help, -h",
        "--version",
    ] {
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
    // What check itself prints, read from a run of it as the executable runs.
    let (code, out, _) = fixture.printed(&["check", "--color=never"]);
    assert_eq!(code, ExitCode::Problems.code());
    // The standard library is part of the program, so only the project's own file is this
    // file's to report on: exactly one line names it, and it reads the place, the stage, the
    // severity, and the message.
    // @lfy def/cli/main.lfy:main
    let mine: Vec<&str> = out.lines().filter(|line| line.starts_with("def/a.lfy:")).collect();
    assert_eq!(mine, vec!["def/a.lfy:1:10: binder error: z is not declared in scope"], "{out}");
    assert_eq!(fixture.run(&["check"]), ExitCode::Problems.code());
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
    let (program, plan) = plan_with(&fixture.root, &[], &[], &[], &[]);
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
    // @lfy def/cli/main.lfy:main#main:main:3fbe8b3c0216de28ee03acabf6f8094637f58313fd15ab4b1a7ed940933d4151
    let (code, out, _) = fixture.printed(&["format", "--check", "--color=never"]);
    assert_eq!(code, ExitCode::Problems.code());
    assert!(out.lines().any(|line| line.trim() == "def/a.lfy"), "{out}");
    assert_eq!(fixture.read("def/a.lfy"), "const   x=1;\n");
    // Without --check the file is replaced by the standard layout.
    // @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["format"]), ExitCode::Success.code());
    assert_eq!(fixture.read("def/a.lfy"), "const x = 1;\n");
    // And formatting it again changes nothing.
    assert_eq!(fixture.run(&["format", "--check"]), ExitCode::Success.code());
}

/// With no files given, format takes every .lfy file under each source directory.
// @lfy def/cli/main.lfy:main#main:main:c7d7291e53c36dbf36962e6e8f80c4b5660d3e85a30bf7c345d861e32d8e7fc6
#[test]
fn format_takes_the_files_of_every_source_directory() {
    let fixture = Fixture::new();
    fixture
        .write("elfie.json", r#"{ "source": ["def", "src"] }"#)
        .write("def/a.lfy", "const   x=1;\n")
        .write("src/x/b.lfy", "const   y=2;\n")
        .write("other/c.lfy", "const   z=3;\n");
    assert_eq!(fixture.run(&["format"]), ExitCode::Success.code());
    assert_eq!(fixture.read("def/a.lfy"), "const x = 1;\n");
    assert_eq!(fixture.read("src/x/b.lfy"), "const y = 2;\n");
    assert_eq!(fixture.read("other/c.lfy"), "const   z=3;\n");
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
