// @lfy def/cli/compile.lfy:compile
//! Tests of the order units start and merge in, the copy each runs in, what stops a compile or
//! skips a unit, the locations a command's output names, the unit gate and the final gate,
//! what a merge writes or undoes, and what a merged unit's foreign edits do to the units
//! after it.

use super::*;
use crate::compiling::{is_stub_line, keep_kept_units};
use crate::copies::{UnitCopy, make_copy, make_start, make_unit_copy, own_contents, start_of, unit_copy_of};
use crate::processes::{located, location_in, within};
use crate::running::{Run, Stop};

/// Three independent units and a compiler writing each one's output.
// @lfy def/cli/compile.lfy:compile#compile:compile:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
fn three_units(fixture: &Fixture) -> &Fixture {
    one_target(fixture)
        .write("def/a.lfy", "d A {}\n")
        .write("def/b.lfy", "d B {}\n")
        .write("def/c.lfy", "d C {}\n")
        .write("compiler.sh", &per_unit_compiler(fixture))
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]))
}

/// With one unit allowed to run at a time, the earliest ready unit in plan order starts
/// first, so the units run in plan order; every one of them merges into the one change set,
/// which the root takes.
// @lfy def/cli/compile.lfy:compile#compile:compile:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
// @lfy def/cli/compile.lfy:compile#compile:compile:7ca8d55384b2971cfabab5ffdce367c3faadb58b56ff385b0a2860676eae5360
// @lfy def/cli/compile.lfy:compile#compile:compile:52dc0204d72665f32b60eef1095ce6a73d89469226840fd8606ff7d622cc4745
#[test]
fn with_one_job_units_run_in_plan_order() {
    let fixture = Fixture::new();
    three_units(&fixture);
    let (_, plan) = plan_with(&fixture.root, &[], &[], &[], &[]);
    let order: Vec<String> = plan.units.iter().map(|unit| format!("{}\n", unit.stem)).collect();
    // @lfy def/cli/compile.lfy:compile#compile:compile:7ca8d55384b2971cfabab5ffdce367c3faadb58b56ff385b0a2860676eae5360
    assert_eq!(fixture.run(&["compile", "--jobs", "1"]), ExitCode::Success.code());
    assert_eq!(fixture.read("attempts.txt"), order.concat());
    // At most one ran at once: every started line says so.
    // @lfy def/cli/compile.lfy:compile#compile:compile:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
    let log = fixture.read("elfie-requests/compile.log");
    let started: Vec<&str> = log.lines().filter(|line| line.contains("▸ started")).collect();
    assert_eq!(started.len(), 3, "{log}");
    assert!(started.iter().all(|line| line.ends_with("1 of 1 running")), "{log}");
    // Every one of them was merged into the one change set the compile worked, which the root
    // took.
    // @lfy def/cli/compile.lfy:compile#compile:compile:52dc0204d72665f32b60eef1095ce6a73d89469226840fd8606ff7d622cc4745
    for stem in ["a", "b", "c"] {
        assert!(fixture.root.join(format!("out/{stem}.rs")).is_file(), "{stem} did not reach the root");
    }
}

/// With the default, up to four units run at once: three units that wait for nothing all
/// start before any of them is done, and they merge in plan order.
// @lfy def/cli/compile.lfy:compile#compile:compile:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
// @lfy def/cli/compile.lfy:compile#compile:compile:932f229a081364bb85c06954c07796f035ea0cd10a0285dc8a983a60bbe1fcc8
// @lfy def/cli/compile.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
// @lfy def/cli/compile.lfy:compile#compile:compile:77eb0c2c3b5559ce6700a2a65f66921a86b34f2759727cb081ba2ffcc3450fa0
#[test]
fn units_that_wait_for_nothing_run_at_once_and_merge_in_order() {
    let fixture = Fixture::new();
    three_units(&fixture);
    let (_, plan) = plan_with(&fixture.root, &[], &[], &[], &[]);
    let order: Vec<&str> = plan.units.iter().map(|unit| unit.stem.as_str()).collect();
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/compile.lfy:compile#compile:compile:932f229a081364bb85c06954c07796f035ea0cd10a0285dc8a983a60bbe1fcc8
    let started: Vec<&str> = log.lines().filter(|line| line.contains("▸ started")).collect();
    assert!(started[0].ends_with("1 of 4 running") && started[2].ends_with("3 of 4 running"), "{log}");
    // @lfy def/cli/compile.lfy:compile#compile:compile:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
    let merged: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("✓ merged"))
        .filter_map(|line| order.iter().copied().find(|stem| line.contains(&format!("  {stem}  "))))
        .collect();
    assert_eq!(merged, order, "{log}");
    // @lfy def/cli/compile.lfy:compile#compile:compile:77eb0c2c3b5559ce6700a2a65f66921a86b34f2759727cb081ba2ffcc3450fa0
    assert!(log.lines().any(|line| line.contains("✓ merged") && line.ends_with("2 files")), "{log}");
    // The only building lines are the two of each unit's own gate.
    assert_eq!(log.lines().filter(|line| line.contains("▸ building")).count(), 6, "{log}");
}

/// Without --continue a unit that stops stops the compile: no unit starts after it. With
/// --continue every unit depending on it is skipped and every other one still runs and
/// merges; either way no final gate runs and the change set is kept.
// @lfy def/cli/compile.lfy:compile#compile:compile:9051d3b21e94dd454bcd1cc00024dba85e18ea05da69961602354cc8d1b436bc
// @lfy def/cli/compile.lfy:compile#compile:compile:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
// @lfy def/cli/compile.lfy:compile#compile:compile:89552d50e7102f22cda0a0d893e8e711ee7287db9662f10df24991eada8a1d3e
// @lfy def/cli/compile.lfy:compile#compile:compile:85a283dc1f225f02956fbed8a23688bb8b6f297fdc8bea08fed48d08fd0de9b5
// @lfy def/cli/compile.lfy:compile#compile:compile:51b22c76c1410a187c0f287178a9ecb28048b54a983754ad873fa703eb7809a3
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
    // Without --continue nothing starts after b, one unit running at a time.
    // @lfy def/cli/compile.lfy:compile#compile:compile:9051d3b21e94dd454bcd1cc00024dba85e18ea05da69961602354cc8d1b436bc
    assert_eq!(fixture.run(&["compile", "--jobs", "1"]), ExitCode::Problems.code());
    let ran: Vec<String> = fixture.read("attempts.txt").lines().map(str::to_string).collect();
    let after_b: Vec<&String> = ran.iter().skip_while(|stem| *stem != "b").skip(1).collect();
    assert!(after_b.is_empty(), "{ran:?}");
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/compile.lfy:compile#compile:compile:85a283dc1f225f02956fbed8a23688bb8b6f297fdc8bea08fed48d08fd0de9b5
    assert!(!log.contains("finalGate"), "{log}");
    assert!(ChangeSet::load(&fixture.root, "rust").is_some());
    // With --continue a is skipped, its state planned, and c runs and merges.
    // @lfy def/cli/compile.lfy:compile#compile:compile:983a29cf93655951616c5c562ff1c6178966fa0f7acb433740f541446615af08
    // @lfy def/cli/compile.lfy:compile#compile:compile:89552d50e7102f22cda0a0d893e8e711ee7287db9662f10df24991eada8a1d3e
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
// @lfy def/cli/compile.lfy:compile#compile:compile:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
#[test]
fn a_line_names_the_first_path_and_line_it_reads() {
    // @lfy def/cli/compile.lfy:compile#compile:compile:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
    assert_eq!(location_in("  --> crates/a/src/a.rs:12:5"), Some(("crates/a/src/a.rs".to_string(), 12)));
    assert_eq!(location_in("warning: unused at src/b.rs:3"), Some(("src/b.rs".to_string(), 3)));
    assert_eq!(location_in("error[E0432]: unresolved import"), None);
    // A time of day reads no path.
    assert_eq!(location_in("at 12:30 in (src/c.rs:7)"), Some(("src/c.rs".to_string(), 7)));
    let fixture = Fixture::new();
    fixture.write("out/out.rs", "x\n").write("out/inner/a.rs", "y\n");
    let ran_in = fixture.root.join("out");
    // @lfy def/cli/compile.lfy:compile#compile:compile:8ff734425428618669a4f137ee0c85c9936237068dab48c69ebdcab1c20cfc15
    assert_eq!(located("inner/a.rs", &ran_in, &fixture.root), "out/inner/a.rs");
    assert_eq!(located("out/inner/a.rs", &ran_in, &fixture.root), "out/inner/a.rs");
    // A path under the output directory, or the output directory the root.
    // @lfy def/cli/compile.lfy:compile#compile:compile:20d39c165928d43ffa93da6970f91339f7c50e254aa218ea87fb80072d87f066
    assert!(within("out/a.rs", "out"));
    assert!(!within("outer/a.rs", "out"));
    assert!(within("a.rs", ""));
}

/// A unit's own lines are the lines of the files its maps name that lie in no region of
/// another unit's marker, as the map files in its directory give them.
// @lfy def/cli/compile.lfy:compile#compile:compile:6b6776d9051184c3c3b68bf73d7695e20b6854877e1970d17e5ef917f80dd7c0
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
    // @lfy def/cli/compile.lfy:compile#compile:compile:6b6776d9051184c3c3b68bf73d7695e20b6854877e1970d17e5ef917f80dd7c0
    let own = run.own_lines(a, &maps, &fixture.root);
    let lines: Vec<usize> = own["out/shared.rs"].iter().copied().collect();
    assert_eq!(lines, vec![1, 2]);
}

/// The unit gate's lint fails it only through a line naming one of the unit's own lines,
/// and the problem names those lines.
// @lfy def/cli/compile.lfy:compile#compile:compile:3bac0db5561152245649fca44e6f2f4aa275ec221f2d4954e94046bd327e3cab
// @lfy def/cli/compile.lfy:compile#compile:compile:c5ad7b82a71dd3043b2097cdddcd5cd97253716022319656a0981d35b1fdb607
// @lfy def/cli/compile.lfy:compile#compile:compile:4502368c3b4408a12c9ea2de6677c21637ddd009b1433f9ca2537f0d00e249e0
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
    // @lfy def/cli/compile.lfy:compile#compile:compile:3bac0db5561152245649fca44e6f2f4aa275ec221f2d4954e94046bd327e3cab
    let problem = run.unit_gate(0, &maps, &fixture.root).unwrap_err();
    // @lfy def/cli/compile.lfy:compile#compile:compile:c5ad7b82a71dd3043b2097cdddcd5cd97253716022319656a0981d35b1fdb607
    assert_eq!(problem, "the lint command exited with 1\nwarning: unused at a.rs:2:1");
}

/// The roles of a target whose language gives a stub line or none, and whose build command
/// notes that it ran.
// @lfy def/cli/compile.lfy:compile#compile:compile:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
fn stub_roles(stub_line: &str) -> String {
    format!(
        "trait lang extends targetLanguage: `Rust` {{\n  .fileExtension = \"rs\";\n  .markerComment = \"//\";\n{stub_line}}}\n\
         trait flat extends targetLayout: `One file per unit` {{ }}\n\
         trait cargo extends targetEcosystem: `Crates` {{\n  .ecosystem = \"cargo\";\n  @commands\n    \
         .add({{ operation = Operation.build, line = \"echo built >> built.txt\" }});\n}}\n"
    )
}

/// A run of one unit `a` whose output is the text given, with a change set made for it.
// @lfy def/cli/compile.lfy:compile#compile:compile:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
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
// @lfy def/cli/compile.lfy:compile#compile:compile:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
// @lfy def/cli/compile.lfy:compile#compile:compile:67a1f8d3c09f325ad74066024558e41cfb46dd0f03a0b40b1a053070d1333908
// @lfy def/cli/compile.lfy:compile#compile:compile:041f6b776f5a24240eda0d930f6f59f73477128b53f77574b05efcf1ee65d4d0
#[test]
fn the_final_gate_counts_every_stub_left() {
    let fixture = Fixture::new();
    let roles = stub_roles("  .stubLine = 'todo!(\"elfie stub: {{unit}}: {{reason}}\")';\n");
    let output = "// @lfy def/a.lfy:A\npub fn a() {\n    todo!(\"elfie stub: b: why\")\n}\n// says elfie stub: but is no stub line\n";
    let (mut run, target, copy) = a_gated_run(&fixture, &roles, output);
    // @lfy def/cli/compile.lfy:compile#compile:compile:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
    let problems = run.final_gate(&target);
    assert_eq!(problems[0], "out/a.rs:3: is a stub line");
    assert!(!copy.join("out/built.txt").exists(), "a command ran while a stub was left");
    // Without --no-verify and with no verifier named, a unit neither accepted nor reviewed is
    // a problem naming it.
    // @lfy def/cli/compile.lfy:compile#compile:compile:041f6b776f5a24240eda0d930f6f59f73477128b53f77574b05efcf1ee65d4d0
    assert_eq!(problems[1], "a is planned");
    assert_eq!(problems.len(), 2, "{problems:?}");
    // With the stub gone, the build runs in the copy.
    // @lfy def/cli/compile.lfy:compile#compile:compile:67a1f8d3c09f325ad74066024558e41cfb46dd0f03a0b40b1a053070d1333908
    fs::write(copy.join("out/a.rs"), "// @lfy def/a.lfy:A\npub fn a() {}\n").unwrap();
    run.sets.get_mut("rust").unwrap().units[0].state = UnitState::Accepted;
    assert!(run.final_gate(&target).is_empty());
    assert!(copy.join("out/built.txt").is_file(), "the build did not run in the copy");
}

/// With no stub line given by the target's language, no line is a stub line, and the build
/// runs in the copy.
// @lfy def/cli/compile.lfy:compile#compile:compile:05a98f1380573aab179458037c363fd33d4604fb69940c131e203ae327ee9388
#[test]
fn with_no_stub_line_no_line_is_one() {
    let fixture = Fixture::new();
    let output = "// @lfy def/a.lfy:A\npub fn a() {\n    todo!(\"elfie stub: b: why\")\n}\n";
    let (mut run, target, copy) = a_gated_run(&fixture, &stub_roles(""), output);
    run.sets.get_mut("rust").unwrap().units[0].state = UnitState::Accepted;
    // @lfy def/cli/compile.lfy:compile#compile:compile:05a98f1380573aab179458037c363fd33d4604fb69940c131e203ae327ee9388
    assert!(run.final_gate(&target).is_empty());
    assert!(copy.join("out/built.txt").is_file(), "the build did not run in the copy");
}

/// A command of the target that cannot be started, here because the directory it runs in is
/// gone, is a command that could not run: the code is failure, not problems.
// @lfy def/cli/compile.lfy:compile#compile:compile:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
#[test]
fn a_target_command_that_cannot_start_is_a_failure() {
    let fixture = Fixture::new();
    let (mut run, target, copy) = a_gated_run(&fixture, &stub_roles(""), "// @lfy def/a.lfy:A\npub fn a() {}\n");
    run.sets.get_mut("rust").unwrap().units[0].state = UnitState::Accepted;
    fs::remove_dir_all(copy.join("out")).unwrap();
    // @lfy def/cli/compile.lfy:compile#compile:compile:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
    let problems = run.final_gate(&target);
    assert!(problems[0].starts_with("the build command exited with -1"), "{problems:?}");
    assert_eq!(run.code, ExitCode::Failure);
}

/// A stub line is the whole line, white space around it aside, reading the target's stub line
/// with any text for the unit and the reason.
// @lfy def/cli/compile.lfy:compile#compile:compile:310411e96e27f58a43d127d3470e193be29741813413d0d700495a7648e6299d
#[test]
fn a_stub_line_reads_the_stub_line_with_any_unit_and_reason() {
    let stub = "todo!(\"elfie stub: {{unit}}: {{reason}}\")";
    // @lfy def/cli/compile.lfy:compile#compile:compile:310411e96e27f58a43d127d3470e193be29741813413d0d700495a7648e6299d
    assert!(is_stub_line(stub, "    todo!(\"elfie stub: cli/main: B: takes a number\")  "));
    assert!(is_stub_line(stub, "todo!(\"elfie stub: : \")"));
    assert!(!is_stub_line(stub, "let x = todo!(\"elfie stub: a: b\");"));
    assert!(!is_stub_line(stub, "// todo!(\"elfie stub: a: b\")"));
    assert!(!is_stub_line(stub, "todo!(\"elfie stub: a\")"));
    assert!(is_stub_line("panic!()", " panic!() "));
    assert!(!is_stub_line("panic!()", "panic!(1)"));
}

/// A run of a project whose units are `a` and `c`, with a change set made for them whose start
/// is made, each unit planned in it, and the copy and the target given back.
// @lfy def/cli/compile.lfy:compile#compile:compile:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
fn a_set_of_two(fixture: &Fixture, roles: &str, arguments: &[&str]) -> (Run, Target, PathBuf) {
    library(fixture)
        .write("def/roles.lfy", roles)
        .write("def/targets.lfy", TARGETS)
        .write("def/a.lfy", "d A {}\n")
        .write("def/c.lfy", "d C {}\n")
        .write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n")
        .write("out/c.rs", "// @lfy def/c.lfy:C\npub struct C {}\n");
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    let mut run = a_run(fixture, arguments);
    let target = run.program.workspace.targets[0].clone();
    let copy = make_copy(&fixture.root, &target).unwrap();
    make_start(&fixture.root, &copy, &target).unwrap();
    run.sets.insert(
        "rust".to_string(),
        ChangeSet {
            target: "rust".to_string(),
            directory: copy.to_string_lossy().into_owned(),
            units: vec![
                ChangeSetUnit::planned("a", Reason::Changed, "", ""),
                ChangeSetUnit::planned("c", Reason::Changed, "", ""),
            ],
            foreign_edits: Vec::new(),
        },
    );
    (run, target, copy)
}

/// A unit's copy of the compile's first plan holds every file a new copy would hold, the
/// change set's own files as its start holds them, and the merges of the units it waits for
/// that merged since the start was made; nothing else that merged, whatever the order units
/// started or finished in. A directory at its path is removed first. A copy made as the
/// change set's copy stands holds that copy's own files, and the merge of every unit merged.
// @lfy def/cli/compile.lfy:compile#compile:compile:3de63e91d36a4914ecba5efb6db7e4b1c99793fe3707ad3fcd9f39dcfb2252af
// @lfy def/cli/compile.lfy:compile#compile:compile:39478a22468516407804aed2553166dbbab86c075fc13792b787cbb941ddd5c7
// @lfy def/cli/compile.lfy:compile#compile:compile:996c71654a860d590a6b9b7e973ff56b5ca9fa6ddf123e8172f04d1287cca3e4
// @lfy def/cli/compile.lfy:compile#compile:compile:4f3ee5302864ac43e74b609edd4d32ee20f6d99060ed1e0babcc498c956098ff
// @lfy def/cli/compile.lfy:compile#compile:compile:2125687777aae924ecaa844b9ab21cf364a6cb1234b7ce57b7f6c3c0550d179e
// @lfy def/cli/compile.lfy:compile#compile:compile:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
#[test]
fn a_units_copy_holds_the_start_and_the_merges_it_waits_for() {
    let fixture = Fixture::new();
    let (mut run, target, copy) = a_set_of_two(&fixture, ROLES, &["compile"]);
    let place = |run: &Run, stem: &str| run.plan.units.iter().position(|unit| unit.stem == stem).unwrap();
    let (a, c) = (place(&run, "a"), place(&run, "c"));
    // a merged since the start, writing out/a.rs, and c waits for it.
    fs::write(copy.join("out/a.rs"), "// @lfy def/a.lfy:A\npub struct A(u8);\n").unwrap();
    run.sets.get_mut("rust").unwrap().merge(&fixture.root, 0, "a", vec!["out/a.rs".to_string()]).unwrap();
    run.sets.get_mut("rust").unwrap().units[0].state = UnitState::Accepted;
    run.schedule.since_start.insert(a);
    run.schedule.merged.insert(a);
    run.schedule.first.insert(a, Vec::new());
    run.schedule.first.insert(c, vec![a]);
    // The change set's copy moves on past what a merged; c's copy is made from the start.
    fs::write(copy.join("out/a.rs"), "// @lfy def/a.lfy:A\npub struct A(u16);\n").unwrap();
    let at = unit_copy_of(&fixture.root, "rust", "c");
    // @lfy def/cli/compile.lfy:compile#compile:compile:996c71654a860d590a6b9b7e973ff56b5ca9fa6ddf123e8172f04d1287cca3e4
    fs::create_dir_all(&at).unwrap();
    fs::write(at.join("stale.txt"), "old\n").unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    // A command that leaves the copy as it was, so that it can be read once the run returns.
    run.start_unit(c, "true", &sender);
    let returned = receiver.recv().unwrap();
    assert_eq!(returned.unit, c);
    // @lfy def/cli/compile.lfy:compile#compile:compile:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
    assert!(at.ends_with("elfie-compile/cache/changes/rust/units/c"), "{at:?}");
    let held: &UnitCopy = &run.schedule.copies[&c];
    // @lfy def/cli/compile.lfy:compile#compile:compile:996c71654a860d590a6b9b7e973ff56b5ca9fa6ddf123e8172f04d1287cca3e4
    assert!(!at.join("stale.txt").exists());
    // @lfy def/cli/compile.lfy:compile#compile:compile:3de63e91d36a4914ecba5efb6db7e4b1c99793fe3707ad3fcd9f39dcfb2252af
    // @lfy def/cli/compile.lfy:compile#compile:compile:39478a22468516407804aed2553166dbbab86c075fc13792b787cbb941ddd5c7
    assert_eq!(held.made["out/a.rs"], b"// @lfy def/a.lfy:A\npub struct A(u8);\n".to_vec());
    assert_eq!(held.made["out/c.rs"], fs::read(start_of(&fixture.root, "rust").join("out/c.rs")).unwrap());
    assert!(at.join("def/c.lfy").is_file(), "a file of the root is missing");
    // @lfy def/cli/compile.lfy:compile#compile:compile:2125687777aae924ecaa844b9ab21cf364a6cb1234b7ce57b7f6c3c0550d179e
    assert!(held.holds.contains("a"), "{:?}", held.holds);
    // A copy made as the change set's copy stands holds its own files as that copy holds them.
    // @lfy def/cli/compile.lfy:compile#compile:compile:4f3ee5302864ac43e74b609edd4d32ee20f6d99060ed1e0babcc498c956098ff
    let again = unit_copy_of(&fixture.root, "rust", "x");
    make_unit_copy(&fixture.root, &target, &again, &copy).unwrap();
    assert_eq!(own_contents(&again, &target)["out/a.rs"], b"// @lfy def/a.lfy:A\npub struct A(u16);\n".to_vec());
    run.remove_every_copy();
    assert!(!at.exists(), "the copy is left");
    run.agent.server.stop();
}

/// A unit that stops without being accepted merges nothing of its copy: every file of its
/// output directory that its copy holds otherwise than when it was made, or added since, is
/// written under `elfie-requests/<name>.rejected`, the change set's copy is left as it was,
/// and the unit's copy is removed.
// @lfy def/cli/compile.lfy:compile#compile:compile:6e192fbbaa3d541cc1fa9490c15245bc628f83977d91727e7004d801f77b9104
// @lfy def/cli/compile.lfy:compile#compile:compile:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
// @lfy def/cli/compile.lfy:compile#compile:compile:38e29db3efde6fef8e4f2e633ff44b5dee097d14f8bbbe3735b12b4a79baeb5c
#[test]
fn a_stopped_unit_keeps_what_its_copy_changed_and_merges_nothing() {
    let fixture = Fixture::new();
    let (mut run, target, copy) = a_set_of_two(&fixture, ROLES, &["compile", "--continue"]);
    let at = unit_copy_of(&fixture.root, "rust", "a");
    make_unit_copy(&fixture.root, &target, &at, &copy).unwrap();
    run.schedule.copies.insert(0, UnitCopy { made: own_contents(&at, &target), directory: at.clone(), holds: BTreeSet::new() });
    fs::write(at.join("out/a.rs"), "// @lfy def/a.lfy:A\nnew\n").unwrap();
    fs::write(at.join("out/extra.rs"), "added\n").unwrap();
    // @lfy def/cli/compile.lfy:compile#compile:compile:6e192fbbaa3d541cc1fa9490c15245bc628f83977d91727e7004d801f77b9104
    run.stopped_unit(0, Stop::Rejected);
    assert_eq!(fixture.read("elfie-requests/a.rejected/out/a.rs"), "// @lfy def/a.lfy:A\nnew\n");
    assert_eq!(fixture.read("elfie-requests/a.rejected/out/extra.rs"), "added\n");
    assert!(fs::read_to_string(copy.join("out/a.rs")).unwrap().contains("pub struct A {}"));
    assert!(!copy.join("out/extra.rs").exists());
    // @lfy def/cli/compile.lfy:compile#compile:compile:38e29db3efde6fef8e4f2e633ff44b5dee097d14f8bbbe3735b12b4a79baeb5c
    assert!(!at.exists(), "the copy is left");
    // Its state is rejected, written at once, and with --continue the compile goes on.
    // @lfy def/cli/compile.lfy:compile#compile:compile:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
    // @lfy def/cli/compile.lfy:compile#compile:compile:3d654e560f2e1144270b416e60c71b73f41429d32f691bba2f8f6fab0038bd7a
    assert_eq!(ChangeSet::load(&fixture.root, "rust").unwrap().units[0].state, UnitState::Rejected);
    assert!(!run.halted);
}

/// A unit's copy for merging, made from the change set's copy as it stands, with its output
/// rewritten as given and its maps recorded there.
// @lfy def/cli/compile.lfy:compile#compile:compile:89c7d76425815427ea012671f3b4f7dddc0031ff70cc737c12eebf4300797c95
fn a_unit_done_in_its_copy(fixture: &Fixture, run: &mut Run, target: &Target, copy: &Path, unit: usize, text: &str) -> PathBuf {
    let stem = run.stem(unit);
    let at = unit_copy_of(&fixture.root, "rust", &stem);
    make_unit_copy(&fixture.root, target, &at, copy).unwrap();
    run.schedule.copies.insert(unit, UnitCopy { made: own_contents(&at, target), directory: at.clone(), holds: BTreeSet::new() });
    fs::write(at.join(format!("out/{stem}.rs")), text).unwrap();
    at
}

/// A unit's merge writes the files its copy changed into the change set's copy, three ways
/// where the change set's copy changed them too, writes its map file as its copy holds it,
/// and records the unit as accepted with what the merge wrote; a merge that wrote an output
/// runs the build, and no command runs after a merge that wrote none.
// @lfy def/cli/compile.lfy:compile#compile:compile:89c7d76425815427ea012671f3b4f7dddc0031ff70cc737c12eebf4300797c95
// @lfy def/cli/compile.lfy:compile#compile:compile:dd602ad17aa5add15d7e0b72130ebaa6955e268ab8b31ac9d2c28c819c36a028
// @lfy def/cli/compile.lfy:compile#compile:compile:e1494d50bc4ca15b22408682181b99b6a82a81da36ad36697a5ce42702ba41d9
// @lfy def/cli/compile.lfy:compile#compile:compile:55e0af72225acb1c6b134a3811478ff630be96bdc40d1f40970151a84cb1eada
// @lfy def/cli/compile.lfy:compile#compile:compile:e97aeb05e9fe84cdc671d0ff5fc5ad164bbca228900888f23cc6a7beedcdb985
// @lfy def/cli/compile.lfy:compile#compile:compile:440bee8e71d6f1c2ae85585e6d67504b3322b68096a7a0167d6acade8f10fde8
#[test]
fn a_merge_writes_what_the_copy_changed_and_accepts_the_unit() {
    let fixture = Fixture::new();
    let (mut run, target, copy) = a_set_of_two(&fixture, &stub_roles(""), &["compile", "--no-verify"]);
    let at = a_unit_done_in_its_copy(&fixture, &mut run, &target, &copy, 0, "// @lfy def/a.lfy:A\npub struct A(u8);\n");
    // The map file the unit's copy holds, as its run recorded it there.
    fs::write(at.join("elfie-compile/maps/rust/a.json"), "[]\n").unwrap();
    // @lfy def/cli/compile.lfy:compile#compile:compile:89c7d76425815427ea012671f3b4f7dddc0031ff70cc737c12eebf4300797c95
    run.merge_unit(0, UnitState::Accepted, Vec::new());
    assert!(fs::read_to_string(copy.join("out/a.rs")).unwrap().contains("A(u8)"));
    // @lfy def/cli/compile.lfy:compile#compile:compile:e1494d50bc4ca15b22408682181b99b6a82a81da36ad36697a5ce42702ba41d9
    assert_eq!(fs::read_to_string(copy.join("elfie-compile/maps/rust/a.json")).unwrap(), "[]\n");
    // @lfy def/cli/compile.lfy:compile#compile:compile:e97aeb05e9fe84cdc671d0ff5fc5ad164bbca228900888f23cc6a7beedcdb985
    assert!(copy.join("out/built.txt").is_file(), "the build did not run after the merge");
    // @lfy def/cli/compile.lfy:compile#compile:compile:55e0af72225acb1c6b134a3811478ff630be96bdc40d1f40970151a84cb1eada
    let set = run.sets["rust"].clone();
    assert_eq!(set.units[0].state, UnitState::Accepted);
    assert_eq!(set.units[0].merged, ["elfie-compile/maps/rust/a.json", "out/a.rs"]);
    assert!(!at.exists(), "the copy is left");
    // The change set's copy changed c.rs elsewhere since c's copy was made: both changes are
    // kept, as the three-way merge gives them.
    // @lfy def/cli/compile.lfy:compile#compile:compile:dd602ad17aa5add15d7e0b72130ebaa6955e268ab8b31ac9d2c28c819c36a028
    fs::write(copy.join("out/c.rs"), "// @lfy def/c.lfy:C\npub struct C {}\n").unwrap();
    let at = a_unit_done_in_its_copy(&fixture, &mut run, &target, &copy, 1, "// @lfy def/c.lfy:C\npub struct C {}\n// more\n");
    fs::write(copy.join("out/c.rs"), "// first\n// @lfy def/c.lfy:C\npub struct C {}\n").unwrap();
    fs::remove_file(copy.join("out/built.txt")).unwrap();
    run.merge_unit(1, UnitState::Accepted, Vec::new());
    assert_eq!(fs::read_to_string(copy.join("out/c.rs")).unwrap(), "// first\n// @lfy def/c.lfy:C\npub struct C {}\n// more\n");
    assert!(!at.exists());
    // A merge that wrote no output runs no command.
    // @lfy def/cli/compile.lfy:compile#compile:compile:440bee8e71d6f1c2ae85585e6d67504b3322b68096a7a0167d6acade8f10fde8
    fs::remove_file(copy.join("out/built.txt")).unwrap();
    run.sets.get_mut("rust").unwrap().units[0].state = UnitState::Planned;
    let text = fs::read_to_string(copy.join("out/a.rs")).unwrap();
    a_unit_done_in_its_copy(&fixture, &mut run, &target, &copy, 0, &text);
    run.merge_unit(0, UnitState::Accepted, Vec::new());
    assert!(!copy.join("out/built.txt").exists(), "a command ran after a merge that wrote nothing");
}

/// A merge whose build fails is undone: every file it wrote holds what it held before, and
/// nothing is recorded; the unit runs again from a copy of the change set's copy as it stands.
/// A merge undone twice in a row stops the unit, planned, with the reason as its problem. A
/// region of the unit's own that another unit changed since its copy was made undoes it too,
/// with nothing written.
// @lfy def/cli/compile.lfy:compile#compile:compile:fd9b01ad05607a5a609c83192a3af14b8fadef545c84da493960eca8b0faef6d
// @lfy def/cli/compile.lfy:compile#compile:compile:a758d1de98247fd21db6f348f4b6ad304727df53a1f2655c9c1794be92ac7fc5
// @lfy def/cli/compile.lfy:compile#compile:compile:ee6f110a6e8a80fe7f058125db6f1cd190ee42098edb6f0deaed19ccf0a70a19
// @lfy def/cli/compile.lfy:compile#compile:compile:654d815adda3e49a4d09b56d2ccda6d531ddada64a727f523fcd7ed1b5ff4b9b
// @lfy def/cli/compile.lfy:compile#compile:compile:d67feda78f528338c7b27d494e8089387777160911a29fc46173c99bfa1b08fd
#[test]
fn a_merge_that_fails_its_build_is_undone() {
    let fixture = Fixture::new();
    let roles = "trait lang extends targetLanguage: `Rust` {\n  .fileExtension = \"rs\";\n  .markerComment = \"//\";\n}\n\
                 trait flat extends targetLayout: `One file per unit` { }\n\
                 trait cargo extends targetEcosystem: `Crates` {\n  .ecosystem = \"cargo\";\n  @commands\n    \
                 .add({ operation = Operation.build, line = \"exit 3\" });\n}\n";
    let (mut run, target, copy) = a_set_of_two(&fixture, roles, &["compile", "--no-verify", "--continue"]);
    let was = fs::read_to_string(copy.join("out/a.rs")).unwrap();
    a_unit_done_in_its_copy(&fixture, &mut run, &target, &copy, 0, "// @lfy def/a.lfy:A\npub struct A(u8);\n");
    // @lfy def/cli/compile.lfy:compile#compile:compile:fd9b01ad05607a5a609c83192a3af14b8fadef545c84da493960eca8b0faef6d
    run.merge_unit(0, UnitState::Accepted, Vec::new());
    assert_eq!(fs::read_to_string(copy.join("out/a.rs")).unwrap(), was);
    assert_eq!(run.sets["rust"].units[0].state, UnitState::Planned);
    // @lfy def/cli/compile.lfy:compile#compile:compile:a758d1de98247fd21db6f348f4b6ad304727df53a1f2655c9c1794be92ac7fc5
    assert!(run.schedule.again.contains(&0) && run.schedule.undone.contains(&0));
    assert!(!run.stopped.contains(&0));
    // Undone a second time in a row: the unit stops, its state planned.
    // @lfy def/cli/compile.lfy:compile#compile:compile:ee6f110a6e8a80fe7f058125db6f1cd190ee42098edb6f0deaed19ccf0a70a19
    // @lfy def/cli/compile.lfy:compile#compile:compile:654d815adda3e49a4d09b56d2ccda6d531ddada64a727f523fcd7ed1b5ff4b9b
    a_unit_done_in_its_copy(&fixture, &mut run, &target, &copy, 0, "// @lfy def/a.lfy:A\npub struct A(u8);\n");
    run.merge_unit(0, UnitState::Accepted, Vec::new());
    assert!(run.stopped.contains(&0));
    assert_eq!(run.sets["rust"].units[0].state, UnitState::Planned);
    assert_eq!(run.failed, 1);
    // c's own region changed in the change set's copy since its copy was made: undone, and
    // nothing of it written.
    // @lfy def/cli/compile.lfy:compile#compile:compile:d67feda78f528338c7b27d494e8089387777160911a29fc46173c99bfa1b08fd
    a_unit_done_in_its_copy(&fixture, &mut run, &target, &copy, 1, "// @lfy def/c.lfy:C\npub struct C(u8);\n");
    fs::write(copy.join("out/c.rs"), "// @lfy def/c.lfy:C\npub struct C(u16);\n").unwrap();
    run.merge_unit(1, UnitState::Accepted, Vec::new());
    assert_eq!(fs::read_to_string(copy.join("out/c.rs")).unwrap(), "// @lfy def/c.lfy:C\npub struct C(u16);\n");
    assert!(run.schedule.undone.contains(&1));
}

/// A foreign edit recorded for a unit not yet in the change set has it join the change set
/// at its place in plan order, as touched; one for a unit already accepted plans it again.
// @lfy def/cli/compile.lfy:compile#compile:compile:eeb6eb51ce2fa61105a263d0328d2874ebfc0bc3ced175501ee78c99422ed3ec
// @lfy def/cli/compile.lfy:compile#compile:compile:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
// @lfy def/cli/compile.lfy:compile#compile:compile:55e0af72225acb1c6b134a3811478ff630be96bdc40d1f40970151a84cb1eada
// @lfy def/cli/compile.lfy:compile#compile:compile:186f5bf02a1197f42b15d46a728b008732f742e22b0a3360bbff586dc9f0dcab
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
    // @lfy def/cli/compile.lfy:compile#compile:compile:55e0af72225acb1c6b134a3811478ff630be96bdc40d1f40970151a84cb1eada
    run.accept_merged(a, UnitState::Accepted, vec![edit], Vec::new());
    let set = run.sets["rust"].clone();
    // @lfy def/cli/compile.lfy:compile#compile:compile:55e0af72225acb1c6b134a3811478ff630be96bdc40d1f40970151a84cb1eada
    assert_eq!(set.units[0].state, UnitState::Accepted);
    assert_eq!(set.units[0].edits, 1);
    assert_eq!(set.foreign_edits.len(), 1);
    // @lfy def/cli/compile.lfy:compile#compile:compile:eeb6eb51ce2fa61105a263d0328d2874ebfc0bc3ced175501ee78c99422ed3ec
    let stems: Vec<&str> = set.units.iter().map(|unit| unit.stem.as_str()).collect();
    assert_eq!(stems, vec!["a", "c"]);
    assert_eq!(set.units[1].reason, Reason::Touched);
    assert_eq!(set.units[1].state, UnitState::Planned);
    // The plan is made again with the edit, so c's reason is touched there too.
    // @lfy def/cli/compile.lfy:compile#compile:compile:fc8d9730ea95e71cb418a2d3c7249b91a9f7ca20d5041d67b3296632614122c8
    assert_eq!(run.plan.units[c].reason, Some(Reason::Touched));
    // An owner already accepted is planned again.
    // @lfy def/cli/compile.lfy:compile#compile:compile:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
    run.sets.get_mut("rust").unwrap().units[1].state = UnitState::Reviewed;
    let again = ForeignEdit { note: "once more".to_string(), ..run.sets["rust"].foreign_edits[0].clone() };
    run.accept_merged(a, UnitState::Accepted, vec![again], Vec::new());
    assert_eq!(run.sets["rust"].units[1].state, UnitState::Planned);
    assert_eq!(run.sets["rust"].foreign_edits.len(), 2);
}

/// An owner before the merged unit in plan order, planned again by its foreign edit, is
/// planned during the compile: it starts once every planned unit before it has merged, been
/// skipped, or stopped, and every unit after it waits for it to merge.
// @lfy def/cli/compile.lfy:compile#compile:compile:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
// @lfy def/cli/compile.lfy:compile#compile:compile:9fb81fa1507e767993a4d11085badfdfe00d34fe56bfd3c332d91cc5d3215da5
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
    assert!(run.settled_before(c));
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
    // @lfy def/cli/compile.lfy:compile#compile:compile:832c4ab9f9106d5819645657a00f8b70a71f5d25244ab1212be34cffa6e9bbd1
    run.accept_merged(c, UnitState::Accepted, vec![edit], Vec::new());
    // @lfy def/cli/compile.lfy:compile#compile:compile:9fb81fa1507e767993a4d11085badfdfe00d34fe56bfd3c332d91cc5d3215da5
    assert!(run.schedule.again.contains(&a));
    assert!(run.waiting(a) && run.ready(a));
    assert!(!run.settled_before(c), "c would not wait for a");
}

/// A unit a kept change set accepted, with the hash and requirements it has now, is planned
/// with the reason the change set holds for it even when the plan gives it none, unless a
/// stem given requests it.
// @lfy def/cli/compile.lfy:compile#compile:compile:7ddc3a3c56212d70e81bbe0dc295510b33a966489a029c6a5c3c3a0907c4b225
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
    // @lfy def/cli/compile.lfy:compile#compile:compile:7ddc3a3c56212d70e81bbe0dc295510b33a966489a029c6a5c3c3a0907c4b225
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
