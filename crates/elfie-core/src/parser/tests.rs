//! Tests of the criteria of `def/parser/traits.lfy`, `def/parser/data.lfy` and the
//! grammar clauses the parser applies.

use super::*;
use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::statement::Statement;

// @lfy def/parser/traits.lfy:recoverable
#[test]
fn a_required_element_error_covers_up_to_the_sync_at_the_same_bracket_depth() {
    // The `;` inside the call is not at the same depth as the call's missing `)`.
    let source = "const x = f(a; b); y;";
    let tree = file(source);
    check_lossless(&tree, source);
    let call = first_statement(&tree).find(Expression::Call).unwrap();
    assert_eq!(shape(call, &tree), ["Name", "GroupOpen(()", "Items", "Error(\"; b\")", "GroupClose())"]);
    assert_eq!(tree.errors.len(), 1);
    assert_eq!(tree.errors[0].expected, vec!["GroupClose"]);
    // @lfy def/parser/traits.lfy:recoverable
    // A closing bracket that would take the depth below 0 ends the error before it.
    let tree = file("for (x y) { }");
    let for_ = first_statement(&tree);
    assert_eq!(
        shape(for_, &tree),
        ["ForKeyword(for)", "GroupOpen(()", "Declared", "Error(\"y\")", "GroupClose())", "Block"]
    );
    assert_eq!(tree.errors[0].expected, vec!["InKeyword", "OfKeyword", "Comma"]);
    // The remaining elements are tried as if optional, the failed one included.
    let tree = file("(a b) => c;");
    let parameters = first_statement(&tree).find(Expression::Parameters).unwrap();
    assert_eq!(shape(parameters, &tree), ["GroupOpen(()", "Parameter", "Error(\"b\")", "GroupClose())"]);
    assert_eq!(tree.errors.len(), 1);
}

// @lfy def/parser/traits.lfy:recoverable
#[test]
fn a_required_element_error_covers_up_to_the_end_of_the_input_without_a_sync() {
    // Nothing after the failed element is a sync token of the call at the same depth, so
    // the error covers every remaining token.
    let source = "const x = f(a b";
    let tree = file(source);
    check_lossless(&tree, source);
    let call = first_statement(&tree).find(Expression::Call).unwrap();
    assert_eq!(shape(call, &tree), ["Name", "GroupOpen(()", "Items", "Error(\"b\")"]);
    let error = call.errors()[0];
    assert_eq!(error.expected, vec!["GroupClose"]);
    assert_eq!(error.end, tree.tokens.len());
}

// @lfy def/parser/traits.lfy:recoverable
#[test]
fn a_stopped_repetition_ends_or_covers_the_stopping_tokens() {
    // A token an element after the repetition begins with ends it.
    let tree = file("{ a; }");
    assert!(tree.errors.is_empty());
    // @lfy def/parser/traits.lfy:recoverable
    // A sync token no element after can begin with is an error alone.
    let source = "{ a; ; b; } ;";
    let tree = file(source);
    check_lossless(&tree, source);
    let block = first_statement(&tree);
    assert_eq!(
        shape(block, &tree),
        ["BlockOpen({)", "ExpressionStatement", "Error(\";\")", "ExpressionStatement", "BlockClose(})"]
    );
    assert_eq!(shape(&tree.root, &tree), ["Block", "Error(\";\")"]);
    assert_eq!(tree.errors.len(), 2);
    assert!(tree.errors[0].expected.contains(&"Identifier"));
    assert!(tree.errors[0].expected.contains(&"BlockClose"));
    // @lfy def/parser/traits.lfy:recoverable
    // Any other token is covered up to the next token that begins the repeated element,
    // an element after it, or a sync token.
    let source = "{ ) ] a; }";
    let tree = file(source);
    check_lossless(&tree, source);
    let block = first_statement(&tree);
    assert_eq!(
        shape(block, &tree),
        ["BlockOpen({)", "Error(\") \")", "Error(\"] \")", "ExpressionStatement", "BlockClose(})"]
    );
    let tree = file(") a;");
    assert_eq!(shape(&tree.root, &tree), ["Error(\") \")", "ExpressionStatement"]);
}

// @lfy def/parser/data.lfy:ErrorNode.expected
#[test]
fn invalid_tokens_inside_expressions_expect_an_operator_or_an_operand() {
    let tree = file("a # + b;");
    let operation = first_statement(&tree).node(Expression::AdditiveOperation).unwrap();
    assert_eq!(shape(operation, &tree), ["Name", "Error(\"#\")", "Plus(+)", "Name"]);
    let expected = &tree.errors[0].expected;
    assert!(expected.contains(&"Plus") && expected.contains(&"ValueAccessor") && expected.contains(&"PlainSetter"));
    assert!(!expected.contains(&"Identifier"));
    let tree = file("a + # b;");
    assert!(tree.errors[0].expected.contains(&"Identifier"));
    assert!(!tree.errors[0].expected.contains(&"Plus"));
    let tree = file("!#a;");
    let not = first_statement(&tree).node(Expression::NotOperation).unwrap();
    assert_eq!(shape(not, &tree), ["LogicalNot(!)", "Error(\"#\")", "Name"]);
    assert!(tree.errors[0].expected.contains(&"Identifier"));
    // Inside a right operand the minimum is in force: after `a * b` only stronger
    // operators could continue.
    let tree = file("a * b # ** c;");
    let expected = &tree.errors[0].expected;
    assert!(expected.contains(&"Power") && expected.contains(&"GroupOpen"));
    assert!(!expected.contains(&"Plus") && !expected.contains(&"Star"));
}

// @lfy def/parser/data.lfy:ErrorNode.expected
#[test]
fn an_invalid_token_inside_an_alternation_expects_every_alternative_and_what_follows() {
    let tree = file("`a\\qb`;");
    let expected = &tree.errors[0].expected;
    assert_eq!(expected, &["Backtick", "ExecutionOpen", "ReferenceOpen", "TemplateBody"]);
    // In a call with no arguments yet, an operand or the closing bracket could continue.
    let tree = file("f(#);");
    let expected = &tree.errors[0].expected;
    assert!(expected.contains(&"Identifier") && expected.contains(&"GroupClose"));
    // Between statements of a block, a statement or the closing brace.
    let tree = file("{ # a; }");
    let expected = &tree.errors[0].expected;
    assert!(expected.contains(&"ConstKeyword") && expected.contains(&"BlockClose"));
    // After a declared name the open rule is the declaration: its setter or semicolon.
    let tree = file("const x # ;");
    let expected = &tree.errors[0].expected;
    assert_eq!(expected, &["Semicolon", "PlainSetter"]);
    // After the statements of a block, another statement or the closing brace.
    let tree = file("{ a; # }");
    let expected = &tree.errors[0].expected;
    assert!(expected.contains(&"ConstKeyword") && expected.contains(&"BlockClose"));
    // Several optional clauses at one position: the first spans them all.
    let tree = file("d X # { }");
    // @lfy def/grammar/rules/statement.lfy:DataDeclaration
    // The `TypeParameters` of a declaration are the first of them.
    assert_eq!(
        tree.errors[0].expected,
        &["IsKeyword", "ExtendsKeyword", "BlockOpen", "Semicolon", "Colon", "LessThan"]
    );
    let tree = file("(a) # => a;");
    assert_eq!(
        tree.errors[0].expected,
        &["IsKeyword", "Colon", "SingleArrowRight", "SingleArrowRightGlyph", "DoubleArrowRight"]
    );
    let tree = file("(a # = 1) => a;");
    assert_eq!(tree.errors[0].expected, &["Colon", "QuestionMark", "PlainSetter"]);
    // Inside a reference only the admitted operations could continue: no call.
    let tree = file("const x: T # [] = 1;");
    let expected = &tree.errors[0].expected;
    assert!(expected.contains(&"ValueAccessor") && expected.contains(&"ListOpen"));
    assert!(!expected.contains(&"GroupOpen") && !expected.contains(&"Plus"));
}

// @lfy def/grammar/traits.lfy:prefix
#[test]
fn an_invalid_token_after_a_prefix_operator_is_not_space() {
    let tree = file("x = -#1;");
    let negate = first_statement(&tree).find(Expression::NegateOperation).unwrap();
    assert_eq!(shape(negate, &tree), ["Minus(-)", "Error(\"#\")", "Number"]);
    assert_eq!(tree.errors.len(), 1);
}

// @lfy def/parser/traits.lfy:recoverable
#[test]
fn expected_lists_what_could_have_continued_the_open_rule() {
    let tree = file("a #");
    assert_eq!(tree.errors[0].expected, vec!["Semicolon"]);
    let tree = file("const #");
    assert_eq!(tree.errors[0].expected, vec!["Identifier"]);
    let tree = file("break }");
    assert_eq!(tree.errors[0].expected, vec!["Semicolon"]);
    let statement_starts = &tree.errors[1].expected;
    for terminal in ["AgentDataKeyword", "ConstKeyword", "WhereKeyword", "Identifier", "BlockOpen", "Minus"] {
        assert!(statement_starts.contains(&terminal), "{terminal}: {statement_starts:?}");
    }
    assert!(!statement_starts.contains(&"BlockClose"));
    assert!(!statement_starts.contains(&"Plus"));
    let tree = file("return;");
    assert!(tree.errors[0].expected.contains(&"Identifier"));
    assert!(tree.errors[0].expected.contains(&"LogicalNot"));
    assert!(!tree.errors[0].expected.contains(&"Semicolon"));
    // @lfy def/parser/traits.lfy:recoverable
    // A required element that cannot be satisfied names everything that could have
    // continued the open rule at that index, the optional elements skipped there
    // included, exactly as an invalid token at the same index does.
    for source in ["const x", "const x # ;"] {
        let tree = file(source);
        assert_eq!(tree.errors[0].expected, vec!["Semicolon", "PlainSetter"], "{source}");
    }
    for source in ["d X", "d X # { }"] {
        let tree = file(source);
        assert_eq!(
            tree.errors[0].expected,
            vec!["IsKeyword", "ExtendsKeyword", "BlockOpen", "Semicolon", "Colon", "LessThan"],
            "{source}"
        );
    }
}

// trivia

// @lfy def/parser/traits.lfy:trivia
#[test]
fn trivia_is_not_taken_inside_rules_that_reference_a_body_terminal() {
    let source = "`a {{ b }} c`;";
    let tree = file(source);
    check_lossless(&tree, source);
    let template = first_statement(&tree).node(Expression::Template).unwrap();
    assert!(template.children.iter().all(|child| child.as_token().is_none_or(|index| {
        !crate::grammar::rules().any(|_| false) && !tree.token(index).is(crate::grammar::terminals::space::Space::Space)
    })));
    let execution = template.node(Expression::TemplateExecution).unwrap();
    assert_eq!(execution.children.len(), 5);
    assert!(tree.token(execution.children[1].as_token().unwrap()).is(crate::grammar::terminals::space::Space::Space));
    assert!(tree.errors.is_empty());
}

// @lfy def/grammar/terminals/comment.lfy:Documentation
#[test]
fn a_new_line_inside_a_line_documentation_reference_is_not_trivia() {
    let source = "/// see [[a\nb]] c\nx;";
    let tree = file(source);
    check_lossless(&tree, source);
    let documentation = tree.root.node(crate::grammar::terminals::comment::Comment::Documentation).unwrap();
    let reference = documentation.node(Expression::TemplateReference).unwrap();
    assert_eq!(
        shape(reference, &tree),
        ["ReferenceOpen([[)", "Reference", "Error(\"\\nb\")", "ReferenceClose(]])"]
    );
    assert_eq!(tree.errors.len(), 1);
    // Inside block documentation the line break is trivia.
    let source = "/** see [[a\n.b]] **/\nx;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
}

// documented

#[test]
fn documentation_before_a_statement_with_only_trivia_between_is_attached() {
    let source = "/// a\n/// b\n\n// c\nconst x;\ny;\n/** d **/ z;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statements: Vec<&Node> = tree.root.nodes().filter(|node| node.rule.is_statement()).collect();
    assert_eq!(statements.len(), 3);
    let docs: Vec<String> = statements[0]
        .documentation
        .iter()
        .map(|doc| tree.raw(doc.start, doc.end))
        .collect();
    assert_eq!(docs, ["/// a", "/// b"]);
    assert!(statements[1].documentation.is_empty());
    assert_eq!(statements[2].documentation.len(), 1);
    assert_eq!(tree.raw(statements[2].documentation[0].start, statements[2].documentation[0].end), "/** d **/");
    // Inside a block, and for a nested statement.
    let tree = file("{ /// e\n async /// f\n break; }");
    let block = first_statement(&tree);
    assert!(block.documentation.is_empty());
    let async_ = block.node(Statement::Async).unwrap();
    assert_eq!(async_.documentation.len(), 1);
    let break_ = async_.node(Statement::Break).unwrap();
    assert_eq!(break_.documentation.len(), 1);
    assert_eq!(tree.raw(break_.documentation[0].start, break_.documentation[0].end), "/// f");
    // Documentation nodes are children too, so every token is still reached once.
    assert!(tree.errors.is_empty());
}

/// The raw text of the documentation attached to a node.
fn attached(node: &Node, tree: &Tree) -> Vec<String> {
    node.documentation
        .iter()
        .map(|documentation| tree.raw(documentation.start, documentation.end))
        .collect()
}

/// The first statement of a file, documentation before it skipped.
fn first_declaration(tree: &Tree) -> &Node {
    tree.root
        .nodes()
        .find(|node| node.rule.is_statement())
        .expect("a statement")
}

// @lfy def/grammar/terminals/comment.lfy:Documentation
#[test]
fn documentation_attaches_to_the_declaration_that_follows_it() {
    let source = "/** a **/\n// c\n\n/** b **/\n/// d\nconst x;\n";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let declaration = first_declaration(&tree);
    assert!(declaration.is(Statement::VariableDeclaration));
    assert_eq!(attached(declaration, &tree), ["/** a **/", "/** b **/", "/// d"]);
    // A declaration follows, so nothing is left for the "global" object.
    assert!(tree.root.documentation.is_empty());
}

// @lfy def/grammar/terminals/comment.lfy:Documentation
#[test]
fn documentation_that_no_declaration_follows_attaches_to_nothing() {
    for source in [
        // One block of documentation on its own.
        "const x;\n/** a **/\n",
        // Two blocks in a row, but not both opened with a `BlockDocumentationOpen`.
        "const x;\n/** a **/\n/// b\n",
        "const x;\n/// a\n/// b\n",
    ] {
        let tree = file(source);
        check_lossless(&tree, source);
        assert!(tree.errors.is_empty(), "{source:?}: {:?}", tree.errors);
        assert!(tree.root.documentation.is_empty(), "{source:?}");
        assert!(attached(first_declaration(&tree), &tree).is_empty(), "{source:?}");
    }
}

// @lfy def/grammar/terminals/comment.lfy:Documentation
#[test]
fn trailing_block_documentation_blocks_attach_to_the_global_object() {
    let source = "const x;\n/** a **/\n// c\n/** b **/\n";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    assert_eq!(attached(&tree.root, &tree), ["/** a **/", "/** b **/"]);
    assert!(attached(first_declaration(&tree), &tree).is_empty());
    // A whole file of nothing but documentation attaches to the "global" object too.
    let source = "/** a **/\n/** b **/\n/** c **/\n";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    assert_eq!(attached(&tree.root, &tree), ["/** a **/", "/** b **/", "/** c **/"]);
    // A declaration between them leaves only the run after it for the "global" object.
    let source = "/** a **/\nconst x;\n/** b **/\n/** c **/\n";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    assert_eq!(attached(&tree.root, &tree), ["/** b **/", "/** c **/"]);
    assert_eq!(attached(first_declaration(&tree), &tree), ["/** a **/"]);
}

// triedBefore

// @lfy def/parser/traits.lfy:triedBefore
#[test]
fn the_rule_tried_first_wins_and_the_other_is_tried_only_when_it_fails() {
    let tree = file("(a) => a; (a); where (a) -> b; {} x = {};");
    let statements: Vec<&Node> = tree.root.nodes().collect();
    // @lfy def/parser/traits.lfy:triedBefore
    // `InlineFunction` is tried before `Group` and does not fail here, so `Group` is not
    // tried at that index at all.
    assert!(statements[0].node(Expression::InlineFunction).is_some());
    assert!(statements[0].find(Expression::Group).is_none());
    // @lfy def/parser/traits.lfy:triedBefore
    // Here `InlineFunction` fails at the same index, so `Group` is tried there.
    assert!(statements[1].node(Expression::InlineFunction).is_none());
    assert!(statements[1].node(Expression::Group).is_some());
    let condition = statements[2].find(Statement::Condition).unwrap();
    assert_eq!(shape(condition, &tree), ["Group"]);
    assert!(statements[3].is(Statement::Block));
    assert!(statements[4].find(Expression::Object).is_some());
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let tree = file("where ((a) or (b)) and !(c) -> q;");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let conditions = first_statement(&tree).node(Statement::Conditions).unwrap();
    assert_eq!(shape(conditions, &tree), ["Condition", "AndKeyword(and)", "Condition"]);
    assert!(conditions.nodes().next().unwrap().node(Statement::ConditionGroup).is_some());
}

// Grammar clauses the parser applies

// @lfy def/grammar/rules/expression.lfy:Current
#[test]
fn a_member_name_is_only_taken_when_nothing_sits_between_it_and_the_accessor() {
    // Current: the member name is not matched across space, so `is` is a trait clause.
    let tree = file("if (!(. is binding) && (operator is binding)) { }");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let current = tree.root.find(Expression::Current).unwrap();
    assert_eq!(shape(current, &tree), ["ValueAccessor(.)"]);
    assert!(tree.root.find(Expression::Traits).is_some());
    let tree = file(". type;");
    let current = first_statement(&tree).node(Expression::Current).unwrap();
    assert_eq!(shape(current, &tree), ["ValueAccessor(.)"]);
    assert_eq!(tree.errors.len(), 1);
    let tree = file(".type;");
    assert_eq!(shape(first_statement(&tree).node(Expression::Current).unwrap(), &tree), ["ValueAccessor(.)", "TypeKeyword(type)"]);
    // Only identifiers and `type` are member names.
    let tree = file("x.is;");
    assert!(!tree.errors.is_empty());
    // @lfy def/grammar/traits.lfy:postfix
    // Member: with space before the member name the rule cannot be a match at all.
    let tree = file("b. c;");
    assert!(first_statement(&tree).find(Expression::Member).is_none());
    assert!(!tree.errors.is_empty());
    let tree = file("b.\n  c;");
    assert!(!tree.errors.is_empty());
    // A member access with nothing after the accessor still matches across a line break.
    let tree = file("b. ;");
    let member = first_statement(&tree).node(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Name", "ValueAccessor(.)"]);
    assert!(tree.errors.is_empty());
    // Space before the accessor never matters.
    let tree = file("items\n  .map(f)\n  .join(g);");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
}

// @lfy def/grammar/rules/expression.lfy:Group
#[test]
fn a_group_cannot_match_before_a_trait_clause_a_definition_or_a_double_arrow() {
    for source in ["(a) : T;", "(a) is t;", "(a) =>;"] {
        let tree = file(source);
        check_lossless(&tree, source);
        assert!(!tree.errors.is_empty(), "{source}");
        assert!(tree.root.find(Expression::Group).is_none(), "{source}");
    }
    let tree = file("(a) -> b;");
    assert!(tree.root.find(Expression::Group).is_some());
}

// @lfy def/grammar/traits.lfy:prefix
#[test]
fn negation_and_dereference_cannot_be_matched_across_space() {
    let tree = expr("-1");
    assert!(tree.root.is(Expression::NegateOperation));
    let tree = expr("- 1");
    assert!(tree.root.is(Expression::Expression));
    assert_eq!(tree.errors.len(), 1);
    let tree = expr("a - 1");
    assert!(tree.root.is(Expression::AdditiveOperation));
    let tree = expr("&x");
    assert!(tree.root.is(Expression::Dereference));
    let tree = expr("&\nx");
    assert!(!tree.errors.is_empty());
    let tree = expr("! a");
    assert!(tree.root.is(Expression::NotOperation));
}

// @lfy def/grammar/rules/expression.lfy:InlineFunction
#[test]
fn a_block_after_the_double_arrow_is_a_block_and_a_conditional_owns_its_colon() {
    let tree = expr("() => { a = 1; }");
    assert!(tree.root.is(Expression::InlineFunction));
    assert!(tree.root.node(Statement::Block).is_some());
    assert!(tree.root.node(Expression::Object).is_none());
    // @lfy def/grammar/rules/expression.lfy:Conditional
    let tree = expr("a ? x : T : c");
    assert!(tree.root.is(Expression::Definition));
    let conditional = tree.root.node(Expression::Conditional).unwrap();
    assert_eq!(shape(conditional, &tree), ["Name", "QuestionMark(?)", "Name", "Colon(:)", "Name"]);
}

// Generics

// @lfy def/grammar/rules/expression.lfy:Generic
#[test]
fn a_generic_matches_only_when_the_token_after_its_arguments_allows_it() {
    // @lfy def/grammar/rules/expression.lfy:Generic
    // `$items = List<T>;`: a `Semicolon` follows the `GreaterThan`.
    let source = "$items = List<T>;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let assignment = first_statement(&tree).node(Expression::Assignment).unwrap();
    let generic = assignment.node(Expression::Generic).unwrap();
    assert_eq!(
        shape(generic, &tree),
        ["Name", "LessThan(<)", "TypeExpression", "GreaterThan(>)"]
    );
    assert_eq!(tree.raw(generic.start, generic.end), "List<T>");
    // @lfy def/grammar/rules/expression.lfy:Generic
    // `f<string>(x)`: a `GroupOpen` follows, so the `Generic` is the called expression.
    let tree = expr("f<string>(x)");
    check_lossless(&tree, "f<string>(x)");
    assert!(tree.root.is(Expression::Call));
    let generic = tree.root.node(Expression::Generic).unwrap();
    assert_eq!(tree.raw(generic.start, generic.end), "f<string>");
    assert!(generic.find(Expression::PrimitiveType).is_some());
    // @lfy def/grammar/rules/expression.lfy:Generic
    // `a < b`: no `GreaterThan` follows `b`, so the `LessThan` is a comparison.
    let tree = expr("a < b");
    check_lossless(&tree, "a < b");
    assert!(tree.root.is(Expression::RelationalOperation));
    assert!(tree.root.find(Expression::Generic).is_none());
    assert!(tree.errors.is_empty());
    // @lfy def/grammar/rules/expression.lfy:Generic
    // `a < b > c`: the `Identifier` after the `GreaterThan` rules the `Generic` out, so
    // two comparisons read as `(a < b) > c`.
    let tree = expr("a < b > c");
    check_lossless(&tree, "a < b > c");
    assert!(tree.root.is(Expression::RelationalOperation));
    let left = tree.root.node(Expression::RelationalOperation).unwrap();
    assert_eq!(tree.raw(left.start, left.end), "a < b");
    assert!(tree.root.find(Expression::Generic).is_none());
    assert!(tree.errors.is_empty());
    // @lfy def/grammar/rules/expression.lfy:Generic
    // `List<T>=x` compares, it does not assign: the longest match lexes `>=` as one
    // token, which cannot close the arguments.
    let tree = expr("List<T>=x");
    check_lossless(&tree, "List<T>=x");
    assert!(tree.root.find(Expression::Generic).is_none());
    assert!(tree.root.find(Expression::Assignment).is_none());
    assert_eq!(tree.root.rule, Entity::Expression(Expression::RelationalOperation));
}

// @lfy def/grammar/rules/expression.lfy:Generic
#[test]
fn a_generic_binds_at_the_access_level() {
    let tree = expr("a + f<T>(x)");
    check_lossless(&tree, "a + f<T>(x)");
    assert!(tree.root.is(Expression::AdditiveOperation));
    let call = tree.root.node(Expression::Call).unwrap();
    assert_eq!(tree.raw(call.start, call.end), "f<T>(x)");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let tree = expr("x.f<T>(y)");
    check_lossless(&tree, "x.f<T>(y)");
    assert!(tree.root.is(Expression::Call));
    let generic = tree.root.node(Expression::Generic).unwrap();
    assert_eq!(tree.raw(generic.start, generic.end), "x.f<T>");
    assert!(generic.node(Expression::Member).is_some());
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
}

// @lfy def/grammar/rules/expression.lfy:TypeItem
#[test]
fn type_arguments_close_one_list_each_and_are_never_a_generic() {
    let source = "const x: List<List<T>>;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    // `TypeValue` is an alternation list, so the item it selected stands in its place.
    let outer = first_statement(&tree).find(Expression::TypeItem).unwrap();
    assert_eq!(shape(outer, &tree), ["Reference", "TypeArguments"]);
    let arguments = outer.node(Expression::TypeArguments).unwrap();
    assert_eq!(
        shape(arguments, &tree),
        ["LessThan(<)", "TypeExpression", "GreaterThan(>)"]
    );
    // @lfy def/grammar/rules/expression.lfy:TypeArguments
    // The two `GreaterThan` tokens close one list each.
    let inner = arguments.find(Expression::TypeItem).unwrap();
    assert_eq!(shape(inner, &tree), ["Reference", "TypeArguments"]);
    assert_eq!(tree.raw(inner.start, inner.end), "List<T>");
    assert!(first_statement(&tree).find(Expression::Generic).is_none());
    // @lfy def/grammar/rules/expression.lfy:TypeItem
    // `T[]` is the same type as `List<T>`. After a `Reference` the brackets are the
    // `Index` of it with nothing inside, which is the array type of it; the `TypeItem`
    // takes them itself only where no reference chain could.
    let source = "const x: T[];";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let item = first_statement(&tree).find(Expression::TypeItem).unwrap();
    assert_eq!(shape(item, &tree), ["Reference"]);
    let index = item.find(Expression::Index).unwrap();
    assert_eq!(shape(index, &tree), ["Name", "ListOpen([)", "ListClose(])"]);
}

// @lfy def/grammar/rules/expression.lfy:TypeGroup
#[test]
fn a_parenthesized_type_is_a_type_group_unless_a_function_type_follows() {
    let source = "const x: (A | B)[];";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let item = first_statement(&tree).find(Expression::TypeItem).unwrap();
    assert_eq!(shape(item, &tree), ["TypeGroup", "ListOpen([)", "ListClose(])"]);
    // @lfy def/grammar/rules/expression.lfy:FunctionType
    let source = "const x: (item: T) => U;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let statement = first_statement(&tree);
    assert!(statement.find(Expression::TypeGroup).is_none());
    let function = statement.find(Expression::FunctionType).unwrap();
    assert_eq!(
        shape(function, &tree),
        ["Parameters", "DoubleArrowRight(=>)", "TypeExpression"]
    );
}

// @lfy def/grammar/rules/statement.lfy:DataDeclaration#DataDeclaration:rule:c5da10f7899cb7a05130a701932ae7bfdb34403ee3dddb544acd06b36b7ff914
#[test]
fn a_declaration_takes_its_type_parameters_after_its_identifier() {
    let source = "d List<T>;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let declaration = first_statement(&tree);
    assert!(declaration.is(Statement::DataDeclaration));
    assert_eq!(
        shape(declaration, &tree),
        ["AgentDataKeyword(d)", "Identifier(List)", "TypeParameters", "Semicolon(;)"]
    );
    let parameters = declaration.node(Expression::TypeParameters).unwrap();
    let names: Vec<&Node> = parameters.nodes_of(Expression::TypeParameter).collect();
    assert_eq!(names.len(), 1);
    assert_eq!(shape(names[0], &tree), ["Identifier(T)"]);
    // @lfy def/grammar/rules/statement.lfy:TypeDeclaration
    let source = "type Pair<A, B extends object> { left: A = A, right: B = B }";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let declaration = first_statement(&tree);
    assert!(declaration.is(Statement::TypeDeclaration));
    let parameters = declaration.node(Expression::TypeParameters).unwrap();
    assert_eq!(parameters.nodes_of(Expression::TypeParameter).count(), 2);
    // @lfy def/grammar/rules/expression.lfy:TypeParameter
    let extends = parameters
        .nodes_of(Expression::TypeParameter)
        .nth(1)
        .unwrap();
    assert_eq!(
        shape(extends, &tree),
        ["Identifier(B)", "ExtendsKeyword(extends)", "TypeExpression"]
    );
}

// @lfy def/grammar/rules/expression.lfy:Signature
#[test]
fn a_signature_carries_its_type_parameters_and_typed_parameters() {
    let source = "fn map<T, U>(list: List<T>, transform: (item: T) => U) => List<U>;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let declaration = first_statement(&tree);
    assert!(declaration.is(Statement::AgentFunctionDeclaration));
    let signature = declaration.node(Expression::Signature).unwrap();
    assert_eq!(
        shape(signature, &tree),
        ["Identifier(map)", "TypeParameters", "Parameters"]
    );
    let type_parameters: Vec<String> = signature
        .node(Expression::TypeParameters)
        .unwrap()
        .nodes_of(Expression::TypeParameter)
        .map(|parameter| tree.raw(parameter.start, parameter.end))
        .collect();
    assert_eq!(type_parameters, ["T", "U"]);
    let parameters: Vec<&Node> = signature
        .node(Expression::Parameters)
        .unwrap()
        .nodes_of(Expression::Parameter)
        .collect();
    assert_eq!(parameters.len(), 2);
    // @lfy def/grammar/rules/expression.lfy:TypeItem
    let list = parameters[0].find(Expression::TypeItem).unwrap();
    assert_eq!(shape(list, &tree), ["Reference", "TypeArguments"]);
    assert_eq!(tree.raw(list.start, list.end), "List<T>");
    // @lfy def/grammar/rules/expression.lfy:FunctionType
    let function = parameters[1].find(Expression::FunctionType).unwrap();
    assert_eq!(tree.raw(function.start, function.end), "(item: T) => U");
    // The return type is `List` with the type argument `U`.
    let returned = declaration
        .node(Expression::TypeExpression)
        .expect("the return type");
    assert_eq!(tree.raw(returned.start, returned.end), "List<U>");
}

// data

// @lfy def/parser/data.lfy:Node
#[test]
fn nodes_expose_their_rule_and_error_nodes_cover_tokens() {
    let tree = file("a;");
    let statement = first_statement(&tree);
    assert_eq!(statement.rule().identifier, "ExpressionStatement");
    assert_eq!(statement.rule().text, Statement::ExpressionStatement.text());
    let tree = file("const ;");
    assert_eq!((tree.errors[0].start, tree.errors[0].end), (2, 2));
    assert!(tree.errors[0].children.is_empty());
}

// @lfy def/parser/data.lfy:Tree.errors
#[test]
fn errors_are_every_error_node_in_the_tree_ordered_by_start() {
    let source = "const = 1; { ) x } return;";
    let tree = file(source);
    check_lossless(&tree, source);
    let starts: Vec<usize> = tree.errors.iter().map(|error| error.start).collect();
    let mut sorted = starts.clone();
    sorted.sort_unstable();
    assert_eq!(starts, sorted);
    assert_eq!(tree.errors.len(), tree.root.errors().len());
    assert!(tree.errors.len() >= 4, "{starts:?}");
}
