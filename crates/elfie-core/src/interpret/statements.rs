// @lfy def/interpret/main.lfy:expand
//! Running the statements of a body: declarations, clauses, the traits one applies, the
//! layers of a target, and control flow.

use super::*;

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

/// The members of a layer every layer of a target adds to rather than sets, so several
/// layers holding one are no conflict: the target has every item any of them set.
// @lfy def/interpret/main.lfy:expand
const ACCUMULATED: [&str; 3] = ["provides", "requires", "outputExtensions"];

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

impl Interpreter<'_> {
    /// Runs one statement.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn exec(&mut self, r: NodeRef, env: &mut Env) {
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
    pub(super) fn exec_declaration(&mut self, r: NodeRef, symbol: SymbolId, env: &mut Env) {
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
    pub(super) fn value_parameters(&self, entity: EntityId) -> Vec<SymbolId> {
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
    pub(super) fn exec_context_only(&mut self, block: NodeRef, env: &mut Env) {
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
    pub(super) fn is_context_expression(&self, expression: NodeRef) -> bool {
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
    pub(super) fn apply_clause(&mut self, entity: EntityId, clause: NodeRef, extends: bool, env: &mut Env) {
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
    pub(super) fn include_members(&mut self, receiver: EntityId, from: EntityId) {
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
    pub(super) fn join_member(&mut self, receiver: EntityId, member: SymbolId) {
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
    pub(super) fn new_scope(&mut self, parent: Option<ScopeId>, owner: NodeRef, current: EntityId) -> ScopeId {
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
    pub(super) fn eval_arguments(&mut self, arguments: &[NodeRef], env: &mut Env) -> Vec<Interim> {
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
    pub(super) fn apply_trait(
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
    pub(super) fn trait_env(&mut self, receiver: EntityId, trait_id: EntityId, values: &[Interim]) -> Env {
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
    pub(super) fn target_data(&self) -> Option<EntityId> {
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
    pub(super) fn declares_a_target(&self, entity: EntityId) -> bool {
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
    pub(super) fn layer_of(&self, item: &Interim) -> Option<(EntityId, Vec<Interim>)> {
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
    pub(super) fn apply_layers(&mut self, declaration: NodeRef, entity: EntityId, value: &Interim) {
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
        // Each layer is applied in guidance order, and what it set is read back from the
        // const afterwards: the values the window holds are the layer's own and those of
        // the traits it extends, which a layer sets through it.
        // @lfy def/interpret/main.lfy:expand#expand:expand:da7602e2b3243080f2623c7ec36d5389af778549e7da59559ebadf2d10ec6e0a
        // @lfy def/interpret/main.lfy:expand#expand:expand:24896a9f182aab4cba64abb17efdb629e1de1a7ac24c9edcdbc3fa14873d813b
        let mut set: Vec<(String, Interim, EntityId)> = Vec::new();
        for (subject, arguments) in layers {
            let before = self.model.entities[entity].values.len();
            self.apply_trait(
                entity,
                subject,
                Vec::new(),
                arguments,
                AppliedSource::Apply(declaration),
                false,
            );
            let added = self.model.entities[entity].values[before..].to_vec();
            self.name_disagreeing_layers(declaration, subject, &added, &mut set);
        }
    }

    /// Names the member and both layers at the const when two layers of one target set the
    /// same member to different values. [`ACCUMULATED`] is left out: every layer adds to
    /// those. Within one layer the last value it set is the value it sets, as reading the
    /// member back from the const gives.
    // @lfy def/interpret/main.lfy:expand#expand:expand:24896a9f182aab4cba64abb17efdb629e1de1a7ac24c9edcdbc3fa14873d813b
    pub(super) fn name_disagreeing_layers(
        &mut self,
        declaration: NodeRef,
        layer: EntityId,
        added: &[(String, Interim)],
        set: &mut Vec<(String, Interim, EntityId)>,
    ) {
        let mut own: Vec<(String, Interim)> = Vec::new();
        for (member, value) in added {
            if ACCUMULATED.contains(&member.as_str()) {
                continue;
            }
            match own.iter_mut().find(|(name, _)| name == member) {
                Some(slot) => slot.1 = value.clone(),
                None => own.push((member.clone(), value.clone())),
            }
        }
        for (member, value) in own {
            match set.iter().find(|(name, _, _)| *name == member) {
                Some((_, earlier, other)) if *earlier != value => {
                    let first = self.layer_name(*other);
                    let second = self.layer_name(layer);
                    self.problem(
                        declaration,
                        format!(
                            "the layers {first} and {second} of this target set {member} to \
                             different values"
                        ),
                    );
                }
                // An earlier layer set it to the same value, so the two agree.
                Some(_) => {}
                None => set.push((member, value, layer)),
            }
        }
    }

    /// The name of the trait a layer names, for a message that names the layer.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn layer_name(&self, layer: EntityId) -> String {
        self.model.entities[layer]
            .identifier
            .clone()
            .unwrap_or_else(|| "a layer".to_string())
    }

    /// The layer of a trait with these arguments: the object holding the trait as its
    /// subject and the arguments, in order. Nothing is applied by choosing one.
    // @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
    pub(super) fn layer_value(
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

    /// Names the trait at a call of `layer` that gives more arguments than the trait has
    /// parameters with none of them a spread, or fewer than it has parameters that are
    /// neither optional nor spread. A parameter may be left out when it is a spread, when a
    /// question mark marks it optional, or when a setter gives it a default.
    // @lfy def/interpret/main.lfy:expand#expand:expand:199274135094063b0af3eb69ed4b59c3d739b76aca14b459ae4a3f182852b47d
    pub(super) fn check_layer_arity(&mut self, call: NodeRef, subject: EntityId, given: usize) {
        let parameters: Vec<SymbolId> = self.model.entities[subject].parameters().to_vec();
        let nodes: Vec<NodeRef> = parameters
            .iter()
            .map(|&parameter| self.model.symbols[parameter].node)
            .collect();
        let spread = nodes
            .iter()
            .any(|&node| self.model.is(node, E::SpreadParameter));
        let required = nodes
            .iter()
            .filter(|&&node| {
                !self.model.is(node, E::SpreadParameter)
                    && !self.model.has_token(node, P::QuestionMark)
                    && !self.model.has_token(node, P::PlainSetter)
            })
            .count();
        let wanted = nodes.len();
        if given >= required && (spread || given <= wanted) {
            return;
        }
        let name = self.model.entities[subject]
            .identifier
            .clone()
            .unwrap_or_else(|| "a trait".to_string());
        let wants = if spread {
            format!("at least {required}")
        } else if required == wanted {
            format!("{wanted}")
        } else {
            format!("{required} to {wanted}")
        };
        self.problem(
            call,
            format!("{name} takes {wants} arguments as a layer, not {given}"),
        );
    }

    /// The default value of a parameter, read from what follows its setter.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn default_of(&mut self, parameter: NodeRef) -> Option<Interim> {
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
    pub(super) fn exec_where(&mut self, r: NodeRef, env: &mut Env) {
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
    pub(super) fn exec_with(&mut self, r: NodeRef, env: &mut Env) {
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
    pub(super) fn exec_if(&mut self, r: NodeRef, env: &mut Env) {
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
    pub(super) fn exec_for(&mut self, r: NodeRef, env: &mut Env) {
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
    pub(super) fn loop_bindings(
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
    pub(super) fn iterate(&self, value: Interim, keys: bool) -> Vec<Interim> {
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
    pub(super) fn exec_match(&mut self, r: NodeRef, env: &mut Env) {
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
    pub(super) fn exec_code(&mut self, block: NodeRef, env: &mut Env) {
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
    pub(super) fn exec_for_code(&mut self, r: NodeRef, env: &mut Env) {
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
    pub(super) fn bind_parameters(&mut self, function: NodeRef, values: &[Interim], env: &mut Env) {
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
}
