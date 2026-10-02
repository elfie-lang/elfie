//! Compiled from `def/lexer/data.lfy`.

use std::fmt;
use std::sync::Arc;

use crate::grammar::{Entity, GrammarRule, Rule};

use super::modes::Mode;

/// One piece of the source text.
// @lfy def/lexer/data.lfy:Token
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The grammar terminal whose syntax matched; `None` for text no terminal matched.
    /// [`Token::rule`] gives the terminal's EBNF form.
    pub rule: Option<Entity>, // @lfy def/lexer/data.lfy:Token.rule
    /// The characters exactly as written.
    pub raw: String, // @lfy def/lexer/data.lfy:Token.raw#Token:Token:08bc3c62e45e8c18474bd06c149c28ad4318a52cec71b1b096d1b3a2a415da9a
    /// The characters after escapes are applied and number underscores are removed;
    /// equal to `raw` for every other token.
    pub value: String, // @lfy def/lexer/data.lfy:Token.value
    /// The file the token came from.
    pub file: Arc<str>, // @lfy def/lexer/data.lfy:Token.file
    /// Line the token starts on, counting from 1.
    pub line: usize, // @lfy def/lexer/data.lfy:Token.line
    /// Column the token starts at within its line, counting from 0.
    pub column: usize, // @lfy def/lexer/data.lfy:Token.column#Token:Token:d6c6347909b76e848bd002d8acf5f71e8645247a33ba4d8c58b2fb5ab394ea0f
}

impl Token {
    /// `$rule` as the EBNF form of the terminal that matched.
    // @lfy def/lexer/data.lfy:Token.rule
    pub fn rule(&self) -> Option<Rule> {
        self.rule.map(GrammarRule::rule)
    }

    /// Whether no terminal matched this token's text.
    // @lfy def/lexer/data.lfy:Token#Token:Token:8b5e1090119d07d0f077df14acc623339caddfbe3e7b6c3ebd0011d54ef831e6
    pub fn is_invalid(&self) -> bool {
        self.rule.is_none()
    }

    /// Whether the token was matched by this terminal.
    pub fn is<R: GrammarRule>(&self, rule: R) -> bool {
        self.rule == Some(rule.entity())
    }
}

/// One entry of the [`ModeStack`]: a mode together with the terminal whose token opened
/// it (`None` for the entry the stack starts out with).
// @lfy def/lexer/data.lfy:ModeStack
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeEntry {
    pub mode: Mode,
    pub opener: Option<Entity>,
}

/// Which region of the source the lexer is in, innermost last.
// @lfy def/lexer/data.lfy:ModeStack
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
    /// A stack at the outermost region of a file.
    // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:bf411502468e506f2012f5c6a63631b0ef4f5dea1eb101d6df718011283b76c5
    pub fn new() -> Self {
        ModeStack {
            entries: vec![ModeEntry {
                mode: Mode::Code,
                opener: None,
            }],
        }
    }

    /// Enters `mode`, recording `opener` as the terminal of the token that opened it.
    // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:f82916659858a2376833b32b3836b29a21d2022c9ffa32e2e46e89b80389fd17
    pub fn push(&mut self, mode: Mode, opener: Entity) {
        self.entries.push(ModeEntry {
            mode,
            opener: Some(opener),
        });
    }

    /// Leaves `mode` and gives back the entry that held it, or `None` when `mode` is not
    /// the one at the top. The entry the stack starts out with stays.
    // @lfy def/lexer/data.lfy:ModeStack
    pub fn pop(&mut self, mode: Mode) -> Option<ModeEntry> {
        if self.entries.len() > 1 && self.top().mode == mode {
            self.entries.pop() // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:e2b0ccde26274c5ce305ff388c5f304553e0795d8ad22face285038ed21c3e18
        } else {
            None // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:c3c805728eb6eef2d1af6e837dd8b9b7f93e1faaa789eb55668cfe5d02e3e51e
        }
    }

    /// The innermost entry.
    // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:7b1f41aa68081d63f57185763de9bb6a50a34befacdccf00852084711ce76388
    pub fn top(&self) -> &ModeEntry {
        self.entries.last().expect("the stack is never empty")
    }

    /// The mode of the innermost entry.
    pub fn top_mode(&self) -> Mode {
        self.top().mode
    }

    /// Whether no mode is open above the one the stack starts out with.
    pub fn is_only_code(&self) -> bool {
        self.entries.len() == 1
    }

    /// The entries above the one the stack starts out with, bottom first.
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

    // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:bf411502468e506f2012f5c6a63631b0ef4f5dea1eb101d6df718011283b76c5
    #[test]
    fn the_stack_starts_with_code_and_is_never_empty() {
        let mut stack = ModeStack::new();
        assert_eq!(stack.top_mode(), Mode::Code);
        assert!(stack.is_only_code());
        assert_eq!(stack.pop(Mode::Code), None);
        assert_eq!(stack.modes(), vec![Mode::Code]);
        assert!(stack.open().is_empty());
    }

    // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:f82916659858a2376833b32b3836b29a21d2022c9ffa32e2e46e89b80389fd17
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

    // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:e2b0ccde26274c5ce305ff388c5f304553e0795d8ad22face285038ed21c3e18
    // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:c3c805728eb6eef2d1af6e837dd8b9b7f93e1faaa789eb55668cfe5d02e3e51e
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

    // @lfy def/lexer/data.lfy:ModeStack#ModeStack:ModeStack:7b1f41aa68081d63f57185763de9bb6a50a34befacdccf00852084711ce76388
    #[test]
    fn the_entry_at_the_top_is_the_one_the_next_token_is_read_against() {
        let mut stack = ModeStack::new();
        stack.push(Mode::Template, backtick());
        assert_eq!(stack.top().mode, Mode::Template);
        assert_eq!(stack.top().opener, Some(backtick()));
        stack.push(Mode::Reference, Entity::Literal(Literal::ReferenceOpen));
        assert_eq!(stack.top().mode, Mode::Reference);
        assert_eq!(stack.top_mode(), *stack.modes().last().unwrap());
        stack.pop(Mode::Reference);
        assert_eq!(stack.top().mode, Mode::Template);
    }

    // @lfy def/lexer/data.lfy:Token.rule
    // @lfy def/lexer/data.lfy:Token#Token:Token:8b5e1090119d07d0f077df14acc623339caddfbe3e7b6c3ebd0011d54ef831e6
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
