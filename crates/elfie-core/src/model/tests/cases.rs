// @lfy def/model/main.lfy:bind
//! The `@test` cases of `bind`.

use super::*;

// @lfy def/model/main.lfy:bind#bind:bind:ba97510fe33631742318285e9d014009568e9145e198b7cab795d387f224b9ed
// @lfy def/model/main.lfy:resolve#resolve:resolve:53a3ecc3bb2df32dde8139e2bf330f085ed18d7051765d5fc332abe19efd20e5
#[test]
fn test_trait_member_and_application() {
    let model = bind_one("trait t { $x: `d` = string; } d A is t {} const y = A.x;");
    assert_clean(&model);
    assert_eq!(names(&model, model.file_scopes[0]), ["t", "A", "y"]);
    let t = entity(&model, "t");
    let a = entity(&model, "A");
    // One Applied for t.
    assert_eq!(model.entities[a].traits.len(), 1);
    assert_eq!(model.entities[a].traits[0].entity, t);
    assert!(matches!(
        model.entities[a].traits[0].source,
        AppliedSource::Is(_)
    ));
    // A member x with definition d and type string.
    let x = member(&model, a, "x");
    assert_eq!(model.symbols[x].kind, SymbolKind::Member);
    let x_entity = model.symbols[x].entity;
    assert_eq!(model.entities[x_entity].definition.as_deref(), Some("d"));
    assert_eq!(
        model.entities[x_entity].ty,
        Some(TypeRef::Primitive("string"))
    );
    // Trait.entities of t holding A.
    assert_eq!(entities_of(&model, t), vec![a]);
    // A Usage for A.x resolving to that member.
    let member_node = node(&model, 0, E::Member, "A.x");
    let use_of_x = usage(&model, member_node);
    assert_eq!(use_of_x.name.as_deref(), Some("x"));
    assert_eq!(use_of_x.layer, Layer::Value);
    assert_eq!(use_of_x.symbol, Some(x));
    assert_eq!(resolve(&model, member_node), Some(x));
}

// @lfy def/model/main.lfy:bind#bind:bind:8be862e432d183265802e189a8108a3201256cc35ce11fa53d6adc08d36807dd
// @lfy def/model/main.lfy:bind#bind:bind:85e0b777d37918aa64dd528397bd3f5891099278b7d9d4b0c17cd037bcc845ff
// @lfy def/model/main.lfy:resolve#resolve:resolve:38d6961088ad199ed76c8f764b44b1c881e653dd079ba248e2e38c3d317d2632
// @lfy def/model/main.lfy:usagesOf#usagesOf:usagesOf:f5c6a0b87ff4045a8a8155ddade52ffe53608bfbb7a906aa5191ef0684804f7c
#[test]
fn test_module_symbol_and_loop_variable() {
    let main = source(
        "main.lfy",
        "use \"./tokens/keyword\" as Keyword; for (const token in Keyword) { }",
        &[Some("tokens/keyword.lfy")],
    );
    let keyword = source("tokens/keyword.lfy", "d Const {} d Let {}", &[]);
    let model = bind(vec![main, keyword]);
    assert_clean(&model);
    let main = model.file("main.lfy").unwrap();
    let keyword = model.file("tokens/keyword.lfy").unwrap();
    // A module symbol Keyword bound to the second file's entity.
    let module = file_symbol(&model, main, "Keyword");
    assert_eq!(model.symbols[module].kind, SymbolKind::Module);
    assert_eq!(model.symbols[module].entity, model.file_entities[keyword]);
    // A loop variable token in the For scope, and nowhere else.
    let for_node = node(&model, main, S::For, "for (const token in Keyword) { }");
    let for_scope = model.scope_of(for_node).expect("a For owns a scope");
    assert_eq!(names(&model, for_scope), ["token"]);
    let token = model.lookup_local(for_scope, "token").unwrap();
    assert_eq!(model.symbols[token].kind, SymbolKind::LoopVariable);
    assert_eq!(names(&model, model.file_scopes[main]), ["Keyword"]);
    assert_eq!(model.symbol_of(for_node), Some(token));
    // A Usage of Keyword with layer value resolving to the module symbol.
    let use_of_module = usage(&model, node(&model, main, E::Name, "Keyword"));
    assert_eq!(use_of_module.layer, Layer::Value);
    assert_eq!(use_of_module.symbol, Some(module));
    assert_eq!(usages_of(&model, module).len(), 1);
}

// @lfy def/model/main.lfy:bind#bind:bind:0e0ebc3f4e95b4a0d73136e1a0a343ae561cd0d20a56025c7457b59f3702bf7d
// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
// @lfy def/model/main.lfy:resolve#resolve:resolve:0752010452c7ee73c4f54e7a2440a5284244bf5798e148f4798a6a6da5832785
#[test]
fn test_unresolved_name() {
    let model = bind_one("const y = z;");
    assert_eq!(names(&model, model.file_scopes[0]), ["y"]);
    let z = node(&model, 0, E::Name, "z");
    let use_of_z = usage(&model, z);
    assert_eq!(use_of_z.name.as_deref(), Some("z"));
    assert_eq!(use_of_z.symbol, None);
    assert_eq!(resolve(&model, z), None);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, z);
}

// @lfy def/model/main.lfy:bind
#[test]
fn test_extends_entities_and_extenders() {
    let model = bind_one("trait a {} trait b extends a {} d C is b {}");
    assert_clean(&model);
    let (a, b, c) = (
        entity(&model, "a"),
        entity(&model, "b"),
        entity(&model, "C"),
    );
    assert_eq!(entities_of(&model, a), vec![c]);
    assert_eq!(model.entities[a].extenders(), &[b]);
    assert_eq!(entities_of(&model, b), vec![c]);
    assert_eq!(model.entities[b].extenders(), &[] as &[EntityId]);
    // C carries b directly and a through b.
    let traits: Vec<EntityId> = model.entities[c].traits.iter().map(|t| t.entity).collect();
    assert_eq!(traits, [b, a]);
    assert!(matches!(
        model.entities[c].traits[1].source,
        AppliedSource::Inherited(0)
    ));
    assert!(matches!(
        model.entities[b].traits[0].source,
        AppliedSource::Extends(_)
    ));
}

// @lfy def/model/main.lfy:bind#bind:bind:7b05f6d5579971e158b35b5c1a33048105acd947f999f60a52c06f4489505b74
// @lfy def/model/main.lfy:bind#bind:bind:e1768186983ff55248116d434452fbc5887f69cc23437b9787440736b57fb111
// @lfy def/model/main.lfy:bind#bind:bind:972725faf5a2897534a0e4be77c67b10e2cce3499191a51d3ab27e1bbe14d1ae
#[test]
fn test_with_on_a_member_attaches_to_the_member() {
    let model = bind_one(
        "d X { $m: `d` = string; with X$m { where (`s`) -> `b`; @test({ input = \"a\", expect = \"a\" }); } }",
    );
    assert_clean(&model);
    let x = entity(&model, "X");
    let m = model.symbols[member(&model, x, "m")].entity;
    // The member carries one criterion whose situation is s and behavior is b.
    assert_eq!(
        criteria_texts(&model, m),
        [(
            Some(vec!["s".to_string()]),
            Some(vec!["b".to_string()]),
            m
        )]
    );
    // And one test.
    assert_eq!(model.entities[m].tests.len(), 1);
    assert_eq!(model.entities[m].tests[0].input_text, "\"a\"");
    assert_eq!(model.entities[m].tests[0].expect_text, "\"a\"");
    // X carries no criteria and no tests.
    assert!(model.entities[x].acceptance_criteria.is_empty());
    assert!(model.entities[x].tests.is_empty());
}

// @lfy def/model/main.lfy:bind#bind:bind:aca4776b504719a25e83650081cfb2797bbc9776038499ce3118a8868655f65d
// @lfy def/model/main.lfy:bind#bind:bind:7b5a1fec626e619b600c3faefaa81db781f8ebdd297cba7893369d95d1f212c7
// @lfy def/model/main.lfy:bind#bind:bind:88eb2d2cca1fc9e39dfbb1fd77266bdb656d9b5b76c1c8d103d7ea2000ccaa4d
// @lfy def/model/main.lfy:bind#bind:bind:96fe0cf1480dabf1b8ddea3f3c4ae29add39001d68d41f069dc4e8035dce6095
#[test]
fn test_a_program_file_resolves_names_in_the_prelude() {
    let entity_file = source_from(
        "lib/prelude/entity.lfy",
        "d Entity { $identifier: `The declared name` = string; }",
        &[],
        Origin::Library,
    );
    let main = source_from(
        "lib/main.lfy",
        "use \"./prelude/entity\";",
        &[Some("lib/prelude/entity.lfy")],
        Origin::Prelude,
    );
    let a = source("a.lfy", "d A {} const n = A@identifier; const e = Entity;", &[]);
    let model = bind(vec![entity_file, main, a]);
    assert_clean(&model);
    let (library, prelude, program) = (
        model.file("lib/prelude/entity.lfy").unwrap(),
        model.file("lib/main.lfy").unwrap(),
        model.file("a.lfy").unwrap(),
    );
    // The parent of a program file's scope is the prelude scope; a library file and the
    // prelude itself have none.
    assert_eq!(
        model.scopes[model.file_scopes[program]].parent,
        Some(model.file_scopes[prelude])
    );
    assert_eq!(model.scopes[model.file_scopes[prelude]].parent, None);
    assert_eq!(model.scopes[model.file_scopes[library]].parent, None);
    // The usage of Entity resolves to the prelude's symbol, which the prelude imports.
    let declared = file_symbol(&model, library, "Entity");
    assert_eq!(
        model.scopes[model.file_scopes[prelude]].imports,
        [declared]
    );
    assert_eq!(
        usage(&model, node(&model, program, E::Name, "Entity")).symbol,
        Some(declared)
    );
    // The usage of identifier resolves to that member of Entity.
    let identifier = member(&model, model.symbols[declared].entity, "identifier");
    let read = usage(&model, node(&model, program, E::Member, "A@identifier"));
    assert_eq!(read.layer, Layer::Context);
    assert_eq!(read.symbol, Some(identifier));
}

// @lfy def/model/main.lfy:bind#bind:bind:cb5992ae8591ce356ff1a44cf61809f3ae9baf591461b92a8a07297a87520468
// @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
#[test]
fn test_apply_resolves_through_the_kind_data() {
    let text = "trait t {} d A {} t.apply(A);";
    // With a prelude, apply is the member of Trait the prelude declares.
    let model = bind_with_prelude(text);
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let apply = prelude_member(&model, "Trait", "apply");
    assert_eq!(
        usage(&model, node(&model, program, E::Member, "t.apply")).symbol,
        Some(apply)
    );
    let t = model.symbols[file_symbol(&model, program, "t")].entity;
    let a = model.symbols[file_symbol(&model, program, "A")].entity;
    assert_eq!(model.entities[a].traits.len(), 1);
    assert_eq!(model.entities[a].traits[0].entity, t);
    assert_eq!(entities_of(&model, t), vec![a]);
    // Without one the call still applies the trait, and apply resolves to nothing.
    // @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
    let model = bind_one(text);
    assert_eq!(problems(&model), vec!["t.apply: t has no member apply"]);
    let (t, a) = (entity(&model, "t"), entity(&model, "A"));
    assert_eq!(model.entities[a].traits.len(), 1);
    assert_eq!(model.entities[a].traits[0].entity, t);
    assert_eq!(
        usage(&model, node(&model, 0, E::Member, "t.apply")).symbol,
        None
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:4cb96974d918e7df0ed61b9db5926546a7fc19ffa628ee3afe7f53046371017b
// @lfy def/model/main.lfy:bind#bind:bind:d6f02618aac6e3cd44227a66ddaf2ec1f0c729ec00b56a27ed1104b872133624
// @lfy def/model/main.lfy:bind#bind:bind:84b135bc194c962eab1f272cc9a44b88891577b78f5263c55b7da1a415de77b2
#[test]
fn test_rule_named_in_is_clause_resolves_to_the_trait() {
    let b = source(
        "b.lfy",
        "trait rule(syntax: string) { $rule: `the rule` = string; }",
        &[],
    );
    let a = source(
        "a.lfy",
        "use \"./b\"; d A is rule(`x`): `a` {} d B is rule(`y`): `b` {}",
        &[Some("b.lfy")],
    );
    let model = bind(vec![b, a]);
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let rule_file = model.file("b.lfy").unwrap();
    let rule_trait = file_symbol(&model, rule_file, "rule");
    let rule_entity = model.symbols[rule_trait].entity;
    // Both usages of rule resolve to the trait, not the member A and B each gain.
    let use_a = usage(&model, node(&model, program, E::TraitUse, "rule(`x`)"));
    let use_b = usage(&model, node(&model, program, E::TraitUse, "rule(`y`)"));
    assert_eq!(use_a.symbol, Some(rule_trait));
    assert_eq!(use_b.symbol, Some(rule_trait));
    let (a_entity, b_entity) = (
        model.symbols[file_symbol(&model, program, "A")].entity,
        model.symbols[file_symbol(&model, program, "B")].entity,
    );
    assert_ne!(member(&model, a_entity, "rule"), rule_trait);
    // A and B each carry one Applied for rule.
    assert_eq!(model.entities[a_entity].traits.len(), 1);
    assert_eq!(model.entities[a_entity].traits[0].entity, rule_entity);
    assert_eq!(model.entities[b_entity].traits.len(), 1);
    assert_eq!(model.entities[b_entity].traits[0].entity, rule_entity);
}

// @lfy def/model/main.lfy:bind#bind:bind:7a0b3c3b948e3a548a7a00553d1a2034a3f8e40b40d58671af64e6894b3904a9
// @lfy def/model/main.lfy:bind#bind:bind:3337bcd2f277d75bdff5cf73924f263e8aaba8499b6f5777ab89546662fd288b
#[test]
fn test_current_with_no_member_name_applies_to_the_file() {
    let model = bind_with_prelude("trait t {} d A {} t.apply(@);");
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let t = model.symbols[file_symbol(&model, program, "t")].entity;
    let a = model.symbols[file_symbol(&model, program, "A")].entity;
    let file_entity = model.file_entities[program];
    assert_eq!(model.entities[file_entity].traits.len(), 1);
    assert_eq!(model.entities[file_entity].traits[0].entity, t);
    assert!(model.entities[a].traits.is_empty());
}

// @lfy def/model/main.lfy:bind#bind:bind:355e034e3cf906671eb9c8b2a9aa0ce561cb231515c8d84b0bcbd34e6e176a8c
// @lfy def/model/main.lfy:bind#bind:bind:ae1713bf3f23dc73e68d44c37f8cc54e9074e2bc5281035c1bac774fea5c6001
#[test]
fn test_type_parameter_of_a_declaration() {
    let model = bind_one("d Item {} d Box<T extends Item = string> { $item: `x` = T; }");
    assert_clean(&model);
    // Symbols Item and Box in the file scope.
    assert_eq!(names(&model, model.file_scopes[0]), ["Item", "Box"]);
    let item = entity(&model, "Item");
    let boxed = entity(&model, "Box");
    // A symbol T of kind typeParameter in the scope Box owns, beside its members.
    let box_scope = model.entities[boxed].scope.expect("a data owns a scope");
    assert_eq!(names(&model, box_scope), ["T", "item"]);
    let t = model.lookup_local(box_scope, "T").expect("a symbol T");
    assert_eq!(model.symbols[t].kind, SymbolKind::TypeParameter);
    // Entity.typeParameters of Box holds T's entity alone.
    let t = model.symbols[t].entity;
    assert_eq!(model.type_parameters(boxed), vec![t]);
    // Its type is Item, its default is string, and it is optional and never a spread.
    assert_eq!(model.entities[t].ty, Some(TypeRef::Entity(item)));
    assert_eq!(
        model.entities[t].value("defaultValue"),
        Some(&Value::Type(Box::new(TypeRef::Primitive("string"))))
    );
    assert_eq!(model.entities[t].value("optional"), Some(&Value::Bool(true)));
    assert_eq!(model.entities[t].value("spread"), Some(&Value::Bool(false)));
    // The member item has T's entity as its type.
    let member_item = model.symbols[member(&model, boxed, "item")].entity;
    assert_eq!(model.entities[member_item].ty, Some(TypeRef::Entity(t)));
}

// @lfy def/model/main.lfy:bind#bind:bind:3189c3f37ea68cb3dc89e1c79842252c20b3b013dd01e698a55e37d78fd03c1b
// @lfy def/model/main.lfy:bind#bind:bind:b14a00f45f5f1aed95a60c9685ea61353a2569a91f82696c6db3053bda11450e
// @lfy def/model/main.lfy:bind#bind:bind:c3a2c16f705e37fc0e8e626331285a435d93f0cde33bcb941c715741f6b53131
#[test]
fn test_a_declaration_seen_with_type_arguments() {
    let model = bind_one(
        "d Box<T> { $item: `x` = T; $items: `y` = T[]; } const b = Box<string>; const i = b.item; const j = b.items;",
    );
    assert_clean(&model);
    let boxed = entity(&model, "Box");
    // The type of b is Box seen with one type argument, string.
    let Some(TypeRef::Entity(seen)) = model.entities[entity(&model, "b")].ty else {
        panic!("b has no entity type");
    };
    assert_eq!(model.entities[seen].identifier.as_deref(), Some("Box"));
    assert_eq!(model.entities[seen].ty, Some(TypeRef::Entity(boxed)));
    assert_eq!(
        model.type_arguments(seen),
        vec![TypeRef::Primitive("string")]
    );
    assert_eq!(model.type_parameters(seen), model.type_parameters(boxed));
    // The usage of item on b resolves to Box's member, and i has type string.
    let read = node(&model, 0, E::Member, "b.item");
    assert_eq!(usage(&model, read).symbol, Some(member(&model, boxed, "item")));
    assert_eq!(
        model.entities[entity(&model, "i")].ty,
        Some(TypeRef::Primitive("string"))
    );
    // j has type List of string, so its item type is string.
    let j = &model.entities[entity(&model, "j")];
    assert_eq!(
        j.ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("string"))))
    );
    assert_eq!(j.item_type, Some(TypeRef::Primitive("string")));
    // The member items of Box itself keeps its type, List of T.
    let t = model.type_parameters(boxed)[0];
    let items = model.symbols[member(&model, boxed, "items")].entity;
    assert_eq!(
        model.entities[items].ty,
        Some(TypeRef::List(Box::new(TypeRef::Entity(t))))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:3ac95c4e5cc4405dcbbe7fdeffdc22176a54f84d56caf246b594f4d1797bc365
#[test]
fn test_too_many_type_arguments_is_a_problem() {
    let model = bind_one("d Box<T> {} const b = Box<string, number>;");
    let Some(TypeRef::Entity(seen)) = model.entities[entity(&model, "b")].ty else {
        panic!("b has no entity type");
    };
    assert_eq!(
        model.type_arguments(seen),
        vec![TypeRef::Primitive("string"), TypeRef::Primitive("number")]
    );
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(
        model.problems[0].node,
        node(&model, 0, E::Generic, "Box<string, number>")
    );
}
