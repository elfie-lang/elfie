//! Compiled from `def/grammar/main.lfy`.
//!
//! Every `d` declaration becomes a variant of the enum compiled from its file, and every
//! such enum implements [`GrammarRule`] (the `rule` trait): identifier, syntax, rule text,
//! definition, [`Category`] and [`Binding`]. [`Entity`] wraps those enums so any rule can
//! be named, and [`rules`] iterates every rule of the grammar in the order the files load
//! from `def/grammar/main.lfy`: `use` statements are followed depth first and each file is
//! loaded once, which gives keywords, identifiers, space, literals, punctuation,
//! expressions, comments, statements and finally the file rule. [`ebnf`] holds the EBNF
//! parser and matcher behind the rules.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

pub mod checks; // @lfy def/grammar/main.lfy:21
pub mod ebnf;
pub mod precedence; // @lfy def/grammar/main.lfy:1
pub mod rules; // @lfy def/grammar/main.lfy:2
pub mod terminals; // @lfy def/grammar/main.lfy:5
pub mod traits; // @lfy def/grammar/main.lfy:2

pub use checks::{Check, Violation};
pub use precedence::Level;
pub use traits::{
    Associativity, Becomes, Binding, Category, GrammarRule, PunctuationClass, Rule, Terminal,
};

use rules::expression::Expression;
use rules::file::File;
use rules::statement::Statement;
use terminals::comment::Comment;
use terminals::identifier::Identifier;
use terminals::keyword::Keyword;
use terminals::literal::Literal;
use terminals::punctuation::Punctuation;
use terminals::space::Space;

/// Declares the rules of one grammar file as an enum implementing [`GrammarRule`].
///
/// ```text
/// /// acceptance criteria
/// Name is [category, binding]: "definition" = "syntax",
/// ```
/// mirrors `d Name is category(...), binding(...): `definition` { criteria }`. The binding
/// and the definition are optional.
macro_rules! grammar_rules {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident is [ $category:expr $(, $binding:expr)? ] $( : $definition:literal )? = $syntax:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $( $(#[$variant_meta])* $( #[doc = $definition] )? $variant ),+
        }

        impl $name {
            /// Every rule of this file, in declaration order.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];
        }

        impl From<$name> for $crate::grammar::Entity {
            fn from(rule: $name) -> Self {
                $crate::grammar::Entity::$name(rule)
            }
        }

        impl $crate::grammar::GrammarRule for $name {
            fn identifier(self) -> &'static str {
                match self { $( $name::$variant => stringify!($variant) ),+ }
            }

            fn syntax(self) -> &'static str {
                match self { $( $name::$variant => $syntax ),+ }
            }

            fn text(self) -> &'static str {
                match self {
                    $( $name::$variant => concat!(stringify!($variant), " = ", $syntax, " ;") ),+
                }
            }

            fn definition(self) -> &'static str {
                match self {
                    $( $name::$variant => $crate::grammar::literal_or_empty!($($definition)?) ),+
                }
            }

            fn category(self) -> $crate::grammar::Category {
                match self { $( $name::$variant => $category ),+ }
            }

            fn binding(self) -> Option<$crate::grammar::Binding> {
                match self { $( $name::$variant => $crate::grammar::some_or_none!($($binding)?) ),+ }
            }
        }
    };
}

macro_rules! literal_or_empty {
    () => {
        ""
    };
    ($literal:literal) => {
        $literal
    };
}

macro_rules! some_or_none {
    () => {
        None
    };
    ($value:expr) => {
        Some($value)
    };
}

pub(crate) use {grammar_rules, literal_or_empty, some_or_none};

/// Any rule of the grammar: the enum of the file it was declared in, wrapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Entity {
    Keyword(Keyword),         // @lfy def/grammar/main.lfy:7
    Identifier(Identifier),   // @lfy def/grammar/main.lfy:6
    Space(Space),             // @lfy def/grammar/main.lfy:10
    Literal(Literal),         // @lfy def/grammar/main.lfy:1
    Punctuation(Punctuation), // @lfy def/grammar/main.lfy:9
    Expression(Expression),   // @lfy def/grammar/main.lfy:2
    Comment(Comment),         // @lfy def/grammar/main.lfy:5
    Statement(Statement),     // @lfy def/grammar/main.lfy:4
    File(File),               // @lfy def/grammar/main.lfy:3
}

macro_rules! delegate {
    ($self:ident . $method:ident ( $($arg:expr),* )) => {
        match $self {
            Entity::Keyword(rule) => rule.$method($($arg),*),
            Entity::Identifier(rule) => rule.$method($($arg),*),
            Entity::Space(rule) => rule.$method($($arg),*),
            Entity::Literal(rule) => rule.$method($($arg),*),
            Entity::Punctuation(rule) => rule.$method($($arg),*),
            Entity::Expression(rule) => rule.$method($($arg),*),
            Entity::Comment(rule) => rule.$method($($arg),*),
            Entity::Statement(rule) => rule.$method($($arg),*),
            Entity::File(rule) => rule.$method($($arg),*),
        }
    };
}

impl GrammarRule for Entity {
    fn identifier(self) -> &'static str {
        delegate!(self.identifier())
    }

    fn syntax(self) -> &'static str {
        delegate!(self.syntax())
    }

    fn text(self) -> &'static str {
        delegate!(self.text())
    }

    fn definition(self) -> &'static str {
        delegate!(self.definition())
    }

    fn category(self) -> Category {
        delegate!(self.category())
    }

    fn binding(self) -> Option<Binding> {
        delegate!(self.binding())
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.identifier())
    }
}

impl Entity {
    /// Every rule of the grammar in load order; the same as [`rules`].
    pub fn all() -> impl Iterator<Item = Entity> {
        fn all<R: GrammarRule>(all: &'static [R]) -> impl Iterator<Item = Entity> {
            all.iter().map(|rule| rule.entity())
        }
        all(Keyword::ALL)
            .chain(all(Identifier::ALL))
            .chain(all(Space::ALL))
            .chain(all(Literal::ALL))
            .chain(all(Punctuation::ALL))
            .chain(all(Expression::ALL))
            .chain(all(Comment::ALL))
            .chain(all(Statement::ALL))
            .chain(all(File::ALL))
    }

    /// The rule with the given identifier.
    pub fn lookup(identifier: &str) -> Option<Entity> {
        static INDEX: OnceLock<HashMap<&'static str, Entity>> = OnceLock::new();
        INDEX
            .get_or_init(|| {
                Entity::all()
                    .map(|rule| (rule.identifier(), rule))
                    .collect()
            })
            .get(identifier)
            .copied()
    }

    /// `terminal@entities`: every rule matched against characters and delivered as a
    /// single token.
    pub fn terminals() -> impl Iterator<Item = Entity> {
        Entity::all().filter(|rule| rule.is_terminal())
    }
}

/// `rule@entities`: every rule of the grammar in load order.
pub fn rules() -> impl Iterator<Item = Entity> {
    Entity::all()
}

/// The EBNF of every terminal, one rule per line.
// @lfy def/grammar/main.lfy:4
pub fn terminal_document() -> String {
    let lines: Vec<&'static str> = Entity::terminals().map(|rule| rule.rule().text).collect(); // @lfy def/grammar/main.lfy:6
    lines.join("\n") // @lfy def/grammar/main.lfy:9
}

/// The EBNF of every rule, one rule per line.
// @lfy def/grammar/main.lfy:12
pub fn grammar_document() -> String {
    let lines: Vec<&'static str> = rules().map(|rule| rule.rule().text).collect(); // @lfy def/grammar/main.lfy:14
    lines.join("\n") // @lfy def/grammar/main.lfy:18
}

/// The global acceptance criteria of the grammar: every [`Check`] holds for every rule,
/// otherwise each failing rule is reported with the check it fails.
// @lfy def/grammar/main.lfy:21
pub fn validate() -> Result<(), Vec<Violation>> {
    checks::validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // @lfy def/grammar/main.lfy:24
    #[test]
    fn rule_identifiers_are_unique_and_looked_up_by_name() {
        let mut seen = HashSet::new();
        for rule in rules() {
            assert!(seen.insert(rule.identifier()), "{} declared twice", rule);
            assert_eq!(Entity::lookup(rule.identifier()), Some(rule));
        }
        assert_eq!(seen.len(), rules().count());
        assert_eq!(Entity::lookup("NoSuchRule"), None);
        assert_eq!(
            Entity::lookup("SourceFile"),
            Some(Entity::File(File::SourceFile))
        );
    }

    #[test]
    fn rules_load_in_file_order() {
        let first: Vec<&str> = rules().take(2).map(|rule| rule.identifier()).collect();
        assert_eq!(first, vec!["AgentDataKeyword", "AgentFunctionKeyword"]);
        assert_eq!(
            rules().last().map(|rule| rule.identifier()),
            Some("SourceFile")
        );
        let mut previous = 0;
        for enum_start in [
            "AgentDataKeyword",
            "IdentifierStart",
            "NewLine",
            "Character",
            "ValueAccessor",
            "MemberName",
            "BlockCommentOpen",
            "DataDeclaration",
            "SourceFile",
        ] {
            let index = rules()
                .position(|rule| rule.identifier() == enum_start)
                .unwrap();
            assert!(index >= previous, "{enum_start}");
            previous = index;
        }
    }

    // @lfy def/grammar/main.lfy:4
    #[test]
    fn the_terminal_document_lists_every_terminal_once_per_line() {
        let document = terminal_document();
        // The NewLine rule's own syntax holds line breaks, so the lines are compared by
        // joining the rule texts rather than by splitting the document.
        let texts: Vec<&str> = Entity::terminals()
            .map(|terminal| terminal.text())
            .collect();
        assert_eq!(document, texts.join("\n"));
        assert_eq!(texts.len(), 153);
        assert!(
            document.starts_with("AgentDataKeyword = \"d\" ;\nAgentFunctionKeyword = \"fn\" ;\n")
        );
        assert!(document.contains(
            "\nIdentifier = ( [[IdentifierStart]] | \"_\" ) , (: [[IdentifierContinue]] :) ;\n"
        ));
        assert!(!document.contains("SourceFile"));
        assert!(!document.contains("\nCharacter ="));
        assert!(!document.contains("\nStatement ="));
    }

    // @lfy def/grammar/main.lfy:12
    #[test]
    fn the_grammar_document_lists_every_rule_once_per_line() {
        let document = grammar_document();
        let texts: Vec<&str> = rules().map(|rule| rule.text()).collect();
        assert_eq!(document, texts.join("\n"));
        assert_eq!(texts.len(), 275);
        assert!(document.ends_with("SourceFile = (: [[Statement]] :) ;"));
        assert!(document.len() > terminal_document().len());
    }

    // @lfy def/grammar/main.lfy:21
    #[test]
    fn the_grammar_satisfies_its_global_acceptance_criteria() {
        assert_eq!(validate(), Ok(()));
    }

    #[test]
    fn every_rule_compiles_as_ebnf_and_resolves_its_references() {
        let grammar = ebnf::grammar();
        for rule in rules() {
            assert!(
                grammar.expression(rule.identifier()).is_some()
                    || grammar.builtin(rule.identifier()).is_some(),
                "{rule} was not compiled"
            );
        }
    }

    #[test]
    fn entities_display_as_their_identifier_and_convert_from_their_enums() {
        assert_eq!(Entity::from(Keyword::IfKeyword).to_string(), "IfKeyword");
        assert_eq!(
            Keyword::IfKeyword.entity(),
            Entity::Keyword(Keyword::IfKeyword)
        );
        assert_eq!(
            Entity::Space(Space::Space).definition(),
            "Various space characters"
        );
        assert_eq!(Entity::Keyword(Keyword::AbstractKeyword).definition(), "");
    }
}
