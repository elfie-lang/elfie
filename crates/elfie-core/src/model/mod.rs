//! Compiled from `def/model`: the bound program and its queries.

mod bind;
mod components;
pub mod data;
mod eval;
mod trees;

use std::rc::Rc;

pub use bind::SYNTHETIC;
pub use components::{accessor_layer, declaring, is_scoped, reading_layer};
pub use data::*;

/// Build the model of a program from the resolved files of all of it.
///
/// Binding runs three passes over every file: declare (scopes, symbols, entities), apply
/// (traits, members, criteria), resolve (usages, properties, templates). The sources are
/// bound in the order given, which places every file after the files it uses. Binding
/// never stops; every failure is a problem.
// @lfy def/model/main.lfy:10
pub fn bind(sources: Vec<Source>) -> Model {
    let mut binder = bind::Binder::new(sources);
    let files = binder.trees.sources.len();
    for file in 0..files {
        binder.declare_file(file); // @lfy def/model/main.lfy:23
    }
    for file in 0..files {
        binder.link_uses(file); // @lfy def/model/main.lfy:37
    }
    for file in 0..files {
        binder.type_entities(file); // @lfy def/model/main.lfy:81
    }
    for file in 0..files {
        binder.apply_file(file); // @lfy def/model/main.lfy:43
    }
    for file in 0..files {
        binder.resolve_file(file); // @lfy def/model/main.lfy:63
    }
    binder.finish_definitions();
    let mut model = binder.model;
    model.problems.sort_by_key(|problem| (problem.node.file, problem.node.index));
    model.sources = match Rc::try_unwrap(binder.trees) {
        Ok(trees) => trees.sources,
        Err(shared) => shared.sources.clone(),
    };
    model
}

impl bind::Binder {
    /// Entity.definition for every entity whose declaration gave one and whose body did
    /// not run.
    // @lfy def/model/main.lfy:85
    fn finish_definitions(&mut self) {
        for entity in 0..self.model.entities.len() {
            if self.model.entities[entity].definition.is_some() {
                continue;
            }
            let Some(node) = self.model.entities[entity].definition_node else { continue };
            let scope = self.model.enclosing_scope(node);
            let env = eval::Env {
                vars: Vec::new(),
                current: entity,
                target: entity,
                contributor: entity,
                scope,
                in_trait: false,
                dry: true,
                ret: None,
            };
            let text = self.text_of(node, &env);
            self.model.entities[entity].definition = Some(text);
        }
    }
}

/// The symbol a node uses or declares.
// @lfy def/model/main.lfy:124
pub fn resolve(model: &Model, node: NodeRef) -> Option<SymbolId> {
    if let Some(usage) = model.usage_of(node) {
        return model.usages[usage].symbol; // @lfy def/model/main.lfy:127
    }
    model.symbol_of(node) // @lfy def/model/main.lfy:128
}

/// Everything that has a trait; empty when the entity is not a trait.
// @lfy def/model/main.lfy:131
pub fn entities_of(model: &Model, trait_entity: EntityId) -> Vec<EntityId> {
    model.entities[trait_entity].entities().to_vec()
}

/// Every use of a symbol: every usage whose symbol is the target or an alias chain
/// ending at it, in node order.
// @lfy def/model/main.lfy:137
pub fn usages_of(model: &Model, target: SymbolId) -> Vec<UsageId> {
    let entity = model.symbols[target].entity;
    let mut out: Vec<UsageId> = model
        .usages
        .iter()
        .enumerate()
        .filter(|(_, usage)| {
            usage.symbol.is_some_and(|symbol| {
                symbol == target || (model.symbols[symbol].kind == SymbolKind::Alias && model.symbols[symbol].entity == entity)
            })
        })
        .map(|(id, _)| id)
        .collect();
    out.sort_by_key(|&id| (model.usages[id].node.file, model.usages[id].node.index));
    out
}

/// Every criterion attached to an entity, with templates resolved for it: each
/// reference is replaced by the referenced entity's identifier and each execution by
/// its value.
// @lfy def/model/main.lfy:142
pub fn criteria_of(model: &Model, entity: EntityId) -> Vec<Criterion> {
    model.entities[entity]
        .acceptance_criteria
        .iter()
        .map(|criterion| Criterion {
            situation: criterion.situation.as_ref().map(|s| s.iter().map(|t| strip_references(t)).collect()),
            behavior: criterion.behavior.as_ref().map(|s| s.iter().map(|t| strip_references(t)).collect()),
            side_effects: criterion.side_effects.as_ref().map(|s| s.iter().map(|t| strip_references(t)).collect()),
            contributor: criterion.contributor,
            node: criterion.node,
        })
        .collect()
}

/// `[[X]]` becomes `X`.
pub fn strip_references(text: &str) -> String {
    text.replace("[[", "").replace("]]", "")
}

/// The text of a value, as the binder would write it.
pub fn value_text(model: &Model, value: &Value) -> String {
    match value {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if n.fract() == 0.0 { format!("{}", *n as i64) } else { n.to_string() }
        }
        Value::String(s) => s.clone(),
        Value::List(items) => items.iter().map(|i| value_text(model, i)).collect::<Vec<_>>().join(", "),
        Value::Object(pairs) => pairs.iter().map(|(k, v)| format!("{k} = {}", value_text(model, v))).collect::<Vec<_>>().join(", "),
        Value::Entity(e) => model.entities[*e].identifier.clone().unwrap_or_else(|| "anonymous".to_string()),
        Value::Type(ty) => type_text(model, ty),
        Value::Scope(_) => "scope".to_string(),
        Value::Closure(..) | Value::Function(..) => "function".to_string(),
        Value::Criteria(_) => "acceptanceCriteria".to_string(),
        Value::Tester(_) => "test".to_string(),
        Value::Like(inner) => value_text(model, inner),
    }
}

/// The text of a type: identifiers, `T[]`, `A | B`, `is T`.
pub fn type_text(model: &Model, ty: &TypeRef) -> String {
    match ty {
        TypeRef::Entity(e) => model.entities[*e].identifier.clone().unwrap_or_else(|| "anonymous".to_string()),
        TypeRef::Primitive(p) => (*p).to_string(),
        TypeRef::Literal(v) => value_text(model, v),
        TypeRef::List(item) => format!("{}[]", type_text(model, item)),
        TypeRef::Union(items) => items.iter().map(|i| type_text(model, i)).collect::<Vec<_>>().join(" | "),
        TypeRef::Predicate(t) => format!("is {}", model.entities[*t].identifier.clone().unwrap_or_default()),
        TypeRef::Function => "function".to_string(),
        TypeRef::Unknown(text) => text.trim().to_string(),
    }
}
