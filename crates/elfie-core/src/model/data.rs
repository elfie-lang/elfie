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

/// Where a file comes from, which decides what its file scope's parent is.
// @lfy def/model/data.lfy:Origin
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Origin {
    /// Every file but the package `elfie`.
    #[default]
    Program, // @lfy def/model/data.lfy:Origin.program
    /// A file of the package `elfie` other than its main file.
    Library, // @lfy def/model/data.lfy:Origin.library
    /// The main file of the package `elfie`.
    Prelude, // @lfy def/model/data.lfy:Origin.prelude
}

impl Origin {
    /// The value of the enum member: how the origin is spelled.
    pub fn value(self) -> &'static str {
        match self {
            Origin::Program => "program",
            Origin::Library => "library",
            Origin::Prelude => "prelude",
        }
    }
}

/// Every context property; a name after the context accessor must be one of these. A
/// property is matched by its value, not its member name.
///
/// The properties are the members the prelude gives every entity seen through its
/// context layer: `Entity`, `Function`, and `Trait` of `lib/prelude`. That library is a
/// specification and is never generated, so this enumeration of it carries no marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextProperty {
    Identifier,
    Definition,
    Type,
    AcceptanceCriteria,
    Parameters,
    Output,
    Entities,
    Extenders,
    References,
    Like,
    Test,
    Tests,
    ItemType,
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
    TypeParameter, // @lfy def/model/data.lfy:SymbolKind.typeParameter
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
            SymbolKind::TypeParameter => "typeParameter",
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

/// A name bound to an entity in one scope.
// @lfy def/model/data.lfy:Symbol
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The name.
    pub name: String, // @lfy def/model/data.lfy:Symbol.name
    /// What the name is bound to; for an alias, the target's entity.
    pub entity: EntityId, // @lfy def/model/data.lfy:Symbol.entity
    pub kind: SymbolKind, // @lfy def/model/data.lfy:Symbol.kind
    /// The node that declared it.
    pub node: NodeRef, // @lfy def/model/data.lfy:Symbol.node
    /// The token that spells the name, when one does.
    pub name_token: Option<usize>,
    /// The scope that holds it.
    pub scope: ScopeId, // @lfy def/model/data.lfy:Symbol.scope
}

/// A region in which names resolve.
// @lfy def/model/data.lfy:Scope
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// The enclosing scope, settled for a file scope once every file is declared.
    // @lfy def/model/data.lfy:Scope.parent
    // @lfy def/model/data.lfy:Scope#Scope:Scope:63702a99fb7a3d155498fd0cfc8839b62e9c0eebe6543cb81ff763006d571f34
    // @lfy def/model/data.lfy:Scope#Scope:Scope:4402361d3767c258bf2e3e234fe5e7f7afa4b16ff000c9be247ce0ce346d91d9
    // @lfy def/model/data.lfy:Scope#Scope:Scope:88f6519881eecd7fc8282f1965f00635e2031400a6510eb901072f8d595e0812
    // @lfy def/model/data.lfy:Scope#Scope:Scope:d0d1d9f729ef5da8873af7dd76fe6f256b415cef6655f59c2d0b44144e696b3f
    pub parent: Option<ScopeId>,
    /// The node that owns it.
    pub owner: NodeRef,
    pub file: FileId,
    /// The symbols declared directly in it, in order, then the symbols its use statements
    /// import; only a file scope has any of the latter.
    // @lfy def/model/data.lfy:Scope#Scope:Scope:66bb6d7d2dfba72304adc42bb0a2890dd744b25f97e6d0ad706557d0dabf01d1
    pub symbols: Vec<SymbolId>, // @lfy def/model/data.lfy:Scope.symbols
    /// Which of [`Scope::symbols`] were imported rather than declared.
    pub imports: Vec<SymbolId>,
    /// The current entity.
    pub current: EntityId, // @lfy def/model/data.lfy:Scope.current
}

impl Scope {
    /// The symbols declared directly in it, imports left out.
    pub fn declared(&self) -> impl Iterator<Item = SymbolId> + '_ {
        self.symbols
            .iter()
            .copied()
            .filter(|symbol| !self.imports.contains(symbol))
    }
}

/// What applied a trait.
// @lfy def/model/data.lfy:Applied.source
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppliedSource {
    /// Applied directly by an `IsClause`.
    Is(NodeRef),
    /// Applied directly by an `ExtendsClause`.
    Extends(NodeRef),
    /// Applied directly by an apply `Call`.
    Apply(NodeRef),
    /// Inherited through the application at this index of the same entity's traits.
    // @lfy def/model/data.lfy:Entity#Entity:extensionClause:8d32d55313384cf1012c9e519e04b10038df70be66ada35f0b2b3cd59c9a95d3
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
    pub source: AppliedSource, // @lfy def/model/data.lfy:Applied.source
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

/// What a piece of knowledge is. `KnowledgeKind` of the package `elfie`, translated here
/// because [`Entity::knowledge`] holds it.
// @lfy def/model/data.lfy:Knowledge.kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnowledgeKind {
    Reference,
    Example,
    Definition,
    Tool,
}

impl KnowledgeKind {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            KnowledgeKind::Reference => {
                "documentation to read: a language reference, an API, a guide"
            }
            KnowledgeKind::Example => "working code to imitate",
            KnowledgeKind::Definition => "Elfie source that defines it, read like any other",
            KnowledgeKind::Tool => "a tool of the agent server the compiler calls",
        }
    }
}

/// Something the compiler is given to read, because it may not already know it.
/// `Knowledge` of the package `elfie`, translated here because [`Entity::knowledge`]
/// holds it.
// @lfy def/model/data.lfy:Knowledge
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knowledge {
    /// What it teaches, in a few words.
    pub topic: String, // @lfy def/model/data.lfy:Knowledge.topic
    /// What it is.
    pub kind: KnowledgeKind, // @lfy def/model/data.lfy:Knowledge.kind
    /// A path relative to the root of the package or project that declares it; a use path
    /// for a definition; the tool's name for a tool.
    pub source: String, // @lfy def/model/data.lfy:Knowledge.source
    /// Whether the request quotes it in full; `None` to let the compiler read it instead.
    pub quote: Option<bool>, // @lfy def/model/data.lfy:Knowledge.quote
    /// The entity whose body holds it: the entity itself or one of its traits.
    pub contributor: EntityId, // @lfy def/model/data.lfy:Knowledge.contributor
}

/// What a command is for. `Operation` of the package `elfie`, translated here because
/// [`Command::operation`] holds it.
// @lfy def/model/data.lfy:Command.operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    Install,
    Add,
    Build,
    Test,
    Lint,
    Format,
    Run,
}

impl Operation {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            Operation::Install => "install: fetch every dependency the manifest lists",
            Operation::Add => "add: add one dependency",
            Operation::Build => "build: compile or type-check the outputs",
            Operation::Test => "test: run the tests",
            Operation::Lint => "lint: run the linters",
            Operation::Format => "format: check the formatting without changing anything",
            Operation::Run => "run: start what was built",
        }
    }
}

/// A shell command for one operation, so the compiler need not know the tool. `Command`
/// of the package `elfie`, translated here because [`Entity::commands`] holds it.
// @lfy def/model/data.lfy:Command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// What it is for.
    pub operation: Operation, // @lfy def/model/data.lfy:Command.operation
    /// What `sh -c` runs in the target's output directory; `None` to say the operation is
    /// not run.
    pub line: Option<String>, // @lfy def/model/data.lfy:Command.line
    /// The entity whose body holds it: the entity itself or one of its traits.
    pub contributor: EntityId, // @lfy def/model/data.lfy:Command.contributor
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
    // @lfy def/model/data.lfy:Entity.test
    Tester(EntityId),
    /// `@like` of a type: a prompt describing an instance.
    // @lfy def/model/data.lfy:Entity.like
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
        agent: bool, // @lfy def/model/data.lfy:FnEntity.agentic
    },
    /// A declared trait seen through its context layer.
    // @lfy def/model/data.lfy:Entity
    Trait {
        parameters: Vec<SymbolId>,
        /// ContextProperty entities: everything the trait was applied to, directly or by
        /// inheritance, in file then application order.
        // @lfy def/model/data.lfy:Entity#Entity:traitClause:885abc6aa6f23dc47914f2e331127d72b788b2032d860ea70fb7bf826cf30233
        // @lfy def/model/data.lfy:Entity#Entity:extensionClause:5f20e2408a0369cfd8f5ff1815d6013094ecbc1e3cf0ac4dd2571d1cda5b83c7
        // @lfy def/model/data.lfy:Entity#Entity:extensionClause:543a94248601666a7d946001c384de0faaa8613d6d0df610e8a946a349e84757
        entities: Vec<EntityId>,
        /// ContextProperty extenders: every trait that names it in an ExtendsClause.
        // @lfy def/model/data.lfy:Entity#Entity:extensionClause:56a3f99ebcf66cb547618658a10dd4e086a38d56c5e0736c52e9f7cc5d82233c
        extenders: Vec<EntityId>,
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

/// A declared thing seen through its context layer.
// @lfy def/model/data.lfy:Entity
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    /// The declared name; `None` when anonymous.
    pub identifier: Option<String>, // @lfy def/model/data.lfy:Entity.identifier
    /// The description provided for this entity, resolved.
    pub definition: Option<String>, // @lfy def/model/data.lfy:Entity.definition
    /// The node of the string or template the definition was written as.
    pub definition_node: Option<NodeRef>,
    /// The declared or inferred type.
    pub ty: Option<TypeRef>, // @lfy def/model/data.lfy:Entity.type
    /// Acceptance criteria specifying own criteria first, then each trait's in
    /// application order.
    pub acceptance_criteria: Vec<Criterion>, // @lfy def/model/data.lfy:Entity.acceptanceCriteria
    /// Every trait applied to it, direct or inherited, in application order, each with
    /// what applied it.
    // @lfy def/model/data.lfy:Entity#Entity:traitList:a8b6d4d3d7cc54933301dcfe026cc4267738079adceed510560830d01aae2ee0
    // @lfy def/model/data.lfy:Entity#Entity:traitList:49625aa48b0703f6a1c12a97eb8d56fdb19b02bb6918e843148d26465b703698
    pub traits: Vec<Applied>, // @lfy def/model/data.lfy:Entity.traits
    /// What a list-typed entity holds; `None` when the type is not a list.
    pub item_type: Option<TypeRef>, // @lfy def/model/data.lfy:Entity.itemType
    /// Every entity referenced via a Reference in definition, documentation, or value.
    pub references: Vec<UsageId>, // @lfy def/model/data.lfy:Entity.references
    /// ContextProperty tests: list of all added tests.
    pub tests: Vec<Test>, // @lfy def/model/data.lfy:Entity.tests
    /// What the compiler is given to read for it: its own items first, then each trait's
    /// in application order.
    pub knowledge: Vec<Knowledge>, // @lfy def/model/data.lfy:Entity.knowledge
    /// Shell commands for its operations: its own first, then each trait's in application
    /// order.
    pub commands: Vec<Command>, // @lfy def/model/data.lfy:Entity.commands
    /// The targets added to it, in the order they were added; each the entity of an ace
    /// const of the project holding a `Target` of the package `elfie`.
    pub targets: Vec<EntityId>, // @lfy def/model/data.lfy:Entity.targets
    /// The value setters evaluated for it (`.name = value` in a trait or data body).
    pub values: Vec<(String, Value)>,
    pub kind: EntityKind,
    /// The declaring node; `None` for global.
    pub node: Option<NodeRef>, // @lfy def/model/data.lfy:Entity.node
    pub symbol: Option<SymbolId>, // @lfy def/model/data.lfy:Entity.symbol
    /// The scope it owns, when it owns one.
    pub scope: Option<ScopeId>, // @lfy def/model/data.lfy:Entity.scope
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

/// The stage that reported a diagnostic.
// @lfy def/model/data.lfy:Stage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Binder,     // @lfy def/model/data.lfy:Stage.binder
    Generation, // @lfy def/model/data.lfy:Stage.generation
    Lexer,      // @lfy def/model/data.lfy:Stage.lexer
    Loader,     // @lfy def/model/data.lfy:Stage.loader
    Parser,     // @lfy def/model/data.lfy:Stage.parser
}

impl Stage {
    /// The value of the enum member: how the stage is spelled.
    pub fn value(self) -> &'static str {
        match self {
            Stage::Binder => "binder",
            Stage::Generation => "generation",
            Stage::Lexer => "lexer",
            Stage::Loader => "loader",
            Stage::Parser => "parser",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
    }
}

/// Something that did not resolve; binding continues past it.
// @lfy def/model/data.lfy:Problem
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Where.
    pub node: NodeRef, // @lfy def/model/data.lfy:Problem.node
    /// What and why.
    pub message: String, // @lfy def/model/data.lfy:Problem.message
    /// Which stage added it.
    pub stage: Stage, // @lfy def/model/data.lfy:Problem.stage
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
    /// [`Origin::Prelude`] for the main file of the package `elfie`, [`Origin::Library`]
    /// for its other files, [`Origin::Program`] for every other file.
    pub origin: Origin, // @lfy def/model/data.lfy:Source.origin
}

/// The program, queryable.
// @lfy def/model/data.lfy:Model
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
    pub sources: Vec<Source>,
    pub scopes: Vec<Scope>,
    pub symbols: Vec<Symbol>,
    pub entities: Vec<Entity>,
    pub usages: Vec<Usage>,
    /// Every [`Problem`] the binder recorded.
    pub problems: Vec<Problem>, // @lfy def/model/data.lfy:Model.problems
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

// @lfy def/model/data.lfy:Model#Model:Model:7f6312c0336cc0a062228b497a9b06392771a2b287363f8ed24cbfb40efe8d28
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
                .find(|&&symbol| self.symbols[symbol].name == name)
            {
                return Some(symbol);
            }
            current = scope.parent;
        }
        None
    }

    /// The symbol named `name` held directly by `scope`, imports included.
    pub fn lookup_local(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        let scope = &self.scopes[scope];
        scope
            .symbols
            .iter()
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

    /// Whether more than one entity carrying the trait `rule` shares this identifier, so
    /// a reference that spells it resolves to neither.
    // @lfy def/model/main.lfy:bind
    pub fn rule_identifier_is_ambiguous(&self, identifier: &str) -> bool {
        let Some(rule_trait) = self.trait_named("rule") else {
            return false;
        };
        self.entities
            .iter()
            .filter(|entity| {
                entity.identifier.as_deref() == Some(identifier) && entity.has_trait(rule_trait)
            })
            .count()
            > 1
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
