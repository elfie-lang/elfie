// @lfy def/generation/main.lfy:plan
//! Every unit of a workspace, which need generating, and how they are batched.

use super::*;

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
/// Reaching is transitive, as [`reaches`] reads it: a type that only names an entity of its
/// own file whose own types lead to the other file reaches it too.
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
            // A type reaches across the cycle when what it names, or what the types of what
            // it names lead to however deep, is an entity of another file of the cycle:
            // reaching is transitive here, as [`reaches`] and the problem an entity not
            // built for the target leaves both read it, so a declaration that arrives at
            // the other file only through a type of its own file is a candidate too.
            // @lfy def/generation/main.lfy:plan
            let reaches_across = |ty: Option<&TypeRef>| {
                closure(model, &named_entities_of(model, ty))
                    .iter()
                    .any(foreign)
            };
            fallback = fallback.or(model.entities[owner].node);
            for member in all_members(model, owner) {
                if reaches_across(model.entities[member].ty.as_ref())
                    && let Some(node) = model.entities[member].node
                {
                    candidates.push(node);
                }
            }
            for &symbol in model.entities[owner].parameters() {
                let parameter = model.symbols[symbol].entity;
                if reaches_across(model.entities[parameter].ty.as_ref())
                    && let Some(node) = model.entities[parameter].node
                {
                    candidates.push(node);
                }
            }
            if reaches_across(model.entities[owner].output())
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
