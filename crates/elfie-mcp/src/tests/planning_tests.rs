// @lfy def/mcp/read.lfy:units
//! Tests of `units`, `request`, `check`, and `changes`.

use super::*;
use crate::planning::{move_line, unit_of_stem};

/// What `changes` recovers of the source the outputs of the unit with a stem were accepted
/// from: its text, and whether it is only the hash-matched source.
fn recovered_of(session: &Session, stem: &str) -> Option<(String, bool)> {
    let planned = plan_of(session);
    let index = unit_of_stem(planned.workspace(), &planned.plan, stem, None).unwrap();
    let unit = &planned.plan.units[index];
    generation::previous::previous_of(&planned.program, unit, &unit.outputs).map(|found| (found.text, found.hash_matched))
}

// The text lists the problems, then the moves, then the units; this project has neither a
// problem nor a move, so it is the units alone.
// @lfy def/mcp/read.lfy:units#units:units:920638da45d13abf0859774d9b78a3a3087c59fed19e9fe90e97b355194204b6
// @lfy def/mcp/read.lfy:units#units:units:c7fcb0ca869f9f4555904c7d48858c0314cebdf85dc76b32442da4fcb97e9021
#[test]
fn units_lists_every_unit_or_one_targets() {
    let fixture = compiled_project();
    let session = fixture.session();
    assert_eq!(problems(&session, None).unwrap(), "no problems");
    let listed = "rust b fresh []\nrust a fresh [b]";
    assert_eq!(units(&session, None).unwrap(), listed);
    // @lfy def/mcp/read.lfy:units#units:units:9ef7e980a808516fa1e3bd6a7a7746a57128c4ef775e20c6b6ebf41db491c432
    assert_eq!(units(&session, Some("rust")).unwrap(), listed);
    // @lfy def/mcp/read.lfy:units#units:units:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
    let error = units(&session, Some("go")).unwrap_err();
    assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
}

// @lfy def/mcp/read.lfy:request#request:request:458d02f36175e2302ccdf1f4a0a5e4f6427d6ad3caafb42b339a713769527d7f
#[test]
fn request_gives_the_instructions_of_one_unit() {
    let fixture = compiled_project();
    let session = fixture.session();
    // A unit is named by its stem, and by its target too when one is given.
    let instructions = request(&session, "a", None).unwrap();
    assert!(instructions.contains("An A"), "{instructions}");
    assert!(instructions.contains("def/a.lfy"), "{instructions}");
    assert_eq!(request(&session, "a", Some("rust")).unwrap(), instructions);
    assert_ne!(request(&session, "b", None).unwrap(), instructions);
    // @lfy def/mcp/read.lfy:request#request:request:35e5ad0177134a3f3f1deca8a9b000be17f11403bc5bb2ae319face434d2913f
    let error = request(&session, "c", None).unwrap_err();
    assert!(error.contains("no unit has the stem c"), "{error}");
    let error = request(&session, "a", Some("go")).unwrap_err();
    assert!(error.contains("go is not a target"), "{error}");
}

// @lfy def/mcp/read.lfy:check#check:check:c6866961ccb185b658d527b660124a894f5a6e4c4c8a6b6b79f8d40199fd33a7
#[test]
fn check_reads_the_outputs_from_disk_and_writes_nothing() {
    let fixture = compiled_project();
    let session = fixture.session();
    // Rejected: an error listing the problems.
    // @lfy def/mcp/read.lfy:check#check:check:aa65fce675d53a43132ff27e80f270948be090228ac8b39998f2eb2375b5519a
    let error = check(&session, "b", None).unwrap_err();
    assert!(error.starts_with("rejected b:"), "{error}");
    fixture.write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
    // Accepted: the text says so and lists the outputs.
    // @lfy def/mcp/read.lfy:check#check:check:f3f55cf806901e6c7c5de76844080355b2901cae32c8244bf1c4109365eb069d
    let text = check(&session, "b", None).unwrap();
    assert!(text.starts_with("accepted b for rust: 1 output\n  out/b.rs"), "{text}");
    assert!(!fixture.root.join("out/source-map.json").exists());
    let error = check(&session, "a", None).unwrap_err();
    assert!(error.contains("rejected a:"), "{error}");
}

/// A region another unit moved out of a shared output into a file of its own is still
/// found, because the check reads every other file of the output directory where it reads
/// the outputs, and so sees that unit's region where it went.
// @lfy def/mcp/read.lfy:check#check:check:c6866961ccb185b658d527b660124a894f5a6e4c4c8a6b6b79f8d40199fd33a7
#[test]
fn check_reads_every_other_file_of_the_output_directory() {
    let fixture = compiled_project();
    // One output holds a region of each unit, and it is accepted for `b`, so what is
    // recorded for `b` names it.
    fixture.write(
        "out/shared.rs",
        "// @lfy def/b.lfy:B\npub struct B;\n// @lfy def/a.lfy:A\npub struct A;\n",
    );
    record_unit(&fixture, "b");
    // `b` has since moved its region into a file of its own, which carries no marker of
    // `a` and so is none of the outputs the check judges.
    // @lfy def/mcp/read.lfy:check#check:check:c6866961ccb185b658d527b660124a894f5a6e4c4c8a6b6b79f8d40199fd33a7
    fixture.write("out/shared.rs", "// @lfy def/a.lfy:A\npub struct A;\n");
    fixture.write("out/b.rs", "// @lfy def/b.lfy:B\npub struct B;\n");
    let session = fixture.session();
    // @lfy def/mcp/read.lfy:check#check:check:f3f55cf806901e6c7c5de76844080355b2901cae32c8244bf1c4109365eb069d
    let text = check(&session, "a", None).unwrap();
    assert!(text.starts_with("accepted a for rust: 1 output\n  out/shared.rs"), "{text}");
    // With that file gone, the region of `b` is nowhere, and the shared output holds
    // fewer of its regions than what was recorded for it, so the unit is rejected: it is
    // the other file that answered for it.
    // @lfy def/mcp/read.lfy:check#check:check:c6866961ccb185b658d527b660124a894f5a6e4c4c8a6b6b79f8d40199fd33a7
    fs::remove_file(fixture.root.join("out/b.rs")).unwrap();
    let error = check(&session, "a", None).unwrap_err();
    assert!(error.starts_with("rejected a:") && error.contains("out/shared.rs"), "{error}");
}

/// A compiler's check of its own unit is made against the request it was given, the files
/// before its run, and the notes it recorded, so the edits it made to a later unit's code
/// are judged and listed after its outputs; without a note for a fixed unit it is rejected.
// @lfy def/mcp/read.lfy:check#check:check:f62b2808006bb1ede584a2e554e66f850b587343f1593b99f7b47aeb05e5de6f
// @lfy def/mcp/read.lfy:check#check:check:45672f7351ea405dd7ffc891b1f97edf8bc53b0c6318964ac46234940a594410
#[test]
fn check_of_a_compiler_run_judges_its_edits_to_later_units() {
    let (fixture, session) = reached_project();
    edit_reached(&fixture);
    // The fix in `b` has no note yet.
    let error = check(&session, "a", None).unwrap_err();
    assert!(error.contains("gave no note"), "{error}");
    touch(&session, "b", "a renamed G.one to G.uno").unwrap();
    // @lfy def/mcp/read.lfy:check#check:check:118b7ce146ef04014cced08a2301ca2de11ee4af2a1bab5bb2eb970a0fa0b9c7
    let text = check(&session, "a", None).unwrap();
    assert!(text.starts_with("accepted a for rust: 1 output\n  out/a.rs\n"), "{text}");
    assert!(text.contains("\nfix in b out/b.rs:1 3 lines\n"), "{text}");
    assert!(text.contains("\nstub in b out/b.rs:7 1 lines\n"), "{text}");
    // A session bound to no run is given no files before a run, so the same edits are
    // nothing it judges.
    // @lfy def/mcp/read.lfy:check#check:check:c38c9a07a3699d11511265746c3a3d56f97ca029e28d14f4f2fbc5c31e955939
    // @lfy def/mcp/read.lfy:check#check:check:99ef27a8e37ec8ae852a1fbed7fb641118f4d212b8dc0298bd44088933fa5488
    let text = check(&fixture.session(), "a", None).unwrap();
    assert!(!text.contains("fix in"), "{text}");
}

// @lfy def/mcp/read.lfy:changes#changes:changes:76805f808eadb6afc31288f850876553118dd0066c28d603b5716c112369b09c
#[test]
fn changes_reports_what_differs_since_the_outputs_were_accepted() {
    let fixture = accepted_project();
    // The unit's file has changed since its outputs were accepted, and the root is no
    // repository, so no previous source can be recovered: the text says so first, then
    // every entity as added.
    // @lfy def/mcp/read.lfy:changes#changes:changes:b2b31f75eb94528c8ec7526264ada5e72ab1d96bfb72f1d04697c832979b751d
    fixture.write("def/b.lfy", "d B {}\nd C {}\n");
    let session = fixture.session();
    assert_eq!(recovered_of(&session, "b"), None);
    let text = changes(&session, "b", None).unwrap();
    assert!(text.starts_with("neither the source its outputs were accepted from nor"), "{text}");
    assert!(text.contains("\nB added "), "{text}");
    assert!(text.contains("\nC added "), "{text}");
    // The target may be named, and names the same unit.
    // @lfy def/mcp/read.lfy:changes#changes:changes:76805f808eadb6afc31288f850876553118dd0066c28d603b5716c112369b09c
    assert_eq!(changes(&session, "b", Some("rust")).unwrap(), text);
    // The outputs of a unit are empty.
    // @lfy def/mcp/read.lfy:changes#changes:changes:413343511b79d06a29fa38852b4bf26d600fdb0e411965a13d3190ca9375546e
    let text = changes(&session, "a", None).unwrap();
    assert!(text.starts_with("no previous output for a"), "{text}");
    // No unit has that stem.
    // @lfy def/mcp/read.lfy:changes#changes:changes:bbfd13869e5d8aa4ccdda753d25b4797e5140bfb3b5f287cf8f83310a3b51e7b
    let error = changes(&session, "zzz", None).unwrap_err();
    assert!(error.contains("no unit has the stem zzz"), "{error}");
    let error = changes(&session, "zzz", Some("rust")).unwrap_err();
    assert!(error.contains("no unit of the target rust has the stem zzz"), "{error}");
    // The target is set and is not a known target.
    // @lfy def/mcp/read.lfy:changes#changes:changes:e2e8756b78db77377231d8ef5ee9fd3e078703c801906a9b774ee59a2135a3ab
    let error = changes(&session, "b", Some("go")).unwrap_err();
    assert!(error.contains("go is not a target") && error.contains("rust"), "{error}");
}

/// The root is a repository holding the source the outputs were accepted against, so
/// that source is the previous text: the revision is found by the hash of its lowered
/// text, which is what the source map records, and the entities added since are the
/// only changes.
// @lfy def/mcp/read.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
#[test]
fn changes_recovers_the_previous_source_from_git() {
    let fixture = accepted_project();
    fixture
        .git(&["init", "--quiet"])
        .git(&["add", "-A"])
        .git(&[
            "-c",
            "user.email=p@example.com",
            "-c",
            "user.name=p",
            "commit",
            "--quiet",
            "-m",
            "the accepted source",
        ]);
    // The revision is found although the file on disk now carries a comment the
    // committed one did not, because it is matched by the hash of its lowered text and
    // a comment is not lowered.
    // @lfy def/mcp/read.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
    fixture.write("def/b.lfy", "// a comment\nd B {}\nd C {}\n");
    let session = fixture.session();
    // What is passed on is that revision as git holds it, never lowered: `git show` of
    // the revision the map's hash names.
    // @lfy def/mcp/read.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
    assert_eq!(recovered_of(&session, "b"), Some(("d B {}\n".to_string(), false)));
    // @lfy def/mcp/read.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
    let text = changes(&session, "b", None).unwrap();
    assert!(!text.contains("cannot be recovered"), "{text}");
    // `B` was declared in the recovered source, so only `C` is an addition.
    // @lfy def/mcp/read.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
    assert!(!text.contains("B added"), "{text}");
    assert!(text.lines().any(|line| line.starts_with("C added ")), "{text}");
}

/// A project whose unit `b` has an accepted output answering for one criterion, the
/// source it was accepted from, and its id: lowering drops the criteria, so any later
/// wording of that criterion lowers to the same code.
fn project_with_an_accepted_criterion() -> (Fixture, String, String) {
    let fixture = project_with_requirements();
    let local = requirement_id(&fixture, "It answers");
    fixture.write(
        "out/b.rs",
        &format!("// @lfy def/b.lfy:b#{local}\npub fn b() -> String {{\n    String::new()\n}}\n"),
    );
    record_unit(&fixture, "b");
    let accepted = fs::read_to_string(fixture.root.join("def/b.lfy")).unwrap();
    (fixture, accepted, local)
}

/// One commit in the fixture's root, with an author of its own so that a machine with no
/// git identity commits just the same.
fn commit(fixture: &Fixture, message: &str) {
    fixture.git(&["add", "-A"]).git(&[
        "-c",
        "user.email=p@example.com",
        "-c",
        "user.name=p",
        "commit",
        "--quiet",
        "-m",
        message,
    ]);
}

/// Two revisions of a file may lower to the same code and answer for different criteria,
/// so the source the outputs were accepted from is the most recent revision whose
/// criteria and tests are the ones the map records as answered, not merely the most
/// recent one that lowers to the same code.
// @lfy def/mcp/read.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
#[test]
fn changes_recovers_the_revision_the_outputs_were_accepted_from() {
    let (fixture, accepted, _) = project_with_an_accepted_criterion();
    fixture.git(&["init", "--quiet"]);
    commit(&fixture, "the accepted source");
    // A criterion reworded leaves the lowered code untouched, so the newer revision
    // lowers to the same code as the accepted one.
    // @lfy def/mcp/read.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
    fixture.write("def/b.lfy", &accepted.replace("It answers", "It replies"));
    commit(&fixture, "reworded");
    let session = fixture.session();
    let planned = plan_of(&session);
    let index = unit_of_stem(planned.workspace(), &planned.plan, "b", None).unwrap();
    let unit = &planned.plan.units[index];
    let lowered = generation::source_hash(&planned.program.files[unit.lowered].text);
    assert_eq!(lowered, unit.outputs[0].hash, "both revisions lower to the accepted code");
    // The older revision is the one the outputs were accepted from, and it is given back
    // as git holds it, criteria and all.
    // @lfy def/mcp/read.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
    assert_eq!(recovered_of(&session, "b"), Some((accepted.clone(), false)));
    // @lfy def/mcp/read.lfy:changes#changes:changes:53f95c52d3400ab227bfb9638eec7debe3964e9d9931c78c15358a719c56d337
    let text = changes(&session, "b", None).unwrap();
    assert!(!text.contains("is not known"), "{text}");
    // What differs since is the rewording of that criterion, which is a change to `b`.
    assert!(text.lines().any(|line| line.starts_with("b ")), "{text}");
}

/// Where no revision of the file holds the criteria and tests the map records, the
/// latest revision that lowers to the same code stands in for the source the outputs
/// were accepted from, and the text says what that means before anything else.
// @lfy def/mcp/read.lfy:changes#changes:changes:3b2a8142ec9ed2bb1f80958fc3261c1d1b4be77c99faa3057b834d1224905c31
#[test]
fn changes_falls_back_to_the_latest_revision_with_the_same_lowered_code() {
    let (fixture, accepted, _) = project_with_an_accepted_criterion();
    // Only a later wording of the criterion was ever committed, so every revision lowers
    // to the accepted code and none of them answers for what the map records.
    // @lfy def/mcp/read.lfy:changes#changes:changes:3b2a8142ec9ed2bb1f80958fc3261c1d1b4be77c99faa3057b834d1224905c31
    let reworded = accepted.replace("It answers", "It replies");
    fixture.write("def/b.lfy", &reworded);
    fixture.git(&["init", "--quiet"]);
    commit(&fixture, "a later wording");
    let session = fixture.session();
    // @lfy def/mcp/read.lfy:changes#changes:changes:3b2a8142ec9ed2bb1f80958fc3261c1d1b4be77c99faa3057b834d1224905c31
    assert_eq!(recovered_of(&session, "b"), Some((reworded.clone(), true)));
    // The text says so first: the source is not known, the changes are against the
    // latest revision with the same lowered code, and what they say of the criteria and
    // tests may not be what changed since the outputs were accepted.
    // @lfy def/mcp/read.lfy:changes#changes:changes:3b2a8142ec9ed2bb1f80958fc3261c1d1b4be77c99faa3057b834d1224905c31
    let text = changes(&session, "b", None).unwrap();
    let first = text.lines().next().unwrap();
    assert!(first.starts_with("the source its outputs were accepted from is not known"), "{text}");
    assert!(first.contains("the latest revision of its file with the same lowered code"), "{text}");
    assert!(first.contains("may not be those that changed since its outputs were accepted"), "{text}");
    // Nothing differs between that revision and the file as it is, so the rest says so.
    // @lfy def/mcp/read.lfy:changes#changes:changes:c91839604ea31b5f8cf98f38c9e887c17fa9d386f697dd83a406cd2307166b3d
    assert_eq!(text.lines().next_back(), Some("no changes in b since its outputs were accepted"));
}

// @lfy def/mcp/read.lfy:changes#changes:changes:c91839604ea31b5f8cf98f38c9e887c17fa9d386f697dd83a406cd2307166b3d
#[test]
fn changes_says_so_when_nothing_differs() {
    let fixture = accepted_project();
    let session = fixture.session();
    let planned = plan_of(&session);
    let index = unit_of_stem(planned.workspace(), &planned.plan, "b", None).unwrap();
    // The file as it is now is the file the outputs were generated from, so nothing
    // differs.
    // @lfy def/mcp/read.lfy:changes#changes:changes:c91839604ea31b5f8cf98f38c9e887c17fa9d386f697dd83a406cd2307166b3d
    let text = fs::read_to_string(fixture.root.join("def/b.lfy")).unwrap();
    let unit = &planned.plan.units[index];
    let was = generation::previous_of(planned.workspace(), unit, &text, false);
    assert!(generation::changes(&planned.program, unit, Some(&was)).is_empty());
}

/// A project for a target that one entity is built for and the entity it reaches is
/// not: the plan has a problem, so there is no request, while a review request is given
/// as for any plan and does not mention it.
// @lfy def/mcp/read.lfy:request#request:request:2bea1608ac7ace6999af5d0abfb20a8166b6e5149871e97cdc2660b9709891e0
#[test]
fn a_plan_with_problems_has_no_request_but_still_has_a_review() {
    let fixture = Fixture::new();
    fixture
        .with_rust_target()
        .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
        .write("def/b.lfy", "d B: `A B` { }\n")
        // A second target that only `A` is built for, so `A` reaches `B`, which is not
        // built for it.
        .write(
            "def/go.lfy",
            "use \"elfie/target\";\nuse \"./roles\";\nuse \"./a\";\n\n\
             ace const go = targetOf({\n  output = \"go\",\n  language = lang,\n  \
             layout = flat,\n  ecosystem = cargo,\n});\nA@targets.add(go);\n",
        );
    let session = fixture.session();
    let planned = plan_of(&session);
    // A plan with no problem would make this test prove nothing, so it is asserted.
    assert!(!planned.plan.problems.is_empty(), "{}", units(&session, None).unwrap());
    // Each problem is one line, as `problems` spells a diagnostic of stage generation
    // and severity error at its node.
    // @lfy def/mcp/read.lfy:units#units:units:e3b6f3a30f32b79ed993570429ffa615792d56cc2481b2b41aa6f859567ec450
    let listed = units(&session, None).unwrap();
    let first = listed.lines().next().unwrap();
    assert!(first.contains(": generation error: "), "{listed}");
    assert!(first.starts_with("def/"), "{listed}");
    // The problems come before the units.
    // @lfy def/mcp/read.lfy:units#units:units:920638da45d13abf0859774d9b78a3a3087c59fed19e9fe90e97b355194204b6
    let problem_at = listed.lines().position(|line| line.contains(": generation error: ")).unwrap();
    let unit_at = listed.lines().position(|line| line.starts_with("rust ")).unwrap();
    assert!(problem_at < unit_at, "{listed}");
    // There is no request.
    // @lfy def/mcp/read.lfy:request#request:request:2bea1608ac7ace6999af5d0abfb20a8166b6e5149871e97cdc2660b9709891e0
    let error = request(&session, "a", Some("rust")).unwrap_err();
    assert!(error.starts_with("the plan has problems, so there is no request:"), "{error}");
    assert!(error.contains(": generation error: "), "{error}");
    // The review request is given as for any plan, and does not mention the problems.
    // @lfy def/mcp/read.lfy:review#review:review:5c61bb2a8fa4f5d02a3219d0c4a1f8d385dbad4a9caf2a52fa73d59c10014617
    let instructions = review(&session, "b", Some("rust")).unwrap();
    assert!(instructions.contains("Reviewing the unit"), "{instructions}");
    assert!(!instructions.contains("generation error"), "{instructions}");
}

/// Each move is one line reading the word move, the target, the stem it came from, the
/// word into, the stem that compiles it, the identifiers moved in brackets, the word
/// for, and the cause.
// Decision: the planner of this fixture resolves a mutual use without moving anything,
// so the line is read off a move built here rather than off a project contrived to make
// one; the line is what the criterion is about.
// @lfy def/mcp/read.lfy:units#units:units:04942eee0b664f2b18f16b21c1e245d6ec558e513af796471b999cea68d090c6
#[test]
fn a_move_is_one_line_naming_what_moved_and_why() {
    let fixture = compiled_project();
    let session = fixture.session();
    let planned = plan_of(&session);
    let workspace = planned.workspace();
    let plan = &planned.plan;
    let of_b = (0..plan.units.len()).find(|&unit| plan.units[unit].stem == "b").unwrap();
    let of_a = (0..plan.units.len()).find(|&unit| plan.units[unit].stem == "a").unwrap();
    let moved_entity = query::find(workspace, "B")[0];
    let moved = Move {
        entities: vec![moved_entity],
        origin: of_b,
        into: of_a,
        cause: "A.b".to_string(),
    };
    // @lfy def/mcp/read.lfy:units#units:units:04942eee0b664f2b18f16b21c1e245d6ec558e513af796471b999cea68d090c6
    assert_eq!(move_line(workspace, plan, &moved), "move rust b into a [B] for A.b");
}

/// A unit whose output answers for a violated review of the last [`GlobalReview`] is
/// planned with the reason `violated`; without that review the plan is made with no
/// violated unit at all.
// @lfy def/mcp/read.lfy:units#units:units:141e46a551381ad83b1cc8266dc7d3bca58959091eeaa85ab4768b13d4c1f99f
#[test]
fn units_plans_a_violated_unit_again() {
    let fixture = project_with_requirements();
    let global = requirement_id(&fixture, "Nothing is written outside the output directory");
    fixture.write(
        "out/b.rs",
        &format!("// @lfy def/b.lfy:b#{global}\npub fn b() -> String {{\n    String::new()\n}}\n"),
    );
    record_unit(&fixture, "b");

    // There is no last global review, so the plan is made with no violated unit and the
    // recorded unit is up to date.
    // @lfy def/mcp/read.lfy:units#units:units:141e46a551381ad83b1cc8266dc7d3bca58959091eeaa85ab4768b13d4c1f99f
    let session = fixture.session();
    assert_eq!(units(&session, None).unwrap(), "rust b up to date []");

    // A review that is satisfied names no violated unit either. The file is written as
    // the command line writes it: a global review stringified.
    // @lfy def/mcp/read.lfy:units#units:units:141e46a551381ad83b1cc8266dc7d3bca58959091eeaa85ab4768b13d4c1f99f
    let review_file = |status: &str| {
        let last = GlobalReview {
            requirements: "0".repeat(64),
            reviews: vec![Review {
                id: global.clone(),
                file: String::new(),
                line: 0,
                entity: String::new(),
                status: ReviewStatus::from_name(status).unwrap(),
                evidence: "out/b.rs:1-4".to_string(),
                note: "it is under out".to_string(),
            }],
            problems: Vec::new(),
            unplaced: Vec::new(),
            missing: Vec::new(),
        };
        format!("{}\n", last.to_json())
    };
    fixture.write("elfie-requests/global.reviews.json", &review_file("satisfied"));
    let session = fixture.session();
    assert_eq!(units(&session, None).unwrap(), "rust b up to date []");

    // The review is violated and a marker of the unit's output answers for its id, so
    // the unit is planned again.
    // @lfy def/mcp/read.lfy:units#units:units:141e46a551381ad83b1cc8266dc7d3bca58959091eeaa85ab4768b13d4c1f99f
    fixture.write("elfie-requests/global.reviews.json", &review_file("violated"));
    let session = fixture.session();
    assert_eq!(units(&session, None).unwrap(), "rust b violated []");
}
