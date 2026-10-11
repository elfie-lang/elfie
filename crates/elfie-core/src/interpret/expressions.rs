// @lfy def/interpret/main.lfy:evaluate
//! Evaluating an expression at compile time: reading names, members, and layers, and
//! calling what a call names.

use super::*;

/// The trait whose items each satisfy a rule: an `apply` on one applies the trait to every
/// item of it.
// @lfy def/interpret/main.lfy:expand
const ALTERNATION_LIST: &str = "alternationList";

/// `Entity.typeArguments`: what a use of a generic declaration was written with.
// @lfy def/interpret/main.lfy:expand
const TYPE_ARGUMENTS: &str = "typeArguments";

/// `Entity.typeParameters`: what a declaration is generic over.
// @lfy def/interpret/main.lfy:expand
const TYPE_PARAMETERS: &str = "typeParameters";

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

impl Interpreter<'_> {
    /// Notes that what was read only has a value at runtime, or could not be read at all,
    /// so [`evaluate`] gives nothing back and [`lower`] folds nothing here.
    // @lfy def/interpret/main.lfy:evaluate
    pub(super) fn only_at_runtime(&mut self) -> Interim {
        self.runtime = true;
        Interim::Undefined
    }

    /// Evaluates an expression at compile time.
    // @lfy def/interpret/main.lfy:evaluate
    pub(super) fn eval(&mut self, r: NodeRef, env: &mut Env) -> Interim {
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
    pub(super) fn eval_traits(&mut self, r: NodeRef, env: &mut Env) -> Interim {
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
    pub(super) fn eval_index(&mut self, r: NodeRef, env: &mut Env) -> Interim {
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
    pub(super) fn eval_infix(&mut self, r: NodeRef, env: &mut Env) -> Interim {
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
    pub(super) fn eval_assignment(&mut self, r: NodeRef, env: &mut Env) -> Interim {
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
    pub(super) fn eval_current(&mut self, r: NodeRef, env: &mut Env) -> Interim {
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
    pub(super) fn eval_previous(&mut self, r: NodeRef) -> Interim {
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
    pub(super) fn eval_member(&mut self, r: NodeRef, env: &mut Env) -> Interim {
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
        // @lfy def/interpret/main.lfy:expand#expand:expand:d6d80207ed7ea67fd728b58a28b9ec2288d778cb2ae43de5d0dcf264ce3abd1a
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
    // @lfy def/interpret/main.lfy:expand#expand:expand:d6d80207ed7ea67fd728b58a28b9ec2288d778cb2ae43de5d0dcf264ce3abd1a
    pub(super) fn declared_by(&self, left: NodeRef, env: &Env) -> Option<EntityId> {
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
    pub(super) fn access(
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
    pub(super) fn context_property(&mut self, value: &Interim, name: &str) -> Interim {
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
    pub(super) fn entity_property(&mut self, entity: EntityId, name: &str, value: &Interim) -> Interim {
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
    pub(super) fn member_value(&mut self, entity: EntityId, name: &str, env: &mut Env) -> Interim {
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
    pub(super) fn value_of_symbol(&mut self, symbol: SymbolId) -> Interim {
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
    /// being taken. It is taken from the declaration wherever the name was read, so a
    /// compile-time value is the same read before its declaration as after it.
    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:bb33b89909ec6ede4b06c592019c0381b73e99575350a939f2f20b06a6775738
    pub(super) fn value_of_variable(&mut self, entity: EntityId) -> Interim {
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
    pub(super) fn report_cycle(&mut self, entity: EntityId) {
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
    pub(super) fn eval_call(&mut self, r: NodeRef, env: &mut Env) -> Interim {
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
    pub(super) fn eval_method(
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
    pub(super) fn apply_receivers(&mut self, first: NodeRef, env: &mut Env) -> Vec<EntityId> {
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
    pub(super) fn call_value(
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
    pub(super) fn runs_wherever_called(&self, node: NodeRef) -> bool {
        self.model.ancestor(node, S::Ace).is_some()
            || self.model.sources.get(node.file).map(|source| source.origin)
                != Some(Origin::Program)
    }

    /// Calls a declared fn or function. A fn carrying [`BUILTIN`] is performed natively and
    /// never run; a fn carrying nothing has criteria for a body and nothing to run. What
    /// decides whether a call runs is where the call is written, never where the function
    /// it calls is declared.
    // @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
    // @lfy def/interpret/main.lfy:expand#expand:expand:fbc7d19ecec253d62dde790e452968a3262f174b36abe019d91512e5e02c6165
    pub(super) fn call_declared(
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
            // @lfy def/interpret/main.lfy:expand#expand:expand:dbbc87c48b923ddf49e557eac7e618d72f9965cd61a2e1bed597c92a0d16d4f1
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
}
