//! Compiled from `def/lexer/data.lfy`.

use std::fmt;
use std::sync::Arc;

use crate::grammar::RuleInfo;
use crate::grammar::expression::Expression;
use crate::grammar::tokens::comment::Comment;
use crate::grammar::tokens::identifier::Identifier;
use crate::grammar::tokens::keyword::Keyword;
use crate::grammar::tokens::literal::Literal;
use crate::grammar::tokens::operator::Operator;
use crate::grammar::tokens::separator::Separator;

use super::modes::Mode;

/// One entry on the [`LexerModeStack`]: the mode plus the kind of token whose creation
/// pushed it. The creator is the information needed to lex mode-dependent tokens (inline
/// vs. block comments, which boundary opened a string).
// @lfy def/lexer/data.lfy:5
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeEntry {
    pub mode: Mode,
    pub creator: TokenKind,
}

/// Stack of the current mode the lexer should be in based on tokens.
// @lfy def/lexer/data.lfy:1
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LexerModeStack {
    entries: Vec<ModeEntry>,
}

impl LexerModeStack {
    /// A new, empty stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Entering a lexing mode pushes it to the top of the stack.
    // @lfy def/lexer/data.lfy:3
    pub fn push(&mut self, mode: Mode, creator: TokenKind) {
        self.entries.push(ModeEntry { mode, creator });
    }

    /// Exiting the lexing mode at the top of the stack pops a single mode. Nothing is
    /// popped when `mode` is not on top; the popped entry is returned otherwise.
    // @lfy def/lexer/data.lfy:4
    pub fn pop(&mut self, mode: Mode) -> Option<ModeEntry> {
        match self.entries.last() {
            Some(top) if top.mode == mode => self.entries.pop(),
            _ => None,
        }
    }

    /// The entry at the top of the stack, carrying the mode and the token that created it.
    // @lfy def/lexer/data.lfy:5
    pub fn top(&self) -> Option<&ModeEntry> {
        self.entries.last()
    }

    /// The mode at the top of the stack, if any.
    pub fn top_mode(&self) -> Option<Mode> {
        self.entries.last().map(|entry| entry.mode)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// All modes on the stack, bottom first.
    pub fn modes(&self) -> Vec<Mode> {
        self.entries.iter().map(|entry| entry.mode).collect()
    }
}

/// The kind of a lexed [`Token`]: the grammar rule it was pushed to the results as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// A literal token: a simple literal, a string or template boundary, or a string body.
    Literal(Literal), // @lfy def/lexer/main.lfy:62
    /// A template literal body (`Expression.TemplateLiteralBody`).
    Expression(Expression), // @lfy def/lexer/main.lfy:76
    /// A comment or documentation delimiter or body.
    Comment(Comment), // @lfy def/lexer/main.lfy:80
    /// A keyword token.
    Keyword(Keyword), // @lfy def/lexer/main.lfy:95
    /// An operator token.
    Operator(Operator), // @lfy def/lexer/main.lfy:100
    /// A separator token.
    Separator(Separator), // @lfy def/lexer/main.lfy:105
    /// A space token.
    Space, // @lfy def/lexer/main.lfy:110
    /// An identifier token.
    Identifier, // @lfy def/lexer/main.lfy:114
    /// An invalid token: characters no other rule matched.
    Invalid, // @lfy def/lexer/main.lfy:116
}

impl TokenKind {
    /// The grammar rule this kind of token was lexed from. Space tokens come from either
    /// of the two space rules and invalid tokens from none, so both yield `None`.
    pub fn rule(self) -> Option<RuleInfo> {
        match self {
            TokenKind::Literal(rule) => Some(RuleInfo::of(rule)),
            TokenKind::Expression(rule) => Some(RuleInfo::of(rule)),
            TokenKind::Comment(rule) => Some(RuleInfo::of(rule)),
            TokenKind::Keyword(rule) => Some(RuleInfo::of(rule)),
            TokenKind::Operator(rule) => Some(RuleInfo::of(rule)),
            TokenKind::Separator(rule) => Some(RuleInfo::of(rule)),
            TokenKind::Identifier => Some(RuleInfo::of(Identifier::Identifier)),
            TokenKind::Space | TokenKind::Invalid => None,
        }
    }
}

/// Representation of a lexed token.
// @lfy def/lexer/data.lfy:8
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// Normalized and processed token value as derived from the raw source text.
    pub value: String, // @lfy def/lexer/data.lfy:9
    /// Raw source text content that created this token.
    pub raw: String, // @lfy def/lexer/data.lfy:10
    /// Source file this token came from (`"anonymous"` when none was given).
    pub file: Arc<str>, // @lfy def/lexer/data.lfy:13
    /// 1-indexed line the token starts on.
    pub line: usize, // @lfy def/lexer/data.lfy:13
    /// 0-indexed character position within the line the token starts at.
    pub position: usize, // @lfy def/lexer/data.lfy:13
}

impl Token {
    /// A space token only separates tokens and carries no other semantic significance.
    // @lfy def/lexer/data.lfy:16
    pub fn is_space(&self) -> bool {
        self.kind == TokenKind::Space
    }
}

/// Errors raised while lexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// The input ended while the mode stack was not empty.
    // @lfy def/lexer/main.lfy:37
    UnexpectedEndOfFile {
        file: Arc<str>,
        line: usize,
        position: usize,
        /// Modes still open, bottom first.
        open: Vec<Mode>,
    },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::UnexpectedEndOfFile {
                file,
                line,
                position,
                open,
            } => {
                write!(
                    f,
                    "{file}:{line}:{position}: unexpected end of file; open modes: ["
                )?;
                for (index, mode) in open.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    f.write_str(mode.name())?;
                }
                f.write_str("]")
            }
        }
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_open() -> TokenKind {
        TokenKind::Separator(Separator::BlockOpenSeparator)
    }

    fn group_open() -> TokenKind {
        TokenKind::Separator(Separator::GroupOpenSeparator)
    }

    // @lfy def/lexer/data.lfy:3
    #[test]
    fn entering_a_mode_pushes_it_to_the_top() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Block, block_open());
        stack.push(Mode::Group, group_open());
        assert_eq!(stack.top_mode(), Some(Mode::Group));
        assert_eq!(stack.len(), 2);
    }

    // @lfy def/lexer/data.lfy:4
    #[test]
    fn exiting_the_top_mode_pops_a_single_entry() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Block, block_open());
        stack.push(Mode::Group, group_open());
        assert_eq!(stack.pop(Mode::Block), None);
        assert_eq!(stack.len(), 2);
        let popped = stack.pop(Mode::Group).unwrap();
        assert_eq!(popped.mode, Mode::Group);
        assert_eq!(stack.modes(), vec![Mode::Block]);
        assert!(LexerModeStack::new().pop(Mode::List).is_none());
    }

    // @lfy def/lexer/data.lfy:5
    #[test]
    fn stack_entries_carry_the_creating_token() {
        let mut stack = LexerModeStack::new();
        stack.push(
            Mode::Comment,
            TokenKind::Comment(Comment::CommentInlineStart),
        );
        let top = stack.top().unwrap();
        assert_eq!(top.mode, Mode::Comment);
        assert_eq!(top.creator, TokenKind::Comment(Comment::CommentInlineStart));
        assert!(!stack.is_empty());
    }

    // @lfy def/lexer/data.lfy:13
    #[test]
    fn token_tracks_file_line_and_position() {
        let token = Token {
            kind: TokenKind::Identifier,
            value: "x".into(),
            raw: "x".into(),
            file: Arc::from("main.lfy"),
            line: 3,
            position: 7,
        };
        assert_eq!(&*token.file, "main.lfy");
        assert_eq!((token.line, token.position), (3, 7));
        assert_eq!(token.kind.rule().unwrap().identifier(), "Identifier");
    }

    // @lfy def/lexer/data.lfy:16
    #[test]
    fn space_tokens_are_identified_as_separators_only() {
        let space = Token {
            kind: TokenKind::Space,
            value: " ".into(),
            raw: " ".into(),
            file: Arc::from("anonymous"),
            line: 1,
            position: 0,
        };
        assert!(space.is_space());
        assert!(space.kind.rule().is_none());
        let ident = Token {
            kind: TokenKind::Identifier,
            ..space.clone()
        };
        assert!(!ident.is_space());
    }

    #[test]
    fn errors_display_their_location_and_open_modes() {
        let error = LexError::UnexpectedEndOfFile {
            file: Arc::from("a.lfy"),
            line: 2,
            position: 1,
            open: vec![Mode::Group, Mode::List],
        };
        assert_eq!(
            error.to_string(),
            "a.lfy:2:1: unexpected end of file; open modes: [mode::group, mode::list]"
        );
    }
}
