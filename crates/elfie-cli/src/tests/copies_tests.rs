// @lfy def/cli/main.lfy:main
//! Tests of a change set's copy of the root: what it and its base hold, which of its files
//! are its own, what a resumed copy takes from the root, how the root takes every change set
//! at once, and compiles that work through change sets from beginning to end.

use super::*;
use crate::copies::{base_of, changes_of, copy_of, make_copy, merge_all, mode_of, own_files, set_mode, sync_copy};

/// Whether a file may be run; false everywhere on a platform with no such mode, so that
/// comparing two files still holds there.
// @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
fn executable(path: &Path) -> bool {
    mode_of(path).is_some_and(|mode| mode & 0o111 != 0)
}

/// The one target of a fixture, as the workspace declares it.
fn the_target(fixture: &Fixture) -> Target {
    workspace::load(&fixture.root).targets.into_iter().next().expect("the fixture declares a target")
}

/// A project with one target and a file of every kind a copy treats apart: an output, the
/// target's map and review record files, a file of the project, a script, and files under
/// the cache, the CLI's own folder, and a build directory.
// @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
fn a_project_to_copy(fixture: &Fixture) -> &Fixture {
    one_target(fixture)
        .write("def/a.lfy", "d A: `An A` {}\n")
        .write("kept.txt", "one\n")
        .write("run.sh", "one\n")
        .write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n")
        .write("out/target/debug.rs", "built\n")
        .write("elfie-compile/maps/rust/a.json", "[]\n")
        .write("elfie-compile/reviews/rust/a.json", "[]\n")
        .write("elfie-compile/cache/junk.txt", "cache\n")
        .write("elfie-requests/compile.log", "noise\n")
}

/// A new change set's copy holds every file of the root outside the cache, the CLI's own
/// folder, and every directory named target, .git, or node_modules, byte for byte and keeping
/// whether it is executable; a directory at its path is removed first; and its base holds
/// every one of its own files as the copy held it.
// @lfy def/cli/main.lfy:main#main:main:7209ec1dd07cd609c4d0c6340e068b2f8abc452938bf80a2546b4339a185641e
// @lfy def/cli/main.lfy:main#main:main:c40cbc04ab7381898fa0276bff3403d5869070d1ce78a5380f696993bf5a34dd
// @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
// @lfy def/cli/main.lfy:main#main:main:13ac3d0d600577ad94fbbc002962bb3f615a9df8b9c329232562285f53032386
// @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
// @lfy def/cli/main.lfy:main#main:main:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
#[test]
fn a_new_copy_holds_the_root_and_its_base_holds_its_own_files() {
    let fixture = Fixture::new();
    a_project_to_copy(&fixture);
    set_mode(&fixture.root.join("run.sh"), Some(0o755));
    // A directory left at the copy's path and at the base's is removed first.
    // @lfy def/cli/main.lfy:main#main:main:c40cbc04ab7381898fa0276bff3403d5869070d1ce78a5380f696993bf5a34dd
    fixture.write("elfie-compile/cache/changes/rust/copy/stale.txt", "old\n");
    fixture.write("elfie-compile/cache/changes/rust/base/stale.txt", "old\n");
    let target = the_target(&fixture);
    let copy = make_copy(&fixture.root, &target).expect("a copy");
    // @lfy def/cli/main.lfy:main#main:main:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
    assert!(copy.is_absolute());
    assert!(copy.ends_with("elfie-compile/cache/changes/rust/copy"), "{copy:?}");
    assert!(!copy.join("stale.txt").exists());
    assert!(!base_of(&fixture.root, "rust").join("stale.txt").exists());
    // @lfy def/cli/main.lfy:main#main:main:7209ec1dd07cd609c4d0c6340e068b2f8abc452938bf80a2546b4339a185641e
    assert_eq!(fs::read_to_string(copy.join("kept.txt")).unwrap(), "one\n");
    assert_eq!(fs::read_to_string(copy.join("out/a.rs")).unwrap(), fixture.read("out/a.rs"));
    assert!(copy.join("elfie-compile/maps/rust/a.json").is_file());
    assert!(!copy.join("elfie-compile/cache/junk.txt").exists());
    assert!(!copy.join("elfie-requests/compile.log").exists());
    assert!(!copy.join("out/target/debug.rs").exists());
    // @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
    assert_eq!(executable(&copy.join("run.sh")), executable(&fixture.root.join("run.sh")));
    // Its own files are its output directory's, outside a directory named target, and its
    // target's map and review record files.
    // @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
    assert_eq!(
        own_files(&copy, &target),
        vec!["elfie-compile/maps/rust/a.json", "elfie-compile/reviews/rust/a.json", "out/a.rs"]
    );
    // @lfy def/cli/main.lfy:main#main:main:13ac3d0d600577ad94fbbc002962bb3f615a9df8b9c329232562285f53032386
    let base = base_of(&fixture.root, "rust");
    assert_eq!(own_files(&base, &target), own_files(&copy, &target));
    assert!(!base.join("kept.txt").exists());
}

/// In a root that is a git repository the copy holds what git tracks or would track, and a
/// `.git` file pointing git run in it at the root's git directory.
// @lfy def/cli/main.lfy:main#main:main:670dca2fbcb84c029ce58dc80e12e107f388904bf0cf79378ec5b4136e8b6097
// @lfy def/cli/main.lfy:main#main:main:8a305990c26ec5a21e609fca17bc010a769c49511853e849d7b5b29db5b1b906
#[test]
fn a_copy_of_a_repository_holds_what_git_lists_and_reads_its_history() {
    let fixture = Fixture::new();
    a_project_to_copy(&fixture).write(".gitignore", "ignored.txt\n").write("ignored.txt", "x\n");
    if !a_repository(&fixture) {
        return;
    }
    let copy = make_copy(&fixture.root, &the_target(&fixture)).expect("a copy");
    // @lfy def/cli/main.lfy:main#main:main:670dca2fbcb84c029ce58dc80e12e107f388904bf0cf79378ec5b4136e8b6097
    assert!(copy.join("kept.txt").is_file());
    assert!(!copy.join("ignored.txt").exists(), "a file git ignores was copied");
    assert!(!copy.join("elfie-requests/compile.log").exists());
    // @lfy def/cli/main.lfy:main#main:main:8a305990c26ec5a21e609fca17bc010a769c49511853e849d7b5b29db5b1b906
    let at = git_directory(&fixture.root).expect("the root's git directory");
    assert_eq!(fs::read_to_string(copy.join(".git")).unwrap(), format!("gitdir: {}\n", at.to_string_lossy()));
    assert_eq!(git_directory(&copy), Some(at), "git in the copy reads the root");
}

/// What the root takes of a change set is each of its own files that differs from its base,
/// was added, or was removed; a file the root changed since is the three-way merge of both,
/// and nothing of the root changes when a file cannot be merged without a conflict.
// @lfy def/cli/main.lfy:main#main:main:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
// @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
// @lfy def/cli/main.lfy:main#main:main:eade10ea32d1fd58426767f451ba8db4ba721952a1d1d8f7f61428fa7c1b7eb0
// @lfy def/cli/main.lfy:main#main:main:09d98aeb33f1262c0723d2f55d871762843d90677cc202153ba4a0ad4792759d
// @lfy def/cli/main.lfy:main#main:main:f62308cbdc193558b2d780fd52bf6661146cd7a2e58d88bb29026d348fb53412
// @lfy def/cli/main.lfy:main#main:main:7ea6410a8673a0a0b1c2459596d167fac7037f12858c4c99845e67852a0efb8b
#[test]
fn the_root_takes_what_a_change_set_changed_and_nothing_on_a_conflict() {
    let fixture = Fixture::new();
    a_project_to_copy(&fixture)
        .write("out/shared.rs", "a\nb\nc\n")
        .write("out/gone.rs", "away\n")
        .write("out/both.rs", "one\n");
    let target = the_target(&fixture);
    let copy = make_copy(&fixture.root, &target).expect("a copy");
    fs::write(copy.join("out/a.rs"), "// @lfy def/a.lfy:A\npub struct A(u8);\n").unwrap();
    fs::write(copy.join("out/added.rs"), "new\n").unwrap();
    fs::write(copy.join("out/shared.rs"), "a\nb\nC\n").unwrap();
    fs::remove_file(copy.join("out/gone.rs")).unwrap();
    // A file of the project that is none of its own is never taken.
    fs::write(copy.join("kept.txt"), "changed in the copy\n").unwrap();
    // The root changed shared.rs elsewhere in the file since the base was taken.
    fixture.write("out/shared.rs", "A\nb\nc\n");
    let changes = changes_of(&fixture.root, &copy, &target);
    // @lfy def/cli/main.lfy:main#main:main:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
    let paths: Vec<&str> = changes.keys().map(String::as_str).collect();
    assert_eq!(paths, vec!["out/a.rs", "out/added.rs", "out/gone.rs", "out/shared.rs"]);
    // @lfy def/cli/main.lfy:main#main:main:f62308cbdc193558b2d780fd52bf6661146cd7a2e58d88bb29026d348fb53412
    merge_all(&[changes], &fixture.root).expect("every file merges");
    // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
    assert!(fixture.read("out/a.rs").contains("A(u8)"));
    assert_eq!(fixture.read("out/added.rs"), "new\n");
    assert!(!fixture.root.join("out/gone.rs").exists());
    // @lfy def/cli/main.lfy:main#main:main:eade10ea32d1fd58426767f451ba8db4ba721952a1d1d8f7f61428fa7c1b7eb0
    assert_eq!(fixture.read("out/shared.rs"), "A\nb\nC\n");
    assert_eq!(fixture.read("kept.txt"), "one\n");
    // Both sides changed the same line: nothing of the root changes.
    // @lfy def/cli/main.lfy:main#main:main:7ea6410a8673a0a0b1c2459596d167fac7037f12858c4c99845e67852a0efb8b
    let again = Fixture::new();
    a_project_to_copy(&again).write("out/both.rs", "one\n");
    let target = the_target(&again);
    let copy = make_copy(&again.root, &target).expect("a copy");
    fs::write(copy.join("out/both.rs"), "mine\n").unwrap();
    fs::write(copy.join("out/a.rs"), "// @lfy def/a.lfy:A\npub struct A(u16);\n").unwrap();
    again.write("out/both.rs", "theirs\n");
    let changes = changes_of(&again.root, &copy, &target);
    assert_eq!(merge_all(&[changes], &again.root).unwrap_err(), "out/both.rs");
    assert_eq!(again.read("out/both.rs"), "theirs\n");
    assert!(!again.read("out/a.rs").contains("u16"), "a file of the root changed");
}

/// In a git root a file the change set added is taken only when git would track it.
// @lfy def/cli/main.lfy:main#main:main:76e8b3b4bed342c6f79c5f24800d5a700d7eccaad184dc55edcd275cd5a9e32d
#[test]
fn a_file_git_ignores_is_not_taken() {
    let fixture = Fixture::new();
    a_project_to_copy(&fixture).write(".gitignore", "out/ignored.rs\n");
    if !a_repository(&fixture) {
        return;
    }
    let target = the_target(&fixture);
    let copy = make_copy(&fixture.root, &target).expect("a copy");
    fs::write(copy.join("out/ignored.rs"), "x\n").unwrap();
    fs::write(copy.join("out/tracked.rs"), "y\n").unwrap();
    let changes = changes_of(&fixture.root, &copy, &target);
    // @lfy def/cli/main.lfy:main#main:main:76e8b3b4bed342c6f79c5f24800d5a700d7eccaad184dc55edcd275cd5a9e32d
    assert!(changes.contains_key("out/tracked.rs"), "{:?}", changes.keys());
    assert!(!changes.contains_key("out/ignored.rs"), "{:?}", changes.keys());
}

/// A resumed copy takes from the root every file that is none of its own, drops every such
/// file the root no longer has, keeps its own files as they are, and removes nothing under a
/// directory named target, .git, or node_modules.
// @lfy def/cli/main.lfy:main#main:main:489059c8a497ac06d146f6905d881772dce1f6e60f48adf6f5bd5b6ca509bdc2
#[test]
fn a_resumed_copy_is_brought_up_to_the_root() {
    let fixture = Fixture::new();
    a_project_to_copy(&fixture).write("def/b.lfy", "d B {}\n");
    let target = the_target(&fixture);
    let copy = make_copy(&fixture.root, &target).expect("a copy");
    fs::write(copy.join("out/a.rs"), "// @lfy def/a.lfy:A\npub struct A(u8);\n").unwrap();
    // What a build left in the copy, which no new copy would hold.
    for built in ["target/debug/a", "out/node_modules/x/index.js"] {
        fs::create_dir_all(copy.join(built).parent().unwrap()).unwrap();
        fs::write(copy.join(built), "built\n").unwrap();
    }
    // The root moves on: a definition edited, one removed, one added.
    fixture.write("def/a.lfy", "d A: `An A, edited` {}\n").write("def/c.lfy", "d C {}\n");
    fs::remove_file(fixture.root.join("def/b.lfy")).unwrap();
    // @lfy def/cli/main.lfy:main#main:main:489059c8a497ac06d146f6905d881772dce1f6e60f48adf6f5bd5b6ca509bdc2
    sync_copy(&fixture.root, &copy, &target).unwrap();
    assert_eq!(fs::read_to_string(copy.join("def/a.lfy")).unwrap(), "d A: `An A, edited` {}\n");
    assert!(copy.join("def/c.lfy").is_file());
    assert!(!copy.join("def/b.lfy").exists());
    assert!(fs::read_to_string(copy.join("out/a.rs")).unwrap().contains("A(u8)"), "its own file was replaced");
    assert!(copy.join("target/debug/a").is_file(), "a file under target was removed");
    assert!(copy.join("out/node_modules/x/index.js").is_file(), "a file under node_modules was removed");
}

// ---- compiles that work through change sets ------------------------------------------

/// The roles of a target carrying one command line per operation named, and the stub line
/// and stub marker the language gives.
// @lfy def/cli/main.lfy:main#main:main:2122ce59b6ca33af5e18c265f0334204d7ceb9807440dcaae456e418da206c9d
fn roles_with(commands: &str) -> String {
    format!(
        "trait lang extends targetLanguage: `Rust` {{\n  .fileExtension = \"rs\";\n  \
         .markerComment = \"//\";\n  .stubLine = 'todo!(\"elfie stub: {{{{unit}}}}: {{{{reason}}}}\")';\n  \
         .stubMarker = \"elfie stub:\";\n}}\n\
         trait flat extends targetLayout: `One file per unit` {{ }}\n\
         trait cargo extends targetEcosystem: `Crates from crates.io` {{\n  \
         .ecosystem = \"cargo\";\n  @commands{commands};\n}}\n"
    )
}

/// A build command line and a test command line that both exit 0.
// @lfy def/cli/main.lfy:main#main:main:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
const PASSES: &str = "\n    .add({ operation = Operation.build, line = \"true\" })\
                      \n    .add({ operation = Operation.test, line = \"true\" })";

/// A command that notes that it ran and then fails, so that a run of it shows.
// @lfy def/cli/main.lfy:main#main:main:cc6a708b69ecdcd47969295e5a84e602e93615db8d61bd14af671d9f6e9fd325
const RAN_AND_FAILED: &str = "#!/bin/sh\ncat > /dev/null\necho ran >> '<notes>/ran.txt'\nexit 1\n";

/// A verifier that finds nothing violated: it answers satisfied for every id the review
/// request it reads names.
// @lfy def/cli/main.lfy:main#main:main:1a380544042dc2a265daa89f48fba57d42b2227401fa8e393f68af08ae06fc21
const CONTENT: &str = "#!/bin/sh\necho \"$ELFIE_UNIT\" >> '<notes>/verified.txt'\nfor id in $(grep -o '[A-Za-z_]*:[A-Za-z_]*:[0-9a-f]\\{64\\}' | sort -u); do\n  printf '{\"id\":\"%s\",\"status\":\"satisfied\",\"evidence\":\"out\",\"note\":\"fine\"}\\n' \"$id\"\ndone\necho 'ELFIE: REVIEWED'\n";

/// The hash a unit's map file records for one dependency of it.
// @lfy def/cli/main.lfy:main#main:main:42e098db25f729f653afa31b4ff6a56916d8ce4d4d18049d458c6d86ed140e13
fn dependency_hash(fixture: &Fixture, path: &str, source: &str) -> String {
    maps_of(fixture, path)[0]["dependencies"][source].as_str().unwrap_or_default().to_string()
}

/// The signature one unit's recorded maps give its interface now.
// @lfy def/cli/main.lfy:main#main:main:42e098db25f729f653afa31b4ff6a56916d8ce4d4d18049d458c6d86ed140e13
fn signature_of(fixture: &Fixture, source: &str) -> String {
    let maps = generation::source_maps_of(&workspace::load(&fixture.root), None);
    maps.iter()
        .find(|map| map.source == source)
        .map(|map| map.signature.clone())
        .unwrap_or_else(|| panic!("{source} has no recorded map"))
}

/// The log of a compile forgotten, so that the next one reads on its own.
fn forget_log(fixture: &Fixture) {
    let _ = fs::remove_file(fixture.root.join("elfie-requests/compile.log"));
}

/// A git root with two units both recorded and one of them edited since: the edited one
/// alone is compiled, in a copy under `elfie-compile/cache/changes/rust`, its map file is
/// rewritten in the root once the root takes the change set, the other's is left byte for
/// byte as it was, and no change set is left.
// @lfy def/cli/main.lfy:main#main:main:bfa4334a09d69c17d9bd245a5e8c4333d4d86982e7c9326fc4389d63f029f41e
// @lfy def/cli/main.lfy:main#main:main:620fa0faf525e60e5d31d15d5f9c42d2193013818b1cf267a45bff765761c588
#[test]
fn a_compile_of_one_unit_works_in_a_copy_and_leaves_every_other_map_file_alone() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    if !a_repository(&fixture) {
        return;
    }
    fixture.write("def/b.lfy", "d B {}\n");
    fixture.write(
        "compiler.sh",
        &format!("{}echo \"$ELFIE_ROOT\" >> '{}/where.txt'\n", per_unit_compiler(&fixture), fixture.path()),
    );
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    let (was_a, was_b) = (fixture.read("elfie-compile/maps/rust/a.json"), fixture.read("elfie-compile/maps/rust/b.json"));
    fixture.write("def/a.lfy", "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list once` });\n}\n");
    write_verifier(&fixture, SATISFIED);
    fixture.write("attempts.txt", "").write("where.txt", "");
    // @lfy def/cli/main.lfy:main#main:main:bfa4334a09d69c17d9bd245a5e8c4333d4d86982e7c9326fc4389d63f029f41e
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert_eq!(fixture.read("attempts.txt"), "a\n");
    assert!(fixture.read("where.txt").trim().ends_with("elfie-compile/cache/changes/rust/units/a"), "{}", fixture.read("where.txt"));
    // @lfy def/cli/main.lfy:main#main:main:620fa0faf525e60e5d31d15d5f9c42d2193013818b1cf267a45bff765761c588
    assert_ne!(fixture.read("elfie-compile/maps/rust/a.json"), was_a, "a.json was not rewritten");
    assert_eq!(fixture.read("elfie-compile/maps/rust/b.json"), was_b, "b.json changed");
    assert!(!fixture.root.join("elfie-compile/cache/changes/rust.json").exists(), "a change set is left");
    assert!(!fixture.root.join("elfie-compile/cache/changes/rust").exists(), "a copy or a base is left");
}

/// A git root whose unit has the reason dependency has the unit gate run in place of the
/// compiler: it passes, the unit is accepted on it with no compiler and no verifier run, the
/// change set passes its final gate and the root takes it, the maps record the dependency's
/// interface hash as it is now, and no review record file changes.
// @lfy def/cli/main.lfy:main#main:main:42e098db25f729f653afa31b4ff6a56916d8ce4d4d18049d458c6d86ed140e13
// @lfy def/cli/main.lfy:main#main:main:f82d76ff8e23e283093211b99bd7fa4f7edfa0a9f9151cf64b7864379ceec690
// @lfy def/cli/main.lfy:main#main:main:1535e1a49cabe0f48836a4331c9b37e4b542f78aa3bfec5c56622eebf9cd3707
// @lfy def/cli/main.lfy:main#main:main:cc6a708b69ecdcd47969295e5a84e602e93615db8d61bd14af671d9f6e9fd325
#[test]
fn a_dependency_only_unit_is_accepted_on_its_unit_gate() {
    let fixture = Fixture::new();
    library(&fixture)
        .write("def/roles.lfy", &roles_with(PASSES))
        .write("def/targets.lfy", TARGETS)
        .write("def/b.lfy", "d B: `A B` {}\n")
        .write("def/a.lfy", "use \"./b\";\n\nd A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\n")
        .write("compiler.sh", &per_unit_compiler(&fixture));
    if !a_repository(&fixture) {
        return;
    }
    write_verifier(&fixture, SATISFIED);
    fixture.write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]));
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let records = fixture.read("elfie-compile/reviews/rust/a.json");
    let signature = signature_of(&fixture, "def/b.lfy");
    let mut maps = maps_of(&fixture, "elfie-compile/maps/rust/a.json");
    maps[0]["dependencies"]["def/b.lfy"] = serde_json::Value::String("0".repeat(64));
    fixture.write("elfie-compile/maps/rust/a.json", &format!("{maps}\n"));
    fixture.write("compiler.sh", &noted(&fixture, RAN_AND_FAILED)).write("verifier.sh", &noted(&fixture, RAN_AND_FAILED));
    forget_log(&fixture);
    // @lfy def/cli/main.lfy:main#main:main:42e098db25f729f653afa31b4ff6a56916d8ce4d4d18049d458c6d86ed140e13
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert!(!fixture.root.join("ran.txt").exists(), "the compiler or the verifier ran");
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/main.lfy:main#main:main:25b8417f9f55800eec5857dab9bbcf5fbbed7096f0f48ce932ccb4e0ea51ac39
    assert!(log.lines().any(|line| line.contains("▸ building") && line.contains("the unit gate passed")), "{log}");
    assert!(log.lines().any(|line| line.contains("✓ accepted") && line.contains("  a  ")), "{log}");
    assert!(log.contains("▸ finalGate") && log.contains("▸ merging"), "{log}");
    // @lfy def/cli/main.lfy:main#main:main:cc6a708b69ecdcd47969295e5a84e602e93615db8d61bd14af671d9f6e9fd325
    assert_eq!(dependency_hash(&fixture, "elfie-compile/maps/rust/a.json", "def/b.lfy"), signature);
    assert_eq!(fixture.read("elfie-compile/reviews/rust/a.json"), records);
}

/// A git root whose test command passes with stubs allowed when only stubs fail: a member
/// added to b's data changes b's interface, so a, which uses b, waits for b; b's run stubs the
/// caller in a's region, a stubbed line names a once b merged, and a is compiled with reason
/// dependency and b's stub in its request; the final gate finds no stub, and the root takes
/// both.
// @lfy def/cli/main.lfy:main#main:main:8c956516ab865d5332559537c0fa681c7091a355832236231c7b718a630a017e
// @lfy def/cli/main.lfy:main#main:main:186f5bf02a1197f42b15d46a728b008732f742e22b0a3360bbff586dc9f0dcab
// @lfy def/cli/main.lfy:main#main:main:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
// @lfy def/cli/main.lfy:main#main:main:8214a980d0dc5f7d2ad165f456c7096d6e4ef89966519c6a783ecc99a04969fd
// @lfy def/cli/main.lfy:main#main:main:5b241bf72a28eb83044e40272acd76f441efff41a88bd21769255cca804fdc4a
// @lfy def/cli/main.lfy:main#main:main:ba814337ca5699de23cfbe6d5488f298a354e81440cab6936bf45c6ccc9f2411
#[test]
fn a_stub_in_a_later_unit_is_listed_in_its_request() {
    let fixture = Fixture::new();
    let tests = "\n    .add({ operation = Operation.build, line = \"true\" })\
                 \n    .add({ operation = Operation.test, line = \"[ -n \\\"$ALLOW_STUBS\\\" ] || ! grep -rq 'elfie stub' .\" })";
    library(&fixture)
        .write("def/roles.lfy", &roles_with(tests))
        .write("def/targets.lfy", TARGETS)
        .write("def/b.lfy", "d B: `A B` {}\n")
        .write("def/a.lfy", "use \"./b\";\n\nd A: `An A` {\n  $b: `The B it holds` = B;\n}\n")
        .write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B {}\n")
        .write("out/a.rs", "// @lfy def/a.lfy:A\npub fn a() -> u8 {\n    b(1)\n}\n");
    if !a_repository(&fixture) {
        return;
    }
    // Both recorded as they are.
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    // A member is added to b's data since, which changes b's interface.
    fixture.write("def/b.lfy", "d B: `A B` {\n  $n: `The number it holds` = number;\n}\n");
    let (_, plan) = plan_with(&fixture.root, &generation::source_maps_of(&workspace::load(&fixture.root), None), &[], &[], &[]);
    let a = plan.units.iter().find(|unit| unit.stem == "a").unwrap();
    // @lfy def/cli/main.lfy:main#main:main:8cf4943ef6637dd81797621b8abc048edcc7781a4f19546fbe99863339043826
    assert_eq!(a.waits_for, ["b"], "a does not wait for b");
    assert_eq!(a.reason, Some(Reason::Dependency));
    let compiler = noted(
        &fixture,
        "#!/bin/sh\ncat >> '<notes>/requests.txt'\necho \"$ELFIE_UNIT\" >> '<notes>/attempts.txt'\ncase \"$ELFIE_UNIT\" in\n\
         b)\nprintf '// @lfy def/b.lfy:B\\npub struct B(u8);\\n' > \"$ELFIE_ROOT/out/b.rs\"\n\
         printf '// @lfy def/a.lfy:A\\npub fn a() -> u8 {\\n    todo!(\"elfie stub: b: B takes a number now\")\\n}\\n' > \"$ELFIE_ROOT/out/a.rs\"\n;;\n\
         a)\nprintf '// @lfy def/a.lfy:A\\npub fn a() -> u8 {\\n    B(1).0\\n}\\n' > \"$ELFIE_ROOT/out/a.rs\"\n;;\nesac\necho 'ELFIE: DONE'\n",
    );
    fixture
        .write("compiler.sh", &compiler)
        .write("verifier.sh", &noted(&fixture, CONTENT))
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]));
    forget_log(&fixture);
    // @lfy def/cli/main.lfy:main#main:main:8c956516ab865d5332559537c0fa681c7091a355832236231c7b718a630a017e
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let log = fixture.read("elfie-requests/compile.log");
    // b merged, then a stubbed line names a.
    // @lfy def/cli/main.lfy:main#main:main:186f5bf02a1197f42b15d46a728b008732f742e22b0a3360bbff586dc9f0dcab
    let stubbed = log.lines().position(|line| line.contains("○ stubbed") && line.contains("  a  ")).unwrap_or_else(|| panic!("{log}"));
    let merged = log.lines().position(|line| line.contains("✓ merged") && line.contains("  b  ")).unwrap_or_else(|| panic!("{log}"));
    assert!(merged < stubbed, "{log}");
    // a waited for b, and was compiled with reason dependency, its request listing b's stub.
    // @lfy def/cli/main.lfy:main#main:main:8214a980d0dc5f7d2ad165f456c7096d6e4ef89966519c6a783ecc99a04969fd
    // @lfy def/cli/main.lfy:main#main:main:5b241bf72a28eb83044e40272acd76f441efff41a88bd21769255cca804fdc4a
    assert_eq!(fixture.read("attempts.txt"), "b\na\n");
    let started = log.lines().position(|line| line.contains("▸ started") && line.contains("  a  ")).unwrap_or_else(|| panic!("{log}"));
    assert!(merged < started, "{log}");
    assert!(log.lines().any(|line| line.contains("· requesting") && line.contains("  a  ") && line.ends_with("dependency")), "{log}");
    assert!(fixture.read("requests.txt").contains("B takes a number now"), "{}", fixture.read("requests.txt"));
    // The final gate found no stub, and the root took both units.
    // @lfy def/cli/main.lfy:main#main:main:f1b10f02356ca91bbce69918d73b71540bc544c39e28653ce88393191e8e8ffe
    assert!(log.contains("▸ finalGate") && log.contains("▸ merging"), "{log}");
    assert!(fixture.read("out/a.rs").contains("B(1).0"));
    assert!(fixture.read("out/b.rs").contains("B(u8)"));
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("out/a.rs"));
    assert!(fixture.read("elfie-compile/maps/rust/b.json").contains("out/b.rs"));
}

/// Two units that wait for nothing start at once under --jobs 2, and a third, which waits for
/// nothing since the interface of the one it uses is the one its outputs record, starts once
/// one of them is done; they merge in plan order.
// @lfy def/cli/main.lfy:main#main:main:3796b7decb38eb2728fbc8eb3d12a2cc5e151c6d62217a892d3bb1d2e38d511b
// @lfy def/cli/main.lfy:main#main:main:1fd9be2beb849bc4a65969dbedc075ab348b763ef108d6fbc41f4001bc8a86b9
// @lfy def/cli/main.lfy:main#main:main:932f229a081364bb85c06954c07796f035ea0cd10a0285dc8a983a60bbe1fcc8
// @lfy def/cli/main.lfy:main#main:main:64612b8370c169cfba1b380bcbd8e3e7396cfd3a0ff4f100a5d0f40becf6bd69
// @lfy def/cli/main.lfy:main#main:main:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
#[test]
fn two_jobs_run_two_units_at_once() {
    let fixture = Fixture::new();
    let entity = |name: &str, behavior: &str| {
        format!("d {name}: `One {name}` {{\n  @acceptanceCriteria\n    .add({{ behavior = `{behavior}` }});\n}}\n")
    };
    library(&fixture)
        .write("def/roles.lfy", &roles_with(PASSES))
        .write("def/targets.lfy", TARGETS)
        .write("def/a.lfy", &entity("A", "Holds one"))
        .write("def/b.lfy", &entity("B", "Holds two"))
        .write("def/c.lfy", &format!("use \"./a\";\n\n{}", entity("C", "Holds three")))
        .write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n")
        .write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B {}\n")
        .write("out/c.rs", "// @lfy def/c.lfy:C\npub struct C {}\n");
    if !a_repository(&fixture) {
        return;
    }
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    // What each must do is edited since, in its criterion alone, which leaves its interface as
    // it was.
    fixture
        .write("def/a.lfy", &entity("A", "Holds the one"))
        .write("def/b.lfy", &entity("B", "Holds the two"))
        .write("def/c.lfy", &format!("use \"./a\";\n\n{}", entity("C", "Holds the three")));
    let (_, plan) = plan_with(&fixture.root, &generation::source_maps_of(&workspace::load(&fixture.root), None), &[], &[], &[]);
    let c = plan.units.iter().find(|unit| unit.stem == "c").unwrap();
    assert!(c.waits_for.is_empty(), "{:?}", c.waits_for);
    let order: Vec<&str> = plan.units.iter().filter(|unit| unit.reason.is_some()).map(|unit| unit.stem.as_str()).collect();
    assert_eq!(order.len(), 3, "{order:?}");
    // A compiler that writes each unit's output again as it was, slowly enough that the two
    // first runs overlap.
    let compiler = "#!/bin/sh\ncat > /dev/null\nsleep 1\necho 'ELFIE: DONE'\n";
    fixture
        .write("compiler.sh", compiler)
        .write("verifier.sh", &noted(&fixture, CONTENT))
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]));
    forget_log(&fixture);
    // @lfy def/cli/main.lfy:main#main:main:3796b7decb38eb2728fbc8eb3d12a2cc5e151c6d62217a892d3bb1d2e38d511b
    assert_eq!(fixture.run(&["compile", "--jobs", "2"]), ExitCode::Success.code());
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/main.lfy:main#main:main:932f229a081364bb85c06954c07796f035ea0cd10a0285dc8a983a60bbe1fcc8
    let started: Vec<&str> = log.lines().filter(|line| line.contains("▸ started")).collect();
    assert_eq!(started.len(), 3, "{log}");
    assert!(started[0].ends_with("1 of 2 running") && started[1].ends_with("2 of 2 running"), "{log}");
    // The third started once one of the first two was done.
    // @lfy def/cli/main.lfy:main#main:main:64612b8370c169cfba1b380bcbd8e3e7396cfd3a0ff4f100a5d0f40becf6bd69
    let at = |text: &str| log.lines().position(|line| line.contains(text)).unwrap_or_else(|| panic!("no {text} in {log}"));
    assert!(at("✓ reviewed") < at(started[2]), "{log}");
    assert!(started[2].ends_with("2 of 2 running"), "{log}");
    // @lfy def/cli/main.lfy:main#main:main:acabbfb20c858bf034438b84b716e4a6a9d443d7c7d563d0e3fae30099d78448
    let merged: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("✓ merged"))
        .filter_map(|line| order.iter().copied().find(|stem| line.contains(&format!("  {stem}  "))))
        .collect();
    assert_eq!(merged, order, "{log}");
    assert!(log.contains("▸ finalGate") && log.contains("▸ merging"), "{log}");
}

/// A compile stopped when a later unit's compiler was blocked keeps its change set; the
/// next compile resumes it, keeping the unit already reviewed and running the blocked one
/// again, then passes the final gate and merges.
// @lfy def/cli/main.lfy:main#main:main:39f5668011c9acefd792153ca4130581b4795e9d45b0fffb0e36c7124a35d180
// @lfy def/cli/main.lfy:main#main:main:54faac0351075e5f797a3b89b57d4af904f0f8a00cf097ab716767e08566ab00
// @lfy def/cli/main.lfy:main#main:main:85a283dc1f225f02956fbed8a23688bb8b6f297fdc8bea08fed48d08fd0de9b5
// @lfy def/cli/main.lfy:main#main:main:4fb67ab23f1c2255a1177088bce0458b4c7fc72eee0b9f60cc2c47cdc0e85480
#[test]
fn a_kept_change_set_is_resumed() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/b.lfy", "d B: `A B` {\n  @acceptanceCriteria\n    .add({ behavior = `Holds a number` });\n}\n")
        .write("def/a.lfy", "use \"./b\";\n\nd A: `An A` {}\n");
    let compiler = noted(
        &fixture,
        "#!/bin/sh\ncat > /dev/null\necho \"$ELFIE_UNIT\" >> '<notes>/attempts.txt'\nmkdir -p \"$ELFIE_ROOT/out\"\ncase \"$ELFIE_UNIT\" in\n\
         b)\nprintf '// @lfy def/b.lfy:B\\npub struct B {}\\n' > \"$ELFIE_ROOT/out/b.rs\"\n;;\n\
         a)\nif [ ! -f '<notes>/blocked.txt' ]; then echo once > '<notes>/blocked.txt'; echo 'ELFIE: BLOCKED: not yet'; exit 0; fi\n\
         printf '// @lfy def/a.lfy:A\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\n;;\nesac\necho 'ELFIE: DONE'\n",
    );
    fixture
        .write("compiler.sh", &compiler)
        .write("verifier.sh", &noted(&fixture, CONTENT))
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]));
    // b is accepted and reviewed, then a's compiler is blocked: the compile stops, and the
    // root takes nothing.
    // @lfy def/cli/main.lfy:main#main:main:85a283dc1f225f02956fbed8a23688bb8b6f297fdc8bea08fed48d08fd0de9b5
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    assert!(!fixture.root.join("out/b.rs").exists(), "the root took a change set");
    let kept = ChangeSet::load(&fixture.root, "rust").expect("the change set is kept");
    let state = |stem: &str| kept.units.iter().find(|unit| unit.stem == stem).map(|unit| unit.state);
    assert_eq!(state("b"), Some(UnitState::Reviewed));
    assert_eq!(state("a"), Some(UnitState::Planned));
    forget_log(&fixture);
    // @lfy def/cli/main.lfy:main#main:main:39f5668011c9acefd792153ca4130581b4795e9d45b0fffb0e36c7124a35d180
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let log = fixture.read("elfie-requests/compile.log");
    // One resuming line counts the unit kept and the unit that runs again.
    // @lfy def/cli/main.lfy:main#main:main:54faac0351075e5f797a3b89b57d4af904f0f8a00cf097ab716767e08566ab00
    assert!(log.lines().any(|line| line.contains("· resuming") && line.ends_with("rust: 1 units kept, 1 run again")), "{log}");
    // No compiler ran on b again.
    assert_eq!(fixture.read("attempts.txt"), "b\na\na\n");
    assert!(log.contains("▸ finalGate") && log.contains("▸ merging"), "{log}");
    // Both count as accepted, the one kept from the resumed change set included.
    // @lfy def/cli/main.lfy:main#main:main:4fb67ab23f1c2255a1177088bce0458b4c7fc72eee0b9f60cc2c47cdc0e85480
    assert!(log.contains("2 accepted, 0 rejected, 0 blocked, 0 failed"), "{log}");
    assert!(fixture.read("out/a.rs").contains("pub struct A"));
    assert!(fixture.read("out/b.rs").contains("pub struct B"));
}

/// A git root whose lint command fails printing a warning at a line of another unit's
/// output: the unit's own gate passes, since no line of the lint's output names one of its
/// own lines, but the final gate fails naming the lint, its exit code, and the warning's
/// line; the root is unchanged and the change set is kept with the unit reviewed.
// @lfy def/cli/main.lfy:main#main:main:a681a0e7b0078130a0e833cc0cb82a8d09d63a79140fe4dd827e4c490a9021e1
// @lfy def/cli/main.lfy:main#main:main:26c73a135ccf59fa42bd380a4eb712ba0f4ccb70d5082c9a3838abe729e4a29f
// @lfy def/cli/main.lfy:main#main:main:5d082ae0ac17d428f19ee0894765ffab9b0227cf846ca57511c6451d474a7ff0
// @lfy def/cli/main.lfy:main#main:main:3d56bbbdda35f1bdb3f31f4a0bcb7a611efd511b697c7deed056bda76554bac2
#[test]
fn a_lint_warning_in_another_units_lines_fails_the_final_gate() {
    let fixture = Fixture::new();
    let commands = "\n    .add({ operation = Operation.build, line = \"true\" })\
                    \n    .add({ operation = Operation.test, line = \"true\" })\
                    \n    .add({ operation = Operation.lint, line = \"echo 'warning: unused at out/b.rs:2:5'; exit 1\" })";
    library(&fixture)
        .write("def/roles.lfy", &roles_with(commands))
        .write("def/targets.lfy", TARGETS)
        .write("def/a.lfy", "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\n")
        .write("def/b.lfy", "d B: `A B` {}\n")
        .write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n")
        .write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B {}\n");
    if !a_repository(&fixture) {
        return;
    }
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    let recorded = fixture.read("elfie-compile/maps/rust/a.json");
    fixture.write("def/a.lfy", "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list once` });\n}\n");
    fixture
        .write("compiler.sh", &noted(&fixture, A_COMPILER))
        .write("verifier.sh", &noted(&fixture, CONTENT))
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]));
    forget_log(&fixture);
    // @lfy def/cli/main.lfy:main#main:main:a681a0e7b0078130a0e833cc0cb82a8d09d63a79140fe4dd827e4c490a9021e1
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/main.lfy:main#main:main:26c73a135ccf59fa42bd380a4eb712ba0f4ccb70d5082c9a3838abe729e4a29f
    assert!(log.lines().any(|line| line.contains("▸ building") && line.contains("the unit gate passed")), "{log}");
    // @lfy def/cli/main.lfy:main#main:main:5d082ae0ac17d428f19ee0894765ffab9b0227cf846ca57511c6451d474a7ff0
    assert!(log.contains("    - the lint command exited with 1"), "{log}");
    assert!(log.contains("      warning: unused at out/b.rs:2:5"), "{log}");
    // @lfy def/cli/main.lfy:main#main:main:3d56bbbdda35f1bdb3f31f4a0bcb7a611efd511b697c7deed056bda76554bac2
    assert!(!log.contains("▸ merging"), "{log}");
    assert_eq!(fixture.read("elfie-compile/maps/rust/a.json"), recorded, "the root changed");
    let kept = ChangeSet::load(&fixture.root, "rust").expect("the change set is kept");
    assert_eq!(kept.units.iter().find(|unit| unit.stem == "a").map(|unit| unit.state), Some(UnitState::Reviewed));
}

/// A project whose target writes to `src`, `def/a.lfy` declaring A and F, F with one
/// criterion, and `src/a.rs` holding a.lfy's A and two markers naming a.lfy's F, the second
/// answering for its criterion; the outputs recorded, reviewed when `reviewed`, and then F
/// moved into `def/b.lfy`. Gives the id of F's criterion.
// @lfy def/cli/main.lfy:main#main:main:bff02a24e41086dd337710c71f0254cb6709697595b22d9d6202e840528803e3
fn a_moved_entity(fixture: &Fixture, reviewed: bool) -> Option<String> {
    let entity_f = "d F: `An F` {\n  @acceptanceCriteria\n    .add({ behavior = `Gives one` });\n}\n";
    library(fixture)
        .write("def/roles.lfy", ROLES)
        .write("def/targets.lfy", &TARGETS.replace("\"out\"", "\"src\""))
        .write("def/a.lfy", &format!("d A: `An A` {{}}\n\n{entity_f}"));
    let (program, _) = plan_with(&fixture.root, &[], &[], &[], &[]);
    let mut ids = Vec::new();
    for lowered in &program.files {
        if program.workspace.files[lowered.file].path.ends_with("a.lfy") {
            ids_of(&lowered.root, &mut ids);
        }
    }
    let id = ids.first().cloned().expect("F's criterion");
    fixture.write(
        "src/a.rs",
        &format!("// @lfy def/a.lfy:A\npub struct A {{}}\n// @lfy def/a.lfy:F\npub struct F {{}}\n// @lfy def/a.lfy:F#{id}\nimpl F {{ pub fn one() -> u8 {{ 1 }} }}\n"),
    );
    fixture
        .write("compiler.sh", "#!/bin/sh\ncat > /dev/null\necho 'ELFIE: DONE'\n")
        .write("verifier.sh", &noted(fixture, CONTENT))
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh"), ("verifier", "sh verifier.sh")]));
    if reviewed && !a_repository(fixture) {
        return None;
    }
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    if reviewed {
        // Its verdicts recorded, the one on F's criterion among them.
        assert_eq!(fixture.run(&["compile", "--all"]), ExitCode::Success.code());
        assert!(fixture.read("elfie-compile/reviews/rust/a.json").contains(&id), "no record of F's criterion");
    }
    fixture.write("def/a.lfy", "d A: `An A` {}\n").write("def/b.lfy", entity_f);
    Some(id)
}

/// A dry run of a root whose F moved from `def/a.lfy` into `def/b.lfy` prints one relocate
/// line for it, before the unit lines, both units planned as changed, and writes nothing.
// @lfy def/cli/main.lfy:main#main:main:bff02a24e41086dd337710c71f0254cb6709697595b22d9d6202e840528803e3
// @lfy def/cli/main.lfy:main#main:main:5042efb3c946c5366ff884b941565a037c3850a2204c27eacb2e1b0695372035
// @lfy def/cli/main.lfy:main#main:main:3fba4385c7b8b84625d1b60c07fcd93cf8655fa31717ba399f3623d31b803900
// @lfy def/cli/main.lfy:main#main:main:5d97ccabe3f421ec1dcd897744e41b146dc2b9c49e9b10c2749b0b64f809346a
// @lfy def/cli/main.lfy:main#main:main:57f98e0f79a657eab8206e1ac47bf236e459d925b355f7534fc575b98b968a1b
#[test]
fn a_dry_run_prints_the_relocations_and_writes_nothing() {
    let fixture = Fixture::new();
    a_moved_entity(&fixture, false);
    let (output, maps) = (fixture.read("src/a.rs"), fixture.read("elfie-compile/maps/rust/a.json"));
    // @lfy def/cli/main.lfy:main#main:main:bff02a24e41086dd337710c71f0254cb6709697595b22d9d6202e840528803e3
    let (code, out, _) = fixture.printed(&["compile", "--dry-run", "--color=never"]);
    assert_eq!(code, ExitCode::Success.code(), "{out}");
    let lines: Vec<&str> = out.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    // @lfy def/cli/main.lfy:main#main:main:5042efb3c946c5366ff884b941565a037c3850a2204c27eacb2e1b0695372035
    // @lfy def/cli/main.lfy:main#main:main:3fba4385c7b8b84625d1b60c07fcd93cf8655fa31717ba399f3623d31b803900
    let relocate = lines.iter().position(|line| *line == "relocate rust F def/a.lfy into def/b.lfy 2 markers");
    let a = lines.iter().position(|line| line.starts_with("rust a changed"));
    let b = lines.iter().position(|line| line.starts_with("rust b changed"));
    assert!(relocate.is_some() && a.is_some() && b.is_some(), "{out}");
    assert!(relocate < a && relocate < b, "{out}");
    // Painted, the files are subject and the words muted.
    // @lfy def/cli/main.lfy:main#main:main:57f98e0f79a657eab8206e1ac47bf236e459d925b355f7534fc575b98b968a1b
    let relocation = Relocation {
        target: "rust".to_string(),
        entity: "F".to_string(),
        origin: "def/a.lfy".to_string(),
        into: "def/b.lfy".to_string(),
        markers: 1,
    };
    let painted = crate::compiling::relocate_text(&relocation, true);
    assert!(painted.contains(&paint("def/b.lfy", Tone::Subject, true)) && painted.contains(&paint("into", Tone::Muted, true)));
    assert!(crate::compiling::relocate_text(&relocation, false).ends_with("1 marker"));
    // Nothing the relocation gives is written.
    // @lfy def/cli/main.lfy:main#main:main:5d97ccabe3f421ec1dcd897744e41b146dc2b9c49e9b10c2749b0b64f809346a
    assert_eq!(fixture.read("src/a.rs"), output);
    assert_eq!(fixture.read("elfie-compile/maps/rust/a.json"), maps);
    assert!(!fixture.root.join("elfie-compile/maps/rust/b.json").exists());
}

/// A compile of that root relocates F's code into b in the change set: one relocated line
/// counts one entity, two markers, and one review record; b's request says F was relocated
/// into it from `def/a.lfy`; b's review carries the verdict on F's criterion; and once the
/// root takes the change set, `src/a.rs` names `def/b.lfy:F`, with b's map file and review
/// record file written.
// @lfy def/cli/main.lfy:main#main:main:e5cc02f11c4d700f5c14a26237f2478b49f84050526f18072c6560e8fba4e2ea
// @lfy def/cli/main.lfy:main#main:main:be6f670ec86351b9c898da8d21d0741aadee997be9c0c07bfeb4504ac8af962c
// @lfy def/cli/main.lfy:main#main:main:56f0e92efcae2aa29056b323f26d89a551242862fc55f1b85a0c3fc607c66f75
// @lfy def/cli/main.lfy:main#main:main:2b2ecf83e7e057bba8be8cefb6ab9f0de30c74dfba2269df5b9fb4915457dec3
// @lfy def/cli/main.lfy:main#main:main:56dfba76b54a5c6836f292e001f259f857a7245ccec2e5b571ddfb7f62347513
#[test]
fn a_compile_relocates_moved_code_into_its_unit() {
    let fixture = Fixture::new();
    let Some(id) = a_moved_entity(&fixture, true) else { return };
    fixture.write("compiler.sh", &noted(&fixture, "#!/bin/sh\ncat >> '<notes>/requests.txt'\necho 'ELFIE: DONE'\n"));
    forget_log(&fixture);
    // @lfy def/cli/main.lfy:main#main:main:e5cc02f11c4d700f5c14a26237f2478b49f84050526f18072c6560e8fba4e2ea
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/main.lfy:main#main:main:be6f670ec86351b9c898da8d21d0741aadee997be9c0c07bfeb4504ac8af962c
    assert!(
        log.lines().any(|line| line.contains("· relocated") && line.ends_with("rust: 1 entity, 2 markers, 1 review record")),
        "{log}"
    );
    // @lfy def/cli/main.lfy:main#main:main:2b2ecf83e7e057bba8be8cefb6ab9f0de30c74dfba2269df5b9fb4915457dec3
    assert!(fixture.read("requests.txt").contains("- `F` from `def/a.lfy`."), "{}", fixture.read("requests.txt"));
    // b's review carried the verdict on F's criterion.
    assert!(log.lines().any(|line| line.contains("✓ reviewed") && line.contains("  b  ") && line.ends_with("1 carried")), "{log}");
    // @lfy def/cli/main.lfy:main#main:main:56f0e92efcae2aa29056b323f26d89a551242862fc55f1b85a0c3fc607c66f75
    let output = fixture.read("src/a.rs");
    assert!(output.contains("// @lfy def/b.lfy:F\n") && output.contains(&format!("// @lfy def/b.lfy:F#{id}\n")), "{output}");
    assert!(fixture.read("elfie-compile/maps/rust/b.json").contains("src/a.rs"));
    assert!(fixture.read("elfie-compile/reviews/rust/b.json").contains(&id));
}

/// A copy and a base of a change set of no planned target, kept from an earlier compile,
/// are removed.
// @lfy def/cli/main.lfy:main#main:main:b42383729e83012724e5ae4eb03736c20f286e73e36e0bfb45b2affcd6832907
#[test]
fn a_kept_change_set_of_no_planned_target_is_removed() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    let copy = copy_of(&fixture.root, "rust");
    fs::create_dir_all(&copy).unwrap();
    let kept = ChangeSet {
        target: "rust".to_string(),
        directory: copy.to_string_lossy().into_owned(),
        units: Vec::new(),
        foreign_edits: Vec::new(),
    };
    kept.save(&fixture.root).unwrap();
    // @lfy def/cli/main.lfy:main#main:main:b42383729e83012724e5ae4eb03736c20f286e73e36e0bfb45b2affcd6832907
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert!(ChangeSet::load(&fixture.root, "rust").is_none());
    assert!(!copy.exists());
}
