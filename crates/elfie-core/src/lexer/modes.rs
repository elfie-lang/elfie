//! Compiled from `def/lexer/modes.lfy`.
//!
//! The lexing modes, the condition under which each terminal is a candidate
//! (`candidateInModes`), and the mode traits of `def/lexer/traits.lfy` applied to each
//! terminal.

use std::fmt;

use crate::grammar::Entity;
use crate::grammar::terminals::comment::Comment;
use crate::grammar::terminals::literal::Literal;

use super::traits::ModeBehavior;

/// Regions of the source that change which terminals may match.
// @lfy def/lexer/modes.lfy:Mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Code,               // @lfy def/lexer/modes.lfy:Mode.code
    BlockComment,       // @lfy def/lexer/modes.lfy:Mode.blockComment
    LineComment,        // @lfy def/lexer/modes.lfy:Mode.lineComment
    BlockDocumentation, // @lfy def/lexer/modes.lfy:Mode.blockDocumentation
    LineDocumentation,  // @lfy def/lexer/modes.lfy:Mode.lineDocumentation
    SingleQuote,        // @lfy def/lexer/modes.lfy:Mode.singleQuote
    DoubleQuote,        // @lfy def/lexer/modes.lfy:Mode.doubleQuote
    Template,           // @lfy def/lexer/modes.lfy:Mode.template
    Execution,          // @lfy def/lexer/modes.lfy:Mode.execution
    Reference,          // @lfy def/lexer/modes.lfy:Mode.reference
}

impl Mode {
    /// Every mode, in declaration order.
    pub const ALL: &'static [Mode] = &[
        Mode::Code,
        Mode::BlockComment,
        Mode::LineComment,
        Mode::BlockDocumentation,
        Mode::LineDocumentation,
        Mode::SingleQuote,
        Mode::DoubleQuote,
        Mode::Template,
        Mode::Execution,
        Mode::Reference,
    ];

    /// The value the mode was declared with.
    pub const fn name(self) -> &'static str {
        match self {
            Mode::Code => "code",                  // @lfy def/lexer/modes.lfy:Mode.code
            Mode::BlockComment => "block comment", // @lfy def/lexer/modes.lfy:Mode.blockComment
            Mode::LineComment => "line comment",   // @lfy def/lexer/modes.lfy:Mode.lineComment
            Mode::BlockDocumentation => "block documentation", // @lfy def/lexer/modes.lfy:Mode.blockDocumentation
            Mode::LineDocumentation => "line documentation", // @lfy def/lexer/modes.lfy:Mode.lineDocumentation
            Mode::SingleQuote => "single quoted string", // @lfy def/lexer/modes.lfy:Mode.singleQuote
            Mode::DoubleQuote => "double quoted string", // @lfy def/lexer/modes.lfy:Mode.doubleQuote
            Mode::Template => "template",          // @lfy def/lexer/modes.lfy:Mode.template
            Mode::Execution => "template execution", // @lfy def/lexer/modes.lfy:Mode.execution
            Mode::Reference => "template reference", // @lfy def/lexer/modes.lfy:Mode.reference
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// Conditions

/// `const inCode`: anywhere ordinary tokens are read, including inside template
/// expressions and references.
// @lfy def/lexer/modes.lfy:inCode
pub const IN_CODE: &[Mode] = &[Mode::Code, Mode::Execution, Mode::Reference];

/// `$lexCondition`: the modes in which the terminal is a candidate, as `candidateInModes`
/// declares; [`IN_CODE`] for every terminal without a condition of its own.
// @lfy def/lexer/traits.lfy:candidateInModes.lexCondition
pub fn lex_condition(terminal: Entity) -> &'static [Mode] {
    match terminal {
        // Text regions: the boundary opens the mode, only the body and the close are
        // candidates inside it
        Entity::Literal(Literal::SingleQuote) => &[
            Mode::Code,
            Mode::Execution,
            Mode::Reference,
            Mode::SingleQuote,
        ], // @lfy def/grammar/terminals/literal.lfy:SingleQuote.lexCondition
        Entity::Literal(Literal::SingleQuoteBody) => &[Mode::SingleQuote], // @lfy def/grammar/terminals/literal.lfy:SingleQuoteBody.lexCondition
        Entity::Literal(Literal::DoubleQuote) => &[
            Mode::Code,
            Mode::Execution,
            Mode::Reference,
            Mode::DoubleQuote,
        ], // @lfy def/grammar/terminals/literal.lfy:DoubleQuote.lexCondition
        Entity::Literal(Literal::DoubleQuoteBody) => &[Mode::DoubleQuote], // @lfy def/grammar/terminals/literal.lfy:DoubleQuoteBody.lexCondition
        Entity::Literal(Literal::Backtick) => {
            &[Mode::Code, Mode::Execution, Mode::Reference, Mode::Template]
        } // @lfy def/grammar/terminals/literal.lfy:Backtick.lexCondition
        Entity::Literal(Literal::TemplateBody) => &[Mode::Template], // @lfy def/grammar/terminals/literal.lfy:TemplateBody.lexCondition
        Entity::Literal(Literal::ExecutionOpen) => &[Mode::Template], // @lfy def/grammar/terminals/literal.lfy:ExecutionOpen.lexCondition
        Entity::Literal(Literal::ExecutionClose) => &[Mode::Execution], // @lfy def/grammar/terminals/literal.lfy:ExecutionClose.lexCondition
        Entity::Literal(Literal::ReferenceOpen) => &[
            Mode::Template,
            Mode::BlockDocumentation,
            Mode::LineDocumentation,
        ], // @lfy def/grammar/terminals/literal.lfy:ReferenceOpen.lexCondition
        Entity::Literal(Literal::ReferenceClose) => &[Mode::Reference], // @lfy def/grammar/terminals/literal.lfy:ReferenceClose.lexCondition
        // Comments nest; documentation does not
        Entity::Comment(Comment::BlockCommentOpen) => &[Mode::Code, Mode::BlockComment, Mode::Execution], // @lfy def/grammar/terminals/comment.lfy:BlockCommentOpen.lexCondition
        Entity::Comment(Comment::LineCommentOpen) => &[Mode::Code], // @lfy def/grammar/terminals/comment.lfy:LineCommentOpen.lexCondition
        Entity::Comment(Comment::BlockCommentClose) => &[Mode::BlockComment], // @lfy def/grammar/terminals/comment.lfy:BlockCommentClose.lexCondition
        Entity::Comment(Comment::BlockCommentBody) => &[Mode::BlockComment], // @lfy def/grammar/terminals/comment.lfy:BlockCommentBody.lexCondition
        Entity::Comment(Comment::LineCommentBody) => &[Mode::LineComment], // @lfy def/grammar/terminals/comment.lfy:LineCommentBody.lexCondition
        Entity::Comment(Comment::BlockDocumentationOpen) => &[Mode::Code, Mode::Execution], // @lfy def/grammar/terminals/comment.lfy:BlockDocumentationOpen.lexCondition
        Entity::Comment(Comment::BlockDocumentationClose) => &[Mode::BlockDocumentation], // @lfy def/grammar/terminals/comment.lfy:BlockDocumentationClose.lexCondition
        Entity::Comment(Comment::BlockDocumentationBody) => &[Mode::BlockDocumentation], // @lfy def/grammar/terminals/comment.lfy:BlockDocumentationBody.lexCondition
        Entity::Comment(Comment::LineDocumentationOpen) => &[Mode::Code], // @lfy def/grammar/terminals/comment.lfy:LineDocumentationOpen.lexCondition
        Entity::Comment(Comment::LineDocumentationBody) => &[Mode::LineDocumentation], // @lfy def/grammar/terminals/comment.lfy:LineDocumentationBody.lexCondition
        // @lfy def/lexer/main.lfy:lex
        _ => IN_CODE,
    }
}

/// The mode traits applied to each terminal.
// @lfy def/lexer/traits.lfy:modeOpener
pub fn mode_behaviors(terminal: Entity) -> &'static [ModeBehavior] {
    match terminal {
        Entity::Literal(Literal::SingleQuote) => &[
            ModeBehavior::Toggle {
                mode: Mode::SingleQuote,
                when: Some(IN_CODE),
            }, // @lfy def/grammar/terminals/literal.lfy:SingleQuote
            ModeBehavior::AppliedUntilNewLine {
                mode: Mode::SingleQuote,
            }, // @lfy def/grammar/terminals/literal.lfy:SingleQuote
        ],
        Entity::Literal(Literal::DoubleQuote) => &[
            ModeBehavior::Toggle {
                mode: Mode::DoubleQuote,
                when: Some(IN_CODE),
            }, // @lfy def/grammar/terminals/literal.lfy:DoubleQuote
            ModeBehavior::AppliedUntilNewLine {
                mode: Mode::DoubleQuote,
            }, // @lfy def/grammar/terminals/literal.lfy:DoubleQuote
        ],
        Entity::Literal(Literal::Backtick) => &[ModeBehavior::Toggle {
            mode: Mode::Template,
            when: Some(IN_CODE),
        }], // @lfy def/grammar/terminals/literal.lfy:Backtick
        Entity::Literal(Literal::ExecutionOpen) => &[ModeBehavior::Opener {
            mode: Mode::Execution,
            when: Some(&[Mode::Template]),
        }], // @lfy def/grammar/terminals/literal.lfy:ExecutionOpen
        Entity::Literal(Literal::ExecutionClose) => &[ModeBehavior::Closer {
            mode: Mode::Execution,
        }], // @lfy def/grammar/terminals/literal.lfy:ExecutionClose
        Entity::Literal(Literal::ReferenceOpen) => &[ModeBehavior::Opener {
            mode: Mode::Reference,
            when: Some(&[
                Mode::Template,
                Mode::BlockDocumentation,
                Mode::LineDocumentation,
            ]),
        }], // @lfy def/grammar/terminals/literal.lfy:ReferenceOpen
        Entity::Literal(Literal::ReferenceClose) => &[ModeBehavior::Closer {
            mode: Mode::Reference,
        }], // @lfy def/grammar/terminals/literal.lfy:ReferenceClose
        Entity::Comment(Comment::BlockCommentOpen) => &[ModeBehavior::Opener {
            mode: Mode::BlockComment,
            when: Some(&[Mode::Code, Mode::BlockComment, Mode::Execution]),
        }], // @lfy def/grammar/terminals/comment.lfy:BlockCommentOpen
        Entity::Comment(Comment::BlockCommentClose) => &[ModeBehavior::Closer {
            mode: Mode::BlockComment,
        }], // @lfy def/grammar/terminals/comment.lfy:BlockCommentClose
        Entity::Comment(Comment::LineCommentOpen) => &[
            ModeBehavior::Opener {
                mode: Mode::LineComment,
                when: Some(&[Mode::Code]),
            }, // @lfy def/grammar/terminals/comment.lfy:LineCommentOpen
            ModeBehavior::AppliedUntilNewLine {
                mode: Mode::LineComment,
            }, // @lfy def/grammar/terminals/comment.lfy:LineCommentOpen
        ],
        Entity::Comment(Comment::BlockDocumentationOpen) => &[ModeBehavior::Opener {
            mode: Mode::BlockDocumentation,
            when: Some(&[Mode::Code, Mode::Execution]),
        }], // @lfy def/grammar/terminals/comment.lfy:BlockDocumentationOpen
        Entity::Comment(Comment::BlockDocumentationClose) => &[ModeBehavior::Closer {
            mode: Mode::BlockDocumentation,
        }], // @lfy def/grammar/terminals/comment.lfy:BlockDocumentationClose
        Entity::Comment(Comment::LineDocumentationOpen) => &[
            ModeBehavior::Opener {
                mode: Mode::LineDocumentation,
                when: Some(&[Mode::Code]),
            }, // @lfy def/grammar/terminals/comment.lfy:LineDocumentationOpen
            ModeBehavior::AppliedUntilNewLine {
                mode: Mode::LineDocumentation,
            }, // @lfy def/grammar/terminals/comment.lfy:LineDocumentationOpen
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::terminals::keyword::Keyword;
    use crate::grammar::terminals::space::Space;
    use crate::grammar::{GrammarRule, rules};

    // @lfy def/lexer/modes.lfy:Mode
    #[test]
    fn mode_names_match_their_declarations() {
        assert_eq!(Mode::ALL.len(), 10);
        assert_eq!(Mode::Code.name(), "code");
        assert_eq!(Mode::BlockComment.name(), "block comment");
        assert_eq!(Mode::LineDocumentation.name(), "line documentation");
        assert_eq!(Mode::SingleQuote.name(), "single quoted string");
        assert_eq!(Mode::Execution.to_string(), "template execution");
        assert_eq!(Mode::Reference.to_string(), "template reference");
    }

    // @lfy def/lexer/modes.lfy:inCode
    #[test]
    fn terminals_without_a_condition_are_candidates_in_code() {
        assert_eq!(IN_CODE, &[Mode::Code, Mode::Execution, Mode::Reference]);
        assert_eq!(lex_condition(Entity::Keyword(Keyword::IfKeyword)), IN_CODE);
        assert_eq!(lex_condition(Entity::Space(Space::NewLine)), IN_CODE);
        assert_eq!(lex_condition(Entity::Comment(Comment::LineCommentOpen)), &[Mode::Code]);
        assert_eq!(lex_condition(Entity::Comment(Comment::LineDocumentationOpen)), &[Mode::Code]);
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

    // @lfy def/lexer/traits.lfy:candidateInModes
    #[test]
    fn every_body_is_a_candidate_only_inside_its_region() {
        for rule in rules().filter(|rule| rule.is_body()) {
            let condition = lex_condition(rule);
            assert_eq!(condition.len(), 1, "{rule}");
            assert!(!IN_CODE.contains(&condition[0]), "{rule}");
        }
    }

    // @lfy def/lexer/traits.lfy:modeOpener
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
