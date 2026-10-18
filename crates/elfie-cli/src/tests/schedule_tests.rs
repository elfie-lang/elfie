// @lfy def/cli/main.lfy:main
//! Tests of the order units run in, what stops a compile or skips a unit, the locations a
//! command's output names, the unit gate and the final gate, and what an accepted unit's
//! foreign edits do to the units after it.

use super::*;
use crate::compiling::{is_stub_line, keep_kept_units};
use crate::copies::make_copy;
use crate::running::Run;
use crate::processes::{located, location_in, within};
use crate::running::Stop;
use crate::schedule::snapshot_of;

/// Three independent units and a compiler writing each one's output, so that they run one at
/// a time, in plan order.
// @lfy def/cli/main.lfy:main#main:main:de26823f65cede55489b74aa666da89098e28a63c56aff633a4c046e0170b4a1
fn three_units(fixture: &Fixture) -> &Fixture {
    one_target(fixture)
        .write("def/a.lfy", "d A {}\n")
        .write("def/b.lfy", "d B {}\n")
        .write("def/c.lfy", "d C {}\n")
        .write("compiler.sh", &per_unit_compiler(fixture))
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]))
}

/// Units run one at a time, in plan order, each once every unit before it is done.
// @lfy def/cli/main.lfy:main#main:main:de26823f65cede55489b74aa666da89098e28a63c56aff633a4c046e0170b4a1
// @lfy def/cli/main.lfy:main#main:main:d6af6f4d3977a8dd794735d5f4d494c25c55465c8422412f7c2a41ef0f2ce483
// @lfy def/cli/main.lfy:main#main:main:52dc0204d72665f32b60eef1095ce6a73d89469226840fd8606ff7d622cc4745
#[test]
fn units_run_one_at_a_time_in_plan_order() {
    let fixture = Fixture::new();
    three_units(&fixture);
    let (_, plan) = plan_with(&fixture.root, &[], &[], &[], &[]);
    let order: Vec<String> = plan.units.iter().map(|unit| format!("{}\n", unit.stem)).collect();
    // @lfy def/cli/main.lfy:main#main:main:de26823f65cede55489b74aa666da89098e28a63c56aff633a4c046e0170b4a1
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert_eq!(fixture.read("attempts.txt"), order.concat());
    // Every one of them was in the one change set the compile worked, which the root took.
    // @lfy def/cli/main.lfy:main#main:main:52dc0204d72665f32b60eef1095ce6a73d89469226840fd8606ff7d622cc4745
    for stem in ["a", "b", "c"] {
        assert!(fixture.root.join(format!("out/{stem}.rs")).is_file(), "{stem} did not reach the root");
    }
}

/// Without --continue a unit that stops stops the compile: no unit runs after it. With
/// --continue every unit depending on it is skipped and every other one still runs; either
/// way no final gate runs and the change set is kept.
// @lfy def/cli/main.lfy:main#main:main:c1e8fec574c59c5e1bad63913eed21885e232f0b8264e71cda5d800113f8f466
// @lfy def/cli/main.lfy:main#main:main:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
// @lfy def/cli/main.lfy:main#main:main:b298aad200f8b11abdc3d923f5d89a20461d9597ab75a2a1ecf41d1410a4150a
// @lfy def/cli/main.lfy:main#main:main:85a283dc1f225f02956fbed8a23688bb8b6f297fdc8bea08fed48d08fd0de9b5
// @lfy def/cli/main.lfy:main#main:main:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
#[test]
fn a_stopped_unit_stops_the_compile_or_skips_what_depends_on_it() {
    let fixture = Fixture::new();
    // A compiler blocked on b that writes every other unit's output.
    let blocking = noted(
        &fixture,
        "#!/bin/sh\ncat > /dev/null\necho \"$ELFIE_UNIT\" >> '<notes>/attempts.txt'\n\
         if [ \"$ELFIE_UNIT\" = b ]; then echo 'ELFIE: BLOCKED: no'; exit 0; fi\nmkdir -p \"$ELFIE_ROOT/out\"\n\
         name=$(echo \"$ELFIE_UNIT\" | tr 'a-z' 'A-Z')\n\
         printf '// @lfy def/%s.lfy:%s\\npub struct %s {}\\n' \"$ELFIE_UNIT\" \"$name\" \"$name\" > \"$ELFIE_ROOT/out/$ELFIE_UNIT.rs\"\n\
         echo 'ELFIE: DONE'\n",
    );
    one_target(&fixture)
        .write("def/b.lfy", "d B {}\n")
        .write("def/a.lfy", "use \"./b\";\nd A { $b = B; }\n")
        .write("def/c.lfy", "d C {}\n")
        .write("compiler.sh", &blocking)
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    let (_, plan) = plan_with(&fixture.root, &[], &[], &[], &[]);
    let place = |stem: &str| plan.units.iter().position(|unit| unit.stem == stem).unwrap();
    assert!(place("b") < place("a"), "a depends on b");
    // Without --continue nothing runs after b.
    // @lfy def/cli/main.lfy:main#main:main:c1e8fec574c59c5e1bad63913eed21885e232f0b8264e71cda5d800113f8f466
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    let ran: Vec<String> = fixture.read("attempts.txt").lines().map(str::to_string).collect();
    let after_b: Vec<&String> = ran.iter().skip_while(|stem| *stem != "b").skip(1).collect();
    assert!(after_b.is_empty(), "{ran:?}");
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/main.lfy:main#main:main:85a283dc1f225f02956fbed8a23688bb8b6f297fdc8bea08fed48d08fd0de9b5
    assert!(!log.contains("finalGate"), "{log}");
    assert!(ChangeSet::load(&fixture.root, "rust").is_some());
    // With --continue a is skipped, its state planned, and c runs.
    // @lfy def/cli/main.lfy:main#main:main:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
    // @lfy def/cli/main.lfy:main#main:main:b298aad200f8b11abdc3d923f5d89a20461d9597ab75a2a1ecf41d1410a4150a
    fixture.write("attempts.txt", "");
    assert_eq!(fixture.run(&["compile", "--continue"]), ExitCode::Problems.code());
    let ran = fixture.read("attempts.txt");
    assert!(ran.lines().any(|stem| stem == "c"), "{ran}");
    assert!(!ran.lines().any(|stem| stem == "a"), "{ran}");
    let kept = ChangeSet::load(&fixture.root, "rust").expect("the change set is kept");
    let state = |stem: &str| kept.units.iter().find(|unit| unit.stem == stem).map(|unit| unit.state);
    assert_eq!(state("a"), Some(UnitState::Planned));
    assert!(state("c").is_some_and(UnitState::is_done), "{kept:?}");
    let log = fixture.read("elfie-requests/compile.log");
    assert!(!log.contains("finalGate"), "{log}");
}

/// The location a line of a command's output names: the first text reading a path, a colon,
/// and a line number, the path read relative to the directory the command ran in when a file
/// is there, and relative to the unit's directory otherwise.
// @lfy def/cli/main.lfy:main#main:main:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
#[test]
fn a_line_names_the_first_path_and_line_it_reads() {
    // @lfy def/cli/main.lfy:main#main:main:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
    assert_eq!(location_in("  --> crates/a/src/a.rs:12:5"), Some(("crates/a/src/a.rs".to_string(), 12)));
    assert_eq!(location_in("warning: unused at src/b.rs:3"), Some(("src/b.rs".to_string(), 3)));
    assert_eq!(location_in("error[E0432]: unresolved import"), None);
    // A time of day reads no path.
    assert_eq!(location_in("at 12:30 in (src/c.rs:7)"), Some(("src/c.rs".to_string(), 7)));
    let fixture = Fixture::new();
    fixture.write("out/out.rs", "x\n").write("out/inner/a.rs", "y\n");
    let ran_in = fixture.root.join("out");
    // @lfy def/cli/main.lfy:main#main:main:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
    assert_eq!(located("inner/a.rs", &ran_in, &fixture.root), "out/inner/a.rs");
    assert_eq!(located("out/inner/a.rs", &ran_in, &fixture.root), "out/inner/a.rs");
    // A path under the output directory, or the output directory the root.
    // @lfy def/cli/main.lfy:main#main:main:20d39c165928d43ffa93da6970f91339f7c50e254aa218ea87fb80072d87f066
    assert!(within("out/a.rs", "out"));
    assert!(!within("outer/a.rs", "out"));
    assert!(within("a.rs", ""));
}

/// A unit's own lines are the lines of the files its maps name that lie in no region of
/// another unit's marker, as the map files in its directory give them.
// @lfy def/cli/main.lfy:main#main:main:6b6776d9051184c3c3b68bf73d7695e20b6854877e1970d17e5ef917f80dd7c0
#[test]
fn a_units_own_lines_leave_another_units_regions_out() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A {}\n").write("def/b.lfy", "d B {}\n").write(
        "out/shared.rs",
        "// @lfy def/a.lfy:A\npub struct A {}\n// @lfy def/b.lfy:B\npub struct B {}\n",
    );
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    let run = a_run(&fixture, &["compile"]);
    let a = run.plan.units.iter().position(|unit| unit.stem == "a").unwrap();
    let maps = run.maps_in(a, &fixture.root);
    // @lfy def/cli/main.lfy:main#main:main:6b6776d9051184c3c3b68bf73d7695e20b6854877e1970d17e5ef917f80dd7c0
    let own = run.own_lines(a, &maps, &fixture.root);
    let lines: Vec<usize> = own["out/shared.rs"].iter().copied().collect();
    assert_eq!(lines, vec![1, 2]);
}

/// The unit gate's lint fails it only through a line naming one of the unit's own lines,
/// and the problem names those lines.
// @lfy def/cli/main.lfy:main#main:main:3bac0db5561152245649fca44e6f2f4aa275ec221f2d4954e94046bd327e3cab
// @lfy def/cli/main.lfy:main#main:main:c5ad7b82a71dd3043b2097cdddcd5cd97253716022319656a0981d35b1fdb607
// @lfy def/cli/main.lfy:main#main:main:4502368c3b4408a12c9ea2de6677c21637ddd009b1433f9ca2537f0d00e249e0
#[test]
fn a_lint_warning_in_the_units_own_lines_fails_its_gate() {
    let fixture = Fixture::new();
    let roles = "trait lang extends targetLanguage: `Rust` {\n  .fileExtension = \"rs\";\n  .markerComment = \"//\";\n}\n\
                 trait flat extends targetLayout: `One file per unit` { }\n\
                 trait cargo extends targetEcosystem: `Crates` {\n  .ecosystem = \"cargo\";\n  @commands\n    \
                 .add({ operation = Operation.lint, line = \"echo 'warning: unused at a.rs:2:1'; exit 1\" });\n}\n";
    library(&fixture)
        .write("def/roles.lfy", roles)
        .write("def/targets.lfy", TARGETS)
        .write("def/a.lfy", "d A {}\n")
        .write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n");
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    let mut run = a_run(&fixture, &["compile"]);
    let maps = run.maps_in(0, &fixture.root);
    // @lfy def/cli/main.lfy:main#main:main:3bac0db5561152245649fca44e6f2f4aa275ec221f2d4954e94046bd327e3cab
    let problem = run.unit_gate(0, &maps, &fixture.root).unwrap_err();
    // @lfy def/cli/main.lfy:main#main:main:c5ad7b82a71dd3043b2097cdddcd5cd97253716022319656a0981d35b1fdb607
    assert_eq!(problem, "the lint command exited with 1\nwarning: unused at a.rs:2:1");
}

/// The roles of a target whose language gives a stub line or none, and whose build command
/// notes that it ran.
// @lfy def/cli/main.lfy:main#main:main:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
fn stub_roles(stub_line: &str) -> String {
    format!(
        "trait lang extends targetLanguage: `Rust` {{\n  .fileExtension = \"rs\";\n  .markerComment = \"//\";\n{stub_line}}}\n\
         trait flat extends targetLayout: `One file per unit` {{ }}\n\
         trait cargo extends targetEcosystem: `Crates` {{\n  .ecosystem = \"cargo\";\n  @commands\n    \
         .add({{ operation = Operation.build, line = \"echo built >> built.txt\" }});\n}}\n"
    )
}

/// A run of one unit `a` whose output is the text given, with a change set made for it.
// @lfy def/cli/main.lfy:main#main:main:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
fn a_gated_run(fixture: &Fixture, roles: &str, output: &str) -> (Run, Target, PathBuf) {
    library(fixture)
        .write("def/roles.lfy", roles)
        .write("def/targets.lfy", TARGETS)
        .write("def/a.lfy", "d A {}\n")
        .write("out/a.rs", output);
    let mut run = a_run(fixture, &["compile", "--no-verify"]);
    let target = run.program.workspace.targets[0].clone();
    let copy = make_copy(&fixture.root, &target).unwrap();
    run.sets.insert(
        "rust".to_string(),
        ChangeSet {
            target: "rust".to_string(),
            directory: copy.to_string_lossy().into_owned(),
            units: vec![ChangeSetUnit::planned("a", Reason::Fresh, "", "")],
            foreign_edits: Vec::new(),
        },
    );
    (run, target, copy)
}

/// The final gate counts every stub line of an output as a problem, and no line that only
/// mentions a stub, and runs no command while one is left; a unit not reviewed, when a
/// verifier is to review, is one problem naming it.
// @lfy def/cli/main.lfy:main#main:main:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
// @lfy def/cli/main.lfy:main#main:main:67a1f8d3c09f325ad74066024558e41cfb46dd0f03a0b40b1a053070d1333908
// @lfy def/cli/main.lfy:main#main:main:041f6b776f5a24240eda0d930f6f59f73477128b53f77574b05efcf1ee65d4d0
#[test]
fn the_final_gate_counts_every_stub_left() {
    let fixture = Fixture::new();
    let roles = stub_roles("  .stubLine = 'todo!(\"elfie stub: {{unit}}: {{reason}}\")';\n");
    let output = "// @lfy def/a.lfy:A\npub fn a() {\n    todo!(\"elfie stub: b: why\")\n}\n// says elfie stub: but is no stub line\n";
    let (mut run, target, copy) = a_gated_run(&fixture, &roles, output);
    // @lfy def/cli/main.lfy:main#main:main:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
    let problems = run.final_gate(&target);
    assert_eq!(problems[0], "out/a.rs:3: is a stub line");
    assert!(!copy.join("out/built.txt").exists(), "a command ran while a stub was left");
    // Without --no-verify and with no verifier named, a unit neither accepted nor reviewed is
    // a problem naming it.
    // @lfy def/cli/main.lfy:main#main:main:041f6b776f5a24240eda0d930f6f59f73477128b53f77574b05efcf1ee65d4d0
    assert_eq!(problems[1], "a is planned");
    assert_eq!(problems.len(), 2, "{problems:?}");
    // With the stub gone, the build runs in the copy.
    // @lfy def/cli/main.lfy:main#main:main:67a1f8d3c09f325ad74066024558e41cfb46dd0f03a0b40b1a053070d1333908
    fs::write(copy.join("out/a.rs"), "// @lfy def/a.lfy:A\npub fn a() {}\n").unwrap();
    run.sets.get_mut("rust").unwrap().units[0].state = UnitState::Accepted;
    assert!(run.final_gate(&target).is_empty());
    assert!(copy.join("out/built.txt").is_file(), "the build did not run in the copy");
}

/// With no stub line given by the target's language, no line is a stub line, and the build
/// runs in the copy.
// @lfy def/cli/main.lfy:main#main:main:05a98f1380573aab179458037c363fd33d4604fb69940c131e203ae327ee9388
#[test]
fn with_no_stub_line_no_line_is_one() {
    let fixture = Fixture::new();
    let output = "// @lfy def/a.lfy:A\npub fn a() {\n    todo!(\"elfie stub: b: why\")\n}\n";
    let (mut run, target, copy) = a_gated_run(&fixture, &stub_roles(""), output);
    run.sets.get_mut("rust").unwrap().units[0].state = UnitState::Accepted;
    // @lfy def/cli/main.lfy:main#main:main:05a98f1380573aab179458037c363fd33d4604fb69940c131e203ae327ee9388
    assert!(run.final_gate(&target).is_empty());
    assert!(copy.join("out/built.txt").is_file(), "the build did not run in the copy");
}

/// A command of the target that cannot be started, here because the directory it runs in is
/// gone, is a command that could not run: the code is failure, not problems.
// @lfy def/cli/main.lfy:main#main:main:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
#[test]
fn a_target_command_that_cannot_start_is_a_failure() {
    let fixture = Fixture::new();
    let (mut run, target, copy) = a_gated_run(&fixture, &stub_roles(""), "// @lfy def/a.lfy:A\npub fn a() {}\n");
    run.sets.get_mut("rust").unwrap().units[0].state = UnitState::Accepted;
    fs::remove_dir_all(copy.join("out")).unwrap();
    // @lfy def/cli/main.lfy:main#main:main:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
    let problems = run.final_gate(&target);
    assert!(problems[0].starts_with("the build command exited with -1"), "{problems:?}");
    assert_eq!(run.code, ExitCode::Failure);
}

/// A stub line is the whole line, white space around it aside, reading the target's stub line
/// with any text for the unit and the reason.
// @lfy def/cli/main.lfy:main#main:main:310411e96e27f58a43d127d3470e193be29741813413d0d700495a7648e6299d
#[test]
fn a_stub_line_reads_the_stub_line_with_any_unit_and_reason() {
    let stub = "todo!(\"elfie stub: {{unit}}: {{reason}}\")";
    // @lfy def/cli/main.lfy:main#main:main:310411e96e27f58a43d127d3470e193be29741813413d0d700495a7648e6299d
    assert!(is_stub_line(stub, "    todo!(\"elfie stub: cli/main: B: takes a number\")  "));
    assert!(is_stub_line(stub, "todo!(\"elfie stub: : \")"));
    assert!(!is_stub_line(stub, "let x = todo!(\"elfie stub: a: b\");"));
    assert!(!is_stub_line(stub, "// todo!(\"elfie stub: a: b\")"));
    assert!(!is_stub_line(stub, "todo!(\"elfie stub: a\")"));
    assert!(is_stub_line("panic!()", " panic!() "));
    assert!(!is_stub_line("panic!()", "panic!(1)"));
}

/// The files a unit that stops is put back to are those before its first run in the compile,
/// however often it runs again after that.
// @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
#[test]
fn a_unit_is_put_back_to_before_its_first_run() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A {}\n").write("out/a.rs", "// @lfy def/a.lfy:A\nfirst\n");
    let mut run = a_run(&fixture, &["compile"]);
    let root = fixture.root.clone();
    let first = run.first_snapshot(0, &root);
    fixture.write("out/a.rs", "// @lfy def/a.lfy:A\nsecond\n");
    // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
    assert_eq!(run.first_snapshot(0, &root), first);
    assert_eq!(first["out/a.rs"], b"// @lfy def/a.lfy:A\nfirst\n".to_vec());
}

/// A unit that stops without being accepted has the files of its output directory put back
/// as they were before its first run, the files added since removed, and what they held
/// written under `elfie-requests/<name>.rejected`.
// @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
#[test]
fn a_stopped_unit_puts_its_files_back() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A {}\n").write("out/a.rs", "// @lfy def/a.lfy:A\nold\n");
    let mut run = a_run(&fixture, &["compile", "--continue"]);
    let target = run.program.workspace.targets[0].clone();
    let copy = make_copy(&fixture.root, &target).unwrap();
    run.sets.insert(
        "rust".to_string(),
        ChangeSet {
            target: "rust".to_string(),
            directory: copy.to_string_lossy().into_owned(),
            units: vec![ChangeSetUnit::planned("a", Reason::Fresh, "", "")],
            foreign_edits: Vec::new(),
        },
    );
    let snapshot = snapshot_of(&target, &copy);
    fs::write(copy.join("out/a.rs"), "// @lfy def/a.lfy:A\nnew\n").unwrap();
    fs::write(copy.join("out/extra.rs"), "added\n").unwrap();
    // @lfy def/cli/main.lfy:main#main:main:2a92e13e1e56255b4cfa65e553c091ce8af723b6f9bb3156ecf05d84e8637a96
    run.stopped_unit(0, Stop::Rejected, &snapshot);
    assert_eq!(fs::read_to_string(copy.join("out/a.rs")).unwrap(), "// @lfy def/a.lfy:A\nold\n");
    assert!(!copy.join("out/extra.rs").exists());
    assert_eq!(fixture.read("elfie-requests/a.rejected/out/a.rs"), "// @lfy def/a.lfy:A\nnew\n");
    assert_eq!(fixture.read("elfie-requests/a.rejected/out/extra.rs"), "added\n");
    // Its state is rejected, written at once, and with --continue the compile goes on.
    // @lfy def/cli/main.lfy:main#main:main:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
    // @lfy def/cli/main.lfy:main#main:main:3d654e560f2e1144270b416e60c71b73f41429d32f691bba2f8f6fab0038bd7a
    assert_eq!(ChangeSet::load(&fixture.root, "rust").unwrap().units[0].state, UnitState::Rejected);
    assert!(!run.halted);
}

/// A foreign edit recorded for a unit not yet in the change set has it join the change set
/// at its place in plan order, as touched; one for a unit already accepted plans it again.
// @lfy def/cli/main.lfy:main#main:main:eeb6eb51ce2fa61105a263d0328d2874ebfc0bc3ced175501ee78c99422ed3ec
// @lfy def/cli/main.lfy:main#main:main:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
// @lfy def/cli/main.lfy:main#main:main:1effb63cfadc95b40e6ebdc615397f053aaa2c7015afe87022399dbfda5f1f7a
// @lfy def/cli/main.lfy:main#main:main:186f5bf02a1197f42b15d46a728b008732f742e22b0a3360bbff586dc9f0dcab
#[test]
fn a_foreign_edit_plans_its_owner() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/a.lfy", "d A {}\n")
        .write("def/b.lfy", "d B {}\n")
        .write("def/c.lfy", "d C {}\n")
        .write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n")
        .write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B {}\n")
        .write("out/c.rs", "// @lfy def/c.lfy:C\npub struct C {}\n");
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    let mut run = a_run(&fixture, &["compile", "--json"]);
    let place = |run: &Run, stem: &str| run.plan.units.iter().position(|unit| unit.stem == stem).unwrap();
    let (a, c) = (place(&run, "a"), place(&run, "c"));
    run.sets.insert(
        "rust".to_string(),
        ChangeSet {
            target: "rust".to_string(),
            directory: fixture.path(),
            units: vec![ChangeSetUnit::planned("a", Reason::Changed, "", "")],
            foreign_edits: Vec::new(),
        },
    );
    let edit = ForeignEdit {
        by: "a".to_string(),
        owner: "c".to_string(),
        path: "out/c.rs".to_string(),
        kind: ForeignEditKind::Fix,
        region: generation::parse_markers(&fixture.read("out/c.rs")).remove(0),
        before: vec!["pub struct C {}".to_string()],
        after: vec!["pub struct C(u8);".to_string()],
        note: "C holds a number".to_string(),
    };
    // @lfy def/cli/main.lfy:main#main:main:1effb63cfadc95b40e6ebdc615397f053aaa2c7015afe87022399dbfda5f1f7a
    run.accept_in_set(a, vec![edit]);
    let set = run.sets["rust"].clone();
    // @lfy def/cli/main.lfy:main#main:main:1effb63cfadc95b40e6ebdc615397f053aaa2c7015afe87022399dbfda5f1f7a
    assert_eq!(set.units[0].state, UnitState::Accepted);
    assert_eq!(set.units[0].edits, 1);
    assert_eq!(set.foreign_edits.len(), 1);
    // @lfy def/cli/main.lfy:main#main:main:eeb6eb51ce2fa61105a263d0328d2874ebfc0bc3ced175501ee78c99422ed3ec
    let stems: Vec<&str> = set.units.iter().map(|unit| unit.stem.as_str()).collect();
    assert_eq!(stems, vec!["a", "c"]);
    assert_eq!(set.units[1].reason, Reason::Touched);
    assert_eq!(set.units[1].state, UnitState::Planned);
    // The plan is made again with the edit, so c's reason is touched there too.
    // @lfy def/cli/main.lfy:main#main:main:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
    assert_eq!(run.plan.units[c].reason, Some(Reason::Touched));
    // An owner already accepted is planned again.
    // @lfy def/cli/main.lfy:main#main:main:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
    run.sets.get_mut("rust").unwrap().units[1].state = UnitState::Reviewed;
    let again = ForeignEdit { note: "once more".to_string(), ..run.sets["rust"].foreign_edits[0].clone() };
    run.accept_in_set(a, vec![again]);
    assert_eq!(run.sets["rust"].units[1].state, UnitState::Planned);
    assert_eq!(run.sets["rust"].foreign_edits.len(), 2);
}

/// An owner before the accepted unit in plan order, planned again by its foreign edit, is a
/// unit still to run before it, which the compile goes back to before any unit after it.
// @lfy def/cli/main.lfy:main#main:main:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
// @lfy def/cli/main.lfy:main#main:main:d6af6f4d3977a8dd794735d5f4d494c25c55465c8422412f7c2a41ef0f2ce483
#[test]
fn an_earlier_owner_planned_again_is_waited_for() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/a.lfy", "d A {}\n")
        .write("def/c.lfy", "d C {}\n")
        .write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n")
        .write("out/c.rs", "// @lfy def/c.lfy:C\npub struct C {}\n");
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    let mut run = a_run(&fixture, &["compile", "--json"]);
    let place = |run: &Run, stem: &str| run.plan.units.iter().position(|unit| unit.stem == stem).unwrap();
    let (a, c) = (place(&run, "a"), place(&run, "c"));
    assert!(a < c);
    let mut units = vec![ChangeSetUnit::planned("a", Reason::Changed, "", ""), ChangeSetUnit::planned("c", Reason::Changed, "", "")];
    units[0].state = UnitState::Reviewed;
    run.sets.insert(
        "rust".to_string(),
        ChangeSet { target: "rust".to_string(), directory: fixture.path(), units, foreign_edits: Vec::new() },
    );
    let before = run.waiting_before(c);
    assert!(before.is_empty(), "{before:?}");
    let edit = ForeignEdit {
        by: "c".to_string(),
        owner: "a".to_string(),
        path: "out/a.rs".to_string(),
        kind: ForeignEditKind::Fix,
        region: generation::parse_markers(&fixture.read("out/a.rs")).remove(0),
        before: vec!["pub struct A {}".to_string()],
        after: vec!["pub struct A(u8);".to_string()],
        note: "A holds a number".to_string(),
    };
    // @lfy def/cli/main.lfy:main#main:main:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
    run.accept_in_set(c, vec![edit]);
    assert_eq!(run.waiting_before(c).into_iter().collect::<Vec<_>>(), vec![a]);
}

/// A unit a kept change set accepted, with the hash and requirements it has now, is planned
/// with the reason the change set holds for it even when the plan gives it none, unless a
/// stem given requests it.
// @lfy def/cli/main.lfy:main#main:main:7ddc3a3c56212d70e81bbe0dc295510b33a966489a029c6a5c3c3a0907c4b225
#[test]
fn a_unit_a_kept_change_set_accepted_stays_planned() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A {}\n").write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n");
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    let (program, plan) = plan_with(&fixture.root, &generation::source_maps_of(&workspace::load(&fixture.root), None), &[], &[], &[]);
    assert_eq!(plan.units[0].reason, None);
    let hash = generation::source_hash(&plan.units[0].text);
    let requirements = generation::source_hash(&requirement_ids_of(&program, &plan.units[0]).join("\n"));
    let mut held = ChangeSetUnit::planned("a", Reason::Touched, &hash, &requirements);
    held.state = UnitState::Accepted;
    let sets: BTreeMap<String, ChangeSet> = [(
        "rust".to_string(),
        ChangeSet { target: "rust".to_string(), directory: fixture.path(), units: vec![held], foreign_edits: Vec::new() },
    )]
    .into();
    // @lfy def/cli/main.lfy:main#main:main:7ddc3a3c56212d70e81bbe0dc295510b33a966489a029c6a5c3c3a0907c4b225
    let mut kept = plan.clone();
    keep_kept_units(&program, &mut kept, &sets, &[]);
    assert_eq!(kept.units[0].reason, Some(Reason::Touched));
    // A stem given requests it, and the plan's reason stands.
    let mut requested = plan.clone();
    keep_kept_units(&program, &mut requested, &sets, &["a".to_string()]);
    assert_eq!(requested.units[0].reason, None);
    // Another hash: it changed since it was accepted, and nothing is kept.
    let mut changed = sets.clone();
    changed.get_mut("rust").unwrap().units[0].hash = "0".repeat(64);
    let mut again = plan.clone();
    keep_kept_units(&program, &mut again, &changed, &[]);
    assert_eq!(again.units[0].reason, None);
}
