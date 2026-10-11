// @lfy def/workspace/main.lfy:load
//! The targets the program declares, read from the bound model: every `ace const` of a
//! project file holding a `Target` of the package `elfie` gives one, with the layers of
//! its slots in guidance order and the values those layers set.

use std::collections::{BTreeMap, HashSet};

use super::*;
use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::statement::Statement;
use crate::grammar::terminals::keyword::Keyword;
use crate::interpret::{Evaluated, Value as Computed};
use crate::model::{
    self, Entity, EntityId, EntityKind, FileId, Model, NodeRef, Operation, Problem, TypeRef,
};

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
// @lfy def/workspace/main.lfy:load#load:load:d4dc4c84aaf9ba37a87a65a201d1ab6827f3e54dd6bdc33f863354f6c3953644
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
// @lfy def/workspace/main.lfy:load#load:load:9c4b7f6784e978ab38426f62744c733c2fbdfebea9fe78c455d429cc1f876075
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
    // A const with a declared type and one written as a call of `targetOf` both come
    // through here: `Interpreter.evaluate` runs the call, because a call is compile-time
    // by where it is written, and the object either way is the slots.
    // @lfy def/workspace/main.lfy:load#load:load:9c4b7f6784e978ab38426f62744c733c2fbdfebea9fe78c455d429cc1f876075
    match crate::interpret::evaluate(model, value) {
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
// @lfy def/workspace/main.lfy:load#load:load:9c4b7f6784e978ab38426f62744c733c2fbdfebea9fe78c455d429cc1f876075
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
pub(super) fn targets_of(
    model: &mut Model,
    files: &[File],
    library: usize,
) -> (Vec<Target>, Vec<Problem>) {
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
        // @lfy def/workspace/main.lfy:load#load:load:722849a54dc2494a11532b2c54b4db177b978cbbe4a6dd812669557ef1006be2
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
                    stage: crate::model::Stage::Loader,
                });
            }
        }

        // A target cannot be built without an output, a language, and a layout.
        // @lfy def/workspace/main.lfy:load#load:load:02062f73a6a6da19fbd8cce8b082ddb666c5cf47a3baee8fedb5ae408646b65c
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
                stage: crate::model::Stage::Loader,
            });
            continue;
        }

        // A capability a layer needs that no layer gives.
        // @lfy def/workspace/main.lfy:load#load:load:e6137be72f47217dc6c217c142c019f7a2fe996e0f2e91068c2f30d7175fe0cb
        let provided = items_set(model, declaration, PROVIDES);
        for required in items_set(model, declaration, REQUIRES) {
            if !provided.contains(&required) {
                problems.push(Problem {
                    node: at,
                    message: format!(
                        "the target {identifier} requires {required} and none of its layers \
                         provides it"
                    ),
                    stage: crate::model::Stage::Loader,
                });
            }
        }

        // The layers, in guidance order, each trait once at its first place.
        // @lfy def/workspace/main.lfy:load#load:load:183ff338b3a3e2529d4c47790b5d0204470dcfc1bbb33c5621fb1d464e87e974
        let mut chosen: Vec<(EntityId, Vec<Computed>)> = Vec::new();
        for slot in GUIDANCE_ORDER {
            for layer in layers_of(model, &slots, slot) {
                // @lfy def/workspace/main.lfy:load#load:load:d1e245ccebeb2f7623e8da0f5e962388af2f23047e3d15da17c8352c50027b74
                if !chosen.contains(&layer) {
                    chosen.push(layer);
                }
            }
        }
        let layers: Vec<EntityId> = chosen.into_iter().map(|(subject, _)| subject).collect();

        // What its generated code needs, each in the ecosystem its ecosystem layer names.
        // @lfy def/workspace/main.lfy:load#load:load:835f6845f6033e939a044506a018d0a36680e14a6eb9beaa92012131e186f666
        let ecosystem = string_set(model, declaration, ECOSYSTEM).unwrap_or_default();
        let mut native_dependencies = Vec::new();
        if let Some(Computed::List(items)) = slots.get("dependencies") {
            for item in items {
                let Computed::Object(pairs) = item else { continue };
                let Some(Computed::String(name)) = pairs.get("name") else {
                    continue;
                };
                // @lfy def/workspace/main.lfy:load#load:load:835f6845f6033e939a044506a018d0a36680e14a6eb9beaa92012131e186f666
                native_dependencies.push(NativeDependency {
                    identifier: name.clone(),
                    ecosystem: ecosystem.clone(),
                    version: match pairs.get("version") {
                        Some(Computed::String(version)) => Some(version.clone()),
                        _ => None,
                    },
                });
            }
        }
        // @lfy def/workspace/main.lfy:load#load:load:8c54a1933f95101299c7d1fc73f6483a0cb687bec0ecf95ae14ac16aa7131263
        if !native_dependencies.is_empty() && !given(&slots, ECOSYSTEM) {
            problems.push(Problem {
                node: at,
                message: format!("the dependencies of the target {identifier} need an ecosystem"),
                stage: crate::model::Stage::Loader,
            });
        }

        // The extensions its outputs may have: its language's, then every one its layers
        // add, each once.
        // @lfy def/workspace/main.lfy:load#load:load:a937c3da8b47a6fdea99c31a14b24e1c2aa31829c11b46405b9701b9fcf00a07
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
        // @lfy def/workspace/main.lfy:load#load:load:18e900520d7cf988834a9b9730aeffd2562b0c3e7416b8940b4b784993194c57
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
        // @lfy def/workspace/main.lfy:load
        let knowledge = in_guidance_order(
            model,
            &layers,
            &model.entities[declaration].knowledge,
            |item| item.contributor,
        );

        targets.push(Target {
            // @lfy def/workspace/main.lfy:load#load:load:d4dc4c84aaf9ba37a87a65a201d1ab6827f3e54dd6bdc33f863354f6c3953644
            identifier,
            // @lfy def/workspace/main.lfy:load#load:load:d4dc4c84aaf9ba37a87a65a201d1ab6827f3e54dd6bdc33f863354f6c3953644
            declaration,
            // @lfy def/workspace/main.lfy:load#load:load:183ff338b3a3e2529d4c47790b5d0204470dcfc1bbb33c5621fb1d464e87e974
            layers,
            // @lfy def/workspace/main.lfy:load#load:load:f93277efea1e9ae1024146b1325d3d1bc82ec07b0fc2d632c73a5bbbbfb49f17
            output_directory: slots
                .get("output")
                .and_then(|output| match output {
                    Computed::String(output) => Some(trim_directory(output)),
                    _ => None,
                })
                .unwrap_or_default(),
            extensions,
            // @lfy def/workspace/main.lfy:load#load:load:a9fa8cd5e73cab1b5d4064fb290df44132c9dfca9e5d68d87f850ebb418b7a12
            marker_comment: string_set(model, declaration, MARKER_COMMENT).unwrap_or_default(),
            // @lfy def/workspace/main.lfy:load#load:load:a6d3cd85551722d81b7111f6681bf963da4ba6fb1d8ecb483d40fdad958c87c1
            script_runner: string_set(model, declaration, SCRIPT_RUNNER),
            native_dependencies,
            commands,
            knowledge,
        });
    }

    // Two targets that write into one directory, or one into the other's.
    // @lfy def/workspace/main.lfy:load#load:load:0bb2c8b9bdc22bcaf7b8375709c269d2ce9e4a87d15bad0b911bfef5a5e9dfb3
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
                    stage: crate::model::Stage::Loader,
                });
            }
        }
    }
    (targets, problems)
}
