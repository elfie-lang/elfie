//! Compiled from `def/query/main.lfy`: one query surface over a bound [`Workspace`].
//!
//! Both the language server and the agent server call these queries; editors and agents
//! differ only in how they spell a position or a name, so each query takes either a
//! position or a name and nothing protocol-shaped. Positions are spelled exactly as tokens
//! spell them (line from 1, column from 0, counted in characters); no query converts.

use std::collections::HashSet;

pub mod data; // @lfy def/query/main.lfy:16

pub use data::*;

use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::{STATEMENTS, Statement as S};
use crate::grammar::terminals::comment::Comment as C;
use crate::grammar::terminals::identifier::Identifier as I;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::grammar::terminals::literal::Literal as L;
use crate::grammar::terminals::punctuation::Punctuation as P;
use crate::grammar::{Entity as Rule, GrammarRule};
use crate::lexer::Token;
use crate::model::{
    self, ContextProperty, EntityId, EntityKind, FileId, Model, NodeRef, ScopeId, SymbolId,
    SymbolKind, TypeRef, Usage,
};
use crate::parser::Node;
use crate::parser::components::is_trivia;
use crate::workspace::{Workspace, WorkspaceProblem};

/// The project layout file, where a load problem without a path is placed.
const MANIFEST: &str = "elfie.json";
/// The extension of a source file, dropped from path completions.
const EXTENSION: &str = ".lfy";

/// What [`range_of`] takes: a node of a file's tree, or a token by its index into the
/// file's tokens.
// Decision: the definition takes `Node | Token`; a token is given by its index because the
// index is how a tree refers to its tokens, and a node by reference because nodes are not
// copied out of the tree. [`range_of_node`] takes the model's `NodeRef` instead.
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
// @lfy def/query/main.lfy:33
fn file_of(workspace: &Workspace, path: &str) -> Option<FileId> {
    workspace.model.file(path)
}

/// Where a token starts.
fn start_of(token: &Token) -> Position {
    Position::new(token.line, token.column)
}

/// The position just after the last character of a token's raw text, with every line
/// break inside it ending a line, counted as the lexer counts.
// @lfy def/query/main.lfy:27
fn end_of(token: &Token) -> Position {
    let mut line = token.line;
    let mut column = token.column;
    for c in token.raw.chars() {
        if c == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    Position::new(line, column)
}

/// Whether a token is an identifier or a keyword: what a name is spelled with, and what a
/// position exactly after it still belongs to.
fn is_name_token(token: &Token) -> bool {
    token
        .rule
        .is_some_and(|rule| rule == I::Identifier.entity() || rule.is_keyword())
}

/// Whether a token is trivia: space, a line break, or one that no terminal matched.
fn is_trivia_token(token: &Token) -> bool {
    token.raw.is_empty() || token.rule.is_none_or(is_trivia)
}

/// The range of the tokens from `start` up to `end` of a file; empty at the position the
/// first token starts at, or at the end of the file, when it covers no token.
fn span(workspace: &Workspace, file: FileId, start: usize, end: usize) -> Range {
    let tokens = tokens(workspace, file);
    let path = workspace.model.sources[file].path.as_str();
    if start < end && end <= tokens.len() {
        // @lfy def/query/main.lfy:25
        let first = &tokens[start];
        return Range {
            file: first.file.to_string(),
            start: start_of(first),
            end: end_of(&tokens[end - 1]),
        };
    }
    // @lfy def/query/main.lfy:28
    let at = match tokens.get(start) {
        Some(token) => start_of(token),
        None => tokens.last().map_or(Position::new(1, 0), end_of),
    };
    Range::empty(path, at)
}

/// Where a node or token sits.
///
/// `Range.file` is `Token.file` of the first token covered, `Range.start` is its line and
/// column, and `Range.end` is the position just after the last character of the last
/// token covered. A node covering no token gives an empty range at the position the node
/// would start at.
// @lfy def/query/main.lfy:24
pub fn range_of(workspace: &Workspace, file: FileId, node: NodeOrToken<'_>) -> Range {
    match node {
        NodeOrToken::Node(node) => span(workspace, file, node.start, node.end),
        NodeOrToken::Token(index) => span(workspace, file, index, index + 1),
    }
}

/// [`range_of`] for a node the model refers to.
// @lfy def/query/main.lfy:24
pub fn range_of_node(workspace: &Workspace, node: NodeRef) -> Range {
    let info = workspace.model.info(node);
    span(workspace, node.file, info.start, info.end)
}

/// Whether a node reference points into a file: `global` and its scope are synthetic.
fn is_real(model: &Model, node: NodeRef) -> bool {
    node.file < model.sources.len()
}

/// The token under a position, by index.
///
/// `file` is relative to `Workspace.root`. A position exactly between two tokens belongs
/// to the earlier token when that is an identifier or a keyword (a `MemberName` is one of
/// those) and to the later token otherwise. A file not in the program, or a position past
/// the end of the file, gives `None`.
// @lfy def/query/main.lfy:32
pub fn token_at(workspace: &Workspace, file: &str, position: Position) -> Option<usize> {
    let file = file_of(workspace, file)?; // @lfy def/query/main.lfy:36
    let tokens = tokens(workspace, file);
    let mut previous: Option<usize> = None;
    for (index, token) in tokens.iter().enumerate() {
        // Decision: a token without raw text (a mode left open at the end of the file)
        // covers nothing and is never the token under a position.
        if token.raw.is_empty() {
            continue;
        }
        let start = start_of(token);
        if position < start {
            break;
        }
        if position < end_of(token) {
            // @lfy def/query/main.lfy:35
            if position == start
                && previous.is_some_and(|p| is_name_token(&tokens[p]) && end_of(&tokens[p]) == position)
            {
                return previous;
            }
            return Some(index); // @lfy def/query/main.lfy:34
        }
        previous = Some(index);
    }
    // Decision: the end of the file counts as the boundary after the last token, so a
    // position exactly after a final identifier or keyword still belongs to it.
    match previous {
        Some(p) if is_name_token(&tokens[p]) && end_of(&tokens[p]) == position => Some(p),
        _ => None, // @lfy def/query/main.lfy:36
    }
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
// @lfy def/query/main.lfy:52
pub fn nodes_at(workspace: &Workspace, file: &str, position: Position) -> Vec<NodeRef> {
    let Some(id) = file_of(workspace, file) else {
        return Vec::new();
    };
    match token_at(workspace, file, position) {
        Some(index) => nodes_covering(&workspace.model, id, index), // @lfy def/query/main.lfy:54
        None => Vec::new(),                                         // @lfy def/query/main.lfy:55
    }
}

/// Every node below a node, in preorder, the node itself excluded.
fn descendants(model: &Model, node: NodeRef) -> Vec<NodeRef> {
    let infos = &model.nodes[node.file];
    let mut out = Vec::new();
    let mut index = node.index + 1;
    while index < infos.len() {
        let mut ancestor = infos[index].parent;
        let mut inside = false;
        while let Some(a) = ancestor {
            if a == node.index {
                inside = true;
                break;
            }
            if a < node.index {
                break;
            }
            ancestor = infos[a].parent;
        }
        if !inside {
            break;
        }
        out.push(NodeRef {
            file: node.file,
            index,
        });
        index += 1;
    }
    out
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

// ---- Symbols ----------------------------------------------------------------------

/// The usage a `TemplateReference` makes: the first usage of a node inside its
/// `Reference`.
// @lfy def/query/main.lfy:61
fn reference_usage(model: &Model, reference: NodeRef) -> Option<model::UsageId> {
    descendants(model, reference)
        .into_iter()
        .find_map(|node| model.usage_of(node))
}

/// The symbol declared or used at a position.
///
/// On an identifier, or a keyword standing as a member name: walking [`nodes_at`]
/// innermost outward, the first node for which `resolve` gives a symbol whose name the
/// token spells. Inside a `TemplateReference`, in documentation included: the usage of
/// its `Reference`, so references in prose resolve like code. On the string of a `Use`:
/// the module symbol the `Use` declares, or `None` when it has no `as`. Anything else
/// gives `None`.
// @lfy def/query/main.lfy:58
pub fn symbol_at(workspace: &Workspace, file: &str, position: Position) -> Option<SymbolId> {
    let model = &workspace.model;
    let id = file_of(workspace, file)?;
    let index = token_at(workspace, file, position)?;
    let token = &tokens(workspace, id)[index];
    let chain = nodes_covering(model, id, index);
    // @lfy def/query/main.lfy:60
    if is_name_token(token) {
        for &node in chain.iter().rev() {
            if let Some(symbol) = model::resolve(model, node)
                && model.symbols[symbol].name == token.value
            {
                return Some(symbol);
            }
        }
    }
    // @lfy def/query/main.lfy:61
    if let Some(reference) = find_rule(model, &chain, E::TemplateReference.entity()) {
        return reference_usage(model, reference).and_then(|usage| model.usages[usage].symbol);
    }
    // @lfy def/query/main.lfy:62
    if (token.is(L::DoubleQuoteBody) || token.is(L::SingleQuoteBody))
        && let Some(use_node) = find_rule(model, &chain, S::Use.entity())
    {
        return model.symbol_of(use_node);
    }
    None // @lfy def/query/main.lfy:63
}

/// The file a `Use` resolved to in `Source.uses`, when it resolved.
fn use_target(workspace: &Workspace, use_node: NodeRef) -> Option<String> {
    let model = &workspace.model;
    // `Source.uses` lists the uses in the order they appear in the tree.
    let position = model.nodes[use_node.file]
        .iter()
        .enumerate()
        .filter(|(_, info)| info.rule == S::Use.entity())
        .position(|(index, _)| index == use_node.index)?;
    model.sources[use_node.file].uses.get(position)?.clone()
}

/// The range that declares a symbol: the identifier that spells its name, or an empty
/// range at line 1, column 0 of the used file for a module symbol.
// @lfy def/query/main.lfy:77
fn declaration_range(workspace: &Workspace, symbol: SymbolId) -> Option<Range> {
    let model = &workspace.model;
    let symbol = &model.symbols[symbol];
    if !is_real(model, symbol.node) {
        return None;
    }
    if symbol.kind == SymbolKind::Module {
        // @lfy def/query/main.lfy:78
        if let Some(path) = use_target(workspace, symbol.node) {
            return Some(Range::empty(&path, Position::new(1, 0)));
        }
        // Decision: a module symbol whose `Use` resolved to nothing is declared where its
        // name is written.
    }
    Some(match symbol.name_token {
        Some(index) => span(workspace, symbol.node.file, index, index + 1),
        None => range_of_node(workspace, symbol.node),
    })
}

/// Where the symbol at a position is declared.
///
/// The identifier that spells the name in the declaring node of [`symbol_at`]; for a
/// module symbol, or on the path of a `Use`, an empty range at line 1, column 0 of the
/// file the `Use` resolved to. An alias gives its own declaration, not what it aliases; a
/// member added by a trait gives the member statement inside the trait body.
// @lfy def/query/main.lfy:76
pub fn definition_of(workspace: &Workspace, file: &str, position: Position) -> Option<Range> {
    let model = &workspace.model;
    let id = file_of(workspace, file)?;
    if let Some(index) = token_at(workspace, file, position) {
        let chain = nodes_covering(model, id, index);
        // @lfy def/query/main.lfy:78
        if find_rule(model, &chain, E::StringLiteral.entity()).is_some()
            && let Some(use_node) = find_rule(model, &chain, S::Use.entity())
        {
            return use_target(workspace, use_node)
                .map(|path| Range::empty(&path, Position::new(1, 0)));
        }
    }
    let symbol = symbol_at(workspace, file, position)?; // @lfy def/query/main.lfy:81
    declaration_range(workspace, symbol)
}

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

/// The range of a usage: the token that spells the name, or the accessor when the usage
/// is of a layer itself, or the whole node when it has neither.
// @lfy def/query/main.lfy:86
fn usage_range(workspace: &Workspace, usage: &Usage) -> Range {
    match usage.token {
        Some(index) => span(workspace, usage.node.file, index, index + 1),
        None => range_of_node(workspace, usage.node),
    }
}

/// The position of a file in `Workspace.files`; after every file for a path not in it.
fn file_order(workspace: &Workspace, path: &str) -> usize {
    workspace
        .files
        .iter()
        .position(|file| file.path == path)
        .unwrap_or(usize::MAX)
}

/// Sorts ranges into `Workspace.files` order, then by start.
fn sort_ranges(workspace: &Workspace, ranges: &mut [Range]) {
    ranges.sort_by(|a, b| {
        (file_order(workspace, &a.file), &a.file, a.start, a.end).cmp(&(
            file_order(workspace, &b.file),
            &b.file,
            b.start,
            b.end,
        ))
    });
}

/// Every place the symbol at a position is used.
///
/// One range per usage of the symbol, covering the token that spells the name, or the
/// accessor when the usage is of a layer itself, in `Workspace.files` order then by
/// start; with `include_declaration`, the range [`definition_of`] gives comes first.
/// Empty when [`symbol_at`] gives nothing.
// @lfy def/query/main.lfy:84
pub fn references_to(
    workspace: &Workspace,
    file: &str,
    position: Position,
    include_declaration: bool,
) -> Vec<Range> {
    let model = &workspace.model;
    let Some(symbol) = symbol_at(workspace, file, position) else {
        return Vec::new(); // @lfy def/query/main.lfy:89
    };
    let mut usages: Vec<Range> = usages_of_declaration(model, symbol)
        .into_iter()
        .map(|usage| usage_range(workspace, &model.usages[usage]))
        .collect();
    sort_ranges(workspace, &mut usages); // @lfy def/query/main.lfy:88
    usages.dedup();
    let mut out = Vec::new();
    if include_declaration {
        // @lfy def/query/main.lfy:87
        if let Some(declaration) = declaration_range(workspace, symbol) {
            out.push(declaration);
        }
    }
    for range in usages {
        if !out.contains(&range) {
            out.push(range);
        }
    }
    out
}

// ---- Hover ------------------------------------------------------------------------

/// The kind of an entity that has no symbol, named after its `EntityKind`.
fn entity_kind_name(kind: &EntityKind) -> &'static str {
    match kind {
        EntityKind::File => "file",
        EntityKind::Global => "global",
        EntityKind::Data => "data",
        EntityKind::Type => "type",
        EntityKind::Enum => "enum",
        EntityKind::Fn { agent: true, .. } => "agentFunction",
        EntityKind::Fn { agent: false, .. } => "function",
        EntityKind::Trait { .. } => "trait",
        EntityKind::Variable => "variable",
        EntityKind::Alias => "alias",
        EntityKind::External => "external",
        EntityKind::Module => "module",
        EntityKind::LoopVariable => "loopVariable",
        EntityKind::Parameter => "parameter",
        EntityKind::Member => "member",
        EntityKind::EnumMember => "enumMember",
        EntityKind::Anonymous => "anonymous",
    }
}

/// The text of one documentation node: its raw text without the opener and closer, each
/// line without one leading space.
// @lfy def/query/main.lfy:102
fn documentation_of(workspace: &Workspace, file: FileId, node: &Node) -> String {
    let text = workspace.model.sources[file].tree.raw(node.start, node.end);
    let inner = text
        .strip_prefix("///")
        .or_else(|| text.strip_prefix("/**"))
        .unwrap_or(&text);
    let inner = inner.strip_suffix("**/").unwrap_or(inner);
    inner
        .lines()
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The joined text of the documentation nodes of a declaring node; `None` when it has
/// none.
// @lfy def/query/main.lfy:102
fn documentation_text(workspace: &Workspace, node: NodeRef) -> Option<String> {
    let tree_node = workspace.model.node(node);
    if tree_node.documentation.is_empty() {
        return None;
    }
    let text = tree_node
        .documentation
        .iter()
        .map(|documentation| documentation_of(workspace, node.file, documentation))
        .collect::<Vec<_>>()
        .join("\n");
    // Decision: trailing whitespace of the joined text is dropped, so a block comment
    // whose closer sits on its own line adds no empty last line.
    Some(text.trim_end().to_string())
}

/// The identifier of a type, or its source text when it declares no name.
// @lfy def/query/main.lfy:101
fn type_name(model: &Model, ty: &TypeRef) -> String {
    match ty {
        TypeRef::Entity(entity) => model.entities[*entity]
            .identifier
            .clone()
            .unwrap_or_else(|| model::type_text(model, ty)),
        _ => model::type_text(model, ty),
    }
}

/// Everything shown for one entity, at a range.
///
/// The kind is that of the entity's symbol (`member` for a member); the identifier is
/// `Entity.identifier`, or the first line of the declaring node when anonymous; the
/// definition has every template reference replaced by the referenced identifier; the
/// type is the identifier of `Entity.type` or its source text; the documentation is the
/// joined text of the documented nodes without their openers, closers, and one leading
/// space per line; the criteria are `criteriaOf` the entity.
// @lfy def/query/main.lfy:95
pub fn hover_of(workspace: &Workspace, entity: EntityId, range: Range) -> Hover {
    let model = &workspace.model;
    let e = &model.entities[entity];
    let node = e.node.filter(|&node| is_real(model, node));
    // @lfy def/query/main.lfy:98
    let kind = match (&e.kind, e.symbol) {
        (EntityKind::Member, _) => "member".to_string(),
        (_, Some(symbol)) => model.symbols[symbol].kind.as_str().to_string(),
        (kind, None) => entity_kind_name(kind).to_string(),
    };
    // @lfy def/query/main.lfy:99
    let identifier = e.identifier.clone().unwrap_or_else(|| {
        node.map(|node| model.raw(node).lines().next().unwrap_or("").trim().to_string())
            .unwrap_or_default()
    });
    Hover {
        range, // @lfy def/query/main.lfy:97
        kind,
        identifier,
        // Decision: the binder stores the definition with references as written, so they
        // are stripped to the referenced identifier here, as `criteriaOf` does.
        definition: e.definition.as_deref().map(model::strip_references), // @lfy def/query/main.lfy:100
        ty: e.ty.as_ref().map(|ty| type_name(model, ty)), // @lfy def/query/main.lfy:101
        documentation: node.and_then(|node| documentation_text(workspace, node)), // @lfy def/query/main.lfy:102
        criteria: model::criteria_of(model, entity), // @lfy def/query/main.lfy:103
    }
}

/// Everything shown for the symbol at a position: [`hover_of`] the entity of
/// [`symbol_at`], at the range of the token under the position.
// @lfy def/query/main.lfy:106
pub fn hover_at(workspace: &Workspace, file: &str, position: Position) -> Option<Hover> {
    let symbol = symbol_at(workspace, file, position)?; // @lfy def/query/main.lfy:109
    let id = file_of(workspace, file)?;
    let index = token_at(workspace, file, position)?;
    let entity = workspace.model.symbols[symbol].entity;
    Some(hover_of(workspace, entity, span(workspace, id, index, index + 1))) // @lfy def/query/main.lfy:108
}

// ---- Completions ------------------------------------------------------------------

/// The text of a token before a position inside it (all of it when the position is at
/// or past its end).
fn text_before(token: &Token, position: Position) -> String {
    let mut line = token.line;
    let mut column = token.column;
    let mut out = String::new();
    for c in token.raw.chars() {
        if Position::new(line, column) >= position {
            break;
        }
        out.push(c);
        if c == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    out
}

/// The identifier characters at the end of a text.
fn identifier_tail(text: &str) -> String {
    let tail: String = text
        .chars()
        .rev()
        .take_while(|&c| c == '_' || unicode_ident::is_xid_continue(c))
        .collect();
    tail.chars().rev().collect()
}

/// Every symbol visible from a scope: the nearer scope's first, each scope's own symbols
/// then its imports, then `global`; one per name, the nearest winning.
// @lfy def/query/main.lfy:117
fn visible_symbols(model: &Model, scope: ScopeId) -> Vec<SymbolId> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    let mut current = Some(scope);
    while let Some(id) = current {
        let scope = &model.scopes[id];
        for &symbol in scope.symbols.iter().chain(scope.imports.iter()) {
            if seen.insert(model.symbols[symbol].name.as_str()) {
                out.push(symbol);
            }
        }
        current = scope.parent;
    }
    if let Some(global) = model.entities[model.global].symbol
        && seen.insert(model.symbols[global].name.as_str())
    {
        out.push(global);
    }
    out
}

/// A completion for a declared symbol.
fn symbol_completion(model: &Model, symbol: SymbolId) -> Completion {
    let symbol = &model.symbols[symbol];
    Completion {
        label: symbol.name.clone(),
        kind: symbol.kind.as_str().to_string(),
        detail: model.entities[symbol.entity]
            .definition
            .as_deref()
            .map(model::strip_references),
    }
}

/// A keyword completion.
fn keyword_completion(text: &str) -> Completion {
    Completion {
        label: text.to_string(),
        kind: "keyword".to_string(),
        detail: None,
    }
}

/// The keywords named by `[[...]]` references in a piece of EBNF syntax, in order.
fn keywords_in(syntax: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut rest = syntax;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        if let Some(Rule::Keyword(keyword)) = Rule::lookup(&after[..close])
            && let Some(word) = keyword.word()
        {
            out.push(word);
        }
        rest = &after[close + 2..];
    }
    out
}

/// The syntax of the first element of a rule: up to the first comma outside parentheses.
fn first_element(syntax: &str) -> &str {
    let mut depth = 0usize;
    for (index, c) in syntax.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return &syntax[..index],
            _ => {}
        }
    }
    syntax
}

/// The keyword that begins each statement rule, in the order of the rules.
// @lfy def/query/main.lfy:129
fn statement_keywords() -> Vec<&'static str> {
    let mut out = Vec::new();
    for rule in STATEMENTS {
        for word in keywords_in(first_element(rule.syntax())) {
            if !out.contains(&word) {
                out.push(word);
            }
        }
    }
    out
}

/// The primitive type keywords: what `PrimitiveType` lists.
// @lfy def/query/main.lfy:128
fn primitive_type_keywords() -> Vec<&'static str> {
    keywords_in(E::PrimitiveType.syntax())
}

/// The value keywords: true, false, null, and undefined.
// @lfy def/query/main.lfy:130
fn value_keywords() -> Vec<&'static str> {
    let mut out = keywords_in(E::Boolean.syntax());
    out.extend(keywords_in(E::Nullish.syntax()));
    out
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
    model.scopes[model.file_scopes[file]].symbols.clone()
}

/// What a type offers after a value accessor.
fn type_members(model: &Model, ty: &TypeRef) -> Option<Vec<SymbolId>> {
    match ty {
        TypeRef::Entity(entity) => match model.entities[*entity].kind {
            EntityKind::File => Some(module_symbols(model, model.entities[*entity].file?)),
            _ => Some(member_symbols(model, *entity)),
        },
        TypeRef::Predicate(entity) => Some(member_symbols(model, *entity)),
        TypeRef::Union(items) => items.iter().find_map(|item| type_members(model, item)),
        _ => None,
    }
}

/// What the left side of a member access offers: a module's symbols, an entity's members,
/// or an enum's keys; `None` when the left side resolves to nothing.
// @lfy def/query/main.lfy:123
fn left_members(model: &Model, left: NodeRef) -> Option<Vec<SymbolId>> {
    let rule = model.info(left).rule;
    if rule == E::Group.entity() {
        let inner = child_nodes(model, left).into_iter().next()?;
        return left_members(model, inner);
    }
    let symbol = model.usage_of(left).and_then(|usage| model.usages[usage].symbol)?;
    let symbol = &model.symbols[symbol];
    let entity = &model.entities[symbol.entity];
    match symbol.kind {
        SymbolKind::Module => match entity.kind {
            EntityKind::File => Some(module_symbols(model, entity.file?)),
            _ => None,
        },
        SymbolKind::Data
        | SymbolKind::Trait
        | SymbolKind::Type
        | SymbolKind::Enum
        | SymbolKind::Function
        | SymbolKind::AgentFunction => Some(member_symbols(model, symbol.entity)),
        SymbolKind::Alias | SymbolKind::External => match entity.kind {
            EntityKind::Data | EntityKind::Trait { .. } | EntityKind::Type | EntityKind::Enum => {
                Some(member_symbols(model, symbol.entity))
            }
            EntityKind::File => Some(module_symbols(model, entity.file?)),
            _ => entity.ty.as_ref().and_then(|ty| type_members(model, ty)),
        },
        _ => entity.ty.as_ref().and_then(|ty| type_members(model, ty)),
    }
}

/// The directory holding a file, relative to the root; empty for a file at the root.
fn directory_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// A path with every `.` dropped and every `..` applied to the segment before it.
fn normalize(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => match segments.last() {
                Some(&last) if last != ".." => {
                    segments.pop();
                }
                _ => segments.push(".."),
            },
            segment => segments.push(segment),
        }
    }
    segments.join("/")
}

/// The directories and the `.lfy` files without their extension under a directory of the
/// project, sorted, each once.
// @lfy def/query/main.lfy:125
fn entries_under(workspace: &Workspace, directory: &str) -> Vec<Completion> {
    let disk = if directory.is_empty() {
        workspace.root.clone()
    } else {
        workspace.root.join(directory)
    };
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&disk) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() {
                names.push(name);
            } else if let Some(stem) = name.strip_suffix(EXTENSION) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|label| Completion {
            label,
            kind: "path".to_string(),
            detail: None,
        })
        .collect()
}

/// Completions inside the string of a `Use`, with the prefix already typed: for a path
/// beginning with a dot, the entries under the directory the path names so far, relative
/// to the file's directory; otherwise each package's identifier, then entries under its
/// root the same way.
// @lfy def/query/main.lfy:125
fn path_completions(workspace: &Workspace, file: &str, typed: &str) -> (Vec<Completion>, String) {
    let (directory, prefix) = match typed.rfind('/') {
        Some(slash) => (&typed[..slash], &typed[slash + 1..]),
        None => ("", typed),
    };
    let completions = if typed.starts_with('.') {
        let base = normalize(&format!("{}/{directory}", directory_of(file)));
        entries_under(workspace, &base)
    } else if typed.contains('/') {
        let (package, rest) = directory.split_once('/').unwrap_or((directory, ""));
        match workspace
            .packages
            .iter()
            .find(|candidate| candidate.identifier == package)
        {
            Some(package) => entries_under(workspace, &normalize(&format!("{}/{rest}", package.root))),
            None => Vec::new(),
        }
    } else {
        workspace
            .packages
            .iter()
            .map(|package| Completion {
                label: package.identifier.clone(),
                kind: "path".to_string(),
                detail: None,
            })
            .collect()
    };
    (completions, prefix.to_string())
}

/// Whether a statement can begin after a token: at the start of a file or block, after a
/// statement ends, or after a keyword that prefixes a statement.
// Decision: the definition names the situation only; these are the tokens the grammar
// lets a statement follow.
fn statement_begins_after(before: Option<&Token>) -> bool {
    let Some(before) = before else {
        return true;
    };
    before.is(P::Semicolon)
        || before.is(P::BlockOpen)
        || before.is(P::BlockClose)
        || before.is(K::AsyncKeyword)
        || before.is(K::AceKeyword)
        || before.is(K::ElseKeyword)
}

/// Whether an expression can begin after a token: after an operator, an opener, a
/// separator, a setter, or a keyword that takes an expression.
// Decision: the definition names the situation only; an expression follows any
// punctuation that is not a closer, an accessor, or a statement boundary, and the
// keywords that take an expression.
fn expression_begins_after(before: &Token) -> bool {
    let Some(rule) = before.rule else {
        return false;
    };
    if rule.is_punctuation() {
        return !matches!(
            rule,
            Rule::Punctuation(
                P::GroupClose
                    | P::ListClose
                    | P::BlockClose
                    | P::BlockOpen
                    | P::Semicolon
                    | P::ValueAccessor
                    | P::OptionalValueAccessor
                    | P::ContextAccessor
                    | P::ScopeAccessor
                    | P::ParentScopeAccessor
            )
        );
    }
    matches!(
        rule,
        Rule::Keyword(
            K::ReturnKeyword
                | K::AwaitKeyword
                | K::InKeyword
                | K::OfKeyword
                | K::FromKeyword
                | K::WhileKeyword
                | K::IfKeyword
                | K::MatchKeyword
                | K::MatchallKeyword
                | K::WithKeyword
                | K::WhereKeyword
                | K::AndKeyword
                | K::OrKeyword
        )
    )
}

/// What could be typed at a position.
///
/// The context is decided by the nearest token before the position that is not trivia,
/// and by the innermost node there. Every completion appears once, filtered to labels
/// that begin with the identifier characters already typed, case sensitive; those from a
/// nearer scope come before those from an enclosing scope, then file order, then
/// keywords.
///
/// After `@`: one completion of kind `context` per context property. After `$`: the
/// members of the current entity of the scope holding the position (of the left side,
/// when there is one). After `$&`: the members of the parent scope's current entity.
/// After `.` or `?.`: what the left side offers, or nothing when it resolves to nothing.
/// After `is`, `extends`, or a comma in trait uses: every trait visible plus the module
/// symbols. Inside the string of a `Use`: paths. Inside a template reference or
/// documentation: every name visible, then every rule entity. After the colon of a
/// definition clause: every data, type, enum, or trait visible, then the primitive type
/// keywords. Where a statement can begin: every name visible, then the keyword that
/// begins each statement. Where an expression can begin: every name visible, then the
/// value keywords, then the primitive type keywords.
// @lfy def/query/main.lfy:113
pub fn completions_at(workspace: &Workspace, file: &str, position: Position) -> Vec<Completion> {
    let model = &workspace.model;
    let Some(id) = file_of(workspace, file) else {
        return Vec::new();
    };
    let tokens = tokens(workspace, id);

    // The token the position is inside or just after, and the identifier characters
    // typed before the position. @lfy def/query/main.lfy:116
    let at = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| !token.raw.is_empty())
        .find(|(_, token)| start_of(token) < position && position <= end_of(token))
        .map(|(index, _)| index);
    let typed = at.map(|index| text_before(&tokens[index], position));
    let mut prefix = typed.as_deref().map(identifier_tail).unwrap_or_default();
    let prefix_start = Position::new(position.line, position.column - prefix.chars().count());

    // The nearest token before the typed characters that is not trivia.
    // @lfy def/query/main.lfy:116
    let before = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| !is_trivia_token(token) && end_of(token) <= prefix_start)
        .map(|(index, _)| index)
        .next_back();
    let anchor = at.or(before);
    let chain = match anchor {
        Some(index) => nodes_covering(model, id, index),
        None => vec![NodeRef { file: id, index: 0 }],
    };
    let innermost = *chain.last().unwrap_or(&NodeRef { file: id, index: 0 });
    let scope = model.enclosing_scope(innermost);
    let before_token = before.map(|index| &tokens[index]);
    let before_rule = before_token.and_then(|token| token.rule);

    let mut out: Vec<Completion> = Vec::new();

    // Inside the string of a Use. @lfy def/query/main.lfy:125
    let in_use_string = find_rule(model, &chain, S::Use.entity()).is_some()
        && find_rule(model, &chain, E::StringLiteral.entity()).is_some()
        && at.is_some_and(|index| {
            let token = &tokens[index];
            token.is(L::DoubleQuoteBody)
                || token.is(L::SingleQuoteBody)
                || ((token.is(L::DoubleQuote) || token.is(L::SingleQuote))
                    && tokens
                        .get(index + 1)
                        .is_some_and(|next| !next.is(P::Semicolon) && !is_trivia_token(next))
                    && tokens.get(index.wrapping_sub(1)).is_none_or(|previous| {
                        !previous.is(L::DoubleQuoteBody) && !previous.is(L::SingleQuoteBody)
                    }))
        });
    if in_use_string {
        let index = at.expect("checked");
        let token = &tokens[index];
        let text = if token.is(L::DoubleQuoteBody) || token.is(L::SingleQuoteBody) {
            typed.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let (completions, path_prefix) = path_completions(workspace, file, &text);
        out = completions;
        prefix = path_prefix;
        return finish(out, &prefix);
    }

    let members_of = |entity: EntityId| -> Vec<Completion> {
        member_symbols(model, entity)
            .into_iter()
            .map(|symbol| symbol_completion(model, symbol))
            .collect()
    };
    let names = |out: &mut Vec<Completion>| {
        out.extend(
            visible_symbols(model, scope)
                .into_iter()
                .map(|symbol| symbol_completion(model, symbol)),
        );
    };
    // The left side of the accessor at `before`, when it is the operator of a Member.
    let left_of_accessor = || -> Option<NodeRef> {
        let index = before?;
        let holder = *nodes_covering(model, id, index).last()?;
        if model.info(holder).rule != E::Member.entity() {
            return None;
        }
        child_nodes(model, holder).into_iter().next()
    };
    // The innermost node holding a token.
    let holder_of = |index: usize| -> Option<Rule> {
        nodes_covering(model, id, index)
            .last()
            .map(|&node| model.info(node).rule)
    };
    // The index of the nearest token before another that is not trivia.
    let previous_of = |index: usize| -> Option<usize> {
        tokens[..index].iter().rposition(|token| !is_trivia_token(token))
    };
    // Decision: a recoverable rule that fails past its trait uses (`d A is t, ` at the
    // end of a file) puts the comma in an error node, so when the tree does not say the
    // comma is in `TraitUses`, the tokens do: a run of names and commas back to `is` or
    // `extends` is a trait list.
    let in_trait_uses = before.is_some_and(|index| {
        if find_rule(model, &nodes_covering(model, id, index), E::TraitUses.entity()).is_some() {
            return true;
        }
        let mut cursor = index;
        while let Some(previous) = previous_of(cursor) {
            let token = &tokens[previous];
            if token.is(K::IsKeyword) || token.is(K::ExtendsKeyword) {
                return true;
            }
            if !(token.is(I::Identifier) || token.is(P::Comma) || token.is(P::ValueAccessor)) {
                return false;
            }
            cursor = previous;
        }
        false
    });
    // Decision: a `DefinitionClause` whose type is still missing is not in the tree
    // either (`const w: ` at a line end), so a colon that is not in a `Conditional` and
    // directly follows a name, a `)`, or a `?` is taken as a definition colon.
    let is_definition_colon = before.is_some_and(|index| {
        let holder = holder_of(index);
        holder == Some(E::DefinitionClause.entity())
            || holder == Some(E::Definition.entity())
            || (holder != Some(E::Conditional.entity())
                && previous_of(index).is_some_and(|previous| {
                    let token = &tokens[previous];
                    is_name_token(token) || token.is(P::GroupClose) || token.is(P::QuestionMark)
                }))
    });
    let in_prose = find_rule(model, &chain, E::TemplateReference.entity()).is_some()
        || find_rule(model, &chain, C::Documentation.entity()).is_some()
        || matches!(
            before_rule,
            Some(Rule::Literal(L::ReferenceOpen))
                | Some(Rule::Comment(
                    C::LineDocumentationOpen
                        | C::LineDocumentationBody
                        | C::BlockDocumentationOpen
                        | C::BlockDocumentationBody
                ))
        );

    match before_rule {
        // @lfy def/query/main.lfy:120
        Some(Rule::Punctuation(P::ContextAccessor)) => {
            out.extend(ContextProperty::ALL.into_iter().map(|property| Completion {
                label: property.value().to_string(),
                kind: "context".to_string(),
                detail: None,
            }));
        }
        // @lfy def/query/main.lfy:121
        Some(Rule::Punctuation(P::ScopeAccessor)) => {
            // Decision: `$` after a left side (a Member) reaches that side's members, as
            // the value accessor does; alone it reaches the current entity's.
            match left_of_accessor() {
                Some(left) => {
                    if let Some(members) = left_members(model, left) {
                        out.extend(members.into_iter().map(|symbol| symbol_completion(model, symbol)));
                    }
                }
                None => out.extend(members_of(model.scopes[scope].current)),
            }
        }
        // @lfy def/query/main.lfy:122
        Some(Rule::Punctuation(P::ParentScopeAccessor)) => {
            // Decision: the parent of the scope is the nearest enclosing scope whose
            // current entity differs, since a block inside a declaration shares its
            // declaration's current entity.
            let current = model.scopes[scope].current;
            let mut parent = model.scopes[scope].parent;
            while let Some(id) = parent {
                if model.scopes[id].current != current {
                    out.extend(members_of(model.scopes[id].current));
                    break;
                }
                parent = model.scopes[id].parent;
            }
        }
        // @lfy def/query/main.lfy:123
        Some(Rule::Punctuation(P::ValueAccessor | P::OptionalValueAccessor)) => {
            if let Some(members) = left_of_accessor().and_then(|left| left_members(model, left)) {
                out.extend(members.into_iter().map(|symbol| symbol_completion(model, symbol)));
            }
            // @lfy def/query/main.lfy:124
        }
        // @lfy def/query/main.lfy:125
        Some(Rule::Keyword(K::IsKeyword | K::ExtendsKeyword)) => trait_names(model, scope, &mut out),
        Some(Rule::Punctuation(P::Comma)) if in_trait_uses => trait_names(model, scope, &mut out),
        // @lfy def/query/main.lfy:126
        _ if in_prose => {
            names(&mut out);
            if let Some(rule_trait) = model.trait_named("rule") {
                for (entity, e) in model.entities.iter().enumerate() {
                    if e.has_trait(rule_trait) {
                        if let Some(symbol) = e.symbol {
                            out.push(symbol_completion(model, symbol));
                        } else if let Some(identifier) = &e.identifier {
                            out.push(Completion {
                                label: identifier.clone(),
                                kind: entity_kind_name(&model.entities[entity].kind).to_string(),
                                detail: e.definition.as_deref().map(model::strip_references),
                            });
                        }
                    }
                }
            }
        }
        // @lfy def/query/main.lfy:127
        Some(Rule::Punctuation(P::Colon)) if is_definition_colon => {
            out.extend(
                visible_symbols(model, scope)
                    .into_iter()
                    .filter(|&symbol| {
                        matches!(
                            model.symbols[symbol].kind,
                            SymbolKind::Data | SymbolKind::Type | SymbolKind::Enum | SymbolKind::Trait
                        )
                    })
                    .map(|symbol| symbol_completion(model, symbol)),
            );
            out.extend(primitive_type_keywords().into_iter().map(keyword_completion));
        }
        // @lfy def/query/main.lfy:129
        _ if statement_begins_after(before_token) => {
            names(&mut out);
            out.extend(statement_keywords().into_iter().map(keyword_completion));
        }
        // @lfy def/query/main.lfy:130
        _ if before_token.is_some_and(expression_begins_after) => {
            names(&mut out);
            out.extend(value_keywords().into_iter().map(keyword_completion));
            out.extend(primitive_type_keywords().into_iter().map(keyword_completion));
        }
        _ => {}
    }
    finish(out, &prefix)
}

/// Every symbol of kind trait visible from a scope, plus the module symbols.
// Decision: the traits come first, then the modules, each group in scope order.
// @lfy def/query/main.lfy:125
fn trait_names(model: &Model, scope: ScopeId, out: &mut Vec<Completion>) {
    let visible = visible_symbols(model, scope);
    for kind in [SymbolKind::Trait, SymbolKind::Module] {
        out.extend(
            visible
                .iter()
                .filter(|&&symbol| model.symbols[symbol].kind == kind)
                .map(|&symbol| symbol_completion(model, symbol)),
        );
    }
}

/// Filters completions to the labels beginning with the typed prefix, each label once.
// @lfy def/query/main.lfy:117
fn finish(completions: Vec<Completion>, prefix: &str) -> Vec<Completion> {
    let mut seen: HashSet<String> = HashSet::new();
    completions
        .into_iter()
        .filter(|completion| completion.label.starts_with(prefix))
        .filter(|completion| seen.insert(completion.label.clone()))
        .collect()
}

// ---- Diagnostics ------------------------------------------------------------------

/// Every problem of the program, placed.
///
/// A load problem is an error of stage loader at line 1, column 0 of its path, or of
/// `elfie.json` under the root when it has none. A token without a rule is an error of
/// stage lexer at the token: invalid text, or the mode left open when the token has no
/// text. An error node is an error of stage parser listing what was expected and what
/// was found. A problem of the model is an error of stage binder at its node. A `Use`
/// importing symbols of which none is used in the file, or naming a module never used, is
/// a warning of stage binder at the `Use`. With `file` set, only that file's diagnostics
/// are returned. Diagnostics are in `Workspace.files` order, then by start, with
/// `elfie.json` first.
// @lfy def/query/main.lfy:140
pub fn diagnostics_of(workspace: &Workspace, file: Option<&str>) -> Vec<Diagnostic> {
    let model = &workspace.model;
    let mut out = Vec::new();
    let error = |range: Range, stage: Stage, message: String| Diagnostic {
        range,
        severity: Severity::Error,
        stage,
        message,
    };

    for problem in &workspace.problems {
        match problem {
            WorkspaceProblem::Load(problem) => {
                // @lfy def/query/main.lfy:142
                let path = problem.path.as_deref().unwrap_or(MANIFEST); // @lfy def/query/main.lfy:143
                out.push(error(
                    Range::empty(path, Position::new(1, 0)),
                    Stage::Loader,
                    problem.message.clone(),
                ));
            }
            WorkspaceProblem::Bind(problem) => {
                // @lfy def/query/main.lfy:146
                if is_real(model, problem.node) {
                    out.push(error(range_of_node(workspace, problem.node), Stage::Binder, problem.message.clone()));
                }
            }
        }
    }

    for (id, source) in model.sources.iter().enumerate() {
        for (index, token) in source.tree.tokens.iter().enumerate() {
            if token.rule.is_some() {
                continue;
            }
            // @lfy def/query/main.lfy:144
            let message = if token.raw.is_empty() {
                format!("the {} is left open at the end of the file", token.value)
            } else {
                format!("invalid text {:?}", token.raw)
            };
            out.push(error(span(workspace, id, index, index + 1), Stage::Lexer, message));
        }
        for node in &source.tree.errors {
            // @lfy def/query/main.lfy:145
            let found = source.tree.raw(node.start, node.end);
            let found = if found.is_empty() {
                "the end of the file".to_string()
            } else {
                format!("{:?}", found.trim())
            };
            let message = if node.expected.is_empty() {
                format!("unexpected {found}")
            } else {
                format!("expected {}, found {found}", node.expected.join(" or "))
            };
            out.push(error(span(workspace, id, node.start, node.end), Stage::Parser, message));
        }
        out.extend(unused_uses(workspace, id));
    }

    // @lfy def/query/main.lfy:148
    if let Some(file) = file {
        out.retain(|diagnostic| diagnostic.range.file == file);
    }
    // @lfy def/query/main.lfy:149
    out.sort_by(|a, b| diagnostic_key(workspace, a).cmp(&diagnostic_key(workspace, b)));
    out
}

/// The sort key of a diagnostic: `elfie.json` first, then `Workspace.files` order, then
/// paths outside the program, then start.
fn diagnostic_key<'d>(workspace: &Workspace, diagnostic: &'d Diagnostic) -> (usize, &'d str, Position, Position) {
    let order = if diagnostic.range.file == MANIFEST {
        0
    } else {
        file_order(workspace, &diagnostic.range.file).saturating_add(1)
    };
    (order, &diagnostic.range.file, diagnostic.range.start, diagnostic.range.end)
}

/// A warning per `Use` of a file that imports symbols of which none is used in the file,
/// or whose module symbol is never used.
// @lfy def/query/main.lfy:147
fn unused_uses(workspace: &Workspace, file: FileId) -> Vec<Diagnostic> {
    let model = &workspace.model;
    let mut out = Vec::new();
    let uses: Vec<NodeRef> = model.nodes[file]
        .iter()
        .enumerate()
        .filter(|(_, info)| info.rule == S::Use.entity())
        .map(|(index, _)| NodeRef { file, index })
        .collect();
    for (position, use_node) in uses.into_iter().enumerate() {
        let Some(Some(path)) = model.sources[file].uses.get(position) else {
            continue;
        };
        let message = match model.symbol_of(use_node) {
            Some(symbol) => {
                if model::usages_of(model, symbol).is_empty() {
                    Some(format!("the module {} is never used", model.symbols[symbol].name))
                } else {
                    None
                }
            }
            None => {
                let Some(used) = model.file(path) else { continue };
                let imported: HashSet<SymbolId> = module_symbols(model, used).into_iter().collect();
                let any_used = model.usages.iter().any(|usage| {
                    usage.node.file == file && usage.symbol.is_some_and(|symbol| imported.contains(&symbol))
                });
                if any_used {
                    None
                } else {
                    Some(format!("nothing that {path} declares is used in this file"))
                }
            }
        };
        if let Some(message) = message {
            out.push(Diagnostic {
                range: range_of_node(workspace, use_node),
                severity: Severity::Warning,
                stage: Stage::Binder,
                message,
            });
        }
    }
    out
}

// ---- Outline ----------------------------------------------------------------------

/// Whether a symbol is listed in an outline: parameters, loop variables, and modules
/// are not.
fn is_outlined(kind: SymbolKind) -> bool {
    !matches!(
        kind,
        SymbolKind::Parameter | SymbolKind::LoopVariable | SymbolKind::Module
    )
}

/// The outline of one declared symbol: its range covers the declaring node and its
/// documentation, its selection range the identifier, and its children are the members
/// declared in its body, then every declaration nested in its body, in order.
// @lfy def/query/main.lfy:154
fn outline_of_symbol(workspace: &Workspace, symbol: SymbolId) -> Outline {
    let model = &workspace.model;
    let s = &model.symbols[symbol];
    let node = model.node(s.node);
    let start = node
        .documentation
        .first()
        .map_or(node.start, |documentation| documentation.start.min(node.start));
    let range = span(workspace, s.node.file, start, node.end); // @lfy def/query/main.lfy:156
    let selection_range = match s.name_token {
        Some(index) => span(workspace, s.node.file, index, index + 1),
        None => range_of_node(workspace, s.node),
    };
    let mut children = Vec::new();
    let mut listed: HashSet<SymbolId> = HashSet::new();
    // Members declared in the body itself; a member added by a trait is declared
    // elsewhere and left out. @lfy def/query/main.lfy:155
    for member in model.members(s.entity) {
        let m = &model.symbols[member];
        let own = m.node.file == s.node.file
            && m.node != s.node
            && model.info(m.node).start >= node.start
            && model.info(m.node).end <= node.end;
        if own && matches!(m.kind, SymbolKind::Member | SymbolKind::EnumMember) && listed.insert(member) {
            children.push(outline_of_symbol(workspace, member));
        }
    }
    nested_declarations(workspace, s.node, &mut listed, &mut children);
    Outline {
        name: s.name.clone(),
        kind: s.kind.as_str().to_string(),
        range,
        selection_range,
        children,
    }
}

/// Every declaration nested below a node, in order, each with its own children; a node
/// that declares something is not entered further.
// @lfy def/query/main.lfy:155
fn nested_declarations(workspace: &Workspace, node: NodeRef, listed: &mut HashSet<SymbolId>, out: &mut Vec<Outline>) {
    let model = &workspace.model;
    for child in child_nodes(model, node) {
        match model.symbol_of(child) {
            Some(symbol) if listed.contains(&symbol) => {}
            Some(symbol)
                if is_outlined(model.symbols[symbol].kind)
                    && !matches!(model.symbols[symbol].kind, SymbolKind::Member | SymbolKind::EnumMember)
                    && model.symbols[symbol].node == child =>
            {
                listed.insert(symbol);
                out.push(outline_of_symbol(workspace, symbol));
            }
            _ => nested_declarations(workspace, child, listed, out),
        }
    }
}

/// The declarations of one file, nested as written.
///
/// One outline per symbol declared in the file scope, in order, except module symbols
/// and the symbols a `Use` imports; each holds the members of its body and every
/// declaration nested in it. Empty for a file not in the program.
// @lfy def/query/main.lfy:172
pub fn outline_of(workspace: &Workspace, file: &str) -> Vec<Outline> {
    let model = &workspace.model;
    let Some(id) = file_of(workspace, file) else {
        return Vec::new(); // @lfy def/query/main.lfy:157
    };
    model.scopes[model.file_scopes[id]]
        .symbols
        .iter()
        .copied()
        .filter(|&symbol| is_outlined(model.symbols[symbol].kind))
        .map(|symbol| outline_of_symbol(workspace, symbol))
        .collect()
}

/// The files of the program in the order `findSymbols` uses: files without a package
/// first, then package files, each in `Workspace.files` order.
// @lfy def/query/main.lfy:172
fn files_in_search_order(workspace: &Workspace) -> Vec<FileId> {
    let own = workspace.files.iter().filter(|file| file.package.is_none());
    let packaged = workspace.files.iter().filter(|file| file.package.is_some());
    own.chain(packaged).map(|file| file.source).collect()
}

/// Flattens an outline: each entry with its dotted name and no children.
fn flatten(outline: &Outline, parent: Option<&str>, out: &mut Vec<Outline>) {
    let name = match parent {
        Some(parent) => format!("{parent}.{}", outline.name),
        None => outline.name.clone(),
    };
    out.push(Outline {
        name: name.clone(),
        kind: outline.kind.clone(),
        range: outline.range.clone(),
        selection_range: outline.selection_range.clone(),
        children: Vec::new(),
    });
    for child in &outline.children {
        flatten(child, Some(&name), out);
    }
}

/// Declarations across the program whose name contains a text, ignoring case: every
/// outline of every file, flattened, a child named by its parent's name, a dot, and its
/// own. An empty query gives every top level declaration. Files without a package come
/// first, then package files, each in `Workspace.files` order.
// @lfy def/query/main.lfy:180
pub fn find_symbols(workspace: &Workspace, query: &str) -> Vec<Outline> {
    let query = query.to_lowercase();
    let mut out = Vec::new();
    for file in files_in_search_order(workspace) {
        let path = workspace.model.sources[file].path.clone();
        for outline in outline_of(workspace, &path) {
            if query.is_empty() {
                // @lfy def/query/main.lfy:163
                // Decision: flattened entries carry no children, so a top level entry
                // is listed alone.
                let mut top = outline.clone();
                top.children.clear();
                out.push(top);
                continue;
            }
            let mut flat = Vec::new();
            flatten(&outline, None, &mut flat);
            out.extend(flat.into_iter().filter(|entry| entry.name.to_lowercase().contains(&query)));
        }
    }
    out
}

// ---- Rename -----------------------------------------------------------------------

/// Where a symbol is declared, as `file:line`, for a reason.
fn declared_at(workspace: &Workspace, symbol: SymbolId) -> String {
    let model = &workspace.model;
    let s = &model.symbols[symbol];
    if !is_real(model, s.node) {
        return "the program itself".to_string();
    }
    let file = &model.sources[s.node.file].path;
    let line = s
        .name_token
        .map(|index| tokens(workspace, s.node.file)[index].line)
        .or_else(|| model.first_token(s.node).map(|token| token.line))
        .unwrap_or(1);
    format!("{file}:{line}")
}

/// The edits that rename the symbol at a position everywhere, or why it cannot be renamed.
///
/// Nothing declared at the position, a name that is not an identifier or is a keyword,
/// or a symbol named `name` already visible from the declaring scope or from the scope of
/// any usage (the rename would capture or be captured) each give a reason. Otherwise one
/// edit replaces the identifier of the declaration and one the name token of every
/// usage, references in templates and documentation included; for a module symbol only
/// the name after `as` and its usages change. Edits are in `Workspace.files` order, then
/// by start, and never overlap.
// @lfy def/query/main.lfy:187
pub fn rename_at(workspace: &Workspace, file: &str, position: Position, name: &str) -> Result<Vec<Edit>, String> {
    let model = &workspace.model;
    let Some(symbol) = symbol_at(workspace, file, position) else {
        return Err("nothing is declared here".to_string()); // @lfy def/query/main.lfy:189
    };
    // @lfy def/query/main.lfy:190
    if let Some(keyword) = K::from_text(name) {
        return Err(format!("{name} is a keyword ({})", keyword.identifier()));
    }
    if !I::Identifier.matches(name) {
        return Err(format!("{name:?} is not an {}", I::Identifier.identifier()));
    }
    let s = &model.symbols[symbol];
    if s.name == name {
        return Ok(Vec::new());
    }
    let Some(name_token) = s.name_token.filter(|_| is_real(model, s.node)) else {
        return Err("the declaration spells no name that can be renamed".to_string());
    };
    let usages = usages_of_declaration(model, symbol);
    // @lfy def/query/main.lfy:191
    let mut scopes: Vec<ScopeId> = vec![s.scope];
    scopes.extend(usages.iter().map(|&usage| model.enclosing_scope(model.usages[usage].node)));
    for scope in scopes {
        if let Some(other) = model.lookup(scope, name) {
            return Err(format!(
                "a symbol named {name} is already visible, declared at {}",
                declared_at(workspace, other)
            ));
        }
    }
    if model.entities[model.global].identifier.as_deref() == Some(name) {
        return Err(format!("a symbol named {name} is already visible, declared by the program itself"));
    }
    // @lfy def/query/main.lfy:192
    let mut edits = vec![Edit {
        range: span(workspace, s.node.file, name_token, name_token + 1),
        text: name.to_string(),
    }];
    for usage in usages {
        let usage = &model.usages[usage];
        // Decision: only a usage that spells the name changes; a usage through an alias
        // spells the alias, and a usage of a layer alone spells nothing.
        if usage.name.as_deref() != Some(s.name.as_str()) {
            continue;
        }
        if let Some(token) = usage.token {
            edits.push(Edit {
                range: span(workspace, usage.node.file, token, token + 1),
                text: name.to_string(),
            });
        }
    }
    // @lfy def/query/main.lfy:194
    edits.sort_by(|a, b| {
        (file_order(workspace, &a.range.file), &a.range.file, a.range.start).cmp(&(
            file_order(workspace, &b.range.file),
            &b.range.file,
            b.range.start,
        ))
    });
    edits.dedup();
    Ok(edits)
}

// ---- Find and source --------------------------------------------------------------

/// Entities by the name an agent would spell: an identifier, or an owner's identifier, a
/// dot, and a member's name; with a file path and a colon first, only that file's scope
/// is searched. Every entity whose symbol is named that way in any file scope, or whose
/// owner is, in the order [`find_symbols`] uses; empty when nothing matches.
// @lfy def/query/main.lfy:208
pub fn find(workspace: &Workspace, name: &str) -> Vec<EntityId> {
    let model = &workspace.model;
    // @lfy def/query/main.lfy:211
    let (files, name) = match name.split_once(':') {
        Some((path, rest)) => (file_of(workspace, path).into_iter().collect::<Vec<_>>(), rest),
        None => (files_in_search_order(workspace), name),
    };
    // @lfy def/query/main.lfy:209
    let (owner, member) = match name.split_once('.') {
        Some((owner, member)) => (owner, Some(member)),
        None => (name, None),
    };
    let mut out: Vec<EntityId> = Vec::new();
    for file in files {
        for &symbol in &model.scopes[model.file_scopes[file]].symbols {
            let s = &model.symbols[symbol];
            if s.name != owner {
                continue;
            }
            let found = match member {
                None => vec![s.entity],
                Some(member) => model
                    .members(s.entity)
                    .into_iter()
                    .filter(|&m| model.symbols[m].name == member)
                    .map(|m| model.symbols[m].entity)
                    .collect(),
            };
            for entity in found {
                if !out.contains(&entity) {
                    out.push(entity);
                }
            }
        }
    }
    out // @lfy def/query/main.lfy:213
}

/// The text that declares an entity: the raw text of every token the declaring node
/// covers, with the text of its documentation before it; the whole file for the anonymous
/// entity of a file; the member statement inside the trait body for a member added by a
/// trait.
// @lfy def/query/main.lfy:216
pub fn source_of(workspace: &Workspace, entity: EntityId) -> String {
    let model = &workspace.model;
    let e = &model.entities[entity];
    let Some(node) = e.node.filter(|&node| is_real(model, node)) else {
        // Decision: `global` declares nothing in any file, so its source is empty.
        return String::new();
    };
    let tree = &model.sources[node.file].tree;
    if matches!(e.kind, EntityKind::File) {
        return tree.raw(0, tree.tokens.len()); // @lfy def/query/main.lfy:218
    }
    let tree_node = model.node(node);
    let mut out = String::new();
    // @lfy def/query/main.lfy:217
    for documentation in &tree_node.documentation {
        out.push_str(&tree.raw(documentation.start, documentation.end));
        out.push('\n');
    }
    out.push_str(&tree.raw(tree_node.start, tree_node.end));
    out
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::workspace;

    /// A project directory under the system's temporary directory, removed when dropped.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "elfie-query-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("def")).unwrap();
            Fixture { root }
        }

        /// A project with one file `def/a.lfy`.
        fn one(text: &str) -> Fixture {
            let fixture = Fixture::new();
            fixture.write("def/a.lfy", text);
            fixture
        }

        fn write(&self, path: &str, text: &str) -> &Fixture {
            let disk = self.root.join(path);
            fs::create_dir_all(disk.parent().unwrap()).unwrap();
            fs::write(disk, text).unwrap();
            self
        }

        fn load(&self) -> Workspace {
            workspace::load(&self.root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    const A: &str = "def/a.lfy";

    fn at(line: usize, column: usize) -> Position {
        Position::new(line, column)
    }

    /// A position some columns after another on the same line.
    fn after(position: Position, columns: usize) -> Position {
        Position::new(position.line, position.column + columns)
    }

    /// The position of the `occurrence`th (from 0) occurrence of `needle` in `text`.
    fn find_pos(text: &str, needle: &str, occurrence: usize) -> Position {
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

    fn range(file: &str, start: (usize, usize), end: (usize, usize)) -> Range {
        Range {
            file: file.to_string(),
            start: at(start.0, start.1),
            end: at(end.0, end.1),
        }
    }

    fn labels(completions: &[Completion]) -> Vec<&str> {
        completions.iter().map(|c| c.label.as_str()).collect()
    }

    /// The repository itself: every definition file, bound with zero problems.
    fn repository() -> Workspace {
        workspace::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")))
    }

    // @lfy def/query/main.lfy:24
    #[test]
    fn a_range_runs_from_the_first_token_to_just_after_the_last() {
        let fixture = Fixture::one("/** Block\n doc **/\nd A {}\n");
        let ws = fixture.load();
        let id = ws.model.file(A).unwrap();
        // A token holding a line break ends on the next line.
        assert_eq!(range_of(&ws, id, NodeOrToken::Token(1)), range(A, (1, 3), (2, 5)));
        assert_eq!(range_of(&ws, id, NodeOrToken::Token(0)), range(A, (1, 0), (1, 3)));
        let root = &ws.model.sources[id].tree.root;
        assert_eq!(range_of(&ws, id, NodeOrToken::Node(root)), range(A, (1, 0), (4, 0)));
        let declaration = root.find(S::DataDeclaration).unwrap();
        assert_eq!(range_of(&ws, id, NodeOrToken::Node(declaration)), range(A, (3, 0), (3, 6)));
        let r = ws.model.node_ref(id, declaration).unwrap();
        assert_eq!(range_of_node(&ws, r), range(A, (3, 0), (3, 6)));
    }

    // @lfy def/query/main.lfy:28
    #[test]
    fn a_node_covering_no_token_gives_an_empty_range_where_it_would_start() {
        let fixture = Fixture::one("");
        let ws = fixture.load();
        let id = ws.model.file(A).unwrap();
        let root = &ws.model.sources[id].tree.root;
        assert_eq!(root.start, root.end);
        assert_eq!(range_of(&ws, id, NodeOrToken::Node(root)), Range::empty(A, at(1, 0)));
        // An empty error node at the end of the file sits after the last token.
        let fixture = Fixture::one("const y = A@");
        let ws = fixture.load();
        let id = ws.model.file(A).unwrap();
        let error = &ws.model.sources[id].tree.errors[0];
        assert_eq!(error.start, error.end);
        assert_eq!(span(&ws, id, error.start, error.end), Range::empty(A, at(1, 12)));
    }

    // @lfy def/query/main.lfy:39
    #[test]
    fn the_token_just_after_an_identifier_still_belongs_to_it() {
        let fixture = Fixture::one("const x = 1;");
        let ws = fixture.load();
        let index = token_at(&ws, A, at(1, 7)).unwrap();
        assert_eq!(token(&ws, 0, index).raw, "x");
        assert!(token(&ws, 0, index).is(I::Identifier));
        assert_eq!(token_at(&ws, A, at(2, 0)), None);
        assert_eq!(token_at(&ws, A, at(1, 13)), None);
        assert_eq!(token_at(&ws, "def/none.lfy", at(1, 0)), None);
        // Inside a token, and after a keyword.
        assert_eq!(token(&ws, 0, token_at(&ws, A, at(1, 2)).unwrap()).raw, "const");
        assert_eq!(token(&ws, 0, token_at(&ws, A, at(1, 5)).unwrap()).raw, "const");
        // Between a space and a setter the later token wins.
        assert_eq!(token(&ws, 0, token_at(&ws, A, at(1, 8)).unwrap()).raw, "=");
        // Just after a number the later token wins; at the very end nothing does.
        assert_eq!(token(&ws, 0, token_at(&ws, A, at(1, 11)).unwrap()).raw, ";");
        assert_eq!(token_at(&ws, A, at(1, 12)), None);
        // The end of the file after an identifier.
        let fixture = Fixture::one("const x = y");
        let ws = fixture.load();
        assert_eq!(token(&ws, 0, token_at(&ws, A, at(1, 11)).unwrap()).raw, "y");
    }

    // @lfy def/query/main.lfy:52
    #[test]
    fn nodes_at_walks_from_the_root_down_to_the_node_holding_the_token() {
        let fixture = Fixture::one("d A {} const y: `See [[A]]` = 1;");
        let ws = fixture.load();
        let model = &ws.model;
        let position = find_pos("d A {} const y: `See [[A]]` = 1;", "A]]", 0);
        let nodes = nodes_at(&ws, A, position);
        let rules: Vec<&str> = nodes.iter().map(|&r| model.info(r).rule.identifier()).collect();
        assert_eq!(
            rules,
            [
                "SourceFile",
                "VariableDeclaration",
                "Declared",
                "DefinitionClause",
                "TypeExpression",
                "TypeItem",
                "Template",
                "TemplateReference",
                "Reference",
                "Name"
            ]
        );
        assert_eq!(nodes[0], NodeRef { file: 0, index: 0 });
        for pair in nodes.windows(2) {
            assert_eq!(model.parent(pair[1]), Some(pair[0]));
        }
        let token = token_at(&ws, A, position).unwrap();
        let last = model.node(*nodes.last().unwrap());
        assert!(last.children.iter().any(|child| child.as_token() == Some(token)));
        assert!(nodes_at(&ws, A, at(9, 0)).is_empty());
    }

    // @lfy def/query/main.lfy:65
    #[test]
    fn a_name_in_code_and_in_a_template_reference_resolve_alike() {
        let text = "d A {} const y = A;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let symbol = symbol_at(&ws, A, find_pos(text, "A;", 0)).unwrap();
        assert_eq!(ws.model.symbols[symbol].name, "A");
        assert_eq!(ws.model.symbols[symbol].kind, SymbolKind::Data);
        // The declaration itself.
        assert_eq!(symbol_at(&ws, A, at(1, 2)), Some(symbol));
        assert_eq!(symbol_at(&ws, A, find_pos(text, "y", 0)).map(|s| ws.model.symbols[s].name.clone()), Some("y".to_string()));
        // Anything else.
        assert_eq!(symbol_at(&ws, A, find_pos(text, "const", 0)), None);
        assert_eq!(symbol_at(&ws, A, find_pos(text, "=", 0)), None);

        let text = "d A {} const y: `See [[A]]` = 1;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let symbol = symbol_at(&ws, A, find_pos(text, "A]]", 0)).unwrap();
        assert_eq!(ws.model.symbols[symbol].name, "A");
        assert_eq!(ws.model.symbols[symbol].kind, SymbolKind::Data);
        // On the reference's brackets the reference's usage still gives the symbol.
        assert_eq!(symbol_at(&ws, A, find_pos(text, "[[", 0)), Some(symbol));
        assert_eq!(symbol_at(&ws, A, find_pos(text, "]]", 0)), Some(symbol));
    }

    // @lfy def/query/main.lfy:62
    #[test]
    fn the_path_of_a_use_gives_its_module_symbol_or_nothing() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\" as B;\nuse \"./b\";\nconst y = B.B;\n")
            .write("def/b.lfy", "d B {}\n");
        let ws = fixture.load();
        let symbol = symbol_at(&ws, A, at(1, 6)).unwrap();
        assert_eq!(ws.model.symbols[symbol].kind, SymbolKind::Module);
        assert_eq!(ws.model.symbols[symbol].name, "B");
        assert_eq!(symbol_at(&ws, A, at(2, 6)), None);
        // The name after `as` and the module's member.
        assert_eq!(symbol_at(&ws, A, at(1, 13)), Some(symbol));
        assert_eq!(symbol_at(&ws, A, at(3, 10)), Some(symbol));
        let member = symbol_at(&ws, A, at(3, 12)).unwrap();
        assert_eq!(ws.model.symbols[member].kind, SymbolKind::Data);
        assert_eq!(ws.model.sources[ws.model.symbols[member].node.file].path, "def/b.lfy");
    }

    // @lfy def/query/main.lfy:76
    #[test]
    fn definition_is_the_identifier_of_the_declaration() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\" as B;\nuse \"./b\";\nalias C = B.B;\nconst y = C;\ntrait t { $m = string; }\nd A is t { $n = A$m; }\n")
            .write("def/b.lfy", "d B {}\n");
        let ws = fixture.load();
        assert!(ws.problems.is_empty(), "{:?}", ws.problems);
        // A data declared in another file.
        assert_eq!(definition_of(&ws, A, at(3, 12)), Some(range("def/b.lfy", (1, 2), (1, 3))));
        // The path of a Use, and a module symbol, give the used file.
        assert_eq!(definition_of(&ws, A, at(1, 6)), Some(Range::empty("def/b.lfy", at(1, 0))));
        assert_eq!(definition_of(&ws, A, at(2, 6)), Some(Range::empty("def/b.lfy", at(1, 0))));
        assert_eq!(definition_of(&ws, A, at(3, 10)), Some(Range::empty("def/b.lfy", at(1, 0))));
        // An alias gives its own declaration.
        assert_eq!(definition_of(&ws, A, at(4, 10)), Some(range(A, (3, 6), (3, 7))));
        // A member added by a trait gives the statement in the trait body.
        assert_eq!(definition_of(&ws, A, at(6, 18)), Some(range(A, (5, 11), (5, 12))));
        // Nothing declared.
        assert_eq!(definition_of(&ws, A, at(4, 0)), None);
        assert_eq!(definition_of(&ws, A, at(20, 0)), None);
    }

    // @lfy def/query/main.lfy:84
    #[test]
    fn references_cover_every_usage_in_file_order() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\";\nconst y = B;\nconst z: `Is [[B]]` = B;\n")
            .write("def/b.lfy", "d B {}\nconst x = B;\n");
        let ws = fixture.load();
        let with = references_to(&ws, A, at(2, 10), true);
        assert_eq!(
            with,
            [
                range("def/b.lfy", (1, 2), (1, 3)),
                range("def/b.lfy", (2, 10), (2, 11)),
                range(A, (2, 10), (2, 11)),
                range(A, (3, 15), (3, 16)),
                range(A, (3, 22), (3, 23)),
            ]
        );
        let without = references_to(&ws, A, at(2, 10), false);
        assert_eq!(without, with[1..]);
        assert!(references_to(&ws, A, at(2, 0), true).is_empty());
    }

    // @lfy def/query/main.lfy:86
    #[test]
    fn references_to_a_trait_member_include_every_receiver() {
        let text = "trait t { $m = string; }\nd A is t { $n = A$m; $o = $m; }\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert!(ws.problems.is_empty(), "{:?}", ws.problems);
        let from_declaration = references_to(&ws, A, at(1, 11), true);
        assert_eq!(
            from_declaration,
            [range(A, (1, 11), (1, 12)), range(A, (2, 18), (2, 19)), range(A, (2, 27), (2, 28))]
        );
        assert_eq!(references_to(&ws, A, at(2, 18), true), from_declaration);
    }

    // @lfy def/query/main.lfy:110
    #[test]
    fn hover_shows_kind_identifier_definition_and_documentation() {
        let text = "/// Doc\nd A: `An A` {} const y = A;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let hover = hover_at(&ws, A, find_pos(text, "A;", 0)).unwrap();
        assert_eq!(hover.kind, "data");
        assert_eq!(hover.identifier, "A");
        assert_eq!(hover.definition.as_deref(), Some("An A"));
        assert_eq!(hover.documentation.as_deref(), Some("Doc"));
        assert!(hover.criteria.is_empty());
        assert_eq!(hover.range, range(A, (2, 25), (2, 26)));
        assert_eq!(hover_at(&ws, A, at(2, 12)), None);
    }

    // @lfy def/query/main.lfy:95
    #[test]
    fn hover_of_resolves_references_types_members_and_block_documentation() {
        let text = "/** Block\n doc **/\ntrait t { $m: `Of [[A]]` = string; }\nd A is t {}\nconst v: A = A;\nconst w = 1;\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let model = &ws.model;
        assert!(ws.problems.is_empty(), "{:?}", ws.problems);
        let t = find(&ws, "t")[0];
        let hover = hover_of(&ws, t, Range::empty(A, at(1, 0)));
        assert_eq!(hover.kind, "trait");
        assert_eq!(hover.documentation.as_deref(), Some("Block\ndoc"));
        assert_eq!(hover.range, Range::empty(A, at(1, 0)));
        let m = find(&ws, "t.m")[0];
        let hover = hover_of(&ws, m, Range::empty(A, at(1, 0)));
        assert_eq!(hover.kind, "member");
        assert_eq!(hover.identifier, "m");
        assert_eq!(hover.definition.as_deref(), Some("Of A"));
        assert_eq!(hover.ty.as_deref(), Some("string"));
        assert_eq!(hover.documentation, None);
        let v = find(&ws, "v")[0];
        let hover = hover_of(&ws, v, Range::empty(A, at(1, 0)));
        assert_eq!(hover.kind, "variable");
        assert_eq!(hover.ty.as_deref(), Some("A"));
        assert_eq!(hover.definition, None);
        // The anonymous entity of the file is named by its first line.
        let hover = hover_of(&ws, model.file_entities[0], Range::empty(A, at(1, 0)));
        assert_eq!(hover.kind, "file");
        assert_eq!(hover.identifier, "/** Block");
        // The global entity has no node to read.
        let hover = hover_of(&ws, model.global, Range::empty(A, at(1, 0)));
        assert_eq!(hover.identifier, "global");
        assert_eq!(hover.documentation, None);
    }

    // @lfy def/query/main.lfy:133
    #[test]
    fn after_the_context_accessor_every_context_property_is_offered() {
        let fixture = Fixture::one("d A { $x = string; } const y = A@");
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(1, 33));
        assert_eq!(completions.len(), ContextProperty::ALL.len());
        let names = labels(&completions);
        assert!(names.contains(&"identifier"));
        assert!(names.contains(&"definition"));
        assert!(!names.contains(&"A"));
        assert!(!names.contains(&"y"));
        assert!(completions.iter().all(|c| c.kind == "context"));
        // The typed prefix filters, case sensitive.
        let fixture = Fixture::one("d A { $x = string; } const y = A@de");
        let ws = fixture.load();
        assert_eq!(labels(&completions_at(&ws, A, at(1, 35))), ["definition"]);
        let fixture = Fixture::one("d A { $x = string; } const y = A@De");
        let ws = fixture.load();
        assert!(completions_at(&ws, A, at(1, 35)).is_empty());
    }

    // @lfy def/query/main.lfy:137
    #[test]
    fn after_is_every_visible_trait_is_offered() {
        let fixture = Fixture::one("trait t {} d A is ");
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(1, 18));
        assert_eq!(completions.len(), 1, "{completions:?}");
        assert_eq!(completions[0].label, "t");
        assert_eq!(completions[0].kind, "trait");
        // Also after a comma inside trait uses, and after extends; modules are offered too.
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\" as B;\ntrait t {} trait u extends t, \nd A is t, ")
            .write("def/b.lfy", "");
        let ws = fixture.load();
        assert_eq!(labels(&completions_at(&ws, A, at(2, 30))), ["t", "u", "B"]);
        assert_eq!(labels(&completions_at(&ws, A, at(3, 10))), ["t", "u", "B"]);
        assert_eq!(completions_at(&ws, A, at(3, 10))[2].kind, "module");
    }

    // @lfy def/query/main.lfy:121
    #[test]
    fn accessors_offer_members_of_what_they_reach() {
        let text = "d A { $x: `X` = string; $y = number; }\nenum E { one = 1, two = 2 }\nd B { $z = A.; $w = $; $v = $&; $u = E.; $t = q.; $s = A$; }\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let value = completions_at(&ws, A, after(find_pos(text, "A.;", 0), 2));
        assert_eq!(labels(&value), ["x", "y"]);
        assert_eq!(value[0].kind, "member");
        assert_eq!(value[0].detail.as_deref(), Some("X"));
        assert_eq!(labels(&completions_at(&ws, A, after(find_pos(text, "$;", 0), 1))), ["z", "w", "v", "u", "t", "s"]);
        assert!(completions_at(&ws, A, after(find_pos(text, "$&;", 0), 2)).is_empty());
        assert_eq!(labels(&completions_at(&ws, A, after(find_pos(text, "E.;", 0), 2))), ["one", "two"]);
        assert!(completions_at(&ws, A, after(find_pos(text, "q.;", 0), 2)).is_empty());
        assert_eq!(labels(&completions_at(&ws, A, after(find_pos(text, "A$;", 0), 2))), ["x", "y"]);
        // At the top of a data the parent scope reaches the file's entity, which has no
        // members; a nested data reaches its owner's.
        let text = "d A { $x = string; d C { $y = $&; } }\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert_eq!(labels(&completions_at(&ws, A, after(find_pos(text, "$&;", 0), 2))), ["x"]);
    }

    // @lfy def/query/main.lfy:123
    #[test]
    fn a_module_offers_its_own_symbols_after_the_value_accessor() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\" as B;\nconst y = B.;\n")
            .write("def/b.lfy", "use \"./c\";\nd B {}\nconst k = 1;\n")
            .write("def/c.lfy", "d C {}\n");
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(2, 12));
        assert_eq!(labels(&completions), ["B", "k"]);
        assert_eq!(completions[0].kind, "data");
        assert_eq!(completions[1].kind, "variable");
    }

    // @lfy def/query/main.lfy:125
    #[test]
    fn inside_the_string_of_a_use_paths_are_offered() {
        let fixture = Fixture::new();
        fixture
            .write("elfie.json", r#"{ "dependencies": { "rust": { "root": "targets/rust" } } }"#)
            .write("def/a.lfy", "use \"./\";\nuse \"\";\nuse \"rust/\";\nuse \"./sub/\";\nuse \"./s\";\n")
            .write("def/b.lfy", "")
            .write("def/sub/inner.lfy", "")
            .write("def/notes.txt", "")
            .write("targets/rust/main.lfy", "")
            .write("targets/rust/guidance.lfy", "");
        let ws = fixture.load();
        let dotted = completions_at(&ws, A, at(1, 7));
        assert_eq!(labels(&dotted), ["a", "b", "sub"]);
        assert!(dotted.iter().all(|c| c.kind == "path"));
        assert_eq!(labels(&completions_at(&ws, A, at(2, 5))), ["rust"]);
        assert_eq!(labels(&completions_at(&ws, A, at(3, 10))), ["guidance", "main"]);
        assert_eq!(labels(&completions_at(&ws, A, at(4, 11))), ["inner"]);
        // The last segment typed filters.
        assert_eq!(labels(&completions_at(&ws, A, at(5, 8))), ["sub"]);
    }

    // @lfy def/query/main.lfy:126
    #[test]
    fn inside_a_reference_names_then_rule_entities_are_offered() {
        let text = "trait rule {}\nd Identifier is rule {}\nd A: `See [[A]]` {}\n/// Docs [[y]]\nconst y = 1;\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let in_template = completions_at(&ws, A, after(find_pos(text, "[[A]]", 0), 2));
        assert_eq!(labels(&in_template), ["rule", "Identifier", "A", "y", "global"]);
        assert_eq!(in_template[1].kind, "data");
        // Typing a name inside a documentation reference filters the same list.
        let in_documentation = completions_at(&ws, A, after(find_pos(text, "[[y]]", 0), 3));
        assert_eq!(labels(&in_documentation), ["y"]);
        // Every rule entity is offered even when it is not visible from the position.
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "trait rule {}\nd Identifier is rule {}\n")
            .write("def/b.lfy", "const y: `See [[I` = 1;\n");
        let ws = fixture.load();
        let far = completions_at(&ws, "def/b.lfy", at(1, 17));
        assert_eq!(labels(&far), ["Identifier"]);
    }

    // @lfy def/query/main.lfy:127
    #[test]
    fn after_the_colon_of_a_definition_types_are_offered() {
        let text = "d A {}\ntrait t {}\nenum E {}\ntype T {}\nconst v = 1;\nconst w: \nconst u: T = { k: };\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(6, 9));
        assert_eq!(
            labels(&completions),
            ["A", "t", "E", "T", "boolean", "number", "string", "object", "function", "trait"]
        );
        assert_eq!(completions[4].kind, "keyword");
        assert!(!labels(&completions).contains(&"v"));
    }

    // @lfy def/query/main.lfy:129
    #[test]
    fn where_a_statement_begins_names_then_statement_keywords_are_offered() {
        let text = "d A {}\nconst v = 1;\nfunction f(p: string) { const q = 2; \n}\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let top = completions_at(&ws, A, at(5, 0));
        let labels_top = labels(&top);
        assert!(labels_top.starts_with(&["A", "v", "f", "global", "d", "fn", "function", "trait"]), "{labels_top:?}");
        assert!(labels_top.contains(&"const") && labels_top.contains(&"let") && labels_top.contains(&"matchall"));
        assert!(!labels_top.contains(&"q"));
        assert!(!labels_top.contains(&"true"));
        // Inside the function, the nearer scope comes first.
        let inner = completions_at(&ws, A, at(3, 37));
        assert!(labels(&inner).starts_with(&["q", "p", "A", "v", "f"]), "{:?}", labels(&inner));
        // At the start of an empty file.
        let fixture = Fixture::one("");
        let ws = fixture.load();
        assert!(labels(&completions_at(&ws, A, at(1, 0))).contains(&"const"));
    }

    // @lfy def/query/main.lfy:130
    #[test]
    fn where_an_expression_begins_names_then_value_and_type_keywords_are_offered() {
        let text = "d A {}\nconst w = v + ;\nconst v = \n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(3, 10));
        assert_eq!(
            labels(&completions),
            ["A", "w", "v", "global", "true", "false", "null", "undefined", "boolean", "number", "string", "object", "function", "trait"]
        );
        assert_eq!(completions[4].kind, "keyword");
        assert_eq!(labels(&completions_at(&ws, A, at(2, 14)))[..4], ["A", "w", "v", "global"]);
        // A typed prefix filters the names.
        assert_eq!(labels(&completions_at(&ws, A, at(2, 11))), ["v"]);
        // After a name nothing can begin.
        assert!(completions_at(&ws, A, at(2, 12)).is_empty());
        assert!(completions_at(&ws, "def/none.lfy", at(1, 0)).is_empty());
    }

    // @lfy def/query/main.lfy:151
    #[test]
    fn an_undeclared_name_is_a_binder_error_at_the_name() {
        let fixture = Fixture::one("const y = z;");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].stage, Stage::Binder);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].range, range(A, (1, 10), (1, 11)));
        assert!(diagnostics[0].message.contains('z'));
    }

    // @lfy def/query/main.lfy:155
    #[test]
    fn a_missing_name_is_a_parser_error_at_the_equals_sign() {
        let fixture = Fixture::one("const = 1;");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, Some(A));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].stage, Stage::Parser);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].range.start, at(1, 6));
        assert!(diagnostics[0].message.contains("Identifier"), "{}", diagnostics[0].message);
        assert!(diagnostics[0].message.contains('='), "{}", diagnostics[0].message);
        assert!(diagnostics_of(&ws, Some("def/other.lfy")).is_empty());
    }

    // @lfy def/query/main.lfy:142
    #[test]
    fn load_problems_are_placed_at_the_top_of_their_path_or_of_the_manifest() {
        let fixture = Fixture::new();
        fixture
            .write("elfie.json", r#"{ "dependencies": { "gone": { "root": "nowhere" } }, "targets": { "t": 3 } }"#)
            .write("def/a.lfy", "use \"./missing\";\n");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        assert!(diagnostics.iter().all(|d| d.stage == Stage::Loader && d.severity == Severity::Error), "{diagnostics:?}");
        let files: Vec<&str> = diagnostics.iter().map(|d| d.range.file.as_str()).collect();
        assert_eq!(files, ["elfie.json", "def/a.lfy", "nowhere"]);
        assert!(diagnostics.iter().all(|d| d.range.is_empty() && d.range.start == at(1, 0)));
        // A problem with no path goes to elfie.json whether or not it exists.
        let ws = workspace::load(&fixture.root.join("absent"));
        let diagnostics = diagnostics_of(&ws, None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range, Range::empty("elfie.json", at(1, 0)));
    }

    // @lfy def/query/main.lfy:144
    #[test]
    fn invalid_text_and_an_open_mode_are_lexer_errors() {
        let fixture = Fixture::one("const y = 1;\nconst z = `open");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        let lexer: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.stage == Stage::Lexer).collect();
        assert_eq!(lexer.len(), 1, "{diagnostics:?}");
        assert!(lexer[0].message.contains("template"), "{}", lexer[0].message);
        assert_eq!(lexer[0].range, Range::empty(A, at(2, 15)));
        let fixture = Fixture::one("const y = 1 # 2;");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        let lexer: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.stage == Stage::Lexer).collect();
        assert_eq!(lexer.len(), 1, "{diagnostics:?}");
        assert!(lexer[0].message.contains("invalid"), "{}", lexer[0].message);
        assert!(lexer[0].message.contains('#'), "{}", lexer[0].message);
        assert_eq!(lexer[0].range, range(A, (1, 12), (1, 13)));
        // Diagnostics are ordered by file then position.
        let positions: Vec<Position> = diagnostics.iter().map(|d| d.range.start).collect();
        let mut sorted = positions.clone();
        sorted.sort();
        assert_eq!(positions, sorted);
    }

    // @lfy def/query/main.lfy:147
    #[test]
    fn an_unused_use_is_a_binder_warning() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\";\nuse \"./c\" as C;\nuse \"./c\";\nconst z = C1;\n")
            .write("def/b.lfy", "d B {}\n")
            .write("def/c.lfy", "d C1 {}\n");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        let warnings: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        let lines: Vec<usize> = warnings.iter().map(|d| d.range.start.line).collect();
        assert_eq!(lines, [1, 2], "{warnings:?}");
        assert!(warnings.iter().all(|d| d.stage == Stage::Binder));
        assert!(warnings[0].message.contains("def/b.lfy"), "{}", warnings[0].message);
        assert!(warnings[1].message.contains('C'), "{}", warnings[1].message);
        assert_eq!(warnings[0].range, range(A, (1, 0), (1, 10)));
    }

    // @lfy def/query/main.lfy:152
    #[test]
    fn the_outline_nests_members_then_declarations_and_skips_parameters() {
        let text = "use \"./b\" as B;\n/// Doc\nd A is t { $x = string; fn f(p: string) { const q = 1; } $y = number; }\ntrait t { $m = string; }\nenum E { one = 1 }\nfunction g(r: number) { for (const i in [1]) { const s = i; } }\n";
        let fixture = Fixture::new();
        fixture.write("def/a.lfy", text).write("def/b.lfy", "d B {}\n");
        let ws = fixture.load();
        let outline = outline_of(&ws, A);
        let names: Vec<&str> = outline.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["A", "t", "E", "g"]);
        let a = &outline[0];
        assert_eq!(a.kind, "data");
        let end = text.lines().nth(2).unwrap().chars().count();
        assert_eq!(a.range, range(A, (2, 0), (3, end)));
        assert_eq!(a.selection_range, range(A, (3, 2), (3, 3)));
        let children: Vec<(&str, &str)> = a.children.iter().map(|o| (o.name.as_str(), o.kind.as_str())).collect();
        assert_eq!(children, [("x", "member"), ("y", "member"), ("f", "agentFunction")]);
        let f = &a.children[2];
        assert_eq!(f.children.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["q"]);
        assert_eq!(outline[2].children[0].name, "one");
        assert_eq!(outline[2].children[0].kind, "enumMember");
        assert_eq!(outline[3].children.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["s"]);
        assert!(outline_of(&ws, "def/none.lfy").is_empty());
    }

    // @lfy def/query/main.lfy:160
    #[test]
    fn find_symbols_flattens_dotted_names_across_files_own_first() {
        let fixture = Fixture::new();
        fixture
            .write("elfie.json", r#"{ "dependencies": { "p": { "root": "pkg" } } }"#)
            .write("def/a.lfy", "d Alpha { $beta = string; }\n")
            .write("def/b.lfy", "use \"./a\";\nconst gamma = Alpha;\n")
            .write("pkg/main.lfy", "d Beta {}\n");
        let ws = fixture.load();
        let names = |query: &str| -> Vec<String> { find_symbols(&ws, query).into_iter().map(|o| o.name).collect() };
        assert_eq!(names("beta"), ["Alpha.beta", "Beta"]);
        assert_eq!(names("ALPHA"), ["Alpha", "Alpha.beta"]);
        assert_eq!(names(""), ["Alpha", "gamma", "Beta"]);
        assert!(find_symbols(&ws, "").iter().all(|o| o.children.is_empty()));
        assert!(names("zzz").is_empty());
    }

    // @lfy def/query/main.lfy:176
    #[test]
    fn a_rename_edits_the_declaration_the_reference_and_the_value() {
        let text = "d A {} const y: `See [[A]]` = A;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let edits = rename_at(&ws, A, at(1, 2), "B").unwrap();
        assert_eq!(
            edits,
            [
                Edit { range: range(A, (1, 2), (1, 3)), text: "B".to_string() },
                Edit { range: range(A, (1, 23), (1, 24)), text: "B".to_string() },
                Edit { range: range(A, (1, 30), (1, 31)), text: "B".to_string() },
            ]
        );
        // From a usage the edits are the same.
        assert_eq!(rename_at(&ws, A, at(1, 30), "B").unwrap(), edits);
        // Documentation references are renamed too.
        let text = "d A {}\n/// See [[A]]\nconst y = 1;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let edits = rename_at(&ws, A, at(1, 2), "B").unwrap();
        assert_eq!(edits.len(), 2, "{edits:?}");
        assert_eq!(edits[1].range, range(A, (2, 10), (2, 11)));
    }

    // @lfy def/query/main.lfy:211
    #[test]
    fn a_rename_to_a_keyword_or_a_captured_name_is_refused() {
        let text = "d A {} const y = A;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let reason = rename_at(&ws, A, at(1, 2), "const").unwrap_err();
        assert!(reason.contains("const is a keyword"), "{reason}");
        let reason = rename_at(&ws, A, at(1, 2), "1x").unwrap_err();
        assert!(reason.contains("Identifier"), "{reason}");
        let reason = rename_at(&ws, A, at(1, 2), "y").unwrap_err();
        assert!(reason.contains("def/a.lfy:1"), "{reason}");
        let reason = rename_at(&ws, A, at(1, 5), "B").unwrap_err();
        assert!(reason.contains("nothing is declared"), "{reason}");
        // A name visible from a usage's scope captures too.
        let text = "d A {}\nfunction f() { const B = 1; const y = A; }";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert!(rename_at(&ws, A, at(1, 2), "B").is_err());
        assert!(rename_at(&ws, A, at(1, 2), "C").is_ok());
    }

    // @lfy def/query/main.lfy:173
    #[test]
    fn renaming_a_module_changes_the_name_after_as_and_its_usages_only() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\" as B;\nconst y = B.B;\nalias K = B.B;\nconst z = K;\n")
            .write("def/b.lfy", "d B {}\n");
        let ws = fixture.load();
        let edits = rename_at(&ws, A, at(1, 13), "M").unwrap();
        assert_eq!(
            edits,
            [
                Edit { range: range(A, (1, 13), (1, 14)), text: "M".to_string() },
                Edit { range: range(A, (2, 10), (2, 11)), text: "M".to_string() },
                Edit { range: range(A, (3, 10), (3, 11)), text: "M".to_string() },
            ]
        );
        // Renaming the data does not touch the alias's usages.
        let edits = rename_at(&ws, A, at(2, 12), "D").unwrap();
        let files: Vec<(&str, Position)> = edits.iter().map(|e| (e.range.file.as_str(), e.range.start)).collect();
        assert_eq!(files, [("def/b.lfy", at(1, 2)), (A, at(2, 12)), (A, at(3, 12))]);
    }

    // @lfy def/query/main.lfy:208
    #[test]
    fn find_takes_a_name_a_dotted_member_or_a_file_prefix() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "d A { $x = string; }\n")
            .write("def/b.lfy", "d A {}\nconst x = 1;\n");
        let ws = fixture.load();
        let model = &ws.model;
        let both = find(&ws, "A");
        assert_eq!(both.len(), 2);
        assert_eq!(model.entities[both[0]].file, model.file(A));
        assert_eq!(model.entities[both[1]].file, model.file("def/b.lfy"));
        let member = find(&ws, "A.x");
        assert_eq!(member.len(), 1);
        assert!(matches!(model.entities[member[0]].kind, EntityKind::Member));
        assert_eq!(find(&ws, "def/b.lfy:A"), vec![both[1]]);
        assert!(find(&ws, "def/b.lfy:A.x").is_empty());
        assert!(find(&ws, "nothing").is_empty());
        assert!(find(&ws, "def/none.lfy:A").is_empty());
    }

    // @lfy def/query/main.lfy:216
    #[test]
    fn source_is_the_declaration_with_its_documentation_or_the_whole_file() {
        let text = "/// Doc\nd A: `An A` {}\ntrait t { $m = string; }\nd B is t {}\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let model = &ws.model;
        assert_eq!(source_of(&ws, find(&ws, "A")[0]), "/// Doc\nd A: `An A` {}");
        assert_eq!(source_of(&ws, model.file_entities[0]), text);
        assert_eq!(source_of(&ws, find(&ws, "B.m")[0]), "$m = string;");
        assert_eq!(source_of(&ws, model.global), "");
    }

    // @lfy def/query/main.lfy:19
    #[test]
    fn the_repository_answers_every_query_without_errors() {
        let ws = repository();
        assert!(ws.files.len() >= 38, "{}", ws.files.len());
        let diagnostics = diagnostics_of(&ws, None);
        let errors: Vec<&Diagnostic> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "{errors:?}");
        // Every diagnostic is in files order.
        let keys: Vec<_> = diagnostics.iter().map(|d| diagnostic_key(&ws, d)).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);

        let main = "def/query/main.lfy";
        let outline = outline_of(&ws, main);
        let names: Vec<&str> = outline.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names[0], "rangeOf");
        assert_eq!(names.len(), 15);
        assert!(outline.iter().all(|o| o.kind == "agentFunction"));
        // Parameters are not outlined.
        assert!(outline[0].children.is_empty(), "{:?}", outline[0].children);

        // Hover on `Workspace` in the signature of rangeOf.
        let text = fs::read_to_string(ws.root.join(main)).unwrap();
        let position = after(find_pos(&text, "workspace: Workspace", 0), 11);
        let hover = hover_at(&ws, main, position).unwrap();
        assert_eq!(hover.identifier, "Workspace");
        assert_eq!(hover.kind, "data");
        assert!(hover.definition.is_some());
        assert_eq!(definition_of(&ws, main, position).unwrap().file, "def/workspace/data.lfy");
        assert!(references_to(&ws, main, position, false).len() > 15);

        // Hover on a criterion's reference in prose.
        let position = after(find_pos(&text, "[[Token.file]]", 0), 9);
        let hover = hover_at(&ws, main, position).unwrap();
        assert_eq!(hover.identifier, "file");
        assert_eq!(hover.kind, "member");

        // The entity of rangeOf carries its criteria.
        let range_of_entity = find(&ws, "rangeOf");
        assert_eq!(range_of_entity.len(), 1);
        let hover = hover_of(&ws, range_of_entity[0], Range::empty(main, at(1, 0)));
        assert_eq!(hover.criteria.len(), 4);
        assert!(source_of(&ws, range_of_entity[0]).starts_with("fn rangeOf("));
        assert_eq!(find(&ws, "def/lexer/data.lfy:Token.line").len(), 1);
        assert!(find_symbols(&ws, "tokenAt").iter().any(|o| o.name == "tokenAt"));

        // Completions in the definition's own text.
        let position = after(find_pos(&text, "@acceptanceCriteria", 0), 1);
        assert!(labels(&completions_at(&ws, main, position)).contains(&"acceptanceCriteria"));
        let position = after(find_pos(&text, "use \"./data\";", 0), 7);
        assert!(labels(&completions_at(&ws, main, position)).contains(&"data"));
    }
}
