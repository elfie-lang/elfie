// @lfy def/generation/outcome.lfy:outcomeOf
//! How one run of the compiler ended.

use super::*;

/// How one run of the compiler ended, read from its submitted outcome or its report, and the
/// verdict on its unit.
///
/// `report` is everything the compiler wrote to standard output. `verdict` is the verdict of
/// [`accept`] on the unit's outputs after the run, and `None` when the compiler never got that
/// far. `submitted` is the last outcome the compiler submitted through `elfie_submit` during
/// the run, and `None` when it submitted none.
// Decision: a submitted outcome was checked when it arrived, so it wins over the report's
// `ELFIE:` line; the report is read only when nothing was submitted.
// @lfy def/generation/outcome.lfy:outcomeOf
pub fn outcome_of(report: &str, verdict: Option<Verdict>, submitted: Option<&Submission>) -> Outcome {
    const BLOCKED: &str = "ELFIE: BLOCKED:";
    const CLARIFY: &str = "ELFIE: CLARIFY:";

    // The verdict is handed back whatever the outcome.
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:5638d124765a4530529d5bcc010da070ba5c9d64eb393735290bb86942590f82
    let outcome = |kind: OutcomeKind, message: String, verdict: Option<Verdict>| Outcome {
        kind,
        message,
        verdict,
    };
    let rejection = |verdict: &Option<Verdict>| -> Option<String> {
        verdict
            .as_ref()
            .filter(|verdict| !verdict.accepted)
            .map(|verdict| verdict.problems.first().cloned().unwrap_or_default())
    };

    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:b45e8bdae8388f9c4a41678f0f3df59a004bd2edf3747f0da5dcf5dfde13c18b
    if let Some(submission) = submitted {
        return match submission.outcome {
            // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:873c80cfcfc9e8dcfbd60313bc0c671888373485ae58b84fd6d2b1203abec749
            SubmittedOutcome::Blocked => {
                outcome(OutcomeKind::Blocked, submission.message.clone(), verdict)
            }
            // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:8ec2e69b637448582ca10cfc40a3268b7ac51e3c0400efb384bf91954e650bc1
            SubmittedOutcome::Clarify => {
                outcome(OutcomeKind::Clarification, submission.message.clone(), verdict)
            }
            // A compiler that says it is done is still held to the verdict on its unit, and
            // the note it may have submitted says nothing about the outcome.
            // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:0caaac66529a98a351e460ed1abb1858129a0773b2fa6e8f02250c06b9005093
            SubmittedOutcome::Done => {
                // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:2b934058b8993b21d5206312199acde537634a494ac1d4fc8ee1da93ca975b2f
                if let Some(message) = rejection(&verdict) {
                    return outcome(OutcomeKind::Rejected, message, verdict);
                }
                // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:f8ffca525ed8ecad98217b596be4755172abc9a94a4b78445e03ffdcffee7689
                if verdict.is_none() {
                    return outcome(
                        OutcomeKind::Failed,
                        "the compiler submitted done but the unit was not judged".to_string(),
                        verdict,
                    );
                }
                // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:04b6f0c24f4e650e22038c1e80bd287adf96f0ea7bcde76e829f3d07b0be1574
                outcome(OutcomeKind::Accepted, String::new(), verdict)
            }
        };
    }

    let lines: Vec<&str> = report.lines().collect();
    let last = lines
        .iter()
        .rposition(|line| line.trim_start().starts_with("ELFIE:"));
    if let Some(index) = last {
        let line = lines[index].trim_start();
        let prefix = if line.starts_with(BLOCKED) {
            // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:b49d5420de2c94cb85a8ea62b3d2f39a0ea55b1b8867b33ca9d7f24d0eb760c2
            Some((BLOCKED, OutcomeKind::Blocked))
        } else if line.starts_with(CLARIFY) {
            // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:88f72bf6172d2e7ca8cb8cc9cc191d01af0d765a70d6c2b3146a244d430bc889
            Some((CLARIFY, OutcomeKind::Clarification))
        } else {
            None
        };
        if let Some((prefix, kind)) = prefix {
            let mut message = line[prefix.len()..].trim_start().to_string();
            for line in &lines[index + 1..] {
                message.push('\n');
                message.push_str(line);
            }
            return outcome(kind, message.trim_end().to_string(), verdict); // @lfy def/generation/outcome.lfy:outcomeOf
        }
    }
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:58629f7359bc27358dbb0541b2aca9ed992eaca62911c5cb881d8abeb1869bc8
    // A verdict on outputs the compiler never reported on says nothing about the run.
    if last.is_none() {
        return outcome(
            OutcomeKind::Failed,
            "the compiler reported nothing, so it may never have run".to_string(),
            verdict,
        );
    }
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:0c4bafe3144de961cd7e712166d94ab117aa3f9da5f055e814d7cedc962eac68
    if let Some(message) = rejection(&verdict) {
        return outcome(OutcomeKind::Rejected, message, verdict);
    }
    // @lfy def/generation/outcome.lfy:outcomeOf#outcomeOf:outcomeOf:5a42af17406855ef2b866b408df911d0f396a686c64bab7834298de466c148e4
    outcome(OutcomeKind::Accepted, String::new(), verdict)
}
