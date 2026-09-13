//! Compiled from `def/grammar/tokens/space.lfy`.

use super::super::grammar_rules;

grammar_rules! {
    /// Whitespace rules.
    pub enum Space {
        NewLine = "\"\n\" | \"\r\n\"", // @lfy def/grammar/tokens/space.lfy:3

        Space = "[[NewLine]] | \" \" | \"\t\" | \"\u{000B}\" | \"\u{000C}\" | \"\u{0085}\" | \"\u{200E}\" | \"\u{200F}\" | \"\u{2028}\" | \"\u{2029}\"", // @lfy def/grammar/tokens/space.lfy:5
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::EbnfSyntax;
    use super::*;

    // @lfy def/grammar/tokens/space.lfy:3
    #[test]
    fn new_lines_are_lf_or_crlf() {
        assert_eq!(Space::NewLine.longest_match("\nx"), Some(1));
        assert_eq!(Space::NewLine.longest_match("\r\nx"), Some(2));
        assert_eq!(Space::NewLine.longest_match("\rx"), None);
    }

    // @lfy def/grammar/tokens/space.lfy:5
    #[test]
    fn every_space_character_matches_the_space_rule() {
        for c in [
            '\n', ' ', '\t', '\u{000B}', '\u{000C}', '\u{0085}', '\u{200E}', '\u{200F}',
            '\u{2028}', '\u{2029}',
        ] {
            let text = format!("{c}x");
            assert_eq!(
                Space::Space.longest_match(&text),
                Some(c.len_utf8()),
                "{c:?}"
            );
        }
        assert_eq!(Space::Space.longest_match("\r\n"), Some(2));
        assert_eq!(Space::Space.longest_match("\r"), None);
        assert_eq!(Space::Space.longest_match("x"), None);
    }
}
