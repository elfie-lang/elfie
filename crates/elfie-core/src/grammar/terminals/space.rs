//! Compiled from `def/grammar/terminals/space.lfy`.

use super::super::grammar_rules;
use super::super::traits::category::*;

grammar_rules! {
    /// Whitespace terminals.
    pub enum Space {
        /// Value is always equal to `\n` regardless of raw text; see [`NEW_LINE_VALUE`].
        NewLine is [terminal()]: "A line break" = "\"\n\" | \"\r\n\"", // @lfy def/grammar/terminals/space.lfy:3
        Space is [terminal()]: "Various space characters" = "\" \" | \"\t\" | \"\u{000B}\" | \"\u{000C}\" | \"\u{0085}\" | \"\u{200E}\" | \"\u{200F}\" | \"\u{2028}\" | \"\u{2029}\"", // @lfy def/grammar/terminals/space.lfy:6
    }
}

/// The value of every `NewLine` token, whichever spelling the raw text uses.
// @lfy def/grammar/terminals/space.lfy:4
pub const NEW_LINE_VALUE: &str = "\n";

#[cfg(test)]
mod tests {
    use super::super::super::GrammarRule;
    use super::*;

    // @lfy def/grammar/terminals/space.lfy:3
    #[test]
    fn new_lines_are_lf_or_crlf() {
        assert_eq!(Space::NewLine.longest_match("\nx"), Some(1));
        assert_eq!(Space::NewLine.longest_match("\r\nx"), Some(2));
        assert_eq!(Space::NewLine.longest_match("\rx"), None);
        assert_eq!(Space::NewLine.longest_match("\n\n"), Some(1));
        assert_eq!(NEW_LINE_VALUE, "\n");
    }

    // @lfy def/grammar/terminals/space.lfy:6
    #[test]
    fn every_space_character_matches_the_space_rule_alone() {
        for c in [
            ' ', '\t', '\u{000B}', '\u{000C}', '\u{0085}', '\u{200E}', '\u{200F}', '\u{2028}',
            '\u{2029}',
        ] {
            let text = format!("{c}{c}x");
            assert_eq!(
                Space::Space.longest_match(&text),
                Some(c.len_utf8()),
                "{c:?}"
            );
        }
        assert_eq!(Space::Space.longest_match("\n"), None);
        assert_eq!(Space::Space.longest_match("\r\n"), None);
        assert_eq!(Space::Space.longest_match("x"), None);
    }
}
