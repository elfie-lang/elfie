// @lfy def/cli/main.lfy:compile
//! Tests of one compile from end to end: the program it refuses, the requests it writes, the
//! progress lines it prints, the outcomes it reads, the reviews it asks for, and the global
//! review that follows every unit.

use super::*;
use crate::compiling::loaded_diagnostics;
use crate::reviewing::{error_diagnostics, missing_lines, not_reviewed, painted_review, problem_of};
use crate::running::Stop;
use crate::schedule::Ended;

/// Compile prints the errors and returns problems rather than handing the compiler a
/// program with problems.
// @lfy def/cli/main.lfy:compile#compile:compile:3df84098154780d5074fdbf4d1209831b3e3f8bc87b7bc577035c1b63e2f443d
// @lfy def/cli/main.lfy:compile#compile:compile:77ae4e6a5e2396d858d173399db2122dd9965d17bc44b4f05613dfae2362f448
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
// @lfy def/cli/main.lfy:compile#compile:compile:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
// @lfy def/cli/main.lfy:verify#verify:verify:83640c302c4ea1762f8e4981643b58c9c5094bba9180dc2995305d2406a7615c
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
    // @lfy def/cli/main.lfy:compile#compile:compile:f2d15d5c350694370ffb5656be0859f0f69da753546ea57b01ed5e684923d727
    let clean = Fixture::new();
    clean.write("def/b.lfy", "d B: `A B` {}\n").write("def/a.lfy", "use \"./b\";\n\nconst y = 1;\n");
    let loaded = workspace::load(&clean.root);
    assert!(loaded_diagnostics(&loaded).is_empty());
    assert!(query::diagnostics_of(&loaded, None).iter().any(|d| d.severity == Severity::Warning));
}

/// With no compiler named, the request of each planned unit is written to
/// `elfie-requests/<name>.md` under the root, and the code is success. It is read by a
/// compiler run by hand, in no hosted session, so it names none of the tools a session
/// offers; no change set is made.
// @lfy def/cli/main.lfy:compile#compile:compile:af5bc490a429019de13aaa5043d30cc6c3e41965559e13e6116286205a08801e
// @lfy def/cli/main.lfy:compile#compile:compile:d5f565b191777716fd8e847870720c599c23591f1d959c0cb67397a6770f7536
#[test]
fn a_project_with_no_compiler_writes_the_requests() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/cli/main.lfy", "d A: `An A` {}\n");
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // The file is named by the unit's request name.
    // @lfy def/cli/main.lfy:compile#compile:compile:471b5ff5941e414fe0f5f5350c3d0f8c3560fdbfd6326589991ff388f7367356
    let instructions = fixture.read("elfie-requests/cli-main.md");
    assert!(instructions.contains("An A"), "{instructions}");
    assert!(instructions.contains("def/cli/main.lfy"), "{instructions}");
    // The request was made for no hosted session.
    // @lfy def/cli/main.lfy:compile#compile:compile:af5bc490a429019de13aaa5043d30cc6c3e41965559e13e6116286205a08801e
    assert!(!instructions.contains("elfie_task"), "{instructions}");
    assert!(!instructions.contains("elfie_submit"), "{instructions}");
    // No change set is made.
    // @lfy def/cli/main.lfy:compile#compile:compile:d5f565b191777716fd8e847870720c599c23591f1d959c0cb67397a6770f7536
    assert!(!fixture.root.join("elfie-compile/cache/changes").exists());
    // Every progress line is appended to the log, as printed with colors off.
    // @lfy def/cli/main.lfy:compile#compile:compile:19bf885943ca638eedeb6dd907ab4cbe264301f81b3b1bc8f82658eb6a31f2b9
    // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
    let log = fixture.read("elfie-requests/compile.log");
    assert!(log.contains("[0/1]"), "{log}");
    assert!(log.contains("◆ planned"), "{log}");
    assert!(log.contains("1 units planned, 0 moves"), "{log}");
    assert!(log.contains("· requesting"), "{log}");
    assert!(log.contains("◆ finished"), "{log}");
    assert!(!log.contains('\u{1b}'), "the log holds an escape");
    // Neither the compiler nor the verifier ran, so no agent server was hosted and no
    // session's config file was written.
    // @lfy def/cli/main.lfy:compile#compile:compile:528aaa4cb8488c953d03050aadaa0d8bc000afe6ba74f6af3daa9146ffb6c4c4
    assert!(!fixture.root.join("elfie-requests/cli-main.compiler.mcp.json").exists());
}

/// Each progress line reads, in columns separated by two spaces: the counter, the time,
/// the step with its glyph, then the unit when there is one, and the first line of the
/// message.
// @lfy def/cli/main.lfy:compile#compile:compile:fd851430001b39d2d1c79a0807242453986bf07f0fbe972e5ecd9815c3380aba
// @lfy def/cli/main.lfy:compile#compile:compile:e644285bff17055757f9da45f329dc88d9e4ec34a19c3b5ce92b8a4a03950621
// @lfy def/cli/main.lfy:compile#compile:compile:72582398057ef4b458680631986c9aa55de4061eecc790f79ca14cc77a71b86d
// @lfy def/cli/main.lfy:compile#compile:compile:a8367555ef05292d4118872e01aece83edde2d5816e9e4c404840757608603d3
// @lfy def/cli/main.lfy:compile#compile:compile:2ba85a50267062837687f356e2de2edec47dfcbaf012913034645e8c9586d86c
// @lfy def/cli/main.lfy:compile#compile:compile:e6a992a863aebc985eeb11dd2cef62220e5e7590ea2500babb4124684929657c
// @lfy def/cli/main.lfy:compile#compile:compile:679737b9c65e8c1627af4566b3a9f5d490c8790929b3f394353619a617003c6e
// @lfy def/cli/main.lfy:compile#compile:compile:c164c51236df72a93cac3a3c7bcc44ec0848586ed13d2f9add36192db3bc1999
// @lfy def/cli/main.lfy:compile#compile:compile:fa43f8b6da9b3c369ebca761ab832f180a9bc8eb02b280dd2435250f4884c825
#[test]
fn a_progress_line_carries_the_step_the_counts_and_the_elapsed_seconds() {
    let progress = Progress {
        step: Step::Accepted,
        unit: Some("cli/main".to_string()),
        done: 2,
        total: 7,
        elapsed: 12.34,
        message: "3 outputs\nand more".to_string(),
    };
    let line = text_of(&progress, None, None, false);
    assert_eq!(line, format!("[2/7]   12.3s  {}  cli/main  3 outputs", padded("✓ accepted", step_column())));
    let columns: Vec<&str> = line.split("  ").map(str::trim).filter(|column| !column.is_empty()).collect();
    assert_eq!(columns, vec!["[2/7]", "12.3s", "✓ accepted", "cli/main", "3 outputs"], "{line:?}");
    let value = json_of(&progress);
    assert_eq!(value["step"], "accepted");
    assert_eq!(value["unit"], "cli/main");
    assert_eq!(value["done"], 2);
    assert_eq!(value["total"], 7);
    assert_eq!(value["message"], "3 outputs\nand more");
    assert!(value.get("batch").is_none(), "{value}");
    // Painting leaves every column the width it had.
    assert_eq!(width(&text_of(&progress, None, None, true)), width(&line));
    // The unit is painted subject.
    // @lfy def/cli/main.lfy:compile#compile:compile:679737b9c65e8c1627af4566b3a9f5d490c8790929b3f394353619a617003c6e
    assert!(text_of(&progress, None, None, true).contains(&paint("cli/main", Tone::Subject, true)));
    // The counter keeps one width for the whole compile.
    let wide = Progress { done: 9, total: 100, ..progress.clone() };
    assert!(text_of(&wide, None, None, false).starts_with("[  9/100]"));
    // A line that concerns no one unit has no unit column.
    let planned = Progress {
        step: Step::Planned,
        unit: None,
        done: 0,
        total: 7,
        elapsed: 0.0,
        message: "7 units planned, 2 moves".to_string(),
    };
    assert_eq!(
        text_of(&planned, None, None, false),
        format!("[0/7]    0.0s  {}  7 units planned, 2 moves", padded("◆ planned", step_column()))
    );
    assert_eq!(json_of(&planned)["unit"], serde_json::Value::Null);
    // A rejection's message is painted in the step's tone, and a step that is none of
    // rejected, failed, blocked, or clarification is plain, touched and merging among them.
    // @lfy def/cli/main.lfy:compile#compile:compile:c164c51236df72a93cac3a3c7bcc44ec0848586ed13d2f9add36192db3bc1999
    // @lfy def/cli/main.lfy:compile#compile:compile:fa43f8b6da9b3c369ebca761ab832f180a9bc8eb02b280dd2435250f4884c825
    let rejected = Progress { step: Step::Rejected, ..progress.clone() };
    assert!(text_of(&rejected, None, None, true).ends_with(&paint("3 outputs", Tone::Failure, true)));
    for step in [
        Step::Accepted,
        Step::Touched,
        Step::Stubbed,
        Step::FinalGate,
        Step::Merging,
        Step::Relocated,
        Step::Started,
        Step::Merged,
        Step::Undone,
    ] {
        let plain = Progress { step, ..progress.clone() };
        assert!(text_of(&plain, None, None, true).ends_with("3 outputs"), "{step:?}");
    }
    // A reviewed line's message is the counts, each a tally.
    // @lfy def/cli/main.lfy:compile#compile:compile:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
    let counts: Counts = vec![(1, "satisfied", Tone::Success), (0, "violated", Tone::Failure), (0, "unverifiable", Tone::Warning)];
    assert_eq!(counts_message(&counts, false), "1 satisfied, 0 violated, 0 unverifiable");
    assert_eq!(width(&counts_message(&counts, true)), width(&counts_message(&counts, false)));
}

/// A unit's request name is its stem with each slash replaced by a dash.
// @lfy def/cli/main.lfy:compile#compile:compile:471b5ff5941e414fe0f5f5350c3d0f8c3560fdbfd6326589991ff388f7367356
#[test]
fn a_unit_names_its_note_files() {
    assert_eq!(request_name("cli/main"), "cli-main");
    assert_eq!(request_name("a"), "a");
}

/// The answer a person wrote below the question is read back; a question with none gives
/// nothing.
// @lfy def/cli/main.lfy:compile#compile:compile:1589ce12c96cdf908edd65828b29cd2a2ced6c580a9f214a9cfb2030bf2539ce
#[test]
fn the_answer_below_a_question_is_read_back() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let run = a_run(&fixture, &["compile", "--color=never"]);
    let unanswered = format!("# Asked\n\nWhich one?\n\n{ANSWER_HEADING}\n\n<!-- Write the answer below this line. -->\n");
    fixture.write("elfie-requests/a.question.md", &unanswered);
    assert_eq!(run.answer_of("a"), None);
    fixture.write("elfie-requests/a.question.md", &format!("{unanswered}\nThe second one.\n"));
    assert_eq!(run.answer_of("a").as_deref(), Some("The second one."));
}

/// With --json the message of the finished line is the four counts alone.
// @lfy def/cli/main.lfy:compile#compile:compile:1ece271551709a1351fbd18d13e9516ddea9d6fb7ad14955145eb9f320db5bf5
#[test]
fn the_finished_message_with_json_is_the_four_counts() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let invocation = parse_arguments(&args(&["compile", "--json"])).unwrap();
    let log = fixture.root.join("finished.log");
    let planning = Planning::default();
    let (program, plan) = planning.plan(&fixture.root, &[]);
    let reporter = Reporter::new(true, Paint::of(&invocation), 1, vec![log]);
    let mut run = Run::new(program, plan, planning, reporter, &invocation);
    run.rejected = 1;
    run.finish();
    // @lfy def/cli/main.lfy:compile#compile:compile:1ece271551709a1351fbd18d13e9516ddea9d6fb7ad14955145eb9f320db5bf5
    let line = fixture.read("finished.log");
    let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(value["message"], "0 accepted, 1 rejected, 0 blocked, 0 failed", "{line}");
}

/// The code is the worst thing the compile met.
// @lfy def/cli/main.lfy:compile#compile:compile:3d1e2128231c0dd4a623216ed6845b9bfdce55b3a624742046eb528f6cff68dd
// @lfy def/cli/main.lfy:compile#compile:compile:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
#[test]
fn the_code_is_the_worst_thing_the_compile_met() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let mut run = a_run(&fixture, &["compile", "--color=never"]);
    assert_eq!(run.code, ExitCode::Success);
    run.worsen(ExitCode::Problems);
    assert_eq!(run.code, ExitCode::Problems);
    run.worsen(ExitCode::Success);
    assert_eq!(run.code, ExitCode::Problems, "a milder code does not win");
    run.worsen(ExitCode::Failure);
    assert_eq!(run.code, ExitCode::Failure);
}

/// With --accept no compiler is run and no change set is made; outputs that carry no marker
/// for the unit are rejected and the code is problems; accepted ones are written back
/// normalized in the root and their maps recorded there.
// @lfy def/cli/main.lfy:compile#compile:compile:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
// @lfy def/cli/main.lfy:compile#compile:compile:0440d8388eff8352220c16c7b7cbcac23c06b46e2e0c8882f55b1629dc83a76a
// @lfy def/cli/main.lfy:compile#compile:compile:bc92c42f122f408df6d607e49b325266342f43d2432cc4b810428d20b881f68f
// @lfy def/cli/main.lfy:compile#compile:compile:716c12d8173223d984bf23488c69bcafe1c34d0dfe121ff09b38ac8826ccee47
// @lfy def/cli/main.lfy:compile#compile:compile:c65e5aa6168e9f62b98f454af939726af67cc464dfa4dcadffec41cbf6e60044
#[test]
fn accept_checks_what_is_already_on_disk() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    // Nothing on disk: the unit is rejected.
    // @lfy def/cli/main.lfy:compile#compile:compile:c65e5aa6168e9f62b98f454af939726af67cc464dfa4dcadffec41cbf6e60044
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Problems.code());
    // An output carrying a marker for the unit is accepted and recorded.
    fixture.write("out/a.rs", "// @lfy def/a.lfy:1\npub struct A {}\n");
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Success.code());
    // @lfy def/cli/main.lfy:compile#compile:compile:716c12d8173223d984bf23488c69bcafe1c34d0dfe121ff09b38ac8826ccee47
    let maps = fixture.read("elfie-compile/maps/rust/a.json");
    assert!(maps.contains("\"out/a.rs\""), "{maps}");
    // The marker is written back naming the entity rather than the line.
    // @lfy def/cli/main.lfy:compile#compile:compile:bc92c42f122f408df6d607e49b325266342f43d2432cc4b810428d20b881f68f
    assert_eq!(fixture.read("out/a.rs"), "// @lfy def/a.lfy:A\npub struct A {}\n");
    // No change set was made.
    // @lfy def/cli/main.lfy:compile#compile:compile:0440d8388eff8352220c16c7b7cbcac23c06b46e2e0c8882f55b1629dc83a76a
    assert!(!fixture.root.join("elfie-compile/cache/changes").exists());
    // And the unit is then up to date.
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert!(!fixture.root.join("elfie-requests/a.md").exists(), "nothing is requested");
}

/// The compiler is run once per unit, in its change set's copy, with the request on its
/// standard input and ELFIE_ROOT, ELFIE_UNIT, ELFIE, and ELFIE_COMMAND_TOOLS in its
/// environment; its standard output is kept whole as the report, and once the root takes
/// the change set its outputs and maps are in the root and the change set is gone.
// @lfy def/cli/main.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
// @lfy def/cli/main.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
// @lfy def/cli/main.lfy:compile#compile:compile:7522539013e41173fe7dcc12bb506b34925d3a1de610ef78fe9a1db5ea960f10
// @lfy def/cli/main.lfy:compile#compile:compile:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
// @lfy def/cli/main.lfy:compile#compile:compile:d65259b76766c2e72bd7a2d4077d7121611026a93aacf0a91ba6d86e97ddbe6d
// @lfy def/cli/main.lfy:compile#compile:compile:5d1e63160121bb44aeb91bd7d1f6eced00a1991447dd7d14e77ff2e4a4bf2f9a
// @lfy def/cli/main.lfy:compile#compile:compile:26973cf3ae9c173d9722bc790d44288e8dd93cefaa7ca69f28a04291934c180c
#[test]
fn the_compiler_is_run_per_unit_in_its_change_set_and_its_report_is_read() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/a.lfy", "d A: `An A` {}\n")
        .write(
            "compiler.sh",
            &noted(
                &fixture,
                "#!/bin/sh\ncat > '<notes>/instructions.txt'\nprintf '%s\\n' \"$ELFIE_UNIT\" \"$ELFIE\" \"[$ELFIE_COMMAND_TOOLS]\" \"$ELFIE_ROOT\" \"${ELFIE_BATCH-none}\" > '<notes>/environment.txt'\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:1\\npub struct A {}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho done\n",
            ),
        )
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let environment = fixture.read("environment.txt");
    let lines: Vec<&str> = environment.lines().collect();
    // ELFIE_UNIT is the unit's stem.
    // @lfy def/cli/main.lfy:compile#compile:compile:7522539013e41173fe7dcc12bb506b34925d3a1de610ef78fe9a1db5ea960f10
    assert_eq!(lines[0], "a");
    // ELFIE is the path of the running executable.
    // @lfy def/cli/main.lfy:compile#compile:compile:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    assert_eq!(lines[1], std::env::current_exe().unwrap().to_string_lossy());
    assert_eq!(lines[2], "[]");
    // ELFIE_ROOT is the directory it runs in: the unit's own copy, an absolute path.
    // @lfy def/cli/main.lfy:compile#compile:compile:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
    assert!(lines[3].ends_with("elfie-compile/cache/changes/rust/units/a"), "{environment}");
    assert!(Path::new(lines[3]).is_absolute(), "{environment}");
    // Nothing of a batch is in its environment any more.
    assert_eq!(lines[4], "none");
    let instructions = fixture.read("instructions.txt");
    assert!(instructions.contains("An A"), "{instructions}");
    // The request was made for the run's hosted session.
    // @lfy def/cli/main.lfy:compile#compile:compile:3a5a6187176db8b0b5e61a117d0b28b3181489ed6ba22acc61e795ac4bbf9d31
    assert!(instructions.contains("elfie_task"), "{instructions}");
    assert!(instructions.contains("elfie_submit"), "{instructions}");
    // The root took the change set: the output and its maps are there, and the change set's
    // file, copy, and base are gone.
    // @lfy def/cli/main.lfy:compile#compile:compile:5d1e63160121bb44aeb91bd7d1f6eced00a1991447dd7d14e77ff2e4a4bf2f9a
    assert_eq!(fixture.read("out/a.rs"), "// @lfy def/a.lfy:A\npub struct A {}\n");
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    assert!(!fixture.root.join("elfie-compile/cache/changes/rust.json").exists());
    assert!(!fixture.root.join("elfie-compile/cache/changes/rust").exists());
    let log = fixture.read("elfie-requests/compile.log");
    assert!(log.contains("done"), "{log}");
    assert!(log.contains("▸ compiling"), "{log}");
    // Every unit that runs has a line naming it.
    // @lfy def/cli/main.lfy:compile#compile:compile:26973cf3ae9c173d9722bc790d44288e8dd93cefaa7ca69f28a04291934c180c
    assert!(log.lines().any(|line| line.contains("✓ accepted") && line.contains("  a  ")), "{log}");
    // A final gate and a merging line, each naming the target and each a block of its own.
    // @lfy def/cli/main.lfy:compile#compile:compile:f2d95b937709d596c90877c72c560452b08ca12e639e6792b8be4919cd63a3f9
    // @lfy def/cli/main.lfy:compile#compile:compile:d65259b76766c2e72bd7a2d4077d7121611026a93aacf0a91ba6d86e97ddbe6d
    // @lfy def/cli/main.lfy:compile#compile:compile:4630171e9007878ed9f3e54a7018754522e39bd151c749a09d46ba54f64b0a3e
    assert!(log.lines().any(|line| line.contains("▸ finalGate") && line.ends_with("rust")), "{log}");
    assert!(log.lines().any(|line| line.contains("▸ merging") && line.ends_with("rust")), "{log}");
    assert!(log.contains("\n\n[1/1]"), "{log}");
    assert!(log.contains("1 accepted, 0 rejected, 0 blocked, 0 failed"), "{log}");
}

/// A compiler that produces nothing is rejected, run once more, and then rejected after its
/// retries, its change set kept; a blocked one writes its reason, a question writes itself,
/// and an answer written below the question reaches the next request.
// @lfy def/cli/main.lfy:compile#compile:compile:4f06b5aa3049fb148a394aa3261e0dc49c2e9cb564b4604bea58741f2a16204f
// @lfy def/cli/main.lfy:compile#compile:compile:466c1564df1c6196c4187bb8cee949205482091b548db1a7939a7ef1478b2dec
// @lfy def/cli/main.lfy:compile#compile:compile:f2cd308323a1ddb4160c2646b3344c683cf4636e80cf8a2d14cf1ebb99940463
// @lfy def/cli/main.lfy:compile#compile:compile:346b7397a5c634165323cd967d3b1d1db8ef9b2acf33fa25c95a41ae05f695f6
// @lfy def/cli/main.lfy:compile#compile:compile:1589ce12c96cdf908edd65828b29cd2a2ced6c580a9f214a9cfb2030bf2539ce
#[test]
fn a_rejected_unit_is_run_once_more_and_a_blocked_one_writes_its_reason() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/a.lfy", "d A: `An A` {}\n")
        .write("compiler.sh", &noted(&fixture, "#!/bin/sh\ncat > /dev/null\necho 'attempt' >> '<notes>/attempts.txt'\n"))
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    assert_eq!(fixture.read("attempts.txt").lines().count(), 2, "the unit is run once more");
    // Rejected after its retries: its state in the kept change set is rejected.
    // @lfy def/cli/main.lfy:compile#compile:compile:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
    let kept = ChangeSet::load(&fixture.root, "rust").expect("the change set is kept");
    assert_eq!(kept.units[0].state, UnitState::Rejected);
    // A blocked unit writes its reason and stays planned.
    // @lfy def/cli/main.lfy:compile#compile:compile:f2cd308323a1ddb4160c2646b3344c683cf4636e80cf8a2d14cf1ebb99940463
    // @lfy def/cli/main.lfy:compile#compile:compile:654d815adda3e49a4d09b56d2ccda6d531ddada64a727f523fcd7ed1b5ff4b9b
    fixture.write("compiler.sh", "#!/bin/sh\ncat > /dev/null\necho 'ELFIE: BLOCKED: def/a.lfy says nothing'\n");
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    let blocked = fixture.read("elfie-requests/a.blocked.md");
    assert!(blocked.contains("def/a.lfy says nothing"), "{blocked}");
    assert_eq!(ChangeSet::load(&fixture.root, "rust").unwrap().units[0].state, UnitState::Planned);
    // @lfy def/cli/main.lfy:compile#compile:compile:346b7397a5c634165323cd967d3b1d1db8ef9b2acf33fa25c95a41ae05f695f6
    fixture.write("compiler.sh", "#!/bin/sh\ncat > /dev/null\necho 'ELFIE: CLARIFY: which A?'\n");
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    let question = fixture.read("elfie-requests/a.question.md");
    assert!(question.contains("which A?") && question.contains(ANSWER_HEADING), "{question}");
    // The answer written below the question is appended to the next request.
    // @lfy def/cli/main.lfy:compile#compile:compile:1589ce12c96cdf908edd65828b29cd2a2ced6c580a9f214a9cfb2030bf2539ce
    fixture.write("elfie-requests/a.question.md", &format!("{question}\nThe only one.\n"));
    fixture.write("compiler.sh", &noted(&fixture, "#!/bin/sh\ncat > '<notes>/instructions.txt'\n"));
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    assert!(fixture.read("instructions.txt").contains("The only one."));
}

/// A command that cannot be run fails the unit, after being run once more, and the code is
/// failure.
// @lfy def/cli/main.lfy:compile#compile:compile:c836eb712f2d9528c78321a4094d806dd4bf34577eaf2bab287bf88d3e18d9f5
// @lfy def/cli/main.lfy:compile#compile:compile:65e040b99a75c39b4cbc0f3005e29d80c5a7ebd77a30ae4ff9e4cd679a4625c5
// @lfy def/cli/main.lfy:compile#compile:compile:979d0574227f5474ed3c9c68fd3f62895696f4c2e92d830ba8016b19b0624a69
#[test]
fn a_command_that_cannot_run_is_a_failure() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let mut run = a_run(&fixture, &["compile", "--color=never"]);
    let nowhere = PathBuf::from("/no/such/directory");
    // @lfy def/cli/main.lfy:compile#compile:compile:65e040b99a75c39b4cbc0f3005e29d80c5a7ebd77a30ae4ff9e4cd679a4625c5
    assert_eq!(run.work_unit(0, "true", &nowhere), Ended::Stopped(Stop::Failed));
    run.stopped_unit(0, Stop::Failed);
    assert_eq!(run.code, ExitCode::Failure);
    assert_eq!(run.failed, 1);
    // Without --continue the compile stops there.
    // @lfy def/cli/main.lfy:compile#compile:compile:9051d3b21e94dd454bcd1cc00024dba85e18ea05da69961602354cc8d1b436bc
    assert!(run.halted);
    run.agent.server.stop();
}

/// A review is printed as its status, a space, the file, a colon, the line, a space, the
/// entity, a colon, a space, and the note; a violated one becomes one problem reading
/// failure at, the file, a colon, the line, a colon, the note, and the evidence in
/// parentheses.
// @lfy def/cli/main.lfy:verify#verify:verify:15975ba21f2d3e58d9d76c89d6ee8744c310877b9334ab62dcb7dca472cae18d
// @lfy def/cli/main.lfy:compile#compile:compile:8533b0690caaa0d5a3703e6633b44fd6034ce112c1a2c32f151e384e973b8bdf
// @lfy def/cli/main.lfy:verify#verify:verify:0011d65e2e66e2fa6bffcf28bb68b59c7d3425bddda6fd61a196e714724e5195
// @lfy def/cli/main.lfy:compile#compile:compile:2980264ce3d1cc374ba5efe5189f297ea1edce70b8a4cffbc8e194f056d2131b
// @lfy def/cli/main.lfy:compile#compile:compile:97ca8a3e66805eba7104729c205da94cf64474677a3c53e056be523758150d9d
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
    let column = width(&ReviewStatus::Unverifiable.to_string());
    assert_eq!(painted_review(&review, false), format!("{} def/a.lfy:3 A: covered by a test", padded("satisfied", column)));
    assert_eq!(width(&painted_review(&review, true)), width(&painted_review(&review, false)));
    let violated = Review {
        status: ReviewStatus::Violated,
        evidence: "crates/a/src/a.rs:4-6".to_string(),
        note: "returns 0 for an empty list".to_string(),
        ..review.clone()
    };
    // @lfy def/cli/main.lfy:compile#compile:compile:8533b0690caaa0d5a3703e6633b44fd6034ce112c1a2c32f151e384e973b8bdf
    assert_eq!(problem_of(&violated), "failure at def/a.lfy:3: returns 0 for an empty list (crates/a/src/a.rs:4-6)");
    let unverifiable = Review { status: ReviewStatus::Unverifiable, ..review.clone() };
    let every = [review, violated, unverifiable];
    assert_eq!(counts_of(&every), (1, 1, 1));
    assert_eq!(counts_message(&counts_with_missing(&every, 0), false), "1 satisfied, 1 violated, 1 unverifiable");
    // An id the review was asked about that no review names counts as unverifiable and is
    // named on one line as not reviewed.
    // @lfy def/cli/main.lfy:compile#compile:compile:97ca8a3e66805eba7104729c205da94cf64474677a3c53e056be523758150d9d
    assert_eq!(counts_message(&counts_with_missing(&every, 2), false), "1 satisfied, 1 violated, 3 unverifiable");
    let unanswered = ["A:A:one".to_string(), "A:A:two".to_string()];
    let report = ReviewReport { missing: unanswered.to_vec(), ..ReviewReport::default() };
    assert_eq!(missing_lines(&report), "not reviewed: A:A:one, A:A:two");
    assert_eq!(not_reviewed(&unanswered), missing_lines(&report));
    assert!(missing_lines(&ReviewReport::default()).is_empty());
}

/// A violated review rejects the unit: the problem is printed, the unit is run once more
/// with it appended to the instructions, the verifier runs again, and a violated review then
/// rejects the unit after its retries; the root is unchanged and the change set is kept with
/// the unit rejected.
// @lfy def/cli/main.lfy:compile#compile:compile:3d5a9861c6c05acff32b54956cb16867324933c3a2026fdeadb1b419c92796cc
#[test]
fn a_violated_review_sends_the_unit_back_and_then_rejects_it() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, VIOLATED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    // The compiler and the verifier each ran twice.
    // @lfy def/cli/main.lfy:compile#compile:compile:3d5a9861c6c05acff32b54956cb16867324933c3a2026fdeadb1b419c92796cc
    assert_eq!(lines_of(&fixture, "attempts.txt"), 2);
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
    let log = fixture.read("elfie-requests/compile.log");
    assert!(log.contains("✓ reviewed"), "{log}");
    assert!(log.contains("0 satisfied, 1 violated, 0 unverifiable"), "{log}");
    // @lfy def/cli/main.lfy:compile#compile:compile:3d5a9861c6c05acff32b54956cb16867324933c3a2026fdeadb1b419c92796cc
    assert!(log.contains("    - failure at def/a.lfy:3: returns 0 for an empty list (crates/a/src/a.rs:4-6)"), "{log}");
    assert!(log.contains("↻ retrying"), "{log}");
    assert!(!log.contains('\u{1b}'), "the log holds an escape");
    // The compiler was run a second time with that problem appended to its instructions.
    // @lfy def/cli/main.lfy:compile#compile:compile:989f6f309c8ec4cc4f9e9682676ed8016bcfaac8a5f6ccf14d33c1bc0207a0ec
    assert!(fixture.read("instructions.txt").contains("failure at def/a.lfy:3"));
    // The reviews are written, and the unit counts as a rejection.
    // @lfy def/cli/main.lfy:compile#compile:compile:15c52319c401850d74a412fdfeffa9885cf323b190149210e6e0a83cf4d62156
    assert!(fixture.read("elfie-requests/a.reviews.json").contains("violated"));
    assert!(log.contains("0 accepted, 1 rejected"), "{log}");
    // The root is unchanged: its output and its maps are not there.
    // @lfy def/cli/main.lfy:compile#compile:compile:a8d0e0a443a2adbf0143baf22ffaa2b0ec6dc8b39fb3efe5c9ec8d26eee79b87
    // @lfy def/cli/main.lfy:compile#compile:compile:85a283dc1f225f02956fbed8a23688bb8b6f297fdc8bea08fed48d08fd0de9b5
    assert!(!fixture.root.join("out/a.rs").exists(), "the root took something");
    assert!(!fixture.root.join("elfie-compile/maps/rust/a.json").exists(), "nothing is recorded");
    // The change set is kept with a in state rejected.
    // @lfy def/cli/main.lfy:compile#compile:compile:0954f463a673ef9da817b979310906a5a5009931bd64cc4941c7822e82a8d017
    let kept: serde_json::Value =
        serde_json::from_str(&fixture.read("elfie-compile/cache/changes/rust.json")).expect("the change set is kept");
    assert_eq!(kept["units"][0]["stem"], "a");
    assert_eq!(kept["units"][0]["state"], "rejected");
    // What its copy changed is written under elfie-requests, and nothing of it is merged into
    // the change set's copy.
    // @lfy def/cli/main.lfy:compile#compile:compile:6e192fbbaa3d541cc1fa9490c15245bc628f83977d91727e7004d801f77b9104
    assert!(fixture.read("elfie-requests/a.rejected/out/a.rs").contains("pub struct A"));
    let copy = crate::copies::copy_of(&fixture.root, "rust");
    assert!(!copy.join("out/a.rs").exists(), "the unit's copy was merged");
    // The finished line lists the unit that did not complete.
    // @lfy def/cli/main.lfy:compile#compile:compile:284d09f3b4a49da7c8306079f44a6a97f2de5d712f9c7306fcf43a6d5fac53b9
    assert!(log.contains("    did not complete: a"), "{log}");
}

/// A verifier that answered for nothing it was asked about is run once more; its problems
/// are then printed as a failed line and the unit is verified with the reviews parsed from
/// that run as if its report had been complete. A run that answered for every id it was
/// asked about stands as it is, however it ended.
// @lfy def/cli/main.lfy:compile#compile:compile:dbcb80625881c43a2f36b95af5c8519113c0f1736c3c3171e2e3f55e426bd6d7
// @lfy def/cli/main.lfy:compile#compile:compile:85f1d3ef649f3839f3e1f7f75cc2c0c880e5b1c1b9978b38bddb4b309b264f1b
// @lfy def/cli/main.lfy:compile#compile:compile:4db6841d8a4555959bbe4c0ce1755f3815ad577d52bc801c77c3a370bc79872e
// @lfy def/cli/main.lfy:compile#compile:compile:5a007ec86d1ce702345b9b34c99744c4d8993ccab14f803a47d7740f0c6efc9c
#[test]
fn a_verifier_whose_report_does_not_end_is_run_once_more() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, NOTHING_REVIEWED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2, "the verifier ran twice");
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran once");
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/main.lfy:compile#compile:compile:85f1d3ef649f3839f3e1f7f75cc2c0c880e5b1c1b9978b38bddb4b309b264f1b
    assert!(log.contains("✗ failed"), "{log}");
    // @lfy def/cli/main.lfy:compile#compile:compile:4db6841d8a4555959bbe4c0ce1755f3815ad577d52bc801c77c3a370bc79872e
    assert!(log.contains("0 satisfied, 0 violated, 1 unverifiable"), "{log}");
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    let stood = Fixture::new();
    a_compiler_and_a_verifier(&stood, UNENDED);
    assert_eq!(stood.run(&["compile"]), ExitCode::Success.code());
    assert_eq!(lines_of(&stood, "verifications.txt"), 1, "the verifier ran once");
    let log = stood.read("elfie-requests/compile.log");
    assert!(!log.contains("✗ failed"), "{log}");
    assert!(log.contains("1 satisfied, 0 violated, 0 unverifiable"), "{log}");
    // What it wrote beside its reviews follows its reviewed line.
    // @lfy def/cli/main.lfy:compile#compile:compile:5a007ec86d1ce702345b9b34c99744c4d8993ccab14f803a47d7740f0c6efc9c
    assert!(log.contains("the report did not end"), "{log}");
    assert!(stood.read("elfie-requests/a.reviews.json").contains("covered by a test"));
}

/// A run that answered for every id it was asked about stands, whatever else it wrote.
// @lfy def/cli/main.lfy:compile#compile:compile:7fbd8e92981ac0bfb3a6651b7e7bf907ebdd945ef7a5649a3b4a8be0fe8c6fcb
#[test]
fn a_review_that_ended_stands_and_what_else_it_wrote_is_printed() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED_WITH_A_STRAY_OBJECT);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // @lfy def/cli/main.lfy:compile#compile:compile:7fbd8e92981ac0bfb3a6651b7e7bf907ebdd945ef7a5649a3b4a8be0fe8c6fcb
    assert_eq!(lines_of(&fixture, "verifications.txt"), 1, "the verifier ran once");
    let log = fixture.read("elfie-requests/compile.log");
    assert!(log.contains("1 satisfied, 0 violated, 0 unverifiable"), "{log}");
    assert!(log.contains("I also looked at the tests"), "{log}");
    assert!(!log.contains("✗ failed"), "{log}");
}

/// Every line of the verifier's output reaches the log, the lines it wrote to standard
/// error among them, each with the prefix it was shown with and no escape.
// @lfy def/cli/main.lfy:compile#compile:compile:aa66eb73d63beed141bc36accf7ad7dd3920fb56c5500f6e4295a50ad7ba7b37
// @lfy def/cli/main.lfy:compile#compile:compile:2e0dbfd080f3b449aa5c3bcb1573c50971d381975307a1b40917dd84d98a4a42
#[test]
fn every_line_the_verifier_wrote_reaches_the_log() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED_WITH_NOISE);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let log = fixture.read("elfie-requests/compile.log");
    // Prefixed by two spaces, the unit, a space, a bar, and a space.
    // @lfy def/cli/main.lfy:compile#compile:compile:2e0dbfd080f3b449aa5c3bcb1573c50971d381975307a1b40917dd84d98a4a42
    assert!(log.contains("  a \u{2502} reading the outputs"), "{log}");
    assert!(log.contains("  a \u{2502} ELFIE: REVIEWED"), "{log}");
    assert!(log.contains("  a \u{2502} ELFIE: DONE"), "{log}");
    assert!(!log.contains('\u{1b}'), "the log holds an escape");
    // Nothing that was never printed reaches the log: no heading over a report, and no
    // second copy of one.
    // @lfy def/cli/main.lfy:main#main:main:3082965661f2495d1e5e9d8a2177fc2ca759320db1b83c7e7bc0605b9be65f3f
    assert!(!log.contains("--- the "), "{log}");
    assert_eq!(log.lines().filter(|line| line.ends_with("ELFIE: REVIEWED")).count(), 1, "{log}");
}

/// With --no-verify no verifier runs, nothing is reviewed, no reviews file is written, and
/// the unit is done and its source maps recorded when it is accepted.
// @lfy def/cli/main.lfy:compile#compile:compile:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
// @lfy def/cli/main.lfy:compile#compile:compile:55b0f231497b975a7352cc15265c4295dfcd3153f5a093823323dc778bae3913
#[test]
fn no_verify_runs_no_verifier() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, VIOLATED);
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran once");
    assert!(!fixture.root.join("verifications.txt").exists(), "no verifier ran");
    assert!(!fixture.root.join("elfie-requests/a.reviews.json").exists(), "nothing is reviewed");
    // @lfy def/cli/main.lfy:compile#compile:compile:55b0f231497b975a7352cc15265c4295dfcd3153f5a093823323dc778bae3913
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    let plain = Fixture::new();
    a_compiler_and_a_verifier(&plain, VIOLATED).write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    assert_eq!(plain.run(&["compile"]), ExitCode::Success.code());
    assert!(!plain.root.join("verifications.txt").exists());
    assert!(!plain.root.join("elfie-requests/a.reviews.json").exists());
}

/// Verify runs the verifier on the outputs already recorded and compiles nothing.
// @lfy def/cli/main.lfy:verify#verify:verify:57c501f4ac1d2297555bfaa08f4fb4c51e6c4d22db2a4b047c43c8a3e2c631e2
// @lfy def/cli/main.lfy:verify#verify:verify:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
// @lfy def/cli/main.lfy:verify#verify:verify:f6ecc4facc68022e7002a7e92ab9ba501e6cdd6242d9d87adb8b05ae09c3533f
#[test]
fn verify_reviews_what_is_recorded_without_compiling() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1);
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    let _ = fs::remove_file(fixture.root.join("elfie-requests/a.reviews.json"));
    forget_reviews(&fixture);
    assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "no compiler is run");
    let reviews = fixture.read("elfie-requests/a.reviews.json");
    assert!(reviews.contains("covered by a test"), "{reviews}");
    write_verifier(&fixture, VIOLATED);
    assert_eq!(fixture.run(&["verify"]), ExitCode::Problems.code());
    fixture.write("def/b.lfy", "d B {}\n");
    write_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["verify", "b"]), ExitCode::Success.code());
    assert!(!fixture.root.join("elfie-requests/b.reviews.json").exists(), "nothing was reviewed for b");
}

/// Verify on a unit already recorded, whose review is violated, prints that review and the
/// counts, writes the reviews, runs no compiler, and returns problems.
// @lfy def/cli/main.lfy:verify#verify:verify:e4a29778d03a36ce29b6e328c9eb23b560807ae3ccc865de9f31b3507e0a6540
#[test]
fn verify_returns_problems_for_a_violated_review() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert!(fixture.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    let _ = fs::remove_file(fixture.root.join("elfie-requests/a.reviews.json"));
    forget_reviews(&fixture);
    write_verifier(&fixture, VIOLATED_IN_SRC);
    // @lfy def/cli/main.lfy:verify#verify:verify:e4a29778d03a36ce29b6e328c9eb23b560807ae3ccc865de9f31b3507e0a6540
    let (code, out, _) = fixture.printed(&["verify", "a", "--color=never"]);
    assert_eq!(code, ExitCode::Problems.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "no compiler is run");
    assert!(
        out.lines().any(|line| line.starts_with("violated ") && line.ends_with(" def/a.lfy:3 A: returns 0 for an empty list")),
        "{out}"
    );
    assert!(out.contains("0 satisfied, 1 violated, 0 unverifiable"), "{out}");
    let reviews = fixture.read("elfie-requests/a.reviews.json");
    assert!(reviews.contains("returns 0 for an empty list"), "{reviews}");
}

/// With no verifier named, the review request of each requested unit is written to
/// `elfie-requests/<name>.review.md`, and the code is success.
// @lfy def/cli/main.lfy:verify#verify:verify:4433e487958c0421f6e9121d7dec547034bf56c187a29d2478a9f228caba4eb4
// @lfy def/cli/main.lfy:verify#verify:verify:b6938589b155e207c8b6d2d220a8d8726be7f451d3867f2a1d037c89951d9923
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
    let (program, _) = plan_with(&fixture.root, &[], &[], &[], &[]);
    program.criteria.first().map(|criterion| criterion.id.clone()).unwrap_or_else(|| panic!("no global criterion"))
}

/// The same project with one global criterion, so a global review has something to review,
/// and a compiler whose output answers for it.
// @lfy def/cli/main.lfy:compile
fn with_a_global_criterion<'f>(fixture: &'f Fixture, verifier: &str) -> &'f Fixture {
    a_compiler_and_a_verifier(fixture, SATISFIED);
    fixture.write(
        "def/a.lfy",
        "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\nglobal@acceptanceCriteria.add({ behavior = `Every output builds` });\n",
    );
    let global = global_id(fixture);
    fixture.write(
        "compiler.sh",
        &noted(
            fixture,
            &format!(
                "#!/bin/sh\ncat >> '<notes>/instructions.txt'\necho compile >> '<notes>/attempts.txt'\nmkdir -p \"$ELFIE_ROOT/out\"\nprintf '// @lfy def/a.lfy:A#{global}\\npub struct A {{}}\\n' > \"$ELFIE_ROOT/out/a.rs\"\necho 'ELFIE: DONE'\n"
            ),
        ),
    );
    let script = noted(fixture, verifier).replace("<id>", &criterion_id(fixture)).replace("<global>", &global);
    fixture.write("verifier.sh", &script)
}

/// A verifier that answers for the global criterion when it is run for the global review
/// and for the criterion of A otherwise, with the status each is given, noting each run's
/// ELFIE_UNIT and where it ran.
fn branching_verifier(global: &str, local: &str) -> String {
    format!(
        "#!/bin/sh\ncat > /dev/null\necho \"$ELFIE_UNIT\" >> '<notes>/verifications.txt'\necho \"$ELFIE_UNIT=$ELFIE_ROOT\" >> '<notes>/where.txt'\nif [ \"$ELFIE_UNIT\" = global ]; then\n  echo '{{\"id\":\"<global>\",\"status\":\"{global}\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"every output builds\"}}'\nelse\n  echo '{{\"id\":\"<id>\",\"status\":\"{local}\",\"evidence\":\"out/a.rs:1-2\",\"note\":\"covered by a test\"}}'\nfi\necho 'ELFIE: REVIEWED'\n"
    )
}

/// Every global criterion is reviewed once for the whole program after every unit has
/// passed its final gate, in the copy of the one change set the compile works, and the
/// review is written with the hash of the ids it answered for.
// @lfy def/cli/main.lfy:compile#compile:compile:0b09728c30bb0edb3d089358237c8d4a4b44e18393eb5824b8af1001750100e0
// @lfy def/cli/main.lfy:compile#compile:compile:67b4fcdb83c0d4fb7760fbaab79b47962fe177863768224d0870c81726f1cc08
// @lfy def/cli/main.lfy:compile#compile:compile:14716d93e16448c0464d1b36613a20d644d039670aec988719076afec4cd8213
// @lfy def/cli/main.lfy:compile#compile:compile:73c389b755f64b116aefe36d13a3dbbe224b401808c21b5c6e58de07a4b1f1d6
// @lfy def/cli/main.lfy:compile#compile:compile:033aa06dcbf3008dd1606e0361145d7bd0386bb1efe3b055336c5e754f6c7eeb
// @lfy def/cli/main.lfy:compile#compile:compile:5ba968cf4e110e45b045c0bebeafc56f21cb68fcd6d5e2652f7155b49bb57385
#[test]
fn a_global_criterion_is_reviewed_once_after_every_unit() {
    let fixture = Fixture::new();
    with_a_global_criterion(&fixture, &branching_verifier("satisfied", "satisfied"));
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // The verifier ran once for the unit and once for the global review, with ELFIE_UNIT set
    // to global for the second.
    // @lfy def/cli/main.lfy:compile#compile:compile:73c389b755f64b116aefe36d13a3dbbe224b401808c21b5c6e58de07a4b1f1d6
    assert_eq!(fixture.read("verifications.txt"), "a\nglobal\n");
    // The unit's verifier ran in the unit's own copy, and the global review in the change
    // set's copy.
    // @lfy def/cli/main.lfy:compile#compile:compile:14716d93e16448c0464d1b36613a20d644d039670aec988719076afec4cd8213
    // @lfy def/cli/main.lfy:compile#compile:compile:649dfaca80ede5a5de3cfecd48f1de664e250e9bc65773266822f3f63ae34c71
    for line in fixture.read("where.txt").lines() {
        match line.split_once('=') {
            Some(("global", at)) => assert!(at.ends_with("elfie-compile/cache/changes/rust/copy"), "{line}"),
            _ => assert!(line.ends_with("elfie-compile/cache/changes/rust/units/a"), "{line}"),
        }
    }
    let log = fixture.read("elfie-requests/compile.log");
    assert!(log.contains("▸ globalVerifying"), "{log}");
    assert!(log.contains("✓ globalReviewed"), "{log}");
    // The global review came after the final gate and before the merge.
    let at = |text: &str| log.find(text).unwrap_or_else(|| panic!("no {text} in {log}"));
    assert!(at("finalGate") < at("globalVerifying") && at("globalReviewed") < at("merging"), "{log}");
    // @lfy def/cli/main.lfy:compile#compile:compile:033aa06dcbf3008dd1606e0361145d7bd0386bb1efe3b055336c5e754f6c7eeb
    let written = fixture.read("elfie-requests/global.reviews.json");
    let value: serde_json::Value = serde_json::from_str(&written).unwrap();
    let (program, _) = plan_with(&fixture.root, &[], &[], &[], &[]);
    assert_eq!(value["requirements"], GlobalReview::requirements_of(&program));
    assert_eq!(value["reviews"][0]["id"], global_id(&fixture));
    assert_eq!(value["reviews"][0]["status"], "satisfied");
    assert!(violated_ids(&fixture.root).is_empty());
    // The second compile has nothing to merge and the ids it answered for did not change, so
    // no global review runs again.
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert_eq!(fixture.read("verifications.txt"), "a\nglobal\n");
}

/// With no global criterion and no global test, no global review runs and no file is
/// written, by a compile or by a verify.
// @lfy def/cli/main.lfy:compile#compile:compile:f033aae0b13c3e667e25d9d067a7b58b8f438e3b2854dcb127d56baca1010a38
#[test]
fn no_global_criterion_means_no_global_review() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
    let log = fixture.read("elfie-requests/compile.log");
    assert!(!log.contains("globalVerifying"), "{log}");
    assert_eq!(lines_of(&fixture, "verifications.txt"), 1);
    // @lfy def/cli/main.lfy:verify#verify:verify:9fc0da16b4fb4241e8c2e859851d9e3fb85f014aa0a2f7a997decf40c5da9d86
    assert_eq!(fixture.run(&["verify", "--full"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
    assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a file was written");
    assert_eq!(fixture.run(&["verify", "--global"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2);
}

/// A violated global review plans the units whose markers answered for it with reason
/// violated and runs them again in the same compile, then the final gates and the global
/// review once more; a second violated review stops the compile with the code problems,
/// every change set kept, and the next compile plans those units with reason violated.
// @lfy def/cli/main.lfy:compile#compile:compile:1435376dfcd2c7ecb8f1966cb1ce291a33eaded750ea02d6c8b936baa000130c
// @lfy def/cli/main.lfy:compile#compile:compile:afe98c81a441acd8328abb1bd1e4139cb546203a7431be38e71303cf11efb5ea
// @lfy def/cli/main.lfy:compile#compile:compile:7151ea4a586484bae8988155ff4f9c3c2522dc8682ecefe1fbbe3d5ad7a87ec4
// @lfy def/cli/main.lfy:compile#compile:compile:96dd6a88ebe0003d78a67421241c801e1f0e0faf8ced5a63a7825b67a31a2729
// @lfy def/cli/main.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
// @lfy def/cli/main.lfy:compile#compile:compile:4df83fe74548903b3edfa0c021389b44e555ff2b79a5e259ea30564dbc9dca2c
#[test]
fn a_violated_global_review_plans_its_units_once_more() {
    let fixture = Fixture::new();
    with_a_global_criterion(&fixture, &branching_verifier("violated", "satisfied"));
    // @lfy def/cli/main.lfy:compile#compile:compile:96dd6a88ebe0003d78a67421241c801e1f0e0faf8ced5a63a7825b67a31a2729
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    // The unit ran twice and the global review ran after each; the second time its output
    // was the same, so the verdict on its own criterion was carried and no verifier ran on it.
    // @lfy def/cli/main.lfy:compile#compile:compile:afe98c81a441acd8328abb1bd1e4139cb546203a7431be38e71303cf11efb5ea
    assert_eq!(fixture.read("verifications.txt"), "a\nglobal\nglobal\n");
    assert_eq!(lines_of(&fixture, "attempts.txt"), 2, "the unit was compiled once more");
    let log = fixture.read("elfie-requests/compile.log");
    // @lfy def/cli/main.lfy:compile#compile:compile:4df83fe74548903b3edfa0c021389b44e555ff2b79a5e259ea30564dbc9dca2c
    assert!(log.contains("    - failure at global: every output builds (out/a.rs:1-2)"), "{log}");
    assert_eq!(log.matches("▸ finalGate").count(), 2, "{log}");
    // The root took nothing, and the change set is kept.
    // @lfy def/cli/main.lfy:compile#compile:compile:7151ea4a586484bae8988155ff4f9c3c2522dc8682ecefe1fbbe3d5ad7a87ec4
    assert!(!log.contains("▸ merging"), "{log}");
    assert!(!fixture.root.join("out/a.rs").exists(), "the root took a change set");
    assert!(ChangeSet::load(&fixture.root, "rust").is_some(), "the change set was not kept");
    // The next compile plans the unit with reason violated.
    // @lfy def/cli/main.lfy:compile#compile:compile:4def42b297ccd56ad7dacce3f7bd8d7b36e18397d3d99e4b2f198aa3ea0f16c8
    let ids = violated_ids(&fixture.root);
    assert!(ids.contains(&global_id(&fixture)), "{ids:?}");
    let (_, out, _) = fixture.printed(&["compile", "--dry-run", "--color=never"]);
    assert!(out.lines().any(|line| line.trim() == "rust a violated []"), "{out}");
}

/// A violated global review in a compile that runs no compiler, because none is named or
/// because of --accept, makes no change set and runs no compiler: the code is problems, and
/// the next compile plans the unit with reason violated.
// @lfy def/cli/main.lfy:compile#compile:compile:d5f565b191777716fd8e847870720c599c23591f1d959c0cb67397a6770f7536
// @lfy def/cli/main.lfy:compile#compile:compile:0440d8388eff8352220c16c7b7cbcac23c06b46e2e0c8882f55b1629dc83a76a
// @lfy def/cli/main.lfy:compile#compile:compile:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
// @lfy def/cli/main.lfy:compile#compile:compile:3d1e2128231c0dd4a623216ed6845b9bfdce55b3a624742046eb528f6cff68dd
#[test]
fn a_violated_global_review_without_a_compiler_makes_no_change_set() {
    let fixture = Fixture::new();
    with_a_global_criterion(&fixture, &branching_verifier("violated", "satisfied"));
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    // --accept: the compiler is named but runs no more.
    // @lfy def/cli/main.lfy:compile#compile:compile:c51341f34278d51ccda0b1a75908b211f46b7db736525ed29c88351db922c346
    // @lfy def/cli/main.lfy:compile#compile:compile:0440d8388eff8352220c16c7b7cbcac23c06b46e2e0c8882f55b1629dc83a76a
    assert_eq!(fixture.run(&["compile", "--accept"]), ExitCode::Problems.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran");
    let made = || ChangeSet::load(&fixture.root, "rust").is_some() || fixture.root.join("elfie-compile/cache/changes/rust").exists();
    assert!(!made(), "a change set was made");
    // No compiler named, and the global review forgotten so that it runs again.
    // @lfy def/cli/main.lfy:compile#compile:compile:d5f565b191777716fd8e847870720c599c23591f1d959c0cb67397a6770f7536
    fs::remove_file(fixture.root.join("elfie-requests/global.reviews.json")).unwrap();
    fixture.write("elfie.json", &manifest(&[("verifier", "sh verifier.sh")]));
    assert_eq!(fixture.run(&["compile"]), ExitCode::Problems.code());
    assert_eq!(lines_of(&fixture, "attempts.txt"), 1, "the compiler ran");
    assert!(!made(), "a change set was made");
    let (_, out, _) = fixture.printed(&["compile", "--dry-run", "--color=never"]);
    assert!(out.lines().any(|line| line.trim() == "rust a violated []"), "{out}");
}

/// A compile with nothing to do still begins with planned and ends with finished, even
/// when a global review runs between them, in the root, because no global review was ever
/// recorded.
// @lfy def/cli/main.lfy:compile#compile:compile:3b7ff1cb8c57ebc94c72273c9e46802fa9a2881815174d5e72e44b93e5ad00ea
// @lfy def/cli/main.lfy:compile#compile:compile:4da6fc0ef0b92bdd309365b39295a5cc5de6e9d3296556c8da4e7fc7e1c11920
// @lfy def/cli/main.lfy:compile#compile:compile:7c6f5e6de7536e86a256b2b8f3baa8bd24a1238786e81140e15d4950be8f911a
// @lfy def/cli/main.lfy:compile#compile:compile:3e779a9b6f0378e44c4ad48cec920b4a7cc005d5fba9f728bcdb2b295a89a2e6
#[test]
fn a_compile_with_nothing_to_do_still_begins_planned_and_ends_finished() {
    let fixture = Fixture::new();
    with_a_global_criterion(&fixture, &branching_verifier("satisfied", "satisfied"));
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    fs::remove_file(fixture.root.join("elfie-requests/compile.log")).unwrap();
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let log = fixture.read("elfie-requests/compile.log");
    assert!(log.contains("[0/0]"), "{log}");
    assert!(log.contains("▸ globalVerifying"), "{log}");
    // It ran in the root, since the compile works no change set.
    // @lfy def/cli/main.lfy:compile#compile:compile:7c6f5e6de7536e86a256b2b8f3baa8bd24a1238786e81140e15d4950be8f911a
    assert_eq!(fixture.read("where.txt").trim(), format!("global={}", fixture.path()));
    let mut lines = log.lines().filter(|line| !line.trim().is_empty());
    assert!(lines.next().is_some_and(|first| first.contains("◆ planned")), "{log}");
    assert!(lines.next_back().is_some_and(|last| last.contains("◆ finished")), "{log}");
}

/// Verify with --global and no stem reviews no unit, runs the verifier once for the global
/// criteria and tests, and writes the global review; with no stem it reviews every unit
/// first.
// @lfy def/cli/main.lfy:verify#verify:verify:2807195e43f8abc32d92c01234487814a095565d2b9a42d8da80b6f24a665bb6
// @lfy def/cli/main.lfy:verify#verify:verify:d91e473bb0065ff6b530621d2d948f943c39c400da2fd41ae66086d11c34d2cd
// @lfy def/cli/main.lfy:verify#verify:verify:32387edd44a4cc7c91efe2a37b89320c59bfef52abddb28e5661c9df8f68b514
#[test]
fn verify_global_reviews_no_unit() {
    let fixture = Fixture::new();
    with_a_global_criterion(&fixture, &branching_verifier("satisfied", "satisfied"));
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    assert!(!fixture.root.join("verifications.txt").exists(), "no verifier ran");
    assert_eq!(fixture.run(&["verify", "--global"]), ExitCode::Success.code());
    assert_eq!(fixture.read("verifications.txt"), "global\n");
    assert!(!fixture.root.join("elfie-requests/a.reviews.json").exists(), "a unit was reviewed");
    let written = fixture.read("elfie-requests/global.reviews.json");
    assert!(written.contains(&global_id(&fixture)), "{written}");
    // Nothing under elfie-compile is recorded, moved, or removed by verify.
    // @lfy def/cli/main.lfy:verify#verify:verify:64bc2fc6236a9783f1939c6823437a0fee24fced7f789dc76b7dd418a1260f99
    let before = fixture.read("elfie-compile/maps/rust/a.json");
    assert_eq!(fixture.run(&["verify"]), ExitCode::Success.code());
    assert_eq!(fixture.read("elfie-compile/maps/rust/a.json"), before);
    assert_eq!(fixture.read("verifications.txt"), "global\na\nglobal\n");
    let script = noted(&fixture, &branching_verifier("violated", "satisfied"))
        .replace("<id>", &criterion_id(&fixture))
        .replace("<global>", &global_id(&fixture));
    fixture.write("verifier.sh", &script);
    assert_eq!(fixture.run(&["verify", "--global"]), ExitCode::Problems.code());
}
