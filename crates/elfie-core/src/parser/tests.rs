//! Tests compiled from the acceptance criteria and `@test` cases of `def/parser/*.lfy`
//! and from the grammar criteria the parser applies.

use super::*;
use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::file::File;
use crate::grammar::rules::statement::Statement;
use crate::grammar::terminals::identifier::Identifier;
use crate::grammar::terminals::keyword::Keyword;
use crate::grammar::terminals::literal::Literal;
use crate::grammar::terminals::punctuation::Punctuation;
use crate::lexer::lex;

fn tokens(source: &str) -> Vec<Token> {
    lex(source, None).unwrap_or_else(|error| panic!("{source:?}: {error}"))
}

/// `Token[]@like(`The lexing of: …`)` parsed for the whole file.
fn file(source: &str) -> Tree {
    parse(tokens(source), None)
}

/// … parsed for an expression.
fn expr(source: &str) -> Tree {
    parse(tokens(source), Some(Entity::Expression(Expression::Expression)))
}

/// The children of a node that its elements produced, each named: a node by its rule, a
/// token by its terminal and raw text, an error node by `Error`.
fn shape(node: &Node, tree: &Tree) -> Vec<String> {
    node.significant(&tree.tokens)
        .into_iter()
        .map(|child| match child {
            Child::Node(node) => node.rule.identifier().to_owned(),
            Child::Error(error) => format!("Error({:?})", tree.raw(error.start, error.end)),
            Child::Token(index) => {
                let token = &tree.tokens[*index];
                format!("{}({})", token.rule.unwrap().identifier(), token.raw)
            }
        })
        .collect()
}

/// The `Tree@like` check every tree must pass: the root covers every token exactly once
/// and the errors are those in the tree, ordered by start.
// @lfy def/parser/main.lfy:parse
fn check_lossless(tree: &Tree, source: &str) {
    assert_eq!(tree.root.start, 0, "{source:?}");
    assert_eq!(tree.root.end, tree.tokens.len(), "{source:?}");
    let reached = tree.root.token_indices();
    assert_eq!(reached, (0..tree.tokens.len()).collect::<Vec<_>>(), "{source:?}");
    assert_eq!(tree.raw(0, tree.tokens.len()), source, "{source:?}");
    let in_tree: Vec<ErrorNode> = tree.root.errors().into_iter().cloned().collect();
    let mut sorted = in_tree.clone();
    sorted.sort_by_key(|error| error.start);
    assert_eq!(tree.errors, sorted, "{source:?}");
    for node in tree.root.descendants() {
        // @lfy def/parser/data.lfy:Node
        assert!(node.start <= node.end, "{source:?}");
        assert!(
            node.children.iter().all(|child| node.start <= child.start() && child.end() <= node.end),
            "{source:?}: {}",
            node.rule.identifier()
        );
        // @lfy def/parser/data.lfy:Node
        assert!(
            !node.rule.is_alternation_list() || node.rule == tree.root_rule,
            "{source:?}: {}",
            node.rule.identifier()
        );
    }
}

fn first_statement(tree: &Tree) -> &Node {
    tree.root.nodes().next().expect("a statement")
}

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

// @lfy def/parser/main.lfy:parse#parse:parse:a6e88bbebd9637216f3c0cadb5b3a87cd2201a000d7c07f82c254e6d2cca2d8f
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

// @lfy def/parser/main.lfy:parse#parse:parse:085f4541fdac12041a7d961636301805bd71c052d2cbd707344331bdad595d2d
#[test]
fn test_an_error_node_has_no_keyword_when_none_of_its_tokens_are_one() {
    let source = "const = 1;";
    let tree = file(source);
    check_lossless(&tree, source);
    assert_eq!(tree.errors.len(), 1);
    assert!(tree.errors[0].keyword.is_none());
}

// parse

// @lfy def/parser/main.lfy:parse#parse:parse:3f35ff181697621541a9298b81d89397807df5d1961e264f3307eedb34a650e5
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

// @lfy def/parser/main.lfy:parse#parse:parse:3f35ff181697621541a9298b81d89397807df5d1961e264f3307eedb34a650e5
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

// @lfy def/parser/main.lfy:parse#parse:parse:54f128054fc19398b5e648f5ec344eaf7f85d6da005de481dd15820329670646
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

// @lfy def/parser/main.lfy:parse#parse:parse:0ae4cc24d47e2f0f276f866cdb5663645bf4d44ae50e2d8aa0e7593d839b75a9
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

// @lfy def/parser/main.lfy:parse#parse:parse:0a62225249e29a6574042386e16928ffb99e3d1554435a9c5d7d48d2945b7d16
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

// @lfy def/parser/main.lfy:parse#parse:parse:6954bc94999e3e1d5c02044bdd997e19e125f30951437897ddc32b9157b7d01d
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

// @lfy def/parser/main.lfy:parse#parse:parse:ede3a0f56a3cb6e16f63484b69a9ab1eb084232340b4c78e3e8a61c58b3a2e68
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
    // @lfy def/grammar/rules/statement.lfy:Block
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

// @lfy def/parser/main.lfy:parse#parse:parse:228ac08feb110e0d1be0152283de70a96500488f2211c0f993b12661ff399377
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

// @lfy def/parser/main.lfy:parse#parse:parse:51f9f1895d366556fabcb96f770e23b3cafac57b3a1e64c0819c12401b8eef82
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

// @lfy def/parser/main.lfy:parse#parse:parse:88922a29a0f0d1d631771e55c0276656c3851375d91c7860fa827883a186d116
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

// @lfy def/parser/main.lfy:parse#parse:parse:80183e8ed69ffdce6ec75354328ffbee70ab49de04dae9c5f1a835f67a8e5dbd
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

// @lfy def/parser/main.lfy:parse#parse:parse:570bc7b6a84742e084c9b82aa44aa31162a3a7602929b5e56215ce5d32d02299
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

// @lfy def/parser/main.lfy:parse#parse:parse:ca1ac5f3c97e1083d46795ca68dd4af55a2636fd878008961fb26a671a63b855
#[test]
fn an_optional_element_that_is_tried_and_fails_is_skipped() {
    // A leading `&` in a type position can begin the optional union operator, but the
    // element fails there, so it is skipped and the `&` belongs to the type expression.
    let tree = file("const x: & y;");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let type_expression = first_statement(&tree).find(Expression::TypeExpression).unwrap();
    assert_eq!(shape(type_expression, &tree), ["Ampersand(&)", "TypeItem"]);
}

// @lfy def/parser/main.lfy:parse#parse:parse:0ab01e045b160fb8b0a5bd592667a637039c617a30417781c73c32471a4e80b1
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

// @lfy def/parser/main.lfy:parse#parse:parse:e836c72272aea4f33a32a661edcc0e6cb5ec87adb16205b6c82d97c361f48cdd
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

// @lfy def/parser/main.lfy:parse#parse:parse:9c3767cb9a59599479bc09a083b2a2708a04d1b1732d734c777b12260a27ef1b
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

// @lfy def/parser/main.lfy:parse#parse:parse:5a7652382aeff6dc355d1da81e1e9106e2b56b7f73304e3a02486cf27d7322ee
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

// @lfy def/parser/main.lfy:parse#parse:parse:99405f6cf9c6a5e54f3067d541686b159f24b44be7dd3ee4b47b1f12f997642e
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

// @lfy def/parser/main.lfy:parse#parse:parse:c5434301155f78027c7957a31536a926468ffa3b82f8ac1093f532bee11a3f03
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

// @lfy def/parser/main.lfy:parse#parse:parse:7e74975ee875acea83afea95137e69df310549cb4b20534c96a76628bd43fa46
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

// @lfy def/parser/main.lfy:parse#parse:parse:7185c3dbfd8022f2f4a3c0486615c5f13e4b5deb0a6ceaf5cfbab35572ac9968
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

// @lfy def/parser/main.lfy:parse#parse:parse:de35158e51addc98a605ac2f36217d513a79f206fe4d9f6973d7af1639de192c
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

// @lfy def/parser/main.lfy:parse#parse:parse:9c11cec24e885f91edd6a4efce9dfb2cfa4862573074eac789bc5ee64110ce1f
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

// @lfy def/parser/main.lfy:parse#parse:parse:bd89bb1dec6711dc056ba284188acd4c1a500b9ab4036d5e4eab862aa814d74e
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
    // @lfy def/parser/main.lfy:parse#parse:parse:d26e01007222d8cbc0f684150ca1e552df077e9030d4de76495208bb8eedb9f5
    // A rule without recoverable produces nothing, so the group here is left to the
    // statement, which then recovers.
    let tree = file("(a b);");
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::ExpressionStatement));
    assert!(statement.node(Expression::Group).is_none());
}

// @lfy def/parser/main.lfy:parse#parse:parse:f189f369c6fdcb0b4372fc0423e4b92867ee37a2cc9cbe5fff911e10fd9b4977
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

// @lfy def/parser/main.lfy:parse#parse:parse:3f7d43b3d948dafc8be7139b2aec66606e03f088a3278947c20050c6aba3c7d6
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

// @lfy def/parser/main.lfy:parse#parse:parse:0a5b6c4a94dec29395a4dd12408dcd6c4d7a01c1b57040202de98ae688cfe0b3
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

// @lfy def/parser/traits.lfy:documented.documentation
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

// @lfy def/grammar/rules/statement.lfy:DataDeclaration
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
fn nodes_expose_their_rule_and_error_nodes_have_none() {
    let tree = file("a;");
    let statement = first_statement(&tree);
    assert_eq!(statement.rule().identifier, "ExpressionStatement");
    assert_eq!(statement.rule().text, Statement::ExpressionStatement.text());
    let tree = file("const ;");
    assert_eq!(tree.errors[0].rule(), None);
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

#[test]
fn every_statement_rule_is_tried_where_the_next_token_can_begin_it() {
    // @lfy def/parser/main.lfy:parse#parse:parse:16832a8120acaac7bb35f630bad63321dc2f5df0f9f03f68681eea3cfee26986
    assert!(statement_tried(Statement::DataDeclaration));
    // @lfy def/parser/main.lfy:parse#parse:parse:e82fed2b50b5f0d59c2fb4d4f9801292275cb751d2f8c0656892c4cd43ed2e5d
    assert!(statement_tried(Statement::AgentFunctionDeclaration));
    // @lfy def/parser/main.lfy:parse#parse:parse:47c66106f601e22b0c2f44f62c88da18dd7192cfa3068bc72617e43d357b9f4c
    assert!(statement_tried(Statement::FunctionDeclaration));
    // @lfy def/parser/main.lfy:parse#parse:parse:0be0ade7eb062161a38576647ad58c81789fa071a6b29897d2bb3735a70812a2
    assert!(statement_tried(Statement::TraitDeclaration));
    // @lfy def/parser/main.lfy:parse#parse:parse:6182884a5dede234e2a9cff1897b14bdb5140a0fbab4bfde99da31aae335e935
    assert!(statement_tried(Statement::TypeDeclaration));
    // @lfy def/parser/main.lfy:parse#parse:parse:394d7174c384822d3309e0989ea8ee8abf62ddb70a1c395e9f55784c88b19cae
    assert!(statement_tried(Statement::EnumDeclaration));
    // @lfy def/parser/main.lfy:parse#parse:parse:e703592ac1466a81966af28128e675308d8bdc4d264b139396eaa4921c3ff2eb
    assert!(statement_tried(Statement::VariableDeclaration));
    // @lfy def/parser/main.lfy:parse#parse:parse:a0c7e1fe7458a080bd4b0b7f527bb2c09a1ae9615d2047c257fbd1ef2c5da09d
    assert!(statement_tried(Statement::AliasDeclaration));
    // @lfy def/parser/main.lfy:parse#parse:parse:8cb2151a0ad40531ae7cb98724f8ca454dd94fc83a3ae6f8947c37374c7e51bd
    assert!(statement_tried(Statement::ExternalDeclaration));
    // @lfy def/parser/main.lfy:parse#parse:parse:3fd4f6ea52af9127cb1cc3d7c0e90ae526e66c81d2d6e946d35dff1fd2b4868f
    assert!(statement_tried(Statement::Use));
    // @lfy def/parser/main.lfy:parse#parse:parse:252ce7951899effaee21aa05e3b0086237db19ca2aa4be4289fd39114b9992ca
    assert!(statement_tried(Statement::Block));
    // @lfy def/parser/main.lfy:parse#parse:parse:bb16065526b8559cdcf10da9470bb7d458df85f08701fa9c4c4a4b659d310ac8
    assert!(statement_tried(Statement::If));
    // @lfy def/parser/main.lfy:parse#parse:parse:e520cfa26a8e88c428ef3c674a5da14a138215b01d679a41209410d70842c590
    assert!(statement_tried(Statement::For));
    // @lfy def/parser/main.lfy:parse#parse:parse:54649986d169177c4bd75064009fc88d3e15a91b2ebbdfcb3cfb6cc187a6b4cd
    assert!(statement_tried(Statement::While));
    // @lfy def/parser/main.lfy:parse#parse:parse:367cefb46dc9ffe69381e44cbe368c2a2f7d104330cdfc15a2fccf188f5845c2
    assert!(statement_tried(Statement::Loop));
    // @lfy def/parser/main.lfy:parse#parse:parse:c41e7dcc9f7204c588a7ed429dbe3582d58220c79ad4a04fe5ff7a280cb70dcf
    assert!(statement_tried(Statement::Break));
    // @lfy def/parser/main.lfy:parse#parse:parse:25f258e21ca684423ea6b9a57be85a3d827f5ac76bd57fdea668e2024952f806
    assert!(statement_tried(Statement::Continue));
    // @lfy def/parser/main.lfy:parse#parse:parse:76e6a409271b001496347a8b78d826de836a4889aea51e42a90d7cf2fab3cd3e
    assert!(statement_tried(Statement::Return));
    // @lfy def/parser/main.lfy:parse#parse:parse:46f24d9515af21811d3da0671a62bc29c88221d9207ee239d9a2f9ec24bd585c
    assert!(statement_tried(Statement::Match));
    // @lfy def/parser/main.lfy:parse#parse:parse:86a7e6fd5fa74f43a18e7d8e23300956b88748f61109a5a0bab64bb8c8c271b7
    assert!(statement_tried(Statement::With));
    // @lfy def/parser/main.lfy:parse#parse:parse:51e129de2897037593446d2b9713aa8fc29e3eb00e3e7f92d9c42a1b88fedc02
    assert!(statement_tried(Statement::Async));
    // @lfy def/parser/main.lfy:parse#parse:parse:eab2a6c461e5d650245192bfac6318370e369272946130f10c67e333c4d2e040
    assert!(statement_tried(Statement::Ace));
    // @lfy def/parser/main.lfy:parse#parse:parse:0e13f8942db4da88be0d1d77b63d973e9eb60199ffd8892307fa4da60c825d79
    assert!(statement_tried(Statement::ExpressionStatement));
    // @lfy def/parser/main.lfy:parse#parse:parse:fcb653b910e01a3234b5a5823bc41314cdc67a6401766ff6aa211dc084f4f355
    assert!(statement_tried(Statement::Where));
}

#[test]
fn every_primary_rule_is_tried_where_the_next_token_can_begin_it() {
    // @lfy def/parser/main.lfy:parse#parse:parse:7b89900ce3f3d10432d81d655d2469bb2dada7430b1a586f5fcb5e684391ed3e
    assert!(expression_tried(Expression::StringLiteral));
    // @lfy def/parser/main.lfy:parse#parse:parse:8d1f185115356d99cab6fb27e10bd528a352d520f67985d4326ecaecd2a23b5a
    assert!(expression_tried(Expression::Template));
    // @lfy def/parser/main.lfy:parse#parse:parse:08a69a762e9b7443e6de8700a0b7921ced22b4f37d33e2a47e008faf4bd8d05e
    assert!(expression_tried(Expression::Number));
    // @lfy def/parser/main.lfy:parse#parse:parse:b792d24df4584bc8885428b0fa36d9645de1317b29f731a769a0fff0c1cc0abb
    assert!(expression_tried(Expression::Boolean));
    // @lfy def/parser/main.lfy:parse#parse:parse:b28c351f97986ea7999ea7faf014f2c6a3208248f1ee89a6b1f0dc682d6636fd
    assert!(expression_tried(Expression::Nullish));
    // @lfy def/parser/main.lfy:parse#parse:parse:625c84d4af577d66e9ef8b3f3520197e496c0c628495c312cda99aa2034dbbcf
    assert!(expression_tried(Expression::PrimitiveType));
    // @lfy def/parser/main.lfy:parse#parse:parse:31948b8a36a5e6c26fadf4743b705af4aa7015238df853e33a4133ba460a16b5
    assert!(expression_tried(Expression::Name));
    // @lfy def/parser/main.lfy:parse#parse:parse:b924096bb725ee35d1e68c84a92fb88d6ab6f2e97c770eb32c30c98189255bde
    assert!(expression_tried(Expression::Current));
    // @lfy def/parser/main.lfy:parse#parse:parse:dd17e1a3ebcbf4f43466b3adce830886d1d680f0c4bf3e6bb0de2de15e57e912
    assert!(expression_tried(Expression::Previous));
    // @lfy def/parser/main.lfy:parse#parse:parse:715a2d729d2e293055ffe8a4502160ccca5320c58d07de62de3c7ab9828c4afe
    assert!(expression_tried(Expression::Group));
    // @lfy def/parser/main.lfy:parse#parse:parse:aa24e4d7e936b3272199b19413d9223be459874e4e26702d4a44b350809d4182
    assert!(expression_tried(Expression::InlineFunction));
    // @lfy def/parser/main.lfy:parse#parse:parse:a0846d2db33dae302b6614d769a6701f98a7e097761585f395a4e2ce6494dfb4
    assert!(expression_tried(Expression::List));
    // @lfy def/parser/main.lfy:parse#parse:parse:5a574d4a2a318e9e9c797ed00c9eeae89ddc06b1c0676ea1f4c96ae42eb3bbb1
    assert!(expression_tried(Expression::Object));
    // @lfy def/parser/main.lfy:parse#parse:parse:c87bed0155db7cea6d6400e9866512e837b25acaa7b9d3eaf3c8d4aded52f1a3
    assert!(expression_tried(Expression::TypePredicate));
    // @lfy def/parser/main.lfy:parse#parse:parse:302081c1b0c4efe5058c4ad5f1b0697e902492b9606a4426800e8a92316b4a0d
    assert!(expression_tried(Expression::Type));
}

#[test]
fn every_prefix_rule_is_tried_where_the_next_token_can_begin_it() {
    // @lfy def/parser/main.lfy:parse#parse:parse:946f504f7f02afa2bf87f56ef97a5d4d076b3f736aa7de478f7cdb68fedc6206
    assert!(expression_tried(Expression::NotOperation));
    // @lfy def/parser/main.lfy:parse#parse:parse:a6641dff9c18b4f93d67b1f2dbc0bf85c4fc70d00993e0cc2394aea037aafbc3
    assert!(expression_tried(Expression::NegateOperation));
    // @lfy def/parser/main.lfy:parse#parse:parse:ef8111adddd9b772aaae12110cdd63d51f1be427b22e0efb3281e1c828216905
    assert!(expression_tried(Expression::BitwiseNotOperation));
    // @lfy def/parser/main.lfy:parse#parse:parse:96e9a21f753ceb0c6cc75b0d5aa15ee6503e7bab7f70a55d24af6d588a6230b1
    assert!(expression_tried(Expression::Dereference));
    // @lfy def/parser/main.lfy:parse#parse:parse:97ef01f44693a882ceb50cd884e3e29698dd6aa215cd0313562443c351676ede
    assert!(expression_tried(Expression::SpreadOperation));
    // @lfy def/parser/main.lfy:parse#parse:parse:991cdfbc84876155dab03f7dbafeb64969e1cbe78711e504b2cf8288096c8442
    assert!(expression_tried(Expression::AwaitOperation));
    // @lfy def/parser/main.lfy:parse#parse:parse:df27b4d99fe9bff592e15a153effd01d392e0e4f6f06ebf7821de1e3d0093021
    assert!(expression_tried(Expression::InOperation));
    // @lfy def/parser/main.lfy:parse#parse:parse:3f297f3ef4145b190c409ce442da69593c62384166535813531f6eced3d9a3ce
    assert!(expression_tried(Expression::OfOperation));
    // @lfy def/parser/main.lfy:parse#parse:parse:c8fcd7e578df543c9426a29e8766fb360483b32dff00eba13f003a6e822cbeed
    assert!(expression_tried(Expression::FromOperation));
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

#[test]
fn every_infix_operation_is_built_from_the_left_operand_the_operator_and_what_follows() {
    // @lfy def/parser/main.lfy:parse#parse:parse:437e35462673b057e8170394e3b3bac0795480379e488caeb79f061d3e5e5c5a
    assert_eq!(operation("a + b", Expression::AdditiveOperation), ["Name", "Plus(+)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:2b362e7fde6282ae7a198fd87303bbf224f176689866b773f69119c4a72eccc2
    assert_eq!(operation("a * b", Expression::MultiplicativeOperation), ["Name", "Star(*)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:85b67ff6f89e3e467a6313a111a086f1592b3dc4a7b0429ccf85d723c1654193
    assert_eq!(operation("a ** b", Expression::PowerOperation), ["Name", "Power(**)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:27ffba22d2cf2c53ee6b0a720f3755c8f98854a30d200e99c9bf42a762d01416
    assert_eq!(operation("a | b", Expression::BitwiseOrOperation), ["Name", "BitwiseOr(|)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:9dac42a1bc2908ab71cf3757f73919dc7c514e0e7590bd38bf870187c68e7d39
    assert_eq!(operation("a ^ b", Expression::BitwiseXorOperation), ["Name", "BitwiseXor(^)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:3f269d8facec689b9a1c68e1354a1e098af391ffa75cec648f08b21097984881
    assert_eq!(operation("a & b", Expression::BitwiseAndOperation), ["Name", "Ampersand(&)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:c4974c4b9d5066073fa4443bb325d0bc700f926e6d49781d024b2f38a432770f
    assert_eq!(operation("a <= b", Expression::RelationalOperation), ["Name", "LessThanOrEqual(<=)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:b83bed752bcdfdcce0c7474d9d7fde0e71351e5b2266cfb24bde2b5ae553b525
    assert_eq!(operation("a == b", Expression::EqualityOperation), ["Name", "Equal(==)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:e12f8885e872c38351b85ae28eaa89fd0f6632be85b8df01db145a59f8d6b794
    assert_eq!(operation("a && b", Expression::LogicalAndOperation), ["Name", "LogicalAnd(&&)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:39a962420843c951b4d6d12dc1d3c25071d80ad7b18348626c7a462cf491ab4d
    assert_eq!(operation("a ?? b", Expression::CoalescenceOperation), ["Name", "NullishOr(??)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:54578c9aaead10fcecd8b1c9aa6e5b9152fe869f1a25f0471ca1b843b4cfc397
    assert_eq!(operation("a ... b", Expression::RangeOperation), ["Name", "Spread(...)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:1a99ad04ec9008b30ec4199ee98005c61b50135ea3b6f0ca59dd6ddf123308d4
    assert_eq!(operation("a = b", Expression::Assignment), ["Name", "PlainSetter(=)", "Name"]);
}

#[test]
fn every_postfix_operation_is_built_from_the_left_operand_the_operator_and_what_follows() {
    // @lfy def/parser/main.lfy:parse#parse:parse:05d23c8fd62a172c91b9ae854770cc09d36a0b9e6bf99019bcc54d8bcc58b540
    assert_eq!(operation("a.b", Expression::Member), ["Name", "ValueAccessor(.)", "Identifier(b)"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:bd047ceef22480020575d85cd076a2d0d5ec316f5acdf5b63a5c784aca0a9567
    assert_eq!(operation("a[b]", Expression::Index), ["Name", "ListOpen([)", "Name", "ListClose(])"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:68e861bb1f5c87f8afb3e9e990d5968ef0cc8cec0b5533c71a3fb58bf95cacc0
    assert_eq!(operation("a(b)", Expression::Call), ["Name", "GroupOpen(()", "Items", "GroupClose())"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:83ba45aac2571bfc5817e6ffd3fa79d3bc0d02890ef34f286e42745c5d2b6f9e
    assert_eq!(operation("a is t", Expression::Traits), ["Name", "IsKeyword(is)", "TraitUses"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:a6834dd3ad2fa899c42d2c969a4938b0d972894cab7005b3871c740c95857232
    assert_eq!(operation("a : b", Expression::Definition), ["Name", "Colon(:)", "Name"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:143a2d2aaa4165e872f22a2ac95668150c5b75b7304ca78f27340f15f29501f3
    assert_eq!(operation("a as T", Expression::Cast), ["Name", "AsKeyword(as)", "TypeExpression"]);
    // @lfy def/parser/main.lfy:parse#parse:parse:4399c5e5ca1505056e4f793bac891083bbc498938106049e29927317d6798271
    assert_eq!(
        operation("a<T>", Expression::Generic),
        ["Name", "LessThan(<)", "TypeExpression", "GreaterThan(>)"]
    );
    // @lfy def/parser/main.lfy:parse#parse:parse:0f0e0fcf00a4d3c77d2ddff600ba9cabd9f10116992768315302e0b6d5e58043
    assert_eq!(
        operation("a ? b : c", Expression::Conditional),
        ["Name", "QuestionMark(?)", "Name", "Colon(:)", "Name"]
    );
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

// @lfy def/parser/main.lfy:parse#global:def/parser/main.lfy:1c7fdc9e99e5a132c4b70443a4a57960b652345ba09540a2f414a25b9a1fe1ed
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

