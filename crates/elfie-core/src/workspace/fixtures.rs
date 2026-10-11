// @lfy def/workspace/main.lfy:load
//! The projects the tests of this unit are read against, and what those tests read of a
//! loaded workspace.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::load::{MAIN_FILE, load_in};
use super::*;
use crate::grammar::rules::statement::Statement;
use crate::model::{EntityId, Problem};

/// The directory of the library a fixture carries, relative to its root.
pub(super) const LIBRARY_ROOT: &str = "lib";
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
pub(super) const ROLES: &str = "\
trait lang extends targetLanguage {
  .fileExtension = \"rs\";
  .markerComment = \"//\";
}
trait flat extends targetLayout { }
";

/// A project directory under the system's temporary directory, removed when dropped.
pub(super) struct Fixture {
    pub(super) root: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Fixture {
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
    pub(super) fn library(&self, root: &str) -> &Fixture {
        self.write(&join(root, MAIN_FILE), LIBRARY)
            .write(&join(root, "prelude/builtin.lfy"), LIBRARY_BUILTIN)
            .write(&join(root, "prelude/entity.lfy"), LIBRARY_ENTITY)
            .write(&join(root, "prelude/trait.lfy"), LIBRARY_TRAIT)
            .write(&join(root, "criteria/main.lfy"), LIBRARY_CRITERIA)
            .write(&join(root, "target/main.lfy"), LIBRARY_TARGET)
    }

    /// A project with an empty `def` directory.
    pub(super) fn empty() -> Fixture {
        let fixture = Fixture::new();
        fixture.dir("def");
        fixture
    }

    fn dir(&self, path: &str) -> &Fixture {
        fs::create_dir_all(self.root.join(path)).unwrap();
        self
    }

    pub(super) fn write(&self, path: &str, text: &str) -> &Fixture {
        self.write_bytes(path, text.as_bytes())
    }

    pub(super) fn write_bytes(&self, path: &str, bytes: &[u8]) -> &Fixture {
        let disk = self.root.join(path);
        fs::create_dir_all(disk.parent().unwrap()).unwrap();
        fs::write(disk, bytes).unwrap();
        self
    }

    /// The project, loaded with the library the fixture carries standing in for the
    /// one the environment names.
    // @lfy def/workspace/main.lfy:load
    pub(super) fn load(&self) -> Workspace {
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
pub(super) fn load_built_in(root: &Path) -> Workspace {
    load_in(root, BTreeMap::new(), None)
}

/// Whether a file came from the package `elfie`.
// @lfy def/workspace/main.lfy:load
fn is_library(workspace: &Workspace, file: &File) -> bool {
    file.package
        .is_some_and(|package| workspace.packages[package].identifier == LIBRARY_PACKAGE)
}

/// Every file of the program that the standard library did not bring, in bind order.
pub(super) fn project(workspace: &Workspace) -> impl Iterator<Item = &File> {
    workspace
        .files
        .iter()
        .filter(|file| !is_library(workspace, file))
}

pub(super) fn paths(workspace: &Workspace) -> Vec<&str> {
    project(workspace).map(|file| file.path.as_str()).collect()
}

/// Every file of the package `elfie`, in bind order.
// @lfy def/workspace/main.lfy:load
pub(super) fn library_paths(workspace: &Workspace) -> Vec<&str> {
    workspace
        .files
        .iter()
        .filter(|file| is_library(workspace, file))
        .map(|file| file.path.as_str())
        .collect()
}

/// The package `elfie` of a workspace.
// @lfy def/workspace/main.lfy:load
pub(super) fn library(workspace: &Workspace) -> &Package {
    workspace
        .packages
        .iter()
        .find(|package| package.identifier == LIBRARY_PACKAGE)
        .expect("the package elfie is in the program")
}

pub(super) fn load_problems(workspace: &Workspace) -> Vec<&LoadProblem> {
    workspace.load_problems().collect()
}

pub(super) fn uses_of<'w>(workspace: &'w Workspace, path: &str) -> &'w [Option<String>] {
    let file = workspace
        .file(path)
        .unwrap_or_else(|| panic!("{path} is not in the program"));
    workspace.uses(file)
}

/// The identifiers of a list of entities, in order.
pub(super) fn names<'w>(workspace: &'w Workspace, entities: &[EntityId]) -> Vec<&'w str> {
    entities
        .iter()
        .filter_map(|&entity| workspace.model.entities[entity].identifier.as_deref())
        .collect()
}

pub(super) fn bind_problems(workspace: &Workspace) -> Vec<&Problem> {
    workspace.bind_problems().collect()
}

/// The one target of a workspace.
// @lfy def/workspace/main.lfy:load
pub(super) fn one_target(workspace: &Workspace) -> &Target {
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
pub(super) fn at_declaration<'w>(workspace: &'w Workspace, problem: &Problem) -> &'w str {
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
pub(super) fn at_use<'w>(workspace: &'w Workspace, problem: &Problem) -> (&'w str, bool) {
    let source = &workspace.model.sources[problem.node.file];
    (
        source.path.as_str(),
        workspace.model.node(problem.node).is(Statement::Use),
    )
}

/// The text a file of the program was read as, from the tokens it was lexed into.
pub(super) fn text_of(workspace: &Workspace, path: &str) -> String {
    let file = workspace
        .file(path)
        .unwrap_or_else(|| panic!("{path} is not in the program"));
    let tree = workspace.tree(file);
    tree.raw(0, tree.tokens.len())
}

pub(super) fn position(workspace: &Workspace, path: &str) -> usize {
    workspace
        .files
        .iter()
        .position(|file| file.path == path)
        .unwrap_or_else(|| panic!("{path} is not in the program"))
}
