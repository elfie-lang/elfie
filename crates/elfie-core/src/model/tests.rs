//! Tests compiled from the acceptance criteria and `@test` cases of `def/model/main.lfy`.

use std::path::Path;

use super::*;
use crate::grammar::GrammarRule;
use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::Statement as S;

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
         $like: `A value the prompt describes` = (prompt: string) => Entity; \
         $test: `Adds cases` = (...tests: Entity[]) => Entity; }",
        &[],
        Origin::Library,
    );
    let kinds = source_from(
        "lib/prelude/kinds.lfy",
        "use \"./entity\"; d Data extends Entity {} \
         d Trait extends Entity { $entities: `What carries it` = Entity[]; \
         $apply: `Applies it` = (target: Entity) => Trait; } \
         d Function extends Entity { $parameters: `Its parameters` = Entity[]; }",
        &[Some("lib/prelude/entity.lfy")],
        Origin::Library,
    );
    let values = source_from(
        "lib/values/base.lfy",
        "d String { $length: `How many characters` = number; $trim: `Without spaces` = () => string; } \
         d List { $length: `How many items` = number; } \
         d Object { $keys: `Its keys` = () => string[]; } \
         d Template { $text: `The rendered text` = () => string; }",
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

// ---- The @test cases of bind --------------------------------------------------------

/// A trait member joins the entity, `A.x` resolves to it, and the trait knows `A`.
// @lfy def/model/main.lfy:bind
#[test]
fn test_trait_member_and_application() {
    let model = bind_one("trait t { $x: `d` = string; } d A is t {} const y = A.x;");
    assert_clean(&model);
    assert_eq!(names(&model, model.file_scopes[0]), ["t", "A", "y"]);
    let t = entity(&model, "t");
    let a = entity(&model, "A");
    // One Applied for t.
    assert_eq!(model.entities[a].traits.len(), 1);
    assert_eq!(model.entities[a].traits[0].entity, t);
    assert!(matches!(
        model.entities[a].traits[0].source,
        AppliedSource::Is(_)
    ));
    // A member x with definition d and type string.
    let x = member(&model, a, "x");
    assert_eq!(model.symbols[x].kind, SymbolKind::Member);
    let x_entity = model.symbols[x].entity;
    assert_eq!(model.entities[x_entity].definition.as_deref(), Some("d"));
    assert_eq!(
        model.entities[x_entity].ty,
        Some(TypeRef::Primitive("string"))
    );
    // TraitEntity.entities of t holding A.
    assert_eq!(entities_of(&model, t), vec![a]);
    // A Usage for A.x resolving to that member.
    let member_node = node(&model, 0, E::Member, "A.x");
    let use_of_x = usage(&model, member_node);
    assert_eq!(use_of_x.name.as_deref(), Some("x"));
    assert_eq!(use_of_x.layer, Layer::Value);
    assert_eq!(use_of_x.symbol, Some(x));
    assert_eq!(resolve(&model, member_node), Some(x));
}

/// A module symbol, a loop variable in the For scope, and a usage of the module.
// @lfy def/model/main.lfy:bind
#[test]
fn test_module_symbol_and_loop_variable() {
    let main = source(
        "main.lfy",
        "use \"./tokens/keyword\" as Keyword; for (const token in Keyword) { }",
        &[Some("tokens/keyword.lfy")],
    );
    let keyword = source("tokens/keyword.lfy", "d Const {} d Let {}", &[]);
    let model = bind(vec![main, keyword]);
    assert_clean(&model);
    let main = model.file("main.lfy").unwrap();
    let keyword = model.file("tokens/keyword.lfy").unwrap();
    // A module symbol Keyword bound to the second file's entity.
    let module = file_symbol(&model, main, "Keyword");
    assert_eq!(model.symbols[module].kind, SymbolKind::Module);
    assert_eq!(model.symbols[module].entity, model.file_entities[keyword]);
    // A loop variable token in the For scope, and nowhere else.
    let for_node = node(&model, main, S::For, "for (const token in Keyword) { }");
    let for_scope = model.scope_of(for_node).expect("a For owns a scope");
    assert_eq!(names(&model, for_scope), ["token"]);
    let token = model.lookup_local(for_scope, "token").unwrap();
    assert_eq!(model.symbols[token].kind, SymbolKind::LoopVariable);
    assert_eq!(names(&model, model.file_scopes[main]), ["Keyword"]);
    assert_eq!(model.symbol_of(for_node), Some(token));
    // A Usage of Keyword with layer value resolving to the module symbol.
    let use_of_module = usage(&model, node(&model, main, E::Name, "Keyword"));
    assert_eq!(use_of_module.layer, Layer::Value);
    assert_eq!(use_of_module.symbol, Some(module));
    assert_eq!(usages_of(&model, module).len(), 1);
}

/// An unresolved name gives a usage without a symbol and one problem.
// @lfy def/model/main.lfy:bind
#[test]
fn test_unresolved_name() {
    let model = bind_one("const y = z;");
    assert_eq!(names(&model, model.file_scopes[0]), ["y"]);
    let z = node(&model, 0, E::Name, "z");
    let use_of_z = usage(&model, z);
    assert_eq!(use_of_z.name.as_deref(), Some("z"));
    assert_eq!(use_of_z.symbol, None);
    assert_eq!(resolve(&model, z), None);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, z);
}

/// An extending trait is an extender, not one of the entities.
// @lfy def/model/main.lfy:bind
#[test]
fn test_extends_entities_and_extenders() {
    let model = bind_one("trait a {} trait b extends a {} d C is b {}");
    assert_clean(&model);
    let (a, b, c) = (
        entity(&model, "a"),
        entity(&model, "b"),
        entity(&model, "C"),
    );
    assert_eq!(entities_of(&model, a), vec![c]);
    assert_eq!(model.entities[a].extenders(), &[b]);
    assert_eq!(entities_of(&model, b), vec![c]);
    assert_eq!(model.entities[b].extenders(), &[] as &[EntityId]);
    // C carries b directly and a through b.
    let traits: Vec<EntityId> = model.entities[c].traits.iter().map(|t| t.entity).collect();
    assert_eq!(traits, [b, a]);
    assert!(matches!(
        model.entities[c].traits[1].source,
        AppliedSource::Inherited(0)
    ));
    assert!(matches!(
        model.entities[b].traits[0].source,
        AppliedSource::Extends(_)
    ));
}

/// A `with` on a member of a data attaches the block's criteria and tests to the member,
/// not to the data that declares it.
// @lfy def/model/main.lfy:bind
#[test]
fn test_with_on_a_member_attaches_to_the_member() {
    let model = bind_one(
        "d X { $m: `d` = string; with X$m { where (`s`) -> `b`; @test({ input = \"a\", expect = \"a\" }); } }",
    );
    assert_clean(&model);
    let x = entity(&model, "X");
    let m = model.symbols[member(&model, x, "m")].entity;
    // The member carries one criterion whose situation is s and behavior is b.
    assert_eq!(
        criteria_texts(&model, m),
        [(
            Some(vec!["s".to_string()]),
            Some(vec!["b".to_string()]),
            m
        )]
    );
    // And one test.
    assert_eq!(model.entities[m].tests.len(), 1);
    assert_eq!(model.entities[m].tests[0].input_text, "\"a\"");
    assert_eq!(model.entities[m].tests[0].expect_text, "\"a\"");
    // X carries no criteria and no tests.
    assert!(model.entities[x].acceptance_criteria.is_empty());
    assert!(model.entities[x].tests.is_empty());
}

/// A program file sees the prelude through the parent of its file scope, and a name of
/// the prelude resolves there.
// @lfy def/model/main.lfy:bind
#[test]
fn test_a_program_file_resolves_names_in_the_prelude() {
    let entity_file = source_from(
        "lib/prelude/entity.lfy",
        "d Entity { $identifier: `The declared name` = string; }",
        &[],
        Origin::Library,
    );
    let main = source_from(
        "lib/main.lfy",
        "use \"./prelude/entity\";",
        &[Some("lib/prelude/entity.lfy")],
        Origin::Prelude,
    );
    let a = source("a.lfy", "d A {} const n = A@identifier; const e = Entity;", &[]);
    let model = bind(vec![entity_file, main, a]);
    assert_clean(&model);
    let (library, prelude, program) = (
        model.file("lib/prelude/entity.lfy").unwrap(),
        model.file("lib/main.lfy").unwrap(),
        model.file("a.lfy").unwrap(),
    );
    // The parent of a program file's scope is the prelude scope; a library file and the
    // prelude itself have none.
    assert_eq!(
        model.scopes[model.file_scopes[program]].parent,
        Some(model.file_scopes[prelude])
    );
    assert_eq!(model.scopes[model.file_scopes[prelude]].parent, None);
    assert_eq!(model.scopes[model.file_scopes[library]].parent, None);
    // The usage of Entity resolves to the prelude's symbol, which the prelude imports.
    let declared = file_symbol(&model, library, "Entity");
    assert_eq!(
        model.scopes[model.file_scopes[prelude]].imports,
        [declared]
    );
    assert_eq!(
        usage(&model, node(&model, program, E::Name, "Entity")).symbol,
        Some(declared)
    );
    // The usage of identifier resolves to that member of Entity.
    let identifier = member(&model, model.symbols[declared].entity, "identifier");
    let read = usage(&model, node(&model, program, E::Member, "A@identifier"));
    assert_eq!(read.layer, Layer::Context);
    assert_eq!(read.symbol, Some(identifier));
}

/// `t.apply(A)` resolves apply through t's kind data and applies the trait.
// @lfy def/model/main.lfy:bind
#[test]
fn test_apply_resolves_through_the_kind_data() {
    let text = "trait t {} d A {} t.apply(A);";
    // With a prelude, apply is the member of Trait the prelude declares.
    let model = bind_with_prelude(text);
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let apply = prelude_member(&model, "Trait", "apply");
    assert_eq!(
        usage(&model, node(&model, program, E::Member, "t.apply")).symbol,
        Some(apply)
    );
    let t = model.symbols[file_symbol(&model, program, "t")].entity;
    let a = model.symbols[file_symbol(&model, program, "A")].entity;
    assert_eq!(model.entities[a].traits.len(), 1);
    assert_eq!(model.entities[a].traits[0].entity, t);
    assert_eq!(entities_of(&model, t), vec![a]);
    // Without one the call still applies the trait, and apply resolves to nothing.
    let model = bind_one(text);
    assert_clean(&model);
    let (t, a) = (entity(&model, "t"), entity(&model, "A"));
    assert_eq!(model.entities[a].traits.len(), 1);
    assert_eq!(model.entities[a].traits[0].entity, t);
    assert_eq!(
        usage(&model, node(&model, 0, E::Member, "t.apply")).symbol,
        None
    );
}

// ---- Declare ------------------------------------------------------------------------

/// Each SourceFile is a file scope whose current entity is an anonymous entity for the
/// file.
// @lfy def/model/main.lfy:bind
#[test]
fn file_scope_current_is_the_anonymous_file_entity() {
    let model = bind_one("const x = 1;");
    assert_clean(&model);
    let scope = &model.scopes[model.file_scopes[0]];
    assert_eq!(scope.parent, None);
    assert_eq!(scope.current, model.file_entities[0]);
    let file = &model.entities[model.file_entities[0]];
    assert_eq!(file.kind, EntityKind::File);
    assert_eq!(file.identifier, None);
    assert_eq!(file.node, Some(NodeRef { file: 0, index: 0 }));
    assert_eq!(file.scope, Some(model.file_scopes[0]));
}

/// A declaration's Block is the child of the declaration's own scope.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
#[test]
fn block_scope_is_child_of_declaration_scope() {
    let model = bind_one("d A { const inner = 1; } function f(p: string) { const q = p; }");
    assert_clean(&model);
    let a = entity(&model, "A");
    let a_scope = model.entities[a].scope.expect("a data owns a scope");
    assert_eq!(model.scopes[a_scope].parent, Some(model.file_scopes[0]));
    assert_eq!(model.scopes[a_scope].current, a);
    let a_block = model
        .scope_of(node(&model, 0, S::Block, "{ const inner = 1; }"))
        .expect("a Block owns a scope");
    assert_eq!(model.scopes[a_block].parent, Some(a_scope));
    assert_eq!(model.scopes[a_block].current, a);
    assert_eq!(names(&model, a_block), ["inner"]);
    let f = entity(&model, "f");
    let f_scope = model.entities[f].scope.unwrap();
    assert_eq!(names(&model, f_scope), ["p"]);
    let f_block = model
        .scope_of(node(&model, 0, S::Block, "{ const q = p; }"))
        .unwrap();
    assert_eq!(model.scopes[f_block].parent, Some(f_scope));
    assert_eq!(names(&model, f_block), ["q"]);
    // Names in a scope are visible throughout it: p resolves from the block. The first
    // Name reading p spells the parameter and is a declaration, not a usage.
    let p = nodes(&model, 0, E::Name, "p");
    assert_eq!(p.len(), 2);
    assert_eq!(model.usage_of(p[0]), None);
    assert_eq!(usage(&model, p[1]).symbol, model.lookup_local(f_scope, "p"));
}

/// With no source of the prelude origin, no file scope has a parent and a name only the
/// prelude would give is found nowhere.
// @lfy def/model/main.lfy:bind
#[test]
fn without_a_prelude_no_file_scope_has_a_parent() {
    let library = source("lib/prelude/entity.lfy", "d Entity {}", &[]);
    let a = source("a.lfy", "const e = Entity;", &[]);
    let model = bind(vec![library, a]);
    for &scope in &model.file_scopes {
        assert_eq!(model.scopes[scope].parent, None);
    }
    let read = node(&model, model.file("a.lfy").unwrap(), E::Name, "Entity");
    assert_eq!(usage(&model, read).symbol, None);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, read);
}

/// A name a file declares for itself shadows the prelude's throughout that file, and no
/// problem is added.
// @lfy def/model/main.lfy:bind
#[test]
fn a_file_name_shadows_the_prelude_without_a_problem() {
    let mut sources = prelude();
    sources.push(source("a.lfy", "d Entity {} const e = Entity;", &[]));
    let model = bind(sources);
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let own = file_symbol(&model, program, "Entity");
    assert_ne!(model.symbols[own].entity, prelude_data(&model, "Entity"));
    assert_eq!(
        usage(&model, node(&model, program, E::Name, "Entity")).symbol,
        Some(own)
    );
}

/// A name declared twice in the same scope: a problem at the second, the first wins.
// @lfy def/model/main.lfy:bind
#[test]
fn duplicate_declaration_is_a_problem_and_the_first_wins() {
    let model = bind_one("const x = 1; d x {} const y = x;");
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    let second = node(&model, 0, S::DataDeclaration, "d x {}");
    assert_eq!(model.problems[0].node, second);
    assert_eq!(model.symbol_of(second), None);
    let first = node(&model, 0, S::VariableDeclaration, "const x = 1;");
    let x = file_symbol(&model, 0, "x");
    assert_eq!(model.symbols[x].node, first);
    assert_eq!(model.symbols[x].kind, SymbolKind::Variable);
    assert_eq!(names(&model, model.file_scopes[0]), ["x", "y"]);
    assert_eq!(usage(&model, node(&model, 0, E::Name, "x")).symbol, Some(x));
}

/// An ObjectKey in an enum declares an enumMember; one in a plain object declares nothing.
// @lfy def/model/main.lfy:bind
#[test]
fn object_key_declares_only_in_an_enum() {
    let model = bind_one("enum Color { red = 'r', blue = 'b' } const o = { key = 1 };");
    assert_clean(&model);
    let color = entity(&model, "Color");
    let members = model.members(color);
    assert_eq!(
        names(&model, model.entities[color].scope.unwrap()),
        ["red", "blue"]
    );
    for m in &members {
        assert_eq!(model.symbols[*m].kind, SymbolKind::EnumMember);
        assert_eq!(
            model.entities[model.symbols[*m].entity].kind,
            EntityKind::EnumMember
        );
    }
    let red = node(&model, 0, E::ObjectKey, "red = 'r'");
    assert_eq!(model.symbol_of(red), Some(members[0]));
    let key = node(&model, 0, E::ObjectKey, "key = 1");
    assert_eq!(model.symbol_of(key), None);
    assert!(model.symbols.iter().all(|s| s.name != "key"));
}

/// `$x: `d` = string;` in a data body declares a member with that definition and type.
// @lfy def/model/main.lfy:bind
#[test]
fn member_statement_declares_a_member() {
    let model = bind_one("d D { $x: `d` = string; $ys = number[]; }");
    assert_clean(&model);
    let d = entity(&model, "D");
    assert_eq!(names(&model, model.entities[d].scope.unwrap()), ["x", "ys"]);
    let x = member(&model, d, "x");
    assert_eq!(model.symbols[x].kind, SymbolKind::Member);
    let x_entity = model.symbols[x].entity;
    assert_eq!(model.entities[x_entity].kind, EntityKind::Member);
    assert_eq!(model.entities[x_entity].identifier.as_deref(), Some("x"));
    assert_eq!(model.entities[x_entity].definition.as_deref(), Some("d"));
    assert_eq!(
        model.entities[x_entity].ty,
        Some(TypeRef::Primitive("string"))
    );
    // The statement and the Current that spells the name both declare it; neither is a usage.
    let statement = node(&model, 0, S::ExpressionStatement, "$x: `d` = string;");
    assert_eq!(model.symbol_of(statement), Some(x));
    let current = node(&model, 0, E::Current, "$x");
    assert_eq!(model.symbol_of(current), Some(x));
    assert_eq!(model.usage_of(current), None);
    let ys = model.symbols[member(&model, d, "ys")].entity;
    assert_eq!(model.entities[ys].definition, None);
    assert_eq!(
        model.entities[ys].ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("number"))))
    );
}

/// A scope's current entity: the declared entity, the With's entity, or the parent's.
// @lfy def/model/main.lfy:bind
#[test]
fn scope_current_entity() {
    let model = bind_one("d A {} with A { const x = 1; } for (const i in [1]) { const j = i; }");
    assert_clean(&model);
    let a = entity(&model, "A");
    let with = model
        .scope_of(node(&model, 0, S::With, "with A { const x = 1; }"))
        .expect("a With owns a scope");
    assert_eq!(model.scopes[with].current, a);
    let with_block = model
        .scope_of(node(&model, 0, S::Block, "{ const x = 1; }"))
        .unwrap();
    assert_eq!(model.scopes[with_block].parent, Some(with));
    assert_eq!(model.scopes[with_block].current, a);
    let loop_scope = model
        .scope_of(node(
            &model,
            0,
            S::For,
            "for (const i in [1]) { const j = i; }",
        ))
        .unwrap();
    assert_eq!(model.scopes[loop_scope].current, model.file_entities[0]);
    let loop_block = model
        .scope_of(node(&model, 0, S::Block, "{ const j = i; }"))
        .unwrap();
    assert_eq!(model.scopes[loop_block].current, model.file_entities[0]);
}

// ---- Use ----------------------------------------------------------------------------

/// Without `as`, the used file's own symbols, not its imports, are visible.
// @lfy def/model/main.lfy:bind
#[test]
fn use_without_as_imports_own_symbols_only() {
    let c = source("c.lfy", "d C {}", &[]);
    let b = source("b.lfy", "use \"./c\"; d B {}", &[Some("c.lfy")]);
    let a = source(
        "a.lfy",
        "use \"./b\"; const x = B; const y = C;",
        &[Some("b.lfy")],
    );
    let model = bind(vec![c, b, a]);
    let (b_file, a_file) = (model.file("b.lfy").unwrap(), model.file("a.lfy").unwrap());
    let b_symbol = file_symbol(&model, b_file, "B");
    assert_eq!(model.scopes[model.file_scopes[a_file]].imports, [b_symbol]);
    assert_eq!(names(&model, model.file_scopes[a_file]), ["x", "y"]);
    assert_eq!(
        usage(&model, node(&model, a_file, E::Name, "B")).symbol,
        Some(b_symbol)
    );
    assert_eq!(model.lookup(model.file_scopes[a_file], "B"), Some(b_symbol));
    let c_use = node(&model, a_file, E::Name, "C");
    assert_eq!(usage(&model, c_use).symbol, None);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, c_use);
    // b.lfy itself sees C.
    assert_eq!(
        model.lookup(model.file_scopes[b_file], "C"),
        Some(file_symbol(&model, model.file("c.lfy").unwrap(), "C"))
    );
}

/// With `as`, a Member on the module resolves in that file's symbols.
// @lfy def/model/main.lfy:bind
#[test]
fn use_with_as_resolves_members_in_the_module() {
    let b = source("b.lfy", "d B {} trait tb {}", &[]);
    let a = source(
        "a.lfy",
        "use \"./b\" as M; const x = M.B; d A is M.tb {} for (const e in M) {}",
        &[Some("b.lfy")],
    );
    let model = bind(vec![b, a]);
    assert_clean(&model);
    let (b_file, a_file) = (model.file("b.lfy").unwrap(), model.file("a.lfy").unwrap());
    let module = file_symbol(&model, a_file, "M");
    assert_eq!(model.symbols[module].kind, SymbolKind::Module);
    assert_eq!(model.symbols[module].entity, model.file_entities[b_file]);
    assert!(model.scopes[model.file_scopes[a_file]].imports.is_empty());
    let member_use = usage(&model, node(&model, a_file, E::Member, "M.B"));
    assert_eq!(member_use.name.as_deref(), Some("B"));
    assert_eq!(member_use.symbol, Some(file_symbol(&model, b_file, "B")));
    let trait_use = usage(&model, node(&model, a_file, E::TraitUse, "M.tb"));
    assert_eq!(trait_use.symbol, Some(file_symbol(&model, b_file, "tb")));
    let a = model.symbols[file_symbol(&model, a_file, "A")].entity;
    assert_eq!(
        entities_of(
            &model,
            model.symbols[file_symbol(&model, b_file, "tb")].entity
        ),
        vec![a]
    );
    // A For over the module visits its symbols' entities.
    let e = model
        .lookup_local(
            model
                .scope_of(node(&model, a_file, S::For, "for (const e in M) {}"))
                .unwrap(),
            "e",
        )
        .unwrap();
    assert_eq!(model.symbols[e].kind, SymbolKind::LoopVariable);
}

/// A `from` loop over a module visits its symbols' names and their entities together.
// @lfy def/model/main.lfy:bind
#[test]
fn a_from_loop_over_a_module_visits_names_and_entities() {
    let b = source("b.lfy", "d B1: `one` {} d B2: `two` {}", &[]);
    let a = source(
        "a.lfy",
        "use \"./b\" as M; d A { for (const name, item from M) { where (`{{name}}`) -> `{{item@definition}}`; } }",
        &[Some("b.lfy")],
    );
    let model = bind(vec![b, a]);
    assert_clean(&model);
    let a_file = model.file("a.lfy").unwrap();
    let a = model.symbols[file_symbol(&model, a_file, "A")].entity;
    assert_eq!(
        criteria_texts(&model, a),
        [
            (
                Some(vec!["B1".to_string()]),
                Some(vec!["one".to_string()]),
                a
            ),
            (
                Some(vec!["B2".to_string()]),
                Some(vec!["two".to_string()]),
                a
            ),
        ]
    );
    // Both names are loop variables of the For's scope.
    let for_node = node(
        &model,
        a_file,
        S::For,
        "for (const name, item from M) { where (`{{name}}`) -> `{{item@definition}}`; }",
    );
    let for_scope = model.scope_of(for_node).expect("a For owns a scope");
    assert_eq!(names(&model, for_scope), ["name", "item"]);
}

/// A Use whose entry is undefined imports nothing and is not a problem.
// @lfy def/model/main.lfy:bind
#[test]
fn use_of_nothing_imports_nothing() {
    let model = bind(vec![source(
        "a.lfy",
        "use \"./missing\"; const x = 1;",
        &[None],
    )]);
    assert_clean(&model);
    assert!(model.scopes[model.file_scopes[0]].imports.is_empty());
    assert_eq!(names(&model, model.file_scopes[0]), ["x"]);
}

// ---- Apply --------------------------------------------------------------------------

/// `X.apply(Y)` applies X to Y, and to every item of an alternationList.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
#[test]
fn apply_call_applies_the_trait() {
    let model = bind_one(
        "trait X(n: number) {} d Y {} X.apply(Y, 3); \
         trait rule(syntax: string) {} trait alternationList(...items: (is rule)[]) {} \
         d P is rule('p') {} d Q is rule('q') {} d Alt is alternationList(P, Q) {} \
         trait marker {} marker.apply(Alt);",
    );
    assert_clean(&model);
    let (x, y) = (entity(&model, "X"), entity(&model, "Y"));
    assert_eq!(model.entities[y].traits.len(), 1);
    let applied = &model.entities[y].traits[0];
    assert_eq!(applied.entity, x);
    assert_eq!(applied.values, vec![Value::Number(3.0)]);
    assert_eq!(applied.arguments.len(), 1);
    assert_eq!(model.raw(applied.arguments[0]).trim(), "3");
    assert_eq!(
        applied.source,
        AppliedSource::Apply(node(&model, 0, E::Call, "X.apply(Y, 3)"))
    );
    assert_eq!(entities_of(&model, x), vec![y]);
    // The alternationList: every item, not the list itself.
    let (marker, alt, p, q) = (
        entity(&model, "marker"),
        entity(&model, "Alt"),
        entity(&model, "P"),
        entity(&model, "Q"),
    );
    assert_eq!(entities_of(&model, marker), vec![p, q]);
    assert!(model.entities[p].has_trait(marker));
    assert!(model.entities[q].has_trait(marker));
    assert!(!model.entities[alt].has_trait(marker));
}

/// An extends chain applies every base with arguments evaluated from the extending
/// trait's parameters.
// @lfy def/model/main.lfy:bind
#[test]
fn extends_chain_applies_every_base() {
    let model = bind_one(
        "trait a(s: string) { .v = s; } trait b(t: string) extends a(`x{{t}}y`) {} d C is b('m') {}",
    );
    assert_clean(&model);
    let (a, b, c) = (
        entity(&model, "a"),
        entity(&model, "b"),
        entity(&model, "C"),
    );
    assert_eq!(
        model.entities[c].value("v"),
        Some(&Value::String("xmy".to_string()))
    );
    let traits: Vec<(EntityId, Vec<Value>)> = model.entities[c]
        .traits
        .iter()
        .map(|t| (t.entity, t.values.clone()))
        .collect();
    assert_eq!(
        traits,
        [
            (b, vec![Value::String("m".into())]),
            (a, vec![Value::String("xmy".into())])
        ]
    );
    assert_eq!(
        model.entities[c].traits[1].source,
        AppliedSource::Inherited(0)
    );
    assert_eq!(entities_of(&model, a), vec![c]);
    assert_eq!(model.entities[a].extenders(), &[b]);
}

/// The trait body's members, values, and criteria land on the receiver, with the trait
/// as contributor.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
#[test]
fn applied_trait_body_lands_on_the_receiver() {
    let model = bind_one(
        "trait t(n: number) { $m: `member` = string; .k = n; @acceptanceCriteria.add({ behavior = `B {{n}}` }); where (`S`) -> `W`; } \
         d A is t(2) { @acceptanceCriteria.add({ behavior = `own` }); }",
    );
    assert_clean(&model);
    let (t, a) = (entity(&model, "t"), entity(&model, "A"));
    // Each member declared in the trait's body joins the entity's scope.
    let m = member(&model, a, "m");
    assert_eq!(model.symbols[m].scope, model.entities[a].scope.unwrap());
    assert_eq!(
        model.symbols[m].entity,
        model.symbols[member(&model, t, "m")].entity
    );
    // The value setters are evaluated with the parameters bound to the arguments.
    assert_eq!(model.entities[a].value("k"), Some(&Value::Number(2.0)));
    // Own criteria first, then the trait's, with the trait as contributor.
    assert_eq!(
        criteria_texts(&model, a),
        [
            (None, Some(vec!["own".to_string()]), a),
            (None, Some(vec!["B 2".to_string()]), t),
            (Some(vec!["S".to_string()]), Some(vec!["W".to_string()]), t),
        ]
    );
}

/// Two applied traits declaring the same member: the most recently applied trait's
/// definition wins, and nothing is reported.
// @lfy def/model/main.lfy:bind
#[test]
fn the_most_recently_applied_trait_declares_the_member() {
    let model = bind_one(
        "trait t1 { $m: `first` = string; } trait t2 { $m: `second` = number; } d A is t1, t2 {}",
    );
    assert_clean(&model);
    let (t1, t2, a) = (
        entity(&model, "t1"),
        entity(&model, "t2"),
        entity(&model, "A"),
    );
    assert_eq!(names(&model, model.entities[a].scope.unwrap()), ["m"]);
    let m = model.symbols[member(&model, a, "m")].entity;
    assert_eq!(m, model.symbols[member(&model, t2, "m")].entity);
    assert_ne!(m, model.symbols[member(&model, t1, "m")].entity);
    assert_eq!(model.entities[m].definition.as_deref(), Some("second"));
    assert_eq!(model.entities[m].ty, Some(TypeRef::Primitive("number")));
    assert_eq!(
        model.entities[a]
            .traits
            .iter()
            .map(|t| t.entity)
            .collect::<Vec<_>>(),
        [t1, t2]
    );
    // An extending trait is applied after the traits it extends, so it wins too.
    let model = bind_one(
        "trait base { $m: `base` = string; } trait over extends base { $m: `over` = number; } d B is over {}",
    );
    assert_clean(&model);
    let b = entity(&model, "B");
    let m = model.symbols[member(&model, b, "m")].entity;
    assert_eq!(model.entities[m].definition.as_deref(), Some("over"));
}

/// A name declared in a child scope shadows the parent's without a problem.
// @lfy def/model/main.lfy:bind
#[test]
fn a_child_scope_shadows_the_parent_without_a_problem() {
    let model = bind_one("const n = 1; d A { const n = 2; const m = n; }");
    assert_clean(&model);
    let outer = file_symbol(&model, 0, "n");
    let block = model
        .scope_of(node(&model, 0, S::Block, "{ const n = 2; const m = n; }"))
        .expect("a Block owns a scope");
    assert_eq!(names(&model, block), ["n", "m"]);
    let inner = model
        .lookup_local(block, "n")
        .expect("the child scope declares n");
    assert_ne!(inner, outer);
    // The name is visible in the child scope, where it hides the parent's.
    assert_eq!(model.lookup(block, "n"), Some(inner));
    assert_eq!(model.lookup(model.file_scopes[0], "n"), Some(outer));
    assert_eq!(
        usage(&model, node(&model, 0, E::Name, "n")).symbol,
        Some(inner)
    );
}

/// TraitEntity.entities is in file order then application order.
// @lfy def/model/main.lfy:bind
#[test]
fn trait_entities_in_file_then_application_order() {
    let b = source("b.lfy", "trait t {} d B2 is t {} d B1 is t {}", &[]);
    let a = source(
        "a.lfy",
        "use \"./b\"; d A is t {} t.apply(A);",
        &[Some("b.lfy")],
    );
    let model = bind(vec![b, a]);
    assert_clean(&model);
    let t = model.symbols[file_symbol(&model, 0, "t")].entity;
    let ids = |name: &str| model.symbols[model.lookup(model.file_scopes[1], name).unwrap()].entity;
    assert_eq!(entities_of(&model, t), vec![ids("B2"), ids("B1"), ids("A")]);
    // Applying t again records a second application on A but not a second entry.
    assert_eq!(model.entities[ids("A")].traits.len(), 2);
}

// ---- Resolve ------------------------------------------------------------------------

/// A Dereference yields the entity its operand is bound to.
// @lfy def/model/main.lfy:bind
#[test]
fn dereference_yields_the_symbol() {
    let model = bind_one("d X {} const y = &X;");
    assert_clean(&model);
    let x = file_symbol(&model, 0, "X");
    let deref = node(&model, 0, E::Dereference, "&X");
    let use_of = usage(&model, deref);
    assert_eq!(use_of.layer, Layer::Dereference);
    assert_eq!(use_of.symbol, Some(x));
    assert_eq!(resolve(&model, deref), Some(x));
    assert_eq!(usage(&model, node(&model, 0, E::Name, "X")).symbol, Some(x));
}

/// A Previous yields the entity declared by the nearest earlier statement.
// @lfy def/model/main.lfy:bind
#[test]
fn previous_yields_the_earlier_declaration() {
    let model = bind_one("d X {} const y = 1; const z = ^^; d W { const w = ^^; }");
    let y = file_symbol(&model, 0, "y");
    let previous = node(&model, 0, E::Previous, "^^");
    let use_of = usage(&model, previous);
    assert_eq!(use_of.layer, Layer::Previous);
    assert_eq!(use_of.name, None);
    assert_eq!(use_of.symbol, Some(y));
    // In W's block nothing earlier declares anything: unresolved, one problem.
    let inner = model
        .usages
        .iter()
        .filter(|u| u.layer == Layer::Previous)
        .map(|u| u.symbol)
        .collect::<Vec<_>>();
    assert_eq!(inner, [Some(y), None]);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(
        model.info(model.problems[0].node).rule,
        E::Previous.entity()
    );
}

/// A template reference resolves in scope, else to the one rule entity with that name.
// @lfy def/model/main.lfy:bind
#[test]
fn template_reference_resolves_in_scope_or_to_the_rule() {
    let rules = source(
        "r.lfy",
        "trait rule(syntax: string) {} d Foo is rule('x') {}",
        &[],
    );
    let other = source(
        "a.lfy",
        "d X {} const y = `[[X]] and [[Foo]]`; const z = Foo;",
        &[],
    );
    let model = bind(vec![rules, other]);
    let (r_file, a_file) = (model.file("r.lfy").unwrap(), model.file("a.lfy").unwrap());
    let foo = file_symbol(&model, r_file, "Foo");
    let x = file_symbol(&model, a_file, "X");
    let names_in_a: Vec<NodeRef> = (0..model.nodes[a_file].len())
        .map(|index| NodeRef {
            file: a_file,
            index,
        })
        .filter(|&r| model.info(r).rule == E::Name.entity())
        .collect();
    let resolved: Vec<(String, Option<SymbolId>)> = names_in_a
        .iter()
        .filter_map(|&r| {
            model
                .usage_of(r)
                .map(|u| (model.raw(r).trim().to_string(), model.usages[u].symbol))
        })
        .collect();
    assert_eq!(
        resolved,
        [
            ("X".to_string(), Some(x)),
            ("Foo".to_string(), Some(foo)),
            ("Foo".to_string(), None)
        ]
    );
    // Only the bare `Foo` outside the template is a problem.
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, names_in_a[2]);
    assert_eq!(usages_of(&model, foo).len(), 1);
}

/// A context member name must be the value of a ContextProperty.
// @lfy def/model/main.lfy:bind
#[test]
fn unknown_context_property_is_a_problem() {
    let model = bind_one("d X { const a = @identifier; const b = @nonsense; const c = X@type; }");
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    let nonsense = node(&model, 0, E::Current, "@nonsense");
    assert_eq!(model.problems[0].node, nonsense);
    assert_eq!(
        model.problems[0].message,
        "nonsense is not a context property"
    );
    let use_of = usage(&model, nonsense);
    assert_eq!(use_of.layer, Layer::Context);
    assert_eq!(use_of.name.as_deref(), Some("nonsense"));
    assert_eq!(use_of.symbol, None);
    assert_eq!(
        usage(&model, node(&model, 0, E::Current, "@identifier")).layer,
        Layer::Context
    );
    assert_eq!(
        usage(&model, node(&model, 0, E::Member, "X@type")).layer,
        Layer::Context
    );
}

/// The kind data of an entity gives the context layer its names, and the value layer the
/// members of it whose value is a function.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
#[test]
fn the_kind_data_of_an_entity_gives_its_context_and_function_members() {
    let model = bind_with_prelude(
        "trait t {} d A { $m: `own` = string; } const i = A@identifier; const e = t@entities; \
         const w = A@entities; const o = A.m; const p = t.apply; const q = t.entities;",
    );
    assert_clean(&model);
    let file = model.file("a.lfy").unwrap();
    // Entity gives every entity its identifier; Trait gives a trait its entities.
    reads_member(
        &model,
        node(&model, file, E::Member, "A@identifier"),
        "Entity",
        "identifier",
    );
    reads_member(
        &model,
        node(&model, file, E::Member, "t@entities"),
        "Trait",
        "entities",
    );
    // A name of a kind data other than the entity's yields undefined, without a problem.
    let other = usage(&model, node(&model, file, E::Member, "A@entities"));
    assert_eq!(other.layer, Layer::Context);
    assert_eq!(other.symbol, None);
    // The value layer reads the entity's own members first.
    let a = model.symbols[file_symbol(&model, file, "A")].entity;
    assert_eq!(
        usage(&model, node(&model, file, E::Member, "A.m")).symbol,
        Some(member(&model, a, "m"))
    );
    // Then those members of the kind data whose value is a function, and no others.
    reads_member(
        &model,
        node(&model, file, E::Member, "t.apply"),
        "Trait",
        "apply",
    );
    // A member of the kind data whose value is not a function is not one of them.
    assert_eq!(
        usage(&model, node(&model, file, E::Member, "t.entities")).symbol,
        None
    );
}

/// A value of a base data resolves that data's members; an object's keys are its own.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
#[test]
fn a_value_resolves_the_members_of_its_base_data() {
    let model = bind_with_prelude(
        "const s = 'text'; const n = s.length; const t = s.trim(); const xs: string[] = []; \
         const c = xs.length; const o = { a = 1 }; const k = o.keys(); const v = o.a; \
         const p = `text`; const r = p.text();",
    );
    assert_clean(&model);
    let file = model.file("a.lfy").unwrap();
    let read = |raw: &str, data: &str, name: &str| {
        reads_member(&model, node(&model, file, E::Member, raw), data, name);
    };
    read("s.length", "String", "length");
    read("s.trim", "String", "trim");
    read("xs.length", "List", "length");
    read("o.keys", "Object", "keys");
    read("p.text", "Template", "text");
    // An object's keys come first and are not known here, so o.a is no problem.
    assert_eq!(usage(&model, node(&model, file, E::Member, "o.a")).symbol, None);
    // A name the base data does not declare is a problem.
    let model = bind_with_prelude("const s = 'text'; const n = s.nope;");
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(
        model.problems[0].node,
        node(&model, model.file("a.lfy").unwrap(), E::Member, "s.nope")
    );
}

/// The add of `@acceptanceCriteria.add(...)` is the binder's own call: no symbol, no
/// problem, however long the chain.
// @lfy def/model/main.lfy:bind
#[test]
fn add_on_the_criteria_of_a_context_is_the_binders_own_call() {
    let model = bind_with_prelude(
        "d A { @acceptanceCriteria.add({ behavior = `one` }).add({ behavior = `two` }); }",
    );
    assert_clean(&model);
    let file = model.file("a.lfy").unwrap();
    for add in nodes(&model, file, E::Member, "@acceptanceCriteria.add") {
        assert_eq!(usage(&model, add).symbol, None);
    }
    let a = model.symbols[file_symbol(&model, file, "A")].entity;
    assert_eq!(model.entities[a].acceptance_criteria.len(), 2);
}

/// A Member with the value accessor on a data resolves to its member; with the scope
/// accessor, to the member symbol itself.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
#[test]
fn member_on_data_resolves_to_its_member() {
    let model =
        bind_one("d D { $m: `x` = string; } const v = D.m; const s = D$m; const w = D.nope;");
    let d = entity(&model, "D");
    let m = member(&model, d, "m");
    let value = usage(&model, node(&model, 0, E::Member, "D.m"));
    assert_eq!(
        (value.layer, value.name.as_deref(), value.symbol),
        (Layer::Value, Some("m"), Some(m))
    );
    let scope = usage(&model, node(&model, 0, E::Member, "D$m"));
    assert_eq!(
        (scope.layer, scope.name.as_deref(), scope.symbol),
        (Layer::Scope, Some("m"), Some(m))
    );
    let nope = node(&model, 0, E::Member, "D.nope");
    assert_eq!(usage(&model, nope).symbol, None);
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, nope);
    assert_eq!(usages_of(&model, m).len(), 2);
}

/// The parent scope accessor yields the scope that contains the left side's scope, and
/// undefined when there is none.
// @lfy def/model/main.lfy:bind
#[test]
fn parent_scope_accessor_yields_the_containing_scope() {
    let model = bind_one("const v = 1; trait t { .p = A$&; .r = (&v)$&; } d A is t {}");
    assert_clean(&model);
    let a = entity(&model, "A");
    assert_eq!(
        model.entities[a].value("p"),
        Some(&Value::Scope(model.file_scopes[0]))
    );
    // A variable owns no scope, so there is no containing scope to yield.
    assert_eq!(model.entities[a].value("r"), Some(&Value::Undefined));
    assert_eq!(
        usage(&model, node(&model, 0, E::Member, "A$&")).layer,
        Layer::Parent
    );
}

/// A member of a name that resolved to nothing adds no second problem.
// @lfy def/model/main.lfy:bind
#[test]
fn a_member_of_an_unresolved_name_is_not_a_second_problem() {
    let model = bind_one("const y = z.deeper;");
    let z = node(&model, 0, E::Name, "z");
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(model.problems[0].node, z);
    let deeper = node(&model, 0, E::Member, "z.deeper");
    assert_eq!(usage(&model, deeper).name.as_deref(), Some("deeper"));
    assert_eq!(usage(&model, deeper).symbol, None);
}

/// A Name resolves to the first match walking outward.
// @lfy def/model/main.lfy:bind
#[test]
fn name_resolves_to_the_nearest_scope() {
    let model =
        bind_one("const n = 1; function f(n: string) { const inner = n; } const outer = n;");
    assert_clean(&model);
    let f = entity(&model, "f");
    let parameter = model
        .lookup_local(model.entities[f].scope.unwrap(), "n")
        .unwrap();
    let global = file_symbol(&model, 0, "n");
    let uses: Vec<Option<SymbolId>> = model
        .usages
        .iter()
        .filter(|u| u.name.as_deref() == Some("n"))
        .map(|u| u.symbol)
        .collect();
    assert_eq!(uses, [Some(parameter), Some(global)]);
    assert_eq!(usages_of(&model, global).len(), 1);
}

// ---- Properties ---------------------------------------------------------------------

/// Entity.identifier, Entity.definition, FnEntity.parameters, and FnEntity.output.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
#[test]
fn identifier_definition_parameters_and_output() {
    let model = bind_one(
        "d A: `about A` {} d B: 'about B' {} fn f(a: string, ...rest: number[]): `does f` => A {} function g() -> string { return 'x'; }",
    );
    assert_clean(&model);
    let a = entity(&model, "A");
    assert_eq!(model.entities[a].identifier.as_deref(), Some("A"));
    assert_eq!(model.entities[a].definition.as_deref(), Some("about A"));
    assert_eq!(
        model.entities[entity(&model, "B")].definition.as_deref(),
        Some("about B")
    );
    let f = &model.entities[entity(&model, "f")];
    assert_eq!(f.identifier.as_deref(), Some("f"));
    assert_eq!(f.definition.as_deref(), Some("does f"));
    let parameters: Vec<(String, SymbolKind)> = f
        .parameters()
        .iter()
        .map(|&s| (model.symbols[s].name.clone(), model.symbols[s].kind))
        .collect();
    assert_eq!(
        parameters,
        [
            ("a".to_string(), SymbolKind::Parameter),
            ("rest".to_string(), SymbolKind::Parameter)
        ]
    );
    assert_eq!(f.output(), Some(&TypeRef::Entity(a)));
    assert!(matches!(f.kind, EntityKind::Fn { agent: true, .. }));
    let g = &model.entities[entity(&model, "g")];
    assert_eq!(g.definition, None);
    assert!(g.parameters().is_empty());
    assert_eq!(g.output(), Some(&TypeRef::Primitive("string")));
    assert!(matches!(g.kind, EntityKind::Fn { agent: false, .. }));
    // A parameter's type is the type after its colon.
    let rest = model.symbols[f.parameters()[1]].entity;
    assert_eq!(
        model.entities[rest].ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("number"))))
    );
}

/// Entity.type is the entity itself for a data, trait, type, or enum declaration.
// @lfy def/model/main.lfy:bind
#[test]
fn type_of_a_declaration_is_itself() {
    let model = bind_one("d A {} trait T {} type Ty { k = string, } enum En { a = 'a' }");
    assert_clean(&model);
    for name in ["A", "T", "Ty", "En"] {
        let e = entity(&model, name);
        assert_eq!(model.entities[e].ty, Some(TypeRef::Entity(e)), "{name}");
        assert_eq!(model.entities[e].item_type, None, "{name}");
    }
}

/// Entity.type from a type expression, a member's right side, or a value; a list type
/// gives Entity.itemType.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
#[test]
fn type_and_item_type() {
    let model = bind_one(
        "d A {} const xs: A[] = []; const n = 1; const s = 'text'; d D { $items = string[]; $one = A; }",
    );
    assert_clean(&model);
    let a = entity(&model, "A");
    let xs = &model.entities[entity(&model, "xs")];
    assert_eq!(xs.ty, Some(TypeRef::List(Box::new(TypeRef::Entity(a)))));
    assert_eq!(xs.item_type, Some(TypeRef::Entity(a)));
    let n = &model.entities[entity(&model, "n")];
    assert_eq!(n.ty, Some(TypeRef::Literal(Box::new(Value::Number(1.0)))));
    assert_eq!(n.item_type, None);
    let s = &model.entities[entity(&model, "s")];
    assert_eq!(
        s.ty,
        Some(TypeRef::Literal(Box::new(Value::String("text".into()))))
    );
    let d = entity(&model, "D");
    let items = &model.entities[model.symbols[member(&model, d, "items")].entity];
    assert_eq!(
        items.ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("string"))))
    );
    assert_eq!(items.item_type, Some(TypeRef::Primitive("string")));
    let one = &model.entities[model.symbols[member(&model, d, "one")].entity];
    assert_eq!(one.ty, Some(TypeRef::Entity(a)));
    assert_eq!(one.item_type, None);
}

/// An add call or a Where appends one Criterion, in the body or in a With.
// @lfy def/model/main.lfy:bind
#[test]
fn add_and_where_append_criteria() {
    let model = bind_one(
        "d A { @acceptanceCriteria.add({ situation = `s`, behavior = [`b1`, `b2`], sideEffects = [`e`] }).add({ behavior = `second` }); where ((`w1`) or (`w2`)) and (`w3`) -> `then`; } \
         d B {} with B { @acceptanceCriteria.add({ behavior = `from with` }); where (`in with`) -> `w`; }",
    );
    assert_clean(&model);
    let a = entity(&model, "A");
    let criteria = &model.entities[a].acceptance_criteria;
    assert_eq!(criteria.len(), 3);
    assert_eq!(criteria[0].situation, Some(vec!["s".to_string()]));
    assert_eq!(
        criteria[0].behavior,
        Some(vec!["b1".to_string(), "b2".to_string()])
    );
    assert_eq!(criteria[0].side_effects, Some(vec!["e".to_string()]));
    assert_eq!(criteria[0].contributor, a);
    assert_eq!(criteria[1].behavior, Some(vec!["second".to_string()]));
    assert_eq!(criteria[1].side_effects, None);
    assert_eq!(
        criteria[2].situation,
        Some(vec!["w1 or w2".to_string(), "w3".to_string()])
    );
    assert_eq!(criteria[2].behavior, Some(vec!["then".to_string()]));
    assert_eq!(criteria[2].contributor, a);
    let b = entity(&model, "B");
    let in_with: Vec<Option<Vec<String>>> = model.entities[b]
        .acceptance_criteria
        .iter()
        .map(|c| c.behavior.clone())
        .collect();
    assert_eq!(
        in_with,
        [
            Some(vec!["from with".to_string()]),
            Some(vec!["w".to_string()])
        ]
    );
    assert!(
        model.entities[model.file_entities[0]]
            .acceptance_criteria
            .is_empty()
    );
}

/// A call of Entity.test on the entity's context appends each argument as one Test.
// @lfy def/model/main.lfy:bind
#[test]
fn test_call_appends_tests() {
    let model = bind_one(
        "d A { @test({ input = 1, expect = 'one' }, { input = [1, 2], expect = A@like(`two`) }); }",
    );
    assert_clean(&model);
    let tests = &model.entities[entity(&model, "A")].tests;
    assert_eq!(tests.len(), 2);
    assert_eq!(tests[0].input_text, "1");
    assert_eq!(tests[0].expect_text, "'one'");
    assert_eq!(tests[1].input_text, "[1, 2]");
    assert_eq!(tests[1].expect_text, "A@like(`two`)");
    assert_eq!(
        tests[0].input.map(|r| model.info(r).rule),
        Some(E::Number.entity())
    );
    assert!(tests[1].expect.is_some());
}

/// criteriaOf replaces each template reference by the referenced entity's identifier.
// @lfy def/model/main.lfy:criteriaOf
#[test]
fn criteria_of_strips_references() {
    let model = bind_one(
        "d B {} d A { @acceptanceCriteria.add({ situation = `given [[B]]`, behavior = `see [[B]] and [[B.x]]` }); }",
    );
    let a = entity(&model, "A");
    let raw = &model.entities[a].acceptance_criteria[0];
    assert_eq!(
        raw.behavior,
        Some(vec!["see [[B]] and [[B.x]]".to_string()])
    );
    let resolved = criteria_of(&model, a);
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].situation, Some(vec!["given B".to_string()]));
    assert_eq!(
        resolved[0].behavior,
        Some(vec!["see B and B.x".to_string()])
    );
    assert_eq!(resolved[0].contributor, a);
    assert_eq!(entities_of(&model, a), Vec::<EntityId>::new());
}

// ---- The repository -----------------------------------------------------------------

/// The one entity with this identifier satisfying `keep`.
fn find_entity(model: &Model, name: &str, keep: impl Fn(&Entity) -> bool) -> EntityId {
    let found: Vec<EntityId> = (0..model.entities.len())
        .filter(|&e| {
            model.entities[e].identifier.as_deref() == Some(name) && keep(&model.entities[e])
        })
        .collect();
    assert_eq!(found.len(), 1, "entities named {name}: {found:?}");
    found[0]
}

fn object_field<'a>(value: &'a Value, key: &str) -> &'a Value {
    match value {
        Value::Object(pairs) => {
            &pairs
                .iter()
                .find(|(k, _)| k == key)
                .unwrap_or_else(|| panic!("no field {key}"))
                .1
        }
        other => panic!("not an object: {other:?}"),
    }
}

/// The whole repository binds with zero problems, and its model reads as expected.
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
// @lfy def/model/main.lfy:bind
#[test]
fn repository_binds_without_problems() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let workspace = crate::workspace::load(root);
    let model = &workspace.model;
    assert!(
        workspace.files.len() > 20,
        "{} files",
        workspace.files.len()
    );
    let messages: Vec<String> = workspace
        .problems
        .iter()
        .map(|p| format!("{p:?}"))
        .collect();
    assert!(workspace.problems.is_empty(), "{messages:#?}");
    assert!(model.problems.is_empty(), "{:?}", problems(model));

    // The package elfie is in the program, so its main file is the prelude: a program
    // file's scope has the prelude's as parent, and every apply of a trait resolves to
    // the member of Trait the prelude declares.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    let components = model
        .sources
        .iter()
        .position(|source| source.path.ends_with("model/components.lfy"))
        .expect("the definitions declare the model's components");
    assert!(model.scopes[model.file_scopes[components]].parent.is_some());
    let apply = model.symbols[member(
        model,
        find_entity(model, "Trait", |e| e.kind == EntityKind::Data),
        "apply",
    )]
    .entity;
    let applies: Vec<Option<EntityId>> = model
        .usages
        .iter()
        .filter(|usage| usage.node.file == components && usage.name.as_deref() == Some("apply"))
        .map(|usage| usage.symbol.map(|s| model.symbols[s].entity))
        .collect();
    assert!(applies.len() > 10, "{} apply calls", applies.len());
    assert!(applies.iter().all(|&read| read == Some(apply)), "{applies:?}");

    let rule = model
        .trait_named("rule")
        .expect("the grammar declares rule");
    // Space: a rule whose EBNF line reads `Space = " " | ... ;`.
    let space = find_entity(model, "Space", |e| e.kind == EntityKind::Data);
    assert!(model.entities[space].has_trait(rule));
    let space_rule = model.entities[space].value("rule").expect("rule value");
    assert_eq!(
        object_field(space_rule, "identifier"),
        &Value::String("Space".into())
    );
    let text = object_field(space_rule, "text").as_str().unwrap();
    assert!(text.starts_with("Space = \" \" | \"\t\" | "), "{text:?}");
    assert!(
        text.ends_with("| \"\u{2028}\" | \"\u{2029}\" ;"),
        "{text:?}"
    );
    assert_eq!(model.rule_entity("Space"), Some(space));
    // lex: more than 200 criteria.
    let lex = find_entity(model, "lex", |e| matches!(e.kind, EntityKind::Fn { .. }));
    assert!(
        model.entities[lex].acceptance_criteria.len() > 200,
        "{}",
        model.entities[lex].acceptance_criteria.len()
    );
    assert_eq!(
        criteria_of(model, lex).len(),
        model.entities[lex].acceptance_criteria.len()
    );
    // NegateOperation: two binding applications.
    let binding = model
        .trait_named("binding")
        .expect("the grammar declares binding");
    let negate = find_entity(model, "NegateOperation", |e| e.kind == EntityKind::Data);
    let bindings = model.entities[negate]
        .traits
        .iter()
        .filter(|a| a.entity == binding)
        .count();
    assert_eq!(bindings, 2);
    // Primary: an alternation of the primaries.
    let primary = find_entity(model, "Primary", |e| e.kind == EntityKind::Data);
    let syntax = object_field(model.entities[primary].value("rule").unwrap(), "syntax")
        .as_str()
        .unwrap();
    assert!(
        syntax.starts_with("[[StringLiteral]] | [[Template]] | [[Number]] | "),
        "{syntax:?}"
    );
    assert!(syntax.ends_with(" | [[Type]]"), "{syntax:?}");
    let alternation = model.trait_named("alternationList").unwrap();
    assert!(model.entities[primary].has_trait(alternation));
    assert!(model.entities[primary].has_trait(rule));
}
