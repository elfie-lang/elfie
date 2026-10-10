//! Compiled from the global acceptance criteria of `def/grammar/main.lfy`: the checks
//! every rule set must pass, each reported per failing rule with the check it fails.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use super::ebnf::{self, Expr};
use super::terminals::literal::Literal;
use super::traits::{Category, GrammarRule, Terminal};
use super::{Entity, rules};

/// One of the global checks of the grammar.
// @lfy def/grammar/main.lfy:grammarDocument
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Check {
    /// Every bare name referenced in any rule's syntax is the identifier of a rule entity.
    BareNamesAreRules, // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:efd1981bf1275d82e99ce8975d9008db6dfe0706b3d371324a263990718e239e
    /// No two terminal entities have the same syntax.
    TerminalSyntaxIsUnique, // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:76c422dad3532ba972328cec73a8cdb31276b79369b4289021b09512dd330565
    /// Every prefix, infix, and postfix rule either has a binding itself or names an
    /// operator whose every alternative has a binding with one shared precedence.
    OperatorRulesBind, // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:9762a97e1ef9727b6af134273474945e16a090c3fec34e9cdb6eb02dcb0862c9
    /// No rule has more than one of statement, primary, prefix, infix, and postfix.
    OneCategory, // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:5f29986bf08f9902a7ca5e5dca84541a894954f845ae1fabdc32814f74a8c357
    /// Every escape listed by a body is excluded from that body's plain characters by
    /// `Backslash`.
    BodiesExcludeBackslash, // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:60eb6110247c6a9801857b1d7d81d52a38e6be7c32014729b500af7371df187f
    /// No alternative of an alternation can be satisfied without taking a token.
    AlternativesTakeAToken, // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:c7242d6964974dbc0379df87801c492b8576355e0ea5279ded0cc1cf98984c7b
    /// Every alternative of an alternation that shares a first terminal with another
    /// alternative of that alternation is a single rule reference.
    SharedFirstsAreReferences, // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:6216542ad13bc4bbad09938343fcee04c835beef520cc5b4cacef2a5894e6ddf
}

impl Check {
    pub const ALL: &'static [Check] = &[
        Check::BareNamesAreRules,
        Check::TerminalSyntaxIsUnique,
        Check::OperatorRulesBind,
        Check::OneCategory,
        Check::BodiesExcludeBackslash,
        Check::AlternativesTakeAToken,
        Check::SharedFirstsAreReferences,
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
            Check::SharedFirstsAreReferences => {
                "Every alternative of an alternation inside a rule's syntax that shares a first terminal with another alternative of that alternation is a single rule reference"
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

/// One terminal an expression can begin with: a terminal rule where the syntax is read
/// from tokens, or a terminal string where it is read from characters. A character class
/// delivered by a built-in matcher is neither, and begins with no terminal.
// @lfy def/grammar/main.lfy:grammarDocument
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum First {
    Rule(&'static str),
    Text(String),
}

impl fmt::Display for First {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            First::Rule(identifier) => f.write_str(identifier),
            First::Text(text) => write!(f, "{text:?}"),
        }
    }
}

/// The terminals each rule can begin with, by identifier.
type Firsts = HashMap<&'static str, BTreeSet<First>>;

/// The terminals each rule can begin with, as a fixed point over every rule's syntax: a
/// terminal begins with itself, and any other rule with what its syntax reaches first,
/// past every element that can be satisfied without taking anything.
// @lfy def/grammar/main.lfy:grammarDocument
fn first_terminals(grammar: &ebnf::Grammar) -> Firsts {
    let mut firsts: Firsts = HashMap::new();
    for rule in rules().filter(|rule| rule.is_terminal()) {
        firsts.insert(
            rule.identifier(),
            BTreeSet::from([First::Rule(rule.identifier())]),
        );
    }
    loop {
        let mut changed = false;
        for rule in rules().filter(|rule| !rule.is_terminal()) {
            let Some(expr) = grammar.expression(rule.identifier()) else {
                continue;
            };
            let reached = firsts_of(expr, &firsts, grammar);
            let current = firsts.entry(rule.identifier()).or_default();
            if !reached.is_subset(current) {
                current.extend(reached);
                changed = true;
            }
        }
        if !changed {
            return firsts;
        }
    }
}

/// The terminals `expr` can begin with, given the terminals each rule can begin with.
// @lfy def/grammar/main.lfy:grammarDocument
fn firsts_of(expr: &Expr, firsts: &Firsts, grammar: &ebnf::Grammar) -> BTreeSet<First> {
    let mut reached = BTreeSet::new();
    collect_firsts(expr, firsts, grammar, &mut reached);
    reached
}

// @lfy def/grammar/main.lfy:grammarDocument
fn collect_firsts(
    expr: &Expr,
    firsts: &Firsts,
    grammar: &ebnf::Grammar,
    reached: &mut BTreeSet<First>,
) {
    match expr {
        Expr::Terminal(text) => {
            reached.insert(First::Text(text.clone()));
        }
        Expr::Special(_) => {}
        Expr::Reference(name) => match Entity::lookup(name) {
            Some(rule) if rule.is_terminal() => {
                reached.insert(First::Rule(rule.identifier()));
            }
            Some(rule) => reached.extend(
                firsts
                    .get(rule.identifier())
                    .into_iter()
                    .flatten()
                    .cloned(),
            ),
            None => {}
        },
        Expr::Sequence(items) => {
            for item in items {
                collect_firsts(item, firsts, grammar, reached);
                if !grammar.is_expr_nullable(item) {
                    break;
                }
            }
        }
        Expr::Alternation(items) => items
            .iter()
            .for_each(|item| collect_firsts(item, firsts, grammar, reached)),
        Expr::Exception(included, _) => collect_firsts(included, firsts, grammar, reached),
        Expr::Optional(inner) | Expr::Repetition(inner) => {
            collect_firsts(inner, firsts, grammar, reached);
        }
    }
}

/// One expression written back as EBNF, so a violation can name an alternation and its
/// alternatives as they are written.
// @lfy def/grammar/main.lfy:grammarDocument
fn show(expr: &Expr) -> String {
    match expr {
        Expr::Terminal(text) => format!("{text:?}"),
        Expr::Reference(name) => format!("[[{name}]]"),
        Expr::Special(text) => format!("? {text} ?"),
        Expr::Sequence(items) => items.iter().map(show).collect::<Vec<_>>().join(" , "),
        Expr::Alternation(items) => items.iter().map(show).collect::<Vec<_>>().join(" | "),
        Expr::Exception(included, excluded) => format!("{} - {}", show(included), show(excluded)),
        Expr::Optional(inner) => format!("(/ {} /)", show(inner)),
        Expr::Repetition(inner) => format!("(: {} :)", show(inner)),
    }
}

/// Every alternative of every alternation inside one rule's syntax that shares a first
/// terminal with another alternative of the same alternation while being more than a
/// reference to one rule.
// @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:6216542ad13bc4bbad09938343fcee04c835beef520cc5b4cacef2a5894e6ddf
fn shared_firsts(
    rule: Entity,
    syntax: &Expr,
    firsts: &Firsts,
    grammar: &ebnf::Grammar,
    violations: &mut Vec<Violation>,
) {
    syntax.for_each(&mut |expr| {
        let Expr::Alternation(alternatives) = expr else {
            return;
        };
        let reached: Vec<BTreeSet<First>> = alternatives
            .iter()
            .map(|alternative| firsts_of(alternative, firsts, grammar))
            .collect();
        for (index, alternative) in alternatives.iter().enumerate() {
            if matches!(alternative, Expr::Reference(_)) {
                continue;
            }
            for (other, others) in reached.iter().enumerate() {
                if other == index {
                    continue;
                }
                let Some(shared) = reached[index].intersection(others).next() else {
                    continue;
                };
                // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:f6dc345835379d0d75cb9018ac80ad7f1b94cd9b04d2258a43cf41047c1876c1
                violations.push(Violation {
                    rule,
                    check: Check::SharedFirstsAreReferences,
                    detail: format!(
                        "in the alternation {}, alternative {} ({}) shares the first terminal {shared} with alternative {} ({}) and is not a single rule reference",
                        show(expr),
                        index + 1,
                        show(alternative),
                        other + 1,
                        show(&alternatives[other]),
                    ),
                });
                break;
            }
        }
    });
}

/// `global@acceptanceCriteria`: `Ok` when every check holds for every rule, otherwise
/// every failing rule with the check it fails.
// @lfy def/grammar/main.lfy:grammarDocument
pub fn validate() -> Result<(), Vec<Violation>> {
    let mut violations = Vec::new();
    let mut syntaxes: HashMap<&'static str, Entity> = HashMap::new();
    let compiled = ebnf::Grammar::compile_cached().ok();
    let firsts = compiled.map(first_terminals);
    for rule in rules() {
        // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:efd1981bf1275d82e99ce8975d9008db6dfe0706b3d371324a263990718e239e
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
        // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:76c422dad3532ba972328cec73a8cdb31276b79369b4289021b09512dd330565
        if rule.is_terminal()
            && let Some(other) = syntaxes.insert(rule.syntax(), rule)
        {
            violations.push(Violation {
                rule,
                check: Check::TerminalSyntaxIsUnique,
                detail: format!("has the same syntax as {}", other.identifier()),
            });
        }
        // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:9762a97e1ef9727b6af134273474945e16a090c3fec34e9cdb6eb02dcb0862c9
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
        // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:5f29986bf08f9902a7ca5e5dca84541a894954f845ae1fabdc32814f74a8c357
        // A rule's category is a single value, so this check holds by construction.
        // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:60eb6110247c6a9801857b1d7d81d52a38e6be7c32014729b500af7371df187f
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
        // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:c7242d6964974dbc0379df87801c492b8576355e0ea5279ded0cc1cf98984c7b
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
        if let Some(expr) = &parsed
            && let (Some(grammar), Some(firsts)) = (compiled, &firsts)
        {
            shared_firsts(rule, expr, firsts, grammar, &mut violations);
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

    /// Every rule the grammar reports for one check, so a focused test can name its own
    /// failures instead of every failure of every check.
    fn violations_of(check: Check) -> Vec<Violation> {
        validate()
            .err()
            .unwrap_or_default()
            .into_iter()
            .filter(|violation| violation.check == check)
            .collect()
    }

    fn assert_no_violations(check: Check) {
        let violations = violations_of(check);
        assert!(
            violations.is_empty(),
            "{}",
            violations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:efd1981bf1275d82e99ce8975d9008db6dfe0706b3d371324a263990718e239e
    #[test]
    fn every_bare_name_of_every_syntax_names_a_rule() {
        assert_no_violations(Check::BareNamesAreRules);
        let mut references = 0;
        for rule in rules() {
            let expr = ebnf::parse(rule.syntax())
                .unwrap_or_else(|error| panic!("{}: {error}", rule.identifier()));
            for reference in expr.references() {
                references += 1;
                assert!(
                    Entity::lookup(reference).is_some(),
                    "{} references {reference}",
                    rule.identifier()
                );
            }
        }
        assert!(references > 100, "{references}");
    }

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:76c422dad3532ba972328cec73a8cdb31276b79369b4289021b09512dd330565
    #[test]
    fn no_two_terminals_have_the_same_syntax() {
        assert_no_violations(Check::TerminalSyntaxIsUnique);
        let mut syntaxes: HashMap<&'static str, Entity> = HashMap::new();
        for rule in rules().filter(|rule| rule.is_terminal()) {
            assert_eq!(
                syntaxes.insert(rule.syntax(), rule),
                None,
                "{} repeats a syntax",
                rule.identifier()
            );
        }
        assert!(syntaxes.len() > 100, "{}", syntaxes.len());
    }

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:9762a97e1ef9727b6af134273474945e16a090c3fec34e9cdb6eb02dcb0862c9
    #[test]
    fn every_operator_rule_binds_itself_or_through_its_operator() {
        assert_no_violations(Check::OperatorRulesBind);
        let mut operators = 0;
        for rule in rules() {
            if rule.category().operator().is_none() {
                continue;
            }
            operators += 1;
            assert!(
                rule.effective_binding().is_some(),
                "{} does not bind",
                rule.identifier()
            );
        }
        assert!(operators > 20, "{operators}");
    }

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:5f29986bf08f9902a7ca5e5dca84541a894954f845ae1fabdc32814f74a8c357
    #[test]
    fn no_rule_has_more_than_one_of_the_five_categories() {
        assert_no_violations(Check::OneCategory);
        for rule in rules() {
            let category = rule.category();
            let held = [
                matches!(category, Category::Statement),
                matches!(category, Category::Primary),
                matches!(category, Category::Prefix { .. }),
                matches!(category, Category::Infix { .. }),
                matches!(category, Category::Postfix { .. }),
            ]
            .iter()
            .filter(|held| **held)
            .count();
            assert!(held <= 1, "{} holds {held} categories", rule.identifier());
        }
    }

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:60eb6110247c6a9801857b1d7d81d52a38e6be7c32014729b500af7371df187f
    #[test]
    fn every_body_listing_escapes_excludes_backslash() {
        assert_no_violations(Check::BodiesExcludeBackslash);
        let mut bodies = 0;
        for rule in rules() {
            let Category::Terminal(Terminal::Body { excluded, escapes }) = rule.category() else {
                continue;
            };
            if escapes.is_empty() {
                continue;
            }
            bodies += 1;
            assert!(
                excluded.contains(&Entity::Literal(Literal::Backslash)),
                "{} lists escapes without excluding Backslash",
                rule.identifier()
            );
        }
        assert!(bodies >= 3, "{bodies}");
    }

    // @lfy def/grammar/main.lfy:grammarDocument
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
        assert_eq!(Check::ALL.len(), 7);
    }

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:c7242d6964974dbc0379df87801c492b8576355e0ea5279ded0cc1cf98984c7b
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

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:6216542ad13bc4bbad09938343fcee04c835beef520cc5b4cacef2a5894e6ddf
    #[test]
    fn every_alternative_sharing_a_first_terminal_is_a_single_rule_reference() {
        assert_no_violations(Check::SharedFirstsAreReferences);
        let grammar = ebnf::grammar();
        let firsts = first_terminals(grammar);
        let mut sharing = 0;
        for rule in rules() {
            let Some(expr) = rule.expression() else {
                continue;
            };
            expr.for_each(&mut |expr| {
                let Expr::Alternation(alternatives) = expr else {
                    return;
                };
                let reached: Vec<BTreeSet<First>> = alternatives
                    .iter()
                    .map(|alternative| firsts_of(alternative, &firsts, grammar))
                    .collect();
                for (index, alternative) in alternatives.iter().enumerate() {
                    let shares = reached.iter().enumerate().any(|(other, others)| {
                        other != index && reached[index].intersection(others).next().is_some()
                    });
                    if shares {
                        sharing += 1;
                        assert!(
                            matches!(alternative, Expr::Reference(_)),
                            "{}: {}",
                            rule.identifier(),
                            show(alternative)
                        );
                    }
                }
            });
        }
        assert!(sharing > 20, "{sharing}");
        // A terminal begins with itself, and a rule with what its syntax reaches first.
        assert_eq!(
            firsts["Identifier"],
            BTreeSet::from([First::Rule("Identifier")])
        );
        assert_eq!(firsts["Name"], BTreeSet::from([First::Rule("Identifier")]));
        assert!(firsts["Digit"].contains(&First::Text("7".to_owned())));
    }

    // @lfy def/grammar/main.lfy:grammarDocument#global:def/grammar/main.lfy:f6dc345835379d0d75cb9018ac80ad7f1b94cd9b04d2258a43cf41047c1876c1
    #[test]
    fn a_shared_first_terminal_outside_a_reference_names_the_alternation_and_the_alternatives() {
        let grammar = ebnf::grammar();
        let firsts = first_terminals(grammar);
        let rule = Entity::lookup("Expression").expect("a rule");
        let violations = |syntax: &str| {
            let mut violations = Vec::new();
            let expr = ebnf::parse(syntax).expect("parses");
            shared_firsts(rule, &expr, &firsts, grammar, &mut violations);
            violations
        };
        // `Name` begins with `Identifier`, so the sequence shares its first terminal.
        let shared = violations("[[Identifier]] , [[Comma]] | [[Name]]");
        assert_eq!(shared.len(), 1);
        assert_eq!(
            shared[0].to_string(),
            "Expression: in the alternation [[Identifier]] , [[Comma]] | [[Name]], \
             alternative 1 ([[Identifier]] , [[Comma]]) shares the first terminal Identifier \
             with alternative 2 ([[Name]]) and is not a single rule reference \
             (SharedFirstsAreReferences: Every alternative of an alternation inside a rule's \
             syntax that shares a first terminal with another alternative of that alternation \
             is a single rule reference)"
        );
        // Sharing between single rule references, or no sharing at all, is no violation.
        assert!(violations("[[Name]] | [[Current]]").is_empty());
        assert!(violations("[[Identifier]] , [[Comma]] | [[Semicolon]]").is_empty());
        // Nested alternations are checked too, and a character-level terminal string is a
        // first terminal of its own.
        let nested = violations(r#"[[Comma]] | ( "x" , [[Comma]] | "x" )"#);
        assert_eq!(nested.len(), 2);
        assert!(nested[0].detail.contains("the first terminal \"x\""));
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
