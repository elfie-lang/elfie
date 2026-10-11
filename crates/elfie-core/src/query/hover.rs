// @lfy def/query/main.lfy:hoverOf
//! Everything shown for one entity, and for the symbol at a position.

use super::*;
use crate::model::TypeRef;

/// The range that covers the identifier of an entity's declaration, as `namedBy` finds
/// it: the token that spells its name, the declaring node when it spells none.
// Decision: `global` declares nothing in any file, so its range names no file, as
// [`source_of`] gives it no text.
// @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:a7d65e3cb8822c1288b11d4be9d75dc74c04674819b25044dfd48fec1f1a86e3
fn identifier_range(workspace: &Workspace, entity: EntityId) -> Range {
    let model = &workspace.model;
    let e = &model.entities[entity];
    if let Some(symbol) = e.symbol {
        let s = &model.symbols[symbol];
        if is_real(model, s.node)
            && let Some(index) = s.name_token
        {
            return span(workspace, s.node.file, index, index + 1);
        }
    }
    match e.node.filter(|&node| is_real(model, node)) {
        Some(node) => range_of_node(workspace, node),
        None => Range::empty("", Position::new(1, 0)),
    }
}

/// The text of one documentation node: its raw text without the opener and closer, each
/// line without one leading space.
// @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:b395bb2363c1570038cd79a7ed40cf8d984560e819c03a901a14b43ecd099ebe
// The raw text is taken as written, so a reference in documentation keeps its brackets.
// @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:c6d255ae1d2dd294f888b666d7c089499ee5684a23fb38dc9a0797725d6f0f3b
fn documentation_of(workspace: &Workspace, file: FileId, node: &Node) -> String {
    let text = workspace.model.sources[file].tree.raw(node.start, node.end);
    let inner = text
        .strip_prefix("///")
        .or_else(|| text.strip_prefix("/**"))
        .unwrap_or(&text);
    let inner = inner.strip_suffix("**/").unwrap_or(inner);
    inner
        .lines()
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The joined text of the documentation nodes of a declaring node; `None` when it has
/// none.
// @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:b395bb2363c1570038cd79a7ed40cf8d984560e819c03a901a14b43ecd099ebe
fn documentation_text(workspace: &Workspace, node: NodeRef) -> Option<String> {
    let tree_node = workspace.model.node(node);
    if tree_node.documentation.is_empty() {
        return None;
    }
    let text = tree_node
        .documentation
        .iter()
        .map(|documentation| documentation_of(workspace, node.file, documentation))
        .collect::<Vec<_>>()
        .join("\n");
    // Decision: trailing whitespace of the joined text is dropped, so a block comment
    // whose closer sits on its own line adds no empty last line.
    Some(text.trim_end().to_string())
}

/// How a type is spelled wherever a query shows one: [`model::type_text_of`] over the
/// workspace's model.
pub fn type_text_of(workspace: &Workspace, ty: Option<&TypeRef>) -> Option<String> {
    model::type_text_of(&workspace.model, ty)
}

/// The entity that lists a type parameter or declares a member: the current entity of the
/// scope the symbol was declared in, which is the declaration that owns that scope, and so
/// both the declaration whose `TypeParameters` lists a type parameter and the `Member.owner`
/// of a member.
// Decision: an enum member is of kind `enumMember` and not the `member` the criterion
// names, so it reports no owner.
// @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:56038dc91ecf65d2bdfe79a9075ea17d361e3556abf2f01d835c0433eb98e148
// @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:673a55a8b3f334c8adcc6e3e97b7ecca3391634769fcc6eb8e0ad53d9410173d
fn owner_of(model: &Model, entity: EntityId) -> Option<EntityId> {
    let symbol = model.entities[entity].symbol?;
    let symbol = &model.symbols[symbol];
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:9a90fed14cf9b11333b26fd62edb533825a9c1f138ff89d05b998ceb6f939143
    if !matches!(symbol.kind, SymbolKind::TypeParameter | SymbolKind::Member) {
        return None;
    }
    let owner = model.scopes[symbol.scope].current;
    (owner != entity).then_some(owner)
}

/// Everything shown for one entity.
///
/// The range covers the identifier of the entity's declaration; the kind is that of the
/// entity's symbol (`member` for a member); the identifier is `Entity.identifier`, or the
/// first line of the declaring node when anonymous; the definition has every template
/// reference replaced by the referenced identifier; the type is the identifier of
/// `Entity.type` or its source text; the documentation is the joined text of the
/// documented nodes without their openers, closers, and one leading space per line; the
/// criteria are `criteriaOf` the entity; the traits are the identifier of each of
/// `Entity.traits` in application order, so a native data or fn shows `builtin` among
/// them; the owner is the identifier of the declaration whose `TypeParameters` lists a type
/// parameter, or the owner of a member, and `None` for anything else. A member of a kind
/// data or of a base data, reached after the context accessor or a value accessor, hovers
/// as any member: kind `member`, its range in the library file that declares it, and its
/// own definition and type. A type parameter hovers with kind `typeParameter`, the type
/// after its `extends` as its type, and the declaration that lists it as its owner.
// @lfy def/query/main.lfy:hoverOf
pub fn hover_of(workspace: &Workspace, entity: EntityId) -> Hover {
    let model = &workspace.model;
    let e = &model.entities[entity];
    let node = e.node.filter(|&node| is_real(model, node));
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:e17c5067e38fa0a3958c2ef3e01e30d2690690a6415be9560637c938889a02e7
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:8702d699e9d98e8b48b5e35e9f92a249f0779ab0592fc93e1c6033aa4ef467b4
    let identifier = e.identifier.clone().unwrap_or_else(|| {
        node.map(|node| {
            model
                .raw(node)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_default()
    });
    // A type parameter carries no definition, no trait, and no criterion of its own, so
    // the reads below give it nothing without a case for it.
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:68443ea655344459d78515fc060fa700a4e3923b44ec5fcacaa2b2632269927b
    Hover {
        range: identifier_range(workspace, entity),
        kind: kind_of(model, entity),
        identifier,
        // Decision: the binder stores the definition with references as written, so they
        // are stripped to the referenced identifier here, as `criteriaOf` does.
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:4a98847468c1084b02aa57083c43bdc60b9d9e7865c620f5be58c38401bf60a6
        definition: e.definition.as_deref().map(model::strip_references),
        // A type parameter's type is the type after its `extends`, so the same read gives
        // its constraint, and gives nothing when it has none.
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:b129f2741071e023568075a9414b9dd4815aabc5bccf81086fac8d19d877cad9
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:82d2ed0d94372e34fc7af9c288c152a8c2e12fa57a24bd774665c903f95c074c
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:8f5ab872cd3e21284ba6d3d9a3915ba9d3d37b663fc27ec21b270e938e750264
        ty: type_text_of(workspace, e.ty.as_ref()),
        owner: owner_of(model, entity).and_then(|owner| model.entities[owner].identifier.clone()),
        // A trait applied anonymously names nothing and is left out; a native data or fn
        // carries `builtin` among the traits applied to it, so it is named like any other.
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:dfdefece09ed47c4455f9a9339a661a18315a74bfb64bbdcb6a5ea78e063ca92
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:4304745b8a5494523a7d4b59e6de617ed045803bcc0b42a6b87243e2b2ad5f64
        traits: e
            .traits
            .iter()
            .filter_map(|applied| model.entities[applied.entity].identifier.clone())
            .collect(),
        documentation: node.and_then(|node| documentation_text(workspace, node)),
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:1d1e85c7ce3a70099ea62a09272e6712cbaa4302d4f684ea6effb157a8fa308b
        criteria: model::criteria_of(model, entity),
    }
}

/// Everything shown for the symbol at a position: [`hover_of`] the entity of
/// [`symbol_at`].
// @lfy def/query/main.lfy:hoverAt
pub fn hover_at(workspace: &Workspace, file: &str, position: Position) -> Option<Hover> {
    // @lfy def/query/main.lfy:hoverAt#hoverAt:hoverAt:6bdb2ad37511ee384dca32128cdffedcf3b25ed2de7c71a9df0f53486693c122
    let symbol = symbol_at(workspace, file, position)?;
    let entity = workspace.model.symbols[symbol].entity;
    // @lfy def/query/main.lfy:hoverAt#hoverAt:hoverAt:70e660c693b1e26397c1dd93f63dd693d1ece7542346f60692580c4bebe602d3
    Some(hover_of(workspace, entity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::tests::*;

    // @lfy def/query/main.lfy:hoverAt#hoverAt:hoverAt:70e660c693b1e26397c1dd93f63dd693d1ece7542346f60692580c4bebe602d3
    // @lfy def/query/main.lfy:hoverAt#hoverAt:hoverAt:6bdb2ad37511ee384dca32128cdffedcf3b25ed2de7c71a9df0f53486693c122
    // @lfy def/query/main.lfy:hoverAt#hoverAt:hoverAt:6984b8eab7d425099b0d1954eacb86b6197332bbbd6525ce36c2f5139b828170
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:bc58dcd08b279e454ea3f234d6633930816ba962b653ce118f411ca600d35815
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:e17c5067e38fa0a3958c2ef3e01e30d2690690a6415be9560637c938889a02e7
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:4a98847468c1084b02aa57083c43bdc60b9d9e7865c620f5be58c38401bf60a6
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:b395bb2363c1570038cd79a7ed40cf8d984560e819c03a901a14b43ecd099ebe
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:dfdefece09ed47c4455f9a9339a661a18315a74bfb64bbdcb6a5ea78e063ca92
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:1d1e85c7ce3a70099ea62a09272e6712cbaa4302d4f684ea6effb157a8fa308b
    #[test]
    fn hover_shows_kind_identifier_definition_and_documentation() {
        let text = "/// Doc\nd A: `An A` {} const y = A;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let hover = hover_at(&ws, A, find_pos(text, "A;", 0)).unwrap();
        assert_eq!(hover.kind, SymbolKind::Data);
        assert_eq!(hover.identifier, "A");
        assert_eq!(hover.definition.as_deref(), Some("An A"));
        assert_eq!(hover.documentation.as_deref(), Some("Doc"));
        assert!(hover.criteria.is_empty());
        assert!(hover.traits.is_empty());
        // The range covers the identifier of the declaration, not the token hovered.
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:a7d65e3cb8822c1288b11d4be9d75dc74c04674819b25044dfd48fec1f1a86e3
        assert_eq!(hover.range, range(A, (2, 2), (2, 3)));
        assert_eq!(hover.owner, None);
        assert_eq!(hover_at(&ws, A, at(2, 12)), None);
    }

    // @lfy def/query/main.lfy:hoverAt#hoverAt:hoverAt:ec108f7d5cd886ee18c6ce6aac56ce50dab60fa374bf7af37fd04989b559a98d
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:56038dc91ecf65d2bdfe79a9075ea17d361e3556abf2f01d835c0433eb98e148
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:68443ea655344459d78515fc060fa700a4e3923b44ec5fcacaa2b2632269927b
    #[test]
    fn a_type_parameter_hovers_with_its_constraint_and_the_declaration_that_lists_it() {
        let text = "d Item {} d Box<T extends Item> { $item: `d` = T; }";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert!(ws.problems.is_empty(), "{:?}", ws.problems);
        let hover = hover_at(&ws, A, find_pos(text, "T;", 0)).unwrap();
        assert_eq!(hover.kind, SymbolKind::TypeParameter);
        assert_eq!(hover.identifier, "T");
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:82d2ed0d94372e34fc7af9c288c152a8c2e12fa57a24bd774665c903f95c074c
        assert_eq!(hover.ty.as_deref(), Some("Item"));
        assert_eq!(hover.owner.as_deref(), Some("Box"));
        assert_eq!(hover.definition, None);
        assert!(hover.traits.is_empty());
        assert!(hover.criteria.is_empty());
        // The declaration itself, in the `TypeParameters` that lists it, hovers alike.
        assert_eq!(hover_at(&ws, A, find_pos(text, "T extends", 0)), Some(hover));
        // A type parameter without a constraint has no type to show.
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:8f5ab872cd3e21284ba6d3d9a3915ba9d3d37b663fc27ec21b270e938e750264
        let text = "d Box<U> { $item: `d` = U; }";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let hover = hover_at(&ws, A, find_pos(text, "U;", 0)).unwrap();
        assert_eq!(hover.kind, SymbolKind::TypeParameter);
        assert_eq!(hover.ty, None);
        assert_eq!(hover.owner.as_deref(), Some("Box"));
    }

    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:673a55a8b3f334c8adcc6e3e97b7ecca3391634769fcc6eb8e0ad53d9410173d
    #[test]
    fn a_member_names_the_declaration_that_declares_it_as_its_owner() {
        let text = "trait t { $m = string; }\nd A is t { $n = string; }\nconst v = 1;\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert_eq!(hover_of(&ws, find(&ws, "A.n")[0]).owner.as_deref(), Some("A"));
        // A member a trait adds is owned by the entity it was reached through.
        assert_eq!(hover_of(&ws, find(&ws, "t.m")[0]).owner.as_deref(), Some("t"));
        // Anything else reports no owner.
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:9a90fed14cf9b11333b26fd62edb533825a9c1f138ff89d05b998ceb6f939143
        assert_eq!(hover_of(&ws, find(&ws, "v")[0]).owner, None);
        assert_eq!(hover_of(&ws, find(&ws, "A")[0]).owner, None);
    }

    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:bc5317180748f756ff3ce1dc261bcaa95143513bb4264535b9448d06ee539dd5
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:8702d699e9d98e8b48b5e35e9f92a249f0779ab0592fc93e1c6033aa4ef467b4
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:4a98847468c1084b02aa57083c43bdc60b9d9e7865c620f5be58c38401bf60a6
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:b129f2741071e023568075a9414b9dd4815aabc5bccf81086fac8d19d877cad9
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:b395bb2363c1570038cd79a7ed40cf8d984560e819c03a901a14b43ecd099ebe
    #[test]
    fn hover_of_resolves_references_types_members_and_block_documentation() {
        let text = "/** Block\n doc **/\ntrait t { $m: `Of [[A]]` = string; }\nd A is t {}\nconst v: A = A;\nconst w = 1;\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let model = &ws.model;
        assert!(ws.problems.is_empty(), "{:?}", ws.problems);
        let t = find(&ws, "t")[0];
        let hover = hover_of(&ws, t);
        assert_eq!(hover.kind, SymbolKind::Trait);
        assert_eq!(hover.documentation.as_deref(), Some("Block\ndoc"));
        // The identifier of the declaration, documentation excluded.
        // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:a7d65e3cb8822c1288b11d4be9d75dc74c04674819b25044dfd48fec1f1a86e3
        assert_eq!(hover.range, range(A, (3, 6), (3, 7)));
        let m = find(&ws, "t.m")[0];
        let hover = hover_of(&ws, m);
        assert_eq!(hover.kind, SymbolKind::Member);
        assert_eq!(hover.identifier, "m");
        assert_eq!(hover.definition.as_deref(), Some("Of A"));
        assert_eq!(hover.ty.as_deref(), Some("string"));
        assert_eq!(hover.documentation, None);
        assert_eq!(hover.range, range(A, (3, 11), (3, 12)));
        let v = find(&ws, "v")[0];
        let hover = hover_of(&ws, v);
        assert_eq!(hover.kind, SymbolKind::Variable);
        assert_eq!(hover.ty.as_deref(), Some("A"));
        assert_eq!(hover.definition, None);
        // The anonymous entity of the file is named by its first line, and stands for
        // the module the file is.
        let hover = hover_of(&ws, model.file_entities[0]);
        assert_eq!(hover.kind, SymbolKind::Module);
        assert_eq!(hover.identifier, "/** Block");
        assert_eq!(hover.range.start, at(1, 0));
        // The global entity has no node to read, so it names no file.
        let hover = hover_of(&ws, model.global);
        assert_eq!(hover.identifier, "global");
        assert_eq!(hover.documentation, None);
        assert_eq!(hover.range, Range::empty("", at(1, 0)));
    }

    /// [`type_text_of`] the type of the entity a name spells in a source.
    fn type_text_at(workspace: &Workspace, name: &str) -> Option<String> {
        let entity = find(workspace, name);
        assert_eq!(entity.len(), 1, "{name} names one entity");
        let ty = workspace.model.entities[entity[0]].ty.clone();
        type_text_of(workspace, ty.as_ref())
    }

    /// [`type_text_of`] the type of the one entity a name is bound to anywhere in the
    /// program: a parameter of a function type declares no symbol of a file scope.
    fn type_text_of_symbol(workspace: &Workspace, name: &str) -> Option<String> {
        let found: Vec<EntityId> = workspace
            .model
            .symbols
            .iter()
            .filter(|symbol| symbol.name == name)
            .map(|symbol| symbol.entity)
            .collect();
        assert_eq!(found.len(), 1, "{name} names one symbol");
        let ty = workspace.model.entities[found[0]].ty.clone();
        type_text_of(workspace, ty.as_ref())
    }

    // How a type is spelled is `def/model/main.lfy`'s answer; these cover reading it for
    // what a hover shows.
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:b129f2741071e023568075a9414b9dd4815aabc5bccf81086fac8d19d877cad9
    #[test]
    fn a_generic_is_spelled_with_its_arguments_and_a_list_with_its_brackets() {
        let fixture = Fixture::one("d Box<T> {} const b = Box<string>;");
        let ws = fixture.load();
        assert_eq!(type_text_at(&ws, "b").as_deref(), Some("Box<string>"));
        // Nothing to spell.
        assert_eq!(type_text_of(&ws, None), None);
        // A declaration and a primitive are spelled by their identifier.
        let fixture = Fixture::one("d A {}\nconst v: A = A;\nconst w: string = \"x\";\n");
        let ws = fixture.load();
        assert_eq!(type_text_at(&ws, "v").as_deref(), Some("A"));
        assert_eq!(type_text_at(&ws, "w").as_deref(), Some("string"));
        // A list is the item then square brackets, `List<T>` and `T[]` alike, and a
        // function type or a union is parenthesized first.
        let fixture = Fixture::with_prelude(
            "d Box<T> {\n  $items: `d` = T[];\n  $listed: `d` = List<T>;\n  $pairs: `d` = (A | B)[];\n}\nd A {}\nd B {}\ntype S<U> { calls = ((item: U) => U)[] }\n",
        );
        let ws = fixture.load();
        assert_eq!(type_text_at(&ws, "Box.items").as_deref(), Some("T[]"));
        assert_eq!(type_text_at(&ws, "Box.listed").as_deref(), Some("T[]"));
        assert_eq!(type_text_at(&ws, "Box.pairs").as_deref(), Some("(A | B)[]"));
        assert_eq!(
            type_text_at(&ws, "S.calls").as_deref(),
            Some("((item: U) => U)[]")
        );
    }

    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:b129f2741071e023568075a9414b9dd4815aabc5bccf81086fac8d19d877cad9
    #[test]
    fn a_function_type_is_spelled_with_its_parameters_and_its_output() {
        let text =
            "d Box<T> { $items: `d` = T[]; $map: `e` = (transform: (item: T) => T) => T[]; }\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        // The type of the parameter `transform`, which is a function type written in a
        // type position.
        assert_eq!(
            type_text_of_symbol(&ws, "transform").as_deref(),
            Some("(item: T) => T")
        );
        assert_eq!(type_text_at(&ws, "Box.items").as_deref(), Some("T[]"));
        // An optional parameter takes a question mark after its name and a spread one
        // three dots before it.
        let text = "d A {}\ntype S { f = (one: string, two?: number, ...rest: string[]) => A }\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert_eq!(
            type_text_at(&ws, "S.f").as_deref(),
            Some("(one: string, two?: number, ...rest: string[]) => A")
        );
    }

    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:b129f2741071e023568075a9414b9dd4815aabc5bccf81086fac8d19d877cad9
    #[test]
    fn a_member_read_on_a_generic_left_side_spells_the_substituted_type() {
        let text = "d Box<T> { $item: `d` = T; }\nconst b: Box<string> = 1;\nconst c = b.item;\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        // `item` of a Box of string spells string, as the binder substitutes it.
        assert_eq!(type_text_at(&ws, "c").as_deref(), Some("string"));
        assert_eq!(type_text_at(&ws, "Box.item").as_deref(), Some("T"));
    }

    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:dfdefece09ed47c4455f9a9339a661a18315a74bfb64bbdcb6a5ea78e063ca92
    // @lfy def/query/main.lfy:hoverOf#hoverOf:hoverOf:4304745b8a5494523a7d4b59e6de617ed045803bcc0b42a6b87243e2b2ad5f64
    // @lfy def/query/main.lfy:hoverAt#hoverAt:hoverAt:3ac47bdd611b3d3ce886a3bbc4f4ffc93ade4c2a78e8e071b192985bdbddfedb
    #[test]
    fn hover_names_the_traits_and_shows_a_member_of_a_kind_data_as_a_member() {
        let text =
            "trait t {}\nd A is t {}\nfn f(): `d` => number { @acceptanceCriteria.add({ behavior = `b` }); }\nconst y = A@definition;\n";
        let fixture = Fixture::with_prelude(text);
        let ws = fixture.load();
        // Every trait applied, in application order.
        assert_eq!(hover_of(&ws, find(&ws, "A")[0]).traits, ["t"]);
        // A native data shows `builtin` among them.
        let string = find(&ws, "String");
        assert_eq!(string.len(), 1);
        assert_eq!(hover_of(&ws, string[0]).traits, ["builtin"]);
        // A member of a kind data, reached after the context accessor, hovers as any
        // member: its range is in the library file that declares it.
        let position = after(find_pos(text, "@acceptanceCriteria", 0), 1);
        let hover = hover_at(&ws, A, position).unwrap();
        assert_eq!(hover.kind, SymbolKind::Member);
        assert_eq!(hover.identifier, "acceptanceCriteria");
        assert_eq!(hover.range.file, "lib/prelude/entity.lfy");
        assert_eq!(
            hover.definition.as_deref(),
            Some("Its own criteria first, then each trait's in application order")
        );
        // The owner is the kind data the member is declared in, and the definition and the
        // type are the member's own.
        let position = after(find_pos(text, "A@definition", 0), 2);
        let hover = hover_at(&ws, A, position).unwrap();
        assert_eq!(hover.kind, SymbolKind::Member);
        assert_eq!(hover.identifier, "definition");
        assert_eq!(hover.owner.as_deref(), Some("Entity"));
        assert_eq!(hover.range.file, "lib/prelude/entity.lfy");
        assert_eq!(
            hover.definition.as_deref(),
            Some("The description provided for it; undefined when there is none")
        );
        let ty = hover.ty.expect("the member declares a type");
        assert!(ty.contains("string"), "{ty}");
    }
}
