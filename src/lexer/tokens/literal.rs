//! Compiled from `def/lexer/tokens/literal.lfy`.

use std::borrow::Cow;

use super::super::data::{LexerModeStack, Mode};
use super::token_enum;
use super::traits::ModeBehavior;

/// Whether `mode` "is allowed to be created" on top of the current stack. Template
/// reference and execution modes are only allowed when a template literal is on top.
// @lfy def/lexer/tokens/literal.lfy:5
pub fn is_mode_allowed(mode: Mode, stack: &LexerModeStack) -> bool {
    match mode {
        Mode::TemplateReference | Mode::TemplateExecution => in_template(stack), // @lfy def/lexer/tokens/literal.lfy:6
        _ => true,
    }
}

/// `StringLiteralMode` is the top mode on the stack.
// @lfy def/lexer/tokens/literal.lfy:8
pub fn in_string(stack: &LexerModeStack) -> bool {
    stack.top_mode() == Some(Mode::StringLiteral)
}

/// `TemplateLiteralMode` is the top mode on the stack.
// @lfy def/lexer/tokens/literal.lfy:9
pub fn in_template(stack: &LexerModeStack) -> bool {
    stack.top_mode() == Some(Mode::TemplateLiteral)
}

/// `TemplateReferenceMode` is the top mode on the stack.
// @lfy def/lexer/tokens/literal.lfy:10
pub fn in_template_reference(stack: &LexerModeStack) -> bool {
    stack.top_mode() == Some(Mode::TemplateReference)
}

/// `TemplateExecutionMode` is the top mode on the stack.
// @lfy def/lexer/tokens/literal.lfy:11
pub fn in_template_execution(stack: &LexerModeStack) -> bool {
    stack.top_mode() == Some(Mode::TemplateExecution)
}

/// Condition of the string boundaries: the top of the stack is not `TemplateLiteralMode`.
// @lfy def/lexer/tokens/literal.lfy:14
pub fn top_is_not_template_literal(stack: &LexerModeStack) -> bool {
    !in_template(stack)
}

/// Condition of the template boundary: the top of the stack is not `StringLiteralMode`.
// @lfy def/lexer/tokens/literal.lfy:16
pub fn top_is_not_string_literal(stack: &LexerModeStack) -> bool {
    !in_string(stack)
}

token_enum! {
    /// String and template literal boundaries.
    // @lfy def/lexer/tokens/literal.lfy:13
    pub enum Literal {
        StringSingleBoundary => ("LT_STRING_SINGLE_BOUNDARY", "'", "a boundary character for a string"), // @lfy def/lexer/tokens/literal.lfy:14
        StringDoubleBoundary => ("LT_STRING_DOUBLE_BOUNDARY", "\"", "a boundary character for a string"), // @lfy def/lexer/tokens/literal.lfy:15
        StringTemplateBoundary => ("LT_STRING_TEMPLATE_BOUNDARY", "`", "a boundary character for a template string"), // @lfy def/lexer/tokens/literal.lfy:16
    }
}

const STRING_BOUNDARY: &[ModeBehavior] = &[ModeBehavior::Boundary {
    // @lfy def/lexer/tokens/literal.lfy:14
    mode: Mode::StringLiteral,
    condition: Some(top_is_not_template_literal),
}];
const TEMPLATE_BOUNDARY: &[ModeBehavior] = &[ModeBehavior::Boundary {
    // @lfy def/lexer/tokens/literal.lfy:16
    mode: Mode::TemplateLiteral,
    condition: Some(top_is_not_string_literal),
}];

impl Literal {
    /// The `modeBoundary` trait attached to this boundary.
    pub fn mode_behaviors(self) -> &'static [ModeBehavior] {
        match self {
            Literal::StringSingleBoundary | Literal::StringDoubleBoundary => STRING_BOUNDARY,
            Literal::StringTemplateBoundary => TEMPLATE_BOUNDARY,
        }
    }
}

token_enum! {
    /// Execution (`{{ }}`) and reference (`[[ ]]`) blocks inside template literals.
    // @lfy def/lexer/tokens/literal.lfy:19
    pub enum TemplateBlock {
        ExecutionOpen => ("TB_EXECUTION_OPEN", "{{", "an open token for an execution block within a template literal"), // @lfy def/lexer/tokens/literal.lfy:20
        ExecutionClose => ("TB_EXECUTION_CLOSE", "}}", "a close token for an execution block within a template literal"), // @lfy def/lexer/tokens/literal.lfy:21
        ReferenceOpen => ("TB_REFERENCE_OPEN", "[[", "an open token for a reference within a template literal"), // @lfy def/lexer/tokens/literal.lfy:22
        ReferenceClose => ("TB_REFERENCE_CLOSE", "]]", "a close token for a reference within a template literal"), // @lfy def/lexer/tokens/literal.lfy:23
    }
}

const EXECUTION_OPEN: &[ModeBehavior] = &[ModeBehavior::Creator {
    mode: Mode::TemplateExecution,
    condition: Some(in_template),
}]; // @lfy def/lexer/tokens/literal.lfy:20
const EXECUTION_CLOSE: &[ModeBehavior] = &[ModeBehavior::Destroyer {
    mode: Mode::TemplateExecution,
    condition: None,
}]; // @lfy def/lexer/tokens/literal.lfy:21
const REFERENCE_OPEN: &[ModeBehavior] = &[ModeBehavior::Creator {
    mode: Mode::TemplateReference,
    condition: Some(in_template),
}]; // @lfy def/lexer/tokens/literal.lfy:22
const REFERENCE_CLOSE: &[ModeBehavior] = &[ModeBehavior::Destroyer {
    mode: Mode::TemplateReference,
    condition: None,
}]; // @lfy def/lexer/tokens/literal.lfy:23

impl TemplateBlock {
    /// The mode traits attached to this block delimiter.
    pub fn mode_behaviors(self) -> &'static [ModeBehavior] {
        match self {
            TemplateBlock::ExecutionOpen => EXECUTION_OPEN,
            TemplateBlock::ExecutionClose => EXECUTION_CLOSE,
            TemplateBlock::ReferenceOpen => REFERENCE_OPEN,
            TemplateBlock::ReferenceClose => REFERENCE_CLOSE,
        }
    }
}

token_enum! {
    /// Escape sequences inside string and template literals. `value()` is the source
    /// spelling of the sequence; [`match_sequence`] yields what it is replaced with.
    // @lfy def/lexer/tokens/literal.lfy:26
    pub enum Sequence {
        Backslash => ("SQ_BACKSLASH", "\\\\", "literal backslash character"), // @lfy def/lexer/tokens/literal.lfy:27
        CarriageReturn => ("SQ_CARRIAGE_RETURN", "\\r", "literal carriage return character"), // @lfy def/lexer/tokens/literal.lfy:28
        ContinuationNewline => ("SQ_CONTINUATION_NEWLINE", "\\\n", "sequence to continue string through a newline character"), // @lfy def/lexer/tokens/literal.lfy:29
        ContinuationCrlf => ("SQ_CONTINUATION_CRLF", "\\\r\n", "sequence to continue a string through a newline"), // @lfy def/lexer/tokens/literal.lfy:30
        EncodingStart => ("SQ_ENCODING_START", "\\x", "character code encoding marker that is followed by exactly two hex digits for the character code to insert; For all purposes, the two hex digits are considered a part of the sequence"), // @lfy def/lexer/tokens/literal.lfy:31
        UnicodeEncodingStart => ("SQ_UNICODE_ENCODING_START", "\\u", "character code encoding marker that is followed by either four (with an assumed leading 00) or six hex digits for the 24-bit unicode character code to insert; For all purposes, the hex digits are considered a part of the sequence"), // @lfy def/lexer/tokens/literal.lfy:32
        EscapeExecutionStart => ("SQ_ESCAPE_EXECUTION_START", "\\{{", "literal opening execution curly braces to escape an execution start sequence"), // @lfy def/lexer/tokens/literal.lfy:33
        EscapeExecutionEnd => ("SQ_ESCAPE_EXECUTION_END", "\\}}", "literal closing execution curly braces to escape an execution end sequence"), // @lfy def/lexer/tokens/literal.lfy:34
        EscapeReferenceStart => ("SQ_ESCAPE_REFERENCE_START", "\\[[", "literal opening reference brackets to escape a reference start sequence"), // @lfy def/lexer/tokens/literal.lfy:35
        EscapeReferenceEnd => ("SQ_ESCAPE_REFERENCE_END", "\\]]", "literal closing reference brackets to escape a reference end sequence"), // @lfy def/lexer/tokens/literal.lfy:36
        Newline => ("SQ_NEWLINE", "\\n", "literal newline character"), // @lfy def/lexer/tokens/literal.lfy:37
        Null => ("SQ_NULL", "\\0", "literal null character"), // @lfy def/lexer/tokens/literal.lfy:38
        Tab => ("SQ_TAB", "\\t", "literal tab character"), // @lfy def/lexer/tokens/literal.lfy:39

        SingleQuote => ("SQ_SINGLE_QUOTE", "\\'", "literal single quote character"), // @lfy def/lexer/tokens/literal.lfy:41
        DoubleQuote => ("SQ_DOUBLE_QUOTE", "\\\"", "literal double quote character"), // @lfy def/lexer/tokens/literal.lfy:42
        Backtick => ("SQ_BACKTICK", "\\`", "literal backtick character"), // @lfy def/lexer/tokens/literal.lfy:43
    }
}

/// The leading run of ASCII hex digits in `text`, capped at `max` characters.
fn hex_prefix(text: &str, max: usize) -> &str {
    let len = text
        .bytes()
        .take(max)
        .take_while(u8::is_ascii_hexdigit)
        .count();
    &text[..len]
}

impl Sequence {
    /// Decodes this sequence at the start of `rest` (which already starts with
    /// `value()`), returning the total byte length consumed and the replacement text.
    /// `None` means the sequence does not fully match here and the text stays literal.
    fn decode(self, rest: &str) -> Option<(usize, Cow<'static, str>)> {
        let marker = self.value().len();
        let fixed = |text: &'static str| Some((marker, Cow::Borrowed(text)));
        match self {
            Sequence::Backslash => fixed("\\"), // @lfy def/lexer/tokens/literal.lfy:27
            Sequence::CarriageReturn => fixed("\r"), // @lfy def/lexer/tokens/literal.lfy:28
            Sequence::ContinuationNewline | Sequence::ContinuationCrlf => fixed(""), // @lfy def/lexer/tokens/literal.lfy:29
            // @lfy def/lexer/tokens/literal.lfy:31
            Sequence::EncodingStart => {
                let digits = hex_prefix(&rest[marker..], 2);
                if digits.len() != 2 {
                    return None;
                }
                let code = u32::from_str_radix(digits, 16).ok()?;
                let ch = char::from_u32(code)?;
                Some((marker + digits.len(), Cow::Owned(ch.to_string())))
            }
            // @lfy def/lexer/tokens/literal.lfy:32
            Sequence::UnicodeEncodingStart => {
                let digits = hex_prefix(&rest[marker..], 6);
                let used = match digits.len() {
                    6 => 6,
                    4 | 5 => 4,
                    _ => return None,
                };
                let code = u32::from_str_radix(&digits[..used], 16).ok()?;
                // @lfy def/lexer/tokens/literal.lfy:47
                let ch = char::from_u32(code)?;
                Some((marker + used, Cow::Owned(ch.to_string())))
            }
            Sequence::EscapeExecutionStart => fixed("{{"), // @lfy def/lexer/tokens/literal.lfy:33
            Sequence::EscapeExecutionEnd => fixed("}}"),   // @lfy def/lexer/tokens/literal.lfy:34
            Sequence::EscapeReferenceStart => fixed("[["), // @lfy def/lexer/tokens/literal.lfy:35
            Sequence::EscapeReferenceEnd => fixed("]]"),   // @lfy def/lexer/tokens/literal.lfy:36
            Sequence::Newline => fixed("\n"),              // @lfy def/lexer/tokens/literal.lfy:37
            Sequence::Null => fixed("\0"),                 // @lfy def/lexer/tokens/literal.lfy:38
            Sequence::Tab => fixed("\t"),                  // @lfy def/lexer/tokens/literal.lfy:39
            Sequence::SingleQuote => fixed("'"),           // @lfy def/lexer/tokens/literal.lfy:41
            Sequence::DoubleQuote => fixed("\""),          // @lfy def/lexer/tokens/literal.lfy:42
            Sequence::Backtick => fixed("`"),              // @lfy def/lexer/tokens/literal.lfy:43
        }
    }
}

/// Matches an escape sequence at the start of `rest`, returning the sequence, the byte
/// length consumed and the text it is replaced with in the body content. Among
/// competing matches the longest wins (maximal munch).
// @lfy def/lexer/main.lfy:47
pub fn match_sequence(rest: &str) -> Option<(Sequence, usize, Cow<'static, str>)> {
    let mut best: Option<(Sequence, usize, Cow<'static, str>)> = None;
    for (sequence, _) in Sequence::matches_at(rest) {
        let Some((len, replacement)) = sequence.decode(rest) else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, best_len, _)| len > *best_len) {
            best = Some((sequence, len, replacement));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::super::super::data::TokenKind;
    use super::*;

    fn replacement(text: &str) -> Option<(Sequence, usize, String)> {
        match_sequence(text).map(|(sequence, len, value)| (sequence, len, value.into_owned()))
    }

    // @lfy def/lexer/tokens/literal.lfy:5
    #[test]
    fn template_block_modes_are_only_allowed_inside_a_template_literal() {
        let mut stack = LexerModeStack::new();
        assert!(!is_mode_allowed(Mode::TemplateReference, &stack));
        assert!(!is_mode_allowed(Mode::TemplateExecution, &stack));
        assert!(is_mode_allowed(Mode::StringLiteral, &stack));
        stack.push(
            Mode::TemplateLiteral,
            TokenKind::Literal(Literal::StringTemplateBoundary),
        );
        assert!(is_mode_allowed(Mode::TemplateReference, &stack));
        assert!(is_mode_allowed(Mode::TemplateExecution, &stack));
    }

    // @lfy def/lexer/tokens/literal.lfy:8
    #[test]
    fn mode_predicates_follow_the_top_of_the_stack() {
        let mut stack = LexerModeStack::new();
        assert!(top_is_not_template_literal(&stack) && top_is_not_string_literal(&stack));
        stack.push(
            Mode::StringLiteral,
            TokenKind::Literal(Literal::StringSingleBoundary),
        );
        assert!(in_string(&stack) && !top_is_not_string_literal(&stack));
        stack.push(
            Mode::TemplateLiteral,
            TokenKind::Literal(Literal::StringTemplateBoundary),
        );
        assert!(in_template(&stack) && !in_string(&stack) && !top_is_not_template_literal(&stack));
        stack.push(
            Mode::TemplateReference,
            TokenKind::TemplateBlock(TemplateBlock::ReferenceOpen),
        );
        assert!(in_template_reference(&stack) && !in_template(&stack));
        stack.push(
            Mode::TemplateExecution,
            TokenKind::TemplateBlock(TemplateBlock::ExecutionOpen),
        );
        assert!(in_template_execution(&stack) && !in_template_reference(&stack));
    }

    // @lfy def/lexer/tokens/literal.lfy:26
    #[test]
    fn fixed_sequences_are_replaced_by_their_literal_character() {
        let cases: &[(&str, Sequence, &str)] = &[
            ("\\\\", Sequence::Backslash, "\\"),
            ("\\r", Sequence::CarriageReturn, "\r"),
            ("\\\n", Sequence::ContinuationNewline, ""),
            ("\\\r\n", Sequence::ContinuationCrlf, ""),
            ("\\{{", Sequence::EscapeExecutionStart, "{{"),
            ("\\}}", Sequence::EscapeExecutionEnd, "}}"),
            ("\\[[", Sequence::EscapeReferenceStart, "[["),
            ("\\]]", Sequence::EscapeReferenceEnd, "]]"),
            ("\\n", Sequence::Newline, "\n"),
            ("\\0", Sequence::Null, "\0"),
            ("\\t", Sequence::Tab, "\t"),
            ("\\'", Sequence::SingleQuote, "'"),
            ("\\\"", Sequence::DoubleQuote, "\""),
            ("\\`", Sequence::Backtick, "`"),
        ];
        for &(source, sequence, expected) in cases {
            let text = format!("{source}tail");
            assert_eq!(
                replacement(&text),
                Some((sequence, source.len(), expected.to_string())),
                "{source:?}"
            );
        }
        assert_eq!(replacement("\\q"), None);
        assert_eq!(replacement("plain"), None);
    }

    // @lfy def/lexer/tokens/literal.lfy:31
    #[test]
    fn encoding_start_takes_exactly_two_hex_digits() {
        assert_eq!(
            replacement("\\x41BC"),
            Some((Sequence::EncodingStart, 4, "A".into()))
        );
        assert_eq!(
            replacement("\\xe9"),
            Some((Sequence::EncodingStart, 4, "é".into()))
        );
        assert_eq!(replacement("\\x4"), None);
        assert_eq!(replacement("\\xZZ"), None);
    }

    // @lfy def/lexer/tokens/literal.lfy:32
    #[test]
    fn unicode_encoding_start_takes_six_hex_digits_when_available_else_four() {
        assert_eq!(
            replacement("\\u0041"),
            Some((Sequence::UnicodeEncodingStart, 6, "A".into()))
        );
        assert_eq!(
            replacement("\\u0041Z"),
            Some((Sequence::UnicodeEncodingStart, 6, "A".into()))
        );
        assert_eq!(
            replacement("\\u0041B"),
            Some((Sequence::UnicodeEncodingStart, 6, "A".into()))
        );
        assert_eq!(
            replacement("\\u000041"),
            Some((Sequence::UnicodeEncodingStart, 8, "A".into()))
        );
        assert_eq!(
            replacement("\\u01F600"),
            Some((Sequence::UnicodeEncodingStart, 8, "😀".into()))
        );
        assert_eq!(
            replacement("\\u0041BC"),
            Some((Sequence::UnicodeEncodingStart, 8, "\u{41BC}".into()))
        );
        assert_eq!(replacement("\\u004"), None);
    }

    // @lfy def/lexer/tokens/literal.lfy:47
    #[test]
    fn unicode_encoding_of_a_non_scalar_value_stays_literal() {
        assert_eq!(replacement("\\uD800"), None);
        assert_eq!(replacement("\\uFFFFFF"), None);
        assert_eq!(replacement("\\u110000"), None);
    }
}
