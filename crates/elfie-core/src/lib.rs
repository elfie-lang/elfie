//! Elfie compiler crate root.
//!
//! Compiled output of `def/grammar` lives in [`grammar`], of `def/lexer` in [`lexer`], and
//! of `def/parser` in [`parser`].

pub mod grammar; // @lfy def/grammar/main.lfy:1
pub mod lexer; // @lfy def/lexer/main.lfy:lex
pub mod model; // @lfy def/model/main.lfy:bind
pub mod parser; // @lfy def/parser/main.lfy:parse
pub mod query; // @lfy def/query/main.lfy:rangeOf
pub mod workspace; // @lfy def/workspace/main.lfy:load

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::grammar::rules::expression::Expression;
use crate::grammar::rules::statement::Statement;
use crate::grammar::terminals::comment::Comment;
use crate::grammar::terminals::identifier::Identifier;
use crate::grammar::terminals::keyword::Keyword;
use crate::grammar::terminals::literal::Literal;
use crate::grammar::terminals::punctuation::Punctuation;
use crate::grammar::{Entity, GrammarRule};
use crate::lexer::Token;
use crate::model::{ContextProperty, Layer, Model, SymbolKind};
use crate::parser::{Child, Node};
use crate::workspace::Workspace;

/// `Position` advanced past `text`, counting every `NewLine` as ending a line.
fn advance_position(start: query::Position, text: &str) -> query::Position {
    let mut line = start.line;
    let mut column = start.column;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    query::Position { line, column }
}

fn pos_cmp(a: &query::Position, b: &query::Position) -> Ordering {
    (a.line, a.column).cmp(&(b.line, b.column))
}

fn pos_key(p: &query::Position) -> (usize, usize) {
    (p.line, p.column)
}

/// `Node | Token`: what [`range_of`] can be given.
// @lfy def/query/main.lfy:rangeOf
#[derive(Debug, Clone, Copy)]
pub enum NodeOrToken {
    Node(model::NodeRef),
    Token(model::FileId, usize),
}

/// Where a node or token sits.
// @lfy def/query/main.lfy:rangeOf
pub fn range_of(workspace: &Workspace, spot: NodeOrToken) -> query::Range {
    match spot {
        NodeOrToken::Token(file, index) => token_range(workspace, file, index, index + 1), // @lfy def/query/main.lfy:rangeOf
        NodeOrToken::Node(node_ref) => {
            let info = workspace.model.info(node_ref);
            token_range(workspace, node_ref.file, info.start, info.end) // @lfy def/query/main.lfy:rangeOf
        }
    }
}

/// The range covered by the tokens `[start, end)` of a file; empty at the position the
/// range would start at when it covers none.
// @lfy def/query/main.lfy:rangeOf
fn token_range(workspace: &Workspace, file: model::FileId, start: usize, end: usize) -> query::Range {
    let model = &workspace.model;
    let tokens = model.tokens(file);
    let path = model.sources[file].path.clone();
    if start >= end {
        let position = match tokens.get(start) {
            Some(token) => query::Position { line: token.line, column: token.column },
            None => tokens
                .last()
                .map(|token| advance_position(query::Position { line: token.line, column: token.column }, &token.raw))
                .unwrap_or(query::Position { line: 1, column: 0 }),
        };
        return query::Range { file: path, start: position.clone(), end: position };
    }
    let first = &tokens[start];
    let last = &tokens[end - 1];
    let start_position = query::Position { line: first.line, column: first.column };
    let end_position = advance_position(query::Position { line: last.line, column: last.column }, &last.raw);
    query::Range { file: path, start: start_position, end: end_position }
}

/// The index of the token whose range covers `position`, in `file`.
// @lfy def/query/main.lfy:tokenAt
fn token_index_at(model: &Model, file: model::FileId, position: &query::Position) -> Option<usize> {
    let tokens = model.tokens(file);
    for (index, token) in tokens.iter().enumerate() {
        let start = query::Position { line: token.line, column: token.column };
        if pos_cmp(position, &start) == Ordering::Less {
            break; // @lfy def/query/main.lfy:tokenAt
        }
        let end = advance_position(start, &token.raw);
        match pos_cmp(position, &end) {
            Ordering::Less => return Some(index), // @lfy def/query/main.lfy:tokenAt
            Ordering::Equal => {
                // A position exactly between two tokens belongs to the earlier one when it
                // is an Identifier, a Keyword, or a MemberName; otherwise to the later one.
                // @lfy def/query/main.lfy:tokenAt
                let earlier_wins = token
                    .rule
                    .is_some_and(|rule| rule.is_keyword() || rule == Entity::Identifier(Identifier::Identifier));
                if earlier_wins {
                    return Some(index); // @lfy def/query/main.lfy:tokenAt
                }
            }
            Ordering::Greater => {}
        }
    }
    None // @lfy def/query/main.lfy:tokenAt
}

/// The token under a position.
// @lfy def/query/main.lfy:tokenAt
pub fn token_at(workspace: &Workspace, file: &str, position: &query::Position) -> Option<Token> {
    let model = &workspace.model;
    let file_id = model.file(file)?; // @lfy def/query/main.lfy:tokenAt
    let index = token_index_at(model, file_id, position)?;
    Some(model.tokens(file_id)[index].clone())
}

/// Every node covering a position, outermost first.
// @lfy def/query/main.lfy:nodesAt
pub fn nodes_at(workspace: &Workspace, file: &str, position: &query::Position) -> Vec<Node> {
    let model = &workspace.model;
    let Some(file_id) = model.file(file) else { return Vec::new() };
    let Some(token_index) = token_index_at(model, file_id, position) else { return Vec::new() }; // @lfy def/query/main.lfy:nodesAt
    let mut path = Vec::new();
    let mut current = &model.sources[file_id].tree.root; // @lfy def/query/main.lfy:nodesAt
    loop {
        path.push(current.clone());
        let holds_directly = current.children.iter().any(|child| matches!(child, Child::Token(index) if *index == token_index));
        if holds_directly {
            break; // @lfy def/query/main.lfy:nodesAt
        }
        let next = current.children.iter().find_map(|child| match child {
            Child::Node(node) if node.start <= token_index && token_index < node.end => Some(node), // @lfy def/query/main.lfy:nodesAt
            _ => None,
        });
        match next {
            Some(node) => current = node,
            None => break,
        }
    }
    path
}

/// Whether a token spells the body of a `Use` path string.
fn is_use_path_token(token: &Token) -> bool {
    token.is(Literal::DoubleQuoteBody) || token.is(Literal::SingleQuoteBody)
}

/// The symbol declared or used at a position.
// @lfy def/query/main.lfy:symbolAt
pub fn symbol_at(workspace: &Workspace, file: &str, position: &query::Position) -> Option<model::SymbolId> {
    let model = &workspace.model;
    let file_id = model.file(file)?;
    let token_index = token_index_at(model, file_id, position)?;
    let token = &model.tokens(file_id)[token_index];
    if is_use_path_token(token) {
        // @lfy def/query/main.lfy:symbolAt
        let path = nodes_at(workspace, file, position);
        let use_node = path.iter().rev().find(|node| node.is(Statement::Use))?;
        let node_ref = model.node_ref(file_id, use_node)?;
        return model.symbol_of(node_ref);
    }
    if let Some(usage) = model.usages.iter().find(|usage| usage.node.file == file_id && usage.token == Some(token_index)) {
        return usage.symbol; // @lfy def/query/main.lfy:symbolAt
    }
    model
        .symbols
        .iter()
        .position(|symbol| symbol.node.file == file_id && symbol.name_token == Some(token_index)) // @lfy def/query/main.lfy:symbolAt
}

/// The rules a holder search never enters.
fn is_barrier(node: &Node) -> bool {
    node.is(Expression::Parameters) || node.is(Statement::Block) || node.is(Expression::Object) || node.is(Expression::Type)
}

/// The first `Identifier` token below a node, without entering a barrier.
fn find_identifier(node: &Node, tokens: &[Token]) -> Option<usize> {
    for child in &node.children {
        match child {
            Child::Token(index) => {
                if tokens[*index].is(Identifier::Identifier) {
                    return Some(*index);
                }
            }
            Child::Node(child_node) => {
                if is_barrier(child_node) {
                    continue;
                }
                if let Some(found) = find_identifier(child_node, tokens) {
                    return Some(found);
                }
            }
            Child::Error(_) => {}
        }
    }
    None
}

/// The token that spells the declared name of a node, as `declaring` finds it.
fn identifier_token(model: &Model, node_ref: model::NodeRef) -> Option<usize> {
    let node = model.node(node_ref);
    find_identifier(node, model.tokens(node_ref.file))
}

/// The file a `Use` node resolved to, and the empty range at its start.
fn use_target_range(workspace: &Workspace, use_node: model::NodeRef) -> Option<query::Range> {
    let model = &workspace.model;
    let source = &model.sources[use_node.file];
    let index = source
        .tree
        .root
        .descendants()
        .into_iter()
        .filter(|node| node.is(Statement::Use))
        .position(|node| model.node_ref(use_node.file, node) == Some(use_node))?;
    let target = source.uses.get(index)?.as_ref()?;
    let position = query::Position { line: 1, column: 0 };
    Some(query::Range { file: target.clone(), start: position.clone(), end: position })
}

/// Where the symbol at a position is declared.
// @lfy def/query/main.lfy:definitionOf
pub fn definition_of(workspace: &Workspace, file: &str, position: &query::Position) -> Option<query::Range> {
    let model = &workspace.model;
    let file_id = model.file(file)?;
    let token_index = token_index_at(model, file_id, position)?;
    let token = &model.tokens(file_id)[token_index];
    if is_use_path_token(token) {
        // @lfy def/query/main.lfy:definitionOf
        let path = nodes_at(workspace, file, position);
        let use_node = path.iter().rev().find(|node| node.is(Statement::Use))?;
        let node_ref = model.node_ref(file_id, use_node)?;
        return use_target_range(workspace, node_ref);
    }
    let symbol_id = symbol_at(workspace, file, position)?;
    let symbol = &model.symbols[symbol_id];
    if symbol.kind == SymbolKind::Module {
        return use_target_range(workspace, symbol.node); // @lfy def/query/main.lfy:definitionOf
    }
    // An alias resolves to the `AliasDeclaration` itself, and a member added by a trait to
    // the member statement inside the trait body, because `symbol.node` always names where
    // the symbol itself was written. @lfy def/query/main.lfy:definitionOf
    let name_token = identifier_token(model, symbol.node)?;
    Some(range_of(workspace, NodeOrToken::Token(symbol.node.file, name_token)))
}

/// Every place the symbol at a position is used.
// @lfy def/query/main.lfy:referencesTo
pub fn references_to(workspace: &Workspace, file: &str, position: &query::Position, include_declaration: bool) -> Vec<query::Range> {
    let Some(symbol_id) = symbol_at(workspace, file, position) else { return Vec::new() }; // @lfy def/query/main.lfy:referencesTo
    let model = &workspace.model;
    let mut out = Vec::new();
    if include_declaration
        && let Some(range) = definition_of(workspace, file, position)
    {
        out.push(range); // @lfy def/query/main.lfy:referencesTo
    }
    for usage_id in model::usages_of(model, symbol_id) {
        let usage = &model.usages[usage_id];
        let token_index = usage.token.unwrap_or_else(|| model.info(usage.node).start);
        out.push(range_of(workspace, NodeOrToken::Token(usage.node.file, token_index))); // @lfy def/query/main.lfy:referencesTo
    }
    out
}

/// The text that declares an entity.
// @lfy def/query/main.lfy:sourceOf
pub fn source_of(workspace: &Workspace, entity: model::EntityId) -> String {
    let model = &workspace.model;
    let entity_ref = &model.entities[entity];
    if matches!(entity_ref.kind, model::EntityKind::File)
        && let Some(file_id) = entity_ref.file
    {
        // @lfy def/query/main.lfy:sourceOf
        return model.tokens(file_id).iter().map(|token| token.raw.as_str()).collect();
    }
    let Some(node_ref) = entity_ref.node else { return String::new() };
    let node = model.node(node_ref);
    let tokens = model.tokens(node_ref.file);
    let mut text = String::new();
    for doc in &node.documentation {
        text.push_str(&tokens[doc.start..doc.end].iter().map(|token| token.raw.as_str()).collect::<String>()); // @lfy def/query/main.lfy:sourceOf
    }
    text.push_str(&model.raw(node_ref));
    text
}

/// The kind of an entity's symbol; `member` when the entity is a member.
fn hover_kind(model: &Model, entity: model::EntityId) -> SymbolKind {
    let entity_ref = &model.entities[entity];
    if let Some(symbol_id) = entity_ref.symbol {
        return model.symbols[symbol_id].kind;
    }
    match entity_ref.kind {
        model::EntityKind::Member => SymbolKind::Member,
        model::EntityKind::EnumMember => SymbolKind::EnumMember,
        _ => SymbolKind::Data,
    }
}

/// The joined text of the documented nodes of a declaration, each line without its opener
/// and closer and without one leading space.
fn documentation_text(model: &Model, node_ref: model::NodeRef) -> Option<String> {
    let node = model.node(node_ref);
    if node.documentation.is_empty() {
        return None;
    }
    let tokens = model.tokens(node_ref.file);
    let mut lines = Vec::new();
    for doc in &node.documentation {
        let raw: String = tokens[doc.start..doc.end].iter().map(|token| token.raw.as_str()).collect();
        let body = if let Some(inner) = raw.strip_prefix("/**").and_then(|rest| rest.strip_suffix("**/")) {
            inner.to_string()
        } else if let Some(inner) = raw.strip_prefix("///") {
            inner.to_string()
        } else {
            raw
        };
        for line in body.split('\n') {
            lines.push(line.strip_prefix(' ').unwrap_or(line).to_string());
        }
    }
    Some(lines.join("\n"))
}

/// Everything shown for one entity.
// @lfy def/query/main.lfy:hoverOf
pub fn hover_of(workspace: &Workspace, entity: model::EntityId) -> query::Hover {
    let model = &workspace.model;
    let entity_ref = &model.entities[entity];
    let kind = hover_kind(model, entity); // @lfy def/query/main.lfy:hoverOf
    let (range, identifier) = match entity_ref.node {
        Some(node_ref) => {
            let range = match identifier_token(model, node_ref) {
                Some(token_index) => range_of(workspace, NodeOrToken::Token(node_ref.file, token_index)), // @lfy def/query/main.lfy:hoverOf
                None => range_of(workspace, NodeOrToken::Node(node_ref)),
            };
            let identifier = entity_ref
                .identifier
                .clone()
                .unwrap_or_else(|| model.raw(node_ref).lines().next().unwrap_or_default().to_string()); // @lfy def/query/main.lfy:hoverOf
            (range, identifier)
        }
        None => {
            let position = query::Position { line: 1, column: 0 };
            (query::Range { file: String::new(), start: position.clone(), end: position }, entity_ref.identifier.clone().unwrap_or_default())
        }
    };
    let definition = entity_ref.definition.as_deref().map(model::strip_references); // @lfy def/query/main.lfy:hoverOf
    let ty = entity_ref.ty.as_ref().map(|ty| model::type_text(model, ty)); // @lfy def/query/main.lfy:hoverOf
    let documentation = entity_ref.node.and_then(|node_ref| documentation_text(model, node_ref)); // @lfy def/query/main.lfy:hoverOf
    let criteria = model::criteria_of(model, entity); // @lfy def/query/main.lfy:hoverOf
    query::Hover { range, kind, identifier, definition, ty, documentation, criteria }
}

/// Everything shown for the symbol at a position.
// @lfy def/query/main.lfy:hoverAt
pub fn hover_at(workspace: &Workspace, file: &str, position: &query::Position) -> Option<query::Hover> {
    let symbol_id = symbol_at(workspace, file, position)?; // @lfy def/query/main.lfy:hoverAt
    let entity = workspace.model.symbols[symbol_id].entity;
    Some(hover_of(workspace, entity)) // @lfy def/query/main.lfy:hoverAt
}

fn empty_range(path: &str) -> query::Range {
    let position = query::Position { line: 1, column: 0 };
    query::Range { file: path.to_string(), start: position.clone(), end: position }
}

/// Every problem of the program, placed.
// @lfy def/query/main.lfy:diagnosticsOf
pub fn diagnostics_of(workspace: &Workspace, file: Option<&str>) -> Vec<query::Diagnostic> {
    let model = &workspace.model;
    let mut out = Vec::new();
    for problem in workspace.load_problems() {
        // @lfy def/query/main.lfy:diagnosticsOf
        let path = problem.path.clone().unwrap_or_else(|| "elfie.json".to_string());
        if file.is_some_and(|f| f != path) {
            continue;
        }
        out.push(query::Diagnostic {
            range: empty_range(&path),
            severity: query::Severity::Error,
            stage: query::Stage::Loader,
            message: problem.message.clone(),
        });
    }
    for (file_id, source) in model.sources.iter().enumerate() {
        if file.is_some_and(|f| f != source.path) {
            continue;
        }
        let mut file_diags = Vec::new();
        for (index, token) in source.tree.tokens.iter().enumerate() {
            if token.rule.is_none() {
                // @lfy def/query/main.lfy:diagnosticsOf
                let message = if token.raw.is_empty() {
                    "the file ends while a region is still open".to_string()
                } else {
                    format!("invalid text {:?}", token.raw)
                };
                file_diags.push(query::Diagnostic {
                    range: token_range(workspace, file_id, index, index + 1),
                    severity: query::Severity::Error,
                    stage: query::Stage::Lexer,
                    message,
                });
            }
        }
        for error in &source.tree.errors {
            // @lfy def/query/main.lfy:diagnosticsOf
            let text = source.tree.raw(error.start, error.end);
            let message = format!("expected {}, found {:?}", error.expected.join(" or "), text);
            file_diags.push(query::Diagnostic {
                range: token_range(workspace, file_id, error.start, error.end),
                severity: query::Severity::Error,
                stage: query::Stage::Parser,
                message,
            });
        }
        for problem in model.problems.iter().filter(|problem| problem.node.file == file_id) {
            // @lfy def/query/main.lfy:diagnosticsOf
            file_diags.push(query::Diagnostic {
                range: range_of(workspace, NodeOrToken::Node(problem.node)),
                severity: query::Severity::Error,
                stage: query::Stage::Binder,
                message: problem.message.clone(),
            });
        }
        for use_node in source.tree.root.descendants().into_iter().filter(|node| node.is(Statement::Use)) {
            // @lfy def/query/main.lfy:diagnosticsOf
            let Some(node_ref) = model.node_ref(file_id, use_node) else { continue };
            if let Some(module_symbol) = model.symbol_of(node_ref)
                && model::usages_of(model, module_symbol).is_empty()
            {
                file_diags.push(query::Diagnostic {
                    range: range_of(workspace, NodeOrToken::Node(node_ref)),
                    severity: query::Severity::Warning,
                    stage: query::Stage::Binder,
                    message: "this use is never used".to_string(),
                });
            }
        }
        file_diags.sort_by_key(|diagnostic| pos_key(&diagnostic.range.start));
        out.extend(file_diags); // @lfy def/query/main.lfy:diagnosticsOf
    }
    out
}

fn declaration_range(workspace: &Workspace, node_ref: model::NodeRef) -> query::Range {
    let model = &workspace.model;
    let node = model.node(node_ref);
    let info = model.info(node_ref);
    let start = node.documentation.first().map(|doc| doc.start).unwrap_or(info.start);
    token_range(workspace, node_ref.file, start, info.end)
}

/// The declarations of one file, nested as written.
// @lfy def/query/main.lfy:outlineOf
pub fn outline_of(workspace: &Workspace, file: &str) -> Vec<query::Outline> {
    let model = &workspace.model;
    let Some(file_id) = model.file(file) else { return Vec::new() };
    let scope = model.file_scopes[file_id];
    model.scopes[scope]
        .symbols
        .iter()
        .filter(|&&symbol_id| model.symbols[symbol_id].kind != SymbolKind::Module) // @lfy def/query/main.lfy:outlineOf
        .map(|&symbol_id| build_outline(workspace, symbol_id))
        .collect()
}

fn build_outline(workspace: &Workspace, symbol_id: model::SymbolId) -> query::Outline {
    let model = &workspace.model;
    let symbol = &model.symbols[symbol_id];
    let node_ref = symbol.node;
    let range = declaration_range(workspace, node_ref); // @lfy def/query/main.lfy:outlineOf
    let selection_range = match symbol.name_token {
        Some(index) => range_of(workspace, NodeOrToken::Token(node_ref.file, index)), // @lfy def/query/main.lfy:outlineOf
        None => range.clone(),
    };
    let children = model
        .members(symbol.entity)
        .into_iter()
        .filter(|&member_id| {
            // Parameters and loop variables are left out. @lfy def/query/main.lfy:outlineOf
            let kind = model.symbols[member_id].kind;
            kind != SymbolKind::Parameter && kind != SymbolKind::LoopVariable && kind != SymbolKind::Module
        })
        .map(|member_id| build_outline(workspace, member_id))
        .collect();
    query::Outline { name: symbol.name.clone(), kind: symbol.kind, range, selection_range, children }
}

/// Files without a package first, then package files, each in `Workspace.files` order.
fn ordered_files(workspace: &Workspace) -> Vec<&crate::workspace::File> {
    let mut own: Vec<&crate::workspace::File> = workspace.files.iter().filter(|file| file.package.is_none()).collect();
    let mut package: Vec<&crate::workspace::File> = workspace.files.iter().filter(|file| file.package.is_some()).collect();
    own.append(&mut package);
    own
}

fn collect_matches(outline: &query::Outline, prefix: &str, query: &str, out: &mut Vec<query::Outline>) {
    let full_name = if prefix.is_empty() { outline.name.clone() } else { format!("{prefix}.{}", outline.name) };
    if full_name.to_lowercase().contains(&query.to_lowercase()) {
        out.push(query::Outline { name: full_name.clone(), ..outline.clone() });
    }
    for child in &outline.children {
        collect_matches(child, &full_name, query, out);
    }
}

/// Declarations across the program whose name contains a text.
// @lfy def/query/main.lfy:findSymbols
pub fn find_symbols(workspace: &Workspace, query: &str) -> Vec<query::Outline> {
    let mut out = Vec::new();
    for file in ordered_files(workspace) {
        let outlines = outline_of(workspace, &file.path);
        if query.is_empty() {
            // @lfy def/query/main.lfy:findSymbols
            out.extend(outlines);
        } else {
            for outline in &outlines {
                collect_matches(outline, "", query, &mut out); // @lfy def/query/main.lfy:findSymbols
            }
        }
    }
    out
}

/// Entities by the name an agent would spell.
// @lfy def/query/main.lfy:find
pub fn find(workspace: &Workspace, name: &str) -> Vec<model::EntityId> {
    let model = &workspace.model;
    let (scope_file, rest) = match name.split_once(':') {
        Some((path, rest)) if path.ends_with(".lfy") => (model.file(path), rest), // @lfy def/query/main.lfy:find
        _ => (None, name),
    };
    let (owner, leaf) = match rest.split_once('.') {
        Some((owner, leaf)) => (Some(owner), leaf), // @lfy def/query/main.lfy:find
        None => (None, rest),
    };
    let mut out = Vec::new();
    for file in ordered_files(workspace) {
        if let Some(only) = scope_file
            && file.source != only
        {
            continue; // @lfy def/query/main.lfy:find
        }
        let scope = model.file_scopes[file.source];
        for &symbol_id in &model.scopes[scope].symbols {
            let symbol = &model.symbols[symbol_id];
            match owner {
                None => {
                    if symbol.name == leaf {
                        out.push(symbol.entity); // @lfy def/query/main.lfy:find
                    }
                }
                Some(owner_name) => {
                    if symbol.name == owner_name {
                        for member_id in model.members(symbol.entity) {
                            if model.symbols[member_id].name == leaf {
                                out.push(model.symbols[member_id].entity); // @lfy def/query/main.lfy:find
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// Every symbol visible from a scope, nearer scopes first, each name once.
fn visible_symbols(model: &Model, scope: model::ScopeId) -> Vec<model::SymbolId> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut current = Some(scope);
    while let Some(scope_id) = current {
        for &symbol_id in model.scopes[scope_id].symbols.iter().chain(model.scopes[scope_id].imports.iter()) {
            if seen.insert(model.symbols[symbol_id].name.clone()) {
                out.push(symbol_id);
            }
        }
        current = model.scopes[scope_id].parent;
    }
    out
}

/// The identifier characters already typed before a position, when the position is inside
/// an `Identifier` token.
fn typed_identifier_prefix(tokens: &[Token], position: &query::Position) -> Option<String> {
    for token in tokens {
        if !token.is(Identifier::Identifier) || token.line != position.line {
            continue;
        }
        let start = query::Position { line: token.line, column: token.column };
        let end = advance_position(start.clone(), &token.raw);
        if pos_cmp(&start, position) != Ordering::Greater && pos_cmp(position, &end) != Ordering::Greater {
            let offset = position.column.saturating_sub(token.column);
            return Some(token.raw.chars().take(offset).collect());
        }
    }
    None
}

fn dedup_completions(completions: &mut Vec<query::Completion>) {
    let mut seen = HashSet::new();
    completions.retain(|completion| seen.insert(completion.label.clone()));
}

/// What could be typed at a position.
// @lfy def/query/main.lfy:completionsAt
pub fn completions_at(workspace: &Workspace, file: &str, position: &query::Position) -> Vec<query::Completion> {
    let model = &workspace.model;
    let Some(file_id) = model.file(file) else { return Vec::new() };
    let tokens = model.tokens(file_id);

    // The context is decided by the nearest non-trivia token before the position.
    // @lfy def/query/main.lfy:completionsAt
    let mut before: Option<usize> = None;
    for (index, token) in tokens.iter().enumerate() {
        let start = query::Position { line: token.line, column: token.column };
        if pos_cmp(&start, position) != Ordering::Less {
            break;
        }
        if token.rule.is_some_and(|rule| !crate::parser::components::is_trivia(rule)) {
            before = Some(index);
        }
    }

    let scope = {
        let path = nodes_at(workspace, file, position);
        path.iter()
            .rev()
            .find_map(|node| model.node_ref(file_id, node))
            .map(|node_ref| model.enclosing_scope(node_ref))
            .unwrap_or(model.file_scopes[file_id])
    };

    let mut completions: Vec<query::Completion> = Vec::new();
    if let Some(index) = before {
        let token = &tokens[index];
        if token.is(Punctuation::ContextAccessor) {
            // @lfy def/query/main.lfy:completionsAt
            for property in ContextProperty::ALL {
                completions.push(query::Completion {
                    label: property.value().to_string(),
                    kind: query::SymbolOrCompletionKind::Completion(query::CompletionKind::Context),
                    detail: None,
                });
            }
        } else if token.is(Punctuation::ScopeAccessor) {
            // @lfy def/query/main.lfy:completionsAt
            let current = model.scopes[scope].current;
            for member_id in model.members(current) {
                completions.push(symbol_completion(model, member_id));
            }
        } else if token.is(Punctuation::ParentScopeAccessor) {
            // @lfy def/query/main.lfy:completionsAt
            if let Some(parent) = model.scopes[scope].parent {
                let current = model.scopes[parent].current;
                for member_id in model.members(current) {
                    completions.push(symbol_completion(model, member_id));
                }
            }
        } else if token.is(Keyword::IsKeyword) || token.is(Keyword::ExtendsKeyword) || token.is(Punctuation::Comma) {
            // @lfy def/query/main.lfy:completionsAt
            for symbol_id in visible_symbols(model, scope) {
                let symbol = &model.symbols[symbol_id];
                if symbol.kind == SymbolKind::Trait || symbol.kind == SymbolKind::Module {
                    completions.push(symbol_completion(model, symbol_id));
                }
            }
        } else if token.is(Punctuation::Colon) {
            // @lfy def/query/main.lfy:completionsAt
            for symbol_id in visible_symbols(model, scope) {
                let kind = model.symbols[symbol_id].kind;
                if matches!(kind, SymbolKind::Data | SymbolKind::Type | SymbolKind::Enum | SymbolKind::Trait) {
                    completions.push(symbol_completion(model, symbol_id));
                }
            }
            for keyword in ["boolean", "number", "string", "object", "function", "trait"] {
                completions.push(query::Completion { label: keyword.to_string(), kind: query::SymbolOrCompletionKind::Completion(query::CompletionKind::Keyword), detail: None });
            }
        } else {
            // A statement or an expression can begin here: every name visible, plus
            // keywords. @lfy def/query/main.lfy:completionsAt
            for symbol_id in visible_symbols(model, scope) {
                completions.push(symbol_completion(model, symbol_id));
            }
        }
    } else {
        for symbol_id in visible_symbols(model, scope) {
            completions.push(symbol_completion(model, symbol_id));
        }
    }

    if let Some(prefix) = typed_identifier_prefix(tokens, position) {
        completions.retain(|completion| completion.label.starts_with(prefix.as_str())); // @lfy def/query/main.lfy:completionsAt
    }
    dedup_completions(&mut completions);
    completions
}

fn symbol_completion(model: &Model, symbol_id: model::SymbolId) -> query::Completion {
    let symbol = &model.symbols[symbol_id];
    let detail = model.entities[symbol.entity].definition.clone();
    query::Completion { label: symbol.name.clone(), kind: query::SymbolOrCompletionKind::Symbol(symbol.kind), detail }
}

/// The edits that rename the symbol at a position everywhere, or why it cannot be renamed.
// @lfy def/query/main.lfy:renameAt
pub fn rename_at(workspace: &Workspace, file: &str, position: &query::Position, name: &str) -> Result<Vec<query::Edit>, String> {
    let model = &workspace.model;
    let Some(symbol_id) = symbol_at(workspace, file, position) else {
        return Err("nothing is declared here".to_string()); // @lfy def/query/main.lfy:renameAt
    };
    let tokens = crate::lexer::lex(name, None).map_err(|error| format!("{name:?} is not a valid name: {error}"))?;
    if tokens.len() != 1 || !tokens[0].is(Identifier::Identifier) {
        // @lfy def/query/main.lfy:renameAt
        let rule_name = tokens.first().and_then(|token| token.rule).map(GrammarRule::identifier).unwrap_or("text");
        return Err(format!("{name:?} is {} {rule_name}, not an identifier", if tokens.len() == 1 { "the keyword" } else { "not" }));
    }
    let symbol = &model.symbols[symbol_id];
    if let Some(existing) = model.lookup(symbol.scope, name)
        && existing != symbol_id
    {
        // @lfy def/query/main.lfy:renameAt
        let node = model.symbols[existing].node;
        let path = &model.sources[node.file].path;
        let line = model.tokens(node.file).get(model.info(node).start).map_or(1, |token| token.line);
        return Err(format!("{name} is already visible, declared at {path}:{line}"));
    }
    let mut edits = Vec::new();
    if let Some(index) = symbol.name_token {
        // Only the name after `as`, or the identifier itself, changes; the file is not
        // renamed. @lfy def/query/main.lfy:renameAt
        edits.push(query::Edit { range: range_of(workspace, NodeOrToken::Token(symbol.node.file, index)), text: name.to_string() });
    }
    for usage_id in model::usages_of(model, symbol_id) {
        // @lfy def/query/main.lfy:renameAt
        let usage = &model.usages[usage_id];
        if let Some(index) = usage.token {
            edits.push(query::Edit { range: range_of(workspace, NodeOrToken::Token(usage.node.file, index)), text: name.to_string() });
        }
    }
    edits.sort_by_key(|edit| (edit.range.file.clone(), pos_key(&edit.range.start))); // @lfy def/query/main.lfy:renameAt
    Ok(edits)
}

/// `SymbolKind` as the `TokenType` a semantic token gets, following an alias to its target.
fn token_type_for(model: &Model, symbol_id: model::SymbolId) -> query::TokenType {
    let symbol = &model.symbols[symbol_id];
    let mut kind = symbol.kind;
    if kind == SymbolKind::Alias
        && let Some(target_symbol) = model.entities[symbol.entity].symbol
    {
        kind = model.symbols[target_symbol].kind;
    }
    match kind {
        SymbolKind::Data => query::TokenType::Data,
        SymbolKind::Trait => query::TokenType::Trait,
        SymbolKind::Type => query::TokenType::Type,
        SymbolKind::Enum => query::TokenType::Enum,
        SymbolKind::EnumMember => query::TokenType::EnumMember,
        SymbolKind::Function | SymbolKind::AgentFunction => query::TokenType::Function,
        SymbolKind::Variable | SymbolKind::LoopVariable | SymbolKind::External | SymbolKind::Alias => query::TokenType::Variable,
        SymbolKind::Parameter => query::TokenType::Parameter,
        SymbolKind::Member => query::TokenType::Property,
        SymbolKind::Module => query::TokenType::Namespace,
    }
}

/// Whether an entity's implementation is generated.
fn is_agentic(entity: &model::Entity) -> bool {
    match entity.kind {
        model::EntityKind::Fn { agent, .. } => agent,
        model::EntityKind::Data => true,
        _ => false,
    }
}

/// Whether a variable's declaration used `const`.
fn is_const(model: &Model, symbol: &model::Symbol) -> bool {
    model.node(symbol.node).token(Keyword::ConstKeyword, model.tokens(symbol.node.file)).is_some()
}

/// Whether a node is inside a `TemplateReference`, a `TemplateExecution`, or
/// `Documentation`.
fn in_documentation_or_template(model: &Model, node_ref: model::NodeRef) -> bool {
    let mut current = Some(node_ref);
    while let Some(r) = current {
        let node = model.node(r);
        if node.is(Expression::TemplateReference) || node.is(Expression::TemplateExecution) || node.is(Comment::Documentation) {
            return true;
        }
        current = model.parent(r);
    }
    false
}

fn modifier_rank(modifier: query::TokenModifier) -> u8 {
    match modifier {
        query::TokenModifier::Declaration => 0,
        query::TokenModifier::Agentic => 1,
        query::TokenModifier::Readonly => 2,
        query::TokenModifier::Context => 3,
        query::TokenModifier::Scope => 4,
        query::TokenModifier::Value => 5,
        query::TokenModifier::Documentation => 6,
        query::TokenModifier::Unresolved => 7,
    }
}

/// Every name of a file, classified by meaning for an editor to color.
// @lfy def/query/main.lfy:semanticTokensOf
pub fn semantic_tokens_of(workspace: &Workspace, file: &str) -> Vec<query::SemanticToken> {
    let model = &workspace.model;
    let Some(file_id) = model.file(file) else { return Vec::new() };
    let mut by_token: std::collections::BTreeMap<usize, (query::TokenType, Vec<query::TokenModifier>)> = std::collections::BTreeMap::new();
    let problem_nodes: HashSet<model::NodeRef> = model.problems.iter().map(|problem| problem.node).collect();

    for symbol_id in 0..model.symbols.len() {
        // @lfy def/query/main.lfy:semanticTokensOf
        let symbol = &model.symbols[symbol_id];
        if symbol.node.file != file_id {
            continue;
        }
        let Some(token_index) = symbol.name_token else { continue };
        let entity = &model.entities[symbol.entity];
        let ty = token_type_for(model, symbol_id);
        let mut modifiers = vec![query::TokenModifier::Declaration];
        if is_agentic(entity) {
            modifiers.push(query::TokenModifier::Agentic);
        }
        if symbol.kind == SymbolKind::Variable && is_const(model, symbol) {
            modifiers.push(query::TokenModifier::Readonly);
        }
        by_token.insert(token_index, (ty, modifiers));
    }

    for usage in &model.usages {
        // @lfy def/query/main.lfy:semanticTokensOf
        if usage.node.file != file_id {
            continue;
        }
        let Some(token_index) = usage.token else { continue };
        if by_token.contains_key(&token_index) {
            continue;
        }
        let mut unresolved = false;
        let ty = match usage.symbol {
            Some(symbol_id) => token_type_for(model, symbol_id),
            None if usage.layer == Layer::Context => query::TokenType::Property,
            // A member of something the model does not know is not a name of the program
            // when the binder reported no problem there. @lfy def/query/main.lfy:semanticTokensOf
            None if !problem_nodes.contains(&usage.node) => continue,
            None => {
                unresolved = true;
                query::TokenType::Variable
            }
        };
        let mut modifiers = Vec::new();
        if let Some(symbol_id) = usage.symbol {
            let entity = &model.entities[model.symbols[symbol_id].entity];
            if is_agentic(entity) {
                modifiers.push(query::TokenModifier::Agentic);
            }
        }
        let node = model.node(usage.node);
        let follows_accessor = node.is(Expression::Member) || node.is(Expression::Current);
        if follows_accessor {
            match usage.layer {
                Layer::Context => modifiers.push(query::TokenModifier::Context),
                Layer::Scope | Layer::Parent => modifiers.push(query::TokenModifier::Scope),
                Layer::Value => modifiers.push(query::TokenModifier::Value),
                Layer::Dereference | Layer::Previous => {}
            }
        }
        if in_documentation_or_template(model, usage.node) {
            modifiers.push(query::TokenModifier::Documentation);
        }
        if unresolved {
            modifiers.push(query::TokenModifier::Unresolved);
        }
        by_token.insert(token_index, (ty, modifiers));
    }

    by_token
        .into_iter()
        .map(|(index, (ty, mut modifiers))| {
            modifiers.sort_by_key(|modifier| modifier_rank(*modifier)); // @lfy def/query/main.lfy:semanticTokensOf
            modifiers.dedup();
            query::SemanticToken { range: range_of(workspace, NodeOrToken::Token(file_id, index)), ty, modifiers }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::model::Source;
    use crate::workspace::{File, WorkspaceProblem};

    const PATH: &str = "def/a.lfy";

    fn workspace_from(text: &str) -> Workspace {
        let tokens = crate::lexer::lex(text, Some(PATH)).expect("lex");
        let tree = crate::parser::parse(tokens, None);
        let source = Source { path: PATH.to_string(), tree, uses: Vec::new() };
        let model = model::bind(vec![source]);
        let problems = model.problems.iter().cloned().map(WorkspaceProblem::Bind).collect();
        Workspace {
            root: PathBuf::from("."),
            name: "test".to_string(),
            source_directory: "def".to_string(),
            output_directory: "src".to_string(),
            files: vec![File { path: PATH.to_string(), package: None, source: 0 }],
            model,
            packages: Vec::new(),
            targets: Vec::new(),
            native_dependencies: Vec::new(),
            problems,
            overlays: BTreeMap::new(),
        }
    }

    fn position_of(text: &str, needle: &str) -> query::Position {
        position_of_nth(text, needle, 0)
    }

    fn position_of_nth(text: &str, needle: &str, occurrence: usize) -> query::Position {
        let mut search_from = 0;
        let mut found = 0;
        loop {
            let relative = text[search_from..].find(needle).expect("needle not found in test text");
            let index = search_from + relative;
            if found == occurrence {
                let prefix = &text[..index];
                let line = prefix.matches('\n').count() + 1;
                let column = match prefix.rfind('\n') {
                    Some(newline) => prefix[newline + 1..].chars().count(),
                    None => prefix.chars().count(),
                };
                return query::Position { line, column };
            }
            found += 1;
            search_from = index + needle.len();
        }
    }

    fn end_position(text: &str) -> query::Position {
        advance_position(query::Position { line: 1, column: 0 }, text)
    }

    // @lfy def/query/main.lfy:rangeOf
    #[test]
    fn range_of_a_token_covers_its_raw_text() {
        let text = "const y = 1;\n";
        let workspace = workspace_from(text);
        let position = position_of(text, "y");
        let token = token_at(&workspace, PATH, &position).expect("token");
        assert_eq!(token.raw, "y");
        let range = range_of(&workspace, NodeOrToken::Token(0, 2));
        assert_eq!(range.start, query::Position { line: 1, column: 6 });
        assert_eq!(range.end, query::Position { line: 1, column: 7 });
        assert_eq!(range.file, PATH);
    }

    // @lfy def/query/main.lfy:tokenAt
    #[test]
    fn token_at_returns_none_past_the_end_of_the_file() {
        let text = "const y = 1;\n";
        let workspace = workspace_from(text);
        let past = query::Position { line: 50, column: 0 };
        assert!(token_at(&workspace, PATH, &past).is_none());
        assert!(token_at(&workspace, "def/missing.lfy", &position_of(text, "y")).is_none());
    }

    // @lfy def/query/main.lfy:nodesAt
    #[test]
    fn nodes_at_is_empty_when_there_is_no_token() {
        let text = "const y = 1;\n";
        let workspace = workspace_from(text);
        let past = query::Position { line: 50, column: 0 };
        assert!(nodes_at(&workspace, PATH, &past).is_empty());
        let position = position_of(text, "y");
        let path = nodes_at(&workspace, PATH, &position);
        assert!(!path.is_empty());
        assert!(path.first().unwrap().is(crate::grammar::rules::file::File::SourceFile));
    }

    // @lfy def/query/main.lfy:symbolAt
    #[test]
    fn symbol_at_a_name_gives_the_symbol_it_uses() {
        let text = "d A {}\nconst y = A;\n";
        let workspace = workspace_from(text);
        let position = position_of_nth(text, "A", 1);
        let symbol_id = symbol_at(&workspace, PATH, &position).expect("symbol");
        let symbol = &workspace.model.symbols[symbol_id];
        assert_eq!(symbol.name, "A");
        assert_eq!(symbol.kind, SymbolKind::Data);
    }

    // @lfy def/query/main.lfy:symbolAt
    #[test]
    fn symbol_at_a_template_reference_resolves_like_code() {
        let text = "d A {}\nconst y: `See [[A]]` = 1;\n";
        let workspace = workspace_from(text);
        let position = position_of_nth(text, "A", 1);
        let symbol_id = symbol_at(&workspace, PATH, &position).expect("symbol");
        let symbol = &workspace.model.symbols[symbol_id];
        assert_eq!(symbol.name, "A");
        assert_eq!(symbol.kind, SymbolKind::Data);
    }

    // @lfy def/query/main.lfy:definitionOf
    #[test]
    fn definition_of_a_use_gives_the_declaration_range() {
        let text = "d A {}\nconst y = A;\n";
        let workspace = workspace_from(text);
        let use_position = position_of_nth(text, "A", 1);
        let range = definition_of(&workspace, PATH, &use_position).expect("range");
        assert_eq!(range.start, position_of_nth(text, "A", 0));
    }

    // @lfy def/query/main.lfy:referencesTo
    #[test]
    fn references_to_lists_the_declaration_first_then_every_usage() {
        let text = "d A {}\nconst y = A;\n";
        let workspace = workspace_from(text);
        let position = position_of_nth(text, "A", 0);
        let ranges = references_to(&workspace, PATH, &position, true);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start, position_of_nth(text, "A", 0));
        assert_eq!(ranges[1].start, position_of_nth(text, "A", 1));
        let without_declaration = references_to(&workspace, PATH, &position, false);
        assert_eq!(without_declaration.len(), 1);
    }

    // @lfy def/query/main.lfy:sourceOf
    #[test]
    fn source_of_an_entity_is_its_declaration_text() {
        let text = "d A {}\n";
        let workspace = workspace_from(text);
        let position = position_of(text, "A");
        let symbol_id = symbol_at(&workspace, PATH, &position).expect("symbol");
        let entity = workspace.model.symbols[symbol_id].entity;
        assert_eq!(source_of(&workspace, entity), "d A {}");
    }

    // @lfy def/query/main.lfy:hoverAt
    #[test]
    fn hover_at_reports_the_entity_shown_for_a_symbol() {
        let text = "/// Doc\nd A: `desc` {}\nconst y = A;\n";
        let workspace = workspace_from(text);
        let position = position_of_nth(text, "A", 1);
        let hover = hover_at(&workspace, PATH, &position).expect("hover");
        assert_eq!(hover.kind, SymbolKind::Data);
        assert_eq!(hover.identifier, "A");
        assert_eq!(hover.definition.as_deref(), Some("desc"));
        assert_eq!(hover.documentation.as_deref(), Some("Doc"));
        assert!(hover.criteria.is_empty());
    }

    // @lfy def/query/main.lfy:diagnosticsOf
    #[test]
    fn diagnostics_of_reports_an_unresolved_name_at_the_binder_stage() {
        let text = "const y = z;\n";
        let workspace = workspace_from(text);
        let diagnostics = diagnostics_of(&workspace, None);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].stage, query::Stage::Binder);
        assert_eq!(diagnostics[0].start_line(), 1);
    }

    // @lfy def/query/main.lfy:diagnosticsOf
    #[test]
    fn diagnostics_of_reports_a_parse_error_expecting_an_identifier() {
        let text = "const = 1;\n";
        let workspace = workspace_from(text);
        let diagnostics = diagnostics_of(&workspace, Some(PATH));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].stage, query::Stage::Parser);
        assert!(diagnostics[0].message.contains("Identifier"), "{}", diagnostics[0].message);
    }

    impl query::Diagnostic {
        fn start_line(&self) -> usize {
            self.range.start.line
        }
    }

    // @lfy def/query/main.lfy:outlineOf
    #[test]
    fn outline_of_nests_members_under_their_declaration() {
        let text = "d A { $x = string; }\nconst y = 1;\n";
        let workspace = workspace_from(text);
        let outline = outline_of(&workspace, PATH);
        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].name, "A");
        assert_eq!(outline[0].children.len(), 1);
        assert_eq!(outline[0].children[0].name, "x");
        assert_eq!(outline[1].name, "y");
        assert!(outline[1].children.is_empty());
    }

    // @lfy def/query/main.lfy:findSymbols
    #[test]
    fn find_symbols_flattens_only_when_the_query_is_not_empty() {
        let text = "d A { $x = string; }\nconst y = 1;\n";
        let workspace = workspace_from(text);
        assert_eq!(find_symbols(&workspace, "").len(), 2);
        let matches = find_symbols(&workspace, "x");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "A.x");
    }

    // @lfy def/query/main.lfy:find
    #[test]
    fn find_looks_up_a_name_or_an_owner_and_member() {
        let text = "d A { $x = string; }\n";
        let workspace = workspace_from(text);
        let by_name = find(&workspace, "A");
        assert_eq!(by_name.len(), 1);
        let by_member = find(&workspace, "A.x");
        assert_eq!(by_member.len(), 1);
        assert_ne!(by_name[0], by_member[0]);
    }

    // @lfy def/query/main.lfy:semanticTokensOf
    #[test]
    fn semantic_tokens_of_classifies_declarations_and_uses() {
        let text = "d A { $x: `d` = string; }\nconst y = A$x;\n";
        let workspace = workspace_from(text);
        let tokens = semantic_tokens_of(&workspace, PATH);

        let at = |needle: &str, occurrence: usize| position_of_nth(text, needle, occurrence);
        let find_at = |position: query::Position| tokens.iter().find(|token| token.range.start == position).unwrap_or_else(|| panic!("no semantic token at {position:?}"));

        let a_decl = find_at(at("A", 0));
        assert_eq!(a_decl.ty, query::TokenType::Data);
        assert_eq!(a_decl.modifiers, vec![query::TokenModifier::Declaration, query::TokenModifier::Agentic]);

        let x_decl = find_at(at("x", 0));
        assert_eq!(x_decl.ty, query::TokenType::Property);
        assert_eq!(x_decl.modifiers, vec![query::TokenModifier::Declaration]);

        let y_decl = find_at(at("y", 0));
        assert_eq!(y_decl.ty, query::TokenType::Variable);
        assert_eq!(y_decl.modifiers, vec![query::TokenModifier::Declaration, query::TokenModifier::Readonly]);

        let a_use = find_at(at("A", 1));
        assert_eq!(a_use.ty, query::TokenType::Data);
        assert_eq!(a_use.modifiers, vec![query::TokenModifier::Agentic]);

        let x_use = find_at(at("x", 1));
        assert_eq!(x_use.ty, query::TokenType::Property);
        assert_eq!(x_use.modifiers, vec![query::TokenModifier::Scope]);
    }

    // @lfy def/query/main.lfy:semanticTokensOf
    #[test]
    fn semantic_tokens_of_marks_unresolved_references_and_skips_unknown_members() {
        let text = "fn greet(): `See [[missing]]` => number {\n  @acceptanceCriteria.add({ behavior = `b` });\n}\n";
        let workspace = workspace_from(text);
        let tokens = semantic_tokens_of(&workspace, PATH);

        let at = |needle: &str, occurrence: usize| position_of_nth(text, needle, occurrence);
        let find_at = |position: query::Position| tokens.iter().find(|token| token.range.start == position).unwrap_or_else(|| panic!("no semantic token at {position:?}"));

        let f_decl = find_at(at("greet", 0));
        assert_eq!(f_decl.ty, query::TokenType::Function);
        assert_eq!(f_decl.modifiers, vec![query::TokenModifier::Declaration, query::TokenModifier::Agentic]);

        let g_ref = find_at(at("missing", 0));
        assert_eq!(g_ref.ty, query::TokenType::Variable);
        assert_eq!(g_ref.modifiers, vec![query::TokenModifier::Documentation, query::TokenModifier::Unresolved]);

        let context = find_at(at("acceptanceCriteria", 0));
        assert_eq!(context.ty, query::TokenType::Property);
        assert_eq!(context.modifiers, vec![query::TokenModifier::Context]);

        assert!(!tokens.iter().any(|token| token.range.start == at("add", 0)), "add is not a name in the program");
    }

    // @lfy def/query/main.lfy:completionsAt
    #[test]
    fn completions_at_a_context_accessor_lists_every_context_property() {
        let text = "d A { $x = string; }\nconst y = A@";
        let workspace = workspace_from(text);
        let position = end_position(text);
        let completions = completions_at(&workspace, PATH, &position);
        assert_eq!(completions.len(), ContextProperty::ALL.len());
        assert!(completions.iter().all(|completion| matches!(completion.kind, query::SymbolOrCompletionKind::Completion(query::CompletionKind::Context))));
        assert!(completions.iter().any(|completion| completion.label == "identifier"));
        assert!(!completions.iter().any(|completion| completion.label == "A" || completion.label == "y"));
    }

    // @lfy def/query/main.lfy:completionsAt
    #[test]
    fn completions_at_is_lists_only_traits() {
        let text = "trait t {}\nd A is ";
        let workspace = workspace_from(text);
        let position = end_position(text);
        let completions = completions_at(&workspace, PATH, &position);
        assert_eq!(completions.len(), 1, "{completions:?}");
        assert_eq!(completions[0].label, "t");
        assert_eq!(completions[0].kind, query::SymbolOrCompletionKind::Symbol(SymbolKind::Trait));
    }

    // @lfy def/query/main.lfy:renameAt
    #[test]
    fn rename_at_renames_the_declaration_and_every_usage() {
        let text = "d A {}\nconst y: `See [[A]]` = A;\n";
        let workspace = workspace_from(text);
        let position = position_of_nth(text, "A", 0);
        let edits = rename_at(&workspace, PATH, &position, "B").expect("rename");
        assert_eq!(edits.len(), 3, "{edits:?}");
        assert!(edits.iter().all(|edit| edit.text == "B"));
    }

    // @lfy def/query/main.lfy:renameAt
    #[test]
    fn rename_at_a_keyword_is_refused() {
        let text = "d A {}\nconst y: `See [[A]]` = A;\n";
        let workspace = workspace_from(text);
        let position = position_of_nth(text, "A", 0);
        let reason = rename_at(&workspace, PATH, &position, "const").expect_err("refused");
        assert!(reason.contains("const"), "{reason}");
        assert!(reason.contains("keyword"), "{reason}");
    }
}
