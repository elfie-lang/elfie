//! Compiled from `def/grammar/file.lfy`.

use super::grammar_rules;

grammar_rules! {
    /// The root rule of a source file.
    pub enum File {
        SourceFile = "(: [[Statement]] :)", // @lfy def/grammar/file.lfy:3
    }
}
