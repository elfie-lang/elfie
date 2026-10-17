//! Compiled from `def/mcp/main.lfy`: the Elfie agent server.
//!
//! One tool per fn of the definition that carries `tool`, spoken over the Model Context
//! Protocol on standard input and output, or over HTTP on this machine for the length of
//! one compile or verify. The server never writes a file: during compilation the agent
//! writes the outputs itself and asks the server to check them, so one path exists for
//! writing and the CLI owns it.

pub mod data;
pub mod traits; // @lfy def/mcp/traits.lfy:tool

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use elfie_core::generation::{
    self, Batch, Marker, Move, Output, Plan, Request, Requirement, Review, ReviewStatus, Rewording,
    SourceMap, Submission, SubmittedOutcome, Unit,
};
use elfie_core::interpret::{self, LoweredCriterion, LoweredNode, LoweredTest, Program};
use elfie_core::model::{self, Criterion, EntityId, FileId, Model, NodeRef, Operation, Problem, SymbolKind};
use elfie_core::query::{self, Outline, Position, Range};
use elfie_core::workspace::{self, Target, Workspace};
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};

pub use data::{
    Binding, CommandRun, GlobalReview, Host, Role, Session, SessionRecord, Stamp, Tool,
    ToolArgument, ToolResult,
};
pub use traits::{Arguments, Registered};

/// The project layout file; a change to it loads the workspace again.
const MANIFEST: &str = "elfie.json";

/// The exit code when the server itself cannot run, as the CLI reads it.
const FAILURE: i32 = 3;

// ---- the submodules of this unit ----------------------------------------------------

mod commands;
mod compiling;
mod planning;
mod reading;
mod regions;
mod registry;
mod serving;
mod verdicts;

pub use commands::{build, format_check, lint, test};
pub use compiling::{dependency, mark, submit, task};
pub use planning::{changes, check, request, units};
pub use reading::{entity, find, grammar, outline, problems, references};
pub use regions::{output, search, source};
pub use registry::tools;
pub use serving::{host, serve};
pub use verdicts::{global_review, report, report_many, review};

// ---- the session ------------------------------------------------------------------

/// The stamp of one file on disk, or `None` when there is no file there.
///
/// A file is noticed by either its modification time or its size, so an edit that keeps
/// both is not seen by the next call.
// @lfy def/mcp/main.lfy:serve#serve:serve:a0e0712c211c914b7d1655d1a85d5aa373db8b9d94524624d54554e2501f845d
fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok(), metadata.len()))
}

/// A path under the root, relative to it, with forward slashes.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every `.lfy` file under a directory.
fn walk_sources(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "lfy") {
            out.push(path);
        }
    }
}

/// Every file of the program: the files bound, every `.lfy` file under each of the source
/// directories (so that a new file is noticed), and the manifest.
// @lfy def/mcp/main.lfy:serve#serve:serve:000dfc10a663967f375731bea3f28fdece3eae3b1e8afe2b88d285adc7ca4d1c
fn watched(workspace: &Workspace) -> BTreeSet<String> {
    let mut paths: BTreeSet<String> = workspace.files.iter().map(|file| file.path.clone()).collect();
    let mut found = Vec::new();
    for directory in &workspace.source_directories {
        walk_sources(&workspace.root.join(directory), &mut found);
    }
    for path in found {
        paths.insert(relative(&workspace.root, &path));
    }
    paths.insert(MANIFEST.to_string());
    paths
}

/// The stamp of every file of the program that exists on disk, by path.
fn stamps(workspace: &Workspace) -> BTreeMap<String, Stamp> {
    watched(workspace)
        .into_iter()
        .filter_map(|path| stamp(&workspace.root.join(&path)).map(|stamp| (path, stamp)))
        .collect()
}

impl Session {
    /// A session whose workspace is the project at `root`, loaded, with every file of it
    /// stamped as read. It is bound to nothing, so it reads the files on disk and holds no
    /// review.
    // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
    // @lfy def/mcp/main.lfy:serve#serve:serve:8579f2d17bf0b508fda59fe780ac73d54d3271fe8f9994ccd60bd5f66d026331
    pub fn load(root: &Path) -> Session {
        let workspace = workspace::load(root);
        let seen = stamps(&workspace);
        Session {
            workspace,
            seen,
            binding: None,
            reviews: Mutex::new(Vec::new()),
            submission: Mutex::new(None),
        }
    }

    /// Every review `elfie_report` or `elfie_reportMany` has recorded in the session, in the
    /// order recorded.
    pub fn recorded(&self) -> Vec<Review> {
        self.reviews.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    /// What the session recorded: the reviews `elfie_report` and `elfie_reportMany` kept, in
    /// the order recorded, and the outcome `elfie_submit` last kept.
    // @lfy def/mcp/main.lfy:host#host:host:64c0df55947a7e7a600bb0796c4715f740afcfda41cdd6f86b4cb408c4705713
    pub fn record(&self) -> SessionRecord {
        SessionRecord {
            reviews: self.recorded(),
            submission: self
                .submission
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        }
    }

    /// What runs first when a call arrives: every file of the program whose modification
    /// time or size differs from what was seen is re-read through `workspace::change`
    /// with no text, and a file that vanished the same way.
    ///
    /// When `elfie.json` has changed since the last call, the workspace is loaded again
    /// instead, before the tool runs.
    // @lfy def/mcp/main.lfy:serve#serve:serve:995535b9c7a11554f96d88793004bc092de31525b26cd0ca15ce449e17d8e386
    pub fn refresh(&mut self) {
        let now = stamps(&self.workspace);
        // elfie.json has changed since the last call.
        // @lfy def/mcp/main.lfy:serve#serve:serve:4c5aeb9423c7e9206e75798da9c197b91b439325adda5776e09a2061d144988a
        if now.get(MANIFEST) != self.seen.get(MANIFEST) {
            self.workspace = workspace::load(&self.workspace.root);
            self.seen = stamps(&self.workspace);
            return;
        }
        // A `.lfy` file under one of the source directories that is not in the program and that no
        // earlier call saw has no stamp among those seen, so it is among the paths that
        // differ; a file whose modification time or size differs is too, and so is one that
        // has vanished, which has no stamp now.
        // @lfy def/mcp/main.lfy:serve#serve:serve:1893a7a1afe4f16bc76a47596614b6645e060072995b9387c9386905037fe959
        let changed: Vec<String> = now
            .keys()
            .chain(self.seen.keys())
            .filter(|path| now.get(*path) != self.seen.get(*path))
            .cloned()
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        // Each is read through the loader with no text, which reads it from disk when it is
        // there and drops it from the program when it is not.
        // @lfy def/mcp/main.lfy:serve#serve:serve:000dfc10a663967f375731bea3f28fdece3eae3b1e8afe2b88d285adc7ca4d1c
        // @lfy def/mcp/main.lfy:serve#serve:serve:01ce18f671ec478741555e6ff345d61120f118da6209c46d18cd01b3cbff3e6a
        for path in &changed {
            self.workspace = workspace::change(&self.workspace, path, None);
        }
        if !changed.is_empty() {
            self.seen = stamps(&self.workspace);
        }
    }
}

// ---- what more than one submodule reads ---------------------------------------------

/// The identifier of an entity, or `anonymous`.
fn identifier_of(model: &Model, entity: EntityId) -> String {
    model.entities[entity]
        .identifier
        .clone()
        .unwrap_or_else(|| "anonymous".to_string())
}

/// One diagnostic as `problems` spells it: the file, a colon, the line, a colon, the
/// column, a colon, a space, the stage, a space, the severity, a colon, a space, and the
/// message.
// @lfy def/mcp/main.lfy:problems#problems:problems:c8c8b536f1269d0fb4bc9adb64fbce292df0715fa51d5d524a18f16685af7bf6
fn diagnostic_line(file: &str, line: usize, column: usize, stage: &str, severity: &str, message: &str) -> String {
    format!("{file}:{line}:{column}: {stage} {severity}: {message}")
}

/// One lowered test as one line: its input and its expectation, as spelled.
// @lfy def/mcp/main.lfy:source#source:source:bab27e2f0ed21f719d22e545c1c874a283b18a0ff5a8979b2a8bf9d324519b11
fn lowered_test_text(test: &LoweredTest) -> String {
    format!("Input `{}` gives `{}`", test.input_text.trim(), test.expect_text.trim())
}

/// The identifier of the target a batch's units are of; a batch holds units of one target.
fn batch_target<'w>(workspace: &'w Workspace, plan: &Plan, batch: &Batch) -> &'w str {
    match batch.units.first() {
        Some(&unit) => workspace.targets[plan.units[unit].target].identifier.as_str(),
        None => "",
    }
}

/// The targets of a session: the target of the units of the binding's batch when it has one,
/// and every target of the workspace when it has none.
// @lfy def/mcp/main.lfy:host#host:host:f5d226a776398720183cca8da8ecac0ec6273fcfb6503d5533239adec7e2322b
fn session_targets<'w>(workspace: &'w Workspace, binding: Option<&Binding>) -> Vec<&'w Target> {
    // @lfy def/mcp/main.lfy:host#host:host:f5d226a776398720183cca8da8ecac0ec6273fcfb6503d5533239adec7e2322b
    if let Some(binding) = binding
        && let Some(batch) = &binding.batch
    {
        let mut found: Vec<&Target> = Vec::new();
        for &unit in &batch.units {
            if let Some(unit) = binding.plan.units.get(unit)
                && let Some(target) = workspace.targets.get(unit.target)
                && !found.iter().any(|known| known.identifier == target.identifier)
            {
                found.push(target);
            }
        }
        return found;
    }
    // @lfy def/mcp/main.lfy:host#host:host:f5d226a776398720183cca8da8ecac0ec6273fcfb6503d5533239adec7e2322b
    workspace.targets.iter().collect()
}

/// The name of an operation's member, as `Operation` spells it: the word its value begins
/// with, before the colon that introduces what the operation is for.
// @lfy def/mcp/main.lfy:build#build:commandTool:394aef9de6082d12b6f9d58c8eaae738d8ab0f6aef3341b5fda12620fe6500a6
fn operation_name(operation: Operation) -> &'static str {
    operation.value().split(':').next().unwrap_or_default()
}

/// The command of a target for one operation whose line is set, when it has one.
// @lfy def/mcp/main.lfy:build#build:commandTool:654d31e36e3f112d06aa532374f30fda36190672b3a323580052a1c0f96e1401
// @lfy def/mcp/main.lfy:test#test:commandTool:82d963f45b5a888916a03e865844e4c1961de319774b2584f65489dee966cdb5
// @lfy def/mcp/main.lfy:lint#lint:commandTool:db94076681d45420de07709b25f27480d746751609aa1bb42a77c77d453001eb
// @lfy def/mcp/main.lfy:format#format:commandTool:a09c5e43d4c38d2e3196576c80db3cf1cb918134842760d0c7e7aa134b552a6b
fn line_for(target: &Target, operation: Operation) -> Option<&str> {
    target
        .commands
        .iter()
        .find(|command| command.operation == operation)
        .and_then(|command| command.line.as_deref())
}

/// Every file under a directory, recursively, with no directory passed over.
// Decision: no directory is skipped, however it is named: the criteria that read this say
// every file under the output directory, recursively, and a file a build left behind under,
// say, `out/target/` is under it like any other. What `output` calls an output file is
// narrower, and `regions::is_output_file` leaves those directories out for itself.
fn walk_all(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_all(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// The files under the target's output directory, each read from disk, split into the ones
/// that carry a marker for the unit's file and every other one.
///
/// The first are the unit's outputs, which acceptance judges and records a map for; the rest
/// are read for the markers they hold and nothing else, so a region of another unit that
/// moved out of an output of this one is still found where it went.
// @lfy def/mcp/main.lfy:check#check:check:860750a195c0ff4eb9ed95b87939e831cae5a22fecd5d7efe17858c8f124ff9d
fn outputs_of(workspace: &Workspace, plan: &Plan, unit: usize) -> (Vec<Output>, Vec<Output>) {
    let unit = &plan.units[unit];
    let target = &workspace.targets[unit.target];
    let file = &workspace.files[unit.file].path;
    let mut paths = Vec::new();
    walk_all(&workspace.root.join(&target.output_directory), &mut paths);
    paths.sort();
    let mut found = Vec::new();
    let mut elsewhere = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let output = Output {
            path: relative(&workspace.root, &path),
            text,
        };
        if generation::parse_markers(&output.text).iter().any(|marker| &marker.file == file) {
            found.push(output);
        } else {
            // @lfy def/mcp/main.lfy:check#check:check:860750a195c0ff4eb9ed95b87939e831cae5a22fecd5d7efe17858c8f124ff9d
            elsewhere.push(output);
        }
    }
    (found, elsewhere)
}

/// The identifiers of every target, joined.
fn known_targets(workspace: &Workspace) -> String {
    let names: Vec<&str> = workspace.targets.iter().map(|target| target.identifier.as_str()).collect();
    if names.is_empty() { "none".to_string() } else { names.join(", ") }
}

/// An error naming the known targets when the target given is not one.
// @lfy def/mcp/main.lfy:units#units:units:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
fn check_target(workspace: &Workspace, target: Option<&str>) -> Result<(), String> {
    match target {
        Some(target) if !workspace.targets.iter().any(|known| known.identifier == target) => {
            Err(format!("{target} is not a target; the targets are: {}", known_targets(workspace)))
        }
        _ => Ok(()),
    }
}

/// A plan, the program it was planned from, and the source maps it was planned against:
/// what every tool that reads the compiler's work is given.
// Decision: `generation::plan` takes the workspace and gives back the program lowered from
// it beside the plan, and every later call reads that program rather than the workspace, so
// the three travel together.
// @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
struct Planned {
    /// The workspace lowered, which every request and review reads.
    program: Program,
    /// The plan.
    plan: Plan,
    /// The source maps the plan was made against: what the last acceptance recorded.
    maps: Vec<SourceMap>,
}

impl Planned {
    /// The workspace the plan's units index into: the one the program was lowered from.
    fn workspace(&self) -> &Workspace {
        &self.program.workspace
    }
}

/// Every output file of one target, by path relative to the workspace root, in path order:
/// every file, recursively, under the target's output directory whose extension is among the
/// target's, whatever the directories on the way there are named.
///
/// The root of a hosted session's workspace is its binding's directory, so these are the
/// files under that directory and the paths are relative to it.
// @lfy def/mcp/main.lfy:output#output:output:e3ada846896a5d797c74467acb25cc6ab9c9f3667f13eae813f8169d01373c8f
// @lfy def/mcp/main.lfy:mark#mark:mark:20e94fd8b37ced24d2351848435ef39376d3fc15eeb5d4e64ccffd5f95cb4053
// @lfy def/mcp/main.lfy:search#search:search:c17cae68f2b3424b9e51c005c2ca4bc5521e610abeb02c841daa9b067c1fa147
fn output_files(workspace: &Workspace, target: &Target) -> Vec<String> {
    let mut paths = Vec::new();
    walk_all(&workspace.root.join(&target.output_directory), &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| target.extensions.iter().any(|known| known == extension))
        })
        .map(|path| relative(&workspace.root, &path))
        .collect()
}

/// The program of a session's binding, with the workspace read at the session's root: the
/// workspace the binding's plan indexes into, every path of it joining to the directory the
/// run is in.
///
/// A run in a batch's own copy reads the outputs there rather than in the root the compile
/// was lowered from, so the session's workspace stands in for the program's where the two
/// roots differ, and the program is read as it is where they do not, which is every run
/// outside a copy.
fn program_at_root<'a>(session: &'a Session, binding: &'a Binding) -> Cow<'a, Program> {
    if binding.program.workspace.root == session.workspace.root {
        return Cow::Borrowed(&binding.program);
    }
    let mut program = binding.program.clone();
    program.workspace = session.workspace.clone();
    Cow::Owned(program)
}

/// The source maps of the units of a compiler session's batch, read from the markers their
/// target's output files hold now rather than from where the last acceptance recorded them.
///
/// A compiler has just written and edited those files, so it must see its regions where they
/// are now. For a unit whose map file holds maps, `generation::rederive` reads them: the
/// markers are those the files hold when the call arrives, and everything else each map
/// carries — what the unit's text and requirements hashed to, which ids were answered, when
/// it was accepted — is carried over from the maps the file holds, so the unit plans,
/// changes, and requests as the compile planned it.
///
/// A unit whose map file holds none has nothing to carry over, so `rederive` is given one
/// map of the unit whose every other member is empty: it then reads the markers of each
/// output file that holds one of the unit's own exactly as it reads them for a unit that has
/// maps, and carries those empty members over to each. It gives that one map back when no
/// output file holds a marker of the unit, and a map for no output is no map at all.
///
/// `recorded` is what the unit's map file holds: the maps read for it that these stand in
/// for, as the session read them, so that a target whose maps are still recorded under its
/// output directory is read there as everywhere else.
// @lfy def/mcp/main.lfy:host#host:host:7a6f6d82c62937a6200fb29971d7e37887f517b297d8c5a98583a469dfa4b7aa
// @lfy def/mcp/main.lfy:host#host:host:8b0ee6f07bbcc53017d5574c5bc37d7cbf2fa810500d3cf286603559d1e4ecd5
fn own_source_maps(session: &Session, target: Option<&str>, recorded: &[SourceMap]) -> Vec<SourceMap> {
    let Some(binding) = &session.binding else { return Vec::new() };
    let Some(batch) = &binding.batch else { return Vec::new() };
    let workspace = &session.workspace;
    let program = program_at_root(session, binding);
    let mut found: Vec<SourceMap> = Vec::new();
    for &index in &batch.units {
        let Some(unit) = binding.plan.units.get(index) else { continue };
        let Some(of_target) = workspace.targets.get(unit.target) else { continue };
        if target.is_some_and(|target| of_target.identifier != target) {
            continue;
        }
        let Some(file) = workspace.files.get(unit.file) else { continue };
        let held: Vec<SourceMap> = recorded
            .iter()
            .filter(|map| map.target == of_target.identifier && map.source == file.path)
            .cloned()
            .collect();
        // @lfy def/mcp/main.lfy:host#host:host:7a6f6d82c62937a6200fb29971d7e37887f517b297d8c5a98583a469dfa4b7aa
        if !held.is_empty() {
            found.extend(generation::rederive(&program, &binding.plan, index, &held));
            continue;
        }
        // @lfy def/mcp/main.lfy:host#host:host:8b0ee6f07bbcc53017d5574c5bc37d7cbf2fa810500d3cf286603559d1e4ecd5
        let empty = SourceMap {
            target: of_target.identifier.clone(),
            output: String::new(),
            source: file.path.clone(),
            hash: String::new(),
            requirements: String::new(),
            answered: Vec::new(),
            signature: String::new(),
            text: String::new(),
            dependencies: BTreeMap::new(),
            generated: String::new(),
            markers: Vec::new(),
        };
        // An output file is one whose extension is among the target's, as every other walk
        // of an output directory reads one.
        // @lfy def/mcp/main.lfy:host#host:host:8b0ee6f07bbcc53017d5574c5bc37d7cbf2fa810500d3cf286603559d1e4ecd5
        let of_output = |map: &SourceMap| {
            Path::new(&map.output)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| of_target.extensions.iter().any(|known| known == extension))
        };
        found.extend(
            generation::rederive(&program, &binding.plan, index, &[empty])
                .into_iter()
                .filter(of_output),
        );
    }
    found
}

/// [`generation::source_maps_of`] the session's workspace, with the maps the session's
/// review was made with standing in for the recorded maps of the same target and source, and
/// with the maps of a compiler session's own units read from its outputs as they are now.
///
/// A verifier judges the outputs its review was made against, so where the binding carries
/// a map for a target and a source, every map read for that pair is left out and the
/// binding's maps are read in their place; a map the binding carries that replaces none is
/// read after the rest. A compiler reads its own units' regions from the files it has just
/// written, so every map recorded for one of its batch's sources is left out and
/// [`own_source_maps`] is read in their place. With no binding, what the last acceptance
/// recorded under the session's root is all that is read.
// @lfy def/mcp/main.lfy:output#output:output:18bbd05acd9d7ef32f170d4713899aaf2e4c19f93da25c7feae2938e50414fc1
// @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
// @lfy def/mcp/main.lfy:host#host:host:1d7d3545dd3f2db19683b885c877989d8409d11cfdadaa4e2c381a4b9ef084ba
// @lfy def/mcp/main.lfy:host#host:host:232c0725ae5219330822ed9664cc87be38c4591303cbc3dead664044e09b75eb
fn source_maps_of(session: &Session, target: Option<&str>) -> Vec<SourceMap> {
    let mut read = generation::source_maps_of(&session.workspace, target);
    // A compiler session's own units are read from the markers their outputs hold now.
    // @lfy def/mcp/main.lfy:host#host:host:1d7d3545dd3f2db19683b885c877989d8409d11cfdadaa4e2c381a4b9ef084ba
    if let Some(binding) = &session.binding
        && binding.role == Role::Compiler
        && let Some(batch) = &binding.batch
    {
        let own: Vec<String> = batch
            .units
            .iter()
            .filter_map(|&index| binding.plan.units.get(index))
            .filter_map(|unit| session.workspace.files.get(unit.file))
            .map(|file| file.path.clone())
            .collect();
        // @lfy def/mcp/main.lfy:host#host:host:1d7d3545dd3f2db19683b885c877989d8409d11cfdadaa4e2c381a4b9ef084ba
        let left_out: Vec<SourceMap> = read.iter().filter(|map| own.contains(&map.source)).cloned().collect();
        read.retain(|map| !own.contains(&map.source));
        // @lfy def/mcp/main.lfy:host#host:host:7a6f6d82c62937a6200fb29971d7e37887f517b297d8c5a98583a469dfa4b7aa
        read.extend(own_source_maps(session, target, &left_out));
    }
    let given: &[SourceMap] = match &session.binding {
        Some(binding) => &binding.source_maps,
        None => &[],
    };
    if given.is_empty() {
        return read;
    }
    let of_target = |map: &SourceMap| target.is_none_or(|target| map.target == target);
    let stands_for = |map: &SourceMap| given.iter().any(|one| one.target == map.target && one.source == map.source);
    let mut found: Vec<SourceMap> = Vec::new();
    let mut taken: Vec<(String, String)> = Vec::new();
    // @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
    for map in read {
        if !stands_for(&map) {
            found.push(map);
            continue;
        }
        let pair = (map.target.clone(), map.source.clone());
        if taken.contains(&pair) {
            continue;
        }
        found.extend(
            given
                .iter()
                .filter(|one| one.target == pair.0 && one.source == pair.1 && of_target(one))
                .cloned(),
        );
        taken.push(pair);
    }
    // A map the binding carries that stood in for none is read after the rest.
    // @lfy def/mcp/main.lfy:host#host:host:6f8405c8e6f3e4591069a7d71ce8930df3cd097e586d761bdaa8bb9a2df6ce2e
    found.extend(
        given
            .iter()
            .filter(|one| of_target(one) && !taken.contains(&(one.target.clone(), one.source.clone())))
            .cloned(),
    );
    found
}

/// The plan of the session's workspace with [`source_maps_of`] it, and as its violated
/// units the violated units of the last [`GlobalReview`].
// Decision: a unit's outputs are read off the plan, and the violated units are an input of
// planning, so the plan is made once with no violated unit and again only when the last
// global review names one; a project with none, which is every project until a global
// review finds one, is planned once.
// @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
fn plan_of(session: &Session) -> Planned {
    let workspace = &session.workspace;
    let maps = source_maps_of(session, None); // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
    let (program, plan) = generation::plan(workspace.clone(), &maps, &[], &[]);
    // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
    let stems = GlobalReview::read(&workspace.root).map(|last| last.violated_stems(&plan)).unwrap_or_default();
    if stems.is_empty() {
        return Planned { program, plan, maps };
    }
    // @lfy def/mcp/main.lfy:units#units:units:e35cb874630f014bcf475b9b10f74413ed36fe8b669e6a8194fd1ca57d8cf251
    let (program, plan) = generation::plan(workspace.clone(), &maps, &[], &stems);
    Planned { program, plan, maps }
}

/// Nothing when a session may be asked about a batch, and an error naming the one batch it
/// acts on — or the global review, when it is bound to no batch — when it may not.
// @lfy def/mcp/main.lfy:request#request:request:f7ef0e526d107bc01f393317930dc6691e83904a9ec731aef3e304884867cfe0
// @lfy def/mcp/main.lfy:review#review:review:0dcb77dad8ef4db5102c1bd7eeaec630f239dc244db5eee8132f8c57d77677ea
fn own_batch(session: &Session, identifier: &str) -> Result<(), String> {
    let Some(binding) = &session.binding else {
        return Ok(());
    };
    match &binding.batch {
        Some(batch) if batch.identifier == identifier => Ok(()),
        Some(batch) => Err(format!(
            "this session acts only on its own batch, {}; it was asked about {identifier}",
            batch.identifier
        )),
        None => Err(format!(
            "this session acts only on the global review; it was asked about the batch {identifier}"
        )),
    }
}

/// Nothing when a session may be asked about a unit with a stem, and an error naming the one
/// batch it acts on — or the global review — when it may not.
// @lfy def/mcp/main.lfy:check#check:check:9c073d6b09e272ea0d5a712167c00378477b35e103dad16445f0d53b88d7bc30
// @lfy def/mcp/main.lfy:changes#changes:changes:e97791dbc253b4037be4b5a6a54aa430027a02f99710070c689449608a3affe1
fn own_unit(session: &Session, stem: &str) -> Result<(), String> {
    let Some(binding) = &session.binding else {
        return Ok(());
    };
    match &binding.batch {
        Some(batch)
            if batch
                .units
                .iter()
                .filter_map(|&unit| binding.plan.units.get(unit))
                .any(|unit| unit.stem == stem) =>
        {
            Ok(())
        }
        Some(batch) => Err(format!(
            "this session acts only on its own batch, {}; it was asked about the unit {stem}",
            batch.identifier
        )),
        None => Err(format!(
            "this session acts only on the global review; it was asked about the unit {stem}"
        )),
    }
}

/// The unit with a stem, in the target given or the only target it is in; an error
/// saying which when no unit has that stem or the target is ambiguous.
// @lfy def/mcp/main.lfy:check
fn resolve_unit(workspace: &Workspace, plan: &Plan, stem: &str, target: Option<&str>) -> Result<usize, String> {
    check_target(workspace, target)?;
    let matches: Vec<usize> = (0..plan.units.len())
        .filter(|&index| {
            let unit = &plan.units[index];
            unit.stem == stem && target.is_none_or(|target| workspace.targets[unit.target].identifier == target)
        })
        .collect();
    match matches.as_slice() {
        [] => Err(match target {
            Some(target) => format!("no unit of the target {target} has the stem {stem}; elfie_units lists them"),
            None => format!("no unit has the stem {stem}; elfie_units lists them"),
        }),
        [index] => Ok(*index),
        many => {
            let targets: Vec<&str> = many.iter().map(|&index| workspace.targets[plan.units[index].target].identifier.as_str()).collect();
            Err(format!("the unit {stem} is in more than one target ({}); name one with target", targets.join(", ")))
        }
    }
}

/// The batch with an identifier, or the one holding a unit with a stem, in the target
/// given or the only target it is in; an error saying which when no batch has that
/// identifier or holds a unit with that stem, or the target is ambiguous.
///
/// A name that is the identifier of one batch and the stem of a unit of another means the
/// batch whose identifier it is: the identifiers are tried first and the stems only when
/// no batch is named by its identifier.
// @lfy def/mcp/main.lfy:request#request:request:a7e16dee761e4f416f1040c2c3f90c5e87e1352241e235f0eea4146033eccc99
fn resolve_batch(workspace: &Workspace, plan: &Plan, name: &str, target: Option<&str>) -> Result<usize, String> {
    check_target(workspace, target)?;
    let of_target = |batch: &Batch| target.is_none_or(|target| batch_target(workspace, plan, batch) == target);
    // @lfy def/mcp/main.lfy:request#request:request:a7e16dee761e4f416f1040c2c3f90c5e87e1352241e235f0eea4146033eccc99
    let mut matches: Vec<usize> = (0..plan.batches.len())
        .filter(|&index| plan.batches[index].identifier == name && of_target(&plan.batches[index]))
        .collect();
    if matches.is_empty() {
        matches = (0..plan.batches.len())
            .filter(|&index| {
                let batch = &plan.batches[index];
                batch.units.iter().any(|&unit| plan.units[unit].stem == name) && of_target(batch)
            })
            .collect();
    }
    match matches.as_slice() {
        // @lfy def/mcp/main.lfy:request#request:request:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
        [] => Err(match target {
            Some(target) => format!(
                "no batch of the target {target} has the identifier {name} or holds a unit with the stem {name}; elfie_units lists them"
            ),
            None => format!(
                "no batch has the identifier {name} or holds a unit with the stem {name}; elfie_units lists them"
            ),
        }),
        [index] => Ok(*index),
        // @lfy def/mcp/main.lfy:request#request:request:ca0a25d6a947aa41d3cbf9399a371f681224987d6109d85e16a0b7f8a85db14c
        many => {
            let targets: Vec<&str> = many
                .iter()
                .map(|&index| batch_target(workspace, plan, &plan.batches[index]))
                .collect();
            Err(format!("the batch {name} is in more than one target ({}); name one with target", targets.join(", ")))
        }
    }
}

/// The request for a batch, with the existing outputs of its units read from disk and no
/// previous source.
// @lfy def/mcp/main.lfy:request#request:request:92537a4ab2aac8027576852406e446fcb12c7b5b87c96095a386c6d7dac101ed
fn request_for(program: &Program, plan: &Plan, batch: &Batch) -> Request {
    let existing: Vec<Output> = batch
        .units
        .iter()
        .flat_map(|&unit| plan.units[unit].outputs.iter())
        .filter_map(|map| {
            fs::read_to_string(program.workspace.root.join(&map.output))
                .ok()
                .map(|text| Output { path: map.output.clone(), text })
        })
        .collect();
    // The request is made here rather than handed to the session, so it is no hosted
    // compiler's request and quotes the interfaces of its dependencies whole. No unit is
    // given a previous source, so none of them is given a hash-matched one either.
    // @lfy def/mcp/main.lfy:request#request:request:92537a4ab2aac8027576852406e446fcb12c7b5b87c96095a386c6d7dac101ed
    generation::request(program, plan, batch, &existing, &BTreeMap::new(), false, &[])
}

/// The request of the batch that holds a unit.
// Decision: a unit that is up to date is in no batch, because only planned units are
// batched; it is requested as a batch of its own, which is the request its batch would
// be, so a check of an up-to-date unit answers rather than failing.
// @lfy def/mcp/main.lfy:check#check:check:860750a195c0ff4eb9ed95b87939e831cae5a22fecd5d7efe17858c8f124ff9d
fn request_of_unit(program: &Program, plan: &Plan, unit: usize) -> Request {
    match plan.batches.iter().find(|batch| batch.units.contains(&unit)) {
        Some(batch) => request_for(program, plan, batch),
        None => request_for(
            program,
            plan,
            &Batch {
                units: vec![unit],
                identifier: plan.units[unit].stem.clone(),
            },
        ),
    }
}

/// One marker spelled as a marker line spells it after `@lfy` and a space.
// @lfy def/mcp/main.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
// @lfy def/mcp/main.lfy:search#search:search:fa10d4c06de0e21bae6488a493c889c66e5ea9d22f426fbcf0488df26d15e041
fn marker_text(marker: &Marker) -> String {
    match (&marker.entity, marker.column) {
        (Some(entity), _) => match &marker.requirement {
            Some(requirement) => format!("{}:{entity}#{requirement}", marker.file),
            None => format!("{}:{entity}", marker.file),
        },
        (None, Some(column)) => format!("{}:{}:{column}", marker.file, marker.line),
        (None, None) => format!("{}:{}", marker.file, marker.line),
    }
}

/// Every marker one of this batch's output files holds, read as acceptance reads them.
// @lfy def/mcp/main.lfy:mark#mark:mark:c3d247ab4b165bf17b1ba969469f280619af5c95535eec67c2881e691f8a9a83
fn markers_of(text: &str) -> Vec<Marker> {
    generation::parse_markers(text)
}

/// Every marker of one line of an output, as acceptance reads them.
// @lfy def/mcp/main.lfy:mark#mark:mark:c3d247ab4b165bf17b1ba969469f280619af5c95535eec67c2881e691f8a9a83
fn markers_on(markers: &[Marker], line: usize) -> Vec<&Marker> {
    markers.iter().filter(|marker| marker.output_line == line).collect()
}

/// How many characters of one line are given before the rest are left out.
// @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
const KEPT_WIDTH: usize = 500;

/// What marks a line that was cut short.
// @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
const CUT: char = '…';

/// A line kept at most `width` Unicode characters, followed by `…` when it was cut short.
// @lfy def/mcp/main.lfy:search#search:search:68207e69227f6ee9bb5e6b5a46a769482b26d625a8589e28cdcef599d5b1756f
fn shortened(line: &str, width: usize) -> String {
    let mut characters = line.chars();
    let kept: String = characters.by_ref().take(width).collect();
    if characters.next().is_some() {
        format!("{kept}{CUT}")
    } else {
        kept
    }
}

// ---- what the tests of this unit are built from -------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use elfie_core::generation::{Review, ReviewStatus};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

    use super::*;
    use crate::regions::requirements_of;
    use crate::serving::{Hosting, TOKEN_BYTES, call_tool, serve_on};

    mod commands_tests;
    mod compiling_tests;
    mod planning_tests;
    mod reading_tests;
    mod regions_tests;
    mod serving_tests;
    mod verdicts_tests;

    /// The directory of the library a fixture carries, relative to its root.
    const LIBRARY_ROOT: &str = "lib";
    /// The main file of the library a fixture carries: the prelude, which brings the
    /// vocabulary a target is declared with into the scope of every file of the project.
    const LIBRARY: &str = "use \"./prelude/builtin\";\nuse \"./prelude/entity\";\n\
                           use \"./prelude/trait\";\nuse \"./criteria/main\";\n\
                           use \"./target/main\";\n";
    /// The trait a builtin carries, which the library's own declarations need.
    const LIBRARY_BUILTIN: &str = "trait builtin: `Performed by the compiler` { }\n";
    /// The context layer every entity is seen through: the lists a body adds to live
    /// there, and `apply` and `layer` are members of a trait's kind rather than names
    /// nothing declares.
    const LIBRARY_ENTITY: &str = "\
d Entity: `A declared thing seen through its context layer` {
  $knowledge: `What the compiler is given to read for it` = object[];
  $commands: `Shell commands for its operations` = object[];
  $targets: `The targets it is built for` = object[];
}
d Trait extends Entity: `A trait seen through its context layer` {
  $apply: `Applies the trait to a target and returns the trait` = (target: Entity) => Trait;
  $layer: `The trait as a layer of a target, with arguments for its parameters` = (...arguments: string[]) => object;
}
";
    /// `layer`, which chooses a trait as a layer with arguments.
    const LIBRARY_TRAIT: &str = "use \"./builtin\";\n\
                                 fn layer(subject: trait, ...arguments: string[]) is builtin: \
                                 `A trait chosen as a layer of a target` => object;\n";
    /// The enums a knowledge item and a command name.
    const LIBRARY_CRITERIA: &str = "\
enum KnowledgeKind: `What a piece of knowledge is` {
  reference = `documentation to read`,
  example = `working code to imitate`,
  definition = `Elfie source that defines it`,
  tool = `a tool of the agent server`,
}
enum Operation: `What a command is for` {
  install = `install`,
  add = `add`,
  build = `build`,
  test = `test`,
  lint = `lint`,
  format = `format`,
  run = `run`,
}
";
    /// `Target`, the roles its slots take, and the function that gives a record written
    /// in braces its type, as the package `elfie` declares them.
    const LIBRARY_TARGET: &str = "\
use \"../prelude/builtin\";
trait target: `What every layer of a target is` {
  $markerComment: `How a line comment begins` = string;
  $provides: `Capabilities this layer gives its target` = string[];
  $requires: `Capabilities some layer of its target must provide` = string[];
  $outputExtensions: `Extensions besides the language's own` = string[];
  .provides = [];
  .requires = [];
  .outputExtensions = [];
}
trait targetLanguage extends target: `What its code is written in` {
  $fileExtension: `The extension of its files` = string;
}
trait targetRuntime extends target: `What runs its code` { }
trait targetPlatform extends target: `Where it runs` { }
trait targetEcosystem extends target: `Where its dependencies come from` {
  $ecosystem: `What that ecosystem is called` = string;
  $scriptRunner: `What runs a script its manifest names` = string;
}
trait targetFramework extends target: `What its code is built on` { }
trait targetInterface extends target: `What it offers` { }
trait targetLayout extends target: `Where its files go` { }
d Target is builtin: `One artifact the project is built into` {
  $output: `Where everything it writes goes` = string;
  $language: `What its code is written in` = trait;
  $runtime: `What runs its code` = trait | undefined;
  $platforms: `Where it runs` = trait[] | undefined;
  $ecosystem: `Where its dependencies come from` = trait | undefined;
  $frameworks: `What its code is built on` = trait[] | undefined;
  $interfaces: `What it offers` = trait[] | undefined;
  $layout: `Where its files go` = trait;
  $layers: `Anything else the compiler is told` = trait[] | undefined;
  $dependencies: `What its generated code needs` = object[] | undefined;
}
function targetOf(target: Target): `Gives a record written in braces its type` -> Target {
  return target;
}
";
    /// The language, the layout, and the ecosystem the fixture's target is built from.
    const ROLES: &str = "\
trait lang extends targetLanguage: `Rust` {
  .fileExtension = \"rs\";
  .markerComment = \"//\";
}
trait flat extends targetLayout: `One file per unit` { }
trait cargo extends targetEcosystem: `Crates from crates.io` {
  .ecosystem = \"cargo\";
}
";
    /// The `ace const` holding the target `rust`, whose output directory is `out`, added
    /// to `global` so that every entity of every project file is built for it.
    const TARGETS: &str = "\
use \"elfie/target\";
use \"./roles\";

ace const rust = targetOf({
  output = \"out\",
  language = lang,
  layout = flat,
  ecosystem = cargo,
});
global@targets.add(rust);
";
    /// The manifest of a fixture: it names the library the fixture carries.
    const LIBRARY_MANIFEST: &str = r#"{ "dependencies": { "elfie": "./lib" } }"#;

    /// A project directory under the system's temporary directory, removed when dropped.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("elfie-mcp-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("def")).unwrap();
            let fixture = Fixture { root };
            // Every fixture carries a library of its own and names it in its manifest, so
            // that a test never reads the copy of the library the compiler was built
            // with, whose files would be files of the program like any other.
            fixture.write(MANIFEST, LIBRARY_MANIFEST).library();
            fixture
        }

        /// The library of the package `elfie`, written under [`LIBRARY_ROOT`]: the
        /// prelude, the kind data a body's lists live on, and the target vocabulary a
        /// project's `ace const` declares its targets with.
        fn library(&self) -> &Fixture {
            self.write(&format!("{LIBRARY_ROOT}/main.lfy"), LIBRARY)
                .write(&format!("{LIBRARY_ROOT}/prelude/builtin.lfy"), LIBRARY_BUILTIN)
                .write(&format!("{LIBRARY_ROOT}/prelude/entity.lfy"), LIBRARY_ENTITY)
                .write(&format!("{LIBRARY_ROOT}/prelude/trait.lfy"), LIBRARY_TRAIT)
                .write(&format!("{LIBRARY_ROOT}/criteria/main.lfy"), LIBRARY_CRITERIA)
                .write(&format!("{LIBRARY_ROOT}/target/main.lfy"), LIBRARY_TARGET)
        }

        /// The same project with one target `rust` whose output directory is `out`,
        /// declared as an `ace const` of the project and added to `global`, so that every
        /// entity of every project file is built for it.
        fn with_rust_target(&self) -> &Fixture {
            self.write("def/roles.lfy", ROLES).write("def/targets.lfy", TARGETS)
        }

        fn write(&self, path: &str, text: &str) -> &Fixture {
            let disk = self.root.join(path);
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(disk, text).unwrap();
            self
        }

        fn remove(&self, path: &str) -> &Fixture {
            fs::remove_file(self.root.join(path)).unwrap();
            self
        }

        fn session(&self) -> Session {
            Session::load(&self.root)
        }

        /// Git run in the fixture's root, so that a test can make the root a repository and
        /// commit the source a unit's outputs are then accepted against.
        // @lfy def/mcp/main.lfy:changes#changes:changes:6fdf72883c3fb05ab2a58a8325708fe690fc56fcfe75d909abff5f56efdb18e1
        fn git(&self, arguments: &[&str]) -> &Fixture {
            let status = Command::new("git")
                .args(arguments)
                .current_dir(&self.root)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("git");
            assert!(status.success(), "git {arguments:?}");
            self
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// A call through the dispatch table, as the protocol would make it.
    fn call(session: &Session, name: &str, arguments: serde_json::Value) -> ToolResult {
        let tools = tools();
        let registered = traits::find(&tools, name).unwrap_or_else(|| panic!("no tool named {name}"));
        call_tool(registered, session, arguments.as_object().unwrap())
    }

    /// The name of every tool a session over standard input and output is offered, in the
    /// order the definition declares the fns that carry `tool`: every one carrying `tool`
    /// alone but `elfie_report` and `elfie_reportMany`, which only a verifier's hosted
    /// session is offered, and none carrying `compilerTool` or `commandTool`, which need a
    /// binding.
    const TOOL_NAMES: [&str; 15] = [
        "elfie_problems",
        "elfie_find",
        "elfie_entity",
        "elfie_references",
        "elfie_outline",
        "elfie_grammar",
        "elfie_units",
        "elfie_request",
        "elfie_check",
        "elfie_output",
        "elfie_search",
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
    ];

    /// The name of every tool a compiler's hosted session is offered when its target has no
    /// command: every fn carrying `tool` and not `commandTool` but `report` and
    /// `reportMany`.
    const COMPILER_TOOLS: [&str; 19] = [
        "elfie_problems",
        "elfie_find",
        "elfie_entity",
        "elfie_references",
        "elfie_outline",
        "elfie_grammar",
        "elfie_units",
        "elfie_request",
        "elfie_task",
        "elfie_dependency",
        "elfie_check",
        "elfie_mark",
        "elfie_submit",
        "elfie_output",
        "elfie_search",
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
    ];

    /// The name of every tool a verifier's hosted session is offered when its target has no
    /// command: every fn carrying `tool` alone but `check`, `report` and `reportMany` among
    /// them.
    const VERIFIER_TOOLS: [&str; 16] = [
        "elfie_problems",
        "elfie_find",
        "elfie_entity",
        "elfie_references",
        "elfie_outline",
        "elfie_grammar",
        "elfie_units",
        "elfie_request",
        "elfie_output",
        "elfie_search",
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
        "elfie_report",
        "elfie_reportMany",
    ];

    /// The name of every fn of this module that carries `tool`, in declaration order.
    const EVERY_TOOL: [&str; 25] = [
        "elfie_problems",
        "elfie_find",
        "elfie_entity",
        "elfie_references",
        "elfie_outline",
        "elfie_grammar",
        "elfie_units",
        "elfie_request",
        "elfie_task",
        "elfie_dependency",
        "elfie_check",
        "elfie_mark",
        "elfie_submit",
        "elfie_output",
        "elfie_search",
        "elfie_source",
        "elfie_changes",
        "elfie_review",
        "elfie_globalReview",
        "elfie_report",
        "elfie_reportMany",
        "elfie_build",
        "elfie_test",
        "elfie_lint",
        "elfie_format",
    ];

    /// An agent connected to a server over in-memory streams, speaking the protocol's
    /// newline-delimited JSON-RPC: the whole server runs, and closing the input it reads
    /// is standard input closing.
    struct Agent {
        /// The stream the server reads as its standard input; dropped to close it.
        input: Option<DuplexStream>,
        output: BufReader<DuplexStream>,
        served: tokio::task::JoinHandle<i32>,
        next_id: i64,
    }

    impl Agent {
        fn connect(root: &Path) -> Agent {
            let (input, server_input) = tokio::io::duplex(1 << 20);
            let (server_output, output) = tokio::io::duplex(1 << 20);
            let served = tokio::spawn(serve_on(server_input, server_output, root.to_path_buf()));
            Agent {
                input: Some(input),
                output: BufReader::new(output),
                served,
                next_id: 0,
            }
        }

        async fn send(&mut self, message: Value) {
            let line = format!("{message}\n");
            let input = self.input.as_mut().expect("an open input");
            input.write_all(line.as_bytes()).await.unwrap();
            input.flush().await.unwrap();
        }

        /// One line the server wrote to the stream it takes for standard output.
        async fn read(&mut self) -> Value {
            let mut line = String::new();
            let read = self.output.read_line(&mut line).await.unwrap();
            assert_ne!(read, 0, "the server wrote nothing");
            serde_json::from_str(&line).unwrap_or_else(|error| panic!("a protocol message, not {line:?}: {error}"))
        }

        /// A request and the response carrying its id.
        async fn request(&mut self, method: &str, params: Value) -> Value {
            self.next_id += 1;
            let id = self.next_id;
            self.send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
                .await;
            loop {
                let message = self.read().await;
                if message.get("id") == Some(&serde_json::json!(id)) {
                    return message;
                }
            }
        }

        /// Standard input closes; what the server returns.
        async fn close(mut self) -> i32 {
            drop(self.input.take());
            self.served.await.unwrap()
        }
    }

    /// A project of two units, `b` and the `a` that depends on it, for the rust target.
    fn compiled_project() -> Fixture {
        let fixture = Fixture::new();
        fixture
            .with_rust_target()
            .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
            .write("def/b.lfy", "d B {}\n");
        fixture
    }

    /// The unit with a stem accepted against the outputs on disk, with its source maps
    /// recorded in the target's output directory the way the command line records them.
    fn record_unit(fixture: &Fixture, stem: &str) {
        let session = fixture.session();
        let planned = plan_of(&session);
        let (workspace, plan) = (planned.workspace(), &planned.plan);
        let index = resolve_unit(workspace, plan, stem, None).unwrap();
        let request = request_of_unit(&planned.program, plan, index);
        let (outputs, elsewhere) = outputs_of(workspace, plan, index);
        let verdict = generation::accept(&planned.program, plan, &request, index, &outputs, &[], &elsewhere);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        generation::write_source_maps(&fixture.root.join("out/source-map.json"), &verdict.source_maps).unwrap();
    }

    /// A project whose unit `b` has an accepted output, with the source maps recorded in
    /// the target's output directory the way the command line records them.
    fn accepted_project() -> Fixture {
        let fixture = compiled_project();
        fixture.write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
        record_unit(&fixture, "b");
        fixture
    }

    /// The id of the one criterion or test of a project spelled as `text`.
    fn requirement_id(fixture: &Fixture, text: &str) -> String {
        let program = interpret::lower(fixture.session().workspace);
        requirements_of(&program)
            .into_iter()
            .find(|(_, spelled)| spelled == text)
            .map(|(id, _)| id)
            .unwrap_or_else(|| panic!("the program holds a requirement reading {text}"))
    }

    /// A project with one fn carrying one local criterion and one global criterion.
    fn project_with_requirements() -> Fixture {
        let fixture = Fixture::new();
        fixture
            .with_rust_target()
            .write(
                "def/b.lfy",
                "fn b(): `A b` => string { @acceptanceCriteria.add({ behavior = `It answers` }); }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written outside the output directory` });\n",
            );
        fixture
    }

    /// A binding for a role, bound to the batch of a fixture with an identifier or, with
    /// none named, to the global review. A verifier's binding carries the review request
    /// that verifier would have been run with.
    fn binding_of(fixture: &Fixture, role: Role, batch: Option<&str>) -> Binding {
        let session = fixture.session();
        let planned = plan_of(&session);
        let named = batch.map(|name| {
            planned
                .plan
                .batches
                .iter()
                .find(|one| one.identifier == name)
                .cloned()
                .unwrap_or_else(|| panic!("a batch named {name}"))
        });
        let review = match (role, &named) {
            (Role::Verifier, Some(batch)) => Some(generation::review(
                &planned.program,
                &planned.plan,
                batch,
                &planned.maps,
                None,
                &[],
            )),
            _ => None,
        };
        // A compiler run is given the request the compile made for its batch, as the CLI
        // gives it one.
        // @lfy def/mcp/main.lfy:request#request:request:af5eea7b402d3ed59ce0541ca71250ff92ea2e142a768f4b98af9e9bc4f348c9
        let request = match (role, &named) {
            (Role::Compiler, Some(batch)) => Some(generation::request(
                &planned.program,
                &planned.plan,
                batch,
                &[],
                &BTreeMap::new(),
                true,
                &[],
            )),
            _ => None,
        };
        Binding {
            role,
            batch: named,
            directory: fixture.root.clone(),
            plan: planned.plan.clone(),
            program: planned.program.clone(),
            request,
            review,
            source_maps: Vec::new(),
        }
    }

    /// A session bound to a binding, as a host opens one.
    fn bound(fixture: &Fixture, binding: Binding) -> Session {
        let mut workspace = binding.program.workspace.clone();
        workspace.root = fixture.root.clone();
        Session {
            workspace,
            seen: BTreeMap::new(),
            binding: Some(binding),
            reviews: Mutex::new(Vec::new()),
            submission: Mutex::new(None),
        }
    }

    /// A host's own state, with a URL of its own and no session open.
    fn hosting_of(url: &str) -> Hosting {
        Hosting {
            url: url.to_string(),
            token: "a".repeat(TOKEN_BYTES * 2),
            sessions: Mutex::new(BTreeMap::new()),
            opened: AtomicUsize::new(0),
            stopping: tokio::sync::Notify::new(),
        }
    }

    /// A call of a tool by name, as the protocol carries it.
    fn calling(name: &str, arguments: Value) -> CallToolRequestParams {
        serde_json::from_value(serde_json::json!({ "name": name, "arguments": arguments })).expect("a call")
    }

    /// The text of what a call answered, and whether it is an error.
    fn answered(result: &CallToolResult) -> (bool, String) {
        let value = serde_json::to_value(result).expect("a result");
        (
            value["isError"] == serde_json::json!(true),
            value["content"][0]["text"].as_str().unwrap_or_default().to_string(),
        )
    }

    /// The definition of a unit whose fn `b` answers one criterion, with a second criterion
    /// named by `other`.
    fn two_criteria(other: &str) -> String {
        format!(
            "fn b(): `A b` => string {{\n  @acceptanceCriteria.add({{ behavior = `It answers` }}).add({{ behavior = `{other}` }});\n}}\n"
        )
    }

    /// A session bound to a compiler run of the only batch of a fixture, with the source
    /// each unit's outputs were generated from where one is given, so that the request it is
    /// handed knows which criteria and tests were removed.
    // @lfy def/mcp/main.lfy:task#task:compilerTool:1e78463366ba8fbbfbec5fc142e181f67f7b2aead81f243e58972718f0a441a8
    fn compiling(fixture: &Fixture, previous: &[(&str, &str)]) -> Session {
        compiling_with(fixture, previous, &[])
    }

    /// The same, with the paths of the units whose previous source is only their
    /// hash-matched one, which the request reads nothing of as a rewording and whose
    /// removed criteria and tests it can quote neither the place nor the text of.
    // @lfy def/mcp/main.lfy:task#task:task:de5f7c473194374bfa34413eb23292d2536ef397d3077ff1897532c68ab7052d
    fn compiling_with(fixture: &Fixture, previous: &[(&str, &str)], hash_matched: &[&str]) -> Session {
        let session = fixture.session();
        let planned = plan_of(&session);
        let batch = planned.plan.batches.first().cloned().expect("one batch");
        let was: BTreeMap<String, String> = previous
            .iter()
            .map(|(path, text)| ((*path).to_string(), (*text).to_string()))
            .collect();
        let existing: Vec<Output> = batch
            .units
            .iter()
            .flat_map(|&unit| planned.plan.units[unit].outputs.iter())
            .filter_map(|map| {
                fs::read_to_string(fixture.root.join(&map.output))
                    .ok()
                    .map(|text| Output { path: map.output.clone(), text })
            })
            .collect();
        let named: Vec<String> = hash_matched.iter().map(|path| (*path).to_string()).collect();
        let request = generation::request(&planned.program, &planned.plan, &batch, &existing, &was, true, &named);
        bound(
            fixture,
            Binding {
                role: Role::Compiler,
                batch: Some(batch),
                directory: fixture.root.clone(),
                plan: planned.plan.clone(),
                program: planned.program.clone(),
                request: Some(request),
                review: None,
                source_maps: Vec::new(),
            },
        )
    }
}
