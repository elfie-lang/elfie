// @lfy def/workspace/main.lfy:change
//! A workspace with one file read as something other than what is on disk, and what tells
//! whether a path could reach the program at all.

use std::collections::HashMap;

use super::load::load_in;
use super::*;
use crate::model::{FileId, KnowledgeKind};

/// A workspace with one file read as something other than what is on disk.
///
/// `path` is relative to `Workspace::root` of `workspace`; `workspace` is left as it was
/// and the result is a new `Workspace`. With `text` set the file at `path` is read as
/// `text` instead of what is on disk; with `text` `None` it is read from disk again, and
/// leaves the program when there is no file at `path` on disk. A replacement stands until
/// the same `path` is given again. The result is what [`load`] of the same root gives when
/// every file replaced this way holds what it was replaced with. A `path` that is neither
/// under the source directory, nor under a package root, nor resolved to by a `Use` in the
/// program, nor the source path a knowledge item of the program has gives back a workspace
/// matching `workspace`.
// @lfy def/workspace/main.lfy:change
pub fn change(workspace: &Workspace, path: &str, text: Option<&str>) -> Workspace {
    // @lfy def/workspace/main.lfy:change
    if !in_reach(workspace, path) {
        return workspace.clone();
    }
    let mut overlays = workspace.overlays.clone(); // @lfy def/workspace/main.lfy:change
    match text {
        Some(text) => overlays.insert(path.to_string(), text.to_string()), // @lfy def/workspace/main.lfy:change
        None => overlays.remove(path), // @lfy def/workspace/main.lfy:change
    };
    // The library the workspace was loaded with is loaded again, so that a change gives
    // what `load` of the same root gave: where `elfie.json` names the library the entry
    // decides anyway, and where it does not, the directory this workspace found stands
    // in for the one the environment gave.
    // @lfy def/workspace/main.lfy:change
    let library = workspace
        .packages
        .iter()
        .find(|package| package.identifier == LIBRARY_PACKAGE)
        .map(|package| package.root.clone());
    // Decision: the whole program is bound again, which is every re-bind set the criteria
    // name and more — the file at `path`, every file that uses it directly or through
    // other files, every file holding an entity it applies a trait to or adds to, every
    // file whose compile-time code reads the entities of a trait it applies, and every
    // file at all when it reaches `global`. Binding is a function of the sources, so a
    // file outside the set it was bound again with keeps the symbols, usages, and
    // problems it had; no criterion asks which files were revisited, only what the result
    // holds, and a set that is never short is what keeps the result equal to a fresh
    // `load`.
    // @lfy def/workspace/main.lfy:change#change:change:92de1ca10f5fad840986c51b0e2d7a25ad8ae205c4e65ea89d88f0cc2af1945b
    // @lfy def/workspace/main.lfy:change#change:change:e311d9fc70a554b7e62c546f0b90a42eb09de3f1d36cda13a9570db511b0055a
    // @lfy def/workspace/main.lfy:change#change:change:950d3e129d266985048bd6b66cbd684cfe2e70c76f6aab48bf3a235e0c6e32e1
    // @lfy def/workspace/main.lfy:change#change:change:ac2779cfba6e5e3ab2117cc750fb9b75b08469978ef6c0e2f51a177e9b90102b
    // Targets and the problems they give are read from that model, so they are found
    // again with it.
    // @lfy def/workspace/main.lfy:change#change:change:38a2f13f3b49103a5a09f6fbd670f9d20830efb4c41dfdc038f1393bd72ccc10
    load_in(&workspace.root, overlays, library) // @lfy def/workspace/main.lfy:change
}

/// Whether a change at `path` could reach the program: the path is a source file under
/// the source directory or under the root of a package in the program, a `Use` in the
/// program resolves to it, or it is the source path a knowledge item of the program has.
// @lfy def/workspace/main.lfy:change#change:change:de5b3acdad47c893e71937c22eaf52cb7f7123a1fec3e281ded802b921c5dae5
fn in_reach(workspace: &Workspace, path: &str) -> bool {
    // These are the four places the criterion names, and nothing besides them puts a
    // path in reach: a path that is in none of them leaves the result matching the
    // workspace it was given, replacements and all.
    let is_source = is_source(path);
    (is_source && under(path, &workspace.source_directory))
        || (is_source
            && workspace
                .packages
                .iter()
                .any(|package| under(path, &package.root)))
        || workspace
            .model
            .sources
            .iter()
            .any(|source| source.uses.iter().flatten().any(|used| used == path))
        || knowledge_sources(workspace).any(|source| source == path)
}

/// Every source path `load` gives a knowledge item of the program, each joined to the root
/// of the package of the file its contributor is declared in as `load` joins it. Only a
/// reference and an example name a file; a definition names a use path and a tool its own
/// name, so neither gives a path on disk.
// @lfy def/workspace/main.lfy:change
fn knowledge_sources(workspace: &Workspace) -> impl Iterator<Item = String> + '_ {
    let package_of: HashMap<FileId, usize> = workspace
        .files
        .iter()
        .filter_map(|file| file.package.map(|package| (file.source, package)))
        .collect();
    workspace
        .model
        .entities
        .iter()
        .flat_map(|entity| entity.knowledge.iter())
        .filter(|item| matches!(item.kind, KnowledgeKind::Reference | KnowledgeKind::Example))
        .map(move |item| {
            let root = workspace.model.entities[item.contributor]
                .file
                .and_then(|file| package_of.get(&file))
                .map_or(".", |&package| workspace.packages[package].root.as_str());
            normalize(&join(root, &item.source))
        })
}
