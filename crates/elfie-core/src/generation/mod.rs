//! Compiled from `def/generation/main.lfy`: what a dependent may rely on, planning what
//! to generate, asking for it, reading how a run ended, and accepting what comes back.
//!
//! A unit is one source file for one target: outputs and source maps are per file, a
//! declaration's scope is its file, and a smaller unit would make the compiler merge
//! into a file it does not own, which is itself an act of generation. A larger unit
//! would not fit a request, but several units can share one: that is a [`Batch`]. The one
//! exception is a cycle among files, which no order of units can build: its entities move
//! whole into the unit of the lowest file of the cycle, as a [`Move`] records.
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
//! unit; [`reviews_of`] reads the verdicts one unit's criteria and tests were last given
//! and [`record_reviews`] writes them, so that a criterion whose code has not changed since
//! it was found satisfied is carried rather than reviewed again.
//! Nothing here shells out.

use std::collections::{BTreeMap, HashSet};
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
    self, AppliedSource, Command, Criterion, Entity, EntityId, EntityKind, FileId, Knowledge,
    KnowledgeKind, Model, NodeRef, Operation, Problem, SymbolKind, TypeRef,
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
/// Where the review records of a project live, under the workspace root, one file per unit
/// beside its map file.
// Decision: review records live beside the maps in `elfie-compile` and are checked in,
// because a verdict cannot be rebuilt from the definitions, the maps, and the outputs
// without a verifier.
// @lfy def/generation/main.lfy:reviewsOf
const REVIEWS: &str = "elfie-compile/reviews";

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

/// The lowered node that declares an entity, wherever in the program it was declared; an
/// entity moved into a unit is declared in the file it came from, so the whole program is
/// looked through rather than one file.
// @lfy def/generation/main.lfy:plan
fn lowered_declaration(program: &Program, entity: EntityId) -> Option<&LoweredNode> {
    program
        .files
        .iter()
        .find_map(|file| lowered_node_of(file, entity))
}

/// `Unit.text`: the text of the unit's lowered file without the code of the entities moved
/// out of it, then the lowered code of each entity moved into it; exactly the text of the
/// lowered file when nothing moved in or out.
// Decision: the code of a moved entity is found in the text by the spelling lowering gave
// it, since that text is the rendering of the lowered tree and holds it verbatim; the blank
// line a removal leaves behind is closed up so that two spellings of the same unit are the
// same text.
// @lfy def/generation/main.lfy:plan
fn unit_text(
    model: &Model,
    program: &Program,
    lowered: usize,
    moved_out: &[EntityId],
    moved_in: &[EntityId],
) -> String {
    let file = &program.files[lowered];
    if moved_out.is_empty() && moved_in.is_empty() {
        return file.text.clone();
    }
    let mut text = file.text.clone();
    for &entity in moved_out {
        let Some(node) = lowered_node_of(file, entity) else {
            continue;
        };
        let code = lowered_code(model, node);
        let code = code.trim();
        if !code.is_empty()
            && let Some(at) = text.find(code)
        {
            text.replace_range(at..at + code.len(), "");
        }
    }
    while let Some(at) = text.find("\n\n\n") {
        text.replace_range(at..at + 1, "");
    }
    for &entity in moved_in {
        let Some(node) = lowered_declaration(program, entity) else {
            continue;
        };
        let code = lowered_code(model, node);
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        if !text.is_empty() && !text.ends_with("\n\n") {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push('\n');
        }
        text.push_str(code);
        text.push('\n');
    }
    text
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

/// The local criteria and tests of one entity of a unit, criteria first: those of its
/// declaration in the unit's own lowered file, or, for an entity moved into the unit, those
/// of its declaration in the file it came from.
// @lfy def/generation/main.lfy:request
// @lfy def/generation/main.lfy:review
fn declared_requirements(program: &Program, lowered: usize, entity: EntityId) -> Vec<Requirement> {
    let own = entity_requirements(&program.files[lowered], entity);
    if !own.is_empty() || lowered_node_of(&program.files[lowered], entity).is_some() {
        return own;
    }
    let Some(node) = lowered_declaration(program, entity) else {
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

/// One file as it was when the outputs generated from it were accepted: its lowered text,
/// and its local criteria and tests in the order it gives them.
///
/// The text as written is bound through [`change`](crate::workspace::change) and lowered, so
/// the text that comes back is the lowered text a request carries for the file now and the
/// two can be read side by side, and the ids come out spelled as the unit's own are.
/// `None` when the workspace holds no file at that path.
// @lfy def/generation/main.lfy:request#request:request:e6a9a7f8f68b31456fa6f45f2971df1eddc6eed03b9bcb155872fff58c0ca957
fn file_as_it_was(program: &Program, path: &str, text: &str) -> Option<(String, Vec<Requirement>)> {
    let before = crate::interpret::lower(crate::workspace::change(
        &program.workspace,
        path,
        Some(text),
    ));
    let index = before
        .workspace
        .files
        .iter()
        .position(|file| file.path == path)?;
    let file = &before.files[index];
    Some((file.text.clone(), local_requirements(file)))
}

/// `SourceMap.answered`: the ids of a unit's local criteria and tests, sorted — what a
/// source map records and what [`requirements_hash`] hashes.
// @lfy def/generation/data.lfy:SourceMap.answered
fn answered_ids(program: &Program, unit: &Unit) -> Vec<String> {
    let mut ids: Vec<String> = unit
        .requirements(program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();
    ids.sort();
    ids
}

/// `SourceMap.requirements`: the SHA-256 of the ids of a unit's local criteria and tests,
/// sorted and joined by line breaks.
// @lfy def/generation/main.lfy:plan
fn requirements_hash(program: &Program, unit: &Unit) -> String {
    source_hash(&answered_ids(program, unit).join("\n"))
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
// @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:df415af10d739698174e3358d1293c5c82973190a3202adf8f50300ab0f4f3d5
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

/// SHA-256 of a unit's interface text: what a source map's signature records, so that the
/// text a dependent may rely on and the text the signature hashes are the same text.
// @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:df415af10d739698174e3358d1293c5c82973190a3202adf8f50300ab0f4f3d5
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
/// depends on nothing but what a dependent may rely on. Every type is spelled by
/// [`model::type_text_of`], the one speller of a type as Elfie source, so a generic type
/// keeps its arguments, a type that declares no name reads as it was written, and a
/// function type reads as its parameters and its output; a declaration is spelled by its
/// identifier alone, wherever it was declared, so an entity of the `elfie` package reads
/// `List` or `Path` and never a path or a module name.
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
    // @lfy def/generation/data.lfy:SourceMap#SourceMap:SourceMap:fd62af8a8ab692d82a9ff1cd315fe1ba7411cb837421fff2038240e570114c59
    if let Some(spelled) = model::type_text_of(model, record.ty.as_ref()) {
        let _ = write!(out, " — type `{spelled}`");
    }
    let parameters: Vec<String> = record
        .parameters()
        .iter()
        .map(|&symbol| {
            let symbol = &model.symbols[symbol];
            match model::type_text_of(model, model.entities[symbol.entity].ty.as_ref()) {
                Some(spelled) => format!("{}: {spelled}", symbol.name),
                None => symbol.name.clone(),
            }
        })
        .collect();
    if !parameters.is_empty() {
        let _ = write!(out, " — parameters ({})", parameters.join(", "));
    }
    if let Some(spelled) = model::type_text_of(model, record.output()) {
        let _ = write!(out, " — output `{spelled}`");
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------------------------

/// Every unit of a workspace, which need generating, and how they are batched.
///
/// An entity is built for each target among its own targets; where it has none, for each
/// target among the targets of the anonymous entity of its file, and where that has none
/// either, for each target among the targets of `global`. Every file without a package that
/// has at least one built entity for a target gives one unit holding those entities in file
/// order; a file that has a package gives no unit, since a package is compiled by its own
/// project. That holds for the `elfie` package, the standard library, even when `global`
/// holds a target: none of its entities is ever in a unit's entities, though a unit's
/// interface may still name them as types. An entity of a unit whose member, parameter, or
/// output reaches an entity of a project file that is not built for the unit's target
/// leaves one problem at that member, parameter, or output.
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
    // compile-time result in place.
    // @lfy def/generation/main.lfy:plan#plan:plan:3303e4532dda74155594c480805a56fc6c02f04473a087e4e299c538bd992142
    let lowered = crate::interpret::lower(workspace);
    let program = &lowered;
    let workspace = &program.workspace;
    let model = &workspace.model;
    let mut units: Vec<Unit> = Vec::new();
    let mut moves: Vec<Move> = Vec::new();
    // Every move is decided by the definitions alone and never by the target's guidance, so
    // an entity built for two targets moves the same way for both and the same cycle is
    // reported once however many targets reach it.
    // @lfy def/generation/main.lfy:plan
    let mut problems: Vec<Problem> = Vec::new();

    for (target_index, target) in workspace.targets.iter().enumerate() {
        // Selection: which files give a unit, and with which entities.
        // @lfy def/generation/main.lfy:plan
        let mut unit_of_file: Vec<Option<usize>> = vec![None; workspace.files.len()];
        let mut local: Vec<(usize, Vec<EntityId>)> = Vec::new();
        // Every entity a project file declares that has a lowered node: what could have
        // been built for this target. @lfy def/generation/main.lfy:plan
        let mut buildable: Vec<EntityId> = Vec::new();
        for (file_index, file) in workspace.files.iter().enumerate() {
            if file.package.is_some() {
                continue; // @lfy def/generation/main.lfy:plan
            }
            // An entity with no lowered node, such as a trait or an ace function, is never
            // among a unit's entities, so no output has to carry a marker for it.
            // @lfy def/generation/main.lfy:plan
            let lowered = lowered_entities(&program.files[file_index]);
            buildable.extend(lowered.iter().copied());
            // @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
            let entities: Vec<EntityId> = built_entities(model, file.source, target.declaration)
                .into_iter()
                .filter(|entity| lowered.contains(entity))
                .collect();
            if entities.is_empty() {
                continue;
            }
            unit_of_file[file_index] = Some(local.len());
            local.push((file_index, entities));
        }

        // An entity of a unit whose type reaches an entity of a project file that is not
        // built for this target: the reached code would not be there to use.
        // @lfy def/generation/main.lfy:plan#plan:plan:28729b42d15d20d9238283f1a0077d5bcf0dffd32f4bf589ea2c0f639e467065
        let built: Vec<EntityId> = local
            .iter()
            .flat_map(|(_, entities)| entities.iter().copied())
            .collect();
        for problem in unbuilt_problems(workspace, &built, &buildable, &target.identifier) {
            if !problems.contains(&problem) {
                problems.push(problem);
            }
        }

        // Dependencies the uses give, as positions among this target's units.
        // @lfy def/generation/main.lfy:plan
        let uses: Vec<Vec<usize>> = local
            .iter()
            .map(|(file_index, _)| dependencies_of(workspace, *file_index, &unit_of_file))
            .collect();

        // Cycles among the units of this target are resolved before anything else is
        // derived from them: a move changes which entities a unit holds, and so its
        // dependencies, its text, and its reason.
        // @lfy def/generation/main.lfy:plan
        let files: Vec<usize> = local.iter().map(|(file_index, _)| *file_index).collect();
        let own: Vec<Vec<EntityId>> = local.iter().map(|(_, entities)| entities.clone()).collect();
        let mut entities = own.clone();
        let mut dependencies: Vec<Vec<usize>> = Vec::new();
        let mut local_moves: Vec<Moved> = Vec::new();
        resolve_cycles(
            workspace,
            &files,
            &uses,
            &mut entities,
            &mut dependencies,
            &mut local_moves,
            &mut problems,
        );

        // Order: each unit after its dependencies, otherwise in file order.
        // @lfy def/generation/main.lfy:plan
        let order = order_after_dependencies(&dependencies);
        let base = units.len();
        let mut position = vec![0; local.len()];
        for (offset, &item) in order.iter().enumerate() {
            position[item] = base + offset;
        }

        for &item in &order {
            let file_index = files[item];
            let file = &workspace.files[file_index];
            // What the file declared and no longer holds, and what moved into it, in the
            // order the moves were made. @lfy def/generation/main.lfy:plan
            let moved_out: Vec<EntityId> = own[item]
                .iter()
                .copied()
                .filter(|entity| !entities[item].contains(entity))
                .collect();
            let moved_in: Vec<EntityId> = entities[item]
                .iter()
                .copied()
                .filter(|entity| !own[item].contains(entity))
                .collect();
            units.push(Unit {
                target: target_index,
                file: file_index,
                entities: entities[item].clone(),
                // The program's files run parallel to the workspace's, so a file's index
                // is the index of the file lowered from it.
                // @lfy def/generation/main.lfy:plan#plan:plan:f1d41358bbe62b7bc098a9056623218e8f88baf629ec9d4f2caab39e89f37bfc
                lowered: file_index,
                // @lfy def/generation/main.lfy:plan
                text: unit_text(model, program, file_index, &moved_out, &moved_in),
                stem: stem_of(&file.path, &workspace.source_directory), // @lfy def/generation/main.lfy:plan
                dependencies: dependencies[item].iter().map(|&d| position[d]).collect(),
                // @lfy def/generation/main.lfy:plan#plan:plan:84b9ddea7d9b6f73bdde004e1e9dee2c746f57ff862c8302fd0a3150a4df7817
                outputs: source_maps
                    .iter()
                    .filter(|map| map.target == target.identifier && map.source == file.path)
                    .cloned()
                    .collect(),
                reason: None,
            });
        }
        // @lfy def/generation/main.lfy:plan
        for moved in local_moves {
            moves.push(Move {
                entities: moved.entities,
                origin: position[moved.origin],
                into: position[moved.into],
                cause: moved.cause,
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

    let batches = batches_of(&units);
    // @lfy def/generation/main.lfy:plan
    (
        lowered,
        Plan {
            units,
            moves,
            problems,
            batches,
        },
    )
}

/// One move, with the units it is between given as positions among one target's units: what
/// [`resolve_cycles`] records before the units are ordered.
// @lfy def/generation/main.lfy:plan
struct Moved {
    entities: Vec<EntityId>,
    origin: usize,
    into: usize,
    cause: String,
}

/// Entities moved between the units of one target until no two of them reach each other,
/// with the dependencies the last round left.
///
/// A unit depends on the unit of each file its file uses and on every unit holding an entity
/// its entities reach, and on the unit an entity of it moved into. A cycle is two or more
/// units each of which depends on every other, directly or through other units. Its lowest
/// file is the file of one of its units that the file of every other unit of the cycle uses,
/// directly or through other files; every entity of another unit of the cycle that an entity
/// of the lowest file's unit reaches moves into the lowest file's unit, and each unit it
/// leaves gives one move. A cycle no file of which is used by every other leaves one problem
/// and nothing moves for it.
// Decision: when more than one file of a cycle is used by every other — which only a cycle
// among uses can give, and which no move resolves — the first in file order is taken as the
// lowest, so the plan is the same however the files were visited.
// @lfy def/generation/main.lfy:plan
fn resolve_cycles(
    workspace: &Workspace,
    files: &[usize],
    uses: &[Vec<usize>],
    entities: &mut [Vec<EntityId>],
    dependencies: &mut Vec<Vec<usize>>,
    moved: &mut Vec<Moved>,
    problems: &mut Vec<Problem>,
) {
    let model = &workspace.model;
    // A cycle that gives no move is left as it is, so the rounds end.
    // @lfy def/generation/main.lfy:plan
    let mut settled: Vec<Vec<usize>> = Vec::new();
    loop {
        *dependencies = (0..files.len())
            .map(|unit| {
                let mut out = uses[unit].clone();
                // @lfy def/generation/main.lfy:plan
                let reached = reached_by(model, &entities[unit]);
                for (other, held) in entities.iter().enumerate() {
                    if other != unit
                        && !out.contains(&other)
                        && held.iter().any(|entity| reached.contains(entity))
                    {
                        out.push(other);
                    }
                }
                // @lfy def/generation/main.lfy:plan
                for record in moved.iter().filter(|record| record.origin == unit) {
                    if !out.contains(&record.into) {
                        out.push(record.into);
                    }
                }
                out
            })
            .collect();

        let mut again = false;
        // @lfy def/generation/main.lfy:plan
        for cycle in cycles(dependencies) {
            if settled.contains(&cycle) {
                continue;
            }
            // @lfy def/generation/main.lfy:plan
            let Some(lowest) = lowest_file(workspace, files, &cycle) else {
                // A cycle whose files use one another is the uses' own doing, and the
                // workspace already holds a problem at each of those uses: planning adds
                // nothing to it and moves nothing out of it.
                // @lfy def/generation/main.lfy:plan#plan:plan:9e6d3d70db8eebce09554eb34d8e0c45134ec1eba5d58f6da4e57b8a378c0f4e
                if !cycle_among_uses(workspace, files, &cycle) {
                    // @lfy def/generation/main.lfy:plan#plan:plan:fa7d90dd8bdab380388515f7c40bb7848f1274eb5d75942a04288b75a656c7bf
                    let problem = cycle_problem(workspace, files, entities, &cycle);
                    if !problems.contains(&problem) {
                        problems.push(problem);
                    }
                }
                settled.push(cycle);
                continue;
            };
            // @lfy def/generation/main.lfy:plan
            let reached = reached_by(model, &entities[lowest]);
            let mut made = false;
            for &other in cycle.iter().filter(|&&unit| unit != lowest) {
                let moving: Vec<EntityId> = entities[other]
                    .iter()
                    .copied()
                    .filter(|entity| reached.contains(entity))
                    .collect();
                if moving.is_empty() {
                    continue;
                }
                // @lfy def/generation/main.lfy:plan
                let cause = cause_of(model, &entities[lowest], moving[0]);
                entities[other].retain(|entity| !moving.contains(entity));
                entities[lowest].extend(moving.iter().copied());
                moved.push(Moved {
                    entities: moving,
                    origin: other,
                    into: lowest,
                    cause,
                });
                made = true;
            }
            if made {
                again = true;
                break;
            }
            // A cycle whose lowest file reaches nothing of the others leaves them where they
            // are: there is nothing to move, so the rounds end rather than repeat.
            // @lfy def/generation/main.lfy:plan
            settled.push(cycle);
        }
        if !again {
            return;
        }
    }
}

/// Every group of two or more items each of which depends on every other, directly or
/// through other items, each item in at most one group and the groups in item order.
// @lfy def/generation/main.lfy:plan#plan:plan:5ea9c1a46ff002cb966b639dd7fa5752f17660d0d99bf21d939baffb3c345b31
fn cycles(dependencies: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let count = dependencies.len();
    let mut reaches = vec![vec![false; count]; count];
    for (item, row) in dependencies.iter().enumerate() {
        for &dependency in row {
            reaches[item][dependency] = true;
        }
    }
    for through in 0..count {
        for item in 0..count {
            if reaches[item][through] {
                let onward = reaches[through].clone();
                for (to, further) in onward.iter().enumerate() {
                    if *further {
                        reaches[item][to] = true;
                    }
                }
            }
        }
    }
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut grouped = vec![false; count];
    for item in 0..count {
        if grouped[item] {
            continue;
        }
        let group: Vec<usize> = (0..count)
            .filter(|&other| other == item || (reaches[item][other] && reaches[other][item]))
            .collect();
        for &other in &group {
            grouped[other] = true;
        }
        if group.len() > 1 {
            out.push(group);
        }
    }
    out
}

/// The unit of a cycle whose file the file of every other unit of the cycle uses, directly or
/// through other files, and which itself uses none of them; the first such in file order, and
/// `None` when there is none.
///
/// Both halves are needed: a file that uses another file of the cycle is below nothing, so a
/// cycle among uses, where each file uses every other, has no lowest file at all.
// @lfy def/generation/main.lfy:plan#plan:plan:e32007c54b7c28a4e6102a813e4c61dc2c7f36b487af8e3f5c778220338975cf
fn lowest_file(workspace: &Workspace, files: &[usize], cycle: &[usize]) -> Option<usize> {
    let mut candidates: Vec<usize> = cycle
        .iter()
        .copied()
        .filter(|&lowest| {
            cycle.iter().all(|&other| {
                other == lowest
                    || (used_by(workspace, files[other], files[lowest])
                        && !used_by(workspace, files[lowest], files[other]))
            })
        })
        .collect();
    candidates.sort_by_key(|&unit| files[unit]);
    candidates.first().copied()
}

/// Whether the files of a cycle's units form a cycle among uses: two of them each of which
/// uses the other, directly or through other files.
// @lfy def/generation/main.lfy:plan#plan:plan:9e6d3d70db8eebce09554eb34d8e0c45134ec1eba5d58f6da4e57b8a378c0f4e
fn cycle_among_uses(workspace: &Workspace, files: &[usize], cycle: &[usize]) -> bool {
    cycle.iter().any(|&one| {
        cycle.iter().any(|&other| {
            other != one
                && used_by(workspace, files[one], files[other])
                && used_by(workspace, files[other], files[one])
        })
    })
}

/// Whether one file uses another, directly or through other files.
// @lfy def/generation/main.lfy:plan
fn used_by(workspace: &Workspace, from: usize, to: usize) -> bool {
    let mut seen: Vec<usize> = vec![from];
    let mut next = 0;
    while next < seen.len() {
        let file = seen[next];
        next += 1;
        let source = workspace.files[file].source;
        for used in workspace.model.sources[source].uses.iter().flatten() {
            let Some(index) = workspace.files.iter().position(|file| &file.path == used) else {
                continue;
            };
            if index == to {
                return true;
            }
            if !seen.contains(&index) {
                seen.push(index);
            }
        }
    }
    false
}

/// `Move.cause`: the member, parameter, or output of an entity of the receiving unit whose
/// type reaches the first entity moved, spelled as the owner's identifier, a dot, and the
/// member's or parameter's name, or the owner's identifier alone for an output.
// @lfy def/generation/main.lfy:plan
fn cause_of(model: &Model, holders: &[EntityId], moved: EntityId) -> String {
    for &owner in holders {
        let name = model.entities[owner]
            .identifier
            .clone()
            .unwrap_or_default();
        for member in all_members(model, owner) {
            if reaches(model, model.entities[member].ty.as_ref(), moved) {
                let member = model.entities[member]
                    .identifier
                    .clone()
                    .unwrap_or_default();
                return format!("{name}.{member}");
            }
        }
        for &symbol in model.entities[owner].parameters() {
            let parameter = &model.symbols[symbol];
            if reaches(
                model,
                model.entities[parameter.entity].ty.as_ref(),
                moved,
            ) {
                return format!("{name}.{}", parameter.name);
            }
        }
        if reaches(model, model.entities[owner].output(), moved) {
            return name;
        }
    }
    String::new()
}

/// One problem per entity of a unit of one target whose member, parameter, or output
/// reaches an entity of a project file that has a lowered node and is not built for that
/// target: each sits at that member, parameter, or output, and names both entities and the
/// target.
// @lfy def/generation/main.lfy:plan#plan:plan:28729b42d15d20d9238283f1a0077d5bcf0dffd32f4bf589ea2c0f639e467065
fn unbuilt_problems(
    workspace: &Workspace,
    built: &[EntityId],
    buildable: &[EntityId],
    target: &str,
) -> Vec<Problem> {
    let model = &workspace.model;
    let mut out: Vec<Problem> = Vec::new();
    let missing = |ty: Option<&TypeRef>| -> Vec<EntityId> {
        closure(model, &named_entities_of(model, ty))
            .into_iter()
            .filter(|entity| buildable.contains(entity) && !built.contains(entity))
            .collect()
    };
    let mut add = |node: Option<NodeRef>, place: String, reached: Vec<EntityId>| {
        let Some(node) = node else { return };
        for entity in reached {
            let problem = Problem {
                node,
                message: format!(
                    "{place} reaches {}, which is not built for the target {target}",
                    entity_name(model, entity)
                ),
                stage: crate::model::Stage::Generation,
            };
            if !out.contains(&problem) {
                out.push(problem);
            }
        }
    };
    for &owner in built {
        let name = entity_name(model, owner);
        for member in all_members(model, owner) {
            let record = &model.entities[member];
            add(
                record.node,
                format!("the member {name}.{}", entity_name(model, member)),
                missing(record.ty.as_ref()),
            );
        }
        for &symbol in model.entities[owner].parameters() {
            let parameter = model.symbols[symbol].entity;
            let spelled = format!("the parameter {name}.{}", model.symbols[symbol].name);
            add(
                model.entities[parameter].node,
                spelled,
                missing(model.entities[parameter].ty.as_ref()),
            );
        }
        add(
            model.entities[owner].node,
            format!("the output of {name}"),
            missing(model.entities[owner].output()),
        );
    }
    out
}

/// The problem a cycle no file of which is used by every other leaves: it names every file of
/// the cycle, and sits at the first declaration, in file order and then source order, of a
/// member, parameter, or output whose type reaches an entity of another file of the cycle.
// @lfy def/generation/main.lfy:plan
fn cycle_problem(
    workspace: &Workspace,
    files: &[usize],
    entities: &[Vec<EntityId>],
    cycle: &[usize],
) -> Problem {
    let model = &workspace.model;
    let mut order: Vec<usize> = cycle.to_vec();
    order.sort_by_key(|&unit| files[unit]);
    let paths: Vec<String> = order
        .iter()
        .map(|&unit| workspace.files[files[unit]].path.clone())
        .collect();
    let message = format!(
        "{} reach each other, and no file of the cycle is used by every other, so no entity of \
         it can move into the lowest file",
        paths.join(" and ")
    );
    let elsewhere: Vec<EntityId> = order
        .iter()
        .flat_map(|&unit| entities[unit].iter().copied())
        .collect();
    // Every declaration that reaches across the cycle, and then the first of them in file
    // order and, within one file, in source order.
    // @lfy def/generation/main.lfy:plan
    let mut candidates: Vec<NodeRef> = Vec::new();
    let mut fallback: Option<NodeRef> = None;
    for &unit in &order {
        for &owner in &entities[unit] {
            let declared = model.entities[owner].file;
            let foreign = |entity: &EntityId| {
                elsewhere.contains(entity) && model.entities[*entity].file != declared
            };
            fallback = fallback.or(model.entities[owner].node);
            for member in all_members(model, owner) {
                if named_entities_of(model, model.entities[member].ty.as_ref())
                    .iter()
                    .any(foreign)
                    && let Some(node) = model.entities[member].node
                {
                    candidates.push(node);
                }
            }
            for &symbol in model.entities[owner].parameters() {
                let parameter = model.symbols[symbol].entity;
                if named_entities_of(model, model.entities[parameter].ty.as_ref())
                    .iter()
                    .any(foreign)
                    && let Some(node) = model.entities[parameter].node
                {
                    candidates.push(node);
                }
            }
            if named_entities_of(model, model.entities[owner].output())
                .iter()
                .any(foreign)
                && let Some(node) = model.entities[owner].node
            {
                candidates.push(node);
            }
        }
    }
    let place = |node: &NodeRef| {
        (
            workspace
                .files
                .iter()
                .position(|file| file.source == node.file)
                .unwrap_or(usize::MAX),
            node.index,
        )
    };
    candidates.sort_by_key(place);
    Problem {
        node: candidates
            .first()
            .copied()
            .or(fallback)
            .unwrap_or(NodeRef { file: 0, index: 0 }),
        message,
        stage: crate::model::Stage::Generation,
    }
}

/// Whether a type reaches an entity: the entity is named by the type, or by the type of a
/// member, parameter, or output of something the type names, however deep.
// @lfy def/generation/main.lfy:plan
fn reaches(model: &Model, ty: Option<&TypeRef>, entity: EntityId) -> bool {
    let named = named_entities_of(model, ty);
    named.contains(&entity) || closure(model, &named).contains(&entity)
}

/// Every entity a type names, type arguments included.
// @lfy def/generation/main.lfy:plan
fn named_entities_of(model: &Model, ty: Option<&TypeRef>) -> Vec<EntityId> {
    let mut out = Vec::new();
    if let Some(ty) = ty {
        type_entities(model, ty, &mut out);
    }
    out
}

/// Every entity one type names, type arguments included, appended.
// @lfy def/generation/main.lfy:plan
fn type_entities(model: &Model, ty: &TypeRef, out: &mut Vec<EntityId>) {
    match ty {
        TypeRef::Entity(entity) | TypeRef::Predicate(entity) => {
            if out.contains(entity) {
                return;
            }
            out.push(*entity);
            // @lfy def/generation/main.lfy:plan
            for argument in model.type_arguments(*entity) {
                type_entities(model, &argument, out);
            }
        }
        TypeRef::List(item) => type_entities(model, item, out),
        TypeRef::Union(items) => {
            for item in items {
                type_entities(model, item, out);
            }
        }
        _ => {}
    }
}

/// Every entity the types of one entity name: its own type, the types of its members, those
/// a trait added included, the types of its parameters, and its output. Only types are
/// followed, so a reference in a definition, a criterion, or a test names nothing here.
// @lfy def/generation/main.lfy:plan
fn named_by(model: &Model, entity: EntityId, out: &mut Vec<EntityId>) {
    let record = &model.entities[entity];
    if let Some(ty) = &record.ty {
        type_entities(model, ty, out);
    }
    if let Some(output) = record.output() {
        type_entities(model, output, out);
    }
    for &symbol in record.parameters() {
        if let Some(ty) = &model.entities[model.symbols[symbol].entity].ty {
            type_entities(model, ty, out);
        }
    }
    // @lfy def/generation/main.lfy:plan
    for member in all_members(model, entity) {
        if let Some(ty) = &model.entities[member].ty {
            type_entities(model, ty, out);
        }
    }
}

/// The members an entity declares and the members every trait applied to it added, each
/// once, the entity's own first.
// @lfy def/generation/main.lfy:plan
fn all_members(model: &Model, entity: EntityId) -> Vec<EntityId> {
    let mut out = members_of(model, entity);
    for applied in &model.entities[entity].traits {
        for member in members_of(model, applied.entity) {
            if !out.contains(&member) {
                out.push(member);
            }
        }
    }
    out
}

/// Every entity a set of entities reaches: what their types name, and what those name, until
/// nothing new appears.
// @lfy def/generation/main.lfy:plan
fn closure(model: &Model, named: &[EntityId]) -> Vec<EntityId> {
    let mut out: Vec<EntityId> = Vec::new();
    let mut pending: Vec<EntityId> = named.to_vec();
    while let Some(entity) = pending.pop() {
        if out.contains(&entity) {
            continue;
        }
        out.push(entity);
        named_by(model, entity, &mut pending);
    }
    out
}

/// Every entity the entities of a unit reach: reaching is transitive, so what their types
/// name is followed until nothing new appears.
// @lfy def/generation/main.lfy:plan
fn reached_by(model: &Model, roots: &[EntityId]) -> Vec<EntityId> {
    let mut named = Vec::new();
    for &root in roots {
        named_by(model, root, &mut named);
    }
    closure(model, &named)
}

/// The entities of a file built for one target, named by the `ace const` that declares
/// it, in file order.
///
/// An entity whose own targets are not empty is built for each of them and for no other;
/// one whose own targets are empty takes the targets of the anonymous entity of its file,
/// and, where those are empty too, the targets of `global`. Only an entity declared in the
/// file scope of the file is built: a member, a parameter, or an enum member goes with the
/// entity that holds it.
// @lfy def/generation/main.lfy:plan#plan:plan:c3b45b9c75fa54e5426018d8de412d9c76d287a296f9384ce03c03717a3ee93c
fn built_entities(model: &Model, source: usize, declaration: EntityId) -> Vec<EntityId> {
    // @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
    let global: &[EntityId] = &model.entities[model.global].targets;
    // @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
    let of_file: &[EntityId] = model
        .file_entities
        .get(source)
        .map_or(&[], |&entity| model.entities[entity].targets.as_slice());
    let mut out: Vec<EntityId> = Vec::new();
    if let Some(&scope) = model.file_scopes.get(source) {
        for symbol in model.scopes[scope].declared() {
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
            // @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
            let chosen: &[EntityId] = if !entity.targets.is_empty() {
                entity.targets.as_slice()
            } else if of_file.is_empty() {
                global
            } else {
                of_file
            };
            if chosen.contains(&declaration) {
                out.push(symbol.entity);
            }
        }
    }
    out.sort_by_key(|&entity| model.entities[entity].node.map(|node| node.index));
    out
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

/// `Unit.stem`: `Path.relative` of the file's path to the source directory, without the
/// `.lfy` extension. A file the program reached through a `use` from outside the source
/// directory names itself from there, with one `..` per segment the source directory has
/// beyond the ones they share, so that two such files cannot collide.
// @lfy def/generation/main.lfy:plan
// @lfy def/generation/data.lfy:Unit#Unit:Unit:1fc2e33009d6c55647083813e41979cdcd5f51fe2a856a9f6815fb4aa2b68bd3
fn stem_of(path: &str, source_directory: &str) -> String {
    // A unit is only ever made for a file of the project itself, so the file's path and the
    // source directory are both relative to the root and `Path.relative` of the two is
    // always defined; a path that is somehow not keeps itself.
    let relative = path_relative(path, source_directory).unwrap_or_else(|| path.to_string());
    relative
        .strip_suffix(EXTENSION)
        .unwrap_or(&relative)
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
        // @lfy def/generation/main.lfy:plan#plan:plan:68cc4a6ef4fd0e7da9d24e21b1ef05a1e1d173ad7313adf6c3971c21bd01bd3c
        return Some(Reason::Requested);
    }
    if violated.iter().any(|stem| stem == &unit.stem) {
        // @lfy def/generation/main.lfy:plan#plan:plan:d02d6ed2f21e62878f97bfce1df0638b8152e90e9d1fbd87429ffad5d31a7be5
        return Some(Reason::Violated);
    }
    if unit.outputs.is_empty() {
        // @lfy def/generation/main.lfy:plan#plan:plan:01751bca4d2165839842ab83d64cf2cab13330a74b283b7c2cf83fd8513371c3
        return Some(Reason::Fresh);
    }
    // The lowered text the unit compiles, not the source: an edit that leaves the lowered
    // code the same, such as a comment, changes nothing.
    // @lfy def/generation/main.lfy:plan#plan:plan:699a0b8df170aed10dd0c3f2cdf3492c7eff895d33b7e542b8928c357895c458
    let hash = source_hash(&unit.text);
    if unit.outputs.iter().any(|output| output.hash != hash) {
        return Some(Reason::Changed);
    }
    // @lfy def/generation/main.lfy:plan#plan:plan:7725d70d35f6e8be6804e106099b40d9930e162127a1e2c046e3b3a2ec0ff846
    let requirements = requirements_hash(program, unit);
    if unit
        .outputs
        .iter()
        .any(|output| output.requirements != requirements)
    {
        return Some(Reason::Requirements);
    }
    // A dependency whose interface differs from the one the outputs were generated
    // against, or which they record nothing for, plans the unit. An edit that leaves every
    // dependency's interface the same — the body of one fn, say — therefore plans nothing
    // but the unit holding it.
    // @lfy def/generation/main.lfy:plan#plan:plan:43f3dac0dd4bb25573f9bff0c0f81d674671504e8ac666c8ae2bf37faf2d75f8
    let stale = unit.dependencies.iter().any(|&dependency| {
        let path = &workspace.files[units[dependency].file].path;
        let signature = &signatures[dependency];
        unit.outputs
            .iter()
            .any(|output| output.dependencies.get(path) != Some(signature))
    });
    if stale {
        // @lfy def/generation/main.lfy:plan#plan:plan:772d227fa53dc6d543215df30f2905bc04c628366f07dcbff492bb5ce7333638
        return Some(Reason::Dependency);
    }
    // @lfy def/generation/main.lfy:plan#plan:plan:40f45c8240354c40ed6f2a0ef91f2c24787a209455f9f1a93d9e45ae9c1b5dd7
    None
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
// @lfy def/generation/main.lfy:plan#plan:plan:88eb512d83f4888e4edcac53d09e9ed1d190eb09ce99803fb65ec6e80776586c
fn batches_of(units: &[Unit]) -> Vec<Batch> {
    // Decision: the criteria give one guidance and one set of native dependencies per
    // request, so a batch holds units of one target; two targets never share a batch even
    // when their stems agree.
    let mut batches: Vec<Batch> = Vec::new();
    let mut open_characters = 0usize;
    for (index, unit) in units.iter().enumerate() {
        if unit.reason.is_none() {
            continue; // @lfy def/generation/main.lfy:plan
        }
        // The source a request carries is the unit's lowered text, so that is what is
        // measured.
        let characters = unit.text.chars().count();
        let joins = batches.last().is_some_and(|open| {
            let first = &units[open.units[0]];
            // Decision: the exception waives the directory, not the size of a batch; a
            // batch that is already full opens a new one whatever the reason.
            // @lfy def/generation/main.lfy:plan#plan:plan:83ae85d53a06f4a097fb8db49ddcb5211746d5285060c427ab68e0f4d46b4b30
            let dependent = unit.reason == Some(Reason::Dependency)
                // Every planned dependency of the unit is in the open batch, and there is at
                // least one: a unit with no planned dependency waives nothing.
                && unit
                    .dependencies
                    .iter()
                    .all(|&d| units[d].reason.is_none() || open.units.contains(&d))
                && unit
                    .dependencies
                    .iter()
                    .any(|&d| units[d].reason.is_some() && open.units.contains(&d));
            // @lfy def/generation/main.lfy:plan#plan:plan:139ff97944391c8eb7f3b810897377a8bd64ea54d0fe64b6c29a7043348a5379
            first.target == unit.target
                // @lfy def/generation/main.lfy:plan#plan:plan:f5945b26c42abf21599b311c49e8448eb81d2fad73e72ff70a5affec2d2f63c7
                && (first_segment(&first.stem) == first_segment(&unit.stem) || dependent)
                // @lfy def/generation/main.lfy:plan#plan:plan:46d51e256ac0ed89b3413a7e235770719a0f216b2b18d7b133e7e05e6d930718
                && open.units.len() < BATCH_UNITS
                // @lfy def/generation/main.lfy:plan#plan:plan:8b23e12f7a1211640db7cdf6e25859876cd4e9df01a21c0766522b1249acc498
                && open_characters + characters < BATCH_CHARACTERS
        });
        match batches.last_mut() {
            Some(open) if joins => {
                open.units.push(index);
                open_characters += characters;
            }
            // With no batch open, or with the open one refusing the unit, the unit opens a
            // new batch of its own.
            // @lfy def/generation/main.lfy:plan#plan:plan:018965380af8e7bb6f00c88630feb1dbcd8a2af6c98cfbe47cdbb5c5c5714805
            // @lfy def/generation/main.lfy:plan#plan:plan:b7a4d3d09495c60318a05c9c39c728fffd85508badbf8aa414d70dbe6b2aef66
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
/// in it; the guidance is every criterion of the target's declaration, of each of its
/// layers, and of every trait those layers extend, as [`guidance_for`] gathers them; the
/// native dependencies and the commands are the target's; and the knowledge is the
/// target's, then that of each entity of the batch's units, then that of each entity of an
/// interface, each item once. The instructions quote every criterion and test already
/// resolved, so a compiler with no access to the model can still work.
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
            answered: BTreeMap::new(),
            removed: BTreeMap::new(),
            existing: BTreeMap::new(),
            interfaces: Vec::new(),
            guidance: Vec::new(),
            native_dependencies: Vec::new(),
            knowledge: Vec::new(),
            commands: Vec::new(),
        };
    };
    let target = &workspace.targets[plan.units[first].target];

    // @lfy def/generation/main.lfy:request
    let sources: BTreeMap<String, String> = batch
        .units
        .iter()
        .map(|&index| {
            let unit = &plan.units[index];
            (workspace.files[unit.file].path.clone(), unit.text.clone())
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
                unit.requirements(program),
            )
        })
        .collect();
    // The file of each unit of the batch as it was when its outputs were accepted, lowered
    // the way its file is lowered now.
    // @lfy def/generation/main.lfy:request#request:request:e6a9a7f8f68b31456fa6f45f2971df1eddc6eed03b9bcb155872fff58c0ca957
    let was: BTreeMap<String, (String, Vec<Requirement>)> = batch
        .units
        .iter()
        .filter_map(|&index| {
            let path = &workspace.files[plan.units[index].file].path;
            let text = previous.get(path)?;
            let (lowered, requirements) = file_as_it_was(program, path, text)?;
            Some((path.clone(), (lowered, requirements)))
        })
        .collect();
    // The lowered text each unit's outputs were generated from, which the instructions read
    // against the lowered text of the unit now.
    // @lfy def/generation/main.lfy:request#request:request:6232735fed99654f7d25efa96687134e92c7667e0be166ba2471ca9d018c3103
    let previous_lowered: BTreeMap<String, String> = was
        .iter()
        .map(|(path, (lowered, _))| (path.clone(), lowered.clone()))
        .collect();
    // The ids the existing outputs were generated against: what the map file of the unit
    // recorded, or, where it recorded none, the ids of the file as it was.
    // @lfy def/generation/main.lfy:request#request:request:160056c645e3e69afd7ce0bb74137db9a5e5f286df0a983570f9e2569276b533
    // @lfy def/generation/main.lfy:request#request:request:17f8da3e2ff4913864923cd11e57fe5ff20121b6a364f38e388c25c9113dff54
    let answered: BTreeMap<String, Vec<String>> = batch
        .units
        .iter()
        .filter_map(|&index| {
            let path = &workspace.files[plan.units[index].file].path;
            let recorded = plan.units[index]
                .outputs
                .iter()
                .find(|map| !map.answered.is_empty())
                .map(|map| map.answered.clone());
            let ids = match recorded {
                Some(ids) => ids,
                None => was
                    .get(path)?
                    .1
                    .iter()
                    .map(|requirement| requirement.id().to_string())
                    .collect(),
            };
            Some((path.clone(), ids))
        })
        .collect();
    // Each criterion and test of the file as it was that the unit no longer has, in the
    // order the file as it was gives them: what a compiler reading the existing code needs
    // in order to tell code that answers for nothing any more from code that still answers.
    // @lfy def/generation/main.lfy:request#request:request:e9e73367084b12788d91ea49ea57894f06e55c67ebf0856dbb827d3a968fc089
    let removed_requirements: BTreeMap<String, Vec<Requirement>> = was
        .iter()
        .filter_map(|(path, (_, before))| {
            let held = requirements.get(path)?;
            let gone: Vec<Requirement> = before
                .iter()
                .filter(|requirement| {
                    !held.iter().any(|other| other.id() == requirement.id())
                })
                .cloned()
                .collect();
            (!gone.is_empty()).then(|| (path.clone(), gone))
        })
        .collect();
    // @lfy def/generation/data.lfy:Request.removed
    let removed: BTreeMap<String, Vec<String>> = removed_requirements
        .iter()
        .map(|(path, gone)| {
            (
                path.clone(),
                gone.iter()
                    .map(|requirement| requirement.id().to_string())
                    .collect(),
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
    let guidance = guidance_for(model, target);
    // @lfy def/generation/main.lfy:request
    let native_dependencies: Vec<NativeDependency> = target.native_dependencies.clone();
    let commands: Vec<Command> = target.commands.clone();
    // The target's knowledge, then that of each entity of the batch's units, then that of
    // each entity of an interface, each item once.
    // @lfy def/generation/main.lfy:request
    let mut knowledge: Vec<Knowledge> = Vec::new();
    let hold = |items: &[Knowledge], knowledge: &mut Vec<Knowledge>| {
        for item in items {
            if !knowledge.contains(item) {
                knowledge.push(item.clone());
            }
        }
    };
    hold(&target.knowledge, &mut knowledge);
    for &index in &batch.units {
        for &entity in &plan.units[index].entities {
            hold(&model.entities[entity].knowledge, &mut knowledge);
        }
    }
    for interface in &interfaces {
        for &entity in &interface.entities {
            hold(&model.entities[entity].knowledge, &mut knowledge);
        }
    }

    let instructions = instructions(
        program,
        plan,
        batch,
        target,
        &Quoted {
            sources: &sources,
            globals: &globals,
            previous: &previous_lowered,
            answered: &answered,
            removed: &removed_requirements,
            existing: &existing,
            interfaces: &interfaces,
            guidance: &guidance,
            native_dependencies: &native_dependencies,
            knowledge: &knowledge,
            commands: &commands,
        },
    );

    Request {
        batch: batch.clone(),
        instructions,
        sources,
        requirements,
        globals,
        previous: previous_lowered,
        answered,
        removed,
        existing,
        interfaces,
        guidance,
        native_dependencies,
        knowledge,
        commands,
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
    answered: &'r BTreeMap<String, Vec<String>>,
    removed: &'r BTreeMap<String, Vec<Requirement>>,
    existing: &'r BTreeMap<String, String>,
    interfaces: &'r [Interface],
    guidance: &'r [Criterion],
    native_dependencies: &'r [NativeDependency],
    knowledge: &'r [Knowledge],
    commands: &'r [Command],
}

/// The guidance of a target: the criteria of its declaration that neither a layer nor a
/// trait a layer extends gave, in order; then those each of its layers contributed, layer by
/// layer in guidance order; then those of every trait the layers extend, transitively and
/// nearest first, each trait once.
///
/// Every template value is already rendered from the arguments the layer was applied with,
/// since applying a layer to the declaration resolved each of its criteria for that
/// application; a trait the layers extend whose criteria the declaration does not carry
/// still gives its own.
// Decision: the criteria are read off the declaration rather than off each trait, because a
// trait's own criteria keep a template value of its parameters as written — the parameters
// are unbound where the trait is declared — and the application that bound them is what the
// declaration holds.
// @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
pub fn guidance_for(model: &Model, target: &Target) -> Vec<Criterion> {
    let held = model::criteria_of(model, target.declaration);
    // A trait reached through two layers is inherited by the declaration once per path, so
    // what one contributor gave is taken once.
    // @lfy def/generation/main.lfy:request
    let once = |criteria: Vec<Criterion>| -> Vec<Criterion> {
        let mut out: Vec<Criterion> = Vec::new();
        for criterion in criteria {
            if !out.contains(&criterion) {
                out.push(criterion);
            }
        }
        out
    };
    // The layers, then every trait they extend, gathered before anything is taken, because
    // what comes first is what none of them gave. Nearest first: a trait is reached a whole
    // level after the trait that extends it, and a trait reached twice keeps its first
    // place.
    // @lfy def/generation/main.lfy:request
    let mut layered: Vec<EntityId> = Vec::new();
    for &layer in &target.layers {
        if layered.contains(&layer) {
            continue; // @lfy def/generation/main.lfy:request
        }
        layered.push(layer);
    }
    let mut reached = 0;
    while reached < layered.len() {
        let entity = layered[reached];
        reached += 1;
        for applied in &model.entities[entity].traits {
            if !matches!(applied.source, AppliedSource::Extends(_))
                || layered.contains(&applied.entity)
            {
                continue; // @lfy def/generation/main.lfy:request
            }
            layered.push(applied.entity);
        }
    }
    // Whoever else gave the declaration a criterion comes first: the declaration's own body,
    // or the file that adds one with an add call on the const's acceptanceCriteria.
    // @lfy def/generation/main.lfy:request
    let mut out: Vec<Criterion> = once(
        held.iter()
            .filter(|criterion| !layered.contains(&criterion.contributor))
            .cloned()
            .collect(),
    );
    let by = |contributor: EntityId| -> Vec<Criterion> {
        let mine = once(
            held.iter()
                .filter(|criterion| criterion.contributor == contributor)
                .cloned()
                .collect(),
        );
        if !mine.is_empty() {
            return mine;
        }
        once(
            model::criteria_of(model, contributor)
                .into_iter()
                .filter(|criterion| criterion.contributor == contributor)
                .collect(),
        )
    };
    // @lfy def/generation/main.lfy:request
    for &entity in &layered {
        out.extend(by(entity));
    }
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
        previous: _,
        answered,
        removed,
        existing: _,
        interfaces,
        guidance,
        native_dependencies,
        knowledge,
        commands,
    } = *quoted;
    let model = &workspace.model;
    let mut out = String::new();

    // The reader and its job, and the units by stem.
    // @lfy def/generation/main.lfy:request#request:request:571bc891f6509a3d1997b846f685a316e34804763d8271373888f530426558b3
    // @lfy def/generation/main.lfy:request#request:request:d650f0548150afb04ee79524e308a0150e9cf8a3f4dcc9ffde74e3d90e7fcada
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

    // Where the outputs go.
    // @lfy def/generation/main.lfy:request#request:request:800766e448bfb8ed73777735dcb6d1a81a50d15783302ad27486b92c2d669f5e
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
            // @lfy def/generation/main.lfy:request#request:request:28c71f256cbace38745c677d86962737a051927e499b97ac10dc63f2550643be
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
    write_existing(&mut out, workspace, plan, batch, quoted);

    // The guidance.
    // @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
    out.push_str("## Guidance\n\n");
    let _ = writeln!(
        out,
        "The declaration of the target `{}`, each of its layers in guidance order, and every trait \
         those layers extend give this guidance for building against the target:\n",
        target.identifier
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

    // The knowledge. @lfy def/generation/main.lfy:request
    write_knowledge(&mut out, workspace, knowledge);

    // Each interface.
    // @lfy def/generation/main.lfy:request#request:request:43df411fd74fc148bd737fdeb88ec86747b9ad9eb955912e272384f7def7a93f
    out.push_str("## Interfaces\n\n");
    if interfaces.is_empty() {
        out.push_str("The units of this batch depend on no unit outside it.\n\n");
    } else {
        // @lfy def/generation/main.lfy:request#request:request:7424b0fcb62916f23bfb6c1265f3a3f943fb1354558e0d4b0ea4722212bc0c7f
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

    // The commands.
    // @lfy def/generation/main.lfy:request#request:request:09b0190ee8969e01b9ff7a561a98c3663f87fb8b97fa83c588d0631e7989bc20
    write_commands(&mut out, target, commands);

    // One section per unit of the batch, in order.
    // @lfy def/generation/main.lfy:request#request:request:d86c17626a07f5e8ec807bd7740a5f6f8280e6af67444bf1bf5d18744b801d3b
    out.push_str("## The units to compile\n\n");
    for &index in &batch.units {
        let unit = &plan.units[index];
        let file = &workspace.files[unit.file];
        let lowered = &program.files[unit.lowered];
        let answered = answered.get(&file.path).map(Vec::as_slice);
        let _ = writeln!(out, "### `{}` (stem `{}`)\n", file.path, unit.stem);
        // A unit whose lowered code is unchanged has criteria or tests that are not: the
        // code is held to what each of them now says, which may be behavior it never had.
        // @lfy def/generation/main.lfy:request#request:request:dab229dc001954e941ede8f9cf711d6ff5830dd88e08a35f3bccea3da26ad09b
        if unit.reason == Some(Reason::Requirements) {
            out.push_str(
                "The lowered code of this unit is unchanged, but its criteria or tests differ. The \
                 code must do what each criterion and test now says, which may mean writing \
                 behavior it does not have yet.\n\n",
            );
        }
        // A criterion or test the existing outputs did not answer for may be a removed one
        // put in other words, and the code is held to the new wording either way.
        // @lfy def/generation/main.lfy:request#request:request:40e519bc14a840e00c6e0fb7bff68febae9b5b591f182632fddc0a29f6f47704
        if answered.is_some_and(|ids| {
            unit.requirements(program)
                .iter()
                .any(|requirement| !ids.iter().any(|id| id == requirement.id()))
        }) {
            out.push_str(
                "A criterion or test marked new may restate a removed one in other words. Read the \
                 code against what the new one says, never against the removed one.\n\n",
            );
        }
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
        write_moves(&mut out, workspace, plan, index);
        // Each entity's lowered code, and then, apart from the code, its local criteria and
        // tests, each on its own line with its id. @lfy def/generation/main.lfy:request
        for &entity in &unit.entities {
            write_entity(&mut out, model, program, unit.lowered, entity, answered);
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
            write_requirements(&mut out, &own, answered);
        }
        // What the outputs answered for and the unit no longer has, last in its section: an
        // id and a text are all that is left of a criterion or a test a unit has dropped.
        // @lfy def/generation/main.lfy:request#request:request:85fafd5b7c6201f807d532b93b487771bdd141c7ee6bd8854cdc7d981176ff9a
        if let Some(gone) = removed.get(&file.path) {
            out.push_str("#### Removed since its outputs were accepted\n\n");
            write_requirements(&mut out, gone, None);
        }
    }

    // Every global criterion and test, each once with its id.
    // @lfy def/generation/main.lfy:request#request:request:947030aa711f453c710b970e654a978a41f4db5a2776707bc669e1c0efd5bea8
    out.push_str("## Global criteria and tests\n\n");
    out.push_str(
        "These hold across the whole program. A unit answers for one only where it is relevant to \
         that unit's code, and ignores it otherwise; the global review checks each one once, \
         across every output.\n\n",
    );
    if globals.is_empty() {
        out.push_str("(none)\n\n");
    } else {
        write_requirements(&mut out, globals, None);
        out.push('\n');
    }

    // Criteria and tests are never restated in an output.
    // @lfy def/generation/main.lfy:request#request:request:88bad81c76d679b95e82ea566e65627b4a1615584da2fe8b645fd322f0b26fb3
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

    // Markers.
    // @lfy def/generation/main.lfy:request#request:request:671617e25d98e36f8c646b0c76d941034839245f2544e239cb4ede3581bb2f33
    // @lfy def/generation/main.lfy:request#request:request:44f9eb2a9dcf558687d93ade60ce6a62697e5ff6c388a512e83449be94e9a7fd
    out.push_str("## Markers\n\n");
    // The marker comment is spelled out, since the line comment a marker is written in must
    // begin with it and nothing else in the request says how this language begins one.
    let _ = write!(
        out,
        "Every emitted item and every test carries a marker naming the entity it comes from. A marker is \
         written as a line comment beginning with `{}`, then `@lfy`, a space, the source path \
         relative to the root, a colon, and the name of the entity: its identifier, or its owner's \
         identifier, a dot, and a member's name. Never write a line number. A region that answers \
         for a criterion or a test spells its id after the entity and a `#`. Write a criterion's id \
         only on code that does what that criterion says in every situation it names: a marker \
         never stands in for the code, and a marker on code that does something else is a false \
         claim about the program.\n\n",
        target.marker_comment
    );
    for &index in &batch.units {
        let unit = &plan.units[index];
        let file = &workspace.files[unit.file];
        if let Some(&entity) = unit.entities.first() {
            let _ = writeln!(
                out,
                "For example: `{} @lfy {}:{}`.",
                target.marker_comment,
                file.path,
                entity_name(model, entity)
            );
        }
    }
    out.push('\n');

    // Tests.
    // @lfy def/generation/main.lfy:request#request:request:6151dc423cf3f97336663532752b0e5607981574be689ec41445917c49163e8d
    out.push_str("## Tests\n\n");
    out.push_str(
        "Each test becomes one test, and each criterion that can be checked mechanically becomes one \
         test, placed where the guidance says. Each of them carries a marker naming the entity it \
         comes from and, after a `#`, the id of the criterion or test it answers for.\n\n",
    );

    // The tools of the agent server.
    // @lfy def/generation/main.lfy:request#request:request:45fec551e4527a98d2f44a2d0ad6d8d8a27da9f77f1255f53ede96415cfa364f
    out.push_str("## The agent server\n\n");
    out.push_str(
        "The tools of the agent server are called before files are searched or read by hand, because \
         they answer from the program and its source maps at once:\n\n\
         - `elfie_entity` for a definition with its criteria and tests.\n\
         - `elfie_find` for where a name is declared.\n\
         - `elfie_references` for where it is used.\n\
         - `elfie_outline` for the declarations of a file.\n\
         - `elfie_output` for the generated code of an entity, of a requirement id, or of an output file.\n\
         - `elfie_source` for where a line of generated code came from.\n\
         - `elfie_changes` for what changed in a unit since its outputs were accepted.\n\
         - `elfie_check` for whether acceptance would pass.\n\n",
    );

    // How to end the report.
    // @lfy def/generation/main.lfy:request#request:request:e4ff528873311ac2c054d3eb98c7015eca5064897eae15ff47429cfdd53b7860
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

/// What the outputs part says about outputs that exist already: which they are, that only
/// the difference between the previous lowered source and the current one and the criteria
/// and tests marked new or removed require a change, the previous lowered source of each
/// unit that has one, which units have none, and which units have no record of the ids their
/// outputs were generated against.
// @lfy def/generation/main.lfy:request#request:request:3d9b718229fc59a2a379fb37aab7b7235c06fee91fa5fc44b6285037204d9a68
fn write_existing(
    out: &mut String,
    workspace: &Workspace,
    plan: &Plan,
    batch: &Batch,
    quoted: &Quoted<'_>,
) {
    if quoted.existing.is_empty() {
        return;
    }
    out.push_str("These outputs exist already:\n");
    for path in quoted.existing.keys() {
        let _ = writeln!(out, "- `{path}`");
    }
    out.push_str(
        "\nChange only what the difference between the previous source and the current source, and \
         the criteria and tests marked new or removed, requires. Keep names and structure. Keep the \
         markers of unchanged items.\n",
    );
    let paths: Vec<&String> = batch
        .units
        .iter()
        .map(|&index| &workspace.files[plan.units[index].file].path)
        .collect();
    for path in &paths {
        if let Some(text) = quoted.previous.get(*path) {
            let _ = writeln!(
                out,
                "\nThe previous source of `{path}`, lowered as its current source is:\n\n\
                 ```elfie\n{}\n```",
                text.trim_end()
            );
        }
    }
    // A unit whose outputs record no source is reconciled with its source instead.
    // @lfy def/generation/main.lfy:request#request:request:3c1f4cba72285105fde5be93378fbda2d3d93aaf4293fbe643ab3a97d7d1b779
    let unknown: Vec<&&String> = paths
        .iter()
        .filter(|path| !quoted.previous.contains_key(**path))
        .collect();
    if !unknown.is_empty() {
        out.push_str(
            "\nThe source these outputs were generated from is not known. Reconcile the existing \
             outputs of each of them with its source:\n",
        );
        for path in unknown {
            let _ = writeln!(out, "- `{path}`");
        }
    }
    // A unit whose outputs record no ids leaves nothing to say which of its criteria and
    // tests are new, so the existing code is read against every one of them.
    // @lfy def/generation/main.lfy:request#request:request:a960df7e08bfeb90c2fb135fc71e6e80e01a69f9f351d3d8c7eab08f069407c1
    let unanswered: Vec<&&String> = paths
        .iter()
        .filter(|path| !quoted.answered.contains_key(**path))
        .collect();
    if !unanswered.is_empty() {
        out.push_str(
            "\nWhich criteria and tests of these units are new is not known. Read the existing code \
             against each of them before you write its marker:\n",
        );
        for path in unanswered {
            let _ = writeln!(out, "- `{path}`");
        }
    }
    out.push('\n');
}

/// What the compiler is given to read, in the order the request gathered it: a reference
/// or an example either quoted in full or named to be read; a definition as the files its
/// use path resolves to; a tool as the tool of the agent server to call.
// @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
fn write_knowledge(out: &mut String, workspace: &Workspace, knowledge: &[Knowledge]) {
    out.push_str("## Knowledge\n\n");
    if knowledge.is_empty() {
        out.push_str("The compiler is given nothing of its own to read.\n\n");
        return;
    }
    out.push_str("The compiler is given this to read, because it may not already know it:\n\n");
    let mut quoted: Vec<(String, String, String)> = Vec::new();
    for item in knowledge {
        match item.kind {
            // @lfy def/generation/main.lfy:request#request:request:18fa49f6ae0fe059f4d8b65429f7356e822363b0bb901d611f266ca1393c940b
            KnowledgeKind::Tool => {
                let _ = writeln!(
                    out,
                    "- {}: call the tool `{}` of the agent server before relying on what it \
                     answers.",
                    item.topic, item.source
                );
            }
            // @lfy def/generation/main.lfy:request#request:request:179394ea2be57b4a87b41dac2988b30f36b058129a701fc79ff34cc290769606
            KnowledgeKind::Definition => {
                let files = definition_files(workspace, item);
                let named = if files.is_empty() {
                    format!("`{}`", item.source)
                } else {
                    files
                        .iter()
                        .map(|path| format!("`{path}`"))
                        .collect::<Vec<String>>()
                        .join(", ")
                };
                let _ = writeln!(out, "- {}: read {named} as Elfie.", item.topic);
            }
            KnowledgeKind::Reference | KnowledgeKind::Example => {
                let path = under_root(&knowledge_root(workspace, item), &item.source);
                if item.quote == Some(true) {
                    // @lfy def/generation/main.lfy:request#request:request:f64fa963c69a9e30aa2c190c595acdbb6221274d050a5a8533b9cf9291684155
                    let text = std::fs::read_to_string(workspace.root.join(&path))
                        .unwrap_or_else(|error| format!("(`{path}` could not be read: {error})"));
                    quoted.push((item.topic.clone(), path, text));
                } else {
                    // @lfy def/generation/main.lfy:request#request:request:737e4198f2f6bddc790512ae099b13648ed59405ba19506fb0abff20748ec530
                    let _ = writeln!(
                        out,
                        "- {}: read `{path}` before writing anything it covers.",
                        item.topic
                    );
                }
            }
        }
    }
    out.push('\n');
    for (topic, path, text) in quoted {
        let _ = writeln!(
            out,
            "### {topic}\n\nFrom `{path}`:\n\n````\n{}\n````\n",
            text.trim_end()
        );
    }
}

/// The directory a knowledge item's source is relative to: the root of the package of the
/// file its contributor is declared in, and the project's own root when that file is the
/// project's own.
// @lfy def/generation/main.lfy:request
fn knowledge_root(workspace: &Workspace, item: &Knowledge) -> String {
    workspace.model.entities[item.contributor]
        .file
        .and_then(|source| workspace.files.iter().find(|file| file.source == source))
        .and_then(|file| file.package)
        .map_or(String::new(), |package| {
            workspace.packages[package].root.clone()
        })
}

/// The files of the program the source of a knowledge item of the kind `definition`
/// resolves to: it is a use path, read from the file its contributor is declared in.
// @lfy def/generation/main.lfy:request
fn definition_files(workspace: &Workspace, item: &Knowledge) -> Vec<String> {
    let from = workspace.model.entities[item.contributor]
        .file
        .and_then(|source| workspace.files.iter().find(|file| file.source == source));
    let base = if item.source.starts_with('.') {
        let directory = from.map_or("", |file| directory_of(&file.path));
        normalized(&under_root(directory, &item.source))
    } else {
        let (first, rest) = item
            .source
            .split_once('/')
            .unwrap_or((item.source.as_str(), ""));
        match workspace
            .packages
            .iter()
            .find(|package| package.identifier == first)
        {
            Some(package) => normalized(&under_root(&package.root, rest)),
            None => normalized(&item.source),
        }
    };
    let named = format!("{base}{EXTENSION}");
    let main = under_root(&base, &format!("main{EXTENSION}"));
    workspace
        .files
        .iter()
        .filter(|file| file.path == named || file.path == main)
        .map(|file| file.path.clone())
        .collect()
}

/// The commands the compiler runs instead of knowing the tools, one line per operation.
// @lfy def/generation/main.lfy:request
fn write_commands(out: &mut String, target: &Target, commands: &[Command]) {
    out.push_str("## Commands\n\n");
    let _ = writeln!(
        out,
        "These run with `sh -c` in `{}`, with `RUN_SCRIPT` set to {} and, for `add`, `NAME` and \
         `VERSION` set to the dependency's:\n",
        target.output_directory,
        match &target.script_runner {
            Some(runner) => format!("`{runner}`"),
            None => "nothing, since the target names none".to_string(),
        }
    );
    if commands.is_empty() {
        out.push_str("(none)\n");
    }
    for command in commands {
        let _ = writeln!(
            out,
            "- `{}`: {}",
            operation_name(command.operation),
            match &command.line {
                Some(line) => format!("`{line}`"),
                None => "not run".to_string(),
            }
        );
    }
    out.push_str(
        "\nA dependency is added only by running the add command, never by editing a manifest or a \
         lockfile by hand. The build, test, and lint commands are run before the report, and their \
         results stated in it.\n\n",
    );
}

/// The name of an operation, as the standard library spells it.
// @lfy def/generation/main.lfy:request
fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Install => "install",
        Operation::Add => "add",
        Operation::Build => "build",
        Operation::Test => "test",
        Operation::Lint => "lint",
        Operation::Format => "format",
        Operation::Run => "run",
    }
}

/// What moved into and out of one unit, in that unit's section of the instructions: which
/// entities, from or into which unit, and because of which cause.
// @lfy def/generation/main.lfy:request
fn write_moves(out: &mut String, workspace: &Workspace, plan: &Plan, index: usize) {
    let model = &workspace.model;
    let spelled = |entities: &[EntityId]| -> String {
        entities
            .iter()
            .map(|&entity| format!("`{}`", entity_name(model, entity)))
            .collect::<Vec<String>>()
            .join(", ")
    };
    // @lfy def/generation/main.lfy:request
    let into: Vec<&Move> = plan
        .moves
        .iter()
        .filter(|moved| moved.into == index)
        .collect();
    if !into.is_empty() {
        // @lfy def/generation/main.lfy:request#request:request:2a957e8526f8fadbdb509afff57d5ee421e83c1bf0bc08f037202c40650af324
        // @lfy def/generation/main.lfy:request#request:request:69a7dba9df720e986cae645ffafd7885bc8c7022024d385145cf81e43867eea5
        out.push_str(
            "These entities are compiled in this unit although another file declares them, because \
             the two files would otherwise reach each other. Write them into this unit's outputs \
             with markers naming the file that declares them. Wherever this unit's code needs only \
             them, a `use` of that file is satisfied by them here and never by importing that \
             file.\n",
        );
        // @lfy def/generation/main.lfy:request#request:request:78a8534645507634a61935dcee1699d73fdc5735bcadda06eb24bf9f9854ea34
        for moved in into {
            let origin = &plan.units[moved.origin];
            let _ = writeln!(
                out,
                "- {} from `{}`, because of `{}`.",
                spelled(&moved.entities),
                workspace.files[origin.file].path,
                moved.cause
            );
        }
        out.push('\n');
    }
    // @lfy def/generation/main.lfy:request
    let outward: Vec<&Move> = plan
        .moves
        .iter()
        .filter(|moved| moved.origin == index)
        .collect();
    if !outward.is_empty() {
        // @lfy def/generation/main.lfy:request#request:request:1672e75b3730e67adb65f5507e3af45281e70066521a13fafb939415a73393da
        // @lfy def/generation/main.lfy:request#request:request:ab56007e0a1646c5e4d399cc67e5ac36f77f95f1ed5af06ced3edda5f7506ec4
        out.push_str(
            "These entities this file declares are compiled in another unit, because the two files \
             would otherwise reach each other. Import them from that unit's outputs and never write \
             them again, and make each of them available from this unit's outputs under the name \
             and path it would have had, so that code naming it here still finds it.\n",
        );
        // @lfy def/generation/main.lfy:request#request:request:d8077ccebe94afe4cb2e7e3cf3daafbef0270bf0ca0b6141d42e3be46d002c52
        for moved in outward {
            let into = &plan.units[moved.into];
            let _ = writeln!(
                out,
                "- {} in the unit `{}` (`{}`).",
                spelled(&moved.entities),
                into.stem,
                workspace.files[into.file].path
            );
        }
        out.push('\n');
    }
}

/// One entity of a unit: its name, its kind, its definition, its type, its lowered code,
/// and then, apart from the code, its local criteria and tests, each on its own line with
/// its id.
// @lfy def/generation/main.lfy:request
fn write_entity(
    out: &mut String,
    model: &Model,
    program: &Program,
    lowered: usize,
    entity: EntityId,
    answered: Option<&[String]>,
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
    if let Some(ty) = &record.ty {
        let _ = writeln!(out, "Type: `{}`\n", model::type_text(model, ty));
    }
    // An entity moved into the unit is declared in the file it came from, so its code is
    // looked for across the program when the unit's own lowered file does not hold it.
    // @lfy def/generation/main.lfy:request
    if let Some(node) = lowered_node_of(&program.files[lowered], entity)
        .or_else(|| lowered_declaration(program, entity))
    {
        out.push_str("Lowered code:\n\n```elfie\n");
        out.push_str(lowered_code(model, node).trim());
        out.push_str("\n```\n\n");
    }
    write_requirements(
        out,
        &declared_requirements(program, lowered, entity),
        answered,
    );
}

/// Every criterion and test of a list, criteria then tests, each on one line beginning with
/// its id: what a request and a review quote apart from the code.
///
/// Where `answered` holds the ids the existing outputs were generated against, a line whose
/// id is none of them is marked new, so that a compiler reading existing code knows which of
/// them that code was never held to.
// @lfy def/generation/main.lfy:request#request:request:72801863f22ee154bf0e22299d1405c97cd6309fc630197aa315508e2f4829f7
// @lfy def/generation/main.lfy:request#request:request:20c8b9f187ae61ac935c6b929921f420eca7b18fd429d76213ce5975b73af64f
fn write_requirements(out: &mut String, requirements: &[Requirement], answered: Option<&[String]>) {
    let mark = |id: &str| -> &'static str {
        match answered {
            Some(ids) if !ids.iter().any(|known| known == id) => " (new)",
            _ => "",
        }
    };
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
            "- `{}` — {}{}",
            criterion.id,
            criterion.text(),
            mark(&criterion.id)
        );
    }
    out.push('\n');
    out.push_str("Tests:\n");
    if tests.is_empty() {
        out.push_str("(none)\n");
    }
    for test in tests {
        let _ = writeln!(
            out,
            "- `{}` — {}{}",
            test.id,
            test_text(test),
            mark(&test.id)
        );
    }
    out.push('\n');
}

/// One criterion of the guidance as a list item: `When` and its situations, then its
/// behaviors, then its side effects.
///
/// A criterion of the guidance is a criterion as written, never lowered, so it has no
/// [`LoweredCriterion::text`] to be shown by; it is spelled here exactly as that spells a
/// lowered one, so that every criterion a request quotes reads the same way.
// @lfy def/generation/main.lfy:request
fn write_criterion(out: &mut String, criterion: &Criterion) {
    let mut line = String::new();
    if let Some(situation) = &criterion.situation {
        line.push_str("When ");
        line.push_str(&situation.join(", and "));
        line.push_str(": ");
    }
    if let Some(behavior) = &criterion.behavior {
        line.push_str(&behavior.join("; "));
    }
    if let Some(side_effects) = &criterion.side_effects {
        line.push_str(". Side effects: ");
        line.push_str(&side_effects.join("; "));
    }
    let _ = writeln!(out, "- {line}");
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
/// Rejected when there are no outputs, when an output shared with another unit of the
/// target — one a recorded map of that unit names — has lost a region that map records,
/// when an output's path is not under the target's
/// output directory or its extension is not among the target's, when a marker names an
/// entity that its file does not declare, a line
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
///
/// `recorded` is the source maps of the other units of the unit's target, which say which
/// regions of a shared output belong to another unit.
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
    recorded: &[SourceMap],
) -> Verdict {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let planned = &plan.units[unit];
    let target = &workspace.targets[planned.target];
    let file = &workspace.files[planned.file];
    let mut problems: Vec<String> = Vec::new();

    if outputs.is_empty() {
        // @lfy def/generation/main.lfy:accept#accept:accept:4a48dc9bd184c822a38d627cf303e55c050d877bac33bf8d5f8794db459af17d
        problems.push(format!("no output was produced for {}", file.path));
    }

    let named = named_entities(model, file.source, &planned.entities);
    // Every requirement id a marker naming this unit's own file may spell: a local criterion
    // or test of this unit, or a global one of the request. An output is often shared with
    // another unit, which writes its own markers and its own ids into it, so only a marker
    // naming this unit's file is a claim this unit makes and only its id is checked here.
    // @lfy def/generation/main.lfy:accept
    let global_ids: Vec<&str> = request.globals.iter().map(Requirement::id).collect();
    let local_ids: Vec<String> = planned
        .requirements(program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();
    // The files whose markers are this unit's claims: its own, and the file of each entity
    // moved into it, since a moved entity keeps the marker of the file that declares it.
    // @lfy def/generation/main.lfy:accept
    let mut claimed: Vec<&str> = vec![&file.path];
    for moved in plan.moves.iter().filter(|moved| moved.into == unit) {
        let path = &workspace.files[plan.units[moved.origin].file].path;
        if !claimed.contains(&path.as_str()) {
            claimed.push(path);
        }
    }

    // Each marker as the compiler wrote it, beside the one the model resolved it to: the
    // first says where the marker sits in the text, the second is what is recorded.
    // @lfy def/generation/main.lfy:accept
    let mut normalized: Vec<Vec<(Marker, Marker)>> = Vec::new();
    for output in outputs {
        if !under(&output.path, &target.output_directory) {
            // @lfy def/generation/main.lfy:accept#accept:accept:324f6bd2c925ffa32b5feee5d17d3195c149deb99f00247918a0ee6d58d1b870
            problems.push(format!(
                "the output {} is not under the output directory {} of the target {}",
                output.path, target.output_directory, target.identifier
            ));
        }
        // @lfy def/generation/main.lfy:accept#accept:accept:e25297fa468e4f2b18f120993ee14b756d7715661e928e3e161f9aba05aad304
        if !extension_of(&output.path).is_some_and(|extension| {
            target
                .extensions
                .iter()
                .any(|allowed| allowed == extension)
        }) {
            problems.push(format!(
                "the output {} has no extension the target {} allows: {}",
                output.path,
                target.identifier,
                target.extensions.join(", ")
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
            // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
            if let Some(requirement) = &marker.requirement
                && claimed.contains(&marker.file.as_str())
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
                &claimed,
                output,
                &mut problems,
            ) {
                markers.push((marker, resolved));
            }
        }
        normalized.push(markers);
    }

    // An output is shared when a recorded map of another unit of the target names it: one
    // output often holds the code of several definition files, as a crate's root must. What
    // a unit may not do is take away what another unit recorded there, so every region of
    // that map which is not this unit's own must still be in the output — compared by the
    // file, the entity, and the requirement it names rather than by line, since every line
    // moves when the code above it changes. A region naming a file this unit claims is this
    // unit's to write or to drop, and a co-owner's map holds those only because a map records
    // every marker of its output, not because that unit put them there. A co-owner compiled
    // in the same batch is writing its own regions again in this very run, so its map says
    // nothing about what this output must still hold.
    // @lfy def/generation/main.lfy:accept#accept:accept:a61aa4a9283df62d2e6f07fca7d7d134df081b931e93c5cd80d35866633303f7
    // @lfy def/generation/main.lfy:accept#accept:accept:e1803d0137825c4ab13fa3cc1b4bb2fbcc98d2304ae4776fa3a1b7c4ec27453f
    let batched: Vec<&str> = request
        .batch
        .units
        .iter()
        .map(|&batched| workspace.files[plan.units[batched].file].path.as_str())
        .collect();
    for (output, markers) in outputs.iter().zip(&normalized) {
        for map in recorded {
            if map.output != output.path
                || map.source == file.path
                || batched.contains(&map.source.as_str())
            {
                continue;
            }
            for marker in &map.markers {
                if claimed.contains(&marker.file.as_str())
                    || markers.iter().any(|(_, kept)| same_region(kept, marker))
                {
                    continue;
                }
                problems.push(format!(
                    "the output {} is shared with {}, whose region {} is not in it",
                    output.path,
                    map.source,
                    region_of(marker)
                ));
            }
        }
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:2a90cbf7112a647235b9f1c81167c5d44301cbc8369b5d4cc2973be6102fea26
    for &entity in &planned.entities {
        let Some((first, last)) = declaration_lines(model, entity) else {
            continue;
        };
        // An entity moved into the unit is marked with the file that declares it, not with
        // the unit's own file. @lfy def/generation/main.lfy:accept
        let declared = declaring_path(model, entity).unwrap_or(&file.path);
        let covered = normalized
            .iter()
            .flatten()
            .any(|(_, marker)| marker.file == *declared && (first..=last).contains(&marker.line));
        if !covered {
            problems.push(format!(
                "the entity {} of {declared} (lines {first} to {last}) has no marker in any output",
                entity_name(model, entity),
            ));
        }
    }

    if !problems.is_empty() {
        // @lfy def/generation/main.lfy:accept#accept:accept:03e40c9f3bc4db3e262e3c2c66d09c2f2d3d0833ad56ce201f1fafc09fa47f10
        return Verdict {
            accepted: false,
            problems,
            source_maps: Vec::new(),
            outputs: Vec::new(),
        };
    }

    // Nothing of a source map comes from the compiler's text except the names it spelled:
    // every hash, every signature, and the time are taken here.
    // @lfy def/generation/main.lfy:accept#accept:accept:1f423d0a0d06e5fc1e0ddf109f2e402b9a84537a80a91e6e4e459fb24ac288dc
    // @lfy def/generation/main.lfy:accept#accept:accept:90bf10cc4db0c256646cd0f1ba8c45bda247489103c46e1ce26289709829903a
    let hash = source_hash(&planned.text);
    // @lfy def/generation/main.lfy:accept#accept:accept:666051c035bda35cafa77995ca9f05915cb5d6f3d9dc74b172f9187d403c1a15
    let requirements = requirements_hash(program, planned);
    // The ids the hash is of, recorded beside it so a later run can say which of them the
    // unit no longer has.
    // @lfy def/generation/main.lfy:accept#accept:accept:0bcfc0b4637d084c09be1b3c407696d0b16f9c7f082c7f0d91de9ea2950d2902
    let answered = answered_ids(program, planned);
    // @lfy def/generation/main.lfy:accept#accept:accept:4a6c75702e77d3fab75aa1d127f35eaad993c997ed4cd0dd14f01162c019bc21
    let signature = interface_signature(workspace, planned);
    // @lfy def/generation/main.lfy:accept#accept:accept:d9879de703d6314164058495241caf82738d58c8f39595b07a19ace78cdbd758
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
    // @lfy def/generation/main.lfy:accept#accept:accept:95161566ef61156ea6f857f212fb510a3edf6221a4238a87b45595f60b6466bb
    let generated = now_rfc3339();
    let mut source_maps = Vec::new();
    let mut written = Vec::new();
    // @lfy def/generation/main.lfy:accept#accept:accept:30a65355d64f16689cede881188462ef9f94082f48857623e59af57399ad7503
    for (output, markers) in outputs.iter().zip(&normalized) {
        source_maps.push(SourceMap {
            // @lfy def/generation/main.lfy:accept#accept:accept:8d2a3aa698445970d859538af521c42b878c576deae6eac97af3a97c5c7fd25f
            target: target.identifier.clone(),
            // @lfy def/generation/main.lfy:accept#accept:accept:ea45d8dd2770ae8698006fbdd6aa54ebb0af32fabeb5d9edf8a1a432812d7d7c
            output: output.path.clone(),
            // @lfy def/generation/main.lfy:accept#accept:accept:b27edcf9cd5b896cb993b81cba1f8faa47d79995f135c37e63d5e9ab648eed77
            source: file.path.clone(),
            hash: hash.clone(),
            requirements: requirements.clone(),
            answered: answered.clone(),
            signature: signature.clone(),
            dependencies: dependencies.clone(),
            generated: generated.clone(),
            // @lfy def/generation/main.lfy:accept#accept:accept:e4b7f22f09d4b3bbbc78c4edf8763f1f23ff76f5f4e5c858304e8315e8fe30aa
            markers: markers.iter().map(|(_, marker)| marker.clone()).collect(),
        });
        written.push(rewrite_markers(output, markers));
    }
    // Nothing here runs a command of the target: whether an output builds or its tests pass
    // is the caller's to run, as the guidance says.
    // @lfy def/generation/main.lfy:accept#accept:accept:9bde11d25fcf12e8c1a113f8be1c479d68185569c8170c39b3348772bf613541
    // @lfy def/generation/main.lfy:accept#accept:accept:03e40c9f3bc4db3e262e3c2c66d09c2f2d3d0833ad56ce201f1fafc09fa47f10
    Verdict {
        accepted: true,
        problems: Vec::new(),
        source_maps,
        outputs: written,
    }
}

/// Whether two markers name the same region: the same file, the same entity, and the same
/// requirement. Lines are left out on purpose — a region keeps its identity when the code
/// above it grows — and so is the output line, which is where the region sits rather than
/// what it is.
// @lfy def/generation/main.lfy:accept
fn same_region(one: &Marker, other: &Marker) -> bool {
    one.file == other.file && one.entity == other.entity && one.requirement == other.requirement
}

/// A region named the way a marker spells it, for a problem to quote: the file, the entity
/// or the line, and the requirement when it answers for one.
// @lfy def/generation/main.lfy:accept
fn region_of(marker: &Marker) -> String {
    let place = match &marker.entity {
        Some(entity) => entity.clone(),
        None => marker.line.to_string(),
    };
    match &marker.requirement {
        Some(requirement) => format!("{}:{place}#{requirement}", marker.file),
        None => format!("{}:{place}", marker.file),
    }
}

/// One marker with its line derived from the model and, where it named a line inside a
/// declaration, the name of that entity; problems are pushed for a marker that resolves
/// nowhere, and `None` is given only for one that names a file outside the program, which
/// no source map records. A marker naming a line of a file of the program that no
/// declaration covers is given as written, so that the map records it too.
///
/// A number the compiler wrote is only where to look: whatever the marker was spelled as,
/// the line kept is the first line of the declaration it resolved to.
// @lfy def/generation/main.lfy:accept
fn resolve_marker(
    workspace: &Workspace,
    file: &File,
    named: &[(String, EntityId)],
    marker: &Marker,
    claimed: &[&str],
    output: &Output,
    problems: &mut Vec<String>,
) -> Option<Marker> {
    let model = &workspace.model;
    // Decision: an output may carry markers for other files of the program (one Rust file
    // often mirrors several definition files). @lfy def/generation/main.lfy:accept
    let Some(declaring) = workspace.file(&marker.file) else {
        // A marker naming a file outside the program is a fixture or prose, not a claim
        // about the program, so it is ignored and not recorded.
        // @lfy def/generation/main.lfy:accept#accept:accept:f4b16e912ad6060ea8d1d2f85da81f81022ff64e04b9b347f00579d497e86481
        return None;
    };
    // The entities the marker's file declares: the unit's own table for its own file, since
    // that one holds the entities the unit builds as well.
    let own = named_entities(model, declaring.source, &[]);
    let table = if marker.file == file.path {
        named
    } else {
        &own
    };
    // A marker that names an entity keeps its spelling; its line is derived.
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:76e237fb260caf5b11bc6d84be162349e39c76c149a14baf79eb7d16efa750a6
    if let Some(name) = &marker.entity {
        match table.iter().find(|(candidate, _)| candidate == name) {
            // @lfy def/generation/main.lfy:accept#accept:accept:3464ba36f8e1e5d762286978e6be41278e751aded80a6bf152a33a3c4c2cb3a4
            // @lfy def/generation/main.lfy:accept#accept:accept:4d6cc733ae9a9a4d4abc942dff1d089398f749e30a393fd194edafe27c85cf95
            Some(&(_, entity)) => {
                return Some(Marker {
                    line: declaration_line(model, entity).unwrap_or(1),
                    ..marker.clone()
                });
            }
            None => {
                // @lfy def/generation/main.lfy:accept#accept:accept:c9819875941e1d859a6baa6c2043bfaaebeee7923b85b2e2edbd4b88ec00c6f3
                problems.push(format!(
                    "{}:{}: the marker names {}, which {} does not declare",
                    output.path, marker.output_line, name, marker.file
                ));
                return Some(marker.clone());
            }
        }
    }
    // Decision: a marker names a line of the source file, so the last line is counted from
    // that file's own text; the request carries the lowered text, whose lines are not its.
    // The files whose lines this unit answers for are the ones it claims: its own, and the
    // file of each entity moved into it, which keeps the marker of the file declaring it.
    // @lfy def/generation/main.lfy:accept#accept:accept:032dc250d174c0232dd914cac293ffeb3f0494c8065247e29aaa7d0ff849dced
    if claimed.contains(&marker.file.as_str()) {
        let last_line = source_text(workspace, declaring).lines().count().max(1);
        if marker.line == 0 || marker.line > last_line {
            problems.push(format!(
                "{}:{}: the marker names line {} of {}, whose last line is {last_line}",
                output.path, marker.output_line, marker.line, marker.file
            ));
            return Some(marker.clone());
        }
    }
    // A marker that names a line names the innermost declaration covering it, and the line
    // kept is that declaration's first, from the model: the number the compiler wrote said
    // where to look and is not itself recorded. A line no declaration covers names no
    // entity, so the marker stands as written, with the line it spells, which is what a
    // line marker's line is; it names a file of the program and is recorded like any other.
    // @lfy def/generation/main.lfy:accept#accept:accept:f0f63ff3da1a2ceca7ecc1a89adc26e6a86649f0beed8e94ccc5726914eb4c4b
    // @lfy def/generation/main.lfy:accept#accept:accept:e4b7f22f09d4b3bbbc78c4edf8763f1f23ff76f5f4e5c858304e8315e8fe30aa
    let Some((name, entity)) = innermost(model, table, marker.line) else {
        return Some(marker.clone());
    };
    Some(Marker {
        entity: Some(name),
        column: None,
        line: declaration_line(model, entity).unwrap_or(marker.line),
        ..marker.clone()
    })
}

/// The innermost entity whose declaration covers a line, with the name it is named by.
// @lfy def/generation/main.lfy:accept
fn innermost(
    model: &Model,
    named: &[(String, EntityId)],
    line: usize,
) -> Option<(String, EntityId)> {
    let mut best: Option<(usize, &str, EntityId)> = None;
    for (name, entity) in named {
        let Some((first, last)) = declaration_lines(model, *entity) else {
            continue;
        };
        if !(first..=last).contains(&line) {
            continue;
        }
        let width = last - first;
        if best.is_none_or(|(widest, _, _)| width <= widest) {
            best = Some((width, name, *entity));
        }
    }
    best.map(|(_, name, entity)| (name.to_string(), entity))
}

/// Every entity of a file a marker may name, with the name it is named by: each
/// declaration of the file scope and each entity the unit builds, then the members each
/// of them declares as `owner.member`.
// @lfy def/generation/main.lfy:accept
fn named_entities(model: &Model, source: FileId, extra: &[EntityId]) -> Vec<(String, EntityId)> {
    let mut declarations: Vec<EntityId> = Vec::new();
    if let Some(&scope) = model.file_scopes.get(source) {
        for symbol in model.scopes[scope].declared() {
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
    // An entity moved into the unit is declared in another file, and is named by a marker of
    // that file; only what this file declares belongs to its table.
    // @lfy def/generation/main.lfy:accept
    for &entity in extra {
        let record = &model.entities[entity];
        if record.node.is_some()
            && record.file == Some(source)
            && !declarations.contains(&entity)
        {
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

/// The path of the file that declares an entity; `None` for an entity with no node.
// @lfy def/generation/main.lfy:accept
fn declaring_path(model: &Model, entity: EntityId) -> Option<&str> {
    let node = model.entities[entity].node?;
    Some(&model.sources.get(node.file)?.path)
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
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:1dc22603643b17fad4874110ef09a3391b67d71c28a9f5b35a7dfa18438f3409
pub fn source_maps_of(workspace: &Workspace, target: Option<&str>) -> Vec<SourceMap> {
    let mut out: Vec<SourceMap> = Vec::new();
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:e3ab53d40653267f2e160d4a7f3251350dc8120cab5217a982112c74085d6a81
    for named in &workspace.targets {
        // A target other than the one asked for is skipped before its folder is opened, so
        // neither its order nor a map file of its own that cannot be read changes the result.
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:3fa1873c256699bcc68b376b528e4f35f840fe93aad209cc0f992a5dee017fce
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:4790ed9f6184ca4bd2972e5ec7a734acb1266c6d095e7edebd27bf8dd48cf71b
        if target.is_some_and(|target| named.identifier != target) {
            continue;
        }
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:48e0f71f7db0725ebe634eeac3dac1d913b250a6626d34b72282e59198837589
        let folder = workspace.root.join(MAPS).join(&named.identifier);
        let files = if folder.is_dir() {
            map_files(&folder) // @lfy def/generation/main.lfy:sourceMapsOf
        } else {
            // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:c968c8acaf4d38cdd8430f58ae5b09a27d1e2e6bd4ee8861bd63a620ddfd7a9c
            let legacy = workspace
                .root
                .join(&named.output_directory)
                .join(LEGACY_MAPS);
            if legacy.is_file() {
                vec![legacy]
            } else {
                // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:55c87b019cd4d2d7c4a69761ea81c9bd9980381c22407c0e202211f03cfe8dc8
                Vec::new()
            }
        };
        for file in files {
            // A file that cannot be read, or that is no JSON list of source maps,
            // contributes nothing; one that is read gives its maps in its own order, and a
            // map that records no answered ids is read with none.
            // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:ceffdad2c9ea38bce966887b7756582eba5aca5852cb5980c55faa063210bd93
            // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:00a062bda31894d720907cb0ec23e4bc5dd3e4d0db76200eab3b747b849ca38d
            // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:9a20ac7072bf79bbb2d38d819ba4fb8127d258b6f0baf9134dafc7c98398e94a
            for map in read_source_maps(&file) {
                // A map whose output is gone says nothing about a unit that is up to date.
                // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:777506c6ffecf859d5436778cd8e1997eef47cd57e5f8f41754058bbe3117d23
                if workspace.root.join(&map.output).exists() {
                    out.push(map);
                }
            }
        }
    }
    out
}

/// Every `.json` file under a target's folder of map files, in the ascending text order of
/// their paths relative to that folder, the segments separated by `/`.
// Decision: a unit's stem holds the directories it sits in, so its map file does too and
// the folder is walked whole; the order is that of the whole relative paths as text, the
// order the loader lists a directory of source files in, so a map file beside a directory
// of the same name sorts where its own path falls rather than after the directory's files.
// @lfy def/generation/main.lfy:sourceMapsOf
fn map_files(folder: &Path) -> Vec<PathBuf> {
    let mut relative: Vec<String> = Vec::new();
    collect_map_files(folder, "", &mut relative);
    relative.sort();
    relative.iter().map(|path| folder.join(path)).collect()
}

/// Every `.json` file at or under one directory, as a path relative to the folder the walk
/// began in with `/` between the segments, in whatever order the directories give them.
// @lfy def/generation/main.lfy:sourceMapsOf
fn collect_map_files(folder: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{prefix}{name}");
        let path = entry.path();
        if path.is_dir() {
            collect_map_files(&path, &format!("{relative}/"), out);
        } else if name.ends_with(".json") {
            out.push(relative);
        }
    }
}

/// The map file of one unit: `elfie-compile/maps`, the target's identifier, and the unit's
/// stem with the extension `.json`, under the workspace root.
// @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:48e0f71f7db0725ebe634eeac3dac1d913b250a6626d34b72282e59198837589
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
// Only the unit's own map file is written, so recording one unit leaves every other byte
// for byte as it was.
// @lfy def/generation/main.lfy:record#record:record:d7ed944c0449f01835fd94e7de01e58066347177591b374f6a7beee48113f403
// @lfy def/generation/main.lfy:record#record:record:8d039cf20453fa3420e6acdb3ce42b2031909a9bc211b646a683be5017efa7fd
pub fn record(workspace: &Workspace, unit: &Unit, source_maps: &[SourceMap]) -> bool {
    let path = map_file(workspace, unit);
    if source_maps.is_empty() {
        // @lfy def/generation/main.lfy:record#record:record:aa6df37cdc9d7a159ba508148db794c3edbc568247899ccbd55574df85a1177d
        return match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
    }
    // @lfy def/generation/main.lfy:record#record:record:5b5e70df30f90156b3023da573235ca899c7a55801fbecd76167819cf57e21ad
    let mut ordered: Vec<SourceMap> = source_maps.to_vec();
    ordered.sort_by(|left, right| left.output.cmp(&right.output));
    // The readable layout of the maps, with the keys of every object in alphabetical order,
    // so the same maps always give the same bytes and a diff shows only what changed.
    // @lfy def/generation/main.lfy:record#record:record:1a8586a7f2168dcbbbc34c4373d1f944d6e0313f9b0840a53aa8f54e3e3208d6
    // @lfy def/generation/main.lfy:record#record:record:60d0cd914ebe3c1cbbd582cd3229d8adc3a00b879720f1caeec671fb4c707551
    // @lfy def/generation/main.lfy:record#record:record:5c0c63725b35e7fab925c81abc27914b55863257ede7a2863a3bce15e5b03ff5
    let value = serde_json::Value::Array(ordered.iter().map(SourceMap::to_json).collect());
    let Ok(mut text) = serde_json::to_string_pretty(&value) else {
        return false;
    };
    text.push('\n');
    // Maps that differ only in when they were generated are the maps the file holds.
    // @lfy def/generation/main.lfy:record#record:record:acccd867dc43a326a46b14f88ac19bb24373ec1a90795894fa96f075e1fcbcd9
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
    // @lfy def/generation/main.lfy:record#record:record:1ec300497e4609fbc621214de54b44c8b7f666382b4cf02496efdc55ed4cca99
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    // @lfy def/generation/main.lfy:record#record:record:1f4671d2ae5027d4e5bcfaf56dc68c765e27e31a92fc783b7f64a2165f391dd4
    std::fs::write(&path, text).is_ok()
}

// ---------------------------------------------------------------------------------------
// reviewsOf, recordReviews
// ---------------------------------------------------------------------------------------

/// The review record file of one unit: `elfie-compile/reviews`, the target's identifier, and
/// the unit's stem with the extension `.json`, under the workspace root, such as
/// `elfie-compile/reviews/rust/cli/main.json` for `def/cli/main.lfy`.
// @lfy def/generation/data.lfy:ReviewRecord#ReviewRecord:ReviewRecord:d1283130451aac07da56acfa5179b7af277f4cbbb6d93ed00c6b0aad6499db7a
pub fn review_file(workspace: &Workspace, unit: &Unit) -> PathBuf {
    workspace
        .root
        .join(REVIEWS)
        .join(&workspace.targets[unit.target].identifier)
        .join(format!("{}.json", unit.stem))
}

/// The review records of one unit, as its review record file holds them.
///
/// The file holds them as one JSON list, and they come back in the order it lists them. A
/// file that is missing, that cannot be read, or that is no JSON list of review records
/// gives none, so every criterion and test of the unit is reviewed again. A record whose id
/// is the id of no local criterion or test of the unit is left out, so a criterion that was
/// reworded is never carried on the verdict its old id earned. Nothing is written, and
/// nothing under `elfie-compile/cache` is read.
// Decision: the definition passes the workspace; the ids of a unit's local criteria and
// tests are the lowered program's, which a plan here does not own, so the program stands in
// for the workspace as it does for [`changes`].
// @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
// @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:1dc22603643b17fad4874110ef09a3391b67d71c28a9f5b35a7dfa18438f3409
pub fn reviews_of(program: &Program, unit: &Unit) -> Vec<ReviewRecord> {
    let path = review_file(&program.workspace, unit);
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0aa4f9eaff20fee7bf263db985597b4d886a2ac762b47b1af1734107a9ac1503
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let Some(records) = items
        .iter()
        .map(ReviewRecord::from_json)
        .collect::<Option<Vec<ReviewRecord>>>()
    else {
        return Vec::new();
    };
    let local: HashSet<String> = unit
        .requirements(program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:d95399b23b29d50a8fba5af56c20731f8f7b0b2bafe16e9914d5090e374ce251
    records
        .into_iter()
        // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0b41caebdc61220c67f088bc69f465267b2c9e2b36d07c52b02680bf6cb4724c
        .filter(|record| local.contains(&record.id))
        .collect()
}

/// Whether the review records of one unit are recorded in its review record file.
///
/// The file holds them afterwards, replacing whatever it held, and no other file is written.
/// It is the pretty JSON of the records in the order they were given, with the keys of every
/// object in alphabetical order, so the same records always give the same bytes and a diff
/// shows only what changed. Every missing folder above the file is created, so a project
/// cloned without `elfie-compile` records its verdicts; no record at all removes the file,
/// so every criterion of the unit is reviewed again. `false` when the file cannot be
/// written.
// @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:5708abb1bf089eb06657f5688610a6c234b81463164fd199b77b315608316837
// @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:8d039cf20453fa3420e6acdb3ce42b2031909a9bc211b646a683be5017efa7fd
pub fn record_reviews(workspace: &Workspace, unit: &Unit, records: &[ReviewRecord]) -> bool {
    let path = review_file(workspace, unit);
    if records.is_empty() {
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:0149e6a1c5272af0b7ca76e627e0fb18e511db4e2076f128db7607e14f49ae85
        return match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
    }
    // The readable layout of the records, in the order they were given, with the keys of
    // every object in alphabetical order, so the same records always give the same bytes.
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:52d12769dcd44be836a5afeb17b9c7341a2a82265bafc029ee658a417f8e0569
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:c0e143aef64c3c7bed3e5892dd6d03a0c46df17166ce5ffa598b2db167027369
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:60d0cd914ebe3c1cbbd582cd3229d8adc3a00b879720f1caeec671fb4c707551
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:db1f607aac959c64937cb811664557dedde402eb3fdcaaaf6a2db6b1284ecfd1
    let value = serde_json::Value::Array(records.iter().map(ReviewRecord::to_json).collect());
    let Ok(mut text) = serde_json::to_string_pretty(&value) else {
        return false;
    };
    text.push('\n');
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:72846edcc746900edf9c781c66023ad863b10442a8c62d330bee15766a2090c3
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:1f4671d2ae5027d4e5bcfaf56dc68c765e27e31a92fc783b7f64a2165f391dd4
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
        // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:d0dd761f2a88d10b62707b3452f9804e82f4c9ae6d4212b5b8b799f9355a82c5
        if target.is_some_and(|target| map.target != target) {
            continue;
        }
        for marker in &map.markers {
            let matched = match &marker.entity {
                // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:38d2833b48e58c718adaf9951281dfe50568a87b1fd9fcf0cf493596b2406bae
                // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:093e721919b4f8d968ccd2497d31d9e1ac8783ddfb5ee2bbda092b4302715a0f
                Some(entity) => entity == name || entity.starts_with(&member),
                // A marker that names a line belongs to the entity whose declaration
                // covers it.
                // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:66a35d2038e53526c71eb7ac77c60c00ba7678d9ffc14769336d39c7f3aeadb6
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
    let target = workspace.targets[unit.target].identifier.clone();
    // What the unit's file declares for its target, in file order — not what the unit
    // compiles: an entity that moved to another unit is still one of this file's, and one
    // that moved in is the declaring file's to report.
    // @lfy def/generation/main.lfy:changes#changes:changes:718b0ba1ae484d5d40adbb3d2dabcfed7ec6b7a0c976dd9193d6c8b00e479f4f
    let declared = declared_entities(program, &target, &path);
    let now = entity_table(model, &declared);
    let Some(previous) = previous else {
        // @lfy def/generation/main.lfy:changes#changes:changes:771d19c7cdc3e68ae80e9c437cf28ae04d6d6112c99fdff85bcf61c1efacc428
        return declared
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
    // @lfy def/generation/main.lfy:changes#changes:changes:6fd357814bc465ff2deac47548fc8b644c75b3489eab95e456e7d68a0da79754
    let before = crate::interpret::lower(crate::workspace::change(
        workspace,
        &path,
        Some(previous),
    ));
    // What the file declared then, built for the unit's target, read exactly as what it
    // declares now was read above, so that neither side is a unit's contents.
    // @lfy def/generation/main.lfy:changes#changes:changes:718b0ba1ae484d5d40adbb3d2dabcfed7ec6b7a0c976dd9193d6c8b00e479f4f
    let was = entity_table(
        &before.workspace.model,
        &declared_entities(&before, &target, &path),
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
// @lfy def/generation/main.lfy:changes#changes:changes:d7b590dfd89c646eecf798bdafc911cd87dc2b66f0c3bc1e3cb916051b5fa6f8
// @lfy def/generation/main.lfy:changes#changes:changes:f54761e4898e7ebd66aacb630307ee416935a1052c402126c3c712e2eb632c2a
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

/// The entities the file at a path declares for a target in a lowered program, in file
/// order; none when the file or the target is not in it.
///
/// What a unit compiles is not this: an entity of the file may have moved into another
/// unit, and one of another file may have moved into this one. A change is of the file.
// @lfy def/generation/main.lfy:changes#changes:changes:718b0ba1ae484d5d40adbb3d2dabcfed7ec6b7a0c976dd9193d6c8b00e479f4f
fn declared_entities(before: &Program, target: &str, path: &str) -> Vec<EntityId> {
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
        target.declaration,
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
    model::type_text_of(model, model.entities[entity].ty.as_ref()).unwrap_or_default()
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
    model::type_text_of(model, model.entities[entity].output()).unwrap_or_default()
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
/// criterion and test whose verdict is not carried.
///
/// The instructions name the batch and the target, then, unit by unit, every entity with a
/// criterion or test under review: its name, its definition, where each of its regions is,
/// and each of those criteria and tests on one line with its id and its place — the file of
/// its origin relative to the workspace root, a colon, and the line its origin begins on —
/// then, for that unit, the criteria and tests of its own file, and last the verifier
/// protocol. No line of any output is quoted: a region is named, and the verifier pulls its
/// code through the agent server. Only local criteria and tests are reviewed here; a global
/// one is reviewed once, by [`global_review`].
///
/// `records` is what the batch's units were last reviewed as, as [`reviews_of`] gives them.
/// A criterion or test is listed when nothing says its verdict still holds: it has no
/// record, its last verdict was not satisfied, or the regions of its entity hash to
/// something other than what it was judged against. One whose entity generated no region,
/// and one of a unit's own file, is always listed and never carried, since there is no code
/// of its own whose sameness could stand for the verdict. With nothing left to list, the
/// instructions say so and hold no protocol.
///
/// `directory` is the directory the batch's outputs were written in, which the regions a
/// hash is taken over are read from, and `None` means the workspace root.
// Decision: the verifier is told where the regions are and pulls their code through the
// agent server, so a request stays small however large the batch's outputs are, and its
// protocol allows no edits: a review says where and why, never how to fix.
// Decision: a batch compiled in a copy of the root is reviewed in that copy, so the regions
// a verifier is pointed at, and the hashes its verdicts are recorded with, are those of the
// outputs it is judging and never the root's older ones.
// Decision: a criterion or test is reviewed again only when nothing says its verdict still
// holds; a change to code outside the entity's regions, such as a helper no marker names,
// changes no hash and so carries every verdict of that entity.
// Decision: the definition passes the plan and the batch; a plan here does not own its
// workspace or the program lowered from it, so the program is an extra first parameter.
// @lfy def/generation/main.lfy:review#review:review:b4d9c6e00da87a0e74023db7212d00d3105247c847c18b29e94bf3fbe6925699
// @lfy def/generation/main.lfy:review#review:review:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
pub fn review(
    program: &Program,
    plan: &Plan,
    batch: &Batch,
    source_maps: &[SourceMap],
    directory: Option<&Path>,
    records: &[ReviewRecord],
) -> ReviewRequest {
    let workspace = &program.workspace;
    let model = &workspace.model;
    // @lfy def/generation/main.lfy:review#review:review:1c072387bd071f939c216da6d4ce6239cccd17ffa2df2f585223778cc3680fca
    let directory = directory.unwrap_or(workspace.root.as_path());
    let target = batch
        .units
        .first()
        .map(|&index| workspace.targets[plan.units[index].target].identifier.as_str())
        .unwrap_or_default();

    // Unit by unit, entity by entity: the regions of the entity, the hash they come to, and
    // which of its criteria and tests that hash leaves to review.
    let mut asked: Vec<String> = Vec::new();
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut carried: Vec<ReviewRecord> = Vec::new();
    let mut sections = String::new();
    for &index in &batch.units {
        let unit = &plan.units[index];
        let mut body = String::new();
        for &entity in &unit.entities {
            let regions = entity_regions(workspace, source_maps, entity, target);
            // @lfy def/generation/main.lfy:review#review:review:2406e70a6c25c47a8f50f45cfb28517ffe7990ce2a1647dd53f04024b5ea5269
            let hash = source_hash(&regions_text(directory, &regions));
            let requirements = reviewed_requirements(program, unit.lowered, entity);
            let listed = listed_requirements(
                &requirements,
                &hash,
                records,
                !regions.is_empty(),
                &mut hashes,
                &mut carried,
            );
            // @lfy def/generation/main.lfy:review#review:review:bb873e216e23bb2a7659d3b1888588387a1f7980df4fac536938ec32d6d42b8d
            if listed.is_empty() {
                continue;
            }
            let (criteria, tests) = split_requirements(&listed);
            write_review_entity(&mut body, model, entity, &regions, &criteria, &tests);
            asked.extend(ids_of(&criteria));
            asked.extend(ids_of(&tests));
        }
        // A criterion or test of the unit's own file answers for no entity's code, so it has
        // no region of its own, hashes to the empty text, and is always listed.
        // @lfy def/generation/main.lfy:review#review:review:efbaab9e13ed7569408b035ce9b34392a9e0b4fa0a274b747ea27c0ef681cd65
        // @lfy def/generation/main.lfy:review#review:review:944917284f9b6da4a52ad0fe6ba57ee0440f72ee59ecdfdc8b90585e6e1e7417
        let file = &program.files[unit.lowered];
        let own: Vec<Requirement> = file
            .criteria
            .iter()
            .cloned()
            .map(Requirement::Criterion)
            .chain(file.tests.iter().cloned().map(Requirement::Test))
            .collect();
        let listed = listed_requirements(
            &own,
            &source_hash(""),
            records,
            false,
            &mut hashes,
            &mut carried,
        );
        if !listed.is_empty() {
            let (criteria, tests) = split_requirements(&listed);
            let _ = writeln!(body, "#### `{}`\n", workspace.files[unit.file].path);
            write_reviewed(&mut body, model, "Criteria", &criteria);
            write_reviewed(&mut body, model, "Tests", &tests);
            asked.extend(ids_of(&criteria));
            asked.extend(ids_of(&tests));
        }
        if !body.is_empty() {
            let _ = writeln!(
                sections,
                "### `{}` (stem `{}`)\n",
                workspace.files[unit.file].path, unit.stem
            );
            sections.push_str(&body);
        }
    }

    // A heading naming the batch and the target, then what is under review, then the
    // protocol; with nothing under review, the heading and one line saying so.
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Reviewing the batch `{}` for the target `{}`\n",
        batch.identifier, target
    );
    if asked.is_empty() {
        // @lfy def/generation/main.lfy:review#review:review:5133eddf9b981ba9bac41113779c58b491cedfbc67e2e3742ac236b381a254dc
        out.push_str(
            "Nothing in the batch needs review: every criterion and test of its units was \
             found satisfied against the code it still stands on.\n",
        );
    } else {
        // @lfy def/generation/main.lfy:review#review:review:f3ebae56b7c2144a9edb3e3b69ac07f174e3974f258284e909f2e10068ec9831
        out.push_str(
            "Every criterion and test below is local to one entity of this batch. A global \
             one is reviewed once, on its own, across every output of the program.\n\n",
        );
        out.push_str("## What was asked\n\n");
        out.push_str(&sections);
        // The verifier protocol.
        out.push_str(PROTOCOL);
    }

    // @lfy def/generation/main.lfy:review#review:review:197b2d0e6f7458d538f3cc8d8568be98a60636ca07026cd2eeb1d5933910f156
    ReviewRequest {
        batch: Some(batch.clone()),
        instructions: out,
        asked,
        hashes,
        carried,
    }
}

/// Every region one entity of a unit generated: each marker [`regions_of`] finds for its
/// name among the source maps of the batch's target whose file is the file that declares the
/// entity, in output order and then in output line order.
///
/// Only what the file declaring the entity generated, so an entity of another file with the
/// same name brings none of its code, while an entity moved into the unit brings its own,
/// which its declaring file marks.
// @lfy def/generation/main.lfy:review#review:review:0f43e9772c4aad39520e42801704e6fe6e9115829e267701e251b74908849820
// @lfy def/generation/main.lfy:review#review:review:e67f26bdccf8eaa77ebbfe85b9da0b83bf73bff7b2cad901af69d8b4797ad76e
fn entity_regions<'m>(
    workspace: &Workspace,
    source_maps: &'m [SourceMap],
    entity: EntityId,
    target: &str,
) -> Vec<(&'m SourceMap, &'m Marker)> {
    let model = &workspace.model;
    let declaring = declaring_path(model, entity);
    let name = qualified_name(model, entity);
    let mut out: Vec<(&SourceMap, &Marker)> =
        matching_regions(workspace, source_maps, &name, Some(target))
            .into_iter()
            .filter(|(_, marker)| declaring == Some(marker.file.as_str()))
            .collect();
    out.sort_by(|(left_map, left), (right_map, right)| {
        left_map
            .output
            .cmp(&right_map.output)
            .then(left.output_line.cmp(&right.output_line))
    });
    out
}

/// The text the hash of one entity's criteria and tests is taken over: the lines of each
/// region in its own output, one region after another, each line followed by a line break.
/// A region whose output cannot be read adds nothing.
///
/// `directory` is the directory the outputs were written in, which each region is read from.
// @lfy def/generation/main.lfy:review#review:review:2406e70a6c25c47a8f50f45cfb28517ffe7990ce2a1647dd53f04024b5ea5269
// @lfy def/generation/main.lfy:review#review:review:b20a4c21286c959c6a77873bc8d178ac43d720cb03877030c893be7f015f5903
fn regions_text(directory: &Path, regions: &[(&SourceMap, &Marker)]) -> String {
    let mut out = String::new();
    for (map, marker) in regions {
        let Ok(text) = std::fs::read_to_string(directory.join(&map.output)) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        let last = marker.end.min(lines.len());
        let first = marker.output_line.saturating_sub(1).min(last);
        for line in &lines[first..last] {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Which of one entity's criteria and tests a review lists, with every one of them recorded
/// in `hashes` under the hash of that entity's regions and every one it leaves out recorded
/// in `carried`.
///
/// `has_region` is whether the entity generated any region at all: with none, there is no
/// code whose sameness could stand for a verdict, so nothing is carried.
// @lfy def/generation/main.lfy:review#review:review:5ff1db3eab7473b565a623216b6a86f264541a52a33fc4c939e828c7f537c783
// @lfy def/generation/main.lfy:review#review:review:32e025cd22c3a749bbc3afba52f86cb9b26ac41d0f4a822a6e5e00639e557312
// @lfy def/generation/main.lfy:review#review:review:944917284f9b6da4a52ad0fe6ba57ee0440f72ee59ecdfdc8b90585e6e1e7417
fn listed_requirements<'r>(
    requirements: &'r [Requirement],
    hash: &str,
    records: &[ReviewRecord],
    has_region: bool,
    hashes: &mut BTreeMap<String, String>,
    carried: &mut Vec<ReviewRecord>,
) -> Vec<&'r Requirement> {
    let mut out: Vec<&Requirement> = Vec::new();
    for requirement in requirements {
        hashes.insert(requirement.id().to_string(), hash.to_string());
        let held = records
            .iter()
            .find(|record| record.id == requirement.id())
            .filter(|record| {
                has_region && record.status == ReviewStatus::Satisfied && record.hash == hash
            });
        match held {
            Some(record) => carried.push(record.clone()),
            None => out.push(requirement),
        }
    }
    out
}

/// Criteria and tests apart, each in the order they were given.
// @lfy def/generation/main.lfy:review
fn split_requirements<'r>(
    requirements: &[&'r Requirement],
) -> (Vec<&'r Requirement>, Vec<&'r Requirement>) {
    requirements
        .iter()
        .copied()
        .partition(|requirement| matches!(requirement, Requirement::Criterion(_)))
}

/// The ids of criteria or tests, in the order they were given.
// @lfy def/generation/main.lfy:review
fn ids_of(requirements: &[&Requirement]) -> Vec<String> {
    requirements
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect()
}

/// Every local criterion and test the declaration of one entity of a unit holds: its own and
/// those of its members, criteria before tests at each level, taken from the unit's own
/// lowered file or, for an entity moved into the unit, from the file it came from.
// Decision: a member's criteria are reviewed under the declaration that holds it, as
// `regionsOf` gives a member's regions to its owner, so every local criterion and test of a
// unit is listed under one of its entities or under its own file and none goes unasked.
// @lfy def/generation/main.lfy:review#review:review:f3ebae56b7c2144a9edb3e3b69ac07f174e3974f258284e909f2e10068ec9831
fn reviewed_requirements(program: &Program, lowered: usize, entity: EntityId) -> Vec<Requirement> {
    let node = lowered_node_of(&program.files[lowered], entity)
        .or_else(|| lowered_declaration(program, entity));
    let mut out: Vec<Requirement> = Vec::new();
    if let Some(node) = node {
        requirements_within(node, &mut out);
    }
    out
}

/// The criteria and tests of one declaration, appended: those of the node, criteria before
/// tests, then those of the nodes within it, which are its members.
// @lfy def/generation/main.lfy:review
fn requirements_within(node: &LoweredNode, out: &mut Vec<Requirement>) {
    out.extend(node.criteria.iter().cloned().map(Requirement::Criterion));
    out.extend(node.tests.iter().cloned().map(Requirement::Test));
    for child in node.nodes() {
        requirements_within(child, out);
    }
}

/// One entity of a unit for a verifier: its name, its definition, where each of its regions
/// is, and then each criterion and each test under review on one line prefixed by its id and
/// its place.
// @lfy def/generation/main.lfy:review#review:review:b4d9c6e00da87a0e74023db7212d00d3105247c847c18b29e94bf3fbe6925699
fn write_review_entity(
    out: &mut String,
    model: &Model,
    entity: EntityId,
    regions: &[(&SourceMap, &Marker)],
    criteria: &[&Requirement],
    tests: &[&Requirement],
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
    write_regions(out, regions);
    write_reviewed(out, model, "Criteria", criteria);
    write_reviewed(out, model, "Tests", tests);
}

/// Each criterion or test under review on one line: its id, its place, an em dash, and its
/// text.
// @lfy def/generation/main.lfy:review#review:review:b4d9c6e00da87a0e74023db7212d00d3105247c847c18b29e94bf3fbe6925699
fn write_reviewed(out: &mut String, model: &Model, heading: &str, requirements: &[&Requirement]) {
    let _ = writeln!(out, "{heading}:");
    if requirements.is_empty() {
        out.push_str("(none)\n\n");
        return;
    }
    for requirement in requirements {
        let (id, origin, text) = match requirement {
            Requirement::Criterion(criterion) => {
                (&criterion.id, criterion.origin, criterion.text())
            }
            Requirement::Test(test) => (&test.id, test.origin, test_text(test)),
        };
        let _ = writeln!(out, "`{id}` {} — {text}", origin_place(model, origin));
    }
    out.push('\n');
}

/// A criterion's or test's place: the file of its origin relative to the workspace root, a
/// colon, and the line its origin begins on.
// @lfy def/generation/main.lfy:review
// @lfy def/generation/data.lfy:ReviewRequest#ReviewRequest:ReviewRequest:89af425d63c0f40d30e4db0494e14a97b63ea5ff20139e294920432acbccb8cf
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

/// Where every region given is: the output path, a colon, the first output line, a dash, and
/// the last, one region per line. None of their code is written, so the instructions hold no
/// line of any output; the verifier reads a region through the agent server.
// @lfy def/generation/main.lfy:review#review:review:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
// @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
fn write_regions(out: &mut String, regions: &[(&SourceMap, &Marker)]) {
    if regions.is_empty() {
        out.push_str("No region of any output was generated for it.\n\n");
        return;
    }
    out.push_str("Regions:\n");
    for (map, marker) in regions {
        let _ = writeln!(out, "`{}:{}-{}`", map.output, marker.output_line, marker.end);
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------------------
// globalReview
// ---------------------------------------------------------------------------------------

/// Everything a verifier is handed to check every global criterion and test once, across
/// every output of the program.
///
/// The instructions name the global review and the targets, then give each global criterion
/// and then each global test with its id and its place, its text, and where every region
/// whose marker answers for its id is; one that no marker of any output names is listed with
/// a line saying no unit answered for it, so the verifier finds it unverifiable, never
/// violated. No line of any output is quoted, and nothing is carried: a global criterion is
/// asked about every time the review runs. The caller runs it once after every batch of a
/// compile, and whenever the program's global criteria or tests changed; a review it finds
/// violated gives the stems of the units whose markers answered for it, for the `violated`
/// argument of [`plan`].
// Decision: the definition passes the plan; a plan here does not own the program lowered
// from its workspace, and nothing else of the plan is read, so the program stands in for it.
// @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
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
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:3163f899ae0e06b99cc3cfbdf7857a8f62a41a8e5685323600713e1e6347797b
    let mut asked: Vec<String> = Vec::new();
    for requirement in &globals {
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
        let (id, origin, text) = match requirement {
            Requirement::Criterion(criterion) => {
                (&criterion.id, criterion.origin, criterion.text())
            }
            Requirement::Test(test) => (&test.id, test.origin, test_text(test)),
        };
        asked.push(id.clone());
        let _ = writeln!(out, "## `{id}` {}\n", origin_place(model, origin));
        let _ = writeln!(out, "{text}\n");
        // Every region whose marker answers for this id, whatever target it was built for.
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
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
            // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:602d5d9913771ebb9a5ab23bc1b03ed23f9e22888e45cf65e826b7cfdbcc02e2
            out.push_str("No unit answered for it, so it cannot be checked against any region.\n\n");
            continue;
        }
        write_regions(&mut out, &regions);
    }

    // The verifier protocol.
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
    out.push_str(PROTOCOL);

    ReviewRequest {
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:de84b01e368319ebde8eb693c9242574f7b87c6cdedc5229f53fbdf2a3cdd11d
        batch: None,
        instructions: out,
        asked,
        // A global criterion stands on the whole program rather than on the regions of one
        // entity, so there is no hash of its own code to carry a verdict on.
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e9cc8a4c5b45b3440c827bd67a1fd514c12870041202d0cd54fad21179548843
        hashes: BTreeMap::new(),
        carried: Vec::new(),
    }
}

/// What a verifier is told about its own work: that it examines and never edits, how it
/// reads the code of a region, what it submits for each criterion and each test, and how its
/// final message ends.
// Decision: the protocol asks for the place as well as the id, though [`read_review`]
// derives the place from the id and never from what was submitted; a verifier that copies
// the place it was given names its criterion in a way a person can read, and one that
// garbles the id is still placed by the file and line it copied.
// @lfy def/generation/main.lfy:review
// @lfy def/generation/main.lfy:globalReview
// @lfy def/generation/data.lfy:ReviewRequest#ReviewRequest:ReviewRequest:5f3541f331f08c11374202659e49f252d011829d997b2663a9a7aa1eb494c55d
const PROTOCOL: &str = "\
## How to review

You examine and never edit: you change no file, and you propose no code.

The code of a region is read through `elfie_output`: by the id of a criterion or test, by \
the name of an entity, or by reading the lines of the output file the region names.

Submit each verdict through `elfie_report` as soon as you have judged it, one per criterion \
and per test:

- `id` is copied from the id the criterion or test is given above, or left out when you \
  give `file` and `line` instead: the two halves of the place given with it above, the path \
  before the colon and the number after it.
- `status` is `satisfied` when the code does what the criterion says, `violated` when it \
  does something else, or `unverifiable` when no region can be checked against it. A \
  criterion no region can be checked against is unverifiable, never violated.
- `evidence` is the output path and the line range you read, such as \
  `crates/elfie-core/src/x.rs:120-134`, and empty when you read none.
- `note` is one line saying why and, for a violated one, what the code does instead.

The tools of the agent server are called before files are searched or read by hand: \
`elfie_source` for where a line of generated code came from, `elfie_entity` for a \
definition with its criteria and tests, and `elfie_references` for where a name is used.

Where `elfie_report` is not offered, write each verdict in your final message instead, as \
one JSON object on one line with exactly the keys `id`, `file`, `line`, `status`, \
`evidence`, and `note`, holding what `elfie_report` would be given.

End your final message with exactly one line reading `ELFIE: REVIEWED`.
";

// ---------------------------------------------------------------------------------------
// reviewOf
// ---------------------------------------------------------------------------------------

/// What a verifier found, read mechanically from the verdicts it submitted and from its
/// report.
///
/// `recorded` is the reviews the verifier submitted through the agent server during the run,
/// in the order they were recorded, and empty when it submitted none. They come first, and a
/// line of the report naming a criterion one of them already names is dropped: it is neither
/// a review nor a problem.
///
/// The end line is the last line reading `ELFIE: REVIEWED`, with any whitespace around it;
/// lines after it are ignored. The lines read are those before it, or every line when there
/// is none, each with the whitespace around it removed. A line read beginning with `{` is a
/// review line, and every other line is ignored, whatever it says, so prose, headings, and
/// fences around the reviews are never problems. A review line that reads as an object
/// whose status names a member of [`ReviewStatus`], whatever its case, and that names a
/// criterion or test of the program is one review, in report order; every other review line
/// is a problem, as written. Of two that name the same criterion or test only the last is
/// kept, at its own place. With no end line the problems end with a line saying the report
/// did not end.
///
/// A review line names its criterion or test by its id, by an id a character short or long,
/// or — with no id, or with one that names nothing and is near no single one — by the file
/// and line where exactly one of them begins. Its file, line, and entity are derived from
/// that origin, as [`Review::at`] fills them.
// Decision: the report is read leniently and only what tries to be a review can be wrong,
// because a verifier that adds a sentence or a fence around its lines has still reviewed
// everything, and running a second verifier for that costs as much as the first.
// Decision: a verdict submitted through the agent server was checked when it arrived, so it
// wins over a line of the report for the same criterion; the report is read only for what
// was not submitted.
///
/// `asked` is the ids the review request gave the verifier, and empty when they are not
/// known; each of them no review names is missing.
// @lfy def/generation/main.lfy:reviewOf
pub fn review_of(
    report: &str,
    program: &Program,
    asked: &[String],
    recorded: &[Review],
) -> ReviewReport {
    /// What the problems end with when the report has no end line.
    const UNENDED: &str = "the report did not end";

    let placed = placed_requirements(program);
    let begun = requirements_by_place(&placed);
    let lines: Vec<&str> = report.lines().collect();
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:10e1a05e247051fa67e5af8f9351b3d6c08728d0642aa557109f7db27e01e1d6
    let end = lines.iter().rposition(|line| line.trim() == REVIEWED);
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:10e1a05e247051fa67e5af8f9351b3d6c08728d0642aa557109f7db27e01e1d6
    let body = match end {
        Some(index) => &lines[..index],
        None => &lines[..],
    };
    // What the verifier submitted during the run comes first, in the order it was recorded.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9ebef2c6f21093486b00c8a0124de7e4c4262014632c2a66b0b47702c693b59d
    let mut reviews: Vec<Review> = Vec::new();
    for review in recorded {
        reviews.retain(|kept| kept.id != review.id);
        reviews.push(review.clone());
    }
    let submitted = reviews.len();
    let mut problems: Vec<String> = Vec::new();
    let mut unplaced: Vec<String> = Vec::new();
    // The reviews read from the report and the problems come in report order, as the body is
    // read line by line.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:fdcd21d1f06a140e2bfae307be2625b9c27d43f2b73e320c1f152a39fd9638bc
    for line in body {
        // Whitespace around a line is ignored wherever the line is compared or parsed.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:2bf986f1d26195d0e45f531c18f47a790716471b351423d3a52d37d04c898921
        let text = line.trim();
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:24c8a0d8aab48ace02aea35fdac897930db4e5ea8b8b4bd675b3bd00afefeac3
        if !text.starts_with('{') {
            continue;
        }
        match read_review(text, &placed, &begun) {
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:4bf722eab5ca52f5732c6c9f8f080d2efd0dbed5df5ea0319fa4c4a56cade02c
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:731b47ce4e9dbb551b71e00078efb0e67419160fb2a75436b70be87929a8b8a9
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ffcb26fa671fb0476f3effa3e247f546bf1144d511802cfdd3a726c5756fc018
            Some(review) => {
                // A criterion the verifier already submitted a verdict for was checked when
                // it arrived, so the line adds nothing and is no problem either.
                // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:42415f0b3dec0f93094d195e8abb7b6b4c6f2a363c4db117d8c0c9e8b3c5417e
                if reviews[..submitted]
                    .iter()
                    .any(|held| held.id == review.id)
                {
                    continue;
                }
                // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:19f540298a118815926304ba19a124afbedf6dedfe2303d1b51cb9b16e308600
                reviews.retain(|kept| kept.id != review.id);
                reviews.push(review);
            }
            // A line that reads as no object, that names no status of a review, or that
            // places itself nowhere is a problem as written.
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:cc8ee014686684d4965cbc76770ce45c98db561453c188050a7a3d3c4b345ad6
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:065297aa8507d1cce029b56f7ee5ede6af8c5913fb55092cd6433f748dcff490
            // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ebd07d63053ee05f2507003716efdd9c78f20b38aecdf60cca2c1874d85612ac
            None => {
                // A line that claims a violation and places it nowhere is kept apart as
                // well, so that no violation is lost to an id a verifier garbled.
                // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:d4b52386cd8dd2565497fdaaf59b6fcafe4852653e38d732b6f4a7f4cb2d42f2
                if claims_violation(text) {
                    unplaced.push(text.to_string());
                }
                problems.push(text.to_string());
            }
        }
    }
    if end.is_none() {
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:b6eda5717d7bcd79108423e0c0c000b75e1bd73f8bd2a124b3cefcb31a67bae1
        problems.push(UNENDED.to_string());
    }
    // Each id the review was asked about that no review names, in the order it was asked.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:a7bd850a8fc6e8bf19c7c580ea931ed26ea1ea26c2cad6c2ea48096aa93c9535
    let missing: Vec<String> = asked
        .iter()
        .filter(|id| !reviews.iter().any(|review| &&review.id == id))
        .cloned()
        .collect();
    ReviewReport {
        reviews,
        problems,
        unplaced,
        missing,
    }
}

/// Whether a review line that was read as no review still says its status is violated: its
/// text reads as an object whose status names [`ReviewStatus::Violated`], whatever its case.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:d4b52386cd8dd2565497fdaaf59b6fcafe4852653e38d732b6f4a7f4cb2d42f2
fn claims_violation(line: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let Some(status) = value.get("status").and_then(serde_json::Value::as_str) else {
        return false;
    };
    ReviewStatus::from_name(&status.trim().to_lowercase()) == Some(ReviewStatus::Violated)
}

/// One review from one review line: `None` unless the line reads as an object whose status
/// names a member of [`ReviewStatus`], whatever its case, and that names a criterion or
/// test of the program. Its evidence and note are taken as given, and empty when missing;
/// every other key is ignored.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:03c66ae037b851c7cce98a476e8c2dc9fe0e1d51213bd0f884fca3d2bf00fdea
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9edd00519d199e54062049e61315587483989f62a46fc5a31c9be591c90931ce
fn read_review(
    line: &str,
    placed: &BTreeMap<String, (String, usize, String)>,
    begun: &BTreeMap<(String, usize), String>,
) -> Option<Review> {
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let object = value.as_object()?;
    let spelled = object.get("status")?.as_str()?.to_ascii_lowercase();
    let status = ReviewStatus::from_name(&spelled)?;
    let id = named_requirement(object, placed, begun)?;
    let (file, at, entity) = placed.get(&id)?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Some(
        Review {
            id,
            file: String::new(),
            line: 0,
            entity: String::new(),
            status,
            evidence: text("evidence"),
            note: text("note"),
        }
        .at(file, *at, entity),
    )
}

/// Which criterion or test of the program a review line names: the one its id is; the one
/// exactly one id near it begins or is begun by, so an id copied a character short or long
/// still counts; or, failing both — with no id, or with one that is no id of the program and
/// near no single one — the one that begins alone where its file and line say.
// @lfy def/generation/main.lfy:reviewOf
fn named_requirement(
    object: &serde_json::Map<String, serde_json::Value>,
    placed: &BTreeMap<String, (String, usize, String)>,
    begun: &BTreeMap<(String, usize), String>,
) -> Option<String> {
    let given = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:4bf722eab5ca52f5732c6c9f8f080d2efd0dbed5df5ea0319fa4c4a56cade02c
    if placed.contains_key(given) {
        return Some(given.to_string());
    }
    // An id a character short or long names what it nearly is, as long as it nearly is one
    // thing only; an empty id is near every id, so it names nothing by this reading.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:731b47ce4e9dbb551b71e00078efb0e67419160fb2a75436b70be87929a8b8a9
    if !given.is_empty() {
        let mut near = placed.keys().filter(|id| is_near(id, given));
        if let Some(found) = near.next()
            && near.next().is_none()
        {
            return Some(found.clone());
        }
    }
    // With no id, or with one that names nothing and is near nothing or near more than one
    // thing, the place the line gives names it when exactly one begins there.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ffcb26fa671fb0476f3effa3e247f546bf1144d511802cfdd3a726c5756fc018
    let file = object.get("file")?.as_str()?.trim().to_string();
    let line = usize::try_from(object.get("line")?.as_u64()?).ok()?;
    begun.get(&(file, line)).cloned()
}

/// Whether two ids are the same one up to a character: one begins the other, and they share
/// their receiver, their contributor, and at least sixteen hex digits of their hash.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:731b47ce4e9dbb551b71e00078efb0e67419160fb2a75436b70be87929a8b8a9
fn is_near(id: &str, given: &str) -> bool {
    /// How many hex digits of the hash two ids share to be the same one.
    const SHARED: usize = 16;

    if !id.starts_with(given) && !given.starts_with(id) {
        return false;
    }
    let Some((receiver, contributor, hash)) = id_parts(id) else {
        return false;
    };
    let Some((their_receiver, their_contributor, their_hash)) = id_parts(given) else {
        return false;
    };
    if receiver != their_receiver || contributor != their_contributor {
        return false;
    }
    hash.chars()
        .zip(their_hash.chars())
        .take_while(|(left, right)| left == right && left.is_ascii_hexdigit())
        .count()
        >= SHARED
}

/// The receiver, the contributor, and the hash of an id: the receiving entity's name, or
/// `global`, then the name of the entity whose body holds it, then the hash, each after a
/// colon. The receiver and the contributor hold no colon, so the hash is what follows the
/// last one.
// @lfy def/generation/main.lfy:reviewOf
fn id_parts(id: &str) -> Option<(&str, &str, &str)> {
    let (head, hash) = id.rsplit_once(':')?;
    let (receiver, contributor) = head.split_once(':')?;
    Some((receiver, contributor, hash))
}

/// The id of the criterion or test that begins at each place where exactly one of them
/// begins, as its file and its line; a place two begin at is left out, since a review line
/// with no id would name neither.
// @lfy def/generation/main.lfy:reviewOf
fn requirements_by_place(
    placed: &BTreeMap<String, (String, usize, String)>,
) -> BTreeMap<(String, usize), String> {
    let mut out: BTreeMap<(String, usize), Option<String>> = BTreeMap::new();
    for (id, (file, line, _)) in placed {
        out.entry((file.clone(), *line))
            .and_modify(|held| *held = None)
            .or_insert_with(|| Some(id.clone()));
    }
    out.into_iter()
        .filter_map(|(place, id)| id.map(|id| (place, id)))
        .collect()
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
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:55eeb537f74d15d3b738387a76843bfe4e688258322b57eb5046ead0be1e71dc
    if !is_source_path(&path) {
        return None;
    }
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:e5a2dc69b3714f2fe9ec245e19aa4b7d2bc03926cf7410cabc1e0a7ed86cf86c
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

/// A path under a directory, both relative to the root.
fn under_root(directory: &str, name: &str) -> String {
    if is_root(directory) {
        name.to_string()
    } else if name.is_empty() {
        directory.to_string()
    } else {
        format!("{directory}/{name}")
    }
}

/// `Path.extension`: what follows the last dot of the last segment, without the dot; `None`
/// when the name has no dot or only a leading one.
// @lfy def/generation/main.lfy:accept
fn extension_of(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(0) | None => None,
        Some(dot) => Some(&name[dot + 1..]),
    }
}

/// The directory holding a file, relative to the root; empty for a file at the root.
fn directory_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// A path with every `.` dropped and every `..` applied to the segment before it, never
/// looking at the disk.
fn normalized(path: &str) -> String {
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

/// `Path.relative`: the path that names the same place from `base`; `None` when one is
/// absolute and the other is not, since nothing relates them. Both are normalized before
/// they are compared, and the result is the segments of the path after the ones it shares
/// with `base`, preceded by one `..` for each segment `base` has beyond the shared ones;
/// `.` when the two name the same place.
fn path_relative(path: &str, base: &str) -> Option<String> {
    if path.starts_with('/') != base.starts_with('/') {
        return None;
    }
    let path = normalized(path);
    let base = normalized(base);
    let path: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
    let base: Vec<&str> = base.split('/').filter(|segment| !segment.is_empty()).collect();
    let shared = path.iter().zip(&base).take_while(|(a, b)| a == b).count();
    let mut segments = vec![".."; base.len() - shared];
    segments.extend_from_slice(&path[shared..]);
    if segments.is_empty() {
        return Some(".".to_string());
    }
    Some(segments.join("/"))
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

        /// A project with one target `rust`, an output directory `src`, and an empty `def`
        /// directory. The package `elfie` beside it is the standard library every fixture
        /// is loaded against: its main file is the prelude, so `Target`, the roles its
        /// slots take, and `targetOf` are in scope everywhere. `def/roles.lfy` gives the
        /// language, the layout, and the ecosystem a target needs, and `def/targets.lfy`
        /// declares the `ace const` that holds the target and adds it to `global`, so that
        /// every entity of every project file is built for it.
        fn with_rust_target() -> Fixture {
            let fixture = Fixture::new();
            fixture
                .write(
                    "elfie.json",
                    r#"{ "dependencies": { "elfie": { "root": "lib" } } }"#,
                )
                .library()
                .write("def/roles.lfy", ROLES)
                .write("def/targets.lfy", TARGETS);
            fs::create_dir_all(fixture.root.join("def")).unwrap();
            fixture
        }

        /// The library of the package `elfie`, written under `lib`.
        fn library(&self) -> &Fixture {
            self.write("lib/main.lfy", LIBRARY)
                .write("lib/prelude/builtin.lfy", LIBRARY_BUILTIN)
                .write("lib/prelude/entity.lfy", LIBRARY_ENTITY)
                .write("lib/prelude/trait.lfy", LIBRARY_TRAIT)
                .write("lib/criteria/main.lfy", LIBRARY_CRITERIA)
                .write("lib/target/main.lfy", LIBRARY_TARGET)
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

    /// The main file of the package `elfie`: the prelude, which brings the vocabulary a
    /// target is declared with into the scope of every file of the project.
    const LIBRARY: &str = "use \"./prelude/builtin\";\nuse \"./prelude/entity\";\n\
                           use \"./prelude/trait\";\nuse \"./criteria/main\";\n\
                           use \"./target/main\";\n";
    /// The trait a builtin carries, which the library's own declarations need.
    const LIBRARY_BUILTIN: &str = "trait builtin: `Performed by the compiler` { }\n";
    /// The context layer every entity is seen through: the lists a body adds to live
    /// there, and `apply` is a member of a trait's kind rather than a name nothing
    /// declares.
    const LIBRARY_ENTITY: &str = "\
d Entity: `A declared thing seen through its context layer` {
  $knowledge: `What the compiler is given to read for it` = object[];
  $commands: `Shell commands for its operations` = object[];
  $targets: `The targets it is built for` = object[];
}
d Trait extends Entity: `A trait seen through its context layer` {
  $apply: `Applies the trait to a target and returns the trait` = (target: Entity) => Trait;
  $layer: `The trait as a layer of a target, with arguments for its parameters` = (...arguments: string[]) => object;
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
    /// `Target`, the roles its slots take, and the function that gives a record written
    /// in braces its type, as the package `elfie` declares them.
    const LIBRARY_TARGET: &str = "\
use \"../prelude/builtin\";
trait target: `What every layer of a target is` {
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

    /// The language, the layout, and the ecosystem the fixture's target is built from.
    /// The language carries the one criterion a fixture's guidance is measured by.
    const ROLES: &str = "\
trait lang extends targetLanguage: `Rust` {
  .fileExtension = \"rs\";
  .markerComment = \"//\";
  @acceptanceCriteria.add({ behavior = `Each unit becomes one module named after its stem` });
}
trait flat extends targetLayout: `One file per unit` { }
trait cargo extends targetEcosystem: `Crates from crates.io` {
  .ecosystem = \"cargo\";
}
";

    /// The `ace const` holding the target `rust`, added to `global` so that every entity
    /// of every project file is built for it.
    const TARGETS: &str = "\
use \"elfie/target\";
use \"./roles\";

ace const rust = targetOf({
  output = \"src\",
  language = lang,
  layout = flat,
  ecosystem = cargo,
  dependencies = [{ name = \"serde_json\", version = \"1\" }, { name = \"sha2\" }],
});
global@targets.add(rust);
";

    const A: &str = "use \"./b\";\n\n/// A record.\nd A: `An a` {\n  $b = B;\n}\n";
    const B: &str = "d B: `A b` {\n  $x = string;\n}\n\nfn make(x: string): `Makes a b` => B {\n  @acceptanceCriteria.add({ situation = `x is empty`, behavior = `the b holds x` });\n  @test({ input = [\"y\"], expect = B@like(`holding y`) });\n}\n";

    /// The same target declaration with nothing added to `global`, so that only an entity
    /// or a file that holds the target itself is built for it.
    fn without_global(targets: &str) -> String {
        targets.replace("global@targets.add(rust);\n", "")
    }

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
                    answered: answered_ids(program, unit),
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
        let declaration = workspace.targets[0].declaration;
        assert!(
            workspace.model.entities[workspace.model.global]
                .targets
                .contains(&declaration),
            "global@targets.add(rust) must add the target to global"
        );
    }

    // -----------------------------------------------------------------------------------
    // interfaceOf
    // -----------------------------------------------------------------------------------

    // @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:df415af10d739698174e3358d1293c5c82973190a3202adf8f50300ab0f4f3d5
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
        // @lfy def/generation/main.lfy:interfaceOf
        assert_eq!(lines[1], "  - `x` (member) — type `string`");
        assert!(
            lines[2].starts_with("- `make` (agent function: AgentFunctionDeclaration): Makes a b"),
            "{text}"
        );
        // @lfy def/generation/main.lfy:interfaceOf
        assert!(lines[2].contains("— parameters (x: string)"), "{text}");
        assert!(lines[2].contains("— output `B`"), "{text}");
        // Nothing in an interface is a line number, so nothing moves with the source.
        assert!(!text.contains(":1"), "{text}");
    }

    // @lfy def/generation/main.lfy:interfaceOf
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
    // @lfy def/generation/main.lfy:interfaceOf
    // @lfy def/generation/data.lfy:SourceMap#SourceMap:SourceMap:38d2eb41999723e89789b375808876aeefd6bae2d41a4f023a97144d33d882a4
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

    /// Every type an interface line spells is spelled by the one speller of a type as Elfie
    /// source, so a type written with arguments keeps them and a type that declares no name
    /// reads as it was written rather than as the word `anonymous`.
    // @lfy def/generation/data.lfy:SourceMap#SourceMap:SourceMap:fd62af8a8ab692d82a9ff1cd315fe1ba7411cb837421fff2038240e570114c59
    #[test]
    fn an_interface_spells_every_type_as_elfie_source() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d Box<T>: `A box` {\n  $item: `What it holds` = T;\n}\n\nd C: `A c` {\n  $boxed: `A box of strings` = Box<string>;\n  $many: `Boxes` = Box<string>[];\n}\n",
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let text = interface_of(&workspace, a);
        // A type written with arguments keeps them, so a dependent is told what it holds
        // and a change to the arguments alone changes the signature.
        assert!(
            text.contains("  - `boxed` (member): A box of strings — type `Box<string>`"),
            "{text}"
        );
        assert!(
            text.contains("  - `many` (member): Boxes — type `Box<string>[]`"),
            "{text}"
        );
        // Every type of every line is what the speller gives for it, and nothing the
        // interface spells itself.
        let model = &workspace.model;
        for &entity in &a.entities {
            for member in members_of(model, entity) {
                let Some(spelled) = model::type_text_of(model, model.entities[member].ty.as_ref())
                else {
                    continue;
                };
                assert!(
                    text.contains(&format!("— type `{spelled}`")),
                    "{spelled} is missing:\n{text}"
                );
            }
        }
        assert!(!text.contains("anonymous"), "{text}");
    }

    // -----------------------------------------------------------------------------------
    // plan
    // -----------------------------------------------------------------------------------

    /// The program the plan is for is the workspace lowered: `plan` lowers it itself and
    /// gives that program back beside the plan, and every unit names its lowered file by
    /// its place in it.
    // @lfy def/generation/main.lfy:plan#plan:plan:3303e4532dda74155594c480805a56fc6c02f04473a087e4e299c538bd992142
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
        // @lfy def/generation/main.lfy:plan#plan:plan:f1d41358bbe62b7bc098a9056623218e8f88baf629ec9d4f2caab39e89f37bfc
        assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
        for unit in &plan.units {
            assert_eq!(program.files[unit.lowered].file, unit.file);
        }
    }

    /// The standard library is compiled by its own project, so none of its entities is
    /// ever built, even though `global` holds the target among its targets; a unit's
    /// interface may still name them as types.
    // @lfy def/generation/main.lfy:plan#plan:plan:0ef10d0db096eac5dd73e22dd85ec6f629b3c05181b46ac57a218cc07d64b221
    #[test]
    fn the_elfie_package_gives_no_unit_even_though_global_holds_the_target() {
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
        // Neither `A` nor its file holds a target, so `global`'s targets decide, and every
        // entity the project's own file declares in its file scope is built.
        // @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
        assert_eq!(names(&workspace, &plan.units[0].entities), ["A"]);
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:d6ca768a511d5fdd650be5dc4cc490799bb13a803820c6b95c50f95907aa001a
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
        // @lfy def/generation/main.lfy:plan#plan:plan:c6bed32df9850fc2409ccc08a49904114e67a864cad86e4c61b6b0addd1d0dab
        assert!(b_index < a_index);
        // The file `a` uses has a unit of the same target, so that unit is its dependency.
        // @lfy def/generation/main.lfy:plan
        assert_eq!(a.dependencies, [b_index]);
        assert!(b.dependencies.is_empty());
        // Neither unit has an output recorded, so each is fresh.
        // @lfy def/generation/main.lfy:plan#plan:plan:01751bca4d2165839842ab83d64cf2cab13330a74b283b7c2cf83fd8513371c3
        assert_eq!(a.reason, Some(Reason::Fresh));
        assert_eq!(b.reason, Some(Reason::Fresh));
        assert!(a.outputs.is_empty());
        // One unit per file, holding the entities built for the target in file order.
        // @lfy def/generation/main.lfy:plan#plan:plan:913b20182f0e8f00f27782535dbbf5d543af1d94b5560188f87084191178b772
        assert_eq!(names(&workspace, &a.entities), ["A"]);
        assert_eq!(names(&workspace, &b.entities), ["B", "make"]);
        // @lfy def/generation/main.lfy:plan
        assert_eq!(a.stem, "a");
        assert_eq!(b.stem, "b");
        assert_eq!(a.target, 0);
        assert_eq!(a.file, file_index(&workspace, "def/a.lfy"));
        // The batches partition the planned units, in `Plan.units` order.
        // @lfy def/generation/main.lfy:plan#plan:plan:88eb512d83f4888e4edcac53d09e9ed1d190eb09ce99803fb65ec6e80776586c
        assert_eq!(plan.planned().count(), 2);
        assert_eq!(plan.batches[0].units, [b_index, a_index]);
        // One batch, named after the first unit and the count of the others.
        // @lfy def/generation/main.lfy:plan
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
        // @lfy def/generation/main.lfy:plan#plan:plan:40f45c8240354c40ed6f2a0ef91f2c24787a209455f9f1a93d9e45ae9c1b5dd7
        assert!(
            plan.units.iter().all(|unit| unit.reason.is_none()),
            "{:?}",
            plan.units
        );
        // @lfy def/generation/main.lfy:plan#plan:plan:84b9ddea7d9b6f73bdde004e1e9dee2c746f57ff862c8302fd0a3150a4df7817
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(a.outputs.len(), 1);
        assert_eq!(a.outputs[0].source, "def/a.lfy");
        assert_eq!(a.outputs[0].target, "rust");
        assert_eq!(plan.planned().count(), 0);
        // Nothing is planned, so there is no batch.
        // @lfy def/generation/main.lfy:plan
        assert!(plan.batches.is_empty());
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:da57a14227f845b6550a85e82efd1e2eee97963403833ef1c1a4896da8e0a6c6
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
        // @lfy def/generation/main.lfy:plan#plan:plan:699a0b8df170aed10dd0c3f2cdf3492c7eff895d33b7e542b8928c357895c458
        assert_eq!(b.reason, Some(Reason::Changed));
        // b is planned only for itself: its interface is the same.
        // @lfy def/generation/main.lfy:plan#plan:plan:40f45c8240354c40ed6f2a0ef91f2c24787a209455f9f1a93d9e45ae9c1b5dd7
        assert_eq!(a.reason, None);
        // @lfy def/generation/main.lfy:plan
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
        // @lfy def/generation/main.lfy:plan#plan:plan:772d227fa53dc6d543215df30f2905bc04c628366f07dcbff492bb5ce7333638
        assert_eq!(a.reason, Some(Reason::Dependency));
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].units, [b_index, a_index]);
    }

    /// A target no entity, no file, and not `global` holds among its targets gives no unit.
    // @lfy def/generation/main.lfy:plan#plan:plan:cfd8a78a7ab1104d66bbd7d65ce7bb62066a39ae62f6909834bfde20a5a3d84b
    #[test]
    fn a_target_nothing_names_gives_no_units() {
        let fixture = a_and_b();
        fixture.write("def/targets.lfy", &without_global(TARGETS));
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        assert_eq!(workspace.targets.len(), 1);
        let plan = plan_of(&program, &[], &[], &[]);
        // @lfy def/generation/main.lfy:plan
        assert!(plan.units.is_empty(), "{:?}", plan.units);
        assert!(plan.batches.is_empty());
    }

    /// An entity whose own targets hold the target is built for it, and an entity of the
    /// same file whose own targets are empty is not, because neither its file nor `global`
    /// holds the target either.
    // @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
    #[test]
    fn an_entity_whose_own_targets_hold_the_target_is_built() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/targets.lfy", &without_global(TARGETS))
            .write(
                "def/a.lfy",
                "use \"./targets\";\n\nd Marked {\n  $x = string;\n  @targets.add(rust);\n}\n\
                 d Plain { $y = string; }\n",
            );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(names(&workspace, &plan.units[0].entities), ["Marked"]);
    }

    /// The target added to a file's own entity builds everything that file declares in its
    /// file scope, and nothing of any other file.
    // @lfy def/generation/main.lfy:plan#plan:plan:d0ff84cddcd136c8da02cfa47f06025000486fd0e764ca59f7baba40be4d9c45
    #[test]
    fn the_target_on_a_files_own_entity_builds_every_entity_of_that_file() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/targets.lfy", &without_global(TARGETS))
            .write(
                "def/a.lfy",
                "use \"./targets\";\n@targets.add(rust);\n\nd One { $x = string; }\n\
                 d Two { $y = string; }\n",
            )
            .write("def/bare.lfy", "d Untouched { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let plan = plan_of(&program, &[], &[], &[]);
        // One unit, for the file whose own entity holds the target.
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(
            plan.units[0].file,
            file_index(&workspace, "def/a.lfy"),
            "{:?}",
            plan.units
        );
        assert_eq!(names(&workspace, &plan.units[0].entities), ["One", "Two"]);
        // Only what the file scope declares is built: the members go with the data that
        // holds them.
        // @lfy def/generation/main.lfy:plan#plan:plan:c3b45b9c75fa54e5426018d8de412d9c76d287a296f9384ce03c03717a3ee93c
        assert!(
            !names(&workspace, &plan.units[0].entities)
                .iter()
                .any(|name| name.contains('.')),
            "{:?}",
            plan.units[0].entities
        );
    }

    /// An entity built for a target whose member reaches an entity of a project file that
    /// is not built for it leaves one problem, at that member, naming both and the target.
    // @lfy def/generation/main.lfy:plan#plan:plan:28729b42d15d20d9238283f1a0077d5bcf0dffd32f4bf589ea2c0f639e467065
    #[test]
    fn an_entity_reaching_one_not_built_for_its_target_leaves_a_problem_at_the_member() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/targets.lfy", &without_global(TARGETS))
            .write(
                "def/a.lfy",
                "use \"./targets\";\nuse \"./b\";\n\nd A {\n  $b = B;\n  @targets.add(rust);\n}\n",
            )
            .write("def/b.lfy", "d B { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let plan = plan_of(&program, &[], &[], &[]);
        // Only `def/a.lfy` gives a unit, and the `B` its member names is in no unit.
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(plan.problems.len(), 1, "{:?}", plan.problems);
        let problem = &plan.problems[0];
        for named in ["A.b", "B", "rust"] {
            assert!(problem.message.contains(named), "{problem:?}");
        }
        // It sits at the member whose type reaches it.
        // @lfy def/generation/main.lfy:plan
        let member = workspace
            .model
            .entities
            .iter()
            .position(|entity| {
                entity.node == Some(problem.node) && entity.kind == EntityKind::Member
            })
            .expect("the problem sits at a member");
        assert_eq!(
            workspace.model.entities[member].identifier.as_deref(),
            Some("b")
        );
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:0ef10d0db096eac5dd73e22dd85ec6f629b3c05181b46ac57a218cc07d64b221
    #[test]
    fn a_package_file_gives_no_unit_and_a_nested_file_keeps_its_directory_in_its_stem() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/deep/inner.lfy", "d Inner { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        // The library declares data `global` holds the target for, but has a package.
        assert!(workspace.file("lib/target/main.lfy").unwrap().package.is_some());
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

    // @lfy def/generation/main.lfy:plan#plan:plan:c6bed32df9850fc2409ccc08a49904114e67a864cad86e4c61b6b0addd1d0dab
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
        // @lfy def/generation/main.lfy:plan
        assert_eq!(a.dependencies, [d_index, b_index]);
        // @lfy def/generation/main.lfy:plan
        assert!(a_index > b_index && a_index > d_index);
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:9e6d3d70db8eebce09554eb34d8e0c45134ec1eba5d58f6da4e57b8a378c0f4e
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
        assert_eq!(plan.problems, []);
        assert_eq!(plan.moves, []);
        assert!(
            plan.units
                .iter()
                .all(|unit| unit.reason == Some(Reason::Fresh))
        );
    }

    /// Two files that use each other and whose entities reach each other too: neither is the
    /// lowest file, since each uses the other, so the cycle is the uses' and nothing moves.
    // @lfy def/generation/main.lfy:plan#plan:plan:9e6d3d70db8eebce09554eb34d8e0c45134ec1eba5d58f6da4e57b8a378c0f4e
    #[test]
    fn a_cycle_among_uses_whose_entities_reach_each_other_moves_nothing() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/a.lfy", "use \"./b\";\n\nd A {\n  $b = B;\n}\n")
            .write("def/b.lfy", "use \"./a\";\n\nd B {\n  $a = A;\n}\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
        // Each file uses the other, so neither can be the lowest file of the cycle.
        // @lfy def/generation/main.lfy:plan#plan:plan:e32007c54b7c28a4e6102a813e4c61dc2c7f36b487af8e3f5c778220338975cf
        assert!(used_by(&workspace, a.file, b.file));
        assert!(used_by(&workspace, b.file, a.file));
        assert_eq!(lowest_file(&workspace, &[a.file, b.file], &[0, 1]), None);
        // Nothing of the cycle moves, and the plan adds no problem of its own.
        assert_eq!(plan.moves, []);
        assert_eq!(plan.problems, []);
        assert_eq!(names(&workspace, &plan.units[a_index].entities), ["A"]);
        assert_eq!(names(&workspace, &plan.units[b_index].entities), ["B"]);
    }

    /// `def/b.lfy` declares `B`; `def/a.lfy` uses it and declares `A`, whose member is a
    /// `B`, and `C`; `def/c.lfy` applies to `B` a trait adding a member of type `C`. So `A`
    /// reaches `B` and `B` reaches `C`, and the two units reach each other. `def/b.lfy` is
    /// the lowest file of the cycle, since `def/a.lfy` uses it and it uses nothing, so `C`
    /// moves into its unit.
    fn a_cycle() -> Fixture {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/b.lfy", "d B: `A b` {\n  $x = string;\n}\n")
            .write(
                "def/a.lfy",
                "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nd C {\n  $x = string;\n}\n",
            )
            .write(
                "def/c.lfy",
                "use \"./a\";\nuse \"./b\";\n\ntrait holder {\n  $c = C;\n}\nholder.apply(B);\n",
            );
        fixture
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:68e2e70ecc72d488671e852fa7cfa8d7f7c969e753b15735cc176d7a82ed8849
    // @lfy def/generation/main.lfy:plan#plan:plan:5ea9c1a46ff002cb966b639dd7fa5752f17660d0d99bf21d939baffb3c345b31
    // @lfy def/generation/main.lfy:plan#plan:plan:e32007c54b7c28a4e6102a813e4c61dc2c7f36b487af8e3f5c778220338975cf
    // @lfy def/generation/main.lfy:plan#plan:plan:2057c107f5f4608c255f6b82583d495c26804ab224b9335dff27ab7873a8f952
    // @lfy def/generation/main.lfy:plan#plan:plan:69e49301647aab00316cdcd47da5c0e5daf7b6857c4849316ed9ed3501ec71bb
    #[test]
    fn a_cycle_moves_what_the_lowest_file_reaches_into_the_lowest_files_unit() {
        let fixture = a_cycle();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");

        // One move, of `C` from the unit of `def/a.lfy` into the unit of `def/b.lfy`, caused
        // by the member the trait added to `B`.
        // @lfy def/generation/main.lfy:plan
        assert_eq!(plan.problems, []);
        assert_eq!(plan.moves.len(), 1, "{:?}", plan.moves);
        let moved = &plan.moves[0];
        assert_eq!(names(&workspace, &moved.entities), ["C"]);
        assert_eq!((moved.origin, moved.into), (a_index, b_index));
        assert_eq!(moved.cause, "B.c");

        // The moved entity is of the unit it moved into and of no other, its own file order
        // first and the moved entity after it.
        // @lfy def/generation/main.lfy:plan#plan:plan:d516ff06fa2ea2c1732f166f5a912d694fba72ac3eede32d49c7ac20123fe48d
        assert_eq!(names(&workspace, &b.entities), ["B", "C"]);
        assert_eq!(names(&workspace, &a.entities), ["A"]);
        // The unit it left depends on the unit it moved into, and the cycle is gone.
        // @lfy def/generation/main.lfy:plan#plan:plan:4a0cb7f3860e04fe587c52b0ce8e9b92e1275445a3ca0b51fd06d8778b47206e
        assert_eq!(a.dependencies, [b_index]);
        assert!(b.dependencies.is_empty(), "{:?}", b.dependencies);
        assert!(b_index < a_index);
        assert!(
            plan.units
                .iter()
                .all(|unit| unit.reason == Some(Reason::Fresh))
        );

        // The text each unit compiles follows its entities: `C` is in the text of the unit it
        // moved into and in no other. @lfy def/generation/main.lfy:plan
        assert!(b.text.contains("d C"), "{}", b.text);
        assert!(!a.text.contains("d C"), "{}", a.text);
        assert!(a.text.contains("d A"), "{}", a.text);
    }

    /// `def/a.lfy` and `def/b.lfy` reach each other through members a trait of `def/t.lfy`
    /// added, and neither uses the other, so no file of the cycle is used by every other.
    // @lfy def/generation/main.lfy:plan#plan:plan:b4923bbd4e807e07a830205e8adc5377b41e10ed65314c2ba71fb0cc2ecf3740
    // @lfy def/generation/main.lfy:plan#plan:plan:fa7d90dd8bdab380388515f7c40bb7848f1274eb5d75942a04288b75a656c7bf
    #[test]
    fn a_cycle_with_no_lowest_file_leaves_one_problem_and_moves_nothing() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/a.lfy", "d A {\n  $x = string;\n}\n")
            .write("def/b.lfy", "d B {\n  $x = string;\n}\n")
            .write(
                "def/t.lfy",
                "use \"./a\";\nuse \"./b\";\n\ntrait toB {\n  $b = B;\n}\n\ntrait toA {\n  $a = A;\n}\ntoB.apply(A);\ntoA.apply(B);\n",
            );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
        // Each reaches the other, and neither file uses the other.
        assert!(a.dependencies.contains(&b_index));
        assert!(b.dependencies.contains(&a_index));
        assert!(!used_by(&workspace, a.file, b.file));
        assert!(!used_by(&workspace, b.file, a.file));

        // Nothing moves, and one problem names every file of the cycle.
        // @lfy def/generation/main.lfy:plan
        assert_eq!(plan.moves, []);
        assert_eq!(names(&workspace, &plan.units[a_index].entities), ["A"]);
        assert_eq!(names(&workspace, &plan.units[b_index].entities), ["B"]);
        assert_eq!(plan.problems.len(), 1, "{:?}", plan.problems);
        let problem = &plan.problems[0];
        assert!(problem.message.contains("def/a.lfy"), "{problem:?}");
        assert!(problem.message.contains("def/b.lfy"), "{problem:?}");
        // It sits at the member of `A` whose type reaches `B`: the first declaration of the
        // cycle, in file order and then source order, that reaches across it.
        // @lfy def/generation/main.lfy:plan
        let member = workspace
            .model
            .entities
            .iter()
            .position(|entity| {
                entity.node == Some(problem.node) && entity.kind == EntityKind::Member
            })
            .expect("the problem sits at a member");
        assert_eq!(
            workspace.model.entities[member].identifier.as_deref(),
            Some("b")
        );
    }

    /// Two cycles are resolved in two rounds, each round reading the dependencies the last
    /// round left; afterwards no two units of a target reach each other.
    // @lfy def/generation/main.lfy:plan#plan:plan:b83fb4e8773d4d2bc8f168802a062c5f8d414beaa418defd0430f736e6928053
    #[test]
    fn moves_repeat_until_no_cycle_is_left() {
        let fixture = Fixture::with_rust_target();
        for pair in ["1", "2"] {
            fixture
                .write(
                    &format!("def/b{pair}.lfy"),
                    &format!("d B{pair} {{\n  $x = string;\n}}\n"),
                )
                .write(
                    &format!("def/a{pair}.lfy"),
                    &format!(
                        "use \"./b{pair}\";\n\nd A{pair} {{\n  $b = B{pair};\n}}\n\nd C{pair} {{\n  $x = string;\n}}\n"
                    ),
                );
        }
        fixture.write(
            "def/t.lfy",
            "use \"./a1\";\nuse \"./b1\";\nuse \"./a2\";\nuse \"./b2\";\n\ntrait h1 {\n  $c = C1;\n}\n\ntrait h2 {\n  $c = C2;\n}\nh1.apply(B1);\nh2.apply(B2);\n",
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.problems, []);
        assert_eq!(plan.moves.len(), 2, "{:?}", plan.moves);
        for pair in ["1", "2"] {
            let (_, b) = unit_of(&workspace, &plan, &format!("def/b{pair}.lfy"));
            let (_, a) = unit_of(&workspace, &plan, &format!("def/a{pair}.lfy"));
            assert_eq!(
                names(&workspace, &b.entities),
                [format!("B{pair}"), format!("C{pair}")]
            );
            assert_eq!(names(&workspace, &a.entities), [format!("A{pair}")]);
        }
        // Nothing is left that depends on something that depends on it.
        // @lfy def/generation/main.lfy:plan
        let dependencies: Vec<Vec<usize>> = plan
            .units
            .iter()
            .map(|unit| unit.dependencies.clone())
            .collect();
        assert_eq!(cycles(&dependencies), Vec::<Vec<usize>>::new());
    }

    /// Reaching is read from the definitions alone, so an entity built for two targets moves
    /// the same way for both.
    // @lfy def/generation/main.lfy:plan#plan:plan:2aa0a55e70b64bf9e37087a9332dce7e87a8a1376bb6744dab9b2ce7c058717a
    #[test]
    fn every_move_is_the_same_for_every_target() {
        let fixture = a_cycle();
        fixture.write(
            "def/targets.lfy",
            &format!(
                "{}\nace const other = targetOf({{ output = \"other\", language = lang, \
                 layout = flat }});\nglobal@targets.add(other);\n",
                TARGETS.trim_end()
            ),
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_eq!(workspace.targets.len(), 2, "{:?}", workspace.problems);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 4, "{:?}", plan.units);
        // One move per target, both of `C` and both into the unit of `def/b.lfy`.
        // @lfy def/generation/main.lfy:plan
        assert_eq!(plan.moves.len(), 2, "{:?}", plan.moves);
        let moved: Vec<(Vec<String>, String, String)> = plan
            .moves
            .iter()
            .map(|moved| {
                (
                    names(&workspace, &moved.entities),
                    workspace.files[plan.units[moved.into].file].path.clone(),
                    moved.cause.clone(),
                )
            })
            .collect();
        assert_eq!(moved[0], moved[1], "{moved:?}");
        assert_eq!(moved[0].1, "def/b.lfy");
        // The cycle is resolved for both, so neither target leaves a problem, and the one
        // cycle is never reported twice. @lfy def/generation/main.lfy:plan
        assert_eq!(plan.problems, []);
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:68cc4a6ef4fd0e7da9d24e21b1ef05a1e1d173ad7313adf6c3971c21bd01bd3c
    // @lfy def/generation/main.lfy:plan#plan:plan:709c271f920bd31e9716cfbd10c64489c0dfa7e2944a2e80c900ba79a40a3d3c
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
        // A path names a unit as well as a stem, and the one requested unit is the one
        // batch, named after its stem alone.
        // @lfy def/generation/main.lfy:plan
        let plan = plan_of(&program, &maps, &["def/a.lfy".to_string()], &[]);
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(a.reason, Some(Reason::Requested));
        assert_eq!(b.reason, None);
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].identifier, "a");
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

    // @lfy def/generation/main.lfy:plan#plan:plan:661840864856b01f6c2f306f700e65aae858a009d8c010124a47b5fd9885985f
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
        assert!(
            plan.units
                .iter()
                .all(|unit| unit.reason == Some(Reason::Fresh))
        );
        let sizes: Vec<usize> = plan.batches.iter().map(|batch| batch.units.len()).collect();
        // Six of `deep`, then the seventh, and `top` on its own: no batch holds more than
        // six units, and none mixes directories.
        // @lfy def/generation/main.lfy:plan#plan:plan:46d51e256ac0ed89b3413a7e235770719a0f216b2b18d7b133e7e05e6d930718
        assert_eq!(sizes.iter().sum::<usize>(), 8, "{:?}", plan.batches);
        assert_eq!(sizes, [BATCH_UNITS, 1, 1], "{:?}", plan.batches);
        assert_eq!(plan.batches[0].identifier, "deep/u0+5");
        assert_eq!(plan.batches[1].identifier, "deep/u6");
        assert_eq!(plan.batches[2].identifier, "top");
        // A unit joins the open batch only while it shares the first segment of its stem and
        // the batch holds fewer than six units.
        // @lfy def/generation/main.lfy:plan#plan:plan:f5945b26c42abf21599b311c49e8448eb81d2fad73e72ff70a5affec2d2f63c7
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

    /// Two units of one directory whose sources together reach the limit go in two batches,
    /// though neither the count of units nor the directory would have parted them.
    // @lfy def/generation/main.lfy:plan#plan:plan:8b23e12f7a1211640db7cdf6e25859876cd4e9df01a21c0766522b1249acc498
    // @lfy def/generation/main.lfy:plan#plan:plan:b5a7643a3fbd7949303b346993c7bec289b1766eb30f651e165467197f2936b0
    #[test]
    fn two_units_whose_sources_together_reach_the_limit_go_in_two_batches() {
        let fixture = Fixture::with_rust_target();
        // Each file lowers to more than half the limit and less than the whole of it, so
        // either on its own fits a batch and the two together do not.
        let members: String = (0..1_200)
            .map(|index| format!("  $m{index}: `A member of its own name` = string;\n"))
            .collect();
        for name in ["a", "b"] {
            fixture.write(
                &format!("def/x/{name}.lfy"),
                &format!("d {}: `A big one` {{\n{members}}}\n", name.to_uppercase()),
            );
        }
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
        let (_, a) = unit_of(&workspace, &plan, "def/x/a.lfy");
        let (_, b) = unit_of(&workspace, &plan, "def/x/b.lfy");
        for unit in [a, b] {
            let characters = unit.text.chars().count();
            assert!(
                characters < BATCH_CHARACTERS && characters * 2 >= BATCH_CHARACTERS,
                "{characters}"
            );
            assert_eq!(unit.reason, Some(Reason::Fresh));
        }
        // They share the first segment of their stem and would be four units short of the
        // count, so only the size parts them: one batch each, named after its own unit.
        // @lfy def/generation/main.lfy:plan
        assert_eq!(first_segment(&a.stem), first_segment(&b.stem));
        assert_eq!(plan.batches.len(), 2, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].identifier, "x/a");
        assert_eq!(plan.batches[1].identifier, "x/b");
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:83ae85d53a06f4a097fb8db49ddcb5211746d5285060c427ab68e0f4d46b4b30
    // @lfy def/generation/main.lfy:plan#plan:plan:f5f747818d71fc77f0063fbd3976422979d7df380c43c8c14e716c5beb4baa4e
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

    /// A request carries one target's guidance and one set of native dependencies, so units
    /// of two targets never share a batch however well their stems and sizes would fit.
    // @lfy def/generation/main.lfy:plan#plan:plan:139ff97944391c8eb7f3b810897377a8bd64ea54d0fe64b6c29a7043348a5379
    #[test]
    fn a_unit_of_another_target_opens_a_new_batch() {
        let fixture = a_and_b();
        fixture.write(
            "def/targets.lfy",
            &format!(
                "{}\nace const other = targetOf({{ output = \"other\", language = lang, \
                 layout = flat }});\nglobal@targets.add(other);\n",
                TARGETS.trim_end()
            ),
        );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_eq!(workspace.targets.len(), 2, "{:?}", workspace.problems);
        let plan = plan_of(&program, &[], &[], &[]);
        // Two files for each of the two targets, every one of them fresh.
        assert_eq!(plan.units.len(), 4, "{:?}", plan.units);
        assert!(plan.units.iter().all(|unit| unit.reason == Some(Reason::Fresh)));
        // Both files sit directly under the source directory, so every unit shares the first
        // segment of its stem with every other and only the target parts them: one batch per
        // target, each holding that target's two units.
        assert_eq!(plan.batches.len(), 2, "{:?}", plan.batches);
        for batch in &plan.batches {
            let target = plan.units[batch.units[0]].target;
            assert_eq!(batch.units.len(), 2, "{:?}", plan.batches);
            for &index in &batch.units {
                assert_eq!(plan.units[index].target, target, "{:?}", plan.batches);
            }
        }
        assert_ne!(
            plan.units[plan.batches[0].units[0]].target,
            plan.units[plan.batches[1].units[0]].target
        );
    }

    // @lfy def/generation/main.lfy:plan#plan:plan:88eb512d83f4888e4edcac53d09e9ed1d190eb09ce99803fb65ec6e80776586c
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
    // @lfy def/generation/main.lfy:plan#plan:plan:913b20182f0e8f00f27782535dbbf5d543af1d94b5560188f87084191178b772
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
    // @lfy def/generation/main.lfy:plan#plan:plan:43f3dac0dd4bb25573f9bff0c0f81d674671504e8ac666c8ae2bf37faf2d75f8
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
        // @lfy def/generation/main.lfy:plan#plan:plan:7725d70d35f6e8be6804e106099b40d9930e162127a1e2c046e3b3a2ec0ff846
        assert_eq!(b.reason, Some(Reason::Requirements));
        assert_eq!(a.reason, None);
        let map = maps.iter().find(|map| map.source == "def/b.lfy").unwrap();
        assert_eq!(map.hash, source_hash(&program.files[b.lowered].text));
        assert_ne!(map.requirements, requirements_hash(&program, b));
        // The unit's own section says its code is unchanged and that its criteria or tests
        // differ. @lfy def/generation/main.lfy:request
        let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new());
        assert!(
            request
                .instructions
                .contains("The lowered code of this unit is unchanged"),
            "{}",
            request.instructions
        );
    }

    /// A unit's local criteria and tests are those of each of its entities and of its file's
    /// own entity, so moving an entity into another unit moves them with it: the receiving
    /// unit holds them and the unit the entity left holds them no longer.
    // @lfy def/generation/data.lfy:Unit#Unit:Unit:e5292928d76df9bc73129964e9a39f6ef14fe33c190526dc4bb3f76ff2380810
    #[test]
    fn a_units_requirements_are_its_entities_and_move_with_them() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        let plan = plan_of(&program, &[], &[], &[]);
        let ids = |unit: &Unit| {
            unit.requirements(&program)
                .iter()
                .map(|requirement| requirement.id().to_string())
                .collect::<Vec<String>>()
        };
        let (a_index, a_unit) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, b_unit) = unit_of(&workspace, &plan, "def/b.lfy");

        // Of the entities of `def/b.lfy`, only `make` carries anything: one criterion and
        // one test. `def/a.lfy` declares `A` alone, which carries neither.
        let b_ids = ids(b_unit);
        assert_eq!(b_ids.len(), 2);
        assert!(
            b_ids.iter().all(|id| id.starts_with("make:make:")),
            "{b_ids:?}"
        );
        assert!(ids(a_unit).is_empty());

        // `make` moved into the unit of `def/a.lfy`, as a cycle among the two files would
        // move it: its criterion and its test are then that unit's, though its declaration
        // is still in the file it came from, and the unit it left holds neither.
        let make = *b_unit
            .entities
            .iter()
            .find(|&&entity| entity_name(&workspace.model, entity) == "make")
            .expect("make is an entity of def/b.lfy");
        let mut moved = plan.clone();
        moved.units[a_index].entities.push(make);
        moved.units[b_index]
            .entities
            .retain(|&entity| entity != make);
        moved.moves.push(Move {
            entities: vec![make],
            origin: b_index,
            into: a_index,
            cause: "A.b".to_string(),
        });
        assert_eq!(ids(&moved.units[a_index]), b_ids);
        assert!(ids(&moved.units[b_index]).is_empty());
    }

    /// A stem the last global review found violated plans its unit again, and a request
    /// still comes first.
    // @lfy def/generation/main.lfy:plan#plan:plan:7693e6aca754c3b43b49e3c98a9f0532e6853bec3b10d21a9f22fccc00b021ce
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
    // @lfy def/generation/main.lfy:plan
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
        // @lfy def/generation/main.lfy:plan#plan:plan:40f45c8240354c40ed6f2a0ef91f2c24787a209455f9f1a93d9e45ae9c1b5dd7
        let plan = plan_of(&program, &maps, &[], &[]);
        assert!(
            plan.units.iter().all(|unit| unit.reason.is_none()),
            "{:?}",
            plan.units
        );
        assert!(plan.batches.is_empty(), "{:?}", plan.batches);
        // The global review found it violated in `b`, so `b` alone is planned again.
        // @lfy def/generation/main.lfy:plan#plan:plan:d02d6ed2f21e62878f97bfce1df0638b8152e90e9d1fbd87429ffad5d31a7be5
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
    // @lfy def/generation/main.lfy:request#request:request:571bc891f6509a3d1997b846f685a316e34804763d8271373888f530426558b3
    // @lfy def/generation/main.lfy:request#request:request:800766e448bfb8ed73777735dcb6d1a81a50d15783302ad27486b92c2d669f5e
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
        // @lfy def/generation/main.lfy:request#request:request:f3979f1497ff9e1b60be372fddce29ffcfe830689ef46680b690554afd9c7a37
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
        // The declaration has no criterion of its own, the layout and the ecosystem none
        // either, and the language one; `target` is all that they extend, and it has none.
        // @lfy def/generation/main.lfy:request
        assert_eq!(
            request.guidance,
            guidance_for(&workspace.model, &workspace.targets[0])
        );
        assert_eq!(request.guidance.len(), 1);
        // What the target's own dependencies, commands, and knowledge give.
        // @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
        assert_eq!(
            request
                .native_dependencies
                .iter()
                .map(|dependency| dependency.identifier.as_str())
                .collect::<Vec<_>>(),
            ["serde_json", "sha2"]
        );
        assert_eq!(request.native_dependencies, workspace.targets[0].native_dependencies);
        assert_eq!(request.commands, workspace.targets[0].commands);
        assert!(request.knowledge.is_empty(), "{:?}", request.knowledge);

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
        // Markers name entities, never lines, and sit in a line comment beginning with the
        // target's marker comment.
        // @lfy def/generation/main.lfy:request#request:request:671617e25d98e36f8c646b0c76d941034839245f2544e239cb4ede3581bb2f33
        assert!(text.contains("Never write a line number."), "{text}");
        assert!(
            text.contains("a line comment beginning with `//`, then `@lfy`"),
            "{text}"
        );
        assert!(text.contains("`// @lfy def/b.lfy:B`"), "{text}");
        // The three report lines.
        // @lfy def/generation/main.lfy:request
        assert!(text.contains("`ELFIE: DONE`"), "{text}");
        assert!(text.contains("`ELFIE: BLOCKED: `"), "{text}");
        assert!(text.contains("`ELFIE: CLARIFY: `"), "{text}");
        // The tools of the agent server are named, and are called before files are read by
        // hand.
        // @lfy def/generation/main.lfy:request
        for tool in [
            "before files are searched or read by hand",
            "`elfie_entity`",
            "`elfie_find`",
            "`elfie_references`",
            "`elfie_outline`",
            "`elfie_output`",
            "`elfie_source`",
            "`elfie_changes`",
            "`elfie_check`",
        ] {
            assert!(text.contains(tool), "{tool} is missing:\n{text}");
        }
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

        // The commands run with `sh -c` in the output directory, and a dependency is added
        // only by running the add command.
        // @lfy def/generation/main.lfy:request#request:request:09b0190ee8969e01b9ff7a561a98c3663f87fb8b97fa83c588d0631e7989bc20
        assert!(text.contains("These run with `sh -c` in `src`"), "{text}");
        assert!(text.contains("`RUN_SCRIPT`"), "{text}");
        assert!(
            text.contains("`NAME` and `VERSION` set to the dependency's"),
            "{text}"
        );
        assert!(
            text.contains("never by editing a manifest or a lockfile by hand"),
            "{text}"
        );
        assert!(
            text.contains("The build, test, and lint commands are run before the report"),
            "{text}"
        );
        assert!(
            text.contains("The compiler is given nothing of its own to read."),
            "{text}"
        );

        // The sections come in the definition's order.
        // @lfy def/generation/main.lfy:request#request:request:d650f0548150afb04ee79524e308a0150e9cf8a3f4dcc9ffde74e3d90e7fcada
        let headings = [
            "# Compiling",
            "## Where the outputs go",
            "## Guidance",
            "## Native dependencies",
            "## Knowledge",
            "## Interfaces",
            "## Commands",
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

    // @lfy def/generation/main.lfy:request#request:request:d86c17626a07f5e8ec807bd7740a5f6f8280e6af67444bf1bf5d18744b801d3b
    // @lfy def/generation/main.lfy:request#request:request:72801863f22ee154bf0e22299d1405c97cd6309fc630197aa315508e2f4829f7
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

    /// A target whose declaration is given a criterion by an add call at the top of its
    /// file, whose layout layer and whose language layer each hold criteria, whose layout
    /// layer extends `targetFramework` too and fills the frameworks slot as well, and whose
    /// language is applied with an argument: the guidance is what no layer and no trait a
    /// layer extends gave, then each layer's in guidance order, then those of every trait
    /// the layers extend, nearest first and each trait once, with every template value
    /// rendered from the layer's arguments.
    // @lfy def/generation/main.lfy:request#request:request:da24d79558458ac142aa1e4f6b149223ba5ddfe55377ebb46b2bf0a367536d5f
    #[test]
    fn the_guidance_is_the_declarations_criteria_then_its_layers_then_what_they_extend() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write(
                "lib/target/main.lfy",
                &LIBRARY_TARGET
                    .replace(
                        ".outputExtensions = [];",
                        ".outputExtensions = [];\n  \
                         where (`An output is written`) -> `Its path is under the output directory`;",
                    )
                    .replace(
                        "$fileExtension: `The extension of its files` = string;",
                        "$fileExtension: `The extension of its files` = string;\n  \
                         where (`A d declaration is built`) -> `It becomes a type`;",
                    )
                    .replace(
                        "trait targetLayout extends target: `Where its files go` { }",
                        "trait targetLayout extends target: `Where its files go` {\n  \
                         where (`A unit is written`) -> `Its output is under the output directory`;\n}",
                    ),
            )
            .write(
                "def/roles.lfy",
                "trait rust(edition: string) extends targetLanguage: `Rust` {\n  \
                 .fileExtension = \"rs\";\n  .markerComment = \"//\";\n  \
                 where (`A unit is written`) -> `Each unit becomes one module named after its stem`;\n  \
                 where (`An output is written`) -> `Outputs are built as Rust {{edition}}`;\n}\n\
                 trait flat extends targetLayout, targetFramework: `One file per unit` {\n  \
                 where (`A unit is written`) -> `Its output is its stem and the extension`;\n}\n\
                 trait built extends targetFramework: `Built on something, silently` { }\n",
            )
            .write(
                "def/targets.lfy",
                "use \"elfie/target\";\nuse \"./roles\";\n\n\
                 ace const t = targetOf({ output = \"src\", language = rust.layer(\"2024\"), \
                 frameworks = [flat, built], layout = flat });\n\
                 t@acceptanceCriteria.add({ situation = `An output is written`, \
                 behavior = `Nothing outside the output directory is written` });\n\
                 global@targets.add(t);\n",
            )
            .write("def/a.lfy", "d A { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        // `flat` fills the frameworks slot and the layout slot, so it is one layer, at the
        // first of the two in guidance order: before `built`, which fills the frameworks slot
        // after it. Were `flat` the layout alone, `built` would come first.
        assert_eq!(
            names(&workspace, &workspace.targets[0].layers),
            ["flat", "built", "rust"]
        );
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new());
        let behaviors: Vec<String> = request
            .guidance
            .iter()
            .map(|criterion| criterion.behavior.clone().unwrap_or_default().join(" "))
            .collect();
        // What the file's add call gave first, since no layer and no trait a layer extends
        // gave it; then the layout and the language, in guidance order, with the language's
        // template value rendered from its argument; then the traits they extend, nearest
        // first.
        // @lfy def/generation/main.lfy:request
        assert_eq!(
            behaviors,
            [
                "Nothing outside the output directory is written",
                "Its output is its stem and the extension",
                "Each unit becomes one module named after its stem",
                "Outputs are built as Rust 2024",
                "Its output is under the output directory",
                "It becomes a type",
                "Its path is under the output directory",
            ],
            "{:?}",
            request.guidance
        );
        // `target` is reached through `targetLayout` and through `targetLanguage`, and
        // contributes once, at its first place.
        // @lfy def/generation/main.lfy:request
        assert_eq!(
            behaviors
                .iter()
                .filter(|behavior| *behavior == "Its path is under the output directory")
                .count(),
            1,
            "{behaviors:?}"
        );
        // `flat` fills two slots, and contributes once as well.
        // @lfy def/generation/main.lfy:request
        assert_eq!(
            behaviors
                .iter()
                .filter(|behavior| *behavior == "Its output is its stem and the extension")
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
        // A layer applied with arguments has its template values rendered from them, so no
        // template is left in the guidance.
        // @lfy def/generation/main.lfy:request
        assert!(!text.contains("{{edition}}"), "{text}");
    }

    /// The knowledge of the target, of the batch's entities, and of the entities of an
    /// interface, each item once: a reference quoted in full, one named to be read, a
    /// definition as the files its use path resolves to, and a tool of the agent server.
    // @lfy def/generation/main.lfy:request#request:request:3d9e3582187fd1f1532a46bf0f938c787311c81feb59f4a7d7d5b0da9a813a9a
    #[test]
    fn the_knowledge_is_quoted_named_resolved_or_called_by_its_kind() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("docs/style.md", "Two spaces, never a tab.\n")
            .write("docs/shape.md", "A long guide nobody quotes.\n")
            .write(
                "def/a.lfy",
                "use \"./b\";\n\nd A {\n  $b = B;\n  @knowledge.add({ topic = `the house style`, \
                 kind = KnowledgeKind.reference, source = \"docs/style.md\", quote = true });\n  \
                 @knowledge.add({ topic = `the shape of a module`, kind = KnowledgeKind.example, \
                 source = \"docs/shape.md\" });\n  @knowledge.add({ topic = `what a b is`, \
                 kind = KnowledgeKind.definition, source = \"./b\" });\n  \
                 @knowledge.add({ topic = `the program`, kind = KnowledgeKind.tool, \
                 source = \"elfie_entity\" });\n}\n",
            )
            .write("def/b.lfy", "d B { $x = string; }\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![a_index],
            identifier: "a".to_string(),
        };
        let request = request(&program, &plan, &batch, &[], &BTreeMap::new());
        // Each item once, in the order the request gathers them.
        // @lfy def/generation/main.lfy:request
        assert_eq!(
            request
                .knowledge
                .iter()
                .map(|item| item.topic.as_str())
                .collect::<Vec<_>>(),
            [
                "the house style",
                "the shape of a module",
                "what a b is",
                "the program"
            ]
        );
        let text = &request.instructions;
        // A reference whose quote is true is quoted in full, from its source joined to the
        // root of the project its contributor is declared in.
        // @lfy def/generation/main.lfy:request#request:request:f64fa963c69a9e30aa2c190c595acdbb6221274d050a5a8533b9cf9291684155
        assert!(text.contains("From `docs/style.md`:"), "{text}");
        assert!(text.contains("Two spaces, never a tab."), "{text}");
        // One whose quote is left out is named, to be read first.
        // @lfy def/generation/main.lfy:request#request:request:737e4198f2f6bddc790512ae099b13648ed59405ba19506fb0abff20748ec530
        assert!(
            text.contains(
                "- the shape of a module: read `docs/shape.md` before writing anything it covers."
            ),
            "{text}"
        );
        assert!(!text.contains("A long guide nobody quotes."), "{text}");
        // A definition names the files its use path resolves to, read as Elfie.
        // @lfy def/generation/main.lfy:request#request:request:179394ea2be57b4a87b41dac2988b30f36b058129a701fc79ff34cc290769606
        assert!(
            text.contains("- what a b is: read `def/b.lfy` as Elfie."),
            "{text}"
        );
        // A tool is called before anything relies on what it answers.
        // @lfy def/generation/main.lfy:request#request:request:18fa49f6ae0fe059f4d8b65429f7356e822363b0bb901d611f266ca1393c940b
        assert!(
            text.contains(
                "- the program: call the tool `elfie_entity` of the agent server before relying on \
                 what it answers."
            ),
            "{text}"
        );
    }

    // @lfy def/generation/main.lfy:request#request:request:43df411fd74fc148bd737fdeb88ec86747b9ad9eb955912e272384f7def7a93f
    // @lfy def/generation/main.lfy:request#request:request:7424b0fcb62916f23bfb6c1265f3a3f943fb1354558e0d4b0ea4722212bc0c7f
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

    /// The section of a unit entities moved into says which they are, from which file, and
    /// because of which cause, and the section of the unit they left says which they are and
    /// which unit compiles them.
    // @lfy def/generation/main.lfy:request#request:request:78a8534645507634a61935dcee1699d73fdc5735bcadda06eb24bf9f9854ea34
    // @lfy def/generation/main.lfy:request#request:request:d8077ccebe94afe4cb2e7e3cf3daafbef0270bf0ca0b6141d42e3be46d002c52
    #[test]
    fn the_section_of_a_unit_says_what_moved_into_it_and_out_of_it() {
        let fixture = a_cycle();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        let request = request(&program, &plan, &plan.batches[0], &[], &BTreeMap::new());
        let text = &request.instructions;

        // In the section of `def/b.lfy`, which `C` moved into.
        // @lfy def/generation/main.lfy:request
        let into = text
            .find("### `def/b.lfy`")
            .expect("a section for def/b.lfy");
        let out = text
            .find("### `def/a.lfy`")
            .expect("a section for def/a.lfy");
        assert!(into < out, "{text}");
        let moved_in = text
            .find("- `C` from `def/a.lfy`, because of `B.c`.")
            .unwrap_or_else(|| panic!("what moved into def/b.lfy's unit: {text}"));
        assert!((into..out).contains(&moved_in), "{text}");
        assert!(
            text.contains("markers naming the file that declares them"),
            "{text}"
        );
        assert!(
            text.contains("never by importing that file"),
            "{text}"
        );

        // In the section of `def/a.lfy`, which `C` moved out of.
        // @lfy def/generation/main.lfy:request
        let moved_out = text
            .find("- `C` in the unit `b` (`def/b.lfy`).")
            .unwrap_or_else(|| panic!("what moved out of def/a.lfy's unit: {text}"));
        assert!(moved_out > out, "{text}");
        assert!(text.contains("never write them again"), "{text}");
        assert!(
            text.contains("under the name and path it would have had"),
            "{text}"
        );

        // The source of each unit is the text it compiles, so `C` is in the source of the
        // unit it moved into and in no other. @lfy def/generation/main.lfy:request
        assert!(request.sources["def/b.lfy"].contains("d C"), "{text}");
        assert!(!request.sources["def/a.lfy"].contains("d C"), "{text}");
    }

    /// An existing output is kept only where its path is among the outputs of a unit of the
    /// batch, and the previous source each was generated from is the file as it was, lowered
    /// the way the unit's own source is, so that the two texts can be read side by side.
    // @lfy def/generation/main.lfy:request#request:request:ce520ffba1e8298b5ff2f6a6376e60e59a17343e640d10ed179732cd788b8075
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
        // The text as written, which the request lowers itself.
        let previous = BTreeMap::from([(
            "def/a.lfy".to_string(),
            "use \"./b\";\n\nd A: `An a` {\n  $b = B;\n}\n".to_string(),
        )]);
        let with_previous = request(&program, &plan,&plan.batches[0], &existing, &previous);
        assert_eq!(with_previous.existing.len(), 1);
        assert_eq!(with_previous.existing["src/a.rs"], "// old");
        // `Request.previous` is the lowered text of the file as it was, never the text as
        // written: it is read against `Request.sources`, which is lowered too.
        // @lfy def/generation/main.lfy:request#request:request:6232735fed99654f7d25efa96687134e92c7667e0be166ba2471ca9d018c3103
        assert_eq!(with_previous.previous.keys().collect::<Vec<_>>(), ["def/a.lfy"]);
        let was = &with_previous.previous["def/a.lfy"];
        assert_ne!(was, &with_previous.sources["def/a.lfy"]);
        // It is what lowering the file as it was gives: here the same file but for the
        // documentation comment the current one carries.
        // @lfy def/generation/main.lfy:request#request:request:e6a9a7f8f68b31456fa6f45f2971df1eddc6eed03b9bcb155872fff58c0ca957
        let lowered = crate::interpret::lower(crate::workspace::change(
            &workspace,
            "def/a.lfy",
            Some(&previous["def/a.lfy"]),
        ));
        let index = lowered
            .workspace
            .files
            .iter()
            .position(|file| file.path == "def/a.lfy")
            .unwrap();
        assert_eq!(was, &lowered.files[index].text);
        assert!(!was.contains("A record."), "{was}");
        assert!(with_previous.sources["def/a.lfy"].contains("A record."));
        let text = &with_previous.instructions;
        // Only what the difference requires changes, and the markers of unchanged items are
        // kept. The outputs part says so, not a part of its own.
        // @lfy def/generation/main.lfy:request#request:request:3d9b718229fc59a2a379fb37aab7b7235c06fee91fa5fc44b6285037204d9a68
        assert!(
            text.contains("Keep the markers of unchanged items."),
            "{text}"
        );
        assert!(text.contains("Keep names and structure."), "{text}");
        assert!(
            text.contains("the criteria and tests marked new or removed"),
            "{text}"
        );
        let said = text
            .find("These outputs exist already:")
            .unwrap_or_else(|| panic!("{text}"));
        assert!(said < text.find("## Guidance").unwrap(), "{text}");
        assert!(said > text.find("## Where the outputs go").unwrap(), "{text}");
        // The outputs of a unit that has some go at the paths its `Unit.outputs` names.
        // @lfy def/generation/main.lfy:request#request:request:28c71f256cbace38745c677d86962737a051927e499b97ac10dc63f2550643be
        assert!(text.contains("write to the same paths"), "{text}");
        assert!(text.contains("- `src/a.rs`"), "{text}");
        assert!(text.contains(was.trim_end()), "{text}");

        // With no previous source, the existing outputs of that unit are reconciled with it.
        // @lfy def/generation/main.lfy:request#request:request:3c1f4cba72285105fde5be93378fbda2d3d93aaf4293fbe643ab3a97d7d1b779
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
                .contains("Reconcile the existing \noutputs of each of them with its source:")
                || without
                    .instructions
                    .contains("Reconcile the existing outputs of each of them with its source:"),
            "{}",
            without.instructions
        );
    }

    /// A unit whose outputs recorded no answered ids leaves the request nothing to say which
    /// of its criteria and tests are new, so the outputs part says to read the existing code
    /// against each of them.
    // @lfy def/generation/main.lfy:request#request:request:a960df7e08bfeb90c2fb135fc71e6e80e01a69f9f351d3d8c7eab08f069407c1
    #[test]
    fn a_unit_whose_outputs_answered_nothing_is_read_against_every_criterion() {
        let fixture = a_and_b();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let mut maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
        for map in &mut maps {
            map.answered.clear();
        }
        let plan = plan_of(&program, &maps, &["b".to_string()], &[]);
        let existing = [Output {
            path: "src/b.rs".to_string(),
            text: "// old".to_string(),
        }];
        let request = request(&program, &plan, &plan.batches[0], &existing, &BTreeMap::new());
        assert!(request.answered.is_empty(), "{:?}", request.answered);
        assert!(
            request
                .instructions
                .contains("Which criteria and tests of these units are new is not known."),
            "{}",
            request.instructions
        );
        assert!(
            request.instructions.contains("- `def/b.lfy`"),
            "{}",
            request.instructions
        );
    }

    /// A unit whose lowered code is unchanged but whose criteria differ: its section says so,
    /// each criterion the outputs did not answer for is marked new, and each one the unit no
    /// longer has is listed under a heading of its own with its text.
    // @lfy def/generation/main.lfy:request#request:request:e7fe398a823291215f9ad291ba742fac6431d4f937badf251af05e9f389ea522
    #[test]
    fn a_unit_planned_for_its_requirements_marks_what_is_new_and_lists_what_is_gone() {
        let fixture = a_and_b();
        // `b` as it was: `make` carried a criterion about an empty `x` and one about a short
        // one. It carries the first and a third now, so the second is gone and the third new.
        let was = B.replace(
            "@test(",
            "@acceptanceCriteria.add({ situation = `x is short`, behavior = `the b is empty` });\n  \
             @test(",
        );
        let now = B.replace(
            "@test(",
            "@acceptanceCriteria.add({ situation = `x is long`, behavior = `the b is cut` });\n  \
             @test(",
        );
        fixture.write("def/b.lfy", &was);
        let old = fixture.program();
        assert_bound(&old.workspace);
        let maps = current_maps(&old, &plan_of(&old, &[], &[], &[]));
        let before: Vec<String> = {
            let plan = plan_of(&old, &[], &[], &[]);
            let (_, unit) = unit_of(&old.workspace, &plan, "def/b.lfy");
            unit.requirements(&old)
                .iter()
                .map(|requirement| requirement.id().to_string())
                .collect()
        };

        fixture.write("def/b.lfy", &now);
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &maps, &[], &[]);
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
        // The lowered code is the same; only the criteria differ.
        assert_eq!(b.reason, Some(Reason::Requirements));
        let existing = [Output {
            path: "src/b.rs".to_string(),
            text: "// old".to_string(),
        }];
        let batch = Batch {
            units: vec![b_index],
            identifier: "b".to_string(),
        };
        let previous = BTreeMap::from([("def/b.lfy".to_string(), was.clone())]);
        let request = request(&program, &plan, &batch, &existing, &previous);

        // What the outputs answered for is what the map file recorded.
        // @lfy def/generation/main.lfy:request#request:request:160056c645e3e69afd7ce0bb74137db9a5e5f286df0a983570f9e2569276b533
        let mut sorted = before.clone();
        sorted.sort();
        assert_eq!(request.answered["def/b.lfy"], sorted);
        // The one criterion the unit no longer has, in the order the file as it was gave it.
        // @lfy def/generation/main.lfy:request#request:request:e9e73367084b12788d91ea49ea57894f06e55c67ebf0856dbb827d3a968fc089
        let gone = &request.removed["def/b.lfy"];
        assert_eq!(gone.len(), 1, "{gone:?}");
        assert!(before.contains(&gone[0]), "{gone:?} {before:?}");

        let text = &request.instructions;
        // The section says the code is unchanged and holds the code to what each criterion
        // now says.
        // @lfy def/generation/main.lfy:request#request:request:dab229dc001954e941ede8f9cf711d6ff5830dd88e08a35f3bccea3da26ad09b
        assert!(
            text.contains("The lowered code of this unit is unchanged, but its criteria or tests \ndiffer.")
                || text.contains(
                    "The lowered code of this unit is unchanged, but its criteria or tests differ."
                ),
            "{text}"
        );
        assert!(
            text.contains("must do what each criterion and test now says"),
            "{text}"
        );
        // The criterion the outputs answered for is unmarked; the one they did not is new.
        // @lfy def/generation/main.lfy:request#request:request:20c8b9f187ae61ac935c6b929921f420eca7b18fd429d76213ce5975b73af64f
        let held = b.requirements(&program);
        let kept = held
            .iter()
            .find(|requirement| before.contains(&requirement.id().to_string()))
            .expect("one criterion or test the outputs answered for");
        let fresh = held
            .iter()
            .find(|requirement| !before.contains(&requirement.id().to_string()))
            .expect("one the outputs did not answer for");
        assert!(
            text.contains(&format!("`{}` — When x is long: the b is cut (new)", fresh.id())),
            "{text}"
        );
        assert!(!text.contains(&format!("`{}` — ", kept.id())) || {
            let line = text
                .lines()
                .find(|line| line.contains(kept.id()))
                .unwrap_or("");
            !line.ends_with("(new)")
        });
        // A new one may restate a removed one in other words.
        // @lfy def/generation/main.lfy:request#request:request:40e519bc14a840e00c6e0fb7bff68febae9b5b591f182632fddc0a29f6f47704
        assert!(
            text.contains("may restate a removed one in other words"),
            "{text}"
        );
        // What is gone is listed last in the unit's section, with its id and its text.
        // @lfy def/generation/main.lfy:request#request:request:85fafd5b7c6201f807d532b93b487771bdd141c7ee6bd8854cdc7d981176ff9a
        let heading = text
            .find("#### Removed since its outputs were accepted")
            .unwrap_or_else(|| panic!("{text}"));
        assert!(
            text[heading..].contains(&format!("`{}` — When x is short: the b is empty", gone[0])),
            "{text}"
        );
        assert!(heading < text.find("## Global criteria and tests").unwrap());
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

    /// An entity moved into a unit is marked with the file that declares it, so a marker
    /// naming that file covers it and the ids such a marker spells are this unit's.
    // @lfy def/generation/main.lfy:accept#accept:accept:2a90cbf7112a647235b9f1c81167c5d44301cbc8369b5d4cc2973be6102fea26
    // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
    #[test]
    fn a_moved_entity_is_marked_with_the_file_that_declares_it() {
        let fixture = a_cycle();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, unit) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(names(&workspace, &unit.entities), ["B", "C"]);
        let batch = Batch {
            units: vec![index],
            identifier: "b".to_string(),
        };
        let request = request(&program, &plan, &batch, &[], &BTreeMap::new());
        let output = Output {
            path: "src/b.rs".to_string(),
            text: "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/a.lfy:C\npub struct C;\n"
                .to_string(),
        };
        let verdict = accept(
            &program,
            &plan,
            &request,
            index,
            std::slice::from_ref(&output),
            &[],
        );
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert_eq!(
            verdict.source_maps[0]
                .markers
                .iter()
                .map(|marker| (marker.file.clone(), marker.entity.clone()))
                .collect::<Vec<_>>(),
            [
                ("def/b.lfy".to_string(), Some("B".to_string())),
                ("def/a.lfy".to_string(), Some("C".to_string())),
            ]
        );

        // The id a marker naming the file of a moved entity spells is checked against this
        // unit's criteria and tests. @lfy def/generation/main.lfy:accept
        let wrong = Output {
            path: "src/b.rs".to_string(),
            text: "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/a.lfy:C#C:C:ffff\npub struct C;\n"
                .to_string(),
        };
        let verdict = accept(&program, &plan, &request, index, &[wrong], &[]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(verdict.problems[0].contains("C:C:ffff"), "{:?}", verdict.problems);

        // A line beyond the last line of the file declaring a moved entity is a problem of
        // this unit's, just as one beyond the last line of its own file is: both files are
        // this unit's to mark.
        // @lfy def/generation/main.lfy:accept#accept:accept:032dc250d174c0232dd914cac293ffeb3f0494c8065247e29aaa7d0ff849dced
        let beyond = Output {
            path: "src/b.rs".to_string(),
            text: "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/a.lfy:C\npub struct C;\n\n// @lfy def/a.lfy:900\npub struct Extra;\n"
                .to_string(),
        };
        let verdict = accept(&program, &plan, &request, index, &[beyond], &[]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(
            verdict.problems[0].contains("line 900 of def/a.lfy"),
            "{:?}",
            verdict.problems
        );
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
            &[],
        );
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert!(verdict.problems.is_empty());
        assert_eq!(verdict.source_maps.len(), 1);
        // A marker that names an entity is left as written.
        // @lfy def/generation/main.lfy:accept#accept:accept:3464ba36f8e1e5d762286978e6be41278e751aded80a6bf152a33a3c4c2cb3a4
        assert_eq!(verdict.outputs, [output]);
        // One source map per output, in `outputs` order.
        // @lfy def/generation/main.lfy:accept#accept:accept:30a65355d64f16689cede881188462ef9f94082f48857623e59af57399ad7503
        let map = &verdict.source_maps[0];
        // @lfy def/generation/main.lfy:accept#accept:accept:8d2a3aa698445970d859538af521c42b878c576deae6eac97af3a97c5c7fd25f
        assert_eq!(map.target, "rust");
        // @lfy def/generation/main.lfy:accept#accept:accept:ea45d8dd2770ae8698006fbdd6aa54ebb0af32fabeb5d9edf8a1a432812d7d7c
        assert_eq!(map.output, "src/a.rs");
        // @lfy def/generation/main.lfy:accept#accept:accept:b27edcf9cd5b896cb993b81cba1f8faa47d79995f135c37e63d5e9ab648eed77
        assert_eq!(map.source, "def/a.lfy");
        // Every hash of the source map is derived here, never taken from the compiler's text.
        // @lfy def/generation/main.lfy:accept#accept:accept:1f423d0a0d06e5fc1e0ddf109f2e402b9a84537a80a91e6e4e459fb24ac288dc
        // @lfy def/generation/main.lfy:accept#accept:accept:90bf10cc4db0c256646cd0f1ba8c45bda247489103c46e1ce26289709829903a
        assert_eq!(map.hash, source_hash(&request.sources["def/a.lfy"]));
        assert_eq!(map.hash, source_hash(&plan.units[index].text));
        // @lfy def/generation/main.lfy:accept#accept:accept:0bcfc0b4637d084c09be1b3c407696d0b16f9c7f082c7f0d91de9ea2950d2902
        assert_eq!(map.answered, answered_ids(&program, &plan.units[index]));
        assert!(map.answered.windows(2).all(|pair| pair[0] <= pair[1]));
        // @lfy def/generation/main.lfy:accept#accept:accept:4a6c75702e77d3fab75aa1d127f35eaad993c997ed4cd0dd14f01162c019bc21
        assert_eq!(
            map.signature,
            source_hash(&interface_of(&workspace, &plan.units[index]))
        );
        // @lfy def/generation/main.lfy:accept#accept:accept:d9879de703d6314164058495241caf82738d58c8f39595b07a19ace78cdbd758
        assert!(map.dependencies.is_empty());
        assert!(plan.units[index].dependencies.is_empty());
        // @lfy def/generation/main.lfy:accept#accept:accept:95161566ef61156ea6f857f212fb510a3edf6221a4238a87b45595f60b6466bb
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
        // @lfy def/generation/main.lfy:accept#accept:accept:e4b7f22f09d4b3bbbc78c4edf8763f1f23ff76f5f4e5c858304e8315e8fe30aa
        assert_eq!((map.markers[0].output_line, map.markers[0].end), (1, 3));
        // The last marker of an output ends on its last line.
        // @lfy def/generation/main.lfy:accept
        assert_eq!((map.markers[1].output_line, map.markers[1].end), (4, 5));
        // So the regions partition the output from its first marker on.
        // @lfy def/generation/main.lfy:accept
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

    /// A batch compiled in a copy of the root is reviewed in that copy: the regions the
    /// verifier is shown are read from the directory given, and from the workspace root when
    /// none is.
    // @lfy def/generation/main.lfy:review#review:review:1c072387bd071f939c216da6d4ce6239cccd17ffa2df2f585223778cc3680fca
    #[test]
    fn the_regions_of_a_review_are_read_from_the_directory_given() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write(
                "def/a.lfy",
                "d A: `An a` {\n  @acceptanceCriteria.add({ behavior = `it holds` });\n}\n",
            )
            .write("src/a.rs", "the root\n")
            .write("copy/src/a.rs", "the copy\n");
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
            markers: vec![entity_marker(1, "A", 1, 1)],
            ..map_for("src/a.rs", "def/a.lfy")
        }];
        // The region's code is never quoted, so what the directory decides is the hash the
        // verdict is recorded against: the lines of the output it was judged in.
        let root = review(&program, &plan, &batch, &maps, None, &[]);
        let copied = review(
            &program,
            &plan,
            &batch,
            &maps,
            Some(&fixture.root.join("copy")),
            &[],
        );
        let id = root.asked.first().expect("one criterion").clone();
        assert_eq!(root.hashes[&id], source_hash("the root\n"));
        assert_eq!(copied.hashes[&id], source_hash("the copy\n"));
        assert_eq!(root.instructions, copied.instructions);
    }

    /// Sharing an output with another unit of the same target is no problem while the
    /// regions that unit's recorded map names are all still in it; dropping one of them is a
    /// problem naming the path, that unit's source, and the region. A co-owner compiled in
    /// the same batch writes its own regions again, so its map claims nothing, and the
    /// unit's own recorded map for the same path is what acceptance replaces.
    // @lfy def/generation/main.lfy:accept#accept:accept:a61aa4a9283df62d2e6f07fca7d7d134df081b931e93c5cd80d35866633303f7
    // @lfy def/generation/main.lfy:accept#accept:accept:e1803d0137825c4ab13fa3cc1b4bb2fbcc98d2304ae4776fa3a1b7c4ec27453f
    #[test]
    fn a_shared_output_keeps_the_regions_another_units_map_records() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write(
                "def/a.lfy",
                "\n\n/// A.\nd A {\n  $x = string;\n}\n\nd B {\n  $y = string;\n}\n",
            )
            .write("def/c.lfy", "d C {\n  $z = string;\n}\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let (plan, request, index) = one_unit(&program);
        let mine = "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/a.lfy:B\npub struct B;\n";
        let theirs = "\n// @lfy def/c.lfy:C\npub struct C;\n";
        let other = SourceMap {
            markers: vec![Marker {
                output_line: 6,
                file: "def/c.lfy".to_string(),
                entity: Some("C".to_string()),
                line: 1,
                column: None,
                requirement: None,
                end: 7,
            }],
            ..map_for("src/a.rs", "def/c.lfy")
        };
        // The region `def/c.lfy` records is still there, so sharing adds no problem.
        // @lfy def/generation/main.lfy:accept#accept:accept:e1803d0137825c4ab13fa3cc1b4bb2fbcc98d2304ae4776fa3a1b7c4ec27453f
        let shared = named_output(&format!("{mine}{theirs}"));
        let verdict = accept(
            &program,
            &plan,
            &request,
            index,
            std::slice::from_ref(&shared),
            std::slice::from_ref(&other),
        );
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // Dropped, it is one problem naming the path, the other source, and the region.
        // @lfy def/generation/main.lfy:accept#accept:accept:a61aa4a9283df62d2e6f07fca7d7d134df081b931e93c5cd80d35866633303f7
        let dropped = named_output(mine);
        let verdict = accept(
            &program,
            &plan,
            &request,
            index,
            std::slice::from_ref(&dropped),
            std::slice::from_ref(&other),
        );
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        for named in ["src/a.rs", "def/c.lfy", "def/c.lfy:C"] {
            assert!(verdict.problems[0].contains(named), "{:?}", verdict.problems);
        }
        // With `def/c.lfy` in the batch, its map claims nothing: it is being rewritten.
        // @lfy def/generation/main.lfy:accept#accept:accept:e1803d0137825c4ab13fa3cc1b4bb2fbcc98d2304ae4776fa3a1b7c4ec27453f
        let (together, _) = unit_of(&workspace, &plan, "def/c.lfy");
        let batch = Batch {
            units: vec![index, together],
            identifier: "a+1".to_string(),
        };
        let both = super::request(&program, &plan, &batch, &[], &BTreeMap::new());
        let verdict = accept(
            &program,
            &plan,
            &both,
            index,
            std::slice::from_ref(&dropped),
            std::slice::from_ref(&other),
        );
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // The unit's own recorded map for the same output is what acceptance replaces.
        // @lfy def/generation/main.lfy:accept
        let own = map_for("src/a.rs", "def/a.lfy");
        let verdict = accept(
            &program,
            &plan,
            &request,
            index,
            std::slice::from_ref(&dropped),
            std::slice::from_ref(&own),
        );
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // A region of this unit's own file is in the co-owner's map only because a map holds
        // every marker of its output; it is this unit's to drop, so dropping B is no problem
        // of sharing. @lfy def/generation/main.lfy:accept
        let theirs_only = SourceMap {
            markers: vec![
                Marker {
                    output_line: 4,
                    file: "def/a.lfy".to_string(),
                    entity: Some("B".to_string()),
                    line: 8,
                    column: None,
                    requirement: None,
                    end: 5,
                },
                Marker {
                    output_line: 6,
                    file: "def/c.lfy".to_string(),
                    entity: Some("C".to_string()),
                    line: 1,
                    column: None,
                    requirement: None,
                    end: 7,
                },
            ],
            ..map_for("src/a.rs", "def/c.lfy")
        };
        let without_b = named_output(&format!(
            "// @lfy def/a.lfy:A\npub struct A;\npub struct B;\n{theirs}"
        ));
        let verdict = accept(
            &program,
            &plan,
            &request,
            index,
            std::slice::from_ref(&without_b),
            std::slice::from_ref(&theirs_only),
        );
        assert_eq!(
            verdict
                .problems
                .iter()
                .filter(|problem| problem.contains("is shared with"))
                .count(),
            0,
            "{:?}",
            verdict.problems
        );
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:dd9767e93f188ad433adfb81d2b14e5bcc518b6e2d80e9ecca17d5a03a2f1ab3
    #[test]
    fn an_entity_without_a_marker_is_rejected_by_name() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        // A marker on the documentation line of A counts for A; nothing names B.
        let output = named_output("// @lfy def/a.lfy:3\npub struct A;\n");
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        // @lfy def/generation/main.lfy:accept#accept:accept:2a90cbf7112a647235b9f1c81167c5d44301cbc8369b5d4cc2973be6102fea26
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
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // @lfy def/generation/main.lfy:accept#accept:accept:f0f63ff3da1a2ceca7ecc1a89adc26e6a86649f0beed8e94ccc5726914eb4c4b
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
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert!(
            verdict.outputs[0].text.contains("// @lfy def/a.lfy:A.x"),
            "{}",
            verdict.outputs[0].text
        );
    }

    /// Every line a source map derives is the model's: a marker on A's documentation line
    /// records the line A is declared on rather than the number written, and a marker on
    /// B's last line records B's first. A marker on a line no declaration covers names no
    /// entity to derive a line from, so it is recorded as written: it names a file of the
    /// program, and the map holds every marker that does.
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
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // Each marker of the output names a file of the program, so the map holds all three:
        // the two that resolved, with the line of the declaration they named, and the one
        // that resolved to no declaration, with the line it spells and no entity.
        // @lfy def/generation/main.lfy:accept#accept:accept:e4b7f22f09d4b3bbbc78c4edf8763f1f23ff76f5f4e5c858304e8315e8fe30aa
        assert_eq!(
            verdict.source_maps[0]
                .markers
                .iter()
                .map(|marker| (marker.entity.clone(), marker.output_line, marker.line))
                .collect::<Vec<_>>(),
            [
                (None, 1, 1),
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

    // @lfy def/generation/main.lfy:accept#accept:accept:4a48dc9bd184c822a38d627cf303e55c050d877bac33bf8d5f8794db459af17d
    #[test]
    fn no_outputs_are_rejected() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let verdict = accept(&program, &plan, &request, index, &[], &[]);
        assert!(!verdict.accepted);
        assert!(
            verdict.problems[0].contains("no output"),
            "{:?}",
            verdict.problems
        );
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:324f6bd2c925ffa32b5feee5d17d3195c149deb99f00247918a0ee6d58d1b870
    #[test]
    fn an_output_outside_the_output_directory_is_rejected_by_path() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let mut output = named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n");
        output.path = "srcx/a.rs".to_string();
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(
            verdict.problems[0].contains("srcx/a.rs"),
            "{}",
            verdict.problems[0]
        );
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:e25297fa468e4f2b18f120993ee14b756d7715661e928e3e161f9aba05aad304
    #[test]
    fn an_output_whose_extension_the_target_does_not_allow_is_rejected_by_path() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        assert_eq!(program.workspace.targets[0].extensions, ["rs"]);
        let (plan, request, index) = one_unit(&program);
        let mut output = named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n");
        output.path = "src/a.txt".to_string();
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(
            verdict.problems[0].contains("src/a.txt"),
            "{}",
            verdict.problems[0]
        );
        assert!(
            verdict.problems[0].contains("rs"),
            "{}",
            verdict.problems[0]
        );
    }

    // @lfy def/generation/main.lfy:accept#accept:accept:c9819875941e1d859a6baa6c2043bfaaebeee7923b85b2e2edbd4b88ec00c6f3
    #[test]
    fn a_marker_naming_an_entity_the_file_does_not_declare_is_rejected_by_output_line_and_name() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let output =
            named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n// @lfy def/a.lfy:Nowhere\n");
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
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

    // @lfy def/generation/main.lfy:accept#accept:accept:032dc250d174c0232dd914cac293ffeb3f0494c8065247e29aaa7d0ff849dced
    #[test]
    fn a_marker_naming_a_line_past_the_last_is_rejected_by_output_line() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let output =
            named_output("// @lfy def/a.lfy:A\n\n// @lfy def/a.lfy:99\n// @lfy def/a.lfy:B\n");
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
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
    // @lfy def/generation/main.lfy:accept#accept:accept:f4b16e912ad6060ea8d1d2f85da81f81022ff64e04b9b347f00579d497e86481
    #[test]
    fn a_marker_naming_a_file_that_is_not_in_the_program_is_ignored() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let output = named_output(
            "// @lfy def/a.lfy:A\n// @lfy def/b.lfy:8\n// @lfy def/b.lfy:Nowhere\n// @lfy def/a.lfy:B\n",
        );
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
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

    // @lfy def/generation/main.lfy:accept#accept:accept:4d6cc733ae9a9a4d4abc942dff1d089398f749e30a393fd194edafe27c85cf95
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
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // The marker of the other file is kept, and its line is derived from that file.
        let markers = &verdict.source_maps[0].markers;
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[1].file, "def/b.lfy");
        assert_eq!(markers[1].entity.as_deref(), Some("B"));
        assert_eq!(markers[1].line, 1);
        // @lfy def/generation/main.lfy:accept#accept:accept:d9879de703d6314164058495241caf82738d58c8f39595b07a19ace78cdbd758
        assert_eq!(verdict.source_maps[0].dependencies.len(), 1);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(
            verdict.source_maps[0].dependencies["def/b.lfy"],
            source_hash(&interface_of(&workspace, b))
        );

        // A marker of another file may name that file's entity and answer for that file's own
        // local criterion: the id belongs to that file's unit, so this unit does not check it.
        // An output shared by two units holds the other unit's markers and ids, and checking
        // them here would make the shared output unacceptable to one of them.
        // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
        let make = *b.entities.last().expect("make is an entity of b");
        let (criteria, _) = requirement_ids(&program.files[b.lowered], make);
        let output = named_output(&format!(
            "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:make#{}\nfn make() {{}}\n",
            criteria[0]
        ));
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // An id no file of the program knows is not checked either, when the marker names
        // another file: only a marker of this unit's own file makes a claim this unit answers
        // for.
        // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
        let output = named_output(
            "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:make#make:make:nowhere\nfn make() {}\n",
        );
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
    }

    /// A marker of the unit's own file may answer for a local criterion of the unit or for a
    /// global one, and for nothing else; the source map records the hash of the unit's
    /// requirement ids.
    // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
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
        // @lfy def/generation/main.lfy:accept
        let output = named_output(&format!(
            "// @lfy def/a.lfy:A#{}\npub struct A;\n// @lfy def/a.lfy:A#{global}\nfn holds() {{}}\n",
            local[0]
        ));
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // @lfy def/generation/main.lfy:accept#accept:accept:666051c035bda35cafa77995ca9f05915cb5d6f3d9dc74b172f9187d403c1a15
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
        // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
        let output = named_output("// @lfy def/a.lfy:A#A:A:nowhere\npub struct A;\n");
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
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
    // @lfy def/generation/main.lfy:accept#accept:accept:9bde11d25fcf12e8c1a113f8be1c479d68185569c8170c39b3348772bf613541
    #[test]
    fn whether_an_output_builds_is_not_checked_here() {
        let fixture = a_with_two_entities();
        let program = fixture.program();
        let (plan, request, index) = one_unit(&program);
        let output = named_output(
            "// @lfy def/a.lfy:A\nthis is not Rust at all (((\n// @lfy def/a.lfy:B\nnor is this\n",
        );
        let verdict = accept(&program, &plan, &request, index, &[output], &[]);
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
            answered: Vec::new(),
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
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:48e0f71f7db0725ebe634eeac3dac1d913b250a6626d34b72282e59198837589
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
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:3fa1873c256699bcc68b376b528e4f35f840fe93aad209cc0f992a5dee017fce
        assert_eq!(outputs_of(&source_maps_of(&workspace, Some("rust"))).len(), 2);
        assert!(source_maps_of(&workspace, Some("other")).is_empty());
        // A map that records no answered ids is read with none.
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:9a20ac7072bf79bbb2d38d819ba4fb8127d258b6f0baf9134dafc7c98398e94a
        fixture.write(
            "elfie-compile/maps/rust/a.json",
            "[{\"target\":\"rust\",\"output\":\"crates/a/src/a.rs\",\"source\":\"def/a.lfy\",\
             \"hash\":\"\",\"requirements\":\"\",\"signature\":\"\",\"dependencies\":{},\
             \"generated\":\"\",\"markers\":[]}]",
        );
        let maps = source_maps_of(&workspace, Some("rust"));
        assert_eq!(maps[0].output, "crates/a/src/a.rs");
        assert!(maps[0].answered.is_empty(), "{:?}", maps[0].answered);

        // The map of an output that is gone is left out.
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:777506c6ffecf859d5436778cd8e1997eef47cd57e5f8f41754058bbe3117d23
        fs::remove_file(fixture.root.join("crates/b/src/b.rs")).unwrap();
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:49f3bf512172a2a718b6ee96460c1a23944304c9e259e781c8928937dbb775fc
        assert_eq!(
            outputs_of(&source_maps_of(&workspace, None)),
            ["crates/a/src/a.rs"]
        );

        // A map file that cannot be read as a JSON list of source maps contributes nothing,
        // and every other map file of the target is read all the same.
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:00a062bda31894d720907cb0ec23e4bc5dd3e4d0db76200eab3b747b849ca38d
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:2f53d14209715f08aa022f39c528fe655af6686d5dd463d239c591156a170d73
        fixture.write("elfie-compile/maps/rust/a.json", "{\"output\":");
        fixture.write("crates/b/src/b.rs", "// b\n");
        assert_eq!(
            outputs_of(&source_maps_of(&workspace, None)),
            ["crates/b/src/b.rs"]
        );
    }

    /// The map files of a target's folder are read in the ascending text order of their
    /// paths relative to it, so a map file beside a directory of the same name is read
    /// where its own path falls rather than after the directory's files.
    // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:e3ab53d40653267f2e160d4a7f3251350dc8120cab5217a982112c74085d6a81
    #[test]
    fn a_map_file_beside_a_directory_is_read_in_path_order() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("crates/cli.rs", "// cli\n")
            .write("crates/cli/main.rs", "// cli main\n")
            .write("crates/clip.rs", "// clip\n");
        for (map, output, source) in [
            ("cli.json", "crates/cli.rs", "def/cli.lfy"),
            ("cli/main.json", "crates/cli/main.rs", "def/cli/main.lfy"),
            ("clip.json", "crates/clip.rs", "def/clip.lfy"),
        ] {
            write_source_maps(
                &fixture.root.join("elfie-compile/maps/rust").join(map),
                &[map_for(output, source)],
            )
            .unwrap();
        }
        let workspace = load(&fixture.root);
        // `.` sorts before `/`, which sorts before a letter, so `cli.json` comes first,
        // then the file under `cli/`, then `clip.json`.
        assert_eq!(
            outputs_of(&source_maps_of(&workspace, None)),
            ["crates/cli.rs", "crates/cli/main.rs", "crates/clip.rs"]
        );
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

        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:c968c8acaf4d38cdd8430f58ae5b09a27d1e2e6bd4ee8861bd63a620ddfd7a9c
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
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
        // @lfy def/generation/main.lfy:sourceMapsOf#sourceMapsOf:sourceMapsOf:1dc22603643b17fad4874110ef09a3391b67d71c28a9f5b35a7dfa18438f3409
        assert!(fixture.root.join("src/source-map.json").is_file());
        assert!(!fixture.root.join("elfie-compile/cache").exists());
        assert!(!fixture.root.join("elfie-compile/maps/rust/b.json").exists());
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
        // @lfy def/generation/main.lfy:record#record:record:d7ed944c0449f01835fd94e7de01e58066347177591b374f6a7beee48113f403
        // @lfy def/generation/main.lfy:record#record:record:5b5e70df30f90156b3023da573235ca899c7a55801fbecd76167819cf57e21ad
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
        // @lfy def/generation/main.lfy:record#record:record:5c0c63725b35e7fab925c81abc27914b55863257ede7a2863a3bce15e5b03ff5
        // @lfy def/generation/main.lfy:record#record:record:60d0cd914ebe3c1cbbd582cd3229d8adc3a00b879720f1caeec671fb4c707551
        // @lfy def/generation/main.lfy:record#record:record:1a8586a7f2168dcbbbc34c4373d1f944d6e0313f9b0840a53aa8f54e3e3208d6
        let text = fs::read_to_string(&path).unwrap();
        assert!(record(workspace, unit, &[map.clone(), second]));
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|line| line.trim().strip_prefix('"'))
            .filter_map(|line| line.split_once('"').map(|(key, _)| key))
            .collect();
        assert_eq!(keys.len(), 20, "{text}");
        assert!(
            keys.chunks(10)
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
        // @lfy def/generation/main.lfy:record#record:record:acccd867dc43a326a46b14f88ac19bb24373ec1a90795894fa96f075e1fcbcd9
        assert!(record(workspace, unit, std::slice::from_ref(&map)));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        // No source map at all removes the file.
        // @lfy def/generation/main.lfy:record#record:record:aa6df37cdc9d7a159ba508148db794c3edbc568247899ccbd55574df85a1177d
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
        // @lfy def/generation/main.lfy:record#record:record:1ec300497e4609fbc621214de54b44c8b7f666382b4cf02496efdc55ed4cca99
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
    // reviewsOf, recordReviews
    // -----------------------------------------------------------------------------------

    /// A project whose `def` holds `a.lfy`, declaring `A` with two criteria, and `b.lfy`,
    /// declaring `B` with one; the plan of its two units comes with the ids of `A`'s.
    fn a_with_records() -> (Fixture, Program, Plan, Vec<String>) {
        let fixture = Fixture::with_rust_target();
        fixture
            .write(
                "def/a.lfy",
                "d A: `An a` {\n  @acceptanceCriteria\n    .add({ behavior = `the first` })\n    .add({ behavior = `the second` });\n}\n",
            )
            .write(
                "def/b.lfy",
                "d B: `A b` {\n  $x = string;\n  where (`x is set`) -> `B holds x`;\n}\n",
            );
        let program = fixture.program();
        assert_bound(&program.workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (lowered, entity) = {
            let (_, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
            (unit.lowered, unit.entities[0])
        };
        let (ids, _) = requirement_ids(&program.files[lowered], entity);
        assert_eq!(ids.len(), 2, "{ids:?}");
        (fixture, program, plan, ids)
    }

    /// A unit's review record file is `elfie-compile/reviews`, the target's identifier, and
    /// the unit's stem with the extension `.json`, under the workspace root — beside its map
    /// file and never under it.
    // @lfy def/generation/data.lfy:ReviewRecord#ReviewRecord:ReviewRecord:d1283130451aac07da56acfa5179b7af277f4cbbb6d93ed00c6b0aad6499db7a
    #[test]
    fn a_units_review_record_file_is_under_elfie_compile_reviews_by_target_and_stem() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/cli/main.lfy", "d Cli { $x = string; }\n");
        let program = fixture.program();
        assert_bound(&program.workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
        assert_eq!(unit.stem, "cli/main");
        assert_eq!(
            review_file(&program.workspace, unit),
            fixture
                .root
                .join("elfie-compile/reviews/rust/cli/main.json")
        );
        assert_ne!(
            review_file(&program.workspace, unit),
            map_file(&program.workspace, unit)
        );
    }

    /// The records of a unit come back in the order its file lists them; one whose id is of
    /// no local criterion or test of the unit is left out, and a file that is no JSON list of
    /// records gives none.
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:5f98be1ee1d2bca400fb4e2fe3c6433111b63e55a9fd27fe360103362dc8fc62
    #[test]
    fn the_records_of_a_unit_are_read_in_the_order_the_file_lists_them() {
        let (fixture, program, plan, ids) = a_with_records();
        let (_, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
        let records = vec![
            ReviewRecord {
                id: ids[1].clone(),
                status: ReviewStatus::Violated,
                hash: source_hash("second"),
            },
            ReviewRecord {
                id: ids[0].clone(),
                status: ReviewStatus::Satisfied,
                hash: source_hash("first"),
            },
        ];
        assert!(record_reviews(&program.workspace, unit, &records));
        // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:d95399b23b29d50a8fba5af56c20731f8f7b0b2bafe16e9914d5090e374ce251
        assert_eq!(reviews_of(&program, unit), records);

        // A record whose id is of no local criterion or test of the unit is left out, so a
        // criterion that was reworded is never carried on its old id's verdict.
        // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0b41caebdc61220c67f088bc69f465267b2c9e2b36d07c52b02680bf6cb4724c
        let mut foreign = records.clone();
        foreign.push(ReviewRecord {
            id: "A:A:gone".to_string(),
            status: ReviewStatus::Satisfied,
            hash: source_hash("gone"),
        });
        assert!(record_reviews(&program.workspace, unit, &foreign));
        assert_eq!(reviews_of(&program, unit), records);

        // A file that cannot be read as a JSON list of review records gives none.
        // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0aa4f9eaff20fee7bf263db985597b4d886a2ac762b47b1af1734107a9ac1503
        let path = fixture.root.join("elfie-compile/reviews/rust/a.json");
        fixture.write("elfie-compile/reviews/rust/a.json", "{\"id\":");
        assert!(reviews_of(&program, unit).is_empty());
        fixture.write("elfie-compile/reviews/rust/a.json", "[{\"id\":\"x\"}]");
        assert!(reviews_of(&program, unit).is_empty());

        // Nothing is written, and nothing under elfie-compile/cache is read.
        // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
        // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:1dc22603643b17fad4874110ef09a3391b67d71c28a9f5b35a7dfa18438f3409
        let before = fs::read_to_string(&path).unwrap();
        let _ = reviews_of(&program, unit);
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert!(!fixture.root.join("elfie-compile/cache").exists());
    }

    /// A unit whose review record file was never written has no records, so every criterion
    /// and test of it is reviewed again.
    // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:dfd4f71e53b2fb7fce71408d09532e78b71c30b303ef3d8268979cce8c5a7fc3
    #[test]
    fn a_unit_with_no_review_record_file_has_no_records() {
        let (fixture, program, plan, _ids) = a_with_records();
        let (_, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
        assert!(!fixture.root.join("elfie-compile/reviews").exists());
        // @lfy def/generation/main.lfy:reviewsOf#reviewsOf:reviewsOf:0aa4f9eaff20fee7bf263db985597b4d886a2ac762b47b1af1734107a9ac1503
        assert!(reviews_of(&program, unit).is_empty());
    }

    /// A project cloned without `elfie-compile` records its verdicts: every missing folder
    /// above the review record file is created.
    // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:b4e3424b0ba9d0aee9525f1766d277eebbc0ccc029472634a502abe4447f3c45
    #[test]
    fn recording_reviews_creates_every_folder_above_the_file() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/cli/main.lfy",
            "d Cli: `A cli` {\n  $x = string;\n  where (`x is set`) -> `Cli holds x`;\n}\n",
        );
        let program = fixture.program();
        assert_bound(&program.workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (lowered, entity) = {
            let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
            (unit.lowered, unit.entities[0])
        };
        let (ids, _) = requirement_ids(&program.files[lowered], entity);
        let (_, unit) = unit_of(&program.workspace, &plan, "def/cli/main.lfy");
        assert!(!fixture.root.join("elfie-compile").exists());
        let records = vec![ReviewRecord {
            id: ids[0].clone(),
            status: ReviewStatus::Satisfied,
            hash: source_hash("region"),
        }];
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:72846edcc746900edf9c781c66023ad863b10442a8c62d330bee15766a2090c3
        assert!(record_reviews(&program.workspace, unit, &records));
        let path = fixture
            .root
            .join("elfie-compile/reviews/rust/cli/main.json");
        assert!(path.is_file());
        // The records read back, so the next review carries them.
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:5708abb1bf089eb06657f5688610a6c234b81463164fd199b77b315608316837
        assert_eq!(reviews_of(&program, unit), records);
    }

    /// Recording one unit's verdicts replaces its review record file and leaves every other
    /// byte for byte as it was, in the order given with every object's keys alphabetical; no
    /// record at all removes the file, and a file that cannot be written gives `false`.
    // @lfy def/generation/main.lfy:recordReviews
    #[test]
    fn recording_reviews_replaces_the_file_and_leaves_every_other_alone() {
        let (fixture, program, plan, ids) = a_with_records();
        let (_, other) = unit_of(&program.workspace, &plan, "def/b.lfy");
        let (lowered, entity) = (other.lowered, other.entities[0]);
        let (theirs, _) = requirement_ids(&program.files[lowered], entity);
        assert!(record_reviews(
            &program.workspace,
            other,
            &[ReviewRecord {
                id: theirs[0].clone(),
                status: ReviewStatus::Satisfied,
                hash: source_hash("b"),
            }]
        ));
        let beside = fixture.root.join("elfie-compile/reviews/rust/b.json");
        let before = fs::read_to_string(&beside).unwrap();

        let (_, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
        let records = vec![
            ReviewRecord {
                id: ids[1].clone(),
                status: ReviewStatus::Violated,
                hash: source_hash("second"),
            },
            ReviewRecord {
                id: ids[0].clone(),
                status: ReviewStatus::Satisfied,
                hash: source_hash("first"),
            },
        ];
        assert!(record_reviews(&program.workspace, unit, &records));
        let path = fixture.root.join("elfie-compile/reviews/rust/a.json");
        // The records are listed in the order given, not sorted, and the keys of every
        // object are in alphabetical order.
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:c0e143aef64c3c7bed3e5892dd6d03a0c46df17166ce5ffa598b2db167027369
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:52d12769dcd44be836a5afeb17b9c7341a2a82265bafc029ee658a417f8e0569
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:60d0cd914ebe3c1cbbd582cd3229d8adc3a00b879720f1caeec671fb4c707551
        let text = fs::read_to_string(&path).unwrap();
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|line| line.trim().strip_prefix('"'))
            .filter_map(|line| line.split_once('"').map(|(key, _)| key))
            .collect();
        assert_eq!(keys, ["hash", "id", "status", "hash", "id", "status"], "{text}");
        assert!(
            text.find(&ids[1]) < text.find(&ids[0]),
            "the order given: {text}"
        );
        // The same records always give the same bytes, and a second recording replaces what
        // the file held.
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:db1f607aac959c64937cb811664557dedde402eb3fdcaaaf6a2db6b1284ecfd1
        assert!(record_reviews(&program.workspace, unit, &records));
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert!(record_reviews(
            &program.workspace,
            unit,
            std::slice::from_ref(&records[0])
        ));
        assert_eq!(reviews_of(&program, unit), [records[0].clone()]);
        // No other unit's file is written.
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:8d039cf20453fa3420e6acdb3ce42b2031909a9bc211b646a683be5017efa7fd
        assert_eq!(fs::read_to_string(&beside).unwrap(), before);

        // No record at all removes the file, and removing what is already gone is recorded
        // all the same.
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:0149e6a1c5272af0b7ca76e627e0fb18e511db4e2076f128db7607e14f49ae85
        assert!(record_reviews(&program.workspace, unit, &[]));
        assert!(!path.exists());
        assert!(record_reviews(&program.workspace, unit, &[]));

        // A directory where the file belongs cannot be written as a file.
        // @lfy def/generation/main.lfy:recordReviews#recordReviews:recordReviews:1f4671d2ae5027d4e5bcfaf56dc68c765e27e31a92fc783b7f64a2165f391dd4
        fs::create_dir(&path).unwrap();
        assert!(!record_reviews(&program.workspace, unit, &records));
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
            answered: Vec::new(),
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
        // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:093e721919b4f8d968ccd2497d31d9e1ac8783ddfb5ee2bbda092b4302715a0f
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
    // @lfy def/generation/main.lfy:regionsOf
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
        // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:d0dd761f2a88d10b62707b3452f9804e82f4c9ae6d4212b5b8b799f9355a82c5
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
        // @lfy def/generation/main.lfy:changes#changes:changes:771d19c7cdc3e68ae80e9c437cf28ae04d6d6112c99fdff85bcf61c1efacc428
        let changes = changes(&program, unit,None);
        // A name declared now and not before is an addition.
        // @lfy def/generation/main.lfy:changes#changes:changes:9fa34ffb6593e18420b047e381120aa4b10d245c1b1cc825bd68c5b21928f000
        assert_eq!(
            change_kinds(&changes),
            [("A", ChangeKind::Added), ("f", ChangeKind::Added)]
        );
    }

    /// A change is of the file, not of the unit: `def/a.lfy` declares `A` and `C`, and `C`
    /// moves into the unit of `def/b.lfy`, so `C` is reported for `def/a.lfy` and not for
    /// `def/b.lfy`, whose unit compiles it.
    // @lfy def/generation/main.lfy:changes#changes:changes:718b0ba1ae484d5d40adbb3d2dabcfed7ec6b7a0c976dd9193d6c8b00e479f4f
    #[test]
    fn a_moved_entity_is_reported_for_the_file_that_declares_it() {
        let fixture = a_cycle();
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        // `C` has left a's unit and joined b's.
        assert_eq!(names(&workspace, &a.entities), ["A"]);
        assert_eq!(names(&workspace, &b.entities), ["B", "C"]);

        // With no previous text, each file's own declarations are the additions.
        // @lfy def/generation/main.lfy:changes#changes:changes:771d19c7cdc3e68ae80e9c437cf28ae04d6d6112c99fdff85bcf61c1efacc428
        assert_eq!(
            change_kinds(&changes(&program, a, None)),
            [("A", ChangeKind::Added), ("C", ChangeKind::Added)]
        );
        assert_eq!(
            change_kinds(&changes(&program, b, None)),
            [("B", ChangeKind::Added)]
        );

        // Against its own text, neither side differs: both are read as the file's.
        // @lfy def/generation/main.lfy:changes
        let text = "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nd C {\n  $x = string;\n}\n";
        assert_eq!(changes(&program, a, Some(text)), []);
        // `C` was not in the file then and is now, so it is an addition of `def/a.lfy`
        // although the unit of `def/b.lfy` is what compiles it.
        // @lfy def/generation/main.lfy:changes
        let without = "use \"./b\";\n\nd A {\n  $b = B;\n}\n";
        assert_eq!(
            change_kinds(&changes(&program, a, Some(without))),
            [("C", ChangeKind::Added), ("C.x", ChangeKind::Added)]
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
    // @lfy def/generation/main.lfy:changes#changes:changes:1c9565b23d23c4d3bcbb3fe6534631ef1946f8869e3f178f425b4e12595f3e0b
    // @lfy def/generation/main.lfy:changes#changes:changes:f54761e4898e7ebd66aacb630307ee416935a1052c402126c3c712e2eb632c2a
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

    /// A file whose entities read the same after the two declarations swap places, a comment
    /// moves, and the indentation changes gives no change at all; a statement of a written
    /// body gives one.
    // @lfy def/generation/main.lfy:changes#changes:changes:ef493f153deb6dbaac3835bfc84dae2266cdd2d04f5c990f3aae5db76de2424e
    // @lfy def/generation/main.lfy:changes#changes:changes:d7b590dfd89c646eecf798bdafc911cd87dc2b66f0c3bc1e3cb916051b5fa6f8
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
        // Entities are compared by name, never by place or by line, so the two declarations
        // swapping places, a comment moving, and the indentation changing give nothing.
        // @lfy def/generation/main.lfy:changes
        let same = "function g(x: number): `Doubles` -> number {\n    return x * 2;\n}\n\n\
                    // a comment, somewhere else now\nd A {\n\n  $x   = string;\n}\n";
        assert!(changes(&program, unit, Some(same)).is_empty());
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

    /// `def/a.lfy` declaring `A` with two criteria and one test, with `src/a.rs` generated
    /// for it, and the plan and the batch of its one unit.
    fn a_to_review() -> (Fixture, Program, Plan, Batch) {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d A: `An a` {\n  @acceptanceCriteria\n    .add({ behavior = `the first` })\n    .add({ behavior = `the second` });\n  @test({ input = 1, expect = 2 });\n}\n",
        );
        fixture.write("src/a.rs", "alpha\nbeta\ngamma\ndelta\n");
        let program = fixture.program();
        assert_bound(&program.workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, _) = unit_of(&program.workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![index],
            identifier: "a".to_string(),
        };
        (fixture, program, plan, batch)
    }

    /// The text of the whole of that project's one output, which the hash of every criterion
    /// and test of its unit is taken over.
    const A_REGION: &str = "alpha\nbeta\ngamma\ndelta\n";

    /// The one source map of that project: `src/a.rs` from `def/a.lfy`, with one marker
    /// naming `A` and the whole output as its region.
    fn a_source_map() -> Vec<SourceMap> {
        vec![SourceMap {
            target: "rust".to_string(),
            output: "src/a.rs".to_string(),
            source: "def/a.lfy".to_string(),
            hash: source_hash("x"),
            requirements: String::new(),
            answered: Vec::new(),
            signature: source_hash("i"),
            dependencies: BTreeMap::new(),
            generated: "2026-09-18T10:00:00Z".to_string(),
            markers: vec![entity_marker(1, "A", 1, 4)],
        }]
    }

    /// The ids of the criteria and of the tests of the one entity of that project's unit.
    fn a_requirement_ids(program: &Program, plan: &Plan) -> (Vec<String>, Vec<String>) {
        let (_, unit) = unit_of(&program.workspace, plan, "def/a.lfy");
        requirement_ids(&program.files[unit.lowered], unit.entities[0])
    }

    // @lfy def/generation/main.lfy:review#review:review:d24fcd3eb666702ff962f94907d5ca24dc866b3b9eccb1d1480ef5b90c8a1243
    #[test]
    fn a_review_request_lists_every_criterion_with_its_place_and_names_every_region() {
        let (_fixture, program, plan, batch) = a_to_review();
        let maps = a_source_map();
        let request = review(&program, &plan, &batch, &maps, None, &[]);
        // @lfy def/generation/main.lfy:review#review:review:197b2d0e6f7458d538f3cc8d8568be98a60636ca07026cd2eeb1d5933910f156
        assert_eq!(request.batch, Some(batch.clone()));
        let text = &request.instructions;
        // The heading names the batch and the target.
        // @lfy def/generation/main.lfy:review
        assert!(
            text.contains("# Reviewing the batch `a` for the target `rust`"),
            "{text}"
        );
        assert!(text.contains("#### `A` (data: DataDeclaration)"), "{text}");
        assert!(text.contains("Definition: An a"), "{text}");
        // The region is named and none of its lines is written.
        // @lfy def/generation/main.lfy:review#review:review:0f43e9772c4aad39520e42801704e6fe6e9115829e267701e251b74908849820
        // @lfy def/generation/main.lfy:review#review:review:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
        assert!(text.contains("`src/a.rs:1-4`"), "{text}");
        for line in A_REGION.lines() {
            assert!(!text.contains(line), "{line} is quoted:\n{text}");
        }
        // Each criterion of a chain has a place of its own, on the line its `add` begins
        // on, and is prefixed by its id; a test's place is the line its origin begins on.
        // @lfy def/generation/main.lfy:review#review:review:f3ebae56b7c2144a9edb3e3b69ac07f174e3974f258284e909f2e10068ec9831
        let (criteria, tests) = a_requirement_ids(&program, &plan);
        for (id, place, said) in [
            (&criteria[0], 3, "the first"),
            (&criteria[1], 4, "the second"),
        ] {
            assert!(
                text.contains(&format!("`{id}` def/a.lfy:{place} — {said}")),
                "{text}"
            );
        }
        assert!(
            text.contains(&format!("`{}` def/a.lfy:5 — Input `1` gives `2`", tests[0])),
            "{text}"
        );
        // Only local criteria and tests are reviewed here.
        // @lfy def/generation/main.lfy:review
        assert!(text.contains("A global one is reviewed once"), "{text}");
        // Every criterion and every test is asked about, in the order they are listed, and
        // nothing is carried without a record to carry.
        // @lfy def/generation/main.lfy:review#review:review:5ff1db3eab7473b565a623216b6a86f264541a52a33fc4c939e828c7f537c783
        assert_eq!(
            request.asked,
            [criteria[0].clone(), criteria[1].clone(), tests[0].clone()]
        );
        assert!(request.carried.is_empty(), "{:?}", request.carried);
        // Each of them is hashed over the lines of the region of the entity it belongs to.
        // @lfy def/generation/main.lfy:review#review:review:2406e70a6c25c47a8f50f45cfb28517ffe7990ce2a1647dd53f04024b5ea5269
        let region = source_hash(A_REGION);
        for id in &request.asked {
            assert_eq!(request.hashes[id], region, "{id}");
        }
        // The protocol: how a region is read, where a verdict goes, the keys of the line it
        // falls back to, the three statuses, the tools, and the end line.
        // @lfy def/generation/data.lfy:ReviewRequest#ReviewRequest:ReviewRequest:5f3541f331f08c11374202659e49f252d011829d997b2663a9a7aa1eb494c55d
        for part in [
            "examine and never edit",
            "you propose no code",
            "The code of a region is read through `elfie_output`",
            "Submit each verdict through `elfie_report` as soon as you have judged it",
            "one per criterion and per test",
            "`id` is copied from the id the criterion or test is given above, or left out",
            "the two halves of the place given with it above",
            "`satisfied`",
            "`violated`",
            "`unverifiable`",
            "unverifiable, never violated",
            "before files are searched or read by hand",
            "`elfie_source`",
            "`elfie_entity`",
            "`elfie_references`",
            "Where `elfie_report` is not offered",
            "one JSON object on one line with exactly the keys `id`, `file`, `line`, \
             `status`, `evidence`, and `note`",
            "`ELFIE: REVIEWED`",
        ] {
            assert!(text.contains(part), "{part} is missing:\n{text}");
        }
    }

    /// A criterion whose record says it was satisfied against the code the entity's regions
    /// still hash to is carried rather than listed; one whose record says anything else is
    /// listed again.
    // @lfy def/generation/main.lfy:review#review:review:b7aa1df4bc363d68c066212b46c69d99c6ef6a81f69611ea4272b154315f24c0
    #[test]
    fn a_criterion_satisfied_against_the_code_as_it_stands_is_carried() {
        let (_fixture, program, plan, batch) = a_to_review();
        let maps = a_source_map();
        let (criteria, tests) = a_requirement_ids(&program, &plan);
        let hash = source_hash(A_REGION);
        let records = vec![
            ReviewRecord {
                id: criteria[0].clone(),
                status: ReviewStatus::Satisfied,
                hash: hash.clone(),
            },
            ReviewRecord {
                id: criteria[1].clone(),
                status: ReviewStatus::Unverifiable,
                hash: hash.clone(),
            },
        ];
        let request = review(&program, &plan, &batch, &maps, None, &records);
        // The satisfied one is carried with its record, and only the other two are asked.
        // @lfy def/generation/main.lfy:review#review:review:32e025cd22c3a749bbc3afba52f86cb9b26ac41d0f4a822a6e5e00639e557312
        assert_eq!(request.asked, [criteria[1].clone(), tests[0].clone()]);
        assert_eq!(request.carried, [records[0].clone()]);
        let text = &request.instructions;
        assert!(!text.contains(&criteria[0]), "{text}");
        assert!(text.contains(&criteria[1]), "{text}");
        // Every criterion and test of the unit is hashed, carried or not.
        // @lfy def/generation/main.lfy:review#review:review:2406e70a6c25c47a8f50f45cfb28517ffe7990ce2a1647dd53f04024b5ea5269
        assert_eq!(request.hashes.len(), 3, "{:?}", request.hashes);
        assert_eq!(request.hashes[&criteria[0]], hash);
    }

    /// A record made against code the entity's regions no longer hash to carries nothing:
    /// the criterion is listed again.
    // @lfy def/generation/main.lfy:review#review:review:cb4d48355ae14509cede19b75e670e8b67e1a03aff85516a3ad84b640cfc14ba
    #[test]
    fn a_record_of_code_that_changed_carries_nothing() {
        let (_fixture, program, plan, batch) = a_to_review();
        let maps = a_source_map();
        let (criteria, tests) = a_requirement_ids(&program, &plan);
        let records = vec![ReviewRecord {
            id: criteria[0].clone(),
            status: ReviewStatus::Satisfied,
            hash: source_hash("alpha\nbeta\ngamma\nepsilon\n"),
        }];
        let request = review(&program, &plan, &batch, &maps, None, &records);
        // @lfy def/generation/main.lfy:review#review:review:5ff1db3eab7473b565a623216b6a86f264541a52a33fc4c939e828c7f537c783
        assert_eq!(
            request.asked,
            [criteria[0].clone(), criteria[1].clone(), tests[0].clone()]
        );
        assert!(request.carried.is_empty(), "{:?}", request.carried);
    }

    /// With every criterion and test of the batch carried, the instructions are the heading
    /// and one line saying nothing needs review, and no protocol.
    // @lfy def/generation/main.lfy:review#review:review:e0ecb428c24270ba6896549746ba51b216cbdf61967b6e9436de9b2b716b455c
    #[test]
    fn a_batch_whose_every_verdict_is_carried_needs_no_review() {
        let (_fixture, program, plan, batch) = a_to_review();
        let maps = a_source_map();
        let (criteria, tests) = a_requirement_ids(&program, &plan);
        let hash = source_hash(A_REGION);
        let records: Vec<ReviewRecord> = criteria
            .iter()
            .chain(&tests)
            .map(|id| ReviewRecord {
                id: id.clone(),
                status: ReviewStatus::Satisfied,
                hash: hash.clone(),
            })
            .collect();
        let request = review(&program, &plan, &batch, &maps, None, &records);
        // @lfy def/generation/main.lfy:review#review:review:5133eddf9b981ba9bac41113779c58b491cedfbc67e2e3742ac236b381a254dc
        assert!(request.asked.is_empty(), "{:?}", request.asked);
        assert_eq!(request.carried.len(), 3, "{:?}", request.carried);
        let text = &request.instructions;
        assert!(
            text.contains("# Reviewing the batch `a` for the target `rust`"),
            "{text}"
        );
        assert!(text.contains("Nothing in the batch needs review"), "{text}");
        // @lfy def/generation/main.lfy:review#review:review:bb873e216e23bb2a7659d3b1888588387a1f7980df4fac536938ec32d6d42b8d
        assert!(!text.contains("#### `A`"), "{text}");
        assert!(!text.contains("src/a.rs"), "{text}");
        assert!(!text.contains("ELFIE: REVIEWED"), "{text}");
    }

    /// A review excerpts only what the unit's own file generated, and a region a marker of
    /// a second source map names again is repeated as its path and lines alone.
    // @lfy def/generation/main.lfy:review#review:review:e67f26bdccf8eaa77ebbfe85b9da0b83bf73bff7b2cad901af69d8b4797ad76e
    #[test]
    fn a_review_holds_the_regions_of_its_own_file_alone() {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "d A: `An a` {\n  $x = string;\n  where (`x is set`) -> `A holds x`;\n}\n",
        );
        fixture.write("def/b.lfy", "d A: `Another a` {\n  $y = string;\n}\n");
        fixture.write("src/a.rs", "one\ntwo\nthree\nfour\n");
        fixture.write("src/b.rs", "five\nsix\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![index],
            identifier: "a".to_string(),
        };
        let map = |output: &str, source: &str, markers: Vec<Marker>| SourceMap {
            target: "rust".to_string(),
            output: output.to_string(),
            source: source.to_string(),
            hash: source_hash("x"),
            requirements: String::new(),
            answered: Vec::new(),
            signature: source_hash("i"),
            dependencies: BTreeMap::new(),
            generated: "2026-09-18T10:00:00Z".to_string(),
            markers,
        };
        let marker = |file: &str, end: usize| Marker {
            output_line: 1,
            file: file.to_string(),
            entity: Some("A".to_string()),
            line: 1,
            column: None,
            requirement: None,
            end,
        };
        let maps = vec![
            map("src/a.rs", "def/a.lfy", vec![marker("def/a.lfy", 4)]),
            // A shared output records every marker it holds, so the same region of
            // `src/a.rs` is named a second time.
            map(
                "src/a.rs",
                "def/b.lfy",
                vec![marker("def/a.lfy", 4)],
            ),
            map("src/b.rs", "def/b.lfy", vec![marker("def/b.lfy", 2)]),
        ];
        let request = review(&program, &plan, &batch, &maps, None, &[]);
        let text = &request.instructions;
        // `def/b.lfy` declares an `A` of its own, and none of its regions comes.
        assert!(!text.contains("src/b.rs"), "{text}");
        // The region of the unit's own file is named, once per map that records it, and the
        // hash is taken over its lines each time it is named.
        // @lfy def/generation/main.lfy:review#review:review:0f43e9772c4aad39520e42801704e6fe6e9115829e267701e251b74908849820
        assert_eq!(text.matches("`src/a.rs:1-4`").count(), 2, "{text}");
        let whole = "one\ntwo\nthree\nfour\n";
        let id = request.asked.first().expect("one criterion");
        assert_eq!(request.hashes[id], source_hash(&format!("{whole}{whole}")));
    }

    /// An entity moved into a unit is marked with the file that declares it, not with the
    /// unit's own file, so its regions are the ones that file's markers name.
    // @lfy def/generation/main.lfy:review#review:review:0f43e9772c4aad39520e42801704e6fe6e9115829e267701e251b74908849820
    #[test]
    fn a_review_holds_the_regions_of_an_entity_moved_into_the_unit() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write(
                "def/b.lfy",
                "d B: `A b` {\n  $x = string;\n  where (`x is set`) -> `B holds x`;\n}\n",
            )
            .write(
                "def/a.lfy",
                "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nd C {\n  $x = string;\n  \
                 where (`x is set`) -> `C holds x`;\n}\n",
            )
            .write(
                "def/c.lfy",
                "use \"./a\";\nuse \"./b\";\n\ntrait holder {\n  $c = C;\n}\nholder.apply(B);\n",
            );
        fixture.write("src/b.rs", "one\ntwo\nthree\nfour\n");
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, unit) = unit_of(&workspace, &plan, "def/b.lfy");
        // `C` moved into the unit of `def/b.lfy` from `def/a.lfy`.
        assert_eq!(names(&workspace, &unit.entities), ["B", "C"]);
        let batch = Batch {
            units: vec![index],
            identifier: "b".to_string(),
        };
        let marker = |file: &str, entity: &str, line: usize, end: usize| Marker {
            output_line: line,
            file: file.to_string(),
            entity: Some(entity.to_string()),
            line: 1,
            column: None,
            requirement: None,
            end,
        };
        let maps = vec![SourceMap {
            target: "rust".to_string(),
            output: "src/b.rs".to_string(),
            source: "def/b.lfy".to_string(),
            hash: source_hash("x"),
            requirements: String::new(),
            answered: Vec::new(),
            signature: source_hash("i"),
            dependencies: BTreeMap::new(),
            generated: "2026-09-18T10:00:00Z".to_string(),
            markers: vec![
                marker("def/b.lfy", "B", 1, 2),
                marker("def/a.lfy", "C", 3, 4),
            ],
        }];
        let request = review(&program, &plan, &batch, &maps, None, &[]);
        let text = &request.instructions;
        // Each entity is given the region its own declaring file marks, and nothing else.
        assert!(text.contains("`src/b.rs:1-2`"), "{text}");
        assert!(text.contains("`src/b.rs:3-4`"), "{text}");
        assert!(
            !text.contains("No region of any output was generated for it."),
            "{text}"
        );
        // Each entity's criteria are hashed over its own region alone.
        // @lfy def/generation/main.lfy:review#review:review:2406e70a6c25c47a8f50f45cfb28517ffe7990ce2a1647dd53f04024b5ea5269
        assert_eq!(request.asked.len(), 2, "{:?}", request.asked);
        let hashes: Vec<&String> = request
            .asked
            .iter()
            .map(|id| &request.hashes[id])
            .collect();
        assert_eq!(hashes[0], &source_hash("one\ntwo\n"));
        assert_eq!(hashes[1], &source_hash("three\nfour\n"));
    }

    /// An entity moved into a unit is declared in another file, so its criteria and tests
    /// are taken from the file that declares it and not from the unit's own lowered file,
    /// which holds no node for it.
    // @lfy def/generation/main.lfy:review#review:review:f3ebae56b7c2144a9edb3e3b69ac07f174e3974f258284e909f2e10068ec9831
    #[test]
    fn a_review_holds_the_criteria_of_an_entity_moved_into_the_unit() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/b.lfy", "d B: `A b` {\n  $x = string;\n}\n")
            .write(
                "def/a.lfy",
                "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nd C {\n  $x = string;\n  \
                 where (`x is set`) -> `C holds x`;\n}\n",
            )
            .write(
                "def/c.lfy",
                "use \"./a\";\nuse \"./b\";\n\ntrait holder {\n  $c = C;\n}\nholder.apply(B);\n",
            );
        let program = fixture.program();
        let workspace = program.workspace.clone();
        assert_bound(&workspace);
        let plan = plan_of(&program, &[], &[], &[]);
        let (index, unit) = unit_of(&workspace, &plan, "def/b.lfy");
        // `C` moved into the unit of `def/b.lfy` from `def/a.lfy`.
        assert_eq!(names(&workspace, &unit.entities), ["B", "C"]);
        let batch = Batch {
            units: vec![index],
            identifier: "b".to_string(),
        };
        let text = review(&program, &plan, &batch, &[], None, &[]).instructions;
        let at = text
            .find("#### `C`")
            .unwrap_or_else(|| panic!("C's section: {text}"));
        let section = &text[at..];
        let end = section[1..]
            .find("\n#### ")
            .or_else(|| section[1..].find("\n## "))
            .map_or(section.len(), |found| found + 1);
        let section = &section[..end];
        assert!(section.contains("C holds x"), "{section}");
    }

    /// With no source map for the batch the verifier is given the same instructions with no
    /// region, so it can only find the criteria unverifiable, and nothing is carried for an
    /// entity that generated none.
    // @lfy def/generation/main.lfy:review#review:review:27423e2750e221d170626aa6fcc294a7f735f5f6f23b78873b0ff4d956969feb
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
        // A record that would carry the criterion if its entity had a region carries
        // nothing, since there is no code whose sameness could stand for the verdict.
        // @lfy def/generation/main.lfy:review#review:review:944917284f9b6da4a52ad0fe6ba57ee0440f72ee59ecdfdc8b90585e6e1e7417
        let asked = review(&program, &plan, &batch, &[], None, &[]).asked;
        let records: Vec<ReviewRecord> = asked
            .iter()
            .map(|id| ReviewRecord {
                id: id.clone(),
                status: ReviewStatus::Satisfied,
                hash: source_hash(""),
            })
            .collect();
        let request = review(&program, &plan, &batch, &[], None, &records);
        assert_eq!(request.asked, asked);
        assert!(request.carried.is_empty(), "{:?}", request.carried);
        let text = &request.instructions;
        assert!(text.contains("def/a.lfy:6 — it does"), "{text}");
        // An entity with no region of its own hashes to the empty text.
        // @lfy def/generation/main.lfy:review#review:review:efbaab9e13ed7569408b035ce9b34392a9e0b4fa0a274b747ea27c0ef681cd65
        assert_eq!(request.hashes[&asked[0]], source_hash(""));
        // @lfy def/generation/main.lfy:review
        assert!(
            text.contains("No region of any output was generated for it."),
            "{text}"
        );
        assert!(text.contains("`ELFIE: REVIEWED`"), "{text}");
    }

    /// An output a source map names but that cannot be read adds nothing to the hash of the
    /// entity's criteria, though the region is named all the same.
    // @lfy def/generation/main.lfy:review#review:review:b20a4c21286c959c6a77873bc8d178ac43d720cb03877030c893be7f015f5903
    #[test]
    fn a_region_whose_output_cannot_be_read_adds_no_text_to_a_hash() {
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
        // Neither `src/a.rs` nor `src/tests.rs` was written, so neither can be read.
        let maps = two_source_maps();
        let request = review(&program, &plan, &batch, &maps, None, &[]);
        let text = &request.instructions;
        assert!(text.contains("`src/a.rs:20-40`"), "{text}");
        assert!(text.contains("`src/tests.rs:1-11`"), "{text}");
        let id = request.asked.first().expect("one criterion");
        assert_eq!(request.hashes[id], source_hash(""));
    }

    // -----------------------------------------------------------------------------------
    // globalReview
    // -----------------------------------------------------------------------------------

    /// One global criterion, whose id two outputs name in a marker: it is given once with
    /// its id and its place, then where each of its two regions is, then the protocol.
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:5e1c902e7ffa61797f046c11f8967257a21d3e532b41ab323a10168ce9b2902f
    #[test]
    fn the_global_review_gives_each_global_criterion_once_with_every_region() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write(
                "def/a.lfy",
                "d A { $x = string; }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written twice` });\n",
            )
            .write("src/a.rs", "alpha\nbeta\n")
            .write("src/b.rs", "gamma\ndelta\n");
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
            answered: vec![id.clone()],
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
        // Where each region is, one per output, with none of their lines.
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
        assert!(text.contains("`src/a.rs:1-2`"), "{text}");
        assert!(text.contains("`src/b.rs:1-2`"), "{text}");
        for line in ["alpha", "beta", "gamma", "delta"] {
            assert!(!text.contains(line), "{line} is quoted:\n{text}");
        }
        // The id is asked about, and nothing of a global criterion is ever carried.
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:3163f899ae0e06b99cc3cfbdf7857a8f62a41a8e5685323600713e1e6347797b
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e9cc8a4c5b45b3440c827bd67a1fd514c12870041202d0cd54fad21179548843
        assert_eq!(request.asked, std::slice::from_ref(&id));
        assert!(request.hashes.is_empty(), "{:?}", request.hashes);
        assert!(request.carried.is_empty(), "{:?}", request.carried);
        // Then the protocol, with the tools that are called before files are read by hand.
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
        assert!(text.contains("`ELFIE: REVIEWED`"), "{text}");
        assert!(text.contains("unverifiable, never violated"), "{text}");
        assert!(
            text.contains("before files are searched or read by hand"),
            "{text}"
        );
        for tool in [
            "`elfie_output`",
            "`elfie_report`",
            "`elfie_source`",
            "`elfie_entity`",
            "`elfie_references`",
        ] {
            assert!(text.contains(tool), "{tool} is missing:\n{text}");
        }
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
        // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:602d5d9913771ebb9a5ab23bc1b03ed23f9e22888e45cf65e826b7cfdbcc02e2
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

    /// One line of a verifier's report that gives the place of its criterion or test as well
    /// as its id, as the protocol asks for.
    fn placed_review_line(
        id: &str,
        file: &str,
        line: usize,
        status: &str,
        evidence: &str,
        note: &str,
    ) -> String {
        format!(
            "{{\"id\":\"{id}\",\"file\":\"{file}\",\"line\":{line},\"status\":\"{status}\",\
             \"evidence\":\"{evidence}\",\"note\":\"{note}\"}}"
        )
    }

    /// An id of the right shape that is the id of nothing and near nothing: the same id with
    /// its last hex digit changed, so it neither begins the real one nor is begun by it.
    fn mistyped_id(id: &str) -> String {
        let mut mistyped = id.to_string();
        let last = mistyped.pop().expect("an id is not empty");
        mistyped.push(if last == '0' { '1' } else { '0' });
        mistyped
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
        let found = review_of(&report, &program, &[], &[]);
        // Each line read beginning with `{` is a review line, and the reviews come in report
        // order.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:fdcd21d1f06a140e2bfae307be2625b9c27d43f2b73e320c1f152a39fd9638bc
        assert_eq!(found.reviews.len(), 2, "{found:?}");
        // A review line whose id is the id of a criterion of the program names it.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:4bf722eab5ca52f5732c6c9f8f080d2efd0dbed5df5ea0319fa4c4a56cade02c
        assert_eq!(found.reviews[0].status, ReviewStatus::Satisfied);
        assert_eq!(found.reviews[0].id, ids[0]);
        assert_eq!(found.reviews[0].evidence, "src/a.rs:1-30");
        assert_eq!(found.reviews[1].status, ReviewStatus::Violated);
        assert_eq!(found.reviews[1].id, ids[1]);
        // The place of each is derived from the origin of the criterion its id names: the
        // file it is written in, the line its `add` begins on, and the entity it is for.
        // @lfy def/generation/main.lfy:reviewOf
        assert_eq!(found.reviews[0].file, "def/a.lfy");
        assert_eq!(found.reviews[0].entity, "A");
        assert_eq!(found.reviews[0].line, 3);
        assert_eq!(found.reviews[1].line, 4);
        // The prose line is no problem, and neither is a blank one.
        // @lfy def/generation/main.lfy:reviewOf
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        // Lines after the end line are ignored.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:10e1a05e247051fa67e5af8f9351b3d6c08728d0642aa557109f7db27e01e1d6
        let found = review_of(&format!("{report}\nand that is all"), &program, &[], &[]);
        assert_eq!(found.reviews.len(), 2);
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        // Each line read has the whitespace around it removed, so an indented review line
        // is read like any other.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:2bf986f1d26195d0e45f531c18f47a790716471b351423d3a52d37d04c898921
        let found = review_of(
            &format!(
                "   {}   \n{REVIEWED}",
                review_line(&ids[0], "satisfied", "", "indented")
            ),
            &program,
            &[],
            &[],
        );
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert!(found.problems.is_empty(), "{:?}", found.problems);
    }

    /// An id copied a character short or long still names its criterion, as long as exactly
    /// one id of the program is near it.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:731b47ce4e9dbb551b71e00078efb0e67419160fb2a75436b70be87929a8b8a9
    #[test]
    fn an_id_a_character_short_or_long_names_its_criterion() {
        let (_fixture, program, ids) = a_with_two_criteria();
        let short = &ids[0][..ids[0].len() - 1];
        let long = format!("{}0", ids[1]);
        let report = format!(
            "{}\n{}\n{REVIEWED}",
            review_line(short, "satisfied", "", "short"),
            review_line(&long, "violated", "", "long"),
        );
        let found = review_of(&report, &program, &[], &[]);
        assert_eq!(found.reviews.len(), 2, "{found:?}");
        assert_eq!(found.reviews[0].id, ids[0]);
        assert_eq!(found.reviews[0].line, 3);
        assert_eq!(found.reviews[1].id, ids[1]);
        assert_eq!(found.reviews[1].line, 4);
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        // An id that shares only its receiver and its contributor is near nothing: the two
        // hashes have no sixteen hex digits in common.
        // @lfy def/generation/main.lfy:reviewOf
        let (receiver, contributor, _) = id_parts(&ids[0]).expect("an id has three parts");
        let far = review_line(
            &format!("{receiver}:{contributor}:00"),
            "satisfied",
            "",
            "far",
        );
        let found = review_of(&format!("{far}\n{REVIEWED}"), &program, &[], &[]);
        assert!(found.reviews.is_empty(), "{found:?}");
        assert_eq!(found.problems, [far]);
    }

    /// A review line with no id names the criterion that begins alone where its file and
    /// line say, whatever case its status is spelled in and whatever other keys it holds,
    /// and the prose and fences around it are no problems.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:5ccf168d438b1997f2ec7fd50ef09356d148a7940500a34b5a6b2fc51a43b52c
    #[test]
    fn a_review_line_with_no_id_is_placed_by_its_file_and_line() {
        let (_fixture, program, ids) = a_with_two_criteria();
        let report = format!(
            "All tests pass. Report follows.\n```\n\
             {{\"file\":\"def/a.lfy\",\"line\":3,\"entity\":\"A\",\"status\":\"Satisfied\",\
             \"evidence\":\"src/a.rs:1-9\",\"note\":\"ok\"}}\n\
             ```\n{REVIEWED}"
        );
        let found = review_of(&report, &program, &[], &[]);
        // A line read that does not begin with `{` is ignored, prose and fence alike.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:24c8a0d8aab48ace02aea35fdac897930db4e5ea8b8b4bd675b3bd00afefeac3
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        // The place is where exactly one criterion of the program begins, and the status
        // reads whatever its case; the entity key the verifier wrote is ignored.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ffcb26fa671fb0476f3effa3e247f546bf1144d511802cfdd3a726c5756fc018
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.reviews[0].id, ids[0]);
        assert_eq!(found.reviews[0].file, "def/a.lfy");
        assert_eq!(found.reviews[0].line, 3);
        assert_eq!(found.reviews[0].entity, "A");
        assert_eq!(found.reviews[0].status, ReviewStatus::Satisfied);
        assert_eq!(found.reviews[0].evidence, "src/a.rs:1-9");
        assert_eq!(found.reviews[0].note, "ok");
        // A review line missing its evidence and its note reads with both empty, and the
        // keys it holds beyond the four are ignored.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:03c66ae037b851c7cce98a476e8c2dc9fe0e1d51213bd0f884fca3d2bf00fdea
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9edd00519d199e54062049e61315587483989f62a46fc5a31c9be591c90931ce
        let found = review_of(
            &format!(
                "{{\"id\":\"{}\",\"status\":\"UNVERIFIABLE\",\"reason\":\"none\"}}\n{REVIEWED}",
                ids[1]
            ),
            &program,
            &[],
            &[],
        );
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.reviews[0].status, ReviewStatus::Unverifiable);
        assert_eq!(found.reviews[0].evidence, "");
        assert_eq!(found.reviews[0].note, "");
        assert!(found.problems.is_empty(), "{:?}", found.problems);
    }

    /// A review line whose status names no member of `ReviewStatus` is a problem, as
    /// written, while the prose around it is none.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:e4e51ceff95aa7cf3d202b2e42573e755ade4bc85b77d90b1621d3f425a5d109
    #[test]
    fn a_review_line_that_is_no_review_is_a_problem_as_written() {
        let (_fixture, program, ids) = a_with_two_criteria();
        let report = format!(
            "{}\nI think it is fine.\n{}\n{REVIEWED}",
            review_line(&ids[0], "maybe", "", "unsure"),
            review_line(&ids[1], "unverifiable", "", "no region"),
        );
        let found = review_of(&report, &program, &[], &[]);
        // A status that names no member of ReviewStatus is no review.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:065297aa8507d1cce029b56f7ee5ede6af8c5913fb55092cd6433f748dcff490
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.reviews[0].id, ids[1]);
        assert_eq!(found.reviews[0].line, 4);
        assert_eq!(found.reviews[0].status, ReviewStatus::Unverifiable);
        // A review line that is no review is a problem, as written; the prose line is none.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:065297aa8507d1cce029b56f7ee5ede6af8c5913fb55092cd6433f748dcff490
        assert_eq!(
            found.problems,
            [review_line(&ids[0], "maybe", "", "unsure")]
        );
        // A review line whose id names no criterion and no test of the program is a problem
        // too, and so is one that reads as no object at all.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ebd07d63053ee05f2507003716efdd9c78f20b38aecdf60cca2c1874d85612ac
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:cc8ee014686684d4965cbc76770ce45c98db561453c188050a7a3d3c4b345ad6
        let unknown = review_line("A:A:nowhere", "satisfied", "", "made up");
        let broken = "{\"id\":".to_string();
        let found = review_of(&format!("{unknown}\n{broken}\n{REVIEWED}"), &program, &[], &[]);
        assert!(found.reviews.is_empty(), "{found:?}");
        assert_eq!(found.problems, [unknown, broken]);
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
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:10e1a05e247051fa67e5af8f9351b3d6c08728d0642aa557109f7db27e01e1d6
        let found = review_of(&report, &program, &[], &[]);
        // Of two reviews naming the same criterion only the last is kept, at its own place.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:19f540298a118815926304ba19a124afbedf6dedfe2303d1b51cb9b16e308600
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.reviews[0].status, ReviewStatus::Violated);
        assert_eq!(found.reviews[0].note, "second");
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:b6eda5717d7bcd79108423e0c0c000b75e1bd73f8bd2a124b3cefcb31a67bae1
        assert_eq!(found.problems, ["the report did not end"]);
        // An empty report ends with nothing either.
        assert_eq!(
            review_of("", &program, &[], &[]),
            ReviewReport {
                reviews: Vec::new(),
                problems: vec!["the report did not end".to_string()],
                unplaced: Vec::new(),
                missing: Vec::new(),
            }
        );
        // The end line stands with whitespace around it.
        // @lfy def/generation/main.lfy:reviewOf
        let found = review_of(&format!("  {REVIEWED}  "), &program, &[], &[]);
        assert!(found.reviews.is_empty());
        assert!(found.problems.is_empty(), "{:?}", found.problems);

        // With no end line, review lines are read from every line of the report too, and
        // the prose among them is still no problem.
        // @lfy def/generation/main.lfy:reviewOf
        let found = review_of(
            &format!(
                "{}\nI had a look.",
                review_line(&ids[0], "satisfied", "", "ok")
            ),
            &program,
            &[],
            &[],
        );
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.problems, ["the report did not end"]);
    }

    /// A review line whose id names nothing of the program is still placed when its file and
    /// line say where exactly one criterion begins; one that claims a violation and places
    /// itself nowhere at all is kept apart as written, so that a violation is never lost to
    /// an id a verifier garbled; and each id the review was asked about that no review names
    /// is missing.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:d4b52386cd8dd2565497fdaaf59b6fcafe4852653e38d732b6f4a7f4cb2d42f2
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ac3a89b9ae9599493a551003577acdff2855f9342959c0d3bc606cddc53e939d
    #[test]
    fn a_violation_that_is_placed_nowhere_is_kept_apart() {
        let (_fixture, program, ids) = a_with_two_criteria();
        // The second criterion begins on line 4, and nothing else of the program begins
        // there, so the place names it though the id names nothing.
        let mistyped = placed_review_line(
            &mistyped_id(&ids[1]),
            "def/a.lfy",
            4,
            "violated",
            "src/a.rs:12-18",
            "returns undefined",
        );
        let garbled = review_line("A:A:77aa", "VIOLATED", "src/a.rs:3-3", "no such id");
        let report = format!("{mistyped}\n{garbled}\n{REVIEWED}");
        let found = review_of(&report, &program, &[], &[]);
        // A line whose id is the id of nothing and near nothing is placed by its file and
        // its line, at the criterion that begins alone there.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ffcb26fa671fb0476f3effa3e247f546bf1144d511802cfdd3a726c5756fc018
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.reviews[0].id, ids[1]);
        assert_eq!(found.reviews[0].file, "def/a.lfy");
        assert_eq!(found.reviews[0].line, 4);
        assert_eq!(found.reviews[0].entity, "A");
        assert_eq!(found.reviews[0].status, ReviewStatus::Violated);
        assert_eq!(found.reviews[0].note, "returns undefined");
        // The line that names nothing and gives no place is a problem, and, claiming a
        // violation, is kept apart as well.
        assert_eq!(found.problems, std::slice::from_ref(&garbled));
        assert_eq!(found.unplaced, std::slice::from_ref(&garbled));
        // Asked about nothing, nothing is missing.
        assert!(found.missing.is_empty(), "{:?}", found.missing);

        // Asked about both criteria, the one no review names is missing, in the order it
        // was asked.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:a7bd850a8fc6e8bf19c7c580ea931ed26ea1ea26c2cad6c2ea48096aa93c9535
        let asked = vec![ids[0].clone(), ids[1].clone()];
        let found = review_of(&report, &program, &asked, &[]);
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.missing, [ids[0].clone()]);
        assert_eq!(found.unplaced, [garbled]);
    }

    /// One review as the agent server records it when a verifier submits a verdict during
    /// the run.
    fn submitted(id: &str, line: usize, status: ReviewStatus, note: &str) -> Review {
        Review {
            id: id.to_string(),
            file: "def/a.lfy".to_string(),
            line,
            entity: "A".to_string(),
            status,
            evidence: String::new(),
            note: note.to_string(),
        }
    }

    /// A verdict the verifier submitted through the agent server wins over a line of the
    /// report for the same criterion, and that line is neither a review nor a problem.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:e6ffb38e943b879b8a71a248dd7fd22884f5aa51001084248c3eb7c90b3eda91
    #[test]
    fn a_submitted_verdict_wins_over_a_line_of_the_report() {
        let (_fixture, program, ids) = a_with_two_criteria();
        let report = format!(
            "{}\n{REVIEWED}",
            review_line(&ids[0], "satisfied", "", "from the report")
        );
        let recorded = vec![submitted(&ids[0], 3, ReviewStatus::Violated, "submitted")];
        let asked = vec![ids[0].clone(), ids[1].clone()];
        let found = review_of(&report, &program, &asked, &recorded);
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9ebef2c6f21093486b00c8a0124de7e4c4262014632c2a66b0b47702c693b59d
        assert_eq!(found.reviews.len(), 1, "{found:?}");
        assert_eq!(found.reviews[0].status, ReviewStatus::Violated);
        assert_eq!(found.reviews[0].note, "submitted");
        // The line of the report for the same criterion is dropped, not kept apart.
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:42415f0b3dec0f93094d195e8abb7b6b4c6f2a363c4db117d8c0c9e8b3c5417e
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        assert!(found.unplaced.is_empty(), "{:?}", found.unplaced);
        // The criterion neither the run nor the report answered for is missing.
        assert_eq!(found.missing, [ids[1].clone()]);
    }

    /// With every verdict submitted during the run, a report of nothing but its end line
    /// gives those verdicts in the order they were recorded.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ac6ab38189abc2b73e52041d213cdb86de455386497b27bce939a60307b57adc
    #[test]
    fn the_submitted_verdicts_come_in_the_order_they_were_recorded() {
        let (_fixture, program, ids) = a_with_two_criteria();
        let recorded = vec![
            submitted(&ids[1], 4, ReviewStatus::Satisfied, "second criterion"),
            submitted(&ids[0], 3, ReviewStatus::Unverifiable, "first criterion"),
        ];
        let asked = vec![ids[0].clone(), ids[1].clone()];
        // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9ebef2c6f21093486b00c8a0124de7e4c4262014632c2a66b0b47702c693b59d
        let found = review_of(REVIEWED, &program, &asked, &recorded);
        assert_eq!(
            found
                .reviews
                .iter()
                .map(|review| (review.id.as_str(), review.status))
                .collect::<Vec<_>>(),
            [
                (ids[1].as_str(), ReviewStatus::Satisfied),
                (ids[0].as_str(), ReviewStatus::Unverifiable),
            ]
        );
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        assert!(found.missing.is_empty(), "{:?}", found.missing);
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
            &[],
            &[],
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

    // @lfy def/generation/data.lfy:Marker#Marker:Marker:55eeb537f74d15d3b738387a76843bfe4e688258322b57eb5046ead0be1e71dc
    #[test]
    fn only_a_lfy_path_and_a_line_or_an_identifier_path_is_read_as_a_marker() {
        // Text after `@lfy` that is not a space and a `.lfy` path, a colon, and a number or
        // an identifier path is prose: `@lfydef/a.lfy:4` has no space, and none of these has
        // the rest.
        assert!(parse_markers("// @lfydef/a.lfy:4").is_empty());
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
        // @lfy def/generation/data.lfy:Marker#Marker:Marker:e5a2dc69b3714f2fe9ec245e19aa4b7d2bc03926cf7410cabc1e0a7ed86cf86c
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
                answered: vec!["A:A:abc".to_string()],
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
                answered: Vec::new(),
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

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn stems_and_paths() {
        assert_eq!(stem_of("def/a.lfy", "def"), "a");
        assert_eq!(stem_of("def/deep/inner.lfy", "def/"), "deep/inner");
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

    /// A stem is the file's path from the source directory, `..` segments included, and
    /// never the whole path of a file that lies outside it.
    // @lfy def/generation/data.lfy:Unit#Unit:Unit:1fc2e33009d6c55647083813e41979cdcd5f51fe2a856a9f6815fb4aa2b68bd3
    #[test]
    fn a_units_stem_is_its_path_relative_to_the_source_directory() {
        assert_eq!(stem_of("def/a.lfy", "def"), "a");
        assert_eq!(stem_of("def/deep/inner.lfy", "def"), "deep/inner");
        assert_eq!(stem_of("other/x.lfy", "def"), "../other/x");
        assert_eq!(stem_of("x.lfy", "def"), "../x");
        assert_eq!(stem_of("src/a/x.lfy", "src/b"), "../a/x");
        assert_eq!(stem_of("a.lfy", ""), "a");
        // The same path as the source directory names the place itself.
        assert_eq!(path_relative("def", "def").as_deref(), Some("."));
        // Nothing relates an absolute path to a relative one.
        assert_eq!(path_relative("/def/a.lfy", "def"), None);
        assert_eq!(path_relative("/def/a.lfy", "/def").as_deref(), Some("a.lfy"));
    }
}
