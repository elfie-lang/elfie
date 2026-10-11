//! The edge of the server: document URIs as workspace paths, protocol positions as token
//! positions, and a query's answer in the spelling the protocol asks for.
// @lfy def/lsp/main.lfy:serve

use std::collections::HashMap;
use std::path::Path;

use elfie_core::model::{Criterion, SymbolKind};
use elfie_core::query::{self, Position, Range};
use tower_lsp::lsp_types as lsp;
use tower_lsp::lsp_types::Url;

use crate::{Encoding, Session};

/// A document URI as a path relative to the root, with forward slashes; `None` when the
/// document is outside the root.
// @lfy def/lsp/main.lfy:serve#serve:serve:488b56c6bce60decf85f2506fccbabce6330487fb875d7109ca0051f995a67d3
pub(crate) fn relative_path(root: &Path, uri: &Url) -> Option<String> {
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
pub(crate) fn uri_of(root: &Path, path: &str) -> Option<Url> {
    Url::from_file_path(root.join(path)).ok()
}

/// The count of code units of an encoding in the characters before a column of a line;
/// one unit per character past the end of the line.
// @lfy def/lsp/main.lfy:serve#serve:serve:4eb913bc998b39b0bc4cd9e2a63d11b56c77c491c2198de374e643b5f6ff06a5
pub(crate) fn to_units(encoding: Encoding, line: &str, column: usize) -> usize {
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
// @lfy def/lsp/main.lfy:serve#serve:serve:4eb913bc998b39b0bc4cd9e2a63d11b56c77c491c2198de374e643b5f6ff06a5
pub(crate) fn from_units(encoding: Encoding, line: &str, character: usize) -> usize {
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
pub(crate) fn end_of(text: &str, encoding: Encoding) -> lsp::Position {
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
// @lfy def/lsp/main.lfy:serve#serve:serve:6b4c40ea0834c4fb7d6b6e15c78c5b97f7d2a5d43c031c9e328542ac9fc2e737
pub(crate) fn apply_change(
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
// @lfy def/lsp/main.lfy:serve#serve:serve:35a0718e0e91d6b10930384042a0fe513dbb8a7188bc60b57f614166263dd04f
// @lfy def/lsp/main.lfy:serve#serve:serve:4eb913bc998b39b0bc4cd9e2a63d11b56c77c491c2198de374e643b5f6ff06a5
pub(crate) struct Converter {
    encoding: Encoding,
    lines: HashMap<String, Vec<String>>,
}

impl Converter {
    pub(crate) fn new(encoding: Encoding) -> Converter {
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

    // @lfy def/lsp/main.lfy:serve#serve:serve:35a0718e0e91d6b10930384042a0fe513dbb8a7188bc60b57f614166263dd04f
    // @lfy def/lsp/main.lfy:serve#serve:serve:4eb913bc998b39b0bc4cd9e2a63d11b56c77c491c2198de374e643b5f6ff06a5
    pub(crate) fn protocol_position(
        &mut self,
        session: &Session,
        file: &str,
        at: Position,
    ) -> lsp::Position {
        let line = at.line.saturating_sub(1);
        let encoding = self.encoding;
        let text = self.line(session, file, line);
        lsp::Position {
            line: line as u32,
            character: to_units(encoding, text, at.column) as u32,
        }
    }

    // @lfy def/lsp/main.lfy:serve#serve:serve:35a0718e0e91d6b10930384042a0fe513dbb8a7188bc60b57f614166263dd04f
    // @lfy def/lsp/main.lfy:serve#serve:serve:4eb913bc998b39b0bc4cd9e2a63d11b56c77c491c2198de374e643b5f6ff06a5
    pub(crate) fn query_position(
        &mut self,
        session: &Session,
        file: &str,
        at: lsp::Position,
    ) -> Position {
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
    // @lfy def/lsp/main.lfy:serve#serve:serve:35a0718e0e91d6b10930384042a0fe513dbb8a7188bc60b57f614166263dd04f
    // @lfy def/lsp/main.lfy:serve#serve:serve:4eb913bc998b39b0bc4cd9e2a63d11b56c77c491c2198de374e643b5f6ff06a5
    pub(crate) fn range(&mut self, session: &Session, range: &Range) -> lsp::Range {
        lsp::Range {
            start: self.protocol_position(session, &range.file, range.start),
            end: self.protocol_position(session, &range.file, range.end),
        }
    }

    // @lfy def/lsp/main.lfy:serve
    pub(crate) fn location(&mut self, session: &Session, range: &Range) -> Option<lsp::Location> {
        let uri = uri_of(&session.workspace.root, &range.file)?;
        Some(lsp::Location {
            uri,
            range: self.range(session, range),
        })
    }

    // @lfy def/lsp/main.lfy:serve
    pub(crate) fn diagnostic(
        &mut self,
        session: &Session,
        diagnostic: &query::Diagnostic,
    ) -> lsp::Diagnostic {
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
pub(crate) fn hover_markdown(hover: &query::Hover) -> String {
    let mut sections = Vec::new();
    let mut code = format!("{} {}", hover.kind, hover.identifier);
    if let Some(ty) = &hover.ty {
        code.push_str(": ");
        code.push_str(ty);
    }
    // Decision: the owner closes the code line, so the kind, the name and the type read as
    // they are written and the declaration that owns them follows in parentheses.
    // @lfy def/lsp/main.lfy:serve#serve:serve:399bc8d9d0952a6a2bfeebf5c3967a47676bc36520e42a430b7035b01cc860f9
    if let Some(owner) = hover.owner.as_deref().filter(|text| !text.is_empty()) {
        code.push_str(&format!(" ({owner})"));
    }
    sections.push(format!("```elfie\n{code}\n```"));
    if let Some(definition) = hover.definition.as_deref().filter(|text| !text.is_empty()) {
        sections.push(definition.to_string());
    }
    // Decision: the traits are one line of their identifiers in application order, joined by
    // a comma, with nothing around them; the criterion names no label.
    // @lfy def/lsp/main.lfy:serve#serve:serve:90077d5afc89f7b023508ef4944e41cc279601008d7a8e3460226b18cd4e5d84
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
pub(crate) fn completion_kind(kind: &str) -> lsp::CompletionItemKind {
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
pub(crate) fn symbol_kind(kind: SymbolKind) -> lsp::SymbolKind {
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
pub(crate) fn document_symbol(
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
pub(crate) fn symbol_information(
    outline: &query::Outline,
    location: lsp::Location,
) -> lsp::SymbolInformation {
    lsp::SymbolInformation {
        name: outline.name.clone(),
        kind: symbol_kind(outline.kind),
        tags: None,
        deprecated: None,
        location,
        container_name: None,
    }
}
