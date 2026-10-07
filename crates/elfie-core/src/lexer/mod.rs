//! Compiled from `def/lexer/main.lfy`.
//!
//! [`lex`] turns source text into tokens. At each position every terminal that may be
//! lexed in the innermost open region of the [`ModeStack`] (`lexedIn`, or
//! [`modes::IN_CODE`] for a terminal that is neither `lexedIn` nor an escape) is matched
//! against the EBNF of the terminal document, the only grammar the lexer knows; the
//! longest match is the token, a keyword beats an identifier of the same text, and any
//! other tie is an error naming both terminals.

use std::fmt;
use std::sync::{Arc, OnceLock};

pub mod data;
pub mod modes;
pub mod traits;

pub use data::{ModeEntry, ModeStack, Token};
pub use modes::Mode;

use crate::grammar::terminals::comment::{self, Comment};
use crate::grammar::terminals::identifier::Identifier;
use crate::grammar::terminals::literal::{self, Literal};
use crate::grammar::terminals::space::{self, Space};
use crate::grammar::{self, Entity, GrammarRule};

/// The file name used when none is given.
// @lfy def/lexer/main.lfy:lex
pub const ANONYMOUS_FILE: &str = "anonymous";

/// `ace function matches(rule)`: the characters at `offset` match the rule's syntax.
/// Yields the byte length of the longest such match.
fn matches(rule: Entity, source: &str, offset: usize) -> Option<usize> {
    rule.longest_match_at(source, offset)
}

/// The terminal document is the only grammar the lexer knows: the EBNF of every terminal.
// @lfy def/lexer/main.lfy:lex
pub fn terminals() -> String {
    grammar::terminal_document()
}

/// Every terminal the lexer considers, with the regions it may be lexed in. Escapes are matched only
/// as part of another terminal and can never be a token, so they are left out.
// @lfy def/lexer/main.lfy:lex
fn candidates() -> &'static [(Entity, &'static [Mode])] {
    static CANDIDATES: OnceLock<Vec<(Entity, &'static [Mode])>> = OnceLock::new();
    CANDIDATES.get_or_init(|| {
        Entity::terminals()
            .filter(|terminal| !terminal.is_escape()) // @lfy def/lexer/main.lfy:lex
            .map(|terminal| (terminal, modes::lex_condition(terminal))) // @lfy def/lexer/main.lfy:lex
            .collect()
    })
}

/// A compile-time lexer error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// Two terminals matched tokens of equal length and no other criteria specifies
    /// which wins.
    // @lfy def/lexer/main.lfy:lex
    AmbiguousMatch {
        file: Arc<str>,
        line: usize,
        column: usize,
        text: String,
        first: Entity,
        second: Entity,
    },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::AmbiguousMatch {
                file,
                line,
                column,
                text,
                first,
                second,
            } => write!(
                f,
                "{file}:{line}:{column}: {text:?} matches both {first} and {second}"
            ),
        }
    }
}

impl std::error::Error for LexError {}

/// Turn source text into tokens.
///
/// `source` is read from start to end; every token records `file`, or `"anonymous"` when
/// none is given. Text no terminal matches becomes a token without a rule, and a mode
/// still open at the end of the source becomes a token without a rule or raw text whose
/// value names the mode. The only error is a tie between two terminals.
// @lfy def/lexer/main.lfy:lex
pub fn lex(source: &str, file: Option<&str>) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer {
        input: source,
        offset: 0,
        line: 1,                                         // @lfy def/lexer/main.lfy:lex
        column: 0,                                       // @lfy def/lexer/main.lfy:lex
        file: Arc::from(file.unwrap_or(ANONYMOUS_FILE)), // @lfy def/lexer/main.lfy:lex
        stack: ModeStack::new(),                         // @lfy def/lexer/main.lfy:lex
        tokens: Vec::new(),
    };
    lexer.run()?;
    Ok(lexer.tokens)
}

struct Lexer<'a> {
    input: &'a str,
    offset: usize,
    line: usize,
    column: usize,
    file: Arc<str>,
    stack: ModeStack,
    tokens: Vec<Token>,
}

/// Whether `rest` begins with a line break, either spelling.
fn at_line_break(rest: &str) -> bool {
    rest.starts_with('\n') || rest.starts_with("\r\n")
}

impl Lexer<'_> {
    /// Reads the source from start to end.
    // @lfy def/lexer/main.lfy:lex
    fn run(&mut self) -> Result<(), LexError> {
        while self.offset < self.input.len() {
            if at_line_break(&self.input[self.offset..]) {
                traits::before_line_break(&mut self.stack);
            }
            match self.candidate()? {
                // @lfy def/lexer/main.lfy:lex
                Some((terminal, len)) => {
                    self.push(Some(terminal), len);
                    traits::on_token(terminal, &mut self.stack);
                }
                // @lfy def/lexer/main.lfy:lex
                None => {
                    let len = self.invalid_run();
                    self.push(None, len);
                }
            }
        }
        traits::before_line_break(&mut self.stack);
        // @lfy def/lexer/main.lfy:lex
        for entry in self.stack.open() {
            self.tokens.push(Token {
                rule: None,
                raw: String::new(),
                value: entry.mode.name().to_owned(),
                file: Arc::clone(&self.file),
                line: self.line,
                column: self.column,
            });
        }
        Ok(())
    }

    /// The match of one candidate terminal at `offset`, after the `where` clauses of the
    /// terminal are applied.
    fn candidate_match(&self, terminal: Entity) -> Option<(Entity, usize)> {
        let len = matches(terminal, self.input, self.offset)?;
        // @lfy def/grammar/terminals/comment.lfy:BlockDocumentationOpen
        if terminal == Entity::Comment(Comment::BlockDocumentationOpen)
            && comment::block_documentation_open_is_block_comment_open(
                &self.input[self.offset + len..],
            )
        {
            let open = Entity::Comment(Comment::BlockCommentOpen);
            return matches(open, self.input, self.offset).map(|len| (open, len));
        }
        Some((terminal, len))
    }

    /// Matches every candidate terminal at `offset` and picks the token: the longest
    /// match; between a keyword and an identifier of the same text, the keyword. Any
    /// other tie is an error naming both terminals.
    // @lfy def/lexer/main.lfy:lex
    fn candidate(&self) -> Result<Option<(Entity, usize)>, LexError> {
        let mode = self.stack.top_mode();
        let mut best: Option<(Entity, usize)> = None;
        let mut tie: Option<Entity> = None;
        for &(terminal, condition) in candidates() {
            if !condition.contains(&mode) {
                continue;
            }
            let Some((matched, len)) = self.candidate_match(terminal) else {
                continue;
            };
            match best {
                // @lfy def/lexer/main.lfy:lex
                Some((_, longest)) if len < longest => {}
                Some((current, longest)) if len == longest => {
                    if current == matched {
                        continue;
                    }
                    // @lfy def/lexer/main.lfy:lex
                    let identifier = Entity::Identifier(Identifier::Identifier);
                    if current == identifier && matched.is_keyword() {
                        best = Some((matched, len));
                    } else if !(matched == identifier && current.is_keyword()) {
                        tie.get_or_insert(matched);
                    }
                }
                _ => {
                    best = Some((matched, len));
                    tie = None;
                }
            }
        }
        match (best, tie) {
            // @lfy def/lexer/main.lfy:lex
            (Some((first, len)), Some(second)) => Err(LexError::AmbiguousMatch {
                file: Arc::clone(&self.file),
                line: self.line,
                column: self.column,
                text: self.input[self.offset..self.offset + len].to_owned(),
                first,
                second,
            }),
            (best, None) => Ok(best),
            (None, Some(_)) => unreachable!("a tie needs a best match"),
        }
    }

    /// Byte length of the text from `offset` up to the next position where some terminal
    /// matches, which is where lexing continues.
    // @lfy def/lexer/main.lfy:lex
    fn invalid_run(&self) -> usize {
        let mode = self.stack.top_mode();
        let mut end = self.offset;
        loop {
            let c = self.input[end..]
                .chars()
                .next()
                .expect("the run is inside the input");
            end += c.len_utf8();
            let rest = &self.input[end..];
            if rest.is_empty() {
                break;
            }
            // A line break that ends the mode at the top is lexed after the mode ends.
            if at_line_break(rest) && traits::mode_ending_with_line(&self.stack).is_some() {
                break;
            }
            let some_terminal_matches = candidates().iter().any(|&(terminal, condition)| {
                condition.contains(&mode) && matches(terminal, self.input, end).is_some()
            });
            if some_terminal_matches {
                break;
            }
        }
        end - self.offset
    }

    /// Creates the token for `rule` from the next `len` bytes and appends it.
    fn push(&mut self, rule: Option<Entity>, len: usize) {
        let raw = &self.input[self.offset..self.offset + len]; // @lfy def/lexer/main.lfy:lex
        self.tokens.push(Token {
            rule,
            raw: raw.to_owned(),
            value: value(rule, raw),
            file: Arc::clone(&self.file),
            line: self.line,     // @lfy def/lexer/main.lfy:lex
            column: self.column, // @lfy def/lexer/main.lfy:lex
        });
        self.advance(len);
    }

    /// Consumes `len` bytes, tracking the 1-indexed line and the 0-indexed column in
    /// characters of the raw text.
    // @lfy def/lexer/main.lfy:lex
    fn advance(&mut self, len: usize) {
        for c in self.input[self.offset..self.offset + len].chars() {
            if c == '\n' {
                self.line += 1;
                self.column = 0;
            } else {
                self.column += 1;
            }
        }
        self.offset += len;
    }
}

/// The value of a token: what the terminal's criteria say it should be, and the raw text
/// for every terminal without such criteria.
// @lfy def/lexer/main.lfy:lex
fn value(rule: Option<Entity>, raw: &str) -> String {
    match rule {
        // @lfy def/grammar/terminals/space.lfy:NewLine
        Some(Entity::Space(Space::NewLine)) => space::NEW_LINE_VALUE.to_owned(),
        // @lfy def/grammar/terminals/literal.lfy:NumberLiteral
        Some(Entity::Literal(Literal::NumberLiteral)) => literal::number_value(raw),
        // @lfy def/grammar/traits.lfy:body
        Some(rule) if rule.is_body() => grammar::traits::body_value(rule, raw),
        // @lfy def/lexer/main.lfy:lex
        _ => raw.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::rules;
    use crate::grammar::terminals::keyword::Keyword;
    use crate::grammar::terminals::punctuation::Punctuation;
    use std::time::Instant;

    fn tokens(source: &str) -> Vec<Token> {
        lex(source, None).unwrap_or_else(|error| panic!("{source:?}: {error}"))
    }

    fn rules_of(source: &str) -> Vec<Option<Entity>> {
        tokens(source).into_iter().map(|token| token.rule).collect()
    }

    fn values(source: &str) -> Vec<String> {
        tokens(source)
            .into_iter()
            .map(|token| token.value)
            .collect()
    }

    fn raws(source: &str) -> Vec<String> {
        tokens(source).into_iter().map(|token| token.raw).collect()
    }

    fn keyword(keyword: Keyword) -> Option<Entity> {
        Some(Entity::Keyword(keyword))
    }

    fn punctuation(punctuation: Punctuation) -> Option<Entity> {
        Some(Entity::Punctuation(punctuation))
    }

    fn literal(literal: Literal) -> Option<Entity> {
        Some(Entity::Literal(literal))
    }

    fn comment(comment: Comment) -> Option<Entity> {
        Some(Entity::Comment(comment))
    }

    const IDENTIFIER: Option<Entity> = Some(Entity::Identifier(Identifier::Identifier));
    const SPACE: Option<Entity> = Some(Entity::Space(Space::Space));
    const NEW_LINE: Option<Entity> = Some(Entity::Space(Space::NewLine));
    const INVALID: Option<Entity> = None;

    /// `Token@like(`Token for [[rule]] with value "..."`)`
    fn like(rule: impl GrammarRule, value: &str) -> (Option<Entity>, String) {
        (Some(rule.entity()), value.to_owned())
    }

    /// `Token@like(`Invalid token with ... value "..."`)`
    fn invalid(value: &str) -> (Option<Entity>, String) {
        (None, value.to_owned())
    }

    /// The rule and value of every token of `source`.
    fn rule_values(source: &str) -> Vec<(Option<Entity>, String)> {
        tokens(source)
            .into_iter()
            .map(|token| (token.rule, token.value))
            .collect()
    }

    /// The characters a terminal matches: its fixed text, or a sample its syntax accepts.
    fn sample(terminal: Entity) -> &'static str {
        let crate::grammar::Category::Terminal(kind) = terminal.category() else {
            unreachable!()
        };
        if let Some(text) = kind.fixed_text() {
            return text;
        }
        match terminal {
            Entity::Identifier(Identifier::Identifier) => "x",
            Entity::Space(Space::NewLine) => "\n",
            Entity::Space(Space::Space) => " ",
            Entity::Literal(Literal::NumberLiteral) => "1",
            _ => "a",
        }
    }

    /// Source that puts the lexer in `mode`, and how many tokens it takes to get there.
    fn opening(terminal: Entity, mode: Mode) -> (&'static str, usize) {
        // `/**` straight before `**/` is a block comment open, so the documentation close
        // is reached over a body.
        if terminal == Entity::Comment(Comment::BlockDocumentationClose) {
            return ("/** ", 2);
        }
        match mode {
            Mode::Code => ("", 0),
            Mode::BlockComment => ("/*", 1),
            Mode::LineComment => ("//", 1),
            Mode::BlockDocumentation => ("/**", 1),
            Mode::LineDocumentation => ("///", 1),
            Mode::SingleQuote => ("'", 1),
            Mode::DoubleQuote => ("\"", 1),
            Mode::Template => ("`", 1),
            Mode::Execution => ("`{{", 2),
            Mode::Reference => ("`[[", 2),
        }
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:73a7a4972a85e8de228731064674ba62d23fc532c2c7fc64b823ebde817d129e
    #[test]
    fn test_empty_source_gives_no_tokens() {
        assert_eq!(tokens(""), Vec::<Token>::new());
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:7b9f6253aa23103ccfefec7e416516c2ab9ec93261faf9a4d353cf5811c2328b
    #[test]
    fn test_a_constant_declaration() {
        assert_eq!(
            rule_values("const x = 1_0;"),
            vec![
                like(Keyword::ConstKeyword, "const"),
                like(Space::Space, " "),
                like(Identifier::Identifier, "x"),
                like(Space::Space, " "),
                like(Punctuation::PlainSetter, "="),
                like(Space::Space, " "),
                like(Literal::NumberLiteral, "10"),
                like(Punctuation::Semicolon, ";"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:17d7117b296e91576aded6aa38378d5018ad738ecf5f0ead9b23b2e82a4e0233
    #[test]
    fn test_a_template_with_an_execution() {
        assert_eq!(
            rule_values("`a {{b}} c`;"),
            vec![
                like(Literal::Backtick, "`"),
                like(Literal::TemplateBody, "a "),
                like(Literal::ExecutionOpen, "{{"),
                like(Identifier::Identifier, "b"),
                like(Literal::ExecutionClose, "}}"),
                like(Literal::TemplateBody, " c"),
                like(Literal::Backtick, "`"),
                like(Punctuation::Semicolon, ";"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:893b54f62abaf4b2b38de8b0456315205a0488651632fad8b13e9e28b143f889
    #[test]
    fn test_a_single_quoted_string_with_an_escaped_quote() {
        assert_eq!(
            rule_values("'it\\'s'"),
            vec![
                like(Literal::SingleQuote, "'"),
                like(Literal::SingleQuoteBody, "it's"),
                like(Literal::SingleQuote, "'"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:548ada476c75f16baed3abd68708b901268a463fdddcaa50c4f7a0b62eb5142c
    #[test]
    fn test_templates_nested_in_executions() {
        assert_eq!(
            rule_values("`{{`{{'c'}}`}}`"),
            vec![
                like(Literal::Backtick, "`"),
                like(Literal::ExecutionOpen, "{{"),
                like(Literal::Backtick, "`"),
                like(Literal::ExecutionOpen, "{{"),
                like(Literal::SingleQuote, "'"),
                like(Literal::SingleQuoteBody, "c"),
                like(Literal::SingleQuote, "'"),
                like(Literal::ExecutionClose, "}}"),
                like(Literal::Backtick, "`"),
                like(Literal::ExecutionClose, "}}"),
                like(Literal::Backtick, "`"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:e15077d337136a12a2767335f039724d1cf712f07322052d869e4cb691abb2d1
    #[test]
    fn test_a_single_quoted_string_ends_with_its_line() {
        assert_eq!(
            rule_values("'abc\nx"),
            vec![
                like(Literal::SingleQuote, "'"),
                like(Literal::SingleQuoteBody, "abc"),
                like(Space::NewLine, "\n"),
                like(Identifier::Identifier, "x"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:cfc52b20fca76e7bb188b9953a5502be8861f166a41e638d6e92c760063e5c8b
    #[test]
    fn test_an_unclosed_template_execution() {
        let tokens = tokens("`a {{ b");
        assert_eq!(
            tokens
                .iter()
                .map(|t| (t.rule, t.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                like(Literal::Backtick, "`"),
                like(Literal::TemplateBody, "a "),
                like(Literal::ExecutionOpen, "{{"),
                like(Space::Space, " "),
                like(Identifier::Identifier, "b"),
                invalid("template"),
                invalid("template execution"),
            ]
        );
        assert!(tokens[5..].iter().all(|t| t.raw.is_empty()));
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:1ead9942fd53c95b2b1c71274c35a3e025ff5f461becaba43405f95cf75f1b5c
    #[test]
    fn test_a_string_inside_an_execution_hides_the_execution_close() {
        assert_eq!(
            rule_values("`{{ '}}' }}`"),
            vec![
                like(Literal::Backtick, "`"),
                like(Literal::ExecutionOpen, "{{"),
                like(Space::Space, " "),
                like(Literal::SingleQuote, "'"),
                like(Literal::SingleQuoteBody, "}}"),
                like(Literal::SingleQuote, "'"),
                like(Space::Space, " "),
                like(Literal::ExecutionClose, "}}"),
                like(Literal::Backtick, "`"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:ba0e7334dc58b81bf802ad9450c071bbbc8f7c906810e26656c14d8f3fc6e357
    #[test]
    fn test_an_identifier_that_starts_like_a_keyword() {
        assert_eq!(
            rule_values("iffy"),
            vec![like(Identifier::Identifier, "iffy")]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:18f6c9df7322a74ec16b4864e5e95cfb309ad7e8ec97ff75e2b6afa27dea4e24
    #[test]
    fn test_an_empty_block_comment() {
        assert_eq!(
            rule_values("/**/"),
            vec![
                like(Comment::BlockCommentOpen, "/*"),
                like(Comment::BlockCommentClose, "*/"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:10684fce6b28dcd2b3a14ea0901456423c8bd7eb5a85dc0c2b28526d8bd438cf
    #[test]
    fn test_a_block_comment_of_one_star() {
        assert_eq!(
            rule_values("/***/"),
            vec![
                like(Comment::BlockCommentOpen, "/*"),
                like(Comment::BlockCommentBody, "*"),
                like(Comment::BlockCommentClose, "*/"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:891c6b11eadc6cbd3a1b5d861c861e61db82c9401edf65174e86ae127db25f95
    #[test]
    fn test_nested_block_comments() {
        assert_eq!(
            rule_values("/* a /* b */ c */"),
            vec![
                like(Comment::BlockCommentOpen, "/*"),
                like(Comment::BlockCommentBody, " a "),
                like(Comment::BlockCommentOpen, "/*"),
                like(Comment::BlockCommentBody, " b "),
                like(Comment::BlockCommentClose, "*/"),
                like(Comment::BlockCommentBody, " c "),
                like(Comment::BlockCommentClose, "*/"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:8fdeb1cd8ac49dea15d154c8a0cca000a0c04f1f1b01d8261559241ca4b28524
    #[test]
    fn test_a_carriage_return_line_feed_is_one_new_line() {
        let tokens = tokens("a\r\nb");
        assert_eq!(
            tokens.iter().map(|t| t.rule).collect::<Vec<_>>(),
            vec![IDENTIFIER, NEW_LINE, IDENTIFIER]
        );
        assert_eq!(tokens[0].value, "a");
        assert_eq!((tokens[1].raw.as_str(), tokens[1].value.as_str()), ("\r\n", "\n"));
        assert_eq!(tokens[2].value, "b");
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:92421842c4920a003ecb9e3d7bfbb92f5b2dc4b9b221e83ebbdf2898e563f1b6
    #[test]
    fn test_positions_across_a_line_break_in_a_comment() {
        let tokens = tokens("/*\n*/x");
        assert_eq!(
            tokens.iter().map(|t| t.rule).collect::<Vec<_>>(),
            vec![
                comment(Comment::BlockCommentOpen),
                comment(Comment::BlockCommentBody),
                comment(Comment::BlockCommentClose),
                IDENTIFIER
            ]
        );
        assert_eq!((tokens[0].line, tokens[0].column), (1, 0));
        assert_eq!(tokens[1].value, "\n");
        assert_eq!((tokens[1].line, tokens[1].column), (1, 2));
        assert_eq!((tokens[2].line, tokens[2].column), (2, 0));
        assert_eq!(tokens[3].value, "x");
        assert_eq!((tokens[3].line, tokens[3].column), (2, 2));
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:96ecba469c366db6836b75fe8cb5220c8897d4e2575c25a50318c44bc20d4044
    #[test]
    fn test_an_escape_that_is_no_scalar_value_is_kept_as_written() {
        assert_eq!(
            rule_values("\"\\uD800\""),
            vec![
                like(Literal::DoubleQuote, "\""),
                like(Literal::DoubleQuoteBody, "\\uD800"),
                like(Literal::DoubleQuote, "\""),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:fb91d444d42a1275b607e9356599415e9e6f1270854c82b72ee80f4910387d6c
    #[test]
    fn test_the_longer_of_two_operators() {
        assert_eq!(
            rule_values("a<=b<c"),
            vec![
                like(Identifier::Identifier, "a"),
                like(Punctuation::LessThanOrEqual, "<="),
                like(Identifier::Identifier, "b"),
                like(Punctuation::LessThan, "<"),
                like(Identifier::Identifier, "c"),
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:6701c7667b12ea5d6bee360e5df095d113aed71bc729ede8b195a053ed58b60b
    #[test]
    fn test_unmatched_characters_make_one_invalid_token() {
        let tokens = tokens("x ##y");
        assert_eq!(
            tokens.iter().map(|t| t.rule).collect::<Vec<_>>(),
            vec![IDENTIFIER, SPACE, INVALID, IDENTIFIER]
        );
        assert_eq!(tokens[2].raw, "##");
        assert_eq!(tokens[3].value, "y");
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:e49f5f6a58f5fb306b0a46405f983db1638c4962e9f7cb4b5d1a583da7192a06
    #[test]
    fn the_call_ends_in_time_linear_in_the_length_of_source() {
        let unit = "const x = 1_0; `a {{b}} c` 'q' // e\n/* c */ \"s\\n\"\n";
        let time = |copies: usize| {
            let source = unit.repeat(copies);
            (0..2)
                .map(|_| {
                    let start = Instant::now();
                    let tokens = tokens(&source);
                    assert!(tokens.iter().all(|t| !t.is_invalid()));
                    start.elapsed().as_secs_f64()
                })
                .fold(f64::MAX, f64::min)
        };
        let small = time(250).max(1e-4);
        let large = time(2000);
        // Eight times the source, quadratic time would be sixty-four times as long.
        assert!(large < small * 30.0, "{small} s then {large} s");
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:46fca6c8156b28104c359868042425a2d8c3b4cfe2c561ceedb77767a0ae8962
    #[test]
    fn file_is_recorded_on_every_token_and_defaults_to_anonymous() {
        let given = lex("a `b` 'c' (", Some("lib.lfy")).unwrap();
        assert!(given.iter().all(|t| &*t.file == "lib.lfy"));
        assert!(tokens("a `b` (").iter().all(|t| &*t.file == "anonymous"));
        assert_eq!(ANONYMOUS_FILE, "anonymous");
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:b9f9675f76955f9159dda31fad7d768cb5c80e56d5eac562d9bdeec8a9ff8f9a
    #[test]
    fn a_new_code_region_is_the_whole_source_and_no_token_closes_it() {
        assert_eq!(tokens("`").len(), 2);
        assert_eq!(rules_of("x"), vec![IDENTIFIER]);
        assert_eq!(rules_of("}"), vec![punctuation(Punctuation::BlockClose)]);
        // Closers of other regions find no region to close in the code region.
        assert_eq!(
            rules_of("}} ]] */ **/"),
            vec![
                punctuation(Punctuation::BlockClose),
                punctuation(Punctuation::BlockClose),
                SPACE,
                punctuation(Punctuation::ListClose),
                punctuation(Punctuation::ListClose),
                SPACE,
                punctuation(Punctuation::Star),
                punctuation(Punctuation::Slash),
                SPACE,
                punctuation(Punctuation::Power),
                punctuation(Punctuation::Slash),
            ]
        );
        let mut stack = ModeStack::new();
        traits::on_token(
            Entity::Literal(Literal::ExecutionClose),
            &mut stack,
        );
        assert!(stack.is_only_code());
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:705fd2030916df6ab5067c9613d82364d9c18da6e324ec538415ec798828383c
    #[test]
    fn a_region_a_token_opens_is_nested_in_the_innermost_open_region() {
        let mut lexer_stack = ModeStack::new();
        for (terminal, expected) in [
            (
                Entity::Literal(Literal::Backtick),
                vec![Mode::Code, Mode::Template],
            ),
            (
                Entity::Literal(Literal::ExecutionOpen),
                vec![Mode::Code, Mode::Template, Mode::Execution],
            ),
            (
                Entity::Comment(Comment::BlockCommentOpen),
                vec![
                    Mode::Code,
                    Mode::Template,
                    Mode::Execution,
                    Mode::BlockComment,
                ],
            ),
        ] {
            traits::on_token(terminal, &mut lexer_stack);
            assert_eq!(lexer_stack.modes(), expected);
        }
        // The same reading through the source: a block comment inside an expression.
        assert_eq!(
            rules_of("`{{ /* a */ }}`")[3..8],
            [
                comment(Comment::BlockCommentOpen),
                comment(Comment::BlockCommentBody),
                comment(Comment::BlockCommentClose),
                SPACE,
                literal(Literal::ExecutionClose)
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:8417070f62f82d1e18d839ed4c019edcdbaa3cb606742e99f2c209da497ddb6a
    #[test]
    fn the_innermost_open_region_is_the_most_recently_opened_one_still_open() {
        // After the inner template closes, the execution around it is innermost again.
        assert_eq!(
            rules_of("`{{`x`}}y`"),
            vec![
                literal(Literal::Backtick),
                literal(Literal::ExecutionOpen),
                literal(Literal::Backtick),
                literal(Literal::TemplateBody),
                literal(Literal::Backtick),
                literal(Literal::ExecutionClose),
                literal(Literal::TemplateBody),
                literal(Literal::Backtick)
            ]
        );
        // After the nested block comment closes, the outer one is innermost again.
        assert_eq!(
            values("/* a /* b */ c */ d"),
            vec!["/*", " a ", "/*", " b ", "*/", " c ", "*/", " ", "d"]
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:a16b8f611824800aa3e94ae3679cdc0780785c23bffe3d4a42d881d83af149f8
    #[test]
    fn a_closer_of_a_region_that_is_not_the_innermost_leaves_it_open() {
        // `}}` is not lexed inside the string, so the execution stays open.
        // The execution and the template are still open at the end of the source.
        assert_eq!(
            values("`{{ '}} }}"),
            vec!["`", "{{", " ", "'", "}} }}", "template", "template execution"]
        );
        // Nor inside a nested block comment.
        let tokens = tokens("/* /* */");
        assert_eq!(
            tokens.last().map(|t| (t.is_invalid(), t.value.as_str())),
            Some((true, "block comment"))
        );
        let mut stack = ModeStack::new();
        stack.push(Mode::Template, Entity::Literal(Literal::Backtick));
        stack.push(Mode::Execution, Entity::Literal(Literal::ExecutionOpen));
        stack.push(Mode::SingleQuote, Entity::Literal(Literal::SingleQuote));
        traits::on_token(Entity::Literal(Literal::ExecutionClose), &mut stack);
        assert_eq!(stack.modes().len(), 4);
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:413502655066dc7af90a6ab7d8fd822bbf4f98798a65ff8665754f95ff155ef8
    #[test]
    fn a_terminal_that_is_not_lexed_in_a_region_begins_only_where_in_code_holds() {
        assert_eq!(
            modes::lex_condition(Entity::Punctuation(Punctuation::BlockOpen)),
            modes::IN_CODE
        );
        // In code: `{{` is two block opens, because `ExecutionOpen` is no candidate here.
        assert_eq!(
            rules_of("{{}}"),
            vec![
                punctuation(Punctuation::BlockOpen),
                punctuation(Punctuation::BlockOpen),
                punctuation(Punctuation::BlockClose),
                punctuation(Punctuation::BlockClose)
            ]
        );
        // Inside an execution, where `[[` is two list opens for the same reason.
        assert_eq!(
            rules_of("`{{ {} }}`")[2..6],
            [
                SPACE,
                punctuation(Punctuation::BlockOpen),
                punctuation(Punctuation::BlockClose),
                SPACE
            ]
        );
        assert_eq!(
            rules_of("`{{[[]]}}`")[2..6],
            [
                punctuation(Punctuation::ListOpen),
                punctuation(Punctuation::ListOpen),
                punctuation(Punctuation::ListClose),
                punctuation(Punctuation::ListClose)
            ]
        );
        // And inside a reference.
        assert_eq!(
            rules_of("`[[x + 1]]`")[2..7],
            [
                IDENTIFIER,
                SPACE,
                punctuation(Punctuation::Plus),
                SPACE,
                literal(Literal::NumberLiteral)
            ]
        );
        // Where `inCode` does not hold they are no candidates at all.
        assert_eq!(values("'if x'"), vec!["'", "if x", "'"]);
        assert_eq!(values("`a}}b]]c`"), vec!["`", "a}}b]]c", "`"]);
        assert_eq!(values("/* ' ` \" // */"), vec!["/*", " ' ` \" // ", "*/"]);
        assert_eq!(values("// /* */ /** **/"), vec!["//", " /* */ /** **/"]);
        // Escapes are never tokens of their own.
        for rule in tokens("\"\\n\\x41\\u0041\\\\\" `\\`\\{{`")
            .iter()
            .filter_map(|t| t.rule)
        {
            assert!(!rule.is_escape(), "{rule}");
        }
        assert!(
            candidates()
                .iter()
                .all(|(terminal, _)| !terminal.is_escape())
        );
        assert_eq!(
            candidates().len(),
            rules()
                .filter(|r| r.is_terminal() && !r.is_escape())
                .count()
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:0c179742c1ae096575971cf29f9596ba3da76eedf30fcbad1c19dd50c4ee0980
    #[test]
    fn the_longest_match_is_the_token() {
        assert_eq!(
            rules_of("=>"),
            vec![punctuation(Punctuation::DoubleArrowRight)]
        );
        assert_eq!(rules_of("=**"), vec![punctuation(Punctuation::PowerSetter)]);
        assert_eq!(rules_of("**"), vec![punctuation(Punctuation::Power)]);
        assert_eq!(rules_of("..."), vec![punctuation(Punctuation::Spread)]);
        assert_eq!(
            rules_of("^^"),
            vec![punctuation(Punctuation::PreviousStatement)]
        );
        assert_eq!(
            rules_of("$&"),
            vec![punctuation(Punctuation::ParentScopeAccessor)]
        );
        assert_eq!(
            rules_of("?."),
            vec![punctuation(Punctuation::OptionalValueAccessor)]
        );
        assert_eq!(rules_of("??"), vec![punctuation(Punctuation::NullishOr)]);
        assert_eq!(
            rules_of("<-"),
            vec![punctuation(Punctuation::SingleArrowLeft)]
        );
        assert_eq!(
            rules_of("=??"),
            vec![punctuation(Punctuation::NullishSetter)]
        );
        assert_eq!(
            rules_of("&="),
            vec![punctuation(Punctuation::SameReference)]
        );
        assert_eq!(
            rules_of("a=-1"),
            vec![
                IDENTIFIER,
                punctuation(Punctuation::SubtractSetter),
                literal(Literal::NumberLiteral)
            ]
        );
        assert_eq!(rules_of("//"), vec![comment(Comment::LineCommentOpen)]);
        assert_eq!(
            rules_of("///"),
            vec![comment(Comment::LineDocumentationOpen)]
        );
        assert_eq!(
            rules_of("matchall"),
            vec![keyword(Keyword::MatchallKeyword)]
        );
        assert_eq!(rules_of("format"), vec![IDENTIFIER]);
        assert_eq!(rules_of("trueish"), vec![IDENTIFIER]);
        assert_eq!(rules_of("d1"), vec![IDENTIFIER]);
        assert_eq!(
            rules_of("1_"),
            vec![literal(Literal::NumberLiteral), IDENTIFIER]
        );
        // Every terminal's fixed text is lexed to that terminal in every region it is
        // lexed in.
        for terminal in Entity::terminals().filter(|terminal| !terminal.is_escape()) {
            let text = sample(terminal);
            for &mode in modes::lex_condition(terminal) {
                let (open, before) = opening(terminal, mode);
                let source = format!("{open}{text}");
                let tokens = tokens(&source);
                assert_eq!(tokens[before].rule, Some(terminal), "{terminal} in {mode}");
                assert_eq!(tokens[before].raw, text, "{terminal} in {mode}");
            }
        }
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:a1033d1d6478ddf6a9273749960180b1c1cf6be09e5612334b6fbe9911fe8d01
    #[test]
    fn no_two_candidates_tie_on_the_terminal_document() {
        // Every fixed text of every terminal lexes to exactly that terminal in code.
        for terminal in Entity::terminals() {
            let crate::grammar::Category::Terminal(kind) = terminal.category() else {
                unreachable!()
            };
            let Some(text) = kind.fixed_text() else {
                continue;
            };
            if modes::lex_condition(terminal) != modes::IN_CODE
                && !modes::lex_condition(terminal).contains(&Mode::Code)
            {
                continue;
            }
            let tokens = tokens(text);
            assert_eq!(tokens[0].rule, Some(terminal), "{terminal}");
        }
        let error = LexError::AmbiguousMatch {
            file: Arc::from("a.lfy"),
            line: 2,
            column: 1,
            text: "x".into(),
            first: Entity::Keyword(Keyword::IfKeyword),
            second: Entity::Keyword(Keyword::ElseKeyword),
        };
        assert_eq!(
            error.to_string(),
            "a.lfy:2:1: \"x\" matches both IfKeyword and ElseKeyword"
        );
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:015747e825e42ce69407c6e5289e45ef5a111f82dc8bc8fd0dec45f55786e40a
    #[test]
    fn text_no_terminal_matches_is_one_invalid_token_up_to_the_next_match() {
        let tokens = tokens("ab\n c#");
        assert_eq!(
            tokens.iter().map(|t| t.rule).collect::<Vec<_>>(),
            vec![IDENTIFIER, NEW_LINE, SPACE, IDENTIFIER, INVALID]
        );
        let invalid = &tokens[4];
        assert!(invalid.is_invalid());
        assert_eq!((invalid.line, invalid.column), (2, 2));
        assert_eq!((invalid.raw.as_str(), invalid.value.as_str()), ("#", "#"));
        // One token for the whole run; lexing continues afterwards.
        assert_eq!(values("##x"), vec!["##", "x"]);
        assert_eq!(values("\u{FEFF}x"), vec!["\u{FEFF}", "x"]);
        assert_eq!(
            rules_of("^ x"),
            vec![punctuation(Punctuation::BitwiseXor), SPACE, IDENTIFIER]
        );
        // A lone carriage return is no line break.
        let cr = self::tokens("a\rb");
        assert_eq!(cr[1].rule, INVALID);
        assert_eq!((cr[2].line, cr[2].column), (1, 2));
        // Unknown escapes are not body text.
        assert_eq!(
            rules_of("\"a\\qb\""),
            vec![
                literal(Literal::DoubleQuote),
                literal(Literal::DoubleQuoteBody),
                INVALID,
                literal(Literal::DoubleQuoteBody),
                literal(Literal::DoubleQuote)
            ]
        );
        assert_eq!(values("'a\\nb'"), vec!["'", "a", "\\", "nb", "'"]);
        assert_eq!(
            rules_of("`\\x4`"),
            vec![
                literal(Literal::Backtick),
                INVALID,
                literal(Literal::TemplateBody),
                literal(Literal::Backtick)
            ]
        );
        assert_eq!(values("`\\x4`"), vec!["`", "\\", "x4", "`"]);
        // A bare line break is not double quote body text; the string ends with the line.
        assert_eq!(
            rules_of("\"a\nb\""),
            vec![
                literal(Literal::DoubleQuote),
                literal(Literal::DoubleQuoteBody),
                NEW_LINE,
                IDENTIFIER,
                literal(Literal::DoubleQuote)
            ]
        );
        // A run stops at a line break that ends the region at the top.
        assert_eq!(values("/// a]]\nb"), vec!["///", " a", "]", "]", "\n", "b"]);
        // Sequences are not recognised outside strings and templates.
        assert_eq!(rules_of("\\n"), vec![INVALID, IDENTIFIER]);
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:e91ece4e60aef2c042d0d2d9931fb0371765cbb312233c53cfb72436fa714e7d
    #[test]
    fn regions_left_open_at_the_end_become_invalid_tokens_naming_them() {
        for (source, open) in [
            ("`abc", vec!["template"]),
            ("`{{", vec!["template", "template execution"]),
            ("`[[", vec!["template", "template reference"]),
            ("/* x", vec!["block comment"]),
            ("/* /* x */", vec!["block comment"]),
            ("/** x", vec!["block documentation"]),
            ("`{{ (`", vec!["template", "template execution", "template"]),
            ("/// [[x", vec!["line documentation", "template reference"]),
        ] {
            let tokens = tokens(source);
            let unclosed: Vec<&Token> = tokens.iter().filter(|t| t.raw.is_empty()).collect();
            assert_eq!(
                unclosed
                    .iter()
                    .map(|t| t.value.as_str())
                    .collect::<Vec<_>>(),
                open,
                "{source:?}"
            );
            assert!(unclosed.iter().all(|t| t.is_invalid()), "{source:?}");
            // They come after every other token.
            let first = tokens.len() - unclosed.len();
            assert!(tokens[first..].iter().all(|t| t.raw.is_empty()));
            let last = tokens.last().unwrap();
            assert_eq!(last.line, 1, "{source:?}");
            assert_eq!(last.column, source.chars().count(), "{source:?}");
        }
        // Regions that end with the line also end with the source.
        for source in ["'abc", "\"abc", "\"abc\n", "// x", "/// x"] {
            assert!(
                tokens(source).iter().all(|t| !t.raw.is_empty()),
                "{source:?}"
            );
        }
        assert!(tokens("(").iter().all(|t| !t.is_invalid()));
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:96f3ebf394d27ee519580d17930046716c6bb98571c66276b16a2f1700506748
    #[test]
    fn a_bare_name_inside_a_rule_refers_to_the_terminal_or_class_of_that_identifier() {
        // `ContinuationEscape = "\" , NewLine`, `HexEscape = "\x" , HexDigit , HexDigit`
        // and `NumberLiteral = Digit , ...` resolve their names to terminals and classes.
        assert_eq!(values("\"a\\\nb\""), vec!["\"", "ab", "\""]);
        assert_eq!(values("\"a\\\r\nb\""), vec!["\"", "ab", "\""]);
        assert_eq!(values("\"\\x41\""), vec!["\"", "A", "\""]);
        assert_eq!(values("1_0.5"), vec!["10.5"]);
        assert_eq!(raws("1_0.5"), vec!["1_0.5"]);
        // The excluded names of a body are terminals too: a body stops before them.
        assert_eq!(values("// x\ny"), vec!["//", " x", "\n", "y"]);
        assert_eq!(values("/** a /** b **/"), vec!["/**", " a ", "/", "** b ", "**/"]);
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:9605e2ff963ac9c9caf7f6a748be63b943d6195c5516b4f58b567e7dd045376b
    #[test]
    fn the_lexer_matches_against_the_terminal_document_only() {
        let document = terminals();
        assert_eq!(document, grammar::terminal_document());
        for (terminal, _) in candidates() {
            assert!(document.contains(terminal.text()), "{terminal}");
        }
        assert!(!document.contains("Expression ="));
    }

    // @lfy def/lexer/main.lfy:lex
    #[test]
    fn values_follow_the_criteria_of_their_terminals() {
        assert_eq!(values("\n"), vec!["\n"]);
        assert_eq!(values("\r\n"), vec!["\n"]);
        assert_eq!(values("1_0"), vec!["10"]);
        let cases = [
            ("\"\\\\\"", "\\"),
            ("\"\\r\"", "\r"),
            ("\"a\\\nb\"", "ab"),
            ("\"a\\\r\nb\"", "ab"),
            ("\"\\x41\"", "A"),
            ("\"\\u0041\"", "A"),
            ("\"\\u01F600\"", "😀"),
            ("\"\\n\"", "\n"),
            ("\"\\0\"", "\0"),
            ("\"\\t\"", "\t"),
            ("\"\\'\"", "'"),
            ("\"\\\"\"", "\""),
            ("`\\`\\{{\\}}\\[[\\]]\\n`", "`{{}}[[]]\n"),
            ("'a\\\\b\\'c'", "a\\b'c"),
        ];
        for (source, expected) in cases {
            let tokens = tokens(source);
            assert_eq!(tokens.len(), 3, "{source:?}");
            assert_eq!(tokens[1].value, expected, "{source:?}");
            assert_eq!(tokens[1].raw, &source[1..source.len() - 1], "{source:?}");
        }
        // Escaped template blocks stay text inside a template.
        assert_eq!(
            rules_of("`\\{{x\\}}`"),
            vec![
                literal(Literal::Backtick),
                literal(Literal::TemplateBody),
                literal(Literal::Backtick)
            ]
        );
        // Every other token's value is its raw text.
        assert_eq!(values("/** d **/"), vec!["/**", " d ", "**/"]);
        assert_eq!(values("if x"), vec!["if", " ", "x"]);
    }

    // @lfy def/lexer/main.lfy:lex
    #[test]
    fn line_and_column_come_from_the_raw_text() {
        let tokens = tokens("ab cd\n  ef `x\ny` g");
        let positions: Vec<(usize, usize)> = tokens.iter().map(|t| (t.line, t.column)).collect();
        assert_eq!(
            positions,
            vec![
                (1, 0),
                (1, 2),
                (1, 3),
                (1, 5),
                (2, 0),
                (2, 1),
                (2, 2),
                (2, 4),
                (2, 5),
                (2, 6),
                (3, 1),
                (3, 2),
                (3, 3)
            ]
        );
        let body = tokens.iter().find(|t| t.raw == "x\ny").unwrap();
        assert_eq!((body.line, body.column), (2, 6));
        // Joining the raw text reproduces the source.
        assert_eq!(
            tokens.iter().map(|t| t.raw.as_str()).collect::<String>(),
            "ab cd\n  ef `x\ny` g"
        );
        // The source is read as UTF-8 and columns count characters.
        let tokens = self::tokens("é ← 日本");
        assert_eq!(
            tokens.iter().map(|t| t.column).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
    }

    // @lfy def/grammar/terminals/comment.lfy:BlockDocumentationOpen
    #[test]
    fn block_documentation_open_followed_by_a_slash_or_close_is_a_block_comment() {
        assert_eq!(
            rules_of("/**/"),
            vec![
                comment(Comment::BlockCommentOpen),
                comment(Comment::BlockCommentClose)
            ]
        );
        assert_eq!(rules_of("/***/")[1], comment(Comment::BlockCommentBody));
        assert_eq!(rules_of("/**/x")[2], IDENTIFIER);
        assert_eq!(values("/** x **/"), vec!["/**", " x ", "**/"]);
        // Block comments and documentation open inside template executions; line
        // comments and line documentation do not and are not candidates there.
        assert_eq!(
            rules_of("`{{ /* c }} */ x }}`")[3..7],
            [
                comment(Comment::BlockCommentOpen),
                comment(Comment::BlockCommentBody),
                comment(Comment::BlockCommentClose),
                SPACE
            ]
        );
        assert_eq!(
            rules_of("`{{ /** d **/ }}`")[3],
            comment(Comment::BlockDocumentationOpen)
        );
        assert_eq!(
            rules_of("`{{ // c\n}}`")[3..5],
            [
                punctuation(Punctuation::Slash),
                punctuation(Punctuation::Slash)
            ]
        );
        assert_eq!(rules_of("`{{ /// c }}`")[3], punctuation(Punctuation::Slash));
        // Inside a reference neither kind of comment is a candidate.
        assert_eq!(rules_of("`[[ /* c ]]`")[3], punctuation(Punctuation::Slash));
        assert_eq!(rules_of("`[[ // c ]]`")[3], punctuation(Punctuation::Slash));
    }

    // @lfy def/lexer/main.lfy:lex
    #[test]
    fn line_regions_end_before_the_line_break() {
        assert_eq!(
            rules_of("// x\ny"),
            vec![
                comment(Comment::LineCommentOpen),
                comment(Comment::LineCommentBody),
                NEW_LINE,
                IDENTIFIER
            ]
        );
        assert_eq!(rules_of("// x\r\ny")[2], NEW_LINE);
        assert_eq!(values("// x\ry"), vec!["//", " x\ry"]);
        assert_eq!(rules_of("\"ab\ncd\"")[2], NEW_LINE);
        assert_eq!(
            rules_of("/// see [[Foo.bar]] x"),
            vec![
                comment(Comment::LineDocumentationOpen),
                comment(Comment::LineDocumentationBody),
                literal(Literal::ReferenceOpen),
                IDENTIFIER,
                punctuation(Punctuation::ValueAccessor),
                IDENTIFIER,
                literal(Literal::ReferenceClose),
                comment(Comment::LineDocumentationBody)
            ]
        );
    }

    // @lfy def/lexer/main.lfy:lex
    #[test]
    fn the_elfie_definitions_lex_start_to_finish_without_invalid_tokens() {
        for path in [
            "def/grammar/main.lfy",
            "def/grammar/precedence.lfy",
            "def/grammar/rules/expression.lfy",
            "def/grammar/rules/file.lfy",
            "def/grammar/rules/statement.lfy",
            "def/grammar/terminals/comment.lfy",
            "def/grammar/terminals/identifier.lfy",
            "def/grammar/terminals/keyword.lfy",
            "def/grammar/terminals/literal.lfy",
            "def/grammar/terminals/punctuation.lfy",
            "def/grammar/terminals/space.lfy",
            "def/grammar/traits.lfy",
            "def/lexer/data.lfy",
            "def/lexer/main.lfy",
            "def/lexer/modes.lfy",
            "def/lexer/traits.lfy",
            "def/parser/components.lfy",
            "def/parser/data.lfy",
            "def/parser/main.lfy",
            "def/parser/traits.lfy",
        ] {
            let source = std::fs::read_to_string(
                concat!(env!("CARGO_MANIFEST_DIR"), "/../../").to_string() + path,
            )
            .unwrap_or_else(|e| panic!("{path}: {e}"));
            let tokens = lex(&source, Some(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert_eq!(
                tokens.iter().map(|t| t.raw.as_str()).collect::<String>(),
                source,
                "{path}"
            );
            let invalid: Vec<String> = tokens
                .iter()
                .filter(|t| t.is_invalid())
                .map(ToString::to_string)
                .collect();
            assert!(invalid.is_empty(), "{path}: {invalid:?}");
            assert!(tokens.iter().all(|t| &*t.file == path));
        }
    }
}
