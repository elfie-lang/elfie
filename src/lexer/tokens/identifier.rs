//! Compiled from `def/lexer/tokens/identifier.lfy`.

use unicode_ident::{is_xid_continue, is_xid_start};

use super::keyword::Keyword;

/// Matches an identifier at the start of `rest`: a Unicode `XID_Start` or `_` character
/// followed by any number of `XID_Continue` characters, provided the whole run is not a
/// keyword. Returns the matched byte length.
// @lfy def/lexer/tokens/identifier.lfy:6
pub fn match_identifier(rest: &str) -> Option<usize> {
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if first != '_' && !is_xid_start(first) {
        return None;
    }
    let mut len = first.len_utf8();
    for (index, c) in chars {
        if !is_xid_continue(c) {
            break;
        }
        len = index + c.len_utf8();
    }
    if Keyword::from_value(&rest[..len]).is_some() {
        return None;
    }
    Some(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/lexer/tokens/identifier.lfy:6
    #[test]
    fn identifiers_start_with_xid_start_or_underscore() {
        assert_eq!(match_identifier("abc def"), Some(3));
        assert_eq!(match_identifier("_x1 "), Some(3));
        assert_eq!(match_identifier("héllo!"), Some("héllo".len()));
        assert_eq!(match_identifier("日本語 x"), Some("日本語".len()));
        assert_eq!(match_identifier("1abc"), None);
        assert_eq!(match_identifier("-x"), None);
        assert_eq!(match_identifier(""), None);
    }

    // @lfy def/lexer/tokens/identifier.lfy:7
    #[test]
    fn a_run_that_equals_a_keyword_is_not_an_identifier() {
        assert_eq!(match_identifier("const "), None);
        assert_eq!(match_identifier("constant "), Some(8));
        assert_eq!(match_identifier("d"), None);
        assert_eq!(match_identifier("d1"), Some(2));
    }
}
