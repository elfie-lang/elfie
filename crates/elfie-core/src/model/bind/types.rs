// @lfy def/model/main.lfy:bind
//! The type pass: `Entity.type` and `Entity.definition` from a declaration alone, and the
//! types a type expression, an expression, and a member read through type arguments
//! yield.

use super::*;

impl Binder {

    /// Entity.type and Entity.definition nodes for every entity of a file, from its
    /// declaration alone.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    pub fn type_entities(&mut self, file: FileId) {
        let ids: Vec<EntityId> = (0..self.model.entities.len())
            .filter(|&e| self.model.entities[e].file == Some(file))
            .collect();
        for e in ids {
            let (ty, definition) = self.declared_type(e);
            let item_type = self.item_type_of(ty.as_ref());
            let entity = &mut self.model.entities[e];
            entity.ty = ty;
            entity.definition_node = definition;
            entity.item_type = item_type;
        }
    }

    /// `Entity.itemType`: the first of the type arguments of `Entity.type` when that type
    /// is the standard library's `List`, and undefined otherwise. `T[]` and `List<T>` are
    /// the same type, which the model spells as a list, so a bare `List` has no arguments
    /// and no item type.
    // @lfy def/model/main.lfy:bind
    fn item_type_of(&self, ty: Option<&TypeRef>) -> Option<TypeRef> {
        match ty {
            Some(TypeRef::List(item)) => Some((**item).clone()),
            Some(TypeRef::Entity(e)) if self.is_list(*e) => {
                self.model.type_arguments(*e).into_iter().next()
            }
            _ => None,
        }
    }

    /// Whether an entity is the standard library's `List`, or that declaration seen with
    /// arguments.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn is_list(&self, entity: EntityId) -> bool {
        self.prelude_data("List") == Some(self.model.generic_base(entity))
    }

    fn declared_type(&mut self, e: EntityId) -> (Option<TypeRef>, Option<NodeRef>) {
        let trees = self.trees.clone();
        let Some(node) = self.model.entities[e].node else {
            return (None, None);
        };
        let kind = self.model.entities[e].kind.clone();
        match kind {
            EntityKind::Data | EntityKind::Trait { .. } | EntityKind::Type | EntityKind::Enum => {
                let definition = self.definition_clause(node);
                (Some(TypeRef::Entity(e)), definition)
            }
            EntityKind::Fn { .. } => {
                let signature = trees.child(node, E::Signature);
                let definition = signature.and_then(|s| self.definition_clause(s));
                let output = trees
                    .child(node, E::TypeExpression)
                    .map(|t| self.type_from_type_expression(t));
                // FnEntity.parameters is the symbols of the Parameter and SpreadParameter
                // nodes of its Parameters, which a FunctionType declares in the scope it
                // owns just as a declaration does.
                // @lfy def/model/main.lfy:bind
                let parameters = self.value_parameters(e);
                if let EntityKind::Fn {
                    output: output_slot,
                    parameters: parameter_slot,
                    ..
                } = &mut self.model.entities[e].kind
                {
                    *output_slot = output;
                    *parameter_slot = parameters;
                }
                (Some(TypeRef::Function), definition)
            }
            // A type parameter's type is what it must extend, and its kind data is
            // Kinds.Parameter, whose defaultValue is the type after its setter.
            // @lfy def/model/main.lfy:bind
            EntityKind::Parameter if trees.is(node, E::TypeParameter) => {
                let (constraint, default) = self.type_parameter_clauses(node);
                let default = default.map(|t| Value::Type(Box::new(t)));
                let values = &mut self.model.entities[e].values;
                values.push((OPTIONAL.to_string(), Value::Bool(default.is_some())));
                values.push((
                    DEFAULT_VALUE.to_string(),
                    default.unwrap_or(Value::Undefined),
                ));
                values.push((SPREAD.to_string(), Value::Bool(false)));
                (constraint, None)
            }
            EntityKind::Parameter => {
                let (ty, definition) = self.clause_type_or_definition(node);
                let ty = ty.or_else(|| {
                    trees
                        .last_node(node)
                        .filter(|&v| !trees.is(v, E::Name) && !trees.is(v, E::DefinitionClause))
                        .map(|v| self.type_from_expression(v))
                });
                (ty, definition)
            }
            EntityKind::Member => {
                if trees.is(node, E::TypeKey) {
                    let (_, definition) = self.clause_type_or_definition(node);
                    let ty = trees
                        .child(node, E::TypeExpression)
                        .map(|t| self.type_from_type_expression(t));
                    (ty, definition)
                } else {
                    // `$name: desc = Type;`
                    let expression = trees.child_nodes(node).into_iter().next();
                    let mut ty = None;
                    let mut definition = None;
                    if let Some(expression) = expression {
                        let (left, right) = if trees.is(expression, E::Assignment) {
                            let parts = trees.child_nodes(expression);
                            (parts.first().copied(), parts.get(1).copied())
                        } else {
                            (Some(expression), None)
                        };
                        if let Some(left) = left
                            && trees.is(left, E::Definition)
                        {
                            definition = trees.child_nodes(left).get(1).copied();
                        }
                        if let Some(right) = right {
                            ty = Some(self.type_from_expression(right));
                        }
                    }
                    (ty, definition)
                }
            }
            EntityKind::Variable | EntityKind::Alias | EntityKind::External => {
                let declared = trees.child(node, E::Declared);
                let (mut ty, definition) =
                    declared.map_or((None, None), |d| self.clause_type_or_definition(d));
                if ty.is_none()
                    && let Some(value) = trees
                        .child_nodes(node)
                        .into_iter()
                        .find(|&c| !trees.is(c, E::Declared))
                {
                    ty = Some(self.type_from_expression(value));
                }
                (ty, definition)
            }
            EntityKind::LoopVariable => {
                let ty = self.loop_variable_type(node);
                (ty, None)
            }
            EntityKind::EnumMember => {
                let value = trees.child_nodes(node).get(1).copied();
                (value.map(|v| self.type_from_expression(v)), None)
            }
            EntityKind::Module => (None, None),
            _ => (None, None),
        }
    }

    /// The symbols of the parameters an entity declares: its value parameters, in order,
    /// its type parameters left out.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn value_parameters(&self, entity: EntityId) -> Vec<SymbolId> {
        match self.model.entities[entity].scope {
            Some(scope) => self.model.scopes[scope]
                .symbols
                .iter()
                .copied()
                .filter(|&s| self.model.symbols[s].kind == SymbolKind::Parameter)
                .collect(),
            None => Vec::new(),
        }
    }

    /// The type after a type parameter's `extends`, and the type after its `=`.
    // @lfy def/model/main.lfy:bind
    fn type_parameter_clauses(&mut self, node: NodeRef) -> (Option<TypeRef>, Option<TypeRef>) {
        let trees = self.trees.clone();
        let mut constraint = None;
        let mut default = None;
        let mut pending = None;
        for part in trees.parts(node) {
            match part {
                Part::Token(index) => {
                    if trees.token_is(node.file, index, K::ExtendsKeyword) {
                        pending = Some(false);
                    } else if trees.token_is(node.file, index, P::PlainSetter) {
                        pending = Some(true);
                    }
                }
                Part::Node(child) if trees.is(child, E::TypeExpression) => {
                    let ty = self.type_from_type_expression(child);
                    match pending.take() {
                        Some(true) => default = Some(ty),
                        Some(false) => constraint = Some(ty),
                        None => {}
                    }
                }
                Part::Node(_) => {}
            }
        }
        (constraint, default)
    }

    /// The template or string of a DefinitionClause, when it holds one.
    fn definition_clause(&self, node: NodeRef) -> Option<NodeRef> {
        let trees = &self.trees;
        let clause = trees.child(node, E::DefinitionClause)?;
        let expression = trees.child(clause, E::TypeExpression)?;
        self.definition_of_type_expression(expression)
    }

    fn definition_of_type_expression(&self, expression: NodeRef) -> Option<NodeRef> {
        let trees = &self.trees;
        let items = trees.children_of(expression, E::TypeItem);
        if items.len() != 1 {
            return None;
        }
        let value = trees.child_nodes(items[0]).into_iter().next()?;
        (trees.is(value, E::Template) || trees.is(value, E::StringLiteral)).then_some(value)
    }

    /// The type and definition a node with a DefinitionClause declares.
    fn clause_type_or_definition(&mut self, node: NodeRef) -> (Option<TypeRef>, Option<NodeRef>) {
        let trees = self.trees.clone();
        let Some(clause) = trees.child(node, E::DefinitionClause) else {
            return (None, None);
        };
        let Some(expression) = trees.child(clause, E::TypeExpression) else {
            return (None, None);
        };
        if let Some(definition) = self.definition_of_type_expression(expression) {
            return (None, Some(definition));
        }
        (Some(self.type_from_type_expression(expression)), None)
    }

    fn loop_variable_type(&mut self, node: NodeRef) -> Option<TypeRef> {
        let trees = self.trees.clone();
        let for_node = if trees.is(node, S::ForFrom) {
            trees.parent(node)?
        } else {
            node
        };
        let iterable = trees
            .child(for_node, S::ForInOf)
            .or_else(|| trees.child(for_node, S::ForFrom))?;
        let first = trees
            .child_nodes(iterable)
            .into_iter()
            .find(|&c| !trees.is(c, E::Declared))?;
        // `T@entities` gives the things carrying T.
        if trees.is(first, E::Member)
            && let Some((accessor, Some(name))) = trees.accessor_and_name(first)
            && trees.token_is(first.file, accessor, P::ContextAccessor)
            && (trees.token(first.file, name).value == "entities"
                || trees.token(first.file, name).value == "extenders")
            && let Some(left) = trees.left(first)
            && let Some(symbol) = self.iterable_symbol(for_node, left)
        {
            let entity = self.model.symbols[symbol].entity;
            return Some(TypeRef::Predicate(entity));
        }
        match self.type_from_expression(first) {
            TypeRef::List(item) => Some(*item),
            _ => None,
        }
    }

    /// The symbol the left of a For's iterable names, read as if the loop variable were
    /// not declared yet: `for (const rule in rule@entities)` iterates what the trait
    /// `rule` the enclosing scope names carries, because a loop variable's type is what
    /// is being worked out here and cannot be its own.
    // @lfy def/model/main.lfy:bind
    fn iterable_symbol(&self, for_node: NodeRef, left: NodeRef) -> Option<SymbolId> {
        let symbol = self.resolve_name_node(left)?;
        if Some(symbol) != self.model.symbol_of(for_node) {
            return Some(symbol);
        }
        let name = self.trees.name(left)?;
        let scope = self.model.scope_of(for_node)?;
        let outer = self.model.scopes[scope].parent?;
        self.lookup(outer, &name)
    }

    /// TypeRef of a type expression node.
    pub fn type_from_type_expression(&mut self, expression: NodeRef) -> TypeRef {
        let trees = self.trees.clone();
        let items: Vec<TypeRef> = trees
            .children_of(expression, E::TypeItem)
            .into_iter()
            .map(|item| self.type_from_type_item(item))
            .collect();
        match items.len() {
            0 => TypeRef::Unknown(trees.raw(expression)),
            1 => items.into_iter().next().expect("one"),
            _ => TypeRef::Union(items),
        }
    }

    /// A TypeItem: a type value or a parenthesized type, its type arguments when it has
    /// any, and square brackets that make a list of it.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    fn type_from_type_item(&mut self, item: NodeRef) -> TypeRef {
        let trees = self.trees.clone();
        // `T[]` is List of T; nested brackets nest the arguments.
        // @lfy def/model/main.lfy:bind
        let list = trees.has_token(item, P::ListOpen);
        let arguments = trees.child(item, E::TypeArguments);
        let inner = match trees.child_nodes(item).into_iter().next() {
            Some(value) if trees.is(value, E::TypeExpression) => {
                self.type_from_type_expression(value)
            }
            Some(value) => self.type_from_expression(value),
            None => TypeRef::Unknown(trees.raw(item)),
        };
        let inner = match arguments {
            // A Reference followed by TypeArguments is that declaration seen with them.
            // @lfy def/model/main.lfy:bind
            Some(arguments) => {
                let written = trees
                    .children_of(arguments, E::TypeExpression)
                    .into_iter()
                    .map(|argument| self.type_from_type_expression(argument))
                    .collect();
                self.generic_type(arguments, inner, written)
            }
            None => inner,
        };
        if list {
            TypeRef::List(Box::new(inner))
        } else {
            inner
        }
    }

    /// The declaration a type stands for, seen with the arguments a `TypeArguments` or a
    /// `Generic` node was written with: an entity whose identifier, definition, members,
    /// criteria, and type parameters are the declaration's, whose type is the declaration
    /// itself, and whose type arguments are what was written. `List` of one argument is
    /// spelled as a list, so `List<T>` and `T[]` are the same type.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    fn generic_type(&mut self, at: NodeRef, base: TypeRef, arguments: Vec<TypeRef>) -> TypeRef {
        if let Some(ty) = self.generics.get(&at) {
            return ty.clone();
        }
        let TypeRef::Entity(declaration) = base else {
            return base;
        };
        self.check_arity(at, declaration, arguments.len());
        if self.is_list(declaration) && arguments.len() == 1 {
            let list = TypeRef::List(Box::new(arguments.into_iter().next().expect("one")));
            self.generics.insert(at, list.clone());
            return list;
        }
        let entity = self.seen_with(declaration, arguments, Some(at));
        self.generics.insert(at, TypeRef::Entity(entity));
        TypeRef::Entity(entity)
    }

    /// One declaration seen with type arguments: its identifier, definition, members,
    /// criteria, and type parameters are the declaration's, its type is the declaration
    /// itself, and its type arguments are what was written.
    // @lfy def/model/main.lfy:bind
    fn seen_with(
        &mut self,
        declaration: EntityId,
        arguments: Vec<TypeRef>,
        at: Option<NodeRef>,
    ) -> EntityId {
        let source = self.model.entities[declaration].clone();
        let at = at.or(source.node);
        let entity = self.new_entity(source.kind.clone(), at, source.identifier.clone());
        let seen = &mut self.model.entities[entity];
        seen.definition = source.definition;
        seen.definition_node = source.definition_node;
        seen.acceptance_criteria = source.acceptance_criteria;
        seen.traits = source.traits;
        seen.scope = source.scope;
        seen.ty = Some(TypeRef::Entity(declaration));
        seen.values.push((
            TYPE_ARGUMENTS.to_string(),
            Value::List(
                arguments
                    .into_iter()
                    .map(|argument| Value::Type(Box::new(argument)))
                    .collect(),
            ),
        ));
        entity
    }

    /// More arguments than the declaration has type parameters, or fewer than those
    /// without a default, is a problem at the arguments; the entity is bound all the same.
    // @lfy def/model/main.lfy:bind
    fn check_arity(&mut self, at: NodeRef, declaration: EntityId, written: usize) {
        let parameters = self.model.type_parameters(declaration);
        let required = parameters
            .iter()
            .filter(|&&parameter| {
                !matches!(
                    self.model.entities[parameter].value(OPTIONAL),
                    Some(Value::Bool(true))
                )
            })
            .count();
        if written <= parameters.len() && written >= required {
            return;
        }
        let name = self.model.entities[declaration]
            .identifier
            .clone()
            .unwrap_or_else(|| "the declaration".to_string());
        let lists = match parameters.len() {
            1 => "1 type parameter".to_string(),
            other => format!("{other} type parameters"),
        };
        self.problem(
            at,
            format!("{written} type arguments, but {name} lists {lists}"),
        );
    }

    /// TypeRef of an expression standing as a type: after `=` in a member, a default
    /// value, or a variable's value.
    pub fn type_from_expression(&mut self, node: NodeRef) -> TypeRef {
        let trees = self.trees.clone();
        let rule = trees.rule(node);
        if rule == E::PrimitiveType.entity() {
            let token = trees.parts(node).into_iter().find_map(|p| match p {
                Part::Token(i) => Some(i),
                _ => None,
            });
            return match token.map(|i| trees.token(node.file, i).value.as_str()) {
                Some("boolean") => TypeRef::Primitive("boolean"),
                Some("number") => TypeRef::Primitive("number"),
                Some("string") => TypeRef::Primitive("string"),
                Some("object") => TypeRef::Primitive("object"),
                Some("function") => TypeRef::Primitive("function"),
                Some("trait") => TypeRef::Primitive("trait"),
                _ => TypeRef::Unknown(trees.raw(node)),
            };
        }
        if rule == E::Name.entity() || rule == E::Member.entity() {
            // A member is read on what the left side names, or, when the left side names
            // a value, in the entity of its type.
            // @lfy def/model/main.lfy:bind
            let symbol = self
                .resolve_name_node(node)
                .or_else(|| self.member_through_type(node));
            if let Some(symbol) = symbol {
                let kind = self.model.symbols[symbol].kind;
                let entity = self.model.symbols[symbol].entity;
                return match kind {
                    SymbolKind::Data | SymbolKind::Trait | SymbolKind::Type | SymbolKind::Enum => {
                        TypeRef::Entity(entity)
                    }
                    SymbolKind::Alias | SymbolKind::External => TypeRef::Entity(entity),
                    SymbolKind::Module => TypeRef::Entity(entity),
                    // A name that refers to a fn or function declaration is a function,
                    // whether or not the type pass has reached that declaration yet, so
                    // the value of a member written as one is a function.
                    // @lfy def/model/main.lfy:bind
                    SymbolKind::Function | SymbolKind::AgentFunction => TypeRef::Function,
                    // A name of a type parameter yields the type parameter's entity, not
                    // what it must extend.
                    // @lfy def/model/main.lfy:bind
                    SymbolKind::TypeParameter => TypeRef::Entity(entity),
                    // A member read on a left side with type arguments is its declared
                    // type with each of the base's type parameters replaced.
                    // @lfy def/model/main.lfy:bind
                    SymbolKind::Member if rule == E::Member.entity() => self
                        .member_type_at(node, entity)
                        .unwrap_or(TypeRef::Unknown(trees.raw(node))),
                    _ => self.model.entities[entity]
                        .ty
                        .clone()
                        .unwrap_or(TypeRef::Unknown(trees.raw(node))),
                };
            }
            return TypeRef::Unknown(trees.raw(node));
        }
        // A parenthesized type is the type inside it.
        // @lfy def/grammar/rules/expression.lfy:TypeGroup
        if rule == E::TypeGroup.entity() {
            return match trees.child(node, E::TypeExpression) {
                Some(inner) => self.type_from_type_expression(inner),
                None => TypeRef::Unknown(trees.raw(node)),
            };
        }
        // A FunctionType is the anonymous function entity the node owns.
        // @lfy def/model/main.lfy:bind
        if rule == E::FunctionType.entity() {
            return match self.function_type_entity(node) {
                Some(entity) => TypeRef::Entity(entity),
                None => TypeRef::Function,
            };
        }
        // The left expression with type arguments, in an expression position.
        // @lfy def/model/main.lfy:bind
        if rule == E::Generic.entity() {
            let base = match trees.left(node) {
                Some(left) => self.type_from_expression(left),
                None => return TypeRef::Unknown(trees.raw(node)),
            };
            let written = trees
                .children_of(node, E::TypeExpression)
                .into_iter()
                .map(|argument| self.type_from_type_expression(argument))
                .collect();
            return self.generic_type(node, base, written);
        }
        if rule == E::Reference.entity() {
            // A name in a type position is wrapped in a Reference.
            if let Some(inner) = trees.child_nodes(node).into_iter().next() {
                return self.type_from_expression(inner);
            }
            return TypeRef::Unknown(trees.raw(node));
        }
        if rule == E::Index.entity() {
            let parts = trees.child_nodes(node);
            if parts.len() == 1 {
                return TypeRef::List(Box::new(self.type_from_expression(parts[0])));
            }
            return TypeRef::Unknown(trees.raw(node));
        }
        if rule == E::BitwiseOrOperation.entity() {
            let mut items = Vec::new();
            for part in trees.child_nodes(node) {
                match self.type_from_expression(part) {
                    TypeRef::Union(inner) => items.extend(inner),
                    other => items.push(other),
                }
            }
            return TypeRef::Union(items);
        }
        if rule == E::Group.entity()
            && let Some(inner) = trees.child_nodes(node).into_iter().next()
        {
            return self.type_from_expression(inner);
        }
        if rule == E::Nullish.entity() {
            let text = trees.raw(node);
            let value = if text.trim() == "null" {
                Value::Null
            } else {
                Value::Undefined
            };
            return TypeRef::Literal(Box::new(value));
        }
        if rule == E::StringLiteral.entity() {
            return TypeRef::Literal(Box::new(Value::String(
                trees.string_value(node).unwrap_or_default(),
            )));
        }
        if rule == E::Template.entity() {
            // A template's base data, when the prelude declares one; its value is its
            // text, which is what it stands for without a prelude.
            // @lfy def/model/main.lfy:bind
            return match self.prelude_data("Template") {
                Some(data) => TypeRef::Entity(data),
                None => TypeRef::Literal(Box::new(Value::String(trees.raw(node)))),
            };
        }
        if rule == E::Number.entity() {
            let text = trees.raw(node);
            return TypeRef::Literal(Box::new(Value::Number(text.trim().parse().unwrap_or(0.0))));
        }
        if rule == E::Boolean.entity() {
            return TypeRef::Primitive("boolean");
        }
        // An object, whose keys are its own and whose members are the base data's.
        // @lfy def/model/main.lfy:bind
        if rule == E::Object.entity() {
            return TypeRef::Primitive("object");
        }
        // A list value is a list of what it holds: the types of its items, the same one
        // written once, so the value of a variable declared from one reads `List` and
        // its item type.
        // @lfy def/model/main.lfy:bind
        // @lfy def/model/main.lfy:bind
        if rule == E::List.entity() {
            // The items of a list are written as one `Items` node inside the brackets.
            // @lfy def/grammar/rules/expression.lfy:Items
            let written = match trees.child(node, E::Items) {
                Some(items) => trees.child_nodes(items),
                None => trees.child_nodes(node),
            };
            let mut items: Vec<TypeRef> = Vec::new();
            for item in written {
                let ty = self.item_type_from_expression(item);
                if !items.contains(&ty) {
                    items.push(ty);
                }
            }
            let item = match items.len() {
                0 => TypeRef::Unknown(trees.raw(node)),
                1 => items.into_iter().next().expect("one"),
                _ => TypeRef::Union(items),
            };
            return TypeRef::List(Box::new(item));
        }
        if rule == E::InlineFunction.entity() {
            return TypeRef::Function;
        }
        if rule == E::TypePredicate.entity()
            && let Some(trait_use) = trees.child(node, E::TraitUse)
            && let Some(symbol) = self.resolve_trait_use(trait_use)
        {
            return TypeRef::Predicate(self.model.symbols[symbol].entity);
        }
        if rule == E::TypeExpression.entity() {
            return self.type_from_type_expression(node);
        }
        if rule == E::Call.entity()
            && let Some(left) = trees.left(node)
            && let Some(symbol) = self.resolve_name_node(left)
        {
            let entity = self.model.symbols[symbol].entity;
            if let Some(output) = self.model.entities[entity].output() {
                return output.clone();
            }
        }
        TypeRef::Unknown(trees.raw(node))
    }

    /// What one item of a list value holds: the item itself, or, for a spread, what the
    /// list it spreads holds. A list holds values of the item's kind rather than the one
    /// value written, so a literal item is read as the primitive it is one of.
    // @lfy def/model/main.lfy:bind
    fn item_type_from_expression(&mut self, item: NodeRef) -> TypeRef {
        let trees = self.trees.clone();
        let ty = if trees.is(item, E::SpreadOperation) {
            match trees
                .child_nodes(item)
                .into_iter()
                .next()
                .map(|inner| self.type_from_expression(inner))
            {
                Some(TypeRef::List(inner)) => *inner,
                _ => TypeRef::Unknown(trees.raw(item)),
            }
        } else {
            self.type_from_expression(item)
        };
        match ty {
            TypeRef::Literal(value) => match *value {
                Value::String(_) => TypeRef::Primitive("string"),
                Value::Number(_) => TypeRef::Primitive("number"),
                Value::Bool(_) => TypeRef::Primitive("boolean"),
                other => TypeRef::Literal(Box::new(other)),
            },
            other => other,
        }
    }

    /// The anonymous function entity a FunctionType node owns: the current entity of the
    /// scope the node was given when it was declared.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn function_type_entity(&self, node: NodeRef) -> Option<EntityId> {
        let scope = self.model.scope_of(node)?;
        let entity = self.model.scopes[scope].current;
        matches!(self.model.entities[entity].kind, EntityKind::Fn { .. }).then_some(entity)
    }

    /// The member a `Member` node reads when its left side names a value: the member of
    /// the entity of the left side's type.
    // @lfy def/model/main.lfy:bind
    fn member_through_type(&mut self, node: NodeRef) -> Option<SymbolId> {
        let trees = self.trees.clone();
        if !trees.is(node, E::Member) {
            return None;
        }
        let (accessor, name) = trees.accessor_and_name(node)?;
        let layer = accessor_layer(trees.token(node.file, accessor).rule?)?;
        if layer != Layer::Value && layer != Layer::Scope {
            return None;
        }
        let name = trees.token(node.file, name?).value.clone();
        let left = trees.left(node)?;
        let TypeRef::Entity(base) = self.type_from_expression(left) else {
            return None;
        };
        self.member_symbol(base, &name)
    }

    /// The type of a member as it is read on a left side: its declared type with each of
    /// the base's type parameters replaced by the argument at the same position.
    // @lfy def/model/main.lfy:bind
    fn member_type_at(&mut self, node: NodeRef, member: EntityId) -> Option<TypeRef> {
        let declared = self.model.entities[member].ty.clone()?;
        let trees = self.trees.clone();
        let Some(left) = trees.left(node) else {
            return Some(declared);
        };
        let TypeRef::Entity(base) = self.type_from_expression(left) else {
            return Some(declared);
        };
        let parameters = self.model.type_parameters(base);
        let arguments = self.model.type_arguments(base);
        if parameters.is_empty() || arguments.is_empty() {
            return Some(declared);
        }
        Some(self.substitute(declared, &parameters, &arguments))
    }

    /// A type with each of `parameters` replaced by the argument at the same position,
    /// through nested type arguments and through the parameters and output of a function
    /// type. A parameter with no argument at its position stays itself, and nothing
    /// beyond this direct substitution is inferred.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn substitute(
        &mut self,
        ty: TypeRef,
        parameters: &[EntityId],
        arguments: &[TypeRef],
    ) -> TypeRef {
        if parameters.is_empty() {
            return ty;
        }
        match ty {
            TypeRef::Entity(entity) => {
                if let Some(position) = parameters.iter().position(|&p| p == entity) {
                    return match arguments.get(position) {
                        Some(argument) => argument.clone(),
                        None => TypeRef::Entity(entity),
                    };
                }
                let nested = self.model.type_arguments(entity);
                if !nested.is_empty() {
                    let base = self.model.generic_base(entity);
                    let nested = nested
                        .into_iter()
                        .map(|argument| self.substitute(argument, parameters, arguments))
                        .collect();
                    return TypeRef::Entity(self.seen_with(base, nested, None));
                }
                if matches!(self.model.entities[entity].kind, EntityKind::Fn { .. }) {
                    return TypeRef::Entity(self.substituted_function(
                        entity, parameters, arguments,
                    ));
                }
                TypeRef::Entity(entity)
            }
            TypeRef::List(item) => {
                TypeRef::List(Box::new(self.substitute(*item, parameters, arguments)))
            }
            TypeRef::Union(items) => TypeRef::Union(
                items
                    .into_iter()
                    .map(|item| self.substitute(item, parameters, arguments))
                    .collect(),
            ),
            other => other,
        }
    }

    /// A function type with its parameters and its output substituted. Its parameter
    /// symbols are copies: they name the same declarations and are in no scope of their
    /// own, so nothing else sees them.
    // @lfy def/model/main.lfy:bind
    fn substituted_function(
        &mut self,
        entity: EntityId,
        parameters: &[EntityId],
        arguments: &[TypeRef],
    ) -> EntityId {
        let source = self.model.entities[entity].clone();
        let mut substituted = Vec::new();
        for symbol in source.parameters().to_vec() {
            let original = self.model.symbols[symbol].clone();
            let declared = self.model.entities[original.entity].ty.clone();
            let node = self.model.entities[original.entity].node;
            let ty = declared.map(|ty| self.substitute(ty, parameters, arguments));
            let copy = self.new_entity(EntityKind::Parameter, node, Some(original.name.clone()));
            self.model.entities[copy].ty = ty;
            self.model.symbols.push(Symbol {
                entity: copy,
                ..original
            });
            substituted.push(self.model.symbols.len() - 1);
        }
        let output = source
            .output()
            .cloned()
            .map(|ty| self.substitute(ty, parameters, arguments));
        let copy = self.new_entity(
            EntityKind::Fn {
                parameters: substituted,
                output,
                agent: false,
            },
            source.node,
            None,
        );
        self.model.entities[copy].ty = Some(TypeRef::Function);
        self.model.entities[copy].scope = source.scope;
        copy
    }
}
