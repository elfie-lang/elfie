// @lfy def/interpret/main.lfy:evaluate
//! The tests of evaluate.

use super::fixtures::*;
use super::*;
use crate::model::bind;

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:655f8825c2e70d824ae70edabe7443b7721c22be55823451930b947ed3672fc9
#[test]
fn an_arithmetic_expression_has_its_value_at_compile_time() {
    let mut model = bound("ace const c = 1 + 2;\n");
    let declaration = first(&model, 0, S::VariableDeclaration);
    let value = value_of(&model, declaration);
    assert_eq!(
        evaluate(&mut model, value),
        Some(Evaluated::Value(Value::Number(3.0)))
    );
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:4df2a9ef8751412bab3ea9167a05658df60985f6593960415aadb51eb6973c1b
// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:bb33b89909ec6ede4b06c592019c0381b73e99575350a939f2f20b06a6775738
#[test]
fn a_compile_time_value_is_read_before_the_declaration_that_holds_it() {
    let mut model = bound("ace const a = b + 1;\nace const b = 2;\n");
    let declaration = nth(&model, 0, S::VariableDeclaration, 0);
    let value = value_of(&model, declaration);
    assert_eq!(
        evaluate(&mut model, value),
        Some(Evaluated::Value(Value::Number(3.0)))
    );
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:721fecc6bbf7d7e099e34bedb0683a009eb68ac5cd6d2ab534899e76c11507a5
#[test]
fn a_node_is_evaluated_in_the_scope_it_is_written_in() {
    let mut model = bound(
        "ace const x = 1;\nfunction f(x: number) -> number {\n  return x;\n}\nconst n = x;\n",
    );
    // At the top of the file the name is the const.
    let outer = value_of(&model, nth(&model, 0, S::VariableDeclaration, 1));
    assert_eq!(
        evaluate(&mut model, outer),
        Some(Evaluated::Value(Value::Number(1.0)))
    );
    // The same name inside the function is its parameter, which only runtime answers.
    let statement = first(&model, 0, S::Return);
    let inner = model
        .child(statement, S::ExpressionStatement)
        .and_then(|s| model.child_nodes(s).into_iter().next())
        .expect("the returned expression");
    assert_eq!(evaluate(&mut model, inner), None);
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:2da60593210808873570bc274bbe86668ab461197169994b03a18e4b09823ed7
#[test]
fn a_layer_other_than_value_is_read_from_the_model() {
    let mut model = bound("d A {}\nconst n = A@identifier;\n");
    let declaration = nth(&model, 0, S::VariableDeclaration, 0);
    let value = value_of(&model, declaration);
    assert_eq!(
        evaluate(&mut model, value),
        Some(Evaluated::Value(Value::String("A".to_string())))
    );
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:2e1806da61d20fb7c02967b5644f3dedb864c40b7e7efe3474c52f3622d8e34b
#[test]
fn a_dereference_is_read_from_the_model() {
    let mut model = bound("d A {}\nconst n = (&A)@identifier;\n");
    let declaration = nth(&model, 0, S::VariableDeclaration, 0);
    let value = value_of(&model, declaration);
    assert_eq!(
        evaluate(&mut model, value),
        Some(Evaluated::Value(Value::String("A".to_string())))
    );
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:2e1806da61d20fb7c02967b5644f3dedb864c40b7e7efe3474c52f3622d8e34b
#[test]
fn the_previous_statement_is_read_from_the_model() {
    let mut model = bound("d A {}\nconst n = ^^@identifier;\n");
    let declaration = nth(&model, 0, S::VariableDeclaration, 0);
    let value = value_of(&model, declaration);
    assert_eq!(
        evaluate(&mut model, value),
        Some(Evaluated::Value(Value::String("A".to_string())))
    );
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:be4ebeec3fb42e5b9a113fd744d8da7f2b4767acf2f76bd04f0a11ba732663cb
#[test]
fn a_call_of_like_gives_a_prompted_of_the_entitys_type() {
    let mut model = bound_with_prelude("const s = string@like(`a word`);\n");
    let file = model.file("a.lfy").expect("the program file");
    let declaration = first(&model, file, S::VariableDeclaration);
    let value = value_of(&model, declaration);
    let string = entity_named(&model, "String");
    assert_eq!(
        evaluate(&mut model, value),
        Some(Evaluated::Prompted(Prompted {
            value_type: string,
            prompt: "a word".to_string(),
        }))
    );
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:3e472f6364107275a9eb1e7fbdf35e80d3d334ece89274fea50b202686c4c567
// @lfy def/interpret/main.lfy:expand#expand:expand:fbc7d19ecec253d62dde790e452968a3262f174b36abe019d91512e5e02c6165
#[test]
fn a_call_of_a_function_declared_outside_the_programs_runtime_code_is_run() {
    let library = source(
        "lib/wrap.lfy",
        "ace function wrap(held: object): `What it was given` -> object {\n  \
         return held;\n}\n",
        &[],
        Origin::Library,
    );
    let main = source(
        "lib/main.lfy",
        "use \"./wrap\";",
        &[Some("lib/wrap.lfy")],
        Origin::Prelude,
    );
    let program = source("a.lfy", "ace const c = wrap({ a = 1 });\n", &[], Origin::Program);
    let mut model = bind(vec![library, main, program]);
    let file = model.file("a.lfy").expect("the program file");
    let declaration = first(&model, file, S::VariableDeclaration);
    let value = value_of(&model, declaration);
    assert_eq!(
        evaluate(&mut model, value),
        Some(Evaluated::Value(Value::Object(BTreeMap::from([(
            "a".to_string(),
            Value::Number(1.0),
        )]))))
    );
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:3c659b6359b26f9db37a1d4d512e95bf301ec47c24dd44273f3d6bc486e21dc3
// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:b0d45fd642afa8465747a9758d2568b1b29d13daad273eafefebfa8349ce44f6
#[test]
fn the_same_node_twice_gives_the_same_value_and_the_model_is_not_changed() {
    let mut model = bound("d A {}\nconst n = A@identifier;\nconst m = n;\n");
    let declaration = nth(&model, 0, S::VariableDeclaration, 1);
    let value = value_of(&model, declaration);
    let before = model.clone();
    let once = evaluate(&mut model, value);
    let twice = evaluate(&mut model, value);
    assert_eq!(once, twice);
    assert_eq!(once, Some(Evaluated::Value(Value::String("A".to_string()))));
    assert_eq!(before, model, "evaluating changed the model");
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:762654352875a07eb98f9d3d15a3c8d3308f2be90e1f874cac1f1f33a2fe7f9d
#[test]
fn what_only_has_a_value_at_runtime_gives_nothing_back_and_adds_no_problem() {
    let mut model = bound("function f(x: number) -> number {\n  return x + 1;\n}\n");
    let statement = first(&model, 0, S::Return);
    let inner = model
        .child(statement, S::ExpressionStatement)
        .and_then(|s| model.child_nodes(s).into_iter().next())
        .expect("the returned expression");
    assert_eq!(evaluate(&mut model, inner), None);
    assert!(model.problems.is_empty(), "{:?}", model.problems);
}

// @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:6eabf4cd81466f895280d4dd2bf7be3d839f75a1f5ffcdfda67a6c92678b3cca
#[test]
fn a_value_that_needs_its_own_gives_nothing_back_and_names_the_cycle() {
    let mut model = bound("const a = b;\nconst b = a;\n");
    model.problems.clear();
    let declaration = nth(&model, 0, S::VariableDeclaration, 0);
    let value = value_of(&model, declaration);
    assert_eq!(evaluate(&mut model, value), None);
    assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
    assert!(
        model.problems[0].message.contains("needs its own value"),
        "{}",
        model.problems[0].message
    );
}
