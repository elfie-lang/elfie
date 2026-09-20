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
//! Nothing here shells out.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod data; // @lfy def/generation/main.lfy:6

pub use data::*;

use sha2::{Digest, Sha256};
use unicode_ident::{is_xid_continue, is_xid_start};

use crate::model::{
    self, Criterion, Entity, EntityId, EntityKind, FileId, Model, NodeRef, SymbolKind,
};
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

// ---------------------------------------------------------------------------------------
// interfaceOf
// ---------------------------------------------------------------------------------------

/// What dependents of a unit may rely on, as one text whose hash tells whether it changed.
///
/// One line per entity of the unit in file order — its identifier, its kind, its
/// definition, its type, and for a fn its parameters with their types and its output —
/// and, after the line of a data, type, trait, or enum, one line per member or enum
/// member it declares, since a dependent relies on those as much as on the declaration
/// itself. Nothing else is written: a change that leaves every line the same, such as a
/// new criterion or a moved line, leaves the text the same, so dependents are not
/// regenerated for it.
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
/// depends on nothing but what a dependent may rely on.
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
/// that has a package gives no unit, since a package is compiled by its own project.
/// Units come in target order then file order, each after its dependencies. A unit's
/// reason is the first of: requested, fresh, changed, dependency; `None` when it is up to
/// date. The caller drops maps whose output no longer exists before calling.
// @lfy def/generation/main.lfy:plan
pub fn plan(workspace: &Workspace, source_maps: &[SourceMap], requested: &[String]) -> Plan {
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
            let entities = built_entities(model, file.source, target.marker);
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
        units[index].reason = reason_of(workspace, &units, &signatures, index, requested);
    }

    let batches = batches_of(workspace, &units);
    Plan { units, batches }
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

/// Why a unit is planned: the first of requested, fresh, changed, dependency that holds.
// @lfy def/generation/main.lfy:plan
fn reason_of(
    workspace: &Workspace,
    units: &[Unit],
    signatures: &[String],
    index: usize,
    requested: &[String],
) -> Option<Reason> {
    let unit = &units[index];
    let file = &workspace.files[unit.file];
    if requested.iter().any(|r| r == &file.path || r == &unit.stem) {
        return Some(Reason::Requested);
    }
    if unit.outputs.is_empty() {
        return Some(Reason::Fresh);
    }
    let hash = source_hash(&source_text(workspace, file));
    if unit.outputs.iter().any(|output| output.hash != hash) {
        return Some(Reason::Changed);
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
// @lfy def/generation/main.lfy:plan
fn batches_of(workspace: &Workspace, units: &[Unit]) -> Vec<Batch> {
    // Decision: the criteria give one guidance and one set of native dependencies per
    // request, so a batch holds units of one target; two targets never share a batch even
    // when their stems agree.
    let mut batches: Vec<Batch> = Vec::new();
    let mut open_characters = 0usize;
    for (index, unit) in units.iter().enumerate() {
        if unit.reason.is_none() {
            continue; // @lfy def/generation/main.lfy:plan
        }
        let characters = source_text(workspace, &workspace.files[unit.file])
            .chars()
            .count();
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
/// The sources are the joined raw text of each unit's tokens by path; `existing` is kept
/// only where its path is among the outputs of a unit of the batch; there is one
/// interface per dependency of the batch that is not itself in it; the guidance is every
/// criterion of the target's marker; and the native dependencies are the workspace's
/// followed by the target package's. The instructions quote every criterion and test
/// already resolved, so a compiler with no access to the model can still work.
// Decision: the definition passes the plan and the batch; a plan here does not own its
// workspace, so the workspace is an extra first parameter.
// @lfy def/generation/main.lfy:request
pub fn request(
    workspace: &Workspace,
    plan: &Plan,
    batch: &Batch,
    existing: &[Output],
    previous: &BTreeMap<String, String>,
) -> Request {
    let model = &workspace.model;
    let Some(&first) = batch.units.first() else {
        // Decision: an empty batch has no target, so there is nothing to say; the caller
        // never builds one, since `plan` only batches units it planned.
        return Request {
            batch: batch.clone(),
            instructions: String::new(),
            sources: BTreeMap::new(),
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
            let file = &workspace.files[plan.units[index].file];
            (file.path.clone(), source_text(workspace, file))
        })
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
    let guidance = model::criteria_of(model, target.marker); // @lfy def/generation/main.lfy:request
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
        workspace,
        plan,
        batch,
        target,
        &sources,
        previous,
        &existing,
        &interfaces,
        &guidance,
        &native_dependencies,
    );

    Request {
        batch: batch.clone(),
        instructions,
        sources,
        previous: previous.clone(),
        existing,
        interfaces,
        guidance,
        native_dependencies,
    }
}

/// The prompt: what to produce, where, the rules for producing it, and how to report the
/// outcome, in the order the definition lists.
// @lfy def/generation/main.lfy:request
#[allow(clippy::too_many_arguments)]
fn instructions(
    workspace: &Workspace,
    plan: &Plan,
    batch: &Batch,
    target: &Target,
    sources: &BTreeMap<String, String>,
    previous: &BTreeMap<String, String>,
    existing: &BTreeMap<String, String>,
    interfaces: &[Interface],
    guidance: &[Criterion],
    native_dependencies: &[NativeDependency],
) -> String {
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
        "The target's marker `{}` gives this guidance for building against the target:\n",
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
        let _ = writeln!(out, "### `{}` (stem `{}`)\n", file.path, unit.stem);
        if let Some(source) = sources.get(&file.path) {
            out.push_str("Source:\n\n```elfie\n");
            out.push_str(source);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        if unit.entities.is_empty() {
            out.push_str("The unit has no entities.\n\n");
        }
        for &entity in &unit.entities {
            write_entity(&mut out, model, entity);
        }
    }

    // The rules for kinds. @lfy def/generation/main.lfy:request
    out.push_str("## Rules for kinds\n\n");
    out.push_str(
        "- A DataDeclaration becomes a type.\n\
         - An AgentFunctionDeclaration becomes a function whose body satisfies every criterion and test.\n\
         - A FunctionDeclaration is translated statement by statement with nothing added.\n\
         - A TraitDeclaration emits nothing of its own; its members and criteria belong to each entity that carries it.\n\
         - A TypeDeclaration and an EnumDeclaration become their nearest equivalents.\n\
         - An ExternalDeclaration is bound to what the guidance says.\n\
         - An Ace statement is evaluated during compilation and its results inlined where used.\n\
         - A Use is satisfied by the interfaces.\n\n",
    );

    // Markers. @lfy def/generation/main.lfy:request
    out.push_str("## Markers\n\n");
    out.push_str(
        "Every emitted item and every test carries a marker naming the entity it comes from. A marker is \
         written as a line comment of the target's language reading `@lfy`, a space, the source path \
         relative to the root, a colon, and the name of the entity: its identifier, or its owner's \
         identifier, a dot, and a member's name. Never write a line number.\n\n",
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
         test named after its entity, placed where the guidance says.\n\n",
    );

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

/// One entity of a unit: its source text, its kind, and every criterion and test resolved
/// for it.
// @lfy def/generation/main.lfy:request
fn write_entity(out: &mut String, model: &Model, entity: EntityId) {
    let record = &model.entities[entity];
    let _ = writeln!(
        out,
        "#### `{}` ({})\n",
        entity_name(model, entity),
        kind_text(record)
    );
    if let Some(definition) = &record.definition {
        let _ = writeln!(out, "Definition: {definition}\n");
    }
    if let Some(ty) = &record.ty {
        let _ = writeln!(out, "Type: `{}`\n", model::type_text(model, ty));
    }
    if let Some(node) = record.node {
        out.push_str("Declaration:\n\n```elfie\n");
        out.push_str(&declaration_text(model, node));
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }
    let criteria = model::criteria_of(model, entity);
    out.push_str("Criteria:\n");
    if criteria.is_empty() {
        out.push_str("(none)\n");
    }
    for criterion in &criteria {
        write_criterion(out, criterion);
    }
    out.push('\n');
    out.push_str("Tests:\n");
    if record.tests.is_empty() {
        out.push_str("(none)\n");
    }
    for test in &record.tests {
        let _ = writeln!(
            out,
            "- Input `{}` gives `{}`",
            test.input_text.trim(),
            test.expect_text.trim()
        );
    }
    out.push('\n');
}

/// One criterion as a list item: situations, then behaviors, then side effects.
fn write_criterion(out: &mut String, criterion: &Criterion) {
    let mut parts = Vec::new();
    if let Some(situation) = &criterion.situation {
        parts.push(format!("When {}", situation.join(" ")));
    }
    if let Some(behavior) = &criterion.behavior {
        parts.push(behavior.join(" "));
    }
    if let Some(side_effects) = &criterion.side_effects {
        parts.push(format!("Side effects: {}", side_effects.join(" ")));
    }
    let _ = writeln!(out, "- {}", parts.join(": "));
}

/// The source text of a declaration: its documentation, then the node's own text.
fn declaration_text(model: &Model, node: NodeRef) -> String {
    let tree = &model.sources[node.file].tree;
    let declaration = model.node(node);
    let mut text = String::new();
    for documentation in &declaration.documentation {
        text.push_str(&tree.raw(documentation.start, documentation.end));
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    text.push_str(&model.raw(node));
    text
}

/// The identifier of an entity, or `anonymous`.
fn entity_name(model: &Model, entity: EntityId) -> String {
    model.entities[entity]
        .identifier
        .clone()
        .unwrap_or_else(|| "anonymous".to_string())
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
/// rejected verdict as the message; otherwise it was accepted, with an empty message.
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
    if verdicts.is_empty() {
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
/// output directory, when a marker names an entity that its file does not declare or a
/// line of the unit's file beyond its last, or when an entity of the unit has no marker
/// naming it or a line its declaration covers. A marker naming a line that falls inside a
/// declaration is rewritten to name that entity, since a name survives edits and a line
/// does not. A marker naming a file that is not in the program is ignored and not
/// recorded: it is a fixture or prose, not a claim about the program. Otherwise accepted,
/// with one source map per output. Whether an output builds or its tests pass is not
/// checked here; the guidance says how, and the caller runs it.
// Decision: the definition passes the request and the unit; a plan here does not own its
// workspace, so the workspace and the plan are extra first parameters and the unit is
// given as its index into the plan.
// @lfy def/generation/main.lfy:accept
pub fn accept(
    workspace: &Workspace,
    plan: &Plan,
    request: &Request,
    unit: usize,
    outputs: &[Output],
) -> Verdict {
    let model = &workspace.model;
    let planned = &plan.units[unit];
    let target = &workspace.targets[planned.target];
    let file = &workspace.files[planned.file];
    let mut problems: Vec<String> = Vec::new();

    if outputs.is_empty() {
        // @lfy def/generation/main.lfy:accept
        problems.push(format!("no output was produced for {}", file.path));
    }

    // Decision: the file's last line is counted from the request's source, which is the
    // text the outputs were generated from.
    let source = request
        .sources
        .get(&file.path)
        .cloned()
        .unwrap_or_else(|| source_text(workspace, file));
    let last_line = source.lines().count().max(1);
    let named = named_entities(model, file.source, &planned.entities);

    let mut normalized: Vec<Vec<Marker>> = Vec::new();
    for output in outputs {
        if !under(&output.path, &target.output_directory) {
            // @lfy def/generation/main.lfy:accept
            problems.push(format!(
                "the output {} is not under the output directory {} of the target {}",
                output.path, target.output_directory, target.identifier
            ));
        }
        let mut markers = Vec::new();
        for marker in parse_markers(&output.text) {
            // @lfy def/generation/main.lfy:accept
            if let Some(marker) = resolve_marker(
                workspace,
                file,
                &named,
                &marker,
                last_line,
                output,
                &mut problems,
            ) {
                markers.push(marker);
            }
        }
        normalized.push(markers);
    }

    // @lfy def/generation/main.lfy:accept
    for &entity in &planned.entities {
        let Some((first, last)) = declaration_lines(model, entity) else {
            continue;
        };
        let covered = normalized
            .iter()
            .flatten()
            .any(|marker| marker.file == file.path && (first..=last).contains(&marker.line));
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

    // @lfy def/generation/main.lfy:accept
    let hash = source_hash(&source);
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
            signature: signature.clone(),
            dependencies: dependencies.clone(),
            generated: generated.clone(),
            markers: markers.clone(),
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

/// One marker with its line derived and, where it named a line inside a declaration, the
/// name of that entity; problems are pushed for a marker that resolves nowhere, and `None`
/// is given for one that names a file outside the program.
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
    // A marker that names an entity keeps its spelling; its line is derived.
    if let Some(name) = &marker.entity {
        let own = named_entities(model, source, &[]);
        let table = if marker.file == file.path {
            named
        } else {
            &own
        };
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
    if marker.file != file.path {
        return Some(marker.clone());
    }
    if marker.line == 0 || marker.line > last_line {
        // @lfy def/generation/main.lfy:accept
        problems.push(format!(
            "{}:{}: the marker names line {} of {}, whose last line is {last_line}",
            output.path, marker.output_line, marker.line, file.path
        ));
        return Some(marker.clone());
    }
    // @lfy def/generation/main.lfy:accept
    Some(match innermost(model, named, marker.line) {
        Some(name) => Marker {
            entity: Some(name),
            column: None,
            ..marker.clone()
        },
        None => marker.clone(),
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

/// An output with every marker that was rewritten spelled as its name.
// @lfy def/generation/main.lfy:accept
fn rewrite_markers(output: &Output, markers: &[Marker]) -> Output {
    let rewritten: Vec<&Marker> = markers
        .iter()
        .filter(|marker| marker.entity.is_some() && marker.column.is_none())
        .collect();
    if rewritten.is_empty() {
        return output.clone();
    }
    let mut lines: Vec<String> = output.text.lines().map(str::to_string).collect();
    let mut changed = false;
    for marker in rewritten {
        let Some(line) = lines.get_mut(marker.output_line.wrapping_sub(1)) else {
            continue;
        };
        let written = marker.spelling();
        for spelling in [
            format!("{MARKER_PREFIX}{}:{}", marker.file, marker.line),
            format!("{MARKER_PREFIX}{}:{}:{}", marker.file, marker.line, 0),
        ] {
            if line.contains(&spelling) && spelling != written {
                *line = line.replacen(&spelling, &written, 1);
                changed = true;
                break;
            }
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
// markers, hashes, source maps, timestamps
// ---------------------------------------------------------------------------------------

/// Every `@lfy <path>:<line>[:<column>]` or `@lfy <path>:<entity>` marker in an output's
/// text, with the output line it sits on, in output order. The marker sits after the line
/// comment opener of the target's language, whatever that is, so `@lfy ` is matched
/// anywhere in a line. A marker that names an entity has no line until one is derived.
/// Text is read as a marker only when what follows is a path ending in `.lfy`, a colon,
/// and a number or an identifier path; anything else after `@lfy` is prose.
// @lfy def/generation/data.lfy:22
pub fn parse_markers(text: &str) -> Vec<Marker> {
    let mut out = Vec::new();
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
    out
}

/// A marker from what follows `@lfy `: `path:line`, `path:line:column`, or `path:entity`.
/// `None` when the path does not end in `.lfy`, or when what follows the colon is neither
/// a number nor an identifier path: that text is prose, not a marker.
// @lfy def/generation/data.lfy:22
fn parse_marker(token: &str, output_line: usize) -> Option<Marker> {
    // Decision: punctuation that closes a sentence or a comment after the marker is not
    // part of it.
    let token = token.trim_end_matches(['.', ',', ';', ')', ']', '}', '*', '/', '-']);
    let mut parts = token.rsplitn(3, ':');
    let last = parts.next()?;
    let middle = parts.next()?;
    let first = parts.next();
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
        }),
        // @lfy def/generation/data.lfy:Marker.entity
        Err(_) if is_identifier_path(last) => Some(Marker {
            output_line,
            file: path,
            entity: Some(last.to_string()),
            line: 0,
            column: None,
        }),
        Err(_) => None,
    }
}

/// Whether text names a source file: a path ending in `.lfy`, with a name before it.
// @lfy def/generation/data.lfy:22
fn is_source_path(path: &str) -> bool {
    path.len() > EXTENSION.len() && path.ends_with(EXTENSION)
}

/// Whether text is an identifier, or an owner's identifier, a dot, and a member's name.
// @lfy def/generation/data.lfy:22
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

/// The source maps in a `source-map.json`: a JSON array of source map objects. A missing
/// file gives none.
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

/// Write source maps as a `source-map.json`: a JSON array of source map objects, one per
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
        /// directory `src`, and an empty `def` directory.
        fn with_rust_target() -> Fixture {
            let fixture = Fixture::new();
            fixture
                .write(
                    "elfie.json",
                    r#"{
                        "output": "src",
                        "dependencies": { "rust": { "root": "targets/rust" } },
                        "targets": { "rust": { "package": "rust", "marker": "rust" } },
                        "native": [ { "identifier": "serde_json", "ecosystem": "cargo", "version": "1" } ]
                    }"#,
                )
                .write(
                    "targets/rust/elfie.json",
                    r#"{ "native": [ { "identifier": "sha2", "ecosystem": "cargo" } ] }"#,
                )
                .write(
                    "targets/rust/main.lfy",
                    "/// Built for Rust.\ntrait rust {\n  @acceptanceCriteria.add({ behavior = `Each unit becomes one module named after its stem` });\n}\nrust.apply(global);\n",
                );
            fs::create_dir_all(fixture.root.join("def")).unwrap();
            fixture
        }

        fn write(&self, path: &str, text: &str) -> &Fixture {
            let disk = self.root.join(path);
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(disk, text).unwrap();
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

    /// One source map per unit of a plan, recording that unit's source hash, its interface
    /// signature, and the signature of each of its dependencies as they are now: what
    /// acceptance would have written.
    fn current_maps(workspace: &Workspace, plan: &Plan) -> Vec<SourceMap> {
        plan.units
            .iter()
            .map(|unit| {
                let file = &workspace.files[unit.file];
                SourceMap {
                    target: workspace.targets[unit.target].identifier.clone(),
                    output: format!("src/{}.rs", unit.stem),
                    source: file.path.clone(),
                    hash: source_hash(&source_text(workspace, file)),
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

    // @lfy def/generation/main.lfy:interfaceOf
    #[test]
    fn an_interface_holds_one_line_per_entity_with_its_kind_definition_type_and_signature() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let text = interface_of(&workspace, b);
        let lines: Vec<&str> = text.lines().collect();
        // One line per entity, and one per member after a data. @lfy def/generation/main.lfy:interfaceOf
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(lines[0], "- `B` (data: DataDeclaration): A b — type `B`");
        assert_eq!(lines[1], "  - `x` (member) — type `string`");
        assert!(
            lines[2].starts_with("- `make` (agent function: AgentFunctionDeclaration): Makes a b"),
            "{text}"
        );
        assert!(lines[2].contains("— parameters (x: string)"), "{text}");
        assert!(lines[2].contains("— output `B`"), "{text}");
        // Nothing in an interface is a line number, so nothing moves with the source.
        assert!(!text.contains(":1"), "{text}");
    }

    // @lfy def/generation/main.lfy:interfaceOf
    #[test]
    fn a_new_criterion_or_a_moved_line_leaves_the_interface_the_same() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        let plan = plan(&workspace, &[], &[]);
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
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = super::plan(&workspace, &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(interface_of(&workspace, b), before);
    }

    // @lfy def/generation/main.lfy:interfaceOf
    #[test]
    fn a_member_added_to_a_data_changes_the_interface() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        let plan = plan(&workspace, &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let before = interface_of(&workspace, b);

        fixture.write(
            "def/b.lfy",
            &B.replace("$x = string;", "$x = string;\n  $y = number;"),
        );
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = super::plan(&workspace, &[], &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let after = interface_of(&workspace, b);
        assert_ne!(after, before);
        assert!(
            after.contains("  - `y` (member) — type `number`"),
            "{after}"
        );
    }

    // -----------------------------------------------------------------------------------
    // plan
    // -----------------------------------------------------------------------------------

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn two_files_give_two_fresh_units_with_b_before_a_in_one_batch() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        assert_eq!(plan.units.len(), 2, "{:?}", plan.units);
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert!(b_index < a_index);
        assert_eq!(a.dependencies, [b_index]);
        assert!(b.dependencies.is_empty());
        assert_eq!(a.reason, Some(Reason::Fresh));
        assert_eq!(b.reason, Some(Reason::Fresh));
        assert!(a.outputs.is_empty());
        // @lfy def/generation/main.lfy:plan
        assert_eq!(names(&workspace, &a.entities), ["A"]);
        assert_eq!(names(&workspace, &b.entities), ["B", "make"]);
        // @lfy def/generation/main.lfy:plan
        assert_eq!(a.stem, "a");
        assert_eq!(b.stem, "b");
        assert_eq!(a.target, 0);
        assert_eq!(a.file, file_index(&workspace, "def/a.lfy"));
        assert_eq!(plan.planned().count(), 2);
        // One batch, named after the first unit and the count of the others.
        // @lfy def/generation/main.lfy:plan
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].units, [b_index, a_index]);
        assert_eq!(plan.batches[0].identifier, "b+1");
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn matching_source_maps_leave_no_reason_and_no_batches() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let maps = current_maps(&workspace, &plan(&workspace, &[], &[]));
        let plan = plan(&workspace, &maps, &[]);
        assert_eq!(plan.units.len(), 2);
        assert!(
            plan.units.iter().all(|unit| unit.reason.is_none()),
            "{:?}",
            plan.units
        );
        // @lfy def/generation/main.lfy:plan
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(a.outputs.len(), 1);
        assert_eq!(a.outputs[0].source, "def/a.lfy");
        assert_eq!(plan.planned().count(), 0);
        assert!(plan.batches.is_empty()); // @lfy def/generation/main.lfy:plan
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn a_comment_added_inside_a_dependency_changes_it_and_leaves_its_dependent_alone() {
        let fixture = a_and_b();
        let old = fixture.load();
        assert_bound(&old);
        let mut maps = current_maps(&old, &plan(&old, &[], &[]));

        fixture.write("def/b.lfy", &format!("{B}\n// edited\n"));
        let workspace = fixture.load();
        assert_bound(&workspace);
        // The recorded interface signatures still match the current ones.
        let fresh = current_maps(&workspace, &plan(&workspace, &[], &[]));
        for map in &mut maps {
            let current = fresh.iter().find(|m| m.source == map.source).unwrap();
            assert_eq!(map.signature, current.signature);
            assert_eq!(map.dependencies, current.dependencies);
        }

        let plan = plan(&workspace, &maps, &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(b.reason, Some(Reason::Changed));
        // b is planned only for itself: its interface is the same. @lfy def/generation/main.lfy:plan
        assert_eq!(a.reason, None);
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(plan.batches[0].identifier, "b");
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn a_member_added_to_a_dependency_plans_its_dependent_too_in_one_batch() {
        let fixture = a_and_b();
        let old = fixture.load();
        assert_bound(&old);
        let maps = current_maps(&old, &plan(&old, &[], &[]));

        fixture.write(
            "def/b.lfy",
            &B.replace("$x = string;", "$x = string;\n  $y = number;"),
        );
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &maps, &[]);
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(b.reason, Some(Reason::Changed));
        assert_eq!(a.reason, Some(Reason::Dependency)); // @lfy def/generation/main.lfy:plan
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].units, [b_index, a_index]);
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn a_marker_nothing_carries_gives_no_units() {
        let fixture = a_and_b();
        fixture.write("targets/rust/main.lfy", "trait rust { }\n");
        let workspace = fixture.load();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        assert_eq!(workspace.targets.len(), 1);
        let plan = plan(&workspace, &[], &[]);
        assert!(plan.units.is_empty(), "{:?}", plan.units);
        assert!(plan.batches.is_empty()); // @lfy def/generation/main.lfy:plan
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn an_entity_carrying_the_marker_itself_is_built() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("targets/rust/main.lfy", "trait rust { }\n")
            .write(
                "def/a.lfy",
                "use \"rust\";\n\nd Marked is rust { $x = string; }\nd Plain { $y = string; }\n",
            );
        let workspace = fixture.load();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let plan = plan(&workspace, &[], &[]);
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(names(&workspace, &plan.units[0].entities), ["Marked"]);
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn a_package_file_gives_no_unit_and_a_nested_file_keeps_its_directory_in_its_stem() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/deep/inner.lfy", "d Inner { $x = string; }\n");
        let workspace = fixture.load();
        assert_bound(&workspace);
        // The package's main.lfy declares a trait global carries, but has a package.
        assert!(
            workspace
                .file("targets/rust/main.lfy")
                .unwrap()
                .package
                .is_some()
        );
        let plan = plan(&workspace, &[], &[]);
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(plan.units[0].stem, "deep/inner"); // @lfy def/generation/main.lfy:plan
        assert_eq!(
            plan.units[0].file,
            file_index(&workspace, "def/deep/inner.lfy")
        );
        assert_eq!(plan.batches[0].identifier, "deep/inner");
    }

    // @lfy def/generation/main.lfy:plan
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
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        assert_eq!(plan.units.len(), 3, "{:?}", plan.units);
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        let (d_index, _) = unit_of(&workspace, &plan, "def/d.lfy");
        // Each once, in use order: c's uses (d, then b) come through c, then b directly.
        // @lfy def/generation/main.lfy:plan
        assert_eq!(a.dependencies, [d_index, b_index]);
        assert!(a_index > b_index && a_index > d_index); // @lfy def/generation/main.lfy:plan
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn a_cycle_among_uses_adds_nothing() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/a.lfy", "use \"./b\";\nd A { $x = string; }\n")
            .write("def/b.lfy", "use \"./a\";\nd B { $x = string; }\n");
        let workspace = fixture.load();
        // One problem at each `Use` of the cycle; the plan adds nothing beyond them.
        assert_eq!(workspace.problems.len(), 2, "{:?}", workspace.problems);
        let plan = plan(&workspace, &[], &[]);
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

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn requested_comes_before_every_other_reason_by_path_or_stem() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let maps = current_maps(&workspace, &plan(&workspace, &[], &[]));
        let plan = plan(&workspace, &maps, &["b".to_string()]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(b.reason, Some(Reason::Requested));
        // b's interface did not change, so a is left alone. @lfy def/generation/main.lfy:plan
        assert_eq!(a.reason, None);
        let plan = super::plan(&workspace, &[], &["def/a.lfy".to_string()]);
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(a.reason, Some(Reason::Requested));
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn an_output_recording_nothing_for_a_dependency_plans_the_unit() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let mut maps = current_maps(&workspace, &plan(&workspace, &[], &[]));
        for map in &mut maps {
            map.dependencies.clear();
        }
        let plan = plan(&workspace, &maps, &[]);
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
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        assert_eq!(plan.units.len(), 8, "{:?}", plan.units);
        let sizes: Vec<usize> = plan.batches.iter().map(|batch| batch.units.len()).collect();
        // Six of `deep`, then the seventh, and `top` on its own: no batch holds more than
        // six units, and none mixes directories.
        assert_eq!(sizes.iter().sum::<usize>(), 8, "{:?}", plan.batches);
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

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn a_dependent_planned_only_for_its_dependency_joins_its_batch_across_directories() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/core/b.lfy", "d B: `A b` {\n  $x = string;\n}\n")
            .write(
                "def/other/a.lfy",
                "use \"../core/b\";\n\nd A {\n  $b = B;\n}\n",
            );
        let workspace = fixture.load();
        assert_bound(&workspace);
        let maps = current_maps(&workspace, &plan(&workspace, &[], &[]));

        fixture.write(
            "def/core/b.lfy",
            "d B: `A b` {\n  $x = string;\n  $y = number;\n}\n",
        );
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &maps, &[]);
        let (b_index, b) = unit_of(&workspace, &plan, "def/core/b.lfy");
        let (a_index, a) = unit_of(&workspace, &plan, "def/other/a.lfy");
        assert_eq!(b.reason, Some(Reason::Changed));
        assert_eq!(a.reason, Some(Reason::Dependency));
        assert_ne!(first_segment(&a.stem), first_segment(&b.stem));
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].units, [b_index, a_index]);
        assert_eq!(plan.batches[0].identifier, "core/b+1");
    }

    // @lfy def/generation/main.lfy:plan
    #[test]
    fn every_batch_comes_after_the_batches_holding_its_dependencies() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/one/x.lfy", "d X { $x = string; }\n")
            .write("def/two/y.lfy", "use \"../one/x\";\n\nd Y { $x = X; }\n");
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
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

    // -----------------------------------------------------------------------------------
    // request
    // -----------------------------------------------------------------------------------

    // @lfy def/generation/main.lfy:request
    #[test]
    fn a_request_names_the_target_lists_its_units_and_quotes_their_entities() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        let batch = &plan.batches[0];
        let request = request(&workspace, &plan, batch, &[], &BTreeMap::new());
        // @lfy def/generation/main.lfy:request
        assert_eq!(request.batch, *batch);
        assert_eq!(request.batch.units, [b_index, a_index]);
        assert_eq!(request.sources["def/a.lfy"], A);
        assert_eq!(request.sources["def/b.lfy"], B);
        assert!(request.previous.is_empty());
        assert!(request.existing.is_empty()); // @lfy def/generation/main.lfy:request
        // Both units are in the batch, so nothing is an interface.
        // @lfy def/generation/main.lfy:request
        assert!(request.interfaces.is_empty());
        // @lfy def/generation/main.lfy:request
        let marker = workspace.targets[0].marker;
        assert_eq!(
            request.guidance,
            model::criteria_of(&workspace.model, marker)
        );
        assert_eq!(request.guidance.len(), 1);
        // @lfy def/generation/main.lfy:request
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
        assert!(
            text.contains("/// A record.\nd A: `An a` {\n  $b = B;\n}"),
            "{text}"
        );
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
        // Markers name entities, never lines. @lfy def/generation/main.lfy:request
        assert!(text.contains("Never write a line number."), "{text}");
        assert!(text.contains("`@lfy def/b.lfy:B`"), "{text}");
        // The three report lines. @lfy def/generation/main.lfy:request
        assert!(text.contains("`ELFIE: DONE`"), "{text}");
        assert!(text.contains("`ELFIE: BLOCKED: `"), "{text}");
        assert!(text.contains("`ELFIE: CLARIFY: `"), "{text}");
        assert!(text.contains("agent server"), "{text}");
        assert!(!text.contains("## Existing outputs"), "{text}");

        // The sections come in the definition's order. @lfy def/generation/main.lfy:request
        let headings = [
            "# Compiling",
            "## Where the outputs go",
            "## Guidance",
            "## Native dependencies",
            "## Interfaces",
            "## The units to compile",
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
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        let batch = Batch {
            units: vec![b_index],
            identifier: "b".to_string(),
        };
        let request = request(&workspace, &plan, &batch, &[], &BTreeMap::new());
        let text = &request.instructions;
        assert!(
            text.contains("#### `make` (agent function: AgentFunctionDeclaration)"),
            "{text}"
        );
        assert!(text.contains("- When x is empty: the b holds x"), "{text}");
        assert!(
            text.contains("- Input `[\"y\"]` gives `B@like(`holding y`)`"),
            "{text}"
        );
    }

    // @lfy def/generation/main.lfy:request
    #[test]
    fn a_dependency_outside_the_batch_is_an_interface_with_its_outputs_and_entities() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let maps = current_maps(&workspace, &plan(&workspace, &[], &[]));
        let plan = plan(&workspace, &maps, &["a".to_string()]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(plan.batches.len(), 1, "{:?}", plan.batches);
        assert_eq!(plan.batches[0].units, [a_index]);
        let request = request(&workspace, &plan, &plan.batches[0], &[], &BTreeMap::new());
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
        let workspace = fixture.load();
        assert_bound(&workspace);
        let maps = current_maps(&workspace, &plan(&workspace, &[], &[]));
        let plan = plan(&workspace, &maps, &["a".to_string()]);
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
        let with_previous = request(&workspace, &plan, &plan.batches[0], &existing, &previous);
        // @lfy def/generation/main.lfy:request
        assert_eq!(with_previous.existing.len(), 1);
        assert_eq!(with_previous.existing["src/a.rs"], "// old");
        assert_eq!(with_previous.previous, previous);
        let text = &with_previous.instructions;
        assert!(text.contains("## Existing outputs"), "{text}");
        assert!(
            text.contains("keep the markers of unchanged items"),
            "{text}"
        );
        assert!(text.contains("write to the same paths"), "{text}");
        assert!(text.contains("d A {}"), "{text}");

        let without = request(
            &workspace,
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

    // @lfy def/generation/main.lfy:outcomeOf
    #[test]
    fn a_report_ending_in_done_with_accepted_verdicts_is_accepted() {
        let outcome = outcome_of("I wrote it.\nELFIE: DONE", vec![accepted_verdict()]);
        assert_eq!(outcome.kind, OutcomeKind::Accepted); // @lfy def/generation/main.lfy:outcomeOf
        assert_eq!(outcome.message, "");
        assert_eq!(outcome.verdicts.len(), 1); // @lfy def/generation/main.lfy:outcomeOf
    }

    // @lfy def/generation/main.lfy:outcomeOf
    #[test]
    fn a_report_ending_in_clarify_is_a_clarification_with_the_question() {
        let outcome = outcome_of("ELFIE: CLARIFY: should Range.end be inclusive?", Vec::new());
        assert_eq!(outcome.kind, OutcomeKind::Clarification);
        assert_eq!(outcome.message, "should Range.end be inclusive?");
        assert!(outcome.verdicts.is_empty());
    }

    // @lfy def/generation/main.lfy:outcomeOf
    #[test]
    fn a_report_ending_in_blocked_is_blocked_with_the_reason_and_every_line_after_it() {
        let outcome = outcome_of(
            "ELFIE: BLOCKED: [[tokenAt]] contradicts [[nodesAt]] on empty files\nand on one-line files",
            Vec::new(),
        );
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

    // @lfy def/generation/main.lfy:outcomeOf
    #[test]
    fn a_report_with_nothing_and_no_verdicts_has_failed() {
        let outcome = outcome_of("", Vec::new());
        assert_eq!(outcome.kind, OutcomeKind::Failed);
        assert_eq!(outcome.message, "the compiler reported nothing");
        assert!(outcome.verdicts.is_empty());
    }

    // @lfy def/generation/main.lfy:outcomeOf
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
    fn one_unit(workspace: &Workspace) -> (Plan, Request, usize) {
        let plan = plan(workspace, &[], &[]);
        let (index, _) = unit_of(workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![index],
            identifier: "a".to_string(),
        };
        let request = request(workspace, &plan, &batch, &[], &BTreeMap::new());
        (plan, request, index)
    }

    // @lfy def/generation/main.lfy:accept
    #[test]
    fn outputs_with_a_marker_per_entity_are_accepted_with_one_source_map() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let (plan, request, index) = one_unit(&workspace);
        assert_eq!(names(&workspace, &plan.units[index].entities), ["A", "B"]);
        let output = named_output(
            "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/a.lfy:B\npub struct B;\n",
        );
        let verdict = accept(
            &workspace,
            &plan,
            &request,
            index,
            std::slice::from_ref(&output),
        );
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert!(verdict.problems.is_empty());
        assert_eq!(verdict.source_maps.len(), 1);
        // A marker that names an entity is left as written. @lfy def/generation/main.lfy:accept
        assert_eq!(verdict.outputs, [output]);
        let map = &verdict.source_maps[0];
        assert_eq!(map.target, "rust");
        assert_eq!(map.output, "src/a.rs");
        assert_eq!(map.source, "def/a.lfy");
        // @lfy def/generation/main.lfy:accept
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
        // The source map round-trips through the plan: the unit is now up to date.
        let plan = super::plan(&workspace, &verdict.source_maps, &[]);
        assert_eq!(plan.units[index].reason, None);
    }

    // @lfy def/generation/main.lfy:accept
    #[test]
    fn an_entity_without_a_marker_is_rejected_by_name() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let (plan, request, index) = one_unit(&workspace);
        // A marker on the documentation line of A counts for A; nothing names B.
        let output = named_output("// @lfy def/a.lfy:3\npub struct A;\n");
        let verdict = accept(&workspace, &plan, &request, index, &[output]);
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

    // @lfy def/generation/main.lfy:accept
    #[test]
    fn a_marker_naming_a_line_inside_a_declaration_is_rewritten_to_name_it() {
        let fixture = Fixture::with_rust_target();
        // `A` is declared on line 3.
        fixture.write("def/a.lfy", "// a file\n\nd A {\n  $x = string;\n}\n");
        let workspace = fixture.load();
        assert_bound(&workspace);
        let (plan, request, index) = one_unit(&workspace);
        assert_eq!(names(&workspace, &plan.units[index].entities), ["A"]);
        let output = named_output("// @lfy def/a.lfy:3\npub struct A;\n");
        let verdict = accept(&workspace, &plan, &request, index, &[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        // @lfy def/generation/main.lfy:accept
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
        let verdict = accept(&workspace, &plan, &request, index, &[output]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert!(
            verdict.outputs[0].text.contains("// @lfy def/a.lfy:A.x"),
            "{}",
            verdict.outputs[0].text
        );
    }

    // @lfy def/generation/main.lfy:accept
    #[test]
    fn no_outputs_are_rejected() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let (plan, request, index) = one_unit(&workspace);
        let verdict = accept(&workspace, &plan, &request, index, &[]);
        assert!(!verdict.accepted);
        assert!(
            verdict.problems[0].contains("no output"),
            "{:?}",
            verdict.problems
        );
    }

    // @lfy def/generation/main.lfy:accept
    #[test]
    fn an_output_outside_the_output_directory_is_rejected_by_path() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let (plan, request, index) = one_unit(&workspace);
        let mut output = named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n");
        output.path = "srcx/a.rs".to_string();
        let verdict = accept(&workspace, &plan, &request, index, &[output]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(
            verdict.problems[0].contains("srcx/a.rs"),
            "{}",
            verdict.problems[0]
        );
    }

    // @lfy def/generation/main.lfy:accept
    #[test]
    fn a_marker_naming_an_entity_the_file_does_not_declare_is_rejected_by_output_line_and_name() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let (plan, request, index) = one_unit(&workspace);
        let output =
            named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n// @lfy def/a.lfy:Nowhere\n");
        let verdict = accept(&workspace, &plan, &request, index, &[output]);
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

    // @lfy def/generation/main.lfy:accept
    #[test]
    fn a_marker_naming_a_line_past_the_last_is_rejected_by_output_line() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let (plan, request, index) = one_unit(&workspace);
        let output =
            named_output("// @lfy def/a.lfy:A\n\n// @lfy def/a.lfy:99\n// @lfy def/a.lfy:B\n");
        let verdict = accept(&workspace, &plan, &request, index, &[output]);
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
    // @lfy def/generation/main.lfy:accept
    #[test]
    fn a_marker_naming_a_file_that_is_not_in_the_program_is_ignored() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let (plan, request, index) = one_unit(&workspace);
        let output = named_output(
            "// @lfy def/a.lfy:A\n// @lfy def/b.lfy:8\n// @lfy def/b.lfy:Nowhere\n// @lfy def/a.lfy:B\n",
        );
        let verdict = accept(&workspace, &plan, &request, index, &[output]);
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

    // @lfy def/generation/main.lfy:accept
    #[test]
    fn an_output_may_hold_markers_for_other_files_of_the_program() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let batch = Batch {
            units: vec![index],
            identifier: "a".to_string(),
        };
        let request = request(&workspace, &plan, &batch, &[], &BTreeMap::new());
        let output = named_output(
            "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:B\npub struct B;\n",
        );
        let verdict = accept(&workspace, &plan, &request, index, &[output]);
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
    }

    // -----------------------------------------------------------------------------------
    // markers, hashes, source maps, timestamps
    // -----------------------------------------------------------------------------------

    // @lfy def/generation/data.lfy:19
    #[test]
    fn markers_are_parsed_after_any_line_comment_opener() {
        let text = "// @LFY def/a.lfy:4\nfn x() {}\n# @LFY def/a.lfy:5:2\n-- @LFY def/a.lfy:6 -- more\n; @LFY def/a.lfy:7.\nnothing here\n/* @LFY def/a.lfy:8 */\n// @LFY def/a.lfy:Marker.file\n// @LFY nope\n// @LFY :3\n".replace("@LFY", "@lfy");
        let markers = parse_markers(&text);
        let marker = |output_line, line, column| Marker {
            output_line,
            file: "def/a.lfy".to_string(),
            entity: None,
            line,
            column,
        };
        assert_eq!(
            markers,
            [
                marker(1, 4, None),
                marker(3, 5, Some(2)),
                marker(4, 6, None),
                marker(5, 7, None),
                marker(7, 8, None),
                // @lfy def/generation/data.lfy:Marker.entity
                Marker {
                    output_line: 8,
                    file: "def/a.lfy".to_string(),
                    entity: Some("Marker.file".to_string()),
                    line: 0,
                    column: None,
                },
            ]
        );
        assert_eq!(markers[1].spelling(), "@lfy def/a.lfy:5:2");
        assert_eq!(markers[0].spelling(), "@lfy def/a.lfy:4");
        assert_eq!(markers[5].spelling(), "@lfy def/a.lfy:Marker.file");
        assert!(parse_markers("").is_empty());
    }

    /// Text is read as a marker only when what follows `@lfy` and the space is a path
    /// ending in `.lfy`, a colon, and a number or an identifier path.
    // @lfy def/generation/data.lfy:22
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
        ];
        for line in prose {
            let text = line.replace("@LFY", "@lfy");
            assert!(parse_markers(&text).is_empty(), "{text}");
        }
        let text = "// @LFY def/a.lfy:4\n// @LFY def/deep/a.lfy:Marker\n// @LFY def/a.lfy:_x.y2\n"
            .replace("@LFY", "@lfy");
        let markers = parse_markers(&text);
        assert_eq!(markers.len(), 3, "{markers:?}");
        assert_eq!(markers[0].line, 4);
        assert_eq!(markers[1].file, "def/deep/a.lfy");
        assert_eq!(markers[1].entity.as_deref(), Some("Marker"));
        assert_eq!(markers[2].entity.as_deref(), Some("_x.y2"));
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
                    },
                    Marker {
                        output_line: 9,
                        file: "def/a.lfy".to_string(),
                        entity: None,
                        line: 5,
                        column: Some(2),
                    },
                ],
            },
            SourceMap {
                target: "rust".to_string(),
                output: "src/b.rs".to_string(),
                source: "def/b.lfy".to_string(),
                hash: source_hash("y"),
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
