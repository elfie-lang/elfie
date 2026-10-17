//! The tests of `serve`: its pure parts, and its lifecycle driven over the protocol in
//! memory.
// @lfy def/lsp/main.lfy:serve

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use elfie_core::model::{Criterion, SymbolKind};
use elfie_core::query::{self, Position, Range, SemanticToken, TokenModifier, TokenType};
use elfie_core::{format, workspace};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tower_lsp::lsp_types as lsp;
use tower_lsp::lsp_types::Url;

use crate::convert::{
    Converter, apply_change, completion_kind, end_of, from_units, hover_markdown, relative_path,
    symbol_kind, to_units, uri_of,
};
use crate::server::{
    by_file, chosen_root, diagnostics_delta, encode_tokens, exit_code, legend, negotiate, serve_on,
};
use crate::{Document, Encoding, MANIFEST, NOTHING_DECLARED, Session};

// Pure parts.

// @lfy def/lsp/main.lfy:serve#serve:serve:4eb913bc998b39b0bc4cd9e2a63d11b56c77c491c2198de374e643b5f6ff06a5
#[test]
fn columns_convert_to_code_units_and_back() {
    let line = "aé😀b";
    assert_eq!(to_units(Encoding::Utf8, line, 3), 1 + 2 + 4);
    assert_eq!(to_units(Encoding::Utf16, line, 3), 1 + 1 + 2);
    assert_eq!(from_units(Encoding::Utf8, line, 7), 3);
    assert_eq!(from_units(Encoding::Utf16, line, 4), 3);
    // Inside a character: the character's own column.
    assert_eq!(from_units(Encoding::Utf16, line, 3), 2);
    assert_eq!(from_units(Encoding::Utf8, line, 2), 1);
    // Past the end: one unit per character.
    assert_eq!(to_units(Encoding::Utf16, line, 6), 5 + 2);
    assert_eq!(from_units(Encoding::Utf16, line, 7), 6);
    assert_eq!(to_units(Encoding::Utf8, "", 0), 0);
    assert_eq!(from_units(Encoding::Utf8, "", 0), 0);
}

// @lfy def/lsp/main.lfy:serve#serve:serve:35a0718e0e91d6b10930384042a0fe513dbb8a7188bc60b57f614166263dd04f
// @lfy def/lsp/main.lfy:serve#serve:serve:4eb913bc998b39b0bc4cd9e2a63d11b56c77c491c2198de374e643b5f6ff06a5
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

// @lfy def/lsp/main.lfy:serve#serve:serve:6b4c40ea0834c4fb7d6b6e15c78c5b97f7d2a5d43c031c9e328542ac9fc2e737
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

// @lfy def/lsp/main.lfy:serve#serve:serve:0d7ad5672274c815b6ad256268ff29d8425eb7ed8114bb7c9ea52bd41bedde9b
// @lfy def/lsp/main.lfy:serve#serve:serve:aa86a5495219844e1ddc4aa1bfb0955b0cfae99f274a5906be3f96398622b07b
// @lfy def/lsp/main.lfy:serve#serve:serve:fa7d65433e4967e6668886caefd62bc4b421c62a2690c2abe9ff145233f53a4a
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

// @lfy def/lsp/main.lfy:serve#serve:serve:c82aa87a38da9f76f6a0282cf762d75fc1a996f6387757f554851aa77dc039f8
// @lfy def/lsp/main.lfy:serve#serve:serve:5f629591078b6612e51dff25a74ae0148764e1dd9d2ab2591e7da093f299a566
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
    assert_eq!(Encoding::Utf8.value(), "utf-8");
    assert_eq!(Encoding::Utf16.value(), "utf-16");
}

// @lfy def/lsp/main.lfy:serve#serve:serve:19386c9f8368f5aaddbde274c6c2dddb089872f7a9a314186a05cdd7a769b6e3
// @lfy def/lsp/main.lfy:serve#serve:serve:399bc8d9d0952a6a2bfeebf5c3967a47676bc36520e42a430b7035b01cc860f9
// @lfy def/lsp/main.lfy:serve#serve:serve:90077d5afc89f7b023508ef4944e41cc279601008d7a8e3460226b18cd4e5d84
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:399bc8d9d0952a6a2bfeebf5c3967a47676bc36520e42a430b7035b01cc860f9
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
// @lfy def/lsp/main.lfy:serve#serve:serve:b132c3ac7e22fcbfed3082c60d6be3e5fce613162d5fbecbc92f52cc490a1c1c
// @lfy def/lsp/main.lfy:serve#serve:serve:f8f2931d14d2b2a4e1c3b72f0562d356d5e993d089a8899c3712bc042902d862
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:f8f2931d14d2b2a4e1c3b72f0562d356d5e993d089a8899c3712bc042902d862
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:b132c3ac7e22fcbfed3082c60d6be3e5fce613162d5fbecbc92f52cc490a1c1c
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

// @lfy def/lsp/main.lfy:serve#serve:serve:799433db73b6bc8325515763025bff551e73ffe3ff0ee5003d855898d90a732e
// @lfy def/lsp/main.lfy:serve#serve:serve:98094be11487918342040661be4f07e244aca47c388ac7567c98f7fd986d0e72
// @lfy def/lsp/main.lfy:serve#serve:serve:f8f2931d14d2b2a4e1c3b72f0562d356d5e993d089a8899c3712bc042902d862
// @lfy def/lsp/main.lfy:serve#serve:serve:4dfeb9815a7448555bae335da2f77b2f7e33e4ca1b7dc7432e3b0c8c4417afd3
// @lfy def/lsp/main.lfy:serve#serve:serve:707ab7a19774623cb46e195663db4cd66b51d81e6762e0089651d25391104748
// @lfy def/lsp/main.lfy:serve#serve:serve:f33fa650fd2296b8e1b899bbc1c94149ffc4905c1be5d32f40ed7b204e1bab6b
// @lfy def/lsp/main.lfy:serve#serve:serve:c82aa87a38da9f76f6a0282cf762d75fc1a996f6387757f554851aa77dc039f8
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:f8f2931d14d2b2a4e1c3b72f0562d356d5e993d089a8899c3712bc042902d862
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
// @lfy def/lsp/main.lfy:serve#serve:serve:7773981fce7710813ac48558af1654223837f348144a83f759bd110529adc40d
// @lfy def/lsp/main.lfy:serve#serve:serve:c27811cfbd180c4e28720d3291fd35fc8c1a18495d1de90534e7a92078e2300a
// @lfy def/lsp/main.lfy:serve#serve:serve:b57406ccac6e090310b7b7f7e364fe74fc203a3a4c7efce6f2ea07443fdcc6e3
// @lfy def/lsp/main.lfy:serve#serve:serve:f9aa1e1b9148be162e7cbcc4ab2175dbea6ce7907b2f1690eda80d80a689f4ff
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
            .wait_for(|message| message.get("id").is_some() && message.get("method").is_none())
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

// @lfy def/lsp/main.lfy:serve#serve:serve:dd61686d17ea6b5c7edafd85d9b3f1f435cb20bcd3de229295f981606a7b37bc
// @lfy def/lsp/main.lfy:serve#serve:serve:37382c32c2e5d125cef7e8f2a6960e4c8fa8bfce5a4499fcee1b8332af049767
// @lfy def/lsp/main.lfy:serve#serve:serve:36af53505133b481b44d4927a2dd8ef3b7b9cb4c9fc338923acf2b52c83b993c
// @lfy def/lsp/main.lfy:serve#serve:serve:cb81d8e3f0b6567ed41dc152db804c5f411d48090a803d5af4c3a3b6b6bac767
// @lfy def/lsp/main.lfy:serve#serve:serve:e3461a08571790d904995f24b1ab330dd9e4038aa3e11df19c4dad085e9ddb3d
// @lfy def/lsp/main.lfy:serve#serve:serve:3998738246ab112a54cafc42192f647ffab183141e3fd8f3f1e5dc82409c7dc9
// @lfy def/lsp/main.lfy:serve#serve:serve:19386c9f8368f5aaddbde274c6c2dddb089872f7a9a314186a05cdd7a769b6e3
// @lfy def/lsp/main.lfy:serve#serve:serve:2d7434ceacaf117b84a50944d6defac63a931122b09e46f84d1972331095c38d
// @lfy def/lsp/main.lfy:serve#serve:serve:03938d8acea19d9cedc01076466a3b427d91282d72a892fe54dfdd76f1b12547
// @lfy def/lsp/main.lfy:serve#serve:serve:665d248480e23131af33d3302ca360af76f5969fa2b509691c5708e2e4a742da
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
    let at = json!({ "textDocument": { "uri": b }, "position": { "line": 1, "character": 10 } });

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
    // @lfy def/lsp/main.lfy:serve#serve:serve:353c36b50bff92902b8106ae8ad5aa2177f9be6dc03e90b2e62c937373fd1175
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

// @lfy def/lsp/main.lfy:serve#serve:serve:6b702869304d1406c4c91d62256f0e00c22dfe7bc63714e4e23aa92679e4b4b3
// @lfy def/lsp/main.lfy:serve#serve:serve:8515860f3ec119e6aa9375f57168b7240d07e511f2eb7399afae8b3eca6b3a2a
// @lfy def/lsp/main.lfy:serve#serve:serve:28700f9f7649b38774e5a69d7bb8341848a793dbcc8fca77e9a8f5f6a04c3a64
// @lfy def/lsp/main.lfy:serve#serve:serve:777a57de3910fb244406238be7543474c340ffdbe2b0949fec10958ba5478dc1
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
// @lfy def/lsp/main.lfy:serve#serve:serve:b8565649d4ab1cf2d67a413d0fe35867a70a2cb423e57de6e2104a415cb6e951
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:ce4fd9bcf886d04552fa421a2b3c1b42b5cf8a561a7c82e4b934e4741ab52f0a
    // @lfy def/lsp/main.lfy:serve#serve:serve:5c11eec6ca2131eeb40d9fe0e01d7330d8c6bb85d9ca32ac16990413db81067c
    assert_eq!(flat, vec![(0, 2, 1, 1, 0b11), (0, 6, 1, 10, 1 << 4), (2, 6, 1, 9, 0)]);
    let legend = legend();
    assert_eq!(legend.token_types[1].as_str(), "data");
    assert_eq!(legend.token_types[8].as_str(), "typeParameter");
    assert_eq!(legend.token_types.len(), 11);
    assert_eq!(legend.token_modifiers[4].as_str(), "scope");
}

// @lfy def/lsp/main.lfy:serve#serve:serve:f60fe367b8c7bc62490da190f96e531eded102a1ed95a0c464d889b0599a46df
// @lfy def/lsp/main.lfy:serve#serve:serve:707ab7a19774623cb46e195663db4cd66b51d81e6762e0089651d25391104748
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

// @lfy def/lsp/main.lfy:serve#serve:serve:c9d12ec7a2554d47185822774257c9f0ebca78f272521b359d6019998633926b
// @lfy def/lsp/main.lfy:serve#serve:serve:1800e8ae59684dee23afe3307a028fceb453dbcc3af7518f3d52a1793fa2cc8f
// @lfy def/lsp/main.lfy:serve#serve:serve:0b97846abe098e6a1fbfe6db8abe1bcff1075c32c70e67651fff16cca55c2362
// @lfy def/lsp/main.lfy:serve#serve:serve:7866f3607d52874f64675dcd40566e502c90ca8fbccd522413461b5fc6c884dd
// @lfy def/lsp/main.lfy:serve#serve:serve:5f45cd42b1869414e0293381505784b60b7c53b1f7e6e745865e038029ef42e0
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:f8f2931d14d2b2a4e1c3b72f0562d356d5e993d089a8899c3712bc042902d862
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

/// A document outside the root, and one under the root that is under none of the source
/// directories and is not used by a file in the program, each get one log message and nothing
/// else.
// @lfy def/lsp/main.lfy:serve#serve:serve:2372d03fd136ee56b409774c63c22c3dada21e5054e41ed2bc6e1727f8ae9b65
// @lfy def/lsp/main.lfy:serve#serve:serve:12e8b77a71fd33905b1e99773f5c572dd9412e25779a43d5d8c81a01bc33c738
// @lfy def/lsp/main.lfy:serve#serve:serve:204391a1b469a6c11eea3fb3f661b5e14a410c2340a7963424513539a0791f47
// @lfy def/lsp/main.lfy:serve#serve:serve:665d248480e23131af33d3302ca360af76f5969fa2b509691c5708e2e4a742da
#[tokio::test]
async fn a_document_the_program_does_not_hold_is_logged_and_gets_nothing() {
    let root = fixture(&[("def/a.lfy", "d A {}\n"), ("notes/loose.lfy", "d B {}\n")]);
    let mut editor = Editor::connect(root);
    editor.initialize(json!({})).await;
    let logged = |text: &'static str| {
        move |message: &Value| {
            message["method"] == "window/logMessage"
                && message["params"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(text))
        }
    };

    // Outside the root: one log message names the document and the root.
    // @lfy def/lsp/main.lfy:serve#serve:serve:2372d03fd136ee56b409774c63c22c3dada21e5054e41ed2bc6e1727f8ae9b65
    let outside =
        Url::from_file_path(editor.root.parent().unwrap().join("elfie-lsp-outside.lfy")).unwrap();
    editor
        .notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": outside, "languageId": "elfie", "version": 1, "text": "d C {}\n" } }),
        )
        .await;
    let message = editor.wait_for(logged("is outside the root")).await;
    let message = message["params"]["message"].as_str().unwrap().to_string();
    let root_name = editor.root.file_name().unwrap().to_string_lossy().into_owned();
    assert!(message.contains("elfie-lsp-outside.lfy"), "{message}");
    assert!(message.contains(&root_name), "{message}");
    assert!(message.contains("it gets nothing"), "{message}");

    // Under the root but in no file of the program: one log message names the document
    // and the source directories.
    // @lfy def/lsp/main.lfy:serve#serve:serve:12e8b77a71fd33905b1e99773f5c572dd9412e25779a43d5d8c81a01bc33c738
    let source_directories = workspace::load(&editor.root).source_directories;
    let loose = editor.uri("notes/loose.lfy");
    editor
        .notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": loose, "languageId": "elfie", "version": 1, "text": "d B {}\n" } }),
        )
        .await;
    let message = editor.wait_for(logged("used by no file in the program")).await;
    let message = message["params"]["message"].as_str().unwrap().to_string();
    assert!(message.contains("notes/loose.lfy"), "{message}");
    assert!(message.contains("under none of"), "{message}");
    assert!(!source_directories.is_empty());
    for source_directory in &source_directories {
        assert!(message.contains(source_directory.as_str()), "{message}");
    }
    assert!(message.contains("it gets nothing"), "{message}");

    // It gets no diagnostics, and a request on it answers as if nothing were there.
    // @lfy def/lsp/main.lfy:serve#serve:serve:204391a1b469a6c11eea3fb3f661b5e14a410c2340a7963424513539a0791f47
    // @lfy def/lsp/main.lfy:serve#serve:serve:665d248480e23131af33d3302ca360af76f5969fa2b509691c5708e2e4a742da
    let hover = editor
        .request(
            "textDocument/hover",
            json!({ "textDocument": { "uri": editor.uri("notes/loose.lfy") }, "position": { "line": 0, "character": 2 } }),
        )
        .await;
    assert_eq!(hover["result"], Value::Null);
    assert!(
        !editor
            .pending
            .iter()
            .any(|message| message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == json!(editor.uri("notes/loose.lfy").to_string()))
    );

    assert_eq!(editor.exit(true).await, 0);
}

/// A `$/cancelRequest` is ignored and the request it names is answered normally.
// @lfy def/lsp/main.lfy:serve#serve:serve:84d955371f5f011bf11dd22238bcbd23653d0c8794adb6cd27221db62c7ec47d
// @lfy def/lsp/main.lfy:serve#serve:serve:2f2c7ecf2aefa7b4950f022eda84b70f726e7b227d3c33dc71422bc342f64b55
#[tokio::test]
async fn a_cancelled_request_is_answered_anyway() {
    let root = fixture(&[("def/a.lfy", "d A {}\n")]);
    let mut editor = Editor::connect(root);
    editor.initialize(json!({})).await;
    let a = editor.uri("def/a.lfy");
    let id = editor
        .send_request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": a } }),
        )
        .await;
    editor.notify("$/cancelRequest", json!({ "id": id })).await;
    let response = editor
        .wait_for(|message| message.get("id") == Some(&json!(id)))
        .await;
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"][0]["name"], "A");
    assert_eq!(editor.exit(true).await, 0);
}
