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

// @lfy def/workspace/main.lfy:load#load:load:364469ca73386a0fadcdccb16065c7b1fbd8cf7954391bff47aa5ac292a37ca7
// @lfy def/workspace/main.lfy:load#load:load:d89e1a3dc76a0474ada8c0015af825a5f5e8c5e26f0b09d9280c88429bbcfba0
// @lfy def/workspace/main.lfy:load#load:load:932b815f66767e3f29654d009afd1f258dc9f22040a21b8770d2e7aa470350e2
// @lfy def/workspace/main.lfy:load#load:load:1c98732ad9dcd0ae1423690fc2bd117e7dbc13effc288ef7a15560f86a0d3064
// @lfy def/workspace/main.lfy:load#load:load:a08b7bac9a52807fdcd4c5544ebeb10d60db2a277aef3e01ba0ac257e71291a7
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
    assert_eq!(workspace.source_directory, "def");
    assert!(paths(&workspace).is_empty());
    assert!(workspace.targets.is_empty());
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    assert!(workspace.overlays.is_empty());
    // The package elfie is in the program all the same, holding the files of that
    // copy. @lfy def/workspace/main.lfy:load
    assert_eq!(workspace.packages.len(), 1);
    let library = library(&workspace);
    assert_eq!(library.root, normalize(BUILT_IN_LIBRARY));
    assert!(Path::new(&library.root).is_absolute());
    let main = join(&library.root, MAIN_FILE);
    assert!(library_paths(&workspace).contains(&main.as_str()));
    assert!(library_paths(&workspace).len() > 1);
}

/// The library is the first source that is given, and a missing one is a problem
/// rather than a reason to try the next.
// @lfy def/workspace/main.lfy:load#load:load:d907c299c80f931a0deb7cbd87d12cf1f50320cdcfc62ec0ea02ea3217ce3800
// @lfy def/workspace/main.lfy:load#load:load:894c6485f9e8ac83db6e6d6dd3340e9ff0e21ccc789a23347307d15fb610664f
// @lfy def/workspace/main.lfy:load#load:load:18303ab51dd7497f5981f5a7ebbbefa64f43fca6e87ab6ac5a8aa9acaa26e95f
// @lfy def/workspace/main.lfy:load#load:load:1c98732ad9dcd0ae1423690fc2bd117e7dbc13effc288ef7a15560f86a0d3064
// @lfy def/workspace/main.lfy:load#load:load:05b8fa2948fcab3e2f28a0331ccd1f1bfbe7b52d62225256478e886cf3e30d7b
#[test]
fn the_library_is_the_first_source_that_names_one() {
    // The dependency entry elfie.
    let fixture = Fixture::empty();
    fixture.write(
        "elfie.json",
        r#"{ "lib": "byKey", "dependencies": { "elfie": { "root": "byDependency" } } }"#,
    );
    fixture.library("byDependency");
    fixture.library("byKey");
    let workspace = fixture.load();
    assert_eq!(library(&workspace).root, "byDependency");

    // The top level key lib, relative to the root.
    let fixture = Fixture::empty();
    fixture.write("elfie.json", r#"{ "lib": "byKey" }"#);
    fixture.library("byKey");
    let workspace = fixture.load();
    assert_eq!(library(&workspace).root, "byKey");
    assert!(library_paths(&workspace).contains(&"byKey/main.lfy"));

    // What the environment gives for ELFIE_LIB, when elfie.json names neither.
    let fixture = Fixture::empty();
    fixture.write("elfie.json", r#"{ "name": "named" }"#);
    let workspace = fixture.load();
    assert_eq!(library(&workspace).root, LIBRARY_ROOT);

    // A given root that is missing is a problem, and the next source is not tried.
    // @lfy def/workspace/main.lfy:load
    let fixture = Fixture::empty();
    fixture.write("elfie.json", r#"{ "lib": "gone" }"#);
    let workspace = fixture.load();
    assert_eq!(library(&workspace).root, "gone");
    assert!(library_paths(&workspace).is_empty());
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path.as_deref(), Some("gone"));
}

/// The main file of the package elfie is the prelude, its other files are the
/// library, and every file outside it is the program.
// @lfy def/workspace/main.lfy:load#load:load:c866f7d3a969f48fe5b6606e3e27ea34f62037935306ccc97e6bb293cab19e6b
// @lfy def/workspace/main.lfy:load#load:load:1c98732ad9dcd0ae1423690fc2bd117e7dbc13effc288ef7a15560f86a0d3064
// @lfy def/workspace/main.lfy:load#load:load:42ddd4111d6e51a0395e9b1eee8c79a065bd9e53b143029640c9ed2dd233c078
// @lfy def/workspace/main.lfy:load#load:load:656f24a577ae1949e6a118618dbcf080a5f0418f80c6cbeda85d2792cb3f027e
#[test]
fn the_library_gives_the_prelude_and_every_other_file_the_program() {
    let fixture = Fixture::empty();
    fixture
        .write("def/main.lfy", "d Marked is target { }\n")
        .write("lib/values/list.lfy", "d List { }\n");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
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
// @lfy def/workspace/main.lfy:load#load:load:247e20d07c0c842a9a7e7075746d74be2254640efab3b1062fdf4d1aec785a68
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

// @lfy def/workspace/main.lfy:load#load:load:14ff48a32bf3381366534b9ca3dbdb2f1d46fc634249e46bf5134fb6609d3a39
// @lfy def/workspace/main.lfy:load#load:load:553254e418671149fdc2a87c765c1ff91ddcf349bad7d7055bf05a5234721178
#[test]
fn the_manifest_names_the_layout() {
    let fixture = Fixture::new();
    fixture
        .write("elfie.json", r#"{ "name": "named", "source": "source/" }"#)
        .write("source/main.lfy", "trait t { }\n");
    let workspace = fixture.load();
    assert_eq!(workspace.name, "named");
    assert_eq!(workspace.source_directory, "source");
    assert_eq!(paths(&workspace), ["source/main.lfy"]);
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
}

/// A manifest that is a JSON object but gives no name and no source directory leaves
/// each of those defaults standing, exactly as a root with no manifest at all does.
// @lfy def/workspace/main.lfy:load#load:load:fd884247a5f63218e2f23f0ba9abdb91e6a342d07f56f61a79d4f23bfe3eeee7
// @lfy def/workspace/main.lfy:load#load:load:38338fb46f57a64fa2e744d34eae3279caf9b613709e743808ac4f6fba4a0289
#[test]
fn a_manifest_that_gives_no_layout_leaves_the_defaults_standing() {
    let fixture = Fixture::empty();
    fixture
        .write("elfie.json", r#"{ "lib": "lib" }"#)
        .write("def/main.lfy", "");
    let workspace = fixture.load();
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    // The name of the root directory, since elfie.json gives no name.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        workspace.name,
        fixture.root.file_name().unwrap().to_string_lossy()
    );
    // def, since elfie.json gives no source directory.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(workspace.source_directory, DEFAULT_SOURCE_DIRECTORY);
    assert_eq!(paths(&workspace), ["def/main.lfy"]);
}

/// A manifest is optional: where no `elfie.json` exists under the root, the name is
/// the name of the root directory, the source directory is `def`, and no problem is
/// added.
// @lfy def/workspace/main.lfy:load#load:load:8b89b53b4efb9ae82c650c075c04f5a708529dbc6cec061bdfde65ae7811ca3c
// @lfy def/workspace/main.lfy:load#load:load:e1ed96f364bdc98469b9736cdb2e0ce1a008d6ac0623f78e2cb68c8ccaf817a5
#[test]
fn no_manifest_is_no_problem_and_the_defaults_stand() {
    let fixture = Fixture::empty();
    fixture.write("def/main.lfy", "");
    assert!(!fixture.root.join(MANIFEST).exists());
    let workspace = fixture.load();
    assert_eq!(
        workspace.name,
        fixture.root.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(workspace.source_directory, DEFAULT_SOURCE_DIRECTORY);
    assert_eq!(paths(&workspace), ["def/main.lfy"]);
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
}

// @lfy def/workspace/main.lfy:load#load:load:befcf263b48c79ad8201135ce1c5dbca04ee8a875a161cf8942456e4cb97b8e7
// @lfy def/workspace/main.lfy:load#load:load:19a7d4fd318b9f7efc5cbe88610e53a0e61be4be62d5831f8646cc1bad17a9ad
#[test]
fn a_bad_manifest_adds_a_problem_and_the_defaults_stand() {
    for text in ["{ not json", "[1, 2]", "\"a string\""] {
        let fixture = Fixture::empty();
        fixture.write("elfie.json", text).write("def/main.lfy", "");
        let workspace = fixture.load();
        let problems = load_problems(&workspace);
        assert_eq!(problems.len(), 1, "{text}: {problems:?}");
        assert_eq!(problems[0].path.as_deref(), Some("elfie.json"));
        assert_eq!(
            workspace.name,
            fixture.root.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(workspace.source_directory, "def");
        assert_eq!(paths(&workspace), ["def/main.lfy"]);
    }
}

/// Targets, their outputs, and their dependencies are declared in the program, so a
/// manifest in the shape earlier versions read loads with no problem at all: the keys
/// the loader does not read, inside an entry as well as at the top, are ignored.
// @lfy def/workspace/main.lfy:load#load:load:9bf0f0938acd565776218f4c5d6a4434294c537b7873cc8a1e3490f8a5103f94
// @lfy def/workspace/main.lfy:load#load:load:f3a8e524e11cd59e038a6157e4a0fe362764b553e2b29342b421c3e379ad7cb9
#[test]
fn a_key_the_loader_does_not_read_is_ignored_and_adds_no_problem() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{
                    "name": "named",
                    "targets": { "t": { "package": "p", "marker": "m" } },
                    "output": "crates",
                    "native": [ { "identifier": "serde", "ecosystem": "cargo" } ],
                    "dependencies": { "p": { "root": "pkg", "version": "1" } },
                    "compiler": "scripts/compile.sh"
                }"#,
        )
        .write("def/main.lfy", "")
        .write("pkg/main.lfy", "");
    let workspace = fixture.load();
    // The keys the loader reads still stand.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(workspace.name, "named");
    let mut sorted = paths(&workspace);
    sorted.sort_unstable();
    assert_eq!(sorted, ["def/main.lfy", "pkg/main.lfy"]);
    // The manifest's key targets is ignored, so the program declares every target.
    // @lfy def/workspace/main.lfy:load
    assert!(workspace.targets.is_empty());
    assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
}

/// Loading never stops for a failure: a key the loader reads whose value is of the
/// wrong kind is named, and every other key of the manifest is read all the same.
// @lfy def/workspace/main.lfy:load#load:load:fe6fb32b63154e5b9396448cbce2410f62993bbac3c4be6a7db55d8a2f7d0d28
// @lfy def/workspace/main.lfy:load#load:load:b5997648b346dea065f6d06cc85024aa35df3796ec9edbc10d87099971bd1b6b
// @lfy def/workspace/main.lfy:load#load:load:d60b7d2fc94b097354076890d595ac2106b1b65e366b4cc45271b9e7211aa3cb
#[test]
fn a_key_whose_value_is_of_the_wrong_kind_is_named_and_the_rest_is_read() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{
                    "name": 3,
                    "source": [ "def" ],
                    "lib": true,
                    "dependencies": { "p": { "root": "pkg" }, "q": { "root": 4 } }
                }"#,
        )
        .write("def/main.lfy", "")
        .write("pkg/main.lfy", "");
    let workspace = fixture.load();
    let problems = load_problems(&workspace);
    // Each one names elfie.json, the key, and what the key should be.
    // @lfy def/workspace/main.lfy:load
    assert!(
        problems
            .iter()
            .all(|problem| problem.path.as_deref() == Some("elfie.json")),
        "{problems:?}"
    );
    for (key, wanted) in [
        ("\"name\"", "string"),
        ("\"source\"", "string"),
        ("\"lib\"", "string"),
        ("\"q\"", "\"root\""),
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
    // The rest of the manifest is still read: the dependency p gives a package, and
    // the name and the source directory fall back to their defaults.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(
        workspace.name,
        fixture.root.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(workspace.source_directory, DEFAULT_SOURCE_DIRECTORY);
    let mut sorted = paths(&workspace);
    sorted.sort_unstable();
    assert_eq!(sorted, ["def/main.lfy", "pkg/main.lfy"]);
    // The library stands where the environment names it, since "lib" gave nothing.
    // @lfy def/workspace/main.lfy:load
    assert_eq!(library(&workspace).root, LIBRARY_ROOT);
}

// @lfy def/workspace/main.lfy:load#load:load:387544d4a426f77295d5b183dd0d518a256f465f5f332556561411266aca0a8f
// @lfy def/workspace/main.lfy:load#load:load:d89e1a3dc76a0474ada8c0015af825a5f5e8c5e26f0b09d9280c88429bbcfba0
#[test]
fn a_missing_root_or_source_directory_adds_a_problem_with_no_path() {
    let fixture = Fixture::new();
    let missing = fixture.root.join("nowhere");
    let workspace = load_built_in(&missing);
    assert_eq!(workspace.root, missing);
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, None);
    assert!(workspace.files.is_empty());

    // The root exists but holds no source directory; a package there is not read either.
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "p": { "root": "pkg" } } }"#,
        )
        .write("pkg/main.lfy", "");
    let workspace = fixture.load();
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, None);
    assert!(problems[0].message.contains("def"), "{}", problems[0]);
    assert!(workspace.files.is_empty());
    assert_eq!(workspace.packages.len(), 2);
}

// @lfy def/workspace/main.lfy:load#load:load:247e20d07c0c842a9a7e7075746d74be2254640efab3b1062fdf4d1aec785a68
// @lfy def/workspace/main.lfy:load#load:load:a7bb1ab332ef59bd7ef0b193ca2257a60b8457f57538d11c2fd2c2f0c2610ea5
// @lfy def/workspace/main.lfy:load#load:load:912de90eec4ab7a10c1d9fb7853c226b0130e0f61558b54793fccef20e344b08
// @lfy def/workspace/main.lfy:load#load:load:23324d830b23ca1be67dea3c9234abb1945519a3e56b47c394222debdf328fe8
// @lfy def/workspace/main.lfy:load#load:load:c4acf67876f96707cb66e2a889f79373ce852c3192472acb1b3dbff88d3d190a
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

// @lfy def/workspace/main.lfy:load#load:load:1686133b540c858b4c1ab7edb631811da85b909c80a52d63d4084d22d6c3251f
// @lfy def/workspace/main.lfy:load#load:load:5d9353c2d7ef127d55101933cf2d1581e1bf8dfae943a0d3d59cf83a5cb17679
// @lfy def/workspace/main.lfy:load#load:load:a7bb1ab332ef59bd7ef0b193ca2257a60b8457f57538d11c2fd2c2f0c2610ea5
#[test]
fn an_undotted_use_names_a_package_and_resolves_against_its_root() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "rust": { "root": "targets/rust" } } }"#,
        )
        .write(
            "def/main.lfy",
            "use \"rust/guidance\";\nuse \"rust\";\nuse \"nope/thing\";\n",
        )
        .write("targets/rust/main.lfy", "use \"./guidance\";\n")
        .write("targets/rust/guidance.lfy", "");
    let workspace = fixture.load();
    assert_eq!(
        uses_of(&workspace, "def/main.lfy"),
        [
            Some("targets/rust/guidance.lfy".to_string()),
            Some("targets/rust/main.lfy".to_string()),
            None
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

// @lfy def/workspace/main.lfy:load#load:load:2c93d3e7305993d3a84ba8f451e28d1e5401c5189c591e98f5d0fe63976426c5
// @lfy def/workspace/main.lfy:load#load:load:f0c460549fc5debac126dc93031902458c9a539417d42b3b143b29ce41672943
// @lfy def/workspace/main.lfy:load#load:load:05b8fa2948fcab3e2f28a0331ccd1f1bfbe7b52d62225256478e886cf3e30d7b
#[test]
fn each_dependency_gives_a_package_whose_files_are_in_the_program() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{
                    "dependencies": { "rust": { "root": "targets/rust/" }, "gone": { "root": "targets/gone" } }
                }"#,
        )
        .write("def/main.lfy", "")
        .write("targets/rust/main.lfy", "")
        .write("targets/rust/extra/thing.lfy", "");
    let workspace = fixture.load();
    assert_eq!(workspace.packages.len(), 3); // @lfy def/workspace/main.lfy:load
    let rust = workspace
        .packages
        .iter()
        .position(|package| package.identifier == "rust")
        .unwrap();
    assert_eq!(workspace.packages[rust].root, "targets/rust");
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

/// Loading never stops for a failure: a dependency entry that is not an object with a
/// root is named, and every other dependency still gives a package.
// @lfy def/workspace/main.lfy:load#load:load:fe6fb32b63154e5b9396448cbce2410f62993bbac3c4be6a7db55d8a2f7d0d28
// @lfy def/workspace/main.lfy:load#load:load:2c93d3e7305993d3a84ba8f451e28d1e5401c5189c591e98f5d0fe63976426c5
#[test]
fn a_dependency_that_names_no_root_is_named_and_the_rest_still_load() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "p": { "root": "pkg" }, "q": 3 } }"#,
        )
        .write("pkg/main.lfy", "");
    let workspace = fixture.load();
    assert_eq!(
        workspace
            .packages
            .iter()
            .map(|package| package.identifier.as_str())
            .collect::<Vec<_>>(),
        ["p", LIBRARY_PACKAGE]
    );
    assert!(paths(&workspace).contains(&"pkg/main.lfy"));
    let problems = load_problems(&workspace);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path.as_deref(), Some("elfie.json"));
    assert!(problems[0].message.contains("\"q\""), "{}", problems[0]);
}

/// A missing package root is reported even when nothing can be discovered.
// @lfy def/workspace/main.lfy:load#load:load:05b8fa2948fcab3e2f28a0331ccd1f1bfbe7b52d62225256478e886cf3e30d7b
// @lfy def/workspace/main.lfy:load#load:load:387544d4a426f77295d5b183dd0d518a256f465f5f332556561411266aca0a8f
#[test]
fn a_missing_package_root_is_a_problem_even_with_no_source_directory() {
    let fixture = Fixture::new();
    fixture.write(
        "elfie.json",
        r#"{ "dependencies": { "gone": { "root": "packages/gone" } } }"#,
    );
    let workspace = fixture.load();
    // @lfy def/workspace/main.lfy:load
    assert!(workspace.files.is_empty());
    let problems = load_problems(&workspace);
    assert!(
        problems.iter().any(|problem| problem.path.is_none()),
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
            r#"{ "dependencies": { "p": { "root": "pkg" } } }"#,
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
            r#"{ "dependencies": { "p": { "root": "pkg" } } }"#,
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
