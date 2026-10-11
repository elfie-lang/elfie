// @lfy def/model/main.lfy:bind
//! The statements of a body: declarations, trait applications, `with`, `where`, `if`,
//! `for`, and `match`.

use super::*;

impl Binder {
    // ---- Bodies -------------------------------------------------------------------
    //
    // The top level of a file is run by the interpreter's expand pass, which `bind` calls
    // in place of a pass of its own; what is left here runs a body for the resolve pass
    // and for `finish_definitions`.
    // @lfy def/interpret/main.lfy:expand

    /// Runs one statement.
    pub(crate) fn exec(&mut self, r: NodeRef, env: &mut Env) {
        if env.ret.is_some() {
            return;
        }
        let trees = self.trees.clone();
        let rule = trees.rule(r);
        if let Some(symbol) = self.model.symbol_of(r)
            && trees.is_statement(r)
            && self.model.symbols[symbol].kind != SymbolKind::LoopVariable
        {
            self.exec_declaration(r, symbol, env);
            return;
        }
        if rule == S::ExpressionStatement.entity() {
            if let Some(expression) = trees.child_nodes(r).into_iter().next() {
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
            let value = trees
                .child(r, S::ExpressionStatement)
                .and_then(|s| trees.child_nodes(s).into_iter().next())
                .map_or(Value::Undefined, |e| self.eval(e, env));
            env.ret = Some(value);
        } else if rule == S::Block.entity() {
            let scope = self.model.scope_of(r).unwrap_or(env.scope);
            let saved = env.scope;
            let depth = env.vars.len();
            env.scope = scope;
            for statement in trees.child_nodes(r) {
                self.exec(statement, env);
            }
            env.scope = saved;
            env.vars.truncate(depth);
        } else if rule == S::Ace.entity() || rule == S::Async.entity() {
            for statement in trees.child_nodes(r) {
                self.exec(statement, env);
            }
        }
        // Loops, breaks, and uses do nothing at bind time.
    }

    /// A declaration: apply its clauses, then run its body as its own.
    fn exec_declaration(&mut self, r: NodeRef, symbol: SymbolId, env: &mut Env) {
        let trees = self.trees.clone();
        let entity = self.model.symbols[symbol].entity;
        let kind = self.model.symbols[symbol].kind;
        // A variable's value is evaluated when its name is read; nothing to run.
        if matches!(
            kind,
            SymbolKind::Variable
                | SymbolKind::Alias
                | SymbolKind::External
                | SymbolKind::Module
                | SymbolKind::LoopVariable
        ) {
            return;
        }
        // Parameters of the declaration: its value parameters, its type parameters left
        // out, since those are Entity.typeParameters and not FnEntity.parameters.
        // @lfy def/model/main.lfy:bind
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
        // Definition text.
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
        // applyingTraits: the IsClause of the declaration or its signature or Declared.
        let clause_holder = trees
            .child(r, E::Signature)
            .or_else(|| trees.child(r, E::Declared))
            .unwrap_or(r);
        if let Some(is_clause) = trees.child(clause_holder, E::IsClause) {
            self.apply_clause(entity, is_clause, false, env);
        }
        // applyingExtensions: the ExtendsClause.
        if let Some(extends) = trees.child(r, E::ExtendsClause) {
            self.apply_clause(entity, extends, true, env);
        }
        // The body, run as the entity's own.
        if let Some(block) = trees.child(r, S::Block) {
            let scope = self.model.entities[entity].scope.unwrap_or(env.scope);
            let mut own = Env {
                vars: Vec::new(),
                current: entity,
                target: entity,
                contributor: entity,
                scope,
                in_trait: false,
                dry: kind == SymbolKind::Trait,
                ret: None,
            };
            if kind == SymbolKind::Function || kind == SymbolKind::AgentFunction {
                // A function body is code; only its context statements matter here.
                self.exec_context_only(block, &mut own);
            } else {
                self.exec(block, &mut own);
            }
        }
    }

    /// Runs only the statements of a function body that touch the context layer, so
    /// that runtime code does not run at bind time.
    fn exec_context_only(&mut self, block: NodeRef, env: &mut Env) {
        let trees = self.trees.clone();
        let scope = self.model.scope_of(block).unwrap_or(env.scope);
        let saved = env.scope;
        env.scope = scope;
        for statement in trees.child_nodes(block) {
            let rule = trees.rule(statement);
            if rule == S::ExpressionStatement.entity() {
                if let Some(expression) = trees.child_nodes(statement).into_iter().next()
                    && self.is_context_expression(expression)
                {
                    self.eval(expression, env);
                }
            } else if rule == S::Where.entity()
                || rule == S::With.entity()
                || rule == S::For.entity()
                || rule == S::If.entity()
            {
                self.exec(statement, env);
            } else if rule == S::VariableDeclaration.entity() {
                // Constants are read lazily; nothing to do.
            }
        }
        env.scope = saved;
    }

    /// Whether an expression is a chain rooted at a context accessor (`@acceptanceCriteria
    /// .add(...)`, `@test(...)`, `global@...`, `X.apply(...)`).
    fn is_context_expression(&self, expression: NodeRef) -> bool {
        let trees = &self.trees;
        let leftmost = trees.leftmost(expression);
        if trees.is(leftmost, E::Current)
            && let Some((accessor, _)) = trees.accessor_and_name(leftmost)
        {
            return trees.token_is(leftmost.file, accessor, P::ContextAccessor);
        }
        // `X.apply(...)` or `X@acceptanceCriteria...`
        let mut node = expression;
        while trees.rule(node).is_postfix() {
            if trees.is(node, E::Member)
                && let Some((accessor, Some(name))) = trees.accessor_and_name(node)
            {
                let text = trees.token(node.file, name).value.as_str();
                if trees.token_is(node.file, accessor, P::ContextAccessor) || text == "apply" {
                    return true;
                }
            }
            match trees.left(node) {
                Some(left) => node = left,
                None => break,
            }
        }
        false
    }

    /// Each TraitUse becomes one Applied on the declared entity.
    fn apply_clause(&mut self, entity: EntityId, clause: NodeRef, extends: bool, env: &mut Env) {
        let trees = self.trees.clone();
        let Some(uses) = trees.child(clause, E::TraitUses) else {
            return;
        };
        for trait_use in trees.children_of(uses, E::TraitUse) {
            let Some(symbol) = self.resolve_trait_use(trait_use) else {
                continue; // reported by the resolve pass
            };
            let target = self.model.symbols[symbol].entity;
            let arguments = trees
                .child(trait_use, E::Arguments)
                .map(|a| trees.arguments(a))
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
                // Data extending data includes its members.
                // @lfy def/grammar/rules/expression.lfy:ExtendsClause
                self.include_members(entity, target);
            } else {
                self.problem(
                    trait_use,
                    format!(
                        "{} is not a trait",
                        self.model.entities[target]
                            .identifier
                            .clone()
                            .unwrap_or_default()
                    ),
                );
            }
        }
    }

    fn include_members(&mut self, receiver: EntityId, from: EntityId) {
        let Some(from_scope) = self.model.entities[from].scope else {
            return;
        };
        let members: Vec<SymbolId> = self.model.scopes[from_scope]
            .symbols
            .iter()
            .copied()
            .filter(|&s| {
                matches!(
                    self.model.symbols[s].kind,
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
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    fn join_member(&mut self, receiver: EntityId, member: SymbolId) {
        let scope = match self.model.entities[receiver].scope {
            Some(scope) => scope,
            None => {
                let owner = self.model.entities[receiver]
                    .node
                    .unwrap_or(super::bind::SYNTHETIC);
                let scope = self.new_scope(None, owner, receiver);
                self.model.entities[receiver].scope = Some(scope);
                scope
            }
        };
        let name = self.model.symbols[member].name.clone();
        let mut replacing = None;
        if let Some(existing) = self.model.lookup_local(scope, &name) {
            // Decision: only a later trait overrides an earlier one; the entity's own
            // member, or one inherited from data, keeps its definition.
            let trees = self.trees.clone();
            let existing_trait =
                trees.ancestor(self.model.symbols[existing].node, S::TraitDeclaration);
            let new_trait = trees.ancestor(self.model.symbols[member].node, S::TraitDeclaration);
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
                .position(|&s| s == existing);
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
            // @lfy def/model/main.lfy:bind
            Some(position) => self.model.scopes[scope].symbols[position] = id,
            None => self.model.scopes[scope].symbols.push(id),
        }
    }

    /// Evaluates argument nodes where they are written; a spread spreads.
    pub(crate) fn eval_arguments(&mut self, arguments: &[NodeRef], env: &mut Env) -> Vec<Value> {
        let trees = self.trees.clone();
        let mut values = Vec::new();
        for &argument in arguments {
            if trees.is(argument, E::SpreadOperation) {
                let inner = trees.child_nodes(argument).into_iter().next();
                match inner.map(|i| self.eval(i, env)) {
                    Some(Value::List(items)) => values.extend(items),
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
    // @lfy def/model/main.lfy:bind
    pub(crate) fn apply_trait(
        &mut self,
        receiver: EntityId,
        trait_id: EntityId,
        arguments: Vec<NodeRef>,
        values: Vec<Value>,
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
        // The receiver joins the trait's entities, or its extenders.
        // @lfy def/model/main.lfy:bind
        // @lfy def/model/main.lfy:bind
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
        // The parameters of the trait bound to the arguments.
        let mut env = self.trait_env(receiver, trait_id, &values);
        // Each extended trait is applied too, up the whole chain.
        // @lfy def/model/main.lfy:bind
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
        // The trait body's members, value setters, and criteria, for the receiver.
        // @lfy def/model/main.lfy:bind
        // @lfy def/model/main.lfy:bind
        let Some(trait_node) = self.model.entities[trait_id].node else {
            return;
        };
        let trees = self.trees.clone();
        if let Some(block) = trees.child(trait_node, S::Block) {
            // Members declared in the trait body join the entity's scope.
            let trait_scope = self.model.entities[trait_id].scope;
            if let Some(trait_scope) = trait_scope {
                let members: Vec<SymbolId> = self.model.scopes[trait_scope]
                    .symbols
                    .iter()
                    .copied()
                    .filter(|&s| self.model.symbols[s].kind == SymbolKind::Member)
                    .collect();
                for member in members {
                    self.join_member(receiver, member);
                }
            }
            self.exec(block, &mut env);
        }
    }

    fn trait_env(&self, receiver: EntityId, trait_id: EntityId, values: &[Value]) -> Env {
        let trees = &self.trees;
        let mut vars = Vec::new();
        let parameters: Vec<SymbolId> = self.model.entities[trait_id].parameters().to_vec();
        let mut rest = values.iter();
        for (i, parameter) in parameters.iter().enumerate() {
            let symbol = &self.model.symbols[*parameter];
            let spread = trees.is(symbol.node, E::SpreadParameter);
            let value = if spread {
                Value::List(rest.by_ref().cloned().collect())
            } else {
                match rest.next() {
                    Some(value) => value.clone(),
                    None => self.default_of(symbol.node).unwrap_or(Value::Undefined),
                }
            };
            let _ = i;
            vars.push((symbol.name.clone(), value));
        }
        Env {
            vars,
            current: receiver,
            target: receiver,
            contributor: trait_id,
            scope: self.model.entities[trait_id].scope.unwrap_or(self.universe),
            in_trait: true,
            dry: false,
            ret: None,
        }
    }

    /// The default value node of a parameter, evaluated statically.
    pub(super) fn default_of(&self, parameter: NodeRef) -> Option<Value> {
        let trees = &self.trees;
        if !trees.has_token(parameter, P::PlainSetter) {
            return None;
        }
        let value = trees.child_nodes(parameter).into_iter().last()?;
        if trees.is(value, E::Name) || trees.is(value, E::DefinitionClause) {
            return None;
        }
        Some(Value::String(trees.raw(value).trim().to_string())).map(|v| match v {
            Value::String(s) if s == "[]" => Value::List(Vec::new()),
            Value::String(s) if s == "false" => Value::Bool(false),
            Value::String(s) if s == "true" => Value::Bool(true),
            Value::String(s) if s.starts_with('\'') || s.starts_with('"') => {
                Value::String(s[1..s.len() - 1].to_string())
            }
            Value::String(s) if s == "null" => Value::Null,
            Value::String(s) if s == "undefined" => Value::Undefined,
            other => other,
        })
    }

    // ---- Statements ---------------------------------------------------------------

    /// A Where adds a criterion: the conditions are the situation, the expression the
    /// behavior.
    // @lfy def/grammar/rules/statement.lfy:Where
    fn exec_where(&mut self, r: NodeRef, env: &mut Env) {
        let trees = self.trees.clone();
        let Some(conditions) = trees.child(r, S::Conditions) else {
            return;
        };
        let situation = self.conditions_text(conditions, env);
        let behavior = trees
            .child(r, S::ExpressionStatement)
            .and_then(|s| trees.child_nodes(s).into_iter().next())
            .map(|e| self.text_of(e, env));
        let criterion = Criterion {
            situation: Some(situation),
            behavior: behavior.map(|b| vec![b]),
            side_effects: None,
            contributor: env.contributor,
            node: Some(r),
        };
        self.model.entities[env.target]
            .acceptance_criteria
            .push(criterion);
    }

    /// A With runs the block with the name or member as the entity criteria attach to.
    // @lfy def/grammar/rules/statement.lfy:With
    fn exec_with(&mut self, r: NodeRef, env: &mut Env) {
        // A trait's own body is run without a receiver; its With blocks belong to each
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
        let trees = self.trees.clone();
        let children = trees.child_nodes(r);
        let Some(&expression) = children.iter().find(|&&c| !trees.is(c, S::Block)) else {
            return;
        };
        let Some(&block) = children.iter().find(|&&c| trees.is(c, S::Block)) else {
            return;
        };
        // The expression may be a Member using the scope accessor, which resolves to a
        // member symbol: the block's criteria and tests then attach to the member and
        // not to the entity that declares it.
        // @lfy def/model/main.lfy:bind
        let target = match self.eval(expression, env) {
            Value::Entity(entity) => entity,
            _ => match self.resolve_name_node(expression) {
                Some(symbol) => self.model.symbols[symbol].entity,
                None => return,
            },
        };
        // Decision: inside a trait body, `@` keeps meaning the receiver; the With only
        // redirects where criteria attach. Outside a trait, the With's entity is current.
        let mut inner = env.clone();
        inner.target = target;
        if !env.in_trait {
            inner.current = target;
            // Outside a trait the block's criteria are the target's own.
            // @lfy def/model/main.lfy:bind
            inner.contributor = target;
        }
        if let Some(scope) = self.model.scope_of(r) {
            inner.scope = scope;
        }
        self.exec(block, &mut inner);
        env.ret = inner.ret;
    }

    fn exec_if(&mut self, r: NodeRef, env: &mut Env) {
        let trees = self.trees.clone();
        let children = trees.child_nodes(r);
        let Some(&condition) = children.first() else {
            return;
        };
        let taken = self.eval(condition, env).is_truthy();
        if taken {
            if let Some(&branch) = children.get(1) {
                self.exec(branch, env);
            }
        } else if let Some(&else_node) = children.iter().find(|&&c| trees.is(c, S::Else))
            && let Some(branch) = trees.child_nodes(else_node).into_iter().next()
        {
            self.exec(branch, env);
        }
    }

    /// A loop over one or more iterables, visited in order.
    // @lfy def/grammar/rules/statement.lfy:For
    fn exec_for(&mut self, r: NodeRef, env: &mut Env) {
        let trees = self.trees.clone();
        let Some(block) = trees.child(r, S::Block) else {
            return;
        };
        let Some((names, rounds)) = self.loop_bindings(r, env) else {
            return;
        };
        let scope = self.model.scope_of(r).unwrap_or(env.scope);
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
            // Mutations of outer variables (push) flow back.
            for (n, v) in inner.vars.into_iter().take(env.vars.len()) {
                env.set(&n, v);
            }
        }
    }

    /// The names a loop declares and, for each round, what they hold: the values of its
    /// iterables (`in`), their names (`of`), or both (`from`).
    // @lfy def/grammar/rules/statement.lfy:ForFrom
    // @lfy def/model/main.lfy:bind
    pub(super) fn loop_bindings(
        &mut self,
        r: NodeRef,
        env: &mut Env,
    ) -> Option<(Vec<String>, Vec<Vec<Value>>)> {
        let trees = self.trees.clone();
        let declared = trees.child(r, E::Declared)?;
        let mut names = vec![self.declared_name(declared)?];
        let from = trees.child(r, S::ForFrom);
        let (iterables, keys) = match (trees.child(r, S::ForInOf), from) {
            (Some(in_of), _) => (
                trees.child_nodes(in_of),
                trees.has_token(in_of, K::OfKeyword),
            ),
            (None, Some(from)) => {
                let second = trees
                    .child(from, E::Declared)
                    .and_then(|d| self.declared_name(d));
                names.extend(second);
                (
                    trees
                        .child_nodes(from)
                        .into_iter()
                        .filter(|&c| !trees.is(c, E::Declared))
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
                // @lfy def/model/main.lfy:bind
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

    fn declared_name(&self, declared: NodeRef) -> Option<String> {
        let trees = &self.trees;
        trees
            .child_token(
                declared,
                crate::grammar::terminals::identifier::Identifier::Identifier,
            )
            .map(|i| trees.token(declared.file, i).value.clone())
    }

    fn iterate(&self, value: Value, keys: bool) -> Vec<Value> {
        match value {
            Value::List(items) => {
                if keys {
                    (0..items.len()).map(|i| Value::Number(i as f64)).collect()
                } else {
                    items
                }
            }
            Value::Object(pairs) => pairs
                .into_iter()
                .map(|(k, v)| if keys { Value::String(k) } else { v })
                .collect(),
            Value::Entity(entity) => {
                // A module: its symbols' entities (in) or names (of).
                // @lfy def/model/main.lfy:bind
                match self.model.entities[entity].scope {
                    Some(scope) => self.model.scopes[scope]
                        .symbols
                        .iter()
                        .map(|&s| {
                            if keys {
                                Value::String(self.model.symbols[s].name.clone())
                            } else {
                                Value::Entity(self.model.symbols[s].entity)
                            }
                        })
                        .collect(),
                    None => Vec::new(),
                }
            }
            Value::String(text) => text.chars().map(|c| Value::String(c.to_string())).collect(),
            _ => Vec::new(),
        }
    }

    fn exec_match(&mut self, r: NodeRef, env: &mut Env) {
        let trees = self.trees.clone();
        let all = trees.has_token(r, K::MatchallKeyword);
        let children = trees.child_nodes(r);
        let Some(&scrutinee) = children.first() else {
            return;
        };
        let value = self.eval(scrutinee, env);
        for &arm in children.iter().filter(|&&c| trees.is(c, S::MatchArm)) {
            let parts = trees.child_nodes(arm);
            let is_default = trees.has_token(arm, K::DefaultKeyword);
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
}
