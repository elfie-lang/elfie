// @lfy def/cli/main.lfy:main
//! Tests of running the compiler and the verifier through one hosted session: the binding it
//! is opened with, the environment it is handed, the outcome read back from it, and the
//! review records a run leaves behind.

use super::*;
use crate::copies::Where;
use crate::processes::{Agent, Runs, Server};
use crate::running::Run;
use crate::schedule::{Round, Schedule};

/// What a host made by hand recorded: the binding of every session it opened, the URL of
/// every session it closed, and how many times it was stopped.
// @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
struct Recorded {
    opened: Arc<Mutex<Vec<Binding>>>,
    closed: Arc<Mutex<Vec<String>>>,
    stopped: Arc<Mutex<usize>>,
}

/// A host made by hand, which records what it is asked, offers the tool names given, and
/// gives back the record given when a session is closed.
// @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
fn a_host(record: SessionRecord, offers: Vec<String>) -> (Host, Recorded) {
    let opened: Arc<Mutex<Vec<Binding>>> = Arc::new(Mutex::new(Vec::new()));
    let closed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let stopped: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let recorded =
        Recorded { opened: opened.clone(), closed: closed.clone(), stopped: stopped.clone() };
    let host = Host {
        url: "http://127.0.0.1:7/".to_string(),
        token: "a-token".to_string(),
        open: Box::new(move |binding| {
            let mut held = opened.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            held.push(binding);
            format!("http://127.0.0.1:7/sessions/{}", held.len())
        }),
        // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
        offered: Box::new(move |_| offers.clone()),
        close: Box::new(move |url| {
            closed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(url.to_string());
            record.clone()
        }),
        stop: Box::new(move || {
            *stopped.lock().unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        }),
    };
    (host, recorded)
}

/// An agent whose agent server is the one given, or the failure given, rather than one
/// hosted for real.
// @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
fn an_agent(root: &Path, hosted: Result<Host, String>) -> Agent {
    Agent {
        json: false,
        style: Paint { out: false, err: false },
        logs: Vec::new(),
        elfie: PathBuf::from("elfie"),
        // @lfy def/cli/main.lfy:main#main:main:5724f5824450ab53811f66b437b4a0ea4d89fea52c17be8df51e351a55ae8fe9
        server: Arc::new(Server {
            root: root.to_path_buf(),
            hosted: Mutex::new(Some(hosted.map(Arc::new))),
        }),
    }
}

/// One compile of the fixture's one unit, locked, so that a run of an agent can be made
/// by hand.
// @lfy def/cli/main.lfy:main
fn a_run(fixture: &Fixture) -> Run {
    let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
    let invocation = parse_arguments(&args(&["compile", "--color=never"])).unwrap();
    let reporter = Reporter::new(false, Paint::of(&invocation), 1, Vec::new());
    Run::new(program, plan, Vec::new(), reporter, &invocation)
}

/// Every run of the compiler or the verifier works through one hosted session of the
/// agent server: it is opened just before the run starts, bound to what the run is, its
/// config file is written under the root and named by ELFIE_MCP_CONFIG, the command tools
/// it is offered are named by ELFIE_COMMAND_TOOLS, and it is closed once the run has
/// returned, giving back what the run submitted through it, its config file removed.
// @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
// @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
// @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
// @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
// @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
// @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
// @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
#[test]
fn a_run_of_an_agent_works_through_one_hosted_session() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let run = a_run(&fixture);
    let batch = run.plan.batches[0].clone();
    let submitted = Review {
        id: "A:A:0".to_string(),
        file: "def/a.lfy".to_string(),
        line: 1,
        entity: "A".to_string(),
        status: ReviewStatus::Satisfied,
        evidence: "out/a.rs:1-2".to_string(),
        note: "covered by a test".to_string(),
    };
    let record = SessionRecord {
        reviews: vec![submitted.clone()],
        submission: Some(Submission { outcome: SubmittedOutcome::Done, message: String::new() }),
    };
    // The server lists more than the command tools, and lists them in its own order.
    // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    let offers = ["elfie_task", "elfie_test", "elfie_build", "elfie_mark", "elfie_lint"]
        .map(str::to_string)
        .to_vec();
    let (host, recorded) = a_host(record, offers);
    let agent = an_agent(&fixture.root, Ok(host));
    let where_ = Where::Root(fixture.root.clone());
    // The compiler of a batch: its binding has the role compiler, the batch, the
    // directory the run is in, the plan and the program the command made, the request its
    // input was made from, no review, and no source maps.
    // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
    let request = run.request_of(&batch, &fixture.root, true, None);
    let binding =
        run.binding_of(Role::Compiler, Some(&batch), &fixture.root, Some(request.clone()), None, Vec::new());
    let exit = agent.run(
        "printf '%s\\n' \"$ELFIE_MCP_CONFIG\" \"$ELFIE_COMMAND_TOOLS\" \"$ELFIE_SESSION\" \
         \"${ELFIE_RESUME-none}\" > config.txt\n\
         cp \"$ELFIE_MCP_CONFIG\" written.json",
        &where_,
        &batch.identifier,
        "a",
        "",
        binding,
        None,
        None,
    );
    assert_eq!(exit.code, 0, "{}", exit.stderr);
    // ELFIE_SESSION is an agent session id new for the run: a version 4 UUID from the
    // operating system's secure random source, 36 lowercase characters in the 8-4-4-4-12
    // form, and a run that is no resuming run carries no ELFIE_RESUME at all.
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
    let written: Vec<String> = fixture.read("config.txt").lines().map(str::to_string).collect();
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    assert_eq!(written[2], exit.session, "the run was told its own session id");
    assert!(is_a_uuid(&exit.session), "{} is no version 4 UUID", exit.session);
    // @lfy def/cli/main.lfy:main#main:main:531ac1da0d507163b34851e9aa9246359825d051bd4cfdd44097efcf8cc03dcf
    assert_eq!(written[3], "none", "ELFIE_RESUME was in the environment");
    // One session was opened for the run, bound to what it is.
    // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
    let session = {
        let opened = recorded.opened.lock().unwrap();
        assert_eq!(opened.len(), 1);
        // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
        assert_eq!(opened[0].role, Role::Compiler);
        assert_eq!(opened[0].batch.as_ref().map(|batch| batch.identifier.clone()), Some("a".to_string()));
        // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
        assert_eq!(opened[0].directory, fixture.root);
        assert_eq!(opened[0].plan, run.plan);
        assert_eq!(opened[0].program, run.program);
        // The request the run's input was made from, and nothing a verifier is given.
        // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
        assert_eq!(
            opened[0].request.as_ref().map(|request| request.instructions.clone()),
            Some(request.instructions.clone()),
        );
        // @lfy def/cli/main.lfy:main#main:main:49b8e07b5de76a70728002b1123896a9bea926e67c9452ff6c2c821e84281a04
        assert_eq!(opened[0].review, None);
        assert_eq!(opened[0].source_maps, []);
        "http://127.0.0.1:7/sessions/1".to_string()
    };
    // Its config file is written under the root, named by the batch and the role, and
    // ELFIE_MCP_CONFIG in its environment is the absolute path of it; ELFIE_COMMAND_TOOLS
    // is the command tools it was offered, in the order the server listed them.
    // @lfy def/cli/main.lfy:main#main:main:db24670f7a47cc1d0d13a51de65a69c1469315a12c8359b5ba6d6f0d620ded13
    // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    let path = fixture.root.join("elfie-requests/a.compiler.mcp.json");
    let environment = fixture.read("config.txt");
    let mut lines = environment.lines();
    assert_eq!(lines.next(), Some(path.to_string_lossy().as_ref()));
    assert!(path.is_absolute(), "{path:?}");
    // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    assert_eq!(lines.next(), Some("elfie_test elfie_build elfie_lint"));
    // It held one object naming the session's URL and the bearer token, and is gone once
    // the run has returned, since its token no longer opens anything.
    // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
    let value: serde_json::Value = serde_json::from_str(&fixture.read("written.json")).unwrap();
    let server = &value["mcpServers"]["elfie"];
    // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
    assert_eq!(server["type"], "http");
    assert_eq!(server["url"], session);
    assert_eq!(server["headers"]["Authorization"], "Bearer a-token");
    // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
    assert!(!path.exists(), "the config file is still there");
    // The session was closed once the run returned, and what it submitted came back with
    // its output.
    // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
    assert_eq!(*recorded.closed.lock().unwrap(), vec![session]);
    // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
    assert_eq!(exit.record.reviews, vec![submitted]);
    // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
    assert_eq!(exit.record.submission.map(|submission| submission.outcome), Some(SubmittedOutcome::Done));
    // The server is stopped once, however often it is asked to stop.
    // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    agent.server.stop();
    agent.server.stop();
    assert_eq!(*recorded.stopped.lock().unwrap(), 1);
}

/// How a compiler run ended is read from the outcome it submitted through its session,
/// not from its report: a run that submitted `blocked` is blocked, with the submission's
/// message as its reason, though its report says nothing at all.
// @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
#[test]
fn the_outcome_of_a_compiler_run_is_read_from_what_it_submitted() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let mut run = a_run(&fixture);
    let batch = run.plan.batches[0].clone();
    // The run submits blocked through its session and writes no report of its own.
    // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
    let record = SessionRecord {
        reviews: Vec::new(),
        submission: Some(Submission {
            outcome: SubmittedOutcome::Blocked,
            message: "two criteria of A conflict".to_string(),
        }),
    };
    let (host, _recorded) = a_host(record, Vec::new());
    run.agent = an_agent(&fixture.root, Ok(host));
    let run = Mutex::new(run);
    let where_ = Where::Root(fixture.root.clone());
    let runs = {
        let mut state = locked(&run);
        let agent = state.agent.clone();
        Runs::new(agent, state.running_of(&batch.identifier))
    };
    let planned = vec![a_planned_batch(&batch.identifier, 0, Vec::new())];
    let schedule = Mutex::new(Schedule::of(&planned));
    let finished = Condvar::new();
    let round = Round { schedule: &schedule, finished: &finished, planned: &planned, at: 0, copies: false };
    // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
    let compiled = Run::compile_batch(&run, &batch, "true", &where_, &runs, None, &round);
    // The outcome is blocked: nothing is held to record, and the reason the submission
    // carried is written out.
    // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
    assert!(compiled.held.is_none(), "the batch was not blocked");
    // @lfy def/cli/main.lfy:main#main:main:37a3061e2d1da4becaf9fd160790f16ed84d77d4efa8b867622b0546149e5262
    let note = fixture.read("elfie-requests/a.blocked.md");
    assert!(note.contains("two criteria of A conflict"), "{note}");
}

/// The verifier of a batch is bound to the review request it was given and the source
/// maps that request was made with; the verifier of the global review to no batch.
// @lfy def/cli/main.lfy:main#main:main:d7fdc89e00db8f2de0886cdaaecf381963abe8e1eb6392c9dee9bc26a30911b5
// @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
#[test]
fn a_verifier_is_bound_to_the_review_it_was_given() {
    let fixture = Fixture::new();
    one_target(&fixture)
        .write("def/a.lfy", "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\n");
    let run = a_run(&fixture);
    let batch = run.plan.batches[0].clone();
    let maps = generation::source_maps_of(&run.program.workspace, None);
    let request = run.review_request_of(&batch, &maps, Some(&fixture.root), &[]);
    // @lfy def/cli/main.lfy:main#main:main:d7fdc89e00db8f2de0886cdaaecf381963abe8e1eb6392c9dee9bc26a30911b5
    let binding =
        run.binding_of(Role::Verifier, Some(&batch), &fixture.root, None, Some(request.clone()), maps.clone());
    assert_eq!(binding.role, Role::Verifier);
    assert_eq!(binding.batch.as_ref().map(|batch| batch.identifier.clone()), Some("a".to_string()));
    // @lfy def/cli/main.lfy:main#main:main:d7fdc89e00db8f2de0886cdaaecf381963abe8e1eb6392c9dee9bc26a30911b5
    assert_eq!(binding.review.as_ref().map(|review| review.asked.clone()), Some(request.asked.clone()));
    assert_eq!(binding.source_maps, maps);
    // The global review is for no batch.
    // @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
    let global = generation::global_review(&run.program, &maps);
    let binding = run.binding_of(Role::Verifier, None, &fixture.root, None, Some(global), maps.clone());
    // @lfy def/cli/main.lfy:main#main:main:8bc948283105a23f893766f7593a57076a62f423d7ff2093b65ea6acb4853711
    assert_eq!(binding.batch, None);
    assert_eq!(binding.role, Role::Verifier);
    assert!(binding.review.is_some());
    assert_eq!(binding.source_maps, maps);
}

/// An agent server that cannot be hosted runs no compiler and no verifier: the run's code
/// is -1 and its failure names the server, which makes the code of the command failure.
// @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
#[test]
fn a_server_that_cannot_be_hosted_runs_nothing() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let run = a_run(&fixture);
    let batch = run.plan.batches[0].clone();
    let agent = an_agent(&fixture.root, Err("no port of 127.0.0.1 can be listened on".to_string()));
    let where_ = Where::Root(fixture.root.clone());
    let binding = run.binding_of(Role::Compiler, Some(&batch), &fixture.root, None, None, Vec::new());
    // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    let exit = agent.run("echo ran > ran.txt", &where_, &batch.identifier, "a", "", binding, None, None);
    // Nothing ran, and the failure is what the run came to; a run whose code is -1 makes
    // the code of the command failure.
    // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    assert_eq!(exit.code, -1);
    assert!(!fixture.root.join("ran.txt").exists(), "the command ran");
    assert!(exit.stderr.contains("the agent server could not be hosted"), "{}", exit.stderr);
    assert!(exit.stderr.contains("no port"), "{}", exit.stderr);
    // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    assert!(!fixture.root.join("elfie-requests/a.compiler.mcp.json").exists(), "a session was opened");
}

// ---- carrying a verdict --------------------------------------------------------

/// A reviewed line counts the reviews of its own run and then the verdicts its review
/// request carried, which are counted there and nowhere else; with nothing carried the
/// counts end with the unverifiable.
// @lfy def/cli/main.lfy:main#main:main:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
// @lfy def/cli/main.lfy:main#main:main:f088a8a0ce2ac66dea4de2f7c633f70122a6f42b582ae35203ed2bea262e67f3
// @lfy def/cli/main.lfy:main#main:main:40a59e64f37d7276f9f2e1c4d1434060c145115922c8218363f3a700cb7f7beb
// @lfy def/cli/main.lfy:main#main:main:01ac2a8e3e988e7c07b647cd40cc38bce72044f98971d9e5e72eb5c62425cc0e
// @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
// @lfy def/cli/main.lfy:main#main:main:5e715c81a6d8d3edeadda85d5a0a8cc96d3f5aa33c9009f7b213ae941ccb4eff
// @lfy def/cli/main.lfy:main#main:main:cc320fe0b40fde7c47dca674e64001b0a974e93cf2ffa2186a7c30bbf7aad8f7
// @lfy def/cli/main.lfy:main#main:main:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
#[test]
fn a_reviewed_line_counts_the_carried_verdicts_last() {
    let satisfied = Review {
        id: "A:A:0".to_string(),
        file: "def/a.lfy".to_string(),
        line: 3,
        entity: "A".to_string(),
        status: ReviewStatus::Satisfied,
        evidence: "out/a.rs:1-2".to_string(),
        note: "covered by a test".to_string(),
    };
    let reviews = [satisfied];
    // Nothing carried: the message begins with the three counts and ends with them.
    // @lfy def/cli/main.lfy:main#main:main:40a59e64f37d7276f9f2e1c4d1434060c145115922c8218363f3a700cb7f7beb
    // @lfy def/cli/main.lfy:main#main:main:5e715c81a6d8d3edeadda85d5a0a8cc96d3f5aa33c9009f7b213ae941ccb4eff
    assert_eq!(
        counts_message(&counts_with_carried(&reviews, 0, 0), false),
        "1 satisfied, 0 violated, 0 unverifiable"
    );
    // Something carried: the count and the word carried follow, after a comma and a
    // space.
    // @lfy def/cli/main.lfy:main#main:main:f088a8a0ce2ac66dea4de2f7c633f70122a6f42b582ae35203ed2bea262e67f3
    // @lfy def/cli/main.lfy:main#main:main:cc320fe0b40fde7c47dca674e64001b0a974e93cf2ffa2186a7c30bbf7aad8f7
    let counts = counts_with_carried(&reviews, 0, 2);
    assert_eq!(counts_message(&counts, false), "1 satisfied, 0 violated, 0 unverifiable, 2 carried");
    // That count and its word are painted muted, and painting leaves the words as they
    // are.
    // @lfy def/cli/main.lfy:main#main:main:01ac2a8e3e988e7c07b647cd40cc38bce72044f98971d9e5e72eb5c62425cc0e
    assert_eq!(counts.last(), Some(&(2, "carried", Tone::Muted)));
    // @lfy def/cli/main.lfy:main#main:main:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
    assert_eq!(width(&counts_message(&counts, true)), width(&counts_message(&counts, false)));
    assert!(counts_message(&counts, true).contains(&tally(2, "carried", Tone::Muted, true)));
    // A carried verdict is counted only as carried: it is neither satisfied, violated,
    // nor unverifiable.
    // @lfy def/cli/main.lfy:main#main:main:f6fa7d85aff0554d36e021b666a456c9cca8d20bda732b39452c619989af78aa
    // @lfy def/cli/main.lfy:main#main:main:2edbac225cec2318ff261227fbb09c7256786df4743f9748be8742962271a3ac
    assert_eq!(
        counts_message(&counts_with_carried(&[], 0, 3), false),
        "0 satisfied, 0 violated, 0 unverifiable, 3 carried"
    );
}

/// A compile records the verdict on each criterion and test of its units, and a review of
/// a batch whose verdicts all still hold asks about nothing: no verifier runs on it, and
/// --full asks about them again.
// @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
// @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
// @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
// @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
// @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
// @lfy def/cli/main.lfy:main#main:main:f79d62c52638caa4cb00a160316bc7052bc62e29caef830e683dba3fca9581ca
#[test]
fn a_settled_verdict_is_carried_and_asks_no_verifier() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // The verdict is recorded in the unit's review record file, with the hash of the
    // regions it was judged against.
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    let records = fixture.read("elfie-compile/reviews/rust/a.json");
    assert!(records.contains(&criterion_id(&fixture)), "{records}");
    assert!(records.contains("satisfied"), "{records}");
    let (program, plan) = plan_with(&fixture.root, &[], &[], &[]);
    let read = generation::reviews_of(&program, &plan.units[0]);
    // @lfy def/cli/main.lfy:main#main:main:9643453ae724b2467947a6cedaf76f572a32cc0ce4c9e739f85fb2cdff7f7b67
    assert_eq!(read.len(), 1, "{read:?}");
    assert_eq!(read[0].status, ReviewStatus::Satisfied);
    assert!(!read[0].hash.is_empty(), "{read:?}");
    // Nothing of its code changed, so a review of its batch carries that verdict, asks
    // about nothing, and runs no verifier.
    // @lfy def/cli/main.lfy:main#main:main:3e172b389c51b861e8618823a7312c5520d7c60bb6b09e85208fc0f70186651d
    // @lfy def/cli/main.lfy:main#main:main:f79d62c52638caa4cb00a160316bc7052bc62e29caef830e683dba3fca9581ca
    assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "verifications.txt"), 1, "the verifier ran again");
    // With --full every criterion and test is asked about again.
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    assert_eq!(fixture.run(&["verify", "a", "--full"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2, "the verifier did not run again");
    // The review request of the batch asks about nothing while the verdict holds, and
    // carries it instead.
    // @lfy def/cli/main.lfy:main#main:main:cf8adcecd483ba999b73ffdeb3f91c23888f965698e95a0a051f183cdc1495f8
    // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
    let run = a_run(&fixture);
    let batch = run.plan.batches[0].clone();
    let maps = generation::source_maps_of(&run.program.workspace, None);
    let kept = run.records_of(&batch);
    assert_eq!(kept.len(), 1, "{kept:?}");
    let request = run.review_request_of(&batch, &maps, None, &kept);
    // @lfy def/cli/main.lfy:main#main:main:b772521c3f6da36b491eeffa1eda27e9d113f955d69a891c943d7df17113910c
    assert_eq!(request.asked, [] as [String; 0], "{:?}", request.asked);
    assert_eq!(request.carried.len(), 1, "{:?}", request.carried);
    // With no records nothing is carried and the criterion is asked about.
    // @lfy def/cli/main.lfy:main#main:main:e7adbac3b03b379146c0e794f2e98b35f841a35f2d9c85af1e88e0811b8add38
    let full = run.review_request_of(&batch, &maps, None, &[]);
    assert_eq!(full.asked.len(), 1, "{:?}", full.asked);
    assert!(full.carried.is_empty(), "{:?}", full.carried);
}

/// With --no-verify, with no verifier named, or with --accept, no review record file
/// changes.
// @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
// @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
#[test]
fn nothing_that_runs_no_verifier_records_a_verdict() {
    // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
    let quiet = Fixture::new();
    a_compiler_and_a_verifier(&quiet, SATISFIED);
    assert_eq!(quiet.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    assert!(quiet.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
    assert!(!quiet.root.join("elfie-compile/reviews").exists(), "a review record was written");
    // A project naming no verifier is the same.
    // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
    let plain = Fixture::new();
    a_compiler_and_a_verifier(&plain, SATISFIED)
        .write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    assert_eq!(plain.run(&["compile"]), ExitCode::Success.code());
    assert!(!plain.root.join("elfie-compile/reviews").exists(), "a review record was written");
    // With --accept the verifier runs and the maps are recorded, but no review record
    // file changes.
    // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    let accepted = Fixture::new();
    a_compiler_and_a_verifier(&accepted, SATISFIED);
    accepted.write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n");
    assert_eq!(accepted.run(&["compile", "--accept"]), ExitCode::Success.code());
    assert!(accepted.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    assert!(!accepted.root.join("elfie-compile/reviews").exists(), "a review record was written");
}

/// A review record file belonging to no unit of the plan is removed before any batch
/// runs, and the file of a unit of the plan is left as it was.
// @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
#[test]
fn a_review_record_file_of_no_unit_is_removed() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let stale = fixture.read("elfie-compile/reviews/rust/a.json");
    // A unit whose definition file was deleted leaves its review record file behind.
    // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
    fixture.write("elfie-compile/reviews/rust/gone.json", &stale);
    fixture.write("def/b.lfy", "d B {}\n");
    fixture.write("compiler.sh", PER_UNIT_COMPILER);
    write_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
    assert!(
        !fixture.root.join("elfie-compile/reviews/rust/gone.json").exists(),
        "the review record file stayed"
    );
    // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
    assert_eq!(fixture.read("elfie-compile/reviews/rust/a.json"), stale, "a.json changed");
    // With --no-verify nothing of the kind is removed, since nothing reviews anything.
    // @lfy def/cli/main.lfy:main#main:main:10e95f2d1f7f50e861876b9a7f871b2512aee9bc61e507f2611ac724627d5af7
    fixture.write("elfie-compile/reviews/rust/gone.json", &stale);
    fixture.write("def/c.lfy", "d C {}\n");
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    assert!(fixture.root.join("elfie-compile/reviews/rust/gone.json").is_file(), "the file was removed");
}

/// A verify whose review request asked nothing is success, as one whose verifier was
/// started and violated nothing is.
// @lfy def/cli/main.lfy:main#main:main:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
#[test]
fn a_verify_that_asked_nothing_is_success() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // Every verdict is carried, so no verifier is started and nothing is violated.
    // @lfy def/cli/main.lfy:main#main:main:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
    assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Success.code());
    // The verifier started and violated nothing: success too.
    // @lfy def/cli/main.lfy:main#main:main:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
    assert_eq!(fixture.run(&["verify", "a", "--full"]), ExitCode::Success.code());
    // A violated review is problems all the same, carried verdicts or not.
    // @lfy def/cli/main.lfy:main#main:main:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
    write_verifier(&fixture, VIOLATED);
    assert_eq!(fixture.run(&["verify", "a", "--full"]), ExitCode::Problems.code());
}
