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
// @lfy def/parser/main.lfy:15
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
        // @lfy def/parser/data.lfy:12
        assert!(node.start <= node.end, "{source:?}");
        assert!(
            node.children.iter().all(|child| node.start <= child.start() && child.end() <= node.end),
            "{source:?}: {}",
            node.rule.identifier()
        );
        // @lfy def/parser/data.lfy:16
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

// @lfy def/parser/main.lfy:107
#[test]
fn test_an_empty_expression() {
    let tree = expr("");
    assert_eq!(tree.root_rule, Entity::Expression(Expression::Expression));
    assert!(tree.root.is(Expression::Expression));
    assert!(tree.root.children.is_empty());
    assert_eq!((tree.root.start, tree.root.end), (0, 0));
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:111
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

// @lfy def/parser/main.lfy:115
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

// @lfy def/parser/main.lfy:119
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

// @lfy def/parser/main.lfy:123
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

// @lfy def/parser/main.lfy:127
#[test]
fn test_an_empty_file() {
    let tree = file("");
    assert!(tree.root.is(File::SourceFile));
    assert!(tree.root.children.is_empty());
    assert_eq!((tree.root.start, tree.root.end), (0, 0));
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:131
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

// @lfy def/parser/main.lfy:135
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

// @lfy def/parser/main.lfy:139
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

// @lfy def/parser/main.lfy:143
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

// @lfy def/parser/main.lfy:147
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

// @lfy def/parser/main.lfy:151
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

// parse

// @lfy def/parser/main.lfy:16
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

// @lfy def/parser/main.lfy:19
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

// @lfy def/parser/main.lfy:22
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

// @lfy def/parser/main.lfy:23
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

// @lfy def/parser/main.lfy:27
#[test]
fn an_alternation_list_root_holds_leading_trivia_the_item_and_the_leftovers() {
    let tree = expr(" a");
    check_lossless(&tree, " a");
    assert!(tree.root.is(Expression::Expression));
    assert_eq!(shape(&tree.root, &tree), ["Name"]);
    assert_eq!(tree.root.children.len(), 2);
    let tree = expr("a b");
    check_lossless(&tree, "a b");
    assert!(tree.root.is(Expression::Expression));
    assert_eq!(shape(&tree.root, &tree), ["Name", "Error(\"b\")"]);
    assert_eq!(tree.errors.len(), 1);
    // Nothing satisfies the rule: everything is the error node.
    let tree = expr("+ a");
    check_lossless(&tree, "+ a");
    assert_eq!(shape(&tree.root, &tree), ["Error(\"+ a\")"]);
    assert!(tree.errors[0].expected.contains(&"Identifier"));
    assert!(!tree.errors[0].expected.contains(&"Plus"));
    // A statement root that is exactly the item stands in its place.
    let tree = parse(tokens("a;"), Some(Entity::Statement(Statement::Statement)));
    assert!(tree.root.is(Statement::ExpressionStatement));
}

// @lfy def/parser/main.lfy:36
#[test]
fn the_parser_parses_against_the_grammar_document_only() {
    let document = grammar();
    assert_eq!(document, crate::grammar::grammar_document());
    for rule in crate::grammar::rules() {
        assert!(document.contains(rule.text()), "{rule}");
    }
    // @lfy def/parser/main.lfy:41
    // Categories and bindings come from the rules: the tables set none of them.
    let tables = beginning::tables();
    let plus = Entity::Punctuation(Punctuation::Plus);
    let additive = tables.operation(plus).unwrap();
    assert_eq!(additive.effective_binding(), Expression::AdditiveOperation.effective_binding());
    assert!(additive.is_infix());
}

// Rules

// @lfy def/parser/main.lfy:48
#[test]
fn a_terminal_is_satisfied_by_one_token_of_that_terminal() {
    let tree = file("a;");
    let statement = first_statement(&tree);
    assert_eq!(statement.children.len(), 2);
    assert_eq!(statement.children[1], Child::Token(1));
    assert!(tree.token(1).is(Punctuation::Semicolon));
    // @lfy def/parser/main.lfy:49
    let name = statement.node(Expression::Name).unwrap();
    assert_eq!(name.children, vec![Child::Token(0)]);
    assert_eq!((name.start, name.end), (0, 1));
}

// @lfy def/parser/main.lfy:50
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

// @lfy def/parser/main.lfy:57
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

// @lfy def/parser/main.lfy:61
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
    // @lfy def/grammar/rules/statement.lfy:36
    let tree = file("{ a = 1 }");
    assert!(first_statement(&tree).is(Statement::Block));
    assert_eq!(tree.errors.len(), 1);
    // The rule that comes first is recoverable, so the second is never reached once the
    // first has taken a token.
    let tree = file("function;");
    assert!(first_statement(&tree).is(Statement::FunctionDeclaration));
    assert!(!tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:62
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

// @lfy def/parser/main.lfy:63
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

// @lfy def/parser/main.lfy:75
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

// @lfy def/parser/main.lfy:84
#[test]
fn an_optional_element_is_tried_when_the_next_token_can_begin_it() {
    // The member name is optional and taken when the next token can begin it.
    let tree = file("a.;\nb.c;\nx@type;");
    let statements: Vec<&Node> = tree.root.nodes().collect();
    let member = statements[0].node(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Name", "ValueAccessor(.)"]);
    let member = statements[1].node(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Name", "ValueAccessor(.)", "Identifier(c)"]);
    let member = statements[2].node(Expression::Member).unwrap();
    assert_eq!(shape(member, &tree), ["Name", "ContextAccessor(@)", "TypeKeyword(type)"]);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    // When the optional element fails it is skipped: a leading `&` in a type position.
    let tree = file("const x: & y;");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let type_expression = first_statement(&tree).find(Expression::TypeExpression).unwrap();
    assert_eq!(shape(type_expression, &tree), ["Ampersand(&)", "TypeItem"]);
}

// @lfy def/parser/main.lfy:91
#[test]
fn a_repetition_tries_iterations_while_the_next_token_can_begin_one() {
    let tree = file("(a, b, ...rest) => a; (c, ) => c; match x { a -> b, }");
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let statements: Vec<&Node> = tree.root.nodes().collect();
    let parameters = statements[0].find(Expression::Parameters).unwrap();
    assert_eq!(
        shape(parameters, &tree),
        ["GroupOpen(()", "Parameter", "Comma(,)", "Parameter", "Comma(,)", "SpreadParameter", "GroupClose())"]
    );
    let parameters = statements[1].find(Expression::Parameters).unwrap();
    assert_eq!(shape(parameters, &tree), ["GroupOpen(()", "Parameter", "Comma(,)", "GroupClose())"]);
    assert_eq!(statements[2].nodes_of(Statement::MatchArm).count(), 1);
}

// Expressions

// @lfy def/parser/main.lfy:97
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
    // @lfy def/grammar/rules/expression.lfy:127
    let tree = expr("a as string | number");
    assert!(tree.root.is(Expression::Cast));
    assert_eq!(shape(&tree.root, &tree), ["Name", "AsKeyword(as)", "TypeExpression"]);
    assert!(tree.errors.is_empty());
}

// @lfy def/parser/main.lfy:98
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

// @lfy def/parser/main.lfy:99
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

// Errors

// @lfy def/parser/main.lfy:103
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
    // The optional setter was skipped; the element that failed is the semicolon.
    assert_eq!(tree.errors[0].expected, vec!["Semicolon"]);
    // @lfy def/parser/main.lfy:104
    // A rule without recoverable produces nothing, so the group here is left to the
    // statement, which then recovers.
    let tree = file("(a b);");
    let statement = first_statement(&tree);
    assert!(statement.is(Statement::ExpressionStatement));
    assert!(statement.node(Expression::Group).is_none());
}

// @lfy def/parser/traits.lfy:40
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
    // @lfy def/parser/traits.lfy:43
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

// @lfy def/parser/traits.lfy:46
#[test]
fn a_stopped_repetition_ends_or_covers_the_stopping_tokens() {
    // A token an element after the repetition begins with ends it.
    let tree = file("{ a; }");
    assert!(tree.errors.is_empty());
    // @lfy def/parser/traits.lfy:47
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
    // @lfy def/parser/traits.lfy:48
    // Any other token is covered up to the next token that begins the repeated element,
    // an element after it, or a sync token.
    let source = "{ ) ] a; }";
    let tree = file(source);
    check_lossless(&tree, source);
    let block = first_statement(&tree);
    assert_eq!(
        shape(block, &tree),
        ["BlockOpen({)", "Error(\") ] \")", "ExpressionStatement", "BlockClose(})"]
    );
    let tree = file(") a;");
    assert_eq!(shape(&tree.root, &tree), ["Error(\") \")", "ExpressionStatement"]);
}

// @lfy def/parser/data.lfy:21
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

// @lfy def/parser/data.lfy:21
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
    assert_eq!(tree.errors[0].expected, &["IsKeyword", "ExtendsKeyword", "BlockOpen", "Semicolon", "Colon"]);
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

// @lfy def/parser/main.lfy:24
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

// @lfy def/grammar/traits.lfy:108
#[test]
fn an_invalid_token_after_a_prefix_operator_is_not_space() {
    let tree = file("x = -#1;");
    let negate = first_statement(&tree).find(Expression::NegateOperation).unwrap();
    assert_eq!(shape(negate, &tree), ["Minus(-)", "Error(\"#\")", "Number"]);
    assert_eq!(tree.errors.len(), 1);
}

// @lfy def/parser/traits.lfy:51
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
}

// trivia

// @lfy def/parser/traits.lfy:28
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

// @lfy def/grammar/terminals/comment.lfy:39
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

// @lfy def/parser/traits.lfy:32
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

// triedBefore

// @lfy def/parser/traits.lfy:36
#[test]
fn the_rule_tried_first_wins_and_the_other_is_tried_only_when_it_fails() {
    let tree = file("(a) => a; (a); where (a) -> b; {} x = {};");
    let statements: Vec<&Node> = tree.root.nodes().collect();
    assert!(statements[0].node(Expression::InlineFunction).is_some());
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

// @lfy def/grammar/rules/expression.lfy:70
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
    // @lfy def/grammar/traits.lfy:133
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

// @lfy def/grammar/rules/expression.lfy:74
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

// @lfy def/grammar/traits.lfy:108
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

// @lfy def/grammar/rules/expression.lfy:76
#[test]
fn a_block_after_the_double_arrow_is_a_block_and_a_conditional_owns_its_colon() {
    let tree = expr("() => { a = 1; }");
    assert!(tree.root.is(Expression::InlineFunction));
    assert!(tree.root.node(Statement::Block).is_some());
    assert!(tree.root.node(Expression::Object).is_none());
    // @lfy def/grammar/rules/expression.lfy:128
    let tree = expr("a ? x : T : c");
    assert!(tree.root.is(Expression::Definition));
    let conditional = tree.root.node(Expression::Conditional).unwrap();
    assert_eq!(shape(conditional, &tree), ["Name", "QuestionMark(?)", "Name", "Colon(:)", "Name"]);
}

// data

// @lfy def/parser/data.lfy:4
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

// @lfy def/parser/data.lfy:28
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

// The definitions parse under the grammar they define

// @lfy def/parser/main.lfy:17
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

// @lfy def/parser/main.lfy:177
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

