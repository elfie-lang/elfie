//! Compiled from `def/grammar/tokens/operator.lfy`.

use super::super::{Forbidden, RuleTrait, grammar_rules};

grammar_rules! {
    /// Punctuation rules.
    pub enum Operator {
        /// Accesses the data or value
        AccessorValuePunctuation is [RuleTrait::AppliesToCurrent] = r#"".""#, // @lfy def/grammar/tokens/operator.lfy:3
        /// Accesses the context
        AccessorContextPunctuation is [RuleTrait::AppliesToCurrent] = r#""@""#, // @lfy def/grammar/tokens/operator.lfy:4
        /// Accesses the scope
        AccessorScopePunctuation is [RuleTrait::AppliesToCurrent] = r#""$""#, // @lfy def/grammar/tokens/operator.lfy:5

        AccessorPunctuation is [RuleTrait::AppliesToCurrent] = "[[AccessorValuePunctuation]] | [[AccessorContextPunctuation]] | [[AccessorScopePunctuation]]", // @lfy def/grammar/tokens/operator.lfy:7

        /// Single arrow left
        ArrowSingleLeftPunctuation = r#""<-""#, // @lfy def/grammar/tokens/operator.lfy:9
        /// Single arrow left single character
        ArrowSingleLeftSingleCharPunctuation = r#""←""#, // @lfy def/grammar/tokens/operator.lfy:10
        /// Single arrow right
        ArrowSingleRightPunctuation = r#""->""#, // @lfy def/grammar/tokens/operator.lfy:11
        /// Single arrow right single character
        ArrowSingleRightSingleCharPunctuation = r#""→""#, // @lfy def/grammar/tokens/operator.lfy:12
        /// Double arrow right
        ArrowDoubleRightPunctuation = r#""=>""#, // @lfy def/grammar/tokens/operator.lfy:13
        /// Bitwise AND; Also used for dereferencing to the scope assigned to a variable or alias rather than the variable or alias itself
        BitwiseAndPunctuation = r#""&""#, // @lfy def/grammar/tokens/operator.lfy:14
        /// Bitwise OR
        BitwiseOrPunctuation = r#""|""#, // @lfy def/grammar/tokens/operator.lfy:15
        /// Bitwise NOT
        BitwiseNotPunctuation = r#""~""#, // @lfy def/grammar/tokens/operator.lfy:16
        /// Not a match when followed by a non-numeric character.
        /// Bitwise XOR (when followed by a number)
        BitwiseXorPunctuation is [RuleTrait::NotAllowedPostfix(&[Forbidden::NonNumericCharacter])] = r#""^""#, // @lfy def/grammar/tokens/operator.lfy:17
        /// Null coalescence
        CoalescenceNullPunctuation = r#""??""#, // @lfy def/grammar/tokens/operator.lfy:18
        /// Definition
        DefinitionPunctuation = r#"":""#, // @lfy def/grammar/tokens/operator.lfy:19
        /// Less than
        EqualityLessThanPunctuation = r#""<""#, // @lfy def/grammar/tokens/operator.lfy:20
        /// Less than or equal to
        EqualityLessThanOrEqualPunctuation = r#""<=""#, // @lfy def/grammar/tokens/operator.lfy:21
        /// Greater than
        EqualityGreaterThanPunctuation = r#"">""#, // @lfy def/grammar/tokens/operator.lfy:22
        /// Greater than or equal to
        EqualityGreaterThanOrEqualPunctuation = r#"">=""#, // @lfy def/grammar/tokens/operator.lfy:23
        /// Equality (Are these two values equal, no auto-casting)
        EqualityStrictPunctuation = r#""==""#, // @lfy def/grammar/tokens/operator.lfy:24
        /// Inequality
        EqualityUnequalPunctuation = r#""!=""#, // @lfy def/grammar/tokens/operator.lfy:25
        /// Reference equality (do these refer to the same point in memory)
        EqualityReferencePunctuation = r#""&=""#, // @lfy def/grammar/tokens/operator.lfy:26
        /// About equal (equality + auto-casting)
        EqualityWithCastingPunctuation = r#""~=""#, // @lfy def/grammar/tokens/operator.lfy:27
        /// Logical "not"
        LogicalNotPunctuation = r#""!""#, // @lfy def/grammar/tokens/operator.lfy:28
        /// Logical "and"
        LogicalAddPunctuation = r#""&&""#, // @lfy def/grammar/tokens/operator.lfy:29
        /// Logical "or"
        LogicalOrPunctuation = r#""||""#, // @lfy def/grammar/tokens/operator.lfy:30
        /// Optional (chaining, type, etc.) or inline conditional
        QuestionMarkPunctuation = r#""?""#, // @lfy def/grammar/tokens/operator.lfy:31
        /// Previous statement reference
        RefLastPunctuation = r#""^^""#, // @lfy def/grammar/tokens/operator.lfy:32
        /// Apply a definition to the previous statement
        RefDefineLastPunctuation = r#""^^:""#, // @lfy def/grammar/tokens/operator.lfy:33
        /// Setter
        SetterPlainPunctuation = r#""=""#, // @lfy def/grammar/tokens/operator.lfy:34
        /// For boolean, set variable to NOT value
        SetterBooleanTogglePunctuation = r#""=!""#, // @lfy def/grammar/tokens/operator.lfy:35
        /// Add or append and re-set variable
        SetterAdditivePunctuation = r#""=+""#, // @lfy def/grammar/tokens/operator.lfy:36
        /// Add 1 to a numeric value and re-set variable
        SetterIncrementPunctuation = r#""=++""#, // @lfy def/grammar/tokens/operator.lfy:37
        /// Subtract and re-set variable
        SetterSubtractivePunctuation = r#""=-""#, // @lfy def/grammar/tokens/operator.lfy:38
        /// Subtract 1 fom a numeric value and re-set variable
        SetterDecrementPunctuation = r#""=--""#, // @lfy def/grammar/tokens/operator.lfy:39
        /// Multiply and re-set variable
        SetterMultiplicativePunctuation = r#""=*""#, // @lfy def/grammar/tokens/operator.lfy:40
        /// To power of and re-set variable
        SetterPowerPunctuation = r#""=**""#, // @lfy def/grammar/tokens/operator.lfy:41
        /// Bitwise AND and re-set variable
        SetterBitwiseAndPunctuation = r#""=&""#, // @lfy def/grammar/tokens/operator.lfy:42
        /// Bitwise OR and re-set variable
        SetterBitwisseOrPunctuation = r#""=|""#, // @lfy def/grammar/tokens/operator.lfy:43
        /// Set if undefined
        SetterCoalescenceNullPunctuation = r#""=??""#, // @lfy def/grammar/tokens/operator.lfy:44
        /// Set if falsey
        SetterCoalescenceFalseyPunctuation = r#""=||""#, // @lfy def/grammar/tokens/operator.lfy:45
        /// Set if truthy
        SetterCoalescenceTruthyPunctuation = r#""=&&""#, // @lfy def/grammar/tokens/operator.lfy:46

        BitwisePunctuation = "[[BitwiseAndPunctuation]] | [[BitwiseOrPunctuation]] | [[BitwiseNotPunctuation]] | [[BitwiseXorPunctuation]]", // @lfy def/grammar/tokens/operator.lfy:48
        EqualityPunctuation = "[[EqualityLessThanPunctuation]] | [[EqualityLessThanOrEqualPunctuation]] | [[EqualityGreaterThanPunctuation]] | [[EqualityGreaterThanOrEqualPunctuation]] | [[EqualityStrictPunctuation]] | [[EqualityUnequalPunctuation]] | [[EqualityReferencePunctuation]] | [[EqualityWithCastingPunctuation]]", // @lfy def/grammar/tokens/operator.lfy:49
        /// All punctuation that can set a value
        SetterPuncuation = "[[SetterPlainPunctuation]] | [[SetterBooleanTogglePunctuation]] | [[SetterAdditivePunctuation]] | [[SetterIncrementPunctuation]] | [[SetterSubtractivePunctuation]] | [[SetterDecrementPunctuation]] | [[SetterMultiplicativePunctuation]] | [[SetterPowerPunctuation]] | [[SetterBitwiseAndPunctuation]] | [[SetterBitwisseOrPunctuation]] | [[SetterCoalescenceNullPunctuation]] | [[SetterCoalescenceFalseyPunctuation]] | [[SetterCoalescenceTruthyPunctuation]]", // @lfy def/grammar/tokens/operator.lfy:50

        /// Addition, concatenation
        AddPunctuation = r#""+""#, // @lfy def/grammar/tokens/operator.lfy:52
        /// Subtraction
        SubtractPunctuation = r#""-""#, // @lfy def/grammar/tokens/operator.lfy:53
        /// Multiplication
        MultiplyPunctuation = r#""*""#, // @lfy def/grammar/tokens/operator.lfy:54
        /// Power
        PowerPunctuation = r#""**""#, // @lfy def/grammar/tokens/operator.lfy:55
        /// Modulus
        ModulusPunctuation = r#""%""#, // @lfy def/grammar/tokens/operator.lfy:56
        /// Division
        DividePunctuation = r#""/""#, // @lfy def/grammar/tokens/operator.lfy:57
        /// Spread or range
        SpreadPunctuation = r#""...""#, // @lfy def/grammar/tokens/operator.lfy:58

        MathPuncuation = "[[AddPunctuation]] | [[SubtractPunctuation]] | [[MultiplyPunctuation]] | [[PowerPunctuation]] | [[ModulusPunctuation]] | [[DividePunctuation]]", // @lfy def/grammar/tokens/operator.lfy:60
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::EbnfSyntax;
    use super::*;

    // @lfy def/grammar/tokens/operator.lfy:17
    #[test]
    fn only_xor_and_the_accessors_carry_traits() {
        for &operator in Operator::ALL {
            let expected: &[RuleTrait] = match operator {
                Operator::BitwiseXorPunctuation => &[RuleTrait::NotAllowedPostfix(&[
                    Forbidden::NonNumericCharacter,
                ])],
                Operator::AccessorValuePunctuation
                | Operator::AccessorContextPunctuation
                | Operator::AccessorScopePunctuation
                | Operator::AccessorPunctuation => &[RuleTrait::AppliesToCurrent],
                _ => &[],
            };
            assert_eq!(operator.traits(), expected, "{}", operator.identifier());
        }
        assert_eq!(Operator::ALL.len(), 53);
    }

    #[test]
    fn group_rules_match_the_same_text_as_their_members() {
        assert_eq!(Operator::SetterPuncuation.longest_match("=&&x"), Some(3));
        assert_eq!(Operator::MathPuncuation.longest_match("**"), Some(2));
        assert_eq!(Operator::EqualityPunctuation.longest_match("<=>"), Some(2));
        assert_eq!(Operator::BitwisePunctuation.longest_match("^1"), Some(1));
        assert_eq!(Operator::AccessorPunctuation.longest_match("$x"), Some(1));
    }
}
