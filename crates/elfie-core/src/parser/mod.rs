//! Compiled from `def/parser/main.lfy`.
//!
//! [`parse`] turns a token list into a lossless tree for a rule, the whole file by
//! default. The grammar document is the only grammar the parser knows: it reads each
//! rule's syntax, category and binding and sets none of them. A terminal is satisfied by
//! one token; any other rule by a node whose children are what its elements produced, in
//! order, with trivia between elements taken as children of the open node. Statements and
//! expressions are selected by the next token that is not trivia and tried in
//! `triedBefore` order; expressions climb by binding power; recoverable rules close with
//! error nodes instead of failing once they have taken a token.

use std::collections::{HashMap, HashSet};

pub mod beginning;
pub mod components;
pub mod data;
#[cfg(test)]
mod tests;
pub mod traits;

pub use beginning::{Violation, validate};
pub use data::{Child, ErrorNode, Node, Tree};

use crate::grammar::ebnf::{self, Expr};
use crate::grammar::rules::expression::{self as expression, Expression, PREFIXES, PRIMARIES};
use crate::grammar::rules::file::File;
use crate::grammar::rules::statement::{STATEMENTS, Statement};
use crate::grammar::terminals::comment::Comment;
use crate::grammar::terminals::punctuation::Punctuation;
use crate::grammar::terminals::space::Space;
use crate::grammar::{Associativity, Category, Entity, GrammarRule};
use crate::lexer::Token;

use beginning::{Tables, is_expression_rule, is_operation, tables};

/// The grammar document is the only grammar the parser knows: the EBNF of every rule.
// @lfy def/parser/main.lfy:parse
pub fn grammar() -> String {
    crate::grammar::grammar_document()
}

/// Turn a token list into a lossless tree for a rule, the whole file by default.
///
/// [`Tree::tokens`] is `tokens` unchanged, [`Tree::root`] is a node for the rule that
/// covers every token exactly once, and [`Tree::errors`] lists every error node. `rule`
/// is what the caller wants satisfied: `None` for [`File::SourceFile`], a file;
/// [`Statement::Statement`] to reparse one statement; [`Expression::Expression`] for a
/// snippet. Tokens left over after the rule is satisfied become one error node at the end
/// of the root.
// @lfy def/parser/main.lfy:parse
pub fn parse(tokens: Vec<Token>, rule: Option<Entity>) -> Tree {
    parse_counting(tokens, rule).0
}

/// [`parse`], also returning how often each rule was attempted per token index and
/// minimum.
pub(crate) fn parse_counting(tokens: Vec<Token>, rule: Option<Entity>) -> (Tree, HashMap<Key, usize>) {
    let root_rule = rule.unwrap_or(Entity::File(File::SourceFile)); // @lfy def/parser/main.lfy:parse
    let mut parser = Parser::new(&tokens);
    let root = parser.parse_root(root_rule);
    let attempts = std::mem::take(&mut parser.attempts);
    drop(parser);
    // @lfy def/parser/data.lfy:28
    let mut errors: Vec<ErrorNode> = root.errors().into_iter().cloned().collect();
    errors.sort_by_key(|error| error.start);
    let tree = Tree {
        tokens,    // @lfy def/parser/main.lfy:parse
        root_rule, // @lfy def/parser/data.lfy:26
        root,      // @lfy def/parser/main.lfy:parse
        errors,    // @lfy def/parser/main.lfy:parse
    };
    (tree, attempts)
}

/// A rule attempt: the rule, the token index, the minimum binding power in force, and
/// whether a line break counted as trivia.
pub(crate) type Key = (Entity, usize, u8, bool);

/// What a rule produced when it was satisfied: its child and the index after it.
#[derive(Debug, Clone)]
struct Parsed {
    child: Child,
    end: usize,
}

/// The state of one rule's sequence of elements while it is being parsed.
struct Seq {
    /// The sync terminals when the rule is `recoverable`.
    sync: Option<&'static [Entity]>,
    /// Whether trivia may sit between the rule's elements.
    admits_trivia: bool,
    /// Whether the rule has taken a token.
    taken: bool,
    /// Whether the remaining elements are tried as if optional, after an error node.
    lenient: bool,
    /// The binding power of the operation whose tail this is.
    tail_power: Option<u8>,
    /// The terminal of the last token taken, which decides an expression's minimum.
    previous: Option<Entity>,
    /// How many groups deep the elements being parsed are; 0 for the rule's own elements.
    depth: usize,
    /// What could continue the open rule at a token index, recorded by an optional,
    /// repetition or alternation for the probe that takes the tokens there.
    pending: Option<(usize, Vec<&'static str>)>,
}

impl Seq {
    fn new(rule: Entity) -> Seq {
        Seq {
            sync: components::sync(rule),
            admits_trivia: traits::admits_trivia(rule),
            taken: false,
            lenient: false,
            tail_power: None,
            previous: None,
            depth: 0,
            pending: None,
        }
    }

    /// Records what could continue at `at` unless something already is: the first
    /// element that can be skipped there already spans everything after it.
    fn record(&mut self, at: usize, expected: impl FnOnce() -> Vec<&'static str>) {
        if self.pending.as_ref().is_none_or(|(index, _)| *index != at) {
            self.pending = Some((at, expected()));
        }
    }

    /// The expectation recorded for `at`, if any; recording is one-shot.
    fn take_pending(&mut self, at: usize) -> Option<Vec<&'static str>> {
        match self.pending.take() {
            Some((index, expected)) if index == at => Some(expected),
            _ => None,
        }
    }

    /// The minimum an `Expression` element is parsed with: the operation's power inside a
    /// tail, and 0 in every other rule or after `GroupOpen` or `ListOpen`.
    // @lfy def/parser/main.lfy:parse
    fn expression_min(&self) -> u8 {
        match self.tail_power {
            Some(power)
                if !matches!(
                    self.previous,
                    Some(Entity::Punctuation(Punctuation::GroupOpen | Punctuation::ListOpen))
                ) =>
            {
                power
            }
            _ => 0,
        }
    }
}

/// The elements of a rule's syntax or of a group: the items of a sequence, or the
/// expression alone.
fn elements_of(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Sequence(items) => items.iter().collect(),
        other => vec![other],
    }
}

struct Parser<'t> {
    tokens: &'t [Token],
    tables: &'static Tables,
    memo: HashMap<Key, Option<Parsed>>,
    attempts: HashMap<Key, usize>,
    /// Whether a `NewLine` token is trivia: not inside a line documentation's reference.
    newline_is_trivia: bool,
}

impl<'t> Parser<'t> {
    fn new(tokens: &'t [Token]) -> Parser<'t> {
        Parser {
            tokens,
            tables: tables(),
            memo: HashMap::new(),
            attempts: HashMap::new(),
            newline_is_trivia: true,
        }
    }

    fn rule_at(&self, at: usize) -> Option<Entity> {
        self.tokens.get(at).and_then(|token| token.rule)
    }

    /// The identifiers of the terminals that could begin `expr`.
    fn expected_for_expr(&self, expr: &Expr) -> Vec<&'static str> {
        self.tables.identifiers(&self.tables.first_of(expr))
    }

    /// What could continue the open rule where `element` is about to be tried: the
    /// terminals that begin it and, when it can be satisfied without a token, those that
    /// begin what follows it.
    // @lfy def/parser/data.lfy:21
    fn expected_at(&self, element: &Expr, follow: &HashSet<Entity>) -> Vec<&'static str> {
        let mut set = self.tables.first_of(element);
        if ebnf::grammar().is_expr_nullable(element) {
            set.extend(follow.iter().copied());
        }
        self.tables.identifiers(&set)
    }

    // Trivia

    /// Reads past the tokens that never begin a rule from `at`: trivia when `admit` says
    /// the open rule takes it as children, and tokens without a rule, each of which
    /// becomes an error node of one token expecting what the open rule expected. Yields
    /// the children they become and the index of the next token.
    // @lfy def/parser/main.lfy:parse
    fn skip(&mut self, at: usize, admit: bool, expected: impl Fn() -> Vec<&'static str>) -> (Vec<Child>, usize) {
        self.skip_with(at, admit, true, expected)
    }

    /// [`Parser::skip`], taking tokens without a rule as one-token error nodes only when
    /// `invalid` says so.
    fn skip_with(
        &mut self,
        mut at: usize,
        admit: bool,
        invalid: bool,
        expected: impl Fn() -> Vec<&'static str>,
    ) -> (Vec<Child>, usize) {
        let mut children = Vec::new();
        while at < self.tokens.len() {
            match self.tokens[at].rule {
                None if !invalid => break,
                // @lfy def/parser/main.lfy:parse
                None => {
                    children.push(Child::Error(ErrorNode::new(at, at + 1, expected())));
                    at += 1;
                }
                Some(_) if !admit => break,
                // @lfy def/grammar/terminals/comment.lfy:Documentation
                Some(Entity::Space(Space::NewLine)) if !self.newline_is_trivia => break,
                // @lfy def/parser/main.lfy:parse
                Some(rule) if components::is_trivia(rule) => {
                    children.push(Child::Token(at));
                    at += 1;
                }
                Some(rule) => {
                    let trivia_rule = components::TRIVIA
                        .iter()
                        .copied()
                        .find(|&trivia| !trivia.is_terminal() && self.tables.can_begin(trivia, rule));
                    let Some(trivia_rule) = trivia_rule else {
                        break;
                    };
                    let Some(parsed) = self.parse_rule(trivia_rule, at, 0) else {
                        break;
                    };
                    at = parsed.end;
                    children.push(parsed.child);
                }
            }
        }
        (children, at)
    }

    /// [`Parser::skip`] for the open rule of `seq`.
    fn probe(&mut self, seq: &Seq, at: usize, expected: impl Fn() -> Vec<&'static str>) -> (Vec<Child>, usize) {
        self.skip(at, seq.admits_trivia, expected)
    }

    /// The index of the next token that can begin a rule, reading past what
    /// [`Parser::probe`] would take.
    fn peek(&mut self, seq: &Seq, at: usize) -> usize {
        self.probe(seq, at, Vec::new).1
    }

    /// Adds the trivia read past and the child an element produced to the open node.
    fn commit(&mut self, node: &mut Node, seq: &mut Seq, trivia: Vec<Child>, child: Child) {
        node.children.extend(trivia);
        let (start, end) = (child.start(), child.end());
        if end > start {
            seq.taken = true;
            seq.previous = self.tokens[end - 1].rule;
        }
        node.children.push(child);
    }

    // Rules

    /// Tries `rule` at `at` with `min` as the minimum binding power, at most once per
    /// token index and minimum.
    // @lfy def/parser/main.lfy:parse
    fn parse_rule(&mut self, rule: Entity, at: usize, min: u8) -> Option<Parsed> {
        let key = (rule, at, min, self.newline_is_trivia);
        if let Some(hit) = self.memo.get(&key) {
            return hit.clone();
        }
        *self.attempts.entry(key).or_insert(0) += 1;
        let result = match rule.category() {
            // @lfy def/parser/main.lfy:parse
            Category::Terminal(_) => (self.rule_at(at) == Some(rule)).then(|| Parsed {
                child: Child::Token(at),
                end: at + 1,
            }),
            Category::AlternationList(items) => self.parse_list(rule, items, at, min),
            Category::Prefix { .. } => self.parse_prefix(rule, at),
            // @lfy def/parser/main.lfy:parse
            Category::Infix { .. } | Category::Postfix { .. } => None,
            // @lfy def/parser/main.lfy:parse
            Category::Rule | Category::Statement | Category::Primary => self.parse_syntax(rule, at),
        };
        self.memo.insert(key, result.clone());
        result
    }

    /// An alternation list is satisfied by one of its items, which stands in its place.
    // @lfy def/grammar/traits.lfy:alternationList
    fn parse_list(&mut self, rule: Entity, items: &'static [Entity], at: usize, min: u8) -> Option<Parsed> {
        // @lfy def/parser/main.lfy:parse
        if rule == Entity::Expression(Expression::Expression) {
            return self.parse_expression(at, min, None, None);
        }
        // @lfy def/parser/main.lfy:parse
        if items.iter().all(|&item| is_expression_rule(item)) {
            return self.parse_expression_alternation(items, at);
        }
        // @lfy def/parser/main.lfy:parse
        let candidates = if rule == Entity::Statement(Statement::Statement) {
            STATEMENTS
        } else {
            items // @lfy def/parser/main.lfy:parse
        };
        self.try_candidates(candidates, at)
    }

    /// Tries the candidates the next token selects, each only when the one before it
    /// fails there.
    // @lfy def/parser/main.lfy:parse
    fn try_candidates(&mut self, candidates: &[Entity], at: usize) -> Option<Parsed> {
        let terminal = self.rule_at(at)?;
        for candidate in self.tables.select(candidates, terminal) {
            // @lfy def/parser/main.lfy:parse
            if let Some(parsed) = self.parse_rule(candidate, at, 0) {
                return Some(parsed);
            }
        }
        None
    }

    /// A rule with a syntax is satisfied by a node whose children are what its elements
    /// produced, in order.
    // @lfy def/parser/main.lfy:parse
    fn parse_syntax(&mut self, rule: Entity, at: usize) -> Option<Parsed> {
        let expr = rule.expression()?;
        // A NewLine inside a line documentation's TemplateReference is not trivia.
        // @lfy def/grammar/terminals/comment.lfy:Documentation
        let line_documentation = rule == Entity::Comment(Comment::Documentation)
            && self.rule_at(at) == Some(Entity::Comment(Comment::LineDocumentationOpen));
        let saved = self.newline_is_trivia;
        if line_documentation {
            self.newline_is_trivia = false;
        }
        let mut node = Node::open(rule, at);
        let mut seq = Seq::new(rule);
        let elements = elements_of(expr);
        let result = if rule == Entity::Expression(Expression::Current) {
            self.parse_current(&mut node)
        } else {
            self.parse_elements(&elements, &mut node, &mut seq, at, &HashSet::new())
        };
        self.newline_is_trivia = saved;
        let end = result?;
        node.end = end;
        // @lfy def/grammar/rules/expression.lfy:74
        if rule == Entity::Expression(Expression::Group) {
            let (_, next) = self.skip(end, true, Vec::new);
            if self.rule_at(next).is_some_and(expression::group_cannot_match_before) {
                return None;
            }
        }
        traits::attach_documentation(&mut node, self.tokens);
        Some(Parsed {
            child: Child::Node(node),
            end,
        })
    }

    /// `Current`: an accessor and, only when nothing sits between them, a member name.
    /// When space, a line break, comments, or documentation exist between the accessor
    /// and the member name, the member name is not matched.
    // @lfy def/grammar/rules/expression.lfy:70
    fn parse_current(&mut self, node: &mut Node) -> Option<usize> {
        let accessor = self.parse_rule(Entity::Punctuation(Punctuation::Accessor), node.start, 0)?;
        let mut end = accessor.end;
        node.children.push(accessor.child);
        let member_name = Entity::Expression(Expression::MemberName);
        if let Some(terminal) = self.rule_at(end)
            && self.tables.can_begin(member_name, terminal)
            && let Some(name) = self.parse_rule(member_name, end, 0)
        {
            end = name.end;
            node.children.push(name.child);
        }
        Some(end)
    }

    /// Parses `elements` in order into `node`. At the rule's own level a recoverable rule
    /// that has taken a token closes a failed required element with an error node and
    /// tries the remaining elements as if optional; anywhere else a failed element fails
    /// the sequence.
    // @lfy def/parser/traits.lfy:recoverable
    fn parse_elements(
        &mut self,
        elements: &[&Expr],
        node: &mut Node,
        seq: &mut Seq,
        mut pos: usize,
        follow_after: &HashSet<Entity>,
    ) -> Option<usize> {
        for (index, &element) in elements.iter().enumerate() {
            let follow = self.follow_of(&elements[index + 1..], follow_after);
            if let Some(next) = self.parse_element(element, node, seq, pos, &follow) {
                pos = next;
                continue;
            }
            if seq.lenient {
                continue;
            }
            // @lfy def/parser/main.lfy:parse
            if seq.depth == 0
                && seq.taken
                && let Some(sync) = seq.sync
            {
                let expected = self.expected_for_expr(element);
                let (trivia, start) = self.probe(seq, pos, || expected.clone());
                node.children.extend(trivia);
                let end = traits::sweep_end(self.tokens, start, sync);
                node.children.push(Child::Error(ErrorNode::new(start, end, expected)));
                pos = end;
                seq.lenient = true;
                if let Some(next) = self.parse_element(element, node, seq, pos, &follow) {
                    pos = next;
                }
                continue;
            }
            // @lfy def/parser/main.lfy:parse
            return None;
        }
        Some(pos)
    }

    /// The terminals that can begin what comes after the current element: the rest of its
    /// sequence, and past it what follows the group.
    fn follow_of(&self, rest: &[&Expr], follow_after: &HashSet<Entity>) -> HashSet<Entity> {
        let grammar = ebnf::grammar();
        let mut out = HashSet::new();
        for &element in rest {
            out.extend(self.tables.first_of(element));
            if !grammar.is_expr_nullable(element) {
                return out;
            }
        }
        out.extend(follow_after.iter().copied());
        out
    }

    fn parse_element(
        &mut self,
        element: &Expr,
        node: &mut Node,
        seq: &mut Seq,
        pos: usize,
        follow: &HashSet<Entity>,
    ) -> Option<usize> {
        match element {
            Expr::Reference(name) => {
                let rule = Entity::lookup(name)?;
                let expected = seq
                    .take_pending(pos)
                    .unwrap_or_else(|| self.expected_at(element, follow));
                let (trivia, at) = self.probe(seq, pos, || expected.clone());
                let min = if rule == Entity::Expression(Expression::Expression) {
                    seq.expression_min()
                } else {
                    0
                };
                let parsed = self.parse_rule(rule, at, min)?;
                self.commit(node, seq, trivia, parsed.child);
                Some(parsed.end)
            }
            // @lfy def/parser/main.lfy:parse
            Expr::Optional(inner) => {
                let at = self.peek(seq, pos);
                // Whether the element is tried or skipped, what could continue here is
                // the element or what follows it.
                seq.record(pos, || self.expected_at(element, follow));
                if let Some(terminal) = self.rule_at(at)
                    && self.tables.expr_can_begin(inner, terminal)
                    && let Some(end) = self.parse_group(inner, node, seq, pos, follow)
                {
                    return Some(end);
                }
                Some(pos)
            }
            Expr::Repetition(inner) => self.parse_repetition(inner, node, seq, pos, follow),
            Expr::Alternation(alternatives) => self.parse_alternation(alternatives, node, seq, pos, follow),
            Expr::Sequence(_) => self.parse_group(element, node, seq, pos, follow),
            Expr::Terminal(_) | Expr::Special(_) | Expr::Exception(..) => {
                unreachable!("parser-level rules use token-level constructs only")
            }
        }
    }

    /// Parses a group as a unit: what it took is given back when it fails.
    fn parse_group(
        &mut self,
        inner: &Expr,
        node: &mut Node,
        seq: &mut Seq,
        pos: usize,
        follow: &HashSet<Entity>,
    ) -> Option<usize> {
        let saved_len = node.children.len();
        let saved = (seq.taken, seq.previous, seq.lenient, seq.pending.clone());
        seq.depth += 1;
        seq.lenient = false;
        let elements = elements_of(inner);
        let result = self.parse_elements(&elements, node, seq, pos, follow);
        seq.depth -= 1;
        seq.lenient = saved.2;
        if result.is_none() {
            node.children.truncate(saved_len);
            seq.taken = saved.0;
            seq.previous = saved.1;
            seq.pending = saved.3;
        }
        result
    }

    /// Another iteration is tried while the next token can begin the repeated element; an
    /// iteration that fails leaves its tokens and ends the repetition. A recoverable rule
    /// then decides how the repetition stops.
    // @lfy def/parser/main.lfy:parse
    fn parse_repetition(
        &mut self,
        inner: &Expr,
        node: &mut Node,
        seq: &mut Seq,
        mut pos: usize,
        follow: &HashSet<Entity>,
    ) -> Option<usize> {
        loop {
            let at = self.peek(seq, pos);
            if let Some(terminal) = self.rule_at(at)
                && self.tables.expr_can_begin(inner, terminal)
            {
                seq.record(pos, || {
                    let mut all = self.tables.first_of(inner);
                    all.extend(follow.iter().copied());
                    self.tables.identifiers(&all)
                });
                if let Some(end) = self.parse_group(inner, node, seq, pos, follow)
                    && end > pos
                {
                    pos = end;
                    continue;
                }
            }
            // @lfy def/parser/traits.lfy:recoverable
            let Some(sync) = seq.sync else {
                return Some(pos);
            };
            let inner_first = self.tables.first_of(inner);
            let expected = {
                let mut all = inner_first.clone();
                all.extend(follow.iter().copied());
                self.tables.identifiers(&all)
            };
            let stop = traits::repetition_stop(
                self.tokens,
                (at < self.tokens.len()).then_some(at),
                sync,
                |rule| follow.contains(&rule),
                |rule| inner_first.contains(&rule),
            );
            let end = match stop {
                traits::RepetitionStop::Ends => {
                    seq.record(pos, || expected.clone());
                    return Some(pos);
                }
                traits::RepetitionStop::ErrorAlone => at + 1,
                traits::RepetitionStop::ErrorUpTo(end) => end,
            };
            let (trivia, start) = self.probe(seq, pos, || expected.clone());
            node.children.extend(trivia);
            debug_assert_eq!(start, at);
            node.children.push(Child::Error(ErrorNode::new(start, end, expected)));
            seq.taken = true;
            pos = end;
        }
    }

    /// An alternation whose every alternative is an expression rule is satisfied by one
    /// expression; any other alternation by the alternative the next token selects.
    // @lfy def/parser/main.lfy:parse
    fn parse_alternation(
        &mut self,
        alternatives: &[Expr],
        node: &mut Node,
        seq: &mut Seq,
        pos: usize,
        follow: &HashSet<Entity>,
    ) -> Option<usize> {
        let expression_rules: Option<Vec<Entity>> = alternatives
            .iter()
            .map(|alternative| match alternative {
                Expr::Reference(name) => Entity::lookup(name).filter(|rule| is_expression_rule(*rule)),
                _ => None,
            })
            .collect();
        let alternation = Expr::Alternation(alternatives.to_vec());
        seq.record(pos, || self.expected_at(&alternation, follow));
        // @lfy def/parser/main.lfy:parse
        if let Some(items) = expression_rules {
            let expected = seq.take_pending(pos).unwrap_or_default();
            let (trivia, at) = self.probe(seq, pos, || expected.clone());
            let parsed = self.parse_expression_alternation(&items, at)?;
            self.commit(node, seq, trivia, parsed.child);
            return Some(parsed.end);
        }
        // @lfy def/parser/main.lfy:parse
        let at = self.peek(seq, pos);
        let terminal = self.rule_at(at)?;
        let selected: Vec<&Expr> = alternatives
            .iter()
            .filter(|alternative| self.tables.expr_can_begin(alternative, terminal))
            .collect();
        // @lfy def/parser/main.lfy:parse
        let ordered: Vec<&Expr> = if selected.len() > 1 {
            let rules: Vec<Entity> = selected
                .iter()
                .filter_map(|alternative| match alternative {
                    Expr::Reference(name) => Entity::lookup(name),
                    _ => None,
                })
                .collect();
            self.tables
                .select(&rules, terminal)
                .into_iter()
                .filter_map(|rule| {
                    selected.iter().copied().find(|alternative| {
                        matches!(alternative, Expr::Reference(name) if name == rule.identifier())
                    })
                })
                .collect()
        } else {
            selected
        };
        for alternative in ordered {
            // @lfy def/parser/main.lfy:parse
            if let Some(end) = self.parse_element(alternative, node, seq, pos, follow) {
                return Some(end);
            }
        }
        None
    }

    /// The alternation is satisfied by one expression parsed with a minimum one below the
    /// lowest binding power among its infix and postfix alternatives (0 when it has none),
    /// beginning with one of its primary or prefix alternatives, and producing a node for
    /// one of the alternatives.
    // @lfy def/parser/main.lfy:parse
    fn parse_expression_alternation(&mut self, items: &[Entity], at: usize) -> Option<Parsed> {
        // @lfy def/parser/main.lfy:parse
        let min = items
            .iter()
            .filter(|&&item| is_operation(item))
            .filter_map(|item| item.effective_binding())
            .map(|binding| binding.precedence_value())
            .min()
            .map_or(0, |power| power - 1);
        // @lfy def/parser/main.lfy:parse
        let begin_with: Vec<Entity> = items
            .iter()
            .copied()
            .filter(|item| item.is_primary() || item.is_prefix())
            .collect();
        let parsed = self.parse_expression(at, min, Some(&begin_with), Some(items))?;
        // @lfy def/parser/main.lfy:parse
        match &parsed.child {
            Child::Node(node) if items.contains(&node.rule) => Some(parsed),
            _ => None,
        }
    }

    // Expressions

    /// An expression begins with one primary or prefix and continues with any number of
    /// infix and postfix operations, grouped by their binding power.
    // @lfy def/grammar/rules/expression.lfy:142
    fn parse_expression(
        &mut self,
        at: usize,
        min: u8,
        begin_with: Option<&[Entity]>,
        admitted: Option<&[Entity]>,
    ) -> Option<Parsed> {
        let terminal = self.rule_at(at)?;
        let candidates: Vec<Entity> = match begin_with {
            Some(items) => items.to_vec(),
            // @lfy def/parser/main.lfy:parse
            None => PRIMARIES.iter().chain(PREFIXES).copied().collect(),
        };
        let mut left = None;
        // @lfy def/parser/main.lfy:parse
        for candidate in self.tables.select(&candidates, terminal) {
            if let Some(parsed) = self.parse_rule(candidate, at, 0) {
                left = Some(parsed);
                break;
            }
        }
        let mut left = left?;
        let tables = self.tables;
        loop {
            // @lfy def/parser/main.lfy:parse
            let (trivia, next) = self.skip(left.end, true, || tables.operators_continuing(min, admitted));
            let Some(terminal) = self.rule_at(next) else {
                break;
            };
            let Some(operation) = self.tables.operation(terminal) else {
                break;
            };
            let binding = operation.effective_binding().expect("checked by the grammar");
            let power = binding.precedence_value();
            // @lfy def/parser/main.lfy:parse
            let applies = power > min || (power == min && binding.associativity == Some(Associativity::Right));
            if !applies {
                break;
            }
            match self.parse_operation(operation, left, trivia, next, power) {
                Ok(parsed) => left = parsed,
                Err(returned) => {
                    left = returned;
                    break;
                }
            }
        }
        Some(left)
    }

    /// Builds the node of an infix or postfix operation from the left operand, the
    /// operator, and what follows: a right operand parsed with the operation's power as
    /// the minimum, or the postfix tail. Gives the left operand back when the operation
    /// cannot be satisfied.
    // @lfy def/parser/main.lfy:parse
    fn parse_operation(
        &mut self,
        operation: Entity,
        left: Parsed,
        trivia: Vec<Child>,
        at: usize,
        power: u8,
    ) -> Result<Parsed, Parsed> {
        let start = left.child.start();
        let mut rest = Node::open(operation, at);
        rest.children.push(Child::Token(at));
        let end = match operation.category() {
            // @lfy def/parser/main.lfy:parse
            Category::Infix { .. } => {
                let tables = self.tables;
                let (between, operand) =
                    self.skip(at + 1, true, || tables.expected(Entity::Expression(Expression::Expression)));
                match self.parse_rule(Entity::Expression(Expression::Expression), operand, power) {
                    Some(right) => {
                        rest.children.extend(between);
                        rest.children.push(right.child);
                        right.end
                    }
                    None => return Err(left),
                }
            }
            // @lfy def/grammar/traits.lfy:postfix
            Category::Postfix { disallow_space, .. } => match self.tables.tail(operation) {
                Some(tail) => {
                    // @lfy def/grammar/traits.lfy:postfix
                    if disallow_space {
                        let (between, next) = self.skip(at + 1, true, Vec::new);
                        let tail_begins = self
                            .rule_at(next)
                            .is_some_and(|terminal| self.tables.expr_can_begin(tail, terminal));
                        if tail_begins && between.iter().any(|child| !matches!(child, Child::Error(_))) {
                            return Err(left);
                        }
                    }
                    let mut seq = Seq::new(operation);
                    seq.taken = true;
                    seq.tail_power = Some(power); // @lfy def/grammar/traits.lfy:postfix
                    seq.previous = self.rule_at(at);
                    let elements = elements_of(tail);
                    match self.parse_elements(&elements, &mut rest, &mut seq, at + 1, &HashSet::new()) {
                        Some(end) => end,
                        None => return Err(left),
                    }
                }
                None => at + 1,
            },
            _ => unreachable!("only operations continue an expression"),
        };
        let mut node = Node::open(operation, start);
        node.children.push(left.child);
        node.children.extend(trivia);
        node.children.append(&mut rest.children);
        node.end = end;
        traits::attach_documentation(&mut node, self.tokens);
        Ok(Parsed {
            child: Child::Node(node),
            end,
        })
    }

    /// A prefix operation: the operator followed by an operand parsed with the operation's
    /// power as the minimum.
    // @lfy def/grammar/traits.lfy:prefix
    fn parse_prefix(&mut self, rule: Entity, at: usize) -> Option<Parsed> {
        let Category::Prefix { disallow_space, .. } = rule.category() else {
            unreachable!()
        };
        if !self.tables.can_begin(rule, self.rule_at(at)?) {
            return None;
        }
        let power = rule.effective_binding()?.precedence_value();
        let tables = self.tables;
        let (trivia, operand) =
            self.skip(at + 1, true, || tables.expected(Entity::Expression(Expression::Expression)));
        // @lfy def/grammar/traits.lfy:prefix
        // Space, a line break, comments, or documentation between the operator and its
        // operand; a token without a rule is none of those.
        if disallow_space && trivia.iter().any(|child| !matches!(child, Child::Error(_))) {
            return None;
        }
        // @lfy def/parser/main.lfy:parse
        let operand = self.parse_rule(Entity::Expression(Expression::Expression), operand, power)?;
        let mut node = Node::open(rule, at);
        node.children.push(Child::Token(at));
        node.children.extend(trivia);
        node.children.push(operand.child);
        node.end = operand.end;
        Some(Parsed {
            child: Child::Node(node),
            end: operand.end,
        })
    }

    // The root

    /// What could have continued the root rule once it is satisfied: an operator after an
    /// expression, the repeated element of a rule whose syntax ends in a repetition, and
    /// nothing otherwise.
    // @lfy def/parser/data.lfy:21
    fn root_continuations(&self, rule: Entity) -> Vec<&'static str> {
        if rule == Entity::Expression(Expression::Expression) || is_expression_rule(rule) {
            return self.tables.operators_continuing(0, None);
        }
        let Some(expr) = rule.expression() else {
            return Vec::new();
        };
        let grammar = ebnf::grammar();
        let mut set = HashSet::new();
        for element in elements_of(expr).into_iter().rev() {
            if let Expr::Repetition(inner) = element {
                set.extend(self.tables.first_of(inner));
            }
            if !grammar.is_expr_nullable(element) {
                break;
            }
        }
        self.tables.identifiers(&set)
    }

    /// A node for `rule` covering every token exactly once: the node the rule produced,
    /// with anything left over as trailing trivia and one error node at its end. When the
    /// rule is an alternation list and tokens are left over or trivia precedes the item,
    /// the root is a node for the rule holding the trivia, the item, and the error node.
    // @lfy def/parser/main.lfy:parse
    fn parse_root(&mut self, rule: Entity) -> Node {
        let len = self.tokens.len();
        let tables = self.tables;
        let (leading, at) = self.skip(0, true, || tables.expected(rule));
        let parsed = self.parse_rule(rule, at, 0);
        let mut root = match parsed {
            // The node the rule produced is the root; trivia before it is its own.
            Some(Parsed {
                child: Child::Node(mut node),
                ..
            }) if !rule.is_alternation_list() => {
                node.start = 0;
                node.children.splice(0..0, leading);
                node
            }
            // @lfy def/parser/main.lfy:parse
            Some(Parsed {
                child: Child::Node(node),
                ..
            }) if leading.is_empty() && node.end == len => node,
            parsed => {
                let mut root = Node::open(rule, 0);
                root.children.extend(leading);
                match parsed {
                    Some(parsed) => {
                        root.end = parsed.end;
                        root.children.push(parsed.child);
                    }
                    None if at < len => {
                        // @lfy def/parser/main.lfy:parse
                        let expected = self.tables.expected(rule);
                        root.children.push(Child::Error(ErrorNode::new(at, len, expected)));
                        root.end = len;
                    }
                    None => root.end = at,
                }
                root
            }
        };
        // @lfy def/parser/main.lfy:parse
        let (trailing, rest) = self.skip_with(root.end, true, false, Vec::new);
        root.children.extend(trailing);
        if rest < len {
            let expected = self.root_continuations(rule);
            root.children.push(Child::Error(ErrorNode::new(rest, len, expected)));
        }
        root.end = len;
        traits::attach_documentation(&mut root, self.tokens);
        root
    }
}
