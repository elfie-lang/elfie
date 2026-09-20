//! Compiled from `def/model/main.lfy` (the apply pass and the properties pass) and
//! `def/model/traits.lfy` (`applyingTraits`, `applyingExtensions`): executes declaration
//! and trait bodies at bind time, which is where traits are applied, members and values
//! join entities, criteria and tests are recorded, and templates are evaluated.

use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::Statement as S;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::grammar::terminals::literal::Literal as L;
use crate::grammar::terminals::punctuation::Punctuation as P;
use crate::grammar::{Entity as Rule, GrammarRule};

use super::bind::Binder;
use super::data::*;
use super::trees::Part;

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
        self.vars.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    fn set(&mut self, name: &str, value: Value) {
        if let Some(slot) = self.vars.iter_mut().rev().find(|(n, _)| n == name) {
            slot.1 = value;
        } else {
            self.vars.push((name.to_string(), value));
        }
    }
}

impl Binder {
    // ---- Apply pass ---------------------------------------------------------------

    /// Runs the top level of a file: applies the traits of each declaration, runs each
    /// declaration's body, and executes every other statement.
    // @lfy def/model/main.lfy:bind
    pub fn apply_file(&mut self, file: FileId) {
        let trees = self.trees.clone();
        let root = NodeRef { file, index: 0 };
        let entity = self.model.file_entities[file];
        let mut env = Env {
            vars: Vec::new(),
            current: entity,
            target: entity,
            contributor: entity,
            scope: self.model.file_scopes[file],
            in_trait: false,
            dry: false,
            ret: None,
        };
        for statement in trees.child_nodes(root) {
            self.exec(statement, &mut env);
        }
    }

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
        if matches!(kind, SymbolKind::Variable | SymbolKind::Alias | SymbolKind::External | SymbolKind::Module | SymbolKind::LoopVariable) {
            return;
        }
        // Parameters of the declaration.
        let parameters: Vec<SymbolId> = self.model.entities[entity]
            .scope
            .map(|scope| {
                self.model.scopes[scope]
                    .symbols
                    .iter()
                    .copied()
                    .filter(|&s| self.model.symbols[s].kind == SymbolKind::Parameter)
                    .collect()
            })
            .unwrap_or_default();
        match &mut self.model.entities[entity].kind {
            EntityKind::Fn { parameters: slot, .. } | EntityKind::Trait { parameters: slot, .. } => *slot = parameters,
            _ => {}
        }
        // Definition text.
        if let Some(definition) = self.model.entities[entity].definition_node {
            let own = Env { current: entity, target: entity, contributor: entity, ..env.clone() };
            let text = self.text_of(definition, &own);
            self.model.entities[entity].definition = Some(text);
        }
        // applyingTraits: the IsClause of the declaration or its signature or Declared.
        // @lfy def/model/traits.lfy:applyingTraits
        let clause_holder = trees.child(r, E::Signature).or_else(|| trees.child(r, E::Declared)).unwrap_or(r);
        if let Some(is_clause) = trees.child(clause_holder, E::IsClause) {
            self.apply_clause(entity, is_clause, false, env);
        }
        // applyingExtensions: the ExtendsClause.
        // @lfy def/model/traits.lfy:applyingExtensions
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
            } else if rule == S::Where.entity() || rule == S::With.entity() || rule == S::For.entity() || rule == S::If.entity() {
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
        let Some(uses) = trees.child(clause, E::TraitUses) else { return };
        for trait_use in trees.children_of(uses, E::TraitUse) {
            let Some(symbol) = self.resolve_trait_use(trait_use) else {
                continue; // reported by the resolve pass
            };
            let target = self.model.symbols[symbol].entity;
            let arguments = trees.child(trait_use, E::Arguments).map(|a| trees.arguments(a)).unwrap_or_default();
            if self.model.entities[target].is_trait() {
                let values = self.eval_arguments(&arguments, env);
                let source = if extends { AppliedSource::Extends(trait_use) } else { AppliedSource::Is(trait_use) };
                self.apply_trait(entity, target, arguments, values, source, extends);
            } else if extends {
                // Data extending data includes its members.
                // @lfy def/grammar/rules/expression.lfy:ExtendsClause
                self.include_members(entity, target, trait_use);
            } else {
                self.problem(trait_use, format!("{} is not a trait", self.model.entities[target].identifier.clone().unwrap_or_default()));
            }
        }
    }

    fn include_members(&mut self, receiver: EntityId, from: EntityId, node: NodeRef) {
        let Some(from_scope) = self.model.entities[from].scope else { return };
        let members: Vec<SymbolId> = self.model.scopes[from_scope]
            .symbols
            .iter()
            .copied()
            .filter(|&s| matches!(self.model.symbols[s].kind, SymbolKind::Member | SymbolKind::EnumMember))
            .collect();
        for member in members {
            self.join_member(receiver, member, node);
        }
    }

    /// A member declared in a trait joins the entity's scope; a second trait declaring
    /// the same member is a problem and the first wins.
    // @lfy def/model/main.lfy:bind
    fn join_member(&mut self, receiver: EntityId, member: SymbolId, at: NodeRef) {
        let scope = match self.model.entities[receiver].scope {
            Some(scope) => scope,
            None => {
                let owner = self.model.entities[receiver].node.unwrap_or(super::bind::SYNTHETIC);
                let scope = self.new_scope(None, owner, receiver);
                self.model.entities[receiver].scope = Some(scope);
                scope
            }
        };
        let name = self.model.symbols[member].name.clone();
        if let Some(existing) = self.model.lookup_local(scope, &name) {
            // Only two different traits declaring the same member is a problem; an own
            // member, or one inherited from data, overrides silently.
            let trees = self.trees.clone();
            let existing_trait = trees.ancestor(self.model.symbols[existing].node, S::TraitDeclaration);
            let new_trait = trees.ancestor(self.model.symbols[member].node, S::TraitDeclaration);
            if existing != member
                && self.model.symbols[existing].kind == SymbolKind::Member
                && existing_trait.is_some()
                && new_trait.is_some()
                && existing_trait != new_trait
            {
                self.problem(at, format!("two traits declare the member {name}; the first wins"));
            }
            return;
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
        self.model.scopes[scope].symbols.push(id);
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
        self.model.entities[receiver].traits.push(Applied { entity: trait_id, arguments, values: values.clone(), source });
        // The receiver joins the trait's entities, or its extenders.
        // @lfy def/model/main.lfy:bind
        if let EntityKind::Trait { entities, extenders, .. } = &mut self.model.entities[trait_id].kind {
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
            self.apply_trait(receiver, base, base_arguments, base_values, AppliedSource::Inherited(index), false);
        }
        // The trait body's members, value setters, and criteria, for the receiver.
        // @lfy def/model/main.lfy:bind
        let Some(trait_node) = self.model.entities[trait_id].node else { return };
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
                    let at = self.model.symbols[member].node;
                    self.join_member(receiver, member, at);
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
    fn default_of(&self, parameter: NodeRef) -> Option<Value> {
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
            Value::String(s) if s.starts_with('\'') || s.starts_with('"') => Value::String(s[1..s.len() - 1].to_string()),
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
        let Some(conditions) = trees.child(r, S::Conditions) else { return };
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
        self.model.entities[env.target].acceptance_criteria.push(criterion);
    }

    /// Conditions joined by "or" merge into one situation; conditions joined by "and" are
    /// a list of situations. A negated nested group distributes the negation.
    // @lfy def/grammar/rules/statement.lfy:Conditions
    fn conditions_text(&mut self, conditions: NodeRef, env: &mut Env) -> Vec<String> {
        let trees = self.trees.clone();
        let mut groups: Vec<Vec<String>> = vec![Vec::new()];
        for part in trees.parts(conditions) {
            match part {
                Part::Node(condition) => {
                    let text = self.condition_text(condition, env);
                    groups.last_mut().expect("group").extend(text);
                }
                Part::Token(index) if trees.token_is(conditions.file, index, K::AndKeyword) => groups.push(Vec::new()),
                Part::Token(_) => {}
            }
        }
        groups.into_iter().filter(|g| !g.is_empty()).map(|g| g.join(" or ")).collect()
    }

    fn condition_text(&mut self, condition: NodeRef, env: &mut Env) -> Vec<String> {
        let trees = self.trees.clone();
        let negated = trees.has_token(condition, P::LogicalNot);
        let Some(inner) = trees.child_nodes(condition).into_iter().next() else { return Vec::new() };
        let texts: Vec<String> = if trees.is(inner, S::ConditionGroup) {
            match trees.child(inner, S::Conditions) {
                Some(nested) => self.conditions_text(nested, env),
                None => Vec::new(),
            }
        } else if trees.is(inner, E::Group) {
            match trees.child_nodes(inner).into_iter().next() {
                Some(expression) => vec![self.text_of(expression, env)],
                None => Vec::new(),
            }
        } else {
            vec![self.text_of(inner, env)]
        };
        if negated {
            texts.into_iter().map(|t| format!("not ({t})")).collect()
        } else {
            texts
        }
    }

    /// A With runs the block with the name or member as the entity criteria attach to.
    // @lfy def/grammar/rules/statement.lfy:With
    fn exec_with(&mut self, r: NodeRef, env: &mut Env) {
        // A trait's own body is run without a receiver; its With blocks belong to each
        // application, so they are skipped here.
        if env.dry && !env.in_trait && matches!(self.model.entities[env.current].kind, EntityKind::Trait { .. }) {
            return;
        }
        let trees = self.trees.clone();
        let children = trees.child_nodes(r);
        let Some(&expression) = children.iter().find(|&&c| !trees.is(c, S::Block)) else { return };
        let Some(&block) = children.iter().find(|&&c| trees.is(c, S::Block)) else { return };
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
        let Some(&condition) = children.first() else { return };
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
        let Some(declared) = trees.child(r, E::Declared) else { return };
        let Some(name) = trees.child_token(declared, crate::grammar::terminals::identifier::Identifier::Identifier).map(|i| trees.token(r.file, i).value.clone()) else {
            return;
        };
        let Some(block) = trees.child(r, S::Block) else { return };
        let scope = self.model.scope_of(r).unwrap_or(env.scope);
        let (iterables, keys) = match (trees.child(r, S::ForInOf), trees.child(r, S::ForFrom)) {
            (Some(in_of), _) => (trees.child_nodes(in_of), trees.has_token(in_of, K::OfKeyword)),
            (None, Some(from)) => (trees.child_nodes(from).into_iter().filter(|&c| !trees.is(c, E::Declared)).collect(), false),
            _ => return,
        };
        let mut items = Vec::new();
        for iterable in iterables {
            let value = self.eval(iterable, env);
            items.extend(self.iterate(value, keys));
        }
        for item in items {
            let mut inner = env.clone();
            inner.scope = scope;
            inner.vars.push((name.clone(), item));
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

    fn iterate(&self, value: Value, keys: bool) -> Vec<Value> {
        match value {
            Value::List(items) => {
                if keys {
                    (0..items.len()).map(|i| Value::Number(i as f64)).collect()
                } else {
                    items
                }
            }
            Value::Object(pairs) => pairs.into_iter().map(|(k, v)| if keys { Value::String(k) } else { v }).collect(),
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
        let Some(&scrutinee) = children.first() else { return };
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

    // ---- Expressions --------------------------------------------------------------

    /// Evaluates an expression at bind time.
    pub(crate) fn eval(&mut self, r: NodeRef, env: &mut Env) -> Value {
        let trees = self.trees.clone();
        let rule = trees.rule(r);
        let file = r.file;
        if rule == E::Name.entity() {
            let Some(name) = trees.name(r) else { return Value::Undefined };
            if let Some(value) = env.get(&name) {
                return value.clone();
            }
            return match self.lookup(env.scope, &name).or_else(|| self.lookup(self.model.enclosing_scope(r), &name)) {
                Some(symbol) => self.value_of_symbol(symbol),
                None => Value::Undefined,
            };
        }
        if rule == E::StringLiteral.entity() {
            return Value::String(trees.string_value(r).unwrap_or_default());
        }
        if rule == E::Template.entity() {
            return Value::String(self.template_text(r, env));
        }
        if rule == E::Number.entity() {
            let text: String = trees.raw(r).trim().replace('_', "");
            return Value::Number(text.parse().unwrap_or(0.0));
        }
        if rule == E::Boolean.entity() {
            return Value::Bool(trees.raw(r).trim() == "true");
        }
        if rule == E::Nullish.entity() {
            return if trees.raw(r).trim() == "null" { Value::Null } else { Value::Undefined };
        }
        if rule == E::PrimitiveType.entity() {
            return Value::Type(Box::new(self.type_from_expression(r)));
        }
        if rule == E::List.entity() {
            let items = trees.child(r, E::Items).map(|i| trees.items(i)).unwrap_or_default();
            return Value::List(self.eval_arguments(&items, env));
        }
        if rule == E::Object.entity() {
            let mut pairs = Vec::new();
            for key in trees.children_of(r, E::ObjectKey) {
                let name = trees
                    .child(key, E::Declared)
                    .and_then(|d| trees.child_token(d, crate::grammar::terminals::identifier::Identifier::Identifier))
                    .map(|i| trees.token(file, i).value.clone())
                    .unwrap_or_default();
                let value = trees.child_nodes(key).into_iter().find(|&c| !trees.is(c, E::Declared)).map_or(Value::Undefined, |v| self.eval(v, env));
                pairs.push((name, value));
            }
            return Value::Object(pairs);
        }
        if rule == E::Group.entity() {
            return trees.child_nodes(r).into_iter().next().map_or(Value::Undefined, |inner| self.eval(inner, env));
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
            return Value::Closure(r, env.vars.clone());
        }
        if rule == E::Dereference.entity() {
            let Some(operand) = trees.child_nodes(r).into_iter().next() else { return Value::Undefined };
            return match self.eval(operand, env) {
                Value::Entity(e) => Value::Entity(e),
                _ => match self.resolve_name_node(operand) {
                    Some(symbol) => Value::Entity(self.model.symbols[symbol].entity),
                    None => Value::Undefined,
                },
            };
        }
        if rule == E::NotOperation.entity() {
            let operand = trees.child_nodes(r).into_iter().next();
            return Value::Bool(!operand.is_some_and(|o| self.eval(o, env).is_truthy()));
        }
        if rule == E::NegateOperation.entity() {
            let operand = trees.child_nodes(r).into_iter().next();
            return match operand.map(|o| self.eval(o, env)) {
                Some(Value::Number(n)) => Value::Number(-n),
                _ => Value::Undefined,
            };
        }
        if rule == E::SpreadOperation.entity() {
            let operand = trees.child_nodes(r).into_iter().next();
            return operand.map_or(Value::Undefined, |o| self.eval(o, env));
        }
        if rule == E::Traits.entity() {
            // `x is T`: whether the trait is applied.
            let Some(left) = trees.left(r) else { return Value::Bool(false) };
            let value = self.eval(left, env);
            let Value::Entity(entity) = value else { return Value::Bool(false) };
            let uses = trees.child(r, E::TraitUses).map(|u| trees.children_of(u, E::TraitUse)).unwrap_or_default();
            let all = uses.iter().all(|&u| {
                self.resolve_trait_use(u)
                    .map(|s| self.model.symbols[s].entity)
                    .is_some_and(|t| self.model.entities[entity].has_trait(t))
            });
            return Value::Bool(all && !uses.is_empty());
        }
        if rule == E::Conditional.entity() {
            let parts = trees.child_nodes(r);
            if parts.len() == 3 {
                return if self.eval(parts[0], env).is_truthy() { self.eval(parts[1], env) } else { self.eval(parts[2], env) };
            }
            return Value::Undefined;
        }
        if rule == E::Assignment.entity() {
            return self.eval_assignment(r, env);
        }
        if rule == E::Definition.entity() {
            return trees.child_nodes(r).into_iter().next().map_or(Value::Undefined, |left| self.eval(left, env));
        }
        if rule == E::Cast.entity() {
            return trees.child_nodes(r).into_iter().next().map_or(Value::Undefined, |left| self.eval(left, env));
        }
        if rule == E::Index.entity() {
            let parts = trees.child_nodes(r);
            if parts.len() == 1 {
                return Value::Type(Box::new(self.type_from_expression(r)));
            }
            let left = self.eval(parts[0], env);
            let index = self.eval(parts[1], env);
            return match (left, index) {
                (Value::List(items), Value::Number(n)) => items.get(n as usize).cloned().unwrap_or(Value::Undefined),
                (Value::Object(pairs), Value::String(key)) => pairs.into_iter().find(|(k, _)| *k == key).map_or(Value::Undefined, |(_, v)| v),
                _ => Value::Undefined,
            };
        }
        if rule == E::TypeExpression.entity() || rule == E::TypePredicate.entity() {
            return Value::Type(Box::new(self.type_from_expression(r)));
        }
        if rule.is_infix() {
            return self.eval_infix(r, env);
        }
        if rule == E::AwaitOperation.entity() || rule == E::InOperation.entity() || rule == E::OfOperation.entity() || rule == E::FromOperation.entity() {
            return trees.child_nodes(r).into_iter().next().map_or(Value::Undefined, |o| self.eval(o, env));
        }
        Value::Undefined
    }

    fn eval_infix(&mut self, r: NodeRef, env: &mut Env) -> Value {
        let trees = self.trees.clone();
        let rule = trees.rule(r);
        let parts = trees.child_nodes(r);
        if parts.len() != 2 {
            return Value::Undefined;
        }
        if rule == E::LogicalAndOperation.entity() {
            let left = self.eval(parts[0], env);
            return if left.is_truthy() { self.eval(parts[1], env) } else { left };
        }
        if rule == E::CoalescenceOperation.entity() {
            let left = self.eval(parts[0], env);
            let operator = trees.parts(r).into_iter().find_map(|p| match p {
                Part::Token(i) => Some(i),
                _ => None,
            });
            let nullish = operator.is_some_and(|i| trees.token_is(r.file, i, P::NullishOr));
            let take_right = if nullish { matches!(left, Value::Null | Value::Undefined) } else { !left.is_truthy() };
            return if take_right { self.eval(parts[1], env) } else { left };
        }
        let left = self.eval(parts[0], env);
        let right = self.eval(parts[1], env);
        if rule == E::EqualityOperation.entity() {
            let operator = trees.parts(r).into_iter().find_map(|p| match p {
                Part::Token(i) => Some(i),
                _ => None,
            });
            let equal = left == right;
            let negated = operator.is_some_and(|i| trees.token_is(r.file, i, P::NotEqual));
            return Value::Bool(equal != negated);
        }
        if rule == E::AdditiveOperation.entity() {
            let operator = trees.parts(r).into_iter().find_map(|p| match p {
                Part::Token(i) => Some(i),
                _ => None,
            });
            let minus = operator.is_some_and(|i| trees.token_is(r.file, i, P::Minus));
            return match (left, right) {
                (Value::Number(a), Value::Number(b)) => Value::Number(if minus { a - b } else { a + b }),
                (a, b) if !minus => Value::String(format!("{}{}", self.to_text(&a), self.to_text(&b))),
                _ => Value::Undefined,
            };
        }
        if rule == E::BitwiseOrOperation.entity() {
            // A union when both sides are types.
            if let (Value::Type(a), Value::Type(b)) = (&left, &right) {
                let mut items = match (**a).clone() {
                    TypeRef::Union(items) => items,
                    other => vec![other],
                };
                match (**b).clone() {
                    TypeRef::Union(more) => items.extend(more),
                    other => items.push(other),
                }
                return Value::Type(Box::new(TypeRef::Union(items)));
            }
            return Value::Undefined;
        }
        if rule == E::MultiplicativeOperation.entity() || rule == E::PowerOperation.entity() {
            if let (Value::Number(a), Value::Number(b)) = (left, right) {
                let operator = trees.parts(r).into_iter().find_map(|p| match p {
                    Part::Token(i) => Some(i),
                    _ => None,
                });
                let text = operator.map(|i| trees.token(r.file, i).value.clone()).unwrap_or_default();
                return Value::Number(match text.as_str() {
                    "*" => a * b,
                    "/" => a / b,
                    "%" => a % b,
                    "**" => a.powf(b),
                    _ => a,
                });
            }
            return Value::Undefined;
        }
        if rule == E::RelationalOperation.entity() {
            if let (Value::Number(a), Value::Number(b)) = (left, right) {
                let operator = trees.parts(r).into_iter().find_map(|p| match p {
                    Part::Token(i) => Some(i),
                    _ => None,
                });
                let text = operator.map(|i| trees.token(r.file, i).value.clone()).unwrap_or_default();
                return Value::Bool(match text.as_str() {
                    "<" => a < b,
                    "<=" => a <= b,
                    ">" => a > b,
                    ">=" => a >= b,
                    _ => false,
                });
            }
            return Value::Undefined;
        }
        Value::Undefined
    }

    /// `.name = value` sets a value on the current entity; `name = value` rebinds a
    /// variable.
    fn eval_assignment(&mut self, r: NodeRef, env: &mut Env) -> Value {
        let trees = self.trees.clone();
        let parts = trees.child_nodes(r);
        if parts.len() != 2 {
            return Value::Undefined;
        }
        let (left, right) = (parts[0], parts[1]);
        // A member declaration: `$name: desc = Type;` declares, it does not assign.
        if trees.is(left, E::Definition) || self.model.symbol_of(left).is_some() {
            return Value::Undefined;
        }
        if trees.is(left, E::Current) {
            let Some((accessor, Some(name))) = trees.accessor_and_name(left) else { return Value::Undefined };
            if trees.token_is(left.file, accessor, P::ScopeAccessor) {
                return Value::Undefined;
            }
            let value = self.eval(right, env);
            if !env.dry {
                let name = trees.token(left.file, name).value.clone();
                self.model.entities[env.current].values.push((name, value.clone()));
            }
            return value;
        }
        if trees.is(left, E::Name) {
            let value = self.eval(right, env);
            if let Some(name) = trees.name(left) {
                env.set(&name, value.clone());
            }
            return value;
        }
        Value::Undefined
    }

    /// `@prop`, `$member`, `.member`, `$&`, or a bare accessor on the current entity.
    fn eval_current(&mut self, r: NodeRef, env: &mut Env) -> Value {
        let trees = self.trees.clone();
        let Some((accessor, name)) = trees.accessor_and_name(r) else { return Value::Undefined };
        let name = name.map(|i| trees.token(r.file, i).value.clone());
        let current = env.current;
        self.access(Value::Entity(current), trees.token(r.file, accessor).rule.expect("accessor"), name.as_deref(), env)
    }

    fn eval_member(&mut self, r: NodeRef, env: &mut Env) -> Value {
        let trees = self.trees.clone();
        let Some(left) = trees.left(r) else { return Value::Undefined };
        let Some((accessor, name)) = trees.accessor_and_name(r) else { return Value::Undefined };
        let name = name.map(|i| trees.token(r.file, i).value.clone());
        let value = self.eval(left, env);
        self.access(value, trees.token(r.file, accessor).rule.expect("accessor"), name.as_deref(), env)
    }

    /// Reads one layer of a value.
    fn access(&mut self, value: Value, accessor: Rule, name: Option<&str>, env: &mut Env) -> Value {
        let layer = super::components::accessor_layer(accessor).expect("accessor");
        match layer {
            Layer::Context => {
                let Some(name) = name else { return value };
                self.context_property(&value, name)
            }
            Layer::Scope => {
                let Value::Entity(entity) = value else { return Value::Undefined };
                match name {
                    None => self.model.entities[entity].scope.map_or(Value::Undefined, Value::Scope),
                    Some(name) => self.member_value(entity, name, env),
                }
            }
            Layer::Parent => {
                let Value::Entity(entity) = value else { return Value::Undefined };
                let scope = self.model.entities[entity].symbol.map(|s| self.model.symbols[s].scope);
                scope.map_or(Value::Undefined, Value::Scope)
            }
            Layer::Value => {
                let Some(name) = name else { return value };
                match value {
                    Value::Entity(entity) => self.member_value(entity, name, env),
                    Value::Object(pairs) => pairs.into_iter().find(|(k, _)| k == name).map_or(Value::Undefined, |(_, v)| v),
                    Value::List(items) if name == "length" => Value::Number(items.len() as f64),
                    Value::String(text) if name == "length" => Value::Number(text.chars().count() as f64),
                    _ => Value::Undefined,
                }
            }
            Layer::Dereference | Layer::Previous => Value::Undefined,
        }
    }

    /// A context property of a value.
    fn context_property(&mut self, value: &Value, name: &str) -> Value {
        match value {
            Value::Entity(entity) => {
                let entity = *entity;
                match ContextProperty::lookup(name) {
                    Some(ContextProperty::Identifier) => self.model.entities[entity].identifier.clone().map_or(Value::Undefined, Value::String),
                    Some(ContextProperty::Definition) => {
                        if self.model.entities[entity].definition.is_none()
                            && let Some(node) = self.model.entities[entity].definition_node
                        {
                            let scope = self.model.entities[entity].scope.unwrap_or(self.universe);
                            let env = Env { vars: Vec::new(), current: entity, target: entity, contributor: entity, scope, in_trait: false, dry: true, ret: None };
                            let text = self.text_of(node, &env);
                            self.model.entities[entity].definition = Some(text);
                        }
                        self.model.entities[entity].definition.clone().map_or(Value::Undefined, Value::String)
                    }
                    Some(ContextProperty::Type) => self.model.entities[entity].ty.clone().map_or(Value::Undefined, |t| Value::Type(Box::new(t))),
                    Some(ContextProperty::AcceptanceCriteria) => Value::Criteria(entity),
                    Some(ContextProperty::Test) => Value::Tester(entity),
                    Some(ContextProperty::Tests) => Value::List(Vec::new()),
                    Some(ContextProperty::Entities) => Value::List(self.model.entities[entity].entities().iter().map(|&e| Value::Entity(e)).collect()),
                    Some(ContextProperty::Extenders) => Value::List(self.model.entities[entity].extenders().iter().map(|&e| Value::Entity(e)).collect()),
                    Some(ContextProperty::Parameters) => Value::List(
                        self.model.entities[entity].parameters().iter().map(|&s| Value::Entity(self.model.symbols[s].entity)).collect(),
                    ),
                    Some(ContextProperty::Output) => self.model.entities[entity].output().cloned().map_or(Value::Undefined, |t| Value::Type(Box::new(t))),
                    Some(ContextProperty::ItemType) => self.model.entities[entity].item_type.clone().map_or(Value::Undefined, |t| Value::Type(Box::new(t))),
                    Some(ContextProperty::References) => Value::List(Vec::new()),
                    Some(ContextProperty::Like) => Value::Like(Box::new(value.clone())),
                    None => Value::Undefined,
                }
            }
            Value::Type(ty) => match name {
                "type" => Value::Type(ty.clone()),
                "like" => Value::Like(Box::new(value.clone())),
                "itemType" => match &**ty {
                    TypeRef::List(item) => Value::Type(item.clone()),
                    _ => Value::Undefined,
                },
                _ => Value::Undefined,
            },
            Value::String(_) => match name {
                "type" => Value::Type(Box::new(TypeRef::Primitive("string"))),
                "like" => Value::Like(Box::new(value.clone())),
                _ => Value::Undefined,
            },
            Value::Number(_) => match name {
                "type" => Value::Type(Box::new(TypeRef::Primitive("number"))),
                _ => Value::Undefined,
            },
            Value::Bool(_) => match name {
                "type" => Value::Type(Box::new(TypeRef::Primitive("boolean"))),
                _ => Value::Undefined,
            },
            Value::List(items) => match name {
                "type" => Value::Type(Box::new(TypeRef::List(Box::new(TypeRef::Unknown(String::new()))))),
                "like" => Value::Like(Box::new(value.clone())),
                "itemType" => items.first().map_or(Value::Undefined, |first| self.context_property(first, "type")),
                _ => Value::Undefined,
            },
            _ => match name {
                "like" => Value::Like(Box::new(value.clone())),
                _ => Value::Undefined,
            },
        }
    }

    /// The value of a member of an entity: what a setter gave it, or its declared value.
    fn member_value(&mut self, entity: EntityId, name: &str, env: &mut Env) -> Value {
        if let Some(value) = self.model.entities[entity].value(name) {
            return value.clone();
        }
        let symbol = self.member_symbol(entity, name).or_else(|| {
            self.model.entities[entity]
                .traits
                .iter()
                .find_map(|applied| self.member_symbol(applied.entity, name))
        });
        let Some(symbol) = symbol else {
            // A file's symbols.
            if let EntityKind::File = self.model.entities[entity].kind {
                return Value::Undefined;
            }
            return Value::Undefined;
        };
        let symbol_entity = self.model.symbols[symbol].entity;
        match self.model.symbols[symbol].kind {
            SymbolKind::Member => {
                // The declared value: what follows `=` in the member statement.
                let node = self.model.symbols[symbol].node;
                let trees = self.trees.clone();
                if trees.is(node, E::TypeKey) {
                    return Value::Type(Box::new(self.model.entities[symbol_entity].ty.clone().unwrap_or(TypeRef::Unknown(String::new()))));
                }
                let expression = trees.child_nodes(node).into_iter().next();
                if let Some(expression) = expression
                    && trees.is(expression, E::Assignment)
                    && let Some(right) = trees.child_nodes(expression).get(1).copied()
                {
                    let mut inner = env.clone();
                    inner.scope = self.model.symbols[symbol].scope;
                    return self.eval(right, &mut inner);
                }
                Value::Undefined
            }
            _ => self.value_of_symbol(symbol),
        }
    }

    /// The value a symbol stands for when its name is read.
    pub(crate) fn value_of_symbol(&mut self, symbol: SymbolId) -> Value {
        let entity = self.model.symbols[symbol].entity;
        match self.model.symbols[symbol].kind {
            SymbolKind::Variable | SymbolKind::Alias | SymbolKind::External => self.value_of_variable(entity),
            SymbolKind::EnumMember => {
                if let Some(value) = self.cache.get(&entity) {
                    return value.clone();
                }
                let trees = self.trees.clone();
                let node = self.model.symbols[symbol].node;
                let value_node = trees.child_nodes(node).into_iter().find(|&c| !trees.is(c, E::Declared));
                let scope = self.model.symbols[symbol].scope;
                let mut env = Env { vars: Vec::new(), current: entity, target: entity, contributor: entity, scope, in_trait: false, dry: true, ret: None };
                let value = value_node.map_or(Value::Undefined, |v| self.eval(v, &mut env));
                self.cache.insert(entity, value.clone());
                value
            }
            SymbolKind::Module => Value::Entity(entity),
            SymbolKind::Parameter | SymbolKind::LoopVariable | SymbolKind::Member => Value::Undefined,
            _ => Value::Entity(entity),
        }
    }

    fn value_of_variable(&mut self, entity: EntityId) -> Value {
        if let Some(value) = self.cache.get(&entity) {
            return value.clone();
        }
        if self.evaluating.contains(&entity) {
            return Value::Undefined;
        }
        let trees = self.trees.clone();
        let Some(node) = self.model.entities[entity].node else { return Value::Undefined };
        let value_node = trees.child_nodes(node).into_iter().find(|&c| !trees.is(c, E::Declared));
        let Some(value_node) = value_node else { return Value::Undefined };
        let scope = self.model.enclosing_scope(node);
        let current = self.model.scopes[scope].current;
        self.evaluating.insert(entity);
        let mut env = Env { vars: Vec::new(), current, target: current, contributor: current, scope, in_trait: false, dry: true, ret: None };
        let value = self.eval(value_node, &mut env);
        self.evaluating.remove(&entity);
        self.cache.insert(entity, value.clone());
        value
    }

    /// Calls: `add` on criteria, `@test`, `apply` on a trait, the list and string methods,
    /// and declared or inline functions.
    fn eval_call(&mut self, r: NodeRef, env: &mut Env) -> Value {
        let trees = self.trees.clone();
        let Some(callee) = trees.left(r) else { return Value::Undefined };
        let arguments = trees.arguments(r);
        // Method calls.
        if trees.is(callee, E::Member)
            && let (Some(left), Some((accessor, Some(name)))) = (trees.left(callee), trees.accessor_and_name(callee))
            && (trees.token_is(callee.file, accessor, P::ValueAccessor) || trees.token_is(callee.file, accessor, P::OptionalValueAccessor))
        {
            let method = trees.token(callee.file, name).value.clone();
            let receiver = self.eval(left, env);
            match (&receiver, method.as_str()) {
                (Value::Criteria(target), "add") => {
                    // An add Call on the entity's context appends one Criterion.
                    // @lfy def/model/main.lfy:bind
                    let target = *target;
                    for argument in arguments {
                        let criterion = self.criterion_of(argument, env);
                        let target = if env.in_trait && target == env.current { env.target } else { target };
                        self.model.entities[target].acceptance_criteria.push(criterion);
                    }
                    return receiver;
                }
                (Value::Entity(trait_id), "apply") if self.model.entities[*trait_id].is_trait() => {
                    // The trait is applied to the entity the first argument resolves to.
                    // @lfy def/model/main.lfy:bind
                    let trait_id = *trait_id;
                    if env.dry {
                        return Value::Undefined;
                    }
                    let Some((&first, rest)) = arguments.split_first() else { return Value::Undefined };
                    let receivers = self.apply_receivers(first, env);
                    let values = self.eval_arguments(rest, env);
                    for receiver in receivers {
                        self.apply_trait(receiver, trait_id, rest.to_vec(), values.clone(), AppliedSource::Apply(r), false);
                    }
                    return Value::Undefined;
                }
                (Value::List(items), "map") => {
                    let function = arguments.first().map(|&a| self.eval(a, env));
                    let Some(function) = function else { return Value::Undefined };
                    let items = items.clone();
                    let mapped = items.into_iter().map(|item| self.call_value(&function, vec![item], env)).collect();
                    return Value::List(mapped);
                }
                (Value::List(items), "join") => {
                    let separator = arguments.first().map_or(Value::String(",".into()), |&a| self.eval(a, env));
                    let separator = self.to_text(&separator);
                    let texts: Vec<String> = items.iter().map(|i| self.to_text(i)).collect();
                    return Value::String(texts.join(&separator));
                }
                (Value::List(items), "push") => {
                    let mut items = items.clone();
                    for &argument in &arguments {
                        let value = self.eval(argument, env);
                        items.push(value);
                    }
                    if let Some(name) = trees.name(left) {
                        env.set(&name, Value::List(items.clone()));
                    }
                    return Value::Number(items.len() as f64);
                }
                (Value::List(items), "includes") => {
                    let needle = arguments.first().map_or(Value::Undefined, |&a| self.eval(a, env));
                    return Value::Bool(items.contains(&needle));
                }
                (Value::String(text), "includes") => {
                    let needle = arguments.first().map_or(Value::Undefined, |&a| self.eval(a, env));
                    return Value::Bool(text.contains(&self.to_text(&needle)));
                }
                (Value::String(text), "trim") => return Value::String(text.trim().to_string()),
                (Value::String(text), "split") => {
                    let separator = arguments.first().map_or(Value::String(",".into()), |&a| self.eval(a, env));
                    let separator = self.to_text(&separator);
                    return Value::List(text.split(separator.as_str()).map(|s| Value::String(s.to_string())).collect());
                }
                (Value::Entity(_), _) | (Value::Object(_), _) => {
                    let function = self.access(receiver.clone(), P::ValueAccessor.entity(), Some(&method), env);
                    let values = self.eval_arguments(&arguments, env);
                    return self.call_value(&function, values, env);
                }
                _ => return Value::Undefined,
            }
        }
        let function = self.eval(callee, env);
        match function {
            Value::Tester(target) => {
                // A test Call appends each argument as one Test.
                // @lfy def/model/main.lfy:bind
                let target = if env.in_trait && target == env.current { env.target } else { target };
                for argument in arguments {
                    let test = self.test_of(argument);
                    self.model.entities[target].tests.push(test);
                }
                Value::Undefined
            }
            Value::Like(_) => {
                // `X@like(prompt)`: an instance the prompt describes; at bind time, the
                // prompt itself.
                arguments.first().map_or(Value::Undefined, |&a| self.eval(a, env))
            }
            Value::Criteria(_) => Value::Undefined,
            other => {
                let values = self.eval_arguments(&arguments, env);
                self.call_value(&other, values, env)
            }
        }
    }

    /// The entities an apply call targets: the first argument's entity, every item of an
    /// alternationList, or every item a loop variable stands for.
    fn apply_receivers(&mut self, first: NodeRef, env: &mut Env) -> Vec<EntityId> {
        let trees = self.trees.clone();
        let value = self.eval(first, env);
        let entity = match value {
            Value::Entity(entity) => Some(entity),
            _ => self.resolve_name_node(first).map(|s| self.model.symbols[s].entity),
        };
        let Some(entity) = entity else {
            if !trees.is(first, E::Current) {
                self.problem(first, "apply needs an entity as its first argument");
            }
            return Vec::new();
        };
        // An alternationList: every item.
        // @lfy def/model/main.lfy:bind
        if let Some(alternation) = self.model.trait_named("alternationList")
            && let Some(applied) = self.model.entities[entity].traits.iter().find(|a| a.entity == alternation).cloned()
        {
            let mut items = Vec::new();
            for value in &applied.values {
                if let Value::Entity(e) = value {
                    items.push(*e);
                }
            }
            if !items.is_empty() {
                return items;
            }
        }
        vec![entity]
    }

    /// Calls a function value with values.
    fn call_value(&mut self, function: &Value, values: Vec<Value>, env: &mut Env) -> Value {
        let trees = self.trees.clone();
        match function {
            Value::Closure(node, captured) => {
                let mut inner = env.clone();
                inner.vars = captured.clone();
                inner.ret = None;
                self.bind_parameters(*node, &values, &mut inner);
                let body = trees.child_nodes(*node).into_iter().last();
                let Some(body) = body else { return Value::Undefined };
                if trees.is(body, S::Block) {
                    self.exec(body, &mut inner);
                    inner.ret.unwrap_or(Value::Undefined)
                } else {
                    self.eval(body, &mut inner)
                }
            }
            Value::Entity(entity) => {
                let entity = *entity;
                let Some(node) = self.model.entities[entity].node else { return Value::Undefined };
                if !matches!(self.model.entities[entity].kind, EntityKind::Fn { .. }) {
                    return Value::Undefined;
                }
                let scope = self.model.entities[entity].scope.unwrap_or(self.universe);
                let mut inner = Env { vars: Vec::new(), current: entity, target: entity, contributor: entity, scope, in_trait: false, dry: true, ret: None };
                self.bind_parameters(node, &values, &mut inner);
                let Some(body) = trees.child(node, S::Block) else { return Value::Undefined };
                // Run the body as code: every statement, variables included.
                self.exec_code(body, &mut inner);
                inner.ret.unwrap_or(Value::Undefined)
            }
            _ => Value::Undefined,
        }
    }

    /// Runs a block as code: variable declarations bind, loops run, returns return.
    fn exec_code(&mut self, block: NodeRef, env: &mut Env) {
        let trees = self.trees.clone();
        let scope = self.model.scope_of(block).unwrap_or(env.scope);
        let saved = env.scope;
        env.scope = scope;
        for statement in trees.child_nodes(block) {
            if env.ret.is_some() {
                break;
            }
            if trees.is(statement, S::VariableDeclaration) {
                let name = trees
                    .child(statement, E::Declared)
                    .and_then(|d| trees.child_token(d, crate::grammar::terminals::identifier::Identifier::Identifier))
                    .map(|i| trees.token(statement.file, i).value.clone());
                let value = trees.child_nodes(statement).into_iter().find(|&c| !trees.is(c, E::Declared)).map_or(Value::Undefined, |v| self.eval(v, env));
                if let Some(name) = name {
                    env.vars.push((name, value));
                }
            } else if trees.is(statement, S::Block) {
                self.exec_code(statement, env);
            } else if trees.is(statement, S::For) {
                // A loop body may declare variables and push to outer ones.
                self.exec_for_code(statement, env);
            } else {
                self.exec(statement, env);
            }
        }
        env.scope = saved;
    }

    fn exec_for_code(&mut self, r: NodeRef, env: &mut Env) {
        let trees = self.trees.clone();
        let Some(declared) = trees.child(r, E::Declared) else { return };
        let Some(name) = trees.child_token(declared, crate::grammar::terminals::identifier::Identifier::Identifier).map(|i| trees.token(r.file, i).value.clone()) else {
            return;
        };
        let Some(block) = trees.child(r, S::Block) else { return };
        let (iterables, keys) = match trees.child(r, S::ForInOf) {
            Some(in_of) => (trees.child_nodes(in_of), trees.has_token(in_of, K::OfKeyword)),
            None => return,
        };
        let mut items = Vec::new();
        for iterable in iterables {
            let value = self.eval(iterable, env);
            items.extend(self.iterate(value, keys));
        }
        let outer = env.vars.len();
        for item in items {
            env.vars.push((name.clone(), item));
            self.exec_code(block, env);
            env.vars.truncate(outer);
            if env.ret.is_some() {
                return;
            }
        }
    }

    fn bind_parameters(&mut self, function: NodeRef, values: &[Value], env: &mut Env) {
        let trees = self.trees.clone();
        let Some(parameters) = trees.child(function, E::Parameters).or_else(|| trees.child(function, E::Signature).and_then(|s| trees.child(s, E::Parameters))) else {
            return;
        };
        let mut rest = values.iter();
        for parameter in trees.child_nodes(parameters) {
            let spread = trees.is(parameter, E::SpreadParameter);
            let Some(name) = trees.child(parameter, E::Name).and_then(|n| trees.name(n)) else { continue };
            let value = if spread {
                Value::List(rest.by_ref().cloned().collect())
            } else {
                match rest.next() {
                    Some(value) => value.clone(),
                    None => self.default_of(parameter).unwrap_or(Value::Undefined),
                }
            };
            env.vars.push((name, value));
        }
    }

    // ---- Criteria and tests -------------------------------------------------------

    /// One criterion from the object given to `add`.
    fn criterion_of(&mut self, argument: NodeRef, env: &mut Env) -> Criterion {
        let object = self.eval(argument, env);
        let mut criterion = Criterion { contributor: env.contributor, node: Some(argument), ..Criterion::default() };
        if let Value::Object(pairs) = object {
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

    fn texts_of(&self, value: &Value) -> Vec<String> {
        match value {
            Value::List(items) => items.iter().map(|i| self.to_text(i)).collect(),
            other => vec![self.to_text(other)],
        }
    }

    /// One test from the object given to `@test`.
    fn test_of(&self, argument: NodeRef) -> Test {
        let trees = &self.trees;
        let mut test = Test { input: None, expect: None, input_text: String::new(), expect_text: String::new() };
        if trees.is(argument, E::Object) {
            for key in trees.children_of(argument, E::ObjectKey) {
                let name = trees
                    .child(key, E::Declared)
                    .and_then(|d| trees.child_token(d, crate::grammar::terminals::identifier::Identifier::Identifier))
                    .map(|i| trees.token(argument.file, i).value.clone())
                    .unwrap_or_default();
                let value = trees.child_nodes(key).into_iter().find(|&c| !trees.is(c, E::Declared));
                match name.as_str() {
                    "input" => {
                        test.input = value;
                        test.input_text = value.map(|v| trees.raw(v).trim().to_string()).unwrap_or_default();
                    }
                    "expect" => {
                        test.expect = value;
                        test.expect_text = value.map(|v| trees.raw(v).trim().to_string()).unwrap_or_default();
                    }
                    _ => {}
                }
            }
        }
        test
    }

    // ---- Text -----------------------------------------------------------------------

    /// The text of a string or template expression, evaluated; other expressions give
    /// their value as text, or their source when they have none.
    pub(crate) fn text_of(&mut self, r: NodeRef, env: &Env) -> String {
        let mut env = env.clone();
        let value = self.eval(r, &mut env);
        match value {
            Value::Undefined => self.trees.raw(r).trim().to_string(),
            other => self.to_text(&other),
        }
    }

    pub(crate) fn to_text(&self, value: &Value) -> String {
        match value {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => {
                if n.fract() == 0.0 { format!("{}", *n as i64) } else { n.to_string() }
            }
            Value::String(s) => s.clone(),
            Value::List(items) => items.iter().map(|i| self.to_text(i)).collect::<Vec<_>>().join(", "),
            Value::Object(pairs) => pairs.iter().map(|(k, v)| format!("{k} = {}", self.to_text(v))).collect::<Vec<_>>().join(", "),
            Value::Entity(e) => self.model.entities[*e].identifier.clone().unwrap_or_else(|| "anonymous".to_string()),
            Value::Type(ty) => self.type_text(ty),
            Value::Scope(_) => "scope".to_string(),
            Value::Closure(..) | Value::Function(..) => "function".to_string(),
            Value::Criteria(_) => "acceptanceCriteria".to_string(),
            Value::Tester(_) => "test".to_string(),
            Value::Like(inner) => self.to_text(inner),
        }
    }

    pub(crate) fn type_text(&self, ty: &TypeRef) -> String {
        match ty {
            TypeRef::Entity(e) => self.model.entities[*e].identifier.clone().unwrap_or_else(|| "anonymous".to_string()),
            TypeRef::Primitive(p) => (*p).to_string(),
            TypeRef::Literal(v) => self.to_text(v),
            TypeRef::List(item) => format!("{}[]", self.type_text(item)),
            TypeRef::Union(items) => items.iter().map(|i| self.type_text(i)).collect::<Vec<_>>().join(" | "),
            TypeRef::Predicate(t) => format!("is {}", self.model.entities[*t].identifier.clone().unwrap_or_default()),
            TypeRef::Function => "function".to_string(),
            TypeRef::Unknown(text) => text.trim().to_string(),
        }
    }

    /// A template's text: bodies as written, references rendered, executions evaluated.
    fn template_text(&mut self, r: NodeRef, env: &mut Env) -> String {
        let trees = self.trees.clone();
        let node = trees.node(r);
        let tokens = trees.tokens(r.file);
        let mut out = String::new();
        for child in &node.children {
            match child {
                crate::parser::data::Child::Token(index) => {
                    let token = &tokens[*index];
                    if token.is(L::TemplateBody) {
                        out.push_str(&token.value);
                    }
                }
                crate::parser::data::Child::Node(child) => {
                    let child_ref = trees.child_ref(r.file, child);
                    if trees.is(child_ref, E::TemplateReference) {
                        out.push_str("[[");
                        if let Some(reference) = trees.child_nodes(child_ref).into_iter().next() {
                            out.push_str(&self.reference_text(reference, env));
                        }
                        out.push_str("]]");
                    } else if trees.is(child_ref, E::TemplateExecution) {
                        match trees.child_nodes(child_ref).into_iter().next() {
                            Some(expression) => {
                                let value = self.eval(expression, env);
                                match value {
                                    Value::Undefined => out.push_str(&trees.raw(child_ref)),
                                    other => out.push_str(&self.to_text(&other)),
                                }
                            }
                            None => out.push_str(&trees.raw(child_ref)),
                        }
                    }
                }
                crate::parser::data::Child::Error(_) => {}
            }
        }
        out
    }

    /// How a reference inside a template renders: a declared name keeps its text, a
    /// variable or parameter is replaced by what it holds.
    // Decision: `[[Token.file]]` stays `Token.file`; `[[&rule]]` becomes the identifier
    // of the entity the loop variable holds.
    fn reference_text(&mut self, reference: NodeRef, env: &mut Env) -> String {
        let trees = self.trees.clone();
        let rule = trees.rule(reference);
        if rule == E::Reference.entity() {
            return match trees.child_nodes(reference).into_iter().next() {
                Some(inner) => self.reference_text(inner, env),
                None => String::new(),
            };
        }
        if rule == E::Name.entity() {
            let name = trees.name(reference).unwrap_or_default();
            // Only a variable holding an entity is replaced; a string or list keeps the
            // name as written, since it has no identifier.
            if let Some(Value::Entity(e)) = env.get(&name) {
                return self.model.entities[*e].identifier.clone().unwrap_or(name);
            }
            return name;
        }
        if rule == E::Dereference.entity() {
            let Some(operand) = trees.child_nodes(reference).into_iter().next() else { return String::new() };
            return self.reference_text(operand, env);
        }
        if rule == E::Current.entity() {
            let Some((accessor, name)) = trees.accessor_and_name(reference) else { return trees.raw(reference) };
            let head = self.model.entities[env.current].identifier.clone().unwrap_or_default();
            return match name {
                None => head,
                Some(name) => format!("{head}{}{}", trees.token(reference.file, accessor).raw, trees.token(reference.file, name).value),
            };
        }
        if rule == E::Member.entity() {
            let Some(left) = trees.left(reference) else { return trees.raw(reference) };
            let Some((accessor, name)) = trees.accessor_and_name(reference) else { return trees.raw(reference) };
            let head = self.reference_text(left, env);
            return match name {
                None => format!("{head}{}", trees.token(reference.file, accessor).raw),
                Some(name) => format!("{head}{}{}", trees.token(reference.file, accessor).raw, trees.token(reference.file, name).value),
            };
        }
        trees.raw(reference).trim().to_string()
    }
}
