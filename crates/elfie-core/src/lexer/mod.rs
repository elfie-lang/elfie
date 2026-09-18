//! Compiled from `def/lexer/main.lfy`.
//!
//! [`lex`] turns source text into tokens. At each position every terminal whose
//! `candidateInModes` condition holds for the top of the [`ModeStack`] (or [`modes::IN_CODE`]
//! when it has none) is matched against the EBNF of the terminal document, the only
//! grammar the lexer knows; the longest match is the token, a keyword beats an identifier
//! of the same text, and any other tie is an error naming both terminals.

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
// @lfy def/lexer/main.lfy:23
pub const ANONYMOUS_FILE: &str = "anonymous";

/// `ace function matches(rule)`: the characters at `offset` match the rule's syntax.
/// Yields the byte length of the longest such match.
// @lfy def/lexer/main.lfy:11
fn matches(rule: Entity, source: &str, offset: usize) -> Option<usize> {
    rule.longest_match_at(source, offset)
}

/// The terminal document is the only grammar the lexer knows: the EBNF of every terminal.
// @lfy def/lexer/main.lfy:97
pub fn terminals() -> String {
    grammar::terminal_document()
}

/// Every terminal the lexer considers, with its lex condition. Escapes are matched only
/// as part of another terminal and can never be a token, so they are left out.
// @lfy def/lexer/main.lfy:102
fn candidates() -> &'static [(Entity, &'static [Mode])] {
    static CANDIDATES: OnceLock<Vec<(Entity, &'static [Mode])>> = OnceLock::new();
    CANDIDATES.get_or_init(|| {
        Entity::terminals()
            .filter(|terminal| !terminal.is_escape()) // @lfy def/lexer/main.lfy:110
            .map(|terminal| (terminal, modes::lex_condition(terminal))) // @lfy def/lexer/main.lfy:103
            .collect()
    })
}

/// A compile-time lexer error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// Two terminals matched tokens of equal length and no other criteria specifies
    /// which wins.
    // @lfy def/lexer/main.lfy:31
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
// @lfy def/lexer/main.lfy:19
pub fn lex(source: &str, file: Option<&str>) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer {
        input: source,
        offset: 0,
        line: 1,                                         // @lfy def/lexer/main.lfy:25
        column: 0,                                       // @lfy def/lexer/main.lfy:26
        file: Arc::from(file.unwrap_or(ANONYMOUS_FILE)), // @lfy def/lexer/main.lfy:22
        stack: ModeStack::new(),                         // @lfy def/lexer/main.lfy:28
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
    // @lfy def/lexer/main.lfy:21
    fn run(&mut self) -> Result<(), LexError> {
        while self.offset < self.input.len() {
            if at_line_break(&self.input[self.offset..]) {
                // @lfy def/lexer/traits.lfy:37
                traits::before_line_break(&mut self.stack);
            }
            match self.candidate()? {
                // @lfy def/lexer/main.lfy:103
                Some((terminal, len)) => {
                    self.push(Some(terminal), len);
                    traits::on_token(terminal, &mut self.stack); // @lfy def/lexer/traits.lfy:14
                }
                // @lfy def/lexer/main.lfy:114
                None => {
                    let len = self.invalid_run();
                    self.push(None, len);
                }
            }
        }
        // @lfy def/lexer/traits.lfy:37
        traits::before_line_break(&mut self.stack);
        // @lfy def/lexer/main.lfy:50
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
        // @lfy def/grammar/terminals/comment.lfy:12
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
    // @lfy def/lexer/main.lfy:29
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
                // @lfy def/lexer/main.lfy:30
                Some((_, longest)) if len < longest => {}
                Some((current, longest)) if len == longest => {
                    if current == matched {
                        continue;
                    }
                    // @lfy def/lexer/main.lfy:107
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
            // @lfy def/lexer/main.lfy:36
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
    // @lfy def/lexer/main.lfy:48
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
    // @lfy def/lexer/main.lfy:15
    fn push(&mut self, rule: Option<Entity>, len: usize) {
        let raw = &self.input[self.offset..self.offset + len]; // @lfy def/lexer/main.lfy:27
        self.tokens.push(Token {
            rule,
            raw: raw.to_owned(),
            value: value(rule, raw),
            file: Arc::clone(&self.file),
            line: self.line,     // @lfy def/lexer/main.lfy:25
            column: self.column, // @lfy def/lexer/main.lfy:26
        });
        self.advance(len);
    }

    /// Consumes `len` bytes, tracking the 1-indexed line and the 0-indexed column in
    /// characters of the raw text.
    // @lfy def/lexer/main.lfy:25
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
// @lfy def/lexer/main.lfy:38
fn value(rule: Option<Entity>, raw: &str) -> String {
    match rule {
        // @lfy def/grammar/terminals/space.lfy:4
        Some(Entity::Space(Space::NewLine)) => space::NEW_LINE_VALUE.to_owned(),
        // @lfy def/grammar/terminals/literal.lfy:12
        Some(Entity::Literal(Literal::NumberLiteral)) => literal::number_value(raw),
        // @lfy def/grammar/traits.lfy:63
        Some(rule) if rule.is_body() => grammar::traits::body_value(rule, raw),
        // @lfy def/lexer/main.lfy:44
        _ => raw.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::rules;
    use crate::grammar::terminals::keyword::Keyword;
    use crate::grammar::terminals::punctuation::Punctuation;

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
    fn like(token: &Token, rule: impl GrammarRule, value: &str) -> bool {
        token.is(rule) && token.value == value
    }

    // @lfy def/lexer/main.lfy:56
    #[test]
    fn test_empty_source_gives_no_tokens() {
        assert_eq!(tokens(""), Vec::<Token>::new());
    }

    // @lfy def/lexer/main.lfy:60
    #[test]
    fn test_a_constant_declaration() {
        let tokens = tokens("const x = 1_0;");
        assert_eq!(tokens.len(), 8);
        assert!(like(&tokens[0], Keyword::ConstKeyword, "const"));
        assert!(like(&tokens[1], Space::Space, " "));
        assert!(like(&tokens[2], Identifier::Identifier, "x"));
        assert!(like(&tokens[3], Space::Space, " "));
        assert!(like(&tokens[4], Punctuation::PlainSetter, "="));
        assert!(like(&tokens[5], Space::Space, " "));
        assert!(like(&tokens[6], Literal::NumberLiteral, "10"));
        assert!(like(&tokens[7], Punctuation::Semicolon, ";"));
    }

    // @lfy def/lexer/main.lfy:73
    #[test]
    fn test_a_template_with_an_execution() {
        let tokens = tokens("`a {{b}} c`;");
        assert_eq!(tokens.len(), 8);
        assert!(like(&tokens[0], Literal::Backtick, "`"));
        assert!(like(&tokens[1], Literal::TemplateBody, "a "));
        assert!(like(&tokens[2], Literal::ExecutionOpen, "{{"));
        assert!(like(&tokens[3], Identifier::Identifier, "b"));
        assert!(like(&tokens[4], Literal::ExecutionClose, "}}"));
        assert!(like(&tokens[5], Literal::TemplateBody, " c"));
        assert!(like(&tokens[6], Literal::Backtick, "`"));
        assert!(like(&tokens[7], Punctuation::Semicolon, ";"));
    }

    // @lfy def/lexer/main.lfy:86
    #[test]
    fn test_a_single_quoted_string_with_an_escaped_quote() {
        let tokens = tokens("'it\\'s'");
        assert_eq!(tokens.len(), 3);
        assert!(like(&tokens[0], Literal::SingleQuote, "'"));
        assert!(like(&tokens[1], Literal::SingleQuoteBody, "it's"));
        assert!(like(&tokens[2], Literal::SingleQuote, "'"));
    }

    // @lfy def/lexer/main.lfy:21
    #[test]
    fn source_is_read_as_utf8_from_start_to_end() {
        let tokens = tokens("é ← 日本");
        assert_eq!(
            tokens.iter().map(|t| t.rule).collect::<Vec<_>>(),
            vec![
                IDENTIFIER,
                SPACE,
                punctuation(Punctuation::SingleArrowLeftGlyph),
                SPACE,
                IDENTIFIER
            ]
        );
        assert_eq!(
            tokens.iter().map(|t| t.column).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(
            raws("let x = 1;"),
            vec!["let", " ", "x", " ", "=", " ", "1", ";"]
        );
    }

    // @lfy def/lexer/main.lfy:22
    #[test]
    fn file_is_recorded_on_every_token_and_defaults_to_anonymous() {
        assert!(
            lex("a b", Some("lib.lfy"))
                .unwrap()
                .iter()
                .all(|t| &*t.file == "lib.lfy")
        );
        assert!(tokens("a b").iter().all(|t| &*t.file == "anonymous"));
        assert_eq!(ANONYMOUS_FILE, "anonymous");
    }

    // @lfy def/lexer/main.lfy:24
    #[test]
    fn both_line_break_spellings_are_new_line_tokens_with_the_same_value() {
        let lf = tokens("a\nb");
        let crlf = tokens("a\r\nb");
        assert_eq!(
            lf.iter().map(|t| t.rule).collect::<Vec<_>>(),
            vec![IDENTIFIER, NEW_LINE, IDENTIFIER]
        );
        assert_eq!(
            lf.iter().map(|t| &t.value).collect::<Vec<_>>(),
            crlf.iter().map(|t| &t.value).collect::<Vec<_>>()
        );
        assert_eq!(crlf[1].raw, "\r\n");
        assert_eq!(crlf[1].value, "\n");
        assert_eq!((crlf[2].line, crlf[2].column), (2, 0));
        assert_eq!((lf[2].line, lf[2].column), (2, 0));
        // A lone carriage return is no line break.
        let cr = tokens("a\rb");
        assert_eq!(cr[1].rule, INVALID);
        assert_eq!((cr[2].line, cr[2].column), (1, 2));
    }

    // @lfy def/lexer/main.lfy:25
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
    }

    // @lfy def/lexer/main.lfy:27
    #[test]
    fn raw_is_the_original_text_and_value_is_adjusted_only_where_declared() {
        let tokens = tokens("\"a\\nb\"");
        assert_eq!(tokens[1].raw, "a\\nb");
        assert_eq!(tokens[1].value, "a\nb");
        assert_eq!(values("1_000.5"), vec!["1000.5"]);
        assert_eq!(raws("1_000.5"), vec!["1_000.5"]);
        assert_eq!(values("if x"), vec!["if", " ", "x"]);
    }

    // @lfy def/lexer/main.lfy:28
    #[test]
    fn a_new_mode_stack_is_created_for_each_call() {
        assert_eq!(tokens("`").len(), 2);
        assert_eq!(rules_of("x"), vec![IDENTIFIER]);
        assert_eq!(rules_of("}"), vec![punctuation(Punctuation::BlockClose)]);
    }

    // @lfy def/lexer/main.lfy:29
    #[test]
    fn candidates_depend_on_the_mode_at_the_top_of_the_stack() {
        // `{{` outside a template is two block opens.
        assert_eq!(
            rules_of("{{}}"),
            vec![
                punctuation(Punctuation::BlockOpen),
                punctuation(Punctuation::BlockOpen),
                punctuation(Punctuation::BlockClose),
                punctuation(Punctuation::BlockClose)
            ]
        );
        // Inside an execution, ordinary tokens are read; `}}` is the longest match there,
        // so it closes the execution before a lone `}` is read.
        assert_eq!(
            rules_of("`{{{}}}`"),
            vec![
                literal(Literal::Backtick),
                literal(Literal::ExecutionOpen),
                punctuation(Punctuation::BlockOpen),
                literal(Literal::ExecutionClose),
                literal(Literal::TemplateBody),
                literal(Literal::Backtick)
            ]
        );
        assert_eq!(
            rules_of("`{{ {} }}`")[2..6],
            [
                SPACE,
                punctuation(Punctuation::BlockOpen),
                punctuation(Punctuation::BlockClose),
                SPACE
            ]
        );
        // `]]` closes the reference the same way; `[[` inside it is two list opens.
        assert_eq!(
            rules_of("`[[a[0]]]`"),
            vec![
                literal(Literal::Backtick),
                literal(Literal::ReferenceOpen),
                IDENTIFIER,
                punctuation(Punctuation::ListOpen),
                literal(Literal::NumberLiteral),
                literal(Literal::ReferenceClose),
                literal(Literal::TemplateBody),
                literal(Literal::Backtick)
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
        // Text modes only see their body and boundaries.
        assert_eq!(values("'{{x}} // y'"), vec!["'", "{{x}} // y", "'"]);
        assert_eq!(values("\"[[x]] /* y\""), vec!["\"", "[[x]] /* y", "\""]);
        assert_eq!(values("`a}}b]]c`"), vec!["`", "a}}b]]c", "`"]);
        assert_eq!(values("/* ' ` \" // */"), vec!["/*", " ' ` \" // ", "*/"]);
        assert_eq!(values("// /* */ /** **/"), vec!["//", " /* */ /** **/"]);
    }

    // @lfy def/lexer/main.lfy:30
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
            rules_of("<="),
            vec![punctuation(Punctuation::LessThanOrEqual)]
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
    }

    // @lfy def/lexer/main.lfy:31
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

    // @lfy def/lexer/main.lfy:38
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
        // A code point that is not a scalar value is kept as written.
        assert_eq!(
            values("\"\\uD800 \\uFFFFFF\""),
            vec!["\"", "\\uD800 \\uFFFFFF", "\""]
        );
        // Every other token's value is its raw text.
        assert_eq!(values("/** d **/"), vec!["/**", " d ", "**/"]);
    }

    // @lfy def/lexer/main.lfy:46
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
        // A run stops at a line break that ends the mode at the top.
        assert_eq!(values("/// a]]\nb"), vec!["///", " a", "]", "]", "\n", "b"]);
        // Sequences are not recognised outside strings and templates.
        assert_eq!(rules_of("\\n"), vec![INVALID, IDENTIFIER]);
    }

    // @lfy def/lexer/main.lfy:50
    #[test]
    fn modes_left_open_at_the_end_become_invalid_tokens_naming_them() {
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
            let last = tokens.last().unwrap();
            assert_eq!(last.line, 1, "{source:?}");
            assert_eq!(last.column, source.chars().count(), "{source:?}");
        }
        // Modes that end with the line also end with the source.
        for source in ["'abc", "\"abc", "\"abc\n", "// x", "/// x"] {
            assert!(
                tokens(source).iter().all(|t| !t.raw.is_empty()),
                "{source:?}"
            );
        }
        assert!(tokens("(").iter().all(|t| !t.is_invalid()));
    }

    // @lfy def/lexer/main.lfy:107
    #[test]
    fn a_keyword_beats_an_identifier_of_the_same_text() {
        for &rule in crate::grammar::terminals::keyword::KEYWORDS {
            let Entity::Keyword(keyword) = rule else {
                unreachable!()
            };
            let text = keyword.word().unwrap();
            assert_eq!(rules_of(text), vec![Some(rule)], "{rule}");
            let quoted = format!("'{text}'");
            assert_eq!(
                rules_of(&quoted)[1],
                literal(Literal::SingleQuoteBody),
                "{rule}"
            );
        }
        assert_eq!(rules_of("d"), vec![keyword(Keyword::AgentDataKeyword)]);
        assert_eq!(rules_of("true"), vec![keyword(Keyword::TrueKeyword)]);
        assert_eq!(rules_of("with"), vec![keyword(Keyword::WithKeyword)]);
        assert_eq!(rules_of("constant"), vec![IDENTIFIER]);
    }

    // @lfy def/lexer/main.lfy:110
    #[test]
    fn escapes_are_never_tokens() {
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

    // @lfy def/lexer/main.lfy:98
    #[test]
    fn the_lexer_matches_against_the_terminal_document_only() {
        let document = terminals();
        assert_eq!(document, grammar::terminal_document());
        for (terminal, _) in candidates() {
            assert!(document.contains(terminal.text()), "{terminal}");
        }
        assert!(!document.contains("Expression ="));
    }

    // @lfy def/grammar/terminals/comment.lfy:12
    #[test]
    fn block_documentation_open_followed_by_a_slash_or_close_is_a_block_comment() {
        assert_eq!(
            rules_of("/**/"),
            vec![
                comment(Comment::BlockCommentOpen),
                comment(Comment::BlockCommentClose)
            ]
        );
        assert_eq!(values("/***/"), vec!["/*", "*", "*/"]);
        assert_eq!(rules_of("/***/")[1], comment(Comment::BlockCommentBody));
        assert_eq!(rules_of("/**/x")[2], IDENTIFIER);
        assert_eq!(values("/** x **/"), vec!["/**", " x ", "**/"]);
        // Comments nest; documentation does not.
        assert_eq!(
            rules_of("/* a /* b */ c */"),
            vec![
                comment(Comment::BlockCommentOpen),
                comment(Comment::BlockCommentBody),
                comment(Comment::BlockCommentOpen),
                comment(Comment::BlockCommentBody),
                comment(Comment::BlockCommentClose),
                comment(Comment::BlockCommentBody),
                comment(Comment::BlockCommentClose)
            ]
        );
        assert_eq!(
            values("/** a /** b **/"),
            vec!["/**", " a ", "/", "** b ", "**/"]
        );
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
        assert_eq!(rules_of("`{{ /** d **/ }}`")[3], comment(Comment::BlockDocumentationOpen));
        assert_eq!(
            rules_of("`{{ // c\n}}`")[3..5],
            [punctuation(Punctuation::Slash), punctuation(Punctuation::Slash)]
        );
        assert_eq!(rules_of("`{{ /// c }}`")[3], punctuation(Punctuation::Slash));
        // Inside a reference neither kind of comment is a candidate.
        assert_eq!(rules_of("`[[ /* c ]]`")[3], punctuation(Punctuation::Slash));
        assert_eq!(rules_of("`[[ // c ]]`")[3], punctuation(Punctuation::Slash));
    }

    // @lfy def/lexer/traits.lfy:37
    #[test]
    fn line_modes_end_before_the_line_break() {
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
        assert_eq!(
            rules_of("'ab\ncd'"),
            vec![
                literal(Literal::SingleQuote),
                literal(Literal::SingleQuoteBody),
                NEW_LINE,
                IDENTIFIER,
                literal(Literal::SingleQuote)
            ]
        );
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

    // @lfy def/lexer/main.lfy:21
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
            let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../").to_string() + path).unwrap_or_else(|e| panic!("{path}: {e}"));
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
