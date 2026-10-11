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

/// `elfie_task` gives each unit of the batch with its outputs and its reason, then every
/// criterion and test to answer for with its mark, id, place, and text — a reworded one
/// followed by the id and the text it replaces — then what was removed, then the counts;
/// a stem of no unit of the batch is an error naming its units.
// @lfy def/mcp/main.lfy:task#task:task:f5712f43d66f568f15101eeb1ba81a9fc8e2c6c7d9a372433f7bdfc21c443d5f
// @lfy def/mcp/main.lfy:task#task:task:0ad7d52b1e05307d441d73562c31fb6ed56b23934d94cccfa43bcada539529d3
#[test]
fn task_gives_the_obligations_of_the_units_of_its_batch() {
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
    let text = task(&session, None).unwrap();
    // The unit begins with its stem, its file, and its reason, then its outputs.
    // @lfy def/mcp/main.lfy:task#task:task:260c99a22d5afd2cb5bfe5e5341310aff3e4b5c20a62493303d03bc95d6f4767
    // @lfy def/mcp/main.lfy:task#task:task:93d986fe4163c82020f47668d2a385e3662ee162cec7a9332137b5df9c98b068
    assert!(text.contains("unit b def/b.lfy requirements"), "{text}");
    assert!(text.contains("\noutputs out/b.rs\n"), "{text}");
    // Then a line naming the entity, then one line per criterion in request order, each
    // with its mark, id, place, and text.
    // @lfy def/mcp/main.lfy:task#task:task:0ba2a96c6162e5b2088f0d0af27dcf6dcd7a99b009bf3be3772ffa13ffa864f4
    // @lfy def/mcp/main.lfy:task#task:task:e6bdc2cd8bf73910039781fad6be14d73994cd9495ecfb29415132f0e0b7926d
    // @lfy def/mcp/main.lfy:task#task:task:bc0cfc213f87dda7b8e1106191bb902bdb5a2b08b2472742020be295a621309b
    // @lfy def/mcp/main.lfy:task#task:task:48b5a17b6bd8bf11626b7e3c8a77f30620a9bcf76c647c3d0cb15aaa2ac3e582
    // @lfy def/mcp/main.lfy:task#task:task:8bf5a91fa0df3686fb02ff5457cd19a9f3c4d034aceb88db292e7f850ecd6638
    assert!(text.contains("\nb\n") && text.contains("\nc\n"), "{text}");
    // @lfy def/mcp/main.lfy:task#task:task:00479e8dc9cc7003d7a8fc71ee72b37424f99ac82adaf9497bd8a64df5cd83a2
    assert!(text.contains(&format!("kept {kept} def/b.lfy:2 — It answers")), "{text}");
    // @lfy def/mcp/main.lfy:task#task:task:aa3aedc17d31c8c2288f74ab8d995d37aabd29e988ed6623b3ab368c90880080
    assert!(text.contains(&format!("new {fresh} def/b.lfy:5 — It also returns")), "{text}");
    // A reworded one is marked apart from a new one, and the line after it gives the id
    // and the text it replaces.
    // @lfy def/mcp/main.lfy:task#task:task:8aab948f1ff4b43702f1158f16a22c2a5c3be0d41ed51e5ff90073af5c03d692
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
    // @lfy def/mcp/main.lfy:task#task:task:0e1730d59363044ef74bcf4673aef8eae5aecf93a017aa90bc7f8d9d457f6afd
    assert!(text.contains("these were removed since its outputs were accepted"), "{text}");
    assert!(text.contains("\nremoved ") && text.contains("It never fails"), "{text}");
    // The last line counts the marks.
    // @lfy def/mcp/main.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
    assert_eq!(text.lines().next_back(), Some("1 new, 1 reworded, 1 kept, 0 unknown, 1 removed"));
    // Only the unit named is given.
    // @lfy def/mcp/main.lfy:task#task:task:d056015a5c0120d12f5e1daaebd2c2f0c96947c0bbff29ed4ebdeb57af2bcbe2
    assert_eq!(task(&session, Some("b")).unwrap(), text);
    // A stem of no unit of the batch is an error naming the units of the batch.
    // @lfy def/mcp/main.lfy:task#task:task:20cff63e998498e4042833f30de9a63a11338bcbcc0b15f2191ddcf2f7def4ab
    let error = task(&session, Some("a")).unwrap_err();
    assert!(error.contains("no unit of this batch has the stem a") && error.contains("are: b"), "{error}");
    // A session bound to no compiler run is answered with an error, and nothing is read.
    // @lfy def/mcp/main.lfy:task#task:compilerTool:09099b01d8e84ae85a89b2a12642ec68e8d2b277dfec75f117a5e093caf818b2
    let loose = fixture.session();
    assert_eq!(
        task(&loose, None).unwrap_err(),
        "elfie_task acts only in a session bound to a compiler run"
    );
}

/// A removed criterion the request could not quote is given by its id alone, with words
/// saying its text is not known in place of its place and text.
// @lfy def/mcp/main.lfy:task#task:task:de5f7c473194374bfa34413eb23292d2536ef397d3077ff1897532c68ab7052d
#[test]
fn task_says_when_a_removed_criterion_cannot_be_quoted() {
    let (fixture, was) = project_with_a_removed_criterion();
    // The previous source handed over is only the unit's hash-matched one, so the
    // request reads what was dropped off the recorded ids and holds no text for it.
    // @lfy def/mcp/main.lfy:task#task:task:de5f7c473194374bfa34413eb23292d2536ef397d3077ff1897532c68ab7052d
    let session = compiling_with(&fixture, &[("def/b.lfy", &was)], &["def/b.lfy"]);
    let text = task(&session, None).unwrap();
    let removed: Vec<&str> = text.lines().filter(|line| line.starts_with("removed ")).collect();
    // @lfy def/mcp/main.lfy:task#task:task:de5f7c473194374bfa34413eb23292d2536ef397d3077ff1897532c68ab7052d
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
// @lfy def/mcp/main.lfy:task#task:task:3701f0af0023dfc1eadd1dee395415c17f3523d19c39a1d6d8f30a8aff532e9e
// @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
#[test]
fn task_says_where_a_fresh_units_outputs_go_and_what_lacks_a_criterion() {
    let fixture = compiled_project();
    let session = compiling(&fixture, &[]);
    let text = task(&session, None).unwrap();
    // @lfy def/mcp/main.lfy:task#task:task:3701f0af0023dfc1eadd1dee395415c17f3523d19c39a1d6d8f30a8aff532e9e
    assert!(text.contains("no output yet; its outputs go under out"), "{text}");
    // `B` has no criterion and no test, so it is named as still needing a marker.
    // @lfy def/mcp/main.lfy:task#task:task:f9fb73337a13788bc2c97891ea0b6725150d8f585b1ea3c8102578a43fdc42d6
    assert!(text.contains("no criterion or test is for B; each still needs a marker"), "{text}");
    // With no output there is nothing to have answered, so every mark is new and nothing
    // is said to have been removed.
    // @lfy def/mcp/main.lfy:task#task:task:c6544cb71fb79033882e38ad1d044989c8fa3a6dc97e274b3d4054fbb9b65bda
    assert!(!text.contains("is not known"), "{text}");
    // @lfy def/mcp/main.lfy:task#task:task:ca1abaa819942eb8c6c130e520f1c36ee16815c41b54d388cca6a9b8ad14a76c
    assert_eq!(text.lines().next_back(), Some("0 new, 0 reworded, 0 kept, 0 unknown, 0 removed"));
}

/// `elfie_dependency` gives one entity of a dependency outside the batch: the unit and
/// its outputs, then the entity's own interface line and its members'.
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:59b75ec9e00f60a47689d0771606d6eb6e14ab13f45334c91b35dfb8f1d2f4b8
// @lfy def/mcp/main.lfy:dependency#dependency:dependency:588c65261ef39abe974c127325bbce4ef580f262ff0112f967c93e292fb47dc0
#[test]
fn dependency_gives_one_entity_of_a_dependency_of_the_batch() {
    let fixture = Fixture::new();
    fixture
        .with_rust_target()
        .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
        .write("def/b.lfy", "d B: `a thing` { $n: `How many` = number; }\n")
        .write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
    // `b` is accepted already, so only `a` is planned and `b` is a dependency of its
    // batch from outside it.
    record_unit(&fixture, "b");
    let session = fixture.session();
    let planned = plan_of(&session);
    let batch = planned
        .plan
        .batches
        .iter()
        .find(|batch| batch.units.iter().any(|&unit| planned.plan.units[unit].stem == "a"))
        .cloned()
        .expect("the batch of a");
    let request = generation::request(&planned.program, &planned.plan, &batch, &[], &BTreeMap::new(), true, &[]);
    let session = bound(
        &fixture,
        Binding {
            role: Role::Compiler,
            batch: Some(batch),
            directory: fixture.root.clone(),
            plan: planned.plan.clone(),
            program: planned.program.clone(),
            request: Some(request),
            review: None,
            source_maps: Vec::new(),
        },
    );
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:59b75ec9e00f60a47689d0771606d6eb6e14ab13f45334c91b35dfb8f1d2f4b8
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
    // No dependency outside the batch declares the name.
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:588c65261ef39abe974c127325bbce4ef580f262ff0112f967c93e292fb47dc0
    // @lfy def/mcp/main.lfy:dependency#dependency:dependency:57ea76cedd14a64116b4e927fb4f9f13577e2df3fe6caa9fd1c92a2dd6d88d06
    let error = dependency(&session, "Missing").unwrap_err();
    assert!(
        error.contains("no dependency outside the batch declares Missing") && error.contains("elfie_entity"),
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
// @lfy def/mcp/main.lfy:mark#mark:mark:51af2d7f5c09267a7462bdb9c79e3acb6aa356508fd67fec076310690dbdf73e
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
    // @lfy def/mcp/main.lfy:mark#mark:mark:9cbf7b9aff22e535179edfb2494ceb9e2aa081cb354077d9a0d66e38056c48d6
    let written = fs::read_to_string(fixture.root.join("out/b.rs")).unwrap();
    assert_eq!(
        written,
        format!(
            "// @lfy def/b.lfy:B\n// @lfy def/b.lfy:b#{fresh}\npub fn b() -> String {{\n    String::new()\n    // @lfy def/b.lfy:b#{fresh}\n}}\n"
        )
    );
    // No marker names the old id any more, so a second rename is an error.
    // @lfy def/mcp/main.lfy:mark#mark:mark:675bf462c93e343e85219da2087f1672279fc0bc713a21b5d0f5550e274a6a40
    let error = mark(&session, Some(&fresh), Some(&gone), None, None, None).unwrap_err();
    assert!(error.contains("no marker") && error.contains(&gone), "{error}");
    // An id of no criterion or test of the batch's units writes nothing.
    // @lfy def/mcp/main.lfy:mark#mark:mark:9fec123ea2c4a8fd481352dfaebe77ee76ea10475a33ec754f839b2fb2b20f13
    // @lfy def/mcp/main.lfy:mark#mark:mark:165048edf3b25a24ddb15a6916231fdc341c3ce643d5b2d74c220ae0c65ab193
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
    let error = mark(&session, None, None, Some("out/b.rs"), Some(1), None).unwrap_err();
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
    let text = mark(&session, Some(&first), None, Some("out/b.rs"), Some(3), Some("b")).unwrap();
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
    let appended = mark(&session, None, None, Some("out/b.rs"), Some(7), Some("b")).unwrap();
    assert_eq!(appended, "out/b.rs:7 // @lfy def/b.lfy:b");
    assert!(
        fs::read_to_string(fixture.root.join("out/b.rs")).unwrap().ends_with("}\n// @lfy def/b.lfy:b\n"),
        "{written}"
    );
    // A line outside the file, a file that is no output of the target, an entity no unit
    // of the batch declares, and an id of neither the unit nor the globals are each an
    // error that writes nothing.
    let before = fs::read_to_string(fixture.root.join("out/b.rs")).unwrap();
    // @lfy def/mcp/main.lfy:mark#mark:mark:a86c8a8dd444a7e09d183e1c131c3e4e0e74fdc0a4a752698290645da729a4b0
    let error = mark(&session, None, None, Some("out/b.rs"), Some(99), Some("b")).unwrap_err();
    assert!(error.contains("has 7 lines"), "{error}");
    // @lfy def/mcp/main.lfy:mark#mark:mark:62c0bc17d8c1d1e37e6821582cd6b59f05e0204a3f3edd6f1e33e9d2597fc2db
    let error = mark(&session, None, None, Some("out/z.rs"), Some(1), Some("b")).unwrap_err();
    assert!(error.contains("is not an output file"), "{error}");
    // @lfy def/mcp/main.lfy:mark#mark:mark:64c1518801f24801feb6b601cbda298cc2a1a97c81889b15a61fe32d5a3ded45
    let error = mark(&session, None, None, Some("out/b.rs"), Some(1), Some("Z")).unwrap_err();
    assert!(error.contains("no unit of the batch") && error.contains("b"), "{error}");
    // @lfy def/mcp/main.lfy:mark#mark:mark:dd3ef6b416a23f341639e29ff2f36569d059617f0c4d6c5404dea75abb9f0ef2
    let error = mark(&session, Some("b:b:0000"), None, Some("out/b.rs"), Some(1), Some("b")).unwrap_err();
    assert!(error.contains("b:b:0000") && error.contains("no global one"), "{error}");
    // @lfy def/mcp/main.lfy:mark#mark:mark:a7dcf7eb424415c0fcc0ed5a98c1d56456a130be8cb8f194d3f21ec67c1606df
    // @lfy def/mcp/main.lfy:mark#mark:mark:fcfecab8991108e6f27911bb4b6d77cfffcf93ddbb785581af7a1021cbe3b53c
    assert_eq!(fs::read_to_string(fixture.root.join("out/b.rs")).unwrap(), before);
    assert!(!fixture.root.join("elfie-compile").exists());
}

/// A line that falls in a region of a file no unit of the batch owns is refused.
// @lfy def/mcp/main.lfy:mark#mark:mark:aac83695fcc88a2c0a4688d77974f2d0a6805ebf6b2d752b2141ff71a8a745a2
#[test]
fn mark_refuses_a_line_in_another_units_region() {
    let fixture = Fixture::new();
    // One output holds both units' regions; the batch of `a` owns only the first, and
    // `b` is accepted already so that `a` is planned on its own.
    fixture
        .with_rust_target()
        .write("def/a.lfy", "d A: `An A` { $n = number; }\n")
        .write("def/b.lfy", "d B: `A B` { $n = number; }\n")
        .write(
            "out/shared.rs",
            "// @lfy def/a.lfy:A\npub struct A;\n// @lfy def/b.lfy:B\npub struct B;\n",
        );
    record_unit(&fixture, "b");
    let session = fixture.session();
    let planned = plan_of(&session);
    let batch = planned
        .plan
        .batches
        .iter()
        .find(|batch| batch.units.iter().any(|&unit| planned.plan.units[unit].stem == "a"))
        .cloned()
        .expect("the batch of a");
    let request = generation::request(&planned.program, &planned.plan, &batch, &[], &BTreeMap::new(), true, &[]);
    let session = bound(
        &fixture,
        Binding {
            role: Role::Compiler,
            batch: Some(batch),
            directory: fixture.root.clone(),
            plan: planned.plan.clone(),
            program: planned.program.clone(),
            request: Some(request),
            review: None,
            source_maps: Vec::new(),
        },
    );
    let before = fs::read_to_string(fixture.root.join("out/shared.rs")).unwrap();
    // @lfy def/mcp/main.lfy:mark#mark:mark:aac83695fcc88a2c0a4688d77974f2d0a6805ebf6b2d752b2141ff71a8a745a2
    let error = mark(&session, None, None, Some("out/shared.rs"), Some(4), Some("A")).unwrap_err();
    assert!(error.contains("def/b.lfy") && error.contains("region"), "{error}");
    assert_eq!(fs::read_to_string(fixture.root.join("out/shared.rs")).unwrap(), before);
    // A line in the batch's own region is written.
    let text = mark(&session, None, None, Some("out/shared.rs"), Some(2), Some("A")).unwrap();
    assert_eq!(text, "out/shared.rs:2 // @lfy def/a.lfy:A");
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
