//! Compiled from `def/lexer/modes.lfy`.
//!
//! The lexing modes, the predicates that inspect the [`LexerModeStack`], and the mode
//! logic that attaches the mode traits of `def/lexer/traits.lfy` to token kinds.

use std::fmt;

use crate::grammar::tokens::comment::Comment;
use crate::grammar::tokens::literal::Literal;
use crate::grammar::tokens::separator::Separator;

use super::data::{LexerModeStack, TokenKind};
use super::traits::{Condition, ModeBehavior};

/// A lexing mode; `name()` yields the `mode::*` string it was declared as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Comment,         // @lfy def/lexer/modes.lfy:9
    Documentation,   // @lfy def/lexer/modes.lfy:10
    StringLiteral,   // @lfy def/lexer/modes.lfy:12
    TemplateLiteral, // @lfy def/lexer/modes.lfy:13
    /// Allowed only when `TemplateLiteral` or `Documentation` is at the top of the stack.
    TemplateReference, // @lfy def/lexer/modes.lfy:14
    /// Allowed only when `TemplateLiteral` is at the top of the stack.
    TemplateExecution, // @lfy def/lexer/modes.lfy:15
    List,            // @lfy def/lexer/modes.lfy:17
    Block,           // @lfy def/lexer/modes.lfy:18
    Group,           // @lfy def/lexer/modes.lfy:19
}

impl Mode {
    /// The `mode::*` identifier this mode was declared with.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Comment => "mode::comment", // @lfy def/lexer/modes.lfy:9
            Mode::Documentation => "mode::documentation", // @lfy def/lexer/modes.lfy:10
            Mode::StringLiteral => "mode::literal::string", // @lfy def/lexer/modes.lfy:12
            Mode::TemplateLiteral => "mode::literal::template", // @lfy def/lexer/modes.lfy:13
            Mode::TemplateReference => "mode::template::reference", // @lfy def/lexer/modes.lfy:14
            Mode::TemplateExecution => "mode::template::execution", // @lfy def/lexer/modes.lfy:15
            Mode::List => "mode::list",       // @lfy def/lexer/modes.lfy:17
            Mode::Block => "mode::block",     // @lfy def/lexer/modes.lfy:18
            Mode::Group => "mode::group",     // @lfy def/lexer/modes.lfy:19
        }
    }

    /// Whether this mode is allowed to be created on top of the current stack.
    pub fn is_allowed(self, stack: &LexerModeStack) -> bool {
        match self {
            // @lfy def/lexer/modes.lfy:14
            Mode::TemplateReference => {
                in_modes(stack, &[Mode::TemplateLiteral, Mode::Documentation])
            }
            // @lfy def/lexer/modes.lfy:15
            Mode::TemplateExecution => in_modes(stack, &[Mode::TemplateLiteral]),
            _ => true,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// Mode matchers

/// One of `modes` is the top mode on the stack.
// @lfy def/lexer/modes.lfy:23
pub fn in_modes(stack: &LexerModeStack, modes: &[Mode]) -> bool {
    stack.top_mode().is_some_and(|top| modes.contains(&top))
}

/// The top mode on the stack is not one of `modes`.
// @lfy def/lexer/modes.lfy:24
pub fn not_in_modes(stack: &LexerModeStack, modes: &[Mode]) -> bool {
    !in_modes(stack, modes)
}

/// `mode` is on top and was pushed when a token of kind `creator` was created.
fn created_by(stack: &LexerModeStack, mode: Mode, creator: TokenKind) -> bool {
    stack
        .top()
        .is_some_and(|entry| entry.mode == mode && entry.creator == creator)
}

// @lfy def/lexer/modes.lfy:26
pub fn in_comment(stack: &LexerModeStack) -> bool {
    in_modes(stack, &[Mode::Comment])
}

// @lfy def/lexer/modes.lfy:27
pub fn in_inline_comment(stack: &LexerModeStack) -> bool {
    created_by(
        stack,
        Mode::Comment,
        TokenKind::Comment(Comment::CommentInlineStart),
    )
}

// @lfy def/lexer/modes.lfy:28
pub fn in_block_comment(stack: &LexerModeStack) -> bool {
    created_by(
        stack,
        Mode::Comment,
        TokenKind::Comment(Comment::CommentBlockOpen),
    )
}

// @lfy def/lexer/modes.lfy:29
pub fn in_documentation(stack: &LexerModeStack) -> bool {
    in_modes(stack, &[Mode::Documentation])
}

// @lfy def/lexer/modes.lfy:30
pub fn in_inline_documentation(stack: &LexerModeStack) -> bool {
    created_by(
        stack,
        Mode::Documentation,
        TokenKind::Comment(Comment::DocumentationInlineStart),
    )
}

// @lfy def/lexer/modes.lfy:31
pub fn in_block_documentation(stack: &LexerModeStack) -> bool {
    created_by(
        stack,
        Mode::Documentation,
        TokenKind::Comment(Comment::DocumentationBlockOpen),
    )
}

// @lfy def/lexer/modes.lfy:33
pub fn in_string(stack: &LexerModeStack) -> bool {
    in_modes(stack, &[Mode::StringLiteral])
}

// @lfy def/lexer/modes.lfy:34
pub fn in_single_quote_string(stack: &LexerModeStack) -> bool {
    created_by(
        stack,
        Mode::StringLiteral,
        TokenKind::Literal(Literal::SingleQuoteBoundary),
    )
}

// @lfy def/lexer/modes.lfy:35
pub fn in_double_quote_string(stack: &LexerModeStack) -> bool {
    created_by(
        stack,
        Mode::StringLiteral,
        TokenKind::Literal(Literal::DoubleQuoteBoundary),
    )
}

// @lfy def/lexer/modes.lfy:36
pub fn in_template(stack: &LexerModeStack) -> bool {
    in_modes(stack, &[Mode::TemplateLiteral])
}

// @lfy def/lexer/modes.lfy:37
pub fn in_template_reference(stack: &LexerModeStack) -> bool {
    in_modes(stack, &[Mode::TemplateReference])
}

// @lfy def/lexer/modes.lfy:38
pub fn in_template_execution(stack: &LexerModeStack) -> bool {
    in_modes(stack, &[Mode::TemplateExecution])
}

// @lfy def/lexer/modes.lfy:40
pub fn in_list(stack: &LexerModeStack) -> bool {
    in_modes(stack, &[Mode::List])
}

// @lfy def/lexer/modes.lfy:41
pub fn in_block(stack: &LexerModeStack) -> bool {
    in_modes(stack, &[Mode::Block])
}

// @lfy def/lexer/modes.lfy:42
pub fn in_group(stack: &LexerModeStack) -> bool {
    in_modes(stack, &[Mode::Group])
}

/// The modes in which the default is that all text counts as content body.
// @lfy def/lexer/modes.lfy:44
pub const CONTENT_BODY_MODES: &[Mode] = &[
    Mode::Documentation,
    Mode::Comment,
    Mode::StringLiteral,
    Mode::TemplateLiteral,
];

/// In a mode where the default is that all text counts as content body.
// @lfy def/lexer/modes.lfy:44
pub fn in_content_body_mode(stack: &LexerModeStack) -> bool {
    in_modes(stack, CONTENT_BODY_MODES)
}

/// Not in a mode where the default is that all text counts as content body.
// @lfy def/lexer/modes.lfy:45
pub fn not_in_content_body_mode(stack: &LexerModeStack) -> bool {
    not_in_modes(stack, CONTENT_BODY_MODES)
}

// Mode logic

const NOT_IN_DOCUMENTATION_STRING_OR_TEMPLATE: Condition = Condition::NotInModes(&[
    Mode::Documentation,
    Mode::StringLiteral,
    Mode::TemplateLiteral,
]);
const NOT_IN_COMMENT_STRING_OR_TEMPLATE: Condition =
    Condition::NotInModes(&[Mode::Comment, Mode::StringLiteral, Mode::TemplateLiteral]);
const NOT_IN_DOCUMENTATION_COMMENT_OR_TEMPLATE: Condition =
    Condition::NotInModes(&[Mode::Documentation, Mode::Comment, Mode::TemplateLiteral]);
const NOT_IN_CONTENT_BODY_MODE: Condition = Condition::NotInModes(CONTENT_BODY_MODES);

/// The mode traits applied to each kind of token.
// @lfy def/lexer/modes.lfy:48
pub fn mode_behaviors(kind: TokenKind) -> &'static [ModeBehavior] {
    match kind {
        TokenKind::Comment(Comment::CommentBlockOpen) => &[ModeBehavior::Creator {
            mode: Mode::Comment,
            condition: NOT_IN_DOCUMENTATION_STRING_OR_TEMPLATE,
        }], // @lfy def/lexer/modes.lfy:48
        TokenKind::Comment(Comment::CommentInlineStart) => &[
            ModeBehavior::Creator {
                mode: Mode::Comment,
                condition: NOT_IN_DOCUMENTATION_STRING_OR_TEMPLATE,
            }, // @lfy def/lexer/modes.lfy:49
            ModeBehavior::ClearAtNewLine {
                mode: Mode::Comment,
            }, // @lfy def/lexer/modes.lfy:51
        ],
        TokenKind::Comment(Comment::CommentBlockClose) => &[ModeBehavior::Destroyer {
            mode: Mode::Comment,
            condition: Condition::Always,
        }], // @lfy def/lexer/modes.lfy:50
        TokenKind::Comment(Comment::DocumentationBlockOpen) => &[ModeBehavior::Creator {
            mode: Mode::Documentation,
            condition: NOT_IN_COMMENT_STRING_OR_TEMPLATE,
        }], // @lfy def/lexer/modes.lfy:53
        TokenKind::Comment(Comment::DocumentationInlineStart) => &[
            ModeBehavior::Creator {
                mode: Mode::Documentation,
                condition: NOT_IN_COMMENT_STRING_OR_TEMPLATE,
            }, // @lfy def/lexer/modes.lfy:54
            ModeBehavior::ClearAtNewLine {
                mode: Mode::Documentation,
            }, // @lfy def/lexer/modes.lfy:56
        ],
        TokenKind::Comment(Comment::DocumentationBlockClose) => &[ModeBehavior::Destroyer {
            mode: Mode::Documentation,
            condition: Condition::Always,
        }], // @lfy def/lexer/modes.lfy:55
        TokenKind::Literal(Literal::SingleQuoteBoundary) => &[
            ModeBehavior::Boundary {
                mode: Mode::StringLiteral,
                condition: NOT_IN_DOCUMENTATION_COMMENT_OR_TEMPLATE,
            }, // @lfy def/lexer/modes.lfy:58
            // Bail out of single quote literal and let parser throw error in cases of no
            // closing string boundary.
            ModeBehavior::ClearAtNewLine {
                mode: Mode::StringLiteral,
            }, // @lfy def/lexer/modes.lfy:61
        ],
        TokenKind::Literal(Literal::DoubleQuoteBoundary) => &[ModeBehavior::Boundary {
            mode: Mode::StringLiteral,
            condition: NOT_IN_DOCUMENTATION_COMMENT_OR_TEMPLATE,
        }], // @lfy def/lexer/modes.lfy:59
        TokenKind::Literal(Literal::BacktickBoundary) => &[ModeBehavior::Boundary {
            mode: Mode::TemplateLiteral,
            condition: NOT_IN_DOCUMENTATION_STRING_OR_TEMPLATE,
        }], // @lfy def/lexer/modes.lfy:63
        TokenKind::Literal(Literal::ReferenceOpenBoundary) => &[ModeBehavior::Creator {
            mode: Mode::TemplateReference,
            condition: Condition::InModes(&[Mode::TemplateLiteral, Mode::Documentation]),
        }], // @lfy def/lexer/modes.lfy:64
        TokenKind::Literal(Literal::ReferenceCloseBoundary) => &[ModeBehavior::Destroyer {
            mode: Mode::TemplateReference,
            condition: Condition::InModes(&[Mode::TemplateReference]),
        }], // @lfy def/lexer/modes.lfy:65
        TokenKind::Literal(Literal::ExecutionOpenBoundary) => &[ModeBehavior::Creator {
            mode: Mode::TemplateExecution,
            condition: Condition::InModes(&[Mode::TemplateLiteral]),
        }], // @lfy def/lexer/modes.lfy:66
        TokenKind::Literal(Literal::ExecutionCloseBoundary) => &[ModeBehavior::Destroyer {
            mode: Mode::TemplateExecution,
            condition: Condition::InModes(&[Mode::TemplateExecution]),
        }], // @lfy def/lexer/modes.lfy:67
        TokenKind::Separator(Separator::ListOpenSeparator) => &[ModeBehavior::Creator {
            mode: Mode::List,
            condition: NOT_IN_CONTENT_BODY_MODE,
        }], // @lfy def/lexer/modes.lfy:69
        TokenKind::Separator(Separator::ListCloseSeparator) => &[ModeBehavior::Destroyer {
            mode: Mode::List,
            condition: Condition::Always,
        }], // @lfy def/lexer/modes.lfy:70
        TokenKind::Separator(Separator::BlockOpenSeparator) => &[ModeBehavior::Creator {
            mode: Mode::Block,
            condition: NOT_IN_CONTENT_BODY_MODE,
        }], // @lfy def/lexer/modes.lfy:71
        TokenKind::Separator(Separator::BlockCloseSeparator) => &[ModeBehavior::Destroyer {
            mode: Mode::Block,
            condition: Condition::Always,
        }], // @lfy def/lexer/modes.lfy:72
        TokenKind::Separator(Separator::GroupOpenSeparator) => &[ModeBehavior::Creator {
            mode: Mode::Group,
            condition: NOT_IN_CONTENT_BODY_MODE,
        }], // @lfy def/lexer/modes.lfy:73
        TokenKind::Separator(Separator::GroupCloseSeparator) => &[ModeBehavior::Destroyer {
            mode: Mode::Group,
            condition: Condition::Always,
        }], // @lfy def/lexer/modes.lfy:74
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(literal: Literal) -> TokenKind {
        TokenKind::Literal(literal)
    }

    #[test]
    fn mode_names_match_their_declarations() {
        assert_eq!(Mode::Comment.name(), "mode::comment");
        assert_eq!(Mode::Documentation.name(), "mode::documentation");
        assert_eq!(Mode::StringLiteral.name(), "mode::literal::string");
        assert_eq!(Mode::TemplateLiteral.name(), "mode::literal::template");
        assert_eq!(Mode::TemplateReference.name(), "mode::template::reference");
        assert_eq!(Mode::TemplateExecution.name(), "mode::template::execution");
        assert_eq!(Mode::List.name(), "mode::list");
        assert_eq!(Mode::Block.name(), "mode::block");
        assert_eq!(Mode::Group.to_string(), "mode::group");
    }

    // @lfy def/lexer/modes.lfy:14
    #[test]
    fn template_block_modes_are_only_allowed_where_declared() {
        let mut stack = LexerModeStack::new();
        assert!(!Mode::TemplateReference.is_allowed(&stack));
        assert!(!Mode::TemplateExecution.is_allowed(&stack));
        assert!(Mode::StringLiteral.is_allowed(&stack));
        stack.push(
            Mode::Documentation,
            TokenKind::Comment(Comment::DocumentationInlineStart),
        );
        assert!(Mode::TemplateReference.is_allowed(&stack));
        assert!(!Mode::TemplateExecution.is_allowed(&stack));
        stack.push(Mode::TemplateLiteral, lit(Literal::BacktickBoundary));
        assert!(Mode::TemplateReference.is_allowed(&stack));
        assert!(Mode::TemplateExecution.is_allowed(&stack));
    }

    // @lfy def/lexer/modes.lfy:26
    #[test]
    fn predicates_follow_the_top_of_the_stack_and_its_creator() {
        let mut stack = LexerModeStack::new();
        assert!(not_in_content_body_mode(&stack) && !in_comment(&stack));

        stack.push(Mode::Comment, TokenKind::Comment(Comment::CommentBlockOpen));
        assert!(in_comment(&stack) && in_block_comment(&stack) && !in_inline_comment(&stack));
        assert!(in_content_body_mode(&stack) && !in_documentation(&stack));

        stack.push(
            Mode::Comment,
            TokenKind::Comment(Comment::CommentInlineStart),
        );
        assert!(in_inline_comment(&stack) && !in_block_comment(&stack));

        stack.push(
            Mode::Documentation,
            TokenKind::Comment(Comment::DocumentationBlockOpen),
        );
        assert!(in_documentation(&stack) && in_block_documentation(&stack));
        assert!(!in_inline_documentation(&stack) && !in_comment(&stack));

        stack.push(
            Mode::Documentation,
            TokenKind::Comment(Comment::DocumentationInlineStart),
        );
        assert!(in_inline_documentation(&stack) && !in_block_documentation(&stack));

        stack.push(Mode::StringLiteral, lit(Literal::SingleQuoteBoundary));
        assert!(
            in_string(&stack) && in_single_quote_string(&stack) && !in_double_quote_string(&stack)
        );
        stack.push(Mode::StringLiteral, lit(Literal::DoubleQuoteBoundary));
        assert!(in_double_quote_string(&stack) && !in_single_quote_string(&stack));

        stack.push(Mode::TemplateLiteral, lit(Literal::BacktickBoundary));
        assert!(in_template(&stack) && !in_string(&stack) && in_content_body_mode(&stack));
        stack.push(Mode::TemplateReference, lit(Literal::ReferenceOpenBoundary));
        assert!(in_template_reference(&stack) && !in_template(&stack));
        assert!(not_in_content_body_mode(&stack));
        stack.push(Mode::TemplateExecution, lit(Literal::ExecutionOpenBoundary));
        assert!(in_template_execution(&stack) && !in_template_reference(&stack));

        stack.push(
            Mode::List,
            TokenKind::Separator(Separator::ListOpenSeparator),
        );
        assert!(in_list(&stack) && !in_block(&stack) && !in_group(&stack));
        stack.push(
            Mode::Block,
            TokenKind::Separator(Separator::BlockOpenSeparator),
        );
        assert!(in_block(&stack));
        stack.push(
            Mode::Group,
            TokenKind::Separator(Separator::GroupOpenSeparator),
        );
        assert!(in_group(&stack) && !in_block(&stack));
    }

    // @lfy def/lexer/modes.lfy:48
    #[test]
    fn only_delimiters_boundaries_and_brackets_carry_mode_behaviors() {
        assert_eq!(mode_behaviors(lit(Literal::SingleQuoteBoundary)).len(), 2);
        assert_eq!(mode_behaviors(lit(Literal::DoubleQuoteBoundary)).len(), 1);
        assert_eq!(
            mode_behaviors(TokenKind::Comment(Comment::CommentInlineStart)).len(),
            2
        );
        assert_eq!(
            mode_behaviors(TokenKind::Comment(Comment::CommentBlockBody)).len(),
            0
        );
        assert!(mode_behaviors(TokenKind::Separator(Separator::ListContinueSeparator)).is_empty());
        assert!(mode_behaviors(TokenKind::Space).is_empty());
        assert!(mode_behaviors(TokenKind::Identifier).is_empty());
        assert!(mode_behaviors(TokenKind::Invalid).is_empty());
        assert!(mode_behaviors(lit(Literal::NullLiteral)).is_empty());
    }
}
