//! Compiled from `def/generation/data.lfy`: the data of generation.
//!
//! A [`Unit`] is one source file for one target; a [`Batch`] is the units compiled
//! together in one request; a [`Plan`] is every unit of a workspace and how they are
//! batched; a [`Request`] is everything the compiler is handed for one batch; a
//! [`Verdict`] is whether one unit's [`Output`]s are accepted, and the [`SourceMap`]s they
//! earn; an [`Outcome`] is what one run of the compiler came to. The `Target`, `File`,
//! `Unit`, and `Entity` fields of the definition are kept as indices into
//! `Workspace::targets`, `Workspace::files`, `Plan::units`, and `Model::entities`, so that
//! a plan is one plain value that does not own the workspace.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value, json};

use crate::model::{Criterion, EntityId};
use crate::workspace::NativeDependency;

/// Why a unit needs generating.
// @lfy def/generation/data.lfy:Reason
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// No output has been generated for it.
    Fresh, // @lfy def/generation/data.lfy:Reason.fresh
    /// Its source differs from what its output was generated from.
    Changed, // @lfy def/generation/data.lfy:Reason.changed
    /// The interface of a unit it depends on differs from the one its output was generated
    /// against.
    Dependency, // @lfy def/generation/data.lfy:Reason.dependency
    /// The caller asked for it regardless.
    Requested, // @lfy def/generation/data.lfy:Reason.requested
}

impl Reason {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            Reason::Fresh => "no output has been generated for it",
            Reason::Changed => "its source differs from what its output was generated from",
            Reason::Dependency => {
                "the interface of a unit it depends on differs from the one its output was generated against"
            }
            Reason::Requested => "the caller asked for it regardless",
        }
    }

    /// The member's name.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Fresh => "fresh",
            Reason::Changed => "changed",
            Reason::Dependency => "dependency",
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
/// A marker is written in the output as a line comment of the target's language reading
/// `@lfy`, a space, the source path relative to the workspace root, a colon, and then
/// either a line (optionally a colon and a column) or the name of an entity of that file.
/// Text is read as a marker only when what follows is a path ending in `.lfy`, a colon,
/// and a number or an identifier path; anything else after `@lfy` is prose, as
/// [`parse_markers`](super::parse_markers) reads it. The compiler is asked to write names,
/// never lines, so that its markers survive edits that move lines; [`Marker::line`] is
/// derived from the model, never copied from the compiler.
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
}

impl Marker {
    /// The marker as it is spelled after the comment opener: `@lfy path:entity`, or
    /// `@lfy path:line[:column]` when it names no entity.
    // @lfy def/generation/data.lfy:Marker
    pub fn spelling(&self) -> String {
        match (&self.entity, self.column) {
            (Some(entity), _) => format!("@lfy {}:{entity}", self.file),
            (None, Some(column)) => format!("@lfy {}:{}:{column}", self.file, self.line),
            (None, None) => format!("@lfy {}:{}", self.file, self.line),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "outputLine": self.output_line,
            "file": self.file,
            "entity": self.entity,
            "line": self.line,
            "column": self.column,
        })
    }

    pub fn from_json(value: &Value) -> Option<Marker> {
        let object = value.as_object()?;
        Some(Marker {
            output_line: usize_field(object, "outputLine")?,
            file: string_field(object, "file")?,
            entity: string_field(object, "entity"),
            line: usize_field(object, "line")?,
            column: usize_field(object, "column"),
        })
    }
}

/// What one output file was generated from, recorded mechanically at acceptance.
// @lfy def/generation/data.lfy:SourceMap
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    /// The identifier of the target it was generated for.
    pub target: String, // @lfy def/generation/data.lfy:SourceMap.target
    /// The output file's path, relative to the workspace root.
    pub output: String, // @lfy def/generation/data.lfy:SourceMap.output
    /// The source file's path, relative to the workspace root.
    pub source: String, // @lfy def/generation/data.lfy:SourceMap.source
    /// SHA-256 of the source file's bytes when the output was accepted, as lowercase hex.
    pub hash: String, // @lfy def/generation/data.lfy:SourceMap.hash
    /// SHA-256 of the unit's interface text, as `interfaceOf` spells it, when the output
    /// was accepted.
    pub signature: String, // @lfy def/generation/data.lfy:SourceMap.signature
    /// For each dependency's source path, the [`SourceMap::signature`] the output was
    /// generated against.
    pub dependencies: BTreeMap<String, String>, // @lfy def/generation/data.lfy:SourceMap.dependencies
    /// When the output was accepted: the clock at acceptance as an RFC 3339 timestamp, as
    /// [`now_rfc3339`](super::now_rfc3339) spells it.
    pub generated: String, // @lfy def/generation/data.lfy:SourceMap.generated
    /// Every marker in the output, in output order, with lines derived.
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
            "signature": self.signature,
            "dependencies": dependencies,
            "generated": self.generated,
            "markers": self.markers.iter().map(Marker::to_json).collect::<Vec<_>>(),
        })
    }

    /// A source map from its JSON object; `None` when a field is missing or mistyped.
    ///
    // Decision: the definition records a signature and the signatures of the dependencies
    // at acceptance, so one is always written; a source map read back without them was
    // written before they were recorded. Reading takes them as empty rather than failing,
    // which leaves the unit looking out of date against every current signature, so it is
    // regenerated instead of silently kept.
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

/// The work the compiler does as one piece: one source file for one target.
// @lfy def/generation/data.lfy:Unit
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// What it is built for, as an index into `Workspace::targets`.
    pub target: usize, // @lfy def/generation/data.lfy:Unit.target
    /// The source file, as an index into `Workspace::files`.
    pub file: usize, // @lfy def/generation/data.lfy:Unit.file
    /// The entities of the file built for the target, in file order; never an entity of the
    /// elfie package.
    pub entities: Vec<EntityId>, // @lfy def/generation/data.lfy:Unit.entities
    /// The file's path relative to the directory it was found under, without its
    /// extension; the target's guidance spells the output file from it.
    pub stem: String, // @lfy def/generation/data.lfy:Unit.stem
    /// The units of the same target for the files this file uses, transitively through
    /// files that have no unit, as indices into [`Plan::units`].
    pub dependencies: Vec<usize>, // @lfy def/generation/data.lfy:Unit.dependencies
    /// The outputs the last accepted generation produced, from the target's source maps;
    /// empty when none.
    pub outputs: Vec<SourceMap>, // @lfy def/generation/data.lfy:Unit.outputs
    /// Why it is planned; `None` when it is up to date.
    pub reason: Option<Reason>, // @lfy def/generation/data.lfy:Unit.reason
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

/// Everything the compiler is handed to produce the outputs of one batch.
// @lfy def/generation/data.lfy:Request
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The batch.
    pub batch: Batch, // @lfy def/generation/data.lfy:Request.batch
    /// The prompt: what to produce, where, the rules for producing it, what of the standard
    /// library is `builtin` and never generated, and how to report the outcome.
    pub instructions: String, // @lfy def/generation/data.lfy:Request.instructions
    /// The text of each unit's source file, by the file's path.
    pub sources: BTreeMap<String, String>, // @lfy def/generation/data.lfy:Request.sources
    /// The source text each unit's existing outputs were generated from, by the file's
    /// path; absent where unknown.
    pub previous: BTreeMap<String, String>, // @lfy def/generation/data.lfy:Request.previous
    /// The current text of each existing output of the batch, by path.
    pub existing: BTreeMap<String, String>, // @lfy def/generation/data.lfy:Request.existing
    /// One interface per dependency outside the batch, in dependency order, each once.
    pub interfaces: Vec<Interface>, // @lfy def/generation/data.lfy:Request.interfaces
    /// Every criterion of the target's marker, then of every trait it extends down to
    /// `target`, nearest first, and of every trait applied to it with arguments, each
    /// resolved for the marker.
    pub guidance: Vec<Criterion>, // @lfy def/generation/data.lfy:Request.guidance
    /// What the generated code may require from the target's ecosystem.
    pub native_dependencies: Vec<NativeDependency>, // @lfy def/generation/data.lfy:Request.nativeDependencies
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

/// Every unit of a workspace, which need generating, and how they are batched.
// @lfy def/generation/data.lfy:Plan
// Decision: the definition gives the plan its workspace; a plan here does not own the
// workspace (which holds the whole model), so `request` and `accept` take the workspace
// the plan was made from as their first parameter instead.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Every unit of every target, each after its dependencies.
    pub units: Vec<Unit>, // @lfy def/generation/data.lfy:Plan.units
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

    /// A marker names either a line, optionally with a column, or an entity of its file.
    ///
    // Decision: the spellings are of a file the program does not hold, so that the text of
    // this test is a fixture rather than a claim about `def/generation/data.lfy`.
    // @lfy def/generation/data.lfy:Marker
    #[test]
    fn marker_spells_a_line_a_column_or_an_entity() {
        let line = Marker {
            output_line: 3,
            file: "def/a.lfy".to_string(),
            entity: None,
            line: 11,
            column: None,
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

    /// A marker survives the round trip through the JSON of a source map.
    // @lfy def/generation/data.lfy:SourceMap.markers
    #[test]
    fn source_map_round_trips_through_json() {
        let map = SourceMap {
            target: "rust".to_string(),
            output: "crates/elfie-core/src/generation/data.rs".to_string(),
            source: "def/generation/data.lfy".to_string(),
            hash: "abc".to_string(),
            signature: "def".to_string(),
            dependencies: BTreeMap::from([("def/model/data.lfy".to_string(), "ghi".to_string())]),
            generated: "2026-09-19T00:00:00Z".to_string(),
            markers: vec![Marker {
                output_line: 1,
                file: "def/generation/data.lfy".to_string(),
                entity: Some("Marker".to_string()),
                line: 11,
                column: None,
            }],
        };
        assert_eq!(SourceMap::from_json(&map.to_json()), Some(map));
    }

    /// A source map written before signatures were recorded reads back with none, so the
    /// unit looks out of date against every current signature.
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
        assert!(map.dependencies.is_empty());
    }
}
