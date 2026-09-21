//! Compiled from `def/format/main.lfy`.
//!
//! [`format`] lays out the source of a lossless [`Tree`] the standard way. The formatter
//! walks the tree rather than the tokens, so the layout of every token follows the rule it
//! belongs to; the tokens themselves are copied verbatim, only the trivia between them is
//! decided here. It is its own module because both the command line and the language
//! server format, and neither should own the rules.
// @lfy def/format/main.lfy:format

use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::file::File;
use crate::grammar::rules::statement::Statement;
use crate::grammar::terminals::comment::Comment;
use crate::grammar::terminals::identifier::Identifier;
use crate::grammar::terminals::literal::Literal;
use crate::grammar::terminals::punctuation::Punctuation;
use crate::grammar::terminals::space::Space;
use crate::grammar::{Entity, GrammarRule};
use crate::parser::components::is_trivia;
use crate::parser::{Child, ErrorNode, Node, Tree};

/// The column a line may reach before a bracketed list inside it is laid out one item per
/// line.
// @lfy def/format/main.lfy:format
pub const MAX_WIDTH: usize = 120;

/// The indentation of one enclosing block, object, type, or multi-line list.
// @lfy def/format/main.lfy:format
pub const INDENT: &str = "  ";

/// The source of a tree, laid out the standard way.
///
/// The tokens of the result, trivia aside, are [`Tree::tokens`] with the same value in
/// the same order; formatting the result again gives the same result; the text of every
/// body token, comment, and documentation is kept exactly, even its line breaks; every
/// line ends with one line feed and the result ends with one. When [`Tree::errors`] is
/// not empty the result is the source unchanged, because moving tokens the parser could
/// not place would hide the error.
// @lfy def/format/main.lfy:format
pub fn format(tree: &Tree) -> String {
    // @lfy def/format/main.lfy:format
    if !tree.errors.is_empty() {
        return tree.raw(0, tree.tokens.len());
    }
    let mut formatter = Formatter::new(tree, false);
    formatter.root();
    formatter.finish()
}

/// What sits between the text written so far and the next token: nothing, one space, or a
/// line break (with a blank line before it when `blank`). A line break always wins over a
/// space, which is how trailing spaces never appear.
// @lfy def/format/main.lfy:format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    None,
    Space,
    Line { blank: bool },
}

/// A written token: its terminal, the rule of the node it is a child of, and its position
/// among the significant children of that node. The spacing rules read all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tok {
    rule: Entity,
    parent: Entity,
    position: usize,
}

/// The state of one formatting pass. In `flat` mode the pass measures how wide a node is
/// when laid out on one line: no line breaks are written, and `impossible` records that
/// the node holds something that cannot sit on one line (a comment, or a block with
/// statements).
struct Formatter<'t> {
    tree: &'t Tree,
    out: String,
    indent: usize,
    pending: Pending,
    last: Option<Tok>,
    /// `NewLine` tokens read since the last token or comment was written.
    newlines: usize,
    flat: bool,
    impossible: bool,
}

// Terminals the rules below name

const NEW_LINE: Entity = Entity::Space(Space::NewLine);
const COMMA: Entity = Entity::Punctuation(Punctuation::Comma);
const BLOCK_OPEN: Entity = Entity::Punctuation(Punctuation::BlockOpen);
const LESS_THAN: Entity = Entity::Punctuation(Punctuation::LessThan);
const GREATER_THAN: Entity = Entity::Punctuation(Punctuation::GreaterThan);
const MEMBER: Entity = Entity::Expression(Expression::Member);
const CALL: Entity = Entity::Expression(Expression::Call);
const INDEX: Entity = Entity::Expression(Expression::Index);
const ITEMS: Entity = Entity::Expression(Expression::Items);
const TYPE_PARAMETERS: Entity = Entity::Expression(Expression::TypeParameters);
const TYPE_ARGUMENTS: Entity = Entity::Expression(Expression::TypeArguments);
const GENERIC: Entity = Entity::Expression(Expression::Generic);

/// Whether the rule is one link of a `Call` and `Member` chain.
// @lfy def/format/main.lfy:format
fn is_link(rule: Entity) -> bool {
    matches!(rule, MEMBER | CALL | INDEX)
}

/// Whether the rule holds its items between a `LessThan` and a `GreaterThan` that are
/// brackets rather than ordering operators.
// @lfy def/format/main.lfy:format
fn is_angled(rule: Entity) -> bool {
    matches!(rule, TYPE_PARAMETERS | TYPE_ARGUMENTS | GENERIC)
}

/// Whether the rule is an accessor.
fn is_accessor(rule: Entity) -> bool {
    matches!(
        rule,
        Entity::Punctuation(
            Punctuation::ValueAccessor
                | Punctuation::OptionalValueAccessor
                | Punctuation::ContextAccessor
                | Punctuation::ScopeAccessor
                | Punctuation::ParentScopeAccessor
        )
    )
}

/// Whether the terminal closes a bracket that a list can be laid out inside.
fn closes_bracket(rule: Entity) -> bool {
    matches!(
        rule,
        Entity::Punctuation(
            Punctuation::GroupClose | Punctuation::ListClose | Punctuation::BlockClose
        )
    )
}

/// No space after `a`: after an opening bracket or boundary, inside a text body, after a
/// prefix operator that is not a keyword, and after an accessor.
// @lfy def/format/main.lfy:format
fn glue_after(a: Tok) -> bool {
    match a.rule {
        // @lfy def/format/main.lfy:format
        Entity::Punctuation(Punctuation::GroupOpen | Punctuation::ListOpen) => true,
        // The `LessThan` of type parameters and type arguments is a bracket, not an
        // ordering operator, so nothing separates it from the first item.
        // @lfy def/format/main.lfy:format
        LESS_THAN if is_angled(a.parent) => true,
        Entity::Literal(Literal::ExecutionOpen | Literal::ReferenceOpen) => true,
        Entity::Literal(Literal::Backtick | Literal::SingleQuote | Literal::DoubleQuote) => {
            a.position == 0
        }
        Entity::Literal(
            Literal::TemplateBody | Literal::SingleQuoteBody | Literal::DoubleQuoteBody,
        ) => true,
        // A `where` condition's negation is written like the prefix operator it mirrors.
        Entity::Punctuation(Punctuation::LogicalNot) => {
            a.parent.is_prefix()
                || matches!(
                    a.parent,
                    Entity::Statement(Statement::Condition | Statement::ConditionGroup)
                )
        }
        Entity::Punctuation(Punctuation::Spread)
            if a.parent == Entity::Expression(Expression::SpreadParameter) =>
        {
            true
        }
        rule if is_accessor(rule) => matches!(
            a.parent,
            Entity::Expression(Expression::Member | Expression::TraitUse)
        ),
        // Decision: a keyword prefix operator (`await`, `in`, `of`, `from`) keeps the space a
        // keyword always has after it; the "no space" rule is for symbol operators.
        // @lfy def/format/main.lfy:format
        rule => a.parent.is_prefix() && !rule.is_keyword(),
    }
}

/// No space before `b`: before a comma, semicolon, closing bracket or boundary, inside a
/// text body, before the colon and question mark of a definition, and before the operator
/// of a postfix operation that is not a keyword.
// @lfy def/format/main.lfy:format
fn glue_before(a: Tok, b: Tok) -> bool {
    match b.rule {
        // @lfy def/format/main.lfy:format
        Entity::Punctuation(
            Punctuation::Comma
            | Punctuation::Semicolon
            | Punctuation::GroupClose
            | Punctuation::ListClose,
        ) => true,
        // Type parameters attach to the identifier before them and type arguments to the
        // expression before them, and their `GreaterThan` closes them like a bracket.
        // @lfy def/format/main.lfy:format
        LESS_THAN | GREATER_THAN if is_angled(b.parent) => true,
        Entity::Literal(Literal::ExecutionClose | Literal::ReferenceClose) => true,
        Entity::Literal(Literal::Backtick | Literal::SingleQuote | Literal::DoubleQuote) => {
            b.position > 0
        }
        Entity::Literal(
            Literal::TemplateBody | Literal::SingleQuoteBody | Literal::DoubleQuoteBody,
        ) => true,
        // Decision: the colon and question mark of a `Conditional` keep a space on both sides
        // (`a ? b : c`); the no-space rules are for definitions and optional names.
        Entity::Punctuation(Punctuation::Colon) => matches!(
            b.parent,
            Entity::Expression(Expression::Definition | Expression::DefinitionClause)
        ),
        Entity::Punctuation(Punctuation::QuestionMark) => matches!(
            b.parent,
            Entity::Expression(Expression::Parameter | Expression::TypeKey)
        ),
        rule if is_accessor(rule) => matches!(
            b.parent,
            Entity::Expression(Expression::Member | Expression::TraitUse)
        ),
        // The member name of `Current` follows its accessor directly (`@type`), while a bare
        // accessor (`. is binding`) keeps its space.
        _ if b.parent == Entity::Expression(Expression::Current) && b.position == 1 => true,
        Entity::Punctuation(Punctuation::ListOpen) => matches!(
            b.parent,
            Entity::Expression(Expression::Index | Expression::TypeItem)
        ),
        Entity::Punctuation(Punctuation::GroupOpen) => {
            matches!(
                b.parent,
                Entity::Expression(Expression::Call | Expression::Arguments)
            ) || (b.parent == Entity::Expression(Expression::Parameters)
                && (a.rule == Entity::Identifier(Identifier::Identifier)
                    // @lfy def/format/main.lfy:format
                    || (a.rule == GREATER_THAN && is_angled(a.parent))))
        }
        _ => false,
    }
}

/// Whether no space separates two consecutive tokens; every other pair is separated by
/// one space, or by the line break the layout decided.
// @lfy def/format/main.lfy:format
fn glued(a: Tok, b: Tok) -> bool {
    glue_after(a) || glue_before(a, b)
}

impl<'t> Formatter<'t> {
    fn new(tree: &'t Tree, flat: bool) -> Formatter<'t> {
        Formatter {
            tree,
            out: String::new(),
            indent: 0,
            pending: Pending::None,
            last: None,
            newlines: 0,
            flat,
            impossible: false,
        }
    }

    /// A pass that measures one-line width, sharing the tree.
    fn measurer(&self) -> Formatter<'t> {
        Formatter::new(self.tree, true)
    }

    // The text

    /// The result: every line ends with one line feed and so does the result.
    // @lfy def/format/main.lfy:format
    fn finish(mut self) -> String {
        if self.out.is_empty() {
            // Decision: a tree with no tokens formats to the empty text, which has no line
            // to end.
            return String::new();
        }
        while self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out.push('\n');
        self.out
    }

    fn space(&mut self) {
        if self.pending == Pending::None {
            self.pending = Pending::Space;
        }
    }

    /// The next token begins a new line, after a blank line when `blank`; a run of line
    /// breaks is one line break, and blank when any of them was.
    // @lfy def/format/main.lfy:format
    fn newline(&mut self, blank: bool) {
        if self.flat {
            self.space();
            return;
        }
        let blank = blank || matches!(self.pending, Pending::Line { blank: true });
        self.pending = Pending::Line { blank };
    }

    /// Writes what is pending: a space, or a line feed (two for a blank line) and the
    /// indentation. Nothing precedes the first line.
    // @lfy def/format/main.lfy:format
    fn flush(&mut self) {
        match self.pending {
            Pending::None => {}
            Pending::Space => {
                if !self.out.is_empty() {
                    self.out.push(' ');
                }
            }
            Pending::Line { blank } => {
                if !self.out.is_empty() {
                    self.out.push('\n');
                    if blank {
                        self.out.push('\n');
                    }
                }
                for _ in 0..self.indent {
                    self.out.push_str(INDENT);
                }
            }
        }
        self.pending = Pending::None;
    }

    /// The column the next character lands on.
    fn column(&self) -> usize {
        let start = self.out.rfind('\n').map_or(0, |index| index + 1);
        let current = self.out[start..].chars().count();
        match self.pending {
            Pending::None => current,
            Pending::Space => current + 1,
            Pending::Line { .. } => self.indent * INDENT.len(),
        }
    }

    /// Writes one token's text after the separator the rules give it: none when `glue`
    /// or when the pair is [`glued`], one space otherwise, unless a line break is pending.
    // @lfy def/format/main.lfy:format
    fn write(&mut self, text: &str, tok: Tok, glue: bool) {
        let glue = glue || self.last.is_none_or(|last| glued(last, tok));
        if !glue {
            self.space();
        }
        self.flush();
        self.out.push_str(text);
        self.last = Some(tok);
        self.newlines = 0;
    }

    /// Writes the token at `index` as a child of `parent`, keeping its raw text.
    // @lfy def/format/main.lfy:format
    fn token(&mut self, index: usize, parent: Entity, position: usize) {
        self.token_glued(index, parent, position, false);
    }

    fn token_glued(&mut self, index: usize, parent: Entity, position: usize, glue: bool) {
        let token = &self.tree.tokens[index];
        let rule = token
            .rule
            .unwrap_or(Entity::Identifier(Identifier::Identifier));
        let tok = Tok {
            rule,
            parent,
            position,
        };
        let raw = token.raw.clone();
        self.write(&raw, tok, glue);
    }

    /// A trivia token: a line break is counted, a space carries nothing.
    // @lfy def/format/main.lfy:format
    fn trivia_token(&mut self, index: usize) {
        if self.tree.tokens[index].rule == Some(NEW_LINE) {
            self.newlines += 1;
        }
    }

    fn is_trivia_token(&self, index: usize) -> bool {
        self.tree.tokens[index].rule.is_some_and(is_trivia)
    }

    /// Whether a child is one the rule's elements produced rather than trivia.
    fn significant(&self, child: &Child) -> bool {
        match child {
            Child::Node(node) => !is_trivia(node.rule),
            Child::Error(_) => true,
            Child::Token(index) => !self.is_trivia_token(*index),
        }
    }

    /// Writes a comment or documentation exactly as written. On its own line in the
    /// source (a line break precedes it, or nothing does) it stays on its own line, after
    /// a blank line when `blank` allows one, and what follows begins a new line; sharing a
    /// line with code it stays at the end of that line after one space.
    // @lfy def/format/main.lfy:format
    fn comment(&mut self, node: &Node, blank: bool) {
        if self.flat {
            self.impossible = true;
            return;
        }
        let own_line = self.newlines > 0 || self.last.is_none();
        if own_line {
            self.newline(blank && self.newlines >= 2);
        } else {
            self.space();
        }
        self.flush();
        // @lfy def/format/main.lfy:format
        let text = self.tree.raw(node.start, node.end);
        self.out.push_str(&text);
        self.last = Some(Tok {
            rule: node.rule,
            parent: node.rule,
            position: 0,
        });
        self.newlines = 0;
        let ends_with_line = self.tree.tokens[node.start].rule.is_some_and(|rule| {
            matches!(
                rule,
                Entity::Comment(Comment::LineCommentOpen | Comment::LineDocumentationOpen)
            )
        });
        if own_line || ends_with_line {
            self.newline(false);
        }
    }

    /// Tokens the parser could not place; never reached, since a tree with errors is
    /// returned unchanged.
    fn error(&mut self, error: &ErrorNode) {
        let raw = self.tree.raw(error.start, error.end);
        let tok = Tok {
            rule: Entity::Identifier(Identifier::Identifier),
            parent: Entity::Identifier(Identifier::Identifier),
            position: 0,
        };
        self.write(&raw, tok, false);
    }

    // The walk

    /// The root: a file's statements, or any other rule laid out as a node.
    fn root(&mut self) {
        let root = &self.tree.root;
        if root.rule == Entity::File(File::SourceFile) {
            self.statements(root, 0, root.children.len());
        } else {
            self.node(root);
        }
    }

    /// Lays out one node by the rule it satisfies.
    fn node(&mut self, node: &Node) {
        match node.rule {
            Entity::Statement(Statement::Block) => self.block(node),
            Entity::Comment(Comment::Comment | Comment::Documentation) => self.comment(node, false),
            // @lfy def/format/main.lfy:format
            Entity::Expression(
                Expression::Object
                | Expression::Type
                | Expression::List
                | Expression::Arguments
                | Expression::Parameters
                // @lfy def/format/main.lfy:format
                | Expression::TypeParameters
                | Expression::TypeArguments,
            ) => {
                let open = self.first_significant(node).unwrap_or(0);
                self.children(node, 0, open);
                self.bracketed(node, open);
            }
            // The type arguments of a `Generic` follow the expression they apply to.
            // @lfy def/format/main.lfy:format
            GENERIC => {
                let open = node
                    .children
                    .iter()
                    .position(|child| {
                        matches!(child, Child::Token(index) if self.tree.tokens[*index].rule == Some(LESS_THAN))
                    })
                    .unwrap_or(0);
                self.children(node, 0, open);
                self.bracketed(node, open);
            }
            // Decision: the arms of a `match` are a bracketed list too, laid out like an
            // object.
            Entity::Statement(Statement::Match) => {
                let open = node
                    .children
                    .iter()
                    .position(|child| {
                        matches!(child, Child::Token(index) if self.tree.tokens[*index].rule == Some(BLOCK_OPEN))
                    })
                    .unwrap_or(0);
                self.children(node, 0, open);
                self.bracketed(node, open);
            }
            // @lfy def/format/main.lfy:format
            rule if is_link(rule) => self.chain(node),
            _ => self.children(node, 0, node.children.len()),
        }
    }

    /// The token index of the child at `at`, when it is a token.
    fn child_token(&self, node: &Node, at: usize) -> Option<usize> {
        match node.children.get(at) {
            Some(Child::Token(index)) => Some(*index),
            _ => None,
        }
    }

    /// Index of the first significant child.
    fn first_significant(&self, node: &Node) -> Option<usize> {
        node.children
            .iter()
            .position(|child| self.significant(child))
    }

    /// Index of the last significant child.
    fn last_significant(&self, node: &Node) -> Option<usize> {
        node.children
            .iter()
            .rposition(|child| self.significant(child))
    }

    /// Lays out `node.children[from..to]` in order: tokens with the spacing rules, nodes
    /// by their rules, trivia as line-break counts and comments.
    fn children(&mut self, node: &Node, from: usize, to: usize) {
        let mut position = node.children[..from]
            .iter()
            .filter(|child| self.significant(child))
            .count();
        for child in &node.children[from..to] {
            match child {
                Child::Token(index) => {
                    if self.is_trivia_token(*index) {
                        self.trivia_token(*index);
                    } else {
                        self.token(*index, node.rule, position);
                        position += 1;
                    }
                }
                Child::Node(inner) => {
                    if is_trivia(inner.rule) {
                        self.comment(inner, false);
                    } else {
                        self.node(inner);
                        position += 1;
                    }
                }
                Child::Error(error) => {
                    self.error(error);
                    position += 1;
                }
            }
        }
    }

    // Lines and indentation

    /// Statements in `node.children[from..to]`: each begins on its own line at the current
    /// indentation, a run of blank lines between two of them (or the comments between
    /// them) becomes one blank line, and no blank line precedes the first.
    // @lfy def/format/main.lfy:format
    fn statements(&mut self, node: &Node, from: usize, to: usize) {
        let mut first = true;
        for child in &node.children[from..to] {
            match child {
                Child::Token(index) if self.is_trivia_token(*index) => self.trivia_token(*index),
                Child::Token(index) => {
                    self.newline(false);
                    self.token(*index, node.rule, 0);
                    first = false;
                }
                Child::Node(inner) if is_trivia(inner.rule) => {
                    // @lfy def/format/main.lfy:format
                    self.comment(inner, !first);
                    first = false;
                }
                Child::Node(inner) => {
                    // @lfy def/format/main.lfy:format
                    self.newline(!first && self.newlines >= 2);
                    self.node(inner);
                    first = false;
                }
                Child::Error(error) => {
                    self.newline(false);
                    self.error(error);
                    first = false;
                }
            }
        }
    }

    /// A block opens on the line of its statement, holds its statements one level deeper,
    /// and its close stands alone on its own line; an empty block is the two braces
    /// together.
    // @lfy def/format/main.lfy:format
    fn block(&mut self, node: &Node) {
        let Some(close) = self.last_significant(node) else {
            return;
        };
        let open = self.first_significant(node).unwrap_or(0);
        self.children(node, 0, open);
        let Some(open_token) = self.child_token(node, open) else {
            return;
        };
        self.token(open_token, node.rule, 0);
        let content = node.children[open + 1..close]
            .iter()
            .any(|child| !matches!(child, Child::Token(index) if self.is_trivia_token(*index)));
        let Some(close_token) = self.child_token(node, close) else {
            return;
        };
        if !content {
            self.token_glued(close_token, node.rule, 1, true);
            return;
        }
        if self.flat {
            self.impossible = true;
        }
        self.indent += 1;
        self.statements(node, open + 1, close);
        self.indent -= 1;
        self.newline(false);
        self.token(close_token, node.rule, 1);
    }

    // Wrapping

    /// The children between the open and close brackets of a list, with the children of
    /// an `Items` node inlined, so items, commas and trivia are one sequence.
    fn entries<'n>(&self, node: &'n Node, open: usize, close: usize) -> Vec<&'n Child> {
        let mut entries = Vec::new();
        for child in &node.children[open + 1..close] {
            match child {
                Child::Node(inner) if inner.rule == ITEMS => entries.extend(inner.children.iter()),
                other => entries.push(other),
            }
        }
        entries
    }

    /// Whether a significant node or error follows entry `at`.
    fn item_follows(&self, entries: &[&Child], at: usize) -> bool {
        entries[at + 1..]
            .iter()
            .any(|child| !matches!(child, Child::Token(_)) && self.significant(child))
    }

    /// Whether the next significant entry after `at` is a comma.
    fn comma_follows(&self, entries: &[&Child], at: usize) -> bool {
        entries[at + 1..]
            .iter()
            .find(|child| self.significant(child))
            .is_some_and(|child| matches!(child, Child::Token(index) if self.tree.tokens[*index].rule == Some(COMMA)))
    }

    /// Whether the last token of the node closes a bracket.
    fn ends_with_bracket(&self, node: &Node) -> bool {
        let mut node = node;
        loop {
            match node
                .children
                .iter()
                .rev()
                .find(|child| self.significant(child))
            {
                Some(Child::Node(inner)) => node = inner,
                Some(Child::Token(index)) => {
                    return self.tree.tokens[*index].rule.is_some_and(closes_bracket);
                }
                _ => return false,
            }
        }
    }

    /// A bracketed list: `node.children[open]` is the open bracket and the last
    /// significant child the close. It is multi-line when the source held a line break
    /// or a comment between the brackets, or when the line would exceed [`MAX_WIDTH`]:
    /// then one item per line, each ending with a comma, the close on its own line.
    /// Otherwise it stays on one line and a trailing comma before the close is removed.
    ///
    /// Type parameters, type arguments, and the arguments of a `Generic` go multi-line
    /// the same way, but only for length: whatever the source held between the
    /// `LessThan` and the `GreaterThan`, they stay on one line while they fit.
    // @lfy def/format/main.lfy:format
    fn bracketed(&mut self, node: &Node, open: usize) {
        let Some(close) = self.last_significant(node) else {
            return;
        };
        if close <= open {
            self.children(node, open, node.children.len());
            return;
        }
        let entries = self.entries(node, open, close);
        let position = node.children[..open]
            .iter()
            .filter(|child| self.significant(child))
            .count();
        let Some(open_token) = self.child_token(node, open) else {
            return;
        };
        self.token(open_token, node.rule, position);
        let items: Vec<&Node> = entries
            .iter()
            .filter_map(|child| match child {
                Child::Node(inner) if !is_trivia(inner.rule) => Some(inner),
                _ => None,
            })
            .collect();
        let comment = entries
            .iter()
            .any(|child| matches!(child, Child::Node(inner) if is_trivia(inner.rule)));
        let line_break = entries
            .iter()
            .any(|child| matches!(child, Child::Token(index) if self.tree.tokens[*index].rule == Some(NEW_LINE)));
        // Decision: a call whose one argument ends with a bracket (an object, a list, a
        // block, another call) is never wrapped for length; the argument wraps itself, so
        // `.add({` keeps its shape.
        let hugs = matches!(node.rule, CALL | Entity::Expression(Expression::Arguments))
            && items.len() == 1
            && self.ends_with_bracket(items[0]);
        let multiline = if is_angled(node.rule) {
            // @lfy def/format/main.lfy:format
            !self.flat && !items.is_empty() && self.exceeds(node, open, close)
        } else {
            !self.flat
                && (comment
                    || (!items.is_empty()
                        && (line_break || (!hugs && self.exceeds(node, open, close)))))
        };
        self.list(node, open, close, multiline);
    }

    /// Whether the list, laid out on one line from the current column, would run past
    /// [`MAX_WIDTH`] or cannot be laid out on one line at all.
    // @lfy def/format/main.lfy:format
    fn exceeds(&self, node: &Node, open: usize, close: usize) -> bool {
        let mut measurer = self.measurer();
        measurer.list(node, open, close, false);
        // Decision: the width counted is the list itself up to its close; what follows the
        // close on the line (a semicolon, another close) is not counted.
        measurer.impossible || self.column() + measurer.out.chars().count() > MAX_WIDTH
    }

    /// Lays out the entries after the open bracket and then the close, one item per line
    /// with a comma after each when `multiline`, on one line without a trailing comma
    /// otherwise.
    // @lfy def/format/main.lfy:format
    fn list(&mut self, node: &Node, open: usize, close: usize, multiline: bool) {
        let entries = self.entries(node, open, close);
        let parent = node.rule;
        let saved = self.indent;
        if multiline {
            self.indent += 1;
        }
        let mut any = false;
        for (at, child) in entries.iter().enumerate() {
            match child {
                Child::Token(index) if self.is_trivia_token(*index) => self.trivia_token(*index),
                Child::Token(index) if self.tree.tokens[*index].rule == Some(COMMA) => {
                    // @lfy def/format/main.lfy:format
                    if multiline || self.item_follows(&entries, at) {
                        self.token(*index, parent, 1);
                    }
                }
                Child::Token(index) => self.token(*index, parent, 1),
                Child::Node(inner) if is_trivia(inner.rule) => {
                    any = true;
                    self.comment(inner, false);
                }
                Child::Node(inner) => {
                    any = true;
                    if multiline {
                        self.newline(false);
                    }
                    self.node(inner);
                    if multiline && !self.comma_follows(&entries, at) {
                        let tok = Tok {
                            rule: COMMA,
                            parent,
                            position: 1,
                        };
                        self.write(",", tok, true);
                    }
                }
                Child::Error(error) => {
                    any = true;
                    self.error(error);
                }
            }
        }
        if multiline {
            self.indent = saved;
            self.newline(false);
        }
        let position = node.children[..close]
            .iter()
            .filter(|child| self.significant(child))
            .count();
        if let Some(close_token) = self.child_token(node, close) {
            self.token_glued(close_token, parent, position, !any);
        }
    }

    /// A chain of `Call`, `Member` and `Index` on one left side, `node` being its
    /// outermost link. When the source held a line break before a value accessor of the
    /// chain, each value accessor begins a new line indented one level below the line the
    /// chain begins on.
    // @lfy def/format/main.lfy:format
    fn chain(&mut self, node: &Node) {
        let mut links = Vec::new();
        let mut current = node;
        let head = loop {
            links.push(current);
            match current.children.first() {
                Some(Child::Node(left)) if is_link(left.rule) => current = left,
                Some(Child::Node(left)) => break Some(left),
                _ => break None,
            }
        };
        links.reverse();
        // Decision: only the value accessors (`.`, `?.`) break; `@`, `$` and `$&` links stay
        // glued to their left, so `global@acceptanceCriteria` is one line of the chain.
        let broken = !self.flat && links.iter().any(|link| self.breaks(link));
        let line_indent = self.indent;
        match head {
            Some(head) => self.node(head),
            None => {
                if let Some(first) = links[0].children.first() {
                    match first {
                        Child::Token(index) => self.token(*index, links[0].rule, 0),
                        Child::Error(error) => self.error(error),
                        Child::Node(_) => unreachable!("a node head is handled above"),
                    }
                }
            }
        }
        if broken {
            self.indent = line_indent + 1;
        }
        for link in links {
            if link.children.is_empty() {
                continue;
            }
            match link.rule {
                CALL => {
                    let open = link.children[1..]
                        .iter()
                        .position(|child| self.significant(child))
                        .map_or(link.children.len(), |offset| offset + 1);
                    self.children(link, 1, open);
                    if open < link.children.len() {
                        self.bracketed(link, open);
                    }
                }
                MEMBER => {
                    let mut position = 1;
                    for child in &link.children[1..] {
                        match child {
                            Child::Token(index) if self.is_trivia_token(*index) => {
                                self.trivia_token(*index)
                            }
                            Child::Token(index) => {
                                if broken && position == 1 && self.value_accessor(*index) {
                                    self.newline(false);
                                }
                                self.token(*index, link.rule, position);
                                position += 1;
                            }
                            Child::Node(inner) if is_trivia(inner.rule) => {
                                self.comment(inner, false)
                            }
                            Child::Node(inner) => {
                                self.node(inner);
                                position += 1;
                            }
                            Child::Error(error) => {
                                self.error(error);
                                position += 1;
                            }
                        }
                    }
                }
                _ => self.children(link, 1, link.children.len()),
            }
        }
        if broken {
            self.indent = line_indent;
        }
    }

    /// Whether the token is a value accessor.
    fn value_accessor(&self, index: usize) -> bool {
        matches!(
            self.tree.tokens[index].rule,
            Some(Entity::Punctuation(
                Punctuation::ValueAccessor | Punctuation::OptionalValueAccessor
            ))
        )
    }

    /// Whether a `Member` link held a line break before its value accessor in the source.
    // @lfy def/format/main.lfy:format
    fn breaks(&self, link: &Node) -> bool {
        if link.rule != MEMBER {
            return false;
        }
        let mut line_break = false;
        for child in &link.children[1..] {
            match child {
                Child::Token(index) if self.tree.tokens[*index].rule == Some(NEW_LINE) => {
                    line_break = true
                }
                Child::Token(index) if self.is_trivia_token(*index) => {}
                Child::Token(index) => return line_break && self.value_accessor(*index),
                _ => {}
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;

    /// `Tree@like(`The parse of: …`)`
    fn tree(source: &str) -> Tree {
        let tokens = lex(source, None).unwrap_or_else(|error| panic!("{source:?}: {error}"));
        parse(tokens, None)
    }

    fn formatted(source: &str) -> String {
        format(&tree(source))
    }

    /// The tokens that are neither space nor line break, as terminal and value, with the
    /// trailing commas the layout adds or removes left out.
    // @lfy def/format/main.lfy:format
    fn tokens_of(tree: &Tree) -> Vec<(Entity, String)> {
        let kept: Vec<&crate::lexer::Token> = tree
            .tokens
            .iter()
            .filter(|token| !matches!(token.rule, Some(Entity::Space(_))))
            .collect();
        let mut out = Vec::new();
        for (index, token) in kept.iter().enumerate() {
            if token.rule == Some(COMMA) {
                let next = kept[index + 1..]
                    .iter()
                    .find(|token| !matches!(token.rule, Some(Entity::Comment(_))));
                if next.is_none_or(|next| next.rule.is_some_and(closes_bracket)) {
                    continue;
                }
            }
            out.push((token.rule.unwrap(), token.value.clone()));
        }
        out
    }

    /// Every `.lfy` file under `def`.
    fn def_files() -> Vec<(String, String)> {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../def");
        let mut paths = Vec::new();
        let mut pending = vec![std::path::PathBuf::from(root)];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "lfy") {
                    paths.push(path);
                }
            }
        }
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let source = std::fs::read_to_string(&path).unwrap();
                (path.display().to_string(), source)
            })
            .collect()
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn test_a_constant_declaration() {
        assert_eq!(formatted("const   x=1 ;"), "const x = 1;\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn test_a_function_with_one_criterion() {
        assert_eq!(
            formatted("fn f(a:string)=>number{@acceptanceCriteria.add({behavior=`b`,});}"),
            "fn f(a: string) => number {\n  @acceptanceCriteria.add({ behavior = `b` });\n}\n"
        );
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn test_a_tree_with_errors_is_the_source_unchanged() {
        let tree = tree("const = 1;");
        assert!(!tree.errors.is_empty());
        assert_eq!(format(&tree), "const = 1;");
        assert_eq!(formatted("fn f( {\n  x"), "fn f( {\n  x");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn test_a_generic_signature() {
        assert_eq!(
            formatted("fn map < T,U >(list:List< T >,transform:( item:T )=>U)=>List<U>;"),
            "fn map<T, U>(list: List<T>, transform: (item: T) => U) => List<U>;\n"
        );
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn the_definitions_keep_their_tokens_and_format_to_a_fixed_point() {
        let files = def_files();
        assert!(files.len() > 30, "{}", files.len());
        for (path, source) in files {
            let first = tree(&source);
            assert!(
                first.errors.is_empty(),
                "{path}: {}",
                first
                    .errors
                    .iter()
                    .map(|error| format!("{}:{}", error.start, error.end))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let once = format(&first);
            let reparsed = tree(&once);
            assert!(
                reparsed.errors.is_empty(),
                "{path}: formatting introduced errors:\n{once}"
            );
            assert_eq!(
                tokens_of(&reparsed),
                tokens_of(&first),
                "{path}: tokens changed"
            );
            // @lfy def/format/main.lfy:format
            let twice = format(&reparsed);
            assert!(
                twice == once,
                "{path}: not a fixed point:\n--- once\n{once}\n--- twice\n{twice}"
            );
            // @lfy def/format/main.lfy:format
            assert!(once.ends_with('\n') && !once.ends_with("\n\n"), "{path}");
            assert!(!once.contains('\r'), "{path}");
            assert!(!once.contains("\n\n\n"), "{path}");
        }
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn body_comment_and_documentation_text_is_kept_exactly() {
        let source = "/** Doc with [[ref]]\n   over lines **/\nconst x = `a\n  b {{ y }}  c`;  // trailing  \n/// line doc [[x]]\nconst z = /* in\nline */ 'it\\'s';\n";
        let out = formatted(source);
        assert!(out.contains("/** Doc with [[ref]]\n   over lines **/"));
        assert!(out.contains("`a\n  b {{y}}  c`"));
        assert!(out.contains("// trailing  \n"));
        assert!(out.contains("/// line doc [[x]]\n"));
        assert!(out.contains("/* in\nline */"));
        assert!(out.contains("'it\\'s'"));
        assert_eq!(formatted("const n = 1_000.5;"), "const n = 1_000.5;\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn every_line_ends_with_one_line_feed_and_so_does_the_result() {
        assert_eq!(formatted("a;\r\nb;"), "a;\nb;\n");
        assert_eq!(formatted("a;\n\n\n"), "a;\n");
        assert_eq!(formatted(""), "");
        assert_eq!(formatted("  \n\n"), "");
        assert_eq!(formatted("// only a comment"), "// only a comment\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn each_statement_begins_on_its_own_line_indented_per_enclosing_block() {
        assert_eq!(
            formatted("fn f() { a; b; if (a) { c; } else { e; } }"),
            "fn f() {\n  a;\n  b;\n  if (a) {\n    c;\n  } else {\n    e;\n  }\n}\n"
        );
        assert_eq!(
            formatted("const o = {\n  a = 1,\n  b = { c = [\n1 ] },\n};"),
            "const o = {\n  a = 1,\n  b = { c = [\n    1,\n  ] },\n};\n"
        );
        assert_eq!(
            formatted("type T { a = string, }"),
            "type T { a = string }\n"
        );
        assert_eq!(
            formatted("match x { 'a' -> 1,\n default -> 2 }"),
            "match x {\n  'a' -> 1,\n  default -> 2,\n}\n"
        );
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn blank_lines_between_statements_become_one_and_inside_a_statement_are_removed() {
        assert_eq!(
            formatted("\n\na;\n\n\n\nb;\n{\n\n  c;\n\n\n  e;\n\n}"),
            "a;\n\nb;\n{\n  c;\n\n  e;\n}\n"
        );
        assert_eq!(
            formatted("const o = {\n\n  a = 1,\n\n\n  b = 2,\n\n};"),
            "const o = {\n  a = 1,\n  b = 2,\n};\n"
        );
        assert_eq!(formatted("x\n\n  .a()\n\n  .b();"), "x\n  .a()\n  .b();\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn documentation_and_own_line_comments_keep_their_line_and_trailing_comments_their_end() {
        assert_eq!(
            formatted("// top\n\n/// doc\nfn f() {\n// own\na; // end\n  /* block */ b;\n}"),
            "// top\n\n/// doc\nfn f() {\n  // own\n  a; // end\n  /* block */\n  b;\n}\n"
        );
        assert_eq!(
            formatted("const o = {\n  // key\n  a = 1, // one\n  b = 2 };"),
            "const o = {\n  // key\n  a = 1, // one\n  b = 2,\n};\n"
        );
        assert_eq!(
            formatted("fn f() { // open\n  a;\n}"),
            "fn f() { // open\n  a;\n}\n"
        );
        assert_eq!(formatted("x\n  // why\n  .a();"), "x\n  // why\n  .a();\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn trailing_spaces_are_removed_and_every_line_break_is_a_line_feed() {
        let out = formatted("const x = 1;   \r\n\t \r\nconst y = [\r\n  1,   \r\n];   ");
        assert_eq!(out, "const x = 1;\n\nconst y = [\n  1,\n];\n");
        assert!(out.lines().all(|line| !line.ends_with(' ')));
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn operators_are_spaced_by_their_category() {
        assert_eq!(formatted("x=a+b*-c;"), "x = a + b * -c;\n");
        assert_eq!(formatted("y=a**!e-~f;"), "y = a ** !e - ~f;\n");
        assert_eq!(formatted("x = (a&&b||c);"), "x = (a && b || c);\n");
        assert_eq!(formatted("x = (c  ??  e);"), "x = (c ?? e);\n");
        assert_eq!(formatted("x = g|e^f;"), "x = g | e ^ f;\n");
        assert_eq!(formatted("x = e & f;"), "x = e & f;\n");
        assert_eq!(formatted("x = a<b==c>=g;"), "x = a < b == c >= g;\n");
        assert_eq!(formatted("x = a ? b : c;"), "x = a ? b : c;\n");
        assert_eq!(
            formatted("x = a.b ( c ) [ 0 ]@type;"),
            "x = a.b(c)[0]@type;\n"
        );
        assert_eq!(formatted("x = ( a ) => b;"), "x = (a) => b;\n");
        assert_eq!(
            formatted("where ( a ) and ! ( b ) -> c;"),
            "where (a) and !(b) -> c;\n"
        );
        assert_eq!(
            formatted("x = a is t, u as string;"),
            "x = a is t, u as string;\n"
        );
        assert_eq!(formatted("x = &a  ...b;"), "x = &a ... b;\n");
        assert_eq!(
            formatted("x = [...a, await b, in c];"),
            "x = [...a, await b, in c];\n"
        );
        assert_eq!(formatted("x = ^^;"), "x = ^^;\n");
        assert_eq!(formatted("$a: `d` = b@type;"), "$a: `d` = b@type;\n");
        assert_eq!(
            formatted("if (!(. is b) && (c is e)) {}"),
            "if (!(. is b) && (c is e)) {}\n"
        );
        assert_eq!(formatted("f(. , @ , $ , $&);"), "f(., @, $, $&);\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn commas_and_colons_take_one_space_after_and_brackets_none_inside() {
        assert_eq!(formatted("f( a , b );"), "f(a, b);\n");
        assert_eq!(formatted("x = [ 1 , 2 ];"), "x = [1, 2];\n");
        assert_eq!(formatted("x = [ [ 1 ] ];"), "x = [[1]];\n");
        assert_eq!(
            formatted("x = `{{ a }} [[ b.c ]]`;"),
            "x = `{{a}} [[b.c]]`;\n"
        );
        assert_eq!(
            formatted("fn f(a : string, b ? : number = 1, ... c : (is t)[]) ;"),
            "fn f(a: string, b?: number = 1, ...c: (is t)[]);\n"
        );
        assert_eq!(formatted("x = a : `d`;"), "x = a: `d`;\n");
        assert_eq!(
            formatted("d X is a . b ( 1 ) , c : `d` ;"),
            "d X is a.b(1), c: `d`;\n"
        );
        assert_eq!(
            formatted("type T { a ? : `d` = string | number [] }"),
            "type T { a?: `d` = string | number[] }\n"
        );
        assert_eq!(
            formatted("for ( const k , v from a , b ) { }"),
            "for (const k, v from a, b) {}\n"
        );
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn a_keyword_and_a_group_close_before_a_block_open_take_one_space() {
        assert_eq!(formatted("if(a){}else{}"), "if (a) {} else {}\n");
        assert_eq!(formatted("while(a){b;}"), "while (a) {\n  b;\n}\n");
        assert_eq!(formatted("use\"./a\"as A;"), "use \"./a\" as A;\n");
        assert_eq!(
            formatted("ace function f()->string{return`x`;}"),
            "ace function f() -> string {\n  return `x`;\n}\n"
        );
        assert_eq!(
            formatted("trait t(a:string)extends u(a):`d`{}"),
            "trait t(a: string) extends u(a): `d` {}\n"
        );
        assert_eq!(formatted("with a.b{}"), "with a.b {}\n");
        assert_eq!(formatted("enum E:`d`{a=1}"), "enum E: `d` { a = 1 }\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn the_angles_of_type_parameters_and_type_arguments_are_brackets() {
        assert_eq!(formatted("fn f < T >(a: T);"), "fn f<T>(a: T);\n");
        assert_eq!(
            formatted("d Box < T extends thing = string > {}"),
            "d Box<T extends thing = string> {}\n"
        );
        assert_eq!(
            formatted("const x : List < List < T > > = y;"),
            "const x: List<List<T>> = y;\n"
        );
        assert_eq!(formatted("x = f < T >(a);"), "x = f<T>(a);\n");
        assert_eq!(formatted("x = a.f < T > (a);"), "x = a.f<T>(a);\n");
        assert_eq!(formatted("x = T [];"), "x = T[];\n");
        // An ordering operator keeps the spaces of an infix operator.
        assert_eq!(formatted("x = a<b;"), "x = a < b;\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn a_function_type_is_its_parameters_then_the_arrow_and_the_type() {
        assert_eq!(
            formatted("const y : ( a:string )=>number = f;"),
            "const y: (a: string) => number = f;\n"
        );
        assert_eq!(
            formatted("type T { a = ( )=>number }"),
            "type T { a = () => number }\n"
        );
        assert_eq!(
            formatted("fn f(g: (a: T, ...b: U) => (c: T) => U);"),
            "fn f(g: (a: T, ...b: U) => (c: T) => U);\n"
        );
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn a_block_opens_on_its_line_and_closes_alone_or_is_two_braces() {
        assert_eq!(formatted("loop\n{\n}"), "loop {}\n");
        assert_eq!(formatted("loop { break; }"), "loop {\n  break;\n}\n");
        assert_eq!(
            formatted("x = (a) =>\n{\n  b;\n};"),
            "x = (a) => {\n  b;\n};\n"
        );
        assert_eq!(
            formatted("f((a) => { b; }, 1);"),
            "f(\n  (a) => {\n    b;\n  },\n  1,\n);\n"
        );
        assert_eq!(formatted("f((a) => { b; });"), "f((a) => {\n  b;\n});\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn a_list_with_a_line_break_or_too_long_becomes_one_item_per_line() {
        assert_eq!(formatted("x = [\n1, 2];"), "x = [\n  1,\n  2,\n];\n");
        assert_eq!(
            formatted("x = {a = 1, b = 2\n};"),
            "x = {\n  a = 1,\n  b = 2,\n};\n"
        );
        assert_eq!(formatted("f(\n  a);"), "f(\n  a,\n);\n");
        assert_eq!(formatted("fn f(a,\n b);"), "fn f(\n  a,\n  b,\n);\n");
        assert_eq!(
            formatted("x = (a,\n b) => c;"),
            "x = (\n  a,\n  b,\n) => c;\n"
        );
        assert_eq!(formatted("d X is a(\n1) {}"), "d X is a(\n  1,\n) {}\n");
        let long = "x".repeat(60);
        let source = format!("const v = [`{long}`, `{long}`];");
        assert_eq!(
            formatted(&source),
            format!("const v = [\n  `{long}`,\n  `{long}`,\n];\n")
        );
        let source = format!("f({{ a = `{long}`, b = `{long}` }});");
        assert_eq!(
            formatted(&source),
            format!("f({{\n  a = `{long}`,\n  b = `{long}`,\n}});\n")
        );
        // Empty brackets stay together whatever sat between them.
        assert_eq!(
            formatted("f(\n);\nx = [\n];\ny = {\n};"),
            "f();\nx = [];\ny = {};\n"
        );
        assert_eq!(formatted("x = { // c\n};"), "x = { // c\n};\n");
        assert_eq!(formatted("x = {\n  // c\n};"), "x = {\n  // c\n};\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn a_list_on_one_line_loses_its_trailing_comma() {
        assert_eq!(formatted("x = [1, 2,];"), "x = [1, 2];\n");
        assert_eq!(formatted("x = { a = 1, };"), "x = { a = 1 };\n");
        assert_eq!(formatted("f(a, b,);"), "f(a, b);\n");
        assert_eq!(formatted("fn f(a, b,);"), "fn f(a, b);\n");
        assert_eq!(
            formatted("type T { a = string, }"),
            "type T { a = string }\n"
        );
        assert_eq!(formatted("match x { a -> 1, }"), "match x { a -> 1 }\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn type_parameters_and_arguments_go_one_per_line_only_when_the_line_is_too_long() {
        // A line break in the source does not wrap them.
        assert_eq!(formatted("fn f<\n  T,\n  U\n>(a: T);"), "fn f<T, U>(a: T);\n");
        assert_eq!(formatted("const x: List<\n  T\n> = y;"), "const x: List<T> = y;\n");
        assert_eq!(formatted("x = f<\n  T,\n>(a);"), "x = f<T>(a);\n");
        let long = "T".repeat(60);
        assert_eq!(
            formatted(&format!("fn f<{long}, {long}2>(a: T);")),
            format!("fn f<\n  {long},\n  {long}2,\n>(a: T);\n")
        );
        assert_eq!(
            formatted(&format!("const x: List<{long}, {long}2> = y;")),
            format!("const x: List<\n  {long},\n  {long}2,\n> = y;\n")
        );
        assert_eq!(
            formatted(&format!("x = f<{long}, {long}2>(a);")),
            format!("x = f<\n  {long},\n  {long}2,\n>(a);\n")
        );
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn a_chain_with_a_line_break_before_an_accessor_puts_each_member_on_its_own_line() {
        assert_eq!(formatted("x.a().b();"), "x.a().b();\n");
        assert_eq!(formatted("x.a()\n.b();"), "x\n  .a()\n  .b();\n");
        assert_eq!(
            formatted(
                "fn f() {\n  @acceptanceCriteria\n    .add({ behavior = `a`, })\n    .add({\n      behavior = [\n        `b`,\n      ],\n    });\n}"
            ),
            "fn f() {\n  @acceptanceCriteria\n    .add({ behavior = `a` })\n    .add({\n      behavior = [\n        `b`,\n      ],\n    });\n}\n"
        );
        assert_eq!(
            formatted("global@acceptanceCriteria\n  .add({ a = 1 });"),
            "global@acceptanceCriteria\n  .add({ a = 1 });\n"
        );
        assert_eq!(
            formatted("x = [a\n  .b().c[0]];"),
            "x = [a\n  .b()\n  .c[0]];\n"
        );
        assert_eq!(formatted("x = a.b\n  ?.c;"), "x = a\n  .b\n  ?.c;\n");
    }

    // @lfy def/format/main.lfy:format
    #[test]
    fn a_tree_parsed_for_another_rule_is_laid_out_too() {
        let tokens = lex("a+b", None).unwrap();
        let tree = parse(tokens, Some(Entity::Expression(Expression::Expression)));
        assert!(tree.errors.is_empty());
        assert_eq!(format(&tree), "a + b\n");
    }
}
