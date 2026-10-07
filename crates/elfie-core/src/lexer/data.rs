//! Compiled from `def/lexer/data.lfy`.

use std::fmt;
use std::sync::Arc;

use crate::grammar::{Entity, GrammarRule, Rule};

pub use super::modes::{ModeEntry, ModeStack};

/// One piece of the source text.
// @lfy def/lexer/data.lfy:Token
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The grammar terminal whose syntax matched; `None` marks an invalid token.
    /// [`Token::rule`] gives the terminal's EBNF form.
    pub rule: Option<Entity>, // @lfy def/lexer/data.lfy:Token.rule
    /// The characters exactly as written.
    pub raw: String, // @lfy def/lexer/data.lfy:Token.raw#Token:Token:08bc3c62e45e8c18474bd06c149c28ad4318a52cec71b1b096d1b3a2a415da9a
    /// The text the token stands for: `raw` unless its terminal gives another value.
    pub value: String, // @lfy def/lexer/data.lfy:Token.value
    /// The file the token came from.
    pub file: Arc<str>, // @lfy def/lexer/data.lfy:Token.file
    /// Line the token starts on, counting from 1.
    pub line: usize, // @lfy def/lexer/data.lfy:Token.line
    /// Column the token starts at within its line, counting from 0 in characters
    /// (Unicode scalar values), never in bytes or UTF-16 code units.
    pub column: usize, // @lfy def/lexer/data.lfy:Token.column#Token:Token:d6c6347909b76e848bd002d8acf5f71e8645247a33ba4d8c58b2fb5ab394ea0f
}

impl Token {
    /// `$rule` as the EBNF form of the terminal that matched.
    // @lfy def/lexer/data.lfy:Token.rule
    pub fn rule(&self) -> Option<Rule> {
        self.rule.map(GrammarRule::rule)
    }

    /// Whether no terminal matched this token's text.
    // @lfy def/lexer/data.lfy:Token.rule
    pub fn is_invalid(&self) -> bool {
        self.rule.is_none()
    }

    /// Whether the token was matched by this terminal.
    // @lfy def/lexer/data.lfy:Token.rule
    pub fn is<R: GrammarRule>(&self, rule: R) -> bool {
        self.rule == Some(rule.entity())
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rule = self.rule.map_or("invalid", |rule| rule.identifier());
        write!(
            f,
            "{}:{}:{} {rule} {:?}",
            self.file, self.line, self.column, self.raw
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::terminals::comment::Comment;
    use crate::lexer::lex;

    // @lfy def/lexer/data.lfy:Token
    #[test]
    fn a_token_records_its_rule_or_is_invalid() {
        let token = Token {
            rule: Some(Entity::Comment(Comment::LineCommentOpen)),
            raw: "//".into(),
            value: "//".into(),
            file: Arc::from("main.lfy"),
            line: 3,
            column: 7,
        };
        assert_eq!(token.rule().unwrap().text, "LineCommentOpen = \"//\" ;");
        assert!(token.is(Comment::LineCommentOpen));
        assert!(!token.is(Comment::LineCommentBody));
        assert!(!token.is_invalid());
        assert_eq!(token.to_string(), "main.lfy:3:7 LineCommentOpen \"//\"");
        let invalid = Token {
            rule: None,
            ..token.clone()
        };
        assert!(invalid.is_invalid());
        assert_eq!(invalid.rule(), None);
        assert_eq!(invalid.to_string(), "main.lfy:3:7 invalid \"//\"");
    }

    // @lfy def/lexer/data.lfy:Token.raw#Token:Token:08bc3c62e45e8c18474bd06c149c28ad4318a52cec71b1b096d1b3a2a415da9a
    #[test]
    fn joining_the_raw_text_of_every_token_reproduces_the_file() {
        let source = "d A: `x` {\n  /* é */ $b = 1_0;\r\n}\n// end é\n";
        let tokens = lex(source, Some("a.lfy")).unwrap();
        let joined: String = tokens.iter().map(|token| token.raw.as_str()).collect();
        assert_eq!(joined, source);
    }

    // @lfy def/lexer/data.lfy:Token.column#Token:Token:d6c6347909b76e848bd002d8acf5f71e8645247a33ba4d8c58b2fb5ab394ea0f
    #[test]
    fn columns_count_characters_of_the_raw_text() {
        let source = "/* é😀 */ abc 1_0\n";
        let tokens = lex(source, None).unwrap();
        for token in &tokens {
            let start = source
                .split('\n')
                .nth(token.line - 1)
                .unwrap()
                .chars()
                .take(token.column)
                .collect::<String>();
            let rest = &source[start.len()..];
            assert!(rest.starts_with(&token.raw), "{token}");
        }
        let abc = tokens.iter().find(|token| token.raw == "abc").unwrap();
        assert_eq!(abc.column, 9);
        let number = tokens.iter().find(|token| token.raw == "1_0").unwrap();
        assert_eq!(number.column, 13);
        assert_eq!(number.value, "10");
    }
}
