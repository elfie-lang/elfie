//! Compiled from `def/lsp/main.lfy`: the Elfie language server.
//!
//! [`serve`] speaks the Language Server Protocol over standard input and output for one
//! editor until it shuts the server down. Every feature is one query of
//! [`elfie_core::query`] over the [`Session`]'s workspace; the server converts document
//! URIs to workspace paths and protocol positions to token positions at its edge, and
//! nothing else. The protocol plumbing is the `tower-lsp` library, the external
//! declaration `Protocol` of the definition.
// @lfy def/lsp/main.lfy:11

use std::collections::{BTreeMap, HashMap};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use elfie_core::format;
use elfie_core::model::Criterion;
use elfie_core::query::{self, Position, Range, SemanticToken, TokenModifier, TokenType};
use elfie_core::workspace;
use tokio::io::{AsyncRead, AsyncWrite};
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types as lsp;
use tower_lsp::lsp_types::Url;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod data; // @lfy def/lsp/main.lfy:6

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
/// Requests are answered in the order received; a request that fails inside the server
/// yields an error response and the server goes on. Returns 0 when exit follows shutdown
/// and 1 when exit arrives without it.
// @lfy def/lsp/main.lfy:13
pub fn serve(root: Option<&Path>) -> i32 {
    let root = root.map(Path::to_path_buf);
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("elfie lsp: the runtime could not start: {error}"); // @lfy def/lsp/main.lfy:15
            return 1;
        }
    };
    runtime.block_on(serve_on(tokio::io::stdin(), tokio::io::stdout(), root))
}

/// [`serve`] over any pair of streams, so tests can drive the whole server in memory.
// @lfy def/lsp/main.lfy:13
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
    // @lfy def/lsp/main.lfy:16
    Server::new(input, output, socket)
        .concurrency_level(1)
        .serve(service)
        .await;
    exit_code(shared.shut_down.load(Ordering::SeqCst))
}

/// The exit code: 0 when exit follows shutdown, 1 when exit arrives without it.
// Decision: the input stream ending without an exit notification is treated as exit, so
// an editor that dies leaves the same code its exit would have.
// @lfy def/lsp/main.lfy:17
fn exit_code(shut_down: bool) -> i32 {
    if shut_down { 0 } else { 1 }
}

/// The language server: one editor's [`Session`] behind the protocol.
// @lfy def/lsp/main.lfy:13
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
    // @lfy def/lsp/main.lfy:16
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
    // @lfy def/lsp/main.lfy:33
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
    // @lfy def/lsp/main.lfy:31
    fn publish_later(&self, generation: u64) {
        let shared = self.shared.clone();
        let client = self.client.clone();
        tokio::spawn(async move { publish(shared, client, generation).await });
    }

    /// Registers for watched file changes to every `.lfy` file and to `elfie.json` under
    /// the root.
    // Decision: `elfie.json` is watched at every depth, not only at the root, because a
    // package's manifest inside the root changes the program just as the root's does.
    // @lfy def/lsp/main.lfy:24
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
    // @lfy def/lsp/main.lfy:27
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
/// published, and as empty for a file that left the program. A publication whose
/// generation is no longer the latest is dropped, so only the latest workspace is
/// published.
// @lfy def/lsp/main.lfy:31
async fn publish(shared: Arc<Shared>, client: Client, generation: u64) {
    let mut published = shared.published.lock().await;
    // @lfy def/lsp/main.lfy:32
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
        let current = by_file(query::diagnostics_of(&session.workspace, None));
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
    /// The session's workspace is `load` of the root given to `serve`, or of the client's
    /// first workspace folder, or of the current directory, in that order of availability;
    /// the encoding is utf-8 when the client offers it, utf-16 otherwise, and the response
    /// says which; the response advertises hover, definition, references, completion,
    /// rename with prepare, document symbols, workspace symbols, document formatting, and
    /// full text document synchronization.
    // @lfy def/lsp/main.lfy:20
    async fn initialize(&self, params: lsp::InitializeParams) -> Result<lsp::InitializeResult> {
        // Decision: the deprecated `rootUri` is consulted after the workspace folders and
        // before the current directory, because older clients send nothing else.
        // @lfy def/lsp/main.lfy:21
        let root = self
            .shared
            .root
            .clone()
            .or_else(|| {
                params
                    .workspace_folders
                    .as_ref()?
                    .first()?
                    .uri
                    .to_file_path()
                    .ok()
            })
            .or_else(|| root_uri(&params)?.to_file_path().ok())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let root = std::path::absolute(&root).unwrap_or(root);
        // @lfy def/lsp/main.lfy:22
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
            capabilities: capabilities(encoding), // @lfy def/lsp/main.lfy:23
            server_info: Some(lsp::ServerInfo {
                name: "elfie".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    /// After initialized, diagnostics are published for every file of the program, and
    /// the server registers for watched file changes.
    // @lfy def/lsp/main.lfy:24
    async fn initialized(&self, _: lsp::InitializedParams) {
        if self.shared.dynamic_watch.load(Ordering::SeqCst) {
            self.register_watchers();
        }
        let generation = self.bump();
        self.publish_later(generation);
    }

    // @lfy def/lsp/main.lfy:17
    async fn shutdown(&self) -> Result<()> {
        self.shared.shut_down.store(true, Ordering::SeqCst);
        Ok(())
    }

    // @lfy def/lsp/main.lfy:27
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

    // @lfy def/lsp/main.lfy:27
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
    // @lfy def/lsp/main.lfy:28
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
    // @lfy def/lsp/main.lfy:29
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
                    reload = true; // @lfy def/lsp/main.lfy:30
                } else if path.ends_with(EXTENSION) && session.document(&path).is_none() {
                    changed.push(path); // @lfy def/lsp/main.lfy:29
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
    /// reading its situations then its behaviors.
    // @lfy def/lsp/main.lfy:41
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
    // @lfy def/lsp/main.lfy:42
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
    // @lfy def/lsp/main.lfy:43
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
    // @lfy def/lsp/main.lfy:44
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
                        kind: Some(completion_kind(&completion.kind)),
                        detail: completion.detail,
                        ..Default::default()
                    })
                    .collect(),
            ))
        })
    }

    /// The range of the token under the position when `symbolAt` finds a symbol, and an
    /// error saying nothing is declared here otherwise.
    // @lfy def/lsp/main.lfy:45
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

    /// `renameAt` as one workspace edit with the edits grouped by file, or an error
    /// response carrying the reason it returned.
    // @lfy def/lsp/main.lfy:46
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

    /// `outlineOf` nested as the protocol's document symbols, with kinds mapped as
    /// completion maps them.
    // @lfy def/lsp/main.lfy:47
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
    // @lfy def/lsp/main.lfy:48
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

    /// One edit replacing the whole document with `format` of its tree when that differs,
    /// no edits when it is the same, and no edits with a log message when the tree has
    /// errors.
    // @lfy def/lsp/main.lfy:49
    /// `semanticTokensOf` encoded in the protocol's relative form.
    // @lfy def/lsp/main.lfy:56
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

/// The deprecated `rootUri` of an initialize request.
#[allow(deprecated)]
fn root_uri(params: &lsp::InitializeParams) -> Option<&Url> {
    params.root_uri.as_ref()
}

/// The encoding agreed at initialization: utf-8 when the client offers it, utf-16
/// otherwise.
// @lfy def/lsp/main.lfy:22
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
// @lfy def/lsp/main.lfy:23
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
        // @lfy def/lsp/main.lfy:24
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
// @lfy def/lsp/main.lfy:25
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
// @lfy def/lsp/main.lfy:56
fn encode_tokens(tokens: &[SemanticToken], protocol: &[lsp::Range]) -> Vec<lsp::SemanticToken> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut previous_line = 0;
    let mut previous_start = 0;
    for (token, range) in tokens.iter().zip(protocol) {
        let line = range.start.line;
        let start = range.start.character;
        let delta_line = line - previous_line;
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
// @lfy def/lsp/main.lfy:37
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
// @lfy def/lsp/main.lfy:37
fn uri_of(root: &Path, path: &str) -> Option<Url> {
    Url::from_file_path(root.join(path)).ok()
}

/// The count of code units of an encoding in the characters before a column of a line;
/// one unit per character past the end of the line.
// @lfy def/lsp/main.lfy:38
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
// @lfy def/lsp/main.lfy:38
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
// @lfy def/lsp/main.lfy:27
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
// @lfy def/lsp/main.lfy:38
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

    // @lfy def/lsp/main.lfy:38
    fn protocol_position(&mut self, session: &Session, file: &str, at: Position) -> lsp::Position {
        let line = at.line.saturating_sub(1);
        let encoding = self.encoding;
        let text = self.line(session, file, line);
        lsp::Position {
            line: line as u32,
            character: to_units(encoding, text, at.column) as u32,
        }
    }

    // @lfy def/lsp/main.lfy:38
    fn query_position(&mut self, session: &Session, file: &str, at: lsp::Position) -> Position {
        let encoding = self.encoding;
        let text = self.line(session, file, at.line as usize);
        Position {
            line: at.line as usize + 1,
            column: from_units(encoding, text, at.character as usize),
        }
    }

    /// Every range sent is a `Range` converted this way.
    // @lfy def/lsp/main.lfy:39
    fn range(&mut self, session: &Session, range: &Range) -> lsp::Range {
        lsp::Range {
            start: self.protocol_position(session, &range.file, range.start),
            end: self.protocol_position(session, &range.file, range.end),
        }
    }

    // @lfy def/lsp/main.lfy:39
    fn location(&mut self, session: &Session, range: &Range) -> Option<lsp::Location> {
        let uri = uri_of(&session.workspace.root, &range.file)?;
        Some(lsp::Location {
            uri,
            range: self.range(session, range),
        })
    }

    // @lfy def/lsp/main.lfy:31
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

/// Diagnostics by file, in the order the query gave them.
fn by_file(diagnostics: Vec<query::Diagnostic>) -> BTreeMap<String, Vec<query::Diagnostic>> {
    let mut grouped: BTreeMap<String, Vec<query::Diagnostic>> = BTreeMap::new();
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
// @lfy def/lsp/main.lfy:31
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
/// situations then its behaviors.
// @lfy def/lsp/main.lfy:41
fn hover_markdown(hover: &query::Hover) -> String {
    let mut sections = Vec::new();
    let mut code = format!("{} {}", hover.kind, hover.identifier);
    if let Some(ty) = &hover.ty {
        code.push_str(": ");
        code.push_str(ty);
    }
    sections.push(format!("```elfie\n{code}\n```"));
    if let Some(definition) = hover.definition.as_deref().filter(|text| !text.is_empty()) {
        sections.push(definition.to_string());
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
// @lfy def/lsp/main.lfy:41
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
// @lfy def/lsp/main.lfy:44
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

/// A symbol kind mapped as completion maps them.
// Decision: the protocol has no Text or Keyword symbol kind, so what completion maps to
// Text or Keyword is a String symbol.
// @lfy def/lsp/main.lfy:47
fn symbol_kind(kind: &str) -> lsp::SymbolKind {
    match kind {
        "path" => lsp::SymbolKind::FILE,
        "data" | "type" => lsp::SymbolKind::STRUCT,
        "trait" => lsp::SymbolKind::INTERFACE,
        "enum" => lsp::SymbolKind::ENUM,
        "function" | "agentFunction" => lsp::SymbolKind::FUNCTION,
        "member" => lsp::SymbolKind::FIELD,
        "variable" | "parameter" => lsp::SymbolKind::VARIABLE,
        "module" => lsp::SymbolKind::MODULE,
        _ => lsp::SymbolKind::STRING,
    }
}

/// An outline entry nested as a document symbol.
// @lfy def/lsp/main.lfy:47
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
        kind: symbol_kind(&outline.kind),
        tags: None,
        deprecated: None,
        range: converter.range(session, &outline.range),
        selection_range: converter.range(session, &outline.selection_range),
        children: (!children.is_empty()).then_some(children),
    }
}

/// A flattened outline entry as a workspace symbol.
// @lfy def/lsp/main.lfy:48
#[allow(deprecated)]
fn symbol_information(outline: &query::Outline, location: lsp::Location) -> lsp::SymbolInformation {
    lsp::SymbolInformation {
        name: outline.name.clone(),
        kind: symbol_kind(&outline.kind),
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

    // @lfy def/lsp/main.lfy:38
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

    // @lfy def/lsp/main.lfy:38
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

    // @lfy def/lsp/main.lfy:27
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

    // @lfy def/lsp/main.lfy:37
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

    // @lfy def/lsp/main.lfy:22
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

    // @lfy def/lsp/main.lfy:41
    #[test]
    fn hover_reads_code_line_definition_documentation_then_criteria() {
        let hover = query::Hover {
            range: Range::empty("def/a.lfy", Position::new(1, 0)),
            kind: "data".to_string(),
            identifier: "A".to_string(),
            definition: Some("An A".to_string()),
            ty: Some("Base".to_string()),
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
            "```elfie\ndata A: Base\n```\n\nAn A\n\nDoc line\n\n- It rains: Stay in; Read\n- Always"
        );
        let bare = query::Hover {
            definition: None,
            ty: None,
            documentation: Some(String::new()),
            criteria: Vec::new(),
            ..hover
        };
        assert_eq!(hover_markdown(&bare), "```elfie\ndata A\n```");
    }

    // @lfy def/lsp/main.lfy:44
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
        assert_eq!(symbol_kind("data"), lsp::SymbolKind::STRUCT);
        assert_eq!(symbol_kind("trait"), lsp::SymbolKind::INTERFACE);
        assert_eq!(symbol_kind("enum"), lsp::SymbolKind::ENUM);
        assert_eq!(symbol_kind("function"), lsp::SymbolKind::FUNCTION);
        assert_eq!(symbol_kind("member"), lsp::SymbolKind::FIELD);
        assert_eq!(symbol_kind("variable"), lsp::SymbolKind::VARIABLE);
        assert_eq!(symbol_kind("module"), lsp::SymbolKind::MODULE);
        assert_eq!(symbol_kind("alias"), lsp::SymbolKind::STRING);
    }

    // @lfy def/lsp/main.lfy:31
    #[test]
    fn only_files_whose_diagnostics_changed_are_published() {
        let diagnostic = |file: &str, line: usize, message: &str| query::Diagnostic {
            range: Range::empty(file, Position::new(line, 0)),
            severity: query::Severity::Error,
            stage: query::Stage::Binder,
            message: message.to_string(),
        };
        let published = by_file(vec![
            diagnostic("def/a.lfy", 1, "one"),
            diagnostic("def/b.lfy", 1, "two"),
            diagnostic("def/gone.lfy", 1, "three"),
        ]);
        let current = by_file(vec![
            diagnostic("def/a.lfy", 1, "one"),
            diagnostic("def/b.lfy", 2, "two"),
            diagnostic("def/new.lfy", 1, "four"),
        ]);
        let delta = diagnostics_delta(&published, &current);
        let files: Vec<(&str, usize)> = delta
            .iter()
            .map(|(file, diagnostics)| (file.as_str(), diagnostics.len()))
            .collect();
        assert_eq!(
            files,
            vec![("def/b.lfy", 1), ("def/new.lfy", 1), ("def/gone.lfy", 0)]
        );
        assert!(diagnostics_delta(&current, &current).is_empty());
    }

    // @lfy def/lsp/main.lfy:17
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

        /// Sends a request and answers with its response.
        async fn request(&mut self, method: &str, params: Value) -> Value {
            self.next_id += 1;
            let id = self.next_id;
            let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
            if !params.is_null() {
                message["params"] = params;
            }
            self.send(message).await;
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

    // @lfy def/lsp/main.lfy:51
    #[tokio::test]
    async fn a_binder_error_is_published_after_initialized_and_exit_after_shutdown_is_zero() {
        let root = fixture(&[("def/a.lfy", "const y = z;\n")]);
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

    // @lfy def/lsp/main.lfy:17
    #[tokio::test]
    async fn exit_without_shutdown_is_one() {
        let root = fixture(&[("def/a.lfy", "d A {}\n")]);
        let mut editor = Editor::connect(root);
        editor.initialize(json!({})).await;
        assert_eq!(editor.exit(false).await, 1);
    }

    // @lfy def/lsp/main.lfy:55
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

    // @lfy def/lsp/main.lfy:49
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

    // @lfy def/lsp/main.lfy:56
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
        assert_eq!(flat, vec![(0, 2, 1, 1, 0b11), (0, 6, 1, 9, 1 << 4), (2, 6, 1, 8, 0)]);
        let legend = legend();
        assert_eq!(legend.token_types[1].as_str(), "data");
        assert_eq!(legend.token_types.len(), 10);
        assert_eq!(legend.token_modifiers[4].as_str(), "scope");
    }

    // @lfy def/lsp/main.lfy:24
    #[tokio::test]
    async fn semantic_tokens_are_served_for_a_document() {
        let root = fixture(&[("def/a.lfy", "d A {}\nconst y = A;\n")]);
        let mut editor = Editor::connect(root);
        let initialized = editor.initialize(json!({})).await;
        assert!(initialized["result"]["capabilities"]["semanticTokensProvider"]["full"].as_bool().unwrap());
        let a = editor.uri("def/a.lfy");
        let answer = editor.request("textDocument/semanticTokens/full", json!({ "textDocument": { "uri": a } })).await;
        // A at 0:2 (data, declaration+agentic), y at 1:6 (variable, declaration+readonly), A at 1:10 (data, agentic).
        assert_eq!(answer["result"]["data"], json!([0, 2, 1, 1, 0b11, 1, 6, 1, 8, 0b101, 0, 4, 1, 1, 0b10]));
        editor.exit(true).await;
    }
}
