//! Compiled from `def/grammar/precedence.lfy`.

/// Binding power of kinds of operations. A higher value binds tighter.
// @lfy def/grammar/precedence.lfy:Level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Level {
    /// Joining items together in a list
    Joiner = 1, // @lfy def/grammar/precedence.lfy:Level.joiner
    /// Assigning a value
    Assignment = 2, // @lfy def/grammar/precedence.lfy:Level.assignment
    /// Defining an item
    Definition = 3, // @lfy def/grammar/precedence.lfy:Level.definition
    /// Coalescing values
    Coalescence = 4, // @lfy def/grammar/precedence.lfy:Level.coalescence
    LogicalAnd = 5, // @lfy def/grammar/precedence.lfy:Level.logicalAnd
    BitwiseOr = 6,  // @lfy def/grammar/precedence.lfy:Level.bitwiseOr
    BitwiseXor = 7, // @lfy def/grammar/precedence.lfy:Level.bitwiseXor
    BitwiseAnd = 8, // @lfy def/grammar/precedence.lfy:Level.bitwiseAnd
    /// Equality checks between items
    Equality = 9, // @lfy def/grammar/precedence.lfy:Level.equality
    /// Relation checks between items
    Relational = 10, // @lfy def/grammar/precedence.lfy:Level.relational
    /// Extracting information from an item or value
    Extraction = 11, // @lfy def/grammar/precedence.lfy:Level.extraction
    /// Mathmatically additive
    Additive = 12, // @lfy def/grammar/precedence.lfy:Level.additive
    /// Mathmatically multiplicative
    Multiplicative = 13, // @lfy def/grammar/precedence.lfy:Level.multiplicative
    Exponentiation = 14, // @lfy def/grammar/precedence.lfy:Level.exponentiation
    /// Wrapping a statement or expression in a behavior
    Wrapper = 15, // @lfy def/grammar/precedence.lfy:Level.wrapper
    Unary = 16,     // @lfy def/grammar/precedence.lfy:Level.unary
    /// Accesses of value, context, or scope information
    Access = 17, // @lfy def/grammar/precedence.lfy:Level.access
    /// Reference or dereferences
    Reference = 18, // @lfy def/grammar/precedence.lfy:Level.reference
    /// Grouping for controlled precedence
    Group = 19, // @lfy def/grammar/precedence.lfy:Level.group
}

impl Level {
    /// Every level, lowest binding power first.
    pub const ALL: &'static [Level] = &[
        Level::Joiner,
        Level::Assignment,
        Level::Definition,
        Level::Coalescence,
        Level::LogicalAnd,
        Level::BitwiseOr,
        Level::BitwiseXor,
        Level::BitwiseAnd,
        Level::Equality,
        Level::Relational,
        Level::Extraction,
        Level::Additive,
        Level::Multiplicative,
        Level::Exponentiation,
        Level::Wrapper,
        Level::Unary,
        Level::Access,
        Level::Reference,
        Level::Group,
    ];

    /// The numeric binding power of the level.
    pub const fn value(self) -> u8 {
        self as u8
    }

    /// The name the level was declared with.
    pub const fn name(self) -> &'static str {
        match self {
            Level::Joiner => "joiner",
            Level::Assignment => "assignment",
            Level::Definition => "definition",
            Level::Coalescence => "coalescence",
            Level::LogicalAnd => "logicalAnd",
            Level::BitwiseOr => "bitwiseOr",
            Level::BitwiseXor => "bitwiseXor",
            Level::BitwiseAnd => "bitwiseAnd",
            Level::Equality => "equality",
            Level::Relational => "relational",
            Level::Extraction => "extraction",
            Level::Additive => "additive",
            Level::Multiplicative => "multiplicative",
            Level::Exponentiation => "exponentiation",
            Level::Wrapper => "wrapper",
            Level::Unary => "unary",
            Level::Access => "access",
            Level::Reference => "reference",
            Level::Group => "group",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/grammar/precedence.lfy:Level
    #[test]
    fn levels_are_numbered_one_through_nineteen_in_order() {
        assert_eq!(Level::ALL.len(), 19);
        for (index, level) in Level::ALL.iter().enumerate() {
            assert_eq!(level.value() as usize, index + 1, "{}", level.name());
        }
        assert!(Level::Joiner < Level::Assignment);
        assert!(Level::Access < Level::Reference && Level::Reference < Level::Group);
        assert_eq!(Level::LogicalAnd.name(), "logicalAnd");
    }
}
