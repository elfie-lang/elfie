// @lfy def/interpret/main.lfy:phaseOf
//! The tests of phaseOf.

use super::fixtures::*;
use super::phase_of::{CompileCall, compile_time_call};
use super::*;

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:e296a93f89da71bf1df9226c1abb709c149b656286c7a3d31029d1a5b088a47a
#[test]
fn a_trait_declaration_runs_at_compile_time() {
    let model = bound("trait t {}\n");
    let declaration = first(&model, 0, S::TraitDeclaration);
    assert_eq!(phase_of(&model, declaration), Phase::Compile);
}

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:18bed760f0d522c88d3e7716bb2d269bdb11b37a55d0d21f716a1eb4c22a2ae3
#[test]
fn a_statement_directly_in_a_file_that_declares_nothing_runs_at_compile_time() {
    let model = bound("trait scoped {}\nd SourceFile {}\nscoped.apply(SourceFile);\n");
    let statement = first(&model, 0, S::ExpressionStatement);
    assert_eq!(phase_of(&model, statement), Phase::Compile);
    // The apply call inside it is compile time in its own right.
    let call = first(&model, 0, E::Call);
    assert_eq!(compile_time_call(&model, call), Some(CompileCall::Apply));
    assert_eq!(phase_of(&model, call), Phase::Compile);
}

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:c97e9f5136290c1e11a2756a6ba9e7ca228e99d5b6985ad7ea51c45b628a9249
#[test]
fn the_body_of_a_written_function_is_kept_for_runtime() {
    let model = bound("function f() -> number {\n  return 1;\n}\n");
    let statement = first(&model, 0, S::Return);
    assert_eq!(phase_of(&model, statement), Phase::Runtime);
    // The declaration that holds it is runtime too.
    let declaration = first(&model, 0, S::FunctionDeclaration);
    assert_eq!(phase_of(&model, declaration), Phase::Runtime);
}

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:91de1d9e717023c54a963aa58631c2ee9d223c44cbf41ccbf14570dbd4ebbb86
#[test]
fn a_where_runs_at_compile_time() {
    let model = bound("fn f() => number {\n  where (`a`) -> `b`;\n}\n");
    let statement = first(&model, 0, S::Where);
    assert_eq!(phase_of(&model, statement), Phase::Compile);
    // A declaration whose body is compile time is itself runtime.
    let declaration = first(&model, 0, S::AgentFunctionDeclaration);
    assert_eq!(phase_of(&model, declaration), Phase::Runtime);
}

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:3e3bc11466542f33b6a34c37871336e917685651122b0fb123a4f7b811c4de89
#[test]
fn a_file_level_const_is_runtime() {
    let model = bound("const c = 1;\n");
    let declaration = first(&model, 0, S::VariableDeclaration);
    assert_eq!(phase_of(&model, declaration), Phase::Runtime);
}

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:2a01a9a7afcbd15cc69c025b0cf560d009f74520faf3acd45a070b666c52d23b
#[test]
fn a_data_declaration_is_runtime_though_what_its_body_holds_is_not() {
    let model = bound("d A {\n  $x: `d` = string;\n}\n");
    let declaration = first(&model, 0, S::DataDeclaration);
    assert_eq!(phase_of(&model, declaration), Phase::Runtime);
}

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:e296a93f89da71bf1df9226c1abb709c149b656286c7a3d31029d1a5b088a47a
#[test]
fn the_body_of_a_data_and_the_clauses_of_a_declaration_run_at_compile_time() {
    let model = bound("trait t {}\nd A is t {\n  $x: `A member` = string;\n}\n");
    let is_clause = first(&model, 0, E::IsClause);
    assert_eq!(phase_of(&model, is_clause), Phase::Compile);
    let block = first(&model, 0, S::Block);
    assert_eq!(phase_of(&model, block), Phase::Compile);
    // And so does the member declaration inside it.
    let member = first(&model, 0, E::Assignment);
    assert_eq!(phase_of(&model, member), Phase::Compile);
}

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:e296a93f89da71bf1df9226c1abb709c149b656286c7a3d31029d1a5b088a47a
#[test]
fn an_extends_clause_an_ace_and_a_with_run_at_compile_time() {
    let model = bound(
        "trait base {}\ntrait t extends base {}\nd A {}\nace const c = 1;\n\
         with A {\n  where (`a`) -> `b`;\n}\n",
    );
    assert_eq!(phase_of(&model, first(&model, 0, E::ExtendsClause)), Phase::Compile);
    assert_eq!(phase_of(&model, first(&model, 0, S::Ace)), Phase::Compile);
    assert_eq!(phase_of(&model, first(&model, 0, S::With)), Phase::Compile);
    // The declaration the ace holds runs at compile time with it.
    let ace = first(&model, 0, S::Ace);
    let held = model.child_nodes(ace).into_iter().next().expect("a statement");
    assert_eq!(phase_of(&model, held), Phase::Compile);
}

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:e296a93f89da71bf1df9226c1abb709c149b656286c7a3d31029d1a5b088a47a
#[test]
fn a_call_of_add_on_criteria_and_of_test_on_a_context_run_at_compile_time() {
    let model = bound(
        "fn f(): `Does it` => number {\n  @acceptanceCriteria.add({ behavior = `holds` });\n\
         \n  @test({ input = [], expect = 1 });\n}\n",
    );
    // Knowledge, commands, and targets are added the same way and run the same way.
    let others = bound(
        "d A {\n  @knowledge.add({ topic = `a guide` });\n\
         \n  @commands.add({ operation = Operation.build });\n  @targets.add(A);\n}\n",
    );
    let added: Vec<Option<CompileCall>> = others
        .descendants(NodeRef { file: 0, index: 0 })
        .into_iter()
        .filter(|&node| others.is(node, E::Call))
        .map(|call| {
            assert_eq!(phase_of(&others, call), Phase::Compile);
            compile_time_call(&others, call)
        })
        .collect();
    assert_eq!(added, vec![Some(CompileCall::Add); 3], "{added:?}");
    let calls: Vec<NodeRef> = model
        .descendants(NodeRef { file: 0, index: 0 })
        .into_iter()
        .filter(|&node| model.is(node, E::Call))
        .collect();
    let kinds: Vec<Option<CompileCall>> = calls
        .iter()
        .map(|&call| compile_time_call(&model, call))
        .collect();
    assert!(kinds.contains(&Some(CompileCall::Add)), "{kinds:?}");
    assert!(kinds.contains(&Some(CompileCall::Test)), "{kinds:?}");
    for call in calls {
        assert_eq!(phase_of(&model, call), Phase::Compile);
    }
}

// @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:5357c505ea2cc6797f85156d4c4bba6d26483c5ab8599dbf3d34660ece727fa0
#[test]
fn a_template_reference_in_a_definition_runs_at_compile_time() {
    let model = bound("d A {}\nfunction f(): `Reads [[A]]` -> number {\n  return 1;\n}\n");
    let reference = first(&model, 0, E::TemplateReference);
    assert_eq!(phase_of(&model, reference), Phase::Compile);
    // The function it describes is still kept.
    let declaration = first(&model, 0, S::FunctionDeclaration);
    assert_eq!(phase_of(&model, declaration), Phase::Runtime);
}
