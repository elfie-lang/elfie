//! Compiled from `def/query/data.lfy`: the data every query answers with.
//!
//! Positions are spelled exactly as tokens spell them (line from 1, column from 0, counted
//! in characters) so that no query converts anything; the LSP converts at its edge.

use std::fmt;

use crate::model::{Criterion, SymbolKind};

/// A place in a file's text.
// @lfy def/query/data.lfy:Position
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Position {
    /// The line, counting from 1 as `Token.line` does.
    pub line: usize, // @lfy def/query/data.lfy:Position.line
    /// The column within the line, counting characters from 0 as `Token.column` does.
    pub column: usize, // @lfy def/query/data.lfy:Position.column
}

impl Position {
    pub const fn new(line: usize, column: usize) -> Position {
        Position { line, column }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// A run of text in one file.
// @lfy def/query/data.lfy:Range
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Range {
    /// The file's path, as `Token.file` holds it.
    pub file: String, // @lfy def/query/data.lfy:Range.file
    /// Where it begins.
    pub start: Position, // @lfy def/query/data.lfy:Range.start
    /// The position just after the last character; equal to `start` when empty.
    pub end: Position, // @lfy def/query/data.lfy:Range.end
}

impl Range {
    /// An empty range at a position.
    pub fn empty(file: &str, at: Position) -> Range {
        Range {
            file: file.to_string(),
            start: at,
            end: at,
        }
    }

    /// Whether the range covers nothing.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Whether a position is inside the range, the end excluded (as the position just
    /// after the last character) unless the range is empty.
    pub fn contains(&self, at: Position) -> bool {
        if self.is_empty() {
            at == self.start
        } else {
            self.start <= at && at < self.end
        }
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}-{}", self.file, self.start, self.end)
    }
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
    pub fn value(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Information => "information",
            Severity::Hint => "hint",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
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
    pub fn value(self) -> &'static str {
        match self {
            Stage::Loader => "loader",
            Stage::Lexer => "lexer",
            Stage::Parser => "parser",
            Stage::Binder => "binder",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
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

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} ({}): {}",
            self.range.file, self.range.start, self.severity, self.stage, self.message
        )
    }
}

/// Everything an editor or agent shows for one entity.
// @lfy def/query/data.lfy:Hover
#[derive(Debug, Clone, PartialEq)]
pub struct Hover {
    /// The identifier of the entity's declaration, as `declaring` finds it.
    pub range: Range, // @lfy def/query/data.lfy:Hover.range
    /// The kind of the entity's symbol.
    pub kind: SymbolKind, // @lfy def/query/data.lfy:Hover.kind
    /// The entity's name.
    pub identifier: String, // @lfy def/query/data.lfy:Hover.identifier
    /// `Entity.definition` with every template reference and execution resolved for the
    /// entity.
    pub definition: Option<String>, // @lfy def/query/data.lfy:Hover.definition
    /// `Entity.type` spelled as it is written: an identifier, a generic with its
    /// arguments, `T[]` for a list, a function type with its parameters and output, a type
    /// parameter by its name; `None` when there is none.
    pub ty: Option<String>, // @lfy def/query/data.lfy:Hover.type
    /// The identifier of the declaration that lists a type parameter or declares a member;
    /// `None` for anything else.
    pub owner: Option<String>, // @lfy def/query/data.lfy:Hover.owner
    /// The identifier of each of `Entity.traits` in application order; `builtin` among
    /// them for a native thing.
    pub traits: Vec<String>, // @lfy def/query/data.lfy:Hover.traits
    /// The text of the `Documentation` attached to the declaration, without the
    /// boundaries and with references kept as written.
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
    /// The value of the enum member: how the kind is spelled.
    pub fn value(self) -> &'static str {
        match self {
            CompletionKind::Keyword => "keyword",
            CompletionKind::Path => "path",
            CompletionKind::Context => "context",
        }
    }
}

impl fmt::Display for CompletionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
    }
}

/// What a completion offers: the union `SymbolKind | CompletionKind` of
/// [`Completion::kind`], which is the kind of the declaration the completion names, or
/// what it offers when it names none.
// @lfy def/query/data.lfy:Completion.kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OfferedKind {
    /// The kind of the declaration the completion names.
    Symbol(SymbolKind),
    /// What it offers when it names no declaration.
    Completion(CompletionKind),
}

impl OfferedKind {
    /// How the kind is spelled, whichever side of the union it came from.
    pub fn value(self) -> &'static str {
        match self {
            OfferedKind::Symbol(kind) => kind.as_str(),
            OfferedKind::Completion(kind) => kind.value(),
        }
    }
}

impl From<SymbolKind> for OfferedKind {
    fn from(kind: SymbolKind) -> OfferedKind {
        OfferedKind::Symbol(kind)
    }
}

impl From<CompletionKind> for OfferedKind {
    fn from(kind: CompletionKind) -> OfferedKind {
        OfferedKind::Completion(kind)
    }
}

impl fmt::Display for OfferedKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
    }
}

/// One thing that could be typed at a position.
// @lfy def/query/data.lfy:Completion
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// What is shown and inserted.
    pub label: String, // @lfy def/query/data.lfy:Completion.label
    /// What it offers.
    pub kind: OfferedKind, // @lfy def/query/data.lfy:Completion.kind
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
    /// The whole declaration, its `Documentation` included.
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

/// What a semantic token stands for: the protocol's own kinds where one fits, and `data`
/// and `trait` where Elfie has no equivalent there.
// @lfy def/query/data.lfy:TokenType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenType {
    Namespace,     // @lfy def/query/data.lfy:TokenType.namespace
    Data,          // @lfy def/query/data.lfy:TokenType.data
    Trait,         // @lfy def/query/data.lfy:TokenType._trait
    Type,          // @lfy def/query/data.lfy:TokenType._type
    Enum,          // @lfy def/query/data.lfy:TokenType._enum
    EnumMember,    // @lfy def/query/data.lfy:TokenType.enumMember
    Function,      // @lfy def/query/data.lfy:TokenType._function
    Parameter,     // @lfy def/query/data.lfy:TokenType.parameter
    TypeParameter, // @lfy def/query/data.lfy:TokenType.typeParameter
    Variable,      // @lfy def/query/data.lfy:TokenType.variable
    Property,      // @lfy def/query/data.lfy:TokenType.property
}

impl TokenType {
    /// Every type in enum order: the order of a semantic tokens legend.
    pub const ALL: [TokenType; 11] = [
        TokenType::Namespace,
        TokenType::Data,
        TokenType::Trait,
        TokenType::Type,
        TokenType::Enum,
        TokenType::EnumMember,
        TokenType::Function,
        TokenType::Parameter,
        TokenType::TypeParameter,
        TokenType::Variable,
        TokenType::Property,
    ];

    /// The value of the enum member: the name the protocol or the client knows.
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
            TokenType::TypeParameter => "typeParameter",
            TokenType::Variable => "variable",
            TokenType::Property => "property",
        }
    }

    /// The position in [`TokenType::ALL`], which is the legend index.
    pub fn index(self) -> usize {
        TokenType::ALL
            .iter()
            .position(|&t| t == self)
            .expect("listed")
    }
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
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
    /// Every modifier in enum order: the order of a legend and of `SemanticToken.modifiers`.
    pub const ALL: [TokenModifier; 8] = [
        TokenModifier::Declaration,
        TokenModifier::Agentic,
        TokenModifier::Readonly,
        TokenModifier::Context,
        TokenModifier::Scope,
        TokenModifier::Value,
        TokenModifier::Documentation,
        TokenModifier::Unresolved,
    ];

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

    /// The position in [`TokenModifier::ALL`], which is the legend index and the bit.
    pub fn index(self) -> usize {
        TokenModifier::ALL
            .iter()
            .position(|&m| m == self)
            .expect("listed")
    }
}

impl fmt::Display for TokenModifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
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

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/query/data.lfy:Range.end
    #[test]
    fn a_range_is_empty_when_its_end_is_its_start() {
        let at = Position::new(3, 4);
        let empty = Range::empty("def/a.lfy", at);
        assert!(empty.is_empty());
        assert!(empty.contains(at));
        assert!(!empty.contains(Position::new(3, 5)));
        let range = Range {
            file: "def/a.lfy".to_string(),
            start: Position::new(1, 2),
            end: Position::new(1, 5),
        };
        assert!(!range.is_empty());
        assert!(range.contains(Position::new(1, 2)));
        assert!(range.contains(Position::new(1, 4)));
        assert!(!range.contains(Position::new(1, 5)));
        assert_eq!(range.to_string(), "def/a.lfy:1:2-1:5");
    }

    // @lfy def/query/data.lfy:Severity
    #[test]
    fn severities_and_stages_spell_their_values() {
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Warning.value(), "warning");
        assert_eq!(Severity::Information.value(), "information");
        assert_eq!(Severity::Hint.value(), "hint");
        assert_eq!(Stage::Loader.to_string(), "loader");
        assert_eq!(Stage::Lexer.value(), "lexer");
        assert_eq!(Stage::Parser.value(), "parser");
        assert_eq!(Stage::Binder.value(), "binder");
    }

    // @lfy def/query/data.lfy:CompletionKind
    #[test]
    fn a_completion_kind_comes_from_either_side_of_the_union() {
        assert_eq!(CompletionKind::Keyword.to_string(), "keyword");
        assert_eq!(CompletionKind::Path.value(), "path");
        assert_eq!(CompletionKind::Context.value(), "context");
        let named: OfferedKind = SymbolKind::Data.into();
        let offered: OfferedKind = CompletionKind::Path.into();
        assert_eq!(named.value(), "data");
        assert_eq!(offered.to_string(), "path");
        assert_ne!(named, offered);
    }

    // @lfy def/query/data.lfy:TokenType
    #[test]
    fn token_types_and_modifiers_keep_enum_order_and_values() {
        assert_eq!(
            TokenType::ALL.iter().map(|t| t.value()).collect::<Vec<_>>(),
            [
                "namespace",
                "data",
                "trait",
                "type",
                "enum",
                "enumMember",
                "function",
                "parameter",
                "typeParameter",
                "variable",
                "property"
            ]
        );
        assert_eq!(TokenType::Property.index(), 10);
        assert_eq!(TokenType::TypeParameter.index(), 8);
        assert_eq!(
            TokenModifier::ALL
                .iter()
                .map(|m| m.value())
                .collect::<Vec<_>>(),
            [
                "declaration",
                "agentic",
                "readonly",
                "context",
                "scope",
                "value",
                "documentation",
                "unresolved"
            ]
        );
        assert_eq!(TokenModifier::Unresolved.index(), 7);
        assert_eq!(TokenType::Trait.to_string(), "trait");
        assert_eq!(TokenModifier::Scope.to_string(), "scope");
    }
}
