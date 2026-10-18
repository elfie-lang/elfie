// @lfy def/mcp/main.lfy:review
//! Tests of `review`, `global_review`, `report`, and `report_many`.

use super::*;
use crate::verdicts::asked_of;

// @lfy def/mcp/main.lfy:review#review:review:fdaa4bc8880c49e6db561ce8263cc3c4823659432b439716a64cd674b0e8e68a
#[test]
fn review_gives_the_review_request_of_one_unit() {
    let fixture = project_with_requirements();
    let session = fixture.session();
    // The request is made with no review record, so every criterion of the unit is asked
    // about however an earlier review found it.
    // @lfy def/mcp/main.lfy:review#review:review:fdaa4bc8880c49e6db561ce8263cc3c4823659432b439716a64cd674b0e8e68a
    let instructions = review(&session, "b", None).unwrap();
    assert!(instructions.contains("Reviewing the unit"), "{instructions}");
    assert!(instructions.contains("It answers"), "{instructions}");
    assert_eq!(review(&session, "b", Some("rust")).unwrap(), instructions);
    // No unit has that stem, or the target is not one.
    // @lfy def/mcp/main.lfy:review#review:review:35e5ad0177134a3f3f1deca8a9b000be17f11403bc5bb2ae319face434d2913f
    let error = review(&session, "c", None).unwrap_err();
    assert!(error.contains("no unit has the stem c"), "{error}");
    let error = review(&session, "b", Some("go")).unwrap_err();
    assert!(error.contains("go is not a target"), "{error}");
    // A unit that carries no criterion and no test still has a review request.
    let bare = compiled_project();
    let session = bare.session();
    let instructions = review(&session, "a", None).unwrap();
    assert!(instructions.contains("Reviewing the unit"), "{instructions}");
}

/// A project whose one fn carries two local criteria, each beginning on a line of its
/// own so that each is named by its place as well as by its id, and one global
/// criterion, which a review of the unit never asks about.
fn project_with_two_criteria() -> Fixture {
    let fixture = Fixture::new();
    fixture.with_rust_target().write(
        "def/a.lfy",
        "fn a(): `An a` => string {\n  \
         @acceptanceCriteria.add({ behavior = `It answers` });\n  \
         @acceptanceCriteria.add({ behavior = `It answers in one line` });\n}\n\
         global@acceptanceCriteria.add({ behavior = `Nothing is written outside the output directory` });\n",
    );
    fixture
}

/// The file and the line of the criterion or test an id names, as a review request
/// gives the place.
fn place_of_requirement(session: &Session, id: &str) -> (String, usize) {
    let line = serde_json::json!({ "id": id, "status": "satisfied", "evidence": "", "note": "" }).to_string();
    let program = &session.binding.as_ref().expect("a binding").program;
    let found = generation::review_of(&line, program, &[], &[])
        .reviews
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("a criterion named {id}"));
    (found.file, found.line)
}

/// A verdict reported by its place is recorded for the criterion that place names, with
/// its status, evidence, and note; the text says so and counts what is still missing. A
/// status naming none of the three, an id naming no criterion, and an id the review did
/// not ask about each record nothing.
// @lfy def/mcp/main.lfy:report#report:report:34ee53fe216df9928c40072f557778f5a2188225c7e5884099686da22499f026
// @lfy def/mcp/main.lfy:report#report:report:00107440d32bd506e9119bd632f7552b025f2dfc4f0e24ebaf0fd98a73fb2141
// @lfy def/mcp/main.lfy:report#report:report:1899e3f7c1c027232563ba6f23f541c4c864d9ac2021cf429ec4439cce298785
#[test]
fn report_records_one_verdict_and_counts_what_is_still_missing() {
    let fixture = project_with_two_criteria();
    let session = bound(&fixture, binding_of(&fixture, Role::Verifier, Some("a")));
    let asked = asked_of(&session).to_vec();
    assert_eq!(asked.len(), 2, "{asked:?}");
    let first = requirement_id(&fixture, "It answers");
    let second = requirement_id(&fixture, "It answers in one line");
    assert!(asked.contains(&first) && asked.contains(&second), "{asked:?}");

    // Reported by its place rather than by its id: the criterion that place names is
    // recorded, with the status, the evidence, and the note given.
    // @lfy def/mcp/main.lfy:report#report:report:15a298dff4d1c9c629d5217e0802615370e072e57d444f4abd189e391365165b
    // @lfy def/mcp/main.lfy:report#report:report:82b739ccf95aa1e1faa11800427ce14cadd3fdeef5e2c7904cc9999d70958a3a
    let (file, line) = place_of_requirement(&session, &second);
    let text = report(
        &session,
        None,
        Some(&file),
        Some(line),
        "violated",
        "out/a.rs:12-18",
        "returns undefined instead of an empty list",
    )
    .unwrap();
    // @lfy def/mcp/main.lfy:report#report:report:cb0ab5dd13a5f32de066f7ec167f81ff4853716054b8ebe9eae3c72af49a0c4b
    assert_eq!(text.lines().next(), Some(format!("recorded {second} at {file}:{line}").as_str()));
    // @lfy def/mcp/main.lfy:report#report:report:33d92b578b9d5f560fea04be01b80b698d6cef193437113b87d6767239dba56f
    assert!(text.contains(&format!("1 still missing: {first}")), "{text}");
    let recorded = session.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].id, second);
    assert_eq!(recorded[0].status, ReviewStatus::Violated);
    assert_eq!(recorded[0].evidence, "out/a.rs:12-18");
    assert_eq!(recorded[0].note, "returns undefined instead of an empty list");
    // Nothing is written: the verdict is kept in the session alone.
    // @lfy def/mcp/main.lfy:report#report:report:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
    assert!(!fixture.root.join("elfie-requests").exists());

    // An id that names no criterion or test of the review records nothing.
    // @lfy def/mcp/main.lfy:report#report:report:3d3b34634644a0a77e7ee4995575b04f369d10d6495cab7ed34e712d6c0df7d7
    let error = report(&session, Some("A:A:77aa"), None, None, "satisfied", "", "ok").unwrap_err();
    assert!(error.contains("A:A:77aa") && error.contains("no criterion or test"), "{error}");
    assert_eq!(session.recorded().len(), 1);
    // A status naming none of the three records nothing and names the three.
    // @lfy def/mcp/main.lfy:report#report:report:f95e4040e1b2d8658bf53cbe5eaf320a0628d57aafdf090791dea862afeb67fd
    let error = report(&session, Some(&first), None, None, "maybe", "", "unsure").unwrap_err();
    assert!(error.contains("satisfied") && error.contains("violated") && error.contains("unverifiable"), "{error}");
    assert_eq!(session.recorded().len(), 1);
    // A criterion of the program the review did not ask about records nothing either.
    // @lfy def/mcp/main.lfy:report#report:report:eecbccd9ec901434b61790fd663bb5081ca97c913c2328df04808e7f3755d06a
    let global = requirement_id(&fixture, "Nothing is written outside the output directory");
    let error = report(&session, Some(&global), None, None, "satisfied", "", "ok").unwrap_err();
    assert!(error.contains("was not asked about in this review"), "{error}");
    assert_eq!(session.recorded().len(), 1);

    // Every id asked about has a verdict: the text says so and how the final message
    // ends.
    // @lfy def/mcp/main.lfy:report#report:report:979112dd7ac1a472af0b4b0b3b534d9bf9f6464235b53bb1523bcb7fab70370c
    let text = report(&session, Some(&first), None, None, "satisfied", "", "it answers").unwrap();
    assert!(text.contains("every criterion and test asked about has a verdict"), "{text}");
    assert!(text.contains(generation::REVIEWED), "{text}");
    assert_eq!(session.recorded().len(), 2);
    // A second verdict on one criterion stands, and the earlier one is removed.
    // @lfy def/mcp/main.lfy:report#report:report:bfa37d307a244c3332ac692e347ce5f2a534f0f2a1f839c0cab30318da4f37cc
    let text = report(&session, Some(&first), None, None, "UNVERIFIABLE", "", "no region answers").unwrap();
    assert!(text.starts_with(&format!("recorded {first} ")), "{text}");
    let recorded = session.recorded();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].id, first);
    assert_eq!(recorded[1].status, ReviewStatus::Unverifiable);
}

/// Several verdicts in one call: each line that is a JSON object is handled as `report`
/// handles what that line holds, a line of white space alone is skipped, and a line that
/// is no JSON object is an error of its own that never keeps a later line from being
/// handled. The text holds one line for each line handled, in the order written, and then
/// what is still missing.
// @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:b0b09eb45dab5103137288326fe35b1eb10effb35bd4940585aeb5ee3ff559ff
// @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:058b0e578727415727d2873acf14d412ffdc57e2b6544717a2d9d0c1a1ba17fa
#[test]
fn report_many_records_the_verdicts_of_its_lines_one_by_one() {
    let fixture = project_with_two_criteria();
    let session = bound(&fixture, binding_of(&fixture, Role::Verifier, Some("a")));
    let first = requirement_id(&fixture, "It answers");
    let second = requirement_id(&fixture, "It answers in one line");
    let (file, line) = place_of_requirement(&session, &second);
    let lines = format!(
        "{}\n   \n{}\n",
        serde_json::json!({
            "file": file,
            "line": line,
            "status": "violated",
            "evidence": "out/a.rs:12-18",
            "note": "returns undefined instead of an empty list",
        }),
        "satisfied A:A:1f2e",
    );
    let text = report_many(&session, &lines).unwrap();
    let said: Vec<&str> = text.lines().collect();
    assert_eq!(said.len(), 3, "{text}");
    assert_eq!(said[0], format!("recorded {second} at {file}:{line}"));
    assert!(said[1].contains("no JSON object") && said[1].contains("satisfied A:A:1f2e"), "{text}");
    assert_eq!(said[2], format!("1 still missing: {first}"));
    let recorded = session.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].id, second);
    assert_eq!(recorded[0].status, ReviewStatus::Violated);
    assert_eq!(recorded[0].evidence, "out/a.rs:12-18");
    assert_eq!(recorded[0].note, "returns undefined instead of an empty list");
    // Nothing is written: the verdicts are kept in the session alone.
    assert!(!fixture.root.join("elfie-requests").exists());

    // The first line names no criterion of the review, which is an error for that line
    // alone; the second is recorded, and every id asked about then has a verdict. The
    // first line lacks `file`, `line`, and `evidence`, which are left out and empty.
    let lines = format!(
        "{}\n{}\n",
        serde_json::json!({ "id": "A:A:77aa", "status": "satisfied", "note": "ok" }),
        serde_json::json!({ "id": first, "status": "satisfied", "evidence": "out/a.rs:3-9", "note": "ok" }),
    );
    let text = report_many(&session, &lines).unwrap();
    let said: Vec<&str> = text.lines().collect();
    assert_eq!(said.len(), 3, "{text}");
    assert!(said[0].contains("A:A:77aa") && said[0].contains("no criterion or test"), "{text}");
    assert!(said[1].starts_with(&format!("recorded {first} ")), "{text}");
    assert!(said[2].contains("every criterion and test asked about has a verdict"), "{text}");
    assert!(said[2].contains(generation::REVIEWED), "{text}");
    let recorded = session.recorded();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].id, first);
    assert_eq!(recorded[1].evidence, "out/a.rs:3-9");

    // A call that records nothing is an error, and the text still says what each line
    // came to.
    let error = report_many(&session, "satisfied A:A:1f2e\n").unwrap_err();
    assert!(error.contains("no JSON object"), "{error}");
    assert_eq!(session.recorded().len(), 2);
}

// @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:b93638e7046f17fc2cf321187664d92150028acae44164992beee7e46349fb1c
#[test]
fn global_review_gives_the_review_request_of_every_global_requirement() {
    let fixture = project_with_requirements();
    let session = fixture.session();
    assert_eq!(problems(&session, None).unwrap(), "no problems");
    let instructions = global_review(&session).unwrap();
    assert!(instructions.contains("global review"), "{instructions}");
    assert!(instructions.contains("Nothing is written outside the output directory"), "{instructions}");

    // No global criterion or test exists.
    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:f656c023948494da8d316780c15c2c1b96e1512caafb62d843dc6295be0d89be
    let bare = compiled_project();
    let session = bare.session();
    assert_eq!(global_review(&session).unwrap(), "no global criterion or test exists in the program");
}
