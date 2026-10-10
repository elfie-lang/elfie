//! Compiled from `def/query/data.lfy`: the data every query answers with.
//!
//! Positions are spelled exactly as tokens spell them (line from 1, column from 0, counted
//! in characters) so that no query converts anything; the LSP converts at its edge.

use std::fmt;

use crate::grammar::GrammarRule;
use crate::grammar::terminals::identifier::Identifier as I;
use crate::lexer::Token;
use crate::model::{Criterion, SymbolKind};

// The stage that reported a diagnostic is the one the model's problems carry, so a
// diagnostic names it by the enum `def/model/data.lfy` declares and this module re-exports
// rather than by one of its own.
pub use crate::model::Stage;

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

    /// Where a token starts: its line and its column, as the lexer counted them.
    // @lfy def/query/data.lfy:Position
    pub fn of_token(token: &Token) -> Position {
        Position::new(token.line, token.column)
    }

    /// The position just after the last character of a token's raw text, with every line
    /// break inside it ending a line.
    // @lfy def/query/data.lfy:Position
    pub fn after_token(token: &Token) -> Position {
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

    /// The index of the token of a file this position belongs to, over the file's tokens
    /// as the lexer delivered them.
    ///
    /// A position exactly between two tokens belongs to the earlier token when that is an
    /// identifier or a keyword (a `MemberName` is one of those) and to the later token
    /// otherwise. A position past the end of the file belongs to no token.
    // @lfy def/query/data.lfy:Position#Position:Position:861b7a44daa1e4d65aebc43df36fbac3099694952a6750dc2c10a13f271a8327
    pub fn token_in(self, tokens: &[Token]) -> Option<usize> {
        let mut previous: Option<usize> = None;
        for (index, token) in tokens.iter().enumerate() {
            // Decision: a token without raw text (a mode left open at the end of the
            // file) covers nothing and is never the token a position belongs to.
            if token.raw.is_empty() {
                continue;
            }
            let start = Position::of_token(token);
            if self < start {
                break;
            }
            // @lfy def/query/data.lfy:Position#Position:Position:861b7a44daa1e4d65aebc43df36fbac3099694952a6750dc2c10a13f271a8327
            if self < Position::after_token(token) {
                // @lfy def/query/data.lfy:Position#Position:Position:0764e4fc500185958a79dff6c472387292a0b929b29c1698726fd91fb1365902
                if self == start
                    && previous.is_some_and(|p| {
                        is_name_token(&tokens[p]) && Position::after_token(&tokens[p]) == self
                    })
                {
                    return previous;
                }
                // @lfy def/query/data.lfy:Position#Position:Position:a10f6c71c4533b3b95c737d090f9e84f0f2bef8b90d443fce945bf6141f3a94d
                return Some(index);
            }
            previous = Some(index);
        }
        // Decision: the end of the file counts as the boundary after the last token, so a
        // position exactly after a final identifier or keyword still belongs to it.
        // @lfy def/query/data.lfy:Position#Position:Position:0764e4fc500185958a79dff6c472387292a0b929b29c1698726fd91fb1365902
        match previous {
            Some(p) if is_name_token(&tokens[p]) && Position::after_token(&tokens[p]) == self => {
                Some(p)
            }
            // @lfy def/query/data.lfy:Position#Position:Position:f09439a462b605013eb205d44209e33b7a249f771901382b53f30d9e1b0cfbba
            _ => None,
        }
    }
}

/// Whether a token is an identifier or a keyword: what a name is spelled with, a
/// `MemberName` that a keyword stands in for included.
// @lfy def/query/data.lfy:Position
pub fn is_name_token(token: &Token) -> bool {
    token
        .rule
        .is_some_and(|rule| rule == I::Identifier.entity() || rule.is_keyword())
}

/// Whether a node contains a position: whether the run of tokens it covers, `[start, end)`
/// over the file's tokens as `Node` and `ErrorNode` spell it, holds the token the position
/// belongs to.
// @lfy def/query/data.lfy:Position#Position:Position:8b94c4fc19a43ba061b4ab961f36c9bc6c916d1c1c1378c505f590aa07df9a7f
pub fn node_covers(tokens: &[Token], start: usize, end: usize, position: Position) -> bool {
    position
        .token_in(tokens)
        .is_some_and(|index| start <= index && index < end)
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

    /// The range of one token: in the token's own file, from its line and column to just
    /// after the last character of its raw text, each line break inside it ending a line.
    // @lfy def/query/data.lfy:Range#Range:Range:3d464f81ae846c20e751afbb0e5c64b500912337bbf142ea3852c8e1b828cbdd
    // @lfy def/query/data.lfy:Range#Range:Range:42410cc3359cf9c81600301f587a3214f74cc7299ca3f4c83665b1a25d3ca7e6
    // @lfy def/query/data.lfy:Range#Range:Range:9f2a0366cc29ad79e6acbcac521fb346187ba09ea8e99dae986b2007b27be7f9
    pub fn of_token(token: &Token) -> Range {
        Range {
            file: token.file.to_string(),
            start: Position::of_token(token),
            end: Position::after_token(token),
        }
    }

    /// The range of the tokens `[start, end)` of the file at `path`: what a `Node` or an
    /// `ErrorNode` covering them sits at.
    // @lfy def/query/data.lfy:Range#Range:Range:fa2ba58abfd46b09ab8dd55507469634a70abc7e199c06a6631caf7983225c82
    pub fn of_tokens(path: &str, tokens: &[Token], start: usize, end: usize) -> Range {
        if start < end && end <= tokens.len() {
            let first = &tokens[start];
            return Range {
                file: first.file.to_string(),
                start: Position::of_token(first),
                end: Position::after_token(&tokens[end - 1]),
            };
        }
        // @lfy def/query/data.lfy:Range#Range:Range:0409eda181c0b22cae535fe79d05cf2ff5168ef5a35f69ac4fa34582db8a2ef5
        let at = match tokens.get(start) {
            Some(token) => Position::of_token(token),
            None => tokens
                .last()
                .map_or(Position::new(1, 0), Position::after_token),
        };
        Range::empty(path, at)
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
    /// Where the entity is declared.
    pub range: Range, // @lfy def/query/data.lfy:Hover.range
    /// What kind of entity it is.
    pub kind: SymbolKind, // @lfy def/query/data.lfy:Hover.kind
    /// What the entity is called.
    pub identifier: String, // @lfy def/query/data.lfy:Hover.identifier
    /// Its definition, ready to show.
    pub definition: Option<String>, // @lfy def/query/data.lfy:Hover.definition
    /// Its type, spelled as Elfie source.
    pub ty: Option<String>, // @lfy def/query/data.lfy:Hover.type
    /// The declaration it belongs to.
    pub owner: Option<String>, // @lfy def/query/data.lfy:Hover.owner
    /// The traits it carries.
    pub traits: Vec<String>, // @lfy def/query/data.lfy:Hover.traits
    /// The documentation written above its declaration.
    pub documentation: Option<String>, // @lfy def/query/data.lfy:Hover.documentation
    /// Its acceptance criteria, ready to show.
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
    /// What is declared inside it.
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

    /// The tokens of a source, lexed as `def/a.lfy`.
    fn lexed(text: &str) -> Vec<Token> {
        crate::lexer::lex(text, Some("def/a.lfy")).expect("the source lexes")
    }

    // @lfy def/query/data.lfy:Position#Position:Position:861b7a44daa1e4d65aebc43df36fbac3099694952a6750dc2c10a13f271a8327
    // @lfy def/query/data.lfy:Position#Position:Position:0764e4fc500185958a79dff6c472387292a0b929b29c1698726fd91fb1365902
    // @lfy def/query/data.lfy:Position#Position:Position:a10f6c71c4533b3b95c737d090f9e84f0f2bef8b90d443fce945bf6141f3a94d
    // @lfy def/query/data.lfy:Position#Position:Position:f09439a462b605013eb205d44209e33b7a249f771901382b53f30d9e1b0cfbba
    #[test]
    fn a_position_belongs_to_the_token_whose_range_covers_it() {
        let tokens = lexed("const x = 1;");
        let raw = |line, column| {
            Position::new(line, column)
                .token_in(&tokens)
                .map(|index| tokens[index].raw.as_str())
        };
        // Inside a token.
        assert_eq!(raw(1, 2), Some("const"));
        // Between a keyword or an identifier and what follows, the earlier one wins.
        assert_eq!(raw(1, 5), Some("const"));
        assert_eq!(raw(1, 7), Some("x"));
        // Between two tokens of which the earlier spells no name, the later one wins.
        assert_eq!(raw(1, 8), Some("="));
        assert_eq!(raw(1, 11), Some(";"));
        // Past the end of the file, on the last line and beyond it.
        assert_eq!(raw(1, 12), None);
        assert_eq!(raw(2, 0), None);
        // The end of the file is still the boundary after a final name.
        let tokens = lexed("const x = y");
        assert_eq!(
            Position::new(1, 11)
                .token_in(&tokens)
                .map(|index| tokens[index].raw.as_str()),
            Some("y")
        );
    }

    // @lfy def/query/data.lfy:Range#Range:Range:3d464f81ae846c20e751afbb0e5c64b500912337bbf142ea3852c8e1b828cbdd
    // @lfy def/query/data.lfy:Range#Range:Range:42410cc3359cf9c81600301f587a3214f74cc7299ca3f4c83665b1a25d3ca7e6
    // @lfy def/query/data.lfy:Range#Range:Range:9f2a0366cc29ad79e6acbcac521fb346187ba09ea8e99dae986b2007b27be7f9
    // @lfy def/query/data.lfy:Range#Range:Range:fa2ba58abfd46b09ab8dd55507469634a70abc7e199c06a6631caf7983225c82
    // @lfy def/query/data.lfy:Range#Range:Range:0409eda181c0b22cae535fe79d05cf2ff5168ef5a35f69ac4fa34582db8a2ef5
    // @lfy def/query/data.lfy:Position#Position:Position:8b94c4fc19a43ba061b4ab961f36c9bc6c916d1c1c1378c505f590aa07df9a7f
    #[test]
    fn a_range_runs_from_the_first_token_covered_to_just_after_the_last() {
        let tokens = lexed("/** Block\n doc **/\nd A {}\n");
        // A token's range is in its own file, starts where it starts, and ends just after
        // its last character, a line break inside it ending a line.
        assert_eq!(
            Range::of_token(&tokens[1]),
            Range {
                file: "def/a.lfy".to_string(),
                start: Position::new(1, 3),
                end: Position::new(2, 5),
            }
        );
        assert_eq!(
            Range::of_token(&tokens[0]),
            Range {
                file: "def/a.lfy".to_string(),
                start: Position::new(1, 0),
                end: Position::new(1, 3),
            }
        );
        // A run of tokens runs from the first token's start to the last token's end.
        let whole = Range::of_tokens("def/a.lfy", &tokens, 0, tokens.len());
        assert_eq!(whole.start, Position::new(1, 0));
        assert_eq!(whole.end, Position::new(4, 0));
        // A run that covers no token is empty where it would have started.
        assert_eq!(
            Range::of_tokens("def/a.lfy", &tokens, 2, 2),
            Range::empty("def/a.lfy", Position::new(2, 5))
        );
        assert_eq!(
            Range::of_tokens("def/b.lfy", &[], 0, 0),
            Range::empty("def/b.lfy", Position::new(1, 0))
        );
        // A node contains a position when it covers the token the position belongs to.
        assert!(node_covers(&tokens, 0, 2, Position::new(1, 4)));
        assert!(!node_covers(&tokens, 0, 2, Position::new(3, 1)));
        assert!(!node_covers(&tokens, 0, 2, Position::new(9, 0)));
    }

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
