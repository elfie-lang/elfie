//! Compiled from `def/lexer/main.lfy`.
//!
//! [`lex_string`] turns a source string into a `Vec<Token>`. Lexing is driven by a
//! [`LexerModeStack`] whose modes decide which token rules are candidates at each
//! position; among candidates the longest match wins (maximal munch).

use std::borrow::Cow;
use std::sync::Arc;

pub mod data;
pub mod tokens;

pub use data::{LexError, LexerModeStack, Mode, ModeEntry, Token, TokenKind};

use tokens::comment::{self, Comment};
use tokens::identifier;
use tokens::keyword::Keyword;
use tokens::literal::{self, Literal, TemplateBlock};
use tokens::operator::Operator;
use tokens::separator::Separator;
use tokens::traits::{self, FollowRule, ModeBehavior};
use tokens::whitespace;

/// The file name used when none is given.
// @lfy def/lexer/main.lfy:17
pub const ANONYMOUS_FILE: &str = "anonymous";

/// Turn a given string into a set of lexigraphical tokens.
///
/// `file` is recorded on every token; it defaults to `"anonymous"`. Errors are returned
/// for characters that match no rule and for input that ends while a mode is still open.
// @lfy def/lexer/main.lfy:11
pub fn lex_string(str: &str, file: Option<&str>) -> Result<Vec<Token>, LexError> {
    let file: Arc<str> = Arc::from(file.unwrap_or(ANONYMOUS_FILE)); // @lfy def/lexer/main.lfy:16
    let mut lexer = Lexer {
        input: str,
        offset: 0,
        line: 1,     // @lfy def/lexer/main.lfy:19
        position: 0, // @lfy def/lexer/main.lfy:20
        file,
        stack: LexerModeStack::new(), // @lfy def/lexer/main.lfy:22
        tokens: Vec::new(),
        body: None,
    };
    lexer.run()?;
    Ok(lexer.tokens)
}

impl TokenKind {
    /// The mode traits attached to this kind of token.
    pub fn mode_behaviors(self) -> &'static [ModeBehavior] {
        match self {
            TokenKind::Literal(literal) => literal.mode_behaviors(),
            TokenKind::TemplateBlock(block) => block.mode_behaviors(),
            TokenKind::Comment(comment) => comment.mode_behaviors(),
            TokenKind::Separator(separator) => separator.mode_behaviors(),
            _ => &[],
        }
    }

    /// The `cantBeFollowedBy` rules attached to this kind of token.
    pub fn cant_be_followed_by(self) -> &'static [FollowRule] {
        match self {
            TokenKind::Comment(comment) => comment.cant_be_followed_by(),
            TokenKind::Operator(operator) => operator.cant_be_followed_by(),
            _ => &[],
        }
    }
}

/// A possible match at the current position.
enum Candidate {
    /// A token of `kind` spanning `len` bytes; `value` overrides the raw text as value.
    Token {
        kind: TokenKind,
        len: usize,
        value: Option<&'static str>,
    },
    /// An escape sequence spanning `len` bytes, replaced by `replacement` in the body.
    Sequence {
        len: usize,
        replacement: Cow<'static, str>,
    },
}

impl Candidate {
    fn token(kind: TokenKind, len: usize) -> Self {
        Candidate::Token {
            kind,
            len,
            value: None,
        }
    }

    fn len(&self) -> usize {
        match self {
            Candidate::Token { len, .. } | Candidate::Sequence { len, .. } => *len,
        }
    }
}

/// Plain-text characters accumulating into a text / comment body / documentation body token.
struct Body {
    kind: TokenKind,
    raw: String,
    value: String,
    line: usize,
    position: usize,
}

struct Lexer<'a> {
    input: &'a str,
    offset: usize,
    line: usize,
    position: usize,
    file: Arc<str>,
    stack: LexerModeStack,
    tokens: Vec<Token>,
    body: Option<Body>,
}

impl Lexer<'_> {
    /// Processes the input start to finish.
    // @lfy def/lexer/main.lfy:14
    fn run(&mut self) -> Result<(), LexError> {
        while self.offset < self.input.len() {
            let rest = &self.input[self.offset..];
            if whitespace::starts_with_end_of_line(rest) {
                // @lfy def/lexer/tokens/traits.lfy:32
                traits::on_end_of_line(&mut self.stack)?;
            }
            match self.candidate(rest) {
                Some(Candidate::Token { kind, len, value }) => self.emit(kind, len, value)?,
                Some(Candidate::Sequence { len, replacement }) => {
                    // @lfy def/lexer/main.lfy:48
                    self.append_body(TokenKind::Text, len, &replacement);
                }
                None => {
                    let c = rest.chars().next().expect("offset is inside the input");
                    let kind =
                        if literal::in_template(&self.stack) || literal::in_string(&self.stack) {
                            TokenKind::Text // @lfy def/lexer/main.lfy:51
                        } else if comment::in_comment(&self.stack) {
                            TokenKind::CommentBody // @lfy def/lexer/main.lfy:69
                        } else if comment::in_documentation(&self.stack) {
                            TokenKind::DocumentationBody // @lfy def/lexer/main.lfy:70
                        } else {
                            // @lfy def/lexer/main.lfy:101
                            return Err(LexError::UnexpectedToken {
                                file: Arc::clone(&self.file),
                                line: self.line,
                                position: self.position,
                                found: c,
                            });
                        };
                    let mut buffer = [0u8; 4];
                    self.append_body(kind, c.len_utf8(), c.encode_utf8(&mut buffer));
                }
            }
        }
        // @lfy def/lexer/tokens/traits.lfy:32
        traits::on_end_of_line(&mut self.stack)?;
        self.flush_body();
        if !self.stack.is_empty() {
            // @lfy def/lexer/main.lfy:23
            return Err(LexError::UnexpectedEndOfFile {
                file: Arc::clone(&self.file),
                line: self.line,
                position: self.position,
                open: self.stack.modes(),
            });
        }
        Ok(())
    }

    /// Collects every rule that matches at `rest` under the current mode and returns the
    /// longest one.
    // @lfy def/lexer/main.lfy:24
    fn candidate(&self, rest: &str) -> Option<Candidate> {
        let stack = &self.stack;
        let in_string = literal::in_string(stack);
        let in_template = literal::in_template(stack);
        let in_comment = comment::in_comment(stack);
        let in_documentation = comment::in_documentation(stack);

        let mut best: Option<Candidate> = None;
        let mut consider = |candidate: Candidate| {
            if best
                .as_ref()
                .is_none_or(|current| candidate.len() > current.len())
            {
                best = Some(candidate);
            }
        };

        // Literals
        // @lfy def/lexer/main.lfy:30
        if !in_comment && !in_documentation {
            for (boundary, len) in Literal::matches_at(rest) {
                let kind = TokenKind::Literal(boundary);
                // @lfy def/lexer/main.lfy:52
                let started_this_mode = stack.top().is_some_and(|entry| entry.creator == kind);
                if (in_template || in_string) && !started_this_mode {
                    continue; // plain text, not a token for parsing
                }
                consider(Candidate::token(kind, len));
            }
        }

        // Template blocks
        for (block, len) in TemplateBlock::matches_at(rest) {
            let allowed = match block {
                TemplateBlock::ExecutionOpen => in_template, // @lfy def/lexer/main.lfy:34
                TemplateBlock::ExecutionClose => literal::in_template_execution(stack), // @lfy def/lexer/main.lfy:35
                TemplateBlock::ReferenceOpen => in_template, // @lfy def/lexer/main.lfy:36
                TemplateBlock::ReferenceClose => literal::in_template_reference(stack), // @lfy def/lexer/main.lfy:37
            };
            if allowed {
                consider(Candidate::token(TokenKind::TemplateBlock(block), len));
            }
        }

        // Numbers
        // @lfy def/lexer/main.lfy:39
        if !in_comment
            && !in_documentation
            && !in_string
            && !in_template
            && let Some(len) = match_number(rest)
        {
            consider(Candidate::token(TokenKind::Number, len));
        }

        // Sequences
        // @lfy def/lexer/main.lfy:47
        if (in_string || in_template)
            && let Some((_, len, replacement)) = literal::match_sequence(rest)
        {
            consider(Candidate::Sequence { len, replacement });
        }

        // Comments
        // @lfy def/lexer/main.lfy:58
        if !in_string
            && !in_template
            && !comment::in_inline_comment(stack)
            && !comment::in_inline_documentation(stack)
        {
            for (delimiter, len) in Comment::matches_at(rest) {
                // @lfy def/lexer/main.lfy:64
                if delimiter.is_inline()
                    && (comment::in_block_comment(stack) || comment::in_block_documentation(stack))
                {
                    continue;
                }
                let kind = TokenKind::Comment(delimiter);
                if traits::is_followed_by_forbidden(kind, &rest[len..]) {
                    continue;
                }
                consider(Candidate::token(kind, len));
            }
        }

        if !in_string && !in_template && !in_comment && !in_documentation {
            // Keywords
            // @lfy def/lexer/main.lfy:73
            for (keyword, len) in Keyword::matches_at(rest) {
                consider(Candidate::token(TokenKind::Keyword(keyword), len));
            }

            // Operators
            // @lfy def/lexer/main.lfy:78
            for (operator, len) in Operator::matches_at(rest) {
                let kind = TokenKind::Operator(operator);
                if traits::is_followed_by_forbidden(kind, &rest[len..]) {
                    continue;
                }
                consider(Candidate::token(kind, len));
            }

            // Separators
            // @lfy def/lexer/main.lfy:83
            for (separator, len) in Separator::matches_at(rest) {
                consider(Candidate::token(TokenKind::Separator(separator), len));
            }

            // Whitespace
            // @lfy def/lexer/main.lfy:88
            if let Some((space, len, value)) = whitespace::match_space(rest) {
                consider(Candidate::Token {
                    kind: TokenKind::Space(space),
                    len,
                    value: Some(value),
                });
            }

            // Identifiers
            // @lfy def/lexer/main.lfy:93
            if let Some(len) = identifier::match_identifier(rest) {
                consider(Candidate::token(TokenKind::Identifier, len));
            }
        }

        best
    }

    /// Pushes a token spanning the next `len` bytes and lets the mode stack respond to it.
    fn emit(
        &mut self,
        kind: TokenKind,
        len: usize,
        value: Option<&'static str>,
    ) -> Result<(), LexError> {
        self.flush_body();
        let raw = &self.input[self.offset..self.offset + len]; // @lfy def/lexer/main.lfy:21
        self.tokens.push(Token {
            kind,
            value: value.unwrap_or(raw).to_owned(),
            raw: raw.to_owned(),
            file: Arc::clone(&self.file),
            line: self.line,
            position: self.position,
        });
        self.advance(len);
        // @lfy def/lexer/main.lfy:22
        traits::on_token_created(kind, &mut self.stack)
    }

    /// Appends the next `len` bytes of input to the pending body token, contributing
    /// `value` to its processed value.
    fn append_body(&mut self, kind: TokenKind, len: usize, value: &str) {
        let raw = &self.input[self.offset..self.offset + len];
        let body = self.body.get_or_insert_with(|| Body {
            kind,
            raw: String::new(),
            value: String::new(),
            line: self.line,
            position: self.position,
        });
        debug_assert_eq!(body.kind, kind, "a body never spans two modes");
        body.raw.push_str(raw);
        body.value.push_str(value);
        self.advance(len);
    }

    /// Turns the pending body, if any, into a token.
    fn flush_body(&mut self) {
        if let Some(body) = self.body.take() {
            self.tokens.push(Token {
                kind: body.kind,
                value: body.value,
                raw: body.raw,
                file: Arc::clone(&self.file),
                line: body.line,
                position: body.position,
            });
        }
    }

    /// Consumes `len` bytes, tracking the 1-indexed line and 0-indexed character position.
    // @lfy def/lexer/main.lfy:18
    fn advance(&mut self, len: usize) {
        for c in self.input[self.offset..self.offset + len].chars() {
            if c == '\n' {
                self.line += 1;
                self.position = 0;
            } else {
                self.position += 1;
            }
        }
        self.offset += len;
    }
}

/// A digit (0-9) followed by zero or more digits, `.` or `_` characters.
// @lfy def/lexer/main.lfy:40
fn match_number(rest: &str) -> Option<usize> {
    if !rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(
        rest.bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'.' || *b == b'_')
            .count(),
    )
}

#[cfg(test)]
mod tests {
    use super::tokens::whitespace::Space;
    use super::*;

    fn lex(source: &str) -> Vec<Token> {
        lex_string(source, None).unwrap_or_else(|error| panic!("{source:?}: {error}"))
    }

    fn kinds(source: &str) -> Vec<TokenKind> {
        lex(source).into_iter().map(|token| token.kind).collect()
    }

    fn raws(source: &str) -> Vec<String> {
        lex(source).into_iter().map(|token| token.raw).collect()
    }

    fn values(source: &str) -> Vec<String> {
        lex(source).into_iter().map(|token| token.value).collect()
    }

    fn error(source: &str) -> LexError {
        lex_string(source, None).expect_err("expected a lexing error")
    }

    const SP: TokenKind = TokenKind::Space(Space::Space);
    const NL: TokenKind = TokenKind::Space(Space::Newline);
    const SINGLE: TokenKind = TokenKind::Literal(Literal::StringSingleBoundary);
    const DOUBLE: TokenKind = TokenKind::Literal(Literal::StringDoubleBoundary);
    const TEMPLATE: TokenKind = TokenKind::Literal(Literal::StringTemplateBoundary);

    fn op(operator: Operator) -> TokenKind {
        TokenKind::Operator(operator)
    }
    fn sep(separator: Separator) -> TokenKind {
        TokenKind::Separator(separator)
    }
    fn cm(comment: Comment) -> TokenKind {
        TokenKind::Comment(comment)
    }
    fn tb(block: TemplateBlock) -> TokenKind {
        TokenKind::TemplateBlock(block)
    }

    // @lfy def/lexer/main.lfy:13
    #[test]
    fn input_is_utf8_and_positions_count_characters() {
        let tokens = lex("é ← 日本");
        assert_eq!(
            tokens.iter().map(|t| t.kind).collect::<Vec<_>>(),
            vec![
                TokenKind::Identifier,
                SP,
                op(Operator::ArrowSingleLeftSingleChar),
                SP,
                TokenKind::Identifier
            ]
        );
        assert_eq!(
            tokens.iter().map(|t| t.position).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
    }

    // @lfy def/lexer/main.lfy:14
    #[test]
    fn tokens_come_out_in_source_order() {
        assert_eq!(
            raws("let x = 1;"),
            vec!["let", " ", "x", " ", "=", " ", "1", ";"]
        );
    }

    // @lfy def/lexer/main.lfy:15
    #[test]
    fn empty_input_yields_no_tokens() {
        assert_eq!(lex(""), Vec::<Token>::new());
    }

    // @lfy def/lexer/main.lfy:16
    #[test]
    fn file_is_recorded_on_every_token_when_given() {
        let tokens = lex_string("a b", Some("lib.lfy")).unwrap();
        assert!(tokens.iter().all(|token| &*token.file == "lib.lfy"));
    }

    // @lfy def/lexer/main.lfy:17
    #[test]
    fn file_defaults_to_anonymous() {
        assert!(lex("a b").iter().all(|token| &*token.file == "anonymous"));
    }

    // @lfy def/lexer/main.lfy:18
    #[test]
    fn lf_and_crlf_are_interchangeable_end_of_line_tokens() {
        let lf = lex("a\nb");
        let crlf = lex("a\r\nb");
        assert_eq!(
            lf.iter().map(|t| t.kind).collect::<Vec<_>>(),
            crlf.iter().map(|t| t.kind).collect::<Vec<_>>()
        );
        assert_eq!(
            lf.iter().map(|t| &t.value).collect::<Vec<_>>(),
            crlf.iter().map(|t| &t.value).collect::<Vec<_>>()
        );
        assert_eq!(crlf[1].raw, "\r\n");
        assert_eq!(crlf[1].value, "\n");
        assert_eq!((crlf[2].line, crlf[2].position), (2, 0));
        assert_eq!((lf[2].line, lf[2].position), (2, 0));
        // A lone carriage return is a space token but not an end of line.
        let cr = lex("a\rb");
        assert_eq!(cr[1].kind, TokenKind::Space(Space::CarriageReturn));
        assert_eq!((cr[2].line, cr[2].position), (1, 2));
    }

    // @lfy def/lexer/main.lfy:19
    #[test]
    fn line_is_one_indexed_and_counts_end_of_lines_inside_bodies() {
        let tokens = lex("a\n\nb `x\ny` c");
        let find = |raw: &str| tokens.iter().find(|t| t.raw == raw).unwrap();
        assert_eq!(find("a").line, 1);
        assert_eq!(find("b").line, 3);
        assert_eq!(find("x\ny").line, 3);
        assert_eq!((find("c").line, find("c").position), (4, 3));
    }

    // @lfy def/lexer/main.lfy:20
    #[test]
    fn position_is_zero_indexed_within_the_line() {
        let tokens = lex("ab cd\n  ef");
        let positions: Vec<(usize, usize)> = tokens.iter().map(|t| (t.line, t.position)).collect();
        assert_eq!(
            positions,
            vec![(1, 0), (1, 2), (1, 3), (1, 5), (2, 0), (2, 1), (2, 2)]
        );
    }

    // @lfy def/lexer/main.lfy:21
    #[test]
    fn raw_is_the_original_source_text() {
        let tokens = lex("'a\\nb'");
        assert_eq!(tokens[1].raw, "a\\nb");
        assert_eq!(tokens[1].value, "a\nb");
        assert_eq!(
            tokens.iter().map(|t| t.raw.as_str()).collect::<String>(),
            "'a\\nb'"
        );
    }

    // @lfy def/lexer/main.lfy:22
    #[test]
    fn a_fresh_mode_stack_is_used_for_every_run() {
        assert!(matches!(error("("), LexError::UnexpectedEndOfFile { .. }));
        assert_eq!(kinds(")"), vec![sep(Separator::GroupClose)]);
        assert_eq!(kinds("x"), vec![TokenKind::Identifier]);
    }

    // @lfy def/lexer/main.lfy:23
    #[test]
    fn end_of_input_with_open_modes_is_an_unexpected_end_of_file() {
        for source in [
            "(", "{", "[", "'abc", "\"abc", "`abc", "`{{", "`[[", "/* x", "/** x", "`{{ (`",
        ] {
            assert!(
                matches!(error(source), LexError::UnexpectedEndOfFile { .. }),
                "{source:?}"
            );
        }
        let LexError::UnexpectedEndOfFile {
            line,
            position,
            open,
            ..
        } = error("(\n[")
        else {
            panic!()
        };
        assert_eq!((line, position), (2, 1));
        assert_eq!(open, vec![Mode::Group, Mode::List]);
    }

    // @lfy def/lexer/main.lfy:24
    #[test]
    fn maximal_munch_picks_the_longest_candidate() {
        assert_eq!(kinds("=>"), vec![op(Operator::ArrowDoubleRight)]);
        assert_eq!(kinds("=++"), vec![op(Operator::SetterIncrement)]);
        assert_eq!(kinds("=**"), vec![op(Operator::SetterPower)]);
        assert_eq!(kinds("**"), vec![op(Operator::Power)]);
        assert_eq!(kinds("..."), vec![op(Operator::Spread)]);
        assert_eq!(kinds("^^:"), vec![op(Operator::RefDefineLast)]);
        assert_eq!(kinds("<-"), vec![op(Operator::ArrowSingleLeft)]);
        assert_eq!(kinds("<="), vec![op(Operator::EqualityLessThanOrEqual)]);
        assert_eq!(kinds("=??"), vec![op(Operator::SetterCoalescenceNull)]);
        assert_eq!(
            kinds("a=-1"),
            vec![
                TokenKind::Identifier,
                op(Operator::SetterSubtractive),
                TokenKind::Number
            ]
        );
        assert_eq!(kinds("//"), vec![cm(Comment::InlineStart)]);
        assert_eq!(kinds("///"), vec![cm(Comment::DocInlineStart)]);
    }

    // @lfy def/lexer/main.lfy:26
    #[test]
    fn context_decides_which_candidates_are_allowed_before_munching() {
        // `{{` outside a template is two block opens.
        assert_eq!(
            kinds("{{}}"),
            vec![
                sep(Separator::BlockOpen),
                sep(Separator::BlockOpen),
                sep(Separator::BlockClose),
                sep(Separator::BlockClose)
            ]
        );
        // `}}` closes the execution block only when it is on top; a block inside it wins first.
        assert_eq!(
            kinds("`{{{}}}`"),
            vec![
                TEMPLATE,
                tb(TemplateBlock::ExecutionOpen),
                sep(Separator::BlockOpen),
                sep(Separator::BlockClose),
                tb(TemplateBlock::ExecutionClose),
                TEMPLATE
            ]
        );
        // `]]` closes the reference block only when it is on top.
        assert_eq!(
            kinds("`[[a[0]]]`"),
            vec![
                TEMPLATE,
                tb(TemplateBlock::ReferenceOpen),
                TokenKind::Identifier,
                sep(Separator::ListOpen),
                TokenKind::Number,
                sep(Separator::ListClose),
                tb(TemplateBlock::ReferenceClose),
                TEMPLATE
            ]
        );
    }

    // @lfy def/lexer/main.lfy:30
    #[test]
    fn literal_boundaries_open_and_close_strings() {
        assert_eq!(kinds("'a'"), vec![SINGLE, TokenKind::Text, SINGLE]);
        assert_eq!(kinds("\"a\""), vec![DOUBLE, TokenKind::Text, DOUBLE]);
        assert_eq!(kinds("`a`"), vec![TEMPLATE, TokenKind::Text, TEMPLATE]);
        assert_eq!(kinds("''"), vec![SINGLE, SINGLE]);
        // Not literals inside comments or documentation.
        assert_eq!(
            kinds("/* ' */"),
            vec![
                cm(Comment::BlockOpen),
                TokenKind::CommentBody,
                cm(Comment::BlockClose)
            ]
        );
        assert_eq!(
            kinds("/** ` **/"),
            vec![
                cm(Comment::DocBlockOpen),
                TokenKind::DocumentationBody,
                cm(Comment::DocBlockClose)
            ]
        );
        // Strings nest inside template execution blocks, templates nest inside strings' execution blocks.
        assert_eq!(
            kinds("`{{'a'}}`"),
            vec![
                TEMPLATE,
                tb(TemplateBlock::ExecutionOpen),
                SINGLE,
                TokenKind::Text,
                SINGLE,
                tb(TemplateBlock::ExecutionClose),
                TEMPLATE
            ]
        );
        assert_eq!(
            kinds("`{{`b`}}`"),
            vec![
                TEMPLATE,
                tb(TemplateBlock::ExecutionOpen),
                TEMPLATE,
                TokenKind::Text,
                TEMPLATE,
                tb(TemplateBlock::ExecutionClose),
                TEMPLATE
            ]
        );
    }

    // @lfy def/lexer/main.lfy:34
    #[test]
    fn template_blocks_are_only_recognised_in_their_modes() {
        assert_eq!(
            kinds("`a{{b}}c[[e]]f`"),
            vec![
                TEMPLATE,
                TokenKind::Text,
                tb(TemplateBlock::ExecutionOpen),
                TokenKind::Identifier,
                tb(TemplateBlock::ExecutionClose),
                TokenKind::Text,
                tb(TemplateBlock::ReferenceOpen),
                TokenKind::Identifier,
                tb(TemplateBlock::ReferenceClose),
                TokenKind::Text,
                TEMPLATE
            ]
        );
        // `}}` and `]]` in plain template text are text; `{{` inside a string is text.
        assert_eq!(values("`a}}b]]c`"), vec!["`", "a}}b]]c", "`"]);
        assert_eq!(values("'{{x}}'"), vec!["'", "{{x}}", "'"]);
        // `[[` inside an execution block is two list opens, not a reference.
        assert_eq!(
            kinds("`{{[[]]}}`"),
            vec![
                TEMPLATE,
                tb(TemplateBlock::ExecutionOpen),
                sep(Separator::ListOpen),
                sep(Separator::ListOpen),
                sep(Separator::ListClose),
                sep(Separator::ListClose),
                tb(TemplateBlock::ExecutionClose),
                TEMPLATE
            ]
        );
    }

    // @lfy def/lexer/main.lfy:39
    #[test]
    fn numbers_are_digits_followed_by_digits_dots_or_underscores() {
        assert_eq!(raws("1_000.5.3"), vec!["1_000.5.3"]);
        assert_eq!(values("1_000"), vec!["1_000"]);
        assert_eq!(
            kinds("1abc"),
            vec![TokenKind::Number, TokenKind::Identifier]
        );
        assert_eq!(
            kinds(".5"),
            vec![op(Operator::AccessorValue), TokenKind::Number]
        );
        assert_eq!(kinds("'1'"), vec![SINGLE, TokenKind::Text, SINGLE]);
        assert_eq!(kinds("`1`"), vec![TEMPLATE, TokenKind::Text, TEMPLATE]);
        assert_eq!(
            kinds("/*1*/"),
            vec![
                cm(Comment::BlockOpen),
                TokenKind::CommentBody,
                cm(Comment::BlockClose)
            ]
        );
    }

    // @lfy def/lexer/main.lfy:47
    #[test]
    fn sequences_are_replaced_in_the_body_value_but_kept_in_raw() {
        let cases = [
            ("'\\\\'", "\\"),
            ("'\\r'", "\r"),
            ("'a\\\nb'", "ab"),
            ("'a\\\r\nb'", "ab"),
            ("'\\x41'", "A"),
            ("'\\u0041'", "A"),
            ("'\\u01F600'", "😀"),
            ("'\\{{'", "{{"),
            ("'\\}}'", "}}"),
            ("'\\[['", "[["),
            ("'\\]]'", "]]"),
            ("'\\n'", "\n"),
            ("'\\0'", "\0"),
            ("'\\t'", "\t"),
            ("'\\''", "'"),
            ("'\\\"'", "\""),
            ("'\\`'", "`"),
            ("`\\`\\{{\\n`", "`{{\n"),
        ];
        for (source, expected) in cases {
            let tokens = lex(source);
            assert_eq!(tokens.len(), 3, "{source:?}");
            assert_eq!(tokens[1].value, expected, "{source:?}");
            assert_eq!(tokens[1].raw, &source[1..source.len() - 1], "{source:?}");
        }
        // Escaped template blocks stay text inside a template.
        assert_eq!(
            kinds("`\\{{x\\}}`"),
            vec![TEMPLATE, TokenKind::Text, TEMPLATE]
        );
        // Unknown or incomplete sequences and non-scalar code points stay literal text.
        assert_eq!(
            values("'\\q \\x4 \\uD800 \\uFFFFFF'"),
            vec!["'", "\\q \\x4 \\uD800 \\uFFFFFF", "'"]
        );
        // Sequences are not recognised outside strings and templates.
        assert!(matches!(
            error("\\n"),
            LexError::UnexpectedToken { found: '\\', .. }
        ));
    }

    // @lfy def/lexer/main.lfy:51
    #[test]
    fn plain_text_is_grouped_into_one_text_token_until_a_boundary_or_block() {
        let tokens = lex("`ab\ncd {{x}} ef`");
        assert_eq!(tokens[1].kind, TokenKind::Text);
        assert_eq!(tokens[1].value, "ab\ncd ");
        assert_eq!(tokens[3].kind, TokenKind::Identifier);
        assert_eq!(tokens[5].value, " ef");
        assert_eq!(tokens.len(), 7);
    }

    // @lfy def/lexer/main.lfy:52
    #[test]
    fn other_boundary_characters_inside_a_literal_are_plain_text() {
        assert_eq!(values("'a\"b`c'"), vec!["'", "a\"b`c", "'"]);
        assert_eq!(values("\"a'b`c\""), vec!["\"", "a'b`c", "\""]);
        assert_eq!(values("`a'b\"c`"), vec!["`", "a'b\"c", "`"]);
    }

    // @lfy def/lexer/main.lfy:58
    #[test]
    fn comment_delimiters_depend_on_the_comment_mode() {
        // Block comments nest.
        assert_eq!(
            kinds("/* a /* b */ c */"),
            vec![
                cm(Comment::BlockOpen),
                TokenKind::CommentBody,
                cm(Comment::BlockOpen),
                TokenKind::CommentBody,
                cm(Comment::BlockClose),
                TokenKind::CommentBody,
                cm(Comment::BlockClose)
            ]
        );
        // Inline delimiters are not tokens inside block comments or documentation.
        assert_eq!(values("/* // /// */"), vec!["/*", " // /// ", "*/"]);
        assert_eq!(values("/** // /// **/"), vec!["/**", " // /// ", "**/"]);
        // Nothing is a delimiter inside inline comments or documentation.
        assert_eq!(values("// /* */ /** **/"), vec!["//", " /* */ /** **/"]);
        assert_eq!(values("/// /* */ // x"), vec!["///", " /* */ // x"]);
        // `/**` cannot be followed by `/`, so `/**/` is an empty block comment.
        assert_eq!(
            kinds("/**/"),
            vec![cm(Comment::BlockOpen), cm(Comment::BlockClose)]
        );
        // A close delimiter whose mode is not on top is emitted without changing the stack,
        // so `/***/`, `/* x **/` and `/** x */` all reach the end of input with a mode open.
        assert!(matches!(
            error("/***/"),
            LexError::UnexpectedEndOfFile { .. }
        ));
        assert!(matches!(
            error("/* x **/"),
            LexError::UnexpectedEndOfFile { .. }
        ));
        assert!(matches!(
            error("/** x */"),
            LexError::UnexpectedEndOfFile { .. }
        ));
        // Not delimiters inside strings or templates.
        assert_eq!(values("'/* // */'"), vec!["'", "/* // */", "'"]);
        assert_eq!(values("`/* // */`"), vec!["`", "/* // */", "`"]);
        // Inline comments end at `\n`, `\r\n` and EOF; the end of line is lexed as a space token.
        assert_eq!(
            kinds("// x\ny"),
            vec![
                cm(Comment::InlineStart),
                TokenKind::CommentBody,
                NL,
                TokenKind::Identifier
            ]
        );
        assert_eq!(
            kinds("// x\r\ny"),
            vec![
                cm(Comment::InlineStart),
                TokenKind::CommentBody,
                NL,
                TokenKind::Identifier
            ]
        );
        assert_eq!(values("// x\ry"), vec!["//", " x\ry"]);
        assert_eq!(
            kinds("/// x"),
            vec![cm(Comment::DocInlineStart), TokenKind::DocumentationBody]
        );
        assert_eq!(kinds("//"), vec![cm(Comment::InlineStart)]);
    }

    // @lfy def/lexer/main.lfy:69
    #[test]
    fn comment_and_documentation_bodies_are_single_shared_tokens() {
        let tokens = lex("/* a\nb */");
        assert_eq!(tokens[1].kind, TokenKind::CommentBody);
        assert_eq!(tokens[1].value, " a\nb ");
        assert_eq!(tokens[1].raw, " a\nb ");
        assert_eq!((tokens[1].line, tokens[1].position), (1, 2));
        assert_eq!((tokens[2].line, tokens[2].position), (2, 2));

        let tokens = lex("/** d **/");
        assert_eq!(tokens[1].kind, TokenKind::DocumentationBody);
        assert_eq!(tokens[1].value, " d ");
    }

    // @lfy def/lexer/main.lfy:73
    #[test]
    fn every_keyword_lexes_to_its_member_outside_literals_and_comments() {
        for &keyword in Keyword::ALL {
            assert_eq!(
                kinds(keyword.value()),
                vec![TokenKind::Keyword(keyword)],
                "{}",
                keyword.key()
            );
            let quoted = format!("'{}'", keyword.value());
            assert_eq!(
                kinds(&quoted),
                vec![SINGLE, TokenKind::Text, SINGLE],
                "{}",
                keyword.key()
            );
            let commented = format!("/*{}*/", keyword.value());
            assert_eq!(
                kinds(&commented)[1],
                TokenKind::CommentBody,
                "{}",
                keyword.key()
            );
        }
    }

    // @lfy def/lexer/main.lfy:78
    #[test]
    fn every_operator_lexes_to_its_member_outside_literals_and_comments() {
        for &operator in Operator::ALL {
            // `^` may only be followed by a number.
            let source = if operator == Operator::BitwiseXor {
                "^1".to_string()
            } else {
                operator.value().to_string()
            };
            assert_eq!(
                kinds(&source)[0],
                TokenKind::Operator(operator),
                "{}",
                operator.key()
            );
            let templated = format!("`{}`", operator.value());
            assert_eq!(
                kinds(&templated),
                vec![TEMPLATE, TokenKind::Text, TEMPLATE],
                "{}",
                operator.key()
            );
        }
        assert!(matches!(
            error("^ x"),
            LexError::UnexpectedToken { found: '^', .. }
        ));
        assert_eq!(kinds("^^"), vec![op(Operator::RefLast)]);
    }

    // @lfy def/lexer/main.lfy:83
    #[test]
    fn every_separator_lexes_to_its_member_outside_literals_and_comments() {
        assert_eq!(
            kinds("{}()[],;"),
            vec![
                sep(Separator::BlockOpen),
                sep(Separator::BlockClose),
                sep(Separator::GroupOpen),
                sep(Separator::GroupClose),
                sep(Separator::ListOpen),
                sep(Separator::ListClose),
                sep(Separator::ListContinue),
                sep(Separator::StatementEnd)
            ]
        );
        assert_eq!(kinds("'{}()[],;'"), vec![SINGLE, TokenKind::Text, SINGLE]);
        assert_eq!(kinds("/*{}()[],;*/")[1], TokenKind::CommentBody);
        // A closing separator with no matching open mode is still a token.
        assert_eq!(kinds("}"), vec![sep(Separator::BlockClose)]);
    }

    // @lfy def/lexer/main.lfy:88
    #[test]
    fn every_whitespace_character_is_its_own_space_token() {
        for &space in Space::ALL {
            let source = format!("a{}{}b", space.value(), space.value());
            let tokens = lex(&source);
            assert_eq!(tokens.len(), 4, "{}", space.key());
            assert_eq!(tokens[1].kind, TokenKind::Space(space), "{}", space.key());
            assert_eq!(tokens[2].kind, TokenKind::Space(space), "{}", space.key());
            assert!(tokens[1].is_space());
        }
        assert_eq!(kinds("' '"), vec![SINGLE, TokenKind::Text, SINGLE]);
        assert_eq!(kinds("/* */")[1], TokenKind::CommentBody);
    }

    // @lfy def/lexer/main.lfy:93
    #[test]
    fn identifiers_are_xid_runs_that_are_not_keywords() {
        assert_eq!(kinds("héllo _x1 __ constant d1"), {
            let id = TokenKind::Identifier;
            vec![id, SP, id, SP, id, SP, id, SP, id]
        });
        assert_eq!(kinds("const"), vec![TokenKind::Keyword(Keyword::Const)]);
        assert_eq!(
            kinds("if()"),
            vec![
                TokenKind::Keyword(Keyword::If),
                sep(Separator::GroupOpen),
                sep(Separator::GroupClose)
            ]
        );
        assert_eq!(kinds("'x'"), vec![SINGLE, TokenKind::Text, SINGLE]);
        assert_eq!(kinds("`x`"), vec![TEMPLATE, TokenKind::Text, TEMPLATE]);
        assert_eq!(kinds("/*x*/")[1], TokenKind::CommentBody);
        assert_eq!(kinds("/**x**/")[1], TokenKind::DocumentationBody);
    }

    // @lfy def/lexer/main.lfy:101
    #[test]
    fn a_character_matching_no_rule_is_an_unexpected_token() {
        let LexError::UnexpectedToken {
            line,
            position,
            found,
            file,
        } = error("ab\n c#")
        else {
            panic!()
        };
        assert_eq!((line, position, found), (2, 2, '#'));
        assert_eq!(&*file, "anonymous");
        assert!(matches!(
            error("\u{FEFF}x"),
            LexError::UnexpectedToken {
                found: '\u{FEFF}',
                ..
            }
        ));
    }

    #[test]
    fn a_representative_program_lexes_end_to_end() {
        let source = "fn add(a: number, b?: number): `Add {{a}} to [[b]]` => number {\r\n  // sum\n  return a + b;\n}\n";
        let tokens = lex(source);
        assert_eq!(
            tokens.iter().map(|t| t.raw.as_str()).collect::<String>(),
            source
        );
        assert_eq!(tokens[0].kind, TokenKind::Keyword(Keyword::AgentFn));
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == tb(TemplateBlock::ExecutionOpen))
        );
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == tb(TemplateBlock::ReferenceOpen))
        );
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == TokenKind::CommentBody && t.value == " sum")
        );
        let last = tokens.last().unwrap();
        assert_eq!((last.kind, last.line), (NL, 4));
    }
}
