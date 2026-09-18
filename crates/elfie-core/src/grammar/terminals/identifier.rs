//! Compiled from `def/grammar/terminals/identifier.lfy`.

use super::super::grammar_rules;
use super::super::traits::category::*;
use super::keyword::Keyword;

grammar_rules! {
    /// The identifier terminal and the external character classes it is built from, which
    /// the `unicode-ident` crate provides through the EBNF built-ins.
    pub enum Identifier {
        /// `external`: a character class the lexer already knows; never a token of its own.
        IdentifierStart is [rule()] = "? XID_Start defined by Unicode / unicode-ident ?", // @lfy def/grammar/terminals/identifier.lfy:4
        /// `external`: a character class the lexer already knows; never a token of its own.
        IdentifierContinue is [rule()] = "? XID_Continue defined by Unicode / unicode-ident ?", // @lfy def/grammar/terminals/identifier.lfy:5
        /// When the text is a keyword, it is that keyword, not an identifier; see
        /// [`Identifier::keyword_of`].
        Identifier is [terminal()]: "A name" = r#"( [[IdentifierStart]] | "_" ) , (: [[IdentifierContinue]] :)"#, // @lfy def/grammar/terminals/identifier.lfy:7
    }
}

impl Identifier {
    /// `where (The text is a Keyword) -> Is that keyword, not an identifier`: the keyword
    /// that text matched by the identifier rule is instead, if any.
    // @lfy def/grammar/terminals/identifier.lfy:8
    pub fn keyword_of(text: &str) -> Option<Keyword> {
        Keyword::from_text(text)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::GrammarRule;
    use super::*;

    // @lfy def/grammar/terminals/identifier.lfy:7
    #[test]
    fn identifiers_start_with_xid_start_or_underscore() {
        let identifier = Identifier::Identifier;
        assert_eq!(identifier.longest_match("abc def"), Some(3));
        assert_eq!(identifier.longest_match("_x1 "), Some(3));
        assert_eq!(identifier.longest_match("héllo!"), Some("héllo".len()));
        assert_eq!(identifier.longest_match("日本語 x"), Some("日本語".len()));
        assert_eq!(identifier.longest_match("_"), Some(1));
        assert_eq!(identifier.longest_match("1abc"), None);
        assert_eq!(identifier.longest_match("-x"), None);
        assert_eq!(identifier.longest_match(""), None);
        assert!(identifier.is_terminal());
        assert!(!Identifier::IdentifierStart.is_terminal());
    }

    // @lfy def/grammar/terminals/identifier.lfy:8
    #[test]
    fn text_that_is_a_keyword_is_that_keyword() {
        assert_eq!(Identifier::keyword_of("const"), Some(Keyword::ConstKeyword));
        assert_eq!(Identifier::keyword_of("d"), Some(Keyword::AgentDataKeyword));
        assert_eq!(Identifier::keyword_of("yield"), Some(Keyword::YieldKeyword));
        assert_eq!(Identifier::keyword_of("constant"), None);
        assert_eq!(Identifier::keyword_of("d1"), None);
    }
}
