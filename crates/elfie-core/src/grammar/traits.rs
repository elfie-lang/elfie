//! Compiled from `def/grammar/traits.lfy`.
//!
//! `trait rule` is the [`GrammarRule`] trait every rule enum implements. The traits that
//! extend it (`terminal` and its kinds, `alternationList`, `statement`, `primary`, `prefix`,
//! `infix`, `postfix`) are the [`Category`] of a rule, and `binding` is its optional
//! [`Binding`]. The global checks of the grammar live in [`super::checks`]. The `ace` helper functions are compiled both into the syntax literals of
//! every rule (evaluated at compile time) and into the functions here that recompute them,
//! which the tests use to check the two agree.

use std::borrow::Cow;

use super::Entity;
use super::ebnf::{self, Expr};
use super::precedence::Level;

// Rule definitions

/// EBNF form of one grammar rule.
// @lfy def/grammar/traits.lfy:4
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rule {
    /// Name of the rule.
    pub identifier: &'static str, // @lfy def/grammar/traits.lfy:5
    /// Right hand side of the rule in ISO EBNF.
    pub syntax: &'static str, // @lfy def/grammar/traits.lfy:6
    /// The whole rule line.
    pub text: &'static str, // @lfy def/grammar/traits.lfy:7
}

/// `trait rule(syntax: string)`: a grammar rule.
///
/// Optional groups, repetitions, and alternations inside the syntax describe what is
/// valid, not how to choose.
// @lfy def/grammar/traits.lfy:10
pub trait GrammarRule: Copy + Into<Entity> + 'static {
    /// `@identifier`: the name of the rule.
    fn identifier(self) -> &'static str;

    /// The `syntax` argument: the right hand side of the rule in ISO EBNF.
    fn syntax(self) -> &'static str;

    /// `{{@identifier}} = {{syntax}} ;`
    // @lfy def/grammar/traits.lfy:15
    fn text(self) -> &'static str;

    /// `@definition`: the description the rule was declared with; empty when it has none.
    fn definition(self) -> &'static str;

    /// The traits applied to the rule besides `rule` and `binding`.
    fn category(self) -> Category;

    /// The `binding` trait of the rule, when it has one.
    // @lfy def/grammar/traits.lfy:70
    fn binding(self) -> Option<Binding>;

    /// `$rule`: EBNF form of this rule.
    // @lfy def/grammar/traits.lfy:11
    fn rule(self) -> Rule {
        Rule {
            identifier: self.identifier(), // @lfy def/grammar/traits.lfy:13
            syntax: self.syntax(),         // @lfy def/grammar/traits.lfy:14
            text: self.text(),             // @lfy def/grammar/traits.lfy:15
        }
    }

    /// The rule as an [`Entity`].
    fn entity(self) -> Entity {
        self.into()
    }

    /// The compiled EBNF expression of this rule; `None` for the external rules provided
    /// by a built-in matcher.
    fn expression(self) -> Option<&'static Expr> {
        ebnf::grammar().expression(self.identifier())
    }

    /// Byte length of the longest non-empty match of this rule at `source[start..]`.
    // @lfy def/grammar/traits.lfy:19
    fn longest_match_at(self, source: &str, start: usize) -> Option<usize> {
        ebnf::grammar().longest_match_at(self.identifier(), source, start)
    }

    /// Byte length of the longest non-empty prefix of `input` that this rule matches.
    // @lfy def/grammar/traits.lfy:19
    fn longest_match(self, input: &str) -> Option<usize> {
        self.longest_match_at(input, 0)
    }

    /// Whether the whole of `input` is satisfied by this rule.
    // @lfy def/grammar/traits.lfy:19
    fn matches(self, input: &str) -> bool {
        ebnf::grammar().matches(self.identifier(), input)
    }

    /// The binding this rule groups with: its own `binding`, or for an operator rule the
    /// binding of the operator that satisfied it.
    // @lfy def/grammar/traits.lfy:124
    fn effective_binding(self) -> Option<Binding> {
        self.binding()
            .or_else(|| self.category().operator_binding())
    }

    /// `is terminal`
    fn is_terminal(self) -> bool {
        matches!(self.category(), Category::Terminal(_))
    }

    /// `is keyword` (which includes `reserved`)
    fn is_keyword(self) -> bool {
        matches!(
            self.category(),
            Category::Terminal(Terminal::Keyword { .. })
        )
    }

    /// `is escape`
    fn is_escape(self) -> bool {
        matches!(self.category(), Category::Terminal(Terminal::Escape { .. }))
    }

    /// `is body`
    fn is_body(self) -> bool {
        matches!(self.category(), Category::Terminal(Terminal::Body { .. }))
    }

    /// `is punctuation` (which includes setters and the comparison and arithmetic kinds)
    fn is_punctuation(self) -> bool {
        matches!(
            self.category(),
            Category::Terminal(Terminal::Punctuation { .. })
        )
    }

    /// `is alternationList`
    fn is_alternation_list(self) -> bool {
        matches!(self.category(), Category::AlternationList(_))
    }

    /// `is statement`
    fn is_statement(self) -> bool {
        matches!(self.category(), Category::Statement)
    }

    /// `is primary`
    fn is_primary(self) -> bool {
        matches!(self.category(), Category::Primary)
    }

    /// `is prefix`
    fn is_prefix(self) -> bool {
        matches!(self.category(), Category::Prefix { .. })
    }

    /// `is infix`
    fn is_infix(self) -> bool {
        matches!(self.category(), Category::Infix { .. })
    }

    /// `is postfix`
    fn is_postfix(self) -> bool {
        matches!(self.category(), Category::Postfix { .. })
    }
}

// Helper functions

/// `ace function alternation(...items)`: `[[A]] | [[B]] | …`
// @lfy def/grammar/traits.lfy:26
pub fn alternation(items: &[Entity]) -> String {
    items
        .iter()
        .map(|item| format!("[[{}]]", item.identifier()))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// `ace function quoted(text)`: the text as an EBNF terminal, in single quotes when it
/// contains a double quote and in double quotes otherwise.
// @lfy def/grammar/traits.lfy:30
pub fn quoted(text: &str) -> String {
    if text.contains('"') {
        format!("'{text}'")
    } else {
        format!("\"{text}\"")
    }
}

/// `ace function bodySyntax(excluded, escapes)`: any character except the excluded ones,
/// plus the listed escapes, repeated.
// @lfy def/grammar/traits.lfy:34
pub fn body_syntax(excluded: &[Entity], escapes: &[Entity]) -> String {
    let plain = format!("( Character - ( {} ) )", alternation(excluded)); // @lfy def/grammar/traits.lfy:35
    let escapes = alternation(escapes);
    if escapes.is_empty() {
        format!("(: {plain} :)") // @lfy def/grammar/traits.lfy:36
    } else {
        format!("(: {plain} | {escapes} :)") // @lfy def/grammar/traits.lfy:36
    }
}

// Terminals

/// `trait terminal(syntax)` and the traits that extend it: a rule matched against
/// characters and delivered as a single token, which records the rule.
// @lfy def/grammar/traits.lfy:40
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Terminal {
    /// `terminal(syntax)` with no further trait.
    Plain,
    /// `keyword(text)`: a word with fixed meaning in the grammar, which cannot be an
    /// identifier; `reserved` marks a keyword no rule uses yet.
    Keyword {
        text: &'static str,
        reserved: bool, // @lfy def/grammar/traits.lfy:48
    }, // @lfy def/grammar/traits.lfy:44
    /// `punctuation(text)`: punctuation with specific meaning; `class` is the trait that
    /// extends `punctuation`, which also gives the rule its binding.
    Punctuation {
        text: &'static str,
        class: Option<PunctuationClass>,
    }, // @lfy def/grammar/traits.lfy:50
    /// `boundary(text)`: a symbol that opens or closes a region whose contents are text
    /// rather than code.
    Boundary { text: &'static str }, // @lfy def/grammar/traits.lfy:57
    /// `escape(syntax, becomes)`: a sequence inside a text body that stands for other
    /// characters. Matched inside a body that lists this escape, its characters are
    /// replaced by `becomes` in the token value.
    Escape { becomes: Becomes }, // @lfy def/grammar/traits.lfy:59
    /// `body(excluded, escapes)`: text between boundaries: any character except the
    /// excluded ones, plus the listed escapes. The token value is the matched text with
    /// each listed escape replaced, and no token is created when it matches nothing.
    Body {
        excluded: &'static [Entity],
        escapes: &'static [Entity],
    }, // @lfy def/grammar/traits.lfy:63
}

impl Terminal {
    /// The fixed text of a keyword, punctuation or boundary terminal.
    pub const fn fixed_text(self) -> Option<&'static str> {
        match self {
            Terminal::Keyword { text, .. }
            | Terminal::Punctuation { text, .. }
            | Terminal::Boundary { text } => Some(text),
            Terminal::Plain | Terminal::Escape { .. } | Terminal::Body { .. } => None,
        }
    }
}

/// The traits extending `punctuation`, each of which carries a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PunctuationClass {
    /// Assigns, possibly after combining with the current value.
    Setter, // @lfy def/grammar/traits.lfy:51
    /// Orders two values.
    Relational, // @lfy def/grammar/traits.lfy:52
    /// Compares two values.
    Equality, // @lfy def/grammar/traits.lfy:53
    /// Adds or subtracts.
    Additive, // @lfy def/grammar/traits.lfy:54
    /// Multiplies or divides.
    Multiplicative, // @lfy def/grammar/traits.lfy:55
}

impl PunctuationClass {
    /// The binding the class gives its rules.
    pub const fn binding(self) -> Binding {
        match self {
            PunctuationClass::Setter => Binding::right(Level::Assignment), // @lfy def/grammar/traits.lfy:51
            PunctuationClass::Relational => Binding::left(Level::Relational), // @lfy def/grammar/traits.lfy:52
            PunctuationClass::Equality => Binding::left(Level::Equality), // @lfy def/grammar/traits.lfy:53
            PunctuationClass::Additive => Binding::left(Level::Additive), // @lfy def/grammar/traits.lfy:54
            PunctuationClass::Multiplicative => Binding::left(Level::Multiplicative), // @lfy def/grammar/traits.lfy:55
        }
    }
}

/// The `becomes` argument of an escape: what its characters are replaced by.
// @lfy def/grammar/traits.lfy:59
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Becomes {
    /// Fixed replacement text.
    Text(&'static str),
    /// "the character with that code": the character whose code the hex digits of the
    /// escape spell.
    CharacterWithCode, // @lfy def/grammar/terminals/literal.lfy:36
    /// "the character with that code point": the character whose code point the hex
    /// digits of the escape spell. When the digits are not a scalar value the text is
    /// kept as written.
    CharacterWithCodePoint, // @lfy def/grammar/terminals/literal.lfy:37
}

impl Becomes {
    /// The replacement for the matched text of the escape, or `None` when the text is
    /// kept as written.
    // @lfy def/grammar/traits.lfy:60
    pub fn apply(self, matched: &str) -> Option<Cow<'static, str>> {
        match self {
            Becomes::Text(text) => Some(Cow::Borrowed(text)),
            Becomes::CharacterWithCode | Becomes::CharacterWithCodePoint => {
                let digits = matched.trim_start_matches(|c: char| !c.is_ascii_hexdigit());
                let code = u32::from_str_radix(digits, 16).ok()?;
                // @lfy def/grammar/terminals/literal.lfy:41
                char::from_u32(code).map(|c| Cow::Owned(c.to_string()))
            }
        }
    }
}

/// The value of a token matched by a `body` terminal: the matched text with each escape
/// the body lists replaced. Text matched by any other rule is its own value.
// @lfy def/grammar/traits.lfy:65
pub fn body_value(body: Entity, raw: &str) -> String {
    let Category::Terminal(Terminal::Body { escapes, .. }) = body.category() else {
        return raw.to_owned();
    };
    if escapes.is_empty() {
        return raw.to_owned();
    }
    let mut value = String::with_capacity(raw.len());
    let mut position = 0;
    while position < raw.len() {
        let rest = &raw[position..];
        // Every escape begins with a backslash (see the tests), so only a backslash can
        // start one.
        let escape = rest.starts_with('\\').then(|| {
            escapes.iter().find_map(|escape| {
                let len = escape.longest_match_at(raw, position)?;
                let matched = &raw[position..position + len];
                let Category::Terminal(Terminal::Escape { becomes }) = escape.category() else {
                    return None;
                };
                Some((len, becomes.apply(matched)))
            })
        });
        match escape.flatten() {
            // @lfy def/grammar/traits.lfy:60
            Some((len, replacement)) => {
                match replacement {
                    Some(text) => value.push_str(&text),
                    None => value.push_str(&raw[position..position + len]),
                }
                position += len;
            }
            None => {
                let c = rest.chars().next().expect("position is inside the text");
                value.push(c);
                position += c.len_utf8();
            }
        }
    }
    value
}

// Binding power

/// How two operators of the same precedence group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Associativity {
    /// `a b c` reads as `(a b) c`.
    Left, // @lfy def/grammar/traits.lfy:77
    /// `a b c` reads as `a (b c)`.
    Right, // @lfy def/grammar/traits.lfy:78
}

/// `trait binding(precedence, associativity)`: how tightly an operator, or a rule that
/// overrides its operator, groups with its neighbors. An associativity of `None` (the
/// source's `null`) means `a b c` is not valid.
// @lfy def/grammar/traits.lfy:70
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binding {
    /// `$precedence`: binding power; a higher value binds tighter.
    pub precedence: Level, // @lfy def/grammar/traits.lfy:71
    /// `$associativity`: how two of the same precedence group: left, right, or not at all.
    pub associativity: Option<Associativity>, // @lfy def/grammar/traits.lfy:73
}

impl Binding {
    /// `binding(precedence)`: the default associativity is left.
    pub const fn left(precedence: Level) -> Self {
        Binding {
            precedence,
            associativity: Some(Associativity::Left),
        }
    }

    /// `binding(precedence, 'right')`
    pub const fn right(precedence: Level) -> Self {
        Binding {
            precedence,
            associativity: Some(Associativity::Right),
        }
    }

    /// `binding(precedence, null)`: `a b c` is not valid.
    // @lfy def/grammar/traits.lfy:79
    pub const fn non_associative(precedence: Level) -> Self {
        Binding {
            precedence,
            associativity: None,
        }
    }

    /// The numeric binding power.
    pub const fn precedence_value(self) -> u8 {
        self.precedence.value()
    }
}

// Categories/groupings

/// The trait a rule was declared with besides `rule` itself and `binding`. A rule has
/// exactly one category, which is what "no rule has more than one of statement, primary,
/// prefix, infix, and postfix" requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// `rule(syntax)` with no further trait.
    Rule,
    /// `terminal(syntax)` or one of the traits extending it.
    Terminal(Terminal), // @lfy def/grammar/traits.lfy:40
    /// `alternationList(...items)`: a set of rules, any one of which satisfies this rule.
    /// The order of the items carries no meaning; a match for an item is treated as a
    /// match for that item instead of this rule, which is never included in tokens or a
    /// tree.
    AlternationList(&'static [Entity]), // @lfy def/grammar/traits.lfy:84
    /// `statement(syntax)`: a rule that is or can be a top level statement.
    Statement, // @lfy def/grammar/traits.lfy:96
    /// `primary(syntax)`: an expression rule that begins with its own token.
    Primary, // @lfy def/grammar/traits.lfy:98
    /// `prefix(operator, disallowSpace)`: an operator followed by the expression it
    /// applies to. With `disallow_space`, the rule cannot be a match when space, a line
    /// break, comments, or documentation exist between the operator and the expression.
    Prefix {
        operator: Entity,
        disallow_space: bool, // @lfy def/grammar/traits.lfy:108
    }, // @lfy def/grammar/traits.lfy:100
    /// `infix(operator)`: two expressions joined by an operator.
    Infix { operator: Entity }, // @lfy def/grammar/traits.lfy:112
    /// `postfix(operator, tail)`: an expression followed by an operator and, when `tail`
    /// is given, more syntax. It binds as the token that satisfied the operator does
    /// unless the rule itself has a binding; expressions inside the tail that do not
    /// follow `GroupOpen` or `ListOpen` are parsed with the rule's binding power as the
    /// minimum.
    Postfix {
        operator: Entity,
        tail: Option<&'static str>,
        /// With `disallow_space`, the rule cannot be a match when space, a line break,
        /// comments, or documentation exist between the operator and the tail.
        disallow_space: bool, // @lfy def/grammar/traits.lfy:133
    }, // @lfy def/grammar/traits.lfy:119
}

impl Category {
    /// The syntax the category's trait derives from its arguments; `None` for the
    /// categories whose syntax is given directly.
    pub fn derived_syntax(self) -> Option<String> {
        Some(match self {
            Category::Terminal(terminal) => match terminal {
                // @lfy def/grammar/traits.lfy:44
                Terminal::Keyword { text, .. }
                | Terminal::Punctuation { text, .. }
                | Terminal::Boundary { text } => quoted(text),
                // @lfy def/grammar/traits.lfy:63
                Terminal::Body { excluded, escapes } => body_syntax(excluded, escapes),
                Terminal::Plain | Terminal::Escape { .. } => return None,
            },
            // @lfy def/grammar/traits.lfy:84
            Category::AlternationList(items) => alternation(items),
            // @lfy def/grammar/traits.lfy:100
            Category::Prefix { operator, .. } => {
                format!("[[{}]] , [[Expression]]", operator.identifier())
            }
            // @lfy def/grammar/traits.lfy:112
            Category::Infix { operator } => {
                format!(
                    "[[Expression]] , [[{}]] , [[Expression]]",
                    operator.identifier()
                )
            }
            // @lfy def/grammar/traits.lfy:119
            Category::Postfix { operator, tail, .. } => match tail {
                None => format!("[[Expression]] , [[{}]]", operator.identifier()),
                Some(tail) => format!("[[Expression]] , [[{}]] , {tail}", operator.identifier()),
            },
            Category::Rule | Category::Statement | Category::Primary => return None,
        })
    }

    /// The operator of a prefix, infix or postfix rule.
    pub const fn operator(self) -> Option<Entity> {
        match self {
            Category::Prefix { operator, .. }
            | Category::Infix { operator }
            | Category::Postfix { operator, .. } => Some(operator),
            _ => None,
        }
    }

    /// The binding the operator of a prefix, infix or postfix rule carries: the
    /// operator's own binding, or when the operator is an alternation list whose every
    /// alternative binds with one shared precedence, that binding (the associativity of
    /// the first alternative). `None` for other categories and for operators without
    /// such a binding.
    // @lfy def/grammar/traits.lfy:103
    pub fn operator_binding(self) -> Option<Binding> {
        shared_binding(self.operator()?)
    }
}

/// The binding of a rule, looking through alternation lists: every alternative must
/// bind with the same precedence.
// @lfy def/grammar/main.lfy:28
fn shared_binding(rule: Entity) -> Option<Binding> {
    if let Some(binding) = rule.binding() {
        return Some(binding);
    }
    let Category::AlternationList(items) = rule.category() else {
        return None;
    };
    let mut shared: Option<Binding> = None;
    for &item in items {
        let binding = shared_binding(item)?;
        match shared {
            None => shared = Some(binding),
            Some(first) if first.precedence == binding.precedence => {}
            Some(_) => return None,
        }
    }
    shared
}

/// The constructors for [`Category`], named after the traits they compile.
pub mod category {
    use super::{Becomes, Binding, Category, Entity, PunctuationClass, Terminal};

    /// `rule(syntax)`
    pub const fn rule() -> Category {
        Category::Rule
    }

    /// `terminal(syntax)`
    // @lfy def/grammar/traits.lfy:40
    pub const fn terminal() -> Category {
        Category::Terminal(Terminal::Plain)
    }

    /// `keyword(text)`
    // @lfy def/grammar/traits.lfy:44
    pub const fn keyword(text: &'static str) -> Category {
        Category::Terminal(Terminal::Keyword {
            text,
            reserved: false,
        })
    }

    /// `reserved(text)`
    // @lfy def/grammar/traits.lfy:48
    pub const fn reserved(text: &'static str) -> Category {
        Category::Terminal(Terminal::Keyword {
            text,
            reserved: true,
        })
    }

    /// `punctuation(text)`
    // @lfy def/grammar/traits.lfy:50
    pub const fn punctuation(text: &'static str) -> Category {
        Category::Terminal(Terminal::Punctuation { text, class: None })
    }

    const fn classed(text: &'static str, class: PunctuationClass) -> Category {
        Category::Terminal(Terminal::Punctuation {
            text,
            class: Some(class),
        })
    }

    /// `setter(text)`
    // @lfy def/grammar/traits.lfy:51
    pub const fn setter(text: &'static str) -> Category {
        classed(text, PunctuationClass::Setter)
    }

    /// `relational(text)`
    // @lfy def/grammar/traits.lfy:52
    pub const fn relational(text: &'static str) -> Category {
        classed(text, PunctuationClass::Relational)
    }

    /// `equality(text)`
    // @lfy def/grammar/traits.lfy:53
    pub const fn equality(text: &'static str) -> Category {
        classed(text, PunctuationClass::Equality)
    }

    /// `additive(text)`
    // @lfy def/grammar/traits.lfy:54
    pub const fn additive(text: &'static str) -> Category {
        classed(text, PunctuationClass::Additive)
    }

    /// `multiplicative(text)`
    // @lfy def/grammar/traits.lfy:55
    pub const fn multiplicative(text: &'static str) -> Category {
        classed(text, PunctuationClass::Multiplicative)
    }

    /// The binding a `setter`, `relational`, `equality`, `additive` or `multiplicative`
    /// rule carries.
    pub const fn class_binding(class: PunctuationClass) -> Binding {
        class.binding()
    }

    /// `boundary(text)`
    // @lfy def/grammar/traits.lfy:57
    pub const fn boundary(text: &'static str) -> Category {
        Category::Terminal(Terminal::Boundary { text })
    }

    /// `escape(syntax, becomes)`
    // @lfy def/grammar/traits.lfy:59
    pub const fn escape(becomes: Becomes) -> Category {
        Category::Terminal(Terminal::Escape { becomes })
    }

    /// `body(excluded, escapes)`
    // @lfy def/grammar/traits.lfy:63
    pub const fn body(excluded: &'static [Entity], escapes: &'static [Entity]) -> Category {
        Category::Terminal(Terminal::Body { excluded, escapes })
    }

    /// `alternationList(...items)`
    // @lfy def/grammar/traits.lfy:84
    pub const fn alternation_list(items: &'static [Entity]) -> Category {
        Category::AlternationList(items)
    }

    /// `statement(syntax)`
    // @lfy def/grammar/traits.lfy:96
    pub const fn statement() -> Category {
        Category::Statement
    }

    /// `primary(syntax)`
    // @lfy def/grammar/traits.lfy:98
    pub const fn primary() -> Category {
        Category::Primary
    }

    /// `prefix(operator, disallowSpace)`
    // @lfy def/grammar/traits.lfy:100
    pub const fn prefix(operator: Entity, disallow_space: bool) -> Category {
        Category::Prefix {
            operator,
            disallow_space,
        }
    }

    /// `infix(operator)`
    // @lfy def/grammar/traits.lfy:112
    pub const fn infix(operator: Entity) -> Category {
        Category::Infix { operator }
    }

    /// `postfix(operator, tail)`
    // @lfy def/grammar/traits.lfy:119
    pub const fn postfix(operator: Entity, tail: Option<&'static str>, disallow_space: bool) -> Category {
        Category::Postfix {
            operator,
            tail,
            disallow_space,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::rules;
    use crate::grammar::terminals::keyword::Keyword;
    use crate::grammar::terminals::literal::Literal;
    use crate::grammar::terminals::punctuation::Punctuation;

    // @lfy def/grammar/traits.lfy:26
    #[test]
    fn alternation_joins_references_with_bars() {
        assert_eq!(
            alternation(&[
                Entity::Punctuation(Punctuation::Plus),
                Entity::Punctuation(Punctuation::Minus)
            ]),
            "[[Plus]] | [[Minus]]"
        );
        assert_eq!(alternation(&[]), "");
    }

    // @lfy def/grammar/traits.lfy:30
    #[test]
    fn quoted_picks_the_quote_the_text_does_not_contain() {
        assert_eq!(quoted("d"), "\"d\"");
        assert_eq!(quoted("\""), "'\"'");
        assert_eq!(quoted("'"), "\"'\"");
    }

    // @lfy def/grammar/traits.lfy:34
    #[test]
    fn body_syntax_lists_escapes_only_when_there_are_some() {
        let excluded = [Entity::Space(
            crate::grammar::terminals::space::Space::NewLine,
        )];
        assert_eq!(
            body_syntax(&excluded, &[]),
            "(: ( Character - ( [[NewLine]] ) ) :)"
        );
        assert_eq!(
            body_syntax(&excluded, &[Entity::Literal(Literal::TabEscape)]),
            "(: ( Character - ( [[NewLine]] ) ) | [[TabEscape]] :)"
        );
    }

    // @lfy def/grammar/traits.lfy:11
    #[test]
    fn rule_is_identifier_syntax_and_text() {
        let rule = Keyword::ConstKeyword.rule();
        assert_eq!(rule.identifier, "ConstKeyword");
        assert_eq!(rule.syntax, "\"const\"");
        assert_eq!(rule.text, "ConstKeyword = \"const\" ;");
        for rule in rules() {
            assert_eq!(
                rule.text(),
                format!("{} = {} ;", rule.identifier(), rule.syntax())
            );
        }
    }

    // @lfy def/grammar/traits.lfy:44
    #[test]
    fn every_derived_syntax_matches_the_compiled_syntax() {
        for rule in rules() {
            if let Some(derived) = rule.category().derived_syntax() {
                assert_eq!(rule.syntax(), derived, "{}", rule.identifier());
            }
        }
    }

    // @lfy def/grammar/traits.lfy:51
    #[test]
    fn punctuation_classes_carry_their_bindings() {
        assert_eq!(
            PunctuationClass::Setter.binding(),
            Binding::right(Level::Assignment)
        );
        assert_eq!(
            PunctuationClass::Multiplicative.binding(),
            Binding::left(Level::Multiplicative)
        );
        for rule in rules() {
            if let Category::Terminal(Terminal::Punctuation {
                class: Some(class), ..
            }) = rule.category()
            {
                assert_eq!(
                    rule.binding(),
                    Some(class.binding()),
                    "{}",
                    rule.identifier()
                );
            }
        }
    }

    // @lfy def/grammar/traits.lfy:60
    #[test]
    fn escapes_become_their_replacement() {
        assert_eq!(Becomes::Text("\n").apply("\\n"), Some(Cow::Borrowed("\n")));
        assert_eq!(
            Becomes::CharacterWithCode.apply("\\x41"),
            Some(Cow::Owned("A".to_owned()))
        );
        assert_eq!(
            Becomes::CharacterWithCodePoint.apply("\\u01F600"),
            Some(Cow::Owned("😀".to_owned()))
        );
        // @lfy def/grammar/terminals/literal.lfy:41
        assert_eq!(Becomes::CharacterWithCodePoint.apply("\\uD800"), None);
        assert_eq!(Becomes::CharacterWithCodePoint.apply("\\u110000"), None);
    }

    // @lfy def/grammar/traits.lfy:65
    #[test]
    fn body_values_replace_the_listed_escapes_only() {
        let double = Entity::Literal(Literal::DoubleQuoteBody);
        assert_eq!(body_value(double, "a\\nb\\x41\\u0042\\\\"), "a\nbAB\\");
        assert_eq!(
            body_value(double, "kept \\uD800 as written"),
            "kept \\uD800 as written"
        );
        let single = Entity::Literal(Literal::SingleQuoteBody);
        assert_eq!(body_value(single, "it\\'s \\\\ x"), "it's \\ x");
        assert_eq!(body_value(single, "a\\\nb"), "ab");
        let template = Entity::Literal(Literal::TemplateBody);
        assert_eq!(
            body_value(template, "\\{{x\\}} \\[[y\\]] \\`"),
            "{{x}} [[y]] `"
        );
        // Bodies without escapes and other rules keep their text.
        assert_eq!(
            body_value(
                Entity::Space(crate::grammar::terminals::space::Space::NewLine),
                "\\n"
            ),
            "\\n"
        );
        assert_eq!(
            body_value(
                Entity::Comment(crate::grammar::terminals::comment::Comment::LineCommentBody),
                "\\n"
            ),
            "\\n"
        );
    }

    #[test]
    fn every_escape_begins_with_a_backslash() {
        for rule in rules().filter(|rule| rule.is_escape()) {
            let expr = rule
                .expression()
                .unwrap_or_else(|| panic!("{}", rule.identifier()));
            let first = match expr {
                Expr::Sequence(items) => items.first().cloned(),
                other => Some(other.clone()),
            };
            match first {
                Some(Expr::Terminal(text)) => {
                    assert!(text.starts_with('\\'), "{}", rule.identifier())
                }
                other => panic!("{}: {other:?}", rule.identifier()),
            }
        }
    }

    // @lfy def/grammar/traits.lfy:103
    #[test]
    fn operator_rules_bind_as_their_operators_do() {
        use crate::grammar::rules::expression::Expression;
        assert_eq!(
            Expression::NotOperation.binding(),
            Some(Binding::right(Level::Unary))
        );
        assert_eq!(Expression::AdditiveOperation.binding(), None);
        assert_eq!(
            Expression::AdditiveOperation.effective_binding(),
            Some(Binding::left(Level::Additive))
        );
        assert_eq!(
            Expression::Assignment.effective_binding(),
            Some(Binding::right(Level::Assignment))
        );
        assert_eq!(
            Expression::Member.effective_binding(),
            Some(Binding::left(Level::Access))
        );
        assert_eq!(
            Expression::Conditional.effective_binding(),
            Some(Binding::right(Level::Definition))
        );
        assert_eq!(
            Expression::AwaitOperation.binding(),
            Some(Binding::non_associative(Level::Wrapper))
        );
        assert_eq!(Keyword::AsKeyword.category().operator_binding(), None);
    }
}
