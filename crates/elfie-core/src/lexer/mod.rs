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
// @lfy def/lexer/main.lfy:lex
pub const ANONYMOUS_FILE: &str = "anonymous";

/// `ace function matches(rule)`: the characters at `offset` match the rule's syntax.
/// Yields the byte length of the longest such match.
// @lfy def/lexer/main.lfy:matches
fn matches(rule: Entity, source: &str, offset: usize) -> Option<usize> {
    rule.longest_match_at(source, offset)
}

/// The terminal document is the only grammar the lexer knows: the EBNF of every terminal.
// @lfy def/lexer/main.lfy:lex
pub fn terminals() -> String {
    grammar::terminal_document()
}

/// Every terminal the lexer considers, with its lex condition. Escapes are matched only
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
                // @lfy def/lexer/traits.lfy:appliedUntilNewLine
                traits::before_line_break(&mut self.stack);
            }
            match self.candidate()? {
                // @lfy def/lexer/main.lfy:lex
                Some((terminal, len)) => {
                    self.push(Some(terminal), len);
                    traits::on_token(terminal, &mut self.stack); // @lfy def/lexer/traits.lfy:modeOpener
                }
                // @lfy def/lexer/main.lfy:lex
                None => {
                    let len = self.invalid_run();
                    self.push(None, len);
                }
            }
        }
        // @lfy def/lexer/traits.lfy:appliedUntilNewLine
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
    // @lfy def/lexer/main.lfy:push
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:73a7a4972a85e8de228731064674ba62d23fc532c2c7fc64b823ebde817d129e
    #[test]
    fn test_empty_source_gives_no_tokens() {
        assert_eq!(tokens(""), Vec::<Token>::new());
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:7b9f6253aa23103ccfefec7e416516c2ab9ec93261faf9a4d353cf5811c2328b
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:17d7117b296e91576aded6aa38378d5018ad738ecf5f0ead9b23b2e82a4e0233
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:893b54f62abaf4b2b38de8b0456315205a0488651632fad8b13e9e28b143f889
    #[test]
    fn test_a_single_quoted_string_with_an_escaped_quote() {
        let tokens = tokens("'it\\'s'");
        assert_eq!(tokens.len(), 3);
        assert!(like(&tokens[0], Literal::SingleQuote, "'"));
        assert!(like(&tokens[1], Literal::SingleQuoteBody, "it's"));
        assert!(like(&tokens[2], Literal::SingleQuote, "'"));
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:23e721f4e5fa98b0777a5953702989904d09b8cf76601f7f2668c04a8e8c2f7d
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:97a338cd1051f99fec01edad49bbc0c0319cc71cded2d210a9f5ac5b83cd3d52
    // @lfy def/lexer/main.lfy:lex#lex:lex:aa56ac92787fea3855fb1b3f31628f68bbb6ec38ce590e7cb32cd1dfd604caab
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:e596ea1a811f6d4f218899f6ecb7527b53077617a2c318449f925711429adacd
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:47847482a3e68ce6f0a26ef9d0fd372da3498cc761a6fa31445d5b177a9c349c
    // @lfy def/lexer/main.lfy:lex#lex:lex:4720c1a90961a30124edd2bd63f5a331691b9e097486fec98ffe60b06e9fe622
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:f9db63b38441ea1e8401410a35a846620d3039b3b962ef1ba140575c096d2249
    #[test]
    fn raw_is_the_original_text_and_value_is_adjusted_only_where_declared() {
        let tokens = tokens("\"a\\nb\"");
        assert_eq!(tokens[1].raw, "a\\nb");
        assert_eq!(tokens[1].value, "a\nb");
        assert_eq!(values("1_000.5"), vec!["1000.5"]);
        assert_eq!(raws("1_000.5"), vec!["1_000.5"]);
        assert_eq!(values("if x"), vec!["if", " ", "x"]);
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:20d04427ad0b528e56b30362f8f0060de1781d11a657c6d6788c671c0e5c66dc
    #[test]
    fn a_new_mode_stack_is_created_for_each_call() {
        assert_eq!(tokens("`").len(), 2);
        assert_eq!(rules_of("x"), vec![IDENTIFIER]);
        assert_eq!(rules_of("}"), vec![punctuation(Punctuation::BlockClose)]);
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:77c966508bfe60769e47819e1fb6191fd0cce43beb32749773eb8b9b4ab7a851
    #[test]
    fn a_terminal_with_a_lex_condition_is_matched_where_that_condition_holds() {
        // `{{` is a candidate in a template and `}}` in an execution, so `}}` closes the
        // execution before a lone `}` is read; the template body is read between them.
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
        // `]]` closes the reference the same way.
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
        // A backtick names four modes, and is matched in every one of them: in code, in
        // an execution, in a reference, and in the template it closes.
        assert_eq!(
            modes::lex_condition(Entity::Literal(Literal::Backtick)),
            &[Mode::Code, Mode::Execution, Mode::Reference, Mode::Template]
        );
        assert_eq!(
            rules_of("`{{`x`}}`"),
            vec![
                literal(Literal::Backtick),
                literal(Literal::ExecutionOpen),
                literal(Literal::Backtick),
                literal(Literal::TemplateBody),
                literal(Literal::Backtick),
                literal(Literal::ExecutionClose),
                literal(Literal::Backtick)
            ]
        );
        assert_eq!(rules_of("`[[`x`]]`")[2], literal(Literal::Backtick));
        // Text modes only see their body and boundaries.
        assert_eq!(values("'{{x}} // y'"), vec!["'", "{{x}} // y", "'"]);
        assert_eq!(values("\"[[x]] /* y\""), vec!["\"", "[[x]] /* y", "\""]);
        assert_eq!(values("`a}}b]]c`"), vec!["`", "a}}b]]c", "`"]);
        assert_eq!(values("/* ' ` \" // */"), vec!["/*", " ' ` \" // ", "*/"]);
        assert_eq!(values("// /* */ /** **/"), vec!["//", " /* */ /** **/"]);
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:3489f51bad43ed1c292aad6ac0104638551823e0b16a5c5b73e77b68eb7ce19f
    #[test]
    fn a_terminal_without_a_lex_condition_is_matched_wherever_in_code_holds() {
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
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:476bbc6035817b8576db1356118eea406a548b1ae4855e77cf8a93b4c4ec747e
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:22ed1b3c87d41fd6f642e721955f58157a0c01372f21803ca50fe7b8608f86e5
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:f4449978ffdf9eed5f6ec63143a03670875c20038df3324769edc39e6fb8dcc8
    // @lfy def/lexer/main.lfy:lex#lex:lex:09b1ff781edc31ee1df5c0f4640f90f022130351375a8ba444e28b0104082d78
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:24de947d6f7166dda5bb4e493e06cfaa44487312753d77f6f1693b270ebb6b44
    // @lfy def/lexer/main.lfy:lex#lex:lex:0de07769ae8853d0916c070c9b60d8a8b70f4bffa9e1170f6370cc0c4e466a71
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:0a624895048d41f174c726e78fddc0185a3474d7115dc5fa123eb01718b59e90
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:d5834e260284fe7ecb1d20035815676dd0f173ab2b5229f0c641ec4ac1ae8b3a
    // @lfy def/lexer/main.lfy:lex#lex:lex:377c03a0122f86f45edc9430b78d345e1f98567c82a7413b455af636c8abe4b0
    // @lfy def/lexer/main.lfy:lex#lex:lex:45164776c29697d5fc8d4bd51a113b8ff5ceb1d87c7cb53993649fdb2b3b68c4
    // @lfy def/lexer/main.lfy:lex#lex:lex:cff78a579165cf2519e72409a4123e4cb0065919bd0e4c5ef616bdec39d99d97
    // @lfy def/lexer/main.lfy:lex#lex:lex:7d9772b4420a275ee58f4637d5ee1b8974d3d8339ff3d4272ef4af8d8f2cc71b
    // @lfy def/lexer/main.lfy:lex#lex:lex:8a46c8942af051808c3c334bddf5d25f00e558d1b2b98517aea7e42d6d69a97c
    // @lfy def/lexer/main.lfy:lex#lex:lex:29420d87577dc9e7de99e48b7fdfd5a93d2a8ea270130016e7f0541209de733c
    // @lfy def/lexer/main.lfy:lex#lex:lex:d4395990e5b51ad8ee2a77b2cfeb11be57d571876dd08e620d561d0d88a0f28b
    // @lfy def/lexer/main.lfy:lex#lex:lex:550e26f0ddfb2eaa0cd1820af7b042aeb0b522f4797b64edb41199c72d67afb1
    // @lfy def/lexer/main.lfy:lex#lex:lex:a336a1c410be065b30406b364f019a9f286db4ab6002161e6fcf3c9227637871
    // @lfy def/lexer/main.lfy:lex#lex:lex:d1cb7c4ec2b611781f98906a83d942a7916015983e6c56d1337a6c5ab6ebda57
    // @lfy def/lexer/main.lfy:lex#lex:lex:2132f5b87ed1bf096f3852daf5a3c8efd8d0898378b74ea7a58afcba1bb66076
    // @lfy def/lexer/main.lfy:lex#lex:lex:88f8e5435327e4c7b72907218c31c347718fbf2711dbf7165f6c7a366302cbae
    // @lfy def/lexer/main.lfy:lex#lex:lex:48993e60911b25a4a6d37d7aced8650a35289b67c7b9b33d1563a8b028566f03
    // @lfy def/lexer/main.lfy:lex#lex:lex:6a5147ae5e71ae715fa5c75604ec0c764c62e1caf86955f6019f04024fedc566
    // @lfy def/lexer/main.lfy:lex#lex:lex:670632c92a526c0c9ba544b267583db2f989850f0daeca28ef703cf2c0704946
    // @lfy def/lexer/main.lfy:lex#lex:lex:f158a9e2c48558f095590b24febdbade19af37c1c484f04e2d8cfbe45e6952f8
    // @lfy def/lexer/main.lfy:lex#lex:lex:c81c6cb6869c83d022bdb47b1caf6dc5659e95a197a0cce53e87a1078bbcb5db
    // @lfy def/lexer/main.lfy:lex#lex:lex:308f19648d7d19af29b98b1d07616c7f1d97d32c33278fd0d0915f7bbef2b535
    // @lfy def/lexer/main.lfy:lex#lex:lex:d6bac6682556dc19a37f90caedd3b87e9948eb46527d9551c6ec96cae3121090
    // @lfy def/lexer/main.lfy:lex#lex:lex:1abd8f373747b386fb3e592c35d3d87436775929fb95e4766e7babb7476b18eb
    // @lfy def/lexer/main.lfy:lex#lex:lex:063d1dbe5cc5f5ee5adf00b9853ee43909ebbbb2e96eb212bddd01854f7c5ac2
    // @lfy def/lexer/main.lfy:lex#lex:lex:2b84395241cfb2080430ba34d52e8a80649625f3623591df3aa55237526592df
    // @lfy def/lexer/main.lfy:lex#lex:lex:0da43a8c41ec9dce2a4b1468355519f3cf7d9b6de9965c2003db8a32edc79942
    // @lfy def/lexer/main.lfy:lex#lex:lex:854c801cc4be5c0ba53f54321c38d8ff50aaf246917b2eb13f5923d837d47833
    // @lfy def/lexer/main.lfy:lex#lex:lex:55a7ac252194d30e70b0b9015e4253dbac39ac4d1e03ce6d6c29b9775343c1dd
    // @lfy def/lexer/main.lfy:lex#lex:lex:0c414ce91f6de173ff4dc217ee9e4fde05f3de48b1d15710e30ed9e6287d478d
    // @lfy def/lexer/main.lfy:lex#lex:lex:1261710074b5da49ab13656419fba84d00515288871cc36cfa30d4f1027a8e6d
    // @lfy def/lexer/main.lfy:lex#lex:lex:ccb5d0a85036782e1029f7fa2c6d2285b58f67b77d1865d59c2966402ac97ab8
    // @lfy def/lexer/main.lfy:lex#lex:lex:7943f4b413655deea88e77c2f7636070c7d2b11c865bb4235de9bd607b924326
    // @lfy def/lexer/main.lfy:lex#lex:lex:0e54d81cdd684b4a82af11eb141098758514a388a03ab8be1202d607b6546a76
    // @lfy def/lexer/main.lfy:lex#lex:lex:8c5ab07ce0b4543c71474cdfcbd37bef1dcd5a523f2002b8f7651f86b9d2ea9c
    // @lfy def/lexer/main.lfy:lex#lex:lex:3e5390f5a70e9ec6be683f47fa339d1fe16829d511fe4f4bd96fdabf4a2fb0dc
    // @lfy def/lexer/main.lfy:lex#lex:lex:d105db652dad56f3195eaef8e28546b114d0c0a3af486c6880cd3e268e400ef2
    // @lfy def/lexer/main.lfy:lex#lex:lex:4a665157d2fd00211473980cfed31ca836a7bc8fc9e191570290a8bdddff8fdf
    // @lfy def/lexer/main.lfy:lex#lex:lex:d06290b82fc56a13448c0da9b935b2799c6748bc8f90b80be441463274c8578b
    // @lfy def/lexer/main.lfy:lex#lex:lex:e4722204e7812ffc9e4ee15cdbad4af6dca8f6edda67f8e31d96a3170922008f
    // @lfy def/lexer/main.lfy:lex#lex:lex:d56737e7ed82c0f4c740efb019f709e1196d2f570505a43afdd6e8c60c810f9d
    // @lfy def/lexer/main.lfy:lex#lex:lex:c7ab6c897f29f8ae3bc230942c4967f1a181b79ead232c72c157ccd0c5eb1c89
    // @lfy def/lexer/main.lfy:lex#lex:lex:82f88b11448889894551dcb911a0f34ddf229f435c0044c82f50dc4e5848590d
    // @lfy def/lexer/main.lfy:lex#lex:lex:848908d248ad26c673441bb9f629852f4691e1ca1a60a8ad0a59b086ca9b544f
    // @lfy def/lexer/main.lfy:lex#lex:lex:8692f8351b9c4affaebe61c136c955aa227014909f00e55f4d023e82d537487d
    // @lfy def/lexer/main.lfy:lex#lex:lex:4a42ab43cc22e2e6bafc184df1a61ca5b4b1d05ae33378f0270df9fdcc332ae2
    // @lfy def/lexer/main.lfy:lex#lex:lex:164de2b3d8760e7acf99e5fe9fff5be234036153b2d80358f811b889b19c5d8d
    // @lfy def/lexer/main.lfy:lex#lex:lex:f85f8d804052f09a5a945cd54e761e526a7f909e9aca3da5937e6c5c59b95b5f
    // @lfy def/lexer/main.lfy:lex#lex:lex:d1e246e82c77a6e9eb1cd53874af4970d96d105f974ad40c6c54a1c913cabf39
    // @lfy def/lexer/main.lfy:lex#lex:lex:33f25ff224e8cbe5a24aa0143be395baaa7999b1bacad955b945659d7cba5949
    // @lfy def/lexer/main.lfy:lex#lex:lex:60e642a244d6fdfa17ff9705f8cdf78c6d3b104134d8e507b0c1b835deea281a
    // @lfy def/lexer/main.lfy:lex#lex:lex:b0dd5ea59fc2c22ffbab2cf17b43ceaa07437958a88eb5d32a5c5c4d44423e9d
    // @lfy def/lexer/main.lfy:lex#lex:lex:cfb4683ddfce6ca16552d62b96eb5202d3e6f125da9e1b336c3e1ca84a46afee
    // @lfy def/lexer/main.lfy:lex#lex:lex:811db0a069fbd1a08588bac0faf507d805a12e5f7d4599808fd4d1998397b965
    // @lfy def/lexer/main.lfy:lex#lex:lex:a61e68413ea5e5f90dcb2ee72f0d7a12e737308cabdf4ecffbae20d2c5ba58a0
    // @lfy def/lexer/main.lfy:lex#lex:lex:bf65c300b853514e4e33f60fac46ce298910b4c0683dff0cc7c0fc8c15a139f8
    // @lfy def/lexer/main.lfy:lex#lex:lex:d1a41dd5ae4373d81c1d4a2de7858348c8b88c2e2449dee29b8a795e1b37d9b6
    // @lfy def/lexer/main.lfy:lex#lex:lex:1a39f3ac6f7d0e836cd481845255e577539a02eba26748f2d45b856b5e09c674
    // @lfy def/lexer/main.lfy:lex#lex:lex:5d5a80739fc81592f6a21d063aed0b3a0c9e64f10fdec3a585e5fea704bd1dc3
    // @lfy def/lexer/main.lfy:lex#lex:lex:2f9dd18560b0651be33b696bc272d194b86da3c652de17fe9fb7b8fe1ccb3a65
    // @lfy def/lexer/main.lfy:lex#lex:lex:624687f65c4d79e19352bd975f2f6d35d28f8c8b2588e889023e89529b1dd37c
    // @lfy def/lexer/main.lfy:lex#lex:lex:72a2bce21a84fd7d1b5bef614eac52a191b411933b97527384d967c2328c44b2
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:2
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:3
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:4
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:5
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:6
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:7
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:8
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:9
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:10
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:11
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:12
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:13
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:14
    // @lfy def/lexer/main.lfy:lex#lex:lex:61bda96c5031d4739a062049b7a3783d5880e9d95d4bc2a7d054017e1b4c3103:15
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:4f82f81fee7df5d1f1456b33a20e33c3e124f0a730bc36d3ead5162850b4eb4f
    // @lfy def/lexer/main.lfy:lex#lex:lex:96f3ebf394d27ee519580d17930046716c6bb98571c66276b16a2f1700506748
    #[test]
    fn the_lexer_matches_against_the_terminal_document_only() {
        let document = terminals();
        assert_eq!(document, grammar::terminal_document());
        for (terminal, _) in candidates() {
            assert!(document.contains(terminal.text()), "{terminal}");
        }
        assert!(!document.contains("Expression ="));
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

    // @lfy def/lexer/traits.lfy:appliedUntilNewLine
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
