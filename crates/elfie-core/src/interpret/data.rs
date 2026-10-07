//! Compiled from `def/interpret/data.lfy`: the data of interpretation.
//!
//! A [`Phase`] says whether a piece of source runs while binding or is kept for a target.
//! A [`Value`] is what evaluating an expression at compile time gives, and a [`Prompted`]
//! is a value only the compiler can choose, left for generation; [`Evaluated`] is either.
//! A [`LoweredCriterion`] and a [`LoweredTest`] are one criterion and one test as
//! generation and verification see them: entities of their own, apart from the code, each
//! with an id a verifier can answer by. A [`LoweredNode`] is one node of the program with
//! runtime code only and compile-time results in place, a [`LoweredFile`] one file of
//! them, and a [`Program`] the whole workspace lowered.
//!
//! The `Entity`, `Node`, and `File` fields of the definition are kept as an `EntityId`
//! into `Model::entities`, a `NodeRef` into the model's node table, and a `FileId` into
//! `Workspace::files`, as the rest of the compiler keeps them, so that a program is one
//! plain value that does not clone the parse trees it points back at.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::grammar::{Entity, GrammarRule, Rule};
use crate::lexer::Token;
use crate::model::{Criterion, EntityId, FileId, Model, NodeRef, Problem, ScopeId, Test};
use crate::workspace::Workspace;

/// The name an id begins with when the requirement is for the whole program, and the name
/// of the entity every global requirement is added to.
const GLOBAL: &str = "global";
/// The name of an entity that has none.
const ANONYMOUS: &str = "anonymous";

// ---------------------------------------------------------------------------------------
// Phase
// ---------------------------------------------------------------------------------------

/// When a piece of source runs.
// @lfy def/interpret/data.lfy:Phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// It runs while binding and leaves only its results.
    Compile, // @lfy def/interpret/data.lfy:Phase.compile
    /// It is kept, and generated for a target.
    Runtime, // @lfy def/interpret/data.lfy:Phase.runtime
}

impl Phase {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            Phase::Compile => "compile time: it runs while binding and leaves only its results",
            Phase::Runtime => "runtime: it is kept, and generated for a target",
        }
    }
}

// ---------------------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------------------

/// What evaluating an expression at compile time gives.
///
/// The alternatives are every result an evaluated expression can have: `undefined`, `null`,
/// a boolean, a number, a string, a list or object of values, an entity, a scope, or a
/// function with the scope it was written in.
// @lfy def/interpret/data.lfy:Value
// @lfy def/interpret/data.lfy:Value#Value:Value:a904dd93fc132523ce6216cac3e63589e7509c3269dbe38ee30d35a7a9f362dd
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    List(Vec<Value>),
    /// An object of values, by key.
    // Decision: the definition names an object of values and no criterion asks for the
    // order its keys were written in, so it is kept as the map the target names for an
    // object whose value type is known.
    Object(BTreeMap<String, Value>),
    /// A declared thing, as an index into `Model::entities`.
    Entity(EntityId),
    /// A region in which names resolve, as an index into `Model::scopes`.
    Scope(ScopeId),
    /// A function: the node of the declaration or inline function it came from, and the
    /// scope it was written in.
    Function(NodeRef, ScopeId),
}

impl Value {
    /// How the value is written into a lowered tree: one that is not an entity, a scope,
    /// or a function is spelled as the literal of its kind; one that is an entity or a
    /// scope as a reference to its declaration; one that is a function as a reference to
    /// the declaration it came from.
    // @lfy def/interpret/data.lfy:LoweredFile.text
    pub fn spelled(&self, model: &Model) -> String {
        match self {
            // @lfy def/interpret/data.lfy:LoweredFile.text#text:text:e2912e3f2fc81837d841d33742d949fdbe294efd354fc935eb7dab8fdfe7bd3f
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Boolean(boolean) => boolean.to_string(),
            Value::Number(number) => number_literal(*number),
            Value::String(text) => string_literal(text),
            Value::List(items) => {
                let items: Vec<String> = items.iter().map(|item| item.spelled(model)).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Object(pairs) if pairs.is_empty() => "{}".to_string(),
            Value::Object(pairs) => {
                let pairs: Vec<String> = pairs
                    .iter()
                    .map(|(name, value)| format!("{name} = {}", value.spelled(model)))
                    .collect();
                format!("{{ {} }}", pairs.join(", "))
            }
            // @lfy def/interpret/data.lfy:LoweredFile.text#text:text:f36ad3f32d9c0b89aa7131c1ee302bccfd5915675878ffe2ef6d0f5e0bbcec19
            Value::Entity(entity) => declaration_name(model, *entity),
            // A scope is spelled as the declaration that owns it: its current entity.
            Value::Scope(scope) => declaration_name(model, model.scopes[*scope].current),
            // @lfy def/interpret/data.lfy:LoweredFile.text#text:text:61c2941bf25b13fe45f4c73bbdabbab6f9e227762abea01d76e0cf733290ab3b
            Value::Function(node, _) => declaring_name(model, *node),
        }
    }
}

/// A number as Elfie spells it: a whole number without a fractional part.
// @lfy def/interpret/data.lfy:Value
fn number_literal(number: f64) -> String {
    if number.is_finite() && number.fract() == 0.0 {
        format!("{}", number as i64)
    } else {
        number.to_string()
    }
}

/// A string as Elfie spells it: a `DoubleQuoteString`, with every character a
/// `DoubleQuoteBody` excludes written as the escape that becomes it.
// @lfy def/interpret/data.lfy:Value
fn string_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            _ => out.push(character),
        }
    }
    out.push('"');
    out
}

/// A reference to an entity's declaration: the name it was declared with.
// @lfy def/interpret/data.lfy:Value
fn declaration_name(model: &Model, entity: EntityId) -> String {
    model.entities[entity]
        .identifier
        .clone()
        .unwrap_or_else(|| ANONYMOUS.to_string())
}

/// A reference to the declaration a node came from: the name the nearest declaration at or
/// above it bound, so an inline function is spelled as the declaration it was written in.
// @lfy def/interpret/data.lfy:Value
fn declaring_name(model: &Model, node: NodeRef) -> String {
    let mut at = Some(node);
    while let Some(current) = at {
        if let Some(symbol) = model.symbol_of(current) {
            return model.symbols[symbol].name.clone();
        }
        at = model.parent(current);
    }
    ANONYMOUS.to_string()
}

// ---------------------------------------------------------------------------------------
// Prompted
// ---------------------------------------------------------------------------------------

/// A value only the compiler can choose, left for generation to choose.
///
/// Evaluating a call of `like` on an entity's context gives one of these and never a value
/// the interpreter picks, so binding stays deterministic; which value it stands for is
/// decided by `evaluate` in `def/interpret/main.lfy`.
// @lfy def/interpret/data.lfy:Prompted
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompted {
    /// The type the chosen value must have, as an index into `Model::entities`.
    pub value_type: EntityId, // @lfy def/interpret/data.lfy:Prompted.valueType
    /// The prompt that describes it, with its references and executions rendered.
    pub prompt: String, // @lfy def/interpret/data.lfy:Prompted.prompt
}

/// What evaluating an expression gives: a [`Value`], or a [`Prompted`] that only
/// generation can answer.
// @lfy def/interpret/data.lfy:LoweredNode.value
#[derive(Debug, Clone, PartialEq)]
pub enum Evaluated {
    Value(Value),
    Prompted(Prompted),
}

impl Evaluated {
    pub fn as_value(&self) -> Option<&Value> {
        match self {
            Evaluated::Value(value) => Some(value),
            Evaluated::Prompted(_) => None,
        }
    }

    pub fn as_prompted(&self) -> Option<&Prompted> {
        match self {
            Evaluated::Prompted(prompted) => Some(prompted),
            Evaluated::Value(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Criteria and tests
// ---------------------------------------------------------------------------------------

/// Whom a criterion or test is for.
// @lfy def/interpret/data.lfy:RequirementScope
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequirementScope {
    /// One entity, a member, or a file.
    Local, // @lfy def/interpret/data.lfy:RequirementScope.local
    /// The whole program: every unit sees it, and it is verified once.
    Global, // @lfy def/interpret/data.lfy:RequirementScope.global
}

impl RequirementScope {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            RequirementScope::Local => "one entity, a member, or a file",
            RequirementScope::Global => {
                "the whole program: every unit sees it, and it is verified once"
            }
        }
    }
}

/// The name an id begins with: the receiving entity's name, or `global` for a requirement
/// of the whole program. A file's own entity is anonymous, so its file's path is its name.
// @lfy def/interpret/data.lfy:LoweredCriterion.id
pub fn receiver_name(model: &Model, entity: Option<EntityId>) -> String {
    let Some(entity) = entity else {
        return GLOBAL.to_string();
    };
    let record = &model.entities[entity];
    if let Some(identifier) = &record.identifier {
        return identifier.clone();
    }
    match record.file {
        Some(file) if model.file_entities.get(file) == Some(&entity) => {
            model.sources[file].path.clone()
        }
        _ => ANONYMOUS.to_string(),
    }
}

/// One acceptance criterion as generation and verification see it: an entity of its own,
/// apart from the code.
///
/// `LoweredCriterion extends Criterion`, so the situation, behavior, side effects, and
/// contributor of the criterion it was lowered from are its own; they are held as rendered,
/// with each bracketed reference replaced by the name it resolved to and the entities it
/// named kept in [`LoweredCriterion::references`].
// @lfy def/interpret/data.lfy:LoweredCriterion
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredCriterion {
    /// The receiving entity's name, or `global` for a global criterion, then a colon, the
    /// contributor's name, a colon, and the SHA-256 of its situation, behavior, and side
    /// effects as rendered, in lowercase hex.
    pub id: String, // @lfy def/interpret/data.lfy:LoweredCriterion.id
    /// Whether it is for one entity or for the whole program.
    pub scope: RequirementScope, // @lfy def/interpret/data.lfy:LoweredCriterion.scope
    /// The entity it is for; `None` when global.
    pub entity: Option<EntityId>, // @lfy def/interpret/data.lfy:LoweredCriterion.entity
    /// The `Where` statement or `add` call that wrote it.
    pub origin: NodeRef, // @lfy def/interpret/data.lfy:LoweredCriterion.origin
    /// Every entity it names with a bracketed reference, in order, kept as links rather
    /// than rendered names.
    pub references: Vec<EntityId>, // @lfy def/interpret/data.lfy:LoweredCriterion.references
    /// The entity whose body holds it: the entity itself or one of its traits.
    pub contributor: EntityId, // @lfy def/interpret/data.lfy:LoweredCriterion
    /// When it applies: one, several that must all hold, or none for always.
    pub situation: Option<Vec<String>>, // @lfy def/interpret/data.lfy:LoweredCriterion
    /// What must happen: one, or several.
    pub behavior: Option<Vec<String>>, // @lfy def/interpret/data.lfy:LoweredCriterion
    /// What is written outside the value, when anything is.
    pub side_effects: Option<Vec<String>>, // @lfy def/interpret/data.lfy:LoweredCriterion
}

impl LoweredCriterion {
    /// `$id` of a criterion for `receiver` written by `contributor`, before any repeat is
    /// numbered.
    // @lfy def/interpret/data.lfy:LoweredCriterion.id
    pub fn id(receiver: &str, contributor: &str, criterion: &Criterion) -> String {
        format!("{receiver}:{contributor}:{}", sha256(&rendered(criterion)))
    }

    /// Numbers the ids that repeat: among the criteria of one entity that would have the
    /// same id, the first keeps it and the second and later ones end with a colon and
    /// their position among those, counting from 2.
    // @lfy def/interpret/data.lfy:LoweredCriterion
    // @lfy def/interpret/data.lfy:LoweredCriterion.id#id:id:ed05634f7443e116d5eee3de2c022c9636ed6a3a82191382f7e4cc95bf274c9c
    pub fn number_repeats(criteria: &mut [LoweredCriterion]) {
        let keys: Vec<(Option<EntityId>, String)> = criteria
            .iter()
            .map(|criterion| (criterion.entity, criterion.id.clone()))
            .collect();
        for (criterion, position) in criteria.iter_mut().zip(repeats(&keys)) {
            if let Some(position) = position {
                criterion.id = format!("{}:{position}", criterion.id);
            }
        }
    }
}

/// One test as generation and verification see it: an entity of its own, apart from the
/// code.
///
/// `LoweredTest extends Test`, so the input and expect of the test it was lowered from are
/// its own: the nodes they were written as, and the source text of each.
// @lfy def/interpret/data.lfy:LoweredTest
#[derive(Debug, Clone, PartialEq)]
pub struct LoweredTest {
    /// The receiving entity's name, or `global` for a global test, then a colon, the name
    /// of the entity whose body holds it, a colon, and the SHA-256 of its input and expect
    /// as spelled, in lowercase hex.
    pub id: String, // @lfy def/interpret/data.lfy:LoweredTest.id
    /// Whether it is for one entity or for the whole program.
    pub scope: RequirementScope, // @lfy def/interpret/data.lfy:LoweredTest.scope
    /// The entity it is for; `None` when global.
    pub entity: Option<EntityId>, // @lfy def/interpret/data.lfy:LoweredTest.entity
    /// The `test` call argument that wrote it.
    pub origin: NodeRef, // @lfy def/interpret/data.lfy:LoweredTest.origin
    /// Its input, evaluated.
    pub input_value: Evaluated, // @lfy def/interpret/data.lfy:LoweredTest.inputValue
    /// What is expected, evaluated.
    pub expect_value: Evaluated, // @lfy def/interpret/data.lfy:LoweredTest.expectValue
    /// Input for the test: the argument list for a function, a value otherwise.
    pub input: Option<NodeRef>, // @lfy def/interpret/data.lfy:LoweredTest
    /// What is expected: the output for a function, a value otherwise.
    pub expect: Option<NodeRef>, // @lfy def/interpret/data.lfy:LoweredTest
    /// The source text of the input, as spelled.
    pub input_text: String, // @lfy def/interpret/data.lfy:LoweredTest
    /// The source text of the expectation, as spelled.
    pub expect_text: String, // @lfy def/interpret/data.lfy:LoweredTest
}

impl LoweredTest {
    /// `$id` of a test for `receiver` held by the body of `contributor`, before any repeat
    /// is numbered. The input and expect are hashed as spelled, joined by ` => `.
    // @lfy def/interpret/data.lfy:LoweredTest.id
    pub fn id(receiver: &str, contributor: &str, test: &Test) -> String {
        let spelled = format!("{} => {}", test.input_text, test.expect_text);
        format!("{receiver}:{contributor}:{}", sha256(&spelled))
    }

    /// Numbers the ids that repeat: among the tests of one entity that would have the same
    /// id, the first keeps it and the second and later ones end with a colon and their
    /// position among those, counting from 2.
    // @lfy def/interpret/data.lfy:LoweredTest
    // @lfy def/interpret/data.lfy:LoweredTest.id#id:id:5a10510bfa08aca3aa8cf29f02f3376d2fc83e1cd41991620d7b5f566c667508
    pub fn number_repeats(tests: &mut [LoweredTest]) {
        let keys: Vec<(Option<EntityId>, String)> = tests
            .iter()
            .map(|test| (test.entity, test.id.clone()))
            .collect();
        for (test, position) in tests.iter_mut().zip(repeats(&keys)) {
            if let Some(position) = position {
                test.id = format!("{}:{position}", test.id);
            }
        }
    }
}

/// For each key, its position among the keys equal to it, counting from 2; `None` for the
/// first of them.
// @lfy def/interpret/data.lfy:LoweredCriterion
fn repeats(keys: &[(Option<EntityId>, String)]) -> Vec<Option<usize>> {
    let mut out = Vec::with_capacity(keys.len());
    for (index, key) in keys.iter().enumerate() {
        let earlier = keys[..index].iter().filter(|other| *other == key).count();
        out.push((earlier > 0).then_some(earlier + 1));
    }
    out
}

/// A criterion as it is rendered for a request: `When` and its situations, then its
/// behaviors, then `Side effects:` and its side effects, joined by `: `.
// @lfy def/interpret/data.lfy:LoweredCriterion.id
fn rendered(criterion: &Criterion) -> String {
    let mut parts = Vec::new();
    if let Some(situation) = &criterion.situation {
        parts.push(format!("When {}", situation.join(" ")));
    }
    if let Some(behavior) = &criterion.behavior {
        parts.push(behavior.join(" "));
    }
    if let Some(side_effects) = &criterion.side_effects {
        parts.push(format!("Side effects: {}", side_effects.join(" ")));
    }
    parts.join(": ")
}

/// The SHA-256 of a text, in lowercase hex.
// @lfy def/interpret/data.lfy:LoweredCriterion.id
fn sha256(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------------------
// The lowered program
// ---------------------------------------------------------------------------------------

/// One entry of [`LoweredNode::children`]: a lowered node, or a token as the lexer
/// delivered it.
// @lfy def/interpret/data.lfy:LoweredNode.children
#[derive(Debug, Clone, PartialEq)]
pub enum LoweredChild {
    Node(LoweredNode),
    Token(Token),
}

impl LoweredChild {
    pub fn as_node(&self) -> Option<&LoweredNode> {
        match self {
            LoweredChild::Node(node) => Some(node),
            LoweredChild::Token(_) => None,
        }
    }

    pub fn as_token(&self) -> Option<&Token> {
        match self {
            LoweredChild::Token(token) => Some(token),
            LoweredChild::Node(_) => None,
        }
    }
}

/// One node of the program as generation sees it: runtime code only, with compile-time
/// results in place.
///
/// Every lowered node has an origin, so every place generation reads maps back to the
/// source. A lowered node is one of three things: a kept runtime node, which has its
/// children and no value; a folded value, which has a value and no children; or a member
/// of its parent's entity, whose entity is that member. Nothing whose [`Phase`] is
/// [`Phase::Compile`] is kept as written, which is what `lower` in
/// `def/interpret/main.lfy` leaves out.
// @lfy def/interpret/data.lfy:LoweredNode
// @lfy def/interpret/data.lfy:LoweredNode#LoweredNode:LoweredNode:e9deb0370846a9748fe95f5eb5ef3f67f823b463042c92f71770d400a1f499c9
#[derive(Debug, Clone, PartialEq)]
pub struct LoweredNode {
    /// The rule it satisfies. [`LoweredNode::rule`] gives its EBNF form.
    pub rule: Entity, // @lfy def/interpret/data.lfy:LoweredNode.rule
    /// Lowered nodes and tokens in source order; a folded node has none.
    pub children: Vec<LoweredChild>, // @lfy def/interpret/data.lfy:LoweredNode.children
    /// The node of the parse tree it was lowered from; for a folded node, the expression it
    /// replaced.
    pub origin: NodeRef, // @lfy def/interpret/data.lfy:LoweredNode.origin
    /// The entity it declares; `None` when it declares none.
    pub entity: Option<EntityId>, // @lfy def/interpret/data.lfy:LoweredNode.entity
    /// What a folded node holds; `None` for a node kept as written.
    pub value: Option<Evaluated>, // @lfy def/interpret/data.lfy:LoweredNode.value
    /// The local criteria of its entity, in `Entity.acceptanceCriteria` order; empty when
    /// it declares none.
    pub criteria: Vec<LoweredCriterion>, // @lfy def/interpret/data.lfy:LoweredNode.criteria
    /// The local tests of its entity, in order; empty when it declares none.
    pub tests: Vec<LoweredTest>, // @lfy def/interpret/data.lfy:LoweredNode.tests
}

impl LoweredNode {
    /// `$rule` as the EBNF form of the rule this node satisfies.
    // @lfy def/interpret/data.lfy:LoweredNode.rule
    pub fn rule(&self) -> Rule {
        self.rule.rule()
    }

    /// Whether the node holds a folded value rather than children kept as written.
    // @lfy def/interpret/data.lfy:LoweredNode
    // @lfy def/interpret/data.lfy:LoweredNode#LoweredNode:LoweredNode:e9deb0370846a9748fe95f5eb5ef3f67f823b463042c92f71770d400a1f499c9
    pub fn is_folded(&self) -> bool {
        self.value.is_some()
    }

    /// The child lowered nodes, in order.
    pub fn nodes(&self) -> impl Iterator<Item = &LoweredNode> {
        self.children.iter().filter_map(LoweredChild::as_node)
    }
}

/// One file as generation sees it.
// @lfy def/interpret/data.lfy:LoweredFile
#[derive(Debug, Clone, PartialEq)]
pub struct LoweredFile {
    /// The file it was lowered from, as an index into `Workspace::files`.
    pub file: FileId, // @lfy def/interpret/data.lfy:LoweredFile.file
    /// The lowered `SourceFile`.
    pub root: LoweredNode, // @lfy def/interpret/data.lfy:LoweredFile.root
    /// The lowered tree spelled as Elfie source: kept nodes as written, folded nodes as
    /// their value, member nodes as a member with its type and value; never
    /// a criterion or a test.
    pub text: String, // @lfy def/interpret/data.lfy:LoweredFile.text
    /// The local criteria of the file's own entity, in order.
    pub criteria: Vec<LoweredCriterion>, // @lfy def/interpret/data.lfy:LoweredFile.criteria
    /// The local tests of the file's own entity, in order.
    pub tests: Vec<LoweredTest>, // @lfy def/interpret/data.lfy:LoweredFile.tests
}

/// A workspace with every file lowered: what generation reads.
// @lfy def/interpret/data.lfy:Program
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The workspace lowered.
    pub workspace: Workspace, // @lfy def/interpret/data.lfy:Program.workspace
    /// One per file of `Workspace::files`, in the same order.
    pub files: Vec<LoweredFile>, // @lfy def/interpret/data.lfy:Program.files
    /// Every global criterion, each once, in the order they were added.
    pub criteria: Vec<LoweredCriterion>, // @lfy def/interpret/data.lfy:Program.criteria
    /// Every global test, each once, in the order they were added.
    pub tests: Vec<LoweredTest>, // @lfy def/interpret/data.lfy:Program.tests
    /// Every problem lowering added, in node order, each with `Problem::stage` of
    /// `Stage::Generation`.
    pub problems: Vec<Problem>, // @lfy def/interpret/data.lfy:Program.problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::rules::file::File as FileRule;
    use crate::grammar::rules::statement::Statement;
    use crate::lexer::lex;
    use crate::model::{Origin, Source, bind};
    use crate::parser::parse;

    /// One file, `a.lfy`, bound alone.
    fn bound(text: &str) -> Model {
        let tokens = lex(text, Some("a.lfy")).expect("the fixture lexes");
        let tree = parse(tokens, None);
        assert!(tree.errors.is_empty(), "the fixture parses: {}", tree.render());
        bind(vec![Source {
            path: "a.lfy".to_string(),
            tree,
            uses: Vec::new(),
            origin: Origin::Program,
        }])
    }

    /// The entity a name was declared for.
    fn entity_named(model: &Model, name: &str) -> EntityId {
        model
            .entities
            .iter()
            .position(|entity| entity.identifier.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("{name} is declared"))
    }

    /// A node reference standing for a place in `a.lfy`: its root.
    fn somewhere(model: &Model) -> NodeRef {
        model
            .node_ref(0, &model.sources[0].tree.root)
            .expect("the root has a reference")
    }

    fn criterion(situation: Option<&str>, behavior: &str, contributor: EntityId) -> Criterion {
        Criterion {
            situation: situation.map(|text| vec![text.to_string()]),
            behavior: Some(vec![behavior.to_string()]),
            side_effects: None,
            contributor,
            node: None,
        }
    }

    fn lowered(id: String, entity: Option<EntityId>, origin: NodeRef) -> LoweredCriterion {
        LoweredCriterion {
            id,
            scope: RequirementScope::Local,
            entity,
            origin,
            references: Vec::new(),
            contributor: 0,
            situation: None,
            behavior: Some(vec!["it holds".to_string()]),
            side_effects: None,
        }
    }

    // @lfy def/interpret/data.lfy:Value
    // @lfy def/interpret/data.lfy:Value#Value:Value:a904dd93fc132523ce6216cac3e63589e7509c3269dbe38ee30d35a7a9f362dd
    #[test]
    fn an_evaluated_expression_is_one_of_the_kinds_a_value_has() {
        let model = bound("d A: `An a` {\n  $x = string;\n}\n");
        let a = entity_named(&model, "A");
        let scope = model.entities[a].scope.expect("A owns a scope");
        let node = somewhere(&model);
        let every = vec![
            Value::Undefined,
            Value::Null,
            Value::Boolean(true),
            Value::Number(1.5),
            Value::String("a".to_string()),
            Value::List(vec![Value::Null]),
            Value::Object(BTreeMap::from([("k".to_string(), Value::Null)])),
            Value::Entity(a),
            Value::Scope(scope),
            Value::Function(node, scope),
        ];
        // A list or an object of values holds values, and nothing else is a value.
        assert_eq!(every.len(), 10);
        assert_eq!(Value::List(every.clone()).spelled(&model).matches(", ").count(), 9);
        assert!(every.iter().all(|value| !value.spelled(&model).is_empty()));
    }

    // @lfy def/interpret/data.lfy:LoweredFile.text
    // @lfy def/interpret/data.lfy:LoweredFile.text#text:text:e2912e3f2fc81837d841d33742d949fdbe294efd354fc935eb7dab8fdfe7bd3f
    #[test]
    fn a_value_that_is_not_an_entity_a_scope_or_a_function_is_spelled_as_its_literal() {
        let model = bound("d A: `An a` {\n  $x = string;\n}\n");
        assert_eq!(Value::Undefined.spelled(&model), "undefined");
        assert_eq!(Value::Null.spelled(&model), "null");
        assert_eq!(Value::Boolean(true).spelled(&model), "true");
        assert_eq!(Value::Boolean(false).spelled(&model), "false");
        assert_eq!(Value::Number(2.0).spelled(&model), "2");
        assert_eq!(Value::Number(-0.5).spelled(&model), "-0.5");
        assert_eq!(Value::String("a word".to_string()).spelled(&model), "\"a word\"");
        // Every character a `DoubleQuoteBody` excludes is written as the escape for it.
        assert_eq!(
            Value::String("a\"b\\c\nd\te\rf\0g".to_string()).spelled(&model),
            "\"a\\\"b\\\\c\\nd\\te\\rf\\0g\""
        );
        assert_eq!(
            Value::List(vec![Value::Number(1.0), Value::String("x".to_string())]).spelled(&model),
            "[1, \"x\"]"
        );
        assert_eq!(Value::List(Vec::new()).spelled(&model), "[]");
        assert_eq!(Value::Object(BTreeMap::new()).spelled(&model), "{}");
        assert_eq!(
            Value::Object(BTreeMap::from([
                ("a".to_string(), Value::Number(1.0)),
                ("b".to_string(), Value::Null),
            ]))
            .spelled(&model),
            "{ a = 1, b = null }"
        );
    }

    // @lfy def/interpret/data.lfy:LoweredFile.text
    // @lfy def/interpret/data.lfy:LoweredFile.text#text:text:f36ad3f32d9c0b89aa7131c1ee302bccfd5915675878ffe2ef6d0f5e0bbcec19
    #[test]
    fn an_entity_or_a_scope_is_spelled_as_a_reference_to_its_declaration() {
        let model = bound("d A: `An a` {\n  $x = string;\n}\n");
        let a = entity_named(&model, "A");
        assert_eq!(Value::Entity(a).spelled(&model), "A");
        let scope = model.entities[a].scope.expect("A owns a scope");
        assert_eq!(Value::Scope(scope).spelled(&model), "A");
        // A file's own entity is declared by nothing and has no name of its own.
        assert_eq!(Value::Entity(model.file_entities[0]).spelled(&model), "anonymous");
    }

    // @lfy def/interpret/data.lfy:LoweredFile.text
    // @lfy def/interpret/data.lfy:LoweredFile.text#text:text:61c2941bf25b13fe45f4c73bbdabbab6f9e227762abea01d76e0cf733290ab3b
    #[test]
    fn a_function_is_spelled_as_a_reference_to_the_declaration_it_came_from() {
        let model = bound("function f() -> number {\n  return 1;\n}\n");
        let f = entity_named(&model, "f");
        let declaration = model.entities[f].node.expect("f has a declaring node");
        let scope = model.entities[f].scope.expect("f owns a scope");
        assert_eq!(Value::Function(declaration, scope).spelled(&model), "f");
        // A function written inside a declaration is spelled as that declaration.
        let tree = &model.sources[0].tree;
        let inside = tree.root.find(Statement::Return).expect("the return statement");
        let inside = model.node_ref(0, inside).expect("the return has a reference");
        assert_eq!(Value::Function(inside, scope).spelled(&model), "f");
    }

    /// A `Prompted` is a value only the compiler can choose: it keeps the type the value
    /// must have and the prompt that describes it, and it is an alternative of
    /// [`Evaluated`] beside [`Value`] rather than one of `Value`'s own kinds, so nothing
    /// can hand back a value the interpreter picked in its place.
    // @lfy def/interpret/data.lfy:Prompted
    #[test]
    fn a_prompted_keeps_its_type_and_prompt_and_is_no_value() {
        let model = bound("d A: `An a` {\n  $x = string;\n}\n");
        let a = entity_named(&model, "A");
        let prompted = Prompted {
            value_type: a,
            prompt: "holding y".to_string(),
        };
        let evaluated = Evaluated::Prompted(prompted.clone());
        assert_eq!(evaluated.as_prompted(), Some(&prompted));
        assert_eq!(evaluated.as_value(), None);
        assert_eq!(Evaluated::Value(Value::Null).as_prompted(), None);
    }

    // @lfy def/interpret/data.lfy:LoweredCriterion.id
    // @lfy def/interpret/data.lfy:LoweredCriterion.id#id:id:ed05634f7443e116d5eee3de2c022c9636ed6a3a82191382f7e4cc95bf274c9c
    #[test]
    fn two_criteria_of_one_entity_with_the_same_id_are_numbered_from_two() {
        let model = bound("d A: `An a` {\n  $x = string;\n}\n");
        let a = entity_named(&model, "A");
        let origin = somewhere(&model);
        let receiver = receiver_name(&model, Some(a));
        assert_eq!(receiver, "A");
        assert_eq!(receiver_name(&model, None), "global");
        assert_eq!(receiver_name(&model, Some(model.file_entities[0])), "a.lfy");

        // The id is the receiver, the contributor, and the SHA-256 of the criterion as
        // rendered, in lowercase hex; rewording it gives another id.
        let one = criterion(Some("it is asked"), "it holds", a);
        let id = LoweredCriterion::id(&receiver, "A", &one);
        let (head, digest) = id.rsplit_once(':').expect("the id ends in its digest");
        assert_eq!(head, "A:A");
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
        assert_eq!(digest, sha256("When it is asked: it holds"));
        let reworded = criterion(Some("it is asked"), "it holds twice", a);
        assert_ne!(LoweredCriterion::id(&receiver, "A", &reworded), id);

        // Among the criteria of one entity that would have the same id, the second and
        // later ones end with a colon and their position, counting from 2.
        let other = entity_named(&model, "x");
        let mut criteria = vec![
            lowered(id.clone(), Some(a), origin),
            lowered(id.clone(), Some(a), origin),
            lowered(id.clone(), Some(a), origin),
            lowered(id.clone(), Some(other), origin),
        ];
        LoweredCriterion::number_repeats(&mut criteria);
        let ids: Vec<&str> = criteria.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, [&id, &format!("{id}:2"), &format!("{id}:3"), &id]);
    }

    // @lfy def/interpret/data.lfy:LoweredTest.id
    // @lfy def/interpret/data.lfy:LoweredTest.id#id:id:5a10510bfa08aca3aa8cf29f02f3376d2fc83e1cd41991620d7b5f566c667508
    #[test]
    fn two_tests_of_one_entity_with_the_same_id_are_numbered_from_two() {
        let model = bound("d A: `An a` {\n  $x = string;\n}\n");
        let a = entity_named(&model, "A");
        let origin = somewhere(&model);
        let case = Test {
            input: None,
            expect: None,
            input_text: "[\"y\"]".to_string(),
            expect_text: "A@like(`holding y`)".to_string(),
        };
        let id = LoweredTest::id("A", "A", &case);
        assert_eq!(id, format!("A:A:{}", sha256("[\"y\"] => A@like(`holding y`)")));
        let other = Test {
            expect_text: "A@like(`holding z`)".to_string(),
            ..case.clone()
        };
        assert_ne!(LoweredTest::id("A", "A", &other), id);

        let one = LoweredTest {
            id: id.clone(),
            scope: RequirementScope::Local,
            entity: Some(a),
            origin,
            input_value: Evaluated::Value(Value::String("y".to_string())),
            expect_value: Evaluated::Prompted(Prompted {
                value_type: a,
                prompt: "holding y".to_string(),
            }),
            input: case.input,
            expect: case.expect,
            input_text: case.input_text.clone(),
            expect_text: case.expect_text.clone(),
        };
        let mut tests = vec![one.clone(), one.clone(), LoweredTest { entity: None, ..one }];
        LoweredTest::number_repeats(&mut tests);
        let ids: Vec<&str> = tests.iter().map(|test| test.id.as_str()).collect();
        assert_eq!(ids, [&id, &format!("{id}:2"), &id]);
    }

    // @lfy def/interpret/data.lfy:LoweredNode
    // @lfy def/interpret/data.lfy:LoweredNode#LoweredNode:LoweredNode:e9deb0370846a9748fe95f5eb5ef3f67f823b463042c92f71770d400a1f499c9
    #[test]
    fn every_lowered_node_has_an_origin() {
        let model = bound("const c = 1;\n");
        let origin = somewhere(&model);
        let inner = model
            .node_ref(
                0,
                model.sources[0]
                    .tree
                    .root
                    .find(Statement::VariableDeclaration)
                    .expect("the declaration"),
            )
            .expect("the declaration has a reference");
        let folded = LoweredNode {
            rule: Entity::Statement(Statement::VariableDeclaration),
            children: Vec::new(),
            origin: inner,
            entity: None,
            value: Some(Evaluated::Value(Value::Number(1.0))),
            criteria: Vec::new(),
            tests: Vec::new(),
        };
        let root = LoweredNode {
            rule: Entity::File(FileRule::SourceFile),
            children: vec![LoweredChild::Node(folded.clone())],
            origin,
            entity: Some(model.file_entities[0]),
            value: None,
            criteria: Vec::new(),
            tests: Vec::new(),
        };
        // Every node reachable in a lowered tree maps back to a node of the parse tree.
        assert_eq!(root.origin, origin);
        assert_eq!(root.rule().identifier, "SourceFile");
        for node in root.nodes() {
            assert_eq!(node.origin, inner);
            assert_eq!(model.info(node.origin).rule, node.rule);
        }
        assert_eq!(root.nodes().count(), 1);
    }

    // @lfy def/interpret/data.lfy:LoweredNode
    // @lfy def/interpret/data.lfy:LoweredNode#LoweredNode:LoweredNode:e9deb0370846a9748fe95f5eb5ef3f67f823b463042c92f71770d400a1f499c9
    #[test]
    fn a_folded_lowered_node_holds_a_value_and_no_children() {
        let model = bound("const c = 1;\n");
        let origin = somewhere(&model);
        let token = model.sources[0].tree.tokens[0].clone();
        let folded = LoweredNode {
            rule: Entity::Statement(Statement::VariableDeclaration),
            children: Vec::new(),
            origin,
            entity: None,
            value: Some(Evaluated::Value(Value::Number(1.0))),
            criteria: Vec::new(),
            tests: Vec::new(),
        };
        assert!(folded.is_folded());
        assert!(folded.children.is_empty());
        // A node kept as written holds its children and no value.
        let kept = LoweredNode {
            children: vec![LoweredChild::Token(token.clone())],
            value: None,
            ..folded
        };
        assert!(!kept.is_folded());
        assert_eq!(kept.children[0].as_token(), Some(&token));
        assert_eq!(kept.children[0].as_node(), None);
    }
}
