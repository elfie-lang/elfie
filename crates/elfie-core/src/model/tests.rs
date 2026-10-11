//! Tests compiled from the acceptance criteria and `@test` cases of `def/model/main.lfy`.

use std::path::Path;

use super::*;
use crate::grammar::GrammarRule;
use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::Statement as S;

mod cases;
mod declare;
mod expand;
mod properties;
mod repository;
mod resolve;

// ---- Helpers ------------------------------------------------------------------------

/// `Source@like(`<path>, parsed from: <text>`)`, with one entry per `use` statement, from
/// a file of the program.
fn source(path: &str, text: &str, uses: &[Option<&str>]) -> Source {
    source_from(path, text, uses, Origin::Program)
}

/// The same, for a file of the package `elfie`: its main file is the prelude, its others
/// are the library.
// @lfy def/model/main.lfy:bind
fn source_from(path: &str, text: &str, uses: &[Option<&str>], origin: Origin) -> Source {
    let tokens =
        crate::lexer::lex(text, Some(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
    let tree = crate::parser::parse(tokens, None);
    assert!(
        tree.errors.is_empty(),
        "{path} does not parse: {}",
        tree.render()
    );
    Source {
        path: path.to_string(),
        tree,
        uses: uses.iter().map(|u| u.map(str::to_string)).collect(),
        origin,
    }
}

/// `Source@like(`<path>, parsed`)` for text the parser could not fit into its rules: the
/// tree it hands over holds an `ErrorNode`.
// @lfy def/model/main.lfy:bind
fn source_with_errors(path: &str, text: &str) -> Source {
    let tokens =
        crate::lexer::lex(text, Some(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
    let tree = crate::parser::parse(tokens, None);
    assert!(!tree.errors.is_empty(), "{path} parses without an error");
    Source {
        path: path.to_string(),
        tree,
        uses: Vec::new(),
        origin: Origin::Program,
    }
}

/// The entities a trait was applied to, in application order.
fn entities_of(model: &Model, trait_entity: EntityId) -> Vec<EntityId> {
    model.entities[trait_entity].entities().to_vec()
}

/// One file, `a.lfy`, bound alone.
fn bind_one(text: &str) -> Model {
    bind(vec![source("a.lfy", text, &[])])
}

/// The files of a small package `elfie`, each before the files that use it: the kind
/// data an entity is seen through and the base data a value is read through, brought in
/// by the main file, which is the prelude.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
fn prelude() -> Vec<Source> {
    let entity = source_from(
        "lib/prelude/entity.lfy",
        "d Entity { $identifier: `The declared name` = string | undefined; \
         $type: `Its type` = Entity | undefined; \
         $acceptanceCriteria: `Its criteria` = Entity[]; \
         $knowledge: `What it is given to read` = Entity[]; \
         $commands: `Its commands` = Entity[]; \
         $targets: `Its targets` = Entity[]; \
         $like: `A value the prompt describes` = (prompt: string) => Entity; \
         $test: `Adds cases` = (...tests: Entity[]) => Entity; }",
        &[],
        Origin::Library,
    );
    // `Trait.apply` is a member whose value is a reference to a fn declaration, as the
    // standard library writes it; `Entity.like` is one written as an inline function.
    // @lfy def/model/main.lfy:bind
    let kinds = source_from(
        "lib/prelude/kinds.lfy",
        "use \"./entity\"; d Data extends Entity { $isData: `Marks a data` = boolean; } \
         d Trait extends Entity { $entities: `What carries it` = Entity[]; \
         $apply: `Applies it` = apply; } \
         fn apply(subject: Trait, target: Entity): `Applies a trait` => Trait {} \
         d Function extends Entity { $parameters: `Its parameters` = Entity[]; } \
         d Type extends Entity { $isType: `Marks a type` = boolean; } \
         d Enum extends Entity { $isEnum: `Marks an enum` = boolean; } \
         d Member extends Entity { $isMember: `Marks a member` = boolean; } \
         d Parameter extends Entity { $isParameter: `Marks a parameter` = boolean; } \
         d Variable extends Entity { $isVariable: `Marks a variable` = boolean; } \
         d Module extends Entity { $isModule: `Marks a module` = boolean; }",
        &[Some("lib/prelude/entity.lfy")],
        Origin::Library,
    );
    let values = source_from(
        "lib/values/base.lfy",
        "d String { $length: `How many characters` = number; $trim: `Without spaces` = () => string; } \
         d List { $length: `How many items` = number; } \
         d Object { $keys: `Its keys` = () => string[]; } \
         d Template { $text: `The rendered text` = () => string; } \
         d Number { $floor: `Rounded down` = () => number; } \
         d Boolean { $not: `The opposite` = () => boolean; }",
        &[],
        Origin::Library,
    );
    let main = source_from(
        "lib/main.lfy",
        "use \"./prelude/entity\"; use \"./prelude/kinds\"; use \"./values/base\";",
        &[
            Some("lib/prelude/entity.lfy"),
            Some("lib/prelude/kinds.lfy"),
            Some("lib/values/base.lfy"),
        ],
        Origin::Prelude,
    );
    vec![entity, kinds, values, main]
}

/// The prelude, then one file `a.lfy` of the program.
fn bind_with_prelude(text: &str) -> Model {
    let mut sources = prelude();
    sources.push(source("a.lfy", text, &[]));
    bind(sources)
}

/// The entity of the prelude data `data`: what the prelude scope binds that name to.
fn prelude_data(model: &Model, data: &str) -> EntityId {
    let scope = model.file_scopes[model.file("lib/main.lfy").expect("a prelude")];
    let symbol = model
        .lookup_local(scope, data)
        .unwrap_or_else(|| panic!("the prelude declares no {data}"));
    model.symbols[symbol].entity
}

/// The member symbol named `name` of the prelude data `data`.
fn prelude_member(model: &Model, data: &str, name: &str) -> SymbolId {
    member(model, prelude_data(model, data), name)
}

/// Asserts that the node reads the member `name` of the prelude data `data`. A kind data
/// that extends another holds a symbol of its own for each member it takes on, so it is
/// the member entity that says which member was read.
fn reads_member(model: &Model, r: NodeRef, data: &str, name: &str) {
    let want = model.symbols[prelude_member(model, data, name)].entity;
    let got = usage(model, r).symbol.map(|s| model.symbols[s].entity);
    assert_eq!(
        got,
        Some(want),
        "{:?} does not read {data}.{name}",
        model.raw(r)
    );
}

fn problems(model: &Model) -> Vec<String> {
    model
        .problems
        .iter()
        .map(|p| format!("{}: {}", model.raw(p.node).trim(), p.message))
        .collect()
}

fn assert_clean(model: &Model) {
    assert!(model.problems.is_empty(), "problems: {:?}", problems(model));
}

/// The symbol named `name` declared directly in the file scope of `file`.
fn file_symbol(model: &Model, file: FileId, name: &str) -> SymbolId {
    model
        .lookup_local(model.file_scopes[file], name)
        .unwrap_or_else(|| panic!("no symbol {name} in file {file}"))
}

/// The entity the file-scope symbol `name` of `a.lfy` is bound to.
fn entity(model: &Model, name: &str) -> EntityId {
    model.symbols[file_symbol(model, 0, name)].entity
}

/// The names of the symbols declared directly in a scope, in order.
fn names(model: &Model, scope: ScopeId) -> Vec<String> {
    model.scopes[scope]
        .symbols
        .iter()
        .map(|&s| model.symbols[s].name.clone())
        .collect()
}

/// Every node of `file` (in preorder) satisfying `rule` whose source text is `raw`.
fn nodes<R: GrammarRule + Copy>(model: &Model, file: FileId, rule: R, raw: &str) -> Vec<NodeRef> {
    (0..model.nodes[file].len())
        .map(|index| NodeRef { file, index })
        .filter(|&r| model.info(r).rule == rule.entity() && model.raw(r).trim() == raw)
        .collect()
}

/// The first node of `file` (in preorder) satisfying `rule` whose source text is `raw`.
fn node<R: GrammarRule + Copy>(model: &Model, file: FileId, rule: R, raw: &str) -> NodeRef {
    nodes(model, file, rule, raw)
        .first()
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "no {} node reading {raw:?} in file {file}",
                rule.entity().identifier()
            )
        })
}

/// The usage a node made.
fn usage(model: &Model, r: NodeRef) -> &Usage {
    let id = model
        .usage_of(r)
        .unwrap_or_else(|| panic!("no usage for {:?}", model.raw(r)));
    &model.usages[id]
}

/// The member symbol named `name` in the scope an entity owns.
fn member(model: &Model, entity: EntityId, name: &str) -> SymbolId {
    model
        .members(entity)
        .into_iter()
        .find(|&s| model.symbols[s].name == name)
        .unwrap_or_else(|| {
            panic!(
                "entity has no member {name}: {:?}",
                names(model, model.entities[entity].scope.unwrap())
            )
        })
}

/// A criterion's situation, behavior, and contributor.
type CriterionText = (Option<Vec<String>>, Option<Vec<String>>, EntityId);

fn criteria_texts(model: &Model, entity: EntityId) -> Vec<CriterionText> {
    model.entities[entity]
        .acceptance_criteria
        .iter()
        .map(|c| (c.situation.clone(), c.behavior.clone(), c.contributor))
        .collect()
}
