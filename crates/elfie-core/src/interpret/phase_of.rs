// @lfy def/interpret/main.lfy:phaseOf
//! Whether a node runs at compile time or is kept for runtime.

use super::*;

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
pub(super) enum CompileCall {
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
pub(super) enum Added {
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
pub(super) fn added_member(model: &Model, receiver: NodeRef) -> Option<(Added, NodeRef)> {
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
pub(super) fn compile_time_call(model: &Model, call: NodeRef) -> Option<CompileCall> {
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
