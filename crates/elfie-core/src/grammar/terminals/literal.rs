//! Compiled from `def/grammar/terminals/literal.lfy`.

use super::super::traits::category::*;
use super::super::{Becomes, Entity, grammar_rules};
use super::space::Space;

grammar_rules! {
    /// Numbers, the boundaries, escapes and bodies of strings and templates, and the
    /// external character classes they are built from.
    pub enum Literal {
        // Character classes the lexer already knows; never tokens of their own
        /// `external`
        Character is [rule()] = "? any UTF-8 scalar value ?", // @lfy def/grammar/terminals/literal.lfy:5
        /// `external`
        Digit is [rule()] = r#""0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9""#, // @lfy def/grammar/terminals/literal.lfy:6
        /// `external`
        HexDigit is [rule()] = r#"[[Digit]] | "a" | "b" | "c" | "d" | "e" | "f" | "A" | "B" | "C" | "D" | "E" | "F""#, // @lfy def/grammar/terminals/literal.lfy:7
        /// `external`
        Backslash is [rule()] = r#""\""#, // @lfy def/grammar/terminals/literal.lfy:8

        // Numbers
        /// When underscores are present they are removed from the token value; see
        /// [`number_value`].
        NumberLiteral is [terminal()]: "A decimal number; underscores group digits" = r#"[[Digit]] , (: [[Digit]] | ( "_" , [[Digit]] ) :) , (/ "." , [[Digit]] , (: [[Digit]] | ( "_" , [[Digit]] ) :) /)"#, // @lfy def/grammar/terminals/literal.lfy:11

        // Boundaries
        SingleQuote is [boundary("'")]: "Opens and closes a single quoted string" = r#""'""#, // @lfy def/grammar/terminals/literal.lfy:18
        DoubleQuote is [boundary("\"")]: "Opens and closes a double quoted string" = r#"'"'"#, // @lfy def/grammar/terminals/literal.lfy:19
        Backtick is [boundary("`")]: "Opens and closes a template" = r#""`""#, // @lfy def/grammar/terminals/literal.lfy:20
        ExecutionOpen is [boundary("{{")]: "Opens an expression inside a template" = r#""{{""#, // @lfy def/grammar/terminals/literal.lfy:21
        ExecutionClose is [boundary("}}")]: "Closes an expression inside a template" = r#""}}""#, // @lfy def/grammar/terminals/literal.lfy:22
        ReferenceOpen is [boundary("[[")]: "Opens a reference inside a template or documentation" = r#""[[""#, // @lfy def/grammar/terminals/literal.lfy:23
        ReferenceClose is [boundary("]]")]: "Closes a reference inside a template or documentation" = r#""]]""#, // @lfy def/grammar/terminals/literal.lfy:24

        // Escapes: each names what it becomes; bodies list which escapes they accept
        BackslashEscape is [escape(Becomes::Text("\\"))]: "A backslash" = r#""\\""#, // @lfy def/grammar/terminals/literal.lfy:27
        SingleQuoteEscape is [escape(Becomes::Text("'"))]: "A single quote" = r#""\'""#, // @lfy def/grammar/terminals/literal.lfy:28
        DoubleQuoteEscape is [escape(Becomes::Text("\""))]: "A double quote" = r#"'\"'"#, // @lfy def/grammar/terminals/literal.lfy:29
        BacktickEscape is [escape(Becomes::Text("`"))]: "A backtick" = r#"'\`'"#, // @lfy def/grammar/terminals/literal.lfy:30
        NewlineEscape is [escape(Becomes::Text("\n"))]: "A line feed" = r#""\n""#, // @lfy def/grammar/terminals/literal.lfy:31
        CarriageReturnEscape is [escape(Becomes::Text("\r"))]: "A carriage return" = r#""\r""#, // @lfy def/grammar/terminals/literal.lfy:32
        TabEscape is [escape(Becomes::Text("\t"))]: "A tab" = r#""\t""#, // @lfy def/grammar/terminals/literal.lfy:33
        NullEscape is [escape(Becomes::Text("\0"))]: "The null character" = r#""\0""#, // @lfy def/grammar/terminals/literal.lfy:34
        ContinuationEscape is [escape(Becomes::Text(""))]: "A line break that does not end the text" = r#""\" , [[NewLine]]"#, // @lfy def/grammar/terminals/literal.lfy:35
        HexEscape is [escape(Becomes::CharacterWithCode)]: "A character by two hex digits" = r#""\x" , [[HexDigit]] , [[HexDigit]]"#, // @lfy def/grammar/terminals/literal.lfy:36
        /// When the digits are not a scalar value the text is kept as written.
        UnicodeEscape is [escape(Becomes::CharacterWithCodePoint)]: "A character by four or six hex digits" = r#""\u" , [[HexDigit]] , [[HexDigit]] , [[HexDigit]] , [[HexDigit]] , (/ [[HexDigit]] , [[HexDigit]] /)"#, // @lfy def/grammar/terminals/literal.lfy:37
        ExecutionOpenEscape is [escape(Becomes::Text("{{"))]: "Literal opening execution braces" = r#""\{{""#, // @lfy def/grammar/terminals/literal.lfy:43
        ExecutionCloseEscape is [escape(Becomes::Text("}}"))]: "Literal closing execution braces" = r#""\}}""#, // @lfy def/grammar/terminals/literal.lfy:44
        ReferenceOpenEscape is [escape(Becomes::Text("[["))]: "Literal opening reference brackets" = r#""\[[""#, // @lfy def/grammar/terminals/literal.lfy:45
        ReferenceCloseEscape is [escape(Becomes::Text("]]"))]: "Literal closing reference brackets" = r#""\]]""#, // @lfy def/grammar/terminals/literal.lfy:46

        // Bodies: what may appear between boundaries
        SingleQuoteBody is [body(SINGLE_QUOTE_BODY_EXCLUDED, SINGLE_QUOTE_BODY_ESCAPES)]: "Text of a single quoted string" = "(: ( Character - ( [[NewLine]] | [[SingleQuote]] | [[Backslash]] ) ) | [[BackslashEscape]] | [[SingleQuoteEscape]] | [[ContinuationEscape]] :)", // @lfy def/grammar/terminals/literal.lfy:49
        DoubleQuoteBody is [body(DOUBLE_QUOTE_BODY_EXCLUDED, DOUBLE_QUOTE_BODY_ESCAPES)]: "Text of a double quoted string" = "(: ( Character - ( [[NewLine]] | [[DoubleQuote]] | [[Backslash]] ) ) | [[BackslashEscape]] | [[DoubleQuoteEscape]] | [[SingleQuoteEscape]] | [[NewlineEscape]] | [[CarriageReturnEscape]] | [[TabEscape]] | [[NullEscape]] | [[ContinuationEscape]] | [[HexEscape]] | [[UnicodeEscape]] :)", // @lfy def/grammar/terminals/literal.lfy:50
        TemplateBody is [body(TEMPLATE_BODY_EXCLUDED, TEMPLATE_BODY_ESCAPES)]: "Text of a template between its expressions and references" = "(: ( Character - ( [[Backtick]] | [[Backslash]] | [[ExecutionOpen]] | [[ReferenceOpen]] ) ) | [[BackslashEscape]] | [[BacktickEscape]] | [[SingleQuoteEscape]] | [[DoubleQuoteEscape]] | [[NewlineEscape]] | [[CarriageReturnEscape]] | [[TabEscape]] | [[NullEscape]] | [[ContinuationEscape]] | [[HexEscape]] | [[UnicodeEscape]] | [[ExecutionOpenEscape]] | [[ExecutionCloseEscape]] | [[ReferenceOpenEscape]] | [[ReferenceCloseEscape]] :)", // @lfy def/grammar/terminals/literal.lfy:65

        // Strings
        SingleQuoteString is [rule()]: "A string with no interpolation" = "[[SingleQuote]] , (/ [[SingleQuoteBody]] /) , [[SingleQuote]]", // @lfy def/grammar/terminals/literal.lfy:87
        DoubleQuoteString is [rule()]: "A string with escapes" = "[[DoubleQuote]] , (/ [[DoubleQuoteBody]] /) , [[DoubleQuote]]", // @lfy def/grammar/terminals/literal.lfy:88
        // Template is defined in the expression rules due to its more complex nature
    }
}

/// The value of a `NumberLiteral` token: the raw text with its underscores removed.
// @lfy def/grammar/terminals/literal.lfy:14
pub fn number_value(raw: &str) -> String {
    raw.chars().filter(|&c| c != '_').collect()
}

// @lfy def/grammar/terminals/literal.lfy:49
pub const SINGLE_QUOTE_BODY_EXCLUDED: &[Entity] = &[
    Entity::Space(Space::NewLine),
    Entity::Literal(Literal::SingleQuote),
    Entity::Literal(Literal::Backslash),
];
// @lfy def/grammar/terminals/literal.lfy:49
pub const SINGLE_QUOTE_BODY_ESCAPES: &[Entity] = &[
    Entity::Literal(Literal::BackslashEscape),
    Entity::Literal(Literal::SingleQuoteEscape),
    Entity::Literal(Literal::ContinuationEscape),
];
// @lfy def/grammar/terminals/literal.lfy:50
pub const DOUBLE_QUOTE_BODY_EXCLUDED: &[Entity] = &[
    Entity::Space(Space::NewLine),
    Entity::Literal(Literal::DoubleQuote),
    Entity::Literal(Literal::Backslash),
];
// @lfy def/grammar/terminals/literal.lfy:50
pub const DOUBLE_QUOTE_BODY_ESCAPES: &[Entity] = &[
    Entity::Literal(Literal::BackslashEscape),
    Entity::Literal(Literal::DoubleQuoteEscape),
    Entity::Literal(Literal::SingleQuoteEscape),
    Entity::Literal(Literal::NewlineEscape),
    Entity::Literal(Literal::CarriageReturnEscape),
    Entity::Literal(Literal::TabEscape),
    Entity::Literal(Literal::NullEscape),
    Entity::Literal(Literal::ContinuationEscape),
    Entity::Literal(Literal::HexEscape),
    Entity::Literal(Literal::UnicodeEscape),
];
// @lfy def/grammar/terminals/literal.lfy:65
pub const TEMPLATE_BODY_EXCLUDED: &[Entity] = &[
    Entity::Literal(Literal::Backtick),
    Entity::Literal(Literal::Backslash),
    Entity::Literal(Literal::ExecutionOpen),
    Entity::Literal(Literal::ReferenceOpen),
];
// @lfy def/grammar/terminals/literal.lfy:65
pub const TEMPLATE_BODY_ESCAPES: &[Entity] = &[
    Entity::Literal(Literal::BackslashEscape),
    Entity::Literal(Literal::BacktickEscape),
    Entity::Literal(Literal::SingleQuoteEscape),
    Entity::Literal(Literal::DoubleQuoteEscape),
    Entity::Literal(Literal::NewlineEscape),
    Entity::Literal(Literal::CarriageReturnEscape),
    Entity::Literal(Literal::TabEscape),
    Entity::Literal(Literal::NullEscape),
    Entity::Literal(Literal::ContinuationEscape),
    Entity::Literal(Literal::HexEscape),
    Entity::Literal(Literal::UnicodeEscape),
    Entity::Literal(Literal::ExecutionOpenEscape),
    Entity::Literal(Literal::ExecutionCloseEscape),
    Entity::Literal(Literal::ReferenceOpenEscape),
    Entity::Literal(Literal::ReferenceCloseEscape),
];

#[cfg(test)]
mod tests {
    use super::super::super::GrammarRule;
    use super::super::super::traits::body_value;
    use super::*;

    // @lfy def/grammar/terminals/literal.lfy:5
    #[test]
    fn character_classes_are_rules_but_not_terminals() {
        for class in [
            Literal::Character,
            Literal::Digit,
            Literal::HexDigit,
            Literal::Backslash,
        ] {
            assert!(!class.is_terminal(), "{}", class.identifier());
        }
        assert_eq!(Literal::Character.longest_match("日本"), Some(3));
        assert_eq!(Literal::Character.longest_match(""), None);
        assert_eq!(Literal::Digit.longest_match("42"), Some(1));
        assert_eq!(Literal::Digit.longest_match("x"), None);
        assert_eq!(Literal::HexDigit.longest_match("fF"), Some(1));
        assert_eq!(Literal::HexDigit.longest_match("g"), None);
        assert_eq!(Literal::Backslash.longest_match("\\\\"), Some(1));
        assert_eq!(Literal::Backslash.syntax(), "\"\\\"");
    }

    // @lfy def/grammar/terminals/literal.lfy:11
    #[test]
    fn numbers_are_digits_grouped_by_underscores_with_an_optional_fraction() {
        let number = Literal::NumberLiteral;
        assert_eq!(number.longest_match("1_000.5x"), Some(7));
        assert_eq!(number.longest_match("12__3"), Some(2));
        assert_eq!(number.longest_match("1_"), Some(1));
        assert_eq!(number.longest_match("1."), Some(1));
        assert_eq!(number.longest_match("1.5.3"), Some(3));
        assert_eq!(number.longest_match("1._5"), Some(1));
        assert_eq!(number.longest_match("_1"), None);
        assert!(number.matches("1_0.0_1"));
        assert!(!number.matches("1_"));
        // @lfy def/grammar/terminals/literal.lfy:14
        assert_eq!(number_value("1_000.000_1"), "1000.0001");
        assert_eq!(number_value("42"), "42");
    }

    // @lfy def/grammar/terminals/literal.lfy:18
    #[test]
    fn boundaries_are_their_quoted_text() {
        assert_eq!(Literal::SingleQuote.syntax(), "\"'\"");
        assert_eq!(Literal::DoubleQuote.syntax(), "'\"'");
        assert_eq!(Literal::Backtick.syntax(), "\"`\"");
        assert_eq!(Literal::ExecutionOpen.longest_match("{{x"), Some(2));
        assert_eq!(Literal::ReferenceClose.longest_match("]]"), Some(2));
        assert_eq!(Literal::ReferenceClose.longest_match("]"), None);
    }

    // @lfy def/grammar/terminals/literal.lfy:27
    #[test]
    fn every_escape_matches_its_own_spelling() {
        let cases: &[(Literal, &str, usize)] = &[
            (Literal::BackslashEscape, "\\\\", 2),
            (Literal::SingleQuoteEscape, "\\'", 2),
            (Literal::DoubleQuoteEscape, "\\\"", 2),
            (Literal::BacktickEscape, "\\`", 2),
            (Literal::NewlineEscape, "\\n", 2),
            (Literal::CarriageReturnEscape, "\\r", 2),
            (Literal::TabEscape, "\\t", 2),
            (Literal::NullEscape, "\\0", 2),
            (Literal::ContinuationEscape, "\\\n", 2),
            (Literal::ContinuationEscape, "\\\r\n", 3),
            (Literal::HexEscape, "\\x41", 4),
            (Literal::UnicodeEscape, "\\u0041", 6),
            (Literal::UnicodeEscape, "\\u0041BC", 8),
            (Literal::UnicodeEscape, "\\u0041B", 6),
            (Literal::ExecutionOpenEscape, "\\{{", 3),
            (Literal::ExecutionCloseEscape, "\\}}", 3),
            (Literal::ReferenceOpenEscape, "\\[[", 3),
            (Literal::ReferenceCloseEscape, "\\]]", 3),
        ];
        for &(escape, source, len) in cases {
            let text = format!("{source}tail");
            assert_eq!(
                escape.longest_match(&text),
                Some(len),
                "{}",
                escape.identifier()
            );
            assert!(escape.is_escape());
        }
        assert_eq!(Literal::HexEscape.longest_match("\\x4"), None);
        assert_eq!(Literal::UnicodeEscape.longest_match("\\u004"), None);
        assert_eq!(Literal::NewlineEscape.longest_match("\\\\n"), None);
    }

    // @lfy def/grammar/terminals/literal.lfy:41
    #[test]
    fn unicode_escapes_that_are_not_a_scalar_value_are_kept_as_written() {
        let double = Entity::Literal(Literal::DoubleQuoteBody);
        assert_eq!(body_value(double, "a\\u0042c"), "aBc");
        assert_eq!(body_value(double, "a\\uD800c"), "a\\uD800c");
        assert_eq!(body_value(double, "a\\u110000c"), "a\\u110000c");
    }

    // @lfy def/grammar/terminals/literal.lfy:49
    #[test]
    fn bodies_stop_at_their_boundaries_and_unlisted_escapes() {
        let single = Literal::SingleQuoteBody;
        assert_eq!(single.longest_match("it\\'s'"), Some(5));
        assert_eq!(single.longest_match("a\\nb'"), Some(1));
        assert_eq!(single.longest_match("a\nb"), Some(1));
        assert_eq!(single.longest_match("a\\\nb'"), Some(4));
        assert_eq!(single.longest_match("'"), None);
        let double = Literal::DoubleQuoteBody;
        assert_eq!(double.longest_match("a\\nb\"c"), Some(4));
        assert_eq!(double.longest_match("a\\qb"), Some(1));
        assert_eq!(double.longest_match("a\nb"), Some(1));
        assert_eq!(double.longest_match("a\\`b\""), Some(1));
        assert_eq!(double.longest_match("'`\\u01F600\""), Some(10));
        let template = Literal::TemplateBody;
        assert_eq!(template.longest_match("ab`c"), Some(2));
        assert_eq!(template.longest_match("a{{x}}"), Some(1));
        assert_eq!(template.longest_match("a[[x]]"), Some(1));
        assert_eq!(template.longest_match("a}}b]]c"), Some(7));
        assert_eq!(template.longest_match("a\\{{b`"), Some(5));
        assert_eq!(template.longest_match("a\\qb"), Some(1));
        assert_eq!(template.longest_match("x\ny`"), Some(3));
        assert_eq!(template.longest_match("`"), None);
        assert!(single.is_body() && double.is_body() && template.is_body());
    }

    // @lfy def/grammar/terminals/literal.lfy:87
    #[test]
    fn strings_are_a_boundary_an_optional_body_and_a_boundary() {
        assert!(Literal::SingleQuoteString.matches("'it\\'s'"));
        assert!(Literal::SingleQuoteString.matches("''"));
        assert!(!Literal::SingleQuoteString.matches("'a\nb'"));
        assert!(Literal::DoubleQuoteString.matches("\"a\\nb\""));
        assert!(!Literal::DoubleQuoteString.matches("\"a\\qb\""));
        assert!(!Literal::SingleQuoteString.is_terminal());
    }
}
