// @lfy def/generation/main.lfy:review
//! Tests of `review`, `globalReview`, and `reviewOf`.

use super::*;

/// `def/a.lfy` declaring `A` with two criteria and one test, with `src/a.rs` generated
/// for it, and the plan and the index of its one unit.
fn a_to_review() -> (Fixture, Program, Plan, usize) {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d A: `An a` {\n  @acceptanceCriteria\n    .add({ behavior = `the first` })\n    .add({ behavior = `the second` });\n  @test({ input = 1, expect = 2 });\n}\n",
    );
    fixture.write("src/a.rs", "alpha\nbeta\ngamma\ndelta\n");
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, _) = unit_of(&program.workspace, &plan, "def/a.lfy");
    (fixture, program, plan, index)
}

/// The text of the whole of that project's one output, which the hash of every criterion
/// and test of its unit is taken over.
const A_REGION: &str = "alpha\nbeta\ngamma\ndelta\n";

/// The one source map of that project: `src/a.rs` from `def/a.lfy`, with one marker
/// naming `A` and the whole output as its region.
fn a_source_map() -> Vec<SourceMap> {
    vec![SourceMap {
        target: "rust".to_string(),
        output: "src/a.rs".to_string(),
        source: "def/a.lfy".to_string(),
        hash: source_hash("x"),
        requirements: String::new(),
        answered: Vec::new(),
        signature: source_hash("i"),
        text: source_hash(A_REGION),
        dependencies: BTreeMap::new(),
        generated: "2026-09-18T10:00:00Z".to_string(),
        markers: vec![entity_marker(1, "A", 1, 4)],
    }]
}

/// The ids of the criteria and of the tests of the one entity of that project's unit.
fn a_requirement_ids(program: &Program, plan: &Plan) -> (Vec<String>, Vec<String>) {
    let (_, unit) = unit_of(&program.workspace, plan, "def/a.lfy");
    requirement_ids(&program.files[unit.lowered], unit.entities[0])
}

// @lfy def/generation/main.lfy:review#review:review:1539ee32577f98ca87dae778fb98bd55c8110cc7b0a02be6306d516ef9a4f321
#[test]
fn a_review_request_lists_every_criterion_with_its_place_and_names_every_region() {
    let (_fixture, program, plan, under_review) = a_to_review();
    let maps = a_source_map();
    let request = review(&program, &plan, under_review, &maps, None, &[]);
    // @lfy def/generation/main.lfy:review#review:review:88af78276d6f8df3f4a8999ca87628c8de2e6a194a2718d39aa64e803f157c0d
    assert_eq!(request.unit, Some(under_review));
    let text = &request.instructions;
    // The heading names the unit's stem and the target.
    // @lfy def/generation/main.lfy:review
    assert!(
        text.contains("# Reviewing the unit `a` for the target `rust`"),
        "{text}"
    );
    assert!(text.contains("#### `A` (data: DataDeclaration)"), "{text}");
    assert!(text.contains("Definition: An a"), "{text}");
    // The region is named and none of its lines is written.
    // @lfy def/generation/main.lfy:review#review:review:a4959dc5c79c68106f15ab278c665fa965bf1e3d36b6b1a3e0f5449b3673bac1
    // @lfy def/generation/main.lfy:review#review:review:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
    assert!(text.contains("`src/a.rs:1-4`"), "{text}");
    for line in A_REGION.lines() {
        assert!(!text.contains(line), "{line} is quoted:\n{text}");
    }
    // Each criterion of a chain has a place of its own, on the line its `add` begins
    // on, and is prefixed by its id; a test's place is the line its origin begins on.
    // @lfy def/generation/main.lfy:review#review:review:f3ebae56b7c2144a9edb3e3b69ac07f174e3974f258284e909f2e10068ec9831
    let (criteria, tests) = a_requirement_ids(&program, &plan);
    for (id, place, said) in [
        (&criteria[0], 3, "the first"),
        (&criteria[1], 4, "the second"),
    ] {
        assert!(
            text.contains(&format!("`{id}` def/a.lfy:{place} — {said}")),
            "{text}"
        );
    }
    assert!(
        text.contains(&format!("`{}` def/a.lfy:5 — Input `1` gives `2`", tests[0])),
        "{text}"
    );
    // Only local criteria and tests are reviewed here.
    // @lfy def/generation/main.lfy:review
    assert!(text.contains("A global one is reviewed once"), "{text}");
    // Every criterion and every test is asked about, in the order they are listed, and
    // nothing is carried without a record to carry.
    // @lfy def/generation/main.lfy:review#review:review:79a851646614a7ae02fda5c6fca4946a5e185414f8174d5acb5c40413f0c8033
    assert_eq!(
        request.asked,
        [criteria[0].clone(), criteria[1].clone(), tests[0].clone()]
    );
    assert!(request.carried.is_empty(), "{:?}", request.carried);
    // Each of them is hashed over the lines of the region of the entity it belongs to.
    // @lfy def/generation/main.lfy:review#review:review:cd1ebbdd30f45c288876eb3c3e144180848e744373e0780577158f70511f6d01
    let region = source_hash(A_REGION);
    for id in &request.asked {
        assert_eq!(request.hashes[id], region, "{id}");
    }
    // The protocol: how a region is read, where a verdict goes, the keys of the line it
    // falls back to, the three statuses, the tools, and the end line.
    // @lfy def/generation/data.lfy:ReviewRequest#ReviewRequest:ReviewRequest:5e59c550936dcf7bf2e5d061524a9f594d0fad8ec3bcde714954a0e266066d26
    for part in [
        "examine and never edit",
        "you propose no code",
        "The code of a region is read through `elfie_output`",
        "Submit your verdicts as you judge them, many to a turn",
        "one call of `elfie_reportMany` with one JSON line per verdict",
        "several calls of `elfie_report` in the same turn",
        "Never one verdict per turn",
        "`id` is copied from the id the criterion or test is given above, or left out",
        "the two halves of the place given with it above",
        "`satisfied`",
        "`violated`",
        "`unverifiable`",
        "unverifiable, never violated",
        "before files are searched or read by hand",
        "`elfie_source`",
        "`elfie_search` for the lines of the outputs that match a pattern",
        "the body of a helper no marker names",
        "`elfie_entity`",
        "`elfie_references`",
        "Where `elfie_report` is not offered",
        "one JSON object on one line with exactly the keys `id`, `file`, `line`, \
         `status`, `evidence`, and `note`",
        "`ELFIE: REVIEWED`",
    ] {
        assert!(text.contains(part), "{part} is missing:\n{text}");
    }
}

/// Two regions of one entity that meet — the second beginning on the line after the end
/// of the first, in the same output — are given as one range, while a gap between two of
/// them, and a region of another output, keep them apart.
// @lfy def/generation/main.lfy:review#review:review:a223ed6fa73eed7e47f542016405111d798bc5fd892675f33af0b77d0d8681c6
#[test]
fn two_regions_that_meet_in_one_output_are_given_as_one_range() {
    let (_fixture, program, plan, under_review) = a_to_review();
    let map = |output: &str, markers: Vec<Marker>| SourceMap {
        output: output.to_string(),
        markers,
        ..a_source_map().remove(0)
    };
    let maps = vec![
        map(
            "src/a.rs",
            vec![
                entity_marker(1, "A", 1, 2),
                entity_marker(3, "A", 1, 4),
                entity_marker(6, "A", 1, 7),
            ],
        ),
        map("src/b.rs", vec![entity_marker(8, "A", 1, 9)]),
    ];
    let text = review(&program, &plan, under_review, &maps, None, &[]).instructions;
    assert!(text.contains("`src/a.rs:1-4`"), "{text}");
    assert!(!text.contains("`src/a.rs:1-2`"), "{text}");
    assert!(!text.contains("`src/a.rs:3-4`"), "{text}");
    // Line 5 belongs to no region, so the one beginning on line 6 stands on its own,
    // and so does a region of another output that begins on the next line.
    assert!(text.contains("`src/a.rs:6-7`"), "{text}");
    assert!(text.contains("`src/b.rs:8-9`"), "{text}");
}

/// A criterion whose record says it was satisfied against the code the entity's regions
/// still hash to is carried rather than listed; one whose record says anything else is
/// listed again.
// @lfy def/generation/main.lfy:review#review:review:331030ac2ff8059882ba1d6d951e96c529a0aff56edda0112a546d30912cd9a6
#[test]
fn a_criterion_satisfied_against_the_code_as_it_stands_is_carried() {
    let (_fixture, program, plan, under_review) = a_to_review();
    let maps = a_source_map();
    let (criteria, tests) = a_requirement_ids(&program, &plan);
    let hash = source_hash(A_REGION);
    let records = vec![
        ReviewRecord {
            id: criteria[0].clone(),
            status: ReviewStatus::Satisfied,
            hash: hash.clone(),
        },
        ReviewRecord {
            id: criteria[1].clone(),
            status: ReviewStatus::Unverifiable,
            hash: hash.clone(),
        },
    ];
    let request = review(&program, &plan, under_review, &maps, None, &records);
    // The satisfied one is carried with its record, and only the other two are asked.
    // @lfy def/generation/main.lfy:review#review:review:b69235bec9f185a5ede4305f9d6e8d84ea38a64ab23b11147e9b2436d364a84d
    assert_eq!(request.asked, [criteria[1].clone(), tests[0].clone()]);
    assert_eq!(request.carried, [records[0].clone()]);
    let text = &request.instructions;
    assert!(!text.contains(&criteria[0]), "{text}");
    assert!(text.contains(&criteria[1]), "{text}");
    // Every criterion and test of the unit is hashed, carried or not.
    // @lfy def/generation/main.lfy:review#review:review:cd1ebbdd30f45c288876eb3c3e144180848e744373e0780577158f70511f6d01
    assert_eq!(request.hashes.len(), 3, "{:?}", request.hashes);
    assert_eq!(request.hashes[&criteria[0]], hash);
}

/// A touched unit reviewed with its records in the copy its change set is worked in: the
/// entity whose region another unit's fix changed is listed again, and the one no edit
/// touched is carried.
// @lfy def/generation/main.lfy:review#review:review:67ee9ec99448901eb17cdb7b76de2ade931d541db70a75b4279e12a40b128279
#[test]
fn a_touched_unit_is_reviewed_on_the_entity_the_fix_changed_alone() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d A: `An a` {\n  @acceptanceCriteria.add({ behavior = `A holds` });\n}\n\n\
         d C: `A c` {\n  @acceptanceCriteria.add({ behavior = `C holds` });\n}\n",
    );
    let program = fixture.program();
    assert_bound(&program.workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
    assert_eq!(names(&program.workspace, &unit.entities), ["A", "C"]);
    let ids: Vec<String> = unit
        .requirements(&program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();
    // The copy of the root holds src/a.rs as the fix left it; the root holds nothing.
    let copy = fixture.root.join("copy");
    fs::create_dir_all(copy.join("src")).unwrap();
    fs::write(
        copy.join("src/a.rs"),
        "// @lfy def/a.lfy:A\nfixed one\nfixed two\n// @lfy def/a.lfy:C\nthree\nfour\n",
    )
    .unwrap();
    let maps = vec![SourceMap {
        markers: vec![entity_marker(1, "A", 1, 3), entity_marker(4, "C", 5, 6)],
        ..a_source_map().remove(0)
    }];
    let records = vec![
        ReviewRecord {
            id: ids[0].clone(),
            status: ReviewStatus::Satisfied,
            hash: source_hash("one\ntwo\n"),
        },
        ReviewRecord {
            id: ids[1].clone(),
            status: ReviewStatus::Satisfied,
            hash: source_hash("three\nfour\n"),
        },
    ];
    let request = review(&program, &plan, index, &maps, Some(&copy), &records);
    // @lfy def/generation/main.lfy:review#review:review:79a851646614a7ae02fda5c6fca4946a5e185414f8174d5acb5c40413f0c8033
    assert_eq!(request.asked, [ids[0].clone()]);
    // @lfy def/generation/main.lfy:review#review:review:b69235bec9f185a5ede4305f9d6e8d84ea38a64ab23b11147e9b2436d364a84d
    assert_eq!(request.carried, [records[1].clone()]);
}

/// A marker comment is no part of the hash of a region, and the white space before it
/// goes with it, so a line that holds nothing else adds nothing: moving a reworded
/// criterion's markers to its new id leaves every hash of the entity as it was. The code
/// a marker trails, and a comment that is prose, are code and are hashed.
// @lfy def/generation/main.lfy:review#review:review:cd1ebbdd30f45c288876eb3c3e144180848e744373e0780577158f70511f6d01
#[test]
fn a_marker_comment_is_no_part_of_the_hash_of_a_region() {
    let (fixture, program, plan, under_review) = a_to_review();
    let (criteria, _) = a_requirement_ids(&program, &plan);
    let maps = vec![SourceMap {
        markers: vec![entity_marker(1, "A", 1, 6)],
        ..a_source_map().remove(0)
    }];
    fixture.write(
        "src/a.rs",
        "  // @lfy def/a.lfy:A\nalpha\nbeta // @lfy def/a.lfy:A.x\n// a comment\ngamma\ndelta\n",
    );
    let request = review(&program, &plan, under_review, &maps, None, &[]);
    let code = source_hash("alpha\nbeta\n// a comment\ngamma\ndelta\n");
    assert_eq!(request.hashes[&criteria[0]], code);
    // The same code with a criterion's id spelled on one of those markers hashes to the
    // same thing, so no other criterion of the entity is sent back to review.
    fixture.write(
        "src/a.rs",
        &format!(
            "  // @lfy def/a.lfy:A#{}\nalpha\nbeta // @lfy def/a.lfy:A.x\n// a comment\n\
             gamma\ndelta\n",
            criteria[0]
        ),
    );
    let moved = review(&program, &plan, under_review, &maps, None, &[]);
    assert_eq!(moved.hashes[&criteria[0]], code);
    // A line of code that changed does change it.
    fixture.write(
        "src/a.rs",
        "  // @lfy def/a.lfy:A\nalpha\nbeta // @lfy def/a.lfy:A.x\n// a comment\ngamma\nepsilon\n",
    );
    let other = review(&program, &plan, under_review, &maps, None, &[]);
    assert_ne!(other.hashes[&criteria[0]], code);
}

/// A record is carried for the criterion whose id it holds and for no other, so the
/// criterion that rewords it is listed although the regions of their entity are the ones
/// the record was judged against.
// @lfy def/generation/main.lfy:review#review:review:cfa867e320d611cec123830eb03c4368327c6de40443d7d3c904f20f1fb214d2
#[test]
fn a_record_of_the_id_a_criterion_replaced_carries_nothing() {
    let (_fixture, program, plan, under_review) = a_to_review();
    let maps = a_source_map();
    let (criteria, tests) = a_requirement_ids(&program, &plan);
    // The id the second criterion would have replaced, judged against the very code the
    // entity's regions hold now.
    let records = vec![ReviewRecord {
        id: format!("{}x", criteria[1]),
        status: ReviewStatus::Satisfied,
        hash: source_hash(A_REGION),
    }];
    let request = review(&program, &plan, under_review, &maps, None, &records);
    assert_eq!(
        request.asked,
        [criteria[0].clone(), criteria[1].clone(), tests[0].clone()]
    );
    assert!(request.carried.is_empty(), "{:?}", request.carried);
}

/// A record made against code the entity's regions no longer hash to carries nothing:
/// the criterion is listed again.
// @lfy def/generation/main.lfy:review#review:review:29af26a8687b4c42aeccb91116bd72d08a3b1df9e18ea8413bdae5a61d114869
#[test]
fn a_record_of_code_that_changed_carries_nothing() {
    let (_fixture, program, plan, under_review) = a_to_review();
    let maps = a_source_map();
    let (criteria, tests) = a_requirement_ids(&program, &plan);
    let records = vec![ReviewRecord {
        id: criteria[0].clone(),
        status: ReviewStatus::Satisfied,
        hash: source_hash("alpha\nbeta\ngamma\nepsilon\n"),
    }];
    let request = review(&program, &plan, under_review, &maps, None, &records);
    // @lfy def/generation/main.lfy:review#review:review:79a851646614a7ae02fda5c6fca4946a5e185414f8174d5acb5c40413f0c8033
    assert_eq!(
        request.asked,
        [criteria[0].clone(), criteria[1].clone(), tests[0].clone()]
    );
    assert!(request.carried.is_empty(), "{:?}", request.carried);
}

/// With every criterion and test of the unit carried, the instructions are the heading
/// and one line saying nothing needs review, and no protocol.
// @lfy def/generation/main.lfy:review#review:review:93d9b5acfbd7c0036df3f84c0e174e83857f5908e82506f9a878403fa4ed4b52
#[test]
fn a_unit_whose_every_verdict_is_carried_needs_no_review() {
    let (_fixture, program, plan, under_review) = a_to_review();
    let maps = a_source_map();
    let (criteria, tests) = a_requirement_ids(&program, &plan);
    let hash = source_hash(A_REGION);
    let records: Vec<ReviewRecord> = criteria
        .iter()
        .chain(&tests)
        .map(|id| ReviewRecord {
            id: id.clone(),
            status: ReviewStatus::Satisfied,
            hash: hash.clone(),
        })
        .collect();
    let request = review(&program, &plan, under_review, &maps, None, &records);
    // @lfy def/generation/main.lfy:review#review:review:0d8c71e5e6a2fc7df9307c06e3d90e10a1982484663081fdffc8b2b46c7f029b
    assert!(request.asked.is_empty(), "{:?}", request.asked);
    assert_eq!(request.carried.len(), 3, "{:?}", request.carried);
    let text = &request.instructions;
    assert!(
        text.contains("# Reviewing the unit `a` for the target `rust`"),
        "{text}"
    );
    assert!(text.contains("Nothing in the unit needs review"), "{text}");
    // @lfy def/generation/main.lfy:review#review:review:8067f4b70be69f26ef14e51b27d063565e561dc22f54a024158105cad82cc9c5
    assert!(!text.contains("#### `A`"), "{text}");
    assert!(!text.contains("src/a.rs"), "{text}");
    assert!(!text.contains("ELFIE: REVIEWED"), "{text}");
}

/// A review excerpts only what the unit's own file generated, and a region a marker of
/// a second source map names again is repeated as its path and lines alone.
// @lfy def/generation/main.lfy:review#review:review:5923148a7793fded1531a97ff6a9c369389688e0a1ca8f50ed586a04dcee3b46
#[test]
fn a_review_holds_the_regions_of_its_own_file_alone() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d A: `An a` {\n  $x = string;\n  where (`x is set`) -> `A holds x`;\n}\n",
    );
    fixture.write("def/b.lfy", "d A: `Another a` {\n  $y = string;\n}\n");
    fixture.write("src/a.rs", "one\ntwo\nthree\nfour\n");
    fixture.write("src/b.rs", "five\nsix\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let under_review = index;
    let map = |output: &str, source: &str, markers: Vec<Marker>| SourceMap {
        target: "rust".to_string(),
        output: output.to_string(),
        source: source.to_string(),
        hash: source_hash("x"),
        requirements: String::new(),
        answered: Vec::new(),
        signature: source_hash("i"),
        text: String::new(),
        dependencies: BTreeMap::new(),
        generated: "2026-09-18T10:00:00Z".to_string(),
        markers,
    };
    let marker = |file: &str, end: usize| Marker {
        output_line: 1,
        file: file.to_string(),
        entity: Some("A".to_string()),
        line: 1,
        column: None,
        requirement: None,
        end,
    };
    let maps = vec![
        map("src/a.rs", "def/a.lfy", vec![marker("def/a.lfy", 4)]),
        // A shared output records every marker it holds, so the same region of
        // `src/a.rs` is named a second time.
        map(
            "src/a.rs",
            "def/b.lfy",
            vec![marker("def/a.lfy", 4)],
        ),
        map("src/b.rs", "def/b.lfy", vec![marker("def/b.lfy", 2)]),
    ];
    let request = review(&program, &plan, under_review, &maps, None, &[]);
    let text = &request.instructions;
    // `def/b.lfy` declares an `A` of its own, and none of its regions comes.
    assert!(!text.contains("src/b.rs"), "{text}");
    // The region of the unit's own file is named, once per map that records it, and the
    // hash is taken over its lines each time it is named.
    // @lfy def/generation/main.lfy:review#review:review:a4959dc5c79c68106f15ab278c665fa965bf1e3d36b6b1a3e0f5449b3673bac1
    assert_eq!(text.matches("`src/a.rs:1-4`").count(), 2, "{text}");
    let whole = "one\ntwo\nthree\nfour\n";
    let id = request.asked.first().expect("one criterion");
    assert_eq!(request.hashes[id], source_hash(&format!("{whole}{whole}")));
}

/// An entity moved into a unit is marked with the file that declares it, not with the
/// unit's own file, so its regions are the ones that file's markers name.
// @lfy def/generation/main.lfy:review#review:review:a4959dc5c79c68106f15ab278c665fa965bf1e3d36b6b1a3e0f5449b3673bac1
#[test]
fn a_review_holds_the_regions_of_an_entity_moved_into_the_unit() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write(
            "def/b.lfy",
            "d B: `A b` {\n  $x = string;\n  where (`x is set`) -> `B holds x`;\n}\n",
        )
        .write(
            "def/a.lfy",
            "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nd C {\n  $x = string;\n  \
             where (`x is set`) -> `C holds x`;\n}\n",
        )
        .write(
            "def/c.lfy",
            "use \"./a\";\nuse \"./b\";\n\ntrait holder {\n  $c = C;\n}\nholder.apply(B);\n",
        );
    fixture.write("src/b.rs", "one\ntwo\nthree\nfour\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, unit) = unit_of(&workspace, &plan, "def/b.lfy");
    // `C` moved into the unit of `def/b.lfy` from `def/a.lfy`.
    assert_eq!(names(&workspace, &unit.entities), ["B", "C"]);
    let under_review = index;
    let marker = |file: &str, entity: &str, line: usize, end: usize| Marker {
        output_line: line,
        file: file.to_string(),
        entity: Some(entity.to_string()),
        line: 1,
        column: None,
        requirement: None,
        end,
    };
    let maps = vec![SourceMap {
        target: "rust".to_string(),
        output: "src/b.rs".to_string(),
        source: "def/b.lfy".to_string(),
        hash: source_hash("x"),
        requirements: String::new(),
        answered: Vec::new(),
        signature: source_hash("i"),
        text: String::new(),
        dependencies: BTreeMap::new(),
        generated: "2026-09-18T10:00:00Z".to_string(),
        markers: vec![
            marker("def/b.lfy", "B", 1, 2),
            marker("def/a.lfy", "C", 3, 4),
        ],
    }];
    let request = review(&program, &plan, under_review, &maps, None, &[]);
    let text = &request.instructions;
    // Each entity is given the region its own declaring file marks, and nothing else.
    assert!(text.contains("`src/b.rs:1-2`"), "{text}");
    assert!(text.contains("`src/b.rs:3-4`"), "{text}");
    assert!(
        !text.contains("No region of any output was generated for it."),
        "{text}"
    );
    // Each entity's criteria are hashed over its own region alone.
    // @lfy def/generation/main.lfy:review#review:review:cd1ebbdd30f45c288876eb3c3e144180848e744373e0780577158f70511f6d01
    assert_eq!(request.asked.len(), 2, "{:?}", request.asked);
    let hashes: Vec<&String> = request
        .asked
        .iter()
        .map(|id| &request.hashes[id])
        .collect();
    assert_eq!(hashes[0], &source_hash("one\ntwo\n"));
    assert_eq!(hashes[1], &source_hash("three\nfour\n"));
}

/// An entity moved into a unit is declared in another file, so its criteria and tests
/// are taken from the file that declares it and not from the unit's own lowered file,
/// which holds no node for it.
// @lfy def/generation/main.lfy:review#review:review:f3ebae56b7c2144a9edb3e3b69ac07f174e3974f258284e909f2e10068ec9831
#[test]
fn a_review_holds_the_criteria_of_an_entity_moved_into_the_unit() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write("def/b.lfy", "d B: `A b` {\n  $x = string;\n}\n")
        .write(
            "def/a.lfy",
            "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nd C {\n  $x = string;\n  \
             where (`x is set`) -> `C holds x`;\n}\n",
        )
        .write(
            "def/c.lfy",
            "use \"./a\";\nuse \"./b\";\n\ntrait holder {\n  $c = C;\n}\nholder.apply(B);\n",
        );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, unit) = unit_of(&workspace, &plan, "def/b.lfy");
    // `C` moved into the unit of `def/b.lfy` from `def/a.lfy`.
    assert_eq!(names(&workspace, &unit.entities), ["B", "C"]);
    let under_review = index;
    let text = review(&program, &plan, under_review, &[], None, &[]).instructions;
    let at = text
        .find("#### `C`")
        .unwrap_or_else(|| panic!("C's section: {text}"));
    let section = &text[at..];
    let end = section[1..]
        .find("\n#### ")
        .or_else(|| section[1..].find("\n## "))
        .map_or(section.len(), |found| found + 1);
    let section = &section[..end];
    assert!(section.contains("C holds x"), "{section}");
}

/// With no source map for the unit the verifier is given the same instructions with no
/// region, so it can only find the criteria unverifiable, and nothing is carried for an
/// entity that generated none.
// @lfy def/generation/main.lfy:review#review:review:600dffcb18a0b3cdd40367085bb183f86996d54d6c7762da18b59bd751034596
#[test]
fn a_review_request_without_a_source_map_holds_no_region() {
    let fixture = a_with_a_member_and_a_fn();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let under_review = index;
    // A record that would carry the criterion if its entity had a region carries
    // nothing, since there is no code whose sameness could stand for the verdict.
    // @lfy def/generation/main.lfy:review#review:review:944917284f9b6da4a52ad0fe6ba57ee0440f72ee59ecdfdc8b90585e6e1e7417
    let asked = review(&program, &plan, under_review, &[], None, &[]).asked;
    let records: Vec<ReviewRecord> = asked
        .iter()
        .map(|id| ReviewRecord {
            id: id.clone(),
            status: ReviewStatus::Satisfied,
            hash: source_hash(""),
        })
        .collect();
    let request = review(&program, &plan, under_review, &[], None, &records);
    assert_eq!(request.asked, asked);
    assert!(request.carried.is_empty(), "{:?}", request.carried);
    let text = &request.instructions;
    assert!(text.contains("def/a.lfy:6 — it does"), "{text}");
    // An entity with no region of its own hashes to the empty text.
    // @lfy def/generation/main.lfy:review#review:review:efbaab9e13ed7569408b035ce9b34392a9e0b4fa0a274b747ea27c0ef681cd65
    assert_eq!(request.hashes[&asked[0]], source_hash(""));
    // @lfy def/generation/main.lfy:review
    assert!(
        text.contains("No region of any output was generated for it."),
        "{text}"
    );
    assert!(text.contains("`ELFIE: REVIEWED`"), "{text}");
}

/// An output a source map names but that cannot be read adds nothing to the hash of the
/// entity's criteria, though the region is named all the same.
// @lfy def/generation/main.lfy:review#review:review:b20a4c21286c959c6a77873bc8d178ac43d720cb03877030c893be7f015f5903
#[test]
fn a_region_whose_output_cannot_be_read_adds_no_text_to_a_hash() {
    let fixture = a_with_a_member_and_a_fn();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, _) = unit_of(&workspace, &plan, "def/a.lfy");
    let under_review = index;
    // Neither `src/a.rs` nor `src/tests.rs` was written, so neither can be read.
    let maps = two_source_maps();
    let request = review(&program, &plan, under_review, &maps, None, &[]);
    let text = &request.instructions;
    assert!(text.contains("`src/a.rs:20-40`"), "{text}");
    assert!(text.contains("`src/tests.rs:1-11`"), "{text}");
    let id = request.asked.first().expect("one criterion");
    assert_eq!(request.hashes[id], source_hash(""));
}

// -----------------------------------------------------------------------------------
// globalReview
// -----------------------------------------------------------------------------------

/// One global criterion, whose id two outputs name in a marker: it is given once with
/// its id and its place, then where each of its two regions is, then the protocol.
// @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:10afbcf8a7a4d4bfbeb07d0648e6b03f2e305d1a6fd788b1784131a3ea739333
#[test]
fn the_global_review_gives_each_global_criterion_once_with_every_region() {
    let fixture = Fixture::with_rust_target();
    fixture
        .write(
            "def/a.lfy",
            "d A { $x = string; }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written twice` });\n",
        )
        .write("src/a.rs", "alpha\nbeta\n")
        .write("src/b.rs", "gamma\ndelta\n");
    let program = fixture.program();
    assert_bound(&program.workspace);
    assert_eq!(program.criteria.len(), 1, "{:?}", program.criteria);
    let id = program.criteria[0].id.clone();
    let answering = |output: &str| SourceMap {
        target: "rust".to_string(),
        output: output.to_string(),
        source: "def/a.lfy".to_string(),
        hash: source_hash("x"),
        requirements: source_hash("r"),
        answered: vec![id.clone()],
        signature: source_hash("i"),
        text: String::new(),
        dependencies: BTreeMap::new(),
        generated: "2026-09-18T10:00:00Z".to_string(),
        markers: vec![Marker {
            output_line: 1,
            file: "def/a.lfy".to_string(),
            entity: Some("A".to_string()),
            line: 1,
            column: None,
            requirement: Some(id.clone()),
            end: 2,
        }],
    };
    let maps = vec![answering("src/a.rs"), answering("src/b.rs")];
    let request = global_review(&program, &maps);
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:c913846c41638a67bec9c31d691948d7a0a285ed211c13711ab1bfbb7a47c67b
    assert_eq!(request.unit, None);
    let text = &request.instructions;
    // @lfy def/generation/main.lfy:globalReview
    assert!(text.contains("# The global review"), "{text}");
    assert!(text.contains("The targets are `rust`."), "{text}");
    // @lfy def/generation/main.lfy:globalReview
    assert!(
        text.contains(&format!("## `{id}` def/a.lfy:2")),
        "{text}"
    );
    assert_eq!(text.matches(id.as_str()).count(), 1, "{text}");
    assert!(text.contains("Nothing is written twice"), "{text}");
    // Where each region is, one per output, with none of their lines.
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:67735b4dd73460a1ce9714a4d61445247a01cff8daca1989651bea577a7f50e0
    assert!(text.contains("`src/a.rs:1-2`"), "{text}");
    assert!(text.contains("`src/b.rs:1-2`"), "{text}");
    for line in ["alpha", "beta", "gamma", "delta"] {
        assert!(!text.contains(line), "{line} is quoted:\n{text}");
    }
    // The id is asked about, and nothing of a global criterion is ever carried.
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:3163f899ae0e06b99cc3cfbdf7857a8f62a41a8e5685323600713e1e6347797b
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e9cc8a4c5b45b3440c827bd67a1fd514c12870041202d0cd54fad21179548843
    assert_eq!(request.asked, std::slice::from_ref(&id));
    assert!(request.hashes.is_empty(), "{:?}", request.hashes);
    assert!(request.carried.is_empty(), "{:?}", request.carried);
    // Then the protocol, with the tools that are called before files are read by hand.
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:e0b38a1741c79bbd7d207fe9104c4f01ee115f0ccc6e3c9d2c738d8a30bac858
    assert!(text.contains("`ELFIE: REVIEWED`"), "{text}");
    assert!(text.contains("unverifiable, never violated"), "{text}");
    assert!(
        text.contains("before files are searched or read by hand"),
        "{text}"
    );
    for tool in [
        "`elfie_output`",
        "`elfie_report`",
        "`elfie_source`",
        "`elfie_entity`",
        "`elfie_references`",
    ] {
        assert!(text.contains(tool), "{tool} is missing:\n{text}");
    }
}

/// A global criterion no marker of any output names is listed with a line saying no
/// unit answered for it, so the verifier finds it unverifiable, never violated.
// @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:8222ea4a6a9a6f89cc5638b9fd971cd4b2276b001299e8c08a68f0fd51a32ccc
#[test]
fn a_global_criterion_no_marker_names_says_no_unit_answered_for_it() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d A { $x = string; }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written twice` });\n",
    );
    let program = fixture.program();
    assert_bound(&program.workspace);
    let request = global_review(&program, &[]);
    let text = &request.instructions;
    assert_eq!(request.unit, None);
    // @lfy def/generation/main.lfy:globalReview#globalReview:globalReview:602d5d9913771ebb9a5ab23bc1b03ed23f9e22888e45cf65e826b7cfdbcc02e2
    assert!(
        text.contains("No unit answered for it, so it cannot be checked against any region."),
        "{text}"
    );
    assert!(text.contains(&program.criteria[0].id), "{text}");
    assert!(text.contains("`ELFIE: REVIEWED`"), "{text}");
}

// -----------------------------------------------------------------------------------
// reviewOf
// -----------------------------------------------------------------------------------

/// One line of a verifier's report: the id of a criterion or test, and what was found
/// for it.
fn review_line(id: &str, status: &str, evidence: &str, note: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"status\":\"{status}\",\
         \"evidence\":\"{evidence}\",\"note\":\"{note}\"}}"
    )
}

/// One line of a verifier's report that gives the place of its criterion or test as well
/// as its id, as the protocol asks for.
fn placed_review_line(
    id: &str,
    file: &str,
    line: usize,
    status: &str,
    evidence: &str,
    note: &str,
) -> String {
    format!(
        "{{\"id\":\"{id}\",\"file\":\"{file}\",\"line\":{line},\"status\":\"{status}\",\
         \"evidence\":\"{evidence}\",\"note\":\"{note}\"}}"
    )
}

/// An id of the right shape that is the id of nothing and near nothing: the same id with
/// its last hex digit changed, so it neither begins the real one nor is begun by it.
fn mistyped_id(id: &str) -> String {
    let mut mistyped = id.to_string();
    let last = mistyped.pop().expect("an id is not empty");
    mistyped.push(if last == '0' { '1' } else { '0' });
    mistyped
}

/// `def/a.lfy` declaring `A` with two local criteria, on the lines its two `add` calls
/// begin on, and one local test; the ids of the two criteria come with it.
fn a_with_two_criteria() -> (Fixture, Program, Vec<String>) {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d A: `An a` {\n  @acceptanceCriteria\n    .add({ behavior = `the first` })\n    .add({ behavior = `the second` });\n  @test({ input = 1, expect = 2 });\n}\n",
    );
    let program = fixture.program();
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, unit) = unit_of(&program.workspace, &plan, "def/a.lfy");
    let _ = index;
    let entity = unit.entities[0];
    let (ids, _) = requirement_ids(&program.files[unit.lowered], entity);
    assert_eq!(ids.len(), 2, "{ids:?}");
    (fixture, program, ids)
}

// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:6ad3006e396fe8b3c5ea739e4852be683f1bba8ddaa4b1b13dd25123422037c6
#[test]
fn a_report_of_two_reviews_and_a_comment_gives_two_reviews_and_no_problems() {
    let (_fixture, program, ids) = a_with_two_criteria();
    let report = format!(
        "# reading src/a.rs\n{}\n{}\n{REVIEWED}",
        review_line(&ids[0], "satisfied", "src/a.rs:1-30", "the field is there"),
        review_line(&ids[1], "violated", "src/a.rs:12-18", "it returns undefined"),
    );
    let found = review_of(&report, &program, &[], &[]);
    // Each line read beginning with `{` is a review line, and the reviews come in report
    // order.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:fdcd21d1f06a140e2bfae307be2625b9c27d43f2b73e320c1f152a39fd9638bc
    assert_eq!(found.reviews.len(), 2, "{found:?}");
    // A review line whose id is the id of a criterion of the program names it.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:4bf722eab5ca52f5732c6c9f8f080d2efd0dbed5df5ea0319fa4c4a56cade02c
    assert_eq!(found.reviews[0].status, ReviewStatus::Satisfied);
    assert_eq!(found.reviews[0].id, ids[0]);
    assert_eq!(found.reviews[0].evidence, "src/a.rs:1-30");
    assert_eq!(found.reviews[1].status, ReviewStatus::Violated);
    assert_eq!(found.reviews[1].id, ids[1]);
    // The place of each is derived from the origin of the criterion its id names: the
    // file it is written in, the line its `add` begins on, and the entity it is for.
    // @lfy def/generation/main.lfy:reviewOf
    assert_eq!(found.reviews[0].file, "def/a.lfy");
    assert_eq!(found.reviews[0].entity, "A");
    assert_eq!(found.reviews[0].line, 3);
    assert_eq!(found.reviews[1].line, 4);
    // The prose line is no problem, and neither is a blank one.
    // @lfy def/generation/main.lfy:reviewOf
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    // Lines after the end line are ignored.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:10e1a05e247051fa67e5af8f9351b3d6c08728d0642aa557109f7db27e01e1d6
    let found = review_of(&format!("{report}\nand that is all"), &program, &[], &[]);
    assert_eq!(found.reviews.len(), 2);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    // Each line read has the whitespace around it removed, so an indented review line
    // is read like any other.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:2bf986f1d26195d0e45f531c18f47a790716471b351423d3a52d37d04c898921
    let found = review_of(
        &format!(
            "   {}   \n{REVIEWED}",
            review_line(&ids[0], "satisfied", "", "indented")
        ),
        &program,
        &[],
        &[],
    );
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert!(found.problems.is_empty(), "{:?}", found.problems);
}

/// An id copied a character short or long still names its criterion, as long as exactly
/// one id of the program is near it.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:731b47ce4e9dbb551b71e00078efb0e67419160fb2a75436b70be87929a8b8a9
#[test]
fn an_id_a_character_short_or_long_names_its_criterion() {
    let (_fixture, program, ids) = a_with_two_criteria();
    let short = &ids[0][..ids[0].len() - 1];
    let long = format!("{}0", ids[1]);
    let report = format!(
        "{}\n{}\n{REVIEWED}",
        review_line(short, "satisfied", "", "short"),
        review_line(&long, "violated", "", "long"),
    );
    let found = review_of(&report, &program, &[], &[]);
    assert_eq!(found.reviews.len(), 2, "{found:?}");
    assert_eq!(found.reviews[0].id, ids[0]);
    assert_eq!(found.reviews[0].line, 3);
    assert_eq!(found.reviews[1].id, ids[1]);
    assert_eq!(found.reviews[1].line, 4);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    // An id that shares only its receiver and its contributor is near nothing: the two
    // hashes have no sixteen hex digits in common.
    // @lfy def/generation/main.lfy:reviewOf
    let (receiver, contributor, _) = id_parts(&ids[0]).expect("an id has three parts");
    let far = review_line(
        &format!("{receiver}:{contributor}:00"),
        "satisfied",
        "",
        "far",
    );
    let found = review_of(&format!("{far}\n{REVIEWED}"), &program, &[], &[]);
    assert!(found.reviews.is_empty(), "{found:?}");
    assert_eq!(found.problems, [far]);
}

/// A review line with no id names the criterion that begins alone where its file and
/// line say, whatever case its status is spelled in and whatever other keys it holds,
/// and the prose and fences around it are no problems.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:5ccf168d438b1997f2ec7fd50ef09356d148a7940500a34b5a6b2fc51a43b52c
#[test]
fn a_review_line_with_no_id_is_placed_by_its_file_and_line() {
    let (_fixture, program, ids) = a_with_two_criteria();
    let report = format!(
        "All tests pass. Report follows.\n```\n\
         {{\"file\":\"def/a.lfy\",\"line\":3,\"entity\":\"A\",\"status\":\"Satisfied\",\
         \"evidence\":\"src/a.rs:1-9\",\"note\":\"ok\"}}\n\
         ```\n{REVIEWED}"
    );
    let found = review_of(&report, &program, &[], &[]);
    // A line read that does not begin with `{` is ignored, prose and fence alike.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:24c8a0d8aab48ace02aea35fdac897930db4e5ea8b8b4bd675b3bd00afefeac3
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    // The place is where exactly one criterion of the program begins, and the status
    // reads whatever its case; the entity key the verifier wrote is ignored.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ffcb26fa671fb0476f3effa3e247f546bf1144d511802cfdd3a726c5756fc018
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert_eq!(found.reviews[0].id, ids[0]);
    assert_eq!(found.reviews[0].file, "def/a.lfy");
    assert_eq!(found.reviews[0].line, 3);
    assert_eq!(found.reviews[0].entity, "A");
    assert_eq!(found.reviews[0].status, ReviewStatus::Satisfied);
    assert_eq!(found.reviews[0].evidence, "src/a.rs:1-9");
    assert_eq!(found.reviews[0].note, "ok");
    // A review line missing its evidence and its note reads with both empty, and the
    // keys it holds beyond the four are ignored.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:03c66ae037b851c7cce98a476e8c2dc9fe0e1d51213bd0f884fca3d2bf00fdea
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9edd00519d199e54062049e61315587483989f62a46fc5a31c9be591c90931ce
    let found = review_of(
        &format!(
            "{{\"id\":\"{}\",\"status\":\"UNVERIFIABLE\",\"reason\":\"none\"}}\n{REVIEWED}",
            ids[1]
        ),
        &program,
        &[],
        &[],
    );
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert_eq!(found.reviews[0].status, ReviewStatus::Unverifiable);
    assert_eq!(found.reviews[0].evidence, "");
    assert_eq!(found.reviews[0].note, "");
    assert!(found.problems.is_empty(), "{:?}", found.problems);
}

/// A review line whose status names no member of `ReviewStatus` is a problem, as
/// written, while the prose around it is none.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:e4e51ceff95aa7cf3d202b2e42573e755ade4bc85b77d90b1621d3f425a5d109
#[test]
fn a_review_line_that_is_no_review_is_a_problem_as_written() {
    let (_fixture, program, ids) = a_with_two_criteria();
    let report = format!(
        "{}\nI think it is fine.\n{}\n{REVIEWED}",
        review_line(&ids[0], "maybe", "", "unsure"),
        review_line(&ids[1], "unverifiable", "", "no region"),
    );
    let found = review_of(&report, &program, &[], &[]);
    // A status that names no member of ReviewStatus is no review.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:065297aa8507d1cce029b56f7ee5ede6af8c5913fb55092cd6433f748dcff490
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert_eq!(found.reviews[0].id, ids[1]);
    assert_eq!(found.reviews[0].line, 4);
    assert_eq!(found.reviews[0].status, ReviewStatus::Unverifiable);
    // A review line that is no review is a problem, as written; the prose line is none.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:065297aa8507d1cce029b56f7ee5ede6af8c5913fb55092cd6433f748dcff490
    assert_eq!(
        found.problems,
        [review_line(&ids[0], "maybe", "", "unsure")]
    );
    // A review line whose id names no criterion and no test of the program is a problem
    // too, and so is one that reads as no object at all.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ebd07d63053ee05f2507003716efdd9c78f20b38aecdf60cca2c1874d85612ac
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:cc8ee014686684d4965cbc76770ce45c98db561453c188050a7a3d3c4b345ad6
    let unknown = review_line("A:A:nowhere", "satisfied", "", "made up");
    let broken = "{\"id\":".to_string();
    let found = review_of(&format!("{unknown}\n{broken}\n{REVIEWED}"), &program, &[], &[]);
    assert!(found.reviews.is_empty(), "{found:?}");
    assert_eq!(found.problems, [unknown, broken]);
}

/// Of two reviews of the same id only the last is kept, at its own place, and a report
/// that never ends says so.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:389bbf9e57884ff5e66e1b8be297335a6fb9e9a36faae98a32402328be553325
#[test]
fn the_last_review_of_a_place_is_kept_and_an_unended_report_says_so() {
    let (_fixture, program, ids) = a_with_two_criteria();
    let report = format!(
        "{}\n{}",
        review_line(&ids[0], "satisfied", "", "first"),
        review_line(&ids[0], "violated", "src/a.rs:3-3", "second"),
    );
    // With no end line, reviews are read from every line of the report.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:10e1a05e247051fa67e5af8f9351b3d6c08728d0642aa557109f7db27e01e1d6
    let found = review_of(&report, &program, &[], &[]);
    // Of two reviews naming the same criterion only the last is kept, at its own place.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:19f540298a118815926304ba19a124afbedf6dedfe2303d1b51cb9b16e308600
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert_eq!(found.reviews[0].status, ReviewStatus::Violated);
    assert_eq!(found.reviews[0].note, "second");
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:b6eda5717d7bcd79108423e0c0c000b75e1bd73f8bd2a124b3cefcb31a67bae1
    assert_eq!(found.problems, ["the report did not end"]);
    // An empty report ends with nothing either.
    assert_eq!(
        review_of("", &program, &[], &[]),
        ReviewReport {
            reviews: Vec::new(),
            problems: vec!["the report did not end".to_string()],
            unplaced: Vec::new(),
            missing: Vec::new(),
        }
    );
    // The end line stands with whitespace around it.
    // @lfy def/generation/main.lfy:reviewOf
    let found = review_of(&format!("  {REVIEWED}  "), &program, &[], &[]);
    assert!(found.reviews.is_empty());
    assert!(found.problems.is_empty(), "{:?}", found.problems);

    // With no end line, review lines are read from every line of the report too, and
    // the prose among them is still no problem.
    // @lfy def/generation/main.lfy:reviewOf
    let found = review_of(
        &format!(
            "{}\nI had a look.",
            review_line(&ids[0], "satisfied", "", "ok")
        ),
        &program,
        &[],
        &[],
    );
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert_eq!(found.problems, ["the report did not end"]);
}

/// A review line whose id names nothing of the program is still placed when its file and
/// line say where exactly one criterion begins; one that claims a violation and places
/// itself nowhere at all is kept apart as written, so that a violation is never lost to
/// an id a verifier garbled; and each id the review was asked about that no review names
/// is missing.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:d4b52386cd8dd2565497fdaaf59b6fcafe4852653e38d732b6f4a7f4cb2d42f2
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ac3a89b9ae9599493a551003577acdff2855f9342959c0d3bc606cddc53e939d
#[test]
fn a_violation_that_is_placed_nowhere_is_kept_apart() {
    let (_fixture, program, ids) = a_with_two_criteria();
    // The second criterion begins on line 4, and nothing else of the program begins
    // there, so the place names it though the id names nothing.
    let mistyped = placed_review_line(
        &mistyped_id(&ids[1]),
        "def/a.lfy",
        4,
        "violated",
        "src/a.rs:12-18",
        "returns undefined",
    );
    let garbled = review_line("A:A:77aa", "VIOLATED", "src/a.rs:3-3", "no such id");
    let report = format!("{mistyped}\n{garbled}\n{REVIEWED}");
    let found = review_of(&report, &program, &[], &[]);
    // A line whose id is the id of nothing and near nothing is placed by its file and
    // its line, at the criterion that begins alone there.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ffcb26fa671fb0476f3effa3e247f546bf1144d511802cfdd3a726c5756fc018
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert_eq!(found.reviews[0].id, ids[1]);
    assert_eq!(found.reviews[0].file, "def/a.lfy");
    assert_eq!(found.reviews[0].line, 4);
    assert_eq!(found.reviews[0].entity, "A");
    assert_eq!(found.reviews[0].status, ReviewStatus::Violated);
    assert_eq!(found.reviews[0].note, "returns undefined");
    // The line that names nothing and gives no place is a problem, and, claiming a
    // violation, is kept apart as well.
    assert_eq!(found.problems, std::slice::from_ref(&garbled));
    assert_eq!(found.unplaced, std::slice::from_ref(&garbled));
    // Asked about nothing, nothing is missing.
    assert!(found.missing.is_empty(), "{:?}", found.missing);

    // Asked about both criteria, the one no review names is missing, in the order it
    // was asked.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:a7bd850a8fc6e8bf19c7c580ea931ed26ea1ea26c2cad6c2ea48096aa93c9535
    let asked = vec![ids[0].clone(), ids[1].clone()];
    let found = review_of(&report, &program, &asked, &[]);
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert_eq!(found.missing, [ids[0].clone()]);
    assert_eq!(found.unplaced, [garbled]);
}

/// One review as the agent server records it when a verifier submits a verdict during
/// the run.
fn submitted(id: &str, line: usize, status: ReviewStatus, note: &str) -> Review {
    Review {
        id: id.to_string(),
        file: "def/a.lfy".to_string(),
        line,
        entity: "A".to_string(),
        status,
        evidence: String::new(),
        note: note.to_string(),
    }
}

/// A verdict the verifier submitted through the agent server wins over a line of the
/// report for the same criterion, and that line is neither a review nor a problem.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:e6ffb38e943b879b8a71a248dd7fd22884f5aa51001084248c3eb7c90b3eda91
#[test]
fn a_submitted_verdict_wins_over_a_line_of_the_report() {
    let (_fixture, program, ids) = a_with_two_criteria();
    let report = format!(
        "{}\n{REVIEWED}",
        review_line(&ids[0], "satisfied", "", "from the report")
    );
    let recorded = vec![submitted(&ids[0], 3, ReviewStatus::Violated, "submitted")];
    let asked = vec![ids[0].clone(), ids[1].clone()];
    let found = review_of(&report, &program, &asked, &recorded);
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9ebef2c6f21093486b00c8a0124de7e4c4262014632c2a66b0b47702c693b59d
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert_eq!(found.reviews[0].status, ReviewStatus::Violated);
    assert_eq!(found.reviews[0].note, "submitted");
    // The line of the report for the same criterion is dropped, not kept apart.
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:42415f0b3dec0f93094d195e8abb7b6b4c6f2a363c4db117d8c0c9e8b3c5417e
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert!(found.unplaced.is_empty(), "{:?}", found.unplaced);
    // The criterion neither the run nor the report answered for is missing.
    assert_eq!(found.missing, [ids[1].clone()]);
}

/// With every verdict submitted during the run, a report of nothing but its end line
/// gives those verdicts in the order they were recorded.
// @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:ac6ab38189abc2b73e52041d213cdb86de455386497b27bce939a60307b57adc
#[test]
fn the_submitted_verdicts_come_in_the_order_they_were_recorded() {
    let (_fixture, program, ids) = a_with_two_criteria();
    let recorded = vec![
        submitted(&ids[1], 4, ReviewStatus::Satisfied, "second criterion"),
        submitted(&ids[0], 3, ReviewStatus::Unverifiable, "first criterion"),
    ];
    let asked = vec![ids[0].clone(), ids[1].clone()];
    // @lfy def/generation/main.lfy:reviewOf#reviewOf:reviewOf:9ebef2c6f21093486b00c8a0124de7e4c4262014632c2a66b0b47702c693b59d
    let found = review_of(REVIEWED, &program, &asked, &recorded);
    assert_eq!(
        found
            .reviews
            .iter()
            .map(|review| (review.id.as_str(), review.status))
            .collect::<Vec<_>>(),
        [
            (ids[1].as_str(), ReviewStatus::Satisfied),
            (ids[0].as_str(), ReviewStatus::Unverifiable),
        ]
    );
    assert!(found.problems.is_empty(), "{:?}", found.problems);
    assert!(found.missing.is_empty(), "{:?}", found.missing);
}

/// A review of a global criterion is placed at that criterion's origin and belongs to
/// `global`, whatever entity the verifier saw it under.
// @lfy def/generation/main.lfy:reviewOf
#[test]
fn a_review_of_a_global_criterion_belongs_to_global() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d A { $x = string; }\nglobal@acceptanceCriteria.add({ behavior = `Nothing is written outside the output directory` });\n",
    );
    let program = fixture.program();
    assert_eq!(program.criteria.len(), 1, "{:?}", program.criteria);
    let id = program.criteria[0].id.clone();
    assert!(Review::is_global(&id), "{id}");
    let found = review_of(
        &format!(
            "{}\n{REVIEWED}",
            review_line(&id, "satisfied", "src/a.rs:1-3", "it is under src")
        ),
        &program,
        &[],
        &[],
    );
    assert_eq!(found.reviews.len(), 1, "{found:?}");
    assert_eq!(found.reviews[0].entity, GLOBAL);
    assert_eq!(found.reviews[0].file, "def/a.lfy");
    assert_eq!(found.reviews[0].line, 2);
    assert!(found.problems.is_empty(), "{:?}", found.problems);
}

