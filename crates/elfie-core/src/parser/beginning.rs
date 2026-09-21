//! Compiled from the "Rule Beginning" criteria of `def/parser/main.lfy` and its global
//! acceptance criteria: which terminals can begin each rule, the operations each operator
//! terminal continues an expression with, and the compile-time checks that `triedBefore`
//! orders every choice.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use crate::grammar::ebnf::{self, Expr};
use crate::grammar::rules::expression::{PREFIXES, PRIMARIES};
use crate::grammar::rules::statement::STATEMENTS;
use crate::grammar::{Category, Entity, GrammarRule, rules};

use super::traits;

/// What the parser knows about the grammar before any token is read.
pub(crate) struct Tables {
    /// Load order of every rule, used to keep lists of rules deterministic.
    index: HashMap<Entity, usize>,
    /// The terminals each rule can begin with.
    first: HashMap<Entity, HashSet<Entity>>,
    /// The rules each operator terminal continues an expression with, the postfix rule
    /// before the infix rule where the terminal is the operator of both.
    // @lfy def/parser/main.lfy:parse
    operations: HashMap<Entity, Vec<Entity>>,
    /// The parsed tail syntax of every postfix rule with a tail.
    tails: HashMap<Entity, Expr>,
}

impl Tables {
    /// The terminals `rule` can begin with.
    // @lfy def/parser/main.lfy:parse
    pub fn first(&self, rule: Entity) -> &HashSet<Entity> {
        static EMPTY: OnceLock<HashSet<Entity>> = OnceLock::new();
        self.first
            .get(&rule)
            .unwrap_or_else(|| EMPTY.get_or_init(HashSet::new))
    }

    /// Whether a token of `terminal` can be the first token `rule` covers.
    // @lfy def/parser/main.lfy:parse
    pub fn can_begin(&self, rule: Entity, terminal: Entity) -> bool {
        self.first(rule).contains(&terminal)
    }

    /// The terminals `expr` can begin with, following references, every alternative, and
    /// every element that can be satisfied without taking a token.
    // @lfy def/parser/main.lfy:parse
    pub fn first_of(&self, expr: &Expr) -> HashSet<Entity> {
        let mut out = HashSet::new();
        first_of(expr, &self.first, &mut out);
        out
    }

    /// Whether a token of `terminal` can be the first token `expr` covers.
    pub fn expr_can_begin(&self, expr: &Expr, terminal: Entity) -> bool {
        match expr {
            Expr::Reference(name) => Entity::lookup(name).is_some_and(|rule| self.can_begin(rule, terminal)),
            _ => self.first_of(expr).contains(&terminal),
        }
    }

    /// The rules a token of `terminal` could continue an expression with, in the order
    /// they are tried: the postfix rule of the terminal before its infix rule, as
    /// `Generic` comes before `RelationalOperation` at a `LessThan`.
    // @lfy def/parser/main.lfy:parse
    pub fn operations(&self, terminal: Entity) -> &[Entity] {
        self.operations.get(&terminal).map_or(&[], Vec::as_slice)
    }

    /// The identifiers of the operator terminals whose operation would apply with `min`
    /// as the minimum binding power in force, among `admitted` operations when given, in
    /// load order.
    // @lfy def/parser/main.lfy:parse
    pub fn operators_continuing(&self, min: u8, admitted: Option<&[Entity]>) -> Vec<&'static str> {
        let set: HashSet<Entity> = self
            .operations
            .iter()
            .filter(|(_, operations)| {
                operations.iter().any(|operation| {
                    admitted.is_none_or(|admitted| admitted.contains(operation))
                        && operation.effective_binding().is_some_and(|binding| {
                            let power = binding.precedence_value();
                            power > min
                                || (power == min && binding.associativity == Some(crate::grammar::Associativity::Right))
                        })
                })
            })
            .map(|(&terminal, _)| terminal)
            .collect();
        self.identifiers(&set)
    }

    /// The parsed tail of a postfix rule.
    pub fn tail(&self, rule: Entity) -> Option<&Expr> {
        self.tails.get(&rule)
    }

    /// The identifiers of the terminals that could begin `rule`, in load order.
    pub fn expected(&self, rule: Entity) -> Vec<&'static str> {
        self.identifiers(self.first(rule))
    }

    /// The identifiers of a set of rules in load order.
    pub fn identifiers(&self, set: &HashSet<Entity>) -> Vec<&'static str> {
        let mut rules: Vec<Entity> = set.iter().copied().collect();
        rules.sort_by_key(|rule| self.index[rule]);
        rules.into_iter().map(|rule| rule.identifier()).collect()
    }

    /// The rules of `candidates` that can begin with `terminal`, in the order they are
    /// tried.
    // @lfy def/parser/main.lfy:parse
    pub fn select(&self, candidates: &[Entity], terminal: Entity) -> Vec<Entity> {
        let selected: Vec<Entity> = candidates
            .iter()
            .copied()
            .filter(|&candidate| self.can_begin(candidate, terminal))
            .collect();
        traits::order_by_tried_before(&selected).expect("validated when the tables are built")
    }
}

/// The terminals `expr` can begin with, given the terminals each rule can begin with.
fn first_of(expr: &Expr, first: &HashMap<Entity, HashSet<Entity>>, out: &mut HashSet<Entity>) {
    let grammar = ebnf::grammar();
    match expr {
        Expr::Terminal(_) | Expr::Special(_) => {}
        Expr::Reference(name) => {
            if let Some(rule) = Entity::lookup(name) {
                if rule.is_terminal() {
                    out.insert(rule);
                } else if let Some(set) = first.get(&rule) {
                    out.extend(set.iter().copied());
                }
            }
        }
        Expr::Sequence(items) => {
            for item in items {
                first_of(item, first, out);
                if !grammar.is_expr_nullable(item) {
                    break;
                }
            }
        }
        Expr::Alternation(items) => items.iter().for_each(|item| first_of(item, first, out)),
        Expr::Exception(included, _) => first_of(included, first, out),
        Expr::Optional(inner) | Expr::Repetition(inner) => first_of(inner, first, out),
    }
}

/// Whether the rule continues an expression rather than beginning one.
// @lfy def/parser/main.lfy:parse
pub fn is_operation(rule: Entity) -> bool {
    rule.is_infix() || rule.is_postfix()
}

/// Whether the rule is one of the expression categories.
pub fn is_expression_rule(rule: Entity) -> bool {
    rule.is_primary() || rule.is_prefix() || is_operation(rule)
}

/// The terminals an operator rule is satisfied by: the terminal itself, or every terminal
/// of an alternation list.
fn operator_terminals(operator: Entity, out: &mut Vec<Entity>) {
    match operator.category() {
        Category::AlternationList(items) => items.iter().for_each(|&item| operator_terminals(item, out)),
        _ if operator.is_terminal() => out.push(operator),
        _ => {}
    }
}

/// The terminals each rule can begin with, as a fixed point over every rule's syntax.
// @lfy def/parser/main.lfy:parse
fn compute_first() -> HashMap<Entity, HashSet<Entity>> {
    let mut first: HashMap<Entity, HashSet<Entity>> = HashMap::new();
    for rule in rules() {
        if rule.is_terminal() {
            first.insert(rule, HashSet::from([rule]));
        }
    }
    loop {
        let mut changed = false;
        for rule in rules() {
            // @lfy def/parser/main.lfy:parse
            if rule.is_terminal() || is_operation(rule) {
                continue;
            }
            let Some(expr) = rule.expression() else {
                continue;
            };
            let mut set = HashSet::new();
            first_of(expr, &first, &mut set);
            let current = first.entry(rule).or_default();
            if set.len() != current.len() || !set.is_subset(current) {
                current.extend(set);
                changed = true;
            }
        }
        if !changed {
            return first;
        }
    }
}

/// A failure of the global acceptance criteria: a choice `triedBefore` does not decide, or
/// an alternation the parser cannot select among.
// @lfy def/parser/main.lfy:parse
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The terminal at which the candidates collide.
    pub terminal: Entity,
    /// Where the collision is: the statement set, the expression set, or a rule with an
    /// alternation.
    pub within: &'static str,
    /// The candidates the terminal selects.
    pub candidates: Vec<Entity>,
    pub detail: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let candidates: Vec<&str> = self.candidates.iter().map(|rule| rule.identifier()).collect();
        write!(
            f,
            "{} at {}: {} (candidates {})",
            self.within,
            self.terminal.identifier(),
            self.detail,
            candidates.join(", ")
        )
    }
}

/// The global acceptance criteria of the parser, checked against the grammar and the
/// components: every failing terminal or alternation is reported with the candidates it
/// selects.
// @lfy def/parser/main.lfy:parse
pub fn validate() -> Result<(), Vec<Violation>> {
    let first = compute_first();
    let mut violations = Vec::new();
    let terminals: Vec<Entity> = rules().filter(|rule| rule.is_terminal()).collect();
    let check_ordered = |terminal: Entity, within: &'static str, candidates: Vec<Entity>, violations: &mut Vec<Violation>| {
        if let Err((a, b)) = traits::order_by_tried_before(&candidates) {
            violations.push(Violation {
                terminal,
                within,
                candidates,
                detail: format!("{} and {} are not ordered by triedBefore", a.identifier(), b.identifier()),
            });
        }
    };
    for &terminal in &terminals {
        // @lfy def/parser/main.lfy:parse
        let statements: Vec<Entity> = STATEMENTS
            .iter()
            .copied()
            .filter(|rule| first[rule].contains(&terminal))
            .collect();
        check_ordered(terminal, "Statement", statements, &mut violations);
        // @lfy def/parser/main.lfy:parse
        let expressions: Vec<Entity> = PRIMARIES
            .iter()
            .chain(PREFIXES)
            .copied()
            .filter(|rule| first[rule].contains(&terminal))
            .collect();
        check_ordered(terminal, "Expression", expressions, &mut violations);
    }
    // @lfy def/parser/main.lfy:parse
    for rule in rules().filter(|rule| !rule.is_terminal()) {
        let Some(expr) = rule.expression() else {
            continue;
        };
        expr.for_each(&mut |expr| {
            let Expr::Alternation(alternatives) = expr else {
                return;
            };
            let firsts: Vec<HashSet<Entity>> = alternatives
                .iter()
                .map(|alternative| {
                    let mut set = HashSet::new();
                    first_of(alternative, &first, &mut set);
                    set
                })
                .collect();
            for &terminal in &terminals {
                let selected: Vec<usize> = (0..alternatives.len())
                    .filter(|&index| firsts[index].contains(&terminal))
                    .collect();
                if selected.len() < 2 {
                    continue;
                }
                let mut candidates = Vec::new();
                for &index in &selected {
                    // @lfy def/parser/main.lfy:parse
                    match &alternatives[index] {
                        Expr::Reference(name) => candidates.push(Entity::lookup(name).expect("resolved")),
                        other => violations.push(Violation {
                            terminal,
                            within: rule.identifier(),
                            candidates: Vec::new(),
                            detail: format!("alternative {} shares a first terminal but is not a single rule reference: {other:?}", index + 1),
                        }),
                    }
                }
                check_ordered(terminal, rule.identifier(), candidates, &mut violations);
            }
        });
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Internal requirements the compiled parser relies on, checked once: parser-level rules
/// use only references, sequences, alternations, optionals and repetitions, and each
/// operator terminal continues an expression with one rule of each category.
fn assert_supported(operations: &HashMap<Entity, Vec<Entity>>) -> Result<(), String> {
    for rule in rules().filter(|rule| !rule.is_terminal() && rule.category() != Category::Rule || rule.is_statement()) {
        let Some(expr) = rule.expression() else {
            continue;
        };
        let mut problem = None;
        expr.for_each(&mut |expr| {
            if matches!(expr, Expr::Terminal(_) | Expr::Special(_) | Expr::Exception(..)) && problem.is_none() {
                problem = Some(format!("{}: {expr:?} is not a token-level construct", rule.identifier()));
            }
        });
        if let Some(problem) = problem {
            return Err(problem);
        }
    }
    let _ = operations;
    Ok(())
}

/// The tables, built on first use. The grammar and the parser components are fixed at
/// compile time, so a failure here is a defect in `def/` and is reported by panicking.
// @lfy def/parser/main.lfy:parse
pub(crate) fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        if let Err(violations) = validate() {
            let lines: Vec<String> = violations.iter().map(ToString::to_string).collect();
            panic!("parse cannot be compiled:\n{}", lines.join("\n"));
        }
        let index: HashMap<Entity, usize> = rules().enumerate().map(|(index, rule)| (rule, index)).collect();
        let first = compute_first();
        let mut operations: HashMap<Entity, Vec<Entity>> = HashMap::new();
        let mut tails = HashMap::new();
        for rule in rules().filter(|rule| is_operation(*rule)) {
            let operator = rule.category().operator().expect("operations have operators");
            let mut terminals = Vec::new();
            operator_terminals(operator, &mut terminals);
            for terminal in terminals {
                // A terminal may be the operator of a postfix rule and of an infix rule,
                // as `LessThan` is of `Generic` and of `RelationalOperation`: the postfix
                // rule is tried first and the infix rule only where the postfix rule is
                // no match there, so the postfix rule comes first.
                // @lfy def/parser/main.lfy:parse
                let entry = operations.entry(terminal).or_default();
                if let Some(other) = entry.iter().find(|other| other.is_postfix() == rule.is_postfix()) {
                    // @lfy def/parser/main.lfy:parse
                    panic!(
                        "parse cannot be compiled: {} continues an expression with both {} and {}",
                        terminal.identifier(),
                        other.identifier(),
                        rule.identifier()
                    );
                }
                // @lfy def/parser/main.lfy:parse
                match rule.is_postfix() {
                    true => entry.insert(0, rule),
                    false => entry.push(rule),
                }
            }
            if let Category::Postfix { tail: Some(tail), .. } = rule.category() {
                let expr = ebnf::parse(tail).unwrap_or_else(|error| panic!("{}: tail does not parse: {error}", rule.identifier()));
                tails.insert(rule, expr);
            }
        }
        if let Err(problem) = assert_supported(&operations) {
            panic!("parse cannot be compiled: {problem}");
        }
        Tables {
            index,
            first,
            operations,
            tails,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::rules::expression::Expression;
    use crate::grammar::rules::file::File;
    use crate::grammar::rules::statement::Statement;
    use crate::grammar::terminals::identifier::Identifier;
    use crate::grammar::terminals::keyword::Keyword;
    use crate::grammar::terminals::punctuation::Punctuation;

    fn expression(rule: Expression) -> Entity {
        Entity::Expression(rule)
    }

    fn statement(rule: Statement) -> Entity {
        Entity::Statement(rule)
    }

    fn punctuation(rule: Punctuation) -> Entity {
        Entity::Punctuation(rule)
    }

    // @lfy def/parser/main.lfy:parse
    #[test]
    fn infix_and_postfix_rules_begin_with_no_terminal() {
        let tables = tables();
        for rule in rules().filter(|rule| is_operation(*rule)) {
            assert!(tables.first(rule).is_empty(), "{rule}");
        }
        assert!(is_operation(expression(Expression::Member)));
        assert!(!is_operation(expression(Expression::Name)));
        assert!(is_expression_rule(expression(Expression::NotOperation)));
        assert!(!is_expression_rule(statement(Statement::Block)));
    }

    // @lfy def/parser/main.lfy:parse
    #[test]
    fn other_rules_begin_with_the_terminals_their_syntax_reaches_first() {
        let tables = tables();
        let identifier = Entity::Identifier(Identifier::Identifier);
        assert!(tables.can_begin(expression(Expression::Name), identifier));
        assert!(tables.can_begin(expression(Expression::Expression), identifier));
        assert!(tables.can_begin(statement(Statement::ExpressionStatement), identifier));
        assert!(tables.can_begin(statement(Statement::Statement), identifier));
        assert!(tables.can_begin(Entity::File(File::SourceFile), identifier));
        // Through every alternative and past optional elements.
        assert!(tables.can_begin(statement(Statement::Condition), punctuation(Punctuation::LogicalNot)));
        assert!(tables.can_begin(statement(Statement::Condition), punctuation(Punctuation::GroupOpen)));
        assert!(tables.can_begin(expression(Expression::Signature), punctuation(Punctuation::GroupOpen)));
        assert!(tables.can_begin(expression(Expression::TypeExpression), punctuation(Punctuation::Ampersand)));
        assert!(tables.can_begin(expression(Expression::TypeExpression), identifier));
        // An accessor begins an expression through Current, never through Member.
        assert!(tables.can_begin(expression(Expression::Expression), punctuation(Punctuation::ValueAccessor)));
        assert!(tables.can_begin(expression(Expression::Reference), punctuation(Punctuation::ValueAccessor)));
        // Not through an operation.
        assert!(!tables.can_begin(expression(Expression::Expression), punctuation(Punctuation::Semicolon)));
        assert!(!tables.can_begin(expression(Expression::Expression), Entity::Keyword(Keyword::AsKeyword)));
        // A terminal begins with itself.
        assert_eq!(tables.first(identifier).len(), 1);
        // Items can be satisfied without a token, so Arguments begins with GroupOpen only.
        assert_eq!(tables.first(expression(Expression::Arguments)).len(), 1);
        assert_eq!(
            tables.identifiers(tables.first(statement(Statement::VariableDeclaration))),
            vec!["ConstKeyword", "LetKeyword"]
        );
    }

    // @lfy def/parser/main.lfy:parse
    #[test]
    fn each_operator_terminal_continues_an_expression_with_one_rule_of_each_category() {
        let tables = tables();
        assert_eq!(tables.operations(punctuation(Punctuation::Plus)), [expression(Expression::AdditiveOperation)]);
        assert_eq!(tables.operations(punctuation(Punctuation::Minus)), [expression(Expression::AdditiveOperation)]);
        assert_eq!(tables.operations(punctuation(Punctuation::ValueAccessor)), [expression(Expression::Member)]);
        assert_eq!(tables.operations(punctuation(Punctuation::GroupOpen)), [expression(Expression::Call)]);
        assert_eq!(tables.operations(Entity::Keyword(Keyword::AsKeyword)), [expression(Expression::Cast)]);
        assert_eq!(tables.operations(punctuation(Punctuation::PlainSetter)), [expression(Expression::Assignment)]);
        assert_eq!(tables.operations(punctuation(Punctuation::Ampersand)), [expression(Expression::BitwiseAndOperation)]);
        assert!(tables.operations(punctuation(Punctuation::Semicolon)).is_empty());
        assert!(tables.operations(Entity::Keyword(Keyword::InKeyword)).is_empty());
        // A terminal that is the operator of both: the postfix rule comes first.
        // @lfy def/parser/main.lfy:parse
        assert_eq!(
            tables.operations(punctuation(Punctuation::LessThan)),
            [expression(Expression::Generic), expression(Expression::RelationalOperation)]
        );
        assert!(tables.tail(expression(Expression::Member)).is_some());
        assert!(tables.tail(expression(Expression::AdditiveOperation)).is_none());
    }

    // @lfy def/parser/main.lfy:parse
    #[test]
    fn selection_keeps_only_the_candidates_the_terminal_begins_in_tried_before_order() {
        let tables = tables();
        assert_eq!(
            tables.select(STATEMENTS, punctuation(Punctuation::BlockOpen)),
            vec![statement(Statement::Block), statement(Statement::ExpressionStatement)]
        );
        assert_eq!(
            tables.select(STATEMENTS, Entity::Keyword(Keyword::FunctionKeyword)),
            vec![statement(Statement::FunctionDeclaration), statement(Statement::ExpressionStatement)]
        );
        let expressions: Vec<Entity> = PRIMARIES.iter().chain(PREFIXES).copied().collect();
        assert_eq!(
            tables.select(&expressions, punctuation(Punctuation::GroupOpen)),
            vec![expression(Expression::InlineFunction), expression(Expression::Group)]
        );
        assert_eq!(
            tables.select(&expressions, punctuation(Punctuation::BlockOpen)),
            vec![expression(Expression::Object), expression(Expression::Type)]
        );
        assert_eq!(tables.select(&expressions, punctuation(Punctuation::Semicolon)), vec![]);
    }

    // @lfy def/parser/main.lfy:parse
    #[test]
    fn tried_before_orders_every_choice_the_grammar_presents() {
        assert_eq!(validate(), Ok(()));
        let violation = Violation {
            terminal: punctuation(Punctuation::GroupOpen),
            within: "Expression",
            candidates: vec![expression(Expression::Group), expression(Expression::InlineFunction)],
            detail: "Group and InlineFunction are not ordered by triedBefore".to_owned(),
        };
        assert_eq!(
            violation.to_string(),
            "Expression at GroupOpen: Group and InlineFunction are not ordered by triedBefore (candidates Group, InlineFunction)"
        );
    }
}
