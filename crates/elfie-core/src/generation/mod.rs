//! Compiled from `def/generation/main.lfy`: what a dependent may rely on, planning what
//! to generate, asking for it, reading how a run ended, and accepting what comes back.
//!
//! A unit is one source file for one target: outputs and source maps are per file, a
//! declaration's scope is its file, and a smaller unit would make the compiler merge
//! into a file it does not own, which is itself an act of generation. A larger unit
//! would not fit a request. The one exception is a cycle among files, which no order of
//! units can build: its entities move whole into the unit of the lowest file of the cycle,
//! as a [`Move`] records. Units are compiled one at a time, in plan order, and every
//! request, review request, and acceptance is about one unit; a request puts first what is
//! the same for every unit of a target, so that context is reused from a prompt cache.
//! [`interface_of`] is the text whose hash tells a dependent whether anything it may rely
//! on changed; [`plan`] finds every unit of a workspace, which need generating, and the
//! order they are compiled in; [`request`] gathers everything the compiler is handed for one unit,
//! with every criterion and test already resolved so that a compiler with no access to
//! the model can still work; [`outcome_of`] reads how a run ended, from the [`Submission`]
//! the compiler made or from its report; [`accept`] checks
//! outputs structurally, normalizes their markers, and gives them their source maps.
//! [`regions_of`] finds the generated regions one entity's name owns, [`changes`] what
//! differs for each entity of a unit since its outputs were accepted, [`reworded`] which of
//! its criteria and tests took a new id only because their text changed and [`rename`]
//! moves the markers of those ids, [`review`] what an
//! independent verifier is handed for one unit, [`global_review`] what it is handed for
//! every global criterion and test, and [`review_of`] what it found. [`source_maps_of`]
//! reads the source maps a workspace recorded and [`record`] writes those of one accepted
//! unit; [`reviews_of`] reads the verdicts one unit's criteria and tests were last given
//! and [`record_reviews`] writes them, so that a criterion whose code has not changed since
//! it was found satisfied is carried rather than reviewed again. [`relocate`] gives the code
//! of an entity whose declaration moved to another file to that file's unit.
//! Nothing here shells out.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod data; // @lfy def/generation/data.lfy:Plan
pub mod previous; // @lfy def/generation/previous.lfy:previousOf

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

// ---------------------------------------------------------------------------------------
// the submodules of this unit
// ---------------------------------------------------------------------------------------

mod accept;
mod compile_data;
mod interface;
mod mapped;
mod outcome;
mod plan;
mod relocate;
mod request;
mod review;

pub use accept::accept;
pub use compile_data::{
    map_file, record, record_reviews, rederive, review_file, reviews_of, source_maps_of,
};
pub use interface::interface_of;
pub use mapped::{changes, previous_of, regions_of, rename, reworded};
pub use outcome::outcome_of;
pub use plan::plan;
pub use relocate::relocate;
pub use request::{guidance_for, request};
pub use review::{global_review, review, review_of};

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
// interface text
// ---------------------------------------------------------------------------------------

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
// names and places
// ---------------------------------------------------------------------------------------

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
// units, cycles, and batching
// ---------------------------------------------------------------------------------------

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

/// The items in an order that places each after its dependencies, otherwise in the order
/// given. Items in a cycle keep the order given.
// @lfy def/generation/main.lfy:plan#plan:plan:c6bed32df9850fc2409ccc08a49904114e67a864cad86e4c61b6b0addd1d0dab
fn order_after_dependencies(dependencies: &[Vec<usize>]) -> Vec<usize> {
    let count = dependencies.len();
    // What each item reaches, directly or through other items.
    let reach: Vec<Vec<bool>> = (0..count)
        .map(|item| {
            let mut seen = vec![false; count];
            let mut stack = dependencies[item].clone();
            while let Some(next) = stack.pop() {
                if !seen[next] {
                    seen[next] = true;
                    stack.extend(&dependencies[next]);
                }
            }
            seen
        })
        .collect();
    // An item and every other item it shares a cycle with, in the order given. The cycles
    // form no cycle among themselves, so a cycle is placed whole once every item outside it
    // that one of its items depends on is placed.
    fn visit(
        item: usize,
        dependencies: &[Vec<usize>],
        reach: &[Vec<bool>],
        placed: &mut [bool],
        order: &mut Vec<usize>,
    ) {
        if placed[item] {
            return;
        }
        let cycle: Vec<usize> = (0..dependencies.len())
            .filter(|&other| other == item || (reach[item][other] && reach[other][item]))
            .collect();
        for &member in &cycle {
            placed[member] = true;
        }
        for &member in &cycle {
            for &dependency in &dependencies[member] {
                if !cycle.contains(&dependency) {
                    visit(dependency, dependencies, reach, placed, order);
                }
            }
        }
        order.extend(cycle);
    }
    let mut placed = vec![false; count];
    let mut order = Vec::with_capacity(count);
    for item in 0..count {
        visit(item, dependencies, &reach, &mut placed, &mut order);
    }
    order
}

/// `Unit.stem`: the name its file goes by in the program, which the loader made unique
/// among the files and spelled without the `.lfy` extension.
// @lfy def/generation/main.lfy:plan
// @lfy def/generation/data.lfy:Unit#Unit:Unit:544d21ab36e8de59ea9fcf77ae7fa34947e690a8e2c4c41f740f290b2efeb354
fn stem_of(file: &File) -> String {
    file.name.clone()
}

// ---------------------------------------------------------------------------------------
// markers and the regions they begin
// ---------------------------------------------------------------------------------------

/// Whether a marker is one unit's own: it names the unit's file, or it names an entity moved
/// into the unit and the file that declares that entity.
///
/// A marker naming the file an entity moved out of is that unit's own only for the entities
/// that moved in: the file keeps a unit of its own, which still answers for everything that
/// stayed there.
// @lfy def/generation/data.lfy:SourceMap#SourceMap:SourceMap:ebcb994cc8d4e88d7422b2773d618db5d2d5c753477db1c16b1b11635b80e3fb
fn is_own_marker(workspace: &Workspace, plan: &Plan, unit: usize, marker: &Marker) -> bool {
    let model = &workspace.model;
    let planned = &plan.units[unit];
    if marker.file == workspace.files[planned.file].path {
        return true;
    }
    plan.moves
        .iter()
        .filter(|moved| moved.into == unit)
        .flat_map(|moved| moved.entities.iter())
        .any(|&entity| {
            if declaring_path(model, entity) != Some(marker.file.as_str()) {
                return false;
            }
            match &marker.entity {
                // The entity itself, or one of its members, which a marker names as the
                // owner's identifier, a dot, and the member's name.
                Some(name) => {
                    let own = qualified_name(model, entity);
                    *name == own
                        || name
                            .strip_prefix(&own)
                            .is_some_and(|rest| rest.starts_with('.'))
                }
                None => declaration_lines(model, entity)
                    .is_some_and(|(first, last)| (first..=last).contains(&marker.line)),
            }
        })
}

/// The unit's own region text of one output: the text of the regions its own markers begin,
/// in output order, each region's lines stripped as a review strips a region, so that the
/// hash a source map records is of the code this unit wrote and of nothing another unit put
/// in the same file.
// @lfy def/generation/data.lfy:SourceMap#SourceMap:SourceMap:09446c0cd62e3661c21774df9093f0d0bcabcf6e89e04299d1c69e8ff57564e5
fn own_region_text(
    workspace: &Workspace,
    plan: &Plan,
    unit: usize,
    marker_comment: &str,
    text: &str,
    markers: &[Marker],
) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    for marker in markers {
        if is_own_marker(workspace, plan, unit, marker) {
            push_region(&lines, marker, marker_comment, &mut out);
        }
    }
    out
}

/// One region's lines of its output, each with the marker comment it carries and the white
/// space before it taken off, every line that is then empty left out, and each line followed
/// by a line break.
// @lfy def/generation/data.lfy:SourceMap#SourceMap:SourceMap:09446c0cd62e3661c21774df9093f0d0bcabcf6e89e04299d1c69e8ff57564e5
// @lfy def/generation/main.lfy:review#review:review:cd1ebbdd30f45c288876eb3c3e144180848e744373e0780577158f70511f6d01
fn push_region(lines: &[&str], marker: &Marker, marker_comment: &str, out: &mut String) {
    let last = marker.end.min(lines.len());
    let first = marker.output_line.saturating_sub(1).min(last);
    for line in &lines[first..last] {
        let kept = without_marker_comment(line, marker_comment);
        if kept.is_empty() {
            continue;
        }
        out.push_str(kept);
        out.push('\n');
    }
}

/// One line of an output without the marker comment it carries: everything from the comment
/// opener that a marker follows to the end of the line, and the white space before that
/// opener, taken off. A line that carries none comes back whole.
///
/// The opener is looked for before each marker rather than at the start of the line, so a
/// comment that is prose stays and a marker trailing code takes only itself away.
// @lfy def/generation/main.lfy:review#review:review:cd1ebbdd30f45c288876eb3c3e144180848e744373e0780577158f70511f6d01
fn without_marker_comment<'l>(line: &'l str, marker_comment: &str) -> &'l str {
    if marker_comment.is_empty() {
        return line;
    }
    let mut from = 0;
    while let Some(position) = line[from..].find(MARKER_PREFIX) {
        let at = from + position;
        // The comment opener of this marker: the last one before it, which the marker is
        // written after as the guidance says.
        if let Some(opener) = line[..at].rfind(marker_comment) {
            return line[..opener].trim_end();
        }
        from = at + MARKER_PREFIX.len();
    }
    line
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

/// Whether a unit is the owner of a foreign edit: the edit names its stem as the owner and
/// lies under its target's output directory, so an edit recorded for another target, whose
/// stems may agree, is not its.
// @lfy def/generation/main.lfy:plan#plan:plan:f94bebcadb345989b89582a2716579351dbfc0c4b56c770f7661f54fe2e65be3
// @lfy def/generation/main.lfy:request#request:request:56b3e8ed8845880a5efc54ee766d63a577e1be64b2614849b89bca97e2254907
fn owns(unit: &Unit, target: &Target, edit: &ForeignEdit) -> bool {
    edit.owner == unit.stem && under(&edit.path, &target.output_directory)
}

/// The stub line of a target: the value the setters of its declaration give the member
/// `stubLine` of `targetLanguage`, read as [`Target::marker_comment`] is read from
/// `markerComment`; `None` when no setter gives it one, and then no line is a stub line.
// @lfy def/generation/main.lfy:request#request:request:5367b0ce97e471a1ab266e29356a00115ec74e7fcc5ace4805721d77194a180e
// @lfy def/generation/main.lfy:accept#accept:accept:ec9bf2b28c1f3b0e6144d76c6bcf7912791a9eb3f8d41f1c5f2ef658d041f34a
fn stub_line_of(model: &Model, target: &Target) -> Option<String> {
    model.entities[target.declaration]
        .value("stubLine")
        .and_then(model::Value::as_str)
        .map(str::to_string)
}

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

/// A path relative to a directory it is under, both relative to the root.
fn strip_directory<'p>(path: &'p str, directory: &str) -> Option<&'p str> {
    if is_root(directory) {
        return Some(path);
    }
    let directory = directory.trim_end_matches('/');
    path.strip_prefix(directory)?.strip_prefix('/')
}

// ---------------------------------------------------------------------------------------
// what the tests of this unit are built from
// ---------------------------------------------------------------------------------------

#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(test)]
use crate::workspace::load;

/// A project directory under the system's temporary directory, removed when dropped.
#[cfg(test)]
struct Fixture {
    root: PathBuf,
}

#[cfg(test)]
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
                r#"{ "dependencies": { "elfie": "./lib" } }"#,
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

#[cfg(test)]
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The main file of the package `elfie`: the prelude, which brings the vocabulary a
/// target is declared with into the scope of every file of the project.
#[cfg(test)]
const LIBRARY: &str = "use \"./prelude/builtin\";\nuse \"./prelude/entity\";\n\
                       use \"./prelude/trait\";\nuse \"./criteria/main\";\n\
                       use \"./target/main\";\n";
/// The trait a builtin carries, which the library's own declarations need.
#[cfg(test)]
const LIBRARY_BUILTIN: &str = "trait builtin: `Performed by the compiler` { }\n";
/// The context layer every entity is seen through: the lists a body adds to live
/// there, and `apply` is a member of a trait's kind rather than a name nothing
/// declares.
#[cfg(test)]
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
#[cfg(test)]
const LIBRARY_TRAIT: &str = "use \"./builtin\";\n\
                             fn layer(subject: trait, ...arguments: string[]) is builtin: \
                             `A trait chosen as a layer of a target` => object;\n";
/// The enums a knowledge item and a command name.
#[cfg(test)]
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
#[cfg(test)]
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
  $stubLine: `The statement that replaces a body to stub it` = string | undefined;
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
#[cfg(test)]
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

/// The same roles with a stub line set on the language.
#[cfg(test)]
const STUBBED_ROLES: &str = "\
trait lang extends targetLanguage: `Rust` {
  .fileExtension = \"rs\";
  .markerComment = \"//\";
  .stubLine = 'todo!(\"elfie stub: {{unit}}: {{reason}}\")';
}
trait flat extends targetLayout: `One file per unit` { }
trait cargo extends targetEcosystem: `Crates from crates.io` {
  .ecosystem = \"cargo\";
}
";

/// The stub line the stubbed roles set, as the target spells it.
#[cfg(test)]
const STUB_LINE: &str = "todo!(\"elfie stub: {{unit}}: {{reason}}\")";

/// The `ace const` holding the target `rust`, added to `global` so that every entity
/// of every project file is built for it.
#[cfg(test)]
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

#[cfg(test)]
const A: &str = "use \"./b\";\n\n/// A record.\nd A: `An a` {\n  $b = B;\n}\n";
#[cfg(test)]
const B: &str = "d B: `A b` {\n  $x = string;\n}\n\nfn make(x: string): `Makes a b` => B {\n  @acceptanceCriteria.add({ situation = `x is empty`, behavior = `the b holds x` });\n  @test({ input = [\"y\"], expect = B@like(`holding y`) });\n}\n";

/// The same target declaration with nothing added to `global`, so that only an entity
/// or a file that holds the target itself is built for it.
#[cfg(test)]
fn without_global(targets: &str) -> String {
    targets.replace("global@targets.add(rust);\n", "")
}

/// The project of the definition's tests: `def/a.lfy` using `./b`, and `def/b.lfy`.
#[cfg(test)]
fn a_and_b() -> Fixture {
    let fixture = Fixture::with_rust_target();
    fixture.write("def/a.lfy", A).write("def/b.lfy", B);
    fixture
}

#[cfg(test)]
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
#[cfg(test)]
fn plan_of(
    program: &Program,
    maps: &[SourceMap],
    requested: &[String],
    violated: &[String],
) -> Plan {
    let (lowered, plan) = plan(program.workspace.clone(), maps, requested, violated, &[]);
    assert_eq!(
        lowered.files.iter().map(|file| &file.text).collect::<Vec<_>>(),
        program.files.iter().map(|file| &file.text).collect::<Vec<_>>()
    );
    plan
}

#[cfg(test)]
fn unit_of<'p>(workspace: &Workspace, plan: &'p Plan, path: &str) -> (usize, &'p Unit) {
    let file = file_index(workspace, path);
    let index = plan
        .unit_of(0, file)
        .unwrap_or_else(|| panic!("{path} has no unit: {:?}", plan.units));
    (index, &plan.units[index])
}

#[cfg(test)]
fn names(workspace: &Workspace, entities: &[EntityId]) -> Vec<String> {
    entities
        .iter()
        .map(|&entity| entity_name(&workspace.model, entity))
        .collect()
}

/// One source map per unit of a plan, recording that unit's lowered hash, its
/// requirements hash, its interface signature, and the signature of each of its
/// dependencies as they are now: what acceptance would have written.
#[cfg(test)]
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
                text: String::new(),
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

#[cfg(test)]
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

/// `def/b.lfy` declares `B`; `def/a.lfy` uses it and declares `A`, whose member is a
/// `B`, and `C`; `def/c.lfy` applies to `B` a trait adding a member of type `C`. So `A`
/// reaches `B` and `B` reaches `C`, and the two units reach each other. `def/b.lfy` is
/// the lowest file of the cycle, since `def/a.lfy` uses it and it uses nothing, so `C`
/// moves into its unit.
#[cfg(test)]
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

/// `def/a.lfy` declaring a data `A` with a member `x` on lines 1 to 3, and a fn `f`
/// from line 5 on.
#[cfg(test)]
const A_WITH_A_MEMBER_AND_A_FN: &str = "d A: `An a` {\n  $x = string;\n}\n\nfn f(): `Does it` => string {\n  @acceptanceCriteria.add({ behavior = `it does` });\n}\n";

#[cfg(test)]
fn a_with_a_member_and_a_fn() -> Fixture {
    let fixture = Fixture::with_rust_target();
    fixture.write("def/a.lfy", A_WITH_A_MEMBER_AND_A_FN);
    fixture
}

/// One marker naming an entity.
#[cfg(test)]
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
#[cfg(test)]
fn two_source_maps() -> Vec<SourceMap> {
    let map = |output: &str, markers: Vec<Marker>| SourceMap {
        target: "rust".to_string(),
        output: output.to_string(),
        source: "def/a.lfy".to_string(),
        hash: source_hash("x"),
        requirements: String::new(),
        answered: Vec::new(),
        signature: source_hash("i"),
        text: String::new(),
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

/// One marker of a recorded map, naming an entity of a file of the program.
#[cfg(test)]
fn marker_of(file: &str, entity: &str, output_line: usize, end: usize) -> Marker {
    Marker {
        output_line,
        file: file.to_string(),
        entity: Some(entity.to_string()),
        line: 1,
        column: None,
        requirement: None,
        end,
    }
}

/// One source map of the target `rust` for an output and the source it came from.
#[cfg(test)]
fn map_for(output: &str, source: &str) -> SourceMap {
    SourceMap {
        target: "rust".to_string(),
        output: output.to_string(),
        source: source.to_string(),
        hash: source_hash(source),
        requirements: source_hash("requirements"),
        answered: Vec::new(),
        signature: source_hash("interface"),
        text: source_hash(output),
        dependencies: BTreeMap::new(),
        generated: "2026-09-18T10:00:00Z".to_string(),
        markers: Vec::new(),
    }
}

#[cfg(test)]
fn outputs_of(maps: &[SourceMap]) -> Vec<&str> {
    maps.iter().map(|map| map.output.as_str()).collect()
}

#[cfg(test)]
mod accept_tests;
#[cfg(test)]
mod compile_data_tests;
#[cfg(test)]
mod interface_tests;
#[cfg(test)]
mod mapped_tests;
#[cfg(test)]
mod marker_tests;
#[cfg(test)]
mod outcome_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod relocate_tests;
#[cfg(test)]
mod request_tests;
#[cfg(test)]
mod review_tests;
