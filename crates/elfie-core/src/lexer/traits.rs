//! Compiled from `def/lexer/traits.lfy`.
//!
//! The regions of the source ([`Mode`]) and the traits that describe how a token of the
//! terminal carrying them affects the [`ModeStack`]. The traits are compiled to
//! [`ModeBehavior`] values, attached to terminals in [`super::modes`], and applied by the
//! functions here.

use std::fmt;

use crate::grammar::Entity;

use super::modes::{self, ModeStack};

/// Regions of the source that change which terminals may match.
// @lfy def/lexer/traits.lfy:Mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Code,               // @lfy def/lexer/traits.lfy:Mode.code
    BlockComment,       // @lfy def/lexer/traits.lfy:Mode.blockComment
    LineComment,        // @lfy def/lexer/traits.lfy:Mode.lineComment
    BlockDocumentation, // @lfy def/lexer/traits.lfy:Mode.blockDocumentation
    LineDocumentation,  // @lfy def/lexer/traits.lfy:Mode.lineDocumentation
    SingleQuote,        // @lfy def/lexer/traits.lfy:Mode.singleQuote
    DoubleQuote,        // @lfy def/lexer/traits.lfy:Mode.doubleQuote
    Template,           // @lfy def/lexer/traits.lfy:Mode.template
    Execution,          // @lfy def/lexer/traits.lfy:Mode.execution
    Reference,          // @lfy def/lexer/traits.lfy:Mode.reference
}

impl Mode {
    /// Every mode, in declaration order.
    pub const ALL: &'static [Mode] = &[
        Mode::Code,
        Mode::BlockComment,
        Mode::LineComment,
        Mode::BlockDocumentation,
        Mode::LineDocumentation,
        Mode::SingleQuote,
        Mode::DoubleQuote,
        Mode::Template,
        Mode::Execution,
        Mode::Reference,
    ];

    /// The value the mode was declared with.
    pub const fn name(self) -> &'static str {
        match self {
            Mode::Code => "code",
            Mode::BlockComment => "block comment",
            Mode::LineComment => "line comment",
            Mode::BlockDocumentation => "block documentation",
            Mode::LineDocumentation => "line documentation",
            Mode::SingleQuote => "single quoted string",
            Mode::DoubleQuote => "double quoted string",
            Mode::Template => "template",
            Mode::Execution => "template execution",
            Mode::Reference => "template reference",
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The condition that the innermost open region is one of `modes`.
// @lfy def/lexer/traits.lfy:inMode
pub fn in_mode(modes: &[Mode]) -> String {
    let names: Vec<&str> = modes.iter().map(|mode| mode.name()).collect();
    format!("The innermost open region is a {} region", names.join(" or "))
}

/// Whether the innermost open region of `stack` is one of `modes`.
// @lfy def/lexer/traits.lfy:inMode
pub fn top_in(stack: &ModeStack, modes: &[Mode]) -> bool {
    modes.contains(&stack.top_mode())
}

/// How the creation of a token for the terminal carrying the behavior affects the
/// [`ModeStack`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeBehavior {
    /// `opener(mode)`: each token of the terminal opens a region.
    Opener { mode: Mode }, // @lfy def/lexer/traits.lfy:opener
    /// `closer(mode)`: a token of the terminal closes the region when it is the innermost.
    Closer { mode: Mode }, // @lfy def/lexer/traits.lfy:closer
    /// `delimiter(mode)`: the same boundary opens a region and, seen again, closes it.
    Delimiter { mode: Mode }, // @lfy def/lexer/traits.lfy:delimiter
    /// `lineBounded(mode)`: the region this terminal opens ends with its line.
    LineBounded { mode: Mode }, // @lfy def/lexer/traits.lfy:lineBounded
}

/// Applies every mode behavior of `terminal` once a token is created for it. Every
/// condition is evaluated against the stack as it is when the token is created.
pub fn on_token(terminal: Entity, stack: &mut ModeStack) {
    for behavior in modes::mode_behaviors(terminal) {
        match *behavior {
            // @lfy def/lexer/traits.lfy:opener
            ModeBehavior::Opener { mode } => stack.push(mode, terminal),
            // @lfy def/lexer/traits.lfy:closer
            ModeBehavior::Closer { mode } => {
                stack.pop(mode);
            }
            ModeBehavior::Delimiter { mode } => {
                let top = *stack.top();
                if top.mode == mode && top.opener == Some(terminal) {
                    // @lfy def/lexer/traits.lfy:delimiter
                    stack.pop(mode);
                } else {
                    // @lfy def/lexer/traits.lfy:delimiter
                    stack.push(mode, terminal);
                }
            }
            ModeBehavior::LineBounded { .. } => {}
        }
    }
}

/// The mode at the top of the stack when its opener is a terminal whose `lineBounded`
/// names that mode; such a mode ends with the line.
// @lfy def/lexer/traits.lfy:lineBounded
pub fn mode_ending_with_line(stack: &ModeStack) -> Option<Mode> {
    let top = stack.top();
    modes::mode_behaviors(top.opener?)
        .iter()
        .find_map(|behavior| match *behavior {
            ModeBehavior::LineBounded { mode } if mode == top.mode => Some(mode),
            _ => None,
        })
}

/// Applies `lineBounded`: called when the next characters are a `NewLine` or the end of
/// the source, before the line break is lexed, it pops every region at the top that ends
/// with the line.
// @lfy def/lexer/traits.lfy:lineBounded
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

    // @lfy def/lexer/traits.lfy:Mode
    #[test]
    fn mode_names_match_their_declarations() {
        assert_eq!(Mode::ALL.len(), 10);
        assert_eq!(Mode::Code.name(), "code");
        assert_eq!(Mode::BlockComment.name(), "block comment");
        assert_eq!(Mode::LineDocumentation.name(), "line documentation");
        assert_eq!(Mode::SingleQuote.name(), "single quoted string");
        assert_eq!(Mode::Execution.to_string(), "template execution");
        assert_eq!(Mode::Reference.to_string(), "template reference");
    }

    // @lfy def/lexer/traits.lfy:inMode
    #[test]
    fn in_mode_names_the_regions_and_looks_at_the_top_of_the_stack() {
        assert_eq!(
            in_mode(&[Mode::Code, Mode::Execution]),
            "The innermost open region is a code or template execution region"
        );
        let mut stack = ModeStack::new();
        assert!(top_in(&stack, modes::IN_CODE));
        assert!(!top_in(&stack, &[Mode::Template]));
        stack.push(Mode::Template, literal(Literal::Backtick));
        assert!(top_in(&stack, &[Mode::Template]));
        assert!(!top_in(&stack, modes::IN_CODE));
    }

    // @lfy def/lexer/traits.lfy:opener
    #[test]
    fn an_opener_pushes_its_mode_nested_in_the_innermost_region() {
        let mut stack = ModeStack::new();
        on_token(comment(Comment::LineCommentOpen), &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::LineComment]);
        assert_eq!(
            stack.top().opener,
            Some(comment(Comment::LineCommentOpen))
        );

        let mut stack = ModeStack::new();
        stack.push(Mode::Template, literal(Literal::Backtick));
        on_token(literal(Literal::ExecutionOpen), &mut stack);
        assert_eq!(
            stack.modes(),
            vec![Mode::Code, Mode::Template, Mode::Execution]
        );
        assert_eq!(stack.top().opener, Some(literal(Literal::ExecutionOpen)));
        // Block comments nest.
        let mut stack = ModeStack::new();
        on_token(comment(Comment::BlockCommentOpen), &mut stack);
        on_token(comment(Comment::BlockCommentOpen), &mut stack);
        assert_eq!(
            stack.modes(),
            vec![Mode::Code, Mode::BlockComment, Mode::BlockComment]
        );
    }

    // @lfy def/lexer/traits.lfy:closer
    #[test]
    fn a_closer_pops_only_when_its_mode_is_the_innermost() {
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

    // @lfy def/lexer/traits.lfy:delimiter
    #[test]
    fn a_delimiter_opens_its_region_and_the_same_boundary_closes_it() {
        let single = literal(Literal::SingleQuote);
        let mut stack = ModeStack::new();
        on_token(single, &mut stack);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::SingleQuote]);
        on_token(single, &mut stack);
        assert!(stack.is_only_code());
        // Inside a template expression, a backtick opens a nested template and a second
        // one closes it.
        stack.push(Mode::Template, literal(Literal::Backtick));
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

    // @lfy def/lexer/traits.lfy:lineBounded
    #[test]
    fn regions_bounded_by_the_line_close_before_the_line_break() {
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

        // A region that does not end with the line shields the ones below it.
        stack.push(
            Mode::LineDocumentation,
            comment(Comment::LineDocumentationOpen),
        );
        stack.push(Mode::Reference, literal(Literal::ReferenceOpen));
        before_line_break(&mut stack);
        assert_eq!(stack.modes().len(), 4);
        // The code region itself never ends.
        let mut stack = ModeStack::new();
        before_line_break(&mut stack);
        assert!(stack.is_only_code());
    }
}
