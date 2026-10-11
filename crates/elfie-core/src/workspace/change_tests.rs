// @lfy def/workspace/main.lfy:change
//! The tests of change.

use super::fixtures::*;
use super::*;
use crate::grammar::rules::statement::Statement;
use crate::model::{Origin, Problem, SymbolKind};

/// The same `path` names a different file in each workspace, because it is relative
/// to that workspace's own root and to nothing else.
// @lfy def/workspace/main.lfy:change#change:change:c6ba3fb62dabfc9865d520fcd05bbd995a1a53a7219263c2251afb188bbc8a8d
// @lfy def/workspace/main.lfy:change#change:change:de5b3acdad47c893e71937c22eaf52cb7f7123a1fec3e281ded802b921c5dae5
#[test]
fn a_change_path_is_relative_to_the_root_of_its_workspace() {
    let one = Fixture::empty();
    let two = Fixture::empty();
    one.write("def/main.lfy", "")
        .write("def/extra.lfy", "trait inOne { }\n");
    two.write("def/main.lfy", "")
        .write("def/extra.lfy", "trait inTwo { }\n");
    for (fixture, on_disk) in [(&one, "inOne"), (&two, "inTwo")] {
        let replaced = change(&fixture.load(), "def/extra.lfy", Some("trait replaced { }\n"));
        assert!(text_of(&replaced, "def/extra.lfy").contains("replaced"));
        // Reading it from disk again reads it under this workspace's root.
        let restored = change(&replaced, "def/extra.lfy", None);
        let expected = format!("trait {on_disk} {{ }}\n");
        assert_eq!(text_of(&restored, "def/extra.lfy"), expected);
    }
    // An absolute path is not a path relative to the root, so it reaches nothing.
    let loaded = one.load();
    let absolute = one.root.join("def/extra.lfy").to_string_lossy().into_owned();
    assert_eq!(change(&loaded, &absolute, Some("trait t { }\n")), loaded);
}

// @lfy def/workspace/main.lfy:change#change:change:895628dae1b1baf37565a3ffff240d0f54d0a439f28b7fa972933ecd2da95ddf
// @lfy def/workspace/main.lfy:change#change:change:f2ae7b0515ac2d4659beee3bd9e54158441b50ef0bc2890c7cf245afeec51118
// @lfy def/workspace/main.lfy:change#change:change:b89f4ff865d0361c8560b2cb4065ac5a90a90586070fec77f8d560c9f6651982
#[test]
fn a_change_adds_a_file_read_as_the_given_text() {
    let fixture = Fixture::empty();
    fixture.write("def/main.lfy", "");
    let before = fixture.load();
    let after = change(
        &before,
        "def/extra.lfy",
        Some("trait hasName { $name = string; }\n"),
    );
    assert_eq!(before, fixture.load()); // @lfy def/workspace/main.lfy:change
    let mut sorted = paths(&after);
    sorted.sort_unstable();
    assert_eq!(sorted, ["def/extra.lfy", "def/main.lfy"]);
    assert!(after.problems.is_empty(), "{:?}", after.problems);
    let extra = &after.model.sources[after.file("def/extra.lfy").unwrap().source];
    assert!(extra.tree.root.find(Statement::TraitDeclaration).is_some());
    assert_eq!(&*extra.tree.tokens[0].file, "def/extra.lfy");
    assert!(
        after
            .model
            .symbols
            .iter()
            .any(|symbol| symbol.name == "hasName" && symbol.kind == SymbolKind::Trait),
        "{:?}",
        after.model.symbols
    );
    assert_eq!(
        after.overlays.get("def/extra.lfy").map(String::as_str),
        Some("trait hasName { $name = string; }\n")
    );
    assert!(!fixture.root.join("def/extra.lfy").exists());
}

// @lfy def/workspace/main.lfy:change#change:change:3b3d10266814a5ce8c55a7d3a7026b812efcc22038abd92aa59984eab0be26b4
// @lfy def/workspace/main.lfy:change#change:change:483204a8915547a6e7a5c568902e1e48d573437c458b11e04b25176537ae9732
// @lfy def/workspace/main.lfy:change#change:change:c6ba3fb62dabfc9865d520fcd05bbd995a1a53a7219263c2251afb188bbc8a8d
#[test]
fn a_change_to_nothing_reads_the_file_from_disk_again() {
    let fixture = Fixture::empty();
    fixture
        .write("def/main.lfy", "")
        .write("def/extra.lfy", "trait onDisk { }\n");
    let loaded = fixture.load();
    let replaced = change(&loaded, "def/extra.lfy", Some("trait replaced { }\n"));
    let extra = &replaced.model.sources[replaced.file("def/extra.lfy").unwrap().source];
    assert!(
        extra
            .tree
            .raw(0, extra.tree.tokens.len())
            .contains("replaced")
    );
    let restored = change(&replaced, "def/extra.lfy", None);
    assert_eq!(paths(&restored).len(), 2);
    assert_eq!(restored, fixture.load());
    assert!(restored.overlays.is_empty());
}

// @lfy def/workspace/main.lfy:change#change:change:2d77ee8d6c0f68bbe2bd15bf77419431894e057302c497d51c7db36d54c35827
#[test]
fn a_change_to_nothing_with_no_file_on_disk_leaves_the_program() {
    let fixture = Fixture::empty();
    fixture.write("def/main.lfy", "use \"./extra\";\n");
    let loaded = fixture.load();
    assert_eq!(paths(&loaded), ["def/main.lfy"]);
    assert_eq!(uses_of(&loaded, "def/main.lfy"), [None]);
    let added = change(&loaded, "def/extra.lfy", Some(""));
    assert_eq!(paths(&added), ["def/extra.lfy", "def/main.lfy"]);
    assert_eq!(
        uses_of(&added, "def/main.lfy"),
        [Some("def/extra.lfy".to_string())]
    );
    assert!(added.problems.is_empty(), "{:?}", added.problems);
    let removed = change(&added, "def/extra.lfy", None);
    assert_eq!(paths(&removed), ["def/main.lfy"]);
    assert_eq!(removed, loaded);
}

// @lfy def/workspace/main.lfy:change#change:change:ffba7bf1aadfe9658e1cc3606237a6f446338ee57a72c8a92ba0c5e9f12e2448
// @lfy def/workspace/main.lfy:change#change:change:c6ba3fb62dabfc9865d520fcd05bbd995a1a53a7219263c2251afb188bbc8a8d
#[test]
fn a_replacement_stands_until_the_same_path_is_given_again() {
    let fixture = Fixture::empty();
    fixture
        .write("def/main.lfy", "")
        .write("def/a.lfy", "")
        .write("def/b.lfy", "");
    let loaded = fixture.load();
    let first = change(&loaded, "def/a.lfy", Some("trait a { }\n"));
    let second = change(&first, "def/b.lfy", Some("trait b { }\n"));
    assert_eq!(second.overlays.len(), 2);
    let a = &second.model.sources[second.file("def/a.lfy").unwrap().source];
    assert!(a.tree.raw(0, a.tree.tokens.len()).contains("trait a"));
    let third = change(&second, "def/a.lfy", Some("trait again { }\n"));
    let a = &third.model.sources[third.file("def/a.lfy").unwrap().source];
    assert!(a.tree.raw(0, a.tree.tokens.len()).contains("trait again"));
    assert_eq!(third.overlays.len(), 2);
    // @lfy def/workspace/main.lfy:change
    let expected = {
        fixture
            .write("def/a.lfy", "trait again { }\n")
            .write("def/b.lfy", "trait b { }\n");
        fixture.load()
    };
    assert_eq!(third.files, expected.files);
    assert_eq!(third.model, expected.model);
    assert_eq!(third.problems, expected.problems);
}

// @lfy def/workspace/main.lfy:change#change:change:88c72737ee35d3e8b4fc8faad9b0d27687ae69e994738b39b9c59967b95cd2ff
// @lfy def/workspace/main.lfy:change#change:change:de5b3acdad47c893e71937c22eaf52cb7f7123a1fec3e281ded802b921c5dae5
#[test]
fn a_change_out_of_reach_gives_an_equal_workspace() {
    let fixture = Fixture::empty();
    fixture.write("def/main.lfy", "");
    let loaded = fixture.load();
    assert_eq!(
        change(&loaded, "elsewhere/thing.lfy", Some("trait t { }")),
        loaded
    );
    assert_eq!(change(&loaded, "def/notes.txt", Some("text")), loaded);
    assert_eq!(change(&loaded, "src/main.lfy", None), loaded);
    // A file a use resolves to is in reach wherever it lives.
    fixture
        .write("def/main.lfy", "use \"../elsewhere/thing\";\n")
        .write("elsewhere/thing.lfy", "");
    let loaded = fixture.load();
    assert_eq!(
        uses_of(&loaded, "def/main.lfy"),
        [Some("elsewhere/thing.lfy".to_string())]
    );
    let changed = change(&loaded, "elsewhere/thing.lfy", Some("trait t { }\n"));
    assert_ne!(changed, loaded);
    assert_eq!(changed.overlays.len(), 1);

    // The use that brought it into reach goes away, and with it the file: a further
    // change there names none of the four places, so the result matches the
    // workspace it was given, down to the replacement that stood before.
    let gone = change(&changed, "def/main.lfy", Some("\n"));
    assert_eq!(paths(&gone), ["def/main.lfy"]);
    for text in [Some("trait other { }\n"), None] {
        assert_eq!(change(&gone, "elsewhere/thing.lfy", text), gone);
    }

    // The source path a knowledge item of the program has is the fourth place: the
    // item is read there, so a replacement is what the load finds. A path beside it
    // is still out of reach.
    let fixture = Fixture::empty();
    fixture.write(
        "def/main.lfy",
        "d Reads {\n  \
         @knowledge.add({ topic = `the guide`, kind = KnowledgeKind.reference, \
         source = \"docs/guide.md\" });\n}\n",
    );
    let loaded = fixture.load();
    assert_eq!(
        bind_problems(&loaded)
            .iter()
            .map(|problem| problem.message.as_str())
            .collect::<Vec<_>>(),
        ["the knowledge \"docs/guide.md\" does not exist"]
    );
    let written = change(&loaded, "docs/guide.md", Some("how it is written\n"));
    assert_eq!(bind_problems(&written), Vec::<&Problem>::new());
    assert_eq!(change(&loaded, "docs/other.md", Some("elsewhere\n")), loaded);
}

/// A file under the root of a package in the program is in reach of a change, as much
/// as one under the source directory.
// @lfy def/workspace/main.lfy:change#change:change:de5b3acdad47c893e71937c22eaf52cb7f7123a1fec3e281ded802b921c5dae5
#[test]
fn a_change_under_a_package_root_reaches_the_program() {
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "p": { "root": "pkg" } } }"#,
        )
        .write("def/main.lfy", "")
        .write("pkg/main.lfy", "");
    let loaded = fixture.load();
    let changed = change(&loaded, "pkg/extra.lfy", Some("trait inPackage { }\n"));
    assert_ne!(changed, loaded);
    let added = changed.file("pkg/extra.lfy").expect("the added file");
    assert_eq!(
        added
            .package
            .map(|package| changed.packages[package].identifier.as_str()),
        Some("p")
    );
    // The package `elfie` is a package of the program too.
    let changed = change(&loaded, "lib/extra.lfy", Some("trait inLibrary { }\n"));
    assert_ne!(changed, loaded);
    let added = changed.file("lib/extra.lfy").expect("the added file");
    assert_eq!(changed.origin(added), Origin::Library);
}

/// Nothing a change reaches is missed, and nothing it does not reach moves: the file
/// holding an entity the changed file adds to is read again although it uses nothing
/// of it, a file the change reaches in none of those ways keeps the symbols, usages,
/// and problems it had, and the targets and the problems about them are found again
/// from the model the change gives.
// @lfy def/workspace/main.lfy:change#change:change:92de1ca10f5fad840986c51b0e2d7a25ad8ae205c4e65ea89d88f0cc2af1945b
// @lfy def/workspace/main.lfy:change#change:change:e311d9fc70a554b7e62c546f0b90a42eb09de3f1d36cda13a9570db511b0055a
// @lfy def/workspace/main.lfy:change#change:change:38a2f13f3b49103a5a09f6fbd670f9d20830efb4c41dfdc038f1393bd72ccc10
#[test]
fn a_change_reaches_every_file_it_must_and_leaves_the_rest_as_it_was() {
    /// What a file holds in the model: the symbols declared in it, the usages
    /// recorded in it, and the problems pointing into it, each named so that the
    /// comparison does not depend on where the file sits among the sources.
    fn marks(workspace: &Workspace, path: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        let file = workspace.file(path).expect("a file of the program").source;
        let symbols = workspace
            .model
            .symbols
            .iter()
            .filter(|symbol| symbol.node.file == file)
            .map(|symbol| format!("{} {:?}", symbol.name, symbol.kind))
            .collect();
        let usages = workspace
            .model
            .usages
            .iter()
            .filter(|usage| usage.node.file == file)
            .map(|usage| format!("{:?} {:?}", usage.name, usage.layer))
            .collect();
        let problems = workspace
            .model
            .problems
            .iter()
            .filter(|problem| problem.node.file == file)
            .map(|problem| problem.message.clone())
            .collect();
        (symbols, usages, problems)
    }

    /// The topics of the knowledge of the entity with an identifier.
    fn topics(workspace: &Workspace, identifier: &str) -> Vec<String> {
        workspace
            .model
            .entities
            .iter()
            .find(|entity| entity.identifier.as_deref() == Some(identifier))
            .map(|entity| {
                entity
                    .knowledge
                    .iter()
                    .map(|item| item.topic.clone())
                    .collect()
            })
            .unwrap_or_else(|| panic!("{identifier} is declared"))
    }

    let fixture = Fixture::empty();
    fixture
        .write("def/thing.lfy", "d Thing { }\n")
        .write(
            "def/applies.lfy",
            "use \"./thing\";\nwith Thing {\n  \
             @knowledge.add({ topic = `added`, kind = KnowledgeKind.tool, \
             source = \"elfie_check\" });\n}\n",
        )
        .write("def/other.lfy", "trait untouched { $name = string; }\n")
        .write(
            "def/targets.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ output = \"out\", language = lang, layout = flat });\n",
        )
        .write("def/roles.lfy", ROLES);
    let loaded = fixture.load();
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    assert_eq!(topics(&loaded, "Thing"), ["added"]);
    let untouched = marks(&loaded, "def/other.lfy");

    // `def/thing.lfy` uses nothing of `def/applies.lfy`, so only the file that adds
    // to its entity puts it in the re-bind set; the whole program is bound again, so
    // it is read again and `Thing` loses what was added to it.
    let emptied = change(&loaded, "def/applies.lfy", Some(""));
    assert!(emptied.problems.is_empty(), "{:?}", emptied.problems);
    assert_eq!(topics(&emptied, "Thing"), Vec::<String>::new());
    // The file the change reaches in none of those ways is as it was.
    assert_eq!(marks(&emptied, "def/other.lfy"), untouched);

    // The targets, and the problems loading adds about them, come from the model the
    // change gives: swapping the slots of the one target names both of them.
    let swapped = change(
        &loaded,
        "def/targets.lfy",
        Some(
            "use \"elfie/target\";\nuse \"./roles\";\n\
             ace const t = targetOf({ output = \"out\", language = flat, layout = lang });\n",
        ),
    );
    assert_eq!(one_target(&swapped).identifier, "t");
    let problems = bind_problems(&swapped);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems
            .iter()
            .all(|problem| at_declaration(&swapped, problem) == "t"),
        "{problems:?}"
    );
    assert_eq!(marks(&swapped, "def/other.lfy"), untouched);
}

/// A replaced file stands in for one on disk down to the directories above it: the
/// source directory and a package root that only a replacement puts anything in are
/// read as directories, so a change there gives what a load of a root holding that
/// file on disk gives, and adds no problem of its own.
// @lfy def/workspace/main.lfy:change#change:change:c6ba3fb62dabfc9865d520fcd05bbd995a1a53a7219263c2251afb188bbc8a8d
#[test]
fn a_replacement_stands_in_for_the_directory_that_would_hold_it() {
    // The source directory is not on disk at all.
    let fixture = Fixture::new();
    let loaded = fixture.load();
    assert!(paths(&loaded).is_empty());
    assert_eq!(load_problems(&loaded).len(), 1, "{:?}", loaded.problems);
    let text = "trait inSource { }\n";
    let changed = change(&loaded, "def/main.lfy", Some(text));
    assert_eq!(paths(&changed), ["def/main.lfy"]);
    assert!(changed.problems.is_empty(), "{:?}", changed.problems);
    // @lfy def/workspace/main.lfy:change
    let expected = {
        fixture.write("def/main.lfy", text);
        fixture.load()
    };
    assert_eq!(changed.files, expected.files);
    assert_eq!(changed.model, expected.model);
    assert_eq!(changed.problems, expected.problems);

    // The root of a package in the program is not on disk either.
    let fixture = Fixture::empty();
    fixture
        .write(
            "elfie.json",
            r#"{ "dependencies": { "p": { "root": "pkg" } } }"#,
        )
        .write("def/main.lfy", "");
    let loaded = fixture.load();
    assert_eq!(load_problems(&loaded).len(), 1, "{:?}", loaded.problems);
    let text = "trait inPackage { }\n";
    let changed = change(&loaded, "pkg/extra.lfy", Some(text));
    assert!(changed.problems.is_empty(), "{:?}", changed.problems);
    let added = changed.file("pkg/extra.lfy").expect("the added file");
    assert_eq!(
        added
            .package
            .map(|package| changed.packages[package].identifier.as_str()),
        Some("p")
    );
    // @lfy def/workspace/main.lfy:change
    let expected = {
        fixture.write("pkg/extra.lfy", text);
        fixture.load()
    };
    assert_eq!(changed.files, expected.files);
    assert_eq!(changed.model, expected.model);
    assert_eq!(changed.problems, expected.problems);
}
