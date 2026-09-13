//! Compiled from `def/grammar/statement.lfy`.

use super::grammar_rules;

grammar_rules! {
    /// Statement rules.
    pub enum Statement {
        BlockStatement = "[[BlockOpenSeparator]] , (: [[Statement]] :) , [[BlockCloseSeparator]]", // @lfy def/grammar/statement.lfy:9
        AgentDataStatement = "[[AgentDataKeyword]] , [[IdentityExpression]] , (/ [[ExtendsKeyword]] , [[GroupListExpression]] /) , ( [[BlockStatement]] | [[StatementEndSeparator]] )", // @lfy def/grammar/statement.lfy:11
        AgentFunctionStatement = "[[AgentFnKeyword]] , [[FunctionIdentityExpression]] , (/ [[ArrowDoubleRightPunctuation]] , [[TypeOnlyExpression]] /) , ( [[BlockStatement]] | [[StatementEndSeparator]])", // @lfy def/grammar/statement.lfy:13
        AliasStatement = "[[AliasKeyword]] , [[AssignmentExpression]] , [[StatementEndSeparator]]", // @lfy def/grammar/statement.lfy:15
        AsyncStatement = "[[AsyncKeyword]] , [[Statement]]", // @lfy def/grammar/statement.lfy:17
        BreakStatement = "[[BreakKeyword]] , [[StatementEndSeparator]]", // @lfy def/grammar/statement.lfy:19
        AceStatement = "[[CompileTimeExecutionKeyword]] , [[Statement]]", // @lfy def/grammar/statement.lfy:21
        VariableStatement = "( [[ConstKeyword]] | [[LetKeyword]] ) , [[AssignmentExpression]] , [[StatementEndSeparator]]", // @lfy def/grammar/statement.lfy:23
        ContinueStatement = "[[ContinueKeyword]] , [[StatementEndSeparator]]", // @lfy def/grammar/statement.lfy:25
        IfStatement = "[[IfKeyword]] , [[GroupedExpression]] , ( [[ExpressionStatement]] | [[BlockStatement]] ) , (: [[ElseIfStatement]] :) , (/ [[ElseStatement]] /)", // @lfy def/grammar/statement.lfy:27
        ElseIfStatement = "[[ElseKeyword]] , [[IfKeyword]] , [[GroupedExpression]] , ( [[ExpressionStatement]] | [[BlockStatement]] )", // @lfy def/grammar/statement.lfy:29
        ElseStatement = "[[ElseKeyword]] , ( [[ExpressionStatement]] | [[BlockStatement]] )", // @lfy def/grammar/statement.lfy:31
        EnumStatement = "[[EnumKeyword]] , [[IdentityExpression]] , [[ObjectExpression]]", // @lfy def/grammar/statement.lfy:33
        ExternalStatement = "[[ExternalKeyword]] , [[AssignmentExpression]] , [[StatementEndSeparator]]", // @lfy def/grammar/statement.lfy:35
        ExpressionStatement = "[[Expression]] , [[StatementEndSeparator]]", // @lfy def/grammar/statement.lfy:37
        ForFromStatement = "[[ForKeyword]] , [[GroupOpenSeparator]] , (/ [[ConstKeyword]] | [[LetKeyword]] /) , [[IdentityExpression]] , [[ListContinueSeparator]] , [[IdentityExpression]] , [[FromKeyword]] , [[Expression]] , (: [[ListContinueSeparator]] , [[Expression]] :) , (/ [[ListContinueSeparator]] /) , [[GroupCloseSeparator]] , [[BlockStatement]]", // @lfy def/grammar/statement.lfy:39
        ForInOfStatement = "[[ForKeyword]] , [[GroupOpenSeparator]] , (/ [[ConstKeyword]] | [[LetKeyword]] /) , [[IdentityExpression]] , ( [[InKeyword]] | [[OfKeyword]] ) , [[Expression]] , (: [[ListContinueSeparator]] , [[Expression]] :) , (/ [[ListContinueSeparator]] /) , [[GroupCloseSeparator]] , [[BlockStatement]]", // @lfy def/grammar/statement.lfy:41
        FunctionStatement = "[[FunctionKeyword]] , [[FunctionIdentityExpression]] , (/ [[ArrowSingleRightExpression]] , [[TypeOnlyExpression]] /) , [[BlockStatement]]", // @lfy def/grammar/statement.lfy:43
        FunctionInlineStatement = "[[FunctionIdentityExpression]] , (/ [[ArrowSingleRightExpression]] , [[TypeOnlyExpression]] /) , [[ArrowDoubleRightPunctuation]] , ( [[ExpressionStatement]] | [[BlockStatement]] )", // @lfy def/grammar/statement.lfy:45
        LoopStatement = "[[LoopKeyword]] , [[BlockStatement]]", // @lfy def/grammar/statement.lfy:47
        MatchStatement = "( [[MatchKeyword]] | [[MatchallKeyword]] ) , [[Expression]] , [[BlockOpenSeparator]] , (/ ( [[Expression]] | [[DefaultKeyword]] ) , [[ArrowDoubleRightPunctuation]] , [[Expression]] , (: [[ListContinueSeparator]] , ( [[Expression]] | [[DefaultKeyword]] ) , [[ArrowDoubleRightPunctuation]] , [[Expression]] :) /) , (/ [[ListContinueSeparator]] /) , [[BlockCloseSeparator]]", // @lfy def/grammar/statement.lfy:49
        ReturnStatement = "[[ReturnKeyword]] , [[ExpressionStatement]]", // @lfy def/grammar/statement.lfy:51
        TraitStatement = "[[TraitKeyword]] , [[Identifier]] , (/ [[GroupListExpression]] /) , (/ [[ExtendsKeyword]] , [[GroupListExpression]] /) , [[BlockStatement]]", // @lfy def/grammar/statement.lfy:53
        TypeStatement = "[[TypeKeyword]] , [[IdentityExpression]] , [[ObjectExpression]]", // @lfy def/grammar/statement.lfy:55
        UseStatement = "[[UseKeyword]] , ( [[StringLiteral]] | [[TemplateLiteral]] ) , (/ [[AsKeyword]] , [[Identifier]] /) , [[StatementEndSeparator]]", // @lfy def/grammar/statement.lfy:57
        WhileStatement = "[[WhileKeyword]] , [[GroupedExpression]] , [[BlockStatement]]", // @lfy def/grammar/statement.lfy:59
        WithStatement = "[[WithKeyword]] , [[IdentifierExpression]] , [[BlockStatement]]", // @lfy def/grammar/statement.lfy:61
        Statement = "[[AgentDataStatement]] | [[AgentFunctionStatement]] | [[AliasStatement]] | [[AsyncStatement]] | [[BreakStatement]] | [[AceStatement]] | [[VariableStatement]] | [[ContinueStatement]] | [[IfStatement]] | [[EnumStatement]] | [[ExternalStatement]] | [[ExpressionStatement]] | [[ForFromStatement]] | [[ForInOfStatement]] | [[FunctionStatement]] | [[FunctionInlineStatement]] | [[LoopStatement]] | [[MatchStatement]] | [[ReturnStatement]] | [[TraitStatement]] | [[TypeStatement]] | [[UseStatement]] | [[WhileStatement]] | [[WithStatement]]", // @lfy def/grammar/statement.lfy:63
    }
}

#[cfg(test)]
mod tests {
    use super::super::EbnfSyntax;
    use super::*;

    // @lfy def/grammar/statement.lfy:63
    #[test]
    fn every_statement_alternative_is_a_declared_statement() {
        let references = Statement::Statement.expression().unwrap().references();
        assert_eq!(references.len(), 24);
        for reference in references {
            assert!(
                Statement::ALL.iter().any(|s| s.identifier() == reference),
                "{reference}"
            );
        }
        assert_eq!(Statement::ALL.len(), 28);
    }
}
