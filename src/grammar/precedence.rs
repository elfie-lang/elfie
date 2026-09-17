//! Compiled from `def/grammar/precedence.lfy`.

/// Binding power of kinds of operations. A higher value binds tighter.
// @lfy def/grammar/precedence.lfy:1
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Level {
    /// Joining items together in a list
    Joiner = 1, // @lfy def/grammar/precedence.lfy:2
    /// Assigning a value
    Assignment = 2, // @lfy def/grammar/precedence.lfy:3
    /// Defining an item
    Definition = 3, // @lfy def/grammar/precedence.lfy:4
    /// Coalescing values
    Coalescence = 4, // @lfy def/grammar/precedence.lfy:5
    LogicalAnd = 5, // @lfy def/grammar/precedence.lfy:6
    BitwiseOr = 6,  // @lfy def/grammar/precedence.lfy:7
    BitwiseXor = 7, // @lfy def/grammar/precedence.lfy:8
    BitwiseAnd = 8, // @lfy def/grammar/precedence.lfy:9
    /// Equality checks between items
    Equality = 9, // @lfy def/grammar/precedence.lfy:10
    /// Relation checks between items
    Relational = 10, // @lfy def/grammar/precedence.lfy:11
    /// Extracting information from an item or value
    Extraction = 11, // @lfy def/grammar/precedence.lfy:12
    /// Mathmatically additive
    Additive = 12, // @lfy def/grammar/precedence.lfy:13
    /// Mathmatically multiplicative
    Multiplicative = 13, // @lfy def/grammar/precedence.lfy:14
    Exponentiation = 14, // @lfy def/grammar/precedence.lfy:15
    /// Wrapping a statement or expression in a behavior
    Wrapper = 15, // @lfy def/grammar/precedence.lfy:16
    Unary = 16,     // @lfy def/grammar/precedence.lfy:17
    /// Accesses of value, context, or scope information
    Access = 17, // @lfy def/grammar/precedence.lfy:18
    /// Reference or dereferences
    Reference = 18, // @lfy def/grammar/precedence.lfy:19
    /// Grouping for controlled precedence
    Group = 19, // @lfy def/grammar/precedence.lfy:20
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

    // @lfy def/grammar/precedence.lfy:1
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
