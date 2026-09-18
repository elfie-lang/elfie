//! Compiled from `def/parser/traits.lfy`.
//!
//! The parser traits are applied to rules in [`super::components`]; this module holds the
//! constants and the mechanics each trait describes, which the parser in [`super`] calls.

use crate::grammar::terminals::literal::Literal;
use crate::grammar::terminals::punctuation::Punctuation;
use crate::grammar::{Entity, GrammarRule};
use crate::lexer::Token;

use super::components;
use super::data::{Child, Node};

/// `ace const brackets`: token pairs that nest, with the first token opening and the second
/// closing.
// @lfy def/parser/traits.lfy:8
pub const BRACKETS: &[(Entity, Entity)] = &[
    (Entity::Punctuation(Punctuation::GroupOpen), Entity::Punctuation(Punctuation::GroupClose)), // @lfy def/parser/traits.lfy:9
    (Entity::Punctuation(Punctuation::ListOpen), Entity::Punctuation(Punctuation::ListClose)), // @lfy def/parser/traits.lfy:9
    (Entity::Punctuation(Punctuation::BlockOpen), Entity::Punctuation(Punctuation::BlockClose)), // @lfy def/parser/traits.lfy:9
    (Entity::Literal(Literal::ExecutionOpen), Entity::Literal(Literal::ExecutionClose)), // @lfy def/parser/traits.lfy:10
    (Entity::Literal(Literal::ReferenceOpen), Entity::Literal(Literal::ReferenceClose)), // @lfy def/parser/traits.lfy:10
];

/// Whether a token of this terminal opens a bracket.
pub fn opens_bracket(rule: Entity) -> bool {
    BRACKETS.iter().any(|&(open, _)| open == rule)
}

/// Whether a token of this terminal closes a bracket.
pub fn closes_bracket(rule: Entity) -> bool {
    BRACKETS.iter().any(|&(_, close)| close == rule)
}

/// The token index where an error node that begins at `start` ends, and the tokens it
/// covers stop: the next token of `sync` at the same bracket depth, a closing token of
/// [`BRACKETS`] that would take the depth below 0, or the end of the input. Bracket depth
/// is 0 at `start`; each opening token adds one and each closing token removes one, and a
/// token is at the same depth when the depth before it is 0.
// @lfy def/parser/traits.lfy:13
pub fn sweep_end(tokens: &[Token], start: usize, sync: &[Entity]) -> usize {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        let Some(rule) = token.rule else {
            continue;
        };
        // @lfy def/parser/traits.lfy:16
        if depth == 0 && sync.contains(&rule) {
            return index;
        }
        if closes_bracket(rule) {
            // @lfy def/parser/traits.lfy:40
            if depth == 0 {
                return index;
            }
            depth -= 1; // @lfy def/parser/traits.lfy:15
        } else if opens_bracket(rule) {
            depth += 1; // @lfy def/parser/traits.lfy:15
        }
    }
    tokens.len() // @lfy def/parser/traits.lfy:39
}

/// `ace function listed(rules)`: `[[A]], [[B]], …`
// @lfy def/parser/traits.lfy:19
pub fn listed(rules: &[Entity]) -> String {
    rules
        .iter()
        .map(|rule| format!("[[{}]]", rule.identifier()))
        .collect::<Vec<_>>()
        .join(", ")
}

// trivia

/// `trait trivia`: whether tokens of a trivia rule may sit between two elements of `rule`
/// and become children of its node, which they may unless the rule references a `body`
/// terminal.
// @lfy def/parser/traits.lfy:23
pub fn admits_trivia(rule: Entity) -> bool {
    !references_body_terminal(rule)
}

/// Whether the rule's own syntax references a `body` terminal.
// @lfy def/parser/traits.lfy:25
pub fn references_body_terminal(rule: Entity) -> bool {
    rule.expression().is_some_and(|expr| {
        expr.references()
            .into_iter()
            .filter_map(Entity::lookup)
            .any(|reference| reference.is_body())
    })
}

/// Whether a child of a node is trivia: a token of a trivia terminal, or a node of a
/// trivia rule.
pub fn is_trivia_child(child: &Child, tokens: &[Token]) -> bool {
    match child {
        Child::Node(node) => components::is_trivia(node.rule),
        Child::Error(_) => false,
        Child::Token(index) => tokens[*index].rule.is_some_and(components::is_trivia),
    }
}

// documented

/// `trait documented`: the `Documentation` nodes that precede `children[index]` with only
/// trivia between, in source order; empty when there are none.
// @lfy def/parser/traits.lfy:31
pub fn documentation_before(children: &[Child], index: usize, tokens: &[Token]) -> Vec<Node> {
    let mut found = Vec::new();
    for child in children[..index].iter().rev() {
        if !is_trivia_child(child, tokens) {
            break;
        }
        if let Child::Node(node) = child
            && node.rule == Entity::Comment(crate::grammar::terminals::comment::Comment::Documentation)
        {
            found.push(node.clone());
        }
    }
    found.reverse();
    found
}

/// Attaches `$documentation` to every child of `node` whose rule carries `documented`.
// @lfy def/parser/traits.lfy:30
pub fn attach_documentation(node: &mut Node, tokens: &[Token]) {
    for index in 0..node.children.len() {
        let Child::Node(child) = &node.children[index] else {
            continue;
        };
        if !components::is_documented(child.rule) {
            continue;
        }
        let documentation = documentation_before(&node.children, index, tokens);
        if let Child::Node(child) = &mut node.children[index] {
            child.documentation = documentation;
        }
    }
}

// triedBefore

/// `trait triedBefore(other)`: whether `first` is tried ahead of `other`, directly or
/// through rules tried between them.
// @lfy def/parser/traits.lfy:35
pub fn tried_before(first: Entity, other: Entity) -> bool {
    let mut reached = vec![first];
    let mut index = 0;
    while index < reached.len() {
        let from = reached[index];
        for &(before, after) in components::TRIED_BEFORE {
            if before == from {
                if after == other {
                    return true;
                }
                if !reached.contains(&after) {
                    reached.push(after);
                }
            }
        }
        index += 1;
    }
    false
}

/// Orders candidates so that each is tried only when the one before it fails: a total
/// order under [`tried_before`]. `Err` names two candidates the trait does not order.
// @lfy def/parser/traits.lfy:35
pub fn order_by_tried_before(candidates: &[Entity]) -> Result<Vec<Entity>, (Entity, Entity)> {
    for (index, &a) in candidates.iter().enumerate() {
        for &b in &candidates[index + 1..] {
            if !tried_before(a, b) && !tried_before(b, a) {
                return Err((a, b));
            }
        }
    }
    let mut ordered = candidates.to_vec();
    ordered.sort_by(|&a, &b| {
        if tried_before(a, b) {
            std::cmp::Ordering::Less
        } else if tried_before(b, a) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    Ok(ordered)
}

// recoverable

/// What a recoverable rule does when a repetition stops on the token at `at` (`None` at
/// the end of the input).
// @lfy def/parser/traits.lfy:41
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepetitionStop {
    /// The repetition ends and the element after it is tried.
    Ends, // @lfy def/parser/traits.lfy:41
    /// An error node covers the stopping token alone, and the repetition continues after
    /// it.
    ErrorAlone, // @lfy def/parser/traits.lfy:42
    /// An error node covers the stopping token and the tokens after it up to this index,
    /// and the repetition continues there.
    ErrorUpTo(usize), // @lfy def/parser/traits.lfy:43
}

/// Decides how a repetition of a recoverable rule with these `sync` terminals continues
/// when it stops on the token at `at`. `begins_after` tells whether a terminal can begin
/// an element after the repetition and `begins_repeated` whether it can begin the
/// repeated element.
// @lfy def/parser/traits.lfy:41
pub fn repetition_stop(
    tokens: &[Token],
    at: Option<usize>,
    sync: &[Entity],
    begins_after: impl Fn(Entity) -> bool,
    begins_repeated: impl Fn(Entity) -> bool,
) -> RepetitionStop {
    // @lfy def/parser/traits.lfy:41
    let Some(at) = at else {
        return RepetitionStop::Ends;
    };
    let rule = tokens[at].rule;
    if rule.is_some_and(&begins_after) {
        return RepetitionStop::Ends;
    }
    // @lfy def/parser/traits.lfy:42
    if rule.is_some_and(|rule| sync.contains(&rule)) {
        return RepetitionStop::ErrorAlone;
    }
    // @lfy def/parser/traits.lfy:43
    let end = tokens
        .iter()
        .enumerate()
        .skip(at + 1)
        .find(|(_, token)| {
            token.rule.is_some_and(|rule| {
                begins_repeated(rule) || begins_after(rule) || sync.contains(&rule)
            })
        })
        .map_or(tokens.len(), |(index, _)| index);
    RepetitionStop::ErrorUpTo(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::rules::expression::Expression;
    use crate::grammar::rules::statement::Statement;
    use crate::grammar::terminals::comment::Comment;
    use crate::grammar::terminals::identifier::Identifier;
    use crate::lexer::lex;

    fn punctuation(rule: Punctuation) -> Entity {
        Entity::Punctuation(rule)
    }

    // @lfy def/parser/traits.lfy:8
    #[test]
    fn brackets_pair_every_opening_token_with_its_closing_token() {
        assert_eq!(BRACKETS.len(), 5);
        for &(open, close) in BRACKETS {
            assert!(opens_bracket(open) && !closes_bracket(open), "{open}");
            assert!(closes_bracket(close) && !opens_bracket(close), "{close}");
        }
        assert!(!opens_bracket(punctuation(Punctuation::Semicolon)));
        assert!(!closes_bracket(Entity::Literal(Literal::Backtick)));
    }

    // @lfy def/parser/traits.lfy:13
    #[test]
    fn a_sweep_stops_at_a_sync_token_at_depth_zero_or_an_unmatched_close() {
        let tokens = lex("a (b; c) {d} ; e", None).unwrap();
        let sync = components::STATEMENT_SYNC;
        // Depth counts the parentheses, so the `;` inside them does not stop the sweep,
        // and `{` is a sync token at depth 0.
        assert_eq!(tokens[9].raw, "{");
        assert_eq!(sweep_end(&tokens, 0, sync), 9);
        // Starting inside the parentheses, `;` is at depth 0.
        assert_eq!(sweep_end(&tokens, 3, sync), 4);
        // A close that would take the depth below 0 ends the sweep before it.
        assert_eq!(sweep_end(&tokens, 5, sync), 7);
        // No sync at all reaches the end of the input.
        let tokens = lex("a (b c", None).unwrap();
        assert_eq!(sweep_end(&tokens, 0, sync), tokens.len());
        // Boundaries count as brackets: a `;` inside `{{ }}` is not at depth 0.
        let tokens = lex("`x {{ a; }}` ;", None).unwrap();
        assert_eq!(tokens.last().unwrap().raw, ";");
        assert_eq!(sweep_end(&tokens, 0, sync), tokens.len() - 1);
        // Invalid tokens are swept over.
        let tokens = lex("# ;", None).unwrap();
        assert_eq!(sweep_end(&tokens, 0, sync), 2);
    }

    // @lfy def/parser/traits.lfy:19
    #[test]
    fn listed_joins_rule_references_with_commas() {
        assert_eq!(
            listed(components::STATEMENT_SYNC),
            "[[Semicolon]], [[BlockOpen]], [[BlockClose]]"
        );
        assert_eq!(listed(&[]), "");
    }

    // @lfy def/parser/traits.lfy:23
    #[test]
    fn rules_that_reference_a_body_terminal_admit_no_trivia() {
        for rule in [
            Entity::Expression(Expression::Template),
            Entity::Comment(Comment::Comment),
            Entity::Comment(Comment::Documentation),
            Entity::Literal(Literal::SingleQuoteString),
            Entity::Literal(Literal::DoubleQuoteString),
        ] {
            assert!(references_body_terminal(rule), "{rule}");
            assert!(!admits_trivia(rule), "{rule}");
        }
        for rule in [
            Entity::Expression(Expression::TemplateReference),
            Entity::Expression(Expression::TemplateExecution),
            Entity::Expression(Expression::StringLiteral),
            Entity::Statement(Statement::Block),
            Entity::Expression(Expression::Call),
        ] {
            assert!(admits_trivia(rule), "{rule}");
        }
        assert!(!references_body_terminal(Entity::Identifier(Identifier::Identifier)));
    }

    // @lfy def/parser/traits.lfy:35
    #[test]
    fn tried_before_is_transitive_and_orders_candidates() {
        let block = Entity::Statement(Statement::Block);
        let expression = Entity::Expression(Expression::Expression);
        let statement = Entity::Statement(Statement::ExpressionStatement);
        assert!(tried_before(block, expression));
        assert!(tried_before(block, statement));
        assert!(!tried_before(statement, block));
        assert!(!tried_before(block, block));
        let group = Entity::Expression(Expression::Group);
        let inline = Entity::Expression(Expression::InlineFunction);
        assert_eq!(order_by_tried_before(&[group, inline]), Ok(vec![inline, group]));
        assert_eq!(order_by_tried_before(&[statement, block]), Ok(vec![block, statement]));
        assert_eq!(order_by_tried_before(&[block]), Ok(vec![block]));
        assert_eq!(order_by_tried_before(&[group, block]), Err((group, block)));
    }

    // @lfy def/parser/traits.lfy:41
    #[test]
    fn a_stopped_repetition_ends_or_covers_tokens_with_an_error() {
        let tokens = lex("a ; ) b", None).unwrap();
        let sync = components::STATEMENT_SYNC;
        let identifier = Entity::Identifier(Identifier::Identifier);
        let after = |rule: Entity| rule == punctuation(Punctuation::BlockClose);
        let repeated = |rule: Entity| rule == identifier;
        assert_eq!(
            repetition_stop(&tokens, None, sync, after, repeated),
            RepetitionStop::Ends
        );
        assert_eq!(
            repetition_stop(&tokens, Some(2), sync, after, repeated),
            RepetitionStop::ErrorAlone
        );
        assert_eq!(
            repetition_stop(&tokens, Some(4), sync, after, repeated),
            RepetitionStop::ErrorUpTo(6)
        );
        let tokens = lex("} x", None).unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(
            repetition_stop(&tokens, Some(0), sync, after, repeated),
            RepetitionStop::Ends
        );
        // Nothing begins anything: the error runs to the end of the input.
        assert_eq!(
            repetition_stop(&tokens, Some(1), sync, |_| false, |_| false),
            RepetitionStop::ErrorUpTo(3)
        );
        // The next token that begins the repeated element ends it.
        assert_eq!(
            repetition_stop(&tokens, Some(1), sync, |_| false, repeated),
            RepetitionStop::ErrorUpTo(2)
        );
    }
}
