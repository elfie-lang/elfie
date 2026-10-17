// @lfy def/workspace/main.lfy:load
//! The tests of load.

use std::path::Path;

use super::fixtures::*;
use super::load::{BUILT_IN_LIBRARY, DEFAULT_SOURCE_DIRECTORY, MAIN_FILE, MANIFEST};
use super::*;
use crate::grammar::rules::statement::Statement;
use crate::lexer::lex;
use crate::model::{self, Operation, Origin};
use crate::parser::parse;

/// The identifiers of the packages of a workspace, in order.
fn identifiers(workspace: &Workspace) -> Vec<&str> {
    workspace
        .packages
        .iter()
        .map(|package| package.identifier.as_str())
        .collect()
}

/// The package of a workspace with an identifier.
fn package<'w>(workspace: &'w Workspace, identifier: &str) -> &'w Package {
    workspace
        .packages
        .iter()
        .find(|package| package.identifier == identifier)
        .unwrap_or_else(|| panic!("the package {identifier} is in the program"))
}

// @lfy def/workspace/main.lfy:load#load:load:d89e1a3dc76a0474ada8c0015af825a5f5e8c5e26f0b09d9280c88429bbcfba0
// @lfy def/workspace/main.lfy:load#load:load:932b815f66767e3f29654d009afd1f258dc9f22040a21b8770d2e7aa470350e2
// @lfy def/workspace/main.lfy:load#load:load:1c98732ad9dcd0ae1423690fc2bd117e7dbc13effc288ef7a15560f86a0d3064
// @lfy def/workspace/main.lfy:load#load:load:edc92a499ae2b8769762be0cc27e701b8a746a8ac3afd23f6816df3f92b21d46
// @lfy def/workspace/main.lfy:load#load:load:1e7fcb1331fd3ec876fe83870adde4b870652330e329c9ea4f8885b650877a3e
#[test]
fn an_empty_def_and_no_manifest_give_the_defaults() {
    let fixture = Fixture::empty();
    // Nothing names a library, so it is the copy the compiler was built with.
    // @lfy def/workspace/main.lfy:load
    let workspace = load_built_in(&fixture.root);
    assert_eq!(workspace.root, fixture.root); // @lfy def/workspace/main.lfy:load
    assert_eq!(
        workspace.name,
        fixture.root.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(workspace.source_directories, ["def"]);
    assert!(paths(&workspace).is_empty());
    assert!(workspace.targets.is_empty());
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert!(workspace.overlays.is_empty());
    // The package elfie is in the program all the same, holding the files of that
    // copy. @lfy def/workspace/main.lfy:load
    assert_eq!(identifiers(&workspace), [LIBRARY_PACKAGE]);
    let library = library(&workspace);
    assert_eq!(library.root, normalize(BUILT_IN_LIBRARY));
    assert!(Path::new(&library.root).is_absolute());
    let main = join(&library.root, MAIN_FILE);
    assert!(library_paths(&workspace).contains(&main.as_str()));
    assert!(library_paths(&workspace).len() > 1);
}

/// The library is the dependency entry elfie when that entry is a path, whether or not
/// its directory exists, and the copy the compiler was built with otherwise.
// @lfy def/workspace/main.lfy:load#load:load:1c98732ad9dcd0ae1423690fc2bd117e7dbc13effc288ef7a15560f86a0d3064
// @lfy def/workspace/main.lfy:load#load:load:05b8fa2948fcab3e2f28a0331ccd1f1bfbe7b52d62225256478e886cf3e30d7b
// @lfy def/workspace/main.lfy:load#load:load:edc92a499ae2b8769762be0cc27e701b8a746a8ac3afd23f6816df3f92b21d46
// @lfy def/workspace/main.lfy:load#load:load:b32dfca900b488d4e38fbc1eacf76d5247749ba7f635f7daba72260d40877a8d
#[test]
fn the_library_is_the_entry_elfie_when_it_is_a_path_and_else_the_built_in_copy() {
    // The dependency entry elfie, relative to the root.
    let fixture = Fixture::empty();
    fixture.write(
        "elfie.json",
        r#"{ "dependencies": { "elfie": "./byDependency/" } }"#,
    );
    fixture.library("byDependency");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert_eq!(library(&workspace).root, "byDependency");
    assert!(library_paths(&workspace).contains(&"byDependency/main.lfy"));

    // An absolute entry is the path itself.
    let fixture = Fixture::empty();
    let absolute = fixture.root.join("absolute").to_string_lossy().into_owned();
    fixture.write(
        "elfie.json",
        &serde_json::json!({ "dependencies": { "elfie": absolute } }).to_string(),
    );
    fixture.library("absolute");
    let workspace = fixture.load();
    assert_eq!(library(&workspace).root, absolute);
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);

    // No entry elfie: the built-in copy, which the fixture's own library stands for. The
    // key lib that earlier versions read is ignored.
    // @lfy def/workspace/main.lfy:load
    let fixture = Fixture::empty();
    fixture.write("elfie.json", r#"{ "lib": "byKey" }"#);
    fixture.library("byKey");
    let workspace = fixture.load();
    assert_eq!(library(&workspace).root, LIBRARY_ROOT);
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);

    // An entry elfie that is not a path: the built-in copy, and the entry is named.
    // @lfy def/workspace/main.lfy:load
    let fixture = Fixture::empty();
    fixture.write("elfie.json", r#"{ "dependencies": { "elfie": "1.0.0" } }"#);
    let workspace = fixture.load();
    assert_eq!(library(&workspace).root, LIBRARY_ROOT);
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path.as_deref(), Some(MANIFEST));
    assert!(problems[0].message.contains("\"elfie\""), "{}", problems[0]);

    // An entry whose directory is missing stands all the same, and is a problem.
    // @lfy def/workspace/main.lfy:load
    let fixture = Fixture::empty();
    fixture.write("elfie.json", r#"{ "dependencies": { "elfie": "./gone" } }"#);
    let workspace = fixture.load();
    assert_eq!(library(&workspace).root, "gone");
    assert!(library_paths(&workspace).is_empty());
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path.as_deref(), Some("gone"));
}

/// The main file of the package elfie is the prelude, its other files are the
/// library, and every file outside it is the program.
// @lfy def/workspace/main.lfy:load#load:load:1c98732ad9dcd0ae1423690fc2bd117e7dbc13effc288ef7a15560f86a0d3064
// @lfy def/workspace/main.lfy:load#load:load:42ddd4111d6e51a0395e9b1eee8c79a065bd9e53b143029640c9ed2dd233c078
// @lfy def/workspace/main.lfy:load#load:load:656f24a577ae1949e6a118618dbcf080a5f0418f80c6cbeda85d2792cb3f027e
// @lfy def/workspace/main.lfy:load#load:load:b4153a74d5f8efd66fd596d740c535d52bbb76f23a9f3d3592456586dfa3f4cc
#[test]
fn the_library_gives_the_prelude_and_every_other_file_the_program() {
    let fixture = Fixture::empty();
    fixture
        .write("elfie.json", r#"{ "dependencies": { "elfie": "./lib" } }"#)
        .write("def/main.lfy", "d Marked is target { }\n")
        .write("lib/values/list.lfy", "d List { }\n");
    let workspace = load_built_in(&fixture.root);
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert_eq!(library(&workspace).root, "lib");
    let library_files: Vec<&File> = workspace
        .files
        .iter()
        .filter(|file| file.path.starts_with("lib/"))
        .collect();
    assert!(library_files.len() > 1);
    assert!(
        library_files
            .iter()
            .all(|file| file.package == Some(workspace.packages.iter().position(|package| package.identifier == LIBRARY_PACKAGE).unwrap()))
    );
    let origin = |path: &str| {
        let file = workspace.file(path).expect("a file of the program");
        workspace.origin(file)
    };
    assert_eq!(origin("lib/main.lfy"), Origin::Prelude);
    assert_eq!(origin("lib/values/list.lfy"), Origin::Library);
    assert_eq!(origin("def/main.lfy"), Origin::Program);
    // The prelude scope is the file scope of that main file, so a project file sees
    // what it declares without a use. @lfy def/workspace/main.lfy:load
    let project = workspace.file("def/main.lfy").unwrap().source;
    assert_eq!(
        workspace.model.scopes[workspace.model.file_scopes[project]].parent,
        Some(workspace.model.file_scopes[workspace.file("lib/main.lfy").unwrap().source])
    );
}

// @lfy def/workspace/main.lfy:load#load:load:34af94d435a344094a311c95695780e09cd2be7f386baa616feb1b6df6ca36d0
// @lfy def/workspace/main.lfy:load#load:load:0fac45ffcbab6307a0cadab9357862b79b40b58f5064ecf56d4f03ef03659a37
// @lfy def/workspace/main.lfy:load#load:load:97bed938a5d75f39ac08d69a7e0ac6d7a4a2fd040d8d2d4ffa31571a4bc63d87
// @lfy def/workspace/main.lfy:load#load:load:95adb658d370404e9a9733a98206f9323b5efdaf143f4e927a012d32b58d95c3
#[test]
fn a_used_file_comes_before_the_file_that_uses_it() {
    let fixture = Fixture::empty();
    fixture
        .write("def/main.lfy", "use \"./rules\";\n")
        .write("def/rules/main.lfy", "trait rule { }\n");
    let workspace = fixture.load();
    assert_eq!(paths(&workspace), ["def/rules/main.lfy", "def/main.lfy"]);
    assert!(project(&workspace).all(|file| file.package.is_none()));
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert_eq!(
        uses_of(&workspace, "def/main.lfy"),
        [Some("def/rules/main.lfy".to_string())]
    );
    assert_eq!(uses_of(&workspace, "def/rules/main.lfy"), []);
}

// @lfy def/workspace/main.lfy:load#load:load:b2116810fef16c47850eccd77150fd52cf7c716a41be87bfbac49d5ca7531a91
// @lfy def/workspace/main.lfy:load#load:load:b1fbb044be3c701778d0f3eed4d4ad278cdf34c2b917c9b0a33327367959d921
// @lfy def/workspace/main.lfy:load#load:load:5e03556496a99f7d2d13c4effb8ae2e0d1208153ea90ec6e70a7592ef184a1d5
#[test]
fn a_cycle_keeps_discovery_order_and_flags_each_use() {
    let fixture = Fixture::empty();
    fixture
        .write("def/a.lfy", "use \"./b\";\n")
        .write("def/b.lfy", "use \"./a\";\n");
    let workspace = fixture.load();
    assert_eq!(paths(&workspace), ["def/a.lfy", "def/b.lfy"]);
    assert_eq!(load_problems(&workspace), Vec::<&LoadProblem>::new());
    // One `Problem` at each `Use` of the cycle, of the stage that added it.
    // @lfy def/workspace/main.lfy:load#load:load:b1fbb044be3c701778d0f3eed4d4ad278cdf34c2b917c9b0a33327367959d921
    // @lfy def/workspace/main.lfy:load#load:load:83c8947a99852c5cfe9afc540029580ba37bedf5bfe20bba7ed77e79ebb98642
    let problems = bind_problems(&workspace);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems
            .iter()
            .all(|problem| problem.stage == crate::model::Stage::Loader),
        "{problems:?}"
    );
    assert_eq!(at_use(&workspace, problems[0]), ("def/a.lfy", true));
    assert_eq!(problems[0].message, "use \"./b\" is part of a cycle");
    assert_eq!(at_use(&workspace, problems[1]), ("def/b.lfy", true));
    assert_eq!(problems[1].message, "use \"./a\" is part of a cycle");
    // The uses still resolve; only the order could not honor them.
    assert_eq!(
        uses_of(&workspace, "def/a.lfy"),
        [Some("def/b.lfy".to_string())]
    );
    assert_eq!(
        uses_of(&workspace, "def/b.lfy"),
        [Some("def/a.lfy".to_string())]
    );
}

// @lfy def/workspace/main.lfy:load#load:load:901344cabb4e3f5163bfc6613edfc6a6cc386eed876768587fad9c1f451b61b0
// @lfy def/workspace/main.lfy:load#load:load:5d9353c2d7ef127d55101933cf2d1581e1bf8dfae943a0d3d59cf83a5cb17679
#[test]
fn a_use_of_nothing_gives_a_problem_and_an_undefined_entry() {
    let fixture = Fixture::empty();
    fixture.write("def/main.lfy", "\nuse \"./missing\";\n");
    let workspace = fixture.load();
    assert_eq!(paths(&workspace), ["def/main.lfy"]);
    assert_eq!(load_problems(&workspace), Vec::<&LoadProblem>::new());
    // One `Problem` at the `Use`, of the stage that added it.
    // @lfy def/workspace/main.lfy:load#load:load:5d9353c2d7ef127d55101933cf2d1581e1bf8dfae943a0d3d59cf83a5cb17679
    // @lfy def/workspace/main.lfy:load#load:load:83c8947a99852c5cfe9afc540029580ba37bedf5bfe20bba7ed77e79ebb98642
    let problems = bind_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].stage, crate::model::Stage::Loader);
    assert_eq!(at_use(&workspace, problems[0]), ("def/main.lfy", true));
    assert!(
        problems[0]
            .message
            .starts_with("use \"./missing\" resolves to nothing"),
        "{}",
        problems[0]
    );
    assert_eq!(uses_of(&workspace, "def/main.lfy"), [None]);
}

// @lfy def/workspace/main.lfy:load#load:load:a473c608f5a70cc064c8635bd090121d715b920b42ee0fcbe41e3c3a75fdd9d8
// @lfy def/workspace/main.lfy:load#load:load:3fcd57f4ebc2aa8bc9b9f2c2fb00087ebaa95f76f3c93971df4f0a7c72e2cd09
// @lfy def/workspace/main.lfy:load#load:load:d2500ef79b0f845d62d879e55d20851d45ebfa885fa1e7397a17ab04f6f7422d
#[test]
fn the_configuration_names_the_layout() {
    let fixture = Fixture::new();
    fixture
        .write(
            "elfie.json",
            r#"{ "name": "named", "version": "1.0.0", "source": "source/" }"#,
        )
        .write("source/main.lfy", "trait t { }\n");
    let workspace = fixture.load();
    assert_eq!(
        workspace.configuration,
        Configuration {
            name: Some("named".to_string()),
            version: Some("1.0.0".to_string()),
            source: Some(ConfigurationSource::One("source/".to_string())),
            dependencies: None,
            exports: None,
        }
    );
    // @lfy def/workspace/main.lfy:load
    assert_eq!(workspace.name, "named");
    // @lfy def/workspace/main.lfy:load
    assert_eq!(workspace.source_directories, ["source/"]);
    assert_eq!(paths(&workspace), ["source/main.lfy"]);
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
}

/// A configuration that gives no name and no source leaves each of those defaults
/// standing, exactly as a root with no `elfie.json` at all does.
// @lfy def/workspace/main.lfy:load#load:load:8d7ba6d4fe1f04154ef1330dfbe2a0ed619e5674a06aef6b93cf3af417afa0f1
// @lfy def/workspace/main.lfy:load#load:load:bc81bdfb69e2efbe2ffcca801028530c7cbc5a41ff47a15eed0784e947b566cd
#[test]
fn a_configuration_that_gives_no_layout_leaves_the_defaults_standing() {
    let fixture = Fixture::empty();
    fixture
        .write("elfie.json", r#"{ "lib": "lib" }"#)
        .write("def/main.lfy", "");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert_eq!(workspace.configuration, Configuration::default());
    // The name of the root directory, since elfie.json gives no name.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        workspace.name,
        fixture.root.file_name().unwrap().to_string_lossy()
    );
    // def alone, since elfie.json gives no source.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(workspace.source_directories, [DEFAULT_SOURCE_DIRECTORY]);
    assert_eq!(paths(&workspace), ["def/main.lfy"]);
}

/// An `elfie.json` is optional: where none exists at the root or at a package's root,
/// every key of that configuration is undefined and no problem is added.
// @lfy def/workspace/main.lfy:load#load:load:a450cc5b0bb372bd3f2b9fea6f4218845a10c800e1f81ad8274d5cfb882516f4
#[test]
fn no_configuration_is_no_problem_and_the_defaults_stand() {
    let fixture = Fixture::empty();
    fixture
        .write("elfie.json", r#"{ "dependencies": { "p": "./pkg" } }"#)
        .write("def/main.lfy", "")
        .write("pkg/main.lfy", "");
    assert!(!fixture.root.join("pkg").join(MANIFEST).exists());
    let workspace = fixture.load();
    assert_eq!(package(&workspace, "p").configuration, Configuration::default());
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);

    let fixture = Fixture::empty();
    fixture.write("def/main.lfy", "");
    assert!(!fixture.root.join(MANIFEST).exists());
    let workspace = fixture.load();
    assert_eq!(workspace.configuration, Configuration::default());
    assert_eq!(
        workspace.name,
        fixture.root.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(workspace.source_directories, [DEFAULT_SOURCE_DIRECTORY]);
    assert_eq!(paths(&workspace), ["def/main.lfy"]);
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
}

// @lfy def/workspace/main.lfy:load#load:load:a4feb706de87193af1f3285b8bd67c396deb25f48611db7160a8930ed6894335
// @lfy def/workspace/main.lfy:load#load:load:646df2728f9636f2d0fc69773cfe790b426a6d2eb573efedecea7c9ec800aac2
#[test]
fn a_bad_configuration_adds_a_problem_and_the_defaults_stand() {
    for text in ["{\"name\": \"x\",", "{ not json", "[1, 2]", "\"a string\""] {
        let fixture = Fixture::empty();
        fixture.write("elfie.json", text).write("def/main.lfy", "");
        let workspace = fixture.load();
        let problems = load_problems(&workspace);
        assert_eq!(problems.len(), 1, "{text}: {problems:?}");
        assert_eq!(problems[0].path.as_deref(), Some("elfie.json"));
        assert_eq!(workspace.configuration, Configuration::default());
        assert_eq!(
            workspace.name,
            fixture.root.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(workspace.source_directories, ["def"]);
        assert_eq!(paths(&workspace), ["def/main.lfy"]);
    }
    // The same for the elfie.json of a package, whose files still load.
    let fixture = Fixture::empty();
    fixture
        .write("elfie.json", r#"{ "dependencies": { "p": "./pkg" } }"#)
        .write("pkg/elfie.json", "[]")
        .write("pkg/main.lfy", "");
    let workspace = fixture.load();
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path.as_deref(), Some("pkg/elfie.json"));
    assert_eq!(package(&workspace, "p").configuration, Configuration::default());
    assert!(paths(&workspace).contains(&"pkg/main.lfy"));
}

/// Targets, their outputs, and their dependencies are declared in the program, so a
/// configuration in the shape earlier versions read loads with no problem at all: the
/// keys `Configuration` does not declare are ignored.
// @lfy def/workspace/main.lfy:load#load:load:9955f71e445330d81b0395728fbe576a039b18e3bb889f7d498edbbcd4599dbf
// @lfy def/workspace/main.lfy:load#load:load:9bf0f0938acd565776218f4c5d6a4434294c537b7873cc8a1e3490f8a5103f94
#[test]
fn a_key_the_configuration_does_not_declare_is_ignored_and_adds_no_problem() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{
                    "name": "named",
                    "targets": { "t": { "package": "p", "marker": "m" } },
                    "output": "crates",
                    "native": [ { "identifier": "serde", "ecosystem": "cargo" } ],
                    "dependencies": { "p": "./pkg" },
                    "compiler": "scripts/compile.sh"
                }"#,
        )
        .write("def/main.lfy", "")
        .write("pkg/main.lfy", "");
    let workspace = fixture.load();
    // The keys it declares still stand.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(workspace.name, "named");
    let mut sorted = paths(&workspace);
    sorted.sort_unstable();
    assert_eq!(sorted, ["def/main.lfy", "pkg/main.lfy"]);
    // The key targets is ignored, so the program declares every target.
    // @lfy def/workspace/main.lfy:load
    assert!(workspace.targets.is_empty());
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
}

/// Loading never stops for a failure: a key whose value is of another type than the one
/// `Configuration` declares is named with the type it should have and left undefined, and
/// every other key is read all the same.
// @lfy def/workspace/main.lfy:load#load:load:fe6fb32b63154e5b9396448cbce2410f62993bbac3c4be6a7db55d8a2f7d0d28
// @lfy def/workspace/main.lfy:load#load:load:7bf7f7320ea74ff2dd0d08e518d364c8292228a597b9360c3ab0cf7a4178d8b7
// @lfy def/workspace/main.lfy:load#load:load:4fa9b901c6befec4fa20bfa6d19b5371ef837153bfacb6b980b1b0cb23139e3e
#[test]
fn a_key_of_another_type_is_named_and_undefined_and_the_rest_is_read() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{
                    "name": 3,
                    "version": "2.0.0",
                    "source": [ "def", 4 ],
                    "dependencies": { "p": "./pkg", "q": 4 },
                    "exports": [ "ui" ]
                }"#,
        )
        .write("def/main.lfy", "")
        .write("pkg/main.lfy", "");
    let workspace = fixture.load();
    let problems = load_problems(&workspace);
    // Each one names elfie.json, the key, and the type it should have.
    // @lfy def/workspace/main.lfy:load
    assert!(
        problems
            .iter()
            .all(|problem| problem.path.as_deref() == Some("elfie.json")),
        "{problems:?}"
    );
    for (key, wanted) in [
        ("\"name\"", "a string"),
        ("\"source\"", "a string or a list of strings"),
        ("\"dependencies\"", "an object of strings"),
        ("\"exports\"", "an object of strings"),
    ] {
        assert!(
            problems
                .iter()
                .any(|problem| problem.message.contains(key)
                    && problem.message.contains(wanted)),
            "{key}: {problems:?}"
        );
    }
    assert_eq!(problems.len(), 4, "{problems:?}");
    // Each such key is undefined, and the one that is of its type stands.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        workspace.configuration,
        Configuration {
            version: Some("2.0.0".to_string()),
            ..Configuration::default()
        }
    );
    assert_eq!(
        workspace.name,
        fixture.root.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(workspace.source_directories, [DEFAULT_SOURCE_DIRECTORY]);
    // With no dependencies, the package p is not in the program.
    assert_eq!(identifiers(&workspace), [LIBRARY_PACKAGE]);
    assert_eq!(paths(&workspace), ["def/main.lfy"]);
}

// @lfy def/workspace/main.lfy:load#load:load:f97a7916ef1a5ec5f8b25e9caca0c7851b745165411bc7008519899125882bac
#[test]
fn a_missing_root_adds_a_problem_with_no_path_and_no_files() {
    let fixture = Fixture::new();
    let missing = fixture.root.join("nowhere");
    let workspace = load_built_in(&missing);
    assert_eq!(workspace.root, missing);
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, None);
    assert!(workspace.files.is_empty());
}

/// A missing source directory is named, no file comes from it, and the other source
/// directories and the packages still load.
// @lfy def/workspace/main.lfy:load#load:load:0fc20a5c6ed1f12fda66543903de28702cb27892cc578d83c6316902500fef99
#[test]
fn a_missing_source_directory_is_named_and_the_rest_still_loads() {
    let fixture = Fixture::new();
    fixture
        .write(
            "elfie.json",
            r#"{ "source": ["def", "src"], "dependencies": { "p": "./pkg" } }"#,
        )
        .write("src/a.lfy", "")
        .write("pkg/main.lfy", "");
    let workspace = fixture.load();
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path.as_deref(), Some("def"));
    let mut sorted = paths(&workspace);
    sorted.sort_unstable();
    assert_eq!(sorted, ["pkg/main.lfy", "src/a.lfy"]);
}

// @lfy def/workspace/main.lfy:load#load:load:a7bb1ab332ef59bd7ef0b193ca2257a60b8457f57538d11c2fd2c2f0c2610ea5
// @lfy def/workspace/main.lfy:load#load:load:912de90eec4ab7a10c1d9fb7853c226b0130e0f61558b54793fccef20e344b08
// @lfy def/workspace/main.lfy:load#load:load:23324d830b23ca1be67dea3c9234abb1945519a3e56b47c394222debdf328fe8
// @lfy def/workspace/main.lfy:load#load:load:c4acf67876f96707cb66e2a889f79373ce852c3192472acb1b3dbff88d3d190a
// @lfy def/workspace/main.lfy:load#load:load:34b900cfd918601b8e0fbd06775f5015712e20b8edddc83c91a156a80e1e26ff
#[test]
fn every_lfy_file_under_the_source_directory_is_in_the_program_once() {
    let fixture = Fixture::empty();
    fixture
        .write("def/main.lfy", "use \"./deep/inner\";\n")
        .write("def/deep/inner.lfy", "")
        .write("def/deep/other.lfy", "use \"./inner\";\n")
        .write("def/notes.txt", "not source")
        .write("src/main.lfy", "outside the source directory");
    let workspace = fixture.load();
    let mut sorted = paths(&workspace);
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        ["def/deep/inner.lfy", "def/deep/other.lfy", "def/main.lfy"]
    );
    // @lfy def/workspace/main.lfy:load#load:load:c4acf67876f96707cb66e2a889f79373ce852c3192472acb1b3dbff88d3d190a
    assert_eq!(workspace.files.len(), workspace.model.sources.len());
    for (index, file) in workspace.files.iter().enumerate() {
        assert_eq!(file.source, index);
        assert_eq!(workspace.model.sources[index].path, file.path);
    }
    assert!(project(&workspace).all(|file| file.package.is_none()));
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
}

/// Every source directory gives its files, each directory once, and each file of the
/// project is named by its path within its source directory, taking as many more of its
/// leading directories as it needs where two source directories hold the same path.
// @lfy def/workspace/main.lfy:load#load:load:34b900cfd918601b8e0fbd06775f5015712e20b8edddc83c91a156a80e1e26ff
// @lfy def/workspace/main.lfy:load#load:load:95ee4c371559a37196b70b48eb49a98edb20b303a2fad0cdde57142a357b6b38
// @lfy def/workspace/main.lfy:load#load:load:83585aea4fc7bf0a367ab795152302d1d333212f48b23d1c5285dd457b336215
// @lfy def/workspace/main.lfy:load#load:load:b9fa6ce6f09e94bb3ac78519ff241efe16838c62a8adcb86c40e0a3f8106f5bf
#[test]
fn several_source_directories_give_their_files_and_names() {
    let fixture = Fixture::new();
    fixture
        .write("elfie.json", r#"{ "source": ["def", "src", "def"] }"#)
        .write("def/a.lfy", "")
        .write("def/b.lfy", "")
        .write("def/x/a.lfy", "")
        .write("src/a.lfy", "")
        .write("src/x/a.lfy", "");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    // @lfy def/workspace/main.lfy:load
    assert_eq!(workspace.source_directories, ["def", "src"]);
    // @lfy def/workspace/main.lfy:load
    let mut names: Vec<(&str, &str)> = project(&workspace)
        .map(|file| (file.path.as_str(), file.name.as_str()))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            ("def/a.lfy", "def/a"),
            ("def/b.lfy", "b"),
            ("def/x/a.lfy", "def/x/a"),
            ("src/a.lfy", "src/a"),
            ("src/x/a.lfy", "src/x/a"),
        ]
    );
    assert!(project(&workspace).all(|file| file.package.is_none()));
}

/// A file of a project with one source directory is named by its path within it; a file
/// of a package by the package's identifier and its path within the package's root.
// @lfy def/workspace/main.lfy:load#load:load:f3f04a799f9fa89db29eed53cf6239c78d5d4389948995d43fe22b84eba0cbdc
// @lfy def/workspace/main.lfy:load#load:load:4e242171bbac221a9235f0dc2f2b41c8ce9461c40c25cecf4661dd81cdd32704
#[test]
fn a_file_is_named_by_its_path_within_its_source_directory_or_its_package() {
    let fixture = Fixture::empty();
    fixture
        .write("elfie.json", r#"{ "dependencies": { "p": "./pkg" } }"#)
        .write("def/main.lfy", "")
        .write("def/rules/main.lfy", "")
        .write("pkg/deep/thing.lfy", "");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    let name = |path: &str| workspace.file(path).expect("a file of the program").name.as_str();
    // @lfy def/workspace/main.lfy:load
    assert_eq!(name("def/main.lfy"), "main");
    assert_eq!(name("def/rules/main.lfy"), "rules/main");
    // @lfy def/workspace/main.lfy:load
    assert_eq!(name("pkg/deep/thing.lfy"), "p/deep/thing");
    assert_eq!(name("lib/main.lfy"), "elfie/main");
}

// @lfy def/workspace/main.lfy:load#load:load:692ecc7501d808fc5c758f804195b305d9436a6d7de2ce76f48185ac315cfdb7
#[test]
fn a_file_holds_the_parse_of_its_text_with_its_path_as_the_file() {
    let fixture = Fixture::empty();
    let text = "trait hasName { $name = string; }\n";
    fixture.write("def/main.lfy", text);
    let workspace = fixture.load();
    let source = &workspace.model.sources[workspace.file("def/main.lfy").unwrap().source];
    let expected = parse(lex(text, Some("def/main.lfy")).unwrap(), None);
    assert_eq!(source.tree, expected);
    assert_eq!(&*source.tree.tokens[0].file, "def/main.lfy");
    assert!(source.tree.root.find(Statement::TraitDeclaration).is_some());
    assert!(
        source
            .tree
            .root
            .is(crate::grammar::rules::file::File::SourceFile)
    );
}

// @lfy def/workspace/main.lfy:load#load:load:36d73adceb24794b868cfec5c1c2f1bb0befd1aee1a502407f8158d70d49ee14
#[test]
fn a_file_that_cannot_be_read_is_left_out_with_a_problem() {
    let fixture = Fixture::empty();
    fixture
        .write("def/main.lfy", "use \"./bad\";\n")
        .write_bytes("def/bad.lfy", &[0xff, 0xfe, b'x']);
    let workspace = fixture.load();
    assert_eq!(paths(&workspace), ["def/main.lfy"]);
    let problems = load_problems(&workspace);
    assert!(
        problems
            .iter()
            .any(|problem| problem.path.as_deref() == Some("def/bad.lfy")),
        "{problems:?}"
    );
    // Nothing in the program may refer to a file that is not in it, so the use of it
    // resolves to nothing. @lfy def/workspace/main.lfy:load
    assert_eq!(uses_of(&workspace, "def/main.lfy"), [None]);
    let problems = bind_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(at_use(&workspace, problems[0]), ("def/main.lfy", true));
}

// @lfy def/workspace/main.lfy:load#load:load:7cd3d4046cd7170c71b67d6747111258f280a6b304e31b968c1adf210b007213
#[test]
fn a_dotted_use_resolves_against_the_directory_of_its_file() {
    let fixture = Fixture::empty();
    fixture
        .write("def/a/main.lfy", "use \"../b/data\";\nuse \"./data\";\n")
        .write("def/a/data.lfy", "")
        .write("def/b/data.lfy", "");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert_eq!(
        uses_of(&workspace, "def/a/main.lfy"),
        [
            Some("def/b/data.lfy".to_string()),
            Some("def/a/data.lfy".to_string())
        ]
    );
    assert!(position(&workspace, "def/a/data.lfy") < position(&workspace, "def/a/main.lfy"));
    assert!(position(&workspace, "def/b/data.lfy") < position(&workspace, "def/a/main.lfy"));
}

// @lfy def/workspace/main.lfy:load#load:load:5d9353c2d7ef127d55101933cf2d1581e1bf8dfae943a0d3d59cf83a5cb17679
// @lfy def/workspace/main.lfy:load#load:load:a7bb1ab332ef59bd7ef0b193ca2257a60b8457f57538d11c2fd2c2f0c2610ea5
// @lfy def/workspace/main.lfy:load#load:load:99f8a9365b4ddf9b26525fc56b9850112f86430a002df60a6c1453a745a29f40
#[test]
fn an_undotted_use_names_a_package_and_resolves_against_its_root() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "rust": "./targets/rust", "rust/extra": "./targets/extra" } }"#,
        )
        .write(
            "def/main.lfy",
            "use \"rust/guidance\";\nuse \"rust\";\nuse \"nope/thing\";\n\
             use \"rust/extra/more\";\nuse \"elfie/main\";\n",
        )
        .write("targets/rust/main.lfy", "use \"./guidance\";\n")
        .write("targets/rust/guidance.lfy", "")
        .write("targets/extra/more.lfy", "");
    let workspace = fixture.load();
    // The longest run of leading segments that names a package wins, and elfie always
    // names the package elfie. @lfy def/workspace/main.lfy:load
    assert_eq!(
        uses_of(&workspace, "def/main.lfy"),
        [
            Some("targets/rust/guidance.lfy".to_string()),
            Some("targets/rust/main.lfy".to_string()),
            None,
            Some("targets/extra/more.lfy".to_string()),
            Some("lib/main.lfy".to_string()),
        ]
    );
    assert_eq!(
        uses_of(&workspace, "targets/rust/main.lfy"),
        [Some("targets/rust/guidance.lfy".to_string())]
    );
    assert_eq!(load_problems(&workspace), Vec::<&LoadProblem>::new());
    // @lfy def/workspace/main.lfy:load
    let problems = bind_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(at_use(&workspace, problems[0]), ("def/main.lfy", true));
    assert!(
        problems[0].message.contains("\"nope/thing\""),
        "{}",
        problems[0]
    );
    assert!(
        problems[0].message.contains("no package is named \"nope\""),
        "{}",
        problems[0]
    );
}

/// A file of a package names the dependency entries of its package's `elfie.json` and
/// never those of the project's; entries that give one root give one package; and the
/// entry elfie of a package is ignored, elfie naming the one package elfie all the same.
// @lfy def/workspace/main.lfy:load#load:load:a3421e175ba0d8b9bce4fec7e2b2217ba83e5c91e3304b1f4fc241e3a1c7b66a
// @lfy def/workspace/main.lfy:load#load:load:488bdb4d999605f20285df1be26700cce5e26fe51808ce72c77e2e0e74231548
// @lfy def/workspace/main.lfy:load#load:load:d5d5894b6e1a0b68f69a3b71f975fed7a26b6466fdb05517e2dcb72a5888d1a5
// @lfy def/workspace/main.lfy:load#load:load:48288ac53c4ce16476eae28d86777ffa3592a5192db841bcde3e89dd31724f71
// @lfy def/workspace/main.lfy:load#load:load:56743e9f562e649dc3d6042b20e25fdf4744ba98e77150ff1c635cead47157cb
#[test]
fn a_file_of_a_package_names_the_entries_of_its_package() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "a": "./a", "b": "./b", "same": "./a/" } }"#,
        )
        .write(
            "a/elfie.json",
            r#"{ "dependencies": { "inner": "./inner", "bee": "../b", "elfie": "1.0.0" } }"#,
        )
        .write(
            "a/main.lfy",
            "use \"inner/x\";\nuse \"bee\";\nuse \"elfie/main\";\nuse \"a\";\n",
        )
        .write("a/inner/x.lfy", "")
        .write("b/main.lfy", "")
        .write("def/main.lfy", "use \"same\";\nuse \"inner/x\";\n");
    let workspace = fixture.load();
    assert_eq!(load_problems(&workspace), Vec::<&LoadProblem>::new());
    // One package per root, identified by the first entry that gave it.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(identifiers(&workspace), [LIBRARY_PACKAGE, "a", "b", "inner"]);
    assert_eq!(package(&workspace, "inner").root, "a/inner");
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        uses_of(&workspace, "def/main.lfy"),
        [Some("a/main.lfy".to_string()), None]
    );
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        uses_of(&workspace, "a/main.lfy"),
        [
            Some("a/inner/x.lfy".to_string()),
            Some("b/main.lfy".to_string()),
            Some("lib/main.lfy".to_string()),
            None,
        ]
    );
    let mut at: Vec<(&str, bool)> = bind_problems(&workspace)
        .into_iter()
        .map(|problem| at_use(&workspace, problem))
        .collect();
    at.sort_unstable();
    assert_eq!(at, [("a/main.lfy", true), ("def/main.lfy", true)]);
}

/// A use through a package's name follows the longest export key its next segments begin
/// with, even where a file of the package has the path the key spells; an export that
/// names nothing is a problem for the `elfie.json` holding it.
// @lfy def/workspace/main.lfy:load#load:load:19b062c31b01faa188b4bbc0532bdbf5c39c0e25ae249b283d34e4476fc8821d
// @lfy def/workspace/main.lfy:load#load:load:1656e6daf35e6938e3d1f828fb76d318f57cf16a19a3f42543ba0054030bea5d
#[test]
fn a_use_through_a_package_follows_its_exports() {
    let fixture = Fixture::empty();
    fixture
        .write("elfie.json", r#"{ "dependencies": { "@acme/kit": "./kit" } }"#)
        .write(
            "kit/elfie.json",
            r#"{ "exports": { "ui": "./src/ui", "ui/deep": "./src/deeper", "gone": "./nowhere" } }"#,
        )
        .write("kit/src/ui/main.lfy", "")
        .write("kit/src/deeper/thing.lfy", "")
        .write("kit/ui.lfy", "")
        .write(
            "def/main.lfy",
            "use \"@acme/kit/ui\";\nuse \"@acme/kit/ui/deep/thing\";\n",
        );
    let workspace = fixture.load();
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        uses_of(&workspace, "def/main.lfy"),
        [
            Some("kit/src/ui/main.lfy".to_string()),
            Some("kit/src/deeper/thing.lfy".to_string()),
        ]
    );
    assert_eq!(bind_problems(&workspace), Vec::<&crate::model::Problem>::new());
    // @lfy def/workspace/main.lfy:load
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path.as_deref(), Some("kit/elfie.json"));
    assert!(problems[0].message.contains("\"gone\""), "{}", problems[0]);
}

// @lfy def/workspace/main.lfy:load#load:load:f3f04a799f9fa89db29eed53cf6239c78d5d4389948995d43fe22b84eba0cbdc
// @lfy def/workspace/main.lfy:load#load:load:0c8b488d37fb7158b79608cabf8dce157d6af7cad8a6de2e7576d0a25a42cb80
#[test]
fn a_use_of_an_export_resolves_into_its_package() {
    let fixture = Fixture::empty();
    fixture
        .write("elfie.json", r#"{ "dependencies": { "@acme/kit": "./kit" } }"#)
        .write("kit/elfie.json", r#"{ "exports": { "ui": "./src/ui" } }"#)
        .write("kit/src/ui/main.lfy", "")
        .write("def/main.lfy", "use \"@acme/kit/ui\";\n");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert_eq!(
        uses_of(&workspace, "def/main.lfy"),
        [Some("kit/src/ui/main.lfy".to_string())]
    );
    let file = workspace.file("kit/src/ui/main.lfy").expect("a file of the program");
    assert_eq!(
        file.package
            .map(|package| workspace.packages[package].identifier.as_str()),
        Some("@acme/kit")
    );
    // @lfy def/workspace/main.lfy:load
    assert_eq!(file.name, "@acme/kit/src/ui/main");
}

// @lfy def/workspace/main.lfy:load#load:load:360d6474f5e8db42a54f9151cc373d40d39e3e4eb4109f0445a4e06a7160d825
// @lfy def/workspace/main.lfy:load#load:load:0ddc30be18742fce83c1bc733c6db516b1f02d4e58205740f7b26d57638aef3a
// @lfy def/workspace/main.lfy:load#load:load:97bed938a5d75f39ac08d69a7e0ac6d7a4a2fd040d8d2d4ffa31571a4bc63d87
#[test]
fn a_path_resolves_to_the_file_or_else_to_main_in_the_directory() {
    let fixture = Fixture::empty();
    fixture
        .write("def/main.lfy", "use \"./both\";\nuse \"./only\";\n")
        .write("def/both.lfy", "")
        .write("def/both/main.lfy", "")
        .write("def/only/main.lfy", "");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        uses_of(&workspace, "def/main.lfy"),
        [
            Some("def/both.lfy".to_string()),
            Some("def/only/main.lfy".to_string())
        ]
    );
}

// @lfy def/workspace/main.lfy:load#load:load:95adb658d370404e9a9733a98206f9323b5efdaf143f4e927a012d32b58d95c3
#[test]
fn every_file_comes_after_the_files_it_uses() {
    let fixture = Fixture::empty();
    fixture
        .write("def/a.lfy", "use \"./c\";\nuse \"./b\";\n")
        .write("def/b.lfy", "use \"./c\";\nuse \"./d\";\n")
        .write("def/c.lfy", "use \"./d\";\n")
        .write("def/d.lfy", "")
        .write("def/e.lfy", "");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert_eq!(paths(&workspace).len(), 5);
    for file in &workspace.files {
        for used in uses_of(&workspace, &file.path).iter().flatten() {
            assert!(
                position(&workspace, used) < position(&workspace, &file.path),
                "{used} must come before {}: {:?}",
                file.path,
                paths(&workspace)
            );
        }
    }
}

// @lfy def/workspace/main.lfy:load#load:load:b1fbb044be3c701778d0f3eed4d4ad278cdf34c2b917c9b0a33327367959d921
#[test]
fn a_file_using_itself_is_a_cycle_of_one() {
    let fixture = Fixture::empty();
    fixture
        .write("def/main.lfy", "use \"./main\";\n")
        .write("def/other.lfy", "use \"./main\";\n");
    let workspace = fixture.load();
    assert_eq!(paths(&workspace), ["def/main.lfy", "def/other.lfy"]);
    assert_eq!(load_problems(&workspace), Vec::<&LoadProblem>::new());
    let problems = bind_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(at_use(&workspace, problems[0]), ("def/main.lfy", true));
}

// @lfy def/workspace/main.lfy:load#load:load:f0c460549fc5debac126dc93031902458c9a539417d42b3b143b29ce41672943
// @lfy def/workspace/main.lfy:load#load:load:05b8fa2948fcab3e2f28a0331ccd1f1bfbe7b52d62225256478e886cf3e30d7b
// @lfy def/workspace/main.lfy:load#load:load:6b1ef1b2e8a6e582f3257d3e95d4b94c630efd29f5ec5856edbd629ab7a6ee53
#[test]
fn each_path_dependency_gives_a_package_whose_files_are_in_the_program() {
    let fixture = Fixture::empty();
    let absolute = fixture.root.join("elsewhere").to_string_lossy().into_owned();
    fixture
        .write(
            "elfie.json",
            &serde_json::json!({
                "dependencies": {
                    "rust": "./targets/rust/",
                    "gone": "./targets/gone",
                    "far": absolute,
                }
            })
            .to_string(),
        )
        .write("def/main.lfy", "")
        .write("targets/rust/main.lfy", "")
        .write("targets/rust/extra/thing.lfy", "");
    fs_dir(&fixture, "elsewhere");
    let workspace = fixture.load();
    assert_eq!(workspace.packages.len(), 4); // @lfy def/workspace/main.lfy:load
    let rust = workspace
        .packages
        .iter()
        .position(|package| package.identifier == "rust")
        .unwrap();
    // @lfy def/workspace/main.lfy:load
    assert_eq!(workspace.packages[rust].root, "targets/rust");
    assert_eq!(package(&workspace, "far").root, absolute);
    // @lfy def/workspace/main.lfy:load
    let mut sorted = paths(&workspace);
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        [
            "def/main.lfy",
            "targets/rust/extra/thing.lfy",
            "targets/rust/main.lfy"
        ]
    );
    assert_eq!(workspace.file("def/main.lfy").unwrap().package, None);
    assert_eq!(
        workspace.file("targets/rust/main.lfy").unwrap().package,
        Some(rust)
    );
    assert_eq!(
        workspace
            .file("targets/rust/extra/thing.lfy")
            .unwrap()
            .package,
        Some(rust)
    );
    // @lfy def/workspace/main.lfy:load
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path.as_deref(), Some("targets/gone"));
}

/// An empty directory at a path under a fixture's root.
fn fs_dir(fixture: &Fixture, path: &str) {
    std::fs::create_dir_all(fixture.root.join(path)).unwrap();
}

/// A dependency entry is a path, a git URL, or a version; only a path gives a package,
/// and each of the others is named for the `elfie.json` holding it.
// @lfy def/workspace/main.lfy:load#load:load:20cc44ce9010c1d6d8f51447978d2c1bde789da9cd1d0f4bb04869b752e23c86
// @lfy def/workspace/main.lfy:load#load:load:fe9905296aa9979983b3f8a27155ccf43190bc1c52ec44a4c429edf8668342dd
#[test]
fn a_version_or_a_git_dependency_is_named_and_gives_no_package() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "a": "1.2.0", "b": "https://example.com/b.git", "c": "./vendor/c" } }"#,
        )
        .write("vendor/c/main.lfy", "");
    let workspace = fixture.load();
    // @lfy def/workspace/main.lfy:load
    assert_eq!(identifiers(&workspace), [LIBRARY_PACKAGE, "c"]);
    assert_eq!(package(&workspace, "c").root, "vendor/c");
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 2, "{problems:?}");
    for (problem, named) in problems.iter().zip(["\"a\"", "\"b\""]) {
        assert_eq!(problem.path.as_deref(), Some(MANIFEST));
        assert!(problem.message.contains(named), "{problem}");
        assert!(problem.message.contains("cannot be loaded yet"), "{problem}");
    }
}

/// What the value of a dependency entry is.
// @lfy def/workspace/main.lfy:load#load:load:a354cbeef946748f4822d5b7ccc565b0fb084d8ebb9b52938028687e8993b37f
// @lfy def/workspace/main.lfy:load#load:load:c78f44cea8c4c4a346c72577c2baf833dcae5ef0dfac93f3a0cabee3c6d487fc
// @lfy def/workspace/main.lfy:load#load:load:5f36f4fd70eb392d120e8c33f16e39c5ec39653ef25fed8c872656b5cc3fbeea
#[test]
fn a_dependency_value_is_a_path_a_git_url_or_a_version() {
    use super::load::{DependencyKind, dependency_kind};
    // @lfy def/workspace/main.lfy:load
    for path in ["./a", "../a", "/abs/a"] {
        assert_eq!(dependency_kind(path), DependencyKind::Path, "{path}");
    }
    // @lfy def/workspace/main.lfy:load
    for git in [
        "git@example.com:a/b",
        "git+ssh://example.com/a",
        "https://example.com/a",
        "example.com/a.git",
    ] {
        assert_eq!(dependency_kind(git), DependencyKind::Git, "{git}");
    }
    // @lfy def/workspace/main.lfy:load
    for version in ["1.2.0", "^1", "latest", "a/b"] {
        assert_eq!(dependency_kind(version), DependencyKind::Version, "{version}");
    }
}

/// A missing package root is reported even when no source directory exists.
// @lfy def/workspace/main.lfy:load#load:load:05b8fa2948fcab3e2f28a0331ccd1f1bfbe7b52d62225256478e886cf3e30d7b
#[test]
fn a_missing_package_root_is_a_problem_even_with_no_source_directory() {
    let fixture = Fixture::new();
    fixture.write(
        "elfie.json",
        r#"{ "dependencies": { "gone": "./packages/gone" } }"#,
    );
    let workspace = fixture.load();
    let problems = load_problems(&workspace);
    assert!(
        problems
            .iter()
            .any(|problem| problem.path.as_deref() == Some("def")),
        "{problems:?}"
    );
    // @lfy def/workspace/main.lfy:load
    assert!(
        problems
            .iter()
            .any(|problem| problem.path.as_deref() == Some("packages/gone")
                && problem.message.contains("gone")),
        "{problems:?}"
    );
    assert!(paths(&workspace).is_empty());
}


/// An `ace const` of a project file whose value is a `Target` of the package `elfie`
/// gives one target, with the layers of its slots in guidance order and the values its
/// layers set.
// @lfy def/workspace/main.lfy:load#load:load:3d36fd4d532720be47a05eb8515aab69f3fb83add111d5254ae21147d618a1f3
// @lfy def/workspace/main.lfy:load#load:load:d4dc4c84aaf9ba37a87a65a201d1ab6827f3e54dd6bdc33f863354f6c3953644
// @lfy def/workspace/main.lfy:load#load:load:9c4b7f6784e978ab38426f62744c733c2fbdfebea9fe78c455d429cc1f876075
// @lfy def/workspace/main.lfy:load#load:load:183ff338b3a3e2529d4c47790b5d0204470dcfc1bbb33c5621fb1d464e87e974
// @lfy def/workspace/main.lfy:load#load:load:f93277efea1e9ae1024146b1325d3d1bc82ec07b0fc2d632c73a5bbbbfb49f17
// @lfy def/workspace/main.lfy:load#load:load:a9fa8cd5e73cab1b5d4064fb290df44132c9dfca9e5d68d87f850ebb418b7a12
#[test]
fn an_ace_const_of_the_project_holding_a_target_gives_one() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ output = \"out/\", language = lang, layout = flat });\n",
        )
        .write("def/roles.lfy", ROLES);
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    let target = one_target(&workspace);
    assert_eq!(target.identifier, "t"); // @lfy def/workspace/main.lfy:load
    // The declaration is the const's own entity, which carries every layer.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        workspace.model.entities[target.declaration]
            .identifier
            .as_deref(),
        Some("t")
    );
    // The layout is a layer before the language, as guidance order has it.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(names(&workspace, &target.layers), ["flat", "lang"]);
    // The output, with its trailing slash removed.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(target.output_directory, "out");
    assert_eq!(target.extensions, ["rs"]);
    assert_eq!(target.marker_comment, "//");
    assert_eq!(target.script_runner, None);
    assert!(target.native_dependencies.is_empty());
    assert!(target.commands.is_empty());
    assert!(target.knowledge.is_empty());
}

/// Only an `ace const` of the project's own source holds a target: a `let`, a const
/// outside `ace`, and a const of a file of a package hold none. A const whose declared
/// type is the data holds one as much as one calling a function that gives one.
// @lfy def/workspace/main.lfy:load#load:load:d4dc4c84aaf9ba37a87a65a201d1ab6827f3e54dd6bdc33f863354f6c3953644
#[test]
fn a_let_a_plain_const_and_a_const_of_a_package_hold_no_target() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "p": "./pkg" } }"#,
        )
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const typed: Target = { output = \"typed\", language = lang, layout = flat };\n\
             ace let loose = targetOf({ output = \"loose\", language = lang, layout = flat });\n\
             const plain = targetOf({ output = \"plain\", language = lang, layout = flat });\n",
        )
        .write("def/roles.lfy", ROLES)
        .write(
            "pkg/main.lfy",
            "use \"elfie/target\";\n\
             trait packaged extends targetLanguage { }\n\
             trait packedLayout extends targetLayout { }\n\
             ace const inPackage = targetOf({ output = \"pkg\", language = packaged, \
             layout = packedLayout });\n",
        );
    let workspace = fixture.load();
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        workspace
            .targets
            .iter()
            .map(|target| target.identifier.as_str())
            .collect::<Vec<_>>(),
        ["typed"]
    );
    assert_eq!(workspace.targets[0].output_directory, "typed");
}

/// Every slot that holds layers gives them, in guidance order, and a trait in two slots
/// with equal arguments is one layer at its first place.
// @lfy def/workspace/main.lfy:load#load:load:183ff338b3a3e2529d4c47790b5d0204470dcfc1bbb33c5621fb1d464e87e974
// @lfy def/workspace/main.lfy:load#load:load:d1e245ccebeb2f7623e8da0f5e962388af2f23047e3d15da17c8352c50027b74
#[test]
fn the_layers_are_the_slots_in_guidance_order_each_trait_once() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({\n  output = \"out\",\n  language = lang,\n  \
             runtime = runs,\n  platforms = [here],\n  ecosystem = shop,\n  \
             frameworks = [built],\n  interfaces = [offered],\n  layout = flat,\n  \
             layers = [house, flat],\n});\n",
        )
        .write(
            "def/roles.lfy",
            "trait lang extends targetLanguage { }\n\
             trait runs extends targetRuntime { }\n\
             trait here extends targetPlatform { }\n\
             trait shop extends targetEcosystem { }\n\
             trait built extends targetFramework { }\n\
             trait offered extends targetInterface { }\n\
             trait flat extends targetLayout { }\n\
             trait house extends target { }\n",
        );
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    let target = one_target(&workspace);
    // `layers` comes first, so `flat` stands there and not again at the layout.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        names(&workspace, &target.layers),
        [
            "house", "flat", "offered", "built", "runs", "here", "shop", "lang"
        ]
    );
}

/// What a target's setters give it: the extensions its outputs may have, how a line
/// comment begins, what runs a script, and the ecosystem of its dependencies.
// @lfy def/workspace/main.lfy:load#load:load:a937c3da8b47a6fdea99c31a14b24e1c2aa31829c11b46405b9701b9fcf00a07
// @lfy def/workspace/main.lfy:load#load:load:a9fa8cd5e73cab1b5d4064fb290df44132c9dfca9e5d68d87f850ebb418b7a12
// @lfy def/workspace/main.lfy:load#load:load:a6d3cd85551722d81b7111f6681bf963da4ba6fb1d8ecb483d40fdad958c87c1
// @lfy def/workspace/main.lfy:load#load:load:835f6845f6033e939a044506a018d0a36680e14a6eb9beaa92012131e186f666
#[test]
fn a_target_reads_the_values_its_layers_set() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({\n  output = \"out\",\n  language = lang,\n  \
             layout = flat,\n  ecosystem = shop,\n  \
             dependencies = [{ name = \"serde\", version = \"1\" }, { name = \"sha2\" }],\n});\n",
        )
        .write(
            "def/roles.lfy",
            "trait lang extends targetLanguage {\n  .fileExtension = \"rs\";\n  \
             .markerComment = \"//\";\n  .outputExtensions = [\"toml\", \"rs\"];\n}\n\
             trait shop extends targetEcosystem {\n  .ecosystem = \"cargo\";\n  \
             .scriptRunner = \"cargo run --\";\n}\n\
             trait flat extends targetLayout { }\n",
        );
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    let target = one_target(&workspace);
    // The language's own extension first, then every one its layers add, each once.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(target.extensions, ["rs", "toml"]);
    assert_eq!(target.marker_comment, "//");
    assert_eq!(target.script_runner.as_deref(), Some("cargo run --"));
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        target.native_dependencies,
        [
            NativeDependency {
                identifier: "serde".to_string(),
                ecosystem: "cargo".to_string(),
                version: Some("1".to_string()),
            },
            NativeDependency {
                identifier: "sha2".to_string(),
                ecosystem: "cargo".to_string(),
                version: None,
            },
        ]
    );
}

/// One command per operation the target has one for, in the order `Operation` lists
/// them, and the knowledge of the declaration in order.
// @lfy def/workspace/main.lfy:load#load:load:18e900520d7cf988834a9b9730aeffd2562b0c3e7416b8940b4b784993194c57
#[test]
fn a_target_holds_one_command_per_operation_and_its_declarations_knowledge() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ output = \"out\", language = lang, layout = flat });\n\
             with t {\n  @knowledge.add({ topic = `the project's own`, \
             kind = KnowledgeKind.tool, source = \"elfie_check\" });\n}\n",
        )
        .write(
            "def/roles.lfy",
            "trait lang extends targetLanguage {\n  \
             @commands.add({ operation = Operation.test, line = \"cargo test\" });\n  \
             @commands.add({ operation = Operation.build, line = \"cargo build\" });\n  \
             @commands.add({ operation = Operation.build, line = \"never reached\" });\n  \
             @commands.add({ operation = Operation.lint });\n  \
             @knowledge.add({ topic = `the guide`, kind = KnowledgeKind.reference, \
             source = \"docs/guide.md\" });\n  \
             @knowledge.add({ topic = `a tool`, kind = KnowledgeKind.tool, \
             source = \"elfie_entity\" });\n}\n\
             trait flat extends targetLayout { }\n",
        )
        .write("docs/guide.md", "how it is written\n");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    let target = one_target(&workspace);
    // Build before test, as `Operation` lists them; the first command of an operation
    // wins, and `lint` has no line, so it has no command at all.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        target
            .commands
            .iter()
            .map(|command| (command.operation, command.line.as_deref()))
            .collect::<Vec<_>>(),
        [
            (Operation::Build, Some("cargo build")),
            (Operation::Test, Some("cargo test")),
        ]
    );
    // What the file gave the const comes before what its one layer gave it.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        target
            .knowledge
            .iter()
            .map(|item| item.topic.as_str())
            .collect::<Vec<_>>(),
        ["the project's own", "the guide", "a tool"]
    );
}

/// A command the project gives the const itself comes before every command its layers
/// gave it, so an operation whose command has no line is not run however a layer
/// spells it.
// @lfy def/workspace/main.lfy:load#load:load:8b43a836a39ca920b3b058858eb36936434cbec98d4b040e4ce0d3de392af7c0
// @lfy def/workspace/main.lfy:load#load:load:18e900520d7cf988834a9b9730aeffd2562b0c3e7416b8940b4b784993194c57
#[test]
fn a_command_the_file_gives_the_const_comes_before_its_layers() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ output = \"out/\", language = lang, layout = flat });\n\
             with t {\n  @commands.add({ operation = Operation.format });\n}\n",
        )
        .write(
            "def/roles.lfy",
            "trait lang extends targetLanguage {\n  .fileExtension = \"rs\";\n  \
             .markerComment = \"//\";\n  \
             @commands.add({ operation = Operation.build, line = \"cargo build\" });\n  \
             @commands.add({ operation = Operation.format, \
             line = \"cargo fmt --check\" });\n}\n\
             trait flat extends targetLayout { }\n",
        );
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    let target = one_target(&workspace);
    // The layer's build command stands; its format command does not, because the one
    // the file gave the const comes first and names no line.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        target
            .commands
            .iter()
            .map(|command| (command.operation, command.line.as_deref()))
            .collect::<Vec<_>>(),
        [(Operation::Build, Some("cargo build"))]
    );
}

/// The targets come in the order their consts are declared: file by file in bind
/// order, then source order.
// @lfy def/workspace/main.lfy:load#load:load:d4dc4c84aaf9ba37a87a65a201d1ab6827f3e54dd6bdc33f863354f6c3953644
#[test]
fn the_targets_are_in_the_order_their_consts_are_declared() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/main.lfy",
            "use \"./roles\";\nuse \"elfie/target\";\n\
             ace const third = targetOf({ output = \"c\", language = lang, layout = flat });\n",
        )
        .write(
            "def/roles.lfy",
            "use \"elfie/target\";\n\
             trait lang extends targetLanguage { }\n\
             trait flat extends targetLayout { }\n\
             ace const first = targetOf({ output = \"a\", language = lang, layout = flat });\n\
             ace const second = targetOf({ output = \"b\", language = lang, layout = flat });\n",
        );
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert_eq!(paths(&workspace), ["def/roles.lfy", "def/main.lfy"]);
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        workspace
            .targets
            .iter()
            .map(|target| target.identifier.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
}

/// A slot holding a layer whose trait does not extend that slot's role is named, and
/// the target still stands.
// @lfy def/workspace/main.lfy:load#load:load:850852d92a73a5a2483681f4ad973850de80c939bcf5118d7a8ef998b4d12671
// @lfy def/workspace/main.lfy:load#load:load:722849a54dc2494a11532b2c54b4db177b978cbbe4a6dd812669557ef1006be2
#[test]
fn a_layer_that_does_not_extend_its_slots_role_names_the_slot_and_the_trait() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ output = \"out\", language = flat, layout = lang });\n",
        )
        .write("def/roles.lfy", ROLES);
    let workspace = fixture.load();
    assert_eq!(load_problems(&workspace), Vec::<&LoadProblem>::new());
    let problems = bind_problems(&workspace);
    assert_eq!(problems.len(), 2, "{problems:?}");
    // A problem loading adds at a target is of the stage that added it.
    // @lfy def/workspace/main.lfy:load#load:load:83c8947a99852c5cfe9afc540029580ba37bedf5bfe20bba7ed77e79ebb98642
    // @lfy def/workspace/main.lfy:load#load:load:722849a54dc2494a11532b2c54b4db177b978cbbe4a6dd812669557ef1006be2
    for (problem, (slot, name, role)) in problems.iter().zip([
        ("language", "flat", "targetLanguage"),
        ("layout", "lang", "targetLayout"),
    ]) {
        assert_eq!(problem.stage, crate::model::Stage::Loader);
        assert_eq!(at_declaration(&workspace, problem), "t");
        assert!(problem.message.contains(" t "), "{problem}");
        assert!(problem.message.contains(slot), "{problem}");
        assert!(problem.message.contains(name), "{problem}");
        assert!(problem.message.contains(role), "{problem}");
    }
    // A layer in the wrong slot is still a layer of the target.
    assert_eq!(one_target(&workspace).identifier, "t");
}

/// A target without an output, a language, or a layout is named and left out.
// @lfy def/workspace/main.lfy:load#load:load:02062f73a6a6da19fbd8cce8b082ddb666c5cf47a3baee8fedb5ae408646b65c
#[test]
fn a_target_missing_an_output_a_language_or_a_layout_is_left_out() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ language = lang });\n",
        )
        .write("def/roles.lfy", ROLES);
    let workspace = fixture.load();
    assert!(workspace.targets.is_empty()); // @lfy def/workspace/main.lfy:load
    let problems = bind_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(at_declaration(&workspace, problems[0]), "t");
    assert!(problems[0].message.contains(" t "), "{}", problems[0]);
    assert!(problems[0].message.contains("output"), "{}", problems[0]);
    assert!(problems[0].message.contains("layout"), "{}", problems[0]);
}

/// A capability a layer requires that no layer of the target provides is named.
// @lfy def/workspace/main.lfy:load#load:load:e6137be72f47217dc6c217c142c019f7a2fe996e0f2e91068c2f30d7175fe0cb
// @lfy def/workspace/main.lfy:load#load:load:83268ea355172afe7a64fbf9ee1a600122557b3953f62145de7cbf3ab7e9e17b
#[test]
fn a_capability_no_layer_provides_is_named() {
    let needed = "trait server extends targetFramework {\n  .requires = [\"node-api\"];\n}\n";
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ output = \"out\", language = lang, layout = flat, \
             frameworks = [server] });\n",
        )
        .write("def/roles.lfy", &format!("{ROLES}{needed}"));
    let workspace = fixture.load();
    assert_eq!(one_target(&workspace).identifier, "t");
    let problems = bind_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(at_declaration(&workspace, problems[0]), "t");
    assert!(problems[0].message.contains(" t "), "{}", problems[0]);
    assert!(problems[0].message.contains("node-api"), "{}", problems[0]);

    // A layer that provides it leaves nothing to say.
    // @lfy def/workspace/main.lfy:load
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ output = \"out\", language = lang, layout = flat, \
             frameworks = [server], runtime = node });\n",
        )
        .write(
            "def/roles.lfy",
            &format!(
                "{ROLES}{needed}trait node extends targetRuntime {{\n  \
                 .provides = [\"node-api\"];\n}}\n"
            ),
        );
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
}

/// Dependencies need an ecosystem to come from.
// @lfy def/workspace/main.lfy:load#load:load:8c54a1933f95101299c7d1fc73f6483a0cb687bec0ecf95ae14ac16aa7131263
#[test]
fn dependencies_without_an_ecosystem_are_named() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ output = \"out\", language = lang, layout = flat, \
             dependencies = [{ name = \"serde\" }] });\n",
        )
        .write("def/roles.lfy", ROLES);
    let workspace = fixture.load();
    let target = one_target(&workspace);
    assert_eq!(target.native_dependencies.len(), 1);
    assert_eq!(target.native_dependencies[0].ecosystem, "");
    let problems = bind_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(at_declaration(&workspace, problems[0]), "t");
    assert!(problems[0].message.contains("ecosystem"), "{}", problems[0]);
}

/// Two targets that write into one directory, or one into the other's, are named at
/// the declaration of each.
// @lfy def/workspace/main.lfy:load#load:load:0bb2c8b9bdc22bcaf7b8375709c269d2ce9e4a87d15bad0b911bfef5a5e9dfb3
// @lfy def/workspace/main.lfy:load#load:load:95fbbedf0386adb0ad0590c359a7fa6afa27daae3109ef22782249594eba020b
#[test]
fn two_targets_writing_into_one_directory_name_each_other() {
    for (output, named) in [("out/u", "out/u"), ("out", "out"), ("elsewhere", "")] {
        let fixture = Fixture::empty();
        fixture
            .write(
                "def/targets.lfy",
                &format!(
                    "use \"elfie/target\";\nuse \"./roles\";\n\
                     ace const t = targetOf({{ output = \"out\", language = lang, \
                     layout = flat }});\n\
                     ace const u = targetOf({{ output = \"{output}\", language = lang, \
                     layout = flat }});\n"
                ),
            )
            .write("def/roles.lfy", ROLES);
        let workspace = fixture.load();
        assert_eq!(workspace.targets.len(), 2, "{:?}", workspace.problems);
        let problems = bind_problems(&workspace);
        if named.is_empty() {
            assert!(problems.is_empty(), "{output}: {problems:?}");
            continue;
        }
        // @lfy def/workspace/main.lfy:load
        assert_eq!(problems.len(), 2, "{output}: {problems:?}");
        for (problem, at) in problems.iter().zip(["t", "u"]) {
            assert_eq!(at_declaration(&workspace, problem), at);
            assert!(problem.message.contains(" t "), "{problem}");
            assert!(problem.message.contains(" u "), "{problem}");
            assert!(problem.message.ends_with(named), "{problem}");
        }
    }
}

/// Knowledge is vendored and read from disk: a source that is not there is named, and
/// so is one that would be fetched over the network.
// @lfy def/workspace/main.lfy:load#load:load:bac7aeb8877e25e34bb8600328dcdfd9765c1662f19b2c785ef6dc1eb24ddbf2
// @lfy def/workspace/main.lfy:load#load:load:14ac3e3c23396d2f0892c408a3681bf564363703aeb00ea0caa90879bf9f0b26
// @lfy def/workspace/main.lfy:load#load:load:05f2e92c57fc3eec4e710b8bd1bf35d7f4f2ecf0a40b1d8fc331d549eef5bc33
#[test]
fn knowledge_that_is_not_there_or_comes_over_the_network_is_named() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "p": "./pkg" } }"#,
        )
        .write(
            "def/main.lfy",
            "use \"p\";\n\
             d Here is given {\n  \
             @knowledge.add({ topic = `here`, kind = KnowledgeKind.example, \
             source = \"docs/here.md\" });\n}\n\
             d Gone {\n  \
             @knowledge.add({ topic = `gone`, kind = KnowledgeKind.reference, \
             source = \"docs/gone.md\" });\n}\n\
             d Fetched {\n  \
             @knowledge.add({ topic = `fetched`, kind = KnowledgeKind.reference, \
             source = \"https://example.com/guide\" });\n}\n\
             d Named {\n  \
             @knowledge.add({ topic = `named`, kind = KnowledgeKind.definition, \
             source = \"./nowhere\" });\n}\n",
        )
        .write("docs/here.md", "an example\n")
        // A source of a package's trait is relative to that package's root.
        // @lfy def/workspace/main.lfy:load
        .write(
            "pkg/main.lfy",
            "trait given {\n  \
             @knowledge.add({ topic = `the package's guide`, kind = KnowledgeKind.reference, \
             source = \"docs/guide.md\" });\n}\n",
        )
        .write("pkg/docs/guide.md", "how it is written\n");
    let workspace = fixture.load();
    assert_eq!(load_problems(&workspace), Vec::<&LoadProblem>::new());
    let problems = bind_problems(&workspace);
    // `Here` holds the package's item too, since the trait contributed it, and that
    // one is under the package's root; only its own missing item is named.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        problems
            .iter()
            .map(|problem| (
                at_declaration(&workspace, problem),
                problem.message.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("Gone", "the knowledge \"docs/gone.md\" does not exist"),
            (
                "Fetched",
                "the knowledge \"https://example.com/guide\" is fetched over the network; \
                 knowledge is vendored, so a compile never depends on the network"
            ),
        ],
        "{problems:?}"
    );
}

// @lfy def/workspace/main.lfy:load#load:load:f9beb9689c54303ed4e3a7e5f60e42c6dfd70ba8603fe85a05eba3cd28fdf395
// @lfy def/workspace/main.lfy:load#load:load:c4acf67876f96707cb66e2a889f79373ce852c3192472acb1b3dbff88d3d190a
#[test]
fn the_model_is_bound_from_the_files_once() {
    let fixture = Fixture::empty();
    fixture
        .write("def/main.lfy", "use \"./data\";\n")
        .write("def/data.lfy", "");
    let workspace = fixture.load();
    let expected = model::bind(workspace.model.sources.clone());
    assert_eq!(workspace.model, expected);
    assert_eq!(
        workspace
            .model
            .sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<Vec<_>>(),
        workspace
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>()
    );
    // @lfy def/workspace/data.lfy:Workspace.problems
    let load_count = workspace.load_problems().count();
    assert!(
        workspace.problems[..load_count]
            .iter()
            .all(|problem| problem.as_load().is_some())
    );
    assert!(
        workspace.problems[load_count..]
            .iter()
            .all(|problem| problem.as_bind().is_some())
    );
    assert_eq!(
        workspace.bind_problems().cloned().collect::<Vec<_>>(),
        workspace.model.problems
    );
}

// @lfy def/workspace/main.lfy:load
#[test]
fn paths_normalize_and_join() {
    assert_eq!(normalize("def/lexer/../grammar/./main"), "def/grammar/main");
    assert_eq!(normalize("def/../../x"), "../x");
    assert_eq!(normalize("./a//b/"), "a/b");
    // @lfy def/workspace/main.lfy:load
    assert_eq!(normalize("/a/b/../c"), "/a/c");
    assert_eq!(normalize("/../a"), "/a");
    assert_eq!(normalize("a/.."), ".");
    assert_eq!(extension("a/b.tar.gz"), Some("gz"));
    assert_eq!(extension("a/b"), None);
    assert_eq!(extension(".lfy"), None);
    assert_eq!(extension("a.lfy/c"), None);
    assert!(is_source("def/main.lfy"));
    assert!(!is_source("def/notes.txt"));
    assert_eq!(join("def", "main.lfy"), "def/main.lfy");
    assert_eq!(join(".", "main.lfy"), "main.lfy");
    assert_eq!(join("def", ""), "def");
    assert_eq!(directory_of("def/a/b.lfy"), "def/a");
    assert_eq!(directory_of("b.lfy"), "");
    assert!(under("def/a.lfy", "def"));
    assert!(!under("define/a.lfy", "def"));
    assert!(under("anything", "."));
    assert_eq!(trim_directory("def/"), "def");
    assert_eq!(trim_directory(""), ".");
}
