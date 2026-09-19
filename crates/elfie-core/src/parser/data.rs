//! Compiled from `def/parser/data.lfy`.

use std::fmt::Write as _;

use crate::grammar::{Entity, GrammarRule, Rule};
use crate::lexer::Token;

/// One entry of [`Node::children`]: a node, an error node, or a token. A token is given
/// by its index into [`Tree::tokens`], where the token itself lives.
// @lfy def/parser/data.lfy:6
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Child {
    Node(Node),
    Error(ErrorNode),
    Token(usize),
}

impl Child {
    /// Index of the first token covered.
    pub fn start(&self) -> usize {
        match self {
            Child::Node(node) => node.start,
            Child::Error(error) => error.start,
            Child::Token(index) => *index,
        }
    }

    /// Index after the last token covered.
    pub fn end(&self) -> usize {
        match self {
            Child::Node(node) => node.end,
            Child::Error(error) => error.end,
            Child::Token(index) => index + 1,
        }
    }

    pub fn as_node(&self) -> Option<&Node> {
        match self {
            Child::Node(node) => Some(node),
            _ => None,
        }
    }

    pub fn as_error(&self) -> Option<&ErrorNode> {
        match self {
            Child::Error(error) => Some(error),
            _ => None,
        }
    }

    pub fn as_token(&self) -> Option<usize> {
        match self {
            Child::Token(index) => Some(*index),
            _ => None,
        }
    }
}

/// A rule satisfied by a run of consecutive tokens.
///
/// Joining the raw text of the tokens from `start` up to `end` reproduces the source the
/// node covers, and each token below the node is reached through exactly one child. An
/// `alternationList` rule has no node; the item that satisfied it stands in its place.
// @lfy def/parser/data.lfy:4
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The rule this node satisfies. [`Node::rule`] gives its EBNF form.
    pub rule: Entity, // @lfy def/parser/data.lfy:5
    /// Nodes and tokens in source order, trivia included.
    pub children: Vec<Child>, // @lfy def/parser/data.lfy:6
    /// Index of the first token covered, counting from 0 over [`Tree::tokens`].
    pub start: usize, // @lfy def/parser/data.lfy:7
    /// Index after the last token covered.
    pub end: usize, // @lfy def/parser/data.lfy:8
    /// `$documentation` of the `documented` trait: the `Documentation` nodes that precede
    /// this node with only trivia between; empty when there are none, and always empty
    /// for rules without the trait.
    pub documentation: Vec<Node>, // @lfy def/parser/traits.lfy:32
}

impl Node {
    /// A node for `rule` covering nothing yet, starting at `start`.
    pub(crate) fn open(rule: Entity, start: usize) -> Node {
        Node {
            rule,
            children: Vec::new(),
            start,
            end: start,
            documentation: Vec::new(),
        }
    }

    /// `$rule` as the EBNF form of the rule this node satisfies.
    // @lfy def/parser/data.lfy:5
    pub fn rule(&self) -> Rule {
        self.rule.rule()
    }

    /// Whether the node satisfies this rule.
    pub fn is<R: GrammarRule>(&self, rule: R) -> bool {
        self.rule == rule.entity()
    }

    /// The child nodes, in order.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(Child::as_node)
    }

    /// The child nodes satisfying `rule`, in order.
    pub fn nodes_of<R: GrammarRule>(&self, rule: R) -> impl Iterator<Item = &Node> {
        let rule = rule.entity();
        self.nodes().filter(move |node| node.rule == rule)
    }

    /// The first child node satisfying `rule`.
    pub fn node<R: GrammarRule>(&self, rule: R) -> Option<&Node> {
        self.nodes_of(rule).next()
    }

    /// The index of the first child token whose token satisfies `rule`, looked up in
    /// `tokens`.
    pub fn token<R: GrammarRule>(&self, rule: R, tokens: &[Token]) -> Option<usize> {
        let rule = rule.entity();
        self.children
            .iter()
            .filter_map(Child::as_token)
            .find(|&index| tokens[index].rule == Some(rule))
    }

    /// Every node below this one, this one first, in source order.
    pub fn descendants(&self) -> Vec<&Node> {
        let mut out = Vec::new();
        self.collect_descendants(&mut out);
        out
    }

    fn collect_descendants<'n>(&'n self, out: &mut Vec<&'n Node>) {
        out.push(self);
        for child in &self.children {
            if let Child::Node(node) = child {
                node.collect_descendants(out);
            }
        }
    }

    /// The first node at or below this one satisfying `rule`, in source order.
    pub fn find<R: GrammarRule>(&self, rule: R) -> Option<&Node> {
        let rule = rule.entity();
        self.descendants().into_iter().find(|node| node.rule == rule)
    }

    /// Every error node below this one, in source order.
    pub fn errors(&self) -> Vec<&ErrorNode> {
        let mut out = Vec::new();
        self.collect_errors(&mut out);
        out
    }

    fn collect_errors<'n>(&'n self, out: &mut Vec<&'n ErrorNode>) {
        for child in &self.children {
            match child {
                Child::Node(node) => node.collect_errors(out),
                Child::Error(error) => out.push(error),
                Child::Token(_) => {}
            }
        }
    }

    /// The index of every token reached below this node, in the order it is reached.
    pub fn token_indices(&self) -> Vec<usize> {
        let mut out = Vec::new();
        self.collect_tokens(&mut out);
        out
    }

    fn collect_tokens(&self, out: &mut Vec<usize>) {
        for child in &self.children {
            match child {
                Child::Node(node) => node.collect_tokens(out),
                Child::Error(error) => out.extend(error.children.iter().filter_map(Child::as_token)),
                Child::Token(index) => out.push(*index),
            }
        }
    }

    /// The child nodes that are not trivia and not error nodes, plus the child tokens
    /// that are not trivia: what the rule's elements produced.
    pub fn significant<'n>(&'n self, tokens: &'n [Token]) -> Vec<&'n Child> {
        self.children
            .iter()
            .filter(|child| match child {
                Child::Node(node) => !super::components::is_trivia(node.rule),
                Child::Error(_) => true,
                Child::Token(index) => tokens[*index]
                    .rule
                    .is_some_and(|rule| !super::components::is_trivia(rule)),
            })
            .collect()
    }
}

/// Tokens the parser could not fit into the open rule.
// @lfy def/parser/data.lfy:19
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorNode {
    /// The tokens covered, each as [`Child::Token`], in source order.
    pub children: Vec<Child>, // @lfy def/parser/data.lfy:6
    /// Index of the first token covered.
    pub start: usize, // @lfy def/parser/data.lfy:7
    /// Index after the last token covered.
    pub end: usize, // @lfy def/parser/data.lfy:8
    /// Identifiers of what could have continued the open rule at `start`.
    pub expected: Vec<&'static str>, // @lfy def/parser/data.lfy:21
}

impl ErrorNode {
    /// An error node covering the tokens from `start` up to `end`.
    pub fn new(start: usize, end: usize, expected: Vec<&'static str>) -> ErrorNode {
        ErrorNode {
            children: (start..end).map(Child::Token).collect(),
            start,
            end,
            expected,
        }
    }

    /// `$rule`: no rule is being satisfied due to the error.
    // @lfy def/parser/data.lfy:20
    pub fn rule(&self) -> Option<Rule> {
        None
    }
}

/// The result of parsing one token list.
// @lfy def/parser/data.lfy:24
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    /// The tokens parsed, exactly as the lexer delivered them.
    pub tokens: Vec<Token>, // @lfy def/parser/data.lfy:25
    /// The root rule that was being used to build the tree. [`Tree::root_rule`] gives its
    /// EBNF form.
    pub root_rule: Entity, // @lfy def/parser/data.lfy:26
    /// The root node, covering every token.
    pub root: Node, // @lfy def/parser/data.lfy:27
    /// Every error node, if any, in the tree ordered by start.
    pub errors: Vec<ErrorNode>, // @lfy def/parser/data.lfy:28
}

impl Tree {
    /// `$rootRule` as the EBNF form of the root rule.
    // @lfy def/parser/data.lfy:26
    pub fn root_rule(&self) -> Rule {
        self.root_rule.rule()
    }

    /// The token a [`Child::Token`] refers to.
    pub fn token(&self, index: usize) -> &Token {
        &self.tokens[index]
    }

    /// The raw source text of the tokens from `start` up to `end`, joined in order.
    pub fn raw(&self, start: usize, end: usize) -> String {
        self.tokens[start..end]
            .iter()
            .map(|token| token.raw.as_str())
            .collect()
    }

    /// The raw source text a child covers.
    pub fn raw_of(&self, child: &Child) -> String {
        self.raw(child.start(), child.end())
    }

    /// The tree as indented text, one node or token per line: rules with the token range
    /// they cover, error nodes with what they expected, and tokens with their raw text.
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.render_node(&self.root, 0, &mut out);
        out
    }

    fn render_child(&self, child: &Child, depth: usize, out: &mut String) {
        match child {
            Child::Node(node) => self.render_node(node, depth, out),
            Child::Error(error) => {
                let _ = writeln!(
                    out,
                    "{:indent$}Error {}..{} expected [{}] {:?}",
                    "",
                    error.start,
                    error.end,
                    error.expected.join(", "),
                    self.raw(error.start, error.end),
                    indent = depth * 2
                );
            }
            Child::Token(index) => {
                let token = &self.tokens[*index];
                let rule = token.rule.map_or("invalid", |rule| rule.identifier());
                let _ = writeln!(out, "{:indent$}{rule} {:?}", "", token.raw, indent = depth * 2);
            }
        }
    }

    fn render_node(&self, node: &Node, depth: usize, out: &mut String) {
        let _ = writeln!(
            out,
            "{:indent$}{} {}..{}",
            "",
            node.rule.identifier(),
            node.start,
            node.end,
            indent = depth * 2
        );
        for child in &node.children {
            self.render_child(child, depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::rules::expression::Expression;
    use crate::grammar::rules::file::File;
    use crate::grammar::terminals::identifier::Identifier;
    use crate::lexer::lex;

    // @lfy def/parser/data.lfy:19
    #[test]
    fn an_error_node_covers_its_tokens_and_has_no_rule() {
        let error = ErrorNode::new(2, 5, vec!["Identifier"]);
        assert_eq!(error.children, vec![Child::Token(2), Child::Token(3), Child::Token(4)]);
        assert_eq!((error.start, error.end), (2, 5));
        assert_eq!(error.rule(), None);
        assert_eq!(error.expected, vec!["Identifier"]);
        let empty = ErrorNode::new(3, 3, vec![]);
        assert!(empty.children.is_empty());
    }

    // @lfy def/parser/data.lfy:4
    #[test]
    fn a_node_records_its_rule_children_and_range() {
        let tokens = lex("a b", None).unwrap();
        let mut name = Node::open(Entity::Expression(Expression::Name), 0);
        name.children.push(Child::Token(0));
        name.end = 1;
        assert_eq!(name.rule().identifier, "Name");
        assert!(name.is(Expression::Name));
        assert!(!name.is(Expression::Number));
        assert_eq!(name.token(Identifier::Identifier, &tokens), Some(0));
        let mut root = Node::open(Entity::File(File::SourceFile), 0);
        root.children.push(Child::Node(name.clone()));
        root.children.push(Child::Token(1));
        root.children.push(Child::Error(ErrorNode::new(2, 3, vec!["Semicolon"])));
        root.end = 3;
        assert_eq!(root.nodes().count(), 1);
        assert_eq!(root.node(Expression::Name), Some(&name));
        assert_eq!(root.find(Expression::Name), Some(&name));
        assert_eq!(root.find(Expression::Number), None);
        assert_eq!(root.descendants().len(), 2);
        assert_eq!(root.errors().len(), 1);
        assert_eq!(root.token_indices(), vec![0, 1, 2]);
        assert_eq!(root.significant(&tokens).len(), 2);
        let tree = Tree {
            tokens,
            root_rule: Entity::File(File::SourceFile),
            root,
            errors: vec![ErrorNode::new(2, 3, vec!["Semicolon"])],
        };
        assert_eq!(tree.root_rule().identifier, "SourceFile");
        assert_eq!(tree.raw(0, 3), "a b");
        assert_eq!(tree.raw_of(&Child::Token(1)), " ");
        assert_eq!(tree.token(2).raw, "b");
        let rendered = tree.render();
        assert!(rendered.starts_with("SourceFile 0..3\n  Name 0..1\n    Identifier \"a\"\n"));
        assert!(rendered.contains("Error 2..3 expected [Semicolon] \"b\""));
        assert_eq!(Child::Token(4).end(), 5);
        assert!(Child::Token(4).as_node().is_none());
    }
}
