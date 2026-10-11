// @lfy def/generation/main.lfy:outcomeOf
//! How one run of the compiler ended.

use super::*;

/// How one run of the compiler ended, read from its submitted outcome or its report, and the
/// verdicts of its units.
///
/// `submitted` is the last outcome the compiler submitted through `elfie_submit` during the
/// run, and `None` when it submitted none. A submitted outcome decides on its own, and no
/// line of the report is read: `blocked` and `clarify` give blocked and clarification with
/// the submission's message, and `done` gives rejected with the first problem of the first
/// rejected verdict, failed when no unit was judged at all, and accepted otherwise — the
/// note a `done` submission may carry is no part of the message.
///
/// With nothing submitted, the last line of the report beginning with `ELFIE:` decides
/// first: `ELFIE: BLOCKED:` gives blocked and `ELFIE: CLARIFY:` gives clarification, each
/// with the rest of that line and every line after it as the message. With no such line and
/// no verdicts the run failed; with a rejected verdict it was rejected, with the first
/// problem of the first rejected verdict as the message; otherwise it was accepted, with an
/// empty message — a report that ended and left no verdict is accepted, since the compiler
/// said it was done.
// Decision: a submitted outcome was checked when it arrived, so it wins over the report's
// `ELFIE:` line; the report is read only when nothing was submitted.
// @lfy def/generation/main.lfy:outcomeOf
pub fn outcome_of(report: &str, verdicts: Vec<Verdict>, submitted: Option<&Submission>) -> Outcome {
    const BLOCKED: &str = "ELFIE: BLOCKED:";
    const CLARIFY: &str = "ELFIE: CLARIFY:";

    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:b45e8bdae8388f9c4a41678f0f3df59a004bd2edf3747f0da5dcf5dfde13c18b
    if let Some(submission) = submitted {
        return match submission.outcome {
            // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:873c80cfcfc9e8dcfbd60313bc0c671888373485ae58b84fd6d2b1203abec749
            SubmittedOutcome::Blocked => Outcome {
                kind: OutcomeKind::Blocked,
                message: submission.message.clone(),
                verdicts,
            },
            // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:8ec2e69b637448582ca10cfc40a3268b7ac51e3c0400efb384bf91954e650bc1
            SubmittedOutcome::Clarify => Outcome {
                kind: OutcomeKind::Clarification,
                message: submission.message.clone(),
                verdicts,
            },
            // A compiler that says it is done is still held to the verdicts of its units,
            // and the note it may have submitted says nothing about the outcome.
            // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:0caaac66529a98a351e460ed1abb1858129a0773b2fa6e8f02250c06b9005093
            SubmittedOutcome::Done => {
                // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:ae2660461f4dd83b7805f1cd5571a95959adc789f1d82cb5dd47b6d1941cebbe
                if let Some(rejected) = verdicts.iter().find(|verdict| !verdict.accepted) {
                    let message = rejected.problems.first().cloned().unwrap_or_default();
                    return Outcome {
                        kind: OutcomeKind::Rejected,
                        message,
                        verdicts,
                    };
                }
                // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:798eb562651952ea80b4b05980552450a02b660a5f0a7a76f38e2e05b130ac04
                if verdicts.is_empty() {
                    return Outcome {
                        kind: OutcomeKind::Failed,
                        message: "the compiler submitted done but no unit was judged".to_string(),
                        verdicts,
                    };
                }
                // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:6cbc44baf902ab337f838bc362fc27fd6caace8df9741d15d5c4151081cc920b
                Outcome {
                    kind: OutcomeKind::Accepted,
                    message: String::new(),
                    verdicts,
                }
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
            // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:b49d5420de2c94cb85a8ea62b3d2f39a0ea55b1b8867b33ca9d7f24d0eb760c2
            Some((BLOCKED, OutcomeKind::Blocked))
        } else if line.starts_with(CLARIFY) {
            // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:88f72bf6172d2e7ca8cb8cc9cc191d01af0d765a70d6c2b3146a244d430bc889
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
            return Outcome {
                kind,
                message: message.trim_end().to_string(),
                verdicts,
            }; // @lfy def/generation/main.lfy:outcomeOf
        }
    }
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:21fbd937a6b58a02072c2e606c0188be26f64751f8a10f7d260dea1d88e01a21
    if verdicts.is_empty() && last.is_none() {
        return Outcome {
            kind: OutcomeKind::Failed,
            message: "the compiler reported nothing".to_string(),
            verdicts,
        };
    }
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:0e7476527a35bfd8b4cf324745dc9e2aa10aa07b1fb7a1d6da3f32055f0154ac
    if let Some(rejected) = verdicts.iter().find(|verdict| !verdict.accepted) {
        let message = rejected.problems.first().cloned().unwrap_or_default();
        return Outcome {
            kind: OutcomeKind::Rejected,
            message,
            verdicts,
        };
    }
    // @lfy def/generation/main.lfy:outcomeOf#outcomeOf:outcomeOf:9b1f1e3d496ed3a527d2e2d93703ab7d0488cd057539e9129a315be84523bcca
    Outcome {
        kind: OutcomeKind::Accepted,
        message: String::new(),
        verdicts,
    }
}
