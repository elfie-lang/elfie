// @lfy def/query/main.lfy:outlineOf
//! The declarations of one file, nested as written, and the same across the program.

use std::collections::HashSet;

use super::*;

/// Whether a symbol is listed in an outline: parameters, loop variables, and modules
/// are not.
// @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:3a7355b9e45ec3b575bc69fd4a7ce2bffafd0dcfd946c80bcf0c0783dc8c87be
fn is_outlined(kind: SymbolKind) -> bool {
    !matches!(
        kind,
        SymbolKind::Parameter | SymbolKind::LoopVariable | SymbolKind::Module
    )
}

/// The outline of one declared symbol: its range covers the declaring node and its
/// documentation, its selection range the identifier, and its children are the members
/// declared in its body, then every declaration nested in its body, in order.
// @lfy def/query/main.lfy:outlineOf
fn outline_of_symbol(workspace: &Workspace, symbol: SymbolId) -> Outline {
    let model = &workspace.model;
    let s = &model.symbols[symbol];
    let node = model.node(s.node);
    let start = node
        .documentation
        .first()
        .map_or(node.start, |documentation| {
            documentation.start.min(node.start)
        });
    let range = span(workspace, s.node.file, start, node.end);
    let selection_range = match s.name_token {
        Some(index) => span(workspace, s.node.file, index, index + 1),
        None => range_of_node(workspace, s.node),
    };
    let mut children = Vec::new();
    let mut listed: HashSet<SymbolId> = HashSet::new();
    // Members declared in the body itself, then every declaration nested in it; a member
    // added by a trait is declared elsewhere and left out.
    // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:68230cc39cefdc53159dd6a5a1e696ff3653b379ca0c7191c07a5db17ad9ab17
    for member in model.members(s.entity) {
        let m = &model.symbols[member];
        let own = m.node.file == s.node.file
            && m.node != s.node
            && model.info(m.node).start >= node.start
            && model.info(m.node).end <= node.end;
        if own
            && matches!(m.kind, SymbolKind::Member | SymbolKind::EnumMember)
            && listed.insert(member)
        {
            children.push(outline_of_symbol(workspace, member));
        }
    }
    nested_declarations(workspace, s.node, &mut listed, &mut children);
    Outline {
        name: s.name.clone(),
        kind: s.kind,
        range,
        selection_range,
        children,
    }
}

/// Every declaration nested below a node, in order, each with its own children; a node
/// that declares something is not entered further.
// @lfy def/query/main.lfy:outlineOf
fn nested_declarations(
    workspace: &Workspace,
    node: NodeRef,
    listed: &mut HashSet<SymbolId>,
    out: &mut Vec<Outline>,
) {
    let model = &workspace.model;
    for child in child_nodes(model, node) {
        match model.symbol_of(child) {
            Some(symbol) if listed.contains(&symbol) => {}
            Some(symbol)
                if is_outlined(model.symbols[symbol].kind)
                    && !matches!(
                        model.symbols[symbol].kind,
                        SymbolKind::Member | SymbolKind::EnumMember
                    )
                    && model.symbols[symbol].node == child =>
            {
                listed.insert(symbol);
                out.push(outline_of_symbol(workspace, symbol));
            }
            _ => nested_declarations(workspace, child, listed, out),
        }
    }
}

/// The declarations of one file, nested as written.
///
/// One outline per symbol declared in the file scope, in order, except module symbols
/// and the symbols a `Use` imports; each holds the members of its body and every
/// declaration nested in it. Empty for a file not in the program.
// @lfy def/query/main.lfy:outlineOf
pub fn outline_of(workspace: &Workspace, file: &str) -> Vec<Outline> {
    let model = &workspace.model;
    let Some(id) = file_of(workspace, file) else {
        return Vec::new(); // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:267b17e8c1e5a951c9981534d31e87180bedced640f519c1fea34a6ebcb08ae2
    };
    // `Scope::declared` gives the symbols the file scope declares itself, in order, which
    // leaves out the symbols a `Use` imports.
    // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:75e0f581b2c1e2e67024c96113d0bb4d0de9485bba0f4d0bc52668334b0aca46
    model.scopes[model.file_scopes[id]]
        .declared()
        .filter(|&symbol| is_outlined(model.symbols[symbol].kind))
        .map(|symbol| outline_of_symbol(workspace, symbol))
        .collect()
}

/// Flattens an outline: each entry with its dotted name and no children.
// @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:13178e674080b5c706e201a7d26af8467c11a9e94ea94412d33d692ae360d1b1
fn flatten(outline: &Outline, parent: Option<&str>, out: &mut Vec<Outline>) {
    let name = match parent {
        Some(parent) => format!("{parent}.{}", outline.name),
        None => outline.name.clone(),
    };
    out.push(Outline {
        name: name.clone(),
        kind: outline.kind,
        range: outline.range.clone(),
        selection_range: outline.selection_range.clone(),
        children: Vec::new(),
    });
    for child in &outline.children {
        flatten(child, Some(&name), out);
    }
}

/// Declarations across the program whose name contains a text, ignoring case: every
/// outline of every file, flattened, a child named by its parent's name, a dot, and its
/// own. An empty query gives every top level declaration. Files without a package come
/// first, then package files, each in `Workspace.files` order.
// @lfy def/query/main.lfy:findSymbols
pub fn find_symbols(workspace: &Workspace, query: &str) -> Vec<Outline> {
    let query = query.to_lowercase();
    let mut out = Vec::new();
    for file in files_in_search_order(workspace) {
        let path = workspace.model.sources[file].path.clone();
        for outline in outline_of(workspace, &path) {
            // Decision: flattened entries carry no children, so a top level entry is
            // listed alone.
            // @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:f133971178ba35e56f7538600c180d6f1e36300ecd60b6befa6d4bd852510f4f
            if query.is_empty() {
                let mut top = outline.clone();
                top.children.clear();
                out.push(top);
                continue;
            }
            let mut flat = Vec::new();
            flatten(&outline, None, &mut flat);
            // @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:88e9675dd098a5864faa0fd7eadc96f57937a4b1389ee9fe2fde45430ea4af24
            out.extend(
                flat.into_iter()
                    .filter(|entry| entry.name.to_lowercase().contains(&query)),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::tests::*;

    // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:75e0f581b2c1e2e67024c96113d0bb4d0de9485bba0f4d0bc52668334b0aca46
    // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:68230cc39cefdc53159dd6a5a1e696ff3653b379ca0c7191c07a5db17ad9ab17
    // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:3a7355b9e45ec3b575bc69fd4a7ce2bffafd0dcfd946c80bcf0c0783dc8c87be
    // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:267b17e8c1e5a951c9981534d31e87180bedced640f519c1fea34a6ebcb08ae2
    // @lfy def/query/main.lfy:outlineOf#outlineOf:outlineOf:944ffc79e500371c014c0fef2f5b06f6e9ecf9de004bd424c16115b4aed74b22
    #[test]
    fn the_outline_nests_members_then_declarations_and_skips_parameters() {
        let text = "use \"./b\" as B;\n/// Doc\nd A is t { $x = string; fn f(p: string) { const q = 1; } $y = number; }\ntrait t { $m = string; }\nenum E { one = 1 }\nfunction g(r: number) { for (const i in [1]) { const s = i; } }\n";
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", text)
            .write("def/b.lfy", "d B {}\n");
        let ws = fixture.load();
        let outline = outline_of(&ws, A);
        let names: Vec<&str> = outline.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["A", "t", "E", "g"]);
        let a = &outline[0];
        assert_eq!(a.kind, SymbolKind::Data);
        let end = text.lines().nth(2).unwrap().chars().count();
        assert_eq!(a.range, range(A, (2, 0), (3, end)));
        assert_eq!(a.selection_range, range(A, (3, 2), (3, 3)));
        let children: Vec<(&str, &str)> = a
            .children
            .iter()
            .map(|o| (o.name.as_str(), o.kind.as_str()))
            .collect();
        assert_eq!(
            children,
            [("x", "member"), ("y", "member"), ("f", "agentFunction")]
        );
        let f = &a.children[2];
        assert_eq!(
            f.children
                .iter()
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>(),
            ["q"]
        );
        assert_eq!(outline[2].children[0].name, "one");
        assert_eq!(outline[2].children[0].kind, SymbolKind::EnumMember);
        assert_eq!(
            outline[3]
                .children
                .iter()
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>(),
            ["s"]
        );
        assert!(outline_of(&ws, "def/none.lfy").is_empty());
    }

    // @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:88e9675dd098a5864faa0fd7eadc96f57937a4b1389ee9fe2fde45430ea4af24
    // @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:13178e674080b5c706e201a7d26af8467c11a9e94ea94412d33d692ae360d1b1
    // @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:f133971178ba35e56f7538600c180d6f1e36300ecd60b6befa6d4bd852510f4f
    // @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:2cafa010009d2c7783a291a1e27d35e09d9ef1f73122866e4953772fad3a0b32
    // @lfy def/query/main.lfy:findSymbols#findSymbols:findSymbols:67dabcd7a9a6119aa73cbde0eccf8c476a2a39a83bb7d47123def283c00e102d
    #[test]
    fn find_symbols_flattens_dotted_names_across_files_own_first() {
        let fixture = Fixture::new();
        fixture
            .write(
                "elfie.json",
                r#"{ "dependencies": { "elfie": "./lib", "p": "./pkg" } }"#,
            )
            .write("def/a.lfy", "d Alpha { $beta = string; }\n")
            .write("def/b.lfy", "use \"./a\";\nconst gamma = Alpha;\n")
            .write("pkg/main.lfy", "d Beta {}\n");
        let ws = fixture.load();
        let names = |query: &str| -> Vec<String> {
            find_symbols(&ws, query)
                .into_iter()
                .map(|o| o.name)
                .collect()
        };
        assert_eq!(names("beta"), ["Alpha.beta", "Beta"]);
        assert_eq!(names("ALPHA"), ["Alpha", "Alpha.beta"]);
        assert_eq!(names(""), ["Alpha", "gamma", "Beta"]);
        assert!(find_symbols(&ws, "").iter().all(|o| o.children.is_empty()));
        assert!(names("zzz").is_empty());
    }
}
