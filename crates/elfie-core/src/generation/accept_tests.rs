// @lfy def/generation/accept.lfy:accept
//! Tests of `accept`.

use super::*;
use super::accept::{is_any_stub_line, line_diff};

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
    let request = request_for(program, &plan, index);
    (plan, request, index)
}

/// The request for one unit, with nothing handed over of what came before.
fn request_for(program: &Program, plan: &Plan, index: usize) -> Request {
    request(program, plan, index, &[], None, false, &[], &[])
}

/// What `accept` gives when what the files held before the run is not known, so no
/// changed line is judged.
fn judge(
    program: &Program,
    plan: &Plan,
    request: &Request,
    unit: usize,
    outputs: &[Output],
    recorded: &[SourceMap],
    elsewhere: &[Output],
) -> Verdict {
    accept(
        program,
        plan,
        request,
        unit,
        outputs,
        recorded,
        elsewhere,
        None,
        &BTreeMap::new(),
    )
}

/// An entity moved into a unit is marked with the file that declares it, so a marker
/// naming that file covers it and the ids such a marker spells are this unit's.
// @lfy def/generation/accept.lfy:accept#accept:accept:2a90cbf7112a647235b9f1c81167c5d44301cbc8369b5d4cc2973be6102fea26
// @lfy def/generation/accept.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
#[test]
fn a_moved_entity_is_marked_with_the_file_that_declares_it() {
    let fixture = a_cycle();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, unit) = unit_of(&workspace, &plan, "def/b.lfy");
    assert_eq!(names(&workspace, &unit.entities), ["B", "C"]);
    let request = request_for(&program, &plan, index);
    let output = Output {
        path: "src/b.rs".to_string(),
        text: "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/a.lfy:C\npub struct C;\n"
            .to_string(),
    };
    let verdict = judge(
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
    // unit's criteria and tests. @lfy def/generation/accept.lfy:accept
    let wrong = Output {
        path: "src/b.rs".to_string(),
        text: "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/a.lfy:C#C:C:ffff\npub struct C;\n"
            .to_string(),
    };
    let verdict = judge(&program, &plan, &request, index, &[wrong], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(verdict.problems[0].contains("C:C:ffff"), "{:?}", verdict.problems);

    // A line beyond the last line of the file declaring a moved entity is a problem of
    // this unit's, just as one beyond the last line of its own file is: both files are
    // this unit's to mark.
    // @lfy def/generation/accept.lfy:accept#accept:accept:032dc250d174c0232dd914cac293ffeb3f0494c8065247e29aaa7d0ff849dced
    let beyond = Output {
        path: "src/b.rs".to_string(),
        text: "// @lfy def/b.lfy:B\npub struct B;\n\n// @lfy def/a.lfy:C\npub struct C;\n\n// @lfy def/a.lfy:900\npub struct Extra;\n"
            .to_string(),
    };
    let verdict = judge(&program, &plan, &request, index, &[beyond], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].contains("line 900 of def/a.lfy"),
        "{:?}",
        verdict.problems
    );
}

// @lfy def/generation/accept.lfy:accept#accept:accept:bf3fc96705e05d502db0a2644419b38e19666dba765ae89eac34c69c30655edc
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
    let verdict = judge(
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
    // @lfy def/generation/accept.lfy:accept#accept:accept:3464ba36f8e1e5d762286978e6be41278e751aded80a6bf152a33a3c4c2cb3a4
    assert_eq!(verdict.outputs, std::slice::from_ref(&output));
    // One source map per output, in `outputs` order.
    // @lfy def/generation/accept.lfy:accept#accept:accept:30a65355d64f16689cede881188462ef9f94082f48857623e59af57399ad7503
    let map = &verdict.source_maps[0];
    // @lfy def/generation/accept.lfy:accept#accept:accept:8d2a3aa698445970d859538af521c42b878c576deae6eac97af3a97c5c7fd25f
    assert_eq!(map.target, "rust");
    // @lfy def/generation/accept.lfy:accept#accept:accept:ea45d8dd2770ae8698006fbdd6aa54ebb0af32fabeb5d9edf8a1a432812d7d7c
    assert_eq!(map.output, "src/a.rs");
    // @lfy def/generation/accept.lfy:accept#accept:accept:b27edcf9cd5b896cb993b81cba1f8faa47d79995f135c37e63d5e9ab648eed77
    assert_eq!(map.source, "def/a.lfy");
    // Every hash of the source map is derived here, never taken from the compiler's text.
    // @lfy def/generation/accept.lfy:accept#accept:accept:1f423d0a0d06e5fc1e0ddf109f2e402b9a84537a80a91e6e4e459fb24ac288dc
    // @lfy def/generation/accept.lfy:accept#accept:accept:90bf10cc4db0c256646cd0f1ba8c45bda247489103c46e1ce26289709829903a
    assert_eq!(map.hash, source_hash(&request.source));
    assert_eq!(map.hash, source_hash(&plan.units[index].text));
    // @lfy def/generation/accept.lfy:accept#accept:accept:0bcfc0b4637d084c09be1b3c407696d0b16f9c7f082c7f0d91de9ea2950d2902
    assert_eq!(map.answered, answered_ids(&program, &plan.units[index]));
    assert!(map.answered.windows(2).all(|pair| pair[0] <= pair[1]));
    // @lfy def/generation/accept.lfy:accept#accept:accept:4a6c75702e77d3fab75aa1d127f35eaad993c997ed4cd0dd14f01162c019bc21
    assert_eq!(
        map.signature,
        source_hash(&interface_of(&workspace, &plan.units[index]))
    );
    // The text hash is of the unit's own region text of the output as `Verdict.outputs`
    // holds it: both regions are this unit's, and of each line only the code is kept, so
    // the marker comments and the blank line between them are no part of it.
    // @lfy def/generation/accept.lfy:accept#accept:accept:a31f0ba80f83a7d71e25224d7fa0612b14b3e59805cfb780c1293591243c7b01
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
    // @lfy def/generation/accept.lfy:accept#accept:accept:d9879de703d6314164058495241caf82738d58c8f39595b07a19ace78cdbd758
    assert!(map.dependencies.is_empty());
    assert!(plan.units[index].dependencies.is_empty());
    // @lfy def/generation/accept.lfy:accept#accept:accept:95161566ef61156ea6f857f212fb510a3edf6221a4238a87b45595f60b6466bb
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
    // @lfy def/generation/accept.lfy:accept#accept:accept:e4b7f22f09d4b3bbbc78c4edf8763f1f23ff76f5f4e5c858304e8315e8fe30aa
    assert_eq!((map.markers[0].output_line, map.markers[0].end), (1, 3));
    // The last marker of an output ends on its last line.
    // @lfy def/generation/accept.lfy:accept
    assert_eq!((map.markers[1].output_line, map.markers[1].end), (4, 5));
    // So the regions partition the output from its first marker on.
    // @lfy def/generation/accept.lfy:accept
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

/// A unit compiled in a copy of the root is reviewed in that copy: the regions the
/// verifier is shown are read from the directory given, and from the workspace root when
/// none is.
// @lfy def/generation/review.lfy:review#review:review:1c072387bd071f939c216da6d4ce6239cccd17ffa2df2f585223778cc3680fca
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
    let maps = vec![SourceMap {
        markers: vec![entity_marker(1, "A", 1, 1)],
        ..map_for("src/a.rs", "def/a.lfy")
    }];
    // The region's code is never quoted, so what the directory decides is the hash the
    // verdict is recorded against: the lines of the output it was judged in.
    let root = review(&program, &plan, index, &maps, None, &[]);
    let copied = review(
        &program,
        &plan,
        index,
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
/// problem naming the path, that unit's source, and the region. The unit's own recorded
/// map for the same path is what acceptance replaces.
// @lfy def/generation/accept.lfy:accept#accept:accept:1aaaacdd8811d51407ed234af63ea8d8998b836c844531b97748fdce1bbb526d
// @lfy def/generation/accept.lfy:accept#accept:accept:153c7d11fef9fc4164bcdb50d754e6d780760ab490f4f9de86fc5aeb6eaa7b70
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
    // @lfy def/generation/accept.lfy:accept#accept:accept:153c7d11fef9fc4164bcdb50d754e6d780760ab490f4f9de86fc5aeb6eaa7b70
    let shared = named_output(&format!("{mine}{theirs}"));
    let verdict = judge(
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
    // @lfy def/generation/accept.lfy:accept#accept:accept:1aaaacdd8811d51407ed234af63ea8d8998b836c844531b97748fdce1bbb526d
    let dropped = named_output(mine);
    let verdict = judge(
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
    // The unit's own recorded map for the same output is what acceptance replaces.
    // @lfy def/generation/accept.lfy:accept
    let own = map_for("src/a.rs", "def/a.lfy");
    let verdict = judge(
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
    // of sharing. @lfy def/generation/accept.lfy:accept
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
    let verdict = judge(
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
    let request = request_for(&program, &plan, index);
    (program, plan, request, index)
}

/// Splitting a shared module moves a co-owner's region into a submodule file of the unit,
/// where it is counted all the same, so both outputs earn a source map.
// @lfy def/generation/accept.lfy:accept#accept:accept:05909df1f8c1452cdc1c9e6508262ca59120570a3614ebbd27d6ae3278b9c2ec
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
    let verdict = judge(
        &program,
        &plan,
        &request,
        index,
        &[module, part],
        std::slice::from_ref(&other),
        &[],
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // @lfy def/generation/accept.lfy:accept#accept:accept:30a65355d64f16689cede881188462ef9f94082f48857623e59af57399ad7503
    assert_eq!(
        outputs_of(&verdict.source_maps),
        ["src/mod.rs", "src/mod/part.rs"]
    );
}

/// A file of `elsewhere` is read for its markers alone: it holds the co-owner's region, so
/// nothing is lost, and it earns no source map of its own. With no marker in it the region
/// is gone, and the problem names the path, the other source, the region, and both counts.
// @lfy def/generation/accept.lfy:accept#accept:accept:7c77beecc164ae7e0f8d968139a01c753b4d3648c66d978d247d6a690e28f258
// @lfy def/generation/accept.lfy:accept#accept:accept:2eee9403f46d1c92a418f4d939651104b89eeb53a5a71c4da1e87f8c715cd20d
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
    // @lfy def/generation/accept.lfy:accept#accept:accept:f294daed3a09adf31c632ee350bb60d1852500e5b36371b68343a7732c173749
    let moved = Output {
        path: "src/a.rs".to_string(),
        text: "// @lfy def/a.lfy:A\npub struct A;\n".to_string(),
    };
    let verdict = judge(
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
    let verdict = judge(
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
// @lfy def/generation/accept.lfy:accept#accept:accept:35949cfaec8ac36bc0170a4efa086e3d59a7edf0d2decb9fea715e362f27caff
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
    let verdict = judge(
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

// @lfy def/generation/accept.lfy:accept#accept:accept:91d657f922af0ef5d5882e33ee27a54ecf9790eda5172e42756a7d428f5e448b
#[test]
fn an_entity_without_a_marker_is_rejected_by_name() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    // A marker on the documentation line of A counts for A; nothing names B.
    let output = named_output("// @lfy def/a.lfy:3\npub struct A;\n");
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
    // @lfy def/generation/accept.lfy:accept#accept:accept:2a90cbf7112a647235b9f1c81167c5d44301cbc8369b5d4cc2973be6102fea26
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

// @lfy def/generation/accept.lfy:accept#accept:accept:c1e527e5cd13bf2055a97e19e738e0a208cdfe33b97931485ac79c0d1c9ecdde
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
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // @lfy def/generation/accept.lfy:accept#accept:accept:f0f63ff3da1a2ceca7ecc1a89adc26e6a86649f0beed8e94ccc5726914eb4c4b
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
    // @lfy def/generation/accept.lfy:accept#accept:accept:a31f0ba80f83a7d71e25224d7fa0612b14b3e59805cfb780c1293591243c7b01
    assert_eq!(
        verdict.source_maps[0].text,
        source_hash("pub struct A;\n")
    );
    // A line inside a member's declaration names the member.
    let output = named_output(
        "// @lfy def/a.lfy:3\npub struct A {\n  // @lfy def/a.lfy:4\n  x: String,\n}\n",
    );
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
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
// @lfy def/generation/accept.lfy:accept
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
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // Each marker of the output names a file of the program, so the map holds all three:
    // the two that resolved, with the line of the declaration they named, and the one
    // that resolved to no declaration, with the line it spells and no entity.
    // @lfy def/generation/accept.lfy:accept#accept:accept:e4b7f22f09d4b3bbbc78c4edf8763f1f23ff76f5f4e5c858304e8315e8fe30aa
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

// @lfy def/generation/accept.lfy:accept#accept:accept:4a48dc9bd184c822a38d627cf303e55c050d877bac33bf8d5f8794db459af17d
#[test]
fn no_outputs_are_rejected() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let verdict = judge(&program, &plan, &request, index, &[], &[], &[]);
    assert!(!verdict.accepted);
    assert!(
        verdict.problems[0].contains("no output"),
        "{:?}",
        verdict.problems
    );
}

// @lfy def/generation/accept.lfy:accept#accept:accept:324f6bd2c925ffa32b5feee5d17d3195c149deb99f00247918a0ee6d58d1b870
#[test]
fn an_output_outside_the_output_directory_is_rejected_by_path() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let mut output = named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n");
    output.path = "srcx/a.rs".to_string();
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(!verdict.accepted);
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].contains("srcx/a.rs"),
        "{}",
        verdict.problems[0]
    );
}

// @lfy def/generation/accept.lfy:accept#accept:accept:e25297fa468e4f2b18f120993ee14b756d7715661e928e3e161f9aba05aad304
#[test]
fn an_output_whose_extension_the_target_does_not_allow_is_rejected_by_path() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    assert_eq!(program.workspace.targets[0].extensions, ["rs"]);
    let (plan, request, index) = one_unit(&program);
    let mut output = named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n");
    output.path = "src/a.txt".to_string();
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
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

// @lfy def/generation/accept.lfy:accept#accept:accept:c9819875941e1d859a6baa6c2043bfaaebeee7923b85b2e2edbd4b88ec00c6f3
#[test]
fn a_marker_naming_an_entity_the_file_does_not_declare_is_rejected_by_output_line_and_name() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let output =
        named_output("// @lfy def/a.lfy:A\n// @lfy def/a.lfy:B\n// @lfy def/a.lfy:Nowhere\n");
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
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

// @lfy def/generation/accept.lfy:accept#accept:accept:032dc250d174c0232dd914cac293ffeb3f0494c8065247e29aaa7d0ff849dced
#[test]
fn a_marker_naming_a_line_past_the_last_is_rejected_by_output_line() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let output =
        named_output("// @lfy def/a.lfy:A\n\n// @lfy def/a.lfy:99\n// @lfy def/a.lfy:B\n");
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
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
// @lfy def/generation/accept.lfy:accept#accept:accept:f4b16e912ad6060ea8d1d2f85da81f81022ff64e04b9b347f00579d497e86481
#[test]
fn a_marker_naming_a_file_that_is_not_in_the_program_is_ignored() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let output = named_output(
        "// @lfy def/a.lfy:A\n// @lfy def/b.lfy:8\n// @lfy def/b.lfy:Nowhere\n// @lfy def/a.lfy:B\n",
    );
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
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

// @lfy def/generation/accept.lfy:accept#accept:accept:4d6cc733ae9a9a4d4abc942dff1d089398f749e30a393fd194edafe27c85cf95
#[test]
fn an_output_may_hold_markers_for_other_files_of_the_program() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let request = request_for(&program, &plan, index);
    let output = named_output(
        "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:B\npub struct B;\n",
    );
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // The marker of the other file is kept, and its line is derived from that file.
    let markers = &verdict.source_maps[0].markers;
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[1].file, "def/b.lfy");
    assert_eq!(markers[1].entity.as_deref(), Some("B"));
    assert_eq!(markers[1].line, 1);
    // @lfy def/generation/accept.lfy:accept#accept:accept:d9879de703d6314164058495241caf82738d58c8f39595b07a19ace78cdbd758
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
    // @lfy def/generation/accept.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
    let make = *b.entities.last().expect("make is an entity of b");
    let (criteria, _) = requirement_ids(&program.files[b.lowered], make);
    let output = named_output(&format!(
        "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:make#{}\nfn make() {{}}\n",
        criteria[0]
    ));
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // An id no file of the program knows is not checked either, when the marker names
    // another file: only a marker of this unit's own file makes a claim this unit answers
    // for.
    // @lfy def/generation/accept.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
    let output = named_output(
        "// @lfy def/a.lfy:A\npub struct A;\n\n// @lfy def/b.lfy:make#make:make:nowhere\nfn make() {}\n",
    );
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
}

/// A marker of the unit's own file may answer for a local criterion of the unit or for a
/// global one, and for nothing else; the source map records the hash of the unit's
/// requirement ids.
// @lfy def/generation/accept.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
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
    // @lfy def/generation/accept.lfy:accept
    let output = named_output(&format!(
        "// @lfy def/a.lfy:A#{}\npub struct A;\n// @lfy def/a.lfy:A#{global}\nfn holds() {{}}\n",
        local[0]
    ));
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // @lfy def/generation/accept.lfy:accept#accept:accept:666051c035bda35cafa77995ca9f05915cb5d6f3d9dc74b172f9187d403c1a15
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
    // @lfy def/generation/accept.lfy:accept#accept:accept:74f035d081f6ce720abe1fc4c5fcf290ac28f50f8c6b57ea614ec91de0e3e075
    let output = named_output("// @lfy def/a.lfy:A#A:A:nowhere\npub struct A;\n");
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
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
// @lfy def/generation/accept.lfy:accept#accept:accept:9bde11d25fcf12e8c1a113f8be1c479d68185569c8170c39b3348772bf613541
#[test]
fn whether_an_output_builds_is_not_checked_here() {
    let fixture = a_with_two_entities();
    let program = fixture.program();
    let (plan, request, index) = one_unit(&program);
    let output = named_output(
        "// @lfy def/a.lfy:A\nthis is not Rust at all (((\n// @lfy def/a.lfy:B\nnor is this\n",
    );
    let verdict = judge(&program, &plan, &request, index, &[output], &[], &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    assert_eq!(verdict.source_maps.len(), 1);
    // What the files held before the run is not known, so no changed line is judged.
    // @lfy def/generation/accept.lfy:accept#accept:accept:b2a0c24659e730b5b7223cdf8ed1537a82470cbd637d779824236f5a354d9481
    assert!(verdict.foreign_edits.is_empty(), "{:?}", verdict.foreign_edits);
}

// -----------------------------------------------------------------------------------
// what a run changes outside its outputs
// -----------------------------------------------------------------------------------

/// `def/b.lfy` declaring `B`, and `def/a.lfy` using it and declaring `A` and `f`, both with
/// outputs recorded, so `a` comes after `b` and is in its reach; `stubbed` sets a stub line
/// on the target's language.
struct LaterUnit {
    _fixture: Fixture,
    program: Program,
    plan: Plan,
    a: usize,
    b: usize,
}

fn b_then_a(stubbed: bool) -> LaterUnit {
    let fixture = Fixture::with_rust_target();
    if stubbed {
        fixture.write("def/roles.lfy", STUBBED_ROLES);
    }
    fixture
        .write("def/b.lfy", "d B {\n  $x = string;\n}\n")
        .write(
            "def/a.lfy",
            "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nfn f(): `Does f` => string {\n  \
             @acceptanceCriteria.add({ behavior = `it does` });\n}\n",
        );
    let program = fixture.program();
    assert_bound(&program.workspace);
    let mut maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    // `a` was accepted against another interface of `b`, so `a` is in `b`'s reach.
    for map in maps.iter_mut().filter(|map| map.source == "def/a.lfy") {
        map.dependencies.values_mut().for_each(String::clear);
    }
    let plan = plan_of(&program, &maps, &[], &[]);
    let (a, _) = unit_of(&program.workspace, &plan, "def/a.lfy");
    let (b, _) = unit_of(&program.workspace, &plan, "def/b.lfy");
    LaterUnit {
        _fixture: fixture,
        program,
        plan,
        a,
        b,
    }
}

/// `src/a.rs` as `a` was accepted: `A`, then `f` with a body of twenty-one lines.
fn a_as_it_was() -> String {
    let mut text = String::from(
        "// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/a.lfy:f\npub fn f() -> String {\n",
    );
    for line in 0..20 {
        text.push_str(&format!("    let l{line} = {line};\n"));
    }
    text.push_str("    String::new()\n}\n");
    text
}

/// `src/a.rs` with the first `count` lines of `f`'s body rewritten.
fn a_with_lines_rewritten(count: usize) -> String {
    let mut text = a_as_it_was();
    for line in 0..count {
        text = text.replace(
            &format!("    let l{line} = {line};\n"),
            &format!("    let l{line} = {line} + 1;\n"),
        );
    }
    text
}

fn file(path: &str, text: &str) -> Output {
    Output {
        path: path.to_string(),
        text: text.to_string(),
    }
}

const B_OUTPUT: &str = "// @lfy def/b.lfy:B\npub struct B;\n";

/// What `accept` gives for `b`'s run, which wrote `src/b.rs` as it was and left `src/a.rs`
/// as `after`, with `touches` the notes it gave.
fn judge_b(later: &LaterUnit, after: &str, touches: &[(&str, &str)]) -> Verdict {
    let request = request_for(&later.program, &later.plan, later.b);
    assert_eq!(request.reach.units, [later.a]);
    let recorded = [SourceMap {
        markers: vec![marker_of("def/a.lfy", "A", 1, 2), marker_of("def/a.lfy", "f", 3, 25)],
        ..map_for("src/a.rs", "def/a.lfy")
    }];
    let touches: BTreeMap<String, String> = touches
        .iter()
        .map(|(stem, note)| (stem.to_string(), note.to_string()))
        .collect();
    accept(
        &later.program,
        &later.plan,
        &request,
        later.b,
        &[file("src/b.rs", B_OUTPUT)],
        &recorded,
        &[file("src/a.rs", after)],
        Some(&[file("src/b.rs", B_OUTPUT), file("src/a.rs", &a_as_it_was())]),
        &touches,
    )
}

// @lfy def/generation/accept.lfy:accept#accept:accept:760bfb191b329495f0caa71ea575a602ccde9a0f7ed463829b0ed5018edfa6a0
#[test]
fn a_stub_in_a_later_units_region_is_one_foreign_edit() {
    let later = b_then_a(true);
    let body: String = (0..20)
        .map(|line| format!("    let l{line} = {line};\n"))
        .chain(["    String::new()\n".to_string()])
        .collect();
    let stubbed = a_as_it_was().replace(
        &body,
        "    todo!(\"elfie stub: b: B.x is now a number\")\n",
    );
    let verdict = judge_b(&later, &stubbed, &[]);
    assert!(verdict.accepted, "{:?}", verdict.problems);
    // @lfy def/generation/accept.lfy:accept#accept:accept:673c8ca4213750333bbe2a3efcbf3c8c4f0effe89de65f4fdb92b2bb2ca0add3
    assert_eq!(verdict.foreign_edits.len(), 1, "{:?}", verdict.foreign_edits);
    let edit = &verdict.foreign_edits[0];
    assert_eq!(edit.by, "b");
    assert_eq!(edit.owner, "a");
    assert_eq!(edit.path, "src/a.rs");
    assert_eq!(edit.region.entity.as_deref(), Some("f"));
    assert_eq!(edit.region.file, "def/a.lfy");
    assert_eq!(edit.before.len(), 24, "{:?}", edit.before);
    assert_eq!(
        edit.after,
        [
            "// @lfy def/a.lfy:f",
            "pub fn f() -> String {",
            "    todo!(\"elfie stub: b: B.x is now a number\")",
            "}"
        ]
    );
    // Every line it adds is a stub line naming b, and the note is its reason; a stub
    // needs no note and has no cap.
    // @lfy def/generation/accept.lfy:accept#accept:accept:8a1e611687ca02dd6ca89d5c5933e46879b0cbd6ee2ef85fc9e53c8848559b6d
    // @lfy def/generation/accept.lfy:accept#accept:accept:4638ec2f3c3da87201f9123091fb0e8e54d466a9b5173afcc85f622fc1b79852
    assert_eq!(edit.kind, ForeignEditKind::Stub);
    assert_eq!(edit.note, "B.x is now a number");
    // The file of elsewhere earns no source map.
    // @lfy def/generation/accept.lfy:accept#accept:accept:f294daed3a09adf31c632ee350bb60d1852500e5b36371b68343a7732c173749
    assert_eq!(outputs_of(&verdict.source_maps), ["src/b.rs"]);
}

// @lfy def/generation/accept.lfy:accept#accept:accept:9f7b35903fce707f9662ffeb1877bcb75e04e4858414503d7674a8745ce0cd7c
#[test]
fn a_small_fix_with_a_note_is_one_foreign_edit() {
    let later = b_then_a(false);
    let verdict = judge_b(
        &later,
        &a_with_lines_rewritten(3),
        &[("a", "B.x is now a number, so f converts it")],
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    assert_eq!(verdict.foreign_edits.len(), 1, "{:?}", verdict.foreign_edits);
    let edit = &verdict.foreign_edits[0];
    // @lfy def/generation/accept.lfy:accept#accept:accept:8e628203f0c8221c728cebe1ce188a0dd58163d7974231ca4498cd36c2a91296
    assert_eq!(edit.kind, ForeignEditKind::Fix);
    assert_eq!(edit.note, "B.x is now a number, so f converts it");
    assert_eq!((edit.by.as_str(), edit.owner.as_str()), ("b", "a"));
    assert_eq!(edit.changed_lines(), 3);

    // A fix that changes the marker beginning the region is refused, however small.
    // @lfy def/generation/accept.lfy:accept#accept:accept:9003974dcb53621de6101c21aa4bf533f452ad27b09cbc1480aacfe7cccae1ea
    let remarked = a_with_lines_rewritten(1).replace(
        "// @lfy def/a.lfy:f\n",
        "// @lfy def/a.lfy:f#f:f:0000\n",
    );
    let verdict = judge_b(&later, &remarked, &[("a", "a note")]);
    assert!(!verdict.accepted);
    assert!(
        verdict
            .problems
            .iter()
            .any(|problem| problem.contains("src/a.rs") && problem.contains("stay as they were")),
        "{:?}",
        verdict.problems
    );
}

// @lfy def/generation/accept.lfy:accept#accept:accept:4d1752d5ed071305cbf850d742138877d670f62f8b76a6ca4c7ed46bcff7487f
#[test]
fn a_fix_with_no_note_is_rejected() {
    let later = b_then_a(false);
    let verdict = judge_b(&later, &a_with_lines_rewritten(3), &[]);
    assert!(!verdict.accepted);
    // @lfy def/generation/accept.lfy:accept#accept:accept:41e970a839c8e929087ba7d9bce5cfeef8a74241a4acf8c4553e13080b8b99a2
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(verdict.problems[0].contains(" a "), "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].contains("a fix needs a note given through elfie_touch"),
        "{:?}",
        verdict.problems
    );
    assert!(verdict.foreign_edits.is_empty());
    // An empty note is no note.
    let verdict = judge_b(&later, &a_with_lines_rewritten(3), &[("a", "  ")]);
    assert!(!verdict.accepted);
}

// @lfy def/generation/accept.lfy:accept#accept:accept:a51c35c152bb9d7fb1a7cc4532b92c66ccc81c58ef3a9ebfaa360d07e3bdb59b
#[test]
fn a_fix_over_the_cap_of_one_region_is_rejected() {
    let later = b_then_a(false);
    let verdict = judge_b(&later, &a_with_lines_rewritten(12), &[("a", "a note")]);
    assert!(!verdict.accepted);
    // @lfy def/generation/accept.lfy:accept#accept:accept:74b5b83a0e812b11c206618586026619959622840bbeca19fa01551901d0dbee
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    for named in ["src/a.rs", "def/a.lfy:f", "12 lines", "cap of 10", "a bigger edit is a stub"] {
        assert!(verdict.problems[0].contains(named), "{named}: {:?}", verdict.problems);
    }
}

// @lfy def/generation/accept.lfy:accept#accept:accept:912b53b5130dde6fac2bf9728a2f8b434fd8beaf27ecf603cd0947f959aea6ed
#[test]
fn an_edit_in_the_code_of_a_unit_that_does_not_depend_on_the_run_is_rejected() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write(
            "def/c.lfy",
            "fn g(): `Does g` => string {\n  @acceptanceCriteria.add({ behavior = `it does` });\n}\n",
        )
        .write("def/b.lfy", "use \"./c\";\n\nd B {\n  $x = string;\n}\n");
    let program = fixture.program();
    assert_bound(&program.workspace);
    let maps = current_maps(&program, &plan_of(&program, &[], &[], &[]));
    let plan = plan_of(&program, &maps, &[], &[]);
    let (b, _) = unit_of(&program.workspace, &plan, "def/b.lfy");
    let request = request_for(&program, &plan, b);
    assert!(request.reach.units.is_empty(), "{:?}", request.reach);
    let recorded = [SourceMap {
        markers: vec![marker_of("def/c.lfy", "g", 1, 3)],
        ..map_for("src/c.rs", "def/c.lfy")
    }];
    let was = "// @lfy def/c.lfy:g\npub fn g() -> String {\n    String::new()\n}\n";
    let now = "// @lfy def/c.lfy:g\npub fn g() -> String {\n    String::from(\"g\")\n}\n";
    let touches = BTreeMap::from([("c".to_string(), "a note".to_string())]);
    let verdict = accept(
        &program,
        &plan,
        &request,
        b,
        &[file("src/b.rs", B_OUTPUT)],
        &recorded,
        &[file("src/c.rs", now)],
        Some(&[file("src/b.rs", B_OUTPUT), file("src/c.rs", was)]),
        &touches,
    );
    assert!(!verdict.accepted);
    // @lfy def/generation/accept.lfy:accept#accept:accept:081354ab25d7caa23751b7750efc59d1d5c9ca9fc6b2197450de047cfee648f8
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    for named in [
        "src/c.rs:3",
        " c,",
        "only the code of a unit in its reach, one that depends on it, is edited",
    ] {
        assert!(verdict.problems[0].contains(named), "{named}: {:?}", verdict.problems);
    }
    assert!(verdict.foreign_edits.is_empty());
}

// @lfy def/generation/accept.lfy:accept#accept:accept:d9e2181c8f34c29629fcc36b59d0a59088c54f8a0d1bbf0ecea9f8ede9a163ab
#[test]
fn a_changed_line_in_no_units_code_is_rejected() {
    let later = b_then_a(false);
    let request = request_for(&later.program, &later.plan, later.b);
    let verdict = accept(
        &later.program,
        &later.plan,
        &request,
        later.b,
        &[file("src/b.rs", B_OUTPUT)],
        &[],
        &[file("src/notes.rs", "one\ntwo\n")],
        Some(&[file("src/b.rs", B_OUTPUT), file("src/notes.rs", "one\n")]),
        &BTreeMap::new(),
    );
    assert!(!verdict.accepted);
    // @lfy def/generation/accept.lfy:accept#accept:accept:ce15bdbc6104d9c96f45ecd76aa1c3deb9e2e47c5bb449710162aefd10e3aff2
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].contains("src/notes.rs:2") && verdict.problems[0].contains("no unit's code"),
        "{:?}",
        verdict.problems
    );
}

// @lfy def/generation/accept.lfy:accept#accept:accept:3b23d3899645498521c0590204bbfb195589ccfef754c377f1b8fa75a24921d9
#[test]
fn a_stub_left_in_the_units_own_code_is_rejected() {
    let later = b_then_a(true);
    let request = request_for(&later.program, &later.plan, later.a);
    let output = file(
        "src/a.rs",
        "// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/a.lfy:f\npub fn f() -> String {\n    \
         todo!(\"elfie stub: b: B.x is now a number\")\n}\n",
    );
    let verdict = judge(&later.program, &later.plan, &request, later.a, &[output], &[], &[]);
    assert!(!verdict.accepted);
    // A stub line naming any unit, its white space aside, is one.
    // @lfy def/generation/accept.lfy:accept#accept:accept:851d0e32fd8478618d009c3df456da99f2bb0f1983e8e867a9c3e184ece945c9
    // @lfy def/generation/accept.lfy:accept#accept:accept:5e0b06a7232782f41a1259999162148cc42567b3738ee363433be4f33d64c65f
    assert_eq!(verdict.problems.len(), 1, "{:?}", verdict.problems);
    assert!(
        verdict.problems[0].starts_with("src/a.rs:5:")
            && verdict.problems[0].contains("a stub is left in the unit's code"),
        "{:?}",
        verdict.problems
    );
    assert!(is_any_stub_line("  todo!(\"elfie stub: x/y: why\")  ", STUB_LINE));
    assert!(!is_any_stub_line("todo!(\"something else\")", STUB_LINE));
    // Without a stub line, no line is one.
    // @lfy def/generation/accept.lfy:accept#accept:accept:ec9bf2b28c1f3b0e6144d76c6bcf7912791a9eb3f8d41f1c5f2ef658d041f34a
    let plain = b_then_a(false);
    assert_eq!(
        stub_line_of(&plain.program.workspace.model, &plain.program.workspace.targets[0]),
        None
    );
}

// @lfy def/generation/accept.lfy:accept#accept:accept:a21ceb983c04daf8b46b41ecd490263beae458042f5abd4c18b233317651b5f1
#[test]
fn a_touched_unit_accepted_as_the_fix_left_it_makes_the_fix_its_own() {
    let later = b_then_a(false);
    let request = request_for(&later.program, &later.plan, later.a);
    let fixed = a_with_lines_rewritten(2);
    let verdict = accept(
        &later.program,
        &later.plan,
        &request,
        later.a,
        &[file("src/a.rs", &fixed)],
        &[],
        &[],
        Some(&[file("src/a.rs", &fixed)]),
        &BTreeMap::new(),
    );
    assert!(verdict.accepted, "{:?}", verdict.problems);
    assert!(verdict.foreign_edits.is_empty(), "{:?}", verdict.foreign_edits);
    assert_eq!(outputs_of(&verdict.source_maps), ["src/a.rs"]);
    // The text hash is of a's own regions with the fix in them.
    // @lfy def/generation/accept.lfy:accept#accept:accept:a31f0ba80f83a7d71e25224d7fa0612b14b3e59805cfb780c1293591243c7b01
    let code: String = fixed
        .lines()
        .filter(|line| !line.starts_with("// @lfy"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(verdict.source_maps[0].text, source_hash(&code));
}

/// The line diff keeps a longest common subsequence: a rewritten line is one removed and
/// one added, at the lines each has in its own text.
// @lfy def/generation/accept.lfy:accept#accept:accept:0a1470345711d81632a94e78c934b73d81c67b8564e43fcffb6a91c40b535f73
#[test]
fn the_changed_lines_are_those_a_line_diff_removes_and_adds() {
    assert_eq!(line_diff(&["a", "b", "c"], &["a", "x", "c"]), (vec![2], vec![2]));
    assert_eq!(line_diff(&["a", "b"], &["a", "b", "c"]), (vec![], vec![3]));
    assert_eq!(line_diff(&["a", "b", "c"], &["c"]), (vec![1, 2], vec![]));
    assert_eq!(line_diff(&[], &["a"]), (vec![], vec![1]));
    assert_eq!(line_diff(&["a"], &["a"]), (vec![], vec![]));
}

