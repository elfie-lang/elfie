// @lfy def/model/main.lfy:bind
//! Tests of the resolve pass: what a name, a member, a template, and an accessor resolve
//! to, and the problems a name that resolves nowhere adds.

use super::*;

// @lfy def/model/main.lfy:bind#bind:bind:99a57bc2ff67ce0d744b40d5abbde3870acf40c8165f6cf1b133c60dfd86a027
#[test]
fn dereference_yields_the_symbol() {
    let model = bind_one("d X {} const y = &X;");
    assert_clean(&model);
    let x = file_symbol(&model, 0, "X");
    let deref = node(&model, 0, E::Dereference, "&X");
    let use_of = usage(&model, deref);
    assert_eq!(use_of.layer, Layer::Dereference);
    assert_eq!(use_of.symbol, Some(x));
    assert_eq!(resolve(&model, deref), Some(x));
    assert_eq!(usage(&model, node(&model, 0, E::Name, "X")).symbol, Some(x));
}

// @lfy def/model/main.lfy:bind#bind:bind:07e3b6151afda7d5560da7b993bf6696deec981b1bc4bbf729faed935d554b89
// @lfy def/model/main.lfy:bind#bind:bind:0e0ebc3f4e95b4a0d73136e1a0a343ae561cd0d20a56025c7457b59f3702bf7d
#[test]
fn previous_yields_the_earlier_declaration() {
    let model = bind_one("d X {} const y = 1; const z = ^^; d W { const w = ^^; }");
    let y = file_symbol(&model, 0, "y");
    let previous = node(&model, 0, E::Previous, "^^");
    let use_of = usage(&model, previous);
    assert_eq!(use_of.layer, Layer::Previous);
    assert_eq!(use_of.name, None);
    assert_eq!(use_of.symbol, Some(y));
    // In W's block nothing earlier declares anything: unresolved, one problem.
    let inner = model
        .usages
        .iter()
        .filter(|u| u.layer == Layer::Previous)
        .map(|u| u.symbol)
        .collect::<Vec<_>>();
    assert_eq!(inner, [Some(y), None]);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(
        model.info(model.problems[0].node).rule,
        E::Previous.entity()
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:0b03717c4f0414186634383cb7963ba8e06452ae1dcd7f981374596221a09046
// @lfy def/model/main.lfy:bind#bind:bind:f037c79f1549c04cb4e180ca6df046e1cb92277d231bd88fd9490e30745c532f
#[test]
fn template_reference_resolves_in_scope_or_to_the_rule() {
    let rules = source(
        "r.lfy",
        "trait rule(syntax: string) {} d Foo is rule('x') {}",
        &[],
    );
    let other = source(
        "a.lfy",
        "d X {} const y = `[[X]] and [[Foo]]`; const z = Foo;",
        &[],
    );
    let model = bind(vec![rules, other]);
    let (r_file, a_file) = (model.file("r.lfy").unwrap(), model.file("a.lfy").unwrap());
    let foo = file_symbol(&model, r_file, "Foo");
    let x = file_symbol(&model, a_file, "X");
    let names_in_a: Vec<NodeRef> = (0..model.nodes[a_file].len())
        .map(|index| NodeRef {
            file: a_file,
            index,
        })
        .filter(|&r| model.info(r).rule == E::Name.entity())
        .collect();
    let resolved: Vec<(String, Option<SymbolId>)> = names_in_a
        .iter()
        .filter_map(|&r| {
            model
                .usage_of(r)
                .map(|u| (model.raw(r).trim().to_string(), model.usages[u].symbol))
        })
        .collect();
    assert_eq!(
        resolved,
        [
            ("X".to_string(), Some(x)),
            ("Foo".to_string(), Some(foo)),
            ("Foo".to_string(), None)
        ]
    );
    // Only the bare `Foo` outside the template is a problem.
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, names_in_a[2]);
    assert_eq!(usages_of(&model, foo).len(), 1);
}

// @lfy def/model/main.lfy:bind#bind:bind:0296df6a1500806f0d09ded31855a827561c1a703c7de3a10d6ce695067b48bc
// @lfy def/model/main.lfy:bind#bind:bind:d20244dc3bdb37be74e170d6902fb9e329aa1be9313397e41f2f3b4021507236
#[test]
fn two_rule_entities_with_the_same_identifier_is_a_problem() {
    let a = source(
        "a.lfy",
        "trait rule(syntax: string) {} d Foo is rule(`x`) {}",
        &[],
    );
    let b = source("b.lfy", "use \"./a\"; d Foo is rule(`y`) {}", &[Some("a.lfy")]);
    // A third file, so the template reference is not found by normal scope lookup and
    // falls to the rule fallback, which is ambiguous.
    let c = source("c.lfy", "const t = `[[Foo]]`;", &[]);
    let model = bind(vec![a, b, c]);
    let bfile = model.file("b.lfy").unwrap();
    let cfile = model.file("c.lfy").unwrap();
    let second_foo = model.symbols[file_symbol(&model, bfile, "Foo")].entity;
    let second_node = model.entities[second_foo].node.expect("Foo has a node");
    let reference = node(&model, cfile, E::Name, "Foo");
    assert_eq!(
        problems(&model)
            .into_iter()
            .map(|p| p.contains("a.lfy") || p.contains("more than one rule"))
            .collect::<Vec<_>>(),
        [true, true]
    );
    assert_eq!(model.problems[0].node, second_node);
    assert_eq!(model.problems[1].node, reference);
    assert_eq!(usage(&model, reference).symbol, None);
}

// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
// @lfy def/model/main.lfy:bind#bind:bind:96fe0cf1480dabf1b8ddea3f3c4ae29add39001d68d41f069dc4e8035dce6095
#[test]
fn unknown_context_property_is_a_problem() {
    let model = bind_one("d X { const a = @identifier; const b = @nonsense; const c = X@type; }");
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    let nonsense = node(&model, 0, E::Current, "@nonsense");
    assert_eq!(model.problems[0].node, nonsense);
    assert_eq!(
        model.problems[0].message,
        "nonsense is not a context property"
    );
    let use_of = usage(&model, nonsense);
    assert_eq!(use_of.layer, Layer::Context);
    assert_eq!(use_of.name.as_deref(), Some("nonsense"));
    assert_eq!(use_of.symbol, None);
    assert_eq!(
        usage(&model, node(&model, 0, E::Current, "@identifier")).layer,
        Layer::Context
    );
    assert_eq!(
        usage(&model, node(&model, 0, E::Member, "X@type")).layer,
        Layer::Context
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:88eb2d2cca1fc9e39dfbb1fd77266bdb656d9b5b76c1c8d103d7ea2000ccaa4d
// @lfy def/model/main.lfy:bind#bind:bind:96fe0cf1480dabf1b8ddea3f3c4ae29add39001d68d41f069dc4e8035dce6095
// @lfy def/model/main.lfy:bind#bind:bind:38106a1cd270b7104392d7af5d8562882a29a648334a1b33c02a79d4044e59b6
// @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
#[test]
fn the_kind_data_of_an_entity_gives_its_context_and_function_members() {
    let model = bind_with_prelude(
        "trait t {} d A { $m: `own` = string; } const i = A@identifier; const e = t@entities; \
         const w = A@entities; const o = A.m; const p = t.apply; const l = A.like; \
         const q = t.entities;",
    );
    let file = model.file("a.lfy").unwrap();
    // Entity gives every entity its identifier; Trait gives a trait its entities.
    reads_member(
        &model,
        node(&model, file, E::Member, "A@identifier"),
        "Entity",
        "identifier",
    );
    reads_member(
        &model,
        node(&model, file, E::Member, "t@entities"),
        "Trait",
        "entities",
    );
    // A name of a kind data other than the entity's yields undefined, without a problem.
    let other = usage(&model, node(&model, file, E::Member, "A@entities"));
    assert_eq!(other.layer, Layer::Context);
    assert_eq!(other.symbol, None);
    // The value layer reads the entity's own members first.
    let a = model.symbols[file_symbol(&model, file, "A")].entity;
    assert_eq!(
        usage(&model, node(&model, file, E::Member, "A.m")).symbol,
        Some(member(&model, a, "m"))
    );
    // Then those members of the kind data whose value is a function, whether a reference
    // to a fn declaration, as `Trait.apply` is, or an inline function, as `Entity.like`
    // is.
    // @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
    reads_member(
        &model,
        node(&model, file, E::Member, "t.apply"),
        "Trait",
        "apply",
    );
    reads_member(
        &model,
        node(&model, file, E::Member, "A.like"),
        "Entity",
        "like",
    );
    // A member of the kind data whose value is not a function is not one of them, and,
    // the left side being a trait no more lenient than any other, that is a problem.
    // @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
    let entities = node(&model, file, E::Member, "t.entities");
    assert_eq!(usage(&model, entities).symbol, None);
    assert_eq!(problems(&model), vec!["t.entities: t has no member entities"]);
}

// @lfy def/model/main.lfy:bind#bind:bind:88eb2d2cca1fc9e39dfbb1fd77266bdb656d9b5b76c1c8d103d7ea2000ccaa4d
// @lfy def/model/main.lfy:bind#bind:bind:7469dc0e51b05a45c42ac07d3559b9d5c94720317dde6eaf087ee72618320558
// @lfy def/model/main.lfy:bind#bind:bind:4ab962f1f8c2a853dbcf4a5ed964a4812b80251caabdfe1adf58317d38eb7f86
// @lfy def/model/main.lfy:bind#bind:bind:6bba9856ded82c70f13da7b2aa0fbbc2e366d1b54bc63d982c2c791cffc02b9d
// @lfy def/model/main.lfy:bind#bind:bind:e68fadbfb501e47c9319d0bfb58705fa141e710e9cb1c09e921faabea5f58c85
// @lfy def/model/main.lfy:bind#bind:bind:deb9d97c7e68090dbe776aad57845b8cd480921494d2fa9c5591f79157d44d87
// @lfy def/model/main.lfy:bind#bind:bind:d94eac3ad1d5010a0fecbe38ebe7c578f73ae2916e2873e1893b1d4e85159377
// @lfy def/model/main.lfy:bind#bind:bind:643d4a3ad6bf896608420d4cea390c9ad3caf7b86c19c85c2582278080f7cb41
// @lfy def/model/main.lfy:bind#bind:bind:ef1de1afcf97b2998a6d8056b1c312dbed57285fc5c709fa30732826d39b2479
// @lfy def/model/main.lfy:bind#bind:bind:2a6a30770d94c3859fa6999ffee448372b61158346958fcdfb4448bdb53f5f3a
// @lfy def/model/main.lfy:bind#bind:bind:96fe0cf1480dabf1b8ddea3f3c4ae29add39001d68d41f069dc4e8035dce6095
#[test]
fn the_kind_data_of_an_entity_is_the_prelude_data_for_what_it_is() {
    let b = source("b.lfy", "d B {}", &[]);
    let a = source(
        "a.lfy",
        "use \"./b\" as M; trait t {} fn f(p: string): `d` => string {} \
         d D { $m: `d` = string; } type T { k = string, } enum E { r = 'r', } \
         const c1 = t@entities; const c2 = f@parameters; const c3 = D@isData; \
         const c4 = T@isType; const c5 = E@isEnum; const c6 = D@identifier; \
         with D$m { const c7 = @isMember; } with f$p { const c8 = @isParameter; }",
        &[Some("b.lfy")],
    );
    let mut sources = prelude();
    sources.push(b);
    sources.push(a);
    let model = bind(sources);
    assert_clean(&model);
    let file = model.file("a.lfy").unwrap();
    for (raw, data, name) in [
        ("t@entities", "Trait", "entities"),
        ("f@parameters", "Function", "parameters"),
        ("D@isData", "Data", "isData"),
        ("T@isType", "Type", "isType"),
        ("E@isEnum", "Enum", "isEnum"),
        // Every entity is seen through Entity too, whatever else it is.
        ("D@identifier", "Entity", "identifier"),
    ] {
        reads_member(&model, node(&model, file, E::Member, raw), data, name);
    }
    // A variable and a module carry Variable and Module the same way, but a name bound to
    // one yields its value rather than its entity, so neither is read through a name.
    let _ = (prelude_data(&model, "Variable"), prelude_data(&model, "Module"));
    // A member and a parameter are reached as the current entity of a With.
    for (raw, data, name) in [
        ("@isMember", "Member", "isMember"),
        ("@isParameter", "Parameter", "isParameter"),
    ] {
        reads_member(&model, node(&model, file, E::Current, raw), data, name);
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:04c22787733ff3defc0c5a6b52add93b83e7c0630dafc6c75ee0a897057f1463
#[test]
fn a_list_seen_with_one_type_argument_is_seen_through_array() {
    let model = bind_with_prelude(
        "const l = List<string>; const a = l@itemType; const b = l@identifier; \
         const c = List@itemType;",
    );
    let file = model.file("a.lfy").unwrap();
    // This prelude's List lists no type parameter, so its one argument is an arity
    // problem, and the list is seen with the argument as written all the same.
    assert_eq!(problems(&model), vec!["List<string>: 1 type arguments, but List lists 0 type parameters"]);
    // The model spells List seen with one argument as a list type, as it spells `T[]`.
    let l = model.symbols[file_symbol(&model, file, "l")].entity;
    assert_eq!(
        model.entities[l].ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("string"))))
    );
    reads_member(
        &model,
        node(&model, file, E::Member, "l@itemType"),
        "Array",
        "itemType",
    );
    reads_member(
        &model,
        node(&model, file, E::Member, "l@identifier"),
        "Entity",
        "identifier",
    );
    // List seen with no argument is a data, not a list seen with its item type.
    let bare = usage(&model, node(&model, file, E::Member, "List@itemType"));
    assert_eq!(bare.symbol, None);
}

// @lfy def/model/main.lfy:bind#bind:bind:a5c1bc569ddb92f8dd361e4827d274e8148ec9c79e5f47f9178da3acd27018eb
// @lfy def/model/main.lfy:bind#bind:bind:13a7ef34563641503b4fd1a9fdcaa38cbb0f8bb47dc497f235d53d63b8e451da
// @lfy def/model/main.lfy:bind#bind:bind:8615b161cda09a3689fd6b2a04cf2bf8b34b863a2664fe21060d5e9cdad694ea
// @lfy def/model/main.lfy:bind#bind:bind:ee99077d54a77f22a5f264d53d5d58e5e0c6b863c79f255573056c5e7d995703
// @lfy def/model/main.lfy:bind#bind:bind:60a286d3f7df83f47847fa8f557446c01e8bb39dd770556bedea6afd56d5fc3b
// @lfy def/model/main.lfy:bind#bind:bind:5f2d132c545382457130c6e190c71974d985527b87f603c95e936bb60ad7edf0
// @lfy def/model/main.lfy:bind#bind:bind:5b5df5fcbe0bfad1f4242837461d9e5756207c0a361e81048345a263528d5740
// @lfy def/model/main.lfy:bind#bind:bind:f3c216ffbdb497c52484ec15de756394007cc9b0e655697d7a2d5d8e241857de
// @lfy def/model/main.lfy:bind#bind:bind:970d0f64685c6d21821fc7b739fbd15e65769f02adef97284ae744e20be0e6a1
// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
#[test]
fn a_value_resolves_the_members_of_its_base_data() {
    let model = bind_with_prelude(
        "const s = 'text'; const n = s.length; const t = s.trim(); const xs: string[] = []; \
         const c = xs.length; const o = { a = 1 }; const k = o.keys(); const v = o.a; \
         const p = `text`; const r = p.text(); const i = 2; const f = i.floor(); \
         const y = true; const z = y.not(); const g = (x: number) => x; \
         const q = g.parameters;",
    );
    assert_clean(&model);
    let file = model.file("a.lfy").unwrap();
    let read = |raw: &str, data: &str, name: &str| {
        reads_member(&model, node(&model, file, E::Member, raw), data, name);
    };
    read("s.length", "String", "length");
    read("s.trim", "String", "trim");
    read("xs.length", "List", "length");
    read("o.keys", "Object", "keys");
    read("p.text", "Template", "text");
    read("i.floor", "Number", "floor");
    read("y.not", "Boolean", "not");
    read("g.parameters", "Function", "parameters");
    // An object's keys come first, and a key declares nothing, so o.a has no symbol.
    assert_eq!(usage(&model, node(&model, file, E::Member, "o.a")).symbol, None);
    // A name the base data does not declare is a problem.
    let model = bind_with_prelude("const s = 'text'; const n = s.nope;");
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(
        model.problems[0].node,
        node(&model, model.file("a.lfy").unwrap(), E::Member, "s.nope")
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:13a7ef34563641503b4fd1a9fdcaa38cbb0f8bb47dc497f235d53d63b8e451da
#[test]
fn an_objects_keys_come_before_the_members_of_object() {
    let model = bind_with_prelude(
        "const o = { keys = 1, a = 2 }; const k = o.keys; const v = o.a; const m = o.nope; \
         const rows = [{ kind = 1 }, { data = 2 }]; \
         for (const row in rows) { const x = row.kind; const y = row.data; const z = row.other; } \
         const p = { a = 1 }; const pk = p.keys;",
    );
    let file = model.file("a.lfy").unwrap();
    // A key that shares a name with a member of Object is the key, which declares nothing.
    assert_eq!(usage(&model, node(&model, file, E::Member, "o.keys")).symbol, None);
    assert_eq!(usage(&model, node(&model, file, E::Member, "o.a")).symbol, None);
    // Without such a key, the member of Object.
    reads_member(&model, node(&model, file, E::Member, "p.keys"), "Object", "keys");
    // The items of a list of objects have the keys of every item.
    for raw in ["row.kind", "row.data"] {
        assert_eq!(usage(&model, node(&model, file, E::Member, raw)).symbol, None);
    }
    // A name neither a key nor a member is a problem.
    assert_eq!(
        problems(&model),
        vec![
            "o.nope: the object has no key or member nope",
            "row.other: the object has no key or member other",
        ]
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
#[test]
fn a_member_of_a_value_nothing_is_known_of_is_a_problem() {
    let model = bind_with_prelude(
        "const u: string | number = 1; const a = u.nope; \
         const n: object = 1; const b = n.nope; \
         const m: string | undefined = 'x'; const c = m.length;",
    );
    let file = model.file("a.lfy").unwrap();
    // The left side has a symbol, so finding nothing on it is a problem, whatever it is.
    assert_eq!(
        problems(&model),
        vec![
            "u.nope: nothing known of the value has a member nope",
            "n.nope: Object has no member nope",
        ]
    );
    // A value that may be undefined reads as the one other type.
    reads_member(&model, node(&model, file, E::Member, "m.length"), "String", "length");
}

// However long the chain.
// @lfy def/model/main.lfy:bind#bind:bind:154744cba7a0648bfbee408864406e16fb0a9567bd411590ebb2634557f61d87
#[test]
fn add_on_the_criteria_of_a_context_is_the_binders_own_call() {
    let model = bind_with_prelude(
        "d A { @acceptanceCriteria.add({ behavior = `one` }).add({ behavior = `two` }); }",
    );
    assert_clean(&model);
    let file = model.file("a.lfy").unwrap();
    for add in nodes(&model, file, E::Member, "@acceptanceCriteria.add") {
        assert_eq!(usage(&model, add).symbol, None);
    }
    let a = model.symbols[file_symbol(&model, file, "A")].entity;
    assert_eq!(model.entities[a].acceptance_criteria.len(), 2);
}

// Directly, and on the list an earlier add yields.
// @lfy def/model/main.lfy:bind#bind:bind:154744cba7a0648bfbee408864406e16fb0a9567bd411590ebb2634557f61d87
#[test]
fn add_on_the_other_lists_of_a_context_is_the_binders_own_call() {
    let model = bind_with_prelude(
        "d A { @knowledge.add(`k`); @commands.add(`c`).add(`d`); @targets.add(A); }",
    );
    let file = model.file("a.lfy").unwrap();
    for raw in [
        "@knowledge.add",
        "@commands.add",
        "@commands.add(`c`).add",
        "@targets.add",
    ] {
        let add = node(&model, file, E::Member, raw);
        assert_eq!(usage(&model, add).symbol, None, "{raw} resolved to a member");
        assert!(
            !model.problems.iter().any(|problem| problem.node == add),
            "{raw} is a problem: {:?}",
            problems(&model)
        );
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
// @lfy def/model/main.lfy:bind#bind:bind:092cce29356879ba213cc57e7d617f3b488aa91d35f1da0d98ac951bceae5ce4
// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
#[test]
fn member_on_data_resolves_to_its_member() {
    let model =
        bind_one("d D { $m: `x` = string; } const v = D.m; const s = D$m; const w = D.nope;");
    let d = entity(&model, "D");
    let m = member(&model, d, "m");
    let value = usage(&model, node(&model, 0, E::Member, "D.m"));
    assert_eq!(
        (value.layer, value.name.as_deref(), value.symbol),
        (Layer::Value, Some("m"), Some(m))
    );
    let scope = usage(&model, node(&model, 0, E::Member, "D$m"));
    assert_eq!(
        (scope.layer, scope.name.as_deref(), scope.symbol),
        (Layer::Scope, Some("m"), Some(m))
    );
    let nope = node(&model, 0, E::Member, "D.nope");
    assert_eq!(usage(&model, nope).symbol, None);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, nope);
    // The two reads and the Current that declares the member.
    assert_eq!(usages_of(&model, m).len(), 3);
}

// @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
#[test]
fn the_optional_value_accessor_reads_the_value_layer() {
    let model = bind_one("d D { $m: `x` = string; } const v = D?.m;");
    assert_clean(&model);
    let read = usage(&model, node(&model, 0, E::Member, "D?.m"));
    assert_eq!(read.layer, Layer::Value);
    assert_eq!(read.name.as_deref(), Some("m"));
    assert_eq!(read.symbol, Some(member(&model, entity(&model, "D"), "m")));
}

// @lfy def/model/main.lfy:bind#bind:bind:1f1de1c02f09b3bb5636fac9080f9daf36806ec9284ab40dd72c9325b3a67b39
#[test]
fn the_scope_accessor_with_no_name_yields_the_entitys_scope() {
    let model = bind_one(
        "d Scope {} trait t { $s: `the scope of A` = Scope; .s = A$; } \
         d A is t { $m: `x` = string; }",
    );
    assert_clean(&model);
    let a = entity(&model, "A");
    assert_eq!(
        model.entities[a].value("s"),
        Some(&Value::Scope(
            model.entities[a].scope.expect("a data owns a scope")
        ))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:2a5068f916f94485b682af3154cd51a8d3c0343e4c82670f3036ec3d2c890c5c
// @lfy def/model/main.lfy:bind#bind:bind:997e81cefd2e242372fae5b5fb01542d9a18bd3ccc915f34d1c6f2d310ef7c15
#[test]
fn parent_scope_accessor_yields_the_containing_scope() {
    let model = bind_one(
        "const v = 1; d Scope {} trait t { $p: `the scope around A` = Scope; \
         $r: `the scope around a variable` = Scope; .p = A$&; .r = (&v)$&; } d A is t {}",
    );
    assert_clean(&model);
    let a = entity(&model, "A");
    assert_eq!(
        model.entities[a].value("p"),
        Some(&Value::Scope(model.file_scopes[0]))
    );
    // A variable owns no scope, so there is no containing scope to yield.
    assert_eq!(model.entities[a].value("r"), Some(&Value::Undefined));
    assert_eq!(
        usage(&model, node(&model, 0, E::Member, "A$&")).layer,
        Layer::Parent
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
#[test]
fn a_member_of_an_unresolved_name_is_not_a_second_problem() {
    let model = bind_one("const y = z.deeper;");
    let z = node(&model, 0, E::Name, "z");
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, z);
    let deeper = node(&model, 0, E::Member, "z.deeper");
    assert_eq!(usage(&model, deeper).name.as_deref(), Some("deeper"));
    assert_eq!(usage(&model, deeper).symbol, None);
}

// What a predicate reads includes the members of the traits the entities carrying it
// carry, so a name one of them has is found.
// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
// @lfy def/model/main.lfy:bind#bind:bind:ff80c9e9250025190f5b25a2ac7ee8d46d800e996782e73367a03a40cab77dfb
#[test]
fn nothing_found_for_a_name_is_a_problem_whatever_the_left_side_is() {
    let mut sources = prelude();
    sources.push(source("m.lfy", "d Far {}", &[]));
    sources.push(source(
        "a.lfy",
        "use \"./m\" as M; trait t {} trait u { $only: `on u` = string; } d A is t, u {} \
         const v = 'text'; const p = t.nope; const q = A.nope; const r = M.nope; \
         const w = v.nope; const x: is t = A; const ok = x.only; const bad = x.nope;",
        &[Some("m.lfy")],
    ));
    let model = bind(sources);
    let file = model.file("a.lfy").unwrap();
    // A name a carrier of the trait has is found: `x` is one of the entities carrying t,
    // and A, the one of them, carries u as well, so the member u declares is read.
    // @lfy def/model/main.lfy:bind
    let u = model.symbols[file_symbol(&model, file, "u")].entity;
    assert_eq!(
        usage(&model, node(&model, file, E::Member, "x.only")).symbol,
        Some(member(&model, u, "only"))
    );
    // Every other left side is read strictly, a predicate no less than a data.
    for raw in ["t.nope", "A.nope", "M.nope", "v.nope", "x.nope"] {
        assert_eq!(
            usage(&model, node(&model, file, E::Member, raw)).symbol,
            None,
            "{raw}"
        );
    }
    assert_eq!(
        problems(&model),
        vec![
            "t.nope: t has no member nope",
            "A.nope: A has no member nope",
            "M.nope: the module declares no nope",
            "v.nope: String has no member nope",
            "x.nope: t has no member nope",
        ]
    );
    // With no prelude bound the kind data of every entity is empty, so a name only the
    // prelude would give is found nowhere.
    // @lfy def/model/main.lfy:bind
    let model = bind_one("trait t {} d A is t {} const p = t.nope; const q = A.nope;");
    assert_eq!(
        problems(&model),
        vec!["t.nope: t has no member nope", "A.nope: A has no member nope"]
    );
    assert_eq!(
        usage(&model, node(&model, 0, E::Member, "t.nope")).symbol,
        None
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:7b5a1fec626e619b600c3faefaa81db781f8ebdd297cba7893369d95d1f212c7
// @lfy def/model/main.lfy:bind#bind:bind:bda572541abe6b3dd683ffb7f7c5782bdf7382a98d9a342a3b8d72558c0dc03f
#[test]
fn name_resolves_to_the_nearest_scope() {
    let model =
        bind_one("const n = 1; function f(n: string) { const inner = n; } const outer = n;");
    assert_clean(&model);
    let f = entity(&model, "f");
    let parameter = model
        .lookup_local(model.entities[f].scope.unwrap(), "n")
        .unwrap();
    let global = file_symbol(&model, 0, "n");
    let uses: Vec<Option<SymbolId>> = model
        .usages
        .iter()
        .filter(|u| u.name.as_deref() == Some("n"))
        .map(|u| u.symbol)
        .collect();
    assert_eq!(uses, [Some(parameter), Some(global)]);
    assert_eq!(usages_of(&model, global).len(), 1);
}

// @lfy def/model/main.lfy:bind#bind:bind:1ae5ed7b2697e5dba785ff51e02191eb4c4471dbae9009ae9f110e5bddf2cd82
#[test]
fn a_name_of_a_type_parameter_yields_the_parameter() {
    let model = bind_one("d Item {} d Box<T extends Item> { $item: `x` = T; }");
    assert_clean(&model);
    let boxed = entity(&model, "Box");
    let t = model.type_parameters(boxed)[0];
    let read = usage(&model, node(&model, 0, E::Name, "T"));
    assert_eq!(read.layer, Layer::Value);
    assert_eq!(read.symbol.map(|s| model.symbols[s].entity), Some(t));
    // Not Item, which is only what it must extend.
    let item = model.symbols[member(&model, boxed, "item")].entity;
    assert_eq!(model.entities[item].ty, Some(TypeRef::Entity(t)));
    assert_ne!(
        model.entities[item].ty,
        Some(TypeRef::Entity(entity(&model, "Item")))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:d0ff9820f3d48b6bc45af20afec3e2cea3d2a230642bc7e57e4e2bc1adf1828f
// @lfy def/model/main.lfy:bind#bind:bind:8e2d85d92800db4ab851664cdba002e06f2897b88f979a103ccb18275251dc53
#[test]
fn a_member_on_a_type_parameter_reads_what_it_extends() {
    let model = bind_one(
        "d Item { $n: `d` = string; } \
         fn take<T extends Item>(p: T): `d` => string { const q = p.n; } \
         fn loose<U>(p: U): `d` => string { const q = p.n; }",
    );
    assert_clean(&model);
    let reads = nodes(&model, 0, E::Member, "p.n");
    assert_eq!(reads.len(), 2);
    let n = member(&model, entity(&model, "Item"), "n");
    assert_eq!(usage(&model, reads[0]).symbol, Some(n));
    assert_eq!(usage(&model, reads[1]).symbol, None);
}
