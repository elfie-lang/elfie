//! Compiled from `def/lexer/tokens/whitespace.lfy`.

use super::token_enum;

token_enum! {
    /// Whitespace characters. Each one is lexed as its own space token.
    // @lfy def/lexer/tokens/whitespace.lfy:1
    pub enum Space {
        Space => ("SP_SPACE", " ", ""),                                   // @lfy def/lexer/tokens/whitespace.lfy:2
        Newline => ("SP_NEWLINE", "\n", ""),                              // @lfy def/lexer/tokens/whitespace.lfy:3
        CarriageReturn => ("SP_CARRIAGE_RETURN", "\r", ""),               // @lfy def/lexer/tokens/whitespace.lfy:4
        TabHorizontal => ("SP_TAB_HORIZONTAL", "\t", ""),                 // @lfy def/lexer/tokens/whitespace.lfy:5
        TabVertical => ("SP_TAB_VERTICAL", "\u{000B}", ""),               // @lfy def/lexer/tokens/whitespace.lfy:6
        FormFeed => ("SP_FORM_FEED", "\u{000C}", ""),                     // @lfy def/lexer/tokens/whitespace.lfy:7
        NextLine => ("SP_NEXT_LINE", "\u{0085}", ""),                     // @lfy def/lexer/tokens/whitespace.lfy:8
        MarkLeftToRight => ("SP_MARK_LEFT_TO_RIGHT", "\u{200E}", ""),     // @lfy def/lexer/tokens/whitespace.lfy:9
        MarkRightToLeft => ("SP_MARK_RIGHT_TO_LEFT", "\u{200F}", ""),     // @lfy def/lexer/tokens/whitespace.lfy:10
        LineSeparator => ("SP_LINE_SEPARATOR", "\u{2028}", ""),           // @lfy def/lexer/tokens/whitespace.lfy:11
        ParagraphSeparator => ("SP_PARAGRAPH_SEPARATOR", "\u{2029}", ""), // @lfy def/lexer/tokens/whitespace.lfy:12
    }
}

/// The `\r\n` end-of-line form, interchangeable with `\n`.
// @lfy def/lexer/main.lfy:18
pub const CRLF: &str = "\r\n";

/// Whether `rest` starts with an end-of-line (`\n` or `\r\n`).
// @lfy def/lexer/main.lfy:18
pub fn starts_with_end_of_line(rest: &str) -> bool {
    rest.starts_with('\n') || rest.starts_with(CRLF)
}

/// Matches a space token at the start of `rest`, returning the member, the matched byte
/// length and the token value. `\r\n` is one newline token whose value equals `\n`.
// @lfy def/lexer/main.lfy:88
pub fn match_space(rest: &str) -> Option<(Space, usize, &'static str)> {
    if rest.starts_with(CRLF) {
        // @lfy def/lexer/main.lfy:18
        return Some((Space::Newline, CRLF.len(), Space::Newline.value()));
    }
    Space::matches_at(rest)
        .next()
        .map(|(space, len)| (space, len, space.value()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/lexer/tokens/whitespace.lfy:1
    #[test]
    fn every_space_member_matches_only_its_own_character() {
        for &space in Space::ALL {
            let (matched, len, value) = match_space(space.value()).unwrap();
            assert_eq!(matched, space);
            assert_eq!(len, space.value().len());
            assert_eq!(value, space.value());
            assert_eq!(space.value().chars().count(), 1);
        }
        assert!(match_space("x").is_none());
    }

    // @lfy def/lexer/main.lfy:18
    #[test]
    fn crlf_is_one_newline_token_with_the_newline_value() {
        assert_eq!(match_space("\r\nx"), Some((Space::Newline, 2, "\n")));
        assert_eq!(match_space("\rx"), Some((Space::CarriageReturn, 1, "\r")));
        assert!(starts_with_end_of_line("\n"));
        assert!(starts_with_end_of_line("\r\n"));
        assert!(!starts_with_end_of_line("\r"));
        assert!(!starts_with_end_of_line("\u{2028}"));
    }
}
