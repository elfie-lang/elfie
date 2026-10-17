// @lfy def/model/type.lfy:typeTextOf
//! Compiled from `def/model/type.lfy`: the one spelling of a type as Elfie source.

use super::*;

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
// @lfy def/model/type.lfy:typeTextOf
pub fn type_text_of(model: &Model, ty: Option<&TypeRef>) -> Option<String> {
    let ty = ty?; // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:2b94113644ba3735487d3a97b0b21f8e6e1121c594ee59bd9e071265a4e452e8
    Some(spell(model, ty))
}

/// [`type_text_of`] for a type the model holds.
// @lfy def/model/type.lfy:typeTextOf
fn spell(model: &Model, ty: &TypeRef) -> String {
    match ty {
        // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:f16148699adfd180079861fb3fc326f62a6054b22de7cba1572f954cd5c6bc6d
        TypeRef::Entity(entity) => spell_entity(model, *entity),
        // The sugar wins in display, and `List<T>` is already read as a list; a function
        // type, a union, or an intersection is parenthesized before the brackets.
        // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:a41acb96ef1f511b638ed7a20104a4743c98bb3b644142509f866c74e69da009
        // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:61414516aa9dcc43d54bb2290614a167fef3bd1189114bcc921d597f01a1d949
        TypeRef::List(item) => {
            let text = spell(model, item);
            if is_parenthesized(model, item) {
                format!("({text})[]")
            } else {
                format!("{text}[]")
            }
        }
        // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:89c6d688f07b4e56903193fa729c9e26ec93a1a0a789a89665d68a4d40f09c6e
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
// @lfy def/model/type.lfy:typeTextOf
fn is_parenthesized(model: &Model, ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Union(_) | TypeRef::Function => true,
        TypeRef::Entity(entity) => is_function_type(model, *entity),
        _ => false,
    }
}

/// Whether an entity is the entity of a `FunctionType`: a function that declares no name.
// @lfy def/model/type.lfy:typeTextOf
fn is_function_type(model: &Model, entity: EntityId) -> bool {
    let e = &model.entities[entity];
    e.identifier.is_none() && matches!(e.kind, EntityKind::Fn { .. })
}

/// Whether a node reference points into a file: `global` and its scope are synthetic.
// @lfy def/model/type.lfy:typeTextOf
fn is_real(model: &Model, node: NodeRef) -> bool {
    node.file < model.sources.len()
}

/// How an entity in a type position is spelled.
// @lfy def/model/type.lfy:typeTextOf
fn spell_entity(model: &Model, entity: EntityId) -> String {
    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:4a96c90b5e69bd1315e4403a97ce4b57f62102d1d36e4ef0780a8337d654743d
    if is_function_type(model, entity) {
        return spell_function_type(model, entity);
    }
    let e = &model.entities[entity];
    let arguments = model.type_arguments(entity);
    match &e.identifier {
        // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:b29f2ab17865c51c9fee248c87c30d45d61d4ccb9b48e63fe45223606d734cd1
        // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:f63f8edb6dfbb860e598b51c9229ac63885f8b1c500cc4f7813ca3ff681d44bf
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
        // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:edee74f43a9abff4425d49ec621a69d8d9f2187536ca9718daa7b4f53181372a
        // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:5af54fe6fcd6b47238c1df5415f62eca2d8b4c8d406156fec26f1f13110aa1a1
        Some(identifier) => identifier.clone(),
        // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:b64f3996bad7896ba9710253adc19dbf22a7e51e3fc2c012b67aaac677979e0a
        None => match e.node.filter(|&node| is_real(model, node)) {
            Some(node) => model.raw(node).trim().to_string(),
            None => type_text(model, &TypeRef::Entity(entity)),
        },
    }
}

/// The entity of a `FunctionType`, spelled as it is written.
// @lfy def/model/type.lfy:typeTextOf
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
            // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:71cbc82af844db09e5de8e281ec7fcf92ecd3fb7c81640e38d65ec84d146dd3c
            let spread = matches!(p.value(bind::SPREAD), Some(Value::Bool(true)))
                || node.is_some_and(|node| {
                    model.info(node).rule == E::SpreadParameter.entity()
                });
            // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:ead93c122f4d87f56344f0d4d43c1dba5eb4890a6ec0616bccca1833d30ee448
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
            // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:b6bd675e8cdc1c15ba551fc96016b949e34f090af28569c0ca06c74db3d2bf91
            match &p.ty {
                Some(ty) => format!("{dots}{}{question}: {}", s.name, spell(model, ty)),
                None => format!("{dots}{}{question}", s.name),
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    // Decision: a `FunctionType` always writes the type after its arrow, so the output is
    // there to spell.
    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:4a96c90b5e69bd1315e4403a97ce4b57f62102d1d36e4ef0780a8337d654743d
    let output = e.output().map(|ty| spell(model, ty)).unwrap_or_default();
    format!("({parameters}) => {output}")
}

#[cfg(test)]
// @lfy def/model/type.lfy:typeTextOf
mod tests {
    use super::*;

    /// `Model@like(`Bound from: <text>`)`: one file bound alone.
    // @lfy def/model/type.lfy:typeTextOf
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
    // @lfy def/model/type.lfy:typeTextOf
    fn spelled(model: &Model, name: &str) -> Option<String> {
        let found: Vec<&Entity> = model
            .entities
            .iter()
            .filter(|entity| entity.identifier.as_deref() == Some(name))
            .collect();
        assert_eq!(found.len(), 1, "{name} names one entity");
        type_text_of(model, found[0].ty.as_ref())
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:556e1568a7db6f418440066dbddc8e1bea76fb5acbd0ade0f41fef3634861ae2
    #[test]
    fn a_generic_constant_is_spelled_with_its_arguments() {
        let model = bound("d Box<T> {} const b = Box<string>;");
        assert_eq!(spelled(&model, "b").as_deref(), Some("Box<string>"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:8bbc8c58bf42ef089cf559dab4a2940814c858bd3633ad7e969522b99f87d208
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

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:e0af14e47275fec8225ff8ee614e9ba18f817e1f0880f6f439f644df133a0ee5
    #[test]
    fn a_member_of_a_list_of_a_type_parameter_is_spelled_with_brackets() {
        let model = bound("d Box<T> { $items: `d` = T[]; }");
        assert_eq!(spelled(&model, "items").as_deref(), Some("T[]"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:e6b5aa146601a2d6b283da1cb7383c613d79dc0e5e10d943245a6c222c068db0
    #[test]
    fn a_member_read_on_a_generic_is_spelled_with_the_substituted_type() {
        let model = bound("d Box<T> { $item: `d` = T; } const b = Box<string>; const i = b.item;");
        assert_eq!(spelled(&model, "i").as_deref(), Some("string"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:f63f8edb6dfbb860e598b51c9229ac63885f8b1c500cc4f7813ca3ff681d44bf
    #[test]
    fn a_member_read_on_a_generic_list_is_spelled_with_the_substituted_type() {
        let model =
            bound("d Box<T> { $items: `d` = T[]; } const b = Box<string>; const l = b.items;");
        assert_eq!(spelled(&model, "l").as_deref(), Some("string[]"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:2b94113644ba3735487d3a97b0b21f8e6e1121c594ee59bd9e071265a4e452e8
    #[test]
    fn nothing_is_spelled_by_nothing() {
        let model = bound("d A {}");
        assert_eq!(type_text_of(&model, None), None);
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:edee74f43a9abff4425d49ec621a69d8d9f2187536ca9718daa7b4f53181372a
    #[test]
    fn a_declaration_or_a_primitive_is_spelled_by_its_identifier() {
        let model = bound("d A {}\nconst v: A = A;\nconst w: string = \"x\";");
        assert_eq!(spelled(&model, "v").as_deref(), Some("A"));
        assert_eq!(spelled(&model, "w").as_deref(), Some("string"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:5af54fe6fcd6b47238c1df5415f62eca2d8b4c8d406156fec26f1f13110aa1a1
    #[test]
    fn a_type_parameter_is_spelled_by_its_name() {
        let model = bound("d Box<T> { $item: `d` = T; }");
        assert_eq!(spelled(&model, "item").as_deref(), Some("T"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:f16148699adfd180079861fb3fc326f62a6054b22de7cba1572f954cd5c6bc6d
    #[test]
    fn a_spelled_type_reads_back_as_the_same_type() {
        let model = bound("d A {}\nd B {}\nd S { $u: `d` = A | B; }");
        let spelled_u = spelled(&model, "u").expect("u has a type");
        let again = bound(&format!("d A {{}}\nd B {{}}\nd S {{ $u: `d` = {spelled_u}; }}"));
        assert_eq!(spelled(&again, "u"), Some(spelled_u));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:89c6d688f07b4e56903193fa729c9e26ec93a1a0a789a89665d68a4d40f09c6e
    #[test]
    fn a_union_is_spelled_by_its_members() {
        let model = bound("d A {}\nd B {}\nd S { $u: `d` = A | B; }");
        assert_eq!(spelled(&model, "u").as_deref(), Some("A | B"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:b29f2ab17865c51c9fee248c87c30d45d61d4ccb9b48e63fe45223606d734cd1
    #[test]
    fn a_generic_is_spelled_with_its_arguments_between_angle_brackets() {
        let model = bound("d A {}\nd Box<T> {}\nconst g = Box<A>;");
        assert_eq!(spelled(&model, "g").as_deref(), Some("Box<A>"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:a41acb96ef1f511b638ed7a20104a4743c98bb3b644142509f866c74e69da009
    #[test]
    fn a_list_is_spelled_with_brackets() {
        let model = bound("d A {}\nd S { $all: `d` = A[]; }");
        assert_eq!(spelled(&model, "all").as_deref(), Some("A[]"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:61414516aa9dcc43d54bb2290614a167fef3bd1189114bcc921d597f01a1d949
    #[test]
    fn a_list_of_a_union_or_a_function_type_is_parenthesized() {
        let model = bound(
            "d A {}\nd B {}\nd S { $pairs: `d` = (A | B)[]; }\ntype F<U> { calls = ((item: U) => U)[] }",
        );
        assert_eq!(spelled(&model, "pairs").as_deref(), Some("(A | B)[]"));
        assert_eq!(
            spelled(&model, "calls").as_deref(),
            Some("((item: U) => U)[]")
        );
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:4a96c90b5e69bd1315e4403a97ce4b57f62102d1d36e4ef0780a8337d654743d
    #[test]
    fn a_function_type_is_spelled_with_its_parameters_and_output() {
        let model = bound("d A {}\ntype S { f = (one: string, two: number) => A }");
        assert_eq!(
            spelled(&model, "f").as_deref(),
            Some("(one: string, two: number) => A")
        );
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:b6bd675e8cdc1c15ba551fc96016b949e34f090af28569c0ca06c74db3d2bf91
    #[test]
    fn a_plain_parameter_is_spelled_by_its_name_and_type() {
        let model = bound("d A {}\ntype S { f = (one: string) => A }");
        assert_eq!(spelled(&model, "f").as_deref(), Some("(one: string) => A"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:ead93c122f4d87f56344f0d4d43c1dba5eb4890a6ec0616bccca1833d30ee448
    #[test]
    fn an_optional_parameter_is_spelled_with_a_question_mark() {
        let model = bound("d A {}\ntype S { f = (two?: number) => A }");
        assert_eq!(spelled(&model, "f").as_deref(), Some("(two?: number) => A"));
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:71cbc82af844db09e5de8e281ec7fcf92ecd3fb7c81640e38d65ec84d146dd3c
    #[test]
    fn a_spread_parameter_is_spelled_with_three_dots() {
        let model = bound("d A {}\ntype S { f = (...rest: string[]) => A }");
        assert_eq!(
            spelled(&model, "f").as_deref(),
            Some("(...rest: string[]) => A")
        );
    }

    // @lfy def/model/type.lfy:typeTextOf#typeTextOf:typeTextOf:b64f3996bad7896ba9710253adc19dbf22a7e51e3fc2c012b67aaac677979e0a
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
