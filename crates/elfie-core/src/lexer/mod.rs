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

    /// In every mode its condition names, the terminal matches its characters and the
    /// token made of exactly those characters is appended to the result.
    fn becomes_a_token(terminal: Entity) {
        let text = sample(terminal);
        for &mode in modes::lex_condition(terminal) {
            let (open, before) = opening(terminal, mode);
            let source = format!("{open}{text}");
            let tokens = tokens(&source);
            assert_eq!(tokens[before].rule, Some(terminal), "{terminal} in {mode}");
            assert_eq!(tokens[before].raw, text, "{terminal} in {mode}");
        }
    }

    /// Every terminal of one grammar file; an escape is never a token and is left out.
    fn family(of: fn(Entity) -> bool) -> Vec<Entity> {
        Entity::terminals()
            .filter(|terminal| !terminal.is_escape() && of(*terminal))
            .collect()
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

    // @lfy def/lexer/main.lfy:lex#lex:lex:cec5b0060b35e8278d72b923ec0d43308f06aa7d3668d833f0f9543aa35f0b6b
    // @lfy def/lexer/main.lfy:lex#lex:lex:dc8480fe1b132653dbafa761efd05a3e5de6a1ad73c439ff76ccf86eb41b4e0c
    // @lfy def/lexer/main.lfy:lex#lex:lex:c5bbb7eab1368457e4aec54a5d34a062284dc171e493d247f2e7347ebc7e19cc
    // @lfy def/lexer/main.lfy:lex#lex:lex:3870fb344a829a1af69b0a051b53955689850462cc23fc364a34bb2aca8183b2
    // @lfy def/lexer/main.lfy:lex#lex:lex:6707e1c4e79aefdfa69631baf04ef189a740b8f901f8b529fc5e07390bfc5f94
    // @lfy def/lexer/main.lfy:lex#lex:lex:f268de00ddab8ce2b5f7d871d904069096ac91e04a60af6b4ce2a422fa1c1a95
    // @lfy def/lexer/main.lfy:lex#lex:lex:654f9e6f867247c5d5d574b2f07d46c36a8ef6c404b2577b327b7ef43c715b93
    // @lfy def/lexer/main.lfy:lex#lex:lex:efa50aa9537ca0b975faaad4e0030362a65de998b5250e05b22beed0a7368cee
    // @lfy def/lexer/main.lfy:lex#lex:lex:7f5893c6309820127c3f11b74d3e44c52c568ff3b795d36828de9c7033d9cffd
    // @lfy def/lexer/main.lfy:lex#lex:lex:3c1f25e9fb379f2a43e4eebfae1a8608aed01bf47ac801515d43cd72a4b9aa00
    // @lfy def/lexer/main.lfy:lex#lex:lex:2fa5a498ae9ec85be4e1c6d1e06a460dfea9c6ac546fc121bcc72b72e1dc53de
    // @lfy def/lexer/main.lfy:lex#lex:lex:5105638fa5bad15963d1dd1c8d43e507f9bb858ee46561da094e4f3084b8b699
    // @lfy def/lexer/main.lfy:lex#lex:lex:bfb075cfeccf95e83562a04929353aaad00243234c5f3068d79bcefe0d5d8fac
    // @lfy def/lexer/main.lfy:lex#lex:lex:bab1de6ccbb20015f5334ad45d95f0ecbbad634ef1d422e7087145f42149992b
    // @lfy def/lexer/main.lfy:lex#lex:lex:b762c56cacf0a8824211b65c00bdd11e3d1b5b3ebe28b43fc81bfcadfbd23dcb
    // @lfy def/lexer/main.lfy:lex#lex:lex:f182ef90231cda6ee064965b1fea8939cd39f57f5c4a38685ae1c84f633affe6
    // @lfy def/lexer/main.lfy:lex#lex:lex:39a4450c9224d969dcb4486c8c0cd1e1c0f55548cac4317e5c95e4caf4d57637
    // @lfy def/lexer/main.lfy:lex#lex:lex:e821f7046640aca4de7c9dcfb76ccd0fa9c33607a8ae0df4a17ec0651f08b673
    // @lfy def/lexer/main.lfy:lex#lex:lex:211320aebaf874052e03549ce2993cdaf5fefedb24a3947db7e72cbedfa5d90e
    // @lfy def/lexer/main.lfy:lex#lex:lex:4b528f652bd89b67ae315344fc84e7e7b8efa85493ccd53768233029c71b2b48
    // @lfy def/lexer/main.lfy:lex#lex:lex:2e7ceec342fd410345c41ac7dd5f636a2aaa55f2be2f17e48b61492e63e97044
    // @lfy def/lexer/main.lfy:lex#lex:lex:9529f129b5faa025211caf090d1d50026bbc4a9757e5770616f06a3362cc5d0c
    // @lfy def/lexer/main.lfy:lex#lex:lex:dbf33729c1c4c10fcc57d1468c54b995c32049f2dc92dc471997f1c5c9853be2
    // @lfy def/lexer/main.lfy:lex#lex:lex:745e2245316a76b622bc5600d043ecba63e15668b085efea8670a3e8abb5d0b8
    // @lfy def/lexer/main.lfy:lex#lex:lex:4ccc7eb9b187408dc67c78823499788a8dcacb4ee11e4d73a630dadce3499139
    // @lfy def/lexer/main.lfy:lex#lex:lex:e73feba6fff2953097ca9846a635e6d2c36553605ff5798f3a4b45bbf513ffd6
    // @lfy def/lexer/main.lfy:lex#lex:lex:7040eeef7974ee8eaeebb09658f72f54380699d34a04d5bfb8f0fd2f0a229e12
    // @lfy def/lexer/main.lfy:lex#lex:lex:68372814b45240f6aa81688616a72b52475b641e27fb652e85297d7945d21a04
    // @lfy def/lexer/main.lfy:lex#lex:lex:b76cc443143090d99bab21cf30b9ca0a6a770947dc31a76d6d26a48cdfc71b5b
    // @lfy def/lexer/main.lfy:lex#lex:lex:703a2a95ef61e83ffcb052addb946c3f56e92b674fc260a4ed422903954a9642
    // @lfy def/lexer/main.lfy:lex#lex:lex:2f5085f396359f7af20542ccbaf8b2c5e7ef7978eb4abf4cf6fbeb1679f58432
    // @lfy def/lexer/main.lfy:lex#lex:lex:325c2d2c91d44c14aa427480ccaa60016212be139b6bce97f08c24bab54e4d39
    // @lfy def/lexer/main.lfy:lex#lex:lex:14869dd25e0efc9ce425c466211d4226b8cdbbf0e6dccf7a27f18f05ba6abcac
    // @lfy def/lexer/main.lfy:lex#lex:lex:585c3a1937a05ec3fc87a96b60bc027704e116498eb3d9edf1cedd05a97310e3
    // @lfy def/lexer/main.lfy:lex#lex:lex:a64cb890d435a4942bd0b8ae17a55a7c8227fba53aa0784f6883379bb1044817
    // @lfy def/lexer/main.lfy:lex#lex:lex:7e650428e42ded8a9bb8a350213d4a6324b63a73fa89d87790323eb3dc0d490c
    // @lfy def/lexer/main.lfy:lex#lex:lex:625c75b7f4be1913a472badb53f81bf420b396b2d4a4ebd21ff6f78c63381164
    // @lfy def/lexer/main.lfy:lex#lex:lex:4956ba53801967d51706431894ea869c45db6a67e31b4994dce5e7ad34a15af8
    // @lfy def/lexer/main.lfy:lex#lex:lex:cf17acb09e6e4865824242b4d7540b584df2a37594eed987ae530507520c6b27
    // @lfy def/lexer/main.lfy:lex#lex:lex:43ef62873e5d18ccff63df3e3c948a2521ceac33ffd1318db516996ab542cc1c
    // @lfy def/lexer/main.lfy:lex#lex:lex:b1e22ab29fa91a335f6e6a1dc90add5e6dd7411e0cb26be51e341bb511feaf33
    // @lfy def/lexer/main.lfy:lex#lex:lex:90fd4b3ef79f60356529c9dd346fc831a76847e1055db75f0470718722d7dffa
    // @lfy def/lexer/main.lfy:lex#lex:lex:d2999142cb65695b13d3f704eb33fb817b58e8ae804e58870ff6a53df074a217
    // @lfy def/lexer/main.lfy:lex#lex:lex:7c06052b2d02c497166243e250c59210a38b0a796bdf16d76962d8383902c5f9
    // @lfy def/lexer/main.lfy:lex#lex:lex:a8a0598642a41276bf6cdc2e271c5a71d5b280f4ae234fa4c82b2fbae04b4f9e
    // @lfy def/lexer/main.lfy:lex#lex:lex:2a23de3561462af6c7b9a16dc8b0ac8f4878880a6c76037e973503ca80771fd4
    // @lfy def/lexer/main.lfy:lex#lex:lex:366e542b510959f94a82d203f84e593be2ae852962762ee465c01ccfbdc158ce
    // @lfy def/lexer/main.lfy:lex#lex:lex:828053fc6b6d3f4a43380ed0a0070f069378dded10f1bc0ed190102713b2d923
    // @lfy def/lexer/main.lfy:lex#lex:lex:a76c677a3347d0b2c51bb0960adbd023eeb2ef3352bb5a33ead0f54b89867779
    // @lfy def/lexer/main.lfy:lex#lex:lex:5032279d4ce135133e7410ea22f45a7d73c0e0804dad2c4413d1c5c6a693901c
    // @lfy def/lexer/main.lfy:lex#lex:lex:494b8a11519426d1132a18714ff75b478466f661fef518570f38d22cad9b8a40
    // @lfy def/lexer/main.lfy:lex#lex:lex:9e7831dfff98ff32dbee217bbe4f721927eb5833d3359577f1c1bf625c527f28
    // @lfy def/lexer/main.lfy:lex#lex:lex:96934e6b87717d07f43bd985aabc62f9095709cbe03f97bc931f1798a6b82af2
    // @lfy def/lexer/main.lfy:lex#lex:lex:ae552ecaa1d8ed0fa1e25d5ec5c04800d7ecfb2df742ad0b694d355d02df3682
    // @lfy def/lexer/main.lfy:lex#lex:lex:8af86bccb2abe314883546d08dce5cd3cc4ae1cdf70266b145cbad6d5c6bdaf1
    // @lfy def/lexer/main.lfy:lex#lex:lex:00956987f4cf192ca042f83668943e3be97dfb0846c202c1a35b2504492a06a1
    // @lfy def/lexer/main.lfy:lex#lex:lex:425d7035c288d876c979e526e92ee9a4206d296369d1ab677be1c0f08447d852
    // @lfy def/lexer/main.lfy:lex#lex:lex:4518f113b1168338dde89476e53dd2ccf64c7d71e115981edee76b1da2d0843e
    // @lfy def/lexer/main.lfy:lex#lex:lex:54d31ce6d2c8114d458967cc44e82880a69963f65003b9ae9ac1bfb7e650d1c2
    #[test]
    fn every_keyword_becomes_a_token_of_the_characters_it_matched() {
        let keywords = family(|terminal| matches!(terminal, Entity::Keyword(_)));
        assert_eq!(keywords.len(), 59);
        for terminal in keywords {
            becomes_a_token(terminal);
        }
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:daa3168d1b293c7413607a37488ed17b5a87b92bfc714f9048b4d14087fb2344
    // @lfy def/lexer/main.lfy:lex#lex:lex:7dd37cfa024fb6fac1a740126c7dc0f46d67f2fd91166a776b4c8a0f0cc47c4d
    // @lfy def/lexer/main.lfy:lex#lex:lex:a0cdbc2efe39fa2d56cce8ebf10733c70f6cfc012273e8ca285a8c8572f5349c
    #[test]
    fn a_name_and_the_space_terminals_become_tokens_of_the_characters_they_matched() {
        let names_and_space = family(|terminal| {
            matches!(terminal, Entity::Identifier(_) | Entity::Space(_))
        });
        assert_eq!(names_and_space.len(), 3);
        for terminal in names_and_space {
            becomes_a_token(terminal);
        }
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:8b360892b78b6aa6c9ad195eab10616ace9a704ed5375f1c0d22aba18caf60fa
    // @lfy def/lexer/main.lfy:lex#lex:lex:cf2a6f9d014be86e0445e2557b3fcbe990a2aa18995919050d0cf8a4b7e1cb30
    // @lfy def/lexer/main.lfy:lex#lex:lex:65302fc5ab863de18fb1182cf01ec469f778475fc0c5ab51c1493dd72d3262cc
    // @lfy def/lexer/main.lfy:lex#lex:lex:f9a340ab3af7852b982503d324546e9a7fab1b8b57b22fcdfaede46a7a2f4429
    // @lfy def/lexer/main.lfy:lex#lex:lex:465a98939ff1937ab5a7ed3db603ea7a995355f6b786f33148fa3d7c05bd3cbd
    // @lfy def/lexer/main.lfy:lex#lex:lex:f18586978975a6f9784905dfb877afddece7d47e57ca931ce1ab6d006522a2b2
    // @lfy def/lexer/main.lfy:lex#lex:lex:f4b8ffa2e336c929bb4333ca836ffd6003f6a895f9f299763cc8a9955e8d8b2b
    // @lfy def/lexer/main.lfy:lex#lex:lex:1acf7cd8c19498d0e197e5d992e7126268eb2f7392c43072f9f56fc0196899a4
    // @lfy def/lexer/main.lfy:lex#lex:lex:811e047326b17c55f0cb3fc50c7584fbc65e84a3abcfc518b676a4cfd839d867
    // @lfy def/lexer/main.lfy:lex#lex:lex:ef417c15682a43894d15be665088696926f991ceea4c597173ee7bfb67ae0fc3
    // @lfy def/lexer/main.lfy:lex#lex:lex:b6852d8499107bd75a798c7bce1699cb915c3cbca11d1772b7c699cf2803a2b1
    #[test]
    fn every_literal_becomes_a_token_of_the_characters_it_matched() {
        let literals = family(|terminal| matches!(terminal, Entity::Literal(_)));
        assert_eq!(literals.len(), 11);
        for terminal in literals {
            becomes_a_token(terminal);
        }
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:83472bab6533b4a217659d48a67e1da7bfcd11be6e3dda1c48b2135b243db4a5
    // @lfy def/lexer/main.lfy:lex#lex:lex:78291427acdf99773766ae9ad192cc3b9bbb10a8ab34a78fa379d4fe2259935f
    // @lfy def/lexer/main.lfy:lex#lex:lex:3a9c59df01513e84ab814380913cc31fff0854c5e47b4e65e25aead118fded4a
    // @lfy def/lexer/main.lfy:lex#lex:lex:903a8093f5d183eccfe5986e3476daa5ad3d543a7099350caf393a3f3d044b45
    // @lfy def/lexer/main.lfy:lex#lex:lex:3e88f0b5696d3c0a6a543845ef979270059c3e9714e3ac49bf319a8193270b4b
    // @lfy def/lexer/main.lfy:lex#lex:lex:e444d7c00cb89faa3988362c15388fe371aec18acc1e0779ad3c386d84412f27
    // @lfy def/lexer/main.lfy:lex#lex:lex:68c17153e3f0487bc920b8c733216e7abe0fd4a18ab80984da43ddb899f8d69b
    // @lfy def/lexer/main.lfy:lex#lex:lex:9cea6ce250cb1b4d4f79012fde135bef5fdd8f5e12e3001f4921fb77384622bb
    // @lfy def/lexer/main.lfy:lex#lex:lex:e18236e72352c8b5faeeccf956c4c9b0dea0c0b21f7f9efe7b9b430817dd7b3f
    // @lfy def/lexer/main.lfy:lex#lex:lex:a7f4364192a2346eac41756d5f26aa9eb8a93d5a04592e51af89de4f86d17000
    // @lfy def/lexer/main.lfy:lex#lex:lex:4ebe8a2a4771d3ed627b55a7aeaab444f376fb22f79628da719412c21046ed8d
    // @lfy def/lexer/main.lfy:lex#lex:lex:beb981f9fb6813af5cfb7611ecaac34adbf42281c606cc5c682562186a90f31d
    // @lfy def/lexer/main.lfy:lex#lex:lex:80e4ef8c84bb582b0373e2955a684ba8c4151a3f087d2358aa63332f82e06c17
    // @lfy def/lexer/main.lfy:lex#lex:lex:925648d031faa19f627a96d6250c1529280de6a405b6ca2764fba63397bbaefe
    // @lfy def/lexer/main.lfy:lex#lex:lex:ac1c1eb5a644d3c09156e719a324f8c0181fcc3a859987fd1aabb477708c4655
    // @lfy def/lexer/main.lfy:lex#lex:lex:0845c6858ea17ddfeda77891e5dca413908db526960380aa30c740a3483a8dc3
    // @lfy def/lexer/main.lfy:lex#lex:lex:938eae0816169bd58e9b355e3714ceacf2d98e0ede8e76453ac940f7fa11473b
    // @lfy def/lexer/main.lfy:lex#lex:lex:c8318c5931232e593aff431e22736b6b54f592020759bf56cddd0a77ab5c788d
    // @lfy def/lexer/main.lfy:lex#lex:lex:20123c5a0d16a3c63c22ce60f4e25f8c37902fe4a06c45ee71b7b8350f807381
    // @lfy def/lexer/main.lfy:lex#lex:lex:d50dfd712d17827bad12d13307d78d6c0976a09e3070faa803e913475e42415c
    // @lfy def/lexer/main.lfy:lex#lex:lex:1e1b1b5f0a9fc8eaa744027c81e7910bf3a42fd02585b1ac1f2cab88e0567265
    // @lfy def/lexer/main.lfy:lex#lex:lex:7730ac57d8c49b82020fadc004b6e0048cb45fbebe3a3fbb53aec0453e2cca00
    // @lfy def/lexer/main.lfy:lex#lex:lex:450ee6e9ed76b34629d88f741f5af1a8dbe25f00f86e4c44c9a34e9172e11dab
    // @lfy def/lexer/main.lfy:lex#lex:lex:a3338a3ad3bc73ee43f5d57e4293c82d6f06fa50ee65af493ee341b3922f6e02
    // @lfy def/lexer/main.lfy:lex#lex:lex:e4d0300765d272967227c0d6a640e29d92782a59270545ee1a6048ade0f4c27a
    // @lfy def/lexer/main.lfy:lex#lex:lex:d58e9c4c838a03ef2ff7b4edd275e1c6944b8bfe45c0f89a89b3e8edd1bf783a
    // @lfy def/lexer/main.lfy:lex#lex:lex:43ca03f0cc47736f737645e683c517024e2e4cca416afa320961e50d1276d2b9
    // @lfy def/lexer/main.lfy:lex#lex:lex:1210c8d1158cc4f2cfa20539fa63494f3005791c35649c4e78f486e1758e3b95
    // @lfy def/lexer/main.lfy:lex#lex:lex:bde4c4fa8e936832417bc6a32b2130759eb7ba8a1c8308acbf238a35d572fda9
    // @lfy def/lexer/main.lfy:lex#lex:lex:ed837090ef616ebedc9863a32144bcf9ce3894da3a9acbe8d8fa5cd5ef1ee639
    // @lfy def/lexer/main.lfy:lex#lex:lex:65b25f7c1fdc694969bffd2c2147713a85da8510413d1a69640d1a1d0cd9b2c2
    // @lfy def/lexer/main.lfy:lex#lex:lex:551c1cdb7b4634cc42746df5ed1193d5523735ec663934e3d33ad4f201304da3
    // @lfy def/lexer/main.lfy:lex#lex:lex:04a92806c502187b8296ff7f4cc0bdd0365e3829bd34d9949f099d814a51cfc5
    // @lfy def/lexer/main.lfy:lex#lex:lex:03eb8cec25f70932463883b9f55fbc44edf883306b40f4eaac217b4129ee7240
    // @lfy def/lexer/main.lfy:lex#lex:lex:25ace7e0bd83cd11eb06ea47009a60995b8c390057f30f89c9dc379ce7d21917
    // @lfy def/lexer/main.lfy:lex#lex:lex:c0678b85286f2d7b584ce29a054a56c02eebbb8f908b12e050d5123e416ae651
    // @lfy def/lexer/main.lfy:lex#lex:lex:42d54625cc9278f03f63328d78258f9f8bdb093f26b8794517a9b44b738345f3
    // @lfy def/lexer/main.lfy:lex#lex:lex:731f0357d01b27d578fb18bf357317197c99f5ed930933e6a55f72a23c4baecd
    // @lfy def/lexer/main.lfy:lex#lex:lex:17c2d06682531c39cf1868c37ee1957c42c3075164d64d2ce715a08a97ba1ca2
    // @lfy def/lexer/main.lfy:lex#lex:lex:e4423aeba9158cb46659624213ddbeaa9d4287492c5be7f870c5b809769c34cd
    // @lfy def/lexer/main.lfy:lex#lex:lex:27b9bf2f47adc092ad837b3bdd633f855d8dd3a890617fc933226f1c38571c6c
    // @lfy def/lexer/main.lfy:lex#lex:lex:cb7605d81ed4a7b5a7f764a24057cf9f5227a9bc385b6e01f5d14b8813fb94a0
    // @lfy def/lexer/main.lfy:lex#lex:lex:a5d50b1cdd0acae15ebb9a8e708c90b13dbfab9e32ea51a95926873716227f8c
    // @lfy def/lexer/main.lfy:lex#lex:lex:b7b42d92f87ef914c6a1c4d92e8707e621d7139d64d9d5f1a4ee2b9c9d78ce7d
    // @lfy def/lexer/main.lfy:lex#lex:lex:c124301fd090aba3ab974b73c59bcfa13f02315afc44602106ca6d0819a0a93e
    // @lfy def/lexer/main.lfy:lex#lex:lex:2bd7aec7141238cfede2a98ac697875bd44fa6fbb4b133ddcb27f743120b66d5
    // @lfy def/lexer/main.lfy:lex#lex:lex:6f80770c9e2feffa5ad980528f9adf19b6ba8a060f9dde97438a1d755520ad8c
    // @lfy def/lexer/main.lfy:lex#lex:lex:fe79a13d51138ab235084a89567720582082c6fb466d57eee6952bb9c94fd611
    // @lfy def/lexer/main.lfy:lex#lex:lex:a06f3e9532422d47cb4d5469ab77f036d9e33d0f43903e411b1865103d612873
    // @lfy def/lexer/main.lfy:lex#lex:lex:36a661833249fb8ab6e5b09ae6f05c4c7d02cbf30c4c36b47183cfddaf755dbe
    // @lfy def/lexer/main.lfy:lex#lex:lex:b05a4ea4770bc865427b08ff04a127589d83d871e4e1d307d37c5bfc657788f9
    // @lfy def/lexer/main.lfy:lex#lex:lex:3c789b9273943664d1c328ae436d8addf3f5ddfb27aac30adea2932abd1f4377
    // @lfy def/lexer/main.lfy:lex#lex:lex:92481a928fce775f1d27c33450d9ed547b5eb8e91691aeb91f55b030110d3ad9
    // @lfy def/lexer/main.lfy:lex#lex:lex:7eda59f1fa5db032a1da1ae28be4a7a9e1843f84baf3d11c29dba29e6fdebb83
    // @lfy def/lexer/main.lfy:lex#lex:lex:122dd18fec067ee0890ddd8c57e920f3c1b9a6c10f7b65c2463020286aa083b7
    #[test]
    fn every_punctuation_becomes_a_token_of_the_characters_it_matched() {
        let punctuation = family(|terminal| matches!(terminal, Entity::Punctuation(_)));
        assert_eq!(punctuation.len(), 55);
        for terminal in punctuation {
            becomes_a_token(terminal);
        }
    }

    // @lfy def/lexer/main.lfy:lex#lex:lex:39dc045c6e0a685be0179a7d8246c8cecd74cfae9ec9cc7f2668e61f288d7ed4
    // @lfy def/lexer/main.lfy:lex#lex:lex:0e2aca77a4ce3f7ed089abfe7bc847300a5cb9dbbe6f8ed654e679b0c16e676c
    // @lfy def/lexer/main.lfy:lex#lex:lex:ab9c5e43a010d1f3353720f3aceac7780f3a386fb75b0b662019154933a310f4
    // @lfy def/lexer/main.lfy:lex#lex:lex:326ea7d6257b7b6633b5ebc59801b212e41c70fd5b6ad2abff4f9578ed7c7594
    // @lfy def/lexer/main.lfy:lex#lex:lex:6a1b9b1f09088f5975e02e1faf73a5f0e604c1accf80d2efb01411ad9e861882
    // @lfy def/lexer/main.lfy:lex#lex:lex:53d911dd8d9fd26397292d947a5e1bbb45c5cb2f831d810a432f5640b1ade239
    // @lfy def/lexer/main.lfy:lex#lex:lex:db6a98b7010d2d3d93d343543f427d350d245b3afe3269307ae4088f407ccc52
    // @lfy def/lexer/main.lfy:lex#lex:lex:9c251e8c1509fff9f02787fcf13fb21f9fdab7fff7b746bfd718bd486fdae03b
    // @lfy def/lexer/main.lfy:lex#lex:lex:c702d0087d8fba020502259b459aedbf33414807e2d9e7fa9c43324717f3a9a4
    // @lfy def/lexer/main.lfy:lex#lex:lex:8ad502d85fc005795d33e841785b27ed70bd5f91df77c947a7d3290ddf3d567b
    #[test]
    fn every_comment_terminal_becomes_a_token_of_the_characters_it_matched() {
        let comments = family(|terminal| matches!(terminal, Entity::Comment(_)));
        assert_eq!(comments.len(), 10);
        for terminal in comments {
            becomes_a_token(terminal);
        }
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
