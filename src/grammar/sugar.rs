//! Compiled from `def/grammar/sugar.lfy`.
//!
//! The `where` syntax sugar. `where (a) and !(b) -> behavior;` translates to an
//! `@acceptanceCriteria.add` call whose `situation` is the where expression and whose
//! `behavior` is the expression statement after the arrow. Grouped expressions are
//! evaluated left to right into a flat list: `or` merges neighbours into one entry, `and`
//! keeps them as separate entries, `!` in front of a group is distributed (swapping `and`
//! and `or`) and `!` in front of a grouped expression wraps it as `Not (...)`.

use super::grammar_rules;

grammar_rules! {
    /// Syntax sugar rules.
    pub enum Sugar {
        WhereExpression = "( (/ [[LogicalNotPunctuation]] /) , [[GroupOpenSeparator]] , [[WhereExpression]] , [[GroupCloseSeparator]] , (: ( [[AndKeyword]] | [[OrKeyword]] ) , [[WhereExpression]] :) ) | ( (/ [[LogicalNotPunctuation]] /) , [[GroupedExpression]] , (: ( [[AndKeyword]] | [[OrKeyword]] ) , (/ [[LogicalNotPunctuation]] /) , [[GroupedExpression]] :) )", // @lfy def/grammar/sugar.lfy:8
        WhereStatement = "[[WhereKeyword]] , [[WhereExpression]] , [[ArrowSingleRightExpression]] , [[ExpressionStatement]]", // @lfy def/grammar/sugar.lfy:49
    }
}

#[cfg(test)]
mod tests {
    use super::super::EbnfSyntax;
    use super::*;

    // @lfy def/grammar/sugar.lfy:49
    #[test]
    fn where_statements_are_built_from_the_where_expression_and_an_arrow() {
        let references = Sugar::WhereStatement.expression().unwrap().references();
        assert_eq!(
            references,
            vec![
                "WhereKeyword",
                "WhereExpression",
                "ArrowSingleRightExpression",
                "ExpressionStatement"
            ]
        );
        let nested = Sugar::WhereExpression.expression().unwrap().references();
        assert!(nested.contains(&"WhereExpression") && nested.contains(&"GroupedExpression"));
        assert_eq!(Sugar::ALL.len(), 2);
    }
}
