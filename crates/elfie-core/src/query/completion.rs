// @lfy def/query/main.lfy:completionsAt
//! Where a completion is asked, and what could be typed there.

use std::collections::HashSet;
use std::fmt;

use super::*;
use crate::grammar::GrammarRule;
use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::{STATEMENTS, Statement as S};
use crate::grammar::terminals::comment::Comment as C;
use crate::grammar::terminals::identifier::Identifier as I;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::grammar::terminals::literal::Literal as L;
use crate::grammar::terminals::punctuation::Punctuation as P;
use crate::model::{ContextProperty, ScopeId, TypeRef, Value};
use crate::parser::components::is_trivia;

/// The extension of a source file, dropped from path completions.
const EXTENSION: &str = ".lfy";

/// Where a token starts, as [`Position::of_token`] gives it.
fn start_of(token: &Token) -> Position {
    Position::of_token(token)
}

/// The position just after the last character of a token's raw text, as
/// [`Position::after_token`] gives it.
fn end_of(token: &Token) -> Position {
    Position::after_token(token)
}

/// Whether a token is trivia: space, a line break, or one that no terminal matched.
fn is_trivia_token(token: &Token) -> bool {
    token.raw.is_empty() || token.rule.is_none_or(is_trivia)
}

/// The data the prelude gives an entity for what it is: `Entity`, which every entity is
/// seen through, then the data for its own kind. An entity that resolved to nothing is
/// still seen through `Entity`.
// @lfy def/query/main.lfy:completionsAt
fn kind_data(model: &Model, entity: Option<EntityId>) -> Vec<EntityId> {
    let own = match entity.map(|entity| &model.entities[entity].kind) {
        Some(EntityKind::Trait { .. }) => Some("Trait"),
        Some(EntityKind::Fn { .. }) => Some("Function"),
        Some(EntityKind::Data) => Some("Data"),
        Some(EntityKind::Type) => Some("Type"),
        Some(EntityKind::Enum) => Some("Enum"),
        Some(EntityKind::Member) => Some("Member"),
        Some(EntityKind::Parameter) => Some("Parameter"),
        Some(EntityKind::Variable) => Some("Variable"),
        Some(EntityKind::Module | EntityKind::File) => Some("Module"),
        _ => None,
    };
    prelude_data(model, "Entity")
        .into_iter()
        .chain(own.and_then(|name| prelude_data(model, name)))
        .collect()
}

/// The members of the kind data of an entity: those of `Entity` for every entity, then
/// those of the data for its kind, each name once.
// @lfy def/query/main.lfy:completionsAt
fn kind_members(model: &Model, entity: Option<EntityId>) -> Vec<SymbolId> {
    let mut out: Vec<SymbolId> = Vec::new();
    for data in kind_data(model, entity) {
        for symbol in member_symbols(model, data) {
            let name = &model.symbols[symbol].name;
            if !out.iter().any(|&seen| &model.symbols[seen].name == name) {
                out.push(symbol);
            }
        }
    }
    out
}

/// The members of an entity's kind data whose value is a function: what the value layer
/// of an entity adds to its own members.
// @lfy def/query/main.lfy:completionsAt
fn kind_function_members(model: &Model, entity: EntityId) -> Vec<SymbolId> {
    kind_members(model, Some(entity))
        .into_iter()
        .filter(|&symbol| {
            matches!(
                model.entities[model.symbols[symbol].entity].ty,
                Some(TypeRef::Function)
            )
        })
        .collect()
}

/// The data the prelude declares for a value of a type: what that value is. A template
/// already has its data as its type and needs no entry.
// @lfy def/query/main.lfy:completionsAt
fn base_data(model: &Model, ty: &TypeRef) -> Option<EntityId> {
    let name = match ty {
        TypeRef::Primitive("string") => "String",
        TypeRef::Primitive("number") => "Number",
        TypeRef::Primitive("boolean") => "Boolean",
        TypeRef::Primitive("object") => "Object",
        TypeRef::Primitive("function") | TypeRef::Function => "Function",
        TypeRef::List(_) => "List",
        TypeRef::Literal(value) => match **value {
            Value::String(_) => "String",
            Value::Number(_) => "Number",
            Value::Bool(_) => "Boolean",
            _ => return None,
        },
        _ => return None,
    };
    prelude_data(model, name)
}

/// Where a completion is asked.
// @lfy def/query/main.lfy:CompletionContext
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionContext {
    UsePath,     // @lfy def/query/main.lfy:CompletionContext.usePath
    Context,     // @lfy def/query/main.lfy:CompletionContext.context
    Scope,       // @lfy def/query/main.lfy:CompletionContext.scope
    ParentScope, // @lfy def/query/main.lfy:CompletionContext.parentScope
    Value,       // @lfy def/query/main.lfy:CompletionContext.value
    TraitName,   // @lfy def/query/main.lfy:CompletionContext.traitName
    Prose,       // @lfy def/query/main.lfy:CompletionContext.prose
    TypeName,    // @lfy def/query/main.lfy:CompletionContext.typeName
    Statement,   // @lfy def/query/main.lfy:CompletionContext.statement
    Expression,  // @lfy def/query/main.lfy:CompletionContext.expression
}

impl CompletionContext {
    /// Every context in declaration order, which is the order [`completions_at`] tries
    /// them in.
    // @lfy def/query/main.lfy:CompletionContext
    pub const ALL: [CompletionContext; 10] = [
        CompletionContext::UsePath,
        CompletionContext::Context,
        CompletionContext::Scope,
        CompletionContext::ParentScope,
        CompletionContext::Value,
        CompletionContext::TraitName,
        CompletionContext::Prose,
        CompletionContext::TypeName,
        CompletionContext::Statement,
        CompletionContext::Expression,
    ];

    /// The value of the enum member: what makes the context hold at a position. "The
    /// nearest token before the position" is always read with the identifier characters
    /// typed before the position set aside, and never counts trivia.
    // @lfy def/query/main.lfy:CompletionContext
    pub fn value(self) -> &'static str {
        match self {
            CompletionContext::UsePath => "The position is inside the string of a Use",
            CompletionContext::Context => {
                "The nearest token before the position is a ContextAccessor"
            }
            CompletionContext::Scope => "The nearest token before the position is a ScopeAccessor",
            CompletionContext::ParentScope => {
                "The nearest token before the position is a ParentScopeAccessor"
            }
            CompletionContext::Value => {
                "The nearest token before the position is a ValueAccessor or an \
                 OptionalValueAccessor"
            }
            CompletionContext::TraitName => {
                "The nearest token before the position is an IsKeyword, the ExtendsKeyword of an \
                 ExtendsClause, or a Comma inside TraitUses"
            }
            CompletionContext::Prose => {
                "The position is inside a TemplateReference or Documentation"
            }
            CompletionContext::TypeName => {
                "The nearest token before the position is the Colon of a DefinitionClause or \
                 Definition, the LessThan or a Comma of TypeArguments or of Generic, or the \
                 ExtendsKeyword or PlainSetter of a TypeParameter"
            }
            CompletionContext::Statement => "A Statement can begin at the position",
            CompletionContext::Expression => "An Expression can begin at the position",
        }
    }
}

impl fmt::Display for CompletionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
    }
}

/// The text of a token before a position inside it (all of it when the position is at
/// or past its end).
fn text_before(token: &Token, position: Position) -> String {
    let mut line = token.line;
    let mut column = token.column;
    let mut out = String::new();
    for c in token.raw.chars() {
        if Position::new(line, column) >= position {
            break;
        }
        out.push(c);
        if c == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    out
}

/// The identifier characters at the end of a text.
fn identifier_tail(text: &str) -> String {
    let tail: String = text
        .chars()
        .rev()
        .take_while(|&c| c == '_' || unicode_ident::is_xid_continue(c))
        .collect();
    tail.chars().rev().collect()
}

/// Every symbol visible from a scope: the nearer scope's first, each scope's own symbols
/// then its imports, then `global`; one per name, the nearest winning.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:beddcfda7878c84398489ed04b9100d3ca66a779785f71339a12a7aac4ff27d0
fn visible_symbols(model: &Model, scope: ScopeId) -> Vec<SymbolId> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    let mut current = Some(scope);
    while let Some(id) = current {
        let scope = &model.scopes[id];
        for &symbol in scope.symbols.iter().chain(scope.imports.iter()) {
            if seen.insert(model.symbols[symbol].name.as_str()) {
                out.push(symbol);
            }
        }
        current = scope.parent;
    }
    if let Some(global) = model.entities[model.global].symbol
        && seen.insert(model.symbols[global].name.as_str())
    {
        out.push(global);
    }
    out
}

/// A completion for a declared symbol.
fn symbol_completion(model: &Model, symbol: SymbolId) -> Completion {
    let symbol = &model.symbols[symbol];
    Completion {
        label: symbol.name.clone(),
        kind: OfferedKind::Symbol(symbol.kind),
        detail: model.entities[symbol.entity]
            .definition
            .as_deref()
            .map(model::strip_references),
    }
}

/// A keyword completion.
// @lfy def/query/main.lfy:completionsAt
fn keyword_completion(text: &str) -> Completion {
    Completion {
        label: text.to_string(),
        kind: OfferedKind::Completion(CompletionKind::Keyword),
        detail: None,
    }
}

/// The keywords named by `[[...]]` references in a piece of EBNF syntax, in order.
fn keywords_in(syntax: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut rest = syntax;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        if let Some(Rule::Keyword(keyword)) = Rule::lookup(&after[..close])
            && let Some(word) = keyword.word()
        {
            out.push(word);
        }
        rest = &after[close + 2..];
    }
    out
}

/// The syntax of the first element of a rule: up to the first comma outside parentheses.
fn first_element(syntax: &str) -> &str {
    let mut depth = 0usize;
    for (index, c) in syntax.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return &syntax[..index],
            _ => {}
        }
    }
    syntax
}

/// The keyword that begins each statement rule, in the order of the rules.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:51adc1b9fc9e926d06595d76e546949f392045d3296b9a6bb7d093f6a3be644e
fn statement_keywords() -> Vec<&'static str> {
    let mut out = Vec::new();
    for rule in STATEMENTS {
        for word in keywords_in(first_element(rule.syntax())) {
            if !out.contains(&word) {
                out.push(word);
            }
        }
    }
    out
}

/// The primitive type keywords: what `PrimitiveType` lists.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:7b07fa8a1db2032a1eb89bb408148c4ec84ba69ff8899993bea98d722bd80963
fn primitive_type_keywords() -> Vec<&'static str> {
    keywords_in(E::PrimitiveType.syntax())
}

/// The value keywords: true, false, null, and undefined.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:8f8f5282e676ec6a8cb4fd951ff172f975a3732075c4f24ce7253b425558cdb1
fn value_keywords() -> Vec<&'static str> {
    let mut out = keywords_in(E::Boolean.syntax());
    out.extend(keywords_in(E::Nullish.syntax()));
    out
}

/// What an entity offers after a value accessor: its own members, then the members of
/// its kind data whose value is a function.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:c8583c31ea5802803ab8c989f680381b5f19150f120c852fc637551bf5dea079
fn entity_members(model: &Model, entity: EntityId) -> Vec<SymbolId> {
    let mut out = member_symbols(model, entity);
    out.extend(kind_function_members(model, entity));
    out
}

/// What a value offers after a value accessor: the entity's own members, then every
/// member of the base data of its type, which is `String`, `Number`, or `List` for a
/// string, a number, or a list.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:b004a292d579f95ff0e64d5683c2c6d17367d72f08608a1525f516432a7045b6
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:823bbfa9c16889c1e7b52d517feb017ab3e59d11cbe8577cbad6361e38d8d095
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:1c542a5607a0f3e65d2dcd0ac6ce7bb52906094a0243e6ee2400137b80d8a462
fn value_members(model: &Model, entity: EntityId) -> Vec<SymbolId> {
    let mut out = member_symbols(model, entity);
    if let Some(ty) = &model.entities[entity].ty
        && let Some(more) = type_members(model, ty)
    {
        out.extend(more);
    }
    out
}

/// What a type offers after a value accessor.
fn type_members(model: &Model, ty: &TypeRef) -> Option<Vec<SymbolId>> {
    match ty {
        TypeRef::Entity(entity) => match model.entities[*entity].kind {
            EntityKind::File => Some(module_symbols(model, model.entities[*entity].file?)),
            _ => Some(entity_members(model, *entity)),
        },
        TypeRef::Predicate(entity) => Some(entity_members(model, *entity)),
        TypeRef::Union(items) => items.iter().find_map(|item| type_members(model, item)),
        // A value of a base data offers that data's members.
        // @lfy def/query/main.lfy:completionsAt
        ty => base_data(model, ty).map(|data| member_symbols(model, data)),
    }
}

/// The entity the left side of an accessor resolves to; `None` when it resolves to
/// nothing.
// @lfy def/query/main.lfy:completionsAt
fn left_entity(model: &Model, left: NodeRef) -> Option<EntityId> {
    let rule = model.info(left).rule;
    if rule == E::Group.entity() {
        let inner = child_nodes(model, left).into_iter().next()?;
        return left_entity(model, inner);
    }
    let symbol = model
        .usage_of(left)
        .and_then(|usage| model.usages[usage].symbol)?;
    Some(model.symbols[symbol].entity)
}

/// What the left side of a member access offers: a module's symbols; an entity's own
/// members, then the members of its kind data whose value is a function; an enum's keys;
/// or, for a value that is an object, a string, a number, or a list, the entity's own
/// members then every member of `Object`, `String`, `Number`, or `List`. `None` when the
/// left side resolves to nothing.
// @lfy def/query/main.lfy:completionsAt
fn left_members(model: &Model, left: NodeRef) -> Option<Vec<SymbolId>> {
    let rule = model.info(left).rule;
    if rule == E::Group.entity() {
        let inner = child_nodes(model, left).into_iter().next()?;
        return left_members(model, inner);
    }
    let symbol = model
        .usage_of(left)
        .and_then(|usage| model.usages[usage].symbol)?;
    let symbol = &model.symbols[symbol];
    let entity = &model.entities[symbol.entity];
    match symbol.kind {
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:e27a530ad71011bb541a5761da43e628b4d40ce9dd245a5e2f3708a75532b763
        SymbolKind::Module => match entity.kind {
            EntityKind::File => Some(module_symbols(model, entity.file?)),
            _ => None,
        },
        SymbolKind::Data
        | SymbolKind::Trait
        | SymbolKind::Type
        | SymbolKind::Function
        | SymbolKind::AgentFunction => Some(entity_members(model, symbol.entity)),
        // An enum offers its keys alone: not the members of its kind data.
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:c4fce9642c5615b339d7beba622badd73fc8bcf97e3c5688012f4db54d818b20
        SymbolKind::Enum => Some(member_symbols(model, symbol.entity)),
        SymbolKind::Alias | SymbolKind::External => match entity.kind {
            EntityKind::Enum => Some(member_symbols(model, symbol.entity)),
            EntityKind::Data | EntityKind::Trait { .. } | EntityKind::Type => {
                Some(entity_members(model, symbol.entity))
            }
            EntityKind::File => Some(module_symbols(model, entity.file?)),
            _ => Some(value_members(model, symbol.entity)),
        },
        _ => Some(value_members(model, symbol.entity)),
    }
}

/// The keys an object literal is written with, in the order they are written: the name
/// each `ObjectKey` declares. `None` when the entity's value is not an object literal.
// Decision: the keys of an object declare no symbol, so they are read from the tokens the
// declaration covers rather than from the model.
// @lfy def/query/main.lfy:completionsAt
fn object_keys(model: &Model, entity: EntityId) -> Option<Vec<String>> {
    let e = &model.entities[entity];
    if e.ty != Some(TypeRef::Primitive("object")) {
        return None;
    }
    let node = e.node.filter(|&node| is_real(model, node))?;
    // The value of a declaration is its first child that is not the name it declares, as
    // the binder reads it to infer the type.
    let value = child_nodes(model, node)
        .into_iter()
        .find(|&child| model.info(child).rule != E::Declared.entity())?;
    if model.info(value).rule != E::Object.entity() {
        return None;
    }
    let tokens = &model.sources[value.file].tree.tokens;
    Some(
        child_nodes(model, value)
            .into_iter()
            .filter(|&key| model.info(key).rule == E::ObjectKey.entity())
            .filter_map(|key| {
                let declared = child_nodes(model, key)
                    .into_iter()
                    .find(|&child| model.info(child).rule == E::Declared.entity())?;
                let info = model.info(declared);
                tokens[info.start..info.end]
                    .iter()
                    .find(|token| token.is(I::Identifier))
                    .map(|token| token.value.clone())
            })
            .collect(),
    )
}

/// What the left side of a member access offers, as completions: an object's own keys,
/// then the members of `Object`; what [`left_members`] gives for anything else. `None`
/// when the left side resolves to nothing.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:c090000e66157db31b578b3016839e6d86b5bf47a61c842e9482255a61175b4f
fn left_completions(model: &Model, left: NodeRef) -> Option<Vec<Completion>> {
    if let Some(entity) = left_entity(model, left)
        && let Some(keys) = object_keys(model, entity)
    {
        let mut out: Vec<Completion> = keys
            .into_iter()
            .map(|label| Completion {
                label,
                kind: OfferedKind::Symbol(SymbolKind::Member),
                detail: None,
            })
            .collect();
        out.extend(
            value_members(model, entity)
                .into_iter()
                .map(|symbol| symbol_completion(model, symbol)),
        );
        return Some(out);
    }
    Some(
        left_members(model, left)?
            .into_iter()
            .map(|symbol| symbol_completion(model, symbol))
            .collect(),
    )
}

/// The directory holding a file, relative to the root; empty for a file at the root.
fn directory_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

/// A path with every `.` dropped and every `..` applied to the segment before it.
fn normalize(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => match segments.last() {
                Some(&last) if last != ".." => {
                    segments.pop();
                }
                _ => segments.push(".."),
            },
            segment => segments.push(segment),
        }
    }
    segments.join("/")
}

/// The directories and the `.lfy` files without their extension under a directory of the
/// project, sorted, each once.
// @lfy def/query/main.lfy:completionsAt
fn entries_under(workspace: &Workspace, directory: &str) -> Vec<Completion> {
    let disk = if directory.is_empty() {
        workspace.root.clone()
    } else {
        workspace.root.join(directory)
    };
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&disk) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() {
                names.push(name);
            } else if let Some(stem) = name.strip_suffix(EXTENSION) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|label| Completion {
            label,
            kind: OfferedKind::Completion(CompletionKind::Path),
            detail: None,
        })
        .collect()
}

/// Completions inside the string of a `Use`, with the prefix already typed: for a path
/// beginning with a dot, the entries under the directory the path names so far, relative
/// to the file's directory; otherwise each package's identifier, then entries under its
/// root the same way.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:7065c2f898fbfc8f0764635d4eda8bad0129e35dff4dbf77f81b25c2138162e5
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:00d112d094be9e9a2cef44f7a8962bbc76ca48587d3775070df6672096caab27
fn path_completions(workspace: &Workspace, file: &str, typed: &str) -> (Vec<Completion>, String) {
    let (directory, prefix) = match typed.rfind('/') {
        Some(slash) => (&typed[..slash], &typed[slash + 1..]),
        None => ("", typed),
    };
    let completions = if typed.starts_with('.') {
        let base = normalize(&format!("{}/{directory}", directory_of(file)));
        entries_under(workspace, &base)
    } else if typed.contains('/') {
        let (package, rest) = directory.split_once('/').unwrap_or((directory, ""));
        match workspace
            .packages
            .iter()
            .find(|candidate| candidate.identifier == package)
        {
            Some(package) => {
                entries_under(workspace, &normalize(&format!("{}/{rest}", package.root)))
            }
            None => Vec::new(),
        }
    } else {
        workspace
            .packages
            .iter()
            .map(|package| Completion {
                label: package.identifier.clone(),
                kind: OfferedKind::Completion(CompletionKind::Path),
                detail: None,
            })
            .collect()
    };
    (completions, prefix.to_string())
}

/// Whether a statement can begin after a token: at the start of a file or block, after a
/// statement ends, or after a keyword that prefixes a statement.
// Decision: the definition names the situation only; these are the tokens the grammar
// lets a statement follow.
fn statement_begins_after(before: Option<&Token>) -> bool {
    let Some(before) = before else {
        return true;
    };
    before.is(P::Semicolon)
        || before.is(P::BlockOpen)
        || before.is(P::BlockClose)
        || before.is(K::AsyncKeyword)
        || before.is(K::AceKeyword)
        || before.is(K::ElseKeyword)
}

/// Whether an expression can begin after a token: after an operator, an opener, a
/// separator, a setter, or a keyword that takes an expression.
// Decision: the definition names the situation only; an expression follows any
// punctuation that is not a closer, an accessor, or a statement boundary, and the
// keywords that take an expression.
fn expression_begins_after(before: &Token) -> bool {
    let Some(rule) = before.rule else {
        return false;
    };
    if rule.is_punctuation() {
        return !matches!(
            rule,
            Rule::Punctuation(
                P::GroupClose
                    | P::ListClose
                    | P::BlockClose
                    | P::BlockOpen
                    | P::Semicolon
                    | P::ValueAccessor
                    | P::OptionalValueAccessor
                    | P::ContextAccessor
                    | P::ScopeAccessor
                    | P::ParentScopeAccessor
            )
        );
    }
    matches!(
        rule,
        Rule::Keyword(
            K::ReturnKeyword
                | K::AwaitKeyword
                | K::InKeyword
                | K::OfKeyword
                | K::FromKeyword
                | K::WhileKeyword
                | K::IfKeyword
                | K::MatchKeyword
                | K::MatchallKeyword
                | K::WithKeyword
                | K::WhereKeyword
                | K::AndKeyword
                | K::OrKeyword
        )
    )
}

/// What could be typed at a position.
///
/// The context is decided by the nearest token before the position that is not trivia,
/// and by the innermost node there. Every completion appears once, filtered to labels
/// that begin with the identifier characters already typed, case sensitive; those from a
/// nearer scope come before those from an enclosing scope, then file order, then
/// keywords.
///
/// After `@`: one completion of kind `context` per member of the kind data of the entity
/// the left side resolves to, those of `Entity` for every entity then those of the data
/// for its kind, each with the member's definition as its detail. After `$`: the
/// members of the current entity of the scope holding the position (of the left side,
/// when there is one). After `$&`: the members of the parent scope's current entity.
/// After `.` or `?.`: what the left side offers, or nothing when it resolves to nothing.
/// After `is`, the `extends` of an extends clause, or a comma in trait uses: every trait
/// visible plus the module symbols. Inside the string of a `Use`: paths. Inside a template
/// reference or documentation: every name visible, then every rule entity. Where a type
/// is written — after the colon of a definition clause, after the `<` or a comma of type
/// arguments, and after the `extends` or `=` of a type parameter — every data, type,
/// enum, trait, or type parameter visible, then the primitive type keywords. Where a
/// statement can begin: every name visible, then the keyword that begins each statement.
/// Where an expression can begin: every name visible, then the value keywords, then the
/// primitive type keywords.
// @lfy def/query/main.lfy:completionsAt
pub fn completions_at(workspace: &Workspace, file: &str, position: Position) -> Vec<Completion> {
    let model = &workspace.model;
    let Some(id) = file_of(workspace, file) else {
        return Vec::new();
    };
    let tokens = tokens(workspace, id);

    // The token the position is inside or just after, and the identifier characters
    // typed before the position. @lfy def/query/main.lfy:completionsAt
    let at = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| !token.raw.is_empty())
        .find(|(_, token)| start_of(token) < position && position <= end_of(token))
        .map(|(index, _)| index);
    let typed = at.map(|index| text_before(&tokens[index], position));
    let mut prefix = typed.as_deref().map(identifier_tail).unwrap_or_default();
    let prefix_start = Position::new(position.line, position.column - prefix.chars().count());

    // The nearest token before the typed characters that is not trivia.
    // @lfy def/query/main.lfy:completionsAt
    let before = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| !is_trivia_token(token) && end_of(token) <= prefix_start)
        .map(|(index, _)| index)
        .next_back();
    let anchor = at.or(before);
    let chain = match anchor {
        Some(index) => nodes_covering(model, id, index),
        None => vec![NodeRef { file: id, index: 0 }],
    };
    let innermost = *chain.last().unwrap_or(&NodeRef { file: id, index: 0 });
    let scope = model.enclosing_scope(innermost);
    let before_token = before.map(|index| &tokens[index]);
    let before_rule = before_token.and_then(|token| token.rule);

    let mut out: Vec<Completion> = Vec::new();

    // Inside the string of a Use. @lfy def/query/main.lfy:completionsAt
    let in_use_string = find_rule(model, &chain, S::Use.entity()).is_some()
        && find_rule(model, &chain, E::StringLiteral.entity()).is_some()
        && at.is_some_and(|index| {
            let token = &tokens[index];
            token.is(L::DoubleQuoteBody)
                || token.is(L::SingleQuoteBody)
                || ((token.is(L::DoubleQuote) || token.is(L::SingleQuote))
                    && tokens
                        .get(index + 1)
                        .is_some_and(|next| !next.is(P::Semicolon) && !is_trivia_token(next))
                    && tokens.get(index.wrapping_sub(1)).is_none_or(|previous| {
                        !previous.is(L::DoubleQuoteBody) && !previous.is(L::SingleQuoteBody)
                    }))
        });

    let members_of = |entity: EntityId| -> Vec<Completion> {
        member_symbols(model, entity)
            .into_iter()
            .map(|symbol| symbol_completion(model, symbol))
            .collect()
    };
    let names = |out: &mut Vec<Completion>| {
        out.extend(
            visible_symbols(model, scope)
                .into_iter()
                .map(|symbol| symbol_completion(model, symbol)),
        );
    };
    // The left side of the accessor at `before`, when it is the operator of a Member.
    let left_of_accessor = || -> Option<NodeRef> {
        let index = before?;
        let holder = *nodes_covering(model, id, index).last()?;
        if model.info(holder).rule != E::Member.entity() {
            return None;
        }
        child_nodes(model, holder).into_iter().next()
    };
    // The innermost node holding a token.
    let holder_of = |index: usize| -> Option<Rule> {
        nodes_covering(model, id, index)
            .last()
            .map(|&node| model.info(node).rule)
    };
    // The index of the nearest token before another that is not trivia.
    let previous_of = |index: usize| -> Option<usize> {
        tokens[..index]
            .iter()
            .rposition(|token| !is_trivia_token(token))
    };
    // Whether a token is written directly against the token before it, with nothing
    // between them.
    let against_previous = |index: usize| -> bool {
        previous_of(index).is_some_and(|previous| {
            is_name_token(&tokens[previous]) && end_of(&tokens[previous]) == start_of(&tokens[index])
        })
    };
    // Whether a token spells the name of a declaration of this file.
    // @lfy def/query/main.lfy:completionsAt
    let declares_a_name = |index: usize| -> bool {
        model
            .symbols
            .iter()
            .any(|symbol| symbol.node.file == id && symbol.name_token == Some(index))
    };
    // Whether a `<` opens the type arguments of a generic, rather than the type parameters
    // a declaration lists or an ordering comparison.
    // Decision: a `Generic` needs its closing `>` and the token after it, so `Box<` at the
    // end of a line is no more in the tree than an unfinished trait list is; when the tree
    // does not say, the tokens do: a `<` written against a name that is used rather than
    // declared opens type arguments, as a comparison is written with space around it.
    // @lfy def/query/main.lfy:completionsAt
    let opens_type_arguments = |index: usize| -> bool {
        if !tokens[index].is(P::LessThan) {
            return false;
        }
        match holder_of(index) {
            Some(rule) if rule == E::TypeArguments.entity() || rule == E::Generic.entity() => true,
            // The `<` of a `TypeParameters` is followed by a name being declared.
            Some(rule) if rule == E::TypeParameters.entity() => false,
            _ => against_previous(index) && !previous_of(index).is_some_and(declares_a_name),
        }
    };
    // Whether a token is the `extends` or the `=` of a `TypeParameter`, after which the
    // constraint or the default type is written.
    // @lfy def/query/main.lfy:completionsAt
    let of_type_parameter =
        |index: usize| -> bool { holder_of(index) == Some(E::TypeParameter.entity()) };
    // Decision: a recoverable rule that fails past its trait uses (`d A is t, ` at the
    // end of a file) puts the comma in an error node, so when the tree does not say the
    // comma is in `TraitUses`, the tokens do: a run of names and commas back to `is` or
    // `extends` is a trait list.
    let in_trait_uses = before.is_some_and(|index| {
        if find_rule(
            model,
            &nodes_covering(model, id, index),
            E::TraitUses.entity(),
        )
        .is_some()
        {
            return true;
        }
        let mut cursor = index;
        while let Some(previous) = previous_of(cursor) {
            let token = &tokens[previous];
            // The `extends` of a `TypeParameter` names a type, not a trait.
            // @lfy def/query/main.lfy:completionsAt
            if token.is(K::ExtendsKeyword) && of_type_parameter(previous) {
                return false;
            }
            if token.is(K::IsKeyword) || token.is(K::ExtendsKeyword) {
                return true;
            }
            if !(token.is(I::Identifier) || token.is(P::Comma) || token.is(P::ValueAccessor)) {
                return false;
            }
            cursor = previous;
        }
        false
    });
    // The `<` of `TypeArguments` or of `Generic`, or a comma inside either.
    // @lfy def/query/main.lfy:completionsAt
    let in_type_arguments = before.is_some_and(|index| {
        if opens_type_arguments(index) {
            return true;
        }
        if !tokens[index].is(P::Comma) {
            return false;
        }
        let chain = nodes_covering(model, id, index);
        if find_rule(model, &chain, E::TypeArguments.entity()).is_some()
            || find_rule(model, &chain, E::Generic.entity()).is_some()
        {
            return true;
        }
        let mut cursor = index;
        while let Some(previous) = previous_of(cursor) {
            if opens_type_arguments(previous) {
                return true;
            }
            let token = &tokens[previous];
            if !(token.is(I::Identifier) || token.is(P::Comma) || token.is(P::ValueAccessor)) {
                return false;
            }
            cursor = previous;
        }
        false
    });
    // Decision: a `DefinitionClause` whose type is still missing is not in the tree
    // either (`const w: ` at a line end), so a colon that is not in a `Conditional` and
    // directly follows a name, a `)`, or a `?` is taken as a definition colon.
    let is_definition_colon = before.is_some_and(|index| {
        let holder = holder_of(index);
        holder == Some(E::DefinitionClause.entity())
            || holder == Some(E::Definition.entity())
            || (holder != Some(E::Conditional.entity())
                && previous_of(index).is_some_and(|previous| {
                    let token = &tokens[previous];
                    is_name_token(token) || token.is(P::GroupClose) || token.is(P::QuestionMark)
                }))
    });
    // The `extends` or the `=` of a `TypeParameter`.
    // @lfy def/query/main.lfy:completionsAt
    let in_type_parameter = before.is_some_and(|index| {
        (tokens[index].is(K::ExtendsKeyword) || tokens[index].is(P::PlainSetter))
            && of_type_parameter(index)
    });
    // Where a type is written: after the colon of a definition, inside type arguments, or
    // after the `extends` or `=` of a type parameter.
    // @lfy def/query/main.lfy:completionsAt
    let offers_type = (before_rule == Some(Rule::Punctuation(P::Colon)) && is_definition_colon)
        || in_type_arguments
        || in_type_parameter;
    let in_prose = find_rule(model, &chain, E::TemplateReference.entity()).is_some()
        || find_rule(model, &chain, C::Documentation.entity()).is_some()
        || matches!(
            before_rule,
            Some(Rule::Literal(L::ReferenceOpen))
                | Some(Rule::Comment(
                    C::LineDocumentationOpen
                        | C::LineDocumentationBody
                        | C::BlockDocumentationOpen
                        | C::BlockDocumentationBody
                ))
        );

    // The completion context: the first value of `CompletionContext`, in declaration order,
    // that holds at the position, so no case below has to rule the earlier ones out.
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:65d806ac198de35cad869a57d7414bb675ce6b60df93d9550604173e33fb584e
    let context = if in_use_string {
        Some(CompletionContext::UsePath)
    } else if before_rule == Some(Rule::Punctuation(P::ContextAccessor)) {
        Some(CompletionContext::Context)
    } else if before_rule == Some(Rule::Punctuation(P::ScopeAccessor)) {
        Some(CompletionContext::Scope)
    } else if before_rule == Some(Rule::Punctuation(P::ParentScopeAccessor)) {
        Some(CompletionContext::ParentScope)
    } else if matches!(
        before_rule,
        Some(Rule::Punctuation(P::ValueAccessor | P::OptionalValueAccessor))
    ) {
        Some(CompletionContext::Value)
    // The `extends` of an `ExtendsClause` names traits; the `extends` of a `TypeParameter`
    // names the type it must extend.
    } else if before_rule == Some(Rule::Keyword(K::IsKeyword))
        || (before_rule == Some(Rule::Keyword(K::ExtendsKeyword)) && !in_type_parameter)
        || (before_rule == Some(Rule::Punctuation(P::Comma))
            && in_trait_uses
            && !in_type_arguments)
    {
        Some(CompletionContext::TraitName)
    } else if in_prose {
        Some(CompletionContext::Prose)
    } else if offers_type {
        Some(CompletionContext::TypeName)
    } else if statement_begins_after(before_token) {
        Some(CompletionContext::Statement)
    } else if before_token.is_some_and(expression_begins_after) {
        Some(CompletionContext::Expression)
    } else {
        None
    };

    match context {
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:7065c2f898fbfc8f0764635d4eda8bad0129e35dff4dbf77f81b25c2138162e5
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:00d112d094be9e9a2cef44f7a8962bbc76ca48587d3775070df6672096caab27
        Some(CompletionContext::UsePath) => {
            let index = at.expect("a use string holds the token the position is in");
            let token = &tokens[index];
            let text = if token.is(L::DoubleQuoteBody) || token.is(L::SingleQuoteBody) {
                typed.clone().unwrap_or_default()
            } else {
                String::new()
            };
            // The last segment typed, rather than the identifier characters, filters a path.
            let (completions, path_prefix) = path_completions(workspace, file, &text);
            out = completions;
            prefix = path_prefix;
        }
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:365a6fefcaac6ce90927cbfb94ec3099fda7365d3d87ab047cc45048069b9974
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:a5a0eab57676095dddbb5dae2b670022049f0b904028643aa04539c7eccaf9a4
        Some(CompletionContext::Context) => {
            // The entity the left side resolves to, as the binder reads it; with no left
            // side, the current entity of the scope the position is in.
            let entity = match left_of_accessor() {
                Some(left) => left_entity(model, left),
                None => Some(model.scopes[scope].current),
            };
            let members = kind_members(model, entity);
            if members.is_empty() {
                // Decision: with no prelude there is no kind data to read, so the
                // properties the context layer gives every entity are offered, as the
                // binder falls back to them too.
                out.extend(ContextProperty::ALL.into_iter().map(|property| Completion {
                    label: property.value().to_string(),
                    kind: OfferedKind::Completion(CompletionKind::Context),
                    detail: None,
                }));
            }
            out.extend(members.into_iter().map(|symbol| Completion {
                label: model.symbols[symbol].name.clone(),
                kind: OfferedKind::Completion(CompletionKind::Context),
                detail: model.entities[model.symbols[symbol].entity]
                    .definition
                    .as_deref()
                    .map(model::strip_references),
            }));
        }
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:176255d2b086cfe7231a0504fa626007c1b81596ae34941771af6ad8f8196d09
        Some(CompletionContext::Scope) => {
            // The scope layer reaches the current entity of the scope that holds the
            // position, whatever stands to the left of the accessor.
            out.extend(members_of(model.scopes[scope].current))
        }
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:990efbecf19087309c0d26899b7a2489e562a4cb35262c755027dc912b462da2
        Some(CompletionContext::ParentScope) => {
            // Decision: the parent of the scope is the nearest enclosing scope whose
            // current entity differs, since a block inside a declaration shares its
            // declaration's current entity.
            let current = model.scopes[scope].current;
            let mut parent = model.scopes[scope].parent;
            while let Some(id) = parent {
                if model.scopes[id].current != current {
                    out.extend(members_of(model.scopes[id].current));
                    break;
                }
                parent = model.scopes[id].parent;
            }
        }
        Some(CompletionContext::Value) => {
            if let Some(offered) =
                left_of_accessor().and_then(|left| left_completions(model, left))
            {
                out.extend(offered);
            }
            // A left side that resolves to nothing offers nothing.
            // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:24de0b278e7cb59f3d9c83a6bb909c20a031057409da2357a4d77aeab5dd38b9
        }
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:de465a4a16bdf7b27a9f87a8544ae370b2a8d7ce5e534bbb2a0adf76eac4eae7
        Some(CompletionContext::TraitName) => trait_names(model, scope, &mut out),
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:fe4d1e99bec6f6b470424e0b309292cc7f1c240f79b4a916a2200d942a2fdc6b
        Some(CompletionContext::Prose) => {
            names(&mut out);
            if let Some(rule_trait) = model.trait_named("rule") {
                for (entity, e) in model.entities.iter().enumerate() {
                    if e.has_trait(rule_trait) {
                        if let Some(symbol) = e.symbol {
                            out.push(symbol_completion(model, symbol));
                        } else if let Some(identifier) = &e.identifier {
                            out.push(Completion {
                                label: identifier.clone(),
                                kind: OfferedKind::Symbol(kind_of(model, entity)),
                                detail: e.definition.as_deref().map(model::strip_references),
                            });
                        }
                    }
                }
            }
        }
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:7b07fa8a1db2032a1eb89bb408148c4ec84ba69ff8899993bea98d722bd80963
        Some(CompletionContext::TypeName) => {
            out.extend(
                visible_symbols(model, scope)
                    .into_iter()
                    .filter(|&symbol| {
                        matches!(
                            model.symbols[symbol].kind,
                            SymbolKind::Data
                                | SymbolKind::Type
                                | SymbolKind::Enum
                                | SymbolKind::Trait
                                // A type parameter visible from the position is a type
                                // wherever one is offered.
                                | SymbolKind::TypeParameter
                        )
                    })
                    .map(|symbol| symbol_completion(model, symbol)),
            );
            out.extend(
                primitive_type_keywords()
                    .into_iter()
                    .map(keyword_completion),
            );
        }
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:51adc1b9fc9e926d06595d76e546949f392045d3296b9a6bb7d093f6a3be644e
        Some(CompletionContext::Statement) => {
            names(&mut out);
            out.extend(statement_keywords().into_iter().map(keyword_completion));
        }
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:8f8f5282e676ec6a8cb4fd951ff172f975a3732075c4f24ce7253b425558cdb1
        Some(CompletionContext::Expression) => {
            names(&mut out);
            out.extend(value_keywords().into_iter().map(keyword_completion));
            out.extend(
                primitive_type_keywords()
                    .into_iter()
                    .map(keyword_completion),
            );
        }
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:4b1007ad4453a4d4bdbe8d1b8b7da0ac7d83d63f02f9dd3d5ebe142646dc292c
        None => {}
    }
    finish(out, &prefix)
}

/// Every symbol of kind trait visible from a scope, plus the module symbols.
// Decision: the traits come first, then the modules, each group in scope order.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:de465a4a16bdf7b27a9f87a8544ae370b2a8d7ce5e534bbb2a0adf76eac4eae7
fn trait_names(model: &Model, scope: ScopeId, out: &mut Vec<Completion>) {
    let visible = visible_symbols(model, scope);
    for kind in [SymbolKind::Trait, SymbolKind::Module] {
        out.extend(
            visible
                .iter()
                .filter(|&&symbol| model.symbols[symbol].kind == kind)
                .map(|&symbol| symbol_completion(model, symbol)),
        );
    }
}

/// Filters completions to the labels beginning with the typed prefix, each label once.
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:2f73553b7465a85e1646af3834d639129df6aad79e6f573cff6b452a45afd6fb
// @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:f09a4184f25a1539b7642492c9797d7f1ecc3165143e3732feca85defb0f55fa
fn finish(completions: Vec<Completion>, prefix: &str) -> Vec<Completion> {
    let mut seen: HashSet<String> = HashSet::new();
    completions
        .into_iter()
        .filter(|completion| completion.label.starts_with(prefix))
        .filter(|completion| seen.insert(completion.label.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::tests::*;

    /// The names of the members the prelude's `Entity` declares.
    fn entity_members_of(workspace: &Workspace) -> Vec<String> {
        let model = &workspace.model;
        let entity = prelude_data(model, "Entity").expect("the prelude declares Entity");
        member_symbols(model, entity)
            .into_iter()
            .map(|symbol| model.symbols[symbol].name.clone())
            .collect()
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:365a6fefcaac6ce90927cbfb94ec3099fda7365d3d87ab047cc45048069b9974
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:a5a0eab57676095dddbb5dae2b670022049f0b904028643aa04539c7eccaf9a4
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:2f73553b7465a85e1646af3834d639129df6aad79e6f573cff6b452a45afd6fb
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:2f6834a48ec282a9c8bcda7fecd60a3e350cd6c72ebc03a766a32d8b5b1493d1
    #[test]
    fn after_the_context_accessor_the_members_of_the_kind_data_are_offered() {
        let fixture = Fixture::with_prelude("d A { $x = string; } const y = A@");
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(1, 33));
        let names = labels(&completions);
        // One per member of the prelude's `Entity`: the kind data of a data declares
        // those and nothing more.
        assert_eq!(names, entity_members_of(&ws));
        assert!(names.contains(&"identifier"));
        assert!(names.contains(&"definition"));
        assert!(names.contains(&"like"));
        assert!(!names.contains(&"A"));
        assert!(!names.contains(&"y"));
        assert!(
            completions
                .iter()
                .all(|c| c.kind == OfferedKind::Completion(CompletionKind::Context))
        );
        // Each carries the member's definition.
        assert_eq!(
            completions[0].detail.as_deref(),
            Some("The declared name; undefined when anonymous")
        );
        // The typed prefix filters, case sensitive.
        let fixture = Fixture::with_prelude("d A { $x = string; } const y = A@de");
        let ws = fixture.load();
        assert_eq!(labels(&completions_at(&ws, A, at(1, 35))), ["definition"]);
        let fixture = Fixture::with_prelude("d A { $x = string; } const y = A@De");
        let ws = fixture.load();
        assert!(completions_at(&ws, A, at(1, 35)).is_empty());
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:b004a292d579f95ff0e64d5683c2c6d17367d72f08608a1525f516432a7045b6
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:b210279c8cc1507c5a57a7cf6f62fedb15356bd132b00cbf1612db89d6c6576c
    #[test]
    fn after_a_value_accessor_a_string_offers_the_members_of_its_base_data() {
        let text = "const s = \"x\"; const y = s.";
        let fixture = Fixture::with_prelude(text);
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(1, text.chars().count()));
        let names = labels(&completions);
        let string = find(&ws, "String");
        assert_eq!(string.len(), 1, "the prelude declares String once");
        let members: Vec<String> = member_symbols(&ws.model, string[0])
            .into_iter()
            .map(|symbol| ws.model.symbols[symbol].name.clone())
            .collect();
        assert_eq!(names, members);
        assert!(names.contains(&"length"));
        assert!(names.contains(&"trim"));
        assert!(!names.contains(&"s"));
        assert!(!names.contains(&"y"));
        assert!(
            completions
                .iter()
                .all(|c| c.kind == OfferedKind::Symbol(SymbolKind::Member))
        );
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:c090000e66157db31b578b3016839e6d86b5bf47a61c842e9482255a61175b4f
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:c8583c31ea5802803ab8c989f680381b5f19150f120c852fc637551bf5dea079
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:1c542a5607a0f3e65d2dcd0ac6ce7bb52906094a0243e6ee2400137b80d8a462
    #[test]
    fn after_a_value_accessor_an_object_and_an_entity_offer_what_the_binder_resolves() {
        let names = |text: &str| -> Vec<String> {
            let fixture = Fixture::with_prelude(text);
            let workspace = fixture.load();
            let position = at(1, text.chars().count());
            completions_at(&workspace, A, position)
                .into_iter()
                .map(|completion| completion.label)
                .collect()
        };
        // An object offers the keys it is written with, then the members of `Object`.
        let object = names("const o = { a = 1, b = 2 }; const u = o.");
        assert_eq!(object[0], "a", "{object:?}");
        assert_eq!(object[1], "b", "{object:?}");
        assert!(object.contains(&"keys".to_string()), "{object:?}");
        assert!(object.contains(&"merge".to_string()), "{object:?}");
        // An entity's own members, then the members of its kind data whose value is a
        // function.
        let entity = names("d A { $x = string; } const v = A.");
        assert_eq!(entity[0], "x");
        assert!(entity.contains(&"like".to_string()), "{entity:?}");
        assert!(entity.contains(&"test".to_string()), "{entity:?}");
        assert!(!entity.contains(&"identifier".to_string()), "{entity:?}");
        // An enum offers its keys alone, with none of its kind data's members.
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:c4fce9642c5615b339d7beba622badd73fc8bcf97e3c5688012f4db54d818b20
        let enumeration = names("enum E { one = 1, two = 2 } const e = E.");
        assert_eq!(enumeration, ["one", "two"], "{enumeration:?}");
        // A list offers every member of `List`.
        let list = names("const l: number[] = [1]; const w = l.");
        assert!(list.contains(&"map".to_string()), "{list:?}");
        assert!(list.contains(&"length".to_string()), "{list:?}");
        // A number offers every member of `Number`.
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:823bbfa9c16889c1e7b52d517feb017ab3e59d11cbe8577cbad6361e38d8d095
        let number = names("const n = 1; const m = n.");
        assert!(number.contains(&"floor".to_string()), "{number:?}");
        assert!(number.contains(&"clamp".to_string()), "{number:?}");
        assert!(!number.contains(&"n".to_string()), "{number:?}");
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:de465a4a16bdf7b27a9f87a8544ae370b2a8d7ce5e534bbb2a0adf76eac4eae7
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:aff25f9ffa6cd60694c089eacd9189c8e497d435d15f84f960f975444ab2f98e
    #[test]
    fn after_is_every_visible_trait_is_offered() {
        let fixture = Fixture::one("trait t {} d A is ");
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(1, 18));
        assert_eq!(completions.len(), 1, "{completions:?}");
        assert_eq!(completions[0].label, "t");
        assert_eq!(completions[0].kind, OfferedKind::Symbol(SymbolKind::Trait));
        // Also after a comma inside trait uses, and after extends; modules are offered too.
        let fixture = Fixture::new();
        fixture
            .write(
                "def/a.lfy",
                "use \"./b\" as B;\ntrait t {} trait u extends t, \nd A is t, ",
            )
            .write("def/b.lfy", "");
        let ws = fixture.load();
        assert_eq!(labels(&completions_at(&ws, A, at(2, 30))), ["t", "u", "B"]);
        assert_eq!(labels(&completions_at(&ws, A, at(3, 10))), ["t", "u", "B"]);
        assert_eq!(
            completions_at(&ws, A, at(3, 10))[2].kind,
            OfferedKind::Symbol(SymbolKind::Module)
        );
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:176255d2b086cfe7231a0504fa626007c1b81596ae34941771af6ad8f8196d09
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:990efbecf19087309c0d26899b7a2489e562a4cb35262c755027dc912b462da2
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:24de0b278e7cb59f3d9c83a6bb909c20a031057409da2357a4d77aeab5dd38b9
    #[test]
    fn accessors_offer_members_of_what_they_reach() {
        let text = "d A { $x: `X` = string; $y = number; }\nenum E { one = 1, two = 2 }\nd B { $z = A.; $w = $; $v = $&; $u = E.; $t = q.; $s = A$; }\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let value = completions_at(&ws, A, after(find_pos(text, "A.;", 0), 2));
        assert_eq!(labels(&value), ["x", "y"]);
        assert_eq!(value[0].kind, OfferedKind::Symbol(SymbolKind::Member));
        assert_eq!(value[0].detail.as_deref(), Some("X"));
        assert_eq!(
            labels(&completions_at(&ws, A, after(find_pos(text, "$;", 0), 1))),
            ["z", "w", "v", "u", "t", "s"]
        );
        assert!(completions_at(&ws, A, after(find_pos(text, "$&;", 0), 2)).is_empty());
        assert_eq!(
            labels(&completions_at(&ws, A, after(find_pos(text, "E.;", 0), 2))),
            ["one", "two"]
        );
        assert!(completions_at(&ws, A, after(find_pos(text, "q.;", 0), 2)).is_empty());
        // The scope accessor reaches the current entity of the scope holding the
        // position, not the members of whatever stands to its left.
        assert_eq!(
            labels(&completions_at(&ws, A, after(find_pos(text, "A$;", 0), 2))),
            ["z", "w", "v", "u", "t", "s"]
        );
        // At the top of a data the parent scope reaches the file's entity, which has no
        // members; a nested data reaches its owner's.
        let text = "d A { $x = string; d C { $y = $&; } }\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert_eq!(
            labels(&completions_at(&ws, A, after(find_pos(text, "$&;", 0), 2))),
            ["x"]
        );
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:e27a530ad71011bb541a5761da43e628b4d40ce9dd245a5e2f3708a75532b763
    #[test]
    fn a_module_offers_its_own_symbols_after_the_value_accessor() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\" as B;\nconst y = B.;\n")
            .write("def/b.lfy", "use \"./c\";\nd B {}\nconst k = 1;\n")
            .write("def/c.lfy", "d C {}\n");
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(2, 12));
        assert_eq!(labels(&completions), ["B", "k"]);
        assert_eq!(completions[0].kind, OfferedKind::Symbol(SymbolKind::Data));
        assert_eq!(
            completions[1].kind,
            OfferedKind::Symbol(SymbolKind::Variable)
        );
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:7065c2f898fbfc8f0764635d4eda8bad0129e35dff4dbf77f81b25c2138162e5
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:00d112d094be9e9a2cef44f7a8962bbc76ca48587d3775070df6672096caab27
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:09043a1a0fcecf03463cebe94a13f4f932c3d36f9809b5daf05af88c39efbdab
    #[test]
    fn inside_the_string_of_a_use_paths_are_offered() {
        let fixture = Fixture::new();
        fixture
            .write(
                "elfie.json",
                r#"{ "lib": "lib", "dependencies": { "rust": { "root": "targets/rust" } } }"#,
            )
            .write(
                "def/a.lfy",
                "use \"./\";\nuse \"\";\nuse \"rust/\";\nuse \"./sub/\";\nuse \"./s\";\n",
            )
            .write("def/b.lfy", "")
            .write("def/sub/inner.lfy", "")
            .write("def/notes.txt", "")
            .write("targets/rust/main.lfy", "")
            .write("targets/rust/guidance.lfy", "");
        let ws = fixture.load();
        let dotted = completions_at(&ws, A, at(1, 7));
        assert_eq!(labels(&dotted), ["a", "b", "sub"]);
        assert!(
            dotted
                .iter()
                .all(|c| c.kind == OfferedKind::Completion(CompletionKind::Path))
        );
        // Every package is offered, the package elfie every program has among them.
        assert_eq!(labels(&completions_at(&ws, A, at(2, 5))), ["rust", "elfie"]);
        assert_eq!(
            labels(&completions_at(&ws, A, at(3, 10))),
            ["guidance", "main"]
        );
        assert_eq!(labels(&completions_at(&ws, A, at(4, 11))), ["inner"]);
        // The last segment typed filters.
        assert_eq!(labels(&completions_at(&ws, A, at(5, 8))), ["sub"]);
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:fe4d1e99bec6f6b470424e0b309292cc7f1c240f79b4a916a2200d942a2fdc6b
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:782b86b627c5d47fd0fbe2fb7ef040dfc62ee913b76ebd52fd91803fe128f64a
    #[test]
    fn inside_a_reference_names_then_rule_entities_are_offered() {
        let text = "trait rule {}\nd Identifier is rule {}\nd A: `See [[A]]` {}\n/// Docs [[y]]\nconst y = 1;\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let in_template = completions_at(&ws, A, after(find_pos(text, "[[A]]", 0), 2));
        assert_eq!(
            labels(&in_template),
            ["rule", "Identifier", "A", "y", "global"]
        );
        assert_eq!(in_template[1].kind, OfferedKind::Symbol(SymbolKind::Data));
        // Typing a name inside a documentation reference filters the same list.
        let in_documentation = completions_at(&ws, A, after(find_pos(text, "[[y]]", 0), 3));
        assert_eq!(labels(&in_documentation), ["y"]);
        // Every rule entity is offered even when it is not visible from the position.
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "trait rule {}\nd Identifier is rule {}\n")
            .write("def/b.lfy", "const y: `See [[I` = 1;\n");
        let ws = fixture.load();
        let far = completions_at(&ws, "def/b.lfy", at(1, 17));
        assert_eq!(labels(&far), ["Identifier"]);
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:7b07fa8a1db2032a1eb89bb408148c4ec84ba69ff8899993bea98d722bd80963
    #[test]
    fn after_the_colon_of_a_definition_types_are_offered() {
        let text = "d A {}\ntrait t {}\nenum E {}\ntype T {}\nconst v = 1;\nconst w: \nconst u: T = { k: };\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(6, 9));
        assert_eq!(
            labels(&completions),
            [
                "A", "t", "E", "T", "boolean", "number", "string", "object", "function", "trait"
            ]
        );
        assert_eq!(
            completions[4].kind,
            OfferedKind::Completion(CompletionKind::Keyword)
        );
        assert!(!labels(&completions).contains(&"v"));
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:7b07fa8a1db2032a1eb89bb408148c4ec84ba69ff8899993bea98d722bd80963
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:6bf26a197be0446516e4af6746ce185df1d0c1ea7ef553ec3f0cb72cc6e58bff
    #[test]
    fn where_a_type_is_offered_a_visible_type_parameter_is_among_the_completions() {
        // Inside the type arguments of a generic, at the end of the file.
        let text = "d Item {} d Box<T> { $pair: `d` = Box<";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(1, text.chars().count()));
        assert_eq!(
            labels(&completions),
            [
                "T", "Item", "Box", "boolean", "number", "string", "object", "function", "trait"
            ]
        );
        assert_eq!(
            completions[0].kind,
            OfferedKind::Symbol(SymbolKind::TypeParameter)
        );
        assert_eq!(completions[1].kind, OfferedKind::Symbol(SymbolKind::Data));
        assert!(!labels(&completions).contains(&"pair"));

        // A `<` and a comma of type arguments the tree does hold, and the `extends` and
        // the `=` of a type parameter.
        let text = "d Item {}\nd Pair<A, B> { $one: `d` = Pair<Item, Item>; }\nd Box<T extends Item = Item> {}\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let after_open = completions_at(&ws, A, after(find_pos(text, "Pair<Item", 0), 5));
        assert_eq!(
            labels(&after_open)[..4],
            ["A", "B", "Item", "Pair"],
            "{:?}",
            labels(&after_open)
        );
        let after_comma = completions_at(&ws, A, after(find_pos(text, "Item, Item", 0), 6));
        assert_eq!(labels(&after_comma)[..4], ["A", "B", "Item", "Pair"]);
        let after_extends = completions_at(&ws, A, after(find_pos(text, "extends Item", 0), 8));
        assert_eq!(labels(&after_extends)[..3], ["T", "Item", "Pair"]);
        let after_setter = completions_at(&ws, A, after(find_pos(text, "Item = Item", 0), 7));
        assert_eq!(labels(&after_setter)[..3], ["T", "Item", "Pair"]);

        // The `<` after the identifier of a declaration lists the names it is generic
        // over, so no type is offered there.
        // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:8f8f5282e676ec6a8cb4fd951ff172f975a3732075c4f24ce7253b425558cdb1
        let text = "d Item {} d Box<";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(1, text.chars().count()));
        let names = labels(&completions);
        assert!(names.contains(&"true"), "{names:?}");
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:beddcfda7878c84398489ed04b9100d3ca66a779785f71339a12a7aac4ff27d0
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:51adc1b9fc9e926d06595d76e546949f392045d3296b9a6bb7d093f6a3be644e
    #[test]
    fn where_a_statement_begins_names_then_statement_keywords_are_offered() {
        let text = "d A {}\nconst v = 1;\nfunction f(p: string) { const q = 2; \n}\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let top = completions_at(&ws, A, at(5, 0));
        let labels_top = labels(&top);
        assert!(
            labels_top.starts_with(&["A", "v", "f", "global", "d", "fn", "function", "trait"]),
            "{labels_top:?}"
        );
        assert!(
            labels_top.contains(&"const")
                && labels_top.contains(&"let")
                && labels_top.contains(&"matchall")
        );
        assert!(!labels_top.contains(&"q"));
        assert!(!labels_top.contains(&"true"));
        // Inside the function, the nearer scope comes first.
        let inner = completions_at(&ws, A, at(3, 37));
        assert!(
            labels(&inner).starts_with(&["q", "p", "A", "v", "f"]),
            "{:?}",
            labels(&inner)
        );
        // At the start of an empty file.
        let fixture = Fixture::one("");
        let ws = fixture.load();
        assert!(labels(&completions_at(&ws, A, at(1, 0))).contains(&"const"));
    }

    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:8f8f5282e676ec6a8cb4fd951ff172f975a3732075c4f24ce7253b425558cdb1
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:f09a4184f25a1539b7642492c9797d7f1ecc3165143e3732feca85defb0f55fa
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:4b1007ad4453a4d4bdbe8d1b8b7da0ac7d83d63f02f9dd3d5ebe142646dc292c
    // @lfy def/query/main.lfy:completionsAt#completionsAt:completionsAt:65d806ac198de35cad869a57d7414bb675ce6b60df93d9550604173e33fb584e
    #[test]
    fn where_an_expression_begins_names_then_value_and_type_keywords_are_offered() {
        let text = "d A {}\nconst w = v + ;\nconst v = \n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let completions = completions_at(&ws, A, at(3, 10));
        assert_eq!(
            labels(&completions),
            [
                "A",
                "w",
                "v",
                "global",
                "true",
                "false",
                "null",
                "undefined",
                "boolean",
                "number",
                "string",
                "object",
                "function",
                "trait"
            ]
        );
        assert_eq!(
            completions[4].kind,
            OfferedKind::Completion(CompletionKind::Keyword)
        );
        assert_eq!(
            labels(&completions_at(&ws, A, at(2, 14)))[..4],
            ["A", "w", "v", "global"]
        );
        // A typed prefix filters the names.
        assert_eq!(labels(&completions_at(&ws, A, at(2, 11))), ["v"]);
        // After a name nothing can begin.
        assert!(completions_at(&ws, A, at(2, 12)).is_empty());
        assert!(completions_at(&ws, "def/none.lfy", at(1, 0)).is_empty());
    }

    // @lfy def/query/main.lfy:CompletionContext
    #[test]
    fn the_completion_contexts_are_tried_in_declaration_order() {
        assert_eq!(CompletionContext::ALL[0], CompletionContext::UsePath);
        assert_eq!(CompletionContext::ALL[9], CompletionContext::Expression);
        assert_eq!(
            CompletionContext::Prose.to_string(),
            "The position is inside a TemplateReference or Documentation"
        );
        assert!(
            CompletionContext::Context
                .value()
                .contains("ContextAccessor"),
            "{}",
            CompletionContext::Context
        );
        // Every context is listed once.
        for context in CompletionContext::ALL {
            assert_eq!(
                CompletionContext::ALL
                    .iter()
                    .filter(|&&other| other == context)
                    .count(),
                1
            );
        }
    }
}
