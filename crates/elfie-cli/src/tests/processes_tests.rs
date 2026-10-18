// @lfy def/cli/main.lfy:main
//! Tests of running the compiler and the verifier through one hosted session: the binding it
//! is opened with, the environment it is handed, the outcome read back from it, and the
//! review records a run leaves behind.

use super::*;
use crate::processes::{Agent, Server};
use crate::running::{Stop, files_in};
use crate::schedule::Ended;

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
    let recorded = Recorded { opened: opened.clone(), closed: closed.clone(), stopped: stopped.clone() };
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
        server: Arc::new(Server { root: root.to_path_buf(), hosted: Mutex::new(Some(hosted.map(Arc::new))) }),
    }
}

/// Every run of the compiler or the verifier works through one hosted session of the agent
/// server: it is opened just before the run starts, bound to what the run is, its config file
/// is written under the root and named by ELFIE_MCP_CONFIG, the command tools it is offered
/// are named by ELFIE_COMMAND_TOOLS, and it is closed once the run has returned, giving back
/// what the run submitted through it, its config file removed.
// @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
// @lfy def/cli/main.lfy:main#main:main:6d22af1c9b1faa80650ee222bf29ec59916fad170472c864b3ea67c7206c69c9
// @lfy def/cli/main.lfy:main#main:main:5b2c8910206c8e75afcd2cd352436a6895414fa9284039b11ef9f9b3c1fb88bb
// @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
// @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
// @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
// @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
// @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
// @lfy def/cli/main.lfy:main#main:main:631eb66f63680435474026b050f06277c5eee4e5b444e767c71386bdbb3c3118
// @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
#[test]
fn a_run_of_an_agent_works_through_one_hosted_session() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/cli/main.lfy", "d A: `An A` {}\n").write("out/x.rs", "x\n");
    let run = a_run(&fixture, &["compile", "--color=never"]);
    let submitted = Review {
        id: "A:A:0".to_string(),
        file: "def/cli/main.lfy".to_string(),
        line: 1,
        entity: "A".to_string(),
        status: ReviewStatus::Satisfied,
        evidence: "out/a.rs:1-2".to_string(),
        note: "covered by a test".to_string(),
    };
    let record = SessionRecord {
        reviews: vec![submitted.clone()],
        submission: Some(Submission { outcome: SubmittedOutcome::Done, message: String::new() }),
        ..SessionRecord::default()
    };
    let offers = ["elfie_task", "elfie_test", "elfie_build", "elfie_mark", "elfie_lint"].map(str::to_string).to_vec();
    let (host, recorded) = a_host(record, offers);
    let agent = an_agent(&fixture.root, Ok(host));
    let (request, _) = run.request_of(0, &fixture.root, true, &[]);
    let before = files_in(run.target_of_unit(0), &fixture.root);
    // @lfy def/cli/main.lfy:main#main:main:6d22af1c9b1faa80650ee222bf29ec59916fad170472c864b3ea67c7206c69c9
    let binding = run.binding_of(
        Role::Compiler,
        Some(0),
        &fixture.root,
        Some(request.clone()),
        Some(before.clone()),
        None,
        Vec::new(),
    );
    let exit = agent.run(
        "printf '%s\\n' \"$ELFIE_MCP_CONFIG\" \"$ELFIE_COMMAND_TOOLS\" \"$ELFIE_SESSION\" \"${ELFIE_RESUME-none}\" \"$ELFIE_UNIT\" > config.txt\n\
         cp \"$ELFIE_MCP_CONFIG\" written.json",
        &fixture.root,
        "cli/main",
        "",
        binding,
        None,
        None,
    );
    assert_eq!(exit.code, 0, "{}", exit.stderr);
    let written: Vec<String> = fixture.read("config.txt").lines().map(str::to_string).collect();
    // @lfy def/cli/main.lfy:main#main:main:3bba760addbbe89f0bc597ff49d70e040b3328497641cb4a22a68f53d4b68585
    assert_eq!(written[2], exit.session, "the run was told its own session id");
    assert!(is_a_uuid(&exit.session), "{} is no version 4 UUID", exit.session);
    // @lfy def/cli/main.lfy:main#main:main:631eb66f63680435474026b050f06277c5eee4e5b444e767c71386bdbb3c3118
    assert_eq!(written[3], "none", "ELFIE_RESUME was in the environment");
    // @lfy def/cli/main.lfy:main#main:main:7522539013e41173fe7dcc12bb506b34925d3a1de610ef78fe9a1db5ea960f10
    assert_eq!(written[4], "cli/main");
    let session = {
        let opened = recorded.opened.lock().unwrap();
        assert_eq!(opened.len(), 1);
        // @lfy def/cli/main.lfy:main#main:main:6d22af1c9b1faa80650ee222bf29ec59916fad170472c864b3ea67c7206c69c9
        assert_eq!(opened[0].role, Role::Compiler);
        assert_eq!(opened[0].unit, Some(0));
        // @lfy def/cli/main.lfy:main#main:main:2992ce86fed8f5b6e58cb53a172e157d0398241d6fb037edd64df68376cd8d80
        assert_eq!(opened[0].directory, fixture.root);
        assert_eq!(opened[0].plan, run.plan);
        assert_eq!(opened[0].program, run.program);
        assert_eq!(opened[0].request.as_ref().map(|request| request.instructions.clone()), Some(request.instructions.clone()));
        assert_eq!(opened[0].before, Some(before));
        assert_eq!(opened[0].review, None);
        assert_eq!(opened[0].source_maps, []);
        "http://127.0.0.1:7/sessions/1".to_string()
    };
    // The config file is named by the unit's request name and the role.
    // @lfy def/cli/main.lfy:main#main:main:5b2c8910206c8e75afcd2cd352436a6895414fa9284039b11ef9f9b3c1fb88bb
    let path = fixture.root.join("elfie-requests/cli-main.compiler.mcp.json");
    assert_eq!(written[0], path.to_string_lossy());
    // @lfy def/cli/main.lfy:main#main:main:68d7f7104e4c1d779c0c94bfc97e19bb4af58091b1995f477d4af9494dc91e7e
    assert_eq!(written[1], "elfie_test elfie_build elfie_lint");
    // @lfy def/cli/main.lfy:main#main:main:44a95955daee5d89e4435d08b9cc7de1e517c400cca66ecc9ac0f0aecd83045b
    let text = fixture.read("written.json");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(text, value.to_string());
    let server = &value["mcpServers"]["elfie"];
    assert_eq!(server["type"], "http");
    assert_eq!(server["url"], session);
    assert_eq!(server["headers"]["Authorization"], "Bearer a-token");
    // @lfy def/cli/main.lfy:main#main:main:d5aa2bf16f2da120b4d37b7d66b8856800a80b580034d60d8ee7a8967d232f02
    assert!(!path.exists(), "the config file is still there");
    // @lfy def/cli/main.lfy:main#main:main:b4fcd8255dc77a6812ba729d68dac80c3c551fa5f203c2d68df2bab37b05ee57
    assert_eq!(*recorded.closed.lock().unwrap(), vec![session]);
    assert_eq!(exit.record.reviews, vec![submitted]);
    assert_eq!(exit.record.submission.map(|submission| submission.outcome), Some(SubmittedOutcome::Done));
    // @lfy def/cli/main.lfy:main#main:main:77f1f3a8ea3c2d7f433ee642b0418c00d1ce2b7dd1dca0106b299cb12959c12b
    agent.server.stop();
    agent.server.stop();
    assert_eq!(*recorded.stopped.lock().unwrap(), 1);
}

/// How a compiler run ended is read from the outcome it submitted through its session, not
/// from its report: a run that submitted `blocked` is blocked, with the submission's message
/// as its reason, though its report says nothing at all.
// @lfy def/cli/main.lfy:main#main:main:f8eef3f6a490308c065b3e1e9de537e4ca8b958f1a91454d15e2678950511b8b
// @lfy def/cli/main.lfy:main#main:main:758fa1871b09f337b51e44d8a8f6104962caf305712f1929d105dc578aa66787
#[test]
fn the_outcome_of_a_compiler_run_is_read_from_what_it_submitted() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let mut run = a_run(&fixture, &["compile", "--color=never"]);
    let record = SessionRecord {
        submission: Some(Submission { outcome: SubmittedOutcome::Blocked, message: "two criteria of A conflict".to_string() }),
        ..SessionRecord::default()
    };
    let (host, _recorded) = a_host(record, Vec::new());
    run.agent = an_agent(&fixture.root, Ok(host));
    let root = fixture.root.clone();
    // @lfy def/cli/main.lfy:main#main:main:f8eef3f6a490308c065b3e1e9de537e4ca8b958f1a91454d15e2678950511b8b
    assert_eq!(run.work_unit(0, "true", &root), Ended::Stopped(Stop::Blocked));
    let note = fixture.read("elfie-requests/a.blocked.md");
    assert!(note.contains("two criteria of A conflict"), "{note}");
}

/// The verifier of a unit is bound to the review request it was given and the source maps
/// that request was made with; the verifier of the global review to no unit.
// @lfy def/cli/main.lfy:main#main:main:a4c3ad2a056f59fac7b6a9db55c7289d1c10eb4f174437ea829a0b82e2d58f92
// @lfy def/cli/main.lfy:main#main:main:eae2a79c9a6dc51a5daba684245211bdf83b66ea0d0b34fbdc86bb68df82a42d
#[test]
fn a_verifier_is_bound_to_the_review_it_was_given() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {\n  @acceptanceCriteria\n    .add({ behavior = `Counts the list` });\n}\n");
    let run = a_run(&fixture, &["compile"]);
    let maps = generation::source_maps_of(&run.program.workspace, None);
    let request = run.review_request_of(0, &maps, &fixture.root, &[]);
    // @lfy def/cli/main.lfy:main#main:main:a4c3ad2a056f59fac7b6a9db55c7289d1c10eb4f174437ea829a0b82e2d58f92
    let binding = run.binding_of(Role::Verifier, Some(0), &fixture.root, None, None, Some(request.clone()), maps.clone());
    assert_eq!(binding.role, Role::Verifier);
    assert_eq!(binding.unit, Some(0));
    assert_eq!(binding.before, None);
    assert_eq!(binding.review.as_ref().map(|review| review.asked.clone()), Some(request.asked.clone()));
    assert_eq!(binding.source_maps, maps);
    // @lfy def/cli/main.lfy:main#main:main:eae2a79c9a6dc51a5daba684245211bdf83b66ea0d0b34fbdc86bb68df82a42d
    let global = generation::global_review(&run.program, &maps);
    let binding = run.binding_of(Role::Verifier, None, &fixture.root, None, None, Some(global), maps.clone());
    assert_eq!(binding.unit, None);
    assert!(binding.review.is_some());
    assert_eq!(binding.source_maps, maps);
}

/// An agent server that cannot be hosted runs no compiler and no verifier: the run's code is
/// -1 and its failure names the server.
// @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
#[test]
fn a_server_that_cannot_be_hosted_runs_nothing() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let mut run = a_run(&fixture, &["compile"]);
    let agent = an_agent(&fixture.root, Err("no port of 127.0.0.1 can be listened on".to_string()));
    let binding = run.binding_of(Role::Compiler, Some(0), &fixture.root, None, None, None, Vec::new());
    // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    let exit = agent.run("echo ran > ran.txt", &fixture.root, "a", "", binding, None, None);
    assert_eq!(exit.code, -1);
    assert!(!fixture.root.join("ran.txt").exists(), "the command ran");
    assert!(exit.stderr.contains("the agent server could not be hosted"), "{}", exit.stderr);
    assert!(!fixture.root.join("elfie-requests/a.compiler.mcp.json").exists(), "a session was opened");
    // A compile whose server cannot be hosted stops, and its code is failure.
    // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    run.agent = agent;
    let root = fixture.root.clone();
    assert_eq!(run.work_unit(0, "echo ran > ran.txt", &root), Ended::Stopped(Stop::Failed));
    assert!(run.halted);
    assert_eq!(run.code, ExitCode::Failure);
    assert!(!fixture.root.join("ran.txt").exists(), "the command ran");
}

/// A global review whose agent server cannot be hosted gives no report and writes none: the
/// run stops, and its code is failure.
// @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
#[test]
fn a_global_review_whose_server_cannot_be_hosted_fails() {
    let fixture = Fixture::new();
    one_target(&fixture).write("def/a.lfy", "d A: `An A` {}\n");
    let mut run = a_run(&fixture, &["compile"]);
    run.agent = an_agent(&fixture.root, Err("no port of 127.0.0.1 can be listened on".to_string()));
    let request = generation::global_review(&run.program, &[]);
    let root = fixture.root.clone();
    // @lfy def/cli/main.lfy:main#main:main:c98cf702185cba4a24169527abed59a142c455d479534531109ad7041e58476c
    assert!(run.ask_globally("echo ran > ran.txt", &root, &request, &[]).is_none());
    assert!(run.halted);
    assert_eq!(run.code, ExitCode::Failure);
    assert!(!fixture.root.join("ran.txt").exists(), "the command ran");
    assert!(!fixture.root.join("elfie-requests/global.reviews.json").exists(), "a review was written");
}

/// A reviewed line counts the reviews of its own run and then the verdicts its review
/// request carried, which are counted there and nowhere else; with nothing carried the counts
/// end with the unverifiable.
// @lfy def/cli/main.lfy:main#main:main:d51d2b775176b8280c71a2ed87d370cbeb087b43471ac01fd88baa7509973bd3
// @lfy def/cli/main.lfy:main#main:main:79aeca787363a11f87e229a581c29dfcab7548d25cd5e7e3018983a54dbcb14d
// @lfy def/cli/main.lfy:main#main:main:42cb7931abf1b1cbc69474e94e4981f4280630610c6622305d2f9ce3dab316d1
// @lfy def/cli/main.lfy:main#main:main:01ac2a8e3e988e7c07b647cd40cc38bce72044f98971d9e5e72eb5c62425cc0e
// @lfy def/cli/main.lfy:main#main:main:0ae375c1084691369dda163c81e2e1f335de4780dfb63290fe31e727ca79fec3
// @lfy def/cli/main.lfy:main#main:main:e16c3a64811e996470b960cf5e81ef245970ea83b63e901e2b4707167e69d564
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
    // @lfy def/cli/main.lfy:main#main:main:42cb7931abf1b1cbc69474e94e4981f4280630610c6622305d2f9ce3dab316d1
    assert_eq!(counts_message(&counts_with_carried(&reviews, 0, 0), false), "1 satisfied, 0 violated, 0 unverifiable");
    // @lfy def/cli/main.lfy:main#main:main:79aeca787363a11f87e229a581c29dfcab7548d25cd5e7e3018983a54dbcb14d
    let counts = counts_with_carried(&reviews, 0, 2);
    assert_eq!(counts_message(&counts, false), "1 satisfied, 0 violated, 0 unverifiable, 2 carried");
    // @lfy def/cli/main.lfy:main#main:main:01ac2a8e3e988e7c07b647cd40cc38bce72044f98971d9e5e72eb5c62425cc0e
    assert_eq!(counts.last(), Some(&(2, "carried", Tone::Muted)));
    assert_eq!(width(&counts_message(&counts, true)), width(&counts_message(&counts, false)));
    assert!(counts_message(&counts, true).contains(&tally(2, "carried", Tone::Muted, true)));
    // @lfy def/cli/main.lfy:main#main:main:0ae375c1084691369dda163c81e2e1f335de4780dfb63290fe31e727ca79fec3
    assert_eq!(counts_message(&counts_with_carried(&[], 0, 3), false), "0 satisfied, 0 violated, 0 unverifiable, 3 carried");
}

/// A compile records the verdict on each criterion and test of its units, and a review of a
/// unit whose verdicts all still hold asks about nothing: no verifier runs on it, and --full
/// asks about them again.
// @lfy def/cli/main.lfy:main#main:main:05ecd897c6044e88ca2bad408d90091093bb8d358a4cb29d412daaab6695e86c
// @lfy def/cli/main.lfy:main#main:main:974936f3c3946711196e8a624f7b7a1eef10cd5508bc5dc018f39ef22112a2df
// @lfy def/cli/main.lfy:main#main:main:e589c1fc483c3f78252a097b7894875817fba5591db7ee8b3f20c193b5d5dbd9
// @lfy def/cli/main.lfy:main#main:main:8b2d4cb145c85977997b07a3824a4854f7eecccbcae9af2711ec792f26fdb7a9
// @lfy def/cli/main.lfy:main#main:main:1210eb92f28da423c86881acae7b44dcc7eb7346006e861a85cef47a910c5f8f
#[test]
fn a_settled_verdict_is_carried_and_asks_no_verifier() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    // @lfy def/cli/main.lfy:main#main:main:05ecd897c6044e88ca2bad408d90091093bb8d358a4cb29d412daaab6695e86c
    let records = fixture.read("elfie-compile/reviews/rust/a.json");
    assert!(records.contains(&criterion_id(&fixture)), "{records}");
    assert!(records.contains("satisfied"), "{records}");
    // @lfy def/cli/main.lfy:main#main:main:1210eb92f28da423c86881acae7b44dcc7eb7346006e861a85cef47a910c5f8f
    assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "verifications.txt"), 1, "the verifier ran again");
    // @lfy def/cli/main.lfy:main#main:main:8b2d4cb145c85977997b07a3824a4854f7eecccbcae9af2711ec792f26fdb7a9
    assert_eq!(fixture.run(&["verify", "a", "--full"]), ExitCode::Success.code());
    assert_eq!(lines_of(&fixture, "verifications.txt"), 2, "the verifier did not run again");
    // @lfy def/cli/main.lfy:main#main:main:e589c1fc483c3f78252a097b7894875817fba5591db7ee8b3f20c193b5d5dbd9
    let mut run = a_run(&fixture, &["verify"]);
    let maps = generation::source_maps_of(&run.program.workspace, None);
    let root = fixture.root.clone();
    let kept = run.records_of(0, &root);
    assert_eq!(kept.len(), 1, "{kept:?}");
    let request = run.review_request_of(0, &maps, &root, &kept);
    assert!(request.asked.is_empty(), "{:?}", request.asked);
    assert_eq!(request.carried.len(), 1, "{:?}", request.carried);
    let full = run.review_request_of(0, &maps, &root, &[]);
    assert_eq!(full.asked.len(), 1, "{:?}", full.asked);
}

/// With --no-verify, with no verifier named, or with --accept, no review record file
/// changes.
// @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
// @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
// @lfy def/cli/main.lfy:main#main:main:dfb6407e51bd4f65d463919d8814abd45313c5c1939bb539e6352a60cf287078
#[test]
fn nothing_that_runs_no_verifier_records_a_verdict() {
    let quiet = Fixture::new();
    a_compiler_and_a_verifier(&quiet, SATISFIED);
    assert_eq!(quiet.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    assert!(quiet.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    // @lfy def/cli/main.lfy:main#main:main:6996e96f76cb2583f243492586dabf52cc59259bb5c5815b454493f77b21594d
    assert!(!quiet.root.join("elfie-compile/reviews").exists(), "a review record was written");
    let plain = Fixture::new();
    a_compiler_and_a_verifier(&plain, SATISFIED).write("elfie.json", &manifest(&[("compiler", "sh compiler.sh")]));
    assert_eq!(plain.run(&["compile"]), ExitCode::Success.code());
    assert!(!plain.root.join("elfie-compile/reviews").exists(), "a review record was written");
    // With --accept the maps are recorded but no review record file changes.
    // @lfy def/cli/main.lfy:main#main:main:6fc3e455a00e35d03fefc6334b35ff96b50c1c9069d3efe7e721a45743f92e1b
    let accepted = Fixture::new();
    a_compiler_and_a_verifier(&accepted, SATISFIED);
    accepted.write("out/a.rs", "// @lfy def/a.lfy:A\npub struct A {}\n");
    assert_eq!(accepted.run(&["compile", "--accept"]), ExitCode::Success.code());
    assert!(accepted.read("elfie-compile/maps/rust/a.json").contains("\"out/a.rs\""));
    assert!(!accepted.root.join("elfie-compile/reviews").exists(), "a review record was written");
}

/// A review record file belonging to no unit of the plan is removed in the target's working
/// directory before any unit runs, which the root then takes, and the file of a unit of the
/// plan is left as it was.
// @lfy def/cli/main.lfy:main#main:main:ea7ff5171f6e9c135b3736f46e4d6878cfe8097bd5ca7b45686d9b57a0e0cfb7
#[test]
fn a_review_record_file_of_no_unit_is_removed() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    let stale = fixture.read("elfie-compile/reviews/rust/a.json");
    fixture.write("elfie-compile/reviews/rust/gone.json", &stale);
    fixture.write("def/b.lfy", "d B {}\n");
    fixture.write("compiler.sh", &per_unit_compiler(&fixture));
    write_verifier(&fixture, SATISFIED);
    // @lfy def/cli/main.lfy:main#main:main:ea7ff5171f6e9c135b3736f46e4d6878cfe8097bd5ca7b45686d9b57a0e0cfb7
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert!(!fixture.root.join("elfie-compile/reviews/rust/gone.json").exists(), "the review record file stayed");
    assert_eq!(fixture.read("elfie-compile/reviews/rust/a.json"), stale, "a.json changed");
    // With --no-verify nothing of the kind is removed, since nothing reviews anything.
    fixture.write("elfie-compile/reviews/rust/gone.json", &stale);
    fixture.write("def/c.lfy", "d C {}\n");
    assert_eq!(fixture.run(&["compile", "--no-verify"]), ExitCode::Success.code());
    assert!(fixture.root.join("elfie-compile/reviews/rust/gone.json").is_file(), "the file was removed");
}

/// A verify whose review request asked nothing is success, as one whose verifier was started
/// and violated nothing is.
// @lfy def/cli/main.lfy:main#main:main:c3a2ad3cc8b8443eed86ed501261573e2799abb00383db190b158541be935033
// @lfy def/cli/main.lfy:main#main:main:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
#[test]
fn a_verify_that_asked_nothing_is_success() {
    let fixture = Fixture::new();
    a_compiler_and_a_verifier(&fixture, SATISFIED);
    assert_eq!(fixture.run(&["compile"]), ExitCode::Success.code());
    assert_eq!(fixture.run(&["verify", "a"]), ExitCode::Success.code());
    assert_eq!(fixture.run(&["verify", "a", "--full"]), ExitCode::Success.code());
    write_verifier(&fixture, VIOLATED);
    // @lfy def/cli/main.lfy:main#main:main:e66e42fc4344ab7898da31692b13cfa9d1621e6c695a0226fd0e42bb5522bdd4
    assert_eq!(fixture.run(&["verify", "a", "--full"]), ExitCode::Problems.code());
}
