//! Compiled from `def/grammar/tokens/literal.lfy`.

use super::super::grammar_rules;

grammar_rules! {
    /// Literal rules. `UTF8_CHARACTER` is external, provided by the EBNF built-ins.
    #[allow(non_camel_case_types)]
    pub enum Literal {
        /// `external const`
        UTF8_CHARACTER = "? all UTF-8 compatible character codes ?", // @lfy def/grammar/tokens/literal.lfy:4

        /// a literal null value
        NullLiteral = r#""null""#, // @lfy def/grammar/tokens/literal.lfy:6
        /// a literal undefined value
        UndefinedLiteral = r#""undefined""#, // @lfy def/grammar/tokens/literal.lfy:7

        /// a boolean literal value
        BooleanLiteral = r#""true" | "false""#, // @lfy def/grammar/tokens/literal.lfy:9

        DigitLiteral = r#""0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9""#, // @lfy def/grammar/tokens/literal.lfy:11
        /// a number literal; underscores are removed from the value, see [`normalize_number`]
        NumberLiteral = r#"[[DigitLiteral]] , (/ (: [[DigitLiteral]] | "_" :) , [[DigitLiteral]] /) , (/ "." , [[DigitLiteral]] , (/ (: [[DigitLiteral]] | "_" :) , [[DigitLiteral]] /) /) "#, // @lfy def/grammar/tokens/literal.lfy:12

        HexCharacter = r#"[[DigitLiteral]] | "a" | "A" | "b" | "B" | "c" | "C" | "d" | "D" | "e" | "E" | "f" | "F""#, // @lfy def/grammar/tokens/literal.lfy:16

        /// a boundary character for a string
        SingleQuoteBoundary = r#""'""#, // @lfy def/grammar/tokens/literal.lfy:18
        /// a boundary character for a string
        DoubleQuoteBoundary = r#"'"'"#, // @lfy def/grammar/tokens/literal.lfy:19
        /// a boundary character for a template string
        BacktickBoundary = r#""`""#, // @lfy def/grammar/tokens/literal.lfy:20

        /// an open token for an execution block within a template literal
        ExecutionOpenBoundary = r#""{{""#, // @lfy def/grammar/tokens/literal.lfy:23
        /// a close token for an execution block within a template literal
        ExecutionCloseBoundary = r#""}}""#, // @lfy def/grammar/tokens/literal.lfy:24
        /// an open token for a reference within a template literal or documentation block
        ReferenceOpenBoundary = r#""[[""#, // @lfy def/grammar/tokens/literal.lfy:25
        /// a close token for a reference within a template literal or documentation block
        ReferenceCloseBoundary = r#""]]""#, // @lfy def/grammar/tokens/literal.lfy:26

        /// escape and replacement sequences in string and template bodies, decoded by
        /// [`decode_sequence`]
        Sequence = "( \"\\\\\" | \"\\r\" | \"\\\n\" | \"\\\r\n\" | (\"\\x\" , [[HexCharacter]] , [[HexCharacter]]) | (\"\\u\" , [[HexCharacter]] , [[HexCharacter]] , [[HexCharacter]] , [[HexCharacter]] , (/ [[HexCharacter]] , [[HexCharacter]] /) ) | \"\\{{\" | \"\\}}\" | \"\\[[\" | \"\\]]\" | \"\\n\" | \"\\0\" | \"\\t\" | \"\\'\" | '\\\"' | \"\\`\" )", // @lfy def/grammar/tokens/literal.lfy:28

        /// single quote string literal body contents
        SingleQuoteLiteralBody = "(: UTF8_CHARACTER - ( [[NewLine]] | [[SingleQuoteBoundary]] ) :)", // @lfy def/grammar/tokens/literal.lfy:50
        /// double quote string literal body contents
        DoubleQuoteLiteralBody = r#"(: ( UTF8_CHARACTER - ( [[NewLine]] | [[DoubleQuoteBoundary]] | "\" ) ) | [[Sequence]] :)"#, // @lfy def/grammar/tokens/literal.lfy:51

        SingleQuoteLiteral = "[[SingleQuoteBoundary]] , [[SingleQuoteLiteralBody]] , [[SingleQuoteBoundary]]", // @lfy def/grammar/tokens/literal.lfy:53
        DoubleQuoteLiteral = "[[DoubleQuoteBoundary]] , [[DoubleQuoteLiteralBody]] , [[DoubleQuoteBoundary]]", // @lfy def/grammar/tokens/literal.lfy:54
        StringLiteral = "[[SingleQuoteLiteral]] | [[DoubleQuoteLiteral]]", // @lfy def/grammar/tokens/literal.lfy:55
    }
}

/// The value of a matched [`Literal::NumberLiteral`]: the source text with underscores
/// removed, leaving only digits and `.` characters.
// @lfy def/grammar/tokens/literal.lfy:13
pub fn normalize_number(raw: &str) -> String {
    raw.chars().filter(|&c| c != '_').collect()
}

/// What a matched [`Literal::Sequence`] is replaced with in a token value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replacement {
    /// Fixed replacement text.
    Text(&'static str),
    /// A decoded character code.
    Char(char),
    /// The text is treated as literal and is not encoded as a sequence.
    Literal, // @lfy def/grammar/tokens/literal.lfy:47
}

/// Decodes the sequence at the start of `rest` (which must start with `\`): the byte
/// length of the whole sequence and its replacement, or `None` when no sequence starts here.
// @lfy def/grammar/tokens/literal.lfy:28
pub fn decode_sequence(rest: &str) -> Option<(usize, Replacement)> {
    use Replacement::{Char, Text};
    let after = rest.strip_prefix('\\')?;
    let decoded = match after.chars().next()? {
        '\\' => (2, Text("\\")), // @lfy def/grammar/tokens/literal.lfy:29
        'r' => (2, Text("\r")),  // @lfy def/grammar/tokens/literal.lfy:30
        '\n' => (2, Text("")),   // @lfy def/grammar/tokens/literal.lfy:31
        '\r' if after.starts_with("\r\n") => (3, Text("")), // @lfy def/grammar/tokens/literal.lfy:32
        // @lfy def/grammar/tokens/literal.lfy:33
        'x' => {
            let digits = hex_prefix(&after[1..], 2);
            if digits.len() != 2 {
                return None;
            }
            let code = u32::from_str_radix(digits, 16).ok()?;
            (4, Char(char::from_u32(code)?))
        }
        // @lfy def/grammar/tokens/literal.lfy:34
        'u' => {
            let digits = hex_prefix(&after[1..], 6);
            let used = match digits.len() {
                6 => 6,
                4 | 5 => 4,
                _ => return None,
            };
            let code = u32::from_str_radix(&digits[..used], 16).ok()?;
            // @lfy def/grammar/tokens/literal.lfy:47
            (
                2 + used,
                char::from_u32(code).map_or(Replacement::Literal, Char),
            )
        }
        '{' if after.starts_with("{{") => (3, Text("{{")), // @lfy def/grammar/tokens/literal.lfy:35
        '}' if after.starts_with("}}") => (3, Text("}}")), // @lfy def/grammar/tokens/literal.lfy:36
        '[' if after.starts_with("[[") => (3, Text("[[")), // @lfy def/grammar/tokens/literal.lfy:37
        ']' if after.starts_with("]]") => (3, Text("]]")), // @lfy def/grammar/tokens/literal.lfy:38
        'n' => (2, Text("\n")),                            // @lfy def/grammar/tokens/literal.lfy:39
        '0' => (2, Text("\0")),                            // @lfy def/grammar/tokens/literal.lfy:40
        't' => (2, Text("\t")),                            // @lfy def/grammar/tokens/literal.lfy:41
        '\'' => (2, Text("'")),                            // @lfy def/grammar/tokens/literal.lfy:43
        '"' => (2, Text("\"")),                            // @lfy def/grammar/tokens/literal.lfy:44
        '`' => (2, Text("`")),                             // @lfy def/grammar/tokens/literal.lfy:45
        _ => return None,
    };
    Some(decoded)
}

/// The leading run of ASCII hex digits in `text`, at most `max` of them.
fn hex_prefix(text: &str, max: usize) -> &str {
    let len = text
        .bytes()
        .take(max)
        .take_while(u8::is_ascii_hexdigit)
        .count();
    &text[..len]
}

#[cfg(test)]
mod tests {
    use super::super::super::EbnfSyntax;
    use super::Replacement::{Char, Text};
    use super::*;

    // @lfy def/grammar/tokens/literal.lfy:13
    #[test]
    fn underscores_are_removed_from_number_values() {
        assert_eq!(normalize_number("1_000.000_1"), "1000.0001");
        assert_eq!(normalize_number("42"), "42");
    }

    // @lfy def/grammar/tokens/literal.lfy:28
    #[test]
    fn every_sequence_in_the_syntax_decodes() {
        let cases: &[(&str, usize, Replacement)] = &[
            ("\\\\", 2, Text("\\")),   // @lfy def/grammar/tokens/literal.lfy:29
            ("\\r", 2, Text("\r")),    // @lfy def/grammar/tokens/literal.lfy:30
            ("\\\n", 2, Text("")),     // @lfy def/grammar/tokens/literal.lfy:31
            ("\\\r\n", 3, Text("")),   // @lfy def/grammar/tokens/literal.lfy:32
            ("\\x41", 4, Char('A')),   // @lfy def/grammar/tokens/literal.lfy:33
            ("\\u0041", 6, Char('A')), // @lfy def/grammar/tokens/literal.lfy:34
            ("\\{{", 3, Text("{{")),   // @lfy def/grammar/tokens/literal.lfy:35
            ("\\}}", 3, Text("}}")),   // @lfy def/grammar/tokens/literal.lfy:36
            ("\\[[", 3, Text("[[")),   // @lfy def/grammar/tokens/literal.lfy:37
            ("\\]]", 3, Text("]]")),   // @lfy def/grammar/tokens/literal.lfy:38
            ("\\n", 2, Text("\n")),    // @lfy def/grammar/tokens/literal.lfy:39
            ("\\0", 2, Text("\0")),    // @lfy def/grammar/tokens/literal.lfy:40
            ("\\t", 2, Text("\t")),    // @lfy def/grammar/tokens/literal.lfy:41
            ("\\'", 2, Text("'")),     // @lfy def/grammar/tokens/literal.lfy:43
            ("\\\"", 2, Text("\"")),   // @lfy def/grammar/tokens/literal.lfy:44
            ("\\`", 2, Text("`")),     // @lfy def/grammar/tokens/literal.lfy:45
        ];
        for &(source, len, replacement) in cases {
            let text = format!("{source}tail");
            assert_eq!(
                decode_sequence(&text),
                Some((len, replacement)),
                "{source:?}"
            );
            assert_eq!(
                Literal::Sequence.longest_match(&text),
                Some(len),
                "{source:?}"
            );
        }
        assert_eq!(decode_sequence("\\q"), None);
        assert_eq!(decode_sequence("plain"), None);
        assert_eq!(decode_sequence("\\"), None);
    }

    // @lfy def/grammar/tokens/literal.lfy:33
    #[test]
    fn encoding_start_takes_exactly_two_hex_digits() {
        assert_eq!(decode_sequence("\\x41BC"), Some((4, Char('A'))));
        assert_eq!(decode_sequence("\\xe9"), Some((4, Char('é'))));
        assert_eq!(decode_sequence("\\x4"), None);
        assert_eq!(decode_sequence("\\xZZ"), None);
        assert_eq!(Literal::Sequence.longest_match("\\x4"), None);
    }

    // @lfy def/grammar/tokens/literal.lfy:34
    #[test]
    fn unicode_encoding_start_takes_six_hex_digits_when_available_else_four() {
        assert_eq!(decode_sequence("\\u0041"), Some((6, Char('A'))));
        assert_eq!(decode_sequence("\\u0041Z"), Some((6, Char('A'))));
        assert_eq!(decode_sequence("\\u0041B"), Some((6, Char('A'))));
        assert_eq!(decode_sequence("\\u000041"), Some((8, Char('A'))));
        assert_eq!(decode_sequence("\\u01F600"), Some((8, Char('😀'))));
        assert_eq!(decode_sequence("\\u0041BC"), Some((8, Char('\u{41BC}'))));
        assert_eq!(decode_sequence("\\u004"), None);
        assert_eq!(Literal::Sequence.longest_match("\\u0041B"), Some(6));
        assert_eq!(Literal::Sequence.longest_match("\\u0041BC"), Some(8));
    }

    // @lfy def/grammar/tokens/literal.lfy:47
    #[test]
    fn unicode_encoding_of_a_non_scalar_value_is_literal_text() {
        assert_eq!(decode_sequence("\\uD800"), Some((6, Replacement::Literal)));
        assert_eq!(
            decode_sequence("\\uFFFFFF"),
            Some((8, Replacement::Literal))
        );
        assert_eq!(
            decode_sequence("\\u110000"),
            Some((8, Replacement::Literal))
        );
    }

    #[test]
    fn bodies_and_literals_match_as_declared() {
        assert_eq!(Literal::BooleanLiteral.longest_match("false!"), Some(5));
        assert_eq!(Literal::BooleanLiteral.longest_match("trueish"), Some(4));
        assert_eq!(Literal::BooleanLiteral.longest_match("truth"), None);
        assert_eq!(
            Literal::DoubleQuoteLiteralBody.longest_match("a\\nb\"c"),
            Some(4)
        );
        assert_eq!(
            Literal::DoubleQuoteLiteralBody.longest_match("a\\qb"),
            Some(1)
        );
        assert_eq!(
            Literal::DoubleQuoteLiteralBody.longest_match("a\nb"),
            Some(1)
        );
        assert_eq!(
            Literal::SingleQuoteLiteralBody.longest_match("a\\nb'c"),
            Some(4)
        );
    }
}
