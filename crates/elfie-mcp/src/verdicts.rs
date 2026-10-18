// @lfy def/mcp/main.lfy:review
//! What a review asks and what it is answered: the review request for one unit, the one
//! for every global criterion and test, and the verdicts a verifier submits.

use super::*;

/// The full review request for one unit: every criterion and test with where the regions
/// generated for its entities are, and how to report.
///
/// The problems of the plan are not mentioned: a review reads the outputs that exist
/// against what was asked of them, which a plan that could not be made whole does not
/// change.
// @lfy def/mcp/main.lfy:review#review:review:5c61bb2a8fa4f5d02a3219d0c4a1f8d385dbad4a9caf2a52fa73d59c10014617
pub fn review(session: &Session, unit: &str, target: Option<&str>) -> Result<String, String> {
    let planned = plan_of(session);
    let plan = &planned.plan;
    // An error says which when no unit of the target has the stem, or the target is
    // ambiguous.
    // @lfy def/mcp/main.lfy:review#review:review:35e5ad0177134a3f3f1deca8a9b000be17f11403bc5bb2ae319face434d2913f
    let index = resolve_unit(planned.workspace(), plan, unit, target)?;
    // @lfy def/mcp/main.lfy:review#review:review:47939b7e26a096d51016320d59ba7e3a38ffd7f43690c663bdeb522b5f87dcf3
    own_unit(session, plan, index)?;
    // A verifier is given the review request it was run with, so what it is asked about is
    // what the compile asked it, never a request made again from the outputs as they are
    // now; every other session has the request made for the unit.
    // @lfy def/mcp/main.lfy:review#review:review:089fcf12de8d9e51b3bbb0617a1751ca69d441fac98223543f48f619ee444d8b
    if let Some(binding) = &session.binding
        && binding.role == Role::Verifier
        && let Some(given) = &binding.review
    {
        return Ok(given.instructions.clone());
    }
    // @lfy def/mcp/main.lfy:review#review:review:fdaa4bc8880c49e6db561ce8263cc3c4823659432b439716a64cd674b0e8e68a
    Ok(
        generation::review(
            &planned.program,
            plan,
            index,
            &planned.maps,
            None,
            &[],
        )
        .instructions,
    )
}

/// The review request for every global criterion and test, once for the whole program, with
/// where the regions that answer for each are.
// @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:b93638e7046f17fc2cf321187664d92150028acae44164992beee7e46349fb1c
pub fn global_review(session: &Session) -> Result<String, String> {
    let planned = plan_of(session);
    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:f656c023948494da8d316780c15c2c1b96e1512caafb62d843dc6295be0d89be
    if planned.program.criteria.is_empty() && planned.program.tests.is_empty() {
        return Ok("no global criterion or test exists in the program".to_string());
    }
    // @lfy def/mcp/main.lfy:globalReview#globalReview:globalReview:b93638e7046f17fc2cf321187664d92150028acae44164992beee7e46349fb1c
    Ok(generation::global_review(&planned.program, &planned.maps).instructions)
}

// ---- the verdicts of a review -------------------------------------------------------

// Decision: a verdict is submitted as soon as it is judged and checked as it arrives, so a
// verdict with a wrong id is answered at once, while the verifier still knows what it
// meant, and none is lost to a final message that ends too soon or runs too long.

/// How many of the ids still missing a verdict are listed before the rest are only counted.
// @lfy def/mcp/main.lfy:report#report:report:33d92b578b9d5f560fea04be01b80b698d6cef193437113b87d6767239dba56f
const LISTED_MISSING: usize = 5;

/// The ids the review of a session asked about; empty when the session is bound to no
/// review.
// @lfy def/mcp/main.lfy:report#report:report:eecbccd9ec901434b61790fd663bb5081ca97c913c2328df04808e7f3755d06a
pub(crate) fn asked_of(session: &Session) -> &[String] {
    session
        .binding
        .as_ref()
        .and_then(|binding| binding.review.as_ref())
        .map_or(&[], |review| review.asked.as_slice())
}

/// One verdict recorded in the session, or why it was not: the line of text a call answers
/// with for the one verdict `report` is given, and for one line of what `report_many` is
/// given.
fn record_verdict(
    session: &Session,
    id: Option<&str>,
    file: Option<&str>,
    line: Option<usize>,
    status: &str,
    evidence: &str,
    note: &str,
) -> Result<String, String> {
    // The status is read first, so a status naming none of the three records nothing and is
    // answered with the three it could have named.
    // @lfy def/mcp/main.lfy:report#report:report:f95e4040e1b2d8658bf53cbe5eaf320a0628d57aafdf090791dea862afeb67fd
    if ReviewStatus::from_name(&status.to_ascii_lowercase()).is_none() {
        return Err(format!(
            "the status {status} is none of {}, {}, and {}",
            ReviewStatus::Satisfied,
            ReviewStatus::Violated,
            ReviewStatus::Unverifiable
        ));
    }
    // The criterion or test reported on is the one the review report reads a review line as
    // being for, so one line holding what was given is read exactly as a line of a final
    // message would be.
    // @lfy def/mcp/main.lfy:report#report:report:15a298dff4d1c9c629d5217e0802615370e072e57d444f4abd189e391365165b
    let submitted = serde_json::json!({
        "id": id.unwrap_or_default(),
        "file": file.unwrap_or_default(),
        "line": line.unwrap_or_default(),
        "status": status,
        "evidence": evidence,
        "note": note,
    })
    .to_string();
    let mut lowered = None;
    // @lfy def/mcp/main.lfy:report#report:report:15a298dff4d1c9c629d5217e0802615370e072e57d444f4abd189e391365165b
    let program = match &session.binding {
        Some(binding) => &binding.program,
        None => lowered.insert(interpret::lower(session.workspace.clone())),
    };
    let read = generation::review_of(&submitted, program, &[], &[]);
    // @lfy def/mcp/main.lfy:report#report:report:3d3b34634644a0a77e7ee4995575b04f369d10d6495cab7ed34e712d6c0df7d7
    let Some(review) = read.reviews.into_iter().next() else {
        let named = match (id.filter(|id| !id.is_empty()), file, line) {
            (Some(id), _, _) => format!("the id {id}"),
            (None, Some(file), Some(line)) => format!("the place {file}:{line}"),
            (None, Some(file), None) => format!("the file {file}"),
            (None, None, Some(line)) => format!("the line {line}"),
            (None, None, None) => "neither an id nor a place".to_string(),
        };
        return Err(format!("no criterion or test of this review matches {named}"));
    };
    // A verdict on a criterion this review did not ask about is no verdict of this review.
    // @lfy def/mcp/main.lfy:report#report:report:eecbccd9ec901434b61790fd663bb5081ca97c913c2328df04808e7f3755d06a
    let asked = asked_of(session);
    if !asked.iter().any(|one| one == &review.id) {
        return Err(format!("{} was not asked about in this review", review.id));
    }
    let mut recorded = session.reviews.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // Of two verdicts on one criterion the later one stands, so the earlier is removed.
    // @lfy def/mcp/main.lfy:report#report:report:bfa37d307a244c3332ac692e347ce5f2a534f0f2a1f839c0cab30318da4f37cc
    recorded.retain(|kept| kept.id != review.id);
    // @lfy def/mcp/main.lfy:report#report:report:82b739ccf95aa1e1faa11800427ce14cadd3fdeef5e2c7904cc9999d70958a3a
    let place = format!("{}:{}", review.file, review.line);
    let named = review.id.clone();
    recorded.push(review);
    // @lfy def/mcp/main.lfy:report#report:report:cb0ab5dd13a5f32de066f7ec167f81ff4853716054b8ebe9eae3c72af49a0c4b
    Ok(format!("recorded {named} at {place}"))
}

/// What the ids a review asked about that still have no verdict come to, once every verdict
/// a call records is recorded: their count and the first few of them in the order asked, or
/// the line that says none of them is left.
// @lfy def/mcp/main.lfy:report#report:report:33d92b578b9d5f560fea04be01b80b698d6cef193437113b87d6767239dba56f
// @lfy def/mcp/main.lfy:report#report:report:979112dd7ac1a472af0b4b0b3b534d9bf9f6464235b53bb1523bcb7fab70370c
// @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:2d9160c89dd9bd47dfa8dad2056ba095d09c2aa6e220cc04f6a26c6d27a41ae5
fn still_missing(session: &Session) -> String {
    let asked = asked_of(session);
    let recorded = session.reviews.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let missing: Vec<&str> = asked
        .iter()
        .filter(|id| !recorded.iter().any(|review| &review.id == *id))
        .map(String::as_str)
        .collect();
    if missing.is_empty() {
        return format!(
            "every criterion and test asked about has a verdict; end your final message with the one line {}",
            generation::REVIEWED
        );
    }
    let listed: Vec<&str> = missing.iter().take(LISTED_MISSING).copied().collect();
    format!("{} still missing: {}", missing.len(), listed.join(", "))
}

/// Submit the verdict on one criterion or test of the review this session is for.
///
/// Nothing is written: the verdict is kept in the session until the CLI closes it.
// @lfy def/mcp/main.lfy:report#report:report:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn report(
    session: &Session,
    id: Option<&str>,
    file: Option<&str>,
    line: Option<usize>,
    status: &str,
    evidence: &str,
    note: &str,
) -> Result<String, String> {
    let recorded = record_verdict(session, id, file, line, status, evidence, note)?;
    Ok(format!("{recorded}\n{}", still_missing(session)))
}

/// What one line of what `report_many` is given holds, read as a JSON object: what `report`
/// would be given by a call naming each key, with a key the line lacks left out, or empty
/// where a parameter takes no option.
// @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:8817d6377a9a3ed8bf549e88bb1852988f80ba15cbb65696edbd09428290c41f
fn verdict_of(given: &JsonObject) -> (Option<&str>, Option<&str>, Option<usize>, &str, &str, &str) {
    (
        given.get("id").and_then(Value::as_str),
        given.get("file").and_then(Value::as_str),
        given.get("line").and_then(Value::as_u64).map(|line| line as usize),
        given.get("status").and_then(Value::as_str).unwrap_or_default(),
        given.get("evidence").and_then(Value::as_str).unwrap_or_default(),
        given.get("note").and_then(Value::as_str).unwrap_or_default(),
    )
}

/// Submit the verdicts on several criteria and tests of the review this session is for, one
/// per line.
///
/// Nothing is written: the verdicts are kept in the session until the CLI closes it.
// @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:de07280ad3ad9fe61fb4b298176271eb0020a12f42c53f6eaf8d5073a1cf2bc7
pub fn report_many(session: &Session, lines: &str) -> Result<String, String> {
    let mut answers: Vec<String> = Vec::new();
    let mut any = false;
    // @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:d4895ca8f18768836ee19b2767bbb1a0591dad65c2589d0adedbe0436107daf4
    for line in lines.lines() {
        // @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:66828069db5f41b73b8ab00d20e1c5f5ceb268ba68fc34dd1085e933e68f66a8
        if line.trim().is_empty() {
            continue;
        }
        let read = serde_json::from_str::<Value>(line).ok();
        // @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:9977dfb94cd50c90c1801765e39470abd3708f645294d939eb21eb83a42c49a5
        let Some(Value::Object(given)) = read else {
            answers.push(format!("the line is no JSON object: {line}"));
            continue;
        };
        let (id, file, at, status, evidence, note) = verdict_of(&given);
        // @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:c35940031d97e8aed0ee2632eff7713f344217183fb76010ea91b15baab460cb
        // @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:64ed5ff977322d195a6aa50a8684c320b64e96182fd1564663804e1ce3567b83
        match record_verdict(session, id, file, at, status, evidence, note) {
            Ok(recorded) => {
                any = true;
                answers.push(recorded);
            }
            Err(why) => answers.push(why),
        }
    }
    // @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:d28b2fa5b27d4085036fc3c0f3f0fd19b2f361b6d50e82a65ed62745dd2a6f6c
    answers.push(still_missing(session));
    // @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:9541cc1d33d5274076f6526030e2393a3048d50d5f0653a0ba85f76654cdbde4
    let text = answers.join("\n");
    // @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:c095b55b2ada374d5d412e7cd90f75c2876e241f6e6a2405221c471e3c9f0112
    // @lfy def/mcp/main.lfy:reportMany#reportMany:reportMany:926b1cd477f95b79f6e4661ce279bf4fa6c52f4548690cff19daad07e461afea
    if any { Ok(text) } else { Err(text) }
}

