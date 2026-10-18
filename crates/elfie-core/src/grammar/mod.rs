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

pub mod checks; // @lfy def/grammar/main.lfy:grammarDocument
pub mod ebnf;
pub mod precedence; // @lfy def/grammar/main.lfy:grammarDocument
pub mod rules; // @lfy def/grammar/main.lfy:grammarDocument
pub mod terminals; // @lfy def/grammar/main.lfy:terminalDocument
pub mod traits; // @lfy def/grammar/main.lfy:grammarDocument

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
    Keyword(Keyword),         // @lfy def/grammar/main.lfy:terminalDocument
    Identifier(Identifier),   // @lfy def/grammar/main.lfy:terminalDocument
    Space(Space),             // @lfy def/grammar/main.lfy:terminalDocument
    Literal(Literal),         // @lfy def/grammar/main.lfy:terminalDocument
    Punctuation(Punctuation), // @lfy def/grammar/main.lfy:terminalDocument
    Expression(Expression),   // @lfy def/grammar/main.lfy:grammarDocument
    Comment(Comment),         // @lfy def/grammar/main.lfy:terminalDocument
    Statement(Statement),     // @lfy def/grammar/main.lfy:terminalDocument
    File(File),               // @lfy def/grammar/main.lfy:grammarDocument
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

/// The rules named, each as `File::Rule`, wrapped as an [`Entity`] array in the order given.
macro_rules! entities {
    ($($kind:ident :: $rule:ident),+ $(,)?) => {
        [$(Entity::$kind($kind::$rule)),+]
    };
}

/// The EBNF of every terminal, one per line.
// @lfy def/grammar/main.lfy:terminalDocument
pub fn terminal_document() -> String {
    let mut lines: Vec<&'static str> = Vec::new();
    for entity in entities![
        Space::NewLine, Space::Space, Literal::NumberLiteral, Literal::SingleQuote,
        Literal::DoubleQuote, Literal::Backtick, Literal::ExecutionOpen, Literal::ExecutionClose,
        Literal::ReferenceOpen, Literal::ReferenceClose, Literal::BackslashEscape,
        Literal::SingleQuoteEscape, Literal::DoubleQuoteEscape, Literal::BacktickEscape,
        Literal::NewlineEscape, Literal::CarriageReturnEscape, Literal::TabEscape,
        Literal::NullEscape, Literal::ContinuationEscape, Literal::HexEscape,
        Literal::UnicodeEscape, Literal::ExecutionOpenEscape, Literal::ExecutionCloseEscape,
        Literal::ReferenceOpenEscape, Literal::ReferenceCloseEscape, Literal::SingleQuoteBody,
        Literal::DoubleQuoteBody, Literal::TemplateBody, Comment::BlockCommentOpen,
        Comment::BlockCommentClose, Comment::BlockCommentBody, Comment::LineCommentOpen,
        Comment::LineCommentBody, Comment::BlockDocumentationOpen,
        Comment::BlockDocumentationClose, Comment::BlockDocumentationBody,
        Comment::LineDocumentationOpen, Comment::LineDocumentationBody,
        Keyword::AgentDataKeyword, Keyword::AgentFunctionKeyword, Keyword::FunctionKeyword,
        Keyword::TraitKeyword, Keyword::TypeKeyword, Keyword::EnumKeyword, Keyword::AliasKeyword,
        Keyword::ConstKeyword, Keyword::LetKeyword, Keyword::ExternalKeyword,
        Keyword::UseKeyword, Keyword::IsKeyword, Keyword::ExtendsKeyword, Keyword::AsKeyword,
        Keyword::WithKeyword, Keyword::WhereKeyword, Keyword::AndKeyword, Keyword::OrKeyword,
        Keyword::DefaultKeyword, Keyword::IfKeyword, Keyword::ElseKeyword, Keyword::ForKeyword,
        Keyword::InKeyword, Keyword::OfKeyword, Keyword::FromKeyword, Keyword::WhileKeyword,
        Keyword::LoopKeyword, Keyword::BreakKeyword, Keyword::ContinueKeyword,
        Keyword::ReturnKeyword, Keyword::MatchKeyword, Keyword::MatchallKeyword,
        Keyword::AsyncKeyword, Keyword::AwaitKeyword, Keyword::AceKeyword, Keyword::NullKeyword,
        Keyword::UndefinedKeyword, Keyword::TrueKeyword, Keyword::FalseKeyword,
        Keyword::BooleanKeyword, Keyword::NumberKeyword, Keyword::StringKeyword,
        Keyword::ObjectKeyword, Keyword::AbstractKeyword, Keyword::CaseKeyword,
        Keyword::ClassKeyword, Keyword::ImplKeyword, Keyword::InterfaceKeyword,
        Keyword::NewKeyword, Keyword::ModuleKeyword, Keyword::PrivateKeyword,
        Keyword::PublicKeyword, Keyword::SelfKeyword, Keyword::StaticKeyword,
        Keyword::SuperKeyword, Keyword::SwitchKeyword, Keyword::ThenKeyword,
        Keyword::TypeofKeyword, Keyword::YieldKeyword, Identifier::Identifier,
        Punctuation::ValueAccessor, Punctuation::OptionalValueAccessor,
        Punctuation::ContextAccessor, Punctuation::ScopeAccessor,
        Punctuation::ParentScopeAccessor, Punctuation::GroupOpen, Punctuation::GroupClose,
        Punctuation::ListOpen, Punctuation::ListClose, Punctuation::BlockOpen,
        Punctuation::BlockClose, Punctuation::Comma, Punctuation::Semicolon, Punctuation::Colon,
        Punctuation::QuestionMark, Punctuation::Spread, Punctuation::PreviousStatement,
        Punctuation::Ampersand, Punctuation::SingleArrowRight, Punctuation::SingleArrowRightGlyph,
        Punctuation::SingleArrowLeft, Punctuation::SingleArrowLeftGlyph,
        Punctuation::DoubleArrowRight, Punctuation::PlainSetter, Punctuation::ToggleSetter,
        Punctuation::AddSetter, Punctuation::SubtractSetter, Punctuation::MultiplySetter,
        Punctuation::PowerSetter, Punctuation::BitwiseAndSetter, Punctuation::BitwiseOrSetter,
        Punctuation::NullishSetter, Punctuation::FalseySetter, Punctuation::TruthySetter,
        Punctuation::LessThan, Punctuation::LessThanOrEqual, Punctuation::GreaterThan,
        Punctuation::GreaterThanOrEqual, Punctuation::Equal, Punctuation::NotEqual,
        Punctuation::SameReference, Punctuation::AboutEqual, Punctuation::LogicalAnd,
        Punctuation::LogicalOr, Punctuation::NullishOr, Punctuation::LogicalNot,
        Punctuation::Plus, Punctuation::Minus, Punctuation::Star, Punctuation::Slash,
        Punctuation::Percent, Punctuation::Power, Punctuation::BitwiseOr, Punctuation::BitwiseXor,
        Punctuation::BitwiseNot,
    ] {
        lines.push(entity.rule().text);
    }
    lines.join("\n")
}

/// The EBNF of every rule, one per line.
// @lfy def/grammar/main.lfy:grammarDocument
pub fn grammar_document() -> String {
    let mut lines: Vec<&'static str> = Vec::new();
    for entity in entities![
        Space::NewLine, Space::Space, Literal::NumberLiteral, Literal::SingleQuote,
        Literal::DoubleQuote, Literal::Backtick, Literal::ExecutionOpen, Literal::ExecutionClose,
        Literal::ReferenceOpen, Literal::ReferenceClose, Literal::BackslashEscape,
        Literal::SingleQuoteEscape, Literal::DoubleQuoteEscape, Literal::BacktickEscape,
        Literal::NewlineEscape, Literal::CarriageReturnEscape, Literal::TabEscape,
        Literal::NullEscape, Literal::ContinuationEscape, Literal::HexEscape,
        Literal::UnicodeEscape, Literal::ExecutionOpenEscape, Literal::ExecutionCloseEscape,
        Literal::ReferenceOpenEscape, Literal::ReferenceCloseEscape, Literal::SingleQuoteBody,
        Literal::DoubleQuoteBody, Literal::TemplateBody, Literal::SingleQuoteString,
        Literal::DoubleQuoteString, Comment::BlockCommentOpen, Comment::BlockCommentClose,
        Comment::BlockCommentBody, Comment::LineCommentOpen, Comment::LineCommentBody,
        Comment::BlockDocumentationOpen, Comment::BlockDocumentationClose,
        Comment::BlockDocumentationBody, Comment::LineDocumentationOpen,
        Comment::LineDocumentationBody, Comment::Comment, Comment::Documentation,
        Keyword::AgentDataKeyword, Keyword::AgentFunctionKeyword, Keyword::FunctionKeyword,
        Keyword::TraitKeyword, Keyword::TypeKeyword, Keyword::EnumKeyword, Keyword::AliasKeyword,
        Keyword::ConstKeyword, Keyword::LetKeyword, Keyword::ExternalKeyword,
        Keyword::UseKeyword, Keyword::IsKeyword, Keyword::ExtendsKeyword, Keyword::AsKeyword,
        Keyword::WithKeyword, Keyword::WhereKeyword, Keyword::AndKeyword, Keyword::OrKeyword,
        Keyword::DefaultKeyword, Keyword::IfKeyword, Keyword::ElseKeyword, Keyword::ForKeyword,
        Keyword::InKeyword, Keyword::OfKeyword, Keyword::FromKeyword, Keyword::WhileKeyword,
        Keyword::LoopKeyword, Keyword::BreakKeyword, Keyword::ContinueKeyword,
        Keyword::ReturnKeyword, Keyword::MatchKeyword, Keyword::MatchallKeyword,
        Keyword::AsyncKeyword, Keyword::AwaitKeyword, Keyword::AceKeyword, Keyword::NullKeyword,
        Keyword::UndefinedKeyword, Keyword::TrueKeyword, Keyword::FalseKeyword,
        Keyword::BooleanKeyword, Keyword::NumberKeyword, Keyword::StringKeyword,
        Keyword::ObjectKeyword, Keyword::AbstractKeyword, Keyword::CaseKeyword,
        Keyword::ClassKeyword, Keyword::ImplKeyword, Keyword::InterfaceKeyword,
        Keyword::NewKeyword, Keyword::ModuleKeyword, Keyword::PrivateKeyword,
        Keyword::PublicKeyword, Keyword::SelfKeyword, Keyword::StaticKeyword,
        Keyword::SuperKeyword, Keyword::SwitchKeyword, Keyword::ThenKeyword,
        Keyword::TypeofKeyword, Keyword::YieldKeyword, Keyword::Keyword, Identifier::Identifier,
        Punctuation::ValueAccessor, Punctuation::OptionalValueAccessor,
        Punctuation::ContextAccessor, Punctuation::ScopeAccessor,
        Punctuation::ParentScopeAccessor, Punctuation::Accessor, Punctuation::GroupOpen,
        Punctuation::GroupClose, Punctuation::ListOpen, Punctuation::ListClose,
        Punctuation::BlockOpen, Punctuation::BlockClose, Punctuation::Comma,
        Punctuation::Semicolon, Punctuation::Colon, Punctuation::QuestionMark,
        Punctuation::Spread, Punctuation::PreviousStatement, Punctuation::Ampersand,
        Punctuation::SingleArrowRight, Punctuation::SingleArrowRightGlyph,
        Punctuation::SingleArrowLeft, Punctuation::SingleArrowLeftGlyph,
        Punctuation::DoubleArrowRight, Punctuation::SingleArrow, Punctuation::PlainSetter,
        Punctuation::ToggleSetter, Punctuation::AddSetter, Punctuation::SubtractSetter,
        Punctuation::MultiplySetter, Punctuation::PowerSetter, Punctuation::BitwiseAndSetter,
        Punctuation::BitwiseOrSetter, Punctuation::NullishSetter, Punctuation::FalseySetter,
        Punctuation::TruthySetter, Punctuation::Setter, Punctuation::LessThan,
        Punctuation::LessThanOrEqual, Punctuation::GreaterThan, Punctuation::GreaterThanOrEqual,
        Punctuation::Relational, Punctuation::Equal, Punctuation::NotEqual,
        Punctuation::SameReference, Punctuation::AboutEqual, Punctuation::Equality,
        Punctuation::LogicalAnd, Punctuation::LogicalOr, Punctuation::NullishOr,
        Punctuation::Coalescence, Punctuation::LogicalNot, Punctuation::Plus, Punctuation::Minus,
        Punctuation::Additive, Punctuation::Star, Punctuation::Slash, Punctuation::Percent,
        Punctuation::Multiplicative, Punctuation::Power, Punctuation::BitwiseOr,
        Punctuation::BitwiseXor, Punctuation::BitwiseNot, Punctuation::Punctuation,
        Expression::MemberName, Expression::TraitUse, Expression::TraitUses,
        Expression::IsClause, Expression::ExtendsClause, Expression::DefinitionClause,
        Expression::Declared, Expression::ObjectKey, Expression::Parameter,
        Expression::SpreadParameter, Expression::Parameters, Expression::TypeParameter,
        Expression::TypeParameters, Expression::TypeArguments, Expression::Signature,
        Expression::Items, Expression::Arguments, Expression::Reference,
        Expression::TemplateReference, Expression::TemplateExecution, Expression::TypeValue,
        Expression::FunctionType, Expression::TypeGroup, Expression::TypeItem,
        Expression::TypeExpression, Expression::TypeKey, Expression::StringLiteral,
        Expression::Template, Expression::Number, Expression::Boolean, Expression::Nullish,
        Expression::PrimitiveType, Expression::Name, Expression::Current, Expression::Previous,
        Expression::Group, Expression::InlineFunction, Expression::List, Expression::Object,
        Expression::TypePredicate, Expression::Type, Expression::NotOperation,
        Expression::NegateOperation, Expression::BitwiseNotOperation, Expression::Dereference,
        Expression::SpreadOperation, Expression::AwaitOperation, Expression::InOperation,
        Expression::OfOperation, Expression::FromOperation, Expression::AdditiveOperation,
        Expression::MultiplicativeOperation, Expression::PowerOperation,
        Expression::BitwiseOrOperation, Expression::BitwiseXorOperation,
        Expression::BitwiseAndOperation, Expression::RelationalOperation,
        Expression::EqualityOperation, Expression::LogicalAndOperation,
        Expression::CoalescenceOperation, Expression::RangeOperation, Expression::Assignment,
        Expression::Member, Expression::Index, Expression::Call, Expression::Traits,
        Expression::Definition, Expression::Cast, Expression::Generic, Expression::Conditional,
        Expression::Primary, Expression::Prefix, Expression::Infix, Expression::Postfix,
        Expression::Expression, Statement::DataDeclaration, Statement::AgentFunctionDeclaration,
        Statement::FunctionDeclaration, Statement::TraitDeclaration, Statement::TypeDeclaration,
        Statement::EnumDeclaration, Statement::VariableDeclaration, Statement::AliasDeclaration,
        Statement::ExternalDeclaration, Statement::Use, Statement::Block, Statement::If,
        Statement::Else, Statement::ForFrom, Statement::ForInOf, Statement::For,
        Statement::While, Statement::Loop, Statement::Break, Statement::Continue,
        Statement::Return, Statement::MatchArm, Statement::Match, Statement::With,
        Statement::Async, Statement::Ace, Statement::ExpressionStatement, Statement::Condition,
        Statement::ConditionGroup, Statement::Conditions, Statement::Where,
        Statement::Statement, File::SourceFile,
    ] {
        lines.push(entity.rule().text);
    }
    lines.join("\n")
}

/// The global acceptance criteria of the grammar: every [`Check`] holds for every rule,
/// otherwise each failing rule is reported with the check it fails.
// @lfy def/grammar/main.lfy:grammarDocument
pub fn validate() -> Result<(), Vec<Violation>> {
    checks::validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:afbfe42cdc8454da6102f966fb6e895d078bebe73b3c95796edf1fb22f5f5b5c
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

    // @lfy def/grammar/main.lfy:terminalDocument
    #[test]
    fn the_terminal_document_lists_every_terminal_once_per_line() {
        let document = terminal_document();
        // The NewLine rule's own syntax holds line breaks, so the document is searched for
        // each rule text rather than split into lines.
        assert!(document.starts_with(Entity::Space(Space::NewLine).text()));
        assert!(document.ends_with(Entity::Punctuation(Punctuation::BitwiseNot).text()));
        for terminal in [
            Entity::Keyword(Keyword::AgentDataKeyword),
            Entity::Identifier(Identifier::Identifier),
            Entity::Literal(Literal::TemplateBody),
            Entity::Comment(Comment::LineCommentBody),
        ] {
            assert!(terminal.is_terminal(), "{terminal}");
            assert!(document.contains(&format!("\n{}\n", terminal.text())), "{terminal}");
        }
        assert!(!document.contains("SourceFile"));
        assert!(!document.contains("\nCharacter ="));
        assert!(!document.contains("\nStatement ="));
    }

    // @lfy def/grammar/main.lfy:grammarDocument
    #[test]
    fn the_grammar_document_lists_every_rule_once_per_line() {
        let document = grammar_document();
        assert!(document.starts_with(Entity::Space(Space::NewLine).text()));
        assert!(document.ends_with(Entity::File(File::SourceFile).text()));
        for rule in [
            Entity::Keyword(Keyword::Keyword),
            Entity::Literal(Literal::DoubleQuoteString),
            Entity::Expression(Expression::Expression),
            Entity::Statement(Statement::Statement),
        ] {
            assert!(document.contains(&format!("\n{}\n", rule.text())), "{rule}");
        }
        assert!(document.len() > terminal_document().len());
    }

    // @lfy def/grammar/main.lfy:grammarDocument
    #[test]
    fn the_grammar_satisfies_its_global_acceptance_criteria() {
        assert_eq!(validate(), Ok(()));
    }

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:efd1981bf1275d82e99ce8975d9008db6dfe0706b3d371324a263990718e239e
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
            "Whitespace within a line; U+0085, U+2028, and U+2029 are space here, although Unicode counts them as line separators"
        );
        assert_eq!(Entity::Keyword(Keyword::AbstractKeyword).definition(), "");
    }
}
