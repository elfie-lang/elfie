// @lfy def/mcp/main.lfy:build
//! Tests of `build`, `test`, `lint`, and `format`: the target's own commands.

use super::*;
use crate::commands::{KEPT_LINES, STOPPED, Turn, kept_output};

/// The roles of a target carrying one command line per operation named.
// @lfy def/mcp/main.lfy:build#build:commandTool:f83af6a046e8e4f79a777c2d3c9b677a67d05faab6429155fbda5b26e27e4159
fn roles_with(commands: &str) -> String {
    format!(
        "trait lang extends targetLanguage: `Rust` {{\n  .fileExtension = \"rs\";\n  .markerComment = \"//\";\n}}\n\
         trait flat extends targetLayout: `One file per unit` {{ }}\n\
         trait cargo extends targetEcosystem: `Crates from crates.io` {{\n  .ecosystem = \"cargo\";\n  @commands{commands};\n}}\n"
    )
}

/// A project whose one target `rust` writes under `out` and carries the commands given.
/// The output directory is there, since a command runs in it.
fn project_with_commands(commands: &str) -> Fixture {
    let fixture = Fixture::new();
    fixture
        .with_rust_target()
        .write("def/roles.lfy", &roles_with(commands))
        .write("def/b.lfy", "d B {}\n")
        .write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
    fixture
}

/// The run one command tool answered with, read back from its JSON.
fn ran(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|error| panic!("the JSON of a run, not {text:?}: {error}"))
}

/// `elfie_build` runs the target's build command in the run's directory and answers with
/// the JSON of one run: its target, its operation, its line, whether it exited with 0,
/// its code, what it wrote, and how many lines that leaves out.
// @lfy def/mcp/main.lfy:build#build:build:cdf9c3c86a1fe606a631a001eb03e184eb38aa22635bbd147ffb9c962ed295d5
#[test]
fn build_runs_the_targets_build_command_and_says_how_it_ended() {
    let fixture = project_with_commands(
        "\n    .add({ operation = Operation.build, line = \"echo built; exit 3\" })",
    );
    let session = compiling(&fixture, &[]);
    // @lfy def/mcp/main.lfy:build#build:commandTool:394aef9de6082d12b6f9d58c8eaae738d8ab0f6aef3341b5fda12620fe6500a6
    let run = ran(&build(&session, None, None).unwrap());
    assert_eq!(run["target"], "rust");
    assert_eq!(run["operation"], "build");
    assert_eq!(run["line"], "echo built; exit 3");
    // @lfy def/mcp/main.lfy:build#build:commandTool:af9a234fc63d3a56cd02fc43d481612e9c05d96d41642a52c35b8aa861c9ebd9
    assert_eq!(run["succeeded"], serde_json::json!(false));
    assert_eq!(run["exitCode"], serde_json::json!(3));
    // @lfy def/mcp/main.lfy:build#build:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
    // @lfy def/mcp/main.lfy:build#build:commandTool:061a911a3cd22a9aa176a791d4d9f3a03405f6c4d5a5bad8f44fd652e56f6477
    assert_eq!(run["output"], "built\n");
    assert_eq!(run["omitted"], serde_json::json!(0));
    // A failing command is no error of the call: the run is the answer.
    // @lfy def/mcp/main.lfy:build#build:commandTool:2885c8e60a2e0374353fff71832add7b2dedc797a8f0f0bd2dacfc23d1f4d9fe
    let result = call(&session, "elfie_build", serde_json::json!({}));
    assert!(!result.is_error, "{}", result.text);
    // The target has no command for the other operations, so they are errors naming the
    // target and the operation, and nothing runs.
    // @lfy def/mcp/main.lfy:lint#lint:commandTool:db94076681d45420de07709b25f27480d746751609aa1bb42a77c77d453001eb
    let error = lint(&session, None, None).unwrap_err();
    assert!(error.contains("rust") && error.contains("lint"), "{error}");
    // @lfy def/mcp/main.lfy:format#format:commandTool:a09c5e43d4c38d2e3196576c80db3cf1cb918134842760d0c7e7aa134b552a6b
    let error = format_check(&session, None).unwrap_err();
    assert!(error.contains("format"), "{error}");
    // A target that is not one of the workspace's is an error naming the known targets.
    // @lfy def/mcp/main.lfy:build#build:commandTool:1f6aa9de92f8ca8b182ac8744b6bb86bfe4ef0e4fbe7aa6089dd3115896b773a
    let error = build(&session, None, Some("go")).unwrap_err();
    assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
    // The session's unit has one target, so naming it is allowed and naming another is
    // not; this workspace has only `rust`, so the error above covers the second.
    // @lfy def/mcp/main.lfy:build#build:commandTool:8ffd4f1c8964d044d3a2ad994cc592a42773205e5ab265fbba1c238e40c7390c
    // @lfy def/mcp/main.lfy:build#build:commandTool:dd69dd60edc37a9e66b24de9f8fcdeb8cbe86944d11c3b6047dcd05fe8e8c442
    let named = ran(&build(&session, None, Some("rust")).unwrap());
    assert_eq!(named["target"], "rust");
    // With no binding at all the only target of the workspace is the one.
    // @lfy def/mcp/main.lfy:build#build:commandTool:3afd64b040778d28cf00218d59ef25293e4891a2edb10ea0e5b765f3321f7c85
    let loose = fixture.session();
    let only = ran(&build(&loose, None, None).unwrap());
    assert_eq!(only["target"], "rust");
}

/// `elfie_test` gives the test command the words that narrow which tests run, as `TESTS`,
/// and the empty text when it is left out; `SCOPE` is set the same way.
// @lfy def/mcp/main.lfy:test#test:test:88e4049c194be4a5e25c007f182a5dc5b87c5c3c344dcf6e101766888653dea9
// @lfy def/mcp/main.lfy:test#test:test:10df90472b4c8eda107cfe8640548e69895000106be853750ef9e10d539bce1e
#[test]
fn test_narrows_which_tests_run_by_a_filter_and_a_scope() {
    let fixture = project_with_commands(
        "\n    .add({ operation = Operation.test, line = 'echo \"[$TESTS]\"' })\n    .add({ operation = Operation.lint, line = 'echo \"[$SCOPE]\"' })",
    );
    let session = compiling(&fixture, &[]);
    // @lfy def/mcp/main.lfy:test#test:test:c8bf810d04779ce26830e5c43f290fd753346658b3b40ff992ab1cdedef32174
    let run = ran(&test(&session, Some("parse lex"), None, None, false).unwrap());
    assert_eq!(run["succeeded"], serde_json::json!(true));
    assert_eq!(run["output"], "[parse lex]\n");
    // @lfy def/mcp/main.lfy:test#test:test:c7fb9e74d7bb66ee2c63c58681b14d00dc8236468fec85e845bd8aed14276ae6
    let run = ran(&test(&session, None, None, None, false).unwrap());
    assert_eq!(run["output"], "[]\n");
    // `SCOPE` is the scope, or the empty text when it is left out.
    // @lfy def/mcp/main.lfy:lint#lint:lint:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
    // @lfy def/mcp/main.lfy:test#test:test:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
    // @lfy def/mcp/main.lfy:build#build:build:caecccd1cd176ce8fc2634b1001e015b48012d66788d863fb994735ca793fab5
    let run = ran(&lint(&session, Some("elfie-core"), None).unwrap());
    assert_eq!(run["output"], "[elfie-core]\n");
    let run = ran(&lint(&session, None, None).unwrap());
    assert_eq!(run["output"], "[]\n");
}

/// `elfie_test` sets `ALLOW_STUBS` to 1 when the caller asks for stubs to be allowed, and
/// to the empty text when it does not.
// @lfy def/mcp/main.lfy:test#test:test:be3a6e782a4bd1cabce9ac8286b647e61b06390c366369a426b89e2af12baf94
#[test]
fn test_allows_stubs_only_when_asked() {
    let fixture = project_with_commands("\n    .add({ operation = Operation.test, line = 'echo \"[$ALLOW_STUBS]\"' })");
    let session = compiling(&fixture, &[]);
    // @lfy def/mcp/main.lfy:test#test:test:4c1e4df81b056422995d5add58b4a2ab7b8a969ed259ed1ff3b611fc46a0b931
    let run = ran(&test(&session, None, None, None, true).unwrap());
    assert_eq!(run["output"], "[1]\n");
    // @lfy def/mcp/main.lfy:test#test:test:4e7c2e749ecf5d032bf66649d6e5d9daf1357b47d90e6139ef1383be59c6791f
    let run = ran(&test(&session, None, None, None, false).unwrap());
    assert_eq!(run["output"], "[]\n");
    // A call that leaves it out gets the strict outcome; one that gives it reaches the fn
    // with it.
    let result = call(&session, "elfie_test", serde_json::json!({}));
    assert_eq!(ran(&result.text)["output"], "[]\n");
    let result = call(&session, "elfie_test", serde_json::json!({ "allowStubs": true }));
    assert_eq!(ran(&result.text)["output"], "[1]\n");
}

/// A command runs in the target's output directory under the run's directory, with
/// `RUN_SCRIPT` set to the target's script runner, and the calls for one directory run
/// their commands one after the other.
// @lfy def/mcp/main.lfy:build#build:commandTool:9b8ea5c13487cc9695be55b65dee63a54d131b4325fed5195ed378368f8479de
// @lfy def/mcp/main.lfy:build#build:commandTool:12878393e582a75489c9dea1e06ef0237a6e04ef7784802355edb6ae22e9815b
#[test]
fn a_command_runs_in_the_output_directory_with_the_script_runner_set() {
    let fixture = project_with_commands(
        "\n    .add({ operation = Operation.build, line = 'pwd; echo \"[$RUN_SCRIPT]\"' })",
    );
    let session = compiling(&fixture, &[]);
    // @lfy def/mcp/main.lfy:build#build:commandTool:9b8ea5c13487cc9695be55b65dee63a54d131b4325fed5195ed378368f8479de
    let run = ran(&build(&session, None, None).unwrap());
    let output = run["output"].as_str().unwrap_or_default();
    let shown = std::fs::canonicalize(fixture.root.join("out")).unwrap();
    assert!(output.contains(&shown.to_string_lossy().to_string()), "{output}");
    // The target's ecosystem sets no script runner, so `RUN_SCRIPT` is the empty text.
    // @lfy def/mcp/main.lfy:build#build:commandTool:12878393e582a75489c9dea1e06ef0237a6e04ef7784802355edb6ae22e9815b
    assert!(output.contains("[]"), "{output}");
}

/// What a command wrote is kept short: at most two hundred lines from the end, the first
/// hundred earlier lines that read as a failure or follow one, a line counting what is
/// left out, and no line longer than five hundred characters.
// @lfy def/mcp/main.lfy:build#build:commandTool:5cb971efd556449109eeb23f4d811571b6874a2274b251355982e35590d77441
// @lfy def/mcp/main.lfy:build#build:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
// @lfy def/mcp/main.lfy:build#build:commandTool:7a7e7f217024616471823763ae368e16d259097c0caecb49b0e9e3b404db0ca5
// @lfy def/mcp/main.lfy:build#build:commandTool:f6efadbb442d048cef8053d19a026c18b7dbffde5a2b0015a6047743150dc9f3
// @lfy def/mcp/main.lfy:build#build:commandTool:0078fc9e2f6e0d5f31f5e4ca2228f246ffd00ecd0927004d85db9ceed18ed01b
// @lfy def/mcp/main.lfy:build#build:commandTool:a9b744b3a270971d0bd9ce3b4335190df2715b2f07ae02df1b2548deca9fe114
#[test]
fn what_a_command_wrote_is_kept_short() {
    // At most two hundred lines: every line is kept and nothing is omitted.
    // @lfy def/mcp/main.lfy:build#build:commandTool:b3cf05e26d7d3b687e7b0b4028358dba9a481d54a4bddde0f8e7ab00f9743fb7
    let few: Vec<String> = (0..KEPT_LINES).map(|index| format!("line {index}")).collect();
    let (output, omitted) = kept_output(&few, false);
    assert_eq!(omitted, 0);
    assert_eq!(output.lines().count(), KEPT_LINES);
    // @lfy def/mcp/main.lfy:build#build:commandTool:061a911a3cd22a9aa176a791d4d9f3a03405f6c4d5a5bad8f44fd652e56f6477
    assert!(output.ends_with('\n'), "{output}");
    // More than two hundred: the kept earlier lines, then what was left out, then the
    // last two hundred.
    let mut many: Vec<String> = Vec::new();
    many.push("an Error happened".to_string());
    many.push("and this followed it".to_string());
    for index in 0..KEPT_LINES + 20 {
        many.push(format!("quiet {index}"));
    }
    let (output, omitted) = kept_output(&many, false);
    let lines: Vec<&str> = output.lines().collect();
    // @lfy def/mcp/main.lfy:build#build:commandTool:63be0a2d4357c8c544999776293adaded56a3f4a94b5c8ad25682f08f6ac70a6
    assert_eq!(lines[0], "an Error happened");
    assert_eq!(lines[1], "and this followed it");
    // @lfy def/mcp/main.lfy:build#build:commandTool:5cb971efd556449109eeb23f4d811571b6874a2274b251355982e35590d77441
    // @lfy def/mcp/main.lfy:build#build:commandTool:7a7e7f217024616471823763ae368e16d259097c0caecb49b0e9e3b404db0ca5
    assert_eq!(omitted, many.len() - KEPT_LINES - 2);
    assert_eq!(lines[2], format!("[elfie: {omitted} lines omitted]"));
    assert_eq!(lines.len(), 2 + 1 + KEPT_LINES);
    // @lfy def/mcp/main.lfy:build#build:commandTool:a9b744b3a270971d0bd9ce3b4335190df2715b2f07ae02df1b2548deca9fe114
    assert_eq!(lines[lines.len() - 1], &format!("quiet {}", KEPT_LINES + 19));
    // A line the command wrote and the output keeps is cut short at five hundred
    // characters; a line elfie adds is never shortened.
    // @lfy def/mcp/main.lfy:build#build:commandTool:f6efadbb442d048cef8053d19a026c18b7dbffde5a2b0015a6047743150dc9f3
    let wide = vec![format!("warning: {}", "x".repeat(KEPT_WIDTH))];
    let (output, _) = kept_output(&wide, true);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0].chars().count(), KEPT_WIDTH + 1);
    assert!(lines[0].ends_with(CUT), "{output}");
    // A command that was stopped says so after the lines kept as for any run.
    // @lfy def/mcp/main.lfy:build#build:commandTool:1ec8e462d2638913d1d6b8cd5b590f00751d033f0891e5ca220c79cd71a87001
    // @lfy def/mcp/main.lfy:build#build:commandTool:0078fc9e2f6e0d5f31f5e4ca2228f246ffd00ecd0927004d85db9ceed18ed01b
    assert_eq!(lines[1], STOPPED);
}

/// The calls of command tools for one directory run their commands one after the other,
/// in the order they arrived; two calls for different directories wait for neither.
// @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
// @lfy def/mcp/main.lfy:host#host:host:b09cfad6442aec8ac4faf8e13385b2973136e4d0ac08ec20419f15750ea59366
#[test]
fn the_calls_for_one_directory_run_their_commands_in_turn() {
    let one = Path::new("/elfie/one");
    let other = Path::new("/elfie/other");
    let first = Turn::take(one);
    let second = Turn::take(one);
    let elsewhere = Turn::take(other);
    // The first in the queue runs at once, and so does one for another directory.
    // @lfy def/mcp/main.lfy:host#host:host:b09cfad6442aec8ac4faf8e13385b2973136e4d0ac08ec20419f15750ea59366
    first.wait();
    elsewhere.wait();
    // The second waits for the first to end, and runs as soon as it has.
    // @lfy def/mcp/main.lfy:build#build:commandTool:55ee18e11aaf994de7de40b0727c7ff09d9b5230a39f4ff4070a8e0b0108478a
    let waiting = std::thread::spawn(move || {
        second.wait();
        true
    });
    std::thread::sleep(Duration::from_millis(20));
    assert!(!waiting.is_finished());
    drop(first);
    assert!(waiting.join().unwrap_or(false));
    drop(elsewhere);
}
