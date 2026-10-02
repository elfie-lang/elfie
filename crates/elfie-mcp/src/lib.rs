//! Compiled from `def/mcp/main.lfy`: the Elfie agent server.
//!
//! One tool per fn of the definition that carries `tool`, spoken over the Model Context
//! Protocol on standard input and output. The server never writes a file: during
//! compilation the agent writes the outputs itself and asks the server to check them, so
//! one path exists for writing and the CLI owns it.

pub mod data;
pub mod traits; // @lfy def/mcp/traits.lfy:tool

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use elfie_core::generation::{self, Batch, Output, Plan, Request, Review, ReviewStatus, SourceMap, Unit};
use elfie_core::interpret::{self, LoweredCriterion, LoweredNode, LoweredTest, Program};
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

/// Where the command line keeps its own files, under the root.
const REQUESTS: &str = "elfie-requests";

/// The file under [`REQUESTS`] holding the reviews of the last global review, as the
/// command line writes them for the batch named `global`.
const GLOBAL_REVIEWS: &str = "global.reviews.json";

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
    // @lfy def/mcp/main.lfy:serve#serve:serve:30dc14e0f289c1258311c706561d518b00e9be0a7c99c2b8bf4f4fa51f427dfa:2
    pub fn load(root: &Path) -> Session {
        let workspace = workspace::load(root);
        let seen = stamps(&workspace);
        Session { workspace, seen }
    }

    /// What runs first when a call arrives: every file of the program whose modification
    /// time or size differs from what was seen is re-read through `workspace::change`
    /// with no text, and a file that vanished the same way.
    ///
    /// When `elfie.json` has changed since the last call, the workspace is loaded again
    /// instead, before the tool runs.
    // @lfy def/mcp/main.lfy:serve#serve:serve:2ed948e8109fe95f42a2b62690e2684424ec56255d6b6a83239254c5ec9e6bac
    pub fn refresh(&mut self) {
        let now = stamps(&self.workspace);
        // elfie.json has changed since the last call.
        // @lfy def/mcp/main.lfy:serve#serve:serve:a15988b7cf93a498158fb3af594b966703670dfeb7345699b2895672b01ba527
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
// @lfy def/mcp/main.lfy:serve#serve:serve:d9931c3e01bb8f4c0e9c97a72214638b20e7196300b18be309cb4efdce4d7a97
pub fn tools() -> Vec<Registered> {
    vec![
        // @lfy def/mcp/main.lfy:problems#problems:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:problems#problems:tool:17ca63f6e667cdbc9879b6a9a4b465bfa0cad3d8e08ea4e201854602d62910b7
        Registered::new(
            "problems",
            "The problems of the program, or of one file: what the compiler, an editor, or a check would report",
            vec![ToolArgument::new("file", "a path relative to the root; every file when left out", "string", false)], // @lfy def/mcp/main.lfy:problems#problems:tool:8f654b838d201aebfdc3e97dd3b8dfa64d7053fb975ab5821b79f38ddc01224e
            |session, arguments| problems(session, arguments.optional("file")), // @lfy def/mcp/main.lfy:problems#problems:tool:ba984747e1aaa3e0ab0760a7b8cf647241ebdec4a6507096e3bf3761b78fa979
        ),
        // @lfy def/mcp/main.lfy:find#find:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:find#find:tool:0cd55c9a9215aa9debeee8f11681215ec6e96d0d0e1abbeaabe5436f734ae0cc
        Registered::new(
            "find",
            "Where things with a name are declared",
            vec![ToolArgument::new(
                "name",
                "an identifier, or an owner's identifier, a dot, and a member's name; a file path and a colon before it limits the search to that file",
                "string",
                true,
            )], // @lfy def/mcp/main.lfy:find#find:tool:f13d4dcdbe8552ea1c3f43754ac5ce5e29397b23b3936e76b1017bc3f8c60f16
            |session, arguments| find(session, arguments.string("name")?), // @lfy def/mcp/main.lfy:find#find:tool:971d97377faf643c4e9200c29b6037e932d89de733be4474c0ef712ed7417fbf
        ),
        // @lfy def/mcp/main.lfy:entity#entity:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:entity#entity:tool:ccc67cdf1bedf034f2bde07bd97506ffcd15cdd01ea0ec17beb8cc841a9f6d79
        Registered::new(
            "entity",
            "Everything known about the things with a name: definition, type, documentation, traits, members, criteria, tests, references, and source",
            vec![ToolArgument::new("name", "as elfie_find takes it", "string", true)], // @lfy def/mcp/main.lfy:entity#entity:tool:b6aa1b019f1b493d0c9418e516600a41340437c76f900a3794df2f3ea3c75609
            |session, arguments| entity(session, arguments.string("name")?), // @lfy def/mcp/main.lfy:entity#entity:tool:09653d28f4630ddcb5c955d076d309d0fc7a5058322856e311ef30796a91ff94
        ),
        // @lfy def/mcp/main.lfy:references#references:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:references#references:tool:86d7e831b0adefe46b7536a2fb7c28f76658a57f836fa4c0ab091ed635b70bec
        Registered::new(
            "references",
            "Every place things with a name are used",
            vec![ToolArgument::new("name", "as elfie_find takes it", "string", true)], // @lfy def/mcp/main.lfy:references#references:tool:ba3b707c7265a3b448f32076f312f3553c4fe4b270967fe5084489925b67b56f
            |session, arguments| references(session, arguments.string("name")?), // @lfy def/mcp/main.lfy:references#references:tool:96f9fa5d9bb8e15400bdf8aac37e9eba655862c814a4798bd80d3528d9f45fc0
        ),
        // @lfy def/mcp/main.lfy:outline#outline:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:outline#outline:tool:84fbac735258b0b9d0c31e5e2a219f24b5adadd842e432d29fd7d1b15a64558c
        Registered::new(
            "outline",
            "The declarations of one file, nested as written",
            vec![ToolArgument::new("file", "a path relative to the root", "string", true)], // @lfy def/mcp/main.lfy:outline#outline:tool:4175822c2a911ce5792759f98c7ecee206ca9b48728999d274fef32d08f3cfd2
            |session, arguments| outline(session, arguments.string("file")?), // @lfy def/mcp/main.lfy:outline#outline:tool:05297e36a3205bb11b7fa733927c97804ec2a484a1a58a7ac53d14ad318d1259
        ),
        // @lfy def/mcp/main.lfy:grammar#grammar:tool:b4f8ca7e607593fdc678dbde27166be80f48381b3bc80409b7c33613ea7b82c3
        Registered::new(
            "grammar",
            "The grammar of Elfie: every terminal and every rule as EBNF, for an agent writing or reading Elfie",
            vec![],
            |session, _| grammar(session), // @lfy def/mcp/main.lfy:grammar#grammar:tool:e55c1246f92efcfb31fcb0aa2bbd915ae41f53a5bcdf9a8b5d21118f9e915203
        ),
        // @lfy def/mcp/main.lfy:units#units:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:units#units:tool:81cd7489ff3725350f3c57bd392f753bf2d9cbc7211f1852a9331ee24cd102bd
        Registered::new(
            "units",
            "The units the compiler would produce, in order, why each needs generating, and how they are batched",
            vec![ToolArgument::new("target", "the identifier of one target; every target when left out", "string", false)], // @lfy def/mcp/main.lfy:units#units:tool:1307215a8c30b97e22bb98b0897c538d42a97e5b7ed5d42b9ef2593406378026
            |session, arguments| units(session, arguments.optional("target")), // @lfy def/mcp/main.lfy:units#units:tool:edc95ccec49170f3440ba894ee6a2f192b531465792dac68b0a714c684734eaf
        ),
        // @lfy def/mcp/main.lfy:request#request:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:request#request:tool:32dafdd79c78beafdc323c9618c1ff14f63ea291c78e93d93f2d831c3ac192f7
        Registered::new(
            "request",
            "The full request for one batch: what to produce, where, every criterion to satisfy, and how to report",
            vec![
                ToolArgument::new("batch", "the identifier of a batch, as elfie_units lists it, or the stem of one of its units", "string", true), // @lfy def/mcp/main.lfy:request#request:tool:3835a8bcee5ea0587724df128816d69698e0f81fc35d40749e994710946fea04
                ToolArgument::new("target", "the identifier of the batch's target; the only target when left out", "string", false), // @lfy def/mcp/main.lfy:request#request:tool:2bd3ac95d0d78299d660aa774d66563e6ebe22f33bb27316d507eb413d5c323c
            ],
            |session, arguments| request(session, arguments.string("batch")?, arguments.optional("target")), // @lfy def/mcp/main.lfy:request#request:tool:21624fc480793bea9dd29edaf1d50fccc5aed9ecf5be031baba40f6aef69424c
        ),
        // @lfy def/mcp/main.lfy:check#check:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:check#check:tool:d1d7751090d10833f550a85e215903e679fbde0641f76637cea1357bf98f1343
        Registered::new(
            "check",
            "Whether the outputs on disk satisfy the request for one unit, and what is wrong when they do not",
            vec![
                ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true), // @lfy def/mcp/main.lfy:check#check:tool:3f58ad245ad2485de6b988e2ff5a5f2f597a7ad145fd8507f917356ada3e31c1
                ToolArgument::new("target", "the identifier of the unit's target; the only target when left out", "string", false), // @lfy def/mcp/main.lfy:check#check:tool:e95fdedec458dd9ef409f39811501324c3d70ce2921b0e05587b1f3d7feebde0
            ],
            |session, arguments| check(session, arguments.string("unit")?, arguments.optional("target")), // @lfy def/mcp/main.lfy:check#check:tool:2a52a0e9b2bd82bebf01e1bfd3aed5fca61ba5453af95cacc85f4e3b5c4cc01e
        ),
        // @lfy def/mcp/main.lfy:output#output:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:output#output:tool:0a353dbe87fe2f24754698d3ccace24eacf8f9a7bcb49966b66686381694b1a8
        Registered::new(
            "output",
            "Where the generated code for one entity is: every region of every output that came from it, with its text",
            vec![
                ToolArgument::new("name", "as elfie_find takes it", "string", true), // @lfy def/mcp/main.lfy:output#output:tool:e1e9205de453bce452d201f0ae4ccfd4931d2c86b262c4e9a71757f8f4eecbcb
                ToolArgument::new("target", "the identifier of one target; every target when left out", "string", false), // @lfy def/mcp/main.lfy:output#output:tool:df4d1829eaa070671b3ff74634d700eec5b1837b3bcff7bbc0c1a8eb6026924b
            ],
            |session, arguments| output(session, arguments.string("name")?, arguments.optional("target")), // @lfy def/mcp/main.lfy:output#output:tool:6a5f51bd5a5b426c26c7b0f93d9ead569e730c9845dcdb3dda87d68e2dd3211b
        ),
        // @lfy def/mcp/main.lfy:source#source:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:source#source:tool:328e4667b06904decebf22779935ab215a2d8f2eb78fc9a7b6f761d4858bf0cb
        Registered::new(
            "source",
            "Where one line of generated code came from: the definition file, the entity, and its line",
            vec![
                ToolArgument::new("file", "the path of an output file, relative to the root", "string", true), // @lfy def/mcp/main.lfy:source#source:tool:8cf5dd82604b9b82847138935ffb981338930f6f065382a4a60887b442a0c9bb
                ToolArgument::new("line", "a line of that file, counting from 1", "number", true), // @lfy def/mcp/main.lfy:source#source:tool:8cf5dd82604b9b82847138935ffb981338930f6f065382a4a60887b442a0c9bb
            ],
            |session, arguments| source(session, arguments.string("file")?, counted(arguments.string("line")?, "line")?), // @lfy def/mcp/main.lfy:source#source:tool:94844af73619c1ae0e1f4c2d4a41848ebd3a208ff53fa3e946a3d7d4514e41bf
        ),
        // @lfy def/mcp/main.lfy:changes#changes:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:changes#changes:tool:f282ace4db2bc533bcd7dc35861d19cb9b43e14b65324c5f4511f23f5c7761cd
        Registered::new(
            "changes",
            "What differs for each entity of a unit since its outputs were last accepted",
            vec![ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true)], // @lfy def/mcp/main.lfy:changes#changes:tool:f079884195ecc3178deca5c8fd1123133ae18bffd34392ecbcdcb2e62c27df8b
            |session, arguments| changes(session, arguments.string("unit")?), // @lfy def/mcp/main.lfy:changes#changes:tool:6b6d8ff9fad1a9f12427a520cdabdd267f6edc140826dad97721a254c9d2f671
        ),
        // @lfy def/mcp/main.lfy:review#review:tool:752ffa3b700084b21ec5f1b0bcb96c21eb1b5fff7e49097defad0b9e0d9bf1b0
        // @lfy def/mcp/main.lfy:review#review:tool:ef40074c274ea43ef48cf64991147d66153b64d5522fd50cfddc20c419bba321
        Registered::new(
            "review",
            "The full review request for one batch: every criterion and test with the regions generated for its entities, and how to report",
            vec![
                ToolArgument::new("batch", "the identifier of a batch, as elfie_units lists it, or the stem of one of its units", "string", true), // @lfy def/mcp/main.lfy:review#review:tool:14e0a7680715ac2db5c61183cd6d5c6cb4fbf30503129ca3d6d907bb3d023bc2
                ToolArgument::new("target", "the identifier of the batch's target; the only target when left out", "string", false), // @lfy def/mcp/main.lfy:review#review:tool:46926256fcfb0dbf1e82d57b77c9977134588024496854782c23932c3401876c
            ],
            |session, arguments| review(session, arguments.string("batch")?, arguments.optional("target")), // @lfy def/mcp/main.lfy:review#review:tool:1fb13faa68c6e011088d16fa4491c46838e4455437505ce4260d6495d8701ad4
        ),
        // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:2b382037e6282cf364f5ca0c6737a6699ebff47bcb8ea7b76667117272d7c659
        Registered::new(
            "globalReview",
            "The review request for every global criterion and test, once for the whole program, with the regions that answer for each",
            vec![],
            |session, _| global_review(session), // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:6d07ccf09bafe4f0f34c5bc9109ccc2c6feab9da69710f76fc9b06acb461d004
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
    // @lfy def/mcp/main.lfy:serve#serve:serve:30dc14e0f289c1258311c706561d518b00e9be0a7c99c2b8bf4f4fa51f427dfa:2
    fn new(root: &Path) -> Server {
        Server {
            session: Mutex::new(Session::load(root)),
            tools: tools(),
        }
    }

    /// One call: the session is refreshed, then the tool runs.
    // @lfy def/mcp/main.lfy:serve#serve:serve:2ed948e8109fe95f42a2b62690e2684424ec56255d6b6a83239254c5ec9e6bac
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
    // @lfy def/mcp/main.lfy:serve#serve:serve:d9931c3e01bb8f4c0e9c97a72214638b20e7196300b18be309cb4efdce4d7a97
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18) // @lfy def/mcp/main.lfy:Protocol
            .with_server_info(Implementation::new("elfie", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "The Elfie agent server. elfie_problems, elfie_find, elfie_entity, elfie_references, and elfie_outline read the program; elfie_grammar gives the language; elfie_units, elfie_request, elfie_check, elfie_review, and elfie_globalReview drive compilation; elfie_output, elfie_source, and elfie_changes read what the last acceptance recorded. Nothing here writes a file.",
            )
    }

    // @lfy def/mcp/main.lfy:serve#serve:serve:d9931c3e01bb8f4c0e9c97a72214638b20e7196300b18be309cb4efdce4d7a97
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(traits::list(&self.tools)))
    }

    // @lfy def/mcp/main.lfy:serve#serve:serve:a24c91e6fcf0cf6896e12a456432152ce48cb18be4ef1982bb91c77db6e07173
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
// @lfy def/mcp/main.lfy:serve#serve:serve:a24c91e6fcf0cf6896e12a456432152ce48cb18be4ef1982bb91c77db6e07173
pub fn serve(root: Option<&Path>) -> i32 {
    // @lfy def/mcp/main.lfy:serve#serve:serve:f26fa4314eb0b1386d19aef47088b65a9d06b1de778be454db0da4e91712efe6
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
        // @lfy def/mcp/main.lfy:serve#serve:serve:a24c91e6fcf0cf6896e12a456432152ce48cb18be4ef1982bb91c77db6e07173
        let running = match server.serve(rmcp::transport::stdio()).await {
            Ok(running) => running,
            // Decision: standard input closing before the agent initializes is the
            // connection closing, so it exits with 0 like any other close.
            // @lfy def/mcp/main.lfy:serve#serve:serve:18d7a1e7b3b3dce603349f163b7d1c41fbff67f36886f831ddd63cd179f2f427
            Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return 0,
            Err(error) => {
                eprintln!("elfie mcp: {error}");
                return FAILURE;
            }
        };
        // @lfy def/mcp/main.lfy:serve#serve:serve:18d7a1e7b3b3dce603349f163b7d1c41fbff67f36886f831ddd63cd179f2f427
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
// @lfy def/mcp/main.lfy:problems#problems:problems:16582985bd017d251f6d9debd25fd20b2b40f301c842080725fb272024f6b942
pub fn problems(session: &Session, file: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    let diagnostics = query::diagnostics_of(workspace, file); // @lfy def/mcp/main.lfy:problems#problems:problems:16582985bd017d251f6d9debd25fd20b2b40f301c842080725fb272024f6b942
    if diagnostics.is_empty() {
        // Decision: the criterion answers this situation with text, not a failure, and it
        // names no exception for a file outside the program, so a name that matches
        // nothing is answered the same way; elfie_outline is the tool that lists the files.
        if let Some(file) = file {
            return Ok(format!("no problems in {file}")); // @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
        }
        return Ok("no problems".to_string()); // @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
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
// @lfy def/mcp/main.lfy:find#find:find:8d5e1239e348d7ebcaa7e007e6bfc71fed3f9aa4abd903c5cc7f3160b0416367
pub fn find(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        return Ok(nothing_named(name)); // @lfy def/mcp/main.lfy:find#find:find:766647ee7418f78074eae1ef55ef38c6dba1fc8dbd6fd2892f2f3fbef67b878c
    }
    // @lfy def/mcp/main.lfy:find#find:find:8d5e1239e348d7ebcaa7e007e6bfc71fed3f9aa4abd903c5cc7f3160b0416367
    let lines: Vec<String> = entities
        .iter()
        .map(|&entity| {
            let mut line = format!("{} {} {}", kind_of(model, entity), identifier_of(model, entity), place_of(model, entity));
            // The line ends with the definition, after the kind, the identifier, the file,
            // and the line.
            // @lfy def/mcp/main.lfy:find#find:find:eb271c552e5c82df2d2018db45719c602a20801b3e7e3e1f948bf7c9f0108b1d
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
// @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
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
        // file path.
        // @lfy def/mcp/main.lfy:entity#entity:entity:16e0494c4b0257a2957394a09afe69ecd935df34256d7f9b38c9e19dfe57d357
        out.push_str(&format!("# {} {} ({})\n", hover.kind, hover.identifier, place_of(model, id)));
        // The fields of the hover, a heading each.
        // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
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
        // thing.
        // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
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
    // More than one entity: each is separated by a rule.
    // @lfy def/mcp/main.lfy:entity#entity:entity:16e0494c4b0257a2957394a09afe69ecd935df34256d7f9b38c9e19dfe57d357
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
// @lfy def/mcp/main.lfy:references#references:references:172a06e97d4c30625ada08fa53113f0fdd6c084862a2db5f64451f15c9853456
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
            let (file, line, column) = usage_place(model, usage); // @lfy def/mcp/main.lfy:references#references:references:172a06e97d4c30625ada08fa53113f0fdd6c084862a2db5f64451f15c9853456
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
// @lfy def/mcp/main.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
fn render_outline(items: &[Outline], depth: usize, out: &mut Vec<String>) {
    for item in items {
        // Decision: the line is the identifier's, so a documented declaration is listed
        // where its name is, not where its documentation begins.
        out.push(format!("{}{} {} {}", "  ".repeat(depth), item.kind, item.name, item.selection_range.start.line));
        render_outline(&item.children, depth + 1, out);
    }
}

/// The declarations of one file, nested as written.
// @lfy def/mcp/main.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
pub fn outline(session: &Session, file: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    if workspace.file(file).is_none() {
        return Err(not_in_program(workspace, file)); // @lfy def/mcp/main.lfy:outline#outline:outline:36198b9a90de08fa11399e9fe803e7d97f449dc3dea946f533665367357deba2
    }
    let items = query::outline_of(workspace, file);
    if items.is_empty() {
        return Ok(format!("no declarations in {file}"));
    }
    let mut lines = Vec::new();
    render_outline(&items, 0, &mut lines); // @lfy def/mcp/main.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
    Ok(lines.join("\n"))
}

/// The grammar of Elfie: every terminal, a blank line, then every rule as EBNF.
// @lfy def/mcp/main.lfy:grammar#grammar:grammar:51bcd7292055364b6387726881c5443879b09b36eae8e1534b8d84a84effc72f
pub fn grammar(_session: &Session) -> Result<String, String> {
    Ok(format!(
        "{}\n\n{}",
        elfie_core::grammar::terminal_document(),
        elfie_core::grammar::grammar_document()
    )) // @lfy def/mcp/main.lfy:grammar#grammar:grammar:51bcd7292055364b6387726881c5443879b09b36eae8e1534b8d84a84effc72f
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
// @lfy def/mcp/main.lfy:check#check:check:a91609157ad6d366b286b501afb596b0df357f9ed0ec91a12ef228895ea5876b
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
// @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
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

/// The ids of every violated review of `elfie-requests/global.reviews.json` under the root;
/// empty when that file is missing, cannot be read, or holds no violated review.
// @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
fn violated_requirements(workspace: &Workspace) -> Vec<String> {
    let path = workspace.root.join(REQUESTS).join(GLOBAL_REVIEWS);
    // The file is missing, so no unit is violated.
    // @lfy def/mcp/main.lfy:units#units:units:11ee5b3a10c04cc7cd60c4df5fce38fd3a0ac32e7f429bb090254ed85ffffaad
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let Some(reviews) = value.as_array() else {
        return Vec::new();
    };
    reviews
        .iter()
        .filter_map(Review::from_json)
        .filter(|review| review.status == ReviewStatus::Violated) // @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
        .map(|review| review.id)
        .collect()
}

/// The stems of every unit whose outputs hold a marker answering for one of those
/// requirement ids, each once, in plan order.
// @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
fn violated_stems(plan: &Plan, violated: &[String]) -> Vec<String> {
    let mut stems: Vec<String> = Vec::new();
    for unit in &plan.units {
        // @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
        let answered = unit.outputs.iter().flat_map(|map| map.markers.iter()).any(|marker| {
            marker
                .requirement
                .as_ref()
                .is_some_and(|id| violated.iter().any(|found| found == id))
        });
        if answered && !stems.contains(&unit.stem) {
            stems.push(unit.stem.clone());
        }
    }
    stems
}

/// The plan of the workspace with [`generation::source_maps_of`] the workspace, and as the
/// violated units the stems of every unit whose outputs answer for a violated review of
/// `elfie-requests/global.reviews.json`.
// Decision: a unit's outputs are read off the plan, and the violated units are an input of
// planning, so the plan is made once with no violated unit and again only when a violated
// review names a unit; a project with none, which is every project until a global review
// finds one, is planned once.
// @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
fn plan_of(workspace: &Workspace) -> Planned {
    let maps = generation::source_maps_of(workspace, None); // @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
    let (program, plan) = generation::plan(workspace.clone(), &maps, &[], &[]);
    let violated = violated_requirements(workspace);
    let stems = violated_stems(&plan, &violated);
    if stems.is_empty() {
        return Planned { program, plan, maps };
    }
    // @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
    let (program, plan) = generation::plan(workspace.clone(), &maps, &[], &stems);
    Planned { program, plan, maps }
}

/// One unit as a line: the target, the stem, the reason or up to date, and the stems of
/// its dependencies.
// @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
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
// @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
fn batch_line(plan: &Plan, batch: &Batch) -> String {
    let stems: Vec<&str> = batch.units.iter().map(|&unit| plan.units[unit].stem.as_str()).collect();
    format!("batch {} [{}]", batch.identifier, stems.join(", "))
}

/// The units the compiler would produce, in order, why each needs generating, and how
/// they are batched.
// @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
pub fn units(session: &Session, target: Option<&str>) -> Result<String, String> {
    check_target(&session.workspace, target)?;
    let planned = plan_of(&session.workspace);
    let (workspace, plan) = (planned.workspace(), &planned.plan);
    // @lfy def/mcp/main.lfy:units#units:units:6de63799a79eb682b5f24d0c648928d19a69303edca5551e435dd0f0f0192c46
    let mut lines: Vec<String> = (0..plan.units.len())
        .filter(|&index| target.is_none_or(|target| workspace.targets[plan.units[index].target].identifier == target))
        .map(|index| unit_line(workspace, plan, index))
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
    // @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
    let batches: Vec<String> = plan
        .batches
        .iter()
        .filter(|batch| target.is_none_or(|target| batch_target(workspace, plan, batch) == target))
        .map(|batch| batch_line(plan, batch))
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
// @lfy def/mcp/main.lfy:request#request:request:b7098e0823fc0087e091a1ff50eb48b08392f7c3ef6b4b7d24be10e7b1723d58
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
// @lfy def/mcp/main.lfy:request#request:request:4dee59f1344d1563846d889e2ed940f684d08de14d3da6460c92f8859ad610de
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
    generation::request(program, plan, batch, &existing, &BTreeMap::new())
}

/// The request of the batch that holds a unit.
// Decision: a unit that is up to date is in no batch, because only planned units are
// batched; it is requested as a batch of its own, which is the request its batch would
// be, so a check of an up-to-date unit answers rather than failing.
// @lfy def/mcp/main.lfy:check#check:check:a91609157ad6d366b286b501afb596b0df357f9ed0ec91a12ef228895ea5876b
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

/// The full request for one batch: what to produce, where, every criterion to satisfy,
/// and how to report.
// @lfy def/mcp/main.lfy:request#request:request:4dee59f1344d1563846d889e2ed940f684d08de14d3da6460c92f8859ad610de
pub fn request(session: &Session, batch: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(&session.workspace);
    let plan = &planned.plan;
    let index = resolve_batch(planned.workspace(), plan, batch, target)?; // @lfy def/mcp/main.lfy:request#request:request:b7098e0823fc0087e091a1ff50eb48b08392f7c3ef6b4b7d24be10e7b1723d58
    Ok(request_for(&planned.program, plan, &plan.batches[index]).instructions) // @lfy def/mcp/main.lfy:request#request:request:4dee59f1344d1563846d889e2ed940f684d08de14d3da6460c92f8859ad610de
}

/// Whether the outputs on disk satisfy the request for one unit, and what is wrong when
/// they do not. Nothing is written; the CLI records source maps.
// @lfy def/mcp/main.lfy:check#check:check:a91609157ad6d366b286b501afb596b0df357f9ed0ec91a12ef228895ea5876b
pub fn check(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(&session.workspace);
    let (workspace, plan) = (planned.workspace(), &planned.plan);
    let index = resolve_unit(workspace, plan, unit, target)?;
    let request = request_of_unit(&planned.program, plan, index);
    let outputs = outputs_of(workspace, plan, index); // @lfy def/mcp/main.lfy:check#check:check:a91609157ad6d366b286b501afb596b0df357f9ed0ec91a12ef228895ea5876b
    let verdict = generation::accept(&planned.program, plan, &request, index, &outputs);
    if verdict.accepted {
        // @lfy def/mcp/main.lfy:check#check:check:29159b35a1e194319ef50d55c71a4671ff9399b3085a50af8d8420439284f5e1
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
        // @lfy def/mcp/main.lfy:check#check:check:2a16ea176547468dbe87eb74de7c195dc58e16c26122fc69de5f2ff15dd469f1
        let mut text = format!("rejected {}:", plan.units[index].stem);
        for problem in &verdict.problems {
            text.push_str(&format!("\n  {problem}"));
        }
        Err(text)
    }
}

// ---- what the last acceptance recorded ---------------------------------------------

// Decision: the source maps these tools read are `generation::source_maps_of` the
// workspace, as elfie_units reads them, so what a tool answers is what the last acceptance
// recorded in elfie-compile/maps, never a guess from the text on disk.

/// The language a fenced block of an output is marked with: the output's extension, or
/// nothing when it has none.
// @lfy def/mcp/main.lfy:output#output:output:9411c7024e68bf095bee2a9eca56ffddec6c5a2a5b6622a8581b90e33bc08f8e
fn fence_of(path: &str) -> &str {
    Path::new(path).extension().and_then(|extension| extension.to_str()).unwrap_or("")
}

/// The lines of an output from `first` through `last`, counting from 1, as they are on
/// disk; the empty text when the file cannot be read.
// @lfy def/mcp/main.lfy:output#output:output:9411c7024e68bf095bee2a9eca56ffddec6c5a2a5b6622a8581b90e33bc08f8e
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
// @lfy def/mcp/main.lfy:output#output:output:9411c7024e68bf095bee2a9eca56ffddec6c5a2a5b6622a8581b90e33bc08f8e
pub fn output(session: &Session, name: &str, target: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    check_target(workspace, target)?; // @lfy def/mcp/main.lfy:output#output:output:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
    let maps = generation::source_maps_of(workspace, None); // @lfy def/mcp/main.lfy:output#output:output:9411c7024e68bf095bee2a9eca56ffddec6c5a2a5b6622a8581b90e33bc08f8e
    let regions = generation::regions_of(workspace, &maps, name, target); // @lfy def/mcp/main.lfy:output#output:output:9411c7024e68bf095bee2a9eca56ffddec6c5a2a5b6622a8581b90e33bc08f8e
    if regions.is_empty() {
        return Ok(format!("nothing is generated for {name}")); // @lfy def/mcp/main.lfy:output#output:output:5a72fd117a88b8c2701f54c7c438704c2b027927283736cd09fd3ac08959c040
    }
    // A region's output is the source map that holds it; `regions_of` keeps no path, and a
    // marker is matched by the same target and the same marker it was found by.
    // @lfy def/mcp/main.lfy:output#output:output:9411c7024e68bf095bee2a9eca56ffddec6c5a2a5b6622a8581b90e33bc08f8e
    let mut blocks = Vec::new();
    for map in &maps {
        if target.is_some_and(|target| map.target != target) {
            continue;
        }
        for marker in &map.markers {
            if !regions.contains(marker) {
                continue;
            }
            // @lfy def/mcp/main.lfy:output#output:output:9411c7024e68bf095bee2a9eca56ffddec6c5a2a5b6622a8581b90e33bc08f8e
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

/// One lowered criterion as one line: `When` and its situations, then its behaviors, then
/// `Side effects:` and its side effects, joined by `: `, as a request and a review spell it.
// @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
fn lowered_criterion_text(criterion: &LoweredCriterion) -> String {
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

/// One lowered test as one line: its input and its expectation, as spelled.
// @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
fn lowered_test_text(test: &LoweredTest) -> String {
    format!("Input `{}` gives `{}`", test.input_text.trim(), test.expect_text.trim())
}

/// Every criterion and test of a lowered node and the nodes under it, with those of the
/// criteria and tests given, as its id and the one line it is spelled as.
// @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
fn collect_requirements(
    criteria: &[LoweredCriterion],
    tests: &[LoweredTest],
    out: &mut Vec<(String, String)>,
) {
    out.extend(criteria.iter().map(|criterion| (criterion.id.clone(), lowered_criterion_text(criterion))));
    out.extend(tests.iter().map(|test| (test.id.clone(), lowered_test_text(test))));
}

/// Every criterion and test of the lowered program, each as its id and the one line it is
/// spelled as: the local ones of every file, then the global ones.
// @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
fn requirements_of(program: &Program) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for lowered in &program.files {
        let mut nodes: Vec<&LoweredNode> = vec![&lowered.root];
        while let Some(node) = nodes.pop() {
            collect_requirements(&node.criteria, &node.tests, &mut found);
            nodes.extend(node.nodes());
        }
        collect_requirements(&lowered.criteria, &lowered.tests, &mut found);
    }
    // A global criterion or test is in the program once, under no file.
    // @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
    collect_requirements(&program.criteria, &program.tests, &mut found);
    found
}

/// The text of the criterion or test one requirement id names, local or global, from the
/// lowered program; `None` when the program holds none with that id.
// @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
fn requirement_text(program: &Program, id: &str) -> Option<String> {
    requirements_of(program)
        .into_iter()
        .find(|(spelled, _)| spelled == id)
        .map(|(_, text)| text)
}

/// Where one line of generated code came from: the definition file, the entity, and its
/// line.
// @lfy def/mcp/main.lfy:source#source:source:c5a82c9201a62385f7149576d228acb460938212f09111c176ce6ec9d0eb0952
pub fn source(session: &Session, file: &str, line: usize) -> Result<String, String> {
    let workspace = &session.workspace;
    for map in generation::source_maps_of(workspace, None) {
        if map.output != file {
            continue;
        }
        // @lfy def/mcp/main.lfy:source#source:source:c5a82c9201a62385f7149576d228acb460938212f09111c176ce6ec9d0eb0952
        let Some(marker) = map.markers.iter().find(|marker| marker.covers(line)) else {
            continue;
        };
        let mut text = format!("{}:{}", marker.file, marker.line);
        // @lfy def/mcp/main.lfy:source#source:source:1aa708d506a2d81f1f21ef45f6915e3b0ff4b04731b86264aa2f80c2b7f3a7fe
        if let Some(entity) = &marker.entity {
            text.push_str(&format!(" {entity}"));
        }
        // The requirement follows on the line after, and the text of that criterion or test
        // on the line after it; a criterion reads as a sentence, so it stands on its own
        // line rather than beside an id that is a name and a hash.
        // @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
        if let Some(requirement) = &marker.requirement {
            text.push('\n');
            text.push_str(requirement);
            let program = interpret::lower(workspace.clone());
            // A stale map may name an id the program no longer holds; the id still says
            // which requirement the region answered for.
            if let Some(spelled) = requirement_text(&program, requirement) {
                text.push('\n');
                text.push_str(&spelled);
            }
        }
        return Ok(text);
    }
    // @lfy def/mcp/main.lfy:source#source:source:3bd573d9905fc8bbb2ff9b436ef7c2affcef119a2a9cf64efa242292f4bf0299
    Ok(format!("no source map covers {file}:{line}"))
}

/// The unit with a stem, of the first target in `Workspace::targets` order that has one.
// @lfy def/mcp/main.lfy:changes#changes:changes:b07425830b55d4911baeb4d1d7666ab5cd94e11355858e5ca532457c341ca59e
fn unit_of_stem(workspace: &Workspace, plan: &Plan, stem: &str) -> Result<usize, String> {
    for target in 0..workspace.targets.len() {
        if let Some(index) = (0..plan.units.len())
            .find(|&index| plan.units[index].stem == stem && plan.units[index].target == target)
        {
            return Ok(index);
        }
    }
    // @lfy def/mcp/main.lfy:changes#changes:changes:bbfd13869e5d8aa4ccdda753d25b4797e5140bfb3b5f287cf8f83310a3b51e7b
    Err(format!("no unit has the stem {stem}; elfie_units lists them"))
}

/// The source at the last accepted generation, recovered as the command line recovers it:
/// through git when the root is a repository and the unit's source is in it, taking the
/// revision of the file whose hash is the one the source map recorded.
// @lfy def/mcp/main.lfy:changes#changes:changes:2c85dda7d9400c3a4dd656027b5ac3d77e047837af44ce4da61ed085e04f3ab0
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
// @lfy def/mcp/main.lfy:changes#changes:changes:b07425830b55d4911baeb4d1d7666ab5cd94e11355858e5ca532457c341ca59e
pub fn changes(session: &Session, unit: &str) -> Result<String, String> {
    let planned = plan_of(&session.workspace);
    let workspace = planned.workspace();
    let index = unit_of_stem(workspace, &planned.plan, unit)?;
    let of_stem = &planned.plan.units[index];
    if of_stem.outputs.is_empty() {
        return Ok(format!("no previous output for {unit}; nothing has been accepted for it yet")); // @lfy def/mcp/main.lfy:changes#changes:changes:4ed7bd9431f767db26ad16ea1bb29047ae4dbf6bda2d201d0fb60b8476d97a55
    }
    let previous = previous_source(workspace, of_stem); // @lfy def/mcp/main.lfy:changes#changes:changes:2c85dda7d9400c3a4dd656027b5ac3d77e047837af44ce4da61ed085e04f3ab0
    let mut text = String::new();
    if previous.is_none() {
        // @lfy def/mcp/main.lfy:changes#changes:changes:399b91b5c08ae66e623702cfef61d4f8362614c2423d9ba2dfea5b25f1f81844
        text.push_str("the source the outputs were generated from cannot be recovered; every entity is listed as added\n");
    }
    let found = generation::changes(&planned.program, of_stem, previous.as_deref()); // @lfy def/mcp/main.lfy:changes#changes:changes:b07425830b55d4911baeb4d1d7666ab5cd94e11355858e5ca532457c341ca59e
    if found.is_empty() {
        return Ok(format!("no changes in {unit} since its outputs were accepted")); // @lfy def/mcp/main.lfy:changes#changes:changes:028ff59a434c25e62e3e9339d8956e59ddc0f86fe855548f05543607a43efc55
    }
    // @lfy def/mcp/main.lfy:changes#changes:changes:b07425830b55d4911baeb4d1d7666ab5cd94e11355858e5ca532457c341ca59e
    let lines: Vec<String> = found
        .iter()
        .map(|change| format!("{} {} {}", change.entity, change.kind.as_str(), change.detail))
        .collect();
    text.push_str(&lines.join("\n"));
    Ok(text)
}

/// The full review request for one batch: every criterion and test with the regions
/// generated for its entities, and how to report.
// @lfy def/mcp/main.lfy:review#review:review:4c064efe9a4e85573fbf5019a94db7eb2ec7300c7ec3031d702a172598036905
pub fn review(session: &Session, batch: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(&session.workspace);
    let plan = &planned.plan;
    let index = resolve_batch(planned.workspace(), plan, batch, target)?; // @lfy def/mcp/main.lfy:review#review:review:b7098e0823fc0087e091a1ff50eb48b08392f7c3ef6b4b7d24be10e7b1723d58
    // @lfy def/mcp/main.lfy:review#review:review:4c064efe9a4e85573fbf5019a94db7eb2ec7300c7ec3031d702a172598036905
    Ok(generation::review(&planned.program, plan, &plan.batches[index], &planned.maps).instructions)
}

/// The review request for every global criterion and test, once for the whole program, with
/// the regions that answer for each.
// @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:2db0d812a55af1ea0837f371b673217273f2d16946f6a0f6e8c56bf3068973d3
pub fn global_review(session: &Session) -> Result<String, String> {
    let planned = plan_of(&session.workspace);
    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:f656c023948494da8d316780c15c2c1b96e1512caafb62d843dc6295be0d89be
    if planned.program.criteria.is_empty() && planned.program.tests.is_empty() {
        return Ok("no global criterion or test exists in the program".to_string());
    }
    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:2db0d812a55af1ea0837f371b673217273f2d16946f6a0f6e8c56bf3068973d3
    Ok(generation::global_review(&planned.program, &planned.maps).instructions)
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

    // @lfy def/mcp/main.lfy:serve#serve:serve:f2a3c45393b2893543b91d17f647274b8415cfa2f642779a9780db11665d6717
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
                "elfie_globalReview",
            ]
        );
        let listed = traits::list(&tools());
        assert_eq!(listed.len(), 14);
        assert_eq!(listed[0].input_schema["required"], serde_json::json!([]));
        assert_eq!(listed[1].input_schema["required"], serde_json::json!(["name"]));
        // A line is a number, and is listed as one.
        // @lfy def/mcp/main.lfy:source#source:tool:328e4667b06904decebf22779935ab215a2d8f2eb78fc9a7b6f761d4858bf0cb
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

    // @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
    #[test]
    fn problems_says_when_there_are_none() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "const y = 1;\n");
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        assert_eq!(problems(&session, Some("def/a.lfy")).unwrap(), "no problems in def/a.lfy");
        // A file the program does not hold has no problems either, and is answered with
        // text rather than a failure.
        // @lfy def/mcp/main.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
        assert_eq!(problems(&session, Some("def/b.lfy")).unwrap(), "no problems in def/b.lfy");
    }

    // @lfy def/mcp/main.lfy:find#find:tool:e72d2748ae78f2b7e0bcb2a679ad6a90cadff62c6a64a5a0b031bff7e6884a50
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
        // @lfy def/mcp/main.lfy:outline#outline:tool:e72d2748ae78f2b7e0bcb2a679ad6a90cadff62c6a64a5a0b031bff7e6884a50
        let result = call(&session, "elfie_outline", serde_json::json!({ "file": 1 }));
        assert!(result.is_error);
        assert!(result.text.contains("file") && result.text.contains("string"), "{}", result.text);
        // The fn fails: an error carrying the failure.
        // @lfy def/mcp/main.lfy:outline#outline:tool:cd7f5860cb71efc9e8af3b53b7882f4f30d39c32782e5470c22b8cf164dcef50
        let result = call(&session, "elfie_outline", serde_json::json!({ "file": "def/none.lfy" }));
        assert!(result.is_error);
        assert!(result.text.contains("def/a.lfy"), "{}", result.text);
    }

    // @lfy def/mcp/main.lfy:serve#serve:serve:2ed948e8109fe95f42a2b62690e2684424ec56255d6b6a83239254c5ec9e6bac
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

        // @lfy def/mcp/main.lfy:serve#serve:serve:a15988b7cf93a498158fb3af594b966703670dfeb7345699b2895672b01ba527
        fixture.write("elfie.json", r#"{ "name": "renamed", "source": "src", "lib": "lib" }"#);
        fixture.write("src/c.lfy", "const c = 1;\n");
        session.refresh();
        assert_eq!(session.workspace.name, "renamed");
        assert!(session.workspace.file("src/c.lfy").is_some());
        assert!(session.workspace.file("def/a.lfy").is_none());
    }

    // @lfy def/mcp/main.lfy:find#find:find:8d5e1239e348d7ebcaa7e007e6bfc71fed3f9aa4abd903c5cc7f3160b0416367
    #[test]
    fn find_entity_and_references_describe_a_declaration() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "/// Doc\nd A: `An A` { $x: `An x` = string; }\nconst y = A;\n");
        let session = fixture.session();

        let found = find(&session, "A").unwrap();
        // The line ends with the definition. @lfy def/mcp/main.lfy:find#find:find:eb271c552e5c82df2d2018db45719c602a20801b3e7e3e1f948bf7c9f0108b1d
        assert_eq!(found, "data A def/a.lfy:2: An A");
        assert_eq!(find(&session, "def/a.lfy:A").unwrap(), found);
        let none = find(&session, "nothing").unwrap();
        assert!(none.contains("elfie_outline"), "{none}"); // @lfy def/mcp/main.lfy:find#find:find:766647ee7418f78074eae1ef55ef38c6dba1fc8dbd6fd2892f2f3fbef67b878c

        // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
        let described = entity(&session, "A").unwrap();
        assert!(described.starts_with("# data A (def/a.lfy:2)"), "{described}");
        assert!(described.contains("## Definition\nAn A"), "{described}");
        assert!(described.contains("## Documentation\nDoc"), "{described}");
        assert!(described.contains("## Members\n- x: An x (string)"), "{described}");
        assert!(described.contains("## References (1)\n- def/a.lfy:3"), "{described}");
        assert!(described.contains("## Source\n```elfie\n/// Doc\nd A: `An A` { $x: `An x` = string; }\n```"), "{described}");
        assert!(!described.contains("\n---\n"));

        // @lfy def/mcp/main.lfy:references#references:references:172a06e97d4c30625ada08fa53113f0fdd6c084862a2db5f64451f15c9853456
        let used = references(&session, "A").unwrap();
        assert_eq!(used, "def/a.lfy:3:10: const y = A;");
        assert_eq!(references(&session, "y").unwrap(), "y is never used");
    }

    // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
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
    // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
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

    // @lfy def/mcp/main.lfy:entity#entity:entity:16e0494c4b0257a2957394a09afe69ecd935df34256d7f9b38c9e19dfe57d357
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

    // @lfy def/mcp/main.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
    #[test]
    fn outline_indents_declarations_or_lists_the_files() {
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", "/// Doc\nd A { $x = string; }\nconst y = 1;\n");
        let session = fixture.session();
        assert_eq!(outline(&session, "def/a.lfy").unwrap(), "data A 2\n  member x 2\nvariable y 3");
        // @lfy def/mcp/main.lfy:outline#outline:outline:36198b9a90de08fa11399e9fe803e7d97f449dc3dea946f533665367357deba2
        let error = outline(&session, "def/zzz.lfy").unwrap_err();
        assert!(error.contains("def/zzz.lfy is not in the program") && error.contains("def/a.lfy"), "{error}");
    }

    // @lfy def/mcp/main.lfy:grammar#grammar:grammar:51bcd7292055364b6387726881c5443879b09b36eae8e1534b8d84a84effc72f
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

    // @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
    #[test]
    fn units_lists_every_unit_and_batch_or_one_targets() {
        let fixture = compiled_project();
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        let listed = "rust b fresh []\nrust a fresh [b]\n\nbatch b+1 [b, a]";
        assert_eq!(units(&session, None).unwrap(), listed);
        // @lfy def/mcp/main.lfy:units#units:units:6de63799a79eb682b5f24d0c648928d19a69303edca5551e435dd0f0f0192c46
        assert_eq!(units(&session, Some("rust")).unwrap(), listed);
        // @lfy def/mcp/main.lfy:units#units:units:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
        let error = units(&session, Some("go")).unwrap_err();
        assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
    }

    // @lfy def/mcp/main.lfy:request#request:request:4dee59f1344d1563846d889e2ed940f684d08de14d3da6460c92f8859ad610de
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
        // @lfy def/mcp/main.lfy:request#request:request:b7098e0823fc0087e091a1ff50eb48b08392f7c3ef6b4b7d24be10e7b1723d58
        let error = request(&session, "c", None).unwrap_err();
        assert!(error.contains("no batch has the identifier c"), "{error}");
        let error = request(&session, "a", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target"), "{error}");
    }

    // @lfy def/mcp/main.lfy:check#check:check:a91609157ad6d366b286b501afb596b0df357f9ed0ec91a12ef228895ea5876b
    #[test]
    fn check_reads_the_outputs_from_disk_and_writes_nothing() {
        let fixture = compiled_project();
        let session = fixture.session();
        // Rejected: an error listing the problems.
        // @lfy def/mcp/main.lfy:check#check:check:2a16ea176547468dbe87eb74de7c195dc58e16c26122fc69de5f2ff15dd469f1
        let error = check(&session, "b", None).unwrap_err();
        assert!(error.starts_with("rejected b:"), "{error}");
        fixture.write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
        // Accepted: the text says so and lists the outputs.
        // @lfy def/mcp/main.lfy:check#check:check:29159b35a1e194319ef50d55c71a4671ff9399b3085a50af8d8420439284f5e1
        let text = check(&session, "b", None).unwrap();
        assert!(text.starts_with("accepted b for rust: 1 output\n  out/b.rs"), "{text}");
        assert!(!fixture.root.join("out/source-map.json").exists());
        let error = check(&session, "a", None).unwrap_err();
        assert!(error.contains("rejected a:"), "{error}");
    }

    /// The unit with a stem accepted against the outputs on disk, with its source maps
    /// recorded in the target's output directory the way the command line records them.
    fn record_unit(fixture: &Fixture, stem: &str) {
        let session = fixture.session();
        let planned = plan_of(&session.workspace);
        let (workspace, plan) = (planned.workspace(), &planned.plan);
        let index = resolve_unit(workspace, plan, stem, None).unwrap();
        let request = request_of_unit(&planned.program, plan, index);
        let outputs = outputs_of(workspace, plan, index);
        let verdict = generation::accept(&planned.program, plan, &request, index, &outputs);
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

    // @lfy def/mcp/main.lfy:output#output:output:9411c7024e68bf095bee2a9eca56ffddec6c5a2a5b6622a8581b90e33bc08f8e
    #[test]
    fn output_gives_every_region_of_an_entity_with_its_text() {
        let fixture = accepted_project();
        let session = fixture.session();
        let text = output(&session, "B", None).unwrap();
        assert_eq!(text, "out/b.rs:1-2\n```rs\n// @lfy def/b.lfy:B\npub struct B;\n```");
        assert_eq!(output(&session, "B", Some("rust")).unwrap(), text);
        // Nothing is generated for the name.
        // @lfy def/mcp/main.lfy:output#output:output:5a72fd117a88b8c2701f54c7c438704c2b027927283736cd09fd3ac08959c040
        let none = output(&session, "A", None).unwrap();
        assert_eq!(none, "nothing is generated for A");
        // The target is not a known one.
        // @lfy def/mcp/main.lfy:output#output:output:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
        let error = output(&session, "B", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
    }

    // @lfy def/mcp/main.lfy:source#source:source:c5a82c9201a62385f7149576d228acb460938212f09111c176ce6ec9d0eb0952
    #[test]
    fn source_names_the_definition_a_line_came_from() {
        let fixture = accepted_project();
        let session = fixture.session();
        // The line ends with the entity the marker names.
        // @lfy def/mcp/main.lfy:source#source:source:1aa708d506a2d81f1f21ef45f6915e3b0ff4b04731b86264aa2f80c2b7f3a7fe
        assert_eq!(source(&session, "out/b.rs", 1).unwrap(), "def/b.lfy:1 B");
        assert_eq!(source(&session, "out/b.rs", 2).unwrap(), "def/b.lfy:1 B");
        // No source map has the file as its output.
        // @lfy def/mcp/main.lfy:source#source:source:3bd573d9905fc8bbb2ff9b436ef7c2affcef119a2a9cf64efa242292f4bf0299
        let text = source(&session, "out/a.rs", 1).unwrap();
        assert_eq!(text, "no source map covers out/a.rs:1");
        // A line past the region of every marker.
        // @lfy def/mcp/main.lfy:source#source:source:3bd573d9905fc8bbb2ff9b436ef7c2affcef119a2a9cf64efa242292f4bf0299
        assert_eq!(source(&session, "out/b.rs", 9).unwrap(), "no source map covers out/b.rs:9");
        // A number argument reaches the fn as a number.
        // @lfy def/mcp/main.lfy:source#source:tool:94844af73619c1ae0e1f4c2d4a41848ebd3a208ff53fa3e946a3d7d4514e41bf
        let result = call(&session, "elfie_source", serde_json::json!({ "file": "out/b.rs", "line": 2 }));
        assert!(!result.is_error, "{}", result.text);
        assert_eq!(result.text, "def/b.lfy:1 B");
        let result = call(&session, "elfie_source", serde_json::json!({ "file": "out/b.rs", "line": "2" }));
        assert!(result.is_error);
        assert!(result.text.contains("line") && result.text.contains("number"), "{}", result.text);
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
            .write(
                "def/b.lfy",
                "fn b(): `A b` => string { @acceptanceCriteria.add({ behavior = `It answers` }); }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written outside the output directory` });\n",
            );
        fixture
    }

    // @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
    #[test]
    fn source_names_the_requirement_a_region_answers_for() {
        let fixture = project_with_requirements();
        let local = requirement_id(&fixture, "It answers");
        fixture.write(
            "out/b.rs",
            &format!("// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"),
        );
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        record_unit(&fixture, "b");
        // The marker names a requirement, so its id follows on the line after the one
        // reading the file and the line, and then the text of that criterion.
        // @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
        let session = fixture.session();
        assert_eq!(
            source(&session, "out/b.rs", 2).unwrap(),
            format!("def/b.lfy:1 b\n{local}\nIt answers")
        );
    }

    /// A region may answer for a global criterion as well as a local one; its text is read
    /// from the lowered program the same way.
    // @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
    #[test]
    fn source_reads_a_global_requirement_from_the_lowered_program() {
        let fixture = project_with_requirements();
        let global = requirement_id(&fixture, "Nothing is written outside the output directory");
        assert!(Review::is_global(&global), "{global}");
        fixture.write(
            "out/b.rs",
            &format!("// @lfy def/b.lfy:b#{global}\npub fn b() -> String {{\n    String::new()\n}}\n"),
        );
        record_unit(&fixture, "b");
        let session = fixture.session();
        // @lfy def/mcp/main.lfy:source#source:source:749a9cb8498dcaab6c4573db8f6443b82b75917aa0779cafdc233e7e11f6af58
        assert_eq!(
            source(&session, "out/b.rs", 1).unwrap(),
            format!("def/b.lfy:1 b\n{global}\nNothing is written outside the output directory")
        );
    }

    // @lfy def/mcp/main.lfy:changes#changes:changes:b07425830b55d4911baeb4d1d7666ab5cd94e11355858e5ca532457c341ca59e
    #[test]
    fn changes_reports_what_differs_since_the_outputs_were_accepted() {
        let fixture = accepted_project();
        let session = fixture.session();
        // The unit has outputs and the previous text cannot be recovered: the text says so
        // first, then every entity as added.
        // @lfy def/mcp/main.lfy:changes#changes:changes:399b91b5c08ae66e623702cfef61d4f8362614c2423d9ba2dfea5b25f1f81844
        let text = changes(&session, "b").unwrap();
        assert!(text.starts_with("the source the outputs were generated from cannot be recovered"), "{text}");
        assert!(text.contains("\nB added "), "{text}");
        // The outputs of a unit are empty.
        // @lfy def/mcp/main.lfy:changes#changes:changes:4ed7bd9431f767db26ad16ea1bb29047ae4dbf6bda2d201d0fb60b8476d97a55
        let text = changes(&session, "a").unwrap();
        assert!(text.starts_with("no previous output for a"), "{text}");
        // No unit has that stem.
        // @lfy def/mcp/main.lfy:changes#changes:changes:bbfd13869e5d8aa4ccdda753d25b4797e5140bfb3b5f287cf8f83310a3b51e7b
        let error = changes(&session, "zzz").unwrap_err();
        assert!(error.contains("no unit has the stem zzz"), "{error}");
    }

    // @lfy def/mcp/main.lfy:changes#changes:changes:028ff59a434c25e62e3e9339d8956e59ddc0f86fe855548f05543607a43efc55
    #[test]
    fn changes_says_so_when_nothing_differs() {
        let fixture = accepted_project();
        let session = fixture.session();
        let planned = plan_of(&session.workspace);
        let index = unit_of_stem(planned.workspace(), &planned.plan, "b").unwrap();
        // The file as it is now is the file the outputs were generated from, so nothing
        // differs.
        // @lfy def/mcp/main.lfy:changes#changes:changes:028ff59a434c25e62e3e9339d8956e59ddc0f86fe855548f05543607a43efc55
        let text = fs::read_to_string(fixture.root.join("def/b.lfy")).unwrap();
        assert!(generation::changes(&planned.program, &planned.plan.units[index], Some(&text)).is_empty());
    }

    // @lfy def/mcp/main.lfy:review#review:review:4c064efe9a4e85573fbf5019a94db7eb2ec7300c7ec3031d702a172598036905
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
        // ambiguous.
        // @lfy def/mcp/main.lfy:review#review:review:b7098e0823fc0087e091a1ff50eb48b08392f7c3ef6b4b7d24be10e7b1723d58
        let error = review(&session, "c", None).unwrap_err();
        assert!(error.contains("no batch has the identifier c"), "{error}");
        let error = review(&session, "a", Some("go")).unwrap_err();
        assert!(error.contains("go is not a target"), "{error}");
    }

    /// A unit whose output answers for a violated review of
    /// `elfie-requests/global.reviews.json` is planned with the reason `violated`; without
    /// that file the plan is made with no violated unit at all.
    // @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
    #[test]
    fn units_plans_a_violated_unit_again() {
        let fixture = project_with_requirements();
        let global = requirement_id(&fixture, "Nothing is written outside the output directory");
        fixture.write(
            "out/b.rs",
            &format!("// @lfy def/b.lfy:b#{global}\npub fn b() -> String {{\n    String::new()\n}}\n"),
        );
        record_unit(&fixture, "b");

        // The file is missing, so the plan is made with no violated unit and the recorded
        // unit is up to date.
        // @lfy def/mcp/main.lfy:units#units:units:11ee5b3a10c04cc7cd60c4df5fce38fd3a0ac32e7f429bb090254ed85ffffaad
        let session = fixture.session();
        assert_eq!(units(&session, None).unwrap(), "rust b up to date []");

        // A review that is satisfied names no violated unit either.
        let review_line = |status: &str| {
            format!(
                "[{}]\n",
                Review {
                    id: global.clone(),
                    file: String::new(),
                    line: 0,
                    entity: String::new(),
                    status: ReviewStatus::from_name(status).unwrap(),
                    evidence: "out/b.rs:1-4".to_string(),
                    note: "it is under out".to_string(),
                }
                .to_json()
            )
        };
        fixture.write("elfie-requests/global.reviews.json", &review_line("satisfied"));
        let session = fixture.session();
        assert_eq!(units(&session, None).unwrap(), "rust b up to date []");

        // The review is violated and a marker of the unit's output answers for its id, so
        // the unit is planned again.
        // @lfy def/mcp/main.lfy:units#units:units:d82b2211a430e252662742c83354a01320e4defdf9462b768ecab9985cd737aa
        fixture.write("elfie-requests/global.reviews.json", &review_line("violated"));
        let session = fixture.session();
        assert_eq!(units(&session, None).unwrap(), "rust b violated []\n\nbatch b [b]");
    }

    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:2db0d812a55af1ea0837f371b673217273f2d16946f6a0f6e8c56bf3068973d3
    #[test]
    fn global_review_gives_the_review_request_of_every_global_requirement() {
        let fixture = project_with_requirements();
        let session = fixture.session();
        assert_eq!(problems(&session, None).unwrap(), "no problems");
        let instructions = global_review(&session).unwrap();
        assert!(instructions.contains("global review"), "{instructions}");
        assert!(instructions.contains("Nothing is written outside the output directory"), "{instructions}");

        // No global criterion or test exists.
        // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:f656c023948494da8d316780c15c2c1b96e1512caafb62d843dc6295be0d89be
        let bare = compiled_project();
        let session = bare.session();
        assert_eq!(global_review(&session).unwrap(), "no global criterion or test exists in the program");
    }
}
