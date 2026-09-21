//! Compiled from `def/mcp/main.lfy`: the Elfie agent server.
//!
//! One tool per fn of the definition that carries `tool`, spoken over the Model Context
//! Protocol on standard input and output. The server never writes a file: during
//! compilation the agent writes the outputs itself and asks the server to check them, so
//! one path exists for writing and the CLI owns it.

pub mod data; // @lfy def/mcp/data.lfy:1
pub mod traits; // @lfy def/mcp/traits.lfy:1

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use elfie_core::generation::{self, Batch, Marker, Output, Plan, Request, SourceMap, Unit};
use elfie_core::model::{self, Criterion, EntityId, FileId, Model};
use elfie_core::query::{self, Outline, Position, Range};
use elfie_core::workspace::{self, Workspace};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};
use serde_json::Value;

pub use data::{Session, Stamp, Tool, ToolArgument, ToolResult};
pub use traits::{Arguments, Registered};

/// The project layout file; a change to it loads the workspace again.
const MANIFEST: &str = "elfie.json";

/// The exit code when the server itself cannot run, as the CLI reads it.
const FAILURE: i32 = 3;

// ---- the session ------------------------------------------------------------------

/// The stamp of one file on disk, or `None` when there is no file there.
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

/// Every file of the program: the files bound, every `.lfy` file under the source
/// directory (so that a new file is noticed), and the manifest.
fn watched(workspace: &Workspace) -> BTreeSet<String> {
    let mut paths: BTreeSet<String> = workspace.files.iter().map(|file| file.path.clone()).collect();
    let mut found = Vec::new();
    walk_sources(&workspace.root.join(&workspace.source_directory), &mut found);
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
    /// stamped as read.
    // @lfy def/mcp/main.lfy:serve
    pub fn load(root: &Path) -> Session {
        let workspace = workspace::load(root);
        let seen = stamps(&workspace);
        Session { workspace, seen }
    }

    /// What runs first when a call arrives: every file of the program whose modification
    /// time or size differs from what was seen is re-read through `workspace::change`
    /// with no text, a file that vanished the same way, and a changed `elfie.json` loads
    /// the workspace again.
    // @lfy def/mcp/main.lfy:serve
    pub fn refresh(&mut self) {
        let now = stamps(&self.workspace);
        if now.get(MANIFEST) != self.seen.get(MANIFEST) {
            self.workspace = workspace::load(&self.workspace.root);
            self.seen = stamps(&self.workspace);
            return;
        }
        let changed: Vec<String> = now
            .keys()
            .chain(self.seen.keys())
            .filter(|path| now.get(*path) != self.seen.get(*path))
            .cloned()
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        for path in &changed {
            self.workspace = workspace::change(&self.workspace, path, None);
        }
        if !changed.is_empty() {
            self.seen = stamps(&self.workspace);
        }
    }
}

// ---- the server -------------------------------------------------------------------

/// The fns of this module that carry `tool`, in order, as the server lists them.
// @lfy def/mcp/main.lfy:serve
pub fn tools() -> Vec<Registered> {
    vec![
        Registered::new(
            "problems",
            "The problems of the program, or of one file: what the compiler, an editor, or a check would report",
            vec![ToolArgument::new("file", "a path relative to the root; every file when left out", "string", false)], // @lfy def/mcp/main.lfy:problems
            |session, arguments| problems(session, arguments.optional("file")),
        ),
        Registered::new(
            "find",
            "Where things with a name are declared",
            vec![ToolArgument::new(
                "name",
                "an identifier, or an owner's identifier, a dot, and a member's name; a file path and a colon before it limits the search to that file",
                "string",
                true,
            )], // @lfy def/mcp/main.lfy:find
            |session, arguments| find(session, arguments.string("name")?),
        ),
        Registered::new(
            "entity",
            "Everything known about the things with a name: definition, type, documentation, traits, members, criteria, tests, references, and source",
            vec![ToolArgument::new("name", "as elfie_find takes it", "string", true)], // @lfy def/mcp/main.lfy:entity
            |session, arguments| entity(session, arguments.string("name")?),
        ),
        Registered::new(
            "references",
            "Every place things with a name are used",
            vec![ToolArgument::new("name", "as elfie_find takes it", "string", true)], // @lfy def/mcp/main.lfy:references
            |session, arguments| references(session, arguments.string("name")?),
        ),
        Registered::new(
            "outline",
            "The declarations of one file, nested as written",
            vec![ToolArgument::new("file", "a path relative to the root", "string", true)], // @lfy def/mcp/main.lfy:outline
            |session, arguments| outline(session, arguments.string("file")?),
        ),
        Registered::new(
            "grammar",
            "The grammar of Elfie: every terminal and every rule as EBNF, for an agent writing or reading Elfie",
            vec![],
            |session, _| grammar(session),
        ),
        Registered::new(
            "units",
            "The units the compiler would produce, in order, why each needs generating, and how they are batched",
            vec![ToolArgument::new("target", "the identifier of one target; every target when left out", "string", false)], // @lfy def/mcp/main.lfy:units
            |session, arguments| units(session, arguments.optional("target")),
        ),
        Registered::new(
            "request",
            "The full request for one batch: what to produce, where, every criterion to satisfy, and how to report",
            vec![
                ToolArgument::new("batch", "the identifier of a batch, as elfie_units lists it, or the stem of one of its units", "string", true), // @lfy def/mcp/main.lfy:request
                ToolArgument::new("target", "the identifier of the batch's target; the only target when left out", "string", false), // @lfy def/mcp/main.lfy:request
            ],
            |session, arguments| request(session, arguments.string("batch")?, arguments.optional("target")),
        ),
        Registered::new(
            "check",
            "Whether the outputs on disk satisfy the request for one unit, and what is wrong when they do not",
            vec![
                ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true), // @lfy def/mcp/main.lfy:check
                ToolArgument::new("target", "the identifier of the unit's target; the only target when left out", "string", false), // @lfy def/mcp/main.lfy:check
            ],
            |session, arguments| check(session, arguments.string("unit")?, arguments.optional("target")),
        ),
        Registered::new(
            "output",
            "Where the generated code for one entity is: every region of every output that came from it, with its text",
            vec![
                ToolArgument::new("name", "as elfie_find takes it", "string", true), // @lfy def/mcp/main.lfy:output
                ToolArgument::new("target", "the identifier of one target; every target when left out", "string", false), // @lfy def/mcp/main.lfy:output
            ],
            |session, arguments| output(session, arguments.string("name")?, arguments.optional("target")),
        ),
        Registered::new(
            "source",
            "Where one line of generated code came from: the definition file, the entity, and its line",
            vec![
                ToolArgument::new("file", "the path of an output file, relative to the root", "string", true), // @lfy def/mcp/main.lfy:source
                ToolArgument::new("line", "a line of that file, counting from 1", "number", true), // @lfy def/mcp/main.lfy:source
            ],
            |session, arguments| source(session, arguments.string("file")?, counted(arguments.string("line")?, "line")?),
        ),
        Registered::new(
            "changes",
            "What differs for each entity of a unit since its outputs were last accepted",
            vec![ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true)], // @lfy def/mcp/main.lfy:changes
            |session, arguments| changes(session, arguments.string("unit")?),
        ),
        Registered::new(
            "review",
            "The full review request for one batch: every criterion and test with the regions generated for its entities, and how to report",
            vec![
                ToolArgument::new("batch", "the identifier of a batch, as elfie_units lists it, or the stem of one of its units", "string", true), // @lfy def/mcp/main.lfy:review
                ToolArgument::new("target", "the identifier of the batch's target; the only target when left out", "string", false), // @lfy def/mcp/main.lfy:review
            ],
            |session, arguments| review(session, arguments.string("batch")?, arguments.optional("target")),
        ),
    ]
}

/// One call of a fn carrying `tool`: the arguments are checked against the parameters as
/// the parameters are declared, then the fn is called with the session and them.
///
// Decision: `source` takes a line, which is a number, and the trait's `Arguments` gives a
// fn the text of an argument. A number argument is therefore checked as a number here,
// where the fn that carries the trait lives, and then handed on spelled as text through a
// tool that says text, so that the one call path of `traits` still makes the call, catches
// a failure, and answers with the result.
// @lfy def/mcp/main.lfy:source
fn call_tool(registered: &Registered, session: &Session, arguments: &JsonObject) -> ToolResult {
    if let Err(message) = traits::check_arguments(&registered.tool, arguments) {
        return ToolResult::error(message);
    }
    let mut spelled = arguments.clone();
    let mut tool = registered.tool.clone();
    for argument in &mut tool.arguments {
        if argument.ty != "number" {
            continue;
        }
        if let Some(number) = spelled.get(&argument.name).filter(|value| value.is_number()).cloned() {
            spelled.insert(argument.name.clone(), Value::String(number.to_string()));
        }
        argument.ty = "string".to_string();
    }
    traits::call(
        &Registered {
            tool,
            call: registered.call,
        },
        session,
        &spelled,
    )
}

/// A number argument that counts things — a line, an index, or a length — read back from
/// the text it was spelled as.
// @lfy def/mcp/main.lfy:source
fn counted(text: &str, name: &str) -> Result<usize, String> {
    let value: f64 = text
        .parse()
        .map_err(|_| format!("the argument {name} must be a number"))?;
    if value.fract() != 0.0 || value < 1.0 {
        return Err(format!("the argument {name} must be a whole number, counting from 1"));
    }
    Ok(value as usize)
}

/// The protocol server: one session behind a lock, and the tools.
struct Server {
    session: Mutex<Session>,
    tools: Vec<Registered>,
}

impl Server {
    // Decision: the definition loads the workspace on initialize; it is loaded when the
    // server is built, just before the initialize exchange, which the agent cannot tell
    // apart and which keeps the protocol library's handshake untouched.
    // @lfy def/mcp/main.lfy:serve
    fn new(root: &Path) -> Server {
        Server {
            session: Mutex::new(Session::load(root)),
            tools: tools(),
        }
    }

    /// One call: the session is refreshed, then the tool runs.
    // @lfy def/mcp/main.lfy:serve
    fn call(&self, request: &CallToolRequestParams) -> Result<CallToolResult, ErrorData> {
        let Some(registered) = traits::find(&self.tools, &request.name) else {
            return Err(ErrorData::invalid_params(format!("no tool named {}", request.name), None));
        };
        let mut session = self.session.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        session.refresh();
        let empty = JsonObject::new();
        let arguments = request.arguments.as_ref().unwrap_or(&empty);
        let result = call_tool(registered, &session, arguments);
        let content = vec![ContentBlock::text(result.text)];
        Ok(if result.is_error { CallToolResult::error(content) } else { CallToolResult::success(content) })
    }
}

impl ServerHandler for Server {
    /// Tools only: no resources and no prompts.
    // @lfy def/mcp/main.lfy:serve
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18) // @lfy def/mcp/main.lfy:Protocol
            .with_server_info(Implementation::new("elfie", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "The Elfie agent server. elfie_problems, elfie_find, elfie_entity, elfie_references, and elfie_outline read the program; elfie_grammar gives the language; elfie_units, elfie_request, elfie_check, and elfie_review drive compilation; elfie_output, elfie_source, and elfie_changes read what the last acceptance recorded. Nothing here writes a file.",
            )
    }

    // @lfy def/mcp/main.lfy:serve
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(traits::list(&self.tools)))
    }

    // @lfy def/mcp/main.lfy:serve
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.call(&request).map(CallToolResponse::from)
    }
}

/// Serve agents over standard input and output until the connection closes.
///
/// Speaks the protocol over standard input and output; nothing else is ever written to
/// standard output, and logging goes to standard error. Returns 0 when standard input
/// closes.
// @lfy def/mcp/main.lfy:serve
pub fn serve(root: Option<&Path>) -> i32 {
    // @lfy def/mcp/main.lfy:serve
    let root = match root {
        Some(root) => root.to_path_buf(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("elfie mcp: {error}");
            return FAILURE;
        }
    };
    runtime.block_on(async move {
        let server = Server::new(&root);
        // @lfy def/mcp/main.lfy:serve
        let running = match server.serve(rmcp::transport::stdio()).await {
            Ok(running) => running,
            // Decision: standard input closing before the agent initializes is the
            // connection closing, so it exits with 0 like any other close.
            Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return 0,
            Err(error) => {
                eprintln!("elfie mcp: {error}");
                return FAILURE;
            }
        };
        // @lfy def/mcp/main.lfy:serve
        match running.waiting().await {
            Ok(_) => 0,
            Err(error) => {
                eprintln!("elfie mcp: {error}");
                FAILURE
            }
        }
    })
}

// ---- shared helpers ---------------------------------------------------------------

/// The kind of an entity, as its symbol names it.
fn kind_of(model: &Model, entity: EntityId) -> String {
    model.entities[entity]
        .symbol
        .map_or_else(|| "entity".to_string(), |symbol| model.symbols[symbol].kind.as_str().to_string())
}

/// The identifier of an entity, or `anonymous`.
fn identifier_of(model: &Model, entity: EntityId) -> String {
    model.entities[entity]
        .identifier
        .clone()
        .unwrap_or_else(|| "anonymous".to_string())
}

/// The range of the token that spells an entity's name, or of the first token of its
/// declaration; an empty range when it has neither.
fn identifier_range(model: &Model, entity: EntityId) -> Range {
    let record = &model.entities[entity];
    let Some(node) = record.node else { return Range::default() };
    let tokens = model.tokens(node.file);
    let named = record
        .symbol
        .and_then(|symbol| model.symbols[symbol].name_token)
        .and_then(|index| tokens.get(index));
    let token = named.or_else(|| model.first_token(node));
    match token {
        Some(token) => Range {
            file: token.file.to_string(),
            start: Position::new(token.line, token.column),
            end: Position::new(token.line, token.column + token.raw.chars().count()),
        },
        None => Range::default(),
    }
}

/// The file and line an entity is declared at, as `file:line`.
fn place_of(model: &Model, entity: EntityId) -> String {
    let range = identifier_range(model, entity);
    if range.file.is_empty() {
        "unplaced".to_string()
    } else {
        format!("{}:{}", range.file, range.start.line)
    }
}

/// The text of one line of a file, from its tokens.
fn line_text(model: &Model, file: FileId, line: usize) -> String {
    let text: String = model.tokens(file).iter().map(|token| token.raw.as_str()).collect();
    text.lines().nth(line.saturating_sub(1)).unwrap_or("").trim().to_string()
}

/// The text of a criterion: its situation when it has one, then its behavior and side
/// effects.
fn criterion_text(criterion: &Criterion) -> String {
    let mut text = String::new();
    if let Some(situation) = &criterion.situation {
        text.push_str(&format!("when {}: ", situation.join(" ")));
    }
    if let Some(behavior) = &criterion.behavior {
        text.push_str(&behavior.join("\n  "));
    }
    if let Some(side_effects) = &criterion.side_effects {
        if !text.is_empty() {
            text.push_str("; ");
        }
        text.push_str(&format!("side effects: {}", side_effects.join("\n  ")));
    }
    text
}

/// An error saying a file is not in the program and listing the files that are.
fn not_in_program(workspace: &Workspace, file: &str) -> String {
    let mut text = format!("{file} is not in the program; the files are:");
    for known in &workspace.files {
        text.push_str(&format!("\n  {}", known.path));
    }
    text
}

/// The text when nothing is declared with a name.
fn nothing_named(name: &str) -> String {
    format!("nothing is declared as {name}; elfie_outline lists the declarations of one file")
}

// ---- the tools --------------------------------------------------------------------

/// The problems of the program, or of one file: what the compiler, an editor, or a check
/// would report. One line per diagnostic reading the file, the line, the column, the
/// stage, the severity, and the message; the text says so when there are none.
// @lfy def/mcp/main.lfy:problems
pub fn problems(session: &Session, file: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    let diagnostics = query::diagnostics_of(workspace, file); // @lfy def/mcp/main.lfy:problems
    if diagnostics.is_empty() {
        // Decision: the criterion answers this situation with text, not a failure, and it
        // names no exception for a file outside the program, so a name that matches
        // nothing is answered the same way; elfie_outline is the tool that lists the files.
        if let Some(file) = file {
            return Ok(format!("no problems in {file}")); // @lfy def/mcp/main.lfy:problems
        }
        return Ok("no problems".to_string()); // @lfy def/mcp/main.lfy:problems
    }
    let lines: Vec<String> = diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{}:{}:{}: {} {}: {}",
                diagnostic.range.file,
                diagnostic.range.start.line,
                diagnostic.range.start.column,
                diagnostic.stage,
                diagnostic.severity,
                diagnostic.message
            )
        })
        .collect();
    Ok(lines.join("\n"))
}

/// Where things with a name are declared: one line per entity reading the kind, the
/// identifier, the file, the line, and the definition when there is one.
// @lfy def/mcp/main.lfy:find
pub fn find(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        return Ok(nothing_named(name)); // @lfy def/mcp/main.lfy:find
    }
    // @lfy def/mcp/main.lfy:find
    let lines: Vec<String> = entities
        .iter()
        .map(|&entity| {
            let mut line = format!("{} {} {}", kind_of(model, entity), identifier_of(model, entity), place_of(model, entity));
            if let Some(definition) = &model.entities[entity].definition {
                line.push_str(&format!(": {}", model::strip_references(definition)));
            }
            line
        })
        .collect();
    Ok(lines.join("\n"))
}

/// Everything known about the things with a name: for each entity, the hover fields with a
/// heading each, its traits, members, tests, references, and source.
// @lfy def/mcp/main.lfy:entity
pub fn entity(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        return Ok(nothing_named(name));
    }
    let mut sections = Vec::new();
    for &id in &entities {
        let record = &model.entities[id];
        let hover = query::hover_of(workspace, id);
        let mut out = String::new();
        // Each entity is headed by its file, so the caller can narrow the name with a
        // file path. @lfy def/mcp/main.lfy:entity
        out.push_str(&format!("# {} {} ({})\n", hover.kind, hover.identifier, place_of(model, id)));
        // The fields of the hover, a heading each. @lfy def/mcp/main.lfy:entity
        if let Some(definition) = &hover.definition {
            out.push_str(&format!("\n## Definition\n{definition}\n"));
        }
        if let Some(ty) = &hover.ty {
            out.push_str(&format!("\n## Type\n{ty}\n"));
        }
        if let Some(owner) = &hover.owner {
            out.push_str(&format!("\n## Owner\n{owner}\n"));
        }
        if let Some(documentation) = &hover.documentation {
            out.push_str(&format!("\n## Documentation\n{documentation}\n"));
        }
        if !hover.criteria.is_empty() {
            out.push_str("\n## Criteria\n");
            for criterion in &hover.criteria {
                let from = if criterion.contributor == id {
                    String::new()
                } else {
                    format!(" (from {})", identifier_of(model, criterion.contributor))
                };
                out.push_str(&format!("- {}{from}\n", criterion_text(criterion)));
            }
        }
        // The identifier of each trait, as the hover spells them: every one of
        // `Entity.traits` in application order, with `builtin` among them for a native
        // thing. @lfy def/mcp/main.lfy:entity
        if !hover.traits.is_empty() {
            out.push_str(&format!("\n## Traits\n{}\n", hover.traits.join(", ")));
        }
        let members = model.members(id);
        if !members.is_empty() {
            out.push_str("\n## Members\n");
            for symbol in members {
                let symbol = &model.symbols[symbol];
                let member = &model.entities[symbol.entity];
                let mut line = format!("- {}", symbol.name);
                if let Some(definition) = &member.definition {
                    line.push_str(&format!(": {}", model::strip_references(definition)));
                }
                if let Some(ty) = &member.ty {
                    line.push_str(&format!(" ({})", model::type_text(model, ty)));
                }
                out.push_str(&line);
                out.push('\n');
            }
        }
        if !record.tests.is_empty() {
            out.push_str("\n## Tests\n");
            for test in &record.tests {
                out.push_str(&format!("- input: {}\n  expect: {}\n", test.input_text.trim(), test.expect_text.trim()));
            }
        }
        if let Some(symbol) = record.symbol {
            let usages = model::usages_of(model, symbol);
            out.push_str(&format!("\n## References ({})\n", usages.len()));
            for usage in usages {
                let (file, line, _) = usage_place(model, usage);
                out.push_str(&format!("- {file}:{line}\n"));
            }
        }
        out.push_str(&format!("\n## Source\n```elfie\n{}\n```", query::source_of(workspace, id).trim_end()));
        sections.push(out);
    }
    // More than one entity: each is separated by a rule. @lfy def/mcp/main.lfy:entity
    Ok(sections.join("\n\n---\n\n"))
}

/// The file, line, and column of a usage: of the token that spells the name, or of the
/// first token of the using node.
fn usage_place(model: &Model, usage: usize) -> (String, usize, usize) {
    let usage = &model.usages[usage];
    let tokens = model.tokens(usage.node.file);
    let token = usage
        .token
        .and_then(|index| tokens.get(index))
        .or_else(|| model.first_token(usage.node));
    match token {
        Some(token) => (token.file.to_string(), token.line, token.column),
        None => (model.sources[usage.node.file].path.clone(), 0, 0),
    }
}

/// Every place things with a name are used: for each entity, one line per usage of its
/// symbol reading the file, the line, the column, and the text of that line.
// @lfy def/mcp/main.lfy:references
pub fn references(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        // Decision: the definition says nothing about a name that matches no entity;
        // the answer of elfie_find is given so the caller learns the same thing.
        return Ok(nothing_named(name));
    }
    let mut lines = Vec::new();
    for &id in &entities {
        let Some(symbol) = model.entities[id].symbol else { continue };
        for usage in model::usages_of(model, symbol) {
            let (file, line, column) = usage_place(model, usage); // @lfy def/mcp/main.lfy:references
            let text = line_text(model, model.usages[usage].node.file, line);
            lines.push(format!("{file}:{line}:{column}: {text}"));
        }
    }
    if lines.is_empty() {
        // Decision: no usages is a plain answer, not an error.
        return Ok(format!("{name} is never used"));
    }
    Ok(lines.join("\n"))
}

/// One outline and its children as an indented list: two spaces per level, then the
/// kind, the name, and the line.
// @lfy def/mcp/main.lfy:outline
fn render_outline(items: &[Outline], depth: usize, out: &mut Vec<String>) {
    for item in items {
        // Decision: the line is the identifier's, so a documented declaration is listed
        // where its name is, not where its documentation begins.
        out.push(format!("{}{} {} {}", "  ".repeat(depth), item.kind, item.name, item.selection_range.start.line));
        render_outline(&item.children, depth + 1, out);
    }
}

/// The declarations of one file, nested as written.
// @lfy def/mcp/main.lfy:outline
pub fn outline(session: &Session, file: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    if workspace.file(file).is_none() {
        return Err(not_in_program(workspace, file)); // @lfy def/mcp/main.lfy:outline
    }
    let items = query::outline_of(workspace, file);
    if items.is_empty() {
        return Ok(format!("no declarations in {file}"));
    }
    let mut lines = Vec::new();
    render_outline(&items, 0, &mut lines); // @lfy def/mcp/main.lfy:outline
    Ok(lines.join("\n"))
}

/// The grammar of Elfie: every terminal, a blank line, then every rule as EBNF.
// @lfy def/mcp/main.lfy:grammar
pub fn grammar(_session: &Session) -> Result<String, String> {
    Ok(format!(
        "{}\n\n{}",
        elfie_core::grammar::terminal_document(),
        elfie_core::grammar::grammar_document()
    )) // @lfy def/mcp/main.lfy:grammar
}

/// The source maps of each target, read from `source-map.json` in its output directory,
/// dropping any whose output no longer exists.
fn load_source_maps(workspace: &Workspace) -> Vec<SourceMap> {
    let mut maps = Vec::new();
    let mut seen = BTreeSet::new();
    for target in &workspace.targets {
        if !seen.insert(target.output_directory.clone()) {
            continue;
        }
        let path = workspace.root.join(&target.output_directory).join("source-map.json");
        for map in generation::read_source_maps(&path) {
            if workspace.root.join(&map.output).is_file() {
                maps.push(map);
            }
        }
    }
    maps
}

/// Every file under a directory, skipping build and version control directories.
fn walk_all(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target" || name == ".git" || name == "node_modules") {
                continue;
            }
            walk_all(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// The files under the target's output directory that carry a marker for the unit's
/// file, read from disk.
// @lfy def/mcp/main.lfy:check
fn outputs_of(workspace: &Workspace, plan: &Plan, unit: usize) -> Vec<Output> {
    let unit = &plan.units[unit];
    let target = &workspace.targets[unit.target];
    let file = &workspace.files[unit.file].path;
    let mut paths = Vec::new();
    walk_all(&workspace.root.join(&target.output_directory), &mut paths);
    paths.sort();
    let mut found = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if generation::parse_markers(&text).iter().any(|marker| &marker.file == file) {
            found.push(Output {
                path: relative(&workspace.root, &path),
                text,
            });
        }
    }
    found
}

/// The identifiers of every target, joined.
fn known_targets(workspace: &Workspace) -> String {
    let names: Vec<&str> = workspace.targets.iter().map(|target| target.identifier.as_str()).collect();
    if names.is_empty() { "none".to_string() } else { names.join(", ") }
}

/// An error naming the known targets when the target given is not one.
// @lfy def/mcp/main.lfy:units
fn check_target(workspace: &Workspace, target: Option<&str>) -> Result<(), String> {
    match target {
        Some(target) if !workspace.targets.iter().any(|known| known.identifier == target) => {
            Err(format!("{target} is not a target; the targets are: {}", known_targets(workspace)))
        }
        _ => Ok(()),
    }
}

/// The plan of the workspace with the source maps read from each target's output
/// directory.
// @lfy def/mcp/main.lfy:units
fn plan_of(workspace: &Workspace) -> Plan {
    generation::plan(workspace, &load_source_maps(workspace), &[])
}

/// One unit as a line: the target, the stem, the reason or up to date, and the stems of
/// its dependencies.
// @lfy def/mcp/main.lfy:units
fn unit_line(workspace: &Workspace, plan: &Plan, index: usize) -> String {
    let unit = &plan.units[index];
    let reason = unit.reason.map_or_else(|| "up to date".to_string(), |reason| reason.as_str().to_string());
    let dependencies: Vec<&str> = unit.dependencies.iter().map(|&dependency| plan.units[dependency].stem.as_str()).collect();
    format!("{} {} {reason} [{}]", workspace.targets[unit.target].identifier, unit.stem, dependencies.join(", "))
}

/// The identifier of the target a batch's units are of; a batch holds units of one target.
fn batch_target<'w>(workspace: &'w Workspace, plan: &Plan, batch: &Batch) -> &'w str {
    match batch.units.first() {
        Some(&unit) => workspace.targets[plan.units[unit].target].identifier.as_str(),
        None => "",
    }
}

/// One batch as a line: its identifier and the stems it holds.
// @lfy def/mcp/main.lfy:units
fn batch_line(plan: &Plan, batch: &Batch) -> String {
    let stems: Vec<&str> = batch.units.iter().map(|&unit| plan.units[unit].stem.as_str()).collect();
    format!("batch {} [{}]", batch.identifier, stems.join(", "))
}

/// The units the compiler would produce, in order, why each needs generating, and how
/// they are batched.
// @lfy def/mcp/main.lfy:units
pub fn units(session: &Session, target: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    check_target(workspace, target)?;
    let plan = plan_of(workspace);
    // @lfy def/mcp/main.lfy:units
    let mut lines: Vec<String> = (0..plan.units.len())
        .filter(|&index| target.is_none_or(|target| workspace.targets[plan.units[index].target].identifier == target))
        .map(|index| unit_line(workspace, &plan, index))
        .collect();
    if lines.is_empty() {
        return Ok(match target {
            Some(target) => format!("no units for the target {target}"),
            None => "no units".to_string(),
        });
    }
    // Then one line per batch. Decision: a batch holds units of one target, so the
    // target narrows the batches the same way it narrows the units; a blank line and the
    // word batch keep the two lists apart, since a batch line has the shape of a unit's.
    // @lfy def/mcp/main.lfy:units
    let batches: Vec<String> = plan
        .batches
        .iter()
        .filter(|batch| target.is_none_or(|target| batch_target(workspace, &plan, batch) == target))
        .map(|batch| batch_line(&plan, batch))
        .collect();
    if !batches.is_empty() {
        lines.push(String::new());
        lines.extend(batches);
    }
    Ok(lines.join("\n"))
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
// @lfy def/mcp/main.lfy:request
fn resolve_batch(workspace: &Workspace, plan: &Plan, name: &str, target: Option<&str>) -> Result<usize, String> {
    check_target(workspace, target)?;
    let matches: Vec<usize> = (0..plan.batches.len())
        .filter(|&index| {
            let batch = &plan.batches[index];
            let named = batch.identifier == name || batch.units.iter().any(|&unit| plan.units[unit].stem == name);
            named && target.is_none_or(|target| batch_target(workspace, plan, batch) == target)
        })
        .collect();
    match matches.as_slice() {
        [] => Err(match target {
            Some(target) => format!(
                "no batch of the target {target} has the identifier {name} or holds a unit with the stem {name}; elfie_units lists them"
            ),
            None => format!(
                "no batch has the identifier {name} or holds a unit with the stem {name}; elfie_units lists them"
            ),
        }),
        [index] => Ok(*index),
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
// @lfy def/mcp/main.lfy:request
fn request_for(workspace: &Workspace, plan: &Plan, batch: &Batch) -> Request {
    let existing: Vec<Output> = batch
        .units
        .iter()
        .flat_map(|&unit| plan.units[unit].outputs.iter())
        .filter_map(|map| {
            fs::read_to_string(workspace.root.join(&map.output))
                .ok()
                .map(|text| Output { path: map.output.clone(), text })
        })
        .collect();
    generation::request(workspace, plan, batch, &existing, &BTreeMap::new())
}

/// The request of the batch that holds a unit.
// Decision: a unit that is up to date is in no batch, because only planned units are
// batched; it is requested as a batch of its own, which is the request its batch would
// be, so a check of an up-to-date unit answers rather than failing.
// @lfy def/mcp/main.lfy:check
fn request_of_unit(workspace: &Workspace, plan: &Plan, unit: usize) -> Request {
    match plan.batches.iter().find(|batch| batch.units.contains(&unit)) {
        Some(batch) => request_for(workspace, plan, batch),
        None => request_for(
            workspace,
            plan,
            &Batch {
                units: vec![unit],
                identifier: plan.units[unit].stem.clone(),
            },
        ),
    }
}

/// The full request for one batch: what to produce, where, every criterion to satisfy,
/// and how to report.
// @lfy def/mcp/main.lfy:request
pub fn request(session: &Session, batch: &str, target: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    let plan = plan_of(workspace);
    let index = resolve_batch(workspace, &plan, batch, target)?; // @lfy def/mcp/main.lfy:request
    Ok(request_for(workspace, &plan, &plan.batches[index]).instructions) // @lfy def/mcp/main.lfy:request
}

/// Whether the outputs on disk satisfy the request for one unit, and what is wrong when
/// they do not. Nothing is written; the CLI records source maps.
// @lfy def/mcp/main.lfy:check
pub fn check(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    let plan = plan_of(workspace);
    let index = resolve_unit(workspace, &plan, unit, target)?;
    let request = request_of_unit(workspace, &plan, index);
    let outputs = outputs_of(workspace, &plan, index); // @lfy def/mcp/main.lfy:check
    let verdict = generation::accept(workspace, &plan, &request, index, &outputs);
    if verdict.accepted {
        // @lfy def/mcp/main.lfy:check
        let mut text = format!(
            "accepted {} for {}: {} output{}",
            plan.units[index].stem,
            workspace.targets[plan.units[index].target].identifier,
            outputs.len(),
            if outputs.len() == 1 { "" } else { "s" }
        );
        for output in &outputs {
            text.push_str(&format!("\n  {}", output.path));
        }
        text.push_str("\nnothing was written; the CLI records the source maps");
        Ok(text)
    } else {
        // @lfy def/mcp/main.lfy:check
        let mut text = format!("rejected {}:", plan.units[index].stem);
        for problem in &verdict.problems {
            text.push_str(&format!("\n  {problem}"));
        }
        Err(text)
    }
}

// ---- what the last acceptance recorded ---------------------------------------------

// Decision: the source maps these tools read come from each target's output directory, as
// elfie_units reads them through `load_source_maps`, so what a tool answers is what the
// last acceptance recorded, never a guess from the text on disk.

/// The language a fenced block of an output is marked with: the output's extension, or
/// nothing when it has none.
// @lfy def/mcp/main.lfy:output
fn fence_of(path: &str) -> &str {
    Path::new(path).extension().and_then(|extension| extension.to_str()).unwrap_or("")
}

/// The lines of an output from `first` through `last`, counting from 1, as they are on
/// disk; the empty text when the file cannot be read.
// @lfy def/mcp/main.lfy:output
fn excerpt(workspace: &Workspace, output: &str, first: usize, last: usize) -> String {
    let Ok(text) = fs::read_to_string(workspace.root.join(output)) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let from = first.saturating_sub(1).min(lines.len());
    let to = last.min(lines.len()).max(from);
    lines[from..to].join("\n")
}

/// Where the generated code for one entity is: every region of every output that came from
/// it, with its text.
// @lfy def/mcp/main.lfy:output
pub fn output(session: &Session, name: &str, target: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    check_target(workspace, target)?; // @lfy def/mcp/main.lfy:output
    let maps = load_source_maps(workspace);
    let regions = generation::regions_of(workspace, &maps, name, target); // @lfy def/mcp/main.lfy:output
    if regions.is_empty() {
        return Ok(format!("nothing is generated for {name}")); // @lfy def/mcp/main.lfy:output
    }
    // A region's output is the source map that holds it; `regions_of` keeps no path, and a
    // marker is matched by the same target and the same marker it was found by.
    // @lfy def/mcp/main.lfy:output
    let mut blocks = Vec::new();
    for map in &maps {
        if target.is_some_and(|target| map.target != target) {
            continue;
        }
        for marker in &map.markers {
            if !regions.contains(marker) {
                continue;
            }
            // @lfy def/mcp/main.lfy:output
            blocks.push(format!(
                "{}:{}-{}\n```{}\n{}\n```",
                map.output,
                marker.output_line,
                marker.end,
                fence_of(&map.output),
                excerpt(workspace, &map.output, marker.output_line, marker.end)
            ));
        }
    }
    Ok(blocks.join("\n\n"))
}

/// The criterion of the entity a marker names that begins on the marker's source line, as
/// `criteria_of` gives it; `None` when none does.
// @lfy def/mcp/main.lfy:source
fn criterion_on(workspace: &Workspace, marker: &Marker) -> Option<String> {
    let entity = marker.entity.as_ref()?;
    let model = &workspace.model;
    for &id in &query::find(workspace, &format!("{}:{entity}", marker.file)) {
        for criterion in model::criteria_of(model, id) {
            let Some(node) = criterion.node else { continue };
            let Some(token) = model.first_token(node) else { continue };
            if token.line == marker.line {
                return Some(criterion_text(&criterion));
            }
        }
    }
    None
}

/// Where one line of generated code came from: the definition file, the entity, and its
/// line.
// @lfy def/mcp/main.lfy:source
pub fn source(session: &Session, file: &str, line: usize) -> Result<String, String> {
    let workspace = &session.workspace;
    for map in load_source_maps(workspace) {
        if map.output != file {
            continue;
        }
        // @lfy def/mcp/main.lfy:source
        let Some(marker) = map.markers.iter().find(|marker| marker.covers(line)) else {
            continue;
        };
        let mut text = format!("{}:{}", marker.file, marker.line);
        if let Some(entity) = &marker.entity {
            text.push_str(&format!(" {entity}"));
        }
        // @lfy def/mcp/main.lfy:source
        if let Some(criterion) = criterion_on(workspace, marker) {
            text.push('\n');
            text.push_str(&criterion);
        }
        return Ok(text);
    }
    // @lfy def/mcp/main.lfy:source
    Ok(format!("no source map covers {file}:{line}"))
}

/// The unit with a stem, of the first target in `Workspace::targets` order that has one.
// @lfy def/mcp/main.lfy:changes
fn unit_of_stem(workspace: &Workspace, plan: &Plan, stem: &str) -> Result<usize, String> {
    for target in 0..workspace.targets.len() {
        if let Some(index) = (0..plan.units.len())
            .find(|&index| plan.units[index].stem == stem && plan.units[index].target == target)
        {
            return Ok(index);
        }
    }
    // @lfy def/mcp/main.lfy:changes
    Err(format!("no unit has the stem {stem}; elfie_units lists them"))
}

/// The source at the last accepted generation, recovered as the command line recovers it:
/// through git when the root is a repository and the unit's source is in it, taking the
/// revision of the file whose hash is the one the source map recorded.
// @lfy def/mcp/main.lfy:changes
fn previous_source(workspace: &Workspace, unit: &Unit) -> Option<String> {
    let map = unit.outputs.first()?;
    let file = &workspace.files[unit.file].path;
    let log = Command::new("git")
        .args(["log", "--format=%H", "-n", "50", "--", file])
        .current_dir(&workspace.root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !log.status.success() {
        return None;
    }
    for revision in String::from_utf8_lossy(&log.stdout).lines() {
        let show = Command::new("git")
            .args(["show", &format!("{revision}:{file}")])
            .current_dir(&workspace.root)
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !show.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&show.stdout).into_owned();
        if generation::source_hash(&text) == map.hash {
            return Some(text);
        }
    }
    None
}

/// What differs for each entity of a unit since its outputs were last accepted.
// @lfy def/mcp/main.lfy:changes
pub fn changes(session: &Session, unit: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let plan = plan_of(workspace);
    let index = unit_of_stem(workspace, &plan, unit)?;
    let planned = &plan.units[index];
    if planned.outputs.is_empty() {
        return Ok(format!("no previous output for {unit}; nothing has been accepted for it yet")); // @lfy def/mcp/main.lfy:changes
    }
    let previous = previous_source(workspace, planned); // @lfy def/mcp/main.lfy:changes
    let mut text = String::new();
    if previous.is_none() {
        // @lfy def/mcp/main.lfy:changes
        text.push_str("the source the outputs were generated from cannot be recovered; every entity is listed as added\n");
    }
    let found = generation::changes(workspace, planned, previous.as_deref()); // @lfy def/mcp/main.lfy:changes
    if found.is_empty() {
        return Ok(format!("no changes in {unit} since its outputs were accepted")); // @lfy def/mcp/main.lfy:changes
    }
    // @lfy def/mcp/main.lfy:changes
    let lines: Vec<String> = found
        .iter()
        .map(|change| format!("{} {} {}", change.entity, change.kind.as_str(), change.detail))
        .collect();
    text.push_str(&lines.join("\n"));
    Ok(text)
}

/// The full review request for one batch: every criterion and test with the regions
/// generated for its entities, and how to report.
// @lfy def/mcp/main.lfy:review
pub fn review(session: &Session, batch: &str, target: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    let plan = plan_of(workspace);
    let index = resolve_batch(workspace, &plan, batch, target)?; // @lfy def/mcp/main.lfy:review
    // @lfy def/mcp/main.lfy:review
    Ok(generation::review(workspace, &plan, &plan.batches[index], &load_source_maps(workspace)).instructions)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// The directory of the library a fixture carries, relative to its root.
    const LIBRARY_ROOT: &str = "lib";
    /// The library a fixture carries: the trait `target`, which a target's marker must
    /// extend, in the prelude every file of the project sees. It also declares the kind
    /// data `Entity` and `Trait`, so that a target package's `rust.apply(global)` reads
    /// `apply` as a member of the marker's kind rather than as a name nothing declares.
    const LIBRARY: &str = concat!(
        "d Entity: `What every declared thing is seen as through its context layer` {\n}\n\n",
        "d Trait extends Entity: `A trait seen through its context layer` {\n",
        "  $apply: `Applies the trait to a target and returns the trait` = (target: Entity) => Trait;\n}\n\n",
        "trait target { }\n"
    );
    /// The manifest of a fixture: it names the library the fixture carries.
    const LIBRARY_MANIFEST: &str = r#"{ "lib": "lib" }"#;

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
            fixture
                .write(MANIFEST, LIBRARY_MANIFEST)
                .write(&format!("{LIBRARY_ROOT}/main.lfy"), LIBRARY);
            fixture
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

    // @lfy def/mcp/main.lfy:serve
    #[test]
    fn the_tools_are_listed_and_problems_answers_with_the_binder_error() {
        let names: Vec<String> = tools().into_iter().map(|registered| registered.tool.name).collect();
        assert_eq!(
            names,
            [
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
                "elfie_source",
                "elfie_changes",
                "elfie_review",
            ]
        );
        let listed = traits::list(&tools());
        assert_eq!(listed.len(), 13);
        assert_eq!(listed[0].input_schema["required"], serde_json::json!([]));
        assert_eq!(listed[1].input_schema["required"], serde_json::json!(["name"]));
        // A line is a number, and is listed as one. @lfy def/mcp/main.lfy:source
        assert_eq!(listed[10].input_schema["properties"]["line"]["type"], "number");
        assert_eq!(listed[10].input_schema["required"], serde_json::json!(["file", "line"]));

        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = z;\n");
        let session = fixture.session();
        let text = problems(&session, None).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.starts_with("def/a.lfy:1:10: binder error: "), "{text}");
        let result = call(&session, "elfie_problems", serde_json::json!({}));
        assert!(!result.is_error);
        assert_eq!(result.text, text);
        let result = call(&session, "elfie_problems", serde_json::json!({ "file": "def/a.lfy" }));
        assert_eq!(result.text, text);
    }

    // @lfy def/mcp/main.lfy:problems
    #[test]
    fn problems_says_when_there_are_none() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = 1;\n");
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        assert_eq!(problems(&session, Some("def/a.lfy")).unwrap(), "no problems in def/a.lfy");
        // A file the program does not hold has no problems either, and is answered with
        // text rather than a failure. @lfy def/mcp/main.lfy:problems
        assert_eq!(problems(&session, Some("def/b.lfy")).unwrap(), "no problems in def/b.lfy");
    }

    // @lfy def/mcp/traits.lfy:tool
    #[test]
    fn a_bad_argument_is_an_error_naming_it() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = 1;\n");
        let session = fixture.session();
        let result = call(&session, "elfie_find", serde_json::json!({}));
        assert!(result.is_error);
        assert!(result.text.contains("name"), "{}", result.text);
        let result = call(&session, "elfie_find", serde_json::json!({ "name": "y", "extra": true }));
        assert!(result.is_error);
        assert!(result.text.contains("extra"), "{}", result.text);
        let result = call(&session, "elfie_outline", serde_json::json!({ "file": 1 }));
        assert!(result.is_error);
        assert!(result.text.contains("file") && result.text.contains("string"), "{}", result.text);
        // The fn fails: an error carrying the failure. @lfy def/mcp/traits.lfy:tool
        let result = call(&session, "elfie_outline", serde_json::json!({ "file": "def/none.lfy" }));
        assert!(result.is_error);
        assert!(result.text.contains("def/a.lfy"), "{}", result.text);
    }

    // @lfy def/mcp/main.lfy:serve
    #[test]
    fn changed_vanished_and_new_files_are_reread_before_a_call() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = z;\n");
        let mut session = fixture.session();
        assert!(problems(&session, None).unwrap().contains("binder error"));

        fixture.write("def/a.lfy", "const z = 1;\nconst y = z;\n");
        session.refresh();
        assert_eq!(problems(&session, None).unwrap(), "no problems");

        fixture.write("def/b.lfy", "const w = missing;\n");
        session.refresh();
        assert!(session.workspace.file("def/b.lfy").is_some());
        assert!(problems(&session, None).unwrap().contains("def/b.lfy:1:10"));

        fixture.remove("def/b.lfy");
        session.refresh();
        assert!(session.workspace.file("def/b.lfy").is_none());
        assert_eq!(problems(&session, None).unwrap(), "no problems");

        fixture.write("elfie.json", r#"{ "name": "renamed", "source": "src", "lib": "lib" }"#);
        fixture.write("src/c.lfy", "const c = 1;\n");
        session.refresh();
        assert_eq!(session.workspace.name, "renamed");
        assert!(session.workspace.file("src/c.lfy").is_some());
        assert!(session.workspace.file("def/a.lfy").is_none());
    }

    // @lfy def/mcp/main.lfy:find
    #[test]
    fn find_entity_and_references_describe_a_declaration() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "/// Doc\nd A: `An A` { $x: `An x` = string; }\nconst y = A;\n");
        let session = fixture.session();

        let found = find(&session, "A").unwrap();
        assert_eq!(found, "data A def/a.lfy:2: An A");
        assert_eq!(find(&session, "def/a.lfy:A").unwrap(), found);
        let none = find(&session, "nothing").unwrap();
        assert!(none.contains("elfie_outline"), "{none}"); // @lfy def/mcp/main.lfy:find

        // @lfy def/mcp/main.lfy:entity
        let described = entity(&session, "A").unwrap();
        assert!(described.starts_with("# data A (def/a.lfy:2)"), "{described}");
        assert!(described.contains("## Definition\nAn A"), "{described}");
        assert!(described.contains("## Documentation\nDoc"), "{described}");
        assert!(described.contains("## Members\n- x: An x (string)"), "{described}");
        assert!(described.contains("## References (1)\n- def/a.lfy:3"), "{described}");
        assert!(described.contains("## Source\n```elfie\n/// Doc\nd A: `An A` { $x: `An x` = string; }\n```"), "{described}");
        assert!(!described.contains("\n---\n"));

        // @lfy def/mcp/main.lfy:references
        let used = references(&session, "A").unwrap();
        assert_eq!(used, "def/a.lfy:3:10: const y = A;");
        assert_eq!(references(&session, "y").unwrap(), "y is never used");
    }

    // @lfy def/mcp/main.lfy:entity
    #[test]
    fn entity_lists_the_traits_it_carries() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "trait t: `A t` {}\nd A is t {}\n");
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        let described = entity(&session, "A").unwrap();
        assert!(described.contains("## Traits\nt\n"), "{described}");
    }

    // Every field of the hover has a heading, `Hover.owner` among them: a member found as
    // `A.x` is headed by the declaration that declares it.
    // @lfy def/mcp/main.lfy:entity
    #[test]
    fn entity_names_the_owner_of_a_member() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "d A { $x: `An x` = string; }\n");
        let session = fixture.session();
        let described = entity(&session, "A.x").unwrap();
        assert!(described.contains("## Owner\nA\n"), "{described}");
        // A declaration that nothing owns has no such heading.
        assert!(!entity(&session, "A").unwrap().contains("## Owner"));
    }

    // @lfy def/mcp/main.lfy:entity
    #[test]
    fn several_entities_are_separated_by_a_rule_and_headed_by_their_file() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "d A {}\n").write("def/b.lfy", "d A {}\n");
        let session = fixture.session();
        let described = entity(&session, "A").unwrap();
        assert!(described.contains("\n---\n"), "{described}");
        assert!(described.contains("(def/a.lfy:1)") && described.contains("(def/b.lfy:1)"), "{described}");
        let narrowed = entity(&session, "def/b.lfy:A").unwrap();
        assert!(!narrowed.contains("\n---\n"), "{narrowed}");
        assert!(narrowed.contains("(def/b.lfy:1)"), "{narrowed}");
    }

    // @lfy def/mcp/main.lfy:outline
    #[test]
    fn outline_indents_declarations_or_lists_the_files() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "/// Doc\nd A { $x = string; }\nconst y = 1;\n");
        let session = fixture.session();
        assert_eq!(outline(&session, "def/a.lfy").unwrap(), "data A 2\n  member x 2\nvariable y 3");
        // @lfy def/mcp/main.lfy:outline
        let error = outline(&session, "def/zzz.lfy").unwrap_err();
        assert!(error.contains("def/zzz.lfy is not in the program") && error.contains("def/a.lfy"), "{error}");
    }

    // @lfy def/mcp/main.lfy:grammar
    #[test]
    fn grammar_holds_the_terminals_and_the_rules() {
        let fixture = Fixture::new();
        let session = fixture.session();
        let text = grammar(&session).unwrap();
        assert!(text.contains("SourceFile"), "{text}");
        assert!(text.contains("\n\n"), "{text}");
        assert_eq!(
            text,
            format!("{}\n\n{}", elfie_core::grammar::terminal_document(), elfie_core::grammar::grammar_document())
        );
    }

    fn compiled_project() -> Fixture {
        let fixture = Fixture::new();
        fixture
            .write(
                "elfie.json",
                r#"{
                    "name": "p",
                    "lib": "lib",
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust", "output": "out" } }
                }"#,
            )
            .write("targets/rust/main.lfy", "trait rust extends target: `Built as Rust` {}\nrust.apply(global);\n")
            .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
            .write("def/b.lfy", "d B {}\n");
        fixture
    }

    // @lfy def/mcp/main.lfy:units
    #[test]
    fn units_lists_every_unit_and_batch_or_one_targets() {
        let fixture = compiled_project();
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        let listed = "rust b fresh []\nrust a fresh [b]\n\nbatch b+1 [b, a]";
        assert_eq!(units(&session, None).unwrap(), listed);
        assert_eq!(units(&session, Some("rust")).unwrap(), listed);
        // @lfy def/mcp/main.lfy:units
        let error = units(&session, Some("go")).unwrap_err();
        assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
    }

    // @lfy def/mcp/main.lfy:request
    #[test]
    fn request_gives_the_instructions_of_one_batch() {
        let fixture = compiled_project();
        let session = fixture.session();
        // A batch is named by its identifier or by the stem of one of its units.
        let instructions = request(&session, "b+1", None).unwrap();
        assert!(instructions.contains("An A"), "{instructions}");
        assert!(instructions.contains("def/a.lfy"), "{instructions}");
        assert!(instructions.contains("def/b.lfy"), "{instructions}");
        assert_eq!(request(&session, "a", None).unwrap(), instructions);
        assert_eq!(request(&session, "a", Some("rust")).unwrap(), instructions);
        // @lfy def/mcp/main.lfy:request
        let error = request(&session, "c", None).unwrap_err();
        assert!(error.contains("no batch has the identifier c"), "{error}");
        let error = request(&session, "a", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target"), "{error}");
    }

    // @lfy def/mcp/main.lfy:check
    #[test]
    fn check_reads_the_outputs_from_disk_and_writes_nothing() {
        let fixture = compiled_project();
        let session = fixture.session();
        // Rejected: an error listing the problems. @lfy def/mcp/main.lfy:check
        let error = check(&session, "b", None).unwrap_err();
        assert!(error.starts_with("rejected b:"), "{error}");
        fixture.write("out/b.rs", "// @lfy def/b.lfy:1\npub struct B;\n");
        // Accepted: the text says so and lists the outputs. @lfy def/mcp/main.lfy:check
        let text = check(&session, "b", None).unwrap();
        assert!(text.starts_with("accepted b for rust: 1 output\n  out/b.rs"), "{text}");
        assert!(!fixture.root.join("out/source-map.json").exists());
        let error = check(&session, "a", None).unwrap_err();
        assert!(error.contains("rejected a:"), "{error}");
    }

    /// A project whose unit `b` has an accepted output, with the source maps recorded in
    /// the target's output directory the way the command line records them.
    fn accepted_project() -> Fixture {
        let fixture = compiled_project();
        fixture.write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
        let session = fixture.session();
        let workspace = &session.workspace;
        let plan = plan_of(workspace);
        let index = resolve_unit(workspace, &plan, "b", None).unwrap();
        let request = request_of_unit(workspace, &plan, index);
        let outputs = outputs_of(workspace, &plan, index);
        let verdict = generation::accept(workspace, &plan, &request, index, &outputs);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        generation::write_source_maps(&fixture.root.join("out/source-map.json"), &verdict.source_maps).unwrap();
        fixture
    }

    // @lfy def/mcp/main.lfy:output
    #[test]
    fn output_gives_every_region_of_an_entity_with_its_text() {
        let fixture = accepted_project();
        let session = fixture.session();
        let text = output(&session, "B", None).unwrap();
        assert_eq!(text, "out/b.rs:1-2\n```rs\n// @lfy def/b.lfy:B\npub struct B;\n```");
        assert_eq!(output(&session, "B", Some("rust")).unwrap(), text);
        // Nothing is generated for the name. @lfy def/mcp/main.lfy:output
        let none = output(&session, "A", None).unwrap();
        assert_eq!(none, "nothing is generated for A");
        // The target is not a known one. @lfy def/mcp/main.lfy:output
        let error = output(&session, "B", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
    }

    // @lfy def/mcp/main.lfy:source
    #[test]
    fn source_names_the_definition_a_line_came_from() {
        let fixture = accepted_project();
        let session = fixture.session();
        assert_eq!(source(&session, "out/b.rs", 1).unwrap(), "def/b.lfy:1 B");
        assert_eq!(source(&session, "out/b.rs", 2).unwrap(), "def/b.lfy:1 B");
        // No source map has the file as its output. @lfy def/mcp/main.lfy:source
        let text = source(&session, "out/a.rs", 1).unwrap();
        assert_eq!(text, "no source map covers out/a.rs:1");
        // A line past the region of every marker. @lfy def/mcp/main.lfy:source
        assert_eq!(source(&session, "out/b.rs", 9).unwrap(), "no source map covers out/b.rs:9");
        // A number argument reaches the fn as a number. @lfy def/mcp/main.lfy:source
        let result = call(&session, "elfie_source", serde_json::json!({ "file": "out/b.rs", "line": 2 }));
        assert!(!result.is_error, "{}", result.text);
        assert_eq!(result.text, "def/b.lfy:1 B");
        let result = call(&session, "elfie_source", serde_json::json!({ "file": "out/b.rs", "line": "2" }));
        assert!(result.is_error);
        assert!(result.text.contains("line") && result.text.contains("number"), "{}", result.text);
    }

    // @lfy def/mcp/main.lfy:source
    #[test]
    fn source_quotes_a_criterion_that_begins_on_the_line() {
        let fixture = Fixture::new();
        fixture
            .write(
                "elfie.json",
                r#"{
                    "name": "p",
                    "lib": "lib",
                    "dependencies": { "rust": { "root": "targets/rust" } },
                    "targets": { "rust": { "package": "rust", "marker": "rust", "output": "out" } }
                }"#,
            )
            .write("targets/rust/main.lfy", "trait rust extends target: `Built as Rust` {}\nrust.apply(global);\n")
            // The fn and its one criterion are written on one line, so the criterion begins
            // on the line the marker of the fn names.
            .write("def/b.lfy", "fn b(): `A b` => string { @acceptanceCriteria.add({ behavior = `It answers` }); }\n")
            .write("out/b.rs", "// @lfy def/b.lfy:b\npub fn b() -> String {\n    String::new()\n}\n");
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        let workspace = &session.workspace;
        let plan = plan_of(workspace);
        let index = resolve_unit(workspace, &plan, "b", None).unwrap();
        let request = request_of_unit(workspace, &plan, index);
        let outputs = outputs_of(workspace, &plan, index);
        let verdict = generation::accept(workspace, &plan, &request, index, &outputs);
        assert!(verdict.accepted, "{:?}", verdict.problems);
        generation::write_source_maps(&fixture.root.join("out/source-map.json"), &verdict.source_maps).unwrap();
        // A criterion of the entity begins on the line the marker names, so its text
        // follows on the next line. @lfy def/mcp/main.lfy:source
        let session = fixture.session();
        assert_eq!(source(&session, "out/b.rs", 2).unwrap(), "def/b.lfy:1 b\nIt answers");
    }

    // @lfy def/mcp/main.lfy:changes
    #[test]
    fn changes_reports_what_differs_since_the_outputs_were_accepted() {
        let fixture = accepted_project();
        let session = fixture.session();
        // The unit has outputs and the previous text cannot be recovered: the text says so
        // first, then every entity as added. @lfy def/mcp/main.lfy:changes
        let text = changes(&session, "b").unwrap();
        assert!(text.starts_with("the source the outputs were generated from cannot be recovered"), "{text}");
        assert!(text.contains("\nB added "), "{text}");
        // The outputs of a unit are empty. @lfy def/mcp/main.lfy:changes
        let text = changes(&session, "a").unwrap();
        assert!(text.starts_with("no previous output for a"), "{text}");
        // No unit has that stem. @lfy def/mcp/main.lfy:changes
        let error = changes(&session, "zzz").unwrap_err();
        assert!(error.contains("no unit has the stem zzz"), "{error}");
    }

    // @lfy def/mcp/main.lfy:changes
    #[test]
    fn changes_says_so_when_nothing_differs() {
        let fixture = accepted_project();
        let session = fixture.session();
        let workspace = &session.workspace;
        let plan = plan_of(workspace);
        let index = unit_of_stem(workspace, &plan, "b").unwrap();
        // The file as it is now is the file the outputs were generated from, so nothing
        // differs. @lfy def/mcp/main.lfy:changes
        let text = fs::read_to_string(fixture.root.join("def/b.lfy")).unwrap();
        assert!(generation::changes(workspace, &plan.units[index], Some(&text)).is_empty());
    }

    // @lfy def/mcp/main.lfy:review
    #[test]
    fn review_gives_the_review_request_of_one_batch() {
        let fixture = compiled_project();
        let session = fixture.session();
        let instructions = review(&session, "b+1", None).unwrap();
        assert!(instructions.contains("Reviewing the batch"), "{instructions}");
        assert!(instructions.contains("def/a.lfy"), "{instructions}");
        assert_eq!(review(&session, "a", None).unwrap(), instructions);
        assert_eq!(review(&session, "a", Some("rust")).unwrap(), instructions);
        // No batch has that identifier or holds a unit with that stem, or the target is
        // ambiguous. @lfy def/mcp/main.lfy:review
        let error = review(&session, "c", None).unwrap_err();
        assert!(error.contains("no batch has the identifier c"), "{error}");
        let error = review(&session, "a", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target"), "{error}");
    }
}
