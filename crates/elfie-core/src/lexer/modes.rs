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
// @lfy def/lexer/modes.lfy:10
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Code,               // @lfy def/lexer/modes.lfy:11
    BlockComment,       // @lfy def/lexer/modes.lfy:12
    LineComment,        // @lfy def/lexer/modes.lfy:13
    BlockDocumentation, // @lfy def/lexer/modes.lfy:14
    LineDocumentation,  // @lfy def/lexer/modes.lfy:15
    SingleQuote,        // @lfy def/lexer/modes.lfy:16
    DoubleQuote,        // @lfy def/lexer/modes.lfy:17
    Template,           // @lfy def/lexer/modes.lfy:18
    Execution,          // @lfy def/lexer/modes.lfy:19
    Reference,          // @lfy def/lexer/modes.lfy:20
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
            Mode::Code => "code",                  // @lfy def/lexer/modes.lfy:11
            Mode::BlockComment => "block comment", // @lfy def/lexer/modes.lfy:12
            Mode::LineComment => "line comment",   // @lfy def/lexer/modes.lfy:13
            Mode::BlockDocumentation => "block documentation", // @lfy def/lexer/modes.lfy:14
            Mode::LineDocumentation => "line documentation", // @lfy def/lexer/modes.lfy:15
            Mode::SingleQuote => "single quoted string", // @lfy def/lexer/modes.lfy:16
            Mode::DoubleQuote => "double quoted string", // @lfy def/lexer/modes.lfy:17
            Mode::Template => "template",          // @lfy def/lexer/modes.lfy:18
            Mode::Execution => "template execution", // @lfy def/lexer/modes.lfy:19
            Mode::Reference => "template reference", // @lfy def/lexer/modes.lfy:20
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
// @lfy def/lexer/modes.lfy:24
pub const IN_CODE: &[Mode] = &[Mode::Code, Mode::Execution, Mode::Reference];

/// `$lexCondition`: the modes in which the terminal is a candidate, as `candidateInModes`
/// declares; [`IN_CODE`] for every terminal without a condition of its own.
// @lfy def/lexer/traits.lfy:9
pub fn lex_condition(terminal: Entity) -> &'static [Mode] {
    match terminal {
        // Text regions: the boundary opens the mode, only the body and the close are
        // candidates inside it
        Entity::Literal(Literal::SingleQuote) => &[
            Mode::Code,
            Mode::Execution,
            Mode::Reference,
            Mode::SingleQuote,
        ], // @lfy def/lexer/modes.lfy:28
        Entity::Literal(Literal::SingleQuoteBody) => &[Mode::SingleQuote], // @lfy def/lexer/modes.lfy:30
        Entity::Literal(Literal::DoubleQuote) => &[
            Mode::Code,
            Mode::Execution,
            Mode::Reference,
            Mode::DoubleQuote,
        ], // @lfy def/lexer/modes.lfy:33
        Entity::Literal(Literal::DoubleQuoteBody) => &[Mode::DoubleQuote], // @lfy def/lexer/modes.lfy:35
        Entity::Literal(Literal::Backtick) => {
            &[Mode::Code, Mode::Execution, Mode::Reference, Mode::Template]
        } // @lfy def/lexer/modes.lfy:38
        Entity::Literal(Literal::TemplateBody) => &[Mode::Template], // @lfy def/lexer/modes.lfy:39
        Entity::Literal(Literal::ExecutionOpen) => &[Mode::Template], // @lfy def/lexer/modes.lfy:41
        Entity::Literal(Literal::ExecutionClose) => &[Mode::Execution], // @lfy def/lexer/modes.lfy:43
        Entity::Literal(Literal::ReferenceOpen) => &[
            Mode::Template,
            Mode::BlockDocumentation,
            Mode::LineDocumentation,
        ], // @lfy def/lexer/modes.lfy:45
        Entity::Literal(Literal::ReferenceClose) => &[Mode::Reference], // @lfy def/lexer/modes.lfy:47
        // Comments nest; documentation does not
        Entity::Comment(Comment::BlockCommentOpen) => &[Mode::Code, Mode::BlockComment, Mode::Execution], // @lfy def/lexer/modes.lfy:51
        Entity::Comment(Comment::LineCommentOpen) => &[Mode::Code], // @lfy def/lexer/modes.lfy:56
        Entity::Comment(Comment::BlockCommentClose) => &[Mode::BlockComment], // @lfy def/lexer/modes.lfy:53
        Entity::Comment(Comment::BlockCommentBody) => &[Mode::BlockComment], // @lfy def/lexer/modes.lfy:54
        Entity::Comment(Comment::LineCommentBody) => &[Mode::LineComment], // @lfy def/lexer/modes.lfy:58
        Entity::Comment(Comment::BlockDocumentationOpen) => &[Mode::Code, Mode::Execution], // @lfy def/lexer/modes.lfy:61
        Entity::Comment(Comment::BlockDocumentationClose) => &[Mode::BlockDocumentation], // @lfy def/lexer/modes.lfy:63
        Entity::Comment(Comment::BlockDocumentationBody) => &[Mode::BlockDocumentation], // @lfy def/lexer/modes.lfy:64
        Entity::Comment(Comment::LineDocumentationOpen) => &[Mode::Code], // @lfy def/lexer/modes.lfy:66
        Entity::Comment(Comment::LineDocumentationBody) => &[Mode::LineDocumentation], // @lfy def/lexer/modes.lfy:68
        // @lfy def/lexer/main.lfy:103
        _ => IN_CODE,
    }
}

/// The mode traits applied to each terminal.
// @lfy def/lexer/modes.lfy:27
pub fn mode_behaviors(terminal: Entity) -> &'static [ModeBehavior] {
    match terminal {
        Entity::Literal(Literal::SingleQuote) => &[
            ModeBehavior::Toggle {
                mode: Mode::SingleQuote,
                when: Some(IN_CODE),
            }, // @lfy def/lexer/modes.lfy:27
            ModeBehavior::AppliedUntilNewLine {
                mode: Mode::SingleQuote,
            }, // @lfy def/lexer/modes.lfy:29
        ],
        Entity::Literal(Literal::DoubleQuote) => &[
            ModeBehavior::Toggle {
                mode: Mode::DoubleQuote,
                when: Some(IN_CODE),
            }, // @lfy def/lexer/modes.lfy:32
            ModeBehavior::AppliedUntilNewLine {
                mode: Mode::DoubleQuote,
            }, // @lfy def/lexer/modes.lfy:34
        ],
        Entity::Literal(Literal::Backtick) => &[ModeBehavior::Toggle {
            mode: Mode::Template,
            when: Some(IN_CODE),
        }], // @lfy def/lexer/modes.lfy:37
        Entity::Literal(Literal::ExecutionOpen) => &[ModeBehavior::Opener {
            mode: Mode::Execution,
            when: Some(&[Mode::Template]),
        }], // @lfy def/lexer/modes.lfy:40
        Entity::Literal(Literal::ExecutionClose) => &[ModeBehavior::Closer {
            mode: Mode::Execution,
        }], // @lfy def/lexer/modes.lfy:42
        Entity::Literal(Literal::ReferenceOpen) => &[ModeBehavior::Opener {
            mode: Mode::Reference,
            when: Some(&[
                Mode::Template,
                Mode::BlockDocumentation,
                Mode::LineDocumentation,
            ]),
        }], // @lfy def/lexer/modes.lfy:44
        Entity::Literal(Literal::ReferenceClose) => &[ModeBehavior::Closer {
            mode: Mode::Reference,
        }], // @lfy def/lexer/modes.lfy:46
        Entity::Comment(Comment::BlockCommentOpen) => &[ModeBehavior::Opener {
            mode: Mode::BlockComment,
            when: Some(&[Mode::Code, Mode::BlockComment, Mode::Execution]),
        }], // @lfy def/lexer/modes.lfy:50
        Entity::Comment(Comment::BlockCommentClose) => &[ModeBehavior::Closer {
            mode: Mode::BlockComment,
        }], // @lfy def/lexer/modes.lfy:52
        Entity::Comment(Comment::LineCommentOpen) => &[
            ModeBehavior::Opener {
                mode: Mode::LineComment,
                when: Some(&[Mode::Code]),
            }, // @lfy def/lexer/modes.lfy:55
            ModeBehavior::AppliedUntilNewLine {
                mode: Mode::LineComment,
            }, // @lfy def/lexer/modes.lfy:57
        ],
        Entity::Comment(Comment::BlockDocumentationOpen) => &[ModeBehavior::Opener {
            mode: Mode::BlockDocumentation,
            when: Some(&[Mode::Code, Mode::Execution]),
        }], // @lfy def/lexer/modes.lfy:60
        Entity::Comment(Comment::BlockDocumentationClose) => &[ModeBehavior::Closer {
            mode: Mode::BlockDocumentation,
        }], // @lfy def/lexer/modes.lfy:62
        Entity::Comment(Comment::LineDocumentationOpen) => &[
            ModeBehavior::Opener {
                mode: Mode::LineDocumentation,
                when: Some(&[Mode::Code]),
            }, // @lfy def/lexer/modes.lfy:65
            ModeBehavior::AppliedUntilNewLine {
                mode: Mode::LineDocumentation,
            }, // @lfy def/lexer/modes.lfy:67
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

    // @lfy def/lexer/modes.lfy:10
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

    // @lfy def/lexer/modes.lfy:24
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

    // @lfy def/lexer/modes.lfy:26
    #[test]
    fn every_body_is_a_candidate_only_inside_its_region() {
        for rule in rules().filter(|rule| rule.is_body()) {
            let condition = lex_condition(rule);
            assert_eq!(condition.len(), 1, "{rule}");
            assert!(!IN_CODE.contains(&condition[0]), "{rule}");
        }
    }

    // @lfy def/lexer/modes.lfy:27
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
