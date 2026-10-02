//! Compiled from `def/generation/main.lfy`: what a dependent may rely on, planning what
//! to generate, asking for it, reading how a run ended, and accepting what comes back.
//!
//! A unit is one source file for one target: outputs and source maps are per file, a
//! declaration's scope is its file, and a smaller unit would make the compiler merge
//! into a file it does not own, which is itself an act of generation. A larger unit
//! would not fit a request, but several units can share one: that is a [`Batch`].
//! [`interface_of`] is the text whose hash tells a dependent whether anything it may rely
//! on changed; [`plan`] finds every unit of a workspace, which need generating, and how
//! they are batched; [`request`] gathers everything the compiler is handed for one batch,
//! with every criterion and test already resolved so that a compiler with no access to
//! the model can still work; [`outcome_of`] reads how a run ended; [`accept`] checks
//! outputs structurally, normalizes their markers, and gives them their source maps.
//! [`regions_of`] finds the generated regions one entity's name owns, [`changes`] what
//! differs for each entity of a unit since its outputs were accepted, [`review`] what an
//! independent verifier is handed for one batch, [`global_review`] what it is handed for
//! every global criterion and test, and [`review_of`] what it found. [`source_maps_of`]
//! reads the source maps a workspace recorded and [`record`] writes those of one accepted
//! unit.
//! Nothing here shells out.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod data; // @lfy def/generation/data.lfy:Plan

pub use data::*;

use sha2::{Digest, Sha256};
use unicode_ident::{is_xid_continue, is_xid_start};

use crate::grammar::Entity as Rule;
use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::statement::Statement;
use crate::interpret::{
    Evaluated, LoweredChild, LoweredCriterion, LoweredFile, LoweredNode, LoweredTest, Program,
};
use crate::model::{
    self, Applied, AppliedSource, Criterion, Entity, EntityId, EntityKind, FileId, Model, NodeRef,
    SymbolKind, Value,
};
use crate::parser::Child;
use crate::parser::components::is_trivia;
use crate::workspace::{File, NativeDependency, Target, Workspace};

/// The extension of a source file.
const EXTENSION: &str = ".lfy";
/// What a marker starts with, after the comment opener of the target's language.
const MARKER_PREFIX: &str = "@lfy ";
/// How many units one batch may hold.
// @lfy def/generation/main.lfy:plan
const BATCH_UNITS: usize = 6;
/// How many characters of source one batch may hold.
// @lfy def/generation/main.lfy:plan
const BATCH_CHARACTERS: usize = 60_000;
/// Where the compile-time data of a project lives, under the workspace root.
// Decision: compile-time data lives in `elfie-compile` under the workspace root, never
// under an output directory, so outputs hold only generated code. `elfie-compile/maps`
// holds the source maps, one file per unit, so accepting a unit writes one small file
// instead of rewriting every map of the target, and a commit shows exactly which units a
// compile touched.
// @lfy def/generation/main.lfy:sourceMapsOf
const MAPS: &str = "elfie-compile/maps";
/// What earlier versions recorded every map of a target in, under its output directory.
// @lfy def/generation/main.lfy:sourceMapsOf
const LEGACY_MAPS: &str = "source-map.json";

// ---------------------------------------------------------------------------------------
// the lowered program
// ---------------------------------------------------------------------------------------

/// Every lowered node of a tree, the node itself first and then its children in source
/// order.
// @lfy def/generation/main.lfy:plan
fn lowered_nodes<'f>(node: &'f LoweredNode, out: &mut Vec<&'f LoweredNode>) {
    out.push(node);
    for child in node.nodes() {
        lowered_nodes(child, out);
    }
}

/// Every entity a lowered node of the file declares, in file order. An entity whose code
/// runs at compile time, such as a trait or an ace function, has no lowered node and so is
/// not among them.
// @lfy def/generation/main.lfy:plan
fn lowered_entities(file: &LoweredFile) -> Vec<EntityId> {
    let mut nodes = Vec::new();
    lowered_nodes(&file.root, &mut nodes);
    let mut out: Vec<EntityId> = Vec::new();
    for node in nodes {
        if let Some(entity) = node.entity
            && !out.contains(&entity)
        {
            out.push(entity);
        }
    }
    out
}

/// The lowered node that declares an entity; `None` when none does.
// @lfy def/generation/main.lfy:request
fn lowered_node_of(file: &LoweredFile, entity: EntityId) -> Option<&LoweredNode> {
    let mut nodes = Vec::new();
    lowered_nodes(&file.root, &mut nodes);
    nodes.into_iter().find(|node| node.entity == Some(entity))
}

/// The local criteria and tests of one entity of a file, criteria first.
// @lfy def/generation/main.lfy:review
fn entity_requirements(file: &LoweredFile, entity: EntityId) -> Vec<Requirement> {
    let Some(node) = lowered_node_of(file, entity) else {
        return Vec::new();
    };
    node.criteria
        .iter()
        .cloned()
        .map(Requirement::Criterion)
        .chain(node.tests.iter().cloned().map(Requirement::Test))
        .collect()
}

/// Every local criterion and test of a file: those of each lowered node in file order,
/// criteria before tests, then those of the file's own entity.
// @lfy def/generation/main.lfy:request
fn local_requirements(file: &LoweredFile) -> Vec<Requirement> {
    let mut nodes = Vec::new();
    lowered_nodes(&file.root, &mut nodes);
    let mut out: Vec<Requirement> = Vec::new();
    for node in nodes {
        out.extend(node.criteria.iter().cloned().map(Requirement::Criterion));
        out.extend(node.tests.iter().cloned().map(Requirement::Test));
    }
    out.extend(file.criteria.iter().cloned().map(Requirement::Criterion));
    out.extend(file.tests.iter().cloned().map(Requirement::Test));
    out
}

/// `SourceMap.requirements`: the SHA-256 of the ids of a unit's local criteria and tests,
/// sorted and joined by line breaks.
// @lfy def/generation/main.lfy:plan
fn requirements_hash(program: &Program, unit: &Unit) -> String {
    let mut ids: Vec<String> = local_requirements(&program.files[unit.lowered])
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();
    ids.sort();
    source_hash(&ids.join("\n"))
}

/// The lowered code of one node as Elfie source: kept nodes as written, folded nodes as the
/// literal of their value, and member nodes as a member with its type and value.
// Decision: lowering spells a whole file this way into `LoweredFile.text`, and exports no
// spelling of one node; a request quotes one entity's code at a time, so the same spelling
// is written here for a node.
// @lfy def/generation/main.lfy:request
fn lowered_code(model: &Model, node: &LoweredNode) -> String {
    if let Some(value) = &node.value {
        return lowered_value(model, value);
    }
    if let Some(entity) = member_node(model, node) {
        return lowered_member(model, entity);
    }
    let (members, kept): (Vec<&LoweredChild>, Vec<&LoweredChild>) =
        node.children.iter().partition(|child| {
            child
                .as_node()
                .is_some_and(|child| member_node(model, child).is_some())
        });
    let mut out = String::new();
    for child in kept {
        match child {
            LoweredChild::Node(child) => out.push_str(&lowered_code(model, child)),
            LoweredChild::Token(token) => out.push_str(&token.raw),
        }
    }
    // A data and a fn take a block or a semicolon; the block of either is compile time, so
    // what is left ends in the members that are kept apart from it, or in a semicolon.
    let bodied = node.rule == Rule::Statement(Statement::DataDeclaration)
        || node.rule == Rule::Statement(Statement::AgentFunctionDeclaration);
    if members.is_empty() {
        let trimmed = out.trim_end();
        if bodied && !trimmed.ends_with(';') && !trimmed.ends_with('}') {
            return format!("{trimmed};{}", &out[trimmed.len()..]);
        }
        return out;
    }
    let mut body = out.trim_end().to_string();
    body.push_str(" {\n");
    for child in members {
        if let LoweredChild::Node(child) = child {
            body.push_str("  ");
            body.push_str(&lowered_code(model, child));
            body.push('\n');
        }
    }
    body.push('}');
    body
}

/// The member a lowered node is, when it is one of its parent's entity.
// @lfy def/generation/main.lfy:request
fn member_node(model: &Model, node: &LoweredNode) -> Option<EntityId> {
    let entity = node.entity?;
    (node.value.is_none()
        && node.children.is_empty()
        && matches!(
            model.entities[entity].kind,
            EntityKind::Member | EntityKind::EnumMember
        ))
    .then_some(entity)
}

/// A member as Elfie spells one: its name, its description, and its type or value.
// @lfy def/generation/main.lfy:request
fn lowered_member(model: &Model, entity: EntityId) -> String {
    let record = &model.entities[entity];
    let mut out = format!("${}", record.identifier.clone().unwrap_or_default());
    if let Some(definition) = &record.definition {
        let _ = write!(out, ": `{definition}`");
    }
    match record.ty.as_ref().map(|ty| model::type_text(model, ty)) {
        Some(written) if !written.is_empty() => {
            let _ = write!(out, " = {written};");
        }
        _ => out.push(';'),
    }
    out
}

/// A folded value as Elfie spells it; a prompted value as the call that asked for it.
// @lfy def/generation/main.lfy:request
fn lowered_value(model: &Model, value: &Evaluated) -> String {
    match value {
        Evaluated::Value(value) => value.spelled(model),
        Evaluated::Prompted(prompted) => format!(
            "{}@like(`{}`)",
            entity_name(model, prompted.value_type),
            prompted.prompt
        ),
    }
}

// ---------------------------------------------------------------------------------------
// interfaceOf
// ---------------------------------------------------------------------------------------

/// What dependents of a unit may rely on, as one text whose hash tells whether it changed.
///
/// One line per entity of the unit in file order — its identifier, its kind, its
/// definition, its type, and for a fn its parameters with their types and its output —
/// and, after the line of a data, type, trait, or enum, one line per member or enum
/// member it declares, since a dependent relies on those as much as on the declaration
/// itself. A type line spells an entity of the `elfie` package by its identifier alone,
/// such as `List` or `Path`, never by a path or a module name. Nothing else is written: a
/// change that leaves every line the same, such as a new criterion or a moved line, leaves
/// the text the same, so dependents are not regenerated for it.
// @lfy def/generation/main.lfy:interfaceOf
pub fn interface_of(workspace: &Workspace, unit: &Unit) -> String {
    let model = &workspace.model;
    let mut out = String::new();
    for &entity in &unit.entities {
        // @lfy def/generation/main.lfy:interfaceOf
        write_interface_entity(&mut out, model, entity, "");
        // @lfy def/generation/main.lfy:interfaceOf
        for member in members_of(model, entity) {
            write_interface_entity(&mut out, model, member, "  ");
        }
    }
    out
}

/// SHA-256 of a unit's interface text.
// @lfy def/generation/main.lfy:interfaceOf
fn interface_signature(workspace: &Workspace, unit: &Unit) -> String {
    source_hash(&interface_of(workspace, unit))
}

/// The members and enum members a data, type, trait, or enum declares, in order; none for
/// anything else.
// @lfy def/generation/main.lfy:interfaceOf
fn members_of(model: &Model, entity: EntityId) -> Vec<EntityId> {
    let record = &model.entities[entity];
    if !matches!(
        record.kind,
        EntityKind::Data | EntityKind::Type | EntityKind::Enum | EntityKind::Trait { .. }
    ) {
        return Vec::new();
    }
    let Some(scope) = record.scope else {
        return Vec::new();
    };
    model.scopes[scope]
        .symbols
        .iter()
        .filter(|&&symbol| {
            matches!(
                model.symbols[symbol].kind,
                SymbolKind::Member | SymbolKind::EnumMember
            )
        })
        .map(|&symbol| model.symbols[symbol].entity)
        .collect()
}

/// One entity as an interface line: identifier, kind, definition, type, and for a fn its
/// parameters with their types and its output. No line number is written, so the text
/// depends on nothing but what a dependent may rely on. A type is spelled by the
/// identifier of the entity it names, wherever that entity was declared, so an entity of
/// the `elfie` package reads `List` or `Path` and never a path or a module name.
// @lfy def/generation/main.lfy:interfaceOf
fn write_interface_entity(out: &mut String, model: &Model, entity: EntityId, indent: &str) {
    let record = &model.entities[entity];
    let _ = write!(
        out,
        "{indent}- `{}` ({})",
        entity_name(model, entity),
        kind_text(record)
    );
    if let Some(definition) = &record.definition {
        let _ = write!(out, ": {definition}");
    }
    if let Some(ty) = &record.ty {
        let _ = write!(out, " — type `{}`", model::type_text(model, ty));
    }
    let parameters: Vec<String> = record
        .parameters()
        .iter()
        .map(|&symbol| {
            let symbol = &model.symbols[symbol];
            match &model.entities[symbol.entity].ty {
                Some(ty) => format!("{}: {}", symbol.name, model::type_text(model, ty)),
                None => symbol.name.clone(),
            }
        })
        .collect();
    if !parameters.is_empty() {
        let _ = write!(out, " — parameters ({})", parameters.join(", "));
    }
    if let Some(output) = record.output() {
        let _ = write!(out, " — output `{}`", model::type_text(model, output));
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------------------------

/// Every unit of a workspace, which need generating, and how they are batched.
///
/// An entity is built for a target when it carries the target's marker, when the
/// anonymous entity of its file does, or when `global` does; the last two select every
/// entity declared in the file scope. Every file without a package that has at least one
/// built entity for a target gives one unit holding those entities in file order; a file
/// that has a package gives no unit, since a package is compiled by its own project. That
/// holds for the `elfie` package, the standard library, even when the marker is applied to
/// `global`: none of its entities is ever in a unit's entities, though a unit's interface
/// may still name them as types.
/// Units come in target order then file order, each after its dependencies. A unit's
/// reason is the first of: requested, violated, fresh, changed, requirements, dependency;
/// `None` when it is up to date. Callers pass [`source_maps_of`] the workspace, which
/// leaves out maps whose output no longer exists.
///
/// The plan is given with the program it was planned from: [`crate::interpret::lower`] of
/// the workspace, which every unit's lowered file, every request, and every review reads.
// Decision: the definition gives the plan both the workspace and the program lowered from
// it; a `Plan` here owns neither, since the program holds the workspace it was lowered
// from, so the program the plan is for comes back beside it and a unit names its lowered
// file by its index in `Program::files`.
// @lfy def/generation/main.lfy:plan
pub fn plan(
    workspace: Workspace,
    source_maps: &[SourceMap],
    requested: &[String],
    violated: &[String],
) -> (Program, Plan) {
    // The plan's program is the workspace lowered: runtime code only, with every
    // compile-time result in place. @lfy def/generation/main.lfy:plan
    let lowered = crate::interpret::lower(workspace);
    let program = &lowered;
    let workspace = &program.workspace;
    let model = &workspace.model;
    let mut units: Vec<Unit> = Vec::new();

    for (target_index, target) in workspace.targets.iter().enumerate() {
        // Selection: which files give a unit, and with which entities.
        // @lfy def/generation/main.lfy:plan
        let mut unit_of_file: Vec<Option<usize>> = vec![None; workspace.files.len()];
        let mut local: Vec<(usize, Vec<EntityId>)> = Vec::new();
        for (file_index, file) in workspace.files.iter().enumerate() {
            if file.package.is_some() {
                continue; // @lfy def/generation/main.lfy:plan
            }
            // An entity with no lowered node, such as a trait or an ace function, is never
            // among a unit's entities, so no output has to carry a marker for it.
            // @lfy def/generation/main.lfy:plan
            let lowered = lowered_entities(&program.files[file_index]);
            let entities: Vec<EntityId> = built_entities(model, file.source, target.marker)
                .into_iter()
                .filter(|entity| lowered.contains(entity))
                .collect();
            if entities.is_empty() {
                continue;
            }
            unit_of_file[file_index] = Some(local.len());
            local.push((file_index, entities));
        }

        // Dependencies, as positions among this target's units.
        // @lfy def/generation/main.lfy:plan
        let dependencies: Vec<Vec<usize>> = local
            .iter()
            .map(|(file_index, _)| dependencies_of(workspace, *file_index, &unit_of_file))
            .collect();

        // Order: each unit after its dependencies, otherwise in file order.
        // @lfy def/generation/main.lfy:plan
        let order = order_after_dependencies(&dependencies);
        let base = units.len();
        let mut position = vec![0; local.len()];
        for (offset, &item) in order.iter().enumerate() {
            position[item] = base + offset;
        }

        for &item in &order {
            let (file_index, entities) = &local[item];
            let file = &workspace.files[*file_index];
            units.push(Unit {
                target: target_index,
                file: *file_index,
                entities: entities.clone(),
                // The program's files run parallel to the workspace's, so a file's index
                // is the index of the file lowered from it.
                lowered: *file_index,
                stem: stem_of(&file.path, &workspace.source_directory), // @lfy def/generation/main.lfy:plan
                dependencies: dependencies[item].iter().map(|&d| position[d]).collect(),
                // @lfy def/generation/main.lfy:plan
                outputs: source_maps
                    .iter()
                    .filter(|map| map.target == target.identifier && map.source == file.path)
                    .cloned()
                    .collect(),
                reason: None,
            });
        }
    }

    // Staleness. A dependency is compared by the hash of its interface, never by whether
    // it is itself planned, so a dependency planned only because of its own dependencies,
    // and whose interface is unchanged, plans nothing beyond itself.
    // @lfy def/generation/main.lfy:plan
    let signatures: Vec<String> = units
        .iter()
        .map(|unit| interface_signature(workspace, unit))
        .collect();
    for index in 0..units.len() {
        units[index].reason = reason_of(program, &units, &signatures, index, requested, violated);
    }

    let batches = batches_of(program, &units);
    // @lfy def/generation/main.lfy:plan
    (lowered, Plan { units, batches })
}

/// The entities of a file built for a target's marker, in file order.
// @lfy def/generation/main.lfy:plan
fn built_entities(model: &Model, source: usize, marker: EntityId) -> Vec<EntityId> {
    let global_has = model.entities[model.global].has_trait(marker);
    let file_has = model
        .file_entities
        .get(source)
        .is_some_and(|&entity| model.entities[entity].has_trait(marker));
    let mut out: Vec<EntityId> = Vec::new();
    if let Some(&scope) = model.file_scopes.get(source) {
        for &symbol in &model.scopes[scope].symbols {
            let symbol = &model.symbols[symbol];
            if symbol.kind == SymbolKind::Module {
                continue;
            }
            let entity = &model.entities[symbol.entity];
            // Decision: an alias symbol may be bound to an entity of another file; only
            // entities declared in this file belong to its unit.
            if entity.file != Some(source) || entity.node.is_none() || out.contains(&symbol.entity)
            {
                continue;
            }
            if global_has || file_has || entity.has_trait(marker) {
                out.push(symbol.entity);
            }
        }
    }
    // Decision: an entity that carries the marker itself but is declared below the file
    // scope (say, inside a block) is built too, since `entitiesOf` the marker lists it;
    // members, parameters, and other parts of a declaration are not, since they are built
    // with the declaration that holds them.
    for &entity in model::entities_of(model, marker).iter() {
        let record = &model.entities[entity];
        if record.file == Some(source)
            && record.node.is_some()
            && is_declaration(&record.kind)
            && !out.contains(&entity)
        {
            out.push(entity);
        }
    }
    out.sort_by_key(|&entity| model.entities[entity].node.map(|node| node.index));
    out
}

/// Whether an entity kind is a declaration a unit can be made of.
fn is_declaration(kind: &EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::Data
            | EntityKind::Type
            | EntityKind::Enum
            | EntityKind::Fn { .. }
            | EntityKind::Trait { .. }
            | EntityKind::Variable
            | EntityKind::Alias
            | EntityKind::External
    )
}

/// The units (as positions among one target's units) a file depends on: the unit of each
/// file it uses, or, for a used file with no unit, the units of its own uses, transitively.
/// Each unit appears once; a cycle among uses adds nothing.
// @lfy def/generation/main.lfy:plan
fn dependencies_of(
    workspace: &Workspace,
    file_index: usize,
    unit_of_file: &[Option<usize>],
) -> Vec<usize> {
    let mut out = Vec::new();
    let mut visited: HashSet<usize> = HashSet::new();
    visited.insert(file_index);
    walk_uses(workspace, file_index, unit_of_file, &mut visited, &mut out);
    out
}

fn walk_uses(
    workspace: &Workspace,
    file_index: usize,
    unit_of_file: &[Option<usize>],
    visited: &mut HashSet<usize>,
    out: &mut Vec<usize>,
) {
    let source = workspace.files[file_index].source;
    for used in workspace.model.sources[source].uses.iter().flatten() {
        let Some(used_index) = workspace.files.iter().position(|file| &file.path == used) else {
            continue;
        };
        if !visited.insert(used_index) {
            continue; // @lfy def/generation/main.lfy:plan
        }
        match unit_of_file[used_index] {
            Some(unit) => {
                if !out.contains(&unit) {
                    out.push(unit);
                }
            }
            None => walk_uses(workspace, used_index, unit_of_file, visited, out),
        }
    }
}

/// The items in an order that places each after its dependencies, otherwise in the order
/// given. Items in a cycle keep the order given.
// @lfy def/generation/main.lfy:plan
fn order_after_dependencies(dependencies: &[Vec<usize>]) -> Vec<usize> {
    fn visit(item: usize, dependencies: &[Vec<usize>], state: &mut [u8], order: &mut Vec<usize>) {
        if state[item] != 0 {
            return;
        }
        state[item] = 1;
        for &dependency in &dependencies[item] {
            visit(dependency, dependencies, state, order);
        }
        state[item] = 2;
        order.push(item);
    }
    let mut state = vec![0u8; dependencies.len()];
    let mut order = Vec::with_capacity(dependencies.len());
    for item in 0..dependencies.len() {
        visit(item, dependencies, &mut state, &mut order);
    }
    order
}

/// `Unit.stem`: the file's path relative to the source directory, without `.lfy`.
// @lfy def/generation/main.lfy:plan
fn stem_of(path: &str, source_directory: &str) -> String {
    // Decision: a file the program reached through a `use` from outside the source
    // directory keeps its whole path as its stem, so that two such files cannot collide.
    let relative = strip_directory(path, source_directory).unwrap_or(path);
    relative
        .strip_suffix(EXTENSION)
        .unwrap_or(relative)
        .to_string()
}

/// Why a unit is planned: the first of requested, violated, fresh, changed, requirements,
/// dependency that holds.
///
/// A difference in the program's global criteria or tests plans no unit of its own: the
/// global review is run instead, and a unit whose output answers for a global criterion or
/// test that review found violated is passed in `violated`, which plans it again. A review
/// that finds none leaves every unit as it was.
// @lfy def/generation/main.lfy:plan
fn reason_of(
    program: &Program,
    units: &[Unit],
    signatures: &[String],
    index: usize,
    requested: &[String],
    violated: &[String],
) -> Option<Reason> {
    let workspace = &program.workspace;
    let unit = &units[index];
    let file = &workspace.files[unit.file];
    if requested.iter().any(|r| r == &file.path || r == &unit.stem) {
        return Some(Reason::Requested);
    }
    if violated.iter().any(|stem| stem == &unit.stem) {
        return Some(Reason::Violated); // @lfy def/generation/main.lfy:plan
    }
    if unit.outputs.is_empty() {
        return Some(Reason::Fresh);
    }
    // The lowered text, not the source: an edit that leaves the lowered code the same, such
    // as a comment, changes nothing. @lfy def/generation/main.lfy:plan
    let hash = source_hash(&program.files[unit.lowered].text);
    if unit.outputs.iter().any(|output| output.hash != hash) {
        return Some(Reason::Changed);
    }
    // @lfy def/generation/main.lfy:plan
    let requirements = requirements_hash(program, unit);
    if unit
        .outputs
        .iter()
        .any(|output| output.requirements != requirements)
    {
        return Some(Reason::Requirements);
    }
    // A dependency whose interface differs from the one the outputs were generated
    // against, or which they record nothing for, plans the unit.
    let stale = unit.dependencies.iter().any(|&dependency| {
        let path = &workspace.files[units[dependency].file].path;
        let signature = &signatures[dependency];
        unit.outputs
            .iter()
            .any(|output| output.dependencies.get(path) != Some(signature))
    });
    if stale {
        return Some(Reason::Dependency);
    }
    None // @lfy def/generation/main.lfy:plan
}

/// The planned units partitioned into batches, in plan order.
///
/// A unit joins the open batch when both share the first segment of their stem — the
/// directory the unit sits in, empty for a unit directly under the source directory —
/// the batch holds fewer than six units, and the sources of the batch and the unit
/// together are under 60,000 characters; otherwise it opens a new batch. A unit whose
/// reason is dependency and whose only planned dependencies are in the open batch joins
/// that batch even across directories, since it is compiled against what the batch
/// produces. Because the units are visited in plan order, and every unit comes after its
/// dependencies, every batch comes after the batches holding the dependencies of its
/// units.
// @lfy def/generation/main.lfy:plan
fn batches_of(program: &Program, units: &[Unit]) -> Vec<Batch> {
    // Decision: the criteria give one guidance and one set of native dependencies per
    // request, so a batch holds units of one target; two targets never share a batch even
    // when their stems agree.
    let mut batches: Vec<Batch> = Vec::new();
    let mut open_characters = 0usize;
    for (index, unit) in units.iter().enumerate() {
        if unit.reason.is_none() {
            continue; // @lfy def/generation/main.lfy:plan
        }
        // The source a request carries is the lowered text, so that is what is measured.
        let characters = program.files[unit.lowered].text.chars().count();
        let joins = batches.last().is_some_and(|open| {
            let first = &units[open.units[0]];
            // Decision: the exception waives the directory, not the size of a batch; a
            // batch that is already full opens a new one whatever the reason.
            let dependent = unit.reason == Some(Reason::Dependency) // @lfy def/generation/main.lfy:plan
                && unit
                    .dependencies
                    .iter()
                    .all(|&d| units[d].reason.is_none() || open.units.contains(&d));
            first.target == unit.target
                && (first_segment(&first.stem) == first_segment(&unit.stem) || dependent)
                && open.units.len() < BATCH_UNITS
                && open_characters + characters < BATCH_CHARACTERS
        });
        match batches.last_mut() {
            Some(open) if joins => {
                open.units.push(index);
                open_characters += characters;
            }
            _ => {
                batches.push(Batch {
                    units: vec![index],
                    identifier: String::new(),
                });
                open_characters = characters;
            }
        }
    }
    // @lfy def/generation/main.lfy:plan
    for batch in &mut batches {
        batch.identifier = identifier_of(units, &batch.units);
    }
    batches
}

/// The first segment of a stem: the directory the unit sits in, and the empty string for
/// a unit directly under the source directory.
// @lfy def/generation/main.lfy:plan
fn first_segment(stem: &str) -> &str {
    match stem.split_once('/') {
        Some((first, _)) => first,
        None => "",
    }
}

/// `Batch.identifier`: the first unit's stem, then a plus sign and the count of the
/// others when there are any.
// @lfy def/generation/main.lfy:plan
fn identifier_of(units: &[Unit], batch: &[usize]) -> String {
    match batch.split_first() {
        Some((&first, [])) => units[first].stem.clone(),
        Some((&first, rest)) => format!("{}+{}", units[first].stem, rest.len()),
        None => String::new(),
    }
}

/// The text of a file: the joined raw text of its tokens.
fn source_text(workspace: &Workspace, file: &File) -> String {
    workspace.model.sources[file.source]
        .tree
        .tokens
        .iter()
        .map(|token| token.raw.as_str())
        .collect()
}

// ---------------------------------------------------------------------------------------
// request
// ---------------------------------------------------------------------------------------

/// Everything the compiler is handed to produce the outputs of one batch.
///
/// The sources are the lowered text of each unit's file by path, and the requirements its
/// local criteria and tests, each with its id; the globals are every global criterion and
/// test of the program; `existing` is kept only where its path is among the outputs of a
/// unit of the batch; there is one interface per dependency of the batch that is not itself
/// in it; the guidance is every criterion of the target's marker and of every trait it
/// extends, as [`guidance_of`] gathers them; and the native dependencies are the
/// workspace's followed by the target package's. The instructions quote every criterion and
/// test already resolved, so a compiler with no access to the model can still work.
// Decision: the definition passes the plan and the batch; a plan here does not own its
// workspace or the program lowered from it, so the program is an extra first parameter.
// @lfy def/generation/main.lfy:request
pub fn request(
    program: &Program,
    plan: &Plan,
    batch: &Batch,
    existing: &[Output],
    previous: &BTreeMap<String, String>,
) -> Request {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let Some(&first) = batch.units.first() else {
        // Decision: an empty batch has no target, so there is nothing to say; the caller
        // never builds one, since `plan` only batches units it planned.
        return Request {
            batch: batch.clone(),
            instructions: String::new(),
            sources: BTreeMap::new(),
            requirements: BTreeMap::new(),
            globals: Vec::new(),
            previous: previous.clone(),
            existing: BTreeMap::new(),
            interfaces: Vec::new(),
            guidance: Vec::new(),
            native_dependencies: Vec::new(),
        };
    };
    let target = &workspace.targets[plan.units[first].target];

    // @lfy def/generation/main.lfy:request
    let sources: BTreeMap<String, String> = batch
        .units
        .iter()
        .map(|&index| {
            let unit = &plan.units[index];
            (
                workspace.files[unit.file].path.clone(),
                program.files[unit.lowered].text.clone(),
            )
        })
        .collect();
    // @lfy def/generation/main.lfy:request
    let requirements: BTreeMap<String, Vec<Requirement>> = batch
        .units
        .iter()
        .map(|&index| {
            let unit = &plan.units[index];
            (
                workspace.files[unit.file].path.clone(),
                local_requirements(&program.files[unit.lowered]),
            )
        })
        .collect();
    // @lfy def/generation/main.lfy:request
    let globals: Vec<Requirement> = program
        .criteria
        .iter()
        .cloned()
        .map(Requirement::Criterion)
        .chain(program.tests.iter().cloned().map(Requirement::Test))
        .collect();
    // @lfy def/generation/main.lfy:request
    let existing: BTreeMap<String, String> = existing
        .iter()
        .filter(|output| {
            batch.units.iter().any(|&index| {
                plan.units[index]
                    .outputs
                    .iter()
                    .any(|map| map.output == output.path)
            })
        })
        .map(|output| (output.path.clone(), output.text.clone()))
        .collect();
    // @lfy def/generation/main.lfy:request
    let mut interfaces: Vec<Interface> = Vec::new();
    for &index in &batch.units {
        for &dependency in &plan.units[index].dependencies {
            if batch.units.contains(&dependency)
                || interfaces
                    .iter()
                    .any(|interface| interface.unit == dependency)
            {
                continue;
            }
            interfaces.push(Interface {
                unit: dependency,
                outputs: plan.units[dependency]
                    .outputs
                    .iter()
                    .map(|map| map.output.clone())
                    .collect(),
                entities: plan.units[dependency].entities.clone(),
            });
        }
    }
    let guidance = guidance_of(model, target.marker); // @lfy def/generation/main.lfy:request
    // @lfy def/generation/main.lfy:request
    let native_dependencies: Vec<NativeDependency> = workspace
        .native_dependencies
        .iter()
        .chain(
            workspace.packages[target.package]
                .native_dependencies
                .iter(),
        )
        .cloned()
        .collect();

    let instructions = instructions(
        program,
        plan,
        batch,
        target,
        &Quoted {
            sources: &sources,
            globals: &globals,
            previous,
            existing: &existing,
            interfaces: &interfaces,
            guidance: &guidance,
            native_dependencies: &native_dependencies,
        },
    );

    Request {
        batch: batch.clone(),
        instructions,
        sources,
        requirements,
        globals,
        previous: previous.clone(),
        existing,
        interfaces,
        guidance,
        native_dependencies,
    }
}

/// What the instructions quote, gathered before they are written.
// Decision: the definition writes the instructions from the request it is building; the
// parts are grouped here rather than passed one by one.
// @lfy def/generation/main.lfy:request
struct Quoted<'r> {
    sources: &'r BTreeMap<String, String>,
    globals: &'r [Requirement],
    previous: &'r BTreeMap<String, String>,
    existing: &'r BTreeMap<String, String>,
    interfaces: &'r [Interface],
    guidance: &'r [Criterion],
    native_dependencies: &'r [NativeDependency],
}

/// The guidance a target's marker gives: its own criteria, then those of every trait it
/// extends, transitively and nearest first, so a marker extending `targetLanguage` gives
/// its own criteria, then those of `targetLanguage`, then those of `target`. A trait
/// reached twice contributes once, at its first place. A trait the marker extends with
/// arguments, such as `cratePerPrefix` and `modulePerFile`, contributes in extends order
/// after the marker's own, with each of its template values evaluated from those
/// arguments; every template value of the guidance is evaluated for the marker's
/// application.
// Decision: a trait's own criteria keep a template value of its parameters as written,
// since the parameters are unbound where the trait is declared; the value is substituted
// here, where the application that bound them is known.
// @lfy def/generation/main.lfy:request
pub fn guidance_of(model: &Model, marker: EntityId) -> Vec<Criterion> {
    let mut out: Vec<Criterion> = Vec::new();
    let mut seen: Vec<EntityId> = vec![marker];
    // Nearest first: each trait is visited a whole level after the trait that extends it.
    // @lfy def/generation/main.lfy:request
    let mut queue: VecDeque<(EntityId, Vec<(String, String)>)> =
        VecDeque::from([(marker, Vec::new())]);
    while let Some((entity, arguments)) = queue.pop_front() {
        for mut criterion in model::criteria_of(model, entity) {
            // @lfy def/generation/main.lfy:request
            substitute(&mut criterion, &arguments);
            out.push(criterion);
        }
        for applied in &model.entities[entity].traits {
            if !matches!(applied.source, AppliedSource::Extends(_))
                || seen.contains(&applied.entity)
            {
                continue; // @lfy def/generation/main.lfy:request
            }
            seen.push(applied.entity);
            queue.push_back((applied.entity, arguments_of(model, applied, &arguments)));
        }
    }
    out
}

/// The parameters of an applied trait bound to the text of the arguments it was applied
/// with, in declaration order.
// @lfy def/generation/main.lfy:request
fn arguments_of(
    model: &Model,
    applied: &Applied,
    outer: &[(String, String)],
) -> Vec<(String, String)> {
    model.entities[applied.entity]
        .parameters()
        .iter()
        .enumerate()
        .map(|(index, &symbol)| {
            let text = match applied.values.get(index) {
                Some(Value::Undefined) | None => {
                    // An argument written in terms of the extending trait's own parameters
                    // could not be evaluated where it stands; its text carries them, and
                    // the application that bound them is the one outside.
                    match applied.arguments.get(index) {
                        Some(&node) => substitute_text(model.raw(node).trim(), outer),
                        None => String::new(),
                    }
                }
                Some(value) => model::value_text(model, value),
            };
            (model.symbols[symbol].name.clone(), text)
        })
        .collect()
}

/// A criterion with every template value of its texts substituted.
// @lfy def/generation/main.lfy:request
fn substitute(criterion: &mut Criterion, arguments: &[(String, String)]) {
    if arguments.is_empty() {
        return;
    }
    for texts in [
        criterion.situation.as_mut(),
        criterion.behavior.as_mut(),
        criterion.side_effects.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        for text in texts {
            *text = substitute_text(text, arguments);
        }
    }
}

/// A text with each `{{name}}` naming one of the arguments replaced by its value, and
/// every other template value left as written.
// @lfy def/generation/main.lfy:request
fn substitute_text(text: &str, arguments: &[(String, String)]) -> String {
    if arguments.is_empty() || !text.contains("{{") {
        return text.to_string();
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str("{{");
            rest = after;
            continue;
        };
        let inner = &after[..end];
        match arguments.iter().find(|(name, _)| name == inner.trim()) {
            Some((_, value)) => out.push_str(value),
            None => {
                let _ = write!(out, "{{{{{inner}}}}}");
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// The prompt: what to produce, where, the rules for producing it, and how to report the
/// outcome, in the order the definition lists.
// @lfy def/generation/main.lfy:request
fn instructions(
    program: &Program,
    plan: &Plan,
    batch: &Batch,
    target: &Target,
    quoted: &Quoted<'_>,
) -> String {
    let workspace = &program.workspace;
    let Quoted {
        sources,
        globals,
        previous,
        existing,
        interfaces,
        guidance,
        native_dependencies,
    } = *quoted;
    let model = &workspace.model;
    let mut out = String::new();

    // The reader and its job, and the units by stem. @lfy def/generation/main.lfy:request
    let _ = writeln!(
        out,
        "# Compiling the batch `{}` for the target `{}`\n",
        batch.identifier, target.identifier
    );
    let _ = writeln!(
        out,
        "You are the compiler for the target `{}`. Your whole job is the outputs for the units of this \
         batch, and you write nothing outside them:\n",
        target.identifier
    );
    for &index in &batch.units {
        let unit = &plan.units[index];
        let _ = writeln!(
            out,
            "- `{}` (`{}`)",
            unit.stem, workspace.files[unit.file].path
        );
    }
    out.push('\n');

    // Where the outputs go. @lfy def/generation/main.lfy:request
    out.push_str("## Where the outputs go\n\n");
    let _ = writeln!(
        out,
        "The outputs go under `{}`. Each unit's file names are spelled from its stem as the guidance \
         says.\n",
        target.output_directory
    );
    for &index in &batch.units {
        let unit = &plan.units[index];
        if unit.outputs.is_empty() {
            let _ = writeln!(
                out,
                "- `{}`: nothing has been generated for it yet.",
                unit.stem
            );
        } else {
            let paths: Vec<String> = unit
                .outputs
                .iter()
                .map(|map| format!("`{}`", map.output))
                .collect();
            let _ = writeln!(
                out,
                "- `{}`: the last accepted generation produced {}; write to the same paths.",
                unit.stem,
                paths.join(", ")
            );
        }
    }
    out.push('\n');

    // The guidance. @lfy def/generation/main.lfy:request
    out.push_str("## Guidance\n\n");
    let _ = writeln!(
        out,
        "The target's marker `{}`, and every trait it extends, give this guidance for building \
         against the target:\n",
        entity_name(model, target.marker)
    );
    if guidance.is_empty() {
        out.push_str("(none)\n");
    }
    for criterion in guidance {
        write_criterion(&mut out, criterion);
    }
    out.push('\n');

    // The native dependencies. @lfy def/generation/main.lfy:request
    out.push_str("## Native dependencies\n\n");
    out.push_str("The generated code may require these from the target's ecosystem:\n");
    if native_dependencies.is_empty() {
        out.push_str("(none)\n");
    }
    for dependency in native_dependencies {
        match &dependency.version {
            Some(version) => {
                let _ = writeln!(
                    out,
                    "- `{}` ({}), version `{version}`",
                    dependency.identifier, dependency.ecosystem
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "- `{}` ({}), any version",
                    dependency.identifier, dependency.ecosystem
                );
            }
        }
    }
    out.push('\n');

    // Each interface. @lfy def/generation/main.lfy:request
    out.push_str("## Interfaces\n\n");
    if interfaces.is_empty() {
        out.push_str("The units of this batch depend on no unit outside it.\n\n");
    } else {
        out.push_str(
            "The units of this batch depend on the units below. Use their entities by the names their \
             outputs spell, and never edit their outputs.\n\n",
        );
    }
    for interface in interfaces {
        let dependency = &plan.units[interface.unit];
        let _ = writeln!(out, "### `{}`\n", workspace.files[dependency.file].path);
        out.push_str("Outputs:\n");
        if interface.outputs.is_empty() {
            out.push_str("- (not generated yet)\n");
        }
        for path in &interface.outputs {
            let _ = writeln!(out, "- `{path}`");
        }
        out.push_str("Entities:\n");
        // Decision: the members of a data, type, trait, or enum are written too, as
        // `interfaceOf` writes them, since a dependent relies on those as much as on the
        // declaration itself.
        for &entity in &interface.entities {
            write_interface_entity(&mut out, model, entity, "");
            for member in members_of(model, entity) {
                write_interface_entity(&mut out, model, member, "  ");
            }
        }
        out.push('\n');
    }

    // One section per unit of the batch, in order. @lfy def/generation/main.lfy:request
    out.push_str("## The units to compile\n\n");
    for &index in &batch.units {
        let unit = &plan.units[index];
        let file = &workspace.files[unit.file];
        let lowered = &program.files[unit.lowered];
        let _ = writeln!(out, "### `{}` (stem `{}`)\n", file.path, unit.stem);
        if let Some(source) = sources.get(&file.path) {
            out.push_str("Lowered source:\n\n```elfie\n");
            out.push_str(source);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        if unit.entities.is_empty() {
            out.push_str("The unit has no entities.\n\n");
        }
        // Each entity's lowered code, and then, apart from the code, its local criteria and
        // tests, each on its own line with its id. @lfy def/generation/main.lfy:request
        for &entity in &unit.entities {
            write_entity(&mut out, model, lowered, entity);
        }
        // The criteria and tests of the file's own entity belong to no declaration of it.
        // @lfy def/generation/main.lfy:request
        let own: Vec<Requirement> = lowered
            .criteria
            .iter()
            .cloned()
            .map(Requirement::Criterion)
            .chain(lowered.tests.iter().cloned().map(Requirement::Test))
            .collect();
        if !own.is_empty() {
            let _ = writeln!(out, "#### `{}` (file)\n", file.path);
            write_requirements(&mut out, &own);
        }
    }

    // Every global criterion and test, each once with its id.
    // @lfy def/generation/main.lfy:request
    out.push_str("## Global criteria and tests\n\n");
    out.push_str(
        "These hold across the whole program. A unit answers for one only where it is relevant to \
         that unit's code, and ignores it otherwise; the global review checks each one once, \
         across every output.\n\n",
    );
    if globals.is_empty() {
        out.push_str("(none)\n\n");
    } else {
        write_requirements(&mut out, globals);
        out.push('\n');
    }

    // Criteria and tests are never restated in an output.
    // @lfy def/generation/main.lfy:request
    out.push_str("## Criteria and tests in an output\n\n");
    out.push_str(
        "Never restate a criterion or a test in an output, as a comment or as documentation. An \
         output names one only by its id, in the marker of the region that answers for it.\n\n",
    );

    // The standard library. @lfy def/generation/main.lfy:request
    out.push_str("## The standard library\n\n");
    out.push_str(
        "The data and fns of the standard library that carry `builtin` are bound to what the guidance \
         names for them, and are never generated: they are only called. A member of the standard \
         library written out in full is translated where it is used.\n\n",
    );

    // The rules for kinds. @lfy def/generation/main.lfy:request
    out.push_str("## Rules for kinds\n\n");
    out.push_str(
        "- A DataDeclaration becomes a type.\n\
         - An AgentFunctionDeclaration becomes a function whose body satisfies every criterion and test.\n\
         - A FunctionDeclaration is translated statement by statement with nothing added.\n\
         - A TraitDeclaration emits nothing of its own; its members and criteria belong to each entity that carries it.\n\
         - A TypeDeclaration and an EnumDeclaration become their nearest equivalents.\n\
         - An ExternalDeclaration is bound to what the guidance says.\n\
         - A folded value is written as the literal it holds, and a prompted value, spelled \
           `Type@like(`the prompt`)`, is chosen to fit its prompt.\n\
         - A Use is satisfied by the interfaces.\n\n",
    );

    // Markers. @lfy def/generation/main.lfy:request
    out.push_str("## Markers\n\n");
    out.push_str(
        "Every emitted item and every test carries a marker naming the entity it comes from. A marker is \
         written as a line comment of the target's language reading `@lfy`, a space, the source path \
         relative to the root, a colon, and the name of the entity: its identifier, or its owner's \
         identifier, a dot, and a member's name. Never write a line number. A region that answers \
         for a criterion or a test spells its id after the entity and a `#`.\n\n",
    );
    for &index in &batch.units {
        let unit = &plan.units[index];
        let file = &workspace.files[unit.file];
        if let Some(&entity) = unit.entities.first() {
            let _ = writeln!(
                out,
                "For example: `@lfy {}:{}`.",
                file.path,
                entity_name(model, entity)
            );
        }
    }
    out.push('\n');

    // Tests. @lfy def/generation/main.lfy:request
    out.push_str("## Tests\n\n");
    out.push_str(
        "Each test becomes one test, and each criterion that can be checked mechanically becomes one \
         test, placed where the guidance says. Each of them carries a marker naming the entity it \
         comes from and, after a `#`, the id of the criterion or test it answers for.\n\n",
    );

    // A unit whose reason is requirements has code that did not change.
    // @lfy def/generation/main.lfy:request
    let restated: Vec<&Unit> = batch
        .units
        .iter()
        .map(|&index| &plan.units[index])
        .filter(|unit| unit.reason == Some(Reason::Requirements))
        .collect();
    if !restated.is_empty() {
        out.push_str(
            "The code of these units is unchanged: only the tests and the code answering for the \
             criteria and tests whose ids were added or removed need to change.\n",
        );
        for unit in restated {
            let _ = writeln!(out, "- `{}`", unit.stem);
        }
        out.push('\n');
    }

    // Existing outputs. @lfy def/generation/main.lfy:request
    if !existing.is_empty() {
        out.push_str("## Existing outputs\n\n");
        out.push_str("These outputs exist already:\n");
        for path in existing.keys() {
            let _ = writeln!(out, "- `{path}`");
        }
        let known: Vec<&String> = sources
            .keys()
            .filter(|path| previous.contains_key(*path))
            .collect();
        let unknown: Vec<&String> = sources
            .keys()
            .filter(|path| !previous.contains_key(*path))
            .collect();
        out.push('\n');
        if !known.is_empty() {
            out.push_str(
                "The previous source each output was generated from is given. Change only what the \
                 difference between the previous source and the current source requires; keep names and \
                 structure; keep the markers of unchanged items.\n",
            );
            for path in known {
                let _ = writeln!(
                    out,
                    "\nThe previous source of `{path}`:\n\n```elfie\n{}\n```",
                    previous[path].trim_end()
                );
            }
            out.push('\n');
        }
        if !unknown.is_empty() {
            out.push_str("\nThe source these were generated from is not known; reconcile the existing output with the source:\n");
            for path in unknown {
                let _ = writeln!(out, "- `{path}`");
            }
            out.push('\n');
        }
    }

    // The agent server. @lfy def/generation/main.lfy:request
    out.push_str("## The agent server\n\n");
    out.push_str(
        "The tools of the agent server may be called for anything the request leaves out.\n\n",
    );

    // How to end the report. @lfy def/generation/main.lfy:request
    out.push_str("## How to report\n\n");
    out.push_str(
        "End your report with exactly one line:\n\n\
         - `ELFIE: DONE` when every output is written.\n\
         - `ELFIE: BLOCKED: ` followed by the reason when an ambiguous criterion, two criteria in \
           conflict, a name that resolves nowhere, or anything else stops you; write no output for that \
           unit and quote the criterion.\n\
         - `ELFIE: CLARIFY: ` followed by one question when only a person can decide; write nothing.\n",
    );

    out
}

/// One entity of a unit: its name, its kind, its definition, its type, its lowered code,
/// and then, apart from the code, its local criteria and tests, each on its own line with
/// its id.
// @lfy def/generation/main.lfy:request
fn write_entity(out: &mut String, model: &Model, lowered: &LoweredFile, entity: EntityId) {
    let record = &model.entities[entity];
    let _ = writeln!(
        out,
        "#### `{}` ({})\n",
        qualified_name(model, entity),
        kind_text(record)
    );
    if let Some(definition) = &record.definition {
        let _ = writeln!(out, "Definition: {definition}\n");
    }
    if let Some(ty) = &record.ty {
        let _ = writeln!(out, "Type: `{}`\n", model::type_text(model, ty));
    }
    // @lfy def/generation/main.lfy:request
    if let Some(node) = lowered_node_of(lowered, entity) {
        out.push_str("Lowered code:\n\n```elfie\n");
        out.push_str(lowered_code(model, node).trim());
        out.push_str("\n```\n\n");
    }
    write_requirements(out, &entity_requirements(lowered, entity));
}

/// Every criterion and test of a list, criteria then tests, each on one line beginning with
/// its id: what a request and a review quote apart from the code.
// @lfy def/generation/main.lfy:request
fn write_requirements(out: &mut String, requirements: &[Requirement]) {
    let criteria: Vec<&LoweredCriterion> = requirements
        .iter()
        .filter_map(|requirement| match requirement {
            Requirement::Criterion(criterion) => Some(criterion),
            Requirement::Test(_) => None,
        })
        .collect();
    let tests: Vec<&LoweredTest> = requirements
        .iter()
        .filter_map(|requirement| match requirement {
            Requirement::Test(test) => Some(test),
            Requirement::Criterion(_) => None,
        })
        .collect();
    out.push_str("Criteria:\n");
    if criteria.is_empty() {
        out.push_str("(none)\n");
    }
    for criterion in criteria {
        let _ = writeln!(
            out,
            "- `{}` — {}",
            criterion.id,
            criterion_text(
                criterion.situation.as_ref(),
                criterion.behavior.as_ref(),
                criterion.side_effects.as_ref(),
            )
        );
    }
    out.push('\n');
    out.push_str("Tests:\n");
    if tests.is_empty() {
        out.push_str("(none)\n");
    }
    for test in tests {
        let _ = writeln!(out, "- `{}` — {}", test.id, test_text(test));
    }
    out.push('\n');
}

/// One criterion as a list item: situations, then behaviors, then side effects.
fn write_criterion(out: &mut String, criterion: &Criterion) {
    let _ = writeln!(
        out,
        "- {}",
        criterion_text(
            criterion.situation.as_ref(),
            criterion.behavior.as_ref(),
            criterion.side_effects.as_ref(),
        )
    );
}

/// One criterion as one line: `When` and its situations, then its behaviors, then
/// `Side effects:` and its side effects, joined by `: `.
// @lfy def/generation/main.lfy:review
fn criterion_text(
    situation: Option<&Vec<String>>,
    behavior: Option<&Vec<String>>,
    side_effects: Option<&Vec<String>>,
) -> String {
    let mut parts = Vec::new();
    if let Some(situation) = situation {
        parts.push(format!("When {}", situation.join(" ")));
    }
    if let Some(behavior) = behavior {
        parts.push(behavior.join(" "));
    }
    if let Some(side_effects) = side_effects {
        parts.push(format!("Side effects: {}", side_effects.join(" ")));
    }
    parts.join(": ")
}

/// One test as one line: its input and its expectation, as spelled.
// @lfy def/generation/main.lfy:review
fn test_text(test: &LoweredTest) -> String {
    format!(
        "Input `{}` gives `{}`",
        test.input_text.trim(),
        test.expect_text.trim()
    )
}

/// The identifier of an entity, or `anonymous`.
fn entity_name(model: &Model, entity: EntityId) -> String {
    model.entities[entity]
        .identifier
        .clone()
        .unwrap_or_else(|| "anonymous".to_string())
}

/// The name an entity is named by, as a marker and a change spell it: the identifier, or
/// the owner's identifier, a dot, and a member's name.
// @lfy def/generation/main.lfy:review
fn qualified_name(model: &Model, entity: EntityId) -> String {
    if let Some(file) = model.entities[entity].file
        && let Some((name, _)) = named_entities(model, file, &[entity])
            .into_iter()
            .find(|&(_, candidate)| candidate == entity)
    {
        return name;
    }
    entity_name(model, entity)
}

/// The kind of an entity, as the grammar names its declaration.
fn kind_text(entity: &Entity) -> &'static str {
    match &entity.kind {
        EntityKind::Data => "data: DataDeclaration",
        EntityKind::Fn { agent: true, .. } => "agent function: AgentFunctionDeclaration",
        EntityKind::Fn { agent: false, .. } => "function: FunctionDeclaration",
        EntityKind::Trait { .. } => "trait: TraitDeclaration",
        EntityKind::Type => "type: TypeDeclaration",
        EntityKind::Enum => "enum: EnumDeclaration",
        EntityKind::Variable => "variable: VariableDeclaration",
        EntityKind::Alias => "alias: AliasDeclaration",
        EntityKind::External => "external: ExternalDeclaration",
        EntityKind::Module => "module: Use",
        EntityKind::File => "file",
        EntityKind::Global => "global",
        EntityKind::LoopVariable => "loop variable",
        EntityKind::Parameter => "parameter",
        EntityKind::Member => "member",
        EntityKind::EnumMember => "enum member",
        EntityKind::Anonymous => "anonymous scope",
    }
}

// ---------------------------------------------------------------------------------------
// outcomeOf
// ---------------------------------------------------------------------------------------

/// How one run of the compiler ended, read from its report and the verdicts of its units.
///
/// The last line of the report beginning with `ELFIE:` decides first: `ELFIE: BLOCKED:`
/// gives blocked and `ELFIE: CLARIFY:` gives clarification, each with the rest of that
/// line and every line after it as the message. With no such line and no verdicts the run
/// failed; with a rejected verdict it was rejected, with the first problem of the first
/// rejected verdict as the message; otherwise it was accepted, with an empty message — a
/// report that ended and left no verdict is accepted, since the compiler said it was done.
// @lfy def/generation/main.lfy:outcomeOf
pub fn outcome_of(report: &str, verdicts: Vec<Verdict>) -> Outcome {
    const BLOCKED: &str = "ELFIE: BLOCKED:";
    const CLARIFY: &str = "ELFIE: CLARIFY:";

    let lines: Vec<&str> = report.lines().collect();
    let last = lines
        .iter()
        .rposition(|line| line.trim_start().starts_with("ELFIE:"));
    if let Some(index) = last {
        let line = lines[index].trim_start();
        let prefix = if line.starts_with(BLOCKED) {
            Some((BLOCKED, OutcomeKind::Blocked)) // @lfy def/generation/main.lfy:outcomeOf
        } else if line.starts_with(CLARIFY) {
            Some((CLARIFY, OutcomeKind::Clarification)) // @lfy def/generation/main.lfy:outcomeOf
        } else {
            None
        };
        if let Some((prefix, kind)) = prefix {
            let mut message = line[prefix.len()..].trim_start().to_string();
            for line in &lines[index + 1..] {
                message.push('\n');
                message.push_str(line);
            }
            return Outcome {
                kind,
                message: message.trim_end().to_string(),
                verdicts,
            }; // @lfy def/generation/main.lfy:outcomeOf
        }
    }
    if verdicts.is_empty() && last.is_none() {
        // @lfy def/generation/main.lfy:outcomeOf
        return Outcome {
            kind: OutcomeKind::Failed,
            message: "the compiler reported nothing".to_string(),
            verdicts,
        };
    }
    // @lfy def/generation/main.lfy:outcomeOf
    if let Some(rejected) = verdicts.iter().find(|verdict| !verdict.accepted) {
        let message = rejected.problems.first().cloned().unwrap_or_default();
        return Outcome {
            kind: OutcomeKind::Rejected,
            message,
            verdicts,
        };
    }
    // @lfy def/generation/main.lfy:outcomeOf
    Outcome {
        kind: OutcomeKind::Accepted,
        message: String::new(),
        verdicts,
    }
}

// ---------------------------------------------------------------------------------------
// accept
// ---------------------------------------------------------------------------------------

/// Whether outputs satisfy a request for one unit structurally, the source maps they
/// earn, and the outputs with their markers normalized.
///
/// Rejected when there are no outputs, when an output's path is not under the target's
/// output directory, when a marker names an entity that its file does not declare, a line
/// of the unit's file beyond its last, or — naming the unit's own file — a requirement id
/// that is neither a local criterion or test of the unit nor a global one, or when an entity
/// of the unit has no marker naming it or a line its declaration covers. A marker naming a
/// line that falls inside a declaration is rewritten to name that entity, since a name
/// survives edits and a line does not. A marker naming a file that is not in the program is
/// ignored and not recorded: it is a fixture or prose, not a claim about the program.
/// Otherwise accepted, with one source map per output, each marker carrying the end of the
/// region it begins.
/// Whether an output builds or its tests pass is not checked here; the guidance says how,
/// and the caller runs it.
// Decision: the definition passes the request and the unit; a plan here does not own its
// workspace or the program lowered from it, so the program and the plan are extra first
// parameters and the unit is given as its index into the plan.
// @lfy def/generation/main.lfy:accept
pub fn accept(
    program: &Program,
    plan: &Plan,
    request: &Request,
    unit: usize,
    outputs: &[Output],
) -> Verdict {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let planned = &plan.units[unit];
    let target = &workspace.targets[planned.target];
    let file = &workspace.files[planned.file];
    let mut problems: Vec<String> = Vec::new();

    if outputs.is_empty() {
        // @lfy def/generation/main.lfy:accept
        problems.push(format!("no output was produced for {}", file.path));
    }

    // Decision: a marker names a line of the source file, so the last line is counted from
    // the file's own text; the request carries the lowered text, whose lines are not its.
    let last_line = source_text(workspace, file).lines().count().max(1);
    let named = named_entities(model, file.source, &planned.entities);
    // Every requirement id a marker naming this unit's own file may spell: a local criterion
    // or test of this unit, or a global one of the request. An output is often shared with
    // another unit, which writes its own markers and its own ids into it, so only a marker
    // naming this unit's file is a claim this unit makes and only its id is checked here.
    // @lfy def/generation/main.lfy:accept
    let global_ids: Vec<&str> = request.globals.iter().map(Requirement::id).collect();
    let local_ids: Vec<String> = local_requirements(&program.files[planned.lowered])
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();

    // Each marker as the compiler wrote it, beside the one the model resolved it to: the
    // first says where the marker sits in the text, the second is what is recorded.
    // @lfy def/generation/main.lfy:accept
    let mut normalized: Vec<Vec<(Marker, Marker)>> = Vec::new();
    for output in outputs {
        if !under(&output.path, &target.output_directory) {
            // @lfy def/generation/main.lfy:accept
            problems.push(format!(
                "the output {} is not under the output directory {} of the target {}",
                output.path, target.output_directory, target.identifier
            ));
        }
        let mut markers = Vec::new();
        // [`parse_markers`] derives each marker's end as the line before the output line
        // of the next marker in the same output, or the last line of the output for the
        // last one, so the regions of an output partition it from its first marker on.
        // @lfy def/generation/main.lfy:accept
        for marker in parse_markers(&output.text) {
            // A marker naming any other file — another file of the program, whose unit owns
            // the ids it spells, or a file outside the program, which is a fixture or prose
            // claiming nothing about it — spells no id this unit answers for, so none of
            // those is checked.
            // @lfy def/generation/main.lfy:accept#accept:accept:45ec36176d2c984237e6519d40944832cbc76e75c44d4a6e0ff356725704fe84
            if let Some(requirement) = &marker.requirement
                && marker.file == file.path
                && !local_ids.iter().any(|id| id == requirement)
                && !global_ids.iter().any(|&id| id == requirement)
            {
                problems.push(format!(
                    "{}:{}: the marker answers for {requirement}, which is no local criterion or \
                     test of {} and no global one",
                    output.path, marker.output_line, file.path
                ));
            }
            // @lfy def/generation/main.lfy:accept
            if let Some(resolved) = resolve_marker(
                workspace,
                file,
                &named,
                &marker,
                last_line,
                output,
                &mut problems,
            ) {
                markers.push((marker, resolved));
            }
        }
        normalized.push(markers);
    }

    // @lfy def/generation/main.lfy:accept
    for &entity in &planned.entities {
        let Some((first, last)) = declaration_lines(model, entity) else {
            continue;
        };
        let covered = normalized.iter().flatten().any(|(_, marker)| {
            marker.file == file.path && (first..=last).contains(&marker.line)
        });
        if !covered {
            problems.push(format!(
                "the entity {} of {} (lines {first} to {last}) has no marker in any output",
                entity_name(model, entity),
                file.path
            ));
        }
    }

    if !problems.is_empty() {
        return Verdict {
            accepted: false,
            problems,
            source_maps: Vec::new(),
            outputs: Vec::new(),
        };
    }

    // Nothing of a source map comes from the compiler's text except the names it spelled.
    // @lfy def/generation/main.lfy:accept#accept:accept:6a1ede84efd02401a68ed3e9a5852d6dfc704e582532c620de68595d4fe74150
    let hash = source_hash(&program.files[planned.lowered].text);
    let requirements = requirements_hash(program, planned);
    let signature = interface_signature(workspace, planned);
    let dependencies: BTreeMap<String, String> = planned
        .dependencies
        .iter()
        .map(|&dependency| {
            let dependency = &plan.units[dependency];
            (
                workspace.files[dependency.file].path.clone(),
                interface_signature(workspace, dependency),
            )
        })
        .collect();
    let generated = now_rfc3339();
    let mut source_maps = Vec::new();
    let mut written = Vec::new();
    for (output, markers) in outputs.iter().zip(&normalized) {
        source_maps.push(SourceMap {
            target: target.identifier.clone(),
            output: output.path.clone(),
            source: file.path.clone(),
            hash: hash.clone(),
            requirements: requirements.clone(),
            signature: signature.clone(),
            dependencies: dependencies.clone(),
            generated: generated.clone(),
            // @lfy def/generation/main.lfy:accept
            markers: markers.iter().map(|(_, marker)| marker.clone()).collect(),
        });
        written.push(rewrite_markers(output, markers));
    }
    Verdict {
        accepted: true,
        problems: Vec::new(),
        source_maps,
        outputs: written,
    }
}

/// One marker with its line derived from the model and, where it named a line inside a
/// declaration, the name of that entity; problems are pushed for a marker that resolves
/// nowhere, and `None` is given for one that names a file outside the program or a line no
/// declaration covers.
///
/// A number the compiler wrote is only where to look: whatever the marker was spelled as,
/// the line kept is the first line of the declaration it resolved to.
// @lfy def/generation/main.lfy:accept
fn resolve_marker(
    workspace: &Workspace,
    file: &File,
    named: &[(String, EntityId)],
    marker: &Marker,
    last_line: usize,
    output: &Output,
    problems: &mut Vec<String>,
) -> Option<Marker> {
    let model = &workspace.model;
    // Decision: an output may carry markers for other files of the program (one Rust file
    // often mirrors several definition files). @lfy def/generation/main.lfy:accept
    let Some(source) = workspace.file(&marker.file).map(|named| named.source) else {
        // A marker naming a file outside the program is a fixture or prose, not a claim
        // about the program, so it is ignored and not recorded.
        // @lfy def/generation/main.lfy:accept
        return None;
    };
    // The entities the marker's file declares: the unit's own table for its own file, since
    // that one holds the entities the unit builds as well.
    let own = named_entities(model, source, &[]);
    let table = if marker.file == file.path {
        named
    } else {
        &own
    };
    // A marker that names an entity keeps its spelling; its line is derived.
    if let Some(name) = &marker.entity {
        match table.iter().find(|(candidate, _)| candidate == name) {
            Some(&(_, entity)) => {
                return Some(Marker {
                    line: declaration_line(model, entity).unwrap_or(1),
                    ..marker.clone()
                });
            }
            None => {
                // @lfy def/generation/main.lfy:accept
                problems.push(format!(
                    "{}:{}: the marker names {}, which {} does not declare",
                    output.path, marker.output_line, name, marker.file
                ));
                return Some(marker.clone());
            }
        }
    }
    if marker.file == file.path && (marker.line == 0 || marker.line > last_line) {
        // @lfy def/generation/main.lfy:accept
        problems.push(format!(
            "{}:{}: the marker names line {} of {}, whose last line is {last_line}",
            output.path, marker.output_line, marker.line, file.path
        ));
        return Some(marker.clone());
    }
    // A marker that names a line names the innermost declaration covering it, and the line
    // kept is that declaration's first, from the model: the number the compiler wrote said
    // where to look and is not itself recorded. A line no declaration covers names nothing
    // of the model, so there is no line to derive from it and it is not recorded.
    // @lfy def/generation/main.lfy:accept
    let name = innermost(model, table, marker.line)?;
    let &(_, entity) = table.iter().find(|(candidate, _)| *candidate == name)?;
    Some(Marker {
        entity: Some(name),
        column: None,
        line: declaration_line(model, entity)?,
        ..marker.clone()
    })
}

/// The name of the innermost entity whose declaration covers a line.
// @lfy def/generation/main.lfy:accept
fn innermost(model: &Model, named: &[(String, EntityId)], line: usize) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for (name, entity) in named {
        let Some((first, last)) = declaration_lines(model, *entity) else {
            continue;
        };
        if !(first..=last).contains(&line) {
            continue;
        }
        let width = last - first;
        if best.is_none_or(|(widest, _)| width <= widest) {
            best = Some((width, name));
        }
    }
    best.map(|(_, name)| name.to_string())
}

/// Every entity of a file a marker may name, with the name it is named by: each
/// declaration of the file scope and each entity the unit builds, then the members each
/// of them declares as `owner.member`.
// @lfy def/generation/main.lfy:accept
fn named_entities(model: &Model, source: FileId, extra: &[EntityId]) -> Vec<(String, EntityId)> {
    let mut declarations: Vec<EntityId> = Vec::new();
    if let Some(&scope) = model.file_scopes.get(source) {
        for &symbol in &model.scopes[scope].symbols {
            let symbol = &model.symbols[symbol];
            let entity = &model.entities[symbol.entity];
            if symbol.kind != SymbolKind::Module
                && entity.file == Some(source)
                && entity.node.is_some()
                && !declarations.contains(&symbol.entity)
            {
                declarations.push(symbol.entity);
            }
        }
    }
    for &entity in extra {
        if model.entities[entity].node.is_some() && !declarations.contains(&entity) {
            declarations.push(entity);
        }
    }
    let mut out: Vec<(String, EntityId)> = Vec::new();
    for entity in declarations {
        let Some(identifier) = model.entities[entity].identifier.clone() else {
            continue;
        };
        for member in members_of(model, entity) {
            let record = &model.entities[member];
            if let Some(name) = &record.identifier
                && record.node.is_some()
            {
                out.push((format!("{identifier}.{name}"), member));
            }
        }
        out.push((identifier, entity));
    }
    out
}

/// An output with every marker that was rewritten spelled as its name, so that the markers
/// kept are names and the lines recorded beside them are the model's.
///
/// Each is given as the compiler wrote it beside what it resolved to, and is found in the
/// text by the first: a spelling rebuilt from the line would be the model's line, which is
/// not what the compiler wrote and need not be found there at all.
// @lfy def/generation/main.lfy:accept
fn rewrite_markers(output: &Output, markers: &[(Marker, Marker)]) -> Output {
    let rewritten: Vec<&(Marker, Marker)> = markers
        .iter()
        .filter(|(_, marker)| marker.entity.is_some() && marker.column.is_none())
        .collect();
    if rewritten.is_empty() {
        return output.clone();
    }
    let mut lines: Vec<String> = output.text.lines().map(str::to_string).collect();
    let mut changed = false;
    for (as_written, marker) in rewritten {
        let Some(line) = lines.get_mut(marker.output_line.wrapping_sub(1)) else {
            continue;
        };
        let written = marker.spelling();
        let spelling = as_written.spelling();
        if spelling != written && line.contains(&spelling) {
            *line = line.replacen(&spelling, &written, 1);
            changed = true;
        }
    }
    if !changed {
        return output.clone();
    }
    let mut text = lines.join("\n");
    if output.text.ends_with('\n') {
        text.push('\n');
    }
    Output {
        path: output.path.clone(),
        text,
    }
}

/// The first line of an entity's declaring node; `None` for an entity with no node.
// @lfy def/generation/main.lfy:accept
fn declaration_line(model: &Model, entity: EntityId) -> Option<usize> {
    let node = model.entities[entity].node?;
    let info = model.info(node);
    Some(model.tokens(node.file).get(info.start)?.line)
}

/// The first and last source line an entity's declaration covers, documentation
/// included; `None` for an entity with no node.
// @lfy def/generation/main.lfy:accept
fn declaration_lines(model: &Model, entity: EntityId) -> Option<(usize, usize)> {
    let node = model.entities[entity].node?;
    let tokens = model.tokens(node.file);
    let info = model.info(node);
    let declaration = model.node(node);
    // Decision: a marker naming a line of the declaration's documentation counts as
    // naming the declaration, since the documentation belongs to it.
    let start = declaration
        .documentation
        .first()
        .map_or(info.start, |documentation| {
            documentation.start.min(info.start)
        });
    let first = tokens.get(start)?.line;
    let last = if info.end > info.start {
        let token = &tokens[info.end - 1];
        token.line + token.raw.matches('\n').count()
    } else {
        first
    };
    Some((first, last.max(first)))
}

// ---------------------------------------------------------------------------------------
// sourceMapsOf, record
// ---------------------------------------------------------------------------------------

/// Every source map recorded for a workspace whose output still exists.
///
/// A unit's map file is `elfie-compile/maps` under the workspace root, then the target's
/// identifier, then the unit's stem with the extension `.json`, such as
/// `elfie-compile/maps/rust/cli/main.json` for `def/cli/main.lfy`; it holds the unit's
/// source maps as one JSON list. The maps come in the order of the workspace's targets,
/// then in the sorted order of the map files under each target's folder, then in the order
/// each file lists them. `target` is the identifier of one target, or `None` for every
/// target; when it is set, the map files of other targets are never opened. A target with
/// no folder under `elfie-compile/maps` is read from `source-map.json` under its output
/// directory instead, as earlier versions recorded every map of a target, so a project
/// keeps its maps until the next compile moves them. A map file that cannot be read, or is
/// not a JSON list of source maps, contributes nothing, so its unit plans as fresh.
/// Nothing is written, and nothing under `elfie-compile/cache` is read.
// @lfy def/generation/main.lfy:sourceMapsOf
pub fn source_maps_of(workspace: &Workspace, target: Option<&str>) -> Vec<SourceMap> {
    let mut out: Vec<SourceMap> = Vec::new();
    for named in &workspace.targets {
        // @lfy def/generation/main.lfy:sourceMapsOf
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        let folder = workspace.root.join(MAPS).join(&named.identifier);
        let files = if folder.is_dir() {
            map_files(&folder) // @lfy def/generation/main.lfy:sourceMapsOf
        } else {
            // @lfy def/generation/main.lfy:sourceMapsOf
            let legacy = workspace
                .root
                .join(&named.output_directory)
                .join(LEGACY_MAPS);
            if legacy.is_file() {
                vec![legacy]
            } else {
                Vec::new() // @lfy def/generation/main.lfy:sourceMapsOf
            }
        };
        for file in files {
            // @lfy def/generation/main.lfy:sourceMapsOf
            for map in read_source_maps(&file) {
                // A map whose output is gone says nothing about a unit that is up to date.
                // @lfy def/generation/main.lfy:sourceMapsOf
                if workspace.root.join(&map.output).exists() {
                    out.push(map);
                }
            }
        }
    }
    out
}

/// Every `.json` file under a target's folder of map files, sorted by path, a directory
/// walked before the files that follow it.
// Decision: a unit's stem holds the directories it sits in, so its map file does too and
// the folder is walked; the order within one directory is the sorted order of the names,
// as the loader lists source files.
// @lfy def/generation/main.lfy:sourceMapsOf
fn map_files(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
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
    let mut out: Vec<PathBuf> = Vec::new();
    for (name, is_directory) in names {
        let path = folder.join(&name);
        if is_directory {
            out.extend(map_files(&path));
        } else if name.ends_with(".json") {
            out.push(path);
        }
    }
    out
}

/// The map file of one unit: `elfie-compile/maps`, the target's identifier, and the unit's
/// stem with the extension `.json`, under the workspace root.
// @lfy def/generation/main.lfy:sourceMapsOf
pub fn map_file(workspace: &Workspace, unit: &Unit) -> PathBuf {
    workspace
        .root
        .join(MAPS)
        .join(&workspace.targets[unit.target].identifier)
        .join(format!("{}.json", unit.stem))
}

/// Whether the source maps of one accepted unit are recorded in its map file.
///
/// The file holds them afterwards, replacing whatever it held, and no other file is
/// written, so recording one unit leaves every other map file byte for byte as it was. It
/// is the pretty JSON of the maps in output order, each map's markers in output order and
/// the keys of every object in alphabetical order, so the same maps always give the same
/// bytes and a diff shows only what changed. Maps that differ from what the file holds only
/// in when they were generated leave the file as it is, so accepting outputs that did not
/// change changes nothing on disk. Every missing folder above the file is created, so a
/// project cloned without `elfie-compile` records its maps; no source map at all removes
/// the file, so the unit plans as fresh. `false` when the file cannot be written.
// @lfy def/generation/main.lfy:record
pub fn record(workspace: &Workspace, unit: &Unit, source_maps: &[SourceMap]) -> bool {
    let path = map_file(workspace, unit);
    if source_maps.is_empty() {
        // @lfy def/generation/main.lfy:record
        return match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
    }
    let mut ordered: Vec<SourceMap> = source_maps.to_vec();
    ordered.sort_by(|left, right| left.output.cmp(&right.output));
    // @lfy def/generation/main.lfy:record
    let value = serde_json::Value::Array(ordered.iter().map(SourceMap::to_json).collect());
    let Ok(mut text) = serde_json::to_string_pretty(&value) else {
        return false;
    };
    text.push('\n');
    // Maps that differ only in when they were generated are the maps the file holds.
    // @lfy def/generation/main.lfy:record
    let recorded = read_source_maps(&path);
    if recorded.len() == ordered.len()
        && recorded.iter().zip(&ordered).all(|(held, map)| {
            *held
                == SourceMap {
                    generated: held.generated.clone(),
                    ..map.clone()
                }
        })
    {
        return true;
    }
    // @lfy def/generation/main.lfy:record
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    // @lfy def/generation/main.lfy:record
    std::fs::write(&path, text).is_ok()
}

// ---------------------------------------------------------------------------------------
// regionsOf
// ---------------------------------------------------------------------------------------

/// Every region of generated output that came from one entity.
///
/// `name` is an identifier, or an owner's identifier, a dot, and a member's name; `target`
/// is the identifier of one target, or `None` for every target. The markers come source map
/// by source map in the order they are given, and within one in the order of its markers,
/// which is output order. A marker whose entity is `name`, a dot, and a member's name comes
/// too, since a member's region is part of its owner's code, and a marker that names a line
/// rather than an entity comes when its file declares the entity named `name` and the line
/// falls inside its declaring node. Nothing matching gives an empty list.
// @lfy def/generation/main.lfy:regionsOf
pub fn regions_of(
    workspace: &Workspace,
    source_maps: &[SourceMap],
    name: &str,
    target: Option<&str>,
) -> Vec<Marker> {
    // @lfy def/generation/main.lfy:regionsOf
    matching_regions(workspace, source_maps, name, target)
        .into_iter()
        .map(|(_, marker)| marker.clone())
        .collect()
}

/// Every region one name owns, each with the source map that holds it, so that a caller
/// that needs the output path has it; [`regions_of`] is this without the source maps.
// @lfy def/generation/main.lfy:regionsOf
fn matching_regions<'m>(
    workspace: &Workspace,
    source_maps: &'m [SourceMap],
    name: &str,
    target: Option<&str>,
) -> Vec<(&'m SourceMap, &'m Marker)> {
    let model = &workspace.model;
    // A member's region is part of its owner's code, so `A` owns `A.x` too.
    let member = format!("{name}.");
    let mut out: Vec<(&SourceMap, &Marker)> = Vec::new();
    for map in source_maps {
        // @lfy def/generation/main.lfy:regionsOf
        if target.is_some_and(|target| map.target != target) {
            continue;
        }
        for marker in &map.markers {
            let matched = match &marker.entity {
                // @lfy def/generation/main.lfy:regionsOf
                Some(entity) => entity == name || entity.starts_with(&member),
                // A marker that names a line belongs to the entity whose declaration
                // covers it. @lfy def/generation/main.lfy:regionsOf
                None => covers_named(model, &marker.file, name, marker.line),
            };
            if matched {
                out.push((map, marker));
            }
        }
    }
    out
}

/// Whether a file of the program declares an entity of this name whose declaring node
/// covers a line.
// @lfy def/generation/main.lfy:regionsOf
fn covers_named(model: &Model, path: &str, name: &str, line: usize) -> bool {
    let Some(source) = model.file(path) else {
        return false;
    };
    named_entities(model, source, &[])
        .iter()
        .filter(|(candidate, _)| candidate == name)
        .any(|&(_, entity)| {
            declaration_lines(model, entity)
                .is_some_and(|(first, last)| (first..=last).contains(&line))
        })
}

// ---------------------------------------------------------------------------------------
// changes
// ---------------------------------------------------------------------------------------

/// What a change of kind added says, and what one of kind removed says.
const ADDED: &str = "it is declared now and was not in the file the outputs were generated from";
const REMOVED: &str = "it was declared in the file the outputs were generated from and is not now";
/// How long a text may be for a detail to quote it rather than name it.
// @lfy def/generation/main.lfy:changes
const DETAIL_LIMIT: usize = 80;

/// What differs for each entity of a unit between its file as last accepted and as it is
/// now.
///
/// `previous` is the text of the unit's file when its outputs were last accepted, and
/// `None` when they never were, which makes every entity of the unit an addition. The file
/// as it was is bound through [`change`](crate::workspace::change) and its entities are
/// compared with the unit's by name — the identifier, or the owner's identifier, a dot, and
/// a member's name — so a reformatted or reordered file whose entities read the same gives
/// no change at all.
// Decision: a change is found by binding the previous text through the loader and comparing
// entities, never by diffing lines.
// @lfy def/generation/main.lfy:changes
pub fn changes(program: &Program, unit: &Unit, previous: Option<&str>) -> Vec<Change> {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let path = workspace.files[unit.file].path.clone();
    let now = entity_table(model, &unit.entities);
    let Some(previous) = previous else {
        // @lfy def/generation/main.lfy:changes
        return unit
            .entities
            .iter()
            .map(|&entity| Change {
                entity: entity_name(model, entity),
                kind: ChangeKind::Added,
                detail: ADDED.to_string(),
            })
            .collect();
    };
    // The file as it was is bound through the loader and lowered, so that its entities and
    // their requirement ids are read the same way as the unit's own.
    // @lfy def/generation/main.lfy:changes#changes:changes:7866c7528c3621e91492d3baf344ace83e164319b69106af0733471ab759f3a5
    let before = crate::interpret::lower(crate::workspace::change(
        workspace,
        &path,
        Some(previous),
    ));
    let was = entity_table(
        &before.workspace.model,
        &previous_entities(&before, &workspace.targets[unit.target].identifier, &path),
    );
    let lowered = &program.files[unit.lowered];
    let lowered_before = before
        .workspace
        .files
        .iter()
        .position(|file| file.path == path)
        .map(|index| &before.files[index]);

    // File order: the order of the unit's entities, with a removed entity after the last
    // entity that preceded it before and is still declared, or first when none is.
    // @lfy def/generation/main.lfy:changes#changes:changes:3da24f82f28d461099bd3d405b1cb83ee6ea222a16f0a5a4ead591257085c4f7
    let mut anchor: Option<String> = None;
    let mut removed: Vec<(Option<String>, String)> = Vec::new();
    for (name, _) in &was {
        if now.iter().any(|(candidate, _)| candidate == name) {
            anchor = Some(name.clone());
        } else {
            removed.push((anchor.clone(), name.clone()));
        }
    }
    // An entity no still-declared entity preceded comes first.
    // @lfy def/generation/main.lfy:changes#changes:changes:8bf838e57cd210c5a27390714d305ae478381b305687353e2e12088e09cb7b10
    let mut order: Vec<String> = removed
        .iter()
        .filter(|(place, _)| place.is_none())
        .map(|(_, name)| name.clone())
        .collect();
    for (name, _) in &now {
        order.push(name.clone());
        for (place, name) in removed
            .iter()
            .filter(|(place, _)| place.as_deref() == Some(name.as_str()))
        {
            let _ = place;
            order.push(name.clone());
        }
    }

    let mut out: Vec<Change> = Vec::new();
    for name in order {
        let new = now
            .iter()
            .find(|(candidate, _)| candidate == &name)
            .map(|&(_, entity)| entity);
        let old = was
            .iter()
            .find(|(candidate, _)| candidate == &name)
            .map(|&(_, entity)| entity);
        match (old, new) {
            // @lfy def/generation/main.lfy:changes
            (None, Some(_)) => out.push(Change {
                entity: name,
                kind: ChangeKind::Added,
                detail: ADDED.to_string(),
            }),
            // @lfy def/generation/main.lfy:changes
            (Some(_), None) => out.push(Change {
                entity: name,
                kind: ChangeKind::Removed,
                detail: REMOVED.to_string(),
            }),
            // @lfy def/generation/main.lfy:changes
            (Some(old), Some(new)) => {
                let (old_criteria, old_tests) = match lowered_before {
                    Some(file) => requirement_ids(file, old),
                    None => (Vec::new(), Vec::new()),
                };
                let (new_criteria, new_tests) = requirement_ids(lowered, new);
                out.extend(differences(
                    &name,
                    &Side {
                        model: &before.workspace.model,
                        entity: old,
                        criteria: old_criteria,
                        tests: old_tests,
                    },
                    &Side {
                        model,
                        entity: new,
                        criteria: new_criteria,
                        tests: new_tests,
                    },
                ));
            }
            (None, None) => {}
        }
    }
    out
}

/// One side of a comparison: the model an entity was bound in, the entity, and the ids of
/// its local criteria and of its local tests, in order.
// @lfy def/generation/main.lfy:changes
struct Side<'m> {
    model: &'m Model,
    entity: EntityId,
    criteria: Vec<String>,
    tests: Vec<String>,
}

/// The ids of an entity's local criteria and of its local tests, in order.
// @lfy def/generation/main.lfy:changes
fn requirement_ids(file: &LoweredFile, entity: EntityId) -> (Vec<String>, Vec<String>) {
    let requirements = entity_requirements(file, entity);
    let criteria = requirements
        .iter()
        .filter_map(|requirement| match requirement {
            Requirement::Criterion(criterion) => Some(criterion.id.clone()),
            Requirement::Test(_) => None,
        })
        .collect();
    let tests = requirements
        .iter()
        .filter_map(|requirement| match requirement {
            Requirement::Test(test) => Some(test.id.clone()),
            Requirement::Criterion(_) => None,
        })
        .collect();
    (criteria, tests)
}

/// Every entity of a unit and every member it declares, in file order, each with the name
/// it is compared by: the identifier, or the owner's identifier, a dot, and the member's
/// name.
// @lfy def/generation/main.lfy:changes
fn entity_table(model: &Model, entities: &[EntityId]) -> Vec<(String, EntityId)> {
    let mut out: Vec<(String, EntityId)> = Vec::new();
    let push = |name: String, entity: EntityId, out: &mut Vec<(String, EntityId)>| {
        if !out.iter().any(|(candidate, _)| candidate == &name) {
            out.push((name, entity));
        }
    };
    for &entity in entities {
        let name = entity_name(model, entity);
        // A member is compared as an entity of its own, under its owner, so the owner
        // comes first and its members follow it.
        push(name.clone(), entity, &mut out);
        for member in members_of(model, entity) {
            if let Some(member_name) = &model.entities[member].identifier {
                push(format!("{name}.{member_name}"), member, &mut out);
            }
        }
    }
    out
}

/// The entities the file at a path gives for a target in a workspace bound from the
/// previous text; none when the file or the target is not in it.
// @lfy def/generation/main.lfy:changes
fn previous_entities(before: &Program, target: &str, path: &str) -> Vec<EntityId> {
    let workspace = &before.workspace;
    let Some(index) = workspace.files.iter().position(|file| file.path == path) else {
        return Vec::new();
    };
    let Some(target) = workspace
        .targets
        .iter()
        .find(|candidate| candidate.identifier == target)
    else {
        return Vec::new();
    };
    let lowered = lowered_entities(&before.files[index]);
    built_entities(
        &workspace.model,
        workspace.files[index].source,
        target.marker,
    )
    .into_iter()
    .filter(|entity| lowered.contains(entity))
    .collect()
}

/// One change per kind that differs for a name declared on both sides, in the order
/// [`ChangeKind`] declares them.
// @lfy def/generation/main.lfy:changes
fn differences(name: &str, old: &Side<'_>, new: &Side<'_>) -> Vec<Change> {
    let (was, now) = (old.model, new.model);
    let mut out: Vec<Change> = Vec::new();

    // @lfy def/generation/main.lfy:changes
    let old_definition = was.entities[old.entity]
        .definition
        .clone()
        .unwrap_or_default();
    let new_definition = now.entities[new.entity]
        .definition
        .clone()
        .unwrap_or_default();
    if old_definition != new_definition {
        out.push(Change {
            entity: name.to_string(),
            kind: ChangeKind::Definition,
            detail: detail("the definition", &old_definition, &new_definition),
        });
    }

    // @lfy def/generation/main.lfy:changes
    let old_type = type_spelling(was, old.entity);
    let new_type = type_spelling(now, new.entity);
    if old_type != new_type {
        out.push(Change {
            entity: name.to_string(),
            kind: ChangeKind::DeclaredType,
            detail: detail("the type", &old_type, &new_type),
        });
    }

    // @lfy def/generation/main.lfy:changes
    let old_parameters = parameter_spellings(was, old.entity);
    let new_parameters = parameter_spellings(now, new.entity);
    let old_output = output_spelling(was, old.entity);
    let new_output = output_spelling(now, new.entity);
    if old_parameters != new_parameters || old_output != new_output {
        out.push(Change {
            entity: name.to_string(),
            kind: ChangeKind::Signature,
            detail: signature_detail(
                &old_parameters,
                &new_parameters,
                &old_output,
                &new_output,
            ),
        });
    }

    // The ids of the local criteria, never their texts: a criterion whose text is the same
    // has the same id, and one that was reworded has another. @lfy def/generation/main.lfy:changes
    if old.criteria != new.criteria {
        out.push(Change {
            entity: name.to_string(),
            kind: ChangeKind::Criteria,
            detail: id_detail("criteria", "criterion", &old.criteria, &new.criteria),
        });
    }

    // @lfy def/generation/main.lfy:changes
    if old.tests != new.tests {
        out.push(Change {
            entity: name.to_string(),
            kind: ChangeKind::Tests,
            detail: id_detail("tests", "test", &old.tests, &new.tests),
        });
    }

    // @lfy def/generation/main.lfy:changes
    let old_body = body_text(was, old.entity);
    let new_body = body_text(now, new.entity);
    if old_body != new_body {
        out.push(Change {
            entity: name.to_string(),
            kind: ChangeKind::Body,
            detail: detail("the body", &old_body, &new_body),
        });
    }

    out
}

/// One line saying what differs: the old and the new quoted when each is short, and their
/// lengths named otherwise.
// @lfy def/generation/main.lfy:changes#changes:changes:e44d4ec0e8b50ac4324b8435669db74cc0bb0e0e5eae175b424fd5a527f1065a
fn detail(what: &str, old: &str, new: &str) -> String {
    let (before, after) = (old.chars().count(), new.chars().count());
    if before < DETAIL_LIMIT && after < DETAIL_LIMIT {
        format!("{what} was `{old}` and is now `{new}`")
    } else {
        format!("{what} differs: {before} characters before and {after} now")
    }
}

/// One line naming which parameter or output differs.
// @lfy def/generation/main.lfy:changes
fn signature_detail(
    old: &[(String, String)],
    new: &[(String, String)],
    old_output: &str,
    new_output: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (name, ty) in new {
        match old.iter().find(|(candidate, _)| candidate == name) {
            None => parts.push(format!("the parameter {name} was added")),
            Some((_, was)) if was != ty => {
                parts.push(detail(&format!("the parameter {name}"), was, ty));
            }
            Some(_) => {}
        }
    }
    for (name, _) in old {
        if !new.iter().any(|(candidate, _)| candidate == name) {
            parts.push(format!("the parameter {name} was removed"));
        }
    }
    if parts.is_empty() && old.len() == new.len() {
        let names: Vec<&str> = new.iter().map(|(name, _)| name.as_str()).collect();
        parts.push(format!("the parameters are in another order: {}", names.join(", ")));
    }
    if old_output != new_output {
        parts.push(detail("the output", old_output, new_output));
    }
    parts.join("; ")
}

/// One line naming how two lists of requirement ids differ: the count before and the count
/// now, then each id added and each id removed. A removed and an added id at the same
/// position are one reworded, since a criterion or a test whose text changed keeps its
/// place and takes a new id.
// Decision: an id is longer than a detail quotes, so the ids are named rather than quoted,
// as a long old and new are.
// @lfy def/generation/main.lfy:changes
fn id_detail(plural: &str, singular: &str, old: &[String], new: &[String]) -> String {
    let mut parts = vec![format!(
        "{} {plural} before and {} now",
        old.len(),
        new.len()
    )];
    // @lfy def/generation/main.lfy:changes
    let reworded: Vec<(usize, &String, &String)> = old
        .iter()
        .zip(new)
        .enumerate()
        .filter(|(_, (was, is))| was != is && !new.contains(was) && !old.contains(is))
        .map(|(position, (was, is))| (position, was, is))
        .collect();
    for (position, was, is) in &reworded {
        parts.push(format!(
            "the {singular} at {} was reworded, from {was} to {is}",
            position + 1
        ));
    }
    // @lfy def/generation/main.lfy:changes
    for id in new
        .iter()
        .filter(|id| !old.contains(id) && !reworded.iter().any(|(_, _, is)| is == id))
    {
        parts.push(format!("the {singular} {id} was added"));
    }
    for id in old
        .iter()
        .filter(|id| !new.contains(id) && !reworded.iter().any(|(_, was, _)| was == id))
    {
        parts.push(format!("the {singular} {id} was removed"));
    }
    if parts.len() == 1 {
        parts.push(format!("the {plural} are in another order"));
    }
    parts.join("; ")
}

/// The type of an entity, as `interfaceOf` spells it; empty when it has none.
// @lfy def/generation/main.lfy:changes
fn type_spelling(model: &Model, entity: EntityId) -> String {
    match &model.entities[entity].ty {
        Some(ty) => model::type_text(model, ty),
        None => String::new(),
    }
}

/// The parameters of a fn with their types, as `interfaceOf` spells them.
// @lfy def/generation/main.lfy:changes
fn parameter_spellings(model: &Model, entity: EntityId) -> Vec<(String, String)> {
    model.entities[entity]
        .parameters()
        .iter()
        .map(|&symbol| {
            let symbol = &model.symbols[symbol];
            (symbol.name.clone(), type_spelling(model, symbol.entity))
        })
        .collect()
}

/// The output of a fn, as `interfaceOf` spells it; empty when it has none.
// @lfy def/generation/main.lfy:changes
fn output_spelling(model: &Model, entity: EntityId) -> String {
    match model.entities[entity].output() {
        Some(output) => model::type_text(model, output),
        None => String::new(),
    }
}

/// The text of an entity's declaration once its definition, type, parameters, output,
/// criteria, tests, whitespace, and comments are left out: the raw text of the tokens its
/// declaring node covers, with every excluded range and every trivium dropped.
// Decision: a member declares nothing but its name, its definition, and its type, so once
// those are left out nothing of it remains and its body never differs; and the members a
// data, type, trait, or enum declares are left out of its body, since each is compared as
// an entity of its own under it.
// @lfy def/generation/main.lfy:changes
fn body_text(model: &Model, entity: EntityId) -> String {
    let record = &model.entities[entity];
    if matches!(
        record.kind,
        EntityKind::Member | EntityKind::EnumMember | EntityKind::Parameter
    ) {
        return String::new();
    }
    let Some(node) = record.node else {
        return String::new();
    };
    let (start, end) = {
        let info = model.info(node);
        (info.start, info.end)
    };
    let mut excluded: Vec<(usize, usize)> = Vec::new();
    if let Some(definition) = record.definition_node {
        exclude(model, node, definition, &mut excluded);
    }
    // The parameters, with the commas and the parentheses that hold them, so that a
    // parameter added or removed is a change of signature alone; the declared type of a
    // type declaration and the output of a fn, which the declaration holds as a type
    // expression of its own; and the definition of a signature.
    for child in child_nodes(model, node) {
        match model.info(child).rule {
            Rule::Expression(Expression::TypeExpression)
            | Rule::Expression(Expression::Type)
            | Rule::Expression(Expression::Parameters) => {
                exclude(model, node, child, &mut excluded);
            }
            Rule::Expression(Expression::Signature) => {
                for part in child_nodes(model, child) {
                    if matches!(
                        model.info(part).rule,
                        Rule::Expression(Expression::Parameters)
                            | Rule::Expression(Expression::DefinitionClause)
                    ) {
                        exclude(model, node, part, &mut excluded);
                    }
                }
            }
            _ => {}
        }
    }
    for member in members_of(model, entity) {
        if let Some(member) = model.entities[member].node {
            exclude(model, node, member, &mut excluded);
        }
    }
    for criterion in &record.acceptance_criteria {
        if let Some(statement) = criterion.node.and_then(|it| statement_in(model, node, it)) {
            exclude(model, node, statement, &mut excluded);
        }
    }
    for test in &record.tests {
        for part in [test.input, test.expect].into_iter().flatten() {
            if let Some(statement) = statement_in(model, node, part) {
                exclude(model, node, statement, &mut excluded);
            }
        }
    }
    let mut out = String::new();
    for (index, token) in model.tokens(node.file).iter().enumerate().take(end).skip(start) {
        if excluded
            .iter()
            .any(|&(first, last)| (first..last).contains(&index))
        {
            continue;
        }
        if token.rule.is_some_and(is_trivia) {
            continue;
        }
        out.push_str(&token.raw);
    }
    out
}

/// Leave the tokens of a node out of a body, when it falls inside the declaration.
// @lfy def/generation/main.lfy:changes
fn exclude(
    model: &Model,
    declaration: NodeRef,
    node: NodeRef,
    out: &mut Vec<(usize, usize)>,
) {
    if node.file != declaration.file {
        return;
    }
    let (start, end) = {
        let info = model.info(declaration);
        (info.start, info.end)
    };
    let info = model.info(node);
    if info.start >= start && info.end <= end {
        out.push((info.start, info.end));
    }
}

/// The statement of a declaration's body that holds a node: the ancestor whose parent is
/// the declaration or a block of it. `None` when the node is not inside the declaration.
// @lfy def/generation/main.lfy:changes
fn statement_in(model: &Model, declaration: NodeRef, node: NodeRef) -> Option<NodeRef> {
    if node.file != declaration.file {
        return None;
    }
    let (start, end) = {
        let info = model.info(declaration);
        (info.start, info.end)
    };
    let info = model.info(node);
    if info.start < start || info.end > end {
        return None;
    }
    let mut current = node;
    while let Some(parent) = model.parent(current) {
        if parent == declaration {
            return Some(current);
        }
        let info = model.info(parent);
        if info.start < start || info.end > end {
            return Some(current);
        }
        if info.rule == Rule::Statement(Statement::Block) {
            return Some(current);
        }
        current = parent;
    }
    Some(current)
}

/// The nodes a node holds as children, in source order.
// @lfy def/generation/main.lfy:changes
fn child_nodes(model: &Model, node: NodeRef) -> Vec<NodeRef> {
    model
        .node(node)
        .children
        .iter()
        .filter_map(|child| match child {
            Child::Node(child) => model.node_ref(node.file, child),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// review
// ---------------------------------------------------------------------------------------

/// Everything a verifier is handed to check the outputs of one batch against every
/// criterion and test.
///
/// The instructions name the batch and the target, then quote every local criterion and
/// test of every entity of every unit with its id and its place — the file of its origin
/// relative to the workspace root, a colon, and the line its origin begins on — then
/// excerpt every region [`regions_of`] finds for those entities in the target's source
/// maps, then give the verifier protocol. Only local criteria and tests are reviewed here;
/// a global one is reviewed once, by [`global_review`].
// Decision: the verifier is handed the regions already excerpted, so it reads exactly what a
// criterion is about, and its protocol allows no edits: a review says where and why, never
// how to fix.
// Decision: the definition passes the plan and the batch; a plan here does not own its
// workspace or the program lowered from it, so the program is an extra first parameter.
// @lfy def/generation/main.lfy:review#review:review:773848c6e09065334305d916b2185142d987e833789fefbdb3354350383e5261
pub fn review(
    program: &Program,
    plan: &Plan,
    batch: &Batch,
    source_maps: &[SourceMap],
) -> ReviewRequest {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let target = batch
        .units
        .first()
        .map(|&index| workspace.targets[plan.units[index].target].identifier.as_str())
        .unwrap_or_default();
    let mut out = String::new();

    // A heading naming the batch and the target. @lfy def/generation/main.lfy:review
    let _ = writeln!(
        out,
        "# Reviewing the batch `{}` for the target `{}`\n",
        batch.identifier, target
    );
    // @lfy def/generation/main.lfy:review
    out.push_str(
        "Every criterion and test below is local to one entity of this batch. A global one is \
         reviewed once, on its own, across every output of the program.\n\n",
    );

    // Every local criterion and test of every entity, with its id and its place.
    // @lfy def/generation/main.lfy:review
    out.push_str("## What was asked\n\n");
    for &index in &batch.units {
        let unit = &plan.units[index];
        let file = &workspace.files[unit.file];
        let _ = writeln!(out, "### `{}` (stem `{}`)\n", file.path, unit.stem);
        if unit.entities.is_empty() {
            out.push_str("The unit has no entities.\n\n");
        }
        for &entity in &unit.entities {
            write_review_entity(&mut out, model, &program.files[unit.lowered], entity);
        }
    }

    // Then the regions generated for each of those entities.
    // @lfy def/generation/main.lfy:review
    out.push_str("## What was generated\n\n");
    for &index in &batch.units {
        for &entity in &plan.units[index].entities {
            let name = qualified_name(model, entity);
            let _ = writeln!(out, "### `{name}`\n");
            write_regions(
                &mut out,
                workspace,
                matching_regions(workspace, source_maps, &name, Some(target)),
            );
        }
    }

    // The verifier protocol. @lfy def/generation/main.lfy:review
    out.push_str(PROTOCOL);

    ReviewRequest {
        batch: Some(batch.clone()),
        instructions: out,
    }
}

/// One entity of a unit for a verifier: its name, its definition, then each local criterion
/// and each local test on one line prefixed by its id and its place.
// @lfy def/generation/main.lfy:review
fn write_review_entity(
    out: &mut String,
    model: &Model,
    lowered: &LoweredFile,
    entity: EntityId,
) {
    let record = &model.entities[entity];
    let _ = writeln!(
        out,
        "#### `{}` ({})\n",
        qualified_name(model, entity),
        kind_text(record)
    );
    if let Some(definition) = &record.definition {
        let _ = writeln!(out, "Definition: {definition}\n");
    }
    let requirements = entity_requirements(lowered, entity);
    out.push_str("Criteria:\n");
    let mut written = 0;
    for requirement in &requirements {
        if let Requirement::Criterion(criterion) = requirement {
            // @lfy def/generation/main.lfy:review
            let _ = writeln!(
                out,
                "`{}` {} — {}",
                criterion.id,
                origin_place(model, criterion.origin),
                criterion_text(
                    criterion.situation.as_ref(),
                    criterion.behavior.as_ref(),
                    criterion.side_effects.as_ref(),
                )
            );
            written += 1;
        }
    }
    if written == 0 {
        out.push_str("(none)\n");
    }
    out.push('\n');
    out.push_str("Tests:\n");
    written = 0;
    for requirement in &requirements {
        if let Requirement::Test(test) = requirement {
            // @lfy def/generation/main.lfy:review
            let _ = writeln!(
                out,
                "`{}` {} — {}",
                test.id,
                origin_place(model, test.origin),
                test_text(test)
            );
            written += 1;
        }
    }
    if written == 0 {
        out.push_str("(none)\n");
    }
    out.push('\n');
}

/// A criterion's or test's place: the file of its origin relative to the workspace root, a
/// colon, and the line its origin begins on.
// @lfy def/generation/main.lfy:review
fn origin_place(model: &Model, origin: NodeRef) -> String {
    format!(
        "{}:{}",
        model.sources[origin.file].path,
        line_of(model, origin)
    )
}

/// The source line a node begins on, counting from 1.
// @lfy def/generation/main.lfy:review
fn line_of(model: &Model, node: NodeRef) -> usize {
    model.first_token(node).map_or(1, |token| token.line)
}

/// Every region given: the output path, the line range, and the text of those lines in a
/// fenced block; a line saying so instead when the output cannot be read.
// @lfy def/generation/main.lfy:review
fn write_regions(
    out: &mut String,
    workspace: &Workspace,
    regions: Vec<(&SourceMap, &Marker)>,
) {
    if regions.is_empty() {
        out.push_str("No region of any output was generated for it.\n\n");
        return;
    }
    for (map, marker) in regions {
        let _ = writeln!(out, "`{}:{}-{}`\n", map.output, marker.output_line, marker.end);
        // @lfy def/generation/main.lfy:review
        match std::fs::read_to_string(workspace.root.join(&map.output)) {
            Ok(text) => {
                let lines: Vec<&str> = text.lines().collect();
                let last = marker.end.min(lines.len());
                let first = marker.output_line.saturating_sub(1).min(last);
                out.push_str("```\n");
                for line in &lines[first..last] {
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str("```\n\n");
            }
            // @lfy def/generation/main.lfy:review
            Err(error) => {
                let _ = writeln!(out, "The output could not be read: {error}.\n");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// globalReview
// ---------------------------------------------------------------------------------------

/// Everything a verifier is handed to check every global criterion and test once, across
/// every output of the program.
///
/// The instructions name the global review and the targets, then give each global criterion
/// and then each global test with its id and its place, its text, and every region whose
/// marker answers for its id; one that no marker of any output names is listed with a line
/// saying no unit answered for it, so the verifier finds it unverifiable, never violated.
/// The caller runs it once after every batch of a compile, and whenever the program's global
/// criteria or tests changed; a review it finds violated gives the stems of the units whose
/// markers answered for it, for the `violated` argument of [`plan`].
// Decision: the definition passes the plan; a plan here does not own the program lowered
// from its workspace, and nothing else of the plan is read, so the program stands in for it.
// @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:45ae279ab9e22f8c4290aee47f9494f1992000d7f380b8ad4947b52f4f6da8ba
pub fn global_review(program: &Program, source_maps: &[SourceMap]) -> ReviewRequest {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let mut out = String::new();

    // A heading naming the global review and the targets.
    // @lfy def/generation/main.lfy:globalReview
    let targets: Vec<String> = workspace
        .targets
        .iter()
        .map(|target| format!("`{}`", target.identifier))
        .collect();
    let _ = writeln!(
        out,
        "# The global review of every criterion and test of the whole program\n"
    );
    let _ = writeln!(
        out,
        "The targets are {}.\n",
        if targets.is_empty() {
            "(none)".to_string()
        } else {
            targets.join(", ")
        }
    );

    let globals: Vec<Requirement> = program
        .criteria
        .iter()
        .cloned()
        .map(Requirement::Criterion)
        .chain(program.tests.iter().cloned().map(Requirement::Test))
        .collect();
    if globals.is_empty() {
        out.push_str("## What was asked\n\nThe program has no global criterion and no global test.\n\n");
    }
    for requirement in &globals {
        // @lfy def/generation/main.lfy:globalReview
        let (id, origin, text) = match requirement {
            Requirement::Criterion(criterion) => (
                &criterion.id,
                criterion.origin,
                criterion_text(
                    criterion.situation.as_ref(),
                    criterion.behavior.as_ref(),
                    criterion.side_effects.as_ref(),
                ),
            ),
            Requirement::Test(test) => (&test.id, test.origin, test_text(test)),
        };
        let _ = writeln!(out, "## `{id}` {}\n", origin_place(model, origin));
        let _ = writeln!(out, "{text}\n");
        // Every region whose marker answers for this id, whatever target it was built for.
        // @lfy def/generation/main.lfy:globalReview
        let regions: Vec<(&SourceMap, &Marker)> = source_maps
            .iter()
            .flat_map(|map| {
                map.markers
                    .iter()
                    .filter(|marker| marker.requirement.as_deref() == Some(id.as_str()))
                    .map(move |marker| (map, marker))
            })
            .collect();
        if regions.is_empty() {
            // @lfy def/generation/main.lfy:globalReview
            out.push_str("No unit answered for it, so it cannot be checked against any region.\n\n");
            continue;
        }
        write_regions(&mut out, workspace, regions);
    }

    // The verifier protocol. @lfy def/generation/main.lfy:globalReview
    out.push_str(PROTOCOL);

    ReviewRequest {
        // @lfy def/generation/main.lfy:globalReview
        batch: None,
        instructions: out,
    }
}

/// What a verifier is told about its own work: that it examines and never edits, what it
/// writes for each criterion and each test, and how its report ends.
// @lfy def/generation/main.lfy:review
const PROTOCOL: &str = "\
## How to review

You examine and never edit: you change no file, and you propose no code.

Write one JSON object on one line per criterion and per test, with exactly the keys \
`id`, `status`, `evidence`, and `note`:

- `id` is the id the criterion or test is quoted with above. Write no place of your own: \
  the file, the line, and the entity are derived from the id.
- `status` is `satisfied` when the code does what the criterion says, `violated` when it \
  does something else, or `unverifiable` when no region can be checked against it. A \
  criterion no region can be checked against is unverifiable, never violated.
- `evidence` is the output path and the line range you read, such as \
  `crates/elfie-core/src/x.rs:120-134`, and empty when you read none.
- `note` is one line saying why and, for a violated one, what the code does instead.

The tools of the agent server may be called for regions this request leaves out.

End your report with exactly one line reading `ELFIE: REVIEWED`.
";

// ---------------------------------------------------------------------------------------
// reviewOf
// ---------------------------------------------------------------------------------------

/// What a verifier found, read mechanically from its report.
///
/// The end line is the last line reading `ELFIE: REVIEWED`, with any whitespace around it;
/// lines after it are ignored. Each line before it that reads as one review object whose id
/// names a criterion or test of the program is one review, in report order, and of two with
/// the same id only the last is kept, at its own place. Every other line that is not blank
/// and does not begin with `#` is a problem, as written. With no end line every line is
/// read the same way and the problems end with a line saying the report did not end.
///
/// A review is read by its id alone; its file, line, and entity are derived from the
/// origin of the criterion or test that id names, as [`Review::at`] fills them.
// @lfy def/generation/main.lfy:reviewOf
pub fn review_of(report: &str, program: &Program) -> ReviewReport {
    /// What the problems end with when the report has no end line.
    const UNENDED: &str = "the report did not end";

    let placed = placed_requirements(program);
    let lines: Vec<&str> = report.lines().collect();
    // @lfy def/generation/main.lfy:reviewOf
    let end = lines.iter().rposition(|line| line.trim() == REVIEWED);
    let body = match end {
        Some(index) => &lines[..index],
        None => &lines[..],
    };
    let mut reviews: Vec<Review> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for line in body {
        // A line whose id names no criterion and no test of the program is no review: the
        // place a review is given is the origin of what its id names.
        // @lfy def/generation/main.lfy:reviewOf
        let read = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .as_ref()
            .and_then(Review::from_json)
            .and_then(|review| {
                let (file, at, entity) = placed.get(&review.id)?;
                Some(review.at(file, *at, entity))
            });
        match read {
            Some(review) => {
                // @lfy def/generation/main.lfy:reviewOf
                reviews.retain(|kept| kept.id != review.id);
                reviews.push(review);
            }
            None => {
                let text = line.trim_start();
                // @lfy def/generation/main.lfy:reviewOf
                if !text.trim_end().is_empty() && !text.starts_with('#') {
                    problems.push((*line).to_string());
                }
            }
        }
    }
    if end.is_none() {
        // @lfy def/generation/main.lfy:reviewOf
        problems.push(UNENDED.to_string());
    }
    ReviewReport { reviews, problems }
}

/// Every criterion and test of a program by id, global and local, each with the file and
/// the line its origin gives and the name of the entity it belongs to.
// @lfy def/generation/main.lfy:reviewOf
fn placed_requirements(program: &Program) -> BTreeMap<String, (String, usize, String)> {
    let model = &program.workspace.model;
    let mut out: BTreeMap<String, (String, usize, String)> = BTreeMap::new();
    let mut place = |id: &str, origin: NodeRef, entity: Option<EntityId>| {
        let name = match entity {
            Some(entity) => qualified_name(model, entity),
            None => GLOBAL.to_string(),
        };
        out.insert(
            id.to_string(),
            (
                model.sources[origin.file].path.clone(),
                line_of(model, origin),
                name,
            ),
        );
    };
    for criterion in &program.criteria {
        place(&criterion.id, criterion.origin, criterion.entity);
    }
    for test in &program.tests {
        place(&test.id, test.origin, test.entity);
    }
    for file in &program.files {
        for requirement in local_requirements(file) {
            match requirement {
                Requirement::Criterion(criterion) => {
                    place(&criterion.id, criterion.origin, criterion.entity);
                }
                Requirement::Test(test) => place(&test.id, test.origin, test.entity),
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// markers, hashes, source maps, timestamps
// ---------------------------------------------------------------------------------------

/// Every `@lfy <path>:<line>[:<column>]`, `@lfy <path>:<entity>` or
/// `@lfy <path>:<entity>#<id>` marker in an output's text, with the output line it sits on,
/// in output order. The marker sits after the line comment opener of the target's language,
/// whatever that is, so `@lfy ` is matched anywhere in a line. A marker that names an entity
/// has no line until one is derived. Each marker's end is derived as the line before the
/// output line of the next marker, or the last line of the output for the last marker.
// @lfy def/generation/data.lfy:Marker
pub fn parse_markers(text: &str) -> Vec<Marker> {
    let mut out: Vec<Marker> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(position) = rest.find(MARKER_PREFIX) {
            let after = &rest[position + MARKER_PREFIX.len()..];
            let token = after.split(char::is_whitespace).next().unwrap_or("");
            if let Some(marker) = parse_marker(token, index + 1) {
                out.push(marker);
            }
            rest = after;
        }
    }
    // Decision: a marker sharing its output line with the next one ends on the line before
    // both, which is a region covering nothing, so that line still belongs to exactly one
    // marker. @lfy def/generation/data.lfy:Marker.end
    let last_line = text.lines().count();
    for index in 0..out.len() {
        out[index].end = match out.get(index + 1) {
            Some(next) => next.output_line.saturating_sub(1),
            None => last_line,
        };
    }
    out
}

/// A marker from what follows `@lfy `: `path:line`, `path:line:column`, `path:entity`, or
/// `path:entity#id`. `None` when the path does not end in `.lfy`, or when what follows the
/// colon is neither a number nor an identifier path: that text is prose, not a marker. Its
/// end is its own output line until [`parse_markers`] derives it from the marker that
/// follows.
// @lfy def/generation/data.lfy:Marker
fn parse_marker(token: &str, output_line: usize) -> Option<Marker> {
    // Decision: punctuation that closes a sentence or a comment after the marker is not
    // part of it.
    let token = token.trim_end_matches(['.', ',', ';', ')', ']', '}', '*', '/', '-']);
    // A requirement is spelled after the entity and a `#`, and an id holds no `#`, so the
    // place is what precedes the first one. @lfy def/generation/data.lfy:Marker.requirement
    let (token, requirement) = match token.split_once('#') {
        // An empty id is no id, so the text is prose rather than a marker.
        Some((_, "")) => return None,
        Some((place, id)) => (place, Some(id.to_string())),
        None => (token, None),
    };
    let mut parts = token.rsplitn(3, ':');
    let last = parts.next()?;
    let middle = parts.next()?;
    let first = parts.next();
    // A marker that names a line spells no requirement, so a `#` after one leaves text that
    // is neither a number nor an identifier path: prose.
    if requirement.is_some() && last.parse::<usize>().is_ok() {
        return None;
    }
    if let (Some(path), Ok(line), Ok(column)) =
        (first, middle.parse::<usize>(), last.parse::<usize>())
        && is_source_path(path)
    {
        return Some(Marker {
            output_line,
            file: path.to_string(),
            entity: None,
            line,
            column: Some(column),
            requirement: None,
            end: output_line,
        });
    }
    let path = match first {
        Some(first) => format!("{first}:{middle}"),
        None => middle.to_string(),
    };
    if !is_source_path(&path) {
        return None;
    }
    match last.parse::<usize>() {
        Ok(line) => Some(Marker {
            output_line,
            file: path,
            entity: None,
            line,
            column: None,
            requirement: None,
            end: output_line,
        }),
        // @lfy def/generation/data.lfy:Marker.entity
        Err(_) if is_identifier_path(last) => Some(Marker {
            output_line,
            file: path,
            entity: Some(last.to_string()),
            line: 0,
            column: None,
            requirement,
            end: output_line,
        }),
        Err(_) => None,
    }
}

/// Whether text names a source file: a path ending in `.lfy`, with a name before it.
// @lfy def/generation/data.lfy:Marker
fn is_source_path(path: &str) -> bool {
    path.len() > EXTENSION.len() && path.ends_with(EXTENSION)
}

/// Whether text is an identifier, or an owner's identifier, a dot, and a member's name.
// @lfy def/generation/data.lfy:Marker
fn is_identifier_path(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .is_some_and(|first| first == '_' || is_xid_start(first))
                && characters.all(|character| character == '_' || is_xid_continue(character))
        })
}

/// SHA-256 of a text's bytes, as lowercase hex.
// @lfy def/generation/data.lfy:SourceMap.hash
pub fn source_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The source maps in one map file: a JSON array of source map objects. A missing file
/// gives none.
// @lfy def/generation/data.lfy:SourceMap
pub fn read_source_maps(path: &Path) -> Vec<SourceMap> {
    // Decision: a file that cannot be read or parsed gives no source maps either, which
    // makes every unit of the target fresh; the caller sees the file as it is.
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    match value.as_array() {
        Some(items) => items.iter().filter_map(SourceMap::from_json).collect(),
        None => Vec::new(),
    }
}

/// Write source maps as one map file: a JSON array of source map objects, one per
/// line-broken object, creating the directory when it is missing.
// @lfy def/generation/data.lfy:SourceMap
pub fn write_source_maps(path: &Path, maps: &[SourceMap]) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let value = serde_json::Value::Array(maps.iter().map(SourceMap::to_json).collect());
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    std::fs::write(path, text)
}

/// The current time as an RFC 3339 timestamp in UTC, to the second.
// @lfy def/generation/data.lfy:SourceMap.generated
pub fn now_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format_rfc3339(seconds as i64)
}

/// Seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn format_rfc3339(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3600,
        (of_day % 3600) / 60,
        of_day % 60
    )
}

/// An RFC 3339 timestamp as seconds since the Unix epoch, fraction dropped and offset
/// applied; `None` when it is not one.
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, time) = text.split_at_checked(10)?;
    let mut date = date.split('-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: u32 = date.next()?.parse().ok()?;
    let day: u32 = date.next()?.parse().ok()?;
    if date.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let time = time.strip_prefix(['T', 't', ' '])?;
    let (hour, rest) = time.split_at_checked(2)?;
    let (minute, rest) = rest.strip_prefix(':')?.split_at_checked(2)?;
    let (second, rest) = rest.strip_prefix(':')?.split_at_checked(2)?;
    let hour: i64 = hour.parse().ok()?;
    let minute: i64 = minute.parse().ok()?;
    let second: i64 = second.parse().ok()?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let rest = match rest.strip_prefix('.') {
        Some(fraction) => fraction.trim_start_matches(|c: char| c.is_ascii_digit()),
        None => rest,
    };
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.chars().next()? {
                '+' => 1,
                '-' => -1,
                _ => return None,
            };
            let (hours, minutes) = rest[1..].split_once(':')?;
            let hours: i64 = hours.parse().ok()?;
            let minutes: i64 = minutes.parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month = i64::from(month);
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The proleptic Gregorian date of a day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

// ---------------------------------------------------------------------------------------
// paths
// ---------------------------------------------------------------------------------------

/// Whether a directory is the root itself.
fn is_root(directory: &str) -> bool {
    directory.is_empty() || directory == "."
}

/// Whether a path is under a directory, both relative to the root.
fn under(path: &str, directory: &str) -> bool {
    strip_directory(path, directory).is_some()
}

/// A path relative to a directory it is under, both relative to the root.
fn strip_directory<'p>(path: &'p str, directory: &str) -> Option<&'p str> {
    if is_root(directory) {
        return Some(path);
    }
    let directory = directory.trim_end_matches('/');
    path.strip_prefix(directory)?.strip_prefix('/')
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::workspace::load;

    /// A project directory under the system's temporary directory, removed when dropped.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "elfie-gen-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Fixture { root }
        }

        /// A project with one target `rust` whose marker `global` carries, an output
        /// directory `src`, and an empty `def` directory. The package `elfie` beside it
        /// is the standard library every fixture is loaded against: its main file is the
        /// prelude, so the trait `target` a marker must extend is in scope everywhere.
        fn with_rust_target() -> Fixture {
            let fixture = Fixture::new();
            fixture
                .write(
                    "elfie.json",
                    r#"{
                        "output": "src",
                        "dependencies": {
                            "elfie": { "root": "lib" },
                            "rust": { "root": "targets/rust" }
                        },
                        "targets": { "rust": { "package": "rust", "marker": "rust" } },
                        "native": [ { "identifier": "serde_json", "ecosystem": "cargo", "version": "1" } ]
                    }"#,
                )
                .write("lib/main.lfy", LIBRARY)
                .write(
                    "targets/rust/elfie.json",
                    r#"{ "native": [ { "identifier": "sha2", "ecosystem": "cargo" } ] }"#,
                )
                .write("targets/rust/main.lfy", RUST_TARGET);
            fs::create_dir_all(fixture.root.join("def")).unwrap();
            fixture
        }

        /// The same project whose standard library also holds one data `Path` a project
        /// file may name as a type.
        fn with_elfie_package() -> Fixture {
            let fixture = Fixture::with_rust_target();
            fixture
                .write(
                    "lib/main.lfy",
                    &format!("{LIBRARY}\nd Path: `A path` {{\n  $text = string;\n}}\n"),
                )
                .write("def/a.lfy", "use \"elfie\";\n\nd A {\n  $p = Path;\n}\n");
            fixture
        }

        fn write(&self, path: &str, text: &str) -> &Fixture {
            let disk = self.root.join(path);
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(disk, text).unwrap();
            self
        }

        /// The project loaded and lowered: what every entry point of generation reads.
        fn program(&self) -> Program {
            crate::interpret::lower(load(&self.root))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// The main file of the package `elfie`: the prelude. It declares the trait
    /// `target`, which the loader requires every target's marker to extend, and no
    /// criteria of its own, so a fixture's guidance is the marker's alone. It also
    /// declares the kind data `Entity` and `Trait`, so that `rust.apply(global)` reads
    /// `apply` as a member of the marker's kind rather than as a name nothing declares.
    const LIBRARY: &str = "d Entity: `What every declared thing is seen as through its context layer` {\n}\n\nd Trait extends Entity: `A trait seen through its context layer` {\n  $apply: `Applies the trait to a target and returns the trait` = (target: Entity) => Trait;\n}\n\ntrait target: `What an entity carries to be built for one target` {\n}\n";

    /// The package of the target `rust`: one marker trait carried by `global`.
    const RUST_TARGET: &str = "/// Built for Rust.\ntrait rust extends target {\n  @acceptanceCriteria.add({ behavior = `Each unit becomes one module named after its stem` });\n}\nrust.apply(global);\n";

    const A: &str = "use \"./b\";\n\n/// A record.\nd A: `An a` {\n  $b = B;\n}\n";
    const B: &str = "d B: `A b` {\n  $x = string;\n}\n\nfn make(x: string): `Makes a b` => B {\n  @acceptanceCriteria.add({ situation = `x is empty`, behavior = `the b holds x` });\n  @test({ input = [\"y\"], expect = B@like(`holding y`) });\n}\n";

    /// The project of the definition's tests: `def/a.lfy` using `./b`, and `def/b.lfy`.
    fn a_and_b() -> Fixture {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/a.lfy", A).write("def/b.lfy", B);
        fixture
    }

    fn file_index(workspace: &Workspace, path: &str) -> usize {
        workspace
            .files
            .iter()
            .position(|file| file.path == path)
            .unwrap_or_else(|| panic!("{path} is not in the program"))
    }

    /// The plan of a project a test has already lowered. [`plan`] lowers the workspace
    /// itself and gives that program back beside the plan, so a test that holds one plans
    /// the workspace it was lowered from; lowering is idempotent, so the program the call
    /// gives is the one the test holds, and only the plan is kept.
    // @lfy def/generation/main.lfy:plan
    fn plan_of(
        program: &Program,
        maps: &[SourceMap],
        requested: &[String],
        violated: &[String],
    ) -> Plan {
        let (lowered, plan) = plan(program.workspace.clone(), maps, requested, violated);
        assert_eq!(
            lowered.files.iter().map(|file| &file.text).collect::<Vec<_>>(),
            program.files.iter().map(|file| &file.text).collect::<Vec<_>>()
        );
        plan
    }

    fn unit_of<'p>(workspace: &Workspace, plan: &'p Plan, path: &str) -> (usize, &'p Unit) {
        let file = file_index(workspace, path);
        let index = plan
            .unit_of(0, file)
            .unwrap_or_else(|| panic!("{path} has no unit: {:?}", plan.units));
        (index, &plan.units[index])
    }

    fn names(workspace: &Workspace, entities: &[EntityId]) -> Vec<String> {
        entities
            .iter()
            .map(|&entity| entity_name(&workspace.model, entity))
            .collect()
    }

    /// One source map per unit of a plan, recording that unit's lowered hash, its
    /// requirements hash, its interface signature, and the signature of each of its
    /// dependencies as they are now: what acceptance would have written.
    fn current_maps(program: &Program, plan: &Plan) -> Vec<SourceMap> {
        let workspace = &program.workspace;
        plan.units
            .iter()
            .map(|unit| {
                let file = &workspace.files[unit.file];
                SourceMap {
                    target: workspace.targets[unit.target].identifier.clone(),
                    output: format!("src/{}.rs", unit.stem),
                    source: file.path.clone(),
                    hash: source_hash(&program.files[unit.lowered].text),
                    requirements: requirements_hash(program, unit),
                    signature: interface_signature(workspace, unit),
                    dependencies: unit
                        .dependencies
                        .iter()
                        .map(|&dependency| {
                            let dependency = &plan.units[dependency];
                            (
                                workspace.files[dependency.file].path.clone(),
                                interface_signature(workspace, dependency),
                            )
                        })
                        .collect(),
                    generated: "2026-09-18T10:00:00Z".to_string(),
                    markers: Vec::new(),
                }
            })
            .collect()
    }

    fn assert_bound(workspace: &Workspace) {
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let marker = workspace.targets[0].marker;
        assert!(workspace.model.entities[marker].is_trait());
        assert!(
            workspace.model.entities[workspace.model.global].has_trait(marker),
            "rust.apply(global) must apply the marker to global"
        );
    }

    // -----------------------------------------------------------------------------------
    // interfaceOf
    // -----------------------------------------------------------------------------------

    // @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:0a8c932a60ed0656cc05be3e2f629489ee9f346207bc96792c2faa7780b35bc8
    #[test]
    fn an_interface_holds_one_line_per_entity_with_its_kind_definition_type_and_signature() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let text = interface_of(&workspace, b);
        let lines: Vec<&str> = text.lines().collect();
        // One line per entity, and one per member after a data.
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(lines[0], "- `B` (data: DataDeclaration): A b — type `B`");
        // @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:d44e18f93ffee843b64a0c2ca005e823ccf79a877a95abadca4b961d6ebf3fbf
        assert_eq!(lines[1], "  - `x` (member) — type `string`");
        assert!(
            lines[2].starts_with("- `make` (agent function: AgentFunctionDeclaration): Makes a b"),
            "{text}"
        );
        // @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:55ea69cd92f1fbcd51135a8b7d82e1c483fe8211dec4744efcb7f68b754e9603
        assert!(lines[2].contains("— parameters (x: string)"), "{text}");
        assert!(lines[2].contains("— output `B`"), "{text}");
        // Nothing in an interface is a line number, so nothing moves with the source.
        assert!(!text.contains(":1"), "{text}");
    }

    // @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:d5cc23ff84d1f7030319204b0c641e1333e40f718e59106f3308017815b43f02
    #[test]
    fn a_new_criterion_or_a_moved_line_leaves_the_interface_the_same() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let before = interface_of(&workspace, b);

        fixture.write(
            "def/b.lfy",
            &format!(
                "// a comment that moves every line\n{}",
                B.replace(
                    "@test(",
                    "@acceptanceCriteria.add({ behavior = `a new criterion` });\n  @test(",
                )
            ),
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(interface_of(&workspace, b), before);
    }

    // @lfy def/generation/main.lfy:interfaceOf
    #[test]
    fn a_member_added_to_a_data_changes_the_interface() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let before = interface_of(&workspace, b);

        fixture.write(
            "def/b.lfy",
            &B.replace("$x = string;", "$x = string;\n  $y = number;"),
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let after = interface_of(&workspace, b);
        assert_ne!(after, before);
        assert!(
            after.contains("  - `y` (member) — type `number`"),
            "{after}"
        );
    }

    /// A type line names an entity of the `elfie` package by its identifier alone, such as
    /// `List` or `Path`, never by a path or a module name.
    // @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:e28fb34329366387ffd642da5ca0928aef112290bec530896eeb9a4b63ed9b4a
    #[test]
    fn a_type_of_the_elfie_package_is_spelled_by_its_identifier_alone() {
        let fixture = Fixture::with_elfie_package();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let text = interface_of(&workspace, a);
        assert!(text.contains("  - `p` (member) — type `Path`"), "{text}");
        assert!(!text.contains("elfie"), "{text}");
        assert!(!text.contains("lib/"), "{text}");
    }

    // -----------------------------------------------------------------------------------
    // plan
    // -----------------------------------------------------------------------------------

    /// The program the plan is for is the workspace lowered: `plan` lowers it itself and
    /// gives that program back beside the plan, and every unit names its lowered file by
    /// its place in it.
    // @lfy def/generation/main.lfy:plan#plan:plan:c731524b6fa6dd3c90dfca0fafe1f50a433bcd8a45b1191da289c61d71c027a1
    #[test]
    fn the_plan_comes_with_the_workspace_lowered() {
        let fixture = a_and_b();
        let (program, plan) = plan(load(&fixture.root), &[], &[], &[]);
        assert_bound(&program.workspace);
        // What the plan is for is what lowering the same workspace gives.
        // @lfy def/generation/main.lfy:plan
        let lowered = crate::interpret::lower(load(&fixture.root));
        assert_eq!(
            program.files.iter().map(|file| &file.text).collect::<Vec<_>>(),
            lowered.files.iter().map(|file| &file.text).collect::<Vec<_>>()
        );
        assert_eq!(program.files.len(), program.workspace.files.len());
        // Each unit's lowered file is that program's file for the unit's file.
        // @lfy def/generation/main.lfy:plan
        assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
        for unit in &plan.units {
            assert_eq!(program.files[unit.lowered].file, unit.file);
        }
    }

    /// The standard library is compiled by its own project, so none of its entities is
    /// ever built, however the marker is applied; a unit's interface may still name them.
    // @lfy def/generation/main.lfy:plan#plan:plan:be06f4de79309f738b40cc1b0e04dd968e510f8fe0f60d51b3dd47fab9a5c82b
    #[test]
    fn the_elfie_package_gives_no_unit_even_though_the_marker_is_applied_to_global() {
        let fixture = Fixture::with_elfie_package();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        assert!(
            workspace.file("lib/main.lfy").unwrap().package.is_some(),
            "{:?}",
            workspace.files
        );
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(plan.units[0].file, file_index(&workspace, "def/a.lfy"));
        // `global` carries the marker, so every entity the project's own file declares in
        // its file scope is built.
        // @lfy def/generation/main.lfy:plan#plan:plan:ac259fcc44a978f429ea10a8890c972374a471acc83cd96fb7b810b006088e84
        assert_eq!(names(&workspace, &plan.units[0].entities), ["A"]);
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:a2118f8faba26a8e6c0652494d6f74c96af2a992b8992f46db97195703466e36
    #[test]
    fn two_files_give_two_fresh_units_with_b_before_a_in_one_batch() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        // Each unit comes after its dependencies.
        // @lfy def/generation/main.lfy:plan#plan:plan:77cdabac2f33206376b13db1d49958e635787db9c6d54db3ac05b2405c0776da
        assert!(b_index < a_index);
        // The file `a` uses has a unit of the same target, so that unit is its dependency.
        // @lfy def/generation/main.lfy:plan#plan:plan:25f32ad65d223caa8411f2221f1cfe7adee702020f56a78d3cb0f08f0c92c7c4
        assert_eq!(a.dependencies, [b_index]);
        assert!(b.dependencies.is_empty());
        // Neither unit has an output recorded, so each is fresh.
        // @lfy def/generation/main.lfy:plan#plan:plan:01751bca4d2165839842ab83d64cf2cab13330a74b283b7c2cf83fd8513371c3
        assert_eq!(a.reason, Some(Reason::Fresh));
        assert_eq!(b.reason, Some(Reason::Fresh));
        assert!(a.outputs.is_empty());
        // One unit per file, holding the entities built for the target in file order.
        // @lfy def/generation/main.lfy:plan#plan:plan:74330a0de703ad5cdf50520ccae772d3f4740e354b2b63bfe2d46f4e3fd5756b
        assert_eq!(names(&workspace, &a.entities), ["A"]);
        assert_eq!(names(&workspace, &b.entities), ["B", "make"]);
        // @lfy def/generation/main.lfy:plan
        assert_eq!(a.stem, "a");
        assert_eq!(b.stem, "b");
        assert_eq!(a.target, 0);
        assert_eq!(a.file, file_index(&workspace, "def/a.lfy"));
        // The batches partition the planned units, in `Plan.units` order.
        // @lfy def/generation/main.lfy:plan#plan:plan:0910ea1930bd237a7aa923b69454c5f6eeff84187858284d5fc76038c57c67f6
        assert_eq!(plan.planned().count(), 2);
        assert_eq!(plan.batches[0].units, [b_index, a_index]);
        // One batch, named after the first unit and the count of the others.
        // @lfy def/generation/main.lfy:plan#plan:plan:4a43b797673c64c5f48703569bd883f48a01d65de1f0a00b3afa47cd42bbdd9b
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].identifier, "b+1");
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:beb6876df32561e8b00b93be47734a83eab55ef2301d0b18b83bd54cc20f9400
    #[test]
    fn matching_source_maps_leave_no_reason_and_no_batches() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
        let plan = plan_of(&program, &maps, &[], &[]);
        assert_eq!(plan.units.len(), 2);
        // Source, requirements, and every dependency's interface all match what the outputs
        // were generated against, so neither unit has a reason.
        // @lfy def/generation/main.lfy:plan#plan:plan:06b6862e16f0aec536deaa09240adb61e709acb77652a33463a2a03ce2fcfa6d
        assert!(
            plan.units.iter().all(|unit| unit.reason.is_none()),
            "{:?}",
            plan.units
        );
        // @lfy def/generation/main.lfy:plan#plan:plan:bb4bf5858fa1665463b919cafa742a2183b802dd952e3adf3ed7f370c5c06462
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(a.outputs.len(), 1);
        assert_eq!(a.outputs[0].source, "def/a.lfy");
        assert_eq!(plan.planned().count(), 0);
        // Nothing is planned, so there is no batch.
        // @lfy def/generation/main.lfy:plan#plan:plan:00419dd3137b41e45f2661a18ef40a62b4c773266754daf58687bafc6f4fc448
        assert!(plan.batches.is_empty());
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:ec66359827b22c56e28078a9cd8579310aa3ebcbe273f7c1348d153b2c4aab35
    #[test]
    fn a_comment_added_inside_a_dependency_changes_it_and_leaves_its_dependent_alone() {
        let fixture = a_and_b();
        let old = fixture.program();
        assert_bound(&old.workspace);
        let mut maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));

        fixture.write("def/b.lfy", &format!("{B}\n// edited\n"));
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        // The recorded interface signatures still match the current ones.
        let fresh = current_maps(&program,&plan_of(&program, &[], &[], &[]));
        for map in &mut maps {
            let current = fresh.iter().find(|m| m.source == map.source).unwrap();
            assert_eq!(map.signature, current.signature);
            assert_eq!(map.dependencies, current.dependencies);
        }

        let plan = plan_of(&program, &maps, &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        // The lowered text differs from what the output was generated from.
        // @lfy def/generation/main.lfy:plan#plan:plan:9e89bebbb10531378f7a43dc25523a6d46c0679e429d287bddc2d3c54d5017c4
        assert_eq!(b.reason, Some(Reason::Changed));
        // b is planned only for itself: its interface is the same.
        // @lfy def/generation/main.lfy:plan#plan:plan:21c936e9eed81333fa424b811e0e619a14ec0d30a43b4bbe09f654365cf1e4f5
        assert_eq!(a.reason, None);
        // @lfy def/generation/main.lfy:plan#plan:plan:940cf97d0f821e901e514ec764aab55fb31ddcd7848c3b1a6d24143c4b95fea2
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(plan.batches[0].identifier, "b");
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:b95b8ee170e758c24cb0c08f7daa63a613b0c0d221d5987de5dbfe33dcc489c4
    #[test]
    fn a_member_added_to_a_dependency_plans_its_dependent_too_in_one_batch() {
        let fixture = a_and_b();
        let old = fixture.program();
        assert_bound(&old.workspace);
        let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));

        fixture.write(
            "def/b.lfy",
            &B.replace("$x = string;", "$x = string;\n  $y = number;"),
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &maps, &[], &[]);
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(b.reason, Some(Reason::Changed));
        // b's interface changed, so what was generated against it is planned again.
        // @lfy def/generation/main.lfy:plan#plan:plan:b01cae3d6238ad2ce24a209d8745a84fffa0ff56a165c786e1885f9ca6c29ce2
        assert_eq!(a.reason, Some(Reason::Dependency));
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].units, [b_index, a_index]);
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:d15ebad57b16591d5cd11dd65291b904eb647e75f81703360c3226cc3317f612
    #[test]
    fn a_marker_nothing_carries_gives_no_units() {
        let fixture = a_and_b();
        fixture.write("targets/rust/main.lfy", "trait rust extends target { }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        assert_eq!(workspace.targets.len(), 1);
        let plan = plan_of(&program, &[], &[], &[]);
        // Neither an entity, nor its file's own entity, nor `global` carries the marker, so
        // nothing is built for the target.
        // @lfy def/generation/main.lfy:plan#plan:plan:ce4b42de99ea58625b6ea4a3f7b88b97ed5687c5e23dcbed6b6ccd6acc9b238e
        assert!(plan.units.is_empty(), "{:?}", plan.units);
        assert!(plan.batches.is_empty());
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:7c36139cf3ed7d985a135d870fbaef5510ab826c299ee1e57f704e99ec393f65
    #[test]
    fn an_entity_carrying_the_marker_itself_is_built() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("targets/rust/main.lfy", "trait rust extends target { }\n")
            .write(
                "def/a.lfy",
                "use \"rust\";\n\nd Marked is rust { $x = string; }\nd Plain { $y = string; }\n",
            );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(names(&workspace, &plan.units[0].entities), ["Marked"]);
    }

    /// The marker applied to a file's own entity builds everything that file declares in its
    /// file scope, and nothing of any other file.
    // @lfy def/generation/main.lfy:plan#plan:plan:c1c836d62b7c25cbc77c533779c3e617b71ab0a3edce7d38b236098873a17798
    #[test]
    fn the_marker_on_a_files_own_entity_builds_every_entity_of_that_file() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("targets/rust/main.lfy", "trait rust extends target { }\n")
            .write(
                "def/a.lfy",
                "use \"rust\";\nrust.apply(.);\n\nd One { $x = string; }\nd Two { $y = string; }\n",
            )
            .write("def/bare.lfy", "d Untouched { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let plan = plan_of(&program, &[], &[], &[]);
        // One unit, for the file whose own entity carries the marker.
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(
            plan.units[0].file,
            file_index(&workspace, "def/a.lfy"),
            "{:?}",
            plan.units
        );
        assert_eq!(names(&workspace, &plan.units[0].entities), ["One", "Two"]);
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:3fddf2695435bb8dea3228dea358a2ab161da48b37a719e59cc8d5caa41b9747
    #[test]
    fn a_package_file_gives_no_unit_and_a_nested_file_keeps_its_directory_in_its_stem() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/deep/inner.lfy", "d Inner { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        // The package's main.lfy declares a trait global carries, but has a package.
        assert!(
            workspace
                .file("targets/rust/main.lfy")
                .unwrap()
                .package
                .is_some()
        );
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        // The stem keeps the directory the file was found under.
        // @lfy def/generation/main.lfy:plan
        assert_eq!(plan.units[0].stem, "deep/inner");
        assert_eq!(
            plan.units[0].file,
            file_index(&workspace, "def/deep/inner.lfy")
        );
        assert_eq!(plan.batches[0].identifier, "deep/inner");
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:a07ff11d6307a264a4979182b1c0f831053be1bbda51cbfee833c703d5f5ed4d
    #[test]
    fn a_used_file_with_no_unit_contributes_the_units_of_its_own_uses() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write(
                "def/a.lfy",
                "use \"./c\";\nuse \"./b\";\n\nd A { $x = string; }\n",
            )
            .write("def/b.lfy", "d B { $x = string; }\n")
            .write("def/c.lfy", "use \"./d\";\nuse \"./b\";\n")
            .write("def/d.lfy", "d D { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 3, "{:?}", plan.units);
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        let (d_index, _) = unit_of(&workspace, &plan, "def/d.lfy");
        // Each once, in use order: c's uses (d, then b) come through c, then b directly.
        // @lfy def/generation/main.lfy:plan#plan:plan:13f5634596b03beee4c48f43f26a63f8eabec0887876576b9e7c6e76a73ff68c
        assert_eq!(a.dependencies, [d_index, b_index]);
        // @lfy def/generation/main.lfy:plan
        assert!(a_index > b_index && a_index > d_index);
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:a65407acd23ae79d49db581f73620ee82f689fbbc681fa840b44693a10859dcf
    #[test]
    fn a_cycle_among_uses_adds_nothing() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/a.lfy", "use \"./b\";\nd A { $x = string; }\n")
            .write("def/b.lfy", "use \"./a\";\nd B { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        // One problem at each `Use` of the cycle; the plan adds nothing beyond them.
        assert_eq!(workspace.problems.len(), 2, "{:?}", workspace.problems);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 2);
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(a.dependencies, [b_index]);
        assert_eq!(b.dependencies, [a_index]);
        assert!(
            plan.units
                .iter()
                .all(|unit| unit.reason == Some(Reason::Fresh))
        );
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:68cc4a6ef4fd0e7da9d24e21b1ef05a1e1d173ad7313adf6c3971c21bd01bd3c
    #[test]
    fn requested_comes_before_every_other_reason_by_path_or_stem() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
        let plan = plan_of(&program, &maps, &["b".to_string()], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(b.reason, Some(Reason::Requested));
        // b's interface did not change, so a is left alone. @lfy def/generation/main.lfy:plan
        assert_eq!(a.reason, None);
        let plan = plan_of(&program, &[], &["def/a.lfy".to_string()], &[]);
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(a.reason, Some(Reason::Requested));
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn an_output_recording_nothing_for_a_dependency_plans_the_unit() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let mut maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
        for map in &mut maps {
            map.dependencies.clear();
        }
        let plan = plan_of(&program, &maps, &[], &[]);
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(a.reason, Some(Reason::Dependency));
        assert_eq!(b.reason, None);
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn a_batch_holds_at_most_six_units_of_one_directory() {
        let fixture = Fixture::with_rust_target();
        for index in 0..7 {
            fixture.write(
                &format!("def/deep/u{index}.lfy"),
                &format!("d U{index} {{ $x = string; }}\n"),
            );
        }
        fixture.write("def/top.lfy", "d Top { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 8, "{:?}", plan.units);
        let sizes: Vec<usize> = plan.batches.iter().map(|batch| batch.units.len()).collect();
        // Six of `deep`, then the seventh, and `top` on its own: no batch holds more than
        // six units, and none mixes directories.
        // @lfy def/generation/main.lfy:plan#plan:plan:cd995739fb89900db149444dc0089c9c065ffab2f5e8265739019a8dcc7a1328
        assert_eq!(sizes.iter().sum::<usize>(), 8, "{:?}", plan.batches);
        // A unit joins the open batch only while it shares the first segment of its stem and
        // the batch holds fewer than six units.
        // @lfy def/generation/main.lfy:plan#plan:plan:f71ab7b07fe00efd9b3b473a862beed93f9cdafc988c188117cc9f3bd4ccd0a6
        assert!(sizes.iter().all(|&size| size <= BATCH_UNITS), "{sizes:?}");
        for batch in &plan.batches {
            let first = first_segment(&plan.units[batch.units[0]].stem).to_string();
            for &index in &batch.units {
                assert_eq!(
                    first_segment(&plan.units[index].stem),
                    first,
                    "{:?}",
                    plan.batches
                );
            }
        }
        assert_eq!(plan.batches.len(), 3, "{:?}", plan.batches);
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:3efc038bcf44bcb431e125c96c27dfc30c04a8243d18fe29c5ce9f0ae2f7388e
    #[test]
    fn a_dependent_planned_only_for_its_dependency_joins_its_batch_across_directories() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/core/b.lfy", "d B: `A b` {\n  $x = string;\n}\n")
            .write(
                "def/other/a.lfy",
                "use \"../core/b\";\n\nd A {\n  $b = B;\n}\n",
            );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));

        fixture.write(
            "def/core/b.lfy",
            "d B: `A b` {\n  $x = string;\n  $y = number;\n}\n",
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &maps, &[], &[]);
        let (b_index, b) = unit_of(&workspace, &plan, "def/core/b.lfy");
        let (a_index, a) = unit_of(&workspace, &plan, "def/other/a.lfy");
        assert_eq!(b.reason, Some(Reason::Changed));
        assert_eq!(a.reason, Some(Reason::Dependency));
        assert_ne!(first_segment(&a.stem), first_segment(&b.stem));
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].units, [b_index, a_index]);
        assert_eq!(plan.batches[0].identifier, "core/b+1");
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:0a1b3b87c23f03559f003a18d9e83520d7918d97679005431f0a124cc6fba191
    #[test]
    fn every_batch_comes_after_the_batches_holding_its_dependencies() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/one/x.lfy", "d X { $x = string; }\n")
            .write("def/two/y.lfy", "use \"../one/x\";\n\nd Y { $x = X; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.batches.len(), 2, "{:?}", plan.batches);
        let mut seen: Vec<usize> = Vec::new();
        for batch in &plan.batches {
            for &index in &batch.units {
                for &dependency in &plan.units[index].dependencies {
                    assert!(
                        seen.contains(&dependency) || batch.units.contains(&dependency),
                        "{:?}",
                        plan.batches
                    );
                }
            }
            seen.extend(batch.units.iter().copied());
        }
        assert_eq!(plan.batches[0].identifier, "one/x");
        assert_eq!(plan.batches[1].identifier, "two/y");
    }

    /// A unit's lowered file is the one lowered from its file, and an entity whose code runs
    /// at compile time — a trait, or a declaration inside an `ace` — has no lowered node and
    /// is in no unit, so no output has to carry a marker for it.
    // @lfy def/generation/main.lfy:plan#plan:plan:5b80bc0e520d453402f4dd60d25a7a6155e3c799559c4db99f3631ad34f7f38c
    #[test]
    fn an_entity_with_no_lowered_node_is_in_no_unit() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "trait marked: `A trait of its own` {\n}\n\nd A {\n  $x = string;\n}\n\nace d Read {\n  $y = string;\n}\n",
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
        // @lfy def/generation/main.lfy:plan
        assert_eq!(names(&workspace, &unit.entities), ["A"]);
        // @lfy def/generation/main.lfy:plan
        assert_eq!(program.files[unit.lowered].file, unit.file);
        assert_eq!(
            program.files[unit.lowered].text,
            request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new()).sources
                ["def/a.lfy"]
        );
    }

    /// A criterion added to an entity leaves the lowered code the same, so the unit is
    /// planned for its requirements rather than for a change, and its dependent is left
    /// alone; the request says the code of such a unit did not change.
    // @lfy def/generation/main.lfy:plan
    #[test]
    fn a_new_criterion_plans_the_unit_for_its_requirements() {
        let fixture = a_and_b();
        let old = fixture.program();
        assert_bound(&old.workspace);
        let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));

        fixture.write(
            "def/b.lfy",
            &B.replace(
                "@test(",
                "@acceptanceCriteria.add({ behavior = `a new criterion` });\n  @test(",
            ),
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &maps, &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        // The lowered text is the same; only the ids of the local criteria differ.
        // @lfy def/generation/main.lfy:plan#plan:plan:eebafe9d96e4636ac4b3c7588ef0769c1d0db7e587d42508eb5ff082b339c3df
        assert_eq!(b.reason, Some(Reason::Requirements));
        assert_eq!(a.reason, None);
        let map = maps.iter().find(|map| map.source == "def/b.lfy").unwrap();
        assert_eq!(map.hash, source_hash(&program.files[b.lowered].text));
        assert_ne!(map.requirements, requirements_hash(&program, b));
        // The instructions say the code is unchanged, so only what answers for the ids that
        // were added or removed has to change.
        // @lfy def/generation/main.lfy:request#request:request:e3e9d568a3c8b1ed6a7ce84f3a68c184829608fc90f1074915bde92e31593b58
        let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new());
        assert!(
            request
                .instructions
                .contains("The code of these units is unchanged"),
            "{}",
            request.instructions
        );
    }

    /// A stem the last global review found violated plans its unit again, and a request
    /// still comes first.
    // @lfy def/generation/main.lfy:plan
    #[test]
    fn a_violated_stem_plans_the_unit_again() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
        let plan = plan_of(&program, &maps, &[], &["b".to_string()]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        // @lfy def/generation/main.lfy:plan#plan:plan:d02d6ed2f21e62878f97bfce1df0638b8152e90e9d1fbd87429ffad5d31a7be5
        assert_eq!(b.reason, Some(Reason::Violated));
        assert_eq!(a.reason, None);
        // A requested stem comes before a violated one.
        // @lfy def/generation/main.lfy:plan
        let plan = plan_of(&program, &maps, &["b".to_string()], &["b".to_string()]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(b.reason, Some(Reason::Requested));
    }

    /// A global criterion added plans no unit of its own: the global review is run instead,
    /// and only a unit whose output answers for one it finds violated is planned again.
    // @lfy def/generation/main.lfy:plan#plan:plan:fd54ec0e5416cf9dfbd2c15cfc402e49e1591a1287035e4cdd9b6db9102b82b9
    #[test]
    fn a_new_global_criterion_plans_no_unit_and_a_violated_one_plans_its_unit() {
        let fixture = a_and_b();
        let old = fixture.program();
        assert_bound(&old.workspace);
        let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));

        fixture.write(
            "def/b.lfy",
            &format!(
                "{B}\nglobal@acceptanceCriteria.add({{ behavior = `Nothing is written twice` }});\n"
            ),
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        assert_eq!(program.criteria.len(), 1, "{:?}", program.criteria);
        // Nothing is violated, so no unit is planned for the difference.
        // @lfy def/generation/main.lfy:plan#plan:plan:07ce0bbf7752ef1ed97422b09ab526f2a532aec09723bfe61a2d241bd3015a15
        let plan = plan_of(&program, &maps, &[], &[]);
        assert!(
            plan.units.iter().all(|unit| unit.reason.is_none()),
            "{:?}",
            plan.units
        );
        assert!(plan.batches.is_empty(), "{:?}", plan.batches);
        // The global review found it violated in `b`, so `b` alone is planned again.
        // @lfy def/generation/main.lfy:plan#plan:plan:abcdba359043a80f370bfaa3c6f524b52f343e3847436134859b41d978811831
        let plan = plan_of(&program, &maps, &[], &["b".to_string()]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(b.reason, Some(Reason::Violated));
        assert_eq!(a.reason, None);
    }

    // -----------------------------------------------------------------------------------
    // request
    // -----------------------------------------------------------------------------------

    // @lfy def/generation/main.lfy:request#request:request:b4934a86310e0664a47f5c551266b5e7905fd31e14c52b40314353cca9b1b0f8
    #[test]
    fn a_request_names_the_target_lists_its_units_and_quotes_their_entities() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        let batch = &plan.batches[0];
        let request = request(&program, &plan,batch, &[], &BTreeMap::new());
        // @lfy def/generation/main.lfy:request#request:request:5723990970e3d6af3ad300e749a7ad54b296bbbc6db38d3dbe14f175ffc5861e
        assert_eq!(request.batch, *batch);
        assert_eq!(request.batch.units, [b_index, a_index]);
        // The sources are the lowered text of each unit's file: the body of a fn is its
        // criteria and tests, which are read rather than generated, so what is left of one
        // is its signature.
        // @lfy def/generation/main.lfy:request
        let (_, b_unit) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(
            request.sources["def/b.lfy"],
            program.files[b_unit.lowered].text
        );
        assert!(
            request.sources["def/b.lfy"].contains("fn make(x: string): `Makes a b` => B;"),
            "{}",
            request.sources["def/b.lfy"]
        );
        assert!(
            !request.sources["def/b.lfy"].contains("@acceptanceCriteria"),
            "{}",
            request.sources["def/b.lfy"]
        );
        assert!(request.sources["def/a.lfy"].contains("d A: `An a` {"));
        // The requirements hold every local criterion and test of the file, each with its
        // id: one criterion and one test, both of `make`.
        // @lfy def/generation/main.lfy:request
        assert_eq!(request.requirements["def/b.lfy"].len(), 2);
        assert!(
            request.requirements["def/b.lfy"]
                .iter()
                .all(|requirement| requirement.id().starts_with("make:make:")),
            "{:?}",
            request.requirements["def/b.lfy"]
        );
        // Nothing of this project is added to global, so there is no global requirement.
        // @lfy def/generation/main.lfy:request
        assert!(request.globals.is_empty(), "{:?}", request.globals);
        assert!(request.previous.is_empty());
        // @lfy def/generation/main.lfy:request
        assert!(request.existing.is_empty());
        // Both units are in the batch, so nothing is an interface.
        // @lfy def/generation/main.lfy:request
        assert!(request.interfaces.is_empty());
        // @lfy def/generation/main.lfy:request
        let marker = workspace.targets[0].marker;
        // The marker extends nothing, so its own criteria are the whole guidance.
        assert_eq!(
            request.guidance,
            model::criteria_of(&workspace.model, marker)
        );
        assert_eq!(request.guidance.len(), 1);
        // The project's own native dependencies, then those of the target's package.
        // @lfy def/generation/main.lfy:request#request:request:e910cf585baaf5e05d5fe9f5bd4b5cab7296dc4193c10d17d0c3d61829985936
        assert_eq!(
            request
                .native_dependencies
                .iter()
                .map(|dependency| dependency.identifier.as_str())
                .collect::<Vec<_>>(),
            ["serde_json", "sha2"]
        );

        let text = &request.instructions;
        assert!(text.contains("compiler for the target `rust`"), "{text}");
        assert!(text.contains("- `b` (`def/b.lfy`)"), "{text}");
        assert!(text.contains("- `a` (`def/a.lfy`)"), "{text}");
        assert!(text.contains("under `src`"), "{text}");
        assert!(text.contains("### `def/a.lfy` (stem `a`)"), "{text}");
        assert!(text.contains("#### `A` (data: DataDeclaration)"), "{text}");
        // The entity's lowered code, not its source: the documentation and the body a
        // declaration is read for are gone.
        // @lfy def/generation/main.lfy:request
        assert!(text.contains("d A: `An a` {\n  $b = B;\n}"), "{text}");
        assert!(text.contains("Definition: An a"), "{text}");
        assert!(
            text.contains("Each unit becomes one module named after its stem"),
            "{text}"
        );
        assert!(text.contains("`serde_json` (cargo), version `1`"), "{text}");
        assert!(text.contains("`sha2` (cargo), any version"), "{text}");
        assert!(
            text.contains("The units of this batch depend on no unit outside it."),
            "{text}"
        );
        // Markers name entities, never lines.
        // @lfy def/generation/main.lfy:request
        assert!(text.contains("Never write a line number."), "{text}");
        assert!(text.contains("`@lfy def/b.lfy:B`"), "{text}");
        // The three report lines.
        // @lfy def/generation/main.lfy:request
        assert!(text.contains("`ELFIE: DONE`"), "{text}");
        assert!(text.contains("`ELFIE: BLOCKED: `"), "{text}");
        assert!(text.contains("`ELFIE: CLARIFY: `"), "{text}");
        assert!(text.contains("agent server"), "{text}");
        // The standard library is bound, never generated.
        // @lfy def/generation/main.lfy:request
        assert!(
            text.contains("carry `builtin` are bound to what the guidance"),
            "{text}"
        );
        assert!(
            text.contains("written out in full is translated where it is used"),
            "{text}"
        );
        assert!(!text.contains("## Existing outputs"), "{text}");
        // A criterion is never restated in an output, only named by its id.
        // @lfy def/generation/main.lfy:request
        assert!(
            text.contains("Never restate a criterion or a test in an output"),
            "{text}"
        );

        // The sections come in the definition's order.
        // @lfy def/generation/main.lfy:request#request:request:c5fe505e7af909d45f54aa7829eb8a6c41b84a47fc4c63c6d528861647a078f9
        let headings = [
            "# Compiling",
            "## Where the outputs go",
            "## Guidance",
            "## Native dependencies",
            "## Interfaces",
            "## The units to compile",
            "## Global criteria and tests",
            "## Criteria and tests in an output",
            "## The standard library",
            "## Rules for kinds",
            "## Markers",
            "## Tests",
            "## The agent server",
            "## How to report",
        ];
        let positions: Vec<usize> = headings
            .iter()
            .map(|heading| {
                text.find(heading)
                    .unwrap_or_else(|| panic!("{heading} is missing:\n{text}"))
            })
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{positions:?}"
        );
    }

    // @lfy def/generation/main.lfy:request
    #[test]
    fn a_request_quotes_every_criterion_and_test_of_an_entity() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        let batch = Batch {
            units: vec![b_index],
            identifier: "b".to_string(),
        };
        let request = request(&program, &plan, &batch, &[], &BTreeMap::new());
        let text = &request.instructions;
        assert!(
            text.contains("#### `make` (agent function: AgentFunctionDeclaration)"),
            "{text}"
        );
        // Each criterion and each test is on its own line, with its id.
        // @lfy def/generation/main.lfy:request
        let (criteria, tests) = {
            let (_, unit) = unit_of(&workspace, &plan, "def/b.lfy");
            let entity = *unit.entities.last().expect("make is an entity of b");
            requirement_ids(&program.files[unit.lowered], entity)
        };
        assert!(
            text.contains(&format!(
                "- `{}` — When x is empty: the b holds x",
                criteria[0]
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "- `{}` — Input `[\"y\"]` gives `B@like(`holding y`)`",
                tests[0]
            )),
            "{text}"
        );
    }

    /// Every global criterion and test of the program is in one section of its own, each
    /// once with its id, and the instructions say what a unit does with one.
    // @lfy def/generation/main.lfy:request
    #[test]
    fn a_request_holds_every_global_criterion_once_with_its_id() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d A { $x = string; }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written twice` });\n",
        );
        let program = fixture.program();
        assert_bound(&program.workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new());
        // @lfy def/generation/main.lfy:request
        assert_eq!(
            request
                .globals
                .iter()
                .map(|requirement| requirement.id())
                .collect::<Vec<_>>(),
            [program.criteria[0].id.as_str()]
        );
        let text = &request.instructions;
        assert!(text.contains("## Global criteria and tests"), "{text}");
        assert!(
            text.contains(&format!(
                "- `{}` — Nothing is written twice",
                program.criteria[0].id
            )),
            "{text}"
        );
        // @lfy def/generation/main.lfy:request
        assert!(text.contains("These hold across the whole program"), "{text}");
        assert!(
            text.contains("the global review checks each one once"),
            "{text}"
        );
        // It is not in the unit's own section: a local criterion and a global one are kept
        // apart.
        // @lfy def/generation/main.lfy:request
        assert_eq!(text.matches(program.criteria[0].id.as_str()).count(), 1, "{text}");
    }

    /// A marker extending a chain of traits, one of them applied with arguments: the
    /// guidance is the marker's own criteria, then those of every trait it extends,
    /// nearest first, each trait once, with every template value evaluated.
    // @lfy def/generation/main.lfy:request#request:request:fb41725e5b0fca4d5183fe2e3d8c131fb6b11eece1a3dcc2836eec83197e69ab
    #[test]
    fn the_guidance_holds_the_markers_criteria_then_those_of_every_trait_it_extends() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write(
                "targets/rust/main.lfy",
                "trait base extends target: `A base` {\n\
                 \x20 where (`An output is written`) -> `Its path is under the output directory`;\n\
                 }\n\n\
                 trait layout(root: string) extends base: `A layout` {\n\
                 \x20 where (`A unit is written`) -> `Its output is {{root}} then its stem`;\n\
                 }\n\n\
                 trait targetLanguage extends base: `A language` {\n\
                 \x20 where (`A d declaration is built`) -> `It becomes a type`;\n\
                 }\n\n\
                 trait rust extends targetLanguage, layout(\"crates\"): `Rust` {\n\
                 \x20 @acceptanceCriteria\n\
                 \x20   .add({ behavior = `Each unit becomes one module named after its stem` })\n\
                 \x20   .add({ behavior = `Outputs are built for {{@identifier}}` });\n\
                 }\n\n\
                 rust.apply(global);\n",
            )
            .write("def/a.lfy", "d A { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        let request = request(&program, &plan,&plan.batches[0], &[], &BTreeMap::new());
        let behaviors: Vec<String> = request
            .guidance
            .iter()
            .map(|criterion| criterion.behavior.clone().unwrap_or_default().join(" "))
            .collect();
        // The marker's own first, with its template value evaluated for the marker; then
        // targetLanguage and layout, in extends order; then base, reached through both
        // and contributing once.
        // @lfy def/generation/main.lfy:request#request:request:f833bfab327ae02fc19b1e38fe79e92576f93942ee68bf07dea0707f680c38f6
        assert_eq!(
            behaviors,
            [
                "Each unit becomes one module named after its stem",
                "Outputs are built for rust",
                "It becomes a type",
                "Its output is crates then its stem",
                "Its path is under the output directory",
            ],
            "{:?}",
            request.guidance
        );
        // `base` is reached through `targetLanguage` and through `layout`, and contributes
        // once, at its first place.
        // @lfy def/generation/main.lfy:request#request:request:dea7eb13b1ae297c9a158429db016c52c5bd257921f2863ab04cc53dc7771671
        assert_eq!(
            behaviors
                .iter()
                .filter(|behavior| *behavior == "Its path is under the output directory")
                .count(),
            1,
            "{behaviors:?}"
        );
        // Every criterion is quoted in the instructions, in that order.
        let text = &request.instructions;
        let positions: Vec<usize> = behaviors
            .iter()
            .map(|behavior| {
                text.find(behavior.as_str())
                    .unwrap_or_else(|| panic!("{behavior} is missing:\n{text}"))
            })
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{positions:?}"
        );
        // A trait applied with arguments has its template values evaluated from them, so no
        // template is left in the guidance.
        // @lfy def/generation/main.lfy:request#request:request:367debdff9731b3e0930703972a2edde7b8946687292224290a326a1263bfb55
        assert!(!text.contains("{{root}}"), "{text}");
    }

    // @lfy def/generation/main.lfy:request#request:request:11b89fec47c462e32fe4f58c1a2c8c62a950c6893d97a50116d46bb998d8957f
    #[test]
    fn a_dependency_outside_the_batch_is_an_interface_with_its_outputs_and_entities() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
        let plan = plan_of(&program, &maps, &["a".to_string()], &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].units, [a_index]);
        let request = request(&program, &plan,&plan.batches[0], &[], &BTreeMap::new());
        assert_eq!(request.interfaces.len(), 1);
        assert_eq!(request.interfaces[0].unit, b_index);
        assert_eq!(request.interfaces[0].outputs, ["src/b.rs"]);
        assert_eq!(
            names(&workspace, &request.interfaces[0].entities),
            ["B", "make"]
        );
        let text = &request.instructions;
        assert!(text.contains("### `def/b.lfy`"), "{text}");
        assert!(
            text.contains("- `B` (data: DataDeclaration): A b"),
            "{text}"
        );
        assert!(text.contains("  - `x` (member) — type `string`"), "{text}");
        assert!(
            text.contains("- `make` (agent function: AgentFunctionDeclaration): Makes a b"),
            "{text}"
        );
        assert!(text.contains("never edit their outputs"), "{text}");
    }

    // @lfy def/generation/main.lfy:request
    #[test]
    fn existing_outputs_are_kept_only_where_they_are_among_the_units_outputs() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let maps = current_maps(&program,&plan_of(&program, &[], &[], &[]));
        let plan = plan_of(&program, &maps, &["a".to_string()], &[]);
        let existing = [
            Output {
                path: "src/a.rs".to_string(),
                text: "// old".to_string(),
            },
            Output {
                path: "src/other.rs".to_string(),
                text: "// other".to_string(),
            },
        ];
        let previous = BTreeMap::from([("def/a.lfy".to_string(), "d A {}".to_string())]);
        let with_previous = request(&program, &plan,&plan.batches[0], &existing, &previous);
        // @lfy def/generation/main.lfy:request#request:request:ce520ffba1e8298b5ff2f6a6376e60e59a17343e640d10ed179732cd788b8075
        assert_eq!(with_previous.existing.len(), 1);
        assert_eq!(with_previous.existing["src/a.rs"], "// old");
        assert_eq!(with_previous.previous, previous);
        let text = &with_previous.instructions;
        // Only what the difference requires changes, and the markers of unchanged items are
        // kept.
        // @lfy def/generation/main.lfy:request#request:request:c2e330513b32bd4510edb410692c6f3a7c933c58a95ffda258d22fca4c4328a9
        assert!(text.contains("## Existing outputs"), "{text}");
        assert!(
            text.contains("keep the markers of unchanged items"),
            "{text}"
        );
        // The outputs of a unit that has some go at the paths its `Unit.outputs` names.
        // @lfy def/generation/main.lfy:request#request:request:5d2a38caea26eaee56682ab34505d4f806c6b5e6b71bb08c89cfe8612b0961ad
        assert!(text.contains("write to the same paths"), "{text}");
        assert!(text.contains("d A {}"), "{text}");

        let without = request(
            &program,
            &plan,
            &plan.batches[0],
            &existing,
            &BTreeMap::new(),
        );
        assert!(
            without
                .instructions
                .contains("reconcile the existing output with the source"),
            "{}",
            without.instructions
        );
    }

    // -----------------------------------------------------------------------------------
    // outcomeOf
    // -----------------------------------------------------------------------------------

    fn accepted_verdict() -> Verdict {
        Verdict {
            accepted: true,
            problems: Vec::new(),
            source_maps: Vec::new(),
            outputs: Vec::new(),
        }
    }

    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:e5324f4bea05fae7e9593527cb97051401fa31bf436b8e19acaf8723aa328256
    #[test]
    fn a_report_ending_in_done_with_accepted_verdicts_is_accepted() {
        let outcome = outcome_of("I wrote it.\nELFIE: DONE", vec![accepted_verdict()]);
        // @lfy def/generation/main.lfy:outcomeOf
        assert_eq!(outcome.kind, OutcomeKind::Accepted);
        assert_eq!(outcome.message, "");
        // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:2de4f51d5b06b9099294d93a631c4c53a3fac5e7cde4436e300269b119ed21b8
        assert_eq!(outcome.verdicts.len(), 1);
    }

    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:7b6eac4728a15e61226e1f610c492516a8c084cb43b331c9a358884b2a104e73
    #[test]
    fn a_report_ending_in_clarify_is_a_clarification_with_the_question() {
        let outcome = outcome_of("ELFIE: CLARIFY: should Range.end be inclusive?", Vec::new());
        // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:53de4f363ea26bf3a1370bab8f9f83cecf2b3eba54674bdb1eeabebefb27673c
        assert_eq!(outcome.kind, OutcomeKind::Clarification);
        assert_eq!(outcome.message, "should Range.end be inclusive?");
        assert!(outcome.verdicts.is_empty());
    }

    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:c5c86395d450173628730a07f7818f5a9b3639fe8116721227b43fb2a61abb5d
    #[test]
    fn a_report_ending_in_blocked_is_blocked_with_the_reason_and_every_line_after_it() {
        let outcome = outcome_of(
            "ELFIE: BLOCKED: [[tokenAt]] contradicts [[nodesAt]] on empty files\nand on one-line files",
            Vec::new(),
        );
        // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:a4c5e6d3f292645f7f6470a7965c4e918dc86d3ea208ea1161e7aea9b8d54e9a
        assert_eq!(outcome.kind, OutcomeKind::Blocked);
        assert_eq!(
            outcome.message,
            "[[tokenAt]] contradicts [[nodesAt]] on empty files\nand on one-line files"
        );
        // The last such line decides, whatever came before it.
        let outcome = outcome_of("ELFIE: DONE\nELFIE: BLOCKED: no", Vec::new());
        assert_eq!(outcome.kind, OutcomeKind::Blocked);
        assert_eq!(outcome.message, "no");
    }

    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:0fccd9b62ccb962587945c80c1ea93a6c4a0a14d715a11382de9069f057a8717
    #[test]
    fn a_report_with_nothing_and_no_verdicts_has_failed() {
        let outcome = outcome_of("", Vec::new());
        assert_eq!(outcome.kind, OutcomeKind::Failed);
        assert_eq!(outcome.message, "the compiler reported nothing");
        assert!(outcome.verdicts.is_empty());
        // Prose alone, with no line beginning with `ELFIE:`, has failed too.
        // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:19216d3a88ec8d3f27ae0fb6d97d169f662c7bc5dc93c0a33095dc2abed25312
        assert_eq!(
            outcome_of("I had a look around.", Vec::new()).kind,
            OutcomeKind::Failed
        );
    }

    /// A report that ended but left no verdict is accepted: the compiler said it was done,
    /// and no verdict says otherwise.
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:7493907472b5ae9311e57d728e2778642ee5cb03f7a60f07653450479788690a
    #[test]
    fn a_report_that_ended_with_no_verdicts_is_accepted() {
        let outcome = outcome_of("ELFIE: DONE", Vec::new());
        assert_eq!(outcome.kind, OutcomeKind::Accepted);
        assert_eq!(outcome.message, "");
        assert!(outcome.verdicts.is_empty());
    }

    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:c0456656ba1f48f003866e5234345c26bc86461c3655c59997978e13b5ff9d25
    #[test]
    fn a_rejected_verdict_is_rejected_with_its_first_problem() {
        let rejected = Verdict {
            accepted: false,
            problems: vec![
                "the entity B has no marker".to_string(),
                "and another".to_string(),
            ],
            source_maps: Vec::new(),
            outputs: Vec::new(),
        };
        let outcome = outcome_of("ELFIE: DONE", vec![accepted_verdict(), rejected]);
        assert_eq!(outcome.kind, OutcomeKind::Rejected);
        assert_eq!(outcome.message, "the entity B has no marker");
        assert_eq!(outcome.verdicts.len(), 2);
    }

    // -----------------------------------------------------------------------------------
    // accept
    // -----------------------------------------------------------------------------------

    /// One file holding entities A and B, both built.
    fn a_with_two_entities() -> Fixture {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "\n\n/// A.\nd A {\n  $x = string;\n}\n\nd B {\n  $y = string;\n}\n",
        );
        fixture
    }

    fn named_output(text: &str) -> Output {
        Output {
            path: "src/a.rs".to_string(),
            text: text.to_string(),
        }
    }

    /// The plan, the request, and the unit index of `def/a.lfy`.
    fn one_unit(program: &Program) -> (Plan, Request, usize) {
        let plan = plan_of(program, &[], &[], &[]);
        let (index, _) = unit_of(&program.workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![index],
            identifier: "a".to_string(),
        };
        let request = request(program, &plan, &batch, &[], &BTreeMap::new());
        (plan, request, index)
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:499eff9789d84e951586fdadbe7b46cae6dc863092bfb6f280bd8ab26aba5483
    #[test]
    fn outputs_with_a_marker_per_entity_are_accepted_with_one_source_map() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let (plan, request, index) = one_unit(&program);
        assert_eq!(names(&workspace, &plan.units[index].entities), ["A", "B"]);
        let output = named_output(
            "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/a.lfy:B\npub struct B;\n",
        );
        let verdict = accept(
            &program,
            &plan,
            &request,
            index,
            std::slice::from_ref(&output),
        );
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert!(verdict.problems.is_empty());
        assert_eq!(verdict.source_maps.len(), 1);
        // A marker that names an entity is left as written.
        // @lfy def/generation/main.lfy:accept#accept:accept:3464ba36f8e1e5d762286978e6be41278e751aded80a6bf152a33a3c4c2cb3a4
        assert_eq!(verdict.outputs, [output]);
        let map = &verdict.source_maps[0];
        assert_eq!(map.target, "rust");
        assert_eq!(map.output, "src/a.rs");
        assert_eq!(map.source, "def/a.lfy");
        // Every hash of the source map is derived here, never taken from the compiler's text.
        // @lfy def/generation/main.lfy:accept#accept:accept:6a1ede84efd02401a68ed3e9a5852d6dfc704e582532c620de68595d4fe74150
        assert_eq!(map.hash, source_hash(&request.sources["def/a.lfy"]));
        assert_eq!(
            map.signature,
            interface_signature(&workspace, &plan.units[index])
        );
        assert!(map.dependencies.is_empty());
        assert!(parse_rfc3339(&map.generated).is_some(), "{}", map.generated);
        // The lines are derived from the model, never copied from the compiler.
        assert_eq!(
            map.markers
                .iter()
                .map(|marker| (marker.entity.clone(), marker.line))
                .collect::<Vec<_>>(),
            [(Some("A".to_string()), 4), (Some("B".to_string()), 8)]
        );
        // A marker followed by another ends on the line before it.
        // @lfy def/generation/main.lfy:accept#accept:accept:0c0071afea5332b7a7a9332e540fc4d992d28a802923c64d998295878d840795
        assert_eq!((map.markers[0].output_line, map.markers[0].end), (1, 3));
        // The last marker of an output ends on its last line.
        // @lfy def/generation/main.lfy:accept#accept:accept:3557b46ff0f36e90d1b8872dbb99584bf6a38b73bb8e7d7336c61e83b2497d16
        assert_eq!((map.markers[1].output_line, map.markers[1].end), (4, 5));
        // So the regions partition the output from its first marker on.
        // @lfy def/generation/main.lfy:accept#accept:accept:a6921061f9e2ca23b5617e1ac741a1b127dbde2de5a1a64157e32a9d5a0341a0
        assert_eq!(
            map.markers
                .iter()
                .map(|marker| (marker.output_line, marker.end))
                .collect::<Vec<_>>(),
            [(1, 3), (4, 5)]
        );
        assert!(
            map.markers
                .windows(2)
                .all(|pair| pair[0].end + 1 == pair[1].output_line),
            "{:?}",
            map.markers
        );
        // The source map round-trips through the plan: the unit is now up to date.
        let plan = plan_of(&program, &verdict.source_maps, &[], &[]);
        assert_eq!(plan.units[index].reason, None);
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:dd9767e93f188ad433adfb81d2b14e5bcc518b6e2d80e9ecca17d5a03a2f1ab3
    #[test]
    fn an_entity_without_a_marker_is_rejected_by_name() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        // A marker on the documentation line of A counts for A; nothing names B.
        let output = named_output("// @lfy def/a.lfy:3\npub struct A;\n");
        let verdict = accept(&program, &plan, &request, index,&[output]);
        // @lfy def/generation/main.lfy:accept#accept:accept:30f5c9fea521522b0db93130ff39c1ed29c688ede1875cc3e82bfa00376c059a
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(
            verdict.problems[0].contains("the entity B"),
            "{}",
            verdict.problems[0]
        );
        assert!(verdict.source_maps.is_empty());
        assert!(verdict.outputs.is_empty());
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:a3a5cfa7740ebf2dbb44280433dc77dea0ef209c95cea1f7d5ef57ed2c8f471b
    #[test]
    fn a_marker_naming_a_line_inside_a_declaration_is_rewritten_to_name_it() {
        let fixture = Fixture::with_rust_target();
        // `A` is declared on line 3.
        fixture.write("def/a.lfy", "// a file\n\nd A {\n  $x = string;\n}\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let (plan, request, index) = one_unit(&program);
        assert_eq!(names(&workspace, &plan.units[index].entities), ["A"]);
        let output = named_output("// @lfy def/a.lfy:3\npub struct A;\n");
        let verdict = accept(&program, &plan, &request, index,&[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // @lfy def/generation/main.lfy:accept#accept:accept:20bac879246718342cd4a1788980b76a817693a0c3b6bd11addd887e9579c378
        assert_eq!(
            verdict.outputs[0].text,
            "// @lfy def/a.lfy:A\npub struct A;\n"
        );
        assert_eq!(
            verdict.source_maps[0].markers[0].entity.as_deref(),
            Some("A")
        );
        assert_eq!(verdict.source_maps[0].markers[0].line, 3);
        // A line inside a member's declaration names the member.
        let output = named_output(
            "// @lfy def/a.lfy:3\npub struct A {\n  // @lfy def/a.lfy:4\n  x: String,\n}\n",
        );
        let verdict = accept(&program, &plan, &request, index,&[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert!(
            verdict.outputs[0].text.contains("// @lfy def/a.lfy:A.x"),
            "{}",
            verdict.outputs[0].text
        );
    }

    /// Every line a source map keeps is the model's: a marker on A's documentation line
    /// records the line A is declared on rather than the number written, a marker on B's
    /// last line records B's first, and a line no declaration covers is recorded for
    /// nothing, since the model gives no line to derive for it.
    // @lfy def/generation/main.lfy:accept
    #[test]
    fn the_lines_of_a_source_map_are_derived_and_never_the_compilers() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let (plan, request, index) = one_unit(&program);
        // Line 1 is blank and inside no declaration, line 3 is A's documentation and line 4
        // its declaration, and line 10 closes B, which is declared on line 8.
        let output = named_output(
            "// @lfy def/a.lfy:1\n// @lfy def/a.lfy:3\npub struct A;\n// @lfy def/a.lfy:10\npub struct B;\n",
        );
        let verdict = accept(&program, &plan, &request, index,&[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert_eq!(
            verdict.source_maps[0]
                .markers
                .iter()
                .map(|marker| (marker.entity.clone(), marker.output_line, marker.line))
                .collect::<Vec<_>>(),
            [
                (Some("A".to_string()), 2, 4),
                (Some("B".to_string()), 4, 8)
            ]
        );
        // Each is rewritten to the name the line resolved to, though neither name was
        // spelled with the line the map keeps.
        assert_eq!(
            verdict.outputs[0].text,
            "// @lfy def/a.lfy:1\n// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/a.lfy:B\npub struct B;\n"
        );
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:e4e4df7a903109a1bd027ea4215cea9b59b6e1d4a94e01fff48a70c4583d0225
    #[test]
    fn no_outputs_are_rejected() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let verdict = accept(&program, &plan, &request, index,&[]);
        assert!(!verdict.accepted);
        assert!(
            verdict.problems[0].contains("no output"),
            "{:?}",
            verdict.problems
        );
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:f6666f7e7b09c250e8f74ad9d63d505ac8054121912ba700371d76d9f9396fd9
    #[test]
    fn an_output_outside_the_output_directory_is_rejected_by_path() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let mut output = named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n");
        output.path = "srcx/a.rs".to_string();
        let verdict = accept(&program, &plan, &request, index,&[output]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(
            verdict.problems[0].contains("srcx/a.rs"),
            "{}",
            verdict.problems[0]
        );
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:a1c947537070babac5d735f6d5d16856f9de88cadd132d799c85c366ee84bddf
    #[test]
    fn a_marker_naming_an_entity_the_file_does_not_declare_is_rejected_by_output_line_and_name() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let output =
            named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n// @lfy def/a.lfy:Nowhere\n");
        let verdict = accept(&program, &plan, &request, index,&[output]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(
            verdict.problems[0].starts_with("src/a.rs:3:"),
            "{}",
            verdict.problems[0]
        );
        assert!(
            verdict.problems[0].contains("Nowhere"),
            "{}",
            verdict.problems[0]
        );
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:a3e98c9cbbd8253131d2537c09808e4e34f57acedaa9ba29ea709a7fffa209a1
    #[test]
    fn a_marker_naming_a_line_past_the_last_is_rejected_by_output_line() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let output =
            named_output("// @lfy def/a.lfy:A\n\n// @lfy def/a.lfy:99\n// @lfy def/a.lfy:B\n");
        let verdict = accept(&program, &plan, &request, index,&[output]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(
            verdict.problems[0].starts_with("src/a.rs:3:"),
            "{}",
            verdict.problems[0]
        );
        assert!(
            verdict.problems[0].contains("line 99"),
            "{}",
            verdict.problems[0]
        );
    }

    /// A marker naming a file the program does not hold is a fixture or prose, not a claim
    /// about the program, so it is ignored and not recorded.
    // @lfy def/generation/main.lfy:accept#accept:accept:8d4634ba98434cb69bdbf4d30ce208612c075efe72c36db22099bd9af0908f8e
    #[test]
    fn a_marker_naming_a_file_that_is_not_in_the_program_is_ignored() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let output = named_output(
            "// @lfy def/a.lfy:A\n// @lfy def/b.lfy:8\n// @lfy def/b.lfy:Nowhere\n// @lfy def/a.lfy:B\n",
        );
        let verdict = accept(&program, &plan, &request, index,&[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // Neither marker of the file outside the program is recorded.
        let markers = &verdict.source_maps[0].markers;
        assert_eq!(
            markers
                .iter()
                .map(|marker| marker.entity.clone())
                .collect::<Vec<_>>(),
            [Some("A".to_string()), Some("B".to_string())]
        );
        // The output is written back with the text of those markers untouched.
        assert!(
            verdict.outputs[0].text.contains("// @lfy def/b.lfy:8"),
            "{}",
            verdict.outputs[0].text
        );
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:ab8082306f2b543640639148fab629f4f62757525189b63950b8409311fca21f
    #[test]
    fn an_output_may_hold_markers_for_other_files_of_the_program() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![index],
            identifier: "a".to_string(),
        };
        let request = request(&program, &plan,&batch, &[], &BTreeMap::new());
        let output = named_output(
            "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:B\npub struct B;\n",
        );
        let verdict = accept(&program, &plan, &request, index,&[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // The marker of the other file is kept, and its line is derived from that file.
        let markers = &verdict.source_maps[0].markers;
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[1].file, "def/b.lfy");
        assert_eq!(markers[1].entity.as_deref(), Some("B"));
        assert_eq!(markers[1].line, 1);
        // @lfy def/generation/main.lfy:accept
        assert_eq!(verdict.source_maps[0].dependencies.len(), 1);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(
            verdict.source_maps[0].dependencies["def/b.lfy"],
            interface_signature(&workspace, b)
        );

        // A marker of another file may name that file's entity and answer for that file's own
        // local criterion: the id belongs to that file's unit, so this unit does not check it.
        // An output shared by two units holds the other unit's markers and ids, and checking
        // them here would make the shared output unacceptable to one of them.
        // @lfy def/generation/main.lfy:accept#accept:accept:45ec36176d2c984237e6519d40944832cbc76e75c44d4a6e0ff356725704fe84
        let make = *b.entities.last().expect("make is an entity of b");
        let (criteria, _) = requirement_ids(&program.files[b.lowered], make);
        let output = named_output(&format!(
            "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:make#{}\nfn make() {{}}\n",
            criteria[0]
        ));
        let verdict = accept(&program, &plan, &request, index, &[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // An id no file of the program knows is not checked either, when the marker names
        // another file: only a marker of this unit's own file makes a claim this unit answers
        // for.
        // @lfy def/generation/main.lfy:accept#accept:accept:45ec36176d2c984237e6519d40944832cbc76e75c44d4a6e0ff356725704fe84
        let output = named_output(
            "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:make#make:make:nowhere\nfn make() {}\n",
        );
        let verdict = accept(&program, &plan, &request, index, &[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
    }

    /// A marker of the unit's own file may answer for a local criterion of the unit or for a
    /// global one, and for nothing else; the source map records the hash of the unit's
    /// requirement ids.
    // @lfy def/generation/main.lfy:accept#accept:accept:45ec36176d2c984237e6519d40944832cbc76e75c44d4a6e0ff356725704fe84
    #[test]
    fn a_marker_answering_for_an_unknown_requirement_is_rejected_by_output_line_and_id() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d A: `An a` {\n  @acceptanceCriteria.add({ behavior = `it holds` });\n}\nglobal@acceptanceCriteria.add({ behavior = `nothing is written twice` });\n",
        );
        let program = fixture.program();
        assert_bound(&program.workspace);
        let (plan, request, index) = one_unit(&program);
        let unit = &plan.units[index];
        let (local, _) = requirement_ids(&program.files[unit.lowered], unit.entities[0]);
        assert_eq!(local.len(), 1, "{local:?}");
        let global = program.criteria[0].id.clone();

        // An id of a local criterion and one of a global criterion are both known, so the
        // outputs are accepted and earn their source maps.
        // @lfy def/generation/main.lfy:accept#accept:accept:6a1ede84efd02401a68ed3e9a5852d6dfc704e582532c620de68595d4fe74150
        let output = named_output(&format!(
            "// @lfy def/a.lfy:A#{}\npub struct A;\n// @lfy def/a.lfy:A#{global}\nfn holds() {{}}\n",
            local[0]
        ));
        let verdict = accept(&program, &plan, &request, index, &[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // @lfy def/generation/main.lfy:accept#accept:accept:6a1ede84efd02401a68ed3e9a5852d6dfc704e582532c620de68595d4fe74150
        assert_eq!(
            verdict.source_maps[0].requirements,
            requirements_hash(&program, unit)
        );
        assert_eq!(
            verdict.source_maps[0].hash,
            source_hash(&program.files[unit.lowered].text)
        );

        // An id of neither, on a marker of the unit's own file, is rejected by output line
        // and id.
        // @lfy def/generation/main.lfy:accept#accept:accept:45ec36176d2c984237e6519d40944832cbc76e75c44d4a6e0ff356725704fe84
        let output = named_output("// @lfy def/a.lfy:A#A:A:nowhere\npub struct A;\n");
        let verdict = accept(&program, &plan, &request, index, &[output]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(
            verdict.problems[0].starts_with("src/a.rs:1:"),
            "{}",
            verdict.problems[0]
        );
        assert!(
            verdict.problems[0].contains("A:A:nowhere"),
            "{}",
            verdict.problems[0]
        );
    }

    /// Acceptance is structural: an output whose text no compiler of the target would accept
    /// is accepted all the same, because the guidance says how to build it and the caller
    /// runs it.
    // @lfy def/generation/main.lfy:accept#accept:accept:8a6da7604e29c81dbcd555ed276d68432afa28c292704f6dbde91732e283b4de
    #[test]
    fn whether_an_output_builds_is_not_checked_here() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let output = named_output(
            "// @lfy def/a.lfy:A\nthis is not Rust at all (((\n// @lfy def/a.lfy:B\nnor is this\n",
        );
        let verdict = accept(&program, &plan, &request, index, &[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert_eq!(verdict.source_maps.len(), 1);
    }

    // -----------------------------------------------------------------------------------
    // sourceMapsOf, record
    // -----------------------------------------------------------------------------------

    /// One source map of the target `rust` for an output and the source it came from.
    fn map_for(output: &str, source: &str) -> SourceMap {
        SourceMap {
            target: "rust".to_string(),
            output: output.to_string(),
            source: source.to_string(),
            hash: source_hash(source),
            requirements: source_hash("requirements"),
            signature: source_hash("interface"),
            dependencies: BTreeMap::new(),
            generated: "2026-09-18T10:00:00Z".to_string(),
            markers: Vec::new(),
        }
    }

    fn outputs_of(maps: &[SourceMap]) -> Vec<&str> {
        maps.iter().map(|map| map.output.as_str()).collect()
    }

    /// A unit's map file is `elfie-compile/maps`, the target's identifier, and the unit's
    /// stem with the extension `.json`, under the workspace root.
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:32c5b3bb06791ec929391035efa9799397f878add9b5099e6aaa5a3f15b71743
    #[test]
    fn a_units_map_file_is_under_elfie_compile_maps_by_target_and_stem() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/cli/main.lfy", "d Cli { $x = string; }\n");
        let program = fixture.program();
        assert_bound(&program.workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
        assert_eq!(unit.stem, "cli/main");
        assert_eq!(
            map_file(&program.workspace, unit),
            fixture.root.join("elfie-compile/maps/rust/cli/main.json")
        );
    }

    /// Every map file of the target's folder is read, in the sorted order of its files and
    /// then in the order each file lists them, and a map whose output is gone is left out.
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:ede8434812c8a4f5743a0062976be24f2518397279ef2820986a6880aabf5940
    #[test]
    fn the_maps_of_a_target_are_read_in_file_then_list_order() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("crates/a/src/a.rs", "// a\n")
            .write("crates/b/src/b.rs", "// b\n");
        write_source_maps(
            &fixture.root.join("elfie-compile/maps/rust/a.json"),
            &[map_for("crates/a/src/a.rs", "def/a.lfy")],
        )
        .unwrap();
        write_source_maps(
            &fixture.root.join("elfie-compile/maps/rust/b.json"),
            &[map_for("crates/b/src/b.rs", "def/b.lfy")],
        )
        .unwrap();
        let workspace = load(&fixture.root);
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:e3ab53d40653267f2e160d4a7f3251350dc8120cab5217a982112c74085d6a81
        let maps = source_maps_of(&workspace, None);
        assert_eq!(
            outputs_of(&maps),
            ["crates/a/src/a.rs", "crates/b/src/b.rs"]
        );
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:3cc1ca49afa6466d9c8391008610df4dc1ba5877a6018f5ec20137f18d425209
        assert_eq!(outputs_of(&source_maps_of(&workspace, Some("rust"))).len(), 2);
        assert!(source_maps_of(&workspace, Some("other")).is_empty());

        // The map of an output that is gone is left out.
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:777506c6ffecf859d5436778cd8e1997eef47cd57e5f8f41754058bbe3117d23
        fs::remove_file(fixture.root.join("crates/b/src/b.rs")).unwrap();
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:49f3bf512172a2a718b6ee96460c1a23944304c9e259e781c8928937dbb775fc
        assert_eq!(
            outputs_of(&source_maps_of(&workspace, None)),
            ["crates/a/src/a.rs"]
        );

        // A map file that cannot be read as a JSON list of source maps contributes nothing.
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:0bccd0853ef4c61cb0a946b04b8011d43611ca0c2911d156938a87da5a3fa558
        fixture.write("elfie-compile/maps/rust/a.json", "not json");
        assert!(source_maps_of(&workspace, None).is_empty());
    }

    /// A target with no folder under `elfie-compile/maps` is read from `source-map.json`
    /// under its output directory instead, and a target with neither has no source maps.
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:b78229b17a59641dc63064a2acdaf15c44c023b6546931ffbd8ea03fe24f7a36
    #[test]
    fn a_target_with_no_folder_falls_back_to_source_map_json() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("crates/a/src/a.rs", "// a\n")
            .write("crates/b/src/b.rs", "// b\n");
        let workspace = load(&fixture.root);
        // No folder under elfie-compile/maps and no legacy file: no source maps.
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:55c87b019cd4d2d7c4a69761ea81c9bd9980381c22407c0e202211f03cfe8dc8
        assert!(source_maps_of(&workspace, None).is_empty());

        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:01d2a31fa4a7a2da04841b389365d4179bc3b4582f1e2db253c915a9a46cf5e8
        write_source_maps(
            &fixture.root.join("src/source-map.json"),
            &[
                map_for("crates/b/src/b.rs", "def/b.lfy"),
                map_for("crates/a/src/a.rs", "def/a.lfy"),
            ],
        )
        .unwrap();
        assert_eq!(
            outputs_of(&source_maps_of(&workspace, None)),
            ["crates/b/src/b.rs", "crates/a/src/a.rs"]
        );

        // Once the folder exists, the legacy file is never read.
        // @lfy def/generation/main.lfy:sourceMapsOf
        write_source_maps(
            &fixture.root.join("elfie-compile/maps/rust/a.json"),
            &[map_for("crates/a/src/a.rs", "def/a.lfy")],
        )
        .unwrap();
        assert_eq!(
            outputs_of(&source_maps_of(&workspace, None)),
            ["crates/a/src/a.rs"]
        );
        // Nothing is written: the legacy file is left where it was, and nothing under
        // elfie-compile/cache is read.
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:911e7932c6b0a20a3c09d20628728fdc86adf9336718ad41bd9652debf4f9a69
        assert!(fixture.root.join("src/source-map.json").is_file());
        assert!(!fixture.root.join("elfie-compile/cache").exists());
    }

    /// The plan, and the unit of `def/a.lfy`, of a project whose `def` holds `a.lfy` and
    /// `b.lfy` and whose maps folder holds a map file for each.
    fn recorded() -> (Fixture, Program, Plan) {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/a.lfy", "d A { $x = string; }\n")
            .write("def/b.lfy", "d B { $x = string; }\n")
            .write("crates/a/src/a.rs", "// a\n")
            .write("crates/b/src/b.rs", "// b\n");
        write_source_maps(
            &fixture.root.join("elfie-compile/maps/rust/a.json"),
            &[map_for("crates/a/src/a.rs", "def/a.lfy")],
        )
        .unwrap();
        write_source_maps(
            &fixture.root.join("elfie-compile/maps/rust/b.json"),
            &[map_for("crates/b/src/b.rs", "def/b.lfy")],
        )
        .unwrap();
        let program = fixture.program();
        let plan = plan_of(&program, &[], &[], &[]);
        (fixture, program, plan)
    }

    /// Recording one unit replaces its map file and leaves every other byte for byte as it
    /// was, in output order with every object's keys alphabetical.
    // @lfy def/generation/main.lfy:record#record:record:4f6f660ecf7fc0b1fd150a14bbc1aade951cb45364ff03577f2d92ae52e33f01
    #[test]
    fn recording_one_unit_writes_its_map_file_and_leaves_every_other_alone() {
        let (fixture, program, plan) = recorded();
        let workspace = &program.workspace;
        let (_, unit) = unit_of(workspace, &plan, "def/a.lfy");
        let before = fs::read_to_string(fixture.root.join("elfie-compile/maps/rust/b.json")).unwrap();
        let mut map = map_for("crates/a/src/a.rs", "def/a.lfy");
        map.hash = source_hash("a new hash");
        let second = map_for("crates/a/src/other.rs", "def/a.lfy");

        // The unit's map file holds exactly these maps afterwards.
        // @lfy def/generation/main.lfy:record#record:record:b37dc632a905543a14635d6123a188e5db91d356a51971cfd3b14abcad484e5a
        assert!(record(workspace, unit, &[second.clone(), map.clone()]));
        let path = fixture.root.join("elfie-compile/maps/rust/a.json");
        let recorded = read_source_maps(&path);
        // In output order, whatever order they were given in.
        // @lfy def/generation/main.lfy:record
        assert_eq!(
            outputs_of(&recorded),
            ["crates/a/src/a.rs", "crates/a/src/other.rs"]
        );
        assert_eq!(recorded[0].hash, map.hash);
        // No other map file is written.
        // @lfy def/generation/main.lfy:record
        assert_eq!(
            fs::read_to_string(fixture.root.join("elfie-compile/maps/rust/b.json")).unwrap(),
            before
        );
        // The same maps always give the same bytes, and every object's keys are in
        // alphabetical order.
        // @lfy def/generation/main.lfy:record#record:record:0b3107676c8e2e0e6f742d7b4bf4f366f17a178f436600faae8ae601afbe6dbd
        let text = fs::read_to_string(&path).unwrap();
        assert!(record(workspace, unit, &[map.clone(), second]));
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|line| line.trim().strip_prefix('"'))
            .filter_map(|line| line.split_once('"').map(|(key, _)| key))
            .collect();
        assert_eq!(keys.len(), 18, "{text}");
        assert!(
            keys.chunks(9)
                .all(|object| object.windows(2).all(|pair| pair[0] < pair[1])),
            "{text}"
        );
    }

    /// Maps that differ from what the file holds only in when they were generated leave the
    /// file as it is, and no source map at all removes it.
    // @lfy def/generation/main.lfy:record#record:record:ad4005f338884cc64145762274daf706be00f7562ff83b81dc47ba0819ebd368
    #[test]
    fn a_later_generated_time_alone_writes_nothing_and_no_map_removes_the_file() {
        let (fixture, program, plan) = recorded();
        let workspace = &program.workspace;
        let (_, unit) = unit_of(workspace, &plan, "def/a.lfy");
        let path = fixture.root.join("elfie-compile/maps/rust/a.json");
        let before = fs::read_to_string(&path).unwrap();
        let mut map = map_for("crates/a/src/a.rs", "def/a.lfy");
        map.generated = "2030-01-01T00:00:00Z".to_string();
        // Maps that differ only in when they were generated leave the file as it is.
        // @lfy def/generation/main.lfy:record#record:record:63f4ea9ab8a20860ec82f31dda635344539ad9cf04296961aa5de457fbb7cd3f
        assert!(record(workspace, unit, std::slice::from_ref(&map)));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        // No source map at all removes the file.
        // @lfy def/generation/main.lfy:record#record:record:08ffdd0bd42a6d7ccf648a7ef393f291f365d4e80d00921f07556bfc58201ffc
        assert!(record(workspace, unit, &[]));
        assert!(!path.exists());
        // Removing what is already gone is recorded all the same.
        assert!(record(workspace, unit, &[]));
        assert!(source_maps_of(workspace, Some("rust")).len() == 1);
    }

    /// A project cloned without `elfie-compile` records its maps: every missing folder above
    /// the map file is created.
    // @lfy def/generation/main.lfy:record#record:record:2889ebf117abddc9af4fe7cbf26bd78f7da822afc5bfc70dd4ada149df2c4f17
    #[test]
    fn recording_creates_every_folder_above_the_map_file() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/cli/main.lfy", "d Cli { $x = string; }\n")
            .write("crates/elfie-cli/src/main.rs", "// main\n");
        let program = fixture.program();
        assert_bound(&program.workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
        assert!(!fixture.root.join("elfie-compile").exists());
        // Every missing folder above the map file is created.
        // @lfy def/generation/main.lfy:record#record:record:194ce1752f6c13ca28994e3eca650b6f7b123b12b3fb1a4987ff15e1d2d6f4a4
        assert!(record(
            &program.workspace,
            unit,
            &[map_for("crates/elfie-cli/src/main.rs", "def/cli/main.lfy")]
        ));
        let path = fixture
            .root
            .join("elfie-compile/maps/rust/cli/main.json");
        assert!(path.is_file());
        // The map reads back, so the unit is no longer fresh.
        // @lfy def/generation/main.lfy:sourceMapsOf
        assert_eq!(
            outputs_of(&source_maps_of(&program.workspace, None)),
            ["crates/elfie-cli/src/main.rs"]
        );
    }

    /// A map file that cannot be written gives `false`.
    // @lfy def/generation/main.lfy:record
    #[test]
    fn a_map_file_that_cannot_be_written_gives_false() {
        let (fixture, program, plan) = recorded();
        let workspace = &program.workspace;
        let (_, unit) = unit_of(workspace, &plan, "def/a.lfy");
        // A directory where the map file belongs cannot be written as a file.
        fs::remove_file(fixture.root.join("elfie-compile/maps/rust/a.json")).unwrap();
        fs::create_dir(fixture.root.join("elfie-compile/maps/rust/a.json")).unwrap();
        // @lfy def/generation/main.lfy:record#record:record:1f4671d2ae5027d4e5bcfaf56dc68c765e27e31a92fc783b7f64a2165f391dd4
        assert!(!record(
            workspace,
            unit,
            &[map_for("crates/a/src/a.rs", "def/a.lfy")]
        ));
    }

    // -----------------------------------------------------------------------------------
    // regionsOf
    // -----------------------------------------------------------------------------------

    /// `def/a.lfy` declaring a data `A` with a member `x` on lines 1 to 3, and a fn `f`
    /// from line 5 on.
    const A_WITH_A_MEMBER_AND_A_FN: &str = "d A: `An a` {\n  $x = string;\n}\n\nfn f(): `Does it` => string {\n  @acceptanceCriteria.add({ behavior = `it does` });\n}\n";

    fn a_with_a_member_and_a_fn() -> Fixture {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/a.lfy", A_WITH_A_MEMBER_AND_A_FN);
        fixture
    }

    /// One marker naming an entity.
    fn entity_marker(output_line: usize, entity: &str, line: usize, end: usize) -> Marker {
        Marker {
            output_line,
            file: "def/a.lfy".to_string(),
            entity: Some(entity.to_string()),
            line,
            column: None,
            requirement: None,
            end,
        }
    }

    /// The two source maps of the definition's tests: `src/a.rs` with `A`, `A.x`, and `f`,
    /// and `src/tests.rs` with `f` and `A`.
    fn two_source_maps() -> Vec<SourceMap> {
        let map = |output: &str, markers: Vec<Marker>| SourceMap {
            target: "rust".to_string(),
            output: output.to_string(),
            source: "def/a.lfy".to_string(),
            hash: source_hash("x"),
            requirements: String::new(),
            signature: source_hash("i"),
            dependencies: BTreeMap::new(),
            generated: "2026-09-18T10:00:00Z".to_string(),
            markers,
        };
        vec![
            map(
                "src/a.rs",
                vec![
                    entity_marker(3, "A", 1, 8),
                    entity_marker(9, "A.x", 2, 19),
                    entity_marker(20, "f", 5, 40),
                ],
            ),
            map(
                "src/tests.rs",
                vec![entity_marker(1, "f", 5, 11), entity_marker(12, "A", 1, 30)],
            ),
        ]
    }

    /// Every region of an owner, and of every member of it, in source map then output
    /// order.
    // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:ece2f51d1b6914f38fa307ff237a3db1a0f4f406773b246af49015c4d809a5ff
    #[test]
    fn the_regions_of_a_name_hold_its_own_and_those_of_its_members_in_output_order() {
        let fixture = a_with_a_member_and_a_fn();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let maps = two_source_maps();
        // Every marker whose entity is the name, source map by source map and within one in
        // output order.
        // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:38d2833b48e58c718adaf9951281dfe50568a87b1fd9fcf0cf493596b2406bae
        let regions = regions_of(&workspace, &maps, "A", None);
        // A member's region comes with its owner's, since it is part of its owner's code.
        // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:8915ec165a34269822d7cc1bf1492396f38dd0e75e18cbfcfccdf128f6532ef5
        assert_eq!(
            regions
                .iter()
                .map(|marker| (marker.entity.clone(), marker.output_line, marker.end))
                .collect::<Vec<_>>(),
            [
                (Some("A".to_string()), 3, 8),
                (Some("A.x".to_string()), 9, 19),
                (Some("A".to_string()), 12, 30),
            ]
        );
    }

    /// A member's own name gives its region alone, and a name nothing was generated for
    /// gives none.
    // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:b44c74236084b5fe9fc1cd6d551a512812bee292cfaa155dcc6a5eac64764b5c
    #[test]
    fn a_members_name_gives_its_region_alone_and_an_unknown_name_gives_none() {
        let fixture = a_with_a_member_and_a_fn();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        let maps = two_source_maps();
        // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:fb7b6213018e0ac14281583c01918af5136734d69bfbba09e66bdb537c59125f
        let regions = regions_of(&workspace, &maps, "A.x", None);
        assert_eq!(regions.len(), 1, "{regions:?}");
        assert_eq!(regions[0].output_line, 9);
        assert_eq!(regions[0].entity.as_deref(), Some("A.x"));
        // A name no marker names, of itself, of a member of it, or of a line inside its
        // declaration, gives an empty list.
        // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:f7ad766d38aaa1f3fe185238e7663b08246cbc5f1782291b0370e57f8b6c3334
        assert!(regions_of(&workspace, &maps, "B", None).is_empty());
    }

    /// A marker that names a line belongs to the entity whose declaration covers it, and
    /// a target keeps only the source maps of that target.
    // @lfy def/generation/main.lfy:regionsOf
    #[test]
    fn a_line_marker_inside_a_declaration_and_a_target_narrow_the_regions() {
        let fixture = a_with_a_member_and_a_fn();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let line_marker = |output_line: usize, line: usize| Marker {
            output_line,
            file: "def/a.lfy".to_string(),
            entity: None,
            line,
            column: None,
            requirement: None,
            end: output_line,
        };
        let mut maps = two_source_maps();
        maps[0].markers = vec![line_marker(1, 2), line_marker(2, 5), line_marker(3, 99)];
        maps[1].target = "other".to_string();
        // Line 2 falls inside A's declaration; line 5 inside f's; line 99 in nothing.
        // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:66a35d2038e53526c71eb7ac77c60c00ba7678d9ffc14769336d39c7f3aeadb6
        let regions = regions_of(&workspace, &maps, "A", Some("rust"));
        assert_eq!(regions.len(), 1, "{regions:?}");
        assert_eq!(regions[0].output_line, 1);
        assert_eq!(regions_of(&workspace, &maps, "f", Some("rust")).len(), 1);
        // The source map of another target is left out when a target is given.
        // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:f9024ae8f628b3e934c6b4da514ca9c70f9004cfd2a85ae9d17fb070d9cbc8e1
        let mut maps = two_source_maps();
        maps[1].target = "other".to_string();
        assert_eq!(regions_of(&workspace, &maps, "A", None).len(), 3);
        assert_eq!(regions_of(&workspace, &maps, "A", Some("rust")).len(), 2);
        assert_eq!(regions_of(&workspace, &maps, "A", Some("other")).len(), 1);
    }

    // -----------------------------------------------------------------------------------
    // changes
    // -----------------------------------------------------------------------------------

    fn change_kinds(changes: &[Change]) -> Vec<(&str, ChangeKind)> {
        changes
            .iter()
            .map(|change| (change.entity.as_str(), change.kind))
            .collect()
    }

    // @lfy def/generation/main.lfy:changes#changes:changes:3e433d7bcae5a307246d5ee75e073f353540363de5806a450996d56c3ae4527a
    #[test]
    fn without_a_previous_file_every_entity_of_the_unit_is_an_addition() {
        let fixture = a_with_a_member_and_a_fn();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
        // No previous text: one addition per entity of the unit, in file order, and nothing
        // else.
        // @lfy def/generation/main.lfy:changes#changes:changes:1306019827dfec88eae5b6bd940cef6daccef5d2c1398607687088c561f1f181
        let changes = changes(&program, unit,None);
        // A name declared now and not before is an addition.
        // @lfy def/generation/main.lfy:changes#changes:changes:9fa34ffb6593e18420b047e381120aa4b10d245c1b1cc825bd68c5b21928f000
        assert_eq!(
            change_kinds(&changes),
            [("A", ChangeKind::Added), ("f", ChangeKind::Added)]
        );
    }

    // @lfy def/generation/main.lfy:changes#changes:changes:2d464e60a8872ef956aa7879f05ba6a1cf0508a0690ad19ff5d73f1c8840fd51
    #[test]
    fn a_new_definition_and_a_new_parameter_are_a_definition_and_a_signature() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d A: `a thing` {\n  $x = string;\n}\n\nfn f(x: string, y: number): `Does it` => string {\n  @acceptanceCriteria.add({ behavior = `it does` });\n}\n",
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
        let previous = "d A: `a value` {\n  $x = string;\n}\n\nfn f(x: string): `Does it` => string {\n  @acceptanceCriteria.add({ behavior = `it does` });\n}\n";
        // The file as it was is bound through the loader, never diffed line by line.
        // @lfy def/generation/main.lfy:changes
        let changes = changes(&program, unit,Some(previous));
        // A differing definition is one change, a differing parameter list another.
        // @lfy def/generation/main.lfy:changes#changes:changes:31cbf0fb17f9e9a9897caeed410e96022754fa49a032c9c654cd1f04ea0915be
        assert_eq!(
            change_kinds(&changes),
            [("A", ChangeKind::Definition), ("f", ChangeKind::Signature)],
            "{changes:?}"
        );
        // Each is under 80 characters, so the detail quotes the old and the new.
        // @lfy def/generation/main.lfy:changes#changes:changes:f3cde677f1a0cd7fd96797089181f950effc833d36a19ceb41f612a135d9b7a8
        assert!(changes[0].detail.contains("a value"), "{:?}", changes[0]);
        assert!(changes[0].detail.contains("a thing"), "{:?}", changes[0]);
        // A signature is named rather than quoted: the parameter that differs.
        // @lfy def/generation/main.lfy:changes#changes:changes:5209dc5c684e270166372c45b82d43b1ec0be20a8e45931a8401d93a06035108
        assert_eq!(changes[1].detail, "the parameter y was added");
    }

    /// A removed entity comes after the last entity that preceded it before and is still
    /// declared, a member is compared as an entity of its own under its owner, and the
    /// detail of a change of criteria names the count before and the count now.
    // @lfy def/generation/main.lfy:changes#changes:changes:d1db47c85dc702c7532d80c945cafa1e21614801d0f6395c0aac85bdd144e0ee
    #[test]
    fn a_removed_entity_a_removed_member_and_a_new_criterion_are_read_in_file_order() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two` })\n    .add({ behavior = `three` });\n}\n\nd B {\n  $x = string;\n}\n",
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
        let previous = "d A;\n\nfn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two` });\n}\n\nd B {\n  $x = string;\n  $old = number;\n}\n";
        // A name declared before and not now is a removal.
        // @lfy def/generation/main.lfy:changes#changes:changes:577fb23d0eeb277ec3536077bd65e3bae93b79dd0d4651bf34d0baf4721efb72
        let changes = changes(&program, unit,Some(previous));
        // The changes of entities declared now follow the order of the unit's entities; `A`
        // was first before and nothing that preceded it is still declared, so its removal is
        // first, and `B.old`, whose predecessor `f` is still declared, comes after `f`.
        // @lfy def/generation/main.lfy:changes#changes:changes:4fe92e633a3e97cb3af4567ad639361ba2ae14254bd2af13633ec6c79ec86d0d
        assert_eq!(
            change_kinds(&changes),
            [
                ("A", ChangeKind::Removed),
                ("f", ChangeKind::Criteria),
                ("B.old", ChangeKind::Removed),
            ],
            "{changes:?}"
        );
        // The detail names the count before and now, then the id added; an id is longer
        // than a detail quotes, so it is named rather than quoted.
        // @lfy def/generation/main.lfy:changes#changes:changes:0621aa70543db6d32719c3dfed5e9e2bcfd9c529e5a2fdb66e7be05532c656fd
        assert!(
            changes[1]
                .detail
                .starts_with("2 criteria before and 3 now; the criterion "),
            "{:?}",
            changes[1]
        );
        assert!(changes[1].detail.ends_with(" was added"), "{:?}", changes[1]);
    }

    /// A criterion whose text changed keeps its place and takes a new id, so the detail of
    /// the change calls the pair reworded; a test's is read the same way.
    // @lfy def/generation/main.lfy:changes
    #[test]
    fn a_criterion_reworded_in_place_is_called_reworded() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two, said better` });\n  @test({ input = [], expect = `b` });\n}\n",
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
        let previous = "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two` });\n  @test({ input = [], expect = `a` });\n}\n";
        // The ids of the local tests differ, so there is a change of tests too, after the
        // change of criteria, in the order ChangeKind declares them.
        // @lfy def/generation/main.lfy:changes#changes:changes:b93a6681e63ab930b2883e00f1b5abaf044b41b09124728fe0ca10c6e6b45916
        let changes = changes(&program, unit, Some(previous));
        assert_eq!(
            change_kinds(&changes),
            [("f", ChangeKind::Criteria), ("f", ChangeKind::Tests)],
            "{changes:?}"
        );
        // A removed and an added criterion id at the same position are called reworded.
        // @lfy def/generation/main.lfy:changes#changes:changes:3f4523cbbf2804e4e98595903c9316c9f6f0440cb5e3b253a956481e2599fcd8
        assert!(
            changes[0]
                .detail
                .contains("; the criterion at 2 was reworded, from "),
            "{:?}",
            changes[0]
        );
        // A test's is read the same way.
        // @lfy def/generation/main.lfy:changes#changes:changes:a1ed86de222e6b6c85bcfe97b505f96a6306b459b18710f1f74cf664d7ce804b
        assert!(
            changes[1]
                .detail
                .contains("; the test at 1 was reworded, from "),
            "{:?}",
            changes[1]
        );
    }

    /// A file whose entities read the same after a comment, a reordering of nothing, and
    /// whitespace gives no change at all; a statement of a written body gives one.
    // @lfy def/generation/main.lfy:changes
    #[test]
    fn whitespace_and_comments_give_no_change_and_a_new_statement_is_a_body() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "// a comment\nd A {\n  $x = string;\n}\n\nfunction g(x: number): `Doubles` -> number {\n  return x * 2;\n}\n",
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
        // Entities are compared, never lines, so whitespace and comments give nothing.
        // @lfy def/generation/main.lfy:changes
        let same = "d A {\n\n  $x   = string;\n}\n\nfunction g(x: number): `Doubles` -> number {\n  return x * 2;\n}\n";
        assert!(changes(&program, unit,Some(same)).is_empty());
        // The tokens of the declaring node differ once everything read apart from the body is
        // left out, so there is one change of body.
        // @lfy def/generation/main.lfy:changes#changes:changes:d5f5fa3f7e3377735cb0315378b2cf53df8b6751fbf3bbc207d6a162992f2b14
        let other = "d A {\n  $x = string;\n}\n\nfunction g(x: number): `Doubles` -> number {\n  return x * 3;\n}\n";
        let changes = changes(&program, unit,Some(other));
        assert_eq!(change_kinds(&changes), [("g", ChangeKind::Body)], "{changes:?}");
    }

    /// A member whose definition and whose declared type both differ gives one change per
    /// kind that differs, in the order `ChangeKind` declares them.
    // @lfy def/generation/main.lfy:changes#changes:changes:1a1a926241f820a2302b13b9f0e6d3a5150b545da1f4a24ce81c257de353855e
    #[test]
    fn a_name_on_both_sides_gives_one_change_per_kind_that_differs() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/a.lfy", "d A {\n  $x: `one` = string;\n}\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
        let previous = "d A {\n  $x: `two` = number;\n}\n";
        let changes = changes(&program, unit, Some(previous));
        // The definition comes before the declared type, as `ChangeKind` declares them.
        // @lfy def/generation/main.lfy:changes#changes:changes:3a27812b418c50e70dd8f17ab011cf87210b1245f5064b7fcb22af71ebd740ba
        assert_eq!(
            change_kinds(&changes),
            [
                ("A.x", ChangeKind::Definition),
                ("A.x", ChangeKind::DeclaredType),
            ],
            "{changes:?}"
        );
        assert!(changes[1].detail.contains("number"), "{:?}", changes[1]);
        assert!(changes[1].detail.contains("string"), "{:?}", changes[1]);
    }

    // -----------------------------------------------------------------------------------
    // review
    // -----------------------------------------------------------------------------------

    // @lfy def/generation/main.lfy:review#review:review:90e9bf36c1adde74df80ded58ee9c0c22589f0ebc430d828fe6b1e98abf5f2de
    #[test]
    fn a_review_request_quotes_every_criterion_with_its_place_and_excerpts_every_region() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d A: `An a` {\n  @acceptanceCriteria\n    .add({ behavior = `the first` })\n    .add({ behavior = `the second` });\n  @test({ input = 1, expect = 2 });\n}\n",
        );
        fixture.write("src/a.rs", "one\ntwo\nthree\nfour\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![index],
            identifier: "a".to_string(),
        };
        let maps = vec![SourceMap {
            target: "rust".to_string(),
            output: "src/a.rs".to_string(),
            source: "def/a.lfy".to_string(),
            hash: source_hash("x"),
            requirements: String::new(),
            signature: source_hash("i"),
            dependencies: BTreeMap::new(),
            generated: "2026-09-18T10:00:00Z".to_string(),
            markers: vec![entity_marker(1, "A", 1, 4)],
        }];
        let request = review(&program, &plan, &batch,&maps);
        // @lfy def/generation/main.lfy:review#review:review:197b2d0e6f7458d538f3cc8d8568be98a60636ca07026cd2eeb1d5933910f156
        assert_eq!(request.batch, Some(batch));
        let text = &request.instructions;
        // The heading names the batch and the target.
        // @lfy def/generation/main.lfy:review
        assert!(
            text.contains("# Reviewing the batch `a` for the target `rust`"),
            "{text}"
        );
        assert!(text.contains("#### `A` (data: DataDeclaration)"), "{text}");
        assert!(text.contains("Definition: An a"), "{text}");
        // Each criterion of a chain has a place of its own, on the line its `add` begins
        // on, and is prefixed by its id.
        // @lfy def/generation/main.lfy:review#review:review:775d8a8dfc4f9a8764fa84a8d56a00ff51d1817460ffacf779ace250865ffcaa
        let (criteria, tests) = {
            let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
            requirement_ids(&program.files[unit.lowered], unit.entities[0])
        };
        assert!(
            text.contains(&format!("`{}` def/a.lfy:3 — the first", criteria[0])),
            "{text}"
        );
        assert!(
            text.contains(&format!("`{}` def/a.lfy:4 — the second", criteria[1])),
            "{text}"
        );
        // A test's place is the line its origin begins on.
        // @lfy def/generation/main.lfy:review
        assert!(
            text.contains(&format!(
                "`{}` def/a.lfy:5 — Input `1` gives `2`",
                tests[0]
            )),
            "{text}"
        );
        // Only local criteria and tests are reviewed here.
        // @lfy def/generation/main.lfy:review
        assert!(text.contains("A global one is reviewed once"), "{text}");
        // The region, with the lines of the output in a fenced block.
        // @lfy def/generation/main.lfy:review#review:review:ab46be2107934463218771ce935af8b1af8c59e0f5656d26a5d6dd846df1087d
        assert!(text.contains("`src/a.rs:1-4`"), "{text}");
        assert!(text.contains("```\none\ntwo\nthree\nfour\n```"), "{text}");
        // The protocol: the six keys, the three statuses, and the end line.
        // @lfy def/generation/main.lfy:review
        for part in [
            "examine and never edit",
            "`id`, `status`, `evidence`, and `note`",
            "`satisfied`",
            "`violated`",
            "`unverifiable`",
            "unverifiable, never violated",
            "agent server",
            "`ELFIE: REVIEWED`",
        ] {
            assert!(text.contains(part), "{part} is missing:\n{text}");
        }
    }

    /// With no source map for the batch the verifier is given the same instructions with
    /// no region, so it can only find the criteria unverifiable.
    // @lfy def/generation/main.lfy:review#review:review:21df0727a9c7ecebe08a29a6910e791aa77230e050e97b71b77b38380575e408
    #[test]
    fn a_review_request_without_a_source_map_holds_no_region() {
        let fixture = a_with_a_member_and_a_fn();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![index],
            identifier: "a".to_string(),
        };
        let request = review(&program, &plan, &batch,&[]);
        let text = &request.instructions;
        assert!(text.contains("def/a.lfy:6 — it does"), "{text}");
        // @lfy def/generation/main.lfy:review
        assert!(
            text.contains("No region of any output was generated for it."),
            "{text}"
        );
        assert!(text.contains("`ELFIE: REVIEWED`"), "{text}");
    }

    /// An output a source map names but that cannot be read gives a line saying so
    /// instead of a fenced block.
    // @lfy def/generation/main.lfy:review#review:review:2f84308152b64a4bdf82224c54c0caf9e0a72ebcf65d5072ef5d52cc1b92cec5
    #[test]
    fn a_region_whose_output_cannot_be_read_says_so() {
        let fixture = a_with_a_member_and_a_fn();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![index],
            identifier: "a".to_string(),
        };
        let maps = two_source_maps();
        let request = review(&program, &plan, &batch,&maps);
        assert!(
            request
                .instructions
                .contains("The output could not be read:"),
            "{}",
            request.instructions
        );
    }

    // -----------------------------------------------------------------------------------
    // globalReview
    // -----------------------------------------------------------------------------------

    /// One global criterion, whose id two outputs name in a marker: it is given once with
    /// its id and its place, then one region per output, then the protocol.
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:07d990c406ebc44cf6bb563fad2246e918bd3c85fa0ba0bc4228d58bdc0673a3
    #[test]
    fn the_global_review_gives_each_global_criterion_once_with_every_region() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write(
                "def/a.lfy",
                "d A { $x = string; }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written twice` });\n",
            )
            .write("src/a.rs", "one\ntwo\n")
            .write("src/b.rs", "three\nfour\n");
        let program = fixture.program();
        assert_bound(&program.workspace);
        assert_eq!(program.criteria.len(), 1, "{:?}", program.criteria);
        let id = program.criteria[0].id.clone();
        let answering = |output: &str| SourceMap {
            target: "rust".to_string(),
            output: output.to_string(),
            source: "def/a.lfy".to_string(),
            hash: source_hash("x"),
            requirements: source_hash("r"),
            signature: source_hash("i"),
            dependencies: BTreeMap::new(),
            generated: "2026-09-18T10:00:00Z".to_string(),
            markers: vec![Marker {
                output_line: 1,
                file: "def/a.lfy".to_string(),
                entity: Some("A".to_string()),
                line: 1,
                column: None,
                requirement: Some(id.clone()),
                end: 2,
            }],
        };
        let maps = vec![answering("src/a.rs"), answering("src/b.rs")];
        let request = global_review(&program, &maps);
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:de84b01e368319ebde8eb693c9242574f7b87c6cdedc5229f53fbdf2a3cdd11d
        assert_eq!(request.batch, None);
        let text = &request.instructions;
        // @lfy def/generation/main.lfy:globalReview
        assert!(text.contains("# The global review"), "{text}");
        assert!(text.contains("The targets are `rust`."), "{text}");
        // @lfy def/generation/main.lfy:globalReview
        assert!(
            text.contains(&format!("## `{id}` def/a.lfy:2")),
            "{text}"
        );
        assert_eq!(text.matches(id.as_str()).count(), 1, "{text}");
        assert!(text.contains("Nothing is written twice"), "{text}");
        // One region per output, each with the lines of the output in a fenced block.
        // @lfy def/generation/main.lfy:globalReview
        assert!(text.contains("`src/a.rs:1-2`"), "{text}");
        assert!(text.contains("```\none\ntwo\n```"), "{text}");
        assert!(text.contains("`src/b.rs:1-2`"), "{text}");
        assert!(text.contains("```\nthree\nfour\n```"), "{text}");
        // Then the protocol.
        // @lfy def/generation/main.lfy:globalReview
        assert!(text.contains("`ELFIE: REVIEWED`"), "{text}");
        assert!(text.contains("unverifiable, never violated"), "{text}");
    }

    /// A global criterion no marker of any output names is listed with a line saying no
    /// unit answered for it, so the verifier finds it unverifiable, never violated.
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:cf70900c69dc27826a7088df39eabf087281927bfee67200cb82f105ccf30db3
    #[test]
    fn a_global_criterion_no_marker_names_says_no_unit_answered_for_it() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d A { $x = string; }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written twice` });\n",
        );
        let program = fixture.program();
        assert_bound(&program.workspace);
        let request = global_review(&program, &[]);
        let text = &request.instructions;
        assert_eq!(request.batch, None);
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:964ff9ee37432a35d1dc0b5b42ac738488193128815e15d568449402840d5340
        assert!(
            text.contains("No unit answered for it, so it cannot be checked against any region."),
            "{text}"
        );
        assert!(text.contains(&program.criteria[0].id), "{text}");
        assert!(text.contains("`ELFIE: REVIEWED`"), "{text}");
    }

    // -----------------------------------------------------------------------------------
    // reviewOf
    // -----------------------------------------------------------------------------------

    /// One line of a verifier's report: the id of a criterion or test, and what was found
    /// for it.
    fn review_line(id: &str, status: &str, evidence: &str, note: &str) -> String {
        format!(
            "{{\"id\":\"{id}\",\"status\":\"{status}\",\
             \"evidence\":\"{evidence}\",\"note\":\"{note}\"}}"
        )
    }

    /// `def/a.lfy` declaring `A` with two local criteria, on the lines its two `add` calls
    /// begin on, and one local test; the ids of the two criteria come with it.
    fn a_with_two_criteria() -> (Fixture, Program, Vec<String>) {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d A: `An a` {\n  @acceptanceCriteria\n    .add({ behavior = `the first` })\n    .add({ behavior = `the second` });\n  @test({ input = 1, expect = 2 });\n}\n",
        );
        let program = fixture.program();
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
        let _ = index;
        let entity = unit.entities[0];
        let (ids, _) = requirement_ids(&program.files[unit.lowered], entity);
        assert_eq!(ids.len(), 2, "{ids:?}");
        (fixture, program, ids)
    }

    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:6ad3006e396fe8b3c5ea739e4852be683f1bba8ddaa4b1b13dd25123422037c6
    #[test]
    fn a_report_of_two_reviews_and_a_comment_gives_two_reviews_and_no_problems() {
        let (_fixture, program, ids) = a_with_two_criteria();
        let report = format!(
            "# reading src/a.rs\n{}\n{}\n{REVIEWED}",
            review_line(&ids[0], "satisfied", "src/a.rs:1-30", "the field is there"),
            review_line(&ids[1], "violated", "src/a.rs:12-18", "it returns undefined"),
        );
        let found = review_of(&report, &program);
        // Each line before the end line that reads as an object with exactly those four keys,
        // whose id names a criterion or test of the program, is one review, in report order.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:7edfaef26ee15846bc27041e6b1ea1693c38a86f769b13dc68421266c0d05664
        assert_eq!(found.reviews.len(), 2, "{found:?}");
        assert_eq!(found.reviews[0].status, ReviewStatus::Satisfied);
        assert_eq!(found.reviews[0].id, ids[0]);
        assert_eq!(found.reviews[0].evidence, "src/a.rs:1-30");
        assert_eq!(found.reviews[1].status, ReviewStatus::Violated);
        // The place of each is derived from the origin of the criterion its id names: the
        // file it is written in, the line its `add` begins on, and the entity it is for.
        // @lfy def/generation/main.lfy:reviewOf
        assert_eq!(found.reviews[0].file, "def/a.lfy");
        assert_eq!(found.reviews[0].entity, "A");
        assert_eq!(found.reviews[0].line, 3);
        assert_eq!(found.reviews[1].line, 4);
        // A line beginning with `#` is no problem, and neither is a blank one.
        // @lfy def/generation/main.lfy:reviewOf
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        // Lines after the end line are ignored.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:f33d7c1d482ac83b637de975ac61fc8ad9a4e4464ac8f11554da868087905870
        let found = review_of(&format!("{report}\nand that is all"), &program);
        assert_eq!(found.reviews.len(), 2);
        assert!(found.problems.is_empty(), "{:?}", found.problems);
    }

    /// A line whose status names no member of `ReviewStatus` is no review, and neither is
    /// prose or a line whose id names no criterion and no test of the program; each is a
    /// problem, as written.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:5fa253af126379872ed4480c8d44bff938860829f3de98628cc2f657119fd906
    #[test]
    fn a_line_that_is_no_review_is_a_problem_as_written() {
        let (_fixture, program, ids) = a_with_two_criteria();
        let unknown = review_line("A:A:nowhere", "satisfied", "", "made up");
        let report = format!(
            "{}\nI think it is fine.\n{unknown}\n{}\n{REVIEWED}",
            review_line(&ids[0], "maybe", "", "unsure"),
            review_line(&ids[1], "unverifiable", "", "no region"),
        );
        let found = review_of(&report, &program);
        // A status that names no member of ReviewStatus is no review.
        // @lfy def/generation/main.lfy:reviewOf
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.reviews[0].id, ids[1]);
        assert_eq!(found.reviews[0].status, ReviewStatus::Unverifiable);
        // Every line that is no review, is not blank, and does not begin with `#` is a
        // problem, as written, in report order.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9782bbe9f8f67ff19baae92527a7e25d2c8c79db79b8d58afada1955ca127986
        assert_eq!(
            found.problems,
            [
                review_line(&ids[0], "maybe", "", "unsure"),
                "I think it is fine.".to_string(),
                unknown,
            ]
        );
    }

    /// Of two reviews of the same id only the last is kept, at its own place, and a report
    /// that never ends says so.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:389bbf9e57884ff5e66e1b8be297335a6fb9e9a36faae98a32402328be553325
    #[test]
    fn the_last_review_of_a_place_is_kept_and_an_unended_report_says_so() {
        let (_fixture, program, ids) = a_with_two_criteria();
        let report = format!(
            "{}\n{}",
            review_line(&ids[0], "satisfied", "", "first"),
            review_line(&ids[0], "violated", "src/a.rs:3-3", "second"),
        );
        // With no end line, reviews are read from every line of the report.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:7d8dbed5b1381d618440d882e2814afe9d880f924e90eba8eefa0a51cb559d81
        let found = review_of(&report, &program);
        // Of two reviews of the same id only the last is kept, at its own place.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:530bd81507c25a152f040b7f923efcb22c290d283c3c5522b2a007ba119495a8
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.reviews[0].status, ReviewStatus::Violated);
        assert_eq!(found.reviews[0].note, "second");
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:8af09639c386a7e67c0a197f2a2facd2b7b9caabeeb08be071ae2643944463c1
        assert_eq!(found.problems, ["the report did not end"]);
        // An empty report ends with nothing either.
        assert_eq!(
            review_of("", &program),
            ReviewReport {
                reviews: Vec::new(),
                problems: vec!["the report did not end".to_string()],
            }
        );
        // The end line stands with whitespace around it.
        // @lfy def/generation/main.lfy:reviewOf
        let found = review_of(&format!("  {REVIEWED}  "), &program);
        assert!(found.reviews.is_empty());
        assert!(found.problems.is_empty(), "{:?}", found.problems);

        // With no end line, problems are read from every line of the report too.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:4182a74673c9410aae2d6a75652f69b786e21eb5a84e4027d81a8834d12d5e23
        let found = review_of(
            &format!(
                "{}\nI had a look.",
                review_line(&ids[0], "satisfied", "", "ok")
            ),
            &program,
        );
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(
            found.problems,
            ["I had a look.", "the report did not end"]
        );
    }

    /// A review of a global criterion is placed at that criterion's origin and belongs to
    /// `global`, whatever entity the verifier saw it under.
    // @lfy def/generation/main.lfy:reviewOf
    #[test]
    fn a_review_of_a_global_criterion_belongs_to_global() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d A { $x = string; }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written outside the output directory` });\n",
        );
        let program = fixture.program();
        assert_eq!(program.criteria.len(), 1, "{:?}", program.criteria);
        let id = program.criteria[0].id.clone();
        assert!(Review::is_global(&id), "{id}");
        let found = review_of(
            &format!(
                "{}\n{REVIEWED}",
                review_line(&id, "satisfied", "src/a.rs:1-3", "it is under src")
            ),
            &program,
        );
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.reviews[0].entity, GLOBAL);
        assert_eq!(found.reviews[0].file, "def/a.lfy");
        assert_eq!(found.reviews[0].line, 2);
        assert!(found.problems.is_empty(), "{:?}", found.problems);
    }

    // -----------------------------------------------------------------------------------
    // markers, hashes, source maps, timestamps
    // -----------------------------------------------------------------------------------

    // @lfy def/generation/data.lfy:Marker.line
    #[test]
    fn markers_are_parsed_after_any_line_comment_opener() {
        let text = "// @LFY def/a.lfy:4\nfn x() {}\n# @LFY def/a.lfy:5:2\n-- @LFY def/a.lfy:6 -- more\n; @LFY def/a.lfy:7.\nnothing here\n/* @LFY def/a.lfy:8 */\n// @LFY def/a.lfy:Marker.file\n// @LFY nope\n// @LFY :3\n".replace("@LFY", "@lfy");
        let markers = parse_markers(&text);
        let marker = |output_line, line, column, end| Marker {
            output_line,
            file: "def/a.lfy".to_string(),
            entity: None,
            line,
            column,
            requirement: None,
            end,
        };
        assert_eq!(
            markers,
            [
                marker(1, 4, None, 2),
                marker(3, 5, Some(2), 3),
                marker(4, 6, None, 4),
                marker(5, 7, None, 6),
                marker(7, 8, None, 7),
                // @lfy def/generation/data.lfy:Marker.entity
                Marker {
                    output_line: 8,
                    file: "def/a.lfy".to_string(),
                    entity: Some("Marker.file".to_string()),
                    line: 0,
                    column: None,
                    requirement: None,
                    end: 10,
                },
            ]
        );
        assert_eq!(markers[1].spelling(), "@lfy def/a.lfy:5:2");
        assert_eq!(markers[0].spelling(), "@lfy def/a.lfy:4");
        assert_eq!(markers[5].spelling(), "@lfy def/a.lfy:Marker.file");
        assert!(parse_markers("").is_empty());
    }

    // @lfy def/generation/data.lfy:Marker
    #[test]
    fn marker_regions_partition_an_output_from_its_first_marker_on() {
        let text =
            "one\n// @LFY def/a.lfy:A\ntwo\nthree\n// @LFY def/a.lfy:B // @LFY def/a.lfy:C\nfour\n"
                .replace("@LFY", "@lfy");
        let markers = parse_markers(&text);
        let regions: Vec<(usize, usize)> = markers
            .iter()
            .map(|marker| (marker.output_line, marker.end))
            .collect();
        // The two markers of line 5 share it: the first ends before it and covers nothing.
        assert_eq!(regions, [(2, 4), (5, 4), (5, 6)]);
        for line in 1..=text.lines().count() {
            let owners = markers.iter().filter(|marker| marker.covers(line)).count();
            assert_eq!(
                owners,
                usize::from(line >= markers[0].output_line),
                "line {line}"
            );
        }
    }

    // @lfy def/generation/data.lfy:Marker
    #[test]
    fn only_a_lfy_path_and_a_line_or_an_identifier_path_is_read_as_a_marker() {
        let prose = [
            "@LFY see the notes on markers",
            "@LFY notes.md:4",
            "@LFY README:4",
            "@LFY def/a.rs:4",
            "@LFY .lfy:4",
            "@LFY :4",
            "@LFY def/a.lfy:",
            "@LFY def/a.lfy:4-5",
            "@LFY def/a.lfy:1Marker",
            "@LFY def/a.lfy:Marker..file",
            // A line, with or without a column, spells no requirement, and an empty id is
            // none, so each of these is prose.
            "@LFY def/a.lfy:4#A:A:abc",
            "@LFY def/a.lfy:4:2#A:A:abc",
            "@LFY def/a.lfy:Marker#",
        ];
        for line in prose {
            let text = line.replace("@LFY", "@lfy");
            assert!(parse_markers(&text).is_empty(), "{text}");
        }
        // @lfy def/generation/data.lfy:Marker
        let text = "// @LFY def/a.lfy:4\n// @LFY def/deep/a.lfy:Marker\n// @LFY def/a.lfy:_x.y2\n// @LFY def/a.lfy:Marker.file#Marker:Marker:abc\n"
            .replace("@LFY", "@lfy");
        let markers = parse_markers(&text);
        assert_eq!(markers.len(), 4, "{markers:?}");
        assert_eq!(markers[0].line, 4);
        assert_eq!(markers[1].file, "def/deep/a.lfy");
        assert_eq!(markers[1].entity.as_deref(), Some("Marker"));
        assert_eq!(markers[2].entity.as_deref(), Some("_x.y2"));
        assert_eq!(markers[0].requirement, None);
        // A marker read back spells itself as it was written, requirement and all.
        assert_eq!(markers[3].entity.as_deref(), Some("Marker.file"));
        assert_eq!(markers[3].requirement.as_deref(), Some("Marker:Marker:abc"));
        assert_eq!(
            markers[3].spelling(),
            "@lfy def/a.lfy:Marker.file#Marker:Marker:abc"
        );
    }

    // @lfy def/generation/data.lfy:SourceMap.hash
    #[test]
    fn the_source_hash_is_lowercase_hex_sha256() {
        assert_eq!(
            source_hash(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            source_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // @lfy def/generation/data.lfy:SourceMap
    #[test]
    fn source_maps_round_trip_through_json_and_a_missing_file_gives_none() {
        let fixture = Fixture::new();
        let path = fixture.root.join("src").join("source-map.json");
        assert!(read_source_maps(&path).is_empty());
        let maps = vec![
            SourceMap {
                target: "rust".to_string(),
                output: "src/a.rs".to_string(),
                source: "def/a.lfy".to_string(),
                hash: source_hash("x"),
                requirements: source_hash("the requirements of a"),
                signature: source_hash("interface of a"),
                dependencies: BTreeMap::from([(
                    "def/b.lfy".to_string(),
                    source_hash("interface of b"),
                )]),
                generated: "2026-09-18T10:00:00Z".to_string(),
                markers: vec![
                    Marker {
                        output_line: 1,
                        file: "def/a.lfy".to_string(),
                        entity: Some("A".to_string()),
                        line: 4,
                        column: None,
                        requirement: Some("A:A:abc".to_string()),
                        end: 8,
                    },
                    Marker {
                        output_line: 9,
                        file: "def/a.lfy".to_string(),
                        entity: None,
                        line: 5,
                        column: Some(2),
                        requirement: None,
                        end: 12,
                    },
                ],
            },
            SourceMap {
                target: "rust".to_string(),
                output: "src/b.rs".to_string(),
                source: "def/b.lfy".to_string(),
                hash: source_hash("y"),
                requirements: String::new(),
                signature: source_hash("interface of b"),
                dependencies: BTreeMap::new(),
                generated: "2026-09-18T10:00:01Z".to_string(),
                markers: Vec::new(),
            },
        ];
        write_source_maps(&path, &maps).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("[\n"), "{text}");
        assert!(text.contains("\"outputLine\": 9"), "{text}");
        assert_eq!(read_source_maps(&path), maps);
        fixture.write("src/source-map.json", "not json");
        assert!(read_source_maps(&path).is_empty());
        fixture.write("src/source-map.json", "[{\"target\": 1}]");
        assert!(read_source_maps(&path).is_empty());
    }

    // @lfy def/generation/data.lfy:SourceMap.generated
    #[test]
    fn timestamps_are_rfc3339_in_utc_to_the_second() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_789_689_600), "2026-09-18T00:00:00Z");
        assert_eq!(parse_rfc3339("2026-09-18T00:00:00Z"), Some(1_789_689_600));
        assert_eq!(
            parse_rfc3339("2026-09-18T02:00:00.5+02:00"),
            Some(1_789_689_600)
        );
        assert_eq!(
            parse_rfc3339("2026-09-17T22:30:00-01:30"),
            Some(1_789_689_600)
        );
        assert_eq!(parse_rfc3339("yesterday"), None);
        assert_eq!(parse_rfc3339("2026-13-01T00:00:00Z"), None);
        let now = now_rfc3339();
        assert_eq!(now.len(), 20, "{now}");
        assert!(now.ends_with('Z'));
        let seconds = parse_rfc3339(&now).unwrap();
        assert_eq!(format_rfc3339(seconds), now);
        assert!(seconds > 1_789_689_600);
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:1fc2e33009d6c55647083813e41979cdcd5f51fe2a856a9f6815fb4aa2b68bd3
    #[test]
    fn stems_and_paths() {
        assert_eq!(stem_of("def/a.lfy", "def"), "a");
        assert_eq!(stem_of("def/deep/inner.lfy", "def/"), "deep/inner");
        assert_eq!(stem_of("elsewhere/x.lfy", "def"), "elsewhere/x");
        assert_eq!(stem_of("a.lfy", "."), "a");
        assert!(under("src/a.rs", "src"));
        assert!(under("src/a.rs", ""));
        assert!(!under("srcx/a.rs", "src"));
        assert!(!under("src", "src"));
        assert_eq!(
            order_after_dependencies(&[vec![1], vec![2], vec![]]),
            [2, 1, 0]
        );
        assert_eq!(order_after_dependencies(&[vec![1], vec![0]]), [1, 0]);
        // @lfy def/generation/main.lfy:plan
        assert_eq!(first_segment("a"), "");
        assert_eq!(first_segment("deep/inner"), "deep");
        assert_eq!(first_segment("deep/deeper/inner"), "deep");
    }

}
