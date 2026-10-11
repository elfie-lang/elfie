// @lfy def/query/main.lfy:find
//! Entities by the name an agent would spell, and the text that declares one.

use super::*;

/// Entities by the name an agent would spell: an identifier, or an owner's identifier, a
/// dot, and a member's name; with a file path and a colon first, only that file's scope
/// is searched. Every entity whose symbol is named that way in any file scope, or whose
/// owner is, in the order [`find_symbols`] uses; empty when nothing matches.
// @lfy def/query/main.lfy:find
pub fn find(workspace: &Workspace, name: &str) -> Vec<EntityId> {
    let model = &workspace.model;
    // @lfy def/query/main.lfy:find#find:find:61607cb709e91135898032720f10dea2e214fb817b1408e0acc03b32ee2921dd
    let (files, name) = match name.split_once(':') {
        Some((path, rest)) => (
            file_of(workspace, path).into_iter().collect::<Vec<_>>(),
            rest,
        ),
        None => (files_in_search_order(workspace), name),
    };
    let (owner, member) = match name.split_once('.') {
        Some((owner, member)) => (owner, Some(member)),
        None => (name, None),
    };
    let mut out: Vec<EntityId> = Vec::new();
    for file in files {
        for symbol in model.scopes[model.file_scopes[file]].declared() {
            let s = &model.symbols[symbol];
            if s.name != owner {
                continue;
            }
            // @lfy def/query/main.lfy:find#find:find:fd99a23c4e8cda13dfc67f691e8488956baceb9b6a936387d01097718dd9aeeb
            // @lfy def/query/main.lfy:find#find:find:866c797d2ac82b36a267a29edec555c515e616851c8fbd949d9fb18c314fd0e2
            let found = match member {
                None => vec![s.entity],
                Some(member) => model
                    .members(s.entity)
                    .into_iter()
                    .filter(|&m| model.symbols[m].name == member)
                    .map(|m| model.symbols[m].entity)
                    .collect(),
            };
            for entity in found {
                if !out.contains(&entity) {
                    out.push(entity);
                }
            }
        }
    }
    out
}

/// The text that declares an entity: the raw text of every token the declaring node
/// covers, with the text of its documentation before it; the whole file for the anonymous
/// entity of a file; the member statement inside the trait body for a member added by a
/// trait.
// @lfy def/query/main.lfy:sourceOf
pub fn source_of(workspace: &Workspace, entity: EntityId) -> String {
    let model = &workspace.model;
    let e = &model.entities[entity];
    let Some(node) = e.node.filter(|&node| is_real(model, node)) else {
        // Decision: `global` declares nothing in any file, so its source is empty.
        return String::new();
    };
    let tree = &model.sources[node.file].tree;
    if matches!(e.kind, EntityKind::File) {
        return tree.raw(0, tree.tokens.len()); // @lfy def/query/main.lfy:sourceOf#sourceOf:sourceOf:44004ceec3f3da5b19d3201155581866ff4c72d679a91c01a53d7f261cfaca5b
    }
    // A member added by a trait declares the member statement inside the trait body, so its
    // own node already covers exactly that.
    // @lfy def/query/main.lfy:sourceOf#sourceOf:sourceOf:5e22bbec842c0245139e4ce2c5d6c95b1a30db2a6c8582182205d84b18bfa60d
    let tree_node = model.node(node);
    let mut out = String::new();
    // @lfy def/query/main.lfy:sourceOf#sourceOf:sourceOf:1231c1d038570bfc4ea96a2254b88772810a70da99d1226712806be3a3a548d9
    for documentation in &tree_node.documentation {
        out.push_str(&tree.raw(documentation.start, documentation.end));
        out.push('\n');
    }
    out.push_str(&tree.raw(tree_node.start, tree_node.end));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::tests::*;

    // @lfy def/query/main.lfy:find#find:find:fd99a23c4e8cda13dfc67f691e8488956baceb9b6a936387d01097718dd9aeeb
    // @lfy def/query/main.lfy:find#find:find:866c797d2ac82b36a267a29edec555c515e616851c8fbd949d9fb18c314fd0e2
    // @lfy def/query/main.lfy:find#find:find:61607cb709e91135898032720f10dea2e214fb817b1408e0acc03b32ee2921dd
    // @lfy def/query/main.lfy:find#find:find:1286ad770a1cf3a25e55448d3cdfe6b64efc8a2a5c597ae69414751ffe012c2d
    // @lfy def/query/main.lfy:find#find:find:812007218e035a5d35f9aa08e87e3784cecdbcacf453e01eee76367ea460340a
    // @lfy def/query/main.lfy:find#find:find:eb129c9826fc22f454f4cfe0e816a1f02fa2af14dd6f825cc1dc478e38b97cc8
    // @lfy def/query/main.lfy:find#find:find:0f3b3a4e964c2e0613053c78947730591e34949b8b937b9d79ec23c70149d5f3
    #[test]
    fn find_takes_a_name_a_dotted_member_or_a_file_prefix() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "d A { $x = string; }\n")
            .write("def/b.lfy", "d A {}\nconst x = 1;\n");
        let ws = fixture.load();
        let model = &ws.model;
        let both = find(&ws, "A");
        assert_eq!(both.len(), 2);
        assert_eq!(model.entities[both[0]].file, model.file(A));
        assert_eq!(model.entities[both[1]].file, model.file("def/b.lfy"));
        let member = find(&ws, "A.x");
        assert_eq!(member.len(), 1);
        assert!(matches!(model.entities[member[0]].kind, EntityKind::Member));
        assert_eq!(find(&ws, "def/b.lfy:A"), vec![both[1]]);
        assert!(find(&ws, "def/b.lfy:A.x").is_empty());
        assert!(find(&ws, "nothing").is_empty());
        assert!(find(&ws, "def/none.lfy:A").is_empty());
    }

    // @lfy def/query/main.lfy:sourceOf#sourceOf:sourceOf:1231c1d038570bfc4ea96a2254b88772810a70da99d1226712806be3a3a548d9
    // @lfy def/query/main.lfy:sourceOf#sourceOf:sourceOf:44004ceec3f3da5b19d3201155581866ff4c72d679a91c01a53d7f261cfaca5b
    // @lfy def/query/main.lfy:sourceOf#sourceOf:sourceOf:5e22bbec842c0245139e4ce2c5d6c95b1a30db2a6c8582182205d84b18bfa60d
    #[test]
    fn source_is_the_declaration_with_its_documentation_or_the_whole_file() {
        let text = "/// Doc\nd A: `An A` {}\ntrait t { $m = string; }\nd B is t {}\n";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let model = &ws.model;
        assert_eq!(source_of(&ws, find(&ws, "A")[0]), "/// Doc\nd A: `An A` {}");
        assert_eq!(source_of(&ws, model.file_entities[0]), text);
        assert_eq!(source_of(&ws, find(&ws, "B.m")[0]), "$m = string;");
        assert_eq!(source_of(&ws, model.global), "");
    }
}
