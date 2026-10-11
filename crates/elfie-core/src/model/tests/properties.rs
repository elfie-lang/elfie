// @lfy def/model/main.lfy:bind
//! Tests of the properties an entity carries, and of `usagesOf` and `criteriaOf`.

use super::*;

// @lfy def/model/main.lfy:bind
#[test]
fn identifier_definition_parameters_and_output() {
    let model = bind_one(
        "d A: `about A` {} d B: 'about B' {} fn f(a: string, ...rest: number[]): `does f` => A {} function g() -> string { return 'x'; }",
    );
    assert_clean(&model);
    let a = entity(&model, "A");
    assert_eq!(model.entities[a].identifier.as_deref(), Some("A"));
    assert_eq!(model.entities[a].definition.as_deref(), Some("about A"));
    assert_eq!(
        model.entities[entity(&model, "B")].definition.as_deref(),
        Some("about B")
    );
    let f = &model.entities[entity(&model, "f")];
    assert_eq!(f.identifier.as_deref(), Some("f"));
    assert_eq!(f.definition.as_deref(), Some("does f"));
    let parameters: Vec<(String, SymbolKind)> = f
        .parameters()
        .iter()
        .map(|&s| (model.symbols[s].name.clone(), model.symbols[s].kind))
        .collect();
    assert_eq!(
        parameters,
        [
            ("a".to_string(), SymbolKind::Parameter),
            ("rest".to_string(), SymbolKind::Parameter)
        ]
    );
    assert_eq!(f.output(), Some(&TypeRef::Entity(a)));
    assert!(matches!(f.kind, EntityKind::Fn { agent: true, .. }));
    let g = &model.entities[entity(&model, "g")];
    assert_eq!(g.definition, None);
    assert!(g.parameters().is_empty());
    assert_eq!(g.output(), Some(&TypeRef::Primitive("string")));
    assert!(matches!(g.kind, EntityKind::Fn { agent: false, .. }));
    // A parameter's type is the type after its colon.
    let rest = model.symbols[f.parameters()[1]].entity;
    assert_eq!(
        model.entities[rest].ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("number"))))
    );
}

// @lfy def/model/main.lfy:bind
#[test]
fn an_alias_and_an_external_declare_a_symbol_and_an_entity() {
    let model = bind_one("d A {} alias B = A; external C = A;");
    assert_clean(&model);
    assert_eq!(names(&model, model.file_scopes[0]), ["A", "B", "C"]);
    let a = entity(&model, "A");
    // An alias names what it points at: its symbol is bound to the target's entity.
    let alias = file_symbol(&model, 0, "B");
    assert_eq!(model.symbols[alias].kind, SymbolKind::Alias);
    assert_eq!(model.symbols[alias].entity, a);
    // An external has an entity of its own, typed by its right side.
    let external = file_symbol(&model, 0, "C");
    assert_eq!(model.symbols[external].kind, SymbolKind::External);
    let c = model.symbols[external].entity;
    assert_ne!(c, a);
    assert_eq!(model.entities[c].identifier.as_deref(), Some("C"));
    assert_eq!(model.entities[c].kind, EntityKind::External);
    assert_eq!(model.entities[c].ty, Some(TypeRef::Entity(a)));
}

// @lfy def/model/main.lfy:bind#bind:bind:121c6cc109446373ec3499d3bf3ced3751d90b778e159a288124ae94e9d5201c
// @lfy def/model/main.lfy:bind#bind:bind:e3a0d7578b24b0586d11c50514b0da77e12ccd6f18672cfca1ef83805e2beec3
// @lfy def/model/main.lfy:bind#bind:bind:f355d3a4fbe3ba176fa6760801c02f0e6a826a0bd65f0a917451f111c3c9a0dc
#[test]
fn an_index_with_nothing_inside_is_a_list_of_the_left_side() {
    let model = bind_one("d A {} const xs: A[] = []; d D { $one = A[]; $two = A[][]; }");
    assert_clean(&model);
    let a = entity(&model, "A");
    let of_a = TypeRef::List(Box::new(TypeRef::Entity(a)));
    let d = entity(&model, "D");
    let member_type = |name: &str| {
        model.entities[model.symbols[member(&model, d, name)].entity]
            .ty
            .clone()
    };
    assert_eq!(member_type("one"), Some(of_a.clone()));
    assert_eq!(
        member_type("two"),
        Some(TypeRef::List(Box::new(of_a.clone())))
    );
    // The brackets of a type position read the same.
    let xs = &model.entities[entity(&model, "xs")];
    assert_eq!(xs.ty, Some(of_a));
    assert_eq!(xs.item_type, Some(TypeRef::Entity(a)));
}

// @lfy def/model/main.lfy:bind
#[test]
fn type_of_a_declaration_is_itself() {
    let model = bind_one("d A {} trait T {} type Ty { k = string, } enum En { a = 'a' }");
    assert_clean(&model);
    for name in ["A", "T", "Ty", "En"] {
        let e = entity(&model, name);
        assert_eq!(model.entities[e].ty, Some(TypeRef::Entity(e)), "{name}");
        assert_eq!(model.entities[e].item_type, None, "{name}");
    }
}

// @lfy def/model/main.lfy:bind
#[test]
fn type_and_item_type() {
    let model = bind_one(
        "d A {} const xs: A[] = []; const n = 1; const s = 'text'; d D { $items = string[]; $one = A; }",
    );
    assert_clean(&model);
    let a = entity(&model, "A");
    let xs = &model.entities[entity(&model, "xs")];
    assert_eq!(xs.ty, Some(TypeRef::List(Box::new(TypeRef::Entity(a)))));
    assert_eq!(xs.item_type, Some(TypeRef::Entity(a)));
    let n = &model.entities[entity(&model, "n")];
    assert_eq!(n.ty, Some(TypeRef::Literal(Box::new(Value::Number(1.0)))));
    assert_eq!(n.item_type, None);
    let s = &model.entities[entity(&model, "s")];
    assert_eq!(
        s.ty,
        Some(TypeRef::Literal(Box::new(Value::String("text".into()))))
    );
    let d = entity(&model, "D");
    let items = &model.entities[model.symbols[member(&model, d, "items")].entity];
    assert_eq!(
        items.ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("string"))))
    );
    assert_eq!(items.item_type, Some(TypeRef::Primitive("string")));
    let one = &model.entities[model.symbols[member(&model, d, "one")].entity];
    assert_eq!(one.ty, Some(TypeRef::Entity(a)));
    assert_eq!(one.item_type, None);
    // A bare List, with no argument, has no item type; List of one argument is spelled
    // as a list, so it is the same type as the brackets give.
    // @lfy def/model/main.lfy:bind#bind:bind:121c6cc109446373ec3499d3bf3ced3751d90b778e159a288124ae94e9d5201c
    let mut sources = prelude();
    sources.push(source(
        "a.lfy",
        "const bare: List = 1; const of_string: List<string> = 1; const sugared: string[] = [];",
        &[],
    ));
    let model = bind(sources);
    let list = prelude_data(&model, "List");
    let program = model.file("a.lfy").unwrap();
    let bare = model.symbols[file_symbol(&model, program, "bare")].entity;
    assert_eq!(model.entities[bare].ty, Some(TypeRef::Entity(list)));
    assert_eq!(model.entities[bare].item_type, None);
    let of_string = model.symbols[file_symbol(&model, program, "of_string")].entity;
    assert_eq!(
        model.entities[of_string].ty,
        model.entities[model.symbols[file_symbol(&model, program, "sugared")].entity].ty
    );
    assert_eq!(
        model.entities[of_string].item_type,
        Some(TypeRef::Primitive("string"))
    );
    // This prelude's List lists no type parameter, so the one argument is an arity
    // problem; the type is bound with the argument as written all the same.
    // @lfy def/model/main.lfy:bind#bind:bind:3ac95c4e5cc4405dcbbe7fdeffdc22176a54f84d56caf246b594f4d1797bc365
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(
        model.problems[0].node,
        node(&model, program, E::TypeArguments, "<string>")
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:5b5df5fcbe0bfad1f4242837461d9e5756207c0a361e81048345a263528d5740
// @lfy def/model/main.lfy:bind#bind:bind:a5c1bc569ddb92f8dd361e4827d274e8148ec9c79e5f47f9178da3acd27018eb
#[test]
fn a_list_value_has_the_type_of_a_list() {
    let model = bind_with_prelude(
        "d A {} const xs = [1, 2]; const n = xs.length; const many = ['text', A]; \
         const spread = [...xs, 3]; const empty = [];",
    );
    assert_clean(&model);
    let file = model.file("a.lfy").unwrap();
    let ty = |name: &str| {
        model.entities[model.symbols[file_symbol(&model, file, name)].entity]
            .ty
            .clone()
    };
    let item = |name: &str| {
        model.entities[model.symbols[file_symbol(&model, file, name)].entity]
            .item_type
            .clone()
    };
    // A list of numbers is a list of number: the items are values of that kind, not the
    // one value each was written as.
    // @lfy def/model/main.lfy:bind
    assert_eq!(
        ty("xs"),
        Some(TypeRef::List(Box::new(TypeRef::Primitive("number"))))
    );
    assert_eq!(item("xs"), Some(TypeRef::Primitive("number")));
    // Its base data is List, so the members of List are read on it.
    reads_member(&model, node(&model, file, E::Member, "xs.length"), "List", "length");
    // Items of more than one type are a union of them, each written once; a spread holds
    // what the list it spreads holds.
    // @lfy def/model/main.lfy:bind
    let a = model.symbols[file_symbol(&model, file, "A")].entity;
    assert_eq!(
        item("many"),
        Some(TypeRef::Union(vec![
            TypeRef::Primitive("string"),
            TypeRef::Entity(a)
        ]))
    );
    assert_eq!(item("spread"), Some(TypeRef::Primitive("number")));
    // An empty list is a list all the same, of nothing the model can read.
    assert!(matches!(ty("empty"), Some(TypeRef::List(_))));
}

// @lfy def/model/main.lfy:bind#bind:bind:7ef157b80d31dce9baf260f3d953c0bc5b6cfb07b6c4773de70e7be59015ad54
#[test]
fn a_declaration_without_type_parameters_has_none() {
    let model = bind_one("d Plain { $x: `d` = string; } const p: Plain = 1; fn f(): `d` => Plain {}");
    assert_clean(&model);
    let plain = entity(&model, "Plain");
    assert!(model.type_parameters(plain).is_empty());
    assert!(model.type_arguments(plain).is_empty());
    // A Reference without arguments yields the declaration itself.
    assert_eq!(
        model.entities[entity(&model, "p")].ty,
        Some(TypeRef::Entity(plain))
    );
    assert!(model.type_arguments(entity(&model, "p")).is_empty());
}

// @lfy def/model/main.lfy:bind#bind:bind:ae23ffc23c95add84b11276b2987d77d26def0d947d817a507989143282af46c
// @lfy def/model/main.lfy:bind#bind:bind:c3a2c16f705e37fc0e8e626331285a435d93f0cde33bcb941c715741f6b53131
#[test]
fn a_function_type_is_an_anonymous_function_entity() {
    let model = bind_one("type Pair<T> { f = (item: T) => T, } const p: Pair<string> = 1; const q = p.f;");
    assert_clean(&model);
    let pair = entity(&model, "Pair");
    let t = model.type_parameters(pair)[0];
    // The FunctionType owns a scope holding its parameters, and is an anonymous Fn.
    let function_type = node(&model, 0, E::FunctionType, "(item: T) => T");
    let scope = model.scope_of(function_type).expect("a FunctionType owns a scope");
    assert_eq!(names(&model, scope), ["item"]);
    let anonymous = model.scopes[scope].current;
    assert_eq!(model.entities[anonymous].identifier, None);
    assert!(matches!(
        model.entities[anonymous].kind,
        EntityKind::Fn { agent: false, .. }
    ));
    let item = model.symbols[model.entities[anonymous].parameters()[0]].entity;
    assert_eq!(model.entities[item].ty, Some(TypeRef::Entity(t)));
    assert_eq!(model.entities[anonymous].output(), Some(&TypeRef::Entity(t)));
    // Read on a Pair of string, the parameters and the output are substituted.
    let Some(TypeRef::Entity(read)) = model.entities[entity(&model, "q")].ty else {
        panic!("q has no entity type");
    };
    assert_ne!(read, anonymous, "the substitution is a copy, not the declaration");
    let substituted = model.symbols[model.entities[read].parameters()[0]].entity;
    assert_eq!(
        model.entities[substituted].ty,
        Some(TypeRef::Primitive("string"))
    );
    assert_eq!(
        model.entities[read].output(),
        Some(&TypeRef::Primitive("string"))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:b14a00f45f5f1aed95a60c9685ea61353a2569a91f82696c6db3053bda11450e
// @lfy def/model/main.lfy:bind#bind:bind:7ef157b80d31dce9baf260f3d953c0bc5b6cfb07b6c4773de70e7be59015ad54
#[test]
fn a_declaration_seen_with_arguments_carries_the_declarations_own() {
    let model = bind_one(
        "trait t {} d Box<T> is t: `a box` { $item: `d` = T; where (`s`) -> `b`; } const b = Box<string>;",
    );
    assert_clean(&model);
    let boxed = entity(&model, "Box");
    let Some(TypeRef::Entity(seen)) = model.entities[entity(&model, "b")].ty else {
        panic!("b has no entity type");
    };
    assert_eq!(model.entities[seen].identifier.as_deref(), Some("Box"));
    assert_eq!(model.entities[seen].definition.as_deref(), Some("a box"));
    assert_eq!(criteria_texts(&model, seen), criteria_texts(&model, boxed));
    assert!(!criteria_texts(&model, seen).is_empty());
    assert!(model.entities[seen].has_trait(entity(&model, "t")));
    assert_eq!(model.members(seen), model.members(boxed));
    assert_eq!(model.type_parameters(seen), model.type_parameters(boxed));
    assert_eq!(model.entities[seen].ty, Some(TypeRef::Entity(boxed)));
    // The declaration itself is used without arguments, so it has none.
    assert!(model.type_arguments(boxed).is_empty());
}

// @lfy def/model/main.lfy:bind#bind:bind:c3a2c16f705e37fc0e8e626331285a435d93f0cde33bcb941c715741f6b53131
// @lfy def/model/main.lfy:bind#bind:bind:1314d95fa57fff9074b53571e4db2f0029007b9a8661136a4047fe448c4ec7bf
#[test]
fn a_member_read_substitutes_through_nested_arguments() {
    let model = bind_one(
        "d Pair<A, B> {} d Box<T> { $p: `d` = Pair<T, number>; } const b = Box<string>; const c = b.p;",
    );
    assert_clean(&model);
    let pair = entity(&model, "Pair");
    let Some(TypeRef::Entity(read)) = model.entities[entity(&model, "c")].ty else {
        panic!("c has no entity type");
    };
    assert_eq!(model.generic_base(read), pair);
    assert_eq!(
        model.type_arguments(read),
        vec![TypeRef::Primitive("string"), TypeRef::Primitive("number")]
    );
    // The member of Box itself keeps the parameter.
    let t = model.type_parameters(entity(&model, "Box"))[0];
    let declared = model.symbols[member(&model, entity(&model, "Box"), "p")].entity;
    let Some(TypeRef::Entity(declared)) = model.entities[declared].ty else {
        panic!("the member has no entity type");
    };
    assert_eq!(
        model.type_arguments(declared),
        vec![TypeRef::Entity(t), TypeRef::Primitive("number")]
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:972725faf5a2897534a0e4be77c67b10e2cce3499191a51d3ab27e1bbe14d1ae
#[test]
fn add_and_where_append_criteria() {
    let model = bind_one(
        "d A { @acceptanceCriteria.add({ situation = `s`, behavior = [`b1`, `b2`], sideEffects = [`e`] }).add({ behavior = `second` }); where ((`w1`) or (`w2`)) and (`w3`) -> `then`; } \
         d B {} with B { @acceptanceCriteria.add({ behavior = `from with` }); where (`in with`) -> `w`; }",
    );
    assert_clean(&model);
    let a = entity(&model, "A");
    let criteria = &model.entities[a].acceptance_criteria;
    assert_eq!(criteria.len(), 3);
    assert_eq!(criteria[0].situation, Some(vec!["s".to_string()]));
    assert_eq!(
        criteria[0].behavior,
        Some(vec!["b1".to_string(), "b2".to_string()])
    );
    assert_eq!(criteria[0].side_effects, Some(vec!["e".to_string()]));
    assert_eq!(criteria[0].contributor, a);
    assert_eq!(criteria[1].behavior, Some(vec!["second".to_string()]));
    assert_eq!(criteria[1].side_effects, None);
    assert_eq!(
        criteria[2].situation,
        Some(vec!["w1 or w2".to_string(), "w3".to_string()])
    );
    assert_eq!(criteria[2].behavior, Some(vec!["then".to_string()]));
    assert_eq!(criteria[2].contributor, a);
    let b = entity(&model, "B");
    let in_with: Vec<Option<Vec<String>>> = model.entities[b]
        .acceptance_criteria
        .iter()
        .map(|c| c.behavior.clone())
        .collect();
    assert_eq!(
        in_with,
        [
            Some(vec!["from with".to_string()]),
            Some(vec!["w".to_string()])
        ]
    );
    assert!(
        model.entities[model.file_entities[0]]
            .acceptance_criteria
            .is_empty()
    );
}

// The second case reads `A@like(...)` for its expectation.
// @lfy def/model/main.lfy:bind
#[test]
fn test_call_appends_tests() {
    let model = bind_one(
        "d A { @test({ input = 1, expect = 'one' }, { input = [1, 2], expect = A@like(`two`) }); }",
    );
    assert_clean(&model);
    let tests = &model.entities[entity(&model, "A")].tests;
    assert_eq!(tests.len(), 2);
    assert_eq!(tests[0].input_text, "1");
    assert_eq!(tests[0].expect_text, "'one'");
    assert_eq!(tests[1].input_text, "[1, 2]");
    assert_eq!(tests[1].expect_text, "A@like(`two`)");
    assert_eq!(
        tests[0].input.map(|r| model.info(r).rule),
        Some(E::Number.entity())
    );
    assert!(tests[1].expect.is_some());
}

// @lfy def/model/main.lfy:usagesOf#usagesOf:usagesOf:17c43c93cfa66aa764b8655e04d3108f32264d4b7bbae42d1bd6d078241baa1f
#[test]
fn usages_of_follows_an_alias_chain() {
    let model = bind_one("d A {} alias B = A; alias C = B; const x = C; const y = A; const z = 1;");
    assert_clean(&model);
    let found: Vec<String> = usages_of(&model, file_symbol(&model, 0, "A"))
        .into_iter()
        .map(|id| model.raw(model.usages[id].node).trim().to_string())
        .collect();
    assert_eq!(found, ["A", "B", "C", "A"]);
}

// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:10ab31f446369520ef4402e3afa3adf6c05b2834fdd52636b18fba986f4fbb8c
#[test]
fn criteria_of_strips_references() {
    let model = bind_one(
        "d B {} d A { @acceptanceCriteria.add({ situation = `given [[B]]`, behavior = `see [[B]] and [[B.x]]` }); }",
    );
    let a = entity(&model, "A");
    let raw = &model.entities[a].acceptance_criteria[0];
    assert_eq!(
        raw.behavior,
        Some(vec!["see [[B]] and [[B.x]]".to_string()])
    );
    let resolved = criteria_of(&model, a);
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].situation, Some(vec!["given B".to_string()]));
    assert_eq!(
        resolved[0].behavior,
        Some(vec!["see B and B.x".to_string()])
    );
    assert_eq!(resolved[0].contributor, a);
    assert_eq!(entities_of(&model, a), Vec::<EntityId>::new());
}

// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:68ab34550dbf7d97382cac2af535190686399cde2432c30383b3f4d394640826
// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:42d4fffaea6804d033186fda4ffd02f65ef21b5f6d14c5db018da909d530fcc7
#[test]
fn criteria_of_writes_a_type_as_elfie_source() {
    let model = bind_one(
        "d A {} d B {} type S { f = (one: string) => A } \
         d X { $xs: `the items` = (A | B)[]; $call: `the call` = S.f; \
         @acceptanceCriteria \
           .add({ behavior = `Holds {{$xs@type}}` }) \
           .add({ behavior = [`Calls {{$call@type}}`, `and [[A]]`] }); }",
    );
    assert_clean(&model);
    let x = entity(&model, "X");
    let resolved = criteria_of(&model, x);
    assert_eq!(resolved.len(), 2);
    assert_eq!(
        resolved[0].behavior,
        Some(vec!["Holds (A | B)[]".to_string()])
    );
    // The expand pass ran before the function type was read, and wrote `anonymous` for it.
    assert_eq!(
        model.entities[x].acceptance_criteria[1].behavior,
        Some(vec!["Calls anonymous".to_string(), "and [[A]]".to_string()])
    );
    assert_eq!(
        resolved[1].behavior,
        Some(vec![
            "Calls (one: string) => A".to_string(),
            "and A".to_string()
        ])
    );
}
