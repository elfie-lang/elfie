//! Compiled from `def/grammar/tokens/separator.lfy`.

use super::super::grammar_rules;

grammar_rules! {
    /// Separator rules.
    pub enum Separator {
        /// an open token for a block
        BlockOpenSeparator = r#""{""#, // @lfy def/grammar/tokens/separator.lfy:3
        /// a close token for a block
        BlockCloseSeparator = r#""}""#, // @lfy def/grammar/tokens/separator.lfy:4
        /// an open token for a group
        GroupOpenSeparator = r#""(""#, // @lfy def/grammar/tokens/separator.lfy:5
        /// a close token for a group
        GroupCloseSeparator = r#"")""#, // @lfy def/grammar/tokens/separator.lfy:6
        /// an open token for a list
        ListOpenSeparator = r#""[""#, // @lfy def/grammar/tokens/separator.lfy:7
        /// continue character for next item or statement in list
        ListContinueSeparator = r#"",""#, // @lfy def/grammar/tokens/separator.lfy:8
        /// a close token for a list
        ListCloseSeparator = r#""]""#, // @lfy def/grammar/tokens/separator.lfy:9
        /// end character for statements
        StatementEndSeparator = r#"";""#, // @lfy def/grammar/tokens/separator.lfy:10
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::EbnfSyntax;
    use super::*;

    // @lfy def/grammar/tokens/separator.lfy:3
    #[test]
    fn every_separator_is_a_single_character_terminal() {
        for &separator in Separator::ALL {
            assert_eq!(separator.syntax().len(), 3, "{}", separator.identifier());
            assert!(separator.traits().is_empty());
        }
        assert_eq!(Separator::ALL.len(), 8);
    }
}
