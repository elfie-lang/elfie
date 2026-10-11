// @lfy def/query/main.lfy:renameAt
//! The edits that rename the symbol at a position everywhere.

use super::*;
use crate::grammar::GrammarRule;
use crate::grammar::terminals::identifier::Identifier as I;
use crate::grammar::terminals::keyword::Keyword as K;
use crate::model::ScopeId;

/// Where a symbol is declared, as `file:line`, for a reason.
fn declared_at(workspace: &Workspace, symbol: SymbolId) -> String {
    let model = &workspace.model;
    let s = &model.symbols[symbol];
    if !is_real(model, s.node) {
        return "the program itself".to_string();
    }
    let file = &model.sources[s.node.file].path;
    let line = s
        .name_token
        .map(|index| tokens(workspace, s.node.file)[index].line)
        .or_else(|| model.first_token(s.node).map(|token| token.line))
        .unwrap_or(1);
    format!("{file}:{line}")
}

/// The edits that rename the symbol at a position everywhere, or why it cannot be renamed.
///
/// Nothing declared at the position, a name that is not an identifier or is a keyword,
/// or a symbol named `name` already visible from the declaring scope or from the scope of
/// any usage (the rename would capture or be captured) each give a reason. Otherwise one
/// edit replaces the identifier of the declaration and one the name token of every
/// usage, references in templates and documentation included; for a module symbol only
/// the name after `as` and its usages change. Edits are in `Workspace.files` order, then
/// by start, and never overlap.
// @lfy def/query/main.lfy:renameAt
pub fn rename_at(
    workspace: &Workspace,
    file: &str,
    position: Position,
    name: &str,
) -> Result<Vec<Edit>, String> {
    let model = &workspace.model;
    let Some(symbol) = symbol_at(workspace, file, position) else {
        return Err("nothing is declared here".to_string()); // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:b3fa6dbc86d24eb00eade77e10fed4d36515e9a798f0fc3f0f04191c209583ee
    };
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:ae51c574616199669818286421fb3f813b8f790e1b9ee3b80b73b7cbdaa7b374
    if let Some(keyword) = K::from_text(name) {
        return Err(format!("{name} is a keyword ({})", keyword.identifier()));
    }
    if !I::Identifier.matches(name) {
        return Err(format!("{name:?} is not an {}", I::Identifier.identifier()));
    }
    let s = &model.symbols[symbol];
    if s.name == name {
        return Ok(Vec::new());
    }
    let Some(name_token) = s.name_token.filter(|_| is_real(model, s.node)) else {
        return Err("the declaration spells no name that can be renamed".to_string());
    };
    let usages = usages_of_declaration(model, symbol);
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:6fd3328ce678d8db79053c7d449378d41a8ae2b47ff54a1b31b2f3a0d795b6d9
    let mut scopes: Vec<ScopeId> = vec![s.scope];
    scopes.extend(
        usages
            .iter()
            .map(|&usage| model.enclosing_scope(model.usages[usage].node)),
    );
    for scope in scopes {
        if let Some(other) = model.lookup(scope, name) {
            return Err(format!(
                "a symbol named {name} is already visible, declared at {}",
                declared_at(workspace, other)
            ));
        }
    }
    if model.entities[model.global].identifier.as_deref() == Some(name) {
        return Err(format!(
            "a symbol named {name} is already visible, declared by the program itself"
        ));
    }
    // A module symbol is named by the token after `as`, so renaming it edits that name and
    // its usages and never the file.
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:823a0f85531d12e49b75f78010bbb1ed125a887bdcaee1b3a50f0d9137aed0b9
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:4ab53655b20b4378af6810e32d9f6f84ae0ae4b2b6f07ee2a4e2a739ac8121c2
    let mut edits = vec![Edit {
        range: span(workspace, s.node.file, name_token, name_token + 1),
        text: name.to_string(),
    }];
    for usage in usages {
        let usage = &model.usages[usage];
        // Decision: only a usage that spells the name changes; a usage through an alias
        // spells the alias, and a usage of a layer alone spells nothing.
        if usage.name.as_deref() != Some(s.name.as_str()) {
            continue;
        }
        if let Some(token) = usage.token {
            edits.push(Edit {
                range: span(workspace, usage.node.file, token, token + 1),
                text: name.to_string(),
            });
        }
    }
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:1cfa6517d314d6b849e4b5f034888adf621fe9fa21decbb02ea19908d33804c6
    edits.sort_by(|a, b| {
        (
            file_order(workspace, &a.range.file),
            &a.range.file,
            a.range.start,
        )
            .cmp(&(
                file_order(workspace, &b.range.file),
                &b.range.file,
                b.range.start,
            ))
    });
    edits.dedup();
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::tests::*;

    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:823a0f85531d12e49b75f78010bbb1ed125a887bdcaee1b3a50f0d9137aed0b9
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:1cfa6517d314d6b849e4b5f034888adf621fe9fa21decbb02ea19908d33804c6
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:b5d85561894c750665d6d3f99ffb0079ffdefdcd3c1a5f170ebe222eb7cc2623
    #[test]
    fn a_rename_edits_the_declaration_the_reference_and_the_value() {
        let text = "d A {} const y: `See [[A]]` = A;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let edits = rename_at(&ws, A, at(1, 2), "B").unwrap();
        assert_eq!(
            edits,
            [
                Edit {
                    range: range(A, (1, 2), (1, 3)),
                    text: "B".to_string()
                },
                Edit {
                    range: range(A, (1, 23), (1, 24)),
                    text: "B".to_string()
                },
                Edit {
                    range: range(A, (1, 30), (1, 31)),
                    text: "B".to_string()
                },
            ]
        );
        // From a usage the edits are the same.
        assert_eq!(rename_at(&ws, A, at(1, 30), "B").unwrap(), edits);
        // Documentation references are renamed too.
        let text = "d A {}\n/// See [[A]]\nconst y = 1;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let edits = rename_at(&ws, A, at(1, 2), "B").unwrap();
        assert_eq!(edits.len(), 2, "{edits:?}");
        assert_eq!(edits[1].range, range(A, (2, 10), (2, 11)));
    }

    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:b3fa6dbc86d24eb00eade77e10fed4d36515e9a798f0fc3f0f04191c209583ee
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:ae51c574616199669818286421fb3f813b8f790e1b9ee3b80b73b7cbdaa7b374
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:6fd3328ce678d8db79053c7d449378d41a8ae2b47ff54a1b31b2f3a0d795b6d9
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:38c28e52447e67ab5d7c6576c2e7a86c91dc2e0e62c4bf70a154374912a4a4c7
    #[test]
    fn a_rename_to_a_keyword_or_a_captured_name_is_refused() {
        let text = "d A {} const y = A;";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        let reason = rename_at(&ws, A, at(1, 2), "const").unwrap_err();
        assert!(reason.contains("const is a keyword"), "{reason}");
        let reason = rename_at(&ws, A, at(1, 2), "1x").unwrap_err();
        assert!(reason.contains("Identifier"), "{reason}");
        let reason = rename_at(&ws, A, at(1, 2), "y").unwrap_err();
        assert!(reason.contains("def/a.lfy:1"), "{reason}");
        let reason = rename_at(&ws, A, at(1, 5), "B").unwrap_err();
        assert!(reason.contains("nothing is declared"), "{reason}");
        // A name visible from a usage's scope captures too.
        let text = "d A {}\nfunction f() { const B = 1; const y = A; }";
        let fixture = Fixture::one(text);
        let ws = fixture.load();
        assert!(rename_at(&ws, A, at(1, 2), "B").is_err());
        assert!(rename_at(&ws, A, at(1, 2), "C").is_ok());
    }

    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:4ab53655b20b4378af6810e32d9f6f84ae0ae4b2b6f07ee2a4e2a739ac8121c2
    // @lfy def/query/main.lfy:renameAt#renameAt:renameAt:1cfa6517d314d6b849e4b5f034888adf621fe9fa21decbb02ea19908d33804c6
    #[test]
    fn renaming_a_module_changes_the_name_after_as_and_its_usages_only() {
        let fixture = Fixture::new();
        fixture
            .write(
                "def/a.lfy",
                "use \"./b\" as B;\nconst y = B.B;\nalias K = B.B;\nconst z = K;\n",
            )
            .write("def/b.lfy", "d B {}\n");
        let ws = fixture.load();
        let edits = rename_at(&ws, A, at(1, 13), "M").unwrap();
        assert_eq!(
            edits,
            [
                Edit {
                    range: range(A, (1, 13), (1, 14)),
                    text: "M".to_string()
                },
                Edit {
                    range: range(A, (2, 10), (2, 11)),
                    text: "M".to_string()
                },
                Edit {
                    range: range(A, (3, 10), (3, 11)),
                    text: "M".to_string()
                },
            ]
        );
        // Renaming the data does not touch the alias's usages.
        let edits = rename_at(&ws, A, at(2, 12), "D").unwrap();
        let files: Vec<(&str, Position)> = edits
            .iter()
            .map(|e| (e.range.file.as_str(), e.range.start))
            .collect();
        assert_eq!(
            files,
            [("def/b.lfy", at(1, 2)), (A, at(2, 12)), (A, at(3, 12))]
        );
    }
}
