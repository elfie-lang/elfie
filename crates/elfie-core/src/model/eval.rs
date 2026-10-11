//! Compiled from `def/model/main.lfy` (the properties pass) and `def/model/traits.lfy`
//! (`applyingTraits`, `applyingExtensions`): executes declaration and trait bodies, which
//! is where traits are applied, members and values join entities, criteria and tests are
//! recorded, and templates are evaluated. The top level of a file is run by the
//! interpreter's expand pass, which `bind` calls in place of a pass of its own.

use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::Statement as S;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::grammar::terminals::literal::Literal as L;
use crate::grammar::terminals::punctuation::Punctuation as P;
use crate::grammar::{Entity as Rule, GrammarRule};

use super::bind::{self, Binder};
use super::components;
use super::data::*;
use super::trees::Part;

mod expressions;
mod statements;
mod text;

/// The environment a body runs in.
#[derive(Debug, Clone)]
pub(crate) struct Env {
    /// Variables bound by parameters, loop variables, and declarations in the body,
    /// innermost last.
    pub vars: Vec<(String, Value)>,
    /// The entity `@`, `$`, and `.` reach: the declared entity, or the receiver of the
    /// trait being applied.
    pub current: EntityId,
    /// The entity criteria and tests attach to.
    pub target: EntityId,
    /// The entity recorded as the contributor of criteria.
    pub contributor: EntityId,
    /// The lexical scope names resolve in.
    pub scope: ScopeId,
    /// Whether this is a trait body run for a receiver.
    pub in_trait: bool,
    /// Whether side effects (applies, values, members) are suppressed: a trait's own body.
    pub dry: bool,
    /// The value a `return` gave.
    pub ret: Option<Value>,
}

impl Env {
    fn get(&self, name: &str) -> Option<&Value> {
        self.vars
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    fn set(&mut self, name: &str, value: Value) {
        if let Some(slot) = self.vars.iter_mut().rev().find(|(n, _)| n == name) {
            slot.1 = value;
        } else {
            self.vars.push((name.to_string(), value));
        }
    }
}

/// The conditions of a `Where`, before they are written out as the situations of a
/// criterion.
// @lfy def/grammar/rules/statement.lfy:Conditions
#[derive(Debug, Clone, PartialEq, Eq)]
enum Situations {
    /// The text of one condition.
    One(String),
    /// Every one of them holds.
    All(Vec<Situations>),
    /// One of them holds.
    Any(Vec<Situations>),
}

impl Situations {
    /// The negation, with only the text of a single condition negated in place.
    // @lfy def/grammar/rules/statement.lfy:Conditions
    fn negated(self) -> Situations {
        fn each(items: Vec<Situations>) -> Vec<Situations> {
            items.into_iter().map(Situations::negated).collect()
        }
        match self {
            Situations::One(text) => Situations::One(format!("not ({text})")),
            Situations::All(items) => Situations::Any(each(items)),
            Situations::Any(items) => Situations::All(each(items)),
        }
    }

    /// One text per situation, the alternatives of each joined by `or`.
    // @lfy def/grammar/rules/statement.lfy:Conditions
    fn texts(self) -> Vec<String> {
        self.merged()
            .into_iter()
            .filter(|situation| !situation.is_empty())
            .map(|situation| situation.join(" or "))
            .collect()
    }

    /// The list of situations, each as the conditions that merge into it.
    // @lfy def/grammar/rules/statement.lfy:Conditions
    fn merged(self) -> Vec<Vec<String>> {
        match self {
            Situations::One(text) => vec![vec![text]],
            // Every one holds, so the situations of each are situations of their own.
            Situations::All(items) => items.into_iter().flat_map(Situations::merged).collect(),
            // One of them holds, so every way of taking one situation from each merges
            // into a situation of its own.
            Situations::Any(items) => items.into_iter().fold(vec![Vec::new()], |merged, item| {
                let situations = item.merged();
                merged
                    .iter()
                    .flat_map(|before| {
                        situations.iter().map(|situation| {
                            let mut joined = before.clone();
                            joined.extend(situation.iter().cloned());
                            joined
                        })
                    })
                    .collect()
            }),
        }
    }
}

impl Binder {
    /// The conditions as one text per situation.
    // @lfy def/grammar/rules/statement.lfy:Conditions
    fn conditions_text(&mut self, conditions: NodeRef, env: &mut Env) -> Vec<String> {
        self.conditions_situations(conditions, env).texts()
    }

    /// The conditions as one [`Situations`], grouped by the keyword that joins them.
    // @lfy def/grammar/rules/statement.lfy:Conditions
    fn conditions_situations(&mut self, conditions: NodeRef, env: &mut Env) -> Situations {
        let trees = self.trees.clone();
        let mut all: Vec<Situations> = Vec::new();
        let mut any: Vec<Situations> = Vec::new();
        for part in trees.parts(conditions) {
            match part {
                Part::Node(condition) => any.push(self.condition_situations(condition, env)),
                Part::Token(index) if trees.token_is(conditions.file, index, K::AndKeyword) => {
                    all.push(Situations::Any(std::mem::take(&mut any)))
                }
                Part::Token(_) => {}
            }
        }
        all.push(Situations::Any(any));
        Situations::All(all)
    }

    /// One condition as the situations it stands for, with a `!` negating what follows it.
    // @lfy def/grammar/rules/statement.lfy:Conditions
    fn condition_situations(&mut self, condition: NodeRef, env: &mut Env) -> Situations {
        let trees = self.trees.clone();
        let negated = trees.has_token(condition, P::LogicalNot);
        let Some(inner) = trees.child_nodes(condition).into_iter().next() else {
            return Situations::All(Vec::new());
        };
        let situations = if trees.is(inner, S::ConditionGroup) {
            match trees.child(inner, S::Conditions) {
                Some(nested) => self.conditions_situations(nested, env),
                None => Situations::All(Vec::new()),
            }
        } else if trees.is(inner, E::Group) {
            match trees.child_nodes(inner).into_iter().next() {
                Some(expression) => Situations::One(self.text_of(expression, env)),
                None => Situations::All(Vec::new()),
            }
        } else {
            Situations::One(self.text_of(inner, env))
        };
        if negated { situations.negated() } else { situations }
    }
}

#[cfg(test)]
mod tests {
    use super::super::data::{Model, Origin, Source};

    /// One file, `a.lfy`, bound alone.
    fn bind_one(text: &str) -> Model {
        let tokens = crate::lexer::lex(text, Some("a.lfy")).unwrap_or_else(|error| panic!("{error}"));
        let tree = crate::parser::parse(tokens, None);
        assert!(tree.errors.is_empty(), "does not parse: {}", tree.render());
        crate::model::bind(vec![Source {
            path: "a.lfy".to_string(),
            tree,
            uses: Vec::new(),
            origin: Origin::Program,
        }])
    }

    /// The situation of every criterion of `A`, in order, for a file declaring it.
    fn situations(text: &str) -> Vec<Vec<String>> {
        let model = bind_one(text);
        let a = model
            .entities
            .iter()
            .position(|entity| entity.identifier.as_deref() == Some("A"))
            .expect("A is declared");
        model.entities[a]
            .acceptance_criteria
            .iter()
            .map(|criterion| criterion.situation.clone().unwrap_or_default())
            .collect()
    }

    // @lfy def/grammar/rules/statement.lfy:Conditions
    #[test]
    fn conditions_joined_by_or_merge_into_one_situation() {
        assert_eq!(
            situations("d A { where (`a`) -> `x`; where (`a`) or (`b`) or (`c`) -> `y`; }"),
            [vec!["a".to_string()], vec!["a or b or c".to_string()]]
        );
    }

    // @lfy def/grammar/rules/statement.lfy:Conditions
    #[test]
    fn conditions_joined_by_and_are_a_list_of_situations() {
        assert_eq!(
            situations("d A { where (`a`) and (`b`) and (`c`) -> `x`; where ((`a`) or (`b`)) and (`c`) -> `y`; }"),
            [
                vec!["a".to_string(), "b".to_string(), "c".to_string()],
                vec!["a or b".to_string(), "c".to_string()]
            ]
        );
    }

    // @lfy def/grammar/rules/statement.lfy:Conditions
    #[test]
    fn a_negated_nested_group_distributes_the_negation() {
        // "or" inside the group becomes "and": one situation per condition.
        assert_eq!(
            situations("d A { where !((`a`) or (`b`)) -> `x`; }"),
            [vec!["not (a)".to_string(), "not (b)".to_string()]]
        );
        // "and" inside the group becomes "or": the conditions merge into one situation.
        assert_eq!(
            situations("d A { where !((`a`) and (`b`)) -> `x`; }"),
            [vec!["not (a) or not (b)".to_string()]]
        );
        // A `!` on a condition that is no group negates that condition alone, and the
        // conditions beside it are joined as they were written.
        assert_eq!(
            situations("d A { where !(`a`) and (`b`) -> `x`; where !((`a`) and (`b`)) and (`c`) -> `y`; }"),
            [
                vec!["not (a)".to_string(), "b".to_string()],
                vec!["not (a) or not (b)".to_string(), "c".to_string()]
            ]
        );
    }
}
