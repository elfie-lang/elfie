//! Compiled from `def/grammar/terminals/punctuation.lfy`.

use super::super::traits::category::*;
use super::super::{Binding, Entity, GrammarRule, Level, grammar_rules};

grammar_rules! {
    /// Punctuation with specific meaning, and the alternation lists that group it.
    pub enum Punctuation {
        // Accessors: one per layer
        ValueAccessor is [punctuation("."), Binding::left(Level::Access)]: "Reaches into the value layer" = r#"".""#, // @lfy def/grammar/terminals/punctuation.lfy:ValueAccessor
        OptionalValueAccessor is [punctuation("?."), Binding::left(Level::Access)]: "Reaches into the value layer when the value is present" = r#""?.""#, // @lfy def/grammar/terminals/punctuation.lfy:OptionalValueAccessor
        ContextAccessor is [punctuation("@"), Binding::left(Level::Access)]: "Reaches into the context layer" = r#""@""#, // @lfy def/grammar/terminals/punctuation.lfy:ContextAccessor
        ScopeAccessor is [punctuation("$"), Binding::left(Level::Access)]: "Reaches into the scope layer" = r#""$""#, // @lfy def/grammar/terminals/punctuation.lfy:ScopeAccessor
        ParentScopeAccessor is [punctuation("$&"), Binding::left(Level::Access)]: "Reaches into the enclosing scope" = r#""$&""#, // @lfy def/grammar/terminals/punctuation.lfy:ParentScopeAccessor
        Accessor is [alternation_list(ACCESSORS)]: "Any accessor" = "[[ValueAccessor]] | [[OptionalValueAccessor]] | [[ContextAccessor]] | [[ScopeAccessor]] | [[ParentScopeAccessor]]", // @lfy def/grammar/terminals/punctuation.lfy:Accessor

        // Grouping and separation
        GroupOpen is [punctuation("("), Binding::left(Level::Group)]: "Opens a group or an argument list" = r#""(""#, // @lfy def/grammar/terminals/punctuation.lfy:GroupOpen
        GroupClose is [punctuation(")")]: "Closes a group or an argument list" = r#"")""#, // @lfy def/grammar/terminals/punctuation.lfy:GroupClose
        ListOpen is [punctuation("["), Binding::left(Level::Access)]: "Opens a list, an index, or an array type" = r#""[""#, // @lfy def/grammar/terminals/punctuation.lfy:ListOpen
        ListClose is [punctuation("]")]: "Closes a list, an index, or an array type" = r#""]""#, // @lfy def/grammar/terminals/punctuation.lfy:ListClose
        BlockOpen is [punctuation("{")]: "Opens a block or an object" = r#""{""#, // @lfy def/grammar/terminals/punctuation.lfy:BlockOpen
        BlockClose is [punctuation("}")]: "Closes a block or an object" = r#""}""#, // @lfy def/grammar/terminals/punctuation.lfy:BlockClose
        Comma is [punctuation(","), Binding::left(Level::Joiner)]: "Separates items" = r#"",""#, // @lfy def/grammar/terminals/punctuation.lfy:Comma
        Semicolon is [punctuation(";")]: "Ends a statement" = r#"";""#, // @lfy def/grammar/terminals/punctuation.lfy:Semicolon
        Colon is [punctuation(":"), Binding::left(Level::Definition)]: "Attaches a definition to a name" = r#"":""#, // @lfy def/grammar/terminals/punctuation.lfy:Colon
        QuestionMark is [punctuation("?")]: "Marks a name optional, or begins a conditional" = r#""?""#, // @lfy def/grammar/terminals/punctuation.lfy:QuestionMark
        Spread is [punctuation("..."), Binding::right(Level::Definition)]: "Spreads a list, collects the rest, or makes a range" = r#""...""#, // @lfy def/grammar/terminals/punctuation.lfy:Spread
        PreviousStatement is [punctuation("^^"), Binding::right(Level::Reference)]: "The entity the previous statement declared" = r#""^^""#, // @lfy def/grammar/terminals/punctuation.lfy:PreviousStatement
        Ampersand is [punctuation("&"), Binding::right(Level::Reference)]: "The entity a name is bound to rather than its value; bitwise and between values" = r#""&""#, // @lfy def/grammar/terminals/punctuation.lfy:Ampersand

        // Arrows
        SingleArrowRight is [punctuation("->")]: "Leads to a consequence or a return type" = r#""->""#, // @lfy def/grammar/terminals/punctuation.lfy:SingleArrowRight
        SingleArrowRightGlyph is [punctuation("→")]: "The one character spelling of the single right arrow" = r#""→""#, // @lfy def/grammar/terminals/punctuation.lfy:SingleArrowRightGlyph
        SingleArrowLeft is [punctuation("<-")]: "Reserved arrow" = r#""<-""#, // @lfy def/grammar/terminals/punctuation.lfy:SingleArrowLeft
        SingleArrowLeftGlyph is [punctuation("←")]: "The one character spelling of the single left arrow" = r#""←""#, // @lfy def/grammar/terminals/punctuation.lfy:SingleArrowLeftGlyph
        DoubleArrowRight is [punctuation("=>")]: "Leads to a result" = r#""=>""#, // @lfy def/grammar/terminals/punctuation.lfy:DoubleArrowRight
        SingleArrow is [alternation_list(SINGLE_ARROWS)]: "Either spelling of the single right arrow" = "[[SingleArrowRight]] | [[SingleArrowRightGlyph]]", // @lfy def/grammar/terminals/punctuation.lfy:SingleArrow

        // Setters
        PlainSetter is [setter("="), Binding::right(Level::Assignment)]: "Sets" = r#""=""#, // @lfy def/grammar/terminals/punctuation.lfy:PlainSetter
        ToggleSetter is [setter("=!"), Binding::right(Level::Assignment)]: "Sets a boolean to its opposite" = r#""=!""#, // @lfy def/grammar/terminals/punctuation.lfy:ToggleSetter
        AddSetter is [setter("=+"), Binding::right(Level::Assignment)]: "Adds, then sets" = r#""=+""#, // @lfy def/grammar/terminals/punctuation.lfy:AddSetter
        SubtractSetter is [setter("=-"), Binding::right(Level::Assignment)]: "Subtracts, then sets" = r#""=-""#, // @lfy def/grammar/terminals/punctuation.lfy:SubtractSetter
        MultiplySetter is [setter("=*"), Binding::right(Level::Assignment)]: "Multiplies, then sets" = r#""=*""#, // @lfy def/grammar/terminals/punctuation.lfy:MultiplySetter
        PowerSetter is [setter("=**"), Binding::right(Level::Assignment)]: "Raises to a power, then sets" = r#""=**""#, // @lfy def/grammar/terminals/punctuation.lfy:PowerSetter
        BitwiseAndSetter is [setter("=&"), Binding::right(Level::Assignment)]: "Bitwise ands, then sets" = r#""=&""#, // @lfy def/grammar/terminals/punctuation.lfy:BitwiseAndSetter
        BitwiseOrSetter is [setter("=|"), Binding::right(Level::Assignment)]: "Bitwise ors, then sets" = r#""=|""#, // @lfy def/grammar/terminals/punctuation.lfy:BitwiseOrSetter
        NullishSetter is [setter("=??"), Binding::right(Level::Assignment)]: "Sets only when null or undefined" = r#""=??""#, // @lfy def/grammar/terminals/punctuation.lfy:NullishSetter
        FalseySetter is [setter("=||"), Binding::right(Level::Assignment)]: "Sets only when falsey" = r#""=||""#, // @lfy def/grammar/terminals/punctuation.lfy:FalseySetter
        TruthySetter is [setter("=&&"), Binding::right(Level::Assignment)]: "Sets only when truthy" = r#""=&&""#, // @lfy def/grammar/terminals/punctuation.lfy:TruthySetter
        /// `alternationList(...setter@entities)`
        Setter is [alternation_list(SETTERS)]: "Any setter" = "[[PlainSetter]] | [[ToggleSetter]] | [[AddSetter]] | [[SubtractSetter]] | [[MultiplySetter]] | [[PowerSetter]] | [[BitwiseAndSetter]] | [[BitwiseOrSetter]] | [[NullishSetter]] | [[FalseySetter]] | [[TruthySetter]]", // @lfy def/grammar/terminals/punctuation.lfy:Setter

        // Comparison
        LessThan is [relational("<"), Binding::left(Level::Relational)]: "Less than" = r#""<""#, // @lfy def/grammar/terminals/punctuation.lfy:LessThan
        LessThanOrEqual is [relational("<="), Binding::left(Level::Relational)]: "Less than or equal" = r#""<=""#, // @lfy def/grammar/terminals/punctuation.lfy:LessThanOrEqual
        GreaterThan is [relational(">"), Binding::left(Level::Relational)]: "Greater than" = r#"">""#, // @lfy def/grammar/terminals/punctuation.lfy:GreaterThan
        GreaterThanOrEqual is [relational(">="), Binding::left(Level::Relational)]: "Greater than or equal" = r#"">=""#, // @lfy def/grammar/terminals/punctuation.lfy:GreaterThanOrEqual
        /// `alternationList(...relational@entities)`
        Relational is [alternation_list(RELATIONALS)]: "Any ordering operator" = "[[LessThan]] | [[LessThanOrEqual]] | [[GreaterThan]] | [[GreaterThanOrEqual]]", // @lfy def/grammar/terminals/punctuation.lfy:Relational

        Equal is [equality("=="), Binding::left(Level::Equality)]: "Equal without casting" = r#""==""#, // @lfy def/grammar/terminals/punctuation.lfy:Equal
        NotEqual is [equality("!="), Binding::left(Level::Equality)]: "Not equal" = r#""!=""#, // @lfy def/grammar/terminals/punctuation.lfy:NotEqual
        SameReference is [equality("&="), Binding::left(Level::Equality)]: "The same entity" = r#""&=""#, // @lfy def/grammar/terminals/punctuation.lfy:SameReference
        AboutEqual is [equality("~="), Binding::left(Level::Equality)]: "Equal after casting" = r#""~=""#, // @lfy def/grammar/terminals/punctuation.lfy:AboutEqual
        /// `alternationList(...equality@entities)`
        Equality is [alternation_list(EQUALITIES)]: "Any equality operator" = "[[Equal]] | [[NotEqual]] | [[SameReference]] | [[AboutEqual]]", // @lfy def/grammar/terminals/punctuation.lfy:Equality

        // Logic
        LogicalAnd is [punctuation("&&"), Binding::left(Level::LogicalAnd)]: "Both" = r#""&&""#, // @lfy def/grammar/terminals/punctuation.lfy:LogicalAnd
        LogicalOr is [punctuation("||"), Binding::left(Level::Coalescence)]: "Either, yielding the first truthy value" = r#""||""#, // @lfy def/grammar/terminals/punctuation.lfy:LogicalOr
        NullishOr is [punctuation("??"), Binding::left(Level::Coalescence)]: "The right value when the left is null or undefined" = r#""??""#, // @lfy def/grammar/terminals/punctuation.lfy:NullishOr
        Coalescence is [alternation_list(COALESCENCES)]: "Operators that fall back to the right value" = "[[LogicalOr]] | [[NullishOr]]", // @lfy def/grammar/terminals/punctuation.lfy:Coalescence
        LogicalNot is [punctuation("!"), Binding::right(Level::Unary)]: "Not" = r#""!""#, // @lfy def/grammar/terminals/punctuation.lfy:LogicalNot

        // Arithmetic and bitwise
        Plus is [additive("+"), Binding::left(Level::Additive)]: "Addition or concatenation" = r#""+""#, // @lfy def/grammar/terminals/punctuation.lfy:Plus
        Minus is [additive("-"), Binding::left(Level::Additive)]: "Subtraction; also negation as a prefix" = r#""-""#, // @lfy def/grammar/terminals/punctuation.lfy:Minus
        /// `alternationList(...additive@entities)`
        Additive is [alternation_list(ADDITIVES)]: "Any additive operator" = "[[Plus]] | [[Minus]]", // @lfy def/grammar/terminals/punctuation.lfy:Additive

        Star is [multiplicative("*"), Binding::left(Level::Multiplicative)]: "Multiplication" = r#""*""#, // @lfy def/grammar/terminals/punctuation.lfy:Star
        Slash is [multiplicative("/"), Binding::left(Level::Multiplicative)]: "Division" = r#""/""#, // @lfy def/grammar/terminals/punctuation.lfy:Slash
        Percent is [multiplicative("%"), Binding::left(Level::Multiplicative)]: "Remainder" = r#""%""#, // @lfy def/grammar/terminals/punctuation.lfy:Percent
        /// `alternationList(...multiplicative@entities)`
        Multiplicative is [alternation_list(MULTIPLICATIVES)]: "Any multiplicative operator" = "[[Star]] | [[Slash]] | [[Percent]]", // @lfy def/grammar/terminals/punctuation.lfy:Multiplicative

        Power is [punctuation("**"), Binding::right(Level::Exponentiation)]: "Exponentiation" = r#""**""#, // @lfy def/grammar/terminals/punctuation.lfy:Power
        BitwiseOr is [punctuation("|"), Binding::left(Level::BitwiseOr)]: "Bitwise or; also a union type" = r#""|""#, // @lfy def/grammar/terminals/punctuation.lfy:BitwiseOr
        BitwiseXor is [punctuation("^"), Binding::left(Level::BitwiseXor)]: "Bitwise exclusive or" = r#""^""#, // @lfy def/grammar/terminals/punctuation.lfy:BitwiseXor
        BitwiseNot is [punctuation("~"), Binding::right(Level::Unary)]: "Bitwise not" = r#""~""#, // @lfy def/grammar/terminals/punctuation.lfy:BitwiseNot

        /// `alternationList(...punctuation@entities)`
        Punctuation is [alternation_list(PUNCTUATIONS)]: "Any punctuation" = "[[ValueAccessor]] | [[OptionalValueAccessor]] | [[ContextAccessor]] | [[ScopeAccessor]] | [[ParentScopeAccessor]] | [[GroupOpen]] | [[GroupClose]] | [[ListOpen]] | [[ListClose]] | [[BlockOpen]] | [[BlockClose]] | [[Comma]] | [[Semicolon]] | [[Colon]] | [[QuestionMark]] | [[Spread]] | [[PreviousStatement]] | [[Ampersand]] | [[SingleArrowRight]] | [[SingleArrowRightGlyph]] | [[SingleArrowLeft]] | [[SingleArrowLeftGlyph]] | [[DoubleArrowRight]] | [[PlainSetter]] | [[ToggleSetter]] | [[AddSetter]] | [[SubtractSetter]] | [[MultiplySetter]] | [[PowerSetter]] | [[BitwiseAndSetter]] | [[BitwiseOrSetter]] | [[NullishSetter]] | [[FalseySetter]] | [[TruthySetter]] | [[LessThan]] | [[LessThanOrEqual]] | [[GreaterThan]] | [[GreaterThanOrEqual]] | [[Equal]] | [[NotEqual]] | [[SameReference]] | [[AboutEqual]] | [[LogicalAnd]] | [[LogicalOr]] | [[NullishOr]] | [[LogicalNot]] | [[Plus]] | [[Minus]] | [[Star]] | [[Slash]] | [[Percent]] | [[Power]] | [[BitwiseOr]] | [[BitwiseXor]] | [[BitwiseNot]]", // @lfy def/grammar/terminals/punctuation.lfy:Punctuation
    }
}

// @lfy def/grammar/terminals/punctuation.lfy:Accessor
pub const ACCESSORS: &[Entity] = &[
    Entity::Punctuation(Punctuation::ValueAccessor),
    Entity::Punctuation(Punctuation::OptionalValueAccessor),
    Entity::Punctuation(Punctuation::ContextAccessor),
    Entity::Punctuation(Punctuation::ScopeAccessor),
    Entity::Punctuation(Punctuation::ParentScopeAccessor),
];
// @lfy def/grammar/terminals/punctuation.lfy:SingleArrow
pub const SINGLE_ARROWS: &[Entity] = &[
    Entity::Punctuation(Punctuation::SingleArrowRight),
    Entity::Punctuation(Punctuation::SingleArrowRightGlyph),
];
/// `setter@entities`
// @lfy def/grammar/terminals/punctuation.lfy:Setter
pub const SETTERS: &[Entity] = &[
    Entity::Punctuation(Punctuation::PlainSetter),
    Entity::Punctuation(Punctuation::ToggleSetter),
    Entity::Punctuation(Punctuation::AddSetter),
    Entity::Punctuation(Punctuation::SubtractSetter),
    Entity::Punctuation(Punctuation::MultiplySetter),
    Entity::Punctuation(Punctuation::PowerSetter),
    Entity::Punctuation(Punctuation::BitwiseAndSetter),
    Entity::Punctuation(Punctuation::BitwiseOrSetter),
    Entity::Punctuation(Punctuation::NullishSetter),
    Entity::Punctuation(Punctuation::FalseySetter),
    Entity::Punctuation(Punctuation::TruthySetter),
];
/// `relational@entities`
// @lfy def/grammar/terminals/punctuation.lfy:Relational
pub const RELATIONALS: &[Entity] = &[
    Entity::Punctuation(Punctuation::LessThan),
    Entity::Punctuation(Punctuation::LessThanOrEqual),
    Entity::Punctuation(Punctuation::GreaterThan),
    Entity::Punctuation(Punctuation::GreaterThanOrEqual),
];
/// `equality@entities`
// @lfy def/grammar/terminals/punctuation.lfy:Equality
pub const EQUALITIES: &[Entity] = &[
    Entity::Punctuation(Punctuation::Equal),
    Entity::Punctuation(Punctuation::NotEqual),
    Entity::Punctuation(Punctuation::SameReference),
    Entity::Punctuation(Punctuation::AboutEqual),
];
// @lfy def/grammar/terminals/punctuation.lfy:Coalescence
pub const COALESCENCES: &[Entity] = &[
    Entity::Punctuation(Punctuation::LogicalOr),
    Entity::Punctuation(Punctuation::NullishOr),
];
/// `additive@entities`
// @lfy def/grammar/terminals/punctuation.lfy:Additive
pub const ADDITIVES: &[Entity] = &[
    Entity::Punctuation(Punctuation::Plus),
    Entity::Punctuation(Punctuation::Minus),
];
/// `multiplicative@entities`
// @lfy def/grammar/terminals/punctuation.lfy:Multiplicative
pub const MULTIPLICATIVES: &[Entity] = &[
    Entity::Punctuation(Punctuation::Star),
    Entity::Punctuation(Punctuation::Slash),
    Entity::Punctuation(Punctuation::Percent),
];
/// `punctuation@entities`: every rule with the `punctuation` trait or one extending it,
/// in declaration order.
// @lfy def/grammar/terminals/punctuation.lfy:Punctuation
pub const PUNCTUATIONS: &[Entity] = &[
    Entity::Punctuation(Punctuation::ValueAccessor),
    Entity::Punctuation(Punctuation::OptionalValueAccessor),
    Entity::Punctuation(Punctuation::ContextAccessor),
    Entity::Punctuation(Punctuation::ScopeAccessor),
    Entity::Punctuation(Punctuation::ParentScopeAccessor),
    Entity::Punctuation(Punctuation::GroupOpen),
    Entity::Punctuation(Punctuation::GroupClose),
    Entity::Punctuation(Punctuation::ListOpen),
    Entity::Punctuation(Punctuation::ListClose),
    Entity::Punctuation(Punctuation::BlockOpen),
    Entity::Punctuation(Punctuation::BlockClose),
    Entity::Punctuation(Punctuation::Comma),
    Entity::Punctuation(Punctuation::Semicolon),
    Entity::Punctuation(Punctuation::Colon),
    Entity::Punctuation(Punctuation::QuestionMark),
    Entity::Punctuation(Punctuation::Spread),
    Entity::Punctuation(Punctuation::PreviousStatement),
    Entity::Punctuation(Punctuation::Ampersand),
    Entity::Punctuation(Punctuation::SingleArrowRight),
    Entity::Punctuation(Punctuation::SingleArrowRightGlyph),
    Entity::Punctuation(Punctuation::SingleArrowLeft),
    Entity::Punctuation(Punctuation::SingleArrowLeftGlyph),
    Entity::Punctuation(Punctuation::DoubleArrowRight),
    Entity::Punctuation(Punctuation::PlainSetter),
    Entity::Punctuation(Punctuation::ToggleSetter),
    Entity::Punctuation(Punctuation::AddSetter),
    Entity::Punctuation(Punctuation::SubtractSetter),
    Entity::Punctuation(Punctuation::MultiplySetter),
    Entity::Punctuation(Punctuation::PowerSetter),
    Entity::Punctuation(Punctuation::BitwiseAndSetter),
    Entity::Punctuation(Punctuation::BitwiseOrSetter),
    Entity::Punctuation(Punctuation::NullishSetter),
    Entity::Punctuation(Punctuation::FalseySetter),
    Entity::Punctuation(Punctuation::TruthySetter),
    Entity::Punctuation(Punctuation::LessThan),
    Entity::Punctuation(Punctuation::LessThanOrEqual),
    Entity::Punctuation(Punctuation::GreaterThan),
    Entity::Punctuation(Punctuation::GreaterThanOrEqual),
    Entity::Punctuation(Punctuation::Equal),
    Entity::Punctuation(Punctuation::NotEqual),
    Entity::Punctuation(Punctuation::SameReference),
    Entity::Punctuation(Punctuation::AboutEqual),
    Entity::Punctuation(Punctuation::LogicalAnd),
    Entity::Punctuation(Punctuation::LogicalOr),
    Entity::Punctuation(Punctuation::NullishOr),
    Entity::Punctuation(Punctuation::LogicalNot),
    Entity::Punctuation(Punctuation::Plus),
    Entity::Punctuation(Punctuation::Minus),
    Entity::Punctuation(Punctuation::Star),
    Entity::Punctuation(Punctuation::Slash),
    Entity::Punctuation(Punctuation::Percent),
    Entity::Punctuation(Punctuation::Power),
    Entity::Punctuation(Punctuation::BitwiseOr),
    Entity::Punctuation(Punctuation::BitwiseXor),
    Entity::Punctuation(Punctuation::BitwiseNot),
];

impl Punctuation {
    /// The symbol of a punctuation rule; `None` for the alternation lists.
    pub fn symbol(self) -> Option<&'static str> {
        match self.category() {
            super::super::Category::Terminal(terminal) => terminal.fixed_text(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{Category, PunctuationClass, Terminal};
    use super::*;

    // @lfy def/grammar/terminals/punctuation.lfy:Punctuation
    #[test]
    fn every_punctuation_is_listed_once_with_its_quoted_text() {
        assert_eq!(PUNCTUATIONS.len(), 55);
        assert_eq!(Punctuation::ALL.len(), 64);
        let listed: Vec<Punctuation> = Punctuation::ALL
            .iter()
            .copied()
            .filter(|rule| rule.is_punctuation())
            .collect();
        assert_eq!(
            listed.iter().map(|rule| rule.entity()).collect::<Vec<_>>(),
            PUNCTUATIONS
        );
        for rule in listed {
            let text = rule.symbol().unwrap();
            assert_eq!(
                rule.syntax(),
                format!("\"{text}\""),
                "{}",
                rule.identifier()
            );
            assert_eq!(rule.longest_match(text), Some(text.len()));
        }
        assert_eq!(Punctuation::Punctuation.symbol(), None);
    }

    // @lfy def/grammar/terminals/punctuation.lfy:Setter
    #[test]
    fn the_classed_groups_list_exactly_their_class() {
        let groups: [(&[Entity], PunctuationClass); 5] = [
            (SETTERS, PunctuationClass::Setter),
            (RELATIONALS, PunctuationClass::Relational),
            (EQUALITIES, PunctuationClass::Equality),
            (ADDITIVES, PunctuationClass::Additive),
            (MULTIPLICATIVES, PunctuationClass::Multiplicative),
        ];
        for (group, class) in groups {
            let of_class: Vec<Entity> = PUNCTUATIONS
                .iter()
                .copied()
                .filter(|rule| {
                    matches!(
                        rule.category(),
                        Category::Terminal(Terminal::Punctuation { class: Some(c), .. }) if c == class
                    )
                })
                .collect();
            assert_eq!(of_class, group, "{class:?}");
            for &rule in group {
                assert_eq!(rule.binding(), Some(class.binding()), "{rule}");
            }
        }
    }

    // @lfy def/grammar/terminals/punctuation.lfy:ValueAccessor
    #[test]
    fn bindings_follow_the_declarations() {
        assert_eq!(Punctuation::GroupClose.binding(), None);
        assert_eq!(Punctuation::BlockOpen.binding(), None);
        assert_eq!(Punctuation::QuestionMark.binding(), None);
        assert_eq!(
            Punctuation::GroupOpen.binding(),
            Some(Binding::left(Level::Group))
        );
        assert_eq!(
            Punctuation::Spread.binding(),
            Some(Binding::right(Level::Definition))
        );
        assert_eq!(
            Punctuation::Power.binding(),
            Some(Binding::right(Level::Exponentiation))
        );
        assert_eq!(
            Punctuation::LogicalNot.binding(),
            Some(Binding::right(Level::Unary))
        );
        assert_eq!(
            Punctuation::NullishOr.binding(),
            Some(Binding::left(Level::Coalescence))
        );
        assert_eq!(Punctuation::Accessor.binding(), None);
        assert_eq!(Punctuation::Accessor.category().operator_binding(), None);
    }

    #[test]
    fn group_rules_match_the_same_text_as_their_members() {
        assert_eq!(Punctuation::Setter.longest_match("=&&x"), Some(3));
        assert_eq!(Punctuation::Multiplicative.longest_match("**"), Some(1));
        assert_eq!(Punctuation::Relational.longest_match("<=>"), Some(2));
        assert_eq!(Punctuation::Accessor.longest_match("$&x"), Some(2));
        assert_eq!(Punctuation::SingleArrow.longest_match("→"), Some(3));
        assert_eq!(Punctuation::Coalescence.longest_match("??"), Some(2));
    }
}
