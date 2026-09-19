//! Compiled from `def/query/data.lfy`: the data every query answers with.
//!
//! Positions are spelled exactly as tokens spell them (line from 1, column from 0, counted
//! in characters) so that no query converts anything; the LSP converts at its edge.

use std::fmt;

use crate::model::Criterion;

/// A place in a file's text.
// @lfy def/query/data.lfy:10
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Position {
    /// The line, counting from 1 as `Token.line` does.
    pub line: usize, // @lfy def/query/data.lfy:11
    /// The column within the line, counting characters from 0 as `Token.column` does.
    pub column: usize, // @lfy def/query/data.lfy:12
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
// @lfy def/query/data.lfy:15
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Range {
    /// The file's path, as `Token.file` holds it.
    pub file: String, // @lfy def/query/data.lfy:16
    /// Where it begins.
    pub start: Position, // @lfy def/query/data.lfy:17
    /// The position just after the last character; equal to `start` when empty.
    pub end: Position, // @lfy def/query/data.lfy:18
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
// @lfy def/query/data.lfy:21
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,       // @lfy def/query/data.lfy:22
    Warning,     // @lfy def/query/data.lfy:23
    Information, // @lfy def/query/data.lfy:24
    Hint,        // @lfy def/query/data.lfy:25
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
// @lfy def/query/data.lfy:28
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Loader, // @lfy def/query/data.lfy:29
    Lexer,  // @lfy def/query/data.lfy:30
    Parser, // @lfy def/query/data.lfy:31
    Binder, // @lfy def/query/data.lfy:32
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
// @lfy def/query/data.lfy:35
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where it is.
    pub range: Range, // @lfy def/query/data.lfy:36
    /// How bad it is.
    pub severity: Severity, // @lfy def/query/data.lfy:37
    /// Who reported it.
    pub stage: Stage, // @lfy def/query/data.lfy:38
    /// What and why.
    pub message: String, // @lfy def/query/data.lfy:39
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
// @lfy def/query/data.lfy:42
#[derive(Debug, Clone, PartialEq)]
pub struct Hover {
    /// The token the entity was found at; the identifier of its declaration when it was
    /// found by name.
    pub range: Range, // @lfy def/query/data.lfy:43
    /// The kind of declaration, as `declaring` names it; `member` for a member.
    pub kind: String, // @lfy def/query/data.lfy:44
    /// The entity's name.
    pub identifier: String, // @lfy def/query/data.lfy:45
    /// `Entity.definition` with every template reference and execution resolved for the
    /// entity.
    pub definition: Option<String>, // @lfy def/query/data.lfy:46
    /// The identifier of `Entity.type`, or the source text of the type when it is
    /// anonymous.
    pub ty: Option<String>, // @lfy def/query/data.lfy:47
    /// The text of the `Documentation` attached to the declaration, without the
    /// boundaries and with references kept as written.
    pub documentation: Option<String>, // @lfy def/query/data.lfy:48
    /// `criteriaOf` the entity.
    pub criteria: Vec<Criterion>, // @lfy def/query/data.lfy:49
}

/// One thing that could be typed at a position.
// @lfy def/query/data.lfy:52
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// What is shown and inserted.
    pub label: String, // @lfy def/query/data.lfy:53
    /// `keyword`, `path`, `context`, or the kind of declaration as `declaring` names it.
    pub kind: String, // @lfy def/query/data.lfy:54
    /// The definition, when there is one.
    pub detail: Option<String>, // @lfy def/query/data.lfy:55
}

/// One declaration in the outline of a file.
// @lfy def/query/data.lfy:58
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    /// The declared name.
    pub name: String, // @lfy def/query/data.lfy:59
    /// The kind of declaration, as `declaring` names it.
    pub kind: String, // @lfy def/query/data.lfy:60
    /// The whole declaration, its `Documentation` included.
    pub range: Range, // @lfy def/query/data.lfy:61
    /// The identifier alone.
    pub selection_range: Range, // @lfy def/query/data.lfy:62
    /// Members and the declarations nested in its body, in order.
    pub children: Vec<Outline>, // @lfy def/query/data.lfy:63
}

/// One replacement in one file.
// @lfy def/query/data.lfy:66
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// What is replaced.
    pub range: Range, // @lfy def/query/data.lfy:67
    /// What replaces it.
    pub text: String, // @lfy def/query/data.lfy:68
}

/// What a semantic token stands for: the protocol's own kinds where one fits, and `data`
/// and `trait` where Elfie has no equivalent there.
// @lfy def/query/data.lfy:75
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenType {
    Namespace,  // @lfy def/query/data.lfy:76
    Data,       // @lfy def/query/data.lfy:77
    Trait,      // @lfy def/query/data.lfy:78
    Type,       // @lfy def/query/data.lfy:79
    Enum,       // @lfy def/query/data.lfy:80
    EnumMember, // @lfy def/query/data.lfy:81
    Function,   // @lfy def/query/data.lfy:82
    Parameter,  // @lfy def/query/data.lfy:83
    Variable,   // @lfy def/query/data.lfy:84
    Property,   // @lfy def/query/data.lfy:85
}

impl TokenType {
    /// Every type in enum order: the order of a semantic tokens legend.
    pub const ALL: [TokenType; 10] = [
        TokenType::Namespace,
        TokenType::Data,
        TokenType::Trait,
        TokenType::Type,
        TokenType::Enum,
        TokenType::EnumMember,
        TokenType::Function,
        TokenType::Parameter,
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
            TokenType::Variable => "variable",
            TokenType::Property => "property",
        }
    }

    /// The position in [`TokenType::ALL`], which is the legend index.
    pub fn index(self) -> usize {
        TokenType::ALL.iter().position(|&t| t == self).expect("listed")
    }
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
    }
}

/// What refines a semantic token.
// @lfy def/query/data.lfy:88
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenModifier {
    Declaration,   // @lfy def/query/data.lfy:89
    Agentic,       // @lfy def/query/data.lfy:90
    Readonly,      // @lfy def/query/data.lfy:91
    Context,       // @lfy def/query/data.lfy:92
    Scope,         // @lfy def/query/data.lfy:93
    Value,         // @lfy def/query/data.lfy:94
    Documentation, // @lfy def/query/data.lfy:95
    Unresolved,    // @lfy def/query/data.lfy:96
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
        TokenModifier::ALL.iter().position(|&m| m == self).expect("listed")
    }
}

impl fmt::Display for TokenModifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
    }
}

/// One name in a file, classified by what it resolves to.
// @lfy def/query/data.lfy:99
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticToken {
    /// The token that spells the name.
    pub range: Range, // @lfy def/query/data.lfy:100
    /// What it stands for.
    pub ty: TokenType, // @lfy def/query/data.lfy:101
    /// What refines it, in `TokenModifier` order; empty when nothing does.
    pub modifiers: Vec<TokenModifier>, // @lfy def/query/data.lfy:102
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/query/data.lfy:18
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

    // @lfy def/query/data.lfy:21
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

    // @lfy def/query/data.lfy:75
    #[test]
    fn token_types_and_modifiers_keep_enum_order_and_values() {
        assert_eq!(TokenType::ALL.iter().map(|t| t.value()).collect::<Vec<_>>(), ["namespace", "data", "trait", "type", "enum", "enumMember", "function", "parameter", "variable", "property"]);
        assert_eq!(TokenType::Property.index(), 9);
        assert_eq!(TokenModifier::ALL.iter().map(|m| m.value()).collect::<Vec<_>>(), ["declaration", "agentic", "readonly", "context", "scope", "value", "documentation", "unresolved"]);
        assert_eq!(TokenModifier::Unresolved.index(), 7);
        assert_eq!(TokenType::Trait.to_string(), "trait");
        assert_eq!(TokenModifier::Scope.to_string(), "scope");
    }
}
