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

/// The name the prelude gives the trait a fn carries when the interpreter performs it
/// natively rather than running its body.
// @lfy def/interpret/main.lfy:expand
const BUILTIN: &str = "builtin";

/// The trait whose items each satisfy a rule: an `apply` on one applies the trait to every
/// item of it.
// @lfy def/interpret/main.lfy:expand
const ALTERNATION_LIST: &str = "alternationList";

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

/// The data the package `elfie` declares for one thing a project is compiled into: what an
/// `ace const` of the project holds to declare a target.
// @lfy def/interpret/main.lfy:expand
const TARGET: &str = "Target";

/// The member of a layer holding the trait chosen.
// @lfy def/interpret/main.lfy:expand
const SUBJECT: &str = "subject";

/// The member of a layer holding one value per parameter of its trait.
// @lfy def/interpret/main.lfy:expand
const ARGUMENTS: &str = "arguments";

/// The slots of a `Target` that hold its layers, in guidance order.
// @lfy def/interpret/main.lfy:expand
const SLOTS: [&str; 8] = [
    "layers",
    "interfaces",
    "frameworks",
    "layout",
    "runtime",
    "platforms",
    "ecosystem",
    "language",
];

/// The member of an entity that adds tests.
// @lfy def/interpret/main.lfy:expand
const TEST: &str = "test";

/// The member of an entity's context that describes a value only the compiler can choose.
// @lfy def/interpret/main.lfy:evaluate
const LIKE: &str = "like";

/// `Entity.typeArguments`: what a use of a generic declaration was written with.
// @lfy def/interpret/main.lfy:expand
const TYPE_ARGUMENTS: &str = "typeArguments";

/// `Entity.typeParameters`: what a declaration is generic over.
// @lfy def/interpret/main.lfy:expand
const TYPE_PARAMETERS: &str = "typeParameters";

/// One piece of knowledge as a value: the object an `add` call was given, read back.
// @lfy def/interpret/main.lfy:expand
fn knowledge_value(item: &Knowledge) -> Interim {
    Interim::Object(vec![
        ("topic".to_string(), Interim::String(item.topic.clone())),
        (
            "kind".to_string(),
            Interim::String(item.kind.value().to_string()),
        ),
        ("source".to_string(), Interim::String(item.source.clone())),
        (
            "quote".to_string(),
            item.quote.map_or(Interim::Undefined, Interim::Bool),
        ),
    ])
}

/// One command as a value, read back the same way.
// @lfy def/interpret/main.lfy:expand
fn command_value(item: &Command) -> Interim {
    Interim::Object(vec![
        (
            "operation".to_string(),
            Interim::String(item.operation.value().to_string()),
        ),
        (
            "line".to_string(),
            item.line.clone().map_or(Interim::Undefined, Interim::String),
        ),
    ])
}

/// The statement rules that declare a name, so that a statement directly in a
/// [`F::SourceFile`] which is none of them runs at compile time.
// @lfy def/interpret/main.lfy:phaseOf
fn is_declaration(rule: Rule) -> bool {
    [
        S::DataDeclaration.entity(),
        S::AgentFunctionDeclaration.entity(),
        S::FunctionDeclaration.entity(),
        S::TraitDeclaration.entity(),
        S::TypeDeclaration.entity(),
        S::EnumDeclaration.entity(),
        S::VariableDeclaration.entity(),
        S::AliasDeclaration.entity(),
        S::ExternalDeclaration.entity(),
        S::Use.entity(),
    ]
    .contains(&rule)
}

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
// phaseOf
// ---------------------------------------------------------------------------------------

/// Whether a node runs at compile time or is kept for runtime.
///
/// A node is compile time when it is, or is inside, an [`S::Ace`], an [`S::TraitDeclaration`],
/// an [`S::Where`], an [`S::With`], an [`E::IsClause`], an [`E::ExtendsClause`], the
/// [`S::Block`] of an [`S::DataDeclaration`] or an [`S::AgentFunctionDeclaration`], a call
/// of `apply` on a trait, of `add` on an entity's `acceptanceCriteria`, `knowledge`,
/// `commands`, or `targets`, or of `test` on an entity's context, or a statement directly
/// in a [`F::SourceFile`] that is not a declaration; and when it is a
/// [`E::TemplateReference`] or [`E::TemplateExecution`] in a definition, a criterion, or a
/// test. Everything else is runtime: a declaration whose body is compile time is itself
/// runtime, and a file-level `const` is runtime though [`lower`] folds its value.
// @lfy def/interpret/main.lfy:phaseOf
pub fn phase_of(model: &Model, node: NodeRef) -> Phase {
    let mut at = Some(node);
    while let Some(current) = at {
        if runs_at_compile_time(model, current) {
            // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:e296a93f89da71bf1df9226c1abb709c149b656286c7a3d31029d1a5b088a47a
            return Phase::Compile;
        }
        // A template reference or execution in a definition is read while binding, even
        // though the declaration it describes is kept.
        // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:5357c505ea2cc6797f85156d4c4bba6d26483c5ab8599dbf3d34660ece727fa0
        if (model.is(current, E::TemplateReference) || model.is(current, E::TemplateExecution))
            && in_definition(model, current)
        {
            return Phase::Compile;
        }
        at = model.parent(current);
    }
    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:0b2b478046202841146fead796a05acd11de3f9e4a12c62d35acd2b8af467d0b
    Phase::Runtime
}

/// Whether this node itself is one of the compile-time forms.
// @lfy def/interpret/main.lfy:phaseOf
fn runs_at_compile_time(model: &Model, r: NodeRef) -> bool {
    let rule = model.rule_of(r);
    if rule == S::Ace.entity()
        || rule == S::TraitDeclaration.entity()
        || rule == S::Where.entity()
        || rule == S::With.entity()
        || rule == E::IsClause.entity()
        || rule == E::ExtendsClause.entity()
    {
        return true;
    }
    // The body of a data or of a fn is criteria, members, and tests: it is read, never
    // generated.
    // @lfy def/interpret/main.lfy:phaseOf
    if rule == S::Block.entity()
        && let Some(owner) = model.parent(r)
        && (model.is(owner, S::DataDeclaration) || model.is(owner, S::AgentFunctionDeclaration))
    {
        return true;
    }
    if rule == E::Call.entity() && compile_time_call(model, r).is_some() {
        return true;
    }
    // A statement directly in a file that declares nothing runs while binding.
    // @lfy def/interpret/main.lfy:phaseOf
    if rule.is_statement()
        && !is_declaration(rule)
        && model.parent(r).is_some_and(|up| model.is(up, F::SourceFile))
    {
        return true;
    }
    false
}

/// What a compile-time call is, when a `Call` is one.
// @lfy def/interpret/main.lfy:phaseOf
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompileCall {
    /// `T.apply(receiver, ...)` on a trait.
    Apply,
    /// `add(...)` on a member of an entity's context that holds items.
    Add,
    /// `test(...)` on an entity's context.
    Test,
}

/// A member of an entity's context that an `add` call appends one item to.
// @lfy def/interpret/main.lfy:phaseOf
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Added {
    /// `acceptanceCriteria`: one criterion.
    Criteria,
    /// `knowledge`: one piece of knowledge.
    Knowledge,
    /// `commands`: one shell command.
    Commands,
    /// `targets`: one target the entity is built for.
    Targets,
}

/// The member a chain of `add` calls appends to, and the expression that reads it. The
/// chain is walked back, so the second `add` of `@targets.add(a).add(b)` appends to the
/// same member as the first.
// @lfy def/interpret/main.lfy:phaseOf
fn added_member(model: &Model, receiver: NodeRef) -> Option<(Added, NodeRef)> {
    let mut at = receiver;
    loop {
        if let Some((accessor, Some(name))) = model.accessor_and_name(at)
            && model.token_is(at.file, accessor, P::ContextAccessor)
        {
            return match name.as_str() {
                ACCEPTANCE_CRITERIA => Some((Added::Criteria, at)),
                KNOWLEDGE => Some((Added::Knowledge, at)),
                COMMANDS => Some((Added::Commands, at)),
                TARGETS => Some((Added::Targets, at)),
                _ => None,
            };
        }
        // An earlier `add` of the same chain: what it read is what this one appends to.
        // @lfy def/interpret/main.lfy:phaseOf
        if !model.is(at, E::Call) {
            return None;
        }
        let callee = model.left(at)?;
        let (_, Some(method)) = model.accessor_and_name(callee)? else {
            return None;
        };
        if method != "add" {
            return None;
        }
        at = model.left(callee)?;
    }
}

/// Which compile-time call a `Call` node is, if any: `apply` on a trait, `add` on a member
/// of an entity's context that holds items, or `test` on an entity's context.
// @lfy def/interpret/main.lfy:phaseOf
fn compile_time_call(model: &Model, call: NodeRef) -> Option<CompileCall> {
    let callee = model.left(call)?;
    // `@test(...)` or `X@test(...)`: the context member named test.
    if let Some((accessor, Some(name))) = model.accessor_and_name(callee)
        && model.token_is(callee.file, accessor, P::ContextAccessor)
        && name == TEST
    {
        return Some(CompileCall::Test);
    }
    if !model.is(callee, E::Member) {
        return None;
    }
    let (accessor, Some(method)) = model.accessor_and_name(callee)? else {
        return None;
    };
    if !model.token_is(callee.file, accessor, P::ValueAccessor)
        && !model.token_is(callee.file, accessor, P::OptionalValueAccessor)
    {
        return None;
    }
    let receiver = model.left(callee)?;
    if method == "add" && added_member(model, receiver).is_some() {
        return Some(CompileCall::Add);
    }
    if method == "apply" && names_a_trait(model, receiver) {
        return Some(CompileCall::Apply);
    }
    None
}

/// Whether an expression names a trait: a name, or a member of a module, bound to one.
// @lfy def/interpret/main.lfy:phaseOf
fn names_a_trait(model: &Model, r: NodeRef) -> bool {
    match resolve_name_node(model, r) {
        Some(symbol) => model.entities[model.symbols[symbol].entity].is_trait(),
        // An unresolved name may still be a trait; the clause it is written in decides.
        None => false,
    }
}

/// Whether a node stands in the description written for a declaration.
// @lfy def/interpret/main.lfy:phaseOf
fn in_definition(model: &Model, r: NodeRef) -> bool {
    model.ancestor(r, E::DefinitionClause).is_some() || model.ancestor(r, E::Definition).is_some()
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

/// The symbol the nearest earlier statement of the same block declared: what a `Previous`
/// reads, found the way the binder finds it so that a `^^` evaluated while the expand pass
/// is still running reads the same entity the resolve pass binds it to.
// @lfy def/interpret/main.lfy:evaluate
fn previous_symbol(model: &Model, r: NodeRef) -> Option<SymbolId> {
    let mut statement = r;
    while let Some(parent) = model.parent(statement) {
        if model.rule_of(statement).is_statement()
            && (model.is(parent, S::Block) || model.is(parent, F::SourceFile))
        {
            let siblings = model.child_nodes(parent);
            let position = siblings.iter().position(|&s| s == statement)?;
            return siblings[..position]
                .iter()
                .rev()
                .find_map(|&earlier| model.symbol_of(earlier));
        }
        statement = parent;
    }
    None
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

/// The prelude scope, when a source has [`Origin::Prelude`].
// @lfy def/interpret/main.lfy:expand
fn prelude_scope(model: &Model) -> Option<ScopeId> {
    let file = model
        .sources
        .iter()
        .position(|source| source.origin == Origin::Prelude)?;
    model.file_scopes.get(file).copied()
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

/// The conditions of a `Where`, before they are written out as the situations of a
/// criterion: conditions joined by "or" merge into one situation, and conditions joined by
/// "and" are a list of situations.
// @lfy def/interpret/main.lfy:expand
#[derive(Debug, Clone, PartialEq, Eq)]
enum Situations {
    /// The text of one condition.
    One(String),
    /// Every one of them holds: a list of situations.
    All(Vec<Situations>),
    /// One of them holds: they merge into one situation.
    Any(Vec<Situations>),
}

impl Situations {
    /// The negation, distributed over a nested group.
    // @lfy def/interpret/main.lfy:expand
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

    /// One text per situation: the "or" alternatives of each are joined into the one
    /// situation they merge into.
    // @lfy def/interpret/main.lfy:expand
    fn texts(self) -> Vec<String> {
        self.merged()
            .into_iter()
            .filter(|situation| !situation.is_empty())
            .map(|situation| situation.join(" or "))
            .collect()
    }

    /// The list of situations, each as the conditions that merge into it.
    // @lfy def/interpret/main.lfy:expand
    fn merged(self) -> Vec<Vec<String>> {
        match self {
            Situations::One(text) => vec![vec![text]],
            Situations::All(items) => items.into_iter().flat_map(Situations::merged).collect(),
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

impl<'m> Interpreter<'m> {
    // @lfy def/interpret/main.lfy:expand
    fn new(model: &'m mut Model) -> Interpreter<'m> {
        let universe = universe(model);
        let prelude = prelude_scope(model);
        Interpreter {
            model,
            cache: HashMap::new(),
            taking: Vec::new(),
            reported: HashSet::new(),
            universe,
            prelude,
            runtime: false,
            runs_functions: true,
            writes: true,
        }
    }

    // @lfy def/interpret/main.lfy:expand
    fn problem(&mut self, node: NodeRef, message: impl Into<String>) {
        self.model.problems.push(Problem {
            node,
            message: message.into(),
            stage: crate::model::Stage::Binder,
        });
    }

    /// The environment the top level of a file runs in.
    // @lfy def/interpret/main.lfy:expand
    fn file_env(&self, file: FileId) -> Env {
        let entity = self.model.file_entities[file];
        Env {
            vars: Vec::new(),
            current: entity,
            target: entity,
            contributor: entity,
            scope: self.model.file_scopes[file],
            in_trait: false,
            dry: false,
            ace: false,
            ret: None,
        }
    }

    /// The data the prelude declares under this name, when the program has a prelude.
    // @lfy def/interpret/main.lfy:expand
    fn prelude_data(&self, name: &str) -> Option<EntityId> {
        let prelude = self.prelude?;
        let symbol = self.model.lookup_local(prelude, name)?;
        let entity = self.model.symbols[symbol].entity;
        matches!(self.model.entities[entity].kind, EntityKind::Data).then_some(entity)
    }

    /// The kind data of an entity: the prelude data for what it is, then `Entity`, which
    /// every entity is seen through.
    // @lfy def/interpret/main.lfy:expand
    fn kind_data(&self, entity: EntityId) -> Vec<EntityId> {
        let own = match self.model.entities[entity].kind {
            EntityKind::Trait { .. } => Some("Trait"),
            EntityKind::Fn { .. } => Some("Function"),
            EntityKind::Data => Some("Data"),
            EntityKind::Type => Some("Type"),
            EntityKind::Enum => Some("Enum"),
            EntityKind::Member => Some("Member"),
            EntityKind::Parameter => Some("Parameter"),
            EntityKind::Variable => Some("Variable"),
            EntityKind::Module | EntityKind::File => Some("Module"),
            _ => None,
        };
        own.and_then(|name| self.prelude_data(name))
            .into_iter()
            .chain(self.prelude_data("Entity"))
            .collect()
    }

    /// The member of an entity's kind data with this name.
    // @lfy def/interpret/main.lfy:expand
    fn kind_member(&self, entity: EntityId, name: &str) -> Option<SymbolId> {
        self.kind_data(entity)
            .into_iter()
            .find_map(|data| member_symbol(self.model, data, name))
    }

    /// Whether the name is a member of any kind data: one of another kind yields undefined
    /// for this entity, without a problem.
    // @lfy def/interpret/main.lfy:expand
    fn kind_member_anywhere(&self, name: &str) -> bool {
        [
            "Entity", "Trait", "Function", "Data", "Type", "Enum", "Member", "Parameter",
            "Variable", "Module",
        ]
        .iter()
        .any(|kind| {
            self.prelude_data(kind)
                .and_then(|data| member_symbol(self.model, data, name))
                .is_some()
        })
    }

    /// The data a value of this type is read through: the declaration it names, or what the
    /// prelude declares for the kind of value it is.
    // @lfy def/interpret/main.lfy:evaluate
    fn base_data(&self, ty: &TypeRef) -> Option<EntityId> {
        let name = match ty {
            TypeRef::Entity(entity) => return Some(*entity),
            TypeRef::Predicate(entity) => return Some(*entity),
            TypeRef::Primitive("string") => "String",
            TypeRef::Primitive("number") => "Number",
            TypeRef::Primitive("boolean") => "Boolean",
            TypeRef::Primitive("object") => "Object",
            TypeRef::Primitive("function") | TypeRef::Function => "Function",
            TypeRef::List(_) => "List",
            TypeRef::Literal(value) => match **value {
                Interim::String(_) => "String",
                Interim::Number(_) => "Number",
                Interim::Bool(_) => "Boolean",
                _ => return None,
            },
            _ => return None,
        };
        self.prelude_data(name)
    }

    /// Whether a method on a value is an operation of a data carrying [`BUILTIN`], which
    /// the interpreter performs itself rather than calling anything: the dispatch is on the
    /// trait, never on the name alone. A program that declares no data for the value, or no
    /// `builtin` at all, says nothing against it.
    // @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
    fn performs_operation(&self, receiver: &Interim, method: &str) -> bool {
        let ty = match receiver {
            Interim::String(_) => TypeRef::Primitive("string"),
            Interim::Number(_) => TypeRef::Primitive("number"),
            Interim::Bool(_) => TypeRef::Primitive("boolean"),
            Interim::List(_) => TypeRef::List(Box::new(TypeRef::Unknown(String::new()))),
            _ => return true,
        };
        let Some(data) = self.base_data(&ty) else {
            return true;
        };
        self.carries_builtin(data) && member_symbol(self.model, data, method).is_some()
    }

    /// Whether a data carries [`BUILTIN`], or its declaration names it: the `is` clause of
    /// a library data has not always run when a body of another file reaches one of its
    /// operations, and the trait it names is what decides.
    // @lfy def/interpret/main.lfy:expand
    fn carries_builtin(&self, data: EntityId) -> bool {
        let Some(marker) = self.model.trait_named(BUILTIN) else {
            return true;
        };
        if self.model.entities[data].has_trait(marker) {
            return true;
        }
        let Some(node) = self.model.entities[data].node else {
            return false;
        };
        let holder = self.model.child(node, E::Declared).unwrap_or(node);
        let Some(clause) = self.model.child(holder, E::IsClause) else {
            return false;
        };
        self.model
            .descendants(clause)
            .into_iter()
            .filter(|&r| self.model.is(r, E::TraitUse))
            .any(|used| {
                resolve_trait_use(self.model, used)
                    .is_some_and(|symbol| self.model.symbols[symbol].entity == marker)
            })
    }

    /// The type a node written in a type position stands for.
    ///
    /// Decision: the expand pass runs after the binder's type pass, so every declared type
    /// is already on its entity; a type written inside a body is carried as a primitive, as
    /// the declaration a name resolves to, or as the source text it was written as, which
    /// is what a template or a criterion renders it to. Nothing in a compile-time body
    /// reads more of one.
    // @lfy def/interpret/main.lfy:expand
    fn type_of(&self, node: NodeRef) -> TypeRef {
        let model = &*self.model;
        if model.is(node, E::PrimitiveType) {
            let text = model.raw(node).trim().to_string();
            return match text.as_str() {
                "string" => TypeRef::Primitive("string"),
                "number" => TypeRef::Primitive("number"),
                "boolean" => TypeRef::Primitive("boolean"),
                "object" => TypeRef::Primitive("object"),
                "function" => TypeRef::Primitive("function"),
                "trait" => TypeRef::Primitive("trait"),
                _ => TypeRef::Unknown(text),
            };
        }
        if model.is(node, E::Name)
            && let Some(symbol) = resolve_name_node(model, node)
        {
            return TypeRef::Entity(model.symbols[symbol].entity);
        }
        if model.is(node, E::TypeExpression) || model.is(node, E::TypeGroup) {
            let items: Vec<TypeRef> = model
                .child_nodes(node)
                .into_iter()
                .map(|item| self.type_of(item))
                .collect();
            return match items.len() {
                0 => TypeRef::Unknown(model.raw(node)),
                1 => items.into_iter().next().expect("one item"),
                _ => TypeRef::Union(items),
            };
        }
        if model.is(node, E::TypePredicate)
            && let Some(inner) = model.child_nodes(node).into_iter().next()
            && let Some(symbol) = resolve_name_node(model, inner)
        {
            return TypeRef::Predicate(model.symbols[symbol].entity);
        }
        // `T[]` in a type position: an `Index` with nothing inside it.
        if model.is(node, E::Index)
            && let Some(left) = model.left(node)
            && model.child_nodes(node).len() == 1
        {
            return TypeRef::List(Box::new(self.type_of(left)));
        }
        TypeRef::Unknown(model.raw(node))
    }

    // ---- Statements ----------------------------------------------------------------

    /// Runs one statement.
    // @lfy def/interpret/main.lfy:expand
    fn exec(&mut self, r: NodeRef, env: &mut Env) {
        if env.ret.is_some() {
            return;
        }
        let rule = self.model.rule_of(r);
        if let Some(symbol) = self.model.symbol_of(r)
            && rule.is_statement()
            && self.model.symbols[symbol].kind != SymbolKind::LoopVariable
        {
            self.exec_declaration(r, symbol, env);
            return;
        }
        if rule == S::ExpressionStatement.entity() {
            if let Some(expression) = self.model.child_nodes(r).into_iter().next() {
                self.eval(expression, env);
            }
        } else if rule == S::Where.entity() {
            self.exec_where(r, env);
        } else if rule == S::With.entity() {
            self.exec_with(r, env);
        } else if rule == S::If.entity() {
            self.exec_if(r, env);
        } else if rule == S::For.entity() {
            self.exec_for(r, env);
        } else if rule == S::Match.entity() {
            self.exec_match(r, env);
        } else if rule == S::Return.entity() {
            let inner = self
                .model
                .child(r, S::ExpressionStatement)
                .and_then(|s| self.model.child_nodes(s).into_iter().next());
            let value = match inner {
                Some(expression) => self.eval(expression, env),
                None => Interim::Undefined,
            };
            env.ret = Some(value);
        } else if rule == S::Block.entity() {
            let scope = self.model.scope_of(r).unwrap_or(env.scope);
            let saved = env.scope;
            let depth = env.vars.len();
            env.scope = scope;
            for statement in self.model.child_nodes(r) {
                self.exec(statement, env);
            }
            env.scope = saved;
            env.vars.truncate(depth);
        } else if rule == S::Ace.entity() || rule == S::Async.entity() {
            // An `ace` statement runs now, so a value it declares is taken now too.
            // @lfy def/interpret/main.lfy:expand
            let saved = env.ace;
            env.ace = env.ace || rule == S::Ace.entity();
            for statement in self.model.child_nodes(r) {
                self.exec(statement, env);
            }
            env.ace = saved;
        }
        // Loops ended by break, breaks, continues, and uses do nothing at compile time.
    }

    /// A declaration: apply its clauses, then run its body as its own.
    // @lfy def/interpret/main.lfy:expand
    fn exec_declaration(&mut self, r: NodeRef, symbol: SymbolId, env: &mut Env) {
        let entity = self.model.symbols[symbol].entity;
        let kind = self.model.symbols[symbol].kind;
        if matches!(
            kind,
            SymbolKind::Variable
                | SymbolKind::Alias
                | SymbolKind::External
                | SymbolKind::Module
                | SymbolKind::LoopVariable
        ) {
            // A value is taken when its name is read, unless an `ace` asks for it now.
            // @lfy def/interpret/main.lfy:expand
            if env.ace && kind == SymbolKind::Variable {
                let value = self.value_of_variable(entity);
                // An `ace const` of a project file whose value is a `Target` carries every
                // layer of it as one of its traits.
                // @lfy def/interpret/main.lfy:expand#expand:expand:da7602e2b3243080f2623c7ec36d5389af778549e7da59559ebadf2d10ec6e0a
                if !env.dry && self.declares_a_target(entity) {
                    self.apply_layers(r, entity, &value);
                }
            }
            return;
        }
        // The value parameters of the declaration, its type parameters left out.
        let parameters = self.value_parameters(entity);
        match &mut self.model.entities[entity].kind {
            EntityKind::Fn {
                parameters: slot, ..
            }
            | EntityKind::Trait {
                parameters: slot, ..
            } => *slot = parameters,
            _ => {}
        }
        if let Some(definition) = self.model.entities[entity].definition_node {
            let own = Env {
                current: entity,
                target: entity,
                contributor: entity,
                ..env.clone()
            };
            let text = self.text_of(definition, &own);
            self.model.entities[entity].definition = Some(text);
        }
        // The `is` clause of the declaration, of its signature, or of its declared name.
        let holder = self
            .model
            .child(r, E::Signature)
            .or_else(|| self.model.child(r, E::Declared))
            .unwrap_or(r);
        if let Some(is_clause) = self.model.child(holder, E::IsClause) {
            self.apply_clause(entity, is_clause, false, env);
        }
        if let Some(extends) = self.model.child(r, E::ExtendsClause) {
            self.apply_clause(entity, extends, true, env);
        }
        if let Some(block) = self.model.child(r, S::Block) {
            let scope = self.model.entities[entity].scope.unwrap_or(env.scope);
            let mut own = Env {
                vars: Vec::new(),
                current: entity,
                target: entity,
                contributor: entity,
                scope,
                in_trait: false,
                dry: kind == SymbolKind::Trait,
                ace: env.ace,
                ret: None,
            };
            if kind == SymbolKind::Function || kind == SymbolKind::AgentFunction {
                self.exec_context_only(block, &mut own);
            } else {
                self.exec(block, &mut own);
            }
        }
    }

    /// The symbols of the parameters an entity declares, in order.
    // @lfy def/interpret/main.lfy:expand
    fn value_parameters(&self, entity: EntityId) -> Vec<SymbolId> {
        match self.model.entities[entity].scope {
            Some(scope) => self.model.scopes[scope]
                .symbols
                .iter()
                .copied()
                .filter(|&symbol| self.model.symbols[symbol].kind == SymbolKind::Parameter)
                .collect(),
            None => Vec::new(),
        }
    }

    /// Runs only the statements of a function body that run at compile time, so that
    /// runtime code does not run while binding.
    // @lfy def/interpret/main.lfy:expand
    fn exec_context_only(&mut self, block: NodeRef, env: &mut Env) {
        let scope = self.model.scope_of(block).unwrap_or(env.scope);
        let saved = env.scope;
        env.scope = scope;
        for statement in self.model.child_nodes(block) {
            let rule = self.model.rule_of(statement);
            if rule == S::ExpressionStatement.entity() {
                if let Some(expression) = self.model.child_nodes(statement).into_iter().next()
                    && self.is_context_expression(expression)
                {
                    self.eval(expression, env);
                }
            } else if rule == S::Where.entity()
                || rule == S::With.entity()
                || rule == S::For.entity()
                || rule == S::If.entity()
                || rule == S::Ace.entity()
            {
                self.exec(statement, env);
            }
        }
        env.scope = saved;
    }

    /// Whether an expression is a chain rooted at a context accessor, or an `apply`.
    // @lfy def/interpret/main.lfy:expand
    fn is_context_expression(&self, expression: NodeRef) -> bool {
        let model = &*self.model;
        let leftmost = model.leftmost(expression);
        if model.is(leftmost, E::Current)
            && let Some((accessor, _)) = model.accessor_and_name(leftmost)
        {
            return model.token_is(leftmost.file, accessor, P::ContextAccessor);
        }
        let mut node = expression;
        while model.rule_of(node).is_postfix() {
            if model.is(node, E::Member)
                && let Some((accessor, Some(name))) = model.accessor_and_name(node)
                && (model.token_is(node.file, accessor, P::ContextAccessor) || name == "apply")
            {
                return true;
            }
            match model.left(node) {
                Some(left) => node = left,
                None => break,
            }
        }
        false
    }

    /// Each `TraitUse` of a clause becomes one [`Applied`] on the declared entity.
    // @lfy def/interpret/main.lfy:expand
    fn apply_clause(&mut self, entity: EntityId, clause: NodeRef, extends: bool, env: &mut Env) {
        let Some(uses) = self.model.child(clause, E::TraitUses) else {
            return;
        };
        for trait_use in self.model.children_of(uses, E::TraitUse) {
            let Some(symbol) = resolve_trait_use(self.model, trait_use) else {
                continue; // The resolve pass reports a trait name that names nothing.
            };
            let target = self.model.symbols[symbol].entity;
            let arguments = self
                .model
                .child(trait_use, E::Arguments)
                .map(|a| self.model.arguments(a))
                .unwrap_or_default();
            if self.model.entities[target].is_trait() {
                let values = self.eval_arguments(&arguments, env);
                let source = if extends {
                    AppliedSource::Extends(trait_use)
                } else {
                    AppliedSource::Is(trait_use)
                };
                self.apply_trait(entity, target, arguments, values, source, extends);
            } else if extends {
                self.include_members(entity, target);
            } else {
                let name = self.model.entities[target]
                    .identifier
                    .clone()
                    .unwrap_or_default();
                self.problem(trait_use, format!("{name} is not a trait"));
            }
        }
    }

    /// Data extending data includes its members.
    // @lfy def/interpret/main.lfy:expand
    fn include_members(&mut self, receiver: EntityId, from: EntityId) {
        let Some(scope) = self.model.entities[from].scope else {
            return;
        };
        let members: Vec<SymbolId> = self.model.scopes[scope]
            .symbols
            .iter()
            .copied()
            .filter(|&symbol| {
                matches!(
                    self.model.symbols[symbol].kind,
                    SymbolKind::Member | SymbolKind::EnumMember
                )
            })
            .collect();
        for member in members {
            self.join_member(receiver, member);
        }
    }

    /// A member declared in a trait joins the entity's scope; when two applied traits
    /// declare the same member, the most recently applied definition wins.
    // @lfy def/interpret/main.lfy:expand
    fn join_member(&mut self, receiver: EntityId, member: SymbolId) {
        let scope = match self.model.entities[receiver].scope {
            Some(scope) => scope,
            None => {
                let owner = self.model.entities[receiver].node.unwrap_or(SYNTHETIC);
                let scope = self.new_scope(None, owner, receiver);
                self.model.entities[receiver].scope = Some(scope);
                scope
            }
        };
        let name = self.model.symbols[member].name.clone();
        let mut replacing = None;
        if let Some(existing) = self.model.lookup_local(scope, &name) {
            // Only a later trait overrides an earlier one; the entity's own member, or one
            // inherited from data, keeps its definition.
            // @lfy def/interpret/main.lfy:expand
            let existing_trait = self
                .model
                .ancestor(self.model.symbols[existing].node, S::TraitDeclaration);
            let new_trait = self
                .model
                .ancestor(self.model.symbols[member].node, S::TraitDeclaration);
            let two_traits = existing != member
                && self.model.symbols[existing].kind == SymbolKind::Member
                && existing_trait.is_some()
                && new_trait.is_some()
                && existing_trait != new_trait;
            if !two_traits {
                return;
            }
            replacing = self.model.scopes[scope]
                .symbols
                .iter()
                .position(|&symbol| symbol == existing);
        }
        let copy = Symbol {
            name,
            entity: self.model.symbols[member].entity,
            kind: self.model.symbols[member].kind,
            node: self.model.symbols[member].node,
            name_token: self.model.symbols[member].name_token,
            scope,
        };
        self.model.symbols.push(copy);
        let id = self.model.symbols.len() - 1;
        match replacing {
            // The most recently applied trait's definition wins.
            // @lfy def/interpret/main.lfy:expand
            Some(position) => self.model.scopes[scope].symbols[position] = id,
            None => self.model.scopes[scope].symbols.push(id),
        }
    }

    /// The scope an applied trait's members need, for an entity that owned none.
    // @lfy def/interpret/main.lfy:expand
    fn new_scope(&mut self, parent: Option<ScopeId>, owner: NodeRef, current: EntityId) -> ScopeId {
        self.model.scopes.push(Scope {
            parent,
            owner,
            file: owner.file,
            symbols: Vec::new(),
            imports: Vec::new(),
            current,
        });
        self.model.scopes.len() - 1
    }

    /// Evaluates argument nodes where they are written; a spread spreads.
    // @lfy def/interpret/main.lfy:expand
    fn eval_arguments(&mut self, arguments: &[NodeRef], env: &mut Env) -> Vec<Interim> {
        let mut values = Vec::new();
        for &argument in arguments {
            if self.model.is(argument, E::SpreadOperation) {
                let inner = self.model.child_nodes(argument).into_iter().next();
                match inner.map(|i| self.eval(i, env)) {
                    Some(Interim::List(items)) => values.extend(items),
                    Some(other) => values.push(other),
                    None => {}
                }
            } else {
                values.push(self.eval(argument, env));
            }
        }
        values
    }

    /// Applies a trait to an entity: records the application, applies the traits it
    /// extends with their arguments evaluated from its parameters, and runs its body for
    /// the receiver.
    // @lfy def/interpret/main.lfy:expand
    fn apply_trait(
        &mut self,
        receiver: EntityId,
        trait_id: EntityId,
        arguments: Vec<NodeRef>,
        values: Vec<Interim>,
        source: AppliedSource,
        extends: bool,
    ) {
        if receiver == trait_id {
            return;
        }
        let index = self.model.entities[receiver].traits.len();
        self.model.entities[receiver].traits.push(Applied {
            entity: trait_id,
            arguments,
            values: values.clone(),
            source,
        });
        // The receiver joins the trait's entities, or, when it extends it, its extenders,
        // so an extender is never one of its entities.
        // @lfy def/interpret/main.lfy:expand
        if let EntityKind::Trait {
            entities,
            extenders,
            ..
        } = &mut self.model.entities[trait_id].kind
        {
            let list = if extends { extenders } else { entities };
            if !list.contains(&receiver) {
                list.push(receiver);
            }
        }
        if extends {
            return;
        }
        let mut env = self.trait_env(receiver, trait_id, &values);
        // Each extended trait is applied too, up the whole chain, with its arguments
        // evaluated from the extending trait's parameters.
        // @lfy def/interpret/main.lfy:expand
        let bases: Vec<(EntityId, Vec<NodeRef>)> = self.model.entities[trait_id]
            .traits
            .iter()
            .filter(|applied| matches!(applied.source, AppliedSource::Extends(_)))
            .map(|applied| (applied.entity, applied.arguments.clone()))
            .collect();
        for (base, base_arguments) in bases {
            let base_values = self.eval_arguments(&base_arguments, &mut env);
            self.apply_trait(
                receiver,
                base,
                base_arguments,
                base_values,
                AppliedSource::Inherited(index),
                false,
            );
        }
        let Some(trait_node) = self.model.entities[trait_id].node else {
            return;
        };
        if let Some(block) = self.model.child(trait_node, S::Block) {
            // Each member declared in the trait's body joins the entity's scope.
            // @lfy def/interpret/main.lfy:expand
            if let Some(trait_scope) = self.model.entities[trait_id].scope {
                let members: Vec<SymbolId> = self.model.scopes[trait_scope]
                    .symbols
                    .iter()
                    .copied()
                    .filter(|&symbol| self.model.symbols[symbol].kind == SymbolKind::Member)
                    .collect();
                for member in members {
                    self.join_member(receiver, member);
                }
            }
            // The body's value setters, evaluated with the parameters bound to the
            // arguments, and its criteria, with the trait as their contributor.
            // @lfy def/interpret/main.lfy:expand
            // @lfy def/interpret/main.lfy:expand
            self.exec(block, &mut env);
        }
    }

    /// The environment a trait body runs in for a receiver: its parameters bound to the
    /// arguments, `@` the receiver, and the trait the contributor of its criteria.
    // @lfy def/interpret/main.lfy:expand
    fn trait_env(&mut self, receiver: EntityId, trait_id: EntityId, values: &[Interim]) -> Env {
        let parameters: Vec<SymbolId> = self.model.entities[trait_id].parameters().to_vec();
        let mut vars = Vec::new();
        let mut rest = values.iter();
        for parameter in parameters {
            let node = self.model.symbols[parameter].node;
            let name = self.model.symbols[parameter].name.clone();
            let spread = self.model.is(node, E::SpreadParameter);
            let value = if spread {
                Interim::List(rest.by_ref().cloned().collect())
            } else {
                match rest.next() {
                    Some(value) => value.clone(),
                    None => self.default_of(node).unwrap_or(Interim::Undefined),
                }
            };
            vars.push((name, value));
        }
        Env {
            vars,
            current: receiver,
            target: receiver,
            contributor: trait_id,
            scope: self.model.entities[trait_id].scope.unwrap_or(self.universe),
            in_trait: true,
            dry: false,
            ace: false,
            ret: None,
        }
    }

    // ---- Targets and their layers ---------------------------------------------------

    /// The `Target` data of the package `elfie`: the one declared under that name outside
    /// the project's own source, which tells the `ace const` of a target from any other.
    // @lfy def/interpret/main.lfy:expand
    fn target_data(&self) -> Option<EntityId> {
        for (file, &scope) in self.model.file_scopes.iter().enumerate() {
            if self.model.sources.get(file).map(|source| source.origin) != Some(Origin::Library) {
                continue;
            }
            for symbol in self.model.scopes[scope].declared() {
                let symbol = &self.model.symbols[symbol];
                if symbol.kind == SymbolKind::Data && symbol.name == TARGET {
                    return Some(symbol.entity);
                }
            }
        }
        None
    }

    /// Whether an entity is the `ace const` of a project file whose value is a `Target`:
    /// its declared type is that data, or its value is a call of a function whose output
    /// is.
    // @lfy def/interpret/main.lfy:expand
    fn declares_a_target(&self, entity: EntityId) -> bool {
        if !matches!(self.model.entities[entity].kind, EntityKind::Variable) {
            return false;
        }
        let Some(node) = self.model.entities[entity].node else {
            return false;
        };
        if self.model.sources.get(node.file).map(|source| source.origin) != Some(Origin::Program) {
            return false;
        }
        if self.model.ancestor(node, S::Ace).is_none()
            || self.model.has_token(node, K::LetKeyword)
        {
            return false;
        }
        let Some(target) = self.target_data() else {
            return false;
        };
        let holds = Some(TypeRef::Entity(target));
        if self.model.entities[entity].ty == holds {
            return true;
        }
        let value = self
            .model
            .child_nodes(node)
            .into_iter()
            .find(|&child| !self.model.is(child, E::Declared));
        let Some(value) = value.filter(|&value| self.model.is(value, E::Call)) else {
            return false;
        };
        self.model
            .left(value)
            .and_then(|callee| resolve_name_node(self.model, callee))
            .and_then(|symbol| {
                self.model.entities[self.model.symbols[symbol].entity]
                    .output()
                    .cloned()
            })
            == holds
    }

    /// The trait a layer names and the arguments it was chosen with: a trait on its own,
    /// applied with no arguments, or the object `layer` yields.
    // @lfy def/interpret/main.lfy:expand
    fn layer_of(&self, item: &Interim) -> Option<(EntityId, Vec<Interim>)> {
        match item {
            Interim::Entity(entity) if self.model.entities[*entity].is_trait() => {
                Some((*entity, Vec::new()))
            }
            Interim::Object(pairs) => {
                let Some((_, Interim::Entity(subject))) =
                    pairs.iter().find(|(key, _)| key == SUBJECT)
                else {
                    return None;
                };
                if !self.model.entities[*subject].is_trait() {
                    return None;
                }
                let arguments = match pairs.iter().find(|(key, _)| key == ARGUMENTS) {
                    Some((_, Interim::List(items))) => items.clone(),
                    _ => Vec::new(),
                };
                Some((*subject, arguments))
            }
            _ => None,
        }
    }

    /// Applies every layer of a target to the `ace const` that declares it, in guidance
    /// order, with the layer's arguments, so the const carries every layer as one of its
    /// traits, and with it every layer's criteria rendered from those arguments, every
    /// layer's knowledge and commands, and every layer's values, in application order.
    ///
    /// The arguments were evaluated where the layer was chosen, so the application records
    /// their values and no argument nodes.
    // @lfy def/interpret/main.lfy:expand#expand:expand:da7602e2b3243080f2623c7ec36d5389af778549e7da59559ebadf2d10ec6e0a
    // @lfy def/interpret/main.lfy:expand#expand:expand:24896a9f182aab4cba64abb17efdb629e1de1a7ac24c9edcdbc3fa14873d813b
    fn apply_layers(&mut self, declaration: NodeRef, entity: EntityId, value: &Interim) {
        let Interim::Object(pairs) = value else {
            return;
        };
        let pairs = pairs.clone();
        let mut layers: Vec<(EntityId, Vec<Interim>)> = Vec::new();
        for slot in SLOTS {
            let Some((_, held)) = pairs.iter().find(|(key, _)| key == slot) else {
                continue;
            };
            let items = match held {
                Interim::Undefined | Interim::Null => continue,
                Interim::List(items) => items.clone(),
                one => vec![one.clone()],
            };
            for item in items {
                match self.layer_of(&item) {
                    // A trait in two slots with equal arguments is one layer, at its first
                    // place in guidance order.
                    // @lfy def/interpret/main.lfy:expand#expand:expand:da7602e2b3243080f2623c7ec36d5389af778549e7da59559ebadf2d10ec6e0a
                    Some(layer) => {
                        if !layers.contains(&layer) {
                            layers.push(layer);
                        }
                    }
                    // A slot holding neither a trait nor a layer of one.
                    // @lfy def/interpret/main.lfy:expand#expand:expand:f2da259bc931b16b67600269961004bc2fdf0d760814936c7268ae3ed27c6981
                    None => self.problem(
                        declaration,
                        format!("the {slot} of this target is neither a trait nor a layer of one"),
                    ),
                }
            }
        }
        for (subject, arguments) in layers {
            self.apply_trait(
                entity,
                subject,
                Vec::new(),
                arguments,
                AppliedSource::Apply(declaration),
                false,
            );
        }
    }

    /// The layer of a trait with these arguments: the object holding the trait as its
    /// subject and the arguments, in order. Nothing is applied by choosing one.
    // @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
    fn layer_value(
        &mut self,
        call: NodeRef,
        subject: EntityId,
        arguments: Vec<Interim>,
    ) -> Interim {
        self.check_layer_arity(call, subject, arguments.len());
        Interim::Object(vec![
            (SUBJECT.to_string(), Interim::Entity(subject)),
            (ARGUMENTS.to_string(), Interim::List(arguments)),
        ])
    }

    /// Names the trait at a call of `layer` that gives a different number of arguments than
    /// the trait has parameters; a spread parameter takes any number.
    // @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
    fn check_layer_arity(&mut self, call: NodeRef, subject: EntityId, given: usize) {
        let parameters: Vec<SymbolId> = self.model.entities[subject].parameters().to_vec();
        let spread = parameters
            .last()
            .is_some_and(|&last| self.model.is(self.model.symbols[last].node, E::SpreadParameter));
        let wanted = parameters.len();
        if given == wanted || (spread && given + 1 >= wanted) {
            return;
        }
        let name = self.model.entities[subject]
            .identifier
            .clone()
            .unwrap_or_else(|| "a trait".to_string());
        self.problem(
            call,
            format!("{name} takes {wanted} arguments as a layer, not {given}"),
        );
    }

    /// The default value of a parameter, read from what follows its setter.
    // @lfy def/interpret/main.lfy:expand
    fn default_of(&mut self, parameter: NodeRef) -> Option<Interim> {
        if !self.model.has_token(parameter, P::PlainSetter) {
            return None;
        }
        let value = self.model.child_nodes(parameter).into_iter().last()?;
        if self.model.is(value, E::Name) || self.model.is(value, E::DefinitionClause) {
            return None;
        }
        let text = self.model.raw(value).trim().to_string();
        Some(match text.as_str() {
            "[]" => Interim::List(Vec::new()),
            "{}" => Interim::Object(Vec::new()),
            "false" => Interim::Bool(false),
            "true" => Interim::Bool(true),
            "null" => Interim::Null,
            "undefined" => Interim::Undefined,
            quoted if quoted.starts_with('\'') || quoted.starts_with('"') => {
                Interim::String(quoted[1..quoted.len() - 1].to_string())
            }
            other => Interim::String(other.to_string()),
        })
    }

    /// A `Where` adds a criterion: the conditions are the situation, the expression the
    /// behavior.
    // @lfy def/interpret/main.lfy:expand
    fn exec_where(&mut self, r: NodeRef, env: &mut Env) {
        let Some(conditions) = self.model.child(r, S::Conditions) else {
            return;
        };
        let situation = self.conditions_situations(conditions, env).texts();
        let behavior = self
            .model
            .child(r, S::ExpressionStatement)
            .and_then(|s| self.model.child_nodes(s).into_iter().next())
            .map(|e| self.text_of(e, env));
        let criterion = Criterion {
            situation: Some(situation),
            behavior: behavior.map(|text| vec![text]),
            side_effects: None,
            contributor: env.contributor,
            node: Some(r),
        };
        self.model.entities[env.target]
            .acceptance_criteria
            .push(criterion);
    }

    /// The conditions as one [`Situations`]: those an "and" joins all hold, and those an
    /// "or" joins within each of them hold one at a time.
    // @lfy def/interpret/main.lfy:expand
    fn conditions_situations(&mut self, conditions: NodeRef, env: &mut Env) -> Situations {
        let mut all: Vec<Situations> = Vec::new();
        let mut any: Vec<Situations> = Vec::new();
        for part in self.model.parts(conditions) {
            match part {
                Part::Node(condition) => any.push(self.condition_situations(condition, env)),
                Part::Token(index)
                    if self.model.token_is(conditions.file, index, K::AndKeyword) =>
                {
                    all.push(Situations::Any(std::mem::take(&mut any)))
                }
                Part::Token(_) => {}
            }
        }
        all.push(Situations::Any(any));
        Situations::All(all)
    }

    /// One condition as the situations it stands for, with a `!` negating what follows it.
    // @lfy def/interpret/main.lfy:expand
    fn condition_situations(&mut self, condition: NodeRef, env: &mut Env) -> Situations {
        let negated = self.model.has_token(condition, P::LogicalNot);
        let Some(inner) = self.model.child_nodes(condition).into_iter().next() else {
            return Situations::All(Vec::new());
        };
        let situations = if self.model.is(inner, S::ConditionGroup) {
            match self.model.child(inner, S::Conditions) {
                Some(nested) => self.conditions_situations(nested, env),
                None => Situations::All(Vec::new()),
            }
        } else if self.model.is(inner, E::Group) {
            match self.model.child_nodes(inner).into_iter().next() {
                Some(expression) => Situations::One(self.text_of(expression, env)),
                None => Situations::All(Vec::new()),
            }
        } else {
            Situations::One(self.text_of(inner, env))
        };
        if negated {
            situations.negated()
        } else {
            situations
        }
    }

    /// A `With` runs the block with the name or member as the entity criteria attach to.
    // @lfy def/interpret/main.lfy:expand
    fn exec_with(&mut self, r: NodeRef, env: &mut Env) {
        // A trait's own body is run without a receiver; its `with` blocks belong to each
        // application, so they are skipped here.
        if env.dry
            && !env.in_trait
            && matches!(
                self.model.entities[env.current].kind,
                EntityKind::Trait { .. }
            )
        {
            return;
        }
        let children = self.model.child_nodes(r);
        let Some(&expression) = children.iter().find(|&&c| !self.model.is(c, S::Block)) else {
            return;
        };
        let Some(&block) = children.iter().find(|&&c| self.model.is(c, S::Block)) else {
            return;
        };
        let target = match self.eval(expression, env) {
            Interim::Entity(entity) => entity,
            _ => match resolve_name_node(self.model, expression) {
                Some(symbol) => self.model.symbols[symbol].entity,
                None => return,
            },
        };
        let mut inner = env.clone();
        inner.target = target;
        if !env.in_trait {
            inner.current = target;
            inner.contributor = target;
        }
        if let Some(scope) = self.model.scope_of(r) {
            inner.scope = scope;
        }
        self.exec(block, &mut inner);
        env.ret = inner.ret;
    }

    // @lfy def/interpret/main.lfy:expand
    fn exec_if(&mut self, r: NodeRef, env: &mut Env) {
        let children = self.model.child_nodes(r);
        let Some(&condition) = children.first() else {
            return;
        };
        let taken = self.eval(condition, env).is_truthy();
        if taken {
            if let Some(&branch) = children.get(1) {
                self.exec(branch, env);
            }
        } else if let Some(&other) = children.iter().find(|&&c| self.model.is(c, S::Else))
            && let Some(branch) = self.model.child_nodes(other).into_iter().next()
        {
            self.exec(branch, env);
        }
    }

    /// A loop over one or more iterables, visited in order.
    // @lfy def/interpret/main.lfy:expand
    fn exec_for(&mut self, r: NodeRef, env: &mut Env) {
        let Some(block) = self.model.child(r, S::Block) else {
            return;
        };
        let Some((names, rounds)) = self.loop_bindings(r, env) else {
            return;
        };
        let scope = self.model.scope_of(r).unwrap_or(env.scope);
        let outer = env.vars.len();
        for round in rounds {
            let mut inner = env.clone();
            inner.scope = scope;
            for (name, value) in names.iter().zip(round) {
                inner.vars.push((name.clone(), value));
            }
            self.exec(block, &mut inner);
            if inner.ret.is_some() {
                env.ret = inner.ret;
                return;
            }
            // Mutations of outer variables flow back.
            for (name, value) in inner.vars.into_iter().take(outer) {
                env.set(&name, value);
            }
        }
    }

    /// The names a loop declares and, for each round, what they hold: the values of its
    /// iterables (`in`), their names (`of`), or both (`from`).
    // @lfy def/interpret/main.lfy:expand
    fn loop_bindings(
        &mut self,
        r: NodeRef,
        env: &mut Env,
    ) -> Option<(Vec<String>, Vec<Vec<Interim>>)> {
        let declared = self.model.child(r, E::Declared)?;
        let mut names = vec![self.model.name(declared)?];
        let from = self.model.child(r, S::ForFrom);
        let (iterables, keys) = match (self.model.child(r, S::ForInOf), from) {
            (Some(in_of), _) => (
                self.model.child_nodes(in_of),
                self.model.has_token(in_of, K::OfKeyword),
            ),
            (None, Some(from)) => {
                names.extend(self.model.declared_name(from));
                (
                    self.model
                        .child_nodes(from)
                        .into_iter()
                        .filter(|&c| !self.model.is(c, E::Declared))
                        .collect(),
                    false,
                )
            }
            _ => return None,
        };
        let mut rounds = Vec::new();
        for iterable in iterables {
            let value = self.eval(iterable, env);
            if from.is_some() {
                // `from` visits names and values together.
                // @lfy def/interpret/main.lfy:expand
                let values = self.iterate(value.clone(), false);
                for (name, value) in self.iterate(value, true).into_iter().zip(values) {
                    rounds.push(vec![name, value]);
                }
            } else {
                rounds.extend(self.iterate(value, keys).into_iter().map(|item| vec![item]));
            }
        }
        Some((names, rounds))
    }

    /// What a value yields in a loop: its items, or their keys.
    // @lfy def/interpret/main.lfy:expand
    fn iterate(&self, value: Interim, keys: bool) -> Vec<Interim> {
        match value {
            Interim::List(items) => {
                if keys {
                    (0..items.len())
                        .map(|index| Interim::Number(index as f64))
                        .collect()
                } else {
                    items
                }
            }
            Interim::Object(pairs) => pairs
                .into_iter()
                .map(|(key, value)| if keys { Interim::String(key) } else { value })
                .collect(),
            Interim::Entity(entity) => match self.model.entities[entity].scope {
                Some(scope) => self.model.scopes[scope]
                    .symbols
                    .iter()
                    .map(|&symbol| {
                        if keys {
                            Interim::String(self.model.symbols[symbol].name.clone())
                        } else {
                            Interim::Entity(self.model.symbols[symbol].entity)
                        }
                    })
                    .collect(),
                None => Vec::new(),
            },
            Interim::String(text) => text
                .chars()
                .map(|character| Interim::String(character.to_string()))
                .collect(),
            _ => Vec::new(),
        }
    }

    // @lfy def/interpret/main.lfy:expand
    fn exec_match(&mut self, r: NodeRef, env: &mut Env) {
        let all = self.model.has_token(r, K::MatchallKeyword);
        let children = self.model.child_nodes(r);
        let Some(&scrutinee) = children.first() else {
            return;
        };
        let value = self.eval(scrutinee, env);
        let arms: Vec<NodeRef> = children
            .iter()
            .copied()
            .filter(|&c| self.model.is(c, S::MatchArm))
            .collect();
        for arm in arms {
            let parts = self.model.child_nodes(arm);
            let is_default = self.model.has_token(arm, K::DefaultKeyword);
            let (pattern, result) = if is_default {
                (None, parts.first().copied())
            } else {
                (parts.first().copied(), parts.get(1).copied())
            };
            let matched = match pattern {
                None => true,
                Some(pattern) => self.eval(pattern, env) == value,
            };
            if matched {
                if let Some(result) = result {
                    self.eval(result, env);
                }
                if !all {
                    return;
                }
            }
        }
    }

    /// Runs a block as code: variable declarations bind, loops run, returns return.
    // @lfy def/interpret/main.lfy:expand
    fn exec_code(&mut self, block: NodeRef, env: &mut Env) {
        let scope = self.model.scope_of(block).unwrap_or(env.scope);
        let saved = env.scope;
        env.scope = scope;
        for statement in self.model.child_nodes(block) {
            if env.ret.is_some() {
                break;
            }
            if self.model.is(statement, S::VariableDeclaration) {
                let name = self.model.declared_name(statement);
                let expression = self
                    .model
                    .child_nodes(statement)
                    .into_iter()
                    .find(|&c| !self.model.is(c, E::Declared));
                let value = match expression {
                    Some(expression) => self.eval(expression, env),
                    None => Interim::Undefined,
                };
                if let Some(name) = name {
                    env.vars.push((name, value));
                }
            } else if self.model.is(statement, S::Block) {
                self.exec_code(statement, env);
            } else if self.model.is(statement, S::For) {
                self.exec_for_code(statement, env);
            } else {
                self.exec(statement, env);
            }
        }
        env.scope = saved;
    }

    // @lfy def/interpret/main.lfy:expand
    fn exec_for_code(&mut self, r: NodeRef, env: &mut Env) {
        let Some(block) = self.model.child(r, S::Block) else {
            return;
        };
        let Some((names, rounds)) = self.loop_bindings(r, env) else {
            return;
        };
        let outer = env.vars.len();
        for round in rounds {
            for (name, value) in names.iter().zip(round) {
                env.vars.push((name.clone(), value));
            }
            self.exec_code(block, env);
            env.vars.truncate(outer);
            if env.ret.is_some() {
                return;
            }
        }
    }

    // @lfy def/interpret/main.lfy:expand
    fn bind_parameters(&mut self, function: NodeRef, values: &[Interim], env: &mut Env) {
        let parameters = self.model.child(function, E::Parameters).or_else(|| {
            self.model
                .child(function, E::Signature)
                .and_then(|s| self.model.child(s, E::Parameters))
        });
        let Some(parameters) = parameters else {
            return;
        };
        let mut rest = values.iter();
        for parameter in self.model.child_nodes(parameters) {
            let spread = self.model.is(parameter, E::SpreadParameter);
            let Some(name) = self
                .model
                .child(parameter, E::Name)
                .and_then(|n| self.model.name(n))
            else {
                continue;
            };
            let value = if spread {
                Interim::List(rest.by_ref().cloned().collect())
            } else {
                match rest.next() {
                    Some(value) => value.clone(),
                    None => self.default_of(parameter).unwrap_or(Interim::Undefined),
                }
            };
            env.vars.push((name, value));
        }
    }

    // ---- Expressions ---------------------------------------------------------------

    /// Notes that what was read only has a value at runtime, or could not be read at all,
    /// so [`evaluate`] gives nothing back and [`lower`] folds nothing here.
    // @lfy def/interpret/main.lfy:evaluate
    fn only_at_runtime(&mut self) -> Interim {
        self.runtime = true;
        Interim::Undefined
    }

    /// Evaluates an expression at compile time.
    // @lfy def/interpret/main.lfy:evaluate
    fn eval(&mut self, r: NodeRef, env: &mut Env) -> Interim {
        let rule = self.model.rule_of(r);
        if rule == E::Name.entity() {
            let Some(name) = self.model.name(r) else {
                return self.only_at_runtime();
            };
            if let Some(value) = env.get(&name) {
                return value.clone();
            }
            let symbol = lookup(self.model, env.scope, &name)
                .or_else(|| lookup(self.model, self.model.enclosing_scope(r), &name));
            return match symbol {
                Some(symbol) => self.value_of_symbol(symbol),
                None => self.only_at_runtime(),
            };
        }
        if rule == E::StringLiteral.entity() {
            return Interim::String(self.model.string_value(r).unwrap_or_default());
        }
        if rule == E::Template.entity() {
            return Interim::String(self.template_text(r, env));
        }
        if rule == E::Number.entity() {
            let text = self.model.raw(r).trim().replace('_', "");
            return Interim::Number(text.parse().unwrap_or(0.0));
        }
        if rule == E::Boolean.entity() {
            return Interim::Bool(self.model.raw(r).trim() == "true");
        }
        if rule == E::Nullish.entity() {
            return if self.model.raw(r).trim() == "null" {
                Interim::Null
            } else {
                Interim::Undefined
            };
        }
        if rule == E::PrimitiveType.entity()
            || rule == E::TypeExpression.entity()
            || rule == E::TypePredicate.entity()
            || rule == E::Generic.entity()
        {
            return Interim::Type(Box::new(self.type_of(r)));
        }
        if rule == E::List.entity() {
            let items = self
                .model
                .child(r, E::Items)
                .map(|items| self.model.child_nodes(items))
                .unwrap_or_default();
            return Interim::List(self.eval_arguments(&items, env));
        }
        if rule == E::Object.entity() {
            let mut pairs = Vec::new();
            for key in self.model.children_of(r, E::ObjectKey) {
                let name = self.model.declared_name(key).unwrap_or_default();
                let expression = self
                    .model
                    .child_nodes(key)
                    .into_iter()
                    .find(|&c| !self.model.is(c, E::Declared));
                let value = match expression {
                    Some(expression) => self.eval(expression, env),
                    None => Interim::Undefined,
                };
                pairs.push((name, value));
            }
            return Interim::Object(pairs);
        }
        if rule == E::Group.entity()
            || rule == E::Definition.entity()
            || rule == E::Cast.entity()
            || rule == E::Reference.entity()
            || rule == E::SpreadOperation.entity()
            || rule == E::AwaitOperation.entity()
            || rule == E::InOperation.entity()
            || rule == E::OfOperation.entity()
            || rule == E::FromOperation.entity()
        {
            return match self.model.child_nodes(r).into_iter().next() {
                Some(inner) => self.eval(inner, env),
                None => Interim::Undefined,
            };
        }
        if rule == E::Current.entity() {
            return self.eval_current(r, env);
        }
        if rule == E::Member.entity() {
            return self.eval_member(r, env);
        }
        if rule == E::Call.entity() {
            return self.eval_call(r, env);
        }
        if rule == E::InlineFunction.entity() {
            return Interim::Closure(r, env.vars.clone());
        }
        if rule == E::Dereference.entity() {
            let Some(operand) = self.model.child_nodes(r).into_iter().next() else {
                return Interim::Undefined;
            };
            return match self.eval(operand, env) {
                Interim::Entity(entity) => Interim::Entity(entity),
                _ => match resolve_name_node(self.model, operand) {
                    Some(symbol) => Interim::Entity(self.model.symbols[symbol].entity),
                    None => self.only_at_runtime(),
                },
            };
        }
        if rule == E::Previous.entity() {
            return self.eval_previous(r);
        }
        if rule == E::NotOperation.entity() {
            let operand = self.model.child_nodes(r).into_iter().next();
            let truthy = match operand {
                Some(operand) => self.eval(operand, env).is_truthy(),
                None => false,
            };
            return Interim::Bool(!truthy);
        }
        if rule == E::NegateOperation.entity() {
            let operand = self.model.child_nodes(r).into_iter().next();
            return match operand.map(|operand| self.eval(operand, env)) {
                Some(Interim::Number(number)) => Interim::Number(-number),
                _ => self.only_at_runtime(),
            };
        }
        if rule == E::Traits.entity() {
            return self.eval_traits(r, env);
        }
        if rule == E::Conditional.entity() {
            let parts = self.model.child_nodes(r);
            if parts.len() == 3 {
                return if self.eval(parts[0], env).is_truthy() {
                    self.eval(parts[1], env)
                } else {
                    self.eval(parts[2], env)
                };
            }
            return self.only_at_runtime();
        }
        if rule == E::Assignment.entity() {
            return self.eval_assignment(r, env);
        }
        if rule == E::Index.entity() {
            return self.eval_index(r, env);
        }
        if rule.is_infix() {
            return self.eval_infix(r, env);
        }
        // Nothing else has a value the interpreter can read.
        self.only_at_runtime()
    }

    /// `x is T`: whether every trait named is applied to the left side.
    // @lfy def/interpret/main.lfy:evaluate
    fn eval_traits(&mut self, r: NodeRef, env: &mut Env) -> Interim {
        let Some(left) = self.model.left(r) else {
            return Interim::Bool(false);
        };
        let Interim::Entity(entity) = self.eval(left, env) else {
            return Interim::Bool(false);
        };
        let uses = self
            .model
            .child(r, E::TraitUses)
            .map(|uses| self.model.children_of(uses, E::TraitUse))
            .unwrap_or_default();
        let all = uses.iter().all(|&use_node| {
            resolve_trait_use(self.model, use_node)
                .map(|symbol| self.model.symbols[symbol].entity)
                .is_some_and(|applied| self.model.entities[entity].has_trait(applied))
        });
        Interim::Bool(all && !uses.is_empty())
    }

    /// An element of the left side, or, with nothing inside, the array type of it.
    // @lfy def/interpret/main.lfy:evaluate
    fn eval_index(&mut self, r: NodeRef, env: &mut Env) -> Interim {
        let parts = self.model.child_nodes(r);
        if parts.len() == 1 {
            return Interim::Type(Box::new(self.type_of(r)));
        }
        if parts.len() != 2 {
            return self.only_at_runtime();
        }
        let left = self.eval(parts[0], env);
        let index = self.eval(parts[1], env);
        match (left, index) {
            (Interim::List(items), Interim::Number(at)) => {
                items.get(at as usize).cloned().unwrap_or(Interim::Undefined)
            }
            (Interim::Object(pairs), Interim::String(key)) => pairs
                .into_iter()
                .find(|(held, _)| *held == key)
                .map_or(Interim::Undefined, |(_, value)| value),
            _ => self.only_at_runtime(),
        }
    }

    // @lfy def/interpret/main.lfy:evaluate
    fn eval_infix(&mut self, r: NodeRef, env: &mut Env) -> Interim {
        let rule = self.model.rule_of(r);
        let parts = self.model.child_nodes(r);
        if parts.len() != 2 {
            return self.only_at_runtime();
        }
        if rule == E::LogicalAndOperation.entity() {
            let left = self.eval(parts[0], env);
            return if left.is_truthy() {
                self.eval(parts[1], env)
            } else {
                left
            };
        }
        if rule == E::CoalescenceOperation.entity() {
            let left = self.eval(parts[0], env);
            let nullish = self
                .model
                .operator(r)
                .is_some_and(|index| self.model.token_is(r.file, index, P::NullishOr));
            let take_right = if nullish {
                matches!(left, Interim::Null | Interim::Undefined)
            } else {
                !left.is_truthy()
            };
            return if take_right {
                self.eval(parts[1], env)
            } else {
                left
            };
        }
        let left = self.eval(parts[0], env);
        let right = self.eval(parts[1], env);
        if rule == E::EqualityOperation.entity() {
            let negated = self
                .model
                .operator(r)
                .is_some_and(|index| self.model.token_is(r.file, index, P::NotEqual));
            return Interim::Bool((left == right) != negated);
        }
        if rule == E::AdditiveOperation.entity() {
            let minus = self
                .model
                .operator(r)
                .is_some_and(|index| self.model.token_is(r.file, index, P::Minus));
            return match (left, right) {
                (Interim::Number(a), Interim::Number(b)) => {
                    Interim::Number(if minus { a - b } else { a + b })
                }
                (a, b) if !minus => {
                    Interim::String(format!("{}{}", self.to_text(&a), self.to_text(&b)))
                }
                _ => self.only_at_runtime(),
            };
        }
        if rule == E::BitwiseOrOperation.entity() {
            // A union when both sides are types.
            // @lfy def/interpret/main.lfy:evaluate
            if let (Interim::Type(a), Interim::Type(b)) = (&left, &right) {
                let mut items = match (**a).clone() {
                    TypeRef::Union(items) => items,
                    other => vec![other],
                };
                match (**b).clone() {
                    TypeRef::Union(more) => items.extend(more),
                    other => items.push(other),
                }
                return Interim::Type(Box::new(TypeRef::Union(items)));
            }
            return self.only_at_runtime();
        }
        let operator = self
            .model
            .operator(r)
            .map(|index| self.model.token_at(r.file, index).value.clone())
            .unwrap_or_default();
        if rule == E::MultiplicativeOperation.entity() || rule == E::PowerOperation.entity() {
            if let (Interim::Number(a), Interim::Number(b)) = (left, right) {
                return Interim::Number(match operator.as_str() {
                    "*" => a * b,
                    "/" => a / b,
                    "%" => a % b,
                    "**" => a.powf(b),
                    _ => a,
                });
            }
            return self.only_at_runtime();
        }
        if rule == E::RelationalOperation.entity() {
            if let (Interim::Number(a), Interim::Number(b)) = (left, right) {
                return Interim::Bool(match operator.as_str() {
                    "<" => a < b,
                    "<=" => a <= b,
                    ">" => a > b,
                    ">=" => a >= b,
                    _ => false,
                });
            }
            return self.only_at_runtime();
        }
        self.only_at_runtime()
    }

    /// `.name = value` sets a value on the current entity; `name = value` rebinds a
    /// variable.
    // @lfy def/interpret/main.lfy:expand
    fn eval_assignment(&mut self, r: NodeRef, env: &mut Env) -> Interim {
        let parts = self.model.child_nodes(r);
        if parts.len() != 2 {
            return Interim::Undefined;
        }
        let (left, right) = (parts[0], parts[1]);
        // A member declaration declares, it does not assign.
        if self.model.is(left, E::Definition) || self.model.symbol_of(left).is_some() {
            return Interim::Undefined;
        }
        if self.model.is(left, E::Current) {
            let Some((accessor, Some(name))) = self.model.accessor_and_name(left) else {
                return Interim::Undefined;
            };
            if self.model.token_is(left.file, accessor, P::ScopeAccessor) {
                return Interim::Undefined;
            }
            let value = self.eval(right, env);
            if !env.dry {
                self.model.entities[env.current]
                    .values
                    .push((name, value.clone()));
            }
            return value;
        }
        if self.model.is(left, E::Name) {
            let value = self.eval(right, env);
            if let Some(name) = self.model.name(left) {
                env.set(&name, value.clone());
            }
            return value;
        }
        Interim::Undefined
    }

    /// `@property`, `$member`, `.member`, or a bare accessor on the current entity.
    // @lfy def/interpret/main.lfy:expand
    fn eval_current(&mut self, r: NodeRef, env: &mut Env) -> Interim {
        let Some((accessor, name)) = self.model.accessor_and_name(r) else {
            return Interim::Undefined;
        };
        let Some(rule) = self.model.token_at(r.file, accessor).rule else {
            return Interim::Undefined;
        };
        // A `Current` with no member name is the scope's current entity, which at the top
        // level of a file is the anonymous entity for the file.
        // @lfy def/interpret/main.lfy:expand#expand:expand:0ebadc4acebce99f628486e58f24cf71f0da3ccd7eac9ea7d09c7b2bd0db2553
        // @lfy def/interpret/main.lfy:expand#expand:expand:0c816239c7adfd2752ecb13151d98fd53744cd8d23e3eec6f1df8da00917419d
        let current = env.current;
        self.access(Interim::Entity(current), rule, name.as_deref(), env)
    }

    /// `^^`: the entity the nearest earlier statement declared. What it reads depends
    /// only on the model, so it has a value at compile time and what uses it can be
    /// folded. The usage the resolve pass bound answers it once the model is resolved;
    /// while the expand pass is still running there is no usage yet, so the nearest
    /// earlier statement is found the way the binder finds it.
    // @lfy def/interpret/main.lfy:evaluate
    fn eval_previous(&mut self, r: NodeRef) -> Interim {
        let symbol = self
            .model
            .usage_of(r)
            .and_then(|usage| self.model.usages[usage].symbol)
            .or_else(|| previous_symbol(self.model, r));
        match symbol {
            Some(symbol) => Interim::Entity(self.model.symbols[symbol].entity),
            None => self.only_at_runtime(),
        }
    }

    // @lfy def/interpret/main.lfy:evaluate
    fn eval_member(&mut self, r: NodeRef, env: &mut Env) -> Interim {
        let Some(left) = self.model.left(r) else {
            return Interim::Undefined;
        };
        let Some((accessor, name)) = self.model.accessor_and_name(r) else {
            return Interim::Undefined;
        };
        let Some(rule) = self.model.token_at(r.file, accessor).rule else {
            return Interim::Undefined;
        };
        // A context layer of a name bound to a `const` or a `let` is read from the entity
        // the declaration is, never from the value it holds, so an `add` on a const's
        // criteria, knowledge, commands, or targets reaches the const as it reaches any
        // declared entity.
        // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
        let declared = self
            .model
            .token_is(r.file, accessor, P::ContextAccessor)
            .then(|| self.declared_by(left, env))
            .flatten();
        let value = match declared {
            Some(entity) => Interim::Entity(entity),
            None => self.eval(left, env),
        };
        self.access(value, rule, name.as_deref(), env)
    }

    /// The entity a name bound to a `const` or a `let` declares, when it is one: what its
    /// context layer is read from.
    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    fn declared_by(&self, left: NodeRef, env: &Env) -> Option<EntityId> {
        if !self.model.is(left, E::Name) {
            return None;
        }
        let name = self.model.name(left)?;
        if env.get(&name).is_some() {
            return None;
        }
        let symbol = lookup(self.model, env.scope, &name)
            .or_else(|| lookup(self.model, self.model.enclosing_scope(left), &name))?;
        let record = &self.model.symbols[symbol];
        (record.kind == SymbolKind::Variable).then_some(record.entity)
    }

    /// Reads one layer of a value.
    // @lfy def/interpret/main.lfy:evaluate
    fn access(
        &mut self,
        value: Interim,
        accessor: Rule,
        name: Option<&str>,
        env: &mut Env,
    ) -> Interim {
        let Some(layer) = accessor_layer(accessor) else {
            return Interim::Undefined;
        };
        match layer {
            Layer::Context => {
                let Some(name) = name else { return value };
                self.context_property(&value, name)
            }
            Layer::Scope => {
                let Interim::Entity(entity) = value else {
                    return self.only_at_runtime();
                };
                match name {
                    None => self.model.entities[entity]
                        .scope
                        .map_or(Interim::Undefined, Interim::Scope),
                    Some(name) => self.member_value(entity, name, env),
                }
            }
            // The scope that contains the left side's scope, or undefined when there is
            // none.
            Layer::Parent => {
                let Interim::Entity(entity) = value else {
                    return self.only_at_runtime();
                };
                self.model.entities[entity]
                    .scope
                    .and_then(|scope| self.model.scopes[scope].parent)
                    .map_or(Interim::Undefined, Interim::Scope)
            }
            Layer::Value => {
                let Some(name) = name else { return value };
                match value {
                    Interim::Entity(entity) => self.member_value(entity, name, env),
                    Interim::Object(pairs) => pairs
                        .into_iter()
                        .find(|(held, _)| held == name)
                        .map_or(Interim::Undefined, |(_, value)| value),
                    Interim::List(items) if name == "length" => {
                        Interim::Number(items.len() as f64)
                    }
                    Interim::String(text) if name == "length" => {
                        Interim::Number(text.chars().count() as f64)
                    }
                    _ => self.only_at_runtime(),
                }
            }
            Layer::Dereference | Layer::Previous => self.only_at_runtime(),
        }
    }

    /// A context property of a value.
    // @lfy def/interpret/main.lfy:evaluate
    fn context_property(&mut self, value: &Interim, name: &str) -> Interim {
        match value {
            Interim::Entity(entity) => self.entity_property(*entity, name, value),
            Interim::Type(ty) => match name {
                "type" => Interim::Type(ty.clone()),
                LIKE => Interim::Like(Box::new(value.clone())),
                "itemType" => match &**ty {
                    TypeRef::List(item) => Interim::Type(item.clone()),
                    _ => Interim::Undefined,
                },
                _ => Interim::Undefined,
            },
            Interim::String(_) => match name {
                "type" => Interim::Type(Box::new(TypeRef::Primitive("string"))),
                LIKE => Interim::Like(Box::new(value.clone())),
                _ => Interim::Undefined,
            },
            Interim::Number(_) => match name {
                "type" => Interim::Type(Box::new(TypeRef::Primitive("number"))),
                LIKE => Interim::Like(Box::new(value.clone())),
                _ => Interim::Undefined,
            },
            Interim::Bool(_) => match name {
                "type" => Interim::Type(Box::new(TypeRef::Primitive("boolean"))),
                LIKE => Interim::Like(Box::new(value.clone())),
                _ => Interim::Undefined,
            },
            Interim::List(items) => match name {
                "type" => Interim::Type(Box::new(TypeRef::List(Box::new(TypeRef::Unknown(
                    String::new(),
                ))))),
                LIKE => Interim::Like(Box::new(value.clone())),
                "itemType" => {
                    let first = items.first().cloned();
                    match first {
                        Some(first) => self.context_property(&first, "type"),
                        None => Interim::Undefined,
                    }
                }
                _ => Interim::Undefined,
            },
            _ => match name {
                LIKE => Interim::Like(Box::new(value.clone())),
                _ => Interim::Undefined,
            },
        }
    }

    /// One context property of an entity.
    // @lfy def/interpret/main.lfy:evaluate
    fn entity_property(&mut self, entity: EntityId, name: &str, value: &Interim) -> Interim {
        // A name that is a member of a kind data other than the entity's yields undefined
        // for it, so `@parameters ?? @type` stays meaningful.
        if self.prelude.is_some()
            && self.kind_member(entity, name).is_none()
            && self.kind_member_anywhere(name)
        {
            return Interim::Undefined;
        }
        match name {
            TYPE_ARGUMENTS => {
                return Interim::List(
                    self.model
                        .type_arguments(entity)
                        .into_iter()
                        .map(|ty| Interim::Type(Box::new(ty)))
                        .collect(),
                );
            }
            TYPE_PARAMETERS => {
                return Interim::List(
                    self.model
                        .type_parameters(entity)
                        .into_iter()
                        .map(Interim::Entity)
                        .collect(),
                );
            }
            // The items added to the entity, in the order they were added.
            // @lfy def/interpret/main.lfy:expand
            KNOWLEDGE => return self.added_items(entity, Added::Knowledge),
            COMMANDS => return self.added_items(entity, Added::Commands),
            TARGETS => return self.added_items(entity, Added::Targets),
            "defaultValue" | "optional" | "spread" => {
                if let Some(value) = self.model.entities[entity].value(name) {
                    return value.clone();
                }
            }
            _ => {}
        }
        match ContextProperty::lookup(name) {
            Some(ContextProperty::Identifier) => self.model.entities[entity]
                .identifier
                .clone()
                .map_or(Interim::Undefined, Interim::String),
            Some(ContextProperty::Definition) => {
                if let Some(definition) = self.model.entities[entity].definition.clone() {
                    return Interim::String(definition);
                }
                let Some(node) = self.model.entities[entity].definition_node else {
                    return Interim::Undefined;
                };
                let scope = self.model.entities[entity].scope.unwrap_or(self.universe);
                let env = Env {
                    vars: Vec::new(),
                    current: entity,
                    target: entity,
                    contributor: entity,
                    scope,
                    in_trait: false,
                    dry: true,
                    ace: false,
                    ret: None,
                };
                let text = self.text_of(node, &env);
                if self.writes {
                    self.model.entities[entity].definition = Some(text.clone());
                }
                Interim::String(text)
            }
            Some(ContextProperty::Type) => self.model.entities[entity]
                .ty
                .clone()
                .map_or(Interim::Undefined, |ty| Interim::Type(Box::new(ty))),
            Some(ContextProperty::AcceptanceCriteria) => Interim::Criteria(entity),
            Some(ContextProperty::Test) => Interim::Tester(entity),
            Some(ContextProperty::Tests) => Interim::List(Vec::new()),
            // Everything the trait was applied to, complete because a statement that reads
            // it waits until no statement left to run can apply the trait.
            // @lfy def/interpret/main.lfy:expand
            // @lfy def/interpret/main.lfy:expand
            Some(ContextProperty::Entities) => Interim::List(
                self.model.entities[entity]
                    .entities()
                    .iter()
                    .map(|&e| Interim::Entity(e))
                    .collect(),
            ),
            Some(ContextProperty::Extenders) => Interim::List(
                self.model.entities[entity]
                    .extenders()
                    .iter()
                    .map(|&e| Interim::Entity(e))
                    .collect(),
            ),
            Some(ContextProperty::Parameters) => Interim::List(
                self.model.entities[entity]
                    .parameters()
                    .iter()
                    .map(|&symbol| Interim::Entity(self.model.symbols[symbol].entity))
                    .collect(),
            ),
            Some(ContextProperty::Output) => self.model.entities[entity]
                .output()
                .cloned()
                .map_or(Interim::Undefined, |ty| Interim::Type(Box::new(ty))),
            Some(ContextProperty::ItemType) => self.model.entities[entity]
                .item_type
                .clone()
                .map_or(Interim::Undefined, |ty| Interim::Type(Box::new(ty))),
            Some(ContextProperty::References) => Interim::List(Vec::new()),
            Some(ContextProperty::Like) => Interim::Like(Box::new(value.clone())),
            None => Interim::Undefined,
        }
    }

    /// The value of a member of an entity: what a setter gave it, or its declared value.
    // @lfy def/interpret/main.lfy:evaluate
    fn member_value(&mut self, entity: EntityId, name: &str, env: &mut Env) -> Interim {
        if let Some(value) = self.model.entities[entity].value(name) {
            return value.clone();
        }
        let symbol = member_symbol(self.model, entity, name).or_else(|| {
            self.model.entities[entity]
                .traits
                .iter()
                .map(|applied| applied.entity)
                .collect::<Vec<_>>()
                .into_iter()
                .find_map(|applied| member_symbol(self.model, applied, name))
        });
        let Some(symbol) = symbol else {
            return self.only_at_runtime();
        };
        let held = self.model.symbols[symbol].entity;
        if self.model.symbols[symbol].kind != SymbolKind::Member {
            return self.value_of_symbol(symbol);
        }
        let node = self.model.symbols[symbol].node;
        if self.model.is(node, E::TypeKey) {
            return Interim::Type(Box::new(
                self.model.entities[held]
                    .ty
                    .clone()
                    .unwrap_or(TypeRef::Unknown(String::new())),
            ));
        }
        let expression = self.model.child_nodes(node).into_iter().next();
        if let Some(expression) = expression
            && self.model.is(expression, E::Assignment)
            && let Some(right) = self.model.child_nodes(expression).get(1).copied()
        {
            let mut inner = env.clone();
            inner.scope = self.model.symbols[symbol].scope;
            return self.eval(right, &mut inner);
        }
        Interim::Undefined
    }

    /// The value a symbol stands for when its name is read.
    // @lfy def/interpret/main.lfy:evaluate
    fn value_of_symbol(&mut self, symbol: SymbolId) -> Interim {
        let entity = self.model.symbols[symbol].entity;
        match self.model.symbols[symbol].kind {
            SymbolKind::Variable | SymbolKind::Alias | SymbolKind::External => {
                self.value_of_variable(entity)
            }
            SymbolKind::EnumMember => {
                if let Some(value) = self.cache.get(&entity) {
                    return value.clone();
                }
                let node = self.model.symbols[symbol].node;
                let value_node = self
                    .model
                    .child_nodes(node)
                    .into_iter()
                    .find(|&c| !self.model.is(c, E::Declared));
                let scope = self.model.symbols[symbol].scope;
                let mut env = Env {
                    vars: Vec::new(),
                    current: entity,
                    target: entity,
                    contributor: entity,
                    scope,
                    in_trait: false,
                    dry: true,
                    ace: false,
                    ret: None,
                };
                let value = match value_node {
                    Some(value_node) => self.eval(value_node, &mut env),
                    None => Interim::Undefined,
                };
                self.cache.insert(entity, value.clone());
                value
            }
            SymbolKind::Module => Interim::Entity(entity),
            // A parameter, a loop variable, and a member only have a value at runtime.
            // @lfy def/interpret/main.lfy:evaluate
            SymbolKind::Parameter | SymbolKind::LoopVariable | SymbolKind::Member => {
                self.only_at_runtime()
            }
            _ => Interim::Entity(entity),
        }
    }

    /// The value a declared name holds: taken once, and never taken twice while it is
    /// being taken.
    // @lfy def/interpret/main.lfy:evaluate
    fn value_of_variable(&mut self, entity: EntityId) -> Interim {
        if let Some(value) = self.cache.get(&entity) {
            return value.clone();
        }
        // Evaluating a value that needs its own, directly or through other nodes, is a
        // cycle: nothing comes back and the cycle is named once.
        // @lfy def/interpret/main.lfy:evaluate
        if self.taking.contains(&entity) {
            self.report_cycle(entity);
            return self.only_at_runtime();
        }
        // A name with no declaring node is the entity itself, which is how `global` reads.
        // @lfy def/interpret/main.lfy:expand
        let Some(node) = self.model.entities[entity].node else {
            return Interim::Entity(entity);
        };
        // A `let` only has a value at runtime; a `const` is the same value every time.
        // @lfy def/interpret/main.lfy:evaluate
        if self.model.has_token(node, K::LetKeyword) {
            return self.only_at_runtime();
        }
        let value_node = self
            .model
            .child_nodes(node)
            .into_iter()
            .find(|&c| !self.model.is(c, E::Declared));
        let Some(value_node) = value_node else {
            return self.only_at_runtime();
        };
        let scope = self.model.enclosing_scope(node);
        let current = self.model.scopes[scope].current;
        self.taking.push(entity);
        let mut env = Env {
            vars: Vec::new(),
            current,
            target: current,
            contributor: current,
            scope,
            in_trait: false,
            dry: true,
            ace: false,
            ret: None,
        };
        let before = self.runtime;
        self.runtime = false;
        let value = self.eval(value_node, &mut env);
        let stable = !self.runtime;
        self.runtime = before || self.runtime;
        self.taking.pop();
        // Only a value that did not depend on runtime code is remembered, so that the same
        // node read twice gives the same value.
        if stable {
            self.cache.insert(entity, value.clone());
        }
        value
    }

    /// Names a cycle at the declaration whose value needs its own.
    // @lfy def/interpret/main.lfy:evaluate
    fn report_cycle(&mut self, entity: EntityId) {
        if !self.reported.insert(entity) {
            return;
        }
        let Some(node) = self.model.entities[entity].node else {
            return;
        };
        let names: Vec<String> = self
            .taking
            .iter()
            .skip_while(|&&held| held != entity)
            .map(|&held| {
                self.model.entities[held]
                    .identifier
                    .clone()
                    .unwrap_or_else(|| "anonymous".to_string())
            })
            .collect();
        let own = names.first().cloned().unwrap_or_default();
        self.problem(
            node,
            format!("{} needs its own value: {own}", names.join(" needs ")),
        );
    }

    /// Calls: `add` on criteria, `test` on an entity's context, `apply` on a trait, the
    /// members of a builtin data written out in full, and declared or inline functions.
    // @lfy def/interpret/main.lfy:expand
    fn eval_call(&mut self, r: NodeRef, env: &mut Env) -> Interim {
        let Some(callee) = self.model.left(r) else {
            return Interim::Undefined;
        };
        let arguments = self.model.arguments(r);
        if self.model.is(callee, E::Member)
            && let (Some(left), Some((accessor, Some(method)))) =
                (self.model.left(callee), self.model.accessor_and_name(callee))
            && (self.model.token_is(callee.file, accessor, P::ValueAccessor)
                || self
                    .model
                    .token_is(callee.file, accessor, P::OptionalValueAccessor))
        {
            return self.eval_method(r, left, &method, &arguments, env);
        }
        let function = self.eval(callee, env);
        match function {
            Interim::Tester(target) => {
                // A `test` call appends each argument as one test.
                // @lfy def/interpret/main.lfy:expand
                let target = if env.in_trait && target == env.current {
                    env.target
                } else {
                    target
                };
                for argument in arguments {
                    let test = self.test_of(argument);
                    self.model.entities[target].tests.push(test);
                }
                Interim::Undefined
            }
            // `X@like(prompt)`: a value of the entity's type that the prompt describes. Its
            // text is the prompt; `evaluate` gives a [`Prompted`] for it.
            // @lfy def/interpret/main.lfy:evaluate
            Interim::Like(_) => match arguments.first() {
                Some(&argument) => self.eval(argument, env),
                None => Interim::Undefined,
            },
            Interim::Criteria(_) => Interim::Undefined,
            other => {
                let values = self.eval_arguments(&arguments, env);
                self.call_value(r, &other, values, env)
            }
        }
    }

    /// A call of a method on a value: what the receiver is decides what it does.
    // @lfy def/interpret/main.lfy:expand
    fn eval_method(
        &mut self,
        call: NodeRef,
        left: NodeRef,
        method: &str,
        arguments: &[NodeRef],
        env: &mut Env,
    ) -> Interim {
        // An `add` call on a member of the entity's context that holds items appends to it;
        // `acceptanceCriteria` reads as a value of its own and is handled below.
        // @lfy def/interpret/main.lfy:expand
        if method == "add"
            && let Some((added, read)) = added_member(self.model, left)
            && added != Added::Criteria
        {
            // An earlier `add` of the same chain appends its own item first.
            // @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
            if self.model.is(left, E::Call) {
                self.eval(left, env);
            }
            return self.add_item(call, added, read, arguments, env);
        }
        let receiver = self.eval(left, env);
        // An operation of a data carrying builtin is performed by the interpreter itself.
        // @lfy def/interpret/main.lfy:expand
        let native = self.performs_operation(&receiver, method);
        match (&receiver, method) {
            // An `add` call on the entity's context appends one criterion.
            // @lfy def/interpret/main.lfy:expand
            (Interim::Criteria(target), "add") => {
                let target = *target;
                let target = if env.in_trait && target == env.current {
                    env.target
                } else {
                    target
                };
                // A trait's own body is run for the trait too, so the trait keeps the
                // criteria it contributes as its own.
                for &argument in arguments {
                    let criterion = self.criterion_of(argument, env);
                    self.model.entities[target]
                        .acceptance_criteria
                        .push(criterion);
                }
                receiver
            }
            // The trait is applied to the entity the first argument resolves to, with the
            // other arguments as the application's arguments.
            // @lfy def/interpret/main.lfy:expand
            (Interim::Entity(trait_id), "apply")
                if self.model.entities[*trait_id].is_trait() =>
            {
                let trait_id = *trait_id;
                if env.dry {
                    return Interim::Undefined;
                }
                let Some((&first, rest)) = arguments.split_first() else {
                    return Interim::Undefined;
                };
                let receivers = self.apply_receivers(first, env);
                let values = self.eval_arguments(rest, env);
                for receiver in receivers {
                    self.apply_trait(
                        receiver,
                        trait_id,
                        rest.to_vec(),
                        values.clone(),
                        AppliedSource::Apply(call),
                        false,
                    );
                }
                Interim::Undefined
            }
            // `t.layer(...)`: the trait chosen as a layer of a target, with its arguments.
            // @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
            (Interim::Entity(subject), "layer")
                if self.model.entities[*subject].is_trait() =>
            {
                let subject = *subject;
                let values = self.eval_arguments(arguments, env);
                self.layer_value(call, subject, values)
            }
            (Interim::List(items), "map") if native => {
                let Some(&first) = arguments.first() else {
                    return Interim::Undefined;
                };
                let function = self.eval(first, env);
                let items = items.clone();
                let mapped = items
                    .into_iter()
                    .map(|item| self.call_value(call, &function, vec![item], env))
                    .collect();
                Interim::List(mapped)
            }
            (Interim::List(items), "join") if native => {
                let separator = match arguments.first() {
                    Some(&argument) => self.eval(argument, env),
                    None => Interim::String(",".to_string()),
                };
                let separator = self.to_text(&separator);
                let texts: Vec<String> = items.iter().map(|item| self.to_text(item)).collect();
                Interim::String(texts.join(&separator))
            }
            (Interim::List(items), "push") if native => {
                let mut items = items.clone();
                for &argument in arguments {
                    let value = self.eval(argument, env);
                    items.push(value);
                }
                let length = items.len();
                if let Some(name) = self.model.name(left) {
                    env.set(&name, Interim::List(items));
                }
                Interim::Number(length as f64)
            }
            (Interim::List(items), "includes") if native => {
                let needle = match arguments.first() {
                    Some(&argument) => self.eval(argument, env),
                    None => Interim::Undefined,
                };
                Interim::Bool(items.contains(&needle))
            }
            (Interim::String(text), "includes") if native => {
                let text = text.clone();
                let needle = match arguments.first() {
                    Some(&argument) => self.eval(argument, env),
                    None => Interim::Undefined,
                };
                Interim::Bool(text.contains(&self.to_text(&needle)))
            }
            (Interim::String(text), "trim") if native => {
                Interim::String(text.trim().to_string())
            }
            (Interim::String(text), "split") if native => {
                let text = text.clone();
                let separator = match arguments.first() {
                    Some(&argument) => self.eval(argument, env),
                    None => Interim::String(",".to_string()),
                };
                let separator = self.to_text(&separator);
                Interim::List(
                    text.split(separator.as_str())
                        .map(|part| Interim::String(part.to_string()))
                        .collect(),
                )
            }
            (Interim::Entity(_), _) | (Interim::Object(_), _) => {
                let function = self.access(
                    receiver.clone(),
                    P::ValueAccessor.entity(),
                    Some(method),
                    env,
                );
                let values = self.eval_arguments(arguments, env);
                self.call_value(call, &function, values, env)
            }
            _ => self.only_at_runtime(),
        }
    }

    /// The entities an `apply` call targets: the first argument's entity, every entity of a
    /// list, or every item of an [`ALTERNATION_LIST`].
    // @lfy def/interpret/main.lfy:expand
    fn apply_receivers(&mut self, first: NodeRef, env: &mut Env) -> Vec<EntityId> {
        let value = self.eval(first, env);
        if let Interim::List(items) = &value {
            let entities: Vec<EntityId> = items
                .iter()
                .filter_map(|item| match item {
                    Interim::Entity(entity) => Some(*entity),
                    _ => None,
                })
                .collect();
            if !entities.is_empty() {
                return entities;
            }
        }
        let entity = match value {
            Interim::Entity(entity) => Some(entity),
            _ => resolve_name_node(self.model, first).map(|s| self.model.symbols[s].entity),
        };
        let Some(entity) = entity else {
            if !self.model.is(first, E::Current) {
                self.problem(first, "apply needs an entity as its first argument");
            }
            return Vec::new();
        };
        // An alternation list: every item of it.
        // @lfy def/interpret/main.lfy:expand
        if let Some(alternation) = self.model.trait_named(ALTERNATION_LIST)
            && let Some(applied) = self.model.entities[entity]
                .traits
                .iter()
                .find(|applied| applied.entity == alternation)
                .cloned()
        {
            let items: Vec<EntityId> = applied
                .values
                .iter()
                .filter_map(|value| match value {
                    Interim::Entity(entity) => Some(*entity),
                    _ => None,
                })
                .collect();
            if !items.is_empty() {
                return items;
            }
        }
        vec![entity]
    }

    /// Calls a function value with values.
    // @lfy def/interpret/main.lfy:expand
    fn call_value(
        &mut self,
        call: NodeRef,
        function: &Interim,
        values: Vec<Interim>,
        env: &mut Env,
    ) -> Interim {
        match function {
            Interim::Closure(node, captured) => {
                let node = *node;
                let mut inner = env.clone();
                inner.vars = captured.clone();
                inner.ret = None;
                self.bind_parameters(node, &values, &mut inner);
                let Some(body) = self.model.child_nodes(node).into_iter().last() else {
                    return Interim::Undefined;
                };
                if self.model.is(body, S::Block) {
                    self.exec(body, &mut inner);
                    inner.ret.unwrap_or(Interim::Undefined)
                } else {
                    self.eval(body, &mut inner)
                }
            }
            Interim::Entity(entity) => self.call_declared(call, *entity, values, env),
            Interim::Function(node, scope) => {
                let (node, scope) = (*node, *scope);
                let mut inner = env.clone();
                inner.scope = scope;
                inner.vars = Vec::new();
                inner.ret = None;
                self.bind_parameters(node, &values, &mut inner);
                match self.model.child(node, S::Block) {
                    Some(body) => {
                        self.exec_code(body, &mut inner);
                        inner.ret.unwrap_or(Interim::Undefined)
                    }
                    None => Interim::Undefined,
                }
            }
            _ => self.only_at_runtime(),
        }
    }

    /// Whether a declared function runs wherever its call is written, rather than only
    /// where the call is compile-time code: an `ace function`, and a function of a package,
    /// which is none of the program's runtime code.
    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:d998a92e697e3c6a2443e98c1570395e13261c027f5474e299e7ead940568311
    fn runs_wherever_called(&self, node: NodeRef) -> bool {
        self.model.ancestor(node, S::Ace).is_some()
            || self.model.sources.get(node.file).map(|source| source.origin)
                != Some(Origin::Program)
    }

    /// Calls a declared fn or function. A fn carrying [`BUILTIN`] is performed natively and
    /// never run; a fn carrying nothing has criteria for a body and nothing to run. What
    /// decides whether a call runs is where the call is written, never where the function
    /// it calls is declared.
    // @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
    fn call_declared(
        &mut self,
        call: NodeRef,
        entity: EntityId,
        values: Vec<Interim>,
        env: &mut Env,
    ) -> Interim {
        let EntityKind::Fn { agent, .. } = self.model.entities[entity].kind else {
            return self.only_at_runtime();
        };
        let Some(node) = self.model.entities[entity].node else {
            return self.only_at_runtime();
        };
        let builtin = self
            .model
            .trait_named(BUILTIN)
            .is_some_and(|marker| self.model.entities[entity].has_trait(marker));
        if agent {
            // A fn carrying builtin is performed natively, so compile-time code may call
            // one; a call of a fn carrying none has nothing to run.
            // @lfy def/interpret/main.lfy:expand
            if builtin {
                return self.perform_builtin(call, entity, &values, env);
            }
            let name = self.model.entities[entity]
                .identifier
                .clone()
                .unwrap_or_else(|| "a fn".to_string());
            self.problem(
                call,
                format!("{name} is a fn whose body is criteria, so the call has nothing to run"),
            );
            return self.only_at_runtime();
        }
        // An `ace function`, and a function declared outside the program's runtime code,
        // run wherever the call is written; every other written function runs only where
        // the call itself is compile-time code, so runtime code keeps the call.
        // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:d998a92e697e3c6a2443e98c1570395e13261c027f5474e299e7ead940568311
        if !self.runs_functions
            && !self.runs_wherever_called(node)
            && phase_of(self.model, call) != Phase::Compile
        {
            return self.only_at_runtime();
        }
        let scope = self.model.entities[entity].scope.unwrap_or(self.universe);
        let mut inner = Env {
            vars: Vec::new(),
            current: entity,
            target: entity,
            contributor: entity,
            scope,
            in_trait: false,
            dry: true,
            ace: false,
            ret: None,
        };
        self.bind_parameters(node, &values, &mut inner);
        let Some(body) = self.model.child(node, S::Block) else {
            return self.only_at_runtime();
        };
        self.exec_code(body, &mut inner);
        inner.ret.unwrap_or(Interim::Undefined)
    }

    // ---- The builtins the interpreter performs --------------------------------------

    /// Performs a builtin fn itself, as the target's own standard library performs it:
    /// compile-time code may call one, and nothing of its body is ever run. Runtime code
    /// keeps the call, so folding a runtime expression never reads a file, runs a command,
    /// or ends the process.
    // @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
    fn perform_builtin(
        &mut self,
        call: NodeRef,
        entity: EntityId,
        values: &[Interim],
        env: &mut Env,
    ) -> Interim {
        if !self.runs_functions && phase_of(self.model, call) != Phase::Compile {
            return self.only_at_runtime();
        }
        let Some((module, name)) = self.native_name(entity) else {
            return self.only_at_runtime();
        };
        let performed = match module.as_str() {
            "file" => perform_file(&name, values),
            "json" => perform_json(&name, values),
            "time" => perform_time(&name, values),
            "process" => self.perform_process(call, &name, values, env),
            "trait" if name == "apply" => self.perform_apply(call, values, env),
            "trait" if name == "layer" => self.perform_layer(call, values),
            _ => None,
        };
        // A builtin the interpreter has no native for reads as what only runtime answers.
        // @lfy def/interpret/main.lfy:expand
        performed.unwrap_or_else(|| self.only_at_runtime())
    }

    /// The library module and name of a builtin fn: `("file", "read")` for the `read` of
    /// `elfie/system/file`. A fn of the program is none of them, however it is named.
    // @lfy def/interpret/main.lfy:expand
    fn native_name(&self, entity: EntityId) -> Option<(String, String)> {
        let node = self.model.entities[entity].node?;
        let source = &self.model.sources[node.file];
        if source.origin == Origin::Program {
            return None;
        }
        let module = source
            .path
            .rsplit(['/', '\\'])
            .next()?
            .strip_suffix(".lfy")?
            .to_string();
        Some((module, self.model.entities[entity].identifier.clone()?))
    }

    /// The builtins of `elfie/system/process`: a command runs through the platform shell.
    // @lfy def/interpret/main.lfy:expand
    fn perform_process(
        &mut self,
        call: NodeRef,
        name: &str,
        values: &[Interim],
        env: &mut Env,
    ) -> Option<Interim> {
        match name {
            "run" | "stream" => {
                let exited = run_command(
                    &text_argument(values, 0)?,
                    text_argument(values, 1),
                    text_argument(values, 2),
                    values.get(3),
                );
                // Each line of the output reaches `onLine` before the call returns.
                // @lfy def/interpret/main.lfy:expand
                if name == "stream"
                    && let Some(on_line) = values.get(4)
                    && !matches!(on_line, Interim::Undefined | Interim::Null)
                {
                    let on_line = on_line.clone();
                    for (text, is_error) in
                        [(exited.stdout.clone(), false), (exited.stderr.clone(), true)]
                    {
                        for line in text.lines() {
                            let arguments =
                                vec![Interim::String(line.to_string()), Interim::Bool(is_error)];
                            self.call_value(call, &on_line, arguments, env);
                        }
                    }
                }
                Some(Interim::Object(vec![
                    ("code".to_string(), Interim::Number(f64::from(exited.code))),
                    ("stdout".to_string(), Interim::String(exited.stdout)),
                    ("stderr".to_string(), Interim::String(exited.stderr)),
                ]))
            }
            "environment" => Some(
                std::env::var(text_argument(values, 0)?).map_or(Interim::Undefined, Interim::String),
            ),
            "currentDirectory" => Some(
                std::env::current_dir()
                    .map(|directory| directory.to_string_lossy().replace('\\', "/"))
                    .map_or(Interim::Undefined, Interim::String),
            ),
            // The process ends at once, its output flushed, and nothing after the call runs.
            // @lfy def/interpret/main.lfy:expand
            "exit" => {
                let code = match values.first() {
                    Some(Interim::Number(code)) => *code as i32,
                    _ => 0,
                };
                let _ = std::io::Write::flush(&mut std::io::stdout());
                let _ = std::io::Write::flush(&mut std::io::stderr());
                std::process::exit(code)
            }
            "isTerminal" => {
                use std::io::IsTerminal;
                Some(Interim::Bool(match values.first() {
                    Some(Interim::Bool(true)) => std::io::stderr().is_terminal(),
                    _ => std::io::stdout().is_terminal(),
                }))
            }
            _ => None,
        }
    }

    /// `apply` called as a fn rather than as a member of a trait: the trait is applied to
    /// what the second argument names, with the rest as [`Applied::arguments`].
    // @lfy def/interpret/main.lfy:expand
    fn perform_apply(
        &mut self,
        call: NodeRef,
        values: &[Interim],
        env: &mut Env,
    ) -> Option<Interim> {
        let Some(&Interim::Entity(trait_id)) = values.first() else {
            return None;
        };
        if !self.model.entities[trait_id].is_trait() {
            return None;
        }
        if env.dry {
            return Some(Interim::Entity(trait_id));
        }
        let nodes = self.model.arguments(call);
        let target = *nodes.get(1)?;
        let rest = nodes.get(2..).unwrap_or_default().to_vec();
        let receivers = self.apply_receivers(target, env);
        let arguments = self.eval_arguments(&rest, env);
        for receiver in receivers {
            self.apply_trait(
                receiver,
                trait_id,
                rest.clone(),
                arguments.clone(),
                AppliedSource::Apply(call),
                false,
            );
        }
        Some(Interim::Entity(trait_id))
    }

    /// `layer` called as a fn: it yields the object holding the trait as its subject and
    /// the other arguments, evaluated in order, as its arguments. Nothing is applied by the
    /// call itself.
    // @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
    fn perform_layer(&mut self, call: NodeRef, values: &[Interim]) -> Option<Interim> {
        let Some(&Interim::Entity(subject)) = values.first() else {
            return None;
        };
        if !self.model.entities[subject].is_trait() {
            return None;
        }
        Some(self.layer_value(call, subject, values[1..].to_vec()))
    }

    // ---- Knowledge, commands, and targets -------------------------------------------

    /// Appends one item per argument to a member of an entity's context that holds items,
    /// and yields the member's items, so calls chain.
    // @lfy def/interpret/main.lfy:expand
    fn add_item(
        &mut self,
        call: NodeRef,
        added: Added,
        read: NodeRef,
        arguments: &[NodeRef],
        env: &mut Env,
    ) -> Interim {
        let Some(owner) = self.added_owner(read, env) else {
            return Interim::Undefined;
        };
        for &argument in arguments {
            match added {
                // One piece of knowledge with the topic, kind, source, and quote of the
                // object argument, its contributor the entity whose body holds the call.
                // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
                Added::Knowledge => {
                    let item = self.knowledge_of(argument, env);
                    self.model.entities[owner].knowledge.push(item);
                }
                // One command with the operation and line of the object argument.
                // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
                Added::Commands => {
                    let item = self.command_of(argument, env);
                    self.model.entities[owner].commands.push(item);
                }
                Added::Targets => self.add_target(call, owner, argument),
                Added::Criteria => {}
            }
        }
        self.added_items(owner, added)
    }

    /// The entity whose member an `add` call appends to: the current entity for a bare
    /// accessor, and what the left side names otherwise. A body run for a receiver appends
    /// to the receiver, as a criterion of a trait's body attaches to it, so what a trait
    /// adds to its current entity reaches each entity the trait is applied to.
    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    fn added_owner(&mut self, read: NodeRef, env: &mut Env) -> Option<EntityId> {
        let owner = match self.model.left(read) {
            Some(left) => match self.eval(left, env) {
                Interim::Entity(entity) => entity,
                _ => resolve_name_node(self.model, left)
                    .map(|symbol| self.model.symbols[symbol].entity)?,
            },
            None => env.current,
        };
        Some(if env.in_trait && owner == env.current {
            env.target
        } else {
            owner
        })
    }

    /// What a member of an entity's context that holds items reads as.
    // @lfy def/interpret/main.lfy:expand
    fn added_items(&mut self, owner: EntityId, added: Added) -> Interim {
        match added {
            Added::Criteria => Interim::Criteria(owner),
            Added::Knowledge => Interim::List(
                self.model.entities[owner]
                    .knowledge
                    .iter()
                    .map(knowledge_value)
                    .collect(),
            ),
            Added::Commands => Interim::List(
                self.model.entities[owner]
                    .commands
                    .iter()
                    .map(command_value)
                    .collect(),
            ),
            Added::Targets => Interim::List(
                self.model.entities[owner]
                    .targets
                    .iter()
                    .map(|&target| Interim::Entity(target))
                    .collect(),
            ),
        }
    }

    /// The fields of the object given to `add`: each key, the node of its value when it is
    /// written out, and the value.
    // @lfy def/interpret/main.lfy:expand
    fn object_fields(
        &mut self,
        argument: NodeRef,
        env: &mut Env,
    ) -> Vec<(String, Option<NodeRef>, Interim)> {
        if self.model.is(argument, E::Object) {
            let mut out = Vec::new();
            for key in self.model.children_of(argument, E::ObjectKey) {
                let Some(name) = self.model.declared_name(key) else {
                    continue;
                };
                let node = self
                    .model
                    .child_nodes(key)
                    .into_iter()
                    .find(|&child| !self.model.is(child, E::Declared));
                let value = match node {
                    Some(node) => self.eval(node, env),
                    None => Interim::Undefined,
                };
                out.push((name, node, value));
            }
            return out;
        }
        match self.eval(argument, env) {
            Interim::Object(pairs) => pairs
                .into_iter()
                .map(|(key, value)| (key, None, value))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// One piece of knowledge from the object given to `add` on an entity's knowledge.
    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    fn knowledge_of(&mut self, argument: NodeRef, env: &mut Env) -> Knowledge {
        let mut item = Knowledge {
            topic: String::new(),
            kind: KnowledgeKind::Reference,
            source: String::new(),
            quote: None,
            contributor: env.contributor,
        };
        for (key, node, value) in self.object_fields(argument, env) {
            match key.as_str() {
                "topic" => item.topic = self.to_text(&value),
                "kind" => {
                    if let Some(kind) = self.knowledge_kind(node, &value) {
                        item.kind = kind;
                    }
                }
                "source" => item.source = self.to_text(&value),
                "quote" => item.quote = Some(value.is_truthy()),
                _ => {}
            }
        }
        item
    }

    /// One command from the object given to `add` on an entity's commands. The object holds
    /// an operation, as the criteria of `Command` say, so `build` only stands in for one
    /// written without.
    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    fn command_of(&mut self, argument: NodeRef, env: &mut Env) -> Command {
        let mut item = Command {
            operation: Operation::Build,
            line: None,
            contributor: env.contributor,
        };
        for (key, node, value) in self.object_fields(argument, env) {
            match key.as_str() {
                "operation" => {
                    if let Some(operation) = self.operation_named(node, &value) {
                        item.operation = operation;
                    }
                }
                "line" => item.line = Some(self.to_text(&value)),
                _ => {}
            }
        }
        item
    }

    /// The kind a field names: the member of `KnowledgeKind` it reads by name, or the one
    /// whose value it holds.
    // @lfy def/interpret/main.lfy:expand
    fn knowledge_kind(&self, node: Option<NodeRef>, value: &Interim) -> Option<KnowledgeKind> {
        let named = self.member_named(node);
        [
            ("reference", KnowledgeKind::Reference),
            ("example", KnowledgeKind::Example),
            ("definition", KnowledgeKind::Definition),
            ("tool", KnowledgeKind::Tool),
        ]
        .into_iter()
        .find(|&(name, kind)| {
            named.as_deref() == Some(name) || value.as_str() == Some(kind.value())
        })
        .map(|(_, kind)| kind)
    }

    /// The operation a field names, read the same way.
    // @lfy def/interpret/main.lfy:expand
    fn operation_named(&self, node: Option<NodeRef>, value: &Interim) -> Option<Operation> {
        let named = self.member_named(node);
        [
            ("install", Operation::Install),
            ("add", Operation::Add),
            ("build", Operation::Build),
            ("test", Operation::Test),
            ("lint", Operation::Lint),
            ("format", Operation::Format),
            ("run", Operation::Run),
        ]
        .into_iter()
        .find(|&(name, operation)| {
            named.as_deref() == Some(name) || value.as_str() == Some(operation.value())
        })
        .map(|(_, operation)| operation)
    }

    /// The name a field's value reads after an accessor, such as the member of an enum.
    // @lfy def/interpret/main.lfy:expand
    fn member_named(&self, node: Option<NodeRef>) -> Option<String> {
        node.and_then(|node| self.model.accessor_and_name(node))
            .and_then(|(_, name)| name)
    }

    /// `@targets.add(t)`: the entity of the `ace const` the argument names joins the
    /// entity's targets.
    // @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
    fn add_target(&mut self, call: NodeRef, owner: EntityId, argument: NodeRef) {
        // A member, a parameter, and an enum member are built with their owner.
        // @lfy def/interpret/main.lfy:expand#expand:expand:52171aea8ab1474de54983df55c484fadc588bd0d6e58e36b25e18a182ea17bc
        if matches!(
            self.model.entities[owner].kind,
            EntityKind::Member | EntityKind::Parameter | EntityKind::EnumMember
        ) {
            self.problem(
                call,
                "this is built with its owner, so no target is added to it",
            );
            return;
        }
        let Some(target) = self.target_const(argument) else {
            // The argument names no `ace const` of the project holding a `Target`.
            // @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
            let text = self.model.raw(argument).trim().to_string();
            self.problem(argument, format!("{text} names no target"));
            return;
        };
        // The same target twice.
        // @lfy def/interpret/main.lfy:expand#expand:expand:8159e82000937ae0a26519fa34e1579b5f6a3bcd6325987ebc1729fd11812494
        if self.model.entities[owner].targets.contains(&target) {
            let name = self.model.entities[target]
                .identifier
                .clone()
                .unwrap_or_default();
            self.problem(call, format!("the target {name} was added twice"));
            return;
        }
        self.model.entities[owner].targets.push(target);
    }

    /// The entity of the `ace const` an expression names, when that const declares a
    /// target.
    // @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
    fn target_const(&self, argument: NodeRef) -> Option<EntityId> {
        let symbol = resolve_name_node(self.model, argument)?;
        let entity = self.model.symbols[symbol].entity;
        self.declares_a_target(entity).then_some(entity)
    }

    // ---- Criteria and tests ---------------------------------------------------------

    /// One criterion from the object given to `add`.
    // @lfy def/interpret/main.lfy:expand
    fn criterion_of(&mut self, argument: NodeRef, env: &mut Env) -> Criterion {
        let object = self.eval(argument, env);
        let mut criterion = Criterion {
            contributor: env.contributor,
            node: Some(argument),
            ..Criterion::default()
        };
        if let Interim::Object(pairs) = object {
            for (key, value) in pairs {
                let texts = self.texts_of(&value);
                match key.as_str() {
                    "situation" => criterion.situation = Some(texts),
                    "behavior" => criterion.behavior = Some(texts),
                    "sideEffects" => criterion.side_effects = Some(texts),
                    _ => {}
                }
            }
        }
        criterion
    }

    // @lfy def/interpret/main.lfy:expand
    fn texts_of(&self, value: &Interim) -> Vec<String> {
        match value {
            Interim::List(items) => items.iter().map(|item| self.to_text(item)).collect(),
            other => vec![self.to_text(other)],
        }
    }

    /// One test from the object given to `test`.
    // @lfy def/interpret/main.lfy:expand
    fn test_of(&self, argument: NodeRef) -> Test {
        let mut test = Test {
            input: None,
            expect: None,
            input_text: String::new(),
            expect_text: String::new(),
        };
        if !self.model.is(argument, E::Object) {
            return test;
        }
        for key in self.model.children_of(argument, E::ObjectKey) {
            let name = self.model.declared_name(key).unwrap_or_default();
            let value = self
                .model
                .child_nodes(key)
                .into_iter()
                .find(|&c| !self.model.is(c, E::Declared));
            let text = value
                .map(|v| self.model.raw(v).trim().to_string())
                .unwrap_or_default();
            match name.as_str() {
                "input" => {
                    test.input = value;
                    test.input_text = text;
                }
                "expect" => {
                    test.expect = value;
                    test.expect_text = text;
                }
                _ => {}
            }
        }
        test
    }

    // ---- Text -----------------------------------------------------------------------

    /// The text of a string or template expression, evaluated; another expression gives its
    /// value as text, or its source when it has none.
    // @lfy def/interpret/main.lfy:expand
    fn text_of(&mut self, r: NodeRef, env: &Env) -> String {
        let mut env = env.clone();
        let before = self.runtime;
        let value = self.eval(r, &mut env);
        self.runtime = before;
        match value {
            Interim::Undefined => self.model.raw(r).trim().to_string(),
            other => self.to_text(&other),
        }
    }

    // @lfy def/interpret/main.lfy:expand
    fn to_text(&self, value: &Interim) -> String {
        match value {
            Interim::Undefined => "undefined".to_string(),
            Interim::Null => "null".to_string(),
            Interim::Bool(boolean) => boolean.to_string(),
            Interim::Number(number) => {
                if number.fract() == 0.0 {
                    format!("{}", *number as i64)
                } else {
                    number.to_string()
                }
            }
            Interim::String(text) => text.clone(),
            Interim::List(items) => items
                .iter()
                .map(|item| self.to_text(item))
                .collect::<Vec<_>>()
                .join(", "),
            Interim::Object(pairs) => pairs
                .iter()
                .map(|(key, value)| format!("{key} = {}", self.to_text(value)))
                .collect::<Vec<_>>()
                .join(", "),
            Interim::Entity(entity) => self.model.entities[*entity]
                .identifier
                .clone()
                .unwrap_or_else(|| "anonymous".to_string()),
            Interim::Type(ty) => self.type_text(ty),
            Interim::Scope(_) => "scope".to_string(),
            Interim::Closure(..) | Interim::Function(..) => "function".to_string(),
            Interim::Criteria(_) => ACCEPTANCE_CRITERIA.to_string(),
            Interim::Tester(_) => TEST.to_string(),
            Interim::Like(inner) => self.to_text(inner),
        }
    }

    // @lfy def/interpret/main.lfy:expand
    fn type_text(&self, ty: &TypeRef) -> String {
        match ty {
            TypeRef::Entity(entity) => self.model.entities[*entity]
                .identifier
                .clone()
                .unwrap_or_else(|| "anonymous".to_string()),
            TypeRef::Primitive(primitive) => (*primitive).to_string(),
            TypeRef::Literal(value) => self.to_text(value),
            TypeRef::List(item) => format!("{}[]", self.type_text(item)),
            TypeRef::Union(items) => items
                .iter()
                .map(|item| self.type_text(item))
                .collect::<Vec<_>>()
                .join(" | "),
            TypeRef::Predicate(applied) => format!(
                "is {}",
                self.model.entities[*applied]
                    .identifier
                    .clone()
                    .unwrap_or_default()
            ),
            TypeRef::Function => "function".to_string(),
            TypeRef::Unknown(text) => text.trim().to_string(),
        }
    }

    /// A template's text: bodies as written, references rendered, executions evaluated.
    // @lfy def/interpret/main.lfy:expand
    fn template_text(&mut self, r: NodeRef, env: &mut Env) -> String {
        let children: Vec<Child> = self.model.node(r).children.clone();
        let mut out = String::new();
        for child in &children {
            match child {
                Child::Token(index) => {
                    let token = self.model.token_at(r.file, *index);
                    if token.is(L::TemplateBody) {
                        let text = token.value.clone();
                        out.push_str(&text);
                    }
                }
                Child::Node(node) => {
                    let Some(child) = self.model.node_ref(r.file, node) else {
                        continue;
                    };
                    if self.model.is(child, E::TemplateReference) {
                        out.push_str("[[");
                        if let Some(reference) = self.model.child_nodes(child).into_iter().next() {
                            let text = self.reference_text(reference, env);
                            out.push_str(&text);
                        }
                        out.push_str("]]");
                    } else if self.model.is(child, E::TemplateExecution) {
                        match self.model.child_nodes(child).into_iter().next() {
                            Some(expression) => {
                                let value = self.eval(expression, env);
                                match value {
                                    Interim::Undefined => {
                                        let raw = self.model.raw(child);
                                        out.push_str(&raw);
                                    }
                                    other => {
                                        let text = self.to_text(&other);
                                        out.push_str(&text);
                                    }
                                }
                            }
                            None => {
                                let raw = self.model.raw(child);
                                out.push_str(&raw);
                            }
                        }
                    }
                }
                Child::Error(_) => {}
            }
        }
        out
    }

    /// How a reference inside a template renders: a declared name keeps its text, and a
    /// variable or parameter is replaced by what it holds.
    // @lfy def/interpret/main.lfy:expand
    fn reference_text(&mut self, reference: NodeRef, env: &mut Env) -> String {
        let rule = self.model.rule_of(reference);
        if rule == E::Reference.entity() {
            return match self.model.child_nodes(reference).into_iter().next() {
                Some(inner) => self.reference_text(inner, env),
                None => String::new(),
            };
        }
        if rule == E::Name.entity() {
            let name = self.model.name(reference).unwrap_or_default();
            // Only a variable holding an entity is replaced; a string or a list keeps the
            // name as written, since it has no identifier.
            if let Some(Interim::Entity(entity)) = env.get(&name) {
                return self.model.entities[*entity]
                    .identifier
                    .clone()
                    .unwrap_or(name);
            }
            return name;
        }
        if rule == E::Dereference.entity() {
            return match self.model.child_nodes(reference).into_iter().next() {
                Some(operand) => self.reference_text(operand, env),
                None => String::new(),
            };
        }
        if rule == E::Current.entity() {
            let Some((accessor, name)) = self.model.accessor_and_name(reference) else {
                return self.model.raw(reference);
            };
            let head = self.model.entities[env.current]
                .identifier
                .clone()
                .unwrap_or_default();
            return match name {
                None => head,
                Some(name) => {
                    let raw = self.model.token_at(reference.file, accessor).raw.clone();
                    format!("{head}{raw}{name}")
                }
            };
        }
        if rule == E::Member.entity() {
            let Some(left) = self.model.left(reference) else {
                return self.model.raw(reference);
            };
            let Some((accessor, name)) = self.model.accessor_and_name(reference) else {
                return self.model.raw(reference);
            };
            let head = self.reference_text(left, env);
            let raw = self.model.token_at(reference.file, accessor).raw.clone();
            return match name {
                None => format!("{head}{raw}"),
                Some(name) => format!("{head}{raw}{name}"),
            };
        }
        self.model.raw(reference).trim().to_string()
    }

    // ---- What the interpret step hands back -----------------------------------------

    /// A value as the definition names one: the kinds a body needs only while it runs are
    /// not among them, so a type, a set of criteria, and a tester fold into nothing.
    // @lfy def/interpret/main.lfy:evaluate
    fn value_of(&self, value: &Interim) -> Option<Value> {
        Some(match value {
            Interim::Undefined => Value::Undefined,
            Interim::Null => Value::Null,
            Interim::Bool(boolean) => Value::Boolean(*boolean),
            Interim::Number(number) => Value::Number(*number),
            Interim::String(text) => Value::String(text.clone()),
            Interim::List(items) => Value::List(
                items
                    .iter()
                    .map(|item| self.value_of(item))
                    .collect::<Option<Vec<Value>>>()?,
            ),
            Interim::Object(pairs) => Value::Object(
                pairs
                    .iter()
                    .map(|(key, value)| self.value_of(value).map(|value| (key.clone(), value)))
                    .collect::<Option<BTreeMap<String, Value>>>()?,
            ),
            Interim::Entity(entity) => Value::Entity(*entity),
            Interim::Scope(scope) => Value::Scope(*scope),
            Interim::Function(node, scope) => Value::Function(*node, *scope),
            Interim::Closure(node, _) => {
                Value::Function(*node, self.model.enclosing_scope(*node))
            }
            // A type names a declaration or it names none; only the first is a value.
            Interim::Type(ty) => match &**ty {
                TypeRef::Entity(entity) => Value::Entity(*entity),
                _ => return None,
            },
            Interim::Criteria(_) | Interim::Tester(_) | Interim::Like(_) => return None,
        })
    }

    /// The [`Prompted`] a call of `like` on an entity's context stands for, when the node is
    /// one.
    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:1b8998ed21283dddfecd045cb1ee1d2c46c6a9ab9d55452d0805a7b6da098491
    fn prompted_of(&mut self, r: NodeRef, env: &mut Env) -> Option<Prompted> {
        if !self.model.is(r, E::Call) {
            return None;
        }
        let callee = self.model.left(r)?;
        let (accessor, name) = self.model.accessor_and_name(callee)?;
        if !self.model.token_is(callee.file, accessor, P::ContextAccessor) || name.as_deref()? != LIKE
        {
            return None;
        }
        let receiver = match self.model.left(callee) {
            Some(left) => self.eval(left, env),
            None => Interim::Entity(env.current),
        };
        let value_type = match &receiver {
            Interim::Entity(entity) => Some(*entity),
            Interim::Type(ty) => self.base_data(ty),
            Interim::String(_) => self.prelude_data("String"),
            Interim::Number(_) => self.prelude_data("Number"),
            Interim::Bool(_) => self.prelude_data("Boolean"),
            Interim::List(_) => self.prelude_data("List"),
            _ => None,
        }?;
        let argument = self.model.arguments(r).first().copied()?;
        let prompt = self.text_of(argument, env);
        Some(Prompted { value_type, prompt })
    }

    /// The value of an expression at compile time, in the scope it is written in.
    // @lfy def/interpret/main.lfy:evaluate
    fn evaluate_node(&mut self, node: NodeRef) -> Option<Evaluated> {
        let scope = self.model.enclosing_scope(node);
        let current = self.model.scopes[scope].current;
        let mut env = Env {
            vars: Vec::new(),
            current,
            target: current,
            contributor: current,
            scope,
            in_trait: false,
            dry: true,
            ace: false,
            ret: None,
        };
        // A call of like on an entity's context is a value only the compiler can choose.
        // @lfy def/interpret/main.lfy:evaluate
        self.runtime = false;
        if let Some(prompted) = self.prompted_of(node, &mut env)
            && !self.runtime
        {
            return Some(Evaluated::Prompted(prompted));
        }
        self.runtime = false;
        let value = self.eval(node, &mut env);
        // What only has a value at runtime is nothing here, and no problem of its own.
        // @lfy def/interpret/main.lfy:evaluate
        if self.runtime {
            return None;
        }
        self.value_of(&value).map(Evaluated::Value)
    }
}

// ---------------------------------------------------------------------------------------
// The builtins the interpreter performs
// ---------------------------------------------------------------------------------------

/// The text an argument holds, when it holds text.
// @lfy def/interpret/main.lfy:expand
fn text_argument(values: &[Interim], at: usize) -> Option<String> {
    match values.get(at) {
        Some(Interim::String(text)) => Some(text.clone()),
        _ => None,
    }
}

/// The builtins of `elfie/system/file`, performed as `std::fs` performs them.
// @lfy def/interpret/main.lfy:expand
fn perform_file(name: &str, values: &[Interim]) -> Option<Interim> {
    let path = text_argument(values, 0)?;
    let at = std::path::Path::new(&path);
    Some(match name {
        "read" => match std::fs::read_to_string(at) {
            Ok(text) => Interim::String(text),
            Err(_) => Interim::Undefined,
        },
        "write" | "append" => {
            let text = text_argument(values, 1).unwrap_or_default();
            Interim::Bool(write_file(at, &text, name == "append"))
        }
        "exists" => Interim::Bool(at.exists()),
        "list" => {
            let recursive = matches!(values.get(1), Some(Interim::Bool(true)));
            Interim::List(
                list_files(at, recursive)
                    .into_iter()
                    .map(Interim::String)
                    .collect(),
            )
        }
        "createDirectory" => {
            let _ = std::fs::create_dir_all(at);
            Interim::Bool(at.is_dir())
        }
        "stat" => match std::fs::metadata(at) {
            Ok(about) => {
                let size = if about.is_dir() { 0 } else { about.len() };
                Interim::Object(vec![
                    ("size".to_string(), Interim::Number(size as f64)),
                    (
                        "modified".to_string(),
                        Interim::Number(seconds_of(about.modified().ok())),
                    ),
                    ("isDirectory".to_string(), Interim::Bool(about.is_dir())),
                ])
            }
            Err(_) => Interim::Undefined,
        },
        // Only a file is ever removed, so a directory is left alone.
        // @lfy def/interpret/main.lfy:expand
        "remove" => Interim::Bool(at.is_file() && std::fs::remove_file(at).is_ok()),
        _ => return None,
    })
}

/// Writes text as the whole content of a file, or after what it holds, creating every
/// missing directory above it; whether it was written.
// @lfy def/interpret/main.lfy:expand
fn write_file(path: &std::path::Path, text: &str, append: bool) -> bool {
    if let Some(directory) = path.parent()
        && !directory.as_os_str().is_empty()
        && std::fs::create_dir_all(directory).is_err()
    {
        return false;
    }
    if !append {
        return std::fs::write(path, text).is_ok();
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()))
        .is_ok()
}

/// The files in a directory, as paths relative to it with `/` between the segments, in
/// ascending text order; directories are not listed.
// @lfy def/interpret/main.lfy:expand
fn list_files(directory: &std::path::Path, recursive: bool) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return found;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if !kind.is_dir() {
            found.push(name);
        } else if recursive {
            for under in list_files(&entry.path(), true) {
                found.push(format!("{name}/{under}"));
            }
        }
    }
    found.sort();
    found
}

/// A moment as seconds since the Unix epoch, to the millisecond.
// @lfy def/interpret/main.lfy:expand
fn seconds_of(time: Option<std::time::SystemTime>) -> f64 {
    time.and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |since| since.as_millis() as f64 / 1000.0)
}

/// The builtins of `elfie/system/time`: an instant is seconds since the Unix epoch, to the
/// millisecond, which is what `std::time::SystemTime` holds.
// @lfy def/interpret/main.lfy:expand
fn perform_time(name: &str, values: &[Interim]) -> Option<Interim> {
    match name {
        "now" => Some(Interim::Number(seconds_of(Some(
            std::time::SystemTime::now(),
        )))),
        "parse" => Some(
            text_argument(values, 0)
                .and_then(|text| instant_of(&text))
                .map_or(Interim::Undefined, Interim::Number),
        ),
        _ => None,
    }
}

/// The instant an RFC 3339 timestamp names, as seconds since the Unix epoch with its
/// fraction kept to the millisecond; nothing when the text is not one.
// @lfy def/interpret/main.lfy:expand
fn instant_of(text: &str) -> Option<f64> {
    let text = text.trim();
    let seconds = crate::generation::parse_rfc3339(text)? as f64;
    let fraction = text
        .get(19..)
        .and_then(|rest| rest.strip_prefix('.'))
        .map_or(0.0, |rest| {
            let mut digits: String = rest
                .chars()
                .take_while(char::is_ascii_digit)
                .take(3)
                .collect();
            while digits.len() < 3 {
                digits.push('0');
            }
            digits.parse::<f64>().unwrap_or(0.0) / 1000.0
        });
    Some(seconds + fraction)
}

/// The builtins of `elfie/system/json`: what JSON text carries, and the text carrying a
/// value, its keys in the order they were written.
// @lfy def/interpret/main.lfy:expand
fn perform_json(name: &str, values: &[Interim]) -> Option<Interim> {
    match name {
        "parse" => Some(
            text_argument(values, 0)
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .map_or(Interim::Undefined, |held| json_value(&held)),
        ),
        "stringify" => {
            let pretty = matches!(values.get(1), Some(Interim::Bool(true)));
            let text = json_text(values.first().unwrap_or(&Interim::Null), pretty, 0);
            Some(Interim::String(match pretty {
                true => format!("{text}\n"),
                false => text,
            }))
        }
        _ => None,
    }
}

/// What a JSON value holds.
// @lfy def/interpret/main.lfy:expand
fn json_value(value: &serde_json::Value) -> Interim {
    match value {
        serde_json::Value::Null => Interim::Null,
        serde_json::Value::Bool(held) => Interim::Bool(*held),
        serde_json::Value::Number(held) => Interim::Number(held.as_f64().unwrap_or_default()),
        serde_json::Value::String(held) => Interim::String(held.clone()),
        serde_json::Value::Array(items) => Interim::List(items.iter().map(json_value).collect()),
        serde_json::Value::Object(entries) => Interim::Object(
            entries
                .iter()
                .map(|(key, held)| (key.clone(), json_value(held)))
                .collect(),
        ),
    }
}

/// A value as JSON text: on one line with no spaces outside strings, or one entry or item
/// per line indented by two spaces per level, with an empty object or list on one line.
// @lfy def/interpret/main.lfy:expand
fn json_text(value: &Interim, pretty: bool, depth: usize) -> String {
    let (open, close, after) = match pretty {
        true => (
            format!("\n{}", "  ".repeat(depth + 1)),
            format!("\n{}", "  ".repeat(depth)),
            " ",
        ),
        false => (String::new(), String::new(), ""),
    };
    let between = format!(",{open}");
    match value {
        Interim::Bool(held) => held.to_string(),
        Interim::Number(held) => json_number(*held),
        Interim::String(text) => json_string(text),
        Interim::List(items) if !items.is_empty() => {
            let written: Vec<String> = items
                .iter()
                .map(|item| json_text(item, pretty, depth + 1))
                .collect();
            format!("[{open}{}{close}]", written.join(&between))
        }
        Interim::List(_) => "[]".to_string(),
        Interim::Object(pairs) if !pairs.is_empty() => {
            let written: Vec<String> = pairs
                .iter()
                .map(|(key, held)| {
                    let held = json_text(held, pretty, depth + 1);
                    format!("{}:{after}{held}", json_string(key))
                })
                .collect();
            format!("{{{open}{}{close}}}", written.join(&between))
        }
        Interim::Object(_) => "{}".to_string(),
        _ => "null".to_string(),
    }
}

/// A number as JSON spells it: a whole number without a fraction, and nothing JSON cannot
/// carry as null.
// @lfy def/interpret/main.lfy:expand
fn json_number(number: f64) -> String {
    if !number.is_finite() {
        return "null".to_string();
    }
    if number.fract() == 0.0 && number.abs() < 1e15 {
        return format!("{number:.0}");
    }
    number.to_string()
}

/// Text quoted with the escapes JSON requires and no others.
// @lfy def/interpret/main.lfy:expand
fn json_string(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\u{8}' => quoted.push_str("\\b"),
            '\u{c}' => quoted.push_str("\\f"),
            held if held < ' ' => quoted.push_str(&format!("\\u{:04x}", held as u32)),
            held => quoted.push(held),
        }
    }
    quoted.push('"');
    quoted
}

/// What a finished command left: its code, and everything it wrote.
// @lfy def/interpret/main.lfy:expand
struct Exited {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs a command through the platform shell and waits for it, capturing its output whole.
// @lfy def/interpret/main.lfy:expand
fn run_command(
    command: &str,
    input: Option<String>,
    directory: Option<String>,
    environment: Option<&Interim>,
) -> Exited {
    let (program, flag) = match cfg!(windows) {
        true => ("cmd", "/C"),
        false => ("sh", "-c"),
    };
    let mut started = std::process::Command::new(program);
    started.arg(flag).arg(command);
    if let Some(directory) = directory {
        started.current_dir(directory);
    }
    // Each entry is set on top of the inherited environment; nothing is removed from it.
    // @lfy def/interpret/main.lfy:expand
    if let Some(Interim::Object(pairs)) = environment {
        for (name, value) in pairs {
            started.env(name, plain_text(value));
        }
    }
    started
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // The shell could not be started, or the directory is not there.
    // @lfy def/interpret/main.lfy:expand
    let failed = |message: String| Exited {
        code: -1,
        stdout: String::new(),
        stderr: message,
    };
    let mut child = match started.spawn() {
        Ok(child) => child,
        Err(failure) => return failed(failure.to_string()),
    };
    if let Some(mut pipe) = child.stdin.take()
        && let Some(input) = input
    {
        let _ = std::io::Write::write_all(&mut pipe, input.as_bytes());
    }
    match child.wait_with_output() {
        Ok(output) => Exited {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(failure) => failed(failure.to_string()),
    }
}

/// A value as the text an environment variable holds.
// @lfy def/interpret/main.lfy:expand
fn plain_text(value: &Interim) -> String {
    match value {
        Interim::String(text) => text.clone(),
        Interim::Number(number) => json_number(*number),
        Interim::Bool(held) => held.to_string(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------------------
// expand
// ---------------------------------------------------------------------------------------

/// Why a statement cannot run yet.
// @lfy def/interpret/main.lfy:expand
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotReady {
    /// It reads what a trait was applied to, and a statement left to run can still apply
    /// that trait.
    Waiting { at: NodeRef, applied: EntityId },
    /// A name it needs to run does not resolve.
    Unresolved(NodeRef),
}

/// Runs every piece of compile-time code that can add names, until nothing new appears.
///
/// This is the binder's expand pass: it runs after the declare pass and before the resolve
/// pass, over every file in bind order. Each round runs every compile-time statement not
/// yet run whose names now resolve — trait applications, `ace` statements, declaration
/// bodies, and file-level statements — and rounds repeat until one runs nothing. Running a
/// statement changes only the model: symbols join scopes, members and values join entities,
/// trait applications join entities, and criteria and tests join entities; no [`Tree`] is
/// ever changed.
///
/// [`Tree`]: crate::parser::Tree
// @lfy def/interpret/main.lfy:expand
pub fn expand(mut model: Model) -> Model {
    // Every statement of every file, in bind order, which places a file after the files it
    // uses.
    // @lfy def/interpret/main.lfy:expand
    let mut pending: Vec<NodeRef> = Vec::new();
    for file in 0..model.sources.len() {
        pending.extend(model.statements_of(file));
    }
    let mut interpreter = Interpreter::new(&mut model);
    // Rounds repeat until one runs nothing.
    // @lfy def/interpret/main.lfy:expand
    loop {
        let mut ran_something = false;
        let mut deferred: Vec<NodeRef> = Vec::new();
        for index in 0..pending.len() {
            let item = pending[index];
            let others: Vec<NodeRef> = deferred
                .iter()
                .copied()
                .chain(pending[index + 1..].iter().copied())
                .collect();
            match interpreter.readiness(item, &others) {
                None => {
                    interpreter.run_statement(item);
                    ran_something = true;
                }
                Some(_) => deferred.push(item),
            }
        }
        pending = deferred;
        if !ran_something {
            break;
        }
    }
    // The rounds have stopped and these never ran.
    // @lfy def/interpret/main.lfy:expand
    for index in 0..pending.len() {
        let item = pending[index];
        let others: Vec<NodeRef> = pending
            .iter()
            .enumerate()
            .filter(|&(other, _)| other != index)
            .map(|(_, &node)| node)
            .collect();
        match interpreter.readiness(item, &others) {
            // Every statement left is waiting to read what a trait was applied to, and
            // none can run: each is named, with the trait it waits on, and none of them
            // runs.
            // @lfy def/interpret/main.lfy:expand
            Some(NotReady::Waiting { at, applied }) => {
                let name = interpreter.model.entities[applied]
                    .identifier
                    .clone()
                    .unwrap_or_else(|| "a trait".to_string());
                interpreter.problem(
                    at,
                    format!(
                        "this statement waits to read what {name} was applied to, and every \
                         statement left to run is waiting too, so none of them runs"
                    ),
                );
            }
            // A name it needs still does not resolve: it is named, and the statement runs
            // as far as it can.
            // @lfy def/interpret/main.lfy:expand
            Some(NotReady::Unresolved(at)) => {
                let text = interpreter.model.raw(at).trim().to_string();
                interpreter.problem(at, format!("{text} does not resolve"));
                interpreter.run_statement(item);
            }
            None => interpreter.run_statement(item),
        }
    }
    model
}

impl Interpreter<'_> {
    /// Runs one statement of the top level of a file.
    // @lfy def/interpret/main.lfy:expand
    fn run_statement(&mut self, item: NodeRef) {
        let mut env = self.file_env(item.file);
        self.exec(item, &mut env);
    }

    /// Why a statement cannot run yet, or nothing when it can.
    // @lfy def/interpret/main.lfy:expand
    fn readiness(&mut self, item: NodeRef, others: &[NodeRef]) -> Option<NotReady> {
        // A statement that reads what a trait was applied to waits until no statement left
        // to run can apply that trait, so the list it reads is the full one.
        // @lfy def/interpret/main.lfy:expand
        for (at, applied) in self.entity_reads(item) {
            if others.iter().any(|&other| self.applies(other, applied)) {
                return Some(NotReady::Waiting { at, applied });
            }
        }
        // Names are resolved only as far as a statement needs them to run; the resolve pass
        // resolves the rest.
        // @lfy def/interpret/main.lfy:expand
        for at in self.model.descendants(item) {
            if self.model.is(at, E::TraitUse) && resolve_trait_use(self.model, at).is_none() {
                return Some(NotReady::Unresolved(at));
            }
        }
        None
    }

    /// Every read of what a trait was applied to inside a statement: where it is read, and
    /// the trait it reads.
    // @lfy def/interpret/main.lfy:expand
    fn entity_reads(&self, item: NodeRef) -> Vec<(NodeRef, EntityId)> {
        let mut out = Vec::new();
        for node in self.model.descendants(item) {
            if !self.model.is(node, E::Member) {
                continue;
            }
            let Some((accessor, Some(name))) = self.model.accessor_and_name(node) else {
                continue;
            };
            if !self.model.token_is(node.file, accessor, P::ContextAccessor)
                || !matches!(name.as_str(), "entities" | "extenders")
            {
                continue;
            }
            let Some(left) = self.model.left(node) else {
                continue;
            };
            if let Some(symbol) = resolve_name_node(self.model, left) {
                let entity = self.model.symbols[symbol].entity;
                if self.model.entities[entity].is_trait() {
                    out.push((node, entity));
                }
            }
        }
        out
    }

    /// Whether running a statement could apply a trait: it names a trait in a clause, or
    /// calls `apply` on one, that applies the trait itself or reaches it through an
    /// `extends` chain.
    // @lfy def/interpret/main.lfy:expand
    fn applies(&self, item: NodeRef, applied: EntityId) -> bool {
        for node in self.model.descendants(item) {
            if self.model.is(node, E::TraitUse)
                && resolve_trait_use(self.model, node).is_some_and(|symbol| {
                    self.extends_reaches(self.model.symbols[symbol].entity, applied)
                })
            {
                return true;
            }
            if self.model.is(node, E::Call)
                && compile_time_call(self.model, node) == Some(CompileCall::Apply)
                && let Some(callee) = self.model.left(node)
                && let Some(receiver) = self.model.left(callee)
                && resolve_name_node(self.model, receiver).is_some_and(|symbol| {
                    self.extends_reaches(self.model.symbols[symbol].entity, applied)
                })
            {
                return true;
            }
        }
        false
    }

    /// Whether applying one trait applies another: the trait itself, or one its `extends`
    /// clause names, up the whole chain. The chain is read from the declarations rather
    /// than from what a trait already carries, because the statement that would apply it
    /// may run before the declaration whose body records the chain.
    // @lfy def/interpret/main.lfy:expand
    fn extends_reaches(&self, from: EntityId, applied: EntityId) -> bool {
        let mut seen = vec![from];
        let mut queue = vec![from];
        while let Some(current) = queue.pop() {
            if current == applied {
                return true;
            }
            let Some(node) = self.model.entities[current].node else {
                continue;
            };
            let Some(clause) = self.model.child(node, E::ExtendsClause) else {
                continue;
            };
            let Some(uses) = self.model.child(clause, E::TraitUses) else {
                continue;
            };
            for trait_use in self.model.children_of(uses, E::TraitUse) {
                if let Some(symbol) = resolve_trait_use(self.model, trait_use) {
                    let base = self.model.symbols[symbol].entity;
                    if !seen.contains(&base) {
                        seen.push(base);
                        queue.push(base);
                    }
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------------------
// evaluate
// ---------------------------------------------------------------------------------------

/// The value of an expression, computed at compile time when something asks for it.
///
/// The node is evaluated in the scope it is written in, against the model as [`expand`]
/// left it, and the same node evaluated twice gives the same value. Nothing comes back when
/// the node depends on a parameter or a variable that only has a value at runtime, and no
/// problem is added for that; the caller decides whether it is one. A value that needs its
/// own, directly or through other nodes, gives nothing back and adds a problem naming the
/// cycle.
// @lfy def/interpret/main.lfy:evaluate
pub fn evaluate(model: &mut Model, node: NodeRef) -> Option<Evaluated> {
    let mut interpreter = Interpreter::new(model);
    interpreter.runs_functions = false;
    interpreter.writes = false;
    interpreter.evaluate_node(node)
}

// ---------------------------------------------------------------------------------------
// lower
// ---------------------------------------------------------------------------------------

/// The expression rules a folded node never replaces, because their value is not all they
/// are: an assignment sets something, a spread spreads, an await waits, a cast reads a
/// value as a type, a definition attaches a description, and an inline function is a body.
// @lfy def/interpret/main.lfy:lower
fn never_folded(rule: Rule) -> bool {
    [
        E::Assignment.entity(),
        E::SpreadOperation.entity(),
        E::AwaitOperation.entity(),
        E::InOperation.entity(),
        E::OfOperation.entity(),
        E::FromOperation.entity(),
        E::Cast.entity(),
        E::Definition.entity(),
        E::InlineFunction.entity(),
        E::RangeOperation.entity(),
    ]
    .contains(&rule)
}

/// The rules that hold a name being declared or a type being written, rather than a value
/// being read: nothing inside one is folded, because what it stands for is its text.
// @lfy def/interpret/main.lfy:lower
fn declares_or_types(rule: Rule) -> bool {
    [
        E::Declared.entity(),
        E::Signature.entity(),
        E::Parameters.entity(),
        E::Parameter.entity(),
        E::SpreadParameter.entity(),
        E::TypeParameters.entity(),
        E::TypeParameter.entity(),
        E::TypeArguments.entity(),
        E::TypeExpression.entity(),
        E::TypeItem.entity(),
        E::TypeGroup.entity(),
        E::TypeValue.entity(),
        E::TypeKey.entity(),
        E::FunctionType.entity(),
        E::DefinitionClause.entity(),
        E::MemberName.entity(),
    ]
    .contains(&rule)
}

/// Lowering one workspace.
// @lfy def/interpret/main.lfy:lower
struct Lowering<'m> {
    interpreter: Interpreter<'m>,
    /// The entities whose criteria are the guidance of a target rather than criteria of the
    /// program.
    // @lfy def/interpret/main.lfy:lower
    guidance: HashSet<EntityId>,
    /// The local criteria of each entity, in order.
    criteria: HashMap<EntityId, Vec<LoweredCriterion>>,
    /// The local tests of each entity, in order.
    tests: HashMap<EntityId, Vec<LoweredTest>>,
    /// Every global criterion, in the order they were added.
    global_criteria: Vec<LoweredCriterion>,
    /// Every global test, in the order they were added.
    global_tests: Vec<LoweredTest>,
    /// Every problem lowering added.
    problems: Vec<Problem>,
    /// Where the project's own source lives, relative to the workspace root: what an added
    /// use's alias is named after.
    // @lfy def/interpret/main.lfy:lower
    source_directory: String,
    /// The name and directory of the package each file came from, by source index.
    // @lfy def/interpret/main.lfy:lower
    packages: HashMap<FileId, (String, String)>,
}

/// The uses one lowered file's text takes beyond the ones written in it, and the alias each
/// entity whose name would otherwise mean two things is spelled through.
// @lfy def/interpret/main.lfy:lower
#[derive(Default)]
struct AddedUses {
    /// One `Use` per file, in the order the members that need it are spelled.
    // @lfy def/interpret/main.lfy:lower
    uses: Vec<String>,
    /// The alias an entity is spelled through, for every entity of an aliased use.
    // @lfy def/interpret/main.lfy:lower
    aliases: HashMap<EntityId, String>,
}

/// The program as generation sees it: runtime code only, with compile-time results in place.
///
/// Every node whose [`phase_of`] is [`Phase::Runtime`], that is not inside a
/// [`Phase::Compile`] node or an `ace` function, that is not replaced by a folded node, and
/// that is not an expression statement whose expression is [`Phase::Compile`], is kept
/// with the same rule and its kept children in order; every lowered node's origin is the
/// parse node it came from, and a folded node's origin is the expression it replaced.
/// Every criterion and every test of every entity is lowered once, into a
/// [`LoweredCriterion`] or a [`LoweredTest`] with its id, and never into the tree's
/// children.
// @lfy def/interpret/main.lfy:lower
pub fn lower(mut workspace: Workspace) -> Program {
    let mut model = std::mem::take(&mut workspace.model);
    let mut files = Vec::new();
    let (criteria, tests, problems);
    {
        let mut lowering = Lowering::new(&mut model, &workspace);
        lowering.take_requirements();
        // One lowered file per file of the workspace, in the same order.
        // @lfy def/interpret/main.lfy:lower
        for (index, file) in workspace.files.iter().enumerate() {
            files.push(lowering.lower_file(index, file.source));
        }
        criteria = std::mem::take(&mut lowering.global_criteria);
        tests = std::mem::take(&mut lowering.global_tests);
        problems = std::mem::take(&mut lowering.problems);
    }
    workspace.model = model;
    Program {
        workspace,
        files,
        criteria,
        tests,
        problems,
    }
}

impl<'m> Lowering<'m> {
    // @lfy def/interpret/main.lfy:lower
    fn new(model: &'m mut Model, workspace: &Workspace) -> Lowering<'m> {
        let guidance = guidance_entities(workspace);
        let mut interpreter = Interpreter::new(model);
        interpreter.runs_functions = false;
        interpreter.writes = false;
        Lowering {
            interpreter,
            guidance,
            criteria: HashMap::new(),
            tests: HashMap::new(),
            global_criteria: Vec::new(),
            global_tests: Vec::new(),
            problems: Vec::new(),
            source_directory: workspace.source_directory.clone(),
            packages: workspace
                .files
                .iter()
                .filter_map(|file| {
                    let package = workspace.packages.get(file.package?)?;
                    Some((
                        file.source,
                        (package.identifier.clone(), package.root.clone()),
                    ))
                })
                .collect(),
        }
    }

    // @lfy def/interpret/main.lfy:lower
    fn model(&self) -> &Model {
        self.interpreter.model
    }

    /// Lowers every criterion and every test of every entity, once, and sorts each into the
    /// place it belongs: a global one into the program, a local one under its entity.
    // @lfy def/interpret/main.lfy:lower#lower:lower:958d30dcc98b23d0e9373cc5d9ee9503e4e30d4587d485a2f01b9a25cdb0a2d8
    fn take_requirements(&mut self) {
        let global = self.model().global;
        let mut criteria: Vec<LoweredCriterion> = Vec::new();
        let mut tests: Vec<LoweredTest> = Vec::new();
        for entity in 0..self.model().entities.len() {
            let is_global = entity == global;
            let receiver = receiver_name(self.model(), Some(entity));
            let receiver = if is_global {
                receiver_name(self.model(), None)
            } else {
                receiver
            };
            let own = self.model().entities[entity].acceptance_criteria.clone();
            // A criterion the declaration of a target holds, its own or from a layer of the
            // target, is guidance for that target and neither a local nor a global
            // requirement: every request of the target already carries it as guidance.
            // @lfy def/interpret/main.lfy:lower#lower:lower:3032db4bcf2ffe2031d9fceb6ceced6ed34b2271c3cc994fa6cfb219215ef323
            if !self.guidance.contains(&entity) {
                for criterion in own {
                    criteria.push(self.lowered_criterion(entity, is_global, &receiver, &criterion));
                }
            }
            let cases = self.model().entities[entity].tests.clone();
            for case in cases {
                tests.push(self.lowered_test(entity, is_global, &receiver, &case));
            }
        }
        LoweredCriterion::number_repeats(&mut criteria);
        LoweredTest::number_repeats(&mut tests);
        // Two entities may share a name, so two requirements of different entities may still
        // share an id; they are numbered the same way, so that one id names one requirement.
        // @lfy def/interpret/main.lfy:lower
        let numbered = number_collisions(criteria.iter().map(|c| c.id.clone()).collect());
        for (criterion, id) in criteria.iter_mut().zip(numbered) {
            criterion.id = id;
        }
        let numbered = number_collisions(tests.iter().map(|case| case.id.clone()).collect());
        for (case, id) in tests.iter_mut().zip(numbered) {
            case.id = id;
        }
        for criterion in criteria {
            match criterion.scope {
                // A global criterion is in the program once, with global in its id and no
                // entity, and in no node or file.
                // @lfy def/interpret/main.lfy:lower#lower:lower:8b72d813b6781ac6a5965eeefda77688fb7586238258b0ef7bbfaf22db866dd6
                RequirementScope::Global => self.global_criteria.push(criterion),
                RequirementScope::Local => {
                    let entity = criterion.entity.expect("a local criterion is for an entity");
                    self.criteria.entry(entity).or_default().push(criterion);
                }
            }
        }
        for case in tests {
            match case.scope {
                // A global test is in the program once, with global in its id and no
                // entity, and in no node or file.
                // @lfy def/interpret/main.lfy:lower#lower:lower:699284343b9ef81cb11db5668663aac649e418507ad5cea495ddceee254e4d43
                RequirementScope::Global => self.global_tests.push(case),
                RequirementScope::Local => {
                    let entity = case.entity.expect("a local test is for an entity");
                    self.tests.entry(entity).or_default().push(case);
                }
            }
        }
    }

    // @lfy def/interpret/main.lfy:lower
    fn lowered_criterion(
        &mut self,
        entity: EntityId,
        is_global: bool,
        receiver: &str,
        criterion: &Criterion,
    ) -> LoweredCriterion {
        let contributor = receiver_name(self.model(), Some(criterion.contributor));
        let origin = criterion
            .node
            .or(self.model().entities[criterion.contributor].node)
            .unwrap_or(SYNTHETIC);
        let references = self.references_of(criterion, origin);
        let rendered = Criterion {
            situation: strip_each(&criterion.situation),
            behavior: strip_each(&criterion.behavior),
            side_effects: strip_each(&criterion.side_effects),
            contributor: criterion.contributor,
            node: criterion.node,
        };
        LoweredCriterion {
            id: LoweredCriterion::id(receiver, &contributor, &rendered),
            scope: if is_global {
                RequirementScope::Global
            } else {
                RequirementScope::Local
            },
            entity: (!is_global).then_some(entity),
            origin,
            references,
            contributor: criterion.contributor,
            situation: rendered.situation,
            behavior: rendered.behavior,
            side_effects: rendered.side_effects,
        }
    }

    // @lfy def/interpret/main.lfy:lower
    fn lowered_test(
        &mut self,
        entity: EntityId,
        is_global: bool,
        receiver: &str,
        case: &Test,
    ) -> LoweredTest {
        let contributor = receiver;
        let origin = case
            .input
            .or(case.expect)
            .or(self.model().entities[entity].node)
            .unwrap_or(SYNTHETIC);
        let input_value = self.evaluated_or_undefined(case.input);
        let expect_value = self.evaluated_or_undefined(case.expect);
        LoweredTest {
            id: LoweredTest::id(receiver, contributor, case),
            scope: if is_global {
                RequirementScope::Global
            } else {
                RequirementScope::Local
            },
            entity: (!is_global).then_some(entity),
            origin,
            input_value,
            expect_value,
            input: case.input,
            expect: case.expect,
            input_text: case.input_text.clone(),
            expect_text: case.expect_text.clone(),
        }
    }

    /// A test's input or expectation, evaluated; what only runtime could answer is
    /// undefined.
    // @lfy def/interpret/main.lfy:lower
    fn evaluated_or_undefined(&mut self, node: Option<NodeRef>) -> Evaluated {
        node.and_then(|node| self.interpreter.evaluate_node(node))
            .unwrap_or(Evaluated::Value(Value::Undefined))
    }

    /// Every entity a criterion names with a bracketed reference, in order, kept as links
    /// rather than rendered names.
    // @lfy def/interpret/main.lfy:lower
    fn references_of(&self, criterion: &Criterion, origin: NodeRef) -> Vec<EntityId> {
        let scope = if origin == SYNTHETIC {
            self.model()
                .entities[criterion.contributor]
                .scope
                .unwrap_or(0)
        } else {
            self.model().enclosing_scope(origin)
        };
        let mut out = Vec::new();
        for text in [
            &criterion.situation,
            &criterion.behavior,
            &criterion.side_effects,
        ]
        .into_iter()
        .flatten()
        .flatten()
        {
            for name in bracketed(text) {
                if let Some(entity) = self.reference_entity(scope, &name)
                    && !out.contains(&entity)
                {
                    out.push(entity);
                }
            }
        }
        out
    }

    /// The entity a bracketed name stands for: its head looked up in the scope the
    /// criterion was written in, then each part after a dot as a member of the one before.
    // @lfy def/interpret/main.lfy:lower
    fn reference_entity(&self, scope: ScopeId, name: &str) -> Option<EntityId> {
        let mut parts = name.split('.');
        let head = parts.next()?;
        let symbol = lookup(self.model(), scope, head.trim())?;
        let mut entity = self.model().symbols[symbol].entity;
        for part in parts {
            let member = member_symbol(self.model(), entity, part.trim())?;
            entity = self.model().symbols[member].entity;
        }
        Some(entity)
    }

    // ---- The tree ------------------------------------------------------------------

    /// One file as generation sees it.
    // @lfy def/interpret/main.lfy:lower
    fn lower_file(&mut self, index: FileId, source: FileId) -> LoweredFile {
        let root = NodeRef {
            file: source,
            index: 0,
        };
        // A file that holds only compile-time statements lowers to an empty SourceFile.
        // @lfy def/interpret/main.lfy:lower
        let lowered = self.lower_node(root).unwrap_or(LoweredNode {
            rule: F::SourceFile.entity(),
            children: Vec::new(),
            origin: root,
            entity: self.model().file_entities.get(source).copied(),
            value: None,
            criteria: Vec::new(),
            tests: Vec::new(),
        });
        // A member's type may name an entity of a file this one does not use, because a
        // trait of another file added the member; the text takes a use of that file so the
        // name it spells means something.
        // @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
        // @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
        let added = self.added_uses(source, &lowered);
        let text = render(self.model(), &lowered, &added.aliases);
        let text = with_uses(&text, &added.uses);
        let own = self.model().file_entities.get(source).copied();
        // A criterion or a test for the file's own entity joins the file, not a node.
        // @lfy def/interpret/main.lfy:lower
        // @lfy def/interpret/main.lfy:lower
        let criteria = own
            .and_then(|entity| self.criteria.remove(&entity))
            .unwrap_or_default();
        let tests = own
            .and_then(|entity| self.tests.remove(&entity))
            .unwrap_or_default();
        LoweredFile {
            file: index,
            root: lowered,
            text,
            criteria,
            tests,
        }
    }

    /// The uses a lowered file's text takes beyond the ones written in it: one per file a
    /// member's type names and the file does not use, however many members name it, in the
    /// order those members are spelled.
    ///
    /// Decision: a file of the library or the prelude is never added, because what it
    /// declares is in scope in every file already; a file of a package is added by the
    /// package's name, a slash, and its path under the package.
    // @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
    fn added_uses(&self, source: FileId, lowered: &LoweredNode) -> AddedUses {
        let mut added = AddedUses::default();
        let model = self.model();
        if model.sources[source].origin != Origin::Program {
            return added;
        }
        let mut named = Vec::new();
        member_type_entities(model, lowered, &mut named);
        // One use carries every entity of its file, so the file decides whether the names
        // it brings need an alias and each of them is spelled the same way.
        // @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
        let mut by_file: Vec<(FileId, Vec<EntityId>)> = Vec::new();
        for entity in named {
            let Some(node) = model.entities[entity].node else {
                continue;
            };
            let declared = node.file;
            if declared == source || declared >= model.sources.len() {
                continue;
            }
            if model.sources[declared].origin != Origin::Program
                || model.entities[entity].identifier.is_none()            {
                continue;
            }
            let path = &model.sources[declared].path;
            if model.sources[source]
                .uses
                .iter()
                .flatten()
                .any(|used| used == path)
            {
                continue;
            }
            match by_file.iter_mut().find(|(file, _)| *file == declared) {
                Some((_, entities)) => {
                    if !entities.contains(&entity) {
                        entities.push(entity);
                    }
                }
                None => by_file.push((declared, vec![entity])),
            }
        }
        let scope = model.file_scopes[source];
        for (declared, entities) in by_file {
            // The use takes an alias when the file already sees another entity by one of
            // the names it brings, because one name cannot mean two entities.
            // @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
            let taken = entities.iter().any(|&entity| {
                let name = model.entities[entity].identifier.as_deref().unwrap_or("");
                model
                    .lookup(scope, name)
                    .is_some_and(|symbol| model.symbols[symbol].entity != entity)
            });
            // A file of a package is spelled by the package's name, never relative to the
            // owner's file.
            // @lfy def/interpret/main.lfy:lower#lower:lower:7e159bfaa7ce0d4765fa4fdbd906f7230afdf9fce2ae0297f29ee8f15f2af929
            // @lfy def/interpret/main.lfy:lower#lower:lower:28987d915312dda8a1c80b32f3c8d343bef0cf0d86b3f288a863238a9a9837fa
            let path = match self.packages.get(&declared) {
                Some((name, root)) => package_use_path(name, root, &model.sources[declared].path),
                None => use_path(&model.sources[source].path, &model.sources[declared].path),
            };
            if !taken {
                added.uses.push(format!("use \"{path}\";"));
                continue;
            }
            let base = alias_of(&self.source_directory, &model.sources[declared].path);
            let mut alias = base.clone();
            let mut number = 0usize;
            while model.lookup(scope, &alias).is_some()
                || added.aliases.values().any(|taken| *taken == alias)
            {
                number += 1;
                alias = format!("{base}{number}");
            }
            added.uses.push(format!("use \"{path}\" as {alias};"));
            for entity in entities {
                added.aliases.insert(entity, alias.clone());
            }
        }
        added
    }

    /// One node of the program as generation sees it, or nothing when it runs at compile
    /// time.
    // @lfy def/interpret/main.lfy:lower
    fn lower_node(&mut self, r: NodeRef) -> Option<LoweredNode> {
        // A node whose phase is compile has no lowered node, and neither does anything
        // inside it; nor does an expression statement whose expression runs at compile time.
        // @lfy def/interpret/main.lfy:lower#lower:lower:3dd593ebd9e4eded0e675a008bfbb128a421d899ec79d8273982fb718148de70
        if self.is_dropped(r) {
            return None;
        }
        self.check_statement(r);
        if let Some(value) = self.fold(r) {
            // A folded node's origin is the expression it replaced.
            // @lfy def/interpret/main.lfy:lower
            return Some(LoweredNode {
                rule: self.model().rule_of(r),
                children: Vec::new(),
                origin: r,
                entity: None,
                value: Some(value),
                criteria: Vec::new(),
                tests: Vec::new(),
            });
        }
        let mut children: Vec<LoweredChild> = Vec::new();
        for child in self.model().node(r).children.clone() {
            match child {
                Child::Token(index) => {
                    let token = self.model().token_at(r.file, index).clone();
                    children.push(LoweredChild::Token(token));
                }
                // Trivia is kept as the tokens it is written with, so that every lowered
                // node of a tree stands for something the program says.
                // @lfy def/interpret/main.lfy:lower
                Child::Node(node) if is_trivia(node.rule) => {
                    for index in node.token_indices() {
                        let token = self.model().token_at(r.file, index).clone();
                        children.push(LoweredChild::Token(token));
                    }
                }
                Child::Node(node) => {
                    let Some(child) = self.model().node_ref(r.file, &node) else {
                        continue;
                    };
                    match self.lower_node(child) {
                        Some(lowered) => children.push(LoweredChild::Node(lowered)),
                        // What is dropped takes the space written before it with it, so the
                        // text of a kept declaration reads as though the clause or the body
                        // had never been written.
                        // @lfy def/interpret/main.lfy:lower
                        None => while_trivia_pop(&mut children),
                    }
                }
                Child::Error(_) => {}
            }
        }
        let entity = self.model().symbol_of(r).map(|s| self.model().symbols[s].entity);
        let entity = entity.or_else(|| {
            self.model()
                .is(r, F::SourceFile)
                .then(|| self.model().file_entities[r.file])
        });
        // A kept declaration of an entity with members has one child node per member, in
        // member order, after its other kept children.
        // @lfy def/interpret/main.lfy:lower#lower:lower:5531392df6668cb6852deb49090314e7229171d099b764df568cd918c1eeb2c0
        if let Some(entity) = entity {
            for member in self.member_nodes(entity, r) {
                children.push(LoweredChild::Node(member));
            }
        }
        // A criterion or a test for the entity a node declares joins that node.
        // @lfy def/interpret/main.lfy:lower#lower:lower:8ae967486c3764064704fd7f57756f5f55bfb6c3e2550eab46a24b06e02c3040
        // @lfy def/interpret/main.lfy:lower#lower:lower:8b962cee67dcd0e481fc64461f7e97a28f49d8736c64ea75d7bf8661cbbaeb0b
        let is_file = self.model().is(r, F::SourceFile);
        let criteria = match entity {
            Some(entity) if !is_file => self.criteria.remove(&entity).unwrap_or_default(),
            _ => Vec::new(),
        };
        let tests = match entity {
            Some(entity) if !is_file => self.tests.remove(&entity).unwrap_or_default(),
            _ => Vec::new(),
        };
        Some(LoweredNode {
            rule: self.model().rule_of(r),
            children,
            // Every lowered node's origin is the parse node it came from.
            // @lfy def/interpret/main.lfy:lower
            origin: r,
            entity,
            value: None,
            criteria,
            tests,
        })
    }

    /// Whether a node has no lowered node: it runs at compile time, or it is an expression
    /// statement whose expression runs at compile time — an `add` on an entity's criteria, a
    /// `test` on its context, an `apply` on a trait — so nothing of it is left to keep and
    /// the statement goes with its expression rather than staying as a bare semicolon.
    // @lfy def/interpret/main.lfy:lower
    fn is_dropped(&self, r: NodeRef) -> bool {
        if phase_of(self.model(), r) == Phase::Compile {
            return true;
        }
        self.model().is(r, S::ExpressionStatement)
            && self
                .model()
                .child_nodes(r)
                .first()
                .is_some_and(|&expression| phase_of(self.model(), expression) == Phase::Compile)
    }

    /// One lowered node per member of an entity, in member order; a member the declaration
    /// still spells itself, such as a key of an enum or of a type, is already kept.
    // @lfy def/interpret/main.lfy:lower
    fn member_nodes(&mut self, entity: EntityId, declaration: NodeRef) -> Vec<LoweredNode> {
        let members: Vec<SymbolId> = self
            .model()
            .members(entity)
            .into_iter()
            .filter(|&symbol| {
                let record = &self.model().symbols[symbol];
                if !matches!(record.kind, SymbolKind::Member | SymbolKind::EnumMember) {
                    return false;
                }
                !(within(self.model(), record.node, declaration)
                    && phase_of(self.model(), record.node) == Phase::Runtime)
            })
            .collect();
        let mut out = Vec::new();
        for member in members {
            let declared = self.model().symbols[member].node;
            // The origin is the statement that declared the member, which for a member a
            // trait added is the statement in the trait's file.
            // @lfy def/interpret/main.lfy:lower#lower:lower:5531392df6668cb6852deb49090314e7229171d099b764df568cd918c1eeb2c0
            // @lfy def/interpret/main.lfy:lower#lower:lower:dfcda962889085ca36762f22b94e05974c5cc36a48a79b45284c3938ccfe0bb5
            let origin = statement_of(self.model(), declared);
            let held = self.model().symbols[member].entity;
            // A member written as a key of a type or of an enum is that key; one written as
            // a statement of a body is that statement.
            let rule = if self.model().is(declared, E::TypeKey)
                || self.model().is(declared, E::ObjectKey)
            {
                self.model().rule_of(declared)
            } else {
                self.model().rule_of(origin)
            };
            out.push(LoweredNode {
                rule,
                children: Vec::new(),
                origin,
                entity: Some(held),
                value: None,
                criteria: self.criteria.remove(&held).unwrap_or_default(),
                tests: self.tests.remove(&held).unwrap_or_default(),
            });
        }
        out
    }

    /// The problems a kept statement or call adds.
    // @lfy def/interpret/main.lfy:lower
    fn check_statement(&mut self, r: NodeRef) {
        // A file-level `let` is not allowed, and it is kept.
        // @lfy def/interpret/main.lfy:lower#lower:lower:c89173885cbe173a76e8a9a59310d5e69174bce617247c866b7d675aeeb308e2
        if self.model().is(r, S::VariableDeclaration)
            && self.model().has_token(r, K::LetKeyword)
            && self
                .model()
                .parent(r)
                .is_some_and(|up| self.model().is(up, F::SourceFile))
        {
            self.problems.push(Problem {
                node: r,
                message: "a file-level let is not allowed".to_string(),
                stage: crate::model::Stage::Binder,
            });
        }
        // A function that is not ace whose body holds statements, every one of which runs at
        // compile time: the statements run as they would anywhere else, none of them has a
        // lowered node, and the function is named, because its body is empty at runtime. A
        // function inside an `ace` never reaches here: the `ace` runs at compile time, so
        // nothing inside it is lowered at all.
        // @lfy def/interpret/main.lfy:lower#lower:lower:72779fb5f9a2ff6556d57709eb8eaebdb63e30288aa7374e2f904480042c28a7
        if self.model().is(r, S::FunctionDeclaration)
            && let Some(block) = self.model().child(r, S::Block)
        {
            let statements = self.model().child_nodes(block);
            if !statements.is_empty() && statements.iter().all(|&item| self.is_dropped(item)) {
                self.problems.push(Problem {
                    node: r,
                    message: "this function's body is empty at runtime, so it should be ace or \
                              hold runtime code"
                        .to_string(),
                    stage: crate::model::Stage::Binder,
                });
            }
        }
        // Runtime code that calls an ace function with an argument no value can be taken
        // for has nothing to call, because the function will not exist at runtime.
        // @lfy def/interpret/main.lfy:lower
        if self.model().is(r, E::Call)
            && self.calls_an_ace_function(r)
            && self.interpreter.evaluate_node(r).is_none()
        {
            self.problems.push(Problem {
                node: r,
                message: "this calls an ace function with an argument that has no value at \
                          compile time, and the function will not exist at runtime"
                    .to_string(),
                stage: crate::model::Stage::Binder,
            });
        }
    }

    /// Whether a call names a function declared inside an `ace`.
    // @lfy def/interpret/main.lfy:lower
    fn calls_an_ace_function(&self, call: NodeRef) -> bool {
        let Some(callee) = self.model().left(call) else {
            return false;
        };
        let Some(symbol) = resolve_name_node(self.model(), callee) else {
            return false;
        };
        let entity = self.model().symbols[symbol].entity;
        self.model().entities[entity]
            .node
            .is_some_and(|node| self.model().ancestor(node, S::Ace).is_some())
    }

    /// The value a folded node holds, when the node is one.
    // @lfy def/interpret/main.lfy:lower
    fn fold(&mut self, r: NodeRef) -> Option<Evaluated> {
        let rule = self.model().rule_of(r);
        let expression =
            rule.is_primary() || rule.is_prefix() || rule.is_infix() || rule.is_postfix();
        if !expression || never_folded(rule) {
            return None;
        }
        // Nothing inside a declared name or a written type is folded: what it stands for is
        // its text.
        if self.no_fold_region(r) {
            return None;
        }
        // A call is folded only when it is a call of like, or of an ace function, which will
        // not exist at runtime.
        // @lfy def/interpret/main.lfy:lower
        if rule == E::Call.entity() {
            let mut env = self.interpreter.file_env(r.file);
            if let Some(prompted) = self.interpreter.prompted_of(r, &mut env) {
                return Some(Evaluated::Prompted(prompted));
            }
            if !self.calls_an_ace_function(r) {
                return None;
            }
        }
        self.interpreter.evaluate_node(r)
    }

    /// Whether a node stands inside a declared name or a written type.
    // @lfy def/interpret/main.lfy:lower
    fn no_fold_region(&self, r: NodeRef) -> bool {
        let mut at = Some(r);
        while let Some(node) = at {
            let rule = self.model().rule_of(node);
            if declares_or_types(rule) {
                return true;
            }
            if rule.is_statement() || rule == F::SourceFile.entity() {
                return false;
            }
            at = self.model().parent(node);
        }
        false
    }
}

/// Every entity whose criteria are the guidance of a target: the declaration of each one,
/// which carries its own criteria and those of each of its layers.
///
/// [`Target::declaration`]: crate::workspace::Target
// @lfy def/interpret/main.lfy:lower
fn guidance_entities(workspace: &Workspace) -> HashSet<EntityId> {
    workspace
        .targets
        .iter()
        .map(|target| target.declaration)
        .collect()
}

/// Numbers the ids that still repeat: the first keeps its id and the second and later ones
/// end with a colon and their position among those, counting from 2.
// @lfy def/interpret/main.lfy:lower
fn number_collisions(ids: Vec<String>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    ids.into_iter()
        .map(|id| {
            let position = seen.entry(id.clone()).or_insert(0);
            *position += 1;
            if *position == 1 {
                id
            } else {
                format!("{id}:{position}")
            }
        })
        .collect()
}

/// Whether a node is the given one or stands inside it.
// @lfy def/interpret/main.lfy:lower
fn within(model: &Model, node: NodeRef, ancestor: NodeRef) -> bool {
    let mut at = Some(node);
    while let Some(current) = at {
        if current == ancestor {
            return true;
        }
        at = model.parent(current);
    }
    false
}

/// The nearest statement at or above a node, or the node itself when there is none.
// @lfy def/interpret/main.lfy:lower
fn statement_of(model: &Model, r: NodeRef) -> NodeRef {
    let mut at = Some(r);
    while let Some(node) = at {
        if model.rule_of(node).is_statement() {
            return node;
        }
        at = model.parent(node);
    }
    r
}

/// Drops the trivia tokens written just before something that was left out.
// @lfy def/interpret/main.lfy:lower
fn while_trivia_pop(children: &mut Vec<LoweredChild>) {
    while let Some(LoweredChild::Token(token)) = children.last() {
        let trivia = token.rule.is_some_and(is_trivia);
        if !trivia {
            return;
        }
        children.pop();
    }
}

/// `[[X]]` becomes `X` in each text of a criterion.
// @lfy def/interpret/main.lfy:lower
fn strip_each(texts: &Option<Vec<String>>) -> Option<Vec<String>> {
    texts
        .as_ref()
        .map(|texts| texts.iter().map(|text| strip_references(text)).collect())
}

/// The names a text holds in brackets, in order.
// @lfy def/interpret/main.lfy:lower
fn bracketed(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        rest = &rest[open + 2..];
        let Some(close) = rest.find("]]") else { break };
        out.push(rest[..close].to_string());
        rest = &rest[close + 2..];
    }
    out
}

/// Every entity the type of a lowered member node names, in the order the members are
/// spelled, each once.
// @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
fn member_type_entities(model: &Model, node: &LoweredNode, out: &mut Vec<EntityId>) {
    if is_member_node(model, node)
        && let Some(entity) = node.entity
        && let Some(ty) = &model.entities[entity].ty
    {
        type_entities(ty, out);
    }
    for child in node.nodes() {
        member_type_entities(model, child, out);
    }
}

/// Every entity a written type names, in order, each once.
// @lfy def/interpret/main.lfy:lower
fn type_entities(ty: &TypeRef, out: &mut Vec<EntityId>) {
    match ty {
        TypeRef::Entity(entity) | TypeRef::Predicate(entity) => {
            if !out.contains(entity) {
                out.push(*entity);
            }
        }
        TypeRef::List(item) => type_entities(item, out),
        TypeRef::Union(items) => items.iter().for_each(|item| type_entities(item, out)),
        _ => {}
    }
}

/// The path a use of one file is written with from another: the file's path without its
/// extension, relative to the directory of the file the use is written in.
// @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
fn use_path(from: &str, to: &str) -> String {
    let stem = to.strip_suffix(".lfy").unwrap_or(to);
    let here: Vec<&str> = from.split('/').collect();
    let here = &here[..here.len().saturating_sub(1)];
    let there: Vec<&str> = stem.split('/').collect();
    let shared = here
        .iter()
        .zip(there.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<&str> = vec![".."; here.len() - shared];
    parts.extend(there[shared..].iter().copied());
    let path = parts.join("/");
    if path.starts_with('.') {
        path
    } else {
        format!("./{path}")
    }
}

/// The alias an added use takes: the declaring file's path relative to the project's source
/// directory, without its extension and with each slash replaced by an underscore.
// @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
fn alias_of(source_directory: &str, path: &str) -> String {
    let stem = path.strip_suffix(".lfy").unwrap_or(path);
    let prefix = format!("{}/", source_directory.trim_end_matches('/'));
    let relative = stem.strip_prefix(&prefix).unwrap_or(stem);
    relative
        .chars()
        .map(|c| if unicode_ident::is_xid_continue(c) { c } else { '_' })
        .collect()
}

/// The path a use of a file of a package is written with: the package's name, a slash, and
/// the file's path relative to the package's directory, without its extension.
// @lfy def/interpret/main.lfy:lower#lower:lower:7e159bfaa7ce0d4765fa4fdbd906f7230afdf9fce2ae0297f29ee8f15f2af929
fn package_use_path(name: &str, root: &str, path: &str) -> String {
    let stem = path.strip_suffix(".lfy").unwrap_or(path);
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let relative = stem.strip_prefix(&prefix).unwrap_or(stem);
    format!("{name}/{relative}")
}

/// The text of a lowered file with the uses it takes written after the uses written in it,
/// or at its head when it writes none.
// @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
fn with_uses(text: &str, uses: &[String]) -> String {
    if uses.is_empty() {
        return text.to_string();
    }
    let added = uses.join("\n");
    let mut after = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("use ") {
            after = Some(at + line.len());
        }
        at += line.len();
    }
    match after {
        Some(at) => {
            let (head, tail) = text.split_at(at);
            let head = if head.ends_with('\n') {
                head.to_string()
            } else {
                format!("{head}\n")
            };
            format!("{head}{added}\n{tail}")
        }
        None => format!("{added}\n{text}"),
    }
}

/// The lowered tree spelled as Elfie source: kept nodes as written, folded nodes as the
/// literal of their value, and member nodes as a member with its type and value.
// @lfy def/interpret/main.lfy:lower
fn render(model: &Model, node: &LoweredNode, aliases: &HashMap<EntityId, String>) -> String {
    if let Some(value) = &node.value {
        return spell(model, value);
    }
    if let Some(entity) = node.entity
        && node.children.is_empty()
        && matches!(
            model.entities[entity].kind,
            EntityKind::Member | EntityKind::EnumMember
        )
    {
        return spell_member(model, entity, aliases);
    }
    let (members, kept): (Vec<&LoweredChild>, Vec<&LoweredChild>) =
        node.children.iter().partition(|child| match child {
            LoweredChild::Node(child) => is_member_node(model, child),
            LoweredChild::Token(_) => false,
        });
    let mut out = String::new();
    for child in kept {
        match child {
            LoweredChild::Node(child) => out.push_str(&render(model, child, aliases)),
            LoweredChild::Token(token) => out.push_str(&token.raw),
        }
    }
    // A data and a fn take a block or a semicolon; the block of either is compile time, so
    // what is left ends in the members that are kept apart from it, or in a semicolon.
    // @lfy def/interpret/main.lfy:lower
    let bodied =
        node.rule == S::DataDeclaration.entity() || node.rule == S::AgentFunctionDeclaration.entity();
    if members.is_empty() {
        let trimmed = out.trim_end();
        if bodied && !trimmed.ends_with(';') && !trimmed.ends_with('}') {
            return format!("{trimmed};{}", &out[trimmed.len()..]);
        }
        return out;
    }
    let mut body = out.trim_end().to_string();
    body.push_str(" {\n");
    for child in members {
        if let LoweredChild::Node(child) = child {
            body.push_str("  ");
            body.push_str(&render(model, child, aliases));
            body.push('\n');
        }
    }
    body.push('}');
    body
}

/// Whether a lowered node is a member of its parent's entity.
// @lfy def/interpret/main.lfy:lower
fn is_member_node(model: &Model, node: &LoweredNode) -> bool {
    node.value.is_none()
        && node.children.is_empty()
        && node.entity.is_some_and(|entity| {
            matches!(
                model.entities[entity].kind,
                EntityKind::Member | EntityKind::EnumMember
            )
        })
}

/// A member as Elfie spells one: its name, its description, and its type or value.
// @lfy def/interpret/main.lfy:lower
fn spell_member(model: &Model, entity: EntityId, aliases: &HashMap<EntityId, String>) -> String {
    let record: &Declared = &model.entities[entity];
    let name = record.identifier.clone().unwrap_or_default();
    let mut out = format!("${name}");
    if let Some(definition) = &record.definition {
        out.push_str(&format!(": `{definition}`"));
    }
    let written = record
        .ty
        .as_ref()
        .map(|ty| member_type_text(model, ty, aliases))
        .unwrap_or_default();
    if written.is_empty() {
        out.push(';');
    } else {
        out.push_str(&format!(" = {written};"));
    }
    out
}

/// The type of a member as Elfie spells one.
// @lfy def/interpret/main.lfy:lower
fn member_type_text(model: &Model, ty: &TypeRef, aliases: &HashMap<EntityId, String>) -> String {
    match ty {
        // A list of a union takes parentheses before its brackets, because `A | B[]` is a
        // union of `A` with a list of `B` and not a list of `A | B`.
        // @lfy def/interpret/main.lfy:lower#lower:lower:1b8bbc7b6e2316e2fb3cfd378dd5fc01093dbefd96e6098d404519e88a6a68ae
        TypeRef::List(item) => {
            let text = member_type_text(model, item, aliases);
            if matches!(**item, TypeRef::Union(_)) {
                format!("({text})[]")
            } else {
                format!("{text}[]")
            }
        }
        TypeRef::Union(items) => items
            .iter()
            .map(|item| member_type_text(model, item, aliases))
            .collect::<Vec<_>>()
            .join(" | "),
        // An entity brought in by an aliased use is spelled through that alias, so the name
        // means one entity.
        // @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
        TypeRef::Entity(entity) if aliases.contains_key(entity) => {
            let alias = &aliases[entity];
            format!("{alias}.{}", crate::model::type_text(model, ty))
        }
        TypeRef::Predicate(entity) if aliases.contains_key(entity) => {
            let alias = &aliases[entity];
            let name = model.entities[*entity].identifier.clone().unwrap_or_default();
            format!("is {alias}.{name}")
        }
        // Every other type is spelled as the model spells it.
        ty => crate::model::type_text(model, ty),
    }
}

/// A folded value as Elfie spells it; a prompted value as the call that asked for it.
// @lfy def/interpret/main.lfy:lower
fn spell(model: &Model, value: &Evaluated) -> String {
    match value {
        Evaluated::Value(value) => value.spelled(model),
        Evaluated::Prompted(prompted) => {
            let name = model.entities[prompted.value_type]
                .identifier
                .clone()
                .unwrap_or_else(|| "anonymous".to_string());
            format!("{name}@like(`{}`)", prompted.prompt)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::{Source, bind};
    use crate::workspace::File as Loaded;

    // ---- Fixtures ------------------------------------------------------------------

    /// One source of a program, parsed.
    fn source(path: &str, text: &str, uses: &[Option<&str>], origin: Origin) -> Source {
        let tokens = crate::lexer::lex(text, Some(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        let tree = crate::parser::parse(tokens, None);
        assert!(
            tree.errors.is_empty(),
            "{path} does not parse: {}",
            tree.render()
        );
        Source {
            path: path.to_string(),
            tree,
            uses: uses.iter().map(|u| u.map(str::to_string)).collect(),
            origin,
        }
    }

    /// One file, `a.lfy`, bound alone.
    fn bound(text: &str) -> Model {
        bind(vec![source("a.lfy", text, &[], Origin::Program)])
    }

    /// A prelude small enough to read: the kind data every entity is seen through, and the
    /// base data a string is read through.
    fn prelude() -> Vec<Source> {
        let entity = source(
            "lib/prelude/entity.lfy",
            "d Entity { $identifier: `The declared name` = string | undefined; \
             $definition: `Its description` = string | undefined; \
             $type: `Its type` = Entity | undefined; \
             $acceptanceCriteria: `Its criteria` = Entity[]; \
             $like: `A value the prompt describes` = (prompt: string) => Entity; \
             $test: `Adds cases` = (...cases: Entity[]) => Entity; }",
            &[],
            Origin::Library,
        );
        let values = source(
            "lib/values/base.lfy",
            "d String { $length: `How many characters` = number; }",
            &[],
            Origin::Library,
        );
        let main = source(
            "lib/main.lfy",
            "use \"./prelude/entity\"; use \"./values/base\";",
            &[
                Some("lib/prelude/entity.lfy"),
                Some("lib/values/base.lfy"),
            ],
            Origin::Prelude,
        );
        vec![entity, values, main]
    }

    /// A prelude of the builtins the interpreter performs natively: the trait one carries,
    /// and the fns of `elfie/system` a test calls.
    fn natives() -> Vec<Source> {
        let builtin = source(
            "lib/prelude/builtin.lfy",
            "trait builtin: `Performed by the compiler` {}",
            &[],
            Origin::Library,
        );
        let json = source(
            "lib/system/json.lfy",
            "use \"../prelude/builtin\";\n\
             fn parse(text: string) is builtin: `What JSON text carries` => object | undefined;\n\
             fn stringify(value: object, pretty: boolean = false) is builtin: \
             `JSON text carrying a value` => string;\n",
            &[Some("lib/prelude/builtin.lfy")],
            Origin::Library,
        );
        let file = source(
            "lib/system/file.lfy",
            "use \"../prelude/builtin\";\n\
             fn read(path: string) is builtin: `The text of a file` => string | undefined;\n\
             fn write(path: string, text: string) is builtin: `Whether text was written` \
             => boolean;\n\
             fn exists(path: string) is builtin: `Whether something is at a path` => boolean;\n",
            &[Some("lib/prelude/builtin.lfy")],
            Origin::Library,
        );
        let process = source(
            "lib/system/process.lfy",
            "use \"../prelude/builtin\";\n\
             fn run(command: string) is builtin: `Runs a command and waits for it` => object;\n\
             fn environment(name: string) is builtin: `The value of a variable` \
             => string | undefined;\n",
            &[Some("lib/prelude/builtin.lfy")],
            Origin::Library,
        );
        let main = source(
            "lib/main.lfy",
            "use \"./prelude/builtin\";\nuse \"./system/json\";\n\
             use \"./system/file\";\nuse \"./system/process\";\n",
            &[
                Some("lib/prelude/builtin.lfy"),
                Some("lib/system/json.lfy"),
                Some("lib/system/file.lfy"),
                Some("lib/system/process.lfy"),
            ],
            Origin::Prelude,
        );
        vec![builtin, json, file, process, main]
    }

    /// A prelude of what declaring a target needs: the trait a builtin carries, the `layer`
    /// fn of `elfie/prelude/trait`, the `Target` data a const holds with the function that
    /// gives a record its type, the roles its slots take, and the enums `knowledge` and
    /// `commands` name.
    fn targets() -> Vec<Source> {
        let builtin = source(
            "lib/prelude/builtin.lfy",
            "trait builtin: `Performed by the compiler` {}",
            &[],
            Origin::Library,
        );
        let traits = source(
            "lib/prelude/trait.lfy",
            "use \"./builtin\";\n\
             fn layer(subject: trait, ...arguments: string[]) is builtin: \
             `A trait chosen as a layer of a target` => object;\n",
            &[Some("lib/prelude/builtin.lfy")],
            Origin::Library,
        );
        let criteria = source(
            "lib/criteria/main.lfy",
            "enum KnowledgeKind: `What a piece of knowledge is` {\n\
               reference = `documentation to read: a language reference, an API, a guide`,\n\
               example = `working code to imitate`,\n\
             }\n\
             enum Operation: `What a command is for` {\n\
               build = `build: compile or type-check the outputs`,\n\
               test = `test: run the tests`,\n\
             }\n",
            &[],
            Origin::Library,
        );
        let target = source(
            "lib/target/main.lfy",
            "use \"../prelude/builtin\";\n\
             trait target: `Anything else the compiler is told` {}\n\
             trait targetLanguage: `What code is written in` {}\n\
             trait targetLayout: `Where files go` {}\n\
             d Target is builtin: `One artifact the project is built into` {\n\
               $output: `Where everything it writes goes` = string;\n\
               $language: `What its code is written in` = trait;\n\
               $layout: `Where its files go` = trait;\n\
               $layers: `Anything else the compiler is told` = trait[];\n\
             }\n\
             function targetOf(target: Target): `Gives a record its type` -> Target {\n\
               return target;\n\
             }\n",
            &[Some("lib/prelude/builtin.lfy")],
            Origin::Library,
        );
        let main = source(
            "lib/main.lfy",
            "use \"./prelude/builtin\";\nuse \"./prelude/trait\";\n\
             use \"./criteria/main\";\nuse \"./target/main\";\n",
            &[
                Some("lib/prelude/builtin.lfy"),
                Some("lib/prelude/trait.lfy"),
                Some("lib/criteria/main.lfy"),
                Some("lib/target/main.lfy"),
            ],
            Origin::Prelude,
        );
        vec![builtin, traits, criteria, target, main]
    }

    /// That prelude, then one file `a.lfy` of the program, as the expand pass leaves it.
    fn expanded_with_targets(text: &str) -> Model {
        let mut sources = targets();
        sources.push(source("a.lfy", text, &[], Origin::Program));
        expand(declared(sources))
    }

    /// The prelude, then one file `a.lfy` of the program.
    fn bound_with_prelude(text: &str) -> Model {
        let mut sources = prelude();
        sources.push(source("a.lfy", text, &[], Origin::Program));
        bind(sources)
    }

    /// A model as the declare pass leaves it: everything the expand pass adds, taken back
    /// out, so that [`expand`] is handed what the binder hands it.
    fn declared(sources: Vec<Source>) -> Model {
        let mut model = bind(sources);
        for entity in &mut model.entities {
            entity.traits.clear();
            entity.acceptance_criteria.clear();
            entity.values.clear();
            entity.tests.clear();
            entity.knowledge.clear();
            entity.commands.clear();
            entity.targets.clear();
            if let EntityKind::Trait {
                entities,
                extenders,
                ..
            } = &mut entity.kind
            {
                entities.clear();
                extenders.clear();
            }
        }
        // A member an applied trait copied into a receiver's scope is declared somewhere
        // else; the receiver's own members are declared inside the node that owns the scope.
        let scopes = model.scopes.len();
        for scope in 0..scopes {
            let owner = model.scopes[scope].owner;
            if owner.file == usize::MAX {
                continue;
            }
            let kept: Vec<SymbolId> = model.scopes[scope]
                .symbols
                .iter()
                .copied()
                .filter(|&symbol| {
                    let record = &model.symbols[symbol];
                    if !matches!(record.kind, SymbolKind::Member | SymbolKind::EnumMember)
                        || record.node.file == usize::MAX
                    {
                        return true;
                    }
                    within(&model, record.node, owner)
                })
                .collect();
            model.scopes[scope].symbols = kept;
        }
        model.problems.clear();
        model
    }

    /// A workspace of these sources, with no package and no target, holding the model as
    /// [`expand`] leaves it: what the binder hands [`lower`].
    fn workspace_of(sources: Vec<Source>) -> Workspace {
        let files: Vec<Loaded> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| Loaded {
                path: source.path.clone(),
                package: None,
                source: index,
            })
            .collect();
        let model = expand(declared(sources));
        Workspace {
            root: PathBuf::from("."),
            name: "fixture".to_string(),
            source_directory: "def".to_string(),
            files,
            model,
            packages: Vec::new(),
            targets: Vec::new(),
            problems: Vec::new(),
            overlays: BTreeMap::new(),
        }
    }

    /// One file `a.lfy` as a workspace.
    fn workspace(text: &str) -> Workspace {
        workspace_of(vec![source("a.lfy", text, &[], Origin::Program)])
    }

    /// The first node of a file satisfying a rule.
    fn first<R: GrammarRule>(model: &Model, file: FileId, rule: R) -> NodeRef {
        model
            .descendants(NodeRef { file, index: 0 })
            .into_iter()
            .find(|&node| model.is(node, rule))
            .unwrap_or_else(|| panic!("no {}", rule.identifier()))
    }

    /// The node after the given one satisfying a rule: the second `Name`, and so on.
    fn nth<R: GrammarRule>(model: &Model, file: FileId, rule: R, at: usize) -> NodeRef {
        let mut found = model
            .descendants(NodeRef { file, index: 0 })
            .into_iter()
            .filter(|&node| model.is(node, rule));
        found
            .nth(at)
            .unwrap_or_else(|| panic!("no {} at {at}", rule.identifier()))
    }

    fn entity_named(model: &Model, name: &str) -> EntityId {
        model
            .entities
            .iter()
            .position(|entity| entity.identifier.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("{name} is declared"))
    }

    /// The value expression of a declaration: what follows its `Declared`.
    fn value_of(model: &Model, declaration: NodeRef) -> NodeRef {
        model
            .child_nodes(declaration)
            .into_iter()
            .find(|&child| !model.is(child, E::Declared))
            .expect("a declared value")
    }

    /// Every node of a lowered tree, the root first.
    fn every(node: &LoweredNode) -> Vec<&LoweredNode> {
        let mut out = vec![node];
        for child in node.nodes() {
            out.extend(every(child));
        }
        out
    }

    // ---- phaseOf -------------------------------------------------------------------

    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:e296a93f89da71bf1df9226c1abb709c149b656286c7a3d31029d1a5b088a47a
    #[test]
    fn a_trait_declaration_runs_at_compile_time() {
        let model = bound("trait t {}\n");
        let declaration = first(&model, 0, S::TraitDeclaration);
        assert_eq!(phase_of(&model, declaration), Phase::Compile);
    }

    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:18bed760f0d522c88d3e7716bb2d269bdb11b37a55d0d21f716a1eb4c22a2ae3
    #[test]
    fn a_statement_directly_in_a_file_that_declares_nothing_runs_at_compile_time() {
        let model = bound("trait scoped {}\nd SourceFile {}\nscoped.apply(SourceFile);\n");
        let statement = first(&model, 0, S::ExpressionStatement);
        assert_eq!(phase_of(&model, statement), Phase::Compile);
        // The apply call inside it is compile time in its own right.
        let call = first(&model, 0, E::Call);
        assert_eq!(compile_time_call(&model, call), Some(CompileCall::Apply));
        assert_eq!(phase_of(&model, call), Phase::Compile);
    }

    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:c97e9f5136290c1e11a2756a6ba9e7ca228e99d5b6985ad7ea51c45b628a9249
    #[test]
    fn the_body_of_a_written_function_is_kept_for_runtime() {
        let model = bound("function f() -> number {\n  return 1;\n}\n");
        let statement = first(&model, 0, S::Return);
        assert_eq!(phase_of(&model, statement), Phase::Runtime);
        // The declaration that holds it is runtime too.
        let declaration = first(&model, 0, S::FunctionDeclaration);
        assert_eq!(phase_of(&model, declaration), Phase::Runtime);
    }

    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:91de1d9e717023c54a963aa58631c2ee9d223c44cbf41ccbf14570dbd4ebbb86
    #[test]
    fn a_where_runs_at_compile_time() {
        let model = bound("fn f() => number {\n  where (`a`) -> `b`;\n}\n");
        let statement = first(&model, 0, S::Where);
        assert_eq!(phase_of(&model, statement), Phase::Compile);
        // A declaration whose body is compile time is itself runtime.
        let declaration = first(&model, 0, S::AgentFunctionDeclaration);
        assert_eq!(phase_of(&model, declaration), Phase::Runtime);
    }

    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:3e3bc11466542f33b6a34c37871336e917685651122b0fb123a4f7b811c4de89
    #[test]
    fn a_file_level_const_is_runtime() {
        let model = bound("const c = 1;\n");
        let declaration = first(&model, 0, S::VariableDeclaration);
        assert_eq!(phase_of(&model, declaration), Phase::Runtime);
    }

    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:e296a93f89da71bf1df9226c1abb709c149b656286c7a3d31029d1a5b088a47a
    #[test]
    fn the_body_of_a_data_and_the_clauses_of_a_declaration_run_at_compile_time() {
        let model = bound("trait t {}\nd A is t {\n  $x: `A member` = string;\n}\n");
        let is_clause = first(&model, 0, E::IsClause);
        assert_eq!(phase_of(&model, is_clause), Phase::Compile);
        let block = first(&model, 0, S::Block);
        assert_eq!(phase_of(&model, block), Phase::Compile);
        // And so does the member declaration inside it.
        let member = first(&model, 0, E::Assignment);
        assert_eq!(phase_of(&model, member), Phase::Compile);
    }

    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:e296a93f89da71bf1df9226c1abb709c149b656286c7a3d31029d1a5b088a47a
    #[test]
    fn an_extends_clause_an_ace_and_a_with_run_at_compile_time() {
        let model = bound(
            "trait base {}\ntrait t extends base {}\nd A {}\nace const c = 1;\n\
             with A {\n  where (`a`) -> `b`;\n}\n",
        );
        assert_eq!(phase_of(&model, first(&model, 0, E::ExtendsClause)), Phase::Compile);
        assert_eq!(phase_of(&model, first(&model, 0, S::Ace)), Phase::Compile);
        assert_eq!(phase_of(&model, first(&model, 0, S::With)), Phase::Compile);
        // The declaration the ace holds runs at compile time with it.
        let ace = first(&model, 0, S::Ace);
        let held = model.child_nodes(ace).into_iter().next().expect("a statement");
        assert_eq!(phase_of(&model, held), Phase::Compile);
    }

    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:e296a93f89da71bf1df9226c1abb709c149b656286c7a3d31029d1a5b088a47a
    #[test]
    fn a_call_of_add_on_criteria_and_of_test_on_a_context_run_at_compile_time() {
        let model = bound(
            "fn f(): `Does it` => number {\n  @acceptanceCriteria.add({ behavior = `holds` });\n\
             \n  @test({ input = [], expect = 1 });\n}\n",
        );
        // Knowledge, commands, and targets are added the same way and run the same way.
        let others = bound(
            "d A {\n  @knowledge.add({ topic = `a guide` });\n\
             \n  @commands.add({ operation = Operation.build });\n  @targets.add(A);\n}\n",
        );
        let added: Vec<Option<CompileCall>> = others
            .descendants(NodeRef { file: 0, index: 0 })
            .into_iter()
            .filter(|&node| others.is(node, E::Call))
            .map(|call| {
                assert_eq!(phase_of(&others, call), Phase::Compile);
                compile_time_call(&others, call)
            })
            .collect();
        assert_eq!(added, vec![Some(CompileCall::Add); 3], "{added:?}");
        let calls: Vec<NodeRef> = model
            .descendants(NodeRef { file: 0, index: 0 })
            .into_iter()
            .filter(|&node| model.is(node, E::Call))
            .collect();
        let kinds: Vec<Option<CompileCall>> = calls
            .iter()
            .map(|&call| compile_time_call(&model, call))
            .collect();
        assert!(kinds.contains(&Some(CompileCall::Add)), "{kinds:?}");
        assert!(kinds.contains(&Some(CompileCall::Test)), "{kinds:?}");
        for call in calls {
            assert_eq!(phase_of(&model, call), Phase::Compile);
        }
    }

    // @lfy def/interpret/main.lfy:phaseOf#phaseOf:phaseOf:5357c505ea2cc6797f85156d4c4bba6d26483c5ab8599dbf3d34660ece727fa0
    #[test]
    fn a_template_reference_in_a_definition_runs_at_compile_time() {
        let model = bound("d A {}\nfunction f(): `Reads [[A]]` -> number {\n  return 1;\n}\n");
        let reference = first(&model, 0, E::TemplateReference);
        assert_eq!(phase_of(&model, reference), Phase::Compile);
        // The function it describes is still kept.
        let declaration = first(&model, 0, S::FunctionDeclaration);
        assert_eq!(phase_of(&model, declaration), Phase::Runtime);
    }

    // ---- expand --------------------------------------------------------------------

    // @lfy def/interpret/main.lfy:expand#expand:expand:d2cc081cb564b1648825d86ca4a9c1ac0367fd6b11ab493532a0570be1c2f407
    #[test]
    fn a_trait_declared_after_the_entity_that_carries_it_is_still_applied() {
        let model = declared(vec![source(
            "a.lfy",
            "d A is t {}\ntrait t {\n  $x: `d` = string;\n}\n",
            &[],
            Origin::Program,
        )]);
        let model = expand(model);
        let a = entity_named(&model, "A");
        let t = entity_named(&model, "t");
        assert!(model.entities[a].has_trait(t), "A carries t");
        assert!(
            member_symbol(&model, a, "x").is_some(),
            "A has the member x"
        );
        assert_eq!(model.entities[t].entities(), [a]);
        assert!(model.problems.is_empty(), "{:?}", model.problems);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:e95ca6eb34f6fd3768a7222d4bcf16ac081d3e2608c5424733c4c8793a894920
    #[test]
    fn reading_what_a_trait_was_applied_to_waits_for_every_file_that_can_apply_it() {
        let a = source(
            "a.lfy",
            "d Entity {}\ntrait p {}\ntrait list(...items: Entity[]) {\n  .items = items;\n}\n\
             d X is p {}\nd All is list(...p@entities) {}\n",
            &[],
            Origin::Program,
        );
        let b = source(
            "b.lfy",
            "use \"./a\";\nd Y is p {}\n",
            &[Some("a.lfy")],
            Origin::Program,
        );
        let model = expand(declared(vec![a, b]));
        let (x, y, all) = (
            entity_named(&model, "X"),
            entity_named(&model, "Y"),
            entity_named(&model, "All"),
        );
        let items = model.entities[all].value("items").expect("All holds items");
        assert_eq!(
            items,
            &Interim::List(vec![Interim::Entity(x), Interim::Entity(y)]),
            "the value of All holds X and Y, because reading p@entities waited for b.lfy"
        );
        assert!(model.problems.is_empty(), "{:?}", model.problems);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:a553d1617c777bb329d3e59b4063f9acee7795a53ddf5cc2ec6afbc82796fe71
    #[test]
    fn a_call_of_a_fn_that_carries_no_builtin_has_nothing_to_run() {
        let model = declared(vec![source(
            "a.lfy",
            "fn f(): `Gives a number` => number {}\nace const c = f();\n",
            &[],
            Origin::Program,
        )]);
        let mut model = expand(model);
        assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
        assert!(
            model.problems[0].message.contains("f is a fn"),
            "{}",
            model.problems[0].message
        );
        let call = first(&model, 0, E::Call);
        assert_eq!(model.problems[0].node, call);
        // And nothing came of the call, so `c` holds no value.
        let declaration = first(&model, 0, S::VariableDeclaration);
        let value = value_of(&model, declaration);
        assert_eq!(evaluate(&mut model, value), None);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
    #[test]
    fn a_compile_time_call_of_a_fn_carrying_builtin_is_performed_natively() {
        let directory = std::env::temp_dir().join("elfie-interpret-builtins");
        std::fs::create_dir_all(&directory).expect("a directory to write in");
        let path = directory
            .join("hello.txt")
            .to_string_lossy()
            .replace('\\', "/");
        let program = format!(
            "ace const written = write(\"{path}\", \"hello\");\n\
             ace const back = read(\"{path}\");\n\
             ace const missing = read(\"{path}.none\");\n\
             ace const there = exists(\"{path}\");\n\
             ace const json = stringify(parse('{{\"a\": [1, 2]}}'));\n\
             ace const shell = run(\"echo hi\").stdout;\n\
             ace const variable = environment(\"PATH\");\n"
        );
        let mut sources = natives();
        sources.push(source("a.lfy", &program, &[], Origin::Program));
        let mut model = expand(declared(sources));
        assert!(model.problems.is_empty(), "{:?}", model.problems);
        let file = model.file("a.lfy").expect("the program file");
        let values: Vec<NodeRef> = (0..7)
            .map(|at| value_of(&model, nth(&model, file, S::VariableDeclaration, at)))
            .collect();
        let text = |held: &str| Some(Evaluated::Value(Value::String(held.to_string())));
        let truth = Some(Evaluated::Value(Value::Boolean(true)));
        assert_eq!(evaluate(&mut model, values[0]), truth, "write");
        assert_eq!(evaluate(&mut model, values[1]), text("hello"), "read");
        assert_eq!(
            evaluate(&mut model, values[2]),
            Some(Evaluated::Value(Value::Undefined)),
            "read of nothing"
        );
        assert_eq!(evaluate(&mut model, values[3]), truth, "exists");
        assert_eq!(
            evaluate(&mut model, values[4]),
            text("{\"a\":[1,2]}"),
            "parse and stringify"
        );
        assert_eq!(evaluate(&mut model, values[5]), text("hi\n"), "run");
        assert_eq!(
            evaluate(&mut model, values[6]),
            std::env::var("PATH").ok().map(|held| {
                Evaluated::Value(Value::String(held))
            }),
            "environment"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
    #[test]
    fn an_operation_is_performed_natively_only_when_its_data_carries_builtin() {
        // The dispatch is on the trait the data carries, never on the method's name alone.
        let performed = |carries: &str| -> Option<Evaluated> {
            let values = source(
                "lib/values/base.lfy",
                &format!(
                    "trait builtin: `Performed by the compiler` {{}}\n\
                     d List<T> {carries}{{ $join: `The items' text` \
                     = (separator: string) => string; }}\n"
                ),
                &[],
                Origin::Library,
            );
            let main = source(
                "lib/main.lfy",
                "use \"./values/base\";",
                &[Some("lib/values/base.lfy")],
                Origin::Prelude,
            );
            let program = source(
                "a.lfy",
                "ace const joined = [1, 2].join(\"-\");\n",
                &[],
                Origin::Program,
            );
            let mut model = expand(declared(vec![values, main, program]));
            let file = model.file("a.lfy").expect("the program file");
            let value = value_of(&model, first(&model, file, S::VariableDeclaration));
            evaluate(&mut model, value)
        };
        assert_eq!(
            performed("is builtin "),
            Some(Evaluated::Value(Value::String("1-2".to_string())))
        );
        assert_eq!(performed(""), None);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:0c816239c7adfd2752ecb13151d98fd53744cd8d23e3eec6f1df8da00917419d
    #[test]
    fn a_trait_applied_to_a_file_reaches_the_file_and_one_applied_by_a_call_its_argument() {
        let model = declared(vec![source(
            "a.lfy",
            "trait t {\n  $x: `d` = string;\n}\nd A {}\nt.apply(@);\nt.apply(A);\n",
            &[],
            Origin::Program,
        )]);
        let model = expand(model);
        let t = entity_named(&model, "t");
        let a = entity_named(&model, "A");
        let file = model.file_entities[0];
        // A `Current` with no member name at the top level of a file is the file's entity.
        // @lfy def/interpret/main.lfy:expand#expand:expand:0c816239c7adfd2752ecb13151d98fd53744cd8d23e3eec6f1df8da00917419d
        assert!(model.entities[file].has_trait(t), "the file carries t");
        assert!(model.entities[a].has_trait(t), "A carries t");
        assert_eq!(model.entities[t].entities(), [file, a]);
        assert!(model.problems.is_empty(), "{:?}", model.problems);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:07e6e2fbd3e0d55bead798eb2b27ac9dded4a9209d2690ca888ce69ee0b1bddc
    #[test]
    fn an_extended_trait_is_applied_too_and_never_joins_the_entities_of_the_one_it_extends() {
        let model = declared(vec![source(
            "a.lfy",
            "trait base(name: string) {\n  .label = name;\n}\n\
             trait t extends base(`b`) {}\nd A is t {}\n",
            &[],
            Origin::Program,
        )]);
        let model = expand(model);
        let (base, t, a) = (
            entity_named(&model, "base"),
            entity_named(&model, "t"),
            entity_named(&model, "A"),
        );
        assert!(model.entities[a].has_trait(t), "A carries t");
        assert!(model.entities[a].has_trait(base), "A carries base too");
        assert_eq!(
            model.entities[a].value("label"),
            Some(&Interim::String("b".to_string())),
            "the extended trait's arguments were evaluated from the extending trait"
        );
        // An extender is never one of the entities of what it extends.
        // @lfy def/interpret/main.lfy:expand#expand:expand:07e6e2fbd3e0d55bead798eb2b27ac9dded4a9209d2690ca888ce69ee0b1bddc
        assert_eq!(model.entities[base].entities(), [a]);
        assert_eq!(model.entities[base].extenders(), [t]);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:e3fd8ad8b33b19a70c578bfce293824df8a73ed5a0ca91556d8acebf48fe8914
    #[test]
    fn the_most_recently_applied_trait_wins_a_member_two_traits_declare() {
        let model = declared(vec![source(
            "a.lfy",
            "trait first {\n  $x: `From first` = string;\n}\n\
             trait second {\n  $x: `From second` = number;\n}\nd A is first, second {}\n",
            &[],
            Origin::Program,
        )]);
        let model = expand(model);
        let a = entity_named(&model, "A");
        let second = entity_named(&model, "second");
        let member = member_symbol(&model, a, "x").expect("A has the member x");
        let declared_in = model
            .ancestor(model.symbols[member].node, S::TraitDeclaration)
            .expect("declared in a trait");
        assert_eq!(
            model.symbol_of(declared_in).map(|s| model.symbols[s].entity),
            Some(second)
        );
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    #[test]
    fn a_criterion_of_a_trait_body_is_the_entitys_with_the_trait_as_its_contributor() {
        let model = declared(vec![source(
            "a.lfy",
            "trait t {\n  where (`s`) -> `b`;\n}\nd A is t {\n  where (`own`) -> `o`;\n}\n",
            &[],
            Origin::Program,
        )]);
        let model = expand(model);
        let (a, t) = (entity_named(&model, "A"), entity_named(&model, "t"));
        let criteria = &model.entities[a].acceptance_criteria;
        assert_eq!(criteria.len(), 2, "{criteria:?}");
        let contributors: Vec<EntityId> = criteria.iter().map(|c| c.contributor).collect();
        assert!(contributors.contains(&a) && contributors.contains(&t), "{contributors:?}");
        let from_trait = criteria
            .iter()
            .find(|c| c.contributor == t)
            .expect("one from the trait");
        assert_eq!(from_trait.situation.as_deref(), Some(["s".to_string()].as_slice()));
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:c0d9b076f61371a9e50538ed00b2dccc6dff2b97a5ee0229e0e08db54f2e166b
    #[test]
    fn an_apply_whose_first_argument_is_an_alternation_list_reaches_every_item_of_it() {
        let model = declared(vec![source(
            "a.lfy",
            "d Entity {}\ntrait alternationList(...items: Entity[]) {}\n\
             d One {}\nd Two {}\nd Either is alternationList(One, Two) {}\n\
             trait mark {}\nmark.apply(Either);\n",
            &[],
            Origin::Program,
        )]);
        let model = expand(model);
        let (one, two, either, mark) = (
            entity_named(&model, "One"),
            entity_named(&model, "Two"),
            entity_named(&model, "Either"),
            entity_named(&model, "mark"),
        );
        assert_eq!(model.entities[mark].entities(), [one, two]);
        assert!(!model.entities[either].has_trait(mark), "not the list itself");
        assert!(model.problems.is_empty(), "{:?}", model.problems);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:8fd627207ea86becebc501f08576d358a6e4d446addee292cd0f1634bb4b9beb
    #[test]
    fn a_name_a_statement_needs_that_never_resolves_is_named_once_the_rounds_have_stopped() {
        let model = declared(vec![source(
            "a.lfy",
            "trait t {\n  $x: `d` = string;\n}\nd A is missing {}\nd B is t {}\n",
            &[],
            Origin::Program,
        )]);
        let model = expand(model);
        assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
        assert_eq!(
            model.problems[0].message, "missing does not resolve",
            "{:?}",
            model.problems[0]
        );
        // The statements whose names do resolve still ran.
        let b = entity_named(&model, "B");
        assert!(member_symbol(&model, b, "x").is_some());
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:9f92ae8a6753e1a25414725ffb95c7356c3fb1021cf313cd36c68c65c0e8b6a4
    #[test]
    fn reading_what_a_trait_was_applied_to_waits_for_a_statement_that_applies_it_by_extension() {
        // `X` never names `base`; it carries it only because `sub` extends it, and the
        // reader still waits for that statement.
        let model = declared(vec![source(
            "a.lfy",
            "d Entity {}\ntrait base {}\ntrait sub extends base {}\n\
             trait list(...items: Entity[]) {\n  .items = items;\n}\n\
             d All is list(...base@entities) {}\nd X is sub {}\n",
            &[],
            Origin::Program,
        )]);
        let model = expand(model);
        let (x, all) = (entity_named(&model, "X"), entity_named(&model, "All"));
        assert_eq!(
            model.entities[all].value("items"),
            Some(&Interim::List(vec![Interim::Entity(x)])),
            "All read base@entities after X carried base through the extends chain"
        );
        assert!(model.problems.is_empty(), "{:?}", model.problems);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:1ee5f4b0a08eba34d8c061dfcdbf152c3e11233a665f48c77f2394490ee76a30
    #[test]
    fn a_statement_waiting_on_a_trait_nothing_can_apply_any_more_is_named_and_never_runs() {
        // Two statements each wait to read what the other's trait was applied to, so
        // neither can ever run.
        let model = declared(vec![source(
            "a.lfy",
            "d Entity {}\ntrait p {}\ntrait q {}\n\
             trait list(...items: Entity[]) {\n  .items = items;\n}\n\
             d One is p, list(...q@entities) {}\nd Two is q, list(...p@entities) {}\n",
            &[],
            Origin::Program,
        )]);
        let model = expand(model);
        assert_eq!(model.problems.len(), 2, "{:?}", model.problems);
        assert!(
            model
                .problems
                .iter()
                .all(|problem| problem.message.contains("waits to read what")),
            "{:?}",
            model.problems
        );
        // Neither of them ran, so neither holds the items it was waiting for.
        let one = entity_named(&model, "One");
        let two = entity_named(&model, "Two");
        assert_eq!(model.entities[one].value("items"), None);
        assert_eq!(model.entities[two].value("items"), None);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:bc5da8aae1f9989cfd3e5791911af8af36ce78edef0fa9da9594fd85e7449b18
    #[test]
    fn running_a_statement_changes_only_the_model_and_never_a_tree() {
        let sources = vec![source(
            "a.lfy",
            "trait t {\n  $x: `d` = string;\n}\nd A is t {}\n",
            &[],
            Origin::Program,
        )];
        let before = declared(sources.clone());
        let trees: Vec<_> = before
            .sources
            .iter()
            .map(|source| source.tree.clone())
            .collect();
        let after = expand(before);
        let kept: Vec<_> = after
            .sources
            .iter()
            .map(|source| source.tree.clone())
            .collect();
        assert_eq!(trees, kept, "no tree was changed");
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:bbad3f48d6df69b629c7de7d6de1d28945311a91b23745b50defb5cbbfdd328f
    // @lfy def/interpret/main.lfy:expand#expand:expand:da7602e2b3243080f2623c7ec36d5389af778549e7da59559ebadf2d10ec6e0a
    // @lfy def/interpret/main.lfy:expand#expand:expand:24896a9f182aab4cba64abb17efdb629e1de1a7ac24c9edcdbc3fa14873d813b
    // @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
    // @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
    #[test]
    fn an_ace_const_holding_a_target_carries_every_layer_in_guidance_order() {
        let model = expanded_with_targets(
            "trait l extends targetLanguage {\n  .markerComment = \"#\";\n}\n\
             trait f(root: string) extends targetLayout {\n  \
             where (`built`) -> `it is under {{root}}`;\n}\n\
             ace const t = targetOf({ output = \"o\", language = l, layout = f.layer(\"o\") });\n\
             d A {\n  @targets.add(t);\n}\n",
        );
        assert!(model.problems.is_empty(), "{:?}", model.problems);
        let t = entity_named(&model, "t");
        let l = entity_named(&model, "l");
        let f = entity_named(&model, "f");
        // The layout is a layer before the language, as guidance order has it.
        let layers: Vec<EntityId> = model.entities[t]
            .traits
            .iter()
            .filter(|applied| matches!(applied.source, AppliedSource::Apply(_)))
            .map(|applied| applied.entity)
            .collect();
        assert_eq!(layers, vec![f, l], "the layout, then the language");
        // The layer's criterion, with its template value rendered from its argument.
        let criterion = model.entities[t]
            .acceptance_criteria
            .iter()
            .find(|criterion| criterion.contributor == f)
            .expect("a criterion contributed by the layout");
        assert_eq!(
            criterion.behavior,
            Some(vec!["it is under o".to_string()]),
            "{criterion:?}"
        );
        // And the language's value.
        assert_eq!(
            model.entities[t].value("markerComment"),
            Some(&Interim::String("#".to_string()))
        );
        // The data that named the target holds it.
        let a = entity_named(&model, "A");
        assert_eq!(model.entities[a].targets, vec![t]);
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:da7602e2b3243080f2623c7ec36d5389af778549e7da59559ebadf2d10ec6e0a
    #[test]
    fn a_trait_in_two_slots_with_equal_arguments_is_one_layer_at_its_first_place() {
        let model = expanded_with_targets(
            "trait both extends targetLayout {}\n\
             trait l extends targetLanguage {}\n\
             ace const t = targetOf({ output = \"o\", language = l, layout = both, \
             layers = [both] });\n",
        );
        assert!(model.problems.is_empty(), "{:?}", model.problems);
        let t = entity_named(&model, "t");
        let both = entity_named(&model, "both");
        let layers: Vec<EntityId> = model.entities[t]
            .traits
            .iter()
            .filter(|applied| matches!(applied.source, AppliedSource::Apply(_)))
            .map(|applied| applied.entity)
            .collect();
        // `layers` is the first slot of guidance order, so that is where it stands, once.
        assert_eq!(layers.iter().filter(|&&held| held == both).count(), 1);
        assert_eq!(layers.first(), Some(&both), "{layers:?}");
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:f2da259bc931b16b67600269961004bc2fdf0d760814936c7268ae3ed27c6981
    #[test]
    fn a_slot_holding_neither_a_trait_nor_a_layer_of_one_names_the_slot() {
        let model = expanded_with_targets(
            "trait l extends targetLanguage {}\n\
             ace const t = targetOf({ output = \"o\", language = l, layout = \"nothing\" });\n",
        );
        assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
        assert!(
            model.problems[0].message.contains("the layout of this target"),
            "{}",
            model.problems[0].message
        );
        // The language was still applied; only the layout was not.
        let t = entity_named(&model, "t");
        let l = entity_named(&model, "l");
        assert!(model.entities[t].has_trait(l));
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
    #[test]
    fn a_layer_given_a_different_number_of_arguments_names_the_trait() {
        let model = expanded_with_targets(
            "trait l(name: string, root: string) extends targetLanguage {}\n\
             trait f extends targetLayout {}\n\
             ace const t = targetOf({ output = \"o\", language = l.layer(\"a\"), layout = f });\n",
        );
        assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
        assert!(
            model.problems[0].message.contains("l takes 2 arguments as a layer, not 1"),
            "{}",
            model.problems[0].message
        );
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    #[test]
    fn knowledge_and_commands_are_appended_with_the_contributor_a_criterion_would_have() {
        let model = expanded_with_targets(
            "trait given {\n  \
             @knowledge.add({ topic = `the trait's guide`, kind = KnowledgeKind.example, \
             source = \"docs/t.md\" });\n  \
             @commands.add({ operation = Operation.test, line = \"cargo test\" });\n}\n\
             d A is given {\n  \
             @knowledge.add({ topic = `a guide`, kind = KnowledgeKind.reference, \
             source = \"docs/a.md\", quote = true });\n}\n",
        );
        assert!(model.problems.is_empty(), "{:?}", model.problems);
        let a = entity_named(&model, "A");
        let given = entity_named(&model, "given");
        // The `is` clause runs before the body, so the trait's item is appended first;
        // which order the member is read in is `Entity.knowledge`'s business.
        let topics: Vec<(&str, EntityId)> = model.entities[a]
            .knowledge
            .iter()
            .map(|item| (item.topic.as_str(), item.contributor))
            .collect();
        assert_eq!(
            topics,
            vec![("the trait's guide", given), ("a guide", a)],
            "one from the trait, with the trait as its contributor, and one of its own"
        );
        let own = &model.entities[a].knowledge[1];
        assert_eq!(own.kind, KnowledgeKind::Reference);
        assert_eq!(own.source, "docs/a.md");
        assert_eq!(own.quote, Some(true));
        assert_eq!(model.entities[a].knowledge[0].kind, KnowledgeKind::Example);
        // The command the trait added reaches the entity with the trait as its contributor.
        assert_eq!(
            model.entities[a].commands,
            vec![Command {
                operation: Operation::Test,
                line: Some("cargo test".to_string()),
                contributor: given,
            }]
        );
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:7f7104a1337b2505a318c15f0b6be1af739e45b47f0cedf1d221cb3aa0b55877
    // @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
    #[test]
    fn an_argument_that_names_no_ace_const_holding_a_target_is_named_and_adds_nothing() {
        let model = expanded_with_targets(
            "d A {\n  \
             @knowledge.add({ topic = `a guide`, kind = KnowledgeKind.reference, \
             source = \"docs/a.md\", quote = true });\n  @targets.add(A);\n}\n",
        );
        let a = entity_named(&model, "A");
        assert_eq!(model.entities[a].knowledge.len(), 1);
        assert_eq!(model.entities[a].knowledge[0].topic, "a guide");
        assert_eq!(model.entities[a].knowledge[0].contributor, a);
        assert!(model.entities[a].targets.is_empty());
        assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
        assert!(
            model.problems[0].message.contains("names no target"),
            "{}",
            model.problems[0].message
        );
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:8159e82000937ae0a26519fa34e1579b5f6a3bcd6325987ebc1729fd11812494
    #[test]
    fn the_same_target_added_twice_is_named_and_appended_once() {
        let model = expanded_with_targets(
            "trait l extends targetLanguage {}\ntrait f extends targetLayout {}\n\
             ace const t = targetOf({ output = \"o\", language = l, layout = f });\n\
             d A {\n  @targets.add(t).add(t);\n}\n",
        );
        let t = entity_named(&model, "t");
        let a = entity_named(&model, "A");
        assert_eq!(model.entities[a].targets, vec![t]);
        assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
        assert!(
            model.problems[0].message.contains("added twice"),
            "{}",
            model.problems[0].message
        );
    }

    // @lfy def/interpret/main.lfy:expand#expand:expand:52171aea8ab1474de54983df55c484fadc588bd0d6e58e36b25e18a182ea17bc
    #[test]
    fn a_target_added_to_a_member_says_it_is_built_with_its_owner() {
        let model = expanded_with_targets(
            "trait l extends targetLanguage {}\ntrait f extends targetLayout {}\n\
             ace const t = targetOf({ output = \"o\", language = l, layout = f });\n\
             d A {\n  $x: `A member` = string;\n}\n\
             with A$x {\n  @targets.add(t);\n}\n",
        );
        let x = model
            .entities
            .iter()
            .position(|entity| {
                entity.identifier.as_deref() == Some("x")
                    && matches!(entity.kind, EntityKind::Member)
            })
            .expect("the member x");
        assert!(model.entities[x].targets.is_empty());
        assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
        assert!(
            model.problems[0].message.contains("built with its owner"),
            "{}",
            model.problems[0].message
        );
    }

    // ---- evaluate ------------------------------------------------------------------

    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:655f8825c2e70d824ae70edabe7443b7721c22be55823451930b947ed3672fc9
    #[test]
    fn an_arithmetic_expression_has_its_value_at_compile_time() {
        let mut model = bound("ace const c = 1 + 2;\n");
        let declaration = first(&model, 0, S::VariableDeclaration);
        let value = value_of(&model, declaration);
        assert_eq!(
            evaluate(&mut model, value),
            Some(Evaluated::Value(Value::Number(3.0)))
        );
    }

    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:2da60593210808873570bc274bbe86668ab461197169994b03a18e4b09823ed7
    #[test]
    fn a_layer_other_than_value_is_read_from_the_model() {
        let mut model = bound("d A {}\nconst n = A@identifier;\n");
        let declaration = nth(&model, 0, S::VariableDeclaration, 0);
        let value = value_of(&model, declaration);
        assert_eq!(
            evaluate(&mut model, value),
            Some(Evaluated::Value(Value::String("A".to_string())))
        );
    }

    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:2e1806da61d20fb7c02967b5644f3dedb864c40b7e7efe3474c52f3622d8e34b
    #[test]
    fn a_dereference_is_read_from_the_model() {
        let mut model = bound("d A {}\nconst n = (&A)@identifier;\n");
        let declaration = nth(&model, 0, S::VariableDeclaration, 0);
        let value = value_of(&model, declaration);
        assert_eq!(
            evaluate(&mut model, value),
            Some(Evaluated::Value(Value::String("A".to_string())))
        );
    }

    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:2e1806da61d20fb7c02967b5644f3dedb864c40b7e7efe3474c52f3622d8e34b
    #[test]
    fn the_previous_statement_is_read_from_the_model() {
        let mut model = bound("d A {}\nconst n = ^^@identifier;\n");
        let declaration = nth(&model, 0, S::VariableDeclaration, 0);
        let value = value_of(&model, declaration);
        assert_eq!(
            evaluate(&mut model, value),
            Some(Evaluated::Value(Value::String("A".to_string())))
        );
    }

    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:be4ebeec3fb42e5b9a113fd744d8da7f2b4767acf2f76bd04f0a11ba732663cb
    #[test]
    fn a_call_of_like_gives_a_prompted_of_the_entitys_type() {
        let mut model = bound_with_prelude("const s = string@like(`a word`);\n");
        let file = model.file("a.lfy").expect("the program file");
        let declaration = first(&model, file, S::VariableDeclaration);
        let value = value_of(&model, declaration);
        let string = entity_named(&model, "String");
        assert_eq!(
            evaluate(&mut model, value),
            Some(Evaluated::Prompted(Prompted {
                value_type: string,
                prompt: "a word".to_string(),
            }))
        );
    }

    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:3e472f6364107275a9eb1e7fbdf35e80d3d334ece89274fea50b202686c4c567
    #[test]
    fn a_call_of_a_function_declared_outside_the_programs_runtime_code_is_run() {
        let library = source(
            "lib/wrap.lfy",
            "ace function wrap(held: object): `What it was given` -> object {\n  \
             return held;\n}\n",
            &[],
            Origin::Library,
        );
        let main = source(
            "lib/main.lfy",
            "use \"./wrap\";",
            &[Some("lib/wrap.lfy")],
            Origin::Prelude,
        );
        let program = source("a.lfy", "ace const c = wrap({ a = 1 });\n", &[], Origin::Program);
        let mut model = bind(vec![library, main, program]);
        let file = model.file("a.lfy").expect("the program file");
        let declaration = first(&model, file, S::VariableDeclaration);
        let value = value_of(&model, declaration);
        assert_eq!(
            evaluate(&mut model, value),
            Some(Evaluated::Value(Value::Object(BTreeMap::from([(
                "a".to_string(),
                Value::Number(1.0),
            )]))))
        );
    }

    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:3c659b6359b26f9db37a1d4d512e95bf301ec47c24dd44273f3d6bc486e21dc3
    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:b0d45fd642afa8465747a9758d2568b1b29d13daad273eafefebfa8349ce44f6
    #[test]
    fn the_same_node_twice_gives_the_same_value_and_the_model_is_not_changed() {
        let mut model = bound("d A {}\nconst n = A@identifier;\nconst m = n;\n");
        let declaration = nth(&model, 0, S::VariableDeclaration, 1);
        let value = value_of(&model, declaration);
        let before = model.clone();
        let once = evaluate(&mut model, value);
        let twice = evaluate(&mut model, value);
        assert_eq!(once, twice);
        assert_eq!(once, Some(Evaluated::Value(Value::String("A".to_string()))));
        assert_eq!(before, model, "evaluating changed the model");
    }

    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:762654352875a07eb98f9d3d15a3c8d3308f2be90e1f874cac1f1f33a2fe7f9d
    #[test]
    fn what_only_has_a_value_at_runtime_gives_nothing_back_and_adds_no_problem() {
        let mut model = bound("function f(x: number) -> number {\n  return x + 1;\n}\n");
        let statement = first(&model, 0, S::Return);
        let inner = model
            .child(statement, S::ExpressionStatement)
            .and_then(|s| model.child_nodes(s).into_iter().next())
            .expect("the returned expression");
        assert_eq!(evaluate(&mut model, inner), None);
        assert!(model.problems.is_empty(), "{:?}", model.problems);
    }

    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:6eabf4cd81466f895280d4dd2bf7be3d839f75a1f5ffcdfda67a6c92678b3cca
    #[test]
    fn a_value_that_needs_its_own_gives_nothing_back_and_names_the_cycle() {
        let mut model = bound("const a = b;\nconst b = a;\n");
        model.problems.clear();
        let declaration = nth(&model, 0, S::VariableDeclaration, 0);
        let value = value_of(&model, declaration);
        assert_eq!(evaluate(&mut model, value), None);
        assert_eq!(model.problems.len(), 1, "{:?}", model.problems);
        assert!(
            model.problems[0].message.contains("needs its own value"),
            "{}",
            model.problems[0].message
        );
    }

    /// This is the pass the binder runs between declare and resolve, so the model
    /// `crate::model::bind` builds is this pass's work: running it again over the declared
    /// repository reaches the same traits and the same criteria, with no problem of its
    /// own, and both reach what only a complete trait list and a criterion added to
    /// `global` can.
    // @lfy def/interpret/main.lfy:expand#expand:expand:9f92ae8a6753e1a25414725ffb95c7356c3fb1021cf313cd36c68c65c0e8b6a4
    #[test]
    fn the_repository_expands_as_the_binders_expand_pass() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let bound = crate::workspace::load(root).model;
        let expanded = expand(declared(bound.sources.clone()));
        assert_eq!(expanded.entities.len(), bound.entities.len());
        let mut differences: Vec<String> = Vec::new();
        for entity in 0..bound.entities.len() {
            // A declaration seen with type arguments takes what its declaration reached
            // after the expand pass, which is the binder's own step and not this one's.
            if bound.generic_base(entity) != entity {
                continue;
            }
            let (was, now) = (&bound.entities[entity], &expanded.entities[entity]);
            let named = |ids: &[EntityId]| -> Vec<String> {
                ids.iter()
                    .map(|&id| {
                        bound.entities[id]
                            .identifier
                            .clone()
                            .unwrap_or_else(|| format!("#{id}"))
                    })
                    .collect()
            };
            let where_at = was
                .node
                .map(|node| bound.sources[node.file].path.clone())
                .unwrap_or_default();
            // The same traits, in both directions: what the binder's model carries is what
            // this pass applies, and nothing else.
            let mine: Vec<EntityId> = now.traits.iter().map(|a| a.entity).collect();
            let theirs: Vec<EntityId> = was.traits.iter().map(|a| a.entity).collect();
            let missing: Vec<EntityId> = theirs
                .iter()
                .copied()
                .filter(|applied| !mine.contains(applied))
                .collect();
            let extra: Vec<EntityId> = mine
                .iter()
                .copied()
                .filter(|applied| !theirs.contains(applied))
                .collect();
            if !missing.is_empty() || !extra.is_empty() {
                differences.push(format!(
                    "{:?} of {where_at} carries {:?} where the binder's model carries {:?}",
                    was.identifier,
                    named(&mine),
                    named(&theirs)
                ));
            }
            let show = |criteria: &[Criterion]| -> Vec<String> {
                criteria
                    .iter()
                    .map(|c| format!("{:?}/{:?}", c.situation, c.behavior))
                    .collect()
            };
            let held = show(&now.acceptance_criteria);
            let mut absent = show(&was.acceptance_criteria);
            absent.retain(|one| !held.contains(one));
            if !absent.is_empty() {
                differences.push(format!(
                    "{:?} of {where_at} is missing {absent:#?}",
                    was.identifier
                ));
            }
        }
        assert!(differences.is_empty(), "{differences:#?}");
        let messages: Vec<String> = expanded
            .problems
            .iter()
            .map(|problem| format!("{problem:?}"))
            .collect();
        assert!(expanded.problems.is_empty(), "{messages:#?}");

        // A statement that reads what a trait was applied to waits until no statement left
        // to run can apply it, so the loop over `declaring@entities` in `bind` reaches every
        // declaring rule. The binder's own model reaches it too, because this pass is the
        // one it runs.
        // @lfy def/interpret/main.lfy:expand
        let binder = entity_named(&expanded, "bind");
        let says = |model: &Model, entity: EntityId, text: &str| -> bool {
            model.entities[entity]
                .acceptance_criteria
                .iter()
                .any(|criterion| {
                    criterion
                        .situation
                        .iter()
                        .flatten()
                        .any(|one| one.contains(text))
                })
        };
        assert!(
            says(&expanded, binder, "An entity is an enum"),
            "the whole list of kind data rows was read"
        );
        assert!(
            says(&bound, binder, "An entity is an enum"),
            "the binder's expand pass is this one"
        );

        // A criterion added to global's context is global's.
        // @lfy def/interpret/main.lfy:expand
        let reaches = |model: &Model, text: &str| -> bool {
            model.entities[model.global]
                .acceptance_criteria
                .iter()
                .any(|criterion| {
                    criterion
                        .behavior
                        .iter()
                        .flatten()
                        .any(|one| one.contains(text))
                })
        };
        assert!(
            reaches(&expanded, "No two [[terminal]] entities have the same syntax"),
            "what def/grammar adds to global's context is global's"
        );
        assert!(
            reaches(&bound, "No two [[terminal]] entities have the same syntax"),
            "the binder's model reaches it because this pass is the one it runs"
        );
    }

    // ---- lower ---------------------------------------------------------------------

    // @lfy def/interpret/main.lfy:lower#lower:lower:19c61e03880d4a18f2b273191a8e8ffae4a8b61376fa29fde230cb24987ab258
    #[test]
    fn a_data_is_kept_with_its_members_and_its_criteria_and_its_trait_is_dropped() {
        let program = lower(workspace(
            "trait t {\n  $x: `d` = string;\n}\nd A is t {\n  where (`s`) -> `b`;\n}\n",
        ));
        assert_eq!(program.files.len(), 1);
        let file = &program.files[0];
        assert_eq!(file.root.rule, F::SourceFile.entity());
        let kept: Vec<&LoweredNode> = file.root.nodes().collect();
        assert_eq!(kept.len(), 1, "only the data is kept: {kept:?}");
        let data = kept[0];
        assert_eq!(data.rule, S::DataDeclaration.entity());
        // With no clause: the `is t` was dropped with everything inside it.
        assert!(
            !file.text.contains(" is t"),
            "the clause was dropped: {}",
            file.text
        );
        assert!(!file.text.contains("trait t"), "no node for t: {}", file.text);
        // One member node for x, whose origin is the statement in the trait that declared it.
        let members: Vec<&LoweredNode> = data.nodes().collect();
        assert_eq!(members.len(), 1, "{members:?}");
        let member = members[0];
        let model = &program.workspace.model;
        assert_eq!(
            model.entities[member.entity.expect("a member")].identifier.as_deref(),
            Some("x")
        );
        let inside_the_trait = model
            .ancestor(member.origin, S::TraitDeclaration)
            .expect("the origin is in the trait");
        assert_eq!(
            model
                .symbol_of(inside_the_trait)
                .map(|s| model.symbols[s].name.clone()),
            Some("t".to_string())
        );
        // One lowered criterion, with situation `s` and an id naming A twice.
        assert_eq!(data.criteria.len(), 1, "{:?}", data.criteria);
        let criterion = &data.criteria[0];
        assert_eq!(criterion.situation.as_deref(), Some(["s".to_string()].as_slice()));
        assert!(criterion.id.starts_with("A:A:"), "{}", criterion.id);
        assert_eq!(criterion.scope, RequirementScope::Local);
        // And never into the tree's children.
        assert!(
            every(&file.root).iter().all(|node| node.rule != S::Where.entity()),
            "no criterion is a node"
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:1b8bbc7b6e2316e2fb3cfd378dd5fc01093dbefd96e6098d404519e88a6a68ae
    #[test]
    fn a_member_whose_type_is_a_list_of_a_union_is_spelled_with_the_union_parenthesized() {
        let program = lower(workspace(
            "d A {}\nd B {}\nd Box {\n  $pairs: `d` = (A | B)[];\n  $one: `d` = A | B;\n\
             \n  $plain: `d` = A[];\n}\n",
        ));
        let file = &program.files[0];
        assert!(
            file.text.contains("$pairs: `d` = (A | B)[];"),
            "the union takes parentheses before the brackets: {}",
            file.text
        );
        // A union that is not the item of a list, and a list whose item is not a union,
        // take no parentheses.
        assert!(
            file.text.contains("$one: `d` = A | B;") && file.text.contains("$plain: `d` = A[];"),
            "{}",
            file.text
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
    #[test]
    fn a_member_type_naming_an_entity_of_an_unused_file_adds_one_use_of_that_file() {
        let program = lower(workspace_of(vec![
            source(
                "def/model/kinds.lfy",
                "d Kind {}\nd Shape {}\n",
                &[],
                Origin::Program,
            ),
            source(
                "def/t.lfy",
                "use \"./model/kinds\";\ntrait t {\n  $kind: `Which` = Kind;\n  \
                 $shape: `What` = Shape;\n}\n",
                &[Some("def/model/kinds.lfy")],
                Origin::Program,
            ),
            source(
                "def/a.lfy",
                "use \"./t\";\nd A is t {}\n",
                &[Some("def/t.lfy")],
                Origin::Program,
            ),
        ]));
        let file = &program.files[2];
        // One use, however many members name the file, with its path written relative to
        // the file the use is written in.
        assert_eq!(
            file.text.matches("use \"./model/kinds\";").count(),
            1,
            "{}",
            file.text
        );
        let written = file.text.find("use \"./t\";").expect("the use written");
        let added = file
            .text
            .find("use \"./model/kinds\";")
            .expect("the use added");
        assert!(written < added, "the added use comes after: {}", file.text);
        assert!(
            file.text.contains("$kind: `Which` = Kind;")
                && file.text.contains("$shape: `What` = Shape;"),
            "{}",
            file.text
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
    #[test]
    fn an_added_use_takes_an_alias_when_the_file_already_sees_the_name() {
        let program = lower(workspace_of(vec![
            source("def/model/kinds.lfy", "d Kind {}\n", &[], Origin::Program),
            source("def/other.lfy", "d Kind {}\n", &[], Origin::Program),
            source(
                "def/t.lfy",
                "use \"./model/kinds\";\ntrait t {\n  $kind: `Which` = Kind;\n}\n",
                &[Some("def/model/kinds.lfy")],
                Origin::Program,
            ),
            source(
                "def/a.lfy",
                "use \"./other\";\nuse \"./t\";\nd A is t {}\n",
                &[Some("def/other.lfy"), Some("def/t.lfy")],
                Origin::Program,
            ),
        ]));
        let file = &program.files[3];
        // The alias is the declaring file's path under the source directory, without its
        // extension and with each slash an underscore, and the type is spelled through it.
        assert!(
            file.text.contains("use \"./model/kinds\" as model_kinds;"),
            "{}",
            file.text
        );
        assert!(
            file.text.contains("$kind: `Which` = model_kinds.Kind;"),
            "{}",
            file.text
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
    // @lfy def/interpret/main.lfy:lower#lower:lower:7e159bfaa7ce0d4765fa4fdbd906f7230afdf9fce2ae0297f29ee8f15f2af929
    #[test]
    fn an_alias_names_nothing_else_and_a_package_file_is_used_by_its_package() {
        assert_eq!(alias_of("def", "def/model/kind-set.lfy"), "model_kind_set");
        assert_eq!(
            package_use_path("elfie", "../lib", "../lib/prelude/trait.lfy"),
            "elfie/prelude/trait"
        );
        let program = lower(workspace_of(vec![
            source("def/model/kinds.lfy", "d Kind {}\n", &[], Origin::Program),
            source("def/other.lfy", "d Kind {}\n", &[], Origin::Program),
            source("def/model_kinds.lfy", "d model_kinds {}\n", &[], Origin::Program),
            source(
                "def/t.lfy",
                "use \"./model/kinds\";\ntrait t {\n  $kind: `Which` = Kind;\n}\n",
                &[Some("def/model/kinds.lfy")],
                Origin::Program,
            ),
            source(
                "def/a.lfy",
                "use \"./other\";\nuse \"./model_kinds\";\nuse \"./t\";\nd A is t {}\n",
                &[
                    Some("def/other.lfy"),
                    Some("def/model_kinds.lfy"),
                    Some("def/t.lfy"),
                ],
                Origin::Program,
            ),
        ]));
        let file = &program.files[4];
        assert!(
            file.text.contains("use \"./model/kinds\" as model_kinds1;"),
            "{}",
            file.text
        );
        assert!(
            file.text.contains("$kind: `Which` = model_kinds1.Kind;"),
            "{}",
            file.text
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:da4462e6d2b3dd1ccd2d1540c52f12f97141c350c88131325e6ac17e2e542890
    #[test]
    fn an_ace_function_is_dropped_and_the_const_it_answers_holds_its_value() {
        let program = lower(workspace(
            "ace function q(t: string) -> string {\n  return t + \"!\";\n}\nconst c = q(\"x\");\n",
        ));
        let file = &program.files[0];
        let kept: Vec<&LoweredNode> = file.root.nodes().collect();
        assert_eq!(kept.len(), 1, "only the const is kept: {kept:?}");
        assert_eq!(kept[0].rule, S::VariableDeclaration.entity());
        let folded: Vec<&LoweredNode> = every(kept[0])
            .into_iter()
            .filter(|node| node.is_folded())
            .collect();
        assert_eq!(folded.len(), 1, "{folded:?}");
        assert_eq!(
            folded[0].value,
            Some(Evaluated::Value(Value::String("x!".to_string())))
        );
        assert!(file.text.contains("\"x!\""), "{}", file.text);
        assert!(program.problems.is_empty(), "{:?}", program.problems);
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:d661ea358255d3366f18899795e1a76056c8a3e1deed68e3c07998617fe2fcd1
    #[test]
    fn what_the_model_answers_is_folded_and_what_only_runtime_answers_is_kept() {
        let program = lower(workspace(
            "trait r {}\nd A is r {}\nfunction names() -> string[] {\n  let out = [];\n\
             \n  for (const e in r@entities) {\n    out.push(e@identifier);\n  }\n\n  return out;\n}\n",
        ));
        let file = &program.files[0];
        let model = &program.workspace.model;
        let a = entity_named(model, "A");
        let function = file
            .root
            .nodes()
            .find(|node| node.rule == S::FunctionDeclaration.entity())
            .expect("the function is kept");
        let folded: Vec<&LoweredNode> = every(function)
            .into_iter()
            .filter(|node| node.is_folded())
            .collect();
        assert!(
            folded
                .iter()
                .any(|node| node.value
                    == Some(Evaluated::Value(Value::List(vec![Value::Entity(a)])))),
            "r@entities folded to a list holding A: {folded:?}"
        );
        // `e@identifier` is kept, because `e` only has a value at runtime.
        assert!(
            file.text.contains("e@identifier"),
            "e@identifier is kept: {}",
            file.text
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:ddb0c0418b9da6c686e961a3b22818b2b8c151a8614ec2dbeed43fc2390af6fd
    #[test]
    fn a_criterion_added_to_global_or_by_a_trait_applied_to_it_is_global_and_on_no_node() {
        let a = source(
            "a.lfy",
            "trait g {\n  where (`any`) -> `holds`;\n}\ng.apply(global);\nd A {}\nd B {}\n",
            &[],
            Origin::Program,
        );
        let b = source(
            "b.lfy",
            "use \"./a\";\nglobal@acceptanceCriteria.add({ behavior = `all` });\n",
            &[Some("a.lfy")],
            Origin::Program,
        );
        let built = workspace_of(vec![a, b]);
        let global = built.model.global;
        assert_eq!(
            built.model.entities[global].acceptance_criteria.len(),
            2,
            "{:?} problems {:?}",
            built.model.entities[global].acceptance_criteria,
            built.model.problems
        );
        let program = lower(built);
        assert_eq!(program.criteria.len(), 2, "{:?}", program.criteria);
        let ids: Vec<&str> = program.criteria.iter().map(|c| c.id.as_str()).collect();
        assert!(
            ids.iter().any(|id| id.starts_with("global:g:")),
            "the trait's criterion is global: {ids:?}"
        );
        assert!(
            ids.iter().all(|id| id.starts_with("global:")),
            "every one of them is global: {ids:?}"
        );
        for criterion in &program.criteria {
            assert_eq!(criterion.scope, RequirementScope::Global);
            assert_eq!(criterion.entity, None);
        }
        // And on no node and no file.
        for file in &program.files {
            assert!(file.criteria.is_empty(), "{:?}", file.criteria);
            for node in every(&file.root) {
                assert!(node.criteria.is_empty(), "{:?}", node.criteria);
            }
        }
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:5770c9d25caf0fb2ef7bb2b57c8f55ee0200d98e2f2b365f2025766aa3c0a165
    #[test]
    fn a_file_of_only_compile_time_statements_lowers_to_an_empty_source_file() {
        let program = lower(workspace("trait t {}\ntrait u {}\n"));
        let file = &program.files[0];
        assert_eq!(file.root.rule, F::SourceFile.entity());
        assert_eq!(file.root.nodes().count(), 0, "{:?}", file.root);
        assert_eq!(file.text.trim(), "");
        // One lowered file per file of the workspace, in the same order.
        assert_eq!(program.files.len(), program.workspace.files.len());
        assert_eq!(file.file, 0);
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:2fbd03867430231c575f4b7bd56187dd57505ab549bcea0488aa06843b0f9b93
    #[test]
    fn a_function_whose_every_statement_is_compile_time_is_kept_empty_and_named() {
        let program = lower(workspace(
            "trait t {}\nd X {}\nfunction f() {\n  t.apply(X);\n}\n",
        ));
        let file = &program.files[0];
        // The data and the function are kept; the trait is not.
        let kept: Vec<&LoweredNode> = file.root.nodes().collect();
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert_eq!(kept[0].rule, S::DataDeclaration.entity());
        let f = kept[1];
        assert_eq!(f.rule, S::FunctionDeclaration.entity());
        // Its body is empty: the statement ran at compile time and has no lowered node.
        let block = f
            .nodes()
            .find(|node| node.rule == S::Block.entity())
            .expect("the body is kept");
        assert_eq!(block.nodes().count(), 0, "{block:?}");
        // The statement ran as it would anywhere else, so X carries t.
        let model = &program.workspace.model;
        let t = entity_named(model, "t");
        let x = entity_named(model, "X");
        assert!(model.entities[x].has_trait(t), "X carries t");
        // And one problem, at the function rather than at the statement.
        assert_eq!(program.problems.len(), 1, "{:?}", program.problems);
        assert_eq!(program.problems[0].node, f.origin);
        assert!(
            program.problems[0].message.contains("empty at runtime"),
            "{}",
            program.problems[0].message
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:c89173885cbe173a76e8a9a59310d5e69174bce617247c866b7d675aeeb308e2
    #[test]
    fn a_file_level_let_is_not_allowed_and_is_kept() {
        let program = lower(workspace("let n = 1;\n"));
        assert_eq!(program.problems.len(), 1, "{:?}", program.problems);
        assert!(
            program.problems[0].message.contains("file-level let"),
            "{}",
            program.problems[0].message
        );
        let kept: Vec<&LoweredNode> = program.files[0].root.nodes().collect();
        assert_eq!(kept.len(), 1, "the declaration is kept: {kept:?}");
        assert_eq!(kept[0].rule, S::VariableDeclaration.entity());
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:3032db4bcf2ffe2031d9fceb6ceced6ed34b2271c3cc994fa6cfb219215ef323
    #[test]
    fn a_criterion_of_a_targets_declaration_is_guidance_and_no_criterion_of_the_program() {
        let mut sources = targets();
        sources.push(source(
            "a.lfy",
            "trait shape extends targetLanguage {\n  where (`built`) -> `shaped`;\n}\n\
             ace const rust = targetOf({ output = \"crates\", language = shape });\n\
             @acceptanceCriteria.add({ behavior = `the file holds` });\nd A {}\n",
            &[],
            Origin::Program,
        ));
        let mut workspace = workspace_of(sources);
        let declaration = entity_named(&workspace.model, "rust");
        // The const carries the layer's criterion before lowering.
        assert_eq!(
            workspace.model.entities[declaration]
                .acceptance_criteria
                .len(),
            1,
            "{:?}",
            workspace.model.entities[declaration].acceptance_criteria
        );
        workspace.targets.push(crate::workspace::Target {
            identifier: "rust".to_string(),
            declaration,
            layers: vec![entity_named(&workspace.model, "shape")],
            output_directory: "crates".to_string(),
            extensions: Vec::new(),
            marker_comment: "//".to_string(),
            script_runner: None,
            native_dependencies: Vec::new(),
            commands: Vec::new(),
            knowledge: Vec::new(),
        });
        let program = lower(workspace);
        // The file's own criterion is still a requirement; the target's guidance is not.
        let ids: Vec<&str> = program
            .files
            .iter()
            .flat_map(|file| file.criteria.iter())
            .map(|criterion| criterion.id.as_str())
            .collect();
        assert_eq!(ids.len(), 1, "{ids:?}");
        assert!(
            every(&program.files[program.files.len() - 1].root)
                .iter()
                .all(|node| node.criteria.is_empty()),
            "no node carries the target's guidance"
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:958d30dcc98b23d0e9373cc5d9ee9503e4e30d4587d485a2f01b9a25cdb0a2d8
    #[test]
    fn a_test_of_an_entity_is_lowered_once_with_its_id_and_its_values() {
        let program = lower(workspace(
            "fn f(): `Gives a number` => number {\n  @test({ input = [1], expect = 2 });\n}\n",
        ));
        let file = &program.files[0];
        let function = file
            .root
            .nodes()
            .find(|node| node.rule == S::AgentFunctionDeclaration.entity())
            .expect("the fn is kept");
        assert_eq!(function.tests.len(), 1, "{:?}", function.tests);
        let case = &function.tests[0];
        assert!(case.id.starts_with("f:f:"), "{}", case.id);
        assert_eq!(case.scope, RequirementScope::Local);
        assert_eq!(case.input_text, "[1]");
        assert_eq!(case.expect_text, "2");
        assert_eq!(
            case.input_value,
            Evaluated::Value(Value::List(vec![Value::Number(1.0)]))
        );
        assert_eq!(case.expect_value, Evaluated::Value(Value::Number(2.0)));
        // And never into the tree's children.
        assert!(
            every(&file.root)
                .iter()
                .all(|node| node.rule != S::Block.entity()),
            "the body of a fn is not kept"
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:74f66400f215c724d23d2684a36b581975ecba82a95f30ba65103866b8f046b6
    #[test]
    fn a_criterion_for_a_files_own_entity_joins_the_file() {
        let program = lower(workspace(
            "@acceptanceCriteria.add({ behavior = `the file holds` });\nd A {}\n",
        ));
        let file = &program.files[0];
        assert_eq!(file.criteria.len(), 1, "{:?}", file.criteria);
        assert!(
            file.criteria[0].id.starts_with("a.lfy:a.lfy:"),
            "{}",
            file.criteria[0].id
        );
        assert_eq!(file.criteria[0].scope, RequirementScope::Local);
        for node in every(&file.root) {
            assert!(node.criteria.is_empty(), "{:?}", node.criteria);
        }
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:8bfaca8a3f0160f288d5a8a901bb6bc7b79163d2bdbed64540cdb5a0802ef829
    #[test]
    fn a_test_for_a_files_own_entity_joins_the_file() {
        let program = lower(workspace("@test({ input = [], expect = 1 });\nd A {}\n"));
        let file = &program.files[0];
        assert_eq!(file.tests.len(), 1, "{:?}", file.tests);
        assert!(
            file.tests[0].id.starts_with("a.lfy:a.lfy:"),
            "{}",
            file.tests[0].id
        );
        assert_eq!(file.tests[0].scope, RequirementScope::Local);
        assert!(program.tests.is_empty(), "{:?}", program.tests);
        for node in every(&file.root) {
            assert!(node.tests.is_empty(), "{:?}", node.tests);
        }
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:5427d71f70b3ddb4ee8f7b3accfd74c70441030a3d7c200e412ec249d90ce6f5
    #[test]
    fn a_call_of_like_is_one_folded_node_holding_its_prompted() {
        let program = lower(workspace("d A {}\nconst s = A@like(`something an A holds`);\n"));
        let file = &program.files[0];
        let a = entity_named(&program.workspace.model, "A");
        let folded: Vec<&LoweredNode> = every(&file.root)
            .into_iter()
            .filter(|node| node.is_folded())
            .collect();
        assert_eq!(folded.len(), 1, "{folded:?}");
        assert_eq!(
            folded[0].value,
            Some(Evaluated::Prompted(Prompted {
                value_type: a,
                prompt: "something an A holds".to_string(),
            }))
        );
        assert!(
            file.text.contains("A@like(`something an A holds`)"),
            "{}",
            file.text
        );
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:62d2d27335ac81028f0c9e82819680564bcc9debd24a3e002e179a810efaa49f
    #[test]
    fn runtime_code_that_calls_an_ace_function_with_no_compile_time_argument_is_a_problem() {
        let program = lower(workspace(
            "ace function q(t: string) -> string {\n  return t + \"!\";\n}\n\
             function f(x: string) -> string {\n  return q(x);\n}\n",
        ));
        assert_eq!(program.problems.len(), 1, "{:?}", program.problems);
        assert!(
            program.problems[0].message.contains("ace function"),
            "{}",
            program.problems[0].message
        );
        let model = &program.workspace.model;
        assert!(model.is(program.problems[0].node, E::Call));
    }

    // @lfy def/interpret/main.lfy:lower#lower:lower:699284343b9ef81cb11db5668663aac649e418507ad5cea495ddceee254e4d43
    #[test]
    fn a_test_added_to_globals_context_is_global_and_on_no_node() {
        let program = lower(workspace(
            "global@test({ input = [], expect = 1 });\nd A {}\n",
        ));
        assert_eq!(program.tests.len(), 1, "{:?}", program.tests);
        let case = &program.tests[0];
        assert!(case.id.starts_with("global:"), "{}", case.id);
        assert_eq!(case.scope, RequirementScope::Global);
        assert_eq!(case.entity, None);
        for file in &program.files {
            assert!(file.tests.is_empty(), "{:?}", file.tests);
            for node in every(&file.root) {
                assert!(node.tests.is_empty(), "{:?}", node.tests);
            }
        }
    }

    /// The whole repository lowers: one file per file, every origin a node of its own file,
    /// every id its own, and every lowered file still Elfie.
    // @lfy def/interpret/main.lfy:lower#lower:lower:958d30dcc98b23d0e9373cc5d9ee9503e4e30d4587d485a2f01b9a25cdb0a2d8
    #[test]
    fn the_repository_lowers_to_elfie_with_an_origin_for_everything() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let mut loaded = crate::workspace::load(root);
        // The model lowering reads is the model the expand pass left.
        loaded.model = expand(declared(loaded.model.sources.clone()));
        let program = lower(loaded);
        let model = &program.workspace.model;
        assert_eq!(program.files.len(), program.workspace.files.len());
        assert!(program.files.len() > 20, "{} files", program.files.len());
        let mut ids: Vec<&str> = Vec::new();
        for (index, file) in program.files.iter().enumerate() {
            assert_eq!(file.file, index);
            let source = program.workspace.files[index].source;
            for node in every(&file.root) {
                // Every origin names a node of the program: the file's own, or, for a member
                // a trait added, the trait's.
                assert!(node.origin.file < model.sources.len(), "an origin of no file");
                assert!(
                    node.origin.index < model.nodes[node.origin.file].len(),
                    "an origin that names no node"
                );
                if !is_member_node(model, node) {
                    assert_eq!(node.origin.file, source, "an origin of another file");
                }
                // A folded node holds a value and no children; a kept one the other way.
                assert!(!(node.is_folded() && !node.children.is_empty()));
                ids.extend(node.criteria.iter().map(|c| c.id.as_str()));
                ids.extend(node.tests.iter().map(|t| t.id.as_str()));
            }
            ids.extend(file.criteria.iter().map(|c| c.id.as_str()));
            ids.extend(file.tests.iter().map(|t| t.id.as_str()));
            // The lowered text is still Elfie.
            let path = program.workspace.files[index].path.clone();
            let tokens = crate::lexer::lex(&file.text, Some(&path))
                .unwrap_or_else(|error| panic!("{path} does not lex: {error}"));
            let tree = crate::parser::parse(tokens, None);
            assert!(
                tree.errors.is_empty(),
                "the lowered {path} does not parse:\n{}\n{}",
                file.text,
                tree.render()
            );
        }
        ids.extend(program.criteria.iter().map(|c| c.id.as_str()));
        ids.extend(program.tests.iter().map(|t| t.id.as_str()));
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        let repeated: Vec<&str> = sorted
            .windows(2)
            .filter(|pair| pair[0] == pair[1])
            .map(|pair| pair[0])
            .collect();
        assert!(repeated.is_empty(), "two requirements share an id: {repeated:#?}");
        assert!(!program.criteria.is_empty(), "the program has global criteria");
        let messages: Vec<String> = program
            .problems
            .iter()
            .map(|problem| format!("{problem:?}"))
            .collect();
        assert!(program.problems.is_empty(), "{messages:#?}");
    }

    // @lfy def/interpret/main.lfy:lower
    #[test]
    fn a_criterion_keeps_the_entities_it_names_as_links() {
        let program = lower(workspace(
            "d Other {}\nfn f(): `Does it` => number {\n  where (`asked`) -> `reads [[Other]]`;\n}\n",
        ));
        let file = &program.files[0];
        let model = &program.workspace.model;
        let other = entity_named(model, "Other");
        let function = file
            .root
            .nodes()
            .find(|node| node.rule == S::AgentFunctionDeclaration.entity())
            .expect("the fn is kept");
        assert_eq!(function.criteria.len(), 1, "{:?}", function.criteria);
        let criterion = &function.criteria[0];
        assert_eq!(criterion.references, [other]);
        // The texts are held as rendered, with each reference replaced by its name.
        assert_eq!(
            criterion.behavior.as_deref(),
            Some(["reads Other".to_string()].as_slice())
        );
    }
}
