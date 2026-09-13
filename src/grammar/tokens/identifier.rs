//! Compiled from `def/grammar/tokens/identifier.lfy`.

use super::super::{EbnfSyntax, grammar_rules};
use super::keyword::Keyword;

grammar_rules! {
    /// Identifier rules. `XID_Start` and `XID_Continue` are external, provided by the
    /// `unicode-ident` crate through the EBNF built-ins.
    #[allow(non_camel_case_types)]
    pub enum Identifier {
        /// `external const`
        XID_Start = "? XID_Start defined by Unicode / unicode-ident ?", // @lfy def/grammar/tokens/identifier.lfy:3
        /// `external const`
        XID_Continue = "? XID_Continue defined by Unicode / unicode-ident ?", // @lfy def/grammar/tokens/identifier.lfy:4
        /// Identifier of the format XID_Start or _ followed by any number of XID_Continue
        /// characters that do not match a keyword; see [`Identifier::match_identifier`].
        Identifier = r#"( XID_Start | "_" ) , (: XID_Continue :)"#, // @lfy def/grammar/tokens/identifier.lfy:6
    }
}

impl Identifier {
    /// Byte length of the identifier at the start of `rest`, or `None` when there is none
    /// or the matched text is a keyword.
    // @lfy def/grammar/tokens/identifier.lfy:8
    pub fn match_identifier(rest: &str) -> Option<usize> {
        let len = Identifier::Identifier.longest_match(rest)?;
        if Keyword::from_text(&rest[..len]).is_some() {
            return None;
        }
        Some(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/grammar/tokens/identifier.lfy:6
    #[test]
    fn identifiers_start_with_xid_start_or_underscore() {
        assert_eq!(Identifier::match_identifier("abc def"), Some(3));
        assert_eq!(Identifier::match_identifier("_x1 "), Some(3));
        assert_eq!(Identifier::match_identifier("héllo!"), Some("héllo".len()));
        assert_eq!(
            Identifier::match_identifier("日本語 x"),
            Some("日本語".len())
        );
        assert_eq!(Identifier::match_identifier("1abc"), None);
        assert_eq!(Identifier::match_identifier("-x"), None);
        assert_eq!(Identifier::match_identifier(""), None);
    }

    // @lfy def/grammar/tokens/identifier.lfy:8
    #[test]
    fn a_run_that_equals_a_keyword_is_not_an_identifier() {
        assert_eq!(Identifier::match_identifier("const "), None);
        assert_eq!(Identifier::match_identifier("constant "), Some(8));
        assert_eq!(Identifier::match_identifier("d"), None);
        assert_eq!(Identifier::match_identifier("d1"), Some(2));
        assert_eq!(Identifier::match_identifier("with"), None);
        assert_eq!(Identifier::match_identifier("true"), Some(4));
    }
}
