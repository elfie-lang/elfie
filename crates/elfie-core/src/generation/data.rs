//! Compiled from `def/generation/data.lfy`: the data of generation.
//!
//! A [`Unit`] is one source file for one target, with any entities a [`Move`] brought into
//! it; a [`Batch`] is the units compiled together in one request; a [`Plan`] is every unit
//! of a workspace, the moves that resolved cycles among files, and how they are
//! batched; a [`Request`] is everything the compiler is handed for one batch; a
//! [`Verdict`] is whether one unit's [`Output`]s are accepted, and the [`SourceMap`]s they
//! earn; an [`Outcome`] is what one run of the compiler came to; a [`Change`] is one
//! difference for one entity since a unit's output was accepted; a [`ReviewRequest`] is
//! everything a verifier is handed, and a [`ReviewReport`] of [`Review`]s is what it
//! found. The `Target`, `File`,
//! `Unit`, `LoweredFile`, and `Entity` fields of the definition are kept as indices into
//! `Workspace::targets`, `Workspace::files`, `Plan::units`, `Program::files`, and
//! `Model::entities`, so that a plan is one plain value that does not own the workspace or
//! the program lowered from it.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value, json};

use crate::interpret::{LoweredCriterion, LoweredNode, LoweredTest, Program};
use crate::model::{Command, Criterion, EntityId, Knowledge, Problem};
use crate::workspace::NativeDependency;

/// Why a unit needs generating.
// @lfy def/generation/data.lfy:Reason
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// No output has been generated for it.
    Fresh, // @lfy def/generation/data.lfy:Reason.fresh
    /// Its source differs from what its output was generated from.
    Changed, // @lfy def/generation/data.lfy:Reason.changed
    /// Its criteria or tests differ from those its output was generated against, while its
    /// code does not.
    Requirements, // @lfy def/generation/data.lfy:Reason.requirements
    /// The interface of a unit it depends on differs from the one its output was generated
    /// against.
    Dependency, // @lfy def/generation/data.lfy:Reason.dependency
    /// A global criterion or test its output answers for was reviewed as violated.
    Violated, // @lfy def/generation/data.lfy:Reason.violated
    /// The caller asked for it regardless.
    Requested, // @lfy def/generation/data.lfy:Reason.requested
}

impl Reason {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            Reason::Fresh => "no output has been generated for it",
            Reason::Changed => "its source differs from what its output was generated from",
            Reason::Requirements => {
                "its criteria or tests differ from those its output was generated against, while its code does not"
            }
            Reason::Dependency => {
                "the interface of a unit it depends on differs from the one its output was generated against"
            }
            Reason::Violated => {
                "a global criterion or test its output answers for was reviewed as violated"
            }
            Reason::Requested => "the caller asked for it regardless",
        }
    }

    /// The member's name.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Fresh => "fresh",
            Reason::Changed => "changed",
            Reason::Requirements => "requirements",
            Reason::Dependency => "dependency",
            Reason::Violated => "violated",
            Reason::Requested => "requested",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One place where generated code says where it came from.
///
/// [`spelling`](Marker::spelling) writes one, [`parse_markers`](super::parse_markers) reads
/// one back, and [`covers`](Marker::covers) tells which output lines its region holds. A
/// marker carries no text of its own beyond a place and, where it answers for one, the id
/// in [`Marker::requirement`].
// @lfy def/generation/data.lfy:Marker
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The line of the output file that carries the marker, counting from 1.
    pub output_line: usize, // @lfy def/generation/data.lfy:Marker.outputLine
    /// The source path the marker names, relative to the workspace root.
    pub file: String, // @lfy def/generation/data.lfy:Marker.file
    /// The name the marker spells, when it names an entity: the identifier, or the owner's
    /// identifier, a dot, and a member's name; `None` when it names a line.
    pub entity: Option<String>, // @lfy def/generation/data.lfy:Marker.entity
    /// The source line: as spelled, or derived from [`Marker::entity`] as the first line of
    /// that entity's declaration.
    pub line: usize, // @lfy def/generation/data.lfy:Marker.line
    /// The source column, counting from 0; `None` when the marker names a whole line or an
    /// entity.
    pub column: Option<usize>, // @lfy def/generation/data.lfy:Marker.column
    /// The id of the criterion or test the region answers for, spelled after the entity and
    /// a `#`; a global id begins with `global`; `None` when it answers for none. The id is
    /// all a marker holds of it: there is no field for its text.
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:279cbe7d7cbb3cc50918fa7d94399832d2467d90ed0e930ad90edf50af91788a
    pub requirement: Option<String>, // @lfy def/generation/data.lfy:Marker.requirement
    /// The last output line of the region the marker begins: the line before the next
    /// marker in the same output, or the last line of the output.
    pub end: usize, // @lfy def/generation/data.lfy:Marker.end
}

impl Marker {
    /// The marker as it is spelled after the comment opener, in one of four forms:
    /// `@lfy <path>:<entity>`, `@lfy <path>:<entity>#<id>`, `@lfy <path>:<line>`, or
    /// `@lfy <path>:<line>:<column>`.
    // Decision: a requirement is spelled after the entity and a `#`, as `Marker.requirement`
    // says, so a marker that names a line spells none: the code a criterion or a test is
    // answered by belongs to an entity, which is what such a marker names.
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:76e237fb260caf5b11bc6d84be162349e39c76c149a14baf79eb7d16efa750a6
    pub fn spelling(&self) -> String {
        match (&self.entity, self.column) {
            // @lfy def/generation/data.lfy:Marker#Marker:Marker:a0a4e19d85bf94ae097f1675de74253ff9afc262809120446231dad567d2b141
            (Some(entity), _) => match &self.requirement {
                Some(requirement) => format!("@lfy {}:{entity}#{requirement}", self.file),
                None => format!("@lfy {}:{entity}", self.file),
            },
            (None, Some(column)) => format!("@lfy {}:{}:{column}", self.file, self.line),
            (None, None) => format!("@lfy {}:{}", self.file, self.line),
        }
    }

    /// Whether an output line falls in `output_line..=end`. A marker another marker shares
    /// its output line with has an end before both, so it covers nothing.
    // @lfy def/generation/data.lfy:Marker.end
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:e9418054b314dd678a5ad4c10c1b8a87c6937555b64a474b204585151415547f
    pub fn covers(&self, line: usize) -> bool {
        (self.output_line..=self.end).contains(&line)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "outputLine": self.output_line,
            "file": self.file,
            "entity": self.entity,
            "line": self.line,
            "column": self.column,
            "requirement": self.requirement,
            "end": self.end,
        })
    }

    pub fn from_json(value: &Value) -> Option<Marker> {
        let object = value.as_object()?;
        let output_line = usize_field(object, "outputLine")?;
        Some(Marker {
            output_line,
            file: string_field(object, "file")?,
            entity: string_field(object, "entity"),
            line: usize_field(object, "line")?,
            column: usize_field(object, "column"),
            // A marker read back without a requirement answers for none, as one written
            // before requirements were recorded did.
            requirement: string_field(object, "requirement"),
            // Decision: a marker read back without an end was written before ends were
            // recorded; its region is taken as its own line alone, which claims no line it
            // may not own and is never an inverted range.
            end: usize_field(object, "end").unwrap_or(output_line),
        })
    }
}

/// What one output file was generated from, recorded mechanically at acceptance in the map
/// file of its unit.
// @lfy def/generation/data.lfy:SourceMap
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    /// The identifier of the target it was generated for.
    pub target: String, // @lfy def/generation/data.lfy:SourceMap.target
    /// The output file's path, relative to the workspace root.
    pub output: String, // @lfy def/generation/data.lfy:SourceMap.output
    /// The source file's path, relative to the workspace root.
    pub source: String, // @lfy def/generation/data.lfy:SourceMap.source
    /// SHA-256 of [`Unit::text`] of the unit when the output was accepted, as lowercase
    /// hex, so an edit that leaves the lowered code the same, such as a comment, changes
    /// nothing.
    pub hash: String, // @lfy def/generation/data.lfy:SourceMap.hash
    /// SHA-256 of the ids of the unit's local criteria and tests, sorted and joined by line
    /// breaks, when the output was accepted.
    pub requirements: String, // @lfy def/generation/data.lfy:SourceMap.requirements
    /// SHA-256 of the unit's interface text, as `interfaceOf` spells it, when the output
    /// was accepted.
    pub signature: String, // @lfy def/generation/data.lfy:SourceMap.signature
    /// For each dependency's source path, the [`SourceMap::signature`] the output was
    /// generated against.
    pub dependencies: BTreeMap<String, String>, // @lfy def/generation/data.lfy:SourceMap.dependencies
    /// When the output was accepted: the clock at acceptance as an RFC 3339 timestamp, as
    /// [`now_rfc3339`](super::now_rfc3339) spells it.
    pub generated: String, // @lfy def/generation/data.lfy:SourceMap.generated
    /// Every marker in the output, in output order, with [`Marker::line`] and
    /// [`Marker::end`] derived.
    pub markers: Vec<Marker>, // @lfy def/generation/data.lfy:SourceMap.markers
}

impl SourceMap {
    pub fn to_json(&self) -> Value {
        let dependencies = self
            .dependencies
            .iter()
            .map(|(path, signature)| (path.clone(), Value::from(signature.clone())))
            .collect::<Map<_, _>>();
        json!({
            "target": self.target,
            "output": self.output,
            "source": self.source,
            "hash": self.hash,
            "requirements": self.requirements,
            "signature": self.signature,
            "dependencies": dependencies,
            "generated": self.generated,
            "markers": self.markers.iter().map(Marker::to_json).collect::<Vec<_>>(),
        })
    }

    /// A source map from its JSON object; `None` when a field is missing or mistyped.
    ///
    // Decision: the definition records a requirements hash, a signature, and the signatures
    // of the dependencies at acceptance, so each is always written; a source map read back
    // without them was written before they were recorded. Reading takes them as empty
    // rather than failing, which leaves the unit looking out of date against every current
    // hash and signature, so it is regenerated instead of silently kept.
    pub fn from_json(value: &Value) -> Option<SourceMap> {
        let object = value.as_object()?;
        let markers = match object.get("markers") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(Marker::from_json)
                .collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
        };
        let dependencies = match object.get("dependencies") {
            None | Some(Value::Null) => BTreeMap::new(),
            Some(Value::Object(entries)) => entries
                .iter()
                .map(|(path, signature)| {
                    signature
                        .as_str()
                        .map(|text| (path.clone(), text.to_string()))
                })
                .collect::<Option<BTreeMap<_, _>>>()?,
            Some(_) => return None,
        };
        Some(SourceMap {
            target: string_field(object, "target")?,
            output: string_field(object, "output")?,
            source: string_field(object, "source")?,
            hash: string_field(object, "hash")?,
            requirements: string_field(object, "requirements").unwrap_or_default(),
            signature: string_field(object, "signature").unwrap_or_default(),
            dependencies,
            generated: string_field(object, "generated")?,
            markers,
        })
    }
}

fn string_field(object: &Map<String, Value>, name: &str) -> Option<String> {
    object.get(name)?.as_str().map(str::to_string)
}

fn usize_field(object: &Map<String, Value>, name: &str) -> Option<usize> {
    object
        .get(name)?
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
}

/// The work the compiler does as one piece: one source file for one target, with any
/// entities moved into it from other files.
// @lfy def/generation/data.lfy:Unit
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// What it is built for, as an index into `Workspace::targets`.
    pub target: usize, // @lfy def/generation/data.lfy:Unit.target
    /// The source file, as an index into `Workspace::files`.
    pub file: usize, // @lfy def/generation/data.lfy:Unit.file
    /// The entities of the file built for the target that have a lowered node and were not
    /// moved out, in file order, then the entities moved into it, in the order of
    /// [`Plan::moves`], as indices into `Model::entities`; never an entity of the elfie
    /// package, a trait, or an ace function.
    pub entities: Vec<EntityId>, // @lfy def/generation/data.lfy:Unit.entities
    /// The file as lowering gave it, as an index into `Program::files`.
    pub lowered: usize, // @lfy def/generation/data.lfy:Unit.lowered
    /// The lowered code the unit compiles: the text of its lowered file without the code of
    /// the entities moved out of it, then the lowered code of each entity moved into it;
    /// exactly the text of its lowered file when nothing moved in or out.
    pub text: String, // @lfy def/generation/data.lfy:Unit.text
    /// The file's path relative to the directory it was found under, without its
    /// extension; the target's guidance spells the output file from it.
    pub stem: String, // @lfy def/generation/data.lfy:Unit.stem
    /// The units of the same target for the files this file uses, transitively through
    /// files that have no unit, and the units holding the entities its entities reach, as
    /// indices into [`Plan::units`].
    pub dependencies: Vec<usize>, // @lfy def/generation/data.lfy:Unit.dependencies
    /// The outputs the last accepted generation produced, from the unit's map file; empty
    /// when none.
    pub outputs: Vec<SourceMap>, // @lfy def/generation/data.lfy:Unit.outputs
    /// Why it is planned; `None` when it is up to date.
    pub reason: Option<Reason>, // @lfy def/generation/data.lfy:Unit.reason
}

impl Unit {
    /// The unit's local criteria and tests: for each of [`Unit::entities`], in that order,
    /// those of its declaration — its own and those of its members — criteria before tests,
    /// then those of its file's own entity.
    ///
    /// A move needs no further account here. [`Unit::entities`] already holds every entity
    /// moved into the unit and holds none that moved out, so the criteria and tests of a
    /// moved entity are the receiving unit's and no other unit's; the declaration of such an
    /// entity is in the file it was moved from, which is why one is looked for across the
    /// program rather than in the unit's own lowered file alone.
    // @lfy def/generation/data.lfy:Unit.entities
    // @lfy def/generation/data.lfy:Unit#Unit:Unit:a71462ec6c4c1494654b6067e77067cac5154c77a93576479d51af4fa014244e
    pub fn requirements(&self, program: &Program) -> Vec<Requirement> {
        let mut out: Vec<Requirement> = Vec::new();
        for &entity in &self.entities {
            // An entity with no lowered node, such as a trait or an ace function, is never
            // among a unit's entities, so a declaration is found for every one of them.
            if let Some(node) = declaration_of(program, self.lowered, entity) {
                requirements_of(node, &mut out);
            }
        }
        let file = &program.files[self.lowered];
        out.extend(file.criteria.iter().cloned().map(Requirement::Criterion));
        out.extend(file.tests.iter().cloned().map(Requirement::Test));
        out
    }
}

/// The lowered node that declares an entity, looked for in one file of the program first and
/// then in the rest; `None` when no lowered node declares it.
fn declaration_of(program: &Program, first: usize, entity: EntityId) -> Option<&LoweredNode> {
    std::iter::once(first)
        .chain((0..program.files.len()).filter(|&index| index != first))
        .find_map(|index| node_declaring(&program.files[index].root, entity))
}

/// The node of a lowered tree that declares an entity, the node itself before its children.
fn node_declaring(node: &LoweredNode, entity: EntityId) -> Option<&LoweredNode> {
    if node.entity == Some(entity) {
        return Some(node);
    }
    node.nodes().find_map(|child| node_declaring(child, entity))
}

/// The criteria and tests of one declaration, appended: those of the node, criteria before
/// tests, then those of the nodes within it, which are its members.
fn requirements_of(node: &LoweredNode, out: &mut Vec<Requirement>) {
    out.extend(node.criteria.iter().cloned().map(Requirement::Criterion));
    out.extend(node.tests.iter().cloned().map(Requirement::Test));
    for child in node.nodes() {
        requirements_of(child, out);
    }
}

/// Entities of one file compiled in another file's unit, because the files would otherwise
/// reach each other.
// Decision: a cycle among files is resolved by moving entities down into the lowest file of
// the cycle, the one the others already use, so the uses keep saying which file is lower and
// no output ever imports a file that imports it back, whatever the target's language allows.
// @lfy def/generation/data.lfy:Move
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    /// The entities moved, in the file order of the file that declares them, as indices into
    /// `Model::entities`.
    pub entities: Vec<EntityId>, // @lfy def/generation/data.lfy:Move.entities
    /// The unit of the file that declares them, as an index into [`Plan::units`].
    pub origin: usize, // @lfy def/generation/data.lfy:Move.origin
    /// The unit that compiles them: the unit of the lowest file of the cycle, as an index
    /// into [`Plan::units`].
    pub into: usize, // @lfy def/generation/data.lfy:Move.into
    /// The member, parameter, or output of an entity of [`Move::into`] whose type reaches
    /// the first of [`Move::entities`], spelled as the owner's identifier, a dot, and the
    /// member's or parameter's name, or the owner's identifier alone for an output.
    pub cause: String, // @lfy def/generation/data.lfy:Move.cause
}

/// Planned units compiled together in one request, so shared context is sent once.
// @lfy def/generation/data.lfy:Batch
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Batch {
    /// In plan order; every dependency of a unit is in this batch or an earlier one, as
    /// indices into [`Plan::units`].
    pub units: Vec<usize>, // @lfy def/generation/data.lfy:Batch.units
    /// The stem of the first unit, then the count when there are more, as the batch is
    /// named in requests and progress.
    pub identifier: String, // @lfy def/generation/data.lfy:Batch.identifier
}

/// What a dependent unit may rely on from another unit's output.
// @lfy def/generation/data.lfy:Interface
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    /// The unit, as an index into [`Plan::units`].
    pub unit: usize, // @lfy def/generation/data.lfy:Interface.unit
    /// The paths of its outputs.
    pub outputs: Vec<String>, // @lfy def/generation/data.lfy:Interface.outputs
    /// Its entities: each one's identifier, kind, definition, type, parameters and output
    /// as applicable, as indices into `Model::entities`; a type may name an entity of the
    /// elfie package by its identifier.
    pub entities: Vec<EntityId>, // @lfy def/generation/data.lfy:Interface.entities
}

/// One criterion or one test, as a request carries it: what the definition writes as
/// `LoweredCriterion | LoweredTest`. Each carries its own id.
// @lfy def/generation/data.lfy:Request.globals
#[derive(Debug, Clone, PartialEq)]
pub enum Requirement {
    Criterion(LoweredCriterion),
    Test(LoweredTest),
}

impl Requirement {
    /// The id of the criterion or test.
    // @lfy def/generation/data.lfy:Request.globals
    pub fn id(&self) -> &str {
        match self {
            Requirement::Criterion(criterion) => &criterion.id,
            Requirement::Test(test) => &test.id,
        }
    }
}

/// Everything the compiler is handed to produce the outputs of one batch.
// @lfy def/generation/data.lfy:Request
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The batch.
    pub batch: Batch, // @lfy def/generation/data.lfy:Request.batch
    /// The prompt: what to produce, where, the rules for producing it, what of the standard
    /// library is `builtin` and never generated, and how to report the outcome.
    pub instructions: String, // @lfy def/generation/data.lfy:Request.instructions
    /// [`Unit::text`] of each unit, by the file's path.
    pub sources: BTreeMap<String, String>, // @lfy def/generation/data.lfy:Request.sources
    /// The local criteria and tests of each unit, by the file's path, each with its id.
    pub requirements: BTreeMap<String, Vec<Requirement>>, // @lfy def/generation/data.lfy:Request.requirements
    /// Every global criterion and test of the program, each once with its id.
    pub globals: Vec<Requirement>, // @lfy def/generation/data.lfy:Request.globals
    /// The lowered text each unit's existing outputs were generated from, by the file's
    /// path; absent where unknown.
    pub previous: BTreeMap<String, String>, // @lfy def/generation/data.lfy:Request.previous
    /// The current text of each existing output of the batch, by path.
    pub existing: BTreeMap<String, String>, // @lfy def/generation/data.lfy:Request.existing
    /// One interface per dependency outside the batch, in dependency order, each once.
    pub interfaces: Vec<Interface>, // @lfy def/generation/data.lfy:Request.interfaces
    /// The criteria of the target's declaration: its own, then each of the target's layers
    /// in order, then the traits they extend, nearest first, each trait once, with template
    /// values rendered from the layers' arguments.
    pub guidance: Vec<Criterion>, // @lfy def/generation/data.lfy:Request.guidance
    /// What the generated code may require from the target's ecosystem: the target's native
    /// dependencies.
    pub native_dependencies: Vec<NativeDependency>, // @lfy def/generation/data.lfy:Request.nativeDependencies
    /// What the compiler is given to read: the target's knowledge, then the knowledge of
    /// each entity of the batch's units, then of each entity of [`Request::interfaces`],
    /// each item once.
    pub knowledge: Vec<Knowledge>, // @lfy def/generation/data.lfy:Request.knowledge
    /// The commands the compiler runs instead of knowing the tools: the target's commands.
    pub commands: Vec<Command>, // @lfy def/generation/data.lfy:Request.commands
}

/// Everything a verifier is handed to check outputs against what was asked: the local
/// criteria and tests of one batch, or every global one once.
// @lfy def/generation/data.lfy:ReviewRequest
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewRequest {
    /// The batch; `None` for the review of global criteria and tests.
    pub batch: Option<Batch>, // @lfy def/generation/data.lfy:ReviewRequest.batch
    /// The prompt: every criterion and test with its id and place, every region answering
    /// for each, and how to report.
    pub instructions: String, // @lfy def/generation/data.lfy:ReviewRequest.instructions
}

/// One file the compiler produced.
// @lfy def/generation/data.lfy:Output
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Relative to the workspace root.
    pub path: String, // @lfy def/generation/data.lfy:Output.path
    /// Its content.
    pub text: String, // @lfy def/generation/data.lfy:Output.text
}

/// How one run of the compiler ended.
// @lfy def/generation/data.lfy:OutcomeKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutcomeKind {
    /// Every unit of the batch was accepted.
    Accepted, // @lfy def/generation/data.lfy:OutcomeKind.accepted
    /// An output failed acceptance.
    Rejected, // @lfy def/generation/data.lfy:OutcomeKind.rejected
    /// The compiler reported it could not proceed.
    Blocked, // @lfy def/generation/data.lfy:OutcomeKind.blocked
    /// The compiler asked a question only a person can answer.
    Clarification, // @lfy def/generation/data.lfy:OutcomeKind.clarification
    /// The compiler command could not run, or exited without producing anything.
    Failed, // @lfy def/generation/data.lfy:OutcomeKind.failed
}

impl OutcomeKind {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            OutcomeKind::Accepted => "every unit of the batch was accepted",
            OutcomeKind::Rejected => "an output failed acceptance",
            OutcomeKind::Blocked => "the compiler reported it could not proceed",
            OutcomeKind::Clarification => "the compiler asked a question only a person can answer",
            OutcomeKind::Failed => {
                "the compiler command could not run, or exited without producing anything"
            }
        }
    }

    /// The member's name.
    pub fn as_str(self) -> &'static str {
        match self {
            OutcomeKind::Accepted => "accepted",
            OutcomeKind::Rejected => "rejected",
            OutcomeKind::Blocked => "blocked",
            OutcomeKind::Clarification => "clarification",
            OutcomeKind::Failed => "failed",
        }
    }
}

impl fmt::Display for OutcomeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one run of the compiler came to, read mechanically from its report and the
/// verdicts.
// @lfy def/generation/data.lfy:Outcome
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// How it ended.
    pub kind: OutcomeKind, // @lfy def/generation/data.lfy:Outcome.kind
    /// The compiler's reason or question, or the first problem; empty when accepted.
    pub message: String, // @lfy def/generation/data.lfy:Outcome.message
    /// One verdict per unit of the batch, in batch order; empty when the compiler never got
    /// that far.
    pub verdicts: Vec<Verdict>, // @lfy def/generation/data.lfy:Outcome.verdicts
}

/// Whether outputs are accepted for one unit.
// @lfy def/generation/data.lfy:Verdict
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Whether every check passed.
    pub accepted: bool, // @lfy def/generation/data.lfy:Verdict.accepted
    /// What failed and why, one per failure; empty when accepted.
    pub problems: Vec<String>, // @lfy def/generation/data.lfy:Verdict.problems
    /// One per accepted output; empty when rejected.
    pub source_maps: Vec<SourceMap>, // @lfy def/generation/data.lfy:Verdict.sourceMaps
    /// Each accepted output with its markers normalized to the name form, for the caller to
    /// write back; empty when rejected.
    pub outputs: Vec<Output>, // @lfy def/generation/data.lfy:Verdict.outputs
}

/// What differs for one entity between a unit's file as last accepted and as it is now.
// @lfy def/generation/data.lfy:ChangeKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    /// The entity is declared now and was not before.
    Added, // @lfy def/generation/data.lfy:ChangeKind.added
    /// The entity was declared before and is not now.
    Removed, // @lfy def/generation/data.lfy:ChangeKind.removed
    /// Its definition differs.
    Definition, // @lfy def/generation/data.lfy:ChangeKind.definition
    /// Its type differs, as `interfaceOf` spells it.
    DeclaredType, // @lfy def/generation/data.lfy:ChangeKind.declaredType
    /// Its parameters or its output differ.
    Signature, // @lfy def/generation/data.lfy:ChangeKind.signature
    /// Its criteria differ in count or in the text of any.
    Criteria, // @lfy def/generation/data.lfy:ChangeKind.criteria
    /// Its tests differ.
    Tests, // @lfy def/generation/data.lfy:ChangeKind.tests
    /// Anything else in the text of its declaration differs.
    Body, // @lfy def/generation/data.lfy:ChangeKind.body
}

impl ChangeKind {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            ChangeKind::Added => "the entity is declared now and was not before",
            ChangeKind::Removed => "the entity was declared before and is not now",
            ChangeKind::Definition => "its definition differs",
            ChangeKind::DeclaredType => "its type differs, as interfaceOf spells it",
            ChangeKind::Signature => "its parameters or its output differ",
            ChangeKind::Criteria => "its criteria differ in count or in the text of any",
            ChangeKind::Tests => "its tests differ",
            ChangeKind::Body => "anything else in the text of its declaration differs",
        }
    }

    /// The member's name.
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Removed => "removed",
            ChangeKind::Definition => "definition",
            ChangeKind::DeclaredType => "declaredType",
            ChangeKind::Signature => "signature",
            ChangeKind::Criteria => "criteria",
            ChangeKind::Tests => "tests",
            ChangeKind::Body => "body",
        }
    }
}

impl fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One difference for one entity between a unit's file as last accepted and as it is now.
// @lfy def/generation/data.lfy:Change
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The identifier, or the owner's identifier, a dot, and a member's name.
    pub entity: String, // @lfy def/generation/data.lfy:Change.entity
    /// What differs.
    pub kind: ChangeKind, // @lfy def/generation/data.lfy:Change.kind
    /// What changed, in one line, quoting the old and the new when they are short.
    pub detail: String, // @lfy def/generation/data.lfy:Change.detail
}

/// What a verifier found for one criterion or test.
// @lfy def/generation/data.lfy:ReviewStatus
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewStatus {
    /// The output does what it says.
    Satisfied, // @lfy def/generation/data.lfy:ReviewStatus.satisfied
    /// The output does something else.
    Violated, // @lfy def/generation/data.lfy:ReviewStatus.violated
    /// No output can be checked against it.
    Unverifiable, // @lfy def/generation/data.lfy:ReviewStatus.unverifiable
}

impl ReviewStatus {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            ReviewStatus::Satisfied => "the output does what it says",
            ReviewStatus::Violated => "the output does something else",
            ReviewStatus::Unverifiable => "no output can be checked against it",
        }
    }

    /// The member's name.
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewStatus::Satisfied => "satisfied",
            ReviewStatus::Violated => "violated",
            ReviewStatus::Unverifiable => "unverifiable",
        }
    }

    /// The member of this name; `None` when it is no member's name.
    pub fn from_name(name: &str) -> Option<ReviewStatus> {
        match name {
            "satisfied" => Some(ReviewStatus::Satisfied),
            "violated" => Some(ReviewStatus::Violated),
            "unverifiable" => Some(ReviewStatus::Unverifiable),
            _ => None,
        }
    }
}

impl fmt::Display for ReviewStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a verifier's report ends with, on a line of its own.
// @lfy def/generation/data.lfy:Review
pub const REVIEWED: &str = "ELFIE: REVIEWED";

/// The name a review's entity takes when its id names a global criterion or test.
// @lfy def/generation/data.lfy:Review.entity
pub const GLOBAL: &str = "global";

/// A verifier's finding for one criterion or test.
///
/// [`Review::to_json`] writes the object a verifier puts on one line and
/// [`Review::from_json`] reads it back; [`Review::at`] fills the place, which no verifier
/// writes.
// Decision: a review names its criterion or test by id and quotes nothing; its file, line,
// and entity are derived from the id's origin, so a verifier never has to spell a place
// right.
// @lfy def/generation/data.lfy:Review
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// The id of the criterion or test it answers for.
    pub id: String, // @lfy def/generation/data.lfy:Review.id
    /// The definition file the criterion or test is written in, relative to the workspace
    /// root.
    pub file: String, // @lfy def/generation/data.lfy:Review.file
    /// The line the criterion or test begins on.
    pub line: usize, // @lfy def/generation/data.lfy:Review.line
    /// The identifier, or the owner's identifier, a dot, and a member's name, of the entity
    /// it belongs to.
    pub entity: String, // @lfy def/generation/data.lfy:Review.entity
    /// What was found.
    pub status: ReviewStatus, // @lfy def/generation/data.lfy:Review.status
    /// The output path and line range it was checked against, such as
    /// `crates/elfie-core/src/x.rs:120-134`; empty when none.
    pub evidence: String, // @lfy def/generation/data.lfy:Review.evidence
    /// Why, in one line; for a violated one, what the code does instead.
    pub note: String, // @lfy def/generation/data.lfy:Review.note
}

impl Review {
    /// The keys a review is written with, and no others.
    // @lfy def/generation/data.lfy:Review#Review:Review:c6583b64878c8040bf8c20785e6890e5beb1c759c27805f2ef4728e16054e2a9
    const KEYS: [&'static str; 4] = ["id", "status", "evidence", "note"];

    /// The review as the one JSON object a verifier writes on one line. The place is not
    /// written: it is derived from the id, never carried by it.
    // @lfy def/generation/data.lfy:Review
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "status": self.status.as_str(),
            "evidence": self.evidence,
            "note": self.note,
        })
    }

    /// A review from what a verifier wrote; `None` unless the value is an object with
    /// exactly those keys and a status naming a member of [`ReviewStatus`]. The file, the
    /// line, and the entity are left empty, since they are no verifier's to write;
    /// [`Review::at`] fills them from the origin of the criterion or test the id names.
    // @lfy def/generation/data.lfy:Review
    pub fn from_json(value: &Value) -> Option<Review> {
        let object = value.as_object()?;
        if object.len() != Review::KEYS.len()
            || !Review::KEYS.iter().all(|key| object.contains_key(*key))
        {
            return None;
        }
        Some(Review {
            id: string_field(object, "id")?,
            file: String::new(),
            line: 0,
            entity: String::new(),
            status: ReviewStatus::from_name(&string_field(object, "status")?)?,
            evidence: string_field(object, "evidence")?,
            note: string_field(object, "note")?,
        })
    }

    /// Whether an id names a global criterion or test: one whose receiver is [`GLOBAL`].
    // @lfy def/generation/data.lfy:Review.id
    pub fn is_global(id: &str) -> bool {
        id.strip_prefix(GLOBAL).is_some_and(|rest| {
            // The receiver is followed by a colon and the contributor, so `globalThing` is
            // an entity of its own and not the global receiver.
            rest.starts_with(':')
        })
    }

    /// The review with its place taken from the origin of the criterion or test its id
    /// names: the definition file, the line, and the entity's name — [`GLOBAL`] when the id
    /// names a global one. Whatever the verifier wrote for any of the three is replaced.
    // @lfy def/generation/data.lfy:Review.file
    // @lfy def/generation/data.lfy:Review.line
    // @lfy def/generation/data.lfy:Review#Review:Review:8efe3a858a147e4da4fba7946604c05bdf838e9f5318a42100838db3688ed4d3
    pub fn at(self, file: &str, line: usize, entity: &str) -> Review {
        let global = Review::is_global(&self.id);
        Review {
            file: file.to_string(),
            line,
            // @lfy def/generation/data.lfy:Review#Review:Review:77abd40de86a4997a885c35d8dd48756371552411751998bc6c1c30b0569c6a6
            entity: if global {
                GLOBAL.to_string()
            } else {
                entity.to_string()
            },
            ..self
        }
    }
}

/// Everything a verifier found for one batch, read mechanically from its report.
// @lfy def/generation/data.lfy:ReviewReport
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewReport {
    /// Each review, in report order, one per id.
    pub reviews: Vec<Review>, // @lfy def/generation/data.lfy:ReviewReport.reviews
    /// The lines of the verifier's output that were neither a review nor the end line.
    pub problems: Vec<String>, // @lfy def/generation/data.lfy:ReviewReport.problems
}

/// Every unit of a workspace, which need generating, and how they are batched.
// @lfy def/generation/data.lfy:Plan
// Decision: the definition gives the plan its workspace and the program lowered from it; a
// plan here owns neither (the workspace holds the whole model, and the program holds the
// workspace), so `request` and `accept` take the workspace the plan was made from as their
// first parameter instead, and a unit names its lowered file by its index in
// `Program::files`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Every unit of every target, each after its dependencies.
    pub units: Vec<Unit>, // @lfy def/generation/data.lfy:Plan.units
    /// Every move that resolved a cycle among files, in the order the moves were made.
    pub moves: Vec<Move>, // @lfy def/generation/data.lfy:Plan.moves
    /// Every cycle among files that no move could resolve, and every entity that reaches one
    /// not built for its target, one problem each.
    pub problems: Vec<Problem>, // @lfy def/generation/data.lfy:Plan.problems
    /// The planned units grouped for compilation, each batch after the batches holding its
    /// dependencies.
    pub batches: Vec<Batch>, // @lfy def/generation/data.lfy:Plan.batches
}

impl Plan {
    /// The unit of a file for a target, when there is one.
    pub fn unit_of(&self, target: usize, file: usize) -> Option<usize> {
        self.units
            .iter()
            .position(|unit| unit.target == target && unit.file == file)
    }

    /// Every unit that has a reason, in plan order.
    pub fn planned(&self) -> impl Iterator<Item = (usize, &Unit)> {
        self.units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.reason.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Decision: the spellings are of a file the program does not hold, so that the text of
    // this test is a fixture rather than a claim about `def/generation/data.lfy`.
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:76e237fb260caf5b11bc6d84be162349e39c76c149a14baf79eb7d16efa750a6
    #[test]
    fn marker_spells_a_line_a_column_or_an_entity() {
        let line = Marker {
            output_line: 3,
            file: "def/a.lfy".to_string(),
            entity: None,
            line: 11,
            column: None,
            requirement: None,
            end: 7,
        };
        assert_eq!(line.spelling(), "@lfy def/a.lfy:11");

        let column = Marker {
            column: Some(2),
            ..line.clone()
        };
        assert_eq!(column.spelling(), "@lfy def/a.lfy:11:2");

        let entity = Marker {
            entity: Some("Marker.file".to_string()),
            ..column
        };
        assert_eq!(entity.spelling(), "@lfy def/a.lfy:Marker.file");
    }

    /// The name form carries no place of its own, so no line a compiler wrote can reach a
    /// source map: a marker naming an entity spells the name alone, whatever
    /// [`Marker::line`] and [`Marker::column`] hold, and the line recorded is derived from
    /// the model by [`accept`](super::accept).
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:6dfd2385181861e88df13c277e13d237ad94dc3f284515619561d5e7c6488ce2
    #[test]
    fn a_marker_that_names_an_entity_spells_no_line() {
        let named = Marker {
            output_line: 3,
            file: "def/a.lfy".to_string(),
            entity: Some("Marker.file".to_string()),
            line: 11,
            column: Some(2),
            requirement: None,
            end: 7,
        };
        assert_eq!(named.spelling(), "@lfy def/a.lfy:Marker.file");
        assert!(!named.spelling().contains("11"));
        assert!(!named.spelling().contains(":2"));
    }

    // @lfy def/generation/data.lfy:Marker.requirement
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:a0a4e19d85bf94ae097f1675de74253ff9afc262809120446231dad567d2b141
    #[test]
    fn a_marker_for_a_requirement_names_the_entity_and_the_id() {
        let local = Marker {
            output_line: 12,
            file: "def/a.lfy".to_string(),
            entity: Some("Marker".to_string()),
            line: 11,
            column: None,
            requirement: Some("Marker:Marker:abc".to_string()),
            end: 20,
        };
        assert_eq!(local.spelling(), "@lfy def/a.lfy:Marker#Marker:Marker:abc");

        let global = Marker {
            requirement: Some("global:target:def".to_string()),
            ..local.clone()
        };
        assert_eq!(global.spelling(), "@lfy def/a.lfy:Marker#global:target:def");

        // A marker holds the id and nothing else of the criterion: no key of one carries
        // its text, and the spelling is the place and the id alone.
        // @lfy def/generation/data.lfy:Marker#Marker:Marker:279cbe7d7cbb3cc50918fa7d94399832d2467d90ed0e930ad90edf50af91788a
        let mut keys: Vec<String> = local
            .to_json()
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            ["column", "end", "entity", "file", "line", "outputLine", "requirement"]
        );
        assert_eq!(Marker::from_json(&local.to_json()), Some(local));
    }

    /// A marker survives the round trip through the JSON of a source map.
    // @lfy def/generation/data.lfy:SourceMap.markers
    #[test]
    fn source_map_round_trips_through_json() {
        let map = SourceMap {
            target: "rust".to_string(),
            output: "crates/elfie-core/src/generation/data.rs".to_string(),
            source: "def/generation/data.lfy".to_string(),
            hash: "abc".to_string(),
            requirements: "jkl".to_string(),
            signature: "def".to_string(),
            dependencies: BTreeMap::from([("def/model/data.lfy".to_string(), "ghi".to_string())]),
            generated: "2026-09-19T00:00:00Z".to_string(),
            markers: vec![Marker {
                output_line: 1,
                file: "def/generation/data.lfy".to_string(),
                entity: Some("Marker".to_string()),
                line: 11,
                column: None,
                requirement: Some("Marker:Marker:abc".to_string()),
                end: 24,
            }],
        };
        assert_eq!(SourceMap::from_json(&map.to_json()), Some(map));
    }

    // @lfy def/generation/data.lfy:Marker.end
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:e9418054b314dd678a5ad4c10c1b8a87c6937555b64a474b204585151415547f
    #[test]
    fn a_marker_covers_its_output_line_through_its_end() {
        let marker = Marker {
            output_line: 3,
            file: "def/a.lfy".to_string(),
            entity: Some("A".to_string()),
            line: 11,
            column: None,
            requirement: None,
            end: 5,
        };
        assert!(!marker.covers(2));
        assert!(marker.covers(3));
        assert!(marker.covers(5));
        assert!(!marker.covers(6));

        let shared = Marker { end: 2, ..marker };
        assert!(!shared.covers(2));
        assert!(!shared.covers(3));
    }

    /// A marker read back from a source map written before ends were recorded covers its
    /// own line alone.
    // @lfy def/generation/data.lfy:Marker.end
    #[test]
    fn a_marker_without_an_end_reads_back_as_its_own_line() {
        let value = json!({
            "outputLine": 7,
            "file": "def/a.lfy",
            "entity": "A",
            "line": 11,
            "column": null,
        });
        let marker = Marker::from_json(&value).expect("a marker without an end reads");
        assert_eq!(marker.end, 7);
        assert!(marker.covers(7));
    }

    // @lfy def/generation/data.lfy:Review#Review:Review:c6583b64878c8040bf8c20785e6890e5beb1c759c27805f2ef4728e16054e2a9
    #[test]
    fn a_review_is_one_json_object_with_exactly_its_keys() {
        assert_eq!(REVIEWED, "ELFIE: REVIEWED");

        let review = Review {
            id: "Review:Review:abc".to_string(),
            file: String::new(),
            line: 0,
            entity: String::new(),
            status: ReviewStatus::Violated,
            evidence: "crates/elfie-core/src/x.rs:120-134".to_string(),
            note: "it writes two objects on one line".to_string(),
        };
        let value = review.to_json();
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["evidence", "id", "note", "status"]);
        assert_eq!(serde_json::to_string(&value).unwrap().lines().count(), 1);
        assert_eq!(Review::from_json(&value), Some(review));

        // An object with a key too many, one too few, or a status that names no member is
        // no review.
        let mut extra = value.clone();
        extra.as_object_mut().unwrap().insert("why".to_string(), Value::from("x"));
        assert_eq!(Review::from_json(&extra), None);
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("note");
        assert_eq!(Review::from_json(&missing), None);
        let mut unknown = value.clone();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("status".to_string(), Value::from("maybe"));
        assert_eq!(Review::from_json(&unknown), None);
        assert_eq!(ReviewStatus::from_name("satisfied"), Some(ReviewStatus::Satisfied));
    }

    // @lfy def/generation/data.lfy:Review.file
    // @lfy def/generation/data.lfy:Review#Review:Review:8efe3a858a147e4da4fba7946604c05bdf838e9f5318a42100838db3688ed4d3
    #[test]
    fn a_reviews_place_is_derived_from_its_origin_and_never_read() {
        // A verifier that spells a place of its own writes no review at all: the keys are
        // exactly id, status, evidence, and note.
        let spelled = json!({
            "id": "Marker:Marker:abc",
            "file": "def/other.lfy",
            "line": 7,
            "entity": "Other",
            "status": "satisfied",
            "evidence": "",
            "note": "",
        });
        assert_eq!(Review::from_json(&spelled), None);

        let read = Review::from_json(&json!({
            "id": "Marker:Marker:abc",
            "status": "satisfied",
            "evidence": "crates/elfie-core/src/generation/data.rs:120-134",
            "note": "the region spells the marker",
        }))
        .expect("a review of four keys reads");
        assert_eq!((read.file.as_str(), read.line, read.entity.as_str()), ("", 0, ""));

        let placed = read.at("def/generation/data.lfy", 42, "Marker");
        assert_eq!(placed.file, "def/generation/data.lfy");
        assert_eq!(placed.line, 42);
        assert_eq!(placed.entity, "Marker");
        assert!(!Review::is_global(&placed.id));
    }

    // @lfy def/generation/data.lfy:Review.entity
    // @lfy def/generation/data.lfy:Review#Review:Review:77abd40de86a4997a885c35d8dd48756371552411751998bc6c1c30b0569c6a6
    #[test]
    fn a_review_of_a_global_requirement_belongs_to_global() {
        assert_eq!(GLOBAL, "global");
        assert!(Review::is_global("global:target:abc"));
        // An entity whose own name begins with `global` is no global receiver.
        assert!(!Review::is_global("globalDocumentation:target:abc"));

        let review = Review::from_json(&json!({
            "id": "global:target:abc",
            "status": "violated",
            "evidence": "crates/elfie-core/src/generation/data.rs:1-9",
            "note": "the output is written outside the output directory",
        }))
        .expect("a review of four keys reads")
        .at("def/target/target.lfy", 18, "Marker");
        assert_eq!(review.file, "def/target/target.lfy");
        assert_eq!(review.line, 18);
        assert_eq!(review.entity, GLOBAL);
    }

    /// A source map written before a requirements hash and signatures were recorded reads
    /// back with none, so the unit looks out of date against every current one.
    // @lfy def/generation/data.lfy:SourceMap.signature
    #[test]
    fn source_map_without_a_signature_reads_back_empty() {
        let value = json!({
            "target": "rust",
            "output": "crates/elfie-core/src/generation/data.rs",
            "source": "def/generation/data.lfy",
            "hash": "abc",
            "generated": "2026-09-19T00:00:00Z",
            "markers": [],
        });
        let map = SourceMap::from_json(&value).expect("a source map without a signature reads");
        assert_eq!(map.signature, "");
        assert_eq!(map.requirements, ""); // @lfy def/generation/data.lfy:SourceMap.requirements
        assert!(map.dependencies.is_empty());
    }
}
