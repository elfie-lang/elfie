//! Compiled from `def/grammar/rules/expression.lfy`.

use super::super::terminals::identifier::Identifier;
use super::super::terminals::keyword::Keyword;
use super::super::terminals::punctuation::Punctuation;
use super::super::traits::category::*;
use super::super::{Binding, Entity, Level, grammar_rules};

/// `ace function listOf(syntax)`: an EBNF list of `syntax` separated by commas, with an
/// optional trailing comma. Every list in this file's syntax is this function's result.
// @lfy def/grammar/rules/expression.lfy:8
pub fn list_of(syntax: &str) -> String {
    format!("( {syntax} ) , (: [[Comma]] , ( {syntax} ) :) , (/ [[Comma]] /)") // @lfy def/grammar/rules/expression.lfy:9
}

grammar_rules! {
    /// Expression rules: helpers, primaries, prefix, infix and postfix operations, and the
    /// sets that group them.
    pub enum Expression {
        // Helpers: rules other rules are built from
        /// `alternationList(Identifier, TypeKeyword)`
        MemberName is [alternation_list(MEMBER_NAMES)]: "A name after an accessor; The type keyword is allowed because it is a context property" = "[[Identifier]] | [[TypeKeyword]]", // @lfy def/grammar/rules/expression.lfy:14
        TraitUse is [rule()]: "A trait, with arguments when it takes any" = "[[Identifier]] , (: [[ValueAccessor]] , [[Identifier]] :) , (/ [[Arguments]] /)", // @lfy def/grammar/rules/expression.lfy:15
        /// `listOf([[TraitUse]])`
        TraitUses is [rule()]: "One or more traits" = "( [[TraitUse]] ) , (: [[Comma]] , ( [[TraitUse]] ) :) , (/ [[Comma]] /)", // @lfy def/grammar/rules/expression.lfy:16
        IsClause is [rule()]: "Traits applied to a declaration" = "[[IsKeyword]] , [[TraitUses]]", // @lfy def/grammar/rules/expression.lfy:17
        ExtendsClause is [rule()]: "Traits or data a declaration includes" = "[[ExtendsKeyword]] , [[TraitUses]]", // @lfy def/grammar/rules/expression.lfy:18
        DefinitionClause is [rule()]: "A description or type attached to a declaration" = "[[Colon]] , [[TypeExpression]]", // @lfy def/grammar/rules/expression.lfy:19
        Declared is [rule()]: "A declared name with its traits and definition" = "[[Identifier]] , (/ [[IsClause]] /) , (/ [[DefinitionClause]] /)", // @lfy def/grammar/rules/expression.lfy:20
        ObjectKey is [rule()]: "An object key name with its traits, definition, and value" = "[[Declared]] , [[PlainSetter]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:21
        Parameter is [rule()]: "A single parameter" = "[[Name]] , (/ [[QuestionMark]] /) , (/ [[DefinitionClause]] /) , (/ [[PlainSetter]] , [[Expression]] /)", // @lfy def/grammar/rules/expression.lfy:22
        SpreadParameter is [rule()]: "A spread parameter" = "[[Spread]] , [[Name]] , (/ [[DefinitionClause]] /) , (/ [[PlainSetter]] , [[Expression]] /)", // @lfy def/grammar/rules/expression.lfy:23
        Parameters is [rule()]: "A parameter list" = "[[GroupOpen]] , (/ ( ( [[Parameter]] , (: [[Comma]] , [[Parameter]] :) , (/ [[Comma]] , [[SpreadParameter]] /) ) | [[SpreadParameter]] ) , (/ [[Comma]] /) /) , [[GroupClose]]", // @lfy def/grammar/rules/expression.lfy:24
        Signature is [rule()]: "A function's name, parameters, traits, and definition" = "(/ [[Identifier]] /) , [[Parameters]] , (/ [[IsClause]] /) , (/ [[DefinitionClause]] /)", // @lfy def/grammar/rules/expression.lfy:25
        /// `(/ listOf([[Expression]]) /)`
        Items is [rule()]: "Zero or more comma separated expressions" = "(/ ( [[Expression]] ) , (: [[Comma]] , ( [[Expression]] ) :) , (/ [[Comma]] /) /)", // @lfy def/grammar/rules/expression.lfy:26
        Arguments is [rule()]: "A parenthesized list" = "[[GroupOpen]] , [[Items]] , [[GroupClose]]", // @lfy def/grammar/rules/expression.lfy:27
        Reference is [rule()]: "A reference chain" = "( [[Name]] | [[Current]] | [[Dereference]] | [[Member]] | [[Index]] ) ", // @lfy def/grammar/rules/expression.lfy:28
        TemplateReference is [rule()]: "A reference to a name inside a template or documentation" = "[[ReferenceOpen]] , [[Reference]] , [[ReferenceClose]]", // @lfy def/grammar/rules/expression.lfy:29
        TemplateExecution is [rule()]: "An expression whose value is written into a template" = "[[ExecutionOpen]] , [[Expression]] , [[ExecutionClose]]", // @lfy def/grammar/rules/expression.lfy:30
        /// `alternationList(PrimitiveType, StringLiteral, Template, Number, Nullish, Object, List, TypePredicate, Reference)`
        TypeValue is [alternation_list(TYPE_VALUES)]: "A type value" = "[[PrimitiveType]] | [[StringLiteral]] | [[Template]] | [[Number]] | [[Nullish]] | [[Object]] | [[List]] | [[TypePredicate]] | [[Reference]]", // @lfy def/grammar/rules/expression.lfy:31
        TypeItem is [rule()]: "A type, or an array of it" = "( [[TypeValue]] | ( [[GroupOpen]] , [[TypeExpression]] , [[GroupClose]] ) ) , (/ [[ListOpen]] , [[ListClose]] /)", // @lfy def/grammar/rules/expression.lfy:32
        TypeExpression is [rule()]: "An expression in a type position" = "(/ [[BitwiseOr]] | [[Ampersand]] /) , [[TypeItem]] , (: ( [[BitwiseOr]] | [[Ampersand]] ) , [[TypeItem]] :)", // @lfy def/grammar/rules/expression.lfy:33
        TypeKey is [rule()]: "A type key name with its definition and value" = "[[Name]] , (/ [[QuestionMark]] /) , (/ [[DefinitionClause]] /) , [[PlainSetter]] , [[TypeExpression]]", // @lfy def/grammar/rules/expression.lfy:34

        // Primaries
        StringLiteral is [primary()]: "A string" = "[[SingleQuoteString]] | [[DoubleQuoteString]]", // @lfy def/grammar/rules/expression.lfy:38
        Template is [primary()]: "A template: text with references and expressions" = "[[Backtick]] , (: [[TemplateBody]] | [[TemplateReference]] | [[TemplateExecution]] :) , [[Backtick]]", // @lfy def/grammar/rules/expression.lfy:39
        Number is [primary()]: "A number" = "[[NumberLiteral]]", // @lfy def/grammar/rules/expression.lfy:40
        Boolean is [primary()]: "A boolean" = "[[TrueKeyword]] | [[FalseKeyword]]", // @lfy def/grammar/rules/expression.lfy:41
        Nullish is [primary()]: "Null or undefined" = "[[NullKeyword]] | [[UndefinedKeyword]]", // @lfy def/grammar/rules/expression.lfy:42
        PrimitiveType is [primary()]: "A primitive type used as a value" = "[[BooleanKeyword]] | [[NumberKeyword]] | [[StringKeyword]] | [[ObjectKeyword]] | [[FunctionKeyword]] | [[TraitKeyword]]", // @lfy def/grammar/rules/expression.lfy:43
        Name is [primary()]: "A name in scope" = "[[Identifier]]", // @lfy def/grammar/rules/expression.lfy:44
        /// When space, a line break, comments, or documentation exist between the
        /// `Accessor` and the `MemberName`, the `MemberName` is not matched.
        Current is [primary()]: "A layer of the current entity, or a member of it" = "[[Accessor]] , (/ [[MemberName]] /)", // @lfy def/grammar/rules/expression.lfy:45
        Previous is [primary()]: "The entity the previous statement declared" = "[[PreviousStatement]]", // @lfy def/grammar/rules/expression.lfy:48
        /// When the `GroupClose` is followed by `IsKeyword`, `Colon`, or `DoubleArrowRight`
        /// the rule cannot be a match; see [`group_cannot_match_before`].
        Group is [primary()]: "A parenthesized expression" = "[[GroupOpen]] , [[Expression]] , [[GroupClose]]", // @lfy def/grammar/rules/expression.lfy:49
        /// Acceptance criteria:
        /// - A `BlockOpen` after the `DoubleArrowRight` begins a `Block`, never an `Object`.
        InlineFunction is [primary()]: "A function written as an expression" = "[[Parameters]] , (/ [[IsClause]] /) , (/ [[DefinitionClause]] /) , (/ [[SingleArrow]] , [[TypeExpression]] /) , [[DoubleArrowRight]] , ( [[Block]] | [[Expression]] )", // @lfy def/grammar/rules/expression.lfy:52
        List is [primary()]: "A list" = "[[ListOpen]] , [[Items]] , [[ListClose]]", // @lfy def/grammar/rules/expression.lfy:56
        /// `[[BlockOpen]] , (/ listOf([[ObjectKey]]) /) , [[BlockClose]]`
        Object is [primary()]: "An object" = "[[BlockOpen]] , (/ ( [[ObjectKey]] ) , (: [[Comma]] , ( [[ObjectKey]] ) :) , (/ [[Comma]] /) /) , [[BlockClose]]", // @lfy def/grammar/rules/expression.lfy:57
        TypePredicate is [primary()]: "The type of anything with a trait" = "[[IsKeyword]] , [[TraitUse]]", // @lfy def/grammar/rules/expression.lfy:58
        /// `[[BlockOpen]] , (/ listOf([[TypeKey]]) /) , [[BlockClose]]`
        Type is [primary()]: "A type" = "[[BlockOpen]] , (/ ( [[TypeKey]] ) , (: [[Comma]] , ( [[TypeKey]] ) :) , (/ [[Comma]] /) /) , [[BlockClose]]", // @lfy def/grammar/rules/expression.lfy:59

        // Prefix operations
        NotOperation is [prefix(Entity::Punctuation(Punctuation::LogicalNot), false), Binding::right(Level::Unary)]: "Logical not" = "[[LogicalNot]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:63
        NegateOperation is [prefix(Entity::Punctuation(Punctuation::Minus), true), Binding::right(Level::Unary)]: "Negation" = "[[Minus]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:64
        BitwiseNotOperation is [prefix(Entity::Punctuation(Punctuation::BitwiseNot), false), Binding::right(Level::Unary)]: "Bitwise not" = "[[BitwiseNot]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:65
        /// Acceptance criteria:
        /// - When the operand is not a `Name` or `Current`: reported after parsing.
        Dereference is [prefix(Entity::Punctuation(Punctuation::Ampersand), true), Binding::right(Level::Reference)]: "The entity a name is bound to" = "[[Ampersand]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:66
        SpreadOperation is [prefix(Entity::Punctuation(Punctuation::Spread), false), Binding::right(Level::Definition)]: "Spread, or the rest of a list" = "[[Spread]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:73
        AwaitOperation is [prefix(Entity::Keyword(Keyword::AwaitKeyword), false), Binding::non_associative(Level::Wrapper)]: "The value of a promise" = "[[AwaitKeyword]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:74
        InOperation is [prefix(Entity::Keyword(Keyword::InKeyword), false), Binding::right(Level::Extraction)]: "The values of" = "[[InKeyword]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:75
        OfOperation is [prefix(Entity::Keyword(Keyword::OfKeyword), false), Binding::right(Level::Extraction)]: "The keys of" = "[[OfKeyword]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:76
        FromOperation is [prefix(Entity::Keyword(Keyword::FromKeyword), false), Binding::right(Level::Extraction)]: "The keys and values of" = "[[FromKeyword]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:77

        // Infix operations
        AdditiveOperation is [infix(Entity::Punctuation(Punctuation::Additive))]: "Addition, concatenation, or subtraction" = "[[Expression]] , [[Additive]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:81
        MultiplicativeOperation is [infix(Entity::Punctuation(Punctuation::Multiplicative))]: "Multiplication, division, or remainder" = "[[Expression]] , [[Multiplicative]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:82
        PowerOperation is [infix(Entity::Punctuation(Punctuation::Power)), Binding::right(Level::Exponentiation)]: "Exponentiation" = "[[Expression]] , [[Power]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:83
        BitwiseOrOperation is [infix(Entity::Punctuation(Punctuation::BitwiseOr)), Binding::left(Level::BitwiseOr)]: "Bitwise or; a union when both sides are types" = "[[Expression]] , [[BitwiseOr]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:84
        BitwiseXorOperation is [infix(Entity::Punctuation(Punctuation::BitwiseXor)), Binding::left(Level::BitwiseXor)]: "Bitwise exclusive or" = "[[Expression]] , [[BitwiseXor]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:85
        BitwiseAndOperation is [infix(Entity::Punctuation(Punctuation::Ampersand)), Binding::left(Level::BitwiseAnd)]: "Bitwise and" = "[[Expression]] , [[Ampersand]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:86
        RelationalOperation is [infix(Entity::Punctuation(Punctuation::Relational))]: "Ordering comparison" = "[[Expression]] , [[Relational]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:87
        EqualityOperation is [infix(Entity::Punctuation(Punctuation::Equality))]: "Equality comparison" = "[[Expression]] , [[Equality]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:88
        LogicalAndOperation is [infix(Entity::Punctuation(Punctuation::LogicalAnd)), Binding::left(Level::LogicalAnd)]: "Both" = "[[Expression]] , [[LogicalAnd]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:89
        CoalescenceOperation is [infix(Entity::Punctuation(Punctuation::Coalescence))]: "Either, or a fallback" = "[[Expression]] , [[Coalescence]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:90
        RangeOperation is [infix(Entity::Punctuation(Punctuation::Spread)), Binding::right(Level::Definition)]: "A range" = "[[Expression]] , [[Spread]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:91
        Assignment is [infix(Entity::Punctuation(Punctuation::Setter))]: "Sets the left side" = "[[Expression]] , [[Setter]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:92

        // Postfix operations
        Member is [postfix(Entity::Punctuation(Punctuation::Accessor), Some("(/ [[MemberName]] /)"), true)]: "A member of a layer of the left expression, or that layer itself" = "[[Expression]] , [[Accessor]] , (/ [[MemberName]] /)", // @lfy def/grammar/rules/expression.lfy:96
        Index is [postfix(Entity::Punctuation(Punctuation::ListOpen), Some("(/ [[Expression]] /) , [[ListClose]]"), false)]: "An element of the left expression, or with nothing inside, the array type of it" = "[[Expression]] , [[ListOpen]] , (/ [[Expression]] /) , [[ListClose]]", // @lfy def/grammar/rules/expression.lfy:97
        Call is [postfix(Entity::Punctuation(Punctuation::GroupOpen), Some("[[Items]] , [[GroupClose]]"), false)]: "Calls the left expression" = "[[Expression]] , [[GroupOpen]] , [[Items]] , [[GroupClose]]", // @lfy def/grammar/rules/expression.lfy:98
        Traits is [postfix(Entity::Keyword(Keyword::IsKeyword), Some("[[TraitUses]]"), false)]: "Applies traits to the declared left expression" = "[[Expression]] , [[IsKeyword]] , [[TraitUses]]", // @lfy def/grammar/rules/expression.lfy:99
        Definition is [postfix(Entity::Punctuation(Punctuation::Colon), Some("[[Expression]]"), false)]: "Attaches a description or type to the declared left expression" = "[[Expression]] , [[Colon]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:100
        Cast is [postfix(Entity::Keyword(Keyword::AsKeyword), Some("[[TypeExpression]]"), false)]: "A value read as a type" = "[[Expression]] , [[AsKeyword]] , [[TypeExpression]]", // @lfy def/grammar/rules/expression.lfy:101
        /// Acceptance criteria:
        /// - The `Colon` belongs to this rule; the middle expression ends at it.
        Conditional is [postfix(Entity::Punctuation(Punctuation::QuestionMark), Some("[[Expression]] , [[Colon]] , [[Expression]]"), false), Binding::right(Level::Definition)]: "Chooses between two values" = "[[Expression]] , [[QuestionMark]] , [[Expression]] , [[Colon]] , [[Expression]]", // @lfy def/grammar/rules/expression.lfy:102

        // The expression sets
        /// `alternationList(...primary@entities)`
        Primary is [alternation_list(PRIMARIES)]: "Any primary expression" = "[[StringLiteral]] | [[Template]] | [[Number]] | [[Boolean]] | [[Nullish]] | [[PrimitiveType]] | [[Name]] | [[Current]] | [[Previous]] | [[Group]] | [[InlineFunction]] | [[List]] | [[Object]] | [[TypePredicate]] | [[Type]]", // @lfy def/grammar/rules/expression.lfy:109
        /// `alternationList(...prefix@entities)`
        Prefix is [alternation_list(PREFIXES)]: "Any prefix operation" = "[[NotOperation]] | [[NegateOperation]] | [[BitwiseNotOperation]] | [[Dereference]] | [[SpreadOperation]] | [[AwaitOperation]] | [[InOperation]] | [[OfOperation]] | [[FromOperation]]", // @lfy def/grammar/rules/expression.lfy:110
        /// `alternationList(...infix@entities)`
        Infix is [alternation_list(INFIXES)]: "Any infix operation" = "[[AdditiveOperation]] | [[MultiplicativeOperation]] | [[PowerOperation]] | [[BitwiseOrOperation]] | [[BitwiseXorOperation]] | [[BitwiseAndOperation]] | [[RelationalOperation]] | [[EqualityOperation]] | [[LogicalAndOperation]] | [[CoalescenceOperation]] | [[RangeOperation]] | [[Assignment]]", // @lfy def/grammar/rules/expression.lfy:111
        /// `alternationList(...postfix@entities)`
        Postfix is [alternation_list(POSTFIXES)]: "Any postfix operation" = "[[Member]] | [[Index]] | [[Call]] | [[Traits]] | [[Definition]] | [[Cast]] | [[Conditional]]", // @lfy def/grammar/rules/expression.lfy:112
        /// Acceptance criteria:
        /// - Begins with one `Primary` or `Prefix` and continues with any number of `Infix`
        ///   and `Postfix` operations, grouped by their binding power.
        Expression is [alternation_list(EXPRESSIONS)]: "Any expression" = "[[Primary]] | [[Prefix]] | [[Infix]] | [[Postfix]]", // @lfy def/grammar/rules/expression.lfy:113
    }
}

/// The `where` clause of `Group`: the rule cannot be a match when `next`, the token that
/// follows its `GroupClose`, is `IsKeyword`, `Colon`, or `DoubleArrowRight`.
// @lfy def/grammar/rules/expression.lfy:50
pub fn group_cannot_match_before(next: Entity) -> bool {
    matches!(
        next,
        Entity::Keyword(Keyword::IsKeyword)
            | Entity::Punctuation(Punctuation::Colon | Punctuation::DoubleArrowRight)
    )
}

/// `Identifier, TypeKeyword`
// @lfy def/grammar/rules/expression.lfy:14
pub const MEMBER_NAMES: &[Entity] = &[
    Entity::Identifier(Identifier::Identifier),
    Entity::Keyword(Keyword::TypeKeyword),
];

// @lfy def/grammar/rules/expression.lfy:31
pub const TYPE_VALUES: &[Entity] = &[
    Entity::Expression(Expression::PrimitiveType),
    Entity::Expression(Expression::StringLiteral),
    Entity::Expression(Expression::Template),
    Entity::Expression(Expression::Number),
    Entity::Expression(Expression::Nullish),
    Entity::Expression(Expression::Object),
    Entity::Expression(Expression::List),
    Entity::Expression(Expression::TypePredicate),
    Entity::Expression(Expression::Reference),
];
/// `primary@entities`
// @lfy def/grammar/rules/expression.lfy:109
pub const PRIMARIES: &[Entity] = &[
    Entity::Expression(Expression::StringLiteral),
    Entity::Expression(Expression::Template),
    Entity::Expression(Expression::Number),
    Entity::Expression(Expression::Boolean),
    Entity::Expression(Expression::Nullish),
    Entity::Expression(Expression::PrimitiveType),
    Entity::Expression(Expression::Name),
    Entity::Expression(Expression::Current),
    Entity::Expression(Expression::Previous),
    Entity::Expression(Expression::Group),
    Entity::Expression(Expression::InlineFunction),
    Entity::Expression(Expression::List),
    Entity::Expression(Expression::Object),
    Entity::Expression(Expression::TypePredicate),
    Entity::Expression(Expression::Type),
];
/// `prefix@entities`
// @lfy def/grammar/rules/expression.lfy:110
pub const PREFIXES: &[Entity] = &[
    Entity::Expression(Expression::NotOperation),
    Entity::Expression(Expression::NegateOperation),
    Entity::Expression(Expression::BitwiseNotOperation),
    Entity::Expression(Expression::Dereference),
    Entity::Expression(Expression::SpreadOperation),
    Entity::Expression(Expression::AwaitOperation),
    Entity::Expression(Expression::InOperation),
    Entity::Expression(Expression::OfOperation),
    Entity::Expression(Expression::FromOperation),
];
/// `infix@entities`
// @lfy def/grammar/rules/expression.lfy:111
pub const INFIXES: &[Entity] = &[
    Entity::Expression(Expression::AdditiveOperation),
    Entity::Expression(Expression::MultiplicativeOperation),
    Entity::Expression(Expression::PowerOperation),
    Entity::Expression(Expression::BitwiseOrOperation),
    Entity::Expression(Expression::BitwiseXorOperation),
    Entity::Expression(Expression::BitwiseAndOperation),
    Entity::Expression(Expression::RelationalOperation),
    Entity::Expression(Expression::EqualityOperation),
    Entity::Expression(Expression::LogicalAndOperation),
    Entity::Expression(Expression::CoalescenceOperation),
    Entity::Expression(Expression::RangeOperation),
    Entity::Expression(Expression::Assignment),
];
/// `postfix@entities`
// @lfy def/grammar/rules/expression.lfy:112
pub const POSTFIXES: &[Entity] = &[
    Entity::Expression(Expression::Member),
    Entity::Expression(Expression::Index),
    Entity::Expression(Expression::Call),
    Entity::Expression(Expression::Traits),
    Entity::Expression(Expression::Definition),
    Entity::Expression(Expression::Cast),
    Entity::Expression(Expression::Conditional),
];
// @lfy def/grammar/rules/expression.lfy:113
pub const EXPRESSIONS: &[Entity] = &[
    Entity::Expression(Expression::Primary),
    Entity::Expression(Expression::Prefix),
    Entity::Expression(Expression::Infix),
    Entity::Expression(Expression::Postfix),
];

#[cfg(test)]
mod tests {
    use super::super::super::{Category, GrammarRule, rules};
    use super::*;

    // @lfy def/grammar/rules/expression.lfy:8
    #[test]
    fn list_of_separates_the_syntax_with_commas() {
        assert_eq!(
            list_of("[[X]]"),
            "( [[X]] ) , (: [[Comma]] , ( [[X]] ) :) , (/ [[Comma]] /)"
        );
        assert_eq!(Expression::TraitUses.syntax(), list_of("[[TraitUse]]"));
        assert_eq!(
            Expression::Items.syntax(),
            format!("(/ {} /)", list_of("[[Expression]]"))
        );
        assert_eq!(
            Expression::Object.syntax(),
            format!(
                "[[BlockOpen]] , (/ {} /) , [[BlockClose]]",
                list_of("[[ObjectKey]]")
            )
        );
        assert_eq!(
            Expression::Type.syntax(),
            format!(
                "[[BlockOpen]] , (/ {} /) , [[BlockClose]]",
                list_of("[[TypeKey]]")
            )
        );
    }

    // @lfy def/grammar/rules/expression.lfy:109
    #[test]
    fn the_sets_list_every_rule_of_their_category_in_order() {
        let of = |predicate: fn(Entity) -> bool| -> Vec<Entity> {
            rules().filter(|rule| predicate(*rule)).collect()
        };
        assert_eq!(of(|rule| rule.is_primary()), PRIMARIES);
        assert_eq!(of(|rule| rule.is_prefix()), PREFIXES);
        assert_eq!(of(|rule| rule.is_infix()), INFIXES);
        assert_eq!(of(|rule| rule.is_postfix()), POSTFIXES);
        assert_eq!(MEMBER_NAMES, &[Entity::Identifier(Identifier::Identifier), Entity::Keyword(Keyword::TypeKeyword)]);
        assert_eq!(Expression::MemberName.syntax(), "[[Identifier]] | [[TypeKeyword]]");
        assert_eq!(Expression::ALL.len(), 69);
        assert_eq!(INFIXES.len(), 12);
        assert_eq!(POSTFIXES.len(), 7);
    }

    // @lfy def/grammar/rules/expression.lfy:31
    #[test]
    fn type_values_are_the_listed_rules_in_order() {
        assert_eq!(
            Expression::TypeValue.category(),
            Category::AlternationList(TYPE_VALUES)
        );
        assert_eq!(TYPE_VALUES.len(), 9);
        assert_eq!(TYPE_VALUES[8], Entity::Expression(Expression::Reference));
        assert!(TYPE_VALUES.iter().all(|item| !item.is_terminal()));
        assert_eq!(
            Expression::TypeItem.expression().unwrap().references(),
            vec!["TypeValue", "GroupOpen", "TypeExpression", "GroupClose", "ListOpen", "ListClose"]
        );
        assert_eq!(
            Expression::DefinitionClause.expression().unwrap().references(),
            vec!["Colon", "TypeExpression"]
        );
    }

    // @lfy def/grammar/rules/expression.lfy:64
    #[test]
    fn only_negation_and_dereference_disallow_space() {
        for &rule in PREFIXES {
            let Category::Prefix { disallow_space, .. } = rule.category() else {
                panic!("{rule}");
            };
            let expected = matches!(
                rule,
                Entity::Expression(Expression::NegateOperation | Expression::Dereference)
            );
            assert_eq!(disallow_space, expected, "{rule}");
        }
    }

    // @lfy def/grammar/rules/expression.lfy:96
    #[test]
    fn postfix_rules_keep_their_operator_and_tail() {
        assert_eq!(
            Expression::Member.category(),
            Category::Postfix {
                operator: Entity::Punctuation(Punctuation::Accessor),
                tail: Some("(/ [[MemberName]] /)"),
                disallow_space: true,
            }
        );
        assert_eq!(
            Expression::Call.category().operator(),
            Some(Entity::Punctuation(Punctuation::GroupOpen))
        );
        assert_eq!(
            Expression::Conditional.binding(),
            Some(Binding::right(Level::Definition))
        );
        assert_eq!(Expression::Member.binding(), None);
        // @lfy def/grammar/rules/expression.lfy:96
        for &rule in POSTFIXES {
            let Category::Postfix { disallow_space, .. } = rule.category() else {
                panic!("{rule}");
            };
            assert_eq!(disallow_space, rule == Entity::Expression(Expression::Member), "{rule}");
        }
        // @lfy def/grammar/rules/expression.lfy:101
        assert_eq!(
            Expression::Cast.category(),
            Category::Postfix {
                operator: Entity::Keyword(Keyword::AsKeyword),
                tail: Some("[[TypeExpression]]"),
                disallow_space: false,
            }
        );
        assert_eq!(
            Expression::Cast.effective_binding(),
            Some(Binding::left(Level::Relational))
        );
        assert_eq!(
            Expression::Cast.syntax(),
            "[[Expression]] , [[AsKeyword]] , [[TypeExpression]]"
        );
    }

    // @lfy def/grammar/rules/expression.lfy:50
    #[test]
    fn a_group_cannot_match_before_a_clause_or_arrow() {
        assert!(group_cannot_match_before(Entity::Keyword(Keyword::IsKeyword)));
        assert!(group_cannot_match_before(Entity::Punctuation(Punctuation::Colon)));
        assert!(group_cannot_match_before(Entity::Punctuation(Punctuation::DoubleArrowRight)));
        assert!(!group_cannot_match_before(Entity::Punctuation(Punctuation::SingleArrowRight)));
        assert!(!group_cannot_match_before(Entity::Punctuation(Punctuation::Semicolon)));
        assert!(!group_cannot_match_before(Entity::Keyword(Keyword::AsKeyword)));
    }
}
