//! Compiled from `def/lexer/data.lfy`.

use std::fmt;
use std::sync::Arc;

use crate::grammar::{Entity, GrammarRule, Rule};

use super::modes::Mode;

/// One piece of the source text.
///
/// Joining the raw text of every token of a file in order reproduces the file exactly,
/// and the column is based on the raw text in the original source, not the adjusted value.
// @lfy def/lexer/data.lfy:3
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The grammar terminal whose syntax matched; `None` for text no terminal matched,
    /// which flags the token as invalid. [`Token::rule`] gives the terminal's EBNF form.
    pub rule: Option<Entity>, // @lfy def/lexer/data.lfy:4
    /// The characters exactly as written.
    pub raw: String, // @lfy def/lexer/data.lfy:5
    /// The characters after escapes are applied and number underscores are removed;
    /// equal to `raw` for every other token.
    pub value: String, // @lfy def/lexer/data.lfy:6
    /// The file the token came from.
    pub file: Arc<str>, // @lfy def/lexer/data.lfy:7
    /// Line the token starts on, counting from 1.
    pub line: usize, // @lfy def/lexer/data.lfy:8
    /// Column the token starts at within its line, counting from 0.
    pub column: usize, // @lfy def/lexer/data.lfy:9
}

impl Token {
    /// `$rule` as the EBNF form of the terminal that matched.
    // @lfy def/lexer/data.lfy:4
    pub fn rule(&self) -> Option<Rule> {
        self.rule.map(GrammarRule::rule)
    }

    /// Whether the token is flagged as invalid: no terminal matched its text.
    // @lfy def/lexer/data.lfy:15
    pub fn is_invalid(&self) -> bool {
        self.rule.is_none()
    }

    /// Whether the token was matched by this terminal.
    pub fn is<R: GrammarRule>(&self, rule: R) -> bool {
        self.rule == Some(rule.entity())
    }
}

/// One entry of the [`ModeStack`]: a mode together with the terminal whose token opened
/// it (`None` for the code mode the stack starts with).
// @lfy def/lexer/data.lfy:21
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeEntry {
    pub mode: Mode,
    pub opener: Option<Entity>,
}

/// Which region of the source the lexer is in, innermost last. The entry at the top
/// decides which terminals are candidates for the next token.
// @lfy def/lexer/data.lfy:18
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeStack {
    entries: Vec<ModeEntry>,
}

impl Default for ModeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl ModeStack {
    /// A stack holding the code mode, which it never loses.
    // @lfy def/lexer/data.lfy:20
    pub fn new() -> Self {
        ModeStack {
            entries: vec![ModeEntry {
                mode: Mode::Code,
                opener: None,
            }],
        }
    }

    /// A token opens a mode: the mode is pushed together with the rule that opened it.
    // @lfy def/lexer/data.lfy:21
    pub fn push(&mut self, mode: Mode, opener: Entity) {
        self.entries.push(ModeEntry {
            mode,
            opener: Some(opener),
        });
    }

    /// A token closes the mode at the top: the top entry is popped and returned. When
    /// `mode` is not at the top it is not a close and nothing changes. The code mode the
    /// stack starts with is never popped.
    // @lfy def/lexer/data.lfy:22
    pub fn pop(&mut self, mode: Mode) -> Option<ModeEntry> {
        if self.entries.len() > 1 && self.top().mode == mode {
            self.entries.pop() // @lfy def/lexer/data.lfy:22
        } else {
            None // @lfy def/lexer/data.lfy:23
        }
    }

    /// The entry at the top of the stack.
    // @lfy def/lexer/data.lfy:24
    pub fn top(&self) -> &ModeEntry {
        self.entries.last().expect("the stack is never empty")
    }

    /// The mode at the top of the stack.
    pub fn top_mode(&self) -> Mode {
        self.top().mode
    }

    /// Whether the stack holds only the code mode it started with.
    pub fn is_only_code(&self) -> bool {
        self.entries.len() == 1
    }

    /// The entries above the code mode the stack started with, bottom first.
    pub fn open(&self) -> &[ModeEntry] {
        &self.entries[1..]
    }

    /// All modes on the stack, bottom first.
    pub fn modes(&self) -> Vec<Mode> {
        self.entries.iter().map(|entry| entry.mode).collect()
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
    use crate::grammar::terminals::literal::Literal;

    fn backtick() -> Entity {
        Entity::Literal(Literal::Backtick)
    }

    // @lfy def/lexer/data.lfy:20
    #[test]
    fn the_stack_starts_with_code_and_is_never_empty() {
        let mut stack = ModeStack::new();
        assert_eq!(stack.top_mode(), Mode::Code);
        assert!(stack.is_only_code());
        assert_eq!(stack.pop(Mode::Code), None);
        assert_eq!(stack.modes(), vec![Mode::Code]);
        assert!(stack.open().is_empty());
    }

    // @lfy def/lexer/data.lfy:21
    #[test]
    fn opening_a_mode_pushes_it_with_its_opener() {
        let mut stack = ModeStack::new();
        stack.push(Mode::Template, backtick());
        stack.push(Mode::Execution, Entity::Literal(Literal::ExecutionOpen));
        assert_eq!(stack.top_mode(), Mode::Execution);
        assert_eq!(
            stack.top().opener,
            Some(Entity::Literal(Literal::ExecutionOpen))
        );
        assert_eq!(
            stack.modes(),
            vec![Mode::Code, Mode::Template, Mode::Execution]
        );
        assert_eq!(stack.open().len(), 2);
    }

    // @lfy def/lexer/data.lfy:22
    #[test]
    fn closing_pops_only_the_mode_at_the_top() {
        let mut stack = ModeStack::new();
        stack.push(Mode::Template, backtick());
        stack.push(Mode::Execution, Entity::Literal(Literal::ExecutionOpen));
        assert_eq!(stack.pop(Mode::Template), None);
        assert_eq!(stack.modes().len(), 3);
        let popped = stack.pop(Mode::Execution).unwrap();
        assert_eq!(popped.mode, Mode::Execution);
        assert_eq!(stack.modes(), vec![Mode::Code, Mode::Template]);
        assert!(stack.pop(Mode::Template).is_some());
        assert!(stack.is_only_code());
    }

    // @lfy def/lexer/data.lfy:4
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
}
