//! Compiled from `def/model/main.lfy` and `def/model/traits.lfy`: the declare, type, and
//! resolve passes of binding. The apply pass lives in `eval.rs`, which executes trait and
//! declaration bodies.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::grammar::GrammarRule;
use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::Statement as S;
use crate::grammar::terminals::comment::Comment as C;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::grammar::terminals::punctuation::Punctuation as P;

use super::components::{accessor_layer, declaring, is_scoped};
use super::data::*;
use super::trees::{Part, Trees};

/// The node reference of things that have no node: `global` and its scope.
pub const SYNTHETIC: NodeRef = NodeRef {
    file: usize::MAX,
    index: 0,
};

pub(crate) struct Binder {
    pub trees: Rc<Trees>,
    pub model: Model,
    pub cache: HashMap<EntityId, Value>,
    pub evaluating: HashSet<EntityId>,
    /// The universe scope, holding `global`.
    pub universe: ScopeId,
    /// The prelude scope, when a source has [`Origin::Prelude`].
    // @lfy def/model/main.lfy:bind
    pub prelude: Option<ScopeId>,
    /// The type each `TypeArguments` or `Generic` node yields, so that a node written
    /// once is one entity however often its type is read, and its arity is reported once.
    // @lfy def/model/main.lfy:bind
    pub generics: HashMap<NodeRef, TypeRef>,
}

/// The name under which a generic entity holds what it was written with, and a type
/// parameter what its declaration gave it. `Entity` is a record of the model, and the
/// prelude's `Entity` declares these as members of it, so the binder records them where
/// the value of a member of an entity lives.
// @lfy def/model/main.lfy:bind
pub const TYPE_ARGUMENTS: &str = "typeArguments";
/// `Entity.typeParameters`: what a declaration is generic over.
// @lfy def/model/main.lfy:bind
pub const TYPE_PARAMETERS: &str = "typeParameters";
/// `Kinds.Parameter.defaultValue`: what a type parameter takes when it is left out.
// @lfy def/model/main.lfy:bind
pub const DEFAULT_VALUE: &str = "defaultValue";
/// `Kinds.Parameter.optional`: whether a type parameter may be left out.
// @lfy def/model/main.lfy:bind
pub const OPTIONAL: &str = "optional";
/// `Kinds.Parameter.spread`: never true of a type parameter.
// @lfy def/model/main.lfy:bind
pub const SPREAD: &str = "spread";

/// The data the prelude gives an entity for what it is; `Entity` covers every entity and
/// the others only their own kind.
// @lfy def/model/main.lfy:bind
const KIND_DATA: [&str; 10] = [
    "Entity", "Trait", "Function", "Data", "Type", "Enum", "Member", "Parameter", "Variable",
    "Module",
];

/// What the left side of a member access resolves to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Target {
    Module(FileId),
    Entity(EntityId),
    /// Anything carrying the trait; members are looked up leniently.
    Predicate(EntityId),
    /// A value of a base data: its members are that data's.
    // @lfy def/model/main.lfy:bind
    Base(EntityId),
    Unknown,
}

impl Target {
    /// The entity a member name is read on, when the target is one.
    fn entity(&self) -> Option<EntityId> {
        match *self {
            Target::Entity(entity) | Target::Predicate(entity) => Some(entity),
            _ => None,
        }
    }
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
            prelude: None,
            generics: Default::default(),
        }
    }

    pub fn problem(&mut self, node: NodeRef, message: impl Into<String>) {
        self.model.problems.push(Problem {
            node,
            message: message.into(),
        });
    }

    pub fn new_entity(
        &mut self,
        kind: EntityKind,
        node: Option<NodeRef>,
        identifier: Option<String>,
    ) -> EntityId {
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

    pub fn new_scope(
        &mut self,
        parent: Option<ScopeId>,
        owner: NodeRef,
        current: EntityId,
    ) -> ScopeId {
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
    /// problem and the first wins. Only the scope itself is consulted, so a name declared
    /// in a child scope shadows the parent's without a problem.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
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
            existing.name == name
                && !((existing.kind == SymbolKind::Parameter) != (kind == SymbolKind::Parameter)
                    && (existing.kind == SymbolKind::Member || kind == SymbolKind::Member))
        });
        if clashes {
            self.problem(node, format!("{name} is already declared in this scope"));
            return None;
        }
        self.model.symbols.push(Symbol {
            name,
            entity,
            kind,
            node,
            name_token,
            scope,
        });
        let id = self.model.symbols.len() - 1;
        self.model.scopes[scope].symbols.push(id);
        if self.model.entities[entity].symbol.is_none() {
            self.model.entities[entity].symbol = Some(id);
        }
        self.model.symbol_by_node.insert(node, id);
        Some(id)
    }

    pub fn add_usage(
        &mut self,
        node: NodeRef,
        name: Option<String>,
        token: Option<usize>,
        layer: Layer,
        symbol: Option<SymbolId>,
    ) -> UsageId {
        self.model.usages.push(Usage {
            node,
            name,
            token,
            layer,
            symbol,
        });
        let id = self.model.usages.len() - 1;
        self.model.usage_by_node.insert(node, id);
        id
    }

    /// The first symbol named `name` from `scope` outward, then `global`.
    pub fn lookup(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        self.model
            .lookup(scope, name)
            .or_else(|| self.model.lookup_local(self.universe, name))
    }

    /// The same, for a name written at `node`: the member a declaration declares is not
    /// among the names its own value sees, so the value of `Trait$apply` is the fn
    /// `apply` the file declares and not the member being declared.
    // @lfy def/model/main.lfy:bind
    fn lookup_at(&self, node: NodeRef, scope: ScopeId, name: &str) -> Option<SymbolId> {
        let found = self.lookup(scope, name)?;
        if !self.declares_at(node, found) {
            return Some(found);
        }
        let mut current = Some(scope);
        while let Some(id) = current {
            let scope = &self.model.scopes[id];
            if let Some(&symbol) = scope
                .symbols
                .iter()
                .chain(scope.imports.iter())
                .find(|&&symbol| symbol != found && self.model.symbols[symbol].name == name)
            {
                return Some(symbol);
            }
            current = scope.parent;
        }
        self.model.lookup_local(self.universe, name)
    }

    /// Whether the symbol is the member whose own declaration the node stands in.
    // @lfy def/model/main.lfy:bind
    fn declares_at(&self, node: NodeRef, symbol: SymbolId) -> bool {
        let symbol = &self.model.symbols[symbol];
        if symbol.kind != SymbolKind::Member || symbol.node.file != node.file {
            return false;
        }
        let mut current = Some(node);
        while let Some(at) = current {
            if at == symbol.node {
                return true;
            }
            current = self.trees.parent(at);
        }
        false
    }

    pub fn current_of(&self, r: NodeRef) -> EntityId {
        let scope = self.model.enclosing_scope(r);
        self.model.scopes[scope].current
    }

    // ---- Declare ------------------------------------------------------------------

    /// Each SourceFile is a file scope whose current entity is an anonymous entity for
    /// the file. Its parent is settled by [`Binder::link_prelude`], once every file has
    /// been declared.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/traits.lfy:scoped
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

    /// A file scope's parent is the prelude scope, and none for a file whose origin is
    /// the library or the prelude itself. Every file has been declared by now, so the
    /// prelude's scope exists whatever order the files were given in. With no source of
    /// [`Origin::Prelude`] no file scope has a parent, and a name only the prelude would
    /// give is found nowhere.
    // @lfy def/model/data.lfy:Scope
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/traits.lfy:scoped
    pub fn link_prelude(&mut self) {
        let trees = self.trees.clone();
        let Some(prelude) = trees
            .sources
            .iter()
            .position(|source| source.origin == Origin::Prelude)
            .map(|file| self.model.file_scopes[file])
        else {
            return;
        };
        self.prelude = Some(prelude); // @lfy def/model/main.lfy:bind
        for (file, source) in trees.sources.iter().enumerate() {
            if source.origin != Origin::Program {
                continue;
            }
            let scope = self.model.file_scopes[file];
            self.model.scopes[scope].parent = Some(prelude);
        }
    }

    /// Every scoped node creates a scope and every declaring node a symbol and an entity.
    // @lfy def/model/main.lfy:bind
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
        // A FunctionType declares no name, but it is a function all the same: it owns an
        // anonymous entity whose parameters are declared in the scope it owns and whose
        // output is the type after its arrow.
        // @lfy def/model/main.lfy:bind
        let function_type = trees.is(r, E::FunctionType).then(|| {
            self.new_entity(
                EntityKind::Fn {
                    parameters: Vec::new(),
                    output: None,
                    agent: false,
                },
                Some(r),
                None,
            )
        });
        if is_scoped(rule) {
            // A For declares its loop variable, not itself: its current stays the parent's.
            let current = match (&declared, function_type) {
                (Some((entity, kind, _, _)), _) if *kind != SymbolKind::LoopVariable => *entity,
                (_, Some(entity)) => entity,
                _ => self.model.scopes[enclosing].current,
            };
            scope = self.new_scope(Some(enclosing), r, current);
            if let Some((entity, _, _, _)) = declared {
                self.model.entities[entity].scope = Some(scope);
            }
            if let Some(entity) = function_type {
                self.model.entities[entity].scope = Some(scope);
            }
        }
        if let Some((entity, kind, Some(name), name_token)) = declared {
            // A loop variable is added to the scope the For owns instead.
            // @lfy def/model/traits.lfy:declaring
            let target = if kind == SymbolKind::LoopVariable {
                if trees.is(r, S::ForFrom) {
                    enclosing
                } else {
                    scope
                }
            } else {
                enclosing
            };
            self.add_symbol(target, name, entity, kind, r, name_token);
        }
        // A member declared in a data or trait body.
        // @lfy def/model/main.lfy:bind
        if let Some(current) = trees.member_declaration(r) {
            let (_, name_token) = trees.accessor_and_name(current).expect("checked");
            let name_token = name_token.expect("checked");
            let name = trees.token(r.file, name_token).value.clone();
            let owner_scope = self.model.scopes[enclosing].parent.unwrap_or(enclosing);
            let entity = self.new_entity(EntityKind::Member, Some(r), Some(name.clone()));
            if let Some(symbol) = self.add_symbol(
                owner_scope,
                name,
                entity,
                SymbolKind::Member,
                r,
                Some(name_token),
            ) {
                self.model.symbol_by_node.insert(current, symbol);
            }
        }
        // An ObjectKey in the Object of an EnumDeclaration declares an enum member.
        // @lfy def/model/main.lfy:bind
        if trees.is(r, E::ObjectKey)
            && trees.parent(r).is_some_and(|p| trees.is(p, E::Object))
            && trees
                .parent(r)
                .and_then(|p| trees.parent(p))
                .is_some_and(|g| trees.is(g, S::EnumDeclaration))
            && let Some(declared) = trees.child(r, E::Declared)
            && let Some(token) = trees.child_token(
                declared,
                crate::grammar::terminals::identifier::Identifier::Identifier,
            )
        {
            let name = trees.token(r.file, token).value.clone();
            let entity = self.new_entity(EntityKind::EnumMember, Some(r), Some(name.clone()));
            self.add_symbol(
                enclosing,
                name,
                entity,
                SymbolKind::EnumMember,
                r,
                Some(token),
            );
        }
        for child in trees.children(r) {
            self.declare_node(child, scope);
        }
    }

    /// A Use refers to the Source its entry in Source.uses names.
    // @lfy def/model/main.lfy:bind
    pub fn link_uses(&mut self, file: FileId) {
        let trees = self.trees.clone();
        let _ = file;
        // Every Use of the file in preorder, as the workspace lists Source.uses.
        let uses: Vec<NodeRef> = (0..trees.nodes[file].len())
            .map(|index| NodeRef { file, index })
            .filter(|&r| trees.is(r, S::Use))
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
                // @lfy def/model/main.lfy:bind
                if let Some(symbol) = self.model.symbol_of(use_node) {
                    let file_entity = self.model.file_entities[target];
                    self.model.symbols[symbol].entity = file_entity;
                }
            } else {
                // The used file's own symbols, not its imports, are visible.
                // @lfy def/model/main.lfy:bind
                let own: Vec<SymbolId> = self.model.scopes[self.model.file_scopes[target]]
                    .symbols
                    .clone();
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

    /// The symbol a Name node names, looked up statically.
    pub fn resolve_name_node(&self, node: NodeRef) -> Option<SymbolId> {
        let trees = &self.trees;
        if trees.is(node, E::Name) {
            let name = trees.name(node)?;
            return self.lookup_at(node, self.model.enclosing_scope(node), &name);
        }
        if trees.is(node, E::Member) {
            let left = trees.left(node)?;
            let (accessor, name) = trees.accessor_and_name(node)?;
            // The value layer reads a member's value; the scope layer the member itself.
            // Both name the same symbol.
            // @lfy def/model/main.lfy:bind
            if !trees.token_is(node.file, accessor, P::ValueAccessor)
                && !trees.token_is(node.file, accessor, P::ScopeAccessor)
            {
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

    // ---- Kinds and base data ------------------------------------------------------

    /// The data the prelude declares under this name, when the program has a prelude.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn prelude_data(&self, name: &str) -> Option<EntityId> {
        let prelude = self.prelude?;
        let symbol = self.model.lookup_local(prelude, name)?;
        let entity = self.model.symbols[symbol].entity;
        matches!(self.model.entities[entity].kind, EntityKind::Data).then_some(entity)
    }

    /// The kind data of an entity: the prelude data for what it is, then `Entity`, which
    /// every entity is seen through. A kind data extends `Entity`, so it holds those
    /// members too; `Entity` is here for a program whose prelude declares only it.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn kind_data(&self, entity: EntityId) -> Vec<EntityId> {
        let own = match self.model.entities[entity].kind {
            EntityKind::Trait { .. } => Some("Trait"), // @lfy def/model/main.lfy:bind
            EntityKind::Fn { .. } => Some("Function"), // @lfy def/model/main.lfy:bind
            EntityKind::Data => Some("Data"),          // @lfy def/model/main.lfy:bind
            EntityKind::Type => Some("Type"),          // @lfy def/model/main.lfy:bind
            EntityKind::Enum => Some("Enum"),          // @lfy def/model/main.lfy:bind
            EntityKind::Member => Some("Member"),      // @lfy def/model/main.lfy:bind
            EntityKind::Parameter => Some("Parameter"), // @lfy def/model/main.lfy:bind
            EntityKind::Variable => Some("Variable"),  // @lfy def/model/main.lfy:bind
            EntityKind::Module | EntityKind::File => Some("Module"), // @lfy def/model/main.lfy:bind
            _ => None,
        };
        own.and_then(|name| self.prelude_data(name))
            .into_iter()
            .chain(self.prelude_data("Entity")) // @lfy def/model/main.lfy:bind
            .collect()
    }

    /// The member of an entity's kind data with this name.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn kind_member(&self, entity: EntityId, name: &str) -> Option<SymbolId> {
        self.kind_data(entity)
            .into_iter()
            .find_map(|data| self.member_symbol(data, name))
    }

    /// Whether the name is a member of any kind data: one of another kind yields
    /// undefined for this entity, without a problem.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn kind_member_anywhere(&self, name: &str) -> bool {
        KIND_DATA.iter().any(|kind| {
            self.prelude_data(kind)
                .and_then(|data| self.member_symbol(data, name))
                .is_some()
        })
    }

    /// The member of an entity's kind data whose value is a function: what the value
    /// layer of an entity adds to its own members.
    // @lfy def/model/main.lfy:bind
    fn kind_function_member(&self, entity: EntityId, name: &str) -> Option<SymbolId> {
        let symbol = self.kind_member(entity, name)?;
        let member = self.model.symbols[symbol].entity;
        let ty = self.model.entities[member].ty.as_ref()?;
        self.is_function_value(ty).then_some(symbol)
    }

    /// Whether what a member is written as is a function: an inline function, or a
    /// reference to a fn or function declaration, as `Trait.apply` is. Both read as a
    /// function type, and an inline function that owns a fn entity as that entity.
    // @lfy def/model/main.lfy:bind
    fn is_function_value(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Function => true,
            TypeRef::Entity(entity) => {
                matches!(self.model.entities[*entity].kind, EntityKind::Fn { .. })
            }
            _ => false,
        }
    }

    /// The base data of a value of this type: what the prelude declares for the kind of
    /// value it is. A template's type is that data already, so it arrives here as an
    /// entity and needs no entry.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn base_data(&self, ty: &TypeRef) -> Option<EntityId> {
        let name = match ty {
            TypeRef::Primitive("string") => "String", // @lfy def/model/main.lfy:bind
            TypeRef::Primitive("number") => "Number", // @lfy def/model/main.lfy:bind
            TypeRef::Primitive("boolean") => "Boolean", // @lfy def/model/main.lfy:bind
            TypeRef::Primitive("object") => "Object", // @lfy def/model/main.lfy:bind
            TypeRef::Primitive("function") | TypeRef::Function => "Function", // @lfy def/model/main.lfy:bind
            TypeRef::List(_) => "List", // @lfy def/model/main.lfy:bind
            TypeRef::Literal(value) => match **value {
                Value::String(_) => "String",
                Value::Number(_) => "Number",
                Value::Bool(_) => "Boolean",
                _ => return None,
            },
            _ => return None,
        };
        self.prelude_data(name)
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

    /// The trait symbol a TraitUse names, statically.
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
    /// template, a name found nowhere resolves to the one rule entity with that name.
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
        if symbol.is_none() && self.in_template(r) {
            symbol = self
                .model
                .rule_entity(&name)
                .and_then(|e| self.model.entities[e].symbol);
        }
        if symbol.is_none() {
            self.problem(r, format!("{name} is not declared in scope"));
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
        // The Current that declares a member is the declaration, not a usage.
        if self.model.symbol_of(r).is_some() {
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
        // The add of `@acceptanceCriteria.add(...)` is the binder's own call, not a
        // member of the list it reads: it has no symbol and adds no problem.
        // @lfy def/model/main.lfy:bind
        let target = if layer == Layer::Value && self.is_criteria_add(r, name_text.as_deref()) {
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

    /// Whether a member named `add` is read with the value accessor on the criteria of
    /// an entity's context: `@acceptanceCriteria.add(...)`, or the `.add` of the chain
    /// that follows it.
    // @lfy def/model/main.lfy:bind
    fn is_criteria_add(&self, r: NodeRef, name: Option<&str>) -> bool {
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
                return trees.token(node.file, spelled).value == "acceptanceCriteria";
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
                // For a value of a base data, that data's members. An object's keys come
                // first and are not known here, so a name the data does not declare is
                // left to the object.
                // @lfy def/model/main.lfy:bind
                // @lfy def/model/main.lfy:bind
                Target::Base(data) => {
                    let symbol = self.member_symbol(data, name);
                    let object = self.prelude_data("Object") == Some(data);
                    if symbol.is_none() && !object {
                        let what = self.model.entities[data]
                            .identifier
                            .clone()
                            .unwrap_or_else(|| "the value".to_string());
                        self.problem(r, format!("{what} has no member {name}"));
                    }
                    symbol
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
    /// problem is added, whatever the left side is, because being lenient here would hide
    /// a misspelled member behind a silent undefined. The one exception is the trait of a
    /// program with no prelude: a trait is the entity whose value layer is all kind data,
    /// so with none of it a name only the prelude would give is found nowhere and nothing
    /// was misspelled.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    fn no_member(&mut self, r: NodeRef, entity: EntityId, name: &str, current: bool) {
        if matches!(self.model.entities[entity].kind, EntityKind::Trait { .. })
            && self.kind_data(entity).is_empty()
        {
            return;
        }
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
            return self.target_of_type(ty);
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
                Some(file)
                    if matches!(self.model.entities[symbol.entity].kind, EntityKind::File) =>
                {
                    Target::Module(file)
                }
                _ => Target::Unknown,
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
                    _ => Target::Unknown,
                }
            }
            _ => match self.model.entities[symbol.entity].ty.clone() {
                Some(ty) => self.target_of_type(ty),
                None => Target::Unknown,
            },
        }
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
                // A union of entities: the first entity, leniently.
                for item in items {
                    if let TypeRef::Entity(e) = item {
                        return Target::Predicate(e);
                    }
                }
                Target::Unknown
            }
            // A value of any other base data.
            // @lfy def/model/main.lfy:bind
            other => self.base_data(&other).map_or(Target::Unknown, Target::Base),
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

/// What a declaration is generic over, and what a use of it was written with. `Entity` is
/// a record of the model and the prelude declares both as members of it, so the model
/// answers them rather than holding a field for each.
impl Model {
    /// `Entity.typeParameters`: the entity of each `TypeParameter` in the declaration's
    /// `TypeParameters`, in order; empty for a declaration without them. A declaration
    /// seen with arguments owns the declaration's scope, so its type parameters are the
    /// declaration's.
    // @lfy def/model/main.lfy:bind
    pub fn type_parameters(&self, entity: EntityId) -> Vec<EntityId> {
        match self.entities[entity].scope {
            Some(scope) => self.scopes[scope]
                .symbols
                .iter()
                .filter(|&&symbol| self.symbols[symbol].kind == SymbolKind::TypeParameter)
                .map(|&symbol| self.symbols[symbol].entity)
                .collect(),
            None => Vec::new(),
        }
    }

    /// `Entity.typeArguments`: what each argument the type was written with resolves to,
    /// in order; empty for a declaration used without arguments.
    // @lfy def/model/main.lfy:bind
    pub fn type_arguments(&self, entity: EntityId) -> Vec<TypeRef> {
        match self.entities[entity].value(TYPE_ARGUMENTS) {
            Some(Value::List(items)) => items
                .iter()
                .filter_map(|item| match item {
                    Value::Type(ty) => Some((**ty).clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// A declaration seen with arguments takes the declaration's definition, members,
    /// criteria, and traits. The type pass makes the entity, but the apply pass fills the
    /// declaration in after it, so what it takes is copied once binding is done.
    // @lfy def/model/main.lfy:bind
    pub(crate) fn finish_generics(&mut self) {
        for entity in 0..self.entities.len() {
            let base = self.generic_base(entity);
            if base == entity {
                continue;
            }
            self.entities[entity].definition = self.entities[base].definition.clone();
            self.entities[entity].definition_node = self.entities[base].definition_node;
            self.entities[entity].acceptance_criteria =
                self.entities[base].acceptance_criteria.clone();
            self.entities[entity].traits = self.entities[base].traits.clone();
            self.entities[entity].scope = self.entities[base].scope;
        }
    }

    /// The declaration a type seen with arguments stands for; the entity itself for
    /// anything else.
    // @lfy def/model/main.lfy:bind
    pub fn generic_base(&self, entity: EntityId) -> EntityId {
        match self.entities[entity].ty {
            Some(TypeRef::Entity(base))
                if self.entities[entity].value(TYPE_ARGUMENTS).is_some() =>
            {
                base
            }
            _ => entity,
        }
    }
}

fn entity_kind(kind: SymbolKind) -> EntityKind {
    match kind {
        SymbolKind::Data => EntityKind::Data,
        SymbolKind::Trait => EntityKind::Trait {
            parameters: Vec::new(),
            entities: Vec::new(),
            extenders: Vec::new(),
        },
        SymbolKind::Function => EntityKind::Fn {
            parameters: Vec::new(),
            output: None,
            agent: false,
        },
        SymbolKind::AgentFunction => EntityKind::Fn {
            parameters: Vec::new(),
            output: None,
            agent: true,
        },
        SymbolKind::Type => EntityKind::Type,
        SymbolKind::Enum => EntityKind::Enum,
        SymbolKind::Variable => EntityKind::Variable,
        SymbolKind::Alias => EntityKind::Alias,
        SymbolKind::External => EntityKind::External,
        SymbolKind::Module => EntityKind::Module,
        SymbolKind::LoopVariable => EntityKind::LoopVariable,
        // A type parameter is a parameter of the declaration it stands in, so it is one
        // entity kind with the value parameters; its symbol keeps the two apart.
        SymbolKind::Parameter | SymbolKind::TypeParameter => EntityKind::Parameter,
        SymbolKind::Member => EntityKind::Member,
        SymbolKind::EnumMember => EntityKind::EnumMember,
    }
}
