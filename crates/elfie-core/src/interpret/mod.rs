//! Compiled from `def/interpret/main.lfy`: the interpret step.
//!
//! [`phase_of`] says whether a node runs at compile time or is kept for runtime,
//! [`expand`] runs every piece of compile-time code that can add names until nothing new
//! appears, [`evaluate`] gives the value of an expression when something asks for it, and
//! [`lower`] gives the program as generation sees it. The data they work with is in
//! [`data`].
//!
//! Compile-time code runs the way Rust runs it: code that can add names runs in a loop
//! with name resolution until nothing new appears, as macro expansion does, and code that
//! only yields a value is evaluated when something asks for it, as const evaluation does.
//! Elfie lets one body both read the model and add names, which Rust does not, so reading
//! everything a trait was applied to waits until nothing can apply it any more.
//!
//! The parse tree is never rewritten: [`lower`] builds a new tree whose nodes point back
//! at it, as rust-analyzer does for macro expansions, because the query, format, and LSP
//! stages rely on the parse tree reproducing the source.
//!
//! Decision: the interpreter works in `model::Value`, which has the kinds a body needs
//! while it runs — a type, a closure, an entity's `acceptanceCriteria`, its `test`, and a
//! `like` — and hands back [`data::Value`], which has only the kinds the definition names,
//! at its two doors: [`evaluate`] and the folded nodes [`lower`] writes. One value model
//! is shared; only its outside is narrowed.
//!
//! Decision: a name is looked up against the model rather than against a pass of its own,
//! so [`expand`] needs nothing from the binder but the model the declare pass left.

pub mod data;

pub use data::*;

mod builtins;
mod context;
mod evaluate;
mod expand;
mod expressions;
mod interpreter;
mod lower;
mod phase_of;
mod statements;
mod text;

#[cfg(test)]
mod evaluate_tests;
#[cfg(test)]
mod expand_tests;
#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod lower_tests;
#[cfg(test)]
mod phase_of_tests;

pub use evaluate::evaluate;
pub use expand::expand;
pub use lower::lower;
pub use phase_of::phase_of;

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::file::File as F;
use crate::grammar::rules::statement::Statement as S;
use crate::grammar::terminals::identifier::Identifier as I;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::grammar::terminals::literal::Literal as L;
use crate::grammar::terminals::punctuation::Punctuation as P;
use crate::grammar::{Entity as Rule, GrammarRule};
use crate::lexer::Token;
use crate::model::Value as Interim;
use crate::model::{
    Applied, AppliedSource, Command, ContextProperty, Criterion, Entity as Declared, EntityId,
    EntityKind, FileId, Knowledge, KnowledgeKind, Layer, Model, NodeRef, Operation, Origin,
    Problem, SYNTHETIC, Scope, ScopeId, Symbol, SymbolId, SymbolKind, Test, TypeRef,
    accessor_layer, strip_references,
};
use crate::parser::components::is_trivia;
use crate::parser::data::Child;
use crate::workspace::Workspace;
use phase_of::{Added, CompileCall, added_member, compile_time_call};

/// The name the prelude gives the trait a fn carries when the interpreter performs it
/// natively rather than running its body.
// @lfy def/interpret/main.lfy:expand
const BUILTIN: &str = "builtin";

/// The member of an entity a criterion is added to.
// @lfy def/interpret/main.lfy:expand
const ACCEPTANCE_CRITERIA: &str = "acceptanceCriteria";

/// The member of an entity holding what the compiler is given to read for it.
// @lfy def/interpret/main.lfy:expand
const KNOWLEDGE: &str = "knowledge";

/// The member of an entity holding the shell commands for its operations.
// @lfy def/interpret/main.lfy:expand
const COMMANDS: &str = "commands";

/// The member of an entity holding the targets it is built for.
// @lfy def/interpret/main.lfy:expand
const TARGETS: &str = "targets";

/// The member of an entity that adds tests.
// @lfy def/interpret/main.lfy:expand
const TEST: &str = "test";

/// The member of an entity's context that describes a value only the compiler can choose.
// @lfy def/interpret/main.lfy:evaluate
const LIKE: &str = "like";

// ---------------------------------------------------------------------------------------
// Reading the trees of a model
// ---------------------------------------------------------------------------------------

/// One significant child of a node: a nested node or a token that is not trivia.
// @lfy def/interpret/main.lfy:phaseOf
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Node(NodeRef),
    Token(usize),
}

/// The small views of a node's children that the interpret step pattern-matches on. The
/// model already finds a node from its reference and a reference from its node; this adds
/// what reading a body needs on top.
// @lfy def/interpret/main.lfy:phaseOf
trait Read {
    fn rule_of(&self, r: NodeRef) -> Rule;
    fn is<R: GrammarRule>(&self, r: NodeRef, rule: R) -> bool;
    fn token_at(&self, file: FileId, index: usize) -> &Token;
    fn token_is<R: GrammarRule>(&self, file: FileId, index: usize, rule: R) -> bool;
    fn child_ref(&self, file: FileId, child: &crate::parser::data::Node) -> Option<NodeRef>;
    fn parts(&self, r: NodeRef) -> Vec<Part>;
    fn child<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Option<NodeRef>;
    fn children_of<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Vec<NodeRef>;
    fn child_nodes(&self, r: NodeRef) -> Vec<NodeRef>;
    fn child_token<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Option<usize>;
    fn has_token<R: GrammarRule>(&self, r: NodeRef, rule: R) -> bool;
    fn operator(&self, r: NodeRef) -> Option<usize>;
    fn string_value(&self, r: NodeRef) -> Option<String>;
    fn name(&self, r: NodeRef) -> Option<String>;
    fn declared_name(&self, r: NodeRef) -> Option<String>;
    fn accessor_and_name(&self, r: NodeRef) -> Option<(usize, Option<String>)>;
    fn left(&self, r: NodeRef) -> Option<NodeRef>;
    fn arguments(&self, r: NodeRef) -> Vec<NodeRef>;
    fn ancestor<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Option<NodeRef>;
    fn leftmost(&self, r: NodeRef) -> NodeRef;
    fn statements_of(&self, file: FileId) -> Vec<NodeRef>;
    fn descendants(&self, r: NodeRef) -> Vec<NodeRef>;
}

impl Read for Model {
    fn rule_of(&self, r: NodeRef) -> Rule {
        self.info(r).rule
    }

    fn is<R: GrammarRule>(&self, r: NodeRef, rule: R) -> bool {
        self.rule_of(r) == rule.entity()
    }

    fn token_at(&self, file: FileId, index: usize) -> &Token {
        &self.sources[file].tree.tokens[index]
    }

    fn token_is<R: GrammarRule>(&self, file: FileId, index: usize, rule: R) -> bool {
        self.token_at(file, index).rule == Some(rule.entity())
    }

    fn child_ref(&self, file: FileId, child: &crate::parser::data::Node) -> Option<NodeRef> {
        self.node_ref(file, child)
    }

    fn parts(&self, r: NodeRef) -> Vec<Part> {
        let node = self.node(r);
        let tokens = &self.sources[r.file].tree.tokens;
        node.children
            .iter()
            .filter_map(|child| match child {
                Child::Node(child) if !is_trivia(child.rule) => {
                    self.child_ref(r.file, child).map(Part::Node)
                }
                Child::Node(_) | Child::Error(_) => None,
                Child::Token(index) => {
                    let rule = tokens[*index].rule?;
                    (!is_trivia(rule)).then_some(Part::Token(*index))
                }
            })
            .collect()
    }

    fn child<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Option<NodeRef> {
        self.parts(r).into_iter().find_map(|part| match part {
            Part::Node(child) if self.is(child, rule) => Some(child),
            _ => None,
        })
    }

    fn children_of<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Vec<NodeRef> {
        self.parts(r)
            .into_iter()
            .filter_map(|part| match part {
                Part::Node(child) if self.is(child, rule) => Some(child),
                _ => None,
            })
            .collect()
    }

    fn child_nodes(&self, r: NodeRef) -> Vec<NodeRef> {
        self.parts(r)
            .into_iter()
            .filter_map(|part| match part {
                Part::Node(child) => Some(child),
                Part::Token(_) => None,
            })
            .collect()
    }

    fn child_token<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Option<usize> {
        let rule = rule.entity();
        self.parts(r).into_iter().find_map(|part| match part {
            Part::Token(index) if self.token_at(r.file, index).rule == Some(rule) => Some(index),
            _ => None,
        })
    }

    fn has_token<R: GrammarRule>(&self, r: NodeRef, rule: R) -> bool {
        self.child_token(r, rule).is_some()
    }

    /// The first significant child token of a node: the operator of an operation.
    fn operator(&self, r: NodeRef) -> Option<usize> {
        self.parts(r).into_iter().find_map(|part| match part {
            Part::Token(index) => Some(index),
            Part::Node(_) => None,
        })
    }

    fn string_value(&self, r: NodeRef) -> Option<String> {
        let inner = self.child_nodes(r).into_iter().next()?;
        let tokens = &self.sources[r.file].tree.tokens;
        let node = self.node(inner);
        let body = node
            .children
            .iter()
            .filter_map(Child::as_token)
            .find(|&i| tokens[i].is(L::SingleQuoteBody) || tokens[i].is(L::DoubleQuoteBody));
        Some(body.map_or_else(String::new, |i| tokens[i].value.clone()))
    }

    fn name(&self, r: NodeRef) -> Option<String> {
        self.child_token(r, I::Identifier)
            .map(|i| self.token_at(r.file, i).value.clone())
    }

    /// The name a `Declared` node spells.
    fn declared_name(&self, r: NodeRef) -> Option<String> {
        self.child(r, E::Declared)
            .and_then(|declared| self.name(declared))
    }

    /// The accessor token and the name written after it, if any, of a `Current` or
    /// `Member` node.
    fn accessor_and_name(&self, r: NodeRef) -> Option<(usize, Option<String>)> {
        let parts = self.parts(r);
        let mut iter = parts.iter().copied();
        let accessor = if self.is(r, E::Member) {
            iter.next();
            iter.next()
        } else {
            iter.next()
        };
        let Some(Part::Token(accessor)) = accessor else {
            return None;
        };
        accessor_layer(self.token_at(r.file, accessor).rule?)?;
        let name = match iter.next() {
            Some(Part::Token(index)) => {
                let token = self.token_at(r.file, index);
                let is_name = token
                    .rule
                    .is_some_and(|rule| rule == I::Identifier.entity() || rule.is_keyword());
                is_name.then(|| token.value.clone())
            }
            _ => None,
        };
        Some((accessor, name))
    }

    fn left(&self, r: NodeRef) -> Option<NodeRef> {
        match self.parts(r).first() {
            Some(Part::Node(left)) => Some(*left),
            _ => None,
        }
    }

    fn arguments(&self, r: NodeRef) -> Vec<NodeRef> {
        match self.child(r, E::Items) {
            Some(items) => self.child_nodes(items),
            None => Vec::new(),
        }
    }

    fn ancestor<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Option<NodeRef> {
        let mut current = Some(r);
        while let Some(node) = current {
            if self.is(node, rule) {
                return Some(node);
            }
            current = self.parent(node);
        }
        None
    }

    fn leftmost(&self, r: NodeRef) -> NodeRef {
        let mut current = r;
        loop {
            let rule = self.rule_of(current);
            if rule.is_infix() || rule.is_postfix() {
                match self.left(current) {
                    Some(left) => current = left,
                    None => return current,
                }
            } else {
                return current;
            }
        }
    }

    /// The statements directly in a file's `SourceFile`, in order.
    fn statements_of(&self, file: FileId) -> Vec<NodeRef> {
        self.child_nodes(NodeRef { file, index: 0 })
    }

    /// Every node at or below one, that one first, in source order.
    fn descendants(&self, r: NodeRef) -> Vec<NodeRef> {
        let mut out = Vec::new();
        let mut stack = vec![r];
        while let Some(node) = stack.pop() {
            out.push(node);
            let mut children = self.children_including_trivia(node);
            children.reverse();
            stack.extend(children);
        }
        out
    }
}

impl Model {
    /// Every child node of a node, trivia included.
    // @lfy def/interpret/main.lfy:phaseOf
    fn children_including_trivia(&self, r: NodeRef) -> Vec<NodeRef> {
        let node = self.node(r);
        node.children
            .iter()
            .filter_map(|child| match child {
                Child::Node(child) => self.node_ref(r.file, child),
                _ => None,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------
// Resolving names against the model
// ---------------------------------------------------------------------------------------

/// The symbol a `Name` or a `Member` reached through the value or scope layer names,
/// looked up statically: the same lookup the binder's resolve pass makes, against the
/// model the declare pass left.
// @lfy def/interpret/main.lfy:expand
fn resolve_name_node(model: &Model, node: NodeRef) -> Option<SymbolId> {
    if model.is(node, E::Name) {
        let name = model.name(node)?;
        return lookup_at(model, node, model.enclosing_scope(node), &name);
    }
    if model.is(node, E::Member) {
        let left = model.left(node)?;
        let (accessor, name) = model.accessor_and_name(node)?;
        if !model.token_is(node.file, accessor, P::ValueAccessor)
            && !model.token_is(node.file, accessor, P::ScopeAccessor)
        {
            return None;
        }
        let name = name?;
        let left = resolve_name_node(model, left)?;
        let entity = model.symbols[left].entity;
        return member_symbol(model, entity, &name);
    }
    None
}

/// The first symbol named `name` from `scope` outward, then the universe scope that holds
/// `global`.
// @lfy def/interpret/main.lfy:expand
fn lookup(model: &Model, scope: ScopeId, name: &str) -> Option<SymbolId> {
    model
        .lookup(scope, name)
        .or_else(|| model.lookup_local(universe(model), name))
}

/// The same, for a name written at a node: the member a declaration declares is not among
/// the names its own value sees, so the value of `Trait$apply` is the `apply` the file
/// declares and not the member being declared.
// @lfy def/interpret/main.lfy:expand
fn lookup_at(model: &Model, node: NodeRef, scope: ScopeId, name: &str) -> Option<SymbolId> {
    let found = lookup(model, scope, name)?;
    if !declares_at(model, node, found) {
        return Some(found);
    }
    let mut current = Some(scope);
    while let Some(id) = current {
        let holder = &model.scopes[id];
        if let Some(&symbol) = holder
            .symbols
            .iter()
            .chain(holder.imports.iter())
            .find(|&&symbol| symbol != found && model.symbols[symbol].name == name)
        {
            return Some(symbol);
        }
        current = holder.parent;
    }
    model.lookup_local(universe(model), name)
}

/// Whether the symbol is the member whose own declaration the node stands in.
// @lfy def/interpret/main.lfy:expand
fn declares_at(model: &Model, node: NodeRef, symbol: SymbolId) -> bool {
    let symbol = &model.symbols[symbol];
    if symbol.kind != SymbolKind::Member || symbol.node.file != node.file {
        return false;
    }
    let mut current = Some(node);
    while let Some(at) = current {
        if at == symbol.node {
            return true;
        }
        current = model.parent(at);
    }
    false
}

/// The member symbol of an entity: a symbol declared directly in the scope it owns.
// @lfy def/interpret/main.lfy:expand
fn member_symbol(model: &Model, entity: EntityId, name: &str) -> Option<SymbolId> {
    let scope = model.entities[entity].scope?;
    model.lookup_local(scope, name)
}

/// The universe scope, which holds `global`: the one scope no file owns.
// @lfy def/interpret/main.lfy:expand
fn universe(model: &Model) -> ScopeId {
    model
        .scopes
        .iter()
        .position(|scope| scope.owner == SYNTHETIC)
        .unwrap_or(0)
}

/// The trait symbol the `TraitUse` of an `is` or `extends` clause names. A clause of a
/// declaration is read in the scope around that declaration, so a trait and the entity
/// carrying it may share a name.
// @lfy def/interpret/main.lfy:expand
fn resolve_trait_use(model: &Model, trait_use: NodeRef) -> Option<SymbolId> {
    let identifiers: Vec<String> = model
        .parts(trait_use)
        .into_iter()
        .filter_map(|part| match part {
            Part::Token(index) if model.token_is(trait_use.file, index, I::Identifier) => {
                Some(model.token_at(trait_use.file, index).value.clone())
            }
            _ => None,
        })
        .collect();
    let own_scope = model.enclosing_scope(trait_use);
    let clause = model
        .ancestor(trait_use, E::IsClause)
        .or_else(|| model.ancestor(trait_use, E::ExtendsClause));
    let declaration = clause.and_then(|c| {
        let mut current = model.parent(c);
        while let Some(node) = current {
            if model.rule_of(node).is_statement() {
                return Some(node);
            }
            current = model.parent(node);
        }
        None
    });
    let scope = match declaration.and_then(|d| model.scope_of(d)) {
        Some(declared) if declared == own_scope => {
            model.scopes[own_scope].parent.unwrap_or(own_scope)
        }
        _ => own_scope,
    };
    match identifiers.as_slice() {
        [single] => lookup(model, scope, single),
        [module, name] => {
            let module = lookup(model, scope, module)?;
            let entity = model.symbols[module].entity;
            member_symbol(model, entity, name)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------
// The environment a body runs in
// ---------------------------------------------------------------------------------------

/// The environment a body runs in.
// @lfy def/interpret/main.lfy:expand
#[derive(Debug, Clone)]
struct Env {
    /// Variables bound by parameters, loop variables, and declarations in the body,
    /// innermost last.
    vars: Vec<(String, Interim)>,
    /// The entity `@`, `$`, and `.` reach: the declared entity, or the receiver of the
    /// trait being applied.
    current: EntityId,
    /// The entity criteria and tests attach to.
    target: EntityId,
    /// The entity recorded as the contributor of criteria.
    contributor: EntityId,
    /// The lexical scope names resolve in.
    scope: ScopeId,
    /// Whether this is a trait body run for a receiver.
    in_trait: bool,
    /// Whether side effects — applies, values, members — are suppressed: a trait's own
    /// body, and a body run only for its value.
    dry: bool,
    /// Whether the statements run are inside an `ace`, so a value it declares is taken now
    /// rather than when its name is read.
    ace: bool,
    /// The value a `return` gave.
    ret: Option<Interim>,
}

impl Env {
    // @lfy def/interpret/main.lfy:expand
    fn get(&self, name: &str) -> Option<&Interim> {
        self.vars
            .iter()
            .rev()
            .find(|(held, _)| held == name)
            .map(|(_, value)| value)
    }

    // @lfy def/interpret/main.lfy:expand
    fn set(&mut self, name: &str, value: Interim) {
        if let Some(slot) = self.vars.iter_mut().rev().find(|(held, _)| held == name) {
            slot.1 = value;
        } else {
            self.vars.push((name.to_string(), value));
        }
    }
}

// ---------------------------------------------------------------------------------------
// The interpreter
// ---------------------------------------------------------------------------------------

/// Runs compile-time code against a model.
///
/// One interpreter serves [`expand`], which lets a body add names and run a written
/// function, and [`evaluate`], which lets neither: `runs_functions` says which.
// @lfy def/interpret/main.lfy:expand
struct Interpreter<'m> {
    model: &'m mut Model,
    /// The value of each variable and enum member already taken, so the same node read
    /// twice gives the same value.
    // @lfy def/interpret/main.lfy:evaluate
    cache: HashMap<EntityId, Interim>,
    /// The entities whose values are being taken, innermost first: a value that needs its
    /// own is a cycle.
    // @lfy def/interpret/main.lfy:evaluate
    taking: Vec<EntityId>,
    /// The cycles already named, so one cycle adds one problem.
    // @lfy def/interpret/main.lfy:evaluate
    reported: HashSet<EntityId>,
    universe: ScopeId,
    prelude: Option<ScopeId>,
    /// Whether anything read only has a value at runtime, or could not be read at all: set
    /// while evaluating, and what tells [`evaluate`] to give nothing back.
    // @lfy def/interpret/main.lfy:evaluate
    runtime: bool,
    /// Whether a call of a written `function` runs, which only compile-time code may do.
    // @lfy def/interpret/main.lfy:evaluate
    runs_functions: bool,
    /// Whether what is read may be remembered on the entity it was read from, which only
    /// [`expand`] may do: evaluating a node never changes the model.
    // @lfy def/interpret/main.lfy:evaluate
    writes: bool,
}
