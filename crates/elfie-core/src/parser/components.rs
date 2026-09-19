//! Compiled from `def/parser/components.lfy`: which rules carry the parser traits of
//! `def/parser/traits.lfy`, and with what arguments.

use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::file::File;
use crate::grammar::rules::statement::{STATEMENTS, Statement};
use crate::grammar::terminals::comment::Comment;
use crate::grammar::terminals::literal::Literal;
use crate::grammar::terminals::punctuation::Punctuation;
use crate::grammar::terminals::space::Space;
use crate::grammar::{Entity, GrammarRule};

// Trivia

/// `trivia.apply(...)`: the rules that have little to no semantic meaning.
// @lfy def/parser/components.lfy:12
pub const TRIVIA: &[Entity] = &[
    Entity::Space(Space::Space),          // @lfy def/parser/components.lfy:12
    Entity::Space(Space::NewLine),        // @lfy def/parser/components.lfy:13
    Entity::Comment(Comment::Comment),    // @lfy def/parser/components.lfy:14
    Entity::Comment(Comment::Documentation), // @lfy def/parser/components.lfy:15
];

/// Whether the rule carries `trivia`.
pub fn is_trivia(rule: Entity) -> bool {
    TRIVIA.contains(&rule)
}

// Every statement can have documentation and should be recoverable

/// Whether the rule carries `documented`: every statement does.
// @lfy def/parser/components.lfy:19
pub fn is_documented(rule: Entity) -> bool {
    rule.is_statement()
}

/// The sync terminals of every statement and of the source file.
// @lfy def/parser/components.lfy:20
pub const STATEMENT_SYNC: &[Entity] = &[
    Entity::Punctuation(Punctuation::Semicolon),
    Entity::Punctuation(Punctuation::BlockOpen),
    Entity::Punctuation(Punctuation::BlockClose),
];

const GROUP_CLOSE: &[Entity] = &[Entity::Punctuation(Punctuation::GroupClose)];
const LIST_CLOSE: &[Entity] = &[Entity::Punctuation(Punctuation::ListClose)];
const REFERENCE_CLOSE: &[Entity] = &[Entity::Literal(Literal::ReferenceClose)];
const EXECUTION_CLOSE: &[Entity] = &[Entity::Literal(Literal::ExecutionClose)];

/// `recoverable.apply(rule, ...sync)`: the sync terminals of a recoverable rule, or `None`
/// for a rule without the trait.
pub fn sync(rule: Entity) -> Option<&'static [Entity]> {
    Some(match rule {
        Entity::Statement(_) if rule.is_statement() => STATEMENT_SYNC, // @lfy def/parser/components.lfy:20
        Entity::File(File::SourceFile) => STATEMENT_SYNC,               // @lfy def/parser/components.lfy:22
        // More targetted recovery
        Entity::Expression(Expression::Parameters) => GROUP_CLOSE, // @lfy def/parser/components.lfy:25
        Entity::Expression(Expression::Arguments) => GROUP_CLOSE,  // @lfy def/parser/components.lfy:26
        Entity::Expression(Expression::Call) => GROUP_CLOSE,       // @lfy def/parser/components.lfy:27
        Entity::Expression(Expression::List) => LIST_CLOSE,        // @lfy def/parser/components.lfy:28
        Entity::Expression(Expression::Index) => LIST_CLOSE,       // @lfy def/parser/components.lfy:29
        Entity::Expression(Expression::TemplateReference) => REFERENCE_CLOSE, // @lfy def/parser/components.lfy:30
        Entity::Expression(Expression::TemplateExecution) => EXECUTION_CLOSE, // @lfy def/parser/components.lfy:31
        _ => return None,
    })
}

/// Whether the rule carries `recoverable`.
pub fn is_recoverable(rule: Entity) -> bool {
    sync(rule).is_some()
}

// What to try for first when things begin with the same token

/// `triedBefore.apply(first, other)`: `first` is tried ahead of `other` wherever both could
/// begin at the same token.
// @lfy def/parser/components.lfy:34
pub const TRIED_BEFORE: &[(Entity, Entity)] = &[
    (Entity::Statement(Statement::Block), Entity::Expression(Expression::Expression)), // @lfy def/parser/components.lfy:34
    (Entity::Statement(Statement::Block), Entity::Statement(Statement::ExpressionStatement)), // @lfy def/parser/components.lfy:35
    (Entity::Statement(Statement::FunctionDeclaration), Entity::Statement(Statement::ExpressionStatement)), // @lfy def/parser/components.lfy:36
    (Entity::Statement(Statement::TraitDeclaration), Entity::Statement(Statement::ExpressionStatement)), // @lfy def/parser/components.lfy:37
    (Entity::Expression(Expression::Object), Entity::Expression(Expression::Type)), // @lfy def/parser/components.lfy:38
    (Entity::Expression(Expression::InlineFunction), Entity::Expression(Expression::Group)), // @lfy def/parser/components.lfy:39
    (Entity::Statement(Statement::ConditionGroup), Entity::Expression(Expression::Group)), // @lfy def/parser/components.lfy:40
];

/// The rules that carry `documented` and `recoverable` through the statement loop.
pub fn statements() -> &'static [Entity] {
    STATEMENTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::rules;

    // @lfy def/parser/components.lfy:12
    #[test]
    fn trivia_is_space_new_line_comment_and_documentation() {
        assert_eq!(TRIVIA.len(), 4);
        assert!(is_trivia(Entity::Space(Space::Space)));
        assert!(is_trivia(Entity::Space(Space::NewLine)));
        assert!(is_trivia(Entity::Comment(Comment::Comment)));
        assert!(is_trivia(Entity::Comment(Comment::Documentation)));
        assert!(!is_trivia(Entity::Comment(Comment::LineCommentOpen)));
        assert!(!is_trivia(Entity::Statement(Statement::Block)));
    }

    // @lfy def/parser/components.lfy:18
    #[test]
    fn every_statement_is_documented_and_recoverable_at_statement_boundaries() {
        for &rule in statements() {
            assert!(is_documented(rule), "{rule}");
            assert_eq!(sync(rule), Some(STATEMENT_SYNC), "{rule}");
        }
        assert_eq!(statements().len(), 24);
        assert_eq!(sync(Entity::File(File::SourceFile)), Some(STATEMENT_SYNC));
        assert!(!is_documented(Entity::File(File::SourceFile)));
        assert!(!is_documented(Entity::Statement(Statement::Else)));
        assert_eq!(sync(Entity::Statement(Statement::Else)), None);
        assert_eq!(sync(Entity::Statement(Statement::MatchArm)), None);
    }

    // @lfy def/parser/components.lfy:24
    #[test]
    fn targeted_recovery_closes_on_the_matching_bracket() {
        let close = |rule: Expression| sync(Entity::Expression(rule)).unwrap();
        assert_eq!(close(Expression::Parameters), GROUP_CLOSE);
        assert_eq!(close(Expression::Arguments), GROUP_CLOSE);
        assert_eq!(close(Expression::Call), GROUP_CLOSE);
        assert_eq!(close(Expression::List), LIST_CLOSE);
        assert_eq!(close(Expression::Index), LIST_CLOSE);
        assert_eq!(close(Expression::TemplateReference), REFERENCE_CLOSE);
        assert_eq!(close(Expression::TemplateExecution), EXECUTION_CLOSE);
        let recoverable: Vec<Entity> = rules().filter(|rule| is_recoverable(*rule)).collect();
        assert_eq!(recoverable.len(), 24 + 1 + 7);
        assert!(!is_recoverable(Entity::Expression(Expression::Group)));
        assert!(!is_recoverable(Entity::Expression(Expression::Object)));
    }

    // @lfy def/parser/components.lfy:34
    #[test]
    fn tried_before_lists_the_seven_orderings() {
        assert_eq!(TRIED_BEFORE.len(), 7);
        for &(first, other) in TRIED_BEFORE {
            assert_ne!(first, other);
            assert!(!first.is_terminal() && !other.is_terminal(), "{first} {other}");
        }
        assert_eq!(
            TRIED_BEFORE[5],
            (
                Entity::Expression(Expression::InlineFunction),
                Entity::Expression(Expression::Group)
            )
        );
    }
}
