// @lfy def/parser/main.lfy:parse
//! Tests of `parse`: its `@test` cases and the criteria of `def/parser/main.lfy`.

use super::*;
use crate::grammar::rules;
use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::file::File;
use crate::grammar::rules::statement::Statement;
use crate::grammar::terminals::identifier::Identifier;
use crate::grammar::terminals::keyword::Keyword;
use crate::grammar::terminals::literal::Literal;
use crate::grammar::terminals::punctuation::Punctuation;
use crate::lexer::lex;

// The @test cases

// @lfy def/parser/main.lfy:parse#parse:parse:29415d30cd4457403c7405b0d5b4f5d6038680fa973bf80c4adb68a701093953
#[test]
fn test_an_empty_expression() {
    let tree = expr("");
    assert_eq!(tree.root_rule, Entity::Expression(Expression::Expression));
    assert!(tree.root.is(Expression::Expression));
    assert!(tree.root.children.is_empty());
    assert_eq!((tree.root.start, tree.root.end), (0, 0));
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:dfa6647a253ffe17a8a50a22fcc3d3b63051982a51b67c9e1b827ce4b3edce40
#[test]
fn test_subtraction_is_left_associative() {
    let tree = expr("a - b - c");
    check_lossless(&tree, "a - b - c");
    assert!(tree.root.is(Expression::AdditiveOperation));
    assert_eq!(shape(&tree.root, &tree), ["AdditiveOperation", "Minus(-)", "Name"]);
    let left = tree.root.node(Expression::AdditiveOperation).unwrap();
    assert_eq!(shape(left, &tree), ["Name", "Minus(-)", "Name"]);
    assert_eq!(tree.raw(left.start, left.end), "a - b");
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:979850376ec546d4c9a1989b2fab045eca9790fd846c39564db796cb458098a1
#[test]
fn test_negation_binds_tighter_than_power() {
    let tree = expr("-a ** b");
    check_lossless(&tree, "-a ** b");
    assert!(tree.root.is(Expression::PowerOperation));
    assert_eq!(shape(&tree.root, &tree), ["NegateOperation", "Power(**)", "Name"]);
    let negate = tree.root.node(Expression::NegateOperation).unwrap();
    assert_eq!(shape(negate, &tree), ["Minus(-)", "Name"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:fb44ea90f405d58b277834d5797cbcc1ac66170ccb355f021053c8be691d4358
#[test]
fn test_definition_binds_tighter_than_a_setter() {
    let tree = expr("x : T = 1");
    check_lossless(&tree, "x : T = 1");
    assert!(tree.root.is(Expression::Assignment));
    assert_eq!(shape(&tree.root, &tree), ["Definition", "PlainSetter(=)", "Number"]);
    let definition = tree.root.node(Expression::Definition).unwrap();
    assert_eq!(shape(definition, &tree), ["Name", "Colon(:)", "Name"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:e08c50dbc1e9618672b3e6a96015718db5ff23e9dcf6f3074a65b63c069fd051
#[test]
fn test_an_identifier_cannot_follow_a_generic() {
    let tree = expr("a < b > c");
    check_lossless(&tree, "a < b > c");
    assert!(tree.root.is(Expression::RelationalOperation));
    assert_eq!(
        shape(&tree.root, &tree),
        ["RelationalOperation", "GreaterThan(>)", "Name"]
    );
    let left = tree.root.node(Expression::RelationalOperation).unwrap();
    assert_eq!(shape(left, &tree), ["Name", "LessThan(<)", "Name"]);
    assert_eq!(tree.raw(left.start, left.end), "a < b");
    assert!(tree.root.find(Expression::Generic).is_none());
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:cac8412790f13ab2827fddf78060e9a0cc9ca2a414a88befdd3c09b8084b7746
#[test]
fn test_a_call_of_a_generic() {
    let tree = expr("f<string>(x)");
    check_lossless(&tree, "f<string>(x)");
    assert!(tree.root.is(Expression::Call));
    assert_eq!(shape(&tree.root, &tree), ["Generic", "GroupOpen(()", "Items", "GroupClose())"]);
    let generic = tree.root.node(Expression::Generic).unwrap();
    assert_eq!(
        shape(generic, &tree),
        ["Name", "LessThan(<)", "TypeExpression", "GreaterThan(>)"]
    );
    assert!(generic.find(Expression::PrimitiveType).is_some());
    let items = tree.root.node(Expression::Items).unwrap();
    assert_eq!(shape(items, &tree), ["Name"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:737dea3e8427077593a21747d8e40cc4ad4c72255f879947938d230db84a9bd2
#[test]
fn test_conditionals_nest_to_the_right() {
    let tree = expr("a ? b : c ? e : f");
    check_lossless(&tree, "a ? b : c ? e : f");
    assert!(tree.root.is(Expression::Conditional));
    assert_eq!(
        shape(&tree.root, &tree),
        ["Name", "QuestionMark(?)", "Name", "Colon(:)", "Conditional"]
    );
    let nested = tree.root.node(Expression::Conditional).unwrap();
    assert_eq!(tree.raw(nested.start, nested.end), "c ? e : f");
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:7248e542ffb29c423c14a8080a3a0651fe1cb65aeb963c45d52655790b5012e5
#[test]
fn test_an_empty_file() {
    let tree = file("");
    assert!(tree.root.is(File::SourceFile));
    assert!(tree.root.children.is_empty());
    assert_eq!((tree.root.start, tree.root.end), (0, 0));
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:3f8a84886d2ac1b41e09c0341ce6523ac5c24e300c9be8f845a57613a1e70570
#[test]
fn test_a_use_statement() {
    let source = "use \"../grammar/main\" as Grammar;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert_eq!(tree.root.nodes().count(), 1);
    let use_ = first_statement(&tree);
    assert!(use_.is(Statement::Use));
    assert_eq!(
        shape(use_, &tree),
        ["UseKeyword(use)", "StringLiteral", "AsKeyword(as)", "Identifier(Grammar)", "Semicolon(;)"]
    );
    let literal = use_.node(Expression::StringLiteral).unwrap();
    assert_eq!(shape(literal, &tree), ["DoubleQuoteString"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:6988938891a2d92d5272137136d947550e9c19b327baf283b82ebdfbf287092f
#[test]
fn test_an_if_with_a_block_and_an_else() {
    let source = "if (a) { b; } else c;";
    let tree = file(source);
    check_lossless(&tree, source);
    let if_ = first_statement(&tree);
    assert!(if_.is(Statement::If));
    assert_eq!(shape(if_, &tree), ["IfKeyword(if)", "Group", "Block", "Else"]);
    assert_eq!(shape(if_.node(Expression::Group).unwrap(), &tree), ["GroupOpen(()", "Name", "GroupClose())"]);
    let block = if_.node(Statement::Block).unwrap();
    assert_eq!(shape(block, &tree), ["BlockOpen({)", "ExpressionStatement", "BlockClose(})"]);
    assert_eq!(
        tree.raw_of(&Child::Node(block.node(Statement::ExpressionStatement).unwrap().clone())),
        "b;"
    );
    let else_ = if_.node(Statement::Else).unwrap();
    assert_eq!(shape(else_, &tree), ["ElseKeyword(else)", "ExpressionStatement"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:714afaac6e7e3f09adb4919660abdf9b13c9870d797bacc731b5cf5d547f72e5
#[test]
fn test_a_for_in_loop() {
    let source = "for (const x in list) { }";
    let tree = file(source);
    check_lossless(&tree, source);
    let for_ = first_statement(&tree);
    assert!(for_.is(Statement::For));
    assert_eq!(
        shape(for_, &tree),
        ["ForKeyword(for)", "GroupOpen(()", "ConstKeyword(const)", "Declared", "ForInOf", "GroupClose())", "Block"]
    );
    assert_eq!(shape(for_.node(Expression::Declared).unwrap(), &tree), ["Identifier(x)"]);
    assert_eq!(shape(for_.node(Statement::ForInOf).unwrap(), &tree), ["InKeyword(in)", "Name"]);
    assert_eq!(shape(for_.node(Statement::Block).unwrap(), &tree), ["BlockOpen({)", "BlockClose(})"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:03f2c0bfb1387077c4184ea72a0e8d692175658bff919ffe45a1db47aa6255d3
#[test]
fn test_an_inline_function_with_typed_parameters() {
    let source = "(x: string, tail?: string) => x;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::ExpressionStatement));
    let function = statement.node(Expression::InlineFunction).unwrap();
    assert_eq!(shape(function, &tree), ["Parameters", "DoubleArrowRight(=>)", "Name"]);
    let parameters: Vec<&Node> = function
        .node(Expression::Parameters)
        .unwrap()
        .nodes_of(Expression::Parameter)
        .collect();
    assert_eq!(parameters.len(), 2);
    assert_eq!(shape(parameters[0], &tree), ["Name", "DefinitionClause"]);
    assert_eq!(shape(parameters[1], &tree), ["Name", "QuestionMark(?)", "DefinitionClause"]);
    for parameter in parameters {
        let clause = parameter.node(Expression::DefinitionClause).unwrap();
        assert_eq!(tree.raw(clause.start, clause.end), ": string");
        assert!(clause.find(Expression::PrimitiveType).is_some());
    }
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:38a290949c015bb30e965c6da7019363febe8456250c3b6639a31e6aad1aa509
#[test]
fn test_a_template_with_an_execution_and_a_reference() {
    let source = "`a{{b}}[[c.e]]`;";
    let tree = file(source);
    check_lossless(&tree, source);
    let template = first_statement(&tree).node(Expression::Template).unwrap();
    assert_eq!(
        shape(template, &tree),
        ["Backtick(`)", "TemplateBody(a)", "TemplateExecution", "TemplateReference", "Backtick(`)"]
    );
    let execution = template.node(Expression::TemplateExecution).unwrap();
    assert_eq!(shape(execution, &tree), ["ExecutionOpen({{)", "Name", "ExecutionClose(}})"]);
    let reference = template.node(Expression::TemplateReference).unwrap();
    let member = reference.find(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Name", "ValueAccessor(.)", "Identifier(e)"]);
    assert_eq!(tree.raw_of(&Child::Node(member.node(Expression::Name).unwrap().clone())), "c");
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:ff5346465bc782f0b70e1dc0ee3c00e7c3bdb18d6bef456f6c11c6309da683a3
#[test]
fn test_a_declaration_missing_its_name_recovers_at_the_semicolon() {
    let source = "const = 1; let y = 2;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statements: Vec<&Node> = tree.root.nodes().collect();
    assert_eq!(statements.len(), 2);
    assert!(statements[0].is(Statement::VariableDeclaration));
    assert_eq!(shape(statements[0], &tree), ["ConstKeyword(const)", "Error(\"= 1\")", "Semicolon(;)"]);
    let error = statements[0].errors()[0];
    assert_eq!(error.expected, vec!["Identifier"]);
    assert_eq!(tree.raw(error.start, error.end), "= 1");
    assert!(statements[1].is(Statement::VariableDeclaration));
    assert_eq!(
        shape(statements[1], &tree),
        ["LetKeyword(let)", "Declared", "PlainSetter(=)", "Number", "Semicolon(;)"]
    );
    assert_eq!(tree.errors.len(), 1);
}

// @lfy def/parser/main.lfy:parse#parse:parse:34df80050f53bfb40f469e26559ea0850a873167845d8999342b828116630477
#[test]
fn test_a_keyword_that_would_have_been_the_declared_name_is_the_error_s_keyword() {
    let source = "const d = 1;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::VariableDeclaration));
    assert_eq!(shape(statement, &tree), ["ConstKeyword(const)", "Error(\"d = 1\")", "Semicolon(;)"]);
    let error = statement.errors()[0];
    assert_eq!(error.expected, vec!["Identifier"]);
    assert_eq!(tree.raw(error.start, error.end), "d = 1");
    assert_eq!(error.keyword.as_ref().map(|token| token.raw.as_str()), Some("d"));
    assert_eq!(tree.errors.len(), 1);
}

// @lfy def/parser/main.lfy:parse#parse:parse:9d6331552f408da6b3d2e64ac150f2f6ebe0fae7b8aa09c29e657fc9b3e80fdc
#[test]
fn test_a_keyword_where_an_operand_was_expected_is_the_error_s_keyword() {
    let source = "x = c ?? d;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::ExpressionStatement));
    let error = statement.errors()[0];
    assert_eq!(tree.raw(error.start, error.end), "?? d");
    assert_eq!(error.keyword.as_ref().map(|token| token.raw.as_str()), Some("d"));
    assert_eq!(tree.errors.len(), 1);
}

// @lfy def/parser/main.lfy:parse#parse:parse:ff5346465bc782f0b70e1dc0ee3c00e7c3bdb18d6bef456f6c11c6309da683a3
#[test]
fn test_an_error_node_has_no_keyword_when_none_of_its_tokens_are_one() {
    let source = "const = 1;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert_eq!(tree.errors.len(), 1);
    assert!(tree.errors[0].keyword.is_none());
}

// parse

// @lfy def/parser/main.lfy:parse#parse:parse:545a46287bf0abf2a508a3bc2f399480a555592a6c7eb683235c84348a82e3fd
// @lfy def/parser/main.lfy:parse#parse:parse:dcd0b711212c8a500ae9a400b84c961e39b198ef5720dfe74af30f4ceb820a9f
#[test]
fn every_tree_keeps_its_tokens_and_covers_each_exactly_once() {
    for source in [
        "",
        "\n\n",
        "a;",
        "a b c",
        "const x: `d` = a.b(1) + -2 ** 3;\n",
        "/// doc [[x]]\n/** more **/ fn f(a, ...r): T => U { where (a) and !(b) -> c; }",
        "{ ) a; ; # }",
        "if (a b) { c; } else if (d) e; else { }",
        "x = { a = 1 }; y = { a?: T = string }; z = [1, 2,];",
        "`unclosed {{ a",
        "for (x y) { }",
        "match x { (a) -> b, default -> c, }",
        "(a) : T; (b) is c; (d) => e; (f);",
    ] {
        let lexed = tokens(source);
        let tree = parse(lexed.clone(), None);
        assert_eq!(tree.tokens, lexed, "{source:?}");
        check_lossless(&tree, source);
        let tree = parse(lexed, Some(Entity::Expression(Expression::Expression)));
        check_lossless(&tree, source);
    }
}

// @lfy def/parser/main.lfy:parse#parse:parse:0368a70644f5a2b33623b68b70e1f4a4b4f7b074e3cd0749de799c954fc9b3cf
#[test]
fn a_rule_is_attempted_at_most_once_per_token_index_and_minimum() {
    for source in [
        "((((x))))",
        "x = { a = { b = { c?: T = 1 } } };",
        "a - b - c + d * e ** f ** g;",
        "if (a) { if (b) { c; } else d; }",
    ] {
        let (_, attempts) = parse_counting(tokens(source), None);
        assert!(!attempts.is_empty());
        for (key, count) in attempts {
            assert_eq!(count, 1, "{source:?}: {} at {} with minimum {}", key.0, key.1, key.2);
        }
    }
}

// @lfy def/parser/main.lfy:parse#parse:parse:dcd0b711212c8a500ae9a400b84c961e39b198ef5720dfe74af30f4ceb820a9f
#[test]
fn the_rule_is_what_the_caller_wants_satisfied() {
    let tree = parse(tokens("a;"), None);
    assert_eq!(tree.root_rule, Entity::File(File::SourceFile));
    assert_eq!(tree.root_rule().identifier, "SourceFile");
    let tree = parse(tokens("a;"), Some(Entity::Statement(Statement::Statement)));
    assert_eq!(tree.root_rule, Entity::Statement(Statement::Statement));
    assert!(tree.root.is(Statement::ExpressionStatement));
    assert!(tree.errors.is_empty());
    let tree = parse(tokens("a + b"), Some(Entity::Expression(Expression::Expression)));
    assert!(tree.root.is(Expression::AdditiveOperation));
    let tree = parse(tokens("{ a; }"), Some(Entity::Statement(Statement::Block)));
    assert!(tree.root.is(Statement::Block));
    assert!(tree.errors.is_empty());
    // A terminal as the root is one token.
    let tree = parse(tokens("x"), Some(Entity::Identifier(Identifier::Identifier)));
    assert!(tree.root.is(Identifier::Identifier));
    assert_eq!(tree.root.children, vec![Child::Token(0)]);
}

// @lfy def/parser/main.lfy:parse#parse:parse:5ea7d67e9fc72d835d172341cafed90a3d5204dd2f49ec7ded6d3d6f1828d48e
#[test]
fn tokens_left_over_become_one_error_node_at_the_end_of_the_root() {
    let source = "{ a; } b c";
    let tree = parse(tokens(source), Some(Entity::Statement(Statement::Block)));
    check_lossless(&tree, source);
    assert!(tree.root.is(Statement::Block));
    let last = tree.root.children.last().unwrap();
    let error = last.as_error().unwrap();
    assert_eq!(tree.raw(error.start, error.end), "b c");
    assert!(error.expected.is_empty());
    assert_eq!(tree.errors.len(), 1);
    assert_eq!(shape(&tree.root, &tree), ["BlockOpen({)", "ExpressionStatement", "BlockClose(})", "Error(\"b c\")"]);
    // Trailing trivia is not an error.
    let tree = parse(tokens("{ a; } \n"), Some(Entity::Statement(Statement::Block)));
    assert!(tree.errors.is_empty());
    assert_eq!(tree.root.end, tree.tokens.len());
}

// @lfy def/parser/main.lfy:parse#parse:parse:dcd0b711212c8a500ae9a400b84c961e39b198ef5720dfe74af30f4ceb820a9f
#[test]
fn an_alternation_list_root_holds_leading_trivia_and_the_item() {
    let tree = expr(" a");
    check_lossless(&tree, " a");
    assert!(tree.root.is(Expression::Expression));
    assert_eq!(shape(&tree.root, &tree), ["Name"]);
    assert_eq!(tree.root.children.len(), 2);
    assert!(tree.root.children[0].as_token().is_some());
    assert!(tree.errors.is_empty());
    // With neither trivia before the item nor tokens left over, the item stands in the
    // root's place.
    let tree = parse(tokens("a;"), Some(Entity::Statement(Statement::Statement)));
    assert!(tree.root.is(Statement::ExpressionStatement));
}

// @lfy def/parser/main.lfy:parse#parse:parse:ba7b39019b1fa273cf02e733e1051232e9cff31ab1d91f15e794286b4e203eed
#[test]
fn an_alternation_list_root_holds_the_item_and_the_leftovers() {
    let tree = expr("a b");
    check_lossless(&tree, "a b");
    assert!(tree.root.is(Expression::Expression));
    assert_eq!(shape(&tree.root, &tree), ["Name", "Error(\"b\")"]);
    assert_eq!(tree.errors.len(), 1);
    // Trivia before the item and tokens left over after it: the root holds both.
    let tree = expr(" a b");
    check_lossless(&tree, " a b");
    assert!(tree.root.is(Expression::Expression));
    assert!(tree.root.children[0].as_token().is_some());
    assert_eq!(shape(&tree.root, &tree), ["Name", "Error(\"b\")"]);
    // Nothing satisfies the rule: everything is the error node.
    let tree = expr("+ a");
    check_lossless(&tree, "+ a");
    assert_eq!(shape(&tree.root, &tree), ["Error(\"+ a\")"]);
    assert!(tree.errors[0].expected.contains(&"Identifier"));
    assert!(!tree.errors[0].expected.contains(&"Plus"));
}

// @lfy def/parser/main.lfy:parse#parse:parse:b25f4ff45c8f1a8f81e5a9ac7e2e632b5fbd46d2027d7b1e83e7c661f5300a17
#[test]
fn the_parser_parses_against_the_grammar_document_only() {
    let document = grammar();
    assert_eq!(document, crate::grammar::grammar_document());
    for rule in crate::grammar::rules() {
        assert!(document.contains(rule.text()), "{rule}");
    }
    // @lfy def/parser/main.lfy:parse
    // Categories and bindings come from the rules: the tables set none of them.
    let tables = beginning::tables();
    let plus = Entity::Punctuation(Punctuation::Plus);
    let additive = tables.operations(plus)[0];
    assert_eq!(additive.effective_binding(), Expression::AdditiveOperation.effective_binding());
    assert!(additive.is_infix());
}

// Rules

// @lfy def/parser/main.lfy:parse#parse:parse:52beec3a25673985c27513ea6ed89917d7ab0b7fbae592d165c47c9d8e1f9cd6
#[test]
fn a_terminal_is_satisfied_by_one_token_of_that_terminal() {
    let tree = file("a;");
    let statement = first_statement(&tree);
    assert_eq!(statement.children.len(), 2);
    assert_eq!(statement.children[1], Child::Token(1));
    assert!(tree.token(1).is(Punctuation::Semicolon));
    // @lfy def/parser/main.lfy:parse#parse:parse:b6545041ced4ab341c9429d588696be3586319e8dcb9932b30b94cecddce29af
    let name = statement.node(Expression::Name).unwrap();
    assert_eq!(name.children, vec![Child::Token(0)]);
    assert_eq!((name.start, name.end), (0, 1));
}

// @lfy def/parser/main.lfy:parse#parse:parse:dcd0b711212c8a500ae9a400b84c961e39b198ef5720dfe74af30f4ceb820a9f
#[test]
fn trivia_between_elements_becomes_children_of_the_open_node() {
    let source = "a /* c */ // d\n ;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::ExpressionStatement));
    let kinds: Vec<&str> = statement
        .children
        .iter()
        .map(|child| match child {
            Child::Node(node) => node.rule.identifier(),
            Child::Token(index) => tree.token(*index).rule.unwrap().identifier(),
            Child::Error(_) => "Error",
        })
        .collect();
    assert_eq!(
        kinds,
        ["Name", "Space", "Comment", "Space", "Comment", "NewLine", "Space", "Semicolon"]
    );
    assert!(tree.errors.is_empty());
    // Trivia before a statement belongs to the file, and inside an operation to it.
    let tree = file("\n a + b ;");
    let statement = first_statement(&tree);
    assert!(tree.root.children[0].as_token().is_some());
    let operation = statement.node(Expression::AdditiveOperation).unwrap();
    assert_eq!(operation.children.len(), 5);
    assert_eq!(tree.raw(operation.start, operation.end), "a + b");
}

// @lfy def/parser/main.lfy:parse#parse:parse:578aaf59c87e9018da5015fd6780a313001b26bc4c4946f0410fbd734543b9bb
#[test]
fn a_token_without_a_rule_is_an_error_node_of_one_token() {
    let source = "a # ;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statement = first_statement(&tree);
    assert_eq!(shape(statement, &tree), ["Name", "Error(\"#\")", "Semicolon(;)"]);
    assert_eq!(tree.errors.len(), 1);
    assert_eq!(tree.errors[0].expected, vec!["Semicolon"]);
    // Inside a template, where no trivia sits between elements, an invalid token is still
    // an error node of one token.
    let source = "`a\\qb`;";
    let tree = file(source);
    check_lossless(&tree, source);
    let template = first_statement(&tree).node(Expression::Template).unwrap();
    assert_eq!(
        shape(template, &tree),
        ["Backtick(`)", "TemplateBody(a)", "Error(\"\\\\\")", "TemplateBody(qb)", "Backtick(`)"]
    );
}

// Selection

// @lfy def/parser/main.lfy:parse#parse:parse:a3f742ba20ecbb1f1c96b797deabd38eac66aba3b7c42ebb1a3597022f05dade
#[test]
fn a_statement_is_selected_among_the_statement_rules_in_tried_before_order() {
    let tree = file("{ x; } function f() { } trait t { } object;");
    let statements: Vec<&str> = tree.root.nodes().map(|node| node.rule.identifier()).collect();
    assert_eq!(
        statements,
        ["Block", "FunctionDeclaration", "TraitDeclaration", "ExpressionStatement"]
    );
    assert!(tree.errors.is_empty());
    // A statement that begins with a block is a Block, never an Object.
    // @lfy def/grammar/rules/statement.lfy:Block#Block:triedBefore:e09e16018b356ab47e66865a41c2e195ce6af017abdeadcb2510ada2abbdd4d7
    let tree = file("{ a = 1 }");
    assert!(first_statement(&tree).is(Statement::Block));
    assert_eq!(tree.errors.len(), 1);
    // The rule that comes first is recoverable, so the second is never reached once the
    // first has taken a token.
    let tree = file("function;");
    assert!(first_statement(&tree).is(Statement::FunctionDeclaration));
    assert!(!tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:5241f76a5d3e38cb4ee5c2135049cf5dd09d2a51bf4d76f51ed2f05ad2f66acd
#[test]
fn an_expression_is_selected_among_primaries_and_prefixes() {
    let tree = file("x = { a = 1 }; y = { a?: T = string }; z = (a); w = (a) => a;");
    let rights: Vec<&str> = tree
        .root
        .nodes()
        .map(|statement| {
            statement
                .node(Expression::Assignment)
                .unwrap()
                .nodes()
                .nth(1)
                .unwrap()
                .rule
                .identifier()
        })
        .collect();
    assert_eq!(rights, ["Object", "Type", "Group", "InlineFunction"]);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
}

// @lfy def/parser/main.lfy:parse#parse:parse:852177ea3f910234c2b229c0cf5de24b37a30bbbc06eaa0d74af122c2d89223d
#[test]
fn an_alternation_offers_its_alternatives_as_candidates() {
    let tree = file("if (a) b; if (c) { } match x { default -> 1, (y) -> 2 }");
    let statements: Vec<&Node> = tree.root.nodes().collect();
    assert!(statements[0].node(Statement::ExpressionStatement).is_some());
    assert!(statements[1].node(Statement::Block).is_some());
    let arms: Vec<&Node> = statements[2].nodes_of(Statement::MatchArm).collect();
    assert_eq!(shape(arms[0], &tree), ["DefaultKeyword(default)", "SingleArrowRight(->)", "Number"]);
    assert_eq!(shape(arms[1], &tree), ["Group", "SingleArrowRight(->)", "Number"]);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    // No candidate is selected: the alternation fails.
    let tree = file("for (x y) { }");
    assert_eq!(tree.errors.len(), 1);
}

// @lfy def/parser/main.lfy:parse#parse:parse:8791f1af46546e454f5457e36522f723cf33c1b9cf0347980d925f47b18dd1d4
#[test]
fn a_candidate_that_fails_hands_the_token_to_the_next_in_tried_before_order() {
    // A `GroupOpen` selects `InlineFunction` before `Group`; here the inline function
    // fails at that index, so the group is tried there and satisfies the expression.
    let tree = file("(a);");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let statement = first_statement(&tree);
    assert!(statement.node(Expression::InlineFunction).is_none());
    assert!(statement.node(Expression::Group).is_some());
    // A `BlockOpen` selects `Block` before `ExpressionStatement`; a block that fails
    // hands the token on only when it has taken none, so an object is a statement here.
    let tree = file("x = { a = 1 };");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    assert!(first_statement(&tree).find(Expression::Object).is_some());
}

// @lfy def/parser/main.lfy:parse#parse:parse:b122a3907a1e84cdecdab3a9de01bcde8c255c36c0fe8a7492a1b4aa6cc3f1ed
#[test]
fn no_candidate_after_one_that_does_not_fail_is_tried() {
    // `InlineFunction` does not fail here, so `Group` is never tried at that index.
    let tree = file("(a) => a;");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let statement = first_statement(&tree);
    assert!(statement.node(Expression::InlineFunction).is_some());
    assert!(statement.find(Expression::Group).is_none());
    // `Block` does not fail here, so the statement is never an `ExpressionStatement`
    // holding an `Object`.
    let tree = file("{ a = 1 }");
    assert!(first_statement(&tree).is(Statement::Block));
    assert!(tree.root.find(Expression::Object).is_none());
}

// @lfy def/parser/main.lfy:parse#parse:parse:50f445f45220780457d4f8bc7459c8ccd6b0b18da824a94fec40ae0387001051
#[test]
fn an_alternation_with_an_operation_parses_one_below_its_lowest_power() {
    // `Reference = ( Name | Current | Dereference | Member | Index )`: the lowest power
    // among its infix and postfix alternatives is the access level of `Member`, so the
    // expression is parsed one below it and no weaker operation joins it.
    let items = [
        Entity::Expression(Expression::Name),
        Entity::Expression(Expression::Current),
        Entity::Expression(Expression::Dereference),
        Entity::Expression(Expression::Member),
        Entity::Expression(Expression::Index),
    ];
    let access = Expression::Member.effective_binding().unwrap().precedence_value();
    assert_eq!(beginning::expression_alternation_minimum(&items), access - 1);
    let tree = file("`[[a.b]]`;");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let reference = tree.root.find(Expression::Reference).unwrap();
    assert!(reference.nodes().next().unwrap().is(Expression::Member));
    // A weaker operation is below the minimum: the reference stops before it.
    let tree = file("`[[a + b]]`;");
    assert_eq!(tree.errors.len(), 1);
}

// @lfy def/parser/main.lfy:parse#parse:parse:c61ee6646978cfc71d4fb795be1ba0ae2fafa0b0460626e7db2d1f51eb775bef
#[test]
fn an_alternation_without_an_operation_parses_with_minimum_zero() {
    let items = [Entity::Expression(Expression::Name), Entity::Expression(Expression::Group)];
    assert_eq!(beginning::expression_alternation_minimum(&items), 0);
    assert_eq!(beginning::expression_alternation_minimum(&[]), 0);
    // Every alternation of expression rules the grammar presents with an operation among
    // its alternatives keeps a minimum above 0.
    let with_operation = [Entity::Expression(Expression::Name), Entity::Expression(Expression::Member)];
    assert!(beginning::expression_alternation_minimum(&with_operation) > 0);
}

// @lfy def/parser/main.lfy:parse#parse:parse:0778dfb04d4b97381baf71158d420649cf2487436b414495b03b60995f7516a9
#[test]
fn an_alternation_of_expression_rules_is_one_expression_of_limited_power() {
    let tree = file("`[[a.b]] [[&c.q]] [[e[0] ]]`;");
    let references: Vec<&Node> = first_statement(&tree)
        .node(Expression::Template)
        .unwrap()
        .nodes_of(Expression::TemplateReference)
        .collect();
    assert_eq!(references.len(), 3);
    let chains: Vec<&str> = references
        .iter()
        .map(|reference| reference.node(Expression::Reference).unwrap().nodes().next().unwrap().rule.identifier())
        .collect();
    assert_eq!(chains, ["Member", "Member", "Index"]);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    // Steps after a dereference apply to the dereferenced entity.
    let member = references[1].find(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Dereference", "ValueAccessor(.)", "Identifier(q)"]);
    // The node must be one of the alternatives: a call is not, so the reference fails and
    // the template reference recovers.
    let tree = file("`[[a(b)]]`;");
    assert_eq!(tree.errors.len(), 1);
    assert_eq!(tree.raw(tree.errors[0].start, tree.errors[0].end), "a(b)");
    // The same shape in a statement: `with` takes a name or a member.
    let tree = file("with a.b { } with c { }");
    assert!(tree.errors.is_empty());
    let tree = file("with a(b) { }");
    assert_eq!(tree.errors.len(), 1);
}

// @lfy def/parser/main.lfy:parse#parse:parse:706190a656cac524909cc166cc04abd19b61fed89e179b1e22f9078aaa2e4789
#[test]
fn an_optional_element_is_tried_when_the_next_token_can_begin_it() {
    // The member name is optional and taken when the next token can begin it.
    let tree = file("b.c;\nx@type;");
    let statements: Vec<&Node> = tree.root.nodes().collect();
    let member = statements[0].node(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Name", "ValueAccessor(.)", "Identifier(c)"]);
    let member = statements[1].node(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Name", "ContextAccessor(@)", "TypeKeyword(type)"]);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
}

// @lfy def/parser/main.lfy:parse#parse:parse:706190a656cac524909cc166cc04abd19b61fed89e179b1e22f9078aaa2e4789
#[test]
fn an_optional_element_is_skipped_when_the_next_token_cannot_begin_it() {
    // A `Semicolon` cannot begin a member name, so the optional element is skipped.
    let tree = file("a.;");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let member = first_statement(&tree).node(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Name", "ValueAccessor(.)"]);
    // A declaration skips every optional clause its next token cannot begin.
    let tree = file("const x = 1;");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let declared = first_statement(&tree).node(Expression::Declared).unwrap();
    assert_eq!(shape(declared, &tree), ["Identifier(x)"]);
}

// @lfy def/parser/main.lfy:parse#parse:parse:706190a656cac524909cc166cc04abd19b61fed89e179b1e22f9078aaa2e4789
#[test]
fn an_optional_element_that_is_tried_and_fails_is_skipped() {
    // A leading `&` in a type position can begin the optional union operator, but the
    // element fails there, so it is skipped and the `&` belongs to the type expression.
    let tree = file("const x: & y;");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let type_expression = first_statement(&tree).find(Expression::TypeExpression).unwrap();
    assert_eq!(shape(type_expression, &tree), ["Ampersand(&)", "TypeItem"]);
}

// @lfy def/parser/main.lfy:parse#parse:parse:5de309372b69eea9c7b10354e1ad3f703ff93b4b9a89356b37f74aebf4d7dc9d
#[test]
fn a_later_failure_never_revisits_an_optional_element() {
    // The optional expression inside an `Index` is taken as `b`, and the `ListClose`
    // after it then fails: the index recovers where it stands and keeps the expression
    // rather than parsing again without it.
    let source = "a[b c];";
    let tree = file(source);
    check_lossless(&tree, source);
    let index = first_statement(&tree).node(Expression::Index).unwrap();
    assert_eq!(shape(index, &tree), ["Name", "ListOpen([)", "Name", "Error(\"c\")", "ListClose(])"]);
    assert_eq!(tree.errors.len(), 1);
    // The optional `TypeParameters` of a declaration is taken and survives the failure
    // of the element after the clauses that follow it.
    let source = "d X<T> extends ;";
    let tree = file(source);
    check_lossless(&tree, source);
    let declaration = first_statement(&tree);
    assert!(declaration.node(Expression::TypeParameters).is_some());
    assert_eq!(tree.raw(tree.errors[0].start, tree.errors[0].end), "extends ");
    // Revisiting would mean attempting a rule again at the same token index and minimum.
    let (_, attempts) = parse_counting(tokens(source), None);
    for (key, count) in attempts {
        assert_eq!(count, 1, "{} at {} with minimum {}", key.0, key.1, key.2);
    }
}

// @lfy def/parser/main.lfy:parse#parse:parse:9d10aea504259185e52d701dbed880a21517363b27fba335086c49485f715f74
#[test]
fn a_repetition_tries_another_iteration_while_the_next_token_can_begin_one() {
    let tree = file("(a, b, ...rest) => a; { c; e; }");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let statements: Vec<&Node> = tree.root.nodes().collect();
    let parameters = statements[0].find(Expression::Parameters).unwrap();
    assert_eq!(
        shape(parameters, &tree),
        ["GroupOpen(()", "Parameter", "Comma(,)", "Parameter", "Comma(,)", "SpreadParameter", "GroupClose())"]
    );
    // A `Block` repeats its statements while the next token can begin one.
    assert_eq!(statements[1].nodes_of(Statement::ExpressionStatement).count(), 2);
}

// @lfy def/parser/main.lfy:parse#parse:parse:e177fe61a492b6dc0f861cc4af65d89dceb1dc4175ff8221e2adcab0112578a8
#[test]
fn a_repetition_ends_when_the_next_token_cannot_begin_the_repeated_element() {
    let tree = file("(c, ) => c; match x { a -> b, }");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let statements: Vec<&Node> = tree.root.nodes().collect();
    // The `GroupClose` after the trailing comma cannot begin another parameter.
    let parameters = statements[0].find(Expression::Parameters).unwrap();
    assert_eq!(shape(parameters, &tree), ["GroupOpen(()", "Parameter", "Comma(,)", "GroupClose())"]);
    // The `BlockClose` after the trailing comma cannot begin another arm.
    assert_eq!(statements[1].nodes_of(Statement::MatchArm).count(), 1);
}

// @lfy def/parser/main.lfy:parse#parse:parse:e177fe61a492b6dc0f861cc4af65d89dceb1dc4175ff8221e2adcab0112578a8
#[test]
fn an_iteration_that_fails_leaves_its_tokens_and_ends_the_repetition() {
    // The statements of the block repeat; the iteration that begins at `)` fails and
    // leaves its tokens, which the block then covers with one error node.
    let source = "{ a; ) }";
    let tree = file(source);
    check_lossless(&tree, source);
    let block = first_statement(&tree);
    assert_eq!(
        shape(block, &tree),
        ["BlockOpen({)", "ExpressionStatement", "Error(\") \")", "BlockClose(})"]
    );
    assert_eq!(tree.errors.len(), 1);
}

// Expressions

// @lfy def/parser/main.lfy:parse#parse:parse:670dd0bda11bbce2a890e25f36de906c5dd5e745536d1225fa35bc3d4544eda9
#[test]
fn operands_and_tails_are_parsed_with_the_operation_power_as_the_minimum() {
    // A prefix operand stops before a weaker operator.
    let tree = expr("!a && b");
    assert!(tree.root.is(Expression::LogicalAndOperation));
    // A right operand of a left associative operator stops before an equal one.
    let tree = expr("a * b * c");
    assert!(tree.root.node(Expression::MultiplicativeOperation).is_some());
    // A definition's tail takes a union but not an assignment.
    let tree = expr("x : a | b = c");
    assert!(tree.root.is(Expression::Assignment));
    let definition = tree.root.node(Expression::Definition).unwrap();
    assert!(definition.node(Expression::BitwiseOrOperation).is_some());
    // A cast's tail is a type expression.
    // @lfy def/grammar/rules/expression.lfy:Cast
    let tree = expr("a as string | number");
    assert!(tree.root.is(Expression::Cast));
    assert_eq!(shape(&tree.root, &tree), ["Name", "AsKeyword(as)", "TypeExpression"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:a3a41aa47d6f8c5ce84eb52a369276e28005db60620c11d0956e0baa5af1578c
#[test]
fn an_operation_applies_when_its_power_beats_the_minimum_or_ties_to_the_right() {
    let tree = expr("a ** b ** c");
    assert!(tree.root.is(Expression::PowerOperation));
    let right = tree.root.nodes().nth(1).unwrap();
    assert!(right.is(Expression::PowerOperation));
    assert_eq!(tree.raw(right.start, right.end), "b ** c");
    let tree = expr("a = b = c");
    assert!(tree.root.is(Expression::Assignment));
    assert_eq!(tree.raw_of(&Child::Node(tree.root.nodes().nth(1).unwrap().clone())), "b = c");
    let tree = expr("a + b * c");
    assert!(tree.root.is(Expression::AdditiveOperation));
    let tree = expr("a * b + c");
    assert!(tree.root.is(Expression::AdditiveOperation));
    assert!(tree.root.nodes().next().unwrap().is(Expression::MultiplicativeOperation));
}

// @lfy def/parser/main.lfy:parse#parse:parse:1d33f9999208718a92bdb3b5d55319a75b2b1f045141499ce8fd22d6cab30ae3
#[test]
fn expressions_after_an_open_bracket_or_in_other_rules_start_at_minimum_zero() {
    let tree = expr("a[b = c]");
    assert!(tree.root.is(Expression::Index));
    assert!(tree.root.node(Expression::Assignment).is_some());
    let tree = expr("f(a = b, c ? x : e)");
    assert!(tree.root.is(Expression::Call));
    let items = tree.root.node(Expression::Items).unwrap();
    assert_eq!(shape(items, &tree), ["Assignment", "Comma(,)", "Conditional"]);
    let tree = expr("(a = b)");
    assert!(tree.root.is(Expression::Group));
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:c971b2ec65d561653e23d92a462149ebf3b978d9b299ba3e65ae85661bbec770
#[test]
fn the_postfix_rule_of_an_operator_is_tried_before_its_infix_rule() {
    // A LessThan is the operator of Generic, a postfix rule, and of RelationalOperation,
    // an infix rule: the postfix rule comes first.
    let tables = beginning::tables();
    let less_than = Entity::Punctuation(Punctuation::LessThan);
    assert_eq!(
        tables.operations(less_than),
        [
            Entity::Expression(Expression::Generic),
            Entity::Expression(Expression::RelationalOperation)
        ]
    );
    // Every other operator terminal continues with one rule.
    let plus = Entity::Punctuation(Punctuation::Plus);
    assert_eq!(tables.operations(plus), [Entity::Expression(Expression::AdditiveOperation)]);
    assert!(tables.operations(Entity::Punctuation(Punctuation::Semicolon)).is_empty());
    // The postfix rule is the parse of the token where its own criteria hold there.
    let tree = expr("f<T>(x)");
    assert!(tree.root.is(Expression::Call));
    assert!(tree.root.node(Expression::Generic).is_some());
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:c971b2ec65d561653e23d92a462149ebf3b978d9b299ba3e65ae85661bbec770
#[test]
fn the_infix_rule_is_tried_where_the_postfix_rule_of_the_operator_is_no_match() {
    // A `Generic` is no match before the identifier `c`, so the `LessThan` continues the
    // expression as the operator of a `RelationalOperation` instead.
    let tree = expr("a < b > c");
    assert!(tree.root.is(Expression::RelationalOperation));
    assert!(tree.root.find(Expression::Generic).is_none());
    assert!(tree.errors.is_empty());
    let tree = expr("a < b");
    assert!(tree.root.is(Expression::RelationalOperation));
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:c971b2ec65d561653e23d92a462149ebf3b978d9b299ba3e65ae85661bbec770
#[test]
fn the_infix_rule_is_not_tried_where_the_postfix_rule_of_the_operator_matches() {
    // The `Generic` matches, so no `RelationalOperation` is built from the same
    // `LessThan`, and the tokens it covers are the type arguments alone.
    let tree = expr("f<string>(x)");
    assert!(tree.errors.is_empty());
    let generic = tree.root.node(Expression::Generic).unwrap();
    assert_eq!(tree.raw(generic.start, generic.end), "f<string>");
    assert!(tree.root.find(Expression::RelationalOperation).is_none());
    let tree = expr("a<T> = 1");
    assert!(tree.root.is(Expression::Assignment));
    assert!(tree.root.find(Expression::Generic).is_some());
    assert!(tree.root.find(Expression::RelationalOperation).is_none());
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:ff4485628f5316027c6b1e97b6101814e02b73b69e346a4b427eda28e97478cc
#[test]
fn an_attempt_for_one_rule_is_distinct_from_an_attempt_for_another() {
    // A Generic is no match when the expression is parsed for a Reference, so the same
    // tokens at the same index and minimum parse differently for the two rules and
    // neither attempt answers the other.
    let source = "List<T>";
    let as_expression = parse(tokens(source), Some(Entity::Expression(Expression::Expression)));
    check_lossless(&as_expression, source);
    assert!(as_expression.root.is(Expression::Generic));
    assert!(as_expression.errors.is_empty());
    let as_reference = parse(tokens(source), Some(Entity::Expression(Expression::Reference)));
    check_lossless(&as_reference, source);
    assert!(as_reference.root.find(Expression::Generic).is_none());
    assert_eq!(as_reference.errors.len(), 1);
    // Both attempts happen in one parse of a declaration whose type and value are the
    // same tokens: the type holds the TypeArguments of a TypeItem, the value a Generic.
    let source = "const x: List<T> = List<T>;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let declaration = first_statement(&tree);
    let arguments = declaration.find(Expression::TypeArguments).expect("the type");
    assert_eq!(tree.raw(arguments.start, arguments.end), "<T>");
    let generic = declaration.find(Expression::Generic).expect("the value");
    assert_eq!(tree.raw(generic.start, generic.end), "List<T>");
    // Each rule is still attempted at most once per token index and minimum.
    let (_, attempts) = parse_counting(tokens(source), None);
    for (key, count) in attempts {
        assert_eq!(count, 1, "{} at {} with minimum {}", key.0, key.1, key.2);
    }
}

// Errors

// @lfy def/parser/main.lfy:parse#parse:parse:ce5ef922964b0785ccb24c312070b213c8d95315059ea093913c2390c782ff45
#[test]
fn a_recoverable_rule_closes_with_an_error_node_where_it_cannot_continue() {
    let source = "if (a b) { c; }";
    let tree = file(source);
    check_lossless(&tree, source);
    let if_ = first_statement(&tree);
    assert_eq!(shape(if_, &tree), ["IfKeyword(if)", "Error(\"(a b) \")", "Block"]);
    assert_eq!(tree.errors[0].expected, vec!["GroupOpen"]);
    // The input ends inside a rule.
    let tree = file("const x");
    let declaration = first_statement(&tree);
    assert_eq!(shape(declaration, &tree), ["ConstKeyword(const)", "Declared", "Error(\"\")"]);
    // The element that failed is the semicolon, and the optional setter that was skipped
    // there could have continued the declaration too.
    assert_eq!(tree.errors[0].expected, vec!["Semicolon", "PlainSetter"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:ce5ef922964b0785ccb24c312070b213c8d95315059ea093913c2390c782ff45
    // A rule without recoverable produces nothing, so the group here is left to the
    // statement, which then recovers.
    let tree = file("(a b);");
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::ExpressionStatement));
    assert!(statement.node(Expression::Group).is_none());
}

// @lfy def/parser/main.lfy:parse#parse:parse:34df80050f53bfb40f469e26559ea0850a873167845d8999342b828116630477
#[test]
fn the_error_s_keyword_is_the_first_token_it_covers_that_an_identifier_would_have_fit() {
    // Both `d` and `case` are keyword tokens an `Identifier` of the same text would have
    // let the open rule take: the first of them is the error's keyword.
    let source = "const d = case;";
    let tree = file(source);
    check_lossless(&tree, source);
    let error = &tree.errors[0];
    assert_eq!(tree.raw(error.start, error.end), "d = case");
    assert_eq!(error.keyword.as_ref().map(|token| token.raw.as_str()), Some("d"));
}

// @lfy def/parser/main.lfy:parse#parse:parse:ff5346465bc782f0b70e1dc0ee3c00e7c3bdb18d6bef456f6c11c6309da683a3
#[test]
fn an_error_covering_no_such_keyword_has_none() {
    // A keyword the open rule could not have taken as a name either way is not one.
    let source = "const = 1;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert_eq!(tree.errors.len(), 1);
    assert!(tree.errors[0].keyword.is_none());
    let tree = file("a # ;");
    assert!(tree.errors[0].keyword.is_none());
}

// @lfy def/parser/main.lfy:parse#parse:parse:5ea7d67e9fc72d835d172341cafed90a3d5204dd2f49ec7ded6d3d6f1828d48e
#[test]
fn leftover_tokens_without_a_rule_are_one_error_node_at_the_end_of_the_root() {
    let source = "a;\n# #";
    let tree = file(source);
    check_lossless(&tree, source);
    assert_eq!(shape(&tree.root, &tree), ["ExpressionStatement", "Error(\"# #\")"]);
    assert_eq!(tree.errors.len(), 1);
    assert!(tree.errors[0].expected.contains(&"ConstKeyword"));
    let tree = expr("a #");
    assert_eq!(shape(&tree.root, &tree), ["Name", "Error(\"#\")"]);
    assert!(tree.errors[0].expected.contains(&"Plus"));
    let tree = parse(tokens("{ } #"), Some(Entity::Statement(Statement::Block)));
    assert!(tree.errors[0].expected.is_empty());
    // Between two statements a token without a rule is still an error node of one token.
    let tree = file("a;\n# b;");
    assert_eq!(shape(&tree.root, &tree), ["ExpressionStatement", "Error(\"#\")", "ExpressionStatement"]);
}

// Every rule a statement or an expression can begin with

/// Whether `rule` is one of `candidates` and is among the candidates selected at every
/// terminal a token of which can be the first token it covers.
fn is_tried(rule: Entity, candidates: &[Entity]) -> bool {
    let tables = beginning::tables();
    let first = tables.first(rule);
    candidates.contains(&rule)
        && !first.is_empty()
        && first
            .iter()
            .all(|&terminal| tables.select(candidates, terminal).contains(&rule))
}

/// [`is_tried`] among the statement rules.
fn statement_tried(rule: Statement) -> bool {
    is_tried(Entity::Statement(rule), STATEMENTS)
}

/// [`is_tried`] among the primary and prefix rules an expression begins with.
fn expression_tried(rule: Expression) -> bool {
    let candidates: Vec<Entity> = PRIMARIES.iter().chain(PREFIXES).copied().collect();
    is_tried(Entity::Expression(rule), &candidates)
}

// @lfy def/parser/main.lfy:parse#parse:parse:140e8e6b83005b1d43d49be2e061b16a5e12a8406a5e1119ba629e43bed1cff2
#[test]
fn every_statement_rule_is_tried_where_the_next_token_can_begin_it() {
    for rule in [
        Statement::DataDeclaration,
        Statement::AgentFunctionDeclaration,
        Statement::FunctionDeclaration,
        Statement::TraitDeclaration,
        Statement::TypeDeclaration,
        Statement::EnumDeclaration,
        Statement::VariableDeclaration,
        Statement::AliasDeclaration,
        Statement::ExternalDeclaration,
        Statement::Use,
        Statement::Block,
        Statement::If,
        Statement::For,
        Statement::While,
        Statement::Loop,
        Statement::Break,
        Statement::Continue,
        Statement::Return,
        Statement::Match,
        Statement::With,
        Statement::Async,
        Statement::Ace,
        Statement::ExpressionStatement,
        Statement::Where,
    ] {
        assert!(statement_tried(rule), "{rule:?}");
    }
}

// @lfy def/parser/main.lfy:parse#parse:parse:140e8e6b83005b1d43d49be2e061b16a5e12a8406a5e1119ba629e43bed1cff2
#[test]
fn every_primary_and_prefix_rule_is_tried_where_the_next_token_can_begin_it() {
    for rule in [
        Expression::StringLiteral,
        Expression::Template,
        Expression::Number,
        Expression::Boolean,
        Expression::Nullish,
        Expression::PrimitiveType,
        Expression::Name,
        Expression::Current,
        Expression::Previous,
        Expression::Group,
        Expression::InlineFunction,
        Expression::List,
        Expression::Object,
        Expression::TypePredicate,
        Expression::Type,
        Expression::NotOperation,
        Expression::NegateOperation,
        Expression::BitwiseNotOperation,
        Expression::Dereference,
        Expression::SpreadOperation,
        Expression::AwaitOperation,
        Expression::InOperation,
        Expression::OfOperation,
        Expression::FromOperation,
    ] {
        assert!(expression_tried(rule), "{rule:?}");
    }
}

// Every operation an operator continues an expression with

/// The children a node for `rule` holds when the next token is its operator and its
/// binding power continues the expression: the left operand, the operator, and what
/// follows. The parse must cover `source` without an error.
fn operation(source: &str, rule: Expression) -> Vec<String> {
    let tree = expr(source);
    check_lossless(&tree, source);
    assert!(tree.errors.is_empty(), "{source:?}: {:?}", tree.errors);
    assert!(tree.root.is(rule), "{source:?}: {}", tree.root.rule.identifier());
    shape(&tree.root, &tree)
}

// @lfy def/parser/main.lfy:parse#parse:parse:a3a41aa47d6f8c5ce84eb52a369276e28005db60620c11d0956e0baa5af1578c
#[test]
fn every_infix_operation_is_built_from_the_left_operand_the_operator_and_what_follows() {
    assert_eq!(operation("a + b", Expression::AdditiveOperation), ["Name", "Plus(+)", "Name"]);
    assert_eq!(operation("a * b", Expression::MultiplicativeOperation), ["Name", "Star(*)", "Name"]);
    assert_eq!(operation("a ** b", Expression::PowerOperation), ["Name", "Power(**)", "Name"]);
    assert_eq!(operation("a | b", Expression::BitwiseOrOperation), ["Name", "BitwiseOr(|)", "Name"]);
    assert_eq!(operation("a ^ b", Expression::BitwiseXorOperation), ["Name", "BitwiseXor(^)", "Name"]);
    assert_eq!(operation("a & b", Expression::BitwiseAndOperation), ["Name", "Ampersand(&)", "Name"]);
    assert_eq!(operation("a <= b", Expression::RelationalOperation), ["Name", "LessThanOrEqual(<=)", "Name"]);
    assert_eq!(operation("a == b", Expression::EqualityOperation), ["Name", "Equal(==)", "Name"]);
    assert_eq!(operation("a && b", Expression::LogicalAndOperation), ["Name", "LogicalAnd(&&)", "Name"]);
    assert_eq!(operation("a ?? b", Expression::CoalescenceOperation), ["Name", "NullishOr(??)", "Name"]);
    assert_eq!(operation("a ... b", Expression::RangeOperation), ["Name", "Spread(...)", "Name"]);
    assert_eq!(operation("a = b", Expression::Assignment), ["Name", "PlainSetter(=)", "Name"]);
}

// @lfy def/parser/main.lfy:parse#parse:parse:a3a41aa47d6f8c5ce84eb52a369276e28005db60620c11d0956e0baa5af1578c
#[test]
fn every_postfix_operation_is_built_from_the_left_operand_the_operator_and_what_follows() {
    assert_eq!(operation("a.b", Expression::Member), ["Name", "ValueAccessor(.)", "Identifier(b)"]);
    assert_eq!(operation("a[b]", Expression::Index), ["Name", "ListOpen([)", "Name", "ListClose(])"]);
    assert_eq!(operation("a(b)", Expression::Call), ["Name", "GroupOpen(()", "Items", "GroupClose())"]);
    assert_eq!(operation("a is t", Expression::Traits), ["Name", "IsKeyword(is)", "TraitUses"]);
    assert_eq!(operation("a : b", Expression::Definition), ["Name", "Colon(:)", "Name"]);
    assert_eq!(operation("a as T", Expression::Cast), ["Name", "AsKeyword(as)", "TypeExpression"]);
    assert_eq!(
        operation("a<T>", Expression::Generic),
        ["Name", "LessThan(<)", "TypeExpression", "GreaterThan(>)"]
    );
    assert_eq!(
        operation("a ? b : c", Expression::Conditional),
        ["Name", "QuestionMark(?)", "Name", "Colon(:)", "Name"]
    );
}

// More @test cases

// @lfy def/parser/main.lfy:parse#parse:parse:aa77e57dcc5c24cdb81839a9e4b86279ae1cf62f9884bbeeee6deb99aa06e7d2
#[test]
fn test_multiplication_binds_tighter_than_addition_on_the_right() {
    let tree = expr("a + b * c");
    check_lossless(&tree, "a + b * c");
    assert!(tree.root.is(Expression::AdditiveOperation));
    assert_eq!(shape(&tree.root, &tree), ["Name", "Plus(+)", "MultiplicativeOperation"]);
    let right = tree.root.node(Expression::MultiplicativeOperation).unwrap();
    assert_eq!(tree.raw(right.start, right.end), "b * c");
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:8d11449bdfef3e7e228b4a3b400dd5797a74a1d8c034ffb77e279744d313cb70
#[test]
fn test_multiplication_binds_tighter_than_addition_on_the_left() {
    let tree = expr("a * b + c");
    check_lossless(&tree, "a * b + c");
    assert!(tree.root.is(Expression::AdditiveOperation));
    assert_eq!(shape(&tree.root, &tree), ["MultiplicativeOperation", "Plus(+)", "Name"]);
    let left = tree.root.node(Expression::MultiplicativeOperation).unwrap();
    assert_eq!(tree.raw(left.start, left.end), "a * b");
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:517001d8220b7f4021353b75845b360a1dbde811954c0acf0f5d4f2b79571337
#[test]
fn test_power_is_right_associative() {
    let tree = expr("a ** b ** c");
    check_lossless(&tree, "a ** b ** c");
    assert!(tree.root.is(Expression::PowerOperation));
    assert_eq!(shape(&tree.root, &tree), ["Name", "Power(**)", "PowerOperation"]);
    let right = tree.root.node(Expression::PowerOperation).unwrap();
    assert_eq!(tree.raw(right.start, right.end), "b ** c");
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:ecd12a8e8fe1efb3ae522a2ab4561975cf50dc71a5821f37026f7b9a8c2bc37d
#[test]
fn test_a_call_then_an_addition() {
    let tree = expr("f(x) + 1");
    check_lossless(&tree, "f(x) + 1");
    assert!(tree.root.is(Expression::AdditiveOperation));
    assert_eq!(shape(&tree.root, &tree), ["Call", "Plus(+)", "Number"]);
    let call = tree.root.node(Expression::Call).unwrap();
    assert_eq!(shape(call, &tree), ["Name", "GroupOpen(()", "Items", "GroupClose())"]);
    let items = call.node(Expression::Items).unwrap();
    assert_eq!(shape(items, &tree), ["Name"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:e25d66e413017e984faafe9fc207e7916005a7e2b5179e8a587eae06feaeab74
#[test]
fn test_a_negated_number() {
    let tree = expr("-1");
    check_lossless(&tree, "-1");
    assert!(tree.root.is(Expression::NegateOperation));
    assert_eq!(shape(&tree.root, &tree), ["Minus(-)", "Number"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:cc7c40b081965cd23cf3b3bd676ac4275f04545c91b5f9ef19ec45a5df2f0394
#[test]
fn test_a_space_after_minus_keeps_it_from_being_a_negation() {
    let source = "x = - 1;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::ExpressionStatement));
    assert_eq!(shape(statement, &tree)[0], "Name");
    assert_eq!(tree.errors.len(), 1);
    let error = &tree.errors[0];
    assert_eq!(tree.raw(error.start, error.end), "= - 1");
    assert_eq!(error.expected, vec!["Semicolon"]);
}

// @lfy def/parser/main.lfy:parse#parse:parse:ba7b39019b1fa273cf02e733e1051232e9cff31ab1d91f15e794286b4e203eed
#[test]
fn test_a_name_then_a_leftover_name() {
    let tree = expr("a b");
    check_lossless(&tree, "a b");
    assert!(tree.root.is(Expression::Expression));
    assert!(tree.root.node(Expression::Name).is_some());
    assert_eq!(tree.errors.len(), 1);
    let error = &tree.errors[0];
    assert_eq!(tree.raw(error.start, error.end), "b");
}

// @lfy def/parser/main.lfy:parse#parse:parse:62acf28d8606f1942e2c94cf221e3c0ee23ebbecc12ca0104d9b7fcffd8ce30c
#[test]
fn test_an_error_runs_past_a_semicolon_inside_parentheses() {
    let source = "const = f(a; b); let y = 2;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statements: Vec<&Node> = tree.root.nodes().collect();
    assert_eq!(statements.len(), 2);
    assert!(statements[0].is(Statement::VariableDeclaration));
    assert!(statements[1].is(Statement::VariableDeclaration));
    assert_eq!(tree.errors.len(), 1);
    let error = &tree.errors[0];
    assert_eq!(tree.raw(error.start, error.end), "= f(a; b)");
    assert!(statements[0].errors().len() == 1);
    assert!(statements[1].errors().is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:93471ef68721a03aadf57a1f1b6626b4a0c1195d6c288ab35a4c75b330c0f9b4
#[test]
fn test_a_token_without_a_rule_between_two_elements() {
    let source = "let # y = 1;";
    let tree = file(source);
    check_lossless(&tree, source);
    let declaration = first_statement(&tree);
    assert!(declaration.is(Statement::VariableDeclaration));
    assert!(declaration.node(Expression::Declared).is_some());
    assert_eq!(tree.errors.len(), 1);
    let error = &tree.errors[0];
    assert_eq!(tree.raw(error.start, error.end), "#");
    assert!(declaration.errors().len() == 1);
    let shape = shape(declaration, &tree);
    assert_eq!(shape[0], "LetKeyword(let)");
    assert_eq!(shape[1], "Error(\"#\")");
    assert_eq!(shape[2], "Declared");
}

// @lfy def/parser/main.lfy:parse#parse:parse:f77af6bf83ea4ef5cb709a99f71614f0147d505501735cb8fcf89720a7bc0a3a
#[test]
fn test_a_block_is_not_an_object_at_the_start_of_a_statement() {
    let source = "{ a = 1 };";
    let tree = file(source);
    check_lossless(&tree, source);
    let first = first_statement(&tree);
    assert!(first.is(Statement::Block));
    assert!(tree.root.find(Expression::Object).is_none());
    let inner = first.node(Statement::ExpressionStatement).unwrap();
    assert!(inner.node(Expression::Assignment).is_some());
    let inner_errors = inner.errors();
    assert_eq!(inner_errors.len(), 1);
    assert_eq!(inner_errors[0].expected, vec!["Semicolon"]);
    assert_eq!(tree.errors.len(), 2);
    let last = tree.errors.last().unwrap();
    assert_eq!(tree.raw(last.start, last.end), ";");
}

// @lfy def/parser/main.lfy:parse#parse:parse:c581d6e560359bebca8b3ce02ea5e7e246870b3a138374621c63229cccd6c214
#[test]
fn test_a_function_type_is_not_a_type_group() {
    let source = "d A: (a) => b;";
    let tree = file(source);
    check_lossless(&tree, source);
    let declaration = first_statement(&tree);
    assert!(declaration.is(Statement::DataDeclaration));
    let clause = declaration.node(Expression::DefinitionClause).unwrap();
    let function_type = clause.find(Expression::FunctionType).unwrap();
    assert_eq!(shape(function_type, &tree), ["Parameters", "DoubleArrowRight(=>)", "TypeExpression"]);
    assert!(clause.find(Expression::TypeGroup).is_none());
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:87871a94d47489aacbc1f9da346fd954e128d629ab68d94339b41805ace35f04
#[test]
fn test_a_space_after_an_accessor_leaves_no_member_name() {
    let source = "x. is T;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::ExpressionStatement));
    let traits = statement.node(Expression::Traits).unwrap();
    let member = traits.node(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Name", "ValueAccessor(.)"]);
    assert!(member.node(Expression::MemberName).is_none());
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:1ef8b2f814c5749b880487f531087ff388d1f4e1feef2b32071018af8b78a18e
#[test]
fn test_documentation_with_a_comment_between_it_and_the_declaration() {
    let source = "/// doc\n// note\nd A {}";
    let tree = file(source);
    check_lossless(&tree, source);
    let declaration = tree.root.node(Statement::DataDeclaration).unwrap();
    assert_eq!(declaration.documentation.len(), 1);
    assert!(declaration.documentation[0].is(Entity::Comment(Comment::Documentation)));
    assert_eq!(tree.raw(declaration.documentation[0].start, declaration.documentation[0].end), "/// doc");
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:5be2aa5cf14d5315eb7b372df53043e54332506ce13431b4af9fda2736f71294
#[test]
fn test_type_parameters_of_a_data_declaration() {
    let source = "d List<T>;";
    let tree = file(source);
    check_lossless(&tree, source);
    let declaration = first_statement(&tree);
    assert!(declaration.is(Statement::DataDeclaration));
    let parameters = declaration.node(Expression::TypeParameters).unwrap();
    assert_eq!(parameters.nodes_of(Expression::TypeParameter).count(), 1);
    assert_eq!(tree.raw(parameters.start, parameters.end), "<T>");
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:16d4a4f4fb54bdbffb55f3aeb1e264741c102926cd8456012a13040eeb6a1510
#[test]
fn test_a_generic_function_signature() {
    let source = "fn map<T, U>(list: List<T>, transform: (item: T) => U) => List<U>;";
    let tree = file(source);
    check_lossless(&tree, source);
    let declaration = first_statement(&tree);
    assert!(declaration.is(Statement::AgentFunctionDeclaration));
    let signature = declaration.node(Expression::Signature).unwrap();
    let type_parameters = signature.node(Expression::TypeParameters).unwrap();
    assert_eq!(type_parameters.nodes_of(Expression::TypeParameter).count(), 2);
    let parameters: Vec<&Node> = signature
        .node(Expression::Parameters)
        .unwrap()
        .nodes_of(Expression::Parameter)
        .collect();
    assert_eq!(parameters.len(), 2);
    let list = parameters[0].find(Expression::TypeItem).unwrap();
    assert_eq!(tree.raw(list.start, list.end), "List<T>");
    assert!(list.node(Expression::TypeArguments).is_some());
    let transform = parameters[1].find(Expression::FunctionType).unwrap();
    assert_eq!(tree.raw(transform.start, transform.end), "(item: T) => U");
    let returned = declaration
        .nodes_of(Expression::TypeExpression)
        .last()
        .unwrap()
        .find(Expression::TypeItem)
        .unwrap();
    assert_eq!(tree.raw(returned.start, returned.end), "List<U>");
    assert!(returned.node(Expression::TypeArguments).is_some());
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:230f6cf6284b608bd0c889f309a6c1fa57561c0b15dd4dbe44828acd51562578
#[test]
fn test_each_greater_than_closes_one_list() {
    let source = "d A: List<List<T>>;";
    let tree = file(source);
    check_lossless(&tree, source);
    let declaration = first_statement(&tree);
    let clause = declaration.node(Expression::DefinitionClause).unwrap();
    let outer = clause.find(Expression::TypeItem).unwrap();
    let arguments = outer.node(Expression::TypeArguments).unwrap();
    let inner = arguments.find(Expression::TypeItem).unwrap();
    assert_eq!(tree.raw(inner.start, inner.end), "List<T>");
    assert!(inner.node(Expression::TypeArguments).is_some());
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:ee9e137e6e115ffa55c7f1666a027f66238f1a7f0a4645ef10eefe8a3cd4b66e
#[test]
fn test_a_generic_on_the_right_of_an_assignment() {
    let source = "$items = List<T>;";
    let tree = file(source);
    check_lossless(&tree, source);
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::ExpressionStatement));
    let assignment = statement.node(Expression::Assignment).unwrap();
    let generic = assignment.node(Expression::Generic).unwrap();
    assert_eq!(tree.raw(generic.start, generic.end), "List<T>");
    assert_eq!(shape(generic, &tree), ["Name", "LessThan(<)", "TypeExpression", "GreaterThan(>)"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:36a612317aa41dafd5e2046247764cbc6a28a4d77296528f96ac90c97eca0a3a
#[test]
fn test_a_less_than_of_two_names() {
    let tree = expr("a < b");
    check_lossless(&tree, "a < b");
    assert!(tree.root.is(Expression::RelationalOperation));
    assert_eq!(shape(&tree.root, &tree), ["Name", "LessThan(<)", "Name"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:5ede9393fbf5e0e77d705e357816512172615b201da749cdae965954c336ee08
#[test]
fn test_a_greater_than_or_equal_is_no_generic() {
    let tree = expr("List<T>=x");
    check_lossless(&tree, "List<T>=x");
    assert!(tree.root.is(Expression::RelationalOperation));
    assert_eq!(shape(&tree.root, &tree), ["RelationalOperation", "GreaterThanOrEqual(>=)", "Name"]);
    let left = tree.root.node(Expression::RelationalOperation).unwrap();
    assert_eq!(shape(left, &tree), ["Name", "LessThan(<)", "Name"]);
    assert!(tree.root.find(Expression::Generic).is_none());
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:c11f5a077f08affb17bce3bc506db1eb57dd09e4f7259d592da9e6772ff8836b
#[test]
fn test_a_call_of_a_generic_member() {
    let tree = expr("x.f<T>(y)");
    check_lossless(&tree, "x.f<T>(y)");
    assert!(tree.root.is(Expression::Call));
    assert_eq!(shape(&tree.root, &tree), ["Generic", "GroupOpen(()", "Items", "GroupClose())"]);
    let generic = tree.root.node(Expression::Generic).unwrap();
    assert_eq!(shape(generic, &tree), ["Member", "LessThan(<)", "TypeExpression", "GreaterThan(>)"]);
    let items = tree.root.node(Expression::Items).unwrap();
    assert_eq!(shape(items, &tree), ["Name"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:parse#parse:parse:a81aac211d66a6e80d926aaa08d8ad2dfcb10a5c7defec7c07d9d0cec7b77710
#[test]
fn test_an_addition_of_a_call_of_a_generic() {
    let tree = expr("a + f<T>(x)");
    check_lossless(&tree, "a + f<T>(x)");
    assert!(tree.root.is(Expression::AdditiveOperation));
    assert_eq!(shape(&tree.root, &tree), ["Name", "Plus(+)", "Call"]);
    let call = tree.root.node(Expression::Call).unwrap();
    assert!(call.node(Expression::Generic).is_some());
    assert!(tree.errors.is_empty());
}

// Criteria answered by a test of their own

// @lfy def/parser/main.lfy:parse#parse:parse:edb1cd0e9ce01535c47ba76a9202465f6c26e7a839d41247f48b9fa2274cf4b8
#[test]
fn a_bare_name_in_a_rule_is_the_rule_with_that_identifier() {
    // `Group = GroupOpen , Expression , GroupClose`: each bare name is the rule of that
    // identifier, so the node holds a `GroupOpen` token, an expression, and a `GroupClose`.
    let tree = file("(a);");
    let group = first_statement(&tree).node(Expression::Group).unwrap();
    assert_eq!(shape(group, &tree), ["GroupOpen(()", "Name", "GroupClose())"]);
    for rule in rules() {
        assert_eq!(Entity::lookup(rule.identifier()), Some(rule));
    }
}

// @lfy def/parser/main.lfy:parse#parse:parse:90d2f81226a65d9ecc5867302707f041813671f31370734a3b78991f156e57be
#[test]
fn parsing_sets_no_category_and_no_binding() {
    fn snapshot() -> Vec<(Entity, String, String)> {
        rules()
            .map(|rule| (rule, format!("{:?}", rule.category()), format!("{:?}", rule.effective_binding())))
            .collect()
    }
    let before = snapshot();
    for source in ["a + b * c;", "f<T>(x);", "d A {}", "x = - 1;"] {
        let _ = file(source);
    }
    assert_eq!(snapshot(), before);
}

// @lfy def/parser/main.lfy:parse#parse:parse:bac03496c2edc9be0d4dbed2c038593d11afc9db34b99b9e544312813793c3c2
#[test]
fn a_parse_without_errors_takes_time_linear_in_the_tokens() {
    let attempts = |count: usize| {
        let source = "a = b + c * f(x, y);\n".repeat(count);
        let (tree, attempts) = parse_counting(tokens(&source), None);
        assert!(tree.errors.is_empty());
        (tree.tokens.len(), attempts.values().sum::<usize>())
    };
    let (small_tokens, small) = attempts(50);
    let (large_tokens, large) = attempts(400);
    assert_eq!(large_tokens, small_tokens * 8);
    // Eight times the tokens takes at most about eight times the attempts.
    assert!(large <= small * 9, "{small} attempts for {small_tokens} tokens, {large} for {large_tokens}");
}

// @lfy def/parser/main.lfy:parse#parse:parse:e9933d6a3a248bf673389302ede3dd728194fa7e057da54387005500e7979401
#[test]
fn no_trivia_token_and_no_token_without_a_rule_begins_a_rule() {
    for source in [" /* c */ a;", "# a;", "\n// note\n\nb = 1; # c;", "/// doc\n// note\nd A {}"] {
        let tree = file(source);
        check_lossless(&tree, source);
        for node in tree.root.descendants() {
            if node.start == node.end || components::is_trivia(node.rule) || node.rule == tree.root_rule {
                continue;
            }
            let token = &tree.tokens[node.start];
            let rule = token.rule.unwrap_or_else(|| panic!("{source:?}: {} begins at a token without a rule", node.rule.identifier()));
            assert!(
                !components::is_trivia(rule),
                "{source:?}: {} begins at the trivia {}",
                node.rule.identifier(),
                rule.identifier()
            );
        }
    }
}

// The definitions parse under the grammar they define

// @lfy def/parser/main.lfy:parse
#[test]
fn the_elfie_definitions_parse_start_to_finish() {
    let mut found = 0;
    for path in [
        "def/grammar/main.lfy",
        "def/grammar/precedence.lfy",
        "def/grammar/rules/expression.lfy",
        "def/grammar/rules/file.lfy",
        "def/grammar/rules/statement.lfy",
        "def/grammar/terminals/comment.lfy",
        "def/grammar/terminals/identifier.lfy",
        "def/grammar/terminals/keyword.lfy",
        "def/grammar/terminals/literal.lfy",
        "def/grammar/terminals/punctuation.lfy",
        "def/grammar/terminals/space.lfy",
        "def/grammar/traits.lfy",
        "def/lexer/data.lfy",
        "def/lexer/main.lfy",
        "def/lexer/modes.lfy",
        "def/lexer/traits.lfy",
        "def/parser/components.lfy",
        "def/parser/data.lfy",
        "def/parser/main.lfy",
        "def/parser/traits.lfy",
    ] {
        let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../").to_string() + path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let lexed = lex(&source, Some(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        let tree = parse(lexed, None);
        check_lossless(&tree, &source);
        let errors: Vec<String> = tree
            .errors
            .iter()
            .map(|error| tree.raw(error.start, error.end))
            .collect();
        found += errors.len();
        assert!(
            errors.is_empty(),
            "{path}: {:?}",
            tree.errors.iter().map(|error| (tree.tokens[error.start].line, tree.raw(error.start, error.end))).collect::<Vec<_>>()
        );
        assert!(tree.root.descendants().len() > 10, "{path}");
        assert!(tree.root.nodes().all(|node| node.rule.is_statement() || components::is_trivia(node.rule)), "{path}");
    }
    assert_eq!(found, 0);
}

// @lfy def/parser/main.lfy:parse#global:def/parser/components.lfy:3f95087db10d0a0a9a129ba9817ca635ced6a9eff071d21d7bf7666bde2ab6ea
#[test]
fn the_parser_compiles_because_tried_before_orders_every_choice() {
    assert_eq!(validate(), Ok(()));
    let _ = beginning::tables();
    let _ = Violation {
        terminal: Entity::Keyword(Keyword::IfKeyword),
        within: "Statement",
        candidates: vec![],
        detail: String::new(),
    };
    let _ = Literal::Backtick;
}

