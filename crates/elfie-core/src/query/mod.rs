//! Compiled from `def/query/main.lfy`: one query surface over a bound [`Workspace`].
//!
//! Both the language server and the agent server call these queries; editors and agents
//! differ only in how they spell a position or a name, so each query takes either a
//! position or a name and nothing protocol-shaped. Positions are spelled exactly as tokens
//! spell them (line from 1, column from 0, counted in characters); no query converts.
//!
//! The queries themselves live in the submodules below, one per concern, and are re-exported
//! here, so every path by which another unit reached a query still reaches it.

// The directory holds this module file and the output of `def/query/data.lfy`, which it
// declares as a submodule; the declaration comes from the layout, not from an entity.
pub mod data;

pub use data::*;

mod completion;
mod diagnostics;
mod find;
mod hover;
mod outline;
mod rename;
mod semantic_tokens;
mod symbols;

pub use completion::{CompletionContext, completions_at};
pub use diagnostics::diagnostics_of;
pub use find::{find, source_of};
pub use hover::{hover_at, hover_of, type_text_of};
pub use outline::{find_symbols, outline_of};
pub use rename::rename_at;
pub use semantic_tokens::semantic_tokens_of;
pub use symbols::{definition_of, references_to, symbol_at};

use crate::grammar::Entity as Rule;
use crate::lexer::Token;
use crate::model::{
    self, EntityId, EntityKind, FileId, Model, NodeRef, Origin, SymbolId, SymbolKind,
};
use crate::parser::Node;
use crate::workspace::Workspace;

/// What [`range_of`] takes: a node of a file's tree, or a token by its index into the
/// file's tokens.
// Decision: a token is given by its index because the index is how a tree refers to its
// tokens, and a node by reference because nodes are not copied out of the tree.
// [`range_of_node`] takes the model's `NodeRef` instead.
#[derive(Debug, Clone, Copy)]
pub enum NodeOrToken<'a> {
    Node(&'a Node),
    Token(usize),
}

// ---- Tokens and positions ---------------------------------------------------------

/// The token of a file, by index.
pub fn token(workspace: &Workspace, file: FileId, index: usize) -> &Token {
    &workspace.model.sources[file].tree.tokens[index]
}

/// The tokens of a file.
fn tokens(workspace: &Workspace, file: FileId) -> &[Token] {
    &workspace.model.sources[file].tree.tokens
}

/// The file of the program at a path relative to `Workspace.root`, as `File.path` is.
fn file_of(workspace: &Workspace, path: &str) -> Option<FileId> {
    workspace.model.file(path)
}

/// The range of the tokens from `start` up to `end` of a file, as [`Range::of_tokens`]
/// gives it.
fn span(workspace: &Workspace, file: FileId, start: usize, end: usize) -> Range {
    let path = workspace.model.sources[file].path.as_str();
    Range::of_tokens(path, tokens(workspace, file), start, end)
}

/// Where a node or token sits.
///
/// `Range.file` is `Token.file` of the first token covered, `Range.start` is its line and
/// column, and `Range.end` is the position just after the last character of the last
/// token covered. A node covering no token gives an empty range at the position the node
/// would start at.
pub fn range_of(workspace: &Workspace, file: FileId, node: NodeOrToken<'_>) -> Range {
    match node {
        NodeOrToken::Node(node) => span(workspace, file, node.start, node.end),
        NodeOrToken::Token(index) => span(workspace, file, index, index + 1),
    }
}

/// [`range_of`] for a node the model refers to.
pub fn range_of_node(workspace: &Workspace, node: NodeRef) -> Range {
    let info = workspace.model.info(node);
    span(workspace, node.file, info.start, info.end)
}

/// Whether a node reference points into a file: `global` and its scope are synthetic.
fn is_real(model: &Model, node: NodeRef) -> bool {
    node.file < model.sources.len()
}

/// The token under a position, by index: [`Position::token_in`] over the tokens of a file
/// of the program. A file not in the program gives `None`.
pub fn token_at(workspace: &Workspace, file: &str, position: Position) -> Option<usize> {
    let file = file_of(workspace, file)?;
    position.token_in(tokens(workspace, file))
}

/// Every node of a file covering a token, outermost first; the last holds the token as a
/// direct child (or in an error node child).
fn nodes_covering(model: &Model, file: FileId, token: usize) -> Vec<NodeRef> {
    // Ancestors precede descendants in preorder, and the nodes covering one token are
    // all ancestors of one another.
    model.nodes[file]
        .iter()
        .enumerate()
        .filter(|(_, info)| info.start <= token && token < info.end)
        .map(|(index, _)| NodeRef { file, index })
        .collect()
}

/// Every node covering a position, outermost first: the root of the file's tree, then a
/// child of each node before it; the last holds [`token_at`] as a direct child. Empty
/// when [`token_at`] gives nothing.
pub fn nodes_at(workspace: &Workspace, file: &str, position: Position) -> Vec<NodeRef> {
    let Some(id) = file_of(workspace, file) else {
        return Vec::new();
    };
    match token_at(workspace, file, position) {
        Some(index) => nodes_covering(&workspace.model, id, index),
        None => Vec::new(),
    }
}

/// The child nodes of a node, in order, trivia included.
fn child_nodes(model: &Model, node: NodeRef) -> Vec<NodeRef> {
    model
        .node(node)
        .nodes()
        .filter_map(|child| model.node_ref(node.file, child))
        .collect()
}

/// The first node in a chain (outermost first) satisfying a rule.
fn find_rule(model: &Model, chain: &[NodeRef], rule: Rule) -> Option<NodeRef> {
    chain
        .iter()
        .copied()
        .find(|&node| model.info(node).rule == rule)
}

/// The position of a file in `Workspace.files`; after every file for a path not in it.
fn file_order(workspace: &Workspace, path: &str) -> usize {
    workspace
        .files
        .iter()
        .position(|file| file.path == path)
        .unwrap_or(usize::MAX)
}

/// The files of the program in the order `findSymbols` uses: files without a package
/// first, then package files, each in `Workspace.files` order.
// @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:2cafa010009d2c7783a291a1e27d35e09d9ef1f73122866e4953772fad3a0b32
// @lfy def/query/main.lfy:find#find:find:1286ad770a1cf3a25e55448d3cdfe6b64efc8a2a5c597ae69414751ffe012c2d
fn files_in_search_order(workspace: &Workspace) -> Vec<FileId> {
    let own = workspace.files.iter().filter(|file| file.package.is_none());
    let packaged = workspace.files.iter().filter(|file| file.package.is_some());
    own.chain(packaged).map(|file| file.source).collect()
}

// ---- Symbols ----------------------------------------------------------------------

/// Whether two symbols are the same declaration: a member declared in a trait is copied
/// into every entity that receives the trait, with the same entity, node, and name token.
fn same_declaration(model: &Model, a: SymbolId, b: SymbolId) -> bool {
    let (a, b) = (&model.symbols[a], &model.symbols[b]);
    a.entity == b.entity && a.node == b.node && a.name_token == b.name_token && a.kind == b.kind
}

/// Every usage of a symbol, copies of its declaration included, in node order.
// Decision: `usagesOf` takes one symbol, but a trait member is one declaration with a
// symbol in the trait's scope and a copy in every receiving entity's scope; a usage of
// any copy is a usage of the declaration.
fn usages_of_declaration(model: &Model, symbol: SymbolId) -> Vec<model::UsageId> {
    let mut out: Vec<model::UsageId> = (0..model.symbols.len())
        .filter(|&other| same_declaration(model, symbol, other))
        .flat_map(|other| model::usages_of(model, other))
        .collect();
    out.sort_by_key(|&id| (model.usages[id].node.file, model.usages[id].node.index));
    out.dedup();
    out
}

// ---- The prelude ------------------------------------------------------------------

/// The data the prelude declares under a name, when the program has a prelude.
// Decision: the binder reads the prelude through its own scope, so the queries find it
// the same way: the file scope of the one source of [`Origin::Prelude`].
// @lfy def/query/main.lfy:completionsAt
fn prelude_data(model: &Model, name: &str) -> Option<EntityId> {
    let file = model
        .sources
        .iter()
        .position(|source| source.origin == Origin::Prelude)?;
    let symbol = model.lookup_local(model.file_scopes[file], name)?;
    let entity = model.symbols[symbol].entity;
    matches!(model.entities[entity].kind, EntityKind::Data).then_some(entity)
}

/// The members an entity's scope declares: what the scope layer reaches.
fn member_symbols(model: &Model, entity: EntityId) -> Vec<SymbolId> {
    model
        .members(entity)
        .into_iter()
        .filter(|&symbol| {
            matches!(
                model.symbols[symbol].kind,
                SymbolKind::Member | SymbolKind::EnumMember
            )
        })
        .collect()
}

/// The symbols a file's scope declares itself: what a module offers.
fn module_symbols(model: &Model, file: FileId) -> Vec<SymbolId> {
    model.scopes[model.file_scopes[file]].declared().collect()
}

/// The kind of an entity: the kind of its symbol, and `member` when it is a member.
// Decision: only the anonymous entity of a file declares no symbol of its own, and a file
// is what a module symbol names, so it is reported as a module.
// @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:bc58dcd08b279e454ea3f234d6633930816ba962b653ce118f411ca600d35815
fn kind_of(model: &Model, entity: EntityId) -> SymbolKind {
    let e = &model.entities[entity];
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:bc5317180748f756ff3ce1dc261bcaa95143513bb4264535b9448d06ee539dd5
    if matches!(e.kind, EntityKind::Member) {
        return SymbolKind::Member;
    }
    match e.symbol {
        Some(symbol) => model.symbols[symbol].kind,
        None => SymbolKind::Module,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::query::diagnostics::diagnostic_key;
    use crate::workspace;

    /// A project directory under the system's temporary directory, removed when dropped.
    pub(super) struct Fixture {
        pub(super) root: PathBuf,
    }

    /// What a fixture writes as `elfie.json`: the package `elfie` is the fixture's own
    /// `lib`, which holds nothing until a test fills it. A test that writes its own
    /// manifest names `"./lib"` the same way.
    const STUB_MANIFEST: &str = r#"{ "dependencies": { "elfie": "./lib" } }"#;

    impl Fixture {
        pub(super) fn new() -> Fixture {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "elfie-query-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("def")).unwrap();
            let fixture = Fixture { root };
            // Every fixture holds a library of its own, which its `elfie.json` names, so
            // that a test never reads the copy of the library the compiler was built
            // with; [`Fixture::with_prelude`] puts the real one in its place.
            // @lfy def/query/main.lfy:completionsAt
            fixture.write("elfie.json", STUB_MANIFEST);
            fixture.write("lib/main.lfy", "");
            fixture
        }

        /// A project with one file `def/a.lfy`.
        pub(super) fn one(text: &str) -> Fixture {
            let fixture = Fixture::new();
            fixture.write("def/a.lfy", text);
            fixture
        }

        /// A project with one file `def/a.lfy` and the project's own `lib` as the
        /// package `elfie`, so that it has the prelude every program has.
        // @lfy def/query/main.lfy:completionsAt
        pub(super) fn with_prelude(text: &str) -> Fixture {
            let fixture = Fixture::one(text);
            fixture.write("elfie.json", STUB_MANIFEST);
            let library = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../lib"));
            fixture.copy(library, "lib");
            fixture
        }

        /// Every `.lfy` file under a directory, copied in under a path of the fixture.
        fn copy(&self, from: &Path, to: &str) -> &Fixture {
            for entry in fs::read_dir(from).unwrap().filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = format!("{to}/{name}");
                if entry.path().is_dir() {
                    self.copy(&entry.path(), &path);
                } else if name.ends_with(".lfy") {
                    self.write(&path, &fs::read_to_string(entry.path()).unwrap());
                }
            }
            self
        }

        pub(super) fn write(&self, path: &str, text: &str) -> &Fixture {
            let disk = self.root.join(path);
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(disk, text).unwrap();
            self
        }

        pub(super) fn load(&self) -> Workspace {
            workspace::load(&self.root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    pub(super) const A: &str = "def/a.lfy";

    pub(super) fn at(line: usize, column: usize) -> Position {
        Position::new(line, column)
    }

    /// A position some columns after another on the same line.
    pub(super) fn after(position: Position, columns: usize) -> Position {
        Position::new(position.line, position.column + columns)
    }

    /// The position of the `occurrence`th (from 0) occurrence of `needle` in `text`.
    pub(super) fn find_pos(text: &str, needle: &str, occurrence: usize) -> Position {
        let mut from = 0;
        let mut index = None;
        for _ in 0..=occurrence {
            let found = text[from..].find(needle).expect("needle") + from;
            index = Some(found);
            from = found + needle.len();
        }
        let index = index.unwrap();
        let before = &text[..index];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().unwrap().chars().count();
        Position::new(line, column)
    }

    pub(super) fn range(file: &str, start: (usize, usize), end: (usize, usize)) -> Range {
        Range {
            file: file.to_string(),
            start: at(start.0, start.1),
            end: at(end.0, end.1),
        }
    }

    pub(super) fn labels(completions: &[Completion]) -> Vec<&str> {
        completions.iter().map(|c| c.label.as_str()).collect()
    }

    /// The repository itself: every definition file, bound with zero problems.
    fn repository() -> Workspace {
        workspace::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")))
    }

    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:562b82e9b9bff42506308fce309460fd485f976fa4cbcd14307d4e86255c5b7a
    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:e38e58f0b0d00cd0e0b9b3322d0044fe9860e3725299a137a672be2af4bd7846
    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:2c61bf4cf3dcd2797fdc448c45a78d5542297f5ccda29af95840a2b68b5f011a
    // @lfy def/query/main.lfy:sourceOf#sourceOf:sourceOf:1231c1d038570bfc4ea96a2254b88772810a70da99d1226712806be3a3a548d9
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:bc58dcd08b279e454ea3f234d6633930816ba962b653ce118f411ca600d35815
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:bc5317180748f756ff3ce1dc261bcaa95143513bb4264535b9448d06ee539dd5
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:1d1e85c7ce3a70099ea62a09272e6712cbaa4302d4f684ea6effb157a8fa308b
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:365a6fefcaac6ce90927cbfb94ec3099fda7365d3d87ab047cc45048069b9974
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:7065c2f898fbfc8f0764635d4eda8bad0129e35dff4dbf77f81b25c2138162e5
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:215cb16ac34749dba8b39f3c4b0772c200e5c3bcd0f576c0a3c315cbf26dd26e
    // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:75e0f581b2c1e2e67024c96113d0bb4d0de9485bba0f4d0bc52668334b0aca46
    // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:3a7355b9e45ec3b575bc69fd4a7ce2bffafd0dcfd946c80bcf0c0783dc8c87be
    // @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:88e9675dd098a5864faa0fd7eadc96f57937a4b1389ee9fe2fde45430ea4af24
    // @lfy def/query/main.lfy:find#find:find:61607cb709e91135898032720f10dea2e214fb817b1408e0acc03b32ee2921dd
    #[test]
    fn the_repository_answers_every_query_without_errors() {
        let ws = repository();
        assert!(ws.files.len() >= 38, "{}", ws.files.len());
        let diagnostics = diagnostics_of(&ws, None);
        let errors: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        // Every diagnostic is in files order.
        let keys: Vec<_> = diagnostics.iter().map(|d| diagnostic_key(&ws, d)).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);

        let main = "def/query/main.lfy";
        let outline = outline_of(&ws, main);
        let names: Vec<&str> = outline.iter().map(|o| o.name.as_str()).collect();
        // The first declaration of the file, and the queries it goes on to declare; the
        // count is not pinned, so adding a query does not fail this.
        assert_eq!(names[0], "symbolAt", "{names:?}");
        assert!(names.len() >= 14, "{names:?}");
        assert!(names.contains(&"semanticTokensOf"), "{names:?}");
        assert!(names.contains(&"completionsAt"), "{names:?}");
        assert!(
            outline
                .iter()
                .any(|o| o.kind == SymbolKind::AgentFunction && o.name == "symbolAt")
        );
        // Parameters are not outlined.
        assert!(outline[0].children.is_empty(), "{:?}", outline[0].children);

        // Hover on `Workspace` in the signature of the first query.
        let text = fs::read_to_string(ws.root.join(main)).unwrap();
        let position = after(find_pos(&text, "workspace: Workspace", 0), 11);
        let hover = hover_at(&ws, main, position).unwrap();
        assert_eq!(hover.identifier, "Workspace");
        assert_eq!(hover.kind, SymbolKind::Data);
        assert!(hover.definition.is_some());
        assert_eq!(
            definition_of(&ws, main, position).unwrap().file,
            "def/workspace/data.lfy"
        );
        assert!(references_to(&ws, main, position, false).len() > 15);

        // Hover on a reference in prose.
        let position = after(find_pos(&text, "[[Workspace.root]]", 0), 13);
        let hover = hover_at(&ws, main, position).unwrap();
        assert_eq!(hover.identifier, "root");
        assert_eq!(hover.kind, SymbolKind::Member);

        // The entity of a query carries its criteria and its own source.
        let symbol_at_entity = find(&ws, "symbolAt");
        assert_eq!(symbol_at_entity.len(), 1);
        let hover = hover_of(&ws, symbol_at_entity[0]);
        assert!(!hover.criteria.is_empty());
        assert!(source_of(&ws, symbol_at_entity[0]).contains("fn symbolAt("));
        assert_eq!(find(&ws, "def/lexer/data.lfy:Token.line").len(), 1);
        assert!(
            find_symbols(&ws, "symbolAt")
                .iter()
                .any(|o| o.name == "symbolAt")
        );

        // Completions in the definition's own text.
        let position = after(find_pos(&text, "@acceptanceCriteria", 0), 1);
        assert!(labels(&completions_at(&ws, main, position)).contains(&"acceptanceCriteria"));
        let position = after(find_pos(&text, "use \"./data\";", 0), 7);
        assert!(labels(&completions_at(&ws, main, position)).contains(&"data"));
    }
}
