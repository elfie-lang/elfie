//! Compiled from `def/model/data.lfy`: the data of the bound program.
//!
//! Every record lives in one of the arenas of [`Model`] and is referred to by index, so
//! the whole model is one plain value that can be cloned, sent, and compared.

use std::collections::HashMap;
use std::fmt;

use crate::grammar::Entity as Rule;
use crate::lexer::Token;
use crate::parser::data::{Child, Node, Tree};

/// Index of a [`Source`] in [`Model::sources`].
pub type FileId = usize;
/// Index of a [`Scope`] in [`Model::scopes`].
pub type ScopeId = usize;
/// Index of a [`Symbol`] in [`Model::symbols`].
pub type SymbolId = usize;
/// Index of an [`Entity`] in [`Model::entities`].
pub type EntityId = usize;
/// Index of a [`Usage`] in [`Model::usages`].
pub type UsageId = usize;

/// The three layers of every entity, plus the two ways of pointing at an entity itself.
// @lfy def/model/data.lfy:Layer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Value,       // @lfy def/model/data.lfy:Layer.value
    Context,     // @lfy def/model/data.lfy:Layer.context
    Scope,       // @lfy def/model/data.lfy:Layer.scope
    Parent,      // @lfy def/model/data.lfy:Layer.parent
    Dereference, // @lfy def/model/data.lfy:Layer.dereference
    Previous,    // @lfy def/model/data.lfy:Layer.previous
}

impl Layer {
    pub fn value(self) -> &'static str {
        match self {
            Layer::Value => "value",
            Layer::Context => "context",
            Layer::Scope => "scope",
            Layer::Parent => "parent scope",
            Layer::Dereference => "the entity a name is bound to",
            Layer::Previous => "the entity the previous statement declared",
        }
    }
}

/// Every context property; a name after the context accessor must be one of these. A
/// property is matched by its value, not its member name.
// @lfy def/model/data.lfy:ContextProperty
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextProperty {
    Identifier,         // @lfy def/model/data.lfy:ContextProperty.identifier
    Definition,         // @lfy def/model/data.lfy:ContextProperty.definition
    Type,               // @lfy def/model/data.lfy:ContextProperty._type
    AcceptanceCriteria, // @lfy def/model/data.lfy:ContextProperty.acceptanceCriteria
    Parameters,         // @lfy def/model/data.lfy:ContextProperty.parameters
    Output,             // @lfy def/model/data.lfy:ContextProperty.output
    Entities,           // @lfy def/model/data.lfy:ContextProperty.entities
    Extenders,          // @lfy def/model/data.lfy:ContextProperty.extenders
    References,         // @lfy def/model/data.lfy:ContextProperty.references
    Like,               // @lfy def/model/data.lfy:ContextProperty.like
    Test,               // @lfy def/model/data.lfy:ContextProperty.test
    Tests,              // @lfy def/model/data.lfy:ContextProperty.tests
    ItemType,           // @lfy def/model/data.lfy:ContextProperty.itemType
}

impl ContextProperty {
    pub const ALL: [ContextProperty; 13] = [
        ContextProperty::Identifier,
        ContextProperty::Definition,
        ContextProperty::Type,
        ContextProperty::AcceptanceCriteria,
        ContextProperty::Parameters,
        ContextProperty::Output,
        ContextProperty::Entities,
        ContextProperty::Extenders,
        ContextProperty::References,
        ContextProperty::Like,
        ContextProperty::Test,
        ContextProperty::Tests,
        ContextProperty::ItemType,
    ];

    /// The value of the enum member: what is spelled after `@`.
    pub fn value(self) -> &'static str {
        match self {
            ContextProperty::Identifier => "identifier",
            ContextProperty::Definition => "definition",
            ContextProperty::Type => "type",
            ContextProperty::AcceptanceCriteria => "acceptanceCriteria",
            ContextProperty::Parameters => "parameters",
            ContextProperty::Output => "output",
            ContextProperty::Entities => "entities",
            ContextProperty::Extenders => "extenders",
            ContextProperty::References => "references",
            ContextProperty::Like => "like",
            ContextProperty::Test => "test",
            ContextProperty::Tests => "tests",
            ContextProperty::ItemType => "itemType",
        }
    }

    /// The property whose value is `name`.
    pub fn lookup(name: &str) -> Option<ContextProperty> {
        ContextProperty::ALL
            .into_iter()
            .find(|property| property.value() == name)
    }
}

/// Identity of one node of one file's tree: the file and the node's position in a
/// preorder walk of the tree, which [`Model::node`] turns back into the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeRef {
    pub file: FileId,
    pub index: usize,
}

/// What the model records about one node so it can be found again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    pub rule: Rule,
    pub start: usize,
    pub end: usize,
    /// The preorder index of the parent node; `None` for the root.
    pub parent: Option<usize>,
    /// The position among the parent's children, from the root down.
    pub path: Vec<u32>,
}

/// What kind of declaration a symbol came from.
// @lfy def/model/data.lfy:SymbolKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Data,          // @lfy def/model/data.lfy:SymbolKind.data
    Trait,         // @lfy def/model/data.lfy:SymbolKind._trait
    Type,          // @lfy def/model/data.lfy:SymbolKind._type
    Enum,          // @lfy def/model/data.lfy:SymbolKind._enum
    EnumMember,    // @lfy def/model/data.lfy:SymbolKind.enumMember
    Function,      // @lfy def/model/data.lfy:SymbolKind._function
    AgentFunction, // @lfy def/model/data.lfy:SymbolKind.agentFunction
    Variable,      // @lfy def/model/data.lfy:SymbolKind.variable
    LoopVariable,  // @lfy def/model/data.lfy:SymbolKind.loopVariable
    Parameter,     // @lfy def/model/data.lfy:SymbolKind.parameter
    Member,        // @lfy def/model/data.lfy:SymbolKind.member
    Alias,         // @lfy def/model/data.lfy:SymbolKind._alias
    External,      // @lfy def/model/data.lfy:SymbolKind._external
    Module,        // @lfy def/model/data.lfy:SymbolKind._module
}

impl SymbolKind {
    /// The value of the enum member: how the kind is spelled.
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Data => "data",
            SymbolKind::Trait => "trait",
            SymbolKind::Type => "type",
            SymbolKind::Enum => "enum",
            SymbolKind::EnumMember => "enumMember",
            SymbolKind::Function => "function",
            SymbolKind::AgentFunction => "agentFunction",
            SymbolKind::Variable => "variable",
            SymbolKind::LoopVariable => "loopVariable",
            SymbolKind::Parameter => "parameter",
            SymbolKind::Member => "member",
            SymbolKind::Alias => "alias",
            SymbolKind::External => "external",
            SymbolKind::Module => "module",
        }
    }
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A name bound to an entity in one scope. Knows the node that declared it, the scope
/// that holds it, and its [`SymbolKind`].
// @lfy def/model/data.lfy:Symbol
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The name.
    pub name: String, // @lfy def/model/data.lfy:Symbol.name
    /// What the name is bound to; for an alias, the target's entity.
    pub entity: EntityId, // @lfy def/model/data.lfy:Symbol.entity
    pub kind: SymbolKind, // @lfy def/model/data.lfy:Symbol
    /// The node that declared it.
    pub node: NodeRef, // @lfy def/model/data.lfy:Symbol
    /// The token that spells the name, when one does.
    pub name_token: Option<usize>,
    /// The scope that holds it.
    pub scope: ScopeId, // @lfy def/model/data.lfy:Symbol
}

/// A region in which names resolve. Holds the symbols declared directly in it, in order,
/// and knows its parent; a file scope has no parent and also holds the symbols its use
/// statements import.
// @lfy def/model/data.lfy:Scope
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub parent: Option<ScopeId>, // @lfy def/model/data.lfy:Scope
    /// The node that owns it.
    pub owner: NodeRef,
    pub file: FileId,
    /// The symbols declared directly in it, in order.
    pub symbols: Vec<SymbolId>, // @lfy def/model/data.lfy:Scope
    /// The symbols its use statements import; only a file scope has any.
    pub imports: Vec<SymbolId>, // @lfy def/model/data.lfy:Scope
    /// The current entity: what the value, context, and scope accessors reach without a
    /// left expression.
    pub current: EntityId, // @lfy def/model/data.lfy:Scope
}

/// What applied a trait: the clause or call, or the application it was inherited through.
// @lfy def/model/data.lfy:Applied
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppliedSource {
    Is(NodeRef),
    Extends(NodeRef),
    Apply(NodeRef),
    /// Inherited through the application at this index of the same entity's traits.
    Inherited(usize),
}

/// One trait on one entity.
// @lfy def/model/data.lfy:Applied
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The trait.
    pub entity: EntityId, // @lfy def/model/data.lfy:Applied.entity
    /// Argument nodes, in order.
    pub arguments: Vec<NodeRef>, // @lfy def/model/data.lfy:Applied.arguments
    /// The arguments evaluated where they were written, in order; a spread parameter
    /// takes the rest as one list.
    pub values: Vec<Value>,
    pub source: AppliedSource, // @lfy def/model/data.lfy:Applied
}

/// One acceptance criterion as written, with the entity it came from. Texts are stored
/// resolved for the entity that holds the criterion.
// @lfy def/model/data.lfy:Criterion
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Criterion {
    pub situation: Option<Vec<String>>, // @lfy def/model/data.lfy:Criterion.situation
    pub behavior: Option<Vec<String>>,  // @lfy def/model/data.lfy:Criterion.behavior
    pub side_effects: Option<Vec<String>>, // @lfy def/model/data.lfy:Criterion.sideEffects
    /// The entity whose body holds it: the entity itself or one of its traits.
    pub contributor: EntityId, // @lfy def/model/data.lfy:Criterion.contributor
    /// The `add` call or `Where` statement it was written as.
    pub node: Option<NodeRef>,
}

/// One test.
// @lfy def/model/data.lfy:Test
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Test {
    /// Input for the test (parameters for functions, value otherwise).
    pub input: Option<NodeRef>, // @lfy def/model/data.lfy:Test.input
    /// Expected output for the test.
    pub expect: Option<NodeRef>, // @lfy def/model/data.lfy:Test.expect
    /// The source text of the input expression.
    pub input_text: String,
    /// The source text of the expectation.
    pub expect_text: String,
}

/// A type as the model knows it.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeRef {
    /// A data, trait, type, or enum entity, or an alias of one.
    Entity(EntityId),
    /// `string`, `number`, `boolean`, `object`, `function`, or `trait`.
    Primitive(&'static str),
    /// A literal type: a string, template, number, null, or undefined.
    Literal(Box<Value>),
    List(Box<TypeRef>),
    Union(Vec<TypeRef>),
    /// Anything with the trait.
    Predicate(EntityId),
    Function,
    /// A type the model could not read further, as its source text.
    Unknown(String),
}

/// A value of the value layer, as the binder evaluates it at bind time.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    List(Vec<Value>),
    Object(Vec<(String, Value)>),
    Entity(EntityId),
    /// A function: the node of the inline function or declaration, and the scope it was
    /// written in.
    Function(NodeRef, ScopeId),
    Scope(ScopeId),
    Type(Box<TypeRef>),
    /// An inline function with the variables it captured.
    Closure(NodeRef, Vec<(String, Value)>),
    /// `@acceptanceCriteria` of an entity: what `add` is called on.
    Criteria(EntityId),
    /// `@test` of an entity: what is called to add tests.
    Tester(EntityId),
    /// `@like` of a type: a prompt describing an instance.
    Like(Box<Value>),
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

/// What kind of thing an entity is, with the context properties only some kinds have.
#[derive(Debug, Clone, PartialEq)]
pub enum EntityKind {
    /// The anonymous entity of a file.
    File,
    /// The `global` entity.
    Global,
    Data,
    Type,
    Enum,
    /// A declared fn or function seen through its context layer.
    // @lfy def/model/data.lfy:FnEntity
    Fn {
        /// ContextProperty parameters: parameters.
        parameters: Vec<SymbolId>, // @lfy def/model/data.lfy:FnEntity.parameters
        /// ContextProperty output: the declared return type.
        output: Option<TypeRef>, // @lfy def/model/data.lfy:FnEntity.output
        /// Whether the implementation is generated (`fn`) rather than written (`function`).
        agent: bool,
    },
    /// A declared trait seen through its context layer.
    // @lfy def/model/data.lfy:TraitEntity
    Trait {
        parameters: Vec<SymbolId>,
        /// ContextProperty entities: everything the trait was applied to, directly or by
        /// inheritance.
        entities: Vec<EntityId>, // @lfy def/model/data.lfy:TraitEntity.entities
        /// ContextProperty extenders: every trait that names it in an ExtendsClause.
        extenders: Vec<EntityId>, // @lfy def/model/data.lfy:TraitEntity.extenders
    },
    Variable,
    Alias,
    External,
    Module,
    LoopVariable,
    Parameter,
    Member,
    EnumMember,
    /// A scope owner that declares nothing: a block, a with, a loop.
    Anonymous,
}

/// A declared thing seen through its context layer. Knows its declaring node, its
/// symbol, and the scope it owns (parameters, members, body).
// @lfy def/model/data.lfy:Entity
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    /// The declared name; `None` when anonymous.
    pub identifier: Option<String>, // @lfy def/model/data.lfy:TraitEntity.identifier
    /// The description provided for this entity, resolved.
    pub definition: Option<String>, // @lfy def/model/data.lfy:TraitEntity.definition
    /// The node of the string or template the definition was written as.
    pub definition_node: Option<NodeRef>,
    /// The declared or inferred type.
    pub ty: Option<TypeRef>, // @lfy def/model/data.lfy:TraitEntity.type
    /// Acceptance criteria specifying own criteria first, then each trait's in
    /// application order.
    pub acceptance_criteria: Vec<Criterion>, // @lfy def/model/data.lfy:TraitEntity.acceptanceCriteria
    /// Every trait applied to it, direct or inherited, in application order.
    pub traits: Vec<Applied>, // @lfy def/model/data.lfy:TraitEntity.traits
    /// A declared list seen through its context layer: what the list holds, and `None`
    /// when the type is not a list.
    // @lfy def/model/data.lfy:ListEntity
    pub item_type: Option<TypeRef>, // @lfy def/model/data.lfy:ListEntity.itemType
    /// Every entity referenced via a Reference in definition, documentation, or value.
    pub references: Vec<UsageId>, // @lfy def/model/data.lfy:TraitEntity.references
    /// ContextProperty tests: list of all added tests.
    pub tests: Vec<Test>, // @lfy def/model/data.lfy:TraitEntity.tests
    /// The value setters evaluated for it (`.name = value` in a trait or data body).
    pub values: Vec<(String, Value)>,
    pub kind: EntityKind,
    /// The declaring node; `None` for global.
    pub node: Option<NodeRef>, // @lfy def/model/data.lfy:Entity
    pub symbol: Option<SymbolId>, // @lfy def/model/data.lfy:Entity
    /// The scope it owns, when it owns one.
    pub scope: Option<ScopeId>, // @lfy def/model/data.lfy:Entity
    pub file: Option<FileId>,
}

impl Entity {
    pub fn is_trait(&self) -> bool {
        matches!(self.kind, EntityKind::Trait { .. })
    }

    /// `TraitEntity.entities`; empty for anything that is not a trait.
    pub fn entities(&self) -> &[EntityId] {
        match &self.kind {
            EntityKind::Trait { entities, .. } => entities,
            _ => &[],
        }
    }

    /// `TraitEntity.extenders`; empty for anything that is not a trait.
    pub fn extenders(&self) -> &[EntityId] {
        match &self.kind {
            EntityKind::Trait { extenders, .. } => extenders,
            _ => &[],
        }
    }

    /// The parameters of a fn, function, or trait.
    pub fn parameters(&self) -> &[SymbolId] {
        match &self.kind {
            EntityKind::Fn { parameters, .. } | EntityKind::Trait { parameters, .. } => parameters,
            _ => &[],
        }
    }

    /// `FnEntity.output`.
    pub fn output(&self) -> Option<&TypeRef> {
        match &self.kind {
            EntityKind::Fn { output, .. } => output.as_ref(),
            _ => None,
        }
    }

    /// Whether the trait `id` is applied to it, directly or by inheritance.
    pub fn has_trait(&self, id: EntityId) -> bool {
        self.traits.iter().any(|applied| applied.entity == id)
    }

    /// The value a setter gave the name.
    pub fn value(&self, name: &str) -> Option<&Value> {
        self.values
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }
}

/// One use of a name or of an entity.
// @lfy def/model/data.lfy:Usage
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    /// The node that uses it.
    pub node: NodeRef, // @lfy def/model/data.lfy:Usage.node
    /// The name spelled after the accessor; `None` when the usage is of the layer itself.
    pub name: Option<String>, // @lfy def/model/data.lfy:Usage.name
    /// The token that spells the name, or the accessor when there is no name.
    pub token: Option<usize>,
    /// Which layer is read.
    pub layer: Layer, // @lfy def/model/data.lfy:Usage.layer
    /// What it resolved to; `None` when unresolved.
    pub symbol: Option<SymbolId>, // @lfy def/model/data.lfy:Usage.symbol
}

/// Something that did not resolve; binding continues past it.
// @lfy def/model/data.lfy:Problem
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Where.
    pub node: NodeRef, // @lfy def/model/data.lfy:Problem.node
    /// What and why.
    pub message: String, // @lfy def/model/data.lfy:Problem.message
}

/// One file to bind, with its `Use` statements already resolved.
// @lfy def/model/data.lfy:Source
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The file's path, as `Token.file` holds it.
    pub path: String, // @lfy def/model/data.lfy:Source.path
    /// The parse of the file.
    pub tree: Tree, // @lfy def/model/data.lfy:Source.tree
    /// The path each `Use` in the tree refers to, in the order the uses appear; `None`
    /// where it refers to nothing.
    pub uses: Vec<Option<String>>, // @lfy def/model/data.lfy:Source.uses
}

/// The program, queryable. Answers every query of the model module for every file it
/// was bound from and lists every problem in node order.
// @lfy def/model/data.lfy:Model
// @lfy def/model/data.lfy:Model
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
    pub sources: Vec<Source>,
    pub scopes: Vec<Scope>,
    pub symbols: Vec<Symbol>,
    pub entities: Vec<Entity>,
    pub usages: Vec<Usage>,
    /// Every problem in node order.
    pub problems: Vec<Problem>, // @lfy def/model/data.lfy:Model
    /// Per file, one entry per node in preorder.
    pub nodes: Vec<Vec<NodeInfo>>,
    /// The file scope of each file.
    pub file_scopes: Vec<ScopeId>,
    /// The anonymous entity of each file.
    pub file_entities: Vec<EntityId>,
    /// The `global` entity.
    pub global: EntityId,
    pub(crate) usage_by_node: HashMap<NodeRef, UsageId>,
    pub(crate) symbol_by_node: HashMap<NodeRef, SymbolId>,
    pub(crate) scope_by_node: HashMap<NodeRef, ScopeId>,
    pub(crate) node_keys: HashMap<(FileId, usize, usize, Rule), usize>,
}

impl Model {
    /// The node a reference names.
    pub fn node(&self, r: NodeRef) -> &Node {
        let mut node = &self.sources[r.file].tree.root;
        for &step in &self.nodes[r.file][r.index].path {
            node = match &node.children[step as usize] {
                Child::Node(child) => child,
                _ => unreachable!("node path leads to a token"),
            };
        }
        node
    }

    pub fn info(&self, r: NodeRef) -> &NodeInfo {
        &self.nodes[r.file][r.index]
    }

    /// The reference of a node of a file, found by the tokens it covers and its rule.
    pub fn node_ref(&self, file: FileId, node: &Node) -> Option<NodeRef> {
        self.node_keys
            .get(&(file, node.start, node.end, node.rule))
            .map(|&index| NodeRef { file, index })
    }

    /// The parent node of a node.
    pub fn parent(&self, r: NodeRef) -> Option<NodeRef> {
        self.nodes[r.file][r.index].parent.map(|index| NodeRef {
            file: r.file,
            index,
        })
    }

    /// The tokens of a file.
    pub fn tokens(&self, file: FileId) -> &[Token] {
        &self.sources[file].tree.tokens
    }

    /// The raw source a node covers.
    pub fn raw(&self, r: NodeRef) -> String {
        let info = &self.nodes[r.file][r.index];
        self.sources[r.file].tree.raw(info.start, info.end)
    }

    /// The first token a node covers.
    pub fn first_token(&self, r: NodeRef) -> Option<&Token> {
        let info = &self.nodes[r.file][r.index];
        self.sources[r.file].tree.tokens.get(info.start)
    }

    /// The file whose path this is.
    pub fn file(&self, path: &str) -> Option<FileId> {
        self.sources.iter().position(|source| source.path == path)
    }

    /// The scope a node owns, when it owns one.
    pub fn scope_of(&self, r: NodeRef) -> Option<ScopeId> {
        self.scope_by_node.get(&r).copied()
    }

    /// The usage a node made, when it made one.
    pub fn usage_of(&self, r: NodeRef) -> Option<UsageId> {
        self.usage_by_node.get(&r).copied()
    }

    /// The symbol a node declared, when it declared one.
    pub fn symbol_of(&self, r: NodeRef) -> Option<SymbolId> {
        self.symbol_by_node.get(&r).copied()
    }

    /// The nearest scope enclosing a node: the scope of the node itself when it owns
    /// one, else of its nearest ancestor that does.
    pub fn enclosing_scope(&self, r: NodeRef) -> ScopeId {
        let mut current = Some(r);
        while let Some(node) = current {
            if let Some(scope) = self.scope_of(node) {
                return scope;
            }
            current = self.parent(node);
        }
        self.file_scopes[r.file]
    }

    /// The first symbol named `name` walking from `scope` outward, imports included.
    pub fn lookup(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        let mut current = Some(scope);
        while let Some(id) = current {
            let scope = &self.scopes[id];
            if let Some(&symbol) = scope
                .symbols
                .iter()
                .chain(scope.imports.iter())
                .find(|&&symbol| self.symbols[symbol].name == name)
            {
                return Some(symbol);
            }
            current = scope.parent;
        }
        None
    }

    /// The symbol named `name` declared directly in `scope`, imports included.
    pub fn lookup_local(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        let scope = &self.scopes[scope];
        scope
            .symbols
            .iter()
            .chain(scope.imports.iter())
            .copied()
            .find(|&symbol| self.symbols[symbol].name == name)
    }

    /// Every symbol declared directly in the scope an entity owns: members, parameters,
    /// and enum members among them.
    pub fn members(&self, entity: EntityId) -> Vec<SymbolId> {
        match self.entities[entity].scope {
            Some(scope) => self.scopes[scope].symbols.clone(),
            None => Vec::new(),
        }
    }

    /// The trait `rule` of the grammar, when the program declares it.
    pub fn rule_entity(&self, identifier: &str) -> Option<EntityId> {
        let rule_trait = self.trait_named("rule")?;
        let mut found = None;
        for (id, entity) in self.entities.iter().enumerate() {
            if entity.identifier.as_deref() == Some(identifier) && entity.has_trait(rule_trait) {
                if found.is_some() {
                    return None;
                }
                found = Some(id);
            }
        }
        found
    }

    /// The one trait entity with this identifier in any file scope, if exactly one.
    pub fn trait_named(&self, identifier: &str) -> Option<EntityId> {
        let mut found = None;
        for &scope in &self.file_scopes {
            for &symbol in &self.scopes[scope].symbols {
                let symbol = &self.symbols[symbol];
                if symbol.kind == SymbolKind::Trait && symbol.name == identifier {
                    if found.is_some_and(|f| f != symbol.entity) {
                        return None;
                    }
                    found = Some(symbol.entity);
                }
            }
        }
        found
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
