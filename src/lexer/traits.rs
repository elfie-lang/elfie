//! Compiled from `def/lexer/traits.lfy`.
//!
//! Each trait describes how the [`ModeStack`] reacts to the terminal carrying it. They are
//! compiled to [`ModeBehavior`] values, attached to terminals in [`super::modes`], and the
//! functions here that apply them.

use crate::grammar::Entity;

use super::data::ModeStack;
use super::modes::{self, Mode};

/// `ace function inMode(modes)`: the top of the [`ModeStack`] is one of `modes`.
// @lfy def/lexer/traits.lfy:4
pub fn in_mode(stack: &ModeStack, modes: &[Mode]) -> bool {
    modes.contains(&stack.top_mode())
}

/// How the creation of a token for the terminal carrying the behavior affects the
/// [`ModeStack`]. `when` is the optional condition of an opener or toggle: the modes the
/// top of the stack must be in, or `None` for "Always".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeBehavior {
    /// `modeOpener(mode, when?)`: creating this token pushes a mode.
    Opener {
        mode: Mode,
        when: Option<&'static [Mode]>,
    }, // @lfy def/lexer/traits.lfy:13
    /// `modeCloser(mode)`: creating this token pops a mode.
    Closer { mode: Mode }, // @lfy def/lexer/traits.lfy:20
    /// `modeToggle(mode, when?)`: the same boundary opens the mode and, seen again, closes
    /// it.
    Toggle {
        mode: Mode,
        when: Option<&'static [Mode]>,
    }, // @lfy def/lexer/traits.lfy:27
    /// `appliedUntilNewLine(mode)`: the mode this token opens ends with the line.
    AppliedUntilNewLine { mode: Mode }, // @lfy def/lexer/traits.lfy:35
}

/// `when ?? Always`
fn holds(when: Option<&[Mode]>, stack: &ModeStack) -> bool {
    when.is_none_or(|modes| in_mode(stack, modes))
}

/// Applies every mode behavior of `terminal` once a token is created for it. Every
/// condition is evaluated against the stack as it is when the token is created.
// @lfy def/lexer/traits.lfy:14
pub fn on_token(terminal: Entity, stack: &mut ModeStack) {
    for behavior in modes::mode_behaviors(terminal) {
        match *behavior {
            // @lfy def/lexer/traits.lfy:16
            ModeBehavior::Opener { mode, when } => {
                if holds(when, stack) {
                    stack.push(mode, terminal);
                }
            }
            // @lfy def/lexer/traits.lfy:23
            ModeBehavior::Closer { mode } => {
                if stack.top_mode() == mode {
                    stack.pop(mode);
                }
            }
            ModeBehavior::Toggle { mode, when } => {
                let top = *stack.top();
                if top.mode == mode && top.opener == Some(terminal) {
                    // @lfy def/lexer/traits.lfy:30
                    stack.pop(mode);
                } else if top.mode != mode && holds(when, stack) {
                    // @lfy def/lexer/traits.lfy:31
                    stack.push(mode, terminal);
                }
            }
            ModeBehavior::AppliedUntilNewLine { .. } => {}
        }
    }
}

/// The mode at the top of the stack when it was opened by a terminal whose
/// `appliedUntilNewLine` names that mode; such a mode ends with the line.
// @lfy def/lexer/traits.lfy:37
pub fn mode_ending_with_line(stack: &ModeStack) -> Option<Mode> {
    let top = stack.top();
    modes::mode_behaviors(top.opener?)
        .iter()
        .find_map(|behavior| match *behavior {
            ModeBehavior::AppliedUntilNewLine { mode } if mode == top.mode => Some(mode),
            _ => None,
        })
}

/// Applies `appliedUntilNewLine`: called when the next characters are a `NewLine` or the
/// end of the source, before the line break is lexed, it pops every mode at the top that
/// ends with the line.
// @lfy def/lexer/traits.lfy:37
pub fn before_line_break(stack: &mut ModeStack) {
    while let Some(mode) = mode_ending_with_line(stack) {
        stack.pop(mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::terminals::comment::Comment;
    use crate::grammar::terminals::literal::Literal;

    fn literal(literal: Literal) -> Entity {
        Entity::Literal(literal)
    }

    fn comment(comment: Comment) -> Entity {
        Entity::Comment(comment)
    }

    // @lfy def/lexer/traits.lfy:4
    #[test]
    fn in_mode_looks_at_the_top_of_the_stack() {
        let mut stack = ModeStack::new();
        assert!(in_mode(&stack, modes::IN_CODE));
        assert!(!in_mode(&stack, &[Mode::Template]));
        stack.push(Mode::Template, literal(Literal::Backtick));
        assert!(in_mode(&stack, &[Mode::Template]));
        assert!(!in_mode(&stack, modes::IN_CODE));
    }

    // @lfy def/lexer/traits.lfy:16
    #[test]
    fn an_opener_pushes_when_its_condition_holds() {
        let mut stack = ModeStack::new();
        on_token(literal(Literal::ExecutionOpen), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code]);
        on_token(comment(Comment::LineCommentOpen), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::LineComment]);

        let mut stack = ModeStack::new();
        stack.push(Mode::Template, literal(Literal::Backtick));
        on_token(literal(Literal::ExecutionOpen), &mut stack);
        assert_eq!(
            stack.modes(),
            vec![Mode::Code, Mode::Template, Mode::Execution]
        );
        assert_eq!(stack.top().opener, Some(literal(Literal::ExecutionOpen)));
        // Block comments nest; the opener also holds inside a block comment.
        let mut stack = ModeStack::new();
        on_token(comment(Comment::BlockCommentOpen), &mut stack);
        on_token(comment(Comment::BlockCommentOpen), &mut stack);
        assert_eq!(
            stack.modes(),
            vec![Mode::Code, Mode::BlockComment, Mode::BlockComment]
        );
    }

    // @lfy def/lexer/traits.lfy:23
    #[test]
    fn a_closer_pops_only_when_its_mode_is_at_the_top() {
        let mut stack = ModeStack::new();
        stack.push(Mode::Template, literal(Literal::Backtick));
        stack.push(Mode::Execution, literal(Literal::ExecutionOpen));
        on_token(literal(Literal::ReferenceClose), &mut stack);
        assert_eq!(stack.modes().len(), 3);
        on_token(literal(Literal::ExecutionClose), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::Template]);
        on_token(literal(Literal::ExecutionClose), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::Template]);
    }

    // @lfy def/lexer/traits.lfy:30
    #[test]
    fn a_toggle_opens_its_mode_and_the_same_boundary_closes_it() {
        let single = literal(Literal::SingleQuote);
        let double = literal(Literal::DoubleQuote);
        let mut stack = ModeStack::new();
        on_token(single, &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::SingleQuote]);
        // Another boundary neither opens on top of it nor closes it.
        on_token(double, &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::SingleQuote]);
        on_token(single, &mut stack);
        assert!(stack.is_only_code());
        // The toggle respects its condition: nothing opens on top of a template body.
        stack.push(Mode::Template, literal(Literal::Backtick));
        on_token(single, &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::Template]);
        // Inside a template expression, a backtick opens a nested template.
        stack.push(Mode::Execution, literal(Literal::ExecutionOpen));
        on_token(literal(Literal::Backtick), &mut stack);
        assert_eq!(
            stack.modes(),
            vec![Mode::Code, Mode::Template, Mode::Execution, Mode::Template]
        );
        on_token(literal(Literal::Backtick), &mut stack);
        assert_eq!(
            stack.modes(),
            vec![Mode::Code, Mode::Template, Mode::Execution]
        );
    }

    // @lfy def/lexer/traits.lfy:37
    #[test]
    fn modes_applied_until_new_line_end_before_the_line_break() {
        let mut stack = ModeStack::new();
        stack.push(Mode::BlockComment, comment(Comment::BlockCommentOpen));
        assert_eq!(mode_ending_with_line(&stack), None);
        before_line_break(&mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::BlockComment]);

        for (mode, opener) in [
            (Mode::LineComment, comment(Comment::LineCommentOpen)),
            (
                Mode::LineDocumentation,
                comment(Comment::LineDocumentationOpen),
            ),
            (Mode::SingleQuote, literal(Literal::SingleQuote)),
            (Mode::DoubleQuote, literal(Literal::DoubleQuote)),
        ] {
            stack.push(mode, opener);
            assert_eq!(mode_ending_with_line(&stack), Some(mode));
            before_line_break(&mut stack);
            assert_eq!(
                stack.modes(),
                vec![Mode::Code, Mode::BlockComment],
                "{mode}"
            );
        }

        // A mode that does not end with the line shields the ones below it.
        stack.push(
            Mode::LineDocumentation,
            comment(Comment::LineDocumentationOpen),
        );
        stack.push(Mode::Reference, literal(Literal::ReferenceOpen));
        before_line_break(&mut stack);
        assert_eq!(stack.modes().len(), 4);
        // The code mode itself never ends.
        let mut stack = ModeStack::new();
        before_line_break(&mut stack);
        assert!(stack.is_only_code());
    }
}
