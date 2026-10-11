// @lfy def/cli/main.lfy:main
//! Tests of how many batches run at once and of the order a round starts them in.

use super::*;
use crate::commands::JOBS_USAGE;
use crate::compiling::compile_jobs;
use crate::copies::Where;
use crate::schedule::{Done, Next, Schedule};

/// --jobs takes the value that follows it or the one after its equals sign, and anything
/// but a whole number of at least 1 is a usage message naming it.
// @lfy def/cli/main.lfy:parse#parse:parse:8a7c0ab826525b9fea70ea111a51d4e84e153e8e61475a1c4494148b8243fc71
// @lfy def/cli/main.lfy:parse#parse:parse:252d8235d06b32c568f2bf1a13157b65c1788281bf471f3abe705c298312086c
// @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
#[test]
fn jobs_takes_a_whole_number_of_at_least_one() {
    // --jobs followed by a value gives that value, as --target does.
    // @lfy def/cli/main.lfy:parse#parse:parse:252d8235d06b32c568f2bf1a13157b65c1788281bf471f3abe705c298312086c
    let invocation = parse_arguments(&args(&["compile", "--jobs", "2", "lexer/main"])).unwrap();
    assert_eq!(invocation.option("jobs"), Some("2"));
    assert_eq!(jobs_of(&invocation), 2);
    // The value it took is no positional argument.
    // @lfy def/cli/main.lfy:parse#parse:parse:7473c896dd710a7b0dcccc8450958126fa8e9c70c04172f551e9a551ff75afa3
    assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
    assert_eq!(jobs_of(&parse_arguments(&args(&["compile", "--jobs=3"])).unwrap()), 3);
    // 0 is no whole number of at least 1, and neither is anything that is no number.
    // @lfy def/cli/main.lfy:parse#parse:parse:1c1c05d13f47e70159eab3d4e74654eaa380270fc0a4188a73838e3b7cb66082
    // @lfy def/cli/main.lfy:parse#parse:parse:8a7c0ab826525b9fea70ea111a51d4e84e153e8e61475a1c4494148b8243fc71
    let error = parse_arguments(&args(&["compile", "--jobs", "0"])).unwrap_err();
    assert_eq!(error, JOBS_USAGE);
    assert!(error.contains("--jobs"), "{error}");
    for arguments in [
        args(&["compile", "--jobs=0"]),
        args(&["compile", "--jobs=some"]),
        args(&["compile", "--jobs", "-1"]),
        args(&["compile", "--jobs", "1.5"]),
    ] {
        assert_eq!(parse_arguments(&arguments).unwrap_err(), JOBS_USAGE, "{arguments:?}");
    }
}

/// With no --jobs given, four batches run at once.
// @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
#[test]
fn without_jobs_four_batches_run_at_once() {
    // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
    assert_eq!(JOBS_BY_DEFAULT, 4);
    assert_eq!(jobs_of(&parse_arguments(&args(&["compile"])).unwrap()), 4);
}
/// A batch starts once every batch holding a dependency of one of its units is done, at
/// most --jobs run at once, and among those that can start they start in plan order.
// @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
#[test]
fn a_batch_starts_once_what_it_waits_for_is_done() {
    let planned =
        vec![a_planned_batch("a", 0, vec![]), a_planned_batch("b", 1, vec![]), a_planned_batch("c", 2, vec![0])];
    let mut schedule = Schedule::of(&planned);
    // Among those that can start, they start in plan order.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(0)));
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(1)));
    // At most --jobs run at once.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Wait));
    // c waits on a, which is still running.
    // @lfy def/cli/main.lfy:main#main:main:518ea127e00fbf573d96ad491ab3cff1b62b40c65038ea5e89ce53d0dfcd3136
    schedule.finish(1, &Done::Merged);
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Wait));
    schedule.finish(0, &Done::Merged);
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(2)));
    schedule.finish(2, &Done::Merged);
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Over));
}

/// The compile stopped: no batch starts after that, and the batches already running
/// finish as they would.
// @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
#[test]
fn a_stopped_compile_starts_no_batch() {
    let planned = vec![a_planned_batch("a", 0, vec![]), a_planned_batch("b", 1, vec![])];
    let mut schedule = Schedule::of(&planned);
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(0)));
    // @lfy def/cli/main.lfy:main#main:main:ea0ca180677aa757c8b968ceddc681869b4a24dcf1d091a055556581e265e346
    assert!(matches!(schedule.next(&planned, 2, true, false), Next::Wait));
    schedule.finish(0, &Done::Merged);
    assert!(matches!(schedule.next(&planned, 2, true, false), Next::Over));
    // @lfy def/cli/main.lfy:main#main:main:df6beb5b55efabb4bf57333ff81f1f65c615c2988fc6090856921440a10c3639
    assert_eq!(schedule.unstarted(), vec![1]);
}

/// A batch whose file could not be merged runs again from a new copy once no other batch
/// is running, and nothing else starts while it does.
// @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
#[test]
fn a_conflicted_batch_runs_again_with_nothing_else_running() {
    let planned = vec![a_planned_batch("a", 0, vec![]), a_planned_batch("b", 1, vec![])];
    let mut schedule = Schedule::of(&planned);
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(0)));
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    schedule.finish(0, &Done::Conflicted);
    // It is waiting again, in plan order, and runs with nothing else.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(0)));
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Wait));
    schedule.finish(0, &Done::Merged);
    assert!(matches!(schedule.next(&planned, 2, false, false), Next::Start(1)));
}

/// With --jobs 1, or outside a git repository, batches run one at a time and the batch's
/// directory is the root itself.
// @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
#[test]
fn one_job_or_no_repository_runs_in_the_root() {
    let fixture = Fixture::new();
    let four = parse_arguments(&args(&["compile", "--jobs", "4"])).unwrap();
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    assert_eq!(Where::of(&fixture.root, "a", false).directory(), fixture.root);
    // Outside a git repository --jobs does not hold: one batch runs at a time, in the
    // root itself, so that no two compilers share a tree.
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    if git_directory(&fixture.root).is_none() {
        // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
        assert_eq!(compile_jobs(&four, &fixture.root), 1);
        // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
        assert_eq!(Where::of(&fixture.root, "a", false).directory(), fixture.root);
        // One at a time: with one running, nothing else starts.
        // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
        let planned =
            vec![a_planned_batch("a", 0, Vec::new()), a_planned_batch("b", 1, Vec::new())];
        let mut schedule = Schedule::of(&planned);
        assert!(matches!(schedule.next(&planned, 1, false, false), Next::Start(0)));
        assert!(matches!(schedule.next(&planned, 1, false, false), Next::Wait));
    }
    // In a git repository a batch has its own copy, so --jobs stands as it is given.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    if a_repository(&fixture) {
        // @lfy def/cli/main.lfy:main#main:main:4e8f52e9f77f1f725382edf134fa02a15a8235fbd901842dc1bdf532bb78e75a
        assert_eq!(compile_jobs(&four, &fixture.root), 4);
    }
}
