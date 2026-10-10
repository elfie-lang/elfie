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

// ---- The @test cases of bind --------------------------------------------------------

// @lfy def/model/main.lfy:bind#bind:bind:ba97510fe33631742318285e9d014009568e9145e198b7cab795d387f224b9ed
// @lfy def/model/main.lfy:resolve#resolve:resolve:53a3ecc3bb2df32dde8139e2bf330f085ed18d7051765d5fc332abe19efd20e5
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
    // Trait.entities of t holding A.
    assert_eq!(entities_of(&model, t), vec![a]);
    // A Usage for A.x resolving to that member.
    let member_node = node(&model, 0, E::Member, "A.x");
    let use_of_x = usage(&model, member_node);
    assert_eq!(use_of_x.name.as_deref(), Some("x"));
    assert_eq!(use_of_x.layer, Layer::Value);
    assert_eq!(use_of_x.symbol, Some(x));
    assert_eq!(resolve(&model, member_node), Some(x));
}

// @lfy def/model/main.lfy:bind#bind:bind:8be862e432d183265802e189a8108a3201256cc35ce11fa53d6adc08d36807dd
// @lfy def/model/main.lfy:bind#bind:bind:85e0b777d37918aa64dd528397bd3f5891099278b7d9d4b0c17cd037bcc845ff
// @lfy def/model/main.lfy:resolve#resolve:resolve:38d6961088ad199ed76c8f764b44b1c881e653dd079ba248e2e38c3d317d2632
// @lfy def/model/main.lfy:usagesOf#usagesOf:usagesOf:f5c6a0b87ff4045a8a8155ddade52ffe53608bfbb7a906aa5191ef0684804f7c
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

// @lfy def/model/main.lfy:bind#bind:bind:0e0ebc3f4e95b4a0d73136e1a0a343ae561cd0d20a56025c7457b59f3702bf7d
// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
// @lfy def/model/main.lfy:resolve#resolve:resolve:0752010452c7ee73c4f54e7a2440a5284244bf5798e148f4798a6a6da5832785
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

// @lfy def/model/main.lfy:bind#bind:bind:7b05f6d5579971e158b35b5c1a33048105acd947f999f60a52c06f4489505b74
// @lfy def/model/main.lfy:bind#bind:bind:e1768186983ff55248116d434452fbc5887f69cc23437b9787440736b57fb111
// @lfy def/model/main.lfy:bind#bind:bind:972725faf5a2897534a0e4be77c67b10e2cce3499191a51d3ab27e1bbe14d1ae
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

// @lfy def/model/main.lfy:bind#bind:bind:aca4776b504719a25e83650081cfb2797bbc9776038499ce3118a8868655f65d
// @lfy def/model/main.lfy:bind#bind:bind:7b5a1fec626e619b600c3faefaa81db781f8ebdd297cba7893369d95d1f212c7
// @lfy def/model/main.lfy:bind#bind:bind:88eb2d2cca1fc9e39dfbb1fd77266bdb656d9b5b76c1c8d103d7ea2000ccaa4d
// @lfy def/model/main.lfy:bind#bind:bind:96fe0cf1480dabf1b8ddea3f3c4ae29add39001d68d41f069dc4e8035dce6095
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

// @lfy def/model/main.lfy:bind#bind:bind:cb5992ae8591ce356ff1a44cf61809f3ae9baf591461b92a8a07297a87520468
// @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
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
    // @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
    let model = bind_one(text);
    assert_eq!(problems(&model), vec!["t.apply: t has no member apply"]);
    let (t, a) = (entity(&model, "t"), entity(&model, "A"));
    assert_eq!(model.entities[a].traits.len(), 1);
    assert_eq!(model.entities[a].traits[0].entity, t);
    assert_eq!(
        usage(&model, node(&model, 0, E::Member, "t.apply")).symbol,
        None
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:4cb96974d918e7df0ed61b9db5926546a7fc19ffa628ee3afe7f53046371017b
// @lfy def/model/main.lfy:bind#bind:bind:d6f02618aac6e3cd44227a66ddaf2ec1f0c729ec00b56a27ed1104b872133624
// @lfy def/model/main.lfy:bind#bind:bind:84b135bc194c962eab1f272cc9a44b88891577b78f5263c55b7da1a415de77b2
#[test]
fn test_rule_named_in_is_clause_resolves_to_the_trait() {
    let b = source(
        "b.lfy",
        "trait rule(syntax: string) { $rule: `the rule` = string; }",
        &[],
    );
    let a = source(
        "a.lfy",
        "use \"./b\"; d A is rule(`x`): `a` {} d B is rule(`y`): `b` {}",
        &[Some("b.lfy")],
    );
    let model = bind(vec![b, a]);
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let rule_file = model.file("b.lfy").unwrap();
    let rule_trait = file_symbol(&model, rule_file, "rule");
    let rule_entity = model.symbols[rule_trait].entity;
    // Both usages of rule resolve to the trait, not the member A and B each gain.
    let use_a = usage(&model, node(&model, program, E::TraitUse, "rule(`x`)"));
    let use_b = usage(&model, node(&model, program, E::TraitUse, "rule(`y`)"));
    assert_eq!(use_a.symbol, Some(rule_trait));
    assert_eq!(use_b.symbol, Some(rule_trait));
    let (a_entity, b_entity) = (
        model.symbols[file_symbol(&model, program, "A")].entity,
        model.symbols[file_symbol(&model, program, "B")].entity,
    );
    assert_ne!(member(&model, a_entity, "rule"), rule_trait);
    // A and B each carry one Applied for rule.
    assert_eq!(model.entities[a_entity].traits.len(), 1);
    assert_eq!(model.entities[a_entity].traits[0].entity, rule_entity);
    assert_eq!(model.entities[b_entity].traits.len(), 1);
    assert_eq!(model.entities[b_entity].traits[0].entity, rule_entity);
}

// @lfy def/model/main.lfy:bind#bind:bind:7a0b3c3b948e3a548a7a00553d1a2034a3f8e40b40d58671af64e6894b3904a9
// @lfy def/model/main.lfy:bind#bind:bind:3337bcd2f277d75bdff5cf73924f263e8aaba8499b6f5777ab89546662fd288b
#[test]
fn test_current_with_no_member_name_applies_to_the_file() {
    let model = bind_with_prelude("trait t {} d A {} t.apply(@);");
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let t = model.symbols[file_symbol(&model, program, "t")].entity;
    let a = model.symbols[file_symbol(&model, program, "A")].entity;
    let file_entity = model.file_entities[program];
    assert_eq!(model.entities[file_entity].traits.len(), 1);
    assert_eq!(model.entities[file_entity].traits[0].entity, t);
    assert!(model.entities[a].traits.is_empty());
}

// @lfy def/model/main.lfy:bind#bind:bind:355e034e3cf906671eb9c8b2a9aa0ce561cb231515c8d84b0bcbd34e6e176a8c
// @lfy def/model/main.lfy:bind#bind:bind:ae1713bf3f23dc73e68d44c37f8cc54e9074e2bc5281035c1bac774fea5c6001
#[test]
fn test_type_parameter_of_a_declaration() {
    let model = bind_one("d Item {} d Box<T extends Item = string> { $item: `x` = T; }");
    assert_clean(&model);
    // Symbols Item and Box in the file scope.
    assert_eq!(names(&model, model.file_scopes[0]), ["Item", "Box"]);
    let item = entity(&model, "Item");
    let boxed = entity(&model, "Box");
    // A symbol T of kind typeParameter in the scope Box owns, beside its members.
    let box_scope = model.entities[boxed].scope.expect("a data owns a scope");
    assert_eq!(names(&model, box_scope), ["T", "item"]);
    let t = model.lookup_local(box_scope, "T").expect("a symbol T");
    assert_eq!(model.symbols[t].kind, SymbolKind::TypeParameter);
    // Entity.typeParameters of Box holds T's entity alone.
    let t = model.symbols[t].entity;
    assert_eq!(model.type_parameters(boxed), vec![t]);
    // Its type is Item, its default is string, and it is optional and never a spread.
    assert_eq!(model.entities[t].ty, Some(TypeRef::Entity(item)));
    assert_eq!(
        model.entities[t].value("defaultValue"),
        Some(&Value::Type(Box::new(TypeRef::Primitive("string"))))
    );
    assert_eq!(model.entities[t].value("optional"), Some(&Value::Bool(true)));
    assert_eq!(model.entities[t].value("spread"), Some(&Value::Bool(false)));
    // The member item has T's entity as its type.
    let member_item = model.symbols[member(&model, boxed, "item")].entity;
    assert_eq!(model.entities[member_item].ty, Some(TypeRef::Entity(t)));
}

// @lfy def/model/main.lfy:bind#bind:bind:3189c3f37ea68cb3dc89e1c79842252c20b3b013dd01e698a55e37d78fd03c1b
// @lfy def/model/main.lfy:bind#bind:bind:b14a00f45f5f1aed95a60c9685ea61353a2569a91f82696c6db3053bda11450e
// @lfy def/model/main.lfy:bind#bind:bind:c3a2c16f705e37fc0e8e626331285a435d93f0cde33bcb941c715741f6b53131
#[test]
fn test_a_declaration_seen_with_type_arguments() {
    let model = bind_one(
        "d Box<T> { $item: `x` = T; $items: `y` = T[]; } const b = Box<string>; const i = b.item; const j = b.items;",
    );
    assert_clean(&model);
    let boxed = entity(&model, "Box");
    // The type of b is Box seen with one type argument, string.
    let Some(TypeRef::Entity(seen)) = model.entities[entity(&model, "b")].ty else {
        panic!("b has no entity type");
    };
    assert_eq!(model.entities[seen].identifier.as_deref(), Some("Box"));
    assert_eq!(model.entities[seen].ty, Some(TypeRef::Entity(boxed)));
    assert_eq!(
        model.type_arguments(seen),
        vec![TypeRef::Primitive("string")]
    );
    assert_eq!(model.type_parameters(seen), model.type_parameters(boxed));
    // The usage of item on b resolves to Box's member, and i has type string.
    let read = node(&model, 0, E::Member, "b.item");
    assert_eq!(usage(&model, read).symbol, Some(member(&model, boxed, "item")));
    assert_eq!(
        model.entities[entity(&model, "i")].ty,
        Some(TypeRef::Primitive("string"))
    );
    // j has type List of string, so its item type is string.
    let j = &model.entities[entity(&model, "j")];
    assert_eq!(
        j.ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("string"))))
    );
    assert_eq!(j.item_type, Some(TypeRef::Primitive("string")));
    // The member items of Box itself keeps its type, List of T.
    let t = model.type_parameters(boxed)[0];
    let items = model.symbols[member(&model, boxed, "items")].entity;
    assert_eq!(
        model.entities[items].ty,
        Some(TypeRef::List(Box::new(TypeRef::Entity(t))))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:3ac95c4e5cc4405dcbbe7fdeffdc22176a54f84d56caf246b594f4d1797bc365
#[test]
fn test_too_many_type_arguments_is_a_problem() {
    let model = bind_one("d Box<T> {} const b = Box<string, number>;");
    let Some(TypeRef::Entity(seen)) = model.entities[entity(&model, "b")].ty else {
        panic!("b has no entity type");
    };
    assert_eq!(
        model.type_arguments(seen),
        vec![TypeRef::Primitive("string"), TypeRef::Primitive("number")]
    );
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(
        model.problems[0].node,
        node(&model, 0, E::Generic, "Box<string, number>")
    );
}

// ---- Binding as a whole -------------------------------------------------------------

// @lfy def/model/main.lfy:bind#bind:bind:e6d6ea2f30dca657ba2c80d4aad957763540ba6c1f58a5ac35f5fdb31087c24d
#[test]
fn an_error_node_declares_and_uses_nothing() {
    let model = bind(vec![source_with_errors(
        "a.lfy",
        "d A {} const class = 1; const y = A;",
    )]);
    // The statements that parsed declare their names; the run of tokens the parser could
    // not fit declares none, makes no usage, and is no problem of the binder's.
    assert_eq!(names(&model, model.file_scopes[0]), ["A", "y"]);
    assert_clean(&model);
    let errors = &model.sources[0].tree.errors;
    assert_eq!(errors.len(), 1, "{:?}", errors);
    let covered = errors[0].start..errors[0].end;
    for usage in &model.usages {
        let info = model.info(usage.node);
        assert!(
            !covered.contains(&info.start),
            "a usage inside the error node: {:?}",
            model.raw(usage.node)
        );
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:1bf3cdabd2eb716539d1d5e6cf9e31e98e17285a8d9b8c0a0edde46ff38e5aa5
#[test]
fn every_problem_binding_adds_has_the_binder_stage() {
    // One from the resolve pass, and one the interpreter adds while expanding.
    let model = bind_one("trait t {} d A {} t.apply(A); const y = z;");
    assert!(model.problems.len() >= 2, "{:?}", problems(&model));
    for problem in &model.problems {
        assert_eq!(problem.stage, Stage::Binder, "{}", problem.message);
    }
}

// ---- Declare ------------------------------------------------------------------------

// @lfy def/model/main.lfy:bind#bind:bind:3337bcd2f277d75bdff5cf73924f263e8aaba8499b6f5777ab89546662fd288b
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

// @lfy def/model/main.lfy:bind#bind:bind:583d8bd2aacbcdee71c5cda8c83518a8d2ab447460f739666f8df8093b6fc7a5
// @lfy def/model/main.lfy:bind#bind:bind:5dafa3a6e52ad501215f54b081dab8720e0d83c56fb7277975f861591af27874
// @lfy def/model/main.lfy:bind#bind:bind:bda572541abe6b3dd683ffb7f7c5782bdf7382a98d9a342a3b8d72558c0dc03f
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

// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
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

// @lfy def/model/main.lfy:bind#bind:bind:c1bf94ef6ed00fca9ed870486ad290e18a6640d029d6830b1b8b55b921ffc125
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

// @lfy def/model/main.lfy:bind#bind:bind:f9e18cc96433cea6408e03a88c1980be0eb6e901bd010d2324727105ed2cd26d
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

// @lfy def/model/main.lfy:bind#bind:bind:225b9242d9b9e61c650a37dd5d5bc19b72bc8619ce4cf622e81ad1a1fe2fe441
#[test]
fn a_parameter_and_a_member_of_one_scope_may_share_a_name() {
    let model = bind_one("trait t(rule: string) { $rule: `the rule` = string; } d A is t(`x`) {}");
    assert_clean(&model);
    let t = entity(&model, "t");
    let scope = model.entities[t].scope.expect("a trait owns a scope");
    assert_eq!(names(&model, scope), ["rule", "rule"]);
    let kinds: Vec<SymbolKind> = model.scopes[scope]
        .symbols
        .iter()
        .map(|&s| model.symbols[s].kind)
        .collect();
    assert_eq!(kinds, [SymbolKind::Parameter, SymbolKind::Member]);
    // The receiver gains the member, not the parameter.
    let a = entity(&model, "A");
    assert_eq!(model.symbols[member(&model, a, "rule")].kind, SymbolKind::Member);
}

// @lfy def/model/main.lfy:bind#bind:bind:93c5268d20befb000ee5648dec6bb01d0e3f7414ba2340321ba1115432ad0e90
// @lfy def/model/main.lfy:bind#bind:bind:643d4a3ad6bf896608420d4cea390c9ad3caf7b86c19c85c2582278080f7cb41
#[test]
fn a_type_parameter_is_declared_in_the_scope_of_its_declaration() {
    let model = bind_one(
        "d Item {} fn take<T extends Item>(p: T): `d` => T { const q = p; } \
         trait held<U>: `d` { $u: `d` = U; } type Pair<V> { f = V, }",
    );
    assert_clean(&model);
    for (name, spelled) in [("take", "T"), ("held", "U"), ("Pair", "V")] {
        let scope = model.entities[entity(&model, name)]
            .scope
            .unwrap_or_else(|| panic!("{name} owns no scope"));
        assert_eq!(names(&model, scope)[0], spelled, "{name}");
        let symbol = model.lookup_local(scope, spelled).expect("a symbol");
        assert_eq!(model.symbols[symbol].kind, SymbolKind::TypeParameter, "{name}");
    }
    // It is visible in the parameters, the output, and the body of the declaration, and
    // it is not one of FnEntity.parameters.
    let take = entity(&model, "take");
    let t = model.type_parameters(take)[0];
    let parameters: Vec<&str> = model.entities[take]
        .parameters()
        .iter()
        .map(|&s| model.symbols[s].name.as_str())
        .collect();
    assert_eq!(parameters, ["p"]);
    let p = model.symbols[model.entities[take].parameters()[0]].entity;
    assert_eq!(model.entities[p].ty, Some(TypeRef::Entity(t)));
    assert_eq!(model.entities[take].output(), Some(&TypeRef::Entity(t)));
    // Its kind data is the one a parameter is seen through.
    assert_eq!(model.entities[t].kind, EntityKind::Parameter);
    // A type parameter with no extends clause has no type of its own.
    for name in ["held", "Pair"] {
        let parameter = model.type_parameters(entity(&model, name))[0];
        assert_eq!(model.entities[parameter].ty, None, "{name}");
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:fddf44919f484fdf1157ac285fae11ba183fdc5f899939106c66008b5648600b
#[test]
fn a_type_parameter_has_no_definition_traits_or_criteria() {
    let model = bind_one(
        "d Item {} trait marked {} d Box<T extends Item = string, U> is marked: `a box` {}",
    );
    assert_clean(&model);
    let boxed = entity(&model, "Box");
    // The declaration itself has the definition, the trait, and any criteria.
    assert_eq!(model.entities[boxed].definition.as_deref(), Some("a box"));
    assert_eq!(model.entities[boxed].traits.len(), 1);
    for &parameter in &model.type_parameters(boxed) {
        assert_eq!(model.entities[parameter].definition, None);
        assert!(model.entities[parameter].traits.is_empty());
        assert!(model.entities[parameter].acceptance_criteria.is_empty());
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:de2ee272797ce7725fdbaf5ab6d10caca2ea348db8e96a5e734f5a52d980695f
// @lfy def/model/main.lfy:bind#bind:bind:2dfb72545f7c3d290642c5da47ce2e5e122656af1ab30a73baeefd59d6e4d820
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

// @lfy def/model/main.lfy:bind#bind:bind:0e756633c85cba82bf7a5348e64ed3d981aaff25b46e0d41245a2e8e0a9e26c7
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
    // The statement and the Current that spells the name both declare it; the Current also
    // makes its Usage, resolved to the symbol it declares.
    let statement = node(&model, 0, S::ExpressionStatement, "$x: `d` = string;");
    assert_eq!(model.symbol_of(statement), Some(x));
    let current = node(&model, 0, E::Current, "$x");
    assert_eq!(model.symbol_of(current), Some(x));
    assert_eq!(usage(&model, current).symbol, Some(x));
    let ys = model.symbols[member(&model, d, "ys")].entity;
    assert_eq!(model.entities[ys].definition, None);
    assert_eq!(
        model.entities[ys].ty,
        Some(TypeRef::List(Box::new(TypeRef::Primitive("number"))))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:583d8bd2aacbcdee71c5cda8c83518a8d2ab447460f739666f8df8093b6fc7a5
// @lfy def/model/main.lfy:bind#bind:bind:5e66df798807d759740c0bde7780004559259dcb44c7a9fbe44fe762a615c470
// @lfy def/model/main.lfy:bind#bind:bind:5dafa3a6e52ad501215f54b081dab8720e0d83c56fb7277975f861591af27874
#[test]
fn scope_current_entity() {
    let model = bind_one("d A {} with A { const x = 1; } for (const i in [1]) { const j = i; }");
    assert_clean(&model);
    let a = entity(&model, "A");
    // A scope that belongs to a declaration has the declared entity as its current.
    assert_eq!(
        model.scopes[model.entities[a].scope.expect("a data owns a scope")].current,
        a
    );
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

// @lfy def/model/main.lfy:bind#bind:bind:3daaf7c3051f43abb7b4c2664e4c94383f3258ebf028136f683006034540f88a
// @lfy def/model/main.lfy:bind#bind:bind:81cd673c66e8b5ef0d81262e4dd6bcf40b110f884bf038212940017c42f01483
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
    assert_eq!(names(&model, model.file_scopes[a_file]), ["x", "y", "B"]);
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

// @lfy def/model/main.lfy:bind#bind:bind:85e0b777d37918aa64dd528397bd3f5891099278b7d9d4b0c17cd037bcc845ff
// @lfy def/model/main.lfy:bind#bind:bind:ff80c9e9250025190f5b25a2ac7ee8d46d800e996782e73367a03a40cab77dfb
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

// @lfy def/model/main.lfy:bind#bind:bind:659c28f53c8aced2b9b367a2d5ef5a82f73561138a260c1d0dd6ff121bc69857
// @lfy def/model/main.lfy:bind#bind:bind:3d1e39f70281f06dd97d7419a6c202f8938221160fe42d7f258ec7de6438c798
#[test]
fn an_in_loop_over_a_module_visits_entities_and_an_of_loop_names() {
    let module = || source("b.lfy", "d B1: `one` {} d B2: `two` {}", &[]);
    let entities = source(
        "a.lfy",
        "use \"./b\" as M; d A { for (const item in M) { where (`s`) -> `{{item@definition}}`; } }",
        &[Some("b.lfy")],
    );
    let model = bind(vec![module(), entities]);
    assert_clean(&model);
    let a_file = model.file("a.lfy").unwrap();
    let a = model.symbols[file_symbol(&model, a_file, "A")].entity;
    let behaviors: Vec<Option<Vec<String>>> = criteria_texts(&model, a)
        .into_iter()
        .map(|(_, behavior, _)| behavior)
        .collect();
    assert_eq!(
        behaviors,
        [
            Some(vec!["one".to_string()]),
            Some(vec!["two".to_string()]),
        ]
    );
    let names_of = source(
        "a.lfy",
        "use \"./b\" as M; d A { for (const name of M) { where (`{{name}}`) -> `b`; } }",
        &[Some("b.lfy")],
    );
    let model = bind(vec![module(), names_of]);
    assert_clean(&model);
    let a_file = model.file("a.lfy").unwrap();
    let a = model.symbols[file_symbol(&model, a_file, "A")].entity;
    let situations: Vec<Option<Vec<String>>> = criteria_texts(&model, a)
        .into_iter()
        .map(|(situation, _, _)| situation)
        .collect();
    assert_eq!(
        situations,
        [
            Some(vec!["B1".to_string()]),
            Some(vec!["B2".to_string()]),
        ]
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:10a6088c6ab81cb4ca33f996b33bb4781657309f2463c5fb93876dec10a950d4
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

// @lfy def/model/main.lfy:bind#bind:bind:b05e01c96fecdd7752fc78c1300732e0edb54106c8e363777a9955d769e5549e
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

// ---- Expand -------------------------------------------------------------------------

// @lfy def/model/main.lfy:bind
#[test]
fn apply_call_applies_the_trait() {
    let model = bind_one(
        "trait X(n: number) {} d Y {} X.apply(Y, 3); \
         trait rule(syntax: string) {} trait alternationList(...items: (is rule)[]) {} \
         d P is rule('p') {} d Q is rule('q') {} d Alt is alternationList(P, Q) {} \
         trait marker {} marker.apply(Alt);",
    );
    // No prelude declares Trait here, so apply itself is found nowhere and says so; the
    // call applies the trait all the same.
    // @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
    assert_eq!(
        problems(&model),
        vec![
            "X.apply: X has no member apply",
            "marker.apply: marker has no member apply",
        ]
    );
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

// @lfy def/model/main.lfy:bind
#[test]
fn extends_chain_applies_every_base() {
    let model = bind_one(
        "trait a(s: string) { $v: `the value` = string; .v = s; } trait b(t: string) extends a(`x{{t}}y`) {} d C is b('m') {}",
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

// @lfy def/model/main.lfy:bind#bind:bind:972725faf5a2897534a0e4be77c67b10e2cce3499191a51d3ab27e1bbe14d1ae
#[test]
fn applied_trait_body_lands_on_the_receiver() {
    let model = bind_one(
        "trait t(n: number) { $m: `member` = string; $k: `the value` = number; .k = n; @acceptanceCriteria.add({ behavior = `B {{n}}` }); where (`S`) -> `W`; } \
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

// @lfy def/model/main.lfy:bind#bind:bind:c1bf94ef6ed00fca9ed870486ad290e18a6640d029d6830b1b8b55b921ffc125
// @lfy def/model/main.lfy:bind#bind:bind:bda572541abe6b3dd683ffb7f7c5782bdf7382a98d9a342a3b8d72558c0dc03f
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

// @lfy def/model/main.lfy:bind#bind:bind:e30b7230cf90b227b4300b5748895116c69879d10c31441e5e03a08f27cca6cc
#[test]
fn an_ace_statement_runs_in_the_expand_pass() {
    let model = bind_with_prelude("trait t {} d A {} ace t.apply(A);");
    assert_clean(&model);
    let program = model.file("a.lfy").unwrap();
    let t = model.symbols[file_symbol(&model, program, "t")].entity;
    let a = model.symbols[file_symbol(&model, program, "A")].entity;
    assert!(model.entities[a].has_trait(t));
    assert_eq!(entities_of(&model, t), vec![a]);
}

// @lfy def/model/main.lfy:bind#bind:bind:e80995ebd6bedeb0e22990364e986ca70be730d1538f8b8ac004e3c863e88d56
#[test]
fn trait_entities_in_file_then_application_order() {
    let b = source("b.lfy", "trait t {} d B2 is t {} d B1 is t {}", &[]);
    let a = source(
        "a.lfy",
        "use \"./b\"; d A is t {} t.apply(A);",
        &[Some("b.lfy")],
    );
    let model = bind(vec![b, a]);
    // Nothing but the apply these files bind without a prelude, which is found nowhere.
    // @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
    assert_eq!(problems(&model), vec!["t.apply: t has no member apply"]);
    let t = model.symbols[file_symbol(&model, 0, "t")].entity;
    let ids = |name: &str| model.symbols[model.lookup(model.file_scopes[1], name).unwrap()].entity;
    assert_eq!(entities_of(&model, t), vec![ids("B2"), ids("B1"), ids("A")]);
    // Applying t again records a second application on A but not a second entry.
    assert_eq!(model.entities[ids("A")].traits.len(), 2);
}

// ---- Resolve ------------------------------------------------------------------------

// @lfy def/model/main.lfy:bind#bind:bind:99a57bc2ff67ce0d744b40d5abbde3870acf40c8165f6cf1b133c60dfd86a027
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

// @lfy def/model/main.lfy:bind#bind:bind:07e3b6151afda7d5560da7b993bf6696deec981b1bc4bbf729faed935d554b89
// @lfy def/model/main.lfy:bind#bind:bind:0e0ebc3f4e95b4a0d73136e1a0a343ae561cd0d20a56025c7457b59f3702bf7d
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

// @lfy def/model/main.lfy:bind#bind:bind:0b03717c4f0414186634383cb7963ba8e06452ae1dcd7f981374596221a09046
// @lfy def/model/main.lfy:bind#bind:bind:f037c79f1549c04cb4e180ca6df046e1cb92277d231bd88fd9490e30745c532f
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

// @lfy def/model/main.lfy:bind#bind:bind:0296df6a1500806f0d09ded31855a827561c1a703c7de3a10d6ce695067b48bc
// @lfy def/model/main.lfy:bind#bind:bind:d20244dc3bdb37be74e170d6902fb9e329aa1be9313397e41f2f3b4021507236
#[test]
fn two_rule_entities_with_the_same_identifier_is_a_problem() {
    let a = source(
        "a.lfy",
        "trait rule(syntax: string) {} d Foo is rule(`x`) {}",
        &[],
    );
    let b = source("b.lfy", "use \"./a\"; d Foo is rule(`y`) {}", &[Some("a.lfy")]);
    // A third file, so the template reference is not found by normal scope lookup and
    // falls to the rule fallback, which is ambiguous.
    let c = source("c.lfy", "const t = `[[Foo]]`;", &[]);
    let model = bind(vec![a, b, c]);
    let bfile = model.file("b.lfy").unwrap();
    let cfile = model.file("c.lfy").unwrap();
    let second_foo = model.symbols[file_symbol(&model, bfile, "Foo")].entity;
    let second_node = model.entities[second_foo].node.expect("Foo has a node");
    let reference = node(&model, cfile, E::Name, "Foo");
    assert_eq!(
        problems(&model)
            .into_iter()
            .map(|p| p.contains("a.lfy") || p.contains("more than one rule"))
            .collect::<Vec<_>>(),
        [true, true]
    );
    assert_eq!(model.problems[0].node, second_node);
    assert_eq!(model.problems[1].node, reference);
    assert_eq!(usage(&model, reference).symbol, None);
}

// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
// @lfy def/model/main.lfy:bind#bind:bind:96fe0cf1480dabf1b8ddea3f3c4ae29add39001d68d41f069dc4e8035dce6095
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

// @lfy def/model/main.lfy:bind#bind:bind:88eb2d2cca1fc9e39dfbb1fd77266bdb656d9b5b76c1c8d103d7ea2000ccaa4d
// @lfy def/model/main.lfy:bind#bind:bind:96fe0cf1480dabf1b8ddea3f3c4ae29add39001d68d41f069dc4e8035dce6095
// @lfy def/model/main.lfy:bind#bind:bind:38106a1cd270b7104392d7af5d8562882a29a648334a1b33c02a79d4044e59b6
// @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
#[test]
fn the_kind_data_of_an_entity_gives_its_context_and_function_members() {
    let model = bind_with_prelude(
        "trait t {} d A { $m: `own` = string; } const i = A@identifier; const e = t@entities; \
         const w = A@entities; const o = A.m; const p = t.apply; const l = A.like; \
         const q = t.entities;",
    );
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
    // Then those members of the kind data whose value is a function, whether a reference
    // to a fn declaration, as `Trait.apply` is, or an inline function, as `Entity.like`
    // is.
    // @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
    reads_member(
        &model,
        node(&model, file, E::Member, "t.apply"),
        "Trait",
        "apply",
    );
    reads_member(
        &model,
        node(&model, file, E::Member, "A.like"),
        "Entity",
        "like",
    );
    // A member of the kind data whose value is not a function is not one of them, and,
    // the left side being a trait no more lenient than any other, that is a problem.
    // @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
    let entities = node(&model, file, E::Member, "t.entities");
    assert_eq!(usage(&model, entities).symbol, None);
    assert_eq!(problems(&model), vec!["t.entities: t has no member entities"]);
}

// @lfy def/model/main.lfy:bind#bind:bind:88eb2d2cca1fc9e39dfbb1fd77266bdb656d9b5b76c1c8d103d7ea2000ccaa4d
// @lfy def/model/main.lfy:bind#bind:bind:7469dc0e51b05a45c42ac07d3559b9d5c94720317dde6eaf087ee72618320558
// @lfy def/model/main.lfy:bind#bind:bind:4ab962f1f8c2a853dbcf4a5ed964a4812b80251caabdfe1adf58317d38eb7f86
// @lfy def/model/main.lfy:bind#bind:bind:6bba9856ded82c70f13da7b2aa0fbbc2e366d1b54bc63d982c2c791cffc02b9d
// @lfy def/model/main.lfy:bind#bind:bind:e68fadbfb501e47c9319d0bfb58705fa141e710e9cb1c09e921faabea5f58c85
// @lfy def/model/main.lfy:bind#bind:bind:deb9d97c7e68090dbe776aad57845b8cd480921494d2fa9c5591f79157d44d87
// @lfy def/model/main.lfy:bind#bind:bind:d94eac3ad1d5010a0fecbe38ebe7c578f73ae2916e2873e1893b1d4e85159377
// @lfy def/model/main.lfy:bind#bind:bind:643d4a3ad6bf896608420d4cea390c9ad3caf7b86c19c85c2582278080f7cb41
// @lfy def/model/main.lfy:bind#bind:bind:ef1de1afcf97b2998a6d8056b1c312dbed57285fc5c709fa30732826d39b2479
// @lfy def/model/main.lfy:bind#bind:bind:2a6a30770d94c3859fa6999ffee448372b61158346958fcdfb4448bdb53f5f3a
// @lfy def/model/main.lfy:bind#bind:bind:96fe0cf1480dabf1b8ddea3f3c4ae29add39001d68d41f069dc4e8035dce6095
#[test]
fn the_kind_data_of_an_entity_is_the_prelude_data_for_what_it_is() {
    let b = source("b.lfy", "d B {}", &[]);
    let a = source(
        "a.lfy",
        "use \"./b\" as M; trait t {} fn f(p: string): `d` => string {} \
         d D { $m: `d` = string; } type T { k = string, } enum E { r = 'r', } \
         const c1 = t@entities; const c2 = f@parameters; const c3 = D@isData; \
         const c4 = T@isType; const c5 = E@isEnum; const c6 = D@identifier; \
         with D$m { const c7 = @isMember; } with f$p { const c8 = @isParameter; }",
        &[Some("b.lfy")],
    );
    let mut sources = prelude();
    sources.push(b);
    sources.push(a);
    let model = bind(sources);
    assert_clean(&model);
    let file = model.file("a.lfy").unwrap();
    for (raw, data, name) in [
        ("t@entities", "Trait", "entities"),
        ("f@parameters", "Function", "parameters"),
        ("D@isData", "Data", "isData"),
        ("T@isType", "Type", "isType"),
        ("E@isEnum", "Enum", "isEnum"),
        // Every entity is seen through Entity too, whatever else it is.
        ("D@identifier", "Entity", "identifier"),
    ] {
        reads_member(&model, node(&model, file, E::Member, raw), data, name);
    }
    // A variable and a module carry Variable and Module the same way, but a name bound to
    // one yields its value rather than its entity, so neither is read through a name.
    let _ = (prelude_data(&model, "Variable"), prelude_data(&model, "Module"));
    // A member and a parameter are reached as the current entity of a With.
    for (raw, data, name) in [
        ("@isMember", "Member", "isMember"),
        ("@isParameter", "Parameter", "isParameter"),
    ] {
        reads_member(&model, node(&model, file, E::Current, raw), data, name);
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:a5c1bc569ddb92f8dd361e4827d274e8148ec9c79e5f47f9178da3acd27018eb
// @lfy def/model/main.lfy:bind#bind:bind:13a7ef34563641503b4fd1a9fdcaa38cbb0f8bb47dc497f235d53d63b8e451da
// @lfy def/model/main.lfy:bind#bind:bind:8615b161cda09a3689fd6b2a04cf2bf8b34b863a2664fe21060d5e9cdad694ea
// @lfy def/model/main.lfy:bind#bind:bind:ee99077d54a77f22a5f264d53d5d58e5e0c6b863c79f255573056c5e7d995703
// @lfy def/model/main.lfy:bind#bind:bind:60a286d3f7df83f47847fa8f557446c01e8bb39dd770556bedea6afd56d5fc3b
// @lfy def/model/main.lfy:bind#bind:bind:5f2d132c545382457130c6e190c71974d985527b87f603c95e936bb60ad7edf0
// @lfy def/model/main.lfy:bind#bind:bind:5b5df5fcbe0bfad1f4242837461d9e5756207c0a361e81048345a263528d5740
// @lfy def/model/main.lfy:bind#bind:bind:f3c216ffbdb497c52484ec15de756394007cc9b0e655697d7a2d5d8e241857de
// @lfy def/model/main.lfy:bind#bind:bind:970d0f64685c6d21821fc7b739fbd15e65769f02adef97284ae744e20be0e6a1
// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
#[test]
fn a_value_resolves_the_members_of_its_base_data() {
    let model = bind_with_prelude(
        "const s = 'text'; const n = s.length; const t = s.trim(); const xs: string[] = []; \
         const c = xs.length; const o = { a = 1 }; const k = o.keys(); const v = o.a; \
         const p = `text`; const r = p.text(); const i = 2; const f = i.floor(); \
         const y = true; const z = y.not(); const g = (x: number) => x; \
         const q = g.parameters;",
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
    read("i.floor", "Number", "floor");
    read("y.not", "Boolean", "not");
    read("g.parameters", "Function", "parameters");
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

// However long the chain.
// @lfy def/model/main.lfy:bind#bind:bind:154744cba7a0648bfbee408864406e16fb0a9567bd411590ebb2634557f61d87
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

// Directly, and on the list an earlier add yields.
// @lfy def/model/main.lfy:bind#bind:bind:154744cba7a0648bfbee408864406e16fb0a9567bd411590ebb2634557f61d87
#[test]
fn add_on_the_other_lists_of_a_context_is_the_binders_own_call() {
    let model = bind_with_prelude(
        "d A { @knowledge.add(`k`); @commands.add(`c`).add(`d`); @targets.add(A); }",
    );
    let file = model.file("a.lfy").unwrap();
    for raw in [
        "@knowledge.add",
        "@commands.add",
        "@commands.add(`c`).add",
        "@targets.add",
    ] {
        let add = node(&model, file, E::Member, raw);
        assert_eq!(usage(&model, add).symbol, None, "{raw} resolved to a member");
        assert!(
            !model.problems.iter().any(|problem| problem.node == add),
            "{raw} is a problem: {:?}",
            problems(&model)
        );
    }
}

// @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
// @lfy def/model/main.lfy:bind#bind:bind:092cce29356879ba213cc57e7d617f3b488aa91d35f1da0d98ac951bceae5ce4
// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
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
    // The two reads and the Current that declares the member.
    assert_eq!(usages_of(&model, m).len(), 3);
}

// @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
#[test]
fn the_optional_value_accessor_reads_the_value_layer() {
    let model = bind_one("d D { $m: `x` = string; } const v = D?.m;");
    assert_clean(&model);
    let read = usage(&model, node(&model, 0, E::Member, "D?.m"));
    assert_eq!(read.layer, Layer::Value);
    assert_eq!(read.name.as_deref(), Some("m"));
    assert_eq!(read.symbol, Some(member(&model, entity(&model, "D"), "m")));
}

// @lfy def/model/main.lfy:bind#bind:bind:1f1de1c02f09b3bb5636fac9080f9daf36806ec9284ab40dd72c9325b3a67b39
#[test]
fn the_scope_accessor_with_no_name_yields_the_entitys_scope() {
    let model = bind_one(
        "d Scope {} trait t { $s: `the scope of A` = Scope; .s = A$; } \
         d A is t { $m: `x` = string; }",
    );
    assert_clean(&model);
    let a = entity(&model, "A");
    assert_eq!(
        model.entities[a].value("s"),
        Some(&Value::Scope(
            model.entities[a].scope.expect("a data owns a scope")
        ))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:2a5068f916f94485b682af3154cd51a8d3c0343e4c82670f3036ec3d2c890c5c
// @lfy def/model/main.lfy:bind#bind:bind:997e81cefd2e242372fae5b5fb01542d9a18bd3ccc915f34d1c6f2d310ef7c15
#[test]
fn parent_scope_accessor_yields_the_containing_scope() {
    let model = bind_one(
        "const v = 1; d Scope {} trait t { $p: `the scope around A` = Scope; \
         $r: `the scope around a variable` = Scope; .p = A$&; .r = (&v)$&; } d A is t {}",
    );
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

// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
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

// What a predicate reads includes the members of the traits the entities carrying it
// carry, so a name one of them has is found.
// @lfy def/model/main.lfy:bind#bind:bind:3fce3fdc6efc4c1db0d9e110d857211d0e5138fd4988999728334f627356d678
// @lfy def/model/main.lfy:bind#bind:bind:ff80c9e9250025190f5b25a2ac7ee8d46d800e996782e73367a03a40cab77dfb
#[test]
fn nothing_found_for_a_name_is_a_problem_whatever_the_left_side_is() {
    let mut sources = prelude();
    sources.push(source("m.lfy", "d Far {}", &[]));
    sources.push(source(
        "a.lfy",
        "use \"./m\" as M; trait t {} trait u { $only: `on u` = string; } d A is t, u {} \
         const v = 'text'; const p = t.nope; const q = A.nope; const r = M.nope; \
         const w = v.nope; const x: is t = A; const ok = x.only; const bad = x.nope;",
        &[Some("m.lfy")],
    ));
    let model = bind(sources);
    let file = model.file("a.lfy").unwrap();
    // A name a carrier of the trait has is found: `x` is one of the entities carrying t,
    // and A, the one of them, carries u as well, so the member u declares is read.
    // @lfy def/model/main.lfy:bind
    let u = model.symbols[file_symbol(&model, file, "u")].entity;
    assert_eq!(
        usage(&model, node(&model, file, E::Member, "x.only")).symbol,
        Some(member(&model, u, "only"))
    );
    // Every other left side is read strictly, a predicate no less than a data.
    for raw in ["t.nope", "A.nope", "M.nope", "v.nope", "x.nope"] {
        assert_eq!(
            usage(&model, node(&model, file, E::Member, raw)).symbol,
            None,
            "{raw}"
        );
    }
    assert_eq!(
        problems(&model),
        vec![
            "t.nope: t has no member nope",
            "A.nope: A has no member nope",
            "M.nope: the module declares no nope",
            "v.nope: String has no member nope",
            "x.nope: t has no member nope",
        ]
    );
    // With no prelude bound the kind data of every entity is empty, so a name only the
    // prelude would give is found nowhere.
    // @lfy def/model/main.lfy:bind
    let model = bind_one("trait t {} d A is t {} const p = t.nope; const q = A.nope;");
    assert_eq!(
        problems(&model),
        vec!["t.nope: t has no member nope", "A.nope: A has no member nope"]
    );
    assert_eq!(
        usage(&model, node(&model, 0, E::Member, "t.nope")).symbol,
        None
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:7b5a1fec626e619b600c3faefaa81db781f8ebdd297cba7893369d95d1f212c7
// @lfy def/model/main.lfy:bind#bind:bind:bda572541abe6b3dd683ffb7f7c5782bdf7382a98d9a342a3b8d72558c0dc03f
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

// @lfy def/model/main.lfy:bind#bind:bind:1ae5ed7b2697e5dba785ff51e02191eb4c4471dbae9009ae9f110e5bddf2cd82
#[test]
fn a_name_of_a_type_parameter_yields_the_parameter() {
    let model = bind_one("d Item {} d Box<T extends Item> { $item: `x` = T; }");
    assert_clean(&model);
    let boxed = entity(&model, "Box");
    let t = model.type_parameters(boxed)[0];
    let read = usage(&model, node(&model, 0, E::Name, "T"));
    assert_eq!(read.layer, Layer::Value);
    assert_eq!(read.symbol.map(|s| model.symbols[s].entity), Some(t));
    // Not Item, which is only what it must extend.
    let item = model.symbols[member(&model, boxed, "item")].entity;
    assert_eq!(model.entities[item].ty, Some(TypeRef::Entity(t)));
    assert_ne!(
        model.entities[item].ty,
        Some(TypeRef::Entity(entity(&model, "Item")))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:d0ff9820f3d48b6bc45af20afec3e2cea3d2a230642bc7e57e4e2bc1adf1828f
// @lfy def/model/main.lfy:bind#bind:bind:8e2d85d92800db4ab851664cdba002e06f2897b88f979a103ccb18275251dc53
#[test]
fn a_member_on_a_type_parameter_reads_what_it_extends() {
    let model = bind_one(
        "d Item { $n: `d` = string; } \
         fn take<T extends Item>(p: T): `d` => string { const q = p.n; } \
         fn loose<U>(p: U): `d` => string { const q = p.n; }",
    );
    assert_clean(&model);
    let reads = nodes(&model, 0, E::Member, "p.n");
    assert_eq!(reads.len(), 2);
    let n = member(&model, entity(&model, "Item"), "n");
    assert_eq!(usage(&model, reads[0]).symbol, Some(n));
    assert_eq!(usage(&model, reads[1]).symbol, None);
}

// ---- Properties ---------------------------------------------------------------------

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

// @lfy def/model/main.lfy:bind
#[test]
fn an_alias_and_an_external_declare_a_symbol_and_an_entity() {
    let model = bind_one("d A {} alias B = A; external C = A;");
    assert_clean(&model);
    assert_eq!(names(&model, model.file_scopes[0]), ["A", "B", "C"]);
    let a = entity(&model, "A");
    // An alias names what it points at: its symbol is bound to the target's entity.
    let alias = file_symbol(&model, 0, "B");
    assert_eq!(model.symbols[alias].kind, SymbolKind::Alias);
    assert_eq!(model.symbols[alias].entity, a);
    // An external has an entity of its own, typed by its right side.
    let external = file_symbol(&model, 0, "C");
    assert_eq!(model.symbols[external].kind, SymbolKind::External);
    let c = model.symbols[external].entity;
    assert_ne!(c, a);
    assert_eq!(model.entities[c].identifier.as_deref(), Some("C"));
    assert_eq!(model.entities[c].kind, EntityKind::External);
    assert_eq!(model.entities[c].ty, Some(TypeRef::Entity(a)));
}

// @lfy def/model/main.lfy:bind#bind:bind:121c6cc109446373ec3499d3bf3ced3751d90b778e159a288124ae94e9d5201c
// @lfy def/model/main.lfy:bind#bind:bind:e3a0d7578b24b0586d11c50514b0da77e12ccd6f18672cfca1ef83805e2beec3
// @lfy def/model/main.lfy:bind#bind:bind:f355d3a4fbe3ba176fa6760801c02f0e6a826a0bd65f0a917451f111c3c9a0dc
#[test]
fn an_index_with_nothing_inside_is_a_list_of_the_left_side() {
    let model = bind_one("d A {} const xs: A[] = []; d D { $one = A[]; $two = A[][]; }");
    assert_clean(&model);
    let a = entity(&model, "A");
    let of_a = TypeRef::List(Box::new(TypeRef::Entity(a)));
    let d = entity(&model, "D");
    let member_type = |name: &str| {
        model.entities[model.symbols[member(&model, d, name)].entity]
            .ty
            .clone()
    };
    assert_eq!(member_type("one"), Some(of_a.clone()));
    assert_eq!(
        member_type("two"),
        Some(TypeRef::List(Box::new(of_a.clone())))
    );
    // The brackets of a type position read the same.
    let xs = &model.entities[entity(&model, "xs")];
    assert_eq!(xs.ty, Some(of_a));
    assert_eq!(xs.item_type, Some(TypeRef::Entity(a)));
}

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
    // A bare List, with no argument, has no item type; List of one argument is spelled
    // as a list, so it is the same type as the brackets give.
    // @lfy def/model/main.lfy:bind#bind:bind:121c6cc109446373ec3499d3bf3ced3751d90b778e159a288124ae94e9d5201c
    let mut sources = prelude();
    sources.push(source(
        "a.lfy",
        "const bare: List = 1; const of_string: List<string> = 1; const sugared: string[] = [];",
        &[],
    ));
    let model = bind(sources);
    let list = prelude_data(&model, "List");
    let program = model.file("a.lfy").unwrap();
    let bare = model.symbols[file_symbol(&model, program, "bare")].entity;
    assert_eq!(model.entities[bare].ty, Some(TypeRef::Entity(list)));
    assert_eq!(model.entities[bare].item_type, None);
    let of_string = model.symbols[file_symbol(&model, program, "of_string")].entity;
    assert_eq!(
        model.entities[of_string].ty,
        model.entities[model.symbols[file_symbol(&model, program, "sugared")].entity].ty
    );
    assert_eq!(
        model.entities[of_string].item_type,
        Some(TypeRef::Primitive("string"))
    );
    // This prelude's List lists no type parameter, so the one argument is an arity
    // problem; the type is bound with the argument as written all the same.
    // @lfy def/model/main.lfy:bind#bind:bind:3ac95c4e5cc4405dcbbe7fdeffdc22176a54f84d56caf246b594f4d1797bc365
    assert_eq!(model.problems.len(), 1, "{:?}", problems(&model));
    assert_eq!(
        model.problems[0].node,
        node(&model, program, E::TypeArguments, "<string>")
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:5b5df5fcbe0bfad1f4242837461d9e5756207c0a361e81048345a263528d5740
// @lfy def/model/main.lfy:bind#bind:bind:a5c1bc569ddb92f8dd361e4827d274e8148ec9c79e5f47f9178da3acd27018eb
#[test]
fn a_list_value_has_the_type_of_a_list() {
    let model = bind_with_prelude(
        "d A {} const xs = [1, 2]; const n = xs.length; const many = ['text', A]; \
         const spread = [...xs, 3]; const empty = [];",
    );
    assert_clean(&model);
    let file = model.file("a.lfy").unwrap();
    let ty = |name: &str| {
        model.entities[model.symbols[file_symbol(&model, file, name)].entity]
            .ty
            .clone()
    };
    let item = |name: &str| {
        model.entities[model.symbols[file_symbol(&model, file, name)].entity]
            .item_type
            .clone()
    };
    // A list of numbers is a list of number: the items are values of that kind, not the
    // one value each was written as.
    // @lfy def/model/main.lfy:bind
    assert_eq!(
        ty("xs"),
        Some(TypeRef::List(Box::new(TypeRef::Primitive("number"))))
    );
    assert_eq!(item("xs"), Some(TypeRef::Primitive("number")));
    // Its base data is List, so the members of List are read on it.
    reads_member(&model, node(&model, file, E::Member, "xs.length"), "List", "length");
    // Items of more than one type are a union of them, each written once; a spread holds
    // what the list it spreads holds.
    // @lfy def/model/main.lfy:bind
    let a = model.symbols[file_symbol(&model, file, "A")].entity;
    assert_eq!(
        item("many"),
        Some(TypeRef::Union(vec![
            TypeRef::Primitive("string"),
            TypeRef::Entity(a)
        ]))
    );
    assert_eq!(item("spread"), Some(TypeRef::Primitive("number")));
    // An empty list is a list all the same, of nothing the model can read.
    assert!(matches!(ty("empty"), Some(TypeRef::List(_))));
}

// @lfy def/model/main.lfy:bind#bind:bind:7ef157b80d31dce9baf260f3d953c0bc5b6cfb07b6c4773de70e7be59015ad54
#[test]
fn a_declaration_without_type_parameters_has_none() {
    let model = bind_one("d Plain { $x: `d` = string; } const p: Plain = 1; fn f(): `d` => Plain {}");
    assert_clean(&model);
    let plain = entity(&model, "Plain");
    assert!(model.type_parameters(plain).is_empty());
    assert!(model.type_arguments(plain).is_empty());
    // A Reference without arguments yields the declaration itself.
    assert_eq!(
        model.entities[entity(&model, "p")].ty,
        Some(TypeRef::Entity(plain))
    );
    assert!(model.type_arguments(entity(&model, "p")).is_empty());
}

// @lfy def/model/main.lfy:bind#bind:bind:ae23ffc23c95add84b11276b2987d77d26def0d947d817a507989143282af46c
// @lfy def/model/main.lfy:bind#bind:bind:c3a2c16f705e37fc0e8e626331285a435d93f0cde33bcb941c715741f6b53131
#[test]
fn a_function_type_is_an_anonymous_function_entity() {
    let model = bind_one("type Pair<T> { f = (item: T) => T, } const p: Pair<string> = 1; const q = p.f;");
    assert_clean(&model);
    let pair = entity(&model, "Pair");
    let t = model.type_parameters(pair)[0];
    // The FunctionType owns a scope holding its parameters, and is an anonymous Fn.
    let function_type = node(&model, 0, E::FunctionType, "(item: T) => T");
    let scope = model.scope_of(function_type).expect("a FunctionType owns a scope");
    assert_eq!(names(&model, scope), ["item"]);
    let anonymous = model.scopes[scope].current;
    assert_eq!(model.entities[anonymous].identifier, None);
    assert!(matches!(
        model.entities[anonymous].kind,
        EntityKind::Fn { agent: false, .. }
    ));
    let item = model.symbols[model.entities[anonymous].parameters()[0]].entity;
    assert_eq!(model.entities[item].ty, Some(TypeRef::Entity(t)));
    assert_eq!(model.entities[anonymous].output(), Some(&TypeRef::Entity(t)));
    // Read on a Pair of string, the parameters and the output are substituted.
    let Some(TypeRef::Entity(read)) = model.entities[entity(&model, "q")].ty else {
        panic!("q has no entity type");
    };
    assert_ne!(read, anonymous, "the substitution is a copy, not the declaration");
    let substituted = model.symbols[model.entities[read].parameters()[0]].entity;
    assert_eq!(
        model.entities[substituted].ty,
        Some(TypeRef::Primitive("string"))
    );
    assert_eq!(
        model.entities[read].output(),
        Some(&TypeRef::Primitive("string"))
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:b14a00f45f5f1aed95a60c9685ea61353a2569a91f82696c6db3053bda11450e
// @lfy def/model/main.lfy:bind#bind:bind:7ef157b80d31dce9baf260f3d953c0bc5b6cfb07b6c4773de70e7be59015ad54
#[test]
fn a_declaration_seen_with_arguments_carries_the_declarations_own() {
    let model = bind_one(
        "trait t {} d Box<T> is t: `a box` { $item: `d` = T; where (`s`) -> `b`; } const b = Box<string>;",
    );
    assert_clean(&model);
    let boxed = entity(&model, "Box");
    let Some(TypeRef::Entity(seen)) = model.entities[entity(&model, "b")].ty else {
        panic!("b has no entity type");
    };
    assert_eq!(model.entities[seen].identifier.as_deref(), Some("Box"));
    assert_eq!(model.entities[seen].definition.as_deref(), Some("a box"));
    assert_eq!(criteria_texts(&model, seen), criteria_texts(&model, boxed));
    assert!(!criteria_texts(&model, seen).is_empty());
    assert!(model.entities[seen].has_trait(entity(&model, "t")));
    assert_eq!(model.members(seen), model.members(boxed));
    assert_eq!(model.type_parameters(seen), model.type_parameters(boxed));
    assert_eq!(model.entities[seen].ty, Some(TypeRef::Entity(boxed)));
    // The declaration itself is used without arguments, so it has none.
    assert!(model.type_arguments(boxed).is_empty());
}

// @lfy def/model/main.lfy:bind#bind:bind:c3a2c16f705e37fc0e8e626331285a435d93f0cde33bcb941c715741f6b53131
// @lfy def/model/main.lfy:bind#bind:bind:1314d95fa57fff9074b53571e4db2f0029007b9a8661136a4047fe448c4ec7bf
#[test]
fn a_member_read_substitutes_through_nested_arguments() {
    let model = bind_one(
        "d Pair<A, B> {} d Box<T> { $p: `d` = Pair<T, number>; } const b = Box<string>; const c = b.p;",
    );
    assert_clean(&model);
    let pair = entity(&model, "Pair");
    let Some(TypeRef::Entity(read)) = model.entities[entity(&model, "c")].ty else {
        panic!("c has no entity type");
    };
    assert_eq!(model.generic_base(read), pair);
    assert_eq!(
        model.type_arguments(read),
        vec![TypeRef::Primitive("string"), TypeRef::Primitive("number")]
    );
    // The member of Box itself keeps the parameter.
    let t = model.type_parameters(entity(&model, "Box"))[0];
    let declared = model.symbols[member(&model, entity(&model, "Box"), "p")].entity;
    let Some(TypeRef::Entity(declared)) = model.entities[declared].ty else {
        panic!("the member has no entity type");
    };
    assert_eq!(
        model.type_arguments(declared),
        vec![TypeRef::Entity(t), TypeRef::Primitive("number")]
    );
}

// @lfy def/model/main.lfy:bind#bind:bind:972725faf5a2897534a0e4be77c67b10e2cce3499191a51d3ab27e1bbe14d1ae
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

// The second case reads `A@like(...)` for its expectation.
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

// @lfy def/model/main.lfy:usagesOf#usagesOf:usagesOf:17c43c93cfa66aa764b8655e04d3108f32264d4b7bbae42d1bd6d078241baa1f
#[test]
fn usages_of_follows_an_alias_chain() {
    let model = bind_one("d A {} alias B = A; alias C = B; const x = C; const y = A; const z = 1;");
    assert_clean(&model);
    let found: Vec<String> = usages_of(&model, file_symbol(&model, 0, "A"))
        .into_iter()
        .map(|id| model.raw(model.usages[id].node).trim().to_string())
        .collect();
    assert_eq!(found, ["A", "B", "C", "A"]);
}

// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:10ab31f446369520ef4402e3afa3adf6c05b2834fdd52636b18fba986f4fbb8c
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

// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:68ab34550dbf7d97382cac2af535190686399cde2432c30383b3f4d394640826
// @lfy def/model/main.lfy:criteriaOf#criteriaOf:criteriaOf:42d4fffaea6804d033186fda4ffd02f65ef21b5f6d14c5db018da909d530fcc7
#[test]
fn criteria_of_writes_a_type_as_elfie_source() {
    let model = bind_one(
        "d A {} d B {} type S { f = (one: string) => A } \
         d X { $xs: `the items` = (A | B)[]; $call: `the call` = S.f; \
         @acceptanceCriteria \
           .add({ behavior = `Holds {{$xs@type}}` }) \
           .add({ behavior = [`Calls {{$call@type}}`, `and [[A]]`] }); }",
    );
    assert_clean(&model);
    let x = entity(&model, "X");
    let resolved = criteria_of(&model, x);
    assert_eq!(resolved.len(), 2);
    assert_eq!(
        resolved[0].behavior,
        Some(vec!["Holds (A | B)[]".to_string()])
    );
    // The expand pass ran before the function type was read, and wrote `anonymous` for it.
    assert_eq!(
        model.entities[x].acceptance_criteria[1].behavior,
        Some(vec!["Calls anonymous".to_string(), "and [[A]]".to_string()])
    );
    assert_eq!(
        resolved[1].behavior,
        Some(vec![
            "Calls (one: string) => A".to_string(),
            "and A".to_string()
        ])
    );
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
// @lfy def/model/main.lfy:bind#bind:bind:6a35ddeeb22226f71b5d6275d3b6e40813ee087972140fe66f3475a200fc219c
// @lfy def/model/main.lfy:bind#bind:bind:83ed1e2d5690dd20563ce4e4aa95a0e2f0367de80d043c91f14cea70e57aecc0
// @lfy def/model/main.lfy:bind#bind:bind:e80995ebd6bedeb0e22990364e986ca70be730d1538f8b8ac004e3c863e88d56
// @lfy def/model/main.lfy:bind#bind:bind:81cd673c66e8b5ef0d81262e4dd6bcf40b110f884bf038212940017c42f01483
// @lfy def/model/main.lfy:bind#bind:bind:0b03717c4f0414186634383cb7963ba8e06452ae1dcd7f981374596221a09046
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

    // The package elfie is in the program, so its main file is the prelude.
    // @lfy def/model/main.lfy:bind#bind:bind:88eb2d2cca1fc9e39dfbb1fd77266bdb656d9b5b76c1c8d103d7ea2000ccaa4d
    // @lfy def/model/main.lfy:bind#bind:bind:6d483b3015a5d053cdd10c8fb53309930fd856c54535af7ef7e3b4efc15ae894
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
    // lex: more than ten criteria, the last of them the terminal document a template
    // execution writes in.
    let lex = find_entity(model, "lex", |e| matches!(e.kind, EntityKind::Fn { .. }));
    assert!(
        model.entities[lex].acceptance_criteria.len() > 10,
        "{}",
        model.entities[lex].acceptance_criteria.len()
    );
    let terminals = criteria_of(model, lex)
        .pop()
        .and_then(|criterion| criterion.behavior)
        .expect("the last criterion of lex has a behavior");
    assert!(
        terminals[0].contains("Identifier = ( IdentifierStart"),
        "{:?}",
        terminals[0]
    );
    assert_eq!(
        criteria_of(model, lex).len(),
        model.entities[lex].acceptance_criteria.len()
    );
    // NegateOperation: one binding application, which gives it precedence and
    // associativity.
    let binding = model
        .trait_named("binding")
        .expect("the grammar declares binding");
    let negate = find_entity(model, "NegateOperation", |e| e.kind == EntityKind::Data);
    let bindings = model.entities[negate]
        .traits
        .iter()
        .filter(|a| a.entity == binding)
        .count();
    assert_eq!(bindings, 1);
    assert!(model.entities[negate].value("precedence").is_some());
    // Primary: an alternation of the primaries.
    let primary = find_entity(model, "Primary", |e| e.kind == EntityKind::Data);
    let syntax = object_field(model.entities[primary].value("rule").unwrap(), "syntax")
        .as_str()
        .unwrap();
    // The alternatives in the order they are written, however the alternation spells the
    // separator between them.
    assert!(syntax.starts_with("[[StringLiteral]]"), "{syntax:?}");
    for alternative in ["[[Template]]", "[[Number]]", "[[Boolean]]", "[[Object]]"] {
        assert!(syntax.contains(alternative), "{alternative}: {syntax:?}");
    }
    assert!(syntax.ends_with("[[Type]]"), "{syntax:?}");
    let alternation = model.trait_named("alternationList").unwrap();
    assert!(model.entities[primary].has_trait(alternation));
    assert!(model.entities[primary].has_trait(rule));
}
