// @lfy def/mcp/main.lfy:serve
//! Serving agents over standard input and output: which tools a session is offered, how a
//! call reaches the fn it names, and the server that answers.

use super::*;

/// What a fn of this module carries besides `tool`: `compilerTool`, which acts on the batch
/// and the request of a compiler run, `commandTool`, which runs the target's command for one
/// operation, or neither.
// @lfy def/mcp/main.lfy:serve#serve:serve:fae469e7dbfdef868655e96269f387d1ceb2373d31a063ce8c099b6960d6a06a
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carried {
    /// It carries `tool` alone.
    None,
    /// It carries `compilerTool`.
    Compiler,
    /// It carries `commandTool` with this operation as its argument.
    Command(Operation),
}

/// What the fn a tool name belongs to carries besides `tool`.
// @lfy def/mcp/main.lfy:serve#serve:serve:fae469e7dbfdef868655e96269f387d1ceb2373d31a063ce8c099b6960d6a06a
// @lfy def/mcp/main.lfy:host#host:host:dda4c7d26ad4016b54849285dc2862e998ac0d97cd234f0b56608729b03fdbb1
// @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
fn carried(name: &str) -> Carried {
    match name {
        "elfie_task" | "elfie_dependency" | "elfie_mark" | "elfie_submit" => Carried::Compiler,
        "elfie_build" => Carried::Command(Operation::Build),
        "elfie_test" => Carried::Command(Operation::Test),
        "elfie_lint" => Carried::Command(Operation::Lint),
        "elfie_format" => Carried::Command(Operation::Format),
        _ => Carried::None,
    }
}

/// The names of the tools a session is offered.
///
/// A session over standard input and output is offered every fn of this module that carries
/// `tool` alone but `report` and `reportMany`, which submit the verdicts on the criteria of
/// a review it is not running: it has no binding, so no run's directory to run a command in
/// and no batch whose request to read. A compiler's hosted session is offered those, those
/// two apart, together with the fns that carry `compilerTool`; a verifier's is offered both
/// of them and every fn carrying `tool` alone but `check`, which answers whether a unit's
/// outputs would be accepted. Both are offered a fn carrying `commandTool` when a target of
/// the session has a command for its operation.
// @lfy def/mcp/main.lfy:serve#serve:serve:fae469e7dbfdef868655e96269f387d1ceb2373d31a063ce8c099b6960d6a06a
// @lfy def/mcp/main.lfy:host#host:host:dda4c7d26ad4016b54849285dc2862e998ac0d97cd234f0b56608729b03fdbb1
// @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
pub(crate) fn offered(tools: &[Registered], workspace: &Workspace, binding: Option<&Binding>) -> Vec<String> {
    let targets = session_targets(workspace, binding);
    let withheld: &[&str] = match binding.map(|binding| binding.role) {
        Some(Role::Verifier) => &["elfie_check"],
        Some(Role::Compiler) | None => &["elfie_report", "elfie_reportMany"],
    };
    tools
        .iter()
        .map(|registered| registered.tool.name.clone())
        .filter(|name| !withheld.contains(&name.as_str()))
        .filter(|name| match carried(name) {
            Carried::None => true,
            // @lfy def/mcp/main.lfy:serve#serve:serve:fae469e7dbfdef868655e96269f387d1ceb2373d31a063ce8c099b6960d6a06a
            // @lfy def/mcp/main.lfy:host#host:host:7afa5703849ae259b65ae3bd7d32057445477122e6db4ac372c99154d461ea95
            Carried::Compiler => binding.is_some_and(|binding| binding.role == Role::Compiler),
            // @lfy def/mcp/main.lfy:host#host:host:dda4c7d26ad4016b54849285dc2862e998ac0d97cd234f0b56608729b03fdbb1
            Carried::Command(operation) => {
                binding.is_some() && targets.iter().any(|target| line_for(target, operation).is_some())
            }
        })
        .collect()
}

/// Whether the tool of this name changes a file: `elfie_mark`, which writes a marker line
/// into one of a compiler run's outputs, and a command tool, which runs the target's own
/// command and so writes whatever that command always writes.
// @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
fn changes_files(name: &str) -> bool {
    name == "elfie_mark" || matches!(carried(name), Carried::Command(_))
}

/// The instructions the initialize answer carries, named for the one session it is for: the
/// tools that session is offered and nothing else, which of them change files, and, when it
/// is bound to a run, which role it is bound to.
///
/// A session over standard input and output is offered no command tool and no compiler
/// tool, so none of its tools writes a file, and the line says so outright.
// @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
// @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
fn instructions(offered: &[String], binding: Option<&Binding>) -> String {
    let mut text = format!("The Elfie agent server. Its tools in this session are {}.", offered.join(", "));
    let Some(binding) = binding else {
        // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
        text.push_str(" None of them writes a file.");
        return text;
    };
    // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
    let writing: Vec<&str> = offered.iter().map(String::as_str).filter(|name| changes_files(name)).collect();
    let _ = write!(
        text,
        " This session is bound to a {} run.",
        match binding.role {
            Role::Compiler => "compiler",
            Role::Verifier => "verifier",
        }
    );
    // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
    if writing.is_empty() {
        text.push_str(" None of them changes a file.");
        return text;
    }
    let _ = write!(
        text,
        " Of them, {} change files: elfie_mark writes a marker line into one of this run's outputs, \
         and a command tool runs the target's own command, which writes what that command always \
         writes. Every other tool here changes nothing.",
        writing.join(", ")
    );
    text
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
pub(crate) fn call_tool(registered: &Registered, session: &Session, arguments: &JsonObject) -> ToolResult {
    if let Err(message) = traits::check_arguments(&registered.tool, arguments) {
        return ToolResult::error(message);
    }
    let mut spelled = arguments.clone();
    let mut tool = registered.tool.clone();
    for argument in &mut tool.arguments {
        if argument.ty != "number" && argument.ty != "boolean" {
            continue;
        }
        if let Some(value) = spelled
            .get(&argument.name)
            .filter(|value| value.is_number() || value.is_boolean())
            .cloned()
        {
            spelled.insert(argument.name.clone(), Value::String(value.to_string()));
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

/// One session served, with every tool and the names of those the session is offered.
///
/// The session is behind a lock, so a call holds it until it answers and the next waits:
/// the calls of one session are handled one at a time, in the order received. Each served
/// session has a lock of its own, so a call of one never waits for a call of another — save
/// for two calls of command tools for one directory, which share the queue that runs their
/// commands one after the other.
// @lfy def/mcp/main.lfy:serve#serve:serve:b40b7d13aaacdbb22af8b5a419d2c34ed6ffe19d0e12598b4aa56f9dee24fe22
// @lfy def/mcp/main.lfy:host#host:host:ccd2ef2fd65b6d18cbe3bd391b33964306338675eb2302322f14cf7991f56750
// @lfy def/mcp/main.lfy:host#host:host:b09cfad6442aec8ac4faf8e13385b2973136e4d0ac08ec20419f15750ea59366
pub(crate) struct Served {
    /// The session, behind a lock.
    pub(crate) session: Mutex<Session>,
    /// Every fn of this module that carries `tool`.
    tools: Vec<Registered>,
    /// The names of the tools the session is offered, as [`offered`] gives them.
    pub(crate) offered: Vec<String>,
    /// The instructions this session's initialize answer carries, as [`instructions`]
    /// spells them for the tools it is offered and the role it is bound to.
    // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
    // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
    pub(crate) instructions: String,
    /// Whether every call re-reads the files of the program before the tool runs: a session
    /// over standard input and output does, a hosted one never does.
    reread: bool,
}

impl Served {
    // Decision: the definition loads the workspace on initialize; it is loaded when the
    // server is built, just before the initialize exchange, which the agent cannot tell
    // apart and which keeps the protocol library's handshake untouched.
    // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
    pub(crate) fn over_stdio(root: &Path) -> Served {
        let tools = tools();
        let session = Session::load(root);
        let offered = offered(&tools, &session.workspace, None);
        // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
        let instructions = instructions(&offered, None);
        Served {
            session: Mutex::new(session),
            tools,
            offered,
            instructions,
            // @lfy def/mcp/main.lfy:serve#serve:serve:995535b9c7a11554f96d88793004bc092de31525b26cd0ca15ce449e17d8e386
            reread: true,
        }
    }

    /// Whether a tool of this name is offered to the session.
    pub(crate) fn offers(&self, name: &str) -> bool {
        self.offered.iter().any(|offered| offered == name)
    }

    /// The tools the session is offered, as the protocol lists them. The result may be
    /// treated as fresh for no time at all and cached by this client only: what every tool
    /// answers depends on the files and the outputs as they are when the call arrives.
    // @lfy def/mcp/main.lfy:problems#problems:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:find#find:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:entity#entity:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:references#references:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:outline#outline:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:grammar#grammar:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:units#units:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:request#request:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:check#check:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:output#output:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:source#source:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:changes#changes:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:review#review:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:globalReview#globalReview:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:report#report:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:task#task:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:dependency#dependency:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:mark#mark:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:submit#submit:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:search#search:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:build#build:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:test#test:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:lint#lint:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:format#format:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    // @lfy def/mcp/main.lfy:reportMany#reportMany:tool:70270cbd25e8b7a53f43c0a444244a5587dec21cbcd6564c27f745fcf36d40e4
    pub(crate) fn list(&self) -> ListToolsResult {
        let offered: Vec<Registered> = self
            .tools
            .iter()
            .filter(|registered| self.offers(&registered.tool.name))
            .cloned()
            .collect();
        ListToolsResult::with_all_items(traits::list(&offered))
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private)
    }

    /// One call: the session is re-read when it is one that re-reads, then the tool runs. A
    /// call naming a tool the session is not offered is answered with an error saying so,
    /// and the fn is not reached.
    // @lfy def/mcp/main.lfy:serve#serve:serve:b40b7d13aaacdbb22af8b5a419d2c34ed6ffe19d0e12598b4aa56f9dee24fe22
    pub(crate) fn call(&self, request: &CallToolRequestParams) -> Result<CallToolResult, ErrorData> {
        let name: &str = &request.name;
        let Some(registered) = traits::find(&self.tools, name) else {
            return Err(ErrorData::invalid_params(format!("no tool named {name}"), None));
        };
        let result = if self.offers(name) {
            let mut session = self.session.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if self.reread {
                // @lfy def/mcp/main.lfy:serve#serve:serve:995535b9c7a11554f96d88793004bc092de31525b26cd0ca15ce449e17d8e386
                session.refresh();
            }
            let empty = JsonObject::new();
            let arguments = request.arguments.as_ref().unwrap_or(&empty);
            call_tool(registered, &session, arguments)
        } else {
            ToolResult::error(format!("{name} is not offered to this session"))
        };
        let content = vec![ContentBlock::text(result.text)];
        Ok(if result.is_error { CallToolResult::error(content) } else { CallToolResult::success(content) })
    }
}

/// The protocol server: one served session, over standard input and output or over HTTP.
#[derive(Clone)]
struct Server {
    served: Arc<Served>,
}

impl Server {
    /// A server over standard input and output, for the project at `root`.
    // @lfy def/mcp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b:2
    fn new(root: &Path) -> Server {
        Server {
            served: Arc::new(Served::over_stdio(root)),
        }
    }
}

impl ServerHandler for Server {
    /// Tools only: no resources and no prompts.
    // @lfy def/mcp/main.lfy:serve#serve:serve:4d105e14d91381963764973d69e9e54abacde103ac17990250a98dadf1ced8cb
    // @lfy def/mcp/main.lfy:host#host:host:4d105e14d91381963764973d69e9e54abacde103ac17990250a98dadf1ced8cb
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18) // @lfy def/mcp/main.lfy:Protocol
            .with_server_info(Implementation::new("elfie", env!("CARGO_PKG_VERSION")))
            // @lfy def/mcp/main.lfy:serve#serve:serve:3cd568fc43a27e041f94444e0bd774ad591038e833bc93d99d8667e3342f190f
            // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
            .with_instructions(self.served.instructions.clone())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(self.served.list())
    }

    // @lfy def/mcp/main.lfy:serve#serve:serve:7a38d3ecf5e074e43e215152a74b94923a889a14c60638b479fd67135299e336
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.served.call(&request).map(CallToolResponse::from)
    }
}

/// Serve agents over standard input and output until the connection closes.
///
/// The protocol library owns standard output and writes only protocol messages there;
/// nothing here prints to it. Returns 0 when standard input closes.
// @lfy def/mcp/main.lfy:serve#serve:serve:7a38d3ecf5e074e43e215152a74b94923a889a14c60638b479fd67135299e336
// @lfy def/mcp/main.lfy:serve#serve:serve:cdd8cbb19e8838a376ee63434bb80d8dd21a483810121a6bced851760822594c:2
// Nothing here opens a file for writing, and no tool it serves does either: the agent
// writes the outputs itself and asks the server to check them.
// @lfy def/mcp/main.lfy:serve#serve:serve:41ad46670c5145672ee63c9f4ba6f892fd59cb442ca93c795e977b22fd589cef
pub fn serve(root: Option<&Path>) -> i32 {
    // @lfy def/mcp/main.lfy:serve#serve:serve:5d8b7ced1bda14a6a824d8f764b914631ac3a471c462a38e1399cfe3eb88e760
    let root = match root {
        Some(root) => root.to_path_buf(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        // Every line the server logs goes to standard error, which leaves standard output
        // to the protocol alone.
        // @lfy def/mcp/main.lfy:serve#serve:serve:2e8afa5f7b4b656715c118a89eb0aa4e31bf4d6d7c23f5f07c261d830eba22d2:2
        Err(error) => {
            eprintln!("elfie mcp: {error}");
            return FAILURE;
        }
    };
    let (input, output) = rmcp::transport::stdio();
    runtime.block_on(serve_on(input, output, root))
}

/// [`serve`] over any pair of streams, so a test can drive the whole server in memory.
// @lfy def/mcp/main.lfy:serve#serve:serve:7a38d3ecf5e074e43e215152a74b94923a889a14c60638b479fd67135299e336
// @lfy def/mcp/main.lfy:serve#serve:serve:cdd8cbb19e8838a376ee63434bb80d8dd21a483810121a6bced851760822594c:2
pub(crate) async fn serve_on<I, O>(input: I, output: O, root: PathBuf) -> i32
where
    I: AsyncRead + Send + Unpin + 'static,
    O: AsyncWrite + Send + Unpin + 'static,
{
    let server = Server::new(&root);
    // @lfy def/mcp/main.lfy:serve#serve:serve:7a38d3ecf5e074e43e215152a74b94923a889a14c60638b479fd67135299e336
    let running = match server.serve((input, output)).await {
        Ok(running) => running,
        // Decision: standard input closing before the agent initializes is the
        // connection closing, so it exits with 0 like any other close.
        // @lfy def/mcp/main.lfy:serve#serve:serve:e36ddce87201b751562fbe8a68504f70e09879c00d4b93be51b40d6176adae2e
        Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return 0,
        Err(error) => {
            eprintln!("elfie mcp: {error}");
            return FAILURE;
        }
    };
    // @lfy def/mcp/main.lfy:serve#serve:serve:e36ddce87201b751562fbe8a68504f70e09879c00d4b93be51b40d6176adae2e
    match running.waiting().await {
        Ok(_) => 0,
        Err(error) => {
            eprintln!("elfie mcp: {error}");
            FAILURE
        }
    }
}

// Decision: a compile hosts the agent server itself, so every agent it runs reads the plan
// the compile made, not one a server of its own re-made from files that may have changed
// since, and its verdicts reach the CLI through the server rather than through text it
// prints. The server still never writes a file: what `elfie_report` and `elfie_reportMany`
// record stays in memory until the CLI closes the session.

/// The one address a host listens on: the loopback address, never another interface, so no
/// program on another machine can reach a tool.
// @lfy def/mcp/main.lfy:host#host:host:ea1cfd81c073494e7f7377472fc3070be780e4455e629f59e365d21d658269d4
pub(crate) const LOOPBACK: &str = "127.0.0.1";

/// What the path of a hosted session's URL begins with, under the host's own URL.
// @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
const SESSIONS: &str = "sessions";

/// The operating system's secure random source.
// Decision: `/dev/urandom` is the secure random source of the one platform the target
// lists, posix; it is a device the kernel answers from, not configuration, so reading it is
// not configuration read from a fixed absolute path.
// @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
const RANDOM: &str = "/dev/urandom";

/// How many bytes a token holds: 32, which is 64 hex digits.
// @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
pub(crate) const TOKEN_BYTES: usize = 32;

/// A token of its own for one host: 64 lowercase hex digits from the operating system's
/// secure random source, read afresh for each call of [`host`].
// @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
fn fresh_token() -> Result<String, String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    fs::File::open(RANDOM)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes))
        .map_err(|error| format!("the secure random source {RANDOM} cannot be read: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// What one host holds: where it listens, what it demands of a request, and the sessions
/// still open.
// @lfy def/mcp/main.lfy:host
pub(crate) struct Hosting {
    /// Where it listens, as [`Host::url`] spells it.
    pub(crate) url: String,
    /// What every request must carry as its bearer token.
    pub(crate) token: String,
    /// The sessions still open, by the path of their URL.
    pub(crate) sessions: Mutex<BTreeMap<String, Arc<Served>>>,
    /// How many sessions the host has opened, so that no two are given one number.
    // @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
    pub(crate) opened: AtomicUsize,
    /// Notified when the host is to stop listening.
    pub(crate) stopping: tokio::sync::Notify,
}

impl Hosting {
    /// The session open at a path, when one is.
    // @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
    pub(crate) fn session(&self, path: &str) -> Option<Arc<Served>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(path)
            .cloned()
    }

    /// One hosted session bound to a binding, and its URL: the host's URL, `sessions`, a
    /// slash, and a number no earlier session of the host was given.
    ///
    /// Its workspace is the one the binding's program was lowered from, read at the
    /// binding's directory, so every output, source map, and map file a call reads is read
    /// under that directory; nothing of the program is read from disk again.
    // @lfy def/mcp/main.lfy:host#host:host:465078bd14c4c2971c8ae047a59db580f64b30e5fa762af2e656c4accbede7ba
    // @lfy def/mcp/main.lfy:host#host:host:238a67527a5f81bdc05abb6515eacc7b86b9ead269c56d3b3654d34d47184b50
    // @lfy def/mcp/main.lfy:host#host:host:b01f7a894faf3752cba01a91aaed7228df1bb1b93d87ea2c648f034427bcc3fd
    // @lfy def/mcp/main.lfy:host#host:host:bb7c6b5258b2c4026015a8f9915717da1ec50aa40dcc10a43e5b50bc9e39310e
    // @lfy def/mcp/main.lfy:host#host:host:232c0725ae5219330822ed9664cc87be38c4591303cbc3dead664044e09b75eb
    pub(crate) fn open(&self, binding: Binding) -> String {
        let number = self.opened.fetch_add(1, Ordering::Relaxed) + 1;
        let mut workspace = binding.program.workspace.clone();
        workspace.root = binding.directory.clone();
        let tools = tools();
        let offered = offered(&tools, &workspace, Some(&binding));
        // @lfy def/mcp/main.lfy:host#host:host:eeef9359197b88bf6fc69027d5771f23a22ac670ed51c8455dd7ddc9ce90c2b6
        let instructions = instructions(&offered, Some(&binding));
        let session = Session {
            workspace,
            // A hosted session never re-reads a file of the program, so it stamps none.
            seen: BTreeMap::new(),
            binding: Some(binding),
            // @lfy def/mcp/main.lfy:host#host:host:91b44b23c29eaf7242b190da7fe5ab5f3642df5ec77212e680331af6b76e01b1
            reviews: Mutex::new(Vec::new()),
            // @lfy def/mcp/main.lfy:host#host:host:5e5526603ec0ccf1f819bfc584b556e779b3a2a8f9d0f46fdc9b92ad91bb9dc5
            submission: Mutex::new(None),
        };
        let served = Arc::new(Served {
            session: Mutex::new(session),
            tools,
            offered,
            instructions,
            reread: false,
        });
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(format!("/{SESSIONS}/{number}"), served);
        format!("{}{SESSIONS}/{number}", self.url)
    }

    /// The path of a URL under this host's own, when the URL is one of its own.
    // @lfy def/mcp/main.lfy:host#host:host:1f6860e467f9f7b17993f762e5ba4521688af80851fff707da5bbb18f86cdbba
    fn path_of(&self, url: &str) -> Option<String> {
        url.strip_prefix(&self.url).map(|path| format!("/{path}"))
    }

    /// The names of the tools the session at a URL is offered, in the order the server lists
    /// them; a URL that is no open session's is offered none.
    // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
    // @lfy def/mcp/main.lfy:host#host:host:1f6860e467f9f7b17993f762e5ba4521688af80851fff707da5bbb18f86cdbba
    pub(crate) fn offered(&self, url: &str) -> Vec<String> {
        let Some(served) = self.path_of(url).and_then(|path| self.session(&path)) else {
            // @lfy def/mcp/main.lfy:host#host:host:1f6860e467f9f7b17993f762e5ba4521688af80851fff707da5bbb18f86cdbba
            return Vec::new();
        };
        // The tools are listed in the order the server lists them, which is the order the
        // definition declares the fns that carry `tool`.
        // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
        served
            .tools
            .iter()
            .map(|registered| registered.tool.name.clone())
            .filter(|name| served.offers(name))
            .collect()
    }

    /// The session at a URL ended, and what it recorded; a URL that is no open session's
    /// ends nothing and gives a record holding no review and no submission.
    // @lfy def/mcp/main.lfy:host#host:host:64c0df55947a7e7a600bb0796c4715f740afcfda41cdd6f86b4cb408c4705713
    // @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
    pub(crate) fn close(&self, url: &str) -> SessionRecord {
        let Some(path) = self.path_of(url) else {
            // @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
            return SessionRecord::default();
        };
        let removed = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&path);
        let Some(served) = removed else {
            // @lfy def/mcp/main.lfy:host#host:host:8eae0f57be7e371560c08488ce9a3c25a5eb3254d63aa6f4fcd5f317786e93b6
            return SessionRecord::default();
        };
        served
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .record()
    }
}

/// Every request a host answers: the bearer token first, then the session its path names,
/// then the protocol over the streamable HTTP transport of the ecosystem's library.
///
/// The transport is served statelessly, so a request acts on the hosted session its path
/// names whatever MCP session id it carries, and whether it carries one at all.
// @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
// @lfy def/mcp/main.lfy:host#host:host:b20b250482898ccb1326689317f112cb544e9941949596a5c823ce56f4d692b9
// @lfy def/mcp/main.lfy:host#host:host:f876706edb4133d6c7e2e9d4f43bf7b7cc00c92572911e227d0c789a95636e5d
// @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
async fn hosted(
    axum::extract::State(hosting): axum::extract::State<Arc<Hosting>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // A request without the header `Authorization` reading `Bearer `, then the host's
    // token, is answered with 401 and reaches no session, so no tool runs for it.
    // @lfy def/mcp/main.lfy:host#host:host:b20b250482898ccb1326689317f112cb544e9941949596a5c823ce56f4d692b9
    let carried = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if carried != Some(hosting.token.as_str()) {
        return (axum::http::StatusCode::UNAUTHORIZED, "the bearer token of the host is missing or wrong\n").into_response();
    }
    // @lfy def/mcp/main.lfy:host#host:host:f876706edb4133d6c7e2e9d4f43bf7b7cc00c92572911e227d0c789a95636e5d
    let Some(served) = hosting.session(request.uri().path()) else {
        return (axum::http::StatusCode::NOT_FOUND, "no session is open at this URL\n").into_response();
    };
    // @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
    let service = StreamableHttpService::new(
        move || Ok(Server { served: served.clone() }),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig::default()
            // @lfy def/mcp/main.lfy:host#host:host:c0519927a29b45c6d3ed0bf600d8dd929353a41c1678e7be62c950db70763d79
            .with_legacy_session_mode(false)
            .with_json_response(true),
    );
    service.handle(request).await.map(axum::body::Body::new)
}

/// Serve agents over HTTP on this machine for the length of one compile or verify, one
/// hosted session per run of the compiler or the verifier.
///
/// Nothing here opens a file for writing, and the only tool a hosted session serves that
/// does is `elfie_mark`, which writes a marker line into one of the run's own outputs, and a
/// command tool, which writes whatever the target's own command writes. Nothing is written
/// to standard output, since the protocol travels over HTTP and what the host has to say it
/// says by its answer.
// @lfy def/mcp/main.lfy:host#host:host:01fa8e1706370fdc8f52d02ad3bb1a8e87f29d8d913cbe72a9195f08470fa011
// @lfy def/mcp/main.lfy:host#host:host:0587dce8e7ae5b18edf59adfa5cbec0f98b32b34d8182cf85359c17c142f1d8d
pub fn host() -> Result<Host, String> {
    // @lfy def/mcp/main.lfy:host#host:host:246a5aacda8ffe241aedd0b00fec70b3f76a7c6220141f54731dcb8097f58852
    let token = fresh_token()?;
    // The operating system picks the port, and only the loopback address is listened on.
    // @lfy def/mcp/main.lfy:host#host:host:ea1cfd81c073494e7f7377472fc3070be780e4455e629f59e365d21d658269d4
    // @lfy def/mcp/main.lfy:host#host:host:a1747a1f253c33baae12ae425d2c5c491a7c303766db5dce456ac31d206ff9e1
    let listener = std::net::TcpListener::bind((LOOPBACK, 0))
        .map_err(|error| format!("no port of {LOOPBACK} can be listened on: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("the port of {LOOPBACK} that is listened on cannot be read: {error}"))?
        .port();
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("the port {port} of {LOOPBACK} cannot be served: {error}"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("the host cannot be run: {error}"))?;
    let hosting = Arc::new(Hosting {
        // @lfy def/mcp/main.lfy:host#host:host:c0b5fd0495fb73ed8d21e6599998b9acc5a051a0dde254bb492f14f26417fade
        url: format!("http://{LOOPBACK}:{port}/"),
        token: token.clone(),
        sessions: Mutex::new(BTreeMap::new()),
        opened: AtomicUsize::new(0),
        stopping: tokio::sync::Notify::new(),
    });
    let router = axum::Router::new().fallback(hosted).with_state(hosting.clone());
    let serving = {
        let hosting = hosting.clone();
        runtime.spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                return;
            };
            // @lfy def/mcp/main.lfy:host#host:host:d17c6dc3479336c65543a2c99389c29582860e9358d65bd4e36a3d9c355e7825
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { hosting.stopping.notified().await })
                .await;
        })
    };
    let runtime = Arc::new(runtime);
    Ok(Host {
        url: hosting.url.clone(),
        token,
        open: Box::new({
            let hosting = hosting.clone();
            move |binding| hosting.open(binding)
        }),
        // @lfy def/mcp/main.lfy:host#host:host:d3b3abd60412e83fb1835597e514740cc7d4648f94a73575cf7f076399063025
        offered: Box::new({
            let hosting = hosting.clone();
            move |url: &str| hosting.offered(url)
        }),
        close: Box::new({
            let hosting = hosting.clone();
            move |url: &str| hosting.close(url)
        }),
        // Every session still open is ended, the server is told to shut down, and the task
        // that listens is waited for: the port is no longer listened on once it returns.
        // @lfy def/mcp/main.lfy:host#host:host:e07851ea0cedfcd8e3d913fa15d72b1c389e0d5f87edfc1f52b7841decccbd1e
        stop: Box::new({
            let serving = Mutex::new(Some(serving));
            move || {
                hosting
                    .sessions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clear();
                hosting.stopping.notify_one();
                let waiting = serving.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
                if let Some(waiting) = waiting {
                    let _ = runtime.block_on(waiting);
                }
            }
        }),
    })
}
