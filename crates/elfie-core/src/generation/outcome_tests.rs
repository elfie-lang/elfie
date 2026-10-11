// @lfy def/generation/main.lfy:outcomeOf
//! Tests of `outcomeOf`.

use super::*;

fn accepted_verdict() -> Verdict {
    Verdict {
        accepted: true,
        problems: Vec::new(),
        source_maps: Vec::new(),
        outputs: Vec::new(),
    }
}

/// One verdict with one problem, for the runs whose outcome a rejection decides.
fn rejected_verdict(problem: &str) -> Verdict {
    Verdict {
        accepted: false,
        problems: vec![problem.to_string(), "and another".to_string()],
        source_maps: Vec::new(),
        outputs: Vec::new(),
    }
}

// @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:e5324f4bea05fae7e9593527cb97051401fa31bf436b8e19acaf8723aa328256
#[test]
fn a_report_ending_in_done_with_accepted_verdicts_is_accepted() {
    let outcome = outcome_of("I wrote it.\nELFIE: DONE", vec![accepted_verdict()], None);
    // @lfy def/generation/main.lfy:outcomeOf
    assert_eq!(outcome.kind, OutcomeKind::Accepted);
    assert_eq!(outcome.message, "");
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:2de4f51d5b06b9099294d93a631c4c53a3fac5e7cde4436e300269b119ed21b8
    assert_eq!(outcome.verdicts.len(), 1);
}

// @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:7b6eac4728a15e61226e1f610c492516a8c084cb43b331c9a358884b2a104e73
#[test]
fn a_report_ending_in_clarify_is_a_clarification_with_the_question() {
    let outcome = outcome_of(
        "ELFIE: CLARIFY: should Range.end be inclusive?",
        Vec::new(),
        None,
    );
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:88f72bf6172d2e7ca8cb8cc9cc191d01af0d765a70d6c2b3146a244d430bc889
    assert_eq!(outcome.kind, OutcomeKind::Clarification);
    assert_eq!(outcome.message, "should Range.end be inclusive?");
    assert!(outcome.verdicts.is_empty());
}

// @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:c5c86395d450173628730a07f7818f5a9b3639fe8116721227b43fb2a61abb5d
#[test]
fn a_report_ending_in_blocked_is_blocked_with_the_reason_and_every_line_after_it() {
    let outcome = outcome_of(
        "ELFIE: BLOCKED: [[tokenAt]] contradicts [[nodesAt]] on empty files\nand on one-line files",
        Vec::new(),
        None,
    );
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:b49d5420de2c94cb85a8ea62b3d2f39a0ea55b1b8867b33ca9d7f24d0eb760c2
    assert_eq!(outcome.kind, OutcomeKind::Blocked);
    assert_eq!(
        outcome.message,
        "[[tokenAt]] contradicts [[nodesAt]] on empty files\nand on one-line files"
    );
    // The last such line decides, whatever came before it.
    let outcome = outcome_of("ELFIE: DONE\nELFIE: BLOCKED: no", Vec::new(), None);
    assert_eq!(outcome.kind, OutcomeKind::Blocked);
    assert_eq!(outcome.message, "no");
}

// @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:0fccd9b62ccb962587945c80c1ea93a6c4a0a14d715a11382de9069f057a8717
#[test]
fn a_report_with_nothing_and_no_verdicts_has_failed() {
    let outcome = outcome_of("", Vec::new(), None);
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert_eq!(outcome.message, "the compiler reported nothing");
    assert!(outcome.verdicts.is_empty());
    // Prose alone, with no line beginning with `ELFIE:`, has failed too.
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:21fbd937a6b58a02072c2e606c0188be26f64751f8a10f7d260dea1d88e01a21
    assert_eq!(
        outcome_of("I had a look around.", Vec::new(), None).kind,
        OutcomeKind::Failed
    );
}

/// A report that ended but left no verdict is accepted: the compiler said it was done,
/// and no verdict says otherwise.
// @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:9b1f1e3d496ed3a527d2e2d93703ab7d0488cd057539e9129a315be84523bcca
#[test]
fn a_report_that_ended_with_no_verdicts_is_accepted() {
    let outcome = outcome_of("ELFIE: DONE", Vec::new(), None);
    assert_eq!(outcome.kind, OutcomeKind::Accepted);
    assert_eq!(outcome.message, "");
    assert!(outcome.verdicts.is_empty());
}

// @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:0e7476527a35bfd8b4cf324745dc9e2aa10aa07b1fb7a1d6da3f32055f0154ac
#[test]
fn a_rejected_verdict_is_rejected_with_its_first_problem() {
    let rejected = rejected_verdict("the entity B has no marker");
    let outcome = outcome_of("ELFIE: DONE", vec![accepted_verdict(), rejected], None);
    assert_eq!(outcome.kind, OutcomeKind::Rejected);
    assert_eq!(outcome.message, "the entity B has no marker");
    assert_eq!(outcome.verdicts.len(), 2);
}

/// A submitted outcome decides on its own: the report still ends in `ELFIE: DONE`, and
/// the run is blocked with the message the compiler submitted.
// @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:10b502beb5a91d5c287faae6f0200646349dc3e3bc3a83da1ab4d567904ec23d
#[test]
fn a_submitted_block_wins_over_a_report_that_ended_in_done() {
    let submitted = Submission {
        outcome: SubmittedOutcome::Blocked,
        message: "two criteria of f conflict".to_string(),
    };
    let outcome = outcome_of(
        "I wrote it.\nELFIE: DONE",
        vec![accepted_verdict()],
        Some(&submitted),
    );
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:b45e8bdae8388f9c4a41678f0f3df59a004bd2edf3747f0da5dcf5dfde13c18b
    assert_eq!(outcome.kind, OutcomeKind::Blocked);
    assert_eq!(outcome.message, "two criteria of f conflict");
    assert_eq!(outcome.verdicts.len(), 1);

    // A submitted question is a clarification the same way, whatever the report says.
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:8ec2e69b637448582ca10cfc40a3268b7ac51e3c0400efb384bf91954e650bc1
    let asked = Submission {
        outcome: SubmittedOutcome::Clarify,
        message: "should Range.end be inclusive?".to_string(),
    };
    let outcome = outcome_of("ELFIE: DONE", Vec::new(), Some(&asked));
    assert_eq!(outcome.kind, OutcomeKind::Clarification);
    assert_eq!(outcome.message, "should Range.end be inclusive?");
}

/// A compiler that submitted `done` is still held to the verdicts of its units, and the
/// note it submitted with `done` is no part of the message.
// @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:046437ef354a9e453f2f8ca9571b5bb39d5795da87aa09b71759be29bf95c1e2
#[test]
fn a_submitted_done_with_a_rejected_verdict_is_rejected() {
    let submitted = Submission {
        outcome: SubmittedOutcome::Done,
        message: "I had to move a helper".to_string(),
    };
    let outcome = outcome_of(
        "",
        vec![rejected_verdict("no marker for B")],
        Some(&submitted),
    );
    assert_eq!(outcome.kind, OutcomeKind::Rejected);
    assert_eq!(outcome.message, "no marker for B");
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:0caaac66529a98a351e460ed1abb1858129a0773b2fa6e8f02250c06b9005093
    assert!(!outcome.message.contains("I had to move a helper"));

    // With every verdict accepted, a submitted `done` is accepted and the note is gone.
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:6cbc44baf902ab337f838bc362fc27fd6caace8df9741d15d5c4151081cc920b
    let outcome = outcome_of("", vec![accepted_verdict()], Some(&submitted));
    assert_eq!(outcome.kind, OutcomeKind::Accepted);
    assert_eq!(outcome.message, "");
}

/// A compiler that submitted `done` while no unit was judged has failed: its outputs
/// never reached acceptance, whatever its report said.
// @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:a6355c8129add74ebe6d1ca1d6f07a38256353426e20a7f3b9f0de624dad10e4
#[test]
fn a_submitted_done_with_no_verdict_has_failed() {
    let submitted = Submission {
        outcome: SubmittedOutcome::Done,
        message: String::new(),
    };
    let outcome = outcome_of("ELFIE: DONE", Vec::new(), Some(&submitted));
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert_eq!(
        outcome.message,
        "the compiler submitted done but no unit was judged"
    );
    assert!(outcome.verdicts.is_empty());
}

