//! Compiled from `def/lexer/main.lfy`.
//!
//! [`lex_string`] turns a source string into a `Vec<Token>`. Every lexing rule matches a
//! grammar rule's EBNF against the remaining input under a condition on the
//! [`LexerModeStack`]; among the rules that match, the longest match wins and ties go to
//! the rule declared first.

use std::sync::Arc;

pub mod data;
pub mod modes;
pub mod traits;

pub use data::{LexError, LexerModeStack, ModeEntry, Token, TokenKind};
pub use modes::Mode;

use crate::grammar::expression::Expression;
use crate::grammar::tokens::comment::Comment;
use crate::grammar::tokens::identifier::Identifier;
use crate::grammar::tokens::keyword::Keyword;
use crate::grammar::tokens::literal::{self, Literal, Replacement};
use crate::grammar::tokens::operator::Operator;
use crate::grammar::tokens::separator::Separator;
use crate::grammar::tokens::space::Space;
use crate::grammar::{EbnfSyntax, RuleInfo, ebnf};
use modes::{
    in_block_comment, in_block_documentation, in_comment, in_content_body_mode, in_documentation,
    in_double_quote_string, in_inline_comment, in_inline_documentation, in_single_quote_string,
    in_string, in_template, in_template_execution, in_template_reference, not_in_content_body_mode,
};

/// The file name used when none is given.
// @lfy def/lexer/main.lfy:31
pub const ANONYMOUS_FILE: &str = "anonymous";

/// `ace function matchingEBNF(ebnf)`: characters match `{{ebnf.syntax}}` from
/// `{{ebnf.identifier}}`. Yields the byte length of the longest such match.
// @lfy def/lexer/main.lfy:13
fn matching_ebnf(rule: RuleInfo, source: &str, offset: usize) -> Option<usize> {
    ebnf::grammar().longest_match_at(rule.identifier(), source, offset)
}

/// Full EBNF syntax document: the rule of every token, in the order the token files are
/// loaded, separated by blank lines. The lexer uses it as its baseline reference.
// @lfy def/lexer/main.lfy:44
pub fn ebnf_document() -> String {
    fn rules<R: EbnfSyntax>(all: &'static [R]) -> impl Iterator<Item = &'static str> {
        all.iter().map(|rule| rule.rule())
    }
    // @lfy def/lexer/main.lfy:45
    let rules: Vec<&'static str> = rules(Comment::ALL)
        .chain(rules(Identifier::ALL))
        .chain(rules(Keyword::ALL))
        .chain(rules(Literal::ALL))
        .chain(rules(Operator::ALL))
        .chain(rules(Separator::ALL))
        .chain(rules(Space::ALL))
        .collect();
    rules.join("\n\n") // @lfy def/lexer/main.lfy:52
}

/// Turn a given string into a set of lexigraphical tokens.
///
/// `file` is recorded on every token; it defaults to `"anonymous"`. The only error is
/// input that ends while a lexing mode is still open.
// @lfy def/lexer/main.lfy:25
pub fn lex_string(str: &str, file: Option<&str>) -> Result<Vec<Token>, LexError> {
    let file: Arc<str> = Arc::from(file.unwrap_or(ANONYMOUS_FILE)); // @lfy def/lexer/main.lfy:30
    let mut lexer = Lexer {
        input: str,
        offset: 0,
        line: 1,     // @lfy def/lexer/main.lfy:33
        position: 0, // @lfy def/lexer/main.lfy:34
        file,
        stack: LexerModeStack::new(), // @lfy def/lexer/main.lfy:36
        tokens: Vec::new(),
    };
    lexer.run()?;
    Ok(lexer.tokens)
}

/// The best match found so far at the current position.
struct Candidate {
    kind: TokenKind,
    len: usize,
}

/// Records a match of `len` bytes for `kind` unless a longer match was already found.
/// Ties keep the earlier rule. (Traits that forbid a match are applied by the matcher.)
// @lfy def/lexer/main.lfy:40
fn consider(best: &mut Option<Candidate>, kind: TokenKind, len: usize) {
    if best.as_ref().is_none_or(|current| len > current.len) {
        *best = Some(Candidate { kind, len });
    }
}

/// [`consider`] for a kind whose single grammar rule decides the match length.
fn consider_rule(best: &mut Option<Candidate>, kind: TokenKind, source: &str, offset: usize) {
    let rule = kind.rule().expect("a kind lexed from one rule");
    if let Some(len) = matching_ebnf(rule, source, offset) {
        consider(best, kind, len);
    }
}

struct Lexer<'a> {
    input: &'a str,
    offset: usize,
    line: usize,
    position: usize,
    file: Arc<str>,
    stack: LexerModeStack,
    tokens: Vec<Token>,
}

impl Lexer<'_> {
    /// Processes the input start to finish.
    // @lfy def/lexer/main.lfy:28
    fn run(&mut self) -> Result<(), LexError> {
        while self.offset < self.input.len() {
            let rest = &self.input[self.offset..];
            if rest.starts_with('\n') || rest.starts_with("\r\n") {
                // @lfy def/lexer/traits.lfy:32
                traits::on_end_of_line(&mut self.stack);
            }
            match self.candidate() {
                Some(Candidate { kind, len }) => self.push_token(kind, len),
                // @lfy def/lexer/main.lfy:116
                None => {
                    let c = rest.chars().next().expect("offset is inside the input");
                    self.push_token(TokenKind::Invalid, c.len_utf8());
                }
            }
        }
        // @lfy def/lexer/traits.lfy:32
        traits::on_end_of_line(&mut self.stack);
        if !self.stack.is_empty() {
            // @lfy def/lexer/main.lfy:37
            return Err(LexError::UnexpectedEndOfFile {
                file: Arc::clone(&self.file),
                line: self.line,
                position: self.position,
                open: self.stack.modes(),
            });
        }
        Ok(())
    }

    /// Evaluates every lexing rule at `rest` under the current mode and returns the best
    /// candidate: the longest match, ties going to the rule declared first.
    // @lfy def/lexer/main.lfy:38
    fn candidate(&self) -> Option<Candidate> {
        let stack = &self.stack;
        let (source, offset) = (self.input, self.offset);
        let mut best = None;
        let mut rule = |kind: TokenKind| consider_rule(&mut best, kind, source, offset);

        // Literals
        // @lfy def/lexer/main.lfy:61
        if !in_content_body_mode(stack) {
            for simple in [
                Literal::NullLiteral,
                Literal::UndefinedLiteral,
                Literal::BooleanLiteral,
                Literal::NumberLiteral,
            ] {
                rule(TokenKind::Literal(simple));
            }
        }
        // @lfy def/lexer/main.lfy:65
        if !in_double_quote_string(stack)
            && !in_template(stack)
            && !in_comment(stack)
            && !in_documentation(stack)
        {
            rule(TokenKind::Literal(Literal::SingleQuoteBoundary));
        }
        // @lfy def/lexer/main.lfy:66
        if !in_single_quote_string(stack)
            && !in_template(stack)
            && !in_comment(stack)
            && !in_documentation(stack)
        {
            rule(TokenKind::Literal(Literal::DoubleQuoteBoundary));
        }
        // @lfy def/lexer/main.lfy:67
        if !in_single_quote_string(stack)
            && !in_double_quote_string(stack)
            && !in_comment(stack)
            && !in_documentation(stack)
        {
            rule(TokenKind::Literal(Literal::BacktickBoundary));
        }
        // @lfy def/lexer/main.lfy:69
        if in_template(stack) {
            rule(TokenKind::Literal(Literal::ExecutionOpenBoundary));
        }
        // @lfy def/lexer/main.lfy:70
        if in_template_execution(stack) {
            rule(TokenKind::Literal(Literal::ExecutionCloseBoundary));
        }
        // @lfy def/lexer/main.lfy:71
        if in_template(stack) {
            rule(TokenKind::Literal(Literal::ReferenceOpenBoundary));
        }
        // @lfy def/lexer/main.lfy:72
        if in_template_reference(stack) {
            rule(TokenKind::Literal(Literal::ReferenceCloseBoundary));
        }
        // @lfy def/lexer/main.lfy:74
        if in_single_quote_string(stack) {
            rule(TokenKind::Literal(Literal::SingleQuoteLiteralBody));
        }
        // @lfy def/lexer/main.lfy:75
        if in_double_quote_string(stack) {
            rule(TokenKind::Literal(Literal::DoubleQuoteLiteralBody));
        }
        // @lfy def/lexer/main.lfy:76
        if in_template(stack) {
            rule(TokenKind::Expression(Expression::TemplateLiteralBody));
        }
        // Sequences inside those bodies are replaced when the token value is built.
        // @lfy def/lexer/main.lfy:77

        // Comments
        // @lfy def/lexer/main.lfy:80
        if !in_inline_comment(stack)
            && !in_documentation(stack)
            && !in_string(stack)
            && !in_template(stack)
        {
            rule(TokenKind::Comment(Comment::CommentBlockOpen));
        }
        if in_block_comment(stack) {
            rule(TokenKind::Comment(Comment::CommentBlockClose)); // @lfy def/lexer/main.lfy:81
            rule(TokenKind::Comment(Comment::CommentBlockBody)); // @lfy def/lexer/main.lfy:82
        }
        // @lfy def/lexer/main.lfy:83
        if not_in_content_body_mode(stack) {
            rule(TokenKind::Comment(Comment::CommentInlineStart));
        }
        // @lfy def/lexer/main.lfy:84
        if in_inline_comment(stack) {
            rule(TokenKind::Comment(Comment::CommentInlineBody));
        }
        // @lfy def/lexer/main.lfy:86
        if !in_inline_documentation(stack)
            && !in_comment(stack)
            && !in_string(stack)
            && !in_template(stack)
        {
            rule(TokenKind::Comment(Comment::DocumentationBlockOpen));
        }
        if in_block_documentation(stack) {
            rule(TokenKind::Comment(Comment::DocumentationBlockClose)); // @lfy def/lexer/main.lfy:87
            rule(TokenKind::Comment(Comment::DocumentationBlockBody)); // @lfy def/lexer/main.lfy:88
        }
        // @lfy def/lexer/main.lfy:89
        if not_in_content_body_mode(stack) {
            rule(TokenKind::Comment(Comment::DocumentationInlineStart));
        }
        // @lfy def/lexer/main.lfy:90
        if in_inline_documentation(stack) {
            rule(TokenKind::Comment(Comment::DocumentationInlineBody));
        }
        // @lfy def/lexer/main.lfy:91
        if in_documentation(stack) {
            rule(TokenKind::Literal(Literal::ReferenceOpenBoundary));
        }

        if not_in_content_body_mode(stack) {
            // Keywords
            // @lfy def/lexer/main.lfy:94
            for &keyword in Keyword::ALL {
                rule(TokenKind::Keyword(keyword));
            }
            // Operators
            // @lfy def/lexer/main.lfy:99
            for &operator in Operator::ALL {
                rule(TokenKind::Operator(operator));
            }
            // Separators
            // @lfy def/lexer/main.lfy:104
            for &separator in Separator::ALL {
                rule(TokenKind::Separator(separator));
            }
            // Space
            // @lfy def/lexer/main.lfy:109
            for &space in Space::ALL {
                if let Some(len) = matching_ebnf(RuleInfo::of(space), source, offset) {
                    consider(&mut best, TokenKind::Space, len);
                }
            }
            // Identifiers
            // @lfy def/lexer/main.lfy:114
            if let Some(len) = Identifier::match_identifier(&source[offset..]) {
                consider(&mut best, TokenKind::Identifier, len);
            }
        }

        best
    }

    /// Pushes a token spanning the next `len` bytes and lets the mode stack respond to it.
    // @lfy def/lexer/main.lfy:17
    fn push_token(&mut self, kind: TokenKind, len: usize) {
        let raw = &self.input[self.offset..self.offset + len]; // @lfy def/lexer/main.lfy:35
        let value = match kind {
            // @lfy def/lexer/main.lfy:32
            TokenKind::Space if raw == "\r\n" => "\n".to_owned(),
            // @lfy def/grammar/tokens/literal.lfy:13
            TokenKind::Literal(Literal::NumberLiteral) => literal::normalize_number(raw),
            // @lfy def/lexer/main.lfy:77
            TokenKind::Literal(Literal::DoubleQuoteLiteralBody)
            | TokenKind::Expression(Expression::TemplateLiteralBody) => decode_body(raw),
            _ => raw.to_owned(),
        };
        self.tokens.push(Token {
            kind,
            value,
            raw: raw.to_owned(),
            file: Arc::clone(&self.file),
            line: self.line,
            position: self.position,
        });
        self.advance(len);
        // @lfy def/lexer/main.lfy:36
        traits::on_token_created(kind, &mut self.stack);
    }

    /// Consumes `len` bytes, tracking the 1-indexed line and 0-indexed character position.
    // @lfy def/lexer/main.lfy:33
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

/// Characters are replaced in the value according to the rules of `Sequence`; text that is
/// not a sequence, or a sequence that is not encoded, is kept as it is.
// @lfy def/lexer/main.lfy:77
fn decode_body(raw: &str) -> String {
    let mut value = String::with_capacity(raw.len());
    let mut rest = raw;
    while !rest.is_empty() {
        match literal::decode_sequence(rest) {
            Some((len, replacement)) => {
                match replacement {
                    Replacement::Text(text) => value.push_str(text),
                    Replacement::Char(c) => value.push(c),
                    Replacement::Literal => value.push_str(&rest[..len]),
                }
                rest = &rest[len..];
            }
            None => {
                let c = rest.chars().next().expect("rest is not empty");
                value.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    value
}

#[cfg(test)]
mod tests {
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

    const SP: TokenKind = TokenKind::Space;
    const ID: TokenKind = TokenKind::Identifier;
    const INVALID: TokenKind = TokenKind::Invalid;
    const SINGLE: TokenKind = TokenKind::Literal(Literal::SingleQuoteBoundary);
    const DOUBLE: TokenKind = TokenKind::Literal(Literal::DoubleQuoteBoundary);
    const TEMPLATE: TokenKind = TokenKind::Literal(Literal::BacktickBoundary);
    const SBODY: TokenKind = TokenKind::Literal(Literal::SingleQuoteLiteralBody);
    const DBODY: TokenKind = TokenKind::Literal(Literal::DoubleQuoteLiteralBody);
    const TBODY: TokenKind = TokenKind::Expression(Expression::TemplateLiteralBody);
    const NUMBER: TokenKind = TokenKind::Literal(Literal::NumberLiteral);
    const EXEC_OPEN: TokenKind = TokenKind::Literal(Literal::ExecutionOpenBoundary);
    const EXEC_CLOSE: TokenKind = TokenKind::Literal(Literal::ExecutionCloseBoundary);
    const REF_OPEN: TokenKind = TokenKind::Literal(Literal::ReferenceOpenBoundary);
    const REF_CLOSE: TokenKind = TokenKind::Literal(Literal::ReferenceCloseBoundary);

    fn lit(literal: Literal) -> TokenKind {
        TokenKind::Literal(literal)
    }
    fn kw(keyword: Keyword) -> TokenKind {
        TokenKind::Keyword(keyword)
    }
    fn op(operator: Operator) -> TokenKind {
        TokenKind::Operator(operator)
    }
    fn sep(separator: Separator) -> TokenKind {
        TokenKind::Separator(separator)
    }
    fn cm(comment: Comment) -> TokenKind {
        TokenKind::Comment(comment)
    }

    // @lfy def/lexer/main.lfy:27
    #[test]
    fn input_is_utf8_and_positions_count_characters() {
        let tokens = lex("é ← 日本");
        assert_eq!(
            tokens.iter().map(|t| t.kind).collect::<Vec<_>>(),
            vec![
                ID,
                SP,
                op(Operator::ArrowSingleLeftSingleCharPunctuation),
                SP,
                ID
            ]
        );
        assert_eq!(
            tokens.iter().map(|t| t.position).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
    }

    // @lfy def/lexer/main.lfy:28
    #[test]
    fn tokens_come_out_in_source_order() {
        assert_eq!(
            raws("let x = 1;"),
            vec!["let", " ", "x", " ", "=", " ", "1", ";"]
        );
    }

    // @lfy def/lexer/main.lfy:29
    #[test]
    fn empty_input_yields_no_tokens() {
        assert_eq!(lex(""), Vec::<Token>::new());
    }

    // @lfy def/lexer/main.lfy:30
    #[test]
    fn file_is_recorded_on_every_token_when_given() {
        let tokens = lex_string("a b", Some("lib.lfy")).unwrap();
        assert!(tokens.iter().all(|token| &*token.file == "lib.lfy"));
    }

    // @lfy def/lexer/main.lfy:31
    #[test]
    fn file_defaults_to_anonymous() {
        assert!(lex("a b").iter().all(|token| &*token.file == "anonymous"));
    }

    // @lfy def/lexer/main.lfy:32
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
        // A lone carriage return is not a space character.
        let cr = lex("a\rb");
        assert_eq!(cr[1].kind, INVALID);
        assert_eq!((cr[2].line, cr[2].position), (1, 2));
    }

    // @lfy def/lexer/main.lfy:33
    #[test]
    fn line_is_one_indexed_and_counts_end_of_lines_inside_bodies() {
        let tokens = lex("a\n\nb `x\ny` c");
        let find = |raw: &str| tokens.iter().find(|t| t.raw == raw).unwrap();
        assert_eq!(find("a").line, 1);
        assert_eq!(find("b").line, 3);
        assert_eq!(find("x\ny").line, 3);
        assert_eq!((find("c").line, find("c").position), (4, 3));
    }

    // @lfy def/lexer/main.lfy:34
    #[test]
    fn position_is_zero_indexed_within_the_line() {
        let tokens = lex("ab cd\n  ef");
        let positions: Vec<(usize, usize)> = tokens.iter().map(|t| (t.line, t.position)).collect();
        assert_eq!(
            positions,
            vec![(1, 0), (1, 2), (1, 3), (1, 5), (2, 0), (2, 1), (2, 2)]
        );
    }

    // @lfy def/lexer/main.lfy:35
    #[test]
    fn raw_is_the_original_source_text() {
        let tokens = lex("\"a\\nb\"");
        assert_eq!(tokens[1].raw, "a\\nb");
        assert_eq!(tokens[1].value, "a\nb");
        assert_eq!(
            tokens.iter().map(|t| t.raw.as_str()).collect::<String>(),
            "\"a\\nb\""
        );
    }

    // @lfy def/lexer/main.lfy:36
    #[test]
    fn a_fresh_mode_stack_is_used_for_every_run() {
        assert!(matches!(error("("), LexError::UnexpectedEndOfFile { .. }));
        assert_eq!(kinds(")"), vec![sep(Separator::GroupCloseSeparator)]);
        assert_eq!(kinds("x"), vec![ID]);
    }

    // @lfy def/lexer/main.lfy:37
    #[test]
    fn end_of_input_with_open_modes_is_an_unexpected_end_of_file() {
        for source in [
            "(", "{", "[", "\"abc", "`abc", "`{{", "`[[", "/* x", "/** x", "`{{ (`", "/// [[x",
        ] {
            assert!(
                matches!(error(source), LexError::UnexpectedEndOfFile { .. }),
                "{source:?}"
            );
        }
        // Modes cleared at the end of a line are also cleared at the end of the input.
        for source in ["'abc", "// x", "/// x"] {
            assert!(lex_string(source, None).is_ok(), "{source:?}");
        }
        let LexError::UnexpectedEndOfFile {
            line,
            position,
            open,
            ..
        } = error("(\n[");
        assert_eq!((line, position), (2, 1));
        assert_eq!(open, vec![Mode::Group, Mode::List]);
    }

    // @lfy def/lexer/main.lfy:38
    #[test]
    fn maximal_munch_picks_the_longest_candidate() {
        assert_eq!(kinds("=>"), vec![op(Operator::ArrowDoubleRightPunctuation)]);
        assert_eq!(kinds("=++"), vec![op(Operator::SetterIncrementPunctuation)]);
        assert_eq!(kinds("=**"), vec![op(Operator::SetterPowerPunctuation)]);
        assert_eq!(kinds("**"), vec![op(Operator::PowerPunctuation)]);
        assert_eq!(kinds("..."), vec![op(Operator::SpreadPunctuation)]);
        assert_eq!(kinds("^^:"), vec![op(Operator::RefDefineLastPunctuation)]);
        assert_eq!(kinds("<-"), vec![op(Operator::ArrowSingleLeftPunctuation)]);
        assert_eq!(
            kinds("<="),
            vec![op(Operator::EqualityLessThanOrEqualPunctuation)]
        );
        assert_eq!(
            kinds("=??"),
            vec![op(Operator::SetterCoalescenceNullPunctuation)]
        );
        assert_eq!(
            kinds("a=-1"),
            vec![ID, op(Operator::SetterSubtractivePunctuation), NUMBER]
        );
        assert_eq!(kinds("//"), vec![cm(Comment::CommentInlineStart)]);
        assert_eq!(kinds("///"), vec![cm(Comment::DocumentationInlineStart)]);
        assert_eq!(kinds("matchall"), vec![kw(Keyword::MatchallKeyword)]);
        assert_eq!(kinds("format"), vec![ID]);
        assert_eq!(kinds("trueish"), vec![ID]);
    }

    // @lfy def/lexer/main.lfy:40
    #[test]
    fn equal_length_matches_go_to_the_rule_declared_first() {
        assert_eq!(kinds("true"), vec![lit(Literal::BooleanLiteral)]);
        assert_eq!(kinds("false"), vec![lit(Literal::BooleanLiteral)]);
        assert_eq!(kinds("null"), vec![lit(Literal::NullLiteral)]);
        assert_eq!(kinds("undefined"), vec![lit(Literal::UndefinedLiteral)]);
        assert_eq!(kinds("\n"), vec![SP]);
        assert_eq!(kinds("."), vec![op(Operator::AccessorValuePunctuation)]);
        assert_eq!(kinds("+"), vec![op(Operator::AddPunctuation)]);
        assert_eq!(kinds("="), vec![op(Operator::SetterPlainPunctuation)]);
    }

    // @lfy def/lexer/main.lfy:40
    #[test]
    fn context_decides_which_candidates_are_allowed_before_munching() {
        // `{{` outside a template is two block opens.
        assert_eq!(
            kinds("{{}}"),
            vec![
                sep(Separator::BlockOpenSeparator),
                sep(Separator::BlockOpenSeparator),
                sep(Separator::BlockCloseSeparator),
                sep(Separator::BlockCloseSeparator)
            ]
        );
        // `}}` closes the execution block only when it is on top; a block inside it wins first.
        assert_eq!(
            kinds("`{{{}}}`"),
            vec![
                TEMPLATE,
                EXEC_OPEN,
                sep(Separator::BlockOpenSeparator),
                sep(Separator::BlockCloseSeparator),
                EXEC_CLOSE,
                TEMPLATE
            ]
        );
        // `]]` closes the reference block only when it is on top.
        assert_eq!(
            kinds("`[[a[0]]]`"),
            vec![
                TEMPLATE,
                REF_OPEN,
                ID,
                sep(Separator::ListOpenSeparator),
                NUMBER,
                sep(Separator::ListCloseSeparator),
                REF_CLOSE,
                TEMPLATE
            ]
        );
    }

    // @lfy def/lexer/main.lfy:51
    #[test]
    fn the_ebnf_document_lists_every_token_rule_with_bare_references() {
        let document = ebnf_document();
        assert!(document.starts_with("CommentBlockOpen = \"/*\" ;\n\n"));
        assert!(
            document.contains("\n\nIdentifier = ( XID_Start | \"_\" ) , (: XID_Continue :) ;\n\n")
        );
        assert!(document.ends_with("\"\u{2028}\" | \"\u{2029}\" ;"));
        fn check<R: EbnfSyntax>(document: &str, all: &[R]) {
            for &rule in all {
                assert!(document.contains(rule.rule()), "{}", rule.identifier());
            }
        }
        check(&document, Comment::ALL);
        check(&document, Identifier::ALL);
        check(&document, Keyword::ALL);
        check(&document, Literal::ALL);
        check(&document, Operator::ALL);
        check(&document, Separator::ALL);
        check(&document, Space::ALL);
        assert!(!document.contains("SourceFile"));
        // References inside the EBNF are bare rule names.
        assert_eq!(
            crate::grammar::ebnf::parse("[[NewLine]]").unwrap(),
            crate::grammar::ebnf::Expr::Reference("NewLine".to_owned())
        );
    }

    // @lfy def/lexer/main.lfy:61
    #[test]
    fn simple_literals_are_lexed_outside_content_bodies() {
        assert_eq!(kinds("1_000.5"), vec![NUMBER]);
        assert_eq!(values("1_000.5"), vec!["1000.5"]);
        assert_eq!(raws("1_000.5"), vec!["1_000.5"]);
        assert_eq!(kinds("1_"), vec![NUMBER, ID]);
        assert_eq!(kinds("1abc"), vec![NUMBER, ID]);
        assert_eq!(
            kinds(".5"),
            vec![op(Operator::AccessorValuePunctuation), NUMBER]
        );
        assert_eq!(kinds("'1'"), vec![SINGLE, SBODY, SINGLE]);
        assert_eq!(kinds("`true`"), vec![TEMPLATE, TBODY, TEMPLATE]);
        assert_eq!(kinds("/*null*/")[1], cm(Comment::CommentBlockBody));
        assert_eq!(kinds("`{{1}}`")[2], NUMBER);
    }

    // @lfy def/lexer/main.lfy:65
    #[test]
    fn literal_boundaries_open_and_close_strings() {
        assert_eq!(kinds("'a'"), vec![SINGLE, SBODY, SINGLE]);
        assert_eq!(kinds("\"a\""), vec![DOUBLE, DBODY, DOUBLE]);
        assert_eq!(kinds("`a`"), vec![TEMPLATE, TBODY, TEMPLATE]);
        assert_eq!(kinds("''"), vec![SINGLE, SINGLE]);
        // Not boundaries inside comments or documentation.
        assert_eq!(
            kinds("/* ' */"),
            vec![
                cm(Comment::CommentBlockOpen),
                cm(Comment::CommentBlockBody),
                cm(Comment::CommentBlockClose)
            ]
        );
        assert_eq!(
            kinds("/** ` **/"),
            vec![
                cm(Comment::DocumentationBlockOpen),
                cm(Comment::DocumentationBlockBody),
                cm(Comment::DocumentationBlockClose)
            ]
        );
        // Strings nest inside template execution blocks and templates inside those.
        assert_eq!(
            kinds("`{{'a'}}`"),
            vec![
                TEMPLATE, EXEC_OPEN, SINGLE, SBODY, SINGLE, EXEC_CLOSE, TEMPLATE
            ]
        );
        assert_eq!(
            kinds("`{{`b`}}`"),
            vec![
                TEMPLATE, EXEC_OPEN, TEMPLATE, TBODY, TEMPLATE, EXEC_CLOSE, TEMPLATE
            ]
        );
        // Other boundary characters inside a literal are body text.
        assert_eq!(values("'a\"b`c'"), vec!["'", "a\"b`c", "'"]);
        assert_eq!(values("\"a'b`c\""), vec!["\"", "a'b`c", "\""]);
        assert_eq!(values("`a'b\"c`"), vec!["`", "a'b\"c", "`"]);
    }

    // @lfy def/lexer/main.lfy:69
    #[test]
    fn template_blocks_are_only_recognised_in_their_modes() {
        assert_eq!(
            kinds("`a{{b}}c[[e]]f`"),
            vec![
                TEMPLATE, TBODY, EXEC_OPEN, ID, EXEC_CLOSE, TBODY, REF_OPEN, ID, REF_CLOSE, TBODY,
                TEMPLATE
            ]
        );
        // `}}` and `]]` in plain template text are text; `{{` inside a string is text.
        assert_eq!(values("`a}}b]]c`"), vec!["`", "a}}b]]c", "`"]);
        assert_eq!(values("'{{x}}'"), vec!["'", "{{x}}", "'"]);
        assert_eq!(values("\"[[x]]\""), vec!["\"", "[[x]]", "\""]);
        // `[[` inside an execution block is two list opens, not a reference.
        assert_eq!(
            kinds("`{{[[]]}}`"),
            vec![
                TEMPLATE,
                EXEC_OPEN,
                sep(Separator::ListOpenSeparator),
                sep(Separator::ListOpenSeparator),
                sep(Separator::ListCloseSeparator),
                sep(Separator::ListCloseSeparator),
                EXEC_CLOSE,
                TEMPLATE
            ]
        );
    }

    // @lfy def/lexer/main.lfy:74
    #[test]
    fn single_quote_bodies_are_raw_and_end_at_the_line() {
        assert_eq!(values("'a\\nb'"), vec!["'", "a\\nb", "'"]);
        assert_eq!(values("'a\\'"), vec!["'", "a\\", "'"]);
        // Bail out of single quote literal at the end of the line.
        assert_eq!(kinds("'ab\ncd"), vec![SINGLE, SBODY, SP, ID]);
        assert_eq!(kinds("'ab\r\ncd'"), vec![SINGLE, SBODY, SP, ID, SINGLE]);
    }

    // @lfy def/lexer/main.lfy:75
    #[test]
    fn double_quote_and_template_bodies_replace_sequences_in_the_value() {
        let cases = [
            ("\"\\\\\"", "\\"),
            ("\"\\r\"", "\r"),
            ("\"a\\\nb\"", "ab"),
            ("\"a\\\r\nb\"", "ab"),
            ("\"\\x41\"", "A"),
            ("\"\\u0041\"", "A"),
            ("\"\\u01F600\"", "😀"),
            ("\"\\{{\"", "{{"),
            ("\"\\}}\"", "}}"),
            ("\"\\[[\"", "[["),
            ("\"\\]]\"", "]]"),
            ("\"\\n\"", "\n"),
            ("\"\\0\"", "\0"),
            ("\"\\t\"", "\t"),
            ("\"\\'\"", "'"),
            ("\"\\\"\"", "\""),
            ("\"\\`\"", "`"),
            ("`\\`\\{{\\n`", "`{{\n"),
        ];
        for (source, expected) in cases {
            let tokens = lex(source);
            assert_eq!(tokens.len(), 3, "{source:?}");
            assert_eq!(tokens[1].value, expected, "{source:?}");
            assert_eq!(tokens[1].raw, &source[1..source.len() - 1], "{source:?}");
        }
        // Escaped template blocks stay text inside a template.
        assert_eq!(kinds("`\\{{x\\}}`"), vec![TEMPLATE, TBODY, TEMPLATE]);
        // Non-scalar code points are literal text; unknown sequences are not body text.
        assert_eq!(
            values("\"\\uD800 \\uFFFFFF\""),
            vec!["\"", "\\uD800 \\uFFFFFF", "\""]
        );
        assert_eq!(
            kinds("\"a\\qb\""),
            vec![DOUBLE, DBODY, INVALID, DBODY, DOUBLE]
        );
        assert_eq!(kinds("`\\x4`"), vec![TEMPLATE, INVALID, TBODY, TEMPLATE]);
        // A bare end of line is not double quote body text.
        assert_eq!(
            kinds("\"a\nb\""),
            vec![DOUBLE, DBODY, INVALID, DBODY, DOUBLE]
        );
        // Sequences are not recognised outside strings and templates.
        assert_eq!(kinds("\\n"), vec![INVALID, ID]);
    }

    // @lfy def/lexer/main.lfy:76
    #[test]
    fn template_body_is_one_token_up_to_a_boundary_or_block() {
        let tokens = lex("`ab\ncd {{x}} ef`");
        assert_eq!(tokens[1].kind, TBODY);
        assert_eq!(tokens[1].value, "ab\ncd ");
        assert_eq!(tokens[3].kind, ID);
        assert_eq!(tokens[5].value, " ef");
        assert_eq!(tokens.len(), 7);
    }

    // @lfy def/lexer/main.lfy:80
    #[test]
    fn comment_delimiters_depend_on_the_comment_mode() {
        // Block comments nest.
        // @lfy def/grammar/tokens/comment.lfy:7
        assert_eq!(
            kinds("/* a /* b */ c */"),
            vec![
                cm(Comment::CommentBlockOpen),
                cm(Comment::CommentBlockBody),
                cm(Comment::CommentBlockOpen),
                cm(Comment::CommentBlockBody),
                cm(Comment::CommentBlockClose),
                cm(Comment::CommentBlockBody),
                cm(Comment::CommentBlockClose)
            ]
        );
        // Documentation blocks nest.
        // @lfy def/grammar/tokens/comment.lfy:20
        assert_eq!(
            kinds("/** a /** b **/ c **/"),
            vec![
                cm(Comment::DocumentationBlockOpen),
                cm(Comment::DocumentationBlockBody),
                cm(Comment::DocumentationBlockOpen),
                cm(Comment::DocumentationBlockBody),
                cm(Comment::DocumentationBlockClose),
                cm(Comment::DocumentationBlockBody),
                cm(Comment::DocumentationBlockClose)
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
            vec![
                cm(Comment::CommentBlockOpen),
                cm(Comment::CommentBlockClose)
            ]
        );
        // `*/` is documentation body text, so these never close.
        for source in ["/***/", "/** x */"] {
            assert!(
                matches!(error(source), LexError::UnexpectedEndOfFile { .. }),
                "{source:?}"
            );
        }
        // `**/` ends with a comment close.
        assert_eq!(values("/* x **/"), vec!["/*", " x *", "*/"]);
        // Not delimiters inside strings or templates.
        assert_eq!(values("'/* // */'"), vec!["'", "/* // */", "'"]);
        assert_eq!(values("`/* // */`"), vec!["`", "/* // */", "`"]);
        // Inline comments end at `\n`, `\r\n` and EOF; the end of line is a space token.
        assert_eq!(
            kinds("// x\ny"),
            vec![
                cm(Comment::CommentInlineStart),
                cm(Comment::CommentInlineBody),
                SP,
                ID
            ]
        );
        assert_eq!(
            kinds("// x\r\ny"),
            vec![
                cm(Comment::CommentInlineStart),
                cm(Comment::CommentInlineBody),
                SP,
                ID
            ]
        );
        assert_eq!(values("// x\ry"), vec!["//", " x\ry"]);
        assert_eq!(
            kinds("/// x"),
            vec![
                cm(Comment::DocumentationInlineStart),
                cm(Comment::DocumentationInlineBody)
            ]
        );
        assert_eq!(kinds("//"), vec![cm(Comment::CommentInlineStart)]);
        // Comments open inside template execution blocks.
        assert_eq!(
            kinds("`{{ // c\n}}`"),
            vec![
                TEMPLATE,
                EXEC_OPEN,
                SP,
                cm(Comment::CommentInlineStart),
                cm(Comment::CommentInlineBody),
                SP,
                EXEC_CLOSE,
                TEMPLATE
            ]
        );
    }

    // @lfy def/lexer/main.lfy:82
    #[test]
    fn comment_and_documentation_bodies_are_single_tokens() {
        let tokens = lex("/* a\nb */");
        assert_eq!(tokens[1].kind, cm(Comment::CommentBlockBody));
        assert_eq!(tokens[1].value, " a\nb ");
        assert_eq!(tokens[1].raw, " a\nb ");
        assert_eq!((tokens[1].line, tokens[1].position), (1, 2));
        assert_eq!((tokens[2].line, tokens[2].position), (2, 2));

        let tokens = lex("/** d **/");
        assert_eq!(tokens[1].kind, cm(Comment::DocumentationBlockBody));
        assert_eq!(tokens[1].value, " d ");
    }

    // @lfy def/lexer/main.lfy:91
    #[test]
    fn references_are_lexed_inside_documentation() {
        assert_eq!(
            kinds("/// see [[Foo.bar]] x"),
            vec![
                cm(Comment::DocumentationInlineStart),
                cm(Comment::DocumentationInlineBody),
                REF_OPEN,
                ID,
                op(Operator::AccessorValuePunctuation),
                ID,
                REF_CLOSE,
                cm(Comment::DocumentationInlineBody)
            ]
        );
        assert_eq!(
            kinds("/** [[a]] **/"),
            vec![
                cm(Comment::DocumentationBlockOpen),
                cm(Comment::DocumentationBlockBody),
                REF_OPEN,
                ID,
                REF_CLOSE,
                cm(Comment::DocumentationBlockBody),
                cm(Comment::DocumentationBlockClose)
            ]
        );
        // `]]` outside a reference is not inline documentation text; a lone `]` is.
        assert_eq!(
            kinds("/// a]]b"),
            vec![
                cm(Comment::DocumentationInlineStart),
                cm(Comment::DocumentationInlineBody),
                INVALID,
                cm(Comment::DocumentationInlineBody)
            ]
        );
        assert_eq!(values("/// a]]b"), vec!["///", " a", "]", "]b"]);
        // Not references inside comments or execution blocks.
        assert_eq!(values("// [[a]]"), vec!["//", " [[a]]"]);
        assert_eq!(values("/* [[a]] */")[1], " [[a]] ");
    }

    // @lfy def/lexer/main.lfy:94
    #[test]
    fn every_keyword_lexes_to_its_rule_outside_content_bodies() {
        for &keyword in Keyword::ALL {
            assert_eq!(
                kinds(keyword.text()),
                vec![kw(keyword)],
                "{}",
                keyword.identifier()
            );
            let quoted = format!("'{}'", keyword.text());
            assert_eq!(
                kinds(&quoted),
                vec![SINGLE, SBODY, SINGLE],
                "{}",
                keyword.identifier()
            );
            let commented = format!("/*{}*/", keyword.text());
            assert_eq!(
                kinds(&commented)[1],
                cm(Comment::CommentBlockBody),
                "{}",
                keyword.identifier()
            );
        }
        assert_eq!(kinds("with"), vec![kw(Keyword::WithKeyword)]);
    }

    // @lfy def/lexer/main.lfy:99
    #[test]
    fn every_operator_lexes_to_its_rule_outside_content_bodies() {
        for &operator in Operator::ALL {
            let syntax = operator.syntax();
            if !syntax.starts_with('"') {
                continue; // group rules never win a tie against their members
            }
            let text = &syntax[1..syntax.len() - 1];
            // `^` may only be followed by a number.
            let source = if operator == Operator::BitwiseXorPunctuation {
                "^1".to_string()
            } else {
                text.to_string()
            };
            assert_eq!(kinds(&source)[0], op(operator), "{}", operator.identifier());
            let templated = format!("`{text}`");
            assert_eq!(
                kinds(&templated),
                vec![TEMPLATE, TBODY, TEMPLATE],
                "{}",
                operator.identifier()
            );
        }
        assert_eq!(kinds("^ x"), vec![INVALID, SP, ID]);
        assert_eq!(kinds("^^"), vec![op(Operator::RefLastPunctuation)]);
        assert_eq!(
            kinds("/**/x"),
            vec![
                cm(Comment::CommentBlockOpen),
                cm(Comment::CommentBlockClose),
                ID
            ]
        );
    }

    // @lfy def/lexer/main.lfy:104
    #[test]
    fn every_separator_lexes_to_its_rule_outside_content_bodies() {
        assert_eq!(
            kinds("{}()[],;"),
            vec![
                sep(Separator::BlockOpenSeparator),
                sep(Separator::BlockCloseSeparator),
                sep(Separator::GroupOpenSeparator),
                sep(Separator::GroupCloseSeparator),
                sep(Separator::ListOpenSeparator),
                sep(Separator::ListCloseSeparator),
                sep(Separator::ListContinueSeparator),
                sep(Separator::StatementEndSeparator)
            ]
        );
        assert_eq!(kinds("'{}()[],;'"), vec![SINGLE, SBODY, SINGLE]);
        assert_eq!(kinds("/*{}()[],;*/")[1], cm(Comment::CommentBlockBody));
        // A closing separator with no matching open mode is still a token.
        assert_eq!(kinds("}"), vec![sep(Separator::BlockCloseSeparator)]);
    }

    // @lfy def/lexer/main.lfy:109
    #[test]
    fn every_space_character_is_its_own_space_token() {
        for c in [
            ' ', '\t', '\u{000B}', '\u{000C}', '\u{0085}', '\u{200E}', '\u{200F}', '\u{2028}',
            '\u{2029}',
        ] {
            let source = format!("a{c}{c}b");
            let tokens = lex(&source);
            assert_eq!(tokens.len(), 4, "{c:?}");
            assert_eq!(tokens[1].kind, SP, "{c:?}");
            assert_eq!(tokens[2].kind, SP, "{c:?}");
            assert!(tokens[1].is_space());
        }
        assert_eq!(kinds("\n\r\n"), vec![SP, SP]);
        assert_eq!(kinds("' '"), vec![SINGLE, SBODY, SINGLE]);
        assert_eq!(kinds("/* */")[1], cm(Comment::CommentBlockBody));
    }

    // @lfy def/lexer/main.lfy:114
    #[test]
    fn identifiers_are_xid_runs_that_are_not_keywords() {
        assert_eq!(
            kinds("héllo _x1 __ constant d1"),
            vec![ID, SP, ID, SP, ID, SP, ID, SP, ID]
        );
        assert_eq!(kinds("const"), vec![kw(Keyword::ConstKeyword)]);
        assert_eq!(
            kinds("if()"),
            vec![
                kw(Keyword::IfKeyword),
                sep(Separator::GroupOpenSeparator),
                sep(Separator::GroupCloseSeparator)
            ]
        );
        assert_eq!(kinds("'x'"), vec![SINGLE, SBODY, SINGLE]);
        assert_eq!(kinds("`x`"), vec![TEMPLATE, TBODY, TEMPLATE]);
        assert_eq!(kinds("/*x*/")[1], cm(Comment::CommentBlockBody));
        assert_eq!(kinds("/**x**/")[1], cm(Comment::DocumentationBlockBody));
    }

    // @lfy def/lexer/main.lfy:116
    #[test]
    fn characters_matching_no_rule_become_invalid_tokens() {
        let tokens = lex("ab\n c#");
        assert_eq!(
            tokens.iter().map(|t| t.kind).collect::<Vec<_>>(),
            vec![ID, SP, SP, ID, INVALID]
        );
        let invalid = &tokens[4];
        assert_eq!((invalid.line, invalid.position), (2, 2));
        assert_eq!((invalid.raw.as_str(), invalid.value.as_str()), ("#", "#"));
        assert_eq!(&*invalid.file, "anonymous");
        assert_eq!(kinds("\u{FEFF}x"), vec![INVALID, ID]);
        // One token per unmatched character; lexing continues afterwards.
        assert_eq!(kinds("##x"), vec![INVALID, INVALID, ID]);
    }

    // @lfy def/lexer/main.lfy:28
    #[test]
    fn the_elfie_definitions_lex_start_to_finish_without_invalid_tokens() {
        for path in [
            "def/grammar/expression.lfy",
            "def/grammar/file.lfy",
            "def/grammar/statement.lfy",
            "def/grammar/sugar.lfy",
            "def/grammar/tokens/comment.lfy",
            "def/grammar/tokens/identifier.lfy",
            "def/grammar/tokens/keyword.lfy",
            "def/grammar/tokens/literal.lfy",
            "def/grammar/tokens/operator.lfy",
            "def/grammar/tokens/separator.lfy",
            "def/grammar/tokens/space.lfy",
            "def/grammar/tokens/traits.lfy",
            "def/grammar/traits.lfy",
            "def/grammar/types.lfy",
            "def/lexer/data.lfy",
            "def/lexer/main.lfy",
            "def/lexer/modes.lfy",
            "def/lexer/traits.lfy",
        ] {
            let source = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            let tokens = lex_string(&source, Some(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert_eq!(
                tokens.iter().map(|t| t.raw.as_str()).collect::<String>(),
                source,
                "{path}"
            );
            let invalid: Vec<&Token> = tokens.iter().filter(|t| t.kind == INVALID).collect();
            assert!(invalid.is_empty(), "{path}: {invalid:?}");
            assert!(tokens.iter().all(|t| &*t.file == path));
        }
    }

    #[test]
    fn a_representative_program_lexes_end_to_end() {
        let source = "fn add(a: number, b?: number): `Add {{a}} to [[b]]` => number {\r\n  // sum\n  return a + b;\n}\n";
        let tokens = lex(source);
        assert_eq!(
            tokens.iter().map(|t| t.raw.as_str()).collect::<String>(),
            source
        );
        assert_eq!(tokens[0].kind, kw(Keyword::AgentFnKeyword));
        assert!(tokens.iter().any(|t| t.kind == EXEC_OPEN));
        assert!(tokens.iter().any(|t| t.kind == REF_OPEN));
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == cm(Comment::CommentInlineBody) && t.value == " sum")
        );
        assert!(tokens.iter().all(|t| t.kind != INVALID));
        let last = tokens.last().unwrap();
        assert_eq!((last.kind, last.line), (SP, 4));
    }
}
