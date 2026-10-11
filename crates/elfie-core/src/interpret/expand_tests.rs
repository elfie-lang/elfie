// @lfy def/interpret/main.lfy:expand
//! The tests of expand.

use super::fixtures::*;
use super::*;

// @lfy def/interpret/main.lfy:expand#expand:expand:d2cc081cb564b1648825d86ca4a9c1ac0367fd6b11ab493532a0570be1c2f407
#[test]
fn a_trait_declared_after_the_entity_that_carries_it_is_still_applied() {
    let model = declared(vec![source(
        "a.lfy",
        "d A is t {}\ntrait t {\n  $x: `d` = string;\n}\n",
        &[],
        Origin::Program,
    )]);
    let model = expand(model);
    let a = entity_named(&model, "A");
    let t = entity_named(&model, "t");
    assert!(model.entities[a].has_trait(t), "A carries t");
    assert!(
        member_symbol(&model, a, "x").is_some(),
        "A has the member x"
    );
    assert_eq!(model.entities[t].entities(), [a]);
    assert!(model.problems.is_empty(), "{:?}", model.problems);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:e95ca6eb34f6fd3768a7222d4bcf16ac081d3e2608c5424733c4c8793a894920
#[test]
fn reading_what_a_trait_was_applied_to_waits_for_every_file_that_can_apply_it() {
    let a = source(
        "a.lfy",
        "d Entity {}\ntrait p {}\ntrait list(...items: Entity[]) {\n  .items = items;\n}\n\
         d X is p {}\nd All is list(...p@entities) {}\n",
        &[],
        Origin::Program,
    );
    let b = source(
        "b.lfy",
        "use \"./a\";\nd Y is p {}\n",
        &[Some("a.lfy")],
        Origin::Program,
    );
    let model = expand(declared(vec![a, b]));
    let (x, y, all) = (
        entity_named(&model, "X"),
        entity_named(&model, "Y"),
        entity_named(&model, "All"),
    );
    let items = model.entities[all].value("items").expect("All holds items");
    assert_eq!(
        items,
        &Interim::List(vec![Interim::Entity(x), Interim::Entity(y)]),
        "the value of All holds X and Y, because reading p@entities waited for b.lfy"
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:a553d1617c777bb329d3e59b4063f9acee7795a53ddf5cc2ec6afbc82796fe71
// @lfy def/interpret/main.lfy:expand#expand:expand:dbbc87c48b923ddf49e557eac7e618d72f9965cd61a2e1bed597c92a0d16d4f1
#[test]
fn a_call_of_a_fn_that_carries_no_builtin_has_nothing_to_run() {
    let model = declared(vec![source(
        "a.lfy",
        "fn f(): `Gives a number` => number {}\nace const c = f();\n",
        &[],
        Origin::Program,
    )]);
    let mut model = expand(model);
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert!(
        model.problems[0].message.contains("f is a fn"),
        "{}",
        model.problems[0].message
    );
    let call = first(&model, 0, E::Call);
    assert_eq!(model.problems[0].node, call);
    // And nothing came of the call, so `c` holds no value.
    let declaration = first(&model, 0, S::VariableDeclaration);
    let value = value_of(&model, declaration);
    assert_eq!(evaluate(&mut model, value), None);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
#[test]
fn a_compile_time_call_of_a_fn_carrying_builtin_is_performed_natively() {
    let directory = std::env::temp_dir().join("elfie-interpret-builtins");
    std::fs::create_dir_all(&directory).expect("a directory to write in");
    let path = directory
        .join("hello.txt")
        .to_string_lossy()
        .replace('\\', "/");
    let program = format!(
        "ace const written = write(\"{path}\", \"hello\");\n\
         ace const back = read(\"{path}\");\n\
         ace const missing = read(\"{path}.none\");\n\
         ace const there = exists(\"{path}\");\n\
         ace const json = stringify(parse('{{\"a\": [1, 2]}}'));\n\
         ace const shell = run(\"echo hi\").stdout;\n\
         ace const variable = environment(\"PATH\");\n"
    );
    let mut sources = natives();
    sources.push(source("a.lfy", &program, &[], Origin::Program));
    let mut model = expand(declared(sources));
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let file = model.file("a.lfy").expect("the program file");
    let values: Vec<NodeRef> = (0..7)
        .map(|at| value_of(&model, nth(&model, file, S::VariableDeclaration, at)))
        .collect();
    let text = |held: &str| Some(Evaluated::Value(Value::String(held.to_string())));
    let truth = Some(Evaluated::Value(Value::Boolean(true)));
    assert_eq!(evaluate(&mut model, values[0]), truth, "write");
    assert_eq!(evaluate(&mut model, values[1]), text("hello"), "read");
    assert_eq!(
        evaluate(&mut model, values[2]),
        Some(Evaluated::Value(Value::Undefined)),
        "read of nothing"
    );
    assert_eq!(evaluate(&mut model, values[3]), truth, "exists");
    assert_eq!(
        evaluate(&mut model, values[4]),
        text("{\"a\":[1,2]}"),
        "parse and stringify"
    );
    assert_eq!(evaluate(&mut model, values[5]), text("hi\n"), "run");
    assert_eq!(
        evaluate(&mut model, values[6]),
        std::env::var("PATH").ok().map(|held| {
            Evaluated::Value(Value::String(held))
        }),
        "environment"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
#[test]
fn an_operation_is_performed_natively_only_when_its_data_carries_builtin() {
    // The dispatch is on the trait the data carries, never on the method's name alone.
    let performed = |carries: &str| -> Option<Evaluated> {
        let values = source(
            "lib/values/base.lfy",
            &format!(
                "trait builtin: `Performed by the compiler` {{}}\n\
                 d List<T> {carries}{{ $join: `The items' text` \
                 = (separator: string) => string; }}\n"
            ),
            &[],
            Origin::Library,
        );
        let main = source(
            "lib/main.lfy",
            "use \"./values/base\";",
            &[Some("lib/values/base.lfy")],
            Origin::Prelude,
        );
        let program = source(
            "a.lfy",
            "ace const joined = [1, 2].join(\"-\");\n",
            &[],
            Origin::Program,
        );
        let mut model = expand(declared(vec![values, main, program]));
        let file = model.file("a.lfy").expect("the program file");
        let value = value_of(&model, first(&model, file, S::VariableDeclaration));
        evaluate(&mut model, value)
    };
    assert_eq!(
        performed("is builtin "),
        Some(Evaluated::Value(Value::String("1-2".to_string())))
    );
    assert_eq!(performed(""), None);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:0c816239c7adfd2752ecb13151d98fd53744cd8d23e3eec6f1df8da00917419d
#[test]
fn a_trait_applied_to_a_file_reaches_the_file_and_one_applied_by_a_call_its_argument() {
    let model = declared(vec![source(
        "a.lfy",
        "trait t {\n  $x: `d` = string;\n}\nd A {}\nt.apply(@);\nt.apply(A);\n",
        &[],
        Origin::Program,
    )]);
    let model = expand(model);
    let t = entity_named(&model, "t");
    let a = entity_named(&model, "A");
    let file = model.file_entities[0];
    // A `Current` with no member name at the top level of a file is the file's entity.
    // @lfy def/interpret/main.lfy:expand#expand:expand:0c816239c7adfd2752ecb13151d98fd53744cd8d23e3eec6f1df8da00917419d
    assert!(model.entities[file].has_trait(t), "the file carries t");
    assert!(model.entities[a].has_trait(t), "A carries t");
    assert_eq!(model.entities[t].entities(), [file, a]);
    assert!(model.problems.is_empty(), "{:?}", model.problems);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:07e6e2fbd3e0d55bead798eb2b27ac9dded4a9209d2690ca888ce69ee0b1bddc
#[test]
fn an_extended_trait_is_applied_too_and_never_joins_the_entities_of_the_one_it_extends() {
    let model = declared(vec![source(
        "a.lfy",
        "trait base(name: string) {\n  .label = name;\n}\n\
         trait t extends base(`b`) {}\nd A is t {}\n",
        &[],
        Origin::Program,
    )]);
    let model = expand(model);
    let (base, t, a) = (
        entity_named(&model, "base"),
        entity_named(&model, "t"),
        entity_named(&model, "A"),
    );
    assert!(model.entities[a].has_trait(t), "A carries t");
    assert!(model.entities[a].has_trait(base), "A carries base too");
    assert_eq!(
        model.entities[a].value("label"),
        Some(&Interim::String("b".to_string())),
        "the extended trait's arguments were evaluated from the extending trait"
    );
    // An extender is never one of the entities of what it extends.
    // @lfy def/interpret/main.lfy:expand#expand:expand:07e6e2fbd3e0d55bead798eb2b27ac9dded4a9209d2690ca888ce69ee0b1bddc
    assert_eq!(model.entities[base].entities(), [a]);
    assert_eq!(model.entities[base].extenders(), [t]);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:e3fd8ad8b33b19a70c578bfce293824df8a73ed5a0ca91556d8acebf48fe8914
#[test]
fn the_most_recently_applied_trait_wins_a_member_two_traits_declare() {
    let model = declared(vec![source(
        "a.lfy",
        "trait first {\n  $x: `From first` = string;\n}\n\
         trait second {\n  $x: `From second` = number;\n}\nd A is first, second {}\n",
        &[],
        Origin::Program,
    )]);
    let model = expand(model);
    let a = entity_named(&model, "A");
    let second = entity_named(&model, "second");
    let member = member_symbol(&model, a, "x").expect("A has the member x");
    let declared_in = model
        .ancestor(model.symbols[member].node, S::TraitDeclaration)
        .expect("declared in a trait");
    assert_eq!(
        model.symbol_of(declared_in).map(|s| model.symbols[s].entity),
        Some(second)
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
#[test]
fn a_criterion_of_a_trait_body_is_the_entitys_with_the_trait_as_its_contributor() {
    let model = declared(vec![source(
        "a.lfy",
        "trait t {\n  where (`s`) -> `b`;\n}\nd A is t {\n  where (`own`) -> `o`;\n}\n",
        &[],
        Origin::Program,
    )]);
    let model = expand(model);
    let (a, t) = (entity_named(&model, "A"), entity_named(&model, "t"));
    let criteria = &model.entities[a].acceptance_criteria;
    assert_eq!(criteria.len(), 2, "{criteria:?}");
    let contributors: Vec<EntityId> = criteria.iter().map(|c| c.contributor).collect();
    assert!(contributors.contains(&a) && contributors.contains(&t), "{contributors:?}");
    let from_trait = criteria
        .iter()
        .find(|c| c.contributor == t)
        .expect("one from the trait");
    assert_eq!(from_trait.situation.as_deref(), Some(["s".to_string()].as_slice()));
}

// @lfy def/interpret/main.lfy:expand#expand:expand:c0d9b076f61371a9e50538ed00b2dccc6dff2b97a5ee0229e0e08db54f2e166b
#[test]
fn an_apply_whose_first_argument_is_an_alternation_list_reaches_every_item_of_it() {
    let model = declared(vec![source(
        "a.lfy",
        "d Entity {}\ntrait alternationList(...items: Entity[]) {}\n\
         d One {}\nd Two {}\nd Either is alternationList(One, Two) {}\n\
         trait mark {}\nmark.apply(Either);\n",
        &[],
        Origin::Program,
    )]);
    let model = expand(model);
    let (one, two, either, mark) = (
        entity_named(&model, "One"),
        entity_named(&model, "Two"),
        entity_named(&model, "Either"),
        entity_named(&model, "mark"),
    );
    assert_eq!(model.entities[mark].entities(), [one, two]);
    assert!(!model.entities[either].has_trait(mark), "not the list itself");
    assert!(model.problems.is_empty(), "{:?}", model.problems);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:8fd627207ea86becebc501f08576d358a6e4d446addee292cd0f1634bb4b9beb
#[test]
fn a_name_a_statement_needs_that_never_resolves_is_named_once_the_rounds_have_stopped() {
    let model = declared(vec![source(
        "a.lfy",
        "trait t {\n  $x: `d` = string;\n}\nd A is missing {}\nd B is t {}\n",
        &[],
        Origin::Program,
    )]);
    let model = expand(model);
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert_eq!(
        model.problems[0].message, "missing does not resolve",
        "{:?}",
        model.problems[0]
    );
    // The statements whose names do resolve still ran.
    let b = entity_named(&model, "B");
    assert!(member_symbol(&model, b, "x").is_some());
}

// @lfy def/interpret/main.lfy:expand#expand:expand:9f92ae8a6753e1a25414725ffb95c7356c3fb1021cf313cd36c68c65c0e8b6a4
#[test]
fn reading_what_a_trait_was_applied_to_waits_for_a_statement_that_applies_it_by_extension() {
    // `X` never names `base`; it carries it only because `sub` extends it, and the
    // reader still waits for that statement.
    let model = declared(vec![source(
        "a.lfy",
        "d Entity {}\ntrait base {}\ntrait sub extends base {}\n\
         trait list(...items: Entity[]) {\n  .items = items;\n}\n\
         d All is list(...base@entities) {}\nd X is sub {}\n",
        &[],
        Origin::Program,
    )]);
    let model = expand(model);
    let (x, all) = (entity_named(&model, "X"), entity_named(&model, "All"));
    assert_eq!(
        model.entities[all].value("items"),
        Some(&Interim::List(vec![Interim::Entity(x)])),
        "All read base@entities after X carried base through the extends chain"
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:1ee5f4b0a08eba34d8c061dfcdbf152c3e11233a665f48c77f2394490ee76a30
#[test]
fn a_statement_waiting_on_a_trait_nothing_can_apply_any_more_is_named_and_never_runs() {
    // Two statements each wait to read what the other's trait was applied to, so
    // neither can ever run.
    let model = declared(vec![source(
        "a.lfy",
        "d Entity {}\ntrait p {}\ntrait q {}\n\
         trait list(...items: Entity[]) {\n  .items = items;\n}\n\
         d One is p, list(...q@entities) {}\nd Two is q, list(...p@entities) {}\n",
        &[],
        Origin::Program,
    )]);
    let model = expand(model);
    assert_eq!(model.problems.len(), 2, "{:?}", model.problems);
    assert!(
        model
            .problems
            .iter()
            .all(|problem| problem.message.contains("waits to read what")),
        "{:?}",
        model.problems
    );
    // Neither of them ran, so neither holds the items it was waiting for.
    let one = entity_named(&model, "One");
    let two = entity_named(&model, "Two");
    assert_eq!(model.entities[one].value("items"), None);
    assert_eq!(model.entities[two].value("items"), None);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:bc5da8aae1f9989cfd3e5791911af8af36ce78edef0fa9da9594fd85e7449b18
#[test]
fn running_a_statement_changes_only_the_model_and_never_a_tree() {
    let sources = vec![source(
        "a.lfy",
        "trait t {\n  $x: `d` = string;\n}\nd A is t {}\n",
        &[],
        Origin::Program,
    )];
    let before = declared(sources.clone());
    let trees: Vec<_> = before
        .sources
        .iter()
        .map(|source| source.tree.clone())
        .collect();
    let after = expand(before);
    let kept: Vec<_> = after
        .sources
        .iter()
        .map(|source| source.tree.clone())
        .collect();
    assert_eq!(trees, kept, "no tree was changed");
}

// @lfy def/interpret/main.lfy:expand#expand:expand:bbad3f48d6df69b629c7de7d6de1d28945311a91b23745b50defb5cbbfdd328f
// @lfy def/interpret/main.lfy:expand#expand:expand:da7602e2b3243080f2623c7ec36d5389af778549e7da59559ebadf2d10ec6e0a
// @lfy def/interpret/main.lfy:expand#expand:expand:24896a9f182aab4cba64abb17efdb629e1de1a7ac24c9edcdbc3fa14873d813b
// @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
// @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
#[test]
fn an_ace_const_holding_a_target_carries_every_layer_in_guidance_order() {
    let model = expanded_with_targets(
        "trait l extends targetLanguage {\n  .markerComment = \"#\";\n}\n\
         trait f(root: string) extends targetLayout {\n  \
         where (`built`) -> `it is under {{root}}`;\n}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = f.layer(\"o\") });\n\
         d A {\n  @targets.add(t);\n}\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let t = entity_named(&model, "t");
    let l = entity_named(&model, "l");
    let f = entity_named(&model, "f");
    // The layout is a layer before the language, as guidance order has it.
    let layers: Vec<EntityId> = model.entities[t]
        .traits
        .iter()
        .filter(|applied| matches!(applied.source, AppliedSource::Apply(_)))
        .map(|applied| applied.entity)
        .collect();
    assert_eq!(layers, vec![f, l], "the layout, then the language");
    // The layer's criterion, with its template value rendered from its argument.
    let criterion = model.entities[t]
        .acceptance_criteria
        .iter()
        .find(|criterion| criterion.contributor == f)
        .expect("a criterion contributed by the layout");
    assert_eq!(
        criterion.behavior,
        Some(vec!["it is under o".to_string()]),
        "{criterion:?}"
    );
    // And the language's value.
    assert_eq!(
        model.entities[t].value("markerComment"),
        Some(&Interim::String("#".to_string()))
    );
    // The data that named the target holds it.
    let a = entity_named(&model, "A");
    assert_eq!(model.entities[a].targets, vec![t]);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:da7602e2b3243080f2623c7ec36d5389af778549e7da59559ebadf2d10ec6e0a
#[test]
fn a_trait_in_two_slots_with_equal_arguments_is_one_layer_at_its_first_place() {
    let model = expanded_with_targets(
        "trait both extends targetLayout {}\n\
         trait l extends targetLanguage {}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = both, \
         layers = [both] });\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let t = entity_named(&model, "t");
    let both = entity_named(&model, "both");
    let layers: Vec<EntityId> = model.entities[t]
        .traits
        .iter()
        .filter(|applied| matches!(applied.source, AppliedSource::Apply(_)))
        .map(|applied| applied.entity)
        .collect();
    // `layers` is the first slot of guidance order, so that is where it stands, once.
    assert_eq!(layers.iter().filter(|&&held| held == both).count(), 1);
    assert_eq!(layers.first(), Some(&both), "{layers:?}");
}

// @lfy def/interpret/main.lfy:expand#expand:expand:24896a9f182aab4cba64abb17efdb629e1de1a7ac24c9edcdbc3fa14873d813b
#[test]
fn two_layers_setting_one_member_to_different_values_name_the_member_and_both_layers() {
    let model = expanded_with_targets(
        "trait l extends targetLanguage {}\n\
         trait f extends targetLayout {}\n\
         trait one extends target {\n  .provides = [\"a\"];\n  .style = \"wide\";\n}\n\
         trait two extends target {\n  .provides = [\"b\"];\n  .style = \"tall\";\n}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = f, \
         layers = [one, two] });\n",
    );
    // `provides` is added to by every layer, so the two holding different items is no
    // conflict; `style` is set, so it is one.
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    let message = &model.problems[0].message;
    for named in ["style", "one", "two"] {
        assert!(message.contains(named), "{named} is named: {message}");
    }
    assert!(!message.contains("provides"), "{message}");
    // The problem is at the const that declares the target.
    let t = entity_named(&model, "t");
    assert_eq!(
        model.entities[t].node.map(|node| node.file),
        Some(model.problems[0].node.file)
    );
    assert!(model.is(model.problems[0].node, S::VariableDeclaration));
}

// @lfy def/interpret/main.lfy:expand#expand:expand:24896a9f182aab4cba64abb17efdb629e1de1a7ac24c9edcdbc3fa14873d813b
#[test]
fn two_layers_setting_one_member_to_the_same_value_agree() {
    let model = expanded_with_targets(
        "trait l extends targetLanguage {}\n\
         trait f extends targetLayout {}\n\
         trait one extends target {\n  .style = \"wide\";\n}\n\
         trait two extends target {\n  .style = \"wide\";\n}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = f, \
         layers = [one, two] });\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let t = entity_named(&model, "t");
    assert_eq!(
        model.entities[t].value("style"),
        Some(&Interim::String("wide".to_string()))
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:f2da259bc931b16b67600269961004bc2fdf0d760814936c7268ae3ed27c6981
#[test]
fn a_slot_holding_neither_a_trait_nor_a_layer_of_one_names_the_slot() {
    let model = expanded_with_targets(
        "trait l extends targetLanguage {}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = \"nothing\" });\n",
    );
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert!(
        model.problems[0].message.contains("the layout of this target"),
        "{}",
        model.problems[0].message
    );
    // The language was still applied; only the layout was not.
    let t = entity_named(&model, "t");
    let l = entity_named(&model, "l");
    assert!(model.entities[t].has_trait(l));
}

// @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
#[test]
fn a_layer_given_a_different_number_of_arguments_names_the_trait() {
    let model = expanded_with_targets(
        "trait l(name: string, root: string) extends targetLanguage {}\n\
         trait f extends targetLayout {}\n\
         ace const t = targetOf({ output = \"o\", language = l.layer(\"a\"), layout = f });\n",
    );
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert!(
        model.problems[0].message.contains("l takes 2 arguments as a layer, not 1"),
        "{}",
        model.problems[0].message
    );
    // A parameter with a default, like one marked optional, may be left out, so one
    // argument is not fewer than the parameters a call must give.
    let model = expanded_with_targets(
        "trait l(name: string, root: string = \"o\") extends targetLanguage {}\n\
         trait f extends targetLayout {}\n\
         ace const t = targetOf({ output = \"o\", language = l.layer(\"a\"), layout = f });\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let model = expanded_with_targets(
        "trait l(name?: string) extends targetLanguage {}\n\
         trait f extends targetLayout {}\n\
         ace const t = targetOf({ output = \"o\", language = l.layer(), layout = f });\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    // More arguments than parameters, with none of them a spread, is still a problem.
    let model = expanded_with_targets(
        "trait l(name: string) extends targetLanguage {}\n\
         trait f extends targetLayout {}\n\
         ace const t = targetOf({ output = \"o\", \
         language = l.layer(\"a\", \"b\"), layout = f });\n",
    );
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert!(
        model.problems[0].message.contains("l takes 1 arguments as a layer, not 2"),
        "{}",
        model.problems[0].message
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
// @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
// @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
#[test]
fn knowledge_and_commands_are_appended_with_the_contributor_a_criterion_would_have() {
    let model = expanded_with_targets(
        "trait given {\n  \
         @knowledge.add({ topic = `the trait's guide`, kind = KnowledgeKind.example, \
         source = \"docs/t.md\" });\n  \
         @commands.add({ operation = Operation.test, line = \"cargo test\" });\n}\n\
         d A is given {\n  \
         @knowledge.add({ topic = `a guide`, kind = KnowledgeKind.reference, \
         source = \"docs/a.md\", quote = true });\n}\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let a = entity_named(&model, "A");
    let given = entity_named(&model, "given");
    // The `is` clause runs before the body, so the trait's item is appended first;
    // which order the member is read in is `Entity.knowledge`'s business.
    let topics: Vec<(&str, EntityId)> = model.entities[a]
        .knowledge
        .iter()
        .map(|item| (item.topic.as_str(), item.contributor))
        .collect();
    assert_eq!(
        topics,
        vec![("the trait's guide", given), ("a guide", a)],
        "one from the trait, with the trait as its contributor, and one of its own"
    );
    let own = &model.entities[a].knowledge[1];
    assert_eq!(own.kind, KnowledgeKind::Reference);
    assert_eq!(own.source, "docs/a.md");
    assert_eq!(own.quote, Some(true));
    assert_eq!(model.entities[a].knowledge[0].kind, KnowledgeKind::Example);
    // The command the trait added reaches the entity with the trait as its contributor.
    assert_eq!(
        model.entities[a].commands,
        vec![Command {
            operation: Operation::Test,
            line: Some("cargo test".to_string()),
            contributor: given,
        }]
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:7f7104a1337b2505a318c15f0b6be1af739e45b47f0cedf1d221cb3aa0b55877
// @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
#[test]
fn an_argument_that_names_no_ace_const_holding_a_target_is_named_and_adds_nothing() {
    let model = expanded_with_targets(
        "d A {\n  \
         @knowledge.add({ topic = `a guide`, kind = KnowledgeKind.reference, \
         source = \"docs/a.md\", quote = true });\n  @targets.add(A);\n}\n",
    );
    let a = entity_named(&model, "A");
    assert_eq!(model.entities[a].knowledge.len(), 1);
    assert_eq!(model.entities[a].knowledge[0].topic, "a guide");
    assert_eq!(model.entities[a].knowledge[0].contributor, a);
    assert!(model.entities[a].targets.is_empty());
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert!(
        model.problems[0].message.contains("names no target"),
        "{}",
        model.problems[0].message
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:8159e82000937ae0a26519fa34e1579b5f6a3bcd6325987ebc1729fd11812494
#[test]
fn the_same_target_added_twice_is_named_and_appended_once() {
    let model = expanded_with_targets(
        "trait l extends targetLanguage {}\ntrait f extends targetLayout {}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = f });\n\
         d A {\n  @targets.add(t).add(t);\n}\n",
    );
    let t = entity_named(&model, "t");
    let a = entity_named(&model, "A");
    assert_eq!(model.entities[a].targets, vec![t]);
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert!(
        model.problems[0].message.contains("added twice"),
        "{}",
        model.problems[0].message
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:52171aea8ab1474de54983df55c484fadc588bd0d6e58e36b25e18a182ea17bc
#[test]
fn a_target_added_to_a_member_says_it_is_built_with_its_owner() {
    let model = expanded_with_targets(
        "trait l extends targetLanguage {}\ntrait f extends targetLayout {}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = f });\n\
         d A {\n  $x: `A member` = string;\n}\n\
         with A$x {\n  @targets.add(t);\n}\n",
    );
    let x = model
        .entities
        .iter()
        .position(|entity| {
            entity.identifier.as_deref() == Some("x")
                && matches!(entity.kind, EntityKind::Member)
        })
        .expect("the member x");
    assert!(model.entities[x].targets.is_empty());
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert!(
        model.problems[0].message.contains("built with its owner"),
        "{}",
        model.problems[0].message
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:9fd15ed81e451fcd9f336951362bbe3056f84d0c8a72848f35182827c6183df2
#[test]
fn a_data_that_names_a_target_const_holds_that_target() {
    let model = expanded_with_targets(
        "trait l extends targetLanguage {}\ntrait f extends targetLayout {}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = f });\n\
         d A {\n  @targets.add(t);\n}\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let t = entity_named(&model, "t");
    let a = entity_named(&model, "A");
    assert_eq!(model.entities[a].targets, vec![t]);
}

// @lfy def/interpret/main.lfy:expand#expand:expand:324d74ea224e2134d78f0caae9ebd1795e365b8febb4b1960cc6502006aac73a
#[test]
fn a_value_a_layer_sets_is_the_targets_own() {
    let model = expanded_with_targets(
        "trait l extends targetLanguage {\n  .markerComment = \"#\";\n}\n\
         trait f extends targetLayout {}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = f });\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let t = entity_named(&model, "t");
    assert_eq!(
        model.entities[t].value("markerComment"),
        Some(&Interim::String("#".to_string()))
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:328d9016cb991330df7fe7040a6899ac273f8ac167bf7648ddb76949426a70e3
#[test]
fn a_criterion_of_a_layer_is_rendered_with_the_arguments_the_layer_was_given() {
    let model = expanded_with_targets(
        "trait l extends targetLanguage {}\n\
         trait f(root: string) extends targetLayout {\n  \
         where (`built`) -> `it is under {{root}}`;\n}\n\
         ace const t = targetOf({ output = \"o\", language = l, layout = f.layer(\"o\") });\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let t = entity_named(&model, "t");
    let f = entity_named(&model, "f");
    let criterion = model.entities[t]
        .acceptance_criteria
        .iter()
        .find(|criterion| criterion.contributor == f)
        .expect("a criterion contributed by the layout");
    assert_eq!(
        criterion.behavior,
        Some(vec!["it is under o".to_string()]),
        "{criterion:?}"
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:6524f7346bf65638da4a1898089f030adb78c9937ba28ffcfa22b3fea141b329
#[test]
fn a_piece_of_knowledge_a_data_adds_is_its_own() {
    let model = expanded_with_targets(
        "d A {\n  \
         @knowledge.add({ topic = `a guide`, kind = KnowledgeKind.reference, \
         source = \"docs/a.md\", quote = true });\n}\n",
    );
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let a = entity_named(&model, "A");
    assert_eq!(
        model.entities[a].knowledge,
        vec![Knowledge {
            topic: "a guide".to_string(),
            kind: KnowledgeKind::Reference,
            source: "docs/a.md".to_string(),
            quote: Some(true),
            contributor: a,
        }]
    );
}

// @lfy def/interpret/main.lfy:expand#expand:expand:d6d80207ed7ea67fd728b58a28b9ec2288d778cb2ae43de5d0dcf264ce3abd1a
#[test]
fn a_context_layer_of_a_const_is_read_from_the_const_and_not_from_its_value() {
    let mut model = bound("d A {}\nconst c = A;\nconst n = c@identifier;\n");
    let declaration = nth(&model, 0, S::VariableDeclaration, 1);
    let value = value_of(&model, declaration);
    assert_eq!(
        evaluate(&mut model, value),
        Some(Evaluated::Value(Value::String("c".to_string())))
    );
}

/// This is the pass the binder runs between declare and resolve, so the model
/// `crate::model::bind` builds is this pass's work: running it again over the declared
/// repository reaches the same traits and the same criteria, with no problem of its
/// own, and both reach what only a complete trait list and a criterion added to
/// `global` can.
// @lfy def/interpret/main.lfy:expand#expand:expand:9f92ae8a6753e1a25414725ffb95c7356c3fb1021cf313cd36c68c65c0e8b6a4
#[test]
fn the_repository_expands_as_the_binders_expand_pass() {
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let bound = crate::workspace::load(root).model;
    let expanded = expand(declared(bound.sources.clone()));
    assert_eq!(expanded.entities.len(), bound.entities.len());
    let mut differences: Vec<String> = Vec::new();
    for entity in 0..bound.entities.len() {
        // A declaration seen with type arguments takes what its declaration reached
        // after the expand pass, which is the binder's own step and not this one's.
        if bound.generic_base(entity) != entity {
            continue;
        }
        let (was, now) = (&bound.entities[entity], &expanded.entities[entity]);
        let named = |ids: &[EntityId]| -> Vec<String> {
            ids.iter()
                .map(|&id| {
                    bound.entities[id]
                        .identifier
                        .clone()
                        .unwrap_or_else(|| format!("#{id}"))
                })
                .collect()
        };
        let where_at = was
            .node
            .map(|node| bound.sources[node.file].path.clone())
            .unwrap_or_default();
        // The same traits, in both directions: what the binder's model carries is what
        // this pass applies, and nothing else.
        let mine: Vec<EntityId> = now.traits.iter().map(|a| a.entity).collect();
        let theirs: Vec<EntityId> = was.traits.iter().map(|a| a.entity).collect();
        let missing: Vec<EntityId> = theirs
            .iter()
            .copied()
            .filter(|applied| !mine.contains(applied))
            .collect();
        let extra: Vec<EntityId> = mine
            .iter()
            .copied()
            .filter(|applied| !theirs.contains(applied))
            .collect();
        if !missing.is_empty() || !extra.is_empty() {
            differences.push(format!(
                "{:?} of {where_at} carries {:?} where the binder's model carries {:?}",
                was.identifier,
                named(&mine),
                named(&theirs)
            ));
        }
        let show = |criteria: &[Criterion]| -> Vec<String> {
            criteria
                .iter()
                .map(|c| format!("{:?}/{:?}", c.situation, c.behavior))
                .collect()
        };
        let held = show(&now.acceptance_criteria);
        let mut absent = show(&was.acceptance_criteria);
        absent.retain(|one| !held.contains(one));
        if !absent.is_empty() {
            differences.push(format!(
                "{:?} of {where_at} is missing {absent:#?}",
                was.identifier
            ));
        }
    }
    assert!(differences.is_empty(), "{differences:#?}");
    let messages: Vec<String> = expanded
        .problems
        .iter()
        .map(|problem| format!("{problem:?}"))
        .collect();
    assert!(expanded.problems.is_empty(), "{messages:#?}");

    // A statement that reads what a trait was applied to waits until no statement left
    // to run can apply it, so the loop over `declaring@entities` in `bind` reaches every
    // declaring rule. The binder's own model reaches it too, because this pass is the
    // one it runs.
    // @lfy def/interpret/main.lfy:expand
    let binder = entity_named(&expanded, "bind");
    let says = |model: &Model, entity: EntityId, text: &str| -> bool {
        model.entities[entity]
            .acceptance_criteria
            .iter()
            .any(|criterion| {
                criterion
                    .situation
                    .iter()
                    .flatten()
                    .any(|one| one.contains(text))
            })
    };
    assert!(
        says(&expanded, binder, "An entity is an enum"),
        "the whole list of kind data rows was read"
    );
    assert!(
        says(&bound, binder, "An entity is an enum"),
        "the binder's expand pass is this one"
    );

    // A criterion added to global's context is global's.
    // @lfy def/interpret/main.lfy:expand
    let reaches = |model: &Model, text: &str| -> bool {
        model.entities[model.global]
            .acceptance_criteria
            .iter()
            .any(|criterion| {
                criterion
                    .behavior
                    .iter()
                    .flatten()
                    .any(|one| one.contains(text))
            })
    };
    assert!(
        reaches(&expanded, "No two [[terminal]] entities have the same syntax"),
        "what def/grammar adds to global's context is global's"
    );
    assert!(
        reaches(&bound, "No two [[terminal]] entities have the same syntax"),
        "the binder's model reaches it because this pass is the one it runs"
    );
}
