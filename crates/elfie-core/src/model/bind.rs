//! Compiled from `def/model/main.lfy` and `def/model/traits.lfy`: the declare, type, and
//! resolve passes of binding. The expand pass is the interpreter's, which `bind` calls in
//! place of a pass of its own; `eval.rs` runs what is left of a trait or declaration body
//! for the resolve pass.

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

mod declare;
mod resolve;
mod types;

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
            knowledge: Vec::new(),
            commands: Vec::new(),
            targets: Vec::new(),
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
            stage: Stage::Binder,
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
            knowledge: Vec::new(),
            commands: Vec::new(),
            targets: Vec::new(),
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

    /// One usage of the layer that [`accessor_layer`] or `reading_layer` gives the node's
    /// rule or accessor token.
    // @lfy def/model/data.lfy:Usage#Usage:accessor:a83a1a9d1f66f716f26eb38316fc83472bf8306d7ed085dbbb651ca800d7fa1d
    // @lfy def/model/data.lfy:Usage#Usage:accessor:2ce54c1898fc13d3cd8905cc73e0389c8d9fa45563717e429563f4262bcf67cc
    // @lfy def/model/data.lfy:Usage#Usage:accessor:c402a1e08ae87d1f6c648beb2924eaef933acccda58449b98b30527d4a8927f4
    // @lfy def/model/data.lfy:Usage#Usage:accessor:720ba39df2e40462f81cd49bc06aac603c4e40c7e5ad8394cff5c8576a3e835a
    // @lfy def/model/data.lfy:Usage#Usage:accessor:69e04382078756444526894120573aaa0afb9ee9f8977f2be40edeb331977270
    // @lfy def/model/data.lfy:Usage#Usage:accessor:d3767c249789fc2ccbc9da146aeda08cf7c2c77d178ddb3c396ffa1d4fe28c60
    // @lfy def/model/data.lfy:Usage#Usage:accessor:676186f904e52a43b4616d6a74db5e18cf8c8db3089fd5f403e6ba683912d591
    // @lfy def/model/data.lfy:Usage#Usage:accessor:4cb28a1b2479e9a7f95801092e1d411a5b47236dbe7ce3e234c2e753d1155dde
    // @lfy def/model/data.lfy:Usage#Usage:accessor:7ad232beed0bbccd19e09da4145c76e7041be8d3ebc5938738a7560d13d2f100
    // @lfy def/model/data.lfy:Usage#Usage:accessor:ac21cace34f24707815accc4133284f676e8136d3c238c0a1a83cc287fbce07f
    // @lfy def/model/data.lfy:Usage#Usage:accessor:f9666e2c42b94cd9fb83e979bd5504fef2c78d0ac26ec89b455289555cd1c939
    // @lfy def/model/data.lfy:Usage#Usage:accessor:609073365fe6e5381452fd7f8b3d2578555d722eae1cb99cc00895d10ea82ad5
    // @lfy def/model/data.lfy:Usage#Usage:accessor:7dc5ef0b5fa91500246e001f8c08cad34455ae1b67d54b8bbe36f2a891d475bc
    // @lfy def/model/data.lfy:Usage#Usage:accessor:ee33146dc6b219be01564883208e84a9994b9304703fea90f2830a695fc6d751
    // @lfy def/model/data.lfy:Usage#Usage:accessor:6e6f5d3a14fd1410aa67d98ed94a74b127e1f3e1bfbf3b6677c6b61cf5c667a4
    // @lfy def/model/data.lfy:Usage#Usage:accessor:821cf3e93321780941fd9240917de8df4e995d1962a2a9508c03aeb6cb4415c5
    // @lfy def/model/data.lfy:Usage#Usage:accessor:d69e7051ef45630f2599b70224f5da10687332ce1d6c5404a61183128f1cb965
    // @lfy def/model/data.lfy:Usage#Usage:accessor:438adfdcbc75848d58d7798bda90d7a5ced341740dc5b5e1c043e025e8be69fc
    pub fn add_usage(
        &mut self,
        node: NodeRef,
        name: Option<String>,
        token: Option<usize>,
        layer: Layer,
        symbol: Option<SymbolId>,
    ) -> UsageId {
        // `name` is None when the node spells none after its accessor, so the usage reads
        // the layer itself.
        // @lfy def/model/data.lfy:Usage#Usage:accessor:81661e9119e520f274ca72124f9c32bd034bc44cea0c79075c2b24353e1dcb81
        // @lfy def/model/data.lfy:Usage#Usage:accessor:ca0ac41aaccdce37b39bf6e178cb927fe3448c5fceeed3d5c0bb6c42d40d9950
        // @lfy def/model/data.lfy:Usage#Usage:accessor:6e9eedf28265239d3e7be901ea8aaf2f0212f1aa5a38e146b0bb2872ace2a7b3
        // @lfy def/model/data.lfy:Usage#Usage:accessor:43e1913f1171ad26b1ec2dd0debdf4aec74488e056cecabf1c8e8408302ea016
        // @lfy def/model/data.lfy:Usage#Usage:accessor:d8ee27ca3a13f81dce5d003a9b9e2d1f1c36a805d87bde90d13e75d5eeabf917
        // @lfy def/model/data.lfy:Usage#Usage:accessor:527271683eed884372fc741a7365ee1bd48fc1c9481481205e64b7d6be1b0908
        // @lfy def/model/data.lfy:Usage#Usage:accessor:db87cd548192124de3f252e1e3782251a86c47d584602ec523b119c218ffeabc
        // @lfy def/model/data.lfy:Usage#Usage:accessor:3a3ad10ddab6819fee95779cab81849541b1731648bb175a046fbe19a1fc6650
        // @lfy def/model/data.lfy:Usage#Usage:accessor:1fecb2c80fca30a6b0043a803a889d8cdf6e268f8a91b51ef90ff13b1127a1cb
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

    // ---- Lookup -------------------------------------------------------------------

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
}

/// What a declaration is generic over, and what a use of it was written with. `Entity` is
/// a record of the model and the prelude declares both as members of it, so the model
/// answers them rather than holding a field for each.
impl Model {
    /// `Entity.typeParameters`: the entity of each `TypeParameter` in the declaration's
    /// `TypeParameters`, in order; empty for a declaration without them. A declaration
    /// seen with arguments owns the declaration's scope, so its type parameters are the
    /// declaration's.
    // @lfy def/model/data.lfy:Entity.typeParameters
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
    // @lfy def/model/data.lfy:Entity.typeArguments
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
    /// criteria, and traits. The type pass makes the entity, but the expand pass fills the
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
