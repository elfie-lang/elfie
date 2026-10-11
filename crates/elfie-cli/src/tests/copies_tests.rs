// @lfy def/cli/main.lfy:main
//! Tests of a batch's own copy of the root: what it holds, how its files are merged back,
//! what a batch takes from the batch before it, and the build and test that stand in for the
//! compiler.

use super::*;
use crate::copies::{Mirror, mode_of, set_mode};

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
