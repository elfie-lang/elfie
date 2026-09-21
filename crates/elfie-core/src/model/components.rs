//! Compiled from `def/model/components.lfy`: which grammar rules own scopes, read layers,
//! apply traits, and declare names.

use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::file::File as F;
use crate::grammar::rules::statement::Statement as S;
use crate::grammar::terminals::identifier::Identifier as I;
use crate::grammar::terminals::punctuation::Punctuation as P;
use crate::grammar::{Entity as Rule, GrammarRule};

use super::data::{Layer, SymbolKind};

/// The rules carrying `scoped`: each owns a scope.
// @lfy def/model/traits.lfy:scoped
pub fn is_scoped(rule: Rule) -> bool {
    [
        F::SourceFile.entity(),               // @lfy def/grammar/rules/file.lfy:SourceFile
        S::Block.entity(),                    // @lfy def/grammar/rules/statement.lfy:Block
        S::DataDeclaration.entity(),          // @lfy def/grammar/rules/statement.lfy:DataDeclaration
        S::AgentFunctionDeclaration.entity(), // @lfy def/grammar/rules/statement.lfy:AgentFunctionDeclaration
        S::FunctionDeclaration.entity(),      // @lfy def/grammar/rules/statement.lfy:FunctionDeclaration
        E::InlineFunction.entity(),           // @lfy def/grammar/rules/expression.lfy:InlineFunction
        E::FunctionType.entity(),             // @lfy def/grammar/rules/expression.lfy:FunctionType
        S::TraitDeclaration.entity(),         // @lfy def/grammar/rules/statement.lfy:TraitDeclaration
        S::TypeDeclaration.entity(),          // @lfy def/grammar/rules/statement.lfy:TypeDeclaration
        S::EnumDeclaration.entity(),          // @lfy def/grammar/rules/statement.lfy:EnumDeclaration
        S::For.entity(),                      // @lfy def/grammar/rules/statement.lfy:For
        S::With.entity(),                     // @lfy def/grammar/rules/statement.lfy:With
    ]
    .contains(&rule)
}

/// The layer a rule carrying `reading` reads, for the accessor terminals.
// @lfy def/model/traits.lfy:reading
pub fn accessor_layer(rule: Rule) -> Option<Layer> {
    Some(match rule {
        // @lfy def/grammar/terminals/punctuation.lfy:ValueAccessor
        r if r == P::ValueAccessor.entity() => Layer::Value,
        // @lfy def/grammar/terminals/punctuation.lfy:OptionalValueAccessor
        r if r == P::OptionalValueAccessor.entity() => Layer::Value,
        // @lfy def/grammar/terminals/punctuation.lfy:ContextAccessor
        r if r == P::ContextAccessor.entity() => Layer::Context,
        // @lfy def/grammar/terminals/punctuation.lfy:ScopeAccessor
        r if r == P::ScopeAccessor.entity() => Layer::Scope,
        // @lfy def/grammar/terminals/punctuation.lfy:ParentScopeAccessor
        r if r == P::ParentScopeAccessor.entity() => Layer::Parent,
        _ => return None,
    })
}

/// The layer a rule carrying `reading` reads, for the expression rules.
// @lfy def/model/traits.lfy:reading
pub fn reading_layer(rule: Rule) -> Option<Layer> {
    Some(match rule {
        // @lfy def/grammar/rules/expression.lfy:Name
        r if r == E::Name.entity() => Layer::Value,
        // @lfy def/grammar/rules/expression.lfy:TraitUse
        r if r == E::TraitUse.entity() => Layer::Value,
        // @lfy def/grammar/rules/expression.lfy:Dereference
        r if r == E::Dereference.entity() => Layer::Dereference,
        // @lfy def/grammar/rules/expression.lfy:Previous
        r if r == E::Previous.entity() => Layer::Previous,
        _ => return None,
    })
}

/// Which rule spells a declared name, and what kind of symbol it makes.
// @lfy def/model/traits.lfy:declaring
pub fn declaring(rule: Rule) -> Option<(Rule, SymbolKind)> {
    Some(match rule {
        // @lfy def/grammar/rules/statement.lfy:DataDeclaration
        r if r == S::DataDeclaration.entity() => (I::Identifier.entity(), SymbolKind::Data),
        // @lfy def/grammar/rules/statement.lfy:TraitDeclaration
        r if r == S::TraitDeclaration.entity() => (I::Identifier.entity(), SymbolKind::Trait),
        // @lfy def/grammar/rules/statement.lfy:FunctionDeclaration
        r if r == S::FunctionDeclaration.entity() => (E::Signature.entity(), SymbolKind::Function),
        // @lfy def/grammar/rules/statement.lfy:AgentFunctionDeclaration
        r if r == S::AgentFunctionDeclaration.entity() => {
            (E::Signature.entity(), SymbolKind::AgentFunction)
        }
        // A type declaration spells its name with an Identifier of its own, so that the
        // type parameters that may follow it are not mistaken for the name.
        // @lfy def/grammar/rules/statement.lfy:TypeDeclaration
        r if r == S::TypeDeclaration.entity() => (I::Identifier.entity(), SymbolKind::Type),
        // @lfy def/grammar/rules/statement.lfy:EnumDeclaration
        r if r == S::EnumDeclaration.entity() => (E::Declared.entity(), SymbolKind::Enum),
        // @lfy def/grammar/rules/statement.lfy:VariableDeclaration
        r if r == S::VariableDeclaration.entity() => (E::Declared.entity(), SymbolKind::Variable),
        // @lfy def/grammar/rules/statement.lfy:AliasDeclaration
        r if r == S::AliasDeclaration.entity() => (E::Declared.entity(), SymbolKind::Alias),
        // @lfy def/grammar/rules/statement.lfy:ExternalDeclaration
        r if r == S::ExternalDeclaration.entity() => (E::Declared.entity(), SymbolKind::External),
        // @lfy def/grammar/rules/statement.lfy:Use
        r if r == S::Use.entity() => (I::Identifier.entity(), SymbolKind::Module),
        // @lfy def/grammar/rules/statement.lfy:For
        r if r == S::For.entity() => (E::Declared.entity(), SymbolKind::LoopVariable),
        // @lfy def/grammar/rules/statement.lfy:ForFrom
        r if r == S::ForFrom.entity() => (E::Declared.entity(), SymbolKind::LoopVariable),
        // @lfy def/grammar/rules/expression.lfy:Parameter
        r if r == E::Parameter.entity() => (E::Name.entity(), SymbolKind::Parameter),
        // @lfy def/grammar/rules/expression.lfy:SpreadParameter
        r if r == E::SpreadParameter.entity() => (E::Name.entity(), SymbolKind::Parameter),
        // @lfy def/grammar/rules/expression.lfy:TypeParameter
        r if r == E::TypeParameter.entity() => (I::Identifier.entity(), SymbolKind::TypeParameter),
        // @lfy def/grammar/rules/expression.lfy:TypeKey
        r if r == E::TypeKey.entity() => (E::Name.entity(), SymbolKind::Member),
        _ => return None,
    })
}

/// The rules a holder search never enters: `Parameters`, `Block`, `Object`, and `Type`.
// @lfy def/model/traits.lfy:declaring
pub fn is_holder_barrier(rule: Rule) -> bool {
    rule == E::Parameters.entity() || rule == S::Block.entity() || rule == E::Object.entity() || rule == E::Type.entity()
}
