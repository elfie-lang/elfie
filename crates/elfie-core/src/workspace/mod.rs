//! Compiled from `def/workspace/main.lfy`: loading a project from disk and binding it.
//!
//! [`load`] reads the project layout from `elfie.json`, discovers every `.lfy` file under
//! the source directory and under every package root, follows every `use` to the file it
//! names, orders the files so each comes after the files it uses, and binds them once.
//! Loading never stops for a failure: every failure is a [`LoadProblem`] or a `Problem`
//! and the rest of the project still loads. The targets are read from the program itself,
//! after binding: every `ace const` of a project file holding a `Target` of the package
//! `elfie` gives one. [`change`] gives a workspace in which one file is read as other text,
//! by loading the same root again with that replacement in place.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

pub mod data; // @lfy def/workspace/data.lfy:Workspace

pub use data::*;

use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::statement::Statement;
use crate::grammar::terminals::keyword::Keyword;
use crate::grammar::terminals::literal::Literal;
use crate::interpret::{Evaluated, Value as Computed};
use crate::lexer::lex;
use crate::model::{
    self, Entity, EntityId, EntityKind, FileId, KnowledgeKind, Model, NodeRef, Operation, Origin,
    Problem, Source, TypeRef,
};
use crate::parser::{Node, Tree, parse};

/// The project layout file, at the root of the project.
///
/// Decision: the definition names what `elfie.json` holds but never how it is spelled, so
/// this compile fixes the spelling as a JSON object with the optional keys `"name"`,
/// `"source"`, `"lib"`, and `"dependencies"` (an object of identifier to `{ "root" }`).
/// One of those whose value is of the wrong kind is a `LoadProblem` and the rest of the
/// manifest still stands; any other key is never looked at.
const MANIFEST: &str = "elfie.json";
/// `Workspace.sourceDirectory` when `elfie.json` gives none.
const DEFAULT_SOURCE_DIRECTORY: &str = "def"; // @lfy def/workspace/main.lfy:load
/// The file a directory resolves to.
const MAIN_FILE: &str = "main.lfy";
/// `Path::extension` of a source file.
const EXTENSION: &str = "lfy"; // @lfy def/workspace/main.lfy:load
/// The identifier of the package that holds the standard library: it is in the program
/// whether or not `elfie.json` names it.
const LIBRARY_PACKAGE: &str = "elfie"; // @lfy def/workspace/main.lfy:load
/// The variable whose directory holds the library, read when neither the dependency entry
/// `elfie` nor the top level key `lib` of `elfie.json` gives one.
const LIBRARY_VARIABLE: &str = "ELFIE_LIB"; // @lfy def/workspace/main.lfy:load
/// The copy of the library the compiler was built with.
///
/// Decision: the criterion leaves how this copy travels to the target's guidance, and the
/// guidance of the target `rust` says nothing about it. This compile takes the only
/// spelling that keeps `Package.root` a directory whose files are loaded as any other
/// package's, and `Package.root` absolute when the library is found outside the project:
/// the path of the library the crate was built beside, baked in at build time. A compiler
/// carried away from that directory finds the library through `ELFIE_LIB` instead.
const BUILT_IN_LIBRARY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lib"); // @lfy def/workspace/main.lfy:load
/// The data of the package `elfie` that an `ace const` holds to declare a target.
const TARGET_DATA: &str = "Target"; // @lfy def/workspace/main.lfy:load
/// The slots of that data, in the order it declares them, each with the trait a layer of
/// it must extend; `output` and `dependencies` hold no layer, so they have none.
const SLOTS: [(&str, Option<&str>); 10] = [
    ("output", None),
    ("language", Some("targetLanguage")),
    ("runtime", Some("targetRuntime")),
    ("platforms", Some("targetPlatform")),
    ("ecosystem", Some("targetEcosystem")),
    ("frameworks", Some("targetFramework")),
    ("interfaces", Some("targetInterface")),
    ("layout", Some("targetLayout")),
    ("layers", Some("target")),
    ("dependencies", None),
]; // @lfy def/workspace/main.lfy:load
/// The slots whose layers make up `Target.layers`, in guidance order.
const GUIDANCE_ORDER: [&str; 8] = [
    "layers",
    "interfaces",
    "frameworks",
    "layout",
    "runtime",
    "platforms",
    "ecosystem",
    "language",
]; // @lfy def/workspace/main.lfy:load
/// The slots a target cannot do without.
const NEEDED_SLOTS: [&str; 3] = ["output", "language", "layout"]; // @lfy def/workspace/main.lfy:load
/// `Operation`, in the order the enum lists them.
const OPERATIONS: [Operation; 7] = [
    Operation::Install,
    Operation::Add,
    Operation::Build,
    Operation::Test,
    Operation::Lint,
    Operation::Format,
    Operation::Run,
]; // @lfy def/workspace/main.lfy:load
/// The members of the object a `Layer` is: the trait chosen, and its arguments.
const SUBJECT: &str = "subject"; // @lfy def/workspace/main.lfy:load
const ARGUMENTS: &str = "arguments"; // @lfy def/workspace/main.lfy:load
/// The names a target's setters give what it is read from: the extension of its language's
/// files, the extensions its layers add, how a line comment begins, what runs a script,
/// which ecosystem its dependencies come from, and the capabilities its layers give and
/// need.
const FILE_EXTENSION: &str = "fileExtension"; // @lfy def/workspace/main.lfy:load
const OUTPUT_EXTENSIONS: &str = "outputExtensions"; // @lfy def/workspace/main.lfy:load
const MARKER_COMMENT: &str = "markerComment"; // @lfy def/workspace/main.lfy:load
const SCRIPT_RUNNER: &str = "scriptRunner"; // @lfy def/workspace/main.lfy:load
const ECOSYSTEM: &str = "ecosystem"; // @lfy def/workspace/main.lfy:load
const PROVIDES: &str = "provides"; // @lfy def/workspace/main.lfy:load
const REQUIRES: &str = "requires"; // @lfy def/workspace/main.lfy:load
/// What a knowledge source never begins with, because knowledge is vendored.
const NETWORK: [&str; 2] = ["http://", "https://"]; // @lfy def/workspace/main.lfy:load

/// Load a project from disk and bind it.
///
/// Reads the project layout, discovers every file, orders the files by what they use,
/// then binds them once. `Workspace::root` is `root`. Loading never stops for a failure:
/// every failure is a `LoadProblem` or a `Problem`, and the rest of the project still
/// loads.
// @lfy def/workspace/main.lfy:load
pub fn load(root: &Path) -> Workspace {
    // @lfy def/workspace/main.lfy:load
    load_in(root, BTreeMap::new(), std::env::var(LIBRARY_VARIABLE).ok())
}

/// A workspace with one file read as something other than what is on disk.
///
/// `path` is relative to `Workspace::root` of `workspace`; `workspace` is left as it was
/// and the result is a new `Workspace`. With `text` set the file at `path` is read as
/// `text` instead of what is on disk; with `text` `None` it is read from disk again, and
/// leaves the program when there is no file at `path` on disk. A replacement stands until
/// the same `path` is given again. The result is what [`load`] of the same root gives when
/// every file replaced this way holds what it was replaced with. A `path` that is neither
/// under the source directory, nor under a package root, nor resolved to by a `Use` in the
/// program gives back a workspace matching `workspace`.
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
    load_in(&workspace.root, overlays, library) // @lfy def/workspace/main.lfy:change
}

/// Whether a change at `path` could reach the program: the path is a source file under
/// the source directory or under the root of a package in the program, or a `Use` in the
/// program resolves to it.
// @lfy def/workspace/main.lfy:change
fn in_reach(workspace: &Workspace, path: &str) -> bool {
    // These are the three places the criterion names, and nothing besides them puts a
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
}

/// [`load`] with the files in `overlays` read as the text given there instead of what is
/// on disk, and `environment` standing for what the environment gives for `ELFIE_LIB`.
// @lfy def/workspace/main.lfy:change
// @lfy def/workspace/main.lfy:load
fn load_in(
    root: &Path,
    overlays: BTreeMap<String, String>,
    environment: Option<String>,
) -> Workspace {
    let mut loader = Loader {
        root,
        overlays: &overlays,
        problems: Vec::new(),
        use_problems: Vec::new(),
    };

    // Layout. @lfy def/workspace/main.lfy:load
    let manifest = loader.manifest(MANIFEST);
    // Only "name", "source", "lib", and "dependencies" are ever looked at; a target, where
    // its outputs go, and what its generated code needs are declared in the program.
    // @lfy def/workspace/main.lfy:load
    let name = loader
        .string(manifest.as_ref(), "name")
        .unwrap_or_else(|| directory_name(root)); // @lfy def/workspace/main.lfy:load
    let source_directory = loader
        .string(manifest.as_ref(), "source")
        .map(|source| trim_directory(&source))
        .unwrap_or_else(|| DEFAULT_SOURCE_DIRECTORY.to_string()); // @lfy def/workspace/main.lfy:load

    // Dependencies. @lfy def/workspace/main.lfy:load
    let mut packages = loader.packages(manifest.as_ref());
    // The package elfie is in the program whether or not elfie.json names it.
    // @lfy def/workspace/main.lfy:load
    let library = match packages
        .iter()
        .position(|package| package.identifier == LIBRARY_PACKAGE)
    {
        Some(library) => library,
        None => {
            let root = loader.library_root(manifest.as_ref(), environment);
            packages.push(loader.package(LIBRARY_PACKAGE, &root));
            packages.len() - 1
        }
    };

    // Discovery, use, and order. @lfy def/workspace/main.lfy:load
    let mut sources = Vec::new();
    let mut files = Vec::new();
    if !loader.directory(".") {
        // @lfy def/workspace/main.lfy:load
        loader.problem(None, format!("the root {} does not exist", root.display()));
    } else if !loader.directory(&source_directory) {
        // @lfy def/workspace/main.lfy:load
        loader.problem(
            None,
            format!(
                "the source directory {source_directory} does not exist under {}",
                root.display()
            ),
        );
    } else {
        // Decision: when the root or the source directory is missing, nothing at all is
        // discovered, package files included, so that `Workspace.files` is empty as the
        // criterion says.
        let entries = loader.discover(&source_directory, &packages);
        let (order, cycles) = order_by_uses(&entries);
        for (file, index) in cycles {
            let path = entries[file].path.clone();
            let spelled = entries[file].uses[index].spelled.clone();
            // @lfy def/workspace/main.lfy:load
            loader.use_problem(&path, index, spelled.as_deref(), "is part of a cycle");
        }
        for index in order {
            let entry = &entries[index];
            let Some(tree) = &entry.tree else { continue };
            files.push(File {
                path: entry.path.clone(), // @lfy def/workspace/main.lfy:load
                // @lfy def/workspace/main.lfy:load
                // @lfy def/workspace/main.lfy:load
                package: entry.package,
                source: sources.len(),    // @lfy def/workspace/main.lfy:load
            });
            sources.push(Source {
                path: entry.path.clone(),
                tree: tree.clone(),
                uses: entry
                    .uses
                    .iter()
                    .map(|site| site.resolved.clone())
                    .collect(), // @lfy def/workspace/main.lfy:load
                // @lfy def/workspace/main.lfy:load
                origin: origin_of(&entry.path, entry.package, library, &packages[library].root),
            });
        }
    }

    // Binding, once. @lfy def/workspace/main.lfy:load
    let mut model = model::bind(sources);

    // Only the bound model can point at these: a use that resolves to nothing or is part
    // of a cycle, the targets the program declares, and the knowledge its entities name.
    // @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load
    let mut added = use_problems(&model, &loader.use_problems);
    // Targets. @lfy def/workspace/main.lfy:load
    let (targets, mut at_targets) = targets_of(&mut model, &files, library);
    added.append(&mut at_targets);
    added.append(&mut knowledge_problems(&model, &loader, &files, &packages));
    if !added.is_empty() {
        model.problems.append(&mut added);
        model
            .problems
            .sort_by_key(|problem| (problem.node.file, problem.node.index));
    }

    // @lfy def/workspace/data.lfy:Workspace.problems
    let problems = loader
        .problems
        .into_iter()
        .map(WorkspaceProblem::Load)
        .chain(model.problems.iter().cloned().map(WorkspaceProblem::Bind))
        .collect();

    Workspace {
        root: root.to_path_buf(), // @lfy def/workspace/main.lfy:load
        name,
        source_directory,
        files,
        model,
        packages,
        targets,
        problems,
        overlays,
    }
}

/// One `Use` of a file: what it spells and what it resolved to.
// @lfy def/workspace/main.lfy:load
struct UseSite {
    /// The path as written; `None` when the statement has no string.
    spelled: Option<String>,
    /// The path of the file it refers to; `None` where it refers to nothing.
    resolved: Option<String>,
}

/// A problem at a `Use`, before the model can name the node: the file holding the use,
/// the position of the use among that file's uses, and what and why.
// @lfy def/workspace/main.lfy:load
struct UseProblem {
    file: String,
    index: usize,
    message: String,
}

/// One file discovered for the program.
struct Entry {
    path: String,
    package: Option<usize>,
    /// The parse of the file; `None` when it could not be read, in which case it is left
    /// out of the program.
    tree: Option<Tree>,
    uses: Vec<UseSite>,
}

/// The state of one load: where the project is, which files are replaced, and every
/// problem so far.
struct Loader<'a> {
    root: &'a Path,
    overlays: &'a BTreeMap<String, String>,
    problems: Vec<LoadProblem>,
    /// Every problem at a `Use`, in the order it arose, waiting for the model to give it
    /// a node.
    // @lfy def/workspace/main.lfy:load
    use_problems: Vec<UseProblem>,
}

impl Loader<'_> {
    fn problem(&mut self, path: Option<&str>, message: String) {
        self.problems.push(LoadProblem {
            path: path.map(str::to_string),
            message,
        });
    }

    /// A problem at a `Use`, recorded as the file holding it and the position of the use
    /// in that file. Discovery runs before binding, so the node cannot be named yet;
    /// [`use_problems`] turns each of these into a `Problem` at the `Use` node once the
    /// model knows the nodes.
    // @lfy def/workspace/main.lfy:load
    fn use_problem(&mut self, file: &str, index: usize, spelled: Option<&str>, why: &str) {
        let message = match spelled {
            Some(spelled) => format!("use {spelled:?} {why}"),
            None => "the use has no path".to_string(),
        };
        self.use_problems.push(UseProblem {
            file: file.to_string(),
            index,
            message,
        });
    }

    /// Whether the file at a path can be read: it is replaced, or it is on disk.
    fn exists(&self, path: &str) -> bool {
        self.overlays.contains_key(path) || self.root.join(path).is_file()
    }

    /// Whether anything at all is at a path: a replaced file, or a file or a directory on
    /// disk. `FileSystem.exists` of a knowledge source, which may name either.
    // @lfy def/workspace/main.lfy:load
    fn anything_at(&self, path: &str) -> bool {
        self.overlays.contains_key(path) || self.root.join(path).exists()
    }

    /// Whether a directory is on disk, and so has files of its own to walk.
    fn on_disk(&self, directory: &str) -> bool {
        if is_root(directory) {
            self.root.is_dir()
        } else {
            self.root.join(directory).is_dir()
        }
    }

    /// Whether a directory holds anything the program can read: it is on disk, or a
    /// replaced file is under it.
    ///
    /// A replaced file stands in for one on disk, so the directories above it stand in
    /// for directories on disk: a load with a replacement in place gives what a load of
    /// a root holding that file would give, directory checks and all.
    // @lfy def/workspace/main.lfy:change
    fn directory(&self, directory: &str) -> bool {
        self.on_disk(directory) || self.overlays.keys().any(|path| under(path, directory))
    }

    /// The text of the file at a path: what it is replaced with, or what is on disk.
    fn read(&self, path: &str) -> std::io::Result<String> {
        match self.overlays.get(path) {
            Some(text) => Ok(text.clone()),
            None => std::fs::read_to_string(self.root.join(path)),
        }
    }

    /// The object of an `elfie.json`; `None` when there is none, or when it cannot be
    /// read or is not an object.
    ///
    /// A manifest is optional: where none exists no problem is added and every default
    /// stands. One that exists but cannot be read, or whose text is not a JSON object, is
    /// a problem, and every default stands all the same.
    // @lfy def/workspace/main.lfy:load
    fn manifest(&mut self, path: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
        let disk = self.root.join(path);
        if !disk.exists() {
            return None;
        }
        let text = match std::fs::read_to_string(&disk) {
            Ok(text) => text,
            Err(error) => {
                self.problem(Some(path), format!("cannot be read: {error}"));
                return None;
            }
        };
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(object)) => Some(object),
            Ok(_) => {
                self.problem(Some(path), "is not a JSON object".to_string());
                None
            }
            Err(error) => {
                self.problem(Some(path), format!("is not valid JSON: {error}"));
                None
            }
        }
    }

    /// The string a key of the manifest gives; `None` when there is no such key, and
    /// `None` with the key named for the manifest when its value is not a string.
    // @lfy def/workspace/main.lfy:load
    fn string(
        &mut self,
        manifest: Option<&serde_json::Map<String, serde_json::Value>>,
        key: &str,
    ) -> Option<String> {
        match manifest.and_then(|manifest| manifest.get(key)) {
            Some(serde_json::Value::String(value)) => Some(value.clone()),
            None => None,
            // @lfy def/workspace/main.lfy:load
            Some(_) => {
                self.problem(Some(MANIFEST), format!("{key:?} is not a string"));
                None
            }
        }
    }

    /// Each dependency `elfie.json` names gives one package.
    // @lfy def/workspace/main.lfy:load
    fn packages(
        &mut self,
        manifest: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Vec<Package> {
        let mut out = Vec::new();
        let Some(dependencies) = manifest.and_then(|manifest| manifest.get("dependencies")) else {
            return out;
        };
        let Some(dependencies) = dependencies.as_object() else {
            self.problem(
                Some(MANIFEST),
                "\"dependencies\" is not an object".to_string(),
            );
            return out;
        };
        // Decision: dependencies are read in the order `serde_json` keeps the object's
        // keys, which is sorted by name.
        for (identifier, dependency) in dependencies {
            let Some(root) = dependency.get("root").and_then(|v| v.as_str()) else {
                self.problem(
                    Some(MANIFEST),
                    format!("the dependency {identifier:?} needs a string \"root\""),
                );
                continue;
            };
            let package = self.package(identifier, root);
            out.push(package);
        }
        out
    }

    /// One package at a root.
    // @lfy def/workspace/main.lfy:load
    fn package(&mut self, identifier: &str, root: &str) -> Package {
        let root = trim_directory(root);
        // @lfy def/workspace/main.lfy:load
        // Checked here rather than in `discover`, which does not run when the root or
        // the source directory is missing; a missing package root is reported either
        // way.
        if !self.directory(&root) {
            self.problem(
                Some(&root),
                format!("the root of the package {identifier} does not exist"),
            );
        }
        Package {
            identifier: identifier.to_string(),
            root,
        }
    }

    /// The root of the package `elfie` when `elfie.json` names no dependency for it: the
    /// top level key `lib` relative to the root when `elfie.json` has one, else the
    /// directory the environment gives for `ELFIE_LIB` when it gives one, else the copy
    /// of the library the compiler was built with. Each stands whether or not the
    /// directory it names exists, so a missing one is a problem rather than a reason to
    /// try the next.
    // @lfy def/workspace/main.lfy:load
    fn library_root(
        &mut self,
        manifest: Option<&serde_json::Map<String, serde_json::Value>>,
        environment: Option<String>,
    ) -> String {
        self.string(manifest, "lib")
            .or(environment)
            .unwrap_or_else(|| normalize(BUILT_IN_LIBRARY))
    }

    /// Every `.lfy` file under a directory, sorted by path, replaced files included.
    // @lfy def/workspace/main.lfy:load
    fn source_files(&mut self, directory: &str) -> Vec<String> {
        // Decision: discovery order within a directory is the sorted order of the paths.
        let mut out = BTreeSet::new();
        // A directory that only replacements stand in for has nothing on disk to walk.
        // @lfy def/workspace/main.lfy:change
        if self.on_disk(directory) {
            self.walk(directory, &mut out);
        }
        for path in self.overlays.keys() {
            if is_source(path) && under(path, directory) {
                out.insert(path.clone());
            }
        }
        out.into_iter().collect()
    }

    fn walk(&mut self, directory: &str, out: &mut BTreeSet<String>) {
        let entries = match std::fs::read_dir(self.root.join(directory)) {
            Ok(entries) => entries,
            Err(error) => {
                self.problem(Some(directory), format!("cannot be read: {error}"));
                return;
            }
        };
        let mut names: Vec<(String, bool)> = entries
            .filter_map(Result::ok)
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path().is_dir(),
                )
            })
            .collect();
        names.sort();
        for (name, is_directory) in names {
            let path = join(directory, &name);
            if is_directory {
                self.walk(&path, out);
            } else if is_source(&name) {
                // @lfy def/workspace/main.lfy:load
                out.insert(path);
            }
        }
    }

    /// Every file of the program: the source files, every package's files, and every
    /// file a `Use` refers to, each parsed with its uses resolved, in discovery order.
    // @lfy def/workspace/main.lfy:load
    fn discover(&mut self, source_directory: &str, packages: &[Package]) -> Vec<Entry> {
        let mut entries: Vec<Entry> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut pending: Vec<(String, Option<usize>)> = Vec::new();

        for path in self.source_files(source_directory) {
            enqueue(&mut seen, &mut pending, path, None); // @lfy def/workspace/main.lfy:load
        }
        for (id, package) in packages.iter().enumerate() {
            // A missing package root is already reported by `packages`; no file of it is
            // in the program. @lfy def/workspace/main.lfy:load
            if !self.directory(&package.root) {
                continue;
            }
            for path in self.source_files(&package.root) {
                enqueue(&mut seen, &mut pending, path, Some(id)); // @lfy def/workspace/main.lfy:load
            }
        }

        let mut next = 0;
        while next < pending.len() {
            let (path, package) = pending[next].clone();
            next += 1;
            let entry = self.load_file(&path, package, packages, |used| {
                // @lfy def/workspace/main.lfy:load
                enqueue(
                    &mut seen,
                    &mut pending,
                    used.to_string(),
                    package_of(used, packages),
                );
            });
            entries.push(entry);
        }

        // A file that could not be read is left out, so nothing may refer to it.
        // @lfy def/workspace/main.lfy:load
        let unreadable: BTreeSet<String> = entries
            .iter()
            .filter(|entry| entry.tree.is_none())
            .map(|entry| entry.path.clone())
            .collect();
        if !unreadable.is_empty() {
            let mut problems = Vec::new();
            for entry in &mut entries {
                for (index, site) in entry.uses.iter_mut().enumerate() {
                    if site
                        .resolved
                        .as_ref()
                        .is_some_and(|used| unreadable.contains(used))
                    {
                        site.resolved = None;
                        problems.push((entry.path.clone(), index, site.spelled.clone()));
                    }
                }
            }
            for (file, index, spelled) in problems {
                self.use_problem(
                    &file,
                    index,
                    spelled.as_deref(),
                    "refers to a file that could not be read",
                );
            }
        }
        entries
    }

    /// Read, lex, and parse one file and resolve its uses; `found` is told every path a
    /// use resolved to.
    // @lfy def/workspace/main.lfy:load
    fn load_file(
        &mut self,
        path: &str,
        package: Option<usize>,
        packages: &[Package],
        mut found: impl FnMut(&str),
    ) -> Entry {
        let unread = |uses: Vec<UseSite>| Entry {
            path: path.to_string(),
            package,
            tree: None,
            uses,
        };
        let text = match self.read(path) {
            Ok(text) => text,
            Err(error) => {
                self.problem(Some(path), format!("cannot be read: {error}"));
                return unread(Vec::new());
            }
        };
        let tokens = match lex(&text, Some(path)) {
            Ok(tokens) => tokens,
            // Decision: the definition only names a file that cannot be read. A file whose
            // text `Lexer.lex` refuses cannot be turned into a `File` either, so it is
            // left out of the program the same way.
            // @lfy def/workspace/main.lfy:load
            Err(error) => {
                self.problem(Some(path), format!("cannot be lexed: {error}"));
                return unread(Vec::new());
            }
        };
        let tree = parse(tokens, None);
        let mut uses = Vec::new();
        for node in tree.root.descendants() {
            if !node.is(Statement::Use) {
                continue;
            }
            let index = uses.len();
            let spelled = use_path(node, &tree);
            let resolved = match &spelled {
                Some(spelled) => match self.resolve(path, spelled, packages) {
                    Ok(used) => {
                        found(&used);
                        Some(used)
                    }
                    Err(why) => {
                        // @lfy def/workspace/main.lfy:load
                        self.use_problem(
                            path,
                            index,
                            Some(spelled),
                            &format!("resolves to nothing: {why}"),
                        );
                        None
                    }
                },
                // A `use` with no path at all: the parse recovered, but it names nothing.
                // @lfy def/workspace/main.lfy:load
                None => {
                    self.use_problem(path, index, None, "");
                    None
                }
            };
            uses.push(UseSite { spelled, resolved });
        }
        Entry {
            path: path.to_string(),
            package,
            tree: Some(tree),
            uses,
        }
    }

    /// The file a use's path refers to, from the file at `from`.
    // @lfy def/workspace/main.lfy:load
    fn resolve(&self, from: &str, spelled: &str, packages: &[Package]) -> Result<String, String> {
        let base = if spelled.starts_with('.') {
            // @lfy def/workspace/main.lfy:load
            join(directory_of(from), spelled)
        } else {
            // @lfy def/workspace/main.lfy:load
            let (first, rest) = spelled.split_once('/').unwrap_or((spelled, ""));
            let package = packages
                .iter()
                .find(|package| package.identifier == first)
                .ok_or_else(|| format!("no package is named {first:?}"))?;
            join(&package.root, rest)
        };
        let base = normalize(&base);
        // @lfy def/workspace/main.lfy:load
        let file = format!("{base}.{EXTENSION}");
        if self.exists(&file) {
            return Ok(file); // @lfy def/workspace/main.lfy:load
        }
        let main = join(&base, MAIN_FILE);
        if self.exists(&main) {
            return Ok(main);
        }
        Err(format!("neither {file} nor {main} exists"))
    }
}

/// Every recorded problem at a `Use` as a `Problem` at that `Use` node. A use in a file
/// that is not in the program has no node, and is dropped.
// @lfy def/workspace/main.lfy:load
fn use_problems(model: &Model, recorded: &[UseProblem]) -> Vec<Problem> {
    let mut out = Vec::new();
    for problem in recorded {
        let Some(file) = model.file(&problem.file) else {
            continue;
        };
        let node = model.sources[file]
            .tree
            .root
            .descendants()
            .into_iter()
            .filter(|node| node.is(Statement::Use))
            .nth(problem.index);
        let Some(node) = node.and_then(|node| model.node_ref(file, node)) else {
            continue;
        };
        out.push(Problem {
            node,
            message: problem.message.clone(),
            stage: crate::model::Stage::Loader,
        });
    }
    out
}

/// The origin of one file: the prelude for the main file of the package `elfie`, the
/// library for its other files, and the program for every file not in that package.
// @lfy def/workspace/main.lfy:load
fn origin_of(path: &str, package: Option<usize>, library: usize, root: &str) -> Origin {
    if package != Some(library) {
        return Origin::Program;
    }
    if path == join(root, MAIN_FILE) {
        Origin::Prelude
    } else {
        Origin::Library
    }
}

/// The entity of a name the package `elfie` declares: the one with that identifier declared
/// in one of the package's files that `accept` says yes to. `None` when the library in the
/// program declares none, in which case the check that reads it has nothing to say.
///
/// Decision: the criteria name `Target` and the roles of the package elfie, and the main
/// file of the library does not bring them into the prelude, so each is found by its name
/// among the entities the package's files declare rather than by a lookup in a scope.
// @lfy def/workspace/main.lfy:load
fn library_entity(
    model: &Model,
    files: &[File],
    library: usize,
    name: &str,
    accept: impl Fn(&Entity) -> bool,
) -> Option<EntityId> {
    let sources: HashSet<usize> = files
        .iter()
        .filter(|file| file.package == Some(library))
        .map(|file| file.source)
        .collect();
    model.entities.iter().position(|entity| {
        accept(entity)
            && entity.identifier.as_deref() == Some(name)
            && entity.file.is_some_and(|file| sources.contains(&file))
    })
}

/// Whether a trait is a role itself, or among its extenders directly or through other
/// extenders.
// @lfy def/workspace/main.lfy:load
fn extends(model: &Model, role: EntityId, layer: EntityId) -> bool {
    let mut seen = HashSet::new();
    let mut pending = vec![role];
    while let Some(entity) = pending.pop() {
        if entity == layer {
            return true;
        }
        if seen.insert(entity) {
            pending.extend(model.entities[entity].extenders().iter().copied());
        }
    }
    false
}

/// Where a contributor stands among a target's layers: the place in guidance order of the
/// first layer that is it, or that extends it, and nothing for a contributor that is
/// neither a layer nor a trait a layer extends, such as the file that adds to the const.
// @lfy def/workspace/main.lfy:load
fn layer_rank(model: &Model, layers: &[EntityId], contributor: EntityId) -> Option<usize> {
    layers
        .iter()
        .position(|&layer| extends(model, contributor, layer))
}

/// The items of a target's declaration in the order it reads them: those neither a layer
/// nor a trait a layer extends contributed, in the order they were added, then those the
/// layers and the traits they extend contributed, in guidance order.
// @lfy def/workspace/main.lfy:load
fn in_guidance_order<Item: Clone>(
    model: &Model,
    layers: &[EntityId],
    items: &[Item],
    contributor: impl Fn(&Item) -> EntityId,
) -> Vec<Item> {
    let mut own = Vec::new();
    let mut layered: Vec<(usize, &Item)> = Vec::new();
    for item in items {
        match layer_rank(model, layers, contributor(item)) {
            Some(rank) => layered.push((rank, item)),
            None => own.push(item.clone()),
        }
    }
    // Stable, so the items of one layer keep the order they were added in.
    // @lfy def/workspace/main.lfy:load
    layered.sort_by_key(|&(rank, _)| rank);
    own.into_iter()
        .chain(layered.into_iter().map(|(_, item)| item.clone()))
        .collect()
}

/// The child nodes of a node, in source order.
// @lfy def/workspace/main.lfy:load
fn child_nodes(model: &Model, node: NodeRef) -> Vec<NodeRef> {
    let file = node.file;
    model
        .node(node)
        .nodes()
        .filter_map(|child| model.node_ref(file, child))
        .collect()
}

/// Whether a node stands in an `ace` statement, or is one.
// @lfy def/workspace/main.lfy:load
fn within_ace(model: &Model, node: NodeRef) -> bool {
    let mut at = Some(node);
    while let Some(current) = at {
        if model.node(current).is(Statement::Ace) {
            return true;
        }
        at = model.parent(current);
    }
    false
}

/// Every `ace const` of a project file whose value is a `Target` of the package `elfie`:
/// one whose declared type is that data, or whose value is a call of a function whose
/// output is. A const of a file of a package gives no target, whatever it holds.
///
/// They come in the order their consts are declared: file by file in `Workspace::files`
/// order, then source order, which is the order a node reference carries.
// @lfy def/workspace/main.lfy:load
fn target_consts(model: &Model, files: &[File], data: EntityId) -> Vec<EntityId> {
    let project: HashSet<FileId> = files
        .iter()
        .filter(|file| file.package.is_none())
        .map(|file| file.source)
        .collect();
    let holds = Some(TypeRef::Entity(data));
    let mut out: Vec<(NodeRef, EntityId)> = Vec::new();
    for (entity, record) in model.entities.iter().enumerate() {
        if !matches!(record.kind, EntityKind::Variable) {
            continue;
        }
        let Some(node) = record.node else { continue };
        // @lfy def/workspace/main.lfy:load
        if !project.contains(&node.file) {
            continue;
        }
        if !within_ace(model, node)
            || model
                .node(node)
                .token(Keyword::LetKeyword, model.tokens(node.file))
                .is_some()
        {
            continue;
        }
        // @lfy def/workspace/main.lfy:load
        let declared = record.ty == holds;
        let called = child_nodes(model, node)
            .into_iter()
            .find(|&child| !model.node(child).is(Expression::Declared))
            .filter(|&value| model.node(value).is(Expression::Call))
            .and_then(|value| child_nodes(model, value).into_iter().next())
            .and_then(|callee| model::resolve(model, callee))
            .and_then(|symbol| model.entities[model.symbols[symbol].entity].output().cloned())
            == holds;
        if declared || called {
            out.push((node, entity));
        }
    }
    out.sort_by_key(|&(node, _)| (node.file, node.index));
    out.into_iter().map(|(_, entity)| entity).collect()
}

/// The slots of a target, read from `evaluate` of the const's value: the object it gives,
/// or none when it gives no object.
// @lfy def/workspace/main.lfy:load
fn slots_of(model: &mut Model, declaration: EntityId) -> BTreeMap<String, Computed> {
    let Some(node) = model.entities[declaration].node else {
        return BTreeMap::new();
    };
    let value = child_nodes(model, node)
        .into_iter()
        .find(|&child| !model.node(child).is(Expression::Declared));
    let Some(value) = value else {
        return BTreeMap::new();
    };
    // @lfy def/workspace/main.lfy:load
    if let Some(Evaluated::Value(Computed::Object(pairs))) = crate::interpret::evaluate(model, value)
    {
        return pairs;
    }
    // Decision: `Interpreter.evaluate` as the interface provides it gives nothing for a
    // call of a written `function`, because it asks whether the function's declaration
    // stands in an `ace` statement rather than whether the call does; so a const written
    // as a call of `targetOf`, which the criteria name beside a const with a declared
    // type, hands back no object. The record the call was given is that object, and
    // evaluating it reads the same slots, so it is read from there until `evaluate` runs
    // the call.
    // @lfy def/workspace/main.lfy:load
    if !model.node(value).is(Expression::Call) {
        return BTreeMap::new();
    }
    let argument = child_nodes(model, value)
        .into_iter()
        .find(|&child| model.node(child).is(Expression::Items))
        .map(|items| child_nodes(model, items))
        .and_then(|arguments| arguments.into_iter().next());
    match argument.and_then(|argument| crate::interpret::evaluate(model, argument)) {
        Some(Evaluated::Value(Computed::Object(pairs))) => pairs,
        _ => BTreeMap::new(),
    }
}

/// Whether a slot was given anything at all.
// @lfy def/workspace/main.lfy:load
fn given(slots: &BTreeMap<String, Computed>, slot: &str) -> bool {
    !matches!(
        slots.get(slot),
        None | Some(Computed::Undefined) | Some(Computed::Null)
    )
}

/// The trait each layer of a slot names and the arguments it was chosen with: a trait on
/// its own, chosen with none, or the object a `Layer` is. A slot holding anything else is
/// already named where the layers were applied, so nothing is said of it here.
// @lfy def/workspace/main.lfy:load
fn layers_of(
    model: &Model,
    slots: &BTreeMap<String, Computed>,
    slot: &str,
) -> Vec<(EntityId, Vec<Computed>)> {
    let items = match slots.get(slot) {
        None | Some(Computed::Undefined) | Some(Computed::Null) => return Vec::new(),
        Some(Computed::List(items)) => items.clone(),
        Some(one) => vec![one.clone()],
    };
    items
        .iter()
        .filter_map(|item| match item {
            // @lfy def/workspace/main.lfy:load
            Computed::Entity(entity) if model.entities[*entity].is_trait() => {
                Some((*entity, Vec::new()))
            }
            Computed::Object(pairs) => {
                let Some(Computed::Entity(subject)) = pairs.get(SUBJECT) else {
                    return None;
                };
                if !model.entities[*subject].is_trait() {
                    return None;
                }
                let arguments = match pairs.get(ARGUMENTS) {
                    Some(Computed::List(items)) => items.clone(),
                    _ => Vec::new(),
                };
                Some((*subject, arguments))
            }
            _ => None,
        })
        .collect()
}

/// Every item of every value the setters of an entity give a name, in the order the layers
/// were applied.
// @lfy def/workspace/main.lfy:load
fn items_set(model: &Model, entity: EntityId, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (set, value) in &model.entities[entity].values {
        if set != name {
            continue;
        }
        let model::Value::List(items) = value else {
            continue;
        };
        out.extend(items.iter().filter_map(|item| item.as_str()).map(str::to_string));
    }
    out
}

/// The string a setter gave a name; `None` when none did, or when it is no string.
// @lfy def/workspace/main.lfy:load
fn string_set(model: &Model, entity: EntityId, name: &str) -> Option<String> {
    model.entities[entity]
        .value(name)
        .and_then(model::Value::as_str)
        .map(str::to_string)
}

/// Every target the program declares, in the order their consts are declared, and every
/// problem their declarations gave.
// @lfy def/workspace/main.lfy:load
fn targets_of(model: &mut Model, files: &[File], library: usize) -> (Vec<Target>, Vec<Problem>) {
    let mut problems: Vec<Problem> = Vec::new();
    let Some(data) = library_entity(model, files, library, TARGET_DATA, |entity| {
        matches!(entity.kind, EntityKind::Data)
    }) else {
        return (Vec::new(), problems);
    };
    // The role each slot's layers must extend, where the library declares it.
    // @lfy def/workspace/main.lfy:load
    let roles: Vec<(&str, Option<&str>, Option<EntityId>)> = SLOTS
        .iter()
        .map(|&(slot, role)| {
            let entity = role
                .and_then(|role| library_entity(model, files, library, role, Entity::is_trait));
            (slot, role, entity)
        })
        .collect();

    let mut targets = Vec::new();
    for declaration in target_consts(model, files, data) {
        let at = model.entities[declaration]
            .node
            .expect("a target const has a declaring node");
        let identifier = model.entities[declaration]
            .identifier
            .clone()
            .unwrap_or_default();
        let slots = slots_of(model, declaration);

        // A layer whose trait does not extend the role of its slot.
        // @lfy def/workspace/main.lfy:load
        for &(slot, role, entity) in &roles {
            let (Some(role), Some(entity)) = (role, entity) else {
                continue;
            };
            for (subject, _) in layers_of(model, &slots, slot) {
                if extends(model, entity, subject) {
                    continue;
                }
                let name = model.entities[subject].identifier.clone().unwrap_or_default();
                problems.push(Problem {
                    node: at,
                    message: format!(
                        "the target {identifier} gives the {slot} {name}, which does not extend \
                         {role}"
                    ),
                    stage: crate::model::Stage::Binder,
                });
            }
        }

        // A target cannot be built without an output, a language, and a layout.
        // @lfy def/workspace/main.lfy:load
        let missing: Vec<&str> = NEEDED_SLOTS
            .into_iter()
            .filter(|&slot| !given(&slots, slot))
            .collect();
        if !missing.is_empty() {
            problems.push(Problem {
                node: at,
                message: format!(
                    "the target {identifier} gives no {}",
                    missing.join(" and no ")
                ),
                stage: crate::model::Stage::Binder,
            });
            continue;
        }

        // A capability a layer needs that no layer gives.
        // @lfy def/workspace/main.lfy:load
        let provided = items_set(model, declaration, PROVIDES);
        for required in items_set(model, declaration, REQUIRES) {
            if !provided.contains(&required) {
                problems.push(Problem {
                    node: at,
                    message: format!(
                        "the target {identifier} requires {required} and none of its layers \
                         provides it"
                    ),
                    stage: crate::model::Stage::Binder,
                });
            }
        }

        // The layers, in guidance order, each trait once at its first place.
        // @lfy def/workspace/main.lfy:load
        let mut chosen: Vec<(EntityId, Vec<Computed>)> = Vec::new();
        for slot in GUIDANCE_ORDER {
            for layer in layers_of(model, &slots, slot) {
                // @lfy def/workspace/main.lfy:load
                if !chosen.contains(&layer) {
                    chosen.push(layer);
                }
            }
        }
        let layers: Vec<EntityId> = chosen.into_iter().map(|(subject, _)| subject).collect();

        // What its generated code needs, each in the ecosystem its ecosystem layer names.
        // @lfy def/workspace/main.lfy:load
        let ecosystem = string_set(model, declaration, ECOSYSTEM).unwrap_or_default();
        let mut native_dependencies = Vec::new();
        if let Some(Computed::List(items)) = slots.get("dependencies") {
            for item in items {
                let Computed::Object(pairs) = item else { continue };
                let Some(Computed::String(name)) = pairs.get("name") else {
                    continue;
                };
                native_dependencies.push(NativeDependency {
                    identifier: name.clone(), // @lfy def/workspace/main.lfy:load
                    ecosystem: ecosystem.clone(), // @lfy def/workspace/main.lfy:load
                    version: match pairs.get("version") {
                        Some(Computed::String(version)) => Some(version.clone()), // @lfy def/workspace/main.lfy:load
                        _ => None,
                    },
                });
            }
        }
        // @lfy def/workspace/main.lfy:load
        if !native_dependencies.is_empty() && !given(&slots, ECOSYSTEM) {
            problems.push(Problem {
                node: at,
                message: format!("the dependencies of the target {identifier} need an ecosystem"),
                stage: crate::model::Stage::Binder,
            });
        }

        // The extensions its outputs may have: its language's, then every one its layers
        // add, each once. @lfy def/workspace/main.lfy:load
        let mut extensions: Vec<String> = string_set(model, declaration, FILE_EXTENSION)
            .into_iter()
            .collect();
        for extension in items_set(model, declaration, OUTPUT_EXTENSIONS) {
            if !extensions.contains(&extension) {
                extensions.push(extension);
            }
        }

        // One command per operation the target has one for, in the order `Operation` lists
        // them. What something other than a layer gave the declaration comes first, so the
        // project has the last word over its layers; an operation whose first command has
        // no line has none at all.
        // @lfy def/workspace/main.lfy:load#load:load:3f14c93383986089ff035033d9d411beb32a7ba58ae22f7b4b0b105bee9bb8ce
        let offered = in_guidance_order(
            model,
            &layers,
            &model.entities[declaration].commands,
            |command| command.contributor,
        );
        let commands = OPERATIONS
            .into_iter()
            .filter_map(|operation| {
                offered
                    .iter()
                    .find(|command| command.operation == operation)
                    .filter(|command| command.line.is_some())
                    .cloned()
            })
            .collect();

        // What the compiler is given to read for it, the declaration's own before its
        // layers'.
        // @lfy def/workspace/main.lfy:load#load:load:744ae66bb854af2ca9b87b6bdde697e61172669260c42dcdea808e88ddd17c71
        let knowledge = in_guidance_order(
            model,
            &layers,
            &model.entities[declaration].knowledge,
            |item| item.contributor,
        );

        targets.push(Target {
            identifier,  // @lfy def/workspace/main.lfy:load
            declaration, // @lfy def/workspace/main.lfy:load
            layers,      // @lfy def/workspace/main.lfy:load
            // @lfy def/workspace/main.lfy:load
            output_directory: slots
                .get("output")
                .and_then(|output| match output {
                    Computed::String(output) => Some(trim_directory(output)),
                    _ => None,
                })
                .unwrap_or_default(),
            extensions,
            // @lfy def/workspace/main.lfy:load
            marker_comment: string_set(model, declaration, MARKER_COMMENT).unwrap_or_default(),
            // @lfy def/workspace/main.lfy:load
            script_runner: string_set(model, declaration, SCRIPT_RUNNER),
            native_dependencies,
            commands,
            knowledge,
        });
    }

    // Two targets that write into one directory, or one into the other's.
    // @lfy def/workspace/main.lfy:load
    for (first, one) in targets.iter().enumerate() {
        for other in &targets[first + 1..] {
            let (outer, inner) = if one.output_directory.len() <= other.output_directory.len() {
                (one, other)
            } else {
                (other, one)
            };
            if inner.output_directory != outer.output_directory
                && !under(&inner.output_directory, &outer.output_directory)
            {
                continue;
            }
            let message = format!(
                "the targets {} and {} both write into {}",
                one.identifier, other.identifier, inner.output_directory
            );
            for target in [one, other] {
                let node = model.entities[target.declaration]
                    .node
                    .expect("a target const has a declaring node");
                problems.push(Problem {
                    node,
                    message: message.clone(),
                    stage: crate::model::Stage::Binder,
                });
            }
        }
    }
    (targets, problems)
}

/// Every problem the knowledge of the program's entities gives: an item that would be
/// fetched over the network, and a reference or an example whose source is not there.
///
/// A source is relative to the root of the package of the file its contributor is declared
/// in, and to the project's root when that file is the project's own.
// @lfy def/workspace/main.lfy:load
fn knowledge_problems(
    model: &Model,
    loader: &Loader,
    files: &[File],
    packages: &[Package],
) -> Vec<Problem> {
    let mut package_of: HashMap<FileId, usize> = HashMap::new();
    for file in files {
        if let Some(package) = file.package {
            package_of.insert(file.source, package);
        }
    }
    let mut problems = Vec::new();
    for entity in &model.entities {
        let Some(node) = entity.node else { continue };
        for item in &entity.knowledge {
            // @lfy def/workspace/main.lfy:load
            if NETWORK.iter().any(|head| item.source.starts_with(head)) {
                problems.push(Problem {
                    node,
                    message: format!(
                        "the knowledge {:?} is fetched over the network; knowledge is vendored, \
                         so a compile never depends on the network",
                        item.source
                    ),
                    stage: crate::model::Stage::Loader,
                });
                continue;
            }
            if !matches!(item.kind, KnowledgeKind::Reference | KnowledgeKind::Example) {
                continue;
            }
            // @lfy def/workspace/main.lfy:load
            let root = model.entities[item.contributor]
                .file
                .and_then(|file| package_of.get(&file))
                .map_or(".", |&package| packages[package].root.as_str());
            if !loader.anything_at(&normalize(&join(root, &item.source))) {
                problems.push(Problem {
                    node,
                    message: format!("the knowledge {:?} does not exist", item.source),
                    stage: crate::model::Stage::Loader,
                });
            }
        }
    }
    problems
}

/// Add a file to the program once, however many files use it.
// @lfy def/workspace/main.lfy:load
fn enqueue(
    seen: &mut HashSet<String>,
    pending: &mut Vec<(String, Option<usize>)>,
    path: String,
    package: Option<usize>,
) {
    if seen.insert(path.clone()) {
        pending.push((path, package));
    }
}

/// The path a `Use` statement spells: the value of the string's body token.
fn use_path(node: &Node, tree: &Tree) -> Option<String> {
    let literal = node.node(Expression::StringLiteral)?;
    let string = literal.nodes().next()?;
    let body = string
        .token(Literal::DoubleQuoteBody, &tree.tokens)
        .or_else(|| string.token(Literal::SingleQuoteBody, &tree.tokens));
    Some(body.map_or(String::new(), |index| tree.tokens[index].value.clone()))
}

/// The package whose root holds a path, when one does.
fn package_of(path: &str, packages: &[Package]) -> Option<usize> {
    packages
        .iter()
        .enumerate()
        .filter(|(_, package)| under(path, &package.root))
        .max_by_key(|(_, package)| package.root.len())
        .map(|(id, _)| id)
}

/// The files in an order that places every file after the files it uses, and every use
/// that is part of a cycle as `(file, use index)`. Files in a cycle keep the order they
/// were discovered in.
// @lfy def/workspace/main.lfy:load
fn order_by_uses(entries: &[Entry]) -> (Vec<usize>, Vec<(usize, usize)>) {
    let index: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(id, entry)| (entry.path.as_str(), id))
        .collect();
    let edges: Vec<Vec<Option<usize>>> = entries
        .iter()
        .map(|entry| {
            entry
                .uses
                .iter()
                .map(|site| {
                    site.resolved
                        .as_deref()
                        .and_then(|used| index.get(used).copied())
                })
                .collect()
        })
        .collect();
    let components = strongly_connected(&edges);
    let mut component_of = vec![0; entries.len()];
    for (id, component) in components.iter().enumerate() {
        for &node in component {
            component_of[node] = id;
        }
    }
    // Tarjan's algorithm emits a component only after every component it reaches, so
    // this order places every file after the files it uses.
    let mut order = Vec::with_capacity(entries.len());
    for mut component in components {
        component.sort_unstable(); // @lfy def/workspace/main.lfy:load
        order.extend(component);
    }
    let mut cycles = Vec::new();
    for (file, uses) in edges.iter().enumerate() {
        for (use_index, used) in uses.iter().enumerate() {
            if used.is_some_and(|used| component_of[used] == component_of[file]) {
                cycles.push((file, use_index));
            }
        }
    }
    (order, cycles)
}

/// The strongly connected components of a graph, each emitted after every component it
/// has an edge into (Tarjan).
fn strongly_connected(edges: &[Vec<Option<usize>>]) -> Vec<Vec<usize>> {
    struct State<'a> {
        edges: &'a [Vec<Option<usize>>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        components: Vec<Vec<usize>>,
    }

    fn visit(state: &mut State<'_>, node: usize) {
        state.index[node] = Some(state.next);
        state.low[node] = state.next;
        state.next += 1;
        state.stack.push(node);
        state.on_stack[node] = true;
        for used in state.edges[node].iter().flatten().copied() {
            match state.index[used] {
                None => {
                    visit(state, used);
                    state.low[node] = state.low[node].min(state.low[used]);
                }
                Some(index) if state.on_stack[used] => {
                    state.low[node] = state.low[node].min(index);
                }
                Some(_) => {}
            }
        }
        if state.low[node] == state.index[node].unwrap_or(0) {
            let mut component = Vec::new();
            loop {
                let member = state.stack.pop().expect("a component root is on the stack");
                state.on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            state.components.push(component);
        }
    }

    let count = edges.len();
    let mut state = State {
        edges,
        index: vec![None; count],
        low: vec![0; count],
        on_stack: vec![false; count],
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    for node in 0..count {
        if state.index[node].is_none() {
            visit(&mut state, node);
        }
    }
    state.components
}

/// The name of a directory, for a project without a name in `elfie.json`.
// @lfy def/workspace/main.lfy:load
fn directory_name(root: &Path) -> String {
    let name = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    name(root)
        .or_else(|| {
            std::fs::canonicalize(root)
                .ok()
                .and_then(|path| name(&path))
        })
        .unwrap_or_default()
}

/// A directory as `elfie.json` spells it, without a trailing slash.
fn trim_directory(directory: &str) -> String {
    let trimmed = directory.trim_end_matches('/');
    if trimmed.is_empty() {
        ".".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Whether a directory is the root itself.
fn is_root(directory: &str) -> bool {
    directory.is_empty() || directory == "."
}

/// Whether a path is under a directory, both relative to the root.
fn under(path: &str, directory: &str) -> bool {
    is_root(directory)
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// A path under a directory, both relative to the root.
fn join(directory: &str, name: &str) -> String {
    if is_root(directory) {
        name.to_string()
    } else if name.is_empty() {
        directory.to_string()
    } else {
        format!("{directory}/{name}")
    }
}

/// The directory holding a file, relative to the root; empty for a file at the root.
fn directory_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// `Path::extension`: what follows the last dot of the last segment, without the dot;
/// `None` when the name has no dot or only a leading one.
// @lfy def/workspace/main.lfy:load
fn extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(0) | None => None,
        Some(dot) => Some(&name[dot + 1..]),
    }
}

/// Whether a path names a file of the program: one whose extension is `lfy`.
// @lfy def/workspace/main.lfy:load
fn is_source(path: &str) -> bool {
    extension(path) == Some(EXTENSION)
}

/// `Path::normalize`: a path with every `.` dropped and every `..` applied to the segment
/// before it, never looking at the disk. An absolute path stays absolute, and `..` at the
/// root is dropped because the root has no parent.
// @lfy def/workspace/main.lfy:load
fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => match segments.last() {
                Some(&last) if last != ".." => {
                    segments.pop();
                }
                _ if absolute => {}
                _ => segments.push(".."),
            },
            segment => segments.push(segment),
        }
    }
    let joined = segments.join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_string(),
        (false, false) => joined,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::model::SymbolKind;

    /// The directory of the library a fixture carries, relative to its root.
    const LIBRARY_ROOT: &str = "lib";
    /// The main file of the library a fixture carries, which is the prelude: it brings the
    /// vocabulary a target is declared with into the scope of every file of the project.
    const LIBRARY: &str = "use \"./prelude/builtin\";\nuse \"./prelude/entity\";\n\
                           use \"./prelude/trait\";\nuse \"./criteria/main\";\n\
                           use \"./target/main\";\n";
    /// The trait a builtin carries, which the library's own declarations need.
    const LIBRARY_BUILTIN: &str = "trait builtin: `Performed by the compiler` { }\n";
    /// The context layer every entity is seen through, which is where the lists a body
    /// adds to live.
    const LIBRARY_ENTITY: &str = "\
d Entity: `A declared thing seen through its context layer` {
  $knowledge: `What the compiler is given to read for it` = object[];
  $commands: `Shell commands for its operations` = object[];
  $targets: `The targets it is built for` = object[];
}
";
    /// `layer`, which chooses a trait as a layer with arguments.
    const LIBRARY_TRAIT: &str = "use \"./builtin\";\n\
                                 fn layer(subject: trait, ...arguments: string[]) is builtin: \
                                 `A trait chosen as a layer of a target` => object;\n";
    /// The enums a knowledge item and a command name.
    const LIBRARY_CRITERIA: &str = "\
enum KnowledgeKind: `What a piece of knowledge is` {
  reference = `documentation to read`,
  example = `working code to imitate`,
  definition = `Elfie source that defines it`,
  tool = `a tool of the agent server`,
}
enum Operation: `What a command is for` {
  install = `install`,
  add = `add`,
  build = `build`,
  test = `test`,
  lint = `lint`,
  format = `format`,
  run = `run`,
}
";
    /// `Target`, the roles its slots take, and the function that gives a record written in
    /// braces its type, as the package `elfie` declares them.
    const LIBRARY_TARGET: &str = "\
use \"../prelude/builtin\";
trait target: `What an entity carries to be built for one target` {
  $markerComment: `How a line comment begins` = string;
  $provides: `Capabilities this layer gives its target` = string[];
  $requires: `Capabilities some layer of its target must provide` = string[];
  $outputExtensions: `Extensions besides the language's own` = string[];
  .provides = [];
  .requires = [];
  .outputExtensions = [];
}
trait targetLanguage extends target: `What its code is written in` {
  $fileExtension: `The extension of its files` = string;
}
trait targetRuntime extends target: `What runs its code` { }
trait targetPlatform extends target: `Where it runs` { }
trait targetEcosystem extends target: `Where its dependencies come from` {
  $ecosystem: `What that ecosystem is called` = string;
  $scriptRunner: `What runs a script its manifest names` = string;
}
trait targetFramework extends target: `What its code is built on` { }
trait targetInterface extends target: `What it offers` { }
trait targetLayout extends target: `Where its files go` { }
d Target is builtin: `One artifact the project is built into` {
  $output: `Where everything it writes goes` = string;
  $language: `What its code is written in` = trait;
  $runtime: `What runs its code` = trait | undefined;
  $platforms: `Where it runs` = trait[] | undefined;
  $ecosystem: `Where its dependencies come from` = trait | undefined;
  $frameworks: `What its code is built on` = trait[] | undefined;
  $interfaces: `What it offers` = trait[] | undefined;
  $layout: `Where its files go` = trait;
  $layers: `Anything else the compiler is told` = trait[] | undefined;
  $dependencies: `What its generated code needs` = object[] | undefined;
}
function targetOf(target: Target): `Gives a record written in braces its type` -> Target {
  return target;
}
";
    /// A language and a layout to give a target, in `def/roles.lfy`.
    const ROLES: &str = "\
trait lang extends targetLanguage {
  .fileExtension = \"rs\";
  .markerComment = \"//\";
}
trait flat extends targetLayout { }
";

    /// A project directory under the system's temporary directory, removed when dropped.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "elfie-ws-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            let fixture = Fixture { root };
            // Every fixture holds a library of its own, which [`Fixture::load`] names as
            // the environment would, so that a test never reads the copy of the library
            // the compiler was built with. @lfy def/workspace/main.lfy:load
            fixture.library(LIBRARY_ROOT);
            fixture
        }

        /// The library of the package `elfie`, written at a root of its own.
        // @lfy def/workspace/main.lfy:load
        fn library(&self, root: &str) -> &Fixture {
            self.write(&join(root, MAIN_FILE), LIBRARY)
                .write(&join(root, "prelude/builtin.lfy"), LIBRARY_BUILTIN)
                .write(&join(root, "prelude/entity.lfy"), LIBRARY_ENTITY)
                .write(&join(root, "prelude/trait.lfy"), LIBRARY_TRAIT)
                .write(&join(root, "criteria/main.lfy"), LIBRARY_CRITERIA)
                .write(&join(root, "target/main.lfy"), LIBRARY_TARGET)
        }

        /// A project with an empty `def` directory.
        fn empty() -> Fixture {
            let fixture = Fixture::new();
            fixture.dir("def");
            fixture
        }

        fn dir(&self, path: &str) -> &Fixture {
            fs::create_dir_all(self.root.join(path)).unwrap();
            self
        }

        fn write(&self, path: &str, text: &str) -> &Fixture {
            self.write_bytes(path, text.as_bytes())
        }

        fn write_bytes(&self, path: &str, bytes: &[u8]) -> &Fixture {
            let disk = self.root.join(path);
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(disk, bytes).unwrap();
            self
        }

        /// The project, loaded with the library the fixture carries standing in for the
        /// one the environment names.
        // @lfy def/workspace/main.lfy:load
        fn load(&self) -> Workspace {
            load_in(&self.root, BTreeMap::new(), Some(LIBRARY_ROOT.to_string()))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// A load with nothing in the environment, so that the library is the copy the
    /// compiler was built with.
    // @lfy def/workspace/main.lfy:load
    fn load_built_in(root: &Path) -> Workspace {
        load_in(root, BTreeMap::new(), None)
    }

    /// Whether a file came from the package `elfie`.
    // @lfy def/workspace/main.lfy:load
    fn is_library(workspace: &Workspace, file: &File) -> bool {
        file.package
            .is_some_and(|package| workspace.packages[package].identifier == LIBRARY_PACKAGE)
    }

    /// Every file of the program that the standard library did not bring, in bind order.
    fn project(workspace: &Workspace) -> impl Iterator<Item = &File> {
        workspace
            .files
            .iter()
            .filter(|file| !is_library(workspace, file))
    }

    fn paths(workspace: &Workspace) -> Vec<&str> {
        project(workspace).map(|file| file.path.as_str()).collect()
    }

    /// Every file of the package `elfie`, in bind order.
    // @lfy def/workspace/main.lfy:load
    fn library_paths(workspace: &Workspace) -> Vec<&str> {
        workspace
            .files
            .iter()
            .filter(|file| is_library(workspace, file))
            .map(|file| file.path.as_str())
            .collect()
    }

    /// The package `elfie` of a workspace.
    // @lfy def/workspace/main.lfy:load
    fn library(workspace: &Workspace) -> &Package {
        workspace
            .packages
            .iter()
            .find(|package| package.identifier == LIBRARY_PACKAGE)
            .expect("the package elfie is in the program")
    }

    fn load_problems(workspace: &Workspace) -> Vec<&LoadProblem> {
        workspace.load_problems().collect()
    }

    fn uses_of<'w>(workspace: &'w Workspace, path: &str) -> &'w [Option<String>] {
        let file = workspace
            .file(path)
            .unwrap_or_else(|| panic!("{path} is not in the program"));
        workspace.uses(file)
    }

    /// The identifiers of a list of entities, in order.
    fn names<'w>(workspace: &'w Workspace, entities: &[EntityId]) -> Vec<&'w str> {
        entities
            .iter()
            .filter_map(|&entity| workspace.model.entities[entity].identifier.as_deref())
            .collect()
    }

    fn bind_problems(workspace: &Workspace) -> Vec<&Problem> {
        workspace.bind_problems().collect()
    }

    /// The one target of a workspace.
    // @lfy def/workspace/main.lfy:load
    fn one_target(workspace: &Workspace) -> &Target {
        let named: Vec<&str> = workspace
            .targets
            .iter()
            .map(|target| target.identifier.as_str())
            .collect();
        assert_eq!(named.len(), 1, "{named:?} {:?}", workspace.problems);
        &workspace.targets[0]
    }

    /// The name bound by the nearest declaration at or above the node a problem points at.
    // @lfy def/workspace/main.lfy:load
    fn at_declaration<'w>(workspace: &'w Workspace, problem: &Problem) -> &'w str {
        let model = &workspace.model;
        let mut at = Some(problem.node);
        while let Some(node) = at {
            if let Some(symbol) = model.symbol_of(node) {
                return &model.symbols[symbol].name;
            }
            at = model.parent(node);
        }
        ""
    }

    /// The path of the file a problem points into, and whether it points at a `Use`.
    fn at_use<'w>(workspace: &'w Workspace, problem: &Problem) -> (&'w str, bool) {
        let source = &workspace.model.sources[problem.node.file];
        (
            source.path.as_str(),
            workspace.model.node(problem.node).is(Statement::Use),
        )
    }

    /// The text a file of the program was read as, from the tokens it was lexed into.
    fn text_of(workspace: &Workspace, path: &str) -> String {
        let file = workspace
            .file(path)
            .unwrap_or_else(|| panic!("{path} is not in the program"));
        let tree = workspace.tree(file);
        tree.raw(0, tree.tokens.len())
    }

    fn position(workspace: &Workspace, path: &str) -> usize {
        workspace
            .files
            .iter()
            .position(|file| file.path == path)
            .unwrap_or_else(|| panic!("{path} is not in the program"))
    }

    // @lfy def/workspace/main.lfy:load#load:load:364469ca73386a0fadcdccb16065c7b1fbd8cf7954391bff47aa5ac292a37ca7
    // @lfy def/workspace/main.lfy:load#load:load:62b4faedac048472b0892584131d2aca0e6fd7987b34a537d17991af62f3d07d
    // @lfy def/workspace/main.lfy:load#load:load:9f0992576ed1410f00587a7cf8d3c50161589f2a0ff4ee398fb06562182d994b
    // @lfy def/workspace/main.lfy:load#load:load:61e5351e7c9765ef86ef906d60e8806eac1fe5c87337507d464bf5b10d0c47d1
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
    // @lfy def/workspace/main.lfy:load#load:load:51bbb0f1c540e6e7509d7863a8ffc357fde4a36b5fe3f7fe530e391ebf443b0c
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
    // @lfy def/workspace/main.lfy:load#load:load:9f0992576ed1410f00587a7cf8d3c50161589f2a0ff4ee398fb06562182d994b
    // @lfy def/workspace/main.lfy:load#load:load:bd0b1a23fb66f1906e8450409d05cadd0110ab5356c796a743fb941d9ee761be
    // @lfy def/workspace/main.lfy:load#load:load:1a70bf9ff7ad610ada4250ea97c2b74dff227791a63bfff413e8eb49f991aefc
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
    // @lfy def/workspace/main.lfy:load#load:load:789f453b852db580976af75333afef9fc202eab05a47f26e8682b28ecfc82ba9
    // @lfy def/workspace/main.lfy:load#load:load:97bed938a5d75f39ac08d69a7e0ac6d7a4a2fd040d8d2d4ffa31571a4bc63d87
    // @lfy def/workspace/main.lfy:load#load:load:95adb658d370404e9a9733a98206f9323b5efdaf143f4e927a012d32b58d95c3
    // @lfy def/workspace/main.lfy:load#load:load:b081ce9e0d486c569c1c1c0e6c03c97743a2475743273a7eb2d20dd13b85a5dd
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
    // @lfy def/workspace/main.lfy:load#load:load:58b433ac3e636c5a378c9d49965ffc04bf659137970af95eceb1e3e742a81b5d
    #[test]
    fn a_cycle_keeps_discovery_order_and_flags_each_use() {
        let fixture = Fixture::empty();
        fixture
            .write("def/a.lfy", "use \"./b\";\n")
            .write("def/b.lfy", "use \"./a\";\n");
        let workspace = fixture.load();
        assert_eq!(paths(&workspace), ["def/a.lfy", "def/b.lfy"]);
        assert_eq!(load_problems(&workspace), Vec::<&LoadProblem>::new());
        // One `Problem` at each `Use` of the cycle. @lfy def/workspace/main.lfy:load
        let problems = bind_problems(&workspace);
        assert_eq!(problems.len(), 2, "{problems:?}");
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
    // @lfy def/workspace/main.lfy:load#load:load:195232c069d5739fe5849c18ff9275884b3c6e14fe131eb1b9558e60ac66fbb1
    #[test]
    fn a_use_of_nothing_gives_a_problem_and_an_undefined_entry() {
        let fixture = Fixture::empty();
        fixture.write("def/main.lfy", "\nuse \"./missing\";\n");
        let workspace = fixture.load();
        assert_eq!(paths(&workspace), ["def/main.lfy"]);
        assert_eq!(load_problems(&workspace), Vec::<&LoadProblem>::new());
        // One `Problem` at the `Use`. @lfy def/workspace/main.lfy:load
        let problems = bind_problems(&workspace);
        assert_eq!(problems.len(), 1, "{problems:?}");
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
    // @lfy def/workspace/main.lfy:load#load:load:2aab5e323a05c38556b9c01ff598624261dd793bba90223c51b83ddaad02c11b
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

    // @lfy def/workspace/main.lfy:load#load:load:6358d204bfb21de03d38e1c36ba9b00314a60d5dbab459ca070e67220c2c3d2d
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
    // @lfy def/workspace/main.lfy:load#load:load:3930201be17af05974f6f1764085a3edc64b3ba3dedbeb1e2f3b5c0cc7f18287
    // @lfy def/workspace/main.lfy:load#load:load:8e0e8580bd6d20a57045d50d01bfd611f89b99b8dfdd8c42f39034207e1d5df5
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
    // @lfy def/workspace/main.lfy:load#load:load:5088119ce3359a85ec36ef5adecc9df2f9525193df649a0f54a42d28b3c495ad
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

    // @lfy def/workspace/main.lfy:load#load:load:e6a3d9e7c6343bde8007bc07f7cc1bf459122fbfb7bafc0ff5375ddcf301f38f
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

    // @lfy def/workspace/main.lfy:load#load:load:789f453b852db580976af75333afef9fc202eab05a47f26e8682b28ecfc82ba9
    // @lfy def/workspace/main.lfy:load#load:load:b081ce9e0d486c569c1c1c0e6c03c97743a2475743273a7eb2d20dd13b85a5dd
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
        assert_eq!(workspace.files.len(), workspace.model.sources.len()); // @lfy def/workspace/main.lfy:load
        for (index, file) in workspace.files.iter().enumerate() {
            assert_eq!(file.source, index);
            assert_eq!(workspace.model.sources[index].path, file.path);
        }
        assert!(project(&workspace).all(|file| file.package.is_none()));
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    }

    // @lfy def/workspace/main.lfy:load#load:load:e7a894d9316ecd54b4cc1d1f1709c7e7173c47e17338f9dae68bc46451ddf867
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

    // @lfy def/workspace/main.lfy:load#load:load:e98819dddceb631ccf822f6585d7dfedfde26bd030ba4a7cbbcd5a2d312ebe01
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

    // @lfy def/workspace/main.lfy:load#load:load:32c4ce3999a76a9da6916b0795489e2740882845c1a6391b9eaf673eaebe1973
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
    // @lfy def/workspace/main.lfy:load#load:load:195232c069d5739fe5849c18ff9275884b3c6e14fe131eb1b9558e60ac66fbb1
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

    // @lfy def/workspace/main.lfy:load#load:load:0ddc30be18742fce83c1bc733c6db516b1f02d4e58205740f7b26d57638aef3a
    // @lfy def/workspace/main.lfy:load#load:load:97bed938a5d75f39ac08d69a7e0ac6d7a4a2fd040d8d2d4ffa31571a4bc63d87
    // @lfy def/workspace/main.lfy:load#load:load:c0835208419dfb83a0b84ffda822c8fbd72e4221cd95542e1785d76cd7e2a3f2
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

    // @lfy def/workspace/main.lfy:load#load:load:58b433ac3e636c5a378c9d49965ffc04bf659137970af95eceb1e3e742a81b5d
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
    // @lfy def/workspace/main.lfy:load#load:load:51bbb0f1c540e6e7509d7863a8ffc357fde4a36b5fe3f7fe530e391ebf443b0c
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
    // @lfy def/workspace/main.lfy:load#load:load:62b4faedac048472b0892584131d2aca0e6fd7987b34a537d17991af62f3d07d
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
    // @lfy def/workspace/main.lfy:load#load:load:51bbb0f1c540e6e7509d7863a8ffc357fde4a36b5fe3f7fe530e391ebf443b0c
    // @lfy def/workspace/main.lfy:load#load:load:e6a3d9e7c6343bde8007bc07f7cc1bf459122fbfb7bafc0ff5375ddcf301f38f
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
    // @lfy def/workspace/main.lfy:load#load:load:a917a90f036a011701ebcd1c4eba1e462e6225289deab81a4ee023ed381cb249
    // @lfy def/workspace/main.lfy:load#load:load:9c4b7f6784e978ab38426f62744c733c2fbdfebea9fe78c455d429cc1f876075
    // @lfy def/workspace/main.lfy:load#load:load:666f1d127b736248ed7d4d08c806f2162dc72e498eb408f0bd435db09b17e550
    // @lfy def/workspace/main.lfy:load#load:load:f93277efea1e9ae1024146b1325d3d1bc82ec07b0fc2d632c73a5bbbbfb49f17
    // @lfy def/workspace/main.lfy:load#load:load:f564aa87edbe96d9c1792965324682dbb6970be8d40d4eb3f241cea06b3e1d23
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
    // @lfy def/workspace/main.lfy:load#load:load:a917a90f036a011701ebcd1c4eba1e462e6225289deab81a4ee023ed381cb249
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
    // @lfy def/workspace/main.lfy:load#load:load:666f1d127b736248ed7d4d08c806f2162dc72e498eb408f0bd435db09b17e550
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
    // @lfy def/workspace/main.lfy:load#load:load:f564aa87edbe96d9c1792965324682dbb6970be8d40d4eb3f241cea06b3e1d23
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
    // @lfy def/workspace/main.lfy:load#load:load:3f14c93383986089ff035033d9d411beb32a7ba58ae22f7b4b0b105bee9bb8ce
    // @lfy def/workspace/main.lfy:load#load:load:744ae66bb854af2ca9b87b6bdde697e61172669260c42dcdea808e88ddd17c71
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
    // @lfy def/workspace/main.lfy:load#load:load:46b544e43774a3df072b517651d8bcbacfa236b57aab48f219ece1ed7684bfe2
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
    // @lfy def/workspace/main.lfy:load#load:load:5ed9e5077c0cbb9ff30167add3a5f60066247bb6e0a9e0b6ea303db647af75a5
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
    // @lfy def/workspace/main.lfy:load#load:load:b251619aed48e08dc83a4b322d9a09864e78bc42fca67da4fe0d6239e2fefd42
    // @lfy def/workspace/main.lfy:load#load:load:850852d92a73a5a2483681f4ad973850de80c939bcf5118d7a8ef998b4d12671
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
        // @lfy def/workspace/main.lfy:load
        for (problem, (slot, name, role)) in problems.iter().zip([
            ("language", "flat", "targetLanguage"),
            ("layout", "lang", "targetLayout"),
        ]) {
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
    // @lfy def/workspace/main.lfy:load#load:load:11e63bade5d611b5f953483b32e186473ddc2004974ec56616e5746b77bcc070
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
    // @lfy def/workspace/main.lfy:load#load:load:5170cd069bfccbfeb0da6afd8e0b423d9521e2dccd0c7bf2623176d1f3d1eadb
    // @lfy def/workspace/main.lfy:load#load:load:bc0738a446361cfaa81096bbde52491c556bc0acb26fb38242fd50e928208c6e
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

    // @lfy def/workspace/main.lfy:load#load:load:1a70bf9ff7ad610ada4250ea97c2b74dff227791a63bfff413e8eb49f991aefc
    // @lfy def/workspace/main.lfy:load#load:load:b081ce9e0d486c569c1c1c0e6c03c97743a2475743273a7eb2d20dd13b85a5dd
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

    /// The same `path` names a different file in each workspace, because it is relative
    /// to that workspace's own root and to nothing else.
    // @lfy def/workspace/main.lfy:change#change:change:e20858b36c04a1222bbb5e30cc7a08e1f9fc907a1fa76343fcd01dc323b36dc0
    // @lfy def/workspace/main.lfy:change#change:change:c2184e9afc7c73ab41f34979bb5caf4d0b47bc512bd9a05774ac2a16f35ffefe
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
    // @lfy def/workspace/main.lfy:change#change:change:07a83f96d5491695c4c70bcd43ae88989b4641476fbba210b20f253a537ff0b5
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

    // @lfy def/workspace/main.lfy:change#change:change:c2184e9afc7c73ab41f34979bb5caf4d0b47bc512bd9a05774ac2a16f35ffefe
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
        // change there names none of the three places, so the result matches the
        // workspace it was given, down to the replacement that stood before.
        let gone = change(&changed, "def/main.lfy", Some("\n"));
        assert_eq!(paths(&gone), ["def/main.lfy"]);
        for text in [Some("trait other { }\n"), None] {
            assert_eq!(change(&gone, "elsewhere/thing.lfy", text), gone);
        }
    }

    /// A file under the root of a package in the program is in reach of a change, as much
    /// as one under the source directory.
    // @lfy def/workspace/main.lfy:change#change:change:c2184e9afc7c73ab41f34979bb5caf4d0b47bc512bd9a05774ac2a16f35ffefe
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
}
