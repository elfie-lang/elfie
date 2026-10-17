// @lfy def/interpret/main.lfy:phaseOf
//! The programs the tests of this unit are read against.

use std::path::PathBuf;

use super::lower::within;
use super::*;
use crate::model::{Source, bind};
use crate::workspace::File as Loaded;

/// One source of a program, parsed.
pub(super) fn source(path: &str, text: &str, uses: &[Option<&str>], origin: Origin) -> Source {
    let tokens = crate::lexer::lex(text, Some(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    let tree = crate::parser::parse(tokens, None);
    assert!(
        tree.errors.is_empty(),
        "{path} does not parse: {}",
        tree.render()
    );
    Source {
        path: path.to_string(),
        tree,
        uses: uses.iter().map(|u| u.map(str::to_string)).collect(),
        origin,
    }
}

/// One file, `a.lfy`, bound alone.
pub(super) fn bound(text: &str) -> Model {
    bind(vec![source("a.lfy", text, &[], Origin::Program)])
}

/// A prelude small enough to read: the kind data every entity is seen through, and the
/// base data a string is read through.
pub(super) fn prelude() -> Vec<Source> {
    let entity = source(
        "lib/prelude/entity.lfy",
        "d Entity { $identifier: `The declared name` = string | undefined; \
         $definition: `Its description` = string | undefined; \
         $type: `Its type` = Entity | undefined; \
         $acceptanceCriteria: `Its criteria` = Entity[]; \
         $like: `A value the prompt describes` = (prompt: string) => Entity; \
         $test: `Adds cases` = (...cases: Entity[]) => Entity; }",
        &[],
        Origin::Library,
    );
    let values = source(
        "lib/values/base.lfy",
        "d String { $length: `How many characters` = number; }",
        &[],
        Origin::Library,
    );
    let main = source(
        "lib/main.lfy",
        "use \"./prelude/entity\"; use \"./values/base\";",
        &[
            Some("lib/prelude/entity.lfy"),
            Some("lib/values/base.lfy"),
        ],
        Origin::Prelude,
    );
    vec![entity, values, main]
}

/// A prelude of the builtins the interpreter performs natively: the trait one carries,
/// and the fns of `elfie/system` a test calls.
pub(super) fn natives() -> Vec<Source> {
    let builtin = source(
        "lib/prelude/builtin.lfy",
        "trait builtin: `Performed by the compiler` {}",
        &[],
        Origin::Library,
    );
    let json = source(
        "lib/system/json.lfy",
        "use \"../prelude/builtin\";\n\
         fn parse(text: string) is builtin: `What JSON text carries` => object | undefined;\n\
         fn stringify(value: object, pretty: boolean = false) is builtin: \
         `JSON text carrying a value` => string;\n",
        &[Some("lib/prelude/builtin.lfy")],
        Origin::Library,
    );
    let file = source(
        "lib/system/file.lfy",
        "use \"../prelude/builtin\";\n\
         fn read(path: string) is builtin: `The text of a file` => string | undefined;\n\
         fn write(path: string, text: string) is builtin: `Whether text was written` \
         => boolean;\n\
         fn exists(path: string) is builtin: `Whether something is at a path` => boolean;\n",
        &[Some("lib/prelude/builtin.lfy")],
        Origin::Library,
    );
    let process = source(
        "lib/system/process.lfy",
        "use \"../prelude/builtin\";\n\
         fn run(command: string) is builtin: `Runs a command and waits for it` => object;\n\
         fn environment(name: string) is builtin: `The value of a variable` \
         => string | undefined;\n",
        &[Some("lib/prelude/builtin.lfy")],
        Origin::Library,
    );
    let main = source(
        "lib/main.lfy",
        "use \"./prelude/builtin\";\nuse \"./system/json\";\n\
         use \"./system/file\";\nuse \"./system/process\";\n",
        &[
            Some("lib/prelude/builtin.lfy"),
            Some("lib/system/json.lfy"),
            Some("lib/system/file.lfy"),
            Some("lib/system/process.lfy"),
        ],
        Origin::Prelude,
    );
    vec![builtin, json, file, process, main]
}

/// A prelude of what declaring a target needs: the trait a builtin carries, the `layer`
/// fn of `elfie/prelude/trait`, the `Target` data a const holds with the function that
/// gives a record its type, the roles its slots take, and the enums `knowledge` and
/// `commands` name.
pub(super) fn targets() -> Vec<Source> {
    let builtin = source(
        "lib/prelude/builtin.lfy",
        "trait builtin: `Performed by the compiler` {}",
        &[],
        Origin::Library,
    );
    let traits = source(
        "lib/prelude/trait.lfy",
        "use \"./builtin\";\n\
         fn layer(subject: trait, ...arguments: string[]) is builtin: \
         `A trait chosen as a layer of a target` => object;\n",
        &[Some("lib/prelude/builtin.lfy")],
        Origin::Library,
    );
    let criteria = source(
        "lib/criteria/main.lfy",
        "enum KnowledgeKind: `What a piece of knowledge is` {\n\
           reference = `documentation to read: a language reference, an API, a guide`,\n\
           example = `working code to imitate`,\n\
         }\n\
         enum Operation: `What a command is for` {\n\
           build = `build: compile or type-check the outputs`,\n\
           test = `test: run the tests`,\n\
         }\n",
        &[],
        Origin::Library,
    );
    let target = source(
        "lib/target/main.lfy",
        "use \"../prelude/builtin\";\n\
         trait target: `Anything else the compiler is told` {\n\
           $provides: `Capabilities this layer gives its target` = string[];\n\
           $style: `Something one layer of a target sets` = string;\n\
         }\n\
         trait targetLanguage: `What code is written in` {}\n\
         trait targetLayout: `Where files go` {}\n\
         d Target is builtin: `One artifact the project is built into` {\n\
           $output: `Where everything it writes goes` = string;\n\
           $language: `What its code is written in` = trait;\n\
           $layout: `Where its files go` = trait;\n\
           $layers: `Anything else the compiler is told` = trait[];\n\
         }\n\
         function targetOf(target: Target): `Gives a record its type` -> Target {\n\
           return target;\n\
         }\n",
        &[Some("lib/prelude/builtin.lfy")],
        Origin::Library,
    );
    let main = source(
        "lib/main.lfy",
        "use \"./prelude/builtin\";\nuse \"./prelude/trait\";\n\
         use \"./criteria/main\";\nuse \"./target/main\";\n",
        &[
            Some("lib/prelude/builtin.lfy"),
            Some("lib/prelude/trait.lfy"),
            Some("lib/criteria/main.lfy"),
            Some("lib/target/main.lfy"),
        ],
        Origin::Prelude,
    );
    vec![builtin, traits, criteria, target, main]
}

/// That prelude, then one file `a.lfy` of the program, as the expand pass leaves it.
pub(super) fn expanded_with_targets(text: &str) -> Model {
    let mut sources = targets();
    sources.push(source("a.lfy", text, &[], Origin::Program));
    expand(declared(sources))
}

/// The prelude, then one file `a.lfy` of the program.
pub(super) fn bound_with_prelude(text: &str) -> Model {
    let mut sources = prelude();
    sources.push(source("a.lfy", text, &[], Origin::Program));
    bind(sources)
}

/// A model as the declare pass leaves it: everything the expand pass adds, taken back
/// out, so that [`expand`] is handed what the binder hands it.
pub(super) fn declared(sources: Vec<Source>) -> Model {
    let mut model = bind(sources);
    for entity in &mut model.entities {
        entity.traits.clear();
        entity.acceptance_criteria.clear();
        entity.values.clear();
        entity.tests.clear();
        entity.knowledge.clear();
        entity.commands.clear();
        entity.targets.clear();
        if let EntityKind::Trait {
            entities,
            extenders,
            ..
        } = &mut entity.kind
        {
            entities.clear();
            extenders.clear();
        }
    }
    // A member an applied trait copied into a receiver's scope is declared somewhere
    // else; the receiver's own members are declared inside the node that owns the scope.
    let scopes = model.scopes.len();
    for scope in 0..scopes {
        let owner = model.scopes[scope].owner;
        if owner.file == usize::MAX {
            continue;
        }
        let kept: Vec<SymbolId> = model.scopes[scope]
            .symbols
            .iter()
            .copied()
            .filter(|&symbol| {
                let record = &model.symbols[symbol];
                if !matches!(record.kind, SymbolKind::Member | SymbolKind::EnumMember)
                    || record.node.file == usize::MAX
                {
                    return true;
                }
                within(&model, record.node, owner)
            })
            .collect();
        model.scopes[scope].symbols = kept;
    }
    model.problems.clear();
    model
}

/// A workspace of these sources, with no package and no target, holding the model as
/// [`expand`] leaves it: what the binder hands [`lower`].
pub(super) fn workspace_of(sources: Vec<Source>) -> Workspace {
    let files: Vec<Loaded> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| Loaded {
            path: source.path.clone(),
            package: None,
            // The name a file under the source directory goes by: its path under it,
            // without the extension.
            name: source
                .path
                .strip_prefix("def/")
                .unwrap_or(&source.path)
                .trim_end_matches(".lfy")
                .to_string(),
            source: index,
        })
        .collect();
    let model = expand(declared(sources));
    Workspace {
        root: PathBuf::from("."),
        configuration: Default::default(),
        name: "fixture".to_string(),
        source_directories: vec!["def".to_string()],
        files,
        model,
        packages: Vec::new(),
        targets: Vec::new(),
        problems: Vec::new(),
        overlays: BTreeMap::new(),
    }
}

/// One file `a.lfy` as a workspace.
pub(super) fn workspace(text: &str) -> Workspace {
    workspace_of(vec![source("a.lfy", text, &[], Origin::Program)])
}

/// The first node of a file satisfying a rule.
pub(super) fn first<R: GrammarRule>(model: &Model, file: FileId, rule: R) -> NodeRef {
    model
        .descendants(NodeRef { file, index: 0 })
        .into_iter()
        .find(|&node| model.is(node, rule))
        .unwrap_or_else(|| panic!("no {}", rule.identifier()))
}

/// The node after the given one satisfying a rule: the second `Name`, and so on.
pub(super) fn nth<R: GrammarRule>(model: &Model, file: FileId, rule: R, at: usize) -> NodeRef {
    let mut found = model
        .descendants(NodeRef { file, index: 0 })
        .into_iter()
        .filter(|&node| model.is(node, rule));
    found
        .nth(at)
        .unwrap_or_else(|| panic!("no {} at {at}", rule.identifier()))
}

pub(super) fn entity_named(model: &Model, name: &str) -> EntityId {
    model
        .entities
        .iter()
        .position(|entity| entity.identifier.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("{name} is declared"))
}

/// The value expression of a declaration: what follows its `Declared`.
pub(super) fn value_of(model: &Model, declaration: NodeRef) -> NodeRef {
    model
        .child_nodes(declaration)
        .into_iter()
        .find(|&child| !model.is(child, E::Declared))
        .expect("a declared value")
}

/// Every node of a lowered tree, the root first.
pub(super) fn every(node: &LoweredNode) -> Vec<&LoweredNode> {
    let mut out = vec![node];
    for child in node.nodes() {
        out.extend(every(child));
    }
    out
}
