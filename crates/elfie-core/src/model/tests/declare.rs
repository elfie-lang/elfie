// @lfy def/model/main.lfy:bind
//! Tests of binding as a whole, of the declare pass, and of `use`.

use super::*;

// ---- Binding as a whole -------------------------------------------------------------

// @lfy def/model/main.lfy:bind#bind:bind:e6d6ea2f30dca657ba2c80d4aad957763540ba6c1f58a5ac35f5fdb31087c24d
#[test]
fn an_error_node_declares_and_uses_nothing() {
    let model = bind(vec![source_with_errors(
        "a.lfy",
        "d A {} const class = 1; const y = A;",
    )]);
    // The statements that parsed declare their names; the run of tokens the parser could
    // not fit declares none, makes no usage, and is no problem of the binder's.
    assert_eq!(names(&model, model.file_scopes[0]), ["A", "y"]);
    assert_clean(&model);
    let errors = &model.sources[0].tree.errors;
    assert_eq!(errors.len(), 1, "{:?}", errors);
    let covered = errors[0].start..errors[0].end;
    for usage in &model.usages {
        let info = model.info(usage.node);
        assert!(
            !covered.contains(&info.start),
            "a usage inside the error node: {:?}",
            model.raw(usage.node)
        );
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:1bf3cdabd2eb716539d1d5e6cf9e31e98e17285a8d9b8c0a0edde46ff38e5aa5
#[test]
fn every_problem_binding_adds_has_the_binder_stage() {
    // One from the resolve pass, and one the interpreter adds while expanding.
    let model = bind_one("trait t {} d A {} t.apply(A); const y = z;");
    assert!(model.problems.len() >= 2, "{:?}", problems(&model));
    for problem in &model.problems {
        assert_eq!(problem.stage, Stage::Binder, "{}", problem.message);
    }
}

// ---- Declare ------------------------------------------------------------------------

// @lfy def/model/main.lfy:bind#bind:bind:3337bcd2f277d75bdff5cf73924f263e8aaba8499b6f5777ab89546662fd288b
#[test]
fn file_scope_current_is_the_anonymous_file_entity() {
    let model = bind_one("const x = 1;");
    assert_clean(&model);
    let scope = &model.scopes[model.file_scopes[0]];
    assert_eq!(scope.parent, None);
    assert_eq!(scope.current, model.file_entities[0]);
    let file = &model.entities[model.file_entities[0]];
    assert_eq!(file.kind, EntityKind::File);
    assert_eq!(file.identifier, None);
    assert_eq!(file.node, Some(NodeRef { file: 0, index: 0 }));
    assert_eq!(file.scope, Some(model.file_scopes[0]));
}

// @lfy def/model/main.lfy:bind#bind:bind:583d8bd2aacbcdee71c5cda8c83518a8d2ab447460f739666f8df8093b6fc7a5
// @lfy def/model/main.lfy:bind#bind:bind:5dafa3a6e52ad501215f54b081dab8720e0d83c56fb7277975f861591af27874
// @lfy def/model/main.lfy:bind#bind:bind:bda572541abe6b3dd683ffb7f7c5782bdf7382a98d9a342a3b8d72558c0dc03f
#[test]
fn block_scope_is_child_of_declaration_scope() {
    let model = bind_one("d A { const inner = 1; } function f(p: string) { const q = p; }");
    assert_clean(&model);
    let a = entity(&model, "A");
    let a_scope = model.entities[a].scope.expect("a data owns a scope");
    assert_eq!(model.scopes[a_scope].parent, Some(model.file_scopes[0]));
    assert_eq!(model.scopes[a_scope].current, a);
    let a_block = model
        .scope_of(node(&model, 0, S::Block, "{ const inner = 1; }"))
        .expect("a Block owns a scope");
    assert_eq!(model.scopes[a_block].parent, Some(a_scope));
    assert_eq!(model.scopes[a_block].current, a);
    assert_eq!(names(&model, a_block), ["inner"]);
    let f = entity(&model, "f");
    let f_scope = model.entities[f].scope.unwrap();
    assert_eq!(names(&model, f_scope), ["p"]);
    let f_block = model
        .scope_of(node(&model, 0, S::Block, "{ const q = p; }"))
        .unwrap();
    assert_eq!(model.scopes[f_block].parent, Some(f_scope));
    assert_eq!(names(&model, f_block), ["q"]);
    // Names in a scope are visible throughout it: p resolves from the block. The first
    // Name reading p spells the parameter and is a declaration, not a usage.
    let p = nodes(&model, 0, E::Name, "p");
    assert_eq!(p.len(), 2);
    assert_eq!(model.usage_of(p[0]), None);
    assert_eq!(usage(&model, p[1]).symbol, model.lookup_local(f_scope, "p"));
}

// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
#[test]
fn without_a_prelude_no_file_scope_has_a_parent() {
    let library = source("lib/prelude/entity.lfy", "d Entity {}", &[]);
    let a = source("a.lfy", "const e = Entity;", &[]);
    let model = bind(vec![library, a]);
    for &scope in &model.file_scopes {
        assert_eq!(model.scopes[scope].parent, None);
    }
    let read = node(&model, model.file("a.lfy").unwrap(), E::Name, "Entity");
    assert_eq!(usage(&model, read).symbol, None);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, read);
}

// @lfy def/model/main.lfy:bind#bind:bind:c1bf94ef6ed00fca9ed870486ad290e18a6640d029d6830b1b8b55b921ffc125
#[test]
fn a_file_name_shadows_the_prelude_without_a_problem() {
    let mut sources = prelude();
    sources.push(source("a.lfy", "d Entity {} const e = Entity;", &[]));
    let model = bind(sources);
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let own = file_symbol(&model, program, "Entity");
    assert_ne!(model.symbols[own].entity, prelude_data(&model, "Entity"));
    assert_eq!(
        usage(&model, node(&model, program, E::Name, "Entity")).symbol,
        Some(own)
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:f9e18cc96433cea6408e03a88c1980be0eb6e901bd010d2324727105ed2cd26d
#[test]
fn duplicate_declaration_is_a_problem_and_the_first_wins() {
    let model = bind_one("const x = 1; d x {} const y = x;");
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    let second = node(&model, 0, S::DataDeclaration, "d x {}");
    assert_eq!(model.problems[0].node, second);
    assert_eq!(model.symbol_of(second), None);
    let first = node(&model, 0, S::VariableDeclaration, "const x = 1;");
    let x = file_symbol(&model, 0, "x");
    assert_eq!(model.symbols[x].node, first);
    assert_eq!(model.symbols[x].kind, SymbolKind::Variable);
    assert_eq!(names(&model, model.file_scopes[0]), ["x", "y"]);
    assert_eq!(usage(&model, node(&model, 0, E::Name, "x")).symbol, Some(x));
}

// @lfy def/model/main.lfy:bind#bind:bind:225b9242d9b9e61c650a37dd5d5bc19b72bc8619ce4cf622e81ad1a1fe2fe441
#[test]
fn a_parameter_and_a_member_of_one_scope_may_share_a_name() {
    let model = bind_one("trait t(rule: string) { $rule: `the rule` = string; } d A is t(`x`) {}");
    assert_clean(&model);
    let t = entity(&model, "t");
    let scope = model.entities[t].scope.expect("a trait owns a scope");
    assert_eq!(names(&model, scope), ["rule", "rule"]);
    let kinds: Vec<SymbolKind> = model.scopes[scope]
        .symbols
        .iter()
        .map(|&s| model.symbols[s].kind)
        .collect();
    assert_eq!(kinds, [SymbolKind::Parameter, SymbolKind::Member]);
    // The receiver gains the member, not the parameter.
    let a = entity(&model, "A");
    assert_eq!(model.symbols[member(&model, a, "rule")].kind, SymbolKind::Member);
}

// @lfy def/model/main.lfy:bind#bind:bind:93c5268d20befb000ee5648dec6bb01d0e3f7414ba2340321ba1115432ad0e90
// @lfy def/model/main.lfy:bind#bind:bind:643d4a3ad6bf896608420d4cea390c9ad3caf7b86c19c85c2582278080f7cb41
#[test]
fn a_type_parameter_is_declared_in_the_scope_of_its_declaration() {
    let model = bind_one(
        "d Item {} fn take<T extends Item>(p: T): `d` => T { const q = p; } \
         trait held<U>: `d` { $u: `d` = U; } type Pair<V> { f = V, }",
    );
    assert_clean(&model);
    for (name, spelled) in [("take", "T"), ("held", "U"), ("Pair", "V")] {
        let scope = model.entities[entity(&model, name)]
            .scope
            .unwrap_or_else(|| panic!("{name} owns no scope"));
        assert_eq!(names(&model, scope)[0], spelled, "{name}");
        let symbol = model.lookup_local(scope, spelled).expect("a symbol");
        assert_eq!(model.symbols[symbol].kind, SymbolKind::TypeParameter, "{name}");
    }
    // It is visible in the parameters, the output, and the body of the declaration, and
    // it is not one of FnEntity.parameters.
    let take = entity(&model, "take");
    let t = model.type_parameters(take)[0];
    let parameters: Vec<&str> = model.entities[take]
        .parameters()
        .iter()
        .map(|&s| model.symbols[s].name.as_str())
        .collect();
    assert_eq!(parameters, ["p"]);
    let p = model.symbols[model.entities[take].parameters()[0]].entity;
    assert_eq!(model.entities[p].ty, Some(TypeRef::Entity(t)));
    assert_eq!(model.entities[take].output(), Some(&TypeRef::Entity(t)));
    // Its kind data is the one a parameter is seen through.
    assert_eq!(model.entities[t].kind, EntityKind::Parameter);
    // A type parameter with no extends clause has no type of its own.
    for name in ["held", "Pair"] {
        let parameter = model.type_parameters(entity(&model, name))[0];
        assert_eq!(model.entities[parameter].ty, None, "{name}");
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:fddf44919f484fdf1157ac285fae11ba183fdc5f899939106c66008b5648600b
#[test]
fn a_type_parameter_has_no_definition_traits_or_criteria() {
    let model = bind_one(
        "d Item {} trait marked {} d Box<T extends Item = string, U> is marked: `a box` {}",
    );
    assert_clean(&model);
    let boxed = entity(&model, "Box");
    // The declaration itself has the definition, the trait, and any criteria.
    assert_eq!(model.entities[boxed].definition.as_deref(), Some("a box"));
    assert_eq!(model.entities[boxed].traits.len(), 1);
    for &parameter in &model.type_parameters(boxed) {
        assert_eq!(model.entities[parameter].definition, None);
        assert!(model.entities[parameter].traits.is_empty());
        assert!(model.entities[parameter].acceptance_criteria.is_empty());
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:de2ee272797ce7725fdbaf5ab6d10caca2ea348db8e96a5e734f5a52d980695f
// @lfy def/model/main.lfy:bind#bind:bind:2dfb72545f7c3d290642c5da47ce2e5e122656af1ab30a73baeefd59d6e4d820
#[test]
fn object_key_declares_only_in_an_enum() {
    let model = bind_one("enum Color { red = 'r', blue = 'b' } const o = { key = 1 };");
    assert_clean(&model);
    let color = entity(&model, "Color");
    let members = model.members(color);
    assert_eq!(
        names(&model, model.entities[color].scope.unwrap()),
        ["red", "blue"]
    );
    for m in &members {
        assert_eq!(model.symbols[*m].kind, SymbolKind::EnumMember);
        assert_eq!(
            model.entities[model.symbols[*m].entity].kind,
            EntityKind::EnumMember
        );
    }
    let red = node(&model, 0, E::ObjectKey, "red = 'r'");
    assert_eq!(model.symbol_of(red), Some(members[0]));
    let key = node(&model, 0, E::ObjectKey, "key = 1");
    assert_eq!(model.symbol_of(key), None);
    assert!(model.symbols.iter().all(|s| s.name != "key"));
}

// @lfy def/model/main.lfy:bind#bind:bind:0e756633c85cba82bf7a5348e64ed3d981aaff25b46e0d41245a2e8e0a9e26c7
#[test]
fn member_statement_declares_a_member() {
    let model = bind_one("d D { $x: `d` = string; $ys = number[]; }");
    assert_clean(&model);
    let d = entity(&model, "D");
    assert_eq!(names(&model, model.entities[d].scope.unwrap()), ["x", "ys"]);
    let x = member(&model, d, "x");
    assert_eq!(model.symbols[x].kind, SymbolKind::Member);
    let x_entity = model.symbols[x].entity;
    assert_eq!(model.entities[x_entity].kind, EntityKind::Member);
    assert_eq!(model.entities[x_entity].identifier.as_deref(), Some("x"));
    assert_eq!(model.entities[x_entity].definition.as_deref(), Some("d"));
    assert_eq!(
        model.entities[x_entity].ty,
        Some(TypeRef::Primitive("string"))
    );
    // The statement and the Current that spells the name both declare it; the Current also
    // makes its Usage, resolved to the symbol it declares.
    let statement = node(&model, 0, S::ExpressionStatement, "$x: `d` = string;");
    assert_eq!(model.symbol_of(statement), Some(x));
    let current = node(&model, 0, E::Current, "$x");
    assert_eq!(model.symbol_of(current), Some(x));
    assert_eq!(usage(&model, current).symbol, Some(x));
    let ys = model.symbols[member(&model, d, "ys")].entity;
    assert_eq!(model.entities[ys].definition, None);
    assert_eq!(
        model.entities[ys].ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("number"))))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:583d8bd2aacbcdee71c5cda8c83518a8d2ab447460f739666f8df8093b6fc7a5
// @lfy def/model/main.lfy:bind#bind:bind:5e66df798807d759740c0bde7780004559259dcb44c7a9fbe44fe762a615c470
// @lfy def/model/main.lfy:bind#bind:bind:5dafa3a6e52ad501215f54b081dab8720e0d83c56fb7277975f861591af27874
#[test]
fn scope_current_entity() {
    let model = bind_one("d A {} with A { const x = 1; } for (const i in [1]) { const j = i; }");
    assert_clean(&model);
    let a = entity(&model, "A");
    // A scope that belongs to a declaration has the declared entity as its current.
    assert_eq!(
        model.scopes[model.entities[a].scope.expect("a data owns a scope")].current,
        a
    );
    let with = model
        .scope_of(node(&model, 0, S::With, "with A { const x = 1; }"))
        .expect("a With owns a scope");
    assert_eq!(model.scopes[with].current, a);
    let with_block = model
        .scope_of(node(&model, 0, S::Block, "{ const x = 1; }"))
        .unwrap();
    assert_eq!(model.scopes[with_block].parent, Some(with));
    assert_eq!(model.scopes[with_block].current, a);
    let loop_scope = model
        .scope_of(node(
            &model,
            0,
            S::For,
            "for (const i in [1]) { const j = i; }",
        ))
        .unwrap();
    assert_eq!(model.scopes[loop_scope].current, model.file_entities[0]);
    let loop_block = model
        .scope_of(node(&model, 0, S::Block, "{ const j = i; }"))
        .unwrap();
    assert_eq!(model.scopes[loop_block].current, model.file_entities[0]);
}

// ---- Use ----------------------------------------------------------------------------

// @lfy def/model/main.lfy:bind#bind:bind:3daaf7c3051f43abb7b4c2664e4c94383f3258ebf028136f683006034540f88a
// @lfy def/model/main.lfy:bind#bind:bind:81cd673c66e8b5ef0d81262e4dd6bcf40b110f884bf038212940017c42f01483
#[test]
fn use_without_as_imports_own_symbols_only() {
    let c = source("c.lfy", "d C {}", &[]);
    let b = source("b.lfy", "use \"./c\"; d B {}", &[Some("c.lfy")]);
    let a = source(
        "a.lfy",
        "use \"./b\"; const x = B; const y = C;",
        &[Some("b.lfy")],
    );
    let model = bind(vec![c, b, a]);
    let (b_file, a_file) = (model.file("b.lfy").unwrap(), model.file("a.lfy").unwrap());
    let b_symbol = file_symbol(&model, b_file, "B");
    assert_eq!(model.scopes[model.file_scopes[a_file]].imports, [b_symbol]);
    assert_eq!(names(&model, model.file_scopes[a_file]), ["x", "y", "B"]);
    assert_eq!(
        usage(&model, node(&model, a_file, E::Name, "B")).symbol,
        Some(b_symbol)
    );
    assert_eq!(model.lookup(model.file_scopes[a_file], "B"), Some(b_symbol));
    let c_use = node(&model, a_file, E::Name, "C");
    assert_eq!(usage(&model, c_use).symbol, None);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, c_use);
    // b.lfy itself sees C.
    assert_eq!(
        model.lookup(model.file_scopes[b_file], "C"),
        Some(file_symbol(&model, model.file("c.lfy").unwrap(), "C"))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:85e0b777d37918aa64dd528397bd3f5891099278b7d9d4b0c17cd037bcc845ff
// @lfy def/model/main.lfy:bind#bind:bind:ff80c9e9250025190f5b25a2ac7ee8d46d800e996782e73367a03a40cab77dfb
#[test]
fn use_with_as_resolves_members_in_the_module() {
    let b = source("b.lfy", "d B {} trait tb {}", &[]);
    let a = source(
        "a.lfy",
        "use \"./b\" as M; const x = M.B; d A is M.tb {} for (const e in M) {}",
        &[Some("b.lfy")],
    );
    let model = bind(vec![b, a]);
    assert_clean(&model);
    let (b_file, a_file) = (model.file("b.lfy").unwrap(), model.file("a.lfy").unwrap());
    let module = file_symbol(&model, a_file, "M");
    assert_eq!(model.symbols[module].kind, SymbolKind::Module);
    assert_eq!(model.symbols[module].entity, model.file_entities[b_file]);
    assert!(model.scopes[model.file_scopes[a_file]].imports.is_empty());
    let member_use = usage(&model, node(&model, a_file, E::Member, "M.B"));
    assert_eq!(member_use.name.as_deref(), Some("B"));
    assert_eq!(member_use.symbol, Some(file_symbol(&model, b_file, "B")));
    let trait_use = usage(&model, node(&model, a_file, E::TraitUse, "M.tb"));
    assert_eq!(trait_use.symbol, Some(file_symbol(&model, b_file, "tb")));
    let a = model.symbols[file_symbol(&model, a_file, "A")].entity;
    assert_eq!(
        entities_of(
            &model,
            model.symbols[file_symbol(&model, b_file, "tb")].entity
        ),
        vec![a]
    );
    // A For over the module visits its symbols' entities.
    let e = model
        .lookup_local(
            model
                .scope_of(node(&model, a_file, S::For, "for (const e in M) {}"))
                .unwrap(),
            "e",
        )
        .unwrap();
    assert_eq!(model.symbols[e].kind, SymbolKind::LoopVariable);
}

// @lfy def/model/main.lfy:bind#bind:bind:659c28f53c8aced2b9b367a2d5ef5a82f73561138a260c1d0dd6ff121bc69857
// @lfy def/model/main.lfy:bind#bind:bind:3d1e39f70281f06dd97d7419a6c202f8938221160fe42d7f258ec7de6438c798
#[test]
fn an_in_loop_over_a_module_visits_entities_and_an_of_loop_names() {
    let module = || source("b.lfy", "d B1: `one` {} d B2: `two` {}", &[]);
    let entities = source(
        "a.lfy",
        "use \"./b\" as M; d A { for (const item in M) { where (`s`) -> `{{item@definition}}`; } }",
        &[Some("b.lfy")],
    );
    let model = bind(vec![module(), entities]);
    assert_clean(&model);
    let a_file = model.file("a.lfy").unwrap();
    let a = model.symbols[file_symbol(&model, a_file, "A")].entity;
    let behaviors: Vec<Option<Vec<String>>> = criteria_texts(&model, a)
        .into_iter()
        .map(|(_, behavior, _)| behavior)
        .collect();
    assert_eq!(
        behaviors,
        [
            Some(vec!["one".to_string()]),
            Some(vec!["two".to_string()]),
        ]
    );
    let names_of = source(
        "a.lfy",
        "use \"./b\" as M; d A { for (const name of M) { where (`{{name}}`) -> `b`; } }",
        &[Some("b.lfy")],
    );
    let model = bind(vec![module(), names_of]);
    assert_clean(&model);
    let a_file = model.file("a.lfy").unwrap();
    let a = model.symbols[file_symbol(&model, a_file, "A")].entity;
    let situations: Vec<Option<Vec<String>>> = criteria_texts(&model, a)
        .into_iter()
        .map(|(situation, _, _)| situation)
        .collect();
    assert_eq!(
        situations,
        [
            Some(vec!["B1".to_string()]),
            Some(vec!["B2".to_string()]),
        ]
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:10a6088c6ab81cb4ca33f996b33bb4781657309f2463c5fb93876dec10a950d4
#[test]
fn a_from_loop_over_a_module_visits_names_and_entities() {
    let b = source("b.lfy", "d B1: `one` {} d B2: `two` {}", &[]);
    let a = source(
        "a.lfy",
        "use \"./b\" as M; d A { for (const name, item from M) { where (`{{name}}`) -> `{{item@definition}}`; } }",
        &[Some("b.lfy")],
    );
    let model = bind(vec![b, a]);
    assert_clean(&model);
    let a_file = model.file("a.lfy").unwrap();
    let a = model.symbols[file_symbol(&model, a_file, "A")].entity;
    assert_eq!(
        criteria_texts(&model, a),
        [
            (
                Some(vec!["B1".to_string()]),
                Some(vec!["one".to_string()]),
                a
            ),
            (
                Some(vec!["B2".to_string()]),
                Some(vec!["two".to_string()]),
                a
            ),
        ]
    );
    // Both names are loop variables of the For's scope.
    let for_node = node(
        &model,
        a_file,
        S::For,
        "for (const name, item from M) { where (`{{name}}`) -> `{{item@definition}}`; }",
    );
    let for_scope = model.scope_of(for_node).expect("a For owns a scope");
    assert_eq!(names(&model, for_scope), ["name", "item"]);
}

// @lfy def/model/main.lfy:bind#bind:bind:b05e01c96fecdd7752fc78c1300732e0edb54106c8e363777a9955d769e5549e
#[test]
fn use_of_nothing_imports_nothing() {
    let model = bind(vec![source(
        "a.lfy",
        "use \"./missing\"; const x = 1;",
        &[None],
    )]);
    assert_clean(&model);
    assert!(model.scopes[model.file_scopes[0]].imports.is_empty());
    assert_eq!(names(&model, model.file_scopes[0]), ["x"]);
}
