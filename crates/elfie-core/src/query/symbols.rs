// @lfy def/query/main.lfy:symbolAt
//! The symbol a position names, where it is declared, and everywhere it is used.

use super::*;
use crate::grammar::GrammarRule;
use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::Statement as S;
use crate::grammar::terminals::literal::Literal as L;
use crate::model::Usage;

/// Every node below a node, in preorder, the node itself excluded.
fn descendants(model: &Model, node: NodeRef) -> Vec<NodeRef> {
    let infos = &model.nodes[node.file];
    let mut out = Vec::new();
    let mut index = node.index + 1;
    while index < infos.len() {
        let mut ancestor = infos[index].parent;
        let mut inside = false;
        while let Some(a) = ancestor {
            if a == node.index {
                inside = true;
                break;
            }
            if a < node.index {
                break;
            }
            ancestor = infos[a].parent;
        }
        if !inside {
            break;
        }
        out.push(NodeRef {
            file: node.file,
            index,
        });
        index += 1;
    }
    out
}

/// The usage a `TemplateReference` makes: the first usage of a node inside its
/// `Reference`.
// @lfy def/query/main.lfy:symbolAt
fn reference_usage(model: &Model, reference: NodeRef) -> Option<model::UsageId> {
    descendants(model, reference)
        .into_iter()
        .find_map(|node| model.usage_of(node))
}

/// The symbol declared or used at a position.
///
/// On an identifier, or a keyword standing as a member name: walking [`nodes_at`]
/// innermost outward, the first node for which `resolve` gives a symbol whose name the
/// token spells. Inside a `TemplateReference`, in documentation included: the usage of
/// its `Reference`, so references in prose resolve like code. On the string of a `Use`:
/// the module symbol the `Use` declares, or `None` when it has no `as`. Anything else
/// gives `None`.
// @lfy def/query/main.lfy:symbolAt
pub fn symbol_at(workspace: &Workspace, file: &str, position: Position) -> Option<SymbolId> {
    let model = &workspace.model;
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:e17a6e8642233d650e5689114a64b1606e2e9339c72579bfeb326988cf814a67
    let id = file_of(workspace, file)?;
    let index = token_at(workspace, file, position)?;
    let token = &tokens(workspace, id)[index];
    let chain = nodes_covering(model, id, index);
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:562b82e9b9bff42506308fce309460fd485f976fa4cbcd14307d4e86255c5b7a
    if is_name_token(token) {
        for &node in chain.iter().rev() {
            if let Some(symbol) = model::resolve(model, node)
                && model.symbols[symbol].name == token.value
            {
                return Some(symbol);
            }
        }
        // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:dbff44bf75c2725e356cc8d7779b5bb7fca8b96a864bccca93bbd041ae42089b
    }
    // Decision: inside a `TemplateReference` a token that spells a name resolves through
    // that name above, which is the symbol the `Reference`'s usage names for the last link
    // of the chain; the `Reference`'s own usage answers for every other token of it, the
    // brackets included.
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:78170c70506af39a6039bdbd84fe4c788c9351da0dce3aaa661428193f5d1898
    if let Some(reference) = find_rule(model, &chain, E::TemplateReference.entity()) {
        return reference_usage(model, reference).and_then(|usage| model.usages[usage].symbol);
    }
    // A `Use` with `as` declares a module symbol on its node; one without declares none.
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:41ae04c14a30017ad0abb3f1d0bc7898f0866717f4ddc33d718def45735bd547
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:aacbb994d4240562ec9a18e9bc945afb3ba36f42eb9271d196818dc04a8b921f
    if (token.is(L::DoubleQuoteBody) || token.is(L::SingleQuoteBody))
        && let Some(use_node) = find_rule(model, &chain, S::Use.entity())
    {
        return model.symbol_of(use_node);
    }
    None // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:53c3f76e6e388625c64768f5da7efb65e1d35ae56598a8719f63340432d2cb8f
}

/// The file a `Use` resolved to in `Source.uses`, when it resolved.
fn use_target(workspace: &Workspace, use_node: NodeRef) -> Option<String> {
    let model = &workspace.model;
    // `Source.uses` lists the uses in the order they appear in the tree.
    let position = model.nodes[use_node.file]
        .iter()
        .enumerate()
        .filter(|(_, info)| info.rule == S::Use.entity())
        .position(|(index, _)| index == use_node.index)?;
    model.sources[use_node.file].uses.get(position)?.clone()
}

/// The range that declares a symbol: the identifier that spells its name, or an empty
/// range at line 1, column 0 of the used file for a module symbol.
// @lfy def/query/main.lfy:definitionOf
fn declaration_range(workspace: &Workspace, symbol: SymbolId) -> Option<Range> {
    let model = &workspace.model;
    let symbol = &model.symbols[symbol];
    if !is_real(model, symbol.node) {
        return None;
    }
    if symbol.kind == SymbolKind::Module {
        // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:e6e79e89eb5fadea6af0670f3df3a4bdaca711ae684b48103d8ba63af49f5424
        if let Some(path) = use_target(workspace, symbol.node) {
            return Some(Range::empty(&path, Position::new(1, 0)));
        }
        // Decision: a module symbol whose `Use` resolved to nothing is declared where its
        // name is written.
    }
    // The symbol's own node, at the token `namedBy` picked out: an alias declares itself,
    // not what it aliases, and a member added by a trait declares the member statement
    // inside the trait body.
    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:e38e58f0b0d00cd0e0b9b3322d0044fe9860e3725299a137a672be2af4bd7846
    Some(match symbol.name_token {
        Some(index) => span(workspace, symbol.node.file, index, index + 1),
        None => range_of_node(workspace, symbol.node),
    })
}

/// Where the symbol at a position is declared.
///
/// The identifier that spells the name in the declaring node of [`symbol_at`]; for a
/// module symbol, or on the path of a `Use`, an empty range at line 1, column 0 of the
/// file the `Use` resolved to. An alias gives its own declaration, not what it aliases; a
/// member added by a trait gives the member statement inside the trait body.
// @lfy def/query/main.lfy:definitionOf
pub fn definition_of(workspace: &Workspace, file: &str, position: Position) -> Option<Range> {
    let model = &workspace.model;
    let id = file_of(workspace, file)?;
    if let Some(index) = token_at(workspace, file, position) {
        let chain = nodes_covering(model, id, index);
        // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:b8a357e2e09546439bffb53ccb10fe981d92ef6a0575a7833fa222b41605e5f4
        if find_rule(model, &chain, E::StringLiteral.entity()).is_some()
            && let Some(use_node) = find_rule(model, &chain, S::Use.entity())
        {
            return use_target(workspace, use_node)
                .map(|path| Range::empty(&path, Position::new(1, 0)));
        }
    }
    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:9c40717bde75a51a33de54915878526cf7d8e7dc76b73905b6e9ad477a8f3cd4
    let symbol = symbol_at(workspace, file, position)?;
    declaration_range(workspace, symbol)
}

/// The range of a usage: the token that spells the name, or the accessor when the usage
/// is of a layer itself, or the whole node when it has neither.
// @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:4b4cb5cbd24fba6b7a4bf778a6cbde6cae871fe06a7dbd8de7d54dacd7f2a812
// @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:0bee52b15172b8d5f4b423f6f46e68a0f8502383517282bb98b3038f2ce4311c
fn usage_range(workspace: &Workspace, usage: &Usage) -> Range {
    match usage.token {
        Some(index) => span(workspace, usage.node.file, index, index + 1),
        None => range_of_node(workspace, usage.node),
    }
}

/// Sorts ranges into `Workspace.files` order, then by start.
fn sort_ranges(workspace: &Workspace, ranges: &mut [Range]) {
    ranges.sort_by(|a, b| {
        (file_order(workspace, &a.file), &a.file, a.start, a.end).cmp(&(
            file_order(workspace, &b.file),
            &b.file,
            b.start,
            b.end,
        ))
    });
}

/// Every place the symbol at a position is used.
///
/// One range per usage of the symbol, covering the token that spells the name, or the
/// accessor when the usage is of a layer itself, in `Workspace.files` order then by
/// start; with `include_declaration`, the range [`definition_of`] gives comes first.
/// Empty when [`symbol_at`] gives nothing.
// @lfy def/query/main.lfy:referencesTo
pub fn references_to(
    workspace: &Workspace,
    file: &str,
    position: Position,
    include_declaration: bool,
) -> Vec<Range> {
    let model = &workspace.model;
    let Some(symbol) = symbol_at(workspace, file, position) else {
        return Vec::new(); // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:a76991681fd16dd670c4a0f8feca6a55b8576d4dea15b95158f8f338dd7c3f7c
    };
    let mut usages: Vec<Range> = usages_of_declaration(model, symbol)
        .into_iter()
        .map(|usage| usage_range(workspace, &model.usages[usage]))
        .collect();
    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:2c61bf4cf3dcd2797fdc448c45a78d5542297f5ccda29af95840a2b68b5f011a
    sort_ranges(workspace, &mut usages);
    usages.dedup();
    let mut out = Vec::new();
    if include_declaration {
        // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:0ab0b8084ac90ec4aeffe6eaac88a0382e784b5ba6c6cbbbce73857664f7ab26
        if let Some(declaration) = declaration_range(workspace, symbol) {
            out.push(declaration);
        }
    }
    for range in usages {
        if !out.contains(&range) {
            out.push(range);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::terminals::identifier::Identifier as I;
    use crate::query::tests::*;

    // The ranges every query answers with come from [`Range::of_tokens`], which
    // `def/query/data.lfy` answers for; these cover the wrappers over it.
    // @lfy def/query/main.lfy:definitionOf
    #[test]
    fn a_range_runs_from_the_first_token_to_just_after_the_last() {
        let fixture = Fixture::one("/** Block\n doc **/\nd A {}\n");
        let ws = fixture.load();
        let id = ws.model.file(A).unwrap();
        // A token holding a line break ends on the next line.
        assert_eq!(
            range_of(&ws, id, NodeOrToken::Token(1)),
            range(A, (1, 3), (2, 5))
        );
        assert_eq!(
            range_of(&ws, id, NodeOrToken::Token(0)),
            range(A, (1, 0), (1, 3))
        );
        let root = &ws.model.sources[id].tree.root;
        assert_eq!(
            range_of(&ws, id, NodeOrToken::Node(root)),
            range(A, (1, 0), (4, 0))
        );
        let declaration = root.find(S::DataDeclaration).unwrap();
        assert_eq!(
            range_of(&ws, id, NodeOrToken::Node(declaration)),
            range(A, (3, 0), (3, 6))
        );
        let r = ws.model.node_ref(id, declaration).unwrap();
        assert_eq!(range_of_node(&ws, r), range(A, (3, 0), (3, 6)));
    }

    // @lfy def/query/main.lfy:definitionOf
    #[test]
    fn a_node_covering_no_token_gives_an_empty_range_where_it_would_start() {
        let fixture = Fixture::one("");
        let ws = fixture.load();
        let id = ws.model.file(A).unwrap();
        let root = &ws.model.sources[id].tree.root;
        assert_eq!(root.start, root.end);
        assert_eq!(
            range_of(&ws, id, NodeOrToken::Node(root)),
            Range::empty(A, at(1, 0))
        );
        // An empty error node at the end of the file sits after the last token.
        let fixture = Fixture::one("const y = A@");
        let ws = fixture.load();
        let id = ws.model.file(A).unwrap();
        let error = &ws.model.sources[id].tree.errors[0];
        assert_eq!(error.start, error.end);
        assert_eq!(
            span(&ws, id, error.start, error.end),
            Range::empty(A, at(1, 12))
        );
    }

    // Which token a position belongs to is what `def/query/data.lfy` answers for; this
    // covers reading it through a file of the program, as every query does.
    // @lfy def/query/main.lfy:symbolAt
    #[test]
    fn the_token_just_after_an_identifier_still_belongs_to_it() {
        let fixture = Fixture::one("const x = 1;");
        let ws = fixture.load();
        let index = token_at(&ws, A, at(1, 7)).unwrap();
        assert_eq!(token(&ws, 0, index).raw, "x");
        assert!(token(&ws, 0, index).is(I::Identifier));
        assert_eq!(token_at(&ws, A, at(2, 0)), None);
        assert_eq!(token_at(&ws, A, at(1, 13)), None);
        assert_eq!(token_at(&ws, "def/none.lfy", at(1, 0)), None);
        // Inside a token, and after a keyword.
        assert_eq!(
            token(&ws, 0, token_at(&ws, A, at(1, 2)).unwrap()).raw,
            "const"
        );
        assert_eq!(
            token(&ws, 0, token_at(&ws, A, at(1, 5)).unwrap()).raw,
            "const"
        );
        // Between a space and a setter the later token wins.
        assert_eq!(token(&ws, 0, token_at(&ws, A, at(1, 8)).unwrap()).raw, "=");
        // Just after a number the later token wins; at the very end nothing does.
        assert_eq!(token(&ws, 0, token_at(&ws, A, at(1, 11)).unwrap()).raw, ";");
        assert_eq!(token_at(&ws, A, at(1, 12)), None);
        // The end of the file after an identifier.
        let fixture = Fixture::one("const x = y");
        let ws = fixture.load();
        assert_eq!(token(&ws, 0, token_at(&ws, A, at(1, 11)).unwrap()).raw, "y");
    }

    // The chain of nodes containing a position, which every lookup walks.
    // @lfy def/query/main.lfy:symbolAt
    #[test]
    fn nodes_at_walks_from_the_root_down_to_the_node_holding_the_token() {
        let fixture = Fixture::one("d A {} const y: `See [[A]]` = 1;");
        let ws = fixture.load();
        let model = &ws.model;
        let position = find_pos("d A {} const y: `See [[A]]` = 1;", "A]]", 0);
        let nodes = nodes_at(&ws, A, position);
        let rules: Vec<&str> = nodes
            .iter()
            .map(|&r| model.info(r).rule.identifier())
            .collect();
        assert_eq!(
            rules,
            [
                "SourceFile",
                "VariableDeclaration",
                "Declared",
                "DefinitionClause",
                "TypeExpression",
                "TypeItem",
                "Template",
                "TemplateReference",
                "Reference",
                "Name"
            ]
        );
        assert_eq!(nodes[0], NodeRef { file: 0, index: 0 });
        for pair in nodes.windows(2) {
            assert_eq!(model.parent(pair[1]), Some(pair[0]));
        }
        let token = token_at(&ws, A, position).unwrap();
        let last = model.node(*nodes.last().unwrap());
        assert!(
            last.children
                .iter()
                .any(|child| child.as_token() == Some(token))
        );
        assert!(nodes_at(&ws, A, at(9, 0)).is_empty());
    }

    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:562b82e9b9bff42506308fce309460fd485f976fa4cbcd14307d4e86255c5b7a
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:dbff44bf75c2725e356cc8d7779b5bb7fca8b96a864bccca93bbd041ae42089b
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:78170c70506af39a6039bdbd84fe4c788c9351da0dce3aaa661428193f5d1898
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:53c3f76e6e388625c64768f5da7efb65e1d35ae56598a8719f63340432d2cb8f
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:e17a6e8642233d650e5689114a64b1606e2e9339c72579bfeb326988cf814a67
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:49299b3da7368ca5d187315c1871b87eaa56e549cd0d213bef1c20dd647a9f1f
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:cce90b97f1e0ec33c1c384d68d67da790e0540d434d0d71fd61bff8c5326e014
    #[test]
    fn a_name_in_code_and_in_a_template_reference_resolve_alike() {
        let text = "d A {} const y = A;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let symbol = symbol_at(&ws, A, find_pos(text, "A;", 0)).unwrap();
        assert_eq!(ws.model.symbols[symbol].name, "A");
        assert_eq!(ws.model.symbols[symbol].kind, SymbolKind::Data);
        // The declaration itself.
        assert_eq!(symbol_at(&ws, A, at(1, 2)), Some(symbol));
        assert_eq!(
            symbol_at(&ws, A, find_pos(text, "y", 0)).map(|s| ws.model.symbols[s].name.clone()),
            Some("y".to_string())
        );
        // Anything else: a keyword that stands for no member name, punctuation, a name
        // that resolves to nothing, a file not in the program, and a position past the end.
        assert_eq!(symbol_at(&ws, A, find_pos(text, "const", 0)), None);
        assert_eq!(symbol_at(&ws, A, find_pos(text, "=", 0)), None);
        assert_eq!(symbol_at(&ws, "def/none.lfy", at(1, 2)), None);
        assert_eq!(symbol_at(&ws, A, at(9, 0)), None);
        let unresolved = Fixture::one("const y = zz;");
        let unresolved = unresolved.load();
        assert_eq!(
            symbol_at(&unresolved, A, find_pos("const y = zz;", "zz", 0)),
            None
        );

        let text = "d A {} const y: `See [[A]]` = 1;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let symbol = symbol_at(&ws, A, find_pos(text, "A]]", 0)).unwrap();
        assert_eq!(ws.model.symbols[symbol].name, "A");
        assert_eq!(ws.model.symbols[symbol].kind, SymbolKind::Data);
        // On the reference's brackets the reference's usage still gives the symbol.
        assert_eq!(symbol_at(&ws, A, find_pos(text, "[[", 0)), Some(symbol));
        assert_eq!(symbol_at(&ws, A, find_pos(text, "]]", 0)), Some(symbol));
    }

    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:41ae04c14a30017ad0abb3f1d0bc7898f0866717f4ddc33d718def45735bd547
    // @lfy def/query/main.lfy:symbolAt#symbolAt:symbolAt:aacbb994d4240562ec9a18e9bc945afb3ba36f42eb9271d196818dc04a8b921f
    #[test]
    fn the_path_of_a_use_gives_its_module_symbol_or_nothing() {
        let fixture = Fixture::new();
        fixture
            .write(
                "def/a.lfy",
                "use \"./b\" as B;\nuse \"./b\";\nconst y = B.B;\n",
            )
            .write("def/b.lfy", "d B {}\n");
        let ws = fixture.load();
        let symbol = symbol_at(&ws, A, at(1, 6)).unwrap();
        assert_eq!(ws.model.symbols[symbol].kind, SymbolKind::Module);
        assert_eq!(ws.model.symbols[symbol].name, "B");
        assert_eq!(symbol_at(&ws, A, at(2, 6)), None);
        // The name after `as` and the module's member.
        assert_eq!(symbol_at(&ws, A, at(1, 13)), Some(symbol));
        assert_eq!(symbol_at(&ws, A, at(3, 10)), Some(symbol));
        let member = symbol_at(&ws, A, at(3, 12)).unwrap();
        assert_eq!(ws.model.symbols[member].kind, SymbolKind::Data);
        assert_eq!(
            ws.model.sources[ws.model.symbols[member].node.file].path,
            "def/b.lfy"
        );
    }

    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:b8a357e2e09546439bffb53ccb10fe981d92ef6a0575a7833fa222b41605e5f4
    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:e6e79e89eb5fadea6af0670f3df3a4bdaca711ae684b48103d8ba63af49f5424
    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:e38e58f0b0d00cd0e0b9b3322d0044fe9860e3725299a137a672be2af4bd7846
    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:9c40717bde75a51a33de54915878526cf7d8e7dc76b73905b6e9ad477a8f3cd4
    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:1acc1ae62556beb4a1a72c80817747fed9095c6ed395312fbabd21a70ef84587
    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:fc48bd9d077d476733e52532a4d1cdd62f70c9af747af748cb5831d6fb20ea0f
    // @lfy def/query/main.lfy:definitionOf#definitionOf:definitionOf:dd454fd2bc1144afd8c3fc163f46ba49f7d40c4132d9c7d3fb46f28e28a9a512
    #[test]
    fn definition_is_the_identifier_of_the_declaration() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\" as B;\nuse \"./b\";\nalias C = B.B;\nconst y = C;\ntrait t { $m = string; }\nd A is t { $n = A$m; }\n")
            .write("def/b.lfy", "d B {}\n");
        let ws = fixture.load();
        assert!(ws.problems.is_empty(), "{:?}", ws.problems);
        // A data declared in another file.
        assert_eq!(
            definition_of(&ws, A, at(3, 12)),
            Some(range("def/b.lfy", (1, 2), (1, 3)))
        );
        // The path of a Use, and a module symbol, give the used file.
        assert_eq!(
            definition_of(&ws, A, at(1, 6)),
            Some(Range::empty("def/b.lfy", at(1, 0)))
        );
        assert_eq!(
            definition_of(&ws, A, at(2, 6)),
            Some(Range::empty("def/b.lfy", at(1, 0)))
        );
        assert_eq!(
            definition_of(&ws, A, at(3, 10)),
            Some(Range::empty("def/b.lfy", at(1, 0)))
        );
        // An alias gives its own declaration.
        assert_eq!(
            definition_of(&ws, A, at(4, 10)),
            Some(range(A, (3, 6), (3, 7)))
        );
        // A member added by a trait gives the statement in the trait body.
        assert_eq!(
            definition_of(&ws, A, at(6, 18)),
            Some(range(A, (5, 11), (5, 12)))
        );
        // Nothing declared.
        assert_eq!(definition_of(&ws, A, at(4, 0)), None);
        assert_eq!(definition_of(&ws, A, at(20, 0)), None);
    }

    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:0ab0b8084ac90ec4aeffe6eaac88a0382e784b5ba6c6cbbbce73857664f7ab26
    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:2c61bf4cf3dcd2797fdc448c45a78d5542297f5ccda29af95840a2b68b5f011a
    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:4b4cb5cbd24fba6b7a4bf778a6cbde6cae871fe06a7dbd8de7d54dacd7f2a812
    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:a76991681fd16dd670c4a0f8feca6a55b8576d4dea15b95158f8f338dd7c3f7c
    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:cbe3d06fa6292bb59a461fcf4e5ab69c7f32270243475185c9b1c2c2c1fb247d
    #[test]
    fn references_cover_every_usage_in_file_order() {
        let fixture = Fixture::new();
        fixture
            .write(
                "def/a.lfy",
                "use \"./b\";\nconst y = B;\nconst z: `Is [[B]]` = B;\n",
            )
            .write("def/b.lfy", "d B {}\nconst x = B;\n");
        let ws = fixture.load();
        let with = references_to(&ws, A, at(2, 10), true);
        assert_eq!(
            with,
            [
                range("def/b.lfy", (1, 2), (1, 3)),
                range("def/b.lfy", (2, 10), (2, 11)),
                range(A, (2, 10), (2, 11)),
                range(A, (3, 15), (3, 16)),
                range(A, (3, 22), (3, 23)),
            ]
        );
        let without = references_to(&ws, A, at(2, 10), false);
        assert_eq!(without, with[1..]);
        assert!(references_to(&ws, A, at(2, 0), true).is_empty());
    }

    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:0ab0b8084ac90ec4aeffe6eaac88a0382e784b5ba6c6cbbbce73857664f7ab26
    #[test]
    fn references_to_a_trait_member_include_every_receiver() {
        let text = "trait t { $m = string; }\nd A is t { $n = A$m; $o = $m; }\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert!(ws.problems.is_empty(), "{:?}", ws.problems);
        let from_declaration = references_to(&ws, A, at(1, 11), true);
        assert_eq!(
            from_declaration,
            [
                range(A, (1, 11), (1, 12)),
                range(A, (2, 18), (2, 19)),
                range(A, (2, 27), (2, 28))
            ]
        );
        assert_eq!(references_to(&ws, A, at(2, 18), true), from_declaration);
    }

    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:0bee52b15172b8d5f4b423f6f46e68a0f8502383517282bb98b3038f2ce4311c
    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:896acc1412de79bd1efaa7c2301577ac6e91597598207315cfa32f7d6255ed31
    #[test]
    fn a_usage_of_a_layer_itself_is_ranged_at_its_accessor() {
        // `^^` spells no name after its accessor, so its usage is of the previous
        // layer itself and its range covers the accessor instead of a name token.
        let text = "d A {}\nconst y = ^^;\nconst z = A;\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert!(ws.problems.is_empty(), "{:?}", ws.problems);
        assert_eq!(
            references_to(&ws, A, at(1, 2), true),
            [
                range(A, (1, 2), (1, 3)),
                range(A, (2, 10), (2, 12)),
                range(A, (3, 10), (3, 11)),
            ]
        );
    }

    // @lfy def/query/main.lfy:referencesTo#referencesTo:referencesTo:136d2b4e3c707c64e8248b946aad777865a8c943fa97bf9d456692a28722f4f5
    #[test]
    fn references_to_a_data_include_the_usages_of_an_alias_of_it() {
        let fixture = Fixture::one("d A {}\nalias C = A;\nconst y = C;\n");
        let ws = fixture.load();
        assert!(ws.problems.is_empty(), "{:?}", ws.problems);
        assert_eq!(
            references_to(&ws, A, at(1, 2), true),
            [
                range(A, (1, 2), (1, 3)),
                range(A, (2, 10), (2, 11)),
                range(A, (3, 10), (3, 11)),
            ]
        );
    }
}
