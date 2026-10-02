//! Compiled from `def/grammar/rules/file.lfy`.

use super::super::grammar_rules;
use super::super::traits::category::*;

grammar_rules! {
    /// The root rule of a source file.
    pub enum File {
        SourceFile is [rule()]: "A file: statements in order" = "(: [[Statement]] :)", // @lfy def/grammar/rules/file.lfy:SourceFile#SourceFile:rule:97928766b42cd7df993c02d6e9223e100158252eeea6f9063dbbe8dd33834e4a
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::GrammarRule;
    use super::*;

    use crate::lexer::{Token, lex};
    use crate::parser::{Child, Node, Tree, parse};

    // @lfy def/grammar/rules/file.lfy:SourceFile#SourceFile:rule:97928766b42cd7df993c02d6e9223e100158252eeea6f9063dbbe8dd33834e4a
    #[test]
    fn a_source_file_is_statements_in_order() {
        assert_eq!(File::ALL, &[File::SourceFile]);
        assert_eq!(
            File::SourceFile.text(),
            "SourceFile = (: [[Statement]] :) ;"
        );
        assert_eq!(
            File::SourceFile.expression().unwrap().references(),
            vec!["Statement"]
        );
    }

    /// A file whose every token is outside a text body, and none of whose rules refuses
    /// trivia between two of its tokens, so that trivia may sit on either side of each of
    /// them.
    const WITHOUT_A_TEXT_BODY: &str = concat!(
        "const a = 1;\n",
        "d Thing { }\n",
        "function f(n: Number) -> Number { return n + 1; }\n",
        "for (const k in a) { if (k) { break; } else { continue; } }\n",
    );

    /// The tree of `source` as a file, named rule by rule and token by token, with the
    /// trivia left out. Panics when the source does not parse cleanly.
    fn significant_shape(source: &str) -> String {
        let tokens = lex(source, None).unwrap_or_else(|error| panic!("{source:?}: {error}"));
        let tree = parse(tokens, None);
        assert!(tree.errors.is_empty(), "{source:?}: {:?}", tree.errors);
        shape(&tree.root, &tree.tokens)
    }

    fn shape(node: &Node, tokens: &[Token]) -> String {
        let children: Vec<String> = node
            .significant(tokens)
            .into_iter()
            .map(|child| match child {
                Child::Node(child) => shape(child, tokens),
                Child::Error(_) => "Error".to_owned(),
                Child::Token(index) => {
                    let token = &tokens[*index];
                    format!("{}({})", token.rule.unwrap().identifier(), token.raw)
                }
            })
            .collect();
        format!("{}[{}]", node.rule.identifier(), children.join(" "))
    }

    /// `source` with `trivia` put between every two of its tokens.
    fn spread(source: &str, trivia: &str) -> String {
        let tokens = lex(source, None).unwrap_or_else(|error| panic!("{source:?}: {error}"));
        let raw: Vec<&str> = tokens.iter().map(|token| token.raw.as_str()).collect();
        raw.join(trivia)
    }

    /// The tree of `source` as a file. Panics when the source does not lex.
    fn tree_of(source: &str) -> Tree {
        let tokens = lex(source, None).unwrap_or_else(|error| panic!("{source:?}: {error}"));
        parse(tokens, None)
    }

    /// The raw text each error node of `source` as a file covers, in source order.
    fn error_spans(source: &str) -> Vec<String> {
        let tree = tree_of(source);
        tree.errors
            .iter()
            .map(|error| tree.raw(error.start, error.end))
            .collect()
    }

    // @lfy def/grammar/rules/file.lfy:SourceFile#SourceFile:recoverable:d9550fd07931fbd53b6b55c4069ca8833fed6bff3eaee29733d0c44d489b0126
    #[test]
    fn a_source_files_statements_run_to_the_end_of_the_input() {
        for source in ["", "a;", "a; b;", WITHOUT_A_TEXT_BODY] {
            let tree = tree_of(source);
            assert!(tree.errors.is_empty(), "{source:?}: {:?}", tree.errors);
            assert_eq!(tree.root.start, 0, "{source:?}");
            assert_eq!(tree.root.end, tree.tokens.len(), "{source:?}");
        }
    }

    // @lfy def/grammar/rules/file.lfy:SourceFile#SourceFile:recoverable:c50498a552ee44abf7fe3a09d43a5cc50ee96a665edf21d1c85809d2b415ec5d
    #[test]
    fn a_stray_semicolon_or_brace_between_statements_is_covered_alone() {
        assert_eq!(error_spans("a; ; b;"), [";"]);
        assert_eq!(error_spans("a; } b;"), ["}"]);
        assert_eq!(error_spans("a; ; } ; b;"), [";", "}", ";"]);
    }

    // @lfy def/grammar/rules/file.lfy:SourceFile#SourceFile:recoverable:61b9ca57b9c6f0db76783555f549078c6ddf19c5836cc5b200cbb1cab28bc77f
    #[test]
    fn any_other_stray_token_is_covered_up_to_the_next_statement() {
        assert_eq!(error_spans("a; , , b;"), [", , "]);
        // The matched parentheses are counted, so the comma between them is covered too.
        assert_eq!(error_spans("a; , ( ) , b;"), [", ", "( ) , "]);
        // `a; ,` has no statement after the comma for the error to stop at.
        assert_eq!(error_spans("a; ,"), [","]);
    }

    // @lfy def/grammar/rules/file.lfy:SourceFile#SourceFile:recoverable:38bd9f4dd6db310a2703c0f185d6dada112bf329e7b2fef529baa93b64847771
    #[test]
    fn a_close_without_its_open_ends_the_error_before_it() {
        assert_eq!(error_spans("a; , ) b;"), [", ", ") "]);
        assert_eq!(error_spans("a; , ) ) b;"), [", ", ") ", ") "]);
        assert_eq!(error_spans("a; , ]"), [", ", "]"]);
    }

    // @lfy def/grammar/rules/file.lfy:SourceFile#SourceFile:recoverable:b348bf5366168988f57986944a5d40d08474b6ed588ddff0aaefa74a1e7c150f
    #[test]
    fn an_error_between_statements_expects_what_begins_a_statement() {
        for source in ["a; ; b;", "a; , , b;", "a; , ) b;", "a; ,"] {
            let tree = tree_of(source);
            assert!(!tree.errors.is_empty(), "{source:?}");
            for error in &tree.errors {
                for identifier in ["Identifier", "ConstKeyword", "IfKeyword", "BlockOpen", "GroupOpen", "Minus"] {
                    assert!(error.expected.contains(&identifier), "{source:?}: {identifier}");
                }
                for identifier in ["Semicolon", "Comma", "GroupClose", "BlockClose", "Plus", "Space"] {
                    assert!(!error.expected.contains(&identifier), "{source:?}: {identifier}");
                }
            }
        }
    }

    // @lfy def/grammar/rules/file.lfy:SourceFile#SourceFile:SourceFile:745888c84bd8381f81083e03153142ec9de1e1705ae81e03d5643346a18930dc
    #[test]
    fn a_source_file_takes_trivia_between_any_two_of_its_tokens() {
        let plain = significant_shape(WITHOUT_A_TEXT_BODY);
        for trivia in [
            " ",         // Space
            "\t",        // Space
            "\n",        // NewLine
            "/* c */",   // Comment, block
            "// c\n",    // Comment, line
            "/** d **/", // Documentation, block
            "/// d\n",   // Documentation, line
        ] {
            let spread = spread(WITHOUT_A_TEXT_BODY, trivia);
            assert_eq!(significant_shape(&spread), plain, "{trivia:?}");
        }
    }
}
