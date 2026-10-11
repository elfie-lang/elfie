// @lfy def/model/main.lfy:bind
//! The whole repository binds with no problem, and its model reads as expected.

use super::*;

/// The one entity with this identifier satisfying `keep`.
fn find_entity(model: &Model, name: &str, keep: impl Fn(&Entity) -> bool) -> EntityId {
    let found: Vec<EntityId> = (0..model.entities.len())
        .filter(|&e| {
            model.entities[e].identifier.as_deref() == Some(name) && keep(&model.entities[e])
        })
        .collect();
    assert_eq!(found.len(), 1, "entities named {name}: {found:?}");
    found[0]
}

fn object_field<'a>(value: &'a Value, key: &str) -> &'a Value {
    match value {
        Value::Object(pairs) => {
            &pairs
                .iter()
                .find(|(k, _)| k == key)
                .unwrap_or_else(|| panic!("no field {key}"))
                .1
        }
        other => panic!("not an object: {other:?}"),
    }
}

/// The whole repository binds with zero problems, and its model reads as expected.
// @lfy def/model/main.lfy:bind#bind:bind:6a35ddeeb22226f71b5d6275d3b6e40813ee087972140fe66f3475a200fc219c
// @lfy def/model/main.lfy:bind#bind:bind:83ed1e2d5690dd20563ce4e4aa95a0e2f0367de80d043c91f14cea70e57aecc0
// @lfy def/model/main.lfy:bind#bind:bind:e80995ebd6bedeb0e22990364e986ca70be730d1538f8b8ac004e3c863e88d56
// @lfy def/model/main.lfy:bind#bind:bind:81cd673c66e8b5ef0d81262e4dd6bcf40b110f884bf038212940017c42f01483
// @lfy def/model/main.lfy:bind#bind:bind:0b03717c4f0414186634383cb7963ba8e06452ae1dcd7f981374596221a09046
#[test]
fn repository_binds_without_problems() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let workspace = crate::workspace::load(root);
    let model = &workspace.model;
    assert!(
        workspace.files.len() > 20,
        "{} files",
        workspace.files.len()
    );
    let messages: Vec<String> = workspace
        .problems
        .iter()
        .map(|p| format!("{p:?}"))
        .collect();
    assert!(workspace.problems.is_empty(), "{messages:#?}");
    assert!(model.problems.is_empty(), "{:?}", problems(model));

    // The package elfie is in the program, so its main file is the prelude.
    // @lfy def/model/main.lfy:bind#bind:bind:88eb2d2cca1fc9e39dfbb1fd77266bdb656d9b5b76c1c8d103d7ea2000ccaa4d
    // @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
    let components = model
        .sources
        .iter()
        .position(|source| source.path.ends_with("model/components.lfy"))
        .expect("the definitions declare the model's components");
    assert!(model.scopes[model.file_scopes[components]].parent.is_some());
    let apply = model.symbols[member(
        model,
        find_entity(model, "Trait", |e| e.kind == EntityKind::Data),
        "apply",
    )]
    .entity;
    let applies: Vec<Option<EntityId>> = model
        .usages
        .iter()
        .filter(|usage| usage.node.file == components && usage.name.as_deref() == Some("apply"))
        .map(|usage| usage.symbol.map(|s| model.symbols[s].entity))
        .collect();
    assert!(applies.len() > 10, "{} apply calls", applies.len());
    assert!(applies.iter().all(|&read| read == Some(apply)), "{applies:?}");

    let rule = model
        .trait_named("rule")
        .expect("the grammar declares rule");
    // Space: a rule whose EBNF line reads `Space = " " | ... ;`.
    let space = find_entity(model, "Space", |e| e.kind == EntityKind::Data);
    assert!(model.entities[space].has_trait(rule));
    let space_rule = model.entities[space].value("rule").expect("rule value");
    assert_eq!(
        object_field(space_rule, "identifier"),
        &Value::String("Space".into())
    );
    let text = object_field(space_rule, "text").as_str().unwrap();
    assert!(text.starts_with("Space = \" \" | \"\t\" | "), "{text:?}");
    assert!(
        text.ends_with("| \"\u{2028}\" | \"\u{2029}\" ;"),
        "{text:?}"
    );
    assert_eq!(model.rule_entity("Space"), Some(space));
    // lex: more than ten criteria, the last of them the terminal document a template
    // execution writes in.
    let lex = find_entity(model, "lex", |e| matches!(e.kind, EntityKind::Fn { .. }));
    assert!(
        model.entities[lex].acceptance_criteria.len() > 10,
        "{}",
        model.entities[lex].acceptance_criteria.len()
    );
    let terminals = criteria_of(model, lex)
        .pop()
        .and_then(|criterion| criterion.behavior)
        .expect("the last criterion of lex has a behavior");
    assert!(
        terminals[0].contains("Identifier = ( IdentifierStart"),
        "{:?}",
        terminals[0]
    );
    assert_eq!(
        criteria_of(model, lex).len(),
        model.entities[lex].acceptance_criteria.len()
    );
    // NegateOperation: one binding application, which gives it precedence and
    // associativity.
    let binding = model
        .trait_named("binding")
        .expect("the grammar declares binding");
    let negate = find_entity(model, "NegateOperation", |e| e.kind == EntityKind::Data);
    let bindings = model.entities[negate]
        .traits
        .iter()
        .filter(|a| a.entity == binding)
        .count();
    assert_eq!(bindings, 1);
    assert!(model.entities[negate].value("precedence").is_some());
    // Primary: an alternation of the primaries.
    let primary = find_entity(model, "Primary", |e| e.kind == EntityKind::Data);
    let syntax = object_field(model.entities[primary].value("rule").unwrap(), "syntax")
        .as_str()
        .unwrap();
    // The alternatives in the order they are written, however the alternation spells the
    // separator between them.
    assert!(syntax.starts_with("[[StringLiteral]]"), "{syntax:?}");
    for alternative in ["[[Template]]", "[[Number]]", "[[Boolean]]", "[[Object]]"] {
        assert!(syntax.contains(alternative), "{alternative}: {syntax:?}");
    }
    assert!(syntax.ends_with("[[Type]]"), "{syntax:?}");
    let alternation = model.trait_named("alternationList").unwrap();
    assert!(model.entities[primary].has_trait(alternation));
    assert!(model.entities[primary].has_trait(rule));
}
