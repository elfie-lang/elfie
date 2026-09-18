//! Compiled from the global acceptance criteria of `def/grammar/main.lfy`: the checks
//! every rule set must pass, each reported per failing rule with the check it fails.

use std::collections::HashMap;
use std::fmt;

use super::ebnf::{self, Expr};
use super::terminals::literal::Literal;
use super::traits::{Category, GrammarRule, Terminal};
use super::{Entity, rules};

/// One of the global checks of the grammar.
// @lfy def/grammar/main.lfy:30
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Check {
    /// Every bare name referenced in any rule's syntax is the identifier of a rule entity.
    BareNamesAreRules, // @lfy def/grammar/main.lfy:31
    /// No two terminal entities have the same syntax.
    TerminalSyntaxIsUnique, // @lfy def/grammar/main.lfy:32
    /// Every prefix, infix, and postfix rule either has a binding itself or names an
    /// operator whose every alternative has a binding with one shared precedence.
    OperatorRulesBind, // @lfy def/grammar/main.lfy:33
    /// No rule has more than one of statement, primary, prefix, infix, and postfix.
    OneCategory, // @lfy def/grammar/main.lfy:34
    /// Every escape listed by a body is excluded from that body's plain characters by
    /// `Backslash`.
    BodiesExcludeBackslash, // @lfy def/grammar/main.lfy:35
    /// No alternative of an alternation can be satisfied without taking a token.
    AlternativesTakeAToken, // @lfy def/grammar/main.lfy:36
}

impl Check {
    pub const ALL: &'static [Check] = &[
        Check::BareNamesAreRules,
        Check::TerminalSyntaxIsUnique,
        Check::OperatorRulesBind,
        Check::OneCategory,
        Check::BodiesExcludeBackslash,
        Check::AlternativesTakeAToken,
    ];

    /// The behavior the check requires, as declared.
    pub const fn behavior(self) -> &'static str {
        match self {
            Check::BareNamesAreRules => {
                "Every bare name referenced in any rule's syntax is the identifier of a rule entity"
            }
            Check::TerminalSyntaxIsUnique => "No two terminal entities have the same syntax",
            Check::OperatorRulesBind => {
                "Every prefix, infix, and postfix rule either has binding itself or names an operator whose every alternative has binding with one shared precedence"
            }
            Check::OneCategory => {
                "No rule has more than one of statement, primary, prefix, infix, and postfix"
            }
            Check::BodiesExcludeBackslash => {
                "Every escape listed by a body is excluded from that body's plain characters by Backslash"
            }
            Check::AlternativesTakeAToken => {
                "No alternative of an alternation can be satisfied without taking a token."
            }
        }
    }
}

/// A rule that fails one of the checks, reported with which check it fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule: Entity,
    pub check: Check,
    pub detail: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} ({:?}: {})",
            self.rule.identifier(),
            self.detail,
            self.check,
            self.check.behavior()
        )
    }
}

/// `global@acceptanceCriteria`: `Ok` when every check holds for every rule, otherwise
/// every failing rule with the check it fails.
// @lfy def/grammar/main.lfy:30
pub fn validate() -> Result<(), Vec<Violation>> {
    let mut violations = Vec::new();
    let mut syntaxes: HashMap<&'static str, Entity> = HashMap::new();
    for rule in rules() {
        // @lfy def/grammar/main.lfy:31
        let parsed = match ebnf::parse(rule.syntax()) {
            Ok(expr) => {
                for reference in expr.references() {
                    if Entity::lookup(reference).is_none() {
                        violations.push(Violation {
                            rule,
                            check: Check::BareNamesAreRules,
                            detail: format!("references {reference}, which is not a rule"),
                        });
                    }
                }
                Some(expr)
            }
            Err(error) => {
                violations.push(Violation {
                    rule,
                    check: Check::BareNamesAreRules,
                    detail: format!("syntax does not parse: {error}"),
                });
                None
            }
        };
        // @lfy def/grammar/main.lfy:32
        if rule.is_terminal()
            && let Some(other) = syntaxes.insert(rule.syntax(), rule)
        {
            violations.push(Violation {
                rule,
                check: Check::TerminalSyntaxIsUnique,
                detail: format!("has the same syntax as {}", other.identifier()),
            });
        }
        // @lfy def/grammar/main.lfy:33
        if let Some(operator) = rule.category().operator()
            && rule.effective_binding().is_none()
        {
            violations.push(Violation {
                rule,
                check: Check::OperatorRulesBind,
                detail: format!(
                    "has no binding and its operator {} does not bind with one precedence",
                    operator.identifier()
                ),
            });
        }
        // @lfy def/grammar/main.lfy:34
        // A rule's category is a single value, so this check holds by construction.
        // @lfy def/grammar/main.lfy:35
        if let Category::Terminal(Terminal::Body { excluded, escapes }) = rule.category()
            && !escapes.is_empty()
            && !excluded.contains(&Entity::Literal(Literal::Backslash))
        {
            violations.push(Violation {
                rule,
                check: Check::BodiesExcludeBackslash,
                detail: "lists escapes but does not exclude Backslash".to_owned(),
            });
        }
        // @lfy def/grammar/main.lfy:36
        if let Some(expr) = &parsed
            && let Ok(grammar) = ebnf::Grammar::compile_cached()
        {
            expr.for_each(&mut |expr| {
                if let Expr::Alternation(alternatives) = expr {
                    for (index, alternative) in alternatives.iter().enumerate() {
                        if grammar.is_expr_nullable(alternative) {
                            violations.push(Violation {
                                rule,
                                check: Check::AlternativesTakeAToken,
                                detail: format!(
                                    "alternative {} of an alternation can be satisfied without taking a token",
                                    index + 1
                                ),
                            });
                        }
                    }
                }
            });
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::terminals::keyword::Keyword;

    // @lfy def/grammar/main.lfy:30
    #[test]
    fn the_grammar_passes_every_check() {
        if let Err(violations) = validate() {
            panic!(
                "{}",
                violations
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        assert_eq!(Check::ALL.len(), 6);
    }

    // @lfy def/grammar/main.lfy:36
    #[test]
    fn every_alternative_of_every_alternation_takes_a_token() {
        let grammar = ebnf::grammar();
        let mut alternations = 0;
        for rule in rules() {
            let Some(expr) = rule.expression() else {
                continue;
            };
            expr.for_each(&mut |expr| {
                if let Expr::Alternation(alternatives) = expr {
                    alternations += 1;
                    for alternative in alternatives {
                        assert!(
                            !grammar.is_expr_nullable(alternative),
                            "{}: {alternative:?}",
                            rule.identifier()
                        );
                    }
                }
            });
        }
        assert!(alternations > 40, "{alternations}");
        // An optional alternative would be a violation.
        assert!(grammar.is_expr_nullable(&ebnf::parse("(/ [[Comma]] /)").unwrap()));
    }

    #[test]
    fn violations_name_the_rule_the_check_and_the_detail() {
        let violation = Violation {
            rule: Entity::Keyword(Keyword::IfKeyword),
            check: Check::TerminalSyntaxIsUnique,
            detail: "has the same syntax as ElseKeyword".to_owned(),
        };
        assert_eq!(
            violation.to_string(),
            "IfKeyword: has the same syntax as ElseKeyword (TerminalSyntaxIsUnique: No two terminal entities have the same syntax)"
        );
        assert_eq!(
            Check::AlternativesTakeAToken.behavior(),
            "No alternative of an alternation can be satisfied without taking a token."
        );
    }
}
