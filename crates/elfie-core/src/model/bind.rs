//! Compiled from `def/model/main.lfy` and `def/model/traits.lfy`: the declare, type, and
//! resolve passes of binding. The apply pass lives in `eval.rs`, which executes trait and
//! declaration bodies.

use std::collections::HashSet;
use std::rc::Rc;

use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::Statement as S;
use crate::grammar::terminals::comment::Comment as C;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::grammar::terminals::punctuation::Punctuation as P;
use crate::grammar::GrammarRule;

use super::components::{accessor_layer, declaring, is_scoped};
use super::data::*;
use super::trees::{Part, Trees};

/// The node reference of things that have no node: `global` and its scope.
pub const SYNTHETIC: NodeRef = NodeRef { file: usize::MAX, index: 0 };

pub(crate) struct Binder {
    pub trees: Rc<Trees>,
    pub model: Model,
    pub cache: std::collections::HashMap<EntityId, Value>,
    pub evaluating: HashSet<EntityId>,
    /// The universe scope, holding `global`.
    pub universe: ScopeId,
}

/// What the left side of a member access resolves to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Target {
    Module(FileId),
    Entity(EntityId),
    /// Anything carrying the trait; members are looked up leniently.
    Predicate(EntityId),
    Unknown,
}

impl Binder {
    pub fn new(sources: Vec<Source>) -> Binder {
        let trees = Rc::new(Trees::new(sources));
        let mut model = Model::default();
        // The global entity and the universe scope that holds its name.
        model.entities.push(Entity {
            identifier: Some("global".to_string()),
            definition: None,
            definition_node: None,
            ty: None,
            acceptance_criteria: Vec::new(),
            traits: Vec::new(),
            item_type: None,
            references: Vec::new(),
            tests: Vec::new(),
            values: Vec::new(),
            kind: EntityKind::Global,
            node: None,
            symbol: None,
            scope: None,
            file: None,
        });
        model.global = 0;
        model.scopes.push(Scope {
            parent: None,
            owner: SYNTHETIC,
            file: usize::MAX,
            symbols: Vec::new(),
            imports: Vec::new(),
            current: 0,
        });
        model.symbols.push(Symbol {
            name: "global".to_string(),
            entity: 0,
            kind: SymbolKind::External,
            node: SYNTHETIC,
            name_token: None,
            scope: 0,
        });
        model.scopes[0].symbols.push(0);
        model.entities[0].symbol = Some(0);
        model.entities[0].scope = Some(0);
        model.nodes = trees.nodes.clone();
        model.node_keys = trees.keys.clone();
        Binder {
            trees,
            model,
            cache: Default::default(),
            evaluating: Default::default(),
            universe: 0,
        }
    }

    pub fn problem(&mut self, node: NodeRef, message: impl Into<String>) {
        self.model.problems.push(Problem { node, message: message.into() });
    }

    pub fn new_entity(&mut self, kind: EntityKind, node: Option<NodeRef>, identifier: Option<String>) -> EntityId {
        self.model.entities.push(Entity {
            identifier,
            definition: None,
            definition_node: None,
            ty: None,
            acceptance_criteria: Vec::new(),
            traits: Vec::new(),
            item_type: None,
            references: Vec::new(),
            tests: Vec::new(),
            values: Vec::new(),
            kind,
            node,
            symbol: None,
            scope: None,
            file: node.map(|n| n.file),
        });
        self.model.entities.len() - 1
    }

    pub fn new_scope(&mut self, parent: Option<ScopeId>, owner: NodeRef, current: EntityId) -> ScopeId {
        self.model.scopes.push(Scope {
            parent,
            owner,
            file: owner.file,
            symbols: Vec::new(),
            imports: Vec::new(),
            current,
        });
        let id = self.model.scopes.len() - 1;
        self.model.scope_by_node.insert(owner, id);
        id
    }

    /// Adds a symbol to a scope; a second declaration of a name in the same scope is a
    /// problem and the first wins.
    // @lfy def/model/main.lfy:33
    pub fn add_symbol(
        &mut self,
        scope: ScopeId,
        name: String,
        entity: EntityId,
        kind: SymbolKind,
        node: NodeRef,
        name_token: Option<usize>,
    ) -> Option<SymbolId> {
        // Decision: a member (`$name`, reached through the scope layer) may share its name
        // with a parameter (`name`, reached through the value layer); everything else
        // declared twice in one scope is a problem.
        let clashes = self.model.scopes[scope].symbols.iter().any(|&s| {
            let existing = &self.model.symbols[s];
            existing.name == name && !((existing.kind == SymbolKind::Parameter) != (kind == SymbolKind::Parameter) && (existing.kind == SymbolKind::Member || kind == SymbolKind::Member))
        });
        if clashes {
            self.problem(node, format!("{name} is already declared in this scope"));
            return None;
        }
        self.model.symbols.push(Symbol { name, entity, kind, node, name_token, scope });
        let id = self.model.symbols.len() - 1;
        self.model.scopes[scope].symbols.push(id);
        if self.model.entities[entity].symbol.is_none() {
            self.model.entities[entity].symbol = Some(id);
        }
        self.model.symbol_by_node.insert(node, id);
        Some(id)
    }

    pub fn add_usage(&mut self, node: NodeRef, name: Option<String>, token: Option<usize>, layer: Layer, symbol: Option<SymbolId>) -> UsageId {
        self.model.usages.push(Usage { node, name, token, layer, symbol });
        let id = self.model.usages.len() - 1;
        self.model.usage_by_node.insert(node, id);
        id
    }

    /// The first symbol named `name` from `scope` outward, then `global`.
    pub fn lookup(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        self.model.lookup(scope, name).or_else(|| self.model.lookup_local(self.universe, name))
    }

    pub fn current_of(&self, r: NodeRef) -> EntityId {
        let scope = self.model.enclosing_scope(r);
        self.model.scopes[scope].current
    }

    // ---- Declare ------------------------------------------------------------------

    /// Each SourceFile is a file scope whose current entity is an anonymous entity for
    /// the file.
    // @lfy def/model/main.lfy:26
    pub fn declare_file(&mut self, file: FileId) {
        let trees = self.trees.clone();
        let root = NodeRef { file, index: 0 };
        let entity = self.new_entity(EntityKind::File, Some(root), None);
        let scope = self.new_scope(None, root, entity);
        self.model.entities[entity].scope = Some(scope);
        self.model.file_scopes.push(scope);
        self.model.file_entities.push(entity);
        for child in trees.children(root) {
            self.declare_node(child, scope);
        }
    }

    /// Every scoped node creates a scope and every declaring node a symbol and an entity.
    // @lfy def/model/main.lfy:27
    fn declare_node(&mut self, r: NodeRef, enclosing: ScopeId) {
        let trees = self.trees.clone();
        let rule = trees.rule(r);
        let mut scope = enclosing;
        let declared = declaring(rule).map(|(holder, kind)| {
            let name_token = trees.declared_name_token(r, holder);
            let name = name_token.map(|i| trees.token(r.file, i).value.clone());
            let entity = self.new_entity(entity_kind(kind), Some(r), name.clone());
            (entity, kind, name, name_token)
        });
        if is_scoped(rule) {
            let current = match declared {
                Some((entity, _, _, _)) => entity,
                None => self.model.scopes[enclosing].current,
            };
            scope = self.new_scope(Some(enclosing), r, current);
            if let Some((entity, _, _, _)) = declared {
                self.model.entities[entity].scope = Some(scope);
            }
        }
        if let Some((entity, kind, Some(name), name_token)) = declared {
            // A loop variable is added to the scope the For owns instead.
            // @lfy def/model/traits.lfy:43
            let target = if kind == SymbolKind::LoopVariable {
                if trees.is(r, S::ForFrom) { enclosing } else { scope }
            } else {
                enclosing
            };
            self.add_symbol(target, name, entity, kind, r, name_token);
        }
        // A member declared in a data or trait body.
        // @lfy def/model/main.lfy:31
        if let Some(current) = trees.member_declaration(r) {
            let (_, name_token) = trees.accessor_and_name(current).expect("checked");
            let name_token = name_token.expect("checked");
            let name = trees.token(r.file, name_token).value.clone();
            let owner_scope = self.model.scopes[enclosing].parent.unwrap_or(enclosing);
            let entity = self.new_entity(EntityKind::Member, Some(r), Some(name.clone()));
            if let Some(symbol) = self.add_symbol(owner_scope, name, entity, SymbolKind::Member, r, Some(name_token)) {
                self.model.symbol_by_node.insert(current, symbol);
            }
        }
        // An ObjectKey in the Object of an EnumDeclaration declares an enum member.
        // @lfy def/model/main.lfy:32
        if trees.is(r, E::ObjectKey)
            && trees.parent(r).is_some_and(|p| trees.is(p, E::Object))
            && trees.parent(r).and_then(|p| trees.parent(p)).is_some_and(|g| trees.is(g, S::EnumDeclaration))
        {
            if let Some(declared) = trees.child(r, E::Declared) {
                if let Some(token) = trees.child_token(declared, crate::grammar::terminals::identifier::Identifier::Identifier) {
                    let name = trees.token(r.file, token).value.clone();
                    let entity = self.new_entity(EntityKind::EnumMember, Some(r), Some(name.clone()));
                    self.add_symbol(enclosing, name, entity, SymbolKind::EnumMember, r, Some(token));
                }
            }
        }
        for child in trees.children(r) {
            self.declare_node(child, scope);
        }
    }

    /// A Use refers to the Source its entry in Source.uses names.
    // @lfy def/model/main.lfy:38
    pub fn link_uses(&mut self, file: FileId) {
        let trees = self.trees.clone();
        let root = NodeRef { file, index: 0 };
        let uses: Vec<NodeRef> = trees
            .children(root)
            .into_iter()
            .filter(|&c| trees.is(c, S::Use))
            .collect();
        for (i, use_node) in uses.into_iter().enumerate() {
            let Some(Some(path)) = trees.sources[file].uses.get(i) else {
                continue;
            };
            let Some(target) = trees.sources.iter().position(|s| &s.path == path) else {
                self.problem(use_node, format!("{path} is not in the program"));
                continue;
            };
            if trees.has_token(use_node, K::AsKeyword) {
                // The module symbol's entity is the used file's entity.
                // @lfy def/model/main.lfy:40
                if let Some(symbol) = self.model.symbol_of(use_node) {
                    let file_entity = self.model.file_entities[target];
                    self.model.symbols[symbol].entity = file_entity;
                }
            } else {
                // The used file's own symbols, not its imports, are visible.
                // @lfy def/model/main.lfy:39
                let own: Vec<SymbolId> = self.model.scopes[self.model.file_scopes[target]].symbols.clone();
                let scope = self.model.file_scopes[file];
                for symbol in own {
                    if !self.model.scopes[scope].imports.contains(&symbol) {
                        self.model.scopes[scope].imports.push(symbol);
                    }
                }
            }
        }
    }

    // ---- Types --------------------------------------------------------------------

    /// Entity.type and Entity.definition nodes for every entity of a file, from its
    /// declaration alone.
    // @lfy def/model/main.lfy:90
    pub fn type_entities(&mut self, file: FileId) {
        let ids: Vec<EntityId> = (0..self.model.entities.len())
            .filter(|&e| self.model.entities[e].file == Some(file))
            .collect();
        for e in ids {
            let (ty, definition) = self.declared_type(e);
            let entity = &mut self.model.entities[e];
            entity.ty = ty;
            entity.definition_node = definition;
            entity.item_type = match &entity.ty {
                Some(TypeRef::List(item)) => Some((**item).clone()),
                _ => None,
            };
        }
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
                let output = trees.child(node, E::TypeExpression).map(|t| self.type_from_type_expression(t));
                if let EntityKind::Fn { output: slot, .. } = &mut self.model.entities[e].kind {
                    *slot = output;
                }
                (Some(TypeRef::Function), definition)
            }
            EntityKind::Parameter => {
                let (ty, definition) = self.clause_type_or_definition(node);
                let ty = ty.or_else(|| {
                    trees.last_node(node).filter(|&v| !trees.is(v, E::Name) && !trees.is(v, E::DefinitionClause)).map(|v| self.type_from_expression(v))
                });
                (ty, definition)
            }
            EntityKind::Member => {
                if trees.is(node, E::TypeKey) {
                    let (_, definition) = self.clause_type_or_definition(node);
                    let ty = trees.child(node, E::TypeExpression).map(|t| self.type_from_type_expression(t));
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
                        if let Some(left) = left {
                            if trees.is(left, E::Definition) {
                                definition = trees.child_nodes(left).get(1).copied();
                            }
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
                let (mut ty, definition) = declared.map_or((None, None), |d| self.clause_type_or_definition(d));
                if ty.is_none() {
                    if let Some(value) = trees.child_nodes(node).into_iter().find(|&c| !trees.is(c, E::Declared)) {
                        ty = Some(self.type_from_expression(value));
                    }
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
        let for_node = if trees.is(node, S::ForFrom) { trees.parent(node)? } else { node };
        let iterable = trees
            .child(for_node, S::ForInOf)
            .or_else(|| trees.child(for_node, S::ForFrom))?;
        let first = trees.child_nodes(iterable).into_iter().find(|&c| !trees.is(c, E::Declared))?;
        // `T@entities` gives the things carrying T.
        if trees.is(first, E::Member) {
            if let Some((accessor, Some(name))) = trees.accessor_and_name(first) {
                if trees.token_is(first.file, accessor, P::ContextAccessor)
                    && (trees.token(first.file, name).value == "entities" || trees.token(first.file, name).value == "extenders")
                {
                    if let Some(left) = trees.left(first) {
                        if let Some(symbol) = self.resolve_name_node(left) {
                            let entity = self.model.symbols[symbol].entity;
                            return Some(TypeRef::Predicate(entity));
                        }
                    }
                }
            }
        }
        match self.type_from_expression(first) {
            TypeRef::List(item) => Some(*item),
            _ => None,
        }
    }

    /// The symbol a Name node names, looked up statically.
    pub fn resolve_name_node(&self, node: NodeRef) -> Option<SymbolId> {
        let trees = &self.trees;
        if trees.is(node, E::Name) {
            let name = trees.name(node)?;
            return self.lookup(self.model.enclosing_scope(node), &name);
        }
        if trees.is(node, E::Member) {
            let left = trees.left(node)?;
            let (accessor, name) = trees.accessor_and_name(node)?;
            if !trees.token_is(node.file, accessor, P::ValueAccessor) {
                return None;
            }
            let name = trees.token(node.file, name?).value.clone();
            let left = self.resolve_name_node(left)?;
            let entity = self.model.symbols[left].entity;
            return self.member_symbol(entity, &name);
        }
        None
    }

    /// The member symbol of an entity: its own scope's symbols, or a file's symbols.
    pub fn member_symbol(&self, entity: EntityId, name: &str) -> Option<SymbolId> {
        let scope = self.model.entities[entity].scope?;
        self.model.lookup_local(scope, name)
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

    fn type_from_type_item(&mut self, item: NodeRef) -> TypeRef {
        let trees = self.trees.clone();
        let list = trees.has_token(item, P::ListOpen);
        let inner = match trees.child_nodes(item).into_iter().next() {
            Some(value) if trees.is(value, E::TypeExpression) => self.type_from_type_expression(value),
            Some(value) => self.type_from_expression(value),
            None => TypeRef::Unknown(trees.raw(item)),
        };
        if list { TypeRef::List(Box::new(inner)) } else { inner }
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
            if let Some(symbol) = self.resolve_name_node(node) {
                let symbol = &self.model.symbols[symbol];
                return match symbol.kind {
                    SymbolKind::Data | SymbolKind::Trait | SymbolKind::Type | SymbolKind::Enum => TypeRef::Entity(symbol.entity),
                    SymbolKind::Alias | SymbolKind::External => TypeRef::Entity(symbol.entity),
                    SymbolKind::Module => TypeRef::Entity(symbol.entity),
                    _ => self.model.entities[symbol.entity].ty.clone().unwrap_or(TypeRef::Unknown(trees.raw(node))),
                };
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
        if rule == E::Group.entity() {
            if let Some(inner) = trees.child_nodes(node).into_iter().next() {
                return self.type_from_expression(inner);
            }
        }
        if rule == E::Nullish.entity() {
            let text = trees.raw(node);
            let value = if text.trim() == "null" { Value::Null } else { Value::Undefined };
            return TypeRef::Literal(Box::new(value));
        }
        if rule == E::StringLiteral.entity() {
            return TypeRef::Literal(Box::new(Value::String(trees.string_value(node).unwrap_or_default())));
        }
        if rule == E::Template.entity() {
            return TypeRef::Literal(Box::new(Value::String(trees.raw(node))));
        }
        if rule == E::Number.entity() {
            let text = trees.raw(node);
            return TypeRef::Literal(Box::new(Value::Number(text.trim().parse().unwrap_or(0.0))));
        }
        if rule == E::Boolean.entity() {
            return TypeRef::Primitive("boolean");
        }
        if rule == E::InlineFunction.entity() {
            return TypeRef::Function;
        }
        if rule == E::TypePredicate.entity() {
            if let Some(trait_use) = trees.child(node, E::TraitUse) {
                if let Some(symbol) = self.resolve_trait_use(trait_use) {
                    return TypeRef::Predicate(self.model.symbols[symbol].entity);
                }
            }
        }
        if rule == E::TypeExpression.entity() {
            return self.type_from_type_expression(node);
        }
        if rule == E::Call.entity() {
            if let Some(left) = trees.left(node) {
                if let Some(symbol) = self.resolve_name_node(left) {
                    let entity = self.model.symbols[symbol].entity;
                    if let Some(output) = self.model.entities[entity].output() {
                        return output.clone();
                    }
                }
            }
        }
        TypeRef::Unknown(trees.raw(node))
    }

    /// The trait symbol a TraitUse names, statically.
    pub fn resolve_trait_use(&self, trait_use: NodeRef) -> Option<SymbolId> {
        let trees = &self.trees;
        let identifiers: Vec<usize> = trees
            .parts(trait_use)
            .into_iter()
            .filter_map(|p| match p {
                Part::Token(i) if trees.token_is(trait_use.file, i, crate::grammar::terminals::identifier::Identifier::Identifier) => Some(i),
                _ => None,
            })
            .collect();
        let scope = self.model.enclosing_scope(trait_use);
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

    // ---- Resolve ------------------------------------------------------------------

    /// Every reading node of a file gets a Usage, resolved as the model says.
    // @lfy def/model/main.lfy:64
    pub fn resolve_file(&mut self, file: FileId) {
        let root = NodeRef { file, index: 0 };
        self.resolve_node(root);
    }

    fn resolve_node(&mut self, r: NodeRef) {
        let trees = self.trees.clone();
        let rule = trees.rule(r);
        // A With runs its block with the entity its expression resolves to as current.
        // @lfy def/model/main.lfy:34
        if rule == S::With.entity() {
            let children = trees.child_nodes(r);
            for &child in &children {
                if !trees.is(child, S::Block) {
                    self.resolve_node(child);
                    if let Some(usage) = self.model.usage_of(child) {
                        if let Some(symbol) = self.model.usages[usage].symbol {
                            let entity = self.model.symbols[symbol].entity;
                            if let Some(scope) = self.model.scope_of(r) {
                                self.model.scopes[scope].current = entity;
                            }
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
            // @lfy def/model/main.lfy:75
            let operand = trees.child_nodes(r).into_iter().next();
            let symbol = operand.and_then(|o| self.model.usage_of(o)).and_then(|u| self.model.usages[u].symbol);
            let token = operand.and_then(|o| self.model.usage_of(o)).and_then(|u| self.model.usages[u].token);
            self.add_usage(r, operand.map(|o| trees.raw(o).trim().to_string()), token, Layer::Dereference, symbol);
        } else if rule == E::Previous.entity() {
            // A Previous yields the entity declared by the nearest earlier statement.
            // @lfy def/model/main.lfy:76
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
            // @lfy def/model/data.lfy:33
            if let Some(symbol) = self.model.symbol_of(r) {
                let value = trees.child_nodes(r).into_iter().find(|&c| !trees.is(c, E::Declared));
                if let Some(target) = value.and_then(|v| self.model.usage_of(v)).and_then(|u| self.model.usages[u].symbol) {
                    let entity = self.model.symbols[target].entity;
                    self.model.symbols[symbol].entity = entity;
                }
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
        if self.trees.ancestor(r, E::TemplateReference).is_some() {
            if let Some(entity) = self.enclosing_entity(r) {
                self.model.entities[entity].references.push(usage);
            }
        }
    }

    /// A Name resolves to the first match walking from its scope outward; inside a
    /// template, a name found nowhere resolves to the one rule entity with that name.
    // @lfy def/model/main.lfy:64
    fn resolve_name(&mut self, r: NodeRef) {
        let trees = self.trees.clone();
        // The Name that spells a declared name is the declaration, not a usage.
        if let Some(parent) = trees.parent(r) {
            if declaring(trees.rule(parent)).is_some_and(|(holder, _)| holder == E::Name.entity()) {
                return;
            }
        }
        let Some(name) = trees.name(r) else { return };
        let token = trees.name_token(r);
        let scope = self.model.enclosing_scope(r);
        let mut symbol = self.lookup(scope, &name);
        if symbol.is_none() && self.in_template(r) {
            symbol = self.model.rule_entity(&name).and_then(|e| self.model.entities[e].symbol);
        }
        if symbol.is_none() {
            self.problem(r, format!("{name} is not declared in scope"));
        }
        let usage = self.add_usage(r, Some(name), token, Layer::Value, symbol);
        self.record_reference(r, usage);
    }

    /// A Current is bound with the scope's current entity as the left side.
    // @lfy def/model/main.lfy:66
    fn resolve_current(&mut self, r: NodeRef) {
        let trees = self.trees.clone();
        let Some((accessor, name)) = trees.accessor_and_name(r) else { return };
        let layer = accessor_layer(trees.token(r.file, accessor).rule.expect("accessor")).expect("layer");
        // The Current that declares a member is the declaration, not a usage.
        if self.model.symbol_of(r).is_some() {
            return;
        }
        let current = self.current_of(r);
        let name_text = name.map(|i| trees.token(r.file, i).value.clone());
        let symbol = self.resolve_in(r, Target::Entity(current), layer, name_text.as_deref(), true);
        let usage = self.add_usage(r, name_text, Some(name.unwrap_or(accessor)), layer, symbol);
        self.record_reference(r, usage);
    }

    /// A Member resolves its name in what its left side resolves to.
    // @lfy def/model/main.lfy:66
    fn resolve_member(&mut self, r: NodeRef) {
        let trees = self.trees.clone();
        let Some((accessor, name)) = trees.accessor_and_name(r) else { return };
        let layer = accessor_layer(trees.token(r.file, accessor).rule.expect("accessor")).expect("layer");
        let target = trees.left(r).map_or(Target::Unknown, |left| self.target_of(left));
        let name_text = name.map(|i| trees.token(r.file, i).value.clone());
        let symbol = self.resolve_in(r, target, layer, name_text.as_deref(), false);
        let usage = self.add_usage(r, name_text, Some(name.unwrap_or(accessor)), layer, symbol);
        self.record_reference(r, usage);
    }

    /// Resolves a member name in a target through one layer, adding problems as the
    /// model says.
    fn resolve_in(&mut self, r: NodeRef, target: Target, layer: Layer, name: Option<&str>, current: bool) -> Option<SymbolId> {
        let Some(name) = name else { return None };
        match layer {
            Layer::Context => {
                // The MemberName must be the value of a ContextProperty.
                // @lfy def/model/main.lfy:70
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
                    let symbol = self.member_symbol(entity, name).or_else(|| self.inherited_member(entity, name));
                    if symbol.is_none() {
                        let lenient = matches!(
                            self.model.entities[entity].kind,
                            EntityKind::Trait { .. } | EntityKind::External | EntityKind::Alias | EntityKind::Parameter | EntityKind::Variable | EntityKind::LoopVariable
                        ) || self.model.entities[entity].scope.is_none();
                        if !lenient {
                            let owner = self.model.entities[entity].identifier.clone().unwrap_or_else(|| "the current entity".to_string());
                            let what = if current { "the current entity" } else { owner.as_str() };
                            self.problem(r, format!("{what} has no member {name}"));
                        }
                    }
                    symbol
                }
                Target::Predicate(trait_entity) => self.member_symbol(trait_entity, name).or_else(|| self.inherited_member(trait_entity, name)),
                Target::Unknown => None,
            },
        }
    }

    /// A member reached through the traits an entity carries or extends.
    fn inherited_member(&self, entity: EntityId, name: &str) -> Option<SymbolId> {
        for applied in &self.model.entities[entity].traits {
            if let Some(symbol) = self.member_symbol(applied.entity, name) {
                return Some(symbol);
            }
        }
        None
    }

    /// What an expression on the left of an accessor resolves to.
    pub(crate) fn target_of(&mut self, node: NodeRef) -> Target {
        let trees = self.trees.clone();
        let rule = trees.rule(node);
        if rule == E::Name.entity() || rule == E::Member.entity() || rule == E::TraitUse.entity() {
            let symbol = self.model.usage_of(node).and_then(|u| self.model.usages[u].symbol);
            return match symbol {
                Some(symbol) => self.target_of_symbol(symbol),
                None => Target::Unknown,
            };
        }
        if rule == E::Current.entity() {
            let Some((accessor, name)) = trees.accessor_and_name(node) else { return Target::Unknown };
            let current = self.current_of(node);
            if name.is_none() {
                return if trees.token_is(node.file, accessor, P::ContextAccessor) { Target::Unknown } else { Target::Entity(current) };
            }
            let symbol = self.model.usage_of(node).and_then(|u| self.model.usages[u].symbol);
            return match symbol {
                Some(symbol) => self.target_of_symbol(symbol),
                None => Target::Unknown,
            };
        }
        if rule == E::Group.entity() {
            return trees.child_nodes(node).into_iter().next().map_or(Target::Unknown, |inner| self.target_of(inner));
        }
        if rule == E::Dereference.entity() {
            let symbol = self.model.usage_of(node).and_then(|u| self.model.usages[u].symbol);
            return symbol.map_or(Target::Unknown, |s| Target::Entity(self.model.symbols[s].entity));
        }
        if rule == E::Call.entity() {
            if let Some(left) = trees.left(node) {
                let symbol = self.model.usage_of(left).and_then(|u| self.model.usages[u].symbol);
                if let Some(symbol) = symbol {
                    let entity = self.model.symbols[symbol].entity;
                    if let Some(output) = self.model.entities[entity].output() {
                        return self.target_of_type(output.clone());
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
                Some(file) if matches!(self.model.entities[symbol.entity].kind, EntityKind::File) => Target::Module(file),
                _ => Target::Unknown,
            },
            SymbolKind::Data | SymbolKind::Trait | SymbolKind::Type | SymbolKind::Enum | SymbolKind::Function | SymbolKind::AgentFunction => {
                Target::Entity(symbol.entity)
            }
            SymbolKind::Alias | SymbolKind::External => match &self.model.entities[symbol.entity].kind {
                EntityKind::Data | EntityKind::Trait { .. } | EntityKind::Type | EntityKind::Enum => Target::Entity(symbol.entity),
                _ => Target::Unknown,
            },
            _ => match self.model.entities[symbol.entity].ty.clone() {
                Some(ty) => self.target_of_type(ty),
                None => Target::Unknown,
            },
        }
    }

    fn target_of_type(&self, ty: TypeRef) -> Target {
        match ty {
            TypeRef::Entity(e) => match self.model.entities[e].kind {
                EntityKind::File => Target::Module(self.model.entities[e].file.expect("file entity")),
                EntityKind::Trait { .. } => Target::Predicate(e),
                _ => Target::Entity(e),
            },
            TypeRef::Predicate(t) => Target::Predicate(t),
            TypeRef::Union(items) => {
                // A union of entities: the first entity, leniently.
                for item in items {
                    if let TypeRef::Entity(e) = item {
                        return Target::Predicate(e);
                    }
                }
                Target::Unknown
            }
            _ => Target::Unknown,
        }
    }

    /// The symbol declared by the nearest earlier statement of the same block.
    fn previous_symbol(&self, r: NodeRef) -> Option<SymbolId> {
        let trees = &self.trees;
        let mut statement = r;
        while let Some(parent) = trees.parent(statement) {
            if trees.is_statement(statement) && (trees.is(parent, S::Block) || trees.is(parent, crate::grammar::rules::file::File::SourceFile)) {
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

fn entity_kind(kind: SymbolKind) -> EntityKind {
    match kind {
        SymbolKind::Data => EntityKind::Data,
        SymbolKind::Trait => EntityKind::Trait { parameters: Vec::new(), entities: Vec::new(), extenders: Vec::new() },
        SymbolKind::Function => EntityKind::Fn { parameters: Vec::new(), output: None, agent: false },
        SymbolKind::AgentFunction => EntityKind::Fn { parameters: Vec::new(), output: None, agent: true },
        SymbolKind::Type => EntityKind::Type,
        SymbolKind::Enum => EntityKind::Enum,
        SymbolKind::Variable => EntityKind::Variable,
        SymbolKind::Alias => EntityKind::Alias,
        SymbolKind::External => EntityKind::External,
        SymbolKind::Module => EntityKind::Module,
        SymbolKind::LoopVariable => EntityKind::LoopVariable,
        SymbolKind::Parameter => EntityKind::Parameter,
        SymbolKind::Member => EntityKind::Member,
        SymbolKind::EnumMember => EntityKind::EnumMember,
    }
}
