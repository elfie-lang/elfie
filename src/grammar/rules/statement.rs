//! Compiled from `def/grammar/rules/statement.lfy`.

use super::super::traits::category::*;
use super::super::{Entity, grammar_rules};

grammar_rules! {
    /// Statement rules: declarations, blocks and control, criteria sugar, and the set of
    /// every statement.
    pub enum Statement {
        // Declarations
        DataDeclaration is [statement()]: "A data structure whose implementation is generated from its criteria" = "[[AgentDataKeyword]] , [[Identifier]] , (/ [[IsClause]] /) , (/ [[ExtendsClause]] /) , (/ [[DefinitionClause]] /) , ( [[Block]] | [[Semicolon]] )", // @lfy def/grammar/rules/statement.lfy:9
        AgentFunctionDeclaration is [statement()]: "A function whose implementation is generated from its criteria" = "[[AgentFunctionKeyword]] , [[Signature]] , (/ [[DoubleArrowRight]] , [[TypeExpression]] /) , ( [[Block]] | [[Semicolon]] )", // @lfy def/grammar/rules/statement.lfy:10
        FunctionDeclaration is [statement()]: "A function written out in full" = "[[FunctionKeyword]] , [[Signature]] , (/ [[SingleArrow]] , [[TypeExpression]] /) , [[Block]]", // @lfy def/grammar/rules/statement.lfy:11
        TraitDeclaration is [statement()]: "A reusable component" = "[[TraitKeyword]] , [[Identifier]] , (/ [[Parameters]] /) , (/ [[ExtendsClause]] /) , (/ [[DefinitionClause]] /) , [[Block]]", // @lfy def/grammar/rules/statement.lfy:12
        TypeDeclaration is [statement()]: "A structural type" = "[[TypeKeyword]] , [[Declared]] , [[Type]]", // @lfy def/grammar/rules/statement.lfy:13
        EnumDeclaration is [statement()]: "A set of named values" = "[[EnumKeyword]] , [[Declared]] , [[Object]]", // @lfy def/grammar/rules/statement.lfy:14
        VariableDeclaration is [statement()]: "A named value" = "( [[ConstKeyword]] | [[LetKeyword]] ) , [[Declared]] , (/ [[PlainSetter]] , [[Expression]] /) , [[Semicolon]]", // @lfy def/grammar/rules/statement.lfy:15
        AliasDeclaration is [statement()]: "A new name and context for something already named" = "[[AliasKeyword]] , [[Declared]] , [[PlainSetter]] , [[Expression]] , [[Semicolon]]", // @lfy def/grammar/rules/statement.lfy:16
        ExternalDeclaration is [statement()]: "A name defined outside the program" = "[[ExternalKeyword]] , [[Declared]] , [[PlainSetter]] , [[Expression]] , [[Semicolon]]", // @lfy def/grammar/rules/statement.lfy:17
        Use is [statement()]: "Brings a file into scope, as one module name or as its declarations" = "[[UseKeyword]] , [[StringLiteral]] , (/ [[AsKeyword]] , [[Identifier]] /) , [[Semicolon]]", // @lfy def/grammar/rules/statement.lfy:18

        // Blocks and control
        /// Acceptance criteria:
        /// - A `BlockOpen` at the start of a statement always begins a `Block`, never an
        ///   `Object`.
        Block is [statement()]: "A sequence of statements with its own scope" = "[[BlockOpen]] , (: [[Statement]] :) , [[BlockClose]]", // @lfy def/grammar/rules/statement.lfy:22
        If is [statement()]: "A conditional" = "[[IfKeyword]] , [[Group]] , ( [[Block]] | [[ExpressionStatement]] ) , (/ [[Else]] /)", // @lfy def/grammar/rules/statement.lfy:26
        Else is [rule()]: "The other branch; else if is else followed by an if" = "[[ElseKeyword]] , ( [[If]] | [[Block]] | [[ExpressionStatement]] )", // @lfy def/grammar/rules/statement.lfy:27
        /// `[[Comma]] , [[Declared]] , [[FromKeyword]] , listOf([[Expression]])`
        ForFrom is [rule()]: "Loop over keys and values" = "[[Comma]] , [[Declared]] , [[FromKeyword]] , ( [[Expression]] ) , (: [[Comma]] , ( [[Expression]] ) :) , (/ [[Comma]] /)", // @lfy def/grammar/rules/statement.lfy:28
        /// `( [[InKeyword]] | [[OfKeyword]] ) , listOf([[Expression]])`
        ForInOf is [rule()]: "Loop over values or keys" = "( [[InKeyword]] | [[OfKeyword]] ) , ( [[Expression]] ) , (: [[Comma]] , ( [[Expression]] ) :) , (/ [[Comma]] /)", // @lfy def/grammar/rules/statement.lfy:29
        For is [statement()]: "A loop over one or more iterables, visited in order" = "[[ForKeyword]] , [[GroupOpen]] , (/ [[ConstKeyword]] | [[LetKeyword]] /) , [[Declared]] , ( [[ForFrom]] | [[ForInOf]] ) , [[GroupClose]] , [[Block]]", // @lfy def/grammar/rules/statement.lfy:30
        While is [statement()]: "A conditional loop" = "[[WhileKeyword]] , [[Group]] , [[Block]]", // @lfy def/grammar/rules/statement.lfy:31
        Loop is [statement()]: "A loop ended by break" = "[[LoopKeyword]] , [[Block]]", // @lfy def/grammar/rules/statement.lfy:32
        Break is [statement()]: "Leaves the loop" = "[[BreakKeyword]] , [[Semicolon]]", // @lfy def/grammar/rules/statement.lfy:33
        Continue is [statement()]: "Next iteration" = "[[ContinueKeyword]] , [[Semicolon]]", // @lfy def/grammar/rules/statement.lfy:34
        Return is [statement()]: "Ends the function with a value" = "[[ReturnKeyword]] , [[ExpressionStatement]]", // @lfy def/grammar/rules/statement.lfy:35
        MatchArm is [rule()]: "A pattern and its result" = "( [[Expression]] | [[DefaultKeyword]] ) , [[SingleArrow]] , [[Expression]]", // @lfy def/grammar/rules/statement.lfy:36
        /// `( [[MatchKeyword]] | [[MatchallKeyword]] ) , [[Expression]] , [[BlockOpen]] , listOf([[MatchArm]]) , [[BlockClose]]`
        Match is [statement()]: "Branches on patterns" = "( [[MatchKeyword]] | [[MatchallKeyword]] ) , [[Expression]] , [[BlockOpen]] , ( [[MatchArm]] ) , (: [[Comma]] , ( [[MatchArm]] ) :) , (/ [[Comma]] /) , [[BlockClose]]", // @lfy def/grammar/rules/statement.lfy:37
        With is [statement()]: "Runs the block with the name or member as the current entity" = "[[WithKeyword]] , ( [[Name]] | [[Member]] ) , [[Block]]", // @lfy def/grammar/rules/statement.lfy:38
        Async is [statement()]: "Runs the statement as a promise" = "[[AsyncKeyword]] , [[Statement]]", // @lfy def/grammar/rules/statement.lfy:39
        Ace is [statement()]: "Runs the statement at compile time" = "[[AceKeyword]] , [[Statement]]", // @lfy def/grammar/rules/statement.lfy:40
        ExpressionStatement is [statement()]: "An expression on its own" = "[[Expression]] , [[Semicolon]]", // @lfy def/grammar/rules/statement.lfy:41

        // Criteria sugar
        Condition is [rule()]: "One condition, possibly negated or nested" = "(/ [[LogicalNot]] /) , ( [[Group]] | ( [[GroupOpen]] , [[Conditions]] , [[GroupClose]] ) )", // @lfy def/grammar/rules/statement.lfy:45
        /// Acceptance criteria:
        /// - A negated nested group distributes the negation: "and" becomes "or" and "or"
        ///   becomes "and" inside of the group.
        /// - Conditions joined by "or" merge into one situation; conditions joined by "and"
        ///   are a list of situations.
        Conditions is [rule()]: "Conditions joined by and or or" = "[[Condition]] , (: ( [[AndKeyword]] | [[OrKeyword]] ) , [[Condition]] :)", // @lfy def/grammar/rules/statement.lfy:46
        Where is [statement()]: "Adds an acceptance criterion: the conditions are the situation, the expression the behavior" = "[[WhereKeyword]] , [[Conditions]] , [[SingleArrow]] , [[ExpressionStatement]]", // @lfy def/grammar/rules/statement.lfy:51

        /// `alternationList(...statement@entities)`
        Statement is [alternation_list(STATEMENTS)]: "Any statement" = "[[DataDeclaration]] | [[AgentFunctionDeclaration]] | [[FunctionDeclaration]] | [[TraitDeclaration]] | [[TypeDeclaration]] | [[EnumDeclaration]] | [[VariableDeclaration]] | [[AliasDeclaration]] | [[ExternalDeclaration]] | [[Use]] | [[Block]] | [[If]] | [[For]] | [[While]] | [[Loop]] | [[Break]] | [[Continue]] | [[Return]] | [[Match]] | [[With]] | [[Async]] | [[Ace]] | [[ExpressionStatement]] | [[Where]]", // @lfy def/grammar/rules/statement.lfy:53
    }
}

/// `statement@entities`
// @lfy def/grammar/rules/statement.lfy:53
pub const STATEMENTS: &[Entity] = &[
    Entity::Statement(Statement::DataDeclaration),
    Entity::Statement(Statement::AgentFunctionDeclaration),
    Entity::Statement(Statement::FunctionDeclaration),
    Entity::Statement(Statement::TraitDeclaration),
    Entity::Statement(Statement::TypeDeclaration),
    Entity::Statement(Statement::EnumDeclaration),
    Entity::Statement(Statement::VariableDeclaration),
    Entity::Statement(Statement::AliasDeclaration),
    Entity::Statement(Statement::ExternalDeclaration),
    Entity::Statement(Statement::Use),
    Entity::Statement(Statement::Block),
    Entity::Statement(Statement::If),
    Entity::Statement(Statement::For),
    Entity::Statement(Statement::While),
    Entity::Statement(Statement::Loop),
    Entity::Statement(Statement::Break),
    Entity::Statement(Statement::Continue),
    Entity::Statement(Statement::Return),
    Entity::Statement(Statement::Match),
    Entity::Statement(Statement::With),
    Entity::Statement(Statement::Async),
    Entity::Statement(Statement::Ace),
    Entity::Statement(Statement::ExpressionStatement),
    Entity::Statement(Statement::Where),
];

#[cfg(test)]
mod tests {
    use super::super::super::{GrammarRule, rules};
    use super::super::expression::list_of;
    use super::*;

    // @lfy def/grammar/rules/statement.lfy:53
    #[test]
    fn the_statement_set_lists_every_statement_in_order() {
        let statements: Vec<Entity> = rules().filter(|rule| rule.is_statement()).collect();
        assert_eq!(statements, STATEMENTS);
        assert_eq!(STATEMENTS.len(), 24);
        assert_eq!(Statement::ALL.len(), 31);
        for rule in [
            Statement::Else,
            Statement::ForFrom,
            Statement::ForInOf,
            Statement::MatchArm,
            Statement::Condition,
            Statement::Conditions,
            Statement::Statement,
        ] {
            assert!(!rule.is_statement(), "{}", rule.identifier());
        }
    }

    // @lfy def/grammar/rules/statement.lfy:28
    #[test]
    fn lists_inside_statements_come_from_list_of() {
        assert!(
            Statement::ForFrom
                .syntax()
                .ends_with(&list_of("[[Expression]]"))
        );
        assert!(
            Statement::ForInOf
                .syntax()
                .ends_with(&list_of("[[Expression]]"))
        );
        assert_eq!(
            Statement::Match.syntax(),
            format!(
                "( [[MatchKeyword]] | [[MatchallKeyword]] ) , [[Expression]] , [[BlockOpen]] , {} , [[BlockClose]]",
                list_of("[[MatchArm]]")
            )
        );
    }
}
