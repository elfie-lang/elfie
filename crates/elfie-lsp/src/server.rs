//! The server itself: the entry point, the session behind the protocol, the handler of
//! every notification and request, and the publication of diagnostics.
// @lfy def/lsp/main.lfy:serve

use std::collections::{BTreeMap, HashMap};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use elfie_core::format;
use elfie_core::query::{self, SemanticToken, TokenModifier, TokenType};
use elfie_core::workspace;
use tokio::io::{AsyncRead, AsyncWrite};
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types as lsp;
use tower_lsp::lsp_types::Url;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::convert::{
    Converter, apply_change, completion_kind, document_symbol, end_of, hover_markdown,
    relative_path, symbol_information, uri_of,
};
use crate::{Document, Encoding, MANIFEST, NOTHING_DECLARED, Session};

/// The extension of a source file.
const EXTENSION: &str = ".lfy";
/// The id of the watched files registration.
const WATCH_REGISTRATION: &str = "elfie.watched-files";

/// Serve one editor over standard input and output until it shuts the server down.
///
/// Speaks JSON-RPC 2.0 with the protocol's framing over standard input and output;
/// nothing else is ever written to standard output, and logging goes to standard error.
/// Requests are answered in the order received. A request that fails inside the server
/// yields an error response and the server goes on. Exit after shutdown returns 0; exit
/// without shutdown before it returns 1.
// @lfy def/lsp/main.lfy:serve#serve:serve:98094be11487918342040661be4f07e244aca47c388ac7567c98f7fd986d0e72
// @lfy def/lsp/main.lfy:serve#serve:serve:cdd8cbb19e8838a376ee63434bb80d8dd21a483810121a6bced851760822594c
// @lfy def/lsp/main.lfy:serve#serve:serve:2e8afa5f7b4b656715c118a89eb0aa4e31bf4d6d7c23f5f07c261d830eba22d2
pub fn serve(root: Option<&Path>) -> i32 {
    let root = root.map(Path::to_path_buf);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            // @lfy def/lsp/main.lfy:serve#serve:serve:cdd8cbb19e8838a376ee63434bb80d8dd21a483810121a6bced851760822594c
            // @lfy def/lsp/main.lfy:serve#serve:serve:2e8afa5f7b4b656715c118a89eb0aa4e31bf4d6d7c23f5f07c261d830eba22d2
            eprintln!("elfie lsp: the runtime could not start: {error}");
            return 1;
        }
    };
    runtime.block_on(serve_on(tokio::io::stdin(), tokio::io::stdout(), root))
}

/// [`serve`] over any pair of streams, so tests can drive the whole server in memory.
// @lfy def/lsp/main.lfy:serve#serve:serve:98094be11487918342040661be4f07e244aca47c388ac7567c98f7fd986d0e72
// @lfy def/lsp/main.lfy:serve#serve:serve:cdd8cbb19e8838a376ee63434bb80d8dd21a483810121a6bced851760822594c
pub(crate) async fn serve_on<I, O>(input: I, output: O, root: Option<PathBuf>) -> i32
where
    I: AsyncRead + Unpin,
    O: AsyncWrite,
{
    let (service, socket) = LspService::build(|client| Backend::new(client, root)).finish();
    let shared = service.inner().shared.clone();
    // Decision: one message is served at a time, so a request is answered against the
    // documents and files as every notification before it left them, and the responses go
    // out in the order their requests arrived. This gives up `$/cancelRequest`, which no
    // feature here is slow enough to need: `tower-lsp` drops the notification and the
    // request it names is answered normally.
    // @lfy def/lsp/main.lfy:serve#serve:serve:7773981fce7710813ac48558af1654223837f348144a83f759bd110529adc40d
    // @lfy def/lsp/main.lfy:serve#serve:serve:f33fa650fd2296b8e1b899bbc1c94149ffc4905c1be5d32f40ed7b204e1bab6b
    // @lfy def/lsp/main.lfy:serve#serve:serve:c27811cfbd180c4e28720d3291fd35fc8c1a18495d1de90534e7a92078e2300a
    // @lfy def/lsp/main.lfy:serve#serve:serve:84d955371f5f011bf11dd22238bcbd23653d0c8794adb6cd27221db62c7ec47d
    // @lfy def/lsp/main.lfy:serve#serve:serve:2f2c7ecf2aefa7b4950f022eda84b70f726e7b227d3c33dc71422bc342f64b55
    Server::new(input, output, socket)
        .concurrency_level(1)
        .serve(service)
        .await;
    exit_code(shared.shut_down.load(Ordering::SeqCst))
}

/// The exit code: 0 when exit arrives after shutdown, 1 when exit arrives without shutdown
/// before it.
// Decision: the input stream ending without an exit notification is treated as exit, so
// an editor that dies leaves the same code its exit would have.
// @lfy def/lsp/main.lfy:serve#serve:serve:a3eb66fd8aa2ca379fbfd14839959e5a350c90c0887219b3365368fb35b0c94c
// @lfy def/lsp/main.lfy:serve#serve:serve:9f66abf6d379ff5502a71a40d7500855a130cd0371d2c538ef5cc2e6122aeb15
pub(crate) fn exit_code(shut_down: bool) -> i32 {
    if shut_down { 0 } else { 1 }
}

/// The language server: one editor's [`Session`] behind the protocol.
// @lfy def/lsp/main.lfy:serve
pub struct Backend {
    client: Client,
    shared: Arc<Shared>,
}

/// What the server shares with the tasks that publish diagnostics.
// Decision: the session sits behind an `Arc` so a publication can run as its own task
// after the change that caused it has been answered; a later change then drops it.
struct Shared {
    /// The root [`serve`] was given, when it was given one.
    root: Option<PathBuf>,
    /// The session, from initialize on.
    session: Mutex<Option<Session>>,
    /// Counts every change of the workspace; a publication for an older count is dropped.
    generation: AtomicU64,
    /// The diagnostics last published, by file; locked for the whole of one publication so
    /// that publications never interleave.
    published: tokio::sync::Mutex<BTreeMap<String, Vec<query::Diagnostic>>>,
    /// Whether the client registers watched files dynamically.
    dynamic_watch: AtomicBool,
    /// Whether shutdown was received.
    shut_down: AtomicBool,
}

impl Backend {
    /// A server for one client, serving the project at `root` when it is given, else the
    /// one the client names at initialize.
    pub fn new(client: Client, root: Option<PathBuf>) -> Backend {
        Backend {
            client,
            shared: Arc::new(Shared {
                root,
                session: Mutex::new(None),
                generation: AtomicU64::new(0),
                published: tokio::sync::Mutex::new(BTreeMap::new()),
                dynamic_watch: AtomicBool::new(false),
                shut_down: AtomicBool::new(false),
            }),
        }
    }

    /// The session lock, usable even after a panic inside it.
    fn session(&self) -> MutexGuard<'_, Option<Session>> {
        self.shared
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Answers a request from the session. A failure inside the query is an error response
    /// and the server goes on; before initialize there is no session and nothing is
    /// answered.
    // @lfy def/lsp/main.lfy:serve#serve:serve:b57406ccac6e090310b7b7f7e364fe74fc203a3a4c7efce6f2ea07443fdcc6e3
    // @lfy def/lsp/main.lfy:serve#serve:serve:f9aa1e1b9148be162e7cbcc4ab2175dbea6ce7907b2f1690eda80d80a689f4ff
    fn with_session<T>(&self, f: impl FnOnce(&Session) -> T) -> Result<Option<T>> {
        let guard = self.session();
        let Some(session) = guard.as_ref() else {
            return Ok(None);
        };
        match panic::catch_unwind(AssertUnwindSafe(|| f(session))) {
            Ok(value) => Ok(Some(value)),
            Err(_) => Err(Error::internal_error()),
        }
    }

    /// Answers a request on a document. A document outside the root, or not in the
    /// program, answers as if nothing were there.
    // @lfy def/lsp/main.lfy:serve#serve:serve:e4e91df66ac54707cdfaacd7f541123303348db9bceebffe2552e80ba682c886
    fn with_document<T>(
        &self,
        uri: &Url,
        f: impl FnOnce(&Session, &str, &mut Converter) -> Option<T>,
    ) -> Result<Option<T>> {
        self.with_session(|session| {
            let path = relative_path(&session.workspace.root, uri)?;
            session.workspace.file(&path)?;
            let mut converter = Converter::new(session.encoding);
            f(session, &path, &mut converter)
        })
        .map(Option::flatten)
    }

    /// Changes the session under the lock and counts the change; `None` before
    /// initialize.
    fn update<T>(&self, f: impl FnOnce(&mut Session) -> T) -> Option<(u64, T)> {
        let mut guard = self.session();
        let session = guard.as_mut()?;
        let value = f(session);
        Some((self.bump(), value))
    }

    /// Counts one change of the workspace.
    fn bump(&self) -> u64 {
        self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Publishes the diagnostics of the workspace as it is at `generation`, as a task of
    /// its own so that a later change can drop it.
    // @lfy def/lsp/main.lfy:serve
    fn publish_later(&self, generation: u64) {
        let shared = self.shared.clone();
        let client = self.client.clone();
        tokio::spawn(async move { publish(shared, client, generation).await });
    }

    /// Registers for watched file changes to every `.lfy` file and to `elfie.json` under
    /// the root.
    // Decision: `elfie.json` is watched at every depth, not only at the root, because a
    // package's manifest inside the root changes the program just as the root's does.
    // @lfy def/lsp/main.lfy:serve#serve:serve:c9d12ec7a2554d47185822774257c9f0ebca78f272521b359d6019998633926b
    fn register_watchers(&self) {
        let client = self.client.clone();
        tokio::spawn(async move {
            let options = lsp::DidChangeWatchedFilesRegistrationOptions {
                watchers: vec![watcher("**/*.lfy"), watcher("**/elfie.json")],
            };
            let registration = lsp::Registration {
                id: WATCH_REGISTRATION.to_string(),
                method: "workspace/didChangeWatchedFiles".to_string(),
                register_options: serde_json::to_value(options).ok(),
            };
            if let Err(error) = client.register_capability(vec![registration]).await {
                client
                    .log_message(
                        lsp::MessageType::WARNING,
                        format!("watched files could not be registered: {error}"),
                    )
                    .await;
            }
        });
    }

    /// A document opened or changed: the workspace becomes `change` of it with the full
    /// text the editor sent. A document outside the root, or neither under the source
    /// directory nor used by a file in the program, gets nothing and one log message says
    /// why.
    // @lfy def/lsp/main.lfy:serve#serve:serve:9b7a4f33c212b5dfe1ee27317e94e0b7de566f70c71c176ce53d5ac208ff2f22
    // @lfy def/lsp/main.lfy:serve#serve:serve:6b4c40ea0834c4fb7d6b6e15c78c5b97f7d2a5d43c031c9e328542ac9fc2e737
    // @lfy def/lsp/main.lfy:serve#serve:serve:260b156ce0f0573917d8a88611dc1a49b4739ec3dbc00a950d59185ba26ba5b5
    async fn document_changed(
        &self,
        uri: &Url,
        version: i32,
        changes: Vec<lsp::TextDocumentContentChangeEvent>,
        opened: bool,
    ) {
        let outcome = self.update(|session| {
            let Some(path) = relative_path(&session.workspace.root, uri) else {
                // @lfy def/lsp/main.lfy:serve#serve:serve:2372d03fd136ee56b409774c63c22c3dada21e5054e41ed2bc6e1727f8ae9b65
                return opened.then(|| {
                    format!(
                        "{uri} is outside the root {}; it gets nothing",
                        session.workspace.root.display()
                    )
                });
            };
            let previous = session
                .documents
                .iter()
                .position(|document| document.path == path);
            let mut text = previous
                .map(|index| session.documents[index].text.clone())
                .unwrap_or_default();
            for change in &changes {
                text = apply_change(&text, session.encoding, change);
            }
            let document = Document {
                path: path.clone(),
                text: text.clone(),
                version,
            };
            match previous {
                Some(index) => session.documents[index] = document,
                None => session.documents.push(document),
            }
            session.workspace = workspace::change(&session.workspace, &path, Some(&text));
            // @lfy def/lsp/main.lfy:serve#serve:serve:8082da87acbb214151888511ba102b8dd3bf0b4fa5f55c1244f3263f2db21f04
            (opened && session.workspace.file(&path).is_none()).then(|| {
                format!(
                    "{path} is neither under {} nor used by a file in the program; it gets nothing",
                    session.workspace.source_directory
                )
            })
        });
        if let Some((generation, message)) = outcome {
            if let Some(message) = message {
                self.client
                    .log_message(lsp::MessageType::LOG, message)
                    .await;
            }
            self.publish_later(generation);
        }
    }
}

/// Publishes diagnostics for every file whose diagnostics differ from those last
/// published, and as empty for a file that left the program; nothing having been published
/// before the first publication, that one speaks for every file of the program. A
/// publication whose generation is no longer the latest is dropped, so only the latest
/// workspace is published.
// @lfy def/lsp/main.lfy:serve#serve:serve:6991b81371beae78826a2fc55a6b814c2a8b3ad40a2e223771a6746735cdf4cd
// @lfy def/lsp/main.lfy:serve#serve:serve:f8f2931d14d2b2a4e1c3b72f0562d356d5e993d089a8899c3712bc042902d862
async fn publish(shared: Arc<Shared>, client: Client, generation: u64) {
    let mut published = shared.published.lock().await;
    // @lfy def/lsp/main.lfy:serve#serve:serve:1495429613832ca963f01ea2a9c05ebba15c1241e34609dbe8c359d5c7adc254
    if shared.generation.load(Ordering::SeqCst) != generation {
        return;
    }
    let batch = {
        let guard = shared
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(session) = guard.as_ref() else {
            return;
        };
        let current = by_file(
            &program_files(&session.workspace),
            query::diagnostics_of(&session.workspace, None),
        );
        let delta = diagnostics_delta(&published, &current);
        *published = current;
        let mut converter = Converter::new(session.encoding);
        delta
            .into_iter()
            .filter_map(|(file, diagnostics)| {
                let uri = uri_of(&session.workspace.root, &file)?;
                let version = session.document(&file).map(|document| document.version);
                let diagnostics = diagnostics
                    .iter()
                    .map(|diagnostic| converter.diagnostic(session, diagnostic))
                    .collect::<Vec<_>>();
                Some((uri, diagnostics, version))
            })
            .collect::<Vec<_>>()
    };
    for (uri, diagnostics, version) in batch {
        client.publish_diagnostics(uri, diagnostics, version).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    /// The session's workspace is `load` of the root given to `serve` when it was given
    /// one, of the client's first workspace folder when the client names one, and of the
    /// current directory otherwise. The encoding is utf-8 when the client offers it and
    /// utf-16 when it does not, and the response says which; the response advertises hover,
    /// definition, references, completion, rename with prepare, document symbols, workspace
    /// symbols, document formatting, and semantic tokens for a whole document, and full
    /// text document synchronization.
    // @lfy def/lsp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b
    async fn initialize(&self, params: lsp::InitializeParams) -> Result<lsp::InitializeResult> {
        let root = chosen_root(self.shared.root.as_deref(), &params);
        // @lfy def/lsp/main.lfy:serve#serve:serve:c82aa87a38da9f76f6a0282cf762d75fc1a996f6387757f554851aa77dc039f8
        // @lfy def/lsp/main.lfy:serve#serve:serve:5f629591078b6612e51dff25a74ae0148764e1dd9d2ab2591e7da093f299a566
        let encoding = negotiate(
            params
                .capabilities
                .general
                .as_ref()
                .and_then(|general| general.position_encodings.as_deref()),
        );
        let dynamic_watch = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.did_change_watched_files.as_ref())
            .and_then(|watched| watched.dynamic_registration)
            .unwrap_or(false);
        self.shared
            .dynamic_watch
            .store(dynamic_watch, Ordering::SeqCst);
        let workspace = workspace::load(&root);
        *self.session() = Some(Session {
            workspace,
            documents: Vec::new(),
            encoding,
        });
        Ok(lsp::InitializeResult {
            // @lfy def/lsp/main.lfy:serve#serve:serve:4dfeb9815a7448555bae335da2f77b2f7e33e4ca1b7dc7432e3b0c8c4417afd3
            // @lfy def/lsp/main.lfy:serve#serve:serve:707ab7a19774623cb46e195663db4cd66b51d81e6762e0089651d25391104748
            capabilities: capabilities(encoding),
            server_info: Some(lsp::ServerInfo {
                name: "elfie".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    /// After initialized, diagnostics are published for every file of the program, and
    /// the server registers for watched file changes.
    // @lfy def/lsp/main.lfy:serve#serve:serve:f8f2931d14d2b2a4e1c3b72f0562d356d5e993d089a8899c3712bc042902d862
    async fn initialized(&self, _: lsp::InitializedParams) {
        if self.shared.dynamic_watch.load(Ordering::SeqCst) {
            self.register_watchers();
        }
        let generation = self.bump();
        self.publish_later(generation);
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:a3eb66fd8aa2ca379fbfd14839959e5a350c90c0887219b3365368fb35b0c94c
    async fn shutdown(&self) -> Result<()> {
        self.shared.shut_down.store(true, Ordering::SeqCst);
        Ok(())
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:9b7a4f33c212b5dfe1ee27317e94e0b7de566f70c71c176ce53d5ac208ff2f22
    // @lfy def/lsp/main.lfy:serve#serve:serve:260b156ce0f0573917d8a88611dc1a49b4739ec3dbc00a950d59185ba26ba5b5
    async fn did_open(&self, params: lsp::DidOpenTextDocumentParams) {
        let document = params.text_document;
        let change = lsp::TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: document.text,
        };
        self.document_changed(&document.uri, document.version, vec![change], true)
            .await;
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:6b4c40ea0834c4fb7d6b6e15c78c5b97f7d2a5d43c031c9e328542ac9fc2e737
    // @lfy def/lsp/main.lfy:serve#serve:serve:260b156ce0f0573917d8a88611dc1a49b4739ec3dbc00a950d59185ba26ba5b5
    async fn did_change(&self, params: lsp::DidChangeTextDocumentParams) {
        self.document_changed(
            &params.text_document.uri,
            params.text_document.version,
            params.content_changes,
            false,
        )
        .await;
    }

    /// A document closed: the workspace becomes `change` of it with nothing, so the disk
    /// is read again.
    // @lfy def/lsp/main.lfy:serve#serve:serve:1800e8ae59684dee23afe3307a028fceb453dbcc3af7518f3d52a1793fa2cc8f
    // @lfy def/lsp/main.lfy:serve#serve:serve:0b97846abe098e6a1fbfe6db8abe1bcff1075c32c70e67651fff16cca55c2362
    async fn did_close(&self, params: lsp::DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let outcome = self.update(|session| {
            let Some(path) = relative_path(&session.workspace.root, &uri) else {
                return false;
            };
            session.documents.retain(|document| document.path != path);
            session.workspace = workspace::change(&session.workspace, &path, None);
            true
        });
        if let Some((generation, true)) = outcome {
            self.publish_later(generation);
        }
    }

    /// A watched `.lfy` file that is not open changed, appeared, or vanished: the
    /// workspace becomes `change` of it with nothing. `elfie.json` changed: the workspace
    /// is loaded again and every open document applied to it.
    // @lfy def/lsp/main.lfy:serve#serve:serve:7866f3607d52874f64675dcd40566e502c90ca8fbccd522413461b5fc6c884dd
    // @lfy def/lsp/main.lfy:serve#serve:serve:5f45cd42b1869414e0293381505784b60b7c53b1f7e6e745865e038029ef42e0
    async fn did_change_watched_files(&self, params: lsp::DidChangeWatchedFilesParams) {
        let outcome = self.update(|session| {
            let mut reload = false;
            let mut changed = Vec::new();
            for event in &params.changes {
                let Some(path) = relative_path(&session.workspace.root, &event.uri) else {
                    continue;
                };
                if Path::new(&path)
                    .file_name()
                    .is_some_and(|name| name == MANIFEST)
                {
                    reload = true; // @lfy def/lsp/main.lfy:serve#serve:serve:5f45cd42b1869414e0293381505784b60b7c53b1f7e6e745865e038029ef42e0
                } else if path.ends_with(EXTENSION) && session.document(&path).is_none() {
                    changed.push(path); // @lfy def/lsp/main.lfy:serve#serve:serve:7866f3607d52874f64675dcd40566e502c90ca8fbccd522413461b5fc6c884dd
                }
            }
            if reload {
                let mut reloaded = workspace::load(&session.workspace.root);
                for document in &session.documents {
                    reloaded = workspace::change(&reloaded, &document.path, Some(&document.text));
                }
                session.workspace = reloaded;
            } else {
                for path in &changed {
                    session.workspace = workspace::change(&session.workspace, path, None);
                }
            }
            reload || !changed.is_empty()
        });
        if let Some((generation, true)) = outcome {
            self.publish_later(generation);
        }
    }

    /// `hoverAt` as markdown: a code line of the kind, identifier, a colon, and the type;
    /// then the definition; then the documentation; then each criterion as a list item
    /// reading its situations then its behaviors. The code line ends with the owner in
    /// parentheses when `Hover.owner` is set, and the traits follow the definition on one
    /// line when `Hover.traits` is not empty.
    // @lfy def/lsp/main.lfy:serve#serve:serve:19386c9f8368f5aaddbde274c6c2dddb089872f7a9a314186a05cdd7a769b6e3
    async fn hover(&self, params: lsp::HoverParams) -> Result<Option<lsp::Hover>> {
        let at = params.text_document_position_params;
        self.with_document(&at.text_document.uri, |session, path, converter| {
            let position = converter.query_position(session, path, at.position);
            let hover = query::hover_at(&session.workspace, path, position)?;
            Some(lsp::Hover {
                contents: lsp::HoverContents::Markup(lsp::MarkupContent {
                    kind: lsp::MarkupKind::Markdown,
                    value: hover_markdown(&hover),
                }),
                range: Some(converter.range(session, &hover.range)),
            })
        })
    }

    /// `definitionOf` as one location.
    // @lfy def/lsp/main.lfy:serve#serve:serve:37382c32c2e5d125cef7e8f2a6960e4c8fa8bfce5a4499fcee1b8332af049767
    async fn goto_definition(
        &self,
        params: lsp::GotoDefinitionParams,
    ) -> Result<Option<lsp::GotoDefinitionResponse>> {
        let at = params.text_document_position_params;
        self.with_document(&at.text_document.uri, |session, path, converter| {
            let position = converter.query_position(session, path, at.position);
            let range = query::definition_of(&session.workspace, path, position)?;
            let location = converter.location(session, &range)?;
            Some(lsp::GotoDefinitionResponse::Scalar(location))
        })
    }

    /// `referencesTo` with the request's includeDeclaration.
    // @lfy def/lsp/main.lfy:serve#serve:serve:3998738246ab112a54cafc42192f647ffab183141e3fd8f3f1e5dc82409c7dc9
    async fn references(&self, params: lsp::ReferenceParams) -> Result<Option<Vec<lsp::Location>>> {
        let at = params.text_document_position;
        let include_declaration = params.context.include_declaration;
        self.with_document(&at.text_document.uri, |session, path, converter| {
            let position = converter.query_position(session, path, at.position);
            let ranges =
                query::references_to(&session.workspace, path, position, include_declaration);
            Some(
                ranges
                    .iter()
                    .filter_map(|range| converter.location(session, range))
                    .collect(),
            )
        })
    }

    /// `completionsAt` with each kind mapped to the protocol's kinds.
    // @lfy def/lsp/main.lfy:serve#serve:serve:d4e7595e6e9f64e2a9a1d19156e9b56857bed804e62b3119f125644cd3276cfa
    async fn completion(
        &self,
        params: lsp::CompletionParams,
    ) -> Result<Option<lsp::CompletionResponse>> {
        let at = params.text_document_position;
        self.with_document(&at.text_document.uri, |session, path, converter| {
            let position = converter.query_position(session, path, at.position);
            let completions = query::completions_at(&session.workspace, path, position);
            Some(lsp::CompletionResponse::Array(
                completions
                    .into_iter()
                    .map(|completion| lsp::CompletionItem {
                        label: completion.label,
                        kind: Some(completion_kind(completion.kind.value())),
                        detail: completion.detail,
                        ..Default::default()
                    })
                    .collect(),
            ))
        })
    }

    /// The range of the token under the position when `symbolAt` finds a symbol; an error
    /// saying nothing is declared here when `symbolAt` finds no symbol.
    // @lfy def/lsp/main.lfy:serve#serve:serve:cb81d8e3f0b6567ed41dc152db804c5f411d48090a803d5af4c3a3b6b6bac767
    // @lfy def/lsp/main.lfy:serve#serve:serve:e3461a08571790d904995f24b1ab330dd9e4038aa3e11df19c4dad085e9ddb3d
    async fn prepare_rename(
        &self,
        params: lsp::TextDocumentPositionParams,
    ) -> Result<Option<lsp::PrepareRenameResponse>> {
        let found = self.with_document(&params.text_document.uri, |session, path, converter| {
            let position = converter.query_position(session, path, params.position);
            query::symbol_at(&session.workspace, path, position)?;
            let index = query::token_at(&session.workspace, path, position)?;
            let file = session.workspace.model.file(path)?;
            let range = query::range_of(&session.workspace, file, query::NodeOrToken::Token(index));
            Some(lsp::PrepareRenameResponse::Range(
                converter.range(session, &range),
            ))
        })?;
        found
            .map(Some)
            .ok_or_else(|| Error::invalid_params(NOTHING_DECLARED))
    }

    /// `renameAt` as one workspace edit with the edits grouped by file when it returns
    /// edits; an error response carrying the reason it returned when it returns a reason.
    // @lfy def/lsp/main.lfy:serve#serve:serve:36af53505133b481b44d4927a2dd8ef3b7b9cb4c9fc338923acf2b52c83b993c
    // @lfy def/lsp/main.lfy:serve#serve:serve:353c36b50bff92902b8106ae8ad5aa2177f9be6dc03e90b2e62c937373fd1175
    async fn rename(&self, params: lsp::RenameParams) -> Result<Option<lsp::WorkspaceEdit>> {
        let at = params.text_document_position;
        let outcome = self.with_document(&at.text_document.uri, |session, path, converter| {
            let position = converter.query_position(session, path, at.position);
            let edits = query::rename_at(&session.workspace, path, position, &params.new_name);
            Some(edits.map(|edits| {
                let mut changes: HashMap<Url, Vec<lsp::TextEdit>> = HashMap::new();
                for edit in edits {
                    let Some(uri) = uri_of(&session.workspace.root, &edit.range.file) else {
                        continue;
                    };
                    changes.entry(uri).or_default().push(lsp::TextEdit {
                        range: converter.range(session, &edit.range),
                        new_text: edit.text,
                    });
                }
                lsp::WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }
            }))
        })?;
        match outcome {
            Some(Ok(edit)) => Ok(Some(edit)),
            Some(Err(reason)) => Err(Error::invalid_params(reason)),
            None => Err(Error::invalid_params(NOTHING_DECLARED)),
        }
    }

    /// `outlineOf` nested as the protocol's document symbols, with each kind mapped to the
    /// LSP 3.17 SymbolKind: data and type to Struct, trait to Interface, enum to Enum,
    /// enumMember to EnumMember, function and agentFunction to Function, member to Field,
    /// typeParameter to TypeParameter, module to Module, and variable, loopVariable,
    /// parameter, alias, and external to Variable.
    // @lfy def/lsp/main.lfy:serve#serve:serve:2d7434ceacaf117b84a50944d6defac63a931122b09e46f84d1972331095c38d
    async fn document_symbol(
        &self,
        params: lsp::DocumentSymbolParams,
    ) -> Result<Option<lsp::DocumentSymbolResponse>> {
        self.with_document(&params.text_document.uri, |session, path, converter| {
            let outline = query::outline_of(&session.workspace, path);
            Some(lsp::DocumentSymbolResponse::Nested(
                outline
                    .iter()
                    .map(|entry| document_symbol(converter, session, entry))
                    .collect(),
            ))
        })
    }

    /// `findSymbols` with the request's query.
    // Decision: each symbol's location is its whole declaration, documentation included,
    // as the protocol describes a symbol's location; the identifier alone is what document
    // symbols carry as their selection range.
    // @lfy def/lsp/main.lfy:serve#serve:serve:03938d8acea19d9cedc01076466a3b427d91282d72a892fe54dfdd76f1b12547
    async fn symbol(
        &self,
        params: lsp::WorkspaceSymbolParams,
    ) -> Result<Option<Vec<lsp::SymbolInformation>>> {
        self.with_session(|session| {
            let mut converter = Converter::new(session.encoding);
            query::find_symbols(&session.workspace, &params.query)
                .iter()
                .filter_map(|outline| {
                    let location = converter.location(session, &outline.range)?;
                    Some(symbol_information(outline, location))
                })
                .collect()
        })
    }

    /// `semanticTokensOf` encoded in the protocol's relative form.
    // @lfy def/lsp/main.lfy:serve#serve:serve:f60fe367b8c7bc62490da190f96e531eded102a1ed95a0c464d889b0599a46df
    async fn semantic_tokens_full(
        &self,
        params: lsp::SemanticTokensParams,
    ) -> Result<Option<lsp::SemanticTokensResult>> {
        self.with_document(&params.text_document.uri, |session, path, converter| {
            let tokens = query::semantic_tokens_of(&session.workspace, path);
            let protocol: Vec<lsp::Range> = tokens.iter().map(|t| converter.range(session, &t.range)).collect();
            Some(lsp::SemanticTokensResult::Tokens(lsp::SemanticTokens {
                result_id: None,
                data: encode_tokens(&tokens, &protocol),
            }))
        })
    }

    /// One edit replacing the whole document with `format` of its tree when the tree has no
    /// errors and that differs from the document; no edits when the tree has no errors and
    /// it is the same; no edits, with a log message, when the tree has errors.
    // @lfy def/lsp/main.lfy:serve#serve:serve:6b702869304d1406c4c91d62256f0e00c22dfe7bc63714e4e23aa92679e4b4b3
    // @lfy def/lsp/main.lfy:serve#serve:serve:8515860f3ec119e6aa9375f57168b7240d07e511f2eb7399afae8b3eca6b3a2a
    // @lfy def/lsp/main.lfy:serve#serve:serve:28700f9f7649b38774e5a69d7bb8341848a793dbcc8fca77e9a8f5f6a04c3a64
    // @lfy def/lsp/main.lfy:serve#serve:serve:777a57de3910fb244406238be7543474c340ffdbe2b0949fec10958ba5478dc1
    async fn formatting(
        &self,
        params: lsp::DocumentFormattingParams,
    ) -> Result<Option<Vec<lsp::TextEdit>>> {
        let outcome = self.with_document(&params.text_document.uri, |session, path, _| {
            let file = session.workspace.model.file(path)?;
            let tree = &session.workspace.model.sources[file].tree;
            if !tree.errors.is_empty() {
                return Some(Err(format!(
                    "{path} is not formatted because its tree has errors"
                )));
            }
            let current = session
                .document(path)
                .map(|document| document.text.clone())
                .unwrap_or_else(|| tree.raw(0, tree.tokens.len()));
            let formatted = format::format(tree);
            if formatted == current {
                return Some(Ok(None));
            }
            Some(Ok(Some(vec![lsp::TextEdit {
                range: lsp::Range {
                    start: lsp::Position::new(0, 0),
                    end: end_of(&current, session.encoding),
                },
                new_text: formatted,
            }])))
        })?;
        match outcome {
            Some(Ok(edits)) => Ok(edits),
            Some(Err(message)) => {
                self.client
                    .log_message(lsp::MessageType::LOG, message)
                    .await;
                Ok(None)
            }
            None => Ok(None),
        }
    }
}

/// The project the session loads: the root `serve` was given when it was given one, the
/// client's first workspace folder when the client names one, and the current directory
/// otherwise.
// @lfy def/lsp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b
pub(crate) fn chosen_root(given: Option<&Path>, params: &lsp::InitializeParams) -> PathBuf {
    let root = given
        .map(Path::to_path_buf)
        // @lfy def/lsp/main.lfy:serve#serve:serve:aa86a5495219844e1ddc4aa1bfb0955b0cfae99f274a5906be3f96398622b07b
        .or_else(|| {
            params
                .workspace_folders
                .as_ref()?
                .first()?
                .uri
                .to_file_path()
                .ok()
        })
        // @lfy def/lsp/main.lfy:serve#serve:serve:fa7d65433e4967e6668886caefd62bc4b421c62a2690c2abe9ff145233f53a4a
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    std::path::absolute(&root).unwrap_or(root)
}

/// The encoding agreed at initialization: utf-8 when the client offers it, utf-16
/// otherwise.
// @lfy def/lsp/main.lfy:serve#serve:serve:c82aa87a38da9f76f6a0282cf762d75fc1a996f6387757f554851aa77dc039f8
// @lfy def/lsp/main.lfy:serve#serve:serve:5f629591078b6612e51dff25a74ae0148764e1dd9d2ab2591e7da093f299a566
pub(crate) fn negotiate(offered: Option<&[lsp::PositionEncodingKind]>) -> Encoding {
    let offers_utf8 = offered
        .unwrap_or_default()
        .contains(&lsp::PositionEncodingKind::UTF8);
    if offers_utf8 {
        Encoding::Utf8
    } else {
        Encoding::Utf16
    }
}

/// What the server advertises.
// @lfy def/lsp/main.lfy:serve#serve:serve:707ab7a19774623cb46e195663db4cd66b51d81e6762e0089651d25391104748
fn capabilities(encoding: Encoding) -> lsp::ServerCapabilities {
    lsp::ServerCapabilities {
        // @lfy def/lsp/main.lfy:serve#serve:serve:4dfeb9815a7448555bae335da2f77b2f7e33e4ca1b7dc7432e3b0c8c4417afd3
        position_encoding: Some(encoding.protocol()),
        text_document_sync: Some(lsp::TextDocumentSyncCapability::Kind(
            lsp::TextDocumentSyncKind::FULL,
        )),
        hover_provider: Some(lsp::HoverProviderCapability::Simple(true)),
        definition_provider: Some(lsp::OneOf::Left(true)),
        references_provider: Some(lsp::OneOf::Left(true)),
        completion_provider: Some(lsp::CompletionOptions {
            // Decision: the accessors, the colon of a definition clause, and the quote and
            // slash of a `use` path trigger completion, since those are the contexts
            // `completionsAt` answers for.
            trigger_characters: Some(
                [".", "@", "$", ":", "\"", "/"]
                    .iter()
                    .map(|c| c.to_string())
                    .collect(),
            ),
            ..Default::default()
        }),
        rename_provider: Some(lsp::OneOf::Right(lsp::RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        document_symbol_provider: Some(lsp::OneOf::Left(true)),
        workspace_symbol_provider: Some(lsp::OneOf::Left(true)),
        document_formatting_provider: Some(lsp::OneOf::Left(true)),
        // @lfy def/lsp/main.lfy:serve#serve:serve:b8565649d4ab1cf2d67a413d0fe35867a70a2cb423e57de6e2104a415cb6e951
        semantic_tokens_provider: Some(
            lsp::SemanticTokensServerCapabilities::SemanticTokensOptions(lsp::SemanticTokensOptions {
                legend: legend(),
                full: Some(lsp::SemanticTokensFullOptions::Bool(true)),
                range: Some(false),
                work_done_progress_options: Default::default(),
            }),
        ),
        ..Default::default()
    }
}

/// The semantic tokens legend: the values of `TokenType` then of `TokenModifier`, each in
/// enum order, so a client maps the custom types `data` and `trait` itself.
// @lfy def/lsp/main.lfy:serve#serve:serve:b8565649d4ab1cf2d67a413d0fe35867a70a2cb423e57de6e2104a415cb6e951
pub(crate) fn legend() -> lsp::SemanticTokensLegend {
    lsp::SemanticTokensLegend {
        token_types: TokenType::ALL.iter().map(|t| lsp::SemanticTokenType::new(t.value())).collect(),
        token_modifiers: TokenModifier::ALL.iter().map(|m| lsp::SemanticTokenModifier::new(m.value())).collect(),
    }
}

/// Semantic tokens in the protocol's relative form: for each token in order, the line
/// difference from the token before, the start difference when on the same line or the
/// start itself otherwise, the length in encoding units, the legend index of its type, and
/// the modifiers as a bit set by legend index.
// @lfy def/lsp/main.lfy:serve#serve:serve:f60fe367b8c7bc62490da190f96e531eded102a1ed95a0c464d889b0599a46df
pub(crate) fn encode_tokens(
    tokens: &[SemanticToken],
    protocol: &[lsp::Range],
) -> Vec<lsp::SemanticToken> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut previous_line = 0;
    let mut previous_start = 0;
    for (token, range) in tokens.iter().zip(protocol) {
        let line = range.start.line;
        let start = range.start.character;
        let delta_line = line - previous_line;
        // The start of a token on the same line as the one before it is the difference from
        // that one's start; the start of the first token, or of one on a different line, is
        // the start itself.
        // @lfy def/lsp/main.lfy:serve#serve:serve:ce4fd9bcf886d04552fa421a2b3c1b42b5cf8a561a7c82e4b934e4741ab52f0a
        // @lfy def/lsp/main.lfy:serve#serve:serve:5c11eec6ca2131eeb40d9fe0e01d7330d8c6bb85d9ca32ac16990413db81067c
        let delta_start = if delta_line == 0 { start - previous_start } else { start };
        let length = range.end.character.saturating_sub(range.start.character);
        let modifiers = token.modifiers.iter().fold(0u32, |bits, m| bits | (1 << m.index()));
        out.push(lsp::SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: token.ty.index() as u32,
            token_modifiers_bitset: modifiers,
        });
        previous_line = line;
        previous_start = start;
    }
    out
}

/// A watcher for one glob under the root.
fn watcher(pattern: &str) -> lsp::FileSystemWatcher {
    lsp::FileSystemWatcher {
        glob_pattern: lsp::GlobPattern::String(pattern.to_string()),
        kind: None,
    }
}

/// Every file of the program that lies under the root, so that a file the editor could
/// open is one the diagnostics of the program speak for.
// Decision: a file the standard library brought from outside the project is left out; its
// path is absolute, it is no document of this root, and the criterion on a document
// outside the root gives it nothing.
// @lfy def/lsp/main.lfy:serve#serve:serve:f8f2931d14d2b2a4e1c3b72f0562d356d5e993d089a8899c3712bc042902d862
// @lfy def/lsp/main.lfy:serve#serve:serve:cf26413f4c3bbcfd96486c84365ace322d8a1199ddcd33bf8f351b0c39d0614b
fn program_files(workspace: &workspace::Workspace) -> Vec<String> {
    workspace
        .files
        .iter()
        .filter(|file| !Path::new(&file.path).is_absolute())
        .map(|file| file.path.clone())
        .collect()
}

/// Diagnostics by file, in the order the query gave them, with an empty list for every
/// file of the program that carries none, so that a clean file is spoken for too.
// @lfy def/lsp/main.lfy:serve#serve:serve:f8f2931d14d2b2a4e1c3b72f0562d356d5e993d089a8899c3712bc042902d862
pub(crate) fn by_file(
    files: &[String],
    diagnostics: Vec<query::Diagnostic>,
) -> BTreeMap<String, Vec<query::Diagnostic>> {
    let mut grouped: BTreeMap<String, Vec<query::Diagnostic>> = files
        .iter()
        .map(|file| (file.clone(), Vec::new()))
        .collect();
    for diagnostic in diagnostics {
        grouped
            .entry(diagnostic.range.file.clone())
            .or_default()
            .push(diagnostic);
    }
    grouped
}

/// What to publish: every file whose diagnostics differ from those last published, and
/// an empty list for a file that left the program.
// @lfy def/lsp/main.lfy:serve#serve:serve:6991b81371beae78826a2fc55a6b814c2a8b3ad40a2e223771a6746735cdf4cd
// @lfy def/lsp/main.lfy:serve#serve:serve:b132c3ac7e22fcbfed3082c60d6be3e5fce613162d5fbecbc92f52cc490a1c1c
pub(crate) fn diagnostics_delta(
    published: &BTreeMap<String, Vec<query::Diagnostic>>,
    current: &BTreeMap<String, Vec<query::Diagnostic>>,
) -> Vec<(String, Vec<query::Diagnostic>)> {
    let mut delta = Vec::new();
    for (file, diagnostics) in current {
        if published.get(file) != Some(diagnostics) {
            delta.push((file.clone(), diagnostics.clone()));
        }
    }
    for file in published.keys() {
        if !current.contains_key(file) {
            delta.push((file.clone(), Vec::new()));
        }
    }
    delta
}
