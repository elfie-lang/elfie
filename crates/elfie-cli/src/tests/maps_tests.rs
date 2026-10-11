// @lfy def/cli/main.lfy:main
//! Tests of the plan as a compile prints it and of the map files it keeps: the JSON lines,
//! the dry run, the moves, the maps an older project left behind, and the source a unit's
//! last accepted generation came from.

use super::*;
use crate::commands::{created_line, error_position, formatted_line, print_move, rule_name, token_lines, tree_lines};
use crate::compiling::{batch_line, legacy_targets, move_legacy_maps, move_text, unit_line};
use crate::running::{lowered_of, own_region_text, previous_source};

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
