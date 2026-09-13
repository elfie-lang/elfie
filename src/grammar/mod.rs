//! Compiled from `def/grammar`.
//!
//! Every `d` declaration becomes a variant of the enum compiled from its file, and every
//! such enum implements [`EbnfSyntax`] (the `ebnfSyntax` trait): the rule's EBNF
//! identifier, syntax and full rule text, plus the other traits applied to it. [`ebnf`]
//! holds the EBNF parser and matcher behind that trait; [`rules`] and [`lookup`] expose
//! the complete rule set.

pub mod ebnf;
pub mod expression;
pub mod file;
pub mod statement;
pub mod sugar;
pub mod tokens;
pub mod traits;
pub mod types;

pub use tokens::traits::{EbnfSyntax, Forbidden, RuleTrait};
pub use types::EBNFSyntax;

/// Everything known about one grammar rule, independent of the enum it was declared in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuleInfo {
    /// `$ebnf`
    pub ebnf: EBNFSyntax,
    /// Traits applied alongside `ebnfSyntax`.
    pub traits: &'static [RuleTrait],
}

impl RuleInfo {
    pub fn of<R: EbnfSyntax>(rule: R) -> Self {
        Self {
            ebnf: rule.ebnf(),
            traits: rule.traits(),
        }
    }

    pub fn identifier(&self) -> &'static str {
        self.ebnf.identifier
    }
}

/// Declares the rules of one grammar file as an enum implementing [`EbnfSyntax`].
///
/// ```text
/// /// definition
/// Name is [traits] = "syntax",
/// ```
/// mirrors `d Name is ebnfSyntax(`syntax`), traits: `definition` {}`.
macro_rules! grammar_rules {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident $( is [ $($rule_trait:expr),* $(,)? ] )? = $syntax:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $( $(#[$variant_meta])* $variant ),+
        }

        impl $name {
            /// Every rule of this file, in declaration order.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];
        }

        impl $crate::grammar::EbnfSyntax for $name {
            fn identifier(self) -> &'static str {
                match self { $( $name::$variant => stringify!($variant) ),+ }
            }

            fn syntax(self) -> &'static str {
                match self { $( $name::$variant => $syntax ),+ }
            }

            fn rule(self) -> &'static str {
                match self {
                    $( $name::$variant => concat!(stringify!($variant), " = ", $syntax, " ;") ),+
                }
            }

            fn traits(self) -> &'static [$crate::grammar::RuleTrait] {
                match self {
                    $( $name::$variant => {
                        const TRAITS: &[$crate::grammar::RuleTrait] = &[ $( $( $rule_trait ),* )? ];
                        TRAITS
                    } ),+
                }
            }
        }
    };
}

pub(crate) use grammar_rules;

/// Every rule of the grammar, in declaration order: the token files in the order
/// `def/lexer/main.lfy` loads them, then expressions, statements, sugar and the file rule.
pub fn rules() -> impl Iterator<Item = RuleInfo> {
    fn all<R: EbnfSyntax>(all: &'static [R]) -> impl Iterator<Item = RuleInfo> {
        all.iter().map(|rule| RuleInfo::of(*rule))
    }
    all(tokens::comment::Comment::ALL)
        .chain(all(tokens::identifier::Identifier::ALL))
        .chain(all(tokens::keyword::Keyword::ALL))
        .chain(all(tokens::literal::Literal::ALL))
        .chain(all(tokens::operator::Operator::ALL))
        .chain(all(tokens::separator::Separator::ALL))
        .chain(all(tokens::space::Space::ALL))
        .chain(all(expression::Expression::ALL))
        .chain(all(statement::Statement::ALL))
        .chain(all(sugar::Sugar::ALL))
        .chain(all(file::File::ALL))
}

/// The rule with the given EBNF identifier.
pub fn lookup(identifier: &str) -> Option<RuleInfo> {
    rules().find(|rule| rule.identifier() == identifier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn rule_identifiers_are_unique() {
        let mut seen = HashSet::new();
        for rule in rules() {
            assert!(
                seen.insert(rule.identifier()),
                "{} declared twice",
                rule.identifier()
            );
        }
        assert!(seen.len() > 200);
    }

    // @lfy def/grammar/tokens/traits.lfy:8
    #[test]
    fn rule_text_is_identifier_equals_syntax() {
        for rule in rules() {
            assert_eq!(
                rule.ebnf.rule,
                format!("{} = {} ;", rule.ebnf.identifier, rule.ebnf.syntax)
            );
        }
        assert_eq!(
            lookup("BlockOpenSeparator").unwrap().ebnf.rule,
            r#"BlockOpenSeparator = "{" ;"#
        );
    }

    // @lfy def/grammar/tokens/traits.lfy:11
    #[test]
    fn every_rule_parses_as_ebnf_and_resolves_its_references() {
        let grammar = ebnf::Grammar::compile().unwrap_or_else(|errors| {
            panic!(
                "{}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
        for rule in rules() {
            assert!(
                grammar.expression(rule.identifier()).is_some()
                    || grammar.builtin(rule.identifier()).is_some(),
                "{} was not compiled",
                rule.identifier()
            );
        }
    }

    #[test]
    fn lookup_finds_rules_by_identifier() {
        assert_eq!(
            lookup("SourceFile").unwrap().ebnf.syntax,
            "(: [[Statement]] :)"
        );
        assert_eq!(lookup("AgentDataKeyword").unwrap().ebnf.syntax, r#""d""#);
        assert!(lookup("AgentDataKeyword").unwrap().traits.is_empty());
        assert!(lookup("NoSuchRule").is_none());
    }
}
