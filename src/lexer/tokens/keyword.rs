//! Compiled from `def/lexer/tokens/keyword.lfy`.

use super::token_enum;

token_enum! {
    /// Reserved words. A run of identifier characters equal to a keyword's value is
    /// lexed as that keyword instead of an identifier.
    // @lfy def/lexer/tokens/keyword.lfy:5
    pub enum Keyword {
        AgentData => ("KW_AGENT_DATA", "d", "Agentically defined data structure"), // @lfy def/lexer/tokens/keyword.lfy:6
        AgentFn => ("KW_AGENT_FN", "fn", "Agentically defined function"), // @lfy def/lexer/tokens/keyword.lfy:7
        Alias => ("KW_ALIAS", "alias", "Alias for identifier"), // @lfy def/lexer/tokens/keyword.lfy:8
        And => ("KW_AND", "and", "Combine conditions in contexts like the \"where\" syntax sugar keyword"), // @lfy def/lexer/tokens/keyword.lfy:9
        As => ("KW_AS", "as", "Casting or aliasing"), // @lfy def/lexer/tokens/keyword.lfy:10
        Async => ("KW_ASYNC", "async", "Indicating an asynchronous block wrapped in a Promise"), // @lfy def/lexer/tokens/keyword.lfy:11
        Await => ("KW_AWAIT", "await", "Awaiting the completion of a Promise"), // @lfy def/lexer/tokens/keyword.lfy:12
        Break => ("KW_BREAK", "break", "Breaking out of loops"), // @lfy def/lexer/tokens/keyword.lfy:13
        CompileTimeExecution => ("KW_COMPILE_TIME_EXECUTION", "ace", "Marking a statement or block to be executed at compile time"), // @lfy def/lexer/tokens/keyword.lfy:14
        Const => ("KW_CONST", "const", "Defining constant variables"), // @lfy def/lexer/tokens/keyword.lfy:15
        Continue => ("KW_CONTINUE", "continue", "Completing the block, but not breaking out of it"), // @lfy def/lexer/tokens/keyword.lfy:16
        Default => ("KW_DEFAULT", "default", "The default action, value, or block"), // @lfy def/lexer/tokens/keyword.lfy:17
        Else => ("KW_ELSE", "else", "An else statement or block"), // @lfy def/lexer/tokens/keyword.lfy:18
        Enum => ("KW_ENUM", "enum", "Defining a variable with enumerated keys and values"), // @lfy def/lexer/tokens/keyword.lfy:19
        External => ("KW_EXTERNAL", "external", "Referencing something external to the code"), // @lfy def/lexer/tokens/keyword.lfy:20
        For => ("KW_FOR", "for", "A loop over iterators when combined with from, of, and in keywords"), // @lfy def/lexer/tokens/keyword.lfy:21
        From => ("KW_FROM", "from", "Referencing a type, variable, or data structures' keys and values"), // @lfy def/lexer/tokens/keyword.lfy:22
        Function => ("KW_FUNCTION", "function", "An explicitly defined function"), // @lfy def/lexer/tokens/keyword.lfy:23
        If => ("KW_IF", "if", "An if conditional block or statement"), // @lfy def/lexer/tokens/keyword.lfy:24
        In => ("KW_IN", "in", "Referencing a type, variable, or data structures' values"), // @lfy def/lexer/tokens/keyword.lfy:25
        Let => ("KW_LET", "let", "Defining a mutable variable"), // @lfy def/lexer/tokens/keyword.lfy:26
        Loop => ("KW_LOOP", "loop", "A basic loop that runs until it encounters a break"), // @lfy def/lexer/tokens/keyword.lfy:27
        Match => ("KW_MATCH", "match", "A match expression or block that branches based on condition statements"), // @lfy def/lexer/tokens/keyword.lfy:28
        Matchall => ("KW_MATCHALL", "matchall", "A matchall expression or block that applies the results of all condition statements that are true"), // @lfy def/lexer/tokens/keyword.lfy:29
        Of => ("KW_OF", "of", "Referencing a type, variable, or data structures' keys"), // @lfy def/lexer/tokens/keyword.lfy:30
        Or => ("KW_OR", "or", "Branch conditions in contexts like the \"where\" syntax sugar keyword"), // @lfy def/lexer/tokens/keyword.lfy:31
        Return => ("KW_RETURN", "return", "Return expression that ends execution of a function and returns a value or set of values"), // @lfy def/lexer/tokens/keyword.lfy:32
        Trait => ("KW_TRAIT", "trait", "Group of shared definitions, properties, configuration, and other information that can be easily added to data, functions or other kinds of variables"), // @lfy def/lexer/tokens/keyword.lfy:33
        Type => ("KW_TYPE", "type", "Defining a type map"), // @lfy def/lexer/tokens/keyword.lfy:34
        Use => ("KW_USE", "use", "Add the scoped contents of a file, folder, or library to the current scope"), // @lfy def/lexer/tokens/keyword.lfy:35
        While => ("KW_WHILE", "while", "A while loop"), // @lfy def/lexer/tokens/keyword.lfy:36
        Where => ("KW_WHERE", "where", "Syntax sugar for @acceptanceCriteria.add"), // @lfy def/lexer/tokens/keyword.lfy:37

        PrimitiveBoolean => ("KW_PRIMITIVE_BOOLEAN", "boolean", "Primitive boolean type"), // @lfy def/lexer/tokens/keyword.lfy:39
        PrimitiveBooleanFalse => ("KW_PRIMITIVE_BOOLEAN_FALSE", "false", "Primitive boolean value false"), // @lfy def/lexer/tokens/keyword.lfy:40
        PrimitiveBooleanTrue => ("KW_PRIMITIVE_BOOLEAN_TRUE", "true", "Primitive boolean value true"), // @lfy def/lexer/tokens/keyword.lfy:41
        PrimitiveNull => ("KW_PRIMITIVE_NULL", "null", "Primitive null"), // @lfy def/lexer/tokens/keyword.lfy:42
        PrimitiveNumber => ("KW_PRIMITIVE_NUMBER", "number", "Primitive number type"), // @lfy def/lexer/tokens/keyword.lfy:43
        PrimitiveObject => ("KW_PRIMITIVE_OBJECT", "object", "Primitive object type"), // @lfy def/lexer/tokens/keyword.lfy:44
        PrimitiveString => ("KW_PRIMITIVE_STRING", "string", "Primitive string type"), // @lfy def/lexer/tokens/keyword.lfy:45
        PrimitiveUndefined => ("KW_PRIMITIVE_UNDEFINED", "undefined", "Primitive undefined"), // @lfy def/lexer/tokens/keyword.lfy:46

        Abstract => ("KW_ABSTRACT", "abstract", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:48
        Case => ("KW_CASE", "case", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:49
        Class => ("KW_CLASS", "class", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:50
        Impl => ("KW_IMPL", "impl", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:51
        Interface => ("KW_INTERFACE", "interface", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:52
        New => ("KW_NEW", "new", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:53
        Private => ("KW_PRIVATE", "private", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:54
        Public => ("KW_PUBLIC", "public", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:55
        SelfRef => ("KW_SELF", "self", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:56
        Static => ("KW_STATIC", "static", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:57
        Super => ("KW_SUPER", "super", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:58
        Switch => ("KW_SWITCH", "switch", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:59
        Then => ("KW_THEN", "then", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:60
        Typeof => ("KW_TYPEOF", "typeof", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:61
        With => ("KW_WITH", "with", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:62
        Yield => ("KW_YIELD", "yield", "Reserved for future use"), // @lfy def/lexer/tokens/keyword.lfy:63
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/lexer/tokens/keyword.lfy:5
    #[test]
    fn keyword_values_are_unique_and_round_trip() {
        for &keyword in Keyword::ALL {
            assert_eq!(Keyword::from_value(keyword.value()), Some(keyword));
            assert!(keyword.key().starts_with("KW_"));
        }
        assert_eq!(Keyword::ALL.len(), 56);
    }
}
