//! Compiled from `def/grammar/tokens/traits.lfy`.
//!
//! `ebnfSyntax` is the [`EbnfSyntax`] trait every rule enum implements. `appliesToCurrent`,
//! `notAllowedPrefix` and `notAllowedPostfix` become [`RuleTrait`] values attached to the
//! rules they are applied to, together with the functions that evaluate them.

use super::super::ebnf::{self, Expr};
use super::super::types::EBNFSyntax;

/// `trait ebnfSyntax(syntax: string)`: EBNF details for a grammar rule.
// @lfy def/grammar/tokens/traits.lfy:3
pub trait EbnfSyntax: Copy + 'static {
    /// `@identifier`: the rule's name in the EBNF notation.
    // @lfy def/grammar/tokens/traits.lfy:7
    fn identifier(self) -> &'static str;

    /// The `syntax` argument: the rule's syntax in EBNF format.
    // @lfy def/grammar/tokens/traits.lfy:6
    fn syntax(self) -> &'static str;

    /// `{{@identifier}} = {{syntax}} ;`
    // @lfy def/grammar/tokens/traits.lfy:8
    fn rule(self) -> &'static str;

    /// Traits applied to the rule alongside `ebnfSyntax`.
    fn traits(self) -> &'static [RuleTrait];

    /// `$ebnf`: EBNF details for this token.
    // @lfy def/grammar/tokens/traits.lfy:4
    fn ebnf(self) -> EBNFSyntax {
        EBNFSyntax {
            syntax: self.syntax(),         // @lfy def/grammar/tokens/traits.lfy:6
            identifier: self.identifier(), // @lfy def/grammar/tokens/traits.lfy:7
            rule: self.rule(),             // @lfy def/grammar/tokens/traits.lfy:8
        }
    }

    /// The compiled EBNF expression of this rule; `None` for the external rules whose
    /// special sequence is provided by a built-in matcher.
    // @lfy def/grammar/tokens/traits.lfy:11
    fn expression(self) -> Option<&'static Expr> {
        ebnf::grammar().expression(self.identifier())
    }

    /// Byte length of the longest non-empty prefix of `input` that this rule matches.
    // @lfy def/grammar/tokens/traits.lfy:11
    fn longest_match(self, input: &str) -> Option<usize> {
        ebnf::grammar().longest_match(self.identifier(), input)
    }

    /// Byte length of the longest non-empty match of this rule at `source[start..]`. The
    /// text before `start` is available to the rule's traits.
    // @lfy def/grammar/tokens/traits.lfy:11
    fn longest_match_at(self, source: &str, start: usize) -> Option<usize> {
        ebnf::grammar().longest_match_at(self.identifier(), source, start)
    }
}

/// A trait applied to a rule in addition to `ebnfSyntax`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleTrait {
    /// `appliesToCurrent`, see [`applies_to_current`].
    AppliesToCurrent, // @lfy def/grammar/tokens/traits.lfy:14
    /// `notAllowedPrefix(...forbidden)`.
    NotAllowedPrefix(&'static [Forbidden]), // @lfy def/grammar/tokens/traits.lfy:21
    /// `notAllowedPostfix(...forbidden)`.
    NotAllowedPostfix(&'static [Forbidden]), // @lfy def/grammar/tokens/traits.lfy:27
}

/// One `forbidden` entry of `notAllowedPrefix` / `notAllowedPostfix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Forbidden {
    /// Exactly this text.
    Text(&'static str),
    /// `a non-numeric character`: a character that is not an ASCII digit. The start and
    /// the end of the input hold no character, so neither is forbidden.
    NonNumericCharacter, // @lfy def/grammar/tokens/operator.lfy:17
}

impl Forbidden {
    /// Text immediately prior to it is `{{str}}`.
    // @lfy def/grammar/tokens/traits.lfy:23
    pub fn precedes(self, before: &str) -> bool {
        match self {
            Forbidden::Text(text) => before.ends_with(text),
            Forbidden::NonNumericCharacter => before
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_ascii_digit()),
        }
    }

    /// Text immediately following is `{{str}}`.
    // @lfy def/grammar/tokens/traits.lfy:29
    pub fn follows(self, after: &str) -> bool {
        match self {
            Forbidden::Text(text) => after.starts_with(text),
            Forbidden::NonNumericCharacter => {
                after.chars().next().is_some_and(|c| !c.is_ascii_digit())
            }
        }
    }
}

/// Whether a candidate match for a rule carrying `traits` is not considered a match:
/// `before` is the text immediately prior to the candidate, `after` the text immediately
/// following it.
// @lfy def/grammar/tokens/traits.lfy:21
pub fn is_forbidden(traits: &[RuleTrait], before: &str, after: &str) -> bool {
    traits.iter().any(|rule_trait| match rule_trait {
        // @lfy def/grammar/tokens/traits.lfy:23
        RuleTrait::NotAllowedPrefix(forbidden) => forbidden.iter().any(|f| f.precedes(before)),
        // @lfy def/grammar/tokens/traits.lfy:29
        RuleTrait::NotAllowedPostfix(forbidden) => forbidden.iter().any(|f| f.follows(after)),
        RuleTrait::AppliesToCurrent => false,
    })
}

/// `trait appliesToCurrent`: which scope an accessor placed at the start of an expression
/// or statement applies to.
// @lfy def/grammar/tokens/traits.lfy:14
pub mod applies_to_current {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum AppliesTo {
        /// Applies to the current scope.
        CurrentScope, // @lfy def/grammar/tokens/traits.lfy:17
        /// Applies to the global scope.
        GlobalScope, // @lfy def/grammar/tokens/traits.lfy:18
    }

    /// Resolves the scope for an accessor: `is_current_ref` when it is placed at the start
    /// of an expression or statement, `is_in_global_scope` when that is a root level
    /// statement. `None` when the accessor is not a current reference.
    pub fn applies_to(is_current_ref: bool, is_in_global_scope: bool) -> Option<AppliesTo> {
        match (is_current_ref, is_in_global_scope) {
            (true, false) => Some(AppliesTo::CurrentScope), // @lfy def/grammar/tokens/traits.lfy:17
            (true, true) => Some(AppliesTo::GlobalScope),   // @lfy def/grammar/tokens/traits.lfy:18
            (false, _) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::applies_to_current::{AppliesTo, applies_to};
    use super::*;

    // @lfy def/grammar/tokens/traits.lfy:17
    #[test]
    fn applies_to_current_resolves_the_scope() {
        assert_eq!(applies_to(true, false), Some(AppliesTo::CurrentScope));
        assert_eq!(applies_to(true, true), Some(AppliesTo::GlobalScope));
        assert_eq!(applies_to(false, false), None);
        assert_eq!(applies_to(false, true), None);
    }

    // @lfy def/grammar/tokens/traits.lfy:23
    #[test]
    fn not_allowed_prefix_rejects_forbidden_preceding_text() {
        let traits = [RuleTrait::NotAllowedPrefix(&[
            Forbidden::Text("a"),
            Forbidden::NonNumericCharacter,
        ])];
        assert!(is_forbidden(&traits, "xa", ""));
        assert!(is_forbidden(&traits, "x", ""));
        assert!(!is_forbidden(&traits, "1", ""));
        assert!(!is_forbidden(&traits, "", ""));
    }

    // @lfy def/grammar/tokens/traits.lfy:29
    #[test]
    fn not_allowed_postfix_rejects_forbidden_following_text() {
        let text = [RuleTrait::NotAllowedPostfix(&[Forbidden::Text("/")])];
        assert!(is_forbidden(&text, "", "/ x"));
        assert!(!is_forbidden(&text, "", " x"));
        assert!(!is_forbidden(&text, "", ""));
        let numeric = [RuleTrait::NotAllowedPostfix(&[
            Forbidden::NonNumericCharacter,
        ])];
        assert!(is_forbidden(&numeric, "", "x"));
        assert!(is_forbidden(&numeric, "", "^"));
        assert!(!is_forbidden(&numeric, "", "1"));
        assert!(!is_forbidden(&numeric, "", ""));
        assert!(!is_forbidden(&[RuleTrait::AppliesToCurrent], "", "x"));
    }
}
