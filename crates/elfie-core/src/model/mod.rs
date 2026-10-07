//! Compiled from `def/model`: the bound program and its queries.

mod bind;
mod components;
pub mod data;
mod eval;
#[cfg(test)]
mod tests;
mod trees;

use std::rc::Rc;

use crate::grammar::GrammarRule;

pub use bind::SYNTHETIC;
pub use components::{accessor_layer, declaring, is_scoped, reading_layer};
pub use data::*;

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
// @lfy def/model/main.lfy:criteriaOf
// @lfy def/model/main.lfy:criteriaOf
pub fn criteria_of(model: &Model, entity: EntityId) -> Vec<Criterion> {
    model.entities[entity]
        .acceptance_criteria
        .iter()
        .map(|criterion| Criterion {
            situation: criterion
                .situation
                .as_ref()
                .map(|s| s.iter().map(|t| strip_references(t)).collect()),
            behavior: criterion
                .behavior
                .as_ref()
                .map(|s| s.iter().map(|t| strip_references(t)).collect()),
            side_effects: criterion
                .side_effects
                .as_ref()
                .map(|s| s.iter().map(|t| strip_references(t)).collect()),
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

/// How a type is spelled as Elfie source, wherever one is shown, rendered, or hashed.
///
/// A declaration, or a primitive, is spelled by its identifier; a type parameter by its
/// name; a type that declares no name by the source text of its type expression. A
/// declaration seen with type arguments is its identifier, then its arguments spelled the
/// same way between angle brackets; a list is the item spelled the same way then square
/// brackets, so the sugar wins in display; the entity of a `FunctionType` is its
/// parameters between parentheses, then a double arrow, then its output.
// Decision: the model spells a type as a `TypeRef`, so `Entity.type` is handed over as
// one; `TypeRef::Entity` is the entity the definition names, and every other case is a
// type that declares no entity of its own.
// @lfy def/model/data.lfy:typeTextOf
pub fn type_text_of(model: &Model, ty: Option<&TypeRef>) -> Option<String> {
    let ty = ty?; // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:2b94113644ba3735487d3a97b0b21f8e6e1121c594ee59bd9e071265a4e452e8
    Some(spell(model, ty))
}

/// [`type_text_of`] for a type the model holds.
// @lfy def/model/data.lfy:typeTextOf
fn spell(model: &Model, ty: &TypeRef) -> String {
    match ty {
        // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:f16148699adfd180079861fb3fc326f62a6054b22de7cba1572f954cd5c6bc6d
        TypeRef::Entity(entity) => spell_entity(model, *entity),
        // The sugar wins in display, and `List<T>` is already read as a list; a function
        // type, a union, or an intersection is parenthesized before the brackets.
        // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:a41acb96ef1f511b638ed7a20104a4743c98bb3b644142509f866c74e69da009
        // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:61414516aa9dcc43d54bb2290614a167fef3bd1189114bcc921d597f01a1d949
        TypeRef::List(item) => {
            let text = spell(model, item);
            if is_parenthesized(model, item) {
                format!("({text})[]")
            } else {
                format!("{text}[]")
            }
        }
        // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:89c6d688f07b4e56903193fa729c9e26ec93a1a0a789a89665d68a4d40f09c6e
        TypeRef::Union(items) => items
            .iter()
            .map(|item| spell(model, item))
            .collect::<Vec<_>>()
            .join(" | "),
        // A primitive, a predicate, a literal, and a type the model could not read
        // further are spelled as the model spells them.
        ty => type_text(model, ty),
    }
}

/// Whether a type is wrapped in parentheses before the square brackets of a list: a
/// function type or a union.
// @lfy def/model/data.lfy:typeTextOf
fn is_parenthesized(model: &Model, ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Union(_) | TypeRef::Function => true,
        TypeRef::Entity(entity) => is_function_type(model, *entity),
        _ => false,
    }
}

/// Whether an entity is the entity of a `FunctionType`: a function that declares no name.
// @lfy def/model/data.lfy:typeTextOf
fn is_function_type(model: &Model, entity: EntityId) -> bool {
    let e = &model.entities[entity];
    e.identifier.is_none() && matches!(e.kind, EntityKind::Fn { .. })
}

/// Whether a node reference points into a file: `global` and its scope are synthetic.
fn is_real(model: &Model, node: NodeRef) -> bool {
    node.file < model.sources.len()
}

/// How an entity in a type position is spelled.
// @lfy def/model/data.lfy:typeTextOf
fn spell_entity(model: &Model, entity: EntityId) -> String {
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:4a96c90b5e69bd1315e4403a97ce4b57f62102d1d36e4ef0780a8337d654743d
    if is_function_type(model, entity) {
        return spell_function_type(model, entity);
    }
    let e = &model.entities[entity];
    let arguments = model.type_arguments(entity);
    match &e.identifier {
        // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:b29f2ab17865c51c9fee248c87c30d45d61d4ccb9b48e63fe45223606d734cd1
        // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:f63f8edb6dfbb860e598b51c9229ac63885f8b1c500cc4f7813ca3ff681d44bf
        Some(identifier) if !arguments.is_empty() => format!(
            "{identifier}<{}>",
            arguments
                .iter()
                .map(|argument| spell(model, argument))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // A declaration, a primitive the prelude declares, and a type parameter are all
        // spelled by the name they were declared with.
        // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:edee74f43a9abff4425d49ec621a69d8d9f2187536ca9718daa7b4f53181372a
        // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:5af54fe6fcd6b47238c1df5415f62eca2d8b4c8d406156fec26f1f13110aa1a1
        Some(identifier) => identifier.clone(),
        // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:b64f3996bad7896ba9710253adc19dbf22a7e51e3fc2c012b67aaac677979e0a
        None => match e.node.filter(|&node| is_real(model, node)) {
            Some(node) => model.raw(node).trim().to_string(),
            None => type_text(model, &TypeRef::Entity(entity)),
        },
    }
}

/// The entity of a `FunctionType`, spelled as it is written.
// @lfy def/model/data.lfy:typeTextOf
fn spell_function_type(model: &Model, entity: EntityId) -> String {
    let e = &model.entities[entity];
    let parameters = e
        .parameters()
        .iter()
        .map(|&symbol| {
            let s = &model.symbols[symbol];
            let p = &model.entities[s.entity];
            let node = p.node.filter(|&node| is_real(model, node));
            // Decision: the model fills `optional` and `spread` in for a type parameter
            // only, so for a value parameter the node the grammar wrote says it: a
            // `SpreadParameter` takes the rest, and a `QuestionMark` may be left out.
            // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:71cbc82af844db09e5de8e281ec7fcf92ecd3fb7c81640e38d65ec84d146dd3c
            let spread = matches!(p.value(bind::SPREAD), Some(Value::Bool(true)))
                || node.is_some_and(|node| {
                    model.info(node).rule == crate::grammar::rules::expression::Expression::SpreadParameter.entity()
                });
            // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:ead93c122f4d87f56344f0d4d43c1dba5eb4890a6ec0616bccca1833d30ee448
            let optional = matches!(p.value(bind::OPTIONAL), Some(Value::Bool(true)))
                || node.is_some_and(|node| {
                    model
                        .node(node)
                        .token(
                            crate::grammar::terminals::punctuation::Punctuation::QuestionMark,
                            model.tokens(node.file),
                        )
                        .is_some()
                });
            let dots = if spread { "..." } else { "" };
            let question = if optional { "?" } else { "" };
            // Decision: a parameter written without a type is spelled by its name alone,
            // since there is no type to put after the colon.
            // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:b6bd675e8cdc1c15ba551fc96016b949e34f090af28569c0ca06c74db3d2bf91
            match &p.ty {
                Some(ty) => format!("{dots}{}{question}: {}", s.name, spell(model, ty)),
                None => format!("{dots}{}{question}", s.name),
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    // Decision: a `FunctionType` always writes the type after its arrow, so the output is
    // there to spell.
    let output = e.output().map(|ty| spell(model, ty)).unwrap_or_default();
    format!("({parameters}) => {output}")
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

#[cfg(test)]
mod type_text_tests {
    use super::*;

    /// `Model@like(`Bound from: <text>`)`: one file bound alone.
    fn bound(text: &str) -> Model {
        let tokens = crate::lexer::lex(text, Some("a.lfy")).expect("the text lexes");
        let tree = crate::parser::parse(tokens, None);
        assert!(tree.errors.is_empty(), "{}", tree.render());
        bind(vec![Source {
            path: "a.lfy".to_string(),
            tree,
            uses: Vec::new(),
            origin: Origin::Program,
        }])
    }

    /// `Entity@like(`The type of <name>`)`, spelled: the one entity the name declares.
    fn spelled(model: &Model, name: &str) -> Option<String> {
        let found: Vec<&Entity> = model
            .entities
            .iter()
            .filter(|entity| entity.identifier.as_deref() == Some(name))
            .collect();
        assert_eq!(found.len(), 1, "{name} names one entity");
        type_text_of(model, found[0].ty.as_ref())
    }

    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:556e1568a7db6f418440066dbddc8e1bea76fb5acbd0ade0f41fef3634861ae2
    #[test]
    fn a_generic_constant_is_spelled_with_its_arguments() {
        let model = bound("d Box<T> {} const b = Box<string>;");
        assert_eq!(spelled(&model, "b").as_deref(), Some("Box<string>"));
    }

    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:8bbc8c58bf42ef089cf559dab4a2940814c858bd3633ad7e969522b99f87d208
    #[test]
    fn a_parameter_of_a_function_type_is_spelled_as_written() {
        let model = bound(
            "d Box<T> { $items: `d` = T[]; $map: `e` = (transform: (item: T) => T) => T[]; }",
        );
        assert_eq!(
            spelled(&model, "transform").as_deref(),
            Some("(item: T) => T")
        );
    }

    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:e0af14e47275fec8225ff8ee614e9ba18f817e1f0880f6f439f644df133a0ee5
    #[test]
    fn a_member_of_a_list_of_a_type_parameter_is_spelled_with_brackets() {
        let model = bound("d Box<T> { $items: `d` = T[]; }");
        assert_eq!(spelled(&model, "items").as_deref(), Some("T[]"));
    }

    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:e6b5aa146601a2d6b283da1cb7383c613d79dc0e5e10d943245a6c222c068db0
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:f63f8edb6dfbb860e598b51c9229ac63885f8b1c500cc4f7813ca3ff681d44bf
    #[test]
    fn a_member_read_on_a_generic_is_spelled_with_the_substituted_type() {
        let model = bound("d Box<T> { $item: `d` = T; } const b = Box<string>; const i = b.item;");
        assert_eq!(spelled(&model, "i").as_deref(), Some("string"));
    }

    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:2b94113644ba3735487d3a97b0b21f8e6e1121c594ee59bd9e071265a4e452e8
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:f16148699adfd180079861fb3fc326f62a6054b22de7cba1572f954cd5c6bc6d
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:edee74f43a9abff4425d49ec621a69d8d9f2187536ca9718daa7b4f53181372a
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:5af54fe6fcd6b47238c1df5415f62eca2d8b4c8d406156fec26f1f13110aa1a1
    #[test]
    fn nothing_is_spelled_by_nothing_and_a_name_by_its_identifier() {
        let model = bound("d A {}\nd Box<T> { $item: `d` = T; }\nconst v: A = A;\nconst w: string = \"x\";");
        assert_eq!(type_text_of(&model, None), None);
        assert_eq!(spelled(&model, "v").as_deref(), Some("A"));
        assert_eq!(spelled(&model, "w").as_deref(), Some("string"));
        assert_eq!(spelled(&model, "item").as_deref(), Some("T"));
    }

    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:b29f2ab17865c51c9fee248c87c30d45d61d4ccb9b48e63fe45223606d734cd1
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:89c6d688f07b4e56903193fa729c9e26ec93a1a0a789a89665d68a4d40f09c6e
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:a41acb96ef1f511b638ed7a20104a4743c98bb3b644142509f866c74e69da009
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:61414516aa9dcc43d54bb2290614a167fef3bd1189114bcc921d597f01a1d949
    #[test]
    fn a_union_a_generic_and_a_list_are_spelled_by_their_parts() {
        let model = bound(
            "d A {}\nd B {}\nd Box<T> {}\nconst g = Box<A>;\nd S { $u: `d` = A | B; $pairs: `d` = (A | B)[]; }\ntype F<U> { calls = ((item: U) => U)[] }",
        );
        assert_eq!(spelled(&model, "g").as_deref(), Some("Box<A>"));
        assert_eq!(spelled(&model, "u").as_deref(), Some("A | B"));
        assert_eq!(spelled(&model, "pairs").as_deref(), Some("(A | B)[]"));
        assert_eq!(
            spelled(&model, "calls").as_deref(),
            Some("((item: U) => U)[]")
        );
    }

    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:4a96c90b5e69bd1315e4403a97ce4b57f62102d1d36e4ef0780a8337d654743d
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:b6bd675e8cdc1c15ba551fc96016b949e34f090af28569c0ca06c74db3d2bf91
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:ead93c122f4d87f56344f0d4d43c1dba5eb4890a6ec0616bccca1833d30ee448
    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:71cbc82af844db09e5de8e281ec7fcf92ecd3fb7c81640e38d65ec84d146dd3c
    #[test]
    fn a_function_type_is_spelled_with_its_parameters_and_output() {
        let model = bound("d A {}\ntype S { f = (one: string, two?: number, ...rest: string[]) => A }");
        assert_eq!(
            spelled(&model, "f").as_deref(),
            Some("(one: string, two?: number, ...rest: string[]) => A")
        );
    }

    // @lfy def/model/data.lfy:typeTextOf#typeTextOf:typeTextOf:b64f3996bad7896ba9710253adc19dbf22a7e51e3fc2c012b67aaac677979e0a
    #[test]
    fn a_type_that_declares_no_name_is_spelled_by_its_source_text() {
        let model = bound("d A {}\ntype S { f = (one: string) => A }");
        let f = model
            .entities
            .iter()
            .find(|entity| entity.identifier.as_deref() == Some("f"))
            .and_then(|entity| entity.ty.as_ref())
            .expect("f has a type");
        let TypeRef::Entity(entity) = f else {
            panic!("a function type is an entity");
        };
        assert_eq!(model.entities[*entity].identifier, None);
        assert_eq!(
            type_text_of(&model, Some(f)).as_deref(),
            Some("(one: string) => A")
        );
    }
}
