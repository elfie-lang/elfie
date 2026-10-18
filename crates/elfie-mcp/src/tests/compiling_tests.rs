// @lfy def/mcp/main.lfy:task
//! Tests of `task`, `dependency`, `mark`, and `submit`: what a compiler run is bound to.

use super::*;

/// The source of unit `b` as its outputs were accepted: `b` carries two criteria and
/// `c` carries one.
const WAS: &str = "fn b(): `A b` => string {\n  @acceptanceCriteria.add({ behavior = `It answers` }).add({ behavior = `It never fails` });\n}\nfn c(): `A c` => string {\n  @acceptanceCriteria.add({ behavior = `It was slow` });\n}\n";

/// The same source now: `b` has dropped its second criterion, and `c` has reworded its
/// own and gained another. One criterion is kept, one removed, one reworded, one new.
const NOW: &str = "fn b(): `A b` => string {\n  @acceptanceCriteria.add({ behavior = `It answers` });\n}\nfn c(): `A c` => string {\n  @acceptanceCriteria.add({ behavior = `It is quick` }).add({ behavior = `It also returns` });\n}\n";

/// A project whose unit `b` has an accepted output, one criterion of which has since been
/// dropped and one reworded, and the source the output was generated from.
fn project_with_a_removed_criterion() -> (Fixture, String) {
    let fixture = Fixture::new();
    fixture.with_rust_target().write("def/b.lfy", WAS);
    let answers = requirement_id(&fixture, "It answers");
    let never = requirement_id(&fixture, "It never fails");
    let slow = requirement_id(&fixture, "It was slow");
    fixture.write(
        "out/b.rs",
        &format!(
            "// @lfy def/b.lfy:b#{answers}\n// @lfy def/b.lfy:b#{never}\npub fn b() -> String {{\n    String::new()\n}}\n// @lfy def/b.lfy:c#{slow}\npub fn c() -> String {{\n    String::new()\n}}\n"
        ),
    );
    record_unit(&fixture, "b");
    fixture.write("def/b.lfy", NOW);
    (fixture, WAS.to_string())
}

/// `elfie_task` gives the run's unit with its outputs and its reason, then every criterion
/// and test to answer for with its mark, id, place, and text — a reworded one followed by
/// the id and the text it replaces — then what was removed, then the counts.
// @lfy def/mcp/main.lfy:task#task:task:e219c5e4fe018ff55e7ef4c8f0c86e166e46bf4ebd74c07dafed2bbe90a93933
#[test]
fn task_gives_the_obligations_of_its_unit() {
    let (fixture, was) = project_with_a_removed_criterion();
    let kept = requirement_id(&fixture, "It answers");
    let fresh = requirement_id(&fixture, "It also returns");
    let rephrased = requirement_id(&fixture, "It is quick");
    let replaced = {
        let older = Fixture::new();
        older.with_rust_target().write("def/b.lfy", &was);
        requirement_id(&older, "It was slow")
    };
    let session = compiling(&fixture, &[("def/b.lfy", &was)]);
    let text = task(&session).unwrap();
    // The text begins with the unit's stem, its file, and its reason, then its outputs.
    // @lfy def/mcp/main.lfy:task#task:task:539f478257e58ce337bd56a59d32f6e3ac9e5dd1412f40177b504248b1030c39
    // @lfy def/mcp/main.lfy:task#task:task:950a71006d7f807babb9b09d7bc8cd3a42063f1ab0717e6b1b5006fe2031c452
    assert_eq!(text.lines().next(), Some("b def/b.lfy requirements"), "{text}");
    assert_eq!(text.lines().nth(1), Some("outputs out/b.rs"), "{text}");
    // No other unit edited its outputs, so nothing is said of foreign edits.
    assert!(!text.contains("other units edited"), "{text}");
    // Then a line naming the entity, then one line per criterion in request order, each
    // with its mark, id, place, and text.
    // @lfy def/mcp/main.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
    // @lfy def/mcp/main.lfy:task#task:task:42e538670c9d8018e287823eb07140bbb98c23acc8baeac8e3bcfb12b705b1e4
    // @lfy def/mcp/main.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
    // @lfy def/mcp/main.lfy:task#task:task:48b5a17b6bd8bf11626b7e3c8a77f30620a9bcf76c647c3d0cb15aaa2ac3e582
    // @lfy def/mcp/main.lfy:task#task:task:8bf5a91fa0df3686fb02ff5457cd19a9f3c4d034aceb88db292e7f850ecd6638
    assert!(text.contains("\nb\n") && text.contains("\nc\n"), "{text}");
    // @lfy def/mcp/main.lfy:task#task:task:f95499d17f549c43fa265f803f32119fa037bdad61a8ab6c7b710d1e19b92633
    assert!(text.contains(&format!("kept {kept} def/b.lfy:2 — It answers")), "{text}");
    // @lfy def/mcp/main.lfy:task#task:task:97cb756cb2bee31d44637d2616d0097c1d221f7a5830d8b36a4adbec194b8bef
    assert!(text.contains(&format!("new {fresh} def/b.lfy:5 — It also returns")), "{text}");
    // A reworded one is marked apart from a new one, and the line after it gives the id
    // and the text it replaces.
    // @lfy def/mcp/main.lfy:task#task:task:d1e2bd77856838efb047c8fe5d6d27e38d190808ff140a2daf64c7cf550985f3
    assert!(
        text.contains(&format!(
            "reworded {rephrased} def/b.lfy:5 — It is quick\nreplaces {replaced} — It was slow\n"
        )),
        "{text}"
    );
    // Every entity of the unit has a criterion, so nothing is said to lack one.
    // @lfy def/mcp/main.lfy:task#task:task:8cda0fab4d2e20c30f36514faa7a119d7713f6defa2830d8a2284e0161739f87
    assert!(text.contains("no entity of the unit lacks a criterion or test"), "{text}");
    // Then what was removed since the outputs were accepted, one line each.
    // @lfy def/mcp/main.lfy:task#task:task:359c526bf1661585212774516260b538e4f0ab4701c7e10f200e62855c18a363
    assert!(text.contains("these were removed since its outputs were accepted"), "{text}");
    assert!(text.contains("\nremoved ") && text.contains("It never fails"), "{text}");
    // The last line counts the marks.
    // @lfy def/mcp/main.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
    assert_eq!(text.lines().next_back(), Some("1 new, 1 reworded, 1 kept, 0 unknown, 1 removed"));
    // The unit given is the request's own.
    // @lfy def/mcp/main.lfy:task#task:task:face0ae4233969eecd82ba1353a87659c587a519110a0bf39f5a40ab416dbaf8
    assert!(!text.lines().any(|line| line.starts_with("unit ")), "{text}");
    // A session bound to no compiler run is answered with an error, and nothing is read.
    // @lfy def/mcp/main.lfy:task#task:compilerTool:09099b01d8e84ae85a89b2a12642ec68e8d2b277dfec75f117a5e093caf818b2
    let loose = fixture.session();
    assert_eq!(
        task(&loose).unwrap_err(),
        "elfie_task acts only in a session bound to a compiler run"
    );
}

/// The edits other units made to the unit's outputs since it was last accepted come after
/// its outputs, one line each: the kind, by whom, where, the entity of the region, and the
/// note.
// @lfy def/mcp/main.lfy:task#task:task:149f88e6e3ce26a293b2ea7885c0696d5b8207e28d7993f0486dbe0339826da9
#[test]
fn task_gives_the_edits_other_units_made_to_its_outputs() {
    // Unit `b` was accepted with `g`'s one criterion, on line 3, answered in `out/b.rs`,
    // whose region of `g` begins on line 20; nothing has changed since but a fix by `a`.
    let fixture = Fixture::new();
    fixture
        .with_rust_target()
        .write("def/b.lfy", "fn g(): `A g` => string {\n\n  @acceptanceCriteria.add({ behavior = `It answers` });\n}\n");
    let answers = requirement_id(&fixture, "It answers");
    let padding = "\n".repeat(19);
    fixture.write(
        "out/b.rs",
        &format!("{padding}// @lfy def/b.lfy:g#{answers}\npub fn g() -> String {{\n    String::new()\n}}\n"),
    );
    record_unit(&fixture, "b");
    let edit = ForeignEdit {
        by: "a".to_string(),
        owner: "b".to_string(),
        path: "out/b.rs".to_string(),
        kind: ForeignEditKind::Fix,
        region: Marker {
            output_line: 20,
            file: "def/b.lfy".to_string(),
            entity: Some("g".to_string()),
            line: 1,
            column: None,
            requirement: Some(answers.clone()),
            end: 23,
        },
        before: vec!["    String::new()".to_string()],
        after: vec!["    String::from(\"len\")".to_string()],
        note: "a renamed Thing.size to Thing.len".to_string(),
    };
    let session = compiling_touched(&fixture, "b", &[edit]);
    let text = task(&session).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "b def/b.lfy touched", "{text}");
    assert_eq!(lines[1], "outputs out/b.rs", "{text}");
    // @lfy def/mcp/main.lfy:task#task:task:05f4f2c0332db22eae294592d3b5235d10a9000d76d813ad235ab8a7a8c7706a
    assert_eq!(lines[2], "other units edited its outputs since it was last accepted:", "{text}");
    // @lfy def/mcp/main.lfy:task#task:task:e32fa38ec00ece223578e5d30201345f1f5e6104141d5c4635206132b68cfd99
    assert_eq!(lines[3], "fix by a in out/b.rs:20 g — a renamed Thing.size to Thing.len", "{text}");
    assert_eq!(lines[4], "g", "{text}");
    assert_eq!(lines[5], format!("kept {answers} def/b.lfy:3 — It answers"), "{text}");
    assert!(lines[6].contains("no entity of the unit lacks a criterion or test"), "{text}");
    assert_eq!(lines[7], "0 new, 0 reworded, 1 kept, 0 unknown, 0 removed", "{text}");
    assert_eq!(lines.len(), 8, "{text}");
}

/// A removed criterion the request could not quote is given by its id alone, with words
/// saying its text is not known in place of its place and text.
// @lfy def/mcp/main.lfy:task#task:task:66f11c93795ee4ea2822907afd33f054e3589b0287b19b0b618c695e86e32fbb
#[test]
fn task_says_when_a_removed_criterion_cannot_be_quoted() {
    let (fixture, was) = project_with_a_removed_criterion();
    // The previous source handed over is only the unit's hash-matched one, so the
    // request reads what was dropped off the recorded ids and holds no text for it.
    // @lfy def/mcp/main.lfy:task#task:task:66f11c93795ee4ea2822907afd33f054e3589b0287b19b0b618c695e86e32fbb
    let session = compiling_with(&fixture, &[("def/b.lfy", &was)], &["def/b.lfy"]);
    let text = task(&session).unwrap();
    let removed: Vec<&str> = text.lines().filter(|line| line.starts_with("removed ")).collect();
    // @lfy def/mcp/main.lfy:task#task:task:66f11c93795ee4ea2822907afd33f054e3589b0287b19b0b618c695e86e32fbb
    assert!(!removed.is_empty(), "{text}");
    for line in &removed {
        assert!(line.ends_with(" — its text is not known"), "{line}");
        assert_eq!(line.split(' ').count(), 8, "{line}");
    }
    // @lfy def/mcp/main.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
    assert!(text.lines().next_back().unwrap().ends_with(&format!("{} removed", removed.len())), "{text}");
}

/// A unit with no output yet says so and where its outputs go, and names every entity
/// that has no criterion or test of its own.
// @lfy def/mcp/main.lfy:task#task:task:78f9e281986eff5db14a8f174b25fcbdd7a5c3c6d5c7357556c01dab5a977a04
// @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
#[test]
fn task_says_where_a_fresh_units_outputs_go_and_what_lacks_a_criterion() {
    let fixture = compiled_project();
    let session = compiling(&fixture, &[]);
    let text = task(&session).unwrap();
    // @lfy def/mcp/main.lfy:task#task:task:78f9e281986eff5db14a8f174b25fcbdd7a5c3c6d5c7357556c01dab5a977a04
    assert!(text.contains("no output yet; its outputs go under out"), "{text}");
    // `B` has no criterion and no test, so it is named as still needing a marker.
    // @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
    assert!(text.contains("no criterion or test is for B; each still needs a marker"), "{text}");
    // With no output there is nothing to have answered, so every mark is new and nothing
    // is said to have been removed.
    // @lfy def/mcp/main.lfy:task#task:task:04aaa75f3b9f5d57d80f8170d92ad9358d6aab1ed98260a76078d0741d5aae54
    assert!(!text.contains("is not known"), "{text}");
    // @lfy def/mcp/main.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
    assert_eq!(text.lines().next_back(), Some("0 new, 0 reworded, 0 kept, 0 unknown, 0 removed"));
}

/// `elfie_dependency` gives one entity of a dependency of the unit: the unit and its
/// outputs, then the entity's own interface line and its members'.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:701a4697ca110db1aab8f259cccfb3571473bebf42c6150d26adb79f2d13f436
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:c6b15a1cabf3b86dbb6e91700e8acb8959cc0e9afe8a118889b19800713a0551
#[test]
fn dependency_gives_one_entity_of_a_dependency_of_the_unit() {
    let fixture = Fixture::new();
    fixture
        .with_rust_target()
        .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
        .write("def/b.lfy", "d B: `a thing` { $n: `How many` = number; }\n")
        .write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
    // `b` is accepted already, and is a dependency of `a`, the unit the run is for.
    record_unit(&fixture, "b");
    let session = bound(&fixture, binding_of(&fixture, Role::Compiler, Some("a")));
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:701a4697ca110db1aab8f259cccfb3571473bebf42c6150d26adb79f2d13f436
    let text = dependency(&session, "B").unwrap();
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:53420221788d685744cc63a541e15f79755a4b23efe12ec64289b0e9c90ad313
    assert!(text.starts_with("b def/b.lfy\noutputs "), "{text}");
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:5187f431c8e1a018407dfaf23c821f2cfc1f4a15370dc6acb8175a28e7fcd50c
    assert!(text.contains("- `B` (data"), "{text}");
    assert!(text.contains("a thing"), "{text}");
    assert!(text.contains("  - `n` (member)"), "{text}");
    assert!(text.contains("number"), "{text}");
    // A member named after a dot gives only that member's line, and a file path and a
    // colon limits the search to that dependency.
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:82fa16bd07081751c96e7033b5801a1698a74ccf133725c2d1e01ea9edd01680
    let member = dependency(&session, "B.n").unwrap();
    assert!(member.contains("  - `n` (member)"), "{member}");
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:7b425db08ad081030e6137f0d6924c40b3b223d36d698551284b593335193519
    assert_eq!(dependency(&session, "def/b.lfy:B").unwrap(), text);
    assert!(dependency(&session, "def/a.lfy:B").is_err());
    // No dependency of the unit declares the name.
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:c6b15a1cabf3b86dbb6e91700e8acb8959cc0e9afe8a118889b19800713a0551
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:3d365d972122d9a5c77e7362aa0dec8a6f2e7fcc839899f595b9d15614a1d730
    let error = dependency(&session, "Missing").unwrap_err();
    assert!(
        error.contains("no dependency of the unit declares Missing") && error.contains("elfie_entity"),
        "{error}"
    );
    // A session bound to no compiler run is answered with an error.
    // @lfy def/mcp/main.lfy:dependency#dependency:compilerTool:94e97e4b9416897e8f7296ad1449f08bf7cb435fe9a76fc4125ba6de8559c49e
    let loose = fixture.session();
    assert_eq!(
        dependency(&loose, "B").unwrap_err(),
        "elfie_dependency acts only in a session bound to a compiler run"
    );
}

/// `elfie_mark` rewrites every marker of the run's outputs that names an old id, leaves
/// everything else of the file alone, and answers an id of no criterion of its units
/// with an error that writes nothing.
// @lfy def/mcp/main.lfy:mark#mark:mark:17f85f039cb18d4b028de5f6a8a65fdaa0f2d0632ffd547bbf899a2dfd151791
// @lfy def/mcp/main.lfy:mark#mark:mark:9fec123ea2c4a8fd481352dfaebe77ee76ea10475a33ec754f839b2fb2b20f13
#[test]
fn mark_renames_every_marker_of_an_old_id() {
    let (fixture, was) = project_with_a_removed_criterion();
    let fresh = requirement_id(&fixture, "It is quick");
    let gone = {
        let older = Fixture::new();
        older.with_rust_target().write("def/b.lfy", &was);
        requirement_id(&older, "It never fails")
    };
    // The output holds the old id twice, on lines 2 and 5.
    fixture.write(
        "out/b.rs",
        &format!(
            "// @lfy def/b.lfy:B\n// @lfy def/b.lfy:b#{gone}\npub fn b() -> String {{\n    String::new()\n    // @lfy def/b.lfy:b#{gone}\n}}\n"
        ),
    );
    let session = compiling(&fixture, &[("def/b.lfy", &was)]);
    // @lfy def/mcp/main.lfy:mark#mark:mark:4f60b16548a3a5d470c0ee9cd9a0c70970ffdedc84b94fa5e4f5efbde3465999
    // @lfy def/mcp/main.lfy:mark#mark:mark:5bb1f1250f7f259cd65c04fd3be30c251cc135967710c7ce195a78f31866f8fe
    let text = mark(&session, Some(&fresh), Some(&gone), None, None, None).unwrap();
    assert!(text.starts_with("2 markers rewritten"), "{text}");
    assert!(text.contains(&format!("out/b.rs:2 def/b.lfy:b#{fresh}")), "{text}");
    assert!(text.contains(&format!("out/b.rs:5 def/b.lfy:b#{fresh}")), "{text}");
    // The output `rename` returned was written at its path, and it is the file `rename`
    // gives: only the id changed.
    // @lfy def/mcp/main.lfy:mark#mark:mark:86d4a3c6fe5b3120fc0fb96bbb0c929054629025987d9c9f87bbe6cd5de3d8bc
    let written = fs::read_to_string(fixture.root.join("out/b.rs")).unwrap();
    assert_eq!(
        written,
        format!(
            "// @lfy def/b.lfy:B\n// @lfy def/b.lfy:b#{fresh}\npub fn b() -> String {{\n    String::new()\n    // @lfy def/b.lfy:b#{fresh}\n}}\n"
        )
    );
    // No marker names the old id any more, so a second rename is an error.
    // @lfy def/mcp/main.lfy:mark#mark:mark:95861eab55aed3f41b6a4bdab75d4e18ff5a4716d3a6751ca86c64341f33e296
    let error = mark(&session, Some(&fresh), Some(&gone), None, None, None).unwrap_err();
    assert!(error.contains("no marker") && error.contains(&gone), "{error}");
    // An id of no criterion or test of the unit writes nothing.
    // @lfy def/mcp/main.lfy:mark#mark:mark:9fec123ea2c4a8fd481352dfaebe77ee76ea10475a33ec754f839b2fb2b20f13
    // @lfy def/mcp/main.lfy:mark#mark:mark:5bbb75b2e68dfacdf922b33253428ff59e3f168016b98840f11769f1c56a3553
    let error = mark(&session, Some("b:b:0000"), Some(&fresh), None, None, None).unwrap_err();
    assert!(error.contains("b:b:0000") && error.contains("no criterion or test"), "{error}");
    assert_eq!(fs::read_to_string(fixture.root.join("out/b.rs")).unwrap(), written);
    // A rename takes `replaces` and `id` and nothing else, and an insertion takes
    // `file`, `line`, and `entity`.
    // @lfy def/mcp/main.lfy:mark#mark:mark:de06da3d9e51338cb41d6032e36580ce6510b4297822e054fb6f234dc68b0594
    let error = mark(&session, Some(&fresh), Some(&gone), Some("out/b.rs"), None, None).unwrap_err();
    assert!(error.contains("a rename takes replaces and id"), "{error}");
    let error = mark(&session, None, Some(&gone), None, None, None).unwrap_err();
    assert!(error.contains("a rename takes replaces and id"), "{error}");
    // @lfy def/mcp/main.lfy:mark#mark:mark:2959e7495b60ed84e030a118eff9b9a1cb5d699815713975e3e7f53137050e14
    let error = mark(&session, None, None, Some("out/b.rs"), Some(1.0), None).unwrap_err();
    assert!(error.contains("an insertion takes file, line, and entity"), "{error}");
    assert_eq!(fs::read_to_string(fixture.root.join("out/b.rs")).unwrap(), written);
    // A session bound to no compiler run is answered with an error.
    // @lfy def/mcp/main.lfy:mark#mark:compilerTool:56146f12eb0eb11e3ccbcf6b89b563f1173e5c188a183687178892cdd6aefb62
    let loose = fixture.session();
    assert_eq!(
        mark(&loose, Some(&fresh), Some(&gone), None, None, None).unwrap_err(),
        "elfie_mark acts only in a session bound to a compiler run"
    );
}

/// `elfie_mark` inserts one marker line before the line named, at that line's
/// indentation, and leaves every other character of the file alone.
// @lfy def/mcp/main.lfy:mark#mark:mark:4841b8b767e45c4dd46dbfae2894efb5ea42b96bd547ca1c057553b20948b95e
#[test]
fn mark_inserts_one_marker_line_into_an_output() {
    let fixture = Fixture::new();
    fixture.with_rust_target().write("def/b.lfy", &two_criteria("It is quick"));
    let first = requirement_id(&fixture, "It answers");
    fixture.write(
        "out/b.rs",
        "// @lfy def/b.lfy:b\npub fn b() -> String {\n    let helper = 1;\n    String::new()\n}\n",
    );
    let session = compiling(&fixture, &[]);
    // The inserted line takes the indentation of the line it comes before, the target's
    // marker comment, `@lfy`, the declaring file, the entity, and the id.
    // @lfy def/mcp/main.lfy:mark#mark:mark:e959da5d59df47ac7933bb07c057234854cf20b78a1ae1296c4d2521057c5f32
    // @lfy def/mcp/main.lfy:mark#mark:mark:2e402e1e9ad7b4d3ccb7a990b41387cfa8be2150d5feaa6d160ee8adfd91704e
    let text = mark(&session, Some(&first), None, Some("out/b.rs"), Some(3.0), Some("b")).unwrap();
    assert_eq!(text, format!("out/b.rs:3     // @lfy def/b.lfy:b#{first}"));
    // The line that was third is now fourth, and nothing else of the file changed.
    // @lfy def/mcp/main.lfy:mark#mark:mark:f81ece6f1d736c69189687f1b61d0a5d106ce51765776bbd8523caaecef41dea
    let written = fs::read_to_string(fixture.root.join("out/b.rs")).unwrap();
    assert_eq!(
        written,
        format!(
            "// @lfy def/b.lfy:b\npub fn b() -> String {{\n    // @lfy def/b.lfy:b#{first}\n    let helper = 1;\n    String::new()\n}}\n"
        )
    );
    // One past the last line appends it, with no indentation and no id when none is
    // given.
    // @lfy def/mcp/main.lfy:mark#mark:mark:0d151705612442f586374d3ecc64db058c44120fe7a7355be4a84f2aafe6d259
    let appended = mark(&session, None, None, Some("out/b.rs"), Some(7.0), Some("b")).unwrap();
    assert_eq!(appended, "out/b.rs:7 // @lfy def/b.lfy:b");
    assert!(
        fs::read_to_string(fixture.root.join("out/b.rs")).unwrap().ends_with("}\n// @lfy def/b.lfy:b\n"),
        "{written}"
    );
    // A line outside the file, a file that is no output of the target, an entity the unit
    // does not declare, and an id of neither the unit nor the globals are each an error
    // that writes nothing.
    let before = fs::read_to_string(fixture.root.join("out/b.rs")).unwrap();
    // @lfy def/mcp/main.lfy:mark#mark:mark:a86c8a8dd444a7e09d183e1c131c3e4e0e74fdc0a4a752698290645da729a4b0
    let error = mark(&session, None, None, Some("out/b.rs"), Some(99.0), Some("b")).unwrap_err();
    assert!(error.contains("has 7 lines"), "{error}");
    // A line below 1, and a number that is no whole line, are answered with the count too.
    // @lfy def/mcp/main.lfy:mark#mark:mark:a86c8a8dd444a7e09d183e1c131c3e4e0e74fdc0a4a752698290645da729a4b0
    let error = mark(&session, None, None, Some("out/b.rs"), Some(0.0), Some("b")).unwrap_err();
    assert!(error.contains("has 7 lines"), "{error}");
    let error = mark(&session, None, None, Some("out/b.rs"), Some(2.5), Some("b")).unwrap_err();
    assert!(error.contains("has 7 lines"), "{error}");
    // Such a number is of the parameter's type, so a call reaches mark with it.
    // @lfy def/mcp/main.lfy:mark#mark:tool:d98367d940b3e529d2e08602bdac8f47fd16cec96553f895f7018870629df196
    for line in [serde_json::json!(0), serde_json::json!(2.5)] {
        let result = call(&session, "elfie_mark", serde_json::json!({ "file": "out/b.rs", "line": line, "entity": "b" }));
        assert!(result.is_error && result.text.contains("has 7 lines"), "{}", result.text);
    }
    // @lfy def/mcp/main.lfy:mark#mark:mark:096f3a8e4c7a62fc8b97a432770445c9c9061225f9849d6125699bff1233aa64
    let error = mark(&session, None, None, Some("out/z.rs"), Some(1.0), Some("b")).unwrap_err();
    assert!(error.contains("is not an output file"), "{error}");
    // @lfy def/mcp/main.lfy:mark#mark:mark:af478fe84c65f8f3a8750f1acb30f2dbc8adcb4173e9a28f06be3b4389f86287
    let error = mark(&session, None, None, Some("out/b.rs"), Some(1.0), Some("Z")).unwrap_err();
    assert!(error.contains("the unit b does not declare Z"), "{error}");
    // @lfy def/mcp/main.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
    let error = mark(&session, Some("b:b:0000"), None, Some("out/b.rs"), Some(1.0), Some("b")).unwrap_err();
    assert!(error.contains("b:b:0000") && error.contains("no global one"), "{error}");
    // @lfy def/mcp/main.lfy:mark#mark:mark:af3f47a174d2ca2377099af56a6a2784a84ff3e0f957c1bfffcab64de732a8a2
    // @lfy def/mcp/main.lfy:mark#mark:mark:fcfecab8991108e6f27911bb4b6d77cfffcf93ddbb785581af7a1021cbe3b53c
    assert_eq!(fs::read_to_string(fixture.root.join("out/b.rs")).unwrap(), before);
    assert!(!fixture.root.join("elfie-compile").exists());
}

/// A line that falls in a region of another unit is refused, naming the file its marker
/// names and the unit of that file.
// @lfy def/mcp/main.lfy:mark#mark:mark:50f18df59546947552b52843db51a0eab9607fe330a79b7c11e98da7f2cbc7fa
#[test]
fn mark_refuses_a_line_in_another_units_region() {
    let fixture = Fixture::new();
    // One output holds both units' regions; the run of `a` owns only the first, and `b`
    // is accepted already.
    fixture
        .with_rust_target()
        .write("def/a.lfy", "d A: `An A` { $n = number; }\n")
        .write("def/b.lfy", "d B: `A B` { $n = number; }\n")
        .write(
            "out/shared.rs",
            "// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/b.lfy:B\npub struct B;\n",
        );
    record_unit(&fixture, "b");
    let session = bound(&fixture, binding_of(&fixture, Role::Compiler, Some("a")));
    let before = fs::read_to_string(fixture.root.join("out/shared.rs")).unwrap();
    // @lfy def/mcp/main.lfy:mark#mark:mark:50f18df59546947552b52843db51a0eab9607fe330a79b7c11e98da7f2cbc7fa
    let error = mark(&session, None, None, Some("out/shared.rs"), Some(4.0), Some("A")).unwrap_err();
    assert!(error.contains("def/b.lfy") && error.contains("region") && error.contains("the unit b"), "{error}");
    assert_eq!(fs::read_to_string(fixture.root.join("out/shared.rs")).unwrap(), before);
    // A line in the unit's own region is written.
    let text = mark(&session, None, None, Some("out/shared.rs"), Some(2.0), Some("A")).unwrap();
    assert_eq!(text, "out/shared.rs:2 // @lfy def/a.lfy:A");
}

/// An id of a criterion of one entity of the unit is refused on a marker naming another of
/// its entities, and nothing is written.
// @lfy def/mcp/main.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
#[test]
fn mark_refuses_an_id_of_another_entity() {
    let (fixture, _) = project_with_a_removed_criterion();
    let of_b = requirement_id(&fixture, "It answers");
    let session = compiling(&fixture, &[]);
    let before = fs::read_to_string(fixture.root.join("out/b.rs")).unwrap();
    let error = mark(&session, Some(&of_b), None, Some("out/b.rs"), Some(1.0), Some("c")).unwrap_err();
    assert!(error.contains(&of_b) && error.contains("no global one"), "{error}");
    assert_eq!(fs::read_to_string(fixture.root.join("out/b.rs")).unwrap(), before);
    // The same id on its own entity is written.
    let text = mark(&session, Some(&of_b), None, Some("out/b.rs"), Some(1.0), Some("b")).unwrap();
    assert_eq!(text, format!("out/b.rs:1 // @lfy def/b.lfy:b#{of_b}"));
}

/// `elfie_reach` gives the caps, then each unit of the reach with what its fixes changed and
/// the regions the run edited, each with its kind and what is left of the cap; naming a
/// unit gives every region of it, and naming none of the reach is an error listing it.
// @lfy def/mcp/main.lfy:reach#reach:reach:59e61a1123859967c582cef96cff8b0b16148e972b9d89505c8c1d3b28ec3be3
// @lfy def/mcp/main.lfy:reach#reach:reach:1fbbf273aac7eb01ec644d5d1b1178d929d87d1ac5c120f22712289e53785882
#[test]
fn reach_gives_the_later_units_and_what_the_run_did_to_them() {
    let (fixture, session) = reached_project();
    edit_reached(&fixture);
    let text = reach(&session, None).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    // @lfy def/mcp/main.lfy:reach#reach:reach:88bcc79197d799f1c8d800606697b6c0f5cbc7e8119c081ab62717e33251d9a8
    assert!(lines[0].contains("10 lines") && lines[0].contains("40 lines") && lines[0].contains("elfie_touch"), "{text}");
    // @lfy def/mcp/main.lfy:reach#reach:reach:c13bcc7c5ac7bce345fd001c67d733bf56d5c4ba20a0f0a17f12693cbdcbf4cd
    // @lfy def/mcp/main.lfy:reach#reach:reach:dbdf60fafa6bd921c7b019f16bdfdfae6d7b219467ff6679f2dfad86f1b649d3
    assert_eq!(lines[1], "b def/b.lfy 3 of 40, 37 left; needs a note through elfie_touch", "{text}");
    // @lfy def/mcp/main.lfy:reach#reach:reach:2352621fb062f56052c07817412ee289e45facefad7c707279cec577ce229177
    assert_eq!(lines[2], "  out/b.rs:1-6 G fix 3 of 10, 7 left", "{text}");
    // @lfy def/mcp/main.lfy:reach#reach:reach:8e1fa030cbe98f1b5ead1cabbe92371b6dd678d06b86a6c15f37a1c89aa05248
    assert_eq!(lines[3], "  out/b.rs:7-10 H stub", "{text}");
    // A unit with no edited region is listed with its line alone.
    // @lfy def/mcp/main.lfy:reach#reach:reach:236119a1c13c154015a99008f37e6978614d44b922e615904a7e44ec0a907c30
    assert_eq!(lines[4], "c def/c.lfy 0 of 40, 40 left", "{text}");
    assert_eq!(lines.len(), 5, "{text}");
    // Naming a unit gives every region of it, edited or not.
    // @lfy def/mcp/main.lfy:reach#reach:reach:1037292809b87ea03318bf0710ef237ebc0505c0c531acddcf63039f7c21370e
    // @lfy def/mcp/main.lfy:reach#reach:reach:cbeeb6ddf0c9f41ed85f615e25b724bd99960f1a53cee611897aa4b688c885ca
    let named = reach(&session, Some("c")).unwrap();
    assert!(named.ends_with("c def/c.lfy 0 of 40, 40 left\n  out/c.rs:1-2 C unedited"), "{named}");
    // A stem of no unit of the reach is an error listing the reach.
    // @lfy def/mcp/main.lfy:reach#reach:reach:612c79a0bd570cca3b15daca7e28a4d15edd71b75289ef0817a9d8e1bcea025a
    let error = reach(&session, Some("d")).unwrap_err();
    assert!(error.contains("b, c"), "{error}");
    // A note recorded for a unit ends its line.
    // @lfy def/mcp/main.lfy:reach#reach:reach:ab3be2e89dbc4fa27deac091973926fcda512b1fc063a631f089cfac2ddad105
    touch(&session, "b", "a renamed G.one to G.uno").unwrap();
    let text = reach(&session, None).unwrap();
    assert!(text.contains("b def/b.lfy 3 of 40, 37 left; note: a renamed G.one to G.uno\n"), "{text}");
    // Nothing is written.
    // @lfy def/mcp/main.lfy:reach#reach:reach:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    assert!(!fixture.root.join("elfie-compile").exists());
}

/// A run whose reach is empty may edit no other unit's code, and a session bound to no
/// compiler run is answered with an error.
// @lfy def/mcp/main.lfy:reach#reach:reach:ddc6993419dd1198ca5bbbfbd9ca03ed5cf8dd47101df0b07e9f9d64ad5668c2
#[test]
fn reach_of_the_last_unit_is_empty() {
    let (fixture, _) = reached_project();
    let session = bound(&fixture, binding_of(&fixture, Role::Compiler, Some("c")));
    let text = reach(&session, None).unwrap();
    assert!(text.starts_with("this run may edit no other unit's code"), "{text}");
    let error = reach(&session, Some("b")).unwrap_err();
    assert!(error.contains("this run may edit no other unit's code"), "{error}");
    // @lfy def/mcp/main.lfy:reach#reach:compilerTool:d77749f2f1a06bb69e87b5854ab4977a8efe3f853f774535bdca2b43444d57ae
    let loose = fixture.session();
    assert_eq!(
        reach(&loose, None).unwrap_err(),
        "elfie_reach acts only in a session bound to a compiler run"
    );
}

/// `elfie_touch` records a note for a unit of the reach in place of an earlier one, and
/// refuses a unit outside the reach, an empty note, and a note of more than one line.
// @lfy def/mcp/main.lfy:touch#touch:touch:11b71778ad01a570af197b0914104aa214b69e5f582eaa6bcf386546d65940e0
// @lfy def/mcp/main.lfy:touch#touch:touch:a7357f75d072509ce4cea5c0169cd8ae067e39a64c4a90f476fb4877c314157a
#[test]
fn touch_records_why_the_run_edited_a_later_unit() {
    let (fixture, session) = reached_project();
    // @lfy def/mcp/main.lfy:touch#touch:touch:375335b7467058d45a48410b8ae739752a4dfef36ba775f028524a965bc45dfb
    let text = touch(&session, "b", "a renamed Thing.size to Thing.len").unwrap();
    assert!(text.contains("recorded the note for b") && text.contains("replaces it"), "{text}");
    // @lfy def/mcp/main.lfy:touch#touch:touch:89f37d52cf86059d5d6deb84d257f9f2eb6205714c76d832c2e94f430133e27b
    assert_eq!(
        session.touched(),
        BTreeMap::from([("b".to_string(), "a renamed Thing.size to Thing.len".to_string())])
    );
    // A unit outside the reach is an error listing the reach, and nothing is recorded.
    // @lfy def/mcp/main.lfy:touch#touch:touch:3ad8f735544ed9fff63d14702b492f2d67c0766b719e25fd3fbb8be1723c2b20
    let error = touch(&session, "d", "a renamed Thing.size to Thing.len").unwrap_err();
    assert!(error.contains("b, c"), "{error}");
    // @lfy def/mcp/main.lfy:touch#touch:touch:504b1a8b9cf3534b61ea07b959247e1f7b3e2aa08e0ea6413dca004e3b20ba39
    let error = touch(&session, "c", "  ").unwrap_err();
    assert!(error.contains("a note says why"), "{error}");
    // @lfy def/mcp/main.lfy:touch#touch:touch:d47f9dd713936933b35c0217763a11963a0f0a3c340f0ba8d9c47440cb84fd70
    let error = touch(&session, "c", "one\ntwo").unwrap_err();
    assert!(error.contains("one line"), "{error}");
    assert_eq!(session.touched().len(), 1);
    // A later call for the same unit replaces its note.
    touch(&session, "b", "a split G").unwrap();
    assert_eq!(session.touched().get("b").map(String::as_str), Some("a split G"));
    // Nothing is written, and a session bound to no compiler run is an error.
    // @lfy def/mcp/main.lfy:touch#touch:touch:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    assert_eq!(fs::read_to_string(fixture.root.join("out/b.rs")).unwrap(), REACHED_B);
    // @lfy def/mcp/main.lfy:touch#touch:compilerTool:cd6effeac8a136d2283bffd5d7a472f65a0f39326e6548f4a64585634acc8d52
    let loose = fixture.session();
    assert_eq!(
        touch(&loose, "b", "why").unwrap_err(),
        "elfie_touch acts only in a session bound to a compiler run"
    );
}

/// `elfie_submit` records the outcome of the run, replaces an earlier one, and refuses a
/// name that is none of the three and a blocked or clarify with nothing to say.
// @lfy def/mcp/main.lfy:submit#submit:submit:681caf63bf979b14af1394ae6323a1e8bba1467185a28cf0201a947a73c9585d
// @lfy def/mcp/main.lfy:submit#submit:submit:249b3b6f0035e686ddcd8050b765f8ae1695b4409763982164e8d8a663448e75
// @lfy def/mcp/main.lfy:submit#submit:submit:24de485f562965f35b787f105da217baea3e41ef0dc7984011702fcc5274050b
#[test]
fn submit_records_how_the_run_ended() {
    let fixture = compiled_project();
    let session = compiling(&fixture, &[]);
    assert!(session.submission.lock().unwrap().is_none());
    // The outcome is read without regard to case, and what it says is kept.
    // @lfy def/mcp/main.lfy:submit#submit:submit:839e3605860269cb5d2a142eae47825edd5f8e18cc86e1a3b9c4f70278331d8d
    // @lfy def/mcp/main.lfy:submit#submit:submit:bedeb2634345b2348c1509d268083160d2e9755e210b5107f4fbc972522749c1
    // @lfy def/mcp/main.lfy:submit#submit:submit:f94daa1287cb9c2fd43bbdfe11f8cfe51bf9706696102840706f0445f387669a
    let text = submit(&session, "Blocked", "f:f:9c0d contradicts f:f:1f2e on empty lists").unwrap();
    assert!(text.contains("recorded the outcome blocked"), "{text}");
    assert!(text.contains("a later call of elfie_submit replaces it"), "{text}");
    assert!(text.contains("ELFIE: BLOCKED:"), "{text}");
    assert_eq!(
        session.submission.lock().unwrap().as_ref().map(|one| (one.outcome, one.message.clone())),
        Some((SubmittedOutcome::Blocked, "f:f:9c0d contradicts f:f:1f2e on empty lists".to_string()))
    );
    // Clarify needs its question, and nothing is recorded, so the submission still
    // stands.
    // @lfy def/mcp/main.lfy:submit#submit:submit:b99c292c6a953720125d01484c7cc9fc03b9727847086a54737c3363884273cb
    let error = submit(&session, "clarify", "").unwrap_err();
    assert!(error.contains("clarify its question"), "{error}");
    assert_eq!(
        session.submission.lock().unwrap().as_ref().map(|one| one.outcome),
        Some(SubmittedOutcome::Blocked)
    );
    // An outcome that is none of the three is an error naming the three.
    // @lfy def/mcp/main.lfy:submit#submit:submit:28e49a28fbbfa8f85d4d95ecccb6f05ae599e3a6f128b655d73257666a31412e
    let error = submit(&session, "finished", "").unwrap_err();
    assert!(
        error.contains("done") && error.contains("blocked") && error.contains("clarify"),
        "{error}"
    );
    // A later submission replaces the earlier one.
    // @lfy def/mcp/main.lfy:submit#submit:submit:839e3605860269cb5d2a142eae47825edd5f8e18cc86e1a3b9c4f70278331d8d
    submit(&session, "done", "").unwrap();
    assert_eq!(
        session.submission.lock().unwrap().as_ref().map(|one| one.outcome),
        Some(SubmittedOutcome::Done)
    );
    // Nothing is written, and a session bound to no compiler run is an error.
    // @lfy def/mcp/main.lfy:submit#submit:submit:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    assert!(!fixture.root.join("out").exists());
    // @lfy def/mcp/main.lfy:submit#submit:compilerTool:ee05d8be309e4a7aab71c6ed469b76e49a958abace52e8b6407087bf66c21140
    let loose = fixture.session();
    assert_eq!(
        submit(&loose, "done", "").unwrap_err(),
        "elfie_submit acts only in a session bound to a compiler run"
    );
}
