//! EBNF parsing and matching behind the `ebnfSyntax` trait.
//!
//! Compiled support for `def/grammar/tokens/traits.lfy` (`ebnfSyntax`), the external rules
//! of `def/grammar/tokens/identifier.lfy` and `def/grammar/tokens/literal.lfy`, and the
//! EBNF reading rules of `def/lexer/main.lfy`.
//!
//! Rule syntax is ISO/IEC 14977 EBNF: `,` concatenation, `|` alternation, `-` exception,
//! `( )` grouping, `(/ /)` optional, `(: :)` repetition, `"…"` / `'…'` terminals and
//! `? … ?` special sequences. `[[Name]]` and a bare `Name` both reference the rule `Name`.
//!
//! Matching works on characters. `A - B` matches what `A` matches unless `B` matches at
//! the same position; optional, repetition and alternation keep every reachable length so
//! callers can take the longest one (maximal munch) or check for a complete match.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::OnceLock;

use unicode_ident::{is_xid_continue, is_xid_start};

use super::tokens::traits::is_forbidden;
use super::{RuleInfo, rules};

/// A parsed EBNF syntax expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// `"text"` or `'text'`.
    Terminal(String),
    /// `[[Name]]` or a bare `Name`.
    Reference(String),
    /// `? description ?`.
    Special(String),
    /// `a , b , c`.
    Sequence(Vec<Expr>),
    /// `a | b | c`.
    Alternation(Vec<Expr>),
    /// `a - b`.
    Exception(Box<Expr>, Box<Expr>),
    /// `(/ a /)`.
    Optional(Box<Expr>),
    /// `(: a :)`.
    Repetition(Box<Expr>),
}

impl Expr {
    /// Every rule name referenced by this expression, in order of appearance.
    pub fn references(&self) -> Vec<&str> {
        fn walk<'e>(expr: &'e Expr, out: &mut Vec<&'e str>) {
            match expr {
                Expr::Reference(name) => out.push(name),
                Expr::Terminal(_) | Expr::Special(_) => {}
                Expr::Sequence(items) | Expr::Alternation(items) => {
                    items.iter().for_each(|item| walk(item, out));
                }
                Expr::Exception(included, excluded) => {
                    walk(included, out);
                    walk(excluded, out);
                }
                Expr::Optional(inner) | Expr::Repetition(inner) => walk(inner, out),
            }
        }
        let mut out = Vec::new();
        walk(self, &mut out);
        out
    }
}

/// A syntax error in a rule's EBNF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset into the syntax.
    pub position: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.message, self.position)
    }
}

/// Parses the syntax of one rule (the part after `=` and before `;`).
// @lfy def/grammar/tokens/traits.lfy:11
pub fn parse(syntax: &str) -> Result<Expr, ParseError> {
    let mut parser = Parser {
        src: syntax,
        pos: 0,
    };
    let expr = parser.alternation()?;
    parser.skip_whitespace();
    if parser.pos != syntax.len() {
        return Err(parser.error("expected the end of the syntax"));
    }
    Ok(expr)
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError {
            position: self.pos,
            message: message.to_owned(),
        }
    }

    fn skip_whitespace(&mut self) {
        self.pos = self.src.len() - self.rest().trim_start().len();
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip_whitespace();
        if self.rest().starts_with(token) {
            self.pos += token.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: &str) -> Result<(), ParseError> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {token:?}")))
        }
    }

    /// `sequence { "|" sequence }`
    fn alternation(&mut self) -> Result<Expr, ParseError> {
        let mut alternatives = vec![self.sequence()?];
        while self.eat("|") {
            alternatives.push(self.sequence()?);
        }
        Ok(match alternatives.len() {
            1 => alternatives.pop().expect("one alternative"),
            _ => Expr::Alternation(alternatives),
        })
    }

    /// `exception { "," exception }`
    fn sequence(&mut self) -> Result<Expr, ParseError> {
        let mut items = vec![self.exception()?];
        while self.eat(",") {
            items.push(self.exception()?);
        }
        Ok(match items.len() {
            1 => items.pop().expect("one item"),
            _ => Expr::Sequence(items),
        })
    }

    /// `factor [ "-" factor ]`
    fn exception(&mut self) -> Result<Expr, ParseError> {
        let included = self.factor()?;
        if self.eat("-") {
            let excluded = self.factor()?;
            return Ok(Expr::Exception(Box::new(included), Box::new(excluded)));
        }
        Ok(included)
    }

    fn factor(&mut self) -> Result<Expr, ParseError> {
        if self.eat("(/") {
            let inner = self.alternation()?;
            self.expect("/)")?;
            return Ok(Expr::Optional(Box::new(inner)));
        }
        if self.eat("(:") {
            let inner = self.alternation()?;
            self.expect(":)")?;
            return Ok(Expr::Repetition(Box::new(inner)));
        }
        if self.eat("(") {
            let inner = self.alternation()?;
            self.expect(")")?;
            return Ok(inner);
        }
        // @lfy def/lexer/main.lfy:51
        if self.eat("[[") {
            let name = self.name()?;
            self.expect("]]")?;
            return Ok(Expr::Reference(name));
        }
        if self.eat("\"") {
            return self.terminal('"');
        }
        if self.eat("'") {
            return self.terminal('\'');
        }
        if self.eat("?") {
            return self.special();
        }
        self.skip_whitespace();
        if self.rest().starts_with(is_name_char) {
            return Ok(Expr::Reference(self.name()?));
        }
        Err(self.error("expected a terminal, reference, special sequence or group"))
    }

    fn terminal(&mut self, quote: char) -> Result<Expr, ParseError> {
        let rest = self.rest();
        let Some(end) = rest.find(quote) else {
            return Err(self.error("unterminated terminal"));
        };
        if end == 0 {
            return Err(self.error("empty terminal"));
        }
        self.pos += end + quote.len_utf8();
        Ok(Expr::Terminal(rest[..end].to_owned()))
    }

    fn special(&mut self) -> Result<Expr, ParseError> {
        let rest = self.rest();
        let Some(end) = rest.find('?') else {
            return Err(self.error("unterminated special sequence"));
        };
        self.pos += end + 1;
        Ok(Expr::Special(rest[..end].trim().to_owned()))
    }

    fn name(&mut self) -> Result<String, ParseError> {
        self.skip_whitespace();
        let rest = self.rest();
        let len = rest.find(|c| !is_name_char(c)).unwrap_or(rest.len());
        if len == 0 {
            return Err(self.error("expected a rule name"));
        }
        self.pos += len;
        Ok(rest[..len].to_owned())
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// External rules whose `? … ?` special sequence is provided by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    /// `XID_Start` as defined by Unicode (`unicode-ident`).
    XidStart, // @lfy def/grammar/tokens/identifier.lfy:3
    /// `XID_Continue` as defined by Unicode (`unicode-ident`).
    XidContinue, // @lfy def/grammar/tokens/identifier.lfy:4
    /// All UTF-8 compatible character codes.
    Utf8Character, // @lfy def/grammar/tokens/literal.lfy:4
}

impl Builtin {
    fn of(identifier: &str) -> Option<Builtin> {
        match identifier {
            "XID_Start" => Some(Builtin::XidStart),
            "XID_Continue" => Some(Builtin::XidContinue),
            "UTF8_CHARACTER" => Some(Builtin::Utf8Character),
            _ => None,
        }
    }

    /// Whether the built-in matches this single character.
    pub fn accepts(self, c: char) -> bool {
        match self {
            Builtin::XidStart => is_xid_start(c),
            Builtin::XidContinue => is_xid_continue(c),
            Builtin::Utf8Character => true,
        }
    }

    fn ends(self, source: &str, start: usize) -> Vec<usize> {
        match source[start..].chars().next() {
            Some(c) if self.accepts(c) => vec![start + c.len_utf8()],
            _ => Vec::new(),
        }
    }
}

/// An error found while compiling the rule set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Syntax {
        identifier: &'static str,
        error: ParseError,
    },
    UnknownSpecialSequence {
        identifier: &'static str,
        special: String,
    },
    UnresolvedReference {
        identifier: &'static str,
        reference: String,
    },
    DuplicateIdentifier(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Syntax { identifier, error } => write!(f, "{identifier}: {error}"),
            Error::UnknownSpecialSequence {
                identifier,
                special,
            } => write!(f, "{identifier}: no built-in provides ? {special} ?"),
            Error::UnresolvedReference {
                identifier,
                reference,
            } => write!(f, "{identifier}: references undefined rule {reference}"),
            Error::DuplicateIdentifier(identifier) => write!(f, "{identifier} is declared twice"),
        }
    }
}

impl std::error::Error for Error {}

enum Body {
    Expr(Expr),
    Builtin(Builtin),
}

struct Compiled {
    info: RuleInfo,
    body: Body,
}

/// The complete rule set with every syntax parsed and every reference resolved.
pub struct Grammar {
    rules: HashMap<&'static str, Compiled>,
}

/// Nesting depth of rule references beyond which matching gives up (guards against
/// left-recursive rules, which only exist above the token level).
const MAX_DEPTH: usize = 64;

impl Grammar {
    /// Compiles every rule returned by [`rules`].
    // @lfy def/grammar/tokens/traits.lfy:11
    pub fn compile() -> Result<Grammar, Vec<Error>> {
        let mut compiled: HashMap<&'static str, Compiled> = HashMap::new();
        let mut errors = Vec::new();
        for info in rules() {
            let identifier = info.identifier();
            let body = match parse(info.ebnf.syntax) {
                Err(error) => {
                    errors.push(Error::Syntax { identifier, error });
                    continue;
                }
                Ok(Expr::Special(special)) => match Builtin::of(identifier) {
                    Some(builtin) => Body::Builtin(builtin),
                    None => {
                        errors.push(Error::UnknownSpecialSequence {
                            identifier,
                            special,
                        });
                        continue;
                    }
                },
                Ok(expr) => Body::Expr(expr),
            };
            if compiled
                .insert(identifier, Compiled { info, body })
                .is_some()
            {
                errors.push(Error::DuplicateIdentifier(identifier));
            }
        }
        for info in rules() {
            let identifier = info.identifier();
            if let Some(Compiled {
                body: Body::Expr(expr),
                ..
            }) = compiled.get(identifier)
            {
                for reference in expr.references() {
                    if !compiled.contains_key(reference) {
                        errors.push(Error::UnresolvedReference {
                            identifier,
                            reference: reference.to_owned(),
                        });
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(Grammar { rules: compiled })
        } else {
            Err(errors)
        }
    }

    /// The rule with this identifier.
    pub fn rule(&self, identifier: &str) -> Option<RuleInfo> {
        self.rules.get(identifier).map(|rule| rule.info)
    }

    /// The parsed expression of a rule; `None` for built-ins and unknown identifiers.
    pub fn expression(&self, identifier: &str) -> Option<&Expr> {
        match self.rules.get(identifier)?.body {
            Body::Expr(ref expr) => Some(expr),
            Body::Builtin(_) => None,
        }
    }

    /// The built-in matcher of an external rule.
    pub fn builtin(&self, identifier: &str) -> Option<Builtin> {
        match self.rules.get(identifier)?.body {
            Body::Builtin(builtin) => Some(builtin),
            Body::Expr(_) => None,
        }
    }

    /// Byte length of the longest non-empty prefix of `input` matched by the rule.
    pub fn longest_match(&self, identifier: &str, input: &str) -> Option<usize> {
        self.longest_match_at(identifier, input, 0)
    }

    /// Byte length of the longest non-empty match of the rule at `source[start..]`. The
    /// text before `start` is what the rule's `notAllowedPrefix` traits inspect.
    pub fn longest_match_at(&self, identifier: &str, source: &str, start: usize) -> Option<usize> {
        let rule = self.rules.get(identifier)?;
        if let (Body::Expr(Expr::Terminal(text)), []) = (&rule.body, rule.info.traits) {
            return source[start..]
                .starts_with(text.as_str())
                .then_some(text.len());
        }
        self.ends_of_rule(identifier, source, start, 0)
            .pop()
            .map(|end| end - start)
            .filter(|&len| len > 0)
    }

    /// Whether the rule matches the whole of `input`.
    pub fn matches(&self, identifier: &str, input: &str) -> bool {
        self.ends_of_rule(identifier, input, 0, 0)
            .contains(&input.len())
    }

    /// Every offset at which a match of the rule starting at `start` ends, ascending. A
    /// candidate match is dropped when a trait of the rule forbids the text around it.
    fn ends_of_rule(
        &self,
        identifier: &str,
        source: &str,
        start: usize,
        depth: usize,
    ) -> Vec<usize> {
        if depth > MAX_DEPTH {
            return Vec::new();
        }
        let Some(rule) = self.rules.get(identifier) else {
            return Vec::new();
        };
        let mut ends = match &rule.body {
            Body::Expr(expr) => self.ends(expr, source, start, depth),
            Body::Builtin(builtin) => builtin.ends(source, start),
        };
        // @lfy def/grammar/tokens/traits.lfy:21
        if !rule.info.traits.is_empty() {
            ends.retain(|&end| !is_forbidden(rule.info.traits, &source[..start], &source[end..]));
        }
        ends
    }

    /// Every offset at which a match of `expr` starting at `start` ends, ascending.
    fn ends(&self, expr: &Expr, source: &str, start: usize, depth: usize) -> Vec<usize> {
        match expr {
            Expr::Terminal(text) => match source[start..].starts_with(text.as_str()) {
                true => vec![start + text.len()],
                false => Vec::new(),
            },
            Expr::Reference(name) => self.ends_of_rule(name, source, start, depth + 1),
            Expr::Special(_) => Vec::new(),
            Expr::Sequence(items) => {
                let mut current = vec![start];
                for item in items {
                    let mut next = Vec::new();
                    for &position in &current {
                        next.extend(self.ends(item, source, position, depth));
                    }
                    normalize(&mut next);
                    if next.is_empty() {
                        return next;
                    }
                    current = next;
                }
                current
            }
            Expr::Alternation(alternatives) => {
                let mut all = Vec::new();
                for alternative in alternatives {
                    all.extend(self.ends(alternative, source, start, depth));
                }
                normalize(&mut all);
                all
            }
            Expr::Exception(included, excluded) => {
                if self
                    .ends(excluded, source, start, depth)
                    .iter()
                    .any(|&end| end > start)
                {
                    Vec::new()
                } else {
                    self.ends(included, source, start, depth)
                }
            }
            Expr::Optional(inner) => {
                let mut all = vec![start];
                all.extend(self.ends(inner, source, start, depth));
                normalize(&mut all);
                all
            }
            Expr::Repetition(inner) => {
                let mut reached = BTreeSet::from([start]);
                let mut frontier = vec![start];
                while let Some(position) = frontier.pop() {
                    for end in self.ends(inner, source, position, depth) {
                        if end > position && reached.insert(end) {
                            frontier.push(end);
                        }
                    }
                }
                reached.into_iter().collect()
            }
        }
    }
}

fn normalize(ends: &mut Vec<usize>) {
    ends.sort_unstable();
    ends.dedup();
}

/// The compiled grammar, built on first use. The rule set is fixed at compile time, so a
/// failure here is a defect in `def/grammar` and is reported by panicking.
pub fn grammar() -> &'static Grammar {
    static GRAMMAR: OnceLock<Grammar> = OnceLock::new();
    GRAMMAR.get_or_init(|| {
        Grammar::compile().unwrap_or_else(|errors| {
            let errors: Vec<String> = errors.iter().map(ToString::to_string).collect();
            panic!("the grammar does not compile:\n{}", errors.join("\n"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(text: &str) -> Expr {
        Expr::Terminal(text.to_owned())
    }

    fn reference(name: &str) -> Expr {
        Expr::Reference(name.to_owned())
    }

    #[test]
    fn exception_binds_tighter_than_concatenation_which_binds_tighter_than_alternation() {
        assert_eq!(
            parse(r#"[[A]] , "b" | C - "d""#).unwrap(),
            Expr::Alternation(vec![
                Expr::Sequence(vec![reference("A"), terminal("b")]),
                Expr::Exception(Box::new(reference("C")), Box::new(terminal("d"))),
            ])
        );
    }

    #[test]
    fn brackets_groups_specials_and_quotes_parse() {
        assert_eq!(
            parse(r#"(/ 'x' /) , (: ( "y" ) :)"#).unwrap(),
            Expr::Sequence(vec![
                Expr::Optional(Box::new(terminal("x"))),
                Expr::Repetition(Box::new(terminal("y"))),
            ])
        );
        assert_eq!(
            parse("? all UTF-8 compatible character codes ?").unwrap(),
            Expr::Special("all UTF-8 compatible character codes".to_owned())
        );
        assert_eq!(parse(r#"'"'"#).unwrap(), terminal("\""));
        assert_eq!(parse(r#""\""#).unwrap(), terminal("\\"));
        assert_eq!(parse("\"\\\n\"").unwrap(), terminal("\\\n"));
    }

    #[test]
    fn malformed_syntax_is_rejected() {
        assert!(parse("").is_err());
        assert!(parse(r#""a" ,"#).is_err());
        assert!(parse(r#"( "a""#).is_err());
        assert!(parse(r#""a"#).is_err());
        assert!(parse(r#""""#).is_err());
        assert!(parse("[[A]] extra").is_err());
    }

    #[test]
    fn references_are_collected_in_order() {
        let expr = parse("[[A]] , ( B | [[C]] ) - D").unwrap();
        assert_eq!(expr.references(), vec!["A", "B", "C", "D"]);
    }

    // @lfy def/grammar/tokens/traits.lfy:11
    #[test]
    fn the_real_grammar_compiles() {
        let grammar = grammar();
        assert!(grammar.expression("NumberLiteral").is_some());
        assert_eq!(grammar.builtin("XID_Start"), Some(Builtin::XidStart));
        assert_eq!(
            grammar.builtin("UTF8_CHARACTER"),
            Some(Builtin::Utf8Character)
        );
        assert_eq!(grammar.rule("Space").unwrap().identifier(), "Space");
        assert!(grammar.rule("Nope").is_none());
    }

    #[test]
    fn longest_match_is_maximal_and_never_empty() {
        let grammar = grammar();
        assert_eq!(
            grammar.longest_match("MatchallKeyword", "matchall x"),
            Some(8)
        );
        assert_eq!(grammar.longest_match("MatchKeyword", "matchall x"), Some(5));
        assert_eq!(grammar.longest_match("MatchKeyword", "mat"), None);
        assert_eq!(grammar.longest_match("NewLine", "\r\nx"), Some(2));
        assert_eq!(grammar.longest_match("NewLine", "\rx"), None);
        assert_eq!(grammar.longest_match("CommentInlineBody", ""), None);
        assert_eq!(grammar.longest_match("CommentInlineBody", "\nx"), None);
        assert_eq!(grammar.longest_match("CommentInlineBody", "ab\nx"), Some(2));
    }

    #[test]
    fn repetition_backtracks_so_a_trailing_underscore_is_not_part_of_a_number() {
        let grammar = grammar();
        assert_eq!(grammar.longest_match("NumberLiteral", "1_000.5x"), Some(7));
        assert_eq!(grammar.longest_match("NumberLiteral", "12__3"), Some(5));
        assert_eq!(grammar.longest_match("NumberLiteral", "1_"), Some(1));
        assert_eq!(grammar.longest_match("NumberLiteral", "1."), Some(1));
        assert_eq!(grammar.longest_match("NumberLiteral", "1.5.3"), Some(3));
        assert_eq!(grammar.longest_match("NumberLiteral", "_1"), None);
        assert!(grammar.matches("NumberLiteral", "1_0.0_1"));
        assert!(!grammar.matches("NumberLiteral", "1_"));
    }

    #[test]
    fn an_exception_excludes_positions_where_the_excluded_text_starts() {
        let grammar = grammar();
        assert_eq!(grammar.longest_match("CommentBlockBody", "ab*/c"), Some(2));
        assert_eq!(grammar.longest_match("CommentBlockBody", "a/*b"), Some(1));
        assert_eq!(grammar.longest_match("CommentBlockBody", "*/"), None);
        assert_eq!(
            grammar.longest_match("SingleQuoteLiteralBody", "a\r\nb"),
            Some(1)
        );
        assert_eq!(
            grammar.longest_match("SingleQuoteLiteralBody", "a'b"),
            Some(1)
        );
    }

    #[test]
    fn builtins_match_one_character() {
        let grammar = grammar();
        assert_eq!(grammar.longest_match("XID_Start", "é1"), Some(2));
        assert_eq!(grammar.longest_match("XID_Start", "1"), None);
        assert_eq!(grammar.longest_match("XID_Continue", "1"), Some(1));
        assert_eq!(grammar.longest_match("UTF8_CHARACTER", "日本"), Some(3));
        assert_eq!(grammar.longest_match("UTF8_CHARACTER", ""), None);
        assert_eq!(grammar.longest_match("Identifier", "_x1-"), Some(3));
        assert_eq!(grammar.longest_match("Identifier", "1x"), None);
    }

    // @lfy def/grammar/tokens/traits.lfy:27
    #[test]
    fn rule_traits_apply_wherever_the_rule_is_matched() {
        let grammar = grammar();
        assert_eq!(
            grammar.longest_match_at("BitwiseXorPunctuation", "a^ x", 1),
            None
        );
        assert_eq!(
            grammar.longest_match_at("BitwiseXorPunctuation", "a^1", 1),
            Some(1)
        );
        assert_eq!(grammar.longest_match("BitwiseXorPunctuation", "^"), Some(1));
        // Through a reference from a group rule as well.
        assert_eq!(
            grammar.longest_match_at("BitwisePunctuation", "a^ x", 1),
            None
        );
        assert_eq!(
            grammar.longest_match_at("BitwisePunctuation", "a^1", 1),
            Some(1)
        );
        assert_eq!(
            grammar.longest_match_at("BitwisePunctuation", "a&b", 1),
            Some(1)
        );
        // And inside an exception.
        assert_eq!(
            grammar.longest_match("DocumentationBlockOpen", "/**/"),
            None
        );
        assert_eq!(
            grammar.longest_match("DocumentationBlockBody", "x /**/"),
            Some(3)
        );
        assert_eq!(
            grammar.longest_match("DocumentationBlockBody", "x /** y"),
            Some(2)
        );
    }

    #[test]
    fn compile_reports_every_problem_it_finds() {
        assert!(Grammar::compile().is_ok());
        let error = Error::UnresolvedReference {
            identifier: "A",
            reference: "B".to_owned(),
        };
        assert_eq!(error.to_string(), "A: references undefined rule B");
    }
}
