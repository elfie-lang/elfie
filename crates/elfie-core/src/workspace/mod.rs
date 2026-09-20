//! Compiled from `def/workspace/main.lfy`: loading a project from disk and binding it.
//!
//! [`load`] reads the project layout from `elfie.json`, discovers every `.lfy` file under
//! the source directory and under every package root, follows every `use` to the file it
//! names, orders the files so each comes after the files it uses, and binds them once.
//! Loading never stops for a failure: every failure is a [`LoadProblem`] or a `Problem`
//! and the rest of the project still loads. [`change`] gives a workspace in which one file
//! is read as other text, by loading the same root again with that replacement in place.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

pub mod data; // @lfy def/workspace/main.lfy:7

pub use data::*;

use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::statement::Statement;
use crate::grammar::terminals::literal::Literal;
use crate::lexer::lex;
use crate::model::{self, EntityId, Model, Problem, Source, SymbolKind};
use crate::parser::{Node, Tree, parse};

/// The project layout file, at the root of the project and of every package.
///
/// Decision: the definition names what `elfie.json` holds but never how it is spelled, so
/// this compile fixes the spelling as a JSON object with the optional keys `"name"`,
/// `"source"`, `"output"`, `"dependencies"` (an object of identifier to `{ "root" }`),
/// `"targets"` (an object of identifier to `{ "package", "marker", "output"? }`), and
/// `"native"` (a list of `{ "identifier", "ecosystem", "version"? }`). Anything shaped
/// otherwise is a `LoadProblem` and the rest of the manifest still stands.
const MANIFEST: &str = "elfie.json";
/// `Workspace.sourceDirectory` when `elfie.json` gives none.
const DEFAULT_SOURCE_DIRECTORY: &str = "def"; // @lfy def/workspace/main.lfy:load
/// `Workspace.outputDirectory` when `elfie.json` gives none.
const DEFAULT_OUTPUT_DIRECTORY: &str = "src"; // @lfy def/workspace/main.lfy:load
/// The file a directory resolves to, and the file whose scope a target marker is looked
/// up in.
const MAIN_FILE: &str = "main.lfy";
/// The extension of a source file.
const EXTENSION: &str = ".lfy";

/// Load a project from disk and bind it.
///
/// Reads the project layout, discovers every file, orders the files by what they use,
/// then binds them once. `Workspace::root` is `root`. Loading never stops for a failure:
/// every failure is a `LoadProblem` or a `Problem`, and the rest of the project still
/// loads.
// @lfy def/workspace/main.lfy:load
pub fn load(root: &Path) -> Workspace {
    load_with(root, BTreeMap::new())
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
/// program gives back a workspace equal to `workspace`.
// @lfy def/workspace/main.lfy:change
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
    load_with(&workspace.root, overlays) // @lfy def/workspace/main.lfy:change
}

/// Whether a change at `path` could reach the program: the path is a source file under
/// the source directory or a package root, a `Use` resolves to it, or it is replaced now.
// @lfy def/workspace/main.lfy:change
fn in_reach(workspace: &Workspace, path: &str) -> bool {
    // Decision: the definition names only the source directory and the uses. A file under
    // a package root is in the program too, so a change there must reach it as well; and
    // a path replaced earlier must be reachable so that giving it again can undo the
    // replacement.
    let is_source = path.ends_with(EXTENSION);
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
        || workspace.overlays.contains_key(path)
}

/// [`load`] with the files in `overlays` read as the text given there instead of what is
/// on disk.
// @lfy def/workspace/main.lfy:change
fn load_with(root: &Path, overlays: BTreeMap<String, String>) -> Workspace {
    let mut loader = Loader {
        root,
        overlays: &overlays,
        problems: Vec::new(),
        use_problems: Vec::new(),
    };

    // Layout. @lfy def/workspace/main.lfy:load
    let manifest = loader.manifest(MANIFEST);
    let name = manifest
        .as_ref()
        .and_then(|manifest| manifest.get("name")?.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| directory_name(root)); // @lfy def/workspace/main.lfy:load
    let source_directory = manifest
        .as_ref()
        .and_then(|manifest| manifest.get("source")?.as_str())
        .map(trim_directory)
        .unwrap_or_else(|| DEFAULT_SOURCE_DIRECTORY.to_string()); // @lfy def/workspace/main.lfy:load
    let output_directory = manifest
        .as_ref()
        .and_then(|manifest| manifest.get("output")?.as_str())
        .map(trim_directory)
        .unwrap_or_else(|| DEFAULT_OUTPUT_DIRECTORY.to_string()); // @lfy def/workspace/main.lfy:load

    // Dependencies. @lfy def/workspace/main.lfy:load
    let packages = loader.packages(manifest.as_ref());
    let target_specs = loader.target_specs(manifest.as_ref());
    let native_dependencies = match &manifest {
        Some(manifest) => loader.native_dependencies(MANIFEST, manifest), // @lfy def/workspace/main.lfy:load
        None => Vec::new(),
    };

    // Discovery, use, and order. @lfy def/workspace/main.lfy:load
    let mut sources = Vec::new();
    let mut files = Vec::new();
    if !root.is_dir() {
        // @lfy def/workspace/main.lfy:load
        loader.problem(None, format!("the root {} does not exist", root.display()));
    } else if !root.join(&source_directory).is_dir() {
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
            });
        }
    }

    // Binding, once. @lfy def/workspace/main.lfy:load
    let mut model = model::bind(sources);

    // A use that resolves to nothing, and a use that is part of a cycle, are problems at
    // the `Use` itself, which only the bound model can point at.
    // @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load
    let mut at_uses = use_problems(&model, &loader.use_problems);
    if !at_uses.is_empty() {
        model.problems.append(&mut at_uses);
        model
            .problems
            .sort_by_key(|problem| (problem.node.file, problem.node.index));
    }

    // Targets. @lfy def/workspace/main.lfy:load
    let mut targets = Vec::new();
    for spec in target_specs {
        let Some(package) = packages
            .iter()
            .position(|package| package.identifier == spec.package)
        else {
            loader.problem(
                None,
                format!(
                    "the target {} names the package {:?}, which elfie.json does not list as a dependency",
                    spec.identifier, spec.package
                ),
            );
            continue;
        };
        let main = join(&packages[package].root, MAIN_FILE);
        // @lfy def/workspace/main.lfy:load
        let symbol = model
            .file(&main)
            .and_then(|file| model.file_scopes.get(file).copied())
            .and_then(|scope| model.lookup(scope, &spec.marker))
            .and_then(|symbol| model.symbols.get(symbol));
        match symbol {
            Some(symbol) if symbol.kind == SymbolKind::Trait => targets.push(Target {
                identifier: spec.identifier, // @lfy def/workspace/main.lfy:load
                marker: symbol.entity,       // @lfy def/workspace/main.lfy:load
                package,                     // @lfy def/workspace/main.lfy:load
                output_directory: spec.output.unwrap_or_else(|| output_directory.clone()), // @lfy def/workspace/main.lfy:load
            }),
            // @lfy def/workspace/main.lfy:load
            Some(symbol) => loader.problem(
                Some(&main),
                format!(
                    "the marker {:?} of the target {} resolves to a {}, not a trait",
                    spec.marker, spec.identifier, symbol.kind
                ),
            ),
            None => loader.problem(
                Some(&main),
                format!(
                    "the marker {:?} of the target {} resolves to nothing in the file scope of {main}",
                    spec.marker, spec.identifier
                ),
            ),
        }
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
        output_directory,
        files,
        model,
        packages,
        targets,
        native_dependencies,
        problems,
        overlays,
    }
}

/// One target as `elfie.json` spells it, before its marker is resolved.
// @lfy def/workspace/main.lfy:load
struct TargetSpec {
    identifier: String,
    package: String,
    marker: String,
    output: Option<String>,
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

    /// The text of the file at a path: what it is replaced with, or what is on disk.
    fn read(&self, path: &str) -> std::io::Result<String> {
        match self.overlays.get(path) {
            Some(text) => Ok(text.clone()),
            None => std::fs::read_to_string(self.root.join(path)),
        }
    }

    /// The object of an `elfie.json`; `None` when there is none, or when it cannot be
    /// read or is not an object, which adds a problem.
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

    /// The native dependencies an `elfie.json` names under `"native"`.
    // @lfy def/workspace/main.lfy:load
    fn native_dependencies(
        &mut self,
        path: &str,
        manifest: &serde_json::Map<String, serde_json::Value>,
    ) -> Vec<NativeDependency> {
        let mut out = Vec::new();
        let Some(native) = manifest.get("native") else {
            return out;
        };
        let Some(list) = native.as_array() else {
            self.problem(Some(path), "\"native\" is not a list".to_string());
            return out;
        };
        for (index, item) in list.iter().enumerate() {
            let identifier = item.get("identifier").and_then(|v| v.as_str());
            let ecosystem = item.get("ecosystem").and_then(|v| v.as_str());
            let version = item.get("version");
            match (identifier, ecosystem, version) {
                (Some(identifier), Some(ecosystem), None) => out.push(NativeDependency {
                    identifier: identifier.to_string(),
                    ecosystem: ecosystem.to_string(),
                    version: None,
                }),
                (Some(identifier), Some(ecosystem), Some(serde_json::Value::String(version))) => {
                    out.push(NativeDependency {
                        identifier: identifier.to_string(),
                        ecosystem: ecosystem.to_string(),
                        version: Some(version.clone()),
                    })
                }
                _ => self.problem(
                    Some(path),
                    format!(
                        "\"native\" entry {index} needs string \"identifier\" and \"ecosystem\" and an optional string \"version\""
                    ),
                ),
            }
        }
        out
    }

    /// Each dependency `elfie.json` names gives one package, with the native
    /// dependencies of its own `elfie.json`.
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
            let root = trim_directory(root);
            // @lfy def/workspace/main.lfy:load
            // Checked here rather than in `discover`, which does not run when the root or
            // the source directory is missing; a missing package root is reported either
            // way.
            if !self.root.join(&root).is_dir() {
                self.problem(
                    Some(&root),
                    format!("the root of the package {identifier} does not exist"),
                );
            }
            let native_dependencies = match self.manifest(&join(&root, MANIFEST)) {
                Some(package_manifest) => {
                    self.native_dependencies(&join(&root, MANIFEST), &package_manifest)
                } // @lfy def/workspace/main.lfy:load
                None => Vec::new(),
            };
            out.push(Package {
                identifier: identifier.clone(),
                root,
                native_dependencies,
            });
        }
        out
    }

    /// Each target `elfie.json` names, as spelled.
    // @lfy def/workspace/main.lfy:load
    fn target_specs(
        &mut self,
        manifest: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Vec<TargetSpec> {
        let mut out = Vec::new();
        let Some(targets) = manifest.and_then(|manifest| manifest.get("targets")) else {
            return out;
        };
        let Some(targets) = targets.as_object() else {
            self.problem(Some(MANIFEST), "\"targets\" is not an object".to_string());
            return out;
        };
        for (identifier, target) in targets {
            let package = target.get("package").and_then(|v| v.as_str());
            let marker = target.get("marker").and_then(|v| v.as_str());
            let output = target.get("output");
            let output = match output {
                None => None,
                Some(serde_json::Value::String(output)) => Some(trim_directory(output)),
                Some(_) => {
                    self.problem(
                        Some(MANIFEST),
                        format!("the target {identifier:?} has an \"output\" that is not a string"),
                    );
                    continue;
                }
            };
            let (Some(package), Some(marker)) = (package, marker) else {
                self.problem(
                    Some(MANIFEST),
                    format!("the target {identifier:?} needs string \"package\" and \"marker\""),
                );
                continue;
            };
            out.push(TargetSpec {
                identifier: identifier.clone(),
                package: package.to_string(),
                marker: marker.to_string(),
                output,
            });
        }
        out
    }

    /// Every `.lfy` file under a directory, sorted by path, replaced files included.
    // @lfy def/workspace/main.lfy:load
    fn source_files(&mut self, directory: &str) -> Vec<String> {
        // Decision: discovery order within a directory is the sorted order of the paths.
        let mut out = BTreeSet::new();
        self.walk(directory, &mut out);
        for path in self.overlays.keys() {
            if path.ends_with(EXTENSION) && under(path, directory) {
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
            } else if name.ends_with(EXTENSION) {
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
            if !self.root.join(&package.root).is_dir() {
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
        let file = format!("{base}{EXTENSION}");
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
        });
    }
    out
}

/// The entities built for a target: everything the marker trait was applied to, every
/// entity declared in the file scope of a file whose own entity carries the marker, and,
/// when `global` carries the marker, every entity of every project file. Each entity
/// appears once, in that order.
// @lfy def/workspace/main.lfy:load
pub fn entities_for(workspace: &Workspace, target: &Target) -> Vec<EntityId> {
    let model = &workspace.model;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    // @lfy def/workspace/main.lfy:load
    for entity in model::entities_of(model, target.marker) {
        if seen.insert(entity) {
            out.push(entity);
        }
    }
    // @lfy def/workspace/main.lfy:load
    for file in &workspace.files {
        let carries = model
            .file_entities
            .get(file.source)
            .is_some_and(|&entity| model.entities[entity].has_trait(target.marker));
        if !carries {
            continue;
        }
        let Some(&scope) = model.file_scopes.get(file.source) else {
            continue;
        };
        for &symbol in &model.scopes[scope].symbols {
            let entity = model.symbols[symbol].entity;
            if seen.insert(entity) {
                out.push(entity);
            }
        }
    }
    // Every entity of a project file, not only the ones its file scope declares.
    // @lfy def/workspace/main.lfy:load
    if model.entities[model.global].has_trait(target.marker) {
        let project: HashSet<usize> = workspace
            .files
            .iter()
            .filter(|file| file.package.is_none())
            .map(|file| file.source)
            .collect();
        for (entity, declared) in model.entities.iter().enumerate() {
            if declared.file.is_some_and(|file| project.contains(&file)) && seen.insert(entity) {
                out.push(entity);
            }
        }
    }
    out
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

/// A path with every `.` dropped and every `..` applied to the segment before it.
fn normalize(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => match segments.last() {
                Some(&last) if last != ".." => {
                    segments.pop();
                }
                _ => segments.push(".."),
            },
            segment => segments.push(segment),
        }
    }
    segments.join("/")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

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
            Fixture { root }
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

        fn load(&self) -> Workspace {
            load(&self.root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn paths(workspace: &Workspace) -> Vec<&str> {
        workspace
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect()
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

    // @lfy def/workspace/main.lfy:load
    #[test]
    fn an_empty_def_and_no_manifest_give_the_defaults() {
        let fixture = Fixture::empty();
        let workspace = fixture.load();
        assert_eq!(workspace.root, fixture.root); // @lfy def/workspace/main.lfy:load
        assert_eq!(
            workspace.name,
            fixture.root.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(workspace.source_directory, "def");
        assert_eq!(workspace.output_directory, "src");
        assert!(workspace.files.is_empty());
        assert!(workspace.targets.is_empty());
        assert!(workspace.packages.is_empty());
        assert!(workspace.native_dependencies.is_empty());
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        assert!(workspace.overlays.is_empty());
    }

    // @lfy def/workspace/main.lfy:load
    #[test]
    fn a_used_file_comes_before_the_file_that_uses_it() {
        let fixture = Fixture::empty();
        fixture
            .write("def/main.lfy", "use \"./rules\";\n")
            .write("def/rules/main.lfy", "trait rule { }\n");
        let workspace = fixture.load();
        assert_eq!(paths(&workspace), ["def/rules/main.lfy", "def/main.lfy"]);
        assert!(workspace.files.iter().all(|file| file.package.is_none()));
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        assert_eq!(
            uses_of(&workspace, "def/main.lfy"),
            [Some("def/rules/main.lfy".to_string())]
        );
        assert_eq!(uses_of(&workspace, "def/rules/main.lfy"), []);
    }

    // @lfy def/workspace/main.lfy:load
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

    // @lfy def/workspace/main.lfy:load
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

    // @lfy def/workspace/main.lfy:load
    #[test]
    fn the_manifest_names_the_layout() {
        let fixture = Fixture::new();
        fixture
            .write(
                "elfie.json",
                r#"{ "name": "named", "source": "source/", "output": "out" }"#,
            )
            .write("source/main.lfy", "trait t { }\n");
        let workspace = fixture.load();
        assert_eq!(workspace.name, "named");
        assert_eq!(workspace.source_directory, "source");
        assert_eq!(workspace.output_directory, "out");
        assert_eq!(paths(&workspace), ["source/main.lfy"]);
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    }

    // @lfy def/workspace/main.lfy:load
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
            assert_eq!(workspace.output_directory, "src");
            assert_eq!(paths(&workspace), ["def/main.lfy"]);
        }
    }

    // @lfy def/workspace/main.lfy:load
    #[test]
    fn a_missing_root_or_source_directory_adds_a_problem_with_no_path() {
        let fixture = Fixture::new();
        let missing = fixture.root.join("nowhere");
        let workspace = load(&missing);
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
        assert_eq!(workspace.packages.len(), 1);
    }

    // @lfy def/workspace/main.lfy:load
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
            assert!(file.package.is_none());
        }
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
    }

    // @lfy def/workspace/main.lfy:load
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

    // @lfy def/workspace/main.lfy:load
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

    // @lfy def/workspace/main.lfy:load
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

    // @lfy def/workspace/main.lfy:load
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

    // @lfy def/workspace/main.lfy:load
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

    // @lfy def/workspace/main.lfy:load
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
        assert_eq!(workspace.files.len(), 5);
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

    // @lfy def/workspace/main.lfy:load
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

    // @lfy def/workspace/main.lfy:load
    #[test]
    fn each_dependency_gives_a_package_whose_files_are_in_the_program() {
        let fixture = Fixture::empty();
        fixture
            .write(
                "elfie.json",
                r#"{
                    "dependencies": { "rust": { "root": "targets/rust/" }, "gone": { "root": "targets/gone" } },
                    "native": [
                        { "identifier": "tokio", "ecosystem": "cargo", "version": "1" },
                        { "identifier": "serde", "ecosystem": "cargo" }
                    ]
                }"#,
            )
            .write("def/main.lfy", "")
            .write("targets/rust/main.lfy", "")
            .write("targets/rust/extra/thing.lfy", "")
            .write("targets/rust/elfie.json", r#"{ "native": [ { "identifier": "clap", "ecosystem": "cargo", "version": "4" } ] }"#);
        let workspace = fixture.load();
        assert_eq!(workspace.packages.len(), 2);
        let rust = workspace
            .packages
            .iter()
            .position(|package| package.identifier == "rust")
            .unwrap();
        assert_eq!(workspace.packages[rust].root, "targets/rust");
        // @lfy def/workspace/main.lfy:load
        assert_eq!(
            workspace.packages[rust].native_dependencies,
            [NativeDependency {
                identifier: "clap".to_string(),
                ecosystem: "cargo".to_string(),
                version: Some("4".to_string())
            }]
        );
        let gone = workspace
            .packages
            .iter()
            .position(|package| package.identifier == "gone")
            .unwrap();
        assert!(workspace.packages[gone].native_dependencies.is_empty());
        // @lfy def/workspace/main.lfy:load
        assert_eq!(
            workspace.native_dependencies,
            [
                NativeDependency {
                    identifier: "tokio".to_string(),
                    ecosystem: "cargo".to_string(),
                    version: Some("1".to_string())
                },
                NativeDependency {
                    identifier: "serde".to_string(),
                    ecosystem: "cargo".to_string(),
                    version: None
                }
            ]
        );
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

    // @lfy def/workspace/main.lfy:load
    #[test]
    fn a_bad_native_entry_or_package_manifest_adds_a_problem() {
        let fixture = Fixture::empty();
        fixture
            .write("elfie.json", r#"{ "dependencies": { "p": { "root": "pkg" }, "q": 3 }, "native": [ { "identifier": "x" } ] }"#)
            .write("pkg/elfie.json", "nope")
            .write("pkg/main.lfy", "");
        let workspace = fixture.load();
        assert!(workspace.native_dependencies.is_empty());
        assert_eq!(workspace.packages.len(), 1);
        assert!(workspace.packages[0].native_dependencies.is_empty());
        let problems = load_problems(&workspace);
        let mut paths: Vec<Option<&str>> = problems
            .iter()
            .map(|problem| problem.path.as_deref())
            .collect();
        paths.sort_unstable();
        assert_eq!(
            paths,
            [
                Some("elfie.json"),
                Some("elfie.json"),
                Some("pkg/elfie.json")
            ],
            "{problems:?}"
        );
    }

    /// A missing package root is reported even when nothing can be discovered.
    // @lfy def/workspace/main.lfy:load
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

    // @lfy def/workspace/main.lfy:load
    #[test]
    fn each_target_comes_from_a_package_with_a_marker_and_an_output_directory() {
        let fixture = Fixture::empty();
        fixture
            .write(
                "elfie.json",
                r#"{
                    "output": "generated",
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": {
                        "rust": { "package": "rust", "marker": "rust", "output": "crates" },
                        "plain": { "package": "rust", "marker": "rust" },
                        "orphan": { "package": "missing", "marker": "rust" },
                        "absent": { "package": "rust", "marker": "nothing" }
                    }
                }"#,
            )
            .write("def/main.lfy", "")
            .write("targets/rust/main.lfy", "trait rust { }\n");
        let workspace = fixture.load();
        // @lfy def/workspace/main.lfy:load
        assert_eq!(workspace.packages.len(), 1);
        assert!(workspace.file("targets/rust/main.lfy").is_some());
        let problems = load_problems(&workspace);
        // @lfy def/workspace/main.lfy:load
        assert!(
            problems
                .iter()
                .any(|problem| problem.message.contains("orphan")),
            "{problems:?}"
        );
        assert!(
            problems
                .iter()
                .any(|problem| problem.message.contains("\"nothing\"")
                    && problem.path.as_deref() == Some("targets/rust/main.lfy")),
            "{problems:?}"
        );
        assert!(
            workspace
                .targets
                .iter()
                .all(|target| target.identifier != "orphan" && target.identifier != "absent")
        );
        let rust = workspace
            .targets
            .iter()
            .find(|target| target.identifier == "rust")
            .expect("the rust target");
        assert_eq!(rust.package, 0);
        assert_eq!(rust.output_directory, "crates"); // @lfy def/workspace/main.lfy:load
        assert!(workspace.model.entities[rust.marker].is_trait()); // @lfy def/workspace/main.lfy:load
        let plain = workspace
            .targets
            .iter()
            .find(|target| target.identifier == "plain")
            .expect("the plain target");
        assert_eq!(plain.output_directory, "generated"); // @lfy def/workspace/main.lfy:load
        assert_eq!(plain.marker, rust.marker);
    }

    /// The entities built for a target.
    // @lfy def/workspace/main.lfy:load
    #[test]
    fn the_entities_built_for_a_target_are_the_marked_ones() {
        let fixture = Fixture::empty();
        fixture
            .write(
                "elfie.json",
                r#"{
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust" } }
                }"#,
            )
            .write("targets/rust/main.lfy", "trait rust { }\n")
            .write(
                "def/main.lfy",
                "use \"rust\";\nd Marked is rust { }\nd Plain { }\n",
            );
        let workspace = fixture.load();
        let target = workspace
            .targets
            .iter()
            .find(|target| target.identifier == "rust")
            .expect("the rust target");
        // @lfy def/workspace/main.lfy:load
        assert_eq!(names(&workspace, &entities_for(&workspace, target)), ["Marked"]);
    }

    /// When a file's own entity carries the marker, every entity declared in that file's
    /// scope is built.
    // @lfy def/workspace/main.lfy:load
    #[test]
    fn a_marked_file_builds_every_entity_of_its_file_scope() {
        let fixture = Fixture::empty();
        fixture
            .write(
                "elfie.json",
                r#"{
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust" } }
                }"#,
            )
            .write("targets/rust/main.lfy", "trait rust { }\n")
            .write(
                "def/marked.lfy",
                "use \"rust\";\nrust.apply(.);\nd Plain { }\nd Other { }\n",
            )
            .write("def/bare.lfy", "d Untouched { }\n");
        let workspace = fixture.load();
        let target = workspace
            .targets
            .iter()
            .find(|target| target.identifier == "rust")
            .expect("the rust target");
        assert_eq!(
            names(&workspace, &entities_for(&workspace, target)),
            ["Plain", "Other"]
        );
    }

    /// When `global` carries the marker, every entity of every project file is built.
    // @lfy def/workspace/main.lfy:load
    #[test]
    fn global_carrying_the_marker_builds_every_entity_of_every_project_file() {
        let fixture = Fixture::empty();
        fixture
            .write(
                "elfie.json",
                r#"{
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust" } }
                }"#,
            )
            .write("targets/rust/main.lfy", "trait rust { }\nd Guidance { }\n")
            .write(
                "def/main.lfy",
                "use \"rust\";\nrust.apply(global);\nd Plain { $member: `m` = string; }\nd Other { }\n",
            );
        let workspace = fixture.load();
        let target = workspace
            .targets
            .iter()
            .find(|target| target.identifier == "rust")
            .expect("the rust target");
        let built = names(&workspace, &entities_for(&workspace, target));
        // Every entity of the project's own file, and nothing of the package.
        assert!(built.contains(&"Plain"), "{built:?}");
        assert!(built.contains(&"Other"), "{built:?}");
        // Every entity, not only the ones the file scope declares: a member counts too.
        assert!(built.contains(&"member"), "{built:?}");
        assert!(!built.contains(&"Guidance"), "{built:?}");
    }

    // @lfy def/workspace/main.lfy:load
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
            paths(&workspace)
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
    // @lfy def/workspace/main.lfy:change
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

    // @lfy def/workspace/main.lfy:change
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

    // @lfy def/workspace/main.lfy:change
    // @lfy def/workspace/main.lfy:change
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
        assert_eq!(restored.files.len(), 2);
        assert_eq!(restored, fixture.load());
        assert!(restored.overlays.is_empty());
    }

    // @lfy def/workspace/main.lfy:change
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

    // @lfy def/workspace/main.lfy:change
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

    // @lfy def/workspace/main.lfy:change
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
    }

    #[test]
    fn paths_normalize_and_join() {
        assert_eq!(normalize("def/lexer/../grammar/./main"), "def/grammar/main");
        assert_eq!(normalize("def/../../x"), "../x");
        assert_eq!(normalize("./a//b/"), "a/b");
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
