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
// @lfy def/model/traits.lfy:10
// @lfy def/model/components.lfy:10
pub fn is_scoped(rule: Rule) -> bool {
    [
        F::SourceFile.entity(),               // @lfy def/model/components.lfy:10
        S::Block.entity(),                    // @lfy def/model/components.lfy:11
        S::DataDeclaration.entity(),          // @lfy def/model/components.lfy:12
        S::AgentFunctionDeclaration.entity(), // @lfy def/model/components.lfy:13
        S::FunctionDeclaration.entity(),      // @lfy def/model/components.lfy:14
        E::InlineFunction.entity(),           // @lfy def/model/components.lfy:15
        S::TraitDeclaration.entity(),         // @lfy def/model/components.lfy:16
        S::TypeDeclaration.entity(),          // @lfy def/model/components.lfy:17
        S::EnumDeclaration.entity(),          // @lfy def/model/components.lfy:18
        S::For.entity(),                      // @lfy def/model/components.lfy:19
        S::With.entity(),                     // @lfy def/model/components.lfy:20
    ]
    .contains(&rule)
}

/// The layer a rule carrying `reading` reads, for the accessor terminals.
// @lfy def/model/components.lfy:23
pub fn accessor_layer(rule: Rule) -> Option<Layer> {
    Some(match rule {
        r if r == P::ValueAccessor.entity() => Layer::Value, // @lfy def/model/components.lfy:23
        r if r == P::OptionalValueAccessor.entity() => Layer::Value, // @lfy def/model/components.lfy:24
        r if r == P::ContextAccessor.entity() => Layer::Context, // @lfy def/model/components.lfy:25
        r if r == P::ScopeAccessor.entity() => Layer::Scope, // @lfy def/model/components.lfy:26
        r if r == P::ParentScopeAccessor.entity() => Layer::Parent, // @lfy def/model/components.lfy:27
        _ => return None,
    })
}

/// The layer a rule carrying `reading` reads, for the expression rules.
// @lfy def/model/traits.lfy:16
// @lfy def/model/components.lfy:28
pub fn reading_layer(rule: Rule) -> Option<Layer> {
    Some(match rule {
        r if r == E::Name.entity() => Layer::Value,             // @lfy def/model/components.lfy:28
        r if r == E::TraitUse.entity() => Layer::Value,         // @lfy def/model/components.lfy:29
        r if r == E::Dereference.entity() => Layer::Dereference, // @lfy def/model/components.lfy:30
        r if r == E::Previous.entity() => Layer::Previous,      // @lfy def/model/components.lfy:31
        _ => return None,
    })
}

/// Which rule spells a declared name, and what kind of symbol it makes.
// @lfy def/model/components.lfy:37
pub fn declaring(rule: Rule) -> Option<(Rule, SymbolKind)> {
    Some(match rule {
        r if r == S::DataDeclaration.entity() => (I::Identifier.entity(), SymbolKind::Data), // @lfy def/model/components.lfy:38
        r if r == S::TraitDeclaration.entity() => (I::Identifier.entity(), SymbolKind::Trait), // @lfy def/model/components.lfy:39
        r if r == S::FunctionDeclaration.entity() => (E::Signature.entity(), SymbolKind::Function), // @lfy def/model/components.lfy:40
        r if r == S::AgentFunctionDeclaration.entity() => (E::Signature.entity(), SymbolKind::AgentFunction), // @lfy def/model/components.lfy:41
        r if r == S::TypeDeclaration.entity() => (E::Declared.entity(), SymbolKind::Type), // @lfy def/model/components.lfy:42
        r if r == S::EnumDeclaration.entity() => (E::Declared.entity(), SymbolKind::Enum), // @lfy def/model/components.lfy:43
        r if r == S::VariableDeclaration.entity() => (E::Declared.entity(), SymbolKind::Variable), // @lfy def/model/components.lfy:44
        r if r == S::AliasDeclaration.entity() => (E::Declared.entity(), SymbolKind::Alias), // @lfy def/model/components.lfy:45
        r if r == S::ExternalDeclaration.entity() => (E::Declared.entity(), SymbolKind::External), // @lfy def/model/components.lfy:46
        r if r == S::Use.entity() => (I::Identifier.entity(), SymbolKind::Module), // @lfy def/model/components.lfy:47
        r if r == S::For.entity() => (E::Declared.entity(), SymbolKind::LoopVariable), // @lfy def/model/components.lfy:48
        r if r == S::ForFrom.entity() => (E::Declared.entity(), SymbolKind::LoopVariable), // @lfy def/model/components.lfy:49
        r if r == E::Parameter.entity() => (E::Name.entity(), SymbolKind::Parameter), // @lfy def/model/components.lfy:50
        r if r == E::SpreadParameter.entity() => (E::Name.entity(), SymbolKind::Parameter), // @lfy def/model/components.lfy:51
        r if r == E::TypeKey.entity() => (E::Name.entity(), SymbolKind::Member), // @lfy def/model/components.lfy:52
        _ => return None,
    })
}

/// The rules a holder search never enters: `Parameters`, `Block`, `Object`, and `Type`.
// @lfy def/model/traits.lfy:40
pub fn is_holder_barrier(rule: Rule) -> bool {
    rule == E::Parameters.entity() || rule == S::Block.entity() || rule == E::Object.entity() || rule == E::Type.entity()
}
