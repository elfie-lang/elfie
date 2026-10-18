//! Compiled from `def/cli/data.lfy`: what the command line can do and how it returns.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use elfie_core::generation::{ForeignEdit, ForeignEditKind, Marker, Reason};
use serde_json::{Value, json};

/// What the command line can do; each value is the word that names it.
// @lfy def/cli/data.lfy:Command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help,    // @lfy def/cli/data.lfy:Command.help
    Version, // @lfy def/cli/data.lfy:Command.version
    Init,    // @lfy def/cli/data.lfy:Command.init
    Check,   // @lfy def/cli/data.lfy:Command.check
    Format,  // @lfy def/cli/data.lfy:Command.format
    Tree,    // @lfy def/cli/data.lfy:Command.tree
    Tokens,  // @lfy def/cli/data.lfy:Command.tokens
    Compile, // @lfy def/cli/data.lfy:Command.compile
    Verify,  // @lfy def/cli/data.lfy:Command.verify
    Lsp,     // @lfy def/cli/data.lfy:Command.lsp
    Mcp,     // @lfy def/cli/data.lfy:Command.mcp
}

impl Command {
    pub const ALL: [Command; 11] = [
        Command::Help,
        Command::Version,
        Command::Init,
        Command::Check,
        Command::Format,
        Command::Tree,
        Command::Tokens,
        Command::Compile,
        Command::Verify,
        Command::Lsp,
        Command::Mcp,
    ];

    /// The value of the enum member: what is typed.
    pub fn value(self) -> &'static str {
        match self {
            Command::Help => "help",
            Command::Version => "version",
            Command::Init => "init",
            Command::Check => "check",
            Command::Format => "format",
            Command::Tree => "tree",
            Command::Tokens => "tokens",
            Command::Compile => "compile",
            Command::Verify => "verify",
            Command::Lsp => "lsp",
            Command::Mcp => "mcp",
        }
    }

    pub fn lookup(text: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.value() == text)
    }

    /// One line of description for help.
    pub fn description(self) -> &'static str {
        match self {
            Command::Help => "Print this help",
            Command::Version => "Print the version of the executable",
            Command::Init => "Create elfie.json and the source directory in the root",
            Command::Check => "Load the project and print every problem",
            Command::Format => "Rewrite source files in the standard layout (--check only reports)",
            Command::Tree => "Print the parse tree of one file",
            Command::Tokens => "Print the tokens of one file",
            Command::Compile => "Plan the units and run the compiler on each (--dry-run, --all, --target)",
            Command::Verify => "Run the verifier on the outputs already recorded, compiling nothing (--target)",
            Command::Lsp => "Serve an editor over standard input and output",
            Command::Mcp => "Serve agents over standard input and output",
        }
    }
}

/// What the process ends with, through [`std::process::exit`].
// @lfy def/cli/data.lfy:ExitCode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Done, and nothing to report.
    Success = 0, // @lfy def/cli/data.lfy:ExitCode.success
    /// Done, and problems were found or outputs rejected.
    Problems = 1, // @lfy def/cli/data.lfy:ExitCode.problems
    /// The arguments could not be read.
    Usage = 2, // @lfy def/cli/data.lfy:ExitCode.usage
    /// Something the command relies on failed.
    Failure = 3, // @lfy def/cli/data.lfy:ExitCode.failure
}

impl ExitCode {
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// One parsed command line.
// @lfy def/cli/data.lfy:Invocation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// What to do.
    pub command: Command, // @lfy def/cli/data.lfy:Invocation.command
    /// The project directory.
    pub root: String, // @lfy def/cli/data.lfy:Invocation.root
    /// The positional arguments after the command, in order.
    pub arguments: Vec<String>, // @lfy def/cli/data.lfy:Invocation.arguments
    /// Options by name without dashes: `None` for a flag, the text for a valued option.
    pub options: BTreeMap<String, Option<String>>, // @lfy def/cli/data.lfy:Invocation.options
}

impl Invocation {
    pub fn flag(&self, name: &str) -> bool {
        self.options.contains_key(name)
    }

    pub fn option(&self, name: &str) -> Option<&str> {
        self.options.get(name).and_then(|v| v.as_deref())
    }
}

/// What a compile is doing, as progress reports it.
// @lfy def/cli/data.lfy:Step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Planned,         // @lfy def/cli/data.lfy:Step.planned
    Requesting,      // @lfy def/cli/data.lfy:Step.requesting
    Resuming,        // @lfy def/cli/data.lfy:Step.resuming
    Rewording,       // @lfy def/cli/data.lfy:Step.rewording
    Building,        // @lfy def/cli/data.lfy:Step.building
    Compiling,       // @lfy def/cli/data.lfy:Step.compiling
    Checking,        // @lfy def/cli/data.lfy:Step.checking
    Accepted,        // @lfy def/cli/data.lfy:Step.accepted
    Rejected,        // @lfy def/cli/data.lfy:Step.rejected
    Verifying,       // @lfy def/cli/data.lfy:Step.verifying
    Reviewed,        // @lfy def/cli/data.lfy:Step.reviewed
    GlobalVerifying, // @lfy def/cli/data.lfy:Step.globalVerifying
    GlobalReviewed,  // @lfy def/cli/data.lfy:Step.globalReviewed
    Retrying,        // @lfy def/cli/data.lfy:Step.retrying
    Blocked,         // @lfy def/cli/data.lfy:Step.blocked
    Clarification,   // @lfy def/cli/data.lfy:Step.clarification
    Failed,          // @lfy def/cli/data.lfy:Step.failed
    Touched,         // @lfy def/cli/data.lfy:Step.touched
    Stubbed,         // @lfy def/cli/data.lfy:Step.stubbed
    FinalGate,       // @lfy def/cli/data.lfy:Step.finalGate
    Merging,         // @lfy def/cli/data.lfy:Step.merging
    Finished,        // @lfy def/cli/data.lfy:Step.finished
}

impl Step {
    /// The name of the step, as a progress line and a JSON object spell it.
    pub fn name(self) -> &'static str {
        match self {
            Step::Planned => "planned",
            Step::Requesting => "requesting",
            Step::Resuming => "resuming",
            Step::Rewording => "rewording",
            Step::Building => "building",
            Step::Compiling => "compiling",
            Step::Checking => "checking",
            Step::Accepted => "accepted",
            Step::Rejected => "rejected",
            Step::Verifying => "verifying",
            Step::Reviewed => "reviewed",
            Step::GlobalVerifying => "globalVerifying",
            Step::GlobalReviewed => "globalReviewed",
            Step::Retrying => "retrying",
            Step::Blocked => "blocked",
            Step::Clarification => "clarification",
            Step::Failed => "failed",
            Step::Touched => "touched",
            Step::Stubbed => "stubbed",
            Step::FinalGate => "finalGate",
            Step::Merging => "merging",
            Step::Finished => "finished",
        }
    }

    /// The value of the enum member: what the step means.
    pub fn value(self) -> &'static str {
        match self {
            Step::Planned => "the plan is made",
            Step::Requesting => "a unit's request is being assembled",
            Step::Resuming => {
                "a kept change set is taken up again, its units accepted with unchanged \
                 definitions kept as they were"
            }
            Step::Rewording => {
                "the markers of a unit's reworded criteria and tests were rewritten to their new ids"
            }
            Step::Building => {
                "the target's build and test commands are running for a unit, in place of the \
                 compiler or as the gate after its acceptance"
            }
            Step::Compiling => "the compiler is running on a unit",
            Step::Checking => "the outputs of a unit are being checked",
            Step::Accepted => "a unit was accepted and recorded in the change set",
            Step::Rejected => "a unit was rejected",
            Step::Verifying => "the verifier is running on a unit that was accepted",
            Step::Reviewed => "the verifier reviewed a unit, or every verdict on it was carried",
            Step::GlobalVerifying => {
                "the verifier is running once on every global criterion and test"
            }
            Step::GlobalReviewed => "the verifier reviewed the global criteria and tests",
            Step::Retrying => "the compiler is running a unit again with the problems",
            Step::Blocked => "the compiler could not proceed",
            Step::Clarification => "the compiler asked a question",
            Step::Failed => "the compiler command failed",
            Step::Touched => {
                "an accepted unit fixed code inside the regions of a later unit, which is planned \
                 as touched"
            }
            Step::Stubbed => {
                "an accepted unit replaced code inside the regions of a later unit with stubs, \
                 and that unit is planned as touched"
            }
            Step::FinalGate => {
                "the change set is being checked as a whole before the root takes it"
            }
            Step::Merging => "the root is taking every unit of the change set at once",
            Step::Finished => "the compile is over",
        }
    }
}

/// One line of what a compile is doing, for a person or a script watching.
// @lfy def/cli/data.lfy:Progress
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    /// What is happening.
    pub step: Step, // @lfy def/cli/data.lfy:Progress.step
    /// The unit it concerns: for `Touched` and `Stubbed`, the unit edited; `None` for a step
    /// that concerns no one unit, such as `Planned`, `FinalGate`, `Merging`,
    /// `GlobalVerifying`, `GlobalReviewed`, and `Finished`.
    pub unit: Option<String>, // @lfy def/cli/data.lfy:Progress.unit
    /// Units accepted so far.
    pub done: usize, // @lfy def/cli/data.lfy:Progress.done
    /// Units planned.
    pub total: usize, // @lfy def/cli/data.lfy:Progress.total
    /// Seconds since the compile began, as [`std::time::SystemTime::elapsed`] from its
    /// start gives them.
    pub elapsed: f64, // @lfy def/cli/data.lfy:Progress.elapsed
    /// The detail: the reason a unit is planned, a problem, a question, the counts of
    /// the plan, for `Touched` and `Stubbed` the unit whose run made the edits and their
    /// notes, or for `Reviewed` and `GlobalReviewed` the counts of satisfied, violated, and
    /// unverifiable reviews, in that order, then for `Reviewed` the count of verdicts carried
    /// when there are any.
    pub message: String, // @lfy def/cli/data.lfy:Progress.message
}

/// Where one unit of a change set stands.
// @lfy def/cli/data.lfy:UnitState
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitState {
    Planned,  // @lfy def/cli/data.lfy:UnitState.planned
    Accepted, // @lfy def/cli/data.lfy:UnitState.accepted
    Reviewed, // @lfy def/cli/data.lfy:UnitState.reviewed
    Rejected, // @lfy def/cli/data.lfy:UnitState.rejected
}

impl UnitState {
    pub const ALL: [UnitState; 4] =
        [UnitState::Planned, UnitState::Accepted, UnitState::Reviewed, UnitState::Rejected];

    /// The value of the enum member, as a change set's file spells it.
    pub fn value(self) -> &'static str {
        match self {
            UnitState::Planned => "planned",
            UnitState::Accepted => "accepted",
            UnitState::Reviewed => "reviewed",
            UnitState::Rejected => "rejected",
        }
    }

    pub fn lookup(text: &str) -> Option<UnitState> {
        UnitState::ALL.into_iter().find(|s| s.value() == text)
    }

    /// Whether a unit in this state has its outputs accepted in the change set.
    pub fn is_done(self) -> bool {
        matches!(self, UnitState::Accepted | UnitState::Reviewed)
    }
}

/// One unit of a change set and where it stands.
// @lfy def/cli/data.lfy:ChangeSetUnit
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSetUnit {
    /// The [`Unit::stem`](elfie_core::generation::Unit::stem) of the unit.
    pub stem: String, // @lfy def/cli/data.lfy:ChangeSetUnit.stem
    /// Why it is planned.
    pub reason: Reason, // @lfy def/cli/data.lfy:ChangeSetUnit.reason
    /// Where it stands.
    pub state: UnitState, // @lfy def/cli/data.lfy:ChangeSetUnit.state
    /// SHA-256 of the unit's text when it was accepted, as lowercase hex, as
    /// [`SourceMap::hash`](elfie_core::generation::SourceMap::hash) spells it; empty while it
    /// is planned or rejected.
    pub hash: String, // @lfy def/cli/data.lfy:ChangeSetUnit.hash
    /// SHA-256 of the ids of the unit's local criteria and tests when it was accepted, taken
    /// as [`SourceMap::requirements`](elfie_core::generation::SourceMap::requirements) takes
    /// them; empty while it is planned or rejected.
    pub requirements: String, // @lfy def/cli/data.lfy:ChangeSetUnit.requirements
    /// How many foreign edits [`ChangeSet::foreign_edits`] held when the unit was last
    /// accepted in the change set; 0 when it never was; kept while it is planned again.
    pub edits: usize, // @lfy def/cli/data.lfy:ChangeSetUnit.edits
}

impl ChangeSetUnit {
    /// A unit planned now, with the hash and requirements it has now, as
    /// [`ChangeSet::resume`] compares them.
    pub fn planned(stem: &str, reason: Reason, hash: &str, requirements: &str) -> ChangeSetUnit {
        ChangeSetUnit {
            stem: stem.to_string(),
            reason,
            state: UnitState::Planned,
            hash: hash.to_string(),
            requirements: requirements.to_string(),
            edits: 0,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "stem": self.stem,
            "reason": self.reason.as_str(),
            "state": self.state.value(),
            "hash": self.hash,
            "requirements": self.requirements,
            "edits": self.edits,
        })
    }

    fn from_json(value: &Value) -> Option<ChangeSetUnit> {
        let object = value.as_object()?;
        Some(ChangeSetUnit {
            stem: object.get("stem")?.as_str()?.to_string(),
            reason: reason_named(object.get("reason")?.as_str()?)?,
            state: UnitState::lookup(object.get("state")?.as_str()?)?,
            hash: object.get("hash")?.as_str()?.to_string(),
            requirements: object.get("requirements")?.as_str()?.to_string(),
            edits: usize::try_from(object.get("edits")?.as_u64()?).ok()?,
        })
    }
}

/// Every planned unit of one compile for one target, worked in one copy of the root that the
/// root takes only when the whole compile is done.
// Decision: a change set is kept on disk between compiles, so a compile that stops loses no unit
// accepted under definitions that have not changed since.
// @lfy def/cli/data.lfy:ChangeSet
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    /// The [`Target::identifier`](elfie_core::workspace::Target::identifier) of the target.
    pub target: String, // @lfy def/cli/data.lfy:ChangeSet.target
    /// The absolute path of the copy of the root the change set is worked in.
    pub directory: String, // @lfy def/cli/data.lfy:ChangeSet.directory
    /// One per planned unit, in plan order, a unit planned because it was touched included.
    pub units: Vec<ChangeSetUnit>, // @lfy def/cli/data.lfy:ChangeSet.units
    /// Every foreign edit an accepted unit of the change set made, in the order they were
    /// accepted. Only [`ChangeSet::accept`] adds to it.
    pub foreign_edits: Vec<ForeignEdit>, // @lfy def/cli/data.lfy:ChangeSet.foreignEdits
}

impl ChangeSet {
    /// elfie-compile/cache/changes/<target>.json under the root.
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:b655fcdcd7a4f6bc06eb2f5919bbd30b6986babf7942787f32a4b93cb22f3ef2
    pub fn file(root: &Path, target: &str) -> PathBuf {
        root.join("elfie-compile")
            .join("cache")
            .join("changes")
            .join(format!("{target}.json"))
    }

    pub fn to_json(&self) -> Value {
        json!({
            "target": self.target,
            "directory": self.directory,
            "units": self.units.iter().map(ChangeSetUnit::to_json).collect::<Vec<_>>(),
            "foreignEdits": self.foreign_edits.iter().map(foreign_edit_to_json).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(value: &Value) -> Option<ChangeSet> {
        let object = value.as_object()?;
        Some(ChangeSet {
            target: object.get("target")?.as_str()?.to_string(),
            directory: object.get("directory")?.as_str()?.to_string(),
            units: object
                .get("units")?
                .as_array()?
                .iter()
                .map(ChangeSetUnit::from_json)
                .collect::<Option<Vec<_>>>()?,
            foreign_edits: object
                .get("foreignEdits")?
                .as_array()?
                .iter()
                .map(foreign_edit_from_json)
                .collect::<Option<Vec<_>>>()?,
        })
    }

    /// Writes the change set to its file under the root, at once, as compact JSON. Nothing but
    /// [`ChangeSet::remove`] deletes that file or the copy, so a compile that stops before the
    /// root takes the change set leaves both as they were last written.
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:b655fcdcd7a4f6bc06eb2f5919bbd30b6986babf7942787f32a4b93cb22f3ef2
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:da099811d2a1e35b89a470128465e0c875995f7a6184fd85c63c02ccd2817239
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let file = ChangeSet::file(root, &self.target);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file, self.to_json().to_string())
    }

    /// The kept change set of a target; `None` when there is none or its file cannot be read
    /// as one.
    pub fn load(root: &Path, target: &str) -> Option<ChangeSet> {
        let text = std::fs::read_to_string(ChangeSet::file(root, target)).ok()?;
        let value = serde_json::from_str::<Value>(&text).ok()?;
        ChangeSet::from_json(&value).filter(|set| set.target == target)
    }

    /// Removes the change set's file under elfie-compile/cache and its copy, as when the root
    /// takes it. What is already gone is no error.
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:d9f289811c4e43fa3bb96a1b1a7d01f546422d23b339e148ae7b61176238bef6
    pub fn remove(&self, root: &Path) -> std::io::Result<()> {
        let gone = |result: std::io::Result<()>| match result {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
        gone(std::fs::remove_file(ChangeSet::file(root, &self.target)))?;
        gone(std::fs::remove_dir_all(Path::new(&self.directory)))
    }

    /// The place in `now`, the units planned now in plan order with the hash and requirements
    /// each has now, from which units run again: the first place where the stems differ, or
    /// where a unit accepted or reviewed in the change set has another hash or other
    /// requirements than it has now. `now.len()` when every unit keeps its state.
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:77db4241a1867889bf369abe1c66f13af671b71c6c32b2e8966e9283f545fe1c
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:8983cc59f8e506396e449579c05ad935909c8115eac5061aefda6fb014c41a2f
    pub fn kept(&self, now: &[ChangeSetUnit]) -> usize {
        self.units
            .iter()
            .zip(now)
            .position(|(kept, planned)| {
                kept.stem != planned.stem
                    || (kept.state.is_done()
                        && (kept.hash != planned.hash || kept.requirements != planned.requirements))
            })
            .unwrap_or(self.units.len().min(now.len()))
    }

    /// Takes the change set up again for the units planned now: every unit before
    /// [`ChangeSet::kept`] keeps its state, and that unit and every unit after it is the one
    /// planned now at its place, planned again, with the [`ChangeSetUnit::edits`] it had when
    /// it was in the change set. [`ChangeSet::foreign_edits`] stays as it was. Returns the
    /// place of the unit that runs next, the first that is neither accepted nor reviewed;
    /// `None` when every unit is.
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:6b29187de3d79dc587549407b8213f3a4844ef711b59317cbd20c8d1a09e4808
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:77db4241a1867889bf369abe1c66f13af671b71c6c32b2e8966e9283f545fe1c
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:8983cc59f8e506396e449579c05ad935909c8115eac5061aefda6fb014c41a2f
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:4f28781169f9b118953b17a5d1d2ef6b7936745d29530f30e2ad02d7ec23644c
    pub fn resume(&mut self, now: &[ChangeSetUnit]) -> Option<usize> {
        let kept = self.kept(now);
        let again = now[kept..].iter().map(|planned| {
            let edits = self
                .units
                .iter()
                .find(|unit| unit.stem == planned.stem)
                .map_or(0, |unit| unit.edits);
            ChangeSetUnit {
                state: UnitState::Planned,
                hash: String::new(),
                requirements: String::new(),
                edits,
                ..planned.clone()
            }
        });
        let units: Vec<ChangeSetUnit> = self.units[..kept].iter().cloned().chain(again).collect();
        self.units = units;
        self.units.iter().position(|unit| !unit.state.is_done())
    }

    /// Records the unit at `index` as accepted with the hash and requirements it has now, and
    /// appends the foreign edits its run made after every one recorded before, so that its
    /// [`ChangeSetUnit::edits`] is the count they make together.
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:4f28781169f9b118953b17a5d1d2ef6b7936745d29530f30e2ad02d7ec23644c
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:e3f9c542851859c9bb86eee22b99337db1e0fb4cc60d7e58f3ff554e5d6ea747
    pub fn accept(&mut self, index: usize, hash: &str, requirements: &str, edits: Vec<ForeignEdit>) {
        self.foreign_edits.extend(edits);
        let count = self.foreign_edits.len();
        let unit = &mut self.units[index];
        unit.state = UnitState::Accepted;
        unit.hash = hash.to_string();
        unit.requirements = requirements.to_string();
        unit.edits = count;
    }

    /// The foreign edits recorded since the unit of this stem was last accepted in the change
    /// set, in their order: those it owns among the items after its first
    /// [`ChangeSetUnit::edits`]; after none when the unit is not in the change set.
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:115db6923c273f848db7bd842f876ae3f28a068b801bb4956b81311fdb961444
    pub fn edits_since(&self, stem: &str) -> Vec<&ForeignEdit> {
        let skip = self
            .units
            .iter()
            .find(|unit| unit.stem == stem)
            .map_or(0, |unit| unit.edits);
        self.foreign_edits
            .iter()
            .skip(skip)
            .filter(|edit| edit.owner == stem)
            .collect()
    }

    /// Every foreign edit recorded since its owner was last accepted in the change set, in
    /// their order.
    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:115db6923c273f848db7bd842f876ae3f28a068b801bb4956b81311fdb961444
    pub fn recorded_since(&self) -> Vec<&ForeignEdit> {
        self.foreign_edits
            .iter()
            .enumerate()
            .filter(|(index, edit)| {
                let skip = self
                    .units
                    .iter()
                    .find(|unit| unit.stem == edit.owner)
                    .map_or(0, |unit| unit.edits);
                *index >= skip
            })
            .map(|(_, edit)| edit)
            .collect()
    }
}

// @lfy def/cli/data.lfy:ChangeSetUnit.reason
fn reason_named(name: &str) -> Option<Reason> {
    [
        Reason::Fresh,
        Reason::Changed,
        Reason::Requirements,
        Reason::Dependency,
        Reason::Violated,
        Reason::Requested,
        Reason::Touched,
    ]
    .into_iter()
    .find(|reason| reason.as_str() == name)
}

// @lfy def/cli/data.lfy:ChangeSet.foreignEdits
fn foreign_edit_to_json(edit: &ForeignEdit) -> Value {
    json!({
        "by": edit.by,
        "owner": edit.owner,
        "path": edit.path,
        "kind": edit.kind.as_str(),
        "region": edit.region.to_json(),
        "before": edit.before,
        "after": edit.after,
        "note": edit.note,
    })
}

fn foreign_edit_from_json(value: &Value) -> Option<ForeignEdit> {
    let object = value.as_object()?;
    let lines = |name: &str| -> Option<Vec<String>> {
        object
            .get(name)?
            .as_array()?
            .iter()
            .map(|line| line.as_str().map(str::to_string))
            .collect()
    };
    Some(ForeignEdit {
        by: object.get("by")?.as_str()?.to_string(),
        owner: object.get("owner")?.as_str()?.to_string(),
        path: object.get("path")?.as_str()?.to_string(),
        kind: ForeignEditKind::from_name(object.get("kind")?.as_str()?)?,
        region: Marker::from_json(object.get("region")?)?,
        before: lines("before")?,
        after: lines("after")?,
        note: object.get("note")?.as_str()?.to_string(),
    })
}

// @lfy def/cli/data.lfy:ChangeSet
#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("elfie-change-set-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn edit(by: &str, owner: &str, note: &str) -> ForeignEdit {
        ForeignEdit {
            by: by.to_string(),
            owner: owner.to_string(),
            path: "crates/a/src/b.rs".to_string(),
            kind: ForeignEditKind::Fix,
            region: Marker {
                output_line: 3,
                file: "def/b.lfy".to_string(),
                entity: Some("b".to_string()),
                line: 1,
                column: None,
                requirement: None,
                end: 5,
            },
            before: vec!["old".to_string()],
            after: vec!["new".to_string()],
            note: note.to_string(),
        }
    }

    fn accepted(stem: &str, hash: &str) -> ChangeSetUnit {
        ChangeSetUnit {
            state: UnitState::Accepted,
            ..ChangeSetUnit::planned(stem, Reason::Changed, hash, "r")
        }
    }

    fn set(units: Vec<ChangeSetUnit>) -> ChangeSet {
        ChangeSet {
            target: "rust".to_string(),
            directory: "/copy".to_string(),
            units,
            foreign_edits: Vec::new(),
        }
    }

    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:b655fcdcd7a4f6bc06eb2f5919bbd30b6986babf7942787f32a4b93cb22f3ef2
    #[test]
    fn change_set_file_holds_compact_json_of_the_kept_change_set() {
        let root = scratch("file");
        let mut kept = set(vec![accepted("a", "h"), ChangeSetUnit::planned("b", Reason::Touched, "", "")]);
        kept.foreign_edits.push(edit("a", "b", "fixed"));
        kept.save(&root).unwrap();
        let file = root.join("elfie-compile").join("cache").join("changes").join("rust.json");
        assert_eq!(ChangeSet::file(&root, "rust"), file);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains('\n'));
        assert_eq!(text, kept.to_json().to_string());
        assert_eq!(ChangeSet::load(&root, "rust"), Some(kept));
        std::fs::remove_dir_all(&root).unwrap();
    }

    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:d9f289811c4e43fa3bb96a1b1a7d01f546422d23b339e148ae7b61176238bef6
    #[test]
    fn change_set_remove_leaves_neither_file_nor_copy() {
        let root = scratch("remove");
        let copy = root.join("elfie-compile").join("cache").join("changes").join("rust").join("copy");
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join("a.rs"), "a").unwrap();
        let kept = ChangeSet { directory: copy.to_string_lossy().into_owned(), ..set(vec![]) };
        kept.save(&root).unwrap();
        kept.remove(&root).unwrap();
        assert!(!ChangeSet::file(&root, "rust").exists());
        assert!(!copy.exists());
        kept.remove(&root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }

    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:da099811d2a1e35b89a470128465e0c875995f7a6184fd85c63c02ccd2817239
    #[test]
    fn change_set_saved_stays_until_removed() {
        let root = scratch("kept");
        let kept = set(vec![accepted("a", "h")]);
        kept.save(&root).unwrap();
        let mut again = ChangeSet::load(&root, "rust").unwrap();
        again.units[0].state = UnitState::Rejected;
        again.save(&root).unwrap();
        assert_eq!(ChangeSet::load(&root, "rust"), Some(again));
        std::fs::remove_dir_all(&root).unwrap();
    }

    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:6b29187de3d79dc587549407b8213f3a4844ef711b59317cbd20c8d1a09e4808
    #[test]
    fn change_set_resumes_at_the_first_unit_not_done() {
        let mut kept = set(vec![
            accepted("a", "ha"),
            ChangeSetUnit { state: UnitState::Reviewed, ..accepted("b", "hb") },
            ChangeSetUnit { state: UnitState::Rejected, ..ChangeSetUnit::planned("c", Reason::Fresh, "", "") },
            ChangeSetUnit::planned("d", Reason::Fresh, "", ""),
        ]);
        let before = kept.units.clone();
        let now = [
            ChangeSetUnit::planned("a", Reason::Changed, "ha", "r"),
            ChangeSetUnit::planned("b", Reason::Changed, "hb", "r"),
            ChangeSetUnit::planned("c", Reason::Fresh, "hc", "r"),
            ChangeSetUnit::planned("d", Reason::Fresh, "hd", "r"),
        ];
        assert_eq!(kept.kept(&now), 4);
        assert_eq!(kept.resume(&now), Some(2));
        assert_eq!(kept.units, before);
    }

    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:77db4241a1867889bf369abe1c66f13af671b71c6c32b2e8966e9283f545fe1c
    #[test]
    fn change_set_runs_again_from_a_unit_whose_definitions_changed() {
        let mut kept = set(vec![accepted("a", "ha"), accepted("b", "hb"), accepted("c", "hc")]);
        let now = [
            ChangeSetUnit::planned("a", Reason::Changed, "ha", "r"),
            ChangeSetUnit::planned("b", Reason::Changed, "hb", "other"),
            ChangeSetUnit::planned("c", Reason::Changed, "hc", "r"),
        ];
        assert_eq!(kept.resume(&now), Some(1));
        assert_eq!(kept.units[0], accepted("a", "ha"));
        assert!(kept.units[1..].iter().all(|unit| unit.state == UnitState::Planned));
        assert_eq!(kept.units.iter().map(|u| u.stem.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    }

    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:8983cc59f8e506396e449579c05ad935909c8115eac5061aefda6fb014c41a2f
    #[test]
    fn change_set_runs_again_from_where_the_order_differs() {
        let mut kept = set(vec![accepted("a", "ha"), accepted("b", "hb"), accepted("c", "hc")]);
        let now = [
            ChangeSetUnit::planned("a", Reason::Changed, "ha", "r"),
            ChangeSetUnit::planned("x", Reason::Fresh, "hx", "r"),
            ChangeSetUnit::planned("b", Reason::Changed, "hb", "r"),
            ChangeSetUnit::planned("c", Reason::Changed, "hc", "r"),
        ];
        assert_eq!(kept.resume(&now), Some(1));
        assert_eq!(kept.units[0], accepted("a", "ha"));
        assert_eq!(kept.units.iter().map(|u| u.stem.as_str()).collect::<Vec<_>>(), ["a", "x", "b", "c"]);
        assert!(kept.units[1..].iter().all(|unit| unit.state == UnitState::Planned));
        assert_eq!(kept.units[1].reason, Reason::Fresh);
    }

    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:4f28781169f9b118953b17a5d1d2ef6b7936745d29530f30e2ad02d7ec23644c
    #[test]
    fn change_set_keeps_the_edits_of_a_unit_that_runs_again() {
        let mut kept = set(vec![accepted("a", "ha"), ChangeSetUnit::planned("b", Reason::Fresh, "", "")]);
        kept.accept(0, "ha", "r", vec![edit("a", "b", "first")]);
        let now = [
            ChangeSetUnit::planned("a", Reason::Changed, "changed", "r"),
            ChangeSetUnit::planned("b", Reason::Touched, "hb", "r"),
        ];
        assert_eq!(kept.resume(&now), Some(0));
        assert_eq!(kept.units[0].edits, 1);
        kept.accept(0, "changed", "r", vec![edit("a", "b", "second")]);
        let notes: Vec<&str> = kept.foreign_edits.iter().map(|e| e.note.as_str()).collect();
        assert_eq!(notes, ["first", "second"]);
        assert_eq!(kept.units[0].edits, 2);
    }

    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:e3f9c542851859c9bb86eee22b99337db1e0fb4cc60d7e58f3ff554e5d6ea747
    #[test]
    fn change_set_foreign_edits_only_grow_in_order() {
        let mut kept = set(vec![accepted("a", "ha"), accepted("b", "hb")]);
        kept.accept(0, "ha", "r", vec![edit("a", "b", "1"), edit("a", "b", "2")]);
        let before = kept.foreign_edits.clone();
        kept.accept(1, "hb", "r", vec![edit("b", "a", "3")]);
        kept.resume(&[ChangeSetUnit::planned("a", Reason::Changed, "other", "r")]);
        assert_eq!(kept.foreign_edits[..2], before[..]);
        assert_eq!(kept.foreign_edits.len(), 3);
    }

    // @lfy def/cli/data.lfy:ChangeSet#ChangeSet:ChangeSet:115db6923c273f848db7bd842f876ae3f28a068b801bb4956b81311fdb961444
    #[test]
    fn change_set_edits_since_follow_the_units_count() {
        let mut kept = set(vec![accepted("a", "ha"), accepted("b", "hb"), accepted("c", "hc")]);
        kept.accept(0, "ha", "r", vec![edit("a", "b", "1"), edit("a", "c", "2")]);
        kept.accept(1, "hb", "r", vec![edit("b", "c", "3")]);
        kept.accept(2, "hc", "r", vec![edit("c", "b", "4")]);
        let notes = |edits: Vec<&ForeignEdit>| edits.iter().map(|e| e.note.clone()).collect::<Vec<_>>();
        assert_eq!(notes(kept.edits_since("b")), ["4"]);
        assert_eq!(notes(kept.edits_since("c")), Vec::<String>::new());
        assert_eq!(notes(kept.edits_since("z")), Vec::<String>::new());
        assert_eq!(notes(kept.recorded_since()), ["4"]);
    }
}
