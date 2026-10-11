// @lfy def/generation/main.lfy:regionsOf
//! Tests of mapped output access.

use super::*;

/// Every region of an owner, and of every member of it, in source map then output
/// order.
// @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:ece2f51d1b6914f38fa307ff237a3db1a0f4f406773b246af49015c4d809a5ff
#[test]
fn the_regions_of_a_name_hold_its_own_and_those_of_its_members_in_output_order() {
    let fixture = a_with_a_member_and_a_fn();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let maps = two_source_maps();
    // Every marker whose entity is the name, source map by source map and within one in
    // output order.
    // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:38d2833b48e58c718adaf9951281dfe50568a87b1fd9fcf0cf493596b2406bae
    let regions = regions_of(&workspace, &maps, "A", None);
    // A member's region comes with its owner's, since it is part of its owner's code.
    // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:093e721919b4f8d968ccd2497d31d9e1ac8783ddfb5ee2bbda092b4302715a0f
    assert_eq!(
        regions
            .iter()
            .map(|marker| (marker.entity.clone(), marker.output_line, marker.end))
            .collect::<Vec<_>>(),
        [
            (Some("A".to_string()), 3, 8),
            (Some("A.x".to_string()), 9, 19),
            (Some("A".to_string()), 12, 30),
        ]
    );
}

/// A member's own name gives its region alone, and a name nothing was generated for
/// gives none.
// @lfy def/generation/main.lfy:regionsOf
#[test]
fn a_members_name_gives_its_region_alone_and_an_unknown_name_gives_none() {
    let fixture = a_with_a_member_and_a_fn();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    let maps = two_source_maps();
    // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:fb7b6213018e0ac14281583c01918af5136734d69bfbba09e66bdb537c59125f
    let regions = regions_of(&workspace, &maps, "A.x", None);
    assert_eq!(regions.len(), 1, "{regions:?}");
    assert_eq!(regions[0].output_line, 9);
    assert_eq!(regions[0].entity.as_deref(), Some("A.x"));
    // A name no marker names, of itself, of a member of it, or of a line inside its
    // declaration, gives an empty list.
    // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:f7ad766d38aaa1f3fe185238e7663b08246cbc5f1782291b0370e57f8b6c3334
    assert!(regions_of(&workspace, &maps, "B", None).is_empty());
}

/// A marker that names a line belongs to the entity whose declaration covers it, and
/// a target keeps only the source maps of that target.
// @lfy def/generation/main.lfy:regionsOf
#[test]
fn a_line_marker_inside_a_declaration_and_a_target_narrow_the_regions() {
    let fixture = a_with_a_member_and_a_fn();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let line_marker = |output_line: usize, line: usize| Marker {
        output_line,
        file: "def/a.lfy".to_string(),
        entity: None,
        line,
        column: None,
        requirement: None,
        end: output_line,
    };
    let mut maps = two_source_maps();
    maps[0].markers = vec![line_marker(1, 2), line_marker(2, 5), line_marker(3, 99)];
    maps[1].target = "other".to_string();
    // Line 2 falls inside A's declaration; line 5 inside f's; line 99 in nothing.
    // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:66a35d2038e53526c71eb7ac77c60c00ba7678d9ffc14769336d39c7f3aeadb6
    let regions = regions_of(&workspace, &maps, "A", Some("rust"));
    assert_eq!(regions.len(), 1, "{regions:?}");
    assert_eq!(regions[0].output_line, 1);
    assert_eq!(regions_of(&workspace, &maps, "f", Some("rust")).len(), 1);
    // The source map of another target is left out when a target is given.
    // @lfy def/generation/main.lfy:regionsOf#regionsOf:regionsOf:d0dd761f2a88d10b62707b3452f9804e82f4c9ae6d4212b5b8b799f9355a82c5
    let mut maps = two_source_maps();
    maps[1].target = "other".to_string();
    assert_eq!(regions_of(&workspace, &maps, "A", None).len(), 3);
    assert_eq!(regions_of(&workspace, &maps, "A", Some("rust")).len(), 2);
    assert_eq!(regions_of(&workspace, &maps, "A", Some("other")).len(), 1);
}

// -----------------------------------------------------------------------------------
// changes
// -----------------------------------------------------------------------------------

fn change_kinds(changes: &[Change]) -> Vec<(&str, ChangeKind)> {
    changes
        .iter()
        .map(|change| (change.entity.as_str(), change.kind))
        .collect()
}

// @lfy def/generation/main.lfy:changes#changes:changes:3e433d7bcae5a307246d5ee75e073f353540363de5806a450996d56c3ae4527a
#[test]
fn without_a_previous_file_every_entity_of_the_unit_is_an_addition() {
    let fixture = a_with_a_member_and_a_fn();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    // No previous text: one addition per entity of the unit, in file order, and nothing
    // else.
    // @lfy def/generation/main.lfy:changes#changes:changes:771d19c7cdc3e68ae80e9c437cf28ae04d6d6112c99fdff85bcf61c1efacc428
    let changes = changes(&program, unit,None);
    // A name declared now and not before is an addition.
    // @lfy def/generation/main.lfy:changes#changes:changes:9fa34ffb6593e18420b047e381120aa4b10d245c1b1cc825bd68c5b21928f000
    assert_eq!(
        change_kinds(&changes),
        [("A", ChangeKind::Added), ("f", ChangeKind::Added)]
    );
}

/// A change is of the file, not of the unit: `def/a.lfy` declares `A` and `C`, and `C`
/// moves into the unit of `def/b.lfy`, so `C` is reported for `def/a.lfy` and not for
/// `def/b.lfy`, whose unit compiles it.
// @lfy def/generation/main.lfy:changes#changes:changes:718b0ba1ae484d5d40adbb3d2dabcfed7ec6b7a0c976dd9193d6c8b00e479f4f
#[test]
fn a_moved_entity_is_reported_for_the_file_that_declares_it() {
    let fixture = a_cycle();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, a) = unit_of(&workspace, &plan, "def/a.lfy");
    let (_, b) = unit_of(&workspace, &plan, "def/b.lfy");
    // `C` has left a's unit and joined b's.
    assert_eq!(names(&workspace, &a.entities), ["A"]);
    assert_eq!(names(&workspace, &b.entities), ["B", "C"]);

    // With no previous text, each file's own declarations are the additions.
    // @lfy def/generation/main.lfy:changes#changes:changes:771d19c7cdc3e68ae80e9c437cf28ae04d6d6112c99fdff85bcf61c1efacc428
    assert_eq!(
        change_kinds(&changes(&program, a, None)),
        [("A", ChangeKind::Added), ("C", ChangeKind::Added)]
    );
    assert_eq!(
        change_kinds(&changes(&program, b, None)),
        [("B", ChangeKind::Added)]
    );

    // Against its own text, neither side differs: both are read as the file's.
    // @lfy def/generation/main.lfy:changes
    let text = "use \"./b\";\n\nd A {\n  $b = B;\n}\n\nd C {\n  $x = string;\n}\n";
    assert_eq!(changes(&program, a, Some(text)), []);
    // `C` was not in the file then and is now, so it is an addition of `def/a.lfy`
    // although the unit of `def/b.lfy` is what compiles it.
    // @lfy def/generation/main.lfy:changes
    let without = "use \"./b\";\n\nd A {\n  $b = B;\n}\n";
    assert_eq!(
        change_kinds(&changes(&program, a, Some(without))),
        [("C", ChangeKind::Added), ("C.x", ChangeKind::Added)]
    );
}

// @lfy def/generation/main.lfy:changes#changes:changes:2d464e60a8872ef956aa7879f05ba6a1cf0508a0690ad19ff5d73f1c8840fd51
#[test]
fn a_new_definition_and_a_new_parameter_are_a_definition_and_a_signature() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "d A: `a thing` {\n  $x = string;\n}\n\nfn f(x: string, y: number): `Does it` => string {\n  @acceptanceCriteria.add({ behavior = `it does` });\n}\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    let previous = "d A: `a value` {\n  $x = string;\n}\n\nfn f(x: string): `Does it` => string {\n  @acceptanceCriteria.add({ behavior = `it does` });\n}\n";
    // The file as it was is bound through the loader, never diffed line by line.
    // @lfy def/generation/main.lfy:changes
    let changes = changes(&program, unit,Some(previous));
    // A differing definition is one change, a differing parameter list another.
    // @lfy def/generation/main.lfy:changes#changes:changes:31cbf0fb17f9e9a9897caeed410e96022754fa49a032c9c654cd1f04ea0915be
    assert_eq!(
        change_kinds(&changes),
        [("A", ChangeKind::Definition), ("f", ChangeKind::Signature)],
        "{changes:?}"
    );
    // Each is under 80 characters, so the detail quotes the old and the new.
    // @lfy def/generation/main.lfy:changes#changes:changes:f3cde677f1a0cd7fd96797089181f950effc833d36a19ceb41f612a135d9b7a8
    assert!(changes[0].detail.contains("a value"), "{:?}", changes[0]);
    assert!(changes[0].detail.contains("a thing"), "{:?}", changes[0]);
    // A signature is named rather than quoted: the parameter that differs.
    // @lfy def/generation/main.lfy:changes#changes:changes:5209dc5c684e270166372c45b82d43b1ec0be20a8e45931a8401d93a06035108
    assert_eq!(changes[1].detail, "the parameter y was added");
}

/// A removed entity comes after the last entity that preceded it before and is still
/// declared, a member is compared as an entity of its own under its owner, and the
/// detail of a change of criteria names the count before and the count now.
// @lfy def/generation/main.lfy:changes#changes:changes:1c9565b23d23c4d3bcbb3fe6534631ef1946f8869e3f178f425b4e12595f3e0b
// @lfy def/generation/main.lfy:changes#changes:changes:f54761e4898e7ebd66aacb630307ee416935a1052c402126c3c712e2eb632c2a
#[test]
fn a_removed_entity_a_removed_member_and_a_new_criterion_are_read_in_file_order() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two` })\n    .add({ behavior = `three` });\n}\n\nd B {\n  $x = string;\n}\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    let previous = "d A;\n\nfn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two` });\n}\n\nd B {\n  $x = string;\n  $old = number;\n}\n";
    // A name declared before and not now is a removal.
    // @lfy def/generation/main.lfy:changes#changes:changes:577fb23d0eeb277ec3536077bd65e3bae93b79dd0d4651bf34d0baf4721efb72
    let changes = changes(&program, unit,Some(previous));
    // The changes of entities declared now follow the order of the unit's entities; `A`
    // was first before and nothing that preceded it is still declared, so its removal is
    // first, and `B.old`, whose predecessor `f` is still declared, comes after `f`.
    // @lfy def/generation/main.lfy:changes#changes:changes:4fe92e633a3e97cb3af4567ad639361ba2ae14254bd2af13633ec6c79ec86d0d
    assert_eq!(
        change_kinds(&changes),
        [
            ("A", ChangeKind::Removed),
            ("f", ChangeKind::Criteria),
            ("B.old", ChangeKind::Removed),
        ],
        "{changes:?}"
    );
    // The detail names the count before and now, then the id added; an id is longer
    // than a detail quotes, so it is named rather than quoted.
    // @lfy def/generation/main.lfy:changes#changes:changes:0621aa70543db6d32719c3dfed5e9e2bcfd9c529e5a2fdb66e7be05532c656fd
    assert!(
        changes[1]
            .detail
            .starts_with("2 criteria before and 3 now; the criterion "),
        "{:?}",
        changes[1]
    );
    assert!(changes[1].detail.ends_with(" was added"), "{:?}", changes[1]);
}

/// A criterion whose text changed keeps its place and takes a new id, so the detail of
/// the change calls the pair reworded; a test's is read the same way.
// @lfy def/generation/main.lfy:changes#changes:changes:50c1ea7f8d6a011259fc7a78f0cc7c039969c689c7e9b36031eaa1c4a2e2ac63
#[test]
fn a_criterion_reworded_in_place_is_called_reworded() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two, said better` });\n  @test({ input = [], expect = `b` });\n}\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    let previous = "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two` });\n  @test({ input = [], expect = `a` });\n}\n";
    // The ids of the local tests differ, so there is a change of tests too, after the
    // change of criteria, in the order ChangeKind declares them.
    // @lfy def/generation/main.lfy:changes#changes:changes:b93a6681e63ab930b2883e00f1b5abaf044b41b09124728fe0ca10c6e6b45916
    let changes = changes(&program, unit, Some(previous));
    assert_eq!(
        change_kinds(&changes),
        [("f", ChangeKind::Criteria), ("f", ChangeKind::Tests)],
        "{changes:?}"
    );
    // A removed and an added criterion id at the same position are called reworded.
    // @lfy def/generation/main.lfy:changes#changes:changes:7d485ed12575ab1f8fb73fd1af344036fcd0e209e355d3895b8822a170c9d276
    assert!(
        changes[0]
            .detail
            .contains("; the criterion at 2 was reworded, from "),
        "{:?}",
        changes[0]
    );
    // A test's is read the same way.
    // @lfy def/generation/main.lfy:changes#changes:changes:e4f0785b9f7bbfa92530f7edab6e298a57fbc81f4665bd43e72947b6327ef1d6
    assert!(
        changes[1]
            .detail
            .contains("; the test at 1 was reworded, from "),
        "{:?}",
        changes[1]
    );
}

/// A criterion added before the ones the file had leaves every later criterion standing
/// where another one stood, so counting from the first pairs nothing: each id is named
/// added or removed on its own.
// @lfy def/generation/main.lfy:changes#changes:changes:04f91148f0aa55ad8aeb5b1e28ce9d809e265016e9e0207a1c1c2a8a70a14f95
#[test]
fn a_criterion_added_before_the_others_rewords_nothing() {
    let fixture = Fixture::with_rust_target();
    // `f` carries three criteria now: one that was not there, the one that was first,
    // and one worded differently from the one that was second.
    fixture.write(
        "def/a.lfy",
        "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `nought` })\n    .add({ behavior = `one` })\n    .add({ behavior = `two, said better` });\n}\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    let previous = "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two` });\n}\n";
    let changes = changes(&program, unit, Some(previous));
    assert_eq!(
        change_kinds(&changes),
        [("f", ChangeKind::Criteria)],
        "{changes:?}"
    );
    // Counting from the first, the criterion at each position was reworded by nothing:
    // the one that was first is still held, and the one that is first was not.
    assert!(!changes[0].detail.contains("reworded"), "{:?}", changes[0]);
    assert_eq!(changes[0].detail.matches(" was added").count(), 2, "{:?}", changes[0]);
    assert_eq!(
        changes[0].detail.matches(" was removed").count(),
        1,
        "{:?}",
        changes[0]
    );
}

/// A file whose entities read the same after the two declarations swap places, a comment
/// moves, and the indentation changes gives no change at all; a statement of a written
/// body gives one.
// @lfy def/generation/main.lfy:changes#changes:changes:ef493f153deb6dbaac3835bfc84dae2266cdd2d04f5c990f3aae5db76de2424e
// @lfy def/generation/main.lfy:changes#changes:changes:d7b590dfd89c646eecf798bdafc911cd87dc2b66f0c3bc1e3cb916051b5fa6f8
#[test]
fn whitespace_and_comments_give_no_change_and_a_new_statement_is_a_body() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "// a comment\nd A {\n  $x = string;\n}\n\nfunction g(x: number): `Doubles` -> number {\n  return x * 2;\n}\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    // Entities are compared by name, never by place or by line, so the two declarations
    // swapping places, a comment moving, and the indentation changing give nothing.
    // @lfy def/generation/main.lfy:changes
    let same = "function g(x: number): `Doubles` -> number {\n    return x * 2;\n}\n\n\
                // a comment, somewhere else now\nd A {\n\n  $x   = string;\n}\n";
    assert!(changes(&program, unit, Some(same)).is_empty());
    // The tokens of the declaring node differ once everything read apart from the body is
    // left out, so there is one change of body.
    // @lfy def/generation/main.lfy:changes#changes:changes:d5f5fa3f7e3377735cb0315378b2cf53df8b6751fbf3bbc207d6a162992f2b14
    let other = "d A {\n  $x = string;\n}\n\nfunction g(x: number): `Doubles` -> number {\n  return x * 3;\n}\n";
    let changes = changes(&program, unit,Some(other));
    assert_eq!(change_kinds(&changes), [("g", ChangeKind::Body)], "{changes:?}");
}

/// A member whose definition and whose declared type both differ gives one change per
/// kind that differs, in the order `ChangeKind` declares them.
// @lfy def/generation/main.lfy:changes#changes:changes:1a1a926241f820a2302b13b9f0e6d3a5150b545da1f4a24ce81c257de353855e
#[test]
fn a_name_on_both_sides_gives_one_change_per_kind_that_differs() {
    let fixture = Fixture::with_rust_target();
    fixture.write("def/a.lfy", "d A {\n  $x: `one` = string;\n}\n");
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    let previous = "d A {\n  $x: `two` = number;\n}\n";
    let changes = changes(&program, unit, Some(previous));
    // The definition comes before the declared type, as `ChangeKind` declares them.
    // @lfy def/generation/main.lfy:changes#changes:changes:3a27812b418c50e70dd8f17ab011cf87210b1245f5064b7fcb22af71ebd740ba
    assert_eq!(
        change_kinds(&changes),
        [
            ("A.x", ChangeKind::Definition),
            ("A.x", ChangeKind::DeclaredType),
        ],
        "{changes:?}"
    );
    assert!(changes[1].detail.contains("number"), "{:?}", changes[1]);
    assert!(changes[1].detail.contains("string"), "{:?}", changes[1]);
}

// -----------------------------------------------------------------------------------
// reworded, rename
// -----------------------------------------------------------------------------------

/// A criterion and a test each reworded in place give one rewording apiece, the
/// criterion's before the test's, each with the id it replaces and what that one said.
// @lfy def/generation/main.lfy:reworded#reworded:reworded:ae7e96a4afd52516a7f472423ea91c10b39bdddaaa74c711bebe31ca89cb2022
#[test]
fn a_criterion_and_a_test_reworded_in_place_are_paired_with_what_they_replace() {
    let fixture = Fixture::with_rust_target();
    // `f` keeps its first criterion and rewords its second; `B` rewords its first test
    // and keeps its second.
    let now = "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two, said better` });\n}\n\n\
               d B: `A b` {\n  @test({ input = 1, expect = `first, said better` });\n  @test({ input = 2, expect = `second` });\n}\n";
    let was = "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two` });\n}\n\n\
               d B: `A b` {\n  @test({ input = 1, expect = `first` });\n  @test({ input = 2, expect = `second` });\n}\n";
    fixture.write("def/a.lfy", now);
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    // One pair per change that calls a pair reworded, in the order of those changes:
    // the criteria of `f`, then the tests of `B`.
    // @lfy def/generation/main.lfy:reworded#reworded:reworded:af54e4c28fb74d4c2fa75b4d1a8399d931aa52c592d5f16bd59ab25cc730a812
    let rewordings = reworded(&program, unit, was);
    assert_eq!(rewordings.len(), 2, "{rewordings:?}");
    // The id it has now and the id it had, read off the two lowerings.
    // @lfy def/generation/main.lfy:reworded#reworded:reworded:68b8e0096a61b2d00deb8e85b9cff1e55e3c804dbb2f9e01870cd1526955b171
    let held: Vec<String> = unit
        .requirements(&program)
        .iter()
        .map(|requirement| requirement.id().to_string())
        .collect();
    assert!(held.contains(&rewordings[0].id), "{rewordings:?} {held:?}");
    assert!(held.contains(&rewordings[1].id), "{rewordings:?} {held:?}");
    assert!(!held.contains(&rewordings[0].replaces), "{rewordings:?}");
    assert!(!held.contains(&rewordings[1].replaces), "{rewordings:?}");
    // The text is what the removed one said, as a removed requirement spells it: a
    // criterion's text for a criterion, an input and an expectation for a test.
    // @lfy def/generation/main.lfy:reworded#reworded:reworded:ab9d9179b73829cca9561b30789c1b03552c81e265ab5a4d78144f07c0548724
    assert_eq!(rewordings[0].text, "two");
    assert_eq!(rewordings[1].text, "Input `1` gives ``first``");
    // Nothing is written: the file as it is, and the file as it was, are only read.
    // @lfy def/generation/main.lfy:reworded#reworded:reworded:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    assert_eq!(fs::read_to_string(fixture.root.join("def/a.lfy")).unwrap(), now);
}

/// Criteria whose ids differ because one was dropped and another added elsewhere, rather
/// than because a text changed in place, give no rewording at all.
// @lfy def/generation/main.lfy:reworded#reworded:reworded:b68735147b4caf0d742a4450cf54f0b22d08806f8ac2e8463f0605987192d59c
#[test]
fn criteria_that_only_changed_places_give_no_rewording() {
    let fixture = Fixture::with_rust_target();
    fixture.write(
        "def/a.lfy",
        "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `three` })\n    .add({ behavior = `one` });\n}\n",
    );
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (_, unit) = unit_of(&workspace, &plan, "def/a.lfy");
    let was = "fn f(): `Does it` => string {\n  @acceptanceCriteria\n    .add({ behavior = `one` })\n    .add({ behavior = `two` });\n}\n";
    assert_eq!(reworded(&program, unit, was), []);
}

/// Every marker of the unit's own file that names an id being replaced is rewritten in
/// place, every other character of its line is kept, and an output whose markers name
/// another file is not returned at all.
// @lfy def/generation/main.lfy:rename#rename:rename:e6bc4b72f8b7ebef5eec47e1133414f4a958a8832ad53882a94a9cf0dfac72ce
#[test]
fn a_marker_of_the_units_own_file_is_rewritten_to_the_id_that_replaces_it() {
    let fixture = a_and_b();
    let program = fixture.program();
    let workspace = program.workspace.clone();
    assert_bound(&workspace);
    let plan = plan_of(&program, &[], &[], &[]);
    let (index, unit) = unit_of(&workspace, &plan, "def/b.lfy");
    let held = unit
        .requirements(&program)
        .first()
        .expect("one criterion of make")
        .id()
        .to_string();
    let gone = "make:make:1f2e".to_string();
    let ids = BTreeMap::from([(gone.clone(), held.clone())]);
    let outputs = [
        Output {
            path: "src/b.rs".to_string(),
            text: format!(
                "pub fn make() {{}} // @lfy def/b.lfy:make#{gone}\nlet x = 1;\n\
                 // @lfy def/b.lfy:make#{gone}\n// @lfy def/b.lfy:B\n"
            ),
        },
        // A marker naming another file is another unit's, whatever id it spells.
        // @lfy def/generation/main.lfy:rename#rename:rename:3091710a31989abcb75e52c70a366b960dac1fabd585e5a3bf0971af66686c8f
        Output {
            path: "src/a.rs".to_string(),
            text: format!("// @lfy def/a.lfy:A#{gone}\n"),
        },
    ];
    let renamed = rename(&workspace, &plan, index, &outputs, &ids);
    // Only the output that holds a rewritten marker comes back.
    // @lfy def/generation/main.lfy:rename#rename:rename:8cb5f57f44ec1a7de3b03c33feda6e4ceedd4fa55d4d2ec4fdd0ae92c27e9d82
    assert_eq!(renamed.len(), 1, "{renamed:?}");
    assert_eq!(renamed[0].path, "src/b.rs");
    // Both markers of that file name the new id, and nothing else of any line moved.
    // @lfy def/generation/main.lfy:rename#rename:rename:4091b5d02c428166459a836f17453935a76b69ac96600c57604d197dccf120e3
    // @lfy def/generation/main.lfy:rename#rename:rename:90a597b0c488f100859379a0d11bf0e632fbfbb941a554b7b5c2669c9d8320d3
    assert_eq!(
        renamed[0].text,
        format!(
            "pub fn make() {{}} // @lfy def/b.lfy:make#{held}\nlet x = 1;\n\
             // @lfy def/b.lfy:make#{held}\n// @lfy def/b.lfy:B\n"
        )
    );
    // An id no marker of the unit names leaves every output as it is.
    // @lfy def/generation/main.lfy:rename#rename:rename:98bd688d7b8dd91a3fbd9fc7eb8c0999c69eba315873a7bcb74f7627b0a5897d
    let other = BTreeMap::from([("make:make:77aa".to_string(), held)]);
    assert_eq!(rename(&workspace, &plan, index, &outputs, &other), []);
    // Nothing is written: the outputs come back for the caller to write.
    // @lfy def/generation/main.lfy:rename#rename:rename:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    assert!(!fixture.root.join("src/b.rs").exists());
}

