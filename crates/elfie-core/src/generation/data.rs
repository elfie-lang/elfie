//! Compiled from `def/generation/data.lfy`: the data of generation.
//!
//! A [`Unit`] is one source file for one target; a [`Plan`] is every unit of a workspace;
//! a [`Request`] is everything the compiler is handed for one unit; a [`Verdict`] is
//! whether its [`Output`]s are accepted, and the [`SourceMap`]s they earn. The `Target`,
//! `File`, `Unit`, and `Entity` fields of the definition are kept as indices into
//! `Workspace::targets`, `Workspace::files`, `Plan::units`, and `Model::entities`, so
//! that a plan is one plain value that does not own the workspace.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value, json};

use crate::model::{Criterion, EntityId};
use crate::workspace::NativeDependency;

/// Why a unit needs generating.
// @lfy def/generation/data.lfy:4
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// No output has been generated for it.
    Fresh, // @lfy def/generation/data.lfy:5
    /// Its source differs from what its output was generated from.
    Changed, // @lfy def/generation/data.lfy:6
    /// The output of a unit it depends on was generated after its own.
    Dependency, // @lfy def/generation/data.lfy:7
    /// The caller asked for it regardless.
    Requested, // @lfy def/generation/data.lfy:8
}

impl Reason {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            Reason::Fresh => "no output has been generated for it",
            Reason::Changed => "its source differs from what its output was generated from",
            Reason::Dependency => "the output of a unit it depends on was generated after its own",
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
/// `@lfy`, a space, the source path relative to the workspace root, a colon, the line, and
/// optionally a colon and the column.
// @lfy def/generation/data.lfy:11
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The line of the output file that carries the marker, counting from 1.
    pub output_line: usize, // @lfy def/generation/data.lfy:12
    /// The source path the marker names, relative to the workspace root.
    // Decision: the definition leaves the path implicit in how a marker is spelled; it is
    // kept on the marker so that `accept` can check which file a marker names.
    pub file: String, // @lfy def/generation/data.lfy:16
    /// The source line, counting from 1.
    pub line: usize, // @lfy def/generation/data.lfy:13
    /// The source column, counting from 0; `None` when the marker names a whole line.
    pub column: Option<usize>, // @lfy def/generation/data.lfy:14
}

impl Marker {
    /// The marker as it is spelled after the comment opener: `@lfy path:line[:column]`.
    // @lfy def/generation/data.lfy:16
    pub fn spelling(&self) -> String {
        match self.column {
            Some(column) => format!("@lfy {}:{}:{column}", self.file, self.line),
            None => format!("@lfy {}:{}", self.file, self.line),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "outputLine": self.output_line,
            "file": self.file,
            "line": self.line,
            "column": self.column,
        })
    }

    pub fn from_json(value: &Value) -> Option<Marker> {
        let object = value.as_object()?;
        Some(Marker {
            output_line: usize_field(object, "outputLine")?,
            file: string_field(object, "file")?,
            line: usize_field(object, "line")?,
            column: usize_field(object, "column"),
        })
    }
}

/// What one output file was generated from.
// @lfy def/generation/data.lfy:19
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    /// The identifier of the target it was generated for.
    pub target: String, // @lfy def/generation/data.lfy:20
    /// The output file's path, relative to the workspace root.
    pub output: String, // @lfy def/generation/data.lfy:21
    /// The source file's path, relative to the workspace root.
    pub source: String, // @lfy def/generation/data.lfy:22
    /// SHA-256 of the source file's bytes when the output was generated, as lowercase hex.
    pub hash: String, // @lfy def/generation/data.lfy:23
    /// When the output was accepted, as an RFC 3339 timestamp.
    pub generated: String, // @lfy def/generation/data.lfy:24
    /// Every marker in the output, in output order.
    pub markers: Vec<Marker>, // @lfy def/generation/data.lfy:25
}

impl SourceMap {
    pub fn to_json(&self) -> Value {
        json!({
            "target": self.target,
            "output": self.output,
            "source": self.source,
            "hash": self.hash,
            "generated": self.generated,
            "markers": self.markers.iter().map(Marker::to_json).collect::<Vec<_>>(),
        })
    }

    /// A source map from its JSON object; `None` when a field is missing or mistyped.
    pub fn from_json(value: &Value) -> Option<SourceMap> {
        let object = value.as_object()?;
        let markers = match object.get("markers") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items.iter().map(Marker::from_json).collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
        };
        Some(SourceMap {
            target: string_field(object, "target")?,
            output: string_field(object, "output")?,
            source: string_field(object, "source")?,
            hash: string_field(object, "hash")?,
            generated: string_field(object, "generated")?,
            markers,
        })
    }
}

fn string_field(object: &Map<String, Value>, name: &str) -> Option<String> {
    object.get(name)?.as_str().map(str::to_string)
}

fn usize_field(object: &Map<String, Value>, name: &str) -> Option<usize> {
    object.get(name)?.as_u64().and_then(|n| usize::try_from(n).ok())
}

/// The work the compiler does as one piece: one source file for one target.
// @lfy def/generation/data.lfy:28
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// What it is built for, as an index into `Workspace::targets`.
    pub target: usize, // @lfy def/generation/data.lfy:29
    /// The source file, as an index into `Workspace::files`.
    pub file: usize, // @lfy def/generation/data.lfy:30
    /// The entities of the file built for the target, in file order.
    pub entities: Vec<EntityId>, // @lfy def/generation/data.lfy:31
    /// The file's path relative to the directory it was found under, without its
    /// extension; the target's guidance spells the output file from it.
    pub stem: String, // @lfy def/generation/data.lfy:32
    /// The units of the same target for the files this file uses, transitively through
    /// files that have no unit, as indices into [`Plan::units`].
    pub dependencies: Vec<usize>, // @lfy def/generation/data.lfy:33
    /// The outputs the last accepted generation produced, from the target's source maps;
    /// empty when none.
    pub outputs: Vec<SourceMap>, // @lfy def/generation/data.lfy:34
    /// Why it is planned; `None` when it is up to date.
    pub reason: Option<Reason>, // @lfy def/generation/data.lfy:35
}

/// What a dependent unit may rely on from another unit's output.
// @lfy def/generation/data.lfy:38
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    /// The unit, as an index into [`Plan::units`].
    pub unit: usize, // @lfy def/generation/data.lfy:39
    /// The paths of its outputs.
    pub outputs: Vec<String>, // @lfy def/generation/data.lfy:40
    /// Its entities: each one's identifier, kind, definition, type, parameters and output
    /// as applicable, as indices into `Model::entities`.
    pub entities: Vec<EntityId>, // @lfy def/generation/data.lfy:41
}

/// Everything the compiler is handed to produce one unit's outputs.
// @lfy def/generation/data.lfy:44
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The unit, as an index into [`Plan::units`].
    pub unit: usize, // @lfy def/generation/data.lfy:45
    /// The prompt: what to produce, where, and the rules for producing it.
    pub instructions: String, // @lfy def/generation/data.lfy:46
    /// The source file's text.
    pub source: String, // @lfy def/generation/data.lfy:47
    /// The source text the existing outputs were generated from; `None` when unknown.
    pub previous: Option<String>, // @lfy def/generation/data.lfy:48
    /// The current text of each existing output, by path.
    pub existing: BTreeMap<String, String>, // @lfy def/generation/data.lfy:49
    /// One interface per dependency, in dependency order.
    pub interfaces: Vec<Interface>, // @lfy def/generation/data.lfy:50
    /// Every criterion of the target's marker and of every trait it extends, in that
    /// order, resolved for the marker.
    pub guidance: Vec<Criterion>, // @lfy def/generation/data.lfy:51
    /// What the generated code may require from the target's ecosystem.
    pub native_dependencies: Vec<NativeDependency>, // @lfy def/generation/data.lfy:52
}

/// One file the compiler produced.
// @lfy def/generation/data.lfy:55
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Relative to the workspace root.
    pub path: String, // @lfy def/generation/data.lfy:56
    /// Its content.
    pub text: String, // @lfy def/generation/data.lfy:57
}

/// Whether outputs are accepted for a request.
// @lfy def/generation/data.lfy:60
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Whether every check passed.
    pub accepted: bool, // @lfy def/generation/data.lfy:61
    /// What failed and why, one per failure; empty when accepted.
    pub problems: Vec<String>, // @lfy def/generation/data.lfy:62
    /// One per accepted output; empty when rejected.
    pub source_maps: Vec<SourceMap>, // @lfy def/generation/data.lfy:63
}

/// Every unit of a workspace, and which need generating.
// @lfy def/generation/data.lfy:66
// Decision: the definition gives the plan its workspace; a plan here does not own the
// workspace (which holds the whole model), so `request` and `accept` take the workspace
// the plan was made from as their first parameter instead.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Every unit of every target, each after its dependencies.
    pub units: Vec<Unit>, // @lfy def/generation/data.lfy:68
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
