//! Compiled from `def/lexer/tokens/operator.lfy`.

use super::token_enum;
use super::traits::FollowRule;

token_enum! {
    /// Operators. Overlapping spellings are resolved by maximal munch.
    // @lfy def/lexer/tokens/operator.lfy:1
    pub enum Operator {
        AccessorValue => ("OP_ACCESSOR_VALUE", ".", ""), // @lfy def/lexer/tokens/operator.lfy:2
        AccessorContext => ("OP_ACCESSOR_CONTEXT", "@", ""), // @lfy def/lexer/tokens/operator.lfy:3
        AccessorScope => ("OP_ACCESSOR_SCOPE", "$", ""), // @lfy def/lexer/tokens/operator.lfy:4

        ArrowSingleLeft => ("OP_ARROW_SINGLE_LEFT", "<-", "Single arrow left"), // @lfy def/lexer/tokens/operator.lfy:6
        ArrowSingleLeftSingleChar => ("OP_ARROW_SINGLE_LEFT_SINGLE_CHAR", "←", "Single arrow left single character"), // @lfy def/lexer/tokens/operator.lfy:7
        ArrowSingleRight => ("OP_ARROW_SINGLE_RIGHT", "->", "Single arrow right"), // @lfy def/lexer/tokens/operator.lfy:8
        ArrowSingleRightSingleChar => ("OP_ARROW_SINGLE_RIGHT_SINGLE_CHAR", "→", "Single arrow right single character"), // @lfy def/lexer/tokens/operator.lfy:9
        ArrowDoubleRight => ("OP_ARROW_DOUBLE_RIGHT", "=>", "Double arrow right"), // @lfy def/lexer/tokens/operator.lfy:10
        BitwiseAnd => ("OP_BITWISE_AND", "&", "Bitwise AND"), // @lfy def/lexer/tokens/operator.lfy:11
        BitwiseOr => ("OP_BITWISE_OR", "|", "Bitwise OR"), // @lfy def/lexer/tokens/operator.lfy:12
        BitwiseNot => ("OP_BITWISE_NOT", "~", "Bitwise NOT"), // @lfy def/lexer/tokens/operator.lfy:13
        BitwiseXor => ("OP_BITWISE_XOR", "^", "Bitwise XOR (when followed by a number)"), // @lfy def/lexer/tokens/operator.lfy:14
        CoalescenceNull => ("OP_COALESCENCE_NULL", "??", "Null coalescence"), // @lfy def/lexer/tokens/operator.lfy:15
        Definition => ("OP_DEFINITION", ":", "Definition"), // @lfy def/lexer/tokens/operator.lfy:16
        EqualityLessThan => ("OP_EQUALITY_LESS_THAN", "<", "Less than"), // @lfy def/lexer/tokens/operator.lfy:17
        EqualityLessThanOrEqual => ("OP_EQUALITY_LESS_THAN_OR_EQUAL", "<=", "Less than or equal to"), // @lfy def/lexer/tokens/operator.lfy:18
        EqualityGreaterThan => ("OP_EQUALITY_GREATER_THAN", ">", "Greater than"), // @lfy def/lexer/tokens/operator.lfy:19
        EqualityGreaterThanOrEqual => ("OP_EQUALITY_GREATER_THAN_OR_EQUAL", ">=", "Greater than or equal to"), // @lfy def/lexer/tokens/operator.lfy:20
        EqualityStrict => ("OP_EQUALITY_STRICT", "==", "Equality (Are these two values equal, no auto-casting)"), // @lfy def/lexer/tokens/operator.lfy:21
        EqualityUnequal => ("OP_EQUALITY_UNEQUAL", "!=", "Inequality"), // @lfy def/lexer/tokens/operator.lfy:22
        EqualityReference => ("OP_EQUALITY_REFERENCE", "&=", "Reference equality (do these refer to the same point in memory)"), // @lfy def/lexer/tokens/operator.lfy:23
        EqualityWithCasting => ("OP_EQUALITY_WITH_CASTING", "~=", "About equal (equality + auto-casting)"), // @lfy def/lexer/tokens/operator.lfy:24
        LogicalNot => ("OP_LOGICAL_NOT", "!", "Logical Not"), // @lfy def/lexer/tokens/operator.lfy:25
        LogicalAdd => ("OP_LOGICAL_ADD", "&&", "And"), // @lfy def/lexer/tokens/operator.lfy:26
        LogicalOr => ("OP_LOGICAL_OR", "||", "Or"), // @lfy def/lexer/tokens/operator.lfy:27
        QuestionMark => ("OP_QUESTION_MARK", "?", "optional (chaining, type, etc.) or Inline conditional"), // @lfy def/lexer/tokens/operator.lfy:28
        RefLast => ("OP_REF_LAST", "^^", "Previous statement reference"), // @lfy def/lexer/tokens/operator.lfy:29
        RefDefineLast => ("OP_REF_DEFINE_LAST", "^^:", "Apply a definition to the previous statement"), // @lfy def/lexer/tokens/operator.lfy:30
        Setter => ("OP_SETTER", "=", "Setter"), // @lfy def/lexer/tokens/operator.lfy:31
        SetterBooleanToggle => ("OP_SETTER_BOOLEAN_TOGGLE", "=!", "For boolean, set variable to NOT value"), // @lfy def/lexer/tokens/operator.lfy:32
        SetterAdditive => ("OP_SETTER_ADDITIVE", "=+", "Add or append and re-set variable"), // @lfy def/lexer/tokens/operator.lfy:33
        SetterIncrement => ("OP_SETTER_INCREMENT", "=++", "Add 1 to a numeric value and re-set variable"), // @lfy def/lexer/tokens/operator.lfy:34
        SetterSubtractive => ("OP_SETTER_SUBTRACTIVE", "=-", "Subtract and re-set variable"), // @lfy def/lexer/tokens/operator.lfy:35
        SetterDecrement => ("OP_SETTER_DECREMENT", "=--", "Subtract 1 fom a numeric value and re-set variable"), // @lfy def/lexer/tokens/operator.lfy:36
        SetterMultiplicative => ("OP_SETTER_MULTIPLICATIVE", "=*", "Multiply and re-set variable"), // @lfy def/lexer/tokens/operator.lfy:37
        SetterPower => ("OP_SETTER_POWER", "=**", "To power of and re-set variable"), // @lfy def/lexer/tokens/operator.lfy:38
        SetterBitwiseAnd => ("OP_SETTER_BITWISE_AND", "=&", "Bitwise AND and re-set variable"), // @lfy def/lexer/tokens/operator.lfy:39
        SetterBitwisseOr => ("OP_SETTER_BITWISSE_OR", "=|", "Bitwise OR and re-set variable"), // @lfy def/lexer/tokens/operator.lfy:40
        SetterCoalescenceNull => ("OP_SETTER_COALESCENCE_NULL", "=??", "Set if undefined"), // @lfy def/lexer/tokens/operator.lfy:41
        SetterCoalescenceFalsey => ("OP_SETTER_COALESCENCE_FALSEY", "=||", "Set if falsey"), // @lfy def/lexer/tokens/operator.lfy:42
        SetterCoalescenceTruthy => ("OP_SETTER_COALESCENCE_TRUTHY", "=&&", "Set if truthy"), // @lfy def/lexer/tokens/operator.lfy:43

        Add => ("OP_ADD", "+", "Addition, concatenation"), // @lfy def/lexer/tokens/operator.lfy:45
        Subtract => ("OP_SUBTRACT", "-", "Subtraction"), // @lfy def/lexer/tokens/operator.lfy:46
        Multiply => ("OP_MULTIPLY", "*", "Multiplication"), // @lfy def/lexer/tokens/operator.lfy:47
        Power => ("OP_POWER", "**", "Power"), // @lfy def/lexer/tokens/operator.lfy:48
        Modulus => ("OP_MODULUS", "%", "Modulus"), // @lfy def/lexer/tokens/operator.lfy:49
        Divide => ("OP_DIVIDE", "/", "Division"), // @lfy def/lexer/tokens/operator.lfy:50
        Spread => ("OP_SPREAD", "...", "Spread"), // @lfy def/lexer/tokens/operator.lfy:51
    }
}

/// `cantBeFollowedBy([`a non-numeric character`])` on `OP_BITWISE_XOR`.
// @lfy def/lexer/tokens/operator.lfy:14
const XOR_CANT_BE_FOLLOWED_BY: &[FollowRule] = &[FollowRule::NonNumericCharacter];

impl Operator {
    /// The `cantBeFollowedBy` rules attached to this operator.
    pub fn cant_be_followed_by(self) -> &'static [FollowRule] {
        match self {
            Operator::BitwiseXor => XOR_CANT_BE_FOLLOWED_BY, // @lfy def/lexer/tokens/operator.lfy:14
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/lexer/tokens/operator.lfy:1
    #[test]
    fn operator_values_are_unique_and_round_trip() {
        for &operator in Operator::ALL {
            assert_eq!(Operator::from_value(operator.value()), Some(operator));
        }
        assert_eq!(Operator::ALL.len(), 48);
    }

    // @lfy def/lexer/tokens/operator.lfy:14
    #[test]
    fn only_xor_carries_a_follow_rule() {
        for &operator in Operator::ALL {
            let expected: &[FollowRule] = if operator == Operator::BitwiseXor {
                &[FollowRule::NonNumericCharacter]
            } else {
                &[]
            };
            assert_eq!(
                operator.cant_be_followed_by(),
                expected,
                "{}",
                operator.key()
            );
        }
    }
}
