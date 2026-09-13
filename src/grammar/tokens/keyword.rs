//! Compiled from `def/grammar/tokens/keyword.lfy`.

use super::super::{EbnfSyntax, grammar_rules};

grammar_rules! {
    /// Reserved words.
    pub enum Keyword {
        /// Agentically defined data structure
        AgentDataKeyword = r#""d""#, // @lfy def/grammar/tokens/keyword.lfy:3
        /// Agentically defined function
        AgentFnKeyword = r#""fn""#, // @lfy def/grammar/tokens/keyword.lfy:4
        /// Alias for identifier
        AliasKeyword = r#""alias""#, // @lfy def/grammar/tokens/keyword.lfy:5
        /// Combine conditions in contexts like the "where" syntax sugar keyword
        AndKeyword = r#""and""#, // @lfy def/grammar/tokens/keyword.lfy:6
        /// Casting or aliasing
        AsKeyword = r#""as""#, // @lfy def/grammar/tokens/keyword.lfy:7
        /// Indicating an asynchronous block wrapped in a Promise
        AsyncKeyword = r#""async""#, // @lfy def/grammar/tokens/keyword.lfy:8
        /// Awaiting the completion of a Promise
        AwaitKeyword = r#""await""#, // @lfy def/grammar/tokens/keyword.lfy:9
        /// Breaking out of loops
        BreakKeyword = r#""break""#, // @lfy def/grammar/tokens/keyword.lfy:10
        /// Marking a statement or block to be executed at compile time
        CompileTimeExecutionKeyword = r#""ace""#, // @lfy def/grammar/tokens/keyword.lfy:11
        /// Defining constant variables
        ConstKeyword = r#""const""#, // @lfy def/grammar/tokens/keyword.lfy:12
        /// Completing the block, but not breaking out of it
        ContinueKeyword = r#""continue""#, // @lfy def/grammar/tokens/keyword.lfy:13
        /// The default action, value, or block
        DefaultKeyword = r#""default""#, // @lfy def/grammar/tokens/keyword.lfy:14
        /// An else statement or block
        ElseKeyword = r#""else""#, // @lfy def/grammar/tokens/keyword.lfy:15
        /// Defining a variable with enumerated keys and values
        EnumKeyword = r#""enum""#, // @lfy def/grammar/tokens/keyword.lfy:16
        /// Extending an existing trait or data structure
        ExtendsKeyword = r#""extends""#, // @lfy def/grammar/tokens/keyword.lfy:17
        /// Referencing something external to the code
        ExternalKeyword = r#""external""#, // @lfy def/grammar/tokens/keyword.lfy:18
        /// A loop over iterators when combined with from, of, and in keywords
        ForKeyword = r#""for""#, // @lfy def/grammar/tokens/keyword.lfy:19
        /// Referencing a type, variable, or data structures' keys and values
        FromKeyword = r#""from""#, // @lfy def/grammar/tokens/keyword.lfy:20
        /// An explicitly defined function
        FunctionKeyword = r#""function""#, // @lfy def/grammar/tokens/keyword.lfy:21
        /// An if conditional block or statement
        IfKeyword = r#""if""#, // @lfy def/grammar/tokens/keyword.lfy:22
        /// Referencing a type, variable, or data structures' values
        InKeyword = r#""in""#, // @lfy def/grammar/tokens/keyword.lfy:23
        /// Marking traits of an expression, block, variable, or value
        IsKeyword = r#""is""#, // @lfy def/grammar/tokens/keyword.lfy:24
        /// Defining a mutable variable
        LetKeyword = r#""let""#, // @lfy def/grammar/tokens/keyword.lfy:25
        /// A basic loop that runs until it encounters a break
        LoopKeyword = r#""loop""#, // @lfy def/grammar/tokens/keyword.lfy:26
        /// A match expression or block that branches based on condition statements
        MatchKeyword = r#""match""#, // @lfy def/grammar/tokens/keyword.lfy:27
        /// A matchall expression or block that applies the results of all condition statements that are true
        MatchallKeyword = r#""matchall""#, // @lfy def/grammar/tokens/keyword.lfy:28
        /// Referencing a type, variable, or data structures' keys
        OfKeyword = r#""of""#, // @lfy def/grammar/tokens/keyword.lfy:29
        /// Branch conditions in contexts like the "where" syntax sugar keyword
        OrKeyword = r#""or""#, // @lfy def/grammar/tokens/keyword.lfy:30
        /// Return expression that ends execution of a function and returns a value or set of values
        ReturnKeyword = r#""return""#, // @lfy def/grammar/tokens/keyword.lfy:31
        /// Group of shared definitions, properties, configuration, and other information that can be easily added to data, functions or other kinds of variables
        TraitKeyword = r#""trait""#, // @lfy def/grammar/tokens/keyword.lfy:32
        /// Defining a type map
        TypeKeyword = r#""type""#, // @lfy def/grammar/tokens/keyword.lfy:33
        /// Add the scoped contents of a file, folder, or library to the current scope
        UseKeyword = r#""use""#, // @lfy def/grammar/tokens/keyword.lfy:34
        /// A while loop
        WhileKeyword = r#""while""#, // @lfy def/grammar/tokens/keyword.lfy:35
        /// Syntax sugar for @acceptanceCriteria.add
        WhereKeyword = r#""where""#, // @lfy def/grammar/tokens/keyword.lfy:36
        /// Sets the scope for a block
        WithKeyword = r#""with""#, // @lfy def/grammar/tokens/keyword.lfy:37

        /// Primitive boolean type
        PrimitiveBooleanKeyword = r#""boolean""#, // @lfy def/grammar/tokens/keyword.lfy:39
        /// Primitive number type
        PrimitiveNumberKeyword = r#""number""#, // @lfy def/grammar/tokens/keyword.lfy:40
        /// Primitive object type
        PrimitiveObjectKeyword = r#""object""#, // @lfy def/grammar/tokens/keyword.lfy:41
        /// Primitive string type
        PrimitiveStringKeyword = r#""string""#, // @lfy def/grammar/tokens/keyword.lfy:42

        /// Reserved for future use
        AbstractKeyword = r#""abstract""#, // @lfy def/grammar/tokens/keyword.lfy:44
        /// Reserved for future use
        CaseKeyword = r#""case""#, // @lfy def/grammar/tokens/keyword.lfy:45
        /// Reserved for future use
        ClassKeyword = r#""class""#, // @lfy def/grammar/tokens/keyword.lfy:46
        /// Reserved for future use
        ImplKeyword = r#""impl""#, // @lfy def/grammar/tokens/keyword.lfy:47
        /// Reserved for future use
        InterfaceKeyword = r#""interface""#, // @lfy def/grammar/tokens/keyword.lfy:48
        /// Reserved for future use
        NewKeyword = r#""new""#, // @lfy def/grammar/tokens/keyword.lfy:49
        /// Reserved for future use
        PrivateKeyword = r#""private""#, // @lfy def/grammar/tokens/keyword.lfy:50
        /// Reserved for future use
        PublicKeyword = r#""public""#, // @lfy def/grammar/tokens/keyword.lfy:51
        /// Reserved for future use
        SelfKeyword = r#""self""#, // @lfy def/grammar/tokens/keyword.lfy:52
        /// Reserved for future use
        StaticKeyword = r#""static""#, // @lfy def/grammar/tokens/keyword.lfy:53
        /// Reserved for future use
        SuperKeyword = r#""super""#, // @lfy def/grammar/tokens/keyword.lfy:54
        /// Reserved for future use
        SwitchKeyword = r#""switch""#, // @lfy def/grammar/tokens/keyword.lfy:55
        /// Reserved for future use
        ThenKeyword = r#""then""#, // @lfy def/grammar/tokens/keyword.lfy:56
        /// Reserved for future use
        TypeofKeyword = r#""typeof""#, // @lfy def/grammar/tokens/keyword.lfy:57
        /// Reserved for future use
        YieldKeyword = r#""yield""#, // @lfy def/grammar/tokens/keyword.lfy:58
    }
}

impl Keyword {
    /// The reserved word itself (the syntax without its quotes).
    pub fn text(self) -> &'static str {
        let syntax = self.syntax();
        &syntax[1..syntax.len() - 1]
    }

    /// The keyword whose text is exactly `text`, if any.
    pub fn from_text(text: &str) -> Option<Keyword> {
        Keyword::ALL
            .iter()
            .copied()
            .find(|keyword| keyword.text() == text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/grammar/tokens/keyword.lfy:3
    #[test]
    fn keyword_texts_are_unique_and_round_trip() {
        for &keyword in Keyword::ALL {
            assert!(keyword.syntax().starts_with('"') && keyword.syntax().ends_with('"'));
            assert_eq!(Keyword::from_text(keyword.text()), Some(keyword));
            assert!(keyword.identifier().ends_with("Keyword"));
        }
        assert_eq!(Keyword::ALL.len(), 54);
        assert_eq!(Keyword::from_text("true"), None);
        assert_eq!(Keyword::from_text("with"), Some(Keyword::WithKeyword));
    }
}
