//! Compiled from `def/model`: the bound program and its queries.

mod bind;
mod components;
pub mod data;
mod eval;
#[cfg(test)]
mod tests;
mod trees;
mod r#type;

use std::rc::Rc;

use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::Statement as S;
use crate::grammar::terminals::identifier::Identifier as I;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::grammar::terminals::literal::Literal as L;
use crate::grammar::terminals::punctuation::Punctuation as P;
use crate::grammar::GrammarRule;
use crate::parser::components::is_trivia;
use crate::parser::data::Child;

use trees::Part;

pub use bind::SYNTHETIC;
pub use components::{accessor_layer, declaring, is_scoped, reading_layer};
pub use data::*;
pub use r#type::type_text_of;

/// Build the model of a program from the resolved files of all of it.
///
/// Binding runs three passes over every file: declare (scopes, symbols, entities), expand
/// (traits, members, criteria, and every other compile-time statement), resolve (usages,
/// properties, templates). The sources are bound in the order given, which places every
/// file after the files it uses. Binding never stops; every failure it meets is a problem,
/// and an error node declares and uses nothing. Each source is bound once, however many
/// others use it.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
pub fn bind(sources: Vec<Source>) -> Model {
    let mut binder = bind::Binder::new(sources);
    let files = binder.trees.sources.len();
    for file in 0..files {
        binder.declare_file(file); // @lfy def/model/main.lfy:bind
    }
    // @lfy def/model/data.lfy:Scope#Scope:Scope:63702a99fb7a3d155498fd0cfc8839b62e9c0eebe6543cb81ff763006d571f34
    // @lfy def/model/data.lfy:Scope#Scope:Scope:4402361d3767c258bf2e3e234fe5e7f7afa4b16ff000c9be247ce0ce346d91d9
    // @lfy def/model/data.lfy:Scope#Scope:Scope:88f6519881eecd7fc8282f1965f00635e2031400a6510eb901072f8d595e0812
    // @lfy def/model/data.lfy:Scope#Scope:Scope:d0d1d9f729ef5da8873af7dd76fe6f256b415cef6655f59c2d0b44144e696b3f
    binder.link_prelude();
    for file in 0..files {
        binder.link_uses(file); // @lfy def/model/main.lfy:bind
    }
    for file in 0..files {
        binder.type_entities(file); // @lfy def/model/main.lfy:bind
    }
    // The expand pass is the interpreter's expand: trait applications, ace statements,
    // declaration bodies, and file-level statements run there, and the resolve pass below
    // reads what it left. It runs after the declare pass and before the resolve pass, over
    // every file in bind order, and reads the trees through the model, so the model holds
    // them while it runs.
    // @lfy def/model/main.lfy:bind
    // @lfy def/interpret/main.lfy:expand
    let mut expanding = binder.model;
    expanding.sources = binder.trees.sources.clone(); // @lfy def/interpret/main.lfy:expand
    expanding = crate::interpret::expand(expanding); // @lfy def/interpret/main.lfy:expand
    expanding.sources = Vec::new(); // @lfy def/interpret/main.lfy:expand
    binder.model = expanding;
    binder.check_rule_identifiers(); // @lfy def/model/main.lfy:bind
    for file in 0..files {
        binder.resolve_file(file); // @lfy def/model/main.lfy:bind
    }
    binder.finish_definitions();
    let mut model = binder.model;
    // Own criteria first, then each trait's in application order.
    // @lfy def/model/data.lfy:Entity.acceptanceCriteria
    for (id, entity) in model.entities.iter_mut().enumerate() {
        entity
            .acceptance_criteria
            .sort_by_key(|criterion| criterion.contributor != id);
    }
    // A declaration seen with type arguments takes the declaration's definition, members,
    // criteria, and traits, which the expand pass filled in after the type pass made it.
    // @lfy def/model/main.lfy:bind
    model.finish_generics();
    // @lfy def/model/data.lfy:Model.problems
    model
        .problems
        .sort_by_key(|problem| (problem.node.file, problem.node.index));
    model.sources = match Rc::try_unwrap(binder.trees) {
        Ok(trees) => trees.sources,
        Err(shared) => shared.sources.clone(),
    };
    model
}

impl bind::Binder {
    /// Entity.definition for every entity whose declaration gave one and whose body did
    /// not run.
    // @lfy def/model/main.lfy:bind
    fn finish_definitions(&mut self) {
        for entity in 0..self.model.entities.len() {
            if self.model.entities[entity].definition.is_some() {
                continue;
            }
            let Some(node) = self.model.entities[entity].definition_node else {
                continue;
            };
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

/// The symbol a node uses or declares: its usage's symbol, or the symbol it declares, and
/// undefined when the node has no usage in the model and declares no symbol in it.
// @lfy def/model/main.lfy:resolve
// @lfy def/model/main.lfy:resolve
pub fn resolve(model: &Model, node: NodeRef) -> Option<SymbolId> {
    if let Some(usage) = model.usage_of(node) {
        return model.usages[usage].symbol; // @lfy def/model/main.lfy:resolve
    }
    model.symbol_of(node) // @lfy def/model/main.lfy:resolve
}

/// Every use of a symbol: every usage whose symbol is the target or an alias chain
/// ending at it, in node order.
// @lfy def/model/main.lfy:usagesOf
// @lfy def/model/main.lfy:usagesOf
pub fn usages_of(model: &Model, target: SymbolId) -> Vec<UsageId> {
    let entity = model.symbols[target].entity;
    let mut out: Vec<UsageId> = model
        .usages
        .iter()
        .enumerate()
        .filter(|(_, usage)| {
            usage.symbol.is_some_and(|symbol| {
                symbol == target
                    || (model.symbols[symbol].kind == SymbolKind::Alias
                        && model.symbols[symbol].entity == entity)
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
///
/// The expand pass renders a criterion's templates as it runs them, when a type is often
/// still the text it was written as; a template with an execution whose value is a type
/// is rendered again here, for the entity, so that the type is spelled as
/// [`type_text_of`] spells one. A criterion a trait contributed is rendered again for the
/// entity it was applied to, as the expand pass rendered it.
// @lfy def/model/main.lfy:criteriaOf
// @lfy def/model/main.lfy:criteriaOf
pub fn criteria_of(model: &Model, entity: EntityId) -> Vec<Criterion> {
    model.entities[entity]
        .acceptance_criteria
        .iter()
        .map(|criterion| Criterion {
            situation: resolved_texts(model, entity, criterion, "situation", &criterion.situation),
            behavior: resolved_texts(model, entity, criterion, "behavior", &criterion.behavior),
            side_effects: resolved_texts(
                model,
                entity,
                criterion,
                "sideEffects",
                &criterion.side_effects,
            ),
            contributor: criterion.contributor,
            node: criterion.node,
        })
        .collect()
}

/// The texts of one key of a criterion, resolved: what the template renders again when
/// the model can render it, and what the expand pass wrote otherwise.
// @lfy def/model/main.lfy:criteriaOf
fn resolved_texts(
    model: &Model,
    entity: EntityId,
    criterion: &Criterion,
    key: &str,
    texts: &Option<Vec<String>>,
) -> Option<Vec<String>> {
    let texts = texts.as_ref()?;
    let templates = criterion_templates(model, criterion, key);
    let paired = templates.len() == texts.len();
    Some(
        texts
            .iter()
            .enumerate()
            .map(|(at, text)| {
                let again = paired
                    .then(|| templates[at])
                    .and_then(|template| respelled(model, entity, criterion, template, text));
                strip_references(again.as_deref().unwrap_or(text))
            })
            .collect(),
    )
}

/// The template nodes the texts of one key of a criterion were rendered from, in order:
/// the value that key was given in the object passed to `add`, the expression of a
/// `Where` for its behavior, and for its situation the expression of each condition of a
/// `Where` whose conditions are joined by `and` alone, none of them negated or grouped, so
/// that each is one situation.
// @lfy def/model/main.lfy:criteriaOf
fn criterion_templates(model: &Model, criterion: &Criterion, key: &str) -> Vec<NodeRef> {
    let Some(node) = criterion.node else {
        return Vec::new();
    };
    if has_rule(model, node, E::Object) {
        let Some(value) = child_nodes(model, node)
            .into_iter()
            .filter(|&child| has_rule(model, child, E::ObjectKey))
            .find(|&child| declared_key(model, child).as_deref() == Some(key))
            .and_then(|child| {
                child_nodes(model, child)
                    .into_iter()
                    .find(|&part| !has_rule(model, part, E::Declared))
            })
        else {
            return Vec::new();
        };
        if has_rule(model, value, E::List) {
            return child_nodes(model, value)
                .into_iter()
                .find(|&items| has_rule(model, items, E::Items))
                .map(|items| child_nodes(model, items))
                .unwrap_or_default();
        }
        return vec![value];
    }
    if has_rule(model, node, S::Where) && key == "behavior" {
        return child_nodes(model, node)
            .into_iter()
            .find(|&child| has_rule(model, child, S::ExpressionStatement))
            .map(|statement| child_nodes(model, statement))
            .unwrap_or_default();
    }
    if has_rule(model, node, S::Where) && key == "situation" {
        return where_situations(model, node).unwrap_or_default();
    }
    Vec::new()
}

/// The expression of each condition of a `Where`, in order, when each condition is one
/// situation of its own: nothing when one is negated or grouped, or two are joined by `or`.
// @lfy def/model/main.lfy:criteriaOf
fn where_situations(model: &Model, node: NodeRef) -> Option<Vec<NodeRef>> {
    let conditions = child_nodes(model, node)
        .into_iter()
        .find(|&child| has_rule(model, child, S::Conditions))?;
    let tokens = model.tokens(conditions.file);
    let mut out = Vec::new();
    for part in parts_of(model, conditions) {
        match part {
            Part::Node(condition) => {
                if model
                    .node(condition)
                    .token(P::LogicalNot, model.tokens(condition.file))
                    .is_some()
                {
                    return None;
                }
                let inner = child_nodes(model, condition).into_iter().next()?;
                if has_rule(model, inner, E::Group) {
                    out.push(child_nodes(model, inner).into_iter().next()?);
                } else if has_rule(model, inner, S::ConditionGroup) {
                    return None;
                } else {
                    out.push(inner);
                }
            }
            Part::Token(index) if tokens[index].is(K::OrKeyword) => return None,
            Part::Token(_) => {}
        }
    }
    Some(out)
}

/// What the names a body binds hold where a template was rendered, by name, innermost
/// last: a trait's parameters and a loop's variables.
type Bindings = Vec<(String, Value)>;

/// The most ways of binding a template's names that are tried for one text.
const MOST_BINDINGS: usize = 4096;

/// A template rendered again from the bound model for an entity, with every execution
/// whose value is a type spelled by [`type_text_of`]: bodies as written, references as
/// the expand pass renders them, and every execution as its value, evaluated for the
/// entity with each name a body binds holding what it held there.
///
/// The names a body binds are bound again from what the model kept: a trait's parameters
/// from the arguments it was applied with, a body's variables from their declarations,
/// and a loop's variables from each item of what it visits. Of the ways of binding them, the one
/// taken is the one whose rendering, spelled as the expand pass spells a value, is the
/// text the expand pass wrote, so the values are the ones the template was rendered with.
/// Nothing when no way of binding them renders that text.
// Decision: the expand pass writes a criterion's texts with its own spelling of a type, and
// keeps no value it rendered; a text no binding renders again keeps that spelling, since
// the type it spelled is no longer known.
// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:10ab31f446369520ef4402e3afa3adf6c05b2834fdd52636b18fba986f4fbb8c
// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:42d4fffaea6804d033186fda4ffd02f65ef21b5f6d14c5db018da909d530fcc7
// @lfy def/model/main.lfy:criteriaOf
fn respelled(
    model: &Model,
    entity: EntityId,
    criterion: &Criterion,
    template: NodeRef,
    written: &str,
) -> Option<String> {
    if !has_rule(model, template, E::Template) {
        return None;
    }
    let ways = if binds_names(model, template) {
        bindings_for(model, entity, criterion, template)
    } else {
        vec![Vec::new()]
    };
    ways.iter().find_map(|bindings| {
        let (plain, typed) = rendered(model, entity, template, bindings)?;
        (plain == written).then_some(typed)
    })
}

/// A template's text, once spelled as the expand pass spells a value and once with every
/// type spelled by [`type_text_of`]. Nothing when an execution has a value only the
/// interpreter knows.
// @lfy def/model/main.lfy:criteriaOf
fn rendered(
    model: &Model,
    entity: EntityId,
    template: NodeRef,
    bindings: &Bindings,
) -> Option<(String, String)> {
    let tokens = model.tokens(template.file);
    let mut plain = String::new();
    let mut typed = String::new();
    for child in &model.node(template).children {
        match child {
            Child::Token(index) => {
                if tokens[*index].is(L::TemplateBody) {
                    plain.push_str(&tokens[*index].value);
                    typed.push_str(&tokens[*index].value);
                }
            }
            Child::Node(node) => {
                let Some(child) = model.node_ref(template.file, node) else {
                    continue;
                };
                if has_rule(model, child, E::TemplateReference) {
                    let text = match child_nodes(model, child).into_iter().next() {
                        Some(reference) => reference_text(model, entity, bindings, reference),
                        None => String::new(),
                    };
                    let text = format!("[[{text}]]");
                    plain.push_str(&text);
                    typed.push_str(&text);
                } else if has_rule(model, child, E::TemplateExecution) {
                    let value = match child_nodes(model, child).into_iter().next() {
                        Some(expression) => {
                            execution_value(model, entity, bindings, expression, 0)?
                        }
                        None => Value::Undefined,
                    };
                    if value == Value::Undefined {
                        let raw = model.raw(child);
                        plain.push_str(&raw);
                        typed.push_str(&raw);
                    } else {
                        plain.push_str(&value_text(model, &value));
                        typed.push_str(&typed_text(model, &value));
                    }
                }
            }
            Child::Error(_) => {}
        }
    }
    Some((plain, typed))
}

/// The text of a value with every type in it spelled by [`type_text_of`].
// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:42d4fffaea6804d033186fda4ffd02f65ef21b5f6d14c5db018da909d530fcc7
// @lfy def/model/main.lfy:criteriaOf
fn typed_text(model: &Model, value: &Value) -> String {
    match value {
        Value::Type(ty) => type_text_of(model, Some(ty)).unwrap_or_default(),
        Value::List(items) => items
            .iter()
            .map(|item| typed_text(model, item))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(pairs) => pairs
            .iter()
            .map(|(key, value)| format!("{key} = {}", typed_text(model, value)))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Like(inner) => typed_text(model, inner),
        other => value_text(model, other),
    }
}

/// Whether a template reads a name only a body binds: a parameter, a variable, or a loop
/// variable.
// @lfy def/model/main.lfy:criteriaOf
fn binds_names(model: &Model, node: NodeRef) -> bool {
    if has_rule(model, node, E::Name)
        && resolve(model, node).is_some_and(|symbol| {
            matches!(
                model.symbols[symbol].kind,
                SymbolKind::Parameter | SymbolKind::LoopVariable | SymbolKind::Variable
            )
        })
    {
        return true;
    }
    child_nodes(model, node)
        .into_iter()
        .any(|child| binds_names(model, child))
}

/// Every way the names a body binds may have been bound where a template was rendered:
/// the parameters of the trait that contributed the criterion, once for each application
/// of it, then the variables of the loops around the template, outermost first.
// @lfy def/model/main.lfy:criteriaOf
fn bindings_for(
    model: &Model,
    entity: EntityId,
    criterion: &Criterion,
    template: NodeRef,
) -> Vec<Bindings> {
    let mut ways: Vec<Bindings> = vec![Vec::new()];
    let contributor = criterion.contributor;
    if contributor != entity && matches!(model.entities[contributor].kind, EntityKind::Trait { .. })
    {
        let names: Vec<String> = model.entities[contributor]
            .parameters()
            .iter()
            .map(|&symbol| model.symbols[symbol].name.clone())
            .collect();
        if !names.is_empty() {
            ways = applications(model, entity, contributor)
                .into_iter()
                .map(|values| names.iter().cloned().zip(values).collect())
                .collect();
        }
    }
    let stop = model.entities[contributor].node;
    for r in loops_around(model, template, stop) {
        ways = ways
            .into_iter()
            .flat_map(|bindings| {
                loop_rounds(model, entity, &bindings, r)
                    .into_iter()
                    .map(|round| {
                        let mut inner = bindings.clone();
                        inner.extend(round);
                        inner
                    })
                    .collect::<Vec<_>>()
            })
            .take(MOST_BINDINGS)
            .collect();
    }
    ways
}

/// The arguments a trait was applied with: to the entity itself, and when it was applied
/// to it by no application of its own, to whatever it was applied to.
// @lfy def/model/main.lfy:criteriaOf
fn applications(model: &Model, entity: EntityId, contributor: EntityId) -> Vec<Vec<Value>> {
    let of = |holder: &Entity| -> Vec<Vec<Value>> {
        holder
            .traits
            .iter()
            .filter(|applied| applied.entity == contributor)
            .map(|applied| applied.values.clone())
            .collect()
    };
    let own = of(&model.entities[entity]);
    let all = if own.is_empty() {
        model.entities.iter().flat_map(of).collect()
    } else {
        own
    };
    let mut distinct: Vec<Vec<Value>> = Vec::new();
    for values in all {
        if !distinct.contains(&values) {
            distinct.push(values);
        }
    }
    distinct
}

/// The loops whose blocks hold a template, outermost first, up to the declaration of the
/// entity whose body holds it. A variable a body declares is read from its declaration,
/// as the expand pass reads it, so it binds nothing here.
// @lfy def/model/main.lfy:criteriaOf
fn loops_around(model: &Model, template: NodeRef, stop: Option<NodeRef>) -> Vec<NodeRef> {
    let mut loops = Vec::new();
    let mut at = template;
    while let Some(parent) = model.parent(at) {
        if Some(parent) == stop {
            break;
        }
        if has_rule(model, parent, S::For) && has_rule(model, at, S::Block) {
            loops.push(parent);
        }
        at = parent;
    }
    loops.reverse();
    loops
}

/// What a loop's names hold in each of its rounds: the items of what it visits (`in`),
/// their keys (`of`), or both (`from`).
// @lfy def/model/main.lfy:criteriaOf
fn loop_rounds(model: &Model, entity: EntityId, bindings: &Bindings, r: NodeRef) -> Vec<Bindings> {
    let children = child_nodes(model, r);
    let Some(name) = children
        .iter()
        .copied()
        .find(|&c| has_rule(model, c, E::Declared))
        .and_then(|declared| identifier_of(model, declared))
    else {
        return Vec::new();
    };
    let in_of = children.iter().copied().find(|&c| has_rule(model, c, S::ForInOf));
    let from = children.iter().copied().find(|&c| has_rule(model, c, S::ForFrom));
    let mut rounds = Vec::new();
    if let Some(in_of) = in_of {
        let keys = model
            .node(in_of)
            .token(K::OfKeyword, model.tokens(in_of.file))
            .is_some();
        for iterable in child_nodes(model, in_of) {
            let Some(value) = execution_value(model, entity, bindings, iterable, 0) else {
                return Vec::new();
            };
            rounds.extend(
                visited(model, &value, keys)
                    .into_iter()
                    .map(|item| vec![(name.clone(), item)]),
            );
        }
    } else if let Some(from) = from {
        let parts = child_nodes(model, from);
        let Some(second) = parts
            .iter()
            .copied()
            .find(|&c| has_rule(model, c, E::Declared))
            .and_then(|declared| identifier_of(model, declared))
        else {
            return Vec::new();
        };
        for iterable in parts.into_iter().filter(|&c| !has_rule(model, c, E::Declared)) {
            let Some(value) = execution_value(model, entity, bindings, iterable, 0) else {
                return Vec::new();
            };
            let keys = visited(model, &value, true);
            let values = visited(model, &value, false);
            rounds.extend(
                keys.into_iter()
                    .zip(values)
                    .map(|(key, item)| vec![(name.clone(), key), (second.clone(), item)]),
            );
        }
    }
    rounds
}

/// What a value yields in a loop: its items, or their keys.
// @lfy def/model/main.lfy:criteriaOf
fn visited(model: &Model, value: &Value, keys: bool) -> Vec<Value> {
    match value {
        Value::List(items) if keys => (0..items.len()).map(|at| Value::Number(at as f64)).collect(),
        Value::List(items) => items.clone(),
        Value::Object(pairs) => pairs
            .iter()
            .map(|(key, value)| {
                if keys {
                    Value::String(key.clone())
                } else {
                    value.clone()
                }
            })
            .collect(),
        Value::Entity(entity) => match model.entities[*entity].scope {
            Some(scope) => model.scopes[scope]
                .symbols
                .iter()
                .map(|&symbol| {
                    if keys {
                        Value::String(model.symbols[symbol].name.clone())
                    } else {
                        Value::Entity(model.symbols[symbol].entity)
                    }
                })
                .collect(),
            None => Vec::new(),
        },
        Value::String(text) => text
            .chars()
            .map(|character| Value::String(character.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// The deepest a value is followed through the declarations it reads.
const DEEPEST: usize = 32;

/// What an expression evaluates to, for the entity a criterion is rendered for and with
/// the names a body binds holding what they held: a bound name, a declaration, a
/// constant's value, a list, the current entity, and what an accessor reads from those.
/// Nothing for anything else, whose value only the interpreter knows.
// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:10ab31f446369520ef4402e3afa3adf6c05b2834fdd52636b18fba986f4fbb8c
// @lfy def/model/main.lfy:criteriaOf
fn execution_value(
    model: &Model,
    entity: EntityId,
    bindings: &Bindings,
    expression: NodeRef,
    depth: usize,
) -> Option<Value> {
    if depth > DEEPEST {
        return None;
    }
    if has_rule(model, expression, E::Reference) || has_rule(model, expression, E::Group) {
        let inner = child_nodes(model, expression).into_iter().next()?;
        return execution_value(model, entity, bindings, inner, depth);
    }
    if has_rule(model, expression, E::Name) {
        let name = identifier_of(model, expression)?;
        if let Some((_, value)) = bindings.iter().rev().find(|(bound, _)| *bound == name) {
            return Some(value.clone());
        }
        let symbol = resolve(model, expression)?;
        let held = model.symbols[symbol].entity;
        return match model.symbols[symbol].kind {
            SymbolKind::Parameter
            | SymbolKind::LoopVariable
            | SymbolKind::Member
            | SymbolKind::TypeParameter => None,
            SymbolKind::Variable => constant_value(model, held, depth),
            _ => Some(Value::Entity(held)),
        };
    }
    if has_rule(model, expression, E::List) {
        let items = child_nodes(model, expression)
            .into_iter()
            .find(|&items| has_rule(model, items, E::Items))
            .map(|items| child_nodes(model, items))
            .unwrap_or_default();
        return items
            .into_iter()
            .map(|item| execution_value(model, entity, bindings, item, depth + 1))
            .collect::<Option<Vec<_>>>()
            .map(Value::List);
    }
    let tokens = model.tokens(expression.file);
    if has_rule(model, expression, E::Current) {
        let mut parts = parts_of(model, expression).into_iter();
        let Some(Part::Token(accessor)) = parts.next() else {
            return Some(Value::Entity(entity));
        };
        let name = match parts.next() {
            Some(Part::Token(index)) if is_member_name(model, expression.file, index) => {
                Some(tokens[index].value.as_str())
            }
            _ => None,
        };
        return access(model, Value::Entity(entity), tokens[accessor].rule?, name);
    }
    if has_rule(model, expression, E::Member) {
        let (left, accessor, name) = member_parts(model, expression)?;
        let rule = tokens[accessor].rule?;
        // The context layer of a name bound to a variable is the variable's own.
        let declared = (accessor_layer(rule)? == Layer::Context && has_rule(model, left, E::Name))
            .then(|| resolve(model, left))
            .flatten()
            .filter(|&symbol| model.symbols[symbol].kind == SymbolKind::Variable)
            .map(|symbol| Value::Entity(model.symbols[symbol].entity));
        let subject = match declared {
            Some(subject) => subject,
            None => execution_value(model, entity, bindings, left, depth + 1)?,
        };
        return access(model, subject, rule, name.map(|name| tokens[name].value.as_str()));
    }
    None
}

/// The value a `const` holds, from its declaration; nothing for a `let`.
// @lfy def/model/main.lfy:criteriaOf
fn constant_value(model: &Model, variable: EntityId, depth: usize) -> Option<Value> {
    let Some(node) = model.entities[variable].node else {
        return Some(Value::Entity(variable));
    };
    if model
        .node(node)
        .token(K::LetKeyword, model.tokens(node.file))
        .is_some()
    {
        return None;
    }
    let expression = child_nodes(model, node)
        .into_iter()
        .find(|&child| !has_rule(model, child, E::Declared))?;
    let current = model.scopes[model.enclosing_scope(node)].current;
    execution_value(model, current, &Vec::new(), expression, depth + 1)
}

/// What one accessor reads from a value.
// @lfy def/model/main.lfy:criteriaOf
fn access(
    model: &Model,
    value: Value,
    accessor: crate::grammar::Entity,
    name: Option<&str>,
) -> Option<Value> {
    match accessor_layer(accessor)? {
        Layer::Context => {
            let Some(name) = name else {
                return Some(value);
            };
            context_value(model, &value, name)
        }
        Layer::Scope => {
            let Value::Entity(owner) = value else {
                return None;
            };
            match name {
                None => Some(
                    model.entities[owner]
                        .scope
                        .map_or(Value::Undefined, Value::Scope),
                ),
                Some(name) => member_value(model, owner, name),
            }
        }
        Layer::Parent => {
            let Value::Entity(owner) = value else {
                return None;
            };
            Some(
                model.entities[owner]
                    .scope
                    .and_then(|scope| model.scopes[scope].parent)
                    .map_or(Value::Undefined, Value::Scope),
            )
        }
        Layer::Value => {
            let Some(name) = name else {
                return Some(value);
            };
            match value {
                Value::Entity(owner) => member_value(model, owner, name),
                Value::Object(pairs) => Some(
                    pairs
                        .into_iter()
                        .find(|(held, _)| held == name)
                        .map_or(Value::Undefined, |(_, value)| value),
                ),
                Value::List(items) if name == "length" => Some(Value::Number(items.len() as f64)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// What the context layer of a value holds under a name.
// @lfy def/model/main.lfy:criteriaOf
fn context_value(model: &Model, value: &Value, name: &str) -> Option<Value> {
    let typed = |ty: Option<TypeRef>| Some(ty.map_or(Value::Undefined, |ty| Value::Type(Box::new(ty))));
    let listed = |items: &[EntityId]| Some(Value::List(items.iter().map(|&e| Value::Entity(e)).collect()));
    let primitive = |name: &'static str| Some(Value::Type(Box::new(TypeRef::Primitive(name))));
    match value {
        Value::Entity(subject) => {
            let e = &model.entities[*subject];
            match name {
                "identifier" => Some(e.identifier.clone().map_or(Value::Undefined, Value::String)),
                "type" => typed(e.ty.clone()),
                "output" => typed(e.output().cloned()),
                "itemType" => typed(e.item_type.clone()),
                "parameters" => Some(Value::List(
                    e.parameters()
                        .iter()
                        .map(|&symbol| Value::Entity(model.symbols[symbol].entity))
                        .collect(),
                )),
                "typeParameters" => listed(&model.type_parameters(*subject)),
                "typeArguments" => Some(Value::List(
                    model
                        .type_arguments(*subject)
                        .into_iter()
                        .map(|ty| Value::Type(Box::new(ty)))
                        .collect(),
                )),
                "entities" => listed(e.entities()),
                "extenders" => listed(e.extenders()),
                _ => None,
            }
        }
        Value::Type(held) => match name {
            "type" => Some(value.clone()),
            "itemType" => match held.as_ref() {
                TypeRef::List(item) => Some(Value::Type(item.clone())),
                _ => Some(Value::Undefined),
            },
            _ => None,
        },
        Value::String(_) if name == "type" => primitive("string"),
        Value::Number(_) if name == "type" => primitive("number"),
        Value::Bool(_) if name == "type" => primitive("boolean"),
        _ => None,
    }
}

/// The value of a member of an entity: what a setter gave it, or the type it was declared
/// with; a name its scope declares otherwise is that declaration.
// @lfy def/model/main.lfy:criteriaOf
fn member_value(model: &Model, owner: EntityId, name: &str) -> Option<Value> {
    if let Some(value) = model.entities[owner].value(name) {
        return Some(value.clone());
    }
    let scope = model.entities[owner].scope?;
    let symbol = model.lookup_local(scope, name)?;
    let held = model.symbols[symbol].entity;
    match model.symbols[symbol].kind {
        SymbolKind::Member => model.entities[held]
            .ty
            .clone()
            .map(|ty| Value::Type(Box::new(ty))),
        SymbolKind::Parameter | SymbolKind::LoopVariable | SymbolKind::TypeParameter => None,
        SymbolKind::Variable => constant_value(model, held, 0),
        _ => Some(Value::Entity(held)),
    }
}

/// How a reference inside a template renders, as the expand pass renders it: a declared
/// name as written, a name bound to an entity as that entity's identifier, the current
/// entity as the identifier of the entity the criterion is rendered for, and a member
/// chain joined by its accessors.
// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:10ab31f446369520ef4402e3afa3adf6c05b2834fdd52636b18fba986f4fbb8c
// @lfy def/model/main.lfy:criteriaOf
fn reference_text(
    model: &Model,
    entity: EntityId,
    bindings: &Bindings,
    reference: NodeRef,
) -> String {
    if has_rule(model, reference, E::Reference) || has_rule(model, reference, E::Dereference) {
        return match child_nodes(model, reference).into_iter().next() {
            Some(inner) => reference_text(model, entity, bindings, inner),
            None => String::new(),
        };
    }
    let tokens = model.tokens(reference.file);
    if has_rule(model, reference, E::Name) {
        let name = identifier_of(model, reference).unwrap_or_default();
        return match bindings.iter().rev().find(|(bound, _)| *bound == name) {
            Some((_, Value::Entity(held))) => {
                model.entities[*held].identifier.clone().unwrap_or(name)
            }
            _ => name,
        };
    }
    if has_rule(model, reference, E::Current) {
        let head = model.entities[entity].identifier.clone().unwrap_or_default();
        let mut parts = parts_of(model, reference).into_iter();
        let Some(Part::Token(accessor)) = parts.next() else {
            return model.raw(reference);
        };
        return match parts.next() {
            Some(Part::Token(name)) if is_member_name(model, reference.file, name) => {
                format!("{head}{}{}", tokens[accessor].raw, tokens[name].value)
            }
            _ => head,
        };
    }
    if has_rule(model, reference, E::Member) {
        let Some((left, accessor, name)) = member_parts(model, reference) else {
            return model.raw(reference);
        };
        let head = reference_text(model, entity, bindings, left);
        let raw = &tokens[accessor].raw;
        return match name {
            Some(name) => format!("{head}{raw}{}", tokens[name].value),
            None => format!("{head}{raw}"),
        };
    }
    model.raw(reference).trim().to_string()
}

/// The left operand, the accessor token, and the name token of a `Member`.
// @lfy def/model/main.lfy:criteriaOf
fn member_parts(model: &Model, r: NodeRef) -> Option<(NodeRef, usize, Option<usize>)> {
    let mut parts = parts_of(model, r).into_iter();
    let Part::Node(left) = parts.next()? else {
        return None;
    };
    let Part::Token(accessor) = parts.next()? else {
        return None;
    };
    accessor_layer(model.tokens(r.file)[accessor].rule?)?;
    let name = match parts.next() {
        Some(Part::Token(index)) if is_member_name(model, r.file, index) => Some(index),
        _ => None,
    };
    Some((left, accessor, name))
}

/// The significant children of a node, nodes and tokens alike, in order.
// @lfy def/model/main.lfy:criteriaOf
fn parts_of(model: &Model, r: NodeRef) -> Vec<Part> {
    let tokens = model.tokens(r.file);
    model
        .node(r)
        .children
        .iter()
        .filter_map(|child| match child {
            Child::Node(node) if !is_trivia(node.rule) => {
                model.node_ref(r.file, node).map(Part::Node)
            }
            Child::Node(_) | Child::Error(_) => None,
            Child::Token(index) => {
                let rule = tokens[*index].rule?;
                (!is_trivia(rule)).then_some(Part::Token(*index))
            }
        })
        .collect()
}

/// Every child node of a node that is not trivia, in order.
// @lfy def/model/main.lfy:criteriaOf
fn child_nodes(model: &Model, r: NodeRef) -> Vec<NodeRef> {
    model
        .node(r)
        .children
        .iter()
        .filter_map(|child| match child {
            Child::Node(node) if !is_trivia(node.rule) => model.node_ref(r.file, node),
            _ => None,
        })
        .collect()
}

/// Whether a node satisfies a rule.
// @lfy def/model/main.lfy:criteriaOf
fn has_rule<R: GrammarRule>(model: &Model, r: NodeRef, rule: R) -> bool {
    model.info(r).rule == rule.entity()
}

/// Whether a token can be a member name: an identifier or a keyword.
// @lfy def/model/main.lfy:criteriaOf
fn is_member_name(model: &Model, file: FileId, index: usize) -> bool {
    model.tokens(file)[index]
        .rule
        .is_some_and(|rule| rule == I::Identifier.entity() || rule.is_keyword())
}

/// The first identifier token directly inside a node.
// @lfy def/model/main.lfy:criteriaOf
fn identifier_of(model: &Model, r: NodeRef) -> Option<String> {
    let tokens = model.tokens(r.file);
    model
        .node(r)
        .children
        .iter()
        .find_map(|child| match child {
            Child::Token(index) if tokens[*index].is(I::Identifier) => {
                Some(tokens[*index].value.clone())
            }
            _ => None,
        })
}

/// The name an `ObjectKey` declares.
// @lfy def/model/main.lfy:criteriaOf
fn declared_key(model: &Model, object_key: NodeRef) -> Option<String> {
    let declared = child_nodes(model, object_key)
        .into_iter()
        .find(|&child| has_rule(model, child, E::Declared))?;
    identifier_of(model, declared)
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
            if n.fract() == 0.0 {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.clone(),
        Value::List(items) => items
            .iter()
            .map(|i| value_text(model, i))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(pairs) => pairs
            .iter()
            .map(|(k, v)| format!("{k} = {}", value_text(model, v)))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Entity(e) => model.entities[*e]
            .identifier
            .clone()
            .unwrap_or_else(|| "anonymous".to_string()),
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
        TypeRef::Entity(e) => model.entities[*e]
            .identifier
            .clone()
            .unwrap_or_else(|| "anonymous".to_string()),
        TypeRef::Primitive(p) => (*p).to_string(),
        TypeRef::Literal(v) => value_text(model, v),
        TypeRef::List(item) => format!("{}[]", type_text(model, item)),
        TypeRef::Union(items) => items
            .iter()
            .map(|i| type_text(model, i))
            .collect::<Vec<_>>()
            .join(" | "),
        TypeRef::Predicate(t) => format!(
            "is {}",
            model.entities[*t].identifier.clone().unwrap_or_default()
        ),
        TypeRef::Function => "function".to_string(),
        TypeRef::Unknown(text) => text.trim().to_string(),
    }
}
