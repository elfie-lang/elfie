// @lfy def/interpret/main.lfy:expand
//! The interpreter itself, and what it reads of the model to run a body.

use super::*;

/// The prelude scope, when a source has [`Origin::Prelude`].
// @lfy def/interpret/main.lfy:expand
fn prelude_scope(model: &Model) -> Option<ScopeId> {
    let file = model
        .sources
        .iter()
        .position(|source| source.origin == Origin::Prelude)?;
    model.file_scopes.get(file).copied()
}

impl<'m> Interpreter<'m> {
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn new(model: &'m mut Model) -> Interpreter<'m> {
        let universe = universe(model);
        let prelude = prelude_scope(model);
        Interpreter {
            model,
            cache: HashMap::new(),
            taking: Vec::new(),
            reported: HashSet::new(),
            universe,
            prelude,
            runtime: false,
            runs_functions: true,
            writes: true,
        }
    }

    // @lfy def/interpret/main.lfy:expand
    pub(super) fn problem(&mut self, node: NodeRef, message: impl Into<String>) {
        self.model.problems.push(Problem {
            node,
            message: message.into(),
            stage: crate::model::Stage::Binder,
        });
    }

    /// The environment the top level of a file runs in.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn file_env(&self, file: FileId) -> Env {
        let entity = self.model.file_entities[file];
        Env {
            vars: Vec::new(),
            current: entity,
            target: entity,
            contributor: entity,
            scope: self.model.file_scopes[file],
            in_trait: false,
            dry: false,
            ace: false,
            ret: None,
        }
    }

    /// The data the prelude declares under this name, when the program has a prelude.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn prelude_data(&self, name: &str) -> Option<EntityId> {
        let prelude = self.prelude?;
        let symbol = self.model.lookup_local(prelude, name)?;
        let entity = self.model.symbols[symbol].entity;
        matches!(self.model.entities[entity].kind, EntityKind::Data).then_some(entity)
    }

    /// The kind data of an entity: the prelude data for what it is, then `Entity`, which
    /// every entity is seen through.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn kind_data(&self, entity: EntityId) -> Vec<EntityId> {
        let own = match self.model.entities[entity].kind {
            EntityKind::Trait { .. } => Some("Trait"),
            EntityKind::Fn { .. } => Some("Function"),
            EntityKind::Data => Some("Data"),
            EntityKind::Type => Some("Type"),
            EntityKind::Enum => Some("Enum"),
            EntityKind::Member => Some("Member"),
            EntityKind::Parameter => Some("Parameter"),
            EntityKind::Variable => Some("Variable"),
            EntityKind::Module | EntityKind::File => Some("Module"),
            _ => None,
        };
        own.and_then(|name| self.prelude_data(name))
            .into_iter()
            .chain(self.prelude_data("Entity"))
            .collect()
    }

    /// The member of an entity's kind data with this name.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn kind_member(&self, entity: EntityId, name: &str) -> Option<SymbolId> {
        self.kind_data(entity)
            .into_iter()
            .find_map(|data| member_symbol(self.model, data, name))
    }

    /// Whether the name is a member of any kind data: one of another kind yields undefined
    /// for this entity, without a problem.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn kind_member_anywhere(&self, name: &str) -> bool {
        [
            "Entity", "Trait", "Function", "Data", "Type", "Enum", "Member", "Parameter",
            "Variable", "Module",
        ]
        .iter()
        .any(|kind| {
            self.prelude_data(kind)
                .and_then(|data| member_symbol(self.model, data, name))
                .is_some()
        })
    }

    /// The data a value of this type is read through: the declaration it names, or what the
    /// prelude declares for the kind of value it is.
    // @lfy def/interpret/main.lfy:evaluate
    pub(super) fn base_data(&self, ty: &TypeRef) -> Option<EntityId> {
        let name = match ty {
            TypeRef::Entity(entity) => return Some(*entity),
            TypeRef::Predicate(entity) => return Some(*entity),
            TypeRef::Primitive("string") => "String",
            TypeRef::Primitive("number") => "Number",
            TypeRef::Primitive("boolean") => "Boolean",
            TypeRef::Primitive("object") => "Object",
            TypeRef::Primitive("function") | TypeRef::Function => "Function",
            TypeRef::List(_) => "List",
            TypeRef::Literal(value) => match **value {
                Interim::String(_) => "String",
                Interim::Number(_) => "Number",
                Interim::Bool(_) => "Boolean",
                _ => return None,
            },
            _ => return None,
        };
        self.prelude_data(name)
    }

    /// Whether a method on a value is an operation of a data carrying [`BUILTIN`], which
    /// the interpreter performs itself rather than calling anything: the dispatch is on the
    /// trait, never on the name alone. A program that declares no data for the value, or no
    /// `builtin` at all, says nothing against it.
    // @lfy def/interpret/main.lfy:expand#expand:expand:d8317dcbe9cf6885feca31d997bb0425e00cd18a2059d87291a8d6105dc664ef
    pub(super) fn performs_operation(&self, receiver: &Interim, method: &str) -> bool {
        let ty = match receiver {
            Interim::String(_) => TypeRef::Primitive("string"),
            Interim::Number(_) => TypeRef::Primitive("number"),
            Interim::Bool(_) => TypeRef::Primitive("boolean"),
            Interim::List(_) => TypeRef::List(Box::new(TypeRef::Unknown(String::new()))),
            _ => return true,
        };
        let Some(data) = self.base_data(&ty) else {
            return true;
        };
        self.carries_builtin(data) && member_symbol(self.model, data, method).is_some()
    }

    /// Whether a data carries [`BUILTIN`], or its declaration names it: the `is` clause of
    /// a library data has not always run when a body of another file reaches one of its
    /// operations, and the trait it names is what decides.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn carries_builtin(&self, data: EntityId) -> bool {
        let Some(marker) = self.model.trait_named(BUILTIN) else {
            return true;
        };
        if self.model.entities[data].has_trait(marker) {
            return true;
        }
        let Some(node) = self.model.entities[data].node else {
            return false;
        };
        let holder = self.model.child(node, E::Declared).unwrap_or(node);
        let Some(clause) = self.model.child(holder, E::IsClause) else {
            return false;
        };
        self.model
            .descendants(clause)
            .into_iter()
            .filter(|&r| self.model.is(r, E::TraitUse))
            .any(|used| {
                resolve_trait_use(self.model, used)
                    .is_some_and(|symbol| self.model.symbols[symbol].entity == marker)
            })
    }

    /// The type a node written in a type position stands for.
    ///
    /// Decision: the expand pass runs after the binder's type pass, so every declared type
    /// is already on its entity; a type written inside a body is carried as a primitive, as
    /// the declaration a name resolves to, or as the source text it was written as, which
    /// is what a template or a criterion renders it to. Nothing in a compile-time body
    /// reads more of one.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn type_of(&self, node: NodeRef) -> TypeRef {
        let model = &*self.model;
        if model.is(node, E::PrimitiveType) {
            let text = model.raw(node).trim().to_string();
            return match text.as_str() {
                "string" => TypeRef::Primitive("string"),
                "number" => TypeRef::Primitive("number"),
                "boolean" => TypeRef::Primitive("boolean"),
                "object" => TypeRef::Primitive("object"),
                "function" => TypeRef::Primitive("function"),
                "trait" => TypeRef::Primitive("trait"),
                _ => TypeRef::Unknown(text),
            };
        }
        if model.is(node, E::Name)
            && let Some(symbol) = resolve_name_node(model, node)
        {
            return TypeRef::Entity(model.symbols[symbol].entity);
        }
        if model.is(node, E::TypeExpression) || model.is(node, E::TypeGroup) {
            let items: Vec<TypeRef> = model
                .child_nodes(node)
                .into_iter()
                .map(|item| self.type_of(item))
                .collect();
            return match items.len() {
                0 => TypeRef::Unknown(model.raw(node)),
                1 => items.into_iter().next().expect("one item"),
                _ => TypeRef::Union(items),
            };
        }
        if model.is(node, E::TypePredicate)
            && let Some(inner) = model.child_nodes(node).into_iter().next()
            && let Some(symbol) = resolve_name_node(model, inner)
        {
            return TypeRef::Predicate(model.symbols[symbol].entity);
        }
        // `T[]` in a type position: an `Index` with nothing inside it.
        if model.is(node, E::Index)
            && let Some(left) = model.left(node)
            && model.child_nodes(node).len() == 1
        {
            return TypeRef::List(Box::new(self.type_of(left)));
        }
        TypeRef::Unknown(model.raw(node))
    }
}
