//! Compiled from `def/grammar/terminals/keyword.lfy`.

use super::super::traits::category::*;
use super::super::{Binding, Entity, GrammarRule, Level, grammar_rules};

grammar_rules! {
    /// Keywords: words with fixed meaning in the grammar, none of which can be an
    /// identifier.
    pub enum Keyword {
        // Declarations
        AgentDataKeyword is [keyword("d")]: "Begins a data declaration whose implementation is generated" = r#""d""#, // @lfy def/grammar/terminals/keyword.lfy:AgentDataKeyword
        AgentFunctionKeyword is [keyword("fn")]: "Begins a function whose implementation is generated" = r#""fn""#, // @lfy def/grammar/terminals/keyword.lfy:AgentFunctionKeyword
        FunctionKeyword is [keyword("function")]: "Begins a function written out in full; also the function type" = r#""function""#, // @lfy def/grammar/terminals/keyword.lfy:FunctionKeyword
        TraitKeyword is [keyword("trait")]: "Begins a reusable component that can be applied to declarations" = r#""trait""#, // @lfy def/grammar/terminals/keyword.lfy:TraitKeyword
        TypeKeyword is [keyword("type")]: "Begins a structural type" = r#""type""#, // @lfy def/grammar/terminals/keyword.lfy:TypeKeyword
        EnumKeyword is [keyword("enum")]: "Begins a set of named values" = r#""enum""#, // @lfy def/grammar/terminals/keyword.lfy:EnumKeyword
        AliasKeyword is [keyword("alias")]: "Binds a new name to something already named" = r#""alias""#, // @lfy def/grammar/terminals/keyword.lfy:Keyword.rule
        ConstKeyword is [keyword("const")]: "Declares a name that is bound once" = r#""const""#, // @lfy def/grammar/terminals/keyword.lfy:ConstKeyword
        LetKeyword is [keyword("let")]: "Declares a name that can be rebound" = r#""let""#, // @lfy def/grammar/terminals/keyword.lfy:LetKeyword
        ExternalKeyword is [keyword("external")]: "Declares a name whose definition lives outside the program" = r#""external""#, // @lfy def/grammar/terminals/keyword.lfy:ExternalKeyword
        UseKeyword is [keyword("use")]: "Brings another file's declarations into scope" = r#""use""#, // @lfy def/grammar/terminals/keyword.lfy:UseKeyword

        // Clauses
        IsKeyword is [keyword("is"), Binding::left(Level::Relational)]: "Applies traits to a declaration, or names a type by its trait" = r#""is""#, // @lfy def/grammar/terminals/keyword.lfy:IsKeyword
        ExtendsKeyword is [keyword("extends")]: "Makes a declaration include the listed traits or data" = r#""extends""#, // @lfy def/grammar/terminals/keyword.lfy:ExtendsKeyword
        AsKeyword is [keyword("as"), Binding::left(Level::Relational)]: "Names a module, or casts a value" = r#""as""#, // @lfy def/grammar/terminals/keyword.lfy:AsKeyword
        WithKeyword is [keyword("with")]: "Runs a block with another entity as the current one" = r#""with""#, // @lfy def/grammar/terminals/keyword.lfy:WithKeyword
        WhereKeyword is [keyword("where")]: "Adds an acceptance criterion in condition and consequence form" = r#""where""#, // @lfy def/grammar/terminals/keyword.lfy:WhereKeyword
        AndKeyword is [keyword("and")]: "Joins where conditions that must all hold" = r#""and""#, // @lfy def/grammar/terminals/keyword.lfy:AndKeyword
        OrKeyword is [keyword("or")]: "Joins where conditions of which one must hold" = r#""or""#, // @lfy def/grammar/terminals/keyword.lfy:OrKeyword
        DefaultKeyword is [keyword("default")]: "The match arm taken when no other does" = r#""default""#, // @lfy def/grammar/terminals/keyword.lfy:DefaultKeyword

        // Control
        IfKeyword is [keyword("if")]: "Runs a statement when a condition holds" = r#""if""#, // @lfy def/grammar/terminals/keyword.lfy:IfKeyword
        ElseKeyword is [keyword("else")]: "Runs a statement when the if condition does not hold" = r#""else""#, // @lfy def/grammar/terminals/keyword.lfy:ElseKeyword
        ForKeyword is [keyword("for")]: "Loops over iterables" = r#""for""#, // @lfy def/grammar/terminals/keyword.lfy:ForKeyword
        InKeyword is [keyword("in"), Binding::right(Level::Extraction)]: "The values of an iterable" = r#""in""#, // @lfy def/grammar/terminals/keyword.lfy:InKeyword
        OfKeyword is [keyword("of"), Binding::right(Level::Extraction)]: "The keys of an iterable" = r#""of""#, // @lfy def/grammar/terminals/keyword.lfy:OfKeyword
        FromKeyword is [keyword("from"), Binding::right(Level::Extraction)]: "The keys and values of an iterable" = r#""from""#, // @lfy def/grammar/terminals/keyword.lfy:FromKeyword
        WhileKeyword is [keyword("while")]: "Loops while a condition holds" = r#""while""#, // @lfy def/grammar/terminals/keyword.lfy:WhileKeyword
        LoopKeyword is [keyword("loop")]: "Loops until a break" = r#""loop""#, // @lfy def/grammar/terminals/keyword.lfy:LoopKeyword
        BreakKeyword is [keyword("break")]: "Leaves the innermost loop" = r#""break""#, // @lfy def/grammar/terminals/keyword.lfy:BreakKeyword
        ContinueKeyword is [keyword("continue")]: "Starts the next iteration of the innermost loop" = r#""continue""#, // @lfy def/grammar/terminals/keyword.lfy:ContinueKeyword
        ReturnKeyword is [keyword("return")]: "Ends a function with a value" = r#""return""#, // @lfy def/grammar/terminals/keyword.lfy:ReturnKeyword
        MatchKeyword is [keyword("match")]: "Takes the first arm whose pattern matches" = r#""match""#, // @lfy def/grammar/terminals/keyword.lfy:MatchKeyword
        MatchallKeyword is [keyword("matchall")]: "Takes every arm whose pattern matches" = r#""matchall""#, // @lfy def/grammar/terminals/keyword.lfy:MatchallKeyword
        AsyncKeyword is [keyword("async")]: "Marks a statement that runs as a promise" = r#""async""#, // @lfy def/grammar/terminals/keyword.lfy:AsyncKeyword
        AwaitKeyword is [keyword("await"), Binding::non_associative(Level::Wrapper)]: "Waits for a promise" = r#""await""#, // @lfy def/grammar/terminals/keyword.lfy:AwaitKeyword
        AceKeyword is [keyword("ace")]: "Marks a statement that runs at compile time" = r#""ace""#, // @lfy def/grammar/terminals/keyword.lfy:AceKeyword

        // Values and types
        NullKeyword is [keyword("null")]: "The null value" = r#""null""#, // @lfy def/grammar/terminals/keyword.lfy:NullKeyword
        UndefinedKeyword is [keyword("undefined")]: "The undefined value" = r#""undefined""#, // @lfy def/grammar/terminals/keyword.lfy:UndefinedKeyword
        TrueKeyword is [keyword("true")]: "The boolean true value" = r#""true""#, // @lfy def/grammar/terminals/keyword.lfy:TrueKeyword
        FalseKeyword is [keyword("false")]: "The boolean false value" = r#""false""#, // @lfy def/grammar/terminals/keyword.lfy:FalseKeyword
        BooleanKeyword is [keyword("boolean")]: "The boolean type" = r#""boolean""#, // @lfy def/grammar/terminals/keyword.lfy:BooleanKeyword
        NumberKeyword is [keyword("number")]: "The number type" = r#""number""#, // @lfy def/grammar/terminals/keyword.lfy:NumberKeyword
        StringKeyword is [keyword("string")]: "The string type" = r#""string""#, // @lfy def/grammar/terminals/keyword.lfy:StringKeyword
        ObjectKeyword is [keyword("object")]: "The object type" = r#""object""#, // @lfy def/grammar/terminals/keyword.lfy:ObjectKeyword

        // Reserved: keywords no rule uses yet
        AbstractKeyword is [reserved("abstract")] = r#""abstract""#, // @lfy def/grammar/terminals/keyword.lfy:AbstractKeyword
        CaseKeyword is [reserved("case")] = r#""case""#, // @lfy def/grammar/terminals/keyword.lfy:CaseKeyword
        ClassKeyword is [reserved("class")] = r#""class""#, // @lfy def/grammar/terminals/keyword.lfy:ClassKeyword
        ImplKeyword is [reserved("impl")] = r#""impl""#, // @lfy def/grammar/terminals/keyword.lfy:ImplKeyword
        InterfaceKeyword is [reserved("interface")] = r#""interface""#, // @lfy def/grammar/terminals/keyword.lfy:InterfaceKeyword
        NewKeyword is [reserved("new")] = r#""new""#, // @lfy def/grammar/terminals/keyword.lfy:NewKeyword
        ModuleKeyword is [reserved("module")] = r#""module""#, // @lfy def/grammar/terminals/keyword.lfy:ModuleKeyword
        PrivateKeyword is [reserved("private")] = r#""private""#, // @lfy def/grammar/terminals/keyword.lfy:PrivateKeyword
        PublicKeyword is [reserved("public")] = r#""public""#, // @lfy def/grammar/terminals/keyword.lfy:PublicKeyword
        SelfKeyword is [reserved("self")] = r#""self""#, // @lfy def/grammar/terminals/keyword.lfy:SelfKeyword
        StaticKeyword is [reserved("static")] = r#""static""#, // @lfy def/grammar/terminals/keyword.lfy:StaticKeyword
        SuperKeyword is [reserved("super")] = r#""super""#, // @lfy def/grammar/terminals/keyword.lfy:SuperKeyword
        SwitchKeyword is [reserved("switch")] = r#""switch""#, // @lfy def/grammar/terminals/keyword.lfy:SwitchKeyword
        ThenKeyword is [reserved("then")] = r#""then""#, // @lfy def/grammar/terminals/keyword.lfy:ThenKeyword
        TypeofKeyword is [reserved("typeof")] = r#""typeof""#, // @lfy def/grammar/terminals/keyword.lfy:TypeofKeyword
        YieldKeyword is [reserved("yield")] = r#""yield""#, // @lfy def/grammar/terminals/keyword.lfy:YieldKeyword

        /// `alternationList(...keyword@entities)`
        Keyword is [alternation_list(KEYWORDS)]: "Any keyword" = "[[AgentDataKeyword]] | [[AgentFunctionKeyword]] | [[FunctionKeyword]] | [[TraitKeyword]] | [[TypeKeyword]] | [[EnumKeyword]] | [[AliasKeyword]] | [[ConstKeyword]] | [[LetKeyword]] | [[ExternalKeyword]] | [[UseKeyword]] | [[IsKeyword]] | [[ExtendsKeyword]] | [[AsKeyword]] | [[WithKeyword]] | [[WhereKeyword]] | [[AndKeyword]] | [[OrKeyword]] | [[DefaultKeyword]] | [[IfKeyword]] | [[ElseKeyword]] | [[ForKeyword]] | [[InKeyword]] | [[OfKeyword]] | [[FromKeyword]] | [[WhileKeyword]] | [[LoopKeyword]] | [[BreakKeyword]] | [[ContinueKeyword]] | [[ReturnKeyword]] | [[MatchKeyword]] | [[MatchallKeyword]] | [[AsyncKeyword]] | [[AwaitKeyword]] | [[AceKeyword]] | [[NullKeyword]] | [[UndefinedKeyword]] | [[TrueKeyword]] | [[FalseKeyword]] | [[BooleanKeyword]] | [[NumberKeyword]] | [[StringKeyword]] | [[ObjectKeyword]] | [[AbstractKeyword]] | [[CaseKeyword]] | [[ClassKeyword]] | [[ImplKeyword]] | [[InterfaceKeyword]] | [[NewKeyword]] | [[ModuleKeyword]] | [[PrivateKeyword]] | [[PublicKeyword]] | [[SelfKeyword]] | [[StaticKeyword]] | [[SuperKeyword]] | [[SwitchKeyword]] | [[ThenKeyword]] | [[TypeofKeyword]] | [[YieldKeyword]]", // @lfy def/grammar/terminals/keyword.lfy:Keyword
    }
}

/// `keyword@entities`: every rule with the `keyword` trait (reserved ones included), in
/// declaration order.
// @lfy def/grammar/terminals/keyword.lfy:Keyword
pub const KEYWORDS: &[Entity] = &[
    Entity::Keyword(Keyword::AgentDataKeyword),
    Entity::Keyword(Keyword::AgentFunctionKeyword),
    Entity::Keyword(Keyword::FunctionKeyword),
    Entity::Keyword(Keyword::TraitKeyword),
    Entity::Keyword(Keyword::TypeKeyword),
    Entity::Keyword(Keyword::EnumKeyword),
    Entity::Keyword(Keyword::AliasKeyword),
    Entity::Keyword(Keyword::ConstKeyword),
    Entity::Keyword(Keyword::LetKeyword),
    Entity::Keyword(Keyword::ExternalKeyword),
    Entity::Keyword(Keyword::UseKeyword),
    Entity::Keyword(Keyword::IsKeyword),
    Entity::Keyword(Keyword::ExtendsKeyword),
    Entity::Keyword(Keyword::AsKeyword),
    Entity::Keyword(Keyword::WithKeyword),
    Entity::Keyword(Keyword::WhereKeyword),
    Entity::Keyword(Keyword::AndKeyword),
    Entity::Keyword(Keyword::OrKeyword),
    Entity::Keyword(Keyword::DefaultKeyword),
    Entity::Keyword(Keyword::IfKeyword),
    Entity::Keyword(Keyword::ElseKeyword),
    Entity::Keyword(Keyword::ForKeyword),
    Entity::Keyword(Keyword::InKeyword),
    Entity::Keyword(Keyword::OfKeyword),
    Entity::Keyword(Keyword::FromKeyword),
    Entity::Keyword(Keyword::WhileKeyword),
    Entity::Keyword(Keyword::LoopKeyword),
    Entity::Keyword(Keyword::BreakKeyword),
    Entity::Keyword(Keyword::ContinueKeyword),
    Entity::Keyword(Keyword::ReturnKeyword),
    Entity::Keyword(Keyword::MatchKeyword),
    Entity::Keyword(Keyword::MatchallKeyword),
    Entity::Keyword(Keyword::AsyncKeyword),
    Entity::Keyword(Keyword::AwaitKeyword),
    Entity::Keyword(Keyword::AceKeyword),
    Entity::Keyword(Keyword::NullKeyword),
    Entity::Keyword(Keyword::UndefinedKeyword),
    Entity::Keyword(Keyword::TrueKeyword),
    Entity::Keyword(Keyword::FalseKeyword),
    Entity::Keyword(Keyword::BooleanKeyword),
    Entity::Keyword(Keyword::NumberKeyword),
    Entity::Keyword(Keyword::StringKeyword),
    Entity::Keyword(Keyword::ObjectKeyword),
    Entity::Keyword(Keyword::AbstractKeyword),
    Entity::Keyword(Keyword::CaseKeyword),
    Entity::Keyword(Keyword::ClassKeyword),
    Entity::Keyword(Keyword::ImplKeyword),
    Entity::Keyword(Keyword::InterfaceKeyword),
    Entity::Keyword(Keyword::NewKeyword),
    Entity::Keyword(Keyword::ModuleKeyword),
    Entity::Keyword(Keyword::PrivateKeyword),
    Entity::Keyword(Keyword::PublicKeyword),
    Entity::Keyword(Keyword::SelfKeyword),
    Entity::Keyword(Keyword::StaticKeyword),
    Entity::Keyword(Keyword::SuperKeyword),
    Entity::Keyword(Keyword::SwitchKeyword),
    Entity::Keyword(Keyword::ThenKeyword),
    Entity::Keyword(Keyword::TypeofKeyword),
    Entity::Keyword(Keyword::YieldKeyword),
];

impl Keyword {
    /// The word itself for a keyword rule; `None` for the `Keyword` alternation list.
    pub fn word(self) -> Option<&'static str> {
        match self.category() {
            super::super::Category::Terminal(terminal) => terminal.fixed_text(),
            _ => None,
        }
    }

    /// The keyword whose word is exactly `text`, if any.
    // @lfy def/grammar/traits.lfy:45
    pub fn from_text(text: &str) -> Option<Keyword> {
        KEYWORDS.iter().find_map(|rule| match rule {
            Entity::Keyword(keyword) if keyword.word() == Some(text) => Some(*keyword),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // @lfy def/grammar/terminals/keyword.lfy:AgentDataKeyword
    #[test]
    fn keyword_words_are_unique_and_round_trip() {
        for &rule in KEYWORDS {
            let Entity::Keyword(keyword) = rule else {
                panic!("{rule} is not in the keyword enum");
            };
            assert!(keyword.is_keyword(), "{rule}");
            let text = keyword.word().unwrap();
            assert_eq!(keyword.syntax(), format!("\"{text}\""));
            assert_eq!(Keyword::from_text(text), Some(keyword));
            assert!(keyword.identifier().ends_with("Keyword"));
        }
        assert_eq!(KEYWORDS.len(), 59);
        assert_eq!(Keyword::ALL.len(), 60);
        assert_eq!(Keyword::from_text("constant"), None);
        assert_eq!(Keyword::from_text("with"), Some(Keyword::WithKeyword));
        assert_eq!(Keyword::from_text("true"), Some(Keyword::TrueKeyword));
        assert_eq!(Keyword::from_text("Keyword"), None);
        assert_eq!(Keyword::Keyword.word(), None);
        assert!(Keyword::Keyword.is_alternation_list() && !Keyword::Keyword.is_keyword());
    }

    // @lfy def/grammar/terminals/keyword.lfy:AbstractKeyword
    #[test]
    fn reserved_keywords_are_keywords_with_no_definition() {
        let reserved: Vec<Keyword> = Keyword::ALL
            .iter()
            .copied()
            .filter(|keyword| {
                matches!(
                    keyword.category(),
                    super::super::super::Category::Terminal(
                        super::super::super::Terminal::Keyword { reserved: true, .. }
                    )
                )
            })
            .collect();
        assert_eq!(reserved.len(), 16);
        assert_eq!(reserved[0], Keyword::AbstractKeyword);
        assert_eq!(reserved[15], Keyword::YieldKeyword);
        assert!(
            reserved
                .iter()
                .all(|keyword| keyword.definition().is_empty())
        );
        assert!(!Keyword::IfKeyword.definition().is_empty());
    }

    // @lfy def/grammar/terminals/keyword.lfy:IsKeyword
    #[test]
    fn only_the_declared_keywords_bind() {
        for &keyword in Keyword::ALL {
            let expected = match keyword {
                Keyword::IsKeyword | Keyword::AsKeyword => Some(Binding::left(Level::Relational)),
                Keyword::InKeyword | Keyword::OfKeyword | Keyword::FromKeyword => {
                    Some(Binding::right(Level::Extraction))
                }
                Keyword::AwaitKeyword => Some(Binding::non_associative(Level::Wrapper)),
                _ => None,
            };
            assert_eq!(keyword.binding(), expected, "{}", keyword.identifier());
        }
    }
}
