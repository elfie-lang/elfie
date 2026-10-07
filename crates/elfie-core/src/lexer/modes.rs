//! Compiled from `def/lexer/modes.lfy`.
//!
//! The regions in which each terminal may be lexed (`lexedIn`), `inCode`, and the other
//! traits of `def/lexer/traits.lfy` applied to each terminal.

use crate::grammar::Entity;
use crate::grammar::terminals::comment::Comment;
use crate::grammar::terminals::literal::Literal;

pub use super::traits::Mode;
use super::traits::ModeBehavior;

// Conditions

/// `const inCode`: anywhere ordinary tokens are read, including inside template
/// expressions and references.
// @lfy def/lexer/modes.lfy:inCode
pub const IN_CODE: &[Mode] = &[Mode::Code, Mode::Execution, Mode::Reference];

/// The regions in which the terminal may be lexed, as `lexedIn` declares; [`IN_CODE`] for
/// every terminal that is neither `lexedIn` nor an escape.
pub fn lex_condition(terminal: Entity) -> &'static [Mode] {
    match terminal {
        // Text regions: the boundary opens and closes the region; inside it only the body
        // and the boundary are lexed
        Entity::Literal(Literal::SingleQuote) => &[
            Mode::Code,
            Mode::Execution,
            Mode::Reference,
            Mode::SingleQuote,
        ],
        Entity::Literal(Literal::SingleQuoteBody) => &[Mode::SingleQuote],
        Entity::Literal(Literal::DoubleQuote) => &[
            Mode::Code,
            Mode::Execution,
            Mode::Reference,
            Mode::DoubleQuote,
        ],
        Entity::Literal(Literal::DoubleQuoteBody) => &[Mode::DoubleQuote],
        Entity::Literal(Literal::Backtick) => {
            &[Mode::Code, Mode::Execution, Mode::Reference, Mode::Template]
        }
        Entity::Literal(Literal::TemplateBody) => &[Mode::Template],
        Entity::Literal(Literal::ExecutionOpen) => &[Mode::Template],
        Entity::Literal(Literal::ExecutionClose) => &[Mode::Execution],
        Entity::Literal(Literal::ReferenceOpen) => &[
            Mode::Template,
            Mode::BlockDocumentation,
            Mode::LineDocumentation,
        ],
        Entity::Literal(Literal::ReferenceClose) => &[Mode::Reference],
        // Comments nest; documentation does not
        Entity::Comment(Comment::BlockCommentOpen) => {
            &[Mode::Code, Mode::BlockComment, Mode::Execution]
        }
        Entity::Comment(Comment::BlockCommentClose) => &[Mode::BlockComment],
        Entity::Comment(Comment::BlockCommentBody) => &[Mode::BlockComment],
        Entity::Comment(Comment::LineCommentOpen) => &[Mode::Code],
        Entity::Comment(Comment::LineCommentBody) => &[Mode::LineComment],
        Entity::Comment(Comment::BlockDocumentationOpen) => &[Mode::Code, Mode::Execution],
        Entity::Comment(Comment::BlockDocumentationClose) => &[Mode::BlockDocumentation],
        Entity::Comment(Comment::BlockDocumentationBody) => &[Mode::BlockDocumentation],
        Entity::Comment(Comment::LineDocumentationOpen) => &[Mode::Code],
        Entity::Comment(Comment::LineDocumentationBody) => &[Mode::LineDocumentation],
        // @lfy def/lexer/modes.lfy:inCode
        _ => IN_CODE,
    }
}

/// The mode traits (`opener`, `closer`, `delimiter`, `lineBounded`) applied to each
/// terminal.
pub fn mode_behaviors(terminal: Entity) -> &'static [ModeBehavior] {
    use ModeBehavior::{Closer, Delimiter, LineBounded, Opener};
    match terminal {
        Entity::Literal(Literal::SingleQuote) => &[
            Delimiter {
                mode: Mode::SingleQuote,
            },
            LineBounded {
                mode: Mode::SingleQuote,
            },
        ],
        Entity::Literal(Literal::DoubleQuote) => &[
            Delimiter {
                mode: Mode::DoubleQuote,
            },
            LineBounded {
                mode: Mode::DoubleQuote,
            },
        ],
        Entity::Literal(Literal::Backtick) => &[Delimiter {
            mode: Mode::Template,
        }],
        Entity::Literal(Literal::ExecutionOpen) => &[Opener {
            mode: Mode::Execution,
        }],
        Entity::Literal(Literal::ExecutionClose) => &[Closer {
            mode: Mode::Execution,
        }],
        Entity::Literal(Literal::ReferenceOpen) => &[Opener {
            mode: Mode::Reference,
        }],
        Entity::Literal(Literal::ReferenceClose) => &[Closer {
            mode: Mode::Reference,
        }],
        Entity::Comment(Comment::BlockCommentOpen) => &[Opener {
            mode: Mode::BlockComment,
        }],
        Entity::Comment(Comment::BlockCommentClose) => &[Closer {
            mode: Mode::BlockComment,
        }],
        Entity::Comment(Comment::LineCommentOpen) => &[
            Opener {
                mode: Mode::LineComment,
            },
            LineBounded {
                mode: Mode::LineComment,
            },
        ],
        Entity::Comment(Comment::BlockDocumentationOpen) => &[Opener {
            mode: Mode::BlockDocumentation,
        }],
        Entity::Comment(Comment::BlockDocumentationClose) => &[Closer {
            mode: Mode::BlockDocumentation,
        }],
        Entity::Comment(Comment::LineDocumentationOpen) => &[
            Opener {
                mode: Mode::LineDocumentation,
            },
            LineBounded {
                mode: Mode::LineDocumentation,
            },
        ],
        _ => &[],
    }
}

/// One entry of the [`ModeStack`]: a mode together with the terminal whose token opened
/// it (`None` for the entry the stack starts out with).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeEntry {
    pub mode: Mode,
    pub opener: Option<Entity>,
}

/// Which region of the source the lexer is in, innermost last.
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
    pub fn new() -> Self {
        ModeStack {
            entries: vec![ModeEntry {
                mode: Mode::Code,
                opener: None,
            }],
        }
    }

    /// Enters `mode`, recording `opener` as the terminal of the token that opened it.
    pub fn push(&mut self, mode: Mode, opener: Entity) {
        self.entries.push(ModeEntry {
            mode,
            opener: Some(opener),
        });
    }

    /// Leaves `mode` and gives back the entry that held it, or `None` when `mode` is not
    /// the one at the top. The entry the stack starts out with stays.
    pub fn pop(&mut self, mode: Mode) -> Option<ModeEntry> {
        if self.entries.len() > 1 && self.top().mode == mode {
            self.entries.pop()
        } else {
            None
        }
    }

    /// The innermost entry.
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

#[cfg(test)]
mod stack_tests {
    use super::*;

    fn backtick() -> Entity {
        Entity::Literal(Literal::Backtick)
    }

    #[test]
    fn the_stack_starts_with_code_and_is_never_empty() {
        let mut stack = ModeStack::new();
        assert_eq!(stack.top_mode(), Mode::Code);
        assert!(stack.is_only_code());
        assert_eq!(stack.pop(Mode::Code), None);
        assert_eq!(stack.modes(), vec![Mode::Code]);
        assert!(stack.open().is_empty());
    }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::terminals::keyword::Keyword;
    use crate::grammar::terminals::space::Space;
    use crate::grammar::{GrammarRule, rules};

    // @lfy def/lexer/modes.lfy:inCode
    #[test]
    fn terminals_without_a_condition_are_lexed_in_code() {
        assert_eq!(IN_CODE, &[Mode::Code, Mode::Execution, Mode::Reference]);
        assert_eq!(lex_condition(Entity::Keyword(Keyword::IfKeyword)), IN_CODE);
        assert_eq!(lex_condition(Entity::Space(Space::NewLine)), IN_CODE);
        assert_eq!(
            lex_condition(Entity::Comment(Comment::LineCommentOpen)),
            &[Mode::Code]
        );
        assert_eq!(
            lex_condition(Entity::Comment(Comment::LineDocumentationOpen)),
            &[Mode::Code]
        );
        assert_eq!(
            lex_condition(Entity::Comment(Comment::BlockDocumentationOpen)),
            &[Mode::Code, Mode::Execution]
        );
        assert_eq!(
            lex_condition(Entity::Comment(Comment::BlockCommentOpen)),
            &[Mode::Code, Mode::BlockComment, Mode::Execution]
        );
        assert_eq!(
            lex_condition(Entity::Literal(Literal::TemplateBody)),
            &[Mode::Template]
        );
    }

    #[test]
    fn every_body_is_lexed_only_inside_its_region() {
        for rule in rules().filter(|rule| rule.is_body()) {
            let condition = lex_condition(rule);
            assert_eq!(condition.len(), 1, "{rule}");
            assert!(!IN_CODE.contains(&condition[0]), "{rule}");
        }
    }

    #[test]
    fn only_boundaries_carry_mode_behaviors() {
        for rule in rules() {
            let behaviors = mode_behaviors(rule);
            let is_boundary = matches!(
                rule.category(),
                crate::grammar::Category::Terminal(crate::grammar::Terminal::Boundary { .. })
            );
            assert_eq!(!behaviors.is_empty(), is_boundary, "{rule}");
        }
        assert_eq!(
            mode_behaviors(Entity::Literal(Literal::SingleQuote)).len(),
            2
        );
        assert_eq!(mode_behaviors(Entity::Literal(Literal::Backtick)).len(), 1);
        assert_eq!(
            mode_behaviors(Entity::Comment(Comment::LineCommentOpen)).len(),
            2
        );
    }
}
