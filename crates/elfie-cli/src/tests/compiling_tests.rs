// @lfy def/cli/main.lfy:main
//! Tests of one compile from end to end: the program it refuses, the requests it writes, the
//! progress lines it prints, the outcomes it reads, the reviews it asks for, and the global
//! review that follows every batch.

use super::*;
use crate::compiling::loaded_diagnostics;
use crate::copies::Where;
use crate::processes::Runs;
use crate::reviewing::{error_diagnostics, missing_lines, not_reviewed, painted_review, problem_of};
use crate::schedule::{Round, Schedule};

/// Compile prints the errors and returns problems rather than handing the compiler a
/// program with problems.
// @lfy def/cli/main.lfy:main
#[test]
fn compile_refuses_a_program_with_problems() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "const y = z;\n");
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    assert!(!fixture.root.join("out").exists());
}

/// Compile prints the diagnostics of the loaded program, a warning among them; verify
/// prints the error ones alone. The unused use of def/b.lfy is the warning and the
/// undeclared z the error, both in the one file.
// @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
// @lfy def/cli/main.lfy:main#main:main:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
#[test]
fn compile_prints_every_loaded_diagnostic_and_verify_only_the_errors() {
    let fixture = Fixture::new();
    fixture.write("def/b.lfy", "d B: `A B` {}\n").write("def/a.lfy", "use \"./b\";\n\nconst y = z;\n");
    let workspace = workspace::load(&fixture.root);
    let mine: Vec<Diagnostic> = loaded_diagnostics(&workspace).into_iter().filter(|d| d.range.file == "def/a.lfy").collect();
    assert!(mine.iter().any(|d| d.severity == Severity::Error), "{mine:?}");
    assert!(mine.iter().any(|d| d.severity == Severity::Warning), "{mine:?}");
    let errors: Vec<Diagnostic> = error_diagnostics(&workspace).into_iter().filter(|d| d.range.file == "def/a.lfy").collect();
    assert!(errors.iter().all(|d| d.severity == Severity::Error), "{errors:?}");
    assert!(errors.len() < mine.len(), "{errors:?} against {mine:?}");
    // Nothing is printed when loading found no error, whatever warnings it found.
    // @lfy def/cli/main.lfy:main#main:main:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
    let clean = Fixture::new();
    clean.write("def/b.lfy", "d B: `A B` {}\n").write("def/a.lfy", "use \"./b\";\n\nconst y = 1;\n");
    let loaded = workspace::load(&clean.root);
    assert!(loaded_diagnostics(&loaded).is_empty());
    assert!(query::diagnostics_of(&loaded, None).iter().any(|d| d.severity == Severity::Warning));
}

/// With no compiler named, the request of each batch is written to elfie-requests under
/// the root, one file per batch named by its identifier, and the code is success. It is
/// read by a compiler run by hand, in no hosted session, so it names none of the tools a
/// session offers.
// @lfy def/cli/main.lfy:main
// @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
#[test]
fn a_project_with_no_compiler_writes_the_requests() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let instructions = fixture.read("elfie-requests/a.md");
    assert!(instructions.contains("An A"), "{instructions}");
    assert!(instructions.contains("def/a.lfy"), "{instructions}");
    // The request was made for no hosted session: it names neither the tools only a
    // hosted session offers nor the way one submits its outcome.
    // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
    assert!(!instructions.contains("elfie_task"), "{instructions}");
    // @lfy def/cli/main.lfy:main#main:main:de437a586cddefeefabed78d855e65916788f25c33c6c0d9b7286be02d199ce5
    assert!(!instructions.contains("elfie_submit"), "{instructions}");
    // Every progress line is appended to the log, as printed with colors off.
    // @lfy def/cli/main.lfy:main
    let log = fixture.read("elfie-requests/compile.log");
    assert!(log.contains("[0/1]"), "{log}");
    assert!(log.contains("◆ planned"), "{log}");
    assert!(log.contains("· requesting"), "{log}");
    assert!(log.contains("◆ finished"), "{log}");
    // @lfy def/cli/main.lfy:main
    assert!(!log.contains('\u{1b}'), "the log holds an escape");
    // Nothing of the CLI's own is left under the output directory.
    // @lfy def/cli/main.lfy:main
    assert!(!fixture.root.join("out/elfie-requests").exists());
    // Neither the compiler nor the verifier ran, so no agent server was hosted and no
    // session's config file was written.
    // @lfy def/cli/main.lfy:main#main:main:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
    assert!(!fixture.root.join("elfie-requests/a.compiler.mcp.json").exists());
    // @lfy def/cli/main.lfy:main#main:main:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
    assert!(!fixture.root.join("elfie-requests/a.verifier.mcp.json").exists());
}

/// Each progress line reads, in columns separated by two spaces: the counter, the time,
/// the step with its glyph, then the batch and unit when there are any, and the first line
/// of the message. The first line of a compile is planned with the counts, and the last is
/// finished with the count accepted, rejected, blocked, and failed.
// @lfy def/cli/main.lfy:main
#[test]
fn a_progress_line_carries_the_step_the_counts_and_the_elapsed_seconds() {
    let progress = Progress {
        step: Step::Accepted,
        batch: Some("a+1".to_string()),
        unit: Some("cli/main".to_string()),
        done: 2,
        total: 7,
        elapsed: 12.34,
        message: "3 outputs\nand more".to_string(),
    };
    // The counter, the time right aligned to six columns, the step padded to the longest
    // name of Step with its glyph, the batch, the unit, and the first line of the message.
    // @lfy def/cli/main.lfy:main
    let line = text_of(&progress, None, None, false);
    assert_eq!(
        line,
        format!("[2/7]   12.3s  {}  a+1  cli/main  3 outputs", padded("✓ accepted", step_column()))
    );
    // Every column is separated by two spaces, in order. @lfy def/cli/main.lfy:main
    let columns: Vec<&str> =
        line.split("  ").map(str::trim).filter(|column| !column.is_empty()).collect();
    assert_eq!(columns, vec!["[2/7]", "12.3s", "✓ accepted", "a+1", "cli/main", "3 outputs"], "{line:?}");
    let value = json_of(&progress);
    assert_eq!(value["step"], "accepted");
    assert_eq!(value["batch"], "a+1");
    assert_eq!(value["unit"], "cli/main");
    assert_eq!(value["done"], 2);
    assert_eq!(value["total"], 7);
    assert_eq!(value["message"], "3 outputs\nand more");
    // Painting leaves every column the width it had. @lfy def/cli/main.lfy:main
    assert_eq!(width(&text_of(&progress, None, None, true)), width(&line));
    // The counter keeps one width for the whole compile: done is right aligned to the
    // width of total. @lfy def/cli/main.lfy:main
    let wide = Progress { done: 9, total: 100, ..progress.clone() };
    assert!(text_of(&wide, None, None, false).starts_with("[  9/100]"));
    // The unit is not repeated after the batch. @lfy def/cli/main.lfy:main
    let same = Progress { unit: Some("a+1".to_string()), ..progress.clone() };
    assert_eq!(text_of(&same, None, None, false).matches("a+1").count(), 1);
    // @lfy def/cli/main.lfy:main
    let planned = Progress {
        step: Step::Planned,
        batch: None,
        unit: None,
        done: 0,
        total: 7,
        elapsed: 0.0,
        message: "7 units planned, 3 batches".to_string(),
    };
    assert_eq!(
        text_of(&planned, None, None, false),
        format!("[0/7]    0.0s  {}  7 units planned, 3 batches", padded("◆ planned", step_column()))
    );
    assert_eq!(json_of(&planned)["batch"], serde_json::Value::Null);
    // A reviewed line's message is the counts, each a tally; painted or not it holds the
    // same words. @lfy def/cli/main.lfy:main
    let counts: Counts = vec![
        (1, "satisfied", Tone::Success),
        (0, "violated", Tone::Failure),
        (0, "unverifiable", Tone::Warning),
    ];
    assert_eq!(counts_message(&counts, false), "1 satisfied, 0 violated, 0 unverifiable");
    assert_eq!(width(&counts_message(&counts, true)), width(&counts_message(&counts, false)));
    // A rejection's message is painted in the step's tone, and a step that is none of
    // rejected, failed, blocked, or clarification is plain. @lfy def/cli/main.lfy:main
    let rejected = Progress { step: Step::Rejected, ..progress.clone() };
    assert!(text_of(&rejected, None, None, true).ends_with(&paint("3 outputs", Tone::Failure, true)));
    assert!(text_of(&progress, None, None, true).ends_with("3 outputs"));
}

/// A batch identifier holds the stem's slashes, which a file name may not.
// @lfy def/cli/main.lfy:main
#[test]
fn a_batch_names_its_note_files() {
    assert_eq!(file_name_of("cli/main+1"), "cli-main+1");
    assert_eq!(file_name_of("a"), "a");
}

/// The answer a person wrote below the question is read back; a question with none
/// gives nothing.
// @lfy def/cli/main.lfy:main
#[test]
fn the_answer_below_a_question_is_read_back() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
    let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
    let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
    let run = Run::new(program, plan, Vec::new(), reporter, &invocation);
    let batch = run.plan.batches[0].clone();
    let unanswered = format!("# Asked\n\nWhich one?\n\n{ANSWER_HEADING}\n\n<!-- Write the answer below this line. -->\n");
    fixture.write("elfie-requests/a.question.md", &unanswered);
    assert_eq!(run.answer_of(&batch), None);
    fixture.write("elfie-requests/a.question.md", &format!("{unanswered}\nThe second one.\n"));
    assert_eq!(run.answer_of(&batch).as_deref(), Some("The second one."));
}

/// A stopped batch stops only the batches that depend on one of its units, and the
/// finished line lists every batch that did not complete.
// @lfy def/cli/main.lfy:main
#[test]
fn a_stopped_batch_stops_what_depends_on_it() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/a.lfy", "use \"./b\";\nd A: `An A` { $b = B; }\n")
        .write("def/b.lfy", "d B {}\n")
        .write("def/c.lfy", "d C {}\n");
    let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
    assert!(program.workspace.problems.is_empty(), "{:?}", program.workspace.problems);
    let invocation = parse_arguments(&args(&["compile", "--continue", "--color=never"])).unwrap();
    let reporter = Reporter::new(false, Paint::of(&invocation), plan.units.len(), Vec::new());
    let mut run = Run::new(program, plan, Vec::new(), reporter, &invocation);
    assert!(run.continuing);
    // The batch holding b is stopped; a depends on b, c does not.
    let of = |run: &Run, stem: &str| run.plan.units.iter().position(|unit| unit.stem == stem).unwrap();
    let (a, b, c) = (of(&run, "a"), of(&run, "b"), of(&run, "c"));
    run.stop(&Batch { units: vec![b], identifier: "b".to_string() });
    assert!(!run.halted, "--continue keeps the independent batches running");
    assert!(run.depends_on_stopped(&Batch { units: vec![a], identifier: "a".to_string() }));
    assert!(!run.depends_on_stopped(&Batch { units: vec![c], identifier: "c".to_string() }));
    // Without --continue a stopped batch stops the compile.
    let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
    let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
    let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
    let mut run = Run::new(program, plan, Vec::new(), reporter, &invocation);
    run.stop(&Batch { units: vec![b], identifier: "b".to_string() });
    assert!(run.halted);
    assert_eq!(run.incomplete, vec!["b".to_string()]);
}

/// The code is success when every batch was accepted, problems when any was rejected,
/// blocked, or asked a question, and failure when a command could not run.
// @lfy def/cli/main.lfy:main
#[test]
fn the_code_is_the_worst_thing_the_compile_met() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
    let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
    let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
    let mut run = Run::new(program, plan, Vec::new(), reporter, &invocation);
    assert_eq!(run.code, ExitCode::Success);
    run.worsen(ExitCode::Problems);
    assert_eq!(run.code, ExitCode::Problems);
    run.worsen(ExitCode::Success);
    assert_eq!(run.code, ExitCode::Problems, "a milder code does not win");
    run.worsen(ExitCode::Failure);
    assert_eq!(run.code, ExitCode::Failure);
}

/// With --accept no compiler is run; outputs that carry no marker for the unit are
/// rejected and the code is problems.
// @lfy def/cli/main.lfy:main
#[test]
fn accept_checks_what_is_already_on_disk() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    // Nothing on disk: the unit is rejected. @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Problems.code());
    // An output carrying a marker for the unit is accepted and recorded.
    // @lfy def/cli/main.lfy:main
    fixture.write("out/a.rs", "// @lfy def/a.lfy:1\npub struct A {}\n");
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    // The source maps of its outputs are recorded in the unit's own map file.
    // @lfy def/cli/main.lfy:main
    let maps = fixture.read("elfie-compile/maps/rust/a.json");
    assert!(maps.contains("\"out/a.rs\""), "{maps}");
    assert!(maps.contains("\"def/a.lfy\""), "{maps}");
    // The marker is written back naming the entity rather than the line.
    assert_eq!(fixture.read("out/a.rs"), "// @lfy def/a.lfy:A\npub struct A {}\n");
    // And the unit is then up to date. @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert!(!fixture.root.join("elfie-requests/a.md").exists(), "nothing is requested");
}

/// The compiler is run once per batch with the request on its standard input and
/// ELFIE_ROOT, ELFIE_BATCH, and ELFIE_UNITS in its environment; its standard output is
/// kept whole as the report.
// @lfy def/cli/main.lfy:main
#[test]
fn the_compiler_is_run_per_batch_and_its_report_is_read() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/a.lfy", "d A: `An A` {}\n")
        .write(
            "compiler.sh",
            "#!/bin/sh\ncat > \"$ELFIE_ROOT/instructions.txt\"\nprintf '%s\\n' \"$ELFIE_BATCH\" \"$ELFIE_UNITS\" \"$ELFIE\" \"[$ELFIE_COMMAND_TOOLS]\" > \"$ELFIE_ROOT/environment.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:1\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho done\n",
        )
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    // @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // ELFIE is the path of the running executable, so the agent server an agent starts
    // is this same program.
    // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    let environment = fixture.read("environment.txt");
    let mut lines = environment.lines();
    assert_eq!(lines.next(), Some("a"));
    assert_eq!(lines.next(), Some("a"));
    let elfie = std::env::current_exe().unwrap();
    assert_eq!(lines.next(), Some(elfie.to_string_lossy().as_ref()));
    // The target of this project has no command, so its session is offered no command
    // tool and ELFIE_COMMAND_TOOLS is empty.
    // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    assert_eq!(lines.next(), Some("[]"));
    let instructions = fixture.read("instructions.txt");
    assert!(instructions.contains("An A"), "{instructions}");
    // The request was made for the run's hosted session, so it names the tools only such
    // a session offers and how to submit the outcome through it.
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    assert!(instructions.contains("elfie_task"), "{instructions}");
    // @lfy def/cli/main.lfy:main#main:main:c33603aa1a0daddcd764752355373ba6e9d0a7e056273935466dbfa27de55685
    assert!(instructions.contains("elfie_submit"), "{instructions}");
    // @lfy def/cli/main.lfy:main
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    // The report is appended to the log with every progress line.
    // @lfy def/cli/main.lfy:main
    let log = fixture.read("elfie-requests/compile.log");
    assert!(log.contains("done"), "{log}");
    assert!(log.contains("▸ compiling"), "{log}");
    // The unit is not repeated after the batch when it is the batch.
    // @lfy def/cli/main.lfy:main
    assert!(log.contains("[1/1]"), "{log}");
    assert!(log.lines().any(|line| line.contains("✓ accepted") && line.matches(" a ").count() == 1), "{log}");
    assert!(log.contains("1 accepted, 0 rejected, 0 blocked, 0 failed"), "{log}");
}

/// A compiler that produces nothing is rejected, run once more, and then stops the
/// compile; a blocked one writes its reason.
// @lfy def/cli/main.lfy:main
#[test]
fn a_rejected_batch_is_run_once_more_and_a_blocked_one_writes_its_reason() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/a.lfy", "d A: `An A` {}\n")
        .write("compiler.sh", "#!/bin/sh\ncat > /dev/null\necho 'attempt' >> \"$ELFIE_ROOT/attempts.txt\"\n")
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    assert_eq!(fixture.read("attempts.txt").lines().count(), 2, "the batch is run once more");
    // @lfy def/cli/main.lfy:main
    fixture.write("compiler.sh", "#!/bin/sh\ncat > /dev/null\necho 'ELFIE: BLOCKED: def/a.lfy says nothing'\n");
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    let blocked = fixture.read("elfie-requests/a.blocked.md");
    assert!(blocked.contains("def/a.lfy says nothing"), "{blocked}");
    // @lfy def/cli/main.lfy:main
    fixture.write("compiler.sh", "#!/bin/sh\ncat > /dev/null\necho 'ELFIE: CLARIFY: which A?'\n");
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    let question = fixture.read("elfie-requests/a.question.md");
    assert!(question.contains("which A?") && question.contains(ANSWER_HEADING), "{question}");
    // The answer written below the question is appended to the next request.
    fixture.write("elfie-requests/a.question.md", &format!("{question}\nThe only one.\n"));
    fixture.write("compiler.sh", "#!/bin/sh\ncat > \"$ELFIE_ROOT/instructions.txt\"\n");
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    assert!(fixture.read("instructions.txt").contains("The only one."));
}

/// A command that cannot be run is a failure, after being run once more.
// @lfy def/cli/main.lfy:main
#[test]
fn a_command_that_cannot_run_is_a_failure() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/a.lfy", "d A: `An A` {}\n")
        .write("elfie.json", &manifest(&[("compiler", "exit 127")]));
    // The command runs and produces nothing, so the outcome is rejected rather than
    // failed; a command that cannot be spawned at all is the failure.
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
    let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
    let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
    let run = Mutex::new(Run::new(program, plan, Vec::new(), reporter, &invocation));
    let batch = locked(&run).plan.batches[0].clone();
    let nowhere = Where::Root(PathBuf::from("/no/such/directory"));
    let runs = {
        let mut state = locked(&run);
        let agent = state.agent.clone();
        Runs::new(agent, state.running_of(&batch.identifier))
    };
    let planned = vec![a_planned_batch(&batch.identifier, 0, Vec::new())];
    let schedule = Mutex::new(Schedule::of(&planned));
    let finished = Condvar::new();
    let round = Round { schedule: &schedule, finished: &finished, planned: &planned, at: 0, copies: false };
    assert!(Run::compile_batch(&run, &batch, "true", &nowhere, &runs, None, &round).held.is_none());
    let state = locked(&run);
    assert_eq!(state.code, ExitCode::Failure);
    assert_eq!(state.failed, 1);
    assert!(state.halted);
}

/// A review is printed as its status, a space, the file, a colon, the line, a space, the
/// entity, a colon, a space, and the note; a violated one becomes one problem reading
/// failure at, the file, a colon, the line, a colon, the note, and the evidence in
/// parentheses.
// @lfy def/cli/main.lfy:main
#[test]
fn a_review_is_printed_and_a_violated_one_becomes_a_problem() {
    let review = Review {
        id: "A:A:0".to_string(),
        file: "def/a.lfy".to_string(),
        line: 3,
        entity: "A".to_string(),
        status: ReviewStatus::Satisfied,
        evidence: "out/a.rs:1-2".to_string(),
        note: "covered by a test".to_string(),
    };
    // The status is padded to the longest status of ReviewStatus, so every entity starts
    // in the same column. @lfy def/cli/main.lfy:main
    let column = width(&ReviewStatus::Unverifiable.to_string());
    assert_eq!(
        painted_review(&review, false),
        format!("{} def/a.lfy:3 A: covered by a test", padded("satisfied", column))
    );
    // The status is painted in its own tone and the file and line are subject, which
    // leaves the width as it was. @lfy def/cli/main.lfy:main
    assert_eq!(width(&painted_review(&review, true)), width(&painted_review(&review, false)));
    assert_eq!(status_tone(review.status), Tone::Success);
    let violated = Review {
        status: ReviewStatus::Violated,
        evidence: "crates/a/src/a.rs:4-6".to_string(),
        note: "returns 0 for an empty list".to_string(),
        ..review.clone()
    };
    // @lfy def/cli/main.lfy:main
    assert_eq!(problem_of(&violated), "failure at def/a.lfy:3: returns 0 for an empty list (crates/a/src/a.rs:4-6)");
    // An unverifiable review is counted and never rejects. @lfy def/cli/main.lfy:main
    let unverifiable = Review { status: ReviewStatus::Unverifiable, ..review.clone() };
    let every = [review, violated, unverifiable];
    let counts = counts_of(&every);
    assert_eq!(counts, (1, 1, 1));
    // @lfy def/cli/main.lfy:main
    assert_eq!(counts_message(&counts_with_missing(&every, 0), false), "1 satisfied, 1 violated, 1 unverifiable");
    // An id the review was asked about that no review names counts as unverifiable.
    // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
    assert_eq!(counts_message(&counts_with_missing(&every, 2), false), "1 satisfied, 1 violated, 3 unverifiable");
    // And is named as not reviewed on a line of its own, which is the line a compile
    // prints under its reviewed line and a verify under its counts line.
    // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
    let unanswered = ["A:A:one".to_string(), "A:A:two".to_string()];
    assert_eq!(not_reviewed(&unanswered[0]), "A:A:one was not reviewed");
    let report = ReviewReport { missing: unanswered.to_vec(), ..ReviewReport::default() };
    assert_eq!(missing_lines(&report), "A:A:one was not reviewed\nA:A:two was not reviewed");
    assert_eq!(
        missing_lines(&report).lines().collect::<Vec<_>>(),
        unanswered.iter().map(|id| not_reviewed(id)).collect::<Vec<_>>()
    );
    // Nothing follows when the review answered for every id it was asked about.
    // @lfy def/cli/main.lfy:main#main:main:10291079d3776f97261b8bb95179bc83292d62986fd02c692185455aed885b5f
    assert!(missing_lines(&ReviewReport::default()).is_empty());
}

/// A violated review rejects the batch: the problem is printed, the batch is run once
/// more with it appended to the instructions, the verifier runs again, and a violated
/// review then stops the batch as rejected.
// @lfy def/cli/main.lfy:main#main:main:a614f50ad52233df09b271e8629dcbe550c75c8f9eb349f91208cb874d9b7f6d
#[test]
fn a_violated_review_sends_the_batch_back_and_then_rejects_it() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, VIOLATED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    // The compiler and the verifier each ran twice. @lfy def/cli/main.lfy:main
    assert_eq!(lines_of(&fixture, "attempts.txt"), 2);
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
    let log = fixture.read("elfie-requests/compile.log");
    // A reviewed line counting 0 satisfied, 1 violated, 0 unverifiable.
    // @lfy def/cli/main.lfy:main
    assert!(log.contains("✓ reviewed"), "{log}");
    assert!(log.contains("0 satisfied, 1 violated, 0 unverifiable"), "{log}");
    // The problem follows the line it belongs to, indented four spaces and beginning with
    // a dash and a space, then a retrying line. @lfy def/cli/main.lfy:main
    assert!(
        log.contains("    - failure at def/a.lfy:3: returns 0 for an empty list (crates/a/src/a.rs:4-6)"),
        "{log}"
    );
    assert!(log.contains("↻ retrying"), "{log}");
    // The log never holds an escape. @lfy def/cli/main.lfy:main
    assert!(!log.contains('\u{1b}'), "the log holds an escape");
    // The compiler was run a second time with that problem appended to its
    // instructions. @lfy def/cli/main.lfy:main
    assert!(fixture.read("instructions.txt").contains("failure at def/a.lfy:3"));
    // The reviews are written, and the batch counts as a rejection.
    // @lfy def/cli/main.lfy:main
    assert!(fixture.read("elfie-requests/a.reviews.json").contains("violated"));
    assert!(log.contains("0 accepted, 1 rejected"), "{log}");
    // Its source maps are not recorded, so its outputs stay on disk and the unit stays
    // planned. @lfy def/cli/main.lfy:main
    assert!(fixture.root.join("out/a.rs").exists(), "the outputs stay on disk");
    assert!(!fixture.root.join("elfie-compile/maps/rust/a.json").exists(), "nothing is recorded");
    let (_, plan) = plan_with(&fixture.root, &[], &[], &[]);
    assert_eq!(plan.units.len(), 1);
    assert!(plan.units[0].reason.is_some(), "the unit stays planned");
}

/// A verifier that answered for nothing it was asked about is run once more; its problems
/// are then printed as a failed line and the batch is verified with the reviews parsed
/// from that run as if its report had been complete.
///
/// A run that answered for every id it was asked about stands as it is, however it ended:
/// the verifier is not run again for a report that merely did not end.
// @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
// @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
// @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
#[test]
fn a_verifier_whose_report_does_not_end_is_run_once_more() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, NOTHING_REVIEWED);
    // Nothing it was asked about was answered: the run is made again, and the second one
    // answers for nothing either.
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2, "the verifier ran twice");
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran once");
    let log = fixture.read("elfie-requests/compile.log");
    // Its problems are printed as a failed progress line, and the batch is verified with
    // the reviews parsed from that run as if its report had been complete: nothing was
    // violated, so the batch is done and the id it left is counted as unverifiable.
    // @lfy def/cli/main.lfy:main#main:main:f29260af3c81a118f396bd28883ea1ce62addd257c0aced3c261874602ee375a
    assert!(log.contains("✗ failed"), "{log}");
    // @lfy def/cli/main.lfy:main#main:main:3a7d4cf5d94994610376ab45fd0b8926c887075df81b180e7bd9e714d2219c33
    assert!(log.contains("0 satisfied, 0 violated, 1 unverifiable"), "{log}");
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    // A report that answered for every id it was asked about stands as it is, whatever
    // else it did: the verifier is run once, and what it wrote beside its reviews is
    // printed under its reviewed line.
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    let stood = Fixture::new();
    a_compiler_and_a_verifier(&stood, UNENDED);
    assert_eq!(stood.run(&["compile"]), ExitCode::Success.code());
    // @lfy def/cli/main.lfy:main#main:main:4c0e08888be9dc3b6997e5ea919ad1c9206cd3046b518d1ec20dbd018f279b76
    assert_eq!(lines_of(&stood, "verifications.txt"), 1, "the verifier ran once");
    let log = stood.read("elfie-requests/compile.log");
    assert!(!log.contains("✗ failed"), "{log}");
    assert!(log.contains("1 satisfied, 0 violated, 0 unverifiable"), "{log}");
    // What it wrote beside its reviews follows its reviewed line.
    // @lfy def/cli/main.lfy:main#main:main:7ac5c7dbdba835816dd103d3fb573c0697886fc87ae20c77a226aaa3f7b90592
    assert!(log.contains("the report did not end"), "{log}");
    // The reviews of that run are written all the same. @lfy def/cli/main.lfy:main
    assert!(stood.read("elfie-requests/a.reviews.json").contains("covered by a test"));
}

/// A run that answered for every id it was asked about stands, whatever else it wrote:
/// the verifier is not run again, and what it wrote beside its reviews is printed under
/// its reviewed line.
// @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
// @lfy def/cli/main.lfy:main#main:main:7ac5c7dbdba835816dd103d3fb573c0697886fc87ae20c77a226aaa3f7b90592
#[test]
fn a_review_that_ended_stands_and_what_else_it_wrote_is_printed() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED_WITH_A_STRAY_OBJECT);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
    assert_eq!(lines_of(&fixture, "verifications.txt"), 1, "the verifier ran once");
    let log = fixture.read("elfie-requests/compile.log");
    assert!(log.contains("1 satisfied, 0 violated, 0 unverifiable"), "{log}");
    // What it wrote beside its reviews follows its reviewed line, indented.
    // @lfy def/cli/main.lfy:main#main:main:7ac5c7dbdba835816dd103d3fb573c0697886fc87ae20c77a226aaa3f7b90592
    assert!(log.contains("I also looked at the tests"), "{log}");
    // @lfy def/cli/main.lfy:main#main:main:a353bdb9028859017c78bc8c4a41002b178de74b8ffcf75b6de2ec4d515c7bcf
    assert!(!log.contains("✗ failed"), "{log}");
}

/// Every line of the verifier's output reaches the log, the lines it wrote to standard
/// error among them, each with the prefix it was shown with and no escape.
// @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
#[test]
fn every_line_the_verifier_wrote_reaches_the_log() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED_WITH_NOISE);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let log = fixture.read("elfie-requests/compile.log");
    // The line it wrote to standard error, prefixed by the batch as it was shown.
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    assert!(log.contains("  a \u{2502} reading the outputs"), "{log}");
    // Its standard output, and the report it is read from, are there too.
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    assert!(log.contains("  a \u{2502} ELFIE: REVIEWED"), "{log}");
    assert!(log.contains("--- the review of a (attempt 1) ---"), "{log}");
    // What the compiler wrote is logged the same way.
    // @lfy def/cli/main.lfy:main#main:main:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    assert!(log.contains("  a \u{2502} ELFIE: DONE"), "{log}");
    // Every line of it is the line as printed with colors off.
    // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
    assert!(!log.contains('\u{1b}'), "the log holds an escape");
}

/// With --no-verify no verifier runs, nothing is reviewed, no reviews file is written,
/// and the batch is done and its source maps recorded when its units are accepted.
// @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
#[test]
fn no_verify_runs_no_verifier() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, VIOLATED);
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran once");
    assert!(!fixture.root.join("verifications.txt").exists(), "no verifier ran");
    assert!(!fixture.root.join("elfie-requests/a.reviews.json").exists(), "nothing is reviewed");
    // The source maps are recorded in the unit's own map file all the same.
    // @lfy def/cli/main.lfy:main
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    // A project naming no verifier is the same. @lfy def/cli/main.lfy:main
    let plain = Fixture::new();
    a_compiler_and_a_verifier(&plain, VIOLATED).write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    assert_eq!(plain.run(&["compile"]), ExitCode::Success.code());
    assert!(!plain.root.join("verifications.txt").exists());
    assert!(!plain.root.join("elfie-requests/a.reviews.json").exists());
}

/// Verify runs the verifier on the outputs already recorded and compiles nothing.
// @lfy def/cli/main.lfy:main#main:main:37d72b3d77ba44131f58978f7f86e78a53b525f1f2ea28140bd3adeb3c4d2343
#[test]
fn verify_reviews_what_is_recorded_without_compiling() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1);
    // No review was violated, so the held source maps were recorded and the batch was
    // done. @lfy def/cli/main.lfy:main
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    let _ = fs::remove_file(fixture.root.join("elfie-requests/a.reviews.json"));
    // The verdict the compile recorded is forgotten, so the review asks about the
    // criterion again rather than carrying it.
    // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
    forget_reviews(&fixture);
    // @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "no compiler is run");
    let reviews = fixture.read("elfie-requests/a.reviews.json");
    assert!(reviews.contains("covered by a test"), "{reviews}");
    // A violated review makes the code problems. @lfy def/cli/main.lfy:main
    write_verifier(&fixture, VIOLATED);
    assert_eq!(fixture.run(&["verify"]), ExitCode::Problems.code());
    // A stem naming a unit whose outputs are empty is left out, and the code does not
    // change for it. @lfy def/cli/main.lfy:main
    fixture.write("def/b.lfy", "d B {}\n");
    write_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["verify", "b"]), ExitCode::Success.code());
    assert!(!fixture.root.join("elfie-requests/b.reviews.json").exists(), "nothing was reviewed for b");
}

/// Verify on a unit already recorded, whose review is violated, prints that review and
/// the counts, writes the reviews, runs no compiler, and returns problems.
// @lfy def/cli/main.lfy:main#main:main:8127972dd45dbd924545668a1b1bcb7a18efd3820ce24def88c57212aa87d8ab
#[test]
fn verify_returns_problems_for_a_violated_review() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // The output for a is recorded in elfie-compile/maps/rust/a.json.
    // @lfy def/cli/main.lfy:main
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    let _ = fs::remove_file(fixture.root.join("elfie-requests/a.reviews.json"));
    // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
    forget_reviews(&fixture);
    write_verifier(&fixture, VIOLATED_IN_SRC);
    // The code is problems when any review is violated. @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Problems.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "no compiler is run");
    // elfie-requests/a.reviews.json holds the review. @lfy def/cli/main.lfy:main
    let reviews = fixture.read("elfie-requests/a.reviews.json");
    assert!(reviews.contains("returns 0 for an empty list"), "{reviews}");
    assert!(reviews.contains("src/a.rs:4-6"), "{reviews}");
    // It is printed as violated, the file, a colon, the line, the entity, a colon, and
    // the note, then the counts. @lfy def/cli/main.lfy:main
    let (program, _) = plan_with(&fixture.root, &[], &[], &[]);
    let line = format!(
        "{{\"id\":\"{}\",\"status\":\"violated\",\"evidence\":\"src/a.rs:4-6\",\"note\":\"returns 0 for an empty list\"}}\nELFIE: REVIEWED\n",
        criterion_id(&fixture)
    );
    let report = generation::review_of(&line, &program, &[], &[]);
    let column = width(&ReviewStatus::Unverifiable.to_string());
    assert_eq!(
        painted_review(&report.reviews[0], false),
        format!("{} def/a.lfy:3 A: returns 0 for an empty list", padded("violated", column))
    );
    // @lfy def/cli/main.lfy:main
    assert_eq!(
        counts_message(&counts_with_missing(&report.reviews, 0), false),
        "0 satisfied, 1 violated, 0 unverifiable"
    );
}

/// With no verifier named, the review request of each batch is written to
/// elfie-requests named by the batch identifier with the extension .review.md, and the
/// code is success.
// @lfy def/cli/main.lfy:main
#[test]
fn verify_with_no_verifier_writes_the_review_requests() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    fixture.write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    assert_eq!(fixture.run(&["verify"]), ExitCode::Success.code());
    let request = fixture.read("elfie-requests/a.review.md");
    assert!(request.contains("def/a.lfy"), "{request}");
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "nothing is compiled");
}

/// The id of the one global criterion of the project.
fn global_id(fixture: &Fixture) -> String {
    let (program, _) = plan_with(&fixture.root, &[], &[], &[]);
    program.criteria.first().map(|criterion| criterion.id.clone()).unwrap_or_else(|| panic!("no global criterion"))
}

/// The same project with one global criterion, so a global review has something to review,
/// and a compiler whose output answers for it. The verifier's `<global>` is the id of that
/// criterion and its `<id>` the id of the criterion of A.
// @lfy def/cli/main.lfy:main
fn with_a_global_criterion<'f>(fixture: &'f Fixture, verifier: &str) -> &'f Fixture {
    a_compiler_and_a_verifier(fixture, SATISFIED);
    fixture.write(
        "def/a.lfy",
        "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\nglobal@acceptanceCriteria.add({ behavior = `Every output builds` });\n",
    );
    let global = global_id(fixture);
    // The marker answers for the global criterion, so the unit is one the global review
    // can send back. @lfy def/cli/main.lfy:main
    fixture.write(
        "compiler.sh",
        &format!(
            "#!/bin/sh\ncat >> \"$ELFIE_ROOT/instructions.txt\"\necho compile >> \"$ELFIE_ROOT/attempts.txt\"\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:A#{global}\\npub struct A {{}}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho 'ELFIE: DONE'\n"
        ),
    );
    let script = verifier.replace("<id>", &criterion_id(fixture)).replace("<global>", &global);
    fixture.write("verifier.sh", &script)
}

/// A verifier that answers for the global criterion when it is run for the global review
/// and for the criterion of A otherwise, with the status each is given.
fn branching_verifier(global: &str, local: &str) -> String {
    format!(
        "#!/bin/sh\ncat > /dev/null\necho \"$ELFIE_BATCH\" >> \"$ELFIE_ROOT/verifications.txt\"\necho \"$ELFIE_BATCH=$ELFIE_UNITS\" >> \"$ELFIE_ROOT/units.txt\"\nif [ \"$ELFIE_BATCH\" = global ]; then\n  echo '{{\"id\":\"<global>\",\"status\":\"{global}\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"every output builds\"}}'\nelse\n  echo '{{\"id\":\"<id>\",\"status\":\"{local}\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}}'\nfi\necho 'ELFIE: REVIEWED'\n"
    )
}

/// Every global criterion is reviewed once for the whole program after every batch has
/// been handled, and the review is written with the hash of the ids it answered for.
// @lfy def/cli/main.lfy:main
#[test]
fn a_global_criterion_is_reviewed_once_after_every_batch() {
    let fixture = Fixture::new();
    with_a_global_criterion(&fixture, &branching_verifier("satisfied", "satisfied"));
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // The verifier ran once for the batch and once for the global review, with
    // ELFIE_BATCH set to global for the second. @lfy def/cli/main.lfy:main
    assert_eq!(fixture.read("verifications.txt"), "a\nglobal\n");
    // ELFIE_UNITS names every unit whose source maps answer for a global id, the unit
    // this compile just generated among them: it has no outputs from an earlier
    // generation, so only the maps this compile accepted name it.
    // @lfy def/cli/main.lfy:main
    assert_eq!(fixture.read("units.txt"), "a=a\nglobal=a\n");
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/main.lfy:main
    assert!(log.contains("▸ globalVerifying"), "{log}");
    assert!(log.contains("✓ globalReviewed"), "{log}");
    // elfie-requests/global.reviews.json holds the hash of the global ids reviewed and
    // the report. @lfy def/cli/main.lfy:main
    let written = fixture.read("elfie-requests/global.reviews.json");
    let value: serde_json::Value = serde_json::from_str(&written).unwrap();
    let (program, _) = plan_with(&fixture.root, &[], &[], &[]);
    assert_eq!(value["requirements"], GlobalReview::requirements_of(&program));
    assert_eq!(value["reviews"][0]["id"], global_id(&fixture));
    assert_eq!(value["reviews"][0]["status"], "satisfied");
    // Nothing was violated, so nothing is planned with reason violated.
    // @lfy def/cli/main.lfy:main
    assert!(violated_ids(&fixture.root).is_empty());
    // The second compile has nothing to accept and the ids it answered for did not
    // change, so no global review runs again. @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert_eq!(fixture.read("verifications.txt"), "a\nglobal\n");
}

/// With no global criterion and no global test, no global review runs and no file is
/// written, by a compile or by a verify.
// @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
#[test]
fn no_global_criterion_means_no_global_review() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
    assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
    let log = fixture.read("elfie-requests/compile.log");
    assert!(!log.contains("globalVerifying"), "{log}");
    // The verifier ran once, for the batch, and never for a global review.
    assert_eq!(lines_of(&fixture, "verifications.txt"), 1);
    // verify with no stem reviews the batch and asks for no global review either, because
    // there is nothing global to review. With --full the batch is reviewed again rather
    // than carrying the verdict the compile recorded.
    // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    assert_eq!(fixture.run(&["verify", "--full"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
    assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
    // verify --global with nothing global to review runs no verifier at all.
    // @lfy def/cli/main.lfy:main#main:main:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
    assert_eq!(fixture.run(&["verify", "--global"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
    assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
}

/// A violated global review plans the units whose markers answered for it with reason
/// violated and compiles them once more in the same compile, and the global review runs
/// once more after them; a second violated review stops the compile with the code
/// problems, and the next compile plans those units with reason violated.
// @lfy def/cli/main.lfy:main
#[test]
fn a_violated_global_review_plans_its_units_once_more() {
    let fixture = Fixture::new();
    with_a_global_criterion(&fixture, &branching_verifier("violated", "satisfied"));
    // @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    // The batch was handled twice and the global review ran after each round. The second
    // round compiled the unit again and its output was the same, so the verdict on its
    // own criterion was carried and no verifier ran on the batch.
    // @lfy def/cli/main.lfy:main
    // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
    assert_eq!(fixture.read("verifications.txt"), "a\nglobal\nglobal\n");
    assert_eq!(lines_of(&fixture, "attempts.txt"), 2, "the unit was compiled once more");
    let log = fixture.read("elfie-requests/compile.log");
    // One problem per violated review, as failure at, a space, global, a colon, a space,
    // the note, and the evidence in parentheses. @lfy def/cli/main.lfy:main
    assert!(log.contains("    - failure at global: every output builds (out/a.rs:1-2)"), "{log}");
    // The next compile plans those units with reason violated. @lfy def/cli/main.lfy:main
    let ids = violated_ids(&fixture.root);
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&global_id(&fixture)));
    let (_, plan) = planned(&fixture.root, &generation::source_maps_of(&workspace::load(&fixture.root), None), &[], false, &ids);
    let unit = plan.units.iter().find(|unit| unit.stem == "a").unwrap();
    // @lfy def/cli/main.lfy:main
    assert_eq!(unit.reason, Some(Reason::Violated));
}
/// A compile with nothing to do still begins with planned and ends with finished, even
/// when a global review runs between them because no global review was ever recorded.
// @lfy def/cli/main.lfy:main
#[test]
fn a_compile_with_nothing_to_do_still_begins_planned_and_ends_finished() {
    let fixture = Fixture::new();
    with_a_global_criterion(&fixture, &branching_verifier("satisfied", "satisfied"));
    // The unit is compiled and recorded with no verifier, so nothing is planned next time
    // and no global review was ever recorded. @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    fs::remove_file(fixture.root.join("elfie-requests/compile.log")).unwrap();
    // @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let log = fixture.read("elfie-requests/compile.log");
    // Nothing was planned, so the counter keeps the width of no units at all.
    // @lfy def/cli/main.lfy:main
    assert!(log.contains("[0/0]"), "{log}");
    // The global review still ran, and the first progress line is planned all the same.
    // @lfy def/cli/main.lfy:main
    assert!(log.contains("▸ globalVerifying"), "{log}");
    let mut lines = log.lines().filter(|line| !line.trim().is_empty());
    // @lfy def/cli/main.lfy:main
    assert!(lines.next().is_some_and(|first| first.contains("◆ planned")), "{log}");
    // @lfy def/cli/main.lfy:main
    assert!(lines.next_back().is_some_and(|last| last.contains("◆ finished")), "{log}");
}

/// Verify with --global and no stem reviews no batch, runs the verifier once for the
/// global criteria and tests, and writes the global review.
// @lfy def/cli/main.lfy:main
#[test]
fn verify_global_reviews_no_batch() {
    let fixture = Fixture::new();
    with_a_global_criterion(&fixture, &branching_verifier("satisfied", "satisfied"));
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    assert!(!fixture.root.join("verifications.txt").exists(), "no verifier ran");
    // @lfy def/cli/main.lfy:main
    assert_eq!(fixture.run(&["verify", "--global"]), ExitCode::Success.code());
    // @lfy def/cli/main.lfy:main
    assert_eq!(fixture.read("verifications.txt"), "global\n");
    assert!(!fixture.root.join("elfie-requests/a.reviews.json").exists(), "a batch was reviewed");
    // @lfy def/cli/main.lfy:main
    let written = fixture.read("elfie-requests/global.reviews.json");
    assert!(written.contains(&global_id(&fixture)), "{written}");
    // Nothing under elfie-compile is recorded, moved, or removed by verify.
    // @lfy def/cli/main.lfy:main
    let before = fixture.read("elfie-compile/maps/rust/a.json");
    assert_eq!(fixture.run(&["verify"]), ExitCode::Success.code());
    assert_eq!(fixture.read("elfie-compile/maps/rust/a.json"), before);
    // With no stem and no --global every batch is reviewed and the global review follows.
    // @lfy def/cli/main.lfy:main
    assert_eq!(fixture.read("verifications.txt"), "global\na\nglobal\n");
    // A violated review makes the code problems. @lfy def/cli/main.lfy:main
    let script = branching_verifier("violated", "satisfied")
        .replace("<id>", &criterion_id(&fixture))
        .replace("<global>", &global_id(&fixture));
    fixture.write("verifier.sh", &script);
    assert_eq!(fixture.run(&["verify", "--global"]), ExitCode::Problems.code());
}
