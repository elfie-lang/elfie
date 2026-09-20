//! Compiled from `def/grammar/rules/file.lfy`.

use super::super::grammar_rules;
use super::super::traits::category::*;

grammar_rules! {
    /// The root rule of a source file.
    pub enum File {
        /// Acceptance criteria:
        /// - When `Space`, `NewLine`, `Comment`, or `Documentation` appear between two
        ///   tokens and no rule exists on either token specifying that they are not
        ///   allowed for a match: `Space`, `NewLine`, `Comment`, and `Documentation` may
        ///   appear between any two tokens outside text bodies.
        SourceFile is [rule()]: "A file: statements in order" = "(: [[Statement]] :)", // @lfy def/grammar/rules/file.lfy:SourceFile
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::GrammarRule;
    use super::*;

    // @lfy def/grammar/rules/file.lfy:SourceFile
    #[test]
    fn a_source_file_is_statements_in_order() {
        assert_eq!(File::ALL, &[File::SourceFile]);
        assert_eq!(
            File::SourceFile.text(),
            "SourceFile = (: [[Statement]] :) ;"
        );
        assert_eq!(
            File::SourceFile.expression().unwrap().references(),
            vec!["Statement"]
        );
    }
}
