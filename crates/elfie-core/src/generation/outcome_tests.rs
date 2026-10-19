// @lfy def/generation/outcome.lfy:outcomeOf
//! Tests of `outcomeOf`.

use super::*;

fn accepted_verdict() -> Verdict {
    Verdict {
        accepted: true,
        problems: Vec::new(),
        source_maps: Vec::new(),
        outputs: Vec::new(),
        foreign_edits: Vec::new(),
    }
}

/// One verdict with one problem, for the runs whose outcome a rejection decides.
fn rejected_verdict(problem: &str) -> Verdict {
    Verdict {
        accepted: false,
        problems: vec![problem.to_string(), "and another".to_string()],
        source_maps: Vec::new(),
        outputs: Vec::new(),
        foreign_edits: Vec::new(),
    }
}

// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:aea57156b53552b74d294e42cbfdb2c23586a32eb6b72697003fab1201f8de32
#[test]
fn a_report_ending_in_done_with_an_accepted_verdict_is_accepted() {
    let outcome = outcome_of("I wrote it.\nELFIE: DONE", Some(accepted_verdict()), None);
    // @lfy def/generation/outcome.lfy:outcomeOf
    assert_eq!(outcome.kind, OutcomeKind::Accepted);
    assert_eq!(outcome.message, "");
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:5638d124765a4530529d5bcc010da070ba5c9d64eb393735290bb86942590f82
    assert_eq!(outcome.verdict, Some(accepted_verdict()));
}

// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:1f9450576bacd729304bcff7ae084540d9443231518dff19d18117b589962e74
#[test]
fn a_report_ending_in_clarify_is_a_clarification_with_the_question() {
    let outcome = outcome_of("ELFIE: CLARIFY: should Range.end be inclusive?", None, None);
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:88f72bf6172d2e7ca8cb8cc9cc191d01af0d765a70d6c2b3146a244d430bc889
    assert_eq!(outcome.kind, OutcomeKind::Clarification);
    assert_eq!(outcome.message, "should Range.end be inclusive?");
    assert!(outcome.verdict.is_none());
}

// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:6bf1d1141713e0697e5b6705afad5a3fb71a4abc2de75c4c6bf36482558d79c2
#[test]
fn a_report_ending_in_blocked_is_blocked_with_the_reason_and_every_line_after_it() {
    let outcome = outcome_of(
        "ELFIE: BLOCKED: [[tokenAt]] contradicts [[nodesAt]] on empty files\nand on one-line files",
        None,
        None,
    );
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:b49d5420de2c94cb85a8ea62b3d2f39a0ea55b1b8867b33ca9d7f24d0eb760c2
    assert_eq!(outcome.kind, OutcomeKind::Blocked);
    assert_eq!(
        outcome.message,
        "[[tokenAt]] contradicts [[nodesAt]] on empty files\nand on one-line files"
    );
    // The last such line decides, whatever came before it.
    let outcome = outcome_of("ELFIE: DONE\nELFIE: BLOCKED: no", None, None);
    assert_eq!(outcome.kind, OutcomeKind::Blocked);
    assert_eq!(outcome.message, "no");
}

// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:2b319a66fa16164eba8a81c08bc04af79566ac5e04ad5eff1016c81e7c0fa50e
#[test]
fn a_report_with_nothing_and_no_verdict_has_failed() {
    let outcome = outcome_of("", None, None);
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert!(outcome.message.contains("the compiler reported nothing"));
    assert!(outcome.verdict.is_none());
    // Prose alone, with no line beginning with `ELFIE:`, has failed too.
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:58629f7359bc27358dbb0541b2aca9ed992eaca62911c5cb881d8abeb1869bc8
    assert_eq!(
        outcome_of("I had a look around.", None, None).kind,
        OutcomeKind::Failed
    );
}

/// A report that ended but left no verdict is accepted: the compiler said it was done,
/// and no verdict says otherwise.
// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:5a42af17406855ef2b866b408df911d0f396a686c64bab7834298de466c148e4
#[test]
fn a_report_that_ended_with_no_verdict_is_accepted() {
    let outcome = outcome_of("ELFIE: DONE", None, None);
    assert_eq!(outcome.kind, OutcomeKind::Accepted);
    assert_eq!(outcome.message, "");
    assert!(outcome.verdict.is_none());
}

// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:0c4bafe3144de961cd7e712166d94ab117aa3f9da5f055e814d7cedc962eac68
#[test]
fn a_rejected_verdict_is_rejected_with_its_first_problem() {
    let rejected = rejected_verdict("the entity B has no marker");
    let outcome = outcome_of("ELFIE: DONE", Some(rejected.clone()), None);
    assert_eq!(outcome.kind, OutcomeKind::Rejected);
    assert_eq!(outcome.message, "the entity B has no marker");
    assert_eq!(outcome.verdict, Some(rejected));
}

/// A submitted outcome decides on its own: the report still ends in `ELFIE: DONE`, and
/// the run is blocked with the message the compiler submitted.
// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:9b5e375791254742046cae15d9798eefeda114a69a17d7cf7c04b3a002d6ea6b
#[test]
fn a_submitted_block_wins_over_a_report_that_ended_in_done() {
    let submitted = Submission {
        outcome: SubmittedOutcome::Blocked,
        message: "two criteria of f conflict".to_string(),
    };
    let outcome = outcome_of(
        "I wrote it.\nELFIE: DONE",
        Some(accepted_verdict()),
        Some(&submitted),
    );
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:b45e8bdae8388f9c4a41678f0f3df59a004bd2edf3747f0da5dcf5dfde13c18b
    assert_eq!(outcome.kind, OutcomeKind::Blocked);
    assert_eq!(outcome.message, "two criteria of f conflict");
    assert_eq!(outcome.verdict, Some(accepted_verdict()));

    // A submitted question is a clarification the same way, whatever the report says.
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:8ec2e69b637448582ca10cfc40a3268b7ac51e3c0400efb384bf91954e650bc1
    let asked = Submission {
        outcome: SubmittedOutcome::Clarify,
        message: "should Range.end be inclusive?".to_string(),
    };
    let outcome = outcome_of("ELFIE: DONE", None, Some(&asked));
    assert_eq!(outcome.kind, OutcomeKind::Clarification);
    assert_eq!(outcome.message, "should Range.end be inclusive?");
}

/// A compiler that submitted `done` is still held to the verdict on its unit, and the note
/// it submitted with `done` is no part of the message.
// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:aeee475345e86d94cdd32bf4e6ab45bf3249adc5164e178c28e7a6e801566fb3
#[test]
fn a_submitted_done_with_a_rejected_verdict_is_rejected() {
    let submitted = Submission {
        outcome: SubmittedOutcome::Done,
        message: "I had to move a helper".to_string(),
    };
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:2b934058b8993b21d5206312199acde537634a494ac1d4fc8ee1da93ca975b2f
    let outcome = outcome_of(
        "",
        Some(rejected_verdict("no marker for B")),
        Some(&submitted),
    );
    assert_eq!(outcome.kind, OutcomeKind::Rejected);
    assert_eq!(outcome.message, "no marker for B");
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:0caaac66529a98a351e460ed1abb1858129a0773b2fa6e8f02250c06b9005093
    assert!(!outcome.message.contains("I had to move a helper"));

    // With the verdict accepted, a submitted `done` is accepted and the note is gone.
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:04b6f0c24f4e650e22038c1e80bd287adf96f0ea7bcde76e829f3d07b0be1574
    let outcome = outcome_of("", Some(accepted_verdict()), Some(&submitted));
    assert_eq!(outcome.kind, OutcomeKind::Accepted);
    assert_eq!(outcome.message, "");
}

/// A compiler that submitted `done` while its unit was not judged has failed: its outputs
/// never reached acceptance, whatever its report said.
// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:b98c498284033051b8b80db1847c7a733beeea9db786c21553593697deb79933
#[test]
fn a_submitted_done_with_no_verdict_has_failed() {
    let submitted = Submission {
        outcome: SubmittedOutcome::Done,
        message: String::new(),
    };
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:f8ffca525ed8ecad98217b596be4755172abc9a94a4b78445e03ffdcffee7689
    let outcome = outcome_of("ELFIE: DONE", None, Some(&submitted));
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert_eq!(
        outcome.message,
        "the compiler submitted done but the unit was not judged"
    );
    assert!(outcome.verdict.is_none());
}

// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:baaeaa638ceaf8aaf56e2a3d9f7c8d6b7b07dcab5412cb5545dac7efb3a30f4b
#[test]
fn a_report_with_no_elfie_line_has_failed_even_with_an_accepted_verdict() {
    let outcome = outcome_of(
        "Failed to authenticate: OAuth session expired",
        Some(accepted_verdict()),
        None,
    );
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert!(outcome.message.contains("the compiler reported nothing"));
    assert_eq!(outcome.verdict, Some(accepted_verdict()));
}

// @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:c060457377a1c2e51d798422346b2256901a4ab45a0984717a6d9d4401d3281d
#[test]
fn an_empty_report_has_failed_even_with_a_rejected_verdict() {
    let rejected = rejected_verdict("no marker for B");
    let outcome = outcome_of("", Some(rejected.clone()), None);
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert!(outcome.message.contains("the compiler reported nothing"));
    assert_eq!(outcome.verdict, Some(rejected));
}
