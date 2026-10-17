// @lfy def/interpret/main.lfy:lower
//! The tests of lower.

use super::fixtures::*;
use super::lower::{alias_of, is_member_node, package_use_path};
use super::*;

// @lfy def/interpret/main.lfy:lower#lower:lower:19c61e03880d4a18f2b273191a8e8ffae4a8b61376fa29fde230cb24987ab258
// @lfy def/interpret/main.lfy:lower#lower:lower:780cd0f400942687d110a357f183ca0c8d29ee46a09884327d3627fb3e2676ac
#[test]
fn a_data_is_kept_with_its_members_and_its_criteria_and_its_trait_is_dropped() {
    let program = lower(workspace(
        "trait t {\n  $x: `d` = string;\n}\nd A is t {\n  where (`s`) -> `b`;\n}\n",
    ));
    assert_eq!(program.files.len(), 1);
    let file = &program.files[0];
    assert_eq!(file.root.rule, F::SourceFile.entity());
    let kept: Vec<&LoweredNode> = file.root.nodes().collect();
    assert_eq!(kept.len(), 1, "only the data is kept: {kept:?}");
    let data = kept[0];
    assert_eq!(data.rule, S::DataDeclaration.entity());
    // With no clause: the `is t` was dropped with everything inside it.
    assert!(
        !file.text.contains(" is t"),
        "the clause was dropped: {}",
        file.text
    );
    assert!(!file.text.contains("trait t"), "no node for t: {}", file.text);
    // One member node for x, whose origin is the statement in the trait that declared it.
    let members: Vec<&LoweredNode> = data.nodes().collect();
    assert_eq!(members.len(), 1, "{members:?}");
    let member = members[0];
    let model = &program.workspace.model;
    assert_eq!(
        model.entities[member.entity.expect("a member")].identifier.as_deref(),
        Some("x")
    );
    let inside_the_trait = model
        .ancestor(member.origin, S::TraitDeclaration)
        .expect("the origin is in the trait");
    assert_eq!(
        model
            .symbol_of(inside_the_trait)
            .map(|s| model.symbols[s].name.clone()),
        Some("t".to_string())
    );
    // One lowered criterion, with situation `s` and an id naming A twice.
    assert_eq!(data.criteria.len(), 1, "{:?}", data.criteria);
    let criterion = &data.criteria[0];
    assert_eq!(criterion.situation.as_deref(), Some(["s".to_string()].as_slice()));
    assert!(criterion.id.starts_with("A:A:"), "{}", criterion.id);
    assert_eq!(criterion.scope, RequirementScope::Local);
    // And never into the tree's children.
    assert!(
        every(&file.root).iter().all(|node| node.rule != S::Where.entity()),
        "no criterion is a node"
    );
}

// @lfy def/interpret/main.lfy:lower#lower:lower:1b8bbc7b6e2316e2fb3cfd378dd5fc01093dbefd96e6098d404519e88a6a68ae
#[test]
fn a_member_whose_type_is_a_list_of_a_union_is_spelled_with_the_union_parenthesized() {
    let program = lower(workspace(
        "d A {}\nd B {}\nd Box {\n  $pairs: `d` = (A | B)[];\n  $one: `d` = A | B;\n\
         \n  $plain: `d` = A[];\n}\n",
    ));
    let file = &program.files[0];
    assert!(
        file.text.contains("$pairs: `d` = (A | B)[];"),
        "the union takes parentheses before the brackets: {}",
        file.text
    );
    // A union that is not the item of a list, and a list whose item is not a union,
    // take no parentheses.
    assert!(
        file.text.contains("$one: `d` = A | B;") && file.text.contains("$plain: `d` = A[];"),
        "{}",
        file.text
    );
}

/// The spelling is the one `typeTextOf` gives, so a predicate, a list, and a union all
/// read as they were written, and a type the model reads no further than its text is
/// spelled by that text rather than by a stand-in name.
// @lfy def/interpret/main.lfy:lower#lower:lower:1b8bbc7b6e2316e2fb3cfd378dd5fc01093dbefd96e6098d404519e88a6a68ae
#[test]
fn a_member_type_is_spelled_as_type_text_of_spells_it() {
    let program = lower(workspace(
        "d A {}\ntrait t {}\n\
         d B {\n  $pred: `d` = is t;\n  $many: `d` = A[];\n}\n",
    ));
    let file = &program.files[0];
    for written in ["$pred: `d` = is t;", "$many: `d` = A[];"] {
        assert!(file.text.contains(written), "{written}: {}", file.text);
    }
    // Nothing is spelled by a stand-in name.
    assert!(!file.text.contains("anonymous"), "{}", file.text);
}

// @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
#[test]
fn a_member_type_naming_an_entity_of_an_unused_file_adds_one_use_of_that_file() {
    let program = lower(workspace_of(vec![
        source(
            "def/model/kinds.lfy",
            "d Kind {}\nd Shape {}\n",
            &[],
            Origin::Program,
        ),
        source(
            "def/t.lfy",
            "use \"./model/kinds\";\ntrait t {\n  $kind: `Which` = Kind;\n  \
             $shape: `What` = Shape;\n}\n",
            &[Some("def/model/kinds.lfy")],
            Origin::Program,
        ),
        source(
            "def/a.lfy",
            "use \"./t\";\nd A is t {}\n",
            &[Some("def/t.lfy")],
            Origin::Program,
        ),
    ]));
    let file = &program.files[2];
    // One use, however many members name the file, with its path written relative to
    // the file the use is written in.
    assert_eq!(
        file.text.matches("use \"./model/kinds\";").count(),
        1,
        "{}",
        file.text
    );
    let written = file.text.find("use \"./t\";").expect("the use written");
    let added = file
        .text
        .find("use \"./model/kinds\";")
        .expect("the use added");
    assert!(written < added, "the added use comes after: {}", file.text);
    assert!(
        file.text.contains("$kind: `Which` = Kind;")
            && file.text.contains("$shape: `What` = Shape;"),
        "{}",
        file.text
    );
}

// @lfy def/interpret/main.lfy:lower#lower:lower:329857f0c3255927997222dd97d6ffe47c063ae5245311473963c3d3359c7a40
#[test]
fn an_added_use_takes_an_alias_when_the_file_already_sees_the_name() {
    let program = lower(workspace_of(vec![
        source("def/model/kinds.lfy", "d Kind {}\n", &[], Origin::Program),
        source("def/other.lfy", "d Kind {}\n", &[], Origin::Program),
        source(
            "def/t.lfy",
            "use \"./model/kinds\";\ntrait t {\n  $kind: `Which` = Kind;\n}\n",
            &[Some("def/model/kinds.lfy")],
            Origin::Program,
        ),
        source(
            "def/a.lfy",
            "use \"./other\";\nuse \"./t\";\nd A is t {}\n",
            &[Some("def/other.lfy"), Some("def/t.lfy")],
            Origin::Program,
        ),
    ]));
    let file = &program.files[3];
    // The alias is the declaring file's name, model/kinds, with each slash an underscore,
    // and the type is spelled through it.
    assert!(
        file.text.contains("use \"./model/kinds\" as model_kinds;"),
        "{}",
        file.text
    );
    assert!(
        file.text.contains("$kind: `Which` = model_kinds.Kind;"),
        "{}",
        file.text
    );
}

// @lfy def/interpret/main.lfy:lower#lower:lower:329857f0c3255927997222dd97d6ffe47c063ae5245311473963c3d3359c7a40
// @lfy def/interpret/main.lfy:lower#lower:lower:7e159bfaa7ce0d4765fa4fdbd906f7230afdf9fce2ae0297f29ee8f15f2af929
#[test]
fn an_alias_names_nothing_else_and_a_package_file_is_used_by_its_package() {
    assert_eq!(alias_of("model/kind-set"), "model_kind_set");
    assert_eq!(alias_of("elfie/prelude/trait"), "elfie_prelude_trait");
    assert_eq!(
        package_use_path("elfie", "../lib", "../lib/prelude/trait.lfy"),
        "elfie/prelude/trait"
    );
    let program = lower(workspace_of(vec![
        source("def/model/kinds.lfy", "d Kind {}\n", &[], Origin::Program),
        source("def/other.lfy", "d Kind {}\n", &[], Origin::Program),
        source("def/model_kinds.lfy", "d model_kinds {}\n", &[], Origin::Program),
        source(
            "def/t.lfy",
            "use \"./model/kinds\";\ntrait t {\n  $kind: `Which` = Kind;\n}\n",
            &[Some("def/model/kinds.lfy")],
            Origin::Program,
        ),
        source(
            "def/a.lfy",
            "use \"./other\";\nuse \"./model_kinds\";\nuse \"./t\";\nd A is t {}\n",
            &[
                Some("def/other.lfy"),
                Some("def/model_kinds.lfy"),
                Some("def/t.lfy"),
            ],
            Origin::Program,
        ),
    ]));
    let file = &program.files[4];
    assert!(
        file.text.contains("use \"./model/kinds\" as model_kinds1;"),
        "{}",
        file.text
    );
    assert!(
        file.text.contains("$kind: `Which` = model_kinds1.Kind;"),
        "{}",
        file.text
    );
}

// @lfy def/interpret/main.lfy:lower#lower:lower:da4462e6d2b3dd1ccd2d1540c52f12f97141c350c88131325e6ac17e2e542890
#[test]
fn an_ace_function_is_dropped_and_the_const_it_answers_holds_its_value() {
    let program = lower(workspace(
        "ace function q(t: string) -> string {\n  return t + \"!\";\n}\nconst c = q(\"x\");\n",
    ));
    let file = &program.files[0];
    let kept: Vec<&LoweredNode> = file.root.nodes().collect();
    assert_eq!(kept.len(), 1, "only the const is kept: {kept:?}");
    assert_eq!(kept[0].rule, S::VariableDeclaration.entity());
    let folded: Vec<&LoweredNode> = every(kept[0])
        .into_iter()
        .filter(|node| node.is_folded())
        .collect();
    assert_eq!(folded.len(), 1, "{folded:?}");
    assert_eq!(
        folded[0].value,
        Some(Evaluated::Value(Value::String("x!".to_string())))
    );
    assert!(file.text.contains("\"x!\""), "{}", file.text);
    assert!(program.problems.is_empty(), "{:?}", program.problems);
}

// @lfy def/interpret/main.lfy:lower#lower:lower:d661ea358255d3366f18899795e1a76056c8a3e1deed68e3c07998617fe2fcd1
#[test]
fn what_the_model_answers_is_folded_and_what_only_runtime_answers_is_kept() {
    let program = lower(workspace(
        "trait r {}\nd A is r {}\nfunction names() -> string[] {\n  let out = [];\n\
         \n  for (const e in r@entities) {\n    out.push(e@identifier);\n  }\n\n  return out;\n}\n",
    ));
    let file = &program.files[0];
    let model = &program.workspace.model;
    let a = entity_named(model, "A");
    let function = file
        .root
        .nodes()
        .find(|node| node.rule == S::FunctionDeclaration.entity())
        .expect("the function is kept");
    let folded: Vec<&LoweredNode> = every(function)
        .into_iter()
        .filter(|node| node.is_folded())
        .collect();
    assert!(
        folded
            .iter()
            .any(|node| node.value
                == Some(Evaluated::Value(Value::List(vec![Value::Entity(a)])))),
        "r@entities folded to a list holding A: {folded:?}"
    );
    // `e@identifier` is kept, because `e` only has a value at runtime.
    assert!(
        file.text.contains("e@identifier"),
        "e@identifier is kept: {}",
        file.text
    );
}

// @lfy def/interpret/main.lfy:lower#lower:lower:ddb0c0418b9da6c686e961a3b22818b2b8c151a8614ec2dbeed43fc2390af6fd
#[test]
fn a_criterion_added_to_global_or_by_a_trait_applied_to_it_is_global_and_on_no_node() {
    let a = source(
        "a.lfy",
        "trait g {\n  where (`any`) -> `holds`;\n}\ng.apply(global);\nd A {}\nd B {}\n",
        &[],
        Origin::Program,
    );
    let b = source(
        "b.lfy",
        "use \"./a\";\nglobal@acceptanceCriteria.add({ behavior = `all` });\n",
        &[Some("a.lfy")],
        Origin::Program,
    );
    let built = workspace_of(vec![a, b]);
    let global = built.model.global;
    assert_eq!(
        built.model.entities[global].acceptance_criteria.len(),
        2,
        "{:?} problems {:?}",
        built.model.entities[global].acceptance_criteria,
        built.model.problems
    );
    let program = lower(built);
    assert_eq!(program.criteria.len(), 2, "{:?}", program.criteria);
    let ids: Vec<&str> = program.criteria.iter().map(|c| c.id.as_str()).collect();
    assert!(
        ids.iter().any(|id| id.starts_with("global:g:")),
        "the trait's criterion is global: {ids:?}"
    );
    assert!(
        ids.iter().all(|id| id.starts_with("global:")),
        "every one of them is global: {ids:?}"
    );
    for criterion in &program.criteria {
        assert_eq!(criterion.scope, RequirementScope::Global);
        assert_eq!(criterion.entity, None);
    }
    // And on no node and no file.
    for file in &program.files {
        assert!(file.criteria.is_empty(), "{:?}", file.criteria);
        for node in every(&file.root) {
            assert!(node.criteria.is_empty(), "{:?}", node.criteria);
        }
    }
}

// @lfy def/interpret/main.lfy:lower#lower:lower:5770c9d25caf0fb2ef7bb2b57c8f55ee0200d98e2f2b365f2025766aa3c0a165
#[test]
fn a_file_of_only_compile_time_statements_lowers_to_an_empty_source_file() {
    let program = lower(workspace("trait t {}\ntrait u {}\n"));
    let file = &program.files[0];
    assert_eq!(file.root.rule, F::SourceFile.entity());
    assert_eq!(file.root.nodes().count(), 0, "{:?}", file.root);
    assert_eq!(file.text.trim(), "");
    // One lowered file per file of the workspace, in the same order.
    assert_eq!(program.files.len(), program.workspace.files.len());
    assert_eq!(file.file, 0);
}

// @lfy def/interpret/main.lfy:lower#lower:lower:2fbd03867430231c575f4b7bd56187dd57505ab549bcea0488aa06843b0f9b93
#[test]
fn a_function_whose_every_statement_is_compile_time_is_kept_empty_and_named() {
    let program = lower(workspace(
        "trait t {}\nd X {}\nfunction f() {\n  t.apply(X);\n}\n",
    ));
    let file = &program.files[0];
    // The data and the function are kept; the trait is not.
    let kept: Vec<&LoweredNode> = file.root.nodes().collect();
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(kept[0].rule, S::DataDeclaration.entity());
    let f = kept[1];
    assert_eq!(f.rule, S::FunctionDeclaration.entity());
    // Its body is empty: the statement ran at compile time and has no lowered node.
    let block = f
        .nodes()
        .find(|node| node.rule == S::Block.entity())
        .expect("the body is kept");
    assert_eq!(block.nodes().count(), 0, "{block:?}");
    // The statement ran as it would anywhere else, so X carries t.
    let model = &program.workspace.model;
    let t = entity_named(model, "t");
    let x = entity_named(model, "X");
    assert!(model.entities[x].has_trait(t), "X carries t");
    // And one problem, at the function rather than at the statement.
    assert_eq!(program.problems.len(), 1, "{:?}", program.problems);
    assert_eq!(program.problems[0].node, f.origin);
    assert!(
        program.problems[0].message.contains("empty at runtime"),
        "{}",
        program.problems[0].message
    );
}

// @lfy def/interpret/main.lfy:lower#lower:lower:c89173885cbe173a76e8a9a59310d5e69174bce617247c866b7d675aeeb308e2
#[test]
fn a_file_level_let_is_not_allowed_and_is_kept() {
    let program = lower(workspace("let n = 1;\n"));
    assert_eq!(program.problems.len(), 1, "{:?}", program.problems);
    assert!(
        program.problems[0].message.contains("file-level let"),
        "{}",
        program.problems[0].message
    );
    let kept: Vec<&LoweredNode> = program.files[0].root.nodes().collect();
    assert_eq!(kept.len(), 1, "the declaration is kept: {kept:?}");
    assert_eq!(kept[0].rule, S::VariableDeclaration.entity());
}

// @lfy def/interpret/main.lfy:lower#lower:lower:3032db4bcf2ffe2031d9fceb6ceced6ed34b2271c3cc994fa6cfb219215ef323
#[test]
fn a_criterion_of_a_targets_declaration_is_guidance_and_no_criterion_of_the_program() {
    let mut sources = targets();
    sources.push(source(
        "a.lfy",
        "trait shape extends targetLanguage {\n  where (`built`) -> `shaped`;\n}\n\
         ace const rust = targetOf({ output = \"crates\", language = shape });\n\
         @acceptanceCriteria.add({ behavior = `the file holds` });\nd A {}\n",
        &[],
        Origin::Program,
    ));
    let mut workspace = workspace_of(sources);
    let declaration = entity_named(&workspace.model, "rust");
    // The const carries the layer's criterion before lowering.
    assert_eq!(
        workspace.model.entities[declaration]
            .acceptance_criteria
            .len(),
        1,
        "{:?}",
        workspace.model.entities[declaration].acceptance_criteria
    );
    workspace.targets.push(crate::workspace::Target {
        identifier: "rust".to_string(),
        declaration,
        layers: vec![entity_named(&workspace.model, "shape")],
        output_directory: "crates".to_string(),
        extensions: Vec::new(),
        marker_comment: "//".to_string(),
        script_runner: None,
        native_dependencies: Vec::new(),
        commands: Vec::new(),
        knowledge: Vec::new(),
    });
    let program = lower(workspace);
    // The file's own criterion is still a requirement; the target's guidance is not.
    let ids: Vec<&str> = program
        .files
        .iter()
        .flat_map(|file| file.criteria.iter())
        .map(|criterion| criterion.id.as_str())
        .collect();
    assert_eq!(ids.len(), 1, "{ids:?}");
    assert!(
        every(&program.files[program.files.len() - 1].root)
            .iter()
            .all(|node| node.criteria.is_empty()),
        "no node carries the target's guidance"
    );
}

// @lfy def/interpret/main.lfy:lower#lower:lower:958d30dcc98b23d0e9373cc5d9ee9503e4e30d4587d485a2f01b9a25cdb0a2d8
#[test]
fn a_test_of_an_entity_is_lowered_once_with_its_id_and_its_values() {
    let program = lower(workspace(
        "fn f(): `Gives a number` => number {\n  @test({ input = [1], expect = 2 });\n}\n",
    ));
    let file = &program.files[0];
    let function = file
        .root
        .nodes()
        .find(|node| node.rule == S::AgentFunctionDeclaration.entity())
        .expect("the fn is kept");
    assert_eq!(function.tests.len(), 1, "{:?}", function.tests);
    let case = &function.tests[0];
    assert!(case.id.starts_with("f:f:"), "{}", case.id);
    assert_eq!(case.scope, RequirementScope::Local);
    assert_eq!(case.input_text, "[1]");
    assert_eq!(case.expect_text, "2");
    assert_eq!(
        case.input_value,
        Evaluated::Value(Value::List(vec![Value::Number(1.0)]))
    );
    assert_eq!(case.expect_value, Evaluated::Value(Value::Number(2.0)));
    // And never into the tree's children.
    assert!(
        every(&file.root)
            .iter()
            .all(|node| node.rule != S::Block.entity()),
        "the body of a fn is not kept"
    );
}

// @lfy def/interpret/main.lfy:lower#lower:lower:74f66400f215c724d23d2684a36b581975ecba82a95f30ba65103866b8f046b6
#[test]
fn a_criterion_for_a_files_own_entity_joins_the_file() {
    let program = lower(workspace(
        "@acceptanceCriteria.add({ behavior = `the file holds` });\nd A {}\n",
    ));
    let file = &program.files[0];
    assert_eq!(file.criteria.len(), 1, "{:?}", file.criteria);
    assert!(
        file.criteria[0].id.starts_with("a.lfy:a.lfy:"),
        "{}",
        file.criteria[0].id
    );
    assert_eq!(file.criteria[0].scope, RequirementScope::Local);
    for node in every(&file.root) {
        assert!(node.criteria.is_empty(), "{:?}", node.criteria);
    }
}

// @lfy def/interpret/main.lfy:lower#lower:lower:8bfaca8a3f0160f288d5a8a901bb6bc7b79163d2bdbed64540cdb5a0802ef829
#[test]
fn a_test_for_a_files_own_entity_joins_the_file() {
    let program = lower(workspace("@test({ input = [], expect = 1 });\nd A {}\n"));
    let file = &program.files[0];
    assert_eq!(file.tests.len(), 1, "{:?}", file.tests);
    assert!(
        file.tests[0].id.starts_with("a.lfy:a.lfy:"),
        "{}",
        file.tests[0].id
    );
    assert_eq!(file.tests[0].scope, RequirementScope::Local);
    assert!(program.tests.is_empty(), "{:?}", program.tests);
    for node in every(&file.root) {
        assert!(node.tests.is_empty(), "{:?}", node.tests);
    }
}

// @lfy def/interpret/main.lfy:lower#lower:lower:5427d71f70b3ddb4ee8f7b3accfd74c70441030a3d7c200e412ec249d90ce6f5
#[test]
fn a_call_of_like_is_one_folded_node_holding_its_prompted() {
    let program = lower(workspace("d A {}\nconst s = A@like(`something an A holds`);\n"));
    let file = &program.files[0];
    let a = entity_named(&program.workspace.model, "A");
    let folded: Vec<&LoweredNode> = every(&file.root)
        .into_iter()
        .filter(|node| node.is_folded())
        .collect();
    assert_eq!(folded.len(), 1, "{folded:?}");
    assert_eq!(
        folded[0].value,
        Some(Evaluated::Prompted(Prompted {
            value_type: a,
            prompt: "something an A holds".to_string(),
        }))
    );
    assert!(
        file.text.contains("A@like(`something an A holds`)"),
        "{}",
        file.text
    );
}

// @lfy def/interpret/main.lfy:lower#lower:lower:62d2d27335ac81028f0c9e82819680564bcc9debd24a3e002e179a810efaa49f
#[test]
fn runtime_code_that_calls_an_ace_function_with_no_compile_time_argument_is_a_problem() {
    let program = lower(workspace(
        "ace function q(t: string) -> string {\n  return t + \"!\";\n}\n\
         function f(x: string) -> string {\n  return q(x);\n}\n",
    ));
    assert_eq!(program.problems.len(), 1, "{:?}", program.problems);
    assert!(
        program.problems[0].message.contains("runs at compile time"),
        "{}",
        program.problems[0].message
    );
    let model = &program.workspace.model;
    assert!(model.is(program.problems[0].node, E::Call));
}

/// The other half of the same situation: a function declared outside the program's
/// runtime code runs wherever its call is written, so it will not be there at runtime
/// either, though nothing about it is `ace`.
// @lfy def/interpret/main.lfy:lower#lower:lower:62d2d27335ac81028f0c9e82819680564bcc9debd24a3e002e179a810efaa49f
#[test]
fn runtime_code_that_calls_a_package_function_with_no_compile_time_argument_is_a_problem() {
    let library = source(
        "lib/wrap.lfy",
        "function wrap(held: string): `What it was given` -> string {\n  return held;\n}\n",
        &[],
        Origin::Library,
    );
    let main = source(
        "lib/main.lfy",
        "use \"./wrap\";",
        &[Some("lib/wrap.lfy")],
        Origin::Prelude,
    );
    let program = source(
        "a.lfy",
        "function f(x: string) -> string {\n  return wrap(x);\n}\n",
        &[],
        Origin::Program,
    );
    let lowered = lower(workspace_of(vec![library, main, program]));
    assert_eq!(lowered.problems.len(), 1, "{:?}", lowered.problems);
    assert!(
        lowered.problems[0]
            .message
            .contains("runs at compile time"),
        "{}",
        lowered.problems[0].message
    );
    let model = &lowered.workspace.model;
    assert!(model.is(lowered.problems[0].node, E::Call));
}

// @lfy def/interpret/main.lfy:lower#lower:lower:699284343b9ef81cb11db5668663aac649e418507ad5cea495ddceee254e4d43
#[test]
fn a_test_added_to_globals_context_is_global_and_on_no_node() {
    let program = lower(workspace(
        "global@test({ input = [], expect = 1 });\nd A {}\n",
    ));
    assert_eq!(program.tests.len(), 1, "{:?}", program.tests);
    let case = &program.tests[0];
    assert!(case.id.starts_with("global:"), "{}", case.id);
    assert_eq!(case.scope, RequirementScope::Global);
    assert_eq!(case.entity, None);
    for file in &program.files {
        assert!(file.tests.is_empty(), "{:?}", file.tests);
        for node in every(&file.root) {
            assert!(node.tests.is_empty(), "{:?}", node.tests);
        }
    }
}

/// The whole repository lowers: one file per file, every origin a node of its own file,
/// every id its own, and every lowered file still Elfie.
// @lfy def/interpret/main.lfy:lower#lower:lower:958d30dcc98b23d0e9373cc5d9ee9503e4e30d4587d485a2f01b9a25cdb0a2d8
#[test]
fn the_repository_lowers_to_elfie_with_an_origin_for_everything() {
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let mut loaded = crate::workspace::load(root);
    // The model lowering reads is the model the expand pass left.
    loaded.model = expand(declared(loaded.model.sources.clone()));
    let program = lower(loaded);
    let model = &program.workspace.model;
    assert_eq!(program.files.len(), program.workspace.files.len());
    assert!(program.files.len() > 20, "{} files", program.files.len());
    let mut ids: Vec<&str> = Vec::new();
    for (index, file) in program.files.iter().enumerate() {
        assert_eq!(file.file, index);
        let source = program.workspace.files[index].source;
        for node in every(&file.root) {
            // Every origin names a node of the program: the file's own, or, for a member
            // a trait added, the trait's.
            assert!(node.origin.file < model.sources.len(), "an origin of no file");
            assert!(
                node.origin.index < model.nodes[node.origin.file].len(),
                "an origin that names no node"
            );
            if !is_member_node(model, node) {
                assert_eq!(node.origin.file, source, "an origin of another file");
            }
            // A folded node holds a value and no children; a kept one the other way.
            assert!(!(node.is_folded() && !node.children.is_empty()));
            ids.extend(node.criteria.iter().map(|c| c.id.as_str()));
            ids.extend(node.tests.iter().map(|t| t.id.as_str()));
        }
        ids.extend(file.criteria.iter().map(|c| c.id.as_str()));
        ids.extend(file.tests.iter().map(|t| t.id.as_str()));
        // The lowered text is still Elfie.
        let path = program.workspace.files[index].path.clone();
        let tokens = crate::lexer::lex(&file.text, Some(&path))
            .unwrap_or_else(|error| panic!("{path} does not lex: {error}"));
        let tree = crate::parser::parse(tokens, None);
        assert!(
            tree.errors.is_empty(),
            "the lowered {path} does not parse:\n{}\n{}",
            file.text,
            tree.render()
        );
    }
    ids.extend(program.criteria.iter().map(|c| c.id.as_str()));
    ids.extend(program.tests.iter().map(|t| t.id.as_str()));
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    let repeated: Vec<&str> = sorted
        .windows(2)
        .filter(|pair| pair[0] == pair[1])
        .map(|pair| pair[0])
        .collect();
    assert!(repeated.is_empty(), "two requirements share an id: {repeated:#?}");
    assert!(!program.criteria.is_empty(), "the program has global criteria");
    let messages: Vec<String> = program
        .problems
        .iter()
        .map(|problem| format!("{problem:?}"))
        .collect();
    assert!(program.problems.is_empty(), "{messages:#?}");
}

// @lfy def/interpret/main.lfy:lower
#[test]
fn a_criterion_keeps_the_entities_it_names_as_links() {
    let program = lower(workspace(
        "d Other {}\nfn f(): `Does it` => number {\n  where (`asked`) -> `reads [[Other]]`;\n}\n",
    ));
    let file = &program.files[0];
    let model = &program.workspace.model;
    let other = entity_named(model, "Other");
    let function = file
        .root
        .nodes()
        .find(|node| node.rule == S::AgentFunctionDeclaration.entity())
        .expect("the fn is kept");
    assert_eq!(function.criteria.len(), 1, "{:?}", function.criteria);
    let criterion = &function.criteria[0];
    assert_eq!(criterion.references, [other]);
    // The texts are held as rendered, with each reference replaced by its name.
    assert_eq!(
        criterion.behavior.as_deref(),
        Some(["reads Other".to_string()].as_slice())
    );
}
