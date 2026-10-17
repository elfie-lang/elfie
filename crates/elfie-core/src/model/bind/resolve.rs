// @lfy def/model/main.lfy:bind
//! The resolve pass: the usage every name, member, template, and trait use makes, and the
//! problem a name that resolves nowhere adds.

use super::*;

impl Binder {

    /// The trait symbol a TraitUse names, statically. Named in the IsClause or
    /// ExtendsClause of a declaration, it is found walking outward from the scope that
    /// holds the declaration, never from the declaration's own scope, so a member the
    /// declaration gains from an applied trait never shadows the trait itself, as the
    /// member rule of the trait rule would; arguments still resolve in the declaration's
    /// own scope, wherever they are evaluated.
    // @lfy def/model/main.lfy:bind
    pub fn resolve_trait_use(&self, trait_use: NodeRef) -> Option<SymbolId> {
        let trees = &self.trees;
        let identifiers: Vec<usize> = trees
            .parts(trait_use)
            .into_iter()
            .filter_map(|p| match p {
                Part::Token(i)
                    if trees.token_is(
                        trait_use.file,
                        i,
                        crate::grammar::terminals::identifier::Identifier::Identifier,
                    ) =>
                {
                    Some(i)
                }
                _ => None,
            })
            .collect();
        let own_scope = self.model.enclosing_scope(trait_use);
        // The declaring statement the clause belongs to, when the clause is an IsClause
        // or ExtendsClause; only when that statement itself owns a scope (a data, trait,
        // type, enum, or function, never an external or a variable) is there a scope of
        // its own to skip.
        let clause = trees
            .ancestor(trait_use, E::IsClause)
            .or_else(|| trees.ancestor(trait_use, E::ExtendsClause));
        let declaration = clause.and_then(|c| {
            let mut current = trees.parent(c);
            while let Some(node) = current {
                if trees.is_statement(node) {
                    return Some(node);
                }
                current = trees.parent(node);
            }
            None
        });
        let scope = match declaration.and_then(|d| self.model.scope_of(d)) {
            Some(declared_scope) if declared_scope == own_scope => {
                self.model.scopes[own_scope].parent.unwrap_or(own_scope)
            }
            _ => own_scope,
        };
        match identifiers.as_slice() {
            [single] => self.lookup(scope, &trees.token(trait_use.file, *single).value),
            [module, name] => {
                let module = self.lookup(scope, &trees.token(trait_use.file, *module).value)?;
                let entity = self.model.symbols[module].entity;
                self.member_symbol(entity, &trees.token(trait_use.file, *name).value)
            }
            _ => None,
        }
    }

    /// Two entities carrying the trait `rule` with the same identifier: the grammar knows
    /// a rule only by its identifier, so a problem is added at the second, naming the
    /// file of the first. Run after every file has been applied, so every entity's
    /// traits, gained through `is`, `extends`, or `apply`, are complete.
    // @lfy def/model/main.lfy:bind
    pub fn check_rule_identifiers(&mut self) {
        let Some(rule_trait) = self.model.trait_named("rule") else {
            return;
        };
        let mut seen: HashMap<String, EntityId> = HashMap::new();
        for id in 0..self.model.entities.len() {
            if !self.model.entities[id].has_trait(rule_trait) {
                continue;
            }
            let Some(name) = self.model.entities[id].identifier.clone() else {
                continue;
            };
            match seen.get(&name) {
                Some(&first) => {
                    if let Some(node) = self.model.entities[id].node {
                        let path = self.model.entities[first]
                            .node
                            .map(|n| self.trees.sources[n.file].path.clone())
                            .unwrap_or_default();
                        self.problem(node, format!("{name} is already a rule, in {path}"));
                    }
                }
                None => {
                    seen.insert(name, id);
                }
            }
        }
    }

    // ---- Resolve ------------------------------------------------------------------

    /// Every reading node of a file gets a Usage, resolved as the model says.
    // @lfy def/model/main.lfy:bind
    pub fn resolve_file(&mut self, file: FileId) {
        let root = NodeRef { file, index: 0 };
        self.resolve_node(root);
    }

    fn resolve_node(&mut self, r: NodeRef) {
        let trees = self.trees.clone();
        let rule = trees.rule(r);
        // A With runs its block with the entity its expression resolves to as current.
        // @lfy def/model/main.lfy:bind
        if rule == S::With.entity() {
            let children = trees.child_nodes(r);
            for &child in &children {
                if !trees.is(child, S::Block) {
                    self.resolve_node(child);
                    if let Some(usage) = self.model.usage_of(child)
                        && let Some(symbol) = self.model.usages[usage].symbol
                    {
                        let entity = self.model.symbols[symbol].entity;
                        if let Some(scope) = self.model.scope_of(r) {
                            self.set_current(scope, entity);
                        }
                    }
                }
            }
            for &child in &children {
                if trees.is(child, S::Block) {
                    self.resolve_node(child);
                }
            }
            return;
        }
        for child in trees.children(r) {
            self.resolve_node(child);
        }
        if rule == E::Name.entity() {
            self.resolve_name(r);
        } else if rule == E::Current.entity() {
            self.resolve_current(r);
        } else if rule == E::Member.entity() {
            self.resolve_member(r);
        } else if rule == E::TraitUse.entity() {
            let symbol = self.resolve_trait_use(r);
            let name = trees.raw(r).trim().to_string();
            let token = trees.parts(r).into_iter().find_map(|p| match p {
                Part::Token(i) => Some(i),
                _ => None,
            });
            if symbol.is_none() {
                self.problem(r, format!("{name} does not name a trait in scope"));
            }
            self.add_usage(r, Some(name), token, Layer::Value, symbol);
        } else if rule == E::Dereference.entity() {
            // A Dereference yields the entity its operand is bound to.
            // @lfy def/model/main.lfy:bind
            let operand = trees.child_nodes(r).into_iter().next();
            let symbol = operand
                .and_then(|o| self.model.usage_of(o))
                .and_then(|u| self.model.usages[u].symbol);
            let token = operand
                .and_then(|o| self.model.usage_of(o))
                .and_then(|u| self.model.usages[u].token);
            self.add_usage(
                r,
                operand.map(|o| trees.raw(o).trim().to_string()),
                token,
                Layer::Dereference,
                symbol,
            );
        } else if rule == E::Previous.entity() {
            // A Previous yields the entity declared by the nearest earlier statement.
            // @lfy def/model/main.lfy:bind
            let symbol = self.previous_symbol(r);
            let token = trees.parts(r).into_iter().find_map(|p| match p {
                Part::Token(i) => Some(i),
                _ => None,
            });
            if symbol.is_none() {
                self.problem(r, "no earlier statement of this block declares anything");
            }
            self.add_usage(r, None, token, Layer::Previous, symbol);
        } else if rule == S::AliasDeclaration.entity() {
            // An alias's symbol is bound to the target's entity.
            // @lfy def/model/data.lfy:Symbol.entity
            if let Some(symbol) = self.model.symbol_of(r) {
                let value = trees
                    .child_nodes(r)
                    .into_iter()
                    .find(|&c| !trees.is(c, E::Declared));
                if let Some(target) = value
                    .and_then(|v| self.model.usage_of(v))
                    .and_then(|u| self.model.usages[u].symbol)
                {
                    let entity = self.model.symbols[target].entity;
                    self.model.symbols[symbol].entity = entity;
                }
            }
        }
    }

    /// Sets the current entity of a scope and of every scope inside it that inherited the
    /// old one (a Block, For, or With), leaving the scopes of declarations alone.
    // @lfy def/model/main.lfy:bind
    fn set_current(&mut self, scope: ScopeId, entity: EntityId) {
        let old = self.model.scopes[scope].current;
        let mut updated = HashSet::new();
        updated.insert(scope);
        self.model.scopes[scope].current = entity;
        // A scope is created after its parent, so one pass in order reaches every descendant.
        for id in scope + 1..self.model.scopes.len() {
            let inherits = self.model.scopes[id]
                .parent
                .is_some_and(|parent| updated.contains(&parent))
                && self.model.scopes[id].current == old;
            if inherits {
                self.model.scopes[id].current = entity;
                updated.insert(id);
            }
        }
    }

    fn in_template(&self, r: NodeRef) -> bool {
        let trees = &self.trees;
        trees.ancestor(r, E::TemplateReference).is_some()
            || trees.ancestor(r, E::TemplateExecution).is_some()
            || trees.ancestor(r, C::Documentation).is_some()
    }

    /// The entity whose declaration encloses a node, for Entity.references.
    fn enclosing_entity(&self, r: NodeRef) -> Option<EntityId> {
        let mut current = self.trees.parent(r);
        while let Some(node) = current {
            if let Some(symbol) = self.model.symbol_of(node) {
                return Some(self.model.symbols[symbol].entity);
            }
            current = self.trees.parent(node);
        }
        None
    }

    fn record_reference(&mut self, r: NodeRef, usage: UsageId) {
        if self.trees.ancestor(r, E::TemplateReference).is_some()
            && let Some(entity) = self.enclosing_entity(r)
        {
            self.model.entities[entity].references.push(usage);
        }
    }

    /// A Name resolves to the first match walking from its scope outward; inside a
    /// template, a name found nowhere resolves to the one rule entity with that name. Two
    /// rule entities sharing that identifier is neither, and its problem says so rather
    /// than that the name is not declared.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    fn resolve_name(&mut self, r: NodeRef) {
        let trees = self.trees.clone();
        // The Name that spells a declared name is the declaration, not a usage.
        if let Some(parent) = trees.parent(r)
            && declaring(trees.rule(parent)).is_some_and(|(holder, _)| holder == E::Name.entity())
        {
            return;
        }
        let Some(name) = trees.name(r) else { return };
        let token = trees.name_token(r);
        let scope = self.model.enclosing_scope(r);
        let mut symbol = self.lookup_at(r, scope, &name);
        let in_template = self.in_template(r);
        if symbol.is_none() && in_template {
            symbol = self
                .model
                .rule_entity(&name)
                .and_then(|e| self.model.entities[e].symbol);
        }
        if symbol.is_none() {
            let message = if in_template && self.model.rule_identifier_is_ambiguous(&name) {
                format!("{name} is more than one rule")
            } else {
                format!("{name} is not declared in scope")
            };
            self.problem(r, message);
        }
        let usage = self.add_usage(r, Some(name), token, Layer::Value, symbol);
        self.record_reference(r, usage);
    }

    /// A Current is bound with the scope's current entity as the left side.
    // @lfy def/model/main.lfy:bind
    fn resolve_current(&mut self, r: NodeRef) {
        let trees = self.trees.clone();
        let Some((accessor, name)) = trees.accessor_and_name(r) else {
            return;
        };
        let layer =
            accessor_layer(trees.token(r.file, accessor).rule.expect("accessor")).expect("layer");
        // The Current that declares a member still makes its Usage, resolved to the symbol
        // it declares; it is not a reference to anything else.
        if let Some(declared) = self.model.symbol_of(r) {
            let name_text = name.map(|i| trees.token(r.file, i).value.clone());
            self.add_usage(r, name_text, Some(name.unwrap_or(accessor)), layer, Some(declared));
            return;
        }
        let current = self.current_of(r);
        let name_text = name.map(|i| trees.token(r.file, i).value.clone());
        let symbol = self.resolve_in(
            r,
            Target::Entity(current),
            layer,
            name_text.as_deref(),
            true,
        );
        let usage = self.add_usage(r, name_text, Some(name.unwrap_or(accessor)), layer, symbol);
        self.record_reference(r, usage);
    }

    /// A Member resolves its name in what its left side resolves to.
    // @lfy def/model/main.lfy:bind
    fn resolve_member(&mut self, r: NodeRef) {
        let trees = self.trees.clone();
        let Some((accessor, name)) = trees.accessor_and_name(r) else {
            return;
        };
        let layer =
            accessor_layer(trees.token(r.file, accessor).rule.expect("accessor")).expect("layer");
        let name_text = name.map(|i| trees.token(r.file, i).value.clone());
        // The add of `@acceptanceCriteria.add(...)`, and of the other lists an entity
        // carries, is the binder's own call, not a member of the list it reads: it has no
        // symbol and adds no problem.
        // @lfy def/model/main.lfy:bind
        let target = if layer == Layer::Value && self.is_prelude_list_add(r, name_text.as_deref())
        {
            Target::Unknown
        } else {
            trees
                .left(r)
                .map_or(Target::Unknown, |left| self.target_of(left))
        };
        let symbol = self.resolve_in(r, target, layer, name_text.as_deref(), false);
        let usage = self.add_usage(r, name_text, Some(name.unwrap_or(accessor)), layer, symbol);
        self.record_reference(r, usage);
    }

    /// Whether a member named `add` is read with the value accessor on one of the lists
    /// of an entity's context: `@acceptanceCriteria.add(...)`, `@knowledge.add(...)`,
    /// `@commands.add(...)`, `@targets.add(...)`, or the `.add` of the chain that follows
    /// one of them.
    // @lfy def/model/main.lfy:bind
    fn is_prelude_list_add(&self, r: NodeRef, name: Option<&str>) -> bool {
        /// The lists of an entity's context whose `add` the binder answers for itself.
        const LISTS: [&str; 4] = ["acceptanceCriteria", "knowledge", "commands", "targets"];

        if name != Some("add") {
            return false;
        }
        let trees = &self.trees;
        let Some(mut node) = trees.left(r) else {
            return false;
        };
        loop {
            // The left of a later add in the chain is the call of the earlier one.
            if trees.is(node, E::Call)
                && let Some(callee) = trees.left(node)
            {
                node = callee;
                continue;
            }
            let Some((accessor, Some(spelled))) = trees.accessor_and_name(node) else {
                return false;
            };
            if trees.token_is(node.file, accessor, P::ContextAccessor) {
                return LISTS.contains(&trees.token(node.file, spelled).value.as_str());
            }
            if trees.token(node.file, spelled).value == "add"
                && let Some(left) = trees.left(node)
            {
                node = left;
                continue;
            }
            return false;
        }
    }

    /// Resolves a member name in a target through one layer, adding problems as the
    /// model says.
    fn resolve_in(
        &mut self,
        r: NodeRef,
        target: Target,
        layer: Layer,
        name: Option<&str>,
        current: bool,
    ) -> Option<SymbolId> {
        let name = name?;
        match layer {
            Layer::Context => {
                // The MemberName resolves in the members of the entity's kind data and
                // yields that member.
                // @lfy def/model/main.lfy:bind
                if let Some(entity) = target.entity()
                    && let Some(symbol) = self.kind_member(entity, name)
                {
                    return Some(symbol);
                }
                // @lfy def/model/main.lfy:bind#bind:bind:04c22787733ff3defc0c5a6b52add93b83e7c0630dafc6c75ee0a897057f1463
                // A list type is `List` seen with its one item type, so its kind data is
                // the one such an entity has.
                if let Target::Base(data) = &target
                    && self.prelude_data("List") == Some(*data)
                    && let Some(symbol) = self
                        .list_kind_data()
                        .into_iter()
                        .find_map(|kind| self.member_symbol(kind, name))
                {
                    return Some(symbol);
                }
                // A MemberName that is a member of a kind data other than the entity's
                // yields undefined for it, without a problem.
                // @lfy def/model/main.lfy:bind
                if self.kind_member_anywhere(name) {
                    return None;
                }
                // With no prelude, the properties the context layer gives every entity.
                if ContextProperty::lookup(name).is_none() {
                    self.problem(r, format!("{name} is not a context property"));
                }
                None
            }
            Layer::Parent | Layer::Dereference | Layer::Previous => None,
            Layer::Value | Layer::Scope => match target {
                Target::Module(file) => {
                    let scope = self.model.file_scopes[file];
                    let symbol = self.model.lookup_local(scope, name);
                    if symbol.is_none() {
                        self.problem(r, format!("the module declares no {name}"));
                    }
                    symbol
                }
                Target::Entity(entity) => {
                    // An entity's own members first, then those members of its kind data
                    // whose value is a function.
                    // @lfy def/model/main.lfy:bind
                    let symbol = self
                        .member_symbol(entity, name)
                        .or_else(|| self.inherited_member(entity, name))
                        .or_else(|| {
                            (layer == Layer::Value)
                                .then(|| self.kind_function_member(entity, name))
                                .flatten()
                        });
                    if symbol.is_none() {
                        self.no_member(r, entity, name, current);
                    }
                    symbol
                }
                // Anything with the trait: the trait's members, what it extends, and the
                // function members of its kind data. A name found in none of them is a
                // problem here as well, a predicate being no more lenient than a data.
                // @lfy def/model/main.lfy:bind
                Target::Predicate(trait_entity) => {
                    let symbol = self
                        .member_symbol(trait_entity, name)
                        .or_else(|| self.inherited_member(trait_entity, name))
                        .or_else(|| self.kind_function_member(trait_entity, name))
                        .or_else(|| self.carried_member(trait_entity, name));
                    if symbol.is_none() {
                        self.no_member(r, trait_entity, name, false);
                    }
                    symbol
                }
                // An object's keys first, then the members of Object. A key declares
                // nothing, so a name found among the keys has no symbol, and is no
                // problem.
                // @lfy def/model/main.lfy:bind#bind:bind:13a7ef34563641503b4fd1a9fdcaa38cbb0f8bb47dc497f235d53d63b8e451da
                Target::Object(keys) => {
                    if keys.iter().any(|key| key == name) {
                        return None;
                    }
                    let symbol = self
                        .prelude_data("Object")
                        .and_then(|data| self.member_symbol(data, name));
                    if symbol.is_none() {
                        self.problem(r, format!("the object has no key or member {name}"));
                    }
                    symbol
                }
                // For a value of a base data, that data's members; an object whose keys
                // are not known has none of its own.
                // @lfy def/model/main.lfy:bind
                // @lfy def/model/main.lfy:bind
                Target::Base(data) => {
                    let symbol = self.member_symbol(data, name);
                    if symbol.is_none() {
                        let what = self.model.entities[data]
                            .identifier
                            .clone()
                            .unwrap_or_else(|| "the value".to_string());
                        self.problem(r, format!("{what} has no member {name}"));
                    }
                    symbol
                }
                // The left side has a symbol, but nothing is known of what it holds, so
                // the name is found nowhere.
                // @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
                Target::Untyped => {
                    self.problem(r, format!("nothing known of the value has a member {name}"));
                    None
                }
                // The left side has no symbol either: the usage has no symbol and no
                // second problem is added.
                // @lfy def/model/main.lfy:bind
                // @lfy def/model/main.lfy:bind
                Target::Unknown => None,
            },
        }
    }

    /// Nothing was found for a name read on an entity: the usage has no symbol and a
    /// problem is added, whatever the left side is, and even when no prelude is bound so
    /// the entity's kind data is empty, because being lenient here would hide a misspelled
    /// member behind a silent undefined.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    fn no_member(&mut self, r: NodeRef, entity: EntityId, name: &str, current: bool) {
        let owner = self.model.entities[entity]
            .identifier
            .clone()
            .unwrap_or_else(|| "the current entity".to_string());
        let what = if current {
            "the current entity"
        } else {
            owner.as_str()
        };
        self.problem(r, format!("{what} has no member {name}"));
    }

    /// A member of anything that carries a trait: a value that `is t` is one of the
    /// entities `t` was applied to, so a name `t` itself does not spell is looked for
    /// among the traits those entities carry as well — `precedence` on a rule that is
    /// also `binding`, `lexCondition` on a terminal that is also `candidateInModes`. A
    /// name no carrier has is found nowhere, and is a problem like any other.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    fn carried_member(&self, trait_entity: EntityId, name: &str) -> Option<SymbolId> {
        let EntityKind::Trait { entities, .. } = &self.model.entities[trait_entity].kind else {
            return None;
        };
        let mut seen = vec![trait_entity];
        for &carrier in entities {
            for applied in &self.model.entities[carrier].traits {
                if seen.contains(&applied.entity) {
                    continue;
                }
                seen.push(applied.entity);
                if let Some(symbol) = self.member_symbol(applied.entity, name) {
                    return Some(symbol);
                }
            }
        }
        None
    }

    /// A member reached through the traits an entity carries or extends, up the whole
    /// chain: what a trait extends is applied too, so its members are reached as well.
    // @lfy def/model/main.lfy:bind
    fn inherited_member(&self, entity: EntityId, name: &str) -> Option<SymbolId> {
        let mut seen = vec![entity];
        let mut pending: Vec<EntityId> = self.model.entities[entity]
            .traits
            .iter()
            .map(|applied| applied.entity)
            .collect();
        let mut next = 0;
        while next < pending.len() {
            let carried = pending[next];
            next += 1;
            if seen.contains(&carried) {
                continue;
            }
            seen.push(carried);
            if let Some(symbol) = self.member_symbol(carried, name) {
                return Some(symbol);
            }
            pending.extend(
                self.model.entities[carried]
                    .traits
                    .iter()
                    .map(|applied| applied.entity),
            );
        }
        None
    }

    /// What an expression on the left of an accessor resolves to.
    pub(crate) fn target_of(&mut self, node: NodeRef) -> Target {
        let trees = self.trees.clone();
        let rule = trees.rule(node);
        if rule == E::Name.entity() || rule == E::Member.entity() || rule == E::TraitUse.entity() {
            let symbol = self
                .model
                .usage_of(node)
                .and_then(|u| self.model.usages[u].symbol);
            return match symbol {
                Some(symbol) => self.target_of_symbol(symbol),
                None => Target::Unknown,
            };
        }
        if rule == E::Current.entity() {
            let Some((accessor, name)) = trees.accessor_and_name(node) else {
                return Target::Unknown;
            };
            let current = self.current_of(node);
            if name.is_none() {
                return if trees.token_is(node.file, accessor, P::ContextAccessor) {
                    Target::Unknown
                } else {
                    Target::Entity(current)
                };
            }
            let symbol = self
                .model
                .usage_of(node)
                .and_then(|u| self.model.usages[u].symbol);
            return match symbol {
                Some(symbol) => self.target_of_symbol(symbol),
                None => Target::Unknown,
            };
        }
        if rule == E::Group.entity() {
            return trees
                .child_nodes(node)
                .into_iter()
                .next()
                .map_or(Target::Unknown, |inner| self.target_of(inner));
        }
        if rule == E::Dereference.entity() {
            let symbol = self
                .model
                .usage_of(node)
                .and_then(|u| self.model.usages[u].symbol);
            return symbol.map_or(Target::Unknown, |s| {
                Target::Entity(self.model.symbols[s].entity)
            });
        }
        // The left expression with type arguments reads the members of the declaration it
        // names, seen with them.
        // @lfy def/model/main.lfy:bind
        if rule == E::Generic.entity() {
            let ty = self.type_from_expression(node);
            return self.target_of_type(ty).with_no_symbol();
        }
        // An object literal is its keys.
        // @lfy def/model/main.lfy:bind#bind:bind:13a7ef34563641503b4fd1a9fdcaa38cbb0f8bb47dc497f235d53d63b8e451da
        if let Some(keys) = self.object_keys(node) {
            return Target::Object(keys);
        }
        if rule == E::Call.entity() {
            if let Some(left) = trees.left(node) {
                let symbol = self
                    .model
                    .usage_of(left)
                    .and_then(|u| self.model.usages[u].symbol);
                if let Some(symbol) = symbol {
                    let entity = self.model.symbols[symbol].entity;
                    if let Some(output) = self.model.entities[entity].output() {
                        return self.target_of_type(output.clone()).with_no_symbol();
                    }
                }
            }
            return Target::Unknown;
        }
        Target::Unknown
    }

    fn target_of_symbol(&self, symbol: SymbolId) -> Target {
        let symbol = &self.model.symbols[symbol];
        match symbol.kind {
            SymbolKind::Module => match self.model.entities[symbol.entity].file {
                Some(file)
                    if matches!(self.model.entities[symbol.entity].kind, EntityKind::File) =>
                {
                    Target::Module(file)
                }
                _ => Target::Untyped,
            },
            SymbolKind::Data
            | SymbolKind::Trait
            | SymbolKind::Type
            | SymbolKind::Enum
            | SymbolKind::Function
            | SymbolKind::AgentFunction => Target::Entity(symbol.entity),
            SymbolKind::Alias | SymbolKind::External => {
                match &self.model.entities[symbol.entity].kind {
                    EntityKind::Data
                    | EntityKind::Trait { .. }
                    | EntityKind::Type
                    | EntityKind::Enum => Target::Entity(symbol.entity),
                    _ => Target::Untyped,
                }
            }
            _ => {
                // A variable declared from an object literal, or a loop variable over a
                // list of them: its keys are known.
                // @lfy def/model/main.lfy:bind#bind:bind:13a7ef34563641503b4fd1a9fdcaa38cbb0f8bb47dc497f235d53d63b8e451da
                let node = self.model.entities[symbol.entity].node;
                let keys = match symbol.kind {
                    SymbolKind::Variable => node
                        .and_then(|node| self.declared_value(node))
                        .and_then(|value| self.keys_of(value)),
                    SymbolKind::LoopVariable => node
                        .and_then(|node| self.loop_iterable(node))
                        .and_then(|iterable| self.item_keys_of(iterable)),
                    _ => None,
                };
                if let Some(keys) = keys {
                    return Target::Object(keys);
                }
                match self.model.entities[symbol.entity].ty.clone() {
                    Some(ty) => self.target_of_type(ty),
                    None => Target::Untyped,
                }
            }
        }
    }

    /// The value a variable declaration is written with.
    fn declared_value(&self, declaration: NodeRef) -> Option<NodeRef> {
        let trees = &self.trees;
        if !trees.is(declaration, S::VariableDeclaration) {
            return None;
        }
        trees
            .child_nodes(declaration)
            .into_iter()
            .find(|&child| !trees.is(child, E::Declared))
    }

    /// The expression a For iterates over, from the node of its loop variable.
    fn loop_iterable(&self, node: NodeRef) -> Option<NodeRef> {
        let trees = &self.trees;
        let for_node = if trees.is(node, S::ForFrom) {
            trees.parent(node)?
        } else {
            node
        };
        let iterable = trees
            .child(for_node, S::ForInOf)
            .or_else(|| trees.child(for_node, S::ForFrom))?;
        trees
            .child_nodes(iterable)
            .into_iter()
            .find(|&child| !trees.is(child, E::Declared))
    }

    /// The keys of the object an expression is: an object literal, or a variable declared
    /// from one.
    // @lfy def/model/main.lfy:bind#bind:bind:13a7ef34563641503b4fd1a9fdcaa38cbb0f8bb47dc497f235d53d63b8e451da
    fn keys_of(&self, expression: NodeRef) -> Option<Vec<String>> {
        if let Some(keys) = self.object_keys(expression) {
            return Some(keys);
        }
        self.variable_value(expression)
            .and_then(|value| self.keys_of(value))
    }

    /// The keys of the objects a list holds, when every item of it is an object the
    /// binder can read: a list literal, or a variable declared from one.
    // @lfy def/model/main.lfy:bind#bind:bind:13a7ef34563641503b4fd1a9fdcaa38cbb0f8bb47dc497f235d53d63b8e451da
    fn item_keys_of(&self, expression: NodeRef) -> Option<Vec<String>> {
        let trees = &self.trees;
        if trees.is(expression, E::List) {
            let items = trees
                .child(expression, E::Items)
                .map(|items| trees.child_nodes(items))
                .unwrap_or_default();
            let mut keys: Vec<String> = Vec::new();
            for item in items {
                for key in self.keys_of(item)? {
                    if !keys.contains(&key) {
                        keys.push(key);
                    }
                }
            }
            return Some(keys);
        }
        self.variable_value(expression)
            .and_then(|value| self.item_keys_of(value))
    }

    /// The value the variable a name or member is bound to was declared with.
    fn variable_value(&self, expression: NodeRef) -> Option<NodeRef> {
        if !self.trees.is(expression, E::Name) && !self.trees.is(expression, E::Member) {
            return None;
        }
        let symbol = self
            .model
            .usage_of(expression)
            .and_then(|usage| self.model.usages[usage].symbol)?;
        if self.model.symbols[symbol].kind != SymbolKind::Variable {
            return None;
        }
        self.declared_value(self.model.entities[self.model.symbols[symbol].entity].node?)
    }

    /// The keys an object literal spells, when an expression is one: each key's
    /// identifier, or the text of its string.
    // @lfy def/model/main.lfy:bind#bind:bind:13a7ef34563641503b4fd1a9fdcaa38cbb0f8bb47dc497f235d53d63b8e451da
    pub(crate) fn object_keys(&self, expression: NodeRef) -> Option<Vec<String>> {
        let trees = &self.trees;
        if !trees.is(expression, E::Object) {
            return None;
        }
        let keys = trees
            .children_of(expression, E::ObjectKey)
            .into_iter()
            .filter_map(|key| {
                let declared = trees.child(key, E::Declared)?;
                match trees.child_token(
                    declared,
                    crate::grammar::terminals::identifier::Identifier::Identifier,
                ) {
                    Some(token) => Some(trees.token(key.file, token).value.clone()),
                    None => trees.child(declared, E::StringLiteral).map(|literal| {
                        trees
                            .raw(literal)
                            .trim()
                            .trim_matches(|c| c == '"' || c == '\'')
                            .to_string()
                    }),
                }
            })
            .collect();
        Some(keys)
    }

    fn target_of_type(&self, ty: TypeRef) -> Target {
        match ty {
            // A left side whose type is a type parameter reads the members of what the
            // parameter must extend; without an extends clause nothing resolves and no
            // problem is added, because nothing is known about the parameter.
            // @lfy def/model/main.lfy:bind
            TypeRef::Entity(e) if self.is_type_parameter(e) => {
                match self.model.entities[e].ty.clone() {
                    Some(constraint) => self.target_of_type(constraint),
                    None => Target::Unknown,
                }
            }
            TypeRef::Entity(e) => match self.model.entities[e].kind {
                EntityKind::File => {
                    Target::Module(self.model.entities[e].file.expect("file entity"))
                }
                EntityKind::Trait { .. } => Target::Predicate(e),
                _ => Target::Entity(e),
            },
            TypeRef::Predicate(t) => Target::Predicate(t),
            TypeRef::Union(items) => {
                // A value that may also be null or undefined reads as the one other type.
                let items: Vec<TypeRef> = items
                    .into_iter()
                    .filter(|item| {
                        !matches!(
                            item,
                            TypeRef::Literal(value) if matches!(**value, Value::Null | Value::Undefined)
                        )
                    })
                    .collect();
                if let [only] = items.as_slice() {
                    return self.target_of_type(only.clone());
                }
                // A union of entities: the first entity, leniently.
                for item in items {
                    if let TypeRef::Entity(e) = item {
                        return Target::Predicate(e);
                    }
                }
                Target::Untyped
            }
            // A value of any other base data.
            // @lfy def/model/main.lfy:bind
            other => self.base_data(&other).map_or(Target::Untyped, Target::Base),
        }
    }

    /// Whether an entity is the entity of a `TypeParameter`.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn is_type_parameter(&self, entity: EntityId) -> bool {
        self.model.entities[entity]
            .symbol
            .is_some_and(|symbol| self.model.symbols[symbol].kind == SymbolKind::TypeParameter)
    }

    /// The symbol declared by the nearest earlier statement of the same block.
    fn previous_symbol(&self, r: NodeRef) -> Option<SymbolId> {
        let trees = &self.trees;
        let mut statement = r;
        while let Some(parent) = trees.parent(statement) {
            if trees.is_statement(statement)
                && (trees.is(parent, S::Block)
                    || trees.is(parent, crate::grammar::rules::file::File::SourceFile))
            {
                let siblings = trees.child_nodes(parent);
                let position = siblings.iter().position(|&s| s == statement)?;
                for &earlier in siblings[..position].iter().rev() {
                    if let Some(symbol) = self.model.symbol_of(earlier) {
                        return Some(symbol);
                    }
                }
                return None;
            }
            statement = parent;
        }
        None
    }
}
