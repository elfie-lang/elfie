// @lfy def/query/main.lfy:diagnosticsOf
//! Every problem of the program, placed.

use std::collections::HashSet;

use super::*;
use crate::grammar::GrammarRule;
use crate::grammar::rules::statement::Statement as S;
use crate::workspace::WorkspaceProblem;

/// The project layout file, where a load problem without a path is placed.
const MANIFEST: &str = "elfie.json";

/// Every problem of the program, placed.
///
/// A load problem is an error of stage loader at line 1, column 0 of its path, or of
/// `elfie.json` under the root when it has none. A token without a rule is an error of
/// stage lexer at the token: invalid text, or the mode left open when the token has no
/// text. An error node is an error of stage parser listing what was expected and what
/// was found; when it holds the keyword the open rule would have taken as a name, the
/// error is at that keyword instead, saying it is a keyword and cannot be a name. A
/// problem of the model is an error at its node, of stage loader when that
/// node is a `Use` and of stage binder otherwise. A `Use`
/// importing symbols of which none is used in the file, or naming a module never used, is
/// a warning of stage binder at the `Use`, unless the file holding it declares no symbol
/// of its own and so only re-exports. With `file` set, only that file's diagnostics
/// are returned. Diagnostics are in `Workspace.files` order, then by start, with
/// `elfie.json` first.
// @lfy def/query/main.lfy:diagnosticsOf
pub fn diagnostics_of(workspace: &Workspace, file: Option<&str>) -> Vec<Diagnostic> {
    let model = &workspace.model;
    let mut out = Vec::new();
    let error = |range: Range, stage: Stage, message: String| Diagnostic {
        range,
        severity: Severity::Error,
        stage,
        message,
    };

    for problem in &workspace.problems {
        match problem {
            // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:7b71c411410f4a67352e35d1dea692d221ca1b0c055125cdedeef1785b20a62c
            // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:45788e4c5aba41f636eb55e7bb6787d58df3fb0d471d2ae00d710a2b8f6c3c46
            WorkspaceProblem::Load(problem) => {
                let path = problem.path.as_deref().unwrap_or(MANIFEST);
                out.push(error(
                    Range::empty(path, Position::new(1, 0)),
                    Stage::Loader,
                    problem.message.clone(),
                ));
            }
            // Every problem names the stage that added it, so loading's problems at a
            // `Use` and binding's elsewhere are told apart by `Problem.stage` alone.
            // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:b3fbff74b696c426c78e1622473bf46991ed2ce09b13dde41d409769b81f49aa
            WorkspaceProblem::Bind(problem) => {
                if is_real(model, problem.node) {
                    out.push(error(
                        range_of_node(workspace, problem.node),
                        problem.stage,
                        problem.message.clone(),
                    ));
                }
            }
        }
    }

    for (id, source) in model.sources.iter().enumerate() {
        for (index, token) in source.tree.tokens.iter().enumerate() {
            if token.rule.is_some() {
                continue;
            }
            // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:659c3e35343ca8a0c0f8b062549f92c4b7d902b040a6d5d79f2a6fb736e27351
            // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:6df72f95158bd04dcd3388f6c70e24ba0281159a37885e0a68a081dd3198143d
            let message = if token.raw.is_empty() {
                format!("the {} is left open at the end of the file", token.value)
            } else {
                format!("invalid text {:?}", token.raw)
            };
            out.push(error(
                span(workspace, id, index, index + 1),
                Stage::Lexer,
                message,
            ));
        }
        for node in &source.tree.errors {
            // `ErrorNode.keyword` is held by value, so its range is read from the token
            // itself rather than by index into the file's tokens.
            // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:8eb39df1b8c7036fa72ba03e13399e89f19b99508ff87fe86125cb50f6c3c9fa
            if let Some(keyword) = &node.keyword {
                out.push(error(
                    Range::of_token(keyword),
                    Stage::Parser,
                    format!("{} is a keyword and cannot be a name", keyword.raw),
                ));
                continue;
            }
            // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:bda5e055c75f47a4e289fbc70bae813935676e1b44427a57fa4c9bee23a0f7da
            let found = source.tree.raw(node.start, node.end);
            let found = if found.is_empty() {
                "the end of the file".to_string()
            } else {
                format!("{:?}", found.trim())
            };
            let message = if node.expected.is_empty() {
                format!("unexpected {found}")
            } else {
                format!("expected {}, found {found}", node.expected.join(" or "))
            };
            out.push(error(
                span(workspace, id, node.start, node.end),
                Stage::Parser,
                message,
            ));
        }
        out.extend(unused_uses(workspace, id));
    }

    if let Some(file) = file {
        out.retain(|diagnostic| diagnostic.range.file == file);
    }
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:215cb16ac34749dba8b39f3c4b0772c200e5c3bcd0f576c0a3c315cbf26dd26e
    out.sort_by(|a, b| diagnostic_key(workspace, a).cmp(&diagnostic_key(workspace, b)));
    out
}

/// The sort key of a diagnostic: `elfie.json` first, then `Workspace.files` order, then
/// paths outside the program, then start.
pub(super) fn diagnostic_key<'d>(
    workspace: &Workspace,
    diagnostic: &'d Diagnostic,
) -> (usize, &'d str, Position, Position) {
    let order = if diagnostic.range.file == MANIFEST {
        0
    } else {
        file_order(workspace, &diagnostic.range.file).saturating_add(1)
    };
    (
        order,
        &diagnostic.range.file,
        diagnostic.range.start,
        diagnostic.range.end,
    )
}

/// A warning per `Use` of a file that imports symbols of which none is used in the file,
/// or whose module symbol is never used. A file that declares no symbol of its own is
/// there to re-export what it uses, so none of its uses is warned about.
// @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:8e851e2cb4180e8b1c677c4d71389d4e1b9374ee37f175153cb37d3aae9034bf
fn unused_uses(workspace: &Workspace, file: FileId) -> Vec<Diagnostic> {
    let model = &workspace.model;
    let mut out = Vec::new();
    // A file that declares no symbol of its own, as the main file of the package elfie
    // does, only re-exports what it uses.
    if model.scopes[model.file_scopes[file]].declared().next().is_none() {
        return out;
    }
    let uses: Vec<NodeRef> = model.nodes[file]
        .iter()
        .enumerate()
        .filter(|(_, info)| info.rule == S::Use.entity())
        .map(|(index, _)| NodeRef { file, index })
        .collect();
    for (position, use_node) in uses.into_iter().enumerate() {
        let Some(Some(path)) = model.sources[file].uses.get(position) else {
            continue;
        };
        let message = match model.symbol_of(use_node) {
            Some(symbol) => {
                if model::usages_of(model, symbol).is_empty() {
                    Some(format!(
                        "the module {} is never used",
                        model.symbols[symbol].name
                    ))
                } else {
                    None
                }
            }
            None => {
                let Some(used) = model.file(path) else {
                    continue;
                };
                let imported: HashSet<SymbolId> = module_symbols(model, used).into_iter().collect();
                let any_used = model.usages.iter().any(|usage| {
                    usage.node.file == file
                        && usage
                            .symbol
                            .is_some_and(|symbol| imported.contains(&symbol))
                });
                if any_used {
                    None
                } else {
                    Some(format!("nothing that {path} declares is used in this file"))
                }
            }
        };
        if let Some(message) = message {
            out.push(Diagnostic {
                range: range_of_node(workspace, use_node),
                severity: Severity::Warning,
                stage: Stage::Binder,
                message,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::tests::*;
    use crate::workspace;

    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:b3fbff74b696c426c78e1622473bf46991ed2ce09b13dde41d409769b81f49aa
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:9081d09fe7497f189e9f1595ce2f2ce9f21c0ab185845713df749c40ec6dbb36
    #[test]
    fn an_undeclared_name_is_a_binder_error_at_the_name() {
        let fixture = Fixture::one("const y = z;");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].stage, Stage::Binder);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].range, range(A, (1, 10), (1, 11)));
        assert!(diagnostics[0].message.contains('z'));
    }

    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:bda5e055c75f47a4e289fbc70bae813935676e1b44427a57fa4c9bee23a0f7da
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:3591e88c1e517b788048aa32485e38f91b33246adf96ea3d290f78bd2d38ec39
    #[test]
    fn a_missing_name_is_a_parser_error_at_the_equals_sign() {
        let fixture = Fixture::one("const = 1;");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, Some(A));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].stage, Stage::Parser);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].range.start, at(1, 6));
        assert!(
            diagnostics[0].message.contains("Identifier"),
            "{}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains('='),
            "{}",
            diagnostics[0].message
        );
        assert!(diagnostics_of(&ws, Some("def/other.lfy")).is_empty());
    }

    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:8eb39df1b8c7036fa72ba03e13399e89f19b99508ff87fe86125cb50f6c3c9fa
    #[test]
    fn a_keyword_taken_as_a_name_is_a_parser_error_at_that_keyword() {
        let fixture = Fixture::one("const d = 1;");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].stage, Stage::Parser);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].range, range(A, (1, 6), (1, 7)));
        assert_eq!(diagnostics[0].message, "d is a keyword and cannot be a name");
    }

    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:7b71c411410f4a67352e35d1dea692d221ca1b0c055125cdedeef1785b20a62c
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:45788e4c5aba41f636eb55e7bb6787d58df3fb0d471d2ae00d710a2b8f6c3c46
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:b3fbff74b696c426c78e1622473bf46991ed2ce09b13dde41d409769b81f49aa
    #[test]
    fn load_problems_are_placed_at_the_top_of_their_path_or_of_the_manifest() {
        let fixture = Fixture::new();
        fixture
            .write(
                "elfie.json",
                r#"{ "lib": "lib", "dependencies": { "gone": { "root": "nowhere" } }, "name": 3 }"#,
            )
            .write("def/a.lfy", "use \"./missing\";\n");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.stage == Stage::Loader && d.severity == Severity::Error),
            "{diagnostics:?}"
        );
        let files: Vec<&str> = diagnostics.iter().map(|d| d.range.file.as_str()).collect();
        assert_eq!(files, ["elfie.json", "def/a.lfy", "nowhere"]);
        // Each `LoadProblem` sits at the top of its path; the unresolved `Use` is a
        // `Problem` at its node, of the stage `Problem.stage` names.
        // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:7b71c411410f4a67352e35d1dea692d221ca1b0c055125cdedeef1785b20a62c
        assert_eq!(diagnostics[0].range, Range::empty("elfie.json", at(1, 0)));
        assert_eq!(diagnostics[1].range, range(A, (1, 0), (1, 16)));
        assert_eq!(diagnostics[2].range, Range::empty("nowhere", at(1, 0)));
        // A problem with no path goes to elfie.json whether or not it exists.
        let ws = workspace::load(&fixture.root.join("absent"));
        let diagnostics = diagnostics_of(&ws, None);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range, Range::empty("elfie.json", at(1, 0)));
    }

    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:6df72f95158bd04dcd3388f6c70e24ba0281159a37885e0a68a081dd3198143d
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:659c3e35343ca8a0c0f8b062549f92c4b7d902b040a6d5d79f2a6fb736e27351
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:215cb16ac34749dba8b39f3c4b0772c200e5c3bcd0f576c0a3c315cbf26dd26e
    #[test]
    fn invalid_text_and_an_open_mode_are_lexer_errors() {
        let fixture = Fixture::one("const y = 1;\nconst z = `open");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        let lexer: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.stage == Stage::Lexer)
            .collect();
        assert_eq!(lexer.len(), 1, "{diagnostics:?}");
        assert!(
            lexer[0].message.contains("template"),
            "{}",
            lexer[0].message
        );
        assert_eq!(lexer[0].range, Range::empty(A, at(2, 15)));
        let fixture = Fixture::one("const y = 1 # 2;");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        let lexer: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.stage == Stage::Lexer)
            .collect();
        assert_eq!(lexer.len(), 1, "{diagnostics:?}");
        assert!(lexer[0].message.contains("invalid"), "{}", lexer[0].message);
        assert!(lexer[0].message.contains('#'), "{}", lexer[0].message);
        assert_eq!(lexer[0].range, range(A, (1, 12), (1, 13)));
        // Diagnostics are ordered by file then position.
        let positions: Vec<Position> = diagnostics.iter().map(|d| d.range.start).collect();
        let mut sorted = positions.clone();
        sorted.sort();
        assert_eq!(positions, sorted);
    }

    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:8e851e2cb4180e8b1c677c4d71389d4e1b9374ee37f175153cb37d3aae9034bf
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:c10084a238db0d56540c9f6a7e1725d83ae9ed7f46d1d34cc88f993bf0334416
    #[test]
    fn an_unused_use_is_a_binder_warning() {
        let fixture = Fixture::new();
        fixture
            .write(
                "def/a.lfy",
                "use \"./b\";\nuse \"./c\" as C;\nuse \"./c\";\nconst z = C1;\n",
            )
            .write("def/b.lfy", "d B {}\n")
            .write("def/c.lfy", "d C1 {}\n");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        let warnings: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        let lines: Vec<usize> = warnings.iter().map(|d| d.range.start.line).collect();
        assert_eq!(lines, [1, 2], "{warnings:?}");
        assert!(warnings.iter().all(|d| d.stage == Stage::Binder));
        assert!(
            warnings[0].message.contains("def/b.lfy"),
            "{}",
            warnings[0].message
        );
        assert!(warnings[1].message.contains('C'), "{}", warnings[1].message);
        assert_eq!(warnings[0].range, range(A, (1, 0), (1, 10)));
    }

    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:8e851e2cb4180e8b1c677c4d71389d4e1b9374ee37f175153cb37d3aae9034bf
    // @lfy def/query/main.lfy:diagnosticsOf#diagnosticsOf:diagnosticsOf:e9581273e68e498bbc69cdcf24ce021a85a747e110aab176e0a5096d9315059c
    #[test]
    fn a_file_that_only_re_exports_is_never_warned_about() {
        let fixture = Fixture::new();
        fixture
            .write("def/a.lfy", "use \"./b\";\nuse \"./c\";\n")
            .write("def/b.lfy", "d B {}\n")
            .write("def/c.lfy", "d C1 {}\n");
        let ws = fixture.load();
        // `def/a.lfy` declares nothing of its own, so neither of its uses is warned
        // about although nothing in it uses what they import.
        let diagnostics = diagnostics_of(&ws, None);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        // The main file of the package elfie is that same shape.
        let fixture = Fixture::with_prelude("");
        let ws = fixture.load();
        let diagnostics = diagnostics_of(&ws, None);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
}
