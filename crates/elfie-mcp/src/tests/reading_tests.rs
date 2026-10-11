// @lfy def/mcp/main.lfy:find
//! Tests of the tools that read the program: `find`, `entity`, `references`, `outline`,
//! and `grammar`.

use super::*;

// @lfy def/mcp/main.lfy:find#find:find:8d5e1239e348d7ebcaa7e007e6bfc71fed3f9aa4abd903c5cc7f3160b0416367
#[test]
fn find_entity_and_references_describe_a_declaration() {
    let fixture = Fixture::new();
    fixture.write("def/a.lfy", "/// Doc\nd A: `An A` { $x: `An x` = string; }\nconst y = A;\n");
    let session = fixture.session();

    let found = find(&session, "A").unwrap();
    // The line ends with the definition. @lfy def/mcp/main.lfy:find#find:find:eb271c552e5c82df2d2018db45719c602a20801b3e7e3e1f948bf7c9f0108b1d
    assert_eq!(found, "data A def/a.lfy:2: An A");
    assert_eq!(find(&session, "def/a.lfy:A").unwrap(), found);
    let none = find(&session, "nothing").unwrap();
    // @lfy def/mcp/main.lfy:find#find:find:33784dd4ac5c7832aad69dab34430cacc1eb4b82930eb20f500fcfe9c89be26c
    assert!(none.contains("nothing is declared as nothing"), "{none}");
    assert!(none.contains("elfie_outline"), "{none}"); // @lfy def/mcp/main.lfy:find#find:find:1a0733810793cda3b844ef9c1beb121fa974d605748755292d61aa511c80a1e2

    // @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
    let described = entity(&session, "A").unwrap();
    assert!(described.starts_with("# data A (def/a.lfy:2)"), "{described}");
    assert!(described.contains("## Definition\nAn A"), "{described}");
    assert!(described.contains("## Documentation\nDoc"), "{described}");
    assert!(described.contains("## Members\n- x: An x (string)"), "{described}");
    assert!(described.contains("## References (1)\n- def/a.lfy:3"), "{described}");
    assert!(described.contains("## Source\n```elfie\n/// Doc\nd A: `An A` { $x: `An x` = string; }\n```"), "{described}");
    assert!(!described.contains("\n---\n"));

    // @lfy def/mcp/main.lfy:references#references:references:172a06e97d4c30625ada08fa53113f0fdd6c084862a2db5f64451f15c9853456
    let used = references(&session, "A").unwrap();
    assert_eq!(used, "def/a.lfy:3:10: const y = A;");
    assert_eq!(references(&session, "y").unwrap(), "y is never used");
}

// @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
#[test]
fn entity_lists_the_traits_it_carries() {
    let fixture = Fixture::new();
    fixture.write("def/a.lfy", "trait t: `A t` {}\nd A is t {}\n");
    let session = fixture.session();
    assert_eq!(problems(&session, None).unwrap(), "no problems");
    let described = entity(&session, "A").unwrap();
    assert!(described.contains("## Traits\nt\n"), "{described}");
}

// Every field of the hover has a heading, `Hover.owner` among them: a member found as
// `A.x` is headed by the declaration that declares it.
// @lfy def/mcp/main.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
#[test]
fn entity_names_the_owner_of_a_member() {
    let fixture = Fixture::new();
    fixture.write("def/a.lfy", "d A { $x: `An x` = string; }\n");
    let session = fixture.session();
    let described = entity(&session, "A.x").unwrap();
    assert!(described.contains("## Owner\nA\n"), "{described}");
    // A declaration that nothing owns has no such heading.
    assert!(!entity(&session, "A").unwrap().contains("## Owner"));
}

// @lfy def/mcp/main.lfy:entity#entity:entity:0c69bca0860a743d038fb5c4f6c609c4ded6631ac9cf95f37cccba908854cbcd
#[test]
fn several_entities_are_separated_by_a_rule_and_headed_by_their_file() {
    let fixture = Fixture::new();
    fixture.write("def/a.lfy", "d A {}\n").write("def/b.lfy", "d A {}\n");
    let session = fixture.session();
    let described = entity(&session, "A").unwrap();
    assert!(described.contains("\n---\n"), "{described}");
    assert!(described.contains("(def/a.lfy:1)") && described.contains("(def/b.lfy:1)"), "{described}");
    let narrowed = entity(&session, "def/b.lfy:A").unwrap();
    assert!(!narrowed.contains("\n---\n"), "{narrowed}");
    assert!(narrowed.contains("(def/b.lfy:1)"), "{narrowed}");
}

// @lfy def/mcp/main.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
#[test]
fn outline_indents_declarations_or_lists_the_files() {
    let fixture = Fixture::new();
    fixture.write("def/a.lfy", "/// Doc\nd A { $x = string; }\nconst y = 1;\n");
    let session = fixture.session();
    assert_eq!(outline(&session, "def/a.lfy").unwrap(), "data A 2\n  member x 2\nvariable y 3");
    // @lfy def/mcp/main.lfy:outline#outline:outline:a024fe62db1e31cd5f20fef57d6d0cc38073b0724bb0ecf3e828e91bf7d495d3
    let error = outline(&session, "def/zzz.lfy").unwrap_err();
    assert!(error.contains("def/zzz.lfy is not in the program") && error.contains("def/a.lfy"), "{error}");
}

// @lfy def/mcp/main.lfy:grammar#grammar:grammar:51bcd7292055364b6387726881c5443879b09b36eae8e1534b8d84a84effc72f
#[test]
fn grammar_holds_the_terminals_and_the_rules() {
    let fixture = Fixture::new();
    let session = fixture.session();
    let text = grammar(&session).unwrap();
    assert!(text.contains("SourceFile"), "{text}");
    assert!(text.contains("\n\n"), "{text}");
    assert_eq!(
        text,
        format!("{}\n\n{}", elfie_core::grammar::terminal_document(), elfie_core::grammar::grammar_document())
    );
}
