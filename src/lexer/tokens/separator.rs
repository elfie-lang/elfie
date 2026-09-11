//! Compiled from `def/lexer/tokens/separator.lfy`.

use super::super::data::{LexerModeStack, Mode};
use super::token_enum;
use super::traits::ModeBehavior;

/// `ListMode` is the top mode on the stack.
// @lfy def/lexer/tokens/separator.lfy:7
pub fn in_list(stack: &LexerModeStack) -> bool {
    stack.top_mode() == Some(Mode::List)
}

/// `BlockMode` is the top mode on the stack.
// @lfy def/lexer/tokens/separator.lfy:8
pub fn in_block(stack: &LexerModeStack) -> bool {
    stack.top_mode() == Some(Mode::Block)
}

/// `GroupMode` is the top mode on the stack.
// @lfy def/lexer/tokens/separator.lfy:9
pub fn in_group(stack: &LexerModeStack) -> bool {
    stack.top_mode() == Some(Mode::Group)
}

token_enum! {
    /// Separators. Open/close pairs push and pop their list/block/group modes.
    // @lfy def/lexer/tokens/separator.lfy:11
    pub enum Separator {
        BlockOpen => ("SR_BLOCK_OPEN", "{", "an open token for a block"), // @lfy def/lexer/tokens/separator.lfy:12
        BlockClose => ("SR_BLOCK_CLOSE", "}", "a close token for a block"), // @lfy def/lexer/tokens/separator.lfy:13
        GroupOpen => ("SR_GROUP_OPEN", "(", "an open token for a group"), // @lfy def/lexer/tokens/separator.lfy:14
        GroupClose => ("SR_GROUP_CLOSE", ")", "a close token for a group"), // @lfy def/lexer/tokens/separator.lfy:15
        ListOpen => ("SR_LIST_OPEN", "[", "an open token for a list"), // @lfy def/lexer/tokens/separator.lfy:16
        ListContinue => ("SR_LIST_CONTINUE", ",", "continue character for next item or statement in list"), // @lfy def/lexer/tokens/separator.lfy:17
        ListClose => ("SR_LIST_CLOSE", "]", "a close token for a list"), // @lfy def/lexer/tokens/separator.lfy:18
        StatementEnd => ("SR_STATEMENT_END", ";", "end character for statements"), // @lfy def/lexer/tokens/separator.lfy:19
    }
}

const BLOCK_CREATOR: &[ModeBehavior] = &[ModeBehavior::Creator {
    mode: Mode::Block,
    condition: None,
}]; // @lfy def/lexer/tokens/separator.lfy:12
const BLOCK_DESTROYER: &[ModeBehavior] = &[ModeBehavior::Destroyer {
    mode: Mode::Block,
    condition: None,
}]; // @lfy def/lexer/tokens/separator.lfy:13
const GROUP_CREATOR: &[ModeBehavior] = &[ModeBehavior::Creator {
    mode: Mode::Group,
    condition: None,
}]; // @lfy def/lexer/tokens/separator.lfy:14
const GROUP_DESTROYER: &[ModeBehavior] = &[ModeBehavior::Destroyer {
    mode: Mode::Group,
    condition: None,
}]; // @lfy def/lexer/tokens/separator.lfy:15
const LIST_CREATOR: &[ModeBehavior] = &[ModeBehavior::Creator {
    mode: Mode::List,
    condition: None,
}]; // @lfy def/lexer/tokens/separator.lfy:16
const LIST_DESTROYER: &[ModeBehavior] = &[ModeBehavior::Destroyer {
    mode: Mode::List,
    condition: None,
}]; // @lfy def/lexer/tokens/separator.lfy:18

impl Separator {
    /// The mode traits (`modeCreator` / `modeDestroyer`) attached to this separator.
    pub fn mode_behaviors(self) -> &'static [ModeBehavior] {
        match self {
            Separator::BlockOpen => BLOCK_CREATOR,
            Separator::BlockClose => BLOCK_DESTROYER,
            Separator::GroupOpen => GROUP_CREATOR,
            Separator::GroupClose => GROUP_DESTROYER,
            Separator::ListOpen => LIST_CREATOR,
            Separator::ListClose => LIST_DESTROYER,
            Separator::ListContinue | Separator::StatementEnd => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::data::TokenKind;
    use super::*;

    // @lfy def/lexer/tokens/separator.lfy:7
    #[test]
    fn mode_predicates_follow_the_top_of_the_stack() {
        let mut stack = LexerModeStack::new();
        assert!(!in_list(&stack) && !in_block(&stack) && !in_group(&stack));
        stack.push(Mode::List, TokenKind::Separator(Separator::ListOpen));
        assert!(in_list(&stack));
        stack.push(Mode::Block, TokenKind::Separator(Separator::BlockOpen));
        assert!(in_block(&stack) && !in_list(&stack));
        stack.push(Mode::Group, TokenKind::Separator(Separator::GroupOpen));
        assert!(in_group(&stack) && !in_block(&stack));
    }

    // @lfy def/lexer/tokens/separator.lfy:11
    #[test]
    fn separators_carry_their_mode_traits() {
        let creator = |behaviors: &[ModeBehavior]| match behaviors {
            [
                ModeBehavior::Creator {
                    mode,
                    condition: None,
                },
            ] => Some(*mode),
            _ => None,
        };
        let destroyer = |behaviors: &[ModeBehavior]| match behaviors {
            [
                ModeBehavior::Destroyer {
                    mode,
                    condition: None,
                },
            ] => Some(*mode),
            _ => None,
        };
        assert_eq!(
            creator(Separator::BlockOpen.mode_behaviors()),
            Some(Mode::Block)
        );
        assert_eq!(
            destroyer(Separator::BlockClose.mode_behaviors()),
            Some(Mode::Block)
        );
        assert_eq!(
            creator(Separator::GroupOpen.mode_behaviors()),
            Some(Mode::Group)
        );
        assert_eq!(
            destroyer(Separator::GroupClose.mode_behaviors()),
            Some(Mode::Group)
        );
        assert_eq!(
            creator(Separator::ListOpen.mode_behaviors()),
            Some(Mode::List)
        );
        assert_eq!(
            destroyer(Separator::ListClose.mode_behaviors()),
            Some(Mode::List)
        );
        assert!(Separator::ListContinue.mode_behaviors().is_empty());
        assert!(Separator::StatementEnd.mode_behaviors().is_empty());
    }
}
