// @lfy def/generation/main.lfy:accept
//! Tests of `accept`.

use super::*;

/// One file holding entities A and B, both built.
fn a_with_two_entities() -> Fixture {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "\n\n/// A.\nd A {\n  $x = string;\n}\n\nd B {\n  $y = string;\n}\n",
    );
    fixture
}

fn named_output(text: &str) -> Output {
    Output {
        path: "src/a.rs".to_string(),
        text: text.to_string(),
    }
}

/// The plan, the request, and the unit index of `def/a.lfy`.
fn one_unit(program: &Program) -> (Plan, Request, usize) {
    let plan = plan_of(program, &[], &[], &[]);
    let (index, _) = unit_of(&program.workspace, &plan, "def/a.lfy");
    let batch = Batch {
        units: vec![index],
        identifier: "a".to_string(),
    };
    let request = request(program, &plan, &batch, &[], &BTreeMap::new(), false, &[]);
    (plan, request, index)
}

/// An entity moved into a unit is marked with the file that declares it, so a marker
/// naming that file covers it and the ids such a marker spells are this unit's.
// @lfy def/generation/main.lfy:accept#accept:accept:2a90cbf7112a647235b9f1c81167c5d44301cbc8369b5d4cc2973be6102fea26
// @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
#[test]
fn a_moved_entity_is_marked_with_the_file_that_declares_it() {
    let fixture = a_cycle();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, unit) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(names(&workspace, &unit.entities), ["B", "C"]);
    let batch = Batch {
        units: vec![index],
        identifier: "b".to_string(),
    };
    let request = request(&program, &plan, &batch, &[], &BTreeMap::new(), false, &[]);
    let output = Output {
        path: "src/b.rs".to_string(),
        text: "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/a.lfy:C\npub struct C;\n"
            .to_string(),
    };
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        std::slice::from_ref(&output),
        &[],
        &[],
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    assert_eq!(
        verdict.source_maps[0]
            .markers
            .iter()
            .map(|marker| (marker.file.clone(), marker.entity.clone()))
            .collect::<Vec<_>>(),
        [
            ("def/b.lfy".to_string(), Some("B".to_string())),
            ("def/a.lfy".to_string(), Some("C".to_string())),
        ]
    );

    // The id a marker naming the file of a moved entity spells is checked against this
    // unit's criteria and tests. @lfy def/generation/main.lfy:accept
    let wrong = Output {
        path: "src/b.rs".to_string(),
        text: "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/a.lfy:C#C:C:ffff\npub struct C;\n"
            .to_string(),
    };
    let verdict = accept(&program, &plan, &request, index, &[wrong], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(verdict.problems[0].contains("C:C:ffff"), "{:?}", verdict.problems);

    // A line beyond the last line of the file declaring a moved entity is a problem of
    // this unit's, just as one beyond the last line of its own file is: both files are
    // this unit's to mark.
    // @lfy def/generation/main.lfy:accept#accept:accept:032dc250d174c0232dd914cac293ffeb3f0494c8065247e29aaa7d0ff849dced
    let beyond = Output {
        path: "src/b.rs".to_string(),
        text: "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/a.lfy:C\npub struct C;\n\n// @lfy def/a.lfy:900\npub struct Extra;\n"
            .to_string(),
    };
    let verdict = accept(&program, &plan, &request, index, &[beyond], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].contains("line 900 of def/a.lfy"),
        "{:?}",
        verdict.problems
    );
}

// @lfy def/generation/main.lfy:accept#accept:accept:499eff9789d84e951586fdadbe7b46cae6dc863092bfb6f280bd8ab26aba5483
#[test]
fn outputs_with_a_marker_per_entity_are_accepted_with_one_source_map() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let (plan, request, index) = one_unit(&program);
    assert_eq!(names(&workspace, &plan.units[index].entities), ["A", "B"]);
    let output = named_output(
        "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/a.lfy:B\npub struct B;\n",
    );
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        std::slice::from_ref(&output),
        &[],
        &[],
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    assert!(verdict.problems.is_empty());
    assert_eq!(verdict.source_maps.len(), 1);
    // A marker that names an entity is left as written.
    // @lfy def/generation/main.lfy:accept#accept:accept:3464ba36f8e1e5d762286978e6be41278e751aded80a6bf152a33a3c4c2cb3a4
    assert_eq!(verdict.outputs, std::slice::from_ref(&output));
    // One source map per output, in `outputs` order.
    // @lfy def/generation/main.lfy:accept#accept:accept:30a65355d64f16689cede881188462ef9f94082f48857623e59af57399ad7503
    let map = &verdict.source_maps[0];
    // @lfy def/generation/main.lfy:accept#accept:accept:8d2a3aa698445970d859538af521c42b878c576deae6eac97af3a97c5c7fd25f
    assert_eq!(map.target, "rust");
    // @lfy def/generation/main.lfy:accept#accept:accept:ea45d8dd2770ae8698006fbdd6aa54ebb0af32fabeb5d9edf8a1a432812d7d7c
    assert_eq!(map.output, "src/a.rs");
    // @lfy def/generation/main.lfy:accept#accept:accept:b27edcf9cd5b896cb993b81cba1f8faa47d79995f135c37e63d5e9ab648eed77
    assert_eq!(map.source, "def/a.lfy");
    // Every hash of the source map is derived here, never taken from the compiler's text.
    // @lfy def/generation/main.lfy:accept#accept:accept:1f423d0a0d06e5fc1e0ddf109f2e402b9a84537a80a91e6e4e459fb24ac288dc
    // @lfy def/generation/main.lfy:accept#accept:accept:90bf10cc4db0c256646cd0f1ba8c45bda247489103c46e1ce26289709829903a
    assert_eq!(map.hash, source_hash(&request.sources["def/a.lfy"]));
    assert_eq!(map.hash, source_hash(&plan.units[index].text));
    // @lfy def/generation/main.lfy:accept#accept:accept:0bcfc0b4637d084c09be1b3c407696d0b16f9c7f082c7f0d91de9ea2950d2902
    assert_eq!(map.answered, answered_ids(&program, &plan.units[index]));
    assert!(map.answered.windows(2).all(|pair| pair[0] <= pair[1]));
    // @lfy def/generation/main.lfy:accept#accept:accept:4a6c75702e77d3fab75aa1d127f35eaad993c997ed4cd0dd14f01162c019bc21
    assert_eq!(
        map.signature,
        source_hash(&interface_of(&workspace, &plan.units[index]))
    );
    // The text hash is of the unit's own region text of the output as `Verdict.outputs`
    // holds it: both regions are this unit's, and of each line only the code is kept, so
    // the marker comments and the blank line between them are no part of it.
    // @lfy def/generation/main.lfy:accept#accept:accept:a31f0ba80f83a7d71e25224d7fa0612b14b3e59805cfb780c1293591243c7b01
    assert_eq!(
        map.text,
        source_hash(&own_region_text(
            &workspace,
            &plan,
            index,
            "//",
            &verdict.outputs[0].text,
            &map.markers,
        ))
    );
    assert_eq!(map.text, source_hash("pub struct A;\npub struct B;\n"));
    // @lfy def/generation/main.lfy:accept#accept:accept:d9879de703d6314164058495241caf82738d58c8f39595b07a19ace78cdbd758
    assert!(map.dependencies.is_empty());
    assert!(plan.units[index].dependencies.is_empty());
    // @lfy def/generation/main.lfy:accept#accept:accept:95161566ef61156ea6f857f212fb510a3edf6221a4238a87b45595f60b6466bb
    assert!(parse_rfc3339(&map.generated).is_some(), "{}", map.generated);
    // The lines are derived from the model, never copied from the compiler.
    assert_eq!(
        map.markers
            .iter()
            .map(|marker| (marker.entity.clone(), marker.line))
            .collect::<Vec<_>>(),
        [(Some("A".to_string()), 4), (Some("B".to_string()), 8)]
    );
    // A marker followed by another ends on the line before it.
    // @lfy def/generation/main.lfy:accept#accept:accept:e4b7f22f09d4b3bbbc78c4edf8763f1f23ff76f5f4e5c858304e8315e8fe30aa
    assert_eq!((map.markers[0].output_line, map.markers[0].end), (1, 3));
    // The last marker of an output ends on its last line.
    // @lfy def/generation/main.lfy:accept
    assert_eq!((map.markers[1].output_line, map.markers[1].end), (4, 5));
    // So the regions partition the output from its first marker on.
    // @lfy def/generation/main.lfy:accept
    assert_eq!(
        map.markers
            .iter()
            .map(|marker| (marker.output_line, marker.end))
            .collect::<Vec<_>>(),
        [(1, 3), (4, 5)]
    );
    assert!(
        map.markers
            .windows(2)
            .all(|pair| pair[0].end + 1 == pair[1].output_line),
        "{:?}",
        map.markers
    );
    // The source map round-trips through the plan: the unit is now up to date.
    let plan = plan_of(&program, &verdict.source_maps, &[], &[]);
    assert_eq!(plan.units[index].reason, None);
}

/// A batch compiled in a copy of the root is reviewed in that copy: the regions the
/// verifier is shown are read from the directory given, and from the workspace root when
/// none is.
// @lfy def/generation/main.lfy:review#review:review:1c072387bd071f939c216da6d4ce6239cccd17ffa2df2f585223778cc3680fca
#[test]
fn the_regions_of_a_review_are_read_from_the_directory_given() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write(
            "def/a.lfy",
            "d A: `An a` {\n  @acceptanceCriteria.add({ behavior = `it holds` });\n}\n",
        )
        .write("src/a.rs", "the root\n")
        .write("copy/src/a.rs", "the copy\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let batch = Batch {
        units: vec![index],
        identifier: "a".to_string(),
    };
    let maps = vec![SourceMap {
        markers: vec![entity_marker(1, "A", 1, 1)],
        ..map_for("src/a.rs", "def/a.lfy")
    }];
    // The region's code is never quoted, so what the directory decides is the hash the
    // verdict is recorded against: the lines of the output it was judged in.
    let root = review(&program, &plan, &batch, &maps, None, &[]);
    let copied = review(
        &program,
        &plan,
        &batch,
        &maps,
        Some(&fixture.root.join("copy")),
        &[],
    );
    let id = root.asked.first().expect("one criterion").clone();
    assert_eq!(root.hashes[&id], source_hash("the root\n"));
    assert_eq!(copied.hashes[&id], source_hash("the copy\n"));
    assert_eq!(root.instructions, copied.instructions);
}

/// Sharing an output with another unit of the same target is no problem while the
/// regions that unit's recorded map names are all still in it; dropping one of them is a
/// problem naming the path, that unit's source, and the region. A co-owner compiled in
/// the same batch writes its own regions again, so its map claims nothing, and the
/// unit's own recorded map for the same path is what acceptance replaces.
// @lfy def/generation/main.lfy:accept#accept:accept:14f27c5c2a72fd5634bfa39ff79904e5261edd57a78c9b62e1afdfe63c7a7385
// @lfy def/generation/main.lfy:accept#accept:accept:72d604363730e1a186d6ae456843c97ae6a857f121cf28d54e4bb331119e4af4
#[test]
fn a_shared_output_keeps_the_regions_another_units_map_records() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write(
            "def/a.lfy",
            "\n\n/// A.\nd A {\n  $x = string;\n}\n\nd B {\n  $y = string;\n}\n",
        )
        .write("def/c.lfy", "d C {\n  $z = string;\n}\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let (plan, request, index) = one_unit(&program);
    let mine = "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/a.lfy:B\npub struct B;\n";
    let theirs = "\n// @lfy def/c.lfy:C\npub struct C;\n";
    let other = SourceMap {
        markers: vec![Marker {
            output_line: 6,
            file: "def/c.lfy".to_string(),
            entity: Some("C".to_string()),
            line: 1,
            column: None,
            requirement: None,
            end: 7,
        }],
        ..map_for("src/a.rs", "def/c.lfy")
    };
    // The region `def/c.lfy` records is still there, so sharing adds no problem.
    // @lfy def/generation/main.lfy:accept#accept:accept:72d604363730e1a186d6ae456843c97ae6a857f121cf28d54e4bb331119e4af4
    let shared = named_output(&format!("{mine}{theirs}"));
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        std::slice::from_ref(&shared),
        std::slice::from_ref(&other),
        &[],
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // Dropped, it is one problem naming the path, the other source, and the region.
    // @lfy def/generation/main.lfy:accept#accept:accept:14f27c5c2a72fd5634bfa39ff79904e5261edd57a78c9b62e1afdfe63c7a7385
    let dropped = named_output(mine);
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        std::slice::from_ref(&dropped),
        std::slice::from_ref(&other),
        &[],
    );
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    for named in ["src/a.rs", "def/c.lfy", "def/c.lfy:C"] {
        assert!(verdict.problems[0].contains(named), "{:?}", verdict.problems);
    }
    // With `def/c.lfy` in the batch, its map claims nothing: it is being rewritten.
    // @lfy def/generation/main.lfy:accept#accept:accept:72d604363730e1a186d6ae456843c97ae6a857f121cf28d54e4bb331119e4af4
    let (together, _) = unit_of(&workspace, &plan, "def/c.lfy");
    let batch = Batch {
        units: vec![index, together],
        identifier: "a+1".to_string(),
    };
    let both = super::request(&program, &plan, &batch, &[], &BTreeMap::new(), false, &[]);
    let verdict = accept(
        &program,
        &plan,
        &both,
        index,
        std::slice::from_ref(&dropped),
        std::slice::from_ref(&other),
        &[],
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // The unit's own recorded map for the same output is what acceptance replaces.
    // @lfy def/generation/main.lfy:accept
    let own = map_for("src/a.rs", "def/a.lfy");
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        std::slice::from_ref(&dropped),
        std::slice::from_ref(&own),
        &[],
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // A region of this unit's own file is in the co-owner's map only because a map holds
    // every marker of its output; it is this unit's to drop, so dropping B is no problem
    // of sharing. @lfy def/generation/main.lfy:accept
    let theirs_only = SourceMap {
        markers: vec![
            Marker {
                output_line: 4,
                file: "def/a.lfy".to_string(),
                entity: Some("B".to_string()),
                line: 8,
                column: None,
                requirement: None,
                end: 5,
            },
            Marker {
                output_line: 6,
                file: "def/c.lfy".to_string(),
                entity: Some("C".to_string()),
                line: 1,
                column: None,
                requirement: None,
                end: 7,
            },
        ],
        ..map_for("src/a.rs", "def/c.lfy")
    };
    let without_b = named_output(&format!(
        "// @lfy def/a.lfy:A\npub struct A;\npub struct B;\n{theirs}"
    ));
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        std::slice::from_ref(&without_b),
        std::slice::from_ref(&theirs_only),
        &[],
    );
    assert_eq!(
        verdict
            .problems
            .iter()
            .filter(|problem| problem.contains("is shared with"))
            .count(),
        0,
        "{:?}",
        verdict.problems
    );
}

/// A project of `def/a.lfy`, which declares `A` with a member `f`, and `def/b.lfy`, which
/// declares `B`, with the plan, the request, and the unit index of `def/b.lfy`.
fn b_beside_a(fixture: &Fixture) -> (Program, Plan, Request, usize) {
    fixture
        .write("def/a.lfy", "d A {\n  $f = string;\n}\n")
        .write("def/b.lfy", "d B {\n  $y = string;\n}\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, _) = unit_of(&workspace, &plan, "def/b.lfy");
    let batch = Batch {
        units: vec![index],
        identifier: "b".to_string(),
    };
    let request = request(&program, &plan, &batch, &[], &BTreeMap::new(), false, &[]);
    (program, plan, request, index)
}

/// Splitting a shared module moves a co-owner's region into a submodule file of the unit,
/// where it is counted all the same, so both outputs earn a source map.
// @lfy def/generation/main.lfy:accept#accept:accept:10e6e70a3330f82d7742ca3f211796f061eb9cc322a79d280c4c4e84b177e156
#[test]
fn a_co_owners_region_is_counted_across_the_outputs_written() {
    let fixture = Fixture::with_rust_target();
    let (program, plan, request, index) = b_beside_a(&fixture);
    let other = SourceMap {
        markers: vec![
            marker_of("def/a.lfy", "A", 1, 2),
            marker_of("def/b.lfy", "B", 3, 4),
        ],
        ..map_for("src/mod.rs", "def/a.lfy")
    };
    let module = Output {
        path: "src/mod.rs".to_string(),
        text: "// @lfy def/b.lfy:B\npub struct B;\n".to_string(),
    };
    let part = Output {
        path: "src/mod/part.rs".to_string(),
        text: "// @lfy def/b.lfy:B\nimpl B {}\n// @lfy def/a.lfy:A\npub struct A;\n"
            .to_string(),
    };
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        &[module, part],
        std::slice::from_ref(&other),
        &[],
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // @lfy def/generation/main.lfy:accept#accept:accept:30a65355d64f16689cede881188462ef9f94082f48857623e59af57399ad7503
    assert_eq!(
        outputs_of(&verdict.source_maps),
        ["src/mod.rs", "src/mod/part.rs"]
    );
}

/// A file of `elsewhere` is read for its markers alone: it holds the co-owner's region, so
/// nothing is lost, and it earns no source map of its own. With no marker in it the region
/// is gone, and the problem names the path, the other source, the region, and both counts.
// @lfy def/generation/main.lfy:accept#accept:accept:d1de8415e4bd6d8cde87018eebbc29ae9bdc66a5087f76e672baa866cf680bbc
// @lfy def/generation/main.lfy:accept#accept:accept:c47da6d261757f3bb9953d11fbbb65c9bec16903f913ebe579a4c997553b5b13
#[test]
fn a_file_of_elsewhere_is_read_for_its_markers_and_earns_no_source_map() {
    let fixture = Fixture::with_rust_target();
    let (program, plan, request, index) = b_beside_a(&fixture);
    let other = SourceMap {
        markers: vec![
            marker_of("def/a.lfy", "A", 1, 2),
            marker_of("def/b.lfy", "B", 3, 4),
        ],
        ..map_for("src/mod.rs", "def/a.lfy")
    };
    let module = Output {
        path: "src/mod.rs".to_string(),
        text: "// @lfy def/b.lfy:B\npub struct B;\n".to_string(),
    };
    // @lfy def/generation/main.lfy:accept#accept:accept:3735db25617f44c846b04cc15709baf47cbf8bfaeecebcf0e6adb99a577d2af4
    let moved = Output {
        path: "src/a.rs".to_string(),
        text: "// @lfy def/a.lfy:A\npub struct A;\n".to_string(),
    };
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        std::slice::from_ref(&module),
        std::slice::from_ref(&other),
        std::slice::from_ref(&moved),
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    assert_eq!(outputs_of(&verdict.source_maps), ["src/mod.rs"]);
    assert_eq!(
        verdict.outputs.iter().map(|output| &output.path).collect::<Vec<_>>(),
        ["src/mod.rs"]
    );

    let empty = Output {
        path: "src/a.rs".to_string(),
        text: "pub struct A;\n".to_string(),
    };
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        std::slice::from_ref(&module),
        std::slice::from_ref(&other),
        std::slice::from_ref(&empty),
    );
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    for named in ["src/mod.rs", "def/a.lfy", "def/a.lfy:A", " 0 ", " 1 "] {
        assert!(verdict.problems[0].contains(named), "{:?}", verdict.problems);
    }
}

/// A region the co-owner's maps record twice must be among the target's files twice, so a
/// file holding it once is one short, and both counts are named.
// @lfy def/generation/main.lfy:accept#accept:accept:831b6353380d6f04883110539c89e60a16918ff659f1da4acb2b62f1d344d94d
#[test]
fn a_region_two_of_a_co_owners_maps_record_is_counted_in_both() {
    let fixture = Fixture::with_rust_target();
    let (program, plan, request, index) = b_beside_a(&fixture);
    let recorded = [
        SourceMap {
            markers: vec![
                marker_of("def/a.lfy", "A", 1, 2),
                marker_of("def/b.lfy", "B", 3, 4),
            ],
            ..map_for("src/lib.rs", "def/a.lfy")
        },
        SourceMap {
            markers: vec![marker_of("def/a.lfy", "A", 1, 2)],
            ..map_for("src/a.rs", "def/a.lfy")
        },
    ];
    let root = Output {
        path: "src/lib.rs".to_string(),
        text: "// @lfy def/b.lfy:B\npub struct B;\n".to_string(),
    };
    let moved = Output {
        path: "src/a.rs".to_string(),
        text: "// @lfy def/a.lfy:A\npub struct A;\n".to_string(),
    };
    let verdict = accept(
        &program,
        &plan,
        &request,
        index,
        std::slice::from_ref(&root),
        &recorded,
        std::slice::from_ref(&moved),
    );
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    for named in ["src/lib.rs", "def/a.lfy", "def/a.lfy:A", " 1 ", " 2 "] {
        assert!(verdict.problems[0].contains(named), "{:?}", verdict.problems);
    }
}

// @lfy def/generation/main.lfy:accept#accept:accept:dd9767e93f188ad433adfb81d2b14e5bcc518b6e2d80e9ecca17d5a03a2f1ab3
#[test]
fn an_entity_without_a_marker_is_rejected_by_name() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    // A marker on the documentation line of A counts for A; nothing names B.
    let output = named_output("// @lfy def/a.lfy:3\npub struct A;\n");
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    // @lfy def/generation/main.lfy:accept#accept:accept:2a90cbf7112a647235b9f1c81167c5d44301cbc8369b5d4cc2973be6102fea26
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].contains("the entity B"),
        "{}",
        verdict.problems[0]
    );
    assert!(verdict.source_maps.is_empty());
    assert!(verdict.outputs.is_empty());
}

// @lfy def/generation/main.lfy:accept#accept:accept:a3a5cfa7740ebf2dbb44280433dc77dea0ef209c95cea1f7d5ef57ed2c8f471b
#[test]
fn a_marker_naming_a_line_inside_a_declaration_is_rewritten_to_name_it() {
    let fixture = Fixture::with_rust_target();
    // `A` is declared on line 3.
    fixture.write("def/a.lfy", "// a file\n\nd A {\n  $x = string;\n}\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let (plan, request, index) = one_unit(&program);
    assert_eq!(names(&workspace, &plan.units[index].entities), ["A"]);
    let output = named_output("// @lfy def/a.lfy:3\npub struct A;\n");
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // @lfy def/generation/main.lfy:accept#accept:accept:f0f63ff3da1a2ceca7ecc1a89adc26e6a86649f0beed8e94ccc5726914eb4c4b
    assert_eq!(
        verdict.outputs[0].text,
        "// @lfy def/a.lfy:A\npub struct A;\n"
    );
    assert_eq!(
        verdict.source_maps[0].markers[0].entity.as_deref(),
        Some("A")
    );
    assert_eq!(verdict.source_maps[0].markers[0].line, 3);
    // The text hash is of the unit's own region text of the output, of which a marker
    // comment is no part, so normalizing a marker from a line to a name leaves it as it
    // was and sends nothing back to review.
    // @lfy def/generation/main.lfy:accept#accept:accept:a31f0ba80f83a7d71e25224d7fa0612b14b3e59805cfb780c1293591243c7b01
    assert_eq!(
        verdict.source_maps[0].text,
        source_hash("pub struct A;\n")
    );
    // A line inside a member's declaration names the member.
    let output = named_output(
        "// @lfy def/a.lfy:3\npub struct A {\n  // @lfy def/a.lfy:4\n  x: String,\n}\n",
    );
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    assert!(
        verdict.outputs[0].text.contains("// @lfy def/a.lfy:A.x"),
        "{}",
        verdict.outputs[0].text
    );
}

/// Every line a source map derives is the model's: a marker on A's documentation line
/// records the line A is declared on rather than the number written, and a marker on
/// B's last line records B's first. A marker on a line no declaration covers names no
/// entity to derive a line from, so it is recorded as written: it names a file of the
/// program, and the map holds every marker that does.
// @lfy def/generation/main.lfy:accept
#[test]
fn the_lines_of_a_source_map_are_derived_and_never_the_compilers() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let (plan, request, index) = one_unit(&program);
    // Line 1 is blank and inside no declaration, line 3 is A's documentation and line 4
    // its declaration, and line 10 closes B, which is declared on line 8.
    let output = named_output(
        "// @lfy def/a.lfy:1\n// @lfy def/a.lfy:3\npub struct A;\n// @lfy def/a.lfy:10\npub struct B;\n",
    );
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // Each marker of the output names a file of the program, so the map holds all three:
    // the two that resolved, with the line of the declaration they named, and the one
    // that resolved to no declaration, with the line it spells and no entity.
    // @lfy def/generation/main.lfy:accept#accept:accept:e4b7f22f09d4b3bbbc78c4edf8763f1f23ff76f5f4e5c858304e8315e8fe30aa
    assert_eq!(
        verdict.source_maps[0]
            .markers
            .iter()
            .map(|marker| (marker.entity.clone(), marker.output_line, marker.line))
            .collect::<Vec<_>>(),
        [
            (None, 1, 1),
            (Some("A".to_string()), 2, 4),
            (Some("B".to_string()), 4, 8)
        ]
    );
    // Each is rewritten to the name the line resolved to, though neither name was
    // spelled with the line the map keeps.
    assert_eq!(
        verdict.outputs[0].text,
        "// @lfy def/a.lfy:1\n// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/a.lfy:B\npub struct B;\n"
    );
}

// @lfy def/generation/main.lfy:accept#accept:accept:4a48dc9bd184c822a38d627cf303e55c050d877bac33bf8d5f8794db459af17d
#[test]
fn no_outputs_are_rejected() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let verdict = accept(&program, &plan, &request, index, &[], &[], &[]);
    assert!(!verdict.accepted);
    assert!(
        verdict.problems[0].contains("no output"),
        "{:?}",
        verdict.problems
    );
}

// @lfy def/generation/main.lfy:accept#accept:accept:324f6bd2c925ffa32b5feee5d17d3195c149deb99f00247918a0ee6d58d1b870
#[test]
fn an_output_outside_the_output_directory_is_rejected_by_path() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let mut output = named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n");
    output.path = "srcx/a.rs".to_string();
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].contains("srcx/a.rs"),
        "{}",
        verdict.problems[0]
    );
}

// @lfy def/generation/main.lfy:accept#accept:accept:e25297fa468e4f2b18f120993ee14b756d7715661e928e3e161f9aba05aad304
#[test]
fn an_output_whose_extension_the_target_does_not_allow_is_rejected_by_path() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    assert_eq!(program.workspace.targets[0].extensions, ["rs"]);
    let (plan, request, index) = one_unit(&program);
    let mut output = named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n");
    output.path = "src/a.txt".to_string();
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].contains("src/a.txt"),
        "{}",
        verdict.problems[0]
    );
    assert!(
        verdict.problems[0].contains("rs"),
        "{}",
        verdict.problems[0]
    );
}

// @lfy def/generation/main.lfy:accept#accept:accept:c9819875941e1d859a6baa6c2043bfaaebeee7923b85b2e2edbd4b88ec00c6f3
#[test]
fn a_marker_naming_an_entity_the_file_does_not_declare_is_rejected_by_output_line_and_name() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let output =
        named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n// @lfy def/a.lfy:Nowhere\n");
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].starts_with("src/a.rs:3:"),
        "{}",
        verdict.problems[0]
    );
    assert!(
        verdict.problems[0].contains("Nowhere"),
        "{}",
        verdict.problems[0]
    );
}

// @lfy def/generation/main.lfy:accept#accept:accept:032dc250d174c0232dd914cac293ffeb3f0494c8065247e29aaa7d0ff849dced
#[test]
fn a_marker_naming_a_line_past_the_last_is_rejected_by_output_line() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let output =
        named_output("// @lfy def/a.lfy:A\n\n// @lfy def/a.lfy:99\n// @lfy def/a.lfy:B\n");
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].starts_with("src/a.rs:3:"),
        "{}",
        verdict.problems[0]
    );
    assert!(
        verdict.problems[0].contains("line 99"),
        "{}",
        verdict.problems[0]
    );
}

/// A marker naming a file the program does not hold is a fixture or prose, not a claim
/// about the program, so it is ignored and not recorded.
// @lfy def/generation/main.lfy:accept#accept:accept:f4b16e912ad6060ea8d1d2f85da81f81022ff64e04b9b347f00579d497e86481
#[test]
fn a_marker_naming_a_file_that_is_not_in_the_program_is_ignored() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let output = named_output(
        "// @lfy def/a.lfy:A\n// @lfy def/b.lfy:8\n// @lfy def/b.lfy:Nowhere\n// @lfy def/a.lfy:B\n",
    );
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // Neither marker of the file outside the program is recorded.
    let markers = &verdict.source_maps[0].markers;
    assert_eq!(
        markers
            .iter()
            .map(|marker| marker.entity.clone())
            .collect::<Vec<_>>(),
        [Some("A".to_string()), Some("B".to_string())]
    );
    // The output is written back with the text of those markers untouched.
    assert!(
        verdict.outputs[0].text.contains("// @lfy def/b.lfy:8"),
        "{}",
        verdict.outputs[0].text
    );
}

// @lfy def/generation/main.lfy:accept#accept:accept:4d6cc733ae9a9a4d4abc942dff1d089398f749e30a393fd194edafe27c85cf95
#[test]
fn an_output_may_hold_markers_for_other_files_of_the_program() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let batch = Batch {
        units: vec![index],
        identifier: "a".to_string(),
    };
    let request = request(&program, &plan,&batch, &[], &BTreeMap::new(), false, &[]);
    let output = named_output(
        "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:B\npub struct B;\n",
    );
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // The marker of the other file is kept, and its line is derived from that file.
    let markers = &verdict.source_maps[0].markers;
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[1].file, "def/b.lfy");
    assert_eq!(markers[1].entity.as_deref(), Some("B"));
    assert_eq!(markers[1].line, 1);
    // @lfy def/generation/main.lfy:accept#accept:accept:d9879de703d6314164058495241caf82738d58c8f39595b07a19ace78cdbd758
    assert_eq!(verdict.source_maps[0].dependencies.len(), 1);
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(
        verdict.source_maps[0].dependencies["def/b.lfy"],
        source_hash(&interface_of(&workspace, b))
    );

    // A marker of another file may name that file's entity and answer for that file's own
    // local criterion: the id belongs to that file's unit, so this unit does not check it.
    // An output shared by two units holds the other unit's markers and ids, and checking
    // them here would make the shared output unacceptable to one of them.
    // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
    let make = *b.entities.last().expect("make is an entity of b");
    let (criteria, _) = requirement_ids(&program.files[b.lowered], make);
    let output = named_output(&format!(
        "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:make#{}\nfn make() {{}}\n",
        criteria[0]
    ));
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // An id no file of the program knows is not checked either, when the marker names
    // another file: only a marker of this unit's own file makes a claim this unit answers
    // for.
    // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
    let output = named_output(
        "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:make#make:make:nowhere\nfn make() {}\n",
    );
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
}

/// A marker of the unit's own file may answer for a local criterion of the unit or for a
/// global one, and for nothing else; the source map records the hash of the unit's
/// requirement ids.
// @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
#[test]
fn a_marker_answering_for_an_unknown_requirement_is_rejected_by_output_line_and_id() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d A: `An a` {\n  @acceptanceCriteria.add({ behavior = `it holds` });\n}\nglobal@acceptanceCriteria.add({ behavior = `nothing is written twice` });\n",
    );
    let program = fixture.program();
    assert_bound(&program.workspace);
    let (plan, request, index) = one_unit(&program);
    let unit = &plan.units[index];
    let (local, _) = requirement_ids(&program.files[unit.lowered], unit.entities[0]);
    assert_eq!(local.len(), 1, "{local:?}");
    let global = program.criteria[0].id.clone();

    // An id of a local criterion and one of a global criterion are both known, so the
    // outputs are accepted and earn their source maps.
    // @lfy def/generation/main.lfy:accept
    let output = named_output(&format!(
        "// @lfy def/a.lfy:A#{}\npub struct A;\n// @lfy def/a.lfy:A#{global}\nfn holds() {{}}\n",
        local[0]
    ));
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // @lfy def/generation/main.lfy:accept#accept:accept:666051c035bda35cafa77995ca9f05915cb5d6f3d9dc74b172f9187d403c1a15
    assert_eq!(
        verdict.source_maps[0].requirements,
        requirements_hash(&program, unit)
    );
    assert_eq!(
        verdict.source_maps[0].hash,
        source_hash(&program.files[unit.lowered].text)
    );

    // An id of neither, on a marker of the unit's own file, is rejected by output line
    // and id.
    // @lfy def/generation/main.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
    let output = named_output("// @lfy def/a.lfy:A#A:A:nowhere\npub struct A;\n");
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].starts_with("src/a.rs:1:"),
        "{}",
        verdict.problems[0]
    );
    assert!(
        verdict.problems[0].contains("A:A:nowhere"),
        "{}",
        verdict.problems[0]
    );
}

/// Acceptance is structural: an output whose text no compiler of the target would accept
/// is accepted all the same, because the guidance says how to build it and the caller
/// runs it.
// @lfy def/generation/main.lfy:accept#accept:accept:9bde11d25fcf12e8c1a113f8be1c479d68185569c8170c39b3348772bf613541
#[test]
fn whether_an_output_builds_is_not_checked_here() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let output = named_output(
        "// @lfy def/a.lfy:A\nthis is not Rust at all (((\n// @lfy def/a.lfy:B\nnor is this\n",
    );
    let verdict = accept(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    assert_eq!(verdict.source_maps.len(), 1);
}

