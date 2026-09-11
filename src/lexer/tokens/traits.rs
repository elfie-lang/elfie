//! Compiled from `def/lexer/tokens/traits.lfy`.
//!
//! Each trait in the source describes how the [`LexerModeStack`] reacts when a token
//! carrying that trait is created. Traits are compiled to [`ModeBehavior`] /
//! [`FollowRule`] values attached to token kinds, plus the functions here that apply them.

use super::super::data::{LexError, LexerModeStack, Mode, TokenKind};
use super::literal;

/// A condition evaluated against the mode stack at the moment a token is created.
pub type Predicate = fn(&LexerModeStack) -> bool;

/// How the creation of a token affects the mode stack.
#[derive(Debug, Clone, Copy)]
pub enum ModeBehavior {
    /// `modeCreator(mode, condition?)`
    Creator {
        mode: Mode,
        condition: Option<Predicate>,
    }, // @lfy def/lexer/tokens/traits.lfy:3
    /// `modeDestroyer(mode, condition?)`
    Destroyer {
        mode: Mode,
        condition: Option<Predicate>,
    }, // @lfy def/lexer/tokens/traits.lfy:11
    /// `modeBoundary(mode, condition?)`
    Boundary {
        mode: Mode,
        condition: Option<Predicate>,
    }, // @lfy def/lexer/tokens/traits.lfy:19
    /// `clearModeAtNewLine(mode)`
    ClearAtNewLine { mode: Mode }, // @lfy def/lexer/tokens/traits.lfy:31
}

/// One entry of a `cantBeFollowedBy([...])` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowRule {
    /// Followed by this exact text.
    Literal(&'static str), // @lfy def/lexer/tokens/comment.lfy:25
    /// Followed by `a non-numeric character` (anything but an ASCII digit).
    NonNumericCharacter, // @lfy def/lexer/tokens/operator.lfy:14
}

fn holds(condition: Option<Predicate>, stack: &LexerModeStack) -> bool {
    condition.is_none_or(|condition| condition(stack))
}

/// Applies every mode behavior of `kind` after a token of that kind was created.
/// All conditions are evaluated against the stack as it was when the token was created.
pub fn on_token_created(kind: TokenKind, stack: &mut LexerModeStack) -> Result<(), LexError> {
    for behavior in kind.mode_behaviors() {
        match *behavior {
            // @lfy def/lexer/tokens/traits.lfy:6
            ModeBehavior::Creator { mode, condition } => {
                if literal::is_mode_allowed(mode, stack) && holds(condition, stack) {
                    stack.push(mode, kind);
                }
            }
            // @lfy def/lexer/tokens/traits.lfy:14
            ModeBehavior::Destroyer { mode, condition } => {
                if stack.top_mode() == Some(mode) && holds(condition, stack) {
                    stack.pop(mode)?;
                }
            }
            // @lfy def/lexer/tokens/traits.lfy:24
            ModeBehavior::Boundary { mode, condition } => {
                let in_mode = stack.top_mode() == Some(mode); // @lfy def/lexer/tokens/traits.lfy:21
                let matches_open = stack.top().is_some_and(|entry| entry.creator == kind); // @lfy def/lexer/tokens/traits.lfy:22
                if !in_mode && holds(condition, stack) {
                    stack.push(mode, kind);
                } else if in_mode && matches_open {
                    // @lfy def/lexer/tokens/traits.lfy:26
                    stack.pop(mode)?;
                }
            }
            ModeBehavior::ClearAtNewLine { .. } => {}
        }
    }
    Ok(())
}

/// Applies `clearModeAtNewLine`: when `\n`, `\r\n` or EOF is encountered and the mode at
/// the top of the stack was created by a token carrying the trait, the mode is popped.
// @lfy def/lexer/tokens/traits.lfy:32
pub fn on_end_of_line(stack: &mut LexerModeStack) -> Result<(), LexError> {
    let Some(top) = stack.top() else {
        return Ok(());
    };
    let (top_mode, creator) = (top.mode, top.creator);
    for behavior in creator.mode_behaviors() {
        if let ModeBehavior::ClearAtNewLine { mode } = *behavior {
            // @lfy def/lexer/tokens/traits.lfy:34
            if top_mode == mode {
                stack.pop(mode)?;
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Applies `cantBeFollowedBy`: a candidate token is not a match when the text after it
/// satisfies any of its forbidden follow rules.
// @lfy def/lexer/tokens/traits.lfy:41
pub fn is_followed_by_forbidden(kind: TokenKind, after: &str) -> bool {
    kind.cant_be_followed_by().iter().any(|rule| match *rule {
        FollowRule::Literal(text) => after.starts_with(text),
        FollowRule::NonNumericCharacter => {
            after.chars().next().is_some_and(|c| !c.is_ascii_digit())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::comment::Comment;
    use super::super::literal::{Literal, TemplateBlock};
    use super::super::separator::Separator;
    use super::*;

    fn kind_of(separator: Separator) -> TokenKind {
        TokenKind::Separator(separator)
    }

    // @lfy def/lexer/tokens/traits.lfy:6
    #[test]
    fn mode_creator_pushes_when_allowed_and_condition_holds() {
        let mut stack = LexerModeStack::new();
        on_token_created(kind_of(Separator::BlockOpen), &mut stack).unwrap();
        assert_eq!(stack.modes(), vec![Mode::Block]);

        // TemplateExecution is only allowed on top of a template literal.
        on_token_created(
            TokenKind::TemplateBlock(TemplateBlock::ExecutionOpen),
            &mut stack,
        )
        .unwrap();
        assert_eq!(stack.modes(), vec![Mode::Block]);

        stack.push(
            Mode::TemplateLiteral,
            TokenKind::Literal(Literal::StringTemplateBoundary),
        );
        on_token_created(
            TokenKind::TemplateBlock(TemplateBlock::ExecutionOpen),
            &mut stack,
        )
        .unwrap();
        assert_eq!(
            stack.modes(),
            vec![Mode::Block, Mode::TemplateLiteral, Mode::TemplateExecution]
        );
    }

    // @lfy def/lexer/tokens/traits.lfy:14
    #[test]
    fn mode_destroyer_pops_only_when_its_mode_is_on_top() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Block, kind_of(Separator::BlockOpen));
        stack.push(Mode::Group, kind_of(Separator::GroupOpen));
        on_token_created(kind_of(Separator::BlockClose), &mut stack).unwrap();
        assert_eq!(stack.modes(), vec![Mode::Block, Mode::Group]);
        on_token_created(kind_of(Separator::GroupClose), &mut stack).unwrap();
        assert_eq!(stack.modes(), vec![Mode::Block]);
        on_token_created(kind_of(Separator::BlockClose), &mut stack).unwrap();
        assert!(stack.is_empty());
    }

    // @lfy def/lexer/tokens/traits.lfy:24
    #[test]
    fn mode_boundary_pushes_then_pops_for_the_same_boundary_only() {
        let single = TokenKind::Literal(Literal::StringSingleBoundary);
        let double = TokenKind::Literal(Literal::StringDoubleBoundary);
        let mut stack = LexerModeStack::new();
        on_token_created(single, &mut stack).unwrap();
        assert_eq!(stack.modes(), vec![Mode::StringLiteral]);
        on_token_created(double, &mut stack).unwrap();
        assert_eq!(stack.modes(), vec![Mode::StringLiteral]);
        on_token_created(single, &mut stack).unwrap();
        assert!(stack.is_empty());
    }

    // @lfy def/lexer/tokens/traits.lfy:24
    #[test]
    fn mode_boundary_respects_its_condition() {
        let mut stack = LexerModeStack::new();
        stack.push(
            Mode::TemplateLiteral,
            TokenKind::Literal(Literal::StringTemplateBoundary),
        );
        on_token_created(
            TokenKind::Literal(Literal::StringSingleBoundary),
            &mut stack,
        )
        .unwrap();
        assert_eq!(stack.modes(), vec![Mode::TemplateLiteral]);
    }

    // @lfy def/lexer/tokens/traits.lfy:34
    #[test]
    fn clear_mode_at_new_line_pops_only_inline_created_modes() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Comment, TokenKind::Comment(Comment::BlockOpen));
        on_end_of_line(&mut stack).unwrap();
        assert_eq!(stack.modes(), vec![Mode::Comment]);

        stack.push(Mode::Comment, TokenKind::Comment(Comment::InlineStart));
        on_end_of_line(&mut stack).unwrap();
        assert_eq!(stack.modes(), vec![Mode::Comment]);

        stack.push(
            Mode::Documentation,
            TokenKind::Comment(Comment::DocInlineStart),
        );
        on_end_of_line(&mut stack).unwrap();
        assert_eq!(stack.modes(), vec![Mode::Comment]);
    }

    // @lfy def/lexer/tokens/traits.lfy:41
    #[test]
    fn cant_be_followed_by_rejects_forbidden_followers() {
        assert!(is_followed_by_forbidden(
            TokenKind::Comment(Comment::DocBlockOpen),
            "/ x"
        ));
        assert!(!is_followed_by_forbidden(
            TokenKind::Comment(Comment::DocBlockOpen),
            " x"
        ));
        assert!(!is_followed_by_forbidden(
            TokenKind::Comment(Comment::DocBlockOpen),
            ""
        ));
        assert!(!is_followed_by_forbidden(
            TokenKind::Comment(Comment::BlockOpen),
            "/"
        ));
    }
}
