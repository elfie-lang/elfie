// @lfy def/query/main.lfy:semanticTokensOf
//! Every name of a file, classified by meaning for an editor to color.

use super::*;
use crate::grammar::GrammarRule;
use crate::grammar::rules::expression::Expression as E;
use crate::grammar::terminals::comment::Comment as C;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::model::{ContextProperty, Layer};

/// The data the prelude gives an entity for what it is; `Entity` covers every entity and
/// the others only their own kind, as the binder reads them.
// @lfy def/query/main.lfy:semanticTokensOf
const KIND_DATA: [&str; 10] = [
    "Entity", "Trait", "Function", "Data", "Type", "Enum", "Member", "Parameter", "Variable",
    "Module",
];

/// Whether a name is a member of any kind data: one of another kind yields undefined for
/// an entity, and is still a member and not a name of its own.
// Decision: with no prelude the context layer gives every entity the properties
// [`ContextProperty`] lists, as the binder falls back to them too.
// @lfy def/query/main.lfy:semanticTokensOf
fn is_kind_member(model: &Model, name: &str) -> bool {
    if prelude_data(model, "Entity").is_none() {
        return ContextProperty::lookup(name).is_some();
    }
    KIND_DATA.iter().any(|kind| {
        prelude_data(model, kind)
            .map(|data| member_symbols(model, data))
            .is_some_and(|members| {
                members
                    .into_iter()
                    .any(|symbol| model.symbols[symbol].name == name)
            })
    })
}

/// The semantic token type of a symbol, by its kind; an alias by the entity it is bound
/// to.
// @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:b5097f3e8a9bd502be04b8014dabcf1f1547271685407ae1e2223de4a7075b1e
fn token_type_of(model: &Model, symbol: SymbolId) -> TokenType {
    let symbol = &model.symbols[symbol];
    match symbol.kind {
        SymbolKind::Data => TokenType::Data,
        SymbolKind::Trait => TokenType::Trait,
        SymbolKind::Type => TokenType::Type,
        SymbolKind::Enum => TokenType::Enum,
        SymbolKind::EnumMember => TokenType::EnumMember,
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:7553927202180ccb4a1480c855ff7bce209d8347bd717f503c9e26e48a4e853e
        SymbolKind::Function | SymbolKind::AgentFunction => TokenType::Function,
        SymbolKind::Parameter => TokenType::Parameter,
        // A type parameter is a type parameter wherever its name appears: in the
        // `TypeParameters` that lists it, in a type position, and in an expression alike,
        // because the symbol it resolves to is the same everywhere.
        SymbolKind::TypeParameter => TokenType::TypeParameter,
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:a984da220516d1eb9f619741ae9b54ac88167c3170597684aa511c4a78237250
        SymbolKind::Variable | SymbolKind::LoopVariable | SymbolKind::External => {
            TokenType::Variable
        }
        // A member is a property whatever its value is, a function among the rest.
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:4030ad0c418b90242ba53d2775af7f8d8c83378db270cb127458b34b20de5488
        SymbolKind::Member => TokenType::Property,
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:5a6ddde0f77237867aec3554925d122dfb6dc9d6b04bb03baeadc32bbe06532b
        SymbolKind::Module => TokenType::Namespace,
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:6ec488a19a8bed1c16f06d4a1611f4e4b0e65e6f2197c3f7fa8ea4c3380a02fe
        SymbolKind::Alias => match &model.entities[symbol.entity].kind {
            EntityKind::Data => TokenType::Data,
            EntityKind::Trait { .. } => TokenType::Trait,
            EntityKind::Type => TokenType::Type,
            EntityKind::Enum => TokenType::Enum,
            EntityKind::EnumMember => TokenType::EnumMember,
            EntityKind::Fn { .. } => TokenType::Function,
            EntityKind::File | EntityKind::Module => TokenType::Namespace,
            EntityKind::Member => TokenType::Property,
            EntityKind::Parameter => TokenType::Parameter,
            _ => TokenType::Variable,
        },
    }
}

/// Whether the entity a symbol is bound to was declared with `d` or `fn`.
// @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:4e4adb43507288877e4db588fd52ac3ef62e096cc3a05cffa8b16348c956995c
fn is_agentic(model: &Model, symbol: SymbolId) -> bool {
    let entity = &model.entities[model.symbols[symbol].entity];
    matches!(
        entity.kind,
        EntityKind::Data | EntityKind::Fn { agent: true, .. }
    )
}

/// Whether a symbol is a variable declared with `const`.
// @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:5eefc067fa28b96019e3711925aedc4c3768d4a6b2acb288419a23fd9c448d25
fn is_readonly(model: &Model, symbol: SymbolId) -> bool {
    let symbol = &model.symbols[symbol];
    if symbol.kind != SymbolKind::Variable || !is_real(model, symbol.node) {
        return false;
    }
    let node = model.node(symbol.node);
    node.token(K::ConstKeyword, model.tokens(symbol.node.file))
        .is_some()
}

/// Whether a node lies inside a `TemplateReference`, a `TemplateExecution`, or
/// `Documentation`.
// @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:929a8a48590d4dfcb4256bca6989606d777cf6d9ee2e49acd1a6bfa1f110c5c3
fn in_prose(model: &Model, node: NodeRef) -> bool {
    let mut current = Some(node);
    while let Some(at) = current {
        let rule = model.info(at).rule;
        if rule == E::TemplateReference.entity()
            || rule == E::TemplateExecution.entity()
            || rule == C::Documentation.entity()
        {
            return true;
        }
        current = model.parent(at);
    }
    false
}

/// Modifiers as a set, given back in `TokenModifier` order with each at most once.
// @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:257cf12f2043369c4f7b1756bac875bb567b96424f0241706a296272661fa75b
fn modifier_list(set: u32) -> Vec<TokenModifier> {
    TokenModifier::ALL
        .into_iter()
        .filter(|modifier| set & (1 << modifier.index()) != 0)
        .collect()
}

fn with_modifier(set: &mut u32, modifier: TokenModifier) {
    *set |= 1 << modifier.index();
}

/// Every name of a file, classified by meaning for an editor to color.
///
/// One token per token that spells a declared name or a used name, a member of a kind
/// data after the context accessor among them; keywords, punctuation, strings, numbers,
/// and comments get none. Tokens are in position order and never overlap; a file not in
/// the program gives an empty list.
// @lfy def/query/main.lfy:semanticTokensOf
pub fn semantic_tokens_of(workspace: &Workspace, file: &str) -> Vec<SemanticToken> {
    let model = &workspace.model;
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:267b17e8c1e5a951c9981534d31e87180bedced640f519c1fea34a6ebcb08ae2
    let Some(file_id) = file_of(workspace, file) else {
        return Vec::new();
    };
    // Keyed by token index, so the result is in position order and no two overlap.
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:3b27c9ca5a3e24fd29d4157ee2bfa19d5898698dad2b7c38af7e81e4ceb7a64d
    let mut found: std::collections::BTreeMap<usize, SemanticToken> =
        std::collections::BTreeMap::new();
    // Declarations: only a symbol's own name token, which is never a keyword, a piece of
    // punctuation, a string, a number, or a comment.
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:9ad5e72c68be0bdeeebbd17ad580e0a72eab72354e4da0434ddae56182063354
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:851770c083f006b02cc5d2a02798a436922a8ff2ae64f24665c3254b61353fa8
    for (id, symbol) in model.symbols.iter().enumerate() {
        if !is_real(model, symbol.node) || symbol.node.file != file_id {
            continue;
        }
        let Some(token) = symbol.name_token else {
            continue;
        };
        if found.contains_key(&token) {
            continue;
        }
        let mut set = 0;
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:3d5de36e57170404015c3fd3283d61ce639c92d2761518d47c0cd218f5470c0d
        with_modifier(&mut set, TokenModifier::Declaration);
        if is_agentic(model, id) {
            with_modifier(&mut set, TokenModifier::Agentic);
        }
        if is_readonly(model, id) {
            with_modifier(&mut set, TokenModifier::Readonly);
        }
        found.insert(
            token,
            SemanticToken {
                range: span(workspace, file_id, token, token + 1),
                ty: token_type_of(model, id),
                modifiers: modifier_list(set),
            },
        );
    }
    // Usages: a usage that spells no name, and one whose token is not a name, get none; a
    // token already carrying a declaration keeps it, so declaration is on it alone.
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:9ad5e72c68be0bdeeebbd17ad580e0a72eab72354e4da0434ddae56182063354
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:851770c083f006b02cc5d2a02798a436922a8ff2ae64f24665c3254b61353fa8
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:2c4baa798268254fe796a7177ce7e181039ce3f2c8da249d8d58acbd488e660e
    for usage in &model.usages {
        if usage.node.file != file_id || usage.name.is_none() {
            continue;
        }
        let Some(token) = usage.token else { continue };
        if found.contains_key(&token) || !is_name_token(&model.tokens(file_id)[token]) {
            continue;
        }
        let mut set = 0;
        let rule = model.info(usage.node).rule;
        let accessed = rule == E::Current.entity() || rule == E::Member.entity();
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:fd59af79b69b792d0a46a4e76d8ebfedb0de64782a023593d40b457931d966b0
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:c4f711c354ffdeeec2e978fa878ed96e20f487f54db0f996c048c56ca0682c3b
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:c597e906a0e9079e77628eb66872af16f638fc6b43ea00c501a100e1e353e39f
        if accessed {
            match usage.layer {
                Layer::Context => with_modifier(&mut set, TokenModifier::Context),
                Layer::Scope | Layer::Parent => with_modifier(&mut set, TokenModifier::Scope),
                Layer::Value => with_modifier(&mut set, TokenModifier::Value),
                Layer::Dereference | Layer::Previous => {}
            }
        }
        if in_prose(model, usage.node) {
            with_modifier(&mut set, TokenModifier::Documentation);
        }
        let ty = match usage.symbol {
            Some(symbol) => {
                if is_agentic(model, symbol) {
                    with_modifier(&mut set, TokenModifier::Agentic);
                }
                if is_readonly(model, symbol) {
                    with_modifier(&mut set, TokenModifier::Readonly);
                }
                token_type_of(model, symbol)
            }
            // A name after the context accessor that is a member of a kind data other than
            // the left side's yields undefined for it and is still a property.
            // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:bfc5baaec53553979f5f12c554d2060fa9a91a3e6446f3329a8e91d5db495c9c
            None if usage.layer == Layer::Context
                && is_kind_member(model, usage.name.as_deref().unwrap_or_default()) =>
            {
                with_modifier(&mut set, TokenModifier::Unresolved);
                TokenType::Property
            }
            None => {
                // A member of something the model does not know got no problem and names no
                // member of any kind data, so it is not a name of the program.
                // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:05cd36753531cfdfd96d75443056536cbe666c99424ac4ad3a90e22d205cd79b
                if !model
                    .problems
                    .iter()
                    .any(|problem| problem.node == usage.node)
                {
                    continue;
                }
                // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:c35e767a966f9b0f4bd9befc33e6221d14d56bb8f00fb071a1c2ea1dc14f7673
                with_modifier(&mut set, TokenModifier::Unresolved);
                TokenType::Variable
            }
        };
        found.insert(
            token,
            SemanticToken {
                range: span(workspace, file_id, token, token + 1),
                ty,
                modifiers: modifier_list(set),
            },
        );
    }
    found.into_values().collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::query::tests::*;
    use crate::workspace;

    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:9ad5e72c68be0bdeeebbd17ad580e0a72eab72354e4da0434ddae56182063354
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:851770c083f006b02cc5d2a02798a436922a8ff2ae64f24665c3254b61353fa8
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:b5097f3e8a9bd502be04b8014dabcf1f1547271685407ae1e2223de4a7075b1e
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:4030ad0c418b90242ba53d2775af7f8d8c83378db270cb127458b34b20de5488
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:3d5de36e57170404015c3fd3283d61ce639c92d2761518d47c0cd218f5470c0d
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:2c4baa798268254fe796a7177ce7e181039ce3f2c8da249d8d58acbd488e660e
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:4e4adb43507288877e4db588fd52ac3ef62e096cc3a05cffa8b16348c956995c
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:5eefc067fa28b96019e3711925aedc4c3768d4a6b2acb288419a23fd9c448d25
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:c4f711c354ffdeeec2e978fa878ed96e20f487f54db0f996c048c56ca0682c3b
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:257cf12f2043369c4f7b1756bac875bb567b96424f0241706a296272661fa75b
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:430c48f672d369d5cc541511bf45e6b2aaa2f3456b98e7bc2156c2740dd20a2e
    #[test]
    fn semantic_tokens_classify_declarations_and_layers() {
        let text = "d A { $x: `d` = string; } const y = A$x;";
        let fixture = Fixture::one(text);
        let workspace = fixture.load();
        let tokens = semantic_tokens_of(&workspace, A);
        let summary: Vec<(String, TokenType, Vec<TokenModifier>)> = tokens
            .iter()
            .map(|t| {
                let index = token_at(&workspace, A, t.range.start).unwrap();
                (
                    token(&workspace, 0, index).value.clone(),
                    t.ty,
                    t.modifiers.clone(),
                )
            })
            .collect();
        use TokenModifier::*;
        assert_eq!(
            summary,
            vec![
                ("A".to_string(), TokenType::Data, vec![Declaration, Agentic]),
                ("x".to_string(), TokenType::Property, vec![Declaration]),
                (
                    "y".to_string(),
                    TokenType::Variable,
                    vec![Declaration, Readonly]
                ),
                ("A".to_string(), TokenType::Data, vec![Agentic]),
                ("x".to_string(), TokenType::Property, vec![Scope]),
            ]
        );
        // In position order, never overlapping.
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:3b27c9ca5a3e24fd29d4157ee2bfa19d5898698dad2b7c38af7e81e4ceb7a64d
        for pair in tokens.windows(2) {
            assert!(
                pair[0].range.end <= pair[1].range.start
                    || pair[0].range.end.line < pair[1].range.start.line
            );
        }
    }

    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:7553927202180ccb4a1480c855ff7bce209d8347bd717f503c9e26e48a4e853e
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:bfc5baaec53553979f5f12c554d2060fa9a91a3e6446f3329a8e91d5db495c9c
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:c35e767a966f9b0f4bd9befc33e6221d14d56bb8f00fb071a1c2ea1dc14f7673
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:05cd36753531cfdfd96d75443056536cbe666c99424ac4ad3a90e22d205cd79b
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:4e4adb43507288877e4db588fd52ac3ef62e096cc3a05cffa8b16348c956995c
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:fd59af79b69b792d0a46a4e76d8ebfedb0de64782a023593d40b457931d966b0
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:929a8a48590d4dfcb4256bca6989606d777cf6d9ee2e49acd1a6bfa1f110c5c3
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:257cf12f2043369c4f7b1756bac875bb567b96424f0241706a296272661fa75b
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:f7e95d099032dc39875a99af987f12943e4070f473b00cb0eb86f3899f2ecc39
    #[test]
    fn semantic_tokens_mark_prose_context_properties_and_unresolved_names() {
        let text = "fn f(): `See [[g]]` => number { @acceptanceCriteria.add({ behavior = `b` }); }";
        let fixture = Fixture::with_prelude(text);
        let workspace = fixture.load();
        let tokens = semantic_tokens_of(&workspace, A);
        let names: Vec<(String, TokenType, Vec<TokenModifier>)> = tokens
            .iter()
            .map(|t| {
                let index = token_at(&workspace, A, t.range.start).unwrap();
                (
                    token(&workspace, 0, index).value.clone(),
                    t.ty,
                    t.modifiers.clone(),
                )
            })
            .collect();
        use TokenModifier::*;
        assert_eq!(
            names,
            vec![
                (
                    "f".to_string(),
                    TokenType::Function,
                    vec![Declaration, Agentic]
                ),
                (
                    "g".to_string(),
                    TokenType::Variable,
                    vec![Documentation, Unresolved]
                ),
                (
                    "acceptanceCriteria".to_string(),
                    TokenType::Property,
                    vec![Context]
                ),
            ]
        );
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:267b17e8c1e5a951c9981534d31e87180bedced640f519c1fea34a6ebcb08ae2
        assert!(semantic_tokens_of(&workspace, "def/missing.lfy").is_empty());
    }

    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:b5097f3e8a9bd502be04b8014dabcf1f1547271685407ae1e2223de4a7075b1e
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:851770c083f006b02cc5d2a02798a436922a8ff2ae64f24665c3254b61353fa8
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:0175b9a1ad4d709d86c0dc488c03e3b10815b5b821fe6d42aaca5b37fcdd5d3c
    #[test]
    fn semantic_tokens_classify_a_type_parameter_wherever_its_name_appears() {
        let text = "d Box<T> { $item: `d` = T; } const b = Box<string>;";
        let fixture = Fixture::one(text);
        let workspace = fixture.load();
        let tokens = semantic_tokens_of(&workspace, A);
        let summary: Vec<(String, TokenType, Vec<TokenModifier>)> = tokens
            .iter()
            .map(|t| {
                let index = token_at(&workspace, A, t.range.start).unwrap();
                (
                    token(&workspace, 0, index).value.clone(),
                    t.ty,
                    t.modifiers.clone(),
                )
            })
            .collect();
        use TokenModifier::*;
        assert_eq!(
            summary,
            vec![
                (
                    "Box".to_string(),
                    TokenType::Data,
                    vec![Declaration, Agentic]
                ),
                ("T".to_string(), TokenType::TypeParameter, vec![Declaration]),
                ("item".to_string(), TokenType::Property, vec![Declaration]),
                ("T".to_string(), TokenType::TypeParameter, vec![]),
                (
                    "b".to_string(),
                    TokenType::Variable,
                    vec![Declaration, Readonly]
                ),
                ("Box".to_string(), TokenType::Data, vec![Agentic]),
            ]
        );
        // `string` is a keyword, so it gets no token.
        // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:851770c083f006b02cc5d2a02798a436922a8ff2ae64f24665c3254b61353fa8
        assert!(!summary.iter().any(|(raw, ..)| raw == "string"));
    }

    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:851770c083f006b02cc5d2a02798a436922a8ff2ae64f24665c3254b61353fa8
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:3d5de36e57170404015c3fd3283d61ce639c92d2761518d47c0cd218f5470c0d
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:fd59af79b69b792d0a46a4e76d8ebfedb0de64782a023593d40b457931d966b0
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:929a8a48590d4dfcb4256bca6989606d777cf6d9ee2e49acd1a6bfa1f110c5c3
    #[test]
    fn semantic_tokens_cover_the_repository_without_keywords() {
        let workspace = workspace::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")));
        let tokens = semantic_tokens_of(&workspace, "def/lexer/data.lfy");
        // The file declares a data and its members and names several types, so it has many
        // names; the exact count is a property of the definitions and is not pinned here.
        assert!(tokens.len() > 10, "{}", tokens.len());
        for t in &tokens {
            let index = token_at(&workspace, "def/lexer/data.lfy", t.range.start).unwrap();
            let file = workspace.model.file("def/lexer/data.lfy").unwrap();
            let raw = &token(&workspace, file, index).raw;
            assert!(
                raw.chars().all(|c| c.is_alphanumeric() || c == '_'),
                "{raw:?} is not a name"
            );
        }
        assert!(
            tokens
                .iter()
                .any(|t| t.ty == TokenType::Data
                    && t.modifiers.contains(&TokenModifier::Declaration))
        );
        assert!(
            tokens
                .iter()
                .any(|t| t.modifiers.contains(&TokenModifier::Documentation))
        );
        assert!(
            tokens
                .iter()
                .any(|t| t.modifiers.contains(&TokenModifier::Context))
        );
    }

    /// Every semantic token of a file as its name, its type, and its modifiers.
    fn summarize(
        workspace: &Workspace,
        file: &str,
    ) -> Vec<(String, TokenType, Vec<TokenModifier>)> {
        let id = workspace.model.file(file).unwrap();
        semantic_tokens_of(workspace, file)
            .iter()
            .map(|t| {
                let index = token_at(workspace, file, t.range.start).unwrap();
                (
                    token(workspace, id, index).value.clone(),
                    t.ty,
                    t.modifiers.clone(),
                )
            })
            .collect()
    }

    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:a984da220516d1eb9f619741ae9b54ac88167c3170597684aa511c4a78237250
    #[test]
    fn a_loop_variable_and_an_external_are_variables() {
        let text =
            "external E = undefined;\nfunction g() { for (const i in [1]) { const s = i; } }\n";
        let fixture = Fixture::one(text);
        let workspace = fixture.load();
        use TokenModifier::*;
        assert_eq!(
            summarize(&workspace, A),
            vec![
                ("E".to_string(), TokenType::Variable, vec![Declaration]),
                ("g".to_string(), TokenType::Function, vec![Declaration]),
                ("i".to_string(), TokenType::Variable, vec![Declaration]),
                (
                    "s".to_string(),
                    TokenType::Variable,
                    vec![Declaration, Readonly]
                ),
                ("i".to_string(), TokenType::Variable, vec![]),
            ]
        );
    }

    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:5a6ddde0f77237867aec3554925d122dfb6dc9d6b04bb03baeadc32bbe06532b
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:6ec488a19a8bed1c16f06d4a1611f4e4b0e65e6f2197c3f7fa8ea4c3380a02fe
    // @lfy def/query/main.lfy:semanticTokensOf#semanticTokensOf:semanticTokensOf:c597e906a0e9079e77628eb66872af16f638fc6b43ea00c501a100e1e353e39f
    #[test]
    fn a_module_a_value_accessor_and_an_alias_are_classified_by_what_they_reach() {
        let fixture = Fixture::new();
        fixture
            .write(
                "def/a.lfy",
                "use \"./b\" as B;\nd A { $x = string; }\nalias C = A;\nconst z = A.x;\nconst w = B.B;\n",
            )
            .write("def/b.lfy", "d B {}\n");
        let workspace = fixture.load();
        use TokenModifier::*;
        // The name a Use binds is a namespace; an alias takes the type of the entity it
        // is bound to; a name after a value accessor carries the value modifier, a
        // member of a data and a symbol of a module alike.
        assert_eq!(
            summarize(&workspace, A),
            vec![
                ("B".to_string(), TokenType::Namespace, vec![Declaration]),
                ("A".to_string(), TokenType::Data, vec![Declaration, Agentic]),
                ("x".to_string(), TokenType::Property, vec![Declaration]),
                (
                    "C".to_string(),
                    TokenType::Data,
                    vec![Declaration, Agentic]
                ),
                ("A".to_string(), TokenType::Data, vec![Agentic]),
                (
                    "z".to_string(),
                    TokenType::Variable,
                    vec![Declaration, Readonly]
                ),
                ("A".to_string(), TokenType::Data, vec![Agentic]),
                ("x".to_string(), TokenType::Property, vec![Value]),
                (
                    "w".to_string(),
                    TokenType::Variable,
                    vec![Declaration, Readonly]
                ),
                ("B".to_string(), TokenType::Namespace, vec![]),
                (
                    "B".to_string(),
                    TokenType::Data,
                    vec![Agentic, Value]
                ),
            ]
        );
    }
}
