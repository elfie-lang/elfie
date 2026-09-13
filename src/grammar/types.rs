//! Compiled from `def/grammar/types.lfy`.

/// EBNF details for a grammar rule.
// @lfy def/grammar/types.lfy:1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EBNFSyntax {
    /// Full EBNF form representation.
    pub rule: &'static str, // @lfy def/grammar/types.lfy:2
    /// Identifier for this token in the EBNF format notation.
    pub identifier: &'static str, // @lfy def/grammar/types.lfy:3
    /// Syntax for this token in EBNF format.
    pub syntax: &'static str, // @lfy def/grammar/types.lfy:4
}
