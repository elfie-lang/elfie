// @lfy def/interpret/main.lfy:expand
//! Running every piece of compile-time code that can add names, until nothing new
//! appears.

use super::*;

/// Why a statement cannot run yet.
// @lfy def/interpret/main.lfy:expand
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NotReady {
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
        if ran_something {
            continue;
        }
        // The rounds have stopped. A statement left whose name still does not resolve runs
        // as far as it can, and the name is named when running it did not make it resolve;
        // the rounds then resume, since running it may have added names.
        // @lfy def/interpret/main.lfy:expand#expand:expand:8fd627207ea86becebc501f08576d358a6e4d446addee292cd0f1634bb4b9beb
        let stuck = (0..pending.len()).find(|&index| {
            let others: Vec<NodeRef> = pending
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != index)
                .map(|(_, &node)| node)
                .collect();
            matches!(
                interpreter.readiness(pending[index], &others),
                Some(NotReady::Unresolved(_))
            )
        });
        let Some(index) = stuck else {
            break;
        };
        let item = pending.remove(index);
        interpreter.run_statement(item);
        if let Some(at) = interpreter.unresolved_name(item) {
            let text = interpreter.model.raw(at).trim().to_string();
            interpreter.problem(at, format!("{text} does not resolve"));
        }
    }
    // Every statement left is waiting on a trait.
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
            // The loop above leaves no statement whose name does not resolve.
            Some(NotReady::Unresolved(_)) | None => interpreter.run_statement(item),
        }
    }
    model
}

impl Interpreter<'_> {
    /// Runs one statement of the top level of a file.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn run_statement(&mut self, item: NodeRef) {
        let mut env = self.file_env(item.file);
        self.exec(item, &mut env);
    }

    /// Why a statement cannot run yet, or nothing when it can.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn readiness(&mut self, item: NodeRef, others: &[NodeRef]) -> Option<NotReady> {
        // A statement that reads what a trait was applied to waits until no statement left
        // to run can apply that trait, so the list it reads is the full one.
        // @lfy def/interpret/main.lfy:expand
        for (at, applied) in self.entity_reads(item) {
            if others.iter().any(|&other| self.applies(other, applied)) {
                return Some(NotReady::Waiting { at, applied });
            }
        }
        self.unresolved_name(item).map(NotReady::Unresolved)
    }

    /// The first name a statement needs to run that does not resolve yet: a trait an `is`
    /// or `extends` clause names, or a path through the scope layer that a `with` targets
    /// or an `apply` is called on or with. Names are resolved only as far as a statement
    /// needs them to run; the resolve pass resolves the rest.
    // @lfy def/interpret/main.lfy:expand#expand:expand:9f92ae8a6753e1a25414725ffb95c7356c3fb1021cf313cd36c68c65c0e8b6a4
    pub(super) fn unresolved_name(&self, item: NodeRef) -> Option<NodeRef> {
        for at in self.model.descendants(item) {
            if self.model.is(at, E::TraitUse) && resolve_trait_use(self.model, at).is_none() {
                return Some(at);
            }
            // A trait's own body runs once per application, against its receiver, so the
            // paths inside it are not read here.
            if self.model.ancestor(at, S::TraitDeclaration).is_some() {
                continue;
            }
            let mut paths: Vec<NodeRef> = Vec::new();
            if self.model.is(at, S::With) {
                paths.extend(
                    self.model
                        .child_nodes(at)
                        .into_iter()
                        .find(|&child| !self.model.is(child, S::Block)),
                );
            }
            if self.model.is(at, E::Call)
                && compile_time_call(self.model, at) == Some(CompileCall::Apply)
            {
                paths.extend(self.model.left(at).and_then(|callee| self.model.left(callee)));
                paths.extend(self.model.arguments(at).first().copied());
            }
            if let Some(path) = paths.into_iter().find(|&path| self.scope_path_unresolved(path)) {
                return Some(path);
            }
        }
        None
    }

    /// Whether a path through the scope layer, such as `A$x`, starts at a declared name and
    /// does not resolve yet, as when the member it reaches is added by a statement still to
    /// run. A path that starts at a variable or a parameter only has its value when it runs.
    // @lfy def/interpret/main.lfy:expand#expand:expand:0ebadc4acebce99f628486e58f24cf71f0da3ccd7eac9ea7d09c7b2bd0db2553
    fn scope_path_unresolved(&self, path: NodeRef) -> bool {
        let mut base = path;
        let mut through_scope = false;
        while self.model.is(base, E::Member) {
            let Some((accessor, Some(_))) = self.model.accessor_and_name(base) else {
                return false;
            };
            if !self.model.token_is(base.file, accessor, P::ScopeAccessor) {
                return false;
            }
            let Some(left) = self.model.left(base) else {
                return false;
            };
            through_scope = true;
            base = left;
        }
        if !through_scope || !self.model.is(base, E::Name) {
            return false;
        }
        let declared = resolve_name_node(self.model, base).is_some_and(|symbol| {
            !matches!(
                self.model.symbols[symbol].kind,
                SymbolKind::Variable
                    | SymbolKind::LoopVariable
                    | SymbolKind::Parameter
                    | SymbolKind::TypeParameter
            )
        });
        declared && resolve_name_node(self.model, path).is_none()
    }

    /// Every read of what a trait was applied to inside a statement: where it is read, and
    /// the trait it reads.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn entity_reads(&self, item: NodeRef) -> Vec<(NodeRef, EntityId)> {
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
    pub(super) fn applies(&self, item: NodeRef, applied: EntityId) -> bool {
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
    pub(super) fn extends_reaches(&self, from: EntityId, applied: EntityId) -> bool {
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
