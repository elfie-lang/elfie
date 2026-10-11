// @lfy def/workspace/main.lfy:load
//! Loading a project from disk and binding it: the layout `elfie.json` gives, the
//! packages it names, the discovery of every file, the order the files are bound in, and
//! the problems loading adds that only the bound model can point at.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use super::targets::targets_of;
use super::*;
use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::statement::Statement;
use crate::grammar::terminals::literal::Literal;
use crate::lexer::lex;
use crate::model::{self, FileId, KnowledgeKind, Model, Origin, Problem, Source};
use crate::parser::{Node, Tree, parse};

/// The project layout file, at the root of the project.
///
/// Decision: the definition names what `elfie.json` holds but never how it is spelled, so
/// this compile fixes the spelling as a JSON object with the optional keys `"name"`,
/// `"source"`, `"lib"`, and `"dependencies"` (an object of identifier to `{ "root" }`).
/// One of those whose value is of the wrong kind is a `LoadProblem` and the rest of the
/// manifest still stands; any other key is never looked at.
pub(super) const MANIFEST: &str = "elfie.json";
/// `Workspace.sourceDirectory` when `elfie.json` gives none.
pub(super) const DEFAULT_SOURCE_DIRECTORY: &str = "def"; // @lfy def/workspace/main.lfy:load
/// The file a directory resolves to.
pub(super) const MAIN_FILE: &str = "main.lfy";
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
pub(super) const BUILT_IN_LIBRARY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lib"); // @lfy def/workspace/main.lfy:load
/// What a knowledge source never begins with, because knowledge is vendored.
const NETWORK: [&str; 2] = ["http://", "https://"]; // @lfy def/workspace/main.lfy:load

/// A project loaded from disk and bound.
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

/// [`load`] with the files in `overlays` read as the text given there instead of what is
/// on disk, and `environment` standing for what the environment gives for `ELFIE_LIB`.
// @lfy def/workspace/main.lfy:change
// @lfy def/workspace/main.lfy:load
pub(super) fn load_in(
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
    // @lfy def/workspace/main.lfy:load#load:load:f3a8e524e11cd59e038a6157e4a0fe362764b553e2b29342b421c3e379ad7cb9
    // @lfy def/workspace/main.lfy:load#load:load:14ff48a32bf3381366534b9ca3dbdb2f1d46fc634249e46bf5134fb6609d3a39
    // @lfy def/workspace/main.lfy:load#load:load:fd884247a5f63218e2f23f0ba9abdb91e6a342d07f56f61a79d4f23bfe3eeee7
    let name = loader
        .string(manifest.as_ref(), "name")
        .unwrap_or_else(|| directory_name(root));
    // @lfy def/workspace/main.lfy:load#load:load:553254e418671149fdc2a87c765c1ff91ddcf349bad7d7055bf05a5234721178
    // @lfy def/workspace/main.lfy:load#load:load:38338fb46f57a64fa2e744d34eae3279caf9b613709e743808ac4f6fba4a0289
    let source_directory = loader
        .string(manifest.as_ref(), "source")
        .map(|source| trim_directory(&source))
        .unwrap_or_else(|| DEFAULT_SOURCE_DIRECTORY.to_string());

    // Dependencies. @lfy def/workspace/main.lfy:load
    let mut packages = loader.packages(manifest.as_ref());
    // The package elfie is in the program whether or not elfie.json names it.
    // @lfy def/workspace/main.lfy:load#load:load:932b815f66767e3f29654d009afd1f258dc9f22040a21b8770d2e7aa470350e2
    // @lfy def/workspace/main.lfy:load#load:load:1c98732ad9dcd0ae1423690fc2bd117e7dbc13effc288ef7a15560f86a0d3064
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
        // @lfy def/workspace/main.lfy:load#load:load:387544d4a426f77295d5b183dd0d518a256f465f5f332556561411266aca0a8f
        loader.problem(None, format!("the root {} does not exist", root.display()));
    } else if !loader.directory(&source_directory) {
        // @lfy def/workspace/main.lfy:load#load:load:387544d4a426f77295d5b183dd0d518a256f465f5f332556561411266aca0a8f
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
            // @lfy def/workspace/main.lfy:load#load:load:b1fbb044be3c701778d0f3eed4d4ad278cdf34c2b917c9b0a33327367959d921
            loader.use_problem(&path, index, spelled.as_deref(), "is part of a cycle");
        }
        for index in order {
            let entry = &entries[index];
            let Some(tree) = &entry.tree else { continue };
            files.push(File {
                // @lfy def/workspace/main.lfy:load#load:load:23324d830b23ca1be67dea3c9234abb1945519a3e56b47c394222debdf328fe8
                path: entry.path.clone(),
                // @lfy def/workspace/main.lfy:load#load:load:247e20d07c0c842a9a7e7075746d74be2254640efab3b1062fdf4d1aec785a68
                // @lfy def/workspace/main.lfy:load#load:load:f0c460549fc5debac126dc93031902458c9a539417d42b3b143b29ce41672943
                package: entry.package,
                // @lfy def/workspace/main.lfy:load#load:load:c4acf67876f96707cb66e2a889f79373ce852c3192472acb1b3dbff88d3d190a
                source: sources.len(),
            });
            sources.push(Source {
                path: entry.path.clone(),
                // @lfy def/workspace/main.lfy:load#load:load:692ecc7501d808fc5c758f804195b305d9436a6d7de2ce76f48185ac315cfdb7
                tree: tree.clone(),
                // @lfy def/workspace/main.lfy:load#load:load:0fac45ffcbab6307a0cadab9357862b79b40b58f5064ecf56d4f03ef03659a37
                uses: entry
                    .uses
                    .iter()
                    .map(|site| site.resolved.clone())
                    .collect(),
                // @lfy def/workspace/main.lfy:load#load:load:42ddd4111d6e51a0395e9b1eee8c79a065bd9e53b143029640c9ed2dd233c078
                origin: origin_of(&entry.path, entry.package, library, &packages[library].root),
            });
        }
    }

    // Binding, once.
    // @lfy def/workspace/main.lfy:load#load:load:f9beb9689c54303ed4e3a7e5f60e42c6dfd70ba8603fe85a05eba3cd28fdf395
    let mut model = model::bind(sources);

    // Only the bound model can point at these: a use that resolves to nothing or is part
    // of a cycle, the targets the program declares, and the knowledge its entities name.
    // Each of them is a problem loading adds rather than one binding found, so each one
    // carries `Stage::Loader`.
    // @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load#load:load:83c8947a99852c5cfe9afc540029580ba37bedf5bfe20bba7ed77e79ebb98642
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
        // @lfy def/workspace/main.lfy:load#load:load:d89e1a3dc76a0474ada8c0015af825a5f5e8c5e26f0b09d9280c88429bbcfba0
        root: root.to_path_buf(),
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
    // @lfy def/workspace/main.lfy:load#load:load:19a7d4fd318b9f7efc5cbe88610e53a0e61be4be62d5831f8646cc1bad17a9ad
    // @lfy def/workspace/main.lfy:load#load:load:8b89b53b4efb9ae82c650c075c04f5a708529dbc6cec061bdfde65ae7811ca3c
    // @lfy def/workspace/main.lfy:load#load:load:e1ed96f364bdc98469b9736cdb2e0ce1a008d6ac0623f78e2cb68c8ccaf817a5
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
    // @lfy def/workspace/main.lfy:load#load:load:d60b7d2fc94b097354076890d595ac2106b1b65e366b4cc45271b9e7211aa3cb
    fn string(
        &mut self,
        manifest: Option<&serde_json::Map<String, serde_json::Value>>,
        key: &str,
    ) -> Option<String> {
        match manifest.and_then(|manifest| manifest.get(key)) {
            Some(serde_json::Value::String(value)) => Some(value.clone()),
            None => None,
            // @lfy def/workspace/main.lfy:load#load:load:b5997648b346dea065f6d06cc85024aa35df3796ec9edbc10d87099971bd1b6b
            Some(_) => {
                self.problem(Some(MANIFEST), format!("{key:?} is not a string"));
                None
            }
        }
    }

    /// Each dependency `elfie.json` names gives one package.
    // @lfy def/workspace/main.lfy:load#load:load:2c93d3e7305993d3a84ba8f451e28d1e5401c5189c591e98f5d0fe63976426c5
    // @lfy def/workspace/main.lfy:load#load:load:b5997648b346dea065f6d06cc85024aa35df3796ec9edbc10d87099971bd1b6b
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
        // @lfy def/workspace/main.lfy:load#load:load:05b8fa2948fcab3e2f28a0331ccd1f1bfbe7b52d62225256478e886cf3e30d7b
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
    // @lfy def/workspace/main.lfy:load#load:load:d907c299c80f931a0deb7cbd87d12cf1f50320cdcfc62ec0ea02ea3217ce3800
    // @lfy def/workspace/main.lfy:load#load:load:894c6485f9e8ac83db6e6d6dd3340e9ff0e21ccc789a23347307d15fb610664f
    // @lfy def/workspace/main.lfy:load#load:load:18303ab51dd7497f5981f5a7ebbbefa64f43fca6e87ab6ac5a8aa9acaa26e95f
    // @lfy def/workspace/main.lfy:load#load:load:a08b7bac9a52807fdcd4c5544ebeb10d60db2a277aef3e01ba0ac257e71291a7
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
    // @lfy def/workspace/main.lfy:load#load:load:247e20d07c0c842a9a7e7075746d74be2254640efab3b1062fdf4d1aec785a68
    // @lfy def/workspace/main.lfy:load#load:load:f0c460549fc5debac126dc93031902458c9a539417d42b3b143b29ce41672943
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
    // @lfy def/workspace/main.lfy:load#load:load:a7bb1ab332ef59bd7ef0b193ca2257a60b8457f57538d11c2fd2c2f0c2610ea5
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
                // @lfy def/workspace/main.lfy:load#load:load:a7bb1ab332ef59bd7ef0b193ca2257a60b8457f57538d11c2fd2c2f0c2610ea5
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
        // @lfy def/workspace/main.lfy:load#load:load:36d73adceb24794b868cfec5c1c2f1bb0befd1aee1a502407f8158d70d49ee14
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
                        // @lfy def/workspace/main.lfy:load#load:load:5d9353c2d7ef127d55101933cf2d1581e1bf8dfae943a0d3d59cf83a5cb17679
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
                // @lfy def/workspace/main.lfy:load#load:load:5d9353c2d7ef127d55101933cf2d1581e1bf8dfae943a0d3d59cf83a5cb17679
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
            // @lfy def/workspace/main.lfy:load#load:load:7cd3d4046cd7170c71b67d6747111258f280a6b304e31b968c1adf210b007213
            join(directory_of(from), spelled)
        } else {
            // @lfy def/workspace/main.lfy:load#load:load:1686133b540c858b4c1ab7edb631811da85b909c80a52d63d4084d22d6c3251f
            let (first, rest) = spelled.split_once('/').unwrap_or((spelled, ""));
            let package = packages
                .iter()
                .find(|package| package.identifier == first)
                .ok_or_else(|| format!("no package is named {first:?}"))?;
            join(&package.root, rest)
        };
        let base = normalize(&base);
        // @lfy def/workspace/main.lfy:load#load:load:0ddc30be18742fce83c1bc733c6db516b1f02d4e58205740f7b26d57638aef3a
        let file = format!("{base}.{EXTENSION}");
        if self.exists(&file) {
            return Ok(file);
        }
        // @lfy def/workspace/main.lfy:load#load:load:97bed938a5d75f39ac08d69a7e0ac6d7a4a2fd040d8d2d4ffa31571a4bc63d87
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
            // @lfy def/workspace/main.lfy:load#load:load:05f2e92c57fc3eec4e710b8bd1bf35d7f4f2ecf0a40b1d8fc331d549eef5bc33
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
            // @lfy def/workspace/main.lfy:load#load:load:bac7aeb8877e25e34bb8600328dcdfd9765c1662f19b2c785ef6dc1eb24ddbf2
            let root = model.entities[item.contributor]
                .file
                .and_then(|file| package_of.get(&file))
                .map_or(".", |&package| packages[package].root.as_str());
            // @lfy def/workspace/main.lfy:load#load:load:14ac3e3c23396d2f0892c408a3681bf564363703aeb00ea0caa90879bf9f0b26
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
// @lfy def/workspace/main.lfy:load#load:load:912de90eec4ab7a10c1d9fb7853c226b0130e0f61558b54793fccef20e344b08
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
// @lfy def/workspace/main.lfy:load#load:load:95adb658d370404e9a9733a98206f9323b5efdaf143f4e927a012d32b58d95c3
// @lfy def/workspace/main.lfy:load#load:load:b1fbb044be3c701778d0f3eed4d4ad278cdf34c2b917c9b0a33327367959d921
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
        // @lfy def/workspace/main.lfy:load#load:load:5e03556496a99f7d2d13c4effb8ae2e0d1208153ea90ec6e70a7592ef184a1d5
        component.sort_unstable();
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
