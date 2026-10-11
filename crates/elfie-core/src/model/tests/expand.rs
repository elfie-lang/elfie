// @lfy def/model/main.lfy:bind
//! Tests of the expand pass: trait applications, bodies, and `ace`.

use super::*;

// @lfy def/model/main.lfy:bind
#[test]
fn apply_call_applies_the_trait() {
    let model = bind_one(
        "trait X(n: number) {} d Y {} X.apply(Y, 3); \
         trait rule(syntax: string) {} trait alternationList(...items: (is rule)[]) {} \
         d P is rule('p') {} d Q is rule('q') {} d Alt is alternationList(P, Q) {} \
         trait marker {} marker.apply(Alt);",
    );
    // No prelude declares Trait here, so apply itself is found nowhere and says so; the
    // call applies the trait all the same.
    // @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
    assert_eq!(
        problems(&model),
        vec![
            "X.apply: X has no member apply",
            "marker.apply: marker has no member apply",
        ]
    );
    let (x, y) = (entity(&model, "X"), entity(&model, "Y"));
    assert_eq!(model.entities[y].traits.len(), 1);
    let applied = &model.entities[y].traits[0];
    assert_eq!(applied.entity, x);
    assert_eq!(applied.values, vec![Value::Number(3.0)]);
    assert_eq!(applied.arguments.len(), 1);
    assert_eq!(model.raw(applied.arguments[0]).trim(), "3");
    assert_eq!(
        applied.source,
        AppliedSource::Apply(node(&model, 0, E::Call, "X.apply(Y, 3)"))
    );
    assert_eq!(entities_of(&model, x), vec![y]);
    // The alternationList: every item, not the list itself.
    let (marker, alt, p, q) = (
        entity(&model, "marker"),
        entity(&model, "Alt"),
        entity(&model, "P"),
        entity(&model, "Q"),
    );
    assert_eq!(entities_of(&model, marker), vec![p, q]);
    assert!(model.entities[p].has_trait(marker));
    assert!(model.entities[q].has_trait(marker));
    assert!(!model.entities[alt].has_trait(marker));
}

// @lfy def/model/main.lfy:bind
#[test]
fn extends_chain_applies_every_base() {
    let model = bind_one(
        "trait a(s: string) { $v: `the value` = string; .v = s; } trait b(t: string) extends a(`x{{t}}y`) {} d C is b('m') {}",
    );
    assert_clean(&model);
    let (a, b, c) = (
        entity(&model, "a"),
        entity(&model, "b"),
        entity(&model, "C"),
    );
    assert_eq!(
        model.entities[c].value("v"),
        Some(&Value::String("xmy".to_string()))
    );
    let traits: Vec<(EntityId, Vec<Value>)> = model.entities[c]
        .traits
        .iter()
        .map(|t| (t.entity, t.values.clone()))
        .collect();
    assert_eq!(
        traits,
        [
            (b, vec![Value::String("m".into())]),
            (a, vec![Value::String("xmy".into())])
        ]
    );
    assert_eq!(
        model.entities[c].traits[1].source,
        AppliedSource::Inherited(0)
    );
    assert_eq!(entities_of(&model, a), vec![c]);
    assert_eq!(model.entities[a].extenders(), &[b]);
}

// @lfy def/model/main.lfy:bind#bind:bind:972725faf5a2897534a0e4be77c67b10e2cce3499191a51d3ab27e1bbe14d1ae
#[test]
fn applied_trait_body_lands_on_the_receiver() {
    let model = bind_one(
        "trait t(n: number) { $m: `member` = string; $k: `the value` = number; .k = n; @acceptanceCriteria.add({ behavior = `B {{n}}` }); where (`S`) -> `W`; } \
         d A is t(2) { @acceptanceCriteria.add({ behavior = `own` }); }",
    );
    assert_clean(&model);
    let (t, a) = (entity(&model, "t"), entity(&model, "A"));
    // Each member declared in the trait's body joins the entity's scope.
    let m = member(&model, a, "m");
    assert_eq!(model.symbols[m].scope, model.entities[a].scope.unwrap());
    assert_eq!(
        model.symbols[m].entity,
        model.symbols[member(&model, t, "m")].entity
    );
    // The value setters are evaluated with the parameters bound to the arguments.
    assert_eq!(model.entities[a].value("k"), Some(&Value::Number(2.0)));
    // Own criteria first, then the trait's, with the trait as contributor.
    assert_eq!(
        criteria_texts(&model, a),
        [
            (None, Some(vec!["own".to_string()]), a),
            (None, Some(vec!["B 2".to_string()]), t),
            (Some(vec!["S".to_string()]), Some(vec!["W".to_string()]), t),
        ]
    );
}

// @lfy def/model/main.lfy:bind
#[test]
fn the_most_recently_applied_trait_declares_the_member() {
    let model = bind_one(
        "trait t1 { $m: `first` = string; } trait t2 { $m: `second` = number; } d A is t1, t2 {}",
    );
    assert_clean(&model);
    let (t1, t2, a) = (
        entity(&model, "t1"),
        entity(&model, "t2"),
        entity(&model, "A"),
    );
    assert_eq!(names(&model, model.entities[a].scope.unwrap()), ["m"]);
    let m = model.symbols[member(&model, a, "m")].entity;
    assert_eq!(m, model.symbols[member(&model, t2, "m")].entity);
    assert_ne!(m, model.symbols[member(&model, t1, "m")].entity);
    assert_eq!(model.entities[m].definition.as_deref(), Some("second"));
    assert_eq!(model.entities[m].ty, Some(TypeRef::Primitive("number")));
    assert_eq!(
        model.entities[a]
            .traits
            .iter()
            .map(|t| t.entity)
            .collect::<Vec<_>>(),
        [t1, t2]
    );
    // An extending trait is applied after the traits it extends, so it wins too.
    let model = bind_one(
        "trait base { $m: `base` = string; } trait over extends base { $m: `over` = number; } d B is over {}",
    );
    assert_clean(&model);
    let b = entity(&model, "B");
    let m = model.symbols[member(&model, b, "m")].entity;
    assert_eq!(model.entities[m].definition.as_deref(), Some("over"));
}

// @lfy def/model/main.lfy:bind#bind:bind:c1bf94ef6ed00fca9ed870486ad290e18a6640d029d6830b1b8b55b921ffc125
// @lfy def/model/main.lfy:bind#bind:bind:bda572541abe6b3dd683ffb7f7c5782bdf7382a98d9a342a3b8d72558c0dc03f
#[test]
fn a_child_scope_shadows_the_parent_without_a_problem() {
    let model = bind_one("const n = 1; d A { const n = 2; const m = n; }");
    assert_clean(&model);
    let outer = file_symbol(&model, 0, "n");
    let block = model
        .scope_of(node(&model, 0, S::Block, "{ const n = 2; const m = n; }"))
        .expect("a Block owns a scope");
    assert_eq!(names(&model, block), ["n", "m"]);
    let inner = model
        .lookup_local(block, "n")
        .expect("the child scope declares n");
    assert_ne!(inner, outer);
    // The name is visible in the child scope, where it hides the parent's.
    assert_eq!(model.lookup(block, "n"), Some(inner));
    assert_eq!(model.lookup(model.file_scopes[0], "n"), Some(outer));
    assert_eq!(
        usage(&model, node(&model, 0, E::Name, "n")).symbol,
        Some(inner)
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:e30b7230cf90b227b4300b5748895116c69879d10c31441e5e03a08f27cca6cc
#[test]
fn an_ace_statement_runs_in_the_expand_pass() {
    let model = bind_with_prelude("trait t {} d A {} ace t.apply(A);");
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let t = model.symbols[file_symbol(&model, program, "t")].entity;
    let a = model.symbols[file_symbol(&model, program, "A")].entity;
    assert!(model.entities[a].has_trait(t));
    assert_eq!(entities_of(&model, t), vec![a]);
}

// @lfy def/model/main.lfy:bind#bind:bind:e80995ebd6bedeb0e22990364e986ca70be730d1538f8b8ac004e3c863e88d56
#[test]
fn trait_entities_in_file_then_application_order() {
    let b = source("b.lfy", "trait t {} d B2 is t {} d B1 is t {}", &[]);
    let a = source(
        "a.lfy",
        "use \"./b\"; d A is t {} t.apply(A);",
        &[Some("b.lfy")],
    );
    let model = bind(vec![b, a]);
    // Nothing but the apply these files bind without a prelude, which is found nowhere.
    // @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
    assert_eq!(problems(&model), vec!["t.apply: t has no member apply"]);
    let t = model.symbols[file_symbol(&model, 0, "t")].entity;
    let ids = |name: &str| model.symbols[model.lookup(model.file_scopes[1], name).unwrap()].entity;
    assert_eq!(entities_of(&model, t), vec![ids("B2"), ids("B1"), ids("A")]);
    // Applying t again records a second application on A but not a second entry.
    assert_eq!(model.entities[ids("A")].traits.len(), 2);
}
