// @lfy def/workspace/main.lfy:load
//! Loading a project from disk and binding it: the configuration each `elfie.json` gives,
//! the packages its dependencies name, the discovery of every file, the name each file
//! goes by, the order the files are bound in, and the problems loading adds that only the
//! bound model can point at.

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

/// The configuration file, at the root of the project and at the root of each package.
pub(super) const MANIFEST: &str = "elfie.json";
/// The one entry of `Workspace.sourceDirectories` when `elfie.json` gives no source.
pub(super) const DEFAULT_SOURCE_DIRECTORY: &str = "def"; // @lfy def/workspace/main.lfy:load
/// The file a directory resolves to.
pub(super) const MAIN_FILE: &str = "main.lfy";
/// The copy of the library the compiler was built with.
///
/// Decision: the criterion leaves how this copy travels to the target's guidance, and the
/// guidance of the target `rust` says nothing about it. This compile takes the only
/// spelling that keeps `Package.root` a directory whose files are loaded as any other
/// package's, and `Package.root` absolute when the library is found outside the project:
/// the path of the library the crate was built beside, baked in at build time.
pub(super) const BUILT_IN_LIBRARY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../lib"); // @lfy def/workspace/main.lfy:load
/// What a knowledge source never begins with, because knowledge is vendored.
const NETWORK: [&str; 2] = ["http://", "https://"]; // @lfy def/workspace/main.lfy:load

/// A project loaded from disk and bound.
///
/// Reads the configuration, discovers every file, orders the files by what they use,
/// then binds them once. `Workspace::root` is `root`. Loading never stops for a failure:
/// every failure is a `LoadProblem` or a `Problem`, and the rest of the project still
/// loads.
// @lfy def/workspace/main.lfy:load
pub fn load(root: &Path) -> Workspace {
    // @lfy def/workspace/main.lfy:load
    load_in(root, BTreeMap::new(), None)
}

/// [`load`] with the files in `overlays` read as the text given there instead of what is
/// on disk, and `built_in` standing for the copy of the library the compiler was built
/// with when it is given.
// @lfy def/workspace/main.lfy:change
// @lfy def/workspace/main.lfy:load
pub(super) fn load_in(
    root: &Path,
    overlays: BTreeMap<String, String>,
    built_in: Option<String>,
) -> Workspace {
    let mut loader = Loader {
        root,
        overlays: &overlays,
        problems: Vec::new(),
        use_problems: Vec::new(),
    };

    // Configuration. @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load#load:load:d2500ef79b0f845d62d879e55d20851d45ebfa885fa1e7397a17ab04f6f7422d
    let configuration = loader.configuration(".");
    // Layout. @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load#load:load:bc81bdfb69e2efbe2ffcca801028530c7cbc5a41ff47a15eed0784e947b566cd
    // @lfy def/workspace/main.lfy:load#load:load:3fcd57f4ebc2aa8bc9b9f2c2fb00087ebaa95f76f3c93971df4f0a7c72e2cd09
    let name = match &configuration.name {
        Some(name) => name.clone(),
        None => directory_name(root),
    };
    // @lfy def/workspace/main.lfy:load#load:load:a473c608f5a70cc064c8635bd090121d715b920b42ee0fcbe41e3c3a75fdd9d8
    let source_directories = match &configuration.source {
        Some(ConfigurationSource::One(directory)) => vec![directory.clone()],
        // @lfy def/workspace/main.lfy:load#load:load:83585aea4fc7bf0a367ab795152302d1d333212f48b23d1c5285dd457b336215
        Some(ConfigurationSource::Several(directories)) => {
            let mut once = Vec::new();
            for directory in directories {
                if !once.contains(directory) {
                    once.push(directory.clone());
                }
            }
            once
        }
        // @lfy def/workspace/main.lfy:load#load:load:8d7ba6d4fe1f04154ef1330dfbe2a0ed619e5674a06aef6b93cf3af417afa0f1
        None => vec![DEFAULT_SOURCE_DIRECTORY.to_string()],
    };

    // Dependencies. @lfy def/workspace/main.lfy:load
    let packages = loader.packages(&configuration, built_in);
    let library = packages.library;

    // Discovery, use, and order. @lfy def/workspace/main.lfy:load
    let mut sources = Vec::new();
    let mut files = Vec::new();
    if !loader.directory(".") {
        // @lfy def/workspace/main.lfy:load#load:load:f97a7916ef1a5ec5f8b25e9caca0c7851b745165411bc7008519899125882bac
        loader.problem(None, format!("the root {} does not exist", root.display()));
    } else {
        let mut listed = Vec::new();
        // @lfy def/workspace/main.lfy:load#load:load:0fc20a5c6ed1f12fda66543903de28702cb27892cc578d83c6316902500fef99
        for directory in &source_directories {
            let directory = trim_directory(directory);
            if loader.directory(&directory) {
                listed.push(directory);
            } else {
                loader.problem(
                    Some(&directory),
                    format!(
                        "the source directory {directory} does not exist under {}",
                        root.display()
                    ),
                );
            }
        }
        let entries = loader.discover(&listed, &packages);
        let (order, cycles) = order_by_uses(&entries);
        for (file, index) in cycles {
            let path = entries[file].path.clone();
            let spelled = entries[file].uses[index].spelled.clone();
            // @lfy def/workspace/main.lfy:load#load:load:b1fbb044be3c701778d0f3eed4d4ad278cdf34c2b917c9b0a33327367959d921
            loader.use_problem(&path, index, spelled.as_deref(), "is part of a cycle");
        }
        let order: Vec<usize> = order
            .into_iter()
            .filter(|&index| entries[index].tree.is_some())
            .collect();
        let named: Vec<(&str, Option<usize>)> = order
            .iter()
            .map(|&index| (entries[index].path.as_str(), entries[index].package))
            .collect();
        let names = file_names(&named, &packages.list, &listed);
        for (index, name) in order.into_iter().zip(names) {
            let entry = &entries[index];
            let Some(tree) = &entry.tree else { continue };
            files.push(File {
                // @lfy def/workspace/main.lfy:load#load:load:23324d830b23ca1be67dea3c9234abb1945519a3e56b47c394222debdf328fe8
                path: entry.path.clone(),
                // @lfy def/workspace/main.lfy:load#load:load:f0c460549fc5debac126dc93031902458c9a539417d42b3b143b29ce41672943
                package: entry.package,
                name,
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
                origin: origin_of(
                    &entry.path,
                    entry.package,
                    library,
                    &packages.list[library].root,
                ),
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
    added.append(&mut knowledge_problems(
        &model,
        &loader,
        &files,
        &packages.list,
    ));
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
        configuration,
        name,
        source_directories,
        files,
        model,
        packages: packages.list,
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

/// The packages of the program and the names each file can reach them by.
// @lfy def/workspace/main.lfy:load
struct Packages {
    /// Every package, the package `elfie` first, then each in the order its dependency
    /// entry was read.
    list: Vec<Package>,
    /// The package `elfie` in [`Packages::list`].
    library: usize,
    /// The dependency entries a file of the project can name, by key.
    project: BTreeMap<String, usize>,
    /// The dependency entries a file of each package can name, by key, parallel to
    /// [`Packages::list`].
    names: Vec<BTreeMap<String, usize>>,
}

impl Packages {
    /// The dependency entries a file of a package, or of the project, can name.
    // @lfy def/workspace/main.lfy:load
    fn nameable(&self, package: Option<usize>) -> &BTreeMap<String, usize> {
        match package {
            Some(package) => &self.names[package],
            None => &self.project,
        }
    }

    /// The package a key names from a file of a package, or of the project; `elfie`
    /// always names the one package `elfie`.
    // @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load#load:load:56743e9f562e649dc3d6042b20e25fdf4744ba98e77150ff1c635cead47157cb
    // @lfy def/workspace/main.lfy:load#load:load:a3421e175ba0d8b9bce4fec7e2b2217ba83e5c91e3304b1f4fc241e3a1c7b66a
    fn named(&self, package: Option<usize>, key: &str) -> Option<usize> {
        if key == LIBRARY_PACKAGE {
            return Some(self.library);
        }
        self.nameable(package).get(key).copied()
    }
}

/// What the value of a dependency entry is.
// @lfy def/workspace/main.lfy:load
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DependencyKind {
    Path,
    Git,
    Version,
}

/// What the value of a dependency entry is: a path, a git URL, or a version.
// @lfy def/workspace/main.lfy:load
// @lfy def/workspace/main.lfy:load#load:load:a354cbeef946748f4822d5b7ccc565b0fb084d8ebb9b52938028687e8993b37f
// @lfy def/workspace/main.lfy:load#load:load:c78f44cea8c4c4a346c72577c2baf833dcae5ef0dfac93f3a0cabee3c6d487fc
// @lfy def/workspace/main.lfy:load#load:load:5f36f4fd70eb392d120e8c33f16e39c5ec39653ef25fed8c872656b5cc3fbeea
pub(super) fn dependency_kind(value: &str) -> DependencyKind {
    if value.starts_with("./") || value.starts_with("../") || Path::new(value).is_absolute() {
        DependencyKind::Path
    } else if value.starts_with("git@")
        || value.starts_with("git+")
        || value.contains("://")
        || value.ends_with(".git")
    {
        DependencyKind::Git
    } else {
        DependencyKind::Version
    }
}

/// The root of the package a dependency entry that is a path gives, from the directory
/// holding its `elfie.json`: the path itself when it is absolute.
// @lfy def/workspace/main.lfy:load
fn dependency_root(base: &str, value: &str) -> String {
    if Path::new(value).is_absolute() {
        value.to_string()
    } else {
        normalize(&join(base, value))
    }
}

/// The segments of a path as a `Use` or an export key spells it, without empty and `.`
/// segments.
fn segments_of(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect()
}

/// The value of the longest key of `Configuration.exports` whose segments begin `rest`,
/// and the segments of `rest` after that key.
// @lfy def/workspace/main.lfy:load
fn export_of<'c, 'r>(
    configuration: &'c Configuration,
    rest: &'r [&'r str],
) -> Option<(&'c str, &'r [&'r str])> {
    configuration
        .exports
        .as_ref()?
        .iter()
        .filter_map(|(key, value)| {
            let key = segments_of(key);
            (rest.len() >= key.len() && rest[..key.len()] == key[..])
                .then_some((key.len(), value.as_str()))
        })
        .max_by_key(|&(length, _)| length)
        .map(|(length, value)| (value, &rest[length..]))
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

    /// The `elfie.json` in a directory, read as JSON, each key of [`Configuration`] taken
    /// from the key of the same name.
    // @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load#load:load:9955f71e445330d81b0395728fbe576a039b18e3bb889f7d498edbbcd4599dbf
    fn configuration(&mut self, directory: &str) -> Configuration {
        let path = join(directory, MANIFEST);
        // @lfy def/workspace/main.lfy:load#load:load:a450cc5b0bb372bd3f2b9fea6f4218845a10c800e1f81ad8274d5cfb882516f4
        if !self.exists(&path) {
            return Configuration::default();
        }
        let object = match self.read(&path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(serde_json::Value::Object(object)) => object,
                Ok(_) => {
                    // @lfy def/workspace/main.lfy:load#load:load:a4feb706de87193af1f3285b8bd67c396deb25f48611db7160a8930ed6894335
                    self.problem(Some(&path), "is not a JSON object".to_string());
                    return Configuration::default();
                }
                Err(error) => {
                    self.problem(Some(&path), format!("is not valid JSON: {error}"));
                    return Configuration::default();
                }
            },
            Err(error) => {
                self.problem(Some(&path), format!("cannot be read: {error}"));
                return Configuration::default();
            }
        };
        let string = |value: &serde_json::Value| value.as_str().map(str::to_string);
        let strings = |value: &serde_json::Value| -> Option<BTreeMap<String, String>> {
            value
                .as_object()?
                .iter()
                .map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect()
        };
        let source = |value: &serde_json::Value| match value {
            serde_json::Value::String(one) => Some(ConfigurationSource::One(one.clone())),
            serde_json::Value::Array(several) => several
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .map(ConfigurationSource::Several),
            _ => None,
        };
        Configuration {
            name: self.key(&path, &object, "name", "a string", string),
            version: self.key(&path, &object, "version", "a string", string),
            source: self.key(
                &path,
                &object,
                "source",
                "a string or a list of strings",
                source,
            ),
            dependencies: self.key(
                &path,
                &object,
                "dependencies",
                "an object of strings",
                strings,
            ),
            exports: self.key(&path, &object, "exports", "an object of strings", strings),
        }
    }

    /// The value of one key of an `elfie.json` as the type [`Configuration`] declares for
    /// it; `None` when it is absent, and `None` with the key and its type named for that
    /// `elfie.json` when it is of another type.
    // @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load#load:load:7bf7f7320ea74ff2dd0d08e518d364c8292228a597b9360c3ab0cf7a4178d8b7
    // @lfy def/workspace/main.lfy:load#load:load:4fa9b901c6befec4fa20bfa6d19b5371ef837153bfacb6b980b1b0cb23139e3e
    fn key<T>(
        &mut self,
        path: &str,
        object: &serde_json::Map<String, serde_json::Value>,
        key: &str,
        wanted: &str,
        read: impl Fn(&serde_json::Value) -> Option<T>,
    ) -> Option<T> {
        let value = object.get(key)?;
        let read = read(value);
        if read.is_none() {
            self.problem(Some(path), format!("{key:?} should be {wanted}"));
        }
        read
    }

    /// Every package of the program: the package `elfie`, then each package a dependency
    /// entry that is a path gives, of the project's configuration and then of each
    /// package's in turn.
    // @lfy def/workspace/main.lfy:load
    fn packages(&mut self, configuration: &Configuration, built_in: Option<String>) -> Packages {
        let entries = configuration.dependencies.clone().unwrap_or_default();
        // @lfy def/workspace/main.lfy:load#load:load:edc92a499ae2b8769762be0cc27e701b8a746a8ac3afd23f6816df3f92b21d46
        // @lfy def/workspace/main.lfy:load#load:load:b32dfca900b488d4e38fbc1eacf76d5247749ba7f635f7daba72260d40877a8d
        let library_root = match entries.get(LIBRARY_PACKAGE) {
            Some(value) if dependency_kind(value) == DependencyKind::Path => {
                dependency_root(".", value)
            }
            _ => built_in.unwrap_or_else(|| normalize(BUILT_IN_LIBRARY)),
        };
        let mut packages = Packages {
            list: Vec::new(),
            library: 0,
            project: BTreeMap::new(),
            names: Vec::new(),
        };
        packages.library = self.package(&mut packages, LIBRARY_PACKAGE, library_root);
        self.dependencies(&mut packages, None, &entries);
        let mut next = 0;
        while next < packages.list.len() {
            let entries = packages.list[next]
                .configuration
                .dependencies
                .clone()
                .unwrap_or_default();
            self.dependencies(&mut packages, Some(next), &entries);
            self.exports(&packages.list[next]);
            next += 1;
        }
        packages
    }

    /// The packages the dependency entries of one `elfie.json` give, and the names a file
    /// of its project or package reaches them by.
    // @lfy def/workspace/main.lfy:load
    fn dependencies(
        &mut self,
        packages: &mut Packages,
        owner: Option<usize>,
        entries: &BTreeMap<String, String>,
    ) {
        let base = owner.map_or_else(|| ".".to_string(), |owner| packages.list[owner].root.clone());
        let manifest = join(&base, MANIFEST);
        // Decision: entries are read in the order `serde_json` keeps an object's keys,
        // which is sorted by key.
        for (key, value) in entries {
            let kind = dependency_kind(value);
            // @lfy def/workspace/main.lfy:load#load:load:488bdb4d999605f20285df1be26700cce5e26fe51808ce72c77e2e0e74231548
            if key == LIBRARY_PACKAGE && (owner.is_some() || kind == DependencyKind::Path) {
                continue;
            }
            // @lfy def/workspace/main.lfy:load#load:load:20cc44ce9010c1d6d8f51447978d2c1bde789da9cd1d0f4bb04869b752e23c86
            if kind != DependencyKind::Path {
                let what = if kind == DependencyKind::Git {
                    "a git URL"
                } else {
                    "a version"
                };
                self.problem(
                    Some(&manifest),
                    format!(
                        "the dependency {key:?} is {what}, and version and git dependencies \
                         cannot be loaded yet"
                    ),
                );
                continue;
            }
            // @lfy def/workspace/main.lfy:load#load:load:48288ac53c4ce16476eae28d86777ffa3592a5192db841bcde3e89dd31724f71
            // @lfy def/workspace/main.lfy:load#load:load:6b1ef1b2e8a6e582f3257d3e95d4b94c630efd29f5ec5856edbd629ab7a6ee53
            let root = dependency_root(&base, value);
            let package = self.package(packages, key, root);
            match owner {
                Some(owner) => packages.names[owner].insert(key.clone(), package),
                None => packages.project.insert(key.clone(), package),
            };
        }
    }

    /// The one package at a root, added when no dependency entry has given it before.
    // @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load#load:load:d5d5894b6e1a0b68f69a3b71f975fed7a26b6466fdb05517e2dcb72a5888d1a5
    fn package(&mut self, packages: &mut Packages, identifier: &str, root: String) -> usize {
        if let Some(package) = packages.list.iter().position(|package| package.root == root) {
            return package;
        }
        // @lfy def/workspace/main.lfy:load#load:load:df79827338f394294ebc29291a335b68f3f6948d1a49732d54593d2f3df93774
        let configuration = if self.directory(&root) {
            self.configuration(&root)
        } else {
            self.problem(
                Some(&root),
                format!("the root of the package {identifier} does not exist"),
            );
            Configuration::default()
        };
        packages.list.push(Package {
            identifier: identifier.to_string(),
            root,
            configuration,
        });
        packages.names.push(BTreeMap::new());
        packages.list.len() - 1
    }

    /// A problem for every export of a package that names nothing under its root.
    // @lfy def/workspace/main.lfy:load
    // @lfy def/workspace/main.lfy:load#load:load:19b062c31b01faa188b4bbc0532bdbf5c39c0e25ae249b283d34e4476fc8821d
    fn exports(&mut self, package: &Package) {
        let Some(exports) = &package.configuration.exports else {
            return;
        };
        let manifest = join(&package.root, MANIFEST);
        for (key, value) in exports {
            let target = normalize(&join(&package.root, value));
            if !self.anything_at(&target) && !self.directory(&target) {
                self.problem(
                    Some(&manifest),
                    format!(
                        "the export {key:?} names {value}, which is neither a file nor a \
                         directory under {}",
                        package.root
                    ),
                );
            }
        }
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

    /// Every file of the program: the files of each source directory, every package's
    /// files, and every file a `Use` refers to, each parsed with its uses resolved, in
    /// discovery order.
    // @lfy def/workspace/main.lfy:load#load:load:a7bb1ab332ef59bd7ef0b193ca2257a60b8457f57538d11c2fd2c2f0c2610ea5
    fn discover(&mut self, source_directories: &[String], packages: &Packages) -> Vec<Entry> {
        let mut entries: Vec<Entry> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut pending: Vec<(String, Option<usize>)> = Vec::new();

        // @lfy def/workspace/main.lfy:load#load:load:34b900cfd918601b8e0fbd06775f5015712e20b8edddc83c91a156a80e1e26ff
        for directory in source_directories {
            for path in self.source_files(directory) {
                enqueue(&mut seen, &mut pending, path, None); // @lfy def/workspace/main.lfy:load
            }
        }
        for (id, package) in packages.list.iter().enumerate() {
            // A missing package root is already reported by `packages`; no file of it is
            // in the program. @lfy def/workspace/main.lfy:load
            if !self.directory(&package.root) {
                continue;
            }
            // A file under the roots of two packages, one inside the other, carries the
            // package whose root is nearer to it.
            for path in self.source_files(&package.root) {
                let package = package_of(&path, &packages.list).or(Some(id));
                enqueue(&mut seen, &mut pending, path, package); // @lfy def/workspace/main.lfy:load
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
                    package_of(used, &packages.list),
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
        packages: &Packages,
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
                Some(spelled) => match self.resolve(path, package, spelled, packages) {
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

    /// The file a use's path refers to, from the file at `from` of the package `package`,
    /// or of the project when it is `None`.
    // @lfy def/workspace/main.lfy:load
    fn resolve(
        &self,
        from: &str,
        package: Option<usize>,
        spelled: &str,
        packages: &Packages,
    ) -> Result<String, String> {
        let base = if spelled.starts_with('.') {
            // @lfy def/workspace/main.lfy:load#load:load:7cd3d4046cd7170c71b67d6747111258f280a6b304e31b968c1adf210b007213
            join(directory_of(from), spelled)
        } else {
            // @lfy def/workspace/main.lfy:load#load:load:99f8a9365b4ddf9b26525fc56b9850112f86430a002df60a6c1453a745a29f40
            let segments: Vec<&str> = spelled.split('/').collect();
            let (count, named) = (1..=segments.len())
                .rev()
                .find_map(|count| {
                    packages
                        .named(package, &segments[..count].join("/"))
                        .map(|named| (count, named))
                })
                .ok_or_else(|| format!("no package is named {:?}", segments[0]))?;
            let named = &packages.list[named];
            let rest = &segments[count..];
            // @lfy def/workspace/main.lfy:load#load:load:1656e6daf35e6938e3d1f828fb76d318f57cf16a19a3f42543ba0054030bea5d
            match export_of(&named.configuration, rest) {
                Some((value, after)) => join(&join(&named.root, value), &after.join("/")),
                None => join(&named.root, &rest.join("/")),
            }
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

/// A path without its `.lfy` extension.
fn without_extension(path: &str) -> &str {
    path.strip_suffix(".lfy").unwrap_or(path)
}

/// A path relative to a directory it is under, both relative to the root.
fn relative_to<'p>(path: &'p str, directory: &str) -> &'p str {
    if is_root(directory) {
        return path;
    }
    path.strip_prefix(directory)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
}

/// The name each file of the program goes by, in the order the files are given.
///
/// Decision: a file of the project under none of the source directories, which only a
/// `Use` brings in, is named by its path relative to the root.
// @lfy def/workspace/main.lfy:load
fn file_names(
    files: &[(&str, Option<usize>)],
    packages: &[Package],
    source_directories: &[String],
) -> Vec<String> {
    let mut names = vec![String::new(); files.len()];
    // The path of each file of the project relative to its source directory.
    let mut within: Vec<(usize, &str)> = Vec::new();
    for (index, &(path, package)) in files.iter().enumerate() {
        match package {
            // @lfy def/workspace/main.lfy:load#load:load:f3f04a799f9fa89db29eed53cf6239c78d5d4389948995d43fe22b84eba0cbdc
            Some(package) => {
                let package = &packages[package];
                names[index] = format!(
                    "{}/{}",
                    package.identifier,
                    without_extension(relative_to(path, &package.root))
                );
            }
            None => {
                // @lfy def/workspace/main.lfy:load#load:load:4e242171bbac221a9235f0dc2f2b41c8ce9461c40c25cecf4661dd81cdd32704
                let directory = source_directories
                    .iter()
                    .filter(|directory| under(path, directory))
                    .max_by_key(|directory| directory.len())
                    .map_or(".", String::as_str);
                within.push((index, without_extension(relative_to(path, directory))));
            }
        }
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for &(_, relative) in &within {
        *counts.entry(relative).or_default() += 1;
    }
    let mut shared = Vec::new();
    for &(index, relative) in &within {
        if counts[relative] == 1 {
            names[index] = relative.to_string();
        } else {
            shared.push(index);
        }
    }
    let segments: Vec<Vec<&str>> = files
        .iter()
        .map(|&(path, _)| without_extension(path).split('/').collect())
        .collect();
    // @lfy def/workspace/main.lfy:load#load:load:95ee4c371559a37196b70b48eb49a98edb20b303a2fad0cdde57142a357b6b38
    for &index in &shared {
        let own = &segments[index];
        let mut name = own.join("/");
        for count in 1..=own.len() {
            let end = &own[own.len() - count..];
            let candidate = end.join("/");
            let ends_another = shared
                .iter()
                .any(|&other| other != index && segments[other].ends_with(end));
            let named = names
                .iter()
                .enumerate()
                .any(|(other, taken)| other != index && *taken == candidate);
            if !ends_another && !named {
                name = candidate;
                break;
            }
        }
        names[index] = name;
    }
    names
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
