//! Compiled from `def/lexer/tokens/comment.lfy`.

use super::super::data::{LexerModeStack, Mode, TokenKind};
use super::token_enum;
use super::traits::{FollowRule, ModeBehavior};

fn top_is(stack: &LexerModeStack, mode: Mode, creator: Comment) -> bool {
    stack
        .top()
        .is_some_and(|entry| entry.mode == mode && entry.creator == TokenKind::Comment(creator))
}

/// `CommentMode` is the top mode on the stack.
// @lfy def/lexer/tokens/comment.lfy:8
pub fn in_comment(stack: &LexerModeStack) -> bool {
    stack.top_mode() == Some(Mode::Comment)
}

/// `CommentMode` is on top and was pushed when an inline comment token was created.
// @lfy def/lexer/tokens/comment.lfy:9
pub fn in_inline_comment(stack: &LexerModeStack) -> bool {
    top_is(stack, Mode::Comment, Comment::InlineStart)
}

/// `CommentMode` is on top and was pushed when a block comment start token was created.
// @lfy def/lexer/tokens/comment.lfy:10
pub fn in_block_comment(stack: &LexerModeStack) -> bool {
    top_is(stack, Mode::Comment, Comment::BlockOpen)
}

/// `DocumentationMode` is the top mode on the stack.
// @lfy def/lexer/tokens/comment.lfy:11
pub fn in_documentation(stack: &LexerModeStack) -> bool {
    stack.top_mode() == Some(Mode::Documentation)
}

/// `DocumentationMode` is on top and was pushed when an inline documentation token was created.
// @lfy def/lexer/tokens/comment.lfy:12
pub fn in_inline_documentation(stack: &LexerModeStack) -> bool {
    top_is(stack, Mode::Documentation, Comment::DocInlineStart)
}

/// `DocumentationMode` is on top and was pushed when a block documentation start token was created.
// @lfy def/lexer/tokens/comment.lfy:13
pub fn in_block_documentation(stack: &LexerModeStack) -> bool {
    top_is(stack, Mode::Documentation, Comment::DocBlockOpen)
}

token_enum! {
    /// Comment and documentation delimiters.
    // @lfy def/lexer/tokens/comment.lfy:15
    pub enum Comment {
        BlockOpen => ("CM_BLOCK_OPEN", "/*", "a block comment start [[Token]]"), // @lfy def/lexer/tokens/comment.lfy:16
        BlockClose => ("CM_BLOCK_CLOSE", "*/", "a block comment end [[Token]]"), // @lfy def/lexer/tokens/comment.lfy:19
        InlineStart => ("CM_INLINE_START", "//", "an inline comment [[Token]]"), // @lfy def/lexer/tokens/comment.lfy:22
        DocBlockOpen => ("CM_DOC_BLOCK_OPEN", "/**", "a block documentation start [[Token]]"), // @lfy def/lexer/tokens/comment.lfy:25
        DocBlockClose => ("CM_DOC_BLOCK_CLOSE", "**/", "a block documentation end [[Token]]"), // @lfy def/lexer/tokens/comment.lfy:28
        DocInlineStart => ("CM_DOC_INLINE_START", "///", "an inline documentation [[Token]]"), // @lfy def/lexer/tokens/comment.lfy:31
    }
}

const BLOCK_OPEN: &[ModeBehavior] = &[ModeBehavior::Creator {
    mode: Mode::Comment,
    condition: None,
}]; // @lfy def/lexer/tokens/comment.lfy:16
const BLOCK_CLOSE: &[ModeBehavior] = &[ModeBehavior::Destroyer {
    mode: Mode::Comment,
    condition: None,
}]; // @lfy def/lexer/tokens/comment.lfy:19
const INLINE_START: &[ModeBehavior] = &[
    // @lfy def/lexer/tokens/comment.lfy:22
    ModeBehavior::Creator {
        mode: Mode::Comment,
        condition: None,
    },
    ModeBehavior::ClearAtNewLine {
        mode: Mode::Comment,
    },
];
const DOC_BLOCK_OPEN: &[ModeBehavior] = &[ModeBehavior::Creator {
    mode: Mode::Documentation,
    condition: None,
}]; // @lfy def/lexer/tokens/comment.lfy:25
const DOC_BLOCK_OPEN_CANT_BE_FOLLOWED_BY: &[FollowRule] = &[FollowRule::Literal("/")]; // @lfy def/lexer/tokens/comment.lfy:25
const DOC_BLOCK_CLOSE: &[ModeBehavior] = &[ModeBehavior::Destroyer {
    mode: Mode::Documentation,
    condition: None,
}]; // @lfy def/lexer/tokens/comment.lfy:28
const DOC_INLINE_START: &[ModeBehavior] = &[
    // @lfy def/lexer/tokens/comment.lfy:31
    ModeBehavior::Creator {
        mode: Mode::Documentation,
        condition: None,
    },
    ModeBehavior::ClearAtNewLine {
        mode: Mode::Documentation,
    },
];

impl Comment {
    /// The mode traits attached to this delimiter.
    pub fn mode_behaviors(self) -> &'static [ModeBehavior] {
        match self {
            Comment::BlockOpen => BLOCK_OPEN,
            Comment::BlockClose => BLOCK_CLOSE,
            Comment::InlineStart => INLINE_START,
            Comment::DocBlockOpen => DOC_BLOCK_OPEN,
            Comment::DocBlockClose => DOC_BLOCK_CLOSE,
            Comment::DocInlineStart => DOC_INLINE_START,
        }
    }

    /// The `cantBeFollowedBy` rules attached to this delimiter.
    pub fn cant_be_followed_by(self) -> &'static [FollowRule] {
        match self {
            Comment::DocBlockOpen => DOC_BLOCK_OPEN_CANT_BE_FOLLOWED_BY, // @lfy def/lexer/tokens/comment.lfy:25
            _ => &[],
        }
    }

    /// `key.includes('INLINE')`, folded at compile time.
    // @lfy def/lexer/main.lfy:64
    pub fn is_inline(self) -> bool {
        matches!(self, Comment::InlineStart | Comment::DocInlineStart)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/lexer/tokens/comment.lfy:8
    #[test]
    fn comment_predicates_distinguish_inline_and_block_creators() {
        let mut stack = LexerModeStack::new();
        assert!(!in_comment(&stack) && !in_documentation(&stack));

        stack.push(Mode::Comment, TokenKind::Comment(Comment::BlockOpen));
        assert!(in_comment(&stack) && in_block_comment(&stack) && !in_inline_comment(&stack));
        assert!(!in_documentation(&stack));

        stack.push(Mode::Comment, TokenKind::Comment(Comment::InlineStart));
        assert!(in_comment(&stack) && in_inline_comment(&stack) && !in_block_comment(&stack));

        stack.push(
            Mode::Documentation,
            TokenKind::Comment(Comment::DocBlockOpen),
        );
        assert!(in_documentation(&stack) && in_block_documentation(&stack));
        assert!(!in_inline_documentation(&stack) && !in_comment(&stack));

        stack.push(
            Mode::Documentation,
            TokenKind::Comment(Comment::DocInlineStart),
        );
        assert!(in_inline_documentation(&stack) && !in_block_documentation(&stack));
    }

    // @lfy def/lexer/tokens/comment.lfy:15
    #[test]
    fn delimiters_carry_their_traits() {
        assert!(matches!(
            Comment::BlockOpen.mode_behaviors(),
            [ModeBehavior::Creator {
                mode: Mode::Comment,
                condition: None
            }]
        ));
        assert!(matches!(
            Comment::BlockClose.mode_behaviors(),
            [ModeBehavior::Destroyer {
                mode: Mode::Comment,
                condition: None
            }]
        ));
        assert!(matches!(
            Comment::InlineStart.mode_behaviors(),
            [
                ModeBehavior::Creator {
                    mode: Mode::Comment,
                    condition: None
                },
                ModeBehavior::ClearAtNewLine {
                    mode: Mode::Comment
                }
            ]
        ));
        assert!(matches!(
            Comment::DocBlockOpen.mode_behaviors(),
            [ModeBehavior::Creator {
                mode: Mode::Documentation,
                condition: None
            }]
        ));
        assert_eq!(
            Comment::DocBlockOpen.cant_be_followed_by(),
            &[FollowRule::Literal("/")]
        );
        assert!(matches!(
            Comment::DocBlockClose.mode_behaviors(),
            [ModeBehavior::Destroyer {
                mode: Mode::Documentation,
                condition: None
            }]
        ));
        assert!(matches!(
            Comment::DocInlineStart.mode_behaviors(),
            [
                ModeBehavior::Creator {
                    mode: Mode::Documentation,
                    condition: None
                },
                ModeBehavior::ClearAtNewLine {
                    mode: Mode::Documentation
                }
            ]
        ));
        for &comment in Comment::ALL {
            assert_eq!(comment.is_inline(), comment.key().contains("INLINE"));
        }
    }
}
