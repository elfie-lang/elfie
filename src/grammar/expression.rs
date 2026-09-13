//! Compiled from `def/grammar/expression.lfy`.

use super::grammar_rules;

grammar_rules! {
    /// Expression rules.
    pub enum Expression {
        ArrowSingleRightExpression = "[[ArrowSingleRightPunctuation]] | [[ArrowSingleRightSingleCharPunctuation]]", // @lfy def/grammar/expression.lfy:8
        ArrowSingleLeftExpression = "[[ArrowSingleLeftPunctuation]] | [[ArrowSingleLeftSingleCharPunctuation]]", // @lfy def/grammar/expression.lfy:10
        /// template literal reference block
        TemplateLiteralReference = "[[ReferenceOpenBoundary]] , [[IdentifierExpression]] , [[ReferenceCloseBoundary]]", // @lfy def/grammar/expression.lfy:12
        /// template literal execution block
        TemplateLiteralExecution = "[[ExecutionOpenBoundary]] , [[Expression]] , [[ExecutionCloseBoundary]]", // @lfy def/grammar/expression.lfy:14
        /// template literal body contents
        TemplateLiteralBody = r#"(: ( UTF8_CHARACTER - ( [[BacktickBoundary]] | "\" | [[ReferenceOpenBoundary]] | [[ExecutionOpenBoundary]] ) ) | [[Sequence]] :)"#, // @lfy def/grammar/expression.lfy:16
        TemplateLiteral = "[[BacktickBoundary]] , (: [[TemplateLiteralBody]] | [[TemplateLiteralReference]] | [[TemplateLiteralExecution]] :) , [[BacktickBoundary]]", // @lfy def/grammar/expression.lfy:18
        LiteralExpression = "[[StringLiteral]] | [[TemplateLiteral]] | [[NumberLiteral]] | [[BooleanLiteral]] | [[NullLiteral]] | [[UndefinedLiteral]] | [[Identifier]]", // @lfy def/grammar/expression.lfy:20
        IdentifierAccessorExpression = "[[AccessorPunctuation]] , (/ (/ [[QuestionMarkPunctuation]] /) , [[Identifier]] /)", // @lfy def/grammar/expression.lfy:22
        IdentifierReferenceExpression = "[[BitwiseAndPunctuation]] , [[Identifier]]", // @lfy def/grammar/expression.lfy:24
        ListAccessExpression = "[[Expression]] , [[ListOpenSeparator]] , ([[Expression]]) , [[ListCloseSeparator]]", // @lfy def/grammar/expression.lfy:26
        GroupedExpression = "[[GroupOpenSeparator]] , [[Expression]] , [[GroupCloseSeparator]]", // @lfy def/grammar/expression.lfy:28
        NegationExpression = "[[LogicalNotPunctuation]] , [[Expression]]", // @lfy def/grammar/expression.lfy:30
        InlineConditionalExpression = "[[Expression]] , [[QuestionMarkPunctuation]] , [[Expression]] , [[DefinitionPunctuation]] , [[Expression]]", // @lfy def/grammar/expression.lfy:32
        CoalescenceExpression = "[[Expression]] , ( [[CoalescenceNullPunctuation]] | [[LogicalOrPunctuation]] ) , [[Expression]]", // @lfy def/grammar/expression.lfy:34
        ArithmaticExpression = "[[Expression]] , ( [[MathPuncuation]] | [[BitwisePunctuation]] ) , [[Expression]]", // @lfy def/grammar/expression.lfy:36
        ComparisonExpression = "[[Expression]] , [[EqualityPunctuation]] , [[Expression]]", // @lfy def/grammar/expression.lfy:38
        LogicalExpression = "[[Expression]] , ( [[LogicalAddPunctuation]] | [[LogicalOrPunctuation]] ) , [[Expression]]", // @lfy def/grammar/expression.lfy:40
        IdentifierExpression = "[[Expression]] , (: [[IdentifierAccessorExpression]] | [[ListAccessExpression]] :)", // @lfy def/grammar/expression.lfy:42
        CallExpression = "[[Expression]] , [[GroupListExpression]]", // @lfy def/grammar/expression.lfy:44
        TraitExpression = "[[Identifier]] , (/ [[GroupListExpression]] /)", // @lfy def/grammar/expression.lfy:46
        IsExpression = "[[IsKeyword]] , [[TraitExpression]] , (: [[ListContinueSeparator]] , [[TraitExpression]] :) , (/ [[ListContinueSeparator]] /)", // @lfy def/grammar/expression.lfy:48
        DefinitionExpression = "[[DefinitionPunctuation]] , [[Expression]]", // @lfy def/grammar/expression.lfy:50
        IdentityExpression = "[[Identifier]] , (/ [[IsExpression]] /) , (/ [[DefinitionExpression]] /)", // @lfy def/grammar/expression.lfy:52
        FunctionIdentityExpression = "(/ [[Identifier]] /) , [[GroupListExpression]] , (/ [[IsExpression]] /) , (/ [[DefinitionExpression]]  /)", // @lfy def/grammar/expression.lfy:54
        PrimitiveExpression = "[[PrimitiveBooleanKeyword]] | [[NullLiteral]] | [[PrimitiveNumberKeyword]] | [[PrimitiveObjectKeyword]] | [[PrimitiveStringKeyword]] | [[UndefinedLiteral]] | [[FunctionKeyword]]", // @lfy def/grammar/expression.lfy:56
        CastExpression = "[[Expression]] , [[AsKeyword]] , ( [[IdentifierExpression]] | [[PrimitiveExpression]] )", // @lfy def/grammar/expression.lfy:58
        AssignmentExpression = "[[Expression]] , (/ [[IsExpression]] /) , (/ [[DefinitionExpression]] /), (/ [[SetterPuncuation]] , [[Expression]] /)", // @lfy def/grammar/expression.lfy:60
        ListExpression = "[[ListOpenSeparator]] , (: [[Expression]] | [[ListContinueSeparator]] :) , [[ListCloseSeparator]]", // @lfy def/grammar/expression.lfy:62
        GroupListExpression = "[[GroupOpenSeparator]] , (/ [[Expression]] , (: [[ListContinueSeparator]] , [[Expression]] :) , (/ [[ListContinueSeparator]] /) /) , [[GroupCloseSeparator]]", // @lfy def/grammar/expression.lfy:64
        ObjectExpression = "[[BlockOpenSeparator]] , (/ [[AssignmentExpression]] , (: [[ListContinueSeparator]] , [[AssignmentExpression]] :) , (/ [[ListContinueSeparator]] /) /) , [[BlockCloseSeparator]]", // @lfy def/grammar/expression.lfy:66
        AwaitExpression = "[[AwaitKeyword]] , [[Expression]]", // @lfy def/grammar/expression.lfy:68
        SpreadExpression = "[[SpreadPunctuation]] , [[Expression]]", // @lfy def/grammar/expression.lfy:70
        RangeExpression = "[[Expression]] , [[SpreadPunctuation]] , [[Expression]]", // @lfy def/grammar/expression.lfy:72
        FromExpression = "[[FromKeyword]] , [[Expression]]", // @lfy def/grammar/expression.lfy:74
        OfExpression = "[[OfKeyword]] , [[Expression]]", // @lfy def/grammar/expression.lfy:76
        InExpression = "[[InKeyword]] , [[Expression]]", // @lfy def/grammar/expression.lfy:78
        TypeOnlyExpression = "[[LiteralExpression]] | [[IdentifierExpression]] | [[ObjectExpression]] | [[ListExpression]]", // @lfy def/grammar/expression.lfy:80
        Expression = "[[LiteralExpression]] | [[IdentifierAccessorExpression]] | [[IdentifierReferenceExpression]] | [[ListAccessExpression]] | [[GroupedExpression]] | [[NegationExpression]] | [[ArithmaticExpression]] | [[ComparisonExpression]] | [[LogicalExpression]] | [[IdentifierExpression]] | [[CallExpression]] | [[DefinitionExpression]] | [[CastExpression]] | [[AssignmentExpression]] | [[ListExpression]] | [[GroupListExpression]] | [[ObjectExpression]] | [[AwaitExpression]] | [[SpreadExpression]] | [[RangeExpression]] | [[FromExpression]] | [[OfExpression]] | [[InExpression]]", // @lfy def/grammar/expression.lfy:82
    }
}

#[cfg(test)]
mod tests {
    use super::super::EbnfSyntax;
    use super::*;

    // @lfy def/grammar/expression.lfy:16
    #[test]
    fn template_literal_bodies_stop_at_boundaries_blocks_and_bare_backslashes() {
        assert_eq!(
            Expression::TemplateLiteralBody.longest_match("ab`c"),
            Some(2)
        );
        assert_eq!(
            Expression::TemplateLiteralBody.longest_match("a{{x}}"),
            Some(1)
        );
        assert_eq!(
            Expression::TemplateLiteralBody.longest_match("a[[x]]"),
            Some(1)
        );
        assert_eq!(
            Expression::TemplateLiteralBody.longest_match("a}}b]]c"),
            Some(7)
        );
        assert_eq!(
            Expression::TemplateLiteralBody.longest_match("a\\{{b`"),
            Some(5)
        );
        assert_eq!(
            Expression::TemplateLiteralBody.longest_match("a\\qb"),
            Some(1)
        );
        assert_eq!(
            Expression::TemplateLiteralBody.longest_match("x\ny`"),
            Some(3)
        );
        assert_eq!(Expression::TemplateLiteralBody.longest_match("`"), None);
    }

    #[test]
    fn expression_rules_are_declared_in_order() {
        assert_eq!(Expression::ALL.len(), 38);
        assert_eq!(Expression::ALL[0], Expression::ArrowSingleRightExpression);
        assert_eq!(Expression::ALL[37], Expression::Expression);
        assert_eq!(
            Expression::TemplateLiteralReference.rule(),
            "TemplateLiteralReference = [[ReferenceOpenBoundary]] , [[IdentifierExpression]] , [[ReferenceCloseBoundary]] ;"
        );
    }
}
