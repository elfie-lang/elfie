//! Compiled from `def/query/data.lfy`: the shapes the query surface returns for an editor
//! or agent, independent of any protocol.

use crate::model::{Criterion, SymbolKind};

/// A place in a file's text.
// @lfy def/query/data.lfy:Position
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    /// The line, counting from 1 as `Token::line` does.
    pub line: usize, // @lfy def/query/data.lfy:Position.line
    /// The column within the line, counting characters from 0 as `Token::column` does.
    pub column: usize, // @lfy def/query/data.lfy:Position.column
}

/// A run of text in one file.
// @lfy def/query/data.lfy:Range
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    /// The file's path, as `Token::file` holds it.
    pub file: String, // @lfy def/query/data.lfy:Range.file
    /// Where it begins.
    pub start: Position, // @lfy def/query/data.lfy:Range.start
    /// The position just after the last character; equal to `Range::start` when empty.
    pub end: Position, // @lfy def/query/data.lfy:Range.end
}

/// How bad a diagnostic is.
// @lfy def/query/data.lfy:Severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,       // @lfy def/query/data.lfy:Severity.error
    Warning,     // @lfy def/query/data.lfy:Severity.warning
    Information, // @lfy def/query/data.lfy:Severity.information
    Hint,        // @lfy def/query/data.lfy:Severity.hint
}

impl Severity {
    /// The value of the enum member: what is spelled after `=`.
    pub fn value(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Information => "information",
            Severity::Hint => "hint",
        }
    }
}

/// The stage that reported a diagnostic.
// @lfy def/query/data.lfy:Stage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Loader, // @lfy def/query/data.lfy:Stage.loader
    Lexer,  // @lfy def/query/data.lfy:Stage.lexer
    Parser, // @lfy def/query/data.lfy:Stage.parser
    Binder, // @lfy def/query/data.lfy:Stage.binder
}

impl Stage {
    /// The value of the enum member: what is spelled after `=`.
    pub fn value(self) -> &'static str {
        match self {
            Stage::Loader => "loader",
            Stage::Lexer => "lexer",
            Stage::Parser => "parser",
            Stage::Binder => "binder",
        }
    }
}

/// One problem, placed.
// @lfy def/query/data.lfy:Diagnostic
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where it is.
    pub range: Range, // @lfy def/query/data.lfy:Diagnostic.range
    /// How bad it is.
    pub severity: Severity, // @lfy def/query/data.lfy:Diagnostic.severity
    /// Who reported it.
    pub stage: Stage, // @lfy def/query/data.lfy:Diagnostic.stage
    /// What and why.
    pub message: String, // @lfy def/query/data.lfy:Diagnostic.message
}

/// Everything an editor or agent shows for one entity.
// @lfy def/query/data.lfy:Hover
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    /// The identifier of the entity's declaration, as `declaring` finds it.
    pub range: Range, // @lfy def/query/data.lfy:Hover.range
    /// The kind of the entity's symbol.
    pub kind: SymbolKind, // @lfy def/query/data.lfy:Hover.kind
    /// The entity's name.
    pub identifier: String, // @lfy def/query/data.lfy:Hover.identifier
    /// `Entity::definition` with every template reference and execution resolved for the
    /// entity.
    pub definition: Option<String>, // @lfy def/query/data.lfy:Hover.definition
    /// The identifier of `Entity::ty`, or the source text of the type when it is anonymous.
    pub ty: Option<String>, // @lfy def/query/data.lfy:Hover.type
    /// The text of the documentation attached to the declaration, without the boundaries
    /// and with references kept as written.
    pub documentation: Option<String>, // @lfy def/query/data.lfy:Hover.documentation
    /// `criteriaOf` the entity.
    pub criteria: Vec<Criterion>, // @lfy def/query/data.lfy:Hover.criteria
}

/// What a completion offers when it does not name a declaration.
// @lfy def/query/data.lfy:CompletionKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    Keyword, // @lfy def/query/data.lfy:CompletionKind.keyword
    Path,    // @lfy def/query/data.lfy:CompletionKind.path
    Context, // @lfy def/query/data.lfy:CompletionKind.context
}

impl CompletionKind {
    /// The value of the enum member: what is spelled after `=`.
    pub fn value(self) -> &'static str {
        match self {
            CompletionKind::Keyword => "keyword",
            CompletionKind::Path => "path",
            CompletionKind::Context => "context",
        }
    }
}

/// `Completion::kind`: `SymbolKind | CompletionKind`.
// @lfy def/query/data.lfy:Completion.kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolOrCompletionKind {
    Symbol(SymbolKind),
    Completion(CompletionKind),
}

impl SymbolOrCompletionKind {
    pub fn as_symbol(self) -> Option<SymbolKind> {
        match self {
            SymbolOrCompletionKind::Symbol(kind) => Some(kind),
            SymbolOrCompletionKind::Completion(_) => None,
        }
    }

    pub fn as_completion(self) -> Option<CompletionKind> {
        match self {
            SymbolOrCompletionKind::Completion(kind) => Some(kind),
            SymbolOrCompletionKind::Symbol(_) => None,
        }
    }
}

/// One thing that could be typed at a position.
// @lfy def/query/data.lfy:Completion
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// What is shown and inserted.
    pub label: String, // @lfy def/query/data.lfy:Completion.label
    /// What it offers.
    pub kind: SymbolOrCompletionKind, // @lfy def/query/data.lfy:Completion.kind
    /// The definition, when there is one.
    pub detail: Option<String>, // @lfy def/query/data.lfy:Completion.detail
}

/// One declaration in the outline of a file.
// @lfy def/query/data.lfy:Outline
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    /// The declared name.
    pub name: String, // @lfy def/query/data.lfy:Outline.name
    /// The kind of the declaration.
    pub kind: SymbolKind, // @lfy def/query/data.lfy:Outline.kind
    /// The whole declaration, its documentation included.
    pub range: Range, // @lfy def/query/data.lfy:Outline.range
    /// The identifier alone.
    pub selection_range: Range, // @lfy def/query/data.lfy:Outline.selectionRange
    /// Members and the declarations nested in its body, in order.
    pub children: Vec<Outline>, // @lfy def/query/data.lfy:Outline.children
}

/// One replacement in one file.
// @lfy def/query/data.lfy:Edit
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// What is replaced.
    pub range: Range, // @lfy def/query/data.lfy:Edit.range
    /// What replaces it.
    pub text: String, // @lfy def/query/data.lfy:Edit.text
}

/// What a semantic token stands for; the protocol's own kinds where one fits, and data and
/// trait where Elfie has no equivalent there.
// @lfy def/query/data.lfy:TokenType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenType {
    Namespace,  // @lfy def/query/data.lfy:TokenType.namespace
    Data,       // @lfy def/query/data.lfy:TokenType.data
    Trait,      // @lfy def/query/data.lfy:TokenType._trait
    Type,       // @lfy def/query/data.lfy:TokenType._type
    Enum,       // @lfy def/query/data.lfy:TokenType._enum
    EnumMember, // @lfy def/query/data.lfy:TokenType.enumMember
    Function,   // @lfy def/query/data.lfy:TokenType._function
    Parameter,  // @lfy def/query/data.lfy:TokenType.parameter
    Variable,   // @lfy def/query/data.lfy:TokenType.variable
    Property,   // @lfy def/query/data.lfy:TokenType.property
}

impl TokenType {
    /// The value of the enum member: what is spelled after `=`.
    pub fn value(self) -> &'static str {
        match self {
            TokenType::Namespace => "namespace",
            TokenType::Data => "data",
            TokenType::Trait => "trait",
            TokenType::Type => "type",
            TokenType::Enum => "enum",
            TokenType::EnumMember => "enumMember",
            TokenType::Function => "function",
            TokenType::Parameter => "parameter",
            TokenType::Variable => "variable",
            TokenType::Property => "property",
        }
    }
}

/// What refines a semantic token.
// @lfy def/query/data.lfy:TokenModifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenModifier {
    Declaration,   // @lfy def/query/data.lfy:TokenModifier.declaration
    Agentic,       // @lfy def/query/data.lfy:TokenModifier.agentic
    Readonly,      // @lfy def/query/data.lfy:TokenModifier.readonly
    Context,       // @lfy def/query/data.lfy:TokenModifier.context
    Scope,         // @lfy def/query/data.lfy:TokenModifier.scope
    Value,         // @lfy def/query/data.lfy:TokenModifier.value
    Documentation, // @lfy def/query/data.lfy:TokenModifier.documentation
    Unresolved,    // @lfy def/query/data.lfy:TokenModifier.unresolved
}

impl TokenModifier {
    /// The value of the enum member: what is spelled after `=`.
    pub fn value(self) -> &'static str {
        match self {
            TokenModifier::Declaration => "declaration",
            TokenModifier::Agentic => "agentic",
            TokenModifier::Readonly => "readonly",
            TokenModifier::Context => "context",
            TokenModifier::Scope => "scope",
            TokenModifier::Value => "value",
            TokenModifier::Documentation => "documentation",
            TokenModifier::Unresolved => "unresolved",
        }
    }
}

/// One name in a file, classified by what it resolves to.
// @lfy def/query/data.lfy:SemanticToken
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticToken {
    /// The token that spells the name.
    pub range: Range, // @lfy def/query/data.lfy:SemanticToken.range
    /// What it stands for.
    pub ty: TokenType, // @lfy def/query/data.lfy:SemanticToken.type
    /// What refines it, in `TokenModifier` order; empty when nothing does.
    pub modifiers: Vec<TokenModifier>, // @lfy def/query/data.lfy:SemanticToken.modifiers
}
