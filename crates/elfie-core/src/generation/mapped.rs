// @lfy def/generation/maps.lfy:regionsOf
//! Mapped output access: regions, changes, rewordings, and renamed markers.

use super::*;

/// Every region of generated output that came from one entity.
///
/// `name` is an identifier, or an owner's identifier, a dot, and a member's name; `target`
/// is the identifier of one target, or `None` for every target. The markers come source map
/// by source map in the order they are given, and within one in the order of its markers,
/// which is output order. A marker whose entity is `name`, a dot, and a member's name comes
/// too, since a member's region is part of its owner's code, and a marker that names a line
/// rather than an entity comes when its file declares the entity named `name` and the line
/// falls inside its declaring node. Nothing matching gives an empty list.
// @lfy def/generation/maps.lfy:regionsOf
pub fn regions_of(
    workspace: &Workspace,
    source_maps: &[SourceMap],
    name: &str,
    target: Option<&str>,
) -> Vec<Marker> {
    // @lfy def/generation/maps.lfy:regionsOf
    matching_regions(workspace, source_maps, name, target)
        .into_iter()
        .map(|(_, marker)| marker.clone())
        .collect()
}

// ---------------------------------------------------------------------------------------
// changes
// ---------------------------------------------------------------------------------------

/// What a change of kind added says, and what one of kind removed says.
const ADDED: &str = "it is declared now and was not in the file the outputs were generated from";
const REMOVED: &str = "it was declared in the file the outputs were generated from and is not now";
/// How long a text may be for a detail to quote it rather than name it.
// @lfy def/generation/changes.lfy:changes
const DETAIL_LIMIT: usize = 80;

/// What differs for each entity of a unit between its file as last accepted and as it is
/// now.
///
/// `previous` is the unit's source at its last accepted generation, or its hash-matched
/// source, as recovered for the unit, and `None` when neither is known, which makes every
/// entity of the unit an addition. The file as it was is read from the program of
/// `previous`, already lowered, and its entities are compared with the unit's by name — the
/// identifier, or the owner's identifier, a dot, and a member's name — so a reformatted or
/// reordered file whose entities read the same gives no change at all.
// Decision: a change is found by comparing entities, never by diffing lines.
// @lfy def/generation/changes.lfy:changes
pub fn changes(program: &Program, unit: &Unit, previous: Option<&Previous>) -> Vec<Change> {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let path = workspace.files[unit.file].path.clone();
    let target = workspace.targets[unit.target].identifier.clone();
    // What the unit's file declares for its target, in file order — not what the unit
    // compiles: an entity that moved to another unit is still one of this file's, and one
    // that moved in is the declaring file's to report.
    // @lfy def/generation/changes.lfy:changes#changes:changes:718b0ba1ae484d5d40adbb3d2dabcfed7ec6b7a0c976dd9193d6c8b00e479f4f
    let declared = declared_entities(program, &target, &path);
    let now = entity_table(model, &declared);
    let Some(previous) = previous else {
        // @lfy def/generation/changes.lfy:changes#changes:changes:771d19c7cdc3e68ae80e9c437cf28ae04d6d6112c99fdff85bcf61c1efacc428
        return declared
            .iter()
            .map(|&entity| Change {
                entity: entity_name(model, entity),
                kind: ChangeKind::Added,
                detail: ADDED.to_string(),
            })
            .collect();
    };
    // The file as it was is the one the program of `previous` declares, lowered once when it
    // was recovered, so its entities are read from that program's model and nothing is
    // lowered or bound here.
    // @lfy def/generation/changes.lfy:changes#changes:changes:2c1d93c21febc2f19ccf11a3653269f8b49c306e0c8cbf783655ab561be4a92f
    let before = &previous.program;
    // What the file declared then, built for the unit's target, read exactly as what it
    // declares now was read above, so that neither side is a unit's contents.
    // @lfy def/generation/changes.lfy:changes#changes:changes:718b0ba1ae484d5d40adbb3d2dabcfed7ec6b7a0c976dd9193d6c8b00e479f4f
    let was = entity_table(
        &before.workspace.model,
        &declared_entities(before, &target, &path),
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
    // @lfy def/generation/changes.lfy:changes#changes:changes:3da24f82f28d461099bd3d405b1cb83ee6ea222a16f0a5a4ead591257085c4f7
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
    // @lfy def/generation/changes.lfy:changes#changes:changes:8bf838e57cd210c5a27390714d305ae478381b305687353e2e12088e09cb7b10
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
            // @lfy def/generation/changes.lfy:changes
            (None, Some(_)) => out.push(Change {
                entity: name,
                kind: ChangeKind::Added,
                detail: ADDED.to_string(),
            }),
            // @lfy def/generation/changes.lfy:changes
            (Some(_), None) => out.push(Change {
                entity: name,
                kind: ChangeKind::Removed,
                detail: REMOVED.to_string(),
            }),
            // @lfy def/generation/changes.lfy:changes
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
// @lfy def/generation/changes.lfy:changes
struct Side<'m> {
    model: &'m Model,
    entity: EntityId,
    criteria: Vec<String>,
    tests: Vec<String>,
}

/// Every entity of a unit and every member it declares, in file order, each with the name
/// it is compared by: the identifier, or the owner's identifier, a dot, and the member's
/// name.
// @lfy def/generation/changes.lfy:changes#changes:changes:d7b590dfd89c646eecf798bdafc911cd87dc2b66f0c3bc1e3cb916051b5fa6f8
// @lfy def/generation/changes.lfy:changes#changes:changes:f54761e4898e7ebd66aacb630307ee416935a1052c402126c3c712e2eb632c2a
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
// @lfy def/generation/changes.lfy:changes#changes:changes:718b0ba1ae484d5d40adbb3d2dabcfed7ec6b7a0c976dd9193d6c8b00e479f4f
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
// @lfy def/generation/changes.lfy:changes
fn differences(name: &str, old: &Side<'_>, new: &Side<'_>) -> Vec<Change> {
    let (was, now) = (old.model, new.model);
    let mut out: Vec<Change> = Vec::new();

    // @lfy def/generation/changes.lfy:changes
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

    // @lfy def/generation/changes.lfy:changes
    let old_type = type_spelling(was, old.entity);
    let new_type = type_spelling(now, new.entity);
    if old_type != new_type {
        out.push(Change {
            entity: name.to_string(),
            kind: ChangeKind::DeclaredType,
            detail: detail("the type", &old_type, &new_type),
        });
    }

    // @lfy def/generation/changes.lfy:changes
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
    // has the same id, and one that was reworded has another. @lfy def/generation/changes.lfy:changes
    if old.criteria != new.criteria {
        out.push(Change {
            entity: name.to_string(),
            kind: ChangeKind::Criteria,
            detail: id_detail("criteria", "criterion", &old.criteria, &new.criteria),
        });
    }

    // @lfy def/generation/changes.lfy:changes
    if old.tests != new.tests {
        out.push(Change {
            entity: name.to_string(),
            kind: ChangeKind::Tests,
            detail: id_detail("tests", "test", &old.tests, &new.tests),
        });
    }

    // @lfy def/generation/changes.lfy:changes
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
// @lfy def/generation/changes.lfy:changes#changes:changes:e44d4ec0e8b50ac4324b8435669db74cc0bb0e0e5eae175b424fd5a527f1065a
fn detail(what: &str, old: &str, new: &str) -> String {
    let (before, after) = (old.chars().count(), new.chars().count());
    if before < DETAIL_LIMIT && after < DETAIL_LIMIT {
        format!("{what} was `{old}` and is now `{new}`")
    } else {
        format!("{what} differs: {before} characters before and {after} now")
    }
}

/// One line naming which parameter or output differs.
// @lfy def/generation/changes.lfy:changes
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
// @lfy def/generation/changes.lfy:changes
fn id_detail(plural: &str, singular: &str, old: &[String], new: &[String]) -> String {
    let mut parts = vec![format!(
        "{} {plural} before and {} now",
        old.len(),
        new.len()
    )];
    let reworded = reworded_pairs(old, new);
    for (position, was, is) in &reworded {
        parts.push(format!(
            "the {singular} at {} was reworded, from {was} to {is}",
            position + 1
        ));
    }
    // @lfy def/generation/changes.lfy:changes
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

/// Which pairs of a removed and an added requirement id two lists of them call reworded:
/// counting from the first, the id at a position that the new list does not hold beside the
/// id at that same position that the old list did not hold. Each pair comes with its
/// position, counting from 0.
///
/// This is the one place the pairing is decided, so that [`id_detail`], which says it in a
/// line, and [`reworded`], which hands the pairs on, can never differ.
// @lfy def/generation/changes.lfy:changes#changes:changes:7d485ed12575ab1f8fb73fd1af344036fcd0e209e355d3895b8822a170c9d276
// @lfy def/generation/changes.lfy:changes#changes:changes:e4f0785b9f7bbfa92530f7edab6e298a57fbc81f4665bd43e72947b6327ef1d6
fn reworded_pairs<'i>(old: &'i [String], new: &'i [String]) -> Vec<(usize, &'i String, &'i String)> {
    old.iter()
        .zip(new)
        .enumerate()
        .filter(|(_, (was, is))| was != is && !new.contains(was) && !old.contains(is))
        .map(|(position, (was, is))| (position, was, is))
        .collect()
}

/// The type of an entity, as `interfaceOf` spells it; empty when it has none.
// @lfy def/generation/changes.lfy:changes
fn type_spelling(model: &Model, entity: EntityId) -> String {
    model::type_text_of(model, model.entities[entity].ty.as_ref()).unwrap_or_default()
}

/// The parameters of a fn with their types, as `interfaceOf` spells them.
// @lfy def/generation/changes.lfy:changes
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
// @lfy def/generation/changes.lfy:changes
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
// @lfy def/generation/changes.lfy:changes
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
// @lfy def/generation/changes.lfy:changes
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
// @lfy def/generation/changes.lfy:changes
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
// @lfy def/generation/changes.lfy:changes
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
// reworded, rename
// ---------------------------------------------------------------------------------------

/// A unit's previous source as [`changes`], [`reworded`], and [`request`] take it: `text`,
/// the unit's file as it was, as written, with the workspace lowered once with the file read
/// as `text` and every other file as it is, and whether it is only the hash-matched source.
// Decision: the callers that recover a previous source hold its text; the one lowering it
// needs is done here, once, so the functions that read it lower and bind nothing.
// @lfy def/generation/changes.lfy:changes
pub fn previous_of(workspace: &Workspace, unit: &Unit, text: &str, hash_matched: bool) -> Previous {
    let path = &workspace.files[unit.file].path;
    Previous {
        text: text.to_string(),
        program: crate::interpret::lower(crate::workspace::change(workspace, path, Some(text))),
        hash_matched,
    }
}

/// The local criteria and tests of a unit whose id changed since its outputs were accepted
/// only because their text did, each paired with the id it replaces and what that one said.
///
/// `previous` is the unit's source at its last accepted generation, as recovered for the
/// unit. One [`Rewording`] comes of each pair of a removed and an added id that a change
/// of [`changes`] calls reworded, in the order of those changes, so the two never differ
/// about what was reworded; a pair whose added id is the id of no local criterion or test of
/// the unit — one of an entity that moved to another unit, say — gives none. The text is
/// what the removed one said, read off the unit's file in the program of `previous`, exactly
/// as a [`RemovedRequirement`] takes its own. Nothing is written, lowered, or bound.
// Decision: the definition passes the workspace; the ids of a unit's local criteria and
// tests are the lowered program's, which a plan here does not own, so the program stands in
// for the workspace as it does for [`changes`].
// @lfy def/generation/changes.lfy:reworded#reworded:reworded:af54e4c28fb74d4c2fa75b4d1a8399d931aa52c592d5f16bd59ab25cc730a812
// @lfy def/generation/changes.lfy:reworded#reworded:reworded:ddbdcd5e429486d18ddb68f37de72b7e4f0a6da2b4f0a61f901c41b98bb3774e
pub fn reworded(program: &Program, unit: &Unit, previous: &Previous) -> Vec<Rewording> {
    let workspace = &program.workspace;
    let model = &workspace.model;
    let path = workspace.files[unit.file].path.clone();
    let target = workspace.targets[unit.target].identifier.clone();
    // The file as it was, lowered the one time it was recovered: its entities are compared
    // with the unit's, and what each of its criteria and tests said is read off the same
    // lowering.
    let before = &previous.program;
    let Some(index) = before
        .workspace
        .files
        .iter()
        .position(|file| file.path == path)
    else {
        return Vec::new();
    };
    let lowered_before = &before.files[index];
    // @lfy def/generation/changes.lfy:reworded#reworded:reworded:2f1cf2b33aaff46609462adfbaa4694777c741613a70a5ec0912b0a967cb69da
    let said: BTreeMap<String, String> = local_requirements(lowered_before)
        .iter()
        .map(|requirement| match requirement {
            Requirement::Criterion(criterion) => (criterion.id.clone(), criterion.text()),
            Requirement::Test(test) => (test.id.clone(), test_text(test)),
        })
        .collect();
    let now = entity_table(model, &declared_entities(program, &target, &path));
    let was = entity_table(
        &before.workspace.model,
        &declared_entities(before, &target, &path),
    );
    let lowered = &program.files[unit.lowered];
    let local: Vec<String> = unit
        .requirements(program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();

    let mut out: Vec<Rewording> = Vec::new();
    for (name, new) in &now {
        let Some(&(_, old)) = was.iter().find(|(candidate, _)| candidate == name) else {
            continue;
        };
        let (old_criteria, old_tests) = requirement_ids(lowered_before, old);
        let (new_criteria, new_tests) = requirement_ids(lowered, *new);
        // Criteria before tests, as a change of criteria comes before one of tests.
        for (old_ids, new_ids) in [(&old_criteria, &new_criteria), (&old_tests, &new_tests)] {
            for (_, replaces, id) in reworded_pairs(old_ids, new_ids) {
                // A pair whose added id is no local criterion or test of this unit belongs
                // to an entity the unit does not compile.
                // @lfy def/generation/changes.lfy:reworded#reworded:reworded:76d54235a5b3b04295f6e88bef925ebc2cdc34e3d520ac52acf3457939b122bc
                if !local.contains(id) {
                    continue;
                }
                // @lfy def/generation/changes.lfy:reworded#reworded:reworded:68b8e0096a61b2d00deb8e85b9cff1e55e3c804dbb2f9e01870cd1526955b171
                out.push(Rewording {
                    id: id.clone(),
                    replaces: replaces.clone(),
                    text: said.get(replaces).cloned().unwrap_or_default(),
                });
            }
        }
    }
    out
}

/// The output files in which a marker of one unit names an id being replaced, with each such
/// marker rewritten to name the new id.
///
/// `ids` maps each id a marker names now to the id it is to name afterwards. A marker is
/// read as [`accept`] reads one, and is rewritten only where it names a source path this
/// unit claims — its own file, or the file of an entity moved into it — since a marker
/// naming another file is another unit's. Everything else of the line, and every other line,
/// is kept, and an output that holds no rewritten marker is not returned at all. Nothing is
/// written: the files come back for the caller to write.
// Decision: one rule rewrites a marker's id, whether a compile moves the markers of reworded
// criteria before the compiler runs or a compiler asks `elfie_mark` to, so the two never
// differ.
// Decision: the definition passes the unit; which files a unit claims is the plan's, which
// does not own its workspace here, so the workspace and the plan are extra first parameters
// and the unit is given as its index into the plan, as [`accept`] takes it.
// @lfy def/generation/accept.lfy:rename#rename:rename:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn rename(
    workspace: &Workspace,
    plan: &Plan,
    unit: usize,
    outputs: &[Output],
    ids: &BTreeMap<String, String>,
) -> Vec<Output> {
    // The files whose markers are this unit's, exactly as acceptance claims them.
    // @lfy def/generation/accept.lfy:rename#rename:rename:3091710a31989abcb75e52c70a366b960dac1fabd585e5a3bf0971af66686c8f
    let planned = &plan.units[unit];
    let mut claimed: Vec<&str> = vec![&workspace.files[planned.file].path];
    for moved in plan.moves.iter().filter(|moved| moved.into == unit) {
        let path = &workspace.files[plan.units[moved.origin].file].path;
        if !claimed.contains(&path.as_str()) {
            claimed.push(path);
        }
    }
    let mut out: Vec<Output> = Vec::new();
    // @lfy def/generation/accept.lfy:rename#rename:rename:90a597b0c488f100859379a0d11bf0e632fbfbb941a554b7b5c2669c9d8320d3
    for output in outputs {
        // Each line with the break that ends it, so that what is not rewritten comes
        // through character for character, whatever a line ends with.
        let mut text = String::with_capacity(output.text.len());
        let mut changed = false;
        for line in output.text.split_inclusive('\n') {
            match renamed_line(line, ids, &claimed) {
                Some(line) => {
                    text.push_str(&line);
                    changed = true;
                }
                None => text.push_str(line),
            }
        }
        // @lfy def/generation/accept.lfy:rename#rename:rename:8cb5f57f44ec1a7de3b03c33feda6e4ceedd4fa55d4d2ec4fdd0ae92c27e9d82
        if !changed {
            continue;
        }
        out.push(Output {
            path: output.path.clone(),
            text,
        });
    }
    out
}

/// One line of an output with the id of every marker of it that names a replaced id
/// rewritten, or `None` when it holds no such marker.
///
/// The line is walked for markers the way [`parse_markers`] walks it, and each one's id is
/// replaced within the token it was spelled in, so that the comment opener, the path, the
/// entity, anything written after the marker, and the break that ends the line are left
/// exactly as they were.
// @lfy def/generation/accept.lfy:rename#rename:rename:98bd688d7b8dd91a3fbd9fc7eb8c0999c69eba315873a7bcb74f7627b0a5897d
// @lfy def/generation/accept.lfy:rename#rename:rename:4091b5d02c428166459a836f17453935a76b69ac96600c57604d197dccf120e3
fn renamed_line(line: &str, ids: &BTreeMap<String, String>, claimed: &[&str]) -> Option<String> {
    let mut out = String::new();
    let mut rest = line;
    let mut changed = false;
    while let Some(position) = rest.find(MARKER_PREFIX) {
        let start = position + MARKER_PREFIX.len();
        let (before, after) = rest.split_at(start);
        let length = after
            .find(char::is_whitespace)
            .unwrap_or(after.len());
        let (token, tail) = after.split_at(length);
        out.push_str(before);
        match parse_marker(token, 1) {
            Some(marker) => {
                let renamed = marker
                    .requirement
                    .as_ref()
                    .filter(|_| claimed.contains(&marker.file.as_str()))
                    .and_then(|requirement| {
                        ids.get(requirement)
                            .map(|id| token.replacen(requirement.as_str(), id, 1))
                    });
                match renamed {
                    Some(renamed) => {
                        out.push_str(&renamed);
                        changed = true;
                    }
                    None => out.push_str(token),
                }
            }
            None => out.push_str(token),
        }
        rest = tail;
    }
    out.push_str(rest);
    changed.then_some(out)
}
