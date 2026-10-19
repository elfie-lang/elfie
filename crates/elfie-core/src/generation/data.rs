//! Compiled from `def/generation/data.lfy`: the data of generation.
//!
//! A [`Unit`] is one source file for one target, with any entities a [`Move`] brought into
//! it; a [`Plan`] is every unit of a workspace, the moves that resolved cycles among files,
//! and the order the planned units are compiled in; a [`Request`] is everything the
//! compiler is handed for one unit, with the [`Reach`] of later units' outputs its run may
//! edit and every [`ForeignEdit`] other runs made in its own; a
//! [`Verdict`] is whether one unit's [`Output`]s are accepted, and the [`SourceMap`]s they
//! earn; an [`Outcome`] is what one run of the compiler came to, read from the
//! [`Submission`] it made or from its report; a [`Change`] is one
//! difference for one entity since a unit's output was accepted; a [`ReviewRequest`] is
//! everything a verifier is handed, a [`ReviewReport`] of [`Review`]s is what it found, and
//! a [`ReviewRecord`] is what one of those verdicts is kept as, so that the next review of
//! the same criterion can be skipped while the code it was judged against stands. The
//! `Target`, `File`,
//! `Unit`, `LoweredFile`, and `Entity` fields of the definition are kept as indices into
//! `Workspace::targets`, `Workspace::files`, `Plan::units`, `Program::files`, and
//! `Model::entities`, so that a plan is one plain value that does not own the workspace or
//! the program lowered from it.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value, json};

use crate::interpret::{LoweredCriterion, LoweredNode, LoweredTest, Program};
use crate::model::{Command, Criterion, EntityId, Knowledge, Problem};
use crate::workspace::{NativeDependency, Workspace};

/// Why a unit needs generating.
// @lfy def/generation/data.lfy:Reason
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// Its source differs from what its output was generated from.
    Changed, // @lfy def/generation/data.lfy:Reason.changed
    /// The interface of a unit it depends on differs from the one its output was generated
    /// against.
    Dependency, // @lfy def/generation/data.lfy:Reason.dependency
    /// No output has been generated for it.
    Fresh, // @lfy def/generation/data.lfy:Reason.fresh
    /// The caller asked for it regardless.
    Requested, // @lfy def/generation/data.lfy:Reason.requested
    /// Its criteria or tests differ from those its output was generated against, while its
    /// lowered code does not, so what its code must do may differ.
    Requirements, // @lfy def/generation/data.lfy:Reason.requirements
    /// Another unit edited its outputs during this compile.
    Touched, // @lfy def/generation/data.lfy:Reason.touched
    /// A global criterion or test its output answers for was reviewed as violated.
    Violated, // @lfy def/generation/data.lfy:Reason.violated
}

impl Reason {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            Reason::Fresh => "no output has been generated for it",
            Reason::Changed => "its source differs from what its output was generated from",
            Reason::Requirements => {
                "its criteria or tests differ from those its output was generated against, while its lowered code does not, so what its code must do may differ"
            }
            Reason::Dependency => {
                "the interface of a unit it depends on differs from the one its output was generated against"
            }
            Reason::Violated => {
                "a global criterion or test its output answers for was reviewed as violated"
            }
            Reason::Requested => "the caller asked for it regardless",
            Reason::Touched => "another unit edited its outputs during this compile",
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
            Reason::Touched => "touched",
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
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:28a0e4876e2210cd634262461b8037fae2548f3013fa1cbcc86d69334755b903
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
/// file of its unit, and re-read from the files when another unit writes them, carrying over
/// what acceptance recorded.
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
    /// hex.
    pub hash: String, // @lfy def/generation/data.lfy:SourceMap.hash
    /// SHA-256 of the ids of the unit's local criteria and tests, sorted and joined by line
    /// breaks, when the output was accepted.
    pub requirements: String, // @lfy def/generation/data.lfy:SourceMap.requirements
    /// The ids of the unit's local criteria and tests when the output was accepted, sorted;
    /// empty in a map file written before they were recorded. These are what
    /// [`SourceMap::requirements`] hashes.
    pub answered: Vec<String>, // @lfy def/generation/data.lfy:SourceMap.answered
    /// SHA-256 of the unit's interface text when the output was accepted, as lowercase hex.
    pub signature: String, // @lfy def/generation/data.lfy:SourceMap.signature
    /// SHA-256, as lowercase hex, of the unit's own region text of the output as it was
    /// accepted, as [`own_region_text`](super::own_region_text) takes it; empty in a map file
    /// written before it was recorded, which no hash equals.
    pub text: String, // @lfy def/generation/data.lfy:SourceMap.text
    /// For each dependency's source path, the [`SourceMap::signature`] the output was
    /// generated against.
    pub dependencies: BTreeMap<String, String>, // @lfy def/generation/data.lfy:SourceMap.dependencies
    /// When the output was accepted: the clock at acceptance as an RFC 3339 timestamp, as
    /// [`now_rfc3339`](super::now_rfc3339) spells it, carried over when the map is re-read
    /// from the files.
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
            "answered": self.answered,
            "signature": self.signature,
            "text": self.text,
            "dependencies": dependencies,
            "generated": self.generated,
            "markers": self.markers.iter().map(Marker::to_json).collect::<Vec<_>>(),
        })
    }

    /// A source map from its JSON object; `None` when a field is missing or mistyped.
    ///
    // Decision: the definition records a requirements hash, the ids it hashes, a signature,
    // the hash of the output's own text, and the signatures of the dependencies at
    // acceptance, so each is always written; a source map read back without them was written
    // before they were recorded. Reading takes them as empty rather than failing, which
    // leaves the unit looking out of date against every current hash and signature, so it is
    // regenerated instead of silently kept.
    pub fn from_json(value: &Value) -> Option<SourceMap> {
        let object = value.as_object()?;
        // @lfy def/generation/data.lfy:SourceMap.answered
        let answered = match object.get("answered") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()?,
            Some(_) => return None,
        };
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
            answered,
            signature: string_field(object, "signature").unwrap_or_default(),
            // @lfy def/generation/data.lfy:SourceMap.text
            text: string_field(object, "text").unwrap_or_default(),
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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    /// The file's path as it names itself from the workspace's source directory, without the
    /// `.lfy` extension; the target's guidance spells the output file from it.
    pub stem: String, // @lfy def/generation/data.lfy:Unit.stem
    /// The units of the same target its unit needs built first, each once, as indices into
    /// [`Plan::units`].
    pub dependencies: Vec<usize>, // @lfy def/generation/data.lfy:Unit.dependencies
    /// The outputs the last accepted generation produced, from the unit's map file; empty
    /// when none.
    pub outputs: Vec<SourceMap>, // @lfy def/generation/data.lfy:Unit.outputs
    /// Why it is planned; `None` when it is up to date.
    pub reason: Option<Reason>, // @lfy def/generation/data.lfy:Unit.reason
    /// The [`Unit::stem`] of each planned unit it starts after, in [`Plan::units`] order:
    /// each of [`Unit::dependencies`] that is planned and either has no [`Unit::outputs`] or
    /// has an interface whose hash differs from what the unit's outputs record for it.
    pub waits_for: Vec<String>, // @lfy def/generation/data.lfy:Unit.waitsFor
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
    // @lfy def/generation/data.lfy:Unit#Unit:Unit:e5292928d76df9bc73129964e9a39f6ef14fe33c190526dc4bb3f76ff2380810
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

/// What a foreign edit did to the region it is in.
// @lfy def/generation/data.lfy:ForeignEditKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForeignEditKind {
    /// It only removed lines of the region and added the target's stub line, naming the unit
    /// that made it, in their place.
    Stub, // @lfy def/generation/data.lfy:ForeignEditKind.stub
    /// Anything else, within the fix cap of [`Reach`].
    Fix, // @lfy def/generation/data.lfy:ForeignEditKind.fix
}

impl ForeignEditKind {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            ForeignEditKind::Stub => {
                "it only removed lines of the region and added the target's stub line, naming the unit that made it, in their place"
            }
            ForeignEditKind::Fix => "anything else, within the fix cap of [[Reach]]",
        }
    }

    /// The member's name.
    pub fn as_str(self) -> &'static str {
        match self {
            ForeignEditKind::Stub => "stub",
            ForeignEditKind::Fix => "fix",
        }
    }

    /// The member of this name; `None` when it is no member's name.
    pub fn from_name(name: &str) -> Option<ForeignEditKind> {
        match name {
            "stub" => Some(ForeignEditKind::Stub),
            "fix" => Some(ForeignEditKind::Fix),
            _ => None,
        }
    }
}

impl fmt::Display for ForeignEditKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One edit an accepted run for one unit made inside one region of another unit's output.
// Decision: a foreign edit names its units by `Unit::stem`, not by an index into
// `Plan::units`, because it is kept on disk in a change set and read back by a later
// compile, and a stem names one unit within its target.
// @lfy def/generation/data.lfy:ForeignEdit
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignEdit {
    /// The [`Unit::stem`] of the unit whose run made it.
    pub by: String, // @lfy def/generation/data.lfy:ForeignEdit.by
    /// The [`Unit::stem`] of the unit whose region it is in; always one of [`Reach::units`]
    /// of [`ForeignEdit::by`].
    pub owner: String, // @lfy def/generation/data.lfy:ForeignEdit.owner
    /// The output file it is in, relative to the workspace root.
    pub path: String, // @lfy def/generation/data.lfy:ForeignEdit.path
    /// Whether it is a stub or a fix.
    pub kind: ForeignEditKind, // @lfy def/generation/data.lfy:ForeignEdit.kind
    /// The marker that begins the region, as the owner's outputs held it before the edit;
    /// the edit leaves it as it was.
    pub region: Marker, // @lfy def/generation/data.lfy:ForeignEdit.region
    /// The lines of the region before the edit, in order, each without its line break.
    pub before: Vec<String>, // @lfy def/generation/data.lfy:ForeignEdit.before
    /// The lines of the region after the edit, in order, each without its line break.
    pub after: Vec<String>, // @lfy def/generation/data.lfy:ForeignEdit.after
    /// Why it was made: for a stub, the reason its stub line gives; for a fix, the note the
    /// run gave through `elfie_touch` for [`ForeignEdit::owner`].
    pub note: String, // @lfy def/generation/data.lfy:ForeignEdit.note
}

impl ForeignEdit {
    /// The lines the edit changes: the larger of the lines a line diff of
    /// [`ForeignEdit::before`] and [`ForeignEdit::after`] removes and the lines it adds.
    pub fn changed_lines(&self) -> usize {
        changed_lines(&self.before, &self.after)
    }

    /// Whether the edit is a stub by [`ForeignEdit::by`]: every line of
    /// [`ForeignEdit::after`] is a line of [`ForeignEdit::before`] or a stub line naming it,
    /// as `stub_line`, the target language's stub line, spells one.
    pub fn is_stub(&self, stub_line: &str) -> bool {
        is_stub_edit(&self.before, &self.after, &self.by, stub_line)
    }
}

/// The lines a line diff of `before` and `after` changes: the larger of the lines it removes
/// and the lines it adds, the diff keeping a longest common subsequence of the two.
// @lfy def/generation/data.lfy:ForeignEdit#ForeignEdit:ForeignEdit:9f4bf64d1380c6e4b6c272a55bacefc244e5567e4a656b78cc11b4f90cfb8a93
pub fn changed_lines(before: &[String], after: &[String]) -> usize {
    let mut row = vec![0usize; after.len() + 1];
    for old in before {
        let mut diagonal = 0;
        for (column, new) in after.iter().enumerate() {
            let above = row[column + 1];
            row[column + 1] = if old == new {
                diagonal + 1
            } else {
                above.max(row[column])
            };
            diagonal = above;
        }
    }
    let common = row[after.len()];
    (before.len() - common).max(after.len() - common)
}

/// The reason a stub line naming `unit` gives, when `line` is one: `stub_line` with
/// `{{unit}}` filled in with `unit` and anything in place of `{{reason}}`, with any white
/// space around it; `None` when `line` is no such stub line.
pub fn stub_reason(line: &str, unit: &str, stub_line: &str) -> Option<String> {
    let template = stub_line.trim().replace("{{unit}}", unit);
    let line = line.trim();
    match template.split_once("{{reason}}") {
        Some((head, tail)) => {
            let rest = line.strip_prefix(head)?;
            let reason = rest.strip_suffix(tail)?;
            Some(reason.to_string())
        }
        None => (line == template).then(String::new),
    }
}

/// Whether every line of `after` is a line of `before` or a stub line naming `unit`, and
/// at least one is a stub line.
// @lfy def/generation/data.lfy:ForeignEdit#ForeignEdit:ForeignEdit:24fcd7d322f7ad58780c8f5cc628542707536eb6113f5cd82e55ded738222475
pub fn is_stub_edit(before: &[String], after: &[String], unit: &str, stub_line: &str) -> bool {
    let mut stubbed = false;
    for line in after {
        if stub_reason(line, unit, stub_line).is_some() {
            stubbed = true;
        } else if !before.iter().any(|old| old == line) {
            return false;
        }
    }
    stubbed
}

/// The most lines a fix may change in one region.
// @lfy def/generation/data.lfy:Reach#Reach:Reach:c90883b427df6042bb5e2b86ae95d5236aa29a83b45b38d8e4ccb6edea1528d2
pub const FIX_LINES: usize = 10;

/// The most lines the fixes of one run may change across the regions of one other unit.
// @lfy def/generation/data.lfy:Reach#Reach:Reach:f753230f130da5275f26d9b3c385903babe45c15cf1b85e36c6df1e6a739d620
pub const FIX_UNIT_LINES: usize = 40;

/// Which other units' outputs a run for one unit may edit, and how much.
// Decision: the fix cap is two numbers kept here, once, so they can be tuned from
// measurements without touching what reads them; a reach carries them so a request and the
// agent server state the same cap.
// @lfy def/generation/data.lfy:Reach
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reach {
    /// Every unit of the same target that has outputs and depends on the unit, directly or
    /// through other units, in [`Plan::units`] order, as indices into it, whether or not the
    /// unit's interface changed; their outputs and regions are those of their
    /// [`Unit::outputs`].
    pub units: Vec<usize>, // @lfy def/generation/data.lfy:Reach.units
    /// The most lines a fix may change in one region.
    pub fix_lines: usize, // @lfy def/generation/data.lfy:Reach.fixLines
    /// The most lines the fixes of one run may change across the regions of one unit of
    /// [`Reach::units`].
    pub fix_unit_lines: usize, // @lfy def/generation/data.lfy:Reach.fixUnitLines
}

impl Reach {
    /// The reach of these units, with the fix cap [`FIX_LINES`] and [`FIX_UNIT_LINES`].
    // @lfy def/generation/data.lfy:Reach
    pub fn new(units: Vec<usize>) -> Reach {
        Reach {
            units,
            fix_lines: FIX_LINES,
            fix_unit_lines: FIX_UNIT_LINES,
        }
    }

    /// The reach of the unit at `index` of `plan`, the workspace being the one the plan was
    /// made from: every unit of its target that has outputs and depends on it, directly or
    /// through other units, whether or not the unit's interface changed.
    ///
    /// It reads the plan's units, their outputs, and the definitions alone, so what is
    /// planned changes nothing of it.
    // @lfy def/generation/data.lfy:Reach.units
    // @lfy def/generation/data.lfy:Reach#Reach:Reach:9fa35d3e58bd1c6985f920bf4d94049cd12b7b69531ce2e5a0b17cef22dd18e1
    pub fn of(_workspace: &Workspace, plan: &Plan, index: usize) -> Reach {
        let unit = &plan.units[index];
        let target = unit.target;
        let same = |other: &Unit| other.target == target && !other.outputs.is_empty();
        let mut depends = vec![false; plan.units.len()];
        depends[index] = true;
        // A unit comes after its dependencies in `Plan::units`, but a pass repeated until
        // nothing changes does not rely on that order.
        let mut changed = true;
        while changed {
            changed = false;
            for (at, other) in plan.units.iter().enumerate() {
                if !depends[at] && other.dependencies.iter().any(|&dependency| depends[dependency]) {
                    depends[at] = true;
                    changed = true;
                }
            }
        }
        // @lfy def/generation/data.lfy:Reach#Reach:Reach:ff348933ceb648c9d23dbc473954536137029600d6386cd50fe46337be163dd9
        // Every dependent with outputs, whatever interface it records: a change of behaviour
        // alone can stop a dependent's tests from passing.
        Reach::new(
            plan.units
                .iter()
                .enumerate()
                .filter(|&(at, other)| at != index && depends[at] && same(other))
                .map(|(at, _)| at)
                .collect(),
        )
    }
}

impl Default for Reach {
    fn default() -> Reach {
        Reach::new(Vec::new())
    }
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

/// A local criterion or test a unit's existing outputs were generated against that the unit
/// no longer has.
// Decision: a removed criterion or test is kept as its id, place, and text, taken while the
// file as it was is lowered, because its origin belongs to that lowering and nothing later
// can read it.
// @lfy def/generation/data.lfy:RemovedRequirement
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedRequirement {
    /// The id it had.
    pub id: String, // @lfy def/generation/data.lfy:RemovedRequirement.id
    /// Where it was in the unit's file as it was when its outputs were accepted: the file
    /// relative to the workspace root, a colon, and the line its origin began on; empty when
    /// not known.
    pub place: String, // @lfy def/generation/data.lfy:RemovedRequirement.place
    /// What it said: the criterion's text for a criterion, its input and its expectation as
    /// written for a test; empty when not known.
    pub text: String, // @lfy def/generation/data.lfy:RemovedRequirement.text
}

/// A local criterion or test whose id changed since a unit's outputs were accepted because
/// its text changed while it kept its position among its entity's criteria or tests.
// Decision: a criterion reworded in place keeps its place but not its id, because an id
// hashes the text. Pairing it with the id it replaces lets markers move to the new id
// mechanically, while what the new text asks is still read against the code.
// @lfy def/generation/data.lfy:Rewording
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewording {
    /// The id it has now.
    pub id: String, // @lfy def/generation/data.lfy:Rewording.id
    /// The id it had when the unit's outputs were accepted.
    pub replaces: String, // @lfy def/generation/data.lfy:Rewording.replaces
    /// What it said then, as a [`RemovedRequirement`] spells what a removed one said.
    pub text: String, // @lfy def/generation/data.lfy:Rewording.text
}

/// The source of a unit at its last accepted generation, or its hash-matched source,
/// recovered once and lowered once.
// @lfy def/generation/data.lfy:Previous
#[derive(Debug, Clone, PartialEq)]
pub struct Previous {
    /// The unit's file as it was, as written and never lowered.
    pub text: String, // @lfy def/generation/data.lfy:Previous.text
    /// The program lowered with the unit's file read as [`Previous::text`] and every other
    /// file as it is.
    pub program: Program, // @lfy def/generation/data.lfy:Previous.program
    /// Whether it is only the unit's hash-matched source, as [`SourceMap`] defines one,
    /// because neither the unit's file as loaded nor any revision of it is its source at its
    /// last accepted generation.
    pub hash_matched: bool, // @lfy def/generation/data.lfy:Previous.hashMatched
}

/// The code of one entity moved from the outputs of the unit of the file that declared it to
/// the unit of the file that declares it now, by rewriting the markers that name it.
// @lfy def/generation/data.lfy:Relocation
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Relocation {
    /// The identifier of the target whose outputs hold the markers.
    pub target: String, // @lfy def/generation/data.lfy:Relocation.target
    /// The name the markers spell: the identifier, or the owner's identifier, a dot, and a
    /// member's name.
    pub entity: String, // @lfy def/generation/data.lfy:Relocation.entity
    /// The file the markers named, relative to the workspace root; it no longer declares the
    /// entity.
    pub origin: String, // @lfy def/generation/data.lfy:Relocation.origin
    /// The file the markers name now, relative to the workspace root: the one project file
    /// that declares the entity.
    pub into: String, // @lfy def/generation/data.lfy:Relocation.into
    /// How many markers were rewritten.
    pub markers: usize, // @lfy def/generation/data.lfy:Relocation.markers
}

/// The review records one unit's review record file is to hold.
// @lfy def/generation/data.lfy:UnitReviews
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnitReviews {
    /// The identifier of the unit's target.
    pub target: String, // @lfy def/generation/data.lfy:UnitReviews.target
    /// The [`Unit::stem`] of the unit.
    pub stem: String, // @lfy def/generation/data.lfy:UnitReviews.stem
    /// Its records, sorted by [`ReviewRecord::id`].
    pub records: Vec<ReviewRecord>, // @lfy def/generation/data.lfy:UnitReviews.records
}

/// What relocating the code of entities that moved to another file gives, for the caller to
/// write.
// @lfy def/generation/data.lfy:Relocated
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Relocated {
    /// One per entity, file it left, and target, in `Workspace::targets` order, then in
    /// `Workspace::files` order of [`Relocation::into`], then in the order its first marker
    /// appears in the outputs, taken in path order.
    pub relocations: Vec<Relocation>, // @lfy def/generation/data.lfy:Relocated.relocations
    /// Each output file holding a rewritten marker, with its text once its markers are
    /// rewritten, in path order.
    pub outputs: Vec<Output>, // @lfy def/generation/data.lfy:Relocated.outputs
    /// Every source map read, with the maps of each unit a relocation moved code out of or
    /// into replaced by its maps as they read once [`Relocated::outputs`] are written.
    pub source_maps: Vec<SourceMap>, // @lfy def/generation/data.lfy:Relocated.sourceMaps
    /// The review records of each unit a relocation moved review records out of or into, as
    /// they are to be recorded, in [`Plan::units`] order.
    pub reviews: Vec<UnitReviews>, // @lfy def/generation/data.lfy:Relocated.reviews
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

/// Everything the compiler is handed to produce the outputs of one unit.
// @lfy def/generation/data.lfy:Request
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The unit, as an index into [`Plan::units`].
    pub unit: usize, // @lfy def/generation/data.lfy:Request.unit
    /// The prompt: what to produce, where, the rules for producing it, and how to report
    /// the outcome.
    pub instructions: String, // @lfy def/generation/data.lfy:Request.instructions
    /// [`Unit::text`] of the unit.
    pub source: String, // @lfy def/generation/data.lfy:Request.source
    /// The local criteria and tests of the unit, each with its id.
    pub requirements: Vec<Requirement>, // @lfy def/generation/data.lfy:Request.requirements
    /// Every global criterion and test of the program, each once with its id.
    pub globals: Vec<Requirement>, // @lfy def/generation/data.lfy:Request.globals
    /// The lowered text the unit's existing outputs were generated from; `None` where
    /// unknown.
    pub previous: Option<String>, // @lfy def/generation/data.lfy:Request.previous
    /// The ids of the local criteria and tests the unit's existing outputs were generated
    /// against; `None` where unknown.
    pub answered: Option<Vec<String>>, // @lfy def/generation/data.lfy:Request.answered
    /// The local criteria and tests the unit's existing outputs were generated against that
    /// the unit no longer has and that no criterion or test now rewords; `None` where the
    /// source they were generated from is unknown.
    pub removed: Option<Vec<RemovedRequirement>>, // @lfy def/generation/data.lfy:Request.removed
    /// The local criteria and tests of the unit that reword one its existing outputs were
    /// generated against; `None` where the source they were generated from is unknown.
    pub reworded: Option<Vec<Rewording>>, // @lfy def/generation/data.lfy:Request.reworded
    /// Whether the unit's existing outputs differ from what it asks for now only in the ids
    /// of reworded criteria and tests.
    pub reworded_only: bool, // @lfy def/generation/data.lfy:Request.rewordedOnly
    /// The current text of each existing output of the unit, by path.
    pub existing: BTreeMap<String, String>, // @lfy def/generation/data.lfy:Request.existing
    /// Every edit another unit's run made inside the unit's regions since the unit was last
    /// accepted, in the order they were made; each has the unit as [`ForeignEdit::owner`].
    pub foreign_edits: Vec<ForeignEdit>, // @lfy def/generation/data.lfy:Request.foreignEdits
    /// The later units whose outputs the run may edit, and the fix cap.
    pub reach: Reach, // @lfy def/generation/data.lfy:Request.reach
    /// Every relocation of this compile whose [`Relocation::into`] or
    /// [`Relocation::origin`] is the unit's file, for the unit's target, in the order they
    /// were made.
    pub relocated: Vec<Relocation>, // @lfy def/generation/data.lfy:Request.relocated
    /// One interface per dependency of the unit, in dependency order, each once.
    pub interfaces: Vec<Interface>, // @lfy def/generation/data.lfy:Request.interfaces
    /// The criteria of the target's declaration, in the guidance order, with template
    /// values rendered from the layers' arguments.
    pub guidance: Vec<Criterion>, // @lfy def/generation/data.lfy:Request.guidance
    /// What the generated code may require from the target's ecosystem: the target's native
    /// dependencies.
    pub native_dependencies: Vec<NativeDependency>, // @lfy def/generation/data.lfy:Request.nativeDependencies
    /// What the compiler is given to read: the target's knowledge, then the knowledge of
    /// each entity of the unit, then of each entity of [`Request::interfaces`],
    /// each item once.
    pub knowledge: Vec<Knowledge>, // @lfy def/generation/data.lfy:Request.knowledge
    /// The commands the compiler runs instead of knowing the tools: the target's commands.
    pub commands: Vec<Command>, // @lfy def/generation/data.lfy:Request.commands
}

/// Everything a verifier is handed to check outputs against what was asked: the local
/// criteria and tests of one unit that need review, or every global one once.
// @lfy def/generation/data.lfy:ReviewRequest
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewRequest {
    /// The unit, as an index into [`Plan::units`]; `None` for the review of global criteria
    /// and tests.
    pub unit: Option<usize>, // @lfy def/generation/data.lfy:ReviewRequest.unit
    /// The prompt: every criterion and test under review with its id and place, where the
    /// regions answering for each are, and how to report.
    pub instructions: String, // @lfy def/generation/data.lfy:ReviewRequest.instructions
    /// The id of every criterion and test [`ReviewRequest::instructions`] lists, in the
    /// order listed.
    pub asked: Vec<String>, // @lfy def/generation/data.lfy:ReviewRequest.asked
    /// For every local criterion and test of the unit, by id, the hash of the
    /// regions of the entity it belongs to, as the review computed it; empty for the review
    /// of global criteria and tests.
    pub hashes: BTreeMap<String, String>, // @lfy def/generation/data.lfy:ReviewRequest.hashes
    /// The record of every local criterion and test of the unit that is not
    /// listed, kept from its last review; empty for the review of global criteria and
    /// tests.
    pub carried: Vec<ReviewRecord>, // @lfy def/generation/data.lfy:ReviewRequest.carried
}

/// The last verdict on one local criterion or test of a unit, with the hash of the code it
/// was judged against.
///
/// [`ReviewRecord::of`] makes one from a [`Review`] of a run and the hashes of that run's
/// review request; a later review skips the criterion while its record is
/// [`ReviewStatus::Satisfied`] and its hash is still what the code hashes to.
// @lfy def/generation/data.lfy:ReviewRecord
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRecord {
    /// The id of the criterion or test.
    pub id: String, // @lfy def/generation/data.lfy:ReviewRecord.id
    /// What its last review found.
    pub status: ReviewStatus, // @lfy def/generation/data.lfy:ReviewRecord.status
    /// The criterion's or test's hash in [`ReviewRequest::hashes`] of the review request it
    /// was reviewed under.
    pub hash: String, // @lfy def/generation/data.lfy:ReviewRecord.hash
}

impl ReviewRecord {
    /// The record one review of a run earns: the review's id and status, with the hash the
    /// run's review request gave that id.
    ///
    /// A request that gave the id no hash leaves the record's hash empty, which no text
    /// hashes to, so the criterion is reviewed again rather than carried on a hash nothing
    /// was computed for.
    // @lfy def/generation/data.lfy:ReviewRecord#ReviewRecord:ReviewRecord:0ce7f4194c3e524869964be823e6403c4b1830e71fd55b600d154bcf9e69d6e3
    pub fn of(review: &Review, hashes: &BTreeMap<String, String>) -> ReviewRecord {
        ReviewRecord {
            id: review.id.clone(),
            status: review.status,
            hash: hashes.get(&review.id).cloned().unwrap_or_default(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "status": self.status.as_str(),
            "hash": self.hash,
        })
    }

    /// A record from its JSON object; `None` when a field is missing or mistyped, or when
    /// the status names no member of [`ReviewStatus`].
    pub fn from_json(value: &Value) -> Option<ReviewRecord> {
        let object = value.as_object()?;
        Some(ReviewRecord {
            id: string_field(object, "id")?,
            status: ReviewStatus::from_name(&string_field(object, "status")?)?,
            hash: string_field(object, "hash")?,
        })
    }
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
    /// The unit was accepted.
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
            OutcomeKind::Accepted => "the unit was accepted",
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

/// How the compiler says its run ended when it submits the outcome through the agent server.
// @lfy def/generation/data.lfy:SubmittedOutcome
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubmittedOutcome {
    /// Every output is written.
    Done, // @lfy def/generation/data.lfy:SubmittedOutcome.done
    /// It could not proceed.
    Blocked, // @lfy def/generation/data.lfy:SubmittedOutcome.blocked
    /// Only a person can decide.
    Clarify, // @lfy def/generation/data.lfy:SubmittedOutcome.clarify
}

impl SubmittedOutcome {
    /// The value of the enum member.
    pub fn value(self) -> &'static str {
        match self {
            SubmittedOutcome::Done => "every output is written",
            SubmittedOutcome::Blocked => "it could not proceed",
            SubmittedOutcome::Clarify => "only a person can decide",
        }
    }

    /// The member's name.
    pub fn as_str(self) -> &'static str {
        match self {
            SubmittedOutcome::Done => "done",
            SubmittedOutcome::Blocked => "blocked",
            SubmittedOutcome::Clarify => "clarify",
        }
    }

    /// The member of this name; `None` when it is no member's name.
    pub fn from_name(name: &str) -> Option<SubmittedOutcome> {
        match name {
            "done" => Some(SubmittedOutcome::Done),
            "blocked" => Some(SubmittedOutcome::Blocked),
            "clarify" => Some(SubmittedOutcome::Clarify),
            _ => None,
        }
    }
}

impl fmt::Display for SubmittedOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The outcome a compiler submitted through the agent server.
// Decision: the compiler states its outcome through `elfie_submit`, checked when the call is
// made, so the CLI reads a structured outcome and never a line of text; the `ELFIE:` line of
// the final message is kept for a compiler with no agent server, or one run by hand.
// @lfy def/generation/data.lfy:Submission
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    /// How it says the run ended.
    pub outcome: SubmittedOutcome, // @lfy def/generation/data.lfy:Submission.outcome
    /// The reason for blocked or the question for clarify; for done, a note that is no part
    /// of the outcome, and may be empty.
    pub message: String, // @lfy def/generation/data.lfy:Submission.message
}

/// What one run of the compiler came to, read mechanically from its submission or its
/// report, and the verdict.
// @lfy def/generation/data.lfy:Outcome
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// How it ended.
    pub kind: OutcomeKind, // @lfy def/generation/data.lfy:Outcome.kind
    /// The compiler's reason or question, or the first problem; empty when accepted.
    pub message: String, // @lfy def/generation/data.lfy:Outcome.message
    /// The verdict on the unit; `None` when the compiler never got that far.
    pub verdict: Option<Verdict>, // @lfy def/generation/data.lfy:Outcome.verdict
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
    /// Every edit the outputs make inside a region of another unit, one per region, in the
    /// order of their paths and then of their lines; empty when rejected.
    pub foreign_edits: Vec<ForeignEdit>, // @lfy def/generation/data.lfy:Verdict.foreignEdits
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
    /// Its type differs, as the interface text of [`SourceMap::signature`] spells it.
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
            ChangeKind::DeclaredType => {
                "its type differs, as the interface text of [[SourceMap.signature]] spells it"
            }
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
    // @lfy def/generation/data.lfy:Review
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

/// Everything a verifier found for one unit, or for the global review, read mechanically
/// from its report.
// @lfy def/generation/data.lfy:ReviewReport
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewReport {
    /// Each review submitted through the agent server, in the order recorded, then each
    /// read from the report, in report order; one per id.
    pub reviews: Vec<Review>, // @lfy def/generation/data.lfy:ReviewReport.reviews
    /// Each review line that was not read as a review, then the line reading the report did
    /// not end when there was no end line.
    pub problems: Vec<String>, // @lfy def/generation/data.lfy:ReviewReport.problems
    /// Each review line that was not read as a review though its status is violated, as
    /// written; each is in [`ReviewReport::problems`] too.
    pub unplaced: Vec<String>, // @lfy def/generation/data.lfy:ReviewReport.unplaced
    /// Each id the review was asked about that no review in [`ReviewReport::reviews`] names,
    /// in the order it was asked.
    pub missing: Vec<String>, // @lfy def/generation/data.lfy:ReviewReport.missing
}

/// Every unit of a workspace, which need generating, and the order they are compiled in.
// @lfy def/generation/data.lfy:Plan
// Decision: the definition gives the plan its workspace and the program lowered from it; a
// plan here owns neither (the workspace holds the whole model, and the program holds the
// workspace), so `request` and `accept` take the workspace the plan was made from as their
// first parameter instead, and a unit names its lowered file by its index in
// `Program::files`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Every unit of every target, each after its dependencies; the planned units are
    /// compiled in this order, each starting once each unit of its [`Unit::waits_for`] has
    /// merged into its change set, been skipped, or stopped.
    pub units: Vec<Unit>, // @lfy def/generation/data.lfy:Plan.units
    /// Every move that resolved a cycle among files, in plan order of the origin unit, then
    /// in file order of the first of the entities moved.
    pub moves: Vec<Move>, // @lfy def/generation/data.lfy:Plan.moves
    /// Every cycle among files that no move could resolve, and every entity that reaches one
    /// not built for its target, one problem each.
    pub problems: Vec<Problem>, // @lfy def/generation/data.lfy:Plan.problems
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
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:76e237fb260caf5b11bc6d84be162349e39c76c149a14baf79eb7d16efa750a6
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
            // @lfy def/generation/data.lfy:SourceMap.answered
            answered: vec!["Marker:Marker:abc".to_string()],
            signature: "def".to_string(),
            // @lfy def/generation/data.lfy:SourceMap.text
            text: "mno".to_string(),
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
    // @lfy def/generation/data.lfy:Marker#Marker:Marker:28a0e4876e2210cd634262461b8037fae2548f3013fa1cbcc86d69334755b903
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

    // @lfy def/generation/data.lfy:Review
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

    /// A record made from a review of a run takes that review's id and status, and the hash
    /// the run's review request gave the id; one made from a review the request gave no hash
    /// for takes none, so the criterion is reviewed again.
    // @lfy def/generation/data.lfy:ReviewRecord#ReviewRecord:ReviewRecord:0ce7f4194c3e524869964be823e6403c4b1830e71fd55b600d154bcf9e69d6e3
    #[test]
    fn a_record_is_a_reviews_id_and_status_with_the_hash_it_was_judged_against() {
        let review = Review {
            id: "A:A:abc".to_string(),
            file: "def/a.lfy".to_string(),
            line: 4,
            entity: "A".to_string(),
            status: ReviewStatus::Satisfied,
            evidence: "src/a.rs:1-30".to_string(),
            note: "the field is there".to_string(),
        };
        let hashes = BTreeMap::from([("A:A:abc".to_string(), "deadbeef".to_string())]);
        let record = ReviewRecord::of(&review, &hashes);
        assert_eq!(record.id, "A:A:abc");
        assert_eq!(record.status, ReviewStatus::Satisfied);
        assert_eq!(record.hash, "deadbeef");
        // The record is written with its keys in alphabetical order and reads back whole.
        let value = record.to_json();
        let keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["hash", "id", "status"]);
        assert_eq!(ReviewRecord::from_json(&value), Some(record));

        // A review the request gave no hash for is recorded with none.
        let unhashed = ReviewRecord::of(&review, &BTreeMap::new());
        assert_eq!(unhashed.hash, "");
        // A status that names no member of `ReviewStatus` is no record.
        assert_eq!(
            ReviewRecord::from_json(&json!({"id": "A:A:abc", "status": "maybe", "hash": ""})),
            None
        );
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
        assert!(map.answered.is_empty()); // @lfy def/generation/data.lfy:SourceMap.answered
        // An empty text hash equals no hash of an output, so the output looks rewritten.
        // @lfy def/generation/data.lfy:SourceMap.text
        assert_eq!(map.text, "");
        assert!(map.dependencies.is_empty());
    }

    // @lfy def/generation/data.lfy:ForeignEdit
    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| line.to_string()).collect()
    }

    // @lfy def/generation/data.lfy:ForeignEdit#ForeignEdit:ForeignEdit:9f4bf64d1380c6e4b6c272a55bacefc244e5567e4a656b78cc11b4f90cfb8a93
    #[test]
    fn foreign_edit_counts_the_larger_of_removed_and_added_lines() {
        let before = lines(&["a", "b", "c", "d"]);
        // One line changed is one removed and one added.
        assert_eq!(changed_lines(&before, &lines(&["a", "x", "c", "d"])), 1);
        // Two removed and one added count two.
        assert_eq!(changed_lines(&before, &lines(&["a", "x", "d"])), 2);
        // Three added and none removed count three.
        assert_eq!(
            changed_lines(&before, &lines(&["a", "b", "x", "y", "z", "c", "d"])),
            3
        );
        // A moved line is one removed and one added.
        assert_eq!(changed_lines(&before, &lines(&["b", "c", "d", "a"])), 1);
        assert_eq!(changed_lines(&before, &before), 0);
        let edit = ForeignEdit {
            by: "b".to_string(),
            owner: "a".to_string(),
            path: "src/a.rs".to_string(),
            kind: ForeignEditKind::Fix,
            region: Marker {
                output_line: 1,
                file: "def/a.lfy".to_string(),
                entity: Some("f".to_string()),
                line: 1,
                column: None,
                requirement: None,
                end: 4,
            },
            before,
            after: lines(&["a", "x", "d"]),
            note: "a fix".to_string(),
        };
        assert_eq!(edit.changed_lines(), 2);
    }

    // @lfy def/generation/data.lfy:ForeignEdit#ForeignEdit:ForeignEdit:24fcd7d322f7ad58780c8f5cc628542707536eb6113f5cd82e55ded738222475
    #[test]
    fn foreign_edit_stub_keeps_only_lines_of_before_and_stub_lines_naming_its_unit() {
        let stub = r#"unimplemented!("elfie stub: {{unit}}: {{reason}}")"#;
        let before = lines(&["fn f() {", "    body();", "}"]);
        let stubbed = lines(&[
            "fn f() {",
            r#"    unimplemented!("elfie stub: b: a field moved")"#,
            "}",
        ]);
        assert!(is_stub_edit(&before, &stubbed, "b", stub));
        assert_eq!(
            stub_reason(&stubbed[1], "b", stub).as_deref(),
            Some("a field moved")
        );
        // A stub line naming another unit is not a stub by this one.
        assert!(!is_stub_edit(&before, &stubbed, "c", stub));
        // A line neither of before nor a stub line makes the edit a fix.
        let fixed = lines(&["fn f() {", "    other();", "}"]);
        assert!(!is_stub_edit(&before, &fixed, "b", stub));
        let mixed = lines(&[
            "fn f() {",
            r#"    unimplemented!("elfie stub: b: why")"#,
            "    other();",
            "}",
        ]);
        assert!(!is_stub_edit(&before, &mixed, "b", stub));
        // Removing lines without a stub line is no stub.
        assert!(!is_stub_edit(&before, &lines(&["fn f() {", "}"]), "b", stub));
    }

    // @lfy def/generation/data.lfy:Reach#Reach:Reach:c90883b427df6042bb5e2b86ae95d5236aa29a83b45b38d8e4ccb6edea1528d2
    #[test]
    fn reach_fix_lines_is_ten() {
        assert_eq!(Reach::new(Vec::new()).fix_lines, 10);
        assert_eq!(Reach::default().fix_lines, 10);
    }

    // @lfy def/generation/data.lfy:Reach#Reach:Reach:f753230f130da5275f26d9b3c385903babe45c15cf1b85e36c6df1e6a739d620
    #[test]
    fn reach_fix_unit_lines_is_forty() {
        assert_eq!(Reach::new(Vec::new()).fix_unit_lines, 40);
        assert_eq!(Reach::default().fix_unit_lines, 40);
    }
}
