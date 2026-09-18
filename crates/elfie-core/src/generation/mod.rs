//! Compiled from `def/generation/main.lfy`: planning what to generate, asking for it,
//! and accepting what comes back.
//!
//! A unit is one source file for one target: outputs and source maps are per file, a
//! declaration's scope is its file, and a smaller unit would make the compiler merge
//! into a file it does not own, which is itself an act of generation. A larger unit
//! would not fit a request. [`plan`] finds every unit of a workspace and which of them
//! need generating; [`request`] gathers everything the compiler is handed for one unit,
//! with every criterion and test already resolved so that a compiler with no access to
//! the model can still work; [`accept`] checks outputs structurally and gives them their
//! source maps. Nothing here shells out.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod data; // @lfy def/generation/main.lfy:6

pub use data::*;

use sha2::{Digest, Sha256};

use crate::model::{self, Criterion, Entity, EntityId, EntityKind, Model, NodeRef, SymbolKind};
use crate::workspace::{File, NativeDependency, Target, Workspace};

/// The extension of a source file.
const EXTENSION: &str = ".lfy";
/// What a marker starts with, after the comment opener of the target's language.
const MARKER_PREFIX: &str = "@lfy ";

// ---------------------------------------------------------------------------------------
// plan
// ---------------------------------------------------------------------------------------

/// Every unit of a workspace, and which of them need generating.
///
/// An entity is built for a target when it carries the target's marker, when the
/// anonymous entity of its file does, or when `global` does; the last two select every
/// entity declared in the file scope. Every file without a package that has at least one
/// built entity for a target gives one unit holding those entities in file order; a file
/// that has a package gives no unit, since a package is compiled by its own project.
/// Units come in target order then file order, each after its dependencies. A unit's
/// reason is the first of: requested, fresh, changed, dependency; `None` when it is up to
/// date. The caller drops maps whose output no longer exists before calling.
// @lfy def/generation/main.lfy:12
pub fn plan(workspace: &Workspace, source_maps: &[SourceMap], requested: &[String]) -> Plan {
    let model = &workspace.model;
    let mut units: Vec<Unit> = Vec::new();

    for (target_index, target) in workspace.targets.iter().enumerate() {
        // Selection: which files give a unit, and with which entities.
        // @lfy def/generation/main.lfy:16
        let mut unit_of_file: Vec<Option<usize>> = vec![None; workspace.files.len()];
        let mut local: Vec<(usize, Vec<EntityId>)> = Vec::new();
        for (file_index, file) in workspace.files.iter().enumerate() {
            if file.package.is_some() {
                continue; // @lfy def/generation/main.lfy:17
            }
            let entities = built_entities(model, file.source, target.marker);
            if entities.is_empty() {
                continue;
            }
            unit_of_file[file_index] = Some(local.len());
            local.push((file_index, entities));
        }

        // Dependencies, as positions among this target's units.
        // @lfy def/generation/main.lfy:22
        let dependencies: Vec<Vec<usize>> = local
            .iter()
            .map(|(file_index, _)| dependencies_of(workspace, *file_index, &unit_of_file))
            .collect();

        // Order: each unit after its dependencies, otherwise in file order.
        // @lfy def/generation/main.lfy:24
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
                stem: stem_of(&file.path, &workspace.source_directory), // @lfy def/generation/main.lfy:18
                dependencies: dependencies[item].iter().map(|&d| position[d]).collect(),
                // @lfy def/generation/main.lfy:28
                outputs: source_maps
                    .iter()
                    .filter(|map| map.target == target.identifier && map.source == file.path)
                    .cloned()
                    .collect(),
                reason: None,
            });
        }
    }

    // Staleness, in plan order so that every dependency's reason is known first (a
    // dependency placed later can only come from a cycle, which adds nothing).
    // @lfy def/generation/main.lfy:29
    for index in 0..units.len() {
        let reason = reason_of(workspace, &units, index, requested);
        units[index].reason = reason;
    }

    Plan { units }
}

/// The entities of a file built for a target's marker, in file order.
// @lfy def/generation/main.lfy:15
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
            if entity.file != Some(source) || entity.node.is_none() || out.contains(&symbol.entity) {
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
// @lfy def/generation/main.lfy:22
fn dependencies_of(workspace: &Workspace, file_index: usize, unit_of_file: &[Option<usize>]) -> Vec<usize> {
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
            continue; // @lfy def/generation/main.lfy:23
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
// @lfy def/generation/main.lfy:24
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
// @lfy def/generation/main.lfy:18
fn stem_of(path: &str, source_directory: &str) -> String {
    // Decision: a file the program reached through a `use` from outside the source
    // directory keeps its whole path as its stem, so that two such files cannot collide.
    let relative = strip_directory(path, source_directory).unwrap_or(path);
    relative.strip_suffix(EXTENSION).unwrap_or(relative).to_string()
}

/// Why a unit is planned: the first of requested, fresh, changed, dependency that holds.
// @lfy def/generation/main.lfy:29
fn reason_of(workspace: &Workspace, units: &[Unit], index: usize, requested: &[String]) -> Option<Reason> {
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
    let earliest = unit
        .outputs
        .iter()
        .map(|output| output.generated.as_str())
        .min_by(|a, b| compare_timestamps(a, b));
    let stale = unit.dependencies.iter().any(|&dependency| {
        let dependency = &units[dependency];
        dependency.reason.is_some()
            || earliest.is_some_and(|earliest| {
                dependency
                    .outputs
                    .iter()
                    .any(|output| compare_timestamps(&output.generated, earliest).is_gt())
            })
    });
    if stale {
        return Some(Reason::Dependency);
    }
    None
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

/// Everything the compiler is handed to produce one unit's outputs.
///
/// `unit` is an index into [`Plan::units`]. The source is the joined raw text of the
/// file's tokens; `existing` is kept only where its path is among the unit's outputs;
/// there is one interface per dependency; the guidance is every criterion of the target's
/// marker; and the native dependencies are the workspace's followed by the target
/// package's. The instructions quote every criterion and test already resolved, so a
/// compiler with no access to the model can still work.
// Decision: the definition passes the plan and the unit; a plan here does not own its
// workspace, so the workspace is an extra first parameter and the unit is given as its
// index into the plan.
// @lfy def/generation/main.lfy:51
pub fn request(
    workspace: &Workspace,
    plan: &Plan,
    unit: usize,
    existing: &[Output],
    previous: Option<&str>,
) -> Request {
    let model = &workspace.model;
    let planned = &plan.units[unit];
    let target = &workspace.targets[planned.target];
    let file = &workspace.files[planned.file];

    let source = source_text(workspace, file); // @lfy def/generation/main.lfy:53
    // @lfy def/generation/main.lfy:54
    let existing: BTreeMap<String, String> = existing
        .iter()
        .filter(|output| planned.outputs.iter().any(|map| map.output == output.path))
        .map(|output| (output.path.clone(), output.text.clone()))
        .collect();
    // @lfy def/generation/main.lfy:55
    let interfaces: Vec<Interface> = planned
        .dependencies
        .iter()
        .map(|&dependency| Interface {
            unit: dependency,
            outputs: plan.units[dependency].outputs.iter().map(|map| map.output.clone()).collect(),
            entities: plan.units[dependency].entities.clone(),
        })
        .collect();
    let guidance = model::criteria_of(model, target.marker); // @lfy def/generation/main.lfy:56
    // @lfy def/generation/main.lfy:57
    let native_dependencies: Vec<NativeDependency> = workspace
        .native_dependencies
        .iter()
        .chain(workspace.packages[target.package].native_dependencies.iter())
        .cloned()
        .collect();

    let instructions = instructions(
        workspace,
        plan,
        planned,
        target,
        file,
        &source,
        previous,
        &existing,
        &interfaces,
        &guidance,
        &native_dependencies,
    );

    Request {
        unit,
        instructions,
        source,
        previous: previous.map(str::to_string), // @lfy def/generation/main.lfy:53
        existing,
        interfaces,
        guidance,
        native_dependencies,
    }
}

/// The prompt: what to produce, where, and the rules for producing it, in the order the
/// definition lists.
// @lfy def/generation/main.lfy:62
#[allow(clippy::too_many_arguments)]
fn instructions(
    workspace: &Workspace,
    plan: &Plan,
    unit: &Unit,
    target: &Target,
    file: &File,
    source: &str,
    previous: Option<&str>,
    existing: &BTreeMap<String, String>,
    interfaces: &[Interface],
    guidance: &[Criterion],
    native_dependencies: &[NativeDependency],
) -> String {
    let model = &workspace.model;
    let mut out = String::new();
    let _ = source;

    // The reader and its job. @lfy def/generation/main.lfy:65
    let _ = writeln!(out, "# Compiling `{}` for the target `{}`\n", file.path, target.identifier);
    let _ = writeln!(
        out,
        "You are the compiler for the target `{}`. Your whole job is the outputs for this one source \
         file, `{}`, and you write nothing outside them.\n",
        target.identifier, file.path
    );

    // Where the outputs go. @lfy def/generation/main.lfy:66
    out.push_str("## Where the outputs go\n\n");
    let _ = writeln!(
        out,
        "The outputs go under `{}`. Their file names are spelled from the stem `{}` as the guidance says.",
        target.output_directory, unit.stem
    );
    if !unit.outputs.is_empty() {
        out.push_str("The last accepted generation produced these outputs; write to the same paths:\n");
        for map in &unit.outputs {
            let _ = writeln!(out, "- `{}`", map.output);
        }
    }
    out.push('\n');

    // Each entity. @lfy def/generation/main.lfy:67
    out.push_str("## The entities to compile\n\n");
    if unit.entities.is_empty() {
        out.push_str("The unit has no entities.\n\n");
    }
    for &entity in &unit.entities {
        write_entity(&mut out, model, entity);
    }

    // The guidance and the native dependencies. @lfy def/generation/main.lfy:68
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
    out.push_str("## Native dependencies\n\n");
    out.push_str("The generated code may require these from the target's ecosystem:\n");
    if native_dependencies.is_empty() {
        out.push_str("(none)\n");
    }
    for dependency in native_dependencies {
        match &dependency.version {
            Some(version) => {
                let _ = writeln!(out, "- `{}` ({}), version `{version}`", dependency.identifier, dependency.ecosystem);
            }
            None => {
                let _ = writeln!(out, "- `{}` ({}), any version", dependency.identifier, dependency.ecosystem);
            }
        }
    }
    out.push('\n');

    // Each interface. @lfy def/generation/main.lfy:69
    out.push_str("## Interfaces\n\n");
    if interfaces.is_empty() {
        out.push_str("This unit depends on no other unit.\n");
    } else {
        out.push_str(
            "This unit depends on the units below. Use their entities by the names their outputs \
             spell, and never edit their outputs.\n\n",
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
        for &entity in &interface.entities {
            write_interface_entity(&mut out, model, entity);
        }
        out.push('\n');
    }

    // The rules for kinds. @lfy def/generation/main.lfy:70
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

    // Markers. @lfy def/generation/main.lfy:71
    out.push_str("## Markers\n\n");
    let _ = writeln!(
        out,
        "Every emitted item and every test carries a marker for the source line it comes from. A marker \
         is written as a line comment of the target's language reading `@lfy <path>:<line>`: `@lfy`, a \
         space, the source path relative to the root (here `{}`), a colon, the line counting from 1, and \
         optionally a colon and the column counting from 0. For example: `@lfy {}:1`.\n",
        file.path, file.path
    );

    // Tests. @lfy def/generation/main.lfy:72
    out.push_str("## Tests\n\n");
    out.push_str(
        "Each test becomes one test, and each criterion that can be checked mechanically becomes one \
         test named after its entity, placed where the guidance says.\n\n",
    );

    // Existing outputs. @lfy def/generation/main.lfy:73
    if !existing.is_empty() {
        out.push_str("## Existing outputs\n\n");
        out.push_str("These outputs exist already:\n");
        for path in existing.keys() {
            let _ = writeln!(out, "- `{path}`");
        }
        match previous {
            Some(_) => out.push_str(
                "\nThe previous source they were generated from is given. Change only what the difference \
                 between the previous source and the current source requires; keep names and structure; \
                 keep the markers of unchanged lines.\n\n",
            ),
            None => out.push_str(
                "\nThe source they were generated from is not known. Reconcile the existing output with \
                 the source.\n\n",
            ),
        }
    }

    // Ambiguity. @lfy def/generation/main.lfy:74
    out.push_str("## Ambiguity\n\n");
    out.push_str(
        "An ambiguous criterion, two criteria in conflict, or a name that resolves nowhere means no output \
         for the unit and a report quoting the criterion, never a guess.\n\n",
    );

    // The agent server. @lfy def/generation/main.lfy:75
    out.push_str("## The agent server\n\n");
    out.push_str("The tools of the agent server may be called for anything the request leaves out.\n");

    out
}

/// One entity of the unit: its source text, its kind, and every criterion and test
/// resolved for it.
// @lfy def/generation/main.lfy:67
fn write_entity(out: &mut String, model: &Model, entity: EntityId) {
    let record = &model.entities[entity];
    let _ = writeln!(out, "### `{}` ({})\n", entity_name(model, entity), kind_text(record));
    if let Some(definition) = &record.definition {
        let _ = writeln!(out, "Definition: {definition}\n");
    }
    if let Some(ty) = &record.ty {
        let _ = writeln!(out, "Type: `{}`\n", model::type_text(model, ty));
    }
    if let Some(node) = record.node {
        out.push_str("Source:\n\n```elfie\n");
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
        let _ = writeln!(out, "- Input `{}` gives `{}`", test.input_text.trim(), test.expect_text.trim());
    }
    out.push('\n');
}

/// One entity of an interface: identifier, kind, definition, and type.
// @lfy def/generation/main.lfy:69
fn write_interface_entity(out: &mut String, model: &Model, entity: EntityId) {
    let record = &model.entities[entity];
    let _ = write!(out, "- `{}` ({})", entity_name(model, entity), kind_text(record));
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
// accept
// ---------------------------------------------------------------------------------------

/// Whether outputs satisfy a request structurally, and the source maps they earn.
///
/// Rejected when there are no outputs, when an output's path is not under the target's
/// output directory, when a marker names another file or a line beyond the file's last,
/// or when an entity of the unit has no marker naming a line its declaration covers.
/// Otherwise accepted, with one source map per output holding the SHA-256 of the
/// request's source, the time of acceptance, and every marker in output order. Whether an
/// output builds or its tests pass is not checked here; the guidance says how, and the
/// caller runs it.
// Decision: the definition passes only the request; the workspace and plan the request
// was made from are extra first parameters, since the request refers to its unit by index.
// @lfy def/generation/main.lfy:87
pub fn accept(workspace: &Workspace, plan: &Plan, request: &Request, outputs: &[Output]) -> Verdict {
    let model = &workspace.model;
    let unit = &plan.units[request.unit];
    let target = &workspace.targets[unit.target];
    let file = &workspace.files[unit.file];
    let mut problems: Vec<String> = Vec::new();

    if outputs.is_empty() {
        // @lfy def/generation/main.lfy:89
        problems.push(format!("no output was produced for {}", file.path));
    }

    // Decision: the file's last line is counted from the request's source, which is the
    // text the outputs were generated from.
    let last_line = request.source.lines().count().max(1);
    let parsed: Vec<Vec<Marker>> = outputs.iter().map(|output| parse_markers(&output.text)).collect();

    for (output, markers) in outputs.iter().zip(&parsed) {
        if !under(&output.path, &target.output_directory) {
            // @lfy def/generation/main.lfy:90
            problems.push(format!(
                "the output {} is not under the output directory {} of the target {}",
                output.path, target.output_directory, target.identifier
            ));
        }
        for marker in markers {
            // @lfy def/generation/main.lfy:91
            // Decision: an output may carry markers for other files of the program (one
            // Rust file often mirrors several definition files); only a marker naming a
            // file outside the program, or a line past its end, is a problem.
            if marker.file != file.path {
                if !workspace.files.iter().any(|f| f.path == marker.file) {
                    problems.push(format!(
                        "{}:{}: the marker names {}, which is not in the program",
                        output.path, marker.output_line, marker.file
                    ));
                }
            } else if marker.line == 0 || marker.line > last_line {
                problems.push(format!(
                    "{}:{}: the marker names line {} of {}, whose last line is {last_line}",
                    output.path, marker.output_line, marker.line, file.path
                ));
            }
        }
    }

    // @lfy def/generation/main.lfy:92
    for &entity in &unit.entities {
        let Some((first, last)) = declaration_lines(model, entity) else { continue };
        let covered = parsed
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
        };
    }

    // @lfy def/generation/main.lfy:93
    let hash = source_hash(&request.source);
    let generated = now_rfc3339();
    let source_maps = outputs
        .iter()
        .zip(parsed)
        .map(|(output, markers)| SourceMap {
            target: target.identifier.clone(),
            output: output.path.clone(),
            source: file.path.clone(),
            hash: hash.clone(),
            generated: generated.clone(),
            markers,
        })
        .collect();
    Verdict {
        accepted: true,
        problems: Vec::new(),
        source_maps,
    }
}

/// The first and last source line an entity's declaration covers, documentation
/// included; `None` for an entity with no node.
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
        .map_or(info.start, |documentation| documentation.start.min(info.start));
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

/// Every `@lfy <path>:<line>[:<column>]` marker in an output's text, with the output line
/// it sits on, in output order. The marker sits after the line comment opener of the
/// target's language, whatever that is, so `@lfy ` is matched anywhere in a line.
// @lfy def/generation/data.lfy:16
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

/// A marker from what follows `@lfy `: `path:line` or `path:line:column`.
fn parse_marker(token: &str, output_line: usize) -> Option<Marker> {
    // Decision: punctuation that closes a sentence or a comment after the marker is not
    // part of it.
    let token = token.trim_end_matches(['.', ',', ';', ')', ']', '}', '*', '/', '-']);
    let mut parts = token.rsplitn(3, ':');
    let last = parts.next()?;
    let middle = parts.next()?;
    let first = parts.next();
    if let (Some(path), Ok(line), Ok(column)) = (first, middle.parse::<usize>(), last.parse::<usize>())
        && !path.is_empty()
    {
        return Some(Marker {
            output_line,
            file: path.to_string(),
            line,
            column: Some(column),
        });
    }
    let path = match first {
        Some(first) => format!("{first}:{middle}"),
        None => middle.to_string(),
    };
    let line = last.parse::<usize>().ok()?;
    if path.is_empty() {
        return None;
    }
    Some(Marker {
        output_line,
        file: path,
        line,
        column: None,
    })
}

/// SHA-256 of a text's bytes, as lowercase hex.
// @lfy def/generation/data.lfy:23
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
// @lfy def/generation/data.lfy:19
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
// @lfy def/generation/data.lfy:19
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
// @lfy def/generation/data.lfy:24
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

/// Two timestamps compared as instants when both parse, else as text.
fn compare_timestamps(a: &str, b: &str) -> std::cmp::Ordering {
    match (parse_rfc3339(a), parse_rfc3339(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => a.cmp(b),
    }
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
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 } as u32;
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

    fn map_for(workspace: &Workspace, path: &str, output: &str, generated: &str) -> SourceMap {
        let file = &workspace.files[file_index(workspace, path)];
        SourceMap {
            target: "rust".to_string(),
            output: output.to_string(),
            source: path.to_string(),
            hash: source_hash(&source_text(workspace, file)),
            generated: generated.to_string(),
            markers: Vec::new(),
        }
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

    // @lfy def/generation/main.lfy:33
    #[test]
    fn two_files_give_two_fresh_units_with_b_before_a() {
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
        // @lfy def/generation/main.lfy:16
        assert_eq!(names(&workspace, &a.entities), ["A"]);
        assert_eq!(names(&workspace, &b.entities), ["B", "make"]);
        // @lfy def/generation/main.lfy:18
        assert_eq!(a.stem, "a");
        assert_eq!(b.stem, "b");
        assert_eq!(a.target, 0);
        assert_eq!(a.file, file_index(&workspace, "def/a.lfy"));
        assert_eq!(plan.planned().count(), 2);
    }

    // @lfy def/generation/main.lfy:37
    #[test]
    fn matching_source_maps_leave_no_reason() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let maps = [
            map_for(&workspace, "def/a.lfy", "src/a.rs", "2026-09-18T10:00:00Z"),
            map_for(&workspace, "def/b.lfy", "src/b.rs", "2026-09-18T10:00:00Z"),
        ];
        let plan = plan(&workspace, &maps, &[]);
        assert_eq!(plan.units.len(), 2);
        assert!(plan.units.iter().all(|unit| unit.reason.is_none()), "{:?}", plan.units);
        // @lfy def/generation/main.lfy:28
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(a.outputs, [maps[0].clone()]);
        assert_eq!(plan.planned().count(), 0);
    }

    // @lfy def/generation/main.lfy:41
    #[test]
    fn an_edited_dependency_is_changed_and_its_dependent_is_stale() {
        let fixture = a_and_b();
        let old = fixture.load();
        assert_bound(&old);
        let maps = [
            map_for(&old, "def/a.lfy", "src/a.rs", "2026-09-18T10:00:00Z"),
            map_for(&old, "def/b.lfy", "src/b.rs", "2026-09-18T10:00:00Z"),
        ];
        fixture.write("def/b.lfy", &format!("{B}\n// edited\n"));
        let workspace = fixture.load();
        let plan = plan(&workspace, &maps, &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(b.reason, Some(Reason::Changed));
        assert_eq!(a.reason, Some(Reason::Dependency));
    }

    // @lfy def/generation/main.lfy:45
    #[test]
    fn a_marker_nothing_carries_gives_no_units() {
        let fixture = a_and_b();
        fixture.write("targets/rust/main.lfy", "trait rust { }\n");
        let workspace = fixture.load();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        assert_eq!(workspace.targets.len(), 1);
        let plan = plan(&workspace, &[], &[]);
        assert!(plan.units.is_empty(), "{:?}", plan.units);
    }

    // @lfy def/generation/main.lfy:15
    #[test]
    fn an_entity_carrying_the_marker_itself_is_built() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("targets/rust/main.lfy", "trait rust { }\n")
            .write("def/a.lfy", "use \"rust\";\n\nd Marked is rust { $x = string; }\nd Plain { $y = string; }\n");
        let workspace = fixture.load();
        assert!(workspace.problems.is_empty(), "{:?}", workspace.problems);
        let plan = plan(&workspace, &[], &[]);
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(names(&workspace, &plan.units[0].entities), ["Marked"]);
    }

    // @lfy def/generation/main.lfy:17
    #[test]
    fn a_package_file_gives_no_unit_and_a_nested_file_keeps_its_directory_in_its_stem() {
        let fixture = Fixture::with_rust_target();
        fixture.write("def/deep/inner.lfy", "d Inner { $x = string; }\n");
        let workspace = fixture.load();
        assert_bound(&workspace);
        // The package's main.lfy declares a trait global carries, but has a package.
        assert!(workspace.file("targets/rust/main.lfy").unwrap().package.is_some());
        let plan = plan(&workspace, &[], &[]);
        assert_eq!(plan.units.len(), 1, "{:?}", plan.units);
        assert_eq!(plan.units[0].stem, "deep/inner"); // @lfy def/generation/main.lfy:18
        assert_eq!(plan.units[0].file, file_index(&workspace, "def/deep/inner.lfy"));
    }

    // @lfy def/generation/main.lfy:22
    #[test]
    fn a_used_file_with_no_unit_contributes_the_units_of_its_own_uses() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/a.lfy", "use \"./c\";\nuse \"./b\";\n\nd A { $x = string; }\n")
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
        // @lfy def/generation/main.lfy:23
        assert_eq!(a.dependencies, [d_index, b_index]);
        assert!(a_index > b_index && a_index > d_index); // @lfy def/generation/main.lfy:24
    }

    // @lfy def/generation/main.lfy:23
    #[test]
    fn a_cycle_among_uses_adds_nothing() {
        let fixture = Fixture::with_rust_target();
        fixture
            .write("def/a.lfy", "use \"./b\";\nd A { $x = string; }\n")
            .write("def/b.lfy", "use \"./a\";\nd B { $x = string; }\n");
        let workspace = fixture.load();
        assert!(workspace.load_problems().count() == 2, "{:?}", workspace.problems);
        let plan = plan(&workspace, &[], &[]);
        assert_eq!(plan.units.len(), 2);
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, b) = unit_of(&workspace, &plan, "def/b.lfy");
        assert_eq!(a.dependencies, [b_index]);
        assert_eq!(b.dependencies, [a_index]);
        assert!(plan.units.iter().all(|unit| unit.reason == Some(Reason::Fresh)));
    }

    // @lfy def/generation/main.lfy:29
    #[test]
    fn requested_comes_before_every_other_reason_by_path_or_stem() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let maps = [
            map_for(&workspace, "def/a.lfy", "src/a.rs", "2026-09-18T10:00:00Z"),
            map_for(&workspace, "def/b.lfy", "src/b.rs", "2026-09-18T10:00:00Z"),
        ];
        let plan = plan(&workspace, &maps, &["b".to_string()]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(b.reason, Some(Reason::Requested));
        assert_eq!(a.reason, Some(Reason::Dependency));
        let plan = super::plan(&workspace, &[], &["def/a.lfy".to_string()]);
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(a.reason, Some(Reason::Requested));
    }

    // @lfy def/generation/main.lfy:29
    #[test]
    fn a_dependency_generated_later_than_the_units_earliest_output_makes_it_stale() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let maps = [
            map_for(&workspace, "def/a.lfy", "src/a.rs", "2026-09-18T10:00:00Z"),
            map_for(&workspace, "def/a.lfy", "src/a_tests.rs", "2026-09-18T12:00:00Z"),
            map_for(&workspace, "def/b.lfy", "src/b.rs", "2026-09-18T11:00:00+00:00"),
        ];
        let plan = plan(&workspace, &maps, &[]);
        let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(b.reason, None);
        assert_eq!(a.outputs.len(), 2);
        assert_eq!(a.reason, Some(Reason::Dependency));
        // A dependency generated at the same instant in another spelling is not later.
        let maps = [
            map_for(&workspace, "def/a.lfy", "src/a.rs", "2026-09-18T10:00:00Z"),
            map_for(&workspace, "def/b.lfy", "src/b.rs", "2026-09-18T12:00:00+02:00"),
        ];
        let plan = super::plan(&workspace, &maps, &[]);
        let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(a.reason, None);
    }

    // @lfy def/generation/main.lfy:81
    #[test]
    fn a_request_names_the_target_quotes_the_entities_and_has_one_interface() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        let request = request(&workspace, &plan, a_index, &[], None);
        assert_eq!(request.unit, a_index);
        assert_eq!(request.source, A); // @lfy def/generation/main.lfy:53
        assert_eq!(request.previous, None);
        assert!(request.existing.is_empty());
        // @lfy def/generation/main.lfy:55
        assert_eq!(request.interfaces.len(), 1);
        assert_eq!(request.interfaces[0].unit, b_index);
        assert!(request.interfaces[0].outputs.is_empty());
        assert_eq!(names(&workspace, &request.interfaces[0].entities), ["B", "make"]);
        // @lfy def/generation/main.lfy:56
        let marker = workspace.targets[0].marker;
        assert_eq!(request.guidance, model::criteria_of(&workspace.model, marker));
        assert_eq!(request.guidance.len(), 1);
        // @lfy def/generation/main.lfy:57
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
        assert!(text.contains("`def/a.lfy`"), "{text}");
        assert!(text.contains("under `src`"), "{text}");
        assert!(text.contains("stem `a`"), "{text}");
        assert!(text.contains("### `A` (data: DataDeclaration)"), "{text}");
        assert!(text.contains("/// A record.\nd A: `An a` {\n  $b = B;\n}"), "{text}");
        assert!(text.contains("Definition: An a"), "{text}");
        assert!(text.contains("Each unit becomes one module named after its stem"), "{text}");
        assert!(text.contains("`serde_json` (cargo), version `1`"), "{text}");
        assert!(text.contains("`sha2` (cargo), any version"), "{text}");
        assert!(text.contains("### `def/b.lfy`"), "{text}");
        assert!(text.contains("- `B` (data: DataDeclaration): A b"), "{text}");
        assert!(text.contains("- `make` (agent function: AgentFunctionDeclaration): Makes a b"), "{text}");
        assert!(text.contains("`@lfy def/a.lfy:1`"), "{text}");
        assert!(
            text.contains("An ambiguous criterion, two criteria in conflict, or a name that resolves nowhere means no output for the unit and a report quoting the criterion, never a guess"),
            "{text}"
        );
        assert!(text.contains("agent server"), "{text}");
        assert!(!text.contains("## Existing outputs"), "{text}");

        // The sections come in the definition's order. @lfy def/generation/main.lfy:64
        let headings = [
            "# Compiling",
            "## Where the outputs go",
            "## The entities to compile",
            "## Guidance",
            "## Native dependencies",
            "## Interfaces",
            "## Rules for kinds",
            "## Markers",
            "## Tests",
            "## Ambiguity",
            "## The agent server",
        ];
        let positions: Vec<usize> = headings
            .iter()
            .map(|heading| text.find(heading).unwrap_or_else(|| panic!("{heading} is missing:\n{text}")))
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{positions:?}");
    }

    // @lfy def/generation/main.lfy:67
    #[test]
    fn a_request_quotes_every_criterion_and_test_of_an_entity() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        let (b_index, _) = unit_of(&workspace, &plan, "def/b.lfy");
        let request = request(&workspace, &plan, b_index, &[], None);
        let text = &request.instructions;
        assert!(text.contains("### `make` (agent function: AgentFunctionDeclaration)"), "{text}");
        assert!(text.contains("- When x is empty: the b holds x"), "{text}");
        assert!(text.contains("- Input `[\"y\"]` gives `B@like(`holding y`)`"), "{text}");
        assert!(text.contains("This unit depends on no other unit."), "{text}");
    }

    // @lfy def/generation/main.lfy:54
    #[test]
    fn existing_outputs_are_kept_only_where_they_are_among_the_units_outputs() {
        let fixture = a_and_b();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let maps = [map_for(&workspace, "def/a.lfy", "src/a.rs", "2026-09-18T10:00:00Z")];
        let plan = plan(&workspace, &maps, &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let existing = [
            Output { path: "src/a.rs".to_string(), text: "// old".to_string() },
            Output { path: "src/other.rs".to_string(), text: "// other".to_string() },
        ];
        let with_previous = request(&workspace, &plan, a_index, &existing, Some("d A {}"));
        assert_eq!(with_previous.existing.len(), 1);
        assert_eq!(with_previous.existing["src/a.rs"], "// old");
        assert_eq!(with_previous.previous.as_deref(), Some("d A {}"));
        // @lfy def/generation/main.lfy:73
        assert!(with_previous.instructions.contains("## Existing outputs"));
        assert!(with_previous.instructions.contains("keep the markers of unchanged lines"));
        assert!(with_previous.instructions.contains("write to the same paths:\n- `src/a.rs`"));
        let without = request(&workspace, &plan, a_index, &existing, None);
        assert!(without.instructions.contains("Reconcile the existing output with the source"));
    }

    fn accepted_output() -> Output {
        Output {
            path: "src/a.rs".to_string(),
            text: "// @lfy def/a.lfy:4\npub struct A;\n\n// @lfy def/a.lfy:8\npub struct B;\n".to_string(),
        }
    }

    /// One file holding entities A and B, both built.
    fn a_with_two_entities() -> Fixture {
        let fixture = Fixture::with_rust_target();
        fixture.write(
            "def/a.lfy",
            "\n\n/// A.\nd A {\n  $x = string;\n}\n\nd B {\n  $y = string;\n}\n",
        );
        fixture
    }

    // @lfy def/generation/main.lfy:98
    #[test]
    fn outputs_with_a_marker_per_entity_are_accepted_with_one_source_map() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        assert_bound(&workspace);
        let plan = plan(&workspace, &[], &[]);
        let (a_index, a) = unit_of(&workspace, &plan, "def/a.lfy");
        assert_eq!(names(&workspace, &a.entities), ["A", "B"]);
        let request = request(&workspace, &plan, a_index, &[], None);
        let verdict = accept(&workspace, &plan, &request, &[accepted_output()]);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        assert!(verdict.problems.is_empty());
        assert_eq!(verdict.source_maps.len(), 1);
        let map = &verdict.source_maps[0];
        assert_eq!(map.target, "rust");
        assert_eq!(map.output, "src/a.rs");
        assert_eq!(map.source, "def/a.lfy");
        assert_eq!(map.hash, source_hash(&request.source)); // @lfy def/generation/main.lfy:93
        assert!(parse_rfc3339(&map.generated).is_some(), "{}", map.generated);
        assert_eq!(
            map.markers,
            [
                Marker { output_line: 1, file: "def/a.lfy".to_string(), line: 4, column: None },
                Marker { output_line: 4, file: "def/a.lfy".to_string(), line: 8, column: None },
            ]
        );
        // The source map round-trips through the plan: the unit is now up to date.
        let plan = super::plan(&workspace, &verdict.source_maps, &[]);
        assert_eq!(plan.units[a_index].reason, None);
    }

    // @lfy def/generation/main.lfy:102
    #[test]
    fn an_entity_without_a_marker_is_rejected_by_name() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let plan = plan(&workspace, &[], &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let request = request(&workspace, &plan, a_index, &[], None);
        // A marker on the documentation line of A counts for A; nothing names B.
        let output = Output {
            path: "src/a.rs".to_string(),
            text: "// @lfy def/a.lfy:3\npub struct A;\n".to_string(),
        };
        let verdict = accept(&workspace, &plan, &request, &[output]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(verdict.problems[0].contains("the entity B"), "{}", verdict.problems[0]);
        assert!(verdict.source_maps.is_empty());
    }

    // @lfy def/generation/main.lfy:89
    #[test]
    fn no_outputs_are_rejected() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let plan = plan(&workspace, &[], &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let request = request(&workspace, &plan, a_index, &[], None);
        let verdict = accept(&workspace, &plan, &request, &[]);
        assert!(!verdict.accepted);
        assert!(verdict.problems[0].contains("no output"), "{:?}", verdict.problems);
    }

    // @lfy def/generation/main.lfy:90
    #[test]
    fn an_output_outside_the_output_directory_is_rejected_by_path() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let plan = plan(&workspace, &[], &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let request = request(&workspace, &plan, a_index, &[], None);
        let mut output = accepted_output();
        output.path = "srcx/a.rs".to_string();
        let verdict = accept(&workspace, &plan, &request, &[output]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
        assert!(verdict.problems[0].contains("srcx/a.rs"), "{}", verdict.problems[0]);
    }

    // @lfy def/generation/main.lfy:91
    #[test]
    fn a_marker_naming_another_file_or_a_line_past_the_last_is_rejected_by_output_line() {
        let fixture = a_with_two_entities();
        let workspace = fixture.load();
        let plan = plan(&workspace, &[], &[]);
        let (a_index, _) = unit_of(&workspace, &plan, "def/a.lfy");
        let request = request(&workspace, &plan, a_index, &[], None);
        let output = Output {
            path: "src/a.rs".to_string(),
            text: "// @lfy def/a.lfy:4\n// @lfy def/b.lfy:8\n\n// @lfy def/a.lfy:99\n// @lfy def/a.lfy:8\n".to_string(),
        };
        let verdict = accept(&workspace, &plan, &request, &[output]);
        assert!(!verdict.accepted);
        assert_eq!(verdict.problems.len(), 2, "{:?}", verdict.problems);
        assert!(verdict.problems[0].starts_with("src/a.rs:2:"), "{}", verdict.problems[0]);
        assert!(verdict.problems[0].contains("def/b.lfy"), "{}", verdict.problems[0]);
        assert!(verdict.problems[1].starts_with("src/a.rs:4:"), "{}", verdict.problems[1]);
        assert!(verdict.problems[1].contains("line 99"), "{}", verdict.problems[1]);
    }

    // @lfy def/generation/data.lfy:16
    #[test]
    fn markers_are_parsed_after_any_line_comment_opener() {
        let text = "// @LFY def/a.lfy:4\nfn x() {}\n# @LFY def/a.lfy:5:2\n-- @LFY def/a.lfy:6 -- more\n; @LFY def/a.lfy:7.\nnothing here\n/* @LFY def/a.lfy:8 */\n// @LFY nope\n// @LFY :3\n".replace("@LFY", "@lfy");
        let markers = parse_markers(&text);
        let marker = |output_line, line, column| Marker {
            output_line,
            file: "def/a.lfy".to_string(),
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
            ]
        );
        assert_eq!(markers[1].spelling(), "@lfy def/a.lfy:5:2");
        assert_eq!(markers[0].spelling(), "@lfy def/a.lfy:4");
        assert!(parse_markers("").is_empty());
    }

    // @lfy def/generation/data.lfy:23
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

    // @lfy def/generation/data.lfy:19
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
                generated: "2026-09-18T10:00:00Z".to_string(),
                markers: vec![
                    Marker { output_line: 1, file: "def/a.lfy".to_string(), line: 4, column: None },
                    Marker { output_line: 9, file: "def/a.lfy".to_string(), line: 5, column: Some(2) },
                ],
            },
            SourceMap {
                target: "rust".to_string(),
                output: "src/b.rs".to_string(),
                source: "def/b.lfy".to_string(),
                hash: source_hash("y"),
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

    // @lfy def/generation/data.lfy:24
    #[test]
    fn timestamps_are_rfc3339_in_utc_to_the_second() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_789_689_600), "2026-09-18T00:00:00Z");
        assert_eq!(parse_rfc3339("2026-09-18T00:00:00Z"), Some(1_789_689_600));
        assert_eq!(parse_rfc3339("2026-09-18T02:00:00.5+02:00"), Some(1_789_689_600));
        assert_eq!(parse_rfc3339("2026-09-17T22:30:00-01:30"), Some(1_789_689_600));
        assert_eq!(parse_rfc3339("yesterday"), None);
        assert_eq!(parse_rfc3339("2026-13-01T00:00:00Z"), None);
        let now = now_rfc3339();
        assert_eq!(now.len(), 20, "{now}");
        assert!(now.ends_with('Z'));
        let seconds = parse_rfc3339(&now).unwrap();
        assert_eq!(format_rfc3339(seconds), now);
        assert!(seconds > 1_789_689_600);
        assert!(compare_timestamps("2026-09-18T10:00:00Z", "2026-09-18T12:00:00+02:00").is_eq());
        assert!(compare_timestamps("b", "a").is_gt());
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
        assert_eq!(order_after_dependencies(&[vec![1], vec![2], vec![]]), [2, 1, 0]);
        assert_eq!(order_after_dependencies(&[vec![1], vec![0]]), [1, 0]);
    }
}
