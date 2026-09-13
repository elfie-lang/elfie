//! Compiled from `def/lexer/traits.lfy`.
//!
//! Each trait describes how the [`LexerModeStack`] reacts when a token carrying it is
//! created. They are compiled to [`ModeBehavior`] values (attached to token kinds in
//! [`super::modes`]) and the functions here that apply them.

use super::data::{LexerModeStack, TokenKind};
use super::modes::{self, Mode};

/// The optional `condition` argument of a mode trait, evaluated against the mode stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    /// No condition was given (`condition ?? true`).
    Always,
    /// `inModes(...)`
    InModes(&'static [Mode]), // @lfy def/lexer/modes.lfy:23
    /// `notInModes(...)`
    NotInModes(&'static [Mode]), // @lfy def/lexer/modes.lfy:24
}

impl Condition {
    pub fn holds(self, stack: &LexerModeStack) -> bool {
        match self {
            Condition::Always => true,
            Condition::InModes(modes) => modes::in_modes(stack, modes),
            Condition::NotInModes(modes) => modes::not_in_modes(stack, modes),
        }
    }
}

/// How the creation of a token affects the mode stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeBehavior {
    /// `modeCreator(mode, condition?)`
    Creator { mode: Mode, condition: Condition }, // @lfy def/lexer/traits.lfy:3
    /// `modeDestroyer(mode, condition?)`
    Destroyer { mode: Mode, condition: Condition }, // @lfy def/lexer/traits.lfy:11
    /// `modeBoundary(mode, condition?)`
    Boundary { mode: Mode, condition: Condition }, // @lfy def/lexer/traits.lfy:19
    /// `clearModeAtNewLine(mode)`
    ClearAtNewLine { mode: Mode }, // @lfy def/lexer/traits.lfy:31
}

/// Applies every mode behavior of `kind` after a token of that kind was created. All
/// conditions are evaluated against the stack as it was when the token was created.
// @lfy def/lexer/traits.lfy:4
pub fn on_token_created(kind: TokenKind, stack: &mut LexerModeStack) {
    for behavior in modes::mode_behaviors(kind) {
        match *behavior {
            // @lfy def/lexer/traits.lfy:6
            ModeBehavior::Creator { mode, condition } => {
                if mode.is_allowed(stack) && condition.holds(stack) {
                    stack.push(mode, kind);
                }
            }
            // @lfy def/lexer/traits.lfy:14
            ModeBehavior::Destroyer { mode, condition } => {
                if stack.top_mode() == Some(mode) && condition.holds(stack) {
                    stack.pop(mode);
                }
            }
            // @lfy def/lexer/traits.lfy:24
            ModeBehavior::Boundary { mode, condition } => {
                let in_mode = stack.top_mode() == Some(mode); // @lfy def/lexer/traits.lfy:21
                let matches_open = stack.top().is_some_and(|entry| entry.creator == kind); // @lfy def/lexer/traits.lfy:22
                if !in_mode && condition.holds(stack) {
                    stack.push(mode, kind);
                } else if in_mode && matches_open {
                    // @lfy def/lexer/traits.lfy:26
                    stack.pop(mode);
                }
            }
            ModeBehavior::ClearAtNewLine { .. } => {}
        }
    }
}

/// Applies `clearModeAtNewLine`: when `\n`, `\r\n` or the end of the input is encountered
/// and the mode at the top of the stack was created by a token carrying the trait for that
/// mode, the mode is popped.
// @lfy def/lexer/traits.lfy:32
pub fn on_end_of_line(stack: &mut LexerModeStack) {
    let Some(top) = stack.top() else {
        return;
    };
    let (top_mode, creator) = (top.mode, top.creator);
    for behavior in modes::mode_behaviors(creator) {
        // @lfy def/lexer/traits.lfy:34
        if let ModeBehavior::ClearAtNewLine { mode } = *behavior
            && top_mode == mode
        {
            stack.pop(mode);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::tokens::comment::Comment;
    use crate::grammar::tokens::literal::Literal;
    use crate::grammar::tokens::separator::Separator;

    fn sep(separator: Separator) -> TokenKind {
        TokenKind::Separator(separator)
    }

    fn lit(literal: Literal) -> TokenKind {
        TokenKind::Literal(literal)
    }

    // @lfy def/lexer/traits.lfy:6
    #[test]
    fn mode_creator_pushes_when_allowed_and_condition_holds() {
        let mut stack = LexerModeStack::new();
        on_token_created(sep(Separator::BlockOpenSeparator), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Block]);

        // TemplateExecution is only allowed on top of a template literal.
        on_token_created(lit(Literal::ExecutionOpenBoundary), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Block]);

        stack.push(Mode::TemplateLiteral, lit(Literal::BacktickBoundary));
        on_token_created(lit(Literal::ExecutionOpenBoundary), &mut stack);
        assert_eq!(
            stack.modes(),
            vec![Mode::Block, Mode::TemplateLiteral, Mode::TemplateExecution]
        );

        // A block open inside a string body mode does not push (notInContentBodyMode).
        let mut stack = LexerModeStack::new();
        stack.push(Mode::StringLiteral, lit(Literal::SingleQuoteBoundary));
        on_token_created(sep(Separator::BlockOpenSeparator), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::StringLiteral]);
    }

    // @lfy def/lexer/traits.lfy:14
    #[test]
    fn mode_destroyer_pops_only_when_its_mode_is_on_top() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Block, sep(Separator::BlockOpenSeparator));
        stack.push(Mode::Group, sep(Separator::GroupOpenSeparator));
        on_token_created(sep(Separator::BlockCloseSeparator), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Block, Mode::Group]);
        on_token_created(sep(Separator::GroupCloseSeparator), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Block]);
        on_token_created(sep(Separator::BlockCloseSeparator), &mut stack);
        assert!(stack.is_empty());
    }

    // @lfy def/lexer/traits.lfy:24
    #[test]
    fn mode_boundary_pushes_then_pops_for_the_same_boundary_only() {
        let single = lit(Literal::SingleQuoteBoundary);
        let double = lit(Literal::DoubleQuoteBoundary);
        let mut stack = LexerModeStack::new();
        on_token_created(single, &mut stack);
        assert_eq!(stack.modes(), vec![Mode::StringLiteral]);
        on_token_created(double, &mut stack);
        assert_eq!(stack.modes(), vec![Mode::StringLiteral]);
        on_token_created(single, &mut stack);
        assert!(stack.is_empty());
    }

    // @lfy def/lexer/traits.lfy:24
    #[test]
    fn mode_boundary_respects_its_condition() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::TemplateLiteral, lit(Literal::BacktickBoundary));
        on_token_created(lit(Literal::SingleQuoteBoundary), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::TemplateLiteral]);
        stack.push(Mode::TemplateExecution, lit(Literal::ExecutionOpenBoundary));
        on_token_created(lit(Literal::BacktickBoundary), &mut stack);
        assert_eq!(
            stack.modes(),
            vec![
                Mode::TemplateLiteral,
                Mode::TemplateExecution,
                Mode::TemplateLiteral
            ]
        );
    }

    // @lfy def/lexer/traits.lfy:34
    #[test]
    fn clear_mode_at_new_line_pops_only_modes_created_by_a_clearing_token() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Comment, TokenKind::Comment(Comment::CommentBlockOpen));
        on_end_of_line(&mut stack);
        assert_eq!(stack.modes(), vec![Mode::Comment]);

        stack.push(
            Mode::Comment,
            TokenKind::Comment(Comment::CommentInlineStart),
        );
        on_end_of_line(&mut stack);
        assert_eq!(stack.modes(), vec![Mode::Comment]);

        stack.push(
            Mode::Documentation,
            TokenKind::Comment(Comment::DocumentationInlineStart),
        );
        on_end_of_line(&mut stack);
        assert_eq!(stack.modes(), vec![Mode::Comment]);

        stack.push(Mode::StringLiteral, lit(Literal::SingleQuoteBoundary));
        on_end_of_line(&mut stack);
        assert_eq!(stack.modes(), vec![Mode::Comment]);

        stack.push(Mode::StringLiteral, lit(Literal::DoubleQuoteBoundary));
        on_end_of_line(&mut stack);
        assert_eq!(stack.modes(), vec![Mode::Comment, Mode::StringLiteral]);
    }
}
