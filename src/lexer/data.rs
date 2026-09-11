//! Compiled from `def/lexer/data.lfy` (`LexerModeStack`, `Token`), together with
//! the lexer-mode declarations from `def/lexer/tokens/{literal,comment,separator}.lfy`.

use std::fmt;
use std::sync::Arc;

use super::tokens::comment::Comment;
use super::tokens::keyword::Keyword;
use super::tokens::literal::{Literal, TemplateBlock};
use super::tokens::operator::Operator;
use super::tokens::separator::Separator;
use super::tokens::whitespace::Space;

/// A lexing mode. Each variant is one of the `mode::*` string constants declared in the
/// token definition files; `name()` yields the original string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    StringLiteral,     // @lfy def/lexer/tokens/literal.lfy:3
    TemplateLiteral,   // @lfy def/lexer/tokens/literal.lfy:4
    TemplateReference, // @lfy def/lexer/tokens/literal.lfy:5
    TemplateExecution, // @lfy def/lexer/tokens/literal.lfy:6
    Comment,           // @lfy def/lexer/tokens/comment.lfy:5
    Documentation,     // @lfy def/lexer/tokens/comment.lfy:6
    List,              // @lfy def/lexer/tokens/separator.lfy:3
    Block,             // @lfy def/lexer/tokens/separator.lfy:4
    Group,             // @lfy def/lexer/tokens/separator.lfy:5
}

impl Mode {
    /// The `mode::*` identifier this mode was declared with.
    pub fn name(self) -> &'static str {
        match self {
            Mode::StringLiteral => "mode::literal::string", // @lfy def/lexer/tokens/literal.lfy:3
            Mode::TemplateLiteral => "mode::literal::template", // @lfy def/lexer/tokens/literal.lfy:4
            Mode::TemplateReference => "mode::template::reference", // @lfy def/lexer/tokens/literal.lfy:5
            Mode::TemplateExecution => "mode::template::execution", // @lfy def/lexer/tokens/literal.lfy:6
            Mode::Comment => "mode::comment", // @lfy def/lexer/tokens/comment.lfy:5
            Mode::Documentation => "mode::documentation", // @lfy def/lexer/tokens/comment.lfy:6
            Mode::List => "mode::list",       // @lfy def/lexer/tokens/separator.lfy:3
            Mode::Block => "mode::block",     // @lfy def/lexer/tokens/separator.lfy:4
            Mode::Group => "mode::group",     // @lfy def/lexer/tokens/separator.lfy:5
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One entry on the [`LexerModeStack`]: the mode plus the token kind whose creation
/// pushed it. The creator is the "information needed to be able to successfully lex"
/// mode-dependent tokens (inline vs. block comments, matching string boundaries).
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
    /// A new, empty stack (one is initialized for every `lex_string` run).
    pub fn new() -> Self {
        Self::default()
    }

    /// Entering a lexing mode pushes it to the top of the stack.
    // @lfy def/lexer/data.lfy:3
    pub fn push(&mut self, mode: Mode, creator: TokenKind) {
        self.entries.push(ModeEntry { mode, creator });
    }

    /// Exiting the mode at the top of the stack pops exactly one entry; exiting a mode that
    /// is not at the top is an `InvalidModePop` error.
    // @lfy def/lexer/data.lfy:4
    pub fn pop(&mut self, mode: Mode) -> Result<ModeEntry, LexError> {
        match self.entries.last() {
            Some(top) if top.mode == mode => Ok(self.entries.pop().expect("top exists")),
            top => Err(LexError::InvalidModePop {
                // @lfy def/lexer/data.lfy:5
                expected: mode,
                found: top.map(|entry| entry.mode),
            }),
        }
    }

    /// The entry at the top of the stack, carrying the mode and its creating token.
    // @lfy def/lexer/data.lfy:6
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

/// The kind of a lexed [`Token`]. Enumerated kinds carry the enum member they were
/// matched from; the remaining kinds are produced by the free-form lexing rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Literal(Literal),             // @lfy def/lexer/main.lfy:31
    TemplateBlock(TemplateBlock), // @lfy def/lexer/main.lfy:34
    Number,                       // @lfy def/lexer/main.lfy:45
    Text,                         // @lfy def/lexer/main.lfy:51
    Comment(Comment),             // @lfy def/lexer/main.lfy:66
    CommentBody,                  // @lfy def/lexer/main.lfy:69
    DocumentationBody,            // @lfy def/lexer/main.lfy:70
    Keyword(Keyword),             // @lfy def/lexer/main.lfy:74
    Operator(Operator),           // @lfy def/lexer/main.lfy:79
    Separator(Separator),         // @lfy def/lexer/main.lfy:84
    Space(Space),                 // @lfy def/lexer/main.lfy:89
    Identifier,                   // @lfy def/lexer/main.lfy:99
}

/// Representation of a lexed token.
// @lfy def/lexer/data.lfy:9
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// Normalized and processed token value as derived from the raw source text.
    pub value: String, // @lfy def/lexer/data.lfy:10
    /// Raw source text content that created this token.
    pub raw: String, // @lfy def/lexer/data.lfy:11
    /// Source file this token came from (`"anonymous"` when none was given).
    pub file: Arc<str>, // @lfy def/lexer/data.lfy:14
    /// 1-indexed line the token starts on.
    pub line: usize, // @lfy def/lexer/data.lfy:14
    /// 0-indexed character position within the line the token starts at.
    pub position: usize, // @lfy def/lexer/data.lfy:14
}

impl Token {
    /// A space token only separates tokens and carries no other semantic significance.
    // @lfy def/lexer/data.lfy:16
    pub fn is_space(&self) -> bool {
        matches!(self.kind, TokenKind::Space(_))
    }
}

/// Errors raised while lexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// A character matched no lexing rule.
    // @lfy def/lexer/main.lfy:101
    UnexpectedToken {
        file: Arc<str>,
        line: usize,
        position: usize,
        found: char,
    },
    /// The input ended while the mode stack was not empty.
    // @lfy def/lexer/main.lfy:23
    UnexpectedEndOfFile {
        file: Arc<str>,
        line: usize,
        position: usize,
        /// Modes still open, bottom first.
        open: Vec<Mode>,
    },
    /// A mode was exited that was not at the top of the stack.
    // @lfy def/lexer/data.lfy:5
    InvalidModePop { expected: Mode, found: Option<Mode> },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::UnexpectedToken {
                file,
                line,
                position,
                found,
            } => {
                write!(f, "{file}:{line}:{position}: unexpected token {found:?}")
            }
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
            LexError::InvalidModePop { expected, found } => match found {
                Some(found) => write!(
                    f,
                    "invalid mode pop: expected {expected} at top of stack, found {found}"
                ),
                None => write!(
                    f,
                    "invalid mode pop: expected {expected} at top of stack, stack is empty"
                ),
            },
        }
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/lexer/data.lfy:3
    #[test]
    fn entering_a_mode_pushes_it_to_the_top() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Block, TokenKind::Separator(Separator::BlockOpen));
        stack.push(Mode::Group, TokenKind::Separator(Separator::GroupOpen));
        assert_eq!(stack.top_mode(), Some(Mode::Group));
        assert_eq!(stack.len(), 2);
    }

    // @lfy def/lexer/data.lfy:4
    #[test]
    fn exiting_the_top_mode_pops_a_single_entry() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Block, TokenKind::Separator(Separator::BlockOpen));
        stack.push(Mode::Group, TokenKind::Separator(Separator::GroupOpen));
        let popped = stack.pop(Mode::Group).unwrap();
        assert_eq!(popped.mode, Mode::Group);
        assert_eq!(stack.modes(), vec![Mode::Block]);
    }

    // @lfy def/lexer/data.lfy:5
    #[test]
    fn exiting_a_mode_not_at_the_top_is_an_invalid_mode_pop() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Block, TokenKind::Separator(Separator::BlockOpen));
        stack.push(Mode::Group, TokenKind::Separator(Separator::GroupOpen));
        assert_eq!(
            stack.pop(Mode::Block),
            Err(LexError::InvalidModePop {
                expected: Mode::Block,
                found: Some(Mode::Group)
            })
        );
        assert_eq!(stack.len(), 2);
        assert_eq!(
            LexerModeStack::new().pop(Mode::List),
            Err(LexError::InvalidModePop {
                expected: Mode::List,
                found: None
            })
        );
    }

    // @lfy def/lexer/data.lfy:6
    #[test]
    fn stack_entries_carry_the_creating_token() {
        let mut stack = LexerModeStack::new();
        stack.push(Mode::Comment, TokenKind::Comment(Comment::InlineStart));
        let top = stack.top().unwrap();
        assert_eq!(top.mode, Mode::Comment);
        assert_eq!(top.creator, TokenKind::Comment(Comment::InlineStart));
    }

    // @lfy def/lexer/data.lfy:14
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
    }

    // @lfy def/lexer/data.lfy:16
    #[test]
    fn space_tokens_are_identified_as_separators_only() {
        let space = Token {
            kind: TokenKind::Space(Space::Space),
            value: " ".into(),
            raw: " ".into(),
            file: Arc::from("anonymous"),
            line: 1,
            position: 0,
        };
        assert!(space.is_space());
        let ident = Token {
            kind: TokenKind::Identifier,
            ..space.clone()
        };
        assert!(!ident.is_space());
    }

    #[test]
    fn mode_names_match_their_declarations() {
        assert_eq!(Mode::StringLiteral.name(), "mode::literal::string");
        assert_eq!(Mode::TemplateLiteral.name(), "mode::literal::template");
        assert_eq!(Mode::TemplateReference.name(), "mode::template::reference");
        assert_eq!(Mode::TemplateExecution.name(), "mode::template::execution");
        assert_eq!(Mode::Comment.name(), "mode::comment");
        assert_eq!(Mode::Documentation.name(), "mode::documentation");
        assert_eq!(Mode::List.name(), "mode::list");
        assert_eq!(Mode::Block.name(), "mode::block");
        assert_eq!(Mode::Group.name(), "mode::group");
    }
}
