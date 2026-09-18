//! EBNF parsing and matching behind the `rule` trait.
//!
//! Compiled support for `def/grammar/traits.lfy` (`rule`), the external character classes
//! of `def/grammar/terminals/identifier.lfy` and `def/grammar/terminals/literal.lfy`, and
//! the EBNF reading rules of `def/lexer/main.lfy`.
//!
//! Rule syntax is ISO/IEC 14977 EBNF: `,` concatenation, `|` alternation, `-` exception,
//! `( )` grouping, `(/ /)` optional, `(: :)` repetition, `"…"` / `'…'` terminals and
//! `? … ?` special sequences. `[[Name]]` and a bare `Name` both reference the rule `Name`.
//!
//! Matching works on characters. `A - B` matches what `A` matches unless `B` matches at
//! the same position; optional, repetition and alternation keep every reachable length so
//! callers can take the longest one or check for a complete match.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use unicode_ident::{is_xid_continue, is_xid_start};

use super::{Entity, GrammarRule, rules};

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
    /// Visits this expression and every expression nested inside it, outermost first.
    pub fn for_each<'e>(&'e self, visit: &mut impl FnMut(&'e Expr)) {
        visit(self);
        match self {
            Expr::Terminal(_) | Expr::Reference(_) | Expr::Special(_) => {}
            Expr::Sequence(items) | Expr::Alternation(items) => {
                items.iter().for_each(|item| item.for_each(visit));
            }
            Expr::Exception(included, excluded) => {
                included.for_each(visit);
                excluded.for_each(visit);
            }
            Expr::Optional(inner) | Expr::Repetition(inner) => inner.for_each(visit),
        }
    }

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
// @lfy def/grammar/traits.lfy:6
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
        // @lfy def/lexer/main.lfy:99
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

/// External rules whose `? … ?` special sequence is provided by the compiler: the
/// character classes the lexer already knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    /// `XID_Start` as defined by Unicode (`unicode-ident`).
    IdentifierStart, // @lfy def/grammar/terminals/identifier.lfy:4
    /// `XID_Continue` as defined by Unicode (`unicode-ident`).
    IdentifierContinue, // @lfy def/grammar/terminals/identifier.lfy:5
    /// Any UTF-8 scalar value.
    Character, // @lfy def/grammar/terminals/literal.lfy:5
}

impl Builtin {
    fn of(identifier: &str) -> Option<Builtin> {
        match identifier {
            "IdentifierStart" => Some(Builtin::IdentifierStart),
            "IdentifierContinue" => Some(Builtin::IdentifierContinue),
            "Character" => Some(Builtin::Character),
            _ => None,
        }
    }

    /// Whether the built-in matches this single character.
    pub fn accepts(self, c: char) -> bool {
        match self {
            Builtin::IdentifierStart => is_xid_start(c),
            Builtin::IdentifierContinue => is_xid_continue(c),
            Builtin::Character => true,
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
    rule: Entity,
    body: Body,
}

/// The complete rule set with every syntax parsed and every reference resolved.
pub struct Grammar {
    rules: HashMap<&'static str, Compiled>,
    /// The rules that can be satisfied without taking anything: an empty match.
    nullable: HashSet<&'static str>,
}

impl Grammar {
    /// Compiles every rule returned by [`rules`].
    // @lfy def/grammar/traits.lfy:19
    pub fn compile() -> Result<Grammar, Vec<Error>> {
        let mut compiled: HashMap<&'static str, Compiled> = HashMap::new();
        let mut errors = Vec::new();
        for rule in rules() {
            let identifier = rule.identifier();
            let body = match parse(rule.syntax()) {
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
                .insert(identifier, Compiled { rule, body })
                .is_some()
            {
                errors.push(Error::DuplicateIdentifier(identifier));
            }
        }
        // @lfy def/grammar/main.lfy:31
        for rule in rules() {
            let identifier = rule.identifier();
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
            let nullable = nullable_rules(&compiled);
            Ok(Grammar {
                rules: compiled,
                nullable,
            })
        } else {
            Err(errors)
        }
    }

    /// Whether the rule can be satisfied without taking anything.
    // @lfy def/grammar/main.lfy:37
    pub fn is_nullable(&self, identifier: &str) -> bool {
        self.nullable.contains(identifier)
    }

    /// Whether `expr` can be satisfied without taking anything: optional groups and
    /// repetitions always can, a sequence can when every item can, an alternation when
    /// any alternative can, and a reference when the rule it names can.
    // @lfy def/grammar/main.lfy:37
    pub fn is_expr_nullable(&self, expr: &Expr) -> bool {
        expr_nullable(expr, &self.nullable)
    }

    /// The rule with this identifier.
    pub fn rule(&self, identifier: &str) -> Option<Entity> {
        self.rules.get(identifier).map(|rule| rule.rule)
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

    /// Byte length of the longest non-empty match of the rule at `source[start..]`.
    pub fn longest_match_at(&self, identifier: &str, source: &str, start: usize) -> Option<usize> {
        let rule = self.rules.get(identifier)?;
        if let Body::Expr(Expr::Terminal(text)) = &rule.body {
            return source[start..]
                .starts_with(text.as_str())
                .then_some(text.len());
        }
        Matcher::new(self, source)
            .ends_of_rule(identifier, start)
            .pop()
            .map(|end| end - start)
            .filter(|&len| len > 0)
    }

    /// Whether the rule matches the whole of `input`.
    pub fn matches(&self, identifier: &str, input: &str) -> bool {
        Matcher::new(self, input)
            .ends_of_rule(identifier, 0)
            .contains(&input.len())
    }
}

fn expr_nullable(expr: &Expr, nullable: &HashSet<&'static str>) -> bool {
    match expr {
        Expr::Terminal(_) | Expr::Special(_) => false,
        Expr::Reference(name) => nullable.contains(name.as_str()),
        Expr::Sequence(items) => items.iter().all(|item| expr_nullable(item, nullable)),
        Expr::Alternation(items) => items.iter().any(|item| expr_nullable(item, nullable)),
        Expr::Exception(included, _) => expr_nullable(included, nullable),
        Expr::Optional(_) | Expr::Repetition(_) => true,
    }
}

/// The rules that can match nothing, found as a fixed point over every rule's syntax. A
/// terminal is delivered as one token and never counts as nullable, whatever its
/// character-level syntax allows: a body that matches no characters creates no token.
fn nullable_rules(compiled: &HashMap<&'static str, Compiled>) -> HashSet<&'static str> {
    let mut nullable: HashSet<&'static str> = HashSet::new();
    loop {
        let mut changed = false;
        for (&identifier, rule) in compiled {
            if let Body::Expr(expr) = &rule.body
                && !rule.rule.is_terminal()
                && !nullable.contains(identifier)
                && expr_nullable(expr, &nullable)
            {
                nullable.insert(identifier);
                changed = true;
            }
        }
        if !changed {
            return nullable;
        }
    }
}

/// One match of the grammar against one source text. Every rule's ends at every start
/// are computed once; a rule reached again while it is being computed at the same start
/// (left recursion, which only exists above the terminal level) contributes no ends to
/// itself, so matching always terminates.
struct Matcher<'g, 's> {
    grammar: &'g Grammar,
    source: &'s str,
    ends: HashMap<(&'g str, usize), Vec<usize>>,
    in_progress: HashSet<(&'g str, usize)>,
}

impl<'g, 's> Matcher<'g, 's> {
    fn new(grammar: &'g Grammar, source: &'s str) -> Self {
        Matcher {
            grammar,
            source,
            ends: HashMap::new(),
            in_progress: HashSet::new(),
        }
    }

    /// Every offset at which a match of the rule starting at `start` ends, ascending.
    fn ends_of_rule(&mut self, identifier: &str, start: usize) -> Vec<usize> {
        let Some((&identifier, rule)) = self.grammar.rules.get_key_value(identifier) else {
            return Vec::new();
        };
        let key = (identifier, start);
        if let Some(ends) = self.ends.get(&key) {
            return ends.clone();
        }
        if !self.in_progress.insert(key) {
            return Vec::new();
        }
        let ends = match &rule.body {
            Body::Expr(expr) => self.ends(expr, start),
            Body::Builtin(builtin) => builtin.ends(self.source, start),
        };
        self.in_progress.remove(&key);
        self.ends.insert(key, ends.clone());
        ends
    }

    /// Every offset at which a match of `expr` starting at `start` ends, ascending.
    fn ends(&mut self, expr: &'g Expr, start: usize) -> Vec<usize> {
        match expr {
            Expr::Terminal(text) => match self.source[start..].starts_with(text.as_str()) {
                true => vec![start + text.len()],
                false => Vec::new(),
            },
            Expr::Reference(name) => self.ends_of_rule(name, start),
            Expr::Special(_) => Vec::new(),
            Expr::Sequence(items) => {
                let mut current = vec![start];
                for item in items {
                    let mut next = Vec::new();
                    for &position in &current {
                        next.extend(self.ends(item, position));
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
                    all.extend(self.ends(alternative, start));
                }
                normalize(&mut all);
                all
            }
            Expr::Exception(included, excluded) => {
                if self.ends(excluded, start).iter().any(|&end| end > start) {
                    Vec::new()
                } else {
                    self.ends(included, start)
                }
            }
            Expr::Optional(inner) => {
                let mut all = vec![start];
                all.extend(self.ends(inner, start));
                normalize(&mut all);
                all
            }
            Expr::Repetition(inner) => {
                let mut reached = BTreeSet::from([start]);
                let mut frontier = vec![start];
                while let Some(position) = frontier.pop() {
                    for end in self.ends(inner, position) {
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

impl Grammar {
    /// The compiled grammar built on first use, or the errors that stop it compiling.
    pub fn compile_cached() -> Result<&'static Grammar, &'static [Error]> {
        static GRAMMAR: OnceLock<Result<Grammar, Vec<Error>>> = OnceLock::new();
        match GRAMMAR.get_or_init(Grammar::compile) {
            Ok(grammar) => Ok(grammar),
            Err(errors) => Err(errors),
        }
    }
}

/// The compiled grammar, built on first use. The rule set is fixed at compile time, so a
/// failure here is a defect in `def/grammar` and is reported by panicking.
pub fn grammar() -> &'static Grammar {
    Grammar::compile_cached().unwrap_or_else(|errors| {
        let errors: Vec<String> = errors.iter().map(ToString::to_string).collect();
        panic!("the grammar does not compile:\n{}", errors.join("\n"))
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
            parse("? any UTF-8 scalar value ?").unwrap(),
            Expr::Special("any UTF-8 scalar value".to_owned())
        );
        assert_eq!(parse(r#"'"'"#).unwrap(), terminal("\""));
        assert_eq!(parse(r#""\""#).unwrap(), terminal("\\"));
        assert_eq!(parse("\"\\\n\"").unwrap(), terminal("\\\n"));
        assert_eq!(
            parse("[[A]] , (: [[B]] :) ").unwrap(),
            Expr::Sequence(vec![
                reference("A"),
                Expr::Repetition(Box::new(reference("B")))
            ])
        );
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

    #[test]
    fn the_real_grammar_compiles() {
        let grammar = grammar();
        assert!(grammar.expression("NumberLiteral").is_some());
        assert_eq!(
            grammar.builtin("IdentifierStart"),
            Some(Builtin::IdentifierStart)
        );
        assert_eq!(grammar.builtin("Character"), Some(Builtin::Character));
        assert!(grammar.expression("Digit").is_some());
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
        assert_eq!(grammar.longest_match("LineCommentBody", ""), None);
        assert_eq!(grammar.longest_match("LineCommentBody", "\nx"), None);
        assert_eq!(grammar.longest_match("LineCommentBody", "ab\nx"), Some(2));
    }

    #[test]
    fn repetition_backtracks_so_a_trailing_underscore_is_not_part_of_a_number() {
        let grammar = grammar();
        assert_eq!(grammar.longest_match("NumberLiteral", "1_000.5x"), Some(7));
        assert_eq!(grammar.longest_match("NumberLiteral", "1_"), Some(1));
        assert!(grammar.matches("NumberLiteral", "1_0.0_1"));
        assert!(!grammar.matches("NumberLiteral", "1_"));
    }

    #[test]
    fn an_exception_excludes_positions_where_the_excluded_text_starts() {
        let grammar = grammar();
        assert_eq!(grammar.longest_match("BlockCommentBody", "ab*/c"), Some(2));
        assert_eq!(grammar.longest_match("BlockCommentBody", "a/*b"), Some(1));
        assert_eq!(grammar.longest_match("BlockCommentBody", "*/"), None);
        assert_eq!(grammar.longest_match("SingleQuoteBody", "a\r\nb"), Some(1));
        assert_eq!(grammar.longest_match("SingleQuoteBody", "a'b"), Some(1));
    }

    #[test]
    fn builtins_match_one_character() {
        let grammar = grammar();
        assert_eq!(grammar.longest_match("IdentifierStart", "é1"), Some(2));
        assert_eq!(grammar.longest_match("IdentifierStart", "1"), None);
        assert_eq!(grammar.longest_match("IdentifierContinue", "1"), Some(1));
        assert_eq!(grammar.longest_match("Character", "日本"), Some(3));
        assert_eq!(grammar.longest_match("Character", ""), None);
        assert_eq!(grammar.longest_match("Identifier", "_x1-"), Some(3));
        assert_eq!(grammar.longest_match("Identifier", "1x"), None);
    }

    #[test]
    fn left_recursive_rules_terminate() {
        let grammar = grammar();
        assert!(grammar.matches("Documentation", "/// a [[b]] d"));
        assert!(!grammar.matches("Documentation", "/// a ]] d"));
        assert_eq!(grammar.longest_match("Expression", "x"), Some(1));
        assert_eq!(grammar.longest_match("Statement", "break;"), Some(6));
    }

    // @lfy def/grammar/main.lfy:37
    #[test]
    fn nullable_rules_are_those_that_can_match_nothing() {
        let grammar = grammar();
        assert!(grammar.is_nullable("Items"));
        assert!(!grammar.is_nullable("Expression"));
        assert!(!grammar.is_nullable("Member"));
        assert!(grammar.is_nullable("SourceFile"));
        // A body terminal matches no characters only by creating no token.
        assert!(!grammar.is_nullable("TemplateBody"));
        assert!(!grammar.is_nullable("LineCommentBody"));
        assert!(!grammar.is_nullable("Identifier"));
        assert!(!grammar.is_nullable("Character"));
        assert!(grammar.is_expr_nullable(&parse("(/ [[Comma]] /)").unwrap()));
        assert!(grammar.is_expr_nullable(&parse("(: [[Comma]] :) , [[Items]]").unwrap()));
        assert!(!grammar.is_expr_nullable(&parse("[[Items]] , [[Comma]]").unwrap()));
        assert!(grammar.is_expr_nullable(&parse("[[Comma]] | [[Items]]").unwrap()));
        let mut count = 0;
        parse("[[A]] , ( B | (/ C /) )").unwrap().for_each(&mut |_| count += 1);
        assert_eq!(count, 6);
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
