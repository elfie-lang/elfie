//! Compiled from `def/lsp/main.lfy`: the Elfie language server.
//!
//! [`serve`] speaks the Language Server Protocol over standard input and output for one
//! editor until it shuts the server down. Every feature is one query of
//! [`elfie_core::query`] over the [`Session`]'s workspace; the server converts document
//! URIs to workspace paths and protocol positions to token positions at its edge, and
//! nothing else. The protocol plumbing is the `tower-lsp` library, the external
//! declaration `Protocol` of the definition.
// @lfy def/lsp/main.lfy:Protocol

use std::collections::{BTreeMap, HashMap};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use elfie_core::format;
use elfie_core::model::{Criterion, SymbolKind};
use elfie_core::query::{self, Position, Range, SemanticToken, TokenModifier, TokenType};
use elfie_core::workspace;
use tokio::io::{AsyncRead, AsyncWrite};
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types as lsp;
use tower_lsp::lsp_types::Url;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod data;

pub use data::*;

/// The project layout file, whose change reloads the whole workspace.
const MANIFEST: &str = "elfie.json";
/// The extension of a source file.
const EXTENSION: &str = ".lfy";
/// The id of the watched files registration.
const WATCH_REGISTRATION: &str = "elfie.watched-files";
/// The message of a rename request on nothing.
const NOTHING_DECLARED: &str = "nothing is declared here";

/// Serve one editor over standard input and output until it shuts the server down.
///
/// Speaks JSON-RPC 2.0 with the protocol's framing over standard input and output;
/// nothing else is ever written to standard output, and logging goes to standard error.
/// Requests are answered in the order received. A request that fails inside the server
/// yields an error response and the server goes on. Exit after shutdown returns 0; exit
/// without shutdown before it returns 1.
// @lfy def/lsp/main.lfy:serve#serve:serve:7196738de4a6bf8ae29e9643c39ab240a403c940fbd56ad082508f89c036e5cf
pub fn serve(root: Option<&Path>) -> i32 {
    let root = root.map(Path::to_path_buf);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("elfie lsp: the runtime could not start: {error}"); // @lfy def/lsp/main.lfy:serve#serve:serve:7196738de4a6bf8ae29e9643c39ab240a403c940fbd56ad082508f89c036e5cf
            return 1;
        }
    };
    runtime.block_on(serve_on(tokio::io::stdin(), tokio::io::stdout(), root))
}

/// [`serve`] over any pair of streams, so tests can drive the whole server in memory.
// @lfy def/lsp/main.lfy:serve#serve:serve:7196738de4a6bf8ae29e9643c39ab240a403c940fbd56ad082508f89c036e5cf
async fn serve_on<I, O>(input: I, output: O, root: Option<PathBuf>) -> i32
where
    I: AsyncRead + Unpin,
    O: AsyncWrite,
{
    let (service, socket) = LspService::build(|client| Backend::new(client, root)).finish();
    let shared = service.inner().shared.clone();
    // Decision: one request is served at a time, so requests are answered in the order
    // received as the definition asks. This gives up `$/cancelRequest`, which no feature
    // here is slow enough to need.
    // @lfy def/lsp/main.lfy:serve#serve:serve:4937fcd30fb289ef0849fe779932e64c88f1f2bba4f6cd86d0e8a102b01fba3a
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
fn exit_code(shut_down: bool) -> i32 {
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:52e894544bf48ab65ab233bbd812c781ada5a68b11319afc43c1782210328930
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:b4ebed8149cfbbdc05efa962364790809b8664d6cfc03bcd3f11b8470cb345e8
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:446bf5e30c867947956aee2a2210a127e47cae9b58d4270c7a32efdcfe1c8fa6
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:182e5604e01a5de3e510428d81a663886ad46818c1e00ea04d00844bfb30bb6e
    // @lfy def/lsp/main.lfy:serve#serve:serve:b4ebed8149cfbbdc05efa962364790809b8664d6cfc03bcd3f11b8470cb345e8
    async fn document_changed(
        &self,
        uri: &Url,
        version: i32,
        changes: Vec<lsp::TextDocumentContentChangeEvent>,
        opened: bool,
    ) {
        let outcome = self.update(|session| {
            let Some(path) = relative_path(&session.workspace.root, uri) else {
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
// @lfy def/lsp/main.lfy:serve#serve:serve:515f9ffc52eafb3e6dea5e0d8887a58b960cabe1e7f8d580cdbebfa1a13c69aa
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:30dc14e0f289c1258311c706561d518b00e9be0a7c99c2b8bf4f4fa51f427dfa
    async fn initialize(&self, params: lsp::InitializeParams) -> Result<lsp::InitializeResult> {
        let root = chosen_root(self.shared.root.as_deref(), &params);
        // @lfy def/lsp/main.lfy:serve#serve:serve:5a0b7fb571d24e54c8199848856e25a156bcb2638bc4afdafbcc29e42fdde20d
        // @lfy def/lsp/main.lfy:serve#serve:serve:ba7ece03ff9de8b007d301f51ded29f62b4305f54a4f85cdd16cd729c553d371
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
            capabilities: capabilities(encoding), // @lfy def/lsp/main.lfy:serve#serve:serve:79f6eb4bccf71db134864b486cfcf84547422c24d02478c3e526d8f744682cb9
            server_info: Some(lsp::ServerInfo {
                name: "elfie".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    /// After initialized, diagnostics are published for every file of the program, and
    /// the server registers for watched file changes.
    // @lfy def/lsp/main.lfy:serve#serve:serve:515f9ffc52eafb3e6dea5e0d8887a58b960cabe1e7f8d580cdbebfa1a13c69aa
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

    // @lfy def/lsp/main.lfy:serve#serve:serve:182e5604e01a5de3e510428d81a663886ad46818c1e00ea04d00844bfb30bb6e
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

    // @lfy def/lsp/main.lfy:serve#serve:serve:182e5604e01a5de3e510428d81a663886ad46818c1e00ea04d00844bfb30bb6e
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:b9952d80447661b1f4bc21b235257faeaaf5e82545ead260b127d1699140b876
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:ec02e46e24878bf02d348987ba906b6878e72ff6ddb4b9955a57849c7eeefb33
    // @lfy def/lsp/main.lfy:serve#serve:serve:d71245c327c27c0b7ed7588fd887ae9bdf7afc23a61fec09deb75bd6fce23fbb
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
                    reload = true; // @lfy def/lsp/main.lfy:serve#serve:serve:d71245c327c27c0b7ed7588fd887ae9bdf7afc23a61fec09deb75bd6fce23fbb
                } else if path.ends_with(EXTENSION) && session.document(&path).is_none() {
                    changed.push(path); // @lfy def/lsp/main.lfy:serve#serve:serve:ec02e46e24878bf02d348987ba906b6878e72ff6ddb4b9955a57849c7eeefb33
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:8868f2ece5fbde667df29fb1c491f3e589e47ab7c7dcf13b4b95e3fdb6f3cda8
    // @lfy def/lsp/main.lfy:serve#serve:serve:835c173bd97a488aa98a1838613c55dc63cd9cff3ec5d90020608a1e9b7e0d44
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:748a92f9b1fedc9f365f976776894b79d464951404a4bc6568a67e0b60ee0f30
    // @lfy def/lsp/main.lfy:serve#serve:serve:707b1bec80ad9693cd2dd8518ff0bec234e2f7f098edaccda133e8d4dba6695b
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:439a7379dfeebabd58172f24f5ef517eb067a078cabcf0b02cab8ed5b9c76ed3
    // @lfy def/lsp/main.lfy:serve#serve:serve:b7c85c085f0f4d121a753fd49d66621f7c0d6737432a6bd9fa7ba8ca342c2a27
    // @lfy def/lsp/main.lfy:serve#serve:serve:13627f82b3b59127f8dcb8782d70df253a9f2af00eb90ddc3167b1c8233438d7
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
// @lfy def/lsp/main.lfy:serve#serve:serve:30dc14e0f289c1258311c706561d518b00e9be0a7c99c2b8bf4f4fa51f427dfa
fn chosen_root(given: Option<&Path>, params: &lsp::InitializeParams) -> PathBuf {
    let root = given
        .map(Path::to_path_buf)
        // @lfy def/lsp/main.lfy:serve#serve:serve:ef49892b8f2fb6e936b6a3e996499aed7a9202a5251d85c6530391d13822d99c
        .or_else(|| {
            params
                .workspace_folders
                .as_ref()?
                .first()?
                .uri
                .to_file_path()
                .ok()
        })
        // @lfy def/lsp/main.lfy:serve#serve:serve:9b470be59e32acca9166c801c02dc83923c61108c1efc06fc3e63ba9883fb1eb
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    std::path::absolute(&root).unwrap_or(root)
}

/// The encoding agreed at initialization: utf-8 when the client offers it, utf-16
/// otherwise.
// @lfy def/lsp/main.lfy:serve#serve:serve:5a0b7fb571d24e54c8199848856e25a156bcb2638bc4afdafbcc29e42fdde20d
// @lfy def/lsp/main.lfy:serve#serve:serve:ba7ece03ff9de8b007d301f51ded29f62b4305f54a4f85cdd16cd729c553d371
fn negotiate(offered: Option<&[lsp::PositionEncodingKind]>) -> Encoding {
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
// @lfy def/lsp/main.lfy:serve#serve:serve:79f6eb4bccf71db134864b486cfcf84547422c24d02478c3e526d8f744682cb9
fn capabilities(encoding: Encoding) -> lsp::ServerCapabilities {
    lsp::ServerCapabilities {
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
        // @lfy def/lsp/main.lfy:serve#serve:serve:a9a3e15a0294152121c3e797cfcd5d313310cf15f49f240842b5ce26e2030773
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
// @lfy def/lsp/main.lfy:serve#serve:serve:a9a3e15a0294152121c3e797cfcd5d313310cf15f49f240842b5ce26e2030773
fn legend() -> lsp::SemanticTokensLegend {
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
fn encode_tokens(tokens: &[SemanticToken], protocol: &[lsp::Range]) -> Vec<lsp::SemanticToken> {
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
        // @lfy def/lsp/main.lfy:serve#serve:serve:62c2f8afd2291ac996baf6f72bab173741e824dbfae095fd305eddb7490900f4
        // @lfy def/lsp/main.lfy:serve#serve:serve:b4de9630aa7442b8a14279471c6cd6fdee8bae7c9a93407f14622e13fc83f5ac
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

/// A document URI as a path relative to the root, with forward slashes; `None` when the
/// document is outside the root.
// @lfy def/lsp/main.lfy:serve#serve:serve:488b56c6bce60decf85f2506fccbabce6330487fb875d7109ca0051f995a67d3
fn relative_path(root: &Path, uri: &Url) -> Option<String> {
    let path = uri.to_file_path().ok()?;
    let relative = path
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .ok()
        .or_else(|| {
            let root = root.canonicalize().ok()?;
            let path = path.canonicalize().ok()?;
            path.strip_prefix(&root).map(Path::to_path_buf).ok()
        })?;
    let text = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    (!text.is_empty()).then_some(text)
}

/// A path relative to the root as a document URI.
// @lfy def/lsp/main.lfy:serve#serve:serve:488b56c6bce60decf85f2506fccbabce6330487fb875d7109ca0051f995a67d3
fn uri_of(root: &Path, path: &str) -> Option<Url> {
    Url::from_file_path(root.join(path)).ok()
}

/// The count of code units of an encoding in the characters before a column of a line;
/// one unit per character past the end of the line.
// @lfy def/lsp/main.lfy:serve#serve:serve:f2dfae1d6221881edd3eb9b2628e9e91d1515627a0ed6577a10a5075f03d8484
fn to_units(encoding: Encoding, line: &str, column: usize) -> usize {
    let mut units = 0;
    let mut chars = 0;
    for c in line.chars() {
        if chars == column {
            return units;
        }
        units += encoding.units_of(c);
        chars += 1;
    }
    units + (column - chars)
}

/// The column of the character that a count of code units of an encoding reaches on a
/// line; a count inside a character gives that character's column, and one past the end
/// of the line counts one character per unit.
// @lfy def/lsp/main.lfy:serve#serve:serve:f2dfae1d6221881edd3eb9b2628e9e91d1515627a0ed6577a10a5075f03d8484
fn from_units(encoding: Encoding, line: &str, character: usize) -> usize {
    let mut units = 0;
    let mut column = 0;
    for c in line.chars() {
        let next = units + encoding.units_of(c);
        if next > character {
            return column;
        }
        units = next;
        column += 1;
    }
    column + (character - units)
}

/// The position just after the last character of a text, in protocol terms.
fn end_of(text: &str, encoding: Encoding) -> lsp::Position {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.last().copied().unwrap_or("");
    lsp::Position {
        line: (lines.len() - 1) as u32,
        character: to_units(encoding, last, last.chars().count()) as u32,
    }
}

/// The byte offset of a protocol position in a text.
fn offset_of(text: &str, encoding: Encoding, position: lsp::Position) -> usize {
    let mut byte = 0;
    for (index, line) in text.split('\n').enumerate() {
        if index == position.line as usize {
            let column = from_units(encoding, line, position.character as usize);
            let within = line
                .char_indices()
                .nth(column)
                .map(|(offset, _)| offset)
                .unwrap_or(line.len());
            return byte + within;
        }
        byte += line.len() + 1;
    }
    text.len()
}

/// The text after one content change: the whole text when the change carries no range,
/// else the held text with the range replaced.
// Decision: full synchronization is what is advertised, but a change that carries a range
// anyway is reassembled into the full text rather than dropped, since the definition
// says incremental changes would only be reassembled.
// @lfy def/lsp/main.lfy:serve#serve:serve:182e5604e01a5de3e510428d81a663886ad46818c1e00ea04d00844bfb30bb6e
fn apply_change(
    text: &str,
    encoding: Encoding,
    change: &lsp::TextDocumentContentChangeEvent,
) -> String {
    let Some(range) = change.range else {
        return change.text.clone();
    };
    let start = offset_of(text, encoding, range.start);
    let end = offset_of(text, encoding, range.end).max(start);
    let mut result = String::with_capacity(text.len() + change.text.len());
    result.push_str(&text[..start]);
    result.push_str(&change.text);
    result.push_str(&text[end..]);
    result
}

/// The lines of a file as the session sees it: the open document's text, else the text
/// of the file's tokens.
fn file_lines(session: &Session, file: &str) -> Vec<String> {
    let text = match session.document(file) {
        Some(document) => document.text.clone(),
        None => match session.workspace.model.file(file) {
            Some(index) => {
                let tree = &session.workspace.model.sources[index].tree;
                tree.raw(0, tree.tokens.len())
            }
            None => String::new(),
        },
    };
    text.split('\n').map(str::to_string).collect()
}

/// Converts positions between the queries' terms and the protocol's for one session: a
/// protocol line is the position's line minus one, and a protocol character is the count
/// of the session's encoding's code units in the characters before the column.
// @lfy def/lsp/main.lfy:serve#serve:serve:f2dfae1d6221881edd3eb9b2628e9e91d1515627a0ed6577a10a5075f03d8484
struct Converter {
    encoding: Encoding,
    lines: HashMap<String, Vec<String>>,
}

impl Converter {
    fn new(encoding: Encoding) -> Converter {
        Converter {
            encoding,
            lines: HashMap::new(),
        }
    }

    /// The lines of a file, read once per conversion batch.
    fn line(&mut self, session: &Session, file: &str, line: usize) -> &str {
        let lines = self
            .lines
            .entry(file.to_string())
            .or_insert_with(|| file_lines(session, file));
        lines.get(line).map(String::as_str).unwrap_or("")
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:f2dfae1d6221881edd3eb9b2628e9e91d1515627a0ed6577a10a5075f03d8484
    fn protocol_position(&mut self, session: &Session, file: &str, at: Position) -> lsp::Position {
        let line = at.line.saturating_sub(1);
        let encoding = self.encoding;
        let text = self.line(session, file, line);
        lsp::Position {
            line: line as u32,
            character: to_units(encoding, text, at.column) as u32,
        }
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:f2dfae1d6221881edd3eb9b2628e9e91d1515627a0ed6577a10a5075f03d8484
    fn query_position(&mut self, session: &Session, file: &str, at: lsp::Position) -> Position {
        let encoding = self.encoding;
        let text = self.line(session, file, at.line as usize);
        Position {
            line: at.line as usize + 1,
            column: from_units(encoding, text, at.character as usize),
        }
    }

    /// Every range sent is a `Range` whose positions are converted to a protocol line of
    /// the position's line minus one and a protocol character of the count of the session's
    /// encoding's code units before the position's column on that line.
    // @lfy def/lsp/main.lfy:serve#serve:serve:c6aaf9c340e97da137353d24f7866c2554e9e82b3605853cfcf41d76d04e32a2
    fn range(&mut self, session: &Session, range: &Range) -> lsp::Range {
        lsp::Range {
            start: self.protocol_position(session, &range.file, range.start),
            end: self.protocol_position(session, &range.file, range.end),
        }
    }

    // @lfy def/lsp/main.lfy:serve
    fn location(&mut self, session: &Session, range: &Range) -> Option<lsp::Location> {
        let uri = uri_of(&session.workspace.root, &range.file)?;
        Some(lsp::Location {
            uri,
            range: self.range(session, range),
        })
    }

    // @lfy def/lsp/main.lfy:serve
    fn diagnostic(&mut self, session: &Session, diagnostic: &query::Diagnostic) -> lsp::Diagnostic {
        lsp::Diagnostic {
            range: self.range(session, &diagnostic.range),
            severity: Some(severity(diagnostic.severity)),
            code: Some(lsp::NumberOrString::String(diagnostic.stage.to_string())),
            source: Some("elfie".to_string()),
            message: diagnostic.message.clone(),
            ..Default::default()
        }
    }
}

/// Every file of the program that lies under the root, so that a file the editor could
/// open is one the diagnostics of the program speak for.
// Decision: a file the standard library brought from outside the project is left out; its
// path is absolute, it is no document of this root, and the criterion on a document
// outside the root gives it nothing.
// @lfy def/lsp/main.lfy:serve#serve:serve:515f9ffc52eafb3e6dea5e0d8887a58b960cabe1e7f8d580cdbebfa1a13c69aa
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
// @lfy def/lsp/main.lfy:serve#serve:serve:515f9ffc52eafb3e6dea5e0d8887a58b960cabe1e7f8d580cdbebfa1a13c69aa
fn by_file(
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
// @lfy def/lsp/main.lfy:serve#serve:serve:8887ce9fee63fc855408cc167c51763a5ffe4985add44b4abc8c37bbf02baf60
fn diagnostics_delta(
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

/// A severity as the protocol spells it.
fn severity(severity: query::Severity) -> lsp::DiagnosticSeverity {
    match severity {
        query::Severity::Error => lsp::DiagnosticSeverity::ERROR,
        query::Severity::Warning => lsp::DiagnosticSeverity::WARNING,
        query::Severity::Information => lsp::DiagnosticSeverity::INFORMATION,
        query::Severity::Hint => lsp::DiagnosticSeverity::HINT,
    }
}

/// A hover as markdown: a code line of the kind, identifier, a colon, and the type; then
/// the definition; then the documentation; then each criterion as a list item reading its
/// situations then its behaviors. The code line ends with the owner in parentheses when
/// there is one, after the type; the traits follow the definition on one line when there
/// are any, before the documentation, so a native thing shows `builtin`.
// @lfy def/lsp/main.lfy:serve#serve:serve:19386c9f8368f5aaddbde274c6c2dddb089872f7a9a314186a05cdd7a769b6e3
fn hover_markdown(hover: &query::Hover) -> String {
    let mut sections = Vec::new();
    let mut code = format!("{} {}", hover.kind, hover.identifier);
    if let Some(ty) = &hover.ty {
        code.push_str(": ");
        code.push_str(ty);
    }
    // Decision: the owner closes the code line, so the kind, the name and the type read as
    // they are written and the declaration that owns them follows in parentheses.
    // @lfy def/lsp/main.lfy:serve#serve:serve:82ed8af526f087dbb8f60d177aff6b9438d990b79b4432612ede9da2ce310740
    if let Some(owner) = hover.owner.as_deref().filter(|text| !text.is_empty()) {
        code.push_str(&format!(" ({owner})"));
    }
    sections.push(format!("```elfie\n{code}\n```"));
    if let Some(definition) = hover.definition.as_deref().filter(|text| !text.is_empty()) {
        sections.push(definition.to_string());
    }
    // Decision: the traits are one line of their identifiers in application order, joined by
    // a comma, with nothing around them; the criterion names no label.
    // @lfy def/lsp/main.lfy:serve#serve:serve:db3a71ea06a9558557ec20264744aaab1992bb0cd0faff6697eb4e314f15f4f2
    if !hover.traits.is_empty() {
        sections.push(hover.traits.join(", "));
    }
    if let Some(documentation) = hover
        .documentation
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        sections.push(documentation.to_string());
    }
    if !hover.criteria.is_empty() {
        sections.push(
            hover
                .criteria
                .iter()
                .map(|criterion| format!("- {}", criterion_item(criterion)))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    sections.join("\n\n")
}

/// One criterion as a list item: its situations, then its behaviors.
// Decision: several situations or behaviors are joined by a semicolon, and the situations
// are separated from the behaviors by a colon.
// @lfy def/lsp/main.lfy:serve#serve:serve:19386c9f8368f5aaddbde274c6c2dddb089872f7a9a314186a05cdd7a769b6e3
fn criterion_item(criterion: &Criterion) -> String {
    let situations = criterion
        .situation
        .iter()
        .flatten()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("; ");
    let behaviors = criterion
        .behavior
        .iter()
        .flatten()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("; ");
    match (situations.is_empty(), behaviors.is_empty()) {
        (false, false) => format!("{situations}: {behaviors}"),
        (false, true) => situations,
        (true, _) => behaviors,
    }
}

/// A completion kind mapped to the protocol's kinds: keyword to Keyword, path to File,
/// data and type to Struct, trait to Interface, enum to Enum, function and agentFunction
/// to Function, member to Field, variable and parameter to Variable, module to Module,
/// anything else to Text.
// @lfy def/lsp/main.lfy:serve#serve:serve:d4e7595e6e9f64e2a9a1d19156e9b56857bed804e62b3119f125644cd3276cfa
fn completion_kind(kind: &str) -> lsp::CompletionItemKind {
    match kind {
        "keyword" => lsp::CompletionItemKind::KEYWORD,
        "path" => lsp::CompletionItemKind::FILE,
        "data" | "type" => lsp::CompletionItemKind::STRUCT,
        "trait" => lsp::CompletionItemKind::INTERFACE,
        "enum" => lsp::CompletionItemKind::ENUM,
        "function" | "agentFunction" => lsp::CompletionItemKind::FUNCTION,
        "member" => lsp::CompletionItemKind::FIELD,
        "variable" | "parameter" => lsp::CompletionItemKind::VARIABLE,
        "module" => lsp::CompletionItemKind::MODULE,
        _ => lsp::CompletionItemKind::TEXT,
    }
}

/// An outline kind mapped to the LSP 3.17 SymbolKind: data and type to Struct, trait to
/// Interface, enum to Enum, enumMember to EnumMember, function and agentFunction to
/// Function, member to Field, typeParameter to TypeParameter, module to Module, and
/// variable, loopVariable, parameter, alias, and external to Variable.
// Decision: the kind is the enum, not its spelling, so the mapping is exhaustive over the
// kinds an outline can carry and no kind falls through to one the criterion does not name.
// @lfy def/lsp/main.lfy:serve#serve:serve:2d7434ceacaf117b84a50944d6defac63a931122b09e46f84d1972331095c38d
fn symbol_kind(kind: SymbolKind) -> lsp::SymbolKind {
    match kind {
        SymbolKind::Data | SymbolKind::Type => lsp::SymbolKind::STRUCT,
        SymbolKind::Trait => lsp::SymbolKind::INTERFACE,
        SymbolKind::Enum => lsp::SymbolKind::ENUM,
        SymbolKind::EnumMember => lsp::SymbolKind::ENUM_MEMBER,
        SymbolKind::Function | SymbolKind::AgentFunction => lsp::SymbolKind::FUNCTION,
        SymbolKind::Member => lsp::SymbolKind::FIELD,
        SymbolKind::TypeParameter => lsp::SymbolKind::TYPE_PARAMETER,
        SymbolKind::Module => lsp::SymbolKind::MODULE,
        SymbolKind::Variable
        | SymbolKind::LoopVariable
        | SymbolKind::Parameter
        | SymbolKind::Alias
        | SymbolKind::External => lsp::SymbolKind::VARIABLE,
    }
}

/// An outline entry nested as a document symbol.
// @lfy def/lsp/main.lfy:serve#serve:serve:2d7434ceacaf117b84a50944d6defac63a931122b09e46f84d1972331095c38d
#[allow(deprecated)]
fn document_symbol(
    converter: &mut Converter,
    session: &Session,
    outline: &query::Outline,
) -> lsp::DocumentSymbol {
    let children = outline
        .children
        .iter()
        .map(|child| document_symbol(converter, session, child))
        .collect::<Vec<_>>();
    lsp::DocumentSymbol {
        name: outline.name.clone(),
        detail: None,
        kind: symbol_kind(outline.kind),
        tags: None,
        deprecated: None,
        range: converter.range(session, &outline.range),
        selection_range: converter.range(session, &outline.selection_range),
        children: (!children.is_empty()).then_some(children),
    }
}

/// A flattened outline entry as a workspace symbol.
// @lfy def/lsp/main.lfy:serve#serve:serve:03938d8acea19d9cedc01076466a3b427d91282d72a892fe54dfdd76f1b12547
#[allow(deprecated)]
fn symbol_information(outline: &query::Outline, location: lsp::Location) -> lsp::SymbolInformation {
    lsp::SymbolInformation {
        name: outline.name.clone(),
        kind: symbol_kind(outline.kind),
        tags: None,
        deprecated: None,
        location,
        container_name: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

    use super::*;

    // Pure parts.

    // @lfy def/lsp/main.lfy:serve#serve:serve:f2dfae1d6221881edd3eb9b2628e9e91d1515627a0ed6577a10a5075f03d8484
    #[test]
    fn columns_convert_to_code_units_and_back() {
        let line = "aé😀b";
        assert_eq!(to_units(Encoding::Utf8, line, 3), 1 + 2 + 4);
        assert_eq!(to_units(Encoding::Utf16, line, 3), 1 + 1 + 2);
        assert_eq!(to_units(Encoding::Utf32, line, 3), 3);
        assert_eq!(from_units(Encoding::Utf8, line, 7), 3);
        assert_eq!(from_units(Encoding::Utf16, line, 4), 3);
        assert_eq!(from_units(Encoding::Utf32, line, 3), 3);
        // Inside a character: the character's own column.
        assert_eq!(from_units(Encoding::Utf16, line, 3), 2);
        assert_eq!(from_units(Encoding::Utf8, line, 2), 1);
        // Past the end: one unit per character.
        assert_eq!(to_units(Encoding::Utf16, line, 6), 5 + 2);
        assert_eq!(from_units(Encoding::Utf16, line, 7), 6);
        assert_eq!(to_units(Encoding::Utf8, "", 0), 0);
        assert_eq!(from_units(Encoding::Utf8, "", 0), 0);
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:c6aaf9c340e97da137353d24f7866c2554e9e82b3605853cfcf41d76d04e32a2
    // @lfy def/lsp/main.lfy:serve#serve:serve:f2dfae1d6221881edd3eb9b2628e9e91d1515627a0ed6577a10a5075f03d8484
    #[test]
    fn ranges_convert_through_the_session_lines() {
        let root = fixture(&[("def/a.lfy", "const x = 'é😀';\nconst y = x;\n")]);
        let session = Session {
            workspace: workspace::load(&root),
            documents: Vec::new(),
            encoding: Encoding::Utf16,
        };
        let mut converter = Converter::new(Encoding::Utf16);
        let range = Range {
            file: "def/a.lfy".to_string(),
            start: Position::new(1, 14),
            end: Position::new(2, 7),
        };
        let converted = converter.range(&session, &range);
        assert_eq!(converted.start, lsp::Position::new(0, 15));
        assert_eq!(converted.end, lsp::Position::new(1, 7));
        assert_eq!(
            converter.query_position(&session, "def/a.lfy", lsp::Position::new(0, 15)),
            Position::new(1, 14)
        );
        // An open document's text wins over the tokens on disk.
        let session = Session {
            documents: vec![Document {
                path: "def/a.lfy".to_string(),
                text: "😀😀 x".to_string(),
                version: 2,
            }],
            ..session
        };
        let mut converter = Converter::new(Encoding::Utf8);
        assert_eq!(
            converter.protocol_position(&session, "def/a.lfy", Position::new(1, 3)),
            lsp::Position::new(0, 9)
        );
        let _ = fs::remove_dir_all(&root);
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:182e5604e01a5de3e510428d81a663886ad46818c1e00ea04d00844bfb30bb6e
    #[test]
    fn changes_with_a_range_are_reassembled_into_the_text() {
        let text = "const x = 1;\nconst y = 2;\n";
        let change =
            |start: (u32, u32), end: (u32, u32), new: &str| lsp::TextDocumentContentChangeEvent {
                range: Some(lsp::Range {
                    start: lsp::Position::new(start.0, start.1),
                    end: lsp::Position::new(end.0, end.1),
                }),
                range_length: None,
                text: new.to_string(),
            };
        assert_eq!(
            apply_change(text, Encoding::Utf16, &change((1, 6), (1, 7), "z")),
            "const x = 1;\nconst z = 2;\n"
        );
        assert_eq!(
            apply_change(text, Encoding::Utf16, &change((0, 12), (1, 0), " ")),
            "const x = 1; const y = 2;\n"
        );
        let whole = lsp::TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "d A {}".to_string(),
        };
        assert_eq!(apply_change(text, Encoding::Utf16, &whole), "d A {}");
        assert_eq!(end_of("a\nbc", Encoding::Utf8), lsp::Position::new(1, 2));
        assert_eq!(end_of("a\n", Encoding::Utf8), lsp::Position::new(1, 0));
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:488b56c6bce60decf85f2506fccbabce6330487fb875d7109ca0051f995a67d3
    #[test]
    fn uris_map_to_paths_relative_to_the_root_and_back() {
        let root = fixture(&[("def/a.lfy", "d A {}\n")]);
        let uri = uri_of(&root, "def/a.lfy").unwrap();
        assert!(uri.as_str().ends_with("/def/a.lfy"));
        assert_eq!(relative_path(&root, &uri).as_deref(), Some("def/a.lfy"));
        let outside = Url::from_file_path(root.parent().unwrap().join("other.lfy")).unwrap();
        assert_eq!(relative_path(&root, &outside), None);
        let root_itself = Url::from_file_path(&root).unwrap();
        assert_eq!(relative_path(&root, &root_itself), None);
        let _ = fs::remove_dir_all(&root);
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:30dc14e0f289c1258311c706561d518b00e9be0a7c99c2b8bf4f4fa51f427dfa
    // @lfy def/lsp/main.lfy:serve#serve:serve:ef49892b8f2fb6e936b6a3e996499aed7a9202a5251d85c6530391d13822d99c
    // @lfy def/lsp/main.lfy:serve#serve:serve:9b470be59e32acca9166c801c02dc83923c61108c1efc06fc3e63ba9883fb1eb
    #[test]
    #[allow(deprecated)]
    fn the_given_root_beats_the_first_folder_which_beats_the_current_directory() {
        let folder = |path: &str| lsp::WorkspaceFolder {
            uri: Url::from_file_path(path).unwrap(),
            name: path.to_string(),
        };
        let named = lsp::InitializeParams {
            workspace_folders: Some(vec![
                folder("/tmp/elfie-first"),
                folder("/tmp/elfie-second"),
            ]),
            ..Default::default()
        };
        let given = PathBuf::from("/tmp/elfie-given");
        assert_eq!(chosen_root(Some(&given), &named), given);
        assert_eq!(chosen_root(None, &named), PathBuf::from("/tmp/elfie-first"));
        let none = lsp::InitializeParams::default();
        assert_eq!(chosen_root(Some(&given), &none), given);
        assert_eq!(
            chosen_root(None, &none),
            std::env::current_dir().expect("a current directory")
        );
        let only_root_uri = lsp::InitializeParams {
            root_uri: Some(Url::from_file_path("/tmp/elfie-root-uri").unwrap()),
            ..Default::default()
        };
        assert_eq!(chosen_root(Some(&given), &only_root_uri), given);
        assert_eq!(
            chosen_root(None, &only_root_uri),
            std::env::current_dir().expect("a current directory")
        );
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:5a0b7fb571d24e54c8199848856e25a156bcb2638bc4afdafbcc29e42fdde20d
    // @lfy def/lsp/main.lfy:serve#serve:serve:ba7ece03ff9de8b007d301f51ded29f62b4305f54a4f85cdd16cd729c553d371
    #[test]
    fn utf8_is_agreed_only_when_offered() {
        assert_eq!(negotiate(None), Encoding::Utf16);
        assert_eq!(
            negotiate(Some(&[lsp::PositionEncodingKind::UTF16])),
            Encoding::Utf16
        );
        assert_eq!(
            negotiate(Some(&[
                lsp::PositionEncodingKind::UTF32,
                lsp::PositionEncodingKind::UTF8
            ])),
            Encoding::Utf8
        );
        assert_eq!(Encoding::Utf8.protocol(), lsp::PositionEncodingKind::UTF8);
        assert_eq!(Encoding::Utf16.value(), "utf-16");
        assert_eq!(Encoding::Utf32.value(), "utf-32");
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:19386c9f8368f5aaddbde274c6c2dddb089872f7a9a314186a05cdd7a769b6e3
    // @lfy def/lsp/main.lfy:serve#serve:serve:82ed8af526f087dbb8f60d177aff6b9438d990b79b4432612ede9da2ce310740
    // @lfy def/lsp/main.lfy:serve#serve:serve:db3a71ea06a9558557ec20264744aaab1992bb0cd0faff6697eb4e314f15f4f2
    #[test]
    fn hover_reads_code_line_owner_definition_traits_documentation_then_criteria() {
        let hover = query::Hover {
            range: Range::empty("def/a.lfy", Position::new(1, 0)),
            kind: elfie_core::model::SymbolKind::Data,
            identifier: "A".to_string(),
            definition: Some("An A".to_string()),
            ty: Some("Base".to_string()),
            owner: None,
            traits: vec!["builtin".to_string(), "tool".to_string()],
            documentation: Some("Doc line".to_string()),
            criteria: vec![
                Criterion {
                    situation: Some(vec!["It rains".to_string()]),
                    behavior: Some(vec!["Stay in".to_string(), "Read".to_string()]),
                    ..Default::default()
                },
                Criterion {
                    behavior: Some(vec!["Always".to_string()]),
                    ..Default::default()
                },
            ],
        };
        assert_eq!(
            hover_markdown(&hover),
            "```elfie\ndata A: Base\n```\n\nAn A\n\nbuiltin, tool\n\nDoc line\n\n- It rains: Stay in; Read\n- Always"
        );
        // A member's owner closes the code line.
        // @lfy def/lsp/main.lfy:serve#serve:serve:82ed8af526f087dbb8f60d177aff6b9438d990b79b4432612ede9da2ce310740
        let owned = query::Hover {
            kind: elfie_core::model::SymbolKind::Member,
            identifier: "n".to_string(),
            ty: Some("string".to_string()),
            owner: Some("A".to_string()),
            ..hover.clone()
        };
        assert!(
            hover_markdown(&owned).starts_with("```elfie\nmember n: string (A)\n```"),
            "{}",
            hover_markdown(&owned)
        );
        let bare = query::Hover {
            definition: None,
            ty: None,
            owner: Some("Box".to_string()),
            traits: Vec::new(),
            documentation: Some(String::new()),
            criteria: Vec::new(),
            ..hover
        };
        assert_eq!(hover_markdown(&bare), "```elfie\ndata A (Box)\n```");
    }

    /// Every completion kind maps to the protocol's kind the completion criterion names.
    // @lfy def/lsp/main.lfy:serve#serve:serve:d4e7595e6e9f64e2a9a1d19156e9b56857bed804e62b3119f125644cd3276cfa
    #[test]
    fn kinds_map_to_the_protocols_kinds() {
        let cases = [
            ("keyword", lsp::CompletionItemKind::KEYWORD),
            ("path", lsp::CompletionItemKind::FILE),
            ("data", lsp::CompletionItemKind::STRUCT),
            ("type", lsp::CompletionItemKind::STRUCT),
            ("trait", lsp::CompletionItemKind::INTERFACE),
            ("enum", lsp::CompletionItemKind::ENUM),
            ("function", lsp::CompletionItemKind::FUNCTION),
            ("agentFunction", lsp::CompletionItemKind::FUNCTION),
            ("member", lsp::CompletionItemKind::FIELD),
            ("variable", lsp::CompletionItemKind::VARIABLE),
            ("parameter", lsp::CompletionItemKind::VARIABLE),
            ("module", lsp::CompletionItemKind::MODULE),
            ("context", lsp::CompletionItemKind::TEXT),
            ("alias", lsp::CompletionItemKind::TEXT),
        ];
        for (kind, expected) in cases {
            assert_eq!(completion_kind(kind), expected, "{kind}");
        }
    }

    /// Every outline kind maps to the SymbolKind the document symbol criterion names.
    // @lfy def/lsp/main.lfy:serve#serve:serve:2d7434ceacaf117b84a50944d6defac63a931122b09e46f84d1972331095c38d
    #[test]
    fn outline_kinds_map_to_the_lsp_symbol_kinds() {
        let cases = [
            (SymbolKind::Data, lsp::SymbolKind::STRUCT),
            (SymbolKind::Type, lsp::SymbolKind::STRUCT),
            (SymbolKind::Trait, lsp::SymbolKind::INTERFACE),
            (SymbolKind::Enum, lsp::SymbolKind::ENUM),
            (SymbolKind::EnumMember, lsp::SymbolKind::ENUM_MEMBER),
            (SymbolKind::Function, lsp::SymbolKind::FUNCTION),
            (SymbolKind::AgentFunction, lsp::SymbolKind::FUNCTION),
            (SymbolKind::Member, lsp::SymbolKind::FIELD),
            (SymbolKind::TypeParameter, lsp::SymbolKind::TYPE_PARAMETER),
            (SymbolKind::Module, lsp::SymbolKind::MODULE),
            (SymbolKind::Variable, lsp::SymbolKind::VARIABLE),
            (SymbolKind::LoopVariable, lsp::SymbolKind::VARIABLE),
            (SymbolKind::Parameter, lsp::SymbolKind::VARIABLE),
            (SymbolKind::Alias, lsp::SymbolKind::VARIABLE),
            (SymbolKind::External, lsp::SymbolKind::VARIABLE),
        ];
        for (kind, expected) in cases {
            assert_eq!(symbol_kind(kind), expected, "{kind}");
        }
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:6991b81371beae78826a2fc55a6b814c2a8b3ad40a2e223771a6746735cdf4cd
    // @lfy def/lsp/main.lfy:serve#serve:serve:8887ce9fee63fc855408cc167c51763a5ffe4985add44b4abc8c37bbf02baf60
    // @lfy def/lsp/main.lfy:serve#serve:serve:515f9ffc52eafb3e6dea5e0d8887a58b960cabe1e7f8d580cdbebfa1a13c69aa
    #[test]
    fn only_files_whose_diagnostics_changed_are_published() {
        let diagnostic = |file: &str, line: usize, message: &str| query::Diagnostic {
            range: Range::empty(file, Position::new(line, 0)),
            severity: query::Severity::Error,
            stage: query::Stage::Binder,
            message: message.to_string(),
        };
        let program = |files: &[&str]| files.iter().map(|file| file.to_string()).collect::<Vec<_>>();
        let names = |delta: &[(String, Vec<query::Diagnostic>)]| {
            delta
                .iter()
                .map(|(file, diagnostics)| (file.clone(), diagnostics.len()))
                .collect::<Vec<_>>()
        };
        let listed = |files: &[(&str, usize)]| {
            files
                .iter()
                .map(|(file, count)| (file.to_string(), *count))
                .collect::<Vec<_>>()
        };
        let published = by_file(
            &program(&["def/a.lfy", "def/b.lfy", "def/gone.lfy", "def/clean.lfy"]),
            vec![
                diagnostic("def/a.lfy", 1, "one"),
                diagnostic("def/b.lfy", 1, "two"),
                diagnostic("def/gone.lfy", 1, "three"),
            ],
        );
        // Nothing was published before the first publication, so every file of the program
        // is in its delta, a file with no problems as an empty list.
        // @lfy def/lsp/main.lfy:serve#serve:serve:515f9ffc52eafb3e6dea5e0d8887a58b960cabe1e7f8d580cdbebfa1a13c69aa
        assert_eq!(
            names(&diagnostics_delta(&BTreeMap::new(), &published)),
            listed(&[
                ("def/a.lfy", 1),
                ("def/b.lfy", 1),
                ("def/clean.lfy", 0),
                ("def/gone.lfy", 1),
            ])
        );
        let current = by_file(
            &program(&["def/a.lfy", "def/b.lfy", "def/new.lfy", "def/clean.lfy"]),
            vec![
                diagnostic("def/a.lfy", 1, "one"),
                diagnostic("def/b.lfy", 2, "two"),
                diagnostic("def/new.lfy", 1, "four"),
            ],
        );
        // `def/a.lfy` and `def/clean.lfy` did not change, so they are not published again;
        // `def/gone.lfy` left the program, so its diagnostics are published empty.
        // @lfy def/lsp/main.lfy:serve#serve:serve:8887ce9fee63fc855408cc167c51763a5ffe4985add44b4abc8c37bbf02baf60
        assert_eq!(
            names(&diagnostics_delta(&published, &current)),
            listed(&[("def/b.lfy", 1), ("def/new.lfy", 1), ("def/gone.lfy", 0)])
        );
        assert!(diagnostics_delta(&current, &current).is_empty());
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:a3eb66fd8aa2ca379fbfd14839959e5a350c90c0887219b3365368fb35b0c94c
    // @lfy def/lsp/main.lfy:serve#serve:serve:9f66abf6d379ff5502a71a40d7500855a130cd0371d2c538ef5cc2e6122aeb15
    #[test]
    fn exit_follows_shutdown_with_zero_and_without_it_with_one() {
        assert_eq!(exit_code(true), 0);
        assert_eq!(exit_code(false), 1);
    }

    // The lifecycle, driven over the protocol in memory.

    static FIXTURES: AtomicUsize = AtomicUsize::new(0);

    /// A project directory holding the given files, under the temporary directory.
    fn fixture(files: &[(&str, &str)]) -> PathBuf {
        let number = FIXTURES.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("elfie-lsp-{}-{number}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, text) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// Whether a publication carries a diagnostic whose message holds a text.
    fn reported(published: &Value, text: &str) -> bool {
        published["params"]["diagnostics"]
            .as_array()
            .is_some_and(|diagnostics| {
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic["message"].as_str().is_some_and(|m| m.contains(text)))
            })
    }

    /// An editor connected to a server over in-memory streams.
    struct Editor {
        root: PathBuf,
        input: DuplexStream,
        output: DuplexStream,
        served: tokio::task::JoinHandle<i32>,
        pending: VecDeque<Value>,
        next_id: i64,
    }

    impl Editor {
        fn connect(root: PathBuf) -> Editor {
            let (input, server_input) = tokio::io::duplex(1 << 20);
            let (server_output, output) = tokio::io::duplex(1 << 20);
            let served = tokio::spawn(serve_on(server_input, server_output, Some(root.clone())));
            Editor {
                root,
                input,
                output,
                served,
                pending: VecDeque::new(),
                next_id: 0,
            }
        }

        fn uri(&self, path: &str) -> Url {
            uri_of(&self.root, path).unwrap()
        }

        async fn send(&mut self, message: Value) {
            let body = message.to_string();
            let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
            self.input.write_all(framed.as_bytes()).await.unwrap();
        }

        /// A message; `params` is left out when it is null, as clients spell it.
        async fn notify(&mut self, method: &str, params: Value) {
            let mut message = json!({ "jsonrpc": "2.0", "method": method });
            if !params.is_null() {
                message["params"] = params;
            }
            self.send(message).await;
        }

        /// Sends a request without waiting for its response; the id it was sent with.
        async fn send_request(&mut self, method: &str, params: Value) -> i64 {
            self.next_id += 1;
            let id = self.next_id;
            let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
            if !params.is_null() {
                message["params"] = params;
            }
            self.send(message).await;
            id
        }

        /// Sends a request and answers with its response.
        async fn request(&mut self, method: &str, params: Value) -> Value {
            let id = self.send_request(method, params).await;
            self.wait_for(|message| message.get("id") == Some(&json!(id)))
                .await
        }

        async fn read(&mut self) -> Option<Value> {
            let mut header = Vec::new();
            loop {
                let mut byte = [0u8; 1];
                self.output.read_exact(&mut byte).await.ok()?;
                header.push(byte[0]);
                if header.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let header = String::from_utf8_lossy(&header);
            let length: usize = header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length:"))
                .and_then(|value| value.trim().parse().ok())?;
            let mut body = vec![0u8; length];
            self.output.read_exact(&mut body).await.ok()?;
            serde_json::from_slice(&body).ok()
        }

        /// The first message, already received or yet to come, that satisfies a test.
        async fn wait_for(&mut self, test: impl Fn(&Value) -> bool) -> Value {
            if let Some(index) = self.pending.iter().position(&test) {
                return self.pending.remove(index).unwrap();
            }
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    let message = self.read().await.expect("the server closed its output");
                    if test(&message) {
                        return message;
                    }
                    self.pending.push_back(message);
                }
            })
            .await
            .expect("timed out waiting for a message")
        }

        async fn published_for(&mut self, path: &str) -> Value {
            let uri = self.uri(path).to_string();
            self.wait_for(|message| {
                message["method"] == "textDocument/publishDiagnostics"
                    && message["params"]["uri"] == json!(uri)
            })
            .await
        }

        async fn initialize(&mut self, capabilities: Value) -> Value {
            let result = self
                .request("initialize", json!({ "capabilities": capabilities }))
                .await;
            self.notify("initialized", json!({})).await;
            result
        }

        /// Shuts the server down and exits; the server's exit code.
        async fn exit(mut self, shutdown: bool) -> i32 {
            if shutdown {
                let response = self.request("shutdown", Value::Null).await;
                assert!(response.get("error").is_none(), "{response}");
            }
            self.notify("exit", Value::Null).await;
            // The editor closes the pipe after exit; the server ends on that.
            let Editor {
                root,
                input,
                served,
                ..
            } = self;
            drop(input);
            let code = tokio::time::timeout(Duration::from_secs(20), served)
                .await
                .expect("the server did not exit")
                .unwrap();
            let _ = fs::remove_dir_all(&root);
            code
        }
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:1df5bc12c0235689cc0af6c159f4a864f322565d7e78ad72b7a25e1bb20ac88f
    // @lfy def/lsp/main.lfy:serve#serve:serve:7196738de4a6bf8ae29e9643c39ab240a403c940fbd56ad082508f89c036e5cf
    // @lfy def/lsp/main.lfy:serve#serve:serve:515f9ffc52eafb3e6dea5e0d8887a58b960cabe1e7f8d580cdbebfa1a13c69aa
    // @lfy def/lsp/main.lfy:serve#serve:serve:79f6eb4bccf71db134864b486cfcf84547422c24d02478c3e526d8f744682cb9
    // @lfy def/lsp/main.lfy:serve#serve:serve:5a0b7fb571d24e54c8199848856e25a156bcb2638bc4afdafbcc29e42fdde20d
    // @lfy def/lsp/main.lfy:serve#serve:serve:a3eb66fd8aa2ca379fbfd14839959e5a350c90c0887219b3365368fb35b0c94c
    #[tokio::test]
    async fn a_binder_error_is_published_after_initialized_and_exit_after_shutdown_is_zero() {
        let root = fixture(&[("def/a.lfy", "const y = z;\n"), ("def/ok.lfy", "d B {}\n")]);
        let mut editor = Editor::connect(root);
        let result = editor
            .initialize(json!({ "general": { "positionEncodings": ["utf-8", "utf-16"] } }))
            .await;
        let capabilities = &result["result"]["capabilities"];
        assert_eq!(capabilities["positionEncoding"], "utf-8");
        assert_eq!(capabilities["textDocumentSync"], 1);
        assert_eq!(capabilities["hoverProvider"], true);
        assert_eq!(capabilities["definitionProvider"], true);
        assert_eq!(capabilities["referencesProvider"], true);
        assert!(capabilities["completionProvider"].is_object());
        assert_eq!(capabilities["renameProvider"]["prepareProvider"], true);
        assert_eq!(capabilities["documentSymbolProvider"], true);
        assert_eq!(capabilities["workspaceSymbolProvider"], true);
        assert_eq!(capabilities["documentFormattingProvider"], true);

        let published = editor.published_for("def/a.lfy").await;
        let diagnostics = published["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0]["code"], "binder");
        assert_eq!(diagnostics[0]["severity"], 1);
        assert_eq!(diagnostics[0]["source"], "elfie");
        assert_eq!(
            diagnostics[0]["range"],
            json!({ "start": { "line": 0, "character": 10 }, "end": { "line": 0, "character": 11 } })
        );

        // Every file of the program is published, one with no problems as an empty list.
        // @lfy def/lsp/main.lfy:serve#serve:serve:515f9ffc52eafb3e6dea5e0d8887a58b960cabe1e7f8d580cdbebfa1a13c69aa
        let published = editor.published_for("def/ok.lfy").await;
        assert_eq!(published["params"]["diagnostics"], json!([]));

        // A change that fixes the file publishes an empty list for it.
        editor
            .notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": editor.uri("def/a.lfy"), "languageId": "elfie", "version": 1, "text": "const y = z;\n" } }),
            )
            .await;
        editor
            .notify(
                "textDocument/didChange",
                json!({ "textDocument": { "uri": editor.uri("def/a.lfy"), "version": 2 }, "contentChanges": [{ "text": "const z = 1;\nconst y = z;\n" }] }),
            )
            .await;
        let published = editor.published_for("def/a.lfy").await;
        assert_eq!(published["params"]["diagnostics"], json!([]));
        assert_eq!(published["params"]["version"], 2);

        assert_eq!(editor.exit(true).await, 0);
    }

    /// Several requests sent at once are answered in the order they were received, and a
    /// request that fails is one error response among them with the server going on.
    // @lfy def/lsp/main.lfy:serve#serve:serve:4937fcd30fb289ef0849fe779932e64c88f1f2bba4f6cd86d0e8a102b01fba3a
    // @lfy def/lsp/main.lfy:serve#serve:serve:52e894544bf48ab65ab233bbd812c781ada5a68b11319afc43c1782210328930
    #[tokio::test]
    async fn requests_are_answered_in_the_order_received() {
        let root = fixture(&[("def/a.lfy", "d A {}\nconst y = A;\n")]);
        let mut editor = Editor::connect(root);
        editor.initialize(json!({})).await;
        let a = editor.uri("def/a.lfy");
        let at = |line: u32, character: u32| {
            json!({ "textDocument": { "uri": a.clone() }, "position": { "line": line, "character": character } })
        };
        let mut sent = Vec::new();
        sent.push(
            editor
                .send_request(
                    "textDocument/documentSymbol",
                    json!({ "textDocument": { "uri": a.clone() } }),
                )
                .await,
        );
        // Nothing is declared on the keyword, so this one is the error response.
        sent.push(
            editor
                .send_request("textDocument/prepareRename", at(1, 1))
                .await,
        );
        sent.push(editor.send_request("textDocument/hover", at(0, 2)).await);
        sent.push(
            editor
                .send_request("textDocument/definition", at(1, 10))
                .await,
        );
        sent.push(
            editor
                .send_request("workspace/symbol", json!({ "query": "A" }))
                .await,
        );
        let mut answered = Vec::new();
        let mut failed = Vec::new();
        while answered.len() < sent.len() {
            let response = editor
                .wait_for(|message| {
                    message.get("id").is_some() && message.get("method").is_none()
                })
                .await;
            let id = response["id"].as_i64().expect("a response id");
            if response.get("error").is_some() {
                failed.push(id);
            }
            answered.push(id);
        }
        assert_eq!(answered, sent);
        assert_eq!(failed, vec![sent[1]]);
        editor.exit(true).await;
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:9f66abf6d379ff5502a71a40d7500855a130cd0371d2c538ef5cc2e6122aeb15
    #[tokio::test]
    async fn exit_without_shutdown_is_one() {
        let root = fixture(&[("def/a.lfy", "d A {}\n")]);
        let mut editor = Editor::connect(root);
        editor.initialize(json!({})).await;
        assert_eq!(editor.exit(false).await, 1);
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:6a551fb00360c29fa3b9d0074fef3369855aad68d014c80903f4c67d7841e334
    // @lfy def/lsp/main.lfy:serve#serve:serve:37382c32c2e5d125cef7e8f2a6960e4c8fa8bfce5a4499fcee1b8332af049767
    // @lfy def/lsp/main.lfy:serve#serve:serve:748a92f9b1fedc9f365f976776894b79d464951404a4bc6568a67e0b60ee0f30
    // @lfy def/lsp/main.lfy:serve#serve:serve:8868f2ece5fbde667df29fb1c491f3e589e47ab7c7dcf13b4b95e3fdb6f3cda8
    // @lfy def/lsp/main.lfy:serve#serve:serve:835c173bd97a488aa98a1838613c55dc63cd9cff3ec5d90020608a1e9b7e0d44
    // @lfy def/lsp/main.lfy:serve#serve:serve:3998738246ab112a54cafc42192f647ffab183141e3fd8f3f1e5dc82409c7dc9
    // @lfy def/lsp/main.lfy:serve#serve:serve:19386c9f8368f5aaddbde274c6c2dddb089872f7a9a314186a05cdd7a769b6e3
    // @lfy def/lsp/main.lfy:serve#serve:serve:2d7434ceacaf117b84a50944d6defac63a931122b09e46f84d1972331095c38d
    // @lfy def/lsp/main.lfy:serve#serve:serve:03938d8acea19d9cedc01076466a3b427d91282d72a892fe54dfdd76f1b12547
    // @lfy def/lsp/main.lfy:serve#serve:serve:b4ebed8149cfbbdc05efa962364790809b8664d6cfc03bcd3f11b8470cb345e8
    #[tokio::test]
    async fn definition_and_rename_cross_files() {
        let root = fixture(&[
            ("def/a.lfy", "d A {}\n"),
            ("def/b.lfy", "use \"./a\";\nconst y = A;\n"),
        ]);
        let mut editor = Editor::connect(root);
        editor.initialize(json!({})).await;
        let b = editor.uri("def/b.lfy");
        let a = editor.uri("def/a.lfy");
        let at =
            json!({ "textDocument": { "uri": b }, "position": { "line": 1, "character": 10 } });

        let definition = editor.request("textDocument/definition", at.clone()).await;
        assert_eq!(
            definition["result"],
            json!({ "uri": a, "range": { "start": { "line": 0, "character": 2 }, "end": { "line": 0, "character": 3 } } })
        );

        let prepared = editor
            .request("textDocument/prepareRename", at.clone())
            .await;
        assert_eq!(
            prepared["result"],
            json!({ "start": { "line": 1, "character": 10 }, "end": { "line": 1, "character": 11 } })
        );

        let mut rename = at.clone();
        rename["newName"] = json!("B");
        let renamed = editor.request("textDocument/rename", rename).await;
        let changes = &renamed["result"]["changes"];
        assert_eq!(
            changes[a.as_str()],
            json!([{ "range": { "start": { "line": 0, "character": 2 }, "end": { "line": 0, "character": 3 } }, "newText": "B" }])
        );
        assert_eq!(
            changes[b.as_str()],
            json!([{ "range": { "start": { "line": 1, "character": 10 }, "end": { "line": 1, "character": 11 } }, "newText": "B" }])
        );

        // A rename `renameAt` refuses is an error response carrying the reason it returned.
        // @lfy def/lsp/main.lfy:serve#serve:serve:707b1bec80ad9693cd2dd8518ff0bec234e2f7f098edaccda133e8d4dba6695b
        let mut keyword = at.clone();
        keyword["newName"] = json!("const");
        let refused = editor.request("textDocument/rename", keyword).await;
        assert!(
            refused["error"]["message"]
                .as_str()
                .is_some_and(|reason| reason.contains("const is a keyword")),
            "{refused}"
        );

        // Nothing is declared on the keyword, so prepare and rename are errors.
        let nowhere =
            json!({ "textDocument": { "uri": b }, "position": { "line": 1, "character": 1 } });
        let prepared = editor
            .request("textDocument/prepareRename", nowhere.clone())
            .await;
        assert_eq!(prepared["error"]["message"], NOTHING_DECLARED);
        let mut rename = nowhere;
        rename["newName"] = json!("B");
        let renamed = editor.request("textDocument/rename", rename).await;
        assert!(renamed["error"].is_object());

        let references = editor
            .request(
                "textDocument/references",
                json!({ "textDocument": { "uri": b }, "position": { "line": 1, "character": 10 }, "context": { "includeDeclaration": true } }),
            )
            .await;
        assert_eq!(references["result"].as_array().unwrap().len(), 2);

        let hover = editor.request("textDocument/hover", at.clone()).await;
        let markdown = hover["result"]["contents"]["value"].as_str().unwrap_or("");
        assert!(markdown.starts_with("```elfie\ndata A"), "{hover}");

        let symbols = editor
            .request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": a } }),
            )
            .await;
        assert_eq!(symbols["result"][0]["name"], "A");
        assert_eq!(symbols["result"][0]["kind"], 23);

        let found = editor
            .request("workspace/symbol", json!({ "query": "a" }))
            .await;
        let names: Vec<&str> = found["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|symbol| symbol["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"A"), "{names:?}");

        // A document outside the root answers as if nothing were there.
        let outside = Url::from_file_path(editor.root.parent().unwrap().join("x.lfy")).unwrap();
        let hover = editor
            .request(
                "textDocument/hover",
                json!({ "textDocument": { "uri": outside }, "position": { "line": 0, "character": 0 } }),
            )
            .await;
        assert_eq!(hover["result"], Value::Null);

        assert_eq!(editor.exit(true).await, 0);
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:439a7379dfeebabd58172f24f5ef517eb067a078cabcf0b02cab8ed5b9c76ed3
    // @lfy def/lsp/main.lfy:serve#serve:serve:b7c85c085f0f4d121a753fd49d66621f7c0d6737432a6bd9fa7ba8ca342c2a27
    // @lfy def/lsp/main.lfy:serve#serve:serve:13627f82b3b59127f8dcb8782d70df253a9f2af00eb90ddc3167b1c8233438d7
    #[tokio::test]
    async fn formatting_replaces_the_whole_document_only_when_it_differs() {
        let root = fixture(&[("def/a.lfy", "d A {}\n")]);
        let mut editor = Editor::connect(root);
        editor.initialize(json!({})).await;
        let a = editor.uri("def/a.lfy");
        let expected = {
            let workspace = workspace::load(&editor.root);
            let tree = &workspace.model.sources[workspace.model.file("def/a.lfy").unwrap()].tree;
            format::format(tree)
        };
        let params = json!({ "textDocument": { "uri": a }, "options": { "tabSize": 2, "insertSpaces": true } });
        let response = editor
            .request("textDocument/formatting", params.clone())
            .await;
        if expected == "d A {}\n" {
            assert_eq!(response["result"], Value::Null);
        } else {
            assert_eq!(response["result"][0]["newText"], expected);
        }

        editor
            .notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": a, "languageId": "elfie", "version": 1, "text": "d   A   {}" } }),
            )
            .await;
        let response = editor
            .request("textDocument/formatting", params.clone())
            .await;
        let edits = response["result"].as_array().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(
            edits[0]["range"],
            json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 10 } })
        );
        assert_ne!(edits[0]["newText"], "d   A   {}");

        // A tree with errors is not formatted, and the log says so.
        editor
            .notify(
                "textDocument/didChange",
                json!({ "textDocument": { "uri": a, "version": 2 }, "contentChanges": [{ "text": "const = 1;" }] }),
            )
            .await;
        let response = editor.request("textDocument/formatting", params).await;
        assert_eq!(response["result"], Value::Null);
        let logged = editor
            .wait_for(|message| {
                message["method"] == "window/logMessage"
                    && message["params"]["message"]
                        .as_str()
                        .is_some_and(|text| text.contains("errors"))
            })
            .await;
        assert!(
            logged["params"]["message"]
                .as_str()
                .unwrap()
                .contains("def/a.lfy")
        );

        assert_eq!(editor.exit(true).await, 0);
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:f60fe367b8c7bc62490da190f96e531eded102a1ed95a0c464d889b0599a46df
    // @lfy def/lsp/main.lfy:serve#serve:serve:a9a3e15a0294152121c3e797cfcd5d313310cf15f49f240842b5ce26e2030773
    #[test]
    fn tokens_are_encoded_relative_to_the_one_before() {
        let range = |line, start, end| lsp::Range { start: lsp::Position::new(line, start), end: lsp::Position::new(line, end) };
        let query_range = Range { file: "def/a.lfy".into(), start: Position::new(1, 0), end: Position::new(1, 0) };
        let tokens = vec![
            SemanticToken { range: query_range.clone(), ty: TokenType::Data, modifiers: vec![TokenModifier::Declaration, TokenModifier::Agentic] },
            SemanticToken { range: query_range.clone(), ty: TokenType::Property, modifiers: vec![TokenModifier::Scope] },
            SemanticToken { range: query_range, ty: TokenType::Variable, modifiers: vec![] },
        ];
        let encoded = encode_tokens(&tokens, &[range(0, 2, 3), range(0, 8, 9), range(2, 6, 7)]);
        let flat: Vec<(u32, u32, u32, u32, u32)> = encoded.iter().map(|t| (t.delta_line, t.delta_start, t.length, t.token_type, t.token_modifiers_bitset)).collect();
        // The first token's start is the start itself, the second is on the same line so its
        // start is the difference from the one before, and the third is on another line so
        // its start is the start itself again.
        // @lfy def/lsp/main.lfy:serve#serve:serve:62c2f8afd2291ac996baf6f72bab173741e824dbfae095fd305eddb7490900f4
        // @lfy def/lsp/main.lfy:serve#serve:serve:b4de9630aa7442b8a14279471c6cd6fdee8bae7c9a93407f14622e13fc83f5ac
        assert_eq!(flat, vec![(0, 2, 1, 1, 0b11), (0, 6, 1, 10, 1 << 4), (2, 6, 1, 9, 0)]);
        let legend = legend();
        assert_eq!(legend.token_types[1].as_str(), "data");
        assert_eq!(legend.token_types[8].as_str(), "typeParameter");
        assert_eq!(legend.token_types.len(), 11);
        assert_eq!(legend.token_modifiers[4].as_str(), "scope");
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:f60fe367b8c7bc62490da190f96e531eded102a1ed95a0c464d889b0599a46df
    // @lfy def/lsp/main.lfy:serve#serve:serve:79f6eb4bccf71db134864b486cfcf84547422c24d02478c3e526d8f744682cb9
    #[tokio::test]
    async fn semantic_tokens_are_served_for_a_document() {
        let root = fixture(&[("def/a.lfy", "d A {}\nconst y = A;\n")]);
        let mut editor = Editor::connect(root);
        let initialized = editor.initialize(json!({})).await;
        assert!(initialized["result"]["capabilities"]["semanticTokensProvider"]["full"].as_bool().unwrap());
        let a = editor.uri("def/a.lfy");
        let answer = editor.request("textDocument/semanticTokens/full", json!({ "textDocument": { "uri": a } })).await;
        // A at 0:2 (data, declaration+agentic), y at 1:6 (variable, declaration+readonly), A at 1:10 (data, agentic).
        assert_eq!(answer["result"]["data"], json!([0, 2, 1, 1, 0b11, 1, 6, 1, 9, 0b101, 0, 4, 1, 1, 0b10]));
        editor.exit(true).await;
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:446bf5e30c867947956aee2a2210a127e47cae9b58d4270c7a32efdcfe1c8fa6
    // @lfy def/lsp/main.lfy:serve#serve:serve:b9952d80447661b1f4bc21b235257faeaaf5e82545ead260b127d1699140b876
    // @lfy def/lsp/main.lfy:serve#serve:serve:ec02e46e24878bf02d348987ba906b6878e72ff6ddb4b9955a57849c7eeefb33
    // @lfy def/lsp/main.lfy:serve#serve:serve:d71245c327c27c0b7ed7588fd887ae9bdf7afc23a61fec09deb75bd6fce23fbb
    #[tokio::test]
    async fn watched_files_are_registered_and_the_disk_is_read_again() {
        let root = fixture(&[
            ("def/a.lfy", "d A {}\n"),
            ("def/b.lfy", "use \"./a\";\nconst y = A;\n"),
        ]);
        let mut editor = Editor::connect(root);
        editor
            .initialize(
                json!({ "workspace": { "didChangeWatchedFiles": { "dynamicRegistration": true } } }),
            )
            .await;
        let a = editor.uri("def/a.lfy");
        let b = editor.uri("def/b.lfy");

        // Every `.lfy` file and every `elfie.json` under the root is watched.
        let registration = editor
            .wait_for(|message| message["method"] == "client/registerCapability")
            .await;
        let first = &registration["params"]["registrations"][0];
        assert_eq!(first["method"], "workspace/didChangeWatchedFiles");
        let globs: Vec<&str> = first["registerOptions"]["watchers"]
            .as_array()
            .expect("watchers")
            .iter()
            .map(|watcher| watcher["globPattern"].as_str().expect("a glob"))
            .collect();
        assert_eq!(globs, vec!["**/*.lfy", "**/elfie.json"]);
        let answered = json!({ "jsonrpc": "2.0", "id": registration["id"].clone(), "result": null });
        editor.send(answered).await;

        // After initialized, every file of the program was published, each of these two with
        // no problems to report.
        // @lfy def/lsp/main.lfy:serve#serve:serve:515f9ffc52eafb3e6dea5e0d8887a58b960cabe1e7f8d580cdbebfa1a13c69aa
        for file in ["def/a.lfy", "def/b.lfy"] {
            let published = editor.published_for(file).await;
            assert_eq!(published["params"]["diagnostics"], json!([]), "{file}");
        }

        // The open text is what is bound; closing the document reads the disk again.
        editor
            .notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": b.clone(), "languageId": "elfie", "version": 1, "text": "const y = zzz;\n" } }),
            )
            .await;
        let published = editor.published_for("def/b.lfy").await;
        assert!(reported(&published, "zzz"), "{published}");
        editor
            .notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": b.clone() } }),
            )
            .await;
        let published = editor.published_for("def/b.lfy").await;
        assert_eq!(published["params"]["diagnostics"], json!([]));

        // A watched `.lfy` file that is not open changed on disk.
        fs::write(editor.root.join("def/a.lfy"), "d C {}\n").unwrap();
        editor
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({ "changes": [{ "uri": a, "type": 2 }] }),
            )
            .await;
        let published = editor.published_for("def/b.lfy").await;
        assert!(reported(&published, "A is not declared"), "{published}");

        // `elfie.json` changed: the workspace is loaded again, so the new `def/a.lfy` is read,
        // and every open document is applied to it, so the error comes from the open text.
        editor
            .notify(
                "textDocument/didOpen",
                json!({ "textDocument": { "uri": b, "languageId": "elfie", "version": 2, "text": "use \"./a\";\nconst y = C;\n" } }),
            )
            .await;
        let published = editor.published_for("def/b.lfy").await;
        assert_eq!(published["params"]["diagnostics"], json!([]));
        fs::write(editor.root.join("def/a.lfy"), "d A {}\n").unwrap();
        let manifest = editor.uri(MANIFEST);
        editor
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({ "changes": [{ "uri": manifest, "type": 2 }] }),
            )
            .await;
        let published = editor.published_for("def/b.lfy").await;
        assert!(reported(&published, "C is not declared"), "{published}");

        assert_eq!(editor.exit(true).await, 0);
    }
}
